# RFC 0073 的細節

## 1. Chrome 152 的實測（2026-10-22）

Chrome for Testing 152.0.7977.82，Linux，headless，CDP 的 `Runtime.evaluate` 和 `Page.captureScreenshot`（device scale factor 1；要 focus 時開 `Emulation.setFocusEmulationEnabled`）。

### `::selection` 的 style

| 項目 | 結果 |
| --- | --- |
| `.p::selection { background-color: rgb(255,0,0); color: rgb(0,128,0) }`，`.p` 裡的 `<span>` 的 `getComputedStyle(span, '::selection')` | `rgb(255, 0, 0)`、`rgb(0, 128, 0)`：從 parent 的 `::selection` 繼承（highlight inheritance） |
| `.w::selection { color: rgb(4,4,4) }`、`.w span::selection { background-color: rgb(5,5,5) }` 的 `<span>` | 底 `rgb(5, 5, 5)`、字 `rgb(4, 4, 4)`：自己的規則加上 parent 的 |
| `.q ::selection { background: blue }` 的 `.q` 自己 | 底 `rgba(0, 0, 0, 0)`、字 `rgb(0, 0, 0)`：規則只比對到後代 |
| 沒有任何規則的元素、`<input>` | 底 `rgba(0, 0, 0, 0)`、字 `rgb(0, 0, 0)`（`getComputedStyle` 讀不到 UA 的預設顏色） |
| `::selection { background: rgb(1,2,3); font-size: 40px; text-decoration: underline; text-shadow: 1px 1px red; cursor: pointer }` | `rgb(1, 2, 3)`、`16px`、`underline`、`rgb(255, 0, 0) 1px 1px 0px`、`auto`：`font-size`、`cursor` 不作用 |
| `.s, .t::-moz-selection { color: red }` | 整條規則無效（`cssRules` 裡沒有，`.s` 的字照舊） |
| `CSS.supports('selector(…)')`：`::selection`、`::-moz-selection`、`*::selection`、`::selection:hover`、`::selection:window-inactive` | `true`、`false`、`true`、`false`、`true` |
| `#a { color: blue } #a::selection { background: red }` 的 `color` | `rgb(0, 0, 255)`：沒設的 `color` 是元素自己的顏色 |
| `#c { color: rgb(1,1,1) } #c::selection { color: currentcolor }`，`#c` 裡 `color: rgb(2,2,2)` 的 `<span>` | `rgb(1, 1, 1)`、`rgb(2, 2, 2)`：繼承的是 `currentcolor` 這個關鍵字 |
| `#e { background: rgb(9,9,9) } #e::selection { background: inherit }` | 透明：`inherit` 從 parent 的 highlight style 繼承，不是從元素自己 |
| 兄弟 `#g`（綠色）、`#h`（藍色），各自 `::selection { background: red }` | `#h` 讀回綠色（前一個的）：快取的問題，畫出來時字照 `#h` 自己的藍色（上面的截圖：只設背景時字照原本的顏色） |

### `::selection` 裡有作用的 property

Blink 的 `css_properties.json5` 標 `valid_for_highlight: true` 的（2026-10-22 的 `main`）：`color`、`background-color`、`fill`、`stroke`、`stroke-width`、`text-decoration-color`、`-line`、`-inset`、`-skip-ink`、`-skip-spaces`、`-style`、`-thickness`、`text-shadow`、`text-underline-offset`、`text-emphasis-color`，和它們的 `-internal-visited-` 版本。

Chrome 152 上 `#t::selection` 設了每個 property 之後 `getComputedStyle(t, '::selection')` 讀回的：上面的那些都是設的值；`text-underline-position`、`-webkit-text-stroke-width`、`cursor`、`opacity`、`font-weight`、`line-height`、`visibility`、`accent-color` 是元素自己的；`-webkit-text-fill-color`、`-webkit-text-stroke-color`、`caret-color`、`outline-color`、`border-top-color`、`column-rule-color` 設成 Y、`color` 設成 X 時都讀回 X（它們的值是 `currentcolor`，照 highlight 的 `color` 算）；custom property（`--x`）讀回設的值。

### `:window-inactive` 和 `::selection` 後面能接什麼

`CSS.supports('selector(…)')`、`querySelector`、和放進 `<style>` 之後 `cssRules` 的數目，三者一致：

| 選擇器 | 有效 |
| --- | --- |
| `:window-inactive`、`a:window-inactive`、`:not(:window-inactive)`、`:is(:window-inactive)` | 是 |
| `::selection:window-inactive`、`::selection:window-inactive:window-inactive`、`::selection:not(:window-inactive)` | 是 |
| `::-webkit-scrollbar:window-inactive` | 是 |
| `::before:window-inactive`、`::marker:window-inactive` | 否 |
| `::selection:focus`、`::selection:active`、`::selection:hover:window-inactive` | 否 |
| `::selection::before`、`::before::selection`、`::placeholder::selection`、`::selection .a`、`::slotted(span)::selection` | 否 |
| `a::selection`、`:host::selection`、`::part(x)::selection`、`::SELECTION`（讀回 `::selection`） | 是 |

### 選取畫出來的顏色

一個 `<div>`，`color: rgb(0,0,255)`，`selectNodeContents` 選起來，數截圖裡字附近最多的顏色：

| `::selection` 的規則 | 頁面有 focus | 頁面沒有 focus |
| --- | --- | --- |
| 沒有 | 底 `rgb(51, 103, 209)`、字白色 | 底 `rgb(200, 200, 200)`、字 `rgb(50, 50, 50)` |
| `color: rgb(0,128,0)` | 底透明（白色的頁面）、字綠色 | 同左 |
| `background-color: rgb(255,0,0)` | 底紅色、字照原本的藍色 | 同左 |
| `background-color: rgba(255,0,0,0.5)` | 底 `rgb(255, 127, 127)`（疊在白色上）、字藍色 | 同左 |
| `background-color: transparent` | 底透明、字藍色 | 同左 |
| `:window-inactive { background: green }` 和一般的 `{ background: red }` | 紅色 | （沒量） |

