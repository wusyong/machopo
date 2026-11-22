# ipc

對應 Chromium 的 `ipc/` 和 `mojo/`：machopo 的各個 process 之間共用的東西。renderer 現在還是 Browser process 裡的 thread；現在有每個 process 都讀同一個的時鐘 `TimeTicks`、還在同一個 process 裡的 data pipe（`data_pipe`），channel 的核心（`core`）、訊息的值怎麼變成位元組（`codec`）、把它們組起來的端點（`endpoint`）、同一個 process 裡的平台（`local`），和作業系統的平台層（`platform`：macOS、Linux 和 Windows 的）。

`unsafe` 只放在這裡和 `bindings`（`python3 scripts/sanity.py unsafe` 檢查）：這裡的是作業系統的 API，和對共用記憶體的複製。

## `TimeTicks`

作業系統的單調時鐘的讀數（Chromium 的 `base::TimeTicks`）。Rust 的 `Instant` 讀不出原始值，不能跨 process；`TimeTicks` 可以序列化，不同 process 讀到的時間可以互相比較。它不從特定的時刻算起（多半是開機），只有兩個讀數的差有意義；DevTools 的 `MonotonicTime` 是它換成秒（`as_secs_f64`）。

```rust
let start = ipc::TimeTicks::now();
// ...
let took = ipc::TimeTicks::now().saturating_since(start);
```

- Linux、macOS：`rustix` 的 `clock_gettime(CLOCK_MONOTONIC)`，安全的 API。macOS 上它算進睡眠的時間（Rust 的 `Instant` 用的 `CLOCK_UPTIME_RAW` 不算），兩者不要混著減。
- Windows：`QueryPerformanceCounter`，頻率開機時就固定，第一次讀時記下來。
- `performance.now()` 等網頁看得到的時鐘照舊用 `Instant`（renderer 的 `Clock`）；`TimeTicks` 給要跨 process 對齊的時間：載入的時間（`protocol::network::LoadTiming`）和 DevTools 的事件。

## `data_pipe`

一條單向的 byte 管道（Mojo 的 data pipe，`CreateDataPipe`）：`Producer` 寫、`Consumer` 讀，寫了還沒讀的最多是容量那麼多。回應的 body 從 network service 經過它到用它的地方。

```rust
let (mut producer, mut consumer) = ipc::data_pipe::create(512 * 1024);
// network service（tokio）：滿了就等，consumer 不在了回 `Closed`。
producer.write(chunk).await?;
drop(producer); // 結尾
// 讀的一側：不等待，沒有資料時回 `Empty`。
consumer.watch(move || post_task(Task::BodyReadable));
match consumer.read(64 * 1024) {
    Read::Data(bytes) => { /* 照寫進來的順序 */ }
    Read::Empty => { /* 等 watch 的通知 */ }
    Read::Done => { /* 讀完了，producer 也不在了 */ }
}
```

- **上限照 byte 嚴格守住**：一段比剩下的空間大時，先放進放得下的（`Bytes::split_to`，不複製），其餘的等讀走再放；容量至少 1。
- **寫是 async、讀不等待**：寫的一側在 network service 的 tokio 上，背壓就是 `write` 在那裡等；讀的一側是有自己的 event loop 的 thread（renderer 的 main thread），不放 executor：`read` 回現在有的，`watch` 給的 callback 在管道變成可讀時被呼叫一次（Mojo 的 `SimpleWatcher`），由它排一個 task 來讀。
- **`watch` 什麼時候呼叫**：開始 watch 時已經可讀（有資料、或 producer 已經不在）就當場呼叫；之後每次 `read` 回 `Empty`，下一次寫或 producer 的結束呼叫一次（在 producer 的 thread 上）。讀到 `Empty` 或 `Done` 才要求下一次通知，讀了自己那一份就停的 task 要自己排下一個。callback 在放開鎖之後呼叫。
- **兩端的結束**：`Producer` drop 是結尾，consumer 讀完剩下的才是 `Done`；`Consumer` drop 時沒讀的丟掉，producer 的 `write`（等著的也是）回 `Closed`。
- **不等的寫**：`Producer::try_write` 整段寫進去或完全不寫（`Full`、`Closed`），不等。給「事先被告知有多少空間」的寫的一端用：跨 process 的管道收的那一側，送的一端守著容量，所以 `Full` 就是對方的錯。
- **`Consumer` 放在訊息裡就跟著訊息過通道**（Mojo 的訊息裡的 handle）：`Consumer` 能序列化，但只有這個 crate 的 `codec` 做得完。序列化時它把管道從值裡取走（留下的值讀起來是已經結束的管道，drop 不關任何東西）；同一個值序列化第二次是錯誤。別的格式（JSON、純 postcard）序列化它會被拒絕，管道還給那個值；反過來也只有從 `ipc` 的訊息裡讀得出來。做法是一個保留的名字（`$ipc::data_pipe::Consumer`）讓 codec 認出它，管道本身在那一次呼叫裡經過一個 thread-local 交到 codec 手上（serde 的資料模型裡放不了一條管道）；別的型別用了那個名字拿不到管道。`Consumer::capacity()` 是它的容量。
- **讀了多少的掛勾**：`Consumer::observe` 給一個 `ReadObserver`，之後每次讀到資料、讀到結尾都告訴它，`Consumer` drop 時它跟著 drop。跨 process 的管道收的那一側用它回報和取消；單一 process 時不掛。
- **單一 process 的實作**：一把 `Mutex` 裡的 `Bytes` 佇列，加 tokio 的 `Notify` 喚醒等空間的 producer（兩個 thread 真的共用，所以 `Arc`）；寫進來的片段原樣讀出、不複製。拆 process 時管道本身不變、兩個 process 各一條：Browser 那一側的讀出來、一段一段經過控制通道送過去，寫進 renderer 那一側的；renderer 回報讀了多少，Browser 送出而還沒被讀的不超過容量。

## `core`

channel 的核心（Chromium 的 `mojo/core`，不含它的 `mojo/core/platform`）：channel 拿平台那一層讀到、要寫的位元組和 handle 做的事。它不做系統呼叫、不分平台（`python3 scripts/sanity.py deps` 檢查 `src/core/` 裡沒有提到做系統呼叫的 crate、std 裡碰系統的部分、FFI 和分平台的 `cfg`）：位元組就是位元組，handle 是平台說的任何型別（泛型的 `H`），所以三個平台用同一份，整個能在 Miri 和 fuzzer 底下跑。

### `core::frame`：一則訊息的位元組

開頭固定 20 bytes，little-endian，後面是內容：

| 位元組 | 欄位 |
| --- | --- |
| 0..4 | 內容的長度，最多 `MAX_PAYLOAD_LEN`（128 MiB） |
| 4..8 | 目的地的編號 |
| 8..16 | 序號：每一端從 0 開始、每則加一 |
| 16..20 | 跟著這則訊息的 handle 有幾個，最多 `MAX_HANDLES`（16） |

```rust
// 送的一端：編號、寫開頭和內容；handle 由平台那一層跟著送。
let mut encoder = ipc::core::frame::Encoder::new();
encoder.encode(destination, &payload, handles.len(), &mut out)?;

// 收的一端：平台那一層讀到什麼就餵什麼，不管它怎麼切。
let mut decoder = ipc::core::frame::Decoder::new(peer_may_attach);
decoder.feed(&bytes, handles, |message| {
    // message.destination、sequence、payload（&[u8]，還在讀進來的地方）、handles
})?;
```

- **怎麼切都一樣**：socket 是串流、一次讀到的可以是半則或好幾則，Mach 訊息一則就是一則，都走 `feed`。整則到齊才交出去；讀進來的那一塊裡完整的訊息直接從那一塊交出去、不複製，只有沒到齊的開頭留在 `Decoder` 裡。
- **開頭一到就檢查**，不等內容：長度超過上限、序號不是下一個、沒有權限的一端說它帶了 handle、handle 超過上限，都是 `BadMessage`。
- **不照開頭寫的長度配置**：留著的位元組只跟真的到了的一樣多，說自己有 128 MiB 而只送 10 bytes 的對端只花掉 10 bytes。
- **handle**：跟著一則訊息的第一個位元組讀到，照讀到的順序給訊息。對方那一端沒有附 handle 的權限（`peer_may_attach` 是 `false`）時，任何 handle 都是壞訊息，不管開頭有沒有說；訊息沒認領的 handle 最多留兩則訊息的份，再多也是壞訊息。
- **`BadMessage` 之後**什麼都不留、什麼都不再收：留著的位元組和 handle 當場丟掉（handle 的 `Drop` 就是關掉它），之後每次 `feed` 都回同一個錯誤。上層看到它就關掉通道。
- `Encoder` 擋掉對方會拒絕的（太長、太多 handle），被擋掉的不佔序號。
- 目的地認不認得、送出的佇列、斷線，不在這一層，在 `core::channel`。

### `core::channel`：通道的兩側

Chromium 的 `mojo::core::Channel`。底下搬位元組的東西是 `Transport`（平台那一層實作它）：

```rust
pub trait Transport {
    type Handle;
    /// 不等。寫 `bytes` 的開頭（一則訊息剩下的部分），回答收了多少：
    /// 全部、一部分（串流），或 `WouldBlock`。handle 跟著第一個位元組走。
    fn write(&self, bytes: &[u8], handles: &mut Vec<Self::Handle>) -> Result<usize, WriteError>;
}

let (sender, mut receiver) = ipc::core::channel::channel(transport, config);
// 這一端有事要 IO thread 做時呼叫它（讓那條 thread 之後呼叫 `Receiver::woken`）。
sender.on_wake(wake);

// 任何 thread：不等。
sender.send(destination, &payload, handles)?;

// 讀傳輸的那一條 thread（IO thread）的迴圈：
if let Some(gone) = receiver.feed(&bytes, handles, |message| dispatch(message)) { /* 通道結束了 */ }
if let Some(gone) = receiver.peer_gone() { /* 平台說對方不在了 */ }
if let Some(gone) = receiver.woken() { /* 被叫醒之後：別的 thread 關掉了通道 */ }
sender.flush(); // 傳輸又有空間時
```

