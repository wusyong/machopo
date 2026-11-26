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
| fragment 的導覽 | DOM 設網址，renderer 捲動 | `LocalFrame`（捲動見待決定的第 1 條） | `FrameLoader`、`LocalFrameView` |
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

## 2c 的依賴盤點

2026-11-25，`ab6695a9` 上，唯讀。2c 要搬的五件事各碰到 renderer 的什麼、搬過去要 renderer 給什麼。分類：(a) engine 往外叫的 hook；(b) renderer 在呼叫之外問 engine；(c) hook 要 renderer 給的服務（排 task、要 main frame、碰畫面）；(d) 看起來要共用可變的狀態。

**已經有的**：engine 那一側碰得到畫面：`FrameLayout` 是 renderer 的 `Frame`，由 engine 拿著；`FrameLayout::scroll_to_fragment` 已經有，同文件的導覽經過 `cx.lend_layout` 用它（`dom_api` 的 `history.rs`），`check_parent_completed` 也這樣借。排 task 和計時有 `TaskWaker`（收得了時間），輸入在等有 `InputMonitor`。所以 (c) 大多不是新的。

| 要搬的 | renderer 那一側現在碰的 | 搬過去要的 |
| --- | --- | --- |
| HTML 的「the end」：`document_parsed`、`document_aborted`、`run_deferred_scripts` 的尾段、`finish_load`、`document_loaded` | `place_of`、`is_top`、`load_progress`；`scroll_to_fragment`（`FrameView` 的 `scroll_into_view_after_layout`、`request_main_frame`）；`page_agent`、`dom_agent` 的 DOMContentLoaded 和 `load`；`idleness.start()`；`tell_lifecycle`；`load_finished`（`DidFinishLoad` 跟著下一張 commit 的畫面送）；`start_loads`、`check_completed`、`start_image_fetches` | DevTools 的 DOMContentLoaded、`loadEventFired`、`frameStoppedLoading`：(a)，`RendererFrameClient` 已經拿著 `Rc<PageAgent>`、`Rc<DomAgent>`。`tell_lifecycle`：**(d)**，要 `document_frame_infos`（`render_frames` 的 MIME、原文、資源、導覽，加上 engine 的 frame），hook 被叫時 engine 借出去了。`load_finished` 和失敗的網址：(b)，畫面那一步問 engine。要 main frame：(c)，可以做成 (b)（呼叫之後問「要不要」，但要在輸入的 ack 比對 `main_frame_requests` 之前）。fragment：已經有。idleness：(b)（每個 task 之後看 DOM 的 load progress） |
| iframe 晚一點的 `load`（`owner_load_later`、`fire_due_owner_loads`、`RenderFrame::owner_load_at`） | `RenderFrames` 的 `owner_load_at`；scheduler 的 `OwnerLoads`；唯一的觸發點是 `child_navigation_not_sent`（`book_frame_requests` 底下，子 frame 的 fragment 導覽） | DOM 的延後的 element task（`TaskWaker` 收時間）就夠；但觸發點是導覽的帳（第 3 段或待決定的第 1 條），不先搬它就要一個過渡的往下呼叫 |
| 「跑完 script 之後 frame 還在不在」 | `advance_document`、`document_parsed`、`document_aborted`、`run_deferred_scripts`、`document_loaded`、`sheet_loaded`、`resource` 的 script 那一支、`retry_scripts_blocked_by_sheets`、`fire_due_owner_loads` 裡的 `doc(frame)` 重讀，約 30 處 | 跟著上面搬到 engine 那一側之後不用了；留下的：commit 的那些（第 3 段）、同文件的 traversal 的捲動（待決定的第 1 條）、`resource` 照文件找 frame（回應的路由）、畫面那一側的（改成問 engine） |
| 「接著 parse」 | `parsing`、`yields`、`parse_due`、`parser_budget`、`scheduler.has_task(INPUT)`、`InternalTask::ParseOn`；`note_body_parsed`、`parser_stopped`；入口：`read_body`、`resource`、`sheet_loaded`、`retry_scripts_blocked_by_sheets` | 每個會接著 parse 的往下的方法（body 的 bytes、script 和樣式表的回應、`run_dom_task`）都要照值借 engine（第 17 條的形狀），或者一律排 DOM 的 task（時機會變）；`InputMonitor` 和 `has_task(INPUT)` 是不是同一個訊號沒有確認；`body_parsed` 現在兩邊各一份 |
| 樣式表的回應（`sheet_loaded`） | `document_url`、文件的編碼、quirks、`failed`、`css_agent.keep_link_text`（有 session 時）、`set_link_sheet`、`load`／`error`、接著 parse、`finish_load` | 原文：(a)（拿著 `Rc<CssAgent>`）或交回；失敗的網址：DOM 已經有 `failed_sheets`；DevTools 的重抓（`css_refetched`）留在 renderer |

