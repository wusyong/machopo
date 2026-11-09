# ipc

對應 Chromium 的 `ipc/` 和 `mojo/`：machopo 的各個 process 之間共用的東西。renderer 現在還是 Browser process 裡的 thread，channel 和共用記憶體等拆成 process 時才放進來；現在只有每個 process 都讀同一個的時鐘 `TimeTicks`。

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

## `unsafe` 的論證

| 地方 | 為什麼安全 |
| --- | --- |
| `time_ticks.rs` 的 `QueryPerformanceFrequency`、`QueryPerformanceCounter`（只在 Windows） | 傳進去的指標指向一個活著的 `i64`，呼叫只寫它、不留著它；Windows XP 以後這兩個呼叫不會失敗。頻率讀成 0 時當成 1，不會除以 0 |

## 測試

```sh
cargo test -p ipc
```

`tests/it/time_ticks.rs`：時鐘和 `Instant` 一起往前走、相差不到幾毫秒；讀數序列化之後不變；另一個 process（測試自己的執行檔）的讀數落在這個 process 前後兩次讀數之間。Windows 的那一支只在 CI 上編譯和執行。
