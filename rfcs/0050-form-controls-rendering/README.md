# RFC 0050：表單控制項的繪製

- 開始日期：2026-10-16
- 狀態：✅ 已完成：2026-10-17（第 1 到 27 條 2026-10-16 接受；三段都做完，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`；進度：`progress.md`

## 摘要

RFC 0012 路線圖的 G 的第二段（CSS 線）：讓 `<input>`、`<button>`、`<textarea>`、`<select>` 有 Chrome 的預設大小和樣子，畫出文字框裡的值和 placeholder。照 Chromium 分成三層：UA stylesheet 給控制項的字型、padding、邊框和顏色（system color、system font），`appearance` 決定要不要原生的外觀；layout 照 Blink 的 `LayoutTheme` 給 checkbox、radio、range 固定的大小，文字框照 `size`、`cols`、`rows` 和字寬算，選單照最寬的 option，listbox 照 `size` 列；原生的外觀由 paint 的 `theme` 畫（Blink 的 `ThemePainter`、`NativeThemeBase`，Windows 和 Linux 的 Chrome 的樣子）。控制項的內部照 Blink 是 UA shadow tree：文字框的 inner editor 和 placeholder、按鈕的文字、search 和 number 的部件、range 的軌道和把手、選單的 inner element、listbox 和 optgroup 的 slot，`::placeholder` 和 `::-webkit-*` 指到裡面的元素。打字、caret 和使用者的選取不在這份（路線圖的 P）。

## 動機

- **Speedometer**：kun 的控制項原本沒有自己的大小（`display: inline-block`、0×0），也畫不出來。TodoMVC 多半自己給了大小、`appearance: none` 或透明，所以版面大致對，但 `.new-todo` 的 placeholder 和值畫不出來；沒加樣式、照原生大小排的控制項讓下面的版面和 Chrome 不同：兩個 Editor 的工具列按鈕（編輯區的高度跟著它）、Charts 的按鈕、range 和 checkbox、Perf-Dashboard 的 range、Complex-DOM 的 5 個「Owner ID」文字框（在 grid 的一列裡）、React-Stockcharts-SVG 的按鈕（`details.md` 的「Speedometer 用到的（2026-10-16）」）。
- **一般網頁**：表單在畫面上是空白的，使用者只能靠 `<label>` 和鍵盤操作；登入框、搜尋框、按鈕是最常見的元素。
- **WebDriver 和 WPT**：文字控制項 0×0，點不到，wdspec 的鍵盤測試在 fixture 就失敗；`html/rendering/widgets/`、`css/css-ui/appearance-*` 都還不能跑。

解決的 `docs/limitations.md` 的限制：「Bindings 和 DOM API」的「表單控制項的行為」的「控制項還畫不出來……大小是 0……`<select>` 的 `<option>` 的文字照一般的文字顯示」；「Style」的顏色那一條的 system color、pseudo-element 那一條的 `::placeholder`；「WebDriver 和自動化」的「文字控制項的大小是 0，點不到」。

**目標**（第 1 條）：`<input>` 的文字類（`text`、`search`、`password`、`email`、`url`、`tel`、`number`）、`checkbox`、`radio`、`range`、`button` / `submit` / `reset`，`<button>`、`<textarea>`、`<select>`（收起來的選單和 listbox）；`appearance`（`auto`、`none` 和 Chrome 收的關鍵字）、system color、system font、`::placeholder` 和 `::-webkit-input-placeholder`、`::-webkit-inner-spin-button` 等 Chrome 的 `::-webkit-*` 部件（能用 `appearance: none` 拿掉）。

**不在範圍內**：打字、caret、在文字框裡選取（路線圖的 P；script 設的 `value` 畫得出來）；`<select>` 的下拉選單（popup）、點選 listbox 的 option，date、color、file 的選擇器；search 的清除鈕和 number 的上下鈕的點擊；`color-scheme` 的深色、forced colors；`accent-color`；`<meter>`、`<progress>`；`field-sizing`；`appearance: base-select`；macOS 的那一套外觀。

## 設計概要

網頁的作者看到的：

- `<input>` 是一個 Chrome 大小的文字框（邊框、padding、預設 20 個字寬），`value` 和 `placeholder` 畫在裡面，password 畫成圓點；`input::placeholder { color: gray }` 有效。
- `<input type=checkbox>`、`radio`、`range` 是 Chrome 的大小和樣子，勾選、未定、停用、hover、按下時照 Chrome 換；`appearance: none` 之後什麼都不畫，只剩作者給的邊框和背景（TodoMVC 的勾選靠 label 的背景圖）；`::-webkit-slider-thumb` 這些部件可以照常改。
- `<button>` 和按鈕類的 `<input>` 有 Chrome 的邊框、背景和 padding，內容置中；作者給了背景或邊框時照 Chrome 不再畫原生的外觀。
- search 有清除鈕、number 有上下鈕（有焦點或 hover 時顯示，點了不做事）。
- `<select>` 畫成收起來的選單（選到的 option 的 label 和箭頭，和最寬的 option 一樣寬）；有 `multiple` 或 `size` 時是 listbox（`size` 列高，選到的 option 有底色）。
- `color: buttontext`、`background: field` 這些 system color 有 Chrome 的值（Windows 上照系統的設定），`font: caption` 這些 system font 有 Chrome 的大小。

改 kun 的人看到的：`style` 的 UA stylesheet 多了控制項的規則，`theme` 模組決定 effective appearance 和部件的大小；`layout` 的 `control` 模組給控制項自己的大小和 baseline；`paint` 的 `theme` 模組把原生外觀展開成一般的繪圖 op；`dom` 有 UA shadow root（script 拿不到），`dom_api` 的 `text_control_tree` 建它、跟著控制項的狀態改它。

## 詳細設計

選項的比較、Chrome 的實測和 Blink 的做法在 `details.md`。

### 三層

1. **style**：UA stylesheet 照 Chrome 的 `html.css` 給控制項的規則（`font: -webkit-small-control`、padding、邊框、`buttonface` 的背景……），所以有 system color（第 3、7、9、10 條：CSS Color 4 的關鍵字，computed value 是顏色；Windows 上從 registry 讀八個系統色、啟動 renderer 時交給它，其他平台和測試用固定的表）和 system font（第 5 條，`system-ui` 和 Chrome 的大小）。`appearance` 照 Chrome 收關鍵字，`theme` 模組照元素和作者的背景、邊框、圓角算出 effective appearance（Blink 的 `AutoAppearanceFor`、`IsControlStyled`），有 `appearance` 的元素不是 table 的部分、不是 `inline`（`LayoutTheme::AdjustStyle`），checkbox、radio 的 13px 和部件的大小也在這裡（第 6 條）。
2. **layout**：`control` 模組給控制項自己的大小和 baseline，在 block 裡照 replaced 元素的方式算（`width: auto` 是它自己的寬度），在 flex 和 grid 裡照一般的盒子 stretch（第 11 條）：文字框照 `size` 和平均字寬、`<textarea>` 照 `cols`、`rows`（Blink 的 `LayoutTextControl`），range 是 129×16；按鈕照 HTML 的 button layout（第 12 條：內容垂直置中、在 block 裡 `width: auto` 是 fit-content、是 flow root）；選單和 listbox 見下面。UA shadow tree 的內容排在控制項自己的大小裡，不撐開它。
3. **paint**：effective appearance 是原生外觀的盒子，`theme` 模組照部件和狀態（跟著 fragment，第 22 條）展開成圓角矩形、填色、路徑和線（第 20 條），畫在 CSS 的背景和邊框的位置：checkbox 的框和勾、radio 的圓和點、按鈕和文字框的框、range 的軌道和把手、清除鈕的 `×`、上下鈕的箭頭、選單的框和箭頭（`NativeThemeBase` 的幾何、Chromium 淺色的 `ColorProvider` 的顏色，Windows 和 Linux 的那一套，第 15 條）。hover、按下、勾選改變時只重做 fragment、重畫（第 21、22 條）。

### UA shadow tree（第 14、17 條）

照 Blink：控制項有一棵 script 拿不到的 shadow tree（`ShadowRootMode::UserAgent`：`shadowRoot` 是 `null`，事件和 selection 照 closed 的），插進有 browsing context 的 document 之後、下一次算 style 之前建（Blink 的 `ScheduleShadowTreeCreation`）：

- 文字類的 `<input>` 和 `<textarea>`：inner editor（子節點是值的文字）和有 `placeholder` 時的 placeholder 元素，search 和 number 的外面是 decoration container 和清除鈕、上下鈕（Blink 的 `TextControlElement`、`TextFieldInputType`）。
- 按鈕類的 `<input>`：它的文字（`value`，沒有時 `Submit`、`Reset`，第 18 條）。
- range：container、track 和 thumb，thumb 記著值的比例，layout 照它放（第 23 條）。
- `<select>`：收起來的選單是 inner element，listbox 是一個 slot；`<optgroup>` 是它的標題和一個 slot，`<option>` 是它的 label 和一個 slot（有 `label` 時顯示 label）。

`::placeholder`、`::-webkit-*` 是指到裡面的元素的 pseudo-element（Blink 的 shadow pseudo id）：`::-webkit-` 開頭的都收，後面收狀態的 pseudo-class；部件的規則問的 host 的狀態，只給 UA shadow tree 的 host 帶上（不然每個 `:hover` 的變化都要重算）。值、placeholder、`type`、option 的選取和文字改了時，`dom_api` 照著改樹；`type` 要另一棵樹時拿掉子節點重建（root 留著）。

### `<select>`（第 24 到 26 條）

- **收起來的選單**：inner element 的文字是選到的 option 的 label（去掉頭尾的空白；`multiple` 又 `size=1` 時是「N selected」），它也記著每個 option 的 label，layout 照最寬的（select 的字型、取整）加上原生外觀的 4px 和 16px 算寬度（Blink 的 `MenuListIntrinsicInlineSize`），高是字型的高度加上下 1px（`line-height` 重設成 `normal`）。inner element 是一個 block，有那些 margin 和 padding，在選單裡安全地置中（放不下時在頂端），切掉寬出來的文字；baseline 是它的文字的。paint 畫文字框的圓角框和 `∨` 箭頭（線寬固定 2 個 device pixel），作者改了背景或邊框時照 CSS 畫、箭頭畫在上面。
- **listbox**：option 照 block 排（UA 的 `option`、`optgroup` 的規則，`:-internal-list-box` 只有 UA stylesheet 收），高是 `size` 列（預設 4），每列是最高的 option 或 optgroup 的標題（排過一次之後量；Blink 的 `ListBoxItemBlockSize`），自己的 `height`、`min-height`、`max-height` 照常；寬照內容，再留 15px 給捲軸（第 25 條，kun 不畫捲軸）。選到的 option 照 Chrome 的 UA 規則上色，有焦點時是 `SelectedItem`；這些規則的 `:focus` 只問 listbox 自己的焦點。點 option 不改選取（第 26 條）。

### 測試

- **整合測試**：`style` 的 `form_controls`（UA 規則的 computed value 和 Chrome 實測的比、effective appearance、部件和 placeholder 的規則、listbox 的顏色）、`cascade`、`cssom`、`stylesheet`、`invalidation`；`layout` 的 `form_controls`（每種控制項的大小、baseline、置中，數字是 Chrome 152 用同一份 Lato 量的）和 `update`（DOM 改了之後和從頭建的一樣）；`paint` 的 `theme`（從 HTML 到 display list 的每個部件）和 `paint_controller`（狀態改了重畫）；`dom_api` 的 `forms`（每種 UA shadow tree）；`dom` 的 `flat_tree`、`layout_tree_flags`、`tree`。
- **WPT**：`html/rendering/widgets/` 整個、`css/css-ui/` 的 `appearance-*`、`css/css-pseudo/` 的 `placeholder-*`、`css/css-color/` 的 system color、`css/css-fonts/parsing/font-computed.html`（第 13 條），失敗的 `.ini` 寫了原因。
- **情境**：`about:demo` 的表單（原生外觀的 checkbox、radio，沒加樣式的控制項的大小、選單和 listbox），`demo-forms` 和 `form-pseudo-classes`；外觀在 `test.py automation` 的截圖裡看。
- **mutation test**：每一小段一份（`mutants-*.toml`），每段做完時照最後的程式全部重跑。
- **量測**：不排（Speedometer 只在使用者要求時量）。

### 分階段

第 4 條，每段做完、審核之後先 merge 回 `main`：

1. **style 和大小**（第 8 條，1a、1b、1c）：system color、`appearance`、system font、UA stylesheet 的控制項規則、`IsControlStyled`；控制項的大小、baseline 和 button layout；和 WPT、Chrome 對照。
2. **原生外觀和文字**（第 16 條，2a、2b、2c）：文字控制項的 UA shadow tree、值和 placeholder；按鈕類 `<input>` 的文字、search 和 number 的部件；paint 的 theme 和 range 的 shadow tree。
3. **`<select>`**（第 27 條，3a、3b）：收起來的選單；listbox、`<optgroup>`、option 的 label。

## 缺點

- system color 和 UA stylesheet 改了之後，有控制項的 reftest、WPT 和情境的版面都變了，預期重新寫過。
- theme 照著 Chrome 的樣子畫，Chrome 換了外觀時要跟著改；macOS 上畫的不是那裡的 Chrome 的樣子。
- UA shadow tree 讓每個控制項多幾個節點（文字框 2 到 3 個，每個 option 3 個），值和選取改了時多一次同步；Chrome 也是。

## 理由和其他做法

見 `details.md` 的「選項的比較」和各段的選項。不做的話，表單照舊空白，Speedometer 的幾個 suite 版面和 Chrome 不同。

## 先例

- **Blink**：UA stylesheet（`html.css`）、`LayoutTheme`（`AdjustStyle`、`IsControlStyled`、預設大小）、`ThemePainter` 和 `NativeTheme`（各平台的外觀，controls refresh 之後 Windows、Linux 共用 `NativeThemeBase`）；文字控制項是 `TextControlElement` 的 UA shadow tree（`TextControlInnerEditorElement`、placeholder 元素），`LayoutTextControlSingleLine`、`LayoutTextControlMultiLine`；`LayoutButton`；`<select>` 的選單是 `MenuListSelectType` 的 shadow tree 和 `LayoutFlexibleBox`（`LayoutMenuList` 已經拿掉），listbox 是 `ListBoxSelectType` 的 slot 和 `LayoutBlockFlow`。
- **Gecko**：文字框用 native anonymous content（`nsTextControlFrame` 建匿名的 `<div>`），原生外觀用 non-native theme（`nsNativeBasicTheme`，所有平台同一套，用程式畫）。
- **WebKit**：和 Blink 同源，UA shadow tree 加 `RenderTheme`。
- **Servo**：原本 layout 直接把值當成文字畫（沒有 shadow tree），後來改成文字框的 UA shadow tree。

## 已決定的事項

2026-10-16 確認（第 1 段開始前；選項的比較在 `details.md` 的「選項的比較」）：

1. **範圍是常見的控制項全部**（F1）：「目標」列的文字類 `<input>`、`checkbox`、`radio`、`range`、`button` / `submit` / `reset`、`<button>`、`<textarea>`、`<select>`，分三段，Speedometer 用到的在前兩段。UA stylesheet、system color 和 theme 一次定下來，WPT 的 `html/rendering/widgets/` 能整個跑。不選只做 Speedometer 用到的（F2）：其他控制項照舊 0×0，之後回來加時 UA stylesheet 和 theme 要再動一次。
2. **原生外觀由 paint 的 theme 畫**（N1，Blink 的 `ThemePainter`、`NativeTheme`）：`appearance` 是 `auto` 的控制項在背景之後多一個 theme 部件的 display item，用 `compositor_paint` 的路徑和矩形畫，兩個 renderer 共用。不選 UA stylesheet 用 CSS 畫（N2）：range 的把手、number 的箭頭畫不出來，`getComputedStyle` 會讀到 UA 的背景圖，`appearance: none` 要一串規則重設。不選只給大小（N3）：checkbox、radio 在畫面上是空的。
3. **做 CSS Color 4 的 system color，只有淺色的值**（S1）：所有收顏色的 property 都收，computed value 是 Chrome 淺色主題的值；UA stylesheet 照抄 Chrome 的寫法，Complex-DOM 的 Spectrum 用到的（`buttonface`、`canvastext`、`highlight`……）也接上。不選 UA stylesheet 寫死顏色值（S2）：Complex-DOM 用到的照舊丟掉，之後做 system color 時要改回來。
4. **分三段**：style 和大小（版面和 Chrome 一樣）、原生外觀和文字控制項（值、placeholder）、`<select>`。不選先做文字框（把 UA shadow tree 提到第 1 段）：TodoMVC 的 placeholder 先看得到，但版面要晚一段才對，而 UA shadow tree 是最大的一塊。

2026-10-16 確認（第 1 段開始前；選項的比較在 `details.md` 的「第 1 段的選項」）：

5. **`font` 收 system font 的關鍵字**（A1）：CSS 2 的 `caption`、`icon`、`menu`、`message-box`、`small-caption`、`status-bar` 和 Chrome 的 `-webkit-small-control` 等，computed 成 `system-ui` 和 Chrome 的大小（`-webkit-small-control` 是 13.333px）；UA stylesheet 照抄 Chrome 的 `font: -webkit-small-control`；longhand 在 computed 之前是待定的（Blink 的 `CSSPendingSystemFontValue`：宣告值的 `font` 讀回關鍵字，longhand 讀回空字串）。1a 在 Windows 11 的 Chrome 152 實測：`-webkit-*-control` 是 13.333px、`menu`、`small-caption`、`status-bar` 是 12px、其他的 16px，family 是 Arial 和系統的 UI 字型（Microsoft JhengHei UI）；kun 照這些大小，family 照這條用 `system-ui`（測試裡就是 Lato），不讀作業系統（`docs/limitations.md`）。不選 UA stylesheet 直接寫值（A2）：作者寫的 system font 照舊丟掉，UA stylesheet 和 Chrome 的寫法不同。
6. **固定的大小照 Blink 分兩處**（B1）：`appearance` 是 `auto` 的 checkbox、radio 的大小在 style 調整時寫進 `width`、`height`（`auto` 時，Blink 的 `LayoutTheme::AdjustStyle`，computed value 和 Chrome 一樣）；range、文字框、`<textarea>` 是 layout 的 intrinsic size（`LayoutSlider`、`LayoutTextControl::ComputeIntrinsicLogicalWidths`：`size` 乘字型的平均字寬，高度是 line height）。不選全部在 layout（B2）：checkbox 的 computed `width` 是 `auto`，和 Chrome 不同。
7. **system color 照每個平台的 Chrome**（C2）：Blink 在不是 web test 時從 Browser 給的 `ColorProvider` 拿按鈕、文字框、連結、選取和視窗的顏色（`LayoutTheme::SystemColorFromColorProvider`），其他的（`mark`、`selecteditem`、`accentcolor`……）和 web test 用 `LayoutTheme::DefaultSystemColor` 的固定表。`ColorProvider` 在 Windows 的淺色模式照系統色（`MapNativeColorsToCssSystemColors`：`COLOR_BTNFACE`、`COLOR_BTNTEXT`、`COLOR_GRAYTEXT`、`COLOR_HIGHLIGHT`、`COLOR_HIGHLIGHTTEXT`、`COLOR_HOTLIGHT`、`COLOR_WINDOW`、`COLOR_WINDOWTEXT`），Linux 用 Chromium 自己的預設，macOS 照 `NSColor`。Browser process 讀這些、在啟動 renderer 時交給它（Chromium 是 `ColorProvider` 的顏色表），renderer 自己不讀作業系統；headless 和測試用固定的表（Chromium 的 web test 也是）。原本以為跟著平台的只有選取的顏色和強調色，2026-10-16 在 Windows 11 的 Chrome 152 實測之後改正：`buttonface` 是 `rgb(240, 240, 240)`、`graytext` 是 `rgb(109, 109, 109)`、`linktext` 是 `rgb(0, 102, 204)`，都是系統色；強調色反而不給網頁看（`accentcolor` 一律 `#0075ff`），`selecteditem` 是固定的 `#1967d2`（`details.md` 的「system color 的值」）。不選所有平台一張固定的表（C1）：和使用者那個平台的 Chrome 不同。
8. **第 1 段分 1a、1b、1c，1c 做完再 merge**：1a style（system color、`appearance`、system font、UA stylesheet 的控制項規則、`IsControlStyled`）；1b layout（checkbox、radio、range 的大小，文字框和 `<textarea>` 的 intrinsic size、`<button>` 的匿名 flex、baseline）；1c 和 Chrome 實測對照，更新受影響的 reftest、WPT 的預期和情境、`about:demo`。`<input type=button>`、`submit`、`reset` 的寬度要看它的文字（在第 2 段的 shadow tree 裡），第 2 段才對；`<select>` 在第 3 段。