**順序的限制**：`DidFinishLoad` 跟著下一張 commit 的畫面的 swap promise（render-blocked 時等；空的 frame 不帶；`reftest-wait` 在那一張讀）；DevTools 是標題、`page_agent.update`、`lifecycleEvent`，`domContentEventFired` 在 DOMContentLoaded 的 lifecycle 之前，`loadEventFired` 在 `load` 的 lifecycle 和 `frameStoppedLoading` 之前，testharness 的結果在 DevTools 之前；`finish_load` 先 `start_loads` 再 `check_completed`，子 frame 先於父；輸入的 ack 等它要的那一張畫面。

**估計**：hook 2 到 3 個（DevTools 的生命週期、樣式表的原文，也許 `FrameLayout::set_render_blocked`），問 engine 的 3 到 4 個（`load` 結束了沒和失敗的網址、render-blocked、要不要 main frame），新的服務 0 個，另外 3 到 4 個往下的方法改成照值收 engine；合計 6 到 8 個，和 2a 同一個量級。最可能切不乾淨的：`tell_lifecycle`（要嘛呼叫之後送排著的事件，要嘛 `PageAgent` 自己記 frame 的資料）、iframe 晚一點的 `load` 的觸發點、失敗的網址分在兩邊（圖片和 script 的在 renderer，樣式表的在 DOM）。

## 第 3 段的依賴盤點

2026-11-26，`fb1c1d3e` 上，唯讀。分類同「2c 的依賴盤點」：(a) hook、(b) 呼叫之外問 engine、(c) hook 要的服務、(d) 看起來要共用可變的狀態。

**對帳現在的樣子**：`handle_frame_requests` 的呼叫剩 13 處（`after_task` 和 12 處；2a、2c 拿掉了 parse 迴圈裡的），每一輪 `book_frame_requests` 先 `take_asked` 加 `follow_child_frames`（成員的跟隨），再分派 `FrameRequest`；`commit_child` 另外直接 `follow_child_frames`。

| 要搬的 | renderer 那一側現在碰的 | 搬到送出的那次呼叫裡要的 |
| --- | --- | --- |
| 最上層的導覽（`Navigation` 的 `To`、`Reload`、`NewTab`、`GoToOffset`、`ScrollRestoration`） | `navigations_asked`（整個 renderer、照送出順序的帳）；`about_blank_origin_source`（DOM）；`render_frames.navigation_sent` | origin 的來源只要 DOM（`NavigationAsked` 多帶發起者的文件）；`navigations_asked` 是 (d)，或改成 engine 那一側的帳加 (b)，或 client 經過 `Arc` 的 scheduler 每個 post 一個 task（(c)，scheduler 本來就是 `Arc`） |
| 子 frame 的導覽（有 `begin_id`） | `child_no_referrer`、origin 的來源、`navigation_sent`、`sync_child_navigating`（DOM）；`tell_frames`（`frame_infos` 讀 `render_frames` 的 MIME、原文、資源、導覽） | 前幾樣 DOM 算得出；`tell_frames` 是 (d)，和 2c 的 `tell_lifecycle` 同一種：`PageAgent` 多一個當場的入口，或延到 task 結束 |
| iframe 的 `src` 是 fragment（`navigate_to_fragment`）和晚一點的 `load`（第 25 條） | `set_document_url`、`scroll_to_fragment`、`request_main_frame`；`owner_load_later`（`clock`、`owner_load_at`、`OwnerLoads`） | engine 自己做：`lend_layout` 加 `FrameLayout::scroll_to_fragment`、`schedule_frame`，晚一點的 `load` 是 DOM 延後的 element task（`TaskWaker` 收時間）；行為要定：照現在只改網址和捲動，或照規範走 `history::navigate`（多 `hashchange` 和 history 的項目） |
| iframe 的 `src` 是 `javascript:`（`queue_child_commit`） | `RenderFrames.commits`（跨 frame 的佇列）、`ChildCommits` 的 task | 佇列和導覽放在一起（`is_navigating` 兩個都讀）；post task 用 (c)，`run_child_commit` 用 (b) 拿 |
| 同文件的導覽（`SameDocument`） | `own_history_entries.sent_one()`（跨 engine 留著，commit 和 `SetHistoryOffsetAndLength` 也讀）；`request_main_frame` | 計數是 (d)：放在 engine 那一側、換 engine 時帶過去（(b) 兩個方法），或 `Rc<Cell<u64>>`；要畫面用 `schedule_frame` |
| blob URL 的登記和撤銷 | `blob_urls` | renderer 一定設 `LoadSink`，在 sink 裡當場做，這兩支走不到（只有沒有 sink 的 engine 用） |
| 成員的跟隨（`follow_child_frames`） | `render_frames.remove`、`set_child_document`（初始的 `FrameDocument`）、取消不在的文件的 fetch、`request_main_frame` | 只有 engine 建 frame 時當場建 `RenderFrame`（工廠，(a)），`FrameDocument` 也跟著搬，才拿得掉；只搬導覽的帳拿不掉 |

