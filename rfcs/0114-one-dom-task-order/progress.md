# RFC 0114 的進度：DOM 的 task 只有一套順序，worker 和主 thread 一樣

開始日期：2026-11-25

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. scheduler 的核心抽到 `base`，主 thread 改用它，行為不變（已決定的事項第 1、5、7 條） | 做完，待審查 | 2026-11-25 |
| 2. worker 的 loop 改用 scheduler（第 6、8 條） | 做完，待審查 | 2026-11-25 |
| 3. 拿掉 `dom` 的順序 | 沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| scheduler 的核心：順序、延後的 task 取的時候才排、等待、同步等回答、`take_matching`、`put_back`、`post_with`、`close` | `base` 的 `sequence_manager::tests`（`cargo test -p base --lib sequence_manager`） |
| worker 的 loop：DOM 的 task 是記號、`close()` 之後不跑排好的 task、`importScripts()` 等的期間其他 task 留著、script 跑完之前 `Worker` 的訊息留著 | `shell` 的 `workers::`（`a_worker_closed_in_a_task_runs_none_queued` 必壞、`a_worker_s_closed_worker_s_messages_come`）、`event_loop::`、`messaging::`；WPT 的 `workers/`、`webmessaging/`、`html/webappapis/timers/` |
| 主 thread 的 scheduler 行為不變 | `renderer` 的 `task_queue::tests`、`shell` 的整合測試（`event_loop::`、`workers::`、DevTools 的）照舊通過 |

## 做過的檢查

### 第 2 段（2026-11-25，Linux：Fedora 45，debug build，`bb9a6c09` 之上）

**階段結束的檢查**（第 1 段結束時只跑了 `base`、`renderer` 的 clippy，這裡一起補）：`cargo fmt` 改了第 1、2 段的兩個檔案（`sequence_manager.rs`、`task_queue.rs`）的排版；`cargo clippy --workspace --all-targets -- -D warnings` 通過；`cargo test --workspace` 通過（60 個 binary，4865 個通過、1 個 ignored）；`sanity.py` 的 `spans`、`boundary`、`tests`、`numerics`、`docs` 通過。

**做了什麼**：worker 的 thread 有自己的 `WorkerScheduler`（`base::sequence_manager`，queue 是 `Control`、`Owner`、`Browser`、`Dom`、`Internal`），route 和 `Running` 拿著它，`WorkerTask` 照種類進 queue；worker 的 DOM 裝 `TaskWaker`，`WorkerEngine` 多 `set_task_waker`、`run_dom_task`、`number_port_messages`（`dom_js` 照 window 的 `run_dom_task` 寫）；platform task 照主 thread（同時只排一個、延後的 `post_at`）；script 跑之前（module worker 的 graph 跑完之前）不取 `Owner` 的 queue，取代 `early`；`importScripts()` 照第 8 條；`close()` 在迴圈開頭看（主 script 和每個 task 之後），清掉 scheduler、結束；每個 task 之後 `number_port_messages`。原本的 `Inbox`、`put_off`、`platform_due`、`early` 拿掉。

**找到的 bug（以前就有，被一輪的順序蓋住）**：worker 的 engine 的 `worker_closed` 呼叫 `terminate_worker_object`（丟掉它的 worker 送的、還沒分派的訊息），主 thread 的呼叫 `close_worker_object`（照樣送到）。舊的 loop 在處理 `Closed` 之前下一輪就分派了訊息；照 scheduler 的順序，`Closed` 先跑，WPT 的 `webmessaging/message-channels/worker-post-after-close.any.worker.html`、`worker.any.worker.html`、`workers/nested_worker_close_self.worker.html` 逾時。改成 `close_worker_object`，加 `shell` 的 `a_worker_s_closed_worker_s_messages_come`。

**測試**：`workers::a_worker_closed_in_a_task_runs_none_queued`（兩個到期的 timer，第一個 `close()`：第二個不跑）在原本的程式上失敗（`a,b`），之後通過；`a_worker_s_closed_worker_s_messages_come`。`dom_js`、`renderer`、`shell`、`base` 的測試通過。順帶修 `automation_pipe` 的兩個 profiler 的測試把 `timeDeltas` 當成無號的（V8 的樣本偶爾不照順序，delta 是負的，整組跑時一次 `as_u64().unwrap()` panic；RFC 0109 第 2 段改寫時留下的）。

**WPT**（`workers/`、`webmessaging/`、`html/webappapis/timers/`，483 個測試）：第一輪作廢：跑到一半時我把產品的改動收起來重編舊版（確認必壞的測試），執行檔被換掉了。重跑：上面三個逾時（修好之後 0 個非預期），另外 5 個非預期的通過都不是這一段造成的：`negative-settimeout.any.worker.html`（`main` 上也是），`dedicated-worker-in-data-url-context.window.html` 的 4 個（舊的程式在這台機器上也是 4/4 通過；2026-11-24 `main` 的 Nightly 是 0/4，差在環境或那之後 merge 的改動，沒有查），期望值都沒改。

**mutation test**（`mutants-2.toml`，8 個，都抓到）：DOM 的 task 拿不到記號、記號什麼都不跑、`close()` 之後照跑、script 跑完之前就取 `Worker` 的訊息、等 `importScripts()` 時別的回應被丟掉、別的 thread 的 port 訊息沒編號、關掉的子 worker 被當成 terminate。第一輪另有兩個沒抓到：等 `importScripts()` 時收到的 `Stop` 被丟掉是等價的（`terminate()` 先設旗標、打斷 script，主迴圈每次先看旗標；`Stop` 只是叫醒它），所以拿掉了放回去的那一行、也拿掉這個 mutant；platform task「取走時重設」拿掉沒有測試失敗，記進 `docs/test-gaps.md`（原本就有的缺口，描述改成現在的做法）。
### 第 1 段（2026-11-25，Linux：Fedora 45，debug build，`8cabbc17` 之上）

**做了什麼**：`base::sequence_manager`（`Spec`、`SequenceManager`、`Closed`）放 `task_queue.rs` 原本的 queue、sequence、延後 task 的 heap、`queue_due`、`earliest`、等待、同步等回答；`MainThreadScheduler` 改成建在它上面（`MainSpec`：`Queued` 帶 `interrupt`，`MainExtra` 是 `posted_once` 和 `shutting_down`，`taken` 清掉 `posted_once`）。`post_internal_once_at` 經過 `post_with`，`take_interrupting` 經過 `take_matching`，`put_back`、`sync_call`、`close` 照舊的意思。原本只為 DOM 和 renderer 自己的 queue 的延後 task 醒來，改成為「要的 queue」的延後 task 醒來：主 thread 的延後 task 只在這兩個 queue，等價。

**測試**：`base` 的 `sequence_manager::tests` 七個。`base`、`renderer`、`shell` 的測試通過；`sanity.py` 的六項、`base` 和 `renderer` 的 clippy 通過。

**mutation test**（`mutants-1.toml`，12 個）：第一輪 8 個抓到、3 個「hung」（沒有時限的等待在 mutant 底下等不到，測試卡住到 mutation 的時限）、1 個沒抓到。把三處等待改成有時限（等延後的 task 時要在 0.5 秒內回來、同步等回答放在別的 thread 上用 `recv_timeout`），那 3 個重跑都直接失敗。沒抓到的「沒在等的 queue 的延後 task 也叫醒等待」是等價的：多醒一次、照樣回去等，記進 `docs/test-gaps.md`。
