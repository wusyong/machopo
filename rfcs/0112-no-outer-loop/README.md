# RFC 0112：拿掉 event loop 的外圈

- 開始日期：2026-11-24
- 狀態：🚧 已排程：2026-11-24（第 0 到 3 階段做完；第 4 段待審查）
- 細節：`details.md`；進度：`progress.md`

## 摘要

renderer 的 event loop 分兩層：內圈一個個跑 scheduler 的 task，跑完通道上排著的每一個之後，才輪到外圈（`RenderThread::run` 在那一批之後的二十來個步驟）：DOM 的 timer 和其他 task、parser 的續跑、子 frame 的 `srcdoc` 和 `about:` 的 commit、DevTools 的 frame 事件和 DOM 的變動、console 的警告、blob URL 的撤銷……。這些工作要等通道空了才做，通道一直有 task 時就一直晚；它們的先後由外圈的那張清單決定，不是由 task 的來源和時間。Blink 沒有外圈：一件事不是在發生的當下做（`probe::`、`LocalFrameClient`），就是 frame scheduler 上某個 task queue 的 task，或是每個 task 結束時的觀察者（`TaskObserver`）。這份 RFC 讓 machopo 照同樣的三類：導覽在呼叫的當下交給 Browser，DevTools 的事件在改變的當下送，DOM 的 task、parser 的續跑和 commit 是 scheduler 的 task，Chrome 也在 task 之間做的才留在 task 之間，最後拿掉外圈。第一階段只評估、不改產品：盤點外圈的每一步，對照 Chromium 分類，DOM 的 task 怎麼進 scheduler 做原型，結果交給使用者決定範圍和做法。

## 動機

**現況**（2026-11-24 照程式看的，`details.md` 的「盤點」）：

- `RenderThread::run`（`crates/renderer/src/render_thread.rs`）：等一個 task，然後在內圈一個個跑通道上已經排著的（`scheduler.try_next`），每個之後跑 DOM 在那個 task 裡排的 task（`run_queued_tasks`）、更新標題；通道空了才往下走外圈。
- 外圈依序做：caret 的閃爍、新內容的 rendering 逾時、intersection 的延遲、跨 origin 的 iframe 晚到的 `load`、`<select>` 的選單、frame 的要求（`handle_frame_requests`：導覽、blob URL）、子 frame 的 commit、DOM 的 timer 和其他到期的 task（`run_due_tasks`）、parser 的續跑（`parse_on`）、背景圖和 parser 的 script 的請求、等 sheet 的 script、canvas 的 `toBlob`、WebDriver 的結果、console 的警告、標題、blob URL 的撤銷、network idle、DevTools 的 frame 和 DOM 的變動、DevTools 的 `CSS` 要的 sheet、overlay、engine 的 platform task（GC）、idle callback、DOM 變了要不要 frame。
- DOM 的 task（timer、element task、`postMessage`）在 `dom` 的佇列，不在 scheduler 裡：在一個 scheduler 的 task 裡排的，那個 task 之後跑；timer 只在外圈跑。
- 導覽在每次呼叫 engine 之後的 `handle_frame_requests` 送給 Browser（RFC 0019 第 24、35 條：task 結束時，Chromium 是呼叫的當下）。

**問題**：

- **通道一直有 task 時外圈一直晚**：內圈不停，timer、parser、commit、DevTools 的事件都等著。RFC 0110 的 A6 就是這一類（一個 frame 算出 intersection 的 entry 之後，通道上排著的 task 讓外圈晚到下一個 frame 的 `requestAnimationFrame` 之後），它改成 DOM 的 task 才修掉；其他留在外圈的照樣會。
- **先後由清單決定**：DOM 的 timer 和 scheduler 的 task 之間、parser 的續跑和 timer 之間的順序，是外圈的步驟排在哪裡，不是 HTML 的 task source 和 Chrome 的 scheduler 的規則。加一個新的步驟就要在清單裡挑位置，排錯了是下一個同類的 bug。
- **DevTools 看到的順序和 Chrome 不同**：frame 的事件在那一輪最後才送，同一個 task 裡建了又拿掉的 iframe 看不到，新的 frame 的 `executionContextCreated` 在它的 `frameAttached` 之前。
- **導覽晚交給 Browser**：差 task 剩下的時間；同一個 task 裡的幾個導覽也都等那時才依序送（RFC 0019 第 24 條寫的「只留最後一個」已經不是現在的行為，見第 10 條），Chrome 在每個 setter 裡送、後一個取消前一個。

要解決的 `docs/limitations.md` 裡的限制：

- 「DevTools 的 `Page.frameAttached`、`frameDetached`、`frameNavigated` 在 renderer 的 loop 那一輪最後才送」。
- 「`location` 的導覽」那一條的「交給 browser 的時間是 task 結束時（Chromium 是呼叫的當下）」。

**目標**：

