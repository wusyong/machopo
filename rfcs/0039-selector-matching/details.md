# RFC 0039 的細節

## 量測

2026-10-12，`main` 的 `b58e7cb6`，Fedora 44，AMD Ryzen AI 9 HX 370（24 核），接電源；Chrome for Testing 152.0.7977.82。指令和完整的比較表在 `rfcs/0012-speedometer/progress.md` 的「優化前的重新量測：比較表、trace 和 `perf`（2026-10-12，Fedora 44）」。

### trace：renderer main thread 的 self time

`ci.py speedometer --trace --iterations 3`，kun 的 `MACHOPO_TRACE` 和 Chrome 的 Perfetto trace，取 `CrRendererMain`：

| Suite | kun 的總時間 | kun 的 `Document::UpdateStyle` | Chrome 的 `Document::recalcStyle` |
| --- | --- | --- | --- |
| TodoMVC-React-Complex-DOM | 33.1 秒 | 31.0 秒（362 次，平均 86 ms） | 59 ms（48 次） |
| TodoMVC-Svelte-Complex-DOM | 26.8 秒 | 25.1 秒（65 次，平均 387 ms） | 62 ms（50 次） |
| TodoMVC-React-Redux | 5.2 秒 | 4.5 秒（655 次） | 14 ms（45 次） |
| TodoMVC-jQuery | 20.1 秒 | 6.2 秒（994 次） | 15 ms（76 次） |

jQuery 另外有 11.6 秒在 `FunctionCall` 的 self（JS 加上沒有 span 的 DOM API），`perf` 指向 live collection（「不在範圍內」）。

### `perf`：Svelte-Complex-DOM

`perf record -F 199 --call-graph dwarf -p <kun>`，1 個 iteration（含載入頁面）：

| 自己的時間 | 函式 | 從哪裡來 |
| --- | --- | --- |
| 61.6% | `__memcmp_evex_movbe` | 59.3% 是 `NodeRef::attribute` 裡 `QualName` 的 `==`（`local` 和 `namespace` 都是 `Box<str>`） |
| 11.5% | `malloc` | 10.1% 是 `has_class` 的 `QualName::attr("class")`（`Box<str>` 的配置）；值的 `clone` 也在這裡 |
| 4.6% | `matches_complex_selector_internal` | `selectors` 的比對本身 |
| 4.4% | `has_class` | 切 `class` 的值 |
| 2.4% | `style::cascade` | |

### 頁面的大小

Complex-DOM 的 suite 共用 `big-dom-generator` 產生的頁面（以 `svelte-complex/dist` 為例）：`index.html` 約 6600 個元素；`big-dom-with-stacking-context-scrollable.css` 300 KB、657 條規則、903 個 selector，其中 40 個 `:first-child` 這一類、6 個 `:nth-*`、97 個兄弟 combinator、2 個 `:empty`，沒有 `:has()`；`app.css` 73 條規則。

### 估計

現在每個元素比對每個 selector：約 6600 × (903 + UA 的) 次。Complex-DOM 的 CSS 是 Spectrum 的元件樣式，幾乎每條規則最右邊都有 class（`.spectrum-Calendar-date` 這種），分桶之後一個元素的候選是它自己的幾個 class 的桶加上 tag 和「其他」，估計從上千降到幾十以下；再去掉配置和字串比較。估計一次整份重算從 387 ms 降一到兩個數量級，以每段的實測為準。

## 分桶的規則

照 Blink 的 `RuleSet::FindBestRuleSetAndAdd`，從最右邊的 compound 抽出 id、class、tag，依「越少元素符合越好」選一個：

1. 有 id：id 桶。
2. 否則有 class：class 桶（Blink 取最後一個看到的；選哪一個不影響正確性，只影響候選數）。
3. 否則有 tag：tag 桶。`selectors` 對 HTML 文件裡的 HTML 元素比 selector 的小寫名稱（`LocalName` 的 `lower_name`），對其他元素比寫的名稱（`name`，SVG 的 `clipPath`）：照 Stylo，兩個都當 key 存（相同時一份），查的時候用元素自己的名稱，不轉小寫（第 13 條）。
4. 否則：「其他」。

要保守的情況（放進「其他」或照 Blink 的特例）：
- 最右邊的 compound 只有 `:is()` / `:where()`：Blink 在只有一個參數時看進去，否則放進 universal。先一律放「其他」，量了再說。
- `:not(.a)`：不能當成 class `a` 的桶（不符合的元素才符合），忽略 `:not()` 裡的東西。
- `:has()`：`:has()` 的參數不影響分桶，照最右邊 compound 的其他部分。
- pseudo-element：kun 還不解析 pseudo-element（`css` 的 `PseudoElement` 是空的），之後加時照前面的 compound 分桶。
- namespace 限制（`svg|a`）：只影響比對，不影響分桶。
- quirks mode：id 和 class 是 ASCII 不分大小寫。同一份 sheet 可能同時用在兩種模式的 document，所以 key 一律存 ASCII 小寫、查的時候也轉小寫，standards mode 多出來的候選由完整的比對排除（第 12 條）。

