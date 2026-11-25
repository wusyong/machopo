# RFC 0114：DOM 的 task 只有一套順序，worker 和主 thread 一樣

- 開始日期：2026-11-25
- 狀態：✅ 已完成：2026-11-25（Linux）
- 細節：`details.md`（開始之前的現況、對照 Chromium）；進度和驗收：`progress.md`

## 摘要

開始之前，主 thread 的 DOM task 已經只照 scheduler 的順序跑：`dom` 排一個 task 時送一個記號（編號和到期的時間）給 scheduler，記號輪到時跑它自己的那一個 task（RFC 0112 的第 25 條）。但 `dom` 自己還留著一套 `(時間, 編號)` 的順序（`next_task`、一輪一輪的 `begin_task_pass`），給 dedicated worker 的 loop、測試和 debug build 的偵測用；兩套之間另外補了幾處。這份讓每條 worker 的 thread 也有一個自己的 scheduler，scheduler 的核心抽到 `base`、主 thread 和 worker 共用，然後拿掉 `dom` 的那一套：`dom` 只照編號存 task 的內容，順序只在各自的 scheduler，測試也經過同一個核心跑。port 的訊息照一個小的狀態機在能送的時候才編號；worker 的 `close()` 照 Chromium 在那個 task 結束時丟掉排好的 task。worker 之間、worker 和主 thread 之間照舊平行（每個 worker 一條 thread），同一個 worker 裡的 task 照 HTML 一次一個。對照 Chromium：每條 worker 的 thread 有自己的 `WorkerThreadScheduler`，和主 thread 的 `MainThreadSchedulerImpl` 是不同的類別，共用底層的 `SequenceManager`；DOM 的 timer、訊息都是 post 到 task runner 的 task，沒有另一份順序。

## 動機

開始之前（`details.md` 的「現況」，2026-11-25 的 `main`）：

- **兩種 loop**：主 thread 是 scheduler，一次一個 task；worker 沒有 scheduler，一輪一輪呼叫 `WorkerEngine::run_due_tasks`，裡面照 `dom` 的 `next_task` 挑到期的 task，自己的 inbox（`WorkerTask`）收訊息、V8 的 platform task 和 Browser 的回應；worker 的 DOM 沒有 `TaskWaker`。
- **兩套順序之間補的地方**（RFC 0112 的「之後的可能」列的）：
  - port 的訊息在送出的當下只在對方的 thread 上、前面的都編過號時才編號，其餘的等對方挑 task 時照佇列的順序補；port 啟用時重送已經編號的記號（`keep_port`，WPT 的 `webmessaging/` 才抓到）。
  - 兩個時鐘（DOM 的 ms、scheduler 的 `Instant`）差一點時照 scheduler 交出的時間跑（`dom_now().max(task.at)`，只靠審查）。
  - 記號跑它自己的 task 靠 `target_task` 讓那個 task 暫時是它 queue 的第一個。
  - 主 thread 的 engine 還有沒人呼叫的 `run_due_tasks`（`dom_js` 和 `dom_wasm` 各一個，只剩測試用），debug build 的偵測（`assert_no_dom_task_lost`）靠 `dom` 的 `next_task` 問「有沒有到期卻沒記號的」。
- **一輪的模型看得到的差別**：worker `close()` 之後，同一輪裡已經到期的 task 照樣跑（HTML 要丟掉）。
- **加新的東西要改兩個地方**：DOM 新的一種 task 要在挑 task 的地方照 `task_may_run`（記號才找得到它），也要進 `next_task`（worker 和測試才跑得到它）。

**目標**（都達成，驗收見 `progress.md`）：

1. worker 的 loop 和主 thread 是同一種：每條 worker 的 thread 一個自己的 scheduler，一次一個 task；DOM 的 task、worker 自己的工作、V8 的 platform task、Browser 和擁有它的 thread 送來的各是一個 task。
2. DOM 的 task 只有 scheduler 的順序：`dom` 照編號存 task 的內容，沒有 `next_task`、一輪的機制、`task_limit`、時鐘的換算；engine 只有「跑這個編號」（`run_dom_task`）。
3. 補的地方拿掉：port 的訊息在能送的時候（port 啟用、到了它的 thread）才拿編號，不重送；不用 `max(task.at)`。
4. worker 的行為照 HTML：`close()` 之後不再跑它的 task；其餘看得到的順序和之前一樣（WPT 的 `workers/`、`webmessaging/` 照舊通過）。
5. 加新的一種 DOM task 只要在一個地方做（文件跟著改）。

