# RFC 0115 的細節

## 現況

2026-11-25 的 `main`（`59ad93c7`）讀程式整理的（一部分由唯讀的 agent 盤點，關鍵的幾處重讀過）；行號是那時的。

### hook：`RendererFrameClient`

- `crates/renderer/src/render_thread/documents.rs` 4840–4851：欄位是 `to_browser`（`BrowserSender`）、`page_agent`、`dom_agent`、`network_agent`（都是 `Rc`）；不記任何 frame 的狀態。engine 擁有它：`make_engine` 裡 `engine.set_frame_client(Box::new(...))`（`render_thread/devtools.rs` 243–248），和 `RendererLoadSink`、`RendererStorageHost`、`RendererCookieHost` 一樣。
- 方法（4858–5046）：
  - `child_frame_created`：送 `DidCreateChildFrame`，先 flush DOM agent 的改動，再告訴 Page agent。
  - `child_document_created`：只告訴 Page agent。
  - `frame_detached`：送 `DidDetachFrame`，告訴 Page agent。
  - `navigate`：`To`、`Reload` 送 `BeginNavigation`（編號是 thread-local 的計數器，`loader.rs` 56–62），`SameDocument` 送 `DidCommitSameDocumentNavigation`，`NewTab` 送 `OpenUrl`，`GoToOffset` 送 `GoToEntryAtOffset`，`ScrollRestoration` 送 `UpdateState`。只有 `BeginNavigation` 回 `Some(id)`。
  - `navigate_child`：先送 `DidChangeSrcDoc`，不是「哪裡都不去」（fragment、`javascript:`，`child_navigation_goes_nowhere` 5123）就送 `BeginNavigation`；`history_in_new_child_frame`、`load_flags` 從 `&Dom` 算，DevTools 的 initiator 用 `network_agent` 算。

### engine 記下的

- `crates/dom_js/src/frames.rs`：`navigate_child`（352–402）、`navigate`（410–455）呼叫 hook，再推 `FrameRequest::Navigate { frame, navigation, begin_id }` 或 `FrameRequest::Navigation { frame, initiator, navigation, begin_id }` 進 `Frames.requests`；`begin_id` 有的時候 `set_frame_navigating(doc, true)`（392–396、444–448）。
- `FrameRequest`（`dom_api/src/frame.rs` 56–91）由 `take_frame_requests`（`dom_js/src/engine.rs` 654）拿走；`RegisterBlobUrl`、`RevokeBlobUrl` 也走它。
- 導覽從 `dom_api::history::update_navigations`（`history.rs` 663）來：每個操作結束時（`frame.rs` 124 的 `update_elements`）拿 DOM 排的導覽、發 `popstate`、呼叫 host 的 `navigate`。

### 呼叫之後的對帳

- `RenderThread::after_task`（`render_thread.rs` 765–810）裡、和 engine 的其他十七個入口之後（`render_thread.rs` 1301、1785、1921，`documents.rs` 1194、1466、1770、1959、2041、2454、2469、2499、2504、2550、2593、2747、2755、4171）：`handle_frame_requests`（`documents.rs` 1007–1042）。
- `follow_child_frames`（779–837）：engine 說 frame 變了時走一遍 `child_frames_below(top)`，拿掉不見的（`ChildFrames::remove`、取消它們文件的 fetch），新的 frame 推一份 `initial` 的 `FrameDocument`。
- `frame_navigated`（1067–1122）：`begin_id` 推進 `navigations_asked`；最上層的 `To` 記 `main_origin_source`（`about_blank_origin_source` 1284）；子 frame 的 `To`、`Reload` 到 `child_navigation_sent`（1255），沒送的到 `child_navigation_not_sent`（1236）；`SameDocument` 記 `own_history_entries.sent_one()`、要一個 main frame。
- `child_navigation_sent`（1255–1276）：讀 iframe 的 `referrerpolicy`（`child_no_referrer`）、initiator 的文件，`children.navigating(frame)`（清掉之前的導覽和排著的 commit），插一筆 `ChildNavigationRequest { begin_id, no_referrer, origin_source }`，`sync_child_navigating`（903），告訴 Page agent。
- `child_navigation_not_sent`（1236）：fragment 的 `navigate_to_fragment`（2640，經過 `FrameView` 捲動）和 `owner_load_later`；`javascript:` 的 `queue_child_commit`（1406，post `InternalTask::ChildCommits`）。
- `end_cancellation_windows`（`render_thread.rs` 900）：`navigations_asked` 送成 `NavigationCancellationWindowEnded`。
- 要在呼叫之後的理由：這些步驟要 `RenderThread` 的東西，engine 擁有的 hook 碰不到（註解在 `documents.rs` 776–778、1060–1066、1199–1204）：`children` 和 `document`、`main_origin_source`、`navigations_asked`、`own_history_entries`、scheduler、時鐘和 `owner_loads`、`FrameView`、Page agent。其中真的離不開 `RenderThread` 的只有 `FrameView`（fragment 的捲動）和 `FrameDocument`（parser、body 要和 engine 同時借）；其餘的不是 `Rc` 就是可以搬的資料。

