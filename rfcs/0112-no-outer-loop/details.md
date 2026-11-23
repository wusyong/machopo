# RFC 0112 的細節：拿掉 event loop 的外圈

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-24 照 `main` 的 `1a4d20e` 的 `RenderThread::run`（`crates/renderer/src/render_thread.rs`）讀的，順序照程式。「Chromium」一欄對過 Chromium `main` 的 `28312288d3ac`（2026-11-23）的原始碼（subagent 從 Gitiles 讀的，行號是那個 commit 的；`third_party/blink/renderer/` 底下的路徑省略前綴）。「類」：**當場**（Blink 在發生的當下做）、**task**（Blink 排成 scheduler 的 task，括號裡是 `TaskType`）、**之間**（Chrome 也在 task 之間做：`TaskObserver`、idle period、rendering 的步驟）。

**內圈，每個 scheduler 的 task 之後**：

| 步驟 | 做什麼 | Chromium | 類 |
| --- | --- | --- | --- |
| `run_queued_tasks` | DOM 在那個 task 裡排的 task（例如 fragment 導覽的 `hashchange`） | `EnqueueHashchangeEvent` 排 task（`core/frame/local_dom_window.cc:1061`） | task（kDOMManipulation） |
| `update_title` | 分頁的標題 | `Document::UpdateTitle` → `DispatchDidReceiveTitle`，main frame 當場送 mojo 的 `UpdateTitle`（`core/dom/document.cc:2212`） | 當場 |

**外圈，通道空了之後**：

