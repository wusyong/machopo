# RFC 0050 的細節：表單控制項的繪製

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 現況（2026-10-16）

- **style**：UA stylesheet（`crates/style/resources/html.css`）對控制項只有 `input, textarea, select, button { display: inline-block }` 和 `input[type=hidden i] { display: none !important }`，沒有字型、padding、邊框、顏色。沒有 system color（`fieldset` 因此沒有邊框）、沒有 `appearance`、`accent-color`、`caret-color`。pseudo-element 只有 `::before`、`::after`、`::marker`（`css` 的 `selector.rs`、`computed_style` 的 `PseudoId`），其他的讓整條規則無效。`:focus-visible` 有 `outline: auto`（`-webkit-focus-ring-color`）。
- **layout**：replaced 只有 `<img>`、`<iframe>`、`<canvas>` 和最外層的 `<svg>`（`layout` 的 `is_replaced`、`replaced.rs`）。控制項是一般的 inline-block：`<input>` 沒有子節點，所以是 0×0；`<button>` 的子節點照一般的內容排；`<textarea>` 的子文字和 `<select>` 的 `<option>` 文字也照一般的文字排。`<input>`、`<textarea>`、`<select>` 沒有 `::before`、`::after`，`display: contents` 當成 `none`。
- **dom**：值在 `FormControlData`（`dom` 的 `form_control.rs`：`<input>` 的 value、選取範圍、checkedness，`<textarea>` 的 raw value），layout 用的 `NodeRef` 讀得到（`form_control()`），layout 現在不讀。shadow root 只有 `open` 和 `closed`，沒有 UA 的；`attach_shadow` 是唯一的入口。
- **paint**：背景、邊框、圓角、outline、文字、圖片、SVG 的 display item；`compositor_paint` 的 op 有矩形、圓角矩形、線、路徑、glyph，畫勾、圓點、箭頭用得上，但沒有 theme 的程式。focus ring 照 Chrome（2px 的 outline 色加 1px 白）。
- **text**：parley 排版；Range 的幾何（`getClientRects`）有，caret 和選取的繪製都沒有，`dom_api` 的 `editing.rs` 也還沒有打字。

## Speedometer 用到的（2026-10-16）

Speedometer 3.1（`../third_party/speedometer/resources/`）的 20 個 default suite。測試的步驟用 `node.value = …` 加合成的 `input`、`change`、`keydown` 等事件輸入文字，不用真的按鍵，也不移 caret。

| suite | 控制項 | 樣式 | 原生的大小影不影響版面 |
| --- | --- | --- | --- |
| TodoMVC（ES5、ES6-Webpack、React、Redux、Backbone、Angular、Vue、jQuery、Preact、Svelte，和 Complex-DOM 的版本） | 一個 `.new-todo` 文字框（有 placeholder）、`.toggle-all` checkbox、`.clear-completed` 按鈕；每一項一個 `.toggle` checkbox 和 `.destroy` 按鈕（Backbone、jQuery 另外有隱藏的 `.edit` 文字框） | `button` 重設成 `appearance: none`、沒有邊框和 padding；`.new-todo` 有 `width: 100%`、`height: 65px`、padding；checkbox 是 `appearance: none`、`opacity: 0`、絕對定位，勾選的圖是 label 的背景；placeholder 用 `::-webkit-input-placeholder`（不是 `::placeholder`） | 不影響：都有大小、絕對定位或透明 |
| TodoMVC-WebComponents | 同上，在 shadow tree 裡 | `input { padding: 0; border: 0; font: inherit }`；checkbox 是 `appearance: none`（不透明，所以原生外觀一定要拿掉）；`::placeholder` | 不影響 |
| Complex-DOM 的 Spectrum DOM（6 個 Complex-DOM 都一樣） | 24 個 `<input>`（12 個 checkbox、11 個文字、1 個 search）、約 35 個 `<button>` | Spectrum 的元件都 `-webkit-appearance: none`、自己給大小；`::placeholder`、`::-webkit-inner-spin-button`、`::-webkit-search-cancel-button` 等設成 `appearance: none`；checkbox 是 `opacity: .0001`，框用 CSS 畫 | 影響：`.timeline-group` 裡 5 個沒加樣式的「Owner ID」文字框在 `grid-template-columns: 1fr 4fr 1fr` 的列裡 |
| NewsSite-Next、Nuxt | 選單的 checkbox（測試點它 50 次）、幾個按鈕 | `input, button { background-color: transparent; border-color: transparent; padding: 0 }`、`input { appearance: initial }`（就是 `none`） | 不影響 |
| Editor-CodeMirror、TipTap | 工具列的 7 個 `<button>`（24×24 的 SVG 加文字） | `display: flex; margin: 0 2px; padding: 2px`，邊框和背景是原生的 | 影響：編輯區 `flex: 1` 在工具列下面 |
| Charts-observable-plot、chartjs | 5 個按鈕；observable-plot 的 `input[type=range]`（測試設 `value`）、chartjs 的 checkbox | 只有 label 的樣式 | 影響：`#chart` 排在控制項下面 |
| React-Stockcharts-SVG | 一個 `<button>`（點了之後換成圖） | 沒有 | 影響（點之前） |
| Perf-Dashboard | 工具列的 `input[type=range]`、`input[type=number]`（透明、絕對定位）；隱藏的 popover 裡有文字框、checkbox、`<select>` | range 只設高度 | range 影響 |

## 怎麼對照 Chrome

大小和 computed style 照 Chrome for Testing 實測：一份頁面放每種控制項（預設、`size`、`cols`、`rows`、`appearance: none`、作者的邊框），讀 `getBoundingClientRect()` 和 `getComputedStyle()`，寫進 `layout`、`style` 的測試當成期待值（和 RFC 0047 一樣，Blink 的原始碼抓下來對照，不進 repo）。Chrome 的控制項字型（`-webkit-small-control`，13.333px）在各平台是不同的系統字型，文字框的寬度跟著字型的平均字寬，所以文字框的寬度在測試裡用 kun 自己的字型算出的期待值，Chrome 的實測只用來對照算法。

## 選項的比較

### 範圍

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **F1 常見的控制項全部（選了，第 1 條）** | 「目標」列的全部，分三段 | 表單整個看得到，WPT 的 `html/rendering/widgets/` 能跑；UA stylesheet、system color、theme 一次定下來 | 三段；`<select>` 的 listbox、number 和 search 的部件 Speedometer 用不到 |
| F2 只做 Speedometer 用到的 | 文字框、checkbox、button、range | 最快讓 Speedometer 的版面對 | 其他控制項照舊 0×0，之後再回來加時 UA stylesheet 和 theme 要再動一次 |

