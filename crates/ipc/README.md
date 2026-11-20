# ipc

對應 Chromium 的 `ipc/` 和 `mojo/`：machopo 的各個 process 之間共用的東西。renderer 現在還是 Browser process 裡的 thread；現在有每個 process 都讀同一個的時鐘 `TimeTicks`、還在同一個 process 裡的 data pipe（`data_pipe`），和 channel 的核心（`core`，還沒有接上任何傳輸）。

`unsafe` 只放在這裡和 `bindings`（`python3 scripts/sanity.py unsafe` 檢查）：這裡的是作業系統的 API。

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
- 目的地認不認得、送出的佇列、斷線，不在這一層。

## `unsafe` 的論證

| 地方 | 為什麼安全 |
| --- | --- |
| `time_ticks.rs` 的 `QueryPerformanceFrequency`、`QueryPerformanceCounter`（只在 Windows） | 傳進去的指標指向一個活著的 `i64`，呼叫只寫它、不留著它；Windows XP 以後這兩個呼叫不會失敗。頻率讀成 0 時當成 1，不會除以 0 |

## 測試

```sh
cargo test -p ipc
```

`src/core/frame.rs` 的單元測試：開頭的每個欄位在哪裡；三則訊息（有 handle 的、空的、兩個 handle 的）一次餵完、在每一個位置切成兩段、一次一個位元組，交出來的都一樣；只留沒到齊的那一則；長度的上限只看開頭就擋（剛好在上限的不擋）；謊報的長度不佔記憶體；序號跳號和重複；沒有權限的一端留不下任何 handle（開頭說有、開頭說沒有但附了、開頭說有但沒附）；handle 太多（一則的、堆著沒認領的）和不夠；壞訊息之後什麼都不留、答案不變；最後一個序號之後沒有下一則；`Encoder` 擋掉的不佔序號；兩條 thread 經過一個隨意切位元組的假傳輸（`core/fake.rs`，只在測試裡），收到的照順序、各一次。

`tests/it/frame_fuzz.rs`：fuzzer 跑的那一段檢查（`fuzz/fuzz_targets/frame.rs` 直接引用這個檔案），輸入是對端可能送的任何位元組、怎麼切、每一段旁邊附幾個 handle。任何輸入都不能 panic；同樣的位元組不管怎麼切，交出來的訊息和結局一樣；序號從 0 連續；沒有權限的一端的 handle 一個也到不了上層；留著的不比到了的多；每個 handle 不是在 `Decoder` 手上就是已經關掉。測試跑一組種子，並重播 `fuzz/regressions/` 裡 fuzzer 找到過的輸入。

```sh
python3 scripts/test.py ipc fuzz [--minutes M]   # cargo-fuzz，每個 target 跑 M 分鐘（預設 5）
python3 scripts/test.py ipc miri                 # 核心的單元測試在 Miri 底下（約 5 分鐘）
```

兩者用 `fuzz/rust-toolchain.toml` 釘的 nightly（和 elise 的同一個）。fuzzer 失敗時會印出輸入在哪裡：連同修正放進 `fuzz/regressions/`，取一個說明它找到什麼的名字。

`tests/it/data_pipe.rs`：寫進去的照順序讀出、一次最多讀 `max`、寫的一段比空間大時只放得下的進去、讀走才放其餘的（polling 的結果是確定的，不靠 thread 和時間）、結尾在資料之後、consumer 不在了寫失敗（等著的 write 被喚醒）、watch 的通知在什麼時候、幾次；另外一支跨 thread 的：tokio 上的 producer 經過 4 KiB 的管道送 300 KB。

`tests/it/time_ticks.rs`：時鐘和 `Instant` 一起往前走、相差不到幾毫秒；讀數序列化之後不變；另一個 process（測試自己的執行檔）的讀數落在這個 process 前後兩次讀數之間。Windows 的那一支只在 CI 上編譯和執行。