2026-10-16 確認（1a 開始前；選項的比較在 `details.md` 的「1a 的選項」，同一節最後是照 Chrome 做、不另外決定的）：

9. **system color 在算 computed value 時換成顏色**（P1）：`css` 的 `SpecifiedColor` 多一種 system color 的關鍵字（宣告值讀回關鍵字），`compute()` 從 `ComputeContext` 帶的表換成 RGB（computed value 是 `rgb()`，和 Chrome 一樣）；表由 `StyleEngine` 照 renderer 啟動時拿到的值建。和 Blink 的 `StyleColor` 同構，之後做深色、forced colors 只要換表。不選解析時換（P2）：解析和 renderer 的狀態綁在一起，之後做深色時要改成 P1。
10. **1a 讀 Windows 的設定，macOS 之後在 Mac 上補，Linux 用固定的值**（R2）：Browser 那一側從 Windows 的 registry 讀第 7 條的八個系統色（`HKEY_CURRENT_USER\Control Panel\Colors` 的 `ButtonFace`、`ButtonText`、`GrayText`、`Hilight`、`HilightText`、`HotTrackingColor`、`Window`、`WindowText`，和 `GetSysColor` 的值一樣；用 safe 的 crate，不用 `unsafe`），放進啟動 renderer 時的 `RendererFlags`；只在啟動時讀一次（Chromium 在設定改了時送新的 `RendererPreferences`，kun 先不做，新的分頁才看得到）。讀的每個值都在這台開發機上和 Chrome 實測對照。不選連 macOS 一起做（R1）：這台開發機上不能和 Chrome 對照。不選只接好路（R3）：第 7 條的平台差別要之後才有。

