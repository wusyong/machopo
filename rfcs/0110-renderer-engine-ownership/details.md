# RFC 0110 的細節：renderer 和 engine 的狀態只有一個擁有者

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-23，`87fc238`，照程式看的。「Blink」一欄標「對過」的是對過 152.0.7977.82 的原始碼（檔案在「參考資料」），「沒對」的是照對 Blink 的了解寫的，要用的時候再對。

### 為什麼會有副本

`RenderThread` 擁有 engine（`Option<Box<dyn ScriptEngine>>`），engine 擁有 renderer 交給它的 `Frame`（`set_layout`）和 `RendererLoadSink`（`set_load_sink`）。呼叫 engine 時借出的是 `&mut` engine，`RenderThread` 的其他欄位在這段期間借不到：engine 裡的 DOM API（經過 `FrameLayout`、`LoadSink` 的 hook）碰得到的只有 engine 擁有的那兩個物件。其他 renderer 的狀態只能在呼叫結束之後由 renderer 自己改（便條、DOM 上的佇列、外圈的收尾），或用 `Rc`、`RefCell` 和 hook 共享。

分類（「目標」第 1 條）：**留**：跨 process，Chromium 也是訊息；**當下**：該同步卻延後了；**task**：該是 task 卻在外圈；**一份**：同一個 thread 的副本，拿掉；**借用**：只為了借用而共享；**範圍外**：event loop 的改動（「之後的可能」）。

### 便條：`FrameRequest`

`crates/dom_api/src/frame.rs`。renderer 在 `handle_frame_requests` 處理，`render_thread.rs` 4 處（包括外圈的那一次）、`render_thread/documents.rs` 15 處，共 19 處。

| 種類 | 現在 | Blink | 分類 | 看得到的 |
| --- | --- | --- | --- | --- |
| `Created` | engine 當下建 frame 和 window（`create_child`，同步呼叫 `FrameLayout::child_frame_created`），renderer 的 `ChildFrames`、DevTools 等便條 | iframe 插入時當下建 `LocalFrame`，`probe::FrameAttachedToParent` 當下告訴 Page agent（`InspectorPageAgent::FrameAttachedToParent` 當下送 `frameAttached`，對過） | 當下、一份 | DevTools 的 `frameAttached` 晚到 |
| `Removed` | renderer 等便條才拿掉 frame | `HTMLFrameOwnerElement::DisconnectContentFrame` 當下 `Detach`（對過） | 當下 | A5 |
| `Navigate`（iframe 的 `src`） | 便條；到 fragment 的在 renderer 處理，其他的送 `BeginNavigation` | `FrameLoader::StartNavigation` 當下開始，跨文件的送 Browser（沒對） | 跨文件的：留；到 fragment 的：當下 | |
| `Navigation`：`To`、`Reload`、`GoToOffset`、`NewTab` | 便條，renderer 在外圈或收尾點送 Browser | 呼叫的當下送（沒對） | 留（RFC 0019 第 24、35 條） | 送出比 Chrome 晚那個 task 剩下的時間 |
| `Navigation`：`To` 一個 `javascript:` 網址 | 便條，renderer 在處理時執行 | 記在 **Document** 上（`pending_javascript_urls_`），`PostCancellableTask(kNetworking)` 排成 task，文件 `Shutdown`、`open()`、`CancelParsing` 時取消（`Document::ProcessJavaScriptUrl`，`document.cc`，審查對過；我原本寫「掛在 window 上」是錯的） | task，綁在文件上；整個在 `dom_api` 裡做得完 | 子 frame 導覽中時 machopo 在舊文件執行、Chrome 不執行（0109 的實驗） |
| `Navigation`：`SameDocument` | engine 當下改網址；renderer 等便條才捲動、改自己記的網址、告訴 Browser | 同步 commit（`FrameLoader::StartNavigation`，對過）、當下捲動 | 捲動：當下；告訴 Browser：留 | A4；同一個 task 讀 `scrollY` 是舊的 |
| `RegisterBlobUrl`、`RevokeBlobUrl` | 便條；`BlobUrls` 和 `LoadSink` 共享（`Rc<RefCell>`） | 交給 Browser 的 `BlobURLStore`（沒對） | 留；共享的表：借用 | |
| `Fetch` | `dom_js` 沒有 load sink 時才有（`take_frame_requests`），只有 `dom_js` 的測試走得到：renderer 一定會給 sink。`dom_wasm` 沒有 `fetch()`（`fetch_response` 回 `false`）；我原本寫成「wasm 文件的 `fetch()`」是錯的 | 呼叫的當下送（沒對） | 不是問題 | |
| `FetchSent` | `LoadSink` 當下送出，Network agent 等便條才登記、取時間 | 送出的當下告訴 DevTools（`resource_load_observer_for_frame.cc`，對過） | 當下 | A2、A3 |
| `CancelFetch` | 便條，renderer 處理時才取消請求 | `abort()` 的當下取消（沒對） | 當下 | 中止的請求多跑一陣（照程式推的） |

