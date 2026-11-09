# RFC 0039：style 的規則比對

- 開始日期：2026-10-12
- 狀態：✅ 已完成：2026-10-13（第 1 到 16 條；兩段，都在 Fedora 44 上做，第 1 段先 merge 回 `main`；原本的第 3 段（祖先的 bloom filter）照第 3 條不做；驗收和檢查在 `progress.md`）
- 細節：`details.md`

這份是 RFC 0012 的「之後的階段」說的第一份優化的 RFC：量出差距之後，針對最大的瓶頸另開一份。差距和瓶頸的量測在 `rfcs/0012-speedometer/progress.md` 的「優化前的重新量測：比較表、trace 和 `perf`（2026-10-12，Fedora 44）」。

## 摘要

讓 style 重算時「一個元素符合哪些規則」變便宜，不改重算的範圍。照 Blink 分兩段：元素的 `id` 和 `class` 在屬性改變時存成切好的，比對時不配置、不複製（Blink 的 `ElementData` 的 `ClassNames`）；規則照 selector 最右邊的 compound 分到 id、class、tag 和其他的桶，每個元素只比對它的桶裡的（Blink 的 `RuleSet` 和 `ElementRuleCollector`），document 自己的 sheet 合成一個 `RuleSet`（Stylo 的 `CascadeData`）。Complex-DOM 的 suite 的 `Document::UpdateStyle` 是原本的十三分之一到十六分之一（`progress.md`）。原本的第三段（祖先的 bloom filter，Blink 的 `SelectorFilter`）量過之後不做：剩下的時間不在 selector 的比對（第 3 條）。

## 動機

**現況（2026-10-12，`main` 的 `b58e7cb6`；Fedora 44，AMD Ryzen AI 9 HX 370）**：
- **Speedometer**：差距最大的五個 suite 是 TodoMVC-React-Complex-DOM（110.6 倍）、jQuery（36.6）、Svelte-Complex-DOM（18.1）、ES6-Webpack-Complex-DOM（16.4）、Preact-Complex-DOM（14.9）。renderer main thread 的時間幾乎都在 `Document::UpdateStyle`：React-Complex-DOM 3 個 iteration 33.1 秒裡 31.0 秒（Chrome 的 `Document::recalcStyle` 59 ms），Svelte-Complex-DOM 26.8 秒裡 25.1 秒，一次重算平均 387 ms（Chrome 3 個 iteration 全部的 `recalcStyle` 合計 62 ms）。
- **`perf`（Svelte-Complex-DOM）**：61.6% 在 `memcmp`，幾乎全部來自 `NodeRef::attribute` 用 `QualName` 找屬性（字串比較）；11.5% 在 `malloc`，`has_class` 每次建一個 `QualName::attr("class")`、複製 `class` 的值；`has_class` 和 `selectors` 的 `matches_complex_selector_internal` 自己 9%。
- **原因**：`style::cascade` 讓每個元素對每一條規則跑一次 `matches_selector`（`crates/style/src/lib.rs`）。Complex-DOM 的頁面約 6600 個元素、author 的 CSS 657 條規則（903 個 selector），再加上 UA stylesheet；整份重算一次是幾百萬次比對，每次比對 class 或 id 都配置一次、對每個屬性比一次字串。`crates/style/src/selector.rs` 的 `StyleElement` 的 `has_id`、`has_class` 經過 `NodeRef::attribute`（複製值），`attr_matches` 經過 `NodeRef::attributes`（複製整份屬性）。

目標（都達成，數字在 `progress.md`）：
- 重算一次的成本和「每個元素實際可能符合的規則數」成正比，不是和規則的總數成正比。
- 比對 `id`、`class` 和屬性時不配置、不複製。
- 行為不變：每個元素的 computed style 和原本一樣（同樣的規則、同樣的 specificity 和順序），WPT 的結果不變。
- 量得到改善：每段做完，用 `ci.py speedometer --trace` 量 Complex-DOM 的 suite。