2026-10-16 確認（1b 開始前；選項的比較在 `details.md` 的「1b 的選項」，同一節有照 Blink 或 Chrome 的實測做、不另外決定的）：

11. **文字控制項是有自己 intrinsic size 的 block container**（L1，Blink 的 `LayoutTextControl` 是 `LayoutBlockFlow`）：inline 方向的 intrinsic size 由 `size`（`cols`）和平均字寬給、不看內容，`height: auto` 時照內容，沒有內容時是一行的 line height；第 2 段的 shadow tree 照常排在裡面。1b 實測：在 block 裡照 replaced 元素的方式算大小（`width: auto` 是它自己的寬度，不撐滿），在 flex 和 grid 裡照一般的盒子 stretch；它的 `display: inline` 是 atomic inline。1b 原本照 replaced 元素調整 `<input>`、`<select>`、`<textarea>` 的 table 類 `display`（`table` 是 `block`，其他是 `inline`），1c 和 WPT 的 `button-layout/computed-style.html`、Chrome 152 實測對照之後改正：Chrome 的調整是 `LayoutTheme::AdjustStyle` 的，對象是 `appearance` 不是 `none` 的元素（不只控制項，`<span style="appearance: auto">` 也是），`inline`、`inline-table` 和 table 內部的值算成 `inline-block`，`table`、`list-item` 算成 `block`，computed value 就是調整過的；`appearance: none` 的照它自己的 `display`。不選 1b 先當成 leaf（L2）：第 2 段要改成 container，大小和 baseline 的程式要重寫。
12. **`<button>` 照 HTML 的 button layout**（B1，Blink 的 `LayoutButton`）：`display` 是 `inline-block`、`block` 時，它的內容（行，或 in-flow 的子盒子，當成一塊）在 content box 裡垂直置中，放不下時在頂端（Chrome 實測）；`flex`、`grid` 時照那個 `display` 排。原本打算照 HTML 的寫法在 box tree 建一個匿名的 block（anonymous button content box），1b 實作時改成排完之後把內容往下移（`layout` 的 `center_button_content`，和 table cell 的 `vertical-align` 同一條路 `shift_content`）：幾何一樣，box tree 的重建和 restyle 不必多認一種匿名的盒子。1c 和 WPT 的 `button-layout/`、Chrome 152 實測對照之後補上 button layout 的另外兩件事，對象是 `<button>` 和 `<input type=button|submit|reset>`：在 block 裡 `width: auto` 是 fit-content（不論它自己的 `display`；flex、grid 裡照樣 stretch，絕對定位的照一般的盒子；Blink 的 `LayoutBox::AutoWidthShouldFitContent`），它也是 flow root（避開旁邊的 float、包住自己的 float）。不選照舊（B2）：有高度的按鈕內容靠上，和 Chrome 不同。

