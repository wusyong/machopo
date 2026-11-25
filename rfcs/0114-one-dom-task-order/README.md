# RFC 0114：DOM 的 task 只有一套順序，worker 和主 thread 一樣

- 開始日期：2026-11-25
- 狀態：📝 草稿
- 細節：`details.md`

## 摘要

主 thread 的 DOM task 已經只照 scheduler 的順序跑：`dom` 排一個 task 時送一個記號（編號和到期的時間）給 scheduler，記號輪到時跑它自己的那一個 task（RFC 0112 的第 25 條）。但 `dom` 自己還留著一套 `(時間, 編號)` 的順序（`next_task`、一輪一輪的 `begin_task_pass`），給 dedicated worker 的 loop、測試和 debug build 的偵測用；兩套之間另外補了幾處。這份讓每條 worker 的 thread 也有一個自己的 scheduler，然後拿掉 `dom` 的那一套：`dom` 只照編號存 task 的內容，順序只在各自的 scheduler。worker 之間、worker 和主 thread 之間照舊平行（每個 worker 一條 thread），同一個 worker 裡的 task 照 HTML 一次一個（現在也是）。對照 Chromium：每條 worker 的 thread 有自己的 `WorkerThreadScheduler`，和主 thread 的 `MainThreadSchedulerImpl` 是不同的類別，共用底層的 `SequenceManager`；DOM 的 timer、訊息都是 post 到 task runner 的 task，沒有另一份順序。

## 動機

現況（`details.md` 的「現況」，2026-11-25 的 `main`）：

- **兩種 loop**：主 thread 是 scheduler，一次一個 task；worker 沒有 scheduler，一輪一輪呼叫 `WorkerEngine::run_due_tasks`，裡面照 `dom` 的 `next_task` 挑到期的 task，自己的 inbox（`WorkerTask`）收訊息、V8 的 platform task 和 Browser 的回應；worker 的 DOM 沒有 `TaskWaker`。
- **兩套順序之間補的地方**（RFC 0112 的「之後的可能」列的）：
  - port 的訊息在送出的當下只在對方的 thread 上、前面的都編過號時才編號，其餘的等對方挑 task 時照佇列的順序補；port 啟用時重送已經編號的記號（`keep_port`，WPT 的 `webmessaging/` 才抓到）。
  - 兩個時鐘（DOM 的 ms、scheduler 的 `Instant`）差一點時照 scheduler 交出的時間跑（`dom_now().max(task.at)`，只靠審查）。
  - 主 thread 的 engine 還有沒人呼叫的 `run_due_tasks`（`dom_js` 和 `dom_wasm` 各一個，只剩測試用），debug build 的偵測（`assert_no_dom_task_lost`）靠 `dom` 的 `next_task` 問「有沒有到期卻沒記號的」。
- **一輪的模型看得到的差別**：worker `close()` 之後，同一輪裡已經到期的 task 照樣跑（HTML 要丟掉，`docs/limitations.md` 裡 dedicated worker 的那一條）。
- **加新的東西要改兩個地方**：DOM 新的一種 task 要在挑 task 的地方照 `task_may_run`（記號才找得到它），也要進 `next_task`（worker 和測試才跑得到它）；`docs/architecture.md` 的「Event loop：加新的工作時要跟著改的地方」把 worker 列成例外。

**目標**：

1. worker 的 loop 和主 thread 是同一種：每條 worker 的 thread 一個自己的 scheduler，一次一個 task；DOM 的 task、worker 自己的工作、V8 的 platform task、Browser 和擁有它的 thread 送來的各是一個 task。
2. DOM 的 task 只有 scheduler 的順序：`dom` 照編號存 task 的內容，沒有 `next_task`、一輪的機制、`task_limit`、時鐘的換算；engine 只有「跑這個編號」（`run_dom_task`）。
3. 補的地方拿掉：port 的訊息在能送的時候（port 啟用、到了它的 thread）才拿編號，不重送；不用 `max(task.at)`。
4. worker 的行為照 HTML：`close()` 之後不再跑它的 task；其餘看得到的順序和現在一樣（WPT 的 `workers/`、`webmessaging/` 照舊通過）。
5. 加新的一種 DOM task 只要在一個地方做（文件跟著改）。

