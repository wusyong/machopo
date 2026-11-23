# RFC 0112 的細節：拿掉 event loop 的外圈

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-24 照 `main` 的 `1a4d20e` 的 `RenderThread::run`（`crates/renderer/src/render_thread.rs`）讀的，順序照程式。「Chromium」一欄是**憑記憶**，第 0 階段對原始碼之後改寫；「類」是暫定的：**當場**（Blink 在發生的當下做）、**task**（Blink 排成 frame scheduler 的 task）、**之間**（Chrome 也在 task 之間做：`TaskObserver`、idle period、rendering 之前）、**待查**。

**內圈，每個 scheduler 的 task 之後**：

| 步驟 | 做什麼 | Chromium | 類 |
| --- | --- | --- | --- |
| `run_queued_tasks` | DOM 在那個 task 裡排的 task（例如 fragment 導覽的 `hashchange`） | 各自是 frame scheduler 的 task，照 task queue 輪到 | task |
| `update_title` | 分頁的標題 | `Document::setTitle` 經過 `LocalFrameClient` 當場告訴 content 層 | 當場 |

**外圈，通道空了之後**：

| 步驟 | 做什麼 | Chromium | 類 |
| --- | --- | --- | --- |
| `blink_caret` | caret 的閃爍 | `FrameCaret` 的 timer | task |
| `new_content_rendering_timeout` | 新文件一直畫不出來時清掉舊的畫面 | Browser 的 `RenderWidgetHostImpl` 的 timer | task |
| `intersections_due_now` | intersection observer 的 `delay` 到了，要一個 frame | 憑記憶不確定 | 待查 |
| `fire_due_owner_loads` | 跨 origin 的子 frame 在同一份文件內導覽之後，延後的 iframe 的 `load` | 憑記憶不確定 | 待查 |
| `handle_popup_requests` | `<select>` 的選單 | `ExternalPopupMenu::Show`，當場 | 當場 |
| `handle_frame_requests` | 導覽、blob URL 的登記和撤銷（多數的 engine 呼叫之後也跑） | `LocalFrameClient::BeginNavigation` 當場送 | 當場 |
| `run_child_commits` | 子 frame 的 `srcdoc`、`about:` 文件的 commit，各一個 task | `srcdoc` 經過 Browser 的導覽；初始的 `about:blank` 同步 | task |
| `run_due_tasks` | DOM 到期的 task：timer、element task、message | 各自是 frame scheduler 的 task（timer 是 delayed task） | task |
| `parse_on` | 讓出的 parser 續跑，各跑一段 | `HTMLDocumentParser` 讓出時排 task 續跑 | task |
| `start_image_fetches` | DOM 的圖片清單等著、engine 沒送的（style 找到的背景圖） | style recalc 裡找到、當場送 | 待查 |
| `start_script_fetches` | DOM 排著、engine 沒送的 script（parser 的） | parser 和 preload scanner 當場送 | 待查 |
| `retry_scripts_blocked_by_sheets` | 等擋 script 的 sheet 的 parser 的 script，再試一次 | `StyleEngine` 的計數變了時當場通知 | 當場 |
| `start_canvas_encodes` | `toBlob` 的編碼送到 encoder thread | 憑記憶：`CanvasAsyncBlobCreator` 排 idle task | 待查 |
| `send_webdriver_results` | settle 了的 script 的結果給 WebDriver | CDP 的 `Runtime.evaluate` 在 promise settle 時回 | 待查 |
| `log_console_warnings` | engine 的警告到 console | `probe::` 當場 | 當場 |
| `log_canvas_raster_failure` | canvas 的 raster 失敗 | 憑記憶不確定 | 待查 |
| `blob_urls.flush_revoked` | blob URL 的撤銷告訴 Browser（那一批 task 要的載入之後） | `BlobURLStore` 的 mojo 當場送，導覽帶 `BlobURLToken` | 不在範圍內（另一份） |
| `check_network_idle` | network 安靜夠久的文件 | `IdlenessDetector`：task 的觀察者加 timer | 之間 |
| `tell_frames` | DevTools 的 frame 事件（比對上次） | `probe::` 當場 | 當場 |
| `tell_dom_changes` | DevTools 的 DOM 變動 | `probe::` 當場 | 當場 |
| `start_css_refetches` | DevTools 的 `CSS` 要的 sheet，重新抓 | 憑記憶不確定 | 待查 |
| overlay | DevTools 的 overlay 跟著節點，要一個 frame | rendering 的步驟 | 之間 |
| `run_platform_tasks` | engine 的 platform task（GC 的 incremental marking、finalization）和之後的 checkpoint | V8 和 Oilpan 的 platform task 排在 main thread 的 scheduler | task |
| `run_idle_callback` | `requestIdleCallback` 的一個 callback | scheduler 的 idle period | 之間 |
| `request_next_frame_if_dom_changed` | DOM 變了就要一個 frame | `Document::ScheduleLayoutTreeUpdate` 當場要 | 當場或之間 |
| `clear_request_stacks` | 沒有載入拿走的 script 的 stack | 沒有對應（machopo 自己的記錄） | 之間 |
| `set_needs_begin_main_frame` | 告訴 compositor 要不要 main frame | `ScheduleAnimation` | 之間 |

## DOM 的 task 怎麼進 scheduler

草稿，第 0 階段的原型之後補上量到的。

- **現況**：`dom` 的佇列照時間和編號排（RFC 0110 的 `javascript:` 網址用 element task，和另開 networking 的佇列順序一樣，就是因為這個）；scheduler（`crates/renderer/src/task_queue.rs` 的 `MainThreadScheduler`）照來源分 task queue（Browser 的訊息、rendering、input……）。兩者之間的順序是 `run_queued_tasks` 和外圈的 `run_due_tasks` 決定的。
- **T1** 的成本：每個 DOM 的 task 多一個記號經過 scheduler（一次 `Arc` 裡的 lock 和 push），取消的要兩邊都處理；timer 交給 scheduler 的 delayed queue 之後，`until_next_due` 不用再問 DOM 的 timer。
- **T2** 的成本：每次取 task 多問一次 DOM 的佇列的頭（借 engine 的 DOM），不用兩份；但 scheduler 在別的 thread 也被呼叫（IO thread 排 task），問 DOM 只能在 main thread 取 task 的時候。
- 兩者都要回答：DOM 的 task 放進 scheduler 的哪一個 task queue（照 HTML 的 task source 對 Chrome 的 `TaskType`），和其他 queue 怎麼排先後。

## 參考資料

- RFC 0110 的 `README.md` 的已決定的事項第 6、7 條和「之後的可能」；`details.md` 的盤點（外圈的收尾那幾項）。
- RFC 0019 的已決定的事項第 24、35 條（導覽在 task 結束時送）。
- RFC 0109 的 `details.md` 的 A6（外圈的 intersection 通知）。