2026-10-16 確認（1c 開始前）：

13. **1c 把相關的 WPT 整個目錄加進清單**（W1）：`html/rendering/widgets/` 整個、`css/css-ui/` 的 `appearance`、`css/css-color/` 的 system color、`css/css-fonts/parsing/` 的 system font；失敗的 `.ini` 寫明原因，之後的段落做完時更新（RFC 0048 的做法）。大小和排版的測試現在就守著。不選只加解析和 computed value 的（W2）：畫面的測試等第 2 段才加，1b 的大小沒有 WPT 守。

2026-10-16 確認（第 2 段開始前；選項的比較在 `details.md` 的「第 2 段的選項」）：

14. **文字控制項的內部是 UA shadow tree**（T1，Blink 的 `TextControlElement`）：文字類的 `<input>` 和 `<textarea>` 有一棵 script 拿不到的 shadow tree，裡面是 inner editor（值的文字）和 placeholder 的元素，`::placeholder`、`::-webkit-*` 指到裡面的元素；`dom` 現有的 shadow root、flat tree 和 tree scope 的 style 照常用，多一種 UA 的 mode。之後的 `<input>` 按鈕的文字、search 和 number 的部件、第 3 段的 `<select>` 用同一個機制，caret 和選取對到那個文字節點。每個文字框多 2 到 3 個節點（Chrome 也是）。不選 layout 的匿名盒子（T2）：最便宜，但每個 `::-webkit-*` 部件和 `<select>` 都要另外做，之後的編輯也要另外對應。
15. **原生外觀照每個平台的 Chrome**（A2）：Windows 和 Linux 是 `NativeThemeBase` 的那一套（2020 年的 controls refresh 之後兩者共用），在這台 Windows 上和 Chrome 截圖對照；macOS 是 `NativeThemeMac` 的那一套，在 Mac 上量過、對照之後補（和第 10 條的 system color 同樣的做法），補之前 macOS 也畫 Windows 和 Linux 的那一套（`docs/limitations.md`）。不選所有平台一套（A1）：做起來一次，但 macOS 上和那裡的 Chrome 不同。
16. **第 2 段分 2a、2b、2c，照這個順序**：2a 文字控制項的 shadow tree、值、placeholder（Speedometer 的 TodoMVC 的 `.new-todo` 用到）；2b `<input>` 按鈕的文字和 search、number 的部件；2c paint 的 theme。不選先做 theme：比較獨立、比較小，但 Speedometer 用到的要晚一段。

