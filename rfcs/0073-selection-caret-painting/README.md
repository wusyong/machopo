# RFC 0073：畫出選取和游標：`::selection`、`caret-color`

- 開始日期：2026-10-22
- 狀態：✅ 已完成：2026-10-22
- 進度和驗收：`progress.md`
- 細節：`details.md`（Chrome 152 的實測、kun 現在的樣子、Speedometer 的用法、Blink 的對照）

## 摘要

kun 有 `Selection` API 和文字欄位的選取範圍，但畫面上沒有選取的反白，也沒有游標。這份照 Blink 加上這兩樣的繪製：選取的範圍照每個 text fragment 畫底色和文字的顏色（Blink 的 `LayoutSelection`、`HighlightPainter`），樣式來自 `::selection`，照 Chrome 152 的 highlight inheritance（元素的 `::selection` 繼承 parent 的）；focus 在可編輯的地方、選取收合時畫游標（Blink 的 `FrameCaret`），顏色是 `caret-color`，照 Chrome 閃爍，只重畫游標。

## 動機

- **畫面上沒有**（`docs/limitations.md`）：script 設的選取（`getSelection().addRange`）、文字欄位的 `select()`、`setSelectionRange`，都沒有反白；focus 的 `<input>`、`<textarea>` 和 contenteditable 沒有游標。使用者看不到自己在哪裡打字。
- **`::selection` 讓整條規則無效**：parse 時不認得它，`.a, .b::selection { … }` 連 `.a` 都丟掉；`getComputedStyle(el, '::selection')` 是空的 declaration。`caret-color` 不解析。
- **Speedometer**（盤點，`details.md`）：Editor-CodeMirror 和 Editor-TipTap 自己畫選取（CodeMirror 也自己畫游標），用 `::selection { background-color: transparent }` 和 `caret-color: transparent` 藏起原生的；CodeMirror 的主題設 `caret-color: black`。這兩個 suite 在 Chrome 上有 Blink 的選取和游標的工作（算範圍、閃爍的重畫），kun 沒有；runner 自己的 `main.css` 也有 `::selection`。

目標：

- 非收合的選取（document 的 `Selection`，和文字欄位的選取範圍）在每個被選到的 text fragment 上畫底色和文字的顏色，顏色照 `::selection`，沒有規則時是 UA 的預設，頁面沒有 focus 時是 inactive 的預設，都和 Chrome 152 一樣。
- `::selection` 能 parse、能比對，照 highlight inheritance 算 style；只有 css-pseudo 列的 property 有作用；`getComputedStyle(el, '::selection')` 讀得到。
- 收合的選取在可編輯的地方（文字欄位的 inner editor、contenteditable）、頁面有 focus 時畫游標；`caret-color`（`auto | <color>`）照 Chrome 解析、繼承、計算、插值；游標照 Chrome 的間隔閃爍，閃爍只重畫游標。
- 選取或游標改變時只重畫改變的 fragment，不重新 layout。

不在範圍內：

- 使用者用滑鼠和鍵盤**做出**選取（拖曳、雙擊選字、Shift+方向鍵在一般文字上，`user-select` 的效果）、在 contenteditable 裡打字：RFC 0038 切出的「互動」和「編輯」，另外的 RFC。文字欄位裡已有的鍵盤選取（Shift+方向鍵）照舊，這份讓它看得到。
- 被選到的圖片和其他 replaced element 的反白（Blink 蓋一層顏色）、`::target-text`、`::spelling-error`、CSS Custom Highlight API（`::highlight()`）：同一套 highlight 的 style 和繪製之後可以接上（「之後的可能」）。
- `caret-shape`、`caret-animation`、`caret` shorthand（Chrome 152 有前兩個）：Speedometer 沒有用到。
- 拖放時的游標、IME 的組字底線。

要解決的 `docs/limitations.md` 裡的限制：

- 「沒有畫出來的游標」（contenteditable 那一條的一部分：打字的部分不在範圍內）。
- 「選取範圍：畫不出來」（文字欄位）。
- `Selection` 那一條的「選了也畫不出來」。
- `::selection` 讓整條規則無效（pseudo-element 那一條的 `::selection` 部分）。

