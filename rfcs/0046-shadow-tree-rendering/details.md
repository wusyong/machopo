# RFC 0046 的細節：shadow tree 的 rendering

## 現況（2026-10-14）

- **`dom`**：shadow root 是 host 的 rare data 裡的節點，`NodeRef::shadow_root`、`host`、`is_shadow_root`；shadow root 沒有 parent（`parent()` 是 `None`），所以 `NodeRef::children`、`parent` 照 DOM 走，不進 shadow tree。slot 的分配（`slot.rs`）在問的時候算、以 shadow root 的 id 存在 heap 外，帶著算的時候的 `dom_tree_version`，DOM 一改就整個 shadow root 重算（`assigned_nodes`、`assigned_slot`）；RFC 0035 用它發 `slotchange`。
- **style 的 flag**：`mark_ancestors` 沿 `parent` 往上標 `child_needs_style_recalc`，到 shadow root 就停：shadow tree 裡的改動到不了 document。layout tree 的 flag（`mark_layout_ancestors`）一樣。
- **style**：`StyleEngine::recalc_style` 從 document 的子節點照 DOM 往下走（`node.children()`），parent style 是 DOM 的 parent 的；stylesheet 是 UA、document 樹裡的 `<style>` 和 `<link>`（`style_sheet_texts` 照 tree order 收，不進 shadow tree）、document 的 adopted sheet，全部合成一個 `RuleSet` 加上 adopted 的各一個，每個元素配對全部。shadow tree 裡的元素沒有 computed style。`ShadowRoot.adoptedStyleSheets` 存得住（`dom` 的 rare data，`dom_api` 的 `adopted_style_sheets`），但 renderer 只把 document 的交給 style engine。
- **選擇器**：`css::selector::SelectorParser` 沒有開 `parse_host`、`parse_slotted`、`parse_part`：`:host`、`::slotted()` 讓整條規則無效。`style::selector` 的 `Element` 實作 `parent_node_is_shadow_root`、`containing_shadow_host`、`is_html_slot_element`、`is_part` 都回答「沒有」。
- **layout**：`BoxTree::collect` 照 DOM 的子節點收（`parent.children()`），`walk` 照 DOM 的 flag 往下；host 畫它的 light DOM 子節點。
- **`display`**：沒有 `contents`（宣告無效）。UA stylesheet 沒有 `slot` 的規則（`slot` 是 `inline`，照舊畫它的子節點）。
- **幾何**：`document.elementFromPoint` 回答 hit test 打到的元素，只照 frame 往上換（`element_for_hit`），沒有 shadow DOM 的 retarget（現在 shadow tree 裡的元素打不到，所以還沒有差別）。

## Speedometer

- TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM 的元件都用 shadow root：`attachShadow`、`adoptedStyleSheets`（`new CSSStyleSheet`、`replaceSync`）、`:host`（RFC 0012 的盤點，2026-10-12 修正歸屬之後）。量測有效（DOM 的摘要經過 open shadow root），畫面是空的。
- Perf-Dashboard 也用 custom element 和 shadow root。

## 選項的比較

### flat tree 怎麼走（第 1 條）

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **F1 `dom` 的 iterator，每次照 slot 分配的快取走（建議）** | 和 Blink 的 `FlatTreeTraversal` 一樣是走法、不是資料；style、layout、counter 共用一個定義；slot 的分配已經有快取 | DOM 改了之後第一次問某個 shadow root 的分配時要整棵重算（RFC 0035 的做法）；巢狀的 slot（slot 分配到另一個 slot）要一層層展開 | 沒有 shadow root 的節點只多一次「有沒有 shadow root」的檢查（rare data 的指標）；成本小 |
| F2 建一份 flat tree 的結構（每個節點的 flat parent、children），DOM 改時維護 | 走的時候不用查表 | 每個 DOM 改動都要維護第二份樹，和 slot 的分配的時機糾纏；kun 沒有其他地方需要 | 每個節點多兩三個欄位、每次改動多一份工作；成本大 |

### `:host` 等的範圍（第 2 條）

| 選擇器 | Speedometer、一般網頁 | `selectors` crate | 建議 |
| --- | --- | --- | --- |
| `:host`、`:host(<compound>)` | Lit、WebComponents 的元件都用 | 有（`parse_host`） | 做 |
| `::slotted(<compound>)` | 常見 | 有（`parse_slotted`） | 做 |
| `::part()`、`exportparts` | 主題化用，Speedometer 的盤點沒看到 | 有（`parse_part`），但要每個元素的 part 名稱和往外傳的對應 | 之後 |
| `:host-context()` | 少；css-scoping 標成 at-risk，Firefox 沒有 | 沒有 | 不做 |