### 原生外觀怎麼畫

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **N1 paint 的 theme（選了，第 2 條）** | `appearance` 是 `auto` 的控制項，paint 在背景之後加一個 theme 部件的 display item，用 `compositor_paint` 的路徑和矩形畫（Blink 的 `ThemePainter`、`NativeTheme`） | 和 Chrome 同構：`appearance`、`IsControlStyled` 有地方接；range、選單的箭頭、勾都畫得出來；兩個 renderer 都只看 display list | 一個新的 display item 和 theme 的程式（每種部件一個函式） |
| N2 UA stylesheet 用 CSS 畫 | 控制項的外觀全寫在 UA stylesheet：邊框、背景、勾是 `data:` 的 SVG 背景圖，`:checked` 時換 | 不用新的 paint 程式 | range 的把手、number 的箭頭畫不出來；`appearance: none` 要用一串規則把它們重設回去，和 Chrome 的 computed style 不同（作者讀 `getComputedStyle` 會看到 UA 的背景圖） |
| N3 只給大小 | 第 1、2 段只做大小和 CSS 的邊框背景，原生外觀之後 | 最小 | checkbox、radio、range 在畫面上是空的，使用者看不出狀態 |

### system color

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **S1 做 system color（選了，第 3 條）** | `css` 的顏色收 CSS Color 4 的 system color 關鍵字（和 Chrome 收的舊名字），computed value 是 Chrome 淺色主題的值 | UA stylesheet 照抄 Chrome 的寫法；Complex-DOM 的 Spectrum 用 `buttonface`、`canvastext`、`highlight` 等；`fieldset` 的邊框也接得上 | 一張顏色表；之後做深色時要隨 `color-scheme` 換 |
| S2 不做 | UA stylesheet 寫死 Chrome 的顏色值 | 不動顏色的解析 | Complex-DOM 用到的照舊丟掉；UA stylesheet 和 Chrome 的寫法不同，之後做 S1 時要改回來 |

### 第 1 段的選項

#### 控制項的字型

Chrome 的 UA stylesheet 給 `input`、`textarea`、`select`、`button` `font: -webkit-small-control`：一個 system font 的關鍵字，computed value 是 13.333px（10pt）和平台的系統字型。kun 的測試只有 Lato 和 Ahem（`test_fonts`，每個 generic family 都是 Lato），所以測試裡的 family 一律是 Lato。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **A1 system font 的關鍵字（選了，第 5 條）** | `font` shorthand 收 `caption`、`icon`、`menu`、`message-box`、`small-caption`、`status-bar` 和 `-webkit-small-control` 等 Chrome 收的，computed 成 `system-ui` 和 Chrome 的大小 | UA stylesheet 照抄 Chrome；作者寫的 `font: menu`、`font: message-box` 也有效 | `font` 的解析多一種值；序列化照 Chrome（computed 讀回 family 和大小） |
| A2 直接寫值 | UA stylesheet 寫 `font-size: 13.333px; font-family: system-ui` | 不動 `font` 的解析 | 作者的 system font 照舊丟掉；UA stylesheet 和 Chrome 的寫法不同 |

#### 固定大小放在哪裡

Blink 的 `LayoutTheme::AdjustStyle` 在 style 調整時，把 `appearance` 是 `auto` 的 checkbox、radio 的 `width`、`height`（是 `auto` 時）設成 theme 的大小（`getComputedStyle` 的 computed value 就是 13px）；range 的長度和文字框的寬度是 layout 算的 intrinsic size（`LayoutSlider`、`LayoutTextControl::ComputeIntrinsicLogicalWidths`）。`appearance: none` 時都不調整（checkbox 沒有內容，只剩邊框和 padding）。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **B1 照 Blink 分兩處（選了，第 6 條）** | checkbox、radio 的大小在 `style` 的調整（和 blockify 同一個地方）；range、文字框、`<textarea>` 是 `layout` 的 intrinsic size | computed value、`min-width`、百分比、`zoom` 的行為和 Chrome 一樣；layout 只多「文字框」和「range」兩種 intrinsic size | 兩處都要知道 `appearance` 是不是 `auto` |
| B2 全部在 layout | 每種控制項都是 layout 的 intrinsic size | 只有一處 | checkbox 的 `width` 的 computed value 是 `auto`（Chrome 是 13px）；`max-width` 這類跟著 intrinsic size 算，和 Chrome 的 computed 的寬度不同 |

文字框的 intrinsic size 照 Blink：寬度是 `size`（預設 20）乘字型的平均字寬（`OS/2` 表的 `xAvgCharWidth`，沒有時用 `0` 的寬度），高度是一行的 line height；`<textarea>` 是 `cols` 乘平均字寬加捲軸的寬度、`rows` 乘 line height。kun 現在沒有平均字寬（也沒有 `ch`、`ex`），要從字型讀；`ch`、`ex` 不在這份。

#### system color 的值

Chrome 的淺色 system color 多數來自 Blink 的 `LayoutTheme::DefaultSystemColor`（一張固定的表，2026-10-16 讀 Chromium 的 `layout_theme.cc`）：`buttonface` 是 `#efefef`、`buttonborder` 是 `#767676`、`field` 是白、`graytext` 是 `#808080`、`linktext` 是 `#0000ee`……；有 `ColorProvider` 時（不是 web test）按鈕、文字框、連結、選取等改從它拿。`accentcolor` 只在能給網頁看時照系統的強調色，否則是 `#0075ff`。

2026-10-16 在 Windows 11 的 Chrome for Testing 152（`--no-sandbox`）實測每個關鍵字的 computed value，和 registry 的 `HKEY_CURRENT_USER\Control Panel\Colors` 比：