**Browser 的訊息**：`CommitNavigation`（最上層的拿 `take_navigation(top, begin_id)`，子 frame 的比 `begin_id`、`navigation_id`）、`NavigationDropped`、`CommitSameDocumentNavigation`、`SetHistoryOffsetAndLength`：帳在 engine 的 frame 上之後換成 (b) `navigation(frame)`、`take_navigation(frame, begin_id)`。`dom_js` 的 `Frames::set` 換文件時把 frame 整個換掉（只帶 token 和 modules），帳要一樣帶過去（第 3 條：最上層 commit 了別的文件時，它等的導覽留著）。`dom_wasm` 只有最上層，每個 (b) 一份薄的實作。

**commit 照舊由 renderer 驅動切得開**：`commit_document_navigation`、`commit_child`、`run_child_commit` 用的是畫面（`FrameView`、widget）、DevTools、`blob_urls`、`clock`、body 的 pipe、`DidCommitNavigation`，都是 renderer 那一側的；只要在開頭從 engine 拿走導覽（(b)）。

**八處過期的讀法和 1c 的檢查**：八處裡四處 2a、2c 之後已經不在；還在的四處（`finish_loads` 的 `frames()`、resize observer 的 `frame_render_blocked`、給 intersection 的 `child_documents()`、traversal 的 `scroll_to_fragment`）改成讀 engine 的 frame 的文件就好，和導覽的帳無關。1c 的檢查只守導覽的帳（第 10 條）：帳在送出的那次呼叫裡記好，就沒有東西會過期，整個拿掉。

**fragment（待決定的第 1 條）**：「記下、下一次 layout 時捲」已經有（`scroll_into_view_after_layout` 只設 `scroll_target`，Chromium 的 `FragmentAnchor` 的形狀）；輸入的 ack 只要 engine 在同一次呼叫裡 `schedule_frame` 就照舊等那張畫面。剩下要定的：iframe 的 `src` 是 fragment 時照現在的做還是走 `history::navigate`；「下一次 layout」（現在的，script 讀得到捲好的位置）還是「下一次 main frame」（Chromium 的）；traversal 的捲動搬進 `traverse_within_document`。

**估計**：小步（只把導覽的帳、`javascript:` 的 commit 佇列、晚一點的 `load` 搬到 engine 的 frame，commit 照舊由 renderer 做，`FrameDocument` 的表留著）：hook 0 到 1 個，問 engine 的 5 到 7 個，新的服務 1 個（`PageAgent` 當場的入口），和 2a、2c 同一個量級；拿掉 `FrameRequest` 的兩種導覽、`book_frame_requests` 的分派、1c 的檢查和 12 處額外呼叫裡的大約 11 處；`follow_child_frames` 留著（成員的跟隨還是呼叫之後）。完整的第 3 段（`RenderFrame` 和 `FrameDocument` 由 engine 的 frame 擁有）：`FrameDocument` 是 `renderer` 的型別，engine 叫不出它（`renderer` 依賴 engine，反過來是循環），要用 `dom_api` 的 trait 拿著、用時 downcast，和 `Frame` 當 `FrameLayout` 的前例一樣（`engine_frame`）；難的是 `read_body` 這類要同時借 `FrameDocument` 和 engine 的別的部分（engine 要給分開借的方法），約 60 處 `doc()` 要經過 engine 借：風險明顯高。（盤點原本寫成「帶著 `ipc` 的 pipe，只能型別抹除」：不對，`dom_api` 經過 `protocol` 本來就依賴 `ipc`，型別抹除的原因是 crate 的方向。）最危險的：送出時的 `tell_frames`（DevTools 的時機）、`Frames::set` 帶帳、拿掉中間的對帳時跟著消失的副作用（`request_main_frame` 讓輸入的 ack 等、取消不在的文件的 fetch 的時機）、history 的計數跨 engine、`dom_js` 八個用 `take_frame_requests` 的測試檔要改。

## 第 3 段完整的那一半的盤點

2026-11-26，3c 之上，唯讀。小步之後，呼叫之後的對帳只剩 `follow_child_frames`（`after_task` 一次、`commit_child` 一次）：照 engine 的 frame 清單補上新 frame 的 `FrameDocument`（初始的 `about:blank`）、拿掉不見的、取消不見的文件的 fetch、設 DevTools 的「frame 變了」、要一個 main frame。engine 那一側從沒讀過 `FrameDocument`（`dom`、`dom_api`、`dom_js`、`dom_wasm` 裡沒有它；hook 傳的是 DOM 和 `DocumentLoader` 的東西），它只放 renderer 自己的：Browser 的 navigation id、MIME、原文、body 的 pipe、time origin、編碼、畫到哪一版的計數、網路是否安靜、DevTools 的資源。