**不在範圍內**：

- shared worker、service worker、worklet（RFC 0103 的「之後的可能」）。
- 主 thread 的 scheduler 怎麼排優先順序（RFC 0112 的「之後的可能」）。
- worker 的 DevTools session。

解決的 `docs/limitations.md` 的項目：dedicated worker 的那一條的「`close()` 之後，同一輪裡已經到期的 task 照樣跑」。跟著不成立、拿掉的 `docs/test-gaps.md` 的項目：`dom_now().max(task.at)` 只靠審查、`target_task` 的清除和 `first_element_task` 的等價 mutation；worker 的 `platform_due` 叫醒那一條改寫成現在的做法（`platform_posted`）。

## 設計概要

給寫網頁的人：沒有看得到的改變，除了 worker `close()` 之後不再跑已經排好的 task。

給改 machopo 的人：DOM 新的一種 task 拿編號、送記號（`stamp_queued_task` 或 `next_task_number` 加 `wake_for_task`），在 `task_numbered` 加一個分支、有照編號拿的 take，在 `has_task_due_by` 加一項；主 thread 和 worker 都一樣（`docs/architecture.md` 的「Event loop：加新的工作時要跟著改的地方」）。worker 自己的新工作照主 thread 的做法，是 scheduler 的一種 task。測試裡要跑 DOM 的 task 時經過 `dom::TaskTokens`。

## 詳細設計

**scheduler 的核心在 `base`**（第 1、5、7 條）：`base::sequence_manager::SequenceManager<S: Spec>` 是照來源分的 queue、全域的 sequence、延後 task 的 heap（取下一個 task 時才把到期的搬進 queue）、它自己的 `Mutex` 和 `Condvar`：`post`、`post_at`、`post_with`、`next`（可以有時限）、`try_next`、`try_next_by`（照給的時刻，測試的時鐘，第 12 條）、`wait_for`、`has_task`、`take_matching`、`put_back`、`sync_wait`、`post_reply`、`close`。使用的人照 `Spec` 給 queue 的種類、task、同步等的回答、同一把鎖底下自己的狀態和取走 task 時的 hook。主 thread 的 `MainThreadScheduler` 建在它上面（`MainSpec`：DevTools 的打斷、`posted_once`、`shutting_down`），行為不變。

**worker 的 scheduler 和 loop**（第 1、6、8 條）：每條 worker 的 thread 一個 `WorkerScheduler`，queue 是 `Control`（結束）、`Owner`（擁有它的 thread 送來的：`postMessage`、子 worker 的事件）、`Browser`（fetch、module、`importScripts()` 的回應）、`Dom`（記號）、`Internal`（port 的叫醒、V8 的 platform task）。`run_worker` 照 `RenderThread::run`：一次取一個 task、跑、沒有就等；script 跑完之前不取 `Owner` 的 queue。worker 的 DOM 有 `TaskWaker`，`WorkerEngine` 有 `run_dom_task`、`port_messages_arrived`、`has_dom_task_due_for`。`importScripts()` 等 Browser 時只取 `Control` 和 `Browser`，別的載入的回應等完放回最前面（`put_back`），其他 task 留在 queue 裡。V8 的 platform task 照主 thread：同時只排一個，延後的用 `post_at`。`close()` 只設旗標；那個 task 和它的 microtask checkpoint 跑完，loop 開頭看到旗標就 `close` 自己的 scheduler（排好的都不跑）、結束。

**`dom` 只照編號存**（第 2 條）：每個 task（timer、UA 的 timer、idle callback 的 timeout、element task、訊息）建立時從一個計數器拿編號、當場送記號（timer 的帶著到期的 `Instant`）；interval 每次被拿走時重新編號、送新的記號。`task_numbered` 回答一個編號的 task 在哪一種 queue（不管排第幾），`take_*_numbered` 照編號拿，不比時間（scheduler 說它到期了）。沒有 `next_task`、一輪、`target_task`、`has_queued_tasks`，也沒有「拿第一個」的 take；engine 的 `run_dom_task(number)` 是唯一跑 DOM task 的路（`dom_api::run_task` 收編號）。取消的 task 只拿掉 DOM 裡的紀錄，記號輪到時找不到、什麼都不做。

