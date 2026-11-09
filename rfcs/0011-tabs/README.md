# RFC 0011：分頁

- 開始日期：2026-10-06
- 狀態：✅ 已完成：2026-10-06（多個分頁和快捷鍵、分頁列、連結開新分頁、sad tab；每個 renderer 一份 raster 的快取。拖拉排序之後再做。驗收見 `progress.md`）
- 細節：`details.md`

## 摘要

一個視窗可以有多個分頁。照 Chromium 的分法：`TabStripModel` 擁有每個分頁的 `WebContents`，視窗的網頁 view（RFC 0002）顯示目前的分頁，切換時換掉它指向的 `WebContents`；每個分頁一條 renderer thread，隱藏的分頁告訴 renderer，不再要 `BeginFrame`；Viz 在執行中登記和拿掉每個分頁的 frame sink；raster 的快取改成每個 compositor 一份（修掉 `docs/architecture.md`「Process 的邊界」的例外 6）。工具列上方有分頁列；連結能在前景或背景開新分頁；renderer 當掉的分頁留著，顯示 Browser 畫的 sad tab，下一次導覽時重開 renderer。

## 動機

原本 kun 一個視窗只有一個網頁：`BrowserWindow` 啟動時建一個 `WebContents`，Viz 的 `BeginFrameSource` 在啟動時就固定，renderer 不知道自己被隱藏，renderer 當掉就關掉整個視窗。瀏覽器最基本的操作之一是同時開好幾個網頁、切來切去。

另外，raster 的字型和圖片的快取（`TYPEFACES`、`IMAGES`）是整個 process 共用的 `static`，是 process 邊界的例外 6：有多個 renderer 時，不同網站的分頁共用快取，和 site isolation 的方向相反。

目標：
- 一個視窗開、切換、關閉多個分頁；網址列、上一頁和下一頁、視窗的標題跟著目前的分頁。
- 隱藏的分頁不畫、不跑動畫。
- 每個 renderer 自己一份 raster 的快取，`sanity.py boundary` 的允許清單拿掉例外 6。
- 分頁列、連結開新分頁、renderer 當掉時的 sad tab。

不在範圍內：多個視窗；多個分頁共用一個 renderer process（process 模型，RFC 0004）；分頁在視窗之間拖拉；拖拉排序（之後）。

## 設計概要

給使用者：視窗上方的分頁列可以點分頁切換、點關閉按鈕或中鍵關閉、點 `+` 開新分頁；快捷鍵見下面的表。新分頁是 `about:blank`，網址列有 focus。關掉最後一個分頁時視窗跟著關。`target="_blank"` 的連結在前景開新分頁，中鍵和 Cmd/Ctrl+點擊在背景開，再加 Shift 在前景開；從連結開的分頁放在原本的分頁右邊，關掉時回到原本的分頁。renderer 當掉的分頁顯示 sad tab，按 Reload、重新整理或從網址列導覽就重新載入；在網址列輸入 `about:crash` 可以讓目前的分頁的 renderer 結束。

給之後改 `shell` 的人：分頁由 `TabStripModel` 擁有；新的 `WebContents` 由 `BrowserView` 的 `TabFactory` 建（視窗的 `BrowserWindow` 同時向 Viz 登記它的 frame sink），關掉時交還給它。網頁 view 只是顯示某個 `WebContents` 的位置：切換分頁就是換掉第一個網頁 view 指向的分頁。分頁列（`TabStrip`）和 sad tab（`SadTab`）只畫和判斷點到哪裡，點了做什麼由 `BrowserView` 決定。

## 詳細設計

### 分頁和切換