| 關鍵字 | Windows 的 Chrome | 來源 |
| --- | --- | --- |
| `buttonface`（`buttonhighlight`、`buttonshadow`、`threedface`） | `rgb(240, 240, 240)` | `ButtonFace` |
| `buttontext`、`buttonborder`（`activeborder`、`inactiveborder`、`threed*shadow`、`threedhighlight`、`windowframe`） | `rgb(0, 0, 0)` | `ButtonText`（固定的表的 `buttonborder` 是 `#767676`） |
| `graytext` | `rgb(109, 109, 109)` | `GrayText`（固定的表是 `#808080`） |
| `highlight`、`highlighttext` | `rgb(0, 120, 212)`、`rgb(255, 255, 255)` | `Hilight`、`HilightText` |
| `linktext`、`visitedtext`、`activetext` | `rgb(0, 102, 204)` | `HotTrackingColor`（固定的表是 `#0000ee`、`#551a8b`、`#ff0000`） |
| `canvas`、`field`（`activecaption`、`appworkspace`、`background`、`inactivecaption`、`infobackground`、`menu`、`scrollbar`、`window`） | `rgb(255, 255, 255)` | `Window` |
| `canvastext`、`fieldtext`（`captiontext`、`infotext`、`menutext`、`windowtext`） | `rgb(0, 0, 0)` | `WindowText` |
| `inactivecaptiontext` | `rgb(128, 128, 128)` | 固定的表（`graytext` 的 `#808080`） |
| `accentcolor`、`accentcolortext` | `rgb(0, 117, 255)`、`rgb(255, 255, 255)` | 固定（系統的強調色是 `rgb(0, 120, 212)`，不給網頁看） |
| `selecteditem`、`selecteditemtext` | `rgb(25, 103, 210)`、`rgb(255, 255, 255)` | 固定 |
| `mark`、`marktext`、`-webkit-link`、`-webkit-activelink` | `#ffff00`、`#000000`、`#0000ee`、`#ff0000` | 固定 |

`-webkit-focus-ring-color` 在 Chrome 裡只有 UA stylesheet 能用（`CSS.supports` 是 false），kun 現在作者也能用。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| C1 一張固定的表 | 照 Blink 的 `DefaultSystemColor`，所有平台一樣 | 測試在每個平台上結果一樣；不用讀作業系統的設定（在 Windows 上要 `unsafe`） | 和某個平台的 Chrome 有幾個顏色不同（選取的顏色等） |
| **C2 每個平台各自的（選了，第 7 條）** | 照各平台的 Chrome：Browser process 讀 `ColorProvider` 那幾個（Windows 是八個系統色），啟動 renderer 時交給它；其他照固定的表 | 和那個平台的 Chrome 一樣 | 測試的結果跟著平台和使用者的設定變；要讀作業系統的設定 |

#### 怎麼和 Chrome 對照

和 RFC 0047 一樣，用一份暫時的頁面（不進 repo）在 Chrome for Testing 裡量每種控制項的 `getBoundingClientRect()`、`getComputedStyle()` 和 baseline，`@font-face` 載入 kun 測試用的同一份 Lato，結果寫進 `style`、`layout` 的測試當成期待值。Windows 上的 Chrome 要在工具外面加 `--no-sandbox` 才跑得起來（RFC 0012 的 `progress.md`，2026-10-16）。

### 1a 的選項

#### 現況（2026-10-16）

- 顏色在解析時就換成 RGB：`css` 的 `SpecifiedColor` 只有 `CurrentColor`、`Named(名稱, 顏色)` 和 `Rgba`，`-webkit-focus-ring-color` 是一個寫死的 `Named`；`compute()` 不拿 `ComputeContext`。computed 的 `StyleColor` 只有 `CurrentColor` 和 `Rgba`。
- `ComputeContext`（`font_size`、`root_font_size`、`base_url`、`viewport`）在 `style` 的 `cascade_blocks` 裡建，沒有 color scheme 或 theme。
- 啟動 renderer 時給的是 `RendererFlags`（能序列化：`test_fonts`、`web_tests`、`user_agent`……），Browser 那一側讀作業系統的只有 macOS 的 `objc2-app-kit`（連點的間隔）；Windows 的連點間隔因為要 `unsafe` 沒有讀。
- `font` shorthand 不收 system font（`css` 的 `font.rs` 寫明）；longhand 只有 `font-style`、`font-weight`、`font-size`、`line-height`、`font-family`。
- 沒有 `-webkit-` 的 property 別名；`property_set.rs` 寫著 CSSOM 的 `webkitFoo` 對應「等第一個 `-webkit-` 的 property」。
- style 的調整（`cascade_blocks` 的 blockify）看不到元素；元素在外面一層的 `cascade()`，hints 從那裡讀 attribute。

#### system color 在什麼時候變成顏色

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **P1 computed 時換（選了，第 9 條）** | `SpecifiedColor` 多一種 `System(關鍵字)`，序列化寫關鍵字；`compute()` 收 `ComputeContext`，從裡面的 system color 表換成 RGB（computed value 和 Chrome 一樣是 `rgb()`）；表由 `StyleEngine` 照 renderer 啟動時拿到的值建 | 和 Blink 同構（`StyleColor` 帶著關鍵字，解析 style 時照 color scheme 換）；之後做深色、forced colors 只要換表；同一份 stylesheet 給不同的表結果不同 | `compute()` 的簽名改了，用到顏色的地方都要傳 context；animation 的 keyframe 也走同一條路（已經在元素上算） |
| P2 解析時換 | 和 `-webkit-focus-ring-color` 一樣，解析時查一張 renderer 的 thread-local 表，存成 `Named` | 改動最小 | 解析和 renderer 的狀態綁在一起（`css` 的測試要先設表）；之後做深色時要改成 P1 |

#### 1a 讀作業系統的範圍

Chrome 跟著平台的是 `ColorProvider` 的那幾個（第 7 條，Windows 是八個系統色）。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| R1 Windows 和 macOS | Windows 從 registry 讀系統色（用 safe 的 registry crate，不用 `unsafe`）；macOS 用 `objc2-app-kit` 的 `NSColor`；Linux 用固定的值 | 一次做完 | macOS 的值在這台開發機上不能和 Chrome 對照 |
| **R2 先做 Windows（選了，第 10 條）** | 同 R1，但 macOS 在 Mac 上做時補（那時是固定的表） | 每個讀的值都和那個平台的 Chrome 實測對照過 | macOS 暫時和 Chrome 不同 |
| R3 只接好路 | `RendererFlags` 多一張 system color 的表，Browser 一律給固定的值 | 最小 | 第 7 條的平台差別要之後才有 |

都是啟動 renderer 時讀一次：Chromium 在設定改了時送新的 `RendererPreferences` 讓頁面重新 style，kun 先不做（新的分頁才看得到）。

#### 照 Chrome 做、不另外決定的