2026-10-16 確認（2a 開始前；選項的比較在 `details.md` 的「2a 的選項」，同一節最後是照 Blink 做、不另外決定的）：

17. **shadow tree 照 Blink 延後建**（E2，Blink 的 `Document::ScheduleShadowTreeCreation`、`HTMLInputElement::EnsureShadowSubtree`）：文字控制項插進有 browsing context 的 document 時排定，下一次算 style 之前建好；要用到它的入口先建好排定的。沒插進 document 的不建，`type` 改了時拆掉舊的、照新的 type 重新排定。不選建立元素和 `type` 改了時就建（E1）：入口不用先處理排定的，但沒插進 document 的也有這棵樹，和 Blink 不同。

2026-10-16 確認（2b 開始前；選項的比較在 `details.md` 的「2b 的選項」）：

18. **按鈕類 `<input>` 的預設文字固定是英文**（L1）：`submit` 的 `Submit`、`reset` 的 `Reset`，和 Chrome 的 `en-US`、WPT 一致（Chrome 照語系翻譯，Blink 的 `QueryLocalizedString`）。不選照瀏覽器的語系（L2）：kun 沒有語系的機制，Browser 要把語系交給 renderer，另外做。
19. **search 的清除鈕和 number 的上下鈕在 2b 只做繪製**（B1）：照 Blink 的 shadow tree 有它們的盒子和大小，照 Chrome 的 UA stylesheet 在有焦點或 hover 時才顯示（原生的外觀在 2c 畫）；點了不做事（`docs/limitations.md`），等之後的編輯。不選連點擊一起做（B2，Blink 的 `SearchFieldCancelButtonElement`、`SpinButtonElement`）：清空的事件、按住時的連續加減和焦點的互動，2b 會大很多。

