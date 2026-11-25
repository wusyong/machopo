# RFC 0114：DOM 的 task 只有一套順序，worker 和主 thread 一樣

- 開始日期：2026-11-25
- 狀態：👍 已接受：2026-11-25
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

**worker 的 scheduler**：每條 worker 的 thread 一個，不和主 thread、別的 worker 共用（worker 照舊各自平行；同一個 worker 的 task 不能平行：JS 在一個 isolate 裡一次只有一條 thread，HTML 的 event loop 一次一個 task；worker 裡 V8 的 platform task 本來就在 V8 自己的 thread 上平行跑，只有回到 JS 的那一步排進 worker 的 event loop）。兩種 scheduler 共用一份核心（第 1、5 條）：從 `task_queue.rs` 抽出兩邊共用的部分，放進 `base`（照來源分的 queue、全域的 sequence、延後 task 的 heap、挑下一個時才把到期的搬進 queue、`post`、`post_at`、等待和叫醒），主 thread 的 `MainThreadScheduler` 和新的 worker 的 scheduler 各用一份，queue 的種類各自定（對照 Blink：`MainThreadSchedulerImpl` 和 `WorkerThreadScheduler` 都是 `SequenceManager` 上的 task queue）。worker 的 queue（暫定）：DOM、擁有它的 thread 送來的（`postMessage`、`Stop`、子 worker 的事件）、Browser 的（fetch、module 的回應）、V8 的 platform task。

**worker 的 loop**：`run_worker` 照 `RenderThread::run` 的樣子：取一個 task、跑、沒有 task 時等。worker 的 DOM 有 `TaskWaker`（送記號給 worker 的 scheduler）；`WorkerEngine` 多 `run_dom_task(number)`，拿掉 `run_due_tasks`、`until_next_task`。`importScripts()` 等 Browser 的回應時，照主 thread 同步等 Browser 的做法（`sync_call`：在 scheduler 裡等那個編號的回答，其他 task 留在 queue 裡），不再自己 `put_off`。

**`dom` 只照編號存**：拿掉 `next_task`、`NextTask`、`begin_task_pass`、`end_task_pass`、`task_limit`、`queue_heads` 的排序；每種 task 的 take 收編號，找到就拿；`task_may_run`、`task_target` 拿掉（第 2 條）。timer 的到期時間直接用 scheduler 的時鐘換好的 `Instant` 送，跑的時候的 `now` 用 scheduler 交出的。

**port 的訊息**：照第 3 條的狀態機（`Waiting`、`Running`，`enable`、`push`、`ship`、`arrive`）。

**debug build 的偵測**：`dom` 只回答有沒有能跑、早就到期的 task，不排順序；主 thread 和 worker 都裝（第 4 條）。

**`close()`**：那個 task 和它的 microtask checkpoint 跑完時，worker 的 loop 清掉自己的 scheduler、結束（第 6 條）。

### 測試

- WPT：`workers/`、`webmessaging/`、`html/webappapis/timers/` 和清單裡每個目錄的 worker variant 照舊通過；`close()` 看 `workers/WorkerGlobalScope-close.html`、`workers/Worker_terminate_event_queue.htm` 有沒有涵蓋「之後到期的 task 不跑」，沒有就寫 `shell` 的測試。
- `dom`、`dom_js`、`dom_wasm` 裡直接呼叫 `next_task` 和 `run_due_tasks` 的測試（`details.md` 列了）改成經過 `base` 的 scheduler 核心跑（第 5 條），守的事不變。
- `shell` 的 `event_loop::`、`messaging::`、`workers::` 照舊通過；必壞的：worker `close()` 之後到期的 task 不跑。
- mutation test：worker 的 task 沒有記號、記號的順序、port 的三種轉換（`enable`、`push`、`arrive`）、`close()` 之後丟掉、debug build 的偵測。

### 分階段

1. 抽出 scheduler 的核心放進 `base`，主 thread 改用它：行為不變。
2. worker 的 loop 改用 scheduler：DOM 的 task 用記號，其他的工作各是一種 task；`importScripts()` 的等待；`close()` 照 HTML。
3. 拿掉 `dom` 的順序：`next_task`、一輪、`task_limit`、`target_task`、時鐘的換算；port 改成狀態機；測試改成經過 `base` 的核心跑；debug build 的偵測。

## 缺點

- worker 每個 task 多一次 scheduler 的往返（現在一輪跑完到期的全部）：timer 和訊息很多的 worker 會慢一點；Speedometer 不用 worker，平常不量（`docs/benchmarks.md`）。
- `dom` 系列的測試改成經過 `base` 的 scheduler 核心跑，寫法變了。
- 改的是 `dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer` 的介面，範圍大。

## 理由和其他做法

