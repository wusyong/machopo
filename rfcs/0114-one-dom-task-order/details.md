# RFC 0114 的細節

## 現況

2026-11-25 的 `main`（`8fabaf5f`）讀程式整理的；行號是那時的。

### worker 的 loop

- `crates/renderer/src/worker_thread.rs` 的 `run`（649–921）：沒有 scheduler，收件匣是 `Receiver<WorkerTask>` 加一個 `put_off` 的 `VecDeque`（`Inbox`，366–389）。`WorkerTask`（68–86）：`Message`、`Stop`、`Browser`、`PortMessages`、`PlatformTasks`、`Child`。
- script 跑之前只等 `DedicatedWorkerScript`（705–719），`Message` 先存在 `early`，script 跑完才交給 engine（812–817、886–888）。
- 每一輪（819–912）：看 `stopping`、`closing`；`engine.run_due_tasks()`（一輪的 DOM task）；`engine.run_platform_tasks()`；整理 `platform_due`（V8 的 platform 的 waker 填的 `Arc<Mutex<Vec<Instant>>>`，673–685）；等 `min(engine.until_next_task(), 下一個 platform task)`；收到的照種類處理（`Message` 交給 `post_message`，`Stop` 結束，`PortMessages`、`PlatformTasks` 什麼都不做、下一輪跑，`Child` 是子 worker 的事件，`Browser` 是 fetch 和 module 的回應）。
- `importScripts()` 等 Browser 時（`GlobalHost::load`，427–479）直接 `recv`：`Message`、`Child`、`Stop` 和別的 load 的訊息放進 `put_off`，`PortMessages`、`PlatformTasks` 丟掉。
- 叫醒只靠 channel 和等待的期限：port 是 `MainThreadScheduler::worker_port_waker`（`task_queue.rs` 569–578）送 `PortMessages`，platform task 是 waker 送 `PlatformTasks`。**worker 的 DOM 沒有 `set_task_waker`**（只有主 thread 的，`render_thread/devtools.rs` 240–242），`wake_for_task` 在 worker 上什麼都不做。
- `dom_js` 的 `worker.rs`：`until_next_task`（409–414）是 `dom_api::until_next_due`（`timer.rs` 255–267：有 element task 或 posted message 就是 0，否則到下一個到期的時間）；`run_due_tasks`（416–448）`begin_task_pass`、一直 `next_task(context, dom_now())` 跑 `run_task`、`end_task_pass`。

### 主 thread（對照）

- `task_queue.rs`：`MainThreadTask::DomTask { number }` 在 `TaskQueue::Dom`；`State`（174–200）是各 queue 的 `VecDeque` 和全域的 sequence，延後的 task 在 `delayed`（`(at, order)` 的 heap，204–228），`queue_due`（247–265）在取下一個 task 時才把到期的搬進 queue；`dom_task_waker`（454–468）、`post_at`（471–485，到期的當場 post）。
- `render_thread.rs`：`run` 先 `assert_no_dom_task_lost`（debug，692），`DomTask` 交給 `run_dom_task`；renderer 不呼叫 `run_due_tasks`。
- `dom_js` 的 `engine.rs`：`run_dom_task`（1523–1554）`target_task(Some(n))`、`task_numbered`、`now = dom_now().max(task.at)`（1535）、`run_task`、`target_task(None)`；`has_dom_task_due_for`（1556–1565）用 `next_task`；`run_due_tasks`（1467–1521）是多個 window 的一輪，只剩測試呼叫。`dom_wasm` 的同樣三個在 1101–1166（`max` 在 1142）。

### `dom` 的順序