**不在範圍內**：

- shared worker、service worker、worklet（RFC 0103 的「之後的可能」）。
- 主 thread 的 scheduler 怎麼排優先順序（RFC 0112 的「之後的可能」）。
- worker 的 DevTools session。

要解決的 `docs/limitations.md` 的項目：dedicated worker 的那一條的「`close()` 之後，同一輪裡已經到期的 task 照樣跑」。`docs/test-gaps.md` 裡會跟著不成立的：`dom_now().max(task.at)` 只靠審查、worker 的 `platform_due` 叫醒沒有測試、`target_task` 的清除和 `first_element_task` 的等價 mutation（`details.md` 列了行號）。

## 設計概要

給寫網頁的人：沒有看得到的改變，除了 worker `close()` 之後不再跑已經到期的 task。

給改 machopo 的人：DOM 新的一種 task 只要 `stamp_queued_task` 拿編號、`wake_for_task` 送記號、在 `task_numbered` 找得到，主 thread 和 worker 都一樣；worker 自己的新工作照主 thread 的做法，是 scheduler 的一種 task。

## 詳細設計

**worker 的 scheduler**：每條 worker 的 thread 一個，不和主 thread、別的 worker 共用（worker 照舊各自平行；同一個 worker 的 task 不能平行：JS 在一個 isolate 裡一次只有一條 thread，HTML 的 event loop 一次一個 task；worker 裡 V8 的 platform task 本來就在 V8 自己的 thread 上平行跑，只有回到 JS 的那一步排進 worker 的 event loop）。要決定的是兩種 scheduler 共不共用同一份程式（待決定第 1 條）；建議：從 `task_queue.rs` 抽出兩邊共用的部分（照來源分的 queue、全域的 sequence、延後 task 的 heap、挑下一個時才把到期的搬進 queue、`post`、`post_at`、等待和叫醒），主 thread 的 `MainThreadScheduler` 和新的 worker 的 scheduler 各用一份，queue 的種類各自定（對照 Blink：`MainThreadSchedulerImpl` 和 `WorkerThreadScheduler` 都是 `SequenceManager` 上的 task queue）。worker 的 queue（暫定）：DOM、擁有它的 thread 送來的（`postMessage`、`Stop`、子 worker 的事件）、Browser 的（fetch、module 的回應）、V8 的 platform task。

**worker 的 loop**：`run_worker` 照 `RenderThread::run` 的樣子：取一個 task、跑、沒有 task 時等。worker 的 DOM 有 `TaskWaker`（送記號給 worker 的 scheduler）；`WorkerEngine` 多 `run_dom_task(number)`，拿掉 `run_due_tasks`、`until_next_task`。`importScripts()` 等 Browser 的回應時，照主 thread 同步等 Browser 的做法（`sync_call`：在 scheduler 裡等那個編號的回答，其他 task 留在 queue 裡），不再自己 `put_off`。

**`dom` 只照編號存**：拿掉 `next_task`、`NextTask`、`begin_task_pass`、`end_task_pass`、`task_limit`、`queue_heads` 的排序；`task_numbered` 找到編號就跑（主 thread 已經是這樣：`target_task`）。`target_task` 是不是還需要，看 `task_numbered` 之後跑的路是不是都照編號（`details.md`）。timer 的到期時間直接用 scheduler 的時鐘換好的 `Instant` 送，跑的時候的 `now` 用 scheduler 交出的。

**port 的訊息**：送到 port 的家（它的 thread）而且 port 啟用之後才拿編號、送記號；之前在佇列裡等，啟用時一次編號、照順序送。

**debug build 的偵測**：`dom` 沒有順序之後，「到期卻沒記號」問不出來；記號在排 task 的當下就送，丟不掉，偵測可以拿掉，或改成「`dom` 裡有 task、scheduler 裡卻沒有它的記號」（待決定）。

### 測試