## 設計概要

做完之後：

- `getSelection().selectAllChildren(p)` 讓 `p` 的文字有藍底白字（頁面沒有 focus 時是灰底），`input.select()` 讓欄位裡的字反白，和 Chrome 一樣。
- `.cm-line ::selection { background-color: transparent !important }` 讓 CodeMirror 的行裡沒有原生的反白；`p::selection { color: green }` 只改字的顏色，底色變成透明（Chrome：`::selection` 的規則宣告了 `color` 或 `background-color` 時，另一個不再用預設；沒有宣告顏色的規則，例如只有 `text-decoration`，兩個都照預設）。`.w::selection { color: … }` 也作用在 `.w` 裡的 `<span>` 上（highlight inheritance）。選到行尾時多畫一個空白寬的底色，highlight 的陰影和裝飾線照畫，原本的裝飾線改成 highlight 的顏色。
- focus 在 `<input>` 或 contenteditable、選取收合時，那裡有一條 1 px 寬、字的顏色的游標，每 500 ms 亮暗一次（Windows 照系統的）；`caret-color: transparent` 藏起它，`caret-color: red` 改它的顏色。
- `input.select()` 讓欄位取得 focus，Selection API 把選取放進 editing host 時讓它取得 focus，和 Chrome 一樣。
- `getComputedStyle(el, '::selection').backgroundColor` 和 `getComputedStyle(el).caretColor` 讀回 Chrome 的值。

改 kun 的人看到的是：

- `css` 的 `PseudoElement::Selection`；`computed_style` 存一個元素的 highlight style（`::selection` 的），由 style 照 parent 的 highlight style 算。
- renderer 的 `selection` 模組在 pre-paint 之後找出選到的 text node 的哪一段和游標的位置，經過 `BackgroundImages::selection`、`caret` 交給 paint；`Selection` 改變時讓改變的元素 `invalidate`，游標改變、閃爍時 `invalidate_caret`（只走進 stacking context，不重畫 box）；renderer 有閃爍的計時器。
- `layout` 給每一行記下怎麼結束（`LineFragment::end`），`BoxTree` 有 text node 和 `<br>` 到盒子的索引（`is_rendered`、`block_of`）。
- 加新的 highlight pseudo（之後的 `::target-text`）時照 `::selection` 的路走。

## 詳細設計

### 1. `::selection` 的 style

- **parse**：`css::selector::PseudoElement::Selection`，和其他 pseudo-element 一樣只能在 compound selector 的最後；後面可以接 `:window-inactive`（Chrome 152 支援，`CSS.supports('selector(::selection:window-inactive)')` 是 `true`）。`::-moz-selection` 照舊讓整條規則無效（Chrome 一樣）。
- **比對**：規則照 target 分桶（`TARGETS`）多一桶。
- **計算**（highlight inheritance，css-pseudo-4 的 §3.5，Chrome 152）：一個元素的 highlight style 的 parent 是它的 parent 的 highlight style（不是元素自己的 style）。只有 Blink 標成 `valid_for_highlight` 的 property 有作用（`color`、`background-color`、`fill`、`stroke`、`stroke-width`、`text-decoration` 的 longhand、`text-underline-offset`、`text-shadow`、custom property，第 8 條）；其他的照元素，其中 `currentcolor` 的照 highlight 的 `color`。`currentcolor` 和 `inherit` 傳下去的是關鍵字。highlight 自己的 custom property 不繼承，另外存在元素上。
- **什麼時候算**：style recalc 時，只替自己比對到 `::selection` 的元素算（`HighlightStyle`），存在 `RareInherited` 上、跟著繼承；元素和祖先都沒有比對到時是 `None`（第 2、9 條）。
- **`getComputedStyle(el, '::selection')`**：元素的 style 套上這份 highlight style。
- **預設顏色**：元素和祖先的 `::selection` 規則都沒有宣告 `color` 或 `background-color` 時（`HighlightStyle::author_colors`，Blink 的 `HasAuthorHighlightColors`），兩個都畫預設的（第 3、16 條；paired cascade 是第 2 段實測時改的，`progress.md`）。

### 2. 選取的繪製

