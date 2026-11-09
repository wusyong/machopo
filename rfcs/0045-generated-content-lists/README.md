# RFC 0045：生成內容和 list

- 開始日期：2026-10-14
- 狀態：✅ 已完成：2026-10-14（第 1 到 12 條；驗收結果在 `progress.md`）
- 細節：`details.md`；進度：`progress.md`

## 摘要

RFC 0012 路線圖的 S3（CSS 線）：`::before`、`::after` 和它們的 `content`，`display: list-item` 和它的 marker（`::marker`、`list-style-type`、`list-style-position`），以及 CSS counter（`counter-reset`、`counter-increment`、`counter-set`、`counter()`、`counters()`，連同 list 編號用的 `list-item` counter 和 `<ol start>`、`reversed`、`<li value>`）。pseudo-element 的 computed style 存在它的元素上，不建 DOM 節點（Blink 建 `PseudoElement`，見第 1 條）；它們的盒子是元素的盒子的一部分，在 box tree 照 Blink 的位置建（`::before` 是第一個子盒子、`::after` 是最後一個），文字是沒有 text 節點的生成文字（和 `text-overflow` 的省略號一樣）；hit test 打到它時回答它的元素。outside marker 是 list item 的一個不給 taffy 排的盒子，放在第一行的開頭之前。counter 在建 box tree 時照樹的順序算整份文件，繼承規則照 Chrome。table 系列的 `display` 在 table 做之前暫時當 block（第 9 條），讓最常見的 clearfix 包得住 float。分兩段做：先 `::before` / `::after`，再 list 和 counter。

## 動機

盤點（`rfcs/0012-speedometer/progress.md`）：Speedometer 的 suite 丟掉的選擇器裡最多的是 `::before`、`::after`（15、16 個 suite；Complex-DOM 各有 `::before` 38 條、`::after` 10 條，CodeMirror 的 `::after` 16 條，NewsSite 也有），宣告裡 `content` 16 個 suite、`list-style-type` 15 個。這些規則原本整條丟掉（選擇器解析失敗），`::before`、`::after` 的盒子沒建，畫面和 layout 的工作量都和 Chrome 不同（RFC 0012 的「量得對」）。WPT 也一直撞到：css-text 的 `overflow-wrap` 大多數、`text-overflow-016`、`024`、`025`、`floats-114`、css-variables 的幾個都因為沒有 `::before`、`::after` 失敗；很多 reference 用 list 和 counter。

解決的 `docs/limitations.md` 的限制：

- 「Pseudo-class 有限……和所有 pseudo-element」的 `::before`、`::after`、`::marker`（`::slotted()`、`::part()` 等其他 pseudo-element 不在這份）。
- `getComputedStyle` 對 pseudo-element 讀回空的 style（那一條的 pseudo-element 部分）。
- UA stylesheet 裡寫的「沒有 `list-item`：list 是 block」（`crates/style/resources/html.css` 的開頭）。

**目標**：`::before`、`::after`、`::marker` 的選擇器、cascade、computed style 照 CSS 和 Chrome；`content` 的字串、`attr()`、counter 畫得出來、排版照它的 `display`；`<ul>`、`<ol>` 有 marker 和編號；`getComputedStyle(element, '::before')` 讀得到值；WPT 的 `css/css-pseudo/`、`css/css-lists/`、`css/css-content/` 加相關的目錄進來、記下預期結果。

**不在範圍內**：`::first-line`、`::first-letter`、`::placeholder`、`::selection`、`::backdrop`、`::file-selector-button`、`::slotted()`、`::part()`、`::highlight()`；`content` 的 `url()`（圖片）、`open-quote` / `close-quote` 和 `quotes`（第 4 條）、`content` 用在一般元素上（取代元素的內容）；`list-style-image`；`@counter-style` 和預先定義以外的 counter style（第 5 條）；`::marker` 的動畫等；generated content 的選取和 `Range`（Chrome 也選不到）；`display: list-item` 以外的 `display` 雙值語法。

## 設計概要

網頁的作者看到的：