不在範圍內：
- **重算的範圍**（只重算受影響的元素：Blink 的 invalidation set，`docs/limitations.md` 的「Invalidation」第一條）：另一份 RFC。
- **style sharing 和 matched properties 的快取**（Blink 的 `MatchedPropertiesCache`）、`style` attribute 每次重新解析：做完之後剩下的時間主要在這一類（`details.md` 的「其他看到的成本」）。
- **屬性的名稱全部改成 atom**（`dom` 的 `QualName` 用 interned string，Blink 的 `QualifiedName`）：牽動 `dom` 到 `dom_js` 一千多處；這份只讓 style 的路徑不比字串（第 1 條）。
- **live collection 的平方成本**（jQuery 的另一半：`collection[i]` 每次複製整份清單，Blink 的 `CollectionIndexCache`）、**大頁面的載入要好幾秒**（Speedometer 不計時）：各自另外做。

## 設計概要

對網頁的作者和 kun 的使用者，行為不變，只是快。對改 `style` 的人：

- 每個 selector 放進一個桶（`css` 的 `RuleSet`），看的是最右邊的 compound：`#main .item > a` 進 tag 桶的 `a`（`#main` 在左邊，不算）；`.todo-list li.completed` 進 class 桶的 `completed`；`[hidden]`、`*`、`:hover`、`:is(.a)` 這種進「其他」的桶。
- `RuleSet` 有三種：UA stylesheet 的建一次、所有 document 共用；document 自己的 `<style>` 和 `<link>` 照順序合成一個，sheet 變了時重建；adopted 的 `StyleSheetContents` 第一次被 cascade 時建、存在它裡面，adopt 它的 document 共用。
- 重算一個元素時，只拿它的 id、每個 class、名稱和「其他」的桶裡的 selector 出來比對（`RuleCollector`），照規則在 cascade 裡的順序合併，同一條規則取符合的 selector 裡最大的 specificity。
- 元素的 `id` 和切好的 `class` 存在 `dom` 的元素上，屬性改變時一起更新；比對時借用，不複製。

## 詳細設計

### 第 1 段：比對時不配置、不複製

- **`dom`**：元素的狀態（`NodeState::Element`）多存 `id`（`Option<Box<str>>`）和 `class` 切好的清單（`Box<[Box<str>]>`，照 HTML 的 ordered set parser：ASCII 空白切開、去掉重複；第 4、7 條），建立元素時和 `crates/dom/src/scope.rs` 的 `attributes_changed`（設定、移除、替換屬性三條路最後都經過它）裡、名稱是 null namespace 的 `id` 或 `class` 時更新（第 5 條；`QualName::is_attr` 不建名稱就比對）。對外是借用的讀法：`NodeRef::id_for_style()`、`class_names()`、`attributes_ref()` 回傳 `std::cell::Ref`（第 6 條），`attribute()`、`attributes()` 照舊複製。讀的 scope 裡也能寫元素自己的狀態（`set_computed_style`），寫之前要放掉這些 `Ref`。Blink 的 `ElementData` 的 `IdForStyleResolution()`、`ClassNames()`（`SpaceSplitString`），`Element::AttributeChanged` 裡更新。
- **`style`**：`StyleElement::has_id`、`has_class`、`attr_matches`、`:link` 的 `href` 讀上面那些。quirks mode 的 id 和 class 照舊是 ASCII 不分大小寫，比對時處理，存的值保持原樣（第 8 條）。

### 第 2 段：照最右邊的 compound 分桶

- **`RuleSet`**（`crates/css/src/rule_set.rs`，第 11 條）：每個 selector 一筆 `RuleData`（規則在它的 set 裡的位置、selector 在 selector list 裡的位置、specificity），照最右邊的 compound 選一個桶：有 id 進 id 桶；否則有 class 進 class 桶（取第一個；Blink 取最後一個，選哪個只影響候選數）；否則有 tag 進 tag 桶；否則進「其他」。`:is()`、`:where()`、`:not()`、`:has()` 裡的東西不算（`:not(.a)` 符合的是沒有 `a` 的），namespace 限制只影響比對（`details.md` 的「分桶的規則」）。
  - id 和 class 的 key 一律存 ASCII 小寫，查的時候元素的也轉小寫（有大寫字母時才配置）：同一份 sheet 可能同時用在 quirks 和 standards 的 document（第 12 條）。
  - tag 存寫的和小寫的兩份（不同時），查的時候用元素自己的名稱：HTML 文件裡的 HTML 元素照 `selectors` 比小寫的名稱，其他元素（SVG 的 `clipPath`）比寫的（第 13 條）。
  - 雜湊是標準庫的 `HashMap`（第 14 條）。
  - 桶只負責縮小範圍：拿出來的每個 selector 照樣完整比對，桶可以多給、不能漏。