- **送不等**：傳輸收得下就當場寫；收不下的（或前面已經有在等的）排進佇列，照順序。一把鎖守著訊息的編號和寫出去的順序，所以好幾條 thread 同時送時，線上的序號還是連續的、一則訊息不會被另一則插進中間。一次 `write` 只給一則訊息的位元組：Mach 訊息那種「整則或沒有」的傳輸不會拿到半則或兩則。
- **佇列有上限**（`MAX_QUEUED_BYTES`，256 MiB，兩則最大的訊息）：等著的位元組超過它，就是對方不讀了，通道關掉（`Gone::QueueFull`），等著的連同 handle 丟掉。對方「多久沒回應」不是這一層的事。
- **叫醒 IO thread 的 callback 由通道自己拿著**（`Sender::on_wake` 給它，還沒給的時候叫醒沒有作用），傳輸不用知道：佇列從空變成有東西時呼叫一次（IO thread 開始等傳輸有空間，有了就 `flush`），關掉通道時也呼叫；上面一層有事要那條 thread 做時用 `Sender::wake`。一律在放開鎖之後呼叫。
- **收的一側**：`feed` 把到齊的訊息照順序交給 `sink`；目的地不在 `Config::destinations` 以內的、`sink` 回 `BadMessage::Rejected` 的（內容解不出來）、`core::frame` 擋下來的，都是壞訊息，通道關掉，同一批位元組裡後面的不交出去。
- **結束只報告一次**：`feed`、`peer_gone`、`woken` 都可能回 `Some(Gone)`，整條通道只有一次、由 IO thread 拿到；理由是第一個發生的（`PeerClosed`、`BadMessage`、`QueueFull`、這一端自己的 `Closed`）。報告之後不再交出訊息，讀到一半的訊息和它的 handle 當場丟掉；`send` 從通道關掉那一刻起就回 `SendError::Gone`。
- **這一端自己的錯不關通道**：太長、太多 handle（`SendError::Encode`）、沒有權限卻附了 handle（`HandlesNotAllowed`），什麼都不送、不佔序號。
- 組訊息的緩衝區留著給下一則用，超過 64 KiB 的不留。
- 同步呼叫等回答的位置不在這裡：它在 renderer 的 scheduler 裡。

### `core::region`：共用記憶體

對核心來說，一塊 region 是建立它的一端給的編號，加一段記憶體（`Memory`：指標和長度，哪個平台怎麼 map 的它不知道）。對應 Chromium 的 `BigBuffer` 底下的 `base::*SharedMemoryRegion`；不同的是 Chromium 每筆資料新建一塊，這裡留著重複用。

```rust
// 送的一端：這一端寫進去的 region。一塊一次只放一筆資料。
let mut pool = ipc::core::region::Pool::new(limit);
pool.add(number, memory)?;                  // 長度要是 class_len 給的那幾種
let number = pool.put(&bytes);              // 最短的放得下的空塊；沒有就是 None
pool.released(number)?;                     // 對方說它讀完了

// 收的一端：對方寫進去的 region，照編號查。
let mut table = ipc::core::region::Table::new();
table.insert(number, memory);
table.read(number, len, &mut into)?;        // 檢查、複製出來
let read = table.take_read();               // 讀過的，要告訴對方的
```

- **一筆資料用一整塊**：訊息裡提到 region 時只有編號和長度，沒有位置。region 的長度是 64 KiB 起每次加倍、到 1 GiB 的那幾種（`class_len`）；資料放進最短的放得下的空塊。
- **用著的不會再被寫**：`put`、`take` 之後那一塊是「出去了」，到 `released`（對方的訊息）或 `give_back`（拿走它的人自己還，例如 tile 的像素）才再用。還一塊沒出去的、或不是這個池子的，是壞訊息（`RegionNotOut`、`UnknownRegion`）。
- **收的一端只檢查兩件事**：編號在不在表裡（`UnknownRegion`）、長度有沒有超過那一塊（`OutsideRegion`）。內容一律先複製到自己的記憶體才用：對方還能寫那一塊。
- **map 出來的記憶體不變成 `&[u8]`**：進出都經過指標複製。
- **總量有上限**：`Pool::new(limit)`，超過的 `add` 被拒絕。只有空的那一塊拿得走（`remove`）。
- **交出去和還回來都是訊息**（`core::control` 的 `Granted` 和 `Released`）：共用記憶體裡沒有雙方都寫的狀態，「可以再用了」只經過訊息。
- `HeapMemory`：一般配置的記憶體當成 region，給同一個 process 裡的兩端和測試用（Miri 跑得起來）。

### `core::control`：核心自己的訊息

目的地是 `CONTROL_DESTINATION`（上層沒有東西在那裡）。位元組是手排的：一個位元組的種類，後面是 little-endian 的欄位，核心不用序列化的程式就讀得懂對方送的。

| 訊息 | 內容 | 意思 |
| --- | --- | --- |
| `Released` | 一串 region 的編號 | 這些讀完了，可以再寫（一批一則） |
| `Granted` | 編號、長度、給對方寫的或讀的 | 一塊新的 region，handle 跟著這一則 |
| `PipeOpened` | 管道的編號、容量 | 送的一端要寫的一條新管道 |
| `PipeData` | 管道的編號、位元組 | 管道接下來的一段 |
| `PipeRead` | 管道的編號、數量 | 讀的一端又讀了這麼多 |
| `PipeEnded` | 管道的編號 | 寫的一端寫完了 |
| `PipeCancelled` | 管道的編號 | 讀的一端不要了 |

`encode` 沒有寫過的內容（多一個位元組、少一個、不認得的種類）都是壞訊息。`PipeData` 的位元組解出來還在讀進來的地方，不複製。收到之後做什麼在 `endpoint`。

### `core::pipe`：過通道的 data pipe

Mojo 的 data pipe 是共用記憶體裡的 ring；這裡管道的位元組是訊息。兩端各有一半：

```rust
// 寫的一端
let mut pipes = ipc::core::pipe::Outgoing::new();
let pipe = pipes.open(capacity)?;      // 編號；送 PipeOpened
let room = pipes.room(pipe);           // 現在可以送幾個位元組（一則最多 64 KiB）
pipes.sent(pipe, len);                 // 送 PipeData
pipes.peer_read(pipe, count)?;         // 收到 PipeRead
pipes.peer_cancelled(pipe)?;           // 收到 PipeCancelled
pipes.end(pipe);                       // 送 PipeEnded

// 讀的一端
let mut pipes = ipc::core::pipe::Incoming::new();
pipes.opened(pipe, producer)?;         // 收到 PipeOpened；producer 是這一側的 data_pipe
pipes.get(pipe)?;                      // 收到 PipeData：Some 就 try_write
pipes.ended(pipe)?;                    // 收到 PipeEnded
pipes.cancel(pipe);                    // 這一側不要了；送 PipeCancelled
let mut report = ipc::core::pipe::ReadReport::new(capacity);
if let Some(count) = report.read(n) { /* 送 PipeRead */ }
```

- **額度**：寫的一端送出去、對方還沒說讀了的量不超過容量。對方回報的比送的多是壞訊息（`PipeOverRead`）。
- **讀的一端累積到容量的一半才回報**（容量 1 的每個位元組都報）：門檻不超過容量，所以寫的一端額度用完時，讀的一端把手上的讀完一定會回報，不會卡住。
- **讀的一端手上超過容量**是送的一端的錯：收的一側的 `data_pipe` 的 `try_write` 回 `Full`，當成壞訊息（`PipeOverrun`）。
- **編號只會變大、不重複用**，由寫的一端給。靠這一點分辨兩種情況：給「已經結束的管道」的訊息（讀的一端取消時資料還在路上、寫的一端結束時回報還在路上）丟掉就好；給「從來沒開過的管道」的訊息是壞訊息（`UnknownPipe`）。重開一個不比之前都大的編號也是壞訊息（`PipeReopened`）。
- 這兩半接到通道和 `data_pipe` 的那一段在 `endpoint`。

### `core::credit`：管道的額度，Verus 證明過的

`room`（還能送多少）、`sent`（送了之後；超過容量回 `None`）、`read`（對方說讀了之後；比送的多回 `None`）、`threshold`（回報的門檻：容量的一半、至少 1、不超過容量）、`report`（讀了之後要不要回報；每個位元組剛好被回報一次）。

### `core::fit`：池子的算術，Verus 證明過的

`class_len`（一個長度用多大的 region）、`best_fit`（哪一塊空的）、`admits`（加一塊會不會超過上限）。這三個錯了，就是寫超出 mapping、或兩筆資料在同一塊，而測試只有剛好碰到那個長度才看得到。`verus!` 裡面的就是實際跑的程式：平常編的時候規格和證明被拿掉。

- `class_len`：回來的是一種合法的長度、放得下、而且是最小的一種；超過最大的才回 `None`。
- `best_fit`：回來的那一塊是空的、夠長、而且沒有更短的空塊也夠長；回 `None` 時真的沒有任何一塊可以。
- `admits`：就是「總量加這一塊不超過上限」，加法不溢位。

## `codec`

訊息的值和內容的位元組之間（Mojo 產生的序列化程式和它的 message context 的位置）：postcard，外面包一層 serde 的 Serializer 和 Deserializer，其他的值原樣轉過去，只做三件事。

```rust
let mut payload = Vec::new();
ipc::codec::encode(&message, &mut pool, &mut payload)?;     // pool：Place
let message: Msg = ipc::codec::decode(&payload, &mut table)?; // table：Resolve
```

