# RFC 0124 的細節：跨網站的導覽換 renderer

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-27 讀的（`main`，`acdd0704`）。檔案沒有另外寫的都在 `crates/browser/src/`。

### 一個分頁和它的 renderer

- **`WebContents` 擁有一個 `RenderProcessHost`**（`process`），在 `WebContents::new` 裡、任何導覽之前就啟動；怎麼啟動留在 `Launch`（旗標、`RendererLauncher`、GPU、訊息的出口）。
- **只有一個地方換掉它：`relaunch`**，renderer 不在了（`crashed`）之後的下一次導覽從 `begin_navigation` 叫它。它做的事：
  1. 取新的 `FrameSinkId`（client id 是分頁的，sink id 往下數）；
  2. 啟動新的 `RenderProcessHost`，舊的 `abandon`（process 直接結束、不等；thread 放著自己結束）；
  3. 告訴 Viz 舊的 sink 作廢（`InvalidateFrameSinkId`）、登記新的（`RegisterFrameSinkId`）：舊的畫面當場不見，到新的 renderer 畫出第一個 frame 之前是空的；
  4. `entries_heard` 歸零（renderer 自己數的 session history 的新 entry）；
  5. 把分頁的狀態再送一次：`DevToolsMainFrameToken`、`UpdateVisualProperties`、`UpdateScreenRects`、`WasHidden`、`SetFocus`。
- 這是現在最接近「換 process」的路。不一樣的地方：那時舊的 renderer 已經不在（沒有 `unload` 要跑、沒有畫面要留），frame 樹已經清成只剩 main frame，DevTools 的 session 是靠 `crashed` 變回 `false` 才重新接上的（下面）。
- **renderer 送來的訊息帶著是哪個 renderer**（`RendererId`）：嵌入的人（`shell` 的 `browser_view.rs`、`headless.rs`）丟掉不是目前那個的（`is_current_renderer`）；被結束或放掉的 renderer 的 IO thread 不再往上轉（`forwarding`）。所以「同一個分頁先後有好幾個 renderer」這件事，訊息這一層已經照著寫了。

### 導覽

- **文件由 Browser 載入**（Chromium 的 PlzNavigate）：`begin_navigation` → `start_navigation` 建 `NavigationRequest`（掛在 frame 樹上）、交給 network service 載入（`AtRedirects::Wait`）；redirect（`on_navigation_redirected`）只記下那一跳、繼續；有 response 的 head（`on_navigation_response_started`）之後讀 body 的開頭，再 `commit_response` → `commit` → `Committer::commit` 送出 `RendererMsg::CommitNavigation`（body 的開頭加一條 data pipe）。
- **一次導覽在開始時就綁到 renderer**：`start_navigation` 向 `self.process` 拿 `Committer`，它裡面是那個 renderer 的 `RendererSender`。要換 process 的話，「送到哪個 renderer」要能在 commit 之前重新決定。
- `commit` 另外做三件跟著 process 的事：這個 process 能不能讀本機的檔案（`set_file_access`，main frame 的新文件決定）、替這份文件登記 loader（`register_loader`，子資源的請求照它檢查）、在 frame 樹上記「正在 commit 這一筆」。
- 不載入東西的導覽當場 commit（`about_document`）：`about:blank` 是空的文件、origin 跟著發起者（Browser 自己發起的是 opaque）；`about:srcdoc`（只有子 frame）；main frame 其他內建的 `about:` 頁，沒有 origin。`blob:` 的網址由 Browser 整個讀出來再 commit。載入失敗 commit 錯誤頁（`CommittedDocument::error_page`，opaque 的 origin）。
- **renderer 發起的導覽**（`BeginNavigation`，帶 `begin_id`）要等發起它的 task 結束才 commit（`NavigationCancellationWindowEnded`；Chromium 的 `RendererCancellationThrottle`）；被別的導覽取代時告訴 renderer（`dropped`）。
- renderer 回報 commit（`DidCommitNavigation`）：frame 樹對過是不是 Browser 送去的那一筆（不是的話是壞訊息），session history 記下，再把 history 的位置和長度告訴 renderer。
- 導覽沒有 body（沒有 `POST` 的表單導覽）。

### session history

- **全部在 Browser**：`NavigationController` 的 entry，每個 frame 一個 `FrameNavigationEntry`（網址、文件的序號、`history.state` 序列化的結果、scroll restoration 的模式）；commit 時用 `HistoryEntryParams` 帶給 renderer。
- 不恢復捲動位置和表單的內容，沒有 back-forward cache（`docs/limitations.md`）。所以上一頁、下一頁換到新的 renderer 時，它拿到的和現在同一個 renderer 裡重新載入拿到的一樣，不會少東西。

### 跟著分頁的（換 renderer 時留著）