- **`appearance`**：收 css-ui 4 的值（`none`、`auto`、`textfield`、`menulist-button` 和 compat 的 `searchfield`、`textarea`、`checkbox`、`radio`、`menulist`、`listbox`、`meter`、`progress-bar`、`button`）和 Chrome 還收的舊值，`-webkit-appearance` 是別名（CSSOM 的 `webkitAppearance`、`WebkitAppearance` 一起加）；computed value 照寫的關鍵字（Chrome 也是），實際用的外觀另外算（Blink 的 effective appearance，不是 property）。
- **`IsControlStyled`**：照 Blink，cascade 時記下 author origin 有沒有設背景或邊框的 longhand（Blink 的 `HasAuthorBackground`、`HasAuthorBorder`），style 調整時，對 button、文字框、選單有作者樣式的，把 effective appearance 設成 `none`。
- **元素的資訊進 style 的調整**：`cascade()` 把元素是哪一種控制項（`<input>` 的 `type`、`<button>`、`<textarea>`、`<select>`）交給 `cascade_blocks`，所以 animation 的重新 cascade 也照樣調整（Blink 的 `StyleAdjuster` 也看得到元素）。
- **system font**：`font` shorthand 收關鍵字之後展開成五個 longhand（family 是 `system-ui`，大小照 Chrome）；宣告值的 `font` 照 Chrome 讀回關鍵字。

#### 1a 在 Chrome 實測的（2026-10-16，Windows 11，Chrome for Testing 152）

- **`appearance`**：收 `auto`、`none`、`checkbox`、`radio`、`button`、`listbox`、`menulist`、`menulist-button`、`meter`、`progress-bar`、`searchfield`、`textarea`、`textfield`、`base-select`；`push-button`、`square-button`、`inner-spin-button`、`slider-horizontal` 這些舊的都不收。computed value 是寫的關鍵字，初始值 `none`；`-webkit-appearance` 讀回 `appearance`，`style.webkitAppearance` 和 `style.WebkitAppearance` 都有。
- **UA 的 computed value**：文字框 13.3333px、`line-height: normal`、`rgb(118, 118, 118)` 的 2px inset 邊框、padding 1px 2px、`content-box`（search 是 `border-box`）；checkbox 13×13、margin 3px 3px 3px 4px（radio 的是 3px 3px 0 5px）、沒有邊框和背景；range 129×16、margin 2px、`color: rgb(16, 16, 16)`；按鈕 `buttonface` 的背景、`buttonborder` 的 2px outset、padding 1px 6px、置中、`border-box`；`<textarea>` 是 monospace、1px solid、padding 2px、`pre-wrap`；`<select>` 是 1px solid、`border-box`、`align-items: center`、`white-space: pre`（kun 先不寫：它的 `<option>` 現在照一般的內容排，空白留著時空的 `<option>` 會有盒子、能 focus，WPT 的 `canvas-descendants-focusability-006.html` 抓到）；disabled 的文字框 `rgb(84, 84, 84)`、按鈕 `rgba(16, 16, 16, 0.3)`，背景、邊框都是 0.3 的透明度。測試照這些寫（`style` 的 `form_controls`）。`<input>` 的 `overflow: clip` 不寫進 UA stylesheet：kun 把 `clip` 算成 `hidden`，每個 `<input>` 都會變成捲動容器（flex item 的自動最小大小是 0），和 Chrome 不同的地方更多。
- **`IsControlStyled`**：截圖比對，作者宣告了任何一個背景、邊框或圓角的 longhand（`background-image: none`、和 UA 一樣的 `border-width: 2px` 也算）就照 CSS 畫按鈕和文字框；`<select>` 照 CSS 畫但留著箭頭（`menulist-button`）；checkbox 不受影響；`color`、`padding`、`box-shadow` 不算。

### 1b 的選項

#### 現況（2026-10-16）

- `layout` 的 replaced 元素（`<img>`、`<canvas>`、`<iframe>`、最外層的 `<svg>`）是 taffy 的 leaf：`Replaced::measure` 從自然大小給 content size，taffy 照 style 的大小、min、max 解；控制項是一般的 inline-block，照子節點排（`<input>` 沒有子節點，所以只有 padding 和邊框；`<button>`、`<textarea>`、`<select>` 的子節點照一般的內容排）。
- layout 讀得到字型的 `OS/2` 表（`vertical-align: middle` 的 x-height 已經用它），平均字寬（`xAvgCharWidth`）還沒讀。

#### 照 Blink 或 Chrome 的實測做、不另外決定的

- **checkbox、radio**：effective appearance 是 checkbox、radio 時，style 調整把 `auto` 的 `width`、`height` 設成 13px（第 6 條，Blink 的 `LayoutTheme::AdjustStyle`）。
- **range**：effective appearance 是 slider 時，intrinsic size 是 129×16（Chrome 實測；Blink 的 `LayoutTheme` 的 slider 的長度）。
- **文字框的寬度**：`size`（預設 20，`<textarea>` 的 `cols` 預設 20）乘平均字寬（Blink 的 `LayoutTextControl::GetAvgCharWidth`：字型的 `OS/2` 的 `xAvgCharWidth`，`max(width, round(width))`；字型在 Blink 的例外清單裡、或平均字寬大於 `0` 的寬度的 1.7 倍時，改量 `0` 的寬度），加上 Blink 對最大字寬的調整；`<textarea>` 加捲軸的寬度，高度是 `rows`（預設 2）乘 line height。實際的算式在 1b 開始時讀 Blink 的原始碼，用 Chrome 量 kun 測試用的 Lato（`@font-face`）對照。
- **flex item 的自動最小大小**：文字框、range 照它的指定大小（Blink 的 `ContentMinimumInlineSize`），不是 0。
- **baseline**：照 Chrome 實測（文字框是它的文字的 baseline；沒有內容的 checkbox、radio、range 照 Chrome）。
- **範圍**：1b 做文字類的 `<input>`（連同 search、number、password）、`<textarea>`、checkbox、radio、range、`<button>`。`<input type=color>`、`date` 這類、`file`、`image` 和 `button` / `submit` / `reset`（大小要看 shadow tree 裡的內容）在第 2 段，`<select>` 在第 3 段。
- **不做 `ch`、`ex`**：它們是 style 算 computed value 時要的字型度量，和 layout 量文字框的不是同一個地方，之後另外做。

#### 文字控制項在 layout 裡是什麼

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **L1 有自己 intrinsic size 的 block container（選了，第 11 條）** | box tree 裡照舊是 block container；它的 min-content、max-content 的 inline size 由 `size`（`cols`）和平均字寬給，不看內容；`height: auto` 時照內容，沒有內容時是一行的 line height（Blink 的 `LayoutTextControl` 是 `LayoutBlockFlow`） | 第 2 段的 shadow tree（inner editor、placeholder）照常排在裡面，不用改它的種類；和 Blink 同構 | layout 要有「intrinsic size 由別處給的 container」的路（taffy 的 block 量 intrinsic size 時換成給的值） |
| L2 1b 先當成 leaf | 像 replaced 一樣用 `measure` 給大小，子節點不排 | 1b 最小，和 replaced 走同一條路 | 第 2 段要把它改成 container 才能排 shadow tree 的內容，1b 的大小和 baseline 的程式要重寫 |