- **不選只把 worker 改成記號、`dom` 的順序留著給測試**：兩套順序和補的地方都還在，只是少一個用的人。
- **不選 worker 用主 thread 的 `MainThreadScheduler` 本身**：它的 queue 是 renderer 的（Browser 的訊息、rendering、DevTools），worker 沒有那些；Blink 也是兩個 scheduler 共用底層。
- **不選照舊**：每加一種 DOM task 都要記得兩個地方，worker 和主 thread 的行為可能不一樣（`close()` 已經不一樣）。

## 先例

- **Chromium**（2026-11-25 對過原始碼，`details.md` 的「對照 Chromium」）：worker 的 `WorkerThreadScheduler` 和主 thread 的 scheduler 共用 `SequenceManager`，每個 worker global 的 task 照 `TaskType` 分到幾個 queue；`DOMTimer` 在 window 和 worker 是同一段程式，post 到 `context.GetTaskRunner(task_type)`；`close()` 只設旗標，那個 task 結束時 `DidProcessTask` 收掉 worker 的 scheduler，排好的 task 不跑；`MessagePort` 的訊息在 `start()` 之後、在 port 的 task runner 上從 pipe 讀出來，才變成 task。照做：每條 thread 一個 scheduler、共用底層；timer 的 task 兩邊同一段；`close()` 在 task 結束時丟掉 queue；port 的訊息啟用之後才排。
- **HTML**：worker 的 event loop 和 window 的一樣是 task queue；`close()` 丟掉 worker 的 event loop 裡已經排好的 task（"discard any tasks that have been added to the event loop's task queues"）。

## 已決定的事項

1. **每條 worker 的 thread 一個 scheduler，和主 thread 的共用抽出來的核心**（2026-11-25，使用者決定；A）：從 `task_queue.rs` 抽出照來源分的 queue、全域的 sequence、延後 task 的 heap、取下一個時才把到期的搬進 queue、`post`、`post_at`、等待和叫醒，queue 的種類是型別參數，放在 `base`（第 5 條）；主 thread 和每條 worker 的 thread 各用一份，不共用 queue。worker 之間、worker 和主 thread 之間照舊平行；同一個 worker 的 task 一次一個（HTML 的 event loop，JS 在一個 isolate 裡一次一條 thread）。對照 Chromium：`MainThreadSchedulerImpl` 和 `WorkerThreadScheduler` 是不同的類別，共用 `SequenceManager`。不選 worker 自己寫一份小的、只共用型別：同樣的順序規則寫兩次（RFC 0112 第 25 條的 bug 就在「延後的 task 什麼時候搬」這種細節），改一邊要記得改另一邊。
2. **拿掉 `target_task`，照編號直接拿**（2026-11-25，使用者決定；A）：每種 task 的 take（timer、UA 的 timer、idle callback 的 timeout、element task、訊息）收編號，找到就拿；`task_may_run`、`task_target` 和它的設、清一起拿掉。查過：正式的程式裡一個 task 執行中沒有別的路再挑 DOM 的 task（只有記號和 worker 的一輪會挑）。對照 Chromium：task 是它自己的 closure，不用找。不選留著：「記號先設 target、再找第一個」的繞路還在，新的一種 task 還是要記得過 `task_may_run`。
3. **port 的訊息照狀態機在能送的時候才拿編號、送記號**（2026-11-25，使用者決定；A，寫法照狀態機）：
   - port 在它的家（擁有它的 thread）上的佇列是兩種狀態：`Waiting`（沒啟用，或正在轉到別的 thread：訊息沒有編號）和 `Running`（啟用了、在家：每則是 `(Stamp, Message)`，記號已經送出）。`Stamp` 只有「編號並送記號」的函式造得出來，enum 是 port 私有的，所以「能送卻沒編號」和「有編號卻沒送記號」在型別上做不出來。
   - 會改變能不能送的只有 port 自己的方法：`enable`（`start()`、設 `onmessage`：`Waiting` 換成 `Running`，照順序編號、送記號）、`push`（訊息到了家：`Waiting` 照原樣放，`Running` 當場編號再放）、`ship` 和 `arrive`（轉到別的 thread：換回 `Waiting`，舊的家送過的記號跑到時照編號找不到、什麼都不做；在新的家已經啟用就 `enable`）。別的 thread 送來的先進 pipe 上跨 thread 的緩衝區、叫醒家裡的 scheduler，在那個 task 裡才 `push`（編號要用家裡的 `dom`）。
   - 補號（`first_stamp`）、重送（`keep_port`）和「對方在同一條 thread、前面的都編過號才編」的條件拿掉。
   - 型別擋不住的只剩「該轉換時沒呼叫」：例如新的一條啟用 port 的路忘了 `enable`，那個 port 整個停在 `Waiting`、一則都收不到；啟用只有 HTML 的 `start()` 和設 `onmessage` 兩條路，各有測試。三種轉換各有必壞的 mutation（現有的 `shell` 的 `a_ports_message_waits_for_it_to_be_enabled`、`a_moved_ports_messages_come_in_order`、`two_workers_talk_through_a_port_while_the_page_is_busy` 和 WPT 的 `webmessaging/`）。
   - 對照 Chromium：訊息留在 mojo 的 pipe 裡，`start()` 之後在 port 的 task runner 上讀出來才變成 task，每則一個 task（`Connector` 讀一則、替剩下的各 post 一個）；mojo 的 port 也是狀態機（`ports::Node` 的 receiving、buffering、proxying、closed）。HTML 的 port message queue 啟用之前，訊息不在 event loop 裡。
   - 不選照舊送出時編、只拿掉重送：還會有「編了號、port 還沒啟用」的記號輪到時沒事可做，啟用時仍要處理，而補號要用到這份要拿掉的 `dom` 的順序。不選每個 port 一個記號（分派一則、還有剩再 post 一個）：同時到的訊息會被中間排進來的 timer 隔開，和 Chromium、HTML 的順序不同（`shell` 的 `messages_and_timers_run_in_the_order_queued` 會壞）。不選這份就把 port 搬到 `ipc` 上（照 Chromium 用 message pipe）：`ipc` 的通道還不能放進訊息裡轉交、也沒有在擁有它的 thread 上叫醒的 watcher，那是 port 跨 renderer 的那一份的事（「之後的可能」）；同一條 thread 上的 port 也會多繞一趟 IO thread。