### 照 frame 記的狀態

`RenderThread`（`render_thread.rs` 364–551）：

| 欄位 | 型別、key | 誰寫 | 誰讀 | Browser 的訊息 |
| --- | --- | --- | --- | --- |
| `document` | `Option<FrameDocument>`，最上層 | `commit_document_navigation`（`documents.rs` 660）、`replace_engine`（700） | `doc`、`doc_mut`（64 處） | `CommitNavigation` |
| `children` | `ChildFrames` | 見下表 | | |
| `navigations_asked` | `Vec<u64>`（`begin_id`） | `frame_navigated`、`navigate_child` | `end_cancellation_windows` | 送 `NavigationCancellationWindowEnded` |
| `main_origin_source` | `Option<(u64, NodeRoot)>`，最上層 | `frame_navigated` | `commit_navigation_body`（1184）、`replace_engine` | `CommitNavigation` |
| `devtools_main_frame_token` | `Option<FrameToken>` | `DevToolsMainFrameToken` | `frame_token`、`frame_with_body`、commit | |
| `own_history_entries` | 整個 renderer 的計數 | `frame_navigated` | commit、`SetHistoryOffsetAndLength` | |

`ChildFrames`（`documents.rs` 250–264）：

| 欄位 | 型別、key | 誰寫 | 誰讀 | Browser 的訊息 |
| --- | --- | --- | --- | --- |
| `documents` | `Vec<FrameDocument>`，照 `.frame` 找 | `follow_child_frames`（834）、`commit_child`（1647、1656） | `doc`、`known`、`commit_child_navigation`（1323） | `CommitNavigation` |
| `navigations` | `HashMap<FrameId, ChildNavigationRequest>` | `child_navigation_sent` | `commit_child_navigation`（1313、1332）、`child_no_referrer_of`（1352）、`navigation_dropped`（1365）、`sync_child_navigating` | `CommitNavigation`、`NavigationDropped` |
| `commits` | `VecDeque<(FrameId, Url, String)>` | `queue_child_commit` | `run_child_commit`（1478，送 `DidCommitNavigation`） | |
| `owner_loads` | `HashMap<FrameId, f64>` | `owner_load_later`（1419） | `fire_due_owner_loads`（1448） | |

- 最上層 commit 新文件（`commit_document_navigation` 656）和換 engine（`replace_engine` 701）時整個 `ChildFrames` 重設。
- frame 不見不是 Browser 的訊息：`follow_child_frames` 發現之後 `ChildFrames::remove`（317–334），它沒有清 `owner_loads`（`known()` 卻算它）。
- 在 DOM 或 engine 的、不在 renderer 的：navigation timing、文件的網址、「frame 在載入中」（`dom` 的 `frame_navigating`）、session history、frame 的清單。
- 別的照 frame 的：`FrameView.children`（`frame_view.rs` 60–80）、Page agent 的 `lifecycles`、`frames`、`attached`、`committing`（`page_agent.rs` 186–206）、DOM agent 的 `frames`、`css_refetches`、`BlobUrlState.live`。

### frame 的名字

- `dom_api::FrameId`：engine 給的（`Frames::next_id`，最上層是 `FrameId(0)`）。
- `FrameToken`（16 bytes）：子 frame 的由 engine 抽（`new_frame_token`，`create_child`），最上層的由 Browser 給（`DevToolsMainFrameToken`），renderer 和 engine 各存一份。renderer 沒有自己的對照表，問 engine（`frame_token`、`frame_of_token`、`frame_parent`、`child_frames`）。
- `begin_id`：thread-local 的計數器；`navigation_id`：Browser 的；`RENDERER_COMMIT = 0`：renderer 自己的 commit（`javascript:`）。

### 大小

`render_thread/documents.rs` 5789 行、`render_thread.rs` 2108、`loader.rs` 513、`navigation_timing.rs` 539、`page_agent.rs` 798；engine 那一側 `dom_js/src/frames.rs` 968、`dom_api/src/frame.rs` 588、`dom_api/src/history.rs` 767。