- **大的位元組走 region**：值用 `serialize_bytes` 寫的一段位元組（`protocol::BigBuffer` 就是這樣寫自己的）到 64 KiB（`MIN_REGION_LEN`）時，向 `Place` 要一塊；有的話內容寫進 region，訊息裡只有編號和長度。沒有空塊、或比最大的 region 還長，就照樣放在訊息裡。寫出來的是一個兩種 variant 的 enum（放在訊息裡的、在 region 裡的），所以不走 region 的位元組前面多一個位元組。
- **值裡的管道跟著過去**：序列化到 `data_pipe::Consumer` 時，向 `Place` 開一條過通道的管道（`Place::pipe`），訊息裡只寫它的編號；解碼到編號時向 `Resolve` 要這一端替那條管道建好的 `Consumer`（`Resolve::pipe`）。它在值的哪一層都一樣。這一端不開管道（`Place::pipe` 回 `None`，預設就是）時那個值送不出去；對方寫的編號不是一條等著被拿的管道是 `BadMessage::UnknownPipe`，同一個編號提兩次也是。
- **值寫到一半失敗**：已經放進 region 的還回去，為它開的管道結束掉（`Place::unpipe`；管道已經從值裡取走了，來源跟著放掉），`out` 回到原來的樣子。
- **巢狀的深度有上限**（`MAX_DEPTH`，128）：sequence、map、struct、tuple、enum、option、newtype 各算一層。postcard 自己沒有上限，對方送一個包了幾萬層的值就能讓 stack overflow，那接不住。超過的是 `BadMessage::TooDeep`。
- **不是剛好一個值的內容是壞訊息**：解不出來、後面還有多的位元組（`Rejected`）；提到的 region 不對時回報的是 region 的那個錯誤。
- 沒有 region 的一端用 `NoRegions`：全部放在訊息裡，提到 region 的訊息是壞訊息。
- 池子和表是明確傳進來的參數，不是值的 `serialize` 自己要去哪裡找的狀態。包裝裡的 `RefCell` 是因為 serde 把巢狀的值交給 `Serialize::serialize(&self)`；每一段大的位元組借一次。

## `endpoint`

通道的一端，整個的（Mojo 的 `Channel` 在 IO thread 上做的事，加上 `mojo/core` 為訊息、共用緩衝區和 data pipe 做的）：核心的零件接在一個 `Platform` 上。

```rust
let (link, endpoint) = ipc::endpoint::endpoint(platform, config);

// 任何 thread：不等。
link.send(destination, &message)?;              // 大的位元組走 region
let pipe = link.open_pipe(consumer, capacity)?; // 之後送的訊息裡帶這個編號
link.grant_to_peer(len);                        // 給對方一塊它寫的 region（只有建 region 的那一端）
link.close();

// 一個 process 一條 IO thread，它的迴圈管這個 process 所有的通道。
let (io, handle) = ipc::endpoint::IoLoop::new()?;
std::thread::spawn(move || io.run());
handle.serve(endpoint, handler);   // 任何 thread：把一個端點交給它
handle.stop();                     // 停下來；還在的通道在這一端關掉

impl Handler<P> for MyHandler {
    fn message(&mut self, mut message: Received<'_, P>) -> Result<(), BadMessage> {
        match message.destination() {
            0 => { let m: ToMain = message.decode()?; /* 放進 main thread 的佇列 */ }
            1 => { let m: ToCompositor = message.decode()?; /* ... */ }
            _ => unreachable!(),
        }
        // 訊息裡提到管道時：message.take_pipe(pipe)
        Ok(())
    }
    fn gone(&mut self, gone: Gone) { /* 這條通道結束了，理由；之後不會再被呼叫 */ }
}
```

- **平台的介面是兩個角色**，裡面都沒有判斷：
  - `Platform`（每條通道一個）：`write`（`Transport`）、`map`（對方給的 handle 變成記憶體）、`create`（建一塊 region：這一端的 mapping 和給對方的 handle）、`shut`（通道結束時關掉傳輸，對方的平台才知道）。
  - `Waiter`（每條 IO thread 一個）：`add` 和 `remove`（哪些通道要等）、`interrupter`（別的 thread 打斷它的等待用的）、`wait`（等到有事做：哪一條通道讀到位元組和 handle、又有空間、對方不在了，或是被打斷）。位元組是「借」出來的，平台自己記憶體裡有的（例如 out-of-line 的訊息）不用再複製一次。
- **一個 process 一條 IO thread**：`IoLoop` 等這個 process 所有的通道，來了什麼就推那一條通道的端點一步。Browser 只開一個，所有 renderer 的端點都交給它；child 開一個，裡面只有它那一條。型別不擋一個 process 開兩個（測試會這樣扮演兩端），這是用法上的規定。
- **通道被叫醒**（佇列有東西要寫、管道有資料要送、別的 thread 關了它）：迴圈在端點交給它時把 callback 裝到通道上（核心的 `Sender::on_wake`），callback 記下是哪一條、打斷那條 thread 的等待，迴圈推它一步。這一段不經過平台。還沒交給迴圈的通道叫醒沒有作用；交給迴圈時先推一步，所以之前排著的不會掉。「通道被叫醒」（`Event::Woken`）和「等待被打斷」（`Waited::Interrupted`）是兩件事：後者只是迴圈自己有事（有通道被叫醒、有端點交進來、要停）。
- **迴圈的順序只有一份**（`turn`）：讀到的餵給通道，核心自己的訊息當場處理、其餘的交給 `handler`；這一批讀過的 region 合成一則 `Released`；建好排著要給對方的 region；管道的來源有多少、對方有多少空間就送多少；最後寫出等著的。`run` 就是「等、`turn`」。
- **型別在 IO thread 上解**：`handler` 看目的地決定型別，`decode` 時從對方的 region 複製出來。`decode` 的錯誤傳回去，通道就關掉。
- **先告知、再提到**：一則訊息的大位元組放進 region、新建的 region 的 `Granted`、訊息本身，在同一把鎖底下依序送出，別的 thread 的訊息插不進中間。`open_pipe` 先送 `PipeOpened` 才回傳編號。
- **只有能附 handle 的一端建 region**（`Config::may_attach`，Browser）：兩個方向的 region 都是它編號。它自己送大資料時，沒有空塊就當場建一塊（不超過 `region_limit`）；對方要寫的那幾塊由它用 `grant_to_peer` 給。另一端沒有空塊時大資料就放在訊息裡。
- **送不出去的訊息**（通道已經結束、太長）放進 region 的那幾塊當場還回池子。
- **管道**：來源的 `Consumer` 放在訊息的值裡，`send` 就替它開管道（`PipeOpened` 走在那則訊息前面，同一把鎖底下），容量是那條管道自己的；收的一側 `decode` 出來的值裡就是那條管道的 `Consumer`。訊息送不出去時（通道結束了、佇列滿了），為它開的管道結束、來源放掉、告訴對方（對方把替它留的那一端丟掉）。`open_pipe` 和 `take_pipe` 是同一件事分開做的版本（訊息裡自己帶編號）。來源有資料時叫醒 IO thread；收的一側收到 `PipeOpened` 時建一條 `data_pipe`、掛上回報的掛勾，讀的一端等訊息提到它時由 `take_pipe` 拿走。讀的一端 drop 就是取消。對方不被信任時不讓它開管道（`peer_may_open_pipes`）：每一條都是這一端要留的記憶體。
- **通道結束**：`turn` 回 `Some(Gone)` 一次；管道的來源放掉（它們的寫的一端看到讀的不在了）、這一端留著用來送大資料的 region 都放掉（不等最後一個 `Link` 被丟掉：拿著 `Link` 的人可能留很久）、傳輸關掉，之後 `send` 回 `SendError::Gone`。迴圈告訴那個端點的 `handler`（`gone`）、不再等它；其他的通道照常。迴圈被停下來時，還在的每一條都在這一端關掉、`handler` 收到 `Gone::Closed`。

## `local`

同一個 process 裡的 `Platform`：兩端各在一條 thread 上，一端寫的原樣交給另一端，region 是一般配置的記憶體（`HeapMemory`）。端點的測試和 fuzzer 跑在它上面，Miri 也跑得起來。`pair(room)` 的 `room` 是一端最多堆多少位元組沒讀（測傳輸沒有空間的情況）。`LocalWaiter` 是它的 `Waiter`：等好幾個這樣的一端，輪流看，一條很忙的不會讓別條一直等。`poll` 是不等、只看一端的版本，測試用它一步一步推（叫醒不經過它：手動推的測試自己用 `Link::on_wake` 接）。

## `platform`

每個作業系統為通道做的事（Chromium 的 `mojo/core/platform` 和 `base/memory/platform_shared_memory_region_*`）：搬位元組和 handle、等、建和 map 共用記憶體。裡面沒有判斷，薄到可以逐行看：系統呼叫是工具看不進去的。

host 和 child 怎麼各拿到一端，每個平台都是同樣的三個函式：

```rust
// host
let (child, channel) = ipc::platform::spawn(&mut command)?;   // channel：ipc::platform::Channel
let (link, endpoint) = ipc::endpoint::endpoint(channel, config);
// ...看到 child 結束時：
ipc::platform::child_gone(child.id());

// child
let channel = ipc::platform::inherited()?;
```

- `spawn` 啟動 child，回來 child 和 host 這一端。child 那一端怎麼過去是平台的事：macOS 上 child 繼承不了 port，所以留在這個 process 的 rendezvous server（`Server::global()`）等它來拿；Linux 上 child 繼承那一端的 fd（下面的 `platform::linux::channel`）。`spawn` 用過的 `Command` 不要再拿來啟動別的。
- `child_gone`：child 還沒拿它那一端就結束時，macOS 的 kernel 不會告訴 host（那一端還替它留著），要 host 自己說；說了之後 host 這一端就收到「對方不在了」。拿過的 child、kernel 自己會說的平台（Linux：host 在 child 啟動之後就不拿著它那一端），它什麼都不做。
- **平台要自己守的一條**（`Platform` 的說明）：交給端點的 handle 一定是 kernel 為這則訊息搬進來的，不能是對方寫的數值。handle 以數值走在位元組裡的平台（送的一方把它複製進收的一方），對方不能附 handle 時，那個數值不看、一個 handle 都不交上去，說自己有 handle 的訊息由核心拒絕（壞訊息）：數值指的是這個 process 自己的 handle，而且是對方挑的。