4. **debug build 的偵測：只問「有沒有」**（2026-11-25，使用者決定；B）：thread 要等待、scheduler 是空的時候，`dom` 回答有沒有任何能跑、早就到期（5 ms）的 task（timer、UA 的 timer、idle callback 的 timeout、element task、`Running` 的 port 裡的訊息），不排順序；有就 panic。主 thread 和 worker 都裝。新的一種 task 要加進這個查詢（漏了只是少抓）。port 的那一類已經由第 3 條的狀態機在型別上擋住，這個主要守其他的 task 漏記號。對照 Chromium：沒有這種偵測（task 就是 closure，post 了就在 queue 裡）；machopo 的 task 的內容在 `dom`、記號在 scheduler，兩邊要對得上。不選拿掉：timer、element task 漏記號時又會靜靜地卡住（RFC 0112 第 25 條那次就是記號的 bug）。不選逐一對帳（兩邊各記送出、還沒跑的記號，每個 task 之後比對）：漏的當下就抓到，但多兩份 debug 用的帳、每個 task 之後比對，debug build 變慢，帳本身也可能寫錯。
5. **scheduler 的核心放在 `base`，`dom` 系列的測試用真的**（2026-11-25，使用者決定；A）：第 1 條抽出的核心放進 `base`（不加依賴；對照 Chromium 的 `base::sequence_manager`），renderer 的主 thread、worker 和 `dom`、`dom_js`、`dom_wasm` 的測試都用它；測試多一個小的 helper（`TaskWaker` 把記號送進它、跑到沒有為止），取代直接呼叫的 `next_task`、`run_due_tasks` 和 `ScriptEngine` 只剩測試在用的 `run_element_tasks`、`run_posted_messages`、`run_timers`。測試和正式的程式是同一份順序規則。不選核心放在 `renderer`、測試另寫一個照記號排的小 queue：`dom` 的測試不能依賴 `renderer`，兩份規則可能走樣（例如延後的 task 什麼時候搬）。
6. **worker 的 `close()` 照 Chromium：那個 task 結束時丟掉排好的 task**（2026-11-25，使用者決定；A）：`close()` 只設 closing 的旗標；那個 task 和它的 microtask checkpoint 跑完，worker 的 loop 看到旗標就清掉自己的 scheduler（排好的 task 都不跑），照現在的程序結束。看得到的結果和 HTML 的 close a worker 一樣：剩下的 task 都不跑（解決 `docs/limitations.md` 裡 dedicated worker 的那一條）。對照 Chromium：`WorkerGlobalScope::close` 設 `closing_`，`WorkerThread::DidProcessTask` 跑完 checkpoint 之後 `PrepareForShutdownOnWorkerThread`，`worker_scheduler_->Dispose()`。不選在 `close()` 裡當場丟掉（照 HTML 的字面）：結果一樣，但要在 script 的呼叫裡碰 scheduler，那個 task 剩下的部分還要在 queue 已經清掉的狀態下跑完。

## 待決定

沒有。

## 之後的可能

- port 搬到 `ipc` 上、能跨 renderer（`docs/limitations.md` 的 `MessageChannel` 那一條）：`ipc` 要先有能放進訊息裡轉交的 message pipe、在擁有它的 thread 上叫醒（Mojo 的 ports 和 watcher）；第 3 條的 `push`、`enable`、`arrive` 就是到時候 handler 接上的入口。
- shared worker：同一套 scheduler，好幾個文件連到它。
- worker 的 DevTools session：worker 的 scheduler 多一個 DevTools 的 queue，像主 thread 的。