- `.icon::before { content: "\e900"; font-family: icons }` 的圖示、`.clearfix::after { content: ""; display: block; clear: both }`、`.required::after { content: " *"; color: red }` 畫出來，照它的 `display`、`position`、`float` 排。
- `<ul>` 的項目前面有圓點，`<ol>` 有 1. 2. 3.（`start`、`reversed`、`<li value>`），`list-style-type: none` 拿掉，`list-style-position: inside` 讓 marker 進到第一行；`li::marker { color: red }` 改 marker 的顏色。
- `counter-reset: section; h2::before { counter-increment: section; content: counter(section) ". " }` 編號。
- 點 `::before` 的圖示或 marker，事件的 target 是它的元素（和 Chrome 一樣）；`getComputedStyle(el, '::before').content` 讀到 `"\"x\""`。

改 kun 的人看到的：`css` 的選擇器認得 `::before`、`::after`、`::marker`；`style` 在配對元素的規則時一起算這幾個 pseudo-element 的 style，存在元素上（`NodeState::Element` 的 `pseudo_styles`）；`layout` 的 `collect` 在元素的內容前後加上生成的盒子和文字，box 和 fragment 多一個「是哪個 pseudo-element」的欄位；list item 的 outside marker 是 layout 加的盒子；counter 由 `layout` 的 `counters` 模組在建 box tree 時照樹的順序算。

## 詳細設計

### pseudo-element 的選擇器和 style

- **選擇器**：`css::selector::PseudoElement` 是 `Before`、`After`、`Marker`（`parse_pseudo_element`）。`:before`、`:after` 的單冒號寫法照 CSS 2 也收（`selectors` 把它們送到同一個地方），`:marker` 不收。pseudo-element 只能在複合選擇器的最後，後面能接什麼照 `selectors` crate（`is_before_or_after` 只對前兩個成立）。`querySelector('a::before')` 照規範不配對任何元素（不是 `SyntaxError`）。
- **RuleSet 的桶**：pseudo-element 在 `selectors` 裡是自己一個 compound，接在它的元素的 compound 之後；`.a::before` 照它的元素的 compound 進 `a` 的桶（Blink 的 `FindBestRuleSetAndAdd`），不然每個元素都要比對它。
- **style**：配對一個元素的規則時同一趟也找它的 pseudo-element 的：有 pseudo-element 的選擇器用另一個 `MatchingMode::ForStatelessPseudoElement` 的 context（拿掉 pseudo-element、其餘的在元素上配對），每條規則各自記下對元素和對每個 pseudo-element 的最高 specificity（`RuleCollector::matching` 的 `found` 多一個「給誰」）。元素的 style 和以前一樣；每個 pseudo-element 用同一段 cascade（`cascade_blocks`），以元素的 style 當 parent（`is_root` 是 false，blockification 照元素是不是 flex、grid container），`attr()` 填進元素的 attribute 值。`::before`、`::after` 有規則配到就存（連同沒有 `content` 的，`getComputedStyle` 讀它），`content` 的 `normal` 算成 `none`，只有 `content` 是項目的產生盒子（Blink 的 `PseudoElement::IsNeeded`）。`::marker` 見「list item 和 marker」。存在元素上（第 1 條：`NodeState::Element` 的 `pseudo_styles`，`dom` 的 `set_pseudo_styles` 回答有沒有變），變了就標 `LayoutTreeFlags::pseudo_styles_changed`，layout 重新收集它所在的內容（不分只改畫法的和要重排的：pseudo-element 少，照樣重收比較簡單）。`display: none` 的元素也算（`getComputedStyle` 讀得到，和 Chrome 一樣）。
- **繼承**：pseudo-element 的 style 從元素繼承；生成的文字繼承 pseudo-element 的。
- **UA stylesheet**（照 HTML 的 rendering 一節和 Chrome 的 `html.css`）：`li { display: list-item }`、`ol, ul, menu { counter-reset: list-item }`、`ol { list-style-type: decimal }`、`dir, menu, ul { list-style-type: disc }`、巢狀的 circle、square、`::marker { white-space: pre; text-transform: none }`（`unicode-bidi`、`font-variant-numeric` kun 沒有，不寫）。清單的 attribute 是 presentational hint（`style` 的 `hints.rs`，第 11 條）。
- **`getComputedStyle(element, pseudo)`**：`dom` 的 `CssComputedStyleDeclaration` 記下 `StyleTarget`（元素、kun 有的 pseudo-element、kun 沒有的）。不以冒號開頭的參數照元素自己的（CSSOM，Chrome 也是）；`::before`、`::after`、`::marker` 讀那個 pseudo-element 存著的 style，沒有存著時照 Chrome 讀 cascade 什麼都沒有的那一份（`style::pseudo_style_without_rules`：從元素繼承、blockification 照樣做、`::before`、`::after` 的 `content` 是 `none`），不是空的。kun 沒有的 pseudo-element 和 `:marker` 這種只有 CSS 2 的四個才收的單冒號寫法回空的 declaration；以冒號開頭卻解析不了的是 `SyntaxError`（Chrome 回空的，記成限制）。layout 相關的 property 讀 computed value，不找它的盒子（Chrome 照盒子算 used value；記成限制）。