**port 的訊息**（第 3、11 條）：pipe 的每一端是一個小的狀態機：沒編號的訊息（`incoming`）、編了號的（`ready`，每則一個 `Stamp`：只有 `Cx::stamp_task` 造得出來，造的時候就送了記號，欄位私有）、在不在跑（`running`：port 啟用了、在它的 thread 上）。改變它的只有 `PortEnd` 的方法：`enable`（`start()`、設 `onmessage`：開始跑，等著的照順序編號）；`post`（決定裡的 `push`：對方在這條 thread 上跑時當場編號，先把前面別的 thread 送來、還沒編號的編完；否則進 `incoming`，對方在別的 thread 就叫醒它）；`arrived`（叫醒之後，那條 thread 的 `PortMessages` task 裡的 `Cx::port_messages_arrived`：跑著的話把 `incoming` 編號）；`move_to`（決定裡的 `ship` 和到了新家：不再跑，編過號的放回 `incoming` 的最前面，舊的記號照編號找不到）。一端關掉時另一端的 thread 一定被叫醒（同一條 thread 也是），再也收不到的 port 在那個 task 裡、或最後一則被拿走時離開 context 的清單。

**debug build 的偵測**（第 4 條）：thread 要等待、scheduler 是空的時候，`Cx::has_task_due_by` 回答有沒有任何一種 task 早就（5 ms）到期，不排順序；有就 panic。主 thread（`assert_no_dom_task_lost`）和 worker 的 loop 都裝。

**測試經過同一個核心跑**（第 5、9、10、12 條）：`dom::TaskTokens` 是沒有 thread 的 host 的最小 scheduler：記號進 `base` 的 `SequenceManager`，`take_due` 一次拿出當下到期的全部（Blink 的 `test::RunPendingTasks`：跑的期間新排的留到下一次），`take_due_by` 照測試自己的時鐘（`Cx::instant_in` 把 window 的時間換成 `Instant`）。`dom` 的測試用它配照編號的 take（`tests/it/run.rs`）；`dom_api` 的測試的 DOM 帶著一份；`dom_js` 的測試把 engine 包成 `TestEngine`，`Env`（沒有 engine 的測試環境）和 worker 的測試用匯出的 `dom_js::run_dom_task`、`instant_in`。

### 測試

驗收和結果見 `progress.md`：`base` 的 `sequence_manager::tests`；`shell` 的 `event_loop::`、`messaging::`、`workers::`（必壞的：worker `close()` 之後排好的 task 不跑）；`dom` 的 `tasks::`、`message_ports::`、`window::`；WPT 的 `workers/`、`webmessaging/`、`html/webappapis/timers/`；每一段的 mutation test。

### 分階段

1. 抽出 scheduler 的核心放進 `base`，主 thread 改用它：行為不變。
2. worker 的 loop 改用 scheduler：DOM 的 task 用記號，其他的工作各是一種 task；`importScripts()` 的等待；`close()` 照 HTML。
3. 拿掉 `dom` 的順序，分成四段（第 9 條）：3a 照編號拿、拿掉 `target_task` 和時鐘的換算；3b port 的狀態機；3c debug build 的偵測；3d 拿掉 `next_task`、一輪的機制和 engine 只剩測試在用的方法，測試改成經過 `base` 的核心跑。

## 缺點

- worker 每個 task 多一次 scheduler 的往返（之前一輪跑完到期的全部）：timer 和訊息很多的 worker 會慢一點。Speedometer 不用 worker；2026-11-25 量的 20 個 suite 和開始之前的 `main` 比，幾何平均 +0.2%，沒有看得出的變化（RFC 0012 的 `progress.md`）。
- `dom` 系列的測試經過 `TaskTokens` 跑，寫法變了：拿 engine 自己的 frame 當參數的呼叫在 `TestEngine` 上轉呼叫（經過 `DerefMut` 時呼叫先借走了 engine）。
- 改的是 `base`、`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer` 的介面，範圍大。

