# RFC 0002：網頁在視窗裡的位置

- 開始日期：2026-10-03
- 狀態：✅ 已完成：2026-10-06（A2：網頁 view、一個 hit test、可以換的版面；驗收見 `progress.md`）
- 細節：`details.md`

## 摘要

把「網頁在視窗的哪裡」收成一份資料：每個網頁一個「網頁 view」，指向一個 `WebContents`，記著它的 bounds（整數的 physical px）；送給 renderer 的大小、畫的位置、滑鼠點到誰、座標換算都從這份 bounds 推。形狀照 Chromium 的 views / aura，只做現在需要的部分（選項 A2）。

## 動機

之後可能有左側的書籤列、側邊欄，網頁可能放在視窗的任何位置，也可能同時顯示好幾個網頁（像 Chrome 停靠的 DevTools 或分割檢視）。那時點擊的位置會不會算錯、點不到？

**原本的狀況**（2026-10-03）：`crates/shell/src/browser_view.rs` 的 `BrowserView` 裡，「網頁在視窗的哪裡」由五個地方各自算，共用的只有上方工具列的高度（`page_top()`，取整到 physical pixel）：

| 用途 | 程式 | 怎麼算 |
| --- | --- | --- |
| 網頁的大小（renderer 的 viewport） | `resize` | 視窗寬 × (視窗高 − 工具列高) |
| 網頁畫在哪 | `update` 裡的 `SurfaceLayer` | 位置 `(0, 工具列高)` |
| 滑鼠在不在網頁上 | `area_at` | 只看 `y < 工具列高` |
| 滑鼠座標換成網頁座標 | `in_page` | `(x, y − 工具列高)`，x 不動 |
| 網頁裡的元素在視窗的哪裡（自動化測試點元素用） | `page_to_window` | `y + 工具列高` |

只有上方工具列時，這五處一致，所以當時是對的：`browser_view` 的 `the_toolbar_is_over_the_page`（1x、2x）和 `clicks_in_the_page_are_below_the_toolbar`（1x）守住。

**風險**：五處都假設網頁從 x = 0 開始、只有一個網頁，彼此沒有綁在一起。加書籤列時只要漏改一處就出錯：只改畫的位置、沒改 `in_page`，點擊會往左偏一個書籤列的寬度；漏改 `area_at`，點書籤列會被當成點網頁；漏改 `resize`，網頁的 viewport 比畫出來的寬。現有的測試都抓不到：沒有測試的網頁起點 x 不是 0，點擊的測試也只跑 1x。

目標：
- 網頁放在視窗的任何位置，大小、畫的位置、點擊都對。
- 結構上可以同時有好幾個網頁。
- 加新的 UI 區塊時，只改版面，不必找出所有假設網頁位置的地方。

不在範圍內：巢狀 view 和版面管理、完整的鍵盤 focus 管理（見「理由和其他做法」的 A3）；網頁**內部**的 hit test（iframe），那是同一個網頁裡的事；分頁（RFC 0011，建在這份的網頁 view 上）。

## 設計概要

給之後改 `shell` 的人：加一個書籤列時，只要讓版面多產生一塊「書籤列」，並把網頁 view 的 bounds 往右移。網頁的 viewport、畫的位置、點擊和滾輪送到哪、換成什麼座標，都跟著那份 bounds 走，不必另外改。要同時顯示兩個網頁時，版面產生兩個網頁 view，各自有自己的 bounds；點到哪一個，事件就送給哪一個，座標是它自己的；在哪一個按下滑鼠，按鍵就給哪一個。

## 詳細設計

- **網頁 view**（`WebView`，對應 Chromium 的 `views::WebView` 加上 `RenderWidgetHostViewAura` 的位置）：指向一個 `WebContents`（用 `TabId`），記著 bounds 和它的 renderer 最後要的游標。`WebContents` 由 `BrowserView` 的清單擁有，網頁 view 不擁有它（第 2 條）；現在加一個 `WebContents` 就多一個顯示它的網頁 view，分頁時再把兩者分開。第一個網頁 view 的 `WebContents` 是「目前的網頁」：網址列、標題、上一頁和下一頁跟著它（Chromium 的 active web contents）。
- **bounds**：新增的 `gfx::Rect`（整數，對應 Chromium 的 `gfx::Rect`），physical px，是唯一的來源（第 3 條）：
  - 送給 renderer 的大小就是它的大小；
  - `SurfaceLayer` 的位置和大小是它除以 scale；browser 的 compositor 畫的時候取整回 physical px，所以畫出來剛好在 bounds 上；
  - hit test 和座標換算都用它。
