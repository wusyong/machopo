# RFC 0011 的細節：分頁

這份是 `README.md` 的補充，放支撐它的研究和比較。結論和決定以 `README.md` 為準。

## Chromium 怎麼做

- **誰擁有分頁**：`Browser`（`chrome/browser/ui/browser.h`）擁有 `TabStripModel`，`TabStripModel` 擁有每個分頁的 `WebContents`，記著目前的分頁；`BrowserView` 觀察它（`TabStripModelObserver`），目前的分頁換了就呼叫 `contents_web_view_->SetWebContents`。新分頁由 `chrome::NewTab` → `Browser::AddTabAt` 建，用的是視窗的 profile（kun 對應同一份 `RendererFlags` 和 `NetworkContext`）。
- **隱藏**：`WebContentsImpl::WasHidden` → `RenderWidgetHostImpl::WasHidden` → `blink.mojom.Widget::WasHidden` → `WidgetBase::WasHidden` → `LayerTreeHost::SetVisible(false)`；cc 的 scheduler 看不見時不要 `BeginFrame`。另外 Blink 送 `visibilitychange`、節流計時器；Browser 一段時間後丟掉隱藏分頁的 surface（`DelegatedFrameHost` 的 frame eviction），切回來時先顯示舊的或空白，等新的 frame。
- **frame sink**：每個 renderer 的 `FrameSinkId` 由 Browser 向 Viz 登記（`HostFrameSinkManager::RegisterFrameSinkId`），renderer 再透過 Browser 建 `CompositorFrameSink`；`BeginFrame` 由 Viz 經過 `CompositorFrameSinkClient::OnBeginFrame` 直接送給 renderer。關掉時 `InvalidateFrameSinkId`，surface 跟著消失。
- **raster 的快取**：每個 renderer process 有 Skia 的字型快取（process 全域）；解碼後的圖片在 `ImageDecodeCache`，屬於 `LayerTreeHostImpl`。用 OOP-R 時 raster 在 GPU process，字型由 renderer 序列化過去（`SkStrikeServer` / `SkStrikeClient`）。
- **快捷鍵**：`chrome/browser/ui/accelerator_table.cc`（Windows、Linux）和 `global_keyboard_shortcuts_mac.mm`。
- **關掉分頁之後**：`TabStripModel::DetermineNewSelectedIndex` 優先選打開它的分頁（opener），沒有就右邊的，再沒有就左邊的。
- **renderer 當掉**：`WebContentsImpl::RenderProcessGone` → 分頁顯示 sad tab（`SadTabView`），重新整理時建新的 renderer process。

## kun 原本的樣子（2026-10-06）

- `BrowserWindow::start` 建一個 `WebContents`（`TabId(0)`），在 Viz thread 啟動前把它的 frame sink 登記進 `BeginFrameSource`，之後不能再加。
- `BrowserView` 有一個 `WebContents` 的清單，第一個是視窗的 tab（RFC 0002）；其他的只在測試用的版面裡。
- 每個 `WebContents` 有自己的 `RenderProcessHost`，也就是一條 renderer thread；`NetworkContext` 可以 clone，分頁之間共用。
- protocol 沒有 visibility 的訊息；`RendererGone` 讓視窗關掉。
- raster 的 `TYPEFACES`、`IMAGES` 是 `static` 的 `Mutex`，整個 process 共用。
- `DidFinishLoad` 是 main frame 的 swap promise：送出它的是畫出那個 frame 的時候。
