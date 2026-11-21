# ipc

對應 Chromium 的 `ipc/` 和 `mojo/`：machopo 的各個 process 之間共用的東西。renderer 現在還是 Browser process 裡的 thread；現在有每個 process 都讀同一個的時鐘 `TimeTicks`、還在同一個 process 裡的 data pipe（`data_pipe`），channel 的核心（`core`）、訊息的值怎麼變成位元組（`codec`）、把它們組起來的端點（`endpoint`）、同一個 process 裡的平台（`local`），和作業系統的平台層（`platform`：現在有 macOS 的共用記憶體、Mach 訊息和 rendezvous，還沒有通道）。

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
    /// 讓讀傳輸的那條 thread 之後呼叫 `Receiver::woken`。
    fn wake(&self);
}

let (sender, mut receiver) = ipc::core::channel::channel(transport, config);

// 任何 thread：不等。
sender.send(destination, &payload, handles)?;

// 讀傳輸的那一條 thread（IO thread）的迴圈：
if let Some(gone) = receiver.feed(&bytes, handles, |message| dispatch(message)) { /* 通道結束了 */ }
if let Some(gone) = receiver.peer_gone() { /* 平台說對方不在了 */ }
if let Some(gone) = receiver.woken() { /* `wake` 之後：別的 thread 關掉了通道 */ }
sender.flush(); // 傳輸又有空間時
```

- **送不等**：傳輸收得下就當場寫；收不下的（或前面已經有在等的）排進佇列，照順序。一把鎖守著訊息的編號和寫出去的順序，所以好幾條 thread 同時送時，線上的序號還是連續的、一則訊息不會被另一則插進中間。一次 `write` 只給一則訊息的位元組：Mach 訊息那種「整則或沒有」的傳輸不會拿到半則或兩則。
- **佇列有上限**（`MAX_QUEUED_BYTES`，256 MiB，兩則最大的訊息）：等著的位元組超過它，就是對方不讀了，通道關掉（`Gone::QueueFull`），等著的連同 handle 丟掉。對方「多久沒回應」不是這一層的事。
- **佇列從空變成有東西時 `wake` 一次**（IO thread 開始等傳輸有空間，有了就 `flush`），關掉通道時也 `wake`。`wake` 一律在放開鎖之後呼叫。
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

訊息的值和內容的位元組之間（Mojo 產生的序列化程式和它的 message context 的位置）：postcard，外面包一層 serde 的 Serializer 和 Deserializer，其他的值原樣轉過去，只做兩件事。

```rust
let mut payload = Vec::new();
ipc::codec::encode(&message, &mut pool, &mut payload)?;     // pool：Place
let message: Msg = ipc::codec::decode(&payload, &mut table)?; // table：Resolve
```

- **大的位元組走 region**：值用 `serialize_bytes` 寫的一段位元組（`protocol::BigBuffer` 就是這樣寫自己的）到 64 KiB（`MIN_REGION_LEN`）時，向 `Place` 要一塊；有的話內容寫進 region，訊息裡只有編號和長度。沒有空塊、或比最大的 region 還長，就照樣放在訊息裡。寫出來的是一個兩種 variant 的 enum（放在訊息裡的、在 region 裡的），所以不走 region 的位元組前面多一個位元組。
- **值寫到一半失敗**：已經放進 region 的還回去，`out` 回到原來的樣子。
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

// IO thread：阻塞到通道結束，回傳理由。
let gone = endpoint.run(handler);

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
}
```