### DOM 上的佇列，renderer 在外圈取

| 佇列 | 現在 | Blink | 分類 | 看得到的 |
| --- | --- | --- | --- | --- |
| `Dom::take_image_fetches`（`start_image_fetches`） | 外圈才開始載入 | microtask 裡開始（`ImageLoader::EnqueueImageLoadingMicroTask`，對過） | 當下（microtask） | 只有時間和 DevTools 的順序 |
| `Dom::take_script_fetches`（`start_script_fetches`） | 外圈才 fetch | `PrepareScript` 的當下 fetch（`ClassicPendingScript::Fetch`，對過） | 當下 | 同上 |
| `Dom::take_console_warnings`（`log_console_warnings`） | 外圈才交給 Log agent | 當下交給 `FrameConsole::AddMessage`（`LocalDOMWindow::AddConsoleMessageImpl`，對過；之後的路沒追） | 當下 | DevTools 看到的順序 |
| `Cx::take_popup_requests`（`handle_popup_requests`） | 外圈才送 Browser | `ExternalPopupMenu` 送 Browser（沒對） | 留 | |
| DOM agent 的 `take_changes`（`tell_dom_changes`） | 外圈才送 | `InspectorDOMAgent::DidInsertDOMNode` 當下送 `childNodeInserted`（對過） | 當下 | DevTools 看到的順序；A1 看到的 `childNodeCountUpdated` 是它 |
| `Cx::take_canvas_draws`（`frame_view.rs`） | rendering 的時候取 | | 不是這一類 | |
| `Dom::take_loaded_documents`（`run_due_tasks` 之後） | 外圈才取：接著發 iframe 的 `load`、查 parent 載完了沒、告訴 DevTools 的 lifecycle | `CheckCompletedInternal` 同步發 `load`（審查對過的片段） | 當下，但會執行 script：不能在 engine 的呼叫中做 | iframe 的 `load` 和 parent 載完的時機（審查補的） |

### renderer 自己的佇列和外圈的收尾

| 工作 | 現在 | Blink | 分類 | 看得到的 |
| --- | --- | --- | --- | --- |
| `tell_frames`（Page、DOM agent 的 frame 清單） | renderer 的副本，外圈才更新 | 改變的當下由 probe 告訴；CSS agent 列 sheet 時直接讀 document（對過） | 一份 | A1 |
| `run_child_commits`（`srcdoc`、`about:` 的 commit） | 「各自一個 task」，但在外圈，不在 scheduler | 經過 Browser 的導覽（沒對） | task | 和其他 task 的順序 |
| `run_intersection_task` | 外圈 | `PostTaskToDeliverNotifications` post 成 task（對過） | task | A6（推論） |
| `fire_due_owner_loads`（跨 origin 的同一份文件內導覽，iframe 晚 100 ms 的 `load`） | 外圈看時間到了沒 | `cross_origin_load_event_delay` 的 timer（沒對） | task（timer） | |
| `parse_on`、`start_*_fetches` 之後的 parser、`retry_scripts_blocked_by_sheets` | 外圈 | parser 的續跑是 task | 範圍外 | |
| `update_title` | 每個 task 之後比對版本、變了才送 Browser | `DispatchDidReceiveTitle` 送 Browser（沒對） | 留 | |
| `check_network_idle`、`send_webdriver_results`、`blob_urls.flush_revoked` | 外圈 | | 留（跨 process 或計時） | |