- **三種 `RuleSet`**（第 11 條）：UA 的和 sheet 一起建一次（`UserAgentStyleSheet`）；document 的 `<style>`、`<link>` 合成一個（`AuthorSheets` 的 `rule_set` 和 `rule_index`：合起來的第幾條是哪份 sheet 的第幾條），在 sheet 變了、重新解析時重建；adopted 的 `StyleSheetContents::rule_set`（`OnceLock`，內容不可變）。原本 document 的每份 sheet 各一個，Complex-DOM 的頁面有 420 個 `<style>`，每個元素的每個 class 要在每份 sheet 裡各查一次，量到時間都在查桶，所以合成一個。
- **`RuleCollector`**（`crates/style/src/lib.rs`，第 16 條；Blink 的 `ElementRuleCollector`）：對每個 `RuleSet` 收元素的候選，排序、去重（只差大小寫的 class、兩個桶給的同一個 selector），同一條規則的 selector 一起比對、取最大的 specificity，交給 `cascade` 的是規則在整個 cascade 裡的順序（set 的第一條的順序加上它在 set 裡的位置）。收候選的 `Vec` 在一次重算裡重複使用。
- **測試用的開關**（第 15 條）：`StyleEngine::set_linear_matching_for_testing` 讓每個元素比對每個 selector（候選是每份 sheet 的全部，後面的路一樣）；`matched_selectors_for_testing` 回傳完整比對過的 selector 數。

### 沒做的第 3 段：祖先的 bloom filter

原本的計畫：建 `RuleSet` 時算每個 selector 的 `selectors::parser::AncestorHashes`，重算往下走時維護祖先的 tag、id、class 的 counting bloom filter，比對前用 `selector_may_match` 篩掉 descendant 和 child combinator 不可能成立的（Blink 的 `SelectorFilter`），`CssIdent` 的 `precomputed_hash` 改成真的 hash（第 9 條）。第 2 段做完之後量到 Svelte-Complex-DOM 一次重算約 30 ms，`perf` 的 main thread 分散在 `cascade` 本身、套用和複製 property 的值、候選的排序，`selectors` 的比對只佔幾個百分點，bloom filter 只能省其中的一部分，所以不做（第 3 條）。`CssIdent` 的 `precomputed_hash` 照舊是 0，`crates/css/src/selector.rs` 寫明只有 bloom filter 用它。

### 測試

- **`dom`**：`cargo test -p dom --test it style_data::`：建立時就有、設定、替換、移除、`set_attr_node` 之後跟著變；ASCII 空白、去掉重複；只算 null namespace；其他節點沒有；借用的屬性照順序。
- **行為不變**：`cargo test -p style --test it matching::the_buckets_give_the_same_styles` 用 standards、quirks mode 和 adopted sheet 的文件比較分桶和逐條比對的每個元素的 computed style，也斷言值本身（後面的 sheet 贏、adopted 的排在後面、規則取最大的 specificity：這些同時改兩條路，比較兩條路看不出來）。`cascade::` 補了 quirks mode 的 id 和帶 namespace 的屬性 selector。
- **真的變少**：`cargo test -p style --test it matching::few_selectors_are_matched` 用計數：一千條規則裡只比對自己桶裡的（class、id、最右邊的 compound、tag、class 的大小寫）、同一個 selector 只比對一次、「其他」對每個元素都比對。分桶的 mutation（全部丟進「其他」）只有它抓得到。
- **WPT**：`test.py wpt css/selectors/ css/css-cascade/ css/cssom/ dom/nodes/` 的預期結果不變。
- **效能**：沒有自動的效能測試；每段做完用 `ci.py speedometer --trace` 量，記在 `progress.md`（`docs/test-gaps.md` 的「測試守不住的地方」）。

### 分階段

兩段，各自量得到改善：

1. 比對時不配置、不複製（`dom` 的 id 和 class、`StyleElement` 借用）。做完先 merge 回 `main`。
2. `RuleSet` 分桶、document 的 sheet 合成一個：等 RFC 0016 的第 3 段 merge 回 `main` 之後才開始（第 10 條）。