- **版面**（`BrowserViewLayout`，對應 Chromium 的同名類別）：從視窗的大小、scale 和網頁 view 的數目，算出工具列和每個網頁 view 的 bounds，自己取整到 physical px。預設的版面是工具列在上、第一個網頁 view 佔滿下面；測試可以換成自己的版面（第 10 條）。視窗的大小或 scale 改變、增減網頁 view 時重新算。
- **hit test**：版面的結果是一層平的清單，工具列在前，網頁 view 依序在後，後面的蓋住前面的（第 4 條）；`SurfaceLayer` 疊的順序也是這一份。輸入是視窗的座標（CSS px），輸出是點到的目標（工具列，或哪一個網頁 view）和目標自己的座標：（視窗座標 × scale − 目標起點的 physical px）÷ scale。沒有目標時，事件不送給任何網頁（第 5 條）。`area_at` 和 `in_page` 合併成它；`page_to_window` 改成「網頁 view → 視窗」。
- **滑鼠**：
  - 按下左鍵時 capture 記住目標，之後的移動和放開都給它；那個網頁 view 被拿掉時放掉 capture（第 7 條）。
  - 滾輪給指標下面的目標，不看 capture（第 6 條）。
  - 指標離開一個網頁 view 時不通知它（第 9 條）。
- **鍵盤 focus**（第 8 條）：在網頁 view 上按下任何按鈕，它就拿到 focus，按鍵送給有 focus 的網頁 view。網址列有 focus 時照舊給網址列，點網頁時網址列失去 focus；點工具列的按鈕不改網頁的 focus。有 focus 的網頁 view 被拿掉時，focus 給第一個網頁 view。
- **renderer 的訊息**帶著 `TabId` 交給 `BrowserView`，送到顯示那個 `WebContents` 的網頁 view（例如游標）；網址列和視窗標題只跟著目前的網頁（第 7 條）。
- **只動 `shell` 和 `gfx`**：renderer 的 compositor 和 Viz 不受影響，見 `details.md` 的「和其他部分的關係」。

### 測試

- 測試專用的版面放兩個網頁 view，第二個在右下、起點不是整數的 CSS px，在 1x、1.5x、2x 檢查：
  - 畫的位置（讀像素）和送給 renderer 的大小都等於 bounds；
  - 點擊和滾輪送到對的網頁 view，座標是它自己的；
  - 在一個網頁 view 按下、拖到另一個放開，移動和放開都給按下的那個；
  - 游標跟著指標下面的網頁 view，按鍵給最後按下的網頁 view；
  - 拿掉網頁 view 時放掉 capture、focus 回到第一個；
  - 兩個網頁 view 之間的空隙，點了不送給任何網頁。
- 預設的版面：現有的 `the_toolbar_is_over_the_page`、`clicks_in_the_page_are_below_the_toolbar` 補上 1.5x；`tests/scenarios/` 的情境照舊是回歸檢查（畫面上看不出差別）。
- 動到視窗那一層（`window.rs` 傳 `TabId`），跑 `test.py automation`。

### 分階段

一段做完（第 11 條），當成只動 `shell` 和 `gfx` 的獨立改動，在加任何新的 UI 區塊（書籤列、側邊欄）、分頁或第二個網頁之前。

## 缺點

- 現在只有一個網頁、固定在工具列下方，這個改動在畫面上看不出差別，只換來之後改版面時不會出錯；測試要靠測試專用的版面才測得到多個網頁。
- 不含版面管理和完整的 focus 管理：UI 真的變複雜時（可以拖拉的側邊欄、多個能輸入的元件）還要再往 A3 走一步。
- bounds 用 physical px：網頁 view 的位置換成 CSS px 可能是小數（1.5x 時 37px 是 24.67 CSS px），版面要自己取整（第 3 條）。

## 理由和其他做法

| | A1 | A2 | A3 | Chromium |
| --- | --- | --- | --- | --- |
| 每個網頁的 bounds 只有一份，畫、大小、座標都從它推 | ✓（只有一個網頁） | ✓ | ✓ | ✓ |
| 一個 hit test 決定點到誰、換成誰的座標 | ✓ | ✓（平的清單） | ✓（巢狀的樹） | ✓（aura 的 window 樹） |
| 多個網頁 | ✗ | ✓ | ✓ | ✓ |
| capture 跟著目標 | 部分 | ✓ | ✓ | ✓ |
| 巢狀 view、版面管理 | ✗ | ✗ 版面是一個 `BrowserViewLayout` | ✓ | ✓（views） |
| 鍵盤 focus 管理 | ✗ | 只有「按下就拿到 focus」 | ✓ | ✓ |
| views / aura 分兩層 | ✗ | ✗ | ✗ 合成一層 | ✓ |
| 無障礙、拖放、動畫、主題、平台原生的 view | ✗ | ✗ | ✗ | ✓ |