## 狀態的歸屬

做完之後每一份 per-frame 的狀態在哪（`FrameDocument` 的欄位在 `documents.rs` 51–131）：

| 狀態 | 現在 | 之後 | 對照 Chromium |
| --- | --- | --- | --- |
| 文件（`document`）、MIME type、編碼（`encoding`） | renderer 的 `FrameDocument` | engine 的 `LocalFrame` | `DocumentLoader` |
| parser、輸入、`yields`、`parse_due`、`body_parsed` | renderer 的 `FrameDocument` | `LocalFrame` | `DocumentLoader`、`HTMLDocumentParser` |
| parser 擋住的和 deferred 的 script（`blocking_script`、`deferred_scripts`） | renderer 的 `FrameDocument` | `LocalFrame` | `ScriptRunner`、`HTMLParserScriptRunner` |
| 等著的樣式表（`pending_sheets`、`requested_links`） | renderer 的 `FrameDocument` | `LocalFrame` | `StyleEngine`、`LinkStyle` |
| `load_finished`、`aborted`、`failed`、owner 的 `load`（`owner_loads`） | renderer | `LocalFrame` | `FrameLoader`、`Document::CheckCompleted` |
| 「在載入中」 | DOM（`frame_navigating`），由 engine 和 renderer 兩邊設 | `LocalFrame` | `FrameLoader::IsLoading` |
| fragment 的導覽 | DOM 設網址，renderer 捲動 | `LocalFrame`（捲動見待決定的第 2 條） | `FrameLoader`、`LocalFrameView` |
| body 的 pipe（`body`） | renderer 的 `FrameDocument` | `RenderFrame`，bytes 交給 `LocalFrame` | `NavigationBodyLoader`（content） |
| 進行中的導覽（`ChildNavigationRequest`、`main_origin_source`、`navigations_asked`） | renderer 的三張表 | `RenderFrame` | `RenderFrameImpl`、`NavigationClient` |
| `navigation_id`、排著的 renderer 自己的 commit（`commits`） | renderer | `RenderFrame` | `RenderFrameImpl` |
| navigation timing、`time_origin` | DOM 和 renderer | `RenderFrame` 給、`LocalFrame` 用 | `DocumentLoader`（Blink 那一側記） |
| 這個 frame 的 fetch 和 loader（`resources`）、idleness | renderer 的 `FrameDocument` | `RenderFrame` | `RenderFrameImpl` 的 `URLLoaderFactory` |
| DevTools 的 token（`devtools_main_frame_token`） | renderer | `RenderFrame` | `RenderFrameImpl` 的 frame token |
| `rendered_*`、`sent_title`、`title_version`、`waiting_for_reftest` | renderer 的 `FrameDocument` | 第 1 段時決定（畫面那一側的，跟著 `FrameView`） | `LocalFrameView`、`RenderFrameImpl` |

## 依賴的盤點

2026-11-25 的 `main`（`59ad93c7`），唯讀的 agent 盤點、我讀過結果（行號是 `render_thread/documents.rs` 的，`rt` 是 `render_thread.rs`）。

`FrameDocument` 有 64 處 `doc`、`doc_mut` 和 29 處直接用 `document`、`children.documents`，幾乎都在 `documents.rs`。每一處所在的函式除了文件，還碰了：

| 碰到的 | 處數 | 說明 |
| --- | --- | --- |
| 畫面（`FrameView`、`request_main_frame`、lifecycle） | 33 | render-blocking、`rendered_*` 的比對、`DidFinishLoad` 的 swap promise |
| 載入（loader、`FetchResource`、`FrameResources`） | 30 | parser 的 script 和 `<link>` 的樣式表由 renderer 發請求、收回應 |
| 對 Browser 的訊息 | 25 | `UpdateTitle`、`FetchResource`、`DidFinishLoad`、harness 的結果 |
| DevTools | 20 | Page、Network agent 讀 MIME type、原始碼、resources |
| scheduler | 17 | parser 讓出（`InternalTask::ParseOn`）、body 的 waker、idleness |
| 導覽的帳 | 10，另 16 處經過 `handle_frame_requests` | parse 的迴圈裡呼叫 |
| 只有 engine 和文件 | 10 | `script_ready`、`prepare_script`、`document_url` 這類 |