## 理由和其他做法

- **不選只把 worker 改成記號、`dom` 的順序留著給測試**：兩套順序和補的地方都還在，只是少一個用的人。
- **不選 worker 用主 thread 的 `MainThreadScheduler` 本身**：它的 queue 是 renderer 的（Browser 的訊息、rendering、DevTools），worker 沒有那些；Blink 也是兩個 scheduler 共用底層。
- **不選照舊**：每加一種 DOM task 都要記得兩個地方，worker 和主 thread 的行為可能不一樣（`close()` 已經不一樣）。

踩過的坑（細節在 `progress.md`）：

- 第 2 段照 scheduler 的順序，worker 的 `Closed` 比它送的訊息先處理，WPT 的三個測試逾時：worker 的 engine 處理子 worker 自己關閉時用的是 terminate（丟掉還沒分派的訊息），之前被一輪的順序蓋住。改成和主 thread 一樣的 close。
- 第 3 段的 3b：照第 3 條寫成 enum 時，跑著的 port 放不下「別的 thread 送來、還沒輪到編號」的訊息，同一條 thread 後來送的又要排在它們後面（第 11 條）。
- 第 3 段的 3d：`dom_api`、`dom_js` 的 timer 測試用假的時間（`1e9` 毫秒之類），真的時鐘的 scheduler 跑不到；`Env` 用假的時間跑時 task 本身照真的時鐘算 interval 的下一次，interval 一直到期（第 12 條）。

## 先例

- **Chromium**（2026-11-25 對過原始碼，`details.md` 的「對照 Chromium」）：worker 的 `WorkerThreadScheduler` 和主 thread 的 scheduler 共用 `SequenceManager`，每個 worker global 的 task 照 `TaskType` 分到幾個 queue；`DOMTimer` 在 window 和 worker 是同一段程式，post 到 `context.GetTaskRunner(task_type)`；`close()` 只設旗標，那個 task 結束時 `DidProcessTask` 收掉 worker 的 scheduler，排好的 task 不跑；`MessagePort` 的訊息在 `start()` 之後、在 port 的 task runner 上從 pipe 讀出來，才變成 task；測試用 `base::test::TaskEnvironment`（可以 mock time）。照做：每條 thread 一個 scheduler、共用底層；timer 的 task 兩邊同一段；`close()` 在 task 結束時丟掉 queue；port 的訊息啟用之後才排；測試經過同一個核心，時間可以給。
- **HTML**：worker 的 event loop 和 window 的一樣是 task queue；`close()` 丟掉 worker 的 event loop 裡已經排好的 task（"discard any tasks that have been added to the event loop's task queues"）。

## 已決定的事項

