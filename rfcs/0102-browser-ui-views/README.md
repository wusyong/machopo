# RFC 0102：瀏覽器 UI 的 view 樹和可以換的版面

- 開始日期：2026-11-16
- 狀態：🚧 已排程：2026-11-16（第 1、2 階段做完；第 3 階段進行中：3a、3d 做完，3b 延後，接著 3c）
- 細節：`details.md`；進度、驗收和做過的檢查：`progress.md`

## 摘要

瀏覽器的 UI（分頁列、工具列、網址列，以及之後的側邊欄）長期由 machopo 自己畫，不換成 HTML / CSS。為了讓它能有不同的擺法和外觀（例如 Zen 那樣的直式側邊欄），做一個自己的 UI 框架，放在新的 crate `ui`：底層是保留的 view 樹（巢狀的 view、版面管理、事件的分派和 capture、主題），上面是宣告式的一層（寫一個「狀態 → UI」的函式，`ui` 比對前後兩次、只改有變的 view）。`ui` 不知道分頁、`WebContents`、網址列是什麼，設計照我們的目標比較，不要求對應 Chromium 的 Views；瀏覽器的元件留在 `shell`，建在它上面。現在橫式的分頁列和工具列留著，直式的側邊欄是另一種版面（另一個函式），兩種用同一批元件。

## 動機

**起因**（2026-11-16）：想讓 machopo 的 UI 和工具列可以設計成像 Zen 那樣（左邊直的側邊欄放分頁、網址列和上一頁等按鈕，網頁佔右邊），而且之後還能有其他擺法。Zen 的樣子見 `details.md` 的「Zen 的版面」。

**現況**：
- 工具列（`crates/shell/src/toolbar.rs`、`omnibox.rs`）和分頁列（`tab_strip.rs`）各自用常數算每個部分的矩形、自己判斷點到哪裡、自己寫 paint op。按鈕和網址列綁死在 `Toolbar` 裡，不能單獨搬到別的地方。
- `BrowserViewLayout`（`browser_view_layout.rs`）可以換，但它的輸出是寫死的兩個欄位 `tab_strip`、`toolbar`；`BrowserView`（`browser_view.rs`）的 `hit_test`、`update()`、滑鼠和按鍵的分配都分這兩種情況寫，`update()` 算 layer 的大小時還直接用了 `tab_strip::HEIGHT`、`toolbar::HEIGHT`，預設它們在上面。
- 顏色和大小是各檔案裡的 `const`，只有淺色。
- P2 把這套 UI 定為過渡：之後要換成 HTML / CSS 的 UI（`rfcs/pre-rfcs/P2.md` 的「已決定的事項」第 2 條）。RFC 0002 比較過通用的 view 樹（它的 A3），當時「沒有東西能測」而延後；直式側邊欄（直的清單、捲動、收合）正是它要的東西。

**目標**：
- 一棵通用的 view 樹，元件可以巢狀、由版面管理排位置；事件照樹分派。
- 元件（上一頁等按鈕、網址列、橫的分頁列、直的分頁清單）不知道自己擺在哪裡；版面決定擺法。加新的版面或元件時不必改 `BrowserView`。
- 橫式（現在的樣子）和直式的側邊欄兩種版面。
- 顏色和大小集中成主題，可以有深色。
- view 樹的 crate 不依賴瀏覽器的東西，以後有可能拿出來成為 machopo 的 GUI API（現在不承諾，見「已決定的事項」第 2 條）。

**不在範圍內**：
- 公開、穩定的 GUI API（之後另開 RFC）。
- 完整的鍵盤 focus 管理（Tab 鍵走過每個按鈕和分頁）、無障礙、動畫、拖拉排序分頁：見「之後的可能」。
- Zen 的常用網站方塊、資料夾、工作區：版面和元件做好之後，各自是新的元件。
- 網頁的 hit test 和 renderer：不動。網頁在視窗裡的位置照 RFC 0002 的網頁 view。

**要解決的 `docs/limitations.md` 的限制**（「工具列和網址列」）：
- 「這是過渡的原生 UI，之後要換成能用 HTML / CSS 自訂的。」：方向改了（「已決定的事項」第 1 條），做完時改寫成現在的說法。
- 「工具列的顏色和大小是寫死的，不跟著系統的深色模式和字型大小。」：顏色和深色模式（第 3 階段）；字型大小不在範圍內。
- 「視窗的版面只有一層：……沒有巢狀的 view 和版面管理」那一條的前半：巢狀的 view 和版面管理。Tab 鍵的 focus、mouse leave、scroll latching 不在範圍內。
- 「分頁列在系統的視窗框（標題列）下面……分頁太多……超出右邊的看不到、點不到」那一條的一部分：側邊欄畫進標題列、直的分頁清單可以捲動（第 3 階段）；橫式分頁列的溢出照舊。

## 設計概要

給之後改 `shell` 的人：

- **UI 從狀態來**：`shell` 寫一個函式，從瀏覽器的狀態（分頁和它們的標題、目前的分頁、能不能上一頁、網址）描述 UI 長怎樣。狀態變了就再呼叫一次，`ui` 比對前後兩次，只改有變的 view：關掉一個分頁時，分頁清單只拿掉那一個。不必像現在那樣記得呼叫 `sync_toolbar`、`set_tabs`。
- **加一個元件**，例如常用網站的方塊：在 `shell` 寫一個 view，實作畫、想要的大小、滑鼠事件；點擊時送出一個動作（例如「開這個網址」），由 `BrowserView` 執行。它不知道自己在視窗的哪裡，也不碰 `BrowserView` 的 hit test。
- **加一種版面**：再寫一個描述 UI 的函式，決定有哪些容器、元件放在哪個容器、容器怎麼排（橫的一列、直的一欄、佔滿剩下的空間）。橫式是「分頁列一列、工具列一列（按鈕和網址列）、網頁」；側邊欄是「左邊一欄（按鈕、網址列、直的分頁清單）、右邊網頁」。同一個網址列元件在兩種版面裡都用。
- **換外觀**：改主題的值；深色和淺色是兩份主題。

使用 machopo 的人：預設的視窗和現在一樣是橫式；用命令列選項（之後是設定）切換成側邊欄。

## 詳細設計

### 架構

```
shell（瀏覽器的 UI，//chrome/browser/ui）
  BrowserView：分頁、網頁 view、Command
  元件：導覽按鈕、網址列、橫的分頁列、直的分頁清單……
  版面：橫式、側邊欄（各是一個「狀態 → UI」的函式）
        │ 建在
        ▼
ui（新的 crate）
  宣告式的一層：描述、比對（key）、把差別套到樹上
  保留的樹：view、bounds、版面管理、畫、hit test、事件的分派和 capture、主題
        │
        ▼
gfx、compositor_paint、parley（UI 的文字，現在的 ui_text 搬過來）、taffy（排版的協定）
```

- **`ui` 不依賴瀏覽器的東西**：不依賴 `shell`、`browser`、`protocol`、`compositor`、`viz`；不依賴 winit（事件型別自己定，`shell` 的 `window.rs` 轉換）。見「已決定的事項」第 4 條。
- **畫**：`ui` 交出一份 layer 清單（`ui` 自己的型別，第 1 階段裡面只有一個），每個 layer 帶 display list 和變了的範圍；`shell` 轉成 compositor 的 `PictureLayer`，現在那個 layer 的 `invalidation` 從 `None` 變成變了的範圍。網頁照舊是 `SurfaceLayer`，renderer 的 frame 不經過 view 樹。見「已決定的事項」第 9 條。
- **網頁 view**：Chromium 的 `views::WebView` 是樹裡的一個 view。第 1 階段網頁 view 留在樹外，照 RFC 0002 的平的清單；第 2 階段才決定要不要放進樹（版面要同時排 UI 和網頁時需要）。
- **`<select>` 的下拉清單、sad tab**：第 1 階段不動，照舊是另外的 picture layer。

### 測試