- **`Platform`**：平台只做這幾件事，裡面沒有判斷。`write` 和 `wake`（`Transport`）、`wait`（等到有事做：讀到位元組和 handle、又有空間、被叫醒、對方不在了）、`map`（對方給的 handle 變成記憶體）、`create`（建一塊 region：這一端的 mapping 和給對方的 handle）、`shut`（通道結束時關掉傳輸，對方的平台才知道）。
- **迴圈的順序只有一份**（`turn`）：讀到的餵給通道，核心自己的訊息當場處理、其餘的交給 `handler`；這一批讀過的 region 合成一則 `Released`；建好排著要給對方的 region；管道的來源有多少、對方有多少空間就送多少；最後寫出等著的。`run` 就是「等、`turn`」。
- **型別在 IO thread 上解**：`handler` 看目的地決定型別，`decode` 時從對方的 region 複製出來。`decode` 的錯誤傳回去，通道就關掉。
- **先告知、再提到**：一則訊息的大位元組放進 region、新建的 region 的 `Granted`、訊息本身，在同一把鎖底下依序送出，別的 thread 的訊息插不進中間。`open_pipe` 先送 `PipeOpened` 才回傳編號。
- **只有能附 handle 的一端建 region**（`Config::may_attach`，Browser）：兩個方向的 region 都是它編號。它自己送大資料時，沒有空塊就當場建一塊（不超過 `region_limit`）；對方要寫的那幾塊由它用 `grant_to_peer` 給。另一端沒有空塊時大資料就放在訊息裡。
- **送不出去的訊息**（通道已經結束、太長）放進 region 的那幾塊當場還回池子。
- **管道**：來源的 `Consumer` 交給 `open_pipe`，它有資料時叫醒 IO thread；收的一側收到 `PipeOpened` 時建一條 `data_pipe`、掛上回報的掛勾，讀的一端等訊息提到它時由 `take_pipe` 拿走。讀的一端 drop 就是取消。對方不被信任時不讓它開管道（`peer_may_open_pipes`）：每一條都是這一端要留的記憶體。
- **通道結束**：`turn` 回 `Some(Gone)` 一次；管道的來源放掉（它們的寫的一端看到讀的不在了）、傳輸關掉，之後 `send` 回 `SendError::Gone`。

## `local`

同一個 process 裡的 `Platform`：兩端各在一條 thread 上，一端寫的原樣交給另一端，region 是一般配置的記憶體（`HeapMemory`）。端點的測試和 fuzzer 跑在它上面，Miri 也跑得起來。`pair(room)` 的 `room` 是一端最多堆多少位元組沒讀（測傳輸沒有空間的情況）；`poll` 是不等的 `wait`，測試用它一步一步推。

## `platform`

每個作業系統為通道做的事（Chromium 的 `mojo/core/platform` 和 `base/memory/platform_shared_memory_region_*`）：搬位元組和 handle、等、建和 map 共用記憶體。裡面沒有判斷，薄到可以逐行看：系統呼叫是工具看不進去的。

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
- `ReceiveRight`：一個 port 本身。`new` 建一個、`make_send` 做出通往它的 send right、`tell_when_no_senders` 請 kernel 在沒有 send right 剩下時通知。drop 時 port 就死了，通往它的 send right 變成 dead name。
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
- **不送 out-of-line 的記憶體**。
- 比緩衝區長的訊息：kernel 從 port 上拿掉、毀掉，回 `TooLarge`。`Buffer::for_body(n)` 的 body 上限就是 `n`。

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

## `unsafe` 的論證