### 重複的狀態

| 狀態 | 幾份 | 分類 |
| --- | --- | --- |
| frame 的樹 | 四份加 DevTools 的：`dom`（`content_document`、`frame_owner`）、`dom_js` 的 `Frames`（id、token、document、parent、owner）、renderer 的 `ChildFrames`（parent、owner、token、文件）、`FrameView` 的 child view（`show_child` 同步、`remove_child` 等便條；審查補的），DevTools 的 frame 清單 | 一份 |
| 文件的網址 | 兩份：DOM 的、renderer 的 `FrameDocument.url`（同一份文件內的導覽要等便條才改；讀它的有 `run_javascript_url`、`initiator`、子 frame 導覽的 timing） | 一份 |
| 文件的 readiness、encoding、mime type | DOM 的和 `FrameDocument` 的（審查補的） | 一份，或寫明誰為什麼留 |
| session history 的 index、length | Browser 的和 renderer 的 `own_history_entries` | 留 |

### 為了借用而共享的

| 東西 | 和誰共享 | 分類 |
| --- | --- | --- |
| DevTools 的六個 agent（`Rc`） | `DevToolsQueueHandle`：inspector 在 `RenderThread` 借不到時也要用 | 借用 |
| `RequestIds`、`DocumentLoaders`、`BlobUrls`（`Rc<RefCell>`） | `RendererLoadSink` | 借用 |
| `Workers`（`Rc`） | 交給 engine 的 `WorkerHost` | 借用 |
| agent 之間的 `Rc`：page→network、css→dom 和 page、network→log | agent 互相呼叫（審查補的） | 不只是為了 `DevToolsQueueHandle` |
| worker thread 的 `GlobalHost`：`fetches`、`module_fetches`（`Rc<RefCell>`）、`closing`、`inbox`（`Rc`） | worker 的 loop（`worker_thread.rs`；審查補的） | 同一種寫法，借用 |
| `user_agent`、文件的原始碼（`Rc<str>`）、`FrameResources`（`Rc`） | 不可變的值，複製便宜 | 不是這一類 |

hook 的 trait 都是 `&self`（`LoadSink`、`CookieHost`、`WorkerHost`）：一開始就是為內部可變的共享設計的，拿掉共享就要改成 `&mut`；`WorkerHost` 在 worker thread 上也有實作，`worker_host` 刻意放在 `State` 的 `Caches` 裡分開借（審查補的）。

### `dom_wasm`

沒有 `fetch()` 也沒有 load sink（`set_load_sink` 是空的）；導覽只留便條（`navigate`）。

### 小結

- **當下**：`Created`、`Removed`、到 fragment 的 `Navigate`、`SameDocument` 的捲動、`FetchSent`、`CancelFetch`，圖片和 script 的載入，console 的訊息，DOM agent 的改變；`take_loaded_documents` 之後的也是，但會執行 script。
- **一份**：frame 的樹（四份加 DevTools 的）、文件的網址、readiness、encoding、mime type。
- **task**：`javascript:` 網址（綁在文件上）、`srcdoc` 和 `about:` 的 commit、intersection 的通知、iframe 晚到的 `load`。
- **借用**：DevTools 的 agent（也互相拿著）、loader 的三張表、`Workers`、worker thread 的 `GlobalHost`。
- **留**：跨文件的導覽、blob URL、下拉選單、標題、session history 的 index 和 length。
- 0109 壞過的 A1 到 A6 都在「當下」「一份」「task」裡。