有作者的 `::selection` 規則時，沒設的 `background-color` 是透明，不再用預設；字沒設時照元素原本的顏色。macOS 和 Windows 的預設顏色還沒量（Chrome 讀系統的設定）。

規則只比對到別的元素（`.q ::selection` 只比對到 `.q` 的後代，或 `.other::selection`）時，`.q` 自己的字照預設的顏色畫（focus 時底 `rgb(51, 103, 209)`）。

跨行：`<div>ab<br>cd</div><p>ef</p>`，從 `a` 選到 `e`，20 px 的 monospace（一個字 12 px）：第一行和第二行的底色到 x = 35（字到 24，多一個空白寬），最後一行只到選到的字（x = 11）。

### 游標

`<div contenteditable>ab</div>`，20 px 的 monospace、30 px 的行高，`caret-color: rgb(255,0,0)`，focus、選取收合在 `a` 和 `b` 之間：

- 寬 1 px，在 x = 12；高 27 px（y = 1 到 27），不是整個 30 px 的 line box。
- 每 0.1 秒截一次圖：亮約 0.5 秒、暗約 0.5 秒，週期約 1.04 秒。

### `caret-color`

| 項目 | 結果 |
| --- | --- |
| `color: rgb(10,20,30)` 的元素和它的子元素的 `getComputedStyle(…).caretColor` | 都是 `rgb(10, 20, 30)`（`auto` 讀回 `color`） |
| `caret-color: rgb(9,9,9)` 的元素和子元素 | 都是 `rgb(9, 9, 9)`（繼承） |
| 沒設的元素、`<input>` | `rgb(0, 0, 0)` |
| `el.style.caretColor` 設 `auto`、`currentcolor`、`transparent` 讀回 | `auto`、`currentcolor`、`transparent`；computed `rgba(0, 0, 0, 0)` |
| 父元素 `caret-color: currentcolor`，改父元素的 `color` 之後子元素的 `caretColor` | 跟著父元素的新 `color`：`currentcolor` 繼承的是關鍵字，在用的元素上算 |
| `CSS.supports`：`caret-color: auto`、`caret-color: red`、`caret: red`、`caret-shape: block`、`caret-animation: manual` | `true`、`true`、`false`、`true`、`true` |

## 2. kun 現在的樣子（2026-10-22，`main` 的 `f5b43be1`）

- **沒有選取和游標的繪製**：`paint`、`layout`、`renderer` 裡沒有選取或游標的程式。字的顏色只在 `Painter::text_run`（`crates/paint/src/lib.rs`）決定（`text_fill_color`），之後畫 shadow 和裝飾線；選取的繪製會接在這裡。
- **系統顏色**：`gfx::SystemColors` 有 `highlight`（`#1e90ff`）、`highlight_text`（白色），註解說是 Chromium 的 `RendererPreferences` 的初始值；Windows 上 shell 從系統的 `Hilight`、`HilightText` 讀。沒有 inactive 的那一組。頁面裡只有 listbox 的 `<option>` 用 `SelectedItem`。
- **游標**：沒有，renderer 也沒有閃爍的計時器。kun 裡只有網址列（`crates/shell/src/omnibox.rs`）畫選取和游標，用 parley 的 `PlainEditor`、寫死的顏色、不閃爍。
- **pseudo-element**：`css::selector::PseudoElement` 有 `Before`、`After`、`Marker`、`Placeholder`、`Webkit(..)`；其他（包括 `selection`、`-moz-selection`）在 `parse_pseudo_element` 是 `UnsupportedPseudoClassOrElement`，整條規則無效。`computed_style` 的 `PseudoId` 有 `Before`、`After`、`Marker`，和元素的 style 一起存（`PseudoStyles`）；`::placeholder` 是 UA shadow tree 裡的元素。`getComputedStyle` 對 `::selection` 回傳空的 declaration（`dom_api` 的 `computed_style.rs` 的 `OtherPseudo`）。
- **選取的資料**：`Selection` API 有（RFC 0038），文字欄位有 `selectionStart`、`selectionEnd`、Shift+方向鍵（RFC 0050、0052），選取在 inner editor 的 text node 上（RFC 0050 的 T1）。頁面的 focus：renderer 收 `SetFocus`，`dom_api` 的 `PageFocus`。
- **paint 的失效**：`PaintController::invalidate` 照節點失效，`Previous::cached` 比較 box 自己以外的輸入（`docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」）。

## 3. Speedometer 的用法

- **Editor-CodeMirror**（Speedometer 的 resources 裡 `editors/dist/assets/` 的 codemirror 的 bundle）：主題 `&light .cm-content { caretColor: "black" }`、`&dark .cm-content { caretColor: "white" }`；`hideNativeSelection`：`.cm-line ::selection, .cm-line::selection { background-color: transparent !important }`，可以藏起原生的游標時再加 `.cm-line { caret-color: transparent !important }`。選取和游標都畫在它自己的 `.cm-selectionLayer`、`.cm-cursorLayer`。
- **Editor-TipTap**（同一個資料夾的 tiptap 的 bundle，ProseMirror 的 CSS）：`.ProseMirror-hideselection *::selection { background: transparent }`、`.ProseMirror-hideselection * { caret-color: transparent }`（node selection 時）；平常用原生的選取和游標。
- **runner**（Speedometer 的 resources 裡的 main.css）：`::selection { color: var(--background); background-color: var(--foreground) }`。
