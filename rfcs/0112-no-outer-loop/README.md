# RFC 0112：拿掉 event loop 的外圈

- 開始日期：2026-11-24
- 狀態：🚧 已排程：2026-11-24（第 0 到 5 階段做完；做完之後量 Speedometer 找到的問題在修，`progress.md` 的第 7、8 步）
- 細節：`details.md`；進度：`progress.md`

## 摘要

renderer 的 event loop 原本分兩層：內圈一個個跑 scheduler 的 task，跑完通道上排著的每一個之後，才輪到外圈（`RenderThread::run` 在那一批之後的二十來個步驟）：DOM 的 timer 和其他 task、parser 的續跑、子 frame 的 commit、DevTools 的 frame 事件、導覽、blob URL 的撤銷……。通道一直有 task 時它們就一直晚，先後由外圈的那張清單決定。Blink 沒有外圈：一件事不是在發生的當下做，就是 scheduler 上某個 task queue 的 task，或是每個 task 結束時的觀察者（`TaskObserver`）。這份 RFC 讓 machopo 照同樣的三類：導覽在 setter 裡交給 Browser、DevTools 的 frame 事件在改變的當下送；DOM 的 task、parser 的續跑、commit、renderer 自己的 timer 和 V8 的 platform task 都是 scheduler 的 task；Chrome 也在 task 之後做的是 task 之後的觀察者。外圈拿掉了：`RenderThread::run` 只剩取一個 task、跑它、task 之後的觀察者、沒有 task 時的 idle callback，和要不要 main frame（每個 task 結束時告訴 compositor）。

## 動機

**做之前**（2026-11-24 照程式看的，`details.md` 的「盤點」）：

- `RenderThread::run`（`crates/renderer/src/render_thread.rs`）：等一個 task，然後在內圈一個個跑通道上已經排著的（`scheduler.try_next`），每個之後跑 DOM 在那個 task 裡排的 task（`run_queued_tasks`）、更新標題；通道空了才往下走外圈。
- 外圈依序做：caret 的閃爍、新內容的 rendering 逾時、intersection 的延遲、跨 origin 的 iframe 晚到的 `load`、`<select>` 的選單、frame 的要求（`handle_frame_requests`：導覽、blob URL）、子 frame 的 commit、DOM 的 timer 和其他到期的 task（`run_due_tasks`）、parser 的續跑（`parse_on`）、背景圖和 parser 的 script 的請求、等 sheet 的 script、canvas 的 `toBlob`、WebDriver 的結果、console 的警告、標題、blob URL 的撤銷、network idle、DevTools 的 frame 和 DOM 的變動、DevTools 的 `CSS` 要的 sheet、overlay、engine 的 platform task（GC）、idle callback、DOM 變了要不要 frame。
- DOM 的 task（timer、element task、`postMessage`）在 `dom` 的佇列，不在 scheduler 裡：在一個 scheduler 的 task 裡排的，那個 task 之後跑；timer 只在外圈跑。
- 導覽在每次呼叫 engine 之後的 `handle_frame_requests` 送給 Browser（RFC 0019 第 24、35 條：task 結束時，Chromium 是呼叫的當下）。

**問題**：

- **通道一直有 task 時外圈一直晚**：內圈不停，timer、parser、commit、DevTools 的事件都等著。RFC 0110 的 A6 就是這一類（一個 frame 算出 intersection 的 entry 之後，通道上排著的 task 讓外圈晚到下一個 frame 的 `requestAnimationFrame` 之後），它改成 DOM 的 task 才修掉；其他留在外圈的照樣會。
- **先後由清單決定**：DOM 的 timer 和 scheduler 的 task 之間、parser 的續跑和 timer 之間的順序，是外圈的步驟排在哪裡，不是 HTML 的 task source 和 Chrome 的 scheduler 的規則。加一個新的步驟就要在清單裡挑位置，排錯了是下一個同類的 bug。
- **DevTools 看到的順序和 Chrome 不同**：frame 的事件在那一輪最後才送，同一個 task 裡建了又拿掉的 iframe 看不到，新的 frame 的 `executionContextCreated` 在它的 `frameAttached` 之前。
- **導覽晚交給 Browser**：差 task 剩下的時間；Chrome 在每個 setter 裡送、後一個取消前一個。

解決的 `docs/limitations.md` 裡的限制：「DevTools 的 `Page.frameAttached`、`frameDetached`、`frameNavigated` 在 renderer 的 loop 那一輪最後才送」；「`location` 的導覽」那一條的「交給 browser 的時間是 task 結束時」。

**目標**（都做到了）：

1. 盤點外圈（和內圈每個 task 之後）的每一步，對照 Chromium，分成三類：Blink 當場做的、Blink 排成 task 的、Chrome 也在 task 之後做的（`details.md` 的表）。
2. 定規則：Blink 當場做的當場做；Blink 排成 task 的是 scheduler 的 task，和其他 task 照排進來的先後；只有 Chrome 也在 task 之後做的是 task 之後的觀察者，寫明對應 Chrome 的哪一個。
3. 照規則改，拿掉外圈。不加新的 `Rc`、`RefCell`（RFC 0110 的規則：兩邊都要的資料由 `dom` 或 engine 擁有，hook 拿參數）；用到的都是 RFC 0110 第 6 條允許的 DevTools agent 的 `Rc`，和 IO thread 本來就共享的 `Arc<MainThreadScheduler>`。

**不在範圍內**：

- 導覽帶著解析好的 blob（`BlobURLToken`）：撤銷本身改成 task 之後送（第 12 條），只剩導覽帶 token 留給另一份。
- RFC 0110 第 7 條排除的擁有權的尾巴：DevTools agent 之間的 `Rc`、`BlobUrls`、worker thread 的 `Rc<RefCell>`、readiness、encoding、mime type 的副本。
- worker thread 自己的 loop（`worker_thread.rs`）：照舊用一輪的機制（第 14 條），只多跑 V8 的 platform task（第 18 條）。
- Chrome 完整的 scheduler 優先權（input 的 high priority、rendering 的 starvation 防護……）：現有的 `TaskQueues` 照舊，新的 queue 和其他的照排進來的先後。

## 設計概要

給之後改這部分程式的人：renderer 的工作只有三種位置，沒有「等通道空了」：

