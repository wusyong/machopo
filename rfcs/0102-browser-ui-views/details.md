# RFC 0102 的細節：瀏覽器 UI 的 view 樹和可以換的版面

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## Zen 的版面

2026-11-16 在 macOS 上看 Zen（Firefox 的分支）開著 Wikipedia 的視窗（只截圖，沒有操作）：

- **沒有上方的工具列和分頁列**：整個 UI 在左邊約 260 px 寬的側邊欄，網頁佔右邊。
- **側邊欄最上面一列**：系統的紅黃綠按鈕、收起側邊欄的按鈕、上一頁、下一頁、重新載入。UI 畫進了標題列。
- **網址列**：圓角的框，只顯示網域。
- **常用網站**：一排四個方塊圖示。
- **分頁是直的清單**，由上到下：工作區的名稱、可以收合的資料夾、固定的分頁、分隔線、「+ New Tab」、一般的分頁。目前的分頁用比較亮的圓角底色標出。
- **最下面**：工作區的圖示和一個「+」。
- 深色，大量圓角，幾乎沒有框線；側邊欄是不透明的深灰（不是 macOS 的半透明毛玻璃）。

## 做法的比較

`README.md` 的「理由和其他做法」那張表的補充。

### 效能

- **1、2、3 畫的成本一樣**：都是自己寫 paint op，交給 `compositor`（切 tile）、`raster`（Vello）、`viz`（和網頁的 surface 合成）。3 多一次排版：十幾個 view，微秒級，量不出差別。重畫的頻率也一樣：只在 UI 變了（hover、打字、分頁變了）時。
- **和 HTML UI（B）的差別**：B 的 UI 是一個網頁，要一個 renderer：V8 的 isolate、DOM、style、layout、它自己的 compositor。記憶體和啟動時間都比較多（沒有量，machopo 沒有這種 UI）。Chrome 的工具列用 Views 畫，部分理由就是這個；Firefox 的 UI 是網頁技術，換到的是好改外觀。

### 為什麼 2 不夠

2 是「元件和版面分開，但版面自己算每個元件的矩形」。直式側邊欄裡，按鈕一列、網址列、分頁清單是上下排，分頁清單裡每個分頁又是左右排（圖示、標題、關閉）；Zen 還有資料夾裡的分頁。每多一層都要在版面裡手算一次，等於每種版面各寫一套排版。長期用自己畫的 UI 時，版面管理早晚要做。

### HTML UI 會怎麼接

P2 預留的做法：工具列的 `PictureLayer` 換成嵌入 UI renderer 的 `SurfaceLayer`（Chromium 用 `views::WebView` 嵌入 WebUI 的做法）。缺的是 renderer 的 `<input>` 編輯（caret、選取、IME）和讓 UI 頁面呼叫 Browser 的特權通道。選了 3 之後這條路不為 UI 而做；設定頁之類用網頁做時，3 的樹裡放一個網頁 view 就能嵌入。

## Q0. 寫 UI 的方式

2026-11-16 選 B（`README.md` 的「已決定的事項」第 6 條）。比較的標準照第 5 條。

| | A. 保留的樹，命令式 | B. 宣告式，底下是保留的樹 | C. immediate mode |
| --- | --- | --- | --- |
| 寫起來 | 建好 view，狀態變了就呼叫 `set_url`、`set_tabs` 改它 | 寫「狀態 → UI」的函式，框架比對前後兩次，只改有變的 view | 每次有事件就重跑整段畫 UI 的程式，不留 view |
| 誰這樣做 | Chromium Views、AppKit、GTK、Qt、Masonry | SwiftUI、Flutter、React、Xilem | egui、Dear ImGui |
| 換版面 | 拆掉重建 view | 換一個函式 | 換一段程式 |
| 狀態和畫面不同步的 bug | 容易有（現在的 `sync_toolbar`） | 少：畫面只從狀態來 | 少 |
| 狀態變了要重算多少 | 只改有變的 view | 重跑函式加比對（幾十到幾百個節點，微秒級） | 重跑全部 |
| 巢狀排版、文字編輯、無障礙 | 容易 | 容易（底下是保留的樹） | 難：排版常用上一幀的大小，編輯狀態和無障礙要另外用 key 記 |
| 實作成本 | 最低 | 最高：比對、身分、key | 低 |
| 當成 GUI API | 能用，膠水程式多 | 現在主流的寫法 | 適合工具類程式 |