- **A1. 單一的 `page_rect()`**：網頁放在任何位置都對，但只有一個網頁；有多個時要再改一次。
- **A2. 照 Chromium 的形狀，只做需要的部分**：「詳細設計」那一份。之後往 A3 走時不白做：網頁 view 就是 `views::WebView`，hit test 就是 view 樹的事件分派，資料的形狀不變。
- **A3. 通用的 view 樹**（像 `views`：巢狀、版面管理、focus 管理），仍然把 views 和 aura 合成一層。

2026-10-06 選 A2（第 1 條）：A1 只差「清單」和「capture 記目標」，A2 就能涵蓋任何位置和多個網頁；A3 的版面和 focus 管理要等 UI 真的變複雜才用得上，現在做了也沒有東西能測。

## 先例

Chromium 在 Windows、Linux、ChromeOS 上分兩層：views 是瀏覽器 UI 的 view 樹，aura 是比它低一層的視窗樹；網頁是一個 `aura::Window`（`RenderWidgetHostViewAura`），它的 bounds 是唯一的來源，送給 renderer 的大小和 surface layer 的位置都從它推，滑鼠事件沿著 window 樹 hit test、換成網頁座標。版面只決定每個 view 的 bounds，網頁本身不知道自己在視窗的哪裡；一個視窗有好幾個網頁也是同一套。細節和逐項比較見 `details.md` 的「Chromium 怎麼做」。

## 和 Chromium 不同的地方

| 項目 | kun | Chromium | 之後 |
| --- | --- | --- | --- |
| bounds 的單位（第 3 條） | 整數的 physical px；畫的位置和 hit test 完全對齊 | 整數的 DIP，layer 另外對齊 physical pixel（`SnapLayerToPhysicalPixelBoundary`）；非整數倍率下畫的位置和 hit test 最多差不到一個像素 | 不打算改；要改的話見「之後的可能」 |
| hit test 的結構（第 4 條） | 一層平的清單 | aura 的 window 樹、views 的 view 樹 | A3 |
| 滾輪（第 6 條） | 每次都給指標下面的目標 | scroll latching：一串滾動固定給第一個目標 | 之後 |
| 指標離開網頁（第 9 條） | 不通知 renderer | 送 mouse leave，網頁有 `mouseout`，`:hover` 消失 | 之後，和 `:hover` 一起 |
| 鍵盤 focus（第 8 條） | 只有「按下就拿到 focus」 | views 的 `FocusManager`：Tab 鍵切換、focus 的外框、順序 | A3 |

## 已決定的事項