## 原型

2026-11-23，`fetch()` 的登記（A2、A3），從 `87fc238`；patch 和怎麼套在 `prototypes/`，測試的結果在 `progress.md`。四個都讓 A2、A3 的必壞測試變成必過，相關的整組測試照樣通過。審查之後加了一個測試（`tests-unload.diff`：JS 的文件在 `pagehide`、`unload` 裡呼叫 `fetch()`，同時被 wasm 的文件換掉、engine 跟著換），V2 在它上面 panic。

### 三個都要的部分（V0）

- **engine 送出時要交給 sink 的**：DevTools 登記一個請求要它的 frame 的 token、文件的網址、loader id 和 script 的 stack。前三個原本在 renderer 的 `request_frame` 從 `FrameDocument`、`ChildFrames` 的 token 和 DevTools 的 main frame token 拼出來，stack 在 DOM 裡。engine 送出的當下 renderer 的這些都借不到，但 engine 自己都有：token 在 `dom_js` 的 `Frames`（main token 和 renderer 給 DevTools 的是同一個值），網址和 stack 在 DOM。所以 `start_fetch` 多帶一個 `FetchOrigin`。loader id 改成和文件的 loader 記在一起（`DocumentLoaders`，renderer 在設定 loader 的三個地方寫入）。
- **`FetchSent` 只剩沒送出的那一種**；`did_receive_resource` 一開始的 `handle_frame_requests`（註解寫著「this may be one's response」）對 `fetch()` 不再需要。
- **介面**：`LoadSink::start_fetch` 改成 `&mut self`；`ScriptEngine::load_sink_mut`、`LoadSink::as_any_mut`，讓 renderer 拿回自己的 sink（和 `FrameLayout` 的 downcast 一樣）。`dom_wasm` 不用改（沒有 sink，用預設的 `None`）。
- V0 本身就是「逐個改成同步、不動擁有權」：多了一張 `Rc<RefCell>` 的表（renderer 裡從 6 個變成 7 個）。

### V1、V2 和 V3 的比較