### table 系列的 `display`（第 9 條）

- `css` 的 `display` 多 `table`、`inline-table`、`table-row-group`、`table-header-group`、`table-footer-group`、`table-row`、`table-cell`、`table-column-group`、`table-column`、`table-caption`；`computed_style` 的 `Display` 照樣記下它們（`getComputedStyle` 讀回原本的值）。
- layout：`inline-table` 是 atomic inline，和 `inline-block` 一樣；`table-column`、`table-column-group` 不產生盒子（CSS 2 § 17.2：不 render，Chrome 在 table 外面也什麼都不畫；`Display::generates_no_box`）；其他的和 `block` 一樣（`Display::is_block_level` 等照這個對應，taffy 拿到 `Block`）。table 的匿名盒子不建。HTML 的 `<table>` 不受影響（UA stylesheet 沒有給它 table 的 `display`）。
- 記成限制（「沒有 table」那一條），S7 拿掉。

### `content`

- `normal | none | [ <string> | attr(<name>) | counter() | counters() ]+`（`url()`、quote、`/` 之後的替代文字不收，第 4 條）；computed value 照 Chrome 序列化（`getComputedStyle` 讀到 `"x" attr(title) counter(a, upper-roman)` 這種）。`attr()` 的值在 cascade 時填進 computed value（`ContentItem::Attr` 的 `value`；序列化照樣是函式），HTML 元素的名稱不分大小寫；改屬性本來就讓元素重算 style（`dom` 的 `set_attribute` 標 `needs_style_recalc`），值變了 style 就變了，layout 跟著走，不用另外記依賴哪個屬性。counter 的值由 layout 算（「counter」一節）。
- `counter(name, style)`、`counters(name, separator, style)`：`decimal` 以外的 style 才寫進序列化；`<counter-style-name>` 不含 `none`。
- 只用在 `::before`、`::after`、`::marker`；一般元素的 `content` 沒有作用（Chrome 對一般元素的 `content: url()` 會取代內容，不在範圍內）。

### layout：生成的盒子