- **發生的當下**：`location.href = x` 返回時 Browser 已經收到導覽；插入 iframe 返回時 DevTools 已經收到 `frameAttached`，在那個 frame 的 `executionContextCreated` 之前；子 frame 的 `frameNavigated` 在它的 realm 和 script 之前；DevTools 的 `CSS` 的命令要的 sheet 在命令的 task 裡就去抓。
- **scheduler 的 task**：`setTimeout` 的 callback、element task、訊息各是一個 task（`TaskQueue::Dom`）；parser 讓出之後的續跑、子 frame 的 commit、等 sheet 的 script、`javascript:` 網址的 commit、renderer 自己的 timer（caret、intersection 的 `delay`、延後的 iframe `load`、新內容的逾時、network idle、wasm guest 的等待）、V8 的 platform task 是 `TaskQueue::Internal` 的 task。和其他 task 照排進來的先後輪到，延後的到期、scheduler 挑下一個時才排進去；輪到時再看還該不該做。
- **task 之後的觀察者**（`after_task`，Chrome 的 `TaskObserver::DidProcessTask`）：每個 task 之後依序做，加新的步驟時寫明對應 Chrome 的哪一個；沒有 task 時跑一個 idle callback；要不要 main frame 在每個 task 結束時告訴 compositor（Blink 的 `SetNeedsAnimate` 在要求的當下）。

## 詳細設計

### loop

`RenderThread::run` 每一圈：標成 idle（給 DevTools 的 profile）、等下一個 task 或到 idle callback 能跑的時候（`scheduler.next(TaskQueues::ALL, until_idle)`）、跑那個 task（`run_task`）或一個 idle callback、`after_task`；接著看要不要 main frame（`update_begin_main_frame`：要求了 frame、animation frame 的 callback、resize loop 的錯誤、跑著的 transition 和 animation、等著的 mouse move）：每個 task 結束時照實告訴 compositor（Blink 的 `SetNeedsAnimate` 在要求的當下），後面還有 task 也一樣，頁面一直排 task 給自己時 frame 照樣來（第 26 條）。

### 當場做的

- **導覽**（第 10、11 條）：`FrameClient::navigate`、`navigate_child` 在 setter 裡送 `BeginNavigation` 和其他導覽的訊息，請求的編號是 thread-local 的計數器（`loader::next_begin_navigation_id`），engine 送出時當場告訴 DOM frame 在載入中；renderer 的帳在呼叫之後照送了什麼對照。同一份文件內的 traversal 在 `popstate` 之前告訴 Browser。Browser 在發起導覽的 task 結束（`NavigationCancellationWindowEnded`，每個 task 之後送）之前扣著 renderer 發起的導覽的 commit（`HeldCommit`），同一個 frame 的下一個 `BeginNavigation` 先到就取消前一個（Chrome 的 `RendererCancellationThrottle`）。
- **DevTools 的 frame 事件**（第 6 到 8 條）：`RendererFrameClient` 拿 Page、DOM agent 的 `Rc`，`child_frame_created` 先送出排著的 DOM 變動、再送 `frameAttached`（stack 由 `create_child` 當場取），`frame_detached` 送 `frameDetached`；子 frame 的 commit 在 `create_window` 之前把新文件交給 Page agent，engine 建好文件、建 realm 之前呼叫 `child_document_created`，Page agent 送 `frameNavigated`。
- **`CSS` 的重抓**（第 19 條）：`devtools_message` 在 renderer 的 agent 回答之後當場 `start_css_refetches`。
- **`javascript:` 網址的文件**（第 23 條）：有建立它的文件的 origin，沿用初始 `about:blank` 的視窗。

### scheduler 的 task

- **DOM 的 task**（第 3、13 到 15 條）：`dom` 擁有 host 設的 `TaskWaker`，排 task 時送帶著 DOM 的 task 編號的記號（timer、UA 的 timer、idle callback 的 timeout 帶到期的時間），renderer 的 waker 經過 `Arc<MainThreadScheduler>` post `MainThreadTask::DomTask` 到 `TaskQueue::Dom`；輪到時 engine 跑那個編號的 task，不管它在 DOM 的 queue 裡排第幾（`run_dom_task`、`Cx::target_task`，第 25 條），之後一次 microtask checkpoint；取消了的什麼都不做。debug build 在 loop 要等之前檢查有沒有到期卻沒有記號的 DOM task。port 的訊息照佇列的順序拿編號（`number_port_messages`，task 之後掃描），port 啟用時重送已經編號的記號。
- **renderer 自己的工作**（第 16、17 條）：`InternalTask` 的每一種在要做或設定時間的地方 post（`post_internal`、`post_internal_at`），輪到時 `run_internal` 再看還該不該做。network idle 和 wasm guest 的等待在每個 task 之後照最早的期限排，intersection 的 `delay` 在每個 frame 重算時排（第 21、22 條）：這種一直被要求同一個最早時間的經過 `post_internal_once_at`，scheduler 記著每一種排了哪個時間，同一個、比較晚、或早不到 1 ms 的不再排，取出之後再要求就重新排（第 24 條）。
- **V8 的 platform task**（第 18 條）：`bindings` 用 rusty_v8 的 custom platform，V8 post 的 foreground task 照 isolate 存在 `platform_tasks` 的表，叫醒那個 isolate 的 thread：主 thread post `InternalTask::PlatformTasks`，worker 的 loop 收 `WorkerTask::PlatformTasks`，各自跑到期的。

### task 之後的觀察者

`after_task` 依序：port 的訊息編號；`<select>` 的選單和 frame 的要求（engine 的呼叫留下的導覽的帳、blob URL）；engine 沒送的圖片和 script 的請求；`toBlob` 的編碼（第 20 條）；WebDriver 的結果；canvas raster 失敗的 log；標題；blob URL 的撤銷（那個 task 要的載入都已經送出，第 12 條）；導覽的取消窗口；等 sheet 的 script 再試；network idle 和它的延後 task；wasm guest 的延後 task；DevTools 的事件（DOM 的變動、console 的警告、比對 frame）；lifecycle 之後 `CSS` 要的 sheet；overlay 要 frame；DOM 變了要 frame；沒有載入拿走的 script 的 stack。DOM 的 task 之後另外判斷 `load`、送 testharness 的結果（`after_dom_tasks`）。

### 和 Chrome 不同、刻意保留的

- scheduler 沒有優先度：`TaskQueue::Dom`、`TaskQueue::Internal` 和其他 queue 照排進來的先後（第 15、17 條）；Chrome 的 input 比較高，machopo 的 `forms::reset_buttons_and_links` 的兩個 click 照先後跑（第 13 條）。
- `<select>` 的選單、WebDriver 的結果、`toBlob` 在 task 之後，不是當場或 idle task（第 19、20 條）：差別只在同一個 task 之內。
- engine 的 inspector 的回答（`Runtime.evaluate` 裡改了 DOM）比那個 task 的 DOM 變動先到（第 9 條，記在 `docs/limitations.md`）。
- worker 的 loop 照舊一輪一輪跑（第 14 條）。