原本的第 3 段不做（第 3 條）。

## 缺點

- `dom` 的每個元素多存 id 和 class 的副本（記憶體多一點；Blink 也是）。屬性改變的路徑要記得更新，漏掉時 style 用舊的值：靠 `dom` 的測試涵蓋每一條改屬性的路。
- `style` 多一個 `RuleSet` 的資料結構；比對的結果和逐條比對的必須一樣，否則是很難發現的 style bug：靠和逐條比對的比較測試。
- 優化的 mutation（全部進「其他」的桶）在行為上等價，只有計數的測試抓得到。
- document 的 sheet 合成一個：任何一份 `<style>` 改了，整個 set 重建（sheet 本來就整份重新解析，成本同一個量級）。

## 理由和其他做法

- **為什麼先做比對、不先做 invalidation**：一次整份重算 387 ms，Chrome 整份才幾 ms；就算重算的範圍縮小到十分之一，比對的成本照樣比 Chrome 高一兩個數量級。比對變便宜之後，invalidation 的收益才量得準。Blink 兩者都有，這份先做底層的那個。
- **為什麼不用 Stylo 的 `SelectorMap`**：Stylo 的 `style` crate 和 Gecko / Servo 的 DOM、rule tree 綁在一起，不單獨發佈（`docs/architecture.md` 的第三方選型：Stylo 不用）。分桶自己做，對照 Blink 的 `RuleSet`。
- **第 1 段的選項**（第 1 條）：Q1 只為 style 存 id 和 class；Q2 屬性的名稱全部改成 atom。Q2 是 Blink 的全貌，但牽動一千多處、和這份的目標（style 的路徑）只有部分重疊。
- **document 的 sheet 合成一個**（第 11 條）：每份 sheet 各一個時，查桶的次數乘上 sheet 的數目；換更快的雜湊（FxHash）試過只少 5%。
- **不做 bloom filter**（第 3 條）：見「沒做的第 3 段：祖先的 bloom filter」。
- 完整的比較在 `details.md` 的「做法的比較」。

## 先例

- **Blink**：`RuleSet`（`id_rules_`、`class_rules_`、`tag_rules_`、`attr_rules_`、`universal_rules_` 等桶，`RuleSet::FindBestRuleSetAndAdd` 選桶）、`StyleSheetContents::EnsureRuleSet`、`ElementRuleCollector`、`SelectorFilter`（祖先的 bloom filter）、`ElementData::ClassNames()`（`SpaceSplitString`，屬性改變時更新）、`QualifiedName` 和 `AtomicString`。照做的：分桶、adopted 的內容帶著自己的 `RuleSet`、存切好的 class；不照做的：全面的 atom、`attr_rules_` 這個桶（第 2 條）、`SelectorFilter`（第 3 條）。
- **Stylo**：`SelectorMap`（id、class、local name、namespace、其他）、每個 origin 一份 `CascadeData`（document 的 sheet 合成一個照它）、quirks mode 的不分大小寫的 map（kun 的 `RuleSet` 是 sheet 共用的，改成 key 一律存小寫，第 12 條）。
- 細節在 `details.md` 的「先例」。

## 已決定的事項