2026-10-16 確認（2c 開始前；選項的比較在 `details.md` 的「2c 的選項」）：

20. **theme 的部件在 paint 時展開成現有的繪圖 op**（P1，Blink 的 `ThemePainter` 在 `PaintCanvas` 上畫）：paint 的 `theme` 模組照部件和狀態記成圓角矩形、填色、路徑、線，raster、合成和兩個 renderer 都不用改，測試直接看 display list。不選新的 display item（P2）：display list 比較小，但 raster、Viz 和跨 process 的格式都要多一種，和 Chromium 不同。
21. **部件照 Chrome 有互動狀態**（S1，`NativeTheme` 的 `kHovered`、`kPressed`）：按鈕、checkbox、radio、range 的把手在 hover、按下時換顏色；有原生外觀的元素在 hover、active 改變時重畫，就算沒有 CSS 規則用到 `:hover`。不選只有靜態的狀態（S2）：不用加這個重畫，但滑鼠沒有回饋，和 Chrome 不同。

22. **部件的狀態照 Blink 跟著 fragment**（C2，Blink 的 `LayoutTheme::ControlStatesForNode`、`ControlStateChanged`）：layout 建有原生外觀的盒子的 fragment 時從 DOM 讀 theme 用到的狀態（勾選、未定、停用、唯讀、hover、按下、焦點；range 的比例），`dom` 在這些狀態改變時標記那個元素要重做 fragment（新的 layout flag），layout 只重做 fragment、不重排，paint 照 fragment 重畫。`ComputedStyle` 照舊只有 CSS 的值。不選放在 `ComputedStyle`（C1）：程式最少（狀態改了只複製上一份 style、換掉狀態，走現有的 restyle），但 `ComputedStyle` 多一個不是 CSS 的欄位，之後容易被誤用，和 Blink 不同。
23. **range 照 Blink 用 UA shadow tree**（R1，Blink 的 `SliderContainerElement`、`SliderTrackElement`、`SliderThumbElement`）：`::-webkit-slider-container`、`::-webkit-slider-runnable-track`、`::-webkit-slider-thumb` 的元素；`dom_api` 照淨化過的值算出比例（Blink 的 `RatioValue`），記在把手上；layout 排軌道時把把手放在比例 × (軌道寬 − 把手寬)（Blink 的 `LayoutSliderTrack`），作者改了把手的大小也對。作者常寫的 `::-webkit-slider-thumb { appearance: none; ... }` 有效。不選 paint 直接照比例畫（R2）：小，但作者的 `::-webkit-slider-*` 沒有效果，layout 還要重寫一份 range 的值的演算法。