### 測試

- 必壞的測試（改之前在原本的程式上每次都失敗），在 `shell` 的 `event_loop`、`devtools`、`devtools_dom`：通道一直有 task 時 timer、parser、`toBlob` 照樣進行；設 timer 之後、跑過它的時間的 task 裡 post 的訊息在 timer 前面（Chrome 的順序）；`location.href = x` 之後忙一陣，Browser 在忙完之前收到導覽；同一個 task 的兩個導覽只 commit 第二個；DevTools 的 `childNodeInserted`、`frameAttached`、`executionContextCreated` 照 Chrome 的順序，建了又拿掉的 iframe 也收到 `frameAttached` 和 `frameDetached`。清單在 `progress.md` 的「驗收」。
- 每一段跑 mutation test（`mutants-1.toml` 到 `mutants-5.toml`）和相關的 WPT 目錄。

### 分階段

0. 評估，不改產品：盤點、DOM 的 task 進 scheduler 的 T1、T2 兩個原型（`prototypes/`，不合併），交給使用者決定（第 1 到 5 條）。
1. DevTools 的事件（第 6 到 9 條）。
2. 導覽在呼叫的當下送（第 10 到 12 條）。
3. DOM 的 task 是 scheduler 的 task（第 13 到 15 條）。
4. renderer 自己的工作和 V8 的 platform task 排成 task（第 16 到 18 條）。
5. 剩下的步驟改成 task 之後的觀察者和延後的 task，拿掉外圈（第 19 到 23 條）。

## 缺點

- timer、parser、commit 的時序都變了，導覽和 parser 的 WPT 最可能變動；整套由 CI 跑（每一段跑過的目錄見 `progress.md`）。
- 當場送的 hook 變多：每一個都要證明被呼叫時不會回頭執行 script（RFC 0110 的 hook 的規則），DevTools 開著時每次改變都多一次工作。
- DOM 每排一個 task 多一次 scheduler 的 lock 和 push；取消的 task 留下的記號要跑到才知道沒有事。效能沒有量。
- 導覽當場送推翻了 RFC 0019 的兩條，Browser 多了扣住 commit 的狀態。

## 理由和其他做法

- **只在每個 scheduler 的 task 之後也跑外圈**：通道一直有 task 時不再晚，但順序還是清單決定的，DevTools 和導覽照樣在 task 結束時，和 Chrome 不同。不解決「加一步就要挑位置」。
- **一項項修**（像 RFC 0110 的 A6 那樣）：每次修一個症狀，外圈照樣在，下一個同類的 bug 照樣長出來。
- **DOM 的 task 由 scheduler 取 task 時問 DOM（T2）**：改動比較少，但順序由內圈的程式比兩邊，每個 engine 都要實作查詢（第 3 條）。
- **不做**：當時沒有測試因為外圈而不穩定，網頁看得到的只有時序和 Chrome 不同；風險是之後通道更忙（site isolation、更多 process 的訊息）時外圈的工作更晚。

## 先例

- **Chromium**（2026-11-24 對過 `main` 的原始碼，`details.md` 的「盤點」）：renderer main thread 的 `MainThreadSchedulerImpl` 和 `SequenceManager`，每個 frame 的 `FrameScheduler` 照 task type 分 task queue（timer、networking、DOM manipulation、parser……），Blink 排 task 時經過 `ExecutionContext::GetTaskRunner(TaskType)`；DevTools 經過 `probe::` 當場送；導覽經過 `LocalFrameClient::BeginNavigation` 在 setter 裡同步送 `BeginNavigation`；task 之間的工作是 `TaskObserver`（`IdlenessDetector`、DevTools 的通知在每個 task 之後 flush）、idle period 和 rendering 的步驟；scheduler 先看優先度，同一級裡跨 queue 取 enqueue order 最舊的。沒有「通道空了才做」的外圈。
- **HTML**：event loop 每一輪取一個 task、microtask checkpoint、必要時 update the rendering；timer、parser、導覽都是 task source 的 task。
- 其他瀏覽器沒有對照。

## 已決定的事項