- **範圍**：每個 frame 在 pre-paint 之後，從 DOM 讀出 document 的 `Selection` 的 range，或有 focus 的文字欄位的選取範圍（inner editor 的 text node 上的 offset，RFC 0050 的 T1），換成「哪個 text node 的哪一段」和選到的 `<br>`（renderer 的 `selection` 模組，Blink 的 `LayoutSelection::Commit` 給每個 `LayoutObject` 一個 `SelectionState`），交給 paint（`BackgroundImages::selection`，第 12 條）。從終點往回第一個有 layout 的節點是選取的結尾，在它之前的 text node 算延續（`BoxTree` 的索引，第 19 條）。
- **行尾**：layout 給每一行記下怎麼結束（`LineFragment::end`：`<br>`、保留的換行、自動換行、內容的結尾）和框的位置；選取經過行尾時在文字之後畫一個空白寬的底色（第 15、19 條，Chrome 152 實測）。
- **畫**：`Painter::text_run` 照舊畫字和裝飾線，但選取的範圍裡裁掉；之後照 Blink 的 `HighlightPainter` 畫選取：底色（line box 和字型高度的聯集），highlight 的陰影，裝飾線（原本的改成 highlight 的顏色，加上 highlight 自己的），字用 highlight 的 `color`，都裁到選取的範圍（第 14 條）。沒有 `::selection` 規則宣告顏色時用 `SystemColors` 的預設顏色，page 有 focus 時 focused frame 的用 active 的（第 16 條）。
- **失效**：`Selection` 改變時 DOM 的 `selection_version` 加一，文字欄位的選取改變時加 `dom_tree_version`（第 13 條），renderer 重算範圍、和上一次比對，變了的 text node 的 parent 元素和 `<br>` 交給 `PaintController::invalidate`；不重新 layout。

### 3. 游標

- **位置**：active 的 frame 裡，有 focus 的文字欄位（能打字、不唯讀）的收合的選取，或有 focus 的 editing host 裡收合的選取（只有改變了選取的 Selection API 才讓 host 取得 focus，第 20 條）。renderer 在 pre-paint 之後找出它在哪個 text node 的哪個 offset（`<br>` 之前是 `<br>`、空的元素是那個元素和它的第一個字型），交給 paint（第 21 條）。paint 在它的 block 的行上照 `x_range` 放，downstream：自動換行時在下一行的開頭，保留的換行之後在下一行；高度是那裡的字型的高度，在行裡置中（Chrome 152 實測）；空的元素照 `text-align`、`text-indent` 放在一行的位置。
- **畫**：寬 1 CSS px（Chrome 152 實測），顏色是 `caret-color`（`auto` 是那裡的 `color`），`transparent` 不畫。它是游標所在的 block 的一個 display item，在 box 的快取範圍之外，每次走到那個 box 都重畫（Blink 的 `CaretDisplayItemClient`，第 22 條）。
- **閃爍**：renderer 的計時器照 `RendererFlags::caret_blink_interval`（預設 500 ms，Windows 讀系統的，自動化不閃，第 23 條）切換亮暗，`PaintController::invalidate_caret` 只讓游標所在的 stacking context 被走進去；游標出現、移動之後從亮開始（Blink 的 `FrameCaret::StartBlinkCaret`）。
- **`caret-color`**：第 1 段已經做好：`css` 的 longhand，繼承，`RareInherited`；值 `auto | <color>`；`getComputedStyle` 讀回顏色；插值時 `auto` 是離散的；只影響 paint。

### 測試