| | V0：共享 | V1：sink 擁有（做法一） | V2：renderer 擁有、借給 sink（做法二） | V3：DOM 擁有（做法三） |
| --- | --- | --- | --- | --- |
| renderer 裡的 `Rc<RefCell<`（字面上數的） | 7 | 4 | 4 | 4 |
| 其他的執行期借用 | | | | `dom` 的 `sent_fetches`（`RefCell`，`Dom` 的 API 是 `&self`）、每個 thread 一個的計數器（`thread_local` 的 `RefCell`）；四個的 sink 都拿一份 `Rc<NetworkAgent>`（V0 加的）。第二次審查指出：只數 `Rc<RefCell<` 低估了共享；`sent_fetches` 的 `RefCell` 照規則要在註解寫理由，原型沒寫 |
| 在 V0 之上多改的 | | 4 個檔，+119／−72 | 5 個檔，+290／−166，多一個 `engine_slot.rs`（115 行）；大部分是把 `&mut self.engine` 換成 `self.engine.as_mut()` | 7 個檔，+163／−112（`dom` +67） |
| `unsafe` | 沒有 | 沒有 | 沒有 | 沒有 |
| 狀態放在哪 | 兩邊各一個 handle | 有 sink 的 engine 在時是 sink 的，否則是 render thread 的（還沒有 engine、wasm 的 engine、換 engine 的期間） | 永遠是 render thread 的，只在 engine 被借走時借進 sink | `fetch()` 送出的請求是 DOM 的（dom_js 在 sink 送出後記下）；請求的編號是每個 renderer thread 一個計數器（`thread_local`，Blink 的 `CreateUniqueIdentifier`）；renderer 自己送的請求的網址是 renderer 的 |
| 要特別處理的 | | 建 engine 時：先交一個空的 sink，確認 engine 收下了才把狀態移進去（wasm 的 engine 直接丟掉 sink，原本的寫法會把計數器和 `fetches` 一起丟掉）；換 engine 時：丟掉之前取回來（和 `Frame` 一樣） | 丟掉 engine 時取回來（`take_to_drop`）；只讀寫 DOM 和 layout 的地方用不借的 `unlent_mut` | 沒有：換 engine 時舊 DOM 的記錄跟著走，之後才到的回應不是誰的（和原本清掉 `fetches` 一樣） |
| 找狀態的成本 | 一次 `RefCell` | 每次經過 engine：`load_sink_mut` 和 downcast，查兩次（從一個分支回傳借用，另一個分支也算借著，NLL 的限制） | 每次取 engine 借一次、每次取狀態拿回一次，各一次 `load_sink_mut` 和 downcast | 回應來時經過 engine 查 DOM 的一張表 |
| 借用的摩擦 | | 2 個 `&self` 的函式要改成 `&mut self`；2 處要在借 engine 或文件之前先讀狀態 | 同 V1，原因一樣（狀態可能還在 sink 裡，拿回來要 `&mut`） | 同樣 2 個、2 處：`ScriptEngine` 只有 `dom_mut`；`RequestUrls` 不再包 `RefCell` |
| 錯了會怎樣 | | engine 被 parser 拿走時找不到狀態（`expect`，測試裡沒有碰到）；**換 engine 時，unload 裡的 `fetch()` 的請求編號從 1 重來**：狀態在 unload 之前取走，sink 留下一個新的（審查找到，讀程式確認，測試看不出來） | 同 V1 的 parser；**換 engine 時，unload 裡的 `fetch()` 在 sink 的 `expect` panic**：`take_to_drop` 在 `unload_document` 之前拿回狀態（審查找到，重現了）；`unlent_mut` 用錯地方也一樣 | 沒有借出、拿回，沒有這一類 |
| 推到其他共享的表 | | `DocumentLoaders` 也被 `RendererCookieHost` 和 `Workers` 拿著：要搬進 sink 就要把這幾個 host 併成一個 | 借一份更大的狀態給 engine，但 hook 分散在 `State` 的不同欄位，一份狀態放不進幾個地方：要嘛把 hook 併成一個，要嘛各借一份（審查） | 兩邊都要的放進 `dom`（例如文件的 loader 和 DevTools 的 loader id），hook 要的由 engine 當參數帶過去 |

做法二的第一版用 guard（取 engine 時借出、guard 的 `Drop` 拿回），編譯出 202 個錯誤，134 個是「借了兩次」：有 `Drop` 的 guard 的借用到 scope 結束才算完，原本的 `&mut Box` 在最後一次用到時就結束了（NLL），每一處都要手動 `drop`。改成「取 engine 時借出、取狀態時拿回」之後，借用照 NLL 結束，剩下的錯誤和 V1 一樣。

**和草稿的差別**：草稿說做法二要在 `bindings` 加有期限的借用（`unsafe`）。原型的借法是把狀態**移進** engine 擁有的 sink，不是借參照給 V8 的 callback，所以不需要 `unsafe`；代價是借出和拿回在執行期（`Option`），不是靜態的期限。借用檢查器只保證 slot 不會同時交出 engine 和狀態，不保證每條會執行 script 的路都借出了（我原本寫「借用檢查器照樣保證」說得太滿）。

**原型沒有碰的**：DevTools 的 agent 還是 `Rc`（inspector 在 engine 執行中也要用，`DevToolsQueueHandle`；agent 之間也互相拿著），`DocumentLoaders`、`BlobUrls` 還是 `Rc<RefCell>`；效能沒有量。

V0 原本就有、四個都沒改的（審查推的）：同一個 task 裡建 iframe 又呼叫 `contentWindow.fetch()`，那份 `about:blank` 的 loader 要等 `Created` 處理時才記下，請求經過 loader 0，不是 parent 的。

## 審查

