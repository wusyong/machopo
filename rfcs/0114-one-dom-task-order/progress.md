# RFC 0114 的進度：DOM 的 task 只有一套順序，worker 和主 thread 一樣

開始日期：2026-11-25。完成：2026-11-25，Linux（Fedora 45、AMD Ryzen AI 9 HX 370；debug build，Speedometer 是 `perf` profile）；macOS 和 Windows 沒有跑，由每天的 CI 跑。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. scheduler 的核心抽到 `base`，主 thread 改用它，行為不變（已決定的事項第 1、5、7 條） | 做完 | 2026-11-25 |
| 2. worker 的 loop 改用 scheduler（第 6、8 條） | 做完 | 2026-11-25 |
| 3a. 照編號拿、拿掉 `target_task` 和時鐘的換算（第 2 條） | 做完 | 2026-11-25 |
| 3b. port 的狀態機，拿掉補號、重送、每個 task 之後的編號（第 3、11 條） | 做完 | 2026-11-25 |
| 3c. debug build 的偵測只問「有沒有」，主 thread 和 worker 都裝（第 4 條） | 做完 | 2026-11-25 |
| 3d. 拿掉 `next_task`、一輪的機制和 engine 只剩測試在用的方法，測試改成經過 `base` 的 scheduler 跑（第 5、9、10、12 條） | 做完 | 2026-11-25 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| scheduler 的核心 | 跨 queue 照 post 的順序、延後的 task 取的時候才排、照給的時刻取、等待和時限、同步等回答、`take_matching`、`put_back`、`post_with`、`close` | `base` 的 `sequence_manager::tests`（8 個） |
| 主 thread | scheduler 建在核心上，行為不變 | `renderer` 的 `task_queue::tests`、`shell` 的 `event_loop::`、`workers::`、DevTools 的測試 |
| worker 的 loop | DOM 的 task 是記號；`close()` 之後排好的不跑（必壞）；子 worker 自己關閉時它送的訊息照樣到；`importScripts()` 等的期間其他 task 留著；script 跑完之前 `Worker` 的訊息留著 | `shell` 的 `workers::`（`a_worker_closed_in_a_task_runs_none_queued`、`a_worker_s_closed_worker_s_messages_come`）、`event_loop::`、`messaging::`；WPT 的 `workers/`、`webmessaging/`、`html/webappapis/timers/` |
| 照編號拿 | 每一種 task 不管排第幾都照編號拿；UA 的 timer 和 idle callback 的 timeout 照記號的順序 | `shell` 的 `event_loop::a_timer_set_after_an_earlier_ones_time_runs`、`a_ua_timer_set_after_an_earlier_ones_time_runs`、`idle_timeouts_run_earliest_due_first`；`dom` 的 `tasks::`、`window::` |
| port 的狀態機 | 啟用前不編號、啟用時照順序編；同一條 thread 送的在別的 thread 先送來的之後；傳走時編過號的跟著走、在新的地方啟用前不編；關掉時叫醒、再也收不到的 port 離開清單 | `dom` 的 `message_ports::`（8 個）；`shell` 的 `event_loop::a_ports_message_waits_for_it_to_be_enabled`、`a_moved_ports_messages_come_in_order`、`a_shipped_ports_messages_wait_for_its_new_port`、`workers::two_workers_talk_through_a_port_while_the_page_is_busy`；WPT 的 `webmessaging/` |
| debug build 的偵測 | 六種 task 各自算到期、port 啟用前不算 | `dom` 的 `tasks::a_task_of_each_kind_is_due`、`a_port_s_message_is_no_task_due_before_its_port_is_enabled`；偵測本身是檢查（`docs/test-gaps.md`） |
| 測試經過核心 | `TaskTokens` 的記號、測試的時鐘、`Env` 和 worker 的測試照記號跑 | `dom`、`dom_api`、`dom_js`、`dom_wasm` 的整合測試 |
| 效能 | Speedometer 沒有變慢 | `bench.py speedometer --against main`（RFC 0012 的 `progress.md`） |

## 做過的檢查

**最後的驗證**（2026-11-25，`4de628fb`）：`cargo test --workspace`（60 個 binary，4869 個通過，1 個 ignored）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的每一項通過。WPT（`workers/`、`webmessaging/`、`html/webappapis/timers/`，483 個）在第 2 段、3b、3d 之後各跑一次：0 個退步；非預期的通過都在這份之前就有（`negative-settimeout.any.worker.html`，`dedicated-worker-in-data-url-context.window.html` 的 2 到 4 個：舊的程式在這台機器上也通過，2026-11-24 `main` 的 Nightly 是 0/4，差在環境或之後 merge 的改動，沒有查），期望值都沒改。之後只改了文件。