1. **Q1：第 1 段只為 style 存 id 和切好的 class，屬性的名稱不改成 atom**（2026-10-12）：元素存 `id` 和 `class` 的切好的版本（`Box<str>`），`StyleElement` 借用，`attr_matches` 不複製屬性的清單。不選 Q2（`QualName` 改成 atom，連同這一段）：atom 長期是比較好的設計（Blink、Stylo 都是），但牽動一千多處；它取代不了 Q1（atom 只讓「找到 `class`」變便宜，值照樣要切開，Blink 也是 `SpaceSplitString` 加 `AtomicString` 兩者都有）；做完 Q1 和分桶之後 style 的路徑上只剩屬性的 selector 比字串，atom 對這份要量的 style 時間收益不明，收益多半在 parser 和 DOM API。Q1 的成果之後不白做：改成 atom 時切好的 class 只是元素的型別換掉。atom 記在「之後的可能」，另一份 RFC。不選 Q3（只改 `StyleElement`、就地切）：分桶時每個元素照樣要切一次 `class`。
2. **屬性的 selector 先不分桶**（2026-10-12）：`[type=checkbox]` 這種最右邊只有屬性的 selector 放「其他」的桶。Blink 有 `attr_rules_`，Stylo 沒有；「其他」的候選多到值得時再加。
3. **不做祖先的 bloom filter**（2026-10-12 原本是「第 3 段照計畫做，第 2 段的量測出來時再確認」；2026-10-13 確認後改成不做）：第 2 段之後 Svelte-Complex-DOM 一次重算約 30 ms，`perf` 的 main thread 分散在 `cascade` 本身、套用和複製 property 的值、候選的排序，`selectors` 的比對只佔幾個百分點，bloom filter 只能省其中 descendant combinator 不成立的部分。剩下的成本要靠 matched properties 的快取或 invalidation（「之後的可能」）。之後的量測顯示比對又變成大頭時再考慮。
4. **id 和 class 放在元素的狀態裡**（2026-10-12，第 1 段開始前）：`NodeState::Element` 多兩個欄位，每個元素多 32 bytes。Blink 的 `ElementData` 也是每個有屬性的元素都有。不放 rare data：Complex-DOM 幾乎每個元素都有 class，省不到記憶體，讀的時候多一層。
5. **在 `attributes_changed` 和建立元素時更新**（同上）：三條改屬性的路最後都經過它，`cloneNode` 和 parser 經過建立元素。Blink 的 `Element::AttributeChanged` → `ClassAttributeChanged`。不選第一次讀時才算：style 重算在讀的 scope 裡，要多一層 interior mutability。
6. **`dom` 回傳借用**（同上）：`id_for_style()`、`class_names()`、`attributes_ref()` 回傳 `Ref::map` 過的 `std::cell::Ref`，元素的狀態本來就是 `RefCell`，不用 `unsafe`；`attribute()`、`attributes()` 照舊複製，語意不變。
7. **class 去掉重複**（同上）：照 HTML 的 ordered set parser 和 Blink 的 `SpaceSplitString`，分桶時同一個 class 不查兩次。
8. **quirks mode 存原本的值**（同上）：比對時照舊 ASCII 不分大小寫；分桶的 key 見第 12 條。
9. **`CssIdent` 的 hash 不做**（2026-10-12 從第 1 段移到第 3 段：第 1 段沒有東西用它，弄壞了沒有測試抓得到；2026-10-13 隨著第 3 條不做）。
10. **第 2 段等 RFC 0016 的第 3 段 merge 回 `main`**（2026-10-12，第 1 段開始前）：`origin/0016` 的 3a、3a' 把 stylesheet 和規則搬到新的 `css` crate（`StyleSheetContents`）、為 `adoptedStyleSheets` 改寫了 cascade，正是第 2 段要改的地方；規則在 `StyleSheetContents` 裡共用之後，`RuleSet` 照 Blink（`StyleSheetContents::EnsureRuleSet`）建在它上面更好。第 1 段的 `StyleElement` 留在 `style`，rebase 的衝突小，先做。
11. **`RuleSet` 的型別在 `css`；document 自己的 sheet 合成一個，UA 和 adopted 的各一個**（2026-10-12，第 2 段開始前；2026-10-13 改）：UA 的建一次、共用；document 的 `<style>`、`<link>` 照順序合成一個，sheet 改變時（本來就整份重新解析）重建；adopted 的 `StyleSheetContents`（不可變）第一次用到時建、存在它裡面（`OnceLock`），document 之間共用（Blink 的 `StyleSheetContents::EnsureRuleSet`）。原本 document 的 sheet 也是每份一個：做了之後量到 Svelte-Complex-DOM 的重算只少了約四分之一，`perf` 的時間在查桶（雜湊表的 cache miss），因為它的頁面有 420 個 `<style>`，每個元素的每個 class 在每份 sheet 裡各查一次（每次重算上千萬次）；把 SipHash 換成 FxHash 試過只少 5%，次數才是原因。合成一個是 Stylo 的做法（每個 origin 一份 `CascadeData`）。不選 stylesheet 改變時連 UA 和 adopted 整份合起來重建：共用的 adopted sheet 每份 document 各建一次，UA 的也每次重建。
12. **id 和 class 的 key 一律存 ASCII 小寫，查的時候元素的也轉小寫**（2026-10-12，第 2 段開始前）：同一份 sheet 可能同時用在 quirks 和 standards 的 document，key 不能看 document 的模式；standards mode 下 `.Foo` 和 `.foo` 進同一個桶，多的候選由完整的比對排除。元素的 class 或 id 有大寫時才配置（Complex-DOM 的 class 大多有大寫，合成一個 `RuleSet` 之後這不是主要的成本）。Stylo 的 `SelectorMap` 每份 document 一個，quirks mode 改用不分大小寫的 map，kun 的 `RuleSet` 是 sheet 共用的，不能這樣做。
13. **tag 的桶照 Stylo**（同上）：selector 的 `clipPath` 用小寫和原本的大小寫各存一份（不同時），查的時候用元素自己的名稱，不轉小寫、不配置。
14. **標準庫的 `HashMap`**（同上）：不加新的 dependency。2026-10-13 量過：每份 sheet 一個 `RuleSet` 時換成 FxHash 只少 5%，合成一個之後查桶的次數少了幾百倍，更不重要。改成 atom 時用 atom 帶著的 hash。
15. **測試用 `StyleEngine` 的兩個 `_for_testing` 方法**（同上）：`set_linear_matching_for_testing` 切回逐條比對，整合測試對同一份文件比較兩條路的 computed style；`matched_selectors_for_testing` 回傳做了完整比對的 selector 數。命名照 `dom` 的 `new_detached_for_testing`。不選 crate 內的單元測試：拿不到 parser 做的文件，涵蓋的情況少。
16. **候選照 cascade 的順序收集，同一條規則取最大的 specificity**（同上）：id、每個 class、tag 和「其他」的桶，排序、去重後合併，結果和逐條比對一樣；收集用的 `Vec` 在一次重算裡重複使用。Blink 的 `ElementRuleCollector`。