2026-11-23，一個獨立的 subagent 唯讀地審了第 0 階段（讀程式和三個 patch，抽查 Blink 152.0.7977.82 的原始碼），總評「部分正確」。我對過它的主要說法：V2 的 panic（重現了）、V1 的編號重來、`dom_wasm` 沒有 `fetch()`、`FrameView` 的 child view、`take_loaded_documents`、Blink 的 `javascript:` 網址在 Document 上排 task，都對。

- **根本原因**：「借不到 `RenderThread`」是原因之一，不是全部；延後的三種原因寫在 `README.md` 的「動機」。
- **盤點**：上面標「審查補的」的幾項是它找到的，`Fetch` 和 `javascript:` 兩列的更正也是。標「對過」的 Blink 行為它抽查了八項，都符合。
- **原型**：V2 會 panic、V1 編號重來（上表）；V0 和 dom_js 的 `send_fetches` 沒有找到會回歸的行為改變。看得到的改變是改善：`requests_in_flight` 在 `fetch()` 的當下就算進去，network idle 不會提早；DevTools 的 `documentURL` 從 DOM 讀，比 `FrameDocument.url` 新。
- **方向**：提了做法三（`README.md` 的「詳細設計」）；指出 Chromium 的分層支持做法三（`README.md` 的「先例」）；乙的範圍漏了文件網址的那份副本；G2 要把 DevTools 的事件順序一起定（Blink 讓 `frameAttached` 比 network 的事件先到，machopo 的 `executionContextCreated` 在 `create_child` 裡就送了）；Page agent 要的每個 frame 的資料（source、resources、mime）在 renderer，DevTools 暫停時經過 `answer_here`，要另外決定。
- **它沒能驗證的**：沒有執行任何東西；Blink 的 detach 到 parent 的 `CheckCompleted` 那一段只照註解和片段推。

### 第二次審查

2026-11-23，另一個沒參與過的 subagent 唯讀地審了做法三：V3 的正確性、做法三用在 frame 的樹上、範圍「乙」。我對過的：dom_js 的 `update_elements` 在 `note_request_stacks` 之前（`state.rs`）；頂層文件的 commit 不呼叫 `cancel_fetches_of_gone_documents`；iframe 移除時 `discard_document_tree` 只設 discarded、不發 unload；子 frame 初始 `about:blank` 的 loader 在 `Created` 的便條裡設；`self.children` 是 52 處。

- **V3**：沒有找到它新造成的錯誤。請求的編號不會撞號（Browser 照 `LoadKey::Resource { worker, request_id }` 分；worker 有自己的編號；每個 renderer 一條 OS thread；DevTools 的 requestId 帶每個 renderer 的 prefix）；換 engine 之後才到的回應不會被拿錯；送出和記下之間不會有回應插進來；原本依賴 renderer 的 `fetches` 的地方都改到了。要修的：
  - 同一筆記錄有三個地方寫：dom_js 之外，renderer 在沒有 sink 的路（`FrameRequest::Fetch`、`FetchSent` 帶 `Some`）也寫，但 renderer 一定給 sink，這條路只有 dom_js 的測試走得到。實作時拿掉，或只留給測試。
  - 取消還是便條（`CancelFetch`）：記錄已經在 DOM，sink 加一個取消的 hook 就能當場做。
  - 換 engine 之後才失敗的請求，log 印空的網址（原本印舊頁的網址）；兩種都把舊頁的失敗記到新頁，丟掉不記比較好。
  - 原本就有的：頂層文件換掉時不取消舊文件的 `fetch()`（`docs/limitations.md` 有一條相近的），回應不來的請求留在記錄裡；`cancel_fetches_of_gone_documents` 是 O(n²)，順序照 `HashMap`。