- **style**：`::selection` 的 parse（和 `::-moz-selection` 讓整條無效）、比對、highlight inheritance、只有那幾個 property 有作用、`getComputedStyle(el, '::selection')`；`caret-color` 的 parse、繼承、computed value、插值、CSSOM 讀回。預期值在 Chrome 152 上跑同樣的 script（`details.md`）。
- **畫面**（`shell` 的 `selection_painting`：headless 畫出來，比對幾列像素的顏色，或游標的範圍和顏色）：反白的顏色、高度、行尾的框、裝飾線和陰影、paired cascade、文字欄位、沒有 focus 的 frame、shadow tree、之後才改的選取；游標的位置、高度、顏色和不畫的情況。預期值都是在 Chrome 152 上用同一份頁面量的（原本計畫寫成 kun 自己的 reftest，改成這樣是因為像素的預期值直接來自 Chrome，不必另寫參考頁）。
- **計數**：選取改變只重畫離開和進入的元素（`paint` 的 `paint_controller`）；閃爍只重畫游標；`layout` 的索引跟著 DOM 的改變（`update` 的每個測試都和從頭建的比對）。
- **WPT**：`css/css-pseudo/` 的選取和 highlight 的 reftest 和 testharness、`css/css-ui/` 的 `caret-*`：加進清單，失敗的照原因寫 `.ini`。
- **情境**：頁面沒有 focus 時的灰色（`selection`）、游標閃爍和跟著打字走（`caret`，`window caret-blink=`）、方向鍵和失去 focus（`caret-inactive`）。

### 分階段

1. **`::selection` 的 style 和 `caret-color` 的解析**：parse、比對、highlight inheritance、`getComputedStyle`；`caret-color` 的 property。畫面還沒變，`::selection` 不再讓規則無效。
2. **選取的繪製**：pre-paint 的範圍、text fragment 的底色和字、失效；預設和 inactive 的顏色；文字欄位。
3. **游標**：位置、繪製、`caret-color`、閃爍的計時器、只重畫游標。

## 缺點

- 有 `::selection` 規則的 document，每個元素多算一份 highlight style（Blink 一樣），style 的時間和記憶體多一些（第 2 條的取捨）。
- 游標閃爍時每 500 ms 一次 main frame（Chrome 一樣），走進游標所在的 stacking context；頁面有 focus 的可編輯元素時，idle 的 renderer 不再完全安靜。
- 文字的繪製多一條分段的路，`Painter::text_run` 變複雜。
- `select()` 和 Selection API 設定選取的方法成了 async（要同步發 focus 事件），`kun:dom` 的 WIT 升了 minor，guest 要重新編（第 20 條）。

## 理由和其他做法

- **只 parse `::selection`、`caret-color`，不畫**（像 RFC 0068 的「只解析」）：盤點的那一行會消失，但 kun 本來就沒有選取和游標可以藏，Speedometer 上兩邊做的工作照舊不同，網頁上也照舊看不到選取。不選。
- **originating element 的模型**（舊的 `::selection`：只比對元素自己，不繼承 parent 的）：Chrome 152 已經是 highlight inheritance（`details.md` 的實測：子元素拿到 parent 的 `::selection` 的顏色）。不選。
- 每一題的比較在「已決定的事項」。

## 先例

- Blink：`LayoutSelection`（pre-paint 時把 `FrameSelection` 換成每個 `LayoutObject` 的 `SelectionState` 和 fragment 的範圍，只讓改變的失效）、`HighlightPainter` 和 `HighlightStyleUtils`（highlight 的 style、預設顏色、`::selection` 沒設的 property）、`StyleResolver` 的 highlight inheritance（`HighlightData`，存在 `ComputedStyle` 上）、`FrameCaret` 和 `CaretDisplayItemClient`（游標的位置、display item、閃爍的計時器、只讓游標失效）、`LayoutTheme` 的 `ActiveSelectionBackgroundColor` 這類預設顏色。
- Gecko：`nsTextFrame::PaintTextWithSelection`；`::selection` 也改成 highlight inheritance。
- WebKit：仍是 originating element 的模型。

## 已決定的事項

2026-10-22（使用者）：