- **沒有一個欄位只屬於文件的載入**：每個欄位至少有一處在同時碰畫面、載入、scheduler 或 DevTools 的函式裡。最接近的是 parser 的輸入、`yields`、`parse_due`、`aborted`（只多碰 scheduler：`post_internal(ParseOn)`、`has_task(INPUT)`）。
- **和畫面纏在一起的**：`body_parsed` 和 `pending_sheets`（`render_blocked` 擋住第一次畫面、子 frame 的 `FrameView::set_render_blocked`、給 intersection 的 `blocked`、`media` 照 viewport 判斷）；`rendered_*`（`request_next_frame_if_dom_changed` 比 DOM 的版本、`update_document_rendering` 重設）；`load_finished`、`waiting_for_reftest`（commit 的 swap promise）；`sent_title`（時機跟著 `begin_main_frame`）。
- **對 Browser 的**：`navigation_id`、`body`（data pipe、`body_waker`）、`time_origin`、`failed`、`sent_title`、`requested_links`，DevTools 讀的 `mime_type`、`source`、`resources`、`initial`、`idleness`。
- **最纏的函式**：`commit_document_navigation`（七種都碰）、`advance_document`（parse 的迴圈，十處，六種）、`commit_child`、`replace_engine`、`update_document_rendering`、`start_loads_of`、`run_deferred_scripts`、`sheet_loaded`。
- **parser**：`html::parser::Parser<H: ParserHost>`（`html` crate 只依賴 `dom`、html5ever、`elise`、`base`）。engine 照值借給它（`advance_document` 裡 `engine.take()`、`resume`），`ParserHost` 三個方法。讓出：時間的預算到了、或有輸入的 task 在等時，排 `InternalTask::ParseOn`；body 每個 task 最多讀 `BYTES_PER_TASK`，再由 `body_waker` 排 `NavigationBody`。parser 的 script：`dom_api::prepare_script` 決定種類，renderer 發請求（`fetch_parser_script` → `FetchResource`）、回應進 `resource`、`script_ready` 再看擋 script 的樣式表。
- **engine 那一側已經有的**：`dom_api` 的 `lifecycle.rs`（`check_completed`、`document_ready`、`end_load`）、`frame.rs` 的 `check_parent_completed`、`script.rs`（`prepare_script`、`run_script_task`、`send_script_fetches`），`dom` 的 load request 的計數和 `load_progress`，`LoadSink` 的 `start_script_fetch`（DOM 自己排的 script 已經經過 hook 發請求）。兩個 engine 的實作大多是「找到 frame 的 context、進 scope」的薄包裝；`dom_wasm` 只有最上層一個 frame。
- `encoding.rs` 只依賴 `encoding_rs`；`document_body.rs` 是 data pipe、解碼器、`ParserInput`。

## 對照 Chromium（原始碼）

2026-11-25 讀 chromium.googlesource.com 的 `main`（唯讀的 agent 讀、我讀過結果）：

- **送出導覽**：`FrameLoader::StartNavigation` → `LocalFrameClientImpl::BeginNavigation` → `RenderFrameImpl::BeginNavigation`：跑 beforeunload、`frame_->WillStartNavigation`（Blink 記 `FrameLoader::client_navigation_`、進度開始，`HasProvisionalNavigation` 讀它），再交給 `client_navigation_throttler_` 排 `BeginNavigationInternal`，可能晚到之後；`BeginNavigationInternal` 組 `BeginNavigationParams`、`CommonNavigationParams`，設 `is_requesting_navigation_`，把兩份參數存進新的 `navigation_client_impl_`（`NavigationClient`），送 `FrameHost::BeginNavigation`（帶著 `NavigationClient` 的 remote，關掉它就是取消）。所以兩邊各記一份：Blink 記「這個 frame 在導覽」，content 記「對 Browser 的這一次請求」。
- **commit 和 body**：`RenderFrameImpl::CommitNavigation` → `WebLocalFrame::CommitNavigation` → `FrameLoader::CommitNavigation` 建 `DocumentLoader`。body 的 loader（`NavigationBodyLoader`）是 Blink 的，content 只把 Browser 給的 data pipe 交給 Blink 的 `FillNavigationParamsResponseAndBodyLoader`；`DocumentLoader` 擁有它、是它的 client（`BodyDataReceived`、`BodyLoadingFinished`）。
- **parser 和 script**：`DocumentLoader` 擁有 parser，parser 擁有解碼器；`HTMLParserScriptRunner`（parser 擋住的、defer）、`ScriptRunner`（async、in-order）；`Document::CheckCompleted` 判斷 `load`，之後 `DidFinishLoad` 送 Browser。都在 Blink。
- **parser 讓出**：`HTMLDocumentParser` 自己 post 到文件的 `kNetworking` task runner（沒有 `HTMLParserScheduler` 了），時間或 token 的預算到了、或 thread 的 scheduler 說有高優先的工作時讓出。
- **render-blocking**：Blink 的 `RenderBlockingResourceManager`（head 裡的樣式表、script、字型的 preload），`Document::BeginLifecycleUpdatesIfRenderingReady` 之前不更新畫面。
- **fragment 的導覽**：不經過 Browser，Blink 在 `StartNavigation` 裡同步 commit；捲動不是當場做的：設好 `FragmentAnchor`、要一個 frame，在 lifecycle 的 layout 之後才捲（`LocalFrameView::InvokeFragmentAnchor`）。
- **擁有權**：`RenderFrameImpl` 不擁有 `WebLocalFrame`（`raw_ptr`），Blink 也只拿著 client 的 `raw_ptr`；先建 `RenderFrameImpl`，再交給 Blink 的工廠（`CreateMainFrame`、`CreateProvisional`、`CreateLocalChild`）；frame 拆掉時 Blink 告訴它（`FrameDetached`），它 `delete this`。一起建、一起走靠約定，不是擁有權。