1. **每條 worker 的 thread 一個 scheduler，和主 thread 的共用抽出來的核心**（2026-11-25，使用者決定；A）：從 `task_queue.rs` 抽出照來源分的 queue、全域的 sequence、延後 task 的 heap、取下一個時才把到期的搬進 queue、`post`、`post_at`、等待和叫醒，queue 的種類是型別參數，放在 `base`（第 5 條）；主 thread 和每條 worker 的 thread 各用一份，不共用 queue。worker 之間、worker 和主 thread 之間照舊平行；同一個 worker 的 task 一次一個（HTML 的 event loop，JS 在一個 isolate 裡一次一條 thread）。對照 Chromium：`MainThreadSchedulerImpl` 和 `WorkerThreadScheduler` 是不同的類別，共用 `SequenceManager`。不選 worker 自己寫一份小的、只共用型別：同樣的順序規則寫兩次（RFC 0112 第 25 條的 bug 就在「延後的 task 什麼時候搬」這種細節），改一邊要記得改另一邊。
2. **拿掉 `target_task`，照編號直接拿**（2026-11-25，使用者決定；A）：每種 task 的 take（timer、UA 的 timer、idle callback 的 timeout、element task、訊息）收編號，找到就拿；`task_may_run`、`task_target` 和它的設、清一起拿掉。查過：正式的程式裡一個 task 執行中沒有別的路再挑 DOM 的 task（只有記號和 worker 的一輪會挑）。對照 Chromium：task 是它自己的 closure，不用找。不選留著：「記號先設 target、再找第一個」的繞路還在，新的一種 task 還是要記得過 `task_may_run`。
3. **port 的訊息照狀態機在能送的時候才拿編號、送記號**（2026-11-25，使用者決定；A，寫法照狀態機；狀態的寫法照第 11 條）：
   - port 在它的家（擁有它的 thread）上的佇列是兩種狀態：沒在跑（沒啟用，或正在轉到別的 thread：訊息沒有編號）和在跑（啟用了、在家：編了號的每則帶著 `Stamp`，記號已經送出）。`Stamp` 只有「編號並送記號」的函式造得出來，所以「有編號卻沒送記號」在型別上做不出來。
   - 會改變能不能送的只有 port 自己的方法：`enable`（`start()`、設 `onmessage`：開始跑，照順序編號、送記號）、`push`（訊息到了家：沒在跑照原樣放，在跑當場編號再放；寫成 `PortEnd::post`）、`ship` 和 `arrive`（轉到別的 thread：不再跑，舊的家送過的記號跑到時照編號找不到、什麼都不做；寫成 `move_to`）。別的 thread 送來的先放著、叫醒家裡的 scheduler，在那個 task 裡才編號（編號要用家裡的 `dom`；寫成 `arrived`）。
   - 補號、重送（`keep_port`）和「對方在同一條 thread、前面的都編過號才編」的條件拿掉。
   - 型別擋不住的只剩「該轉換時沒呼叫」：例如新的一條啟用 port 的路忘了 `enable`，那個 port 一則都收不到；啟用只有 HTML 的 `start()` 和設 `onmessage` 兩條路，各有測試。每種轉換各有必壞的 mutation（`progress.md`）。
   - 對照 Chromium：訊息留在 mojo 的 pipe 裡，`start()` 之後在 port 的 task runner 上讀出來才變成 task，每則一個 task（`Connector` 讀一則、替剩下的各 post 一個）；mojo 的 port 也是狀態機（`ports::Node` 的 receiving、buffering、proxying、closed）。HTML 的 port message queue 啟用之前，訊息不在 event loop 裡。
   - 不選照舊送出時編、只拿掉重送：還會有「編了號、port 還沒啟用」的記號輪到時沒事可做，啟用時仍要處理，而補號要用到這份要拿掉的 `dom` 的順序。不選每個 port 一個記號（分派一則、還有剩再 post 一個）：同時到的訊息會被中間排進來的 timer 隔開，和 Chromium、HTML 的順序不同（`shell` 的 `messages_and_timers_run_in_the_order_queued` 會壞）。不選這份就把 port 搬到 `ipc` 上（照 Chromium 用 message pipe）：`ipc` 的通道還不能放進訊息裡轉交、也沒有在擁有它的 thread 上叫醒的 watcher，那是 port 跨 renderer 的那一份的事（「之後的可能」）；同一條 thread 上的 port 也會多繞一趟 IO thread。