**Speedometer**（使用者要求的，2026-11-25，`4de628fb` 對 `main` 的 `8cabbc17`，輪流各 3 次）：20 個 suite 的幾何平均 +0.2%，各 suite −4.5% 到 +3.9%、都在起伏裡；記憶體的峰值 −1.7% 到 +2.1%。數字在 RFC 0012 的 `progress.md`。

**mutation test**（每一段的 `mutants-*.toml`，共 58 個）：

- 第 1 段（12 個）：第一輪 3 個「hung」（沒有時限的等待在 mutant 底下等不到）：三處等待改成有時限之後都直接失敗。等價的 1 個：沒在等的 queue 的延後 task 也叫醒等待（多醒一次，`docs/test-gaps.md`）。
- 第 2 段（8 個，都抓到）：DOM 的 task 拿不到記號、記號什麼都不跑、`close()` 之後照跑、script 跑完之前就取 `Worker` 的訊息、等 `importScripts()` 時別的回應被丟掉、關掉的子 worker 被當成 terminate 等。等價的「等 `importScripts()` 時收到的 `Stop` 被丟掉」：拿掉了放回去的那一行。platform task「取走時重設」拿掉沒有測試失敗，記進 `docs/test-gaps.md`。
- 3a（11 個，9 個抓到）：照編號拿換成拿第一個、`task_numbered` 的每一種找不到。補了 UA 的 timer 和 idle callback 的 timeout 的兩個測試。沒抓到的兩個（訊息、idle 的 timeout 照編號換成拿最前面的）是等價的，3d 拿掉「拿第一個」的程式之後 mutant 也不存在了。我一開始把 idle 的「拿第一個設的」也當成等價，加了 mutant 分開兩種才改對。
- 3b（12 個，11 個抓到）：第一輪 5 個沒抓到，補了 `dom` 的 `a_message_from_here_comes_after_those_from_another_thread`、`shell` 的 `a_shipped_ports_messages_wait_for_its_new_port`，並讓 `a_port_whose_other_end_closed_gets_what_came_before` 留住 context（之前 context 自己被回收，「不再留住 port」的檢查不成立）之後抓到 4 個；`arrived` 先看在不在跑是等價的（`docs/test-gaps.md`）。
- 3c（9 個，都抓到）：每一種換成永遠不到期或永遠到期、window 的訊息和 port 的訊息分開。
- 3d（6 個，都抓到）：測試的時鐘換成真的、`instant_in` 不加 time origin、兩種訊息找不到、`run_dom_task` 不用給的時間、`run_task` 不檢查時間。另一個「沒有 timeout 的 idle callback 也照編號找得到」是等價的（它有編號但沒送過記號），那個條件拿掉了。

**找到的 bug（之前就有，被一輪的順序蓋住）**：worker 的 engine 處理子 worker 自己關閉時呼叫 `terminate_worker_object`（丟掉它送的、還沒分派的訊息），主 thread 的呼叫 `close_worker_object`。照 scheduler 的順序 `Closed` 先跑，WPT 的 `webmessaging/message-channels/worker-post-after-close.any.worker.html`、`worker.any.worker.html`、`workers/nested_worker_close_self.worker.html` 逾時；改成 `close_worker_object`。順帶修 `automation_pipe` 的兩個 profiler 的測試把 `timeDeltas` 當成無號的（V8 的樣本偶爾不照順序）。

**跟著行為改寫的測試**（3d）：`dom` 的 `tasks::` 一輪的三個拿掉；interval 在被拿走時就重新排，排在它的 callback 排的 task 之前；`window::` 的三個 timer 測試照編號拿，「同時的照建立的順序」改成看編號和記號的時間；`dom_js` 的 `text_controls`、`canvas` 各一處要連 listener 和 callback 排的 task 跑完，改用跑到沒有為止的 helper；`dom_wasm` 的 guest 等待改用 `run_due_guests`。

**做錯的地方**：第 2 段跑 WPT 的期間，我把產品的改動收起來重編舊版（確認必壞的測試），執行檔被換掉，那一輪作廢、重跑。3c 的時候早跑了一次 `cargo fmt -p dom`（只重排了這份自己寫的幾行）。