#### `<button>` 的內容

HTML 的 rendering 一節（「The button element」）：`display` 是 `inline-block` 這類時，`<button>` 有一個 anonymous button content box，裝它的內容，在按鈕裡垂直置中；`display` 是 `flex`、`grid` 等時照那個 `display` 排（Editor 的工具列按鈕是 `display: flex`）。Blink 的 `LayoutButton` 是 flex 容器加一個匿名的 block。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **B1 照 HTML 的 button layout（選了，第 12 條）** | box tree 為 `inline-block`、`block` 的 `<button>` 建一個匿名的 block 裝內容，在 content box 裡垂直置中；`flex`、`grid` 時不建 | 按鈕有固定高度時內容置中，和 Chrome 一樣（Editor、Charts 的按鈕、TodoMVC 的 `.destroy` 的 `×`）；之後 `<input type=submit>` 的 shadow tree 也放進同樣的盒子 | box tree 多一種匿名的盒子 |
| B2 照舊 | 一般的 inline-block | 不用改 | 有高度的按鈕內容靠上，和 Chrome 不同 |

#### 1b 在 Chrome 實測的（2026-10-16，Windows 11，Chrome for Testing 152）

用 `@font-face` 載入 kun 測試用的 Lato Medium（每 em 2000 單位，`xAvgCharWidth` 1112，`head` 的 `xMin` −1122、`xMax` 2687），字級 10、13.333、16、20、30、40px，`size` 1、2、3、10、20，`<textarea>` 的 `cols` 10、20，`rows` 1、3：

- **平均字寬是 `x` 的 advance**（不是 `OS/2` 的 `xAvgCharWidth`）：寬度對 `size` 的斜率在 16px 是 8.064（`x` 是 8.064，`xAvgCharWidth` 換算是 8.896），在 13.333px 是 7（`x` 是 6.72：`max(w, round(w))` 取 7）。Blink 的 `SimpleFontData` 在 Skia 沒給平均字寬時用 `x` 的寬度；Windows 上的 Chrome 走這條。
- **最大字寬**：`round((xMax − xMin) / upem × 字級)`：13.333px 是 `round(1.9045 × 13.333)` = 25，20px 是 38；10.4px 是 20（量了 10.4px 和 16.6px 確認是乘原本的字級，不是取整的）。
- **文字框的 content 寬度** = `ceil(size × 平均字寬 + 最大字寬 − 平均字寬)`：三十組都吻合（13.333px、`size=20` 是 158，`size=1` 是 25）。
- **`<textarea>` 的 content 寬度** = `ceil(cols × 平均字寬 + 15)`（15 是 Windows 的捲軸寬度）；高度 = `rows` × line height。
- **line height（`normal`）**：ascent、descent 各自四捨五入再相加：16px 的 Lato 是 16 + 3 = 19（不是 19.2）。
- **checkbox、radio** 13×13，**range** 129×16。
- **baseline**：文字框和有文字的按鈕是 content box 的頂加上 ascent（沒有文字的文字框也是）；空的按鈕是 content box 的頂；checkbox、radio、range 是 border box 的底（下面的 margin 在 baseline 以下）；`<textarea>` 是 border box 的底。
- **flex item**：放在 50px 寬的 flex 容器裡，`width: auto` 的文字框照樣 166px（自動最小大小是它的 intrinsic size），range 在 20px 的也照樣 129px；`<textarea>` 是捲動容器（`overflow: auto`），縮成 50px。
- **block 和 stretch**：`display: block` 的文字框照樣 166px（不撐滿）；grid 的 300px 的格子和 flex column 裡，文字框、`<textarea>`、range 都 stretch 成 300px（不是 replaced 元素的 `normal` 就是 `start`）。
- **`appearance: none`**：range 照樣 129×16、baseline 照樣是 border box 的底；文字框照樣 166px；checkbox 是空的 inline-block（baseline 是 margin box 的底）。
- **高度固定的文字框**：它的行在 content box 裡垂直置中，放不下時照樣置中（`height: 10px` 的 baseline 在 border box 的頂以下 13px）。
- **table 的 `display`**：WPT 的 `css/css-tables/table-model-fixup-2.html`：`<input>` 的 `display: table` 是 block、`table-caption` 等是 inline（Blink 對 replaced 元素的調整）。1c 實測改正：不是 replaced 元素的調整，見下面的「1c 在 Chrome 實測的（2026-10-16，Windows 11，Chrome for Testing 152）」。
- **按鈕**：`Go` 是 33.234×22（文字 17.234 加 padding 和邊框），空的是 16×6；`height: 40px` 時內容垂直置中。

#### 1c 在 Chrome 實測的（2026-10-16，Windows 11，Chrome for Testing 152）

和 WPT 的 `html/rendering/widgets/` 對照時量的（`getComputedStyle`、`getBoundingClientRect` 和截圖）：

- **`display` 的調整**（Blink 的 `LayoutTheme::AdjustStyle`）：`appearance` 不是 `none` 的元素，`inline`、`inline-table` 和 table 內部的值算成 `inline-block`，`table`、`list-item` 算成 `block`，`flex`、`grid` 等不變；computed value 就是調整過的。對象看 `appearance` 的值，不看 effective appearance：作者給了邊框的文字框（effective 是 `none`）照樣調整，`<span style="appearance: auto">` 也是 `inline-block`、`appearance: auto` 的 `<li>` 是 `block`（沒有 marker；WPT 的 `appearance-auto-details-list-item.html` 在 Chrome 也失敗）。`appearance: none` 的照它自己的（`display: table-cell` 的 `<input type=file>`、`appearance: none` 的文字框都還是 `table-cell`）。hidden 的 `<input>` 是 `appearance: none`。WPT 的 `button-layout/computed-style.html` 期待 `display` 不變，Chrome 自己也不過。
- **`display: contents`**：`<input>`、`<select>`、`<textarea>`、`<img>`、`<canvas>`、`<video>`、`<iframe>`、`<meter>`、`<progress>` 的 computed value 是 `none`，`<button>` 是 `contents`。
- **按鈕在 block 裡**（`<button>`、`<input type=button>`、`submit`）：`width: auto` 是 fit-content，`display` 是 `block`、`flex`、`grid` 都是（`abc` 是 26.39px，不撐滿 400px）；放不下時是 min-content（30px 寬的容器裡是 35.19px）；`width: 50%` 照舊；flex column、grid、`flex: 1` 的 flex item 撐滿；`left: 0; right: 0` 的絕對定位撐滿 300px。它是 flow root：旁邊有 100px 的 float 時從 100px 開始，包住自己的 float（高 50px）。WPT 的 `display-other.html`：`display: block` 的按鈕包住有 10px margin 的 float 時，`clientHeight` 是 122。
- **`::before`**：checkbox、radio、range、color、file、date 的 `<input>` 和 `<button>` 畫它（在控制項的盒子裡，有原生外觀時照樣畫）；文字類、`button`、`submit`、`reset`、`image` 的 `<input>`，`<select>`、`<textarea>` 不畫（Blink 的 `CanHaveGeneratedChildren`）。
- **`about:demo` 的控制項**（同一份 Lato，13.333px）：文字框 166×22、`<textarea>` 161×38、range 129×16、checkbox 和 radio 13×13；`height: 40px` 的按鈕的文字在 border box 的頂以下 12px。