### `platform::macos::memory`

共用記憶體是 Mach 的 memory entry：一個 port 代表那塊記憶體，沒有名字、沒有檔案描述子，也不能改大小。

```rust
// 建的一端寫、對方只能讀（Chromium 的 ReadOnlySharedMemoryRegion）
let (mine, handle) = memory::create(len, false)?;   // mine 可寫；handle 只有讀的權限
// 對方寫（Chromium 的 UnsafeSharedMemoryRegion）
let (mine, handle) = memory::create(len, true)?;    // mine 只 map 成唯讀；handle 可寫
// 收到 handle 的一端
let theirs = memory::map(&handle, len, writable)?;
```

- 只有這兩種。Chromium 的第三種（可寫、能轉成唯讀一次）不做：只有 Browser 建 region，沒有那種轉手。
- **只能讀的 handle** 是對建的一端自己的 mapping 另建的、只有讀權限的 entry；kernel 不讓它 map 成可寫。能寫的那個 right 不離開 `create`。
- **唯讀的 mapping 一直是唯讀的**：map 時最大的保護就是現在的保護，之後改不成可寫。
- **長度要是整頁**（region 的那幾種長度都是）：kernel 會把長度進位，進位過的不是要的那一塊，不建。map 可以少於整塊（它的開頭），不能多。
- `Mapping`：drop 時解除，實作 `Memory`。region 的 handle 是一個 `SendRight`（下面）。

### `platform::macos::port`

這個 task 拿著的 Mach port 的 right，每一種 drop 時放掉：忘了放的 handle 是一個永遠不死的 port，對方就永遠不知道通道結束了。

- `SendRight`：送訊息的對象，也是 region 的 handle。
- `SendOnceRight`：回一次答案用的。沒用就 drop 的話，kernel 會告訴那個 port 的主人「不會有答案了」。
- `ReceiveRight`：一個 port 本身。`new` 建一個、`make_send` 做出通往它的 send right、`tell_when_no_senders` 請 kernel 在沒有 send right 剩下時通知、`raise_queue_limit` 把佇列從 5 則開到 1024 則。drop 時 port 就死了，通往它的 send right 變成 dead name。
- `SendRight` 的兩種通知：`tell_when_dead`（那個 port 死了；對已經死掉的名字登記會失敗）、`tell_when_sendable`（那個 port 的佇列有空間了）。dead-name 的通知會多給那個名字一個計數，`dead_name_told` 放掉它。
- `PortSet`：好幾個 port 當成一個來收。
- `Right`：訊息從一個 task 帶到另一個 task 的 right（`Send` 或 `Receive`）。
- `into_raw` 給要把 right 搬進訊息的地方；`from_raw` 是 `unsafe`。

### `platform::macos::message`

一則 Mach 訊息，手動排出來、拆開（kernel 的結構是 4 bytes 對齊的，`#[repr(C)]` 排不出來）：header；帶 right 時是 descriptor 的數量和每個 right 一個 port descriptor；然後是 body（長度加位元組，補到 4 的倍數）。

```rust
let mut buffer = message::Buffer::for_body(max_body);
message::send(&mut buffer, To::Port(&to), reply, id, body, rights, timeout)?;
let received = message::receive(&mut buffer, port.name(), timeout)?;
// received.id、received.reply、received.rights、received.sender.pid()、buffer.body(&received)
```

- **誰送的是 kernel 說的**：每一則收到的訊息都帶 audit token，`Sender::pid()` 是送的那個 process，kernel 自己是 0。送的一方改不了。
- **right 不會掉**：送出去的歸 kernel；沒送成的（對方的 port 死了、佇列滿了又不等、太大）當場放掉。收到的歸收的一方，包成會自己放掉的型別。
- **排法不對的訊息整則毀掉**：body 聲稱比訊息長、out-of-line 的記憶體或 port 陣列、不是 send 也不是 receive 的 right、超過 16 個 right、不是「回一次」的 reply right，都是 `Malformed`，它帶來的 right 和記憶體全部放掉（`mach_msg_destroy`），不會先拿一半。
- **body 內嵌或 out-of-line**：`Buffer::for_body(n)` 只收發內嵌的，上限就是 `n`。`Buffer::with_out_of_line(inline, most)` 的 body 超過 `inline` 時走 out-of-line（kernel 把送的一方的那幾頁用 copy-on-write map 進收的一方，不複製內容），最多 `most`；收到超過 `most` 的、兩塊的、內嵌和 out-of-line 同時有的，都是 `Malformed`。收到的那一塊是收的一方自己的（送的一方之後改不到），跟著 `Received` 一起解除。
- **`try_send`**：不等的送，right 用複製的。佇列滿了回 `TimedOut`，呼叫的人手上的 right 原封不動。kernel 在送失敗時會把訊息「退回」成收到的樣子（每個 right 多一個計數、out-of-line 的記憶體重新 map 一份），所以失敗時一律對緩衝區呼叫 `mach_msg_destroy`，把退回來的放掉。
- 比緩衝區長的訊息：kernel 從 port 上拿掉、毀掉，回 `TooLarge`。
- `Received::port`：訊息到的是哪一個 port（在 port set 上收的時候用）。

### `platform::macos::channel`

通道的一端（Chromium 的 `ChannelMac`），實作端點要的 `Platform`：這一端的 receive port，加上通往對方的 send right。

```rust
// host：child 啟動之後
let (channel, rights) = Channel::pair(child.id())?;
server.register(child.id(), rights);
let (link, endpoint) = ipc::endpoint::endpoint(channel, config);

// child
let rights = rendezvous::fetch(host_pid, wait)?;
let channel = Channel::from_rights(rights, host_pid)?;
```

- **核心的一則訊息是一則 Mach 訊息**，整則或沒有：64 KiB（`INLINE_MAX`）以下內嵌，以上 out-of-line。不切。handle 是 send right（region 的 memory entry）。
- **IO thread 等一個 port set**（`PortWaiter`，這個平台的 `Waiter`）：每條通道這一端的 port（對方的訊息和 kernel 關於它的通知都到這裡）都加進去，再加一個只有自己能寫的叫醒用的 port。收到的訊息說它到的是哪一個 port，就知道是哪一條通道的。叫醒是先設旗標、再送一則到叫醒的 port；每次等之前先看旗標，所以叫醒的 port 滿了也不會漏。接收的緩衝區整條 thread 一個。
- 比緩衝區長的內嵌訊息被 kernel 毀掉時不知道是哪一條通道的，略過；那條通道之後的訊息序號不連續，那時才關掉。只有惡意的對端會送（我們自己超過 64 KiB 的走 out-of-line）。
- **送不等**：佇列滿了（port 的佇列開到 1024 則）就回 `WouldBlock`，並請 kernel 在有空間時通知（`MACH_NOTIFY_SEND_POSSIBLE`；登記的當下已經有空間的話它立刻通知），通知到了是 `Event::Writable`。
- **對方不在了**：對方的 port 死了（dead-name）或通往這一端的 right 都沒了（no-senders），任一個都是 `Event::PeerGone`。
- **誰送的看 kernel**：只收對方的訊息。一開始照 pid，第一則到了之後記下它的整個 audit token，之後每一則都要一模一樣。通知只認 kernel 送的（pid 是 0）；對方送一則 id 和通知一樣的訊息沒有任何效果。不是對方送的、id 不對的，都丟掉，通道照常。
- **不是訊息的東西**（比緩衝區長、排法不對）只有對方送得進來，當成對方不在了。
- 收到的 receive right 不是 handle，放掉；那則訊息的 handle 就比它說的少，核心會拒絕。
- `shut` 放掉通往對方的 right 和自己的 port，對方的 kernel 各通知一次。

### `platform::macos::rendezvous`

child 怎麼拿到它的 port（Chromium 的 `MachPortRendezvous`）：child 繼承不了 Mach port，所以 host 向 launchd 的 bootstrap 登記一個 server，名字帶自己的 pid（`org.machopo.MachPortRendezvousServer.<pid>`）；child 用 parent 的 pid 拼出名字、查到、發問；server 照 kernel 給的 audit token（不是照 child 說的任何話）把登記在那個 pid 底下的 right 交出去，只交一次。

```rust
// host：一個 process 一個，一直開著
let server = rendezvous::Server::start()?;
let child = Command::new(exe).spawn()?;
server.register(child.id(), vec![Right::Receive(its_port), Right::Send(to_ours)]);

// child
let rights = rendezvous::fetch(std::os::unix::process::parent_id(), wait)?;
```

- **child 可以比登記早問**（host 要等 child 啟動了才知道它的 pid）：問題留著，`register` 時當場回答。
- **沒有登記的 process 什麼都拿不到**：它的問題一直等，`fetch` 逾時（`TimedOut`）。等著的問題最多 64 個，超過就拒絕最舊的；同一個 process 再問一次，前一個問題就被拒絕。
- **回答過的再問會被拒絕**（`Refused`），不是等。同一個 pid 重新 `register`（pid 被新的 child 重用）之後可以再回答。
- **只回答「問題」**：id 對、沒有內容、沒有帶 right、有回答用的 port。其他的丟掉，帶來的 right 放掉。
- `forget(pid)`：child 還沒問就不在了的時候，收回登記的 right。
- `fetch` 的錯誤：`NoServer`（查不到那個 host 的 server）、`Refused`、`TimedOut`、`Failed`。

### `platform::linux::channel`

通道的一端（Chromium 的 `ChannelPosix`）是一條 Unix stream socket：核心的訊息是串流裡的位元組，寫多少算多少、讀到多少交多少，訊息由核心自己切出來。handle 是 fd（region 的 memfd），跟著它那一則的位元組走（`SCM_RIGHTS`）。