| 步驟 | 做什麼 | Chromium | 類 |
| --- | --- | --- | --- |
| `blink_caret` | caret 的閃爍 | `FrameCaret` 的 repeating timer（`core/editing/frame_caret.cc:64`） | task（kInternalDefault） |
| `new_content_rendering_timeout` | 新文件一直畫不出來時清掉舊的畫面 | Browser 的 `TimeoutMonitor`，到了 `ClearDisplayedGraphics`（`render_widget_host_impl.cc:546`） | task（Browser 的 timer） |
| `intersections_due_now` | intersection observer 的 `delay` 到了，要一個 frame | `ScheduleDelayedIntersection` 的 timer，到了 `ScheduleAnimation`（`core/frame/local_frame_view.cc:5075`） | task（kInternalDefault） |
| `fire_due_owner_loads` | 跨 origin 的子 frame 在同一份文件內導覽之後，延後的 iframe 的 `load` | `CommitSameDocumentNavigation` 排 100 ms 後的 `DispatchLoad`，再導覽就取消（`core/loader/document_loader.cc:2004`） | task（kInternalLoading） |
| `handle_popup_requests` | `<select>` 的選單 | `ExternalPopupMenu::Show` 當場送 mojo（只有 Mac 和 Android；其他平台是 `InternalPopupMenu`） | 當場 |
| `handle_frame_requests` | 導覽（多數的 engine 呼叫之後也跑） | `location.href` 的 setter 裡同步送 `BeginNavigation`：`FrameLoader::StartNavigation` → `RenderFrameImpl::BeginNavigation`（`render_frame_impl.cc:6376`）；同一個 task 裡第二個導覽也送，Browser 取消第一個（「導覽」） | 當場 |
| `handle_frame_requests` | blob URL 的登記和撤銷 | `PublicURLManager::Revoke` 當場送 mojo | 當場（撤銷不在範圍內） |
| `run_child_commits` | 子 frame 的 `srcdoc`、`about:` 文件的 commit，各一個 task | `srcdoc` 由 Browser 驅動，renderer 收到 `CommitNavigation` 才 commit；`about:blank` 在 `BeginNavigation` 裡同步 commit（`render_frame_impl.cc:5602`） | `srcdoc`：task；`about:blank`：當場 |
| `run_due_tasks` | DOM 到期的 task：timer、element task、message | `DOMTimer` 照 nesting 選 `TaskType`，是 delayed task（`core/scheduler/dom_timer.cc:350`） | task（kJavascriptTimer*） |
| `parse_on` | 讓出的 parser 續跑，各跑一段 | `SchedulePumpTokenizer` 排 0 ms 的 `DeferredPumpTokenizerIfPossible`（`core/html/parser/html_document_parser.cc:928`） | task（kNetworking） |
| `start_image_fetches` | DOM 的圖片清單等著、engine 沒送的（style 找到的背景圖） | style recalc 裡 `LoadPendingImages` 同步發 fetch（`element_style_resources.cc:443`） | 當場（style 更新時） |
| `start_script_fetches` | DOM 排著、engine 沒送的 script（parser 的） | `ScriptLoader::PrepareScript` 同步 fetch；preload scanner 在 pump 裡同步送 | 當場 |
| `retry_scripts_blocked_by_sheets` | 等擋 script 的 sheet 的 parser 的 script，再試一次 | 計數歸零時 `DidLoadAllScriptBlockingResources` **排 task** 跑它們（`core/dom/document.cc:5330`） | task（kNetworking） |
| `start_canvas_encodes` | `toBlob` 的編碼送到 encoder thread | main thread 的 idle task 裡逐段編碼，1 秒沒開始就改成立即；webp 當場送到 worker（`canvas_async_blob_creator.cc:254`） | 之間（idle task） |
| `send_webdriver_results` | settle 了的 script 的結果給 WebDriver | V8 inspector 在 promise 的 reaction（microtask）裡當場送 response | 當場 |
| `log_console_warnings` | engine 的警告到 console | 產生時當場 `probe::ConsoleMessageAdded`，CDP 的通知進佇列，task 結束時才送（下面的 `tell_dom_changes`） | 當場產生，之間送出 |
| `log_canvas_raster_failure` | canvas 的 raster 失敗 | 沒有對應（最接近的是 2D context lost 的 0 ms timer） | 沒有對應 |
| `blob_urls.flush_revoked` | blob URL 的撤銷告訴 Browser（那一批 task 要的載入之後） | 當場送 mojo，導覽帶 `BlobURLToken` | 不在範圍內（另一份） |
| `check_network_idle` | network 安靜夠久的文件 | `IdlenessDetector` 是 `TaskTimeObserver`，`WillProcessTask` 時檢查，另有 watchdog 的 timer（`core/loader/idleness_detector.cc:147`） | 之間 |
| `tell_frames` | DevTools 的 frame 事件（比對上次） | `frameAttached` 在 `LocalFrame` 的 constructor 裡當場送並 flush；`frameDetached` 在 `DetachImpl`；`frameNavigated` 在 `WillCommitNavigation`（`core/frame/local_frame.cc:2134`） | 當場 |
| `tell_dom_changes` | DevTools 的 DOM 變動 | `NotifyNodeInserted` 當場 `probe::DidInsertDOMNode`，通知進 `notification_queue_`；`WebDevToolsAgentImpl::DidProcessTask`、送 response 之前、別的 agent `flush()` 時送出（`core/inspector/devtools_session.cc:446`） | 當場產生，之間（每個 task 之後）送出 |
| `start_css_refetches` | DevTools 的 `CSS` 要的 sheet，重新抓 | `CSS.enable` 的那個 task 裡同步 fetch（`inspector_resource_content_loader.cc:76`） | 當場 |
| overlay | DevTools 的 overlay 跟著節點，要一個 frame | style 和 layout 之後的 probe → `ScheduleAnimation`，在 lifecycle 裡畫 | 之間（rendering） |
| `run_platform_tasks` | engine 的 platform task（GC）和之後的 checkpoint | V8 的三種 foreground task runner 對應 main thread scheduler 的 `kMainThreadTaskQueueV8*`（normal、low、best effort），Oilpan 走同一個 platform | task |
| `run_idle_callback` | `requestIdleCallback` 的一個 callback | scheduler 的 idle period（frame 之間的短的、不期待 frame 時的長的） | 之間 |
| `request_next_frame_if_dom_changed` | DOM 變了就要一個 frame | `ScheduleLayoutTreeUpdate` 當場 `ScheduleAnimation` → `SetNeedsAnimate`（`core/dom/document.cc:2544`） | 當場 |
| `clear_request_stacks` | 沒有載入拿走的 script 的 stack | 沒有對應（machopo 自己的記錄） | 之間 |
| `set_needs_begin_main_frame` | 告訴 compositor 要不要 main frame | 要求的當下 `SetNeedsAnimate` | 當場 |