- **位置**（Blink 的 `LayoutTreeBuilder`）：`::before` 是元素的第一個子盒子、`::after` 是最後一個；它的 `display` 決定它是行內（預設）、block、flex、atomic inline……，`position`、`float` 照常。它的子盒子是 `content` 產生的文字（一個沒有 text 節點的 run，第 2 條）。layout 的 `collect` 收一個元素的內容時，前後各加它的 `::before`、`::after`（`collect_pseudo`）：`display: inline` 的是一對 `InlineItem::Open`、`Close` 夾著文字；其他的建一個 `LayoutBox`（`node` 是元素、`pseudo` 是哪一個），照它的 `display`、`position`、`float` 成為 block 子盒子、atomic inline、out-of-flow 盒子或 float。這些盒子不在 `BoxTree::elements` 裡，每次收集內容都重建（舊的跟著被收回）。元素不產生盒子時（`display: contents` kun 沒有、`display: none`、table 的 column）沒有；replaced element 本來就不收集內容；`<input>`、`<textarea>`、`<select>` 照 Blink 的 `CanHaveGeneratedChildren` 不產生。
- **識別**：`LayoutBox`、`BoxFragment`、`InlineBoxFragment`、`TextFragment` 多一個 `pseudo: Option<PseudoId>`，`node` 是元素（第 3 條）；行內內容裡用 `text::Owner`（元素加上 `pseudo`）分辨元素自己的和它的 pseudo-element 的 inline box 和 run。`element_box`、`client_rects`、`is_fixed`、paint 的 `element_bounds`、`paint_offset` 和上一次 paint 的 `by_node` 這類找元素自己的盒子的地方跳過 `pseudo` 是 `Some` 的；hit test 照舊回答 `node`，也就是元素（Blink 的事件 target 是 pseudo-element 的元素）。生成的文字沒有 `source`（不在 `Range` 的幾何、選取裡，Chrome 也是）。
- **局部更新**：pseudo-element 的 style 變了（`pseudo_styles_changed`），`walk_element` 收集它所在的內容：元素自己的盒子的（block、atomic inline），或行內元素所在的容器的；收集時重建生成的盒子。

### list item 和 marker

- **`display: list-item`**：block 的盒子加一個 marker（Blink 的 `LayoutListItem`）。`display` 的值多 `list-item`（和 `inline list-item` 等雙值語法不收），blockify 不變，taffy 拿到 `Block`。
- **`::marker` 的 style**：只替 list item 算（UA stylesheet 的 `::marker` 規則配到每個元素，非 list item 的丟掉）。只套用 css-pseudo 4 列給 marker 的 property（`LonghandId::applies_to_marker`：字型、`color`、`content`、`white-space`、文字的 property；Blink 的 `kValidForMarker`），其他的宣告不算，list 的 property 從 list item 繼承。`content: normal` 留著（表示照 `list-style-type`）。`display` 照位置：outside 是 `inline-block`、inside 是 `inline`；`text-indent` 是 0（Blink 的 `StyleAdjuster::AdjustStyleForMarker`）。
- **marker 的內容**：`::marker` 的 `content` 是 `normal` 時照 `list-style-type` 產生（`disc`、`circle`、`square` 是 `•`、`◦`、`▪` 加一個空格；數字的是 counter style 的字串加 `. `，照 css-counter-styles 的預先定義的 style，第 5 條；字串照原樣）；`list-style-type: none` 或 `::marker { content: none }` 時沒有；`::marker { content: "→ " counter(list-item) }` 照它的項目。文字由 counter 的計算一起產生（下一節）。
- **`list-style-position`**：`inside` 的 marker 是 list item 的第一行最前面的行內盒子（`collect` 在 `::before` 之前放一對 `Open`、`Close` 夾著文字）。`outside`（預設，第 6、12 條）是 list item 的 `LayoutBox::marker`：一個不在 `children` 裡、taffy 不排的盒子，`lay_out_box` 照 atomic inline 的方式排（max-content），`fragment` 把它放在第一行的開頭之前（`first_line`：list item 自己的，或第一個有行的 in-flow 後代的；照 float 移開的行的開頭，不照 `text-align` 的位移）、baseline 對齊那一行，沒有行時在 content box 的左上；不占行的寬度。它是 list item 的 fragment 的最後一個 child（不移動行引用的 atomic inline 的 index），list item 有自己的行時也登記成第一行的第一個 atomic inline（paint 和 hit test 只透過行走有行的盒子的 children）。沒有內容的 list item 至少和 marker 一樣高（Blink 的 `ListMarker::AddToBoxWithoutLineBoxes`）。
- **`list-style`** shorthand（`position`、`image`、`type`；`list-style-image` 只收 `none`），序列化照 Chrome 只寫不是初始值的。

### counter