- **`TabStripModel`**（`shell`，對應 Chromium 的 `TabStripModel`）：分頁的 `WebContents` 的清單和目前的分頁。`BrowserView` 擁有它；第一個網頁 view 顯示目前的分頁，其他的網頁 view（例如 RFC 0002 的測試用版面）顯示 `BrowserView` 另外擁有的 `WebContents`，像 Chromium 停靠的 DevTools（第 9 條）。
- **建立和關閉**：`BrowserView` 用 `TabFactory`（`create(TabId)`、`destroy(WebContents)`）建新的 `WebContents`、交還關掉的。`BrowserWindow` 的實作（`WindowTabs`）用同一份 `RendererFlags`、script engine、`NetworkContext` 和 renderer 訊息的 callback，建好之後向 Viz 登記它的 frame sink；交還時關掉 renderer、從 Viz 拿掉 frame sink。對應 Chromium 的 `Browser`（建分頁）和 `HostFrameSinkManager`。`TabId`（和由它決定的 `FrameSinkId`）只增不減：關掉的分頁的 renderer 晚到的訊息，不會被當成新分頁的。
- **切換**：舊的分頁 `WasHidden`、新的 `WasShown`；網頁 view 指向新的分頁，大小照網頁 view 的 bounds（`WebContents::resize` 只在大小或 scale 變了時才通知 renderer）；放掉指向舊分頁的 capture；按鍵的 focus 給新的分頁，網址列失去 focus（新分頁例外：網址列有 focus）；網址列和上一頁、下一頁的按鈕跟著新的分頁。游標記在每個分頁上（`RenderWidgetHostView` 各自的游標）。
- **關閉**：關掉目前的分頁時，回到打開它的分頁（從連結開的，而且它還開著），否則右邊的分頁，沒有就左邊的（`DetermineNewSelectedIndex`）。關掉最後一個分頁時不拿掉它，`BrowserView` 記下視窗要關（`close_requested`），由視窗那一層關掉視窗、再關掉分頁（Chromium 也是關視窗時才關最後一個分頁，第 6 條）。
- **快捷鍵**（第 7 條；根目錄 README 的「鍵盤和滑鼠」）：

  | 動作 | macOS | Windows、Linux |
  | --- | --- | --- |
  | 新分頁 | Cmd+T | Ctrl+T |
  | 關閉分頁 | Cmd+W | Ctrl+W |
  | 下一個分頁 | Ctrl+Tab，Cmd+Option+→，Cmd+Shift+] | Ctrl+Tab，Ctrl+PageDown |
  | 上一個分頁 | Ctrl+Shift+Tab，Cmd+Option+←，Cmd+Shift+[ | Ctrl+Shift+Tab，Ctrl+PageUp |
  | 第 1 到 8 個分頁、最後一個 | Cmd+1 到 8、Cmd+9 | Ctrl+1 到 8、Ctrl+9 |

  切換在頭尾循環，和 Chrome 一樣。
- **自動化**（RFC 0006）：查詢、點擊、`wait-loaded` 都對目前的分頁；`AutomationHost` 按分頁記載入完成的導覽、查詢和要複製的 frame，一個分頁的 renderer 結束只讓等它的失敗。

### 隱藏的分頁

`RendererMsg::WasHidden`、`WasShown`（Chromium 的 `blink.mojom.Widget::WasHidden`、`WasShown`）到 renderer 的 main thread，再到 compositor（cc 的 `LayerTreeHost::SetVisible`）。看不見的 compositor 不要 `BeginFrame`，所以沒有 main frame、`requestAnimationFrame` 也停；再看得見時重畫一次。Viz 保留隱藏的分頁最後一張 frame，切回來時先顯示它（第 3 條）。`WebContents` 記著告訴過 renderer 的可見性（`is_visible`）。

### Viz 和 raster

- **在執行中登記 frame sink**（第 4 條）：`VizMsg::RegisterFrameSinkId(id, BeginFrameObserver)` 讓 Viz 開始送那個 sink `BeginFrame`；`VizMsg::InvalidateFrameSinkId(id)` 停止送、丟掉它的 surface（`BeginFrameSource::remove_client`、`Display::remove_frame_sink`）。`BeginFrameObserver` 帶著 Browser 給的 closure，只在 Browser 和 Viz 之間，Viz 一直在 Browser process（`sanity.py boundary` 的允許清單，和 `CopyOutputRequest` 的 callback 同一類）。
- **raster 的快取**（第 2 條）：`raster::RasterCache` 放字型和解碼後的圖片，由每個 `LayerTreeHostImpl` 擁有（cc 的 `ImageDecodeCache` 屬於 `LayerTreeHostImpl`），raster 時傳進去。每個 renderer 的 compositor 一份，Browser 的工具列一份。`TYPEFACES`、`IMAGES` 從 `sanity.py boundary` 的允許清單拿掉。

### 分頁列

- **位置和樣子**（第 10 條）：工具列上方獨立一列，高 34 CSS px（`tab_strip::HEIGHT`），底色比工具列深；目前的分頁和工具列同色、上方圓角，和工具列連成一體；其他分頁之間有細的分隔線，hover 時變色。分頁列一直顯示，只有一個分頁也一樣。
- **寬度**（第 11 條）：分頁從左邊排起，每個最寬 240 CSS px；放不下時一起縮，最窄 56；再多的分頁超出分頁列的右邊，看不到，快捷鍵照樣切得到。`+` 按鈕在最後一個分頁之後，超出時停在分頁列的右端。
- **標題**（第 12、13 條）：網頁的標題，沒有標題時是網址（和視窗的標題一樣）。太長時結尾淡出：標題的最後一段用 8 條透明度遞增的細條蓋上分頁的底色（paint 沒有漸層）。
- **關閉按鈕**（第 14 條）：分頁至少 100 CSS px 寬時每個都有，比這窄時只有目前的分頁有。
- **滑鼠**（第 15 條）：在分頁上按下主按鍵就切換；關閉按鈕和 `+` 按鈕要在同一個按鈕上按下又放開；在分頁上按中鍵（放開時）關掉它。指標的游標是箭頭。
- **結構**（第 16 條）：`shell` 的 `TabStrip`（對應 Chromium 的 `TabStrip`、`Tab`、`NewTabButton`）只知道每個分頁的標題和哪個是目前的，點擊的結果交給 `BrowserView` 處理。`BrowserViewLayout` 多給分頁列的 bounds，hit test 多一個目標；分頁列和工具列畫在同一個 picture layer 上（Chromium 的 `TopContainerView`），`DisplayItemList::push_list_at` 把另一份 display list 平移後放進來（cc 的 `DrawRecordOp`）。分頁的標題和網址列的文字用同一套字型和畫 glyph 的程式（`ui_text`，對應 Chromium 的 `gfx::RenderText`）。

### 連結開新分頁

- **開在哪裡**（第 19 條）：`target="_blank"`（和其他不是 `_self`、`_top`、`_parent` 的 target，`opens_new_context`）開前景的新分頁；中鍵、Cmd+點擊（macOS）或 Ctrl+點擊（其他平台）開背景的新分頁，再加 Shift 是前景；Shift+點擊開前景的新分頁（Chrome 開新視窗）；Alt+點擊照一般的點擊（Chrome 下載）。script 自己觸發的點擊不開新分頁，它的 `target` 什麼都不做（沒有使用者手勢，Chrome 會封鎖這種彈出視窗）。
- **由 renderer 決定**（第 20 條）：`dom_api` 的 `NavigationPolicy` 和 `navigation_policy`（Blink 的 `NavigationPolicyFromEvent`），平台在編譯時決定，函式另外帶平台參數，單元測試兩種都測；`FrameClient::follow_link` 帶著它，renderer 換成 `protocol` 的 `WindowOpenDisposition`，放進 `OpenUrl`。`javascript:` 的連結照舊在同一個 document 執行。
- **滑鼠事件**（第 21 條）：`InputEvent` 的按下和放開帶修飾鍵（`Modifiers`：shift、ctrl、alt、meta）；renderer 對每個按鈕各自記住按下的元素，主按鍵以外的點擊 dispatch `auxclick`（可以取消），使用者的中鍵的 `auxclick` 沒被取消時，`<a>` 照連結的點擊處理；`MouseEvent` 和 `MouseEventInit` 有 `ctrlKey`、`shiftKey`、`altKey`、`metaKey`。`mouse-event-init` 的 record 多了欄位，`kun:dom` 升到 0.4.0，所有 guest 重編。
- **Browser**（第 23 條）：`WebContents` 收到開新分頁的 `OpenUrl`，做和同一個分頁一樣的檢查（scheme、只有正在顯示本機檔案的 renderer 可以開本機檔案，用打開它的分頁的 renderer 的權限），記下來（`take_opened`、`OpenUrlParams`），由 `BrowserView` 開（Chromium 的 `WebContentsDelegate::OpenURLFromTab`）。
- **`BrowserView`**（第 22 條）：新分頁放在打開它的分頁右邊，在它之前從同一個分頁開的分頁之後（`TabStripModel::insert`）；背景的新分頁一開始就隱藏、照網頁 view 的大小；記住打開它的分頁（`openers`），關掉目前的分頁時回到它。

### renderer 當掉和 sad tab

- **分頁留著**（第 24 條）：renderer 結束（`RendererGone`）時 `WebContents` 記下當掉了（`is_crashed`），網址、標題、歷史不變；最後一個分頁也一樣，視窗不關。
- **sad tab**（第 24、27 條）：Browser 自己畫（Chrome 的 `SadTabView`）：在網頁 view 的位置用一個 picture layer 取代 renderer 的 surface，一個圖示、標題「This page crashed」、一行說明、「Reload」按鈕。`shell` 的 `SadTab` 只管畫和點到哪裡，按鈕按下又放開在它上面才算；sad tab 的範圍裡只有按鈕有作用，按鍵和滑鼠事件不送給已經結束的 renderer，工具列和快捷鍵照常。背景的分頁當掉，切過去時才看到；分頁列上看不出哪個分頁當掉了。
- **重開 renderer**（第 25 條）：下一次導覽時（sad tab 的按鈕、重新整理、上一頁和下一頁、網址列；Chromium 的 `RenderFrameHostManager`）。`WebContents` 留著啟動 renderer 的參數（`Launch`；訊息的 callback 能共用，`shutdown` 之後放掉），開新的 `RenderProcessHost`，不等舊的 thread（`abandon`）；沿用 `TabId` 和 `FrameSinkId`，`LocalSurfaceId` 往前推再送一次大小，隱藏的分頁告訴新的 renderer；Viz 先丟掉這個 frame sink 和它的 surface（`InvalidateFrameSinkId`，舊的 renderer 的最後一張 frame 不會在新的畫出來之前顯示），再重新登記送 `BeginFrame` 的對象。重新登記由 `WebContents` 自己做：每一種導覽都經過它。
- **`about:crash`**（第 26 條）：debug URL（`DebugUrl`），只在使用者自己導覽時有效（和 `about:gpucrash` 一樣）。`RenderProcessHost::kill` 讓 renderer 立刻結束、IO thread 不再轉送任何訊息，`WebContents` 當下就記下當掉了：之後馬上重新整理時，舊的 `RendererGone` 不會被當成新的 renderer 的。

### 測試

各個 crate 的測試和情境見 `progress.md` 的「驗收」：`browser_view` 用測試用的 `TabFactory` 和版面測分頁、分頁列、連結和 sad tab 的行為和像素；`viz`、`compositor`、`raster`、`dom_api`、`browser` 各自測自己的部分；情境 `tabs`、`tab-strip`、`link-tabs`、`sad-tab` 跟著 `cargo test` 和 `test.py automation` 跑。外觀（分頁列、sad tab）由使用者看截圖確認。

### 分階段

1. **多個分頁**：`TabStripModel`、Viz 在執行中登記 frame sink、隱藏的分頁、每個 renderer 一份 raster 的快取、快捷鍵。那時只能用快捷鍵操作分頁，renderer 當掉時關掉分頁（第 8 條，3b 改掉）。
2. **分頁列**。
3. **3a 連結開新分頁、3b sad tab**。拖拉排序原本也在這一段，之後再做（第 18 條）。

## 缺點

- 每個分頁一條 renderer thread、一個 V8 isolate、一份 raster 的快取：分頁多時記憶體比共用 process 的 Chrome 多。
- 隱藏的分頁 `DidFinishLoad` 要等再顯示時才送（見「和 Chromium 不同的地方」）。
- renderer 當掉後的新 renderer 沿用 `FrameSinkId`：Viz 要先丟掉舊的 surface，否則會畫出舊的 frame（第 25 條）。拆 process 時要改成新的 `FrameSinkId`。

## 理由和其他做法

- **raster 的快取**：選每個 `LayerTreeHostImpl` 一份（第 2 條）。不選 `thread_local`：之後 raster 搬到 worker thread 時，快取會跟著 thread 分散；不選維持共用：不同網站的分頁共用快取，和 site isolation 的方向相反。
- **隱藏的分頁**：選告訴 renderer（第 3 條）。不選「只是不畫它的 surface」：隱藏的分頁照樣跑動畫、raster、送 frame。
- **renderer 當掉**：選 Browser 自己畫 sad tab（第 24 條）。第 1 段先關掉那個分頁（第 8 條），因為 sad tab 要能在同一個 `WebContents` 裡重開 renderer。不選「開新的 renderer 顯示錯誤頁」：和 Chrome 不同，而且要先有能用的 renderer。
- **誰決定連結開在哪裡**：選 renderer（第 20 條），和 Chromium 一樣。

## 先例

Chromium：`Browser` 擁有 `TabStripModel`，`TabStripModel` 擁有每個分頁的 `WebContents`；`BrowserView` 的 `contents_web_view_` 用 `SetWebContents` 換成目前的分頁。分頁隱藏時 `WebContentsImpl::WasHidden` 經過 `RenderWidgetHostImpl::WasHidden` 告訴 renderer，`LayerTreeHost::SetVisible(false)` 讓 scheduler 不再要 `BeginFrame`。每個 renderer 的 frame sink 由 Browser 向 Viz 登記（`HostFrameSinkManager::RegisterFrameSinkId`），關掉時 `InvalidateFrameSinkId`。連結開在哪裡由 Blink 的 `NavigationPolicyFromEvent` 決定，Browser 的 `OpenURLFromTab` 開分頁；renderer 當掉時顯示原生的 `SadTabView`，下一次導覽時 `RenderFrameHostManager` 建新的 process。細節見 `details.md`。

## 和 Chromium 不同的地方

| 項目 | kun | Chromium | 之後 |
| --- | --- | --- | --- |
| 新分頁（第 5 條） | `about:blank`，網址列有 focus | 新分頁頁面（NTP），網址列有 focus | 之後 |
| renderer 當掉後的 `FrameSinkId`（第 25 條） | 新的 renderer 沿用原本的，Viz 先丟掉舊的 surface | 新的 process 有新的 `FrameSinkId` | RFC 0004（拆 process 時） |
| `about:crash`（第 26 條） | renderer 立刻結束，像 `chrome://kill` | `chrome://crash` 讓 renderer 自己 crash，`chrome://kill` 由 Browser 殺掉 | 拆 process 時 |
| 當掉的分頁在分頁列上（第 27 條） | 看不出來 | favicon 換成難過的臉 | 有 favicon 時 |
| 隱藏分頁的載入完成 | `DidFinishLoad` 跟著 frame 送，隱藏時等到再顯示 | 載入完成和 frame 無關 | 之後 |
| 隱藏分頁的其他節流 | 只停 `BeginFrame` | 另外有 `visibilitychange`、計時器節流、一段時間後丟掉 surface | 之後 |
| 關掉分頁之後（第 22 條） | 打開它的分頁還開著就回到它，否則右邊的、沒有就左邊的 | 一樣，但使用者切到別的分頁之後就「忘掉」打開它的分頁 | 之後 |
| 切換時網址列 | 網址列失去 focus，打到一半的網址不保留 | 每個分頁記得 focus 和打到一半的網址 | 之後 |
| renderer 的分配 | 每個分頁一條 renderer thread | 同一個網站的分頁可能共用一個 renderer process | RFC 0004 |
| 分頁列的位置（第 10 條） | 在系統的視窗框（標題列）下面 | 畫在自訂的視窗框裡，和標題列同一列 | 之後 |
| 分頁太多（第 11 條） | 縮到最窄之後，超出的看不到 | 新版可以左右捲動分頁列 | 之後 |
| 標題的淡出（第 12 條） | 8 條透明度遞增的細條疊出來 | 真的線性漸層 | 之後（有 CSS 的漸層時） |
| 分頁上的圖示（第 13 條） | 沒有 favicon，也沒有載入中的動畫 | favicon、載入中的動畫 | 之後 |
| tooltip | 沒有 | 滑過分頁時顯示完整的標題 | 之後 |
| Shift+點擊連結（第 19 條） | 前景的新分頁 | 新視窗 | 有多個視窗時 |
| Alt+點擊連結（第 19 條） | 照一般的點擊 | 下載 | 有下載時 |
| 有名字的 target（第 19 條） | 每次都開新分頁 | 之後同一個名字的連結在同一個分頁開 | 之後 |
| `window.open`（第 23 條） | 沒有 | 開新分頁或視窗，回傳它的 window；沒有使用者手勢時被封鎖 | 之後 |

## 已決定的事項

1. **分三段，先做「多個分頁」**（2026-10-06）：第 1 段 `TabStripModel`、Viz 在執行中登記 frame sink、隱藏的分頁、每個 renderer 一份 raster 的快取、快捷鍵；第 2 段分頁列；第 3 段連結開新分頁、sad tab、拖拉排序，之後再排。多個視窗、分頁共用 renderer process 不在範圍內。
2. **raster 的快取由每個 `LayerTreeHostImpl` 擁有**（2026-10-06）：對應 cc 的 `ImageDecodeCache` 屬於 `LayerTreeHostImpl`；Chromium 的快取是每個 renderer process 一份，這樣「一個 renderer 一份」不依賴 process 的界線。不選 `thread_local`、不選維持共用，理由見「理由和其他做法」。
3. **隱藏的分頁告訴 renderer**（2026-10-06）：`WasHidden`、`WasShown`，compositor 不再要 `BeginFrame`，`requestAnimationFrame` 也停。和 Chromium 一樣；`visibilitychange`、計時器節流、丟掉隱藏分頁的 surface 留到之後。
4. **Viz 在執行中登記和拿掉 frame sink**（2026-10-06）：`VizMsg::RegisterFrameSinkId`、`InvalidateFrameSinkId`，對應 Chromium 的 `HostFrameSinkManager`。送 `BeginFrame` 的 closure 照 `CopyOutputRequest` 的 callback 的前例放進 `sanity.py boundary` 的允許清單：只在 Browser 和 Viz 之間。
5. **新分頁開 `about:blank`，網址列有 focus**（2026-10-06）：Chrome 開新分頁頁面；kun 沒有。
6. **關掉最後一個分頁時視窗跟著關**（2026-10-06）：和 Chrome 一樣。
7. **快捷鍵照 Chrome**（2026-10-06）：見「詳細設計」的表；根目錄 README 的「鍵盤和滑鼠」跟著改。
8. **renderer 當掉時只關掉那個分頁**（2026-10-06；3b 改成 sad tab，見第 24 條）：最後一個就關視窗。Chrome 顯示 sad tab，要能重開 renderer，排到第 3 段。
9. **`TabStripModel` 擁有分頁，`BrowserView` 只換網頁 view 指向的分頁**（2026-10-06）：和 Chromium 一樣（RFC 0002 第 2 條）。每個分頁一條 renderer thread。

10. **分頁列在工具列上方獨立一列**（2026-10-06，第 2 段）：高 34 CSS px，目前的分頁和工具列同色、連成一體。和 Chrome 一樣，但 Chrome 畫在自訂的視窗框（標題列）裡，kun 用系統的視窗框，分頁列在它下面。不選分頁放進工具列那一列（Safari 的精簡模式）、直向分頁（Edge）：和 Chrome 不同。
11. **分頁的寬度最寬 240、最窄 56 CSS px，放不下時一起縮，再多就超出右邊**（2026-10-06，第 2 段）：縮的方式和 Chrome 一樣；Chrome 新版超出時能捲動分頁列，留到之後。
12. **標題太長時結尾淡出，用透明度遞增的細條疊出來**（2026-10-06，第 2 段）：樣子接近 Chrome 的淡出。不選在 paint 加真的線性漸層：要動 `compositor_paint` 和 raster，比較適合和 CSS 的 `linear-gradient` 一起設計，那時再換。不選截斷加「…」：和 Chrome 不同。
13. **分頁上是網頁的標題，沒有標題時是網址**（2026-10-06，第 2 段）：和視窗的標題一樣。Chrome 另有 favicon 和載入中的動畫，kun 還不抓 favicon，留到之後。
14. **關閉按鈕在分頁至少 100 CSS px 寬時每個都有，比這窄時只有目前的分頁有**（2026-10-06，第 2 段）：和 Chrome 一樣，窄的時候只留目前的分頁。
15. **按下就切換分頁；關閉、`+` 按鈕按下又放開才算；中鍵關閉；hover 變色**（2026-10-06，第 2 段）：和 Chrome 一樣。拖拉排序在第 3 段；tooltip 要先有 tooltip 的機制，留到之後。
16. **`TabStrip` 和工具列畫在同一個 picture layer，版面和 hit test 各多一塊**（2026-10-06，第 2 段）：對應 Chromium 的 `TabStrip`、`Tab`、`NewTabButton` 和 `TopContainerView`；網頁的位置照 RFC 0002 只跟著版面走。
17. **測試：單元測試算幾何，`browser_view` 測點擊和像素，情境用 `click-at` 點分頁，外觀由使用者看截圖**（2026-10-06，第 2 段）：情境查不到工具列，只能點座標。

18. **第 3 段分成 3a 連結開新分頁、3b sad tab；拖拉排序之後再做**（2026-10-06）：三件事互不相依，各自有測試和 mutation test，開始前才列那段的決定。拖拉排序原本也在第 3 段，使用者決定之後再做。
19. **哪些點擊開新分頁**（2026-10-06，3a）：`target="_blank"` 和其他不是 `_self`、`_top`、`_parent` 的 target 開前景的新分頁；中鍵、Cmd+點擊（macOS）或 Ctrl+點擊開背景的，再加 Shift 是前景。和 Chrome 一樣；Shift+點擊在 Chrome 開新視窗、Alt+點擊下載、有名字的 target 之後會重用同一個視窗，kun 還沒有這些，分別開前景的新分頁、照一般的點擊、每次開新分頁。
20. **由 renderer 決定開在哪裡**（2026-10-06，3a）：和 Chromium 一樣（Blink 的 `NavigationPolicyFromEvent`，平台在編譯時決定），`OpenUrl` 帶 `WindowOpenDisposition`。不選「renderer 只回報按鈕和修飾鍵、Browser 決定」：和 Chromium 不同。
21. **滑鼠事件帶修飾鍵；非主按鍵 dispatch `auxclick`，中鍵的 `auxclick` 開連結；`MouseEvent` 有修飾鍵的屬性**（2026-10-06，3a）：和 Blink 一樣（`HTMLAnchorElement` 把中鍵的 `auxclick` 當作連結的點擊）。`MouseEvent` 的新屬性也是 `kun:dom` 的 WIT 的新函式。
22. **新分頁放在打開它的分頁之後，關掉時回到打開它的分頁**（2026-10-06，3a）：放在打開它的分頁右邊、在它之前開的那些之後，和 Chrome 一樣；Chrome 在使用者切到別的分頁之後就不再回到打開它的分頁，kun 先不做，記在「和 Chromium 不同的地方」。
23. **`window.open` 之後再做**（2026-10-06，3a）：要回傳另一條 renderer thread 上的 window，還要使用者手勢和彈出視窗的封鎖，比 3a 其他的部分大得多。新分頁的網址照 `open_url` 的檢查，用打開它的分頁的 renderer 的權限。

24. **renderer 當掉時分頁留著，Browser 自己畫 sad tab**（2026-10-06，3b）：和 Chrome 一樣（原生的 `SadTabView`），取代第 8 條；最後一個分頁當掉時視窗也不關。不選「開新的 renderer 顯示錯誤頁」：和 Chrome 不同，而且要先有能用的 renderer。文字用自己的英文，不用 Chrome 的「Aw, Snap!」。
25. **下一次導覽時重開 renderer，沿用 `TabId` 和 `FrameSinkId`**（2026-10-06，3b）：重開的時機和 Chrome 一樣（`RenderFrameHostManager`）。Chrome 的新 process 有新的 `FrameSinkId`；kun 的 `FrameSinkId` 由 `TabId` 決定，沿用它：Viz 先丟掉這個 frame sink 和它的 surface，再重新登記，舊的 renderer 的最後一張 frame 不會在新的畫出來之前顯示；`LocalSurfaceId` 往前推，舊的 renderer 晚到的 frame 也進不來。原本只推 `LocalSurfaceId`，mutation test 發現 Viz 照樣畫出它存著的舊 frame（重開之後沒送大小的 mutation 沒被抓到，因為畫面上是舊 renderer 的最後一張），所以加上丟掉 surface。向 Viz 重新登記由 `WebContents` 自己做：每一種導覽（重新整理、上一頁、網址列）都經過它；第 1 段的「建立時由 `TabFactory` 登記」不變。開始前原本打算重新登記也交給 `TabFactory`，但那要在 `BrowserView` 每個會導覽的地方檢查有沒有重開，所以改成這樣。
26. **`about:crash` 讓 renderer 立刻結束，當下就記下分頁當掉**（2026-10-06，3b）：只在使用者自己導覽時有效。kun 的 renderer 是 thread，真的讓它 panic 可能穿過 V8 的 frame，所以做成立刻結束，效果接近 `chrome://kill`。舊的 renderer 不再轉送訊息，馬上重新整理時，它的 `RendererGone` 不會被當成新的 renderer 的。
27. **分頁列上看不出當掉的分頁；背景的分頁切過去時才看到 sad tab**（2026-10-06，3b）：Chrome 把 favicon 換成難過的臉，kun 還沒有 favicon。
28. **測試：`browser` 測重開，`browser_view` 測 sad tab 的像素、按鈕、快捷鍵、最後一個和背景的分頁，情境用 `about:crash` 和重新整理的快捷鍵；外觀由使用者看截圖**（2026-10-06，3b）。

## 待決定

無。

## 之後的可能

- 拖拉排序分頁（原本是第 3 段的一部分）；拖到分頁列外開新視窗要先有多個視窗。
- 隱藏分頁的 `visibilitychange`、`document.visibilityState`，計時器節流，一段時間後丟掉 surface（Chromium 的 frame eviction）。
- 分頁太多時捲動分頁列；分頁的 favicon、載入中的動畫、tooltip；標題的淡出改用真的漸層。
- 分頁列畫進自訂的視窗框（和標題列同一列）。
- `window.open`；有名字的 target 重用同一個分頁；使用者切到別的分頁之後忘掉打開它的分頁；`onauxclick` 屬性。
- 當掉的分頁在分頁列上的圖示；同一個分頁一再當掉時換一種說明（Chrome 的第二次 sad tab）。
- 每個分頁記得網址列的 focus 和打到一半的網址。
- 新分頁頁面。
- 多個視窗。