- **child 怎麼拿到它那一端**：`spawn` 用 `socketpair` 建兩端（都是 close-on-exec），在 `Command` 的 `pre_exec` 裡，也就是 fork 之後、exec 之前、只在 child 的 process 裡，拿掉 child 那一端的 close-on-exec，號碼放在環境變數 `MACHOPO_IPC_FD`。所以只有這個 child 繼承到，同時啟動的別的 child 拿不到；host 開著的別的東西（其他 child 的通道、region）它也拿不到。`inherited` 讀環境變數，確認那個號碼是 `AF_UNIX` 的 stream socket（0、1、2 不算），收下、重新設 close-on-exec；只拿一次。變數留在 child 的環境裡（在 Linux 上拿掉它只有沒有別的 thread 時才安全），它啟動的 process 看得到，但那個號碼在那裡是關著的，拿不到通道。
  - 號碼不是秘密：fd 只在自己的 process 裡有意義。不放在命令列：child 可能是有自己參數的程式（測試的 child 就是）。
  - 不 dup2 到別的號碼：dup2 會悄悄關掉目標號碼上原本的 fd，來源和目標同號時又什麼都不做。
  - `pre_exec` 的 closure 留在 `Command` 裡，所以那個 `Command` 不能再用：再啟動一次，它會把那個號碼上當時的 fd 交出去。
- **寫**：`sendmsg`，不等（`MSG_DONTWAIT`），對方不在了不要 signal（`MSG_NOSIGNAL`）。寫進去多少回多少；fd 跟著第一個位元組走，寫進去了這一端的就關掉。一則最多 16 個 fd（核心的上限），多的不送。
- **等**：`EpollWaiter` 是一個 level-triggered 的 epoll，裡面是每條通道的 socket 和一個 eventfd（`interrupter` 寫它；寫幾次都只打斷一次）。`epoll_wait` 一次最多拿 16 個事件，每次 `wait` 交出一個、對那條通道做一次 `recvmsg`，讀進 waiter 的 256 KiB 的緩衝區借出去；沒讀完的 epoll 會再說。
- **「可寫」只在寫入被擋之後才等**：socket 幾乎一直有空間，一直等的話 epoll 不停地醒。`write` 碰到 `EAGAIN` 時，在當下那條 thread 上把這條 socket 在 epoll 裡的登記加上「可寫」；IO thread 被告知一次就拿掉。通道和 waiter 為此共用 epoll 的 fd（`Arc`：waiter 先結束時 fd 還在，寫的 thread 不會改到別人的 fd）。還沒有 waiter 的通道被擋過，加入時就帶著「可寫」。鎖的順序：`write` 是在核心的鎖裡被呼叫的，再拿通道自己的；所以拿著通道的鎖時不呼叫核心。
- **對方不在了**是讀到串流的結尾或讀出錯；不看 epoll 的 hang-up，所以對方關掉之前送的都先交出去。
- **收到的 fd** 帶 close-on-exec（`MSG_CMSG_CLOEXEC`）。一次讀到超過 16 個、或 kernel 說有 fd 放不下（`MSG_CTRUNC`），就當成對方不在了。控制資料的緩衝區大小不是那個上限：它會進位，多一兩個也放得下，所以另外數。
- **緩衝區用 kernel 的預設**：量過，Linux 上把 `SO_SNDBUF` 設大沒有差別，`SO_RCVBUF` 對 Unix socket 沒有作用。socket 也不設成 non-blocking：每次讀寫自己說不等。
- `shut` 是 `shutdown(SHUT_RDWR)`：對方讀到結尾，它的寫入被拒絕。一條通道只能有一個 waiter；shut 過的不能再加入。
- `Channel::pair()`：同一個 process 裡的兩端，測試用。

### `platform::linux::memory`

共用記憶體是 memfd：一個沒有名字的檔案，建好就封住（seal）它的長度。API 和 macOS 的那一份同一個形狀，handle 是 `OwnedFd`：

```rust
let (mine, handle) = memory::create(len, false)?;   // mine 可寫；handle 從此誰都 map 不成可寫
let (mine, handle) = memory::create(len, true)?;    // mine 只 map 成唯讀；handle 可寫
let theirs = memory::map(handle.as_fd(), len, writable)?;
```

- **長度封住**（`F_SEAL_SHRINK`、`F_SEAL_GROW`，最後 `F_SEAL_SEAL` 讓 seal 不能再加）：碰到落在檔案結尾之外的 mapping 不是錯誤，是 process 被結束，所以拿著 fd 的對方不能把檔案縮短。
- **只能讀的 handle** 靠 `F_SEAL_FUTURE_WRITE`：建的一端先 map 好自己可寫的那一份，再封；之後這個檔案不管經過哪個 fd（對方收到的、經過 `/proc/self/fd` 重新開的）都 map 不成可寫、`write` 不進去，唯讀的 mapping 也 `mprotect` 不成可寫。這個 seal 要 Linux 5.1；更舊的 kernel 上 `create(_, false)` 回 `None`。Chromium 交出去的是重新開成唯讀的 fd，靠 sandbox 不讓 renderer 碰 `/proc`；這裡不靠 sandbox。
- **不能執行**（`MFD_NOEXEC_SEAL`，Linux 6.3）：能寫 region 的對方不能在裡面寫一個程式再執行它。kernel 不認得這個旗標（回 `EINVAL`）時不帶它再建一次；不記住，region 很少建。
- **`map` 只 map「不能縮短」的檔案**，並且先確認它夠長（memmap2 給了長度就不看檔案的大小）：先看 seal、再量長度，量到的就不會再變短。所以不管 fd 是誰給的，`map` 不會留下會讓 process 結束的 mapping。長度 0 不 map。
- `mmap` 和 `munmap` 經過 memmap2 的 `MmapRaw`（安全的 API，drop 時解除）；其餘是 rustix 的安全呼叫。這一份自己的 `unsafe` 只有 `Mapping` 的 `Memory`。

### `platform::windows::memory`

共用記憶體是 page file 的 section：沒有名字、沒有檔案，也不能改大小。函式和 macOS 的那一組一樣（`create`、`map`、`Mapping`），region 的 handle 是 std 的 `OwnedHandle`。

- **交出去的 handle 只有 map 的權限**：kernel 給的第一個 handle 還有別的（查詢、刪除、讀存取清單），它不離開 `create`；交出去的是用 `DuplicateHandle` 複製的，對方只能讀的那一種只有 `FILE_MAP_READ`，kernel 不讓它 map 成可寫。
- **換不成權限更多的 handle**：要的權限比來源多時 kernel 照 section 的存取清單檢查，所以 section 建立時給一份誰都不允許的清單，其中一項不讓擁有者改清單（不寫的話擁有者不看清單就能改，而同一個使用者的 process 都是擁有者）。Chromium 的清單是空的，靠 sandbox。
- **收到的 handle 不檢查是哪一種 section**：只有 child 收 handle，給的是 host。之後 child 也能送時，要加 Chromium 做的兩個檢查（不是 image section、權限和說的一致），交出去的 handle 也要留查詢的權限。
- **唯讀的 mapping 一直是唯讀的**：map 成唯讀的 view 之後改不成可寫。
- **長度要是整頁**：kernel 建得出任何長度的 section，這裡不建不是整頁的。map 可以少於整塊（它的開頭），不能多；長度 0 在 kernel 是「整塊」的意思，這裡不 map。
- 建立時就向系統預訂整塊的記憶體（commit）；訂不到時沒有 region。
- 交出去的 handle 不被 child 繼承。

### `platform::windows::channel`

通道的一端是一條 named pipe 的一端（Chromium 的 `mojo/core/channel_win.cc`）：讀和寫都是 overlapped 的，完成的通知到 IO thread 的 completion port（`IocpWaiter`，這個平台的 `Waiter`），一個 port 等這個 process 所有的通道。

```rust
let (server, client) = channel::pipe()?;            // 兩端都在這裡建；client 那一端之後交給 child
let host = Channel::host(server, child_process)?;   // child_process：child 的 process handle（借用）
let child = Channel::child(client);                 // 在 child 裡
```