1. **範圍：document 的選取和文字欄位的選取都畫，加上游標和 `caret-color`**。不選只解析不畫：kun 本來就沒有選取和游標可以藏，盤點的那一行消失，但 Speedometer 上兩邊做的工作照舊不同，網頁上也照舊看不到。不選連滑鼠拖曳、雙擊選字一起做：那是 RFC 0038 切出的互動，另一份。
2. **highlight style 在 style recalc 時算**：document 有任何作者的 highlight 規則，就替每個元素算一份（照 parent 的 highlight style，highlight inheritance），存在元素的 style 上，`getComputedStyle` 和 paint 直接讀；沒有規則的 document 不算。Blink 的 `HighlightData` 一樣存在 `ComputedStyle` 上。不選 paint 時只替被選到的元素算：省 style 的時間，但 `getComputedStyle` 要另外算一次，失效也要另外管。（第 1 段實作時，第 9 條把「每個元素都算」改成只替比對到的元素算、其他的跟著繼承：結果一樣，算的次數少。）
3. **預設顏色照 Chrome 152 量到的**：頁面有 focus 時底 `rgb(51, 103, 209)`、字白色，沒有 focus 時底 `rgb(200, 200, 200)`、字 `rgb(50, 50, 50)`（Linux，`details.md`）；Windows 照系統的 `Highlight`、`HighlightText`（Chrome 一樣讀系統的）；macOS 的 Chrome 照系統的強調色，要在 macOS 上量。不選 kun 現有的 `SystemColors::highlight`（`#1e90ff`）：和 Chrome 畫出來的不同。
4. **只讓改變的部分失效**：選取改變時，舊的和新的範圍碰到的 text node 交給 `PaintController::invalidate`，不重新 layout（Blink 的 `LayoutSelection`）。不選整份 document 重畫：每次移動游標都重畫整頁。
5. **游標照 Chrome 閃爍**：500 ms，計時器在 renderer，只讓游標的節點失效；自動化（reftest、情境、WPT、`--automation-pipe`）時不閃爍（content_shell 跑 web test 時一樣關掉）。不選不閃爍：和 Chrome 不同，也少了 Chrome 那每 500 ms 一次的重畫。
6. **支援 `::selection:window-inactive`**：Chrome 152 有；選取的繪製本來就要分頁面有沒有 focus。不選先 parse 失敗。

2026-10-22，第 1 段開始前（使用者）：

7. **highlight style 是只有那幾個 property 的 `HighlightStyle`**：元素的 style 的 `RareInherited` 多一個 `highlight: Option<Arc<HighlightStyle>>`（實作時（2026-10-22）從原本寫的 `PseudoStyles` 改到這裡，見第 9 條）；`getComputedStyle(el, '::selection')` 用元素自己的 style 蓋上它的值，paint 直接讀。算的時候照樣走 cascade（從元素的 style 開始、套比對到的宣告），再取出那幾個欄位。`color` 存成 `StyleColor`：Chrome 的 `::selection { color: currentcolor }` 繼承的是關鍵字，在每個元素上照它自己的 `color` 算（`details.md`），kun 的 `color` 是算好的 `Color`，存不了它。不選每個元素一份完整的 `ComputedStyle`（Blink 的 `HighlightData`）：有規則的 document 每個元素多複製一份約 67 個欄位的主體，`color` 還要另外開例外；Blink 的 style 本來就能存 `currentcolor`，行為一樣，存法不同。
8. **有作用的 property 照 Blink 的 `css_properties.json5` 的 `valid_for_highlight`**，和 kun 有的取交集：`color`、`background-color`、`fill`、`stroke`、`stroke-width`、`text-decoration-line`、`-style`、`-color`、`-thickness`、`text-underline-offset`、`text-shadow` 和 custom property（`LonghandId::applies_to_highlight`，和 `applies_to_marker` 一樣）。其他的讀回元素自己的，但其中值是 `currentcolor` 的照 highlight 的 `color` 算（Chrome 152：`::selection { color: X; -webkit-text-fill-color: Y }` 的 `-webkit-text-fill-color`、`caret-color`、`outline-color` 都讀回 X）。所有有作用的 property 都從 parent 的 highlight style 繼承，包括平常不繼承的（css-pseudo-4 的 highlight inheritance：`.p::selection` 的底色 `.p` 裡的元素也拿到）；沒有 parent 的時候，`background-color` 是透明，`color` 是 `currentcolor`（元素自己的顏色），其他是初始值。實作時（2026-10-22）照 Blink 的清單和 Chrome 的實測更正：原本列了 `-webkit-text-fill-color`、`-webkit-text-stroke-*`，Chrome 不收（`details.md`）。
9. **只替自己比對到 `::selection` 的元素算**：highlight style 跟著 `RareInherited` 繼承，沒有自己的規則的元素共用 parent 的那份（`Arc`）；有的從 parent 的那份加上自己的規則算一份新的。`None` 是元素和祖先都沒有比對到，畫 UA 的預設，`getComputedStyle` 讀回沒有規則時的值（底透明、字是元素的顏色）。parent 的變了，子元素照一般的繼承重算。實作時（2026-10-22）從原本寫的「`RuleSet` 記有沒有規則，有的話替每個元素算，存在 `PseudoStyles`」改成這樣：Chrome 152 在元素和祖先都沒有比對到時照預設的顏色畫，document 裡別的元素有沒有規則都一樣（`details.md`），正好是繼承的 `Option`；recalc 不用另外帶 parent 的 highlight style，也不用旗標，成本只在比對到的元素上。
10. **`:window-inactive` 照頁面的 focus 比對**；只有規則裡有它時，頁面 focus 的改變才觸發 recalc。`::selection:hover` 這類照 Chrome 讓整條規則無效。實作時（2026-10-22）照 Chrome 152 的實測改了（`details.md`）：原本寫只能接在 `::selection` 後面，但 Chrome 的 `:window-inactive` 是一般的 pseudo-class，單獨用（`a:window-inactive`）、在 `:not()`、`:is()` 裡都有效，頁面沒有 focus 時比對到元素；接在 pseudo-element 後面只有 `::selection` 和 `::-webkit-` 的（`::-webkit-scrollbar`）可以，`::before:window-inactive` 無效；`::selection` 後面只接受它。所以它是 `ElementState` 以外、document 的狀態（頁面有沒有 focus，`dom_api` 的 `PageFocus` 改變時寫進 DOM），style 照它比對。
11. **`caret-color` 是 `CaretColor { Auto, Color(StyleColor) }`**，在 `RareInherited`：`getComputedStyle` 讀回算好的顏色（`auto` 和 `currentcolor` 是 `color`），`el.style` 讀回寫的形式；`auto` 的插值是離散的；layout 的 `StyleDifference` 放在 paint-only。