- frame 樹的 root 的 token：分頁建立時取的亂數，也是 DevTools 的 target id 和 main frame 的 id，每個 renderer 一開始就被告知（`DevToolsMainFrameToken`）。
- `sessionStorage` 照分頁（`forget_namespace(tab)` 在關分頁時）；`localStorage`、cookie、HTTP cache 在 profile 的 `StoragePartition`。
- 大小、device scale、`LocalSurfaceId`、視窗在螢幕上的位置、可見、focus：`WebContents` 留著，`relaunch` 重送。
- title、顯示的網址、導覽的編號、session history。

### 跟著 renderer 的（換的時候沒了）

- IO thread 那一側（`IoThreadHost`，每個 renderer 一個）：loader 的表、還在路上的子資源（通道關掉時取消）、worker、它登記的 blob URL（通道關掉時 `revoke_blob_urls_of`）、它開的 storage area（`dom_storage.forget`）。
- 這個 process 能不能讀本機的檔案（`file_access`）。
- frame sink（每個 renderer 一個）；有 GPU 時啟動就給的 tile 用的共用記憶體（16 塊、每塊 256 KiB）；GPU 遺失時通知它的 observer。
- renderer 裡的：V8 的 isolate、名稱表、圖片的快取、DevTools 各個 agent 的狀態（開了哪些 domain、中斷點、overlay）。
- 還沒 ack 的 input 的數量（`input_in_flight`）。

### `unload`

- `pagehide`、`unload` 在**同一個 renderer 裡、下一份文件 commit 時**分派（`renderer` 的 `render_thread/documents.rs` 叫 `unload_document`）。沒有 `beforeunload`。
- renderer 收到 `Shutdown`（關分頁、machopo 結束）只是離開 event loop，**不分派 `pagehide`、`unload`**。
- 所以新的文件在別的 process commit 時，舊的 renderer 沒有人叫它跑 `unload`：要一個新的訊息。

### 畫面：paint holding

- `renderer` 的 `render_thread.rs`（`new_content_rendering_delay`、`start_new_content_rendering_timeout`、`new_content_rendering_timeout`）：main frame 的文件 commit 時開始計時（`RendererFlags::new_content_rendering_delay`，`NEW_CONTENT_RENDERING_DELAY` 是 4 秒，Blink 的 `kNewContentRenderingDelay`）；文件被擋住（`render_blocked`：body 還沒開始、stylesheet 擋著）時不交新的 frame，畫面上留著舊的文件；時間到還被擋住，交一個只有背景的 frame（`render_thread/documents.rs` 的 `commit_empty_frame`，Chromium 的 `RenderWidgetHostImpl::ClearDisplayedGraphics`）。測試：`browser` 的 `navigation.rs` 的 `a_new_document_that_cant_be_drawn_shows_empty_after_the_delay`。
- 不看新舊是不是同一個 origin、舊頁面有沒有被使用者操作過。
- renderer 記著 sticky user activation（`dom` 的 `window.rs` 的 `has_sticky_activation`），Browser 不知道。
- Viz 留著一個 sink 的 frame 和它的 shared image，直到 Browser 叫它作廢那個 sink（`VizMsg::InvalidateFrameSinkId`，`display.rs` 的 `remove_frame_sink`），renderer 的 process 結束了也一樣。嵌入的 surface 沒有 frame 時畫背景色（`surface_aggregator.rs`）；沒有「先畫另一個」的 fallback。

### 嵌入的人

- **視窗和沒有平台視窗的視窗**（`shell` 的 `browser_view.rs`）：每個 frame 問 `contents.surface_id()` 再嵌進 UI 的 frame，分頁換了 sink 會跟著；分頁建立時向 Viz 登記 sink（`browser_window.rs`），`relaunch` 自己登記新的。
- **`headless.rs`**（reftest 走的路）：`Display` 的 root 和 `BeginFrame` 的對象是分頁**一開始**的 sink，不會跟著換（現在 renderer 不在了它就以錯誤結束，所以沒遇過）。
- **自動化的 session**（`shell` 的 `automation_session.rs`）：DevTools 的 session 照分頁記；它每一輪看 `is_crashed`，從 `true` 變回 `false` 時對新的 renderer 重新 `devtools_attach`，並重送 `Page.enable` 和 lifecycle 的事件，其他的 domain 等 client 自己再開。renderer 不在了時送 `Target.targetCrashed`。
- WebDriver（`crates/driver`）經過自動化的 pipe；量測工具（`scripts/bench`）照 process 和 trace 找 renderer。

### 啟動的方式