- **為什麼要 completion port**：pipe 滿了的時候寫入在 `write` 回來之後還在 kernel 裡進行，而 Windows 在 thread 結束時取消它發出的 I/O，除非 handle 在 completion port 上。任何 thread 都會送，送完就結束的 thread 不能把訊息帶走。
- **交給迴圈之前不寫**：pipe 要到 `add` 才在 port 上，在那之前 `write` 回 `WouldBlock`，訊息留在核心的佇列；加入時被拒絕過的那一端收到 `Writable`。一個通道只能加入一個 waiter、一次。
- **寫**：一次最多 64 KiB（`RECORD_MAX`，pipe 的緩衝區，host 的連表頭），其餘的核心留著。pipe 有空間時寫入在呼叫裡就完成，下一筆可以馬上寫，不用等 IO thread（pipe 設了「當場完成的不到 port」）；沒有空間時那一筆在 kernel 裡進行，這段時間 `write` 回 `WouldBlock`，它的通知被 IO thread 從 port 取走時，被拒絕過的收到 `Writable`。量過：每筆都等 port 的話，連續送慢 2 倍（一條通道）到 8 倍（四條）。
- **handle 只從 host 往 child**：host 的每次寫入前面有這一層的表頭（後面幾個位元組、幾個 handle，各 `u32`；每個 handle 一個 8 位元組的數值）。host 寫入當下把 handle 複製進 child 的 process（它留的 process handle 只有 `PROCESS_DUP_HANDLE`），數值是 handle 在 child 裡的值；寫入沒發出去時把放進去的從 child 裡關掉。child 那一端照表頭把數值包成 handle（信任 host），handle 跟著那一次寫入的位元組借出去。
- **child 往 host 沒有表頭**：寫的就是核心的位元組，host 讀到什麼原樣借給核心。host 這一側沒有任何一行把位元組變成 handle；child 的訊息說它有 handle 時由核心當成壞訊息。child 那一端被交了 handle 時放掉它們。
- **讀**：每條通道一塊 64 KiB 的緩衝區（一次寫入的上限），讀在 IO thread 上發出；資料已經在的話當場拿到，不在的話等 port 的通知。child 那一端一次借出一次寫入的位元組（一次讀到好幾筆時一筆一筆借，切到一半的等下一次讀接起來）。
- **一條很忙的通道不讓別條等**：借出一筆之後，下一輪換別條已經有東西的通道；連續 8 輪都在處理已經讀到的之後，一定看一次 port（只看、不等），所以別條通道在 port 裡的通知最多等 8 輪。
- **對方不在了**：讀失敗、讀到 0 個位元組、寫入失敗或少寫、child 收到不合理的表頭（長度 0、超過一次寫入、handle 超過 16 個、數值是 0 或 -1），都是 `Event::PeerGone`，一條通道只報一次。自己 `shut` 的那一端之後什麼都不報。
- **進行中的操作拿著通道**：沒有當場完成的讀寫，kernel 手上有它的 `OVERLAPPED` 和緩衝區，直到它的通知從 port 取走；所以每筆進行中的操作拿著通道內部狀態的一份 `Arc`，waiter 取到通知時才放掉（Chromium 的 `pending_read_`、`pending_write_`）。當場完成的不拿。通道關掉、離開迴圈之後遲到的通知照樣取走、釋放，不報出來。waiter 丟掉時還在進行的不收，那幾塊記憶體不釋放（洩漏是安全的）；`shut` 關掉所有 handle，留下的只有記憶體。
- **`Channel` 的值丟掉就是 `shut`**：waiter 可能還拿著它的內部狀態，handle 不等它。
- **打斷等待**：一個旗標加 `PostQueuedCompletionStatus`，叫很多次只放一筆。
- **child 怎麼拿到它那一端**：`spawn` 建好兩端，把 client 那一端設成可繼承、把它的數值放進 child 的環境變數 `MACHOPO_IPC_CHANNEL`、啟動、關掉自己的那一份；這一段在一把整個 process 共用的鎖底下，同時啟動的兩個 child 不會互相繼承到對方的。啟動完把變數從呼叫的人的 `Command` 拿掉。`inherited` 讀變數、確認它指的是一個 pipe、把 handle 設回不可繼承、把變數從自己的環境拿掉；一個 process 只拿得到一次。拿掉環境變數在 Windows 上任何時候都可以（系統的呼叫本身容許別的 thread 同時讀）。`child_gone` 什麼都不做：child 結束時它那一端跟著關掉，host 的讀就失敗。
- **繼承的缺口**：標準庫啟動 process 時沒辦法只繼承指定的 handle，所以啟動的那一小段裡，這個 process 在別處（不經過 `spawn`）啟動的 process 也會拿到 client 那一端：它能讀寫通道，而且 child 當掉時 pipe 不斷。現在沒有別處會啟動 process。
- **pipe 的兩端**（`pipe()`）：名字是 pid、序號和亂數，只在函式裡；名字第一次用、只有一個 instance、不收別台機器的、client 那一端沒有再建 instance 的權限、server 不能冒用 client，建好後確認 client 連著；失敗就換名字，最多試 8 次。兩端都不被 child 繼承。

## `unsafe` 的論證

在 `crates/ipc/soundness.md`：要保證的性質、信任誰、對 kernel 的假設、不變式、每一處 `unsafe` 為什麼安全、修改時要審查什麼。新增或改動一處 `unsafe` 時，程式裡的 `// SAFETY:` 和那份文件一起改。

## 測試

```sh
cargo test -p ipc
```

`src/core/frame.rs` 的單元測試：開頭的每個欄位在哪裡；三則訊息（有 handle 的、空的、兩個 handle 的）一次餵完、在每一個位置切成兩段、一次一個位元組，交出來的都一樣；只留沒到齊的那一則；長度的上限只看開頭就擋（剛好在上限的不擋）；謊報的長度不佔記憶體；序號跳號和重複；沒有權限的一端留不下任何 handle（開頭說有、開頭說沒有但附了、開頭說有但沒附）；handle 太多（一則的、堆著沒認領的）和不夠；壞訊息之後什麼都不留、答案不變；最後一個序號之後沒有下一則；`Encoder` 擋掉的不佔序號；兩條 thread 經過一個隨意切位元組的假傳輸（`core/fake.rs`，只在測試裡），收到的照順序、各一次。

`src/core/channel.rs` 的單元測試（用一個記下寫了什麼、照給的空間收的假傳輸）：收得下的當場寫；收不下的照順序等、從停下來的地方接著寫，之後送的排在後面；`flush` 寫到沒有空間為止；一次 `write` 只有一則訊息；佇列剛好到上限可以、超過就關（等著的和 handle 丟掉、IO thread 被叫醒、只聽到一次）；寫的時候對方不在了；這一端自己的錯不動通道；大訊息的緩衝區不留；收到的照順序、帶目的地；不認得的目的地、`sink` 拒絕的、框架擋下來的都關掉通道，後面的不交出去；結束只報告一次、理由是第一個。

```sh
python3 scripts/test.py ipc models   # 通道的 thread 共用的東西在 loom 和 shuttle 底下
```

`core::sync` 在 `--cfg ipc_loom`、`--cfg ipc_shuttle` 時把鎖換成 loom 和 shuttle 的（不用 `loom` 這個 cfg：`ipc` 依賴的 tokio 自己也認它，會拿掉幾個模組、換掉幾處實作，測到的就不是平常編出來的那個 tokio）。loom 的模型（`loom_tests`，每一種交錯都試）：兩條 thread 同時送、第三條補空間和 `flush`，每則各寫一次、完整、序號照順序；送的同時對方不在了，結束只聽到一次、之後送不出去、沒有東西留著；兩條同時送而佇列只放得下一則，剛好一條被拒絕、理由是 `QueueFull`。shuttle 的模型（`shuttle_tests`，隨機的排程各 10000 次）：3 條 thread 各送 12 則長短不一的訊息，傳輸一次只收幾個位元組，一條 thread 補空間和 `flush`，全部寫出來、每條 thread 自己的照順序；同樣的情況中途關掉通道，結束只聽到一次，寫出去的是完整的訊息加上最多一則的開頭，沒有「被告知沒送出去」的訊息。

`src/core/region.rs` 的單元測試（記憶體是 `HeapMemory`，Miri 底下也跑）：長度對到哪一種 region；寫進去的就是對方讀到的；放進最短的放得下的空塊；出去了的在對方讀完之前不會再被寫、被拒絕的那一筆沒有碰到它；還一塊沒出去的、還兩次、不是這個池子的都是壞訊息；`take` 的那一塊到 `give_back` 之前是拿的人的；不在表裡的編號、超過那一塊的長度（多一個位元組、`u32::MAX`）不讀，整塊和零個位元組可以；從表拿掉的不能再提、號碼被佔的不換掉原來那一塊；池子只收合法長度、不重複、總量以內的（剛好到上限可以）；只有空的拿得走、拿走之後其他的編號和狀態不變。

```sh
python3 scripts/test.py ipc verus    # core/fit.rs 的證明（cargo verus verify -p ipc）
```

要裝 Verus（版本和裝法見 `test.py ipc -h`）；它用自己的 toolchain 編整個 crate，約一秒，第一次約一分鐘。

`tests/it/codec.rs`：沒有位元組的值寫出來和 postcard 一模一樣（serde 的每一種形狀各有）；每一種形狀裡的位元組都回得來；大的位元組不管巢狀在哪裡都走 region、訊息只剩一點、讀的順序照值裡的順序；對方讀完之後那一塊再用；沒有空塊、或比最大的還長時留在訊息裡，用著的那一塊沒被蓋掉；寫在 `out` 原有的內容後面；序列化到一半失敗的值不留下放進去的 region 和寫了一半的位元組；不是對方的 region、太長、沒有 region 的一端被提到 region、不認得的 variant 都是壞訊息，回報的是第一個錯；多一個位元組、少一段、聲稱比訊息還長的位元組都是壞訊息；六種巢狀的方式各自剛好到上限可以、多一層是 `TooDeep`，一百萬層的不會真的走進去；又寬又淺的值不受影響。

`tests/it/pipe_in_message.rs`（值裡的管道，用一個記下開了哪些管道的假的一端）：`Consumer` 在 struct、`Option`、`Vec`、map、enum 的兩種 variant 裡都照值的順序各開一條、訊息裡只有編號，值留下的是結束的管道而原本的管道還開著，另一端解出來的是它自己那幾條的 `Consumer`；同一個值只能過一次；值在開了管道之後失敗時那些管道被結束、來源放掉、`out` 不變；不開管道的一端送不出帶管道的值，開到一半開不了時前面開的收回；編號不是等著的管道、提兩次、沒有管道的一端都是 `UnknownPipe`；JSON 和純 postcard 都不能序列化或讀出 `Consumer`，被拒絕的值管道還在；冒用那個名字的型別送的時候拿不到管道、收的時候它提到的那一條被放掉、不留給下一個值。

`tests/it/region_fuzz.rs`：fuzzer 跑的另一段檢查（`fuzz/fuzz_targets/region.rs`）。輸入當成三樣東西：一則提到 region 的訊息的內容（不管它寫什麼編號和長度，讀都留在 region 裡面，ASan 看著；解得出來的值再寫、再讀要一樣）、一則 `Control`（解得出來的再寫要是同樣的位元組）、一串對池子和表的操作（加、放、拿、還、對方還、拿掉、讀），每一步都對著一個簡單的模型：結果一樣、放進的是最短的放得下的、出去了的那一塊的內容沒有變、總量不超過上限。重播 `fuzz/regressions/region/`。

`src/core/control.rs` 的單元測試：七種訊息都讀得回來、種類的位元組各不相同、每個欄位在哪裡；沒有人會寫的 20 種內容都是壞訊息。

