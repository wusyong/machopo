# RFC 0131 的細節：`window.open` 和 `opener`

## 盤點（2026-11-28）

### Browser

- **擁有關係**：`WebContents` 直接擁有一個 `RenderProcessHost`（`web_contents.rs` 的 `process`），沒有共用、沒有計數、沒有 process 的登記表；renderer 的生命就是分頁的（關分頁 `shutdown`，`Drop` 也是）。
- **啟動時綁死分頁**：`RenderProcessHost::launch(tab, frame_sink_id, …)`，分頁的編號收在轉送訊息的 closure 裡，每個訊息以 `(tab, RendererId, msg)` 送給 embedder；`TabId` 的註解說它是 Chromium 的 `(process_id, routing_id)` 的替身，renderer 這一側沒有它。frame sink 是 `{ client_id: tab + 1, sink_id }`，IO 那一側替這個 renderer 的每個 Viz 訊息蓋上啟動時定的那一個。
- **訊息怎麼找到分頁**：IO thread → `ContentsEvent::Renderer { renderer, msg }` → 視窗的共用 callback → `BrowserView::on_contents_event(tab, event)` → `is_current_renderer(renderer)`（只和這個分頁的那一個 process 比）。frame 的 token 只在這個分頁自己的 `FrameTree` 裡找。
- **選 process**：`SiteInfo`（`Web(Site) | File | BuiltIn`），`admits(lock, site)` 決定換不換，`allows` 是 loader 和 storage 的檢查；鎖是 `RenderProcessHost` 上的 `Option<SiteInfo>`，第一次 commit 時定。沒有 `SiteInstance`、`BrowsingInstance`；最接近的是鎖、history 的項目記的 `site_info`、`BrowserView` 的 `openers`（只給分頁列排位置用）。沒有預備的 renderer、不重用。
- **開新分頁**：renderer 的 `OpenUrl { initiator, url, disposition, new_window }` → `WebContents::open_url`（發起者的 origin、sandbox 的檢查和旗標）→ `opened`（`OpenUrlParams`：網址、disposition、發起者的 origin、旗標；**沒有開它的分頁、frame、process、名字**）→ `BrowserView::open_from`：新的分頁編號、`WebContents::new`（**一定啟動新的 renderer**）、`navigate_opened`。
- **壞訊息、當掉**：`received_bad_message` 結束 process、`renderer_gone` 只處理這一個分頁。
- **其他照分頁或 renderer 記的**：`sessionStorage` 的 namespace 是分頁的編號；blob URL、loader、storage area 照 `renderer_id`；DevTools 的 target id 是分頁的根 frame 的 token，`can_access_opener` 寫死 `false`；headless 只有一個分頁、不拿 `take_opened`。

### renderer

- **一個 renderer 一份的（本來就是 process 的）**：engine（一個 V8 isolate、一個 `Dom`：一個 GC heap）、scheduler、worker、blob URL、文字、canvas 的 encoder；V8 inspector 的 session。
- **一個分頁一份、現在只有一份的**：`FrameWidget`（`LayerTreeHost`、畫好的輸出、overlay、input 的 ack）、viewport 和螢幕的位置、隱藏和焦點、popup、游標、滑鼠的狀態、自己的 history 項目數、`render_frames` 的 `top` 和它的 token、DevTools 的六個 agent 和 session、WebDriver 的狀態。`spawn_renderer_main_thread` 建一個 `LayerTreeHost`、一個 Compositor thread、一個 frame sink。
- **混在一起的**：layout 的 `Frame`（view 的樹、文字的 context、canvas 的 raster、SVG 的圖都掛在這一個 view 上）；`Dom` 的 session history（「這個頁面的」）；wasm 的文件會換掉整個 engine 和它所有的 frame。
- **engine（`dom_js`）**：`const TOP: FrameId = FrameId(0)`，`main_token` 只有一個；`context`、`document`、`event_handler` 是那一個 window 的，input 送進它；**commit 最上層的文件會 `clear_children()`：清掉 `TOP` 以外所有的 frame**；標題、rendering update 的順序都只從 `TOP` 走。
- **子 frame 現在怎麼表示**：engine 裡每個子 frame 有自己的 realm（同一個 isolate、同一個 heap）、文件、token；renderer 裡有自己的 `RenderFrame` 和 `FrameDocument`；但**沒有自己的 view 和 compositor**，畫進父 frame 的同一份輸出。`dom` 找最上層是照 `frame_owner` 往上走，沒有 owner 的文件本來就是自己的最上層。
- **不帶 frame 或分頁的訊息**：Browser 到 renderer 的 `UpdateVisualProperties`、`UpdateScreenRects`、`WasHidden`、`WasShown`、`SetFocus`、`SetInitialFocus`、`PopupMenuClosed`、`SetHistoryOffsetAndLength`、DevTools 的 attach、detach 和訊息、`QueryElement`、`RequestVisualState`、`Unload`、compositor 的 `BeginFrame` 和 input；renderer 到 Browser 的 `SetCursor`、popup、`TakeFocus`、`InputEventAck`、`DidReceiveUserActivation`、`UpdateTitle`、`DidFinishLoad`、測試的結果、`VisualStateReady`。
- **API 的現況**：`window.opener` 一律 `null`；`close()` 什麼都不做；`closed` 只在 iframe 不在了時是 true；`window.name` 不在 IDL 裡（子 frame 的名字是 iframe 的 `name` 屬性）；`window.open` 沒有。具名的 target（`hyperlink.rs` 的 `chosen_document`）只找 iframe 的 `name`，找不到的每次開新分頁。`MessagePort` 的 pipe 是同一個 process 裡共用的（可以跨 thread），不能跨 renderer。啟動（user activation）不會被用掉。