2026-10-16 確認（第 3 段開始前；選項的比較和 Chrome 的實測在 `details.md` 的「第 3 段的選項」）：

24. **收起來的選單照 Blink 用 UA shadow tree**（M1，Blink 的 `MenuListSelectType::CreateShadowSubtree`、`MenuListInnerElement`）：`dom_api` 給 menulist 建 inner element（子節點是選到的 option 的文字）和不排的 slot，選的、option 的文字改了時同步；layout 照 Blink 把 menulist 當成 flex container（不管 `display`），inner element 是有 theme 的 margin 的 flex item。大小照 `MenuListIntrinsicInlineSize`：最寬的 option 的文字（select 的字型）取整，加上 4 和 16。3a 實作時改成不換 `display`：inner element 是一個 block，控制項的分支照它自己的大小排它，`align-items: center` 時用安全的置中（按鈕的 `center_content`）放在中間，和 Blink 的 flex 加上 `auto` 的 margin 幾何一樣。實測時第一次量的是替代字型（頁面沒等 `@font-face` 載入），layout 的測試對不上才發現、重量（`details.md`）。不選 layout 直接拿選到的 option 的文字（M2）：節點少，但 layout 要讀 DOM 的選取，`text-overflow`、字型、方向要另外處理，和 Chromium 不同。
25. **listbox 照 Chrome 留 15px 的捲軸寬度**（S1）：寬度和 Chrome、WPT 一致；kun 沒有捲軸，那 15px 是空的（`docs/limitations.md`）。不選不留（S2）：和 kun 其他的捲動容器一樣，但比 Chrome 窄，之後做捲軸時要改。
26. **listbox 只做繪製**（B1）：option 照選取的狀態畫（有焦點時 `SelectedItem`，沒有時灰底），滾輪照常捲；點 option 不改選取（`docs/limitations.md`），等之後的表單行為。不選連點選一起做（B2）：點選、`multiple` 的 Ctrl 和 Shift、`input` 和 `change` 是行為，事件和焦點的規則都要做、要測，這段會大很多。
27. **第 3 段分 3a、3b**：3a 收起來的選單（shadow tree、大小、外框和箭頭），3b listbox（版面、選取的顏色、`<optgroup>`、`<hr>`），各自和 Chrome 對照、跑 WPT。不選一段做完：少一輪對照，但一段很大。

## 待決定

沒有。

## 之後的可能

- 打字、caret 和選取（路線圖的 P）。
- `<select>` 的下拉選單和 listbox 的點選、date、color、file 的選擇器，清除鈕和上下鈕的點擊。
- macOS 的那一套外觀和 system color（第 10、15 條）。
- `color-scheme: dark` 和深色的 system color、forced colors、`accent-color`。
- `<meter>`、`<progress>`、`field-sizing`、`appearance: base-select`。
- 捲軸（listbox 和 `<textarea>` 留的 15px）。
