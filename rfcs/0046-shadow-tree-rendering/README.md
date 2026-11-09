# RFC 0046：shadow tree 的 rendering

- 開始日期：2026-10-14
- 狀態：✅ 已完成：2026-10-15（驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`

## 摘要

RFC 0012 路線圖的 K-2（CSS 線）：shadow tree 照 flat tree 算 style、排版、畫出來。host 的子盒子換成它的 shadow tree 的內容，`<slot>` 換成分配給它的 light DOM 子節點（沒有分配的時候是它自己的 fallback 子節點）；shadow tree 有自己的 stylesheet（裡面的 `<style>`、`ShadowRoot.adoptedStyleSheets`），規則只配對同一棵樹裡的元素，加上 `:host`、`:host()` 配對 host、`::slotted()` 配對分配進來的元素，不同樹的規則照 css-scoping 的順序 cascade。`<slot>` 照 UA stylesheet 是 `display: contents`，所以順便做 `display: contents`。分兩段做：先 flat tree 的 style 和 layout（連同 shadow tree 的 stylesheet 和 `display: contents`，分三小段），再 `:host`、`::slotted()` 和跨樹的 cascade。

## 動機

RFC 0035（K-1）做完時，shadow tree 不畫：style 和 layout 照舊走 DOM 的子節點、不進 shadow tree，host 畫它的 light DOM 子節點，shadow tree 裡的元素沒有 style、沒有盒子（`docs/limitations.md` 的 shadow DOM 那一條）。Speedometer 的 TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM 整個介面都在 shadow tree 裡（Lit 的元件用 `adoptedStyleSheets` 和 `:host`），在 kun 上畫面幾乎是空的，style 和 layout 的工作量也和 Chrome 差最多：這兩個 suite 跑得完、量測有效（DOM 的摘要經過 open shadow root，和畫面無關），但「量得對」不成立（RFC 0012 的路線圖：「畫面要 K-2 才有 shadow tree 的 style」）。Perf-Dashboard 和一般用 custom element 的網頁也一樣。

要解決的 `docs/limitations.md` 的限制：

- shadow DOM 那一條的「shadow tree 不畫：style 和 layout 不進 shadow tree，host 照舊畫它的 light DOM 子節點，shadow tree 裡的元素沒有 box」。
- 「`display` 只接受……（沒有 `contents`……）」的 `contents`。

**目標**：

- flat tree：host 畫它的 shadow tree，slot 畫分配給它的節點或 fallback；分配進來的節點從 slot 繼承 style（flat tree 的 parent）。
- shadow tree 的 `<style>` 和 `adoptedStyleSheets` 只套用到那棵樹；document 的規則不進 shadow tree。
- `:host`、`:host(<compound>)`、`::slotted(<compound>)`，和 document 的規則照 css-scoping 的順序 cascade。
- `display: contents`。
- DOM 改了（shadow tree 裡、light DOM、slot 的分配）之後，局部更新 style 和 box tree。
- 幾何的 API（`getBoundingClientRect` 等）、hit test、`getComputedStyle` 對 shadow tree 裡的元素回答它的 style 和盒子。
- WPT 的 `css/css-shadow/`（`part/` 以外）、`css/css-display/` 的 `display: contents`、`shadow-dom/` 裡和 rendering 有關的加進來。

**不在範圍內**：`::part()` 和 `exportparts`、`:host-context()`（`selectors` crate 沒有；css-scoping 標成 at-risk，Firefox 沒有）；shadow tree 裡的 `<link rel=stylesheet>`；`delegatesFocus` 的 focus 委派和 shadow tree 裡的 Tab 順序（RFC 0035 的「之後的可能」）；declarative shadow DOM；form control 的 UA shadow tree（路線圖的 G 第二段）。

## 設計概要

網頁的作者看到的：

- `host.attachShadow({mode: 'open'}).innerHTML = '<style>p { color: green }</style><p>inside</p><slot></slot>'` 畫出綠色的 `inside`，接著是 host 的 light DOM 子節點；document 的 `p { color: red }` 不影響 shadow tree 裡的 `<p>`。
- `shadowRoot.adoptedStyleSheets = [sheet]`（Lit 的做法）套用到那棵 shadow tree。
- `:host { display: block; border: 1px solid }` 畫在 host 上；document 裡寫給 host 的規則蓋過它（outer 的勝出）。`::slotted(span) { font-weight: bold }` 只配對分配進來的 `<span>`，不配對它的子孫。
- `<slot name=x>` 只放 `slot=x` 的子節點；沒有東西分配時畫它自己的子節點（fallback）；`slot` 屬性、slot 的 `name` 改了，畫面跟著變。
- `display: contents` 的元素自己沒有盒子，子節點像是它的父元素的子節點一樣排版。

改 kun 的人看到的：`dom` 多一個 flat tree 的走法（`DomReadScope::flat_tree_children`：host 的子節點是 shadow root 的子節點，slot 的子節點是它分配到的或 fallback）；style 和 layout 都照它走。每個 tree scope（document、shadow root）有自己的 stylesheet 和 `RuleSet`（同樣的內容共用），只在 `dom` 標了它的 stylesheet 可能變了時重收；配對一個元素時只看它那棵樹的規則，host 和分配進來的元素另外看 shadow tree 的 `:host`、`::slotted()` 規則。

## 詳細設計

### flat tree 的走法

- **定義**（DOM 的「flat tree」，Blink 的 `FlatTreeTraversal`、`LayoutTreeBuilderTraversal`）：有 shadow root 的元素的 flat tree 子節點是 shadow root 的子節點（light DOM 子節點不直接出現）；shadow tree 裡的 `<slot>` 的是分配給它的節點（`dom` 的 `SlotAssignments`，RFC 0035 的第 6 條），沒有分配到東西時是它自己的子節點；其他節點照 DOM。沒有分配到 slot 的 light DOM 子節點不在 flat tree 裡：沒有盒子。
- **介面**（第 1 條）：`dom` 的 `DomReadScope::flat_tree_children`（iterator）、`flat_tree_parent` 和 `find_slot`（不用 handle 的 `assigned_slot`），style、layout、counter 和 `dom_api` 的 `offsetParent` 都用它們；slot 的分配照 RFC 0035 的快取（DOM 的版本變了才重算那個 shadow root 的）。原本打算放在 `NodeRef` 上，但分配的快取和 DOM 的版本在 scope 上，`NodeRef` 拿不到，要的話每個 `NodeRef` 都得多帶兩個欄位，所以放在 `DomReadScope`：style 和 layout 的走訪本來就拿著它。
- **繼承**：元素的 parent style 是 flat tree 的 parent 的：shadow root 的子節點從 host 繼承，分配進來的節點從 slot 繼承（slot 是 `display: contents`，不產生盒子，但照樣有 style、被繼承），文字的 style 是它的 flat tree parent 的。

### style

- **shadow tree 的 stylesheet**（第 3 條）：每個 shadow root 的 stylesheet 是樹裡的 `<style>`（照 tree order，不進巢狀的 shadow tree）和它的 `adoptedStyleSheets`（排在 `<style>` 之後，和 document 一樣）。同樣的 `<style>` 文字只 parse 一次、`RuleSet` 共用（Blink 的 `StyleEngine` 以文字快取 `StyleSheetContents`）：一個元件的一百個實例各有一份一樣的 `<style>`。document 的規則只配對 document tree 的元素；shadow tree 的規則只配對同一棵樹的元素。UA stylesheet 配對所有的。
- **`:host`、`:host()`、`::slotted()`**（第 2、11 條）：`css` 的選擇器解析開 `selectors` 的 `parse_host`、`parse_slotted`，`::slotted()` 後面可以接 `::before`、`::after`、`::marker`（`valid_after_slotted`）；`querySelector` 裡它們什麼都不配對。`RuleSet` 把最右邊的 compound 有 `:host` 的規則放進 `host_rules`、有 `::slotted()` 的放進 `slotted_rules`，不進一般的桶。`style::selector` 的 `StyleElement` 帶著 `DomReadScope`，實作 `containing_shadow_host`、`is_html_slot_element`、`assigned_slot`、`parent_node_is_shadow_root`。每個元素的規則來源（`RuleSource`）是：它所在的樹的規則（`MatchingContext::current_host` 是那棵樹的 host，`:host p` 這類規則才配對得到）；host 的話加上它的 shadow tree 的 `host_rules`（對那些規則，host 是 featureless 的：`featureless` 設起來，`.a :host` 不配對）；分配到 slot 的話加上它分配到的每一層 slot 所在的樹的 `slotted_rules`（slot 裡的 slot 一路往上，`selectors` 的 `assigned_slot` 照 `current_host` 找對的那一層）。host 的 style 要它的 shadow tree 的 `host_rules`，所以走訪到了 host 先收集它的 shadow tree 的 stylesheet，再算它的 style。
- **跨樹的 cascade**（css-scoping 3 的「Shadow Trees and the Cascade」，Blink 的 `EncodeTreeOrder`）：不同樹的宣告，normal 的是 shadow-including tree order 前面的樹的勝出，`!important` 的是後面的勝出；同一棵樹裡照 specificity 和順序。`CascadePriority` 在 level 之後多一個 `tree`：一個元素的樹照 shadow-including tree order 編號，它自己的樹是 0（style attribute 和 presentational hint 也是），接著是它分配到的各層 slot 的樹（由外而內），最後是它自己的 shadow tree；normal 的存成 `u32::MAX - 編號`，`!important` 的存成編號。原本照樹在 shadow tree 裡的深度排，但一個元素同時是 host、又分配到 slot 時，slot 的樹和它自己的 shadow tree 一樣深，`::slotted()` 和 `:host` 分不出先後（WPT 的 `shadow-cascade-order-001.html` 的 A4 抓到），所以改成每個元素各自編號。
- **`display: contents`**：`css` 收 `contents`；computed value 照樣（blockification 不改它：css-display 說 `contents` 不 blockify）。UA stylesheet 加 `slot { display: contents }`（HTML 的 rendering 一節）。
- **局部更新**（第 4 條）：DOM 的 style flag 現在只沿 DOM 的 parent 往上標（`mark_ancestors`），shadow root 沒有 parent，shadow tree 裡的改動到不了 document。改成往上標到 shadow root 之後接著標它的 host（shadow-including 的祖先）；style 的走訪到了有 shadow root 的元素，先走 shadow tree，再處理有 flag 的 light DOM 子節點：它們的 parent style 是它們分配到的 slot 的。slot 的分配變了（`slot` 屬性、slot 的 `name`、插入、移除，RFC 0035 發 `slotchange` 的時候）把重新分配的節點標成要重算 style 和重建盒子（`signal_slot_changes` 已經為 `slotchange` 比較了前後的分配，同一份結果拿來標記）；slot 重算 style 時，分配到它的節點跟著重算。`attachShadow` 把 host 標成要重建盒子。shadow tree 的 stylesheet 變了，那個 host 和它的整棵 shadow tree 重算；host 重算時，它的 shadow tree 整棵跟著重算（`:host()` 可能看 host 的 attribute）。看結構的選擇器和元素狀態（`:hover`）的依賴，每個元素各自合併（第 12 條）：它所在的樹的，加上（host）它的 shadow tree 的 `host_rules` 的和其他規則裡的 `:host()` 的，加上（分配到 slot 的）各層 slot 的樹的 `slotted_rules` 的（`ScopeCascade` 的 `dependencies`、`host_dependencies`、`slotted_dependencies`）。
- **stylesheet 的變動由 `dom` 標記**（第 9 條）：tree scope（document 或 shadow root）的 style flag 多一個 `style_sheets_changed`：connected 的樹裡插入、移除 `<style>`（HTML、SVG）或 `<link>`（連同在插入、移除的子樹裡、不在更深的 shadow tree 裡的），`<style>` 的子節點或文字變了，這兩種元素的 attribute 變了，`adoptedStyleSheets` 改了，都標在它所在的 tree scope 上，再照第 4 條往上標。不管樹 connected 與否都標：flag 留在 tree scope 上，直到 style 的走訪到了那裡。shadow root 搬到別的 document 時一律標（另一份 document 的 engine 可能最後看過它；`adopt` 把搬過去的節點的 flag 重設成新節點的，第一版在重設之前標，標記被蓋掉）。constructed sheet 記著 adopt 它的 tree scope（`CssStyleSheet` 的 `adopters`，`WeakSet`，不撐住它們），內容、`media`、`disabled` 改了時標記現在的 adopted 清單裡還有它的那幾個。style engine 只重收有這個 flag 或第一次碰到的 tree scope 的 sheet；document 的也一樣，不再每次 recalc 走整份 document；`<link>` 的 sheet 載入完由 `LinkedStyleSheets` 的版本告訴 engine。測試用的開關讓 engine 每次 recalc 重收全部的 tree scope，和只收有 flag 的比對結果。
- **沒有分配到 slot 的 light DOM 子節點**（第 5 條）：照樣算 style（以 DOM 的 parent 為 parent），`getComputedStyle` 讀得到，只是沒有盒子（像 `display: none` 的祖先下面的元素）。

### layout

- **box tree**：`collect` 照 flat tree 收：host 收它的 shadow tree 的內容；slot 和其他 `display: contents` 的元素不產生盒子，它的 flat tree 子節點（和它的 `::before`、`::after`）收進它的父元素的內容裡（像行內元素一樣走過去，但不加 `Open` / `Close`）；replaced element 和表單控制項的 `contents` 照 `none`（`contents_is_none`）；pseudo-element 的 `contents` 只有它的文字。`display: contents` 的元素裡的文字有它的字型：parley 的 run 原本只帶 brush、字型照最裡面的行內元素的 span，`contents` 的元素沒有 span，文字就拿了 container 的字型（WPT 的 `display-contents-line-height.html` 抓到），所以 run 的 style 不是最裡面那個開著的元素的 style 時，自己設 text property（`push_runs`，Blink 的 `LayoutText` 有自己的 style）。`walk` 的局部更新照同樣的 flag 跨過 shadow root 到 host：到了 host 先走 shadow root（它的 `children_changed` 收集 host 的內容），再走子節點，分配到 slot 的在 slot 所在的盒子裡（`container_of` 沿 flat tree 往上找有盒子的祖先）；分配變了的節點 `needs_reattach`，它們的新舊 slot 標成 `children_changed`，所在的內容重新收集。
- **幾何和 hit test**：fragment 的 `node` 照舊是元素，shadow tree 裡的元素有自己的盒子，`getBoundingClientRect`、`getClientRects`、`offset*` 照常；`display: contents` 的元素沒有盒子（Chrome 一樣：`getBoundingClientRect` 是 0）。`offsetParent` 沿 flat tree 往上找，跳過在元素看不到的 closed shadow tree 裡的（CSSOM View，Blink 的 `unclosedOffsetParent`；原本沿 DOM 的 parent，停在 shadow root）。hit test 打到 shadow tree 裡的元素，事件照 RFC 0035 的 retarget；文字的 run 的 owner 是它在 flat tree 的 parent（style 也照它），分配到 slot 的文字是 slot。`elementFromPoint` 照 DOM 的規則 retarget：`document` 的回答 document 看得到的元素（host），`ShadowRoot` 也有（CSSOM View 的 IDL 只放在 `Document`，Chrome 有，`spec-differences.txt` 記一行），回答樹裡的；打到文字時回答它的 DOM 的 parent 元素（paint 的 `HitTestResult` 多一個 `text`，renderer 換成它的 parent），所以 host 的文字子節點分配到 slot 時是 host（WPT 的 `DocumentOrShadowRoot-prototype-elementFromPoint.html` 照 Blink 的 `InnerElement` 要 host；原本回答 run 的 owner，是 slot）。輸入事件的 target 照舊是 run 的 owner。`Range` 的盒子（`Range::GetBorderAndTextQuads`）裡整個在 range 裡的 `display: contents` 元素算它的子節點的（WPT 的 `range-bounding-client-rect-with-display-contents.html`，原本算它自己的，沒有）。
- **counter、generated content**：counter 的走法照 flat tree（css-lists 3 的 counter 照 flat tree 的順序）；`::before`、`::after` 照舊在元素的內容前後。`display: contents` 的元素不改 counter（css-lists 3 的「counters without boxes」），它的 `::before`、子節點、`::after` 當成它的 parent 的子節點、在它的位置算（Blink 沿 layout tree 算，它不在裡面；第一版沒有這樣做，WPT 的 `counter-order-display-contents.html` 等四個抓到）。

### 測試

- **`dom` 的整合測試**：flat tree 的走法（host、slot 的分配和 fallback、巢狀的 shadow tree、沒有分配的 light 子節點）；flag 跨過 shadow root 到 host。
- **`style` 的整合測試**：shadow tree 的 `<style>` 和 `adoptedStyleSheets` 只套用到那棵樹、document 的不進去；繼承照 flat tree；同樣的文字共用 `RuleSet`；`:host`、`:host()`、`::slotted()`；跨樹的 cascade（normal 和 `!important`）；`display: contents` 的解析和 computed value；局部更新（shadow tree 裡的改動、`slot` 屬性、slot 的 `name`、stylesheet 的改動）。
- **`layout` 的整合測試**：host 排它的 shadow tree、slot 的內容、fallback、`display: contents`（行內和 block 的內容裡、連同 `::before`、`::after`）；局部更新每一步和重建的樹比對（`update.rs` 的 `Page::check`）。
- **`dom_api`、`paint`**：shadow tree 裡的元素的幾何、`elementFromPoint` 的 retarget、hit test。
- **kun 的 reftest**：`shadow/`（新）：`s1-flat-tree`（slot、fallback、沒分配到的、shadow tree 的 `<style>`、從 slot 繼承）、`s2-display-contents`。
- **WPT**：`css/css-shadow/` 整個（`part/` 在它的 `__dir__.ini` 停用；還在失敗的多數用 window 的 named property 或 declarative shadow DOM）、`css/css-display/` 的 `display-contents-*`、`shadow-dom/`（已經在清單裡，更新預期結果）。清單裡其他用到 shadow DOM、`<slot>`、`display: contents` 的測試另外跑，看變通過、變失敗的。
- **情境**：`about:demo` 加一個 custom element（`<kun-card>`：shadow tree 的 `<style>` 和 adopted stylesheet、兩個具名的 slot，按鈕換標題的 slot），情境 `shadow-dom` 確認標題有盒子、換了 slot 之後移動，並截圖。
- **mutation test**：每段一份（`mutants-1a.toml`、`mutants-1b.toml`、`mutants-1c.toml`、`mutants-2.toml`）。
- **量測**：不排（Speedometer 只在使用者要求時量）。

### 分階段

1. **flat tree 的 style 和 layout**（2026-10-14）：做完時 Speedometer 的兩個 suite 的元件畫得出來，但 `:host` 的規則還沒套用（host 的 `display` 等照預設）。分三小段（第 8 條）：
   - 1a **`dom` 和 style**：`dom` 的 flat tree 走法、跨 shadow root 的 flag、分配變了的標記、tree scope 的 stylesheet 變動的標記；shadow tree 的 `<style>` 和 `adoptedStyleSheets`（各自的 `RuleSet`、共用同樣的內容）；style 照 flat tree 走、局部更新；`display: contents` 的解析和 computed value、`slot` 的 UA 規則。
   - 1b **layout**：box tree 照 flat tree 收、`display: contents`、局部更新、counter 照 flat tree。
   - 1c **幾何、hit test 和驗收**：幾何、hit test、`elementFromPoint` 的 retarget（連同 `ShadowRoot.elementFromPoint`）；WPT；`about:demo` 和情境。
2. **`:host`、`:host()`、`::slotted()` 和跨樹的 cascade**（2026-10-15，不分小段，第 13 條）。

## 缺點

- style 和 layout 的走訪多一層判斷（有沒有 shadow root、是不是 slot），slot 的子節點要查分配的快取；沒有 shadow root 的網頁只多一次檢查。
- 每個 shadow root 一組 stylesheet：要收集它的 `<style>`、比對有沒有變。元件多的網頁（一百個實例）靠同樣的文字共用 parse 和 `RuleSet` 才不會慢。
- 配對 host 和分配進來的元素時多看一組規則（`:host`、`::slotted`）。

## 理由和其他做法

完整的比較在 `details.md`。

- **不做**：Speedometer 的兩個 suite 和所有用 custom element 的網頁畫面一直是空的，「量得對」不成立。
- **只做 slot 的 `display: contents`、不做一般的**：box tree 的處理是同一套（元素不產生盒子、子節點收進父元素的內容），一般的 `display: contents` 只多解析和幾個邊界（`<img>` 等 replaced element 的 `contents` 照 `none`），所以一起做（第 6 條）。

## 先例

- **Blink**：`FlatTreeTraversal`（style、選取等）和 `LayoutTreeBuilderTraversal`（建 layout tree）照 flat tree 走；`HTMLSlotElement` 的 assigned nodes 由 `SlotAssignment` 算、改了時標髒；每個 `TreeScope` 一個 `ScopedStyleResolver`，shadow tree 的 stylesheet 由 `ShadowTreeStyleSheetCollection` 收集，同樣的文字共用 `StyleSheetContents`；`ElementRuleCollector` 的 `CollectMatchingShadowHostRules`（`:host`）、`CollectMatchingSlottedRules`（`::slotted`）；`CascadePriority` 帶 tree order（outer 的 normal 勝出）。`display: contents` 的元素沒有 layout object（`LayoutTreeBuilderForElement` 跳過它、收它的子節點）。照做的：上面這些。
- **Gecko / Stylo**：Stylo 的 `selectors` crate 的 `:host`、`::slotted`、`::part` 就是給它用的；每個 shadow root 有 `AuthorStyles`（Stylo 的 `CascadeData`），同樣的 stylesheet 共用。
- **WebKit**：`ComposedTreeIterator`，`Style::Scope` 每個 shadow root 一個。

## 已決定的事項

2026-10-14 確認（選項的比較在 `details.md`）：

1. **flat tree 由 `dom` 的 iterator 走，每次照 slot 分配的快取**（F1）：`NodeRef::flat_tree_children`（實作時放在 `DomReadScope` 上，理由見「flat tree 的走法」），style、layout、counter 共用；和 Blink 的 `FlatTreeTraversal` 一樣是走法、不是資料。成本：沒有 shadow root 的節點只多一次檢查；DOM 改了之後第一次問某個 shadow root 的分配時整棵重算（RFC 0035 的做法）。不選另外維護一份 flat tree 的結構（F2）：每個 DOM 改動都要維護第二份樹，和分配的時機糾纏，kun 沒有其他地方需要。
2. **選擇器做 `:host`、`:host()`、`::slotted()`**：Lit、WebComponents 的元件都用，`selectors` crate 有。`::part()`、`exportparts` 之後再做（Speedometer 的盤點沒看到，要每個元素的 part 名稱和往外傳的對應）；`:host-context()` 不做（`selectors` crate 沒有，css-scoping 標成 at-risk，Firefox 沒有）。
3. **每個 shadow root 一組 stylesheet，同樣的內容共用 parse 和 `RuleSet`**（S1）：`<style>`（照 tree order，不進巢狀的 shadow tree）和 `adoptedStyleSheets`；同樣的 `<style>` 文字以文字為 key 共用，adopted sheet 本來就共用 contents。和 Blink 的 `ScopedStyleResolver`、Stylo 的每個 shadow root 的 `CascadeData` 一樣；配對一個元素只看它那棵樹的規則，一個 shadow root 的 sheet 變了只影響它的 host 的子樹。不選全部合進一個 `RuleSet`（S2）：每個元素配對時要過濾別的樹的規則，同樣的元件的規則重複 N 份，任何一棵樹的 sheet 變了整個重建。
4. **dirty flag 走 shadow-including 的 DOM 祖先**（D1）：`mark_ancestors`（和 layout tree 的）到 shadow root 之後接著標 host；style 和 box tree 的走訪到了 host 先走 shadow tree，再處理有 flag 的 light DOM 子節點（parent style 是分配到的 slot 的）；分配變了的節點標成要重算、要重建。標的時候不用算分配。不選照 flat tree 的祖先標（D2）：mutation 的當下要算分配，分配一改之前標的路徑就不對。
5. **沒有分配到 slot 的 light DOM 子節點照 DOM 的 parent 算 style、沒有盒子**（U1）：`getComputedStyle` 讀得到值，和 `display: none` 下面的元素一樣；走訪不用分兩種。不選不算（U2，Blink 跳過、`getComputedStyle` 另外照需要算）：kun 的 `getComputedStyle` 只讀存著的 style。
6. **做一般的 `display: contents`**（C1）：和 slot 用同一套 box tree 的處理；replaced element 和幾個 HTML 元素照 css-display 當 `none`；UA stylesheet 加 `slot { display: contents }`。不選只給 slot（C2）：作者寫的 `slot { display: block }` 要另外處理，一般的之後還是要做一次。
7. **分兩段**：先 flat tree 的 style 和 layout（連同 shadow tree 的 stylesheet、`display: contents`、幾何、hit test、`elementFromPoint` 的 retarget），再 `:host`、`::slotted()` 和跨樹的 cascade；每段做完可以先 merge。

2026-10-14，第 1 段開始前確認：

8. **第 1 段分三小段**：1a `dom` 和 style、1b layout、1c 幾何、hit test 和驗收（「分階段」）；每小段一份 mutation test，`cargo test --workspace`、fmt 和 clippy 在三小段都做完時跑。範圍跨 `dom`、`css`、`style`、`layout`、`paint`、`dom_api` 和 WPT，一次做完不好審。1a 做完、1b 還沒做時，layout 暫時把 `display: contents` 當 `inline`（和現在的 `slot` 一樣；不 merge 回 `main`）。
9. **tree scope 的 stylesheet 變動由 `dom` 標記，style engine 只處理有標記的 tree scope**（R3，連同 constructed sheet 記 adopt 它的 tree scope）：tree scope 的 style flag 多一個 `style_sheets_changed`，`<style>`、`<link>` 插入、移除、內容或 attribute 變了，`adoptedStyleSheets` 改了時標在它所在的 tree scope（不管樹 connected 與否）；constructed sheet 以 `WeakSet` 記著 adopt 它的 tree scope，內容、`media`、`disabled` 改了只標它們；style engine 只重收有 flag 或第一次碰到的 tree scope 的 sheet，document 的也一樣（「style」一節的「stylesheet 的變動由 `dom` 標記」）。和 Blink 的 `StyleEngine::MarkTreeScopeDirty`、`dirty_tree_scopes_`、`CSSStyleSheet::adopted_tree_scopes_` 一樣（kun 由走訪沿 flag 找到有標記的 tree scope，不另外存一份清單）。成本：插入、移除時多走一次那棵子樹（不進 shadow tree）找這兩種元素，parser 一個個插入的葉子是 O(1)；每份被 adopt 的 sheet 多一個 `WeakSet`（adopt 時可能配置一個 chunk，`soundness.md` 的 I3）；沒有改動的 tree scope 不收。從 sheet 的 adopted 清單拿掉時不從 `WeakSet` 刪：標記時只標清單裡還有它的。有測試用的開關讓 engine 每次重收全部，比對有沒有漏標。不選 R1（style 的走訪進到有 flag 的 shadow tree 就重收：樹裡任何改動都要走整棵樹一次，元件裡的清單一改就重收）、R2（每次 recalc 走全部的 shadow tree，document 現在的做法：和整個 flat tree 成正比，元件多的頁面每個 frame 都付）；也不選 constructed sheet 改了時比對每個 shadow root 的 adopted 清單（O(shadow root 數)，sheet 改得少時夠用，但不是只處理有標記的）。
10. **renderer thread 的 stack 改成 8 MiB**（2026-10-14 確認，做 1c 時發現）：999 層的 shadow tree（WPT 的 `build-deep-detached-shadow-then-append-text.html`）排版時讓 `CrRendererMain` 的 stack（Rust 預設的 2 MiB）溢位；999 層的一般元素本來就一樣，shadow tree 不排版時才沒碰到。改成 Chrome 的 renderer main thread 在 Linux 上的 8 MiB（`render_thread.rs`）。不選把 style、layout、paint 的遞迴改成迴圈：範圍大，不屬於這份；也不選照樣記成限制：只要改一行。

2026-10-15，第 2 段開始前確認：

11. **`:host`、`::slotted()` 的規則另外放兩份清單**（H1）：建 `RuleSet` 時，最右邊的 compound 是 featureless 的 `:host` / `:host()` 的規則放進 `host_rules`，最右邊有 `::slotted()` 的放進 `slotted_rules`，不進一般的桶；host 配對它的 shadow scope 的 `host_rules`（`MatchingContext::with_shadow_host`），分配到 slot 的元素配對它分配到的每一層 slot 所在的 scope 的 `slotted_rules`。和 Blink 的 `RuleSet::shadow_host_rules_`、`slotted_pseudo_element_rules_`，Stylo 的 `CascadeData` 的 `host_rules`、`slotted_rules` 一樣。不選讓 host 和分配到 slot 的元素配對整個 shadow scope 的規則（H2）：每個 host 要試元件的全部規則，`:host` 的規則也進一般的桶、在 shadow tree 裡的元素上一定失敗。
12. **依賴每個元素各自合併**（I1）：看結構的選擇器和元素狀態（`:hover`）的依賴，是元素所在的 tree scope 的，加上（host）它的 shadow scope 的 `host_rules` 的，加上（分配到 slot 的）那幾層 slot 的 scope 的 `slotted_rules` 的；一個元件的 `:host(:hover)` 只讓它的 host 跟著 hover 重算。不選全部的 tree scope 合成一份（I2）：一個元件用了 `:hover`，整頁每個元素的 hover 一改就重算。
13. **第 2 段不再分小段**：只動 `css` 和 `style`，一份 mutation test。跨樹的 cascade 照 css-scoping：`CascadePriority` 在 level 之後加 tree order，normal 的外面的勝出、`!important` 的裡面的勝出（Blink 的 `EncodeTreeOrder` 把 important 的反過來編）；style attribute 和 presentational hint 是元素自己的樹的。host 的 style 要它的 shadow scope 的 `host_rules`，所以走訪先收集 shadow scope 再算 host，shadow scope 的 sheet 變了 host 也重算。

## 待決定

無（第 1 到 13 條都已決定）。

## 之後的可能

- `::part()` 和 `exportparts`、`:host-context()`。
- shadow tree 裡的 `<link rel=stylesheet>`。
- `delegatesFocus` 和 shadow tree 裡的 Tab 順序。
- declarative shadow DOM。
- form control 的 UA shadow tree（路線圖的 G 第二段可能用到）。
- `:has-slotted`（Chrome 有）。
- `elementsFromPoint` 照 Blink 列出文字的 parent（沒有盒子的 slot 也會因為它的 fallback 文字被列出）；輸入事件打到分配到 slot 的文字時，target 照舊是 slot（`elementFromPoint` 回答它的 DOM 的 parent）。
