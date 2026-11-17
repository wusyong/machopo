# RFC 0102：瀏覽器 UI 的 view 樹和可以換的版面

- 開始日期：2026-11-16
- 狀態：🚧 已排程：2026-11-16（第 1 階段做完；第 2 階段進行中，分 2a、2b；第 3 階段的問題到那個階段開始前再決定）
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