- **`ui` 自己的測試**：巢狀的 hit test 和座標換算（1x、1.5x、2x）；事件往上找處理的 view；capture 跟著按下的 view、那個 view 被拿掉時放掉；改了 view 時要重畫；版面管理的排法。
- **只重畫變了的部分的交叉檢查**（`docs/testing.md` 的「快取和增量的交叉檢查」的做法）：`ViewTree::stale_views` 讓每個 view 重錄一次，和留著的比較，不一樣的就是改了卻沒要求重畫的。debug build 的每次 paint 都做一次、不一樣就 panic（1b 加的，`ViewTree::set_check_recordings`），所以 `shell` 的每個測試和情境都檢查；`ui` 自己數 paint 次數的測試關掉它，另外明確地檢查。layer 是從留著的錄製組出來的，留著的都對時，它就是整份重畫的結果，所以比較每個 view 就夠了。變了的範圍（舊位置、新位置、拿掉和隱藏的）由 `ui` 的單元測試一種一種檢查。原本打算每次都整份重畫、比較 raster 的結果（2026-11-16 改）：layer 本來就是錄製組成的，比 raster 只會重複檢查同一件事，還要 `ui` 的測試依賴 `raster`。
- **第 1 階段是重構**：外觀和行為不變。`shell` 現有的單元測試（`toolbar`、`tab_strip`、`browser_view_layout`）、整合測試的 `browser_view`、`windowless` 和情境的 `tab-strip`、`navigation` 守住；測試裡寫死座標的地方照新的程式改寫，但數字不變。
- **第 2 階段之後**：側邊欄的版面在 1x、1.5x、2x 的位置；直的分頁清單的點擊、新分頁、關分頁寫成情境。
- **只能手動確認的**：畫進標題列、深色模式跟著系統（第 3 階段），記進 `docs/test-gaps.md`。
- 每一段照 `CLAUDE.md` 跑 mutation test。

### 分階段

每個階段開始前只決定那個階段的事。

1. **`ui` 的保留的樹，工具列和分頁列搬上去**：view 樹、bounds、畫、hit test、事件的分派和 capture、主題（只有現在的淺色）。`Toolbar`、`TabStrip` 改寫成 view，`BrowserView` 的 `hit_test`、`update()`、輸入的分配改成走樹。外觀和行為不變。
2. **宣告式的一層、版面管理和側邊欄**：描述和比對、容器的排法（版面管理）、外觀和排版用什麼描述、網頁 view 要不要進樹、按鈕組和網址列拆成獨立的元件、直的分頁清單、側邊欄的版面、切換的方式。橫式的 UI 也改成用函式描述。預設照舊是橫式。
3. **外觀**：深色主題、跟著系統的深淺色、側邊欄畫進標題列（macOS 的紅黃綠按鈕疊在側邊欄上）、側邊欄收合、直的清單捲動。

## 缺點

- **要長期維護一套自己的 UI 框架**：Views 的功能（focus 管理、無障礙、動畫、拖放、捲動）都要自己一點一點補。
- **改外觀要寫 Rust、重新編譯**：使用者不能像 Zen、Firefox 那樣用 CSS 改 UI。主題的 token（第 10 條）讓之後能從檔案覆蓋顏色和大小；用 machopo 自己的 CSS 引擎描述外觀是第 2 階段的選項之一。
- **P2 的 HTML UI 方向不走了**：讓 UI 頁面呼叫 Browser 的特權通道、UI 用的 renderer 都不會為 UI 而做（`chrome://` 之類的設定頁之後還是可能用網頁做，那是另一件事）。
- **第 1 階段是純重構**：使用者看不到變化，但改到 `BrowserView` 的輸入分配和畫，要靠現有的測試守住。

## 理由和其他做法

| | 1. 加第二套，用 enum 切換 | 2. 拆成元件，版面回傳清單 | 3. view 樹（選這個） | B. HTML / CSS 的 UI |
| --- | --- | --- | --- | --- |
| 橫式和側邊欄並存 | ✓ | ✓ | ✓ | ✓ |
| 元件可以在版面之間搬 | ✗ | ✓ | ✓ | ✓ |
| 巢狀、版面管理 | ✗ | ✗ 版面自己算矩形 | ✓ | ✓（CSS） |
| 第三種版面的成本 | 每處都多一個分支 | 新的版面 | 新的版面 | 新的 HTML / CSS |
| 使用者自己改外觀 | ✗ | ✗ | 之後：主題的 token 從檔案覆蓋，或第 2 階段選 CSS 描述 | ✓（CSS） |
| 畫一次的成本 | 一樣 | 一樣 | 多一次排版（十幾個 view，微秒級） | 多一個 renderer（V8、DOM、style、layout）的記憶體和啟動時間 |
| 成為 GUI API 的可能 | ✗ | ✗ | ✓ | 另一種（像 Electron） |
| 多久看得到側邊欄 | 最快 | 多一個階段 | 多兩個階段 | 很久：要先有 UI renderer 的 `<input>` 編輯和特權通道 |

- 2026-11-16 選 3（「已決定的事項」第 1 條）。2 的彈性夠撐橫式和側邊欄，但它是「之後換成 HTML UI」的前提下的折衷；長期用自己畫的 UI 時，巢狀和版面管理早晚要做，3 直接做。
- 不選 B：效能（多一個 renderer）、要很久才看得到；長期的 UI 選了自己畫。
- 完整的比較和效能的說明見 `details.md` 的「做法的比較」。

## 先例

- **Chromium**：Chrome 的 UI 是 `//ui/views` 的 view 樹，畫成 Browser 的 compositor 的 layer，網頁是 `views::WebView` 嵌入的 surface。`//ui/views` 和瀏覽器的 `//chrome/browser/ui` 分開，ChromeOS 的桌面（ash）也用它；它不是對外的 API。平板上的分頁列有 WebUI 的版本（`WebUITabStripContainerView`），和 Views 的分頁列看同一個 `TabStripModel`。逐項對照見 `details.md` 的「Chromium 的 views」。
- **Firefox、Zen**：UI 是 HTML / CSS / JS，所以容易改外觀（Zen 的主題和 mod）。Firefox 的 XUL 曾經是對外的 GUI toolkit（Thunderbird 等），後來放棄、改回 HTML。
- **宣告式的 UI**：SwiftUI、Flutter（widget 描述 → element 比對 → render object）、React；Rust 的 Xilem 是宣告式的一層，建在保留的 widget 樹 Masonry 上，和這份 RFC 的兩層同形。Xilem 用泛型把整棵 UI 寫進型別，這份 RFC 用動態型別（「已決定的事項」第 6 條）。
- **Rust**：Masonry / Xilem 用的是同樣的 Vello、Parley；Dioxus 的 Blitz 用 Taffy、Parley、Vello 畫 HTML / CSS；egui 是 immediate mode。

## 已決定的事項