- `RendererLauncher::Process`（預設）、`InProcess`（`--single-process`；`cargo test` 裡直接建的分頁和沒有平台視窗的情境）、`Detached`（測試和 fuzz 直接餵訊息，另一端沒有東西）。
- renderer process 的訊息由 Browser 的一條 IO thread 等；同一個 process 裡的 renderer 各有一條。

### site 怎麼算

- `site` crate 的 `Site`：scheme 加 registrable domain（沒有的話是 host：IP、`localhost`），不管 port；opaque 的 origin 只和自己同一個 site。`Site::of_url` 取網址的 origin 再算。
- `file:` 的網址在 `url` crate 裡每次問都是一個新的 opaque origin，所以兩個 `file:` 的網址照 `Site` 比是不同的 site：「`file:` 算不算同一個 site」要另外定。

### 測試現在怎麼跑

- `cargo test` 裡的整合測試和沒有平台視窗的情境：`InProcess`，renderer 是測試的 process 裡的 thread。
- WPT（經過 machopodriver）、`--window` 的情境、量測工具：真的 `machopo`，每個分頁一個 renderer process。
- 可以把別的 host 對到本機（`--host-resolver-rules`、`net::HostMappingRules`），`shell` 的測試已經用它做跨網站的頁面（`documents.rs`、`devtools.rs`、`webdriver.rs`）。

## DevTools 的 session 的狀態（第 2 段開始時的盤點）

2026-11-27 讀的（`0124`，`d07e93e8`）。

- **Browser 這一側**（`shell` 的 `automation_session.rs` 的 `Attached`）每個 session 記五個旗標：`Page.enable`、`Page.setLifecycleEventsEnabled`、`Network.enable`、`Network.setCacheDisabled`、`Log.enable`。換了 renderer 只重送前兩個（回答丟掉）；後三個 Browser 照樣當成開著，新的 renderer 裡卻沒有。其他的命令 Browser 不看、原樣轉給 renderer。
- **renderer 的 agent**（`renderer` 的 `render_thread/`），每個 session 的狀態：
  - Page（`page_agent.rs`）：開著沒有、lifecycle 的事件要不要；打開 lifecycle 時重播到目前為止的事件。
  - Network（`network_agent.rs`）：`Network.enable` 的三個 buffer 的上限、`setAttachDebugStack`、`setCacheDisabled`，和留著的 response body。
  - Log（`log_agent.rs`）：開著沒有；打開時重播留著的 entry。
  - DOM（`dom_agent.rs`）：開著沒有、`includeWhitespace`；node 的編號、搜尋的結果、`DOM.setInspectedNode`（存在 V8 的 session 裡）。
  - CSS（`css_agent.rs`）：開著沒有（要 DOM 開著）、`trackComputedStyleUpdatesForNode`、`forcePseudoState`（也寫進 DOM）、告訴過它的 sheet。
  - Overlay（`overlay_agent.rs`）：開著沒有、`setInspectMode`、`setPausedInDebuggerMessage`。
- **V8 的 inspector**：每個 session 的狀態在 V8 自己的 `V8InspectorSessionImpl::m_state`：Runtime 的 `enable`、custom formatter、`setMaxCallStackSizeToCapture`、`addBinding`；Debugger 的 `enable`、`setBreakpointsActive`、`setSkipAllPauses`、`setPauseOnExceptions`、`setAsyncCallStackDepth`、`setBlackboxPatterns`、instrumentation breakpoint；Profiler、HeapProfiler、Console 的 `enable` 和設定。`setBreakpointByUrl` 的斷點不在 `m_state`，在 `m_urlBreakpoints`（V8 15.2：只能經過 `V8EmbedderState` 還原）。
  - V8 能存下和還原它：`V8InspectorSession::state()` 給 CBOR，`connect` 給它非空的狀態時各個 agent `restore()`。但 rusty_v8 的 fork 沒有把 `state()` 接出來（`src/inspector.rs`、`src/binding.cc` 都沒有），machopo 的 `bindings` 的 `devtools.rs` 一律給空的狀態（換 engine 時也是，所以 JS 和 wasm 的 engine 互換時 V8 的狀態現在就會不見）。
- **編號**：DOM 的 `nodeId`、V8 的 `objectId`、`scriptId`、execution context 的 id 都是那份文件、那個 renderer 的，換了文件本來就不能用；CSS 的 `styleSheetId` 和 Network 的 `requestId` 帶著 renderer 的前綴。main frame 的 frame id 是 Browser 取的，每個 renderer 都一樣。
- **machopodriver** 只開 `Page.enable` 和 `Page.setLifecycleEventsEnabled`（`crates/driver` 的 `session.rs`），正好是 Browser 現在重送的；其餘的經過 `Machopo.*`，不靠 session 的狀態。

## 量的：啟動一個 renderer process 要多久