- `counter-reset`（連同 `reversed(<name>)`）、`counter-increment`、`counter-set`（`<name> <integer>?` 的清單，`none`；值照 computed value 寫出預設值），不繼承；`list-item` counter：`display: list-item` 的元素沒有自己提到 `list-item` 時自動加 1（最內層的 `list-item` 是 reversed 時減 1）。HTML 的部分照 rendering 一節（第 11 條）：UA stylesheet 的 `ol, ul, menu { counter-reset: list-item }`，`<ol start>`、`reversed` 和 `<li value>` 是 presentational hint（`counter-reset: list-item start-1`、`reversed(list-item) start+1`、`counter-set: list-item value`），`type` 也是（`<ol>`、`<li>` 的數字分大小寫，`<ul>`、`<li>` 的符號不分）。
- **計算**（第 7 條，`layout` 的 `counters` 模組）：照樹的順序走有盒子的元素（`display: none` 和 replaced element 的子孫不算），每個元素（和 pseudo-element）有自己的 CSS counters set：css-lists 3 的「Inheriting Counters」照 Chrome（第 10 條）；reset、increment、set 依序套用，`::marker`、`::before` 接在元素之後，`::after` 在子元素之後。reversed 且沒有值的 counter 的起始值照 css-lists 3 的演算法（每個元素的 increment 取負號累加，到第一個 set 加上它的值為止，再加上最後一個不是 0 的 increment 取負號），在整趟走完之後補上；increment 會溢位時值不變。結果是每個 marker 和顯示 counter 的 pseudo-element 的文字（`BoxTree::counters`），`collect` 照它產生文字。
- **什麼時候重算**：建樹之前算一次；update 時，walk 之後有內容要重新收集、而文件有 counter（或要重新收集的子樹裡有）時，整份重算，和上次的文字比對，變了的把它所在的內容（元素自己的盒子，或最近的有盒子的祖先的）加進重新收集的清單（那些元素或祖先要重建的不必）。counter 的 property 不在「只影響繪製」的 style 差別裡，所以改了它的 style 一定會重新收集內容、觸發重算。Blink 的 `CountersAttachmentContext` 也是整份重算，但只在 counter 相關的改動時；kun 在文件有 counter 時任何重新收集都重算（記成限制）。

### 測試

- **`css`、`style` 的整合測試**：pseudo-element 選擇器的解析（單冒號、位置限制、`querySelector`）、cascade 和繼承、`::marker` 的 style、`content`、list 和 counter property 的解析、序列化、computed value、清單的 hint；`getComputedStyle` 讀 pseudo-element。
- **`layout` 的整合測試**（`generated`、`lists`、`update`）：`::before`、`::after` 的位置（行內、block、float、絕對定位、flex item）、沒有 text 節點的文字、`content: ""` 的空盒子、幾何不算 pseudo-element 的盒子；marker 的位置（outside、inside、沒有行的 list item、巢狀、float 旁邊）、編號（`start`、`reversed`、`value`、counter 的繼承、`counters()`、溢位）；局部更新（換 `content`、插入和移除 list item、改 attribute 和 `list-style-*`），每一步和重建的樹比對。
- **`paint`**：生成文字和 marker 的繪製、hit test 回答元素、元素的 bounds 不算 pseudo-element 的盒子。
- **kun 的 reftest**：`generated/`（`g1-before-after`、`g2-lists`）。
- **WPT**：整個 `css/CSS2/generated-content/`、`css/css-content/`、`css/css-lists/`、`css/CSS2/lists/`、`html/rendering/non-replaced-elements/lists/`，`css/css-pseudo/` 的 `::before`、`::after` 的五個和 `::marker` 的 74 個，cssom 的 `getComputedStyle-pseudo*`；每個失敗的預期結果寫了原因。清單裡原本就有、用到這些功能的測試另外跑過，變通過的拿掉預期結果。
- **mutation test**：每段一份（`mutants-1.toml`、`mutants-2.toml`）。
- **量測**：不排（Speedometer 只在使用者要求時量）；`ci.py speedometer --inventory`（確認這些規則不再被丟掉的盤點）也等使用者要求時再跑。

### 分階段

1. **`::before`、`::after`**：選擇器、pseudo-element 的 style（存法照第 1 條）、`content` 的字串和 `attr()`、生成的盒子和文字、識別（`pseudo`）、hit test、局部更新、`getComputedStyle`；table 系列的 `display` 暫時當 block（第 9 條）。
2. **list 和 counter**：分成 2a（style 這一側：`display: list-item`、`::marker`、`list-style-*`、counter 的三個 property、`counter()` / `counters()`、UA stylesheet 和 HTML 的 hint）和 2b（layout：counter 的計算、marker 的盒子、生成文字裡的 counter），一起審核。