1. **長期的瀏覽器 UI 由 machopo 自己畫，做一棵 view 樹**（2026-11-16）：選「理由和其他做法」的 3。取代 `rfcs/pre-rfcs/P2.md` 的「已決定的事項」第 2 條「之後讓 UI 能用 HTML / CSS 自訂」的長期目標（RFC 0002 的第 12 條把這個方向留給別處決定，就是這裡）。理由：自己畫的 UI 不多一個 renderer，記憶體和啟動比 HTML UI 省；Chrome 的 UI 也是這條路；view 樹之後有機會成為 machopo 的 GUI API。不選 2：它的前提是之後換成 HTML UI。不選 B（HTML UI）：見上一條理由，而且要很久才看得到側邊欄。代價見「缺點」。
2. **view 樹放在新的 crate `ui`，不依賴瀏覽器的東西，現在不承諾公開 API**（2026-11-16）：位置像 Chromium 的 `//ui/views`（和 `//chrome` 分開），內部的設計照第 5 條。瀏覽器的元件和版面留在 `shell`。這樣幾乎不多花成本，以後要拿出來成為 GUI API 時不必再拆；真的要公開時，穩定的 API、無障礙、瀏覽器以外的輸入法和剪貼簿、文件另開 RFC。不選「寫在 `shell` 裡」：和瀏覽器的東西纏在一起，以後拆很痛。
3. **橫式的分頁列和工具列留著，側邊欄是另一種版面**（2026-11-16）：不覆蓋現在的樣子，兩種版面用同一批元件，之後可以有更多種。
4. **`ui` 的邊界**（2026-11-16，第 1 階段的 Q1）：
   - 只依賴 `base`、`gfx`、`compositor_paint`、`parley`（`shell` 的 `ui_text.rs` 搬進來）和 `taffy`（排版的協定，第 12 條；2026-11-16 補上）。不依賴 `shell`、`browser`、`protocol`（瀏覽器的東西）、`compositor`（樹只交出 display list，layer 是 `BrowserView` 的事；個別 view 要自己的 layer 時再加，Q4）和 winit。
   - 滑鼠、按鍵和輸入法的事件型別在 `ui` 裡自己定，按鍵的形狀照 winit 的 `Key`（列舉，不是字串）；`shell` 的 `window.rs`、`keyboard.rs` 轉換。快捷鍵（`command_for`）留在 `shell`、照舊用 winit 判斷，是快捷鍵的不進樹。對照 Chromium 的 `//ui/events`（`ui::KeyEvent`，平台的事件都先轉成它）。不選直接用 winit 的型別：省一個轉換，但 `ui` 綁死 winit，違背第 2 條留的路。不選 `protocol` 的 DOM 事件：依賴瀏覽器的 crate，鍵要用字串比對。
   - 網址列用的 `Clipboard` trait 跟著網址列搬到 `ui`，實作（`arboard`）留在 `shell` 傳進來（Chromium 的 `ui::Clipboard` 在 `//ui/base`）。
5. **`ui` 的設計照我們的目標比較，不要求對應 Chromium**（2026-11-16）：比較的標準是彈性（換版面、加元件、換外觀）、效能（閒著時、狀態變了時、記憶體）、實作和維護的成本、當成 GUI API 好不好用、好不好測、不靠 `Rc` / `RefCell`；Chromium 和其他實作（SwiftUI、Flutter、Masonry / Xilem、egui……）都只是參考。這是 `CLAUDE.md`「型別和模組盡量對應 Chromium 的名字和分層」在 `ui` 的例外：自己的 GUI 框架沒有理由照搬 Views，更好的做法就用更好的。`shell` 這一側（`BrowserView`、`TabStripModel`、`BrowserViewLayout`）照舊對應 Chromium。
6. **宣告式的 UI，疊在保留的樹上**（2026-11-16，Q0）：寫 UI 的人寫一個「狀態 → UI 長怎樣」的函式，`ui` 比對前後兩次的結果，只改保留的樹裡有變的 view（SwiftUI、Flutter、React、Xilem 的做法）。分兩層：保留的樹（view、事件、畫）是底層，宣告式的那一層建在它上面。比對用動態型別（`Box<dyn …>` 加 key），不學 Xilem 用泛型把整棵樹寫進型別：我們的 UI 只有幾十到幾百個節點，動態比對的成本可以忽略，編譯時間和程式複雜度都省很多。理由：換版面只是換一個函式；畫面只從狀態來，沒有「忘了同步」的 bug（現在的 `sync_toolbar`）；分頁清單照分頁的 id 比對；當成 GUI API 是現在主流的寫法。不選只做保留的樹、命令式地改（Views、AppKit、GTK、Qt）：最便宜，但狀態和畫面要手動同步，換版面要拆掉重建。不選 immediate mode（egui、Dear ImGui）：巢狀的排版常要用上一幀的大小，網址列的編輯狀態、捲動位置和之後的無障礙都要另外用 key 記。比較見 `details.md` 的「Q0. 寫 UI 的方式」。
7. **保留的樹用 arena 加帶世代的 id 存 view**（2026-11-16，Q2）：節點（`Box<dyn View>`、bounds、父、子的 id、要不要重畫）放在一個 `Vec` 裡，id 是位置加世代數；拿掉的位置給新的 view 重用時世代數加一，舊的 id 拿不到新的 view。世代數是 u32，到上限時那個位置不再重用（同一個位置重用約 40 億次才發生，只浪費一格）。view 的行為和樹的結構分開存，呼叫一個 view 時兩者分開借，不靠 `Rc` / `RefCell`。理由：宣告式那一層比對完用 id 直接插入、拿掉、搬動 view；capture 的 view 被拿掉時 id 查不到，不用另外處理；往上找父 view 是查表；第 2 階段的版面如果用 taffy，它的 `TaffyTree` 也是 arena 加 id，可以一對一對應。不選父 view 擁有子 view 的 `Box`（Views、AppKit）：capture 記路徑，樹一改就可能指錯，往上找父 view 只能靠遞迴返回，比對要和描述同步走。不選照樹的順序攤平在一個 `Vec`：走訪最快，但插入、拿掉要搬後面所有節點，id 到位置的對照要另外維護。不選用 map 存、id 永遠不重用（`SequenceNumber<u64>`）：更簡單，但查找要算 hash、節點不連續；現在看不出差別，當成 GUI API 時上千個節點的清單用 `Vec` 比較好。
8. **事件的分派；點了之後送出訊息（Elm 架構）**（2026-11-16，Q3）：
   - 滑鼠：hit test 找點下面最深的 view，不處理就往父 view 找；按下時處理的 view 成為 capture，收之後的移動和放開；hover 的進出比較前後兩次的目標。沒有 DOM 的捕獲階段（從上往下先走一遍），瀏覽器 UI 用不到，之後要加不影響其他部分。按鍵給有 focus 的 view，不處理就往上；第 1 階段的順序照舊（快捷鍵、網址列、網頁）。
   - 結果：view 處理事件時往 context 送出一個訊息，樹帶訊息的型別參數（`shell` 用 `Command`）；分派完由使用的一方一個一個處理，狀態變了就重新描述 UI（第 6 條），資料只往一個方向流。理由：使用者能做的事本來就是 `Command`，按鈕和快捷鍵走同一條路；能做的事都列在 enum 裡；測試只要斷言送出了哪個訊息。元件內部的狀態（網址列的打字、選取、游標，hover、按下的樣子）留在 view 自己，不送訊息；網址列只在按 Enter 時送出導覽。不選描述 UI 時附上拿 `&mut 狀態` 的 closure（Xilem、SwiftUI、React 的 `onClick`）：寫起來最順手，但分派時樹正被借著，狀態要從 `BrowserView` 拆出來，能做的事散在各個 closure；當成 GUI API 時可以在訊息上加一層 closure 轉訊息的便利寫法。不選 view 自己拿瀏覽器的參考去改（Views 的 `PressedCallback`）：Rust 裡借用不成立。不選 `Box<dyn Any>` 的訊息：丟掉型別檢查。