2026-11-27，`acdd0704`，release（`cargo build --release`），Linux（Fedora 45、AMD Radeon 890M 的那台），sandbox 開著（Landlock、seccomp、user 和 network namespace），`machopo --remote-debugging-pipe --headless`。每種 30 次、跑兩輪，從 CDP 的命令送出到 `Runtime.evaluate` 回答為止：

| | 中位數 | 最小 | 最大 |
| --- | --- | --- | --- |
| 新的分頁（`Target.createTarget`：啟動 renderer process、commit `about:blank`、跑一句 script） | 12.3、12.4 ms | 11.4 | 13.8 |
| 已經有 renderer 的分頁導覽到 `about:blank`（`Page.navigate`，再跑一句 script） | 1.3、1.4 ms | 1.2 | 2.7 |

- 差的約 **11 ms** 就是多啟動一個 renderer process 的時間（spawn、進 sandbox、V8 的 isolate、IO thread 接上通道、有 GPU 時給 tile 的共用記憶體）。確認過兩個分頁時 Browser 底下是兩個 `machopo --type=renderer`。
- **macOS**（2026-11-28，`a2f67b3a`，macOS 26.6.2、Apple M4，release、sandbox 開著，同樣的量法）：新的分頁 12.8、12.7 ms（最小 12.3、最大 13.5），已經有 renderer 的分頁導覽到 `about:blank` 0.5、0.4 ms，差約 **12 ms**，和 Linux 差不多，不用重新考慮提前啟動和備一個（`README.md` 的第 5、9 條）。另外量了換 renderer 本身：同一個分頁每次導覽到新的 `data:` 網址（每個是一個 site）12.9、13.0 ms，就是啟動一個 renderer 加上一次導覽（每輪第一次 1.5 ms：`about:blank` 的 renderer 還沒有 site，拿下第一個 `data:`、不用換）。兩輪共結束 122 個 renderer，正好是 31 個分頁加 30 次換的兩倍。
- Windows 沒量（Windows 的 process 建立通常比較慢）。debug build 沒有量：連結的是 release 的 V8，其餘的慢不代表使用者看到的。
- 腳本是當場寫的、沒有留在 repo（用 `scripts/bench/common.py` 的 `MachopoPipe`）。

## Chromium

2026-11-27 讀的原始碼：Chromium 的 `main`（開始讀時的 HEAD 是 `4b06ada48a096089230c6bcc50c4e9a5996fc703`，檔案是之後幾分鐘內抓的，沒有釘在那個 commit）。路徑沒有另外寫的都在 `content/browser/renderer_host/`；引文照原文。預設值都是原始碼裡 `BASE_FEATURE` 的預設，Chrome 出貨時用 field trial 改過的沒有查。

### 換不換：`RenderFrameHostManager`

- 入口是 `render_frame_host_manager.cc` 的 `GetFrameHostForNavigation`：算出這次導覽的 `SiteInstance`，「The SiteInstance determines whether to switch RenderFrameHost or not.」process 是那個 `SiteInstance` 的。
- `GetSiteInstanceForNavigation` 分兩步：先 `ShouldSwapBrowsingInstancesForNavigation`（要不要連 `BrowsingInstance` 一起換），再 `DetermineSiteInstanceForURL`。後者的規則照順序：
  1. 錯誤頁（「these must be the first checks」）；
  2. session history 的 entry 存著的 `SiteInstance`（「If the entry has an instance already we should usually use it, unless it is no longer suitable.」）；
  3. 決定了要換 `BrowsingInstance`：一個沒有關聯的新 `SiteInstance`；
  4. 發起者的 `SiteInstance`（「such as for about:blank and sometimes data: URLs」）；
  5. 目前的 `SiteInstance` 還沒用過：「If we haven't used our SiteInstance yet, then we can use it for this navigation. We won't commit the SiteInstance to this site until the response is received (in OnResponseStarted).」
  6. 同一個 site：「Use the current SiteInstance for same site navigations.」
  7. opener 的（main frame 只有這一條）；
  8. 其餘：同一個 `BrowsingInstance` 裡、那個 site 的 `SiteInstance`（`browsing_instance.cc` 的 `GetSiteInstanceForURL`：每個 `BrowsingInstance` 裡一個 site 一個）。
- **同一個 site** 的比法（`site_instance_impl.cc` 的 `IsSameSite`）：scheme 不同就不是；再比 registrable domain；不管 port（「because pages served from different ports can still access each other if they change their document.domain variable」）。目的地是空白頁時當成同一個 site。
- **連 `BrowsingInstance` 一起換**的情況（只有 main frame）：COOP；安全的理由（WebUI 和一般網頁之間、`StoragePartition` 不同……）；history 的 entry 在別的 `BrowsingInstance`；Browser 發起的跨網站導覽（網址列輸入、書籤、搜尋；`IsBrowsingInstanceSwapAllowedForPageTransition`，`file:` 和 `data:` 除外：「Skip data: and file: URLs, as some tests rely on browser-initiated navigations to those URLs to stay in the same process.」）；為了 back-forward cache 的「proactive swap」（桌面版預設開，條件之一是沒有別的視窗能 script 到它）。machopo 沒有 `window.open`、沒有 back-forward cache，一個分頁就是一個 group，這一層現在看不出差別。