`src/core/pipe.rs` 的單元測試：送的不超過空間、被拒絕的不算數；回報的比送的多是壞訊息；一則最多一個 chunk，容量 0 的管道放得下一個位元組；每條管道有自己的編號和額度；給結束的管道的回報和取消丟掉、給沒開過的是壞訊息；讀的一端每讀一半回報一次（奇數的容量、最小的幾種）；六種容量乘六種讀法，寫的一端額度用完之後一定等得到回報；讀的一端分得出結束的和沒開過的；編號不重開。

`tests/it/pipe_fuzz.rs`：fuzzer 跑的第三段檢查（`fuzz/fuzz_targets/pipe.rs`）。寫的一端、讀的一端（真的 `data_pipe`）和中間編成位元組的訊息，照輸入的步驟走：送、結尾、把一則交給對方、讀、取消、和輸入編出來的回報。照規矩的讀的一端：手上不超過容量、讀到的就是寫的、照順序、兩端都不把對方當成壞的，最後一定讀得完。編出來的回報：寫的一端只接受不超過它送的。重播 `fuzz/regressions/pipe/`。

```sh
python3 scripts/test.py ipc tla      # crates/ipc/tla/ 的三個 TLA+ 模型（TLC）
```

`tla/Channel.tla`（通道的一端：佇列的上限、結束只報告一次、理由是第一個、之後不交出訊息、沒有東西留著等）、`tla/Regions.tla`（region 給哪個 peer、先告知再提到、寫進去、對方真的讀到那則訊息時才還、peer 不在了之後不轉給別人）、`tla/Pipe.tla`（管道的額度、回報的門檻、結尾和取消在路上交錯、亂回報的讀的一端、不會卡住）。`tla/check.py` 用 TLC 檢查每個模型成立，再把每一部分拿掉（模型的 `Mutation`），確認 TLC 找得到列出來的那個反例。TLC 的 jar 第一次跑時下載（釘住版本和 SHA-256，放在 `target/tla/`），要有 Java。

`src/platform/macos/memory.rs` 的單元測試（只在 macOS；同一個 process 裡把 handle 再 map 一次當成對方）：建的一端寫的、對方讀得到，而且是同一塊記憶體、不是當時的複本（handle 建好、map 好之後才寫的也看得到）；只能讀的 handle map 成可寫會失敗；對方寫的、建的一端讀得到；唯讀的 mapping 改不成可寫（對方的、建的一端對「對方寫」那一塊的、可寫的 region 唯讀 map 的）；有 mapping 在，放掉 handle 記憶體還在；map 超過長度失敗、少於可以；不是整頁的長度不建；kernel 拒絕的呼叫不給 handle；不是 region 的 port map 不了；handle drop 就是放掉 right、`into_raw` 之後不放；mapping drop 就是解除（map 又 drop 4 GiB，位址空間沒有長大；留著的時候有長大）；池子和表接在真的 region 上。這一組重複跑 300 次、整組單元測試平行跑 50 次都過。

`src/platform/linux/memory.rs` 的單元測試（只在 Linux；同一個 process 裡把 handle 再 map 一次當成對方）：建的一端寫的、對方讀得到，而且是同一塊記憶體；只能讀的 handle map 成可寫會失敗，`write` 不進去，經過 `/proc/self/fd` 重新開成可寫的也一樣；對方唯讀的 mapping `mprotect` 不成可寫；兩端各自只把自己寫的那一種 map 成可寫（看 `/proc/self/maps`）；對方寫的、建的一端讀得到；兩種 region 都不能縮短、加長、再加 seal，seal 就是該有的那幾個；kernel 有 `MFD_NOEXEC_SEAL` 時 region 不能改成可執行；kernel 不認得旗標時不帶它再問一次，別的拒絕不再問，認得的只問一次（用假的 `memfd_create`）；有 mapping 在，放掉 handle 記憶體還在；map 超過長度和長度 0 失敗、少於可以；還能縮短的檔案、`/dev/null`、socket 都不 map；mapping drop 就是解除（數 `/proc/self/maps` 裡這個 memfd 的 mapping）；池子和表接在真的 region 上。

`src/platform/macos/message.rs` 的單元測試（同一個 process 裡自己送給自己）：幾種長度的 body 原樣到、送的人是這個 process；沒有訊息時逾時；right 照順序到、各自還是原來那一種（對它送的訊息從原來的 port 出來）；回答送到發問的人指定的 port；沒回答就丟掉時 kernel 會通知（送的人是 0）；送到死掉的 port 送不出去、帶的 right 被放掉；收到又丟掉的 right 被放掉、receive right 丟掉 port 就死了；超過緩衝區的 body、超過 16 個 right 不送，right 被放掉；比緩衝區長的訊息被拒絕並且從 port 上拿掉；佇列滿了、不等的送會被拒絕，讀走一則就有位置；排法不對的六種（太長的 body、out-of-line 的記憶體、out-of-line 的 port、send-once 的 right、太多 right、不是回一次的 reply）都被拒絕、帶來的被放掉，之後 port 照常。

`src/platform/windows/memory.rs` 的單元測試（只在 Windows；同一個 process 裡把 handle 再 map 一次當成對方）：和 macOS 的那一組同樣的情況（建的一端寫的對方讀得到、是同一塊記憶體；只能讀的 handle map 成可寫會失敗；對方寫的建的一端讀得到；唯讀的 mapping 改不成可寫；有 mapping 在，放掉 handle 記憶體還在；map 超過長度失敗、少於可以、長度 0 不 map；不是整頁的長度不建；kernel 拒絕的呼叫不給 handle；不是 region 的 handle（一個檔案的）map 不了；mapping drop 就是解除；池子和表接在真的 region 上），加上：只能讀的 handle 換不成權限更多的（可寫的、全部的、改清單的都要不到），兩種 handle 都只有 map 的權限（查詢、刪除、讀清單的也要不到）；交出去的 handle 不被 child 繼承；4000 塊建了又放，process 的 handle 沒有變多，留著時一塊一個。

`src/platform/windows/channel.rs` 的單元測試（兩端在同一個 process 裡，各有自己的 `IocpWaiter`，host 的「child」是這個 process；每個測試自己設 10 秒的時限）：一端寫的另一端拿到；長的訊息一次只收一次寫入放得下的（host 的少一個表頭），一段一段照順序到；pipe 有空間時兩個方向各連寫 20 筆都不用等、也不在 port 留下東西；pipe 滿了才有一筆在進行，這時再寫被拒絕，對方讀走、通知取走時收到 `Writable`，沒被拒絕過的不收；交給迴圈之前的寫入被拒絕、加入時被告知、之後寫得出去；0 個位元組什麼都不寫；handle 跟著訊息、送出去之後不在寫的人手上、照順序、各是原本的權限、不被繼承、map 得到同一塊；2000 則帶 handle 的送完 process 的 handle 沒有變多；對方不在時 2000 次送不出去，handle 還在寫的人手上、child 裡一個都沒留；child 被交的 handle 被放掉、host 一個都沒收到，child 寫的像表頭的位元組（數值是 host 自己的 pipe）host 當成位元組、那個 handle 還開著；host 的三筆寫入在一次讀裡，各借出去一次；最長的寫入兩筆；一個位元組一個位元組到的表頭和內容接得起來，一筆之後跟著半筆也不掉；六種不合理的表頭都是對方不在了、之前合理的照樣借出去；打斷 20 次只報一次、之後下一件事照常、另一條 thread 打斷得了；一個 waiter 等四條通道分得出各是哪一條（加入之前就到的也收得到），一條的對方不在了只有那一條被告知，關掉的、已經加入過的加不進去；拿掉的那一端有位元組、被拒絕過、或加入前被拒絕過，都不再報；關掉的一端送不出去，對方先收到關之前送的、再被告知一次，自己那一端被取消的讀什麼都不報；值丟掉就是對方不在了；讀和寫都失敗時也只報一次；host 留的 process handle 問不到 process 的狀態；送的 thread 結束之後它還在進行的寫入照樣到；300 條通道用過放掉之後 process 的 handle 沒有變多（留著時有）；進行中的讀、進行中的寫各自讓通道的內部狀態活著、通知取走之後才釋放，當場完成的讀寫不拿著它（值丟掉就釋放）；一條通道一次讀到 1000 筆要借出去時，另一條通道的訊息最多等 9 筆就輪到，那 1000 筆的順序不亂；關掉之後 pipe 和 child 的 process handle 都不在了；pipe 的兩端不被繼承、第二個 client 連不上、每條的名字不同；變數指到的不是 pipe（空的、0、不是數字、一個開著的檔案）就沒有通道、那個檔案還開著；拿到的那一端被設回不可繼承、變數清掉，再設一次也拿不到第二次。

`src/platform/macos/message.rs` 另外為 out-of-line 和不等的送：長的 body 走 out-of-line、到的一樣；送出去之後送的一方再改，收的一方那一份不變；不收 out-of-line 的緩衝區、上限比較小的緩衝區都拒絕，剛好到上限的收；收了又丟、被拒絕的各 2 GiB 之後位址空間沒有長大；佇列滿時帶著 right 和 16 MiB 的 body 連送 64 次，位址空間和 right 都不變，有空間之後同一個呼叫送得出去；kernel 在有空間時通知、佇列開到 1024 則、port 死掉時通知（在 port set 上也收得到，知道是哪一個 port）。

`src/platform/macos/channel.rs` 的單元測試（兩端在同一個 process 裡，各有自己的 `PortWaiter`；每個測試自己設 10 秒的時限）：一個 `PortWaiter` 等四條通道，分得出各是哪一條的（加入之前就到的也收得到），一條的對方不在了只有那一條被告知，拿掉之後到它的 port 的不算，關掉的一端加不進去；還沒加入迴圈的一端叫醒沒有作用；一端寫的另一端整則拿到（內嵌和 out-of-line 的長度都有）；handle 跟著訊息、送出去之後不在寫的人手上、收的一端 map 得到同一塊；叫醒 20 次只報一次、之後下一件事照常；關掉的一端送不出去、對方先收到關之前送的、再被告知；一端 drop 就是對方不在了；佇列滿了回 `WouldBlock`、帶的 handle 還在、對方讀走一則就收到 `Writable`、同一個寫入成功；只有對方的 port 死了、或只有通往這一端的 right 都沒了，各自都是對方不在了；dead-name 通知多給的計數有放掉；receive right 不算 handle；right 的形狀不對、host 先走了，都建不起一端。