### 第 2 段的選項

#### 文字控制項的內部

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **T1 UA shadow tree（選了，第 14 條）** | `dom` 多一種 UA shadow root（script 拿不到），文字類的 `<input>` 和 `<textarea>` 裡面是 inner editor 和 placeholder 的元素；`::placeholder`、`::-webkit-*` 是 shadow 裡的元素的 pseudo-element 名稱；值改了時更新 inner editor 的文字 | 和 Blink、WebKit、現在的 Servo 同構；layout 和 paint 走一般的 flat tree，`::placeholder` 的 style 走一般的 cascade；之後的編輯（caret、選取）在 inner editor 的文字節點上做，Range 的幾何現成；`<select>`、number、search 的部件用同一個機制 | `dom` 要有 UA shadow root 的概念，script、selector、事件、`getAnimations()` 都不能進去；每個文字框多幾個節點；值和 inner editor 的文字要同步 |
| T2 layout 的匿名盒子 | box tree 為文字框建一個匿名的 block，內容是值的文字（像 `content` 的字串）；placeholder 是一個像 `::before` 的 pseudo-element | 不動 `dom`，節點不變多 | 編輯時 caret 和選取要另外對應到值的文字（Range 的幾何用不上）；`::-webkit-*` 每一個都要特別處理；Servo 後來也放棄這個做法 |

kun 現有的基礎（2026-10-16）：`dom` 有 shadow root、flat tree、slot，style 照 tree scope 分開比對（每個 shadow tree 只比對 UA 和它自己的 stylesheet），排版和繪製照 flat tree。T1 要加的是 UA 的 mode（`shadowRoot` 拿不到、事件和 selection 不進去）、`dom_api` 在 `type` 改了時建和換這棵樹、值改了時同步文字節點，selector 讓 `::placeholder` 比對到 shadow 裡的元素。文字的編輯現在以值的 offset 為單位（`dom` 的 `form_control`），之後的 caret 照 offset 對到 inner editor 的文字節點。

#### 原生外觀照哪個平台

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| A1 所有平台一套 | 都畫 `NativeThemeBase`（Windows、Linux、ChromeOS 共用；Chrome 的 web test 也用它） | 做一次，這台 Windows 上能逐一對照 | macOS 上和那裡的 Chrome 不同 |
| **A2 照每個平台（選了，第 15 條）** | Windows、Linux 畫 `NativeThemeBase`，macOS 畫 `NativeThemeMac` 的樣子 | 和使用者那個平台的 Chrome 一樣 | macOS 的那套要在 Mac 上量、對照，晚一點補 |

#### 2a 的選項

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| E1 建立元素和 `type` 改了時就建 | `dom_api` 在 `<input>`、`<textarea>` 建立時和 `type` 改了時建或換掉 shadow tree | 任何時候讀都已經在，style、layout、`getComputedStyle` 不用先處理排定的 | 沒插進 document 的也有這棵樹（kun 的 parser 建元素時 attribute 已經在，Blink 省下的「先建成 text 再改 type」kun 本來就沒有） |
| **E2 照 Blink 延後建（選了，第 17 條）** | 插進 document 時排定（Blink 的 `ScheduleShadowTreeCreation`），下一次算 style 之前建；要用到的入口先建好（`EnsureShadowSubtree`） | 和 Blink 同構；沒插進 document 的不建 | 每個會算 style 的入口要先建好排定的樹 |

照 Blink 做、不另外決定的：UA shadow root 是 shadow root 的一種 mode（Blink 的 `ShadowRootMode::kUserAgent`：`shadowRoot` 拿不到，`attachShadow` 本來就不收 `<input>`、`<textarea>`）；`::placeholder`、`::-webkit-input-placeholder` 照 Blink 的 shadow pseudo id 比對到 shadow 裡的元素（host 的 tree scope 的規則）；值改了時同步 inner editor 的文字節點（kun 的編輯以值為準，只有一個方向）；password 照 Chrome 的 `-webkit-text-security: disc` 畫成圓點。

#### 2b 的選項

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **L1 預設文字固定英文（選了，第 18 條）** | `Submit`、`Reset`，和 Chrome 的 `en-US` 一樣 | 和 WPT、Chrome 的 web test 一致，按鈕的寬度可預期 | 中文環境的 Chrome 顯示「提交」等 |
| L2 照瀏覽器的語系 | Chrome 照語系翻譯（`QueryLocalizedString`） | 和使用者那個語系的 Chrome 一樣 | kun 沒有語系的機制，要另外做；寬度跟著語系變 |
| **B1 部件只做繪製（選了，第 19 條）** | 清除鈕、上下鈕有盒子和大小，有焦點或 hover 時顯示；點了不做事 | 符合這份的範圍，2b 小 | 畫出來的部件按了沒反應 |
| B2 連點擊一起做 | Blink 的 `SearchFieldCancelButtonElement`（清空、發 `input`）、`SpinButtonElement`（`stepUp`、`stepDown`，按住連續） | 一畫出來就能用 | 事件、計時器、焦點的互動都要做、要測 |

2026-10-16 在 Chrome 量的：沒有焦點和 hover 時，search 的清除鈕和 number 的上下鈕都不顯示（Blink 的 UA stylesheet 是 `opacity: 0; pointer-events: none`，`input:enabled:read-write:-webkit-any(:focus, :hover)` 時是 `opacity: 1`）；有焦點時清除鈕是值右邊的藍色 `×`，上下鈕是兩個灰色箭頭。`--lang=en-US` 時 `submit` 是 57.16×22、`reset` 48.66×22（Lato 13.333px），沒有 `value` 的 `button` 是 16×22（只有 padding 和邊框）。