**做法的比較**：B（`FrameDocument` 放進 engine 的 frame：trait 加 downcast、每次經過 engine 借、分開借的方法）；C（整張表交給 renderer 的 `RendererFrameClient`，hook 當場建和拿掉：只有拿回 client 的一個 downcast，但讀表時 engine 被借走，一樣要分開借，換 engine 時表要搬）；D（engine 的清單是唯一的一份，renderer 的表只是照 frame 存附屬資料的快取：讀之前確認 engine 還有那個 frame，新 frame 的資料第一次用到時才建，不見的在 task 之後清掉，正確性不靠清的時機）；E（`Rc<RefCell>` 共用：每次讀都有執行時的檢查，不考慮）。D 不用碰型別和借用，盤點 D。

**讀表的地方**（`doc`、`doc_mut` 22 處，直接讀 `render_frames` 的其餘）：最上層的（`top`、`top_token`、`top_document`，約 25 處）不受影響：最上層的 `FrameDocument` 是 renderer 在 commit 時建的，script 不建也不拿掉它。子 frame 的分三種：

| 種類 | 哪些 | D 要怎麼改 |
| --- | --- | --- |
| 只要文件的 root | `document_url`、`document_of_frame`（`loader_of`、`subresource_cache_mode` 經過它）、`document_source` 的一半、`document_origin`、`note_failed`、`loader_id` | 改讀 `engine_document`（3c 加的），不經過表 |
| renderer 自己的狀態 | body 的（`start_body`、`body_data`、`body_ended`、`read_body`、`frame_with_body`、`document_load_completed`）、`commit_child_navigation` 和 traversal 的 navigation id、`parse_document`：都只讀 commit 過的文件，`commit_child` 當場建了；初始文件也會用到的：網路是否安靜（`check_network_idle`）、DevTools 的資源（`record_resource`、`css_refetched`、`child_info`）、回應的編碼（`resource`） | `doc`、`doc_mut` 只回傳 engine 還有的 frame 的（`child_parent`）；`doc_mut` 在 engine 有、表上沒有時建初始文件的那一份（計數、偵測、資源都是預設值，和現在 task 之後建的一樣） |
| 走過整張表 | `scripts_blocked_by_sheets`、`frame_of_document`（回應找 frame、DOM agent）、`check_network_idle`、`next_idleness_due` | 照 engine 的 `frames_in_tree_order()` 走、查表；`frame_of_document` 問 engine（`dom_js` 有 `Frames::of_document`：`ScriptEngine` 多一個方法）；`next_idleness_due` 是 `&self`，照舊走快取（不見的 frame 留著的只會讓 loop 早醒一次，醒來什麼都不做） |

**`follow_child_frames` 剩下什麼**：建的那一半拿掉（第一次用到時建）；`commit_child` 裡直接叫的那一次拿掉。task 之後留一步清理，不再是跟隨：拿掉 engine 沒有的 frame 的資料（它們拿著不見的文件的 `NodeRoot` 和 body 的 pipe，和現在一樣在 task 之後放掉）、取消不見的文件的 fetch（本來就照 DOM 的 `sent_fetches` 判斷，不讀表）、DevTools 的「frame 變了」（照 `take_frames_changed`）。要 main frame 的那一行：iframe 加進、拿掉本身是 DOM 的改變，照理是多的，實作時用 mutation test 確認。讀表的地方都先問 engine，這一步晚做、漏做只影響記憶體，不影響讀到什麼。

**`doc()` 改成 `&mut self`**：呼叫它的函式只有 `doc` 自己是 `&self`，其餘都已經是 `&mut self`；`next_idleness_due` 不經過 `doc`。

**估計**：hook 0 個；問 engine 的 0 到 1 個（`frame_of_document`）；改約 15 處（6 處讀 root、4 處走表、`doc`/`doc_mut`、`follow_child_frames` 和 `commit_child`），和 3c 同一個量級。最危險的：初始文件的資料改成第一次用到才建，以前在 task 之後就有（DevTools 的 frame 清單和網路是否安靜是不是在那之前就要它）；清理晚了的那段期間，`next_idleness_due` 和 DevTools 會不會看到不見的 frame。對照 Chromium：content 的 `RenderFrameImpl` 是 Blink 建 `LocalFrame` 時當場建的（C 的形狀）；D 不同的是 renderer 的資料晚一點才建，但兩者都只有一份 frame 的清單。

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