9. **一個 layer，只重錄、只重 raster 變了的 view；留好個別 layer 的路**（2026-11-16，Q4）：每個節點快取自己錄好的 display list，只有 view 要求重畫時才重錄；組合時沿用沒變的部分，變了的 view 的舊位置和新位置（view 搬動時兩個都算）交給 `PictureLayer` 的 `invalidation`，compositor 只重 raster 碰到的 tile（網頁的 layer 已經這樣用）。`ui` 交出的是一份 layer 清單（`ui` 自己的型別，第 1 階段只有一個），個別 view 要自己的 layer（捲動的清單只改位移、動畫）時第 3 階段再加，不改 view 的 API。漏算範圍（畫面沒更新）由交叉檢查守住（「測試」）。理由：側邊欄是整個視窗高，每次 hover 整片重 raster 很浪費；而且「只重畫變了的」要 view 回報自己變了，從整份重畫改過來要回頭改每個 view，所以第 1 階段的重構就定好這個形狀。不選整份重畫（現在）：見上一句。不選每個 view 都有 layer（iOS 的 UIView 和 CALayer）：layer 太多，合成和記憶體都浪費。對照：Blink 的 display item 快取和 raster invalidation；Flutter 的 `RepaintBoundary`、Views 的 `SetPaintToLayer` 是第 3 階段那一步。
10. **主題是具名的 token**（2026-11-16，Q5）：顏色和大小（圓角、間距、字的大小）各是一個 token 的 enum（例如 `ColorToken::ToolbarBackground`），每份主題是一張以 token 為索引的表，用 `match` 建，少一個 token 就編不過；畫的時候從 context 讀，是陣列索引。token 是 app 的（`shell` 的 `ColorToken`），不是 `ui` 的：`ui` 不知道工具列是什麼（第 2 條）。`ui` 只提供做法：`tokens!` 宣告 token 的 enum，`TokenTable` 是表；app 的主題型別由 `App` trait 帶進樹（和訊息的型別同一個參數，第 8 條），context 的 `theme()` 拿到它（2026-11-16，做 1a 時定的形狀；原本的寫法 `theme.color(ColorToken::ToolbarBackground)` 像是 `ui` 擁有 token）。第 1 階段只有一份內建的淺色主題，值和現在 `toolbar.rs`、`tab_strip.rs`、`omnibox.rs` 的常數一樣。理由：在 Rust 裡和具名欄位的 struct 一樣安全、一樣快；token 有名字，之後可以從檔案只覆蓋幾個值、照名字列出來，第 2 階段如果用 CSS 描述外觀，token 一對一對到 CSS 的自訂屬性（`--toolbar-background`）。不選具名欄位的 struct：從檔案讀、覆蓋、接 CSS 都要另外寫對照。不選把顏色寫在 UI 的描述裡（SwiftUI 的 modifier、Flutter 的元件參數）作為主題：深色要在每個用到的地方分；個別元件在描述裡覆蓋 token 的值，等第 2 階段描述的形狀定了再談。對照：design token、Chromium 的 `ColorId`（不做它的 mixer）。
11. **bounds 是 f32 的 CSS px，相對於父 view**（2026-11-16，第 1 階段的 1a）：相對於父 view：容器移動時（側邊欄收合）子 view 不用動，子 view 錄好的內容不帶位置、用 `push_list_at` 平移，容器移動時照樣沿用（第 9 條的快取）；hit test 往下走一層減一次父 view 的位置。f32 的 CSS px：和現在的工具列、分頁列、`PaintOp` 同一個單位，元件不必知道 scale。邊緣對齊 physical px 不靠型別保證，是排版的事：第 2 階段的版面管理算完位置後可以吸附到 physical px（Flutter、SwiftUI 也是小數的邏輯像素）；現在的分頁寬度本來就是小數。網頁 view 第 1 階段在樹外，照舊是整數的 physical px；第 2 階段如果進樹，那個節點把自己在視窗裡的位置取整（RFC 0002 的第 3 條），其他 view 不受影響。不選相對於視窗：容器移動時每個子孫都要改，錄好的內容也要重錄。不選整數的 physical px（網頁 view 的做法）：每個元件都要帶著 scale 計算、取整，現有的計算全部改寫，當成 GUI API 時寫元件的人也要處理。不選整數的 CSS px（Chromium Views 的 DIP）：除出來的寬度要取整，1.5x 時照樣不對齊。
12. **排版用 taffy 的低階介面當協定，每個節點選自己的演算法**（2026-11-16，第 1 階段的 1a）：arena（第 7 條）實作 taffy 的低階 trait（走訪子節點、存排好的結果、快取），和 `layout` crate 的 `BoxTree` 同樣的做法。每個節點決定怎麼排：容器用 taffy 的 flex、grid、block（`compute_flexbox_layout` 等），或自己寫（只排可見範圍的長清單、跟著動畫的位置）；葉子用 `compute_leaf_layout` 加自己的量測（按鈕的固定大小、文字的寬高）。結果由 `compute_cached_layout` 跨整棵樹快取，view 的內容影響大小時標記要重排、往上傳。寫元件的人：葉子只寫一個量測的函式；容器多半直接用 flex。1a 做了 flex、grid 和葉子；還沒有的（第一個要用的人來時再接）：block 的容器、`position: absolute`（taffy 的 fork 把 out-of-flow 的子節點交給包含它的區塊排，要實作它的 `LayoutContainingBlock`；1a 的測試用負的 margin 疊 view）、自己寫排法的容器（view 在排版時要能叫子 view 排，view 本身正被借著，要另外設計）。理由：flex、grid 現成，寬度決定高度、固有大小（min-content、max-content）是 taffy 的協定本來就有的；巢狀的 flex 共用一份快取，不會一層層重複量；自訂的排法照樣能寫；第 2 階段選 Rust 的規則、taffy 或 machopo 自己的 CSS 引擎都接得上（`layout` crate 也走同一套 taffy 介面）。不選父 view 的 `layout` hook 加「想要的大小」（Views 的 `Layout()`、AppKit、Android）：想要的大小不知道可用的空間（Views 後來也把 `CalculatePreferredSize` 改成接收 `SizeBounds`）。不選自己定的「限制往下、大小往上」（Flutter、SwiftUI）：每種容器自己寫；每個 flex 容器各自呼叫 taffy 時，巢狀的容器之間沒有共用的快取，量固有大小會一層層重複。不選每個 view 只帶樣式、taffy 排整棵樹：描述不了的排法沒有出口，而且提前決定了第 2 階段的「外觀和排版用什麼描述」。代價：taffy 的輸入（可用空間是確定值、min-content 或 max-content；只量大小或真的排）比較複雜，簡單的寫法要蓋住它；`ui` 多依賴 `taffy`。
13. **樹擁有一份 `FontContext` 和 `LayoutContext`，view 借用；樹外的 sad tab 和下拉清單也借同一份**（2026-11-16，第 1 階段的 1a）：view 在量測和畫的時候從 context 借；網址列的 `PlainEditor` 本來就在操作時才拿 context。sad tab、`<select>` 的下拉清單和樹都屬於 `BrowserView`，分開借欄位就能用同一份。理由：每建一個 `FontContext` 就重新掃一次系統字型（fontique 的 `SystemFonts::new()`），載入的字型資料也各存一份；現在每個視窗的網址列、分頁列、sad tab 各建一份，下拉清單每打開一次就建一份。測試字型（`test_fonts`）照舊由每棵樹（每個 `BrowserView`）選。外觀不變。不選每個 view 各自一份（現在）：見上。不選每個 view 拿一份 clone：掃描結果共用（裡面是 `Arc<Mutex>`），但字型資料和排版暫存各一份。不選全域的一份（`thread_local`）：同一個 process 裡用測試字型和用系統字型的測試會互相干擾，也是 `CLAUDE.md` 不要的隱藏共享狀態。
14. **從樹外面拿 view 用帶型別的 id**（2026-11-16，第 1 階段的 1a）：`tree.insert(Omnibox::new())` 回傳 `ViewId<Omnibox>`，`tree.get(id)` 拿到的就是 `&Omnibox`；`View` trait 有轉成 `Any` 的方法，樹在內部向下轉型。只有 `insert::<T>` 產生 `ViewId<T>`，位置重用時世代數會變（第 7 條），所以活著的 id 轉型不會失敗，拿掉的 id 拿到 `None`。結構上的操作（子 view、hit test 的結果、capture）用不帶型別的 `AnyViewId`，`ViewId<T>` 隨時轉得過去。用在 `BrowserView` 命令式地碰 view（換分頁時設網址、Cmd+L 讓網址列取得焦點）和測試；第 2 階段有了宣告式那一層，「狀態變了」改成重新描述，這個介面留給命令式的操作。不選不帶型別的 id、取用時指定型別：型別錯了到執行時才知道。不選只能送訊息給 view：每種 view 要有自己的訊息型別，樹要裝各種訊息又要轉型，第 2 階段的比對會取代它。
15. **從外面改 view 走 `tree.update(id, |view, ctx| …)`，view 自己判斷有沒有變**（2026-11-16，第 1 階段的 1a）：view 的方法收一個 context（`omnibox.set_url(url, ctx)`），有變才 `ctx.request_paint()` 或 `ctx.request_layout()`（重排之後也重畫）；事件處理時拿到的是同一種 context。唯讀的 `tree.get(id)`（第 14 條）給測試和查詢。理由：`BrowserView::sync_toolbar()` 在每個 renderer 訊息之後都跑，每次都重設網址和上一頁、下一頁的狀態，大多沒變；現在是在外面比較前後再設 `chrome_dirty`，改成 view 自己判斷，沒變就不重畫，外面不用比較。漏了 `request_paint` 由第 9 條的交叉檢查抓到。對照 Masonry 的 `WidgetMut`（改 widget 時帶著 context）。不選拿到 `&mut` 就當成變了：每個 renderer 訊息都重畫、重 raster 工具列。不選呼叫的一方自己標記（Views 的 `SchedulePaint()`）：容易漏，散在每個呼叫的地方。不選自動比較前後（React 的 `memo`）：每次都要比較狀態或重錄一次。
16. **`sanity.py` 守 `ui` 的兩個方向**（2026-11-16，第 1 階段的 1a）：`ui` 加進 `boundary` 的 `BROWSER_SIDE`，renderer 那一側的 crate 不能依賴它（renderer 有自己畫 UI 的方式；Chromium 也用 DEPS 擋 renderer 引用 `//ui/views`）；`deps` 加一份 `ui` 的 `[dependencies]` 的允許清單（`base`、`gfx`、`compositor_paint`、`parley`、`taffy`），守住第 2、4 條，`[dev-dependencies]` 不限。多依賴一個 crate 時改清單，同時在這份 RFC 寫明理由。不選只加進 `BROWSER_SIDE`：`ui` 依賴瀏覽器的東西時沒人擋。不選不檢查：兩個方向都沒人擋。
17. **工具列拆成按鈕和網址列的 view，分頁列整個是一個 view**（2026-11-17，第 1 階段的 1b）：工具列是一個 flex 的列，三個按鈕 view 和一個網址列 view，位置用 flex 寫出和現在一樣的數字（左邊距 6、間距 4、網址列前多 4、高 30 置中）；每個按鈕自己 hover、按下，網址列自己拿焦點。分頁列照舊自己算每個分頁、關閉鈕、新分頁鈕的位置。理由：按鈕和網址列是第 2 階段要搬進側邊欄的元件；分頁列畫的時候分頁互相依賴（兩個分頁之間的分隔線看兩邊有沒有 hover、是不是目前的分頁；目前的分頁最後畫、蓋在其他分頁上），第 2 階段本來就要重做成直的清單，分頁怎麼拆到時候一起決定。不選兩個都包成一個葉子：幾乎沒用到樹，第 2 階段兩個都要再拆。不選全部拆開：分隔線要看鄰居、畫的順序要另外控制，樹現在沒有直接的做法，拆了又要為直的清單重改。
18. **第 1 階段樹只涵蓋上方的 UI 區域，網頁 view 照舊在樹外**（2026-11-17，第 1 階段的 1b）：版面給一個 UI 區域的矩形（現在分頁列加工具列那一塊），樹的根就是它，裡面用 flex 上下排分頁列和工具列；Browser 的 layer 的大小、hit test 的順序（先網頁 view，再樹）、網頁的大小都和現在一樣。分頁列 view 的高度是分頁列取整後的 physical px 除以倍率，工具列照舊從整數的 physical px 開始（現在兩者各自換算、取整；直接放在 34 CSS px 時，1.25 倍這種倍率下工具列的上緣落在半個像素上）。不選樹涵蓋整個視窗、網頁 view 在樹外：Browser 的 layer 變成整個視窗大，網頁那一塊多出空的 tile，好處第 2 階段才用得到。不選網頁 view 也進樹：那是第 2 階段的題目，和側邊欄一起決定。
19. **1b 的外觀不變，用改之前和改之後的截圖逐像素比較**（2026-11-17，第 1 階段的 1b）：一份暫時的情境把 UI 擺成各種狀態（剛打開；hover 在三個按鈕上、上一頁不能按；網址列全選、打字後游標在中間、太長捲到後面；hover 在分頁、關閉鈕、新分頁鈕上；分頁擠到最窄、超出右邊；標題淡出），在 1x、1.25x、1.5x、2x 各截一張，在 1a 的 commit 和改完的程式上各跑一次，先比 PNG 的內容，不同的再解碼找出差的像素。情境和比較的腳本不 commit，結果寫進 `progress.md`。不選比較 display list：拆成多個 view 之後 op 多了平移、座標換成各個 view 自己的，畫面一樣也比不出相同。不選只靠現有的測試：它們檢查位置和行為，不檢查畫出來的樣子。不選永久的截圖測試和標準答案：第 2、3 階段本來就要改外觀，標準答案要一直換；這裡只要證明這次沒變。
20. **網址列整個是 `shell` 的 view，等第二個文字欄位出現再拆出通用的**（2026-11-17，第 1 階段的 1b）：`omnibox.rs` 大部分是通用的單行編輯（游標、選取、刪除、輸入法、剪貼簿、捲動），網址列特有的只有沒焦點時顯示網址、Escape 改回網址、第一次點擊全選、Enter 送出導覽、貼上時換行變空白。現在只有它一個使用者，拆出來的 API 是照它猜的；有第二個（頁面內搜尋）時再拆，哪些通用、哪些可設定才看得清楚。編輯的程式本來就集中在網址列裡，拆的時候不難。剪貼簿交給樹擁有（`ViewTree::set_clipboard`），網址列從 context 拿；sad tab 和下拉清單借樹的字型（第 13 條）。不選現在就拆成 `ui::TextField` 加上網址列：1b 多了設計通用元件的 API，不再是純重構。
21. **第 2 階段分兩段；外觀和排版先用 Rust 描述，machopo 自己的 CSS 留給之後的一份 RFC**（2026-11-17，第 2 階段）：2a 是宣告式的那一層，現在的橫式 UI 改成用「狀態 → UI」的函式描述（重構，外觀和行為不變）；2b 是直的分頁清單、側邊欄的版面和切換。描述函式直接寫 taffy 的 `Style`（加上簡寫），值從主題的 token 拿；描述的結果是資料（每個節點的 `Style` 和 token），之後換成 CSS 產生同樣的資料，描述 UI 的函式不用重寫。CSS 可行：`css`（解析、各個 property）和 `style_resolver`（從宣告算 cascade）不依賴 DOM，只有 `style`（在 DOM 上比對 selector）依賴；要做的是讓 `selectors` 在 `ui` 的樹上比對、把 `ComputedStyle` 轉成 taffy 的 `Style` 和顏色，`ui` 的依賴的白名單（第 4、16 條）要放寬。它最接近 Zen 那樣讓使用者自己改外觀，但本身是一項大工程，放進第 2 階段會讓側邊欄晚很多才出現。不選 Rust 加上覆蓋 token 的主題檔：之後做 CSS 時它是多餘的。
22. **宣告式那一層：描述是 `Element` 的樹，view 帶一個 Props 型別，相等就跳過**（2026-11-17，第 2 階段的 2a）：每個節點帶 key（可省略）、taffy 的 `Style`、子節點和那個 view 的 Props。比對子節點時有 key 的照 key、沒有的照位置；對得上又是同一種 view 就更新，對不上就拿掉舊的、建新的，換了順序的在樹裡搬動、不重建（React、Flutter 的「同一種型別、同一個 key」）。每種 view 定一個 `Props`（`PartialEq`），實作 `create(props)` 和 `set_props(props, ctx)`；樹裡每個節點記著上一次的 Props 和 Style，Props 相等就不呼叫，Style 變了才重設。理由：描述函式每次狀態改變都重跑，大部分節點沒變，連呼叫都不用；Props 就是元件的公開設定，好測、當成 GUI API 清楚；判斷「有沒有變」照第 15 條留在 view 裡。最接近 Yew（`Properties` 要 `PartialEq`，變了才呼叫 `changed`）和 Dioxus（props 相等就不重新 render）；不同的是 Props 直接交給樹裡保留的 view，不再經過一個 render 函式（底下本來就有保留的樹，第 6 條）。不選描述和 view 分開（Flutter 的 Widget、Xilem 的 `View`）：每種元件要兩個型別。不選每個節點帶建立和更新的 closure（iced 這類每次重跑的）：沒變的節點也每次跑，元件能設定什麼散在各處。
23. **描述函式讀一份狀態的快照 `UiState`，狀態變了才重新描述**（2026-11-17，第 2 階段的 2a）：`BrowserView` 每次 `update()` 抽一份 `UiState`（分頁的標題、目前的分頁、網址、能不能上一頁和下一頁、用不用 macOS 的編輯鍵），和上一份比較，不同才交給描述函式；描述出來的節點再照第 22 條比對 Props。取代 `sync_toolbar`（每個 renderer 訊息之後手動把網址、標題、按鈕的狀態推給各個 view）。描述函式只依賴 `UiState`：側邊欄（2b）是另一個描述函式，吃同一份；測試直接給一份 `UiState`、檢查描述出來的樹。焦點、游標這些 view 自己的狀態不放進 `UiState`（第 8 條）。命令式的操作（讓網址列取得焦點）和測試用全域唯一的名字找到樹裡的 view：`Element` 可以帶名字，宣告式那一層記著名字到 id，`find::<T>(名字)` 拿到帶型別的 id（第 14 條）。不選直接拿 `&BrowserView`：描述函式綁在它的內部，什麼時候重新描述要另外決定。不選響應式的 signal（Leptos、SolidJS、Vue）：要一整套訂閱和傳遞的機制，和 Props 的比對是兩套做法。
24. **網頁 view 進樹，一開始就照 N 個網頁來寫**（2026-11-17，第 2 階段的 2b）：樹涵蓋整個視窗，描述裡放網頁的位置節點（`WebViewSlot`，Props 是它顯示的分頁的 id，key 也是），不畫東西，bounds 由排版決定，取整到 physical px 之後就是那個網頁 view 的 bounds（RFC 0002 的第 3 條，只在這個節點取整）。`BrowserView` 從樹裡收集所有網頁節點、照它們的 bounds 擺網頁 view，不在任何節點裡的分頁是隱藏的，不再假設第一個網頁 view 是目前的分頁；`UiState` 記要顯示哪些分頁（現在只有目前的一個）。hit test 照 RFC 0002：先看網頁 view（bounds 從樹來），再看樹。理由：版面只寫在描述函式一個地方，側邊欄的寬度、之後的收合，網頁都跟著；之後的 split view 只是描述裡多放網頁節點和一條分隔線（拖動時送出訊息、更新狀態，第 8 條），兩個分頁對調只是搬動節點，網頁不重建（2026-11-17 看了 Zen 的 split view：兩個網頁並排、各自圓角、之間有間距，側邊欄把兩個分頁顯示成一列）。Browser 的 layer 變成整個視窗大，網頁那一塊沒有 op，compositor 跳過「看 op 就知道是單色」的 tile（不 raster，只多一個被網頁蓋住的單色 quad），第 18 條擔心的成本很小。對照 Chromium 的 `views::WebView`（樹裡的 view，`WebContents` 的 bounds 跟著它）。不選留在樹外、`BrowserViewLayout` 另外給側邊欄和網頁的矩形：每種版面要在描述和 `BrowserViewLayout` 各寫一次。`BrowserViewLayout`（和測試用的兩個網頁 view 的版面）改成從描述來。split view 本身（分頁怎麼成組、`TabStripModel` 記哪些是一組、側邊欄把一組顯示成一列）和網頁的圓角（compositor 裡 surface 的圓角裁切，第 3 階段）不在 2b。
25. **直的分頁清單：每個分頁一個 view，用分頁的 id 當 key，關閉鈕在分頁 view 裡**（2026-11-17，第 2 階段的 2b）：直的分頁不互相依賴（沒有分隔線，目前的分頁只換底色、不蓋到別人），可以拆開：開關分頁只碰變動的那幾列，hover 由樹處理；之後 split view 的一組是一個橫的容器放兩個分頁 view，拖拉排序和動畫也建在「一個分頁一個節點」上。關閉鈕留在分頁 view 裡、自己判斷點到哪裡（和橫式一樣）：拆成子 view，它不處理的事件會往上傳給分頁，「點關閉鈕」要小心不被當成「切換分頁」，而通用的按鈕元件等 `ui` 當成 GUI API 時再做。行為照橫式：在分頁上按下主按鍵就切換，關閉鈕按下、放開都在它上面才算，中鍵點分頁就關掉；新分頁的按鈕是另一個 view。分頁太多、超出側邊欄時，超出的畫到視窗外、看不到（捲動要第 3 階段的個別 layer 和裁切），記進 `docs/limitations.md`。不選整個清單一個 view（像橫式）：開關分頁整個重畫，split view 的組、拖拉、動畫都要在清單裡另外處理。不選標題和關閉鈕也拆成子 view：見上。
26. **側邊欄的版面：預設 260 CSS px、可以拖右邊緣調整；按鈕、網址列、新分頁、分頁清單由上到下**（2026-11-17，第 2 階段的 2b）：最上面一列是上一頁、下一頁、重新載入（同一批按鈕元件），下面是佔滿寬度的網址列（同一個元件），再下面是一列「+ New Tab」，然後是分頁清單（每列高 36、圓角 8、左右留 8；目前的分頁是亮的底色，hover 時是淡的底色；關閉鈕在 hover 或是目前的分頁時才出現）。網頁在側邊欄右邊，貼齊、不留邊距。新的顏色 token：側邊欄的背景、分頁列的 hover、目前的分頁。寬度是 `UiState` 的 `sidebar_width`，描述函式照它排，網頁節點在 flex 裡佔剩下的；右邊緣一條約 6 px 寬的把手是一個 view（游標是左右調整的，`ui` 的 `Cursor` 多一種），按下收住指標，拖動時送出「寬度改成 x」的訊息，夾在 200 到 min(600, 視窗寬度的一半)。拖動時網頁的 viewport 跟著變，和拖視窗邊緣一樣（Chrome 的側邊面板拖動時也即時改大小）。寬度記在視窗的狀態裡，關掉再開回到預設（machopo 還沒有存設定的地方），記進 `docs/limitations.md`。2026-11-17 看了 Zen：側邊欄可以拉寬，網頁跟著縮；原本的看法是固定 260。不選：Zen 的紅黃綠按鈕和收合鈕、只顯示網域的網址列、留縫的圓角網頁卡片、收合：前兩個和後兩個是第 3 階段的外觀和網址列自己的功能。新分頁放在清單最後：分頁一多就被擠到看不見的地方。
27. **用命令列選項和快捷鍵切換版面**（2026-11-17，第 2 階段的 2b）：版面是 `UiState` 的一個欄位，描述函式照它選橫式或側邊欄。`machopo --ui-layout=sidebar` 啟動時選；`Command::ToggleSidebar` 在視窗開著時切換，快捷鍵是 macOS 的 Cmd+Shift+,、其他平台的 Ctrl+Shift+,（現有的快捷鍵沒用到；記得 Edge 切換直式分頁也是這組，沒查證）。切換時，按鈕和網址列換到另一個父 view 下，比對只在同一個父 view 底下做，所以它們重新建立：網址列打到一半的字和焦點不見，網頁不受影響（網頁是 `BrowserView` 的，網頁節點只是位置）；React 也不跨父節點保留元件。記進 `docs/limitations.md`。不選只有命令列選項：開著時不能比較、不能測切換。不選 UI 裡的切換按鈕（Edge 在分頁列左邊有一個）：改變橫式的外觀、佔位置，和第 3 階段的收合鈕一起設計（Zen 左上角的鈕是收合，不是換版面）。
28. **視窗預設橫式；自動化的 pipe 和情境一律橫式；切換的快捷鍵自動化時照樣能按**（2026-11-17，第 2 階段的 2b）：`--ui-layout=sidebar` 只對直接開的視窗有用；`--automation-pipe`（WPT、machopodriver、Speedometer）和情境都從橫式開始，viewport 的大小（800×600 的視窗裡 800×526）不因版面而變，WPT 的預期結果和 WebDriver 的視窗大小不跟著變。切換的快捷鍵（第 27 條）在自動化時照樣能按：側邊欄的行為（點分頁、拖寬度、切換）在情境裡先按它切過去再測；WPT 的按鍵送給網頁，按不到它。之後要讓自動化從側邊欄開始，再加一個 capability 或情境的設定。原本的建議是情境多一個 `window layout=sidebar` 的設定，2026-11-17 改成先一律橫式、用快捷鍵切換。
29. **第 3 階段分四段：3a 深色、3b 視窗（標題列、透明和毛玻璃）、3c 收合和網頁卡片、3d 捲動**（2026-11-17，第 3 階段）：3a 是深色主題、UI 跟著系統的深淺色，主題的 token 準備好有毛玻璃和沒有毛玻璃兩組值；3b 是視窗那一層：macOS 畫進標題列（內容延伸到整個視窗，紅黃綠按鈕疊在側邊欄上）、視窗透明，後面請系統放毛玻璃（2026-11-17 看了 Zen：側邊欄透出模糊的桌布，是 macOS 的 vibrancy；程式拿不到桌面的像素，只能讓視窗透明、請系統模糊，UI 用半透明的顏色）；3c 是側邊欄收合，網頁變成圓角、留縫的卡片（縫透出毛玻璃）；3d 是分頁清單捲動（`ui` 的捲動容器、個別 view 的 layer 和裁切，第 9 條留的路）。順序：3a 最小，而且後面幾段的顏色都要有深色的值；3b 最接近 Zen；3d 最大、急迫性最低（800 高的視窗要超過約 17 個分頁才超出）。毛玻璃在各平台：macOS 和 Windows 11 有（`window-vibrancy` 的 vibrancy 或 macOS 26 的 Liquid Glass、Mica 或 Acrylic），Linux 看 compositor：KDE 的 Wayland 用 winit 的 `set_blur`（只在有 `org_kde_kwin_blur_manager` 時有用），X11 和 GNOME 沒有，用不透明的主題（只透明、不模糊，後面的東西會清楚透過來，字看不清楚）。毛玻璃的做法（vibrancy 或 Liquid Glass、要不要加 `window-vibrancy`）和橫式要不要畫進標題列，3b 開始時再決定。2026-11-17 做 3a 時改：有毛玻璃的那組值移到 3b，3a 只有不透明的淺色和深色。半透明的值要視窗透明了才看得到、才能檢查，而且系統放在後面的底色看用 vibrancy 還是 Liquid Glass 而不同，值要等那個決定。主題已經是照深淺色建的（`BrowserTheme::new`），3b 再多一個有沒有毛玻璃的參數。
30. **深色主題用 Chrome 舊版的深色**（2026-11-17，第 3 階段的 3a）：淺色主題就是 Chrome 2023 年改版前的淺色（工具列 `#F1F3F4`、分頁列 `#DEE1E6`、文字 `#202124`，Google Material 的灰），深色用同一套的另一面（分頁列 `#202124`、工具列和目前的分頁 `#35363A`、文字 `#E8EAED`），每個 token 一對一對應；其他 token 的值對照 Chromium 原始碼裡的顏色定義確認，對不上的列出來。之後要整體換成 Zen 的風格，是換兩份主題的值，結構不動（第 10 條）。不選照 Zen 的深色：淺色是 Chrome 的、深色是 Zen 的，對不上，Zen 的深色大多靠毛玻璃加一層顏色，沒有毛玻璃時的值要另外找。不選自己配：沒有來源可以對。2026-11-17 對照原始碼的結果在 `details.md` 的「3a. 深色主題的值」：文字照原始碼是白色（`kColorToolbarText`、`kColorOmniboxText`），不是上面寫的 `#E8EAED`；淺色本身和 Chrome 的也不是一對一（Chrome 的工具列是白的）；Chrome 沒有自己的顏色的（hover、分頁之間的線、側邊欄）用淺色主題裡同樣兩個顏色之間的比例推。
31. **直接開的視窗跟著系統的深淺色，`--ui-theme=light|dark` 可以強制；自動化的 pipe 和情境一律淺色**（2026-11-17，第 3 階段的 3a）：winit 給系統的深淺色（`Window::theme()`），使用者切換時有 `ThemeChanged`。`--ui-theme` 照 `--ui-layout` 的樣子只對直接開的視窗有用，`--automation-pipe` 不接受：截圖和 WPT 的結果不該因為跑測試那台機器的系統設定而變（和第 28 條同樣的理由）。深色的正確性由整合測試直接把 `BrowserView` 換成深色、檢查工具列和側邊欄的像素來守。不選只跟著系統：想看深色要改系統設定。不選情境多一個 `window theme=dark` 的設定：情境的截圖只給人看，像素由整合測試檢查；之後要看深色的畫面再加。
32. **網頁的 `prefers-color-scheme` 不在這份 RFC，照舊是 `light`**（2026-11-17，第 3 階段的 3a）：系統是深色時，Chrome 的 UI 變深，網頁的 `prefers-color-scheme` 也是 `dark`（網頁的預設背景照樣是白色，除非網頁用 `color-scheme` 說它支援深色）。那是網頁平台的功能：renderer 啟動時要拿到使用者要的深淺色、切換時重新算 style 和通知 `matchMedia`，還有 `color-scheme` 和深色的表單控制項，動到 renderer 和 `protocol`，要找 WPT 的測試，規模和測試的方式都和 UI 不同，另開一份 RFC（「之後的可能」）。在那之前是深色的 UI 配淺色的網頁，`docs/limitations.md` 原本的那條照舊。
33. **macOS 的毛玻璃用系統的 vibrancy（`NSVisualEffectView`），經過 `window-vibrancy`**（2026-11-17，第 3 階段的 3b）：視窗透明，winit 的 view 下面放一個 `NSVisualEffectView`（側邊欄的材質），wgpu 的 surface 改成不是不透明的（Metal 的 `PostMultiplied`），UI 透明或半透明的地方透出它。這是 Zen 實際的做法（Firefox 用 `NSVisualEffectView`）；公開的 API，10.14 以上都有；系統的材質自己跟著深淺色，「減少透明度」的設定打開時系統換成不透明的。呼叫 AppKit 要 `unsafe`，`shell` 不能有，所以用 `window-vibrancy`（Tauri 的；它用的 objc2 0.6 和 objc2-app-kit 0.3 已經在 `Cargo.lock` 裡），同一個 crate 也有 Windows 的 Mica 和 Acrylic。Chrome 在 macOS 的主視窗是不透明的，沒有對應。實作前先用一個小程式確認 effect view 在 wgpu 的 Metal layer 下面、透得出來；透不出來就回來重新決定。不選 winit 的 `set_blur`：私有的 `CGSSetWindowBackgroundBlurRadius`，只有固定半徑的模糊，沒有材質，顏色要自己疊，也不理會「減少透明度」。不選 macOS 26 的 Liquid Glass（`NSGlassEffectView`）：只有 26 以上，舊的系統照樣要 vibrancy，API 很新；之後要加時當成 vibrancy 之上的選項（「之後的可能」）。2026-11-17 實驗的結果（`details.md` 的「3b 的實驗」）：`window-vibrancy` 把 effect view 加成 winit 的 view 的子 view，子 view 畫在 view 自己的內容上面，wgpu 的 Metal layer 正是 winit 的 view 自己的內容，所以整個被材質蓋住，行不通（Liquid Glass 的 API 也是同樣的放法）；effect view 放在視窗的框裡、winit 的 view 後面才透得出來，那要 machopo 自己寫 AppKit 的呼叫。3b 因此延後（第 34 條）。
34. **3b（視窗的毛玻璃、畫進標題列）延後，先做 3c、3d**（2026-11-17）：照第 33 條的實驗，系統的材質要 machopo 自己把 effect view 放在 winit 的 view 後面，得多一個允許 `unsafe` 的平台 crate（專案的規則是 `unsafe` 只在 `bindings` 和 `ipc`）；其他的路是 fork `window-vibrancy`（為了約 20 行多維護一個 fork），或 winit 的 `set_blur`（私有 API、只有模糊、沒有材質）。都要先改規則或多一份維護，現在不值得。畫進標題列一起延後（和毛玻璃都是視窗那一層的事，透明的視窗才看得出效果）。3c 的網頁卡片和 3d 的捲動不靠它：卡片之間的縫先是側邊欄的顏色。之後要做時，從「之後的可能」的「視窗的毛玻璃和畫進標題列」開始，先決定 `unsafe` 放在哪裡。
35. **先做 3d（分頁清單捲動），再做 3c（收合、網頁卡片）**（2026-11-17）：3c 的收合要像 Zen 的 compact mode 那樣，指標到左邊緣時側邊欄浮出、蓋在網頁上，UI 就要有疊在網頁上面的 layer；3d 的捲動本來就要讓 view 有自己的 layer（第 9 條留的路），先做它，3c 的浮出用同一套東西。原本照第 29 條的順序（3c 先），3c 只能先做「藏起來、快捷鍵展開、網頁跟著縮回」，浮出等之後。
36. **側邊欄只有分頁的那幾列捲動**（2026-11-17，第 3 階段的 3d）：按鈕、網址列和 New Tab 固定在上面，下面的分頁清單捲動。網址列和新分頁隨時按得到，和 Edge 的直式分頁（Chromium）一樣；之後加釘選的分頁（Zen 的 pinned、essentials）也放在固定的那一段。不選整個側邊欄一起捲：捲到下面要先捲回去才能打網址。不選 New Tab 跟著分頁捲：捲到下面時看不到它。
37. **捲動容器是 `ui` 的元件 `ScrollView`，捲動的位移是它自己的狀態**（2026-11-17，第 3 階段的 3d）：描述裡放 `Element::new::<ScrollView>(…)`，內容是它的子節點；位移不在 Props 裡，重新描述時留著；滾輪由它的事件處理，沒用完的往父 view 傳；從外面捲（捲到看得到的地方）照樣走 `tree.update`。樹要認得「這個節點的子 view 移了多少」：hit test、畫的位置、`absolute_bounds` 都扣掉它。對應 Chromium Views 的 `ScrollView`，SwiftUI、Flutter 也是元件。不選樣式的 `overflow: scroll`（CSS 的做法）：位移和滾輪的處理變成樹內建的行為，從外面捲要另外的 API；`ui` 的樣式只管排版。不選側邊欄專用的 view：3c 的浮出、之後的選單和設定頁不能重用。
38. **捲動的內容是自己的 layer，捲動只改位移**（2026-11-17，第 3 階段的 3d）：`ScrollView` 的內容錄進自己的 `PictureLayer`（內容的整個高度），compositor 的 transform 節點帶位移、clip 節點裁在容器裡；捲動不重 raster，只重新合成（compositor 只 raster 看得到的範圍周圍的 tile）。`ui` 的 `Frame` 因此從一個 layer 變成多個，每個帶位置、裁切和位移，重畫的範圍算在各自的 layer 上；`BrowserView` 把它們轉成 compositor 的 layer 和 property tree。這是第 9 條留的「個別 view 自己的 layer」，3c 的浮出（疊在網頁上的 layer，compositor 的 `over_surfaces`）和之後的動畫也用它。對應 Views 的 `ScrollView` 在 `ScrollWithLayers` 時：內容是自己的 `ui::Layer`。不選同一個 layer、捲了就重畫：每捲一格清單看得到的範圍整片重 raster（2 倍時約 62 萬像素），3c 時照樣要做 layer。
39. **捲動的輸入和樣子和網頁一樣：沒有捲軸，滾輪一格 40 px、立刻到位，觸控板照系統**（2026-11-17，第 3 階段的 3d）：滾輪的行數照 `compositor::scroll_utils`（Chromium 的 `kPixelsPerLineStep`）換成 CSS px，沒有平滑的動畫；觸控板的像素和慣性是系統送來的；捲到邊停住，不回彈。最下面被切掉一半的那一列看得出下面還有。捲軸之後和網頁的捲軸一起做（`docs/limitations.md`：網頁沒有捲軸），用同一套畫法和動畫。不選一直顯示的細指示條：它要畫在捲動的內容上面，得另外分層，UI 有捲軸而網頁沒有。不選 macOS 的 overlay 捲軸（Zen 的樣子）：多出計時、淡出、拖動，先替 UI 單獨做一套。
40. **目前的分頁跑出畫面時，捲到剛好看得到**（2026-11-17，第 3 階段的 3d）：切換分頁（快捷鍵、關掉目前的分頁之後）、開新分頁、從連結開的分頁時，目前的分頁那一列在上面看不到就捲到它貼齊頂端，在下面看不到就貼齊底部，看得到就不動（Views 的 `ScrollView::ScrollRectToVisible`、Blink 的 `scrollIntoView({block: 'nearest'})`）；滑鼠點的那一列本來就看得到，不跳。分頁關掉、清單變短或視窗變矮時，位移夾到新的最大值；切換版面時側邊欄重新建立，位移回到 0（和打到一半的網址一樣）。不選不自動捲：開了新分頁看不到它。不選一律捲到中間：點一列清單就跳。