#### 2c 的選項

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **P1 paint 時展開成現有的 op（選了，第 20 條）** | paint 的 `theme` 照部件和狀態記成圓角矩形、填色、路徑、線（Blink 的 `ThemePainter` 在 `PaintCanvas` 上畫） | raster、合成、截圖、兩個 renderer 都不用改；和 Chromium 同構；測試看 display list | 一個部件是幾個 op |
| P2 新的 display item | display list 多一種 theme 部件，raster 照它畫 | display list 比較小 | raster、Viz、跨 process 的格式都要多一種；和 Chromium 不同 |
| **S1 有互動狀態（選了，第 21 條）** | hover、按下時換顏色（`NativeTheme` 的 `kHovered`、`kPressed`） | 和 Chrome 一樣 | 有原生外觀的元素在 hover、active 改變時要重畫 |
| S2 只有靜態的狀態 | 勾選、未定、停用 | 不用加重畫 | 滑鼠沒有回饋 |
| C1 狀態放在 `ComputedStyle` | 狀態改了只複製上一份 style、換掉狀態（不重新 cascade），差異是 `Paint`，走現有的 restyle | `dom`、`layout` 不用改，程式最少 | `ComputedStyle` 多一個不是 CSS 的欄位；新舊 style 的比較要排除它（不觸發 transition）；和 Blink 不同 |
| **C2 狀態跟著 fragment（選了，第 22 條）** | layout 建 fragment 時從 DOM 讀；`dom` 在狀態改變時標記要重做 fragment | 和 Blink 同構，`ComputedStyle` 只有 CSS；狀態的來源只在 DOM | `dom`、`layout`、`paint` 都要改，程式量約 C1 的兩倍；hover 移動時沿祖先鏈多標一個 flag |
| **R1 range 用 UA shadow tree（選了，第 23 條）** | track 和 thumb 的元素，`dom_api` 算比例，layout 照比例和把手的寬度放把手 | 作者的 `::-webkit-slider-*` 有效；和 Blink 一樣 | 每個 range 多幾個節點 |
| R2 paint 直接畫 | layout 算比例，paint 畫軌道和把手 | 小 | 作者的 `::-webkit-slider-*` 沒有效果；layout 要重寫 range 的值的演算法 |

兩種狀態的做法效能一樣：都是那一個盒子重做 fragment、重畫，不重排。

Chromium 的顏色（`ui/color` 的 `CompleteDefaultWebNativeRendererColorIdsDefinition`，淺色）：accent `#0075FF`（hover `#005CC8`、按下 `#3793FF`、停用 `#7676764D`）；邊框 `#767676`（hover `#4F4F4F`、按下 `#8D8D8D`、停用 `#7676764D`），按鈕的邊框同；按鈕的填色 `#EFEFEF`（hover `#E5E5E5`、按下 `#F5F5F5`、停用 `#EFEFEF4D`），range 的軌道（`Fill`）同；checkbox 的背景白色（停用 `#FFFFFF99`）；range 的值和把手 `#0075FF`（hover `#005CC8`、按下 `#3793FF`、停用 `#CBCBCB`），軌道的邊框是邊框色；停用的按鈕先疊一層 `#A9A9A933`。幾何（`ui/native_theme/native_theme_base.cc`）：checkbox 13×13、圓角 2、邊框 1（內縮 0.5），勾是 (0.2w, 0.5h) → (0.4w, 0.7h) → (0.8w, 0.3h) 的線、寬 0.16h、背景色；未定是置中的 8×2；radio 是圓，勾選時邊框是 accent、裡面是直徑 60% 的 accent 圓；按鈕和文字框圓角 2、邊框 1；range 的軌道高 8、圓角 40（膠囊），把手 16×16 的圓、只填色；上下鈕是兩個箭頭（`PathForArrow`）。

#### 分段的順序

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **2a 文字、2b 部件、2c theme（選了，第 16 條）** | 先做 shadow tree、值、placeholder，再做 `<input>` 按鈕的文字和 `::-webkit-*` 部件，最後畫原生外觀 | Speedometer 的 TodoMVC 用到的先做；2b 直接用 2a 的機制 | checkbox、radio 要到 2c 才畫得出來 |
| 先做 theme | 先畫原生外觀，再做文字 | theme 比較獨立、比較小 | Speedometer 用到的要晚一段 |

### 第 3 段的選項

#### 現況（2026-10-16）

`<select>` 有 UA stylesheet 的邊框、背景和 `appearance`（1a），effective appearance 分得出 menulist 和 listbox（`size`、`multiple`），但沒有 shadow tree、沒有原生外觀：`<option>` 的文字照一般的 inline 文字排在框裡（全部的 option 連在一起），`<optgroup>` 的 label 不畫，`<hr>` 畫成一般的線。kun 沒有捲軸（不畫，也不佔寬度），也沒有 `:-internal-*` 的 pseudo-class。

#### 在 Chrome 量的（2026-10-16，Windows 11，Chrome for Testing 152，Lato 13.333px）

量的頁面先等 `document.fonts.load()`：`--dump-dom` 在 `@font-face` 的字型載入之前就量，第一次量到的是替代字型的大小（選單 19 高、`A much longer option` 140 寬），3a 開始時才發現，下面是重量的。

- 收起來的選單（menulist）：高 20（字型的高度 16 + 上下 1 的 padding + 邊框），`line-height` 改了也一樣（computed 是 `normal`）；寬是最寬的 option 的 label（取整）+ 4 + 16 + 邊框：沒有 option 22、`A` 32、`A much longer option` 149（不管選的是哪一個）；20px 的字 57×28。`label` 量的是它原本的樣子（`"  a   b "` 連頭尾的空白），顯示的去掉頭尾的空白；`<optgroup>` 裡的 option 多量 4 個空白的縮排；`multiple` 又 `size=1` 的選單顯示「N selected」（選了一個時是那個 option），也量「N selected」（N 是 option 的數目）。computed `white-space: pre`、`overflow: clip`、`align-items: center`、`border-radius: 0`，畫出來是 2px 的圓角（theme 畫，作者的背景或邊框改成 CSS 畫、留著箭頭）；箭頭是 8×4 的 `∨`，線寬 2 個 device pixel（縮放時線寬不變，箭頭的大小跟著縮放），顏色是 `color`，在右邊 15px 的中間；停用時 `opacity: 0.7`、字和箭頭是 `GrayText`。沒有原生外觀時，寬一樣照 label（沒有 4 和 16），高照 `line-height`（`30px` 時 32），`normal` 時量到 17 或 18（隨頁面不同，沒找到規則）。
- baseline：選單的文字的。16px 的文字旁邊，選單的底比文字的底低 2.5（40px 高的選單低 12.5），kun 是 2 和 12：Chrome 的選單的文字高半個 pixel。
- listbox（`size` > 1，或 `multiple`）：每一列 17（`option` 的 `padding: 0 2px 1px`，行高 16），列高是最高的 option 的（有一個 30px 的 option 時 37），高 = 列高 × `size` + 邊框（`size=4` 是 70，沒寫 `size` 的 `multiple` 也是 4 列，20px 的字 `size=3` 是 77），寬是最寬的 option + 15（捲軸，`overflow-y: scroll` 永遠留）+ 邊框；`border-radius: 2px`、`align-items: flex-start`、`white-space: nowrap`。
- `<optgroup>` 的 label 是粗體的一列，裡面的 option `padding-left: 20px`；`<hr>` 沒有線，是 0.5em 的上 margin。
- 選到的 option：有焦點時 `SelectedItem`（`rgb(25, 103, 210)`）和白字，沒有焦點時 `rgb(206, 206, 206)` 和 `rgb(16, 16, 16)`；停用的 option 是 `rgb(109, 109, 109)`，停用的 select 裡選到的是灰底灰字。