## 缺點

- 每個元素多算 pseudo-element 的 style：只有配到 pseudo-element 的規則（和 list item 的 `::marker`）才算，有的頁面（Complex-DOM 的 38 條 `::before`）多一些 style 的工作，這本來就是 Chrome 也做的事。
- box tree 多一種沒有 DOM 節點、由元素產生的盒子，局部更新和幾何的函式都要記得 `pseudo` 欄位；outside marker 是一個不在 `children` 裡的盒子，收回和排版要另外處理它。
- counter 整份重算：文件有 counter 時，每次有內容要重新收集都走一遍整份文件（Blink 只在 counter 相關的改動時）。

## 理由和其他做法

完整的比較在 `details.md`。

- **pseudo-element 的 style 存在元素上（P1）對建 DOM 節點（P2，Blink 的 `PseudoElement`）**：kun 的 layout 和 hit test 都以元素的 `NodeId` 找盒子，pseudo-element 不需要成為 DOM 的節點；Blink 的 `PseudoElement` 是 `Element` 的子類別、不在樹裡，主要給 layout object 一個 node、給 inspector 和 `getComputedStyle`，kun 用 `(元素, PseudoId)` 就夠。P2 要在 `dom` 建 GC 的物件、管它的生命週期，得到的是之後 `pseudoTarget`（UI Events）這類 API 的地方。
- **生成文字是沒有 text 節點的 run（G1）對建隱藏的 text 節點（G2）**：省略號已經是這樣；G2 會讓 `Range`、`childNodes` 看得到不該有的節點。
- **counter 在建 box tree 時算（C1）對在 style 時算（C2）**：counter 的值依賴樹的順序和 `display: none`（不產生盒子的不算），layout 建盒子的那一趟剛好照樹的順序走過有盒子的元素。
- **踩過的坑**：counter 第一版照 CSS 2 的 scope 做，`counter-001.html` 這類新的測試失敗，查 wpt.fyi 之後改成 Chrome 的繼承（第 10 條）；reversed 的起始值第一版從 reference 推的式子，碰到多個 `value`、0 的 increment 就不對，改照規範的演算法；outside marker 第一版是 list item 的 fragment 的一般 child，有自己的行的盒子只透過行走 children，marker 沒畫也打不到（第 12 條）。
- **不做**：Speedometer 的十幾個 suite 的畫面和 layout 一直和 Chrome 不同，WPT 的很多 reference 跑不出來。

## 先例

- **Blink**：`PseudoElement`（`Element` 的子類別，元素的 `ElementRareData` 擁有它；`content` 的 `ContentData` 產生 `LayoutText` 或 `LayoutQuote`、`LayoutCounter`）；style 由 `StyleResolver::ResolveStyle` 以 `StyleRequest(kPseudoIdBefore)` 算、快取在元素的 `ComputedStyle`；`LayoutTreeBuilderForElement` 在元素的子 layout object 前後插入；hit test 打到 pseudo-element 時事件的 target 是元素。list：`LayoutListItem`、`ListMarker`（`LayoutOutsideListMarker`、`LayoutInsideListMarker`），`ListItemOrdinal` 算 `<ol>` 的編號，`CountersAttachmentContext` 算 counter。照做的：盒子的位置、生成文字、事件給元素、marker 的兩種位置、整份重算 counter、counter 的繼承。不照做的：不建 `PseudoElement` 節點（第 1 條）；編號不另外用 `ListItemOrdinal`，list 的 attribute 是 presentational hint（第 11 條）；disc 等符號畫成字，不照 `ListMarkerPainter::PaintSymbol` 畫圖形。
- **Gecko**：`nsCSSFrameConstructor::CreateGeneratedContent` 建匿名的 content 節點；counter 和 quote 由 `nsGenConList` 在 frame 建構時維護；counter 照 CSS 2 的 scope（WPT 的 `counter-001.html` 失敗）。
- **Servo / Stylo**：pseudo-element 的 style 存在元素的 `ElementData` 裡（`EagerPseudoStyles`），和 kun 的 P1 一樣；`selectors` crate 的 pseudo-element 配對就是給它用的。