### 各種網址（main frame、桌面版的預設）

| 導覽 | Chromium |
| --- | --- |
| 跨網站的 http(s) | 換：新的 `SiteInstance`、鎖到那個 site 的另一個 process |
| 同一個 site | 同一個 process（`RenderFrameHost` 照樣換新的：RenderDocument 預設對所有 frame 開） |
| 新分頁的第一次導覽 | 不換：初始的 `SiteInstance` 還沒有 site，用它和它的 process，response 到了才定它的 site（`NavigationRequest::SelectFrameHostForOnResponseStarted`） |
| 頁面發起的 `about:blank` | 留在發起者的 `SiteInstance`（`CanUseSourceSiteInstance`：「the content is then controlled and/or scriptable by the initiator」） |
| 網址列輸入的 `about:blank` | 留在目前的 process（`IsSuitableForUrlInfo`：「Any process can host an about:blank URL, except the one used for error pages」） |
| `data:` | 頁面不能把 main frame 導覽到 `data:`；網址列輸入的每一個自己一個 process（`site_info.cc`：「each browser-initiated data: URL will get its own process」） |
| `blob:` | 照裡面的 origin 的 site；`blob:null/…` 每個網址自己一個 |
| `file:` | 所有的 `file:` 是同一個 site（`site_info.cc`：`GURL(origin.scheme() + ":")`） |
| 錯誤頁（main frame） | 自己的 process，所有的錯誤頁共用一個（`ComputeErrorPageProcess` 的 `kIsolatedProcess`；`SiteInfo::ShouldUseProcessPerSite`：「Error pages should use process-per-site model」）；伺服器自己給的 4xx、5xx 頁面是一般的文件 |
| 重新整理 | 同一個 `SiteInstance` 和 process（entry 存著的） |
| 上一頁、下一頁 | 照 entry 存著的 `SiteInstance`（`frame_navigation_entry.h`：存的是 `SiteInstance`，不保證 process 還在） |
| renderer 當掉之後 | 一定用新的 `RenderFrameHost`，而且在開始時就換上（「There's no reason to sit around with a sad tab ... while we wait for the navigation to complete.」），不留舊的畫面 |

### 錯誤頁的能力（Chrome）

- Chrome 的錯誤頁裡的 script 有一個一般網頁沒有的物件（`chrome/renderer/net/net_error_page_controller.cc` 的 `NetErrorPageController`）：重新載入、顯示細節、診斷錯誤、captive portal 的登入、存下頁面晚點再看、離線小遊戲的最高分（存進使用者同步的設定）。它由 `NetErrorHelper::EnablePageHelperFunctions` 裝在顯示錯誤頁的 frame 上。
- 到 Browser 的那一段是那個 frame 自己的 interface（`chrome/common/network_diagnostics.mojom` 的 `NetworkDiagnostics::RunNetworkDiagnostics(failed_url)`，經過 `render_frame()->GetRemoteAssociatedInterfaces()`）：綁在 frame 上，不是綁在「錯誤頁的 process」上。子 frame 的錯誤頁不隔離（Chromium 的文件 process_model_and_site_isolation.md：「Currently this only applies to error pages in a main frame.」），隔離與否由 `ContentBrowserClient::ShouldIsolateErrorPage` 決定。
- **為什麼隔離，文件沒有寫**：它只說「Chromium uses a special type of process for error pages provided by the browser ... using process-per-site mode to keep all such pages in the same process」。「因為錯誤頁有特別的能力」是推測。程式裡寫了的是為什麼**共用一個**（`site_info.cc` 的 `ShouldUseProcessPerSite`）：「it is useful to consolidate them to minimize resource usage and there is no security drawback to combining them all in the same process.」

### Browser 自己的頁面（`chrome://` 的 WebUI）