2026-11-16 的看法：預設的版面維持橫式（現有的情境照它的位置點擊）；切換先用命令列選項。第 2 階段確認。

## 待決定

**第 2、3 階段**的問題到那個階段開始前再列：宣告式那一層的形狀；外觀和排版用什麼描述（Rust 的版面規則、taffy，或 machopo 自己的 CSS 引擎幫 UI 套樣式：使用者能像 Zen 那樣用 CSS 改外觀，但不用 V8 和另一個 renderer，代價是 style 和 layout 要能離開 DOM 給 UI 用）；網頁 view 進不進樹；切換的方式；畫進標題列的做法；自動化和 WPT 的視窗用哪個版面。

## 之後的可能

- **focus 管理**：Tab 鍵走過按鈕、網址列和分頁，focus 的外框（Chromium 的 `FocusManager`）。
- **無障礙**：AccessKit。
- **動畫**：側邊欄展開、收合，分頁的開關。
- **Zen 的其他元件**：常用網站的方塊、分頁的資料夾、工作區。
- **拖拉排序分頁**。
- **公開的 GUI API**：`ui` 的 API 在瀏覽器裡用穩之後。
- **使用者自訂外觀**：主題或設定的格式。
- **視窗的毛玻璃和畫進標題列**（原本的 3b，第 33、34 條）：macOS 的 effect view 放在 winit 的 view 後面（要一個允許 `unsafe` 的平台 crate），UI 用半透明的顏色，主題多一組有毛玻璃的值；側邊欄的版面讓內容延伸到整個視窗，紅黃綠按鈕疊在側邊欄上，空白處拖動視窗；Windows 的 Mica，Linux 的 KDE 用 `set_blur`；自動化一律不透明。
- **Liquid Glass**：macOS 26 以上用 `NSGlassEffectView` 代替 vibrancy（第 33 條），舊的系統照舊。
- **網頁跟著深淺色**：`prefers-color-scheme`、`color-scheme`、深色的表單控制項（第 32 條），另開一份 RFC。