1. **開始做第 0 階段**（2026-11-24，使用者要求）：只評估、不改產品。
2. **原型用「通道一直有 task 時的 timer」比**（2026-11-24，使用者同意）：T1、T2 的原型都要讓它從必壞變成必過，再比改動、成本和兩個 engine 各要做什麼。理由：它是外圈最根本的問題（內圈不停、外圈的工作一直晚），也是 DOM 的 task 進 scheduler 的兩種做法差別最大的地方；導覽和 DevTools 的事件是當場做的 hook，和 DOM 的 task 怎麼排無關，不用原型比。
3. **DOM 的 task 用 T1：排的當下告訴 scheduler**（2026-11-24，使用者同意；原本「待決定」的第 2 條）：`dom` 擁有 host 設的 `TaskWaker`，排 task 時送一個記號、timer 的記號在到期時排進 scheduler（編號在那時給，和 Chrome 的 delayed task 到期時才拿到 enqueue order 一樣），scheduler 輪到記號時跑一個 DOM 的 task；task 的內容照 RFC 0110 留在 `dom`。理由：和 Blink 一樣（`ExecutionContext::GetTaskRunner(type)->PostTask`），順序只在 scheduler 一個地方決定；waker 在 `dom` 裡，兩個 engine 一處設好（`make_engine`）；之後 `until_next_due` 不用再問 DOM 的 timer。代價：DOM 每排一個 task 多一次 scheduler 的 lock 和 push；取消的 task 留下的記號要跑到才知道沒有事。不選 T2（scheduler 取 task 時問 DOM）：改動比較少（+62 對 +107 行），但順序由內圈的程式比兩邊，每個 engine 都要實作查詢（原型的 wasm engine 沒有），順序比 Chrome 早。不選 T3（只把 `run_due_tasks` 搬進內圈）：順序還是清單決定的。原型和比較在 `details.md` 的「原型」。
4. **範圍：拿掉整個外圈**（2026-11-24，使用者同意；原本「待決定」的第 1 條；A）：外圈的每一步照 `details.md` 的盤點去它在 Chrome 的位置（當場、scheduler 的 task、每個 task 之後），只有 blob URL 的撤銷（`flush_revoked`）例外，留給導覽帶著解析好的 blob 那一份。理由：只做導覽、DevTools 的事件和 DOM 的 task（B）時外圈還在，剩下的步驟照樣會晚，之後還要另一份 RFC 才拿得掉；盤點之後每一步在 Chrome 都有明確的位置，沒有要另外設計的。代價：parser、commit 和 renderer 自己的 timer 都改成 task，WPT 的時序要整套重看（CI）。
5. **分五段**（2026-11-24，使用者同意）：照「分階段」的第 1 到 5 段，由小到大，每段做完可以先 merge。理由：DevTools 的事件和導覽是當場做的 hook，不牽動 event loop，先做風險最低；DOM 的 task 要先進 scheduler，renderer 自己的工作才有一致的規則可循；拿掉外圈放最後，那時剩下的步驟最少。
6. **第 1 段的第 1 項：`frameAttached`、`frameDetached` 在 `FrameClient` 的 hook 裡當場送**（2026-11-24，使用者決定；A）：`RendererFrameClient` 拿 Page agent 的 `Rc`（RFC 0110 第 6 條允許的那一類），`child_frame_created` 送 `frameAttached`、`frame_detached` 送 `frameDetached`，Page agent 記下哪些已經告訴過，之後的比對不再重送。對照 Chrome：`InspectorPageAgent::FrameAttachedToParent` 在 `LocalFrame` 的 constructor 裡送、接著 `flush()`（註解：Browser 會報告這個 frame 的網路事件，`frameAttached` 要先到前端），`FrameDetachedFromParent` 在 `DetachImpl` 裡送。不選照舊比對、改成每個 task 之後（B）：同一個 task 裡建了又拿掉的 iframe 照樣看不到，也排不出 `frameAttached` 在 `executionContextCreated` 之前。
7. **第 1 段的第 2 項：新 frame 的 `childNodeInserted`、`frameAttached`、`executionContextCreated` 照 Chrome 的順序**（2026-11-24，使用者決定）：`frameAttached` 的 stack 由 dom_js 的 `create_child` 在 DevTools 要 frame 的 stack 時當場取一份、當參數給 hook（Chrome 的 `FrameAttachedToParent` 也是自己 `SourceLocation::CaptureWithFullStackTrace()`，和導覽請求的 initiator 分開取）；`create_child` 先呼叫 client，再建 realm、告訴 inspector；`child_frame_created` 多拿 `&Dom`，`RendererFrameClient` 也拿 DOM agent 的 `Rc`，送 `frameAttached` 之前先送出排著的 DOM 變動。不選把 operation 結束時取 stack 移到 `update_elements` 之前、讓 `create_child` 讀它：只取一次，但改了每個 operation 的順序，和 Chrome 分開取的做法也不同。
8. **第 1 段的第 3 項：子 frame 的 `frameNavigated` 在 commit 的當下、realm 之前送；比對出來的其他 Page 事件在有變化的 task 之後比對**（2026-11-24，使用者決定；先選 B，實作時發現 B 的前提對子 frame 不成立，改成 C1）：
   - frame 建立或拿掉、頂層和子 frame 的 commit 時設一個旗標，每個 task 之後旗標有設才比對（B 的部分：新 frame 初始的 `about:blank`，和 DOM agent 拿到 iframe 的新文件）；收到 DevTools 的指令時和子 frame 送導覽之前照舊先比對。導覽開始或結束不設：它只改 `navigating`，比對只在新的 frame 上用到它（mutation test 看出來的）。外圈的步驟在第 4 段之前還不是 scheduler 的 task，外圈最後也呼叫同一個「task 之後」，照樣看旗標；console 的警告不管有沒有 session 都先交給 log agent（它印出來、留給之後的 session）。
   - 子 frame 的 commit（`commit_child`，子 frame 的 commit 只經過它）在 `create_window` 之前把新文件的 `FrameInfo` 交給 Page agent（沒有 node id）；dom_js 的 `create_window` 在建好文件之後、告訴 inspector 新 realm 之前，呼叫 `FrameClient` 新的 `child_document_created`（沿用初始 `about:blank` 視窗的那條路也呼叫），Page agent 補上文件的編號，renderer 自己 commit 的文件連 loaderId 也一起補上，然後送 `frameStartedLoading`、`init`、`frameNavigated`；之後的比對認得是同一份文件，不再重送。頂層的 commit 本來就在 `create_window` 之前告訴 Page agent。
   - 對照 Chrome：`frameNavigated` 在 `WillCommitNavigation` 送，在新文件的 context 和 script 之前。
   - B 的前提錯在哪：當初以為 commit 自己是一個 task，task 結束時比對看不出差別。但子 frame 的 commit 在同一個 task 裡建 realm（送 `executionContextCreated`）、跑 inline script，所以 `frameNavigated` 會晚到它們之後（臨時的測試量到 `console` 在前）。
   - loaderId 的做法：renderer 自己 commit 的文件（`javascript:` 網址的空文件；`srcdoc` 和 `about:blank` 也經過 browser），loaderId 是文件的 node id，`create_window` 之前還不知道。所以由引擎的 hook 補上。不選改用 commit 前就分配好的 `DocumentId`：它和 browser 的導覽編號是兩個不同的計數器，同一個 frame 可能出現相同的 loaderId。
   - 不選每個 task 之後都比對（A）：開著 DevTools 時每個 task 都要重建整份清單（frame 數的平方）。
   - 不選照舊比對、把差別記進限制（B）：Puppeteer 這類工具等 `frameNavigated` 之後才找 context，會接不上。
   - 當初不選 C 的理由是「commit 的路很多、會漏」，實作時發現不成立：子 frame 的 commit 只有一條路。
9. **第 1 段的第 4 項：DOM 的變動和 console 警告在每個 task 之後、renderer 的 agent 回應之前送**（2026-11-24，使用者決定；A）：內圈每個 task 之後（有 DOM agent 開著才取），`frameAttached` 之前（第 7 條）；renderer 的 agent 的回應之前不用另外取：實作時發現 DOM、CSS、Overlay agent 的指令都不改 DOM（machopo 沒有 `DOM.setAttributeValue` 這類），它們本來就在處理指令之前先取，回應之前已經沒有排著的。對照 Chrome：通知在改變的當下產生、進佇列，`DidProcessTask`、送每個 CDP 回應之前、`flush()` 時送出。和 Chrome 不同、記進 `docs/limitations.md` 的：engine 的 inspector 的回應（`Runtime.evaluate` 裡改了 DOM）照樣先到，變動在那個 task 結束時才到。不選連 engine 的 inspector 的回應之前也送（B）：回應在 V8 裡經過 `devtools_send` 送出，那時手上沒有 `Dom`，要給兩個 engine 的 inspector channel 加拿 `&Dom` 的 hook；script 暫停時訊息要當場送，不能先存起來。

