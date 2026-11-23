# RFC 0112：拿掉 event loop 的外圈

- 開始日期：2026-11-24
- 狀態：📝 草稿
- 細節：`details.md`

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
- **導覽晚交給 Browser**：差 task 剩下的時間；同一個 task 裡的幾個導覽 machopo 只留最後一個（第 24 條），Chrome 是每個都送、後一個取消前一個。

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
1. 之後的階段照決定的順序，建議由小到大：DevTools 的事件、導覽、DOM 的 task 和 parser、commit，最後是其他的步驟和拿掉外圈。每一步各自測試、mutation test、審核。

## 缺點

- event loop 的改動：timer、parser、commit 的時序都會變，導覽和 parser 的 WPT 最可能變動，整套只有 CI 跑。
- 當場送的 hook 變多：每一個都要證明被呼叫時不會回頭執行 script（RFC 0110 的 hook 的規則），DevTools 開著時每次改變都多一次工作。
- 導覽當場送要推翻 RFC 0019 的兩條，Browser 那一側要處理同一個 task 裡連續的導覽。

## 理由和其他做法

- **只在每個 scheduler 的 task 之後也跑外圈**：通道一直有 task 時不再晚，但順序還是清單決定的，DevTools 和導覽照樣在 task 結束時，和 Chrome 不同。可以當成過渡，不解決「加一步就要挑位置」。
- **一項項修**（像 RFC 0110 的 A6 那樣）：每次修一個症狀，外圈照樣在，下一個同類的 bug 照樣長出來。
- **不做**：目前沒有測試因為外圈而不穩定（RFC 0109 的盤點，A 類都由 RFC 0110 修了），網頁看得到的只有時序和 Chrome 不同；風險是之後通道更忙（site isolation、更多 process 的訊息）時外圈的工作更晚。

## 先例

- **Chromium**（憑記憶，第 0 階段對原始碼）：renderer main thread 的 `MainThreadSchedulerImpl` 和 `SequenceManager`，每個 frame 的 `FrameScheduler` 照 task type 分 task queue（timer、networking、DOM manipulation、parser……），Blink 排 task 時經過 `ExecutionContext::GetTaskRunner(TaskType)`；DevTools 經過 `probe::` 當場送；導覽經過 `LocalFrameClient::BeginNavigation` 當場送 `BeginNavigation`；task 之間的工作是 `TaskObserver`（microtask checkpoint、`IdlenessDetector`）和 rendering 的步驟。沒有「通道空了才做」的外圈。
- **HTML**：event loop 每一輪取一個 task、microtask checkpoint、必要時 update the rendering；timer、parser、導覽都是 task source 的 task。
- 其他瀏覽器沒有對照。

## 已決定的事項

無。

## 待決定

1. **範圍**：盤點之後決定外圈的哪些步驟這份要改，哪些記成和 Chrome 不同、留著。
2. **DOM 的 task 怎麼進 scheduler**：T1、T2 或 T3，第 0 階段的原型比較之後決定。2026-11-24 的看法：T1（和 Blink 一樣，順序由 scheduler 決定），前提是取消時兩邊對得上。
3. **導覽的帳在哪裡**：搬進 `dom`、由 engine 當參數帶，或 hook 當場送、renderer 在呼叫之後對照（RFC 0110 第 22 條的做法）。
4. **同一個 task 裡連續的導覽**：照 Chrome 每個都送、Browser 取消前一個，還是保留第 24 條的「只留最後一個」。

## 之後的可能

- worker thread 的 loop 照同樣的規則。
- Chrome 的 scheduler 優先權（input、rendering）：通道更忙時需要。