- WPT：`workers/`、`webmessaging/`、`html/webappapis/timers/` 和清單裡每個目錄的 worker variant 照舊通過；`close()` 的 WPT 看有沒有（`workers/Worker_terminate_event_queue.htm` 這一類），沒有就寫 `shell` 的測試。
- `dom`、`dom_js`、`dom_wasm` 裡直接呼叫 `next_task` 和 `run_due_tasks` 的測試（`details.md` 列了）改成經過一個測試用的 scheduler（照記號的順序跑），守的事不變。
- `shell` 的 `event_loop::`、`messaging::`、`workers::` 照舊通過；必壞的：worker `close()` 之後到期的 task 不跑。
- mutation test：worker 的 task 沒有記號、記號的順序、port 啟用時的編號、`close()` 之後丟掉。

### 分階段

1. 抽出 scheduler 共用的程式，主 thread 改用它：行為不變。
2. worker 的 loop 改用 scheduler：DOM 的 task 用記號，其他的工作各是一種 task；`importScripts()` 的等待；`close()` 照 HTML。
3. 拿掉 `dom` 的順序：`next_task`、一輪、`task_limit`、時鐘的換算、port 的補號和重送；測試改成經過測試用的 scheduler；debug build 的偵測。

## 缺點

- worker 每個 task 多一次 scheduler 的往返（現在一輪跑完到期的全部）：timer 和訊息很多的 worker 會慢一點；Speedometer 不用 worker，平常不量（`docs/benchmarks.md`）。
- `dom` 的單元測試要一個測試用的 scheduler，測試的寫法變了。
- 改的是 `dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer` 的介面，範圍大。

## 理由和其他做法

- **不選只把 worker 改成記號、`dom` 的順序留著給測試**：兩套順序和補的地方都還在，只是少一個用的人。
- **不選 worker 用主 thread 的 `MainThreadScheduler` 本身**：它的 queue 是 renderer 的（Browser 的訊息、rendering、DevTools），worker 沒有那些；Blink 也是兩個 scheduler 共用底層。
- **不選照舊**：每加一種 DOM task 都要記得兩個地方，worker 和主 thread 的行為可能不一樣（`close()` 已經不一樣）。

## 先例

- **Chromium**（要對原始碼）：`WorkerThread` 的 `WorkerThreadScheduler`（`NonMainThreadSchedulerBase`）和每個 worker global 的 `WorkerSchedulerImpl`，task queue 照 `TaskType`；`DOMTimer` 在 worker 裡也 post 延後的 task；`MessagePort::Start` 之後 mojo 的 `Connector` 在 task 裡讀訊息、分派；`DedicatedWorkerGlobalScope::close` 讓之後的 task 不跑（`WorkerThread::PrepareForShutdown` 這一類，要對）。
- **HTML**：worker 的 event loop 和 window 的一樣是 task queue；`close()` 丟掉 worker 的 event loop 裡已經排好的 task（"discard any tasks that have been added to the event loop's task queues"）。

## 已決定的事項

無。

## 待決定

1. **兩種 scheduler 共不共用程式**（每條 thread 各一個 scheduler 是定的）：抽出一個泛型的核心（queue 的種類是型別參數），主 thread 和 worker 各用一份；或 worker 自己寫一份小的，只共用型別。Chromium 是前者（`SequenceManager`）。
2. **`target_task` 留不留**：`dom` 照編號存之後，記號跑的就是那一個 task；但 task 在執行中又挑 task 的路（例如同步的 `dispatchEvent` 不挑，`importScripts()` 的等待會不會）要對過。
3. **port 的訊息什麼時候拿編號**：能送的時候（port 啟用、到了它的 thread）；或照舊送出時編、只拿掉重送。對照 Chromium 的 `Connector`。
4. **debug build 的偵測**：拿掉；或改成比對 `dom` 的 task 和 scheduler 的記號。
5. **`dom` 的測試的 scheduler**：放在 `dom` 的測試裡（一個照記號排的小 queue）；或 `renderer` 的 scheduler 讓測試用（`dom` 的測試不依賴 `renderer`，大概不行）。
6. **`close()` 之後**：照 HTML 丟掉已經排好的 task（scheduler 清掉 DOM 的 queue）；要對 Chrome 實際怎麼做。

## 之後的可能

- shared worker：同一套 scheduler，好幾個文件連到它。
- worker 的 DevTools session：worker 的 scheduler 多一個 DevTools 的 queue，像主 thread 的。