Chrome 152 的 `getComputedStyle(el, '::selection')` 在兄弟元素的 `::selection` 規則形狀一樣時，後一個讀到前一個的 `color`（`details.md`）：那是它的快取的問題，不照做，測試也避開這種寫法。

2026-10-22，第 2 段開始前（使用者）：

12. **paint 的一個新輸入：每個 text node 被選到的那一段**：每個 frame 在 paint 之前，從 DOM 讀出 document 的 `Selection` 和有 focus 的文字欄位的選取範圍，照 fragment 的 `source`（text node 和 UTF-16 的範圍）換成「哪個 text node 的哪一段」，和 `FrameImages` 把 DOM 的東西交給 paint 同一個模式（Blink 的 `LayoutSelection::Commit` 在 pre-paint 時給每個 `LayoutObject` 記 `SelectionState`）。不選在 layout 時寫進 fragment（像 `control_state`）：選取一改就要重新 layout，違反第 4 條。
13. **DOM 記選取的版本號**：`Selection` 和文字欄位的選取改變時加一（實作時：文字欄位的選取存在控制項的狀態裡，改變時照舊加 `dom_tree_version`；`selection_version` 只跟著 `Selection`，排 `selectionchange` 時加一）（現在改選取不增加 `dom_tree_version`，也不要求 main frame）；renderer 看到它變了就重算範圍、和上一次比對，舊的和新的範圍碰到的元素交給 `PaintController::invalidate`，再要一個 main frame。`is_invalidated` 也比對 line 裡 text run 的節點：直接放在匿名 block 裡的文字現在不能單獨失效。Blink 的 `FrameSelection` 改變時 `LayoutSelection` 只讓改變的 `LayoutObject` 重畫。
14. **照 Blink 的 `HighlightPainter` 畫**：先畫底色（高度是 line box 和字型高度的聯集，Chrome 152 實測），字畫兩次：選取以外照原本的顏色，選取以內用 clip 換成 highlight 的顏色；highlight 的 `text-shadow` 和裝飾線也照它畫。`DrawClipped` 要能包 `DrawGlyphs`（現在只收 `DrawPath`）。不選照 cluster 把 glyph 分段：glyph 和 cluster 現在沒有對照，連字跨過選取的邊界時也分不開。
15. **選到行尾時多畫一個空白寬的底色**：選取跨過換行（`<br>`、保留的 `\n`）時，照 Chrome 在行尾多畫一個空白寬的底色；`LineFragment` 記這一行是不是以換行結束，和空白的寬度。不選不畫：和 Chrome 不同。
16. **預設顏色從 `SystemColors` 交給 paint**：`highlight`、`highlight_text` 改成 Chrome 152 在 Linux 上量到的 `rgb(51, 103, 209)`、白色（第 3 條；實作時用它的 CSS `Highlight` 的值 `rgba(0, 65, 198, 0.8)`，畫在白底上就是 `rgb(51, 103, 209)`，`getComputedStyle` 讀到的也一樣），Windows 照舊讀系統的；多一組 inactive 的（`rgb(200, 200, 200)`、`rgb(50, 50, 50)`）；和頁面有沒有 focus 一起交給 paint（paint 現在拿不到 `SystemColors`）。不選寫死在 paint 裡：Windows 的系統顏色用不上。
17. **畫哪些選取**：document 的 `Selection` 不收合時畫；文字欄位的選取只在它有 focus 時畫（Chrome 一樣：沒有 focus 的 `<input>` 看不到它的選取）；`user-select: none` 的文字不畫成被選到（Blink 跳過）。不選一律畫。
18. **自動化的頁面一開始就有 focus**：headless、reftest、WPT、`--automation-pipe` 的頁面一開始有 focus，預設畫 active 的顏色（Chrome 在 wptrunner、webdriver 和 content_shell 跑 web test 時，視窗都有 focus）。不選維持沒有 focus：reftest 都畫成灰色，和 Chrome 跑 WPT 時不同。