**和草稿（憑記憶）不同的**：標題的 IPC 不經過 `LocalFrameClient`；等 sheet 的 script 是排 task，不是當場；`toBlob` 是 idle task；DevTools 的 DOM 變動和 console 是當場產生、**每個 task 之後**送出（不是當場送），所以它們在 machopo 屬於「之間」，差別只在 machopo 等整批 task 之後；`frameAttached` 則是當場送，送之前 flush 佇列，所以 `childNodeInserted` 在它之前。

### scheduler 怎麼挑下一個

- 先看優先度，只在最高的那一級裡挑（`TaskQueueSelector::SelectWorkQueueToService`）；同一級裡跨 queue 取 `TaskOrder` 最舊的（`WorkQueueSets::GetOldestQueueAndTaskOrderInSet`）。immediate task 在 post 時拿到 enqueue order；delayed task 在到期、被搬進 work queue 時才拿到，所以到期的 timer 排在「到期被看到的那一刻」。
- 預設優先度：timer、networking、DOM manipulation 都是 normal；input 是 highest；`kInternalContinueScriptLoading`、`kInternalPostMessageForwarding`、`kInternalNavigationCancellation` 是 very high；render-blocking 的 loading 在頁面可見時是 extremely high。
- 防餓死：同一級裡 delayed 最多連續擠掉 immediate 3 次；rendering 超過 100 ms 沒有 main frame 時 compositor 的 queue 升到 very high。不同優先度之間沒有通用的防餓死。

### 導覽

- 同一個 task 裡設兩次 `location.href`：兩個 `BeginNavigation` 都在 setter 裡送；renderer 換掉舊的 `NavigationClient`（`kResetForNewNavigation`），Browser 經過它斷線或第二個 `OnBeginNavigation` 的 `TakeNavigationRequest` 取消第一個（`navigator.cc:1339`、`frame_tree_node.cc:559`）。例外：進行中的是 browser 發起或 history 的導覽、新的沒有 user gesture 時忽略新的；同一個 URL 在門檻內重複時 renderer 可能不送第二個。
- 第一個導覽不會在兩次之間 commit：`RendererCancellationThrottle` 要等發起導覽的 task 結束（renderer 在 kInternalNavigationCancellation 排的 `NotifyNavigationCancellationWindowEnded`）才讓它進 READY_TO_COMMIT。

### iframe 插入時 DevTools 的順序

`DOM.childNodeInserted`（`NotifyNodeInserted` 的 probe，進佇列）→ `Page.frameAttached`（`LocalFrame` 的 constructor，接著 `flush()`，佇列照順序送出）→ `Runtime.executionContextCreated`（main world 的 context 是 lazy 的：第一次碰它的 V8 物件、子文件跑 script，或 `Runtime.enable` 期間強制建）。

### 沒有對到的

- 背景圖的請求在 style recalc 裡發出之後，`ResourceLoadScheduler` 會不會先壓著。
- `about:blank` 同步 commit 時會不會送 `Page.frameNavigated`。
- parent 讀 `iframe.contentWindow` 會同步建子 frame 的 main world context（照 bindings 推的）。

## DOM 的 task 怎麼進 scheduler

- **現況**：`dom` 的佇列照時間和編號排；scheduler（`crates/renderer/src/task_queue.rs` 的 `MainThreadScheduler`）照來源分 task queue，跨 queue 照 post 的編號（和 Chrome 的 enqueue order 同一個概念）。兩者之間的順序是 `run_queued_tasks`（一個 task 排了 DOM 的 task 時，那個 task 之後跑到期的全部，timer 也順便）和外圈的 `run_due_tasks` 決定的。

### 原型