## 待決定

無。

## 之後的可能

- **matched properties 的快取和 style sharing**（Blink 的 `MatchedPropertiesCache`）、`style` attribute 解析好的快取（Blink 存在 `ElementData` 的 inline style）：這份做完之後，Complex-DOM 的重算剩下的時間主要在 `cascade` 本身、套用和複製 property 的值。
- **style 的 invalidation**（Blink 的 invalidation set、`StyleRecalcChange`）：只重算受影響的元素，`docs/limitations.md` 的「Invalidation」第一條。
- **祖先的 bloom filter**（Blink 的 `SelectorFilter`，連同 `CssIdent` 真的 hash）：之後的量測顯示 selector 的比對又變成大頭時（第 3 條）。
- **屬性的名稱和 class、id 改成 atom**（第 1 條的 Q2，Blink 的 `QualifiedName` / `AtomicString`、Stylo 的 `Atom`）：另一份 RFC。內容：
  - `dom` 的 `QualName`、`Namespace` 改用 interned string，比較是指標；已知的名稱（844 處寫成 `QualName::attr("class")` 這種字面的名稱）是靜態 atom，像 Blink 的 `html_names::kClassAttr`，不再每次配置。
  - 2026-10-12 的看法：用 `string_cache`（markup5ever 的 `LocalName`、`Namespace`、`Prefix`，html5ever 已經用它，HTML、SVG 的名稱有靜態的 atom），`html` 的 parser 直接把 html5ever 的 atom 放進 DOM，不再轉成 `Box<str>`。要決定的：atom 表放在哪裡（`string_cache` 是整個 process 一張表，動態的 atom 建立時拿鎖、複製和丟掉時是 atomic 的參考計數；Blink 每個 thread 一張 `AtomicStringTable`，kun 的多個 renderer 是同一個 process 的 thread），要量 `data-*` 和 custom element 這種動態名稱的成本。
  - 元素存的切好的 class 和 id 也改成 atom，帶著算好的 hash：分桶的查詢（和之後的 bloom filter）不必每次算 hash、不必轉小寫配置。
  - 收益要量的地方：parser（載入大頁面）、`getAttribute` 和 reflection、`querySelector`、layout 和 paint 讀屬性、記憶體（每個元素的每個屬性名稱現在各一份字串）。
- **live collection 的 index 快取**（Blink 的 `CollectionIndexCache`）：jQuery 的另一半。
- **大頁面的載入**：Complex-DOM 的頁面每次載入要好幾秒，剩下的是 parser、style、layout 還是別的，還沒量。