2026-10-22，第 2 段實作中（使用者）：

19. **行尾的框照 Chrome 152 的規則畫，用 layout 的索引判斷選取有沒有延續**：實測 Chrome 不只在強制換行畫框，自動換行和內容的最後一行也畫，規則是：`<br>` 在選取裡、保留的 `\n` 在選取裡時畫；自動換行時，選取從這行的文字以前延續到它之後時畫（掛在行尾的空白不算這行的文字）；內容的最後一行，選取要延續到後面「有 layout 的」內容才畫（`Ctrl+A` 的最後一行、後面是 `display: none` 的文字時不畫，後面是圖片時畫）；框在文字之後、行內元素的 padding 之前，`<br>` 的框用 `<br>` 的字型，空行也畫。`LineFragment` 記行尾的種類和框的位置；`BoxTree` 多一個 text node 和 `<br>` 到它所在的 inline 內容的 box 的索引（`Node::GetLayoutObject` 對 `LayoutText` 的對應），renderer 從選取的終點沿著 DOM 往回找第一個有 layout 的節點，只有它之前的 text node 算延續（Blink 的 `LayoutSelection` 只走有 layout object 的節點，`kEnd` 的就是它），成本 O(選取範圍) 以內，和 Chrome 一樣。索引在建 inline 內容時加、box 釋放或內容重建時拿掉，查的時候再對一次 box 的內容，舊的項目不會答錯（實作時改成只在 debug build 檢查：拿掉的時機已經涵蓋，release 不再多查一次；`layout` 的 `update` 測試每次都和從頭建的比對）。不選走一遍 fragment tree 找最後一個：選取的終點不在文字裡時要 O(整份文件)，fragment 的順序（定位的 box、float、`order`、top layer）也和 DOM 不同；不選反向走 fragment：最壞還是 O(整份文件)，順序的問題一樣；不選只看 DOM 推測（`display: none`、圖片、`white-space` 都會錯）；不選維持第 15 條只畫強制換行（多段選取時每段結尾少一格，和 Chrome 看起來不同）。
20. **`select()` 和 Selection API 讓元素取得 focus，連同在第 2 段做**（使用者，「補上」）：WPT 的分類找到 kun 少了這兩件 Chrome 會做的事，`input.select()` 之後的選取因此畫不出來。照 Chrome 152 實測：`select()` 設好選取之後讓控制項取得 focus（任何 type 的 `<input>`，能取得 focus 的話；不捲動，選取不動；Blink 的 `TextControlElement::select`）；Selection API 設定選取的方法把選取放進 editing host 時讓它取得 focus（anchor 的 host，反向時 anchor 不在 host 裡就是 focus 的；只在 focused frame、host 能取得 focus 時；選取不動；Blink 的 `FrameSelection::SetFocusedNodeIfNeeded`，對應 `SelectionBehaviorOnFocus::kNone`）。focus 事件要同步發，這些方法因此是 `async method`，`kun:dom` 的 WIT 跟著改成 `async func`、升到 0.16.0，guest 重新編。不選記成限制：常見的 `input.select()` 畫不出選取。