- **為什麼分兩層**：B 底下本來就要 A 的保留樹（編輯狀態、捲動位置、capture 都留在 view 裡），所以第 1 階段先做保留的樹（重構，外觀和行為不變），第 2 階段加宣告式的一層，和側邊欄一起做：「換版面＝換一個函式」是 B 最有用的地方。
- **為什麼用動態型別比對**：Xilem 把整棵 UI 的形狀寫進型別（每個 view 是泛型參數），比對不用動態分派，但型別很長、編譯慢、錯誤訊息難讀。我們的 UI 只有幾十到幾百個節點，每次比對走一遍 `Box<dyn …>` 的成本可以忽略。
- **效能**：三者畫的成本一樣（都畫成同一份 display list）。差在狀態變了之後：A 只改有變的；B 多跑一次描述函式和比對；C 重跑全部。對瀏覽器 UI 的大小，三者都在微秒級；閒著的時候都不花。

## Chromium 的 views

參考用（`README.md` 的「已決定的事項」第 5 條：`ui` 不要求對應 Chromium）。

| 項目 | Chromium | 這份 RFC |
| --- | --- | --- |
| 樹 | `views::View`：父 view 擁有子 view（`AddChildView` 收 `std::unique_ptr`），子 view 有指向父的 raw pointer | 第 1 階段的 Q2 |
| bounds | 每個 view 的 `bounds()` 相對於父 view，整數的 DIP | 相對於父 view；單位第 1 階段決定（RFC 0002 的網頁 view 用整數的 physical px） |
| 版面 | `View::Layout()`，`LayoutManager`（`BoxLayout`、`FlexLayout`、`TableLayout`）；`GetPreferredSize()` | 第 2 階段 |
| 畫 | `View::OnPaint(gfx::Canvas*)`，錄成 `cc::DisplayItemList`；預設畫進 `Widget` 的 layer，`SetPaintToLayer()` 的 view 有自己的 `ui::Layer` | 整棵樹一個 layer（Q4）；自己的 layer 之後 |
| 事件的目標 | `Widget` 的 `RootView` 用 `ViewTargeter` 找點下面的 view | 一樣的形狀 |
| 滑鼠 | 按下時從最深的 view 往上找 `OnMousePressed` 回傳 true 的，它收之後的拖曳和放開 | 一樣（Q3） |
| 按鍵 | 給 focus 的 view；加速鍵由 `FocusManager` 先處理 | 第 1 階段照現在：快捷鍵、網址列、網頁 |
| 點擊之後 | 按鈕的 `PressedCallback` 直接呼叫瀏覽器的程式 | 送出動作，`BrowserView` 執行（Q3） |
| focus | `FocusManager`、`FocusTraversable` | 之後 |
| 主題 | `ui::ColorProvider`：`ColorId` 加上一串 mixer；`ui::NativeTheme` 告訴它深淺色 | 具名欄位的 struct（Q5） |
| 文字 | `views::Label`、`views::Textfield`，排版用 `gfx::RenderText`，不經過 Blink | `ui_text` 和網址列的 parley（現在就是這樣） |
| 網頁 | `views::WebView` 是樹裡的 view，`WebContents` 的 bounds 跟著它 | 第 2 階段決定 |
| macOS 的標題列 | `BrowserNonClientFrameViewMac`：分頁畫在標題列的高度，紅黃綠按鈕留給系統畫 | 第 3 階段 |

## 第 1 階段的選項

### Q1. `ui` 的邊界

2026-11-16 決定（`README.md` 的「已決定的事項」第 4 條）。比較：