- **frame 的樹**：做得到，成本比原本寫的大（`README.md` 的「不管哪一種都要處理的」）。`dom` 已經有樹（`content_document`、`frame_owner`、`discard_document_tree`），建立和移除本來就在 engine 呼叫的尾端當場做（`update_elements`），慢的只是 renderer 那一側。`dom_wasm` 沒有自己的 frame 樹（只有 `TOP`，`create_window` 時清掉便條）。不牽動 process 的邊界，送給 Browser 的 `DidCreateChildFrame` 會排到同一個 task 裡的 `FetchResource` 之前（改善）。machopo 移除 iframe 時不發 unload（Blink 在 `LocalFrame::DetachImpl` 先 unload，`local_frame.cc`，審查對過）；要照做的話是 engine 在 `update_elements` 的移除步驟裡發，不是 renderer 的 hook。
- **範圍「乙」**：A1 到 A6 都對得到乙的一組，沒有找到做完還不穩定的直接理由，前提是：G2 一起處理 DevTools 的順序（DOM 變動先送、stack 的順序對調）；`Created` 的補救換成 assert；WebDriver 改讀 `dom`；寫明移除時哪兩件仍然延後；文件的 loader（G1）在 G2 之前；hook 拿 agent 的 `Rc` 要明文決定。
- **它沒能驗證的**：沒有執行任何東西；Blink 的 detach 之後 parent 的 `CheckCompleted`；`devtools.rs` 註解說 ResizeObserver 的 callback 執行時 layout 被借走了，但它在 `run_resize_observer_loop` 的路上沒看到借出（影響同步的 hook 在 callback 裡會不會默默不做）；stack 的順序能不能對調。


## 0109 找到的

在 `0109` branch 上（`cb5930d`）：

- `details.md` 的 A1 到 A6 和「三種原因」：每一個測試壞在哪裡、原因有多確定。
- `progress.md` 的實驗：「每個 task 之後收尾」修得掉放大的測試；五個探針（設了 `location.hash` 之後，同一個 task、下一個 timer、同一次 rendering、idle callback 之後讀不讀得到新的捲動位置）Chromium 156 都是新的，machopo 和「每個 task 之後收尾」都是舊的，當下捲動的原型和 Chromium 一樣。
- `progress.md` 的最後一節：別的 origin 的 `javascript:` 網址、延後的導覽套到新文件的兩個試驗（沒有重現）。

## 參考資料

- Chromium 152.0.7977.82：`third_party/blink/renderer/core/inspector/inspector_dom_agent.cc`（`DidInsertDOMNode`）、`third_party/blink/renderer/core/inspector/inspector_page_agent.cc`（`FrameAttachedToParent`）、`third_party/blink/renderer/core/html/html_frame_owner_element.cc`（`DisconnectContentFrame`）、`third_party/blink/renderer/core/loader/image_loader.cc`（`EnqueueImageLoadingMicroTask`）、`third_party/blink/renderer/core/script/script_loader.cc`（`PrepareScript`）、`third_party/blink/renderer/core/intersection_observer/intersection_observer_controller.cc`（`PostTaskToDeliverNotifications`）、`third_party/blink/renderer/core/frame/local_dom_window.cc`（`AddConsoleMessageImpl`）、`third_party/blink/renderer/core/inspector/inspector_css_agent.cc`（sheet 直接從 document 列）、`third_party/blink/renderer/core/loader/resource_load_observer_for_frame.cc`（送出的當下告訴 DevTools）、`third_party/blink/renderer/core/loader/frame_loader.cc`（`StartNavigation`、同一份文件內的導覽同步 commit）、`third_party/blink/renderer/core/frame/local_frame.cc`（`CanNavigate`）。
- machopo：`crates/dom_api/src/frame.rs`（`FrameRequest`）、`crates/dom_api/src/layout.rs`（`FrameLayout`、`LayoutCell`、`LentLayout`）、`crates/dom_api/src/engine.rs`（`LoadSink`、`ScriptEngine`）、`crates/renderer/src/render_thread.rs`（`run`）、`crates/renderer/src/render_thread/documents.rs`（`handle_frame_requests`、`RendererLoadSink`）、`crates/renderer/src/loader.rs`、`crates/bindings/src/isolate_state.rs`。