| 地方 | 為什麼安全 |
| --- | --- |
| `time_ticks.rs` 的 `QueryPerformanceFrequency`、`QueryPerformanceCounter`（只在 Windows） | 傳進去的指標指向一個活著的 `i64`，呼叫只寫它、不留著它；Windows XP 以後這兩個呼叫不會失敗。頻率讀成 0 時當成 1，不會除以 0 |
| `core/region.rs` 的 `unsafe trait Memory` | 實作的人保證：值活著的期間 `ptr()` 起 `len()` 個位元組可讀（放進 `Pool` 的還要可寫），兩者不變；這個 process 裡沒有別的參照指進那一段。`Arc<M>` 的實作只是轉給裡面的值，`Arc` 讓它活著、不搬動 |
| `core/region.rs` 的 `Pool::put`（複製進 region） | 目的地是 `best_fit` 選的那一塊，它的長度至少是要寫的長度（Verus 證明的後置條件），`Memory` 保證那一段可寫；來源是這個 process 自己的 slice，和 region 不重疊 |
| `core/region.rs` 的 `Table::read`（從 region 複製出來、`set_len`） | 長度先和 region 的長度比過（超過的是 `OutsideRegion`）；目的地是剛 `reserve` 過的多餘容量，`set_len` 只涵蓋剛寫的那幾個位元組。對方可能同時在寫那一塊：讀到的是位元組，不當成有不變量的型別，複製出來之後才交給上層 |
| `platform/macos/memory.rs` 的 `mach_make_memory_entry_64`（`entry`） | 傳進去的是活到呼叫結束的兩個區域變數的指標（大小、回來的名字）；`from` 是這個 task 自己的 mapping 的位址或沒有。回 `KERN_SUCCESS` 才把名字當成 right（`SendRight::from_raw`：新的 send right，只有這裡拿著） |
| `platform/macos/memory.rs` 的 `mach_vm_map`（`map`） | 位址由 kernel 選（`VM_FLAGS_ANYWHERE`），寫進活到呼叫結束的區域變數；不蓋掉這個 task 已有的東西。成功才建 `Mapping` |
| `platform/macos/memory.rs` 的 `SendRight`（`from_raw`、`Drop` 的 `mach_port_deallocate`） | `from_raw` 是 `unsafe fn`：呼叫的人保證名字是這個 task 的一個 send right、沒有別人會放掉它。`Drop` 放掉一次；`into_raw` 用 `forget` 把放掉的責任交出去 |
| `platform/macos/memory.rs` 的 `Mapping`（`Drop` 的 `mach_vm_deallocate`、`Send`、`Sync`、`Memory`） | 範圍是 `map` 得到的那一段，只在 `drop` 解除一次。位址在每條 thread 上都一樣；經過 `Memory` 的讀寫都是整段位元組的複製。`Memory` 的保證：從 `map` 到 `drop` 都 map 著、不搬動、可讀；放進 `Pool` 的是 map 成可寫的（`create(_, false)` 的那一個、`map(_, _, true)` 的）。指標用 `with_exposed_provenance_mut` 從位址做出來：記憶體不是 Rust 配置的 |
| `platform/macos/port.rs` 的三種 right（`from_raw`、`Drop`、`ReceiveRight::new`、`make_send`、`tell_when_no_senders`） | `from_raw` 都是 `unsafe fn`：呼叫的人保證名字是這個 task 的那一種 right、沒有別人會放掉。每種的 `Drop` 用對應的呼叫放掉一次（send 和 send-once 是 `mach_port_deallocate`，receive 是 `mach_port_mod_refs` 減一）；`into_raw` 用 `forget` 交出責任。其餘的呼叫都是對自己的 receive right 做的，寫的是活到呼叫結束的區域變數。`mach_port_request_notification` 是自己宣告的（mach2 沒有），照 `<mach/mach_port.h>` 的原型 |
| `platform/macos/message.rs` 的 `Buffer::bytes`、`bytes_mut` | 把 `Vec<u32>` 看成位元組：字都初始化過，`u32` 的位元組就是位元組、任何位元組也都是合法的 `u32`，長度是 vector 的位元組數 |
| `platform/macos/message.rs` 的 `mach_msg`（送） | 緩衝區裡是一則完整的訊息（大小就是 header 寫的那個、在 vector 的範圍內），對齊和 header 一樣；它提到的 right 是呼叫的人交出來的（`into_raw` 之後才寫進去） |
| `platform/macos/message.rs` 的 `mach_msg_destroy`（送失敗、`discard`、收到排法不對的） | 訊息沒送出去時，它要搬的 right 還是這個 task 的，各放掉一次；只是複製的那個（`To::Port`）kernel 不會動。收到的訊息是 kernel 剛寫進緩衝區的完整的一則，`parse` 失敗時還沒有任何 right 被包成型別，所以每個只放掉一次 |
| `platform/macos/message.rs` 的 `mach_msg`（收）、`Parsed::own` | 緩衝區是 `capacity` 位元組，kernel 不會寫超過。`own` 只在整則訊息 `parse` 通過之後才把名字包成 right：receive right 是訊息搬給這個 task 的，其餘的是 send right（`parse` 不放行別種），reply 是 send-once（`parse` 檢查過） |
| `platform/macos/message.rs` 的 `audit_token_to_pid`（libbsm） | 傳值進去，讀其中一個字 |
| `platform/macos/rendezvous.rs` 的 `bootstrap_check_in`、`bootstrap_look_up` | 傳進去的是活到呼叫結束的 C 字串和一個放名字的區域變數。成功才把名字包成 right：check-in 給的是 service 的 receive right，look-up 給的是 send right |
| `core/region.rs` 的 `HeapMemory`（`Box::into_raw`、`from_raw`、`Send`、`Sync`） | 指標來自 `new` 裡的 `Box::into_raw`，只在 `drop` 還回去一次；配置是這個值自己的，經過 `Memory` 的讀寫都是整段位元組的複製，兩端同時複製和跨 process 的 mapping 是同一種情況 |

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

`tests/it/region_fuzz.rs`：fuzzer 跑的另一段檢查（`fuzz/fuzz_targets/region.rs`）。輸入當成三樣東西：一則提到 region 的訊息的內容（不管它寫什麼編號和長度，讀都留在 region 裡面，ASan 看著；解得出來的值再寫、再讀要一樣）、一則 `Control`（解得出來的再寫要是同樣的位元組）、一串對池子和表的操作（加、放、拿、還、對方還、拿掉、讀），每一步都對著一個簡單的模型：結果一樣、放進的是最短的放得下的、出去了的那一塊的內容沒有變、總量不超過上限。重播 `fuzz/regressions/region/`。