- **每個 host 是自己的一個 site**：`site_info.cc` 的 `GetSiteForOrigin` 取 scheme 加 registrable domain、沒有時取 host，所以 `chrome://settings/` 的 site 是 `chrome://settings`，`chrome://history/` 是另一個。
- **一定要自己的 process**（`site_info.cc`）：「Isolate WebUI pages from one another and from other kinds of schemes.」；而且鎖住：「Most WebUI processes should be locked on all platforms. The only exception is NTP」。
- **和網頁之間、不同的 WebUI 之間一定換**（`render_frame_host_manager.cc` 的 `ShouldSwapBrowsingInstancesForNavigation`）：「For security, we should transition between processes when one is a Web UI page and one isn't, or if the WebUI types differ.」連 `BrowsingInstance` 一起換。唯一的例外是從還沒用過的初始 renderer 出發：「in that case the WebUI navigation can safely reuse them」。
- **可以明講要共用**（`content/public/browser/webui_config.h`）：`WebUIConfig` 的 `process_lock_host`（例如 `"top-chrome"`），同一個值的幾個 WebUI 共用一個 process、各自的 site 不變；註解寫明代價：「WebUIs with the same `process_lock_host` are not process-isolated from each other.」預設是不共用（「locks the renderer process to this WebUI's own site URL」）。
- **`about:` 的網址就是 `chrome://` 的**：使用者輸入的 `about:<名字>` 在導覽之前被改寫成 `chrome://<名字>`（`components/url_formatter/url_fixer.cc`：「Replace the about: scheme with the chrome: scheme.」），只有 `about:blank` 和 `about:srcdoc` 留著（「'about:blank' and 'about:srcdoc' are special-cased」）；`chrome://about` 再改成 `chrome://chrome-urls`（`chrome/browser/browser_about_handler.cc`）。頁面發起的其他 `about:` 被擋掉（同一個檔案的註解：「blocked (by content::RenderProcessHostImpl::FilterURL applied to renderer-initiated navigations)」；`FilterURL` 本身沒有讀）。只做事、不導覽的有兩個（`chrome://restart`、`chrome://quit`）。
- 沒有查：Chrome 的 `GetWebUIType` 怎麼分「種類」（照 host，還是照 controller 的類別）。

### 什麼時候決定：兩次