1. 盤點外圈（和內圈每個 task 之後）的每一步，對照 Chromium，分成三類：Blink 當場做的、Blink 排成 task 的、Chrome 也在 task 之間做的（`details.md` 的表）。
2. 定規則：Blink 當場做的當場做；Blink 排成 task 的是 scheduler 的 task，和其他 task 照 task source 和時間排；只有 Chrome 也在 task 之間做的留在 task 之間，寫明對應 Chrome 的哪一個。
3. 照規則改，拿掉外圈：`RenderThread::run` 只剩「取下一個 task、跑它、task 之間的觀察者、要不要 frame」。不加新的 `Rc`、`RefCell`（RFC 0110 的規則：兩邊都要的資料由 `dom` 或 engine 擁有，hook 拿參數）。
4. 範圍在第 0 階段的盤點之後由使用者決定（RFC 0110 的做法）：導覽、DevTools 的事件、DOM 的 task、parser 和 commit 是這份一定要做的，其他的照盤點決定。

**不在範圍內**：

- 導覽帶著解析好的 blob（`BlobURLToken`）：blob URL 的撤銷時機（`flush_revoked`）由它決定，另寫一份（RFC 0110 的「之後的可能」）。
- RFC 0110 第 7 條排除的擁有權的尾巴：DevTools agent 之間的 `Rc`、`BlobUrls`、worker thread 的 `Rc<RefCell>`、readiness、encoding、mime type 的副本。
- worker thread 自己的 loop（`worker_thread.rs`）：同樣的問題，等這份的規則定了再照做。
- Chrome 完整的 scheduler 優先權（input 的 high priority、rendering 的 starvation 防護……）：只做這份要的，現有的 `TaskQueues` 照舊。

## 設計概要

給之後改這部分程式的人：renderer 的工作只有三種位置，不再有「等通道空了」：

- 發生的當下：`location.href = x` 返回時 Browser 已經收到導覽；插入 iframe 返回時 DevTools 已經收到 `frameAttached`，在那個 frame 的 `executionContextCreated` 之前；`console` 的警告在產生時送。
- scheduler 的 task：`setTimeout` 的 callback、parser 讓出之後的續跑、`srcdoc` 的 commit 各是一個 task，在它的 task queue 裡照時間和順序輪到，通道上一直有 input 時也一樣（照 scheduler 的規則，不是排在全部之後）。
- task 之間：只有 Chrome 也在 task 之間做的（例如 network idle 的檢查、要不要 frame），每一個寫明對應 Chrome 的哪一個觀察者。

## 詳細設計

草稿：第 0 階段的盤點之後才寫得定。現在看得到的三部分：

**導覽在呼叫的當下送**：`dom_api` 的導覽現在記成 `FrameRequest::Navigation`，renderer 在 `handle_frame_requests` 送 `BeginNavigation`；改成 engine 在呼叫裡經過 host 的 hook 送（`FrameClient` 的新方法，Blink 的 `LocalFrameClient::BeginNavigation`）。要回答的：送導覽要 renderer 的帳（`begin_navigation_ids`、`ChildFrames.navigations`、DevTools 的 initiator、子 frame 的 session history），hook 拿不到 `RenderThread`，照 RFC 0110 的做法是帳搬進 `dom` 或由 engine 當參數帶，還是 hook 當場送、帳在呼叫之後對照；推翻 RFC 0019 第 24、35 條（同一個 task 只留最後一個、`back()` 依序送）時 Browser 怎麼取消前一個。

**DevTools 的事件在改變的當下送**：frame 的 `frameAttached`、`frameDetached`、`frameNavigated`，DOM agent 的變動，console 的警告，經過 hook 在 engine 的呼叫裡送（Blink 的 `probe::`）。RFC 0110 第 6 條留下的：iframe 建立時取 script 的 stack 和建 frame 的順序要能對調（`dom_js/src/state.rs` 的 `note_request_stacks` 在 `update_elements` 之後），`childNodeInserted` 要在 `frameAttached` 之前送出（DOM 的變動要先送）；hook 可以拿 DevTools agent 的 `Rc`（0110 第 6 條）。沒開 DevTools 時不能多做事（先問 `enabled()`）。

**DOM 的 task 是 scheduler 的 task**：DOM 的 task 的內容（callback、GC 的參照）照 RFC 0110 留在 `dom`，scheduler 只要知道「有一個、在哪個 queue、什麼時候到期、排第幾」。三種做法（`details.md` 的「DOM 的 task 怎麼進 scheduler」）：

- **T1：排的當下告訴 scheduler**：`dom` 排 task 時經過 host 的 hook 送一個記號給 scheduler（像 load sink 的 `post_request_failed` 那樣經過它拿著的 `Arc<MainThreadScheduler>`：IO thread 也往它排 task，本來就是跨 thread 共享的，不加新的 `Rc`、`RefCell`），scheduler 輪到它時叫 engine 跑那一個。和 Blink 一樣（`ExecutionContext::GetTaskRunner(type)->PostTask`）。要回答的：記號和 DOM 的佇列各一份，取消（文件不在了）時要兩邊一致；timer 的到期時間交給 scheduler 的 delayed queue。
- **T2：scheduler 每次取 task 時問 DOM**：DOM 的佇列照舊，scheduler 取下一個時同時看 DOM 最早的那一個（時間和順序），挑先的。不用 hook、不用兩份；每次取 task 多一次查詢。
- **T3：照舊，只把 `run_due_tasks` 搬進內圈**：每個 scheduler 的 task 之後也跑到期的 timer。最小，但順序還是清單決定的。