`src/core/control.rs` 的單元測試：七種訊息都讀得回來、種類的位元組各不相同、每個欄位在哪裡；沒有人會寫的 20 種內容都是壞訊息。

`src/core/pipe.rs` 的單元測試：送的不超過空間、被拒絕的不算數；回報的比送的多是壞訊息；一則最多一個 chunk，容量 0 的管道放得下一個位元組；每條管道有自己的編號和額度；給結束的管道的回報和取消丟掉、給沒開過的是壞訊息；讀的一端每讀一半回報一次（奇數的容量、最小的幾種）；六種容量乘六種讀法，寫的一端額度用完之後一定等得到回報；讀的一端分得出結束的和沒開過的；編號不重開。

`tests/it/pipe_fuzz.rs`：fuzzer 跑的第三段檢查（`fuzz/fuzz_targets/pipe.rs`）。寫的一端、讀的一端（真的 `data_pipe`）和中間編成位元組的訊息，照輸入的步驟走：送、結尾、把一則交給對方、讀、取消、和輸入編出來的回報。照規矩的讀的一端：手上不超過容量、讀到的就是寫的、照順序、兩端都不把對方當成壞的，最後一定讀得完。編出來的回報：寫的一端只接受不超過它送的。重播 `fuzz/regressions/pipe/`。

```sh
python3 scripts/test.py ipc tla      # crates/ipc/tla/ 的三個 TLA+ 模型（TLC）
```

`tla/Channel.tla`（通道的一端：佇列的上限、結束只報告一次、理由是第一個、之後不交出訊息、沒有東西留著等）、`tla/Regions.tla`（region 給哪個 peer、先告知再提到、寫進去、對方真的讀到那則訊息時才還、peer 不在了之後不轉給別人）、`tla/Pipe.tla`（管道的額度、回報的門檻、結尾和取消在路上交錯、亂回報的讀的一端、不會卡住）。`tla/check.py` 用 TLC 檢查每個模型成立，再把每一部分拿掉（模型的 `Mutation`），確認 TLC 找得到列出來的那個反例。TLC 的 jar 第一次跑時下載（釘住版本和 SHA-256，放在 `target/tla/`），要有 Java。

`src/platform/macos/memory.rs` 的單元測試（只在 macOS；同一個 process 裡把 handle 再 map 一次當成對方）：建的一端寫的、對方讀得到，而且是同一塊記憶體、不是當時的複本（handle 建好、map 好之後才寫的也看得到）；只能讀的 handle map 成可寫會失敗；對方寫的、建的一端讀得到；唯讀的 mapping 改不成可寫（對方的、建的一端對「對方寫」那一塊的、可寫的 region 唯讀 map 的）；有 mapping 在，放掉 handle 記憶體還在；map 超過長度失敗、少於可以；不是整頁的長度不建；kernel 拒絕的呼叫不給 handle；不是 region 的 port map 不了；handle drop 就是放掉 right、`into_raw` 之後不放；mapping drop 就是解除（map 又 drop 4 GiB，位址空間沒有長大；留著的時候有長大）；池子和表接在真的 region 上。這一組重複跑 300 次、整組單元測試平行跑 50 次都過。

`src/platform/macos/message.rs` 的單元測試（同一個 process 裡自己送給自己）：幾種長度的 body 原樣到、送的人是這個 process；沒有訊息時逾時；right 照順序到、各自還是原來那一種（對它送的訊息從原來的 port 出來）；回答送到發問的人指定的 port；沒回答就丟掉時 kernel 會通知（送的人是 0）；送到死掉的 port 送不出去、帶的 right 被放掉；收到又丟掉的 right 被放掉、receive right 丟掉 port 就死了；超過緩衝區的 body、超過 16 個 right 不送，right 被放掉；比緩衝區長的訊息被拒絕並且從 port 上拿掉；佇列滿了、不等的送會被拒絕，讀走一則就有位置；排法不對的六種（太長的 body、out-of-line 的記憶體、out-of-line 的 port、send-once 的 right、太多 right、不是回一次的 reply）都被拒絕、帶來的被放掉，之後 port 照常。

`src/platform/macos/rendezvous.rs` 的單元測試：不是問題的訊息（id 不對、沒有回答用的 port、有內容）不被回答、回答的 right 被放掉，登記的東西還在；真的問題拿得到，而且只有一次。

