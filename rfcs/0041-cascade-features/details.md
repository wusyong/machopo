# RFC 0041 的細節

## Speedometer 的 CSS（2026-10-13）

`../third_party/speedometer/resources/todomvc/big-dom-generator/dist/big-dom.css`：Complex-DOM 系列共用的 CSS（Adobe Spectrum），300 KB、637 條規則，用簡單的 regex 數的：

| 項目 | 數量 |
| --- | --- |
| `var()` | 2399 |
| `calc()` | 100，其中 92 個裡面有 `var()` |
| 值裡有 `var()` 的 shorthand（`margin`、`padding`、`border`、`background`、`font`、`inset`、`border-radius`） | 54 |
| `var()` 的 fallback 裡又有 `var()` | 0 |
| `@media` | 18 個，都是 `(-ms-high-contrast…)` 和 `(forced-colors: active)`，在 Chrome 裡也不成立 |
| 宣告自訂 property 的 selector | 63 個 |

自訂 property 宣告在哪裡：`.spectrum` 一個 selector 600 個，主題的 `.spectrum--light` 309 個、`.spectrum--medium` 303 個（加上授權註解後面那兩個大區塊，各 300 多個）；元件的 class 各自再宣告一批，例如 `.spectrum-Tag` 46 個、`.spectrum-Switch` 34 個、`.spectrum-Stepper` 32 個、`.spectrum-Button` 23 個、`.spectrum-Checkbox` 21 個。也就是：上面一個元素宣告了上千個，往下每個元件的元素再改幾十個。

用到的 pseudo-class：`:active`、`:checked`、`:disabled`、`:first-child`、`:focus`、`:focus-visible`、`:hover`、`:lang`、`:last-child`、`:not`、`:read-only`。用到的邏輯屬性 20 種：`block-size`、`inline-size`、`margin-block(-start|-end)`、`margin-inline(-start|-end)`、`padding-block(-start|-end)`、`padding-inline(-start|-end)`、`inset-block-start` / `-end`、`inset-inline-start` / `-end`、`border-block-start`、`border-inline-end`。

## 第 1 段的選項

### Q1：自訂 property 在 computed style 裡怎麼存

沒有 `@property` 時，所有自訂 property 都繼承，computed value 是代換過的 token 序列。

- **V1：一個 copy-on-write 的 map**（`Option<Arc<map>>`）：沒有宣告自訂 property 的元素和父元素共用同一個 `Arc`；宣告了的，複製父元素的整個 map、改掉自己的幾個。簡單。代價：Spectrum 的形狀下，每個宣告了元件變數的元素（`.spectrum-Tag` 等，頁面上很多個）複製一份上千個項目的 map。
- **V2：分成「根」和「自己的差異」**：一份 style 的變數是一個 `Arc` 指向「根」的那份（第一個宣告了變數的祖先），加上從根往下累積的差異；查詢先看差異、再看根。這是 Blink 以前的 `StyleInheritedVariables` 的 `root_`；Chromium 現在的原始碼已經沒有它（見 V3）。Stylo 的 `CustomPropertiesMap` 是同一類的一般化：一串 `Arc` 的層，自己這層超過 8 個項目、而且上面不到 4 層時才另開一層，否則照 copy-on-write 複製自己這層。
- **V3：persistent 的 hash array-mapped trie**：插入時只複製從改到的節點到根的路徑，任何形狀都是對數的成本。Blink 現在的做法：`StyleVariables` 的 `HashTrieNode`（`third_party/blink/renderer/core/style/style_variables.h`），16 個槽、key 每 4 bit 選一個槽，節點共用時改動要複製節點和它的祖先；整個 map 帶所有 key、value 的 hash 的 XOR，比較時先比它；不支援刪除，「invalid at computed-value time」存成空值。可以自己寫或用 `imbl`、`rpds` 這類 crate。