## 已決定的事項

2026-10-14 確認：

1. **pseudo-element 的 style 存在元素上，不建 DOM 節點**（P1；原本的「待決定」1，逐條審查）：`NodeState::Element` 多一個 `Option<Box<…>>`，`::before`、`::after`、`::marker` 各一個 `Option<Arc<ComputedStyle>>`（Stylo 的 `EagerPseudoStyles`），和元素的 style 一起算、比較、丟；layout 和 hit test 以 `(元素, PseudoId)` 識別。成本：每個元素多一個指標，只有配到 pseudo-element 規則的元素才配置；style 的計算照 Blink，在配對元素的規則時發現有 pseudo-element 的規則配到才算它的 style，和 P2 一樣。不選照 Blink 建 `PseudoElement` 物件（P2）：要在 `dom` 多一種不在樹裡、跟著 style 出現和消失的 GC 節點，所有走 DOM 的程式要確認不會走到它、動到 `soundness.md`；網頁看得到的行為（事件的 target 是元素、`getComputedStyle`、`:hover`）兩者相同，Blink 的 `PseudoElement` 主要是給 layout object 一個 node 和給 DevTools。之後要 `pseudoTarget`、`Element.pseudo()` 時再補物件（「之後的可能」）。
2. **生成文字是沒有 text 節點的 run**（G1；原本的「待決定」2）：`InlineItem::Text` 的 text 節點是 `None`，和 `text-overflow` 的省略號一樣，不在 `Range` 的幾何和選取裡（Chrome 也選不到）。不選建隱藏的 text 節點（G2）：`Range`、`childNodes` 會看得到不該有的節點。
3. **盒子和 fragment 的 `node` 是元素，多一個 `pseudo` 欄位**（I1；原本的「待決定」3）：hit test、paint 的 client、省略號照舊回答元素（Blink 的事件 target 是 pseudo-element 的元素）；找元素自己的盒子的地方（`element_box`、`client_rects`、paint 的 `element_bounds`、box tree 的 `elements`）跳過 `pseudo` 是 `Some` 的。不選 `node` 是 `None`、另外記元素（I2）：hit test 和事件都要另外查元素，`None` 現在的意思（匿名 block）會混在一起。
4. **quote（`open-quote`、`quotes`）和 `content: url()` 之後再做**（原本的「待決定」4）：Speedometer 的盤點沒有看到，記成限制，列在「之後的可能」。
5. **counter style 只做預先定義的**（原本的「待決定」5）：`disc`、`circle`、`square`、`disclosure-open`、`disclosure-closed`、`decimal`、`decimal-leading-zero`、`lower-alpha` / `lower-latin`、`upper-alpha` / `upper-latin`、`lower-roman`、`upper-roman`、`lower-greek`、`none`；其他名稱照收、照寫回、畫成 `decimal`（沒定義的 style 的 fallback）；`@counter-style` 和 `symbols()` 記成限制。
6. **outside marker 照 Blink 的 `LayoutOutsideListMarker`**（M1；原本的「待決定」6）：排成一個 atomic inline 大小的盒子、不進斷行，list item 排好之後放在第一行（可能在後代的 inline formatting context 裡）的起點外面、baseline 對齊；沒有行時放在 content box 的上面，list item 至少和它一樣高。不選第一行開頭的負 margin 行內盒子（M2）：會影響斷行、`text-indent` 和 float 旁的行寬，list item 沒有行時沒有地方放。
7. **counter 在建 box tree 時算**（C1；原本的「待決定」7）：照樹的順序走有盒子的元素（`display: none` 的不算），整份重算、只重建值變了的 marker 和生成文字（Blink 的 `CountersAttachmentContext` 也是）。不選在 style 時算（C2）：style 的局部重算不走整棵樹，counter 的值也不是 computed value。
8. **分兩段**（原本的「待決定」8）：先 `::before`、`::after`，再 list 和 counter，每段做完可以先 merge。
9. **table 系列的 `display` 在 table（S7）之前暫時當 block**（T2；原本的「待決定」9，逐條審查）：`css` 接受 `table`、`inline-table`、`table-row-group`、`table-header-group`、`table-footer-group`、`table-row`、`table-cell`、`table-column-group`、`table-column`、`table-caption`，computed value 和 `getComputedStyle` 照樣是它們；layout 把 `inline-table` 當 `inline-block`、其他當 `block`。最常見的 clearfix（`::after { content: ""; display: table; clear: both }`，Bootstrap 3 等）因此包得住 float：空的 table 盒子排起來和 block 一樣。Speedometer 的原始碼幾乎不用（`display: table` 和這種 clearfix 都沒有，`table-cell`、`inline-table` 只在不屬於 default suite 的 perf.webkit.org），理由是一般的網頁。代價：不照規範的暫時對應，記成限制，S7 拿掉；`table-cell` 並排的版面從「宣告無效、照 inline 擠在一行」變成上下疊（兩者都不對）；原本寫 `display: table`、宣告被丟掉而照 inline 的元素變成 block。HTML 的 `<table>` 不受影響（UA stylesheet 沒有給它 table 的 `display`，照舊是行內）。解析的部分 S7 照用。不選照舊（T1）：生成的 `::after` 出現了，clearfix 卻照樣包不住 float。放在第 1 段（clearfix 是 `::after`）。