10. **第 2 段：導覽由 hook 當場送，renderer 的帳在呼叫之後對照**（2026-11-24，使用者決定；原本「待決定」的第 1 條；C）：
   - `FrameClient` 加一個導覽的 hook，在 setter 裡當場送 `BeginNavigation`、`DidChangeSrcDoc`、同一份文件內的 commit、`GoToEntryAtOffset`、`OpenUrl`。
   - 請求的編號改成 thread-local 的計數器（RFC 0110 的請求編號的做法）；iframe 的 `referrerpolicy` 和「上一頁載入的父文件裡新建的 frame」用 `&Dom` 讀；DevTools 的 initiator 由 hook 拿 Network agent 的 `Rc` 算（load sink 的做法）。
   - 送了什麼記成一筆，renderer 在呼叫之後照舊更新自己的帳（子 frame 的導覽、繼承 origin 的文件、frame 在載入中）；不送 browser 的 `javascript:` 和 fragment 照舊在呼叫之後。
   - 理由：和 Chrome 一樣在 setter 裡送（Blink 的 `LocalFrameClient::BeginNavigation` 呼叫 content 的 `RenderFrameImpl::BeginNavigation`，當場組參數、經過 mojo 送；這一段憑記憶）；帳晚一點更新不會錯，讀它的只有 browser 之後的 commit 和 dropped，都是之後的 task；和第 6 條、RFC 0110 第 22 條同一種做法。
   - 代價：送和記帳在兩個地方，要寫在一起、對得上；「frame 在載入中」要在呼叫裡就讓 DOM 知道（同一個呼叫裡父文件的 `load` 會不會先檢查到，實作時確認）。
   - 實作時確認的：「frame 在載入中」由 engine 在 hook 送出導覽時自己告訴 DOM（`set_frame_navigating`），不等 renderer 的帳；原本就有的問題順便修掉：同一次呼叫裡導覽一個子 frame、再拿掉父文件最後等著的東西，父文件的 `load` 不等那個子 frame（`check_parent_completed` 在呼叫裡就檢查；`a_frame_navigated_as_the_last_wait_goes_delays_the_load`）。`javascript:` 網址不送 browser，照舊在呼叫之後才算載入中（記進 `docs/limitations.md`）。
   - 實作時多的：子 frame 初始的 `about:blank` 的 `init` 在 hook 送出它的第一個導覽之前由 Page agent 送（原本 `begin_child_navigation` 先比對再送導覽）；同一份文件內的 traversal 在 `popstate` 之前告訴 browser（`traverse_within_document` 多一個 callback：Blink 的 `DidFinishSameDocumentNavigation` 在分派 `popstate` 之前，憑記憶），否則 `popstate` 的 handler 發起的導覽比 traversal 的 commit 先到 browser；「上一頁載入的父文件裡新建的 frame」改從 DOM 的 navigation timing 讀，`FrameDocument.history_load` 拿掉。
   - 讀程式時發現：RFC 0019 第 24 條的「同一個 task 只留最後一個」已經不是現在的行為，每個導覽都依序送，browser 收到同一個 frame 的下一個 `BeginNavigation` 時丟掉前一個（`take_any_navigation`、`NavigationDropped`）；和 Chrome 不同的只剩送出的時間。
   - 不選帳搬進 `dom`（A）：renderer 讀 `ChildFrames.navigations` 的幾十處都要改，`dom` 多了 browser 那一層的概念。不選 engine 當參數帶（B）：「上一頁載入的父文件」和 DevTools 的 initiator 是 renderer 的，engine 給不出來，最後還是要像 C 那樣補一份。
11. **第 2 段：同一個 task 裡連續的導覽照 Chrome 的取消窗口**（2026-11-24，使用者決定；原本「待決定」的第 2 條；A）：renderer 在發起導覽的 task 之後告訴 browser 那些導覽的取消窗口結束了（新的訊息，Chrome 的 `NotifyNavigationCancellationWindowEnded`）；browser 在那之前不送 renderer 發起的導覽的 commit（Chrome 的 `RendererCancellationThrottle`），同一個 frame 的下一個 `BeginNavigation` 在那之前到了就取消前一個。理由：導覽在 setter 當場送之後，script 送出第一個再忙一陣、才送第二個時，browser 可能已經拿到第一個的回應；沒有這個窗口，頂層 frame 會先 commit 第一個再 commit 第二個（多一筆 session history，閃過第一份文件），子 frame 的被 renderer 丟掉、browser 卻以為 commit 了。代價：多一個訊息和 browser 的等待狀態，導覽最快在發起它的 task 結束後 commit（和 Chrome 一樣）。不選 renderer 丟掉過期的 commit（B）：browser 和 renderer 的帳不一致。不選照舊等 task 結束才送（C）：第 2 段的目標不成立。

12. **第 2 段：取消窗口在 task 之後結束，blob URL 的撤銷也在 task 之後送；parser 的 script 和背景圖的請求先改成當場送**（2026-11-24，使用者決定）：
   - 實作 2b 時發現：導覽在 setter 當場送之後，browser 可能在 renderer 送出那個 task 撤銷的 blob URL（`flush_revoked`，原本在外圈）之前就 commit，之後再導覽到那個 URL 會成功（browser 的 `blobs` 測試在整組跑時逾時）。
   - 做法：parser 的 script（`start_script_fetches`）和 style 找到的背景圖（`start_image_fetches`）的請求在每個 task 之後就經過 load sink 送出（原本等外圈）；Chrome 在 `ScriptLoader::PrepareScript`、style recalc 的 `LoadPendingImages` 裡同步發 fetch（`details.md` 的盤點），差別只在同一個 task 之內，外面看不出來。接著 `flush_revoked`（每個 task 要的載入都已經送出），再結束第 11 條的取消窗口（Chrome 的位置），最後是 DevTools 的事件。
   - 推翻第 4 條的例外：`flush_revoked` 不再留給「導覽帶著解析好的 blob」那一份，改成 task 之後的觀察者；那一份要做的只剩導覽帶 `BlobURLToken`。
   - 不選取消窗口暫時在外圈的 `flush_revoked` 之後結束（A）：第 2 段比較小，但這兩種請求改成當場送之前，renderer 發起的導覽的 commit 都要等外圈，和 Chrome 不同。不選窗口在 task 之後結束、撤銷照舊在外圈（C）：導覽到 blob URL 立刻撤銷時，外圈跑到之前 browser 還拿得到它。