- **依賴**：`base`（整數運算）、`gfx`（幾何、顏色、路徑）、`compositor_paint`（`DisplayItemList`、`PaintOp`）、`parley`（文字；`shell` 的 `ui_text.rs` 搬進來）。
- **不依賴**：`shell`、`browser`、`protocol`（瀏覽器的東西）；`compositor`（layer 是 `BrowserView` 的事，樹只交出 display list）；winit。
- **事件型別**：`ui` 自己定滑鼠（位置、按鈕、連點次數、modifier）和按鍵（邏輯鍵、文字、modifier）。`protocol::input` 是給 renderer 的 DOM 事件，形狀不同；winit 的型別綁死平台層。`shell` 的 `window.rs` 和 `keyboard.rs` 已經在轉換 winit 的事件，多轉一種。網址列（`omnibox.rs`）現在直接收 winit 的 `Key`，搬上樹時改收 `ui` 的。
- 對照：Chromium 的 `//ui/views` 依賴 `//ui/events`、`//ui/gfx`、`//ui/compositor`，不依賴 `//chrome`、`//content`。

### Q2. 樹的擁有權

2026-11-16 選 a（`README.md` 的「已決定的事項」第 7 條）。宣告式那一層（第 6 條）要能用 key 找到樹裡的 view、更新、插入、拿掉它，所以這題多了這個需求。

| | a. arena 加帶世代的 id | b. 父擁有子的 `Box<dyn View>` | c. 照樹的順序攤平在一個 `Vec` |
| --- | --- | --- | --- |
| 誰這樣做 | taffy 的 `TaffyTree`（slotmap）、多數 ECS | Views、AppKit | Clay 這類排版函式庫 |
| 比對完套用差別 | 用 id 直接插入、拿掉、搬動 | 要和描述同步走下去 | 插入、拿掉要搬後面所有節點 |
| capture 的 view 被拿掉 | id 查不到 | 記路徑（索引），樹改了就可能指錯 | 要另外維護 id 到位置的對照 |
| 往上找父 view | 查表 | 只能靠遞迴返回 | 要另外記 |
| 借用（不靠 `Rc` / `RefCell`） | 結構和 view 分開存，分得開借 | 要同時借父和子 | 簡單 |
| 效能 | 查找 O(1)，節點連續 | 走指標 | 走訪最快，修改最慢 |
| 實作成本 | 中（一百多行） | 中：capture 和比對要另外補 | 高 |

- `Rc<RefCell>` 不列入：`CLAUDE.md` 不預設用它。
- **id 的存法**：`Vec` 的位置加 u32 的世代數。另一個做法是 map 加永遠不重用的 `SequenceNumber<u64>`：更簡單，但查找要算 hash、節點不連續；瀏覽器 UI 的規模看不出差別，上千個節點的清單用 `Vec` 比較好。

### Q3. 事件的分派和動作

2026-11-16 選訊息（`README.md` 的「已決定的事項」第 8 條）。分派（最深的 view 往上找、capture、hover 的進出）各家差不多，要選的是點了之後結果怎麼回到瀏覽器：

| | a. 訊息（Elm 架構） | b. callback 拿到 `&mut 狀態` | c. view 自己改狀態 |
| --- | --- | --- | --- |
| 寫起來 | 按鈕送出一個值（`Command::Back`）；分派完一個一個處理，狀態變了重新描述 UI | 描述 UI 時附上 closure（`\|s\| s.go_back()`），分派時框架借出 `&mut 狀態` | view 拿到瀏覽器的參考，直接呼叫 |
| 誰這樣做 | Elm、iced、Redux | Xilem、SwiftUI、React 的 `onClick` | Views 的 `PressedCallback` |
| 和現在的程式 | `Command`、`BrowserView::run` 已經存在 | 狀態要從 `BrowserView` 拆出來（分派時樹正被借著） | Rust 裡借用不成立 |
| 能做的事看得出來 | 列在 enum 裡 | 散在 closure 裡 | 散在 view 裡 |
| 測試 | 斷言送出了哪個訊息 | 要準備整個狀態 | 難 |
| 要寫的程式 | 每個動作一個 enum 的值 | 最少 | 少 |

### Q4. 畫和重畫