第 2 段實作中定下，2026-10-14 審核確認：

10. **counter 的繼承照 Chrome 的 css-lists 3「Inheriting Counters」**：元素的 counter 是父元素的，加上前一個兄弟的、父元素沒有同名的（WPT 的 `counter-001.html`、`counters-scope-001.html` 的斷言；wpt.fyi 上 Chrome 通過這些、`counter-list-item-2.html` 和 `counter-reset-reversed-siblings-003.html` 失敗）。不選 CSS 2 的 scope（reset 的 counter 延續到後面的兄弟，Firefox 的做法，和上面的測試衝突），也不選規範目前的文字（`counter source` 是前一個兄弟的整個 set：和 `counter-001.html` 衝突，Chrome 不照它）。
11. **HTML 的 list attribute 是 presentational hint**（照 HTML 的 rendering 一節）：`getComputedStyle` 讀得到 `<ol start>` 的 `counter-reset` 和 `<li value>` 的 `counter-set`（WPT 的 `lists-styles.html` 照這個比對）；list item 自動的 `list-item` increment 不寫進 `counter-increment`（`li-counter-increment-computed-style.html`）。不選照 Blink 的 `ListItemOrdinal` 另外算編號：會有兩套編號規則，`counter(list-item)` 和 marker 對不上。
12. **outside marker 是不給 taffy 的盒子，畫成第一行的第一個 atomic inline**：見「list item 和 marker」。不選把它放進行內內容（第 6 條沒選的 M2），也不選在 fragment 多一個 marker 欄位（paint、hit test、pre-paint 都要另外處理它）；它在行裡最先畫，和行的內容重疊時被蓋住（WPT 的 `outside-marker-paint-order.html` 通過；更細的重疊還沒和 Chrome 比對）。

## 待決定

無。

## 之後的可能

- `::first-line`、`::first-letter`、`::placeholder`、`::selection`。
- quote、`content: url()`、`list-style-image`、`@counter-style`、`symbols()`、其他預先定義的 counter style。
- disc、circle、square 照 Blink 的 `ListMarkerPainter::PaintSymbol` 畫成圖形（現在是字，大小和位置和 Chrome 差一點，list 的 WPT 有十個左右因此失敗）。
- counter 只在 counter 相關的改動時重算（記下哪些元素影響 counter，像 Blink 的 counters dirty），不必每次重新收集內容都走整份文件。
- `::before::marker`（`display: list-item` 的 pseudo-element 自己的 marker）。
- UI Events 的 `pseudoTarget`（要 pseudo-element 的物件，第 1 條沒選的 P2）。