13. **第 3 段：一個記號跑它帶著的那一個 DOM task**（2026-11-24，使用者決定；A）：DOM 排一個 task 時送的記號帶著它是哪一個（佇列的種類、window、DOM 的編號），scheduler 輪到時只跑那一個，之後一次 microtask checkpoint 和 task 之後的觀察者；已經取消的（`clearTimeout`、文件不在了）什麼都不做。對照 Chrome：每個 `PostTask` 是自己的 task，取消的靠 weak pointer 在輪到時略過；timer 到期時才拿到 enqueue order，所以設 timer 之後、到期之前才排的 element task 在它前面。不選記號只說「DOM 有 task」、輪到時跑 DOM 最早的一個（B）：DOM 給 timer 的編號在設定時，會跑在那個 element task 前面，順序變成兩套。
   - 實作時定下、發現的（2026-11-24，使用者同意這幾項的做法）：記號帶 DOM 的 task 編號（每種 task 都從同一個計數器拿，interval 重新排時拿新的）；engine 找出最前面是那個編號的 queue（`Cx::task_numbered`；只認第一個會丟 task，第 25 條改成不管排第幾），照 scheduler 交出的時間跑它（DOM 的時鐘換算回來差一點也照樣跑），不在了就什麼都不做。
   - 到期的 timer 在 scheduler 挑下一個 task 時才排進 queue，不是有 task post 進來時（原型是後者）：Blink 的 `SequenceManager` 在 `SelectNextTask` 時搬到期的 delayed task；跑過 timer 時間的 task 裡 `postMessage` 的訊息在 timer 前面（Chrome for Testing 152 跑 5 次都是 `message,timer`，`event_loop::a_timer_takes_its_turn_as_it_is_due`）。
   - port 的訊息照佇列的順序拿編號：前面還有沒編號的（轉送時清掉了，`set_home`），後來的也先不編，等 task 之後掃描時一起編（`number_port_messages`）；port 啟用、加進 window 的清單時，替已經編號的重送記號（記號輪到時找不到它們）。WPT 的 `webmessaging/` 抓到的。
   - worker 自己關閉（`close()`）時，`Worker` 不再收東西，但它之前送的訊息和錯誤照樣送到；只有頁面的 `terminate()` 丟掉排著的（`Cx::close_worker_object`、`worker_messages_dropped`）。原本兩者一樣，是靠 loop 在處理 `Closed` 之前先跑完排著的 DOM task。WPT 的 `webmessaging/message-channels/worker*`、`workers/WorkerGlobalScope-close.html` 抓到的。
   - wasm engine 的 guest 的等待不是 DOM 的 task，照舊由 loop 叫醒、在外圈跑（`until_due_guests`、`run_due_guests`），第 5 段拿掉外圈時一起處理。
   - `shell` 的 `forms::reset_buttons_and_links` 改成讀 `hashchange` 的 `newURL`：測試一次送進兩個 click，第二個現在比第一個的 `hashchange` 先跑（照 scheduler 的先後；Chrome 的 input 優先度更高，同樣先跑，沒有在 Chrome 上確認）。

14. **第 3 段：主 thread 拿掉「一輪」的機制、外圈的 `run_due_tasks` 和內圈的 `run_queued_tasks`**（2026-11-24，使用者決定；A）：每個 DOM task 是 scheduler 的一個 task，畫面和輸入照 scheduler 的先後插進去（Chrome 沒有「一輪」：互相 `postMessage` 的頁照 enqueue order 和其他 task 交錯）。worker 的 loop 不經過這個 scheduler，照舊用一輪的機制（`begin_task_pass`）。不選留著外圈的當保險（B）：DOM 的 task 有兩條路跑，順序看哪條先到，第 3 條的「順序只在 scheduler 一個地方決定」不成立。
15. **第 3 段：scheduler 裡一個 `TaskQueue::Dom`**（2026-11-24，使用者決定；A）：跨 queue 照編號挑，DOM 的 task 之間、和其他來源之間都照排進來的先後；script 停在斷點時不取它。對照 Chrome：每個 frame、每種 task type 一個 queue，同一個優先度的照 enqueue order 挑；分開主要是為了背景分頁的節流，machopo 還沒有。不選照 task type 分（B）：現在沒有東西需要分開。

16. **第 4 段：renderer 自己的 timer 是 scheduler 的延後 task**（2026-11-24，使用者決定；A）：caret 的閃爍、intersection observer 的 `delay`、延後的 iframe `load`、新內容的逾時在設定時 post 一個延後的 task（沿用第 3 段的 delayed heap，到期、挑下一個 task 時才排進去），輪到時再看它還該不該做（取消了、改了時間的什麼都不做）。parser 的續跑、等 sheet 的 script、`javascript:` 網址的 commit 在要做的時候 post 一個 task（Chrome 都是排 0 ms 的 task，kNetworking）。對照 Chrome：這些都是 `TaskRunner::PostDelayedTask` 的 timer（`FrameCaret`、`ScheduleDelayedIntersection`、`DispatchLoad` 的 100 ms、Browser 的 `TimeoutMonitor`），取消的在輪到時略過。不選照舊由 loop 算醒來的時間（B）：順序還是 loop 決定的，第 5 段拿掉外圈時要再改。
17. **第 4 段：這些工作在一個 `TaskQueue::Internal`**（2026-11-24，使用者決定；A）：script 停在斷點時不取它，和 DOM 的 queue 一樣。對照 Chrome：kInternalDefault、kInternalLoading、kNetworking 和 V8 的 queue 大多是同一個優先度。不選照 task type 分（B）：現在沒有東西需要分開。
18. **第 4 段：V8 的 platform task 經過 `PlatformImpl` 送到它的 isolate 的 thread 的 scheduler**（2026-11-24，使用者決定；A）：`bindings` 改用 rusty_v8 的 `PlatformImpl`（V8 的 `PostTask`、`PostDelayedTask` 交給 Rust，不改 V8 的 C++、不用重編）：照 isolate 送到那個 thread（主 thread 的進 `TaskQueue::Internal`，延後的進 delayed heap；worker 的進它的 inbox），idle task 交給 idle period。對照 Chrome：V8 的三種 foreground task runner 是主 thread scheduler 的 `kMainThreadTaskQueueV8*`。代價：platform 是整個 process 共用的，要一張 isolate 到 thread 的對照表（一個 lock），worker 的 loop 要接收；動到 `bindings` 的 platform，照 `crates/bindings/soundness.md` 檢查。不選每個 task 之後拉一次（B）：V8 延後的 task 照舊沒有東西叫醒 loop。不選這一段不做（C）。