- 「GetFrameHostForNavigation will be called more than once during a navigation (currently twice, on request and when it's about to commit in the renderer).」
- **開始時**（網路請求之前，Browser 發起的還在 `beforeunload` 之前）：建 speculative 的 `RenderFrameHost`，它的 process 這時就啟動（`CreateSpeculativeRenderFrameHost` 叫 `GetOrCreateProcess(...)->Init()`），在新的 process 裡把 view 和 frame 建好、藏著。
- **每一次 redirect**（`NavigationRequest::OnRequestRedirected`）：只重算 `SiteInstance`，不換掉 speculative 的 `RenderFrameHost`。
- **response 到了**（`SelectFrameHostForOnResponseStarted`）：用最後的網址和 header 再決定一次；和開始時猜的不一樣，就丟掉 speculative 的、另建一個（有統計浪費了幾次：`WastedSpeculativeRFHCase`）。
- 一個關掉的實驗 `kDeferSpeculativeRFHCreation`：「The creation of a speculative RFH consumes about 2ms and is blocking the network request.」

### 換的過程

1. 開始到新的文件 commit 之前：舊的 `RenderFrameHost` 還是目前的，舊的頁面留在畫面上、照常能用。
2. `CommitNavigation` 送給新 process 的那個 `RenderFrameHost`，帶著 response 的 head 和 body 的 pipe。
3. 新的 renderer 回報 commit（`DidCommit`）時才換（`CommitPending`）：換上新的 `RenderFrameHost`、藏起舊的 view、focus 移過去、「Make the new view show the contents of old view until it has something useful to show.」（`TakeFallbackContentFrom`）、然後 `UnloadOldFrame`。
4. **舊的文件的 `unload`**：`RenderFrameHostImpl::Unload` 送 `Unload` 給舊的 renderer、開始計時，`kUnloadTimeout = base::Milliseconds(500)`；舊的 renderer 回 ack（`OnUnloadACK`）或時間到，舊的 `RenderFrameHost` 就刪掉。**新的文件的 commit 不等舊的 `unload`**（Chromium 的文件 navigation.md：「any unload handlers are executed in the previous document's process concurrently with the creation of the new document in the new process」）。
5. **舊的 process**：它的最後一個 `RenderFrameHost` 刪掉時跟著收掉（`RenderProcessHostImpl::Cleanup`），也就是 ack 到了或 500 ms 之後；worker、keepalive 的請求、back-forward cache 會讓它留久一點。

### 畫面

- commit 之後、新的 renderer 畫出第一個 frame 之前，新的 view 可以先顯示舊頁面的 surface（paint holding）。main frame 只在這幾種情況才留（`navigator.cc` 的 `allow_main_frame_paint_holding`）：初始的空白文件、新舊同一個 origin、或舊的頁面有過使用者的操作（`HasStickyUserActivation`）。所以**跨 origin 的導覽只有在使用者操作過舊頁面時才留舊的畫面**。
- 最多留 4 秒（`third_party/blink/common/widget/constants.cc` 的 `kNewContentRenderingDelay`），之後清掉。留著的期間新頁面不收 input。
- **為什麼限制**（`navigator.cc` 的註解：「Commonly used attacks are done without user activation -- which will not enable paint holding. However, if the user interacts with the page, we treat it as a valid case for paint holding.」「See https://issues.chromium.org/40942531 for reasons we limit paint holding.」）：issue 40942531，「Security: Origin spoof caused by navigation that doesn't paint any content」（Chrome 119）：導覽 commit 之後什麼都沒畫時，畫面照樣顯示前一頁 4 秒，「even if it's a cross-origin cross-site page」；攻擊的頁面讓分頁導覽到受害的網站、讓它畫不出東西，網址列是受害的網站、畫面是攻擊者的，重複導覽能延長。讀了報告的本文，沒有讀留言。

### `beforeunload`

- Browser 發起的導覽：先建好 speculative 的 `RenderFrameHost`，再向舊的 renderer 分派 `beforeunload`，回答了才開始網路請求；renderer 發起的：那個 frame 自己在送 `BeginNavigation` 之前跑過了。
- 逾時也是 `kUnloadTimeout`（500 ms），時間到當成同意。預設開的兩條快的路：沒有 handler 的不問（`kAvoidUnnecessaryBeforeUnloadCheckSync`）；使用者沒有操作過那個 frame 時邊導覽邊跑、只有 commit 等它（`kAsyncBeforeUnload`）。

### 預備的 renderer 和重用

- **spare renderer**（`spare_render_process_host_manager_impl.cc`）：完整的 site isolation 時一直備著一個啟動好、還沒鎖的 process（`kSpareRendererForSitePerProcess` 預設開），被拿走之後再備一個；記憶體有壓力或 process 數到上限時不備。文件只說「This avoids the need to wait for a new process to start.」，沒有寫省多少。
- **同一個 site 的兩個分頁**預設不共用 process（`ProcessReusePolicy::kDefault`：「SiteInstances don't proactively reuse processes」），超過軟性的上限才開始重用同一個 site 的（上限是實體記憶體的一半除以每個分頁估的 85 MB，夾在 3 和平台的上限之間：16 GB 是 96 個）。
- **例外：預設的搜尋引擎**（2026-11-28 讀的，`main`）：content 這一層有 `kProcessPerSiteUpToMainFrameThreshold`（`content_features.cc`，Android 以外預設開）：新的 main frame 的 `SiteInstance` 重用同一個 site 已經在用的 process（`kReusePendingOrCommittedSiteWithMainFrameThreshold`，`render_frame_host_manager.cc` 的 `UpdateProcessReusePolicyForMainFrame`），一個 process 最多 2 個 main frame（`kProcessPerSiteMainFrameThreshold`），那個 process 有 DevTools 接著、或估的記憶體不夠時不重用（`render_process_host_impl.cc` 的 `IsBelowReuseResourceThresholds`；註解：DevTools 在一個 renderer 有好幾個 main frame 時不好用）。content 的預設是每個網站都這樣（`ContentBrowserClient::ShouldAllowProcessPerSiteForMultipleMainFrames` 回 `true`），但要 embedder 對網址說可以（`ShouldReuseAnyExistingProcessForNewMainFrameSiteInstance`，content 的預設回 `false`）。Chrome 只對預設的搜尋引擎說可以（`kProcessPerSiteForDSE`，Android 以外預設開；`prerender_utils.cc`），而且受管理的（enterprise）使用者不用（`kProcessPerSiteSkipEnterpriseUsers` 預設開）。所以 Chrome 裡兩個不相干、同一個 site 的分頁，只有預設的搜尋引擎（例如兩個 Google 搜尋的分頁）會共用 process，其他的網站不會。

### 跨過換 process 留著的

- `FrameTreeNode` 不變；`RenderFrameHost`、`RenderViewHost`、`RenderWidgetHost`、view、`FrameSinkId` 都是新的（`FrameSinkId` 是 process 的編號加 routing id）。大小在換上時送（`WebContentsImpl::NotifySwappedFromRenderManager`），focus 和可見在 `CommitPending` 裡。
- session history 在 Browser（`FrameNavigationEntry` 的 `page_state_`），commit 時用 `CommitNavigationParams.page_state` 給新的 renderer。
- `sessionStorage` 的 namespace 是分頁的 `NavigationController` 的，每個新的 view 都被告知。
- **DevTools**：agent host 照 `FrameTreeNode`；`devtools_frame_token` 建 speculative 的 `RenderFrameHost` 時傳下去，所以 target id 和 main frame 的 id 不變。session 的交接：導覽開始時暫停往 agent 送（`SuspendSendingMessagesToAgent`），`ReadyToCommitNavigation`（commit 的訊息之前）對新的 process 重新 attach，導覽結束再繼續送。**session 的狀態**靠一份 cookie：renderer 的 agent 在每個回答和事件裡附上狀態的變動，Browser 存著，重新 attach 時交給新的 agent（`devtools_agent.mojom`：「cross-process navigation in a frame creates a new DevToolsAgent (in a different process), but we can preserve the state of debugging session by copying it from one agent to another」）。暫停期間會跑 script 的命令以 target 關閉的錯誤結束，其餘的之後重送。

### renderer 在導覽的途中不在了

2026-11-27 讀的（同一份 `main`）。

- **current 的 `RenderFrameHost` 的 process 不在了**（`render_frame_host_impl.cc` 的 `RenderFrameHostImpl::RenderProcessGone`）：「The renderer process is gone, so this frame can no longer be loading.」`ResetOwnedNavigationRequests(NavigationDiscardReason::kRenderProcessGone)` 丟掉**這個 `RenderFrameHost` 擁有的**導覽，也就是已經交給它、等它 commit 的（`ResetOwnedNavigationRequests` 的註解：pending commit 的導覽只在「the FrameTreeNode or renderer process is gone/will be gone soon」時取消）；子 frame 清掉（`ResetChildren`）；標成 `must_be_replaced_for_crash_`。
- **還在載入的導覽**歸 `FrameTreeNode` 擁有，`RenderProcessGone` 不碰它們。commit 時 current 的 `RenderFrameHost` 一定要換（`must_be_replaced_for_crash`），所以在新的 process commit。
- **頁面發起的導覽**跟著它的 renderer 結束：導覽開著一條通往發起它的 renderer 的 `NavigationClient`（`request_navigation_client_`），renderer 不在了這條通道就斷，`NavigationRequest::OnNavigationClientDisconnected` 把沒有說原因的斷線當成取消（`kNoExplicitReason` → `kInternalCancellation`），還沒到 READY_TO_COMMIT 的就 `CancelNavigation`。
- 沒有逐行追：Browser 發起、還在載入的導覽在 response 到了時確實走到新的 process（從 `must_be_replaced_for_crash` 和上面的「early swap」推的）；`history.back()` 這種頁面發起的 history 導覽有沒有那條 `NavigationClient`。

### 沒有 site isolation 的模式

記憶體小的 Android：頁面發起的跨網站導覽留在同一個 process；Browser 發起的跨網站導覽（網址列、書籤）照樣換 `BrowsingInstance`、換 process。這條規則不依賴 site isolation。

### 沒有查證的

- 舊的 renderer 收到 `Unload` 之後 Blink 分派哪些事件、什麼順序（只讀了 Browser 這一側）。
- paint holding 在 renderer 和 cc 那一側的部分；沒有留舊畫面時實際畫的是什麼。
- 預設值在出貨的 Chrome 裡有沒有被 field trial 改過；spare renderer 省下多少時間。
- 文件和程式不一致的三處（以程式為準）：Chromium 的文件 render_document.md 說 `RenderFrameHost` 的交換是有條件的（程式裡預設對所有 frame 開）；`IsBrowsingInstanceSwapAllowedForPageTransition` 的註解說所有 `data:` 的 site 是 `data:`（`site_info.cc` 給每個一個 nonce）；Chromium 的文件 process_model_and_site_isolation.md 說沒有 site isolation 時 Browser 發起的交換要「no other pages in the browsing context group」（那條規則沒有這個檢查）。

## 參考資料

- Chromium 的原始碼（`main`，2026-11-27）：`content/browser/renderer_host/` 的 `render_frame_host_manager.cc`、`navigation_request.cc`、`navigator.cc`、`render_frame_host_impl.cc`、`render_process_host_impl.cc`、`spare_render_process_host_manager_impl.cc`、`frame_navigation_entry.h`；`content/browser/` 的 `site_instance_impl.cc`、`site_info.cc`、`browsing_instance.cc`、`process_reuse_policy.h`、`devtools/render_frame_devtools_agent_host.cc`、`devtools/devtools_session.cc`；`third_party/blink/public/mojom/devtools/devtools_agent.mojom`。
- Chromium 的文件：[process_model_and_site_isolation.md](https://chromium.googlesource.com/chromium/src/+/main/docs/process_model_and_site_isolation.md)、[navigation.md](https://chromium.googlesource.com/chromium/src/+/main/docs/navigation.md)、[navigation_concepts.md](https://chromium.googlesource.com/chromium/src/+/main/docs/navigation_concepts.md)、[session_history.md](https://chromium.googlesource.com/chromium/src/+/main/docs/session_history.md)、[render_document.md](https://chromium.googlesource.com/chromium/src/+/main/docs/render_document.md)。