4. **debug build 的偵測：只問「有沒有」**（2026-11-25，使用者決定；B）：thread 要等待、scheduler 是空的時候，`dom` 回答有沒有任何能跑、早就到期（5 ms）的 task（timer、UA 的 timer、idle callback 的 timeout、element task、window 的訊息、啟用了的 port 裡編過號的訊息），不排順序；有就 panic。主 thread 和 worker 都裝。新的一種 task 要加進這個查詢（漏了只是少抓）。port 的那一類已經由第 3 條的狀態機擋住，這個主要守其他的 task 漏記號。對照 Chromium：沒有這種偵測（task 就是 closure，post 了就在 queue 裡）；machopo 的 task 的內容在 `dom`、記號在 scheduler，兩邊要對得上。不選拿掉：timer、element task 漏記號時又會靜靜地卡住（RFC 0112 第 25 條那次就是記號的 bug）。不選逐一對帳（兩邊各記送出、還沒跑的記號，每個 task 之後比對）：漏的當下就抓到，但多兩份 debug 用的帳、每個 task 之後比對，debug build 變慢，帳本身也可能寫錯。
5. **scheduler 的核心放在 `base`，`dom` 系列的測試用真的**（2026-11-25，使用者決定；A）：第 1 條抽出的核心放進 `base`（不加依賴；對照 Chromium 的 `base::sequence_manager`），renderer 的主 thread、worker 和 `dom`、`dom_api`、`dom_js`、`dom_wasm` 的測試都用它，取代直接呼叫的 `next_task`、`run_due_tasks` 和 `ScriptEngine` 只剩測試在用的 `run_element_tasks`、`run_posted_messages`、`run_timers`（helper 的形狀見第 9、10 條）。測試和正式的程式是同一份順序規則。不選核心放在 `renderer`、測試另寫一個照記號排的小 queue：`dom` 的測試不能依賴 `renderer`，兩份規則可能走樣（例如延後的 task 什麼時候搬）。
6. **worker 的 `close()` 照 Chromium：那個 task 結束時丟掉排好的 task**（2026-11-25，使用者決定；A）：`close()` 只設 closing 的旗標；那個 task 和它的 microtask checkpoint 跑完，worker 的 loop 看到旗標就清掉自己的 scheduler（排好的 task 都不跑），照之前的程序結束。看得到的結果和 HTML 的 close a worker 一樣：剩下的 task 都不跑。對照 Chromium：`WorkerGlobalScope::close` 設 `closing_`，`WorkerThread::DidProcessTask` 跑完 checkpoint 之後 `PrepareForShutdownOnWorkerThread`，`worker_scheduler_->Dispose()`。不選在 `close()` 裡當場丟掉（照 HTML 的字面）：結果一樣，但要在 script 的呼叫裡碰 scheduler，那個 task 剩下的部分還要在 queue 已經清掉的狀態下跑完。
7. **第 1 段：`base` 的核心連鎖和等待一起包**（2026-11-25，使用者決定；B）：`base::sequence_manager::SequenceManager` 有自己的 `Mutex` 和 `Condvar`，提供 `post`、`post_at`、`post_with`、`next`（可以有時限）、`try_next`、`wait_for`、`has_task`、`take_matching`、`put_back`、`sync_wait`、`post_reply`、`close`；使用的人照 `Spec` 給 queue 的種類、task、回答、同一把鎖底下的狀態（`Extra`）和取走 task 時的 hook（`taken`）。主 thread 留下 task 和 queue 的種類、`posted_once`、`shutting_down`、DevTools 的打斷。等待和叫醒的細節（為延後的 task 醒來、時限、同步等）只有一份。對照 Chromium：`SequenceManager` 連同它的 `ThreadController` 的等待。不選只包沒有鎖的資料結構、等待各自寫：等待寫兩次。
8. **第 2 段：`importScripts()` 等 Browser 時只取結束和 Browser 兩個 queue**（2026-11-25，使用者決定；A）：是自己的回應就拿、是重導向就跟；別的載入的回應先拿出來，等完放回 Browser 的 queue 的最前面（`put_back`）；`Stop` 只是叫醒它（是不是要結束看 `shared` 的旗標，主迴圈每次先看）。`Worker` 送來的、DOM 的、自己的工作照順序留在 queue 裡。對照 Chromium：`WorkerClassicScriptLoader::LoadSynchronously` 是同步的呼叫，其他 task 留在 queue 裡。不選同步的載入走 `sync_wait`（照主 thread 的 `sync_call`）：分派 Browser 訊息的地方（主 thread 的 route）要查一份「哪些 request id 是同步的」的表，重導向也要經過它。
9. **第 3 段：分成 3a 到 3d；3d 的測試用一個「跑完到期的」的 helper**（2026-11-25，使用者決定；A）：3a 照編號拿、拿掉 `target_task` 和時鐘的換算；3b port 的狀態機；3c debug build 的偵測；3d 拿掉 `next_task`、一輪的機制和 engine 只剩測試在用的 `run_due_tasks`、`run_timers`、`run_element_tasks`、`run_posted_messages`。測試的 engine 建立時裝一個 `TaskWaker`，記號送進 `base` 的 `SequenceManager`；helper 拿出到期的、拿編號呼叫 `run_dom_task`。原本只跑一種的測試改成跑全部到期的（和正式的一樣），靠「只跑一種」的逐一改寫。不選 helper 也照種類分：正式的程式沒有「只跑一種」的順序，測試守的和正式跑的不一樣，而且 helper 要問 `dom` 每個編號是哪一種。
10. **3d 的 helper 放在 `dom`，engine 的測試包一層**（2026-11-25，使用者決定；A）：`dom` 有一個公開的 `TaskTokens`：`base` 的 `SequenceManager`，一個 queue，`waker()` 給 `Dom::set_task_waker`，`take_due()` 一次拿出當下到期的全部記號（照 Blink 的 `test::RunPendingTasks()`：跑的期間新排的留到下一次，`setInterval(0)` 不會無窮迴圈；要連它們排的一起跑完的測試另外有跑到沒有為止的 helper）。`dom` 的測試直接用它，配 `task_numbered` 和照編號的 take；`dom_js` 的測試把 engine 包成 `TestEngine`（`Deref` 到 engine），建立時裝 waker，它的 `run_due_tasks()` 取代原本的四種呼叫；`dom_js` 的 `Env` 改用匯出的 `run_dom_task`。對照 Chromium：測試用 `base::test::TaskEnvironment`。不選 helper 放在 `dom_api`、`dom` 的測試另寫一份：記號的規則有兩份。不選 engine 沒有 host 的 waker 時自己裝一份、保留 `run_due_tasks`：正式的 engine 帶著一條只有測試走的路。
11. **3b：port 的狀態寫成兩個佇列加一個旗標，一端關掉時同一條 thread 也叫醒**（2026-11-25，實作時改的，收尾時使用者同意寫進來）：第 3 條寫的 enum（`Waiting`、`Running(VecDeque<(Stamp, Message)>)`）放不下跑著的 port 上「別的 thread 送來、還沒輪到編號」的訊息，同一條 thread 後來送的又要排在它們後面；所以是 `incoming`（沒編號）、`ready`（`(Stamp, 訊息)`）和 `running`，`post` 在當場編號前先把 `incoming` 編完。沒在跑時 `ready` 一定是空的（`move_to` 放回 `incoming`）。「有編號卻沒送記號」照樣在型別上做不出來；「能送卻沒編號」由 `enable`、`arrived`、`post` 先編完 `incoming` 守著，各有必壞的 mutation。另外，之前每個 task 之後都看一次 port，再也收不到的 port 會離開 context 的清單；拿掉之後只有叫醒排的 task 會看，所以一端關掉時另一端的 thread 一定叫醒（同一條 thread 也是），最後一則被拿走、對方已經關了時也當場離開清單，否則它一直留在清單裡、回收不了。
12. **3d：`base` 的核心可以照給的時刻取 task，測試用自己的時鐘**（2026-11-25，實作時加的，收尾時使用者同意寫進來）：`SequenceManager::try_next_by(wanted, now)`、`TaskTokens::take_due_by`、`Cx::instant_in`（window 或 worker 的時間換成記號用的 `Instant`）、`dom_js::instant_in` 和可以給時間的 `dom_js::run_dom_task`。`dom_api`、`dom_js` 的 timer 測試用假的時間（spin button 的 250 ms、`1e9` 毫秒之類），只有真的時鐘的話要真的等；排序規則還是 `queue_due` 那一份，只是「現在」由呼叫的人給。照假的時間跑的 task 也要用那個時間（interval 的下一次從它算），否則 interval 一直到期。對照 Chromium：`TaskEnvironment` 的 `TimeSource::MOCK_TIME`。順帶公開的：`TimerState::number`（測試照編號拿 interval）、`Cx::context_window`。不選只用真的時鐘、測試等：timer 的測試要等幾百毫秒到幾秒，而且時間一緊就不穩。

## 待決定

沒有。

## 之後的可能

- port 搬到 `ipc` 上、能跨 renderer（`docs/limitations.md` 的 `MessageChannel` 那一條）：`ipc` 要先有能放進訊息裡轉交的 message pipe、在擁有它的 thread 上叫醒（Mojo 的 ports 和 watcher）；`PortEnd` 的 `post`、`enable`、`arrived` 就是到時候 handler 接上的入口。
- shared worker：同一套 scheduler，好幾個文件連到它。
- worker 的 DevTools session：worker 的 scheduler 多一個 DevTools 的 queue，像主 thread 的。