19. **第 5 段：DevTools 的 `CSS` 重抓當場做，`<select>` 的選單和 WebDriver 的結果在每個 task 之後**（2026-11-24，使用者決定；A）：`CSS` 的重抓是 renderer 自己的 agent 的，在處理命令的那個 task 裡就送（Chrome 的 `InspectorResourceContentLoader` 在 `CSS.enable` 的 task 裡同步 fetch）；選單（Chrome 的 `ExternalPopupMenu::Show` 當場送 mojo）和 WebDriver 的結果（V8 inspector 在 promise 的 reaction 裡當場送）改成 task 之後的觀察者，差別只在同一個 task 之內。不選三個都當場（B）：選單和 WebDriver 的結果各要一個 engine 的 hook，改動大、外面看不出差別。不選三個都在 task 之後（C）：CSS 重抓比 Chrome 晚一點，又沒有省下什麼。
20. **第 5 段：`toBlob` 的編碼在每個 task 之後送到 encoder thread**（2026-11-24，使用者決定；A）：Chrome 在 main thread 的 idle task 裡逐段編碼、1 秒沒開始就改成立即；machopo 的編碼本來就在另一個 thread，task 之後送比 Chrome 早開始，結果一樣。不選照 Chrome 在 idle period 送（B）：多一套「1 秒之後改成立即」的 timer，換不到什麼。
21. **第 5 段：network idle 在每個 task 之後檢查，另外一個延後的 task 在安靜的窗口結束時**（2026-11-24，使用者決定；A）：Chrome 的 `IdlenessDetector` 是 `TaskTimeObserver`（task 的交界檢查），另有 watchdog 的 timer；延後的 task 取代 loop 為它算的醒來時間。不選照舊由 loop 算（B）：loop 就沒辦法只剩取 task、跑 task。
22. **第 5 段：wasm guest 的等待是延後的 task**（2026-11-24，使用者決定；A）：每次跑完 guest 之後，照它最早的期限排一個延後的 task（`TaskQueue::Internal`），和 renderer 其他的 timer 一樣，loop 的等待不再問 engine。不選照舊由 loop 問 engine（B）。

23. **第 5 段：`javascript:` 網址的文件有建立它的文件的 origin，沿用初始 `about:blank` 的視窗**（2026-11-24，使用者決定；A）：第 4 段把 iframe 的 `javascript:` 的 `src` 的 commit 改成 task 之後，WPT 的 `webmessaging/*/018.html` 逾時：commit 排在頁面的 `postMessage` 前面，建了新的 window，送給 `window[0]` 的訊息丟了（以前靠外圈晚一點 commit 才沒事；第 4 段沒有跑 `webmessaging/`）。照 HTML 和 Chrome，`javascript:` 網址的文件有發起它的文件的 origin，初始 `about:blank` 的視窗沿用：`origin_document` 和 engine 的 `inherits_origin` 把它當成 `about:` 的一樣。影響：這類文件的 origin 從 opaque 變成它的 iframe 的文件的。不選只讓 commit 晚一點（B）：要另外挑一個晚一點的位置，和 Chrome 不同，只是繞過這個測試。

24. **做完之後：「不重複排的延後 task」收進 scheduler，加一節給之後加新工作的人看**（2026-11-24，使用者同意）：回顧時看出三處（intersection 的 `delay`、network idle、wasm guest 的等待）各自記著排了哪個時間，同一段邏輯寫三次。改成 scheduler 的 `post_internal_once_at`：照 `InternalTask` 記最早的那一個，取出時清掉。時間從另一個時鐘換算成 `Instant`（renderer 的時鐘、`ipc::TimeTicks`、engine 的），每次換算差一點，所以早不到 1 ms 的也不再排（晚最多 1 ms 跑，處理的地方照舊檢查到期了沒）。不選固定的基準換算（`Clock` 記下 `TimeTicks` 的起點）：macOS 的 `Instant` 在睡眠時不走、`CLOCK_MONOTONIC` 照走，睡過之後換算就錯了。另外在 `docs/architecture.md` 加「Event loop：加新的工作時要跟著改的地方」（三類各怎麼加、`after_task` 的先後依賴、漏 post 的後果），`CLAUDE.md` 指過去：拿掉外圈之後，漏改的多半是那件事不發生，測試常常看不出來。

25. **做完之後：記號跑它自己的那一個 task，不只是 queue 的第一個；debug build 偵測丟掉的 task**（2026-11-24，使用者決定；E）：Speedometer 和 RFC 開始之前的 `79a0ecc` 比，這一版有 9 個 suite 不時卡住、160 秒沒有報告（3 輪共 18 次，`79a0ecc` 0 次；二分到第 3 段）。卡住時 renderer 的 main thread 閒著等 task，頁面的 `setInterval` 也不跑了：
   - 原因：`task_numbered` 只認每個 queue 的第一個。thread 忙過一個 timer 的到期時間之後再設一個已經到期的 timer，新的記號當場拿到編號（`post_at` 對已經到期的不經過 heap），排在 heap 裡較早到期、還沒搬出來的記號前面；它輪到時 DOM 的 timer queue 第一個是較早的那個，記號用掉，那個 timer 從此沒有記號，排在它後面的 timer 也一樣（probe 看到記號 `1189` 輪到時第一個是 `894`）。第 0 階段的原型沒有這個問題：記號跑到期的全部，到期的記號在 post 時就搬出來；第 3 段把它改成一個記號一個 task（第 13 條）、到期的記號在取 task 時才搬，兩者湊在一起才出現，當時的測試都沒有「忙過一個 timer 的時間、再設一個已經到期的」。
   - 對照 Chrome（原始碼和 Chrome for Testing 152 實測）：`DOMTimer` 經過 `TimerBase` post 一個延後的 task，那個 task 呼叫的就是它自己的 `Fired()`，沒有另一份 timer 的 queue；timeout 是 0 的用 `kJavascriptTimerImmediate`；延後的 task 在挑下一個之前搬進 work queue，那時才拿 enqueue order。所以同樣的情況 5 次都先跑 0 ms 的、再跑 10 ms 的（`b,a`）。HTML 只要求 timeout 不比自己長、比自己早設的 timer 先跑，兩種順序都合規範。
   - 做法：scheduler 輪到記號時，`dom` 的 `target_task` 讓那個編號的 task 是它的 queue 唯一能跑的，不管排第幾（timer、UA 的 timer、idle callback 的 timeout、element task、自己的訊息；port 的訊息照佇列的順序拿編號，只看第一個），取出時就清掉，它執行時再挑別的 task 照舊。先進先出照樣成立：同一個 queue 的記號是照排進來的順序 post 的，和 Chrome 一樣由 scheduler 保證。debug build 在 loop 要等之前檢查：DOM 有 5 ms 前就到期的 task、scheduler 裡卻什麼都沒有，就 panic（`assert_no_dom_task_lost`），之後同一類的 bug 不會再靜靜地卡住。
   - 不選 scheduler 的延後 task 一律經過 heap（A）：順序變成 `a,b`，和 Chrome 不同，也只修這一種對不上。不選記號找不到時跑 queue 的第一個當安全網（B）：不會丟 task，但順序是 DOM 的，和 Chrome 不同。
   - 同時找到、沒有修的：通道一直有 task 時（頁面一直排 task 給自己也算），要不要 main frame 只在通道空了時才決定，`requestAnimationFrame` 一直不來。改成每個 task 之後就告訴 compositor 時，shell 有 7 個測試的 headless 以為「input 之後的那一張 frame 已經有 input 引起的全部」，記進 `docs/limitations.md`，之後另外處理。