`tests/it/rendezvous.rs`（只在 macOS；測試把自己再執行一次當成 child，用環境變數告訴它做什麼）：child 照順序拿到登記的 right，而且真的能用（它寫到測試的 port，kernel 說寫的人是 child 的 pid；搬給它的 receive right 收得到測試送的，它看到送的人是 host）；比登記早問的，登記時拿到；回答過的再問被拒絕；沒登記的 process 拿不到（同時有別的 child 登記著也一樣）；登記之後收回，child 拿不到、right 被放掉；同一個 process 的第二個問題擠掉第一個；沒有 server 的 host 是 `NoServer`；一個 process 只能有一個 server。這一組重複跑 30 次、整個整合測試 15 次都過。

`tests/it/endpoint.rs`（兩個端點接在 `local` 上，一端是能建 region 的 host、一端是 child）：訊息照順序到各自的目的地；host 的大位元組走它自己建的 region、對方讀完再用、同時送兩筆就多一塊；到了上限就放在訊息裡；child 不建 region、拿到 host 給的之後才用；管道的位元組照順序、讀的一端不讀就只送容量那麼多、讀了才繼續；讀的一端走了來源就停、通道照常；傳輸沒有空間時什麼都不會永遠卡住；關掉之後兩端都結束、送不出去、放進 region 的還回來。亂送的 child（一條裸的通道，送什麼由測試決定）：不認得的目的地、解不出來的值、還沒寫過或還兩次的 region、提到沒給過它的 region、超過長度、自己附 handle、沒有 handle 的 `Granted`、開管道、對沒開過的管道回報取消送資料、回報超過、提到沒有的管道、不是 `Control` 的內容，host 都關掉通道、說出理由，之後不再交出任何訊息。亂送的 host 對 child 也一樣（超過容量的資料、重開的編號、長度不對或重複的 region）。兩條 thread 各跑一個 `run`，三條 thread 同時送訊息和大位元組、加一條 900 KiB 的管道，全部到齊之後 host 關掉，兩邊的理由各是 `Closed` 和 `PeerClosed`。

`tests/it/endpoint_fuzz.rs`：fuzzer 跑的第四段檢查（`fuzz/fuzz_targets/endpoint.rs`）。host 的端點對一個照輸入送任何 frame 的 child（任何目的地、有沒有 handle），host 自己同時送大位元組、開管道、給 region。不能 panic、不能讀到 region 外面；說了通道結束之後不再交出訊息；region 的總量不超過上限。重播 `fuzz/regressions/endpoint/`。

`tests/it/frame_fuzz.rs`：fuzzer 跑的那一段檢查（`fuzz/fuzz_targets/frame.rs` 直接引用這個檔案），輸入是對端可能送的任何位元組、怎麼切、每一段旁邊附幾個 handle。任何輸入都不能 panic；同樣的位元組不管怎麼切，交出來的訊息和結局一樣；序號從 0 連續；沒有權限的一端的 handle 一個也到不了上層；留著的不比到了的多；每個 handle 不是在 `Decoder` 手上就是已經關掉。測試跑一組種子，並重播 `fuzz/regressions/` 裡 fuzzer 找到過的輸入。

```sh
python3 scripts/test.py ipc fuzz [--minutes M]   # cargo-fuzz，每個 target 跑 M 分鐘（預設 5）
python3 scripts/test.py ipc miri                 # 核心的單元測試和端點資料量小的測試在 Miri 底下
```

兩者用 `fuzz/rust-toolchain.toml` 釘的 nightly（和 elise 的同一個）。fuzzer 失敗時會印出輸入在哪裡：連同修正放進 `fuzz/regressions/`，取一個說明它找到什麼的名字。

`tests/it/data_pipe.rs`：寫進去的照順序讀出、一次最多讀 `max`、寫的一段比空間大時只放得下的進去、讀走才放其餘的（polling 的結果是確定的，不靠 thread 和時間）、結尾在資料之後、consumer 不在了寫失敗（等著的 write 被喚醒）、watch 的通知在什麼時候、幾次；另外一支跨 thread 的：tokio 上的 producer 經過 4 KiB 的管道送 300 KB。

`tests/it/time_ticks.rs`：時鐘和 `Instant` 一起往前走、相差不到幾毫秒；讀數序列化之後不變；另一個 process（測試自己的執行檔）的讀數落在這個 process 前後兩次讀數之間。Windows 的那一支只在 CI 上編譯和執行。