候選的順序：桶裡存規則在 cascade 裡的位置（UA 在前，author 依 sheet 和規則的順序），收集後排序、同一條規則的多個 selector 合併成最大的 specificity，和現在的 `cascade` 的結果一樣。

## 做法的比較

### 第 1 段

| 做法 | 內容 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **Q1（第 1 條）** | `dom` 的元素多存 id 和切好的 class；`StyleElement` 借用；`attr_matches` 不複製 | 只動 `dom` 的三處改屬性的地方和 `style`；直接去掉 `perf` 看到的配置和大部分字串比較 | 其他路徑（`getAttribute`、selector API）照舊比字串 |
| Q2 | `QualName` 的 `local`、`namespace` 改成 interned string（atom），比較是指標 | Blink 的全貌，所有找屬性的地方都便宜 | 牽動 `dom`、`dom_api`、`dom_js`、`html`、`layout`、`paint` 一千多處；atom 表的生命週期（多個 renderer、多個 heap）要設計；和這份的目標只有部分重疊 |
| Q3 | 只改 `StyleElement`：比對時借用 `class` 的值、就地切開，不存 | 最小 | 每次比對照樣切字串、找屬性；分桶要元素的 class 清單時每個元素切一次 |

### 第 2 段

| 做法 | 內容 | 結論 |
| --- | --- | --- |
| **自己做 `RuleSet`（看法）** | 照 Blink 的桶，`style` 的新模組 | 和 kun 照 Blink 分層一致；規模小 |
| Stylo 的 `SelectorMap` | `style` crate 的一部分 | 不單獨發佈，和 Gecko / Servo 的 DOM、rule tree 綁在一起，`docs/architecture.md` 已經決定不用 Stylo |
| 不分桶，只做 bloom filter | 第 3 段單獨做 | bloom filter 只篩 combinator 左邊；最右邊是 class 的 selector 照樣每個元素比對一次，Complex-DOM 的 CSS 幾乎都是這種 |

### 第 3 段

`selectors` crate 已經有 `BloomFilter`（counting，可以減）、`AncestorHashes::new(selector, quirks_mode)`、`selector_may_match`；kun 的 `StyleElement::add_element_unique_hashes` 現在什麼都不加。照 Blink 的 `SelectorFilter`：走訪時 push / pop 祖先的 hash。

## 其他看到的成本

這份不處理，比對變便宜之後再量：
- `cascade` 每次都重新解析元素的 `style` attribute（`CssPropertyValueSet::parse_logged`）；Blink 把解析好的 inline style 存在元素上。
- `recalc_style` 每次都收集所有 stylesheet 的文字（`style_sheet_texts`）和上次的比較，判斷 stylesheet 有沒有變；Blink 由 stylesheet 的變更通知。
- 沒有 matched properties 的快取（Blink 的 `MatchedPropertiesCache`：同樣的規則組合直接拿上次算好的）、沒有 style sharing。
- 重算的範圍（invalidation）。

## 先例

### Blink

- `third_party/blink/renderer/core/css/rule_set.h`：`RuleSet` 的桶（`id_rules_`、`class_rules_`、`attr_rules_`、`tag_rules_`、`universal_rules_`，以及 link、focus、`::cue`、`::slotted`、`:host`、`::part` 這些特例），`RuleData` 存 selector、位置、specificity 和 `descendant_selector_identifier_hashes_`（bloom filter 用）。
- `third_party/blink/renderer/core/css/element_rule_collector.cc`：`CollectMatchingRules` 依元素的 id、每個 class、屬性、tag 拿出桶裡的規則，最後排序。
- `third_party/blink/renderer/core/css/selector_filter.h`：祖先的 bloom filter，`PushParent` / `PopParent`。
- `third_party/blink/renderer/core/dom/element_data.h`：`ClassNames()`（`SpaceSplitString`）、`IdForStyleResolution()`，屬性改變時（`Element::AttributeChanged` → `ClassAttributeChanged`）更新。

### Stylo

`servo/components/style/selector_map.rs` 的 `SelectorMap`：id、class、local name、namespace、attribute（較新的版本）、其他；bloom filter 用 `selectors` 的 `BloomFilter` 和 `AncestorHashes`（kun 用的同一個 crate）。