26. **做完之後：要 main frame 的當下告訴 compositor；headless 等 renderer 閒下來的 frame**（2026-11-24，使用者決定；A）：第 25 條最後一項記下的 `requestAnimationFrame` 在通道一直有 task 時不來（`event_loop::an_animation_frame_comes_while_responses_keep_coming` 改之前 3 次都壞），Speedometer 的 async 也可能因此變長（量的是到下一個 frame 畫完）。
   - 做法：每個 task 之後照實告訴 compositor 要不要 main frame。對照 Chrome：`SetNeedsAnimate` 在要求的當下，另有 rendering 的防餓死（優先度，留給「之後的可能」）。第 5 段改成只在沒有 task 時才決定，是因為那時 6 個 `shell` 的測試壞了，當時以為是「多要了一張 frame」，其實是 headless 的 harness 的假設（下一項）；mutation test 看出「不要了等閒下來才說」是多的（`needs` 是 false 時本來就沒有理由要 frame），拿掉了。
   - headless 的 harness 原本把最後一個 input 的 ack 指定的 frame 當成 input 處理完了：只在「通道空了才要 frame」時成立，改了之後那張 frame 排在 input 留下的 task（連結導覽之後的 `hashchange`）前面，`shell` 有 5 個測試讀到舊的標題。改成 input 都 ack、那張 frame 也畫了之後，再要一張 renderer 沒有 task、也不要 frame 時的 frame（新的 `RendererMsg::RequestIdleFrame`，只在 `web_tests`；回答 `IdleFrameReady` 帶著那張 frame 的 token，像 input 的 ack），畫出來才結束。不選只送一個「閒下來了」的訊息：它走 Browser 的通道、frame 走 Viz 的，兩者不保證先後，harness 可能在最後那張 frame 到之前結束。不改 `VisualStateReady`（加 token）：瀏覽器的測試裡有十幾處照它的欄位 match。content_shell 的 web test 也不是靠某一張 frame，是等頁面說完成，或擷取前要 renderer 再畫一次。
   - 算畫了幾張的測試照 task 怎麼切、begin frame 什麼時候到而變（每個 task 之後就要 frame，載入期間 parse、sheet、`load` 的 task 各可能畫一張；整組一起跑時 canvas 的那一個失敗過），改成照它們的本意：headless 的 `Frame` 多給第一張 frame 的像素和它畫出來的時間、load 之後畫了幾張（照 frame 的 token），「sheet、body 之前沒畫」看第一張就是完整的，alternate 的 sheet 不擋畫面看第一張在它到之前就畫了（它的延遲從 500 ms 拉長到 2 s），canvas 看 load 之後剛好兩張。
   - 同時發現、和 A 無關的：`devtools_dom::queued_tasks_changes_are_told_before_the_next_task` 整組跑時失敗過一次：第二個命令可能在第一個執行到 `postMessage` 之前就排進 scheduler（第 3 段起就是這樣），改成等第一個命令印出一行才送第二個，它守的 2 個 mutant 照樣抓到。
   - `target_task` 只在 engine 跑那個記號的期間設著（mutation test 看出 `dom` 取出 task 時再清一次是等價的：一個 task 執行中沒有地方另外挑 DOM 的 task），`dom` 的 `task_taken` 拿掉。

## 待決定

目前沒有。

## 之後的可能

- **DOM 的 task 只有一套順序，主 thread 和 worker 一樣**（一份 RFC）：主 thread 的記號跑它自己的那一個 task（第 25 條），順序只在 scheduler；但 `dom` 自己還有一套 `(時間, 編號)` 的順序（`next_task`、一輪的 `begin_task_pass`），給 worker 的 loop、測試和 debug build 的偵測用。兩套之間另外補的還在：port 的訊息照佇列的順序拿編號、port 啟用時重送記號（WPT 的 `webmessaging/` 才抓到），兩個時鐘差一點時照 scheduler 交出的時間跑（`dom_now().max(task.at)`，只靠審查），取消的 task 的記號跑到才知道沒事。主 thread 和 worker 也是兩種模型，worker 照舊一輪一輪跑。做法：`dom` 只照編號存 task 的內容（有 GC 的參照，不能進跨 thread 的 scheduler），順序只在 scheduler；worker 的 loop 改用同樣的 scheduler（DOM、自己的工作、V8 的 platform task 各是一個 task），拿掉 `dom` 的 `next_task`、一輪的機制和時鐘的換算，port 的訊息到能送的時候才拿編號。兩件要一起做：worker 還在用 `dom` 的順序時，那一套拿不掉。
- Chrome 的 scheduler 優先權（input、rendering）：通道更忙時需要；那時 DOM 和 renderer 自己的 task 照 task type 分 queue（第 15、17 條沒有分）。那時一起做 rendering 的防餓死（Chrome 的 compositor 的 queue 超過 100 ms 沒有 main frame 時升到很高的優先度）。
- 導覽帶著解析好的 blob（`BlobURLToken`）。