## Chromium

憑對 Chromium 的了解寫的大綱；每一段開始前讀原始碼確認，改寫這一節。

- **`BrowsingInstance`、`SiteInstance`**（`content/browser/site_instance_impl.h`、`browsing_instance.h`）：`BrowsingInstance` 是一組互相拿得到參照的 browsing context（HTML 的 browsing context group）；裡面每個網站一個 `SiteInstance`，對到一個 process。`window.open` 開的視窗和開它的在同一個 `BrowsingInstance`；使用者自己開的分頁、`noopener`、COOP 換的是新的。
- **一個 process 裝好幾個分頁**：renderer 裡每個分頁一個 `blink::Page`（`WebViewImpl`），每個本地的最上層 frame 有自己的 widget 和 compositor（`WebFrameWidgetImpl`、`LayerTreeHost`）；同一個 process 的 `Page` 共用 main thread 和 V8 isolate。Browser 的 `RenderProcessHostImpl` 照它裝的 frame 計數，沒有了才結束。訊息照 frame 的 token 的 mojo 介面走，不照分頁。
- **`window.open` 的路**：Blink 的 `LocalDOMWindow::open` → `CreateNewWindow` → 同步的 `CreateNewWindow` IPC 給 Browser（`RenderFrameHostImpl::CreateNewWindow`）：Browser 建新的 `WebContents`、在同一個 `SiteInstance` 裡，回傳新的 frame 的 token；renderer 當場建新的 `Page` 和初始的 `about:blank`，`window.open` 回傳它的 `WindowProxy`，之後才導覽。`noopener` 時不同步建：新的 `BrowsingInstance`，回傳 `null`。
- **跨網站的彈出視窗**：初始的 `about:blank` 和開它的同源、在開它的 process；導覽到別的網站時換 process，開它的那一側留下 `RenderFrameProxy`（遠端的 window），`opener`、`postMessage`、`closed` 經過 Browser。machopo 沒有 proxy（O4）。
- **`window.close()`**：只有 script 開的、或 history 只有一個項目的最上層可以；renderer 要求 Browser 關。
- **popup blocker**：沒有 transient activation 的 `window.open` 被擋（`chrome/browser/ui/blocked_content`）；開了用掉 activation。
- **COOP**（`content/browser/renderer_host/navigation_request.cc`、`cross_origin_opener_policy_status.cc`）：回應的 `Cross-Origin-Opener-Policy` 和現在的文件的不相容時換 `BrowsingInstance`、清掉 `opener` 和名字。
- **同一個 site 的一般分頁共不共用 process**：預設不共用，過了軟性的上限才重用（`../0124-cross-site-process-swap/details.md` 的「預備的 renderer 和重用」）。

### 讀過原始碼確認的（2026-11-28，第 1 段的第 1 題）

- `BrowsingInstance`、`SiteInstanceGroup` 是 `base::RefCounted`（不是 atomic 的）；`browsing_instance.h`：「Because both classes are RefCounted, they do not need to be manually deleted」。`SiteInstance` 建好之後才 `SetSite`。
- `RenderProcessHostImpl` 不被計數、不被 `SiteInstance` 擁有：登記在全域的表（`GetAllHosts()`，照 process 的編號），`SiteInstanceGroup` 拿不擁有的指標、當它的 observer。每個 frame 是它的 listener（`RenderFrameHostImpl` 建立時 `AddRoute`、不在了 `RemoveRoute`）；`Cleanup()` 在沒有 listener 時 `DeleteSoon` 刪掉自己，先通知每個 observer（`RenderProcessHostDestroyed`）。`Cleanup()` 裡的 `within_process_died_observer_` 是處理重入的：observer 在回呼裡又讓 process 結束，刪除要延後（註解附了 crbug.com/339504）。
- 壞訊息：`ShutdownForBadMessage` → `Shutdown`，結束整個 process；結束之後通知每個 observer（`RenderProcessExited`）。
- `--single-process`：`Cleanup()` 直接返回（那條 renderer thread 永遠留著），壞訊息那條路是 `NOTREACHED`。

### machopo 用到 host 的地方（2026-11-28 數的）

`web_contents.rs` 的 `self.process.…` 47 處：只要 sender、編號、atomic 或 `Mutex` 的 31 處（`send` 18、`send_input_event`、`sender`、兩個 Viz 的 sender、`set_file_access`、`renderer_id`、`id`、`register_loader`、`ask_to_shut_down`）；讀或設鎖的 5 處；唯讀地看 host 的 3 處（`pid`、`shared_memory`、`exit`）；改它或結束它的 8 處（`kill` 2、`crash`、`ended`、`shutdown`、`wait_for_shutdown`、`deliver`、`deliver_gone`）加上換 renderer 的 `mem::replace`。照呼叫關係往上追，69 個公開方法裡 22 個走得到後三類（照名字追，是上限）；它們在 `browser` 的測試有 625 個呼叫點，`shell` 的程式約 20 個（經過 `BrowserView` 的 `contents_mut`、`update_contents`）。建 `WebContents` 的地方 5 處。`WebContents` 沒有 `Drop`，靠 host 自己的 `Drop` 結束 renderer。只有一個視窗。

## WPT

還沒盤點：`html/browsers/the-window-object/`（`open-close/`、`named-access-on-the-window-object/`）、`html/browsers/windows/`（`auxiliary-browsing-contexts/`、`noreferrer-*`、`targeting-*`）、`html/cross-origin-opener-policy/`、sandbox 的那約 30 個、wdspec 的 window 相關的。第 1 題定了之後下載 wpt.fyi 上 Chrome 的結果、數出來。
