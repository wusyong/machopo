# RFC 0002 的細節：網頁在視窗裡的位置

這份是 `README.md` 的補充，放支撐它的研究和比較。結論和決定以 `README.md` 為準。

## Chromium 怎麼做

Windows、Linux、ChromeOS 上是兩層：

- **views**（`ui/views`）：瀏覽器 UI 的 view 樹。`Widget` 是視窗，底下是巢狀的 `View`（工具列、按鈕、網址列、書籤列、側邊欄、承載網頁的 `views::WebView`）。`LayoutManager` 決定每個 view 的 bounds，`FocusManager` 管鍵盤 focus；另外有事件分派的前置和後置 handler、無障礙樹、拖放、動畫、主題。
- **aura**（`ui/aura`）：比 views 低一層的視窗樹。網頁不是普通的 `View`：`views::WebView` 透過 `NativeViewHost` 掛上一個 `aura::Window`，也就是 `RenderWidgetHostViewAura`。它的 bounds 是唯一的來源，傳給 renderer 的大小（`VisualProperties`）和 browser 的 compositor 裡 surface layer 的位置都從它推。滑鼠事件從根視窗沿著 window 樹 hit test（`WindowTargeter`），換成目標 window 自己的座標（`ConvertLocationToTarget`），`RenderWidgetHostViewAura` 收到時已經是網頁座標。按下之後的 move 和 up 都給同一個 window（`CaptureClient`）。

macOS 上網頁是 Cocoa 的 `NSView`（`RenderWidgetHostViewCocoa`），由系統的 view 樹分派事件。

版面（工具列、書籤列、側邊欄、分割的比例）只決定每個 view 的 bounds，網頁本身不知道自己在視窗的哪裡；一個視窗同時有好幾個網頁（DevTools、WebUI 的側邊欄、分割檢視）也是同一套。網頁**內部**跨 process 的 iframe 另外用 Viz 的 hit-test 資料（`HitTestRegionList`）找目標，那是同一個網頁裡的事，和這份 RFC 無關。

| | Chromium | kun 原本（2026-10-03） | kun 完成時（2026-10-06） |
| --- | --- | --- | --- |
| 網頁在哪 | 每個網頁的 view 有一份 bounds | 四處各自假設「從 (0, 工具列高) 開始」 | 每個網頁 view 一份 bounds（整數的 physical px），`BrowserViewLayout` 給 |
| 點到誰 | 從 view / window 樹 hit test | `area_at` 只分「工具列 / 網頁」 | 一個 `hit_test`，平的清單，後面的蓋住前面的 |
| 座標換算 | 用同一份 bounds | `in_page` 只扣 y | `in_target`，用同一份 bounds |
| capture | 跟著目標 | `captured: Option<Area>`，只有「工具列 / 網頁」 | 記 `Target`（工具列，或哪一個網頁 view） |
| 鍵盤 focus | 有 focus 的 view | 網址列有 focus 就給它，否則給唯一的網頁 | 網址列，否則最後按下的網頁 view |
| 畫 | 每個網頁一個 surface layer | `SurfaceLayer` 本來就是清單，已經能放多個、放在任何位置 | 每個網頁 view 一個 `SurfaceLayer`，位置和大小從 bounds 推 |

## 和其他部分的關係

- **renderer 的 compositor（P4）不受影響**：它只活在網頁自己的座標裡，viewport 是網頁的大小，收到的 input 已經是網頁座標，frame 只送到自己的 surface。多個網頁時，每個網頁有自己的 renderer、compositor thread、frame sink 和 `BeginFrame`（`BeginFrameSource` 本來就以 frame sink 為單位送）。Viz 照 Browser 的 frame 把各個 surface 疊上去，本來就是通用的。
- **網頁的起點要對齊整數 physical pixel**：compositor 畫 tile 時把位移取整到 physical pixel，網頁的起點如果落在半個像素上，整頁會糊掉。A2 的 bounds 因此取整；現在的工具列高度也是這樣處理的（`toolbar_px()`）。
- **多個網頁共用一個 renderer process**（Chrome 對同一個網站的分頁會這樣做）時，Chrome 是一個 process 一條 compositor thread、每個網頁（`RenderWidget`）各自一個 `LayerTreeHost` 和 impl 端；kun 現在一個 renderer 只有一個網頁。那是 process 模型的題目（RFC 0004），和網頁放在視窗哪裡無關。
- **瀏覽器的 UI 之後要改成 HTML / CSS**（`P2.md` 的「已決定的事項」第 2 條，`MVP.md` 列為 MVP 之後的目標）：那時工具列本身也是一個網頁，「視窗裡有好幾個網頁、各自在某個位置」會變成常態，A2 的網頁 view 和 hit test 正好是它的基礎，A3 的原生 view 樹反而比較不需要。