2026-10-13 先決定 V2，對照 Chromium 的原始碼時發現 Blink 已經改用 V3，同一天重新決定 V3、自己寫（`README.md` 的「已決定的事項」第 2 條）。V1 在 Spectrum 上的複製量是元件元素數 × 上千；V2 和 V3 都對準這個形狀，V3 在任何形狀下都不會退回整份複製，也是 Blink 現在的選擇。自己寫是因為需要的只有查詢、插入、走訪和累加的 hash，幾百行，不必為它加依賴。

### Q2：cascade 怎麼套用宣告

現在：所有符合的宣告照優先順序排好，`font-size` 先套用，再依序套用其他的，後面的蓋掉前面的（`crates/style/src/lib.rs` 的 `cascade`）。

- **C1：照舊依序套用全部，遇到沒解析的值就代換**：改動最小。代價：被蓋掉的宣告也要代換、解析一次（Spectrum 的元件常常在 `:hover`、`.is-selected` 這種規則裡改同一個 property，前面那條白算）；自訂 property 互相參照時，要先把自訂 property 全部收集完才能代換，等於還是要先分出一個階段。
- **C2：照 Blink 的 `StyleCascade`，先找勝出的宣告再套用**：由優先順序最高的往低走（或照順序記下每個 property 最後一條），每個 property 只記勝出的那一條（Blink 的 `CascadeMap`，Stylo 的 cascade 也是由後往前、記下已經看過的 property）；然後自訂 property 先算（遞迴代換、偵測循環）、`font-size`、其他。每個 property 最多代換、解析一次；不含 `var()` 的宣告也少套用很多次。
- **C3：自訂 property 先依序算完，其他照舊依序套用，只有勝出的沒解析的值才代換**：C1 和 C2 的中間；還是要知道哪一條勝出，等於做了 C2 的一半。

2026-10-13 決定 C2（`README.md` 的「已決定的事項」第 3 條）。多出來的是一個「property → 勝出的宣告」的表（kun 的 longhand 是一個 enum，用陣列就好），換來每個 property 只處理一次；第 3 段以後（`@media` 只改變哪些規則符合、邏輯屬性要照宣告順序和物理的比勝負）也都建在它上面：邏輯屬性照 Blink 在這一步對應成物理的，就能照宣告順序決定勝負。

## 先例

- **Blink**：
  - `StyleCascade`：`Analyze` 收集符合的宣告，`CascadeMap` 記每個 property 勝出的 `CascadePriority`；`Apply` 先套用高優先的（`font-size` 等），再其他；`var()` 在 `ResolveSubstitutions` 裡遞迴代換，`CascadeResolver` 偵測循環。
  - `CSSUnparsedDeclarationValue`（含 `var()` 的 longhand）、`CSSPendingSubstitutionValue`（含 `var()` 的 shorthand 展開成的 longhand）、`CSSVariableData`（token 和原文）。
  - `StyleInheritedVariables` / `StyleNonInheritedVariables`，裡面是 `StyleVariables`（`HashTrieNode`）。
  - `CSSMathExpressionNode`、`CSSMathFunctionValue`、`CalculationValue`（`PixelsAndPercent` 或 expression）。
  - `MediaQueryEvaluator`、`MediaQuerySet`、`RuleSet` 記下用到的 media query 的結果，`StyleEngine::MediaQueryAffectingValueChanged` 在結果改變時重建；`MediaQueryMatcher`、`MediaQueryList`。
  - `Document::UpdateHoverActiveState`、`EventHandler` 的假滑鼠移動、`BoundaryEventDispatcher`。
  - 邏輯屬性是自己的 property，`CSSDirectionAwareResolver` 在 cascade 時照 writing direction 對應成物理的。
- **Stylo**：`custom_properties.rs`（`CustomPropertiesBuilder`、Tarjan 的循環偵測）、`custom_properties_map.rs`（一串 `Arc` 的層）、`PropertyDeclaration::WithVariables`、`CalcLengthPercentage`、`media_queries`、邏輯屬性在 cascade 時照 `WritingMode` 對應。