parser 的續跑和子 frame 的 commit 是 renderer 自己的工作，直接排成 scheduler 的 task（`MainThreadTask` 多兩種），不牽涉 `dom`。

**task 之間留下的**：照盤點，每一個寫明對應 Chrome 的哪一個（`TaskObserver::DidProcessTask`、idle period、`BeginMainFrame` 之前）。

**和 Chrome 不同、刻意保留的**：要在盤點時一項項定。

### 測試

- 必壞的測試（改之前在原本的程式上每次都失敗）：通道上一直有 task 時 timer 照樣按時間跑、parser 照樣續跑；`location.href = x` 之後同一個 task 再忙一陣，Browser 在忙完之前就收到導覽；DevTools 的 `childNodeInserted`、`frameAttached`、`executionContextCreated` 照 Chrome 的順序，同一個 task 裡建了又拿掉的 iframe 也收到 `frameAttached` 和 `frameDetached`。
- 每一步跑 mutation test；相關的 WPT 目錄（`html/webappapis/timers/`、`html/browsers/`、`html/syntax/parsing/`、導覽和 iframe 的）。
- 計數的測試：沒開 DevTools 時，當場送的那幾個 hook 不建事件。

### 分階段

0. **評估，不改產品**：
   - 盤點外圈和內圈每個 task 之後的每一步，各自對照 Chromium 的原始碼（現在的表是憑記憶），分成目標第 1 條的三類，寫成 `details.md` 的表。
   - DOM 的 task 進 scheduler 的 T1、T2 各做一個原型（patch 放在 `prototypes/`，不合併），用「通道一直有 task 時的 timer」的必壞測試比。
   - 交給使用者決定範圍和做法。
1. **DevTools 的事件**：`frameAttached`、`frameDetached`、`frameNavigated` 當場送（先送出排著的 DOM 變動）；DOM 的變動和 console 從整批 task 之後改成每個 task 之後送（Chrome 的 `DidProcessTask`）。
2. **導覽在呼叫的當下送**：推翻 RFC 0019 第 24、35 條；「待決定」第 1、2 條在這一段開始前決定。
3. **DOM 的 task 是 scheduler 的 task**（T1，第 4 條）：一個記號跑一個 DOM 的 task；UA 自己的 timer 和 idle callback 的 timeout 也送記號；`until_next_due` 不再問 DOM 的 timer。
4. **renderer 自己的工作排成 task**：parser 的續跑、`srcdoc` 和 `about:` 的 commit、等 sheet 的 script（kNetworking）；caret、intersection 的 `delay`、延後的 iframe `load`、新內容的逾時（renderer 的 timer）；engine 的 platform task。
5. **剩下的步驟，拿掉外圈**：`<select>` 的選單、WebDriver 的結果、DevTools 的 `CSS` 重抓改成當場；network idle、overlay、idle callback、`toBlob`、request stack 的清理改成每個 task 之後（Chrome 的 `TaskObserver`、idle period）；`RenderThread::run` 只剩取 task、跑它、task 之後的觀察者、要不要 frame。

每一段各自測試、mutation test、審核，做完可以先 merge 回 `main`（`rfcs/README.md` 的「階段的 merge」）。

## 缺點

- event loop 的改動：timer、parser、commit 的時序都會變，導覽和 parser 的 WPT 最可能變動，整套只有 CI 跑。
- 當場送的 hook 變多：每一個都要證明被呼叫時不會回頭執行 script（RFC 0110 的 hook 的規則），DevTools 開著時每次改變都多一次工作。
- 導覽當場送要推翻 RFC 0019 的兩條，Browser 那一側要處理同一個 task 裡連續的導覽。

## 理由和其他做法

- **只在每個 scheduler 的 task 之後也跑外圈**：通道一直有 task 時不再晚，但順序還是清單決定的，DevTools 和導覽照樣在 task 結束時，和 Chrome 不同。可以當成過渡，不解決「加一步就要挑位置」。
- **一項項修**（像 RFC 0110 的 A6 那樣）：每次修一個症狀，外圈照樣在，下一個同類的 bug 照樣長出來。
- **不做**：目前沒有測試因為外圈而不穩定（RFC 0109 的盤點，A 類都由 RFC 0110 修了），網頁看得到的只有時序和 Chrome 不同；風險是之後通道更忙（site isolation、更多 process 的訊息）時外圈的工作更晚。

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
   - 實作時定下、發現的（2026-11-24，使用者同意這幾項的做法）：記號帶 DOM 的 task 編號（每種 task 都從同一個計數器拿，interval 重新排時拿新的）；engine 找出最前面是那個編號的 queue（`Cx::task_numbered`），照 scheduler 交出的時間跑它（DOM 的時鐘換算回來差一點也照樣跑），不在了就什麼都不做。
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

## 待決定

目前沒有。

## 之後的可能

- worker thread 的 loop 照同樣的規則。
- Chrome 的 scheduler 優先權（input、rendering）：通道更忙時需要。