2026-11-24，Linux：Fedora 45、debug build，`0a459d1` 之上。patch 在 `prototypes/`，不合併：`tests.diff`（必壞的測試）、`t1-dom-posts-tasks.diff`、`t2-dom-due-first.diff`，都套在 `0a459d1` 上。

**必壞的測試**（`shell` 的 `event_loop::a_timer_runs_while_responses_keep_coming`）：頁面載完之後，一個 `fetch()` 的回應裡設 `setTimeout(f, 0)`，再接力 40 個 `fetch()`：每個回應的 handler 發下一個、忙 20 ms，所以每個 task 結束時下一個回應已經在通道上。要求 timer 在第 40 個之前跑。

- 沒改：13 次壞 13 次，第 40 個回應時 timer 一次都沒跑（`-1`）。
- Chrome for Testing 152.0.7977.82（同一頁，Python 的 server，`--headless=new`）：5 次，timer 都在第一個回應之前跑。
- 第一版從頁面的 script 直接開始接力，沒改也通過：頁面載入時 DOM 排了 element task，那個 task 之後的 `run_queued_tasks` 順便跑了到期的 timer。改成載完之後、在一個回應裡才開始。

**T2：scheduler 取 task 時問 DOM**（4 個檔，+62 行）：scheduler 的每個 task 記下 post 的時間（`earliest_posted`）；`ScriptEngine::earliest_due_task` 回答 DOM 現在能跑的最早的 task 何時到期（dom_js 的照 `next_task` 換算時鐘）；內圈每個 task 之後，它比通道上最早的早就先 `run_due_tasks`。

**T1：DOM 排 task 時告訴 scheduler**（6 個檔，+107、−4 行）：`dom` 擁有一個 `TaskWaker`（`Box<dyn Fn(Option<Instant>)>`，host 設的）；`stamp_queued_task` 和 timer 的 `add_timer`、interval 重新排時呼叫它；renderer 的 waker 經過 `Arc<MainThreadScheduler>` post 一個 `MainThreadTask::DomTasks` 到新的 `TaskQueue::Dom`，timer 的放進 scheduler 的 delayed 清單，每次 post 和取 task 之前把到期的排進去（編號在那時給）；scheduler 輪到記號時 `run_due_tasks`。在 `make_engine` 設一次。

| | T1 | T2 |
| --- | --- | --- |
| 必壞的測試 | 5 次都通過 | 5 次都通過 |
| 相關的 crate 的整組測試 | 通過（`renderer` 的單元測試的 `kind` 要多一種）；數字在 `progress.md` | 通過；數字在 `progress.md` |
| 兩個 engine | waker 在 `dom`，`make_engine` 一處設好，wasm 的 engine 也是 | 每個 engine 實作 `earliest_due_task`；原型只寫了 dom_js，wasm 的照預設 `None`，等於沒有 |
| 順序 | 到期的 timer 在下一次 post 或取 task 時排進去，接近 Chrome（到期被 scheduler 看到時才拿到 enqueue order） | 照到期時間對 post 時間，比 Chrome 早一點 |
| 每個 task 的成本 | DOM 每排一個 task 多一次 scheduler 的 lock 和 push | 每個 scheduler 的 task 之後多一次 DOM 的查詢（借 engine 的 DOM） |
| 誰決定順序 | scheduler 一個地方 | 內圈的程式比兩邊 |
| 等待 | delayed 清單在 scheduler，之後 `until_next_due` 可以不問 DOM 的 timer | 照舊問 DOM |

兩個原型的簡化：輪到 DOM 的時候都用 `run_due_tasks` 跑完到期的全部，不是一次一個（T1 因此有多出來、什麼都不跑的記號）；T1 沒有替 UA 自己的 timer（`native_timers`）和 idle callback 的 timeout 送記號；都沒有拿掉外圈的 `run_due_tasks`。效能沒有量。

## 參考資料

- RFC 0110 的 `README.md` 的已決定的事項第 6、7 條和「之後的可能」；`details.md` 的盤點（外圈的收尾那幾項）。
- RFC 0019 的已決定的事項第 24、35 條（導覽在 task 結束時送）。
- RFC 0109 的 `details.md` 的 A6（外圈的 intersection 通知）。
