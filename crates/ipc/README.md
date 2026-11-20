# ipc

對應 Chromium 的 `ipc/` 和 `mojo/`：machopo 的各個 process 之間共用的東西。renderer 現在還是 Browser process 裡的 thread，channel 和共用記憶體等拆成 process 時才放進來；現在有每個 process 都讀同一個的時鐘 `TimeTicks`，和還在同一個 process 裡的 data pipe（`data_pipe`）。

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

## `unsafe` 的論證

| 地方 | 為什麼安全 |
| --- | --- |
| `time_ticks.rs` 的 `QueryPerformanceFrequency`、`QueryPerformanceCounter`（只在 Windows） | 傳進去的指標指向一個活著的 `i64`，呼叫只寫它、不留著它；Windows XP 以後這兩個呼叫不會失敗。頻率讀成 0 時當成 1，不會除以 0 |

## 測試

```sh
cargo test -p ipc
```

`tests/it/data_pipe.rs`：寫進去的照順序讀出、一次最多讀 `max`、寫的一段比空間大時只放得下的進去、讀走才放其餘的（polling 的結果是確定的，不靠 thread 和時間）、結尾在資料之後、consumer 不在了寫失敗（等著的 write 被喚醒）、watch 的通知在什麼時候、幾次；另外一支跨 thread 的：tokio 上的 producer 經過 4 KiB 的管道送 300 KB。

`tests/it/time_ticks.rs`：時鐘和 `Instant` 一起往前走、相差不到幾毫秒；讀數序列化之後不變；另一個 process（測試自己的執行檔）的讀數落在這個 process 前後兩次讀數之間。Windows 的那一支只在 CI 上編譯和執行。