2026-11-16 選 b（`README.md` 的「已決定的事項」第 9 條）。`compositor` 的 `PictureLayer` 已經有 `invalidation`（只重 raster 碰到的 tile）和 property tree（transform、clip），網頁在用，UI 沒用。

| | a. 整份重畫（現在） | b. 一個 layer，只重錄、只重 raster 變了的 | c. b 加上個別 view 的 layer | d. 每個 view 都有 layer |
| --- | --- | --- | --- | --- |
| hover 一個按鈕 | 全部重錄，整個 layer 重 raster | 只重錄那個按鈕，只重 raster 它碰到的 tile | 同 b | 只重畫那個 layer |
| 捲動直的分頁清單 | 整份重畫 | 清單可見的範圍整片重畫 | 只改位移，不重畫、不重 raster | 同 c |
| 誰這樣做 | 簡單的框架 | Blink（display item 快取、raster invalidation） | Flutter 的 `RepaintBoundary`、Views 的 `SetPaintToLayer` | iOS 的 UIView / CALayer |
| 實作成本 | 最低 | 中：每個 view 快取錄製結果，算變了的範圍（搬動時新舊兩個位置） | b 加上 layer 清單轉成 compositor 的 layer | 高；layer 太多 |
| 容易出的 bug | 幾乎沒有 | 漏算範圍，畫面沒更新 | 同 b，加上 layer 的順序和裁切 | 同 c |

- 沒有量過現在 UI 重 raster 的成本；側邊欄的面積（整個視窗高，2x 時更大）是選 b 的主因。

### Q5. 主題

2026-11-16 選 b（`README.md` 的「已決定的事項」第 10 條）。

| | a. 具名欄位的 struct | b. 具名的 token | c. 寫在 UI 的描述裡 |
| --- | --- | --- | --- |
| 寫起來 | `theme.toolbar_background` | `theme.color(ColorToken::ToolbarBackground)`，每份主題一張以 token 為索引的表 | 描述時直接給（`.background(...)`） |
| 誰這樣做 | 很多小框架 | design token、CSS 的自訂屬性、Chromium 的 `ColorId`（不含 mixer） | SwiftUI 的 modifier、Flutter 的元件參數 |
| 漏給一個值 | 編譯錯誤 | 編譯錯誤（表用 `match` 建） | 用預設值 |
| 深色主題 | 再一份 struct | 再一張表 | 每個用到的地方分深淺 |
| 從檔案讀、只覆蓋幾個、照名字列出 | 要另外寫對照 | 自然 | 難 |
| 接到之後的 CSS | 要另外寫對照 | 一對一對到自訂屬性 | 像 inline style |
| 效能 | 讀欄位 | 陣列索引 | 一樣 |

- 不做 Chromium 的 `ColorProvider` 的 mixer（一串改顏色的管線）：它是為了上百個元件、使用者的主題色、高對比模式，現在用不到。

## 第 3 階段

### 3a. 深色主題的值

2026-11-17 對照 Chromium 100（`100.0.4896.0`，2023 年改版前）的原始碼：`chrome/browser/themes/theme_properties.cc` 的深色（`GetDarkModeColor`，就是 incognito 的那一組）、`chrome/browser/ui/color/chrome_color_mixer.cc`、`omnibox_color_mixer.cc`、`tab_strip_color_mixer.cc`、`ui/color/core_default_color_mixer.cc`、`ui_color_mixer.cc`，顏色的名字在 `ui/gfx/color_palette.h`。`HSLShift` 照 `ui/gfx/color_utils.cc` 算。

