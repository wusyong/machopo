# RFC 0041：cascade 的功能

- 開始日期：2026-10-13
- 狀態：✅ 已完成：2026-10-13（第 1 到 15 條；五段，各段做完時 merge 回 `main`）
- 細節：`details.md`；進度：`progress.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的 S4，CSS 線接在 S2、S3 之後：「量得對」要的 cascade 的功能。它依賴的 F（RFC 0022）、G 的第一段（RFC 0023）都完成了。

## 摘要

讓 Speedometer 的頁面的 CSS 大部分真的生效：自訂 property 和 `var()`、`calc()` 這一類的數學函式和 viewport 單位、stylesheet 裡的 `@media`（連同 `<style media>`、`<link media>` 和 `matchMedia`）、Speedometer 用到而 kun 還沒有的 pseudo-class（`:hover`、`:active`、`:lang()`、`:read-only` 等）、邏輯屬性（`margin-inline-start` 這一類）。照 Blink 的 `StyleCascade` 把 cascade 改成先找出每個 property 勝出的宣告再套用，`var()` 在套用時代換。分五段，每段做完可以先 merge 回 `main`。

## 動機

**現況（2026-10-13，`main` 的 `297bda44`）**：Speedometer 的 Complex-DOM 系列共用的 `big-dom.css`（Adobe Spectrum，300 KB、637 條規則）在 kun 裡大多不生效：
- `var()` 用了 2399 次，含 `var()` 的宣告整條丟掉（kun 不認得 `var()`，當成無效的值）；自訂 property 解析時存起來，cascade 不用。
- `calc()` 100 次，其中 92 次裡面有 `var()`；`calc()` 整條丟掉。
- 邏輯屬性（`margin-inline-start`、`padding-block`、`inset-inline-end`、`inline-size`…）20 種，都丟掉。
- 有 `:lang()`、`:read-only` 的規則整條丟掉（selector list 裡有一個不認得的 pseudo-class，整條規則無效）；`:hover`、`:active` 解析得了，但永遠不成立。
- `@media` 整個丟掉（這份 CSS 的兩個是高對比模式，在 Chrome 裡也不成立；其他 suite 用 `max-width`、`min-width` 這種會成立的）。

盤點（`rfcs/0012-speedometer/details.md` 的「對照表」）：`@media` 在 14 個 suite、`calc()` 13 個、`var()` 8 個（Complex-DOM 各約 7500 個宣告）、`:hover` 16 個、`:active` 13 個、`:lang(` 和 `:read-only` 各 6 個、`:target` 2 個。

要解決的 `docs/limitations.md` 裡的限制：
- 「Parser」：stylesheet 的 `media` attribute、`@media` 都忽略（`@import` 不在這份）。
- 「Style」：沒有 `calc()`、沒有 viewport 單位、沒有 `inset-inline-start` 這些邏輯屬性；自訂 property 不生效、沒有 `var()`；pseudo-class 有限（這份補 `:hover`、`:active`、`:lang()`、`:read-only`、`:read-write`、`:required`、`:optional`、`:target`）；`box-shadow` 和 `outline-offset` 沒有 `calc()`。
- 「`srcset`、`sizes` 和 `<picture>`」：`sizes` 的長度不能是 `calc()`；`sizes=auto` 拿不到 `var()`、`calc()`、`inline-size` 給的寬度。
- Media query 只用在 `<img>` 選來源。
- 「連結導航和 hit testing」：沒有 hover 和 active 的狀態。

目標：
- 上面這些在 kun 裡和 Chrome 152 一樣生效，WPT 的 `css/css-variables/`、`css/css-values/`（`calc` 和單位）、`css/mediaqueries/`、`css/cssom-view/` 的 `MediaQueryList`、`css/selectors/`、`css/css-logical/` 裡相關的測試通過。
- Complex-DOM 的頁面在 kun 裡和 Chrome 的畫面接近（「量得對」），每段做完用 `ci.py speedometer` 量一次，style 的時間不能因為多了 `var()` 失控（RFC 0039 之後 `Document::UpdateStyle` 已經是原本的十幾分之一）。

不在範圍內：
- **pseudo-element**（`::before`、`::after` 是 S3；`::placeholder`、`::-webkit-input-placeholder` 跟著控制項的繪製（G 的第二段）；`::selection` 跟著選取的繪製）。
- `:valid`、`:invalid`、`:in-range` 這一類：要 constraint validation，kun 還沒有。
- `@property` 和 `CSS.registerProperty`（有型別的自訂 property）、`@layer`、`@import`、`@container`、CSS nesting、`revert`、`env()`、`attr()`（3 個 suite 用在 `content`，跟著 S3）。
- `writing-mode` 和 `direction`：邏輯屬性照 `horizontal-tb`、`ltr` 對應到物理的 property（第 5 段的選項）。
- style 的 invalidation set（只重算受影響的元素）：另一份。

## 設計概要

對網頁的作者：
- `--accent: #06c` 宣告在任何元素上，往下繼承；`color: var(--accent)`、`margin: var(--gap, 8px) 0`、`width: calc(100% - var(--sidebar))` 照規範代換和計算，循環的、代換之後無效的宣告是「invalid at computed-value time」（當成 `unset`）。`getComputedStyle(el).getPropertyValue('--accent')` 讀得到。
- `calc()`、`min()`、`max()`、`clamp()` 用在長度、百分比、數字、角度能用的地方；`vw`、`vh`、`vmin`、`vmax` 照 frame 的 viewport。
- `@media (max-width: 430px) { … }`、`<link rel=stylesheet media="print">` 照 frame 的 viewport 生效，視窗改變大小時重新判斷；`matchMedia('(prefers-color-scheme: dark)')` 回傳 `MediaQueryList`，`change` 事件在 rendering update 裡分派。
- 滑鼠移到元素上 `:hover` 成立、按下時 `:active` 成立；`:lang(en)`、`:read-only` 這些照 HTML 的定義成立。
- `margin-inline-start: 4px` 和 `margin-left: 4px` 一樣（kun 只有 `horizontal-tb`、`ltr`），和物理的 property 照宣告的先後決定誰勝出；`style.marginInlineStart` 讀得回來。

對改 `style` 的人：cascade 先算自訂 property，再找出每個 longhand 勝出的那一條宣告，只套用勝出的；含 `var()` 的宣告在 `css` 裡存成沒解析的 token，套用時才代換、解析。

## 詳細設計

### 第 1 段：自訂 property 和 `var()`

- **解析**（`crates/css/src/variables.rs`、`properties.rs`、`property_set.rs`）：值裡有 `var()` 時（CSS-wide 關鍵字之後、照 property 解析之前檢查），不解析成型別化的 `Value`，存成 `Value::Unparsed`（Blink 的 `CSSUnparsedDeclarationValue`）：去掉前後空白的原文，和它是哪個 shorthand 的；shorthand 的每個 longhand 都放同一個 `Arc`（Blink 的 `CSSPendingSubstitutionValue`）。解析時檢查 `<declaration-value>` 和每個 `var()` 的語法（`var( <custom-property-name> [, <declaration-value>? ]? )`，名稱不分大小寫的 `var`，fallback 可以是空的，結尾沒關的區塊照 CSS 自動關上），不檢查代換之後對不對。SVG 的 presentation attribute 也收（Blink 的 presentation attribute 走同一個 `CSSPropertyParser::ParseValue`）。`url()` 照宣告所在的 stylesheet 的 base URL，和其他宣告一樣。
- **computed style**（`crates/computed_style/src/variables.rs`）：自訂 property 的 computed value 是代換過 `var()` 的文字，去掉前後空白（沒有 `@property`，全部繼承）。`StyleVariables` 是 persistent 的 hash array-mapped trie（第 2 條），`ComputedStyle::inherit` 複製根的 `Arc`；invalid at computed-value time 的存成沒有值的項目（名稱照樣列出，和 Blink 存 null 一樣），沒有刪除。名稱的 hash 是 FNV-1a 加 SplitMix64 的混合，不用任何全域的表（`computed_style` 不能有 interning 或全域的快取）。比較兩份 map：同一個根就相等；個數或所有項目的 hash 的 XOR 不同就不等；否則逐個節點比，共用的子樹（同一個 `Arc`）直接跳過（只有插入、沒有刪除，同一組名稱的 trie 形狀一定一樣）。重算一個元素時，新算的自訂 property 和舊的 style 的相等，就沿用舊的 map：子孫的 map 因此和它們舊的共用節點，比較新舊的 style（決定 box 要不要換）不必走過整份 map。原本沒有這兩件事：Complex-DOM 的頁面根元素上有約 1600 個自訂 property、6600 個元素都繼承，每次整份重算時每個元素都逐項比較新舊兩份內容一樣、但各自建的 map，`Document::UpdateStyle` 變成原本的 3.3 倍（`progress.md`）。
- **cascade**（`crates/style/src/lib.rs`）：照 Blink 的 `StyleCascade`（第 3 條）：區塊照優先順序排好之後由高往低走，每個 longhand 記下第一次遇到的那條（`LonghandId::index` 的陣列，Blink 的 `CascadeMap`），自訂 property 也一樣（第一次遇到的名稱）。自訂 property 先算（`compute_custom_properties`：深度優先、需要時才代換別的自訂 property；照 Blink 的 `CascadeResolver` 用堆疊偵測循環，循環裡的每一個都變成 invalid，引用它們的照自己的 fallback；`initial` 是 guaranteed-invalid、`inherit` 和 `unset` 是父元素的值），再 `font-size`，再其他 longhand 照 `LonghandId` 的順序。勝出的是 `Value::Unparsed` 時代換、照那個 property（或它的 shorthand）解析；同一個 shorthand 的 longhand 共用一次代換和解析；失敗就照 `unset`。代換的結果超過 2 MiB 就失敗（Blink 的 `kMaxVariableBytes`，防止 `--b: var(--a) var(--a)` 這種一層翻倍）。代換是 token 的代換：兩邊的文字會連成一個 token 的地方（`var(--a)px`）插入 `/**/`（css-syntax 序列化 token 的規則）。
  - 改成只套用勝出的宣告時發現 `LonghandId::ALL`（註解寫「每個 longhand」）漏了 `position` 和四個 inset，`getComputedStyle` 的 `length` 和 `item()` 因此一直少列這五個；補上，加了 `ALL` 和 enum 一致的測試。
- **CSSOM**：`CSS.supports('--x', '1')`、`CSS.supports('color', 'var(--x)')` 是 true；`element.style.setProperty('color', 'var(--x)')` 收下，照原文序列化；shorthand 有 `var()` 時讀 shorthand 是原文，它的 longhand 讀回空字串；`cssText` 寫得成 shorthand 時（每個 longhand 都還是它的值）寫 shorthand，否則照 Chrome 把那些 longhand 寫成空的值（`margin-right: ;`，Blink 的 `StylePropertySerializer`）。這樣的文字解析不回來，所以 inline style 照 Blink 存解析好的宣告（第 4 條，`crates/dom/src/node.rs`、`scope.rs` 的 `inline_style` 和 `set_inline_style`，`crates/dom_api/src/css_style_declaration.rs`）：`style` attribute 由 parser、`setAttribute`、`removeAttribute`、`Attr` 設定時解析；CSSOM 改存好的宣告、attribute 寫成 `cssText` 但不再解析；cascade 讀存好的。第 1 段一開始照 RFC 0016 只存 attribute 的文字，CSSOM 每次修改都把 attribute 重新解析，`cssText` 因此改成把 shorthand 寫在它最後一個 longhand 的位置（解析得回來），和 Chrome 不同；審核時改成照 Blink。`getComputedStyle` 讀得到自訂 property（沒有值的是空字串）；`length` 和 `item()` 把它們列在 longhand 之後，連 invalid 的也算（Chrome 的 `CSSEnumeratedCustomProperties` 是 stable，`ComputedStyle::GetVariableNames`），kun 依名稱排序（Chrome 是 hash table 的順序）。
- **失效**：自訂 property 是繼承的，父元素的值改了，子孫跟著重算，現在的「重算整個子樹」已經涵蓋。

### 第 2 段：`calc()` 和 viewport 單位

分三步：2a 長度和百分比，2b 數字、整數、角度，2c viewport 單位。

- **解析**（`crates/css/src/calc.rs`）：`calc()`、`min()`、`max()`、`clamp()` 照 css-values-4 解析成運算樹，邊解析邊化簡（Blink 的 `CSSMathExpressionNode`）：數字乘進去、除以數字（除以 0 無效）、同一個單位的項加起來（絕對長度換成 px）、參數都是同一個單位的 `min()` / `max()` / `clamp()` 直接算出來、只有一個參數的就是它；`+`、`-` 兩邊要有空白；`pi`、`e`。型別檢查：數字、長度、百分比、長度加百分比、角度；`min()`、`max()`、`clamp()` 的參數要同一種，長度和百分比混在一起的無效（第 5 條）。接在共用的解析函式（第 6 條）：長度和長度百分比的 `Length::parse` / `LengthPercentage::parse`，數字、整數、角度的 `parse_number`、`parse_number_at_least`、`parse_integer`、`Angle::parse`，所以每個 property 都有。
- **specified value**（`crates/css/src/values.rs`）：`Length` 多一個 `Calc(Arc<Calc>)` 變體、`LengthPercentage` 也多一個（有百分比的放這裡，只有長度的是 `Length::Calc`），兩者和包著它們的 specified 型別（`LineWidth`、`FontSize`、背景的位置和大小、`transform` 的函式……）不再是 `Copy`，只影響 `css` 的內部。序列化照 css-values-4（Chrome 也是）：`calc()` 包著，項照數字、百分比、各單位的名稱排，負的寫成 ` - `；根是 `min()`、`max()`、`clamp()` 的照寫。數字、整數、角度不需要元素，解析時就算成一個值（整數四捨五入、小於下限的夾到下限），型別不變；CSSOM 讀回的是那個值，不是 Chrome 的 `calc(0.5)`（記成限制）。
- **computed value**（`crates/computed_style/src/lib.rs`）：`LengthPercentage` 多一個 `Calc { px, percent, non_negative }`（第 5 條），`resolve(basis)`、`px()`、`has_percent()` 給用的地方；只有長度的算成 `Px`，只有百分比的是 `Percent`。不能是負的 property（`width`、`padding`……）的 `calc()` 在算出來時夾到 0，有百分比的在用的時候夾（Blink 的 `CalculationValue` 的 `ValueRange`）。`getComputedStyle` 照 Chrome 寫成 `calc(50% - 10px)`；`background-position` 從右邊或下面算的換成從左邊（`calc(90% - 5px)`）。
- **layout 和 paint**：原本直接比對 `Px` / `Percent` 的地方改用 `resolve(basis)`；百分比沒有基準時當成 `auto` 的地方，`calc()` 也一樣。taffy 的 `calc` 是不透明的 handle：`crates/layout/src/calc.rs` 的 `CalcValues` 是 box tree 的一張表（同一個值只放一次），handle 是 `(index + 1) << 3`，`resolve_calc_value` 查表，不用 `unsafe`；workspace 的 taffy 打開 `calc` 這個 feature。taffy 沒有 `calc()` 的 `fit-content()`，那裡只用 `calc()` 的長度部分。做的時候發現 taffy 0.14 的 block layout 用父元素的高度解析子元素上下的 padding 和 border 的百分比（純百分比的也是），記成限制，不 fork taffy。
- **viewport 單位**（第 7 條）：`LengthUnit::Viewport` 是 `vw`、`vh`、`vmin`、`vmax` 和 `s`、`l`、`d` 開頭的那 12 個（桌面上三種 viewport 一樣，照寫的名稱序列化）。`ComputeContext` 多帶 frame 的 viewport 和一個「用到了」的旗標，cascade 結束時寫進 `ComputedStyle::has_viewport_units`（不繼承：子元素拿的是父元素算好的 px，父元素重算時它跟著重算）。`StyleEngine::set_viewport` 在 viewport 變了、而且有 style 用過 viewport 單位時回報；下一次重算走過每個元素，重算有旗標的。renderer 在 `update_own_style` 把 frame 的 viewport 交給它，`update_own_style_and_layout` 先更新 viewport 再更新 style。canvas 的 `letterSpacing` 照舊不收 viewport 單位。
- **其他自己解析長度的地方**：HTML 的 `sizes` attribute 改用 `css` 的長度解析（`parse_length_px`：`calc()`、`svw` 這些都有，`em` 是 16px），值是最後一個 component、前面才是條件（只有一個 `calc()` 時不再被當成不認得的 media condition）；`IntersectionObserver` 的 `rootMargin` 只收 px 和百分比，和 Chrome 一樣，不接。

### 第 3 段：`@media`、`@supports` 和 `matchMedia`

分三步：3a stylesheet 的 `@media`、`@supports` 和重新判斷，3b CSSOM，3c `matchMedia()`。

- **media query 搬到 `css`**：原本在 `style` 的 `media.rs`（現在的 `crates/css/src/media.rs`）的解析和判斷（`MediaQueryList`、`MediaEnvironment`）搬到 `css`，stylesheet 的 parser 在那裡解析 `@media` 的 prelude（Blink 的 `MediaQuerySet`、`MediaQueryEvaluator` 也和 CSS parser 同在 `core/css/`）；`sizes` 和 `<img>` 的來源選擇用搬過去的。
- **3a stylesheet**：parser 收 `@media` 和 `@supports`，存成 grouping rule（Blink 的 `StyleRuleMedia`、`StyleRuleSupports`），裡面可以再巢狀；其他 at-rule 照舊丟掉。`@supports` 的條件用 `CSS.supports` 的判斷，解析時就算好（Blink 的 `ConditionIsSupported`），不隨環境改變。建 `RuleSet` 時跳過不符合的 `@media` 和 `@supports`，記下每個判斷過的 media query 和它的結果（第 10 條）；`<style media>`、`<link media>` 和 constructed stylesheet 的 `media` 是整份的條件，同樣記下。規則在 cascade 的順序照它在原文的位置，不管它在第幾層。
- **重新判斷**（第 10 條）：viewport 或 device pixel ratio 改變時（renderer 的 `environment_changed`，`StyleEngine` 也拿到 device pixel ratio），只重算記下的 media query；有一個結果不同才重建 `RuleSet`、整份重算 style（Blink 的 `RuleSet::DidMediaQueryResultsChange`、`StyleEngine::MediaQueryAffectingValueChanged`）。constructed stylesheet 的 `StyleSheetContents` 快取一個 `RuleSet`，判斷的結果和這次不同就丟掉重建（Blink 的 `StyleSheetContents::EnsureRuleSet`；大小不同的 iframe adopt 同一份時會輪流重建，Blink 也一樣）。
- **3b CSSOM**（第 8 條）：照 Chrome 做 `CSSGroupingRule`（`cssRules`、`insertRule`、`deleteRule`）、`CSSConditionRule`（`conditionText`）、`CSSMediaRule`（`media`）、`CSSSupportsRule`，和 `MediaList`（`mediaText`、`length`、`item()`、`appendMedium`、`deleteMedium`，和 stringifier）、`StyleSheet.media`；`insertRule` 收 `@media`、`@supports`。media query 照 mediaqueries-4 和 Chrome 序列化。`CSSStyleRule` 照舊直接繼承 `CSSRule`（nesting 不在範圍內）。
- **3c `matchMedia()`**：`MediaQueryList`（`media`、`matches`、`change` 事件、`onchange`、`addListener` / `removeListener`）和 `MediaQueryListEvent`，JS 和 wasm。list 是 `EventTargetImpl` 的一種（和 `MessagePort` 一樣），記著 document、query 和上一次的結果；`matches` 每次讀都重新判斷、記下結果（Blink 的 `UpdateMatches`），renderer 的 `BeginMainFrame` 在 animation frame callback 之前對每份有 list 的 document 跑「evaluate media queries and report changes」：照建立的順序重新判斷，結果和上一次不同的發 `change`（Blink 的 `MediaQueryMatcher::MediaFeaturesChanged`，它只在環境改變後判斷；每個 frame 都判斷，結果相同時看不出差別）。document 用 `WeakSet` 記著全部的 list，有過 listener 的另外強指著，script 不拿著它也收得到 `change`；Blink 在 listener 都拿掉之後讓它被回收（`HasPendingActivity`），kun 照樣留到 document 不在了，script 看不出差別，只差在記憶體，記成限制。

### 第 4 段：pseudo-class

分兩步：4a `:hover`、`:active` 和滑鼠的邊界事件，4b 其他的 pseudo-class。

- **4a `:hover`、`:active`**：`dom` 的 `ElementState` 加 `HOVER`、`ACTIVE`，沿著 flat tree 的祖先設定（Blink 的 `Document::UpdateHoverActiveState`）；滑鼠在子 frame 裡時，iframe 元素和它的祖先也算 hover（Blink 的 frame owner 鏈）。renderer 在處理滑鼠的移動、按下、放開時記下滑鼠下的元素和按下的元素；捲動和 layout 改變之後重新判斷滑鼠下的元素（Blink 的 `RecomputeMouseHoverState`），只更新 hover 和邊界事件，不分派 `mousemove`。style 走現有的 element state：只有規則用到這個狀態才重算。
- **4a 邊界事件**（第 13 條）：滑鼠下的元素換了時，照 Blink 的 `BoundaryEventDispatcher` 對舊的發 `mouseout`、對新的發 `mouseover`（冒泡，`relatedTarget` 是另一個），再對離開的祖先發 `mouseleave`、進入的發 `mouseenter`（不冒泡，到共同的祖先為止），和 hover 共用「上次在哪個元素」；元素被移出 document 時照 Blink 改成它還在 document 裡的祖先。
- **4b**：
  - `:lang()`：元素自己和祖先的 `lang` / `xml:lang`（同一個元素上 `xml:lang` 優先），都沒有時是 document 的 content language：最後插入的 `<meta http-equiv="content-language">`，再來是 HTTP 的 `Content-Language`（Blink 的 `Document::ContentLanguage`；renderer 拿不到 response 的 header，commit 時要多帶這一個）。比對時往上找（Blink 的 `ComputeInheritedLanguage`），比對照 selectors-4 的 extended filtering（RFC 4647，`*` 萬用的子標籤）。`lang` 改了本來就重算子樹；content language 改了整份重算。
  - `:read-only` / `:read-write`（第 14 條）：可以編輯的 `input`（`readonly` 適用的 type）、`textarea`，沒有 `readonly`、不是 disabled 的是 read-write，存成 form state；其他元素在 editable（`contenteditable`）時是 read-write，比對時往上找。kun 沒有 `designMode`。`:read-only` 是不 read-write 的元素。
  - `:required` / `:optional`（第 14 條）：`required` 適用的 `input`、`select`、`textarea`，存成 form state。
  - `:target`：document 強指著它的 target element（Blink 的 `Document::css_target_`，`ElementState` 的 `TARGET`），在 parse 完（Blink 的 `FinishedParsing`）、同一份文件內的導覽和 traversal 改了 fragment 時設定（HTML 的「scroll to the fragment」的 indicated part），移除、被別的 document adopt 之後仍然是 target（`:target` 看元素目前的 document 的 target element，搬回來又符合；whatwg/html#10029，WPT 的 `target-pseudo-after-reinsertion.html`、`target-pseudo-after-adoption.html`，Chromium 修 crbug 1252507 之後也是這樣）；之後插入同 `id` 的元素不會變成 target。

### 第 5 段：邏輯屬性

- **property**（第 15 條，`crates/css/src/properties.rs`）：30 個 longhand 是 `LonghandId` 自己的變體，排在物理的之後：`margin`、`padding`、`inset` 的 `-block-start` / `-block-end` / `-inline-start` / `-inline-end`，`border-{block,inline}-{start,end}-{width,style,color}`，`inline-size`、`block-size` 和它們的 `min-`、`max-`。`LonghandId::physical` 是對應到的物理 longhand（kun 只有 `horizontal-tb`、`ltr`：block-start 是 top、inline-start 是 left、inline size 是 width；Blink 的 `CSSDirectionAwareResolver`），解析、套用和 computed value 的序列化都先換成它，只有名稱和 cascade 的位置是自己的。shorthand 18 個：兩個值的 `margin-block`、`margin-inline`、`padding-*`、`inset-*`、`border-{block,inline}-{width,style,color}`（start 再 end，只給一個時兩邊一樣；兩邊一樣時寫一個），一邊的 `border-block-start` 這四個（`border-top` 的寫法），兩邊的 `border-block`、`border-inline`（兩邊一樣時才寫得成它，Blink 的 `BorderPropertyValue`）。
- **cascade**（`crates/style/src/lib.rs`）：勝出的表照物理的那一格收，邏輯和物理的屬性共用一格，跨區塊照優先順序；同一個區塊裡後寫的勝出，所以每個區塊從尾端讀（Blink 先把 surrogate 換成物理的，再照位置比）。一個區塊每個 property 只留最後一個、照位置排（`from_parsed`，和 Blink 的 `FilterProperties` 一樣），不必另外處理。
- **CSSOM**（`crates/css/src/property_set.rs`）：同一組（logical property group：margin、padding、inset、border 的寬度、樣式、顏色、大小、最小、最大）的屬性照 CSSOM 的「set a CSS declaration」：`setProperty` 設定的 property 後面有同一組、對應方式不同的宣告，就把它移到最後（值一樣也移），讓它照設定的順序勝出（Blink 的 `MutableCSSPropertyValueSet::FindInsertionPointForID`、`IsInSameLogicalPropertyGroupWithDifferentMappingLogic`）。`cssText` 照 Blink 的 `StylePropertySerializer::AsText`，沒有規範那條「中間夾著同一組、對應方式不同的宣告就不合成 shorthand」（對照過 Chromium 的原始碼）。camelCase 的名稱（`style.marginInlineStart`）和 `CSS.supports` 自然都有。
- **`getComputedStyle`**：列出邏輯屬性（Chrome 也列），值照對應的物理屬性解析：先換成物理的名稱，再看要不要 layout 的 used value（Blink 的 `ResolveDirectionAwareProperty`；`css` 的 `physical_property`，`dom_api` 的 `computed_style`）。

### 測試

- **`css`、`style` 的整合測試**：每段的解析、序列化、cascade 的結果（代換、循環、fallback、invalid at computed-value time、shorthand 的代換、繼承、`calc()` 的化簡和 computed value、media query 的結果和重新判斷、pseudo-class 的比對、邏輯和物理的勝負）。
- **layout**：`calc()` 混百分比在 block、flex、grid 裡的大小。
- **WPT**：每段匯入相關的目錄（`css/css-variables/`、`css/css-values/`、`css/mediaqueries/`、`css/cssom-view/MediaQueryList-*`、`css/selectors/` 的 hover 和 lang、`css/css-logical/`），結果和預期結果記進 `progress.md`。
- **情境**：`about:demo` 加示範（`var()` 換主題色、`calc()` 和 `@media` 隨視窗寬度變、`:hover` 和邊界事件、`:target`、`margin-inline-start` 和 `inline-size`），寫成 `demo-script` 和 `resize` 的步驟。
- **效能**：每段做完量 Complex-DOM 的 suite 和 jQuery，`Document::UpdateStyle` 的時間記進 `progress.md`。
- mutation test 每段做。

### 分階段

五段，每段做完、審核之後可以先 merge 回 `main`；每段開始前只決定那段的事：

1. 自訂 property 和 `var()`（cascade 改成先找勝出的宣告）。
2. `calc()`、`min()`、`max()`、`clamp()` 和 viewport 單位。
3. `@media`、`<style media>` / `<link media>`、`matchMedia` 和 `MediaQueryList`。
4. pseudo-class：`:hover`、`:active`、`:lang()`、`:read-only` / `:read-write`、`:required` / `:optional`、`:target`。
5. 邏輯屬性。

先 `var()`：Complex-DOM 的 `var()` 最多（2399 次），`calc()` 裡 92% 也有 `var()`；而且 cascade 的改寫是後面幾段的底。

## 缺點

- cascade 多一個階段（收集勝出的宣告），程式比「依序套用、後面蓋前面」複雜；但只套用勝出的宣告，代換和解析的次數比全部套用少。
- 含 `var()` 的宣告每個元素都要代換、重新解析一次：Complex-DOM 現在丟掉的 2399 處會開始花時間。量了再決定要不要快取（Blink 有 `CSSVariableData` 的 token 和解析的快取）。
- `ComputedStyle` 多一份自訂 property 的資料，比較兩份 style 是否相同（決定子孫要不要重算）也要比它。

## 理由和其他做法

- **為什麼一份 RFC、五段**：五件事都在 `css` 和 `style`，cascade 的改寫是共同的底；但彼此獨立到可以分段 merge。拆成五份 RFC 的話，cascade 的改寫只屬於其中一份，其他幾份要回頭引用它。
- **順序**：見「分階段」最後。`@media` 雖然最多 suite 用（14 個），但 evaluator 已經有，工作量小、可以往前挪；`var()` 是 cascade 的改寫，先做讓後面幾段不必改兩次。
- 第 1 段的選項的完整比較在 `details.md` 的「第 1 段的選項」。

## 先例

- **Blink**：`StyleCascade`（`CascadeMap` 記每個 property 勝出的優先順序，`Apply` 時才代換 `var()`、偵測循環）、`CSSUnparsedDeclarationValue` / `CSSPendingSubstitutionValue`、`CSSVariableData`、`StyleVariables`（`HashTrieNode`，persistent 的 array-mapped trie）、`CSSMathExpressionNode` 和 `CalculationValue`、`MediaQueryEvaluator` 和 `RuleSet` 的 media query 結果、`MediaQueryMatcher` 和 `MediaQueryList`、`Document::UpdateHoverActiveState`、`CSSDirectionAwareResolver`。
- **Stylo**：`custom_properties.rs`（代換和 Tarjan 的循環偵測）、`CustomPropertiesMap`（一串 `Arc` 的層，最多 4 層）、`PropertyDeclaration::WithVariables`、`CalcLengthPercentage`。
- 細節在 `details.md` 的「先例」。

## 已決定的事項

1. **一份 RFC、五段，順序是 `var()`、`calc()`、`@media`、pseudo-class、邏輯屬性**（2026-10-13）：五件事都在 `css` 和 `style`，cascade 的改寫（第 3 條）是共同的底，放在第 1 段，後面幾段不必再改一次；`var()` 在 Complex-DOM 用得最多，`calc()` 裡 92% 也有它。範圍照「動機」的「不在範圍內」：pseudo-element、`:valid` 這一類、`@property`、`@layer`、`@import`、`@container`、nesting、`revert`、`env()`、`attr()`、`writing-mode` 和 `direction` 都不在這份。不選「`@media` 先做」（工作量小，但 cascade 的改寫會晚一段）、不選拆成好幾份 RFC（cascade 的改寫只屬於其中一份，其他幾份要回頭引用它）。
2. **Q1：V3，自訂 property 照 Blink 的 `StyleVariables` 存成 persistent 的 hash array-mapped trie**（2026-10-13）：每個節點 16 個槽，key 的 hash 每 4 bit 選一個槽，槽裡是一個值或子節點；節點是 `Arc`，複製一份 style 的變數只複製根的 `Arc`，改一個值時只複製從那個節點到根的路徑；整個 map 帶一個累加的 hash（每一對 key、value 的 hash 的 XOR），比較兩份通常不必走訪。自己寫在 `computed_style`（Blink 也是自己寫的），不加 `imbl`、`rpds` 這類依賴，也不需要 `unsafe`。Spectrum 的形狀是上面一個元素宣告上千個、元件的元素再改幾十個（`details.md` 的「Speedometer 的 CSS（2026-10-13）」），每個元件元素的成本是幾十條路徑，不是上千個項目。原本同一天先決定的是 V2（Blink 舊的 `StyleInheritedVariables` 的 `root_`：指向第一個宣告了變數的祖先的那份，加上往下累積的差異），對照 Chromium 現在的原始碼時發現 Blink 已經拿掉 `root_`、改成這個 trie，重新決定。不選 V1（一個 copy-on-write 的 map：每個元件元素複製上千個項目）、V2（Blink 已經放棄的做法；Stylo 的一串 `Arc` 的層，最多 4 層、自己這層 8 個以內就照 copy-on-write 複製，也是同一類，程式比較少，但層數到了上限之後又回到整層複製）。
3. **Q2：C2，cascade 先找出每個 property 勝出的宣告再套用**（2026-10-13）：照 Blink 的 `StyleCascade` / `CascadeMap`，每個 longhand 只記勝出的那一條；自訂 property 先算（遞迴代換、偵測循環），再 `font-size`，再其他。每個 property 最多代換、解析一次；`@media`、邏輯屬性也建在它上面。不選 C1（依序套用全部：被蓋掉的也要代換，自訂 property 互相參照時還是要分出收集的階段）、C3（只做 C2 的一半）。

4. **inline style 照 Blink 存解析好的宣告**（2026-10-13，第 1 段做完、審核時）：元素在 `dom` 的狀態裡存解析好的 `CssPropertyValueSet`（Blink 的 `ElementData` 的 inline style），在 `style` attribute 由 parser、`setAttribute`、`removeAttribute`、`Attr` 設定時解析；CSSOM 直接改它、把 `cssText` 寫進 attribute 但不再解析回來（Blink 的 `InlineStyleChanged`）；cascade 讀存好的，不再每次解析。這樣 `cssText` 可以照 Chrome 寫出解析不回來的東西：shorthand 的 `var()` 值之後又個別設定一個 longhand 時，其他的 longhand 寫成空的值（`margin-right: ;`，WPT 的 `variable-cssText.html` 的 target9）。原本（第 1 段做完時）kun 每次 CSSOM 修改都把 attribute 重新解析，`cssText` 只好寫成解析得回來的形式，和 Chrome 不同。這推翻 RFC 0016 的第 2 條（只存 attribute 字串）：它不選這個做法的兩個理由，一個是 `style` 的型別要放進 `Node`（`dom` 之後為了 constructed stylesheet 已經依賴 `css`），一個是延遲同步 attribute 時每個讀 attribute 的入口都要先同步；這裡 CSSOM 修改時就把文字寫進 attribute（不像 Blink 延遲到讀的時候），沒有要同步的入口，從外面看和 Blink 一樣（`MutationObserver` 的紀錄、`getAttribute`）。不選「`cssText` 照 Chrome、attribute 另存一份解析得回來的文字」（`getAttribute('style')` 和 `cssText` 會不同，Chrome 兩者一樣），也不選維持現狀當成限制。

5. **Q3：P1，`calc()` 的 computed value 是 `LengthPercentage` 多一個 `Calc { px, percent }` 變體，維持 `Copy`**（2026-10-13，第 2 段開始前）：Blink 的 `CalculationValue` 常見的那一半（`PixelsAndPercent`）。線性的組合（`calc(100% - 20px)`、`calc(50% + 2em)`、`calc(var(--a) * 2)`、巢狀的 `calc()`）都存得下；`min()`、`max()`、`clamp()` 只有長度或只有百分比時在 style 就算成一個值；混了百分比和長度的（`min(100%, 600px)`）先不支援（宣告無效），記成限制。layout 和 paint 用共用的 `resolve(basis)`；taffy 的 `calc` 用 index 當 handle。不選 P2（完整的運算樹放在 `Arc` 裡：全部的寫法都支援，但 `LengthPercentage` 和包著它的 15 個 `Copy` 型別都不再是 `Copy`，`layout`、`paint` 約 300 處要檢查；Speedometer 用到的 `calc()` 都是線性的）。之後需要時加一個 `Expr(Arc<…>)` 變體，`Calc` 留著當常見情況的快路。也不選保持 `Copy` 的兩種表：全域的表（`computed_style` 不能有全域的表或 interning）、每個 `ComputedStyle` 自己的表（`inherit` 把值複製到子元素時 index 指到別人的表）。
6. **Q4：`calc()` 和 `min()`、`max()`、`clamp()` 接在共用的解析函式**（2026-10-13，第 2 段開始前）：長度、長度百分比、數字、整數、角度都經過同一組解析函式，每個 property 自然都有，和 Chrome 一樣。不經過它們的（`sizes` attribute、`IntersectionObserver` 的 `rootMargin` 這類自己解析的）照查到的情況接上或記成限制。不選只先接長度和百分比。
7. **Q5：viewport 單位照 Blink，每個 `ComputedStyle` 記下有沒有用到，viewport 改變時只重算用到的元素**（2026-10-13，第 2 段開始前）：`vw`、`vh`、`vmin`、`vmax`（和 `svw`、`lvw`、`dvw` 這些，桌面上三者相同，當成 `vw`）在 style 算成 px；算的時候用到就在 computed style 記旗標（Blink 的 `ComputedStyle::HasViewportUnits`），frame 的 viewport 改變時把有旗標的元素標成要重算（Blink 的 `StyleEngine::MarkViewportUnitDirty`）。不選「document 用到就整份重算」（簡單，但每次改變視窗大小都重算整份），也不選把 viewport 單位延到之後。

8. **Q6：CSSOM 照 Chrome 完整做 `CSSGroupingRule`、`CSSConditionRule`、`CSSMediaRule`、`MediaList`**（2026-10-13，第 3 段開始前）：`cssRules`、`insertRule`、`deleteRule`、`conditionText`、`media`，`MediaList` 的 `mediaText`、`length`、`item()`、`appendMedium`、`deleteMedium`，`StyleSheet.media`；`insertRule` 收 `@media`。Chrome 這些都有；只做一半的話，`replaceSync` 進去的 `@media` 會生效卻在 `cssRules` 裡看不到，偵測功能的 script 也會判斷錯。不選唯讀的 `CSSMediaRule`（不能 `insertRule` 進 `@media`、沒有 `appendMedium` / `deleteMedium`，和 Chrome 不同），也不選這一段不做 CSSOM（constructed stylesheet 的 `cssRules` 和實際生效的規則對不上）。
9. **Q7：`@supports` 和 `@media` 一起做**（2026-10-13，第 3 段開始前）：條件的判斷 `CSS.supports` 已經有；兩者共用 grouping rule 的解析、`RuleSet` 和 CSSOM（Blink 的 `CSSSupportsRule` 和 `CSSMediaRule` 共用 `CSSConditionRule`），條件也不隨環境改變。網頁常用，WPT 的 `css-variables` 有 67 個 `variable-supports` reftest 卡在它。不選留給之後的 RFC。
10. **Q8：建 `RuleSet` 時判斷 `@media`，結果變了才重建**（2026-10-13，第 3 段開始前）：照 Blink，建 `RuleSet` 時跳過不符合的 `@media`（`RuleSet::AddRulesFromSheet` 的 `MatchMediaForAddRules`），記下每個 media query 和它的結果；環境改變時只重算這些 query，有一個結果不同才重建、整份重算 style（`RuleSet::DidMediaQueryResultsChange`，對照過 Chromium 的原始碼）。比對 selector 的熱路徑不變；Stylo 的 `effective_media_query_results` 也是這樣。constructed stylesheet 照 Blink 的 `StyleSheetContents::EnsureRuleSet` 只快取一個，結果不同就重建。不選「規則都放進 `RuleSet`、比對時才看條件」（不用重建，但比對的熱路徑每條規則多一次檢查，和 Blink、Stylo 都不同）。
11. **Q9：dom 的規則物件是一個 GC 型別 `CssRule`，種類放在資料裡**（2026-10-13，3b 開始前）：原本的 `CssStyleRule` 改成 `CssRule`，放一個 `css::CssRule`（style、media、supports）；`parentRule`、grouping rule 的子規則物件和 `cssRules` 的項目都是 `Edge<CssRule>`。binding 依種類選 `CSSStyleRule`、`CSSMediaRule`、`CSSSupportsRule` 的 prototype（和 `Event`、`EventTarget` 一樣），dom 的函式檢查種類。和 kun 的 `Node`、`Event` 同一個理由：Rust 沒有繼承，拆成多個 GC 型別，指標就要到處轉型。代價是每個規則物件帶著用不到的空 edge（規則物件只在 script 要時才建），以及誤用要在執行時檢查。不選一種 interface 一個 GC 型別（Blink 的 class 分法：`parentRule` 和清單的項目要用 `TaggedEdge` 或每種一條 edge，多出 `unsafe` 的指標轉換和 soundness 的論證，dom_api 每處都要分種類；之後每加一種 at-rule 就多一個 GC 型別）。
12. **Q10：規則的內容照舊不可變，改巢狀的規則時沿著路徑複製**（2026-10-13，3b 開始前）：改了的規則做新的一份，往上每一層 grouping rule 也做新的（只複製那一層的規則清單，其他的共用 `Arc`），直到 sheet 的 `StyleSheetContents`；路上已經建立的規則物件換成新的 `Arc`，用 `Arc::ptr_eq` 找它在父層的位置（和原本改 `selectorText` 的路一樣）。`css` 維持純資料，`soundness.md` 的前提不變，「另一個 `Arc` 就是另一個版本」照舊判斷內容換了沒。不選照 Blink 在原地改（規則要有 interior mutability，共用時先複製整份再重新掛上 CSSOM 物件，`RuleSet` 改記版本號，finalizer drop 的物件帶鎖，soundness 要重寫；從外面看結果一樣）。
13. **Q11：`mouseover`、`mouseout`、`mouseenter`、`mouseleave` 和 `:hover` 一起做**（2026-10-13，第 4 段開始前）：照 Blink 的 `BoundaryEventDispatcher`，和 hover 用同一個「上次在哪個元素」和同一個共同祖先的計算；網頁常用它們做選單和 tooltip，分開做的話之後要再改一次同樣的滑鼠程式。不選只做 `:hover` 和 `:active` 的狀態、邊界事件留給之後的 RFC。
14. **Q12：表單控制項的 `:read-write`、`:required`、`:optional` 存成 form state**（2026-10-13，4b 開始前）：加進 `dom_api` 的 form state，和 `:disabled` 等一起算、一起記錄改變；`:read-write` 本來就依 disabled（fieldset、legend 的規則已經在那裡）和各 input type 的 `readonly` 適用範圍，規則只寫在一處，和現有的表單 pseudo-class 一致。contenteditable 的 `:read-write` 在比對時往上找，判斷 editable 的程式搬進 `dom`，`dom_api` 和 `style` 共用。不選照 Chromium 全部在比對時算（`SelectorChecker` 問元素的 `MatchesReadWritePseudoClass`、`IsRequiredFormControl`）：`style` 不能依賴 `dom_api`，HTML 的控制項規則要搬進 `dom` 或寫兩份。效能上兩者都量不出差別（使用者問過）：A 比對時看 bit，B 改變時不用重算，會改變它們的都是屬性的改變，kun 本來就重算子樹；`ElementState` 的 `u16` 因此用滿。
15. **Q13：邏輯屬性是自己的 property，cascade 時對應成物理的**（2026-10-13，第 5 段開始前）：照 Blink（每個邏輯屬性是自己的 `CSSProperty`，`CSSDirectionAwareResolver` 在 cascade 時照 writing direction 對應）和 Stylo，30 個 longhand 和它們的 shorthand 加進 `css`，cascade 的勝出表照物理的那一格收，同一組照宣告順序勝負；`CssPropertyValueSet` 照 CSSOM 處理同一組的順序。`style.marginInlineStart`、`cssText`、`getComputedStyle` 讀得回邏輯的名稱，WPT 的 `css/css-logical/` 測得到；之後加 `writing-mode`、`direction` 只改對應的那一步。代價是 `properties.rs` 的每個 `match` 多出這些 property。不選解析時換成物理的（cascade 和 CSSOM 不用改，但 CSSOM 讀不回邏輯的名稱、和 Chrome 不同，解析時不知道元素的方向，加 `writing-mode` 要重做）。

## 待決定

沒有。

## 之後的可能

- `@property`、`CSS.registerProperty`（有型別、可以不繼承、可以做動畫的自訂 property）。
- `revert`、`revert-layer` 和 `@layer`。
- `@container` 和 container 的單位。
- 代換之後解析的結果的快取（相同的代換結果不重新解析）。
- `writing-mode`、`direction`：邏輯屬性照它們對應。