2026-10-22，第 3 段開始前（使用者）：

21. **游標和選取走同一條路交給 paint**：renderer 在 pre-paint 之後算出游標在哪個 text node 的哪個 offset（空的可編輯元素是那個元素）和亮暗，經過 `BackgroundImages` 交給 paint；paint 畫游標所在的 block 的行時，照 `x_range` 算出 x、高度是字型的，畫一個游標的 display item（Blink 的 `CaretDisplayItemClient` 在 caret block 的 foreground 畫）。不選 renderer 用 layout 的幾何算出矩形、paint 最後疊一層：要從 fragment tree 從頭走一次，疊在最上層的 z-order 和 Chrome 不同。
22. **游標是自己的 display item，閃爍只重畫它**（Blink 的 `CaretDisplayItemClient` 是獨立的 client）：游標的 item 畫在它的 box 的行之後、不在 box 的快取範圍裡，每次走到那個 box 都重畫；`PaintController::invalidate_caret` 讓它所在的 stacking context 一路到 root 不整段照搬，但 box 自己的背景和文字照舊沿用。不選讓游標的 block 整個失效：每 500 ms 重畫整個 block 的 item，大的 `<textarea>`、contenteditable 比 Chrome 多做很多 paint（Speedometer 的 Editor 兩套的游標一直閃）。
23. **閃爍的間隔是 renderer 的設定，Windows 讀系統的**：`caret_blink_interval: Option<Duration>`（Chromium 的 `RendererPreferences::caret_blink_interval`，是亮或暗各多久；`None` 或 0 不閃），預設 500 ms（Chromium 讀不到系統設定時的 `kDefaultCaretBlinkInterval`）；Windows 照 Chromium 的 `GetCaretBlinkTime` 讀 `HKCU\Control Panel\Desktop\CursorBlinkRate`（`-1` 不閃），和系統色同一條路；headless、reftest、WPT、情境、`--automation-pipe` 不閃（Chromium 的 web test 模式是 0）。Linux 照沒有 GTK 的 Chrome、macOS 也先用 500 ms（Chromium 讀 GTK 的 `gtk-cursor-blink-time` / 2 和 NSUserDefaults 的 `NSTextInsertionPointBlinkPeriodOn` / `Off`、輔助使用的不閃爍的插入點），記進限制。不選一律 500 ms（Windows 讀得到卻不讀）；不選三個平台都讀（Linux 的 gsettings、`settings.ini` 和 macOS 的 `defaults` 都只能近似 Chromium 經過 GTK、AppKit 拿到的值，輔助使用的選項要 Objective-C 的 API）。

## 待決定

（無）

## 之後的可能

- 滑鼠和鍵盤做出選取（拖曳、雙擊、`user-select`），contenteditable 的打字（RFC 0038 切出的互動和編輯）。
- 同一套 highlight 的 style 和繪製接上 `::target-text`、`::spelling-error`、CSS Custom Highlight API。
- 被選到的圖片的反白；`caret-shape`、`caret-animation`。
- 游標和選取的位置照 Chrome 先調整（canonical position，例如跳過 `visibility: hidden` 的文字）；跨 shadow tree 的選取（`getComposedRanges()`）。
- editing host 空的時候也有一行（Blink 的 `HasLineIfEmpty`）：`contenteditable` 改變時要重建盒子。
- Linux（GTK 的 `gtk-cursor-blink-time`）和 macOS（NSUserDefaults、輔助使用的設定）的閃爍間隔。