1. **選 A2，現在做**（2026-10-06）：排在分頁和任何新的 UI 區塊之前。不選 A1：不能有多個網頁，分頁和側邊欄的網頁要再改一次。不選 A3：版面和 focus 管理現在沒有東西能測。
2. **網頁 view 指向 `WebContents`，不擁有它**（2026-10-06）：`WebContents` 由 `BrowserView` 的清單擁有，網頁 view 用 `TabId` 指向。和 Chromium 一樣（`views::WebView::SetWebContents`；`WebContents` 由 `TabStripModel` 擁有），分頁時只要換掉網頁 view 指向的網頁。不選「網頁 view 擁有 `WebContents`」：分頁時要把網頁搬進搬出。
3. **bounds 用整數的 physical px**（2026-10-06）：新增 `gfx::Rect`。畫的位置、送給 renderer 的大小、hit test 三者完全對齊，不用另外對齊；工具列本來就這樣做（`toolbar_px()`）。已知的結果：網頁 view 的位置換成 CSS px 可能是小數，版面要拿 scale 自己取整。不選 Chromium 的做法（整數的 DIP，layer 再用 `SnapLayerToPhysicalPixelBoundary` 對齊）：多一步對齊，非整數倍率下畫的位置和 hit test 最多差不到一個像素，送給 renderer 的大小也要另外取整；記在「之後的可能」。不選小數的 CSS px：網頁的起點落在半個像素上，整頁會糊。
4. **hit test 是一層平的清單**（2026-10-06）：後面的蓋住前面的，`SurfaceLayer` 疊的順序也用同一份。Chromium 是樹（aura 的 `WindowTargeter` 從上層的子 window 找起）；現在沒有巢狀的 view，做成樹也沒有東西能測。之後改成樹只動 hit test 的內部和版面的輸出，對外的形狀（目標、目標自己的座標、capture 記目標）不變。
5. **沒點到任何目標時，事件不送給網頁**（2026-10-06）：例如版面留下的空隙。Chromium 是根 view 收到，也不轉給網頁，對網頁來說一樣。
6. **滾輪給指標下面的目標**（2026-10-06）：和現在一樣，不看 capture。Chromium 有 scroll latching，一串滾動固定給第一個目標；要先定義一串滾動什麼時候開始和結束（touchpad 的 phase、滾輪的計時），記在「之後的可能」。
7. **capture、游標、renderer 的訊息跟著網頁 view**（2026-10-06）：capture 記住是哪個目標，那個網頁 view 被拿掉時放掉；每個網頁 view 記自己的 renderer 要的游標；renderer 的訊息帶著 `TabId` 交給 `BrowserView`。和 Chromium 一樣（aura 的 capture、`RenderWidgetHostView` 各自的游標、IPC 按 routing id 送到 `RenderWidgetHost`）。
8. **在哪個網頁 view 按下，focus 就給它**（2026-10-06）：任何按鈕都算，按鍵送給有 focus 的網頁 view；一開始和有 focus 的網頁 view 被拿掉時，給第一個網頁 view。和 Chromium 按下時改 focus 一樣（aura 的 `FocusClient`）；完整的 focus 管理（views 的 `FocusManager`）屬於 A3。不選「按鍵照舊給唯一的網頁」：有兩個網頁 view 時，按鍵的去處是任意的。
9. **指標離開網頁 view 時不通知 renderer**（2026-10-06）：Chromium 送 mouse leave（網頁有 `mouseout`，`:hover` 消失）；kun 的 protocol 沒有這個事件，renderer 也還沒有 `:hover`，現在加了測不到效果，記在「之後的可能」，和 `:hover` 一起做。
10. **版面是可以換的 `BrowserViewLayout`**（2026-10-06）：對應 Chromium 的 `BrowserViewLayout`。預設是工具列在上、第一個網頁 view 在下；測試用自己的版面放兩個網頁 view，在 1x、1.5x、2x 檢查。兩個網頁 view 現在只在測試裡用。
11. **一段做完**（2026-10-06）：只動 `shell` 和 `gfx`（`Rect`），`window.rs` 把 renderer 訊息的 `TabId` 傳下去；renderer 和 Viz 不動。
12. **瀏覽器 UI 長期的方向不在這份 RFC 決定**（2026-10-06）：原本列在「待決定」。`rfcs/pre-rfcs/P2.md` 的「已決定的事項」第 2 條已經把 HTML / CSS 的 UI 定為 MVP 之後的目標；A2 的網頁 view 和 hit test 不管走哪個方向都用得上。
13. **情境點不到視窗外的元素**（2026-10-06）：做第 5 條時發現，`demo-script` 和 `wasm-demo` 一直在點視窗下緣以外的元素：原本的 `area_at` 把工具列以下的任何一點都當成網頁，連視窗外也是。現在情境的 `click <selector>` 和 `wheel <selector>` 在元素的中心不在網頁上時直接失敗，說明要把視窗開大或先捲動；這兩份情境改用 `window size=800x800`。不選「先把元素捲進視窗再點」（Puppeteer、Playwright 的 click，CDP 的 `DOM.scrollIntoViewIfNeeded`）：要多一個 renderer 的 automation 訊息，記在 RFC 0006 的「之後的可能」。不選「讓視窗外的點也算網頁」：違反第 5 條，人也點不到那裡。

## 待決定

無。

## 之後的可能

- **A3**：UI 變複雜時，加上巢狀 view、版面管理和完整的 focus 管理。平的清單改成樹，只動 hit test 的內部和版面的輸出（第 4 條）。如果瀏覽器 UI 改成 HTML / CSS，視窗這一層可能一直是平的，巢狀交給 UI 網頁自己的 DOM 和 hit test。
- **bounds 改成 Chromium 的做法**（第 3 條）：整數的 DIP，layer 另外對齊 physical pixel。如果版面之後想直接用整數的 CSS px 來想，可以改成這樣；代價是多一步對齊（`SnapLayerToPhysicalPixelBoundary`），非整數倍率下畫的位置和 hit test 最多差不到一個像素，送給 renderer 的大小要另外取整（Chromium 無條件進位，可能蓋到隔壁一個像素），1.5x 的測試要容許差一個像素。
- **scroll latching**（第 6 條）：一串滾動固定給第一個目標，即使指標途中移到別的網頁 view 上。
- **mouse leave**（第 9 條）：和 `:hover`、`mouseout` 一起做。
- **分頁**：RFC 0011（2026-10-06 完成）。`TabStripModel` 擁有 `WebContents`，切換分頁時換掉網頁 view 指向的 `WebContents`（第 2 條）。
- **HTML / CSS 的瀏覽器 UI**（`rfcs/pre-rfcs/P2.md` 的「已決定的事項」第 2 條，`rfcs/pre-rfcs/MVP.md` 列為 MVP 之後的目標）：那時工具列本身也是一個網頁，「視窗裡有好幾個網頁、各自在某個位置」會變成常態，A2 的網頁 view 和 hit test 正好是它的基礎。
- **多個網頁共用一個 renderer process**：屬於 process 模型的題目，見 `details.md` 的「和其他部分的關係」。