## 盤點之後的看法

- machopo 的 renderer 現在替文件做的，在 Chromium 幾乎都是 Blink 的：發子資源的請求（`ResourceFetcher`）、parser 和 script 的排程、render-blocking、`load`、fragment 的捲動。所以「文件的載入搬到 engine 那一側」不是只搬 parser，連發請求、擋畫面的判斷一起。
- 機制大多已經有：engine 那一側要碰 renderer 的服務時，用 RFC 0110、0112、0114 的 hook 的做法（`LoadSink` 已經替 DOM 自己排的 script 發請求，`TaskWaker` 已經讓 DOM 的 task 經過 scheduler）；畫面那一側要知道「擋著沒」時問 engine（像現在問 DOM 的各種版本）。parser 讓出可以照 Chromium 的 `kNetworking` 的做法，變成 DOM 的一種 task。
- 畫面那一半（`rendered_*`、`load_finished` 和 reftest 的 swap promise、標題的時機）不是文件的載入，是每個 frame 的畫面的狀態，跟著 `FrameView`（Chromium 的 `LocalFrameView` 和 `RenderFrameImpl` 的 widget 那一面）；它不用對帳，只用問的。
- fragment 的捲動照 Chromium 改成「設好、下一次 main frame 時捲」，不在導覽的呼叫裡碰 layout。

## 做法的比較

2026-11-25 討論過、只在 renderer 這一側補的做法：

| | A：帳在 hook | B：共用的 `Rc<RefCell>` | C：只收成 `RenderFrame` | C 加 debug 的檢查 | 這份：照 content 和 Blink 分 |
| --- | --- | --- | --- | --- | --- |
| 送出時就記帳 | 是（fragment 的捲動除外） | 是（同左） | 否 | 否 | 是 |
| 十八處 | 剩要 engine 的幾件 | 剩要 engine 的幾件（fragment 的捲動、新 frame 的文件、清掉不見的） | 留著，變成分派 | 留著，忘了叫時 debug build panic | 拿掉 |
| 一個 frame 的狀態 | 兩半：帳在 hook、文件在 `RenderThread` | 兩半：帳在格子、文件在 `RenderThread` | 一個 `RenderFrame` | 同左 | 兩個物件，各管不同的事、各有一個擁有者 |
| 借用 | 靜態，讀帳經過 engine 的 downcast | 執行時：171 處碰 engine 的地方，任何一處借著格子呼叫 engine 就 panic | 靜態 | 靜態 | 靜態，跟著呼叫的方向 |
| 規模 | 中 | 小 | 中（機械） | 中 | 很大，分段 |

為什麼只在 renderer 這一側補拿不完：呼叫之後要做的事裡，fragment 的捲動要那份文件的 layout（經過 engine，而 hook 被叫的時候 engine 正被借著），新 frame 的文件和 parser 要和 engine 同時借（`FrameDocument` 放不進共用的格子）。這兩件在 Chromium 都是 Blink 自己做的；搬到 engine 那一側，hook 就不用碰它們。

不考慮的：engine 的入口多收一個 `&mut` 的 host（hook 在 V8 的 callback 裡被呼叫，從 isolate 的 slot 拿狀態，借用存不進 slot，要 `'static` 或在 `bindings` 加有 `unsafe` 的機制和它的 soundness 論證；每個入口的簽名都要改）。