| token | 淺色 | 深色 | 深色從哪裡來 |
| --- | --- | --- | --- |
| `ToolbarBackground` | `#F1F3F4` | `#35363A` | `kColorToolbar` |
| `ToolbarSeparator` | `#DADCE0` | `#3C4043` | `kColorToolbarContentAreaSeparator` → `kColorSeparator` → `kColorMidground`（`kGoogleGrey800`） |
| `Icon` | `#5F6368` | `#F1F3F4` | `kColorToolbarButtonIcon`：`kChromeIconGrey` 照 `TINT_BUTTONS` 的 `HSLShift`，算出來是 `kGoogleGrey100` |
| `IconDisabled` | `#BDC1C6` | `#7D7E81` | `kColorIconDisabled`：圖示的顏色加 `kDisabledControlAlpha`（`0x61`），疊在工具列上 |
| `ButtonHover` | `#DEE1E5` | `#4C4D50` | 推的：圖示的顏色 12% 疊在工具列上（淺色的比例） |
| `Field`、`FieldFocused` | `#DEE1E5`、白 | `#202124` | `kColorOmniboxBackground`（`kGoogleGrey900`）；深色的 Chrome 拿到焦點時不變亮，只有外框 |
| `FocusRing` | `#1A73E8` | `#8AB4F8` | `kColorAccent`（`kGoogleBlue300`） |
| `FieldText`、`TabTitle` | `#202124` | 白 | `kColorOmniboxText`、`kColorToolbarText`（目前的分頁和其他分頁的字都從它來） |
| `FieldSelection` | `#C6DAFC` | `#4E607F` | `kColorTextSelectionBackground`：accent 加 `kGoogleGreyAlpha500`，疊在網址列上 |
| `TabStrip` | `#DEE1E6` | `#202124` | `kColorFrameActive`：`#DEE1E6` 照 `TINT_FRAME` 的 `HSLShift`，算出來是 `kGoogleGrey900` |
| `TabHover` | `#E8EAED` | `#2A2C2F` | 推的：分頁列和目前的分頁的一半（淺色的比例） |
| `TabButtonHover` | `#CCCFD4` | `#48494C` | 推的：白 18% 疊在分頁列上 |
| `TabSeparator` | `#A0A4AA` | `#67686A` | 推的：白 32% 疊在分頁列上（淺色的比例） |
| `SidebarBackground` | `#F1F3F4` | `#35363A` | machopo 的：工具列的 |
| `SidebarRowHover` | `#E2E5E9` | `#434448` | machopo 的：白 7% 疊在側邊欄上（淺色的比例） |
| `SidebarTabActive` | 白 | `#515256` | machopo 的：白 14% 疊在側邊欄上，比其他列亮（淺色也是） |
| `SidebarEdge` | `#DADCE0` | `#3C4043` | machopo 的：和工具列下的線一樣 |

對不上的：

- **文字是白的，不是 `#E8EAED`**：`kColorToolbarText` 和 `kColorOmniboxText` 在深色都是 `SK_ColorWHITE`；`#E8EAED`（`kGoogleGrey200`）是對話框之類的 `kColorPrimaryForeground`。照原始碼用白。
- **淺色本身不是一對一照 Chrome 的**：Chrome 的工具列和目前的分頁是白的、網址列是 `#F1F3F4`；machopo 的工具列是 `#F1F3F4`、網址列再深一點（`#DEE1E5`），拿到焦點時才變白。深色照 Chrome 的深色，兩者「網址列比工具列深」的關係一樣。
- **Chrome 沒有自己的顏色、用推的**：按鈕的 hover（Chrome 是 ink drop，疊上一層半透明）、hover 的分頁、分頁的關閉鈕的 hover、分頁之間的線。都用淺色主題裡同樣兩個顏色之間的比例。分頁的關閉鈕的 hover 例外：照淺色的比例（9.5%）在深色是 `#353639`，和目前的分頁（`#35363A`）一樣，看不到，所以加到 18%。
- **側邊欄是 machopo 自己的**，Chrome 沒有對應的顏色。

## 參考資料

- Chromium：`ui/views/view.h`、`ui/views/widget/root_view.h`、`ui/views/view_targeter.h`、`ui/views/layout/flex_layout.h`、`ui/color/color_provider.h`、`chrome/browser/ui/views/frame/browser_view_layout.h`、`chrome/browser/ui/views/frame/webui_tab_strip_container_view.h`、`chrome/browser/ui/views/frame/browser_non_client_frame_view_mac.h`。
- Masonry / Xilem：https://github.com/linebender/xilem
- Blitz：https://github.com/DioxusLabs/blitz
- Zen：https://zen-browser.app
