# RFC 0069 的進度：和 Chrome 逐個 JS 函式比 Speedometer 的時間

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-21。完成：2026-10-22，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0069` 上做；完整的 20 個 suite 和 `docs/performance-gaps.md` 在收尾之後（第 18 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 fork 接出 `CpuProfiler`，kun 經過 automation pipe 交出 profile，量 kun 被拖慢多少 | 完成 | 2026-10-21 |
| 2 `--profile`：兩邊取樣、讀 profile、整次執行的對照；閒置的標記 | 完成 | 2026-10-21 |
| 3 只算計時的區間：頁面的時鐘，用同時錄的 trace 驗證 | 完成 | 2026-10-21 |
| 4 20 個 suite 跑 1 個 iteration 的預覽 | 完成 | 2026-10-21 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| kun 的 profiler | `V8PerIsolateData` 的 `CpuProfiler`：profile 是 DevTools 的 JSON、有跑過的函式，取樣照間隔，一次一個，isolate 走時還在取樣也不出事 | `cargo test -p bindings --test it cpu_profiles::` |
| kun 的 profiler | 閒置的標記：標成閒著的時間是 `(idle)`，忙著、不在 script 的是 `(program)`，開始取樣的 task 是忙著的 | `cargo test -p bindings --test it cpu_profiles::idle_and_busy_out_of_script` |
| automation pipe | `Profiler`：分頁的 renderer 取樣頁面的 script，profile 有頁面的函式和網址、取樣照 `setSamplingInterval`；錯誤（沒有在取樣時 `stop`、重複 `start`、間隔 0、renderer 已經不在） | `cargo test -p shell --test it automation_pipe::the_profiler_samples_the_pages_script` |
| automation pipe | renderer 等下一個 task 時是 `(idle)`，畫一個 frame 是 `(program)` | `cargo test -p shell --test it automation_pipe::the_profile_tells_waiting_from_rendering` |
| automation pipe | `Profiler.*` 的 JSON；wasm 的 engine 不能取樣 | `cargo test -p protocol --test it automation::`、`cargo test -p dom_wasm --test it engine::the_engine_cant_profile` |
| `--profile` | 開始的順序：等頁面好了才取樣、照 `PROFILE_INTERVAL`，頁面的時鐘，再讓 runner 開始；runner 等 profile | `python3 scripts/bench/speedometer/speedometer_test.py -k OpenProfiled -k test_profiles_of_both` |
| `--profile` | 讀 profile：樣本算給最內層有網址的 JS 函式；成員穿過 `glue.js`、不算 regular expression，沒有名字的是共用路徑；只算計時的區間（跨過邊界的照比例）；頁面的時鐘的範圍、中間、誤差上限，沒有樣本或對不上時停下；報告的順序、Timed 和區間外、整次執行的 | `python3 scripts/bench/speedometer/speedometer_test.py -k ProfileReading` |
| `--profile` | 兩個瀏覽器真的取樣假的 suite：`pageClock`、誤差上限、區間外、整次執行的 | `python3 scripts/bench/speedometer/speedometer_test.py -k test_profiles_of_both` |
| 量測 | kun 被拖慢的百分比和 Chrome 差 5 個百分點以內（第 8 條） | 手動：`details.md` 的第 6 節 |
| 量測 | 區間放到 profile 上的誤差在第 14 條的門檻內 | 手動：同時錄 trace 比（`details.md` 的第 8 節）；記在 `docs/test-gaps.md` |

## 做過的檢查

2026-10-21 到 22，Fedora 44（AMD Ryzen AI 9 HX 370），兩個瀏覽器都綁在快核（0–3、12–15）上。

- **fork**：`src/cpu_profiler.rs` 和 `src/binding.cc` 的五個函式（`a099261`），發成 `kun-v152.2.0-14`；做的期間設 `RUSTY_V8_ARCHIVE` 連結本機增量重編的 V8（`binding.cc`，8 秒），收尾時改用 release 的。
- **量測**（`details.md`）：
  - kun 被拖慢的程度（第 6 節）：kun 的 `enable` 一開始打開了詳細原始碼位置，看起來被拖慢得少；照 Chrome 只記下來之後，React-Redux Chrome +10.9%、kun +10.1%，Charts-chartjs +9.3%、+5.4%（第 9 條）。
  - 閒置（第 7 節）：kun 的 thread 原本一直是 V8 的 `IDLE`，rendering 算成 `(idle)`；照 Blink 標記之後，畫 5000 個段落的 frame 是 `(program)`、之後等 200 ms 是 `(idle)`。共用路徑的成員合成一個 node（第 12 條）。
  - 區間（第 8 節）：`Profiler.start` 前後的讀數相差 3–25 ms，換成頁面的時鐘；和同時錄的 trace 比，六輪，誤差中位數 0.03–0.49 ms、最大 0.52 ms，真正的位置總在範圍的下限（第 17 條留著取中間）。
  - 20 個 suite 的預覽（第 9 節）：每個 suite 的區間內的時間和 runner 的分數差 0.1–0.7 ms；DOM 呼叫裡強迫的 layout 是最大的一類。
- **找到的問題**：
  - 還在取樣時 `Dispose` profiler 的取樣 thread 還在跑（`an_isolate_goes_while_profiling` 抓到 SIGSEGV）：fork 的 `CpuProfiler` 先停再 `Dispose`。
  - 執行中最佳化（OSR）的程式，profile 開始之前的不記（第 10 條）：自動化的測試每一輪用新的函式。
  - runner 在 `DOMContentLoaded` 就開始，和 profile 的開始有競賽：改成 runner 等 profile。
  - V8 的 `CpuProfile::Serialize` 把 `startTime`、`endTime` 截成 32 位元：記進 `docs/limitations.md`，修它在「之後的可能」。
  - 成員表一開始算給成員呼叫的 builtin、把 `glue.js` 和 regular expression 當成成員：改成 JS 函式正下方的原生 node，穿過 `glue.js`。
  - kun 沒有 scroll anchoring（`overflow-anchor`）：記進 `docs/limitations.md`。
  - kun 的 Charts-chartjs 從約 170 ms 變成約 300 ms：第 1 段的 commit 也一樣（`--against`），是機器的狀態（canvas 的 `fill`、`stroke` 的等待），不是這份的改動。
- **Mutation test**（scratchpad 的 `mutants/0069-{1,2,2b,3,3b,4}.toml`）：
  - 第 1 段 13 條，抓到 11 條；漏的 2 條記進 `docs/test-gaps.md`：renderer 沒有 `RendererFlags::automation` 也處理（沒有直接送 message 的測試工具）、renderer 不在時不回答還在等的 profiler 請求（排不出那個時機）。
  - 第 2 段 15 條，抓到 10 條；補了測試之後重跑 4 條都抓到（開始取樣的 task 留在閒著、Chrome 多的排在前面、導覽之前就開始取樣、沒有設取樣的間隔），剩下的 1 條等價（沒有 profile 時也標記 isolate），記進 `docs/test-gaps.md`。
  - 第 3 段 18 條：runner 不等的那條一開始抓不到（假的 runner 開始得晚），改成檢查 runner 送回的開始時間；之後 15 條抓到，補了測試之後重跑 2 條都抓到（只算最後一個區間、整次執行的就是區間內的），剩下的 1 條（runner 太早開始時不擋）只靠審查，記進 `docs/test-gaps.md`。
  - 成員表的修正 3 條都抓到。
- **跑過的檢查**：
  - 收尾（2026-10-22，rustc 1.99.0，連結 release 的 `kun-v152.2.0-14`，不設 `RUSTY_V8_ARCHIVE`）：build、`cargo test --workspace` 3485 個通過（跳過 2 個，91 秒）、`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy 通過；`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs` 通過；`speedometer_test.py` 108 個通過（跳過 2 個，79 秒）。
  - 每一段做完時也各跑過一次同樣的檢查（`cargo test --workspace` 3483、3485、3485 個通過）。