#### Blink 怎麼做（2026-10-16 的 main）

- UA stylesheet（`html.css`）：`select:-internal-list-box`（Blink 用它指 listbox）的圓角、`overflow`、`white-space`，`select:-internal-list-box:focus option:checked` 和沒有焦點的 `option:checked` 的顏色，`optgroup option` 的縮排，`hr` 的 margin；`option`、`optgroup` 是 `display: block`。
- menulist（`MenuListSelectType::CreateShadowSubtree`）：UA shadow tree 裡是 `MenuListInnerElement`（一個 `<div>`，子節點是選到的 option 的文字，`TextIndentedToRespectGroupLabel` 去掉頭尾空白）和收 option 的 slot（在 popup 裡，不排）；inner element 是 flex item（`flex: 1 1`、`min-width: 0`、`margin` 是 theme 的 4 和 16、`padding` 上下 1、`overflow-x: clip`、`text-overflow` 照 select）。不管 `display`，menulist 一律是 `LayoutFlexibleBox`、listbox 是 `LayoutBlockFlow`。`multiple` 又 `size=1` 的 menulist 顯示「N selected」之類的文字。
- 大小（`LayoutBox::MenuListIntrinsicInlineSize`、`MenuListIntrinsicBlockSize`）：最寬的 option 的文字用 select 的字型量，取整，加 theme 的 padding（`LayoutThemeDefault`：start 4、end 1 + 箭頭的 15）；高是字型高度加上下 1。listbox 的高是列高（最高的 option 或 optgroup label）× `size`（預設 4），寬照內容。
- 繪製（`ThemePainterDefault::PaintMenuList`、`NativeThemeBase::PaintMenuList`）：沒有作者的圓角時照文字框畫（圓角 2、1px 的邊框色、填色是 checkbox 的背景色，CSS 的背景不管），再畫箭頭（`SetupMenuListArrow`：8×4，不閉合的 `∨`，線寬 2）；`menulist-button`（作者改了背景或邊框）只畫箭頭。listbox 沒有原生外觀的部件，照 CSS 畫。
- `appearance: base-select`（可自訂的 select）要作者寫才有，預設是 `auto`；這一段不做。

#### 選項

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **M1 選單照 Blink 用 UA shadow tree（選了，第 24 條）** | `dom_api` 給 menulist 建 inner element（選到的 option 的文字）和不排的 slot，選的改了、option 的文字改了時同步；layout 照 Blink 當成 flex container | 和 2a、2b 同一套機制，結構和 Chrome 一樣；文字的 clip、省略號、字型和方向照一般的盒子 | 每個選單多 2 個節點，選的改變時要同步 |
| M2 layout 直接拿選到的 option 的文字 | layout 建一個匿名的文字盒子 | 節點少 | layout 要讀 DOM 的選取，`text-overflow`、字型、方向要另外處理；和 Chromium 不同 |
| **S1 listbox 照 Chrome 留 15px 的捲軸寬度（選了，第 25 條）** | 版面和 Chrome 一樣，但 kun 沒有捲軸，那 15px 是空的 | 寬度和 Chrome、WPT 一致 | 看得到一條空白；kun 其他的捲動容器不留 |
| S2 listbox 不留 | 和 kun 其他的捲動容器一樣（不佔寬度） | 不會有空白 | 比 Chrome 窄 15px；之後做捲軸時要改 |
| **B1 listbox 只做繪製（選了，第 26 條）** | option 照選取的狀態畫，滾輪照常捲；點 option 不改選取 | 符合這份的範圍（繪製），這段小 | 點了沒反應 |
| B2 listbox 連點選一起做 | 點 option 選它（`multiple` 時 Ctrl、Shift 加選），發 `input`、`change` | 一畫出來就能用 | 是行為，事件、焦點、`multiple` 的規則都要做、要測 |
| **3a 選單、3b listbox（選了，第 27 條）** | 3a 選單的 shadow tree、大小、外框和箭頭；3b listbox 的版面、選取的顏色、`<optgroup>`、`<hr>`；各自和 Chrome 對照、跑 WPT | 每段小，3a 做完就有一半的頁面對了 | 兩段 |
| 一段做完 | 選單和 listbox 一起 | 少一輪對照 | 一段很大 |

照 Blink 做、不另外決定的：`:-internal-list-box` 是只在 UA stylesheet 裡收的 pseudo-class（Blink 也是）；選單的文字是選到的 option 的 label、沒有時是空的；`multiple` 又 `size=1` 的選單照 Chrome 的英文文字（和第 18 條一樣固定英文）；選單的原生外觀照 2c 的 theme（第 20 條），作者改了背景或邊框時只畫箭頭（`menulist-button`，1a 已經分得出）；`appearance: base-select` 和下拉的選單（popup）不做。

## 參考資料

- HTML 的 rendering 一節：「Form controls」、「The input element as a text entry widget」等（`html/rendering/widgets/` 的測試照它）。
- CSS Basic User Interface 4 的 `appearance`；CSS Color 4 的 system color。
- Blink：`third_party/blink/renderer/core/html/resources/html.css`、`core/layout/layout_theme.cc`、`core/paint/theme_painter*.cc`、`ui/native_theme/native_theme_base.cc`、`core/html/forms/text_control_element.cc`、`core/layout/forms/layout_text_control*.cc`、`layout_button.cc`；`<select>` 的 `core/html/forms/select_type.cc`、`menu_list_inner_element.cc`、`core/layout/layout_box.cc` 的 `MenuListIntrinsicInlineSize`、`layout_theme_default.cc`。