`tests/it/process_channel.rs`（兩個真的 process 之間，不分平台：只用 `ipc::platform` 的 `spawn`、`inherited`、`child_gone` 和 `Platform` 的函式，每個平台的那一層都要原樣通過，現在是 macOS、Linux 和 Windows；host 是測試裡真的端點，child 是測試把自己再執行一次；自己動手寫位元組的 child 先等它那一端、再寫，因為有的平台在有 thread 服務之前不寫）：訊息來回；host 的大資料走它建的 region、child 的在沒有 region 時放在訊息裡、給了 region 之後走 region；3 MiB 的字串來回；700 KiB 的管道；child 自己關、host 關、child 直接結束、child 被 kill，另一端都知道，理由對；child 還沒拿它那一端就結束，host 說了 `child_gone` 之後收到對方不在了；child 先睡 1.5 秒不讀，host 連送 3000 則（超過傳輸放得下的），全部照順序回來；child 自己附 handle、送不是訊息的位元組，通道都關掉、什麼都沒交給上層；child 把 host 送的 region map 成可寫會失敗；一條 IO thread（一個迴圈）同時服務四個 child：各自的訊息照順序、大資料各走各的 region，kill 掉一個只有那一條結束，迴圈停下來時其餘的都關掉、child 各自結束。

`tests/it/windows_channel.rs`（只有 Windows 的通道有的：child 怎麼拿到它那一端；測試把自己再執行一次當成 child；另有一個平常不跑的量測 `measure`，在真的通道上量小訊息、連續送、四個 child、放在訊息裡和走 region 的大資料）：child 拿到一次、環境裡沒有那個變數了、第二次拿不到；`spawn` 之後 `Command` 不再帶那個變數，用它再啟動的 child 拿不到；變數是空的、0、不是數字、-1、太大、沒有這個 handle 的數值、沒設，都拿不到；child 啟動一個活得比它久的 child 之後結束，host 兩秒內被告知對方不在了（孫 process 沒有繼承到那一端）；host 之後另外啟動的 process 活著時 child 被 kill，host 兩秒內被告知；八條 thread 同時啟動八個 child，逐一 kill 都馬上被告知（沒有互相繼承）。

`src/platform/linux/channel.rs` 的單元測試（只在 Linux；同一個 process 裡的兩端，各有自己的 waiter）：一端寫的另一端照順序讀到，比一次讀的量長的分好幾次、位元組一樣；沒有位元組的寫入不帶走 handle；handle 跟著它那一則到、寫的一端的關掉了、收到的帶 close-on-exec、map 得到同一塊記憶體；16 個 handle 照順序到；17 個不送；對方一次送 17 個和 64 個都當成對方不在了；「可寫」只在寫入被擋之後說一次，沒被擋不說，有 waiter 之前被擋的加入之後說，和位元組同時來時兩個都說；對方不在了排在它送的東西後面，之後的寫入被拒絕（process 沒有被 signal 結束）；shut 的一端對方知道、兩邊都寫不進去、不能再被等；一個 waiter 分得出三條通道，移掉的不再說、它被擋的寫入不動原本的 waiter、可以換一個 waiter；移掉的 socket 真的離開 epoll（看 `/proc/self/fdinfo`）；一條通道只有一個 waiter；打斷叫五次只說一次、從別的 thread 打斷正在等的；waiter 先結束，通道的寫入照常被擋。整個單元測試的 binary 重複跑 3000 次都過。

`tests/it/linux_channel.rs`（只有 Linux 的通道有的：child 怎麼拿到它那一端）：child 只多拿到一個 fd，就是它的通道，host 開著的別的通道和 region 都不在它手上，拿的時候沒有多開或少開 fd，拿到的帶 close-on-exec，第二次拿不到；`Command` 活得比 child 久，host 照樣知道 child 結束了；環境變數沒有、不是數字、是 0 到 2（連 stdin 就是一條 Unix stream socket 時）、沒開著、開著但是 memfd、datagram 的 Unix socket、TCP 和 UDP 的 socket，都拿不到；是 Unix stream socket 就拿得到。

`tests/it/mach_channel.rs`（只有 macOS 的通道有的）：child 假冒四種 kernel 的通知和一種別的 id（都帶著像樣的內容），沒有任何效果；child 送超過 128 MiB 的 out-of-line，通道關掉；host 以為對方是另一個 process 時，child 送的全部被忽略、通道也不關。這兩組加 rendezvous 的重複跑 30 次都過。

`src/platform/macos/rendezvous.rs` 的單元測試：不是問題的訊息（id 不對、沒有回答用的 port、有內容）不被回答、回答的 right 被放掉，登記的東西還在；真的問題拿得到，而且只有一次。

`tests/it/rendezvous.rs`（只在 macOS；測試把自己再執行一次當成 child，用環境變數告訴它做什麼）：child 照順序拿到登記的 right，而且真的能用（它寫到測試的 port，kernel 說寫的人是 child 的 pid；搬給它的 receive right 收得到測試送的，它看到送的人是 host）；比登記早問的，登記時拿到；回答過的再問被拒絕；沒登記的 process 拿不到（同時有別的 child 登記著也一樣）；登記之後收回，child 拿不到、right 被放掉；同一個 process 的第二個問題擠掉第一個；沒有 server 的 host 是 `NoServer`；一個 process 只能有一個 server。這一組重複跑 30 次、整個整合測試 15 次都過。

`tests/it/endpoint.rs`（兩個端點接在 `local` 上，一端是能建 region 的 host、一端是 child）：訊息照順序到各自的目的地；host 的大位元組走它自己建的 region、對方讀完再用、同時送兩筆就多一塊；到了上限就放在訊息裡；child 不建 region、拿到 host 給的之後才用；管道的位元組照順序、讀的一端不讀就只送容量那麼多、讀了才繼續；讀的一端走了來源就停、通道照常；傳輸沒有空間時什麼都不會永遠卡住；關掉之後兩端都結束、送不出去、放進 region 的還回來。亂送的 child（一條裸的通道，送什麼由測試決定）：不認得的目的地、解不出來的值、還沒寫過或還兩次的 region、提到沒給過它的 region、超過長度、自己附 handle、沒有 handle 的 `Granted`、開管道、對沒開過的管道回報取消送資料、回報超過、提到沒有的管道、不是 `Control` 的內容，host 都關掉通道、說出理由，之後不再交出任何訊息。亂送的 host 對 child 也一樣（超過容量的資料、重開的編號、長度不對或重複的 region）。兩個迴圈各在一條 thread 上、各管一端（端點交給迴圈之前送的也到），三條 thread 同時送訊息和大位元組、加一條 900 KiB 的管道，全部到齊之後 host 關掉，兩邊的 `handler` 各被告知一次，理由是 `Closed` 和 `PeerClosed`。一個迴圈管五條通道（另一個迴圈管它們的另一端）：每條通道自己的訊息照順序到、互不相干，大資料各走各的 region；關掉其中一條只有那一條的兩端被告知，其他的照常；迴圈停下來時剩下的每一條都關掉、各被告知一次。端點交給迴圈之前做的事（開了管道、餵完、結束；關掉通道）那時叫醒不了誰，交給迴圈時才做：管道的位元組到對方、兩端各被告知結束。這三個測試自己設 30 秒的時限。一步一步推的那幾個測試自己接叫醒（`Link::on_wake`）。

`tests/it/endpoint_fuzz.rs`：fuzzer 跑的第四段檢查（`fuzz/fuzz_targets/endpoint.rs`）。host 的端點對一個照輸入送任何 frame 的 child（任何目的地、有沒有 handle），host 自己同時送大位元組、開管道、給 region。不能 panic、不能讀到 region 外面；說了通道結束之後不再交出訊息；region 的總量不超過上限。重播 `fuzz/regressions/endpoint/`。

`tests/it/frame_fuzz.rs`：fuzzer 跑的那一段檢查（`fuzz/fuzz_targets/frame.rs` 直接引用這個檔案），輸入是對端可能送的任何位元組、怎麼切、每一段旁邊附幾個 handle。任何輸入都不能 panic；同樣的位元組不管怎麼切，交出來的訊息和結局一樣；序號從 0 連續；沒有權限的一端的 handle 一個也到不了上層；留著的不比到了的多；每個 handle 不是在 `Decoder` 手上就是已經關掉。測試跑一組種子，並重播 `fuzz/regressions/` 裡 fuzzer 找到過的輸入。

```sh
python3 scripts/test.py ipc fuzz [--minutes M]   # cargo-fuzz，每個 target 跑 M 分鐘（預設 5）
python3 scripts/test.py ipc miri                 # 核心的單元測試和端點資料量小的測試在 Miri 底下
```

兩者用 `fuzz/rust-toolchain.toml` 釘的 nightly（和 elise 的同一個）。fuzzer 失敗時會印出輸入在哪裡：連同修正放進 `fuzz/regressions/`，取一個說明它找到什麼的名字。

`tests/it/data_pipe.rs`：寫進去的照順序讀出、一次最多讀 `max`、寫的一段比空間大時只放得下的進去、讀走才放其餘的（polling 的結果是確定的，不靠 thread 和時間）、結尾在資料之後、consumer 不在了寫失敗（等著的 write 被喚醒）、watch 的通知在什麼時候、幾次；另外一支跨 thread 的：tokio 上的 producer 經過 4 KiB 的管道送 300 KB。

`tests/it/time_ticks.rs`：時鐘和 `Instant` 一起往前走、相差不到幾毫秒；讀數序列化之後不變；另一個 process（測試自己的執行檔）的讀數落在這個 process 前後兩次讀數之間。Windows 的那一支只在 CI 上編譯和執行。