### shadow tree 的 stylesheet 怎麼存（第 3 條）

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **S1 每個 shadow root 一組（建議）**：`<style>` 和 adopted sheet，各自的 `RuleSet`；同樣的 `<style>` 文字共用 parse 和 `RuleSet`（以文字為 key 的快取），adopted sheet 本來就共用 contents | 和 Blink 的 `ScopedStyleResolver`、Stylo 的每個 shadow root 的 `CascadeData` 一樣；配對一個元素只看它那棵樹的規則，不會試別的樹的；一個 shadow root 的 sheet 變了只影響它的 host 的子樹 | style engine 要記每個 shadow root 的 sheet 和 `RuleSet`（以 shadow root 的 id，像 slot 的分配），收集它的 `<style>` 要走它的樹 | 一百個同樣的元件只 parse 一次；收集文字的成本和 shadow tree 的大小成正比，只在那個 shadow root 有改動時重收 |
| S2 全部合進一個 `RuleSet`，每條規則帶「屬於哪棵樹」 | 只有一個 `RuleSet` | 每個元素配對時要過濾別的樹的規則；同樣的元件的規則重複 N 份；任何一棵樹的 sheet 變了整個重建 | 元件多時 `RuleSet` 很大、重建很貴；不建議 |

### dirty flag 怎麼跨過 shadow root（第 4 條）

DOM 的改動要讓 style 和 box tree 的走訪走到它。flat tree 的 parent 和 DOM 的 parent 在兩個地方不同：shadow root 的子節點（flat parent 是 host）、分配到 slot 的節點（flat parent 是 slot，DOM parent 是 host）。

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **D1 shadow-including 的 DOM 祖先（建議）**：`mark_ancestors` 到 shadow root 之後接著標 host；style 和 box tree 的走訪到了 host，先走 shadow tree（照 flag），再處理有 flag 的 light DOM 子節點，它們的 parent style 是分配到的 slot 的（走 shadow tree 時記下每個 slot 的 style）；分配變了的節點標成要重算、要重建 | 標的時候不用知道 slot 的分配（mutation 的當下不必算）；只多「shadow root 往上接 host」一步 | 走訪在 host 要多一步：照 DOM 找有 flag 的 light 子節點、查它的 slot | 標記的成本不變；走訪多查一次分配的快取（只對有 flag 的 light 子節點） |
| D2 照 flat tree 的祖先標：light 子節點往上標它的 slot、slot 的祖先、shadow root、host | 走訪和標記走同一棵樹，走訪不用特別處理 host | mutation 的當下要知道 slot 的分配（`dom` 的 mutation scope 裡算），分配一改，之前標的路徑就不對 | 每次改動可能要算一次分配；分配的改動要先把舊的路徑也標過；成本中 |

Blink 的 style recalc 照 flat tree 走、slot 的分配改了時把受影響的節點標髒；這裡的 D1 走訪的順序一樣，只是標記走 DOM 的祖先（加上 shadow root 到 host），在 host 換回 flat tree。

### 沒有分配到 slot 的 light DOM 子節點的 style（第 5 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **U1 照 DOM 的 parent（host）算、沒有盒子（建議）** | `getComputedStyle` 讀得到值（和 `display: none` 下面的元素一樣）；走訪不用分兩種 | 多算一些不畫的元素的 style |
| U2 不算（Blink 在 style recalc 時跳過不在 flat tree 的元素，`getComputedStyle` 另外照需要算） | 少做工作 | kun 的 `getComputedStyle` 只讀存著的 style，要另外做「需要時才算」 |

### `display: contents` 只給 slot 還是一般的（第 6 條）

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **C1 一般的（建議）**：`css` 收 `contents`，box tree 的 `collect` 對任何 `display: contents` 的元素都不建盒子、把它的子節點和 `::before`、`::after` 收進父元素的內容；replaced element 和幾個 HTML 元素照 css-display 當 `none` | 和 Chrome 一樣；slot 只是 UA stylesheet 的一條規則；WPT 的 `css/css-display/` 有一百多個 `display-contents-*` | 多幾個邊界：沒有盒子的元素的幾何是 0、它的 `::before`、`::after` 的位置、局部更新時它的內容在父元素的容器裡 | 和 slot 用同一套程式，多的只是解析和那幾個邊界；成本小 |
| C2 只給 slot（box tree 認得 slot、不認 `display`） | 範圍小 | `slot { display: block }` 這種作者寫的樣式要另外處理；一般的 `display: contents` 照舊沒有，之後還是要做一次 | 省一點現在的工作，之後重做 |