- 狀態（`lib.rs`）：`task_sequence`、`tasks_queued`、`task_limit`、`task_target`（909–914），時鐘的原點（1044），`set_port_waker`、`set_task_waker`（1053、1063；後者的說明寫「一輪一輪跑的 host 不用」）。
- `tasks.rs`：`TaskQueue`（Timer、NativeTimer、IdleTimeout、Element、Message）；`NextTask` 和 `before`（`(at, number)`）；`dom_now`（ms）；`stamp_queued_task`（104：編號、叫醒）；`wake_for_task`（114–128：DOM 的 ms 換成 `Instant`）；`task_may_run`（133–141：有 target 時只有它，否則編號小於 `task_limit`）；`target_task`、`begin_task_pass`、`end_task_pass`、`task_numbered`、`queue_heads`（191–210，timer 的到期加上 `time_origin` 換到 DOM 的時鐘）、`next_task`（221–228）。
- timer（`window.rs`）：建立時編號、`wake_for_task`（1144、1160）；interval 每次重編、重送（`take_due`，1344–1374）；`first_timer`、`first_native_timer`（1379、1404）；`first_element_task`（1459–1466）。idle callback 的 timeout（`idle.rs` 68、83、179–193）。
- 訊息（`message.rs`、`message_pipe.rs`）：自己的訊息在 `posted_message` 編號（454）；port 的訊息在 `PortEnd::post`（120–145）只在對方的家是目前的 thread、前面的都編過號時編號，否則叫醒對方；`first_message`（578–611）照佇列的順序補號（`first_stamp`，156–163），只看第一個；port 啟用時重送已經編號的記號（`keep_port`，296–328，從 `start_message_port` 來）。
- 取消：只拿掉 DOM 裡的紀錄（`remove_timer`、`cancel_idle_callback`、關掉的 port 的佇列）；記號輪到時 `task_numbered` 找不到，`run_dom_task` 清掉 target、回 false。worker 上拿掉的就不會出現在 `next_task`。

### 誰用 `dom` 的順序

- `next_task`：`dom_js` 的 `worker.rs` 427、`engine.rs` 1483 和 1562，`dom_wasm` 的 1111 和 1165；`dom` 的測試（`crates/dom/tests/it/` 的 `tasks.rs`、`message_ports.rs`、`worker_global.rs`）。
- 一輪的函式：`dom_js` 的 `worker.rs`、`engine.rs`，`dom_wasm`；`dom` 的 `crates/dom/tests/it/tasks.rs`（五組）。
- `run_due_tasks`：worker 的 loop；`dom_js` 的測試（`tasks.rs`、`worker.rs`、`blobs.rs`、`frames.rs`、`termination.rs`）、`dom_wasm` 的 `engine.rs`。
- 沒有測試直接呼叫 `run_dom_task`、`target_task`、`task_numbered`。

### 直接測到這套順序的測試

- `dom`：`tasks::` 的九個（先進先出、timer 的時間、interval、一輪只跑之前排的、旗標、window 的 `load`、idle callback 的 timeout）；`message_ports::` 的六個（訊息跟著 port 到別的 thread、編號的時機）；`worker_global::` 的兩個。
- `dom_js`：`tasks::` 的兩個；`worker::` 的九個（經過 `run_due_tasks`）；`blobs::`、`frames::`、`termination::` 各一些；`dom_wasm` 的 `engine::` 一個。
- `shell`：`event_loop::` 的十一個、`messaging::` 的兩個、`workers::` 的九個（名稱在第 1 次盤點的紀錄）。

### 文件

- `docs/architecture.md` 的「Event loop：加新的工作時要跟著改的地方」：DOM 的 task 經過 `TaskWaker`、記號跑自己的 task、新的一種要過 `task_may_run`、`assert_no_dom_task_lost`；最後一條「worker 的 loop 不一樣」。
- `docs/test-gaps.md`：`dom_now().max(task.at)` 只靠審查；worker 的 `platform_due` 叫醒沒有測試；`assert_no_dom_task_lost` 和 `run_dom_task` 清掉 target；等價的 mutation（`first_element_task` 找整個 queue、清掉 target）；關掉的 worker 還收訊息。
- `docs/limitations.md` 裡 dedicated worker 的那一條：`close()` 之後同一輪裡已經到期的 task 照樣跑。

## 對照 Chromium

還沒對。要對的：`third_party/blink/renderer/platform/scheduler/worker/`（`WorkerThreadScheduler`、`WorkerSchedulerImpl`、`NonMainThreadSchedulerBase`）、`core/workers/worker_thread.cc`（`PrepareForShutdown`、`close()` 之後的 task）、`core/messaging/message_port.cc`（`Start`、`Connector` 什麼時候讀）、`core/frame/dom_timer.cc` 在 worker 裡的 task runner。
