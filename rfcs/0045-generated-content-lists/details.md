# RFC 0045 的細節：生成內容和 list

## 現況（2026-10-14）

- **選擇器**：`css::selector::PseudoElement` 是空的 enum（「None are parsed yet」），`SelectorParser` 沒有覆寫 `parse_pseudo_element`：任何 pseudo-element 都在解析時失敗，整條規則丟掉（`stylesheet.rs` 記成「unsupported selector」）；`querySelector('x::before')` 是 `SyntaxError`。`style` 的配對裡 `is_pseudo_element()` 一律 `false`。
- **computed style**：存在 DOM 節點上（`NodeState::Element { computed_style }`），`StyleEngine::recalc_style` 照 `StyleFlags` 局部重算；值變了時 `set_style_changed()` 標 `LayoutTreeFlags`。
- **UA stylesheet**（`crates/style/resources/html.css`）：開頭寫「沒有 `list-item` 和 table 的 display：list 是 block、table 是行內」；`li` 是 `display: block`，`ol`、`ul` 有 `padding-left: 40px` 和上下 1em 的 margin。
- **`display`**：只有 `block`、`inline`、`flex`、`grid`、`inline-block`、`inline-flex`、`inline-grid`、`none`；其他的值（`list-item`、`table`）讓宣告無效。
- **`content`、counter、`list-style-*`、`quotes`**：都沒有，宣告當成不支援的 property 丟掉。
- **`getComputedStyle(element, pseudo)`**：認得十個 pseudo-element 的名稱，宣告標成 `for_pseudo_element`，讀回空的。
- **layout**：沒有 DOM 節點的盒子只有 `LayoutView` 和匿名 block（含 flex、grid 的匿名 item）；行內內容由 `collect` 照 DOM 的子節點收；文字 run 的 text 節點給 offset mapping（`TextSource`、`TextFragment::source`），`text_rects` 跳過沒有 `source` 的 run。唯一的生成文字是 `text-overflow` 的省略號（`node` 是它跟著的 run 的元素，`source` 是 `None`）。
- **hit test**：回答 `fragment.node`，`None` 時用外面最近的元素。
- **WPT**：`include.txt` 沒有 `css-pseudo`、`css-lists`、`css-content`；`floats-114`、css-variables 的五個、`CSS-supports-L4` 的預期結果寫著沒有 pseudo-element。

## Speedometer 的盤點

`rfcs/0012-speedometer/progress.md`：

- 解析不了的選擇器：Complex-DOM 各 69 到 71 個（`::before` 38、`::after` 10、`::placeholder` 10……），NewsSite 17 個（`::before`、`::after`、`:target`），CodeMirror 22 個（`::after` 16）。
- 最多 suite 丟掉的：選擇器是 `::before` / `::after`（15、16 個 suite），宣告有 `content`（16）、`list-style-type`（15）。
- `::marker`、counter、`quotes` 沒有出現在盤點裡（盤點只記解析不了的；`li` 現在是 block，`list-style-type` 被丟掉，所以 marker 也畫不出來）。

## 選項的比較

### pseudo-element 的 style 存在哪裡（第 1 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **P1 存在元素上（選這個）**：`NodeState::Element` 多 `pseudo_styles`（`::before`、`::after`、`::marker` 各一個 `Option<Arc<ComputedStyle>>`，放在一個 `Option<Box<…>>` 裡，沒有的元素只多一個指標） | 和元素的 style 一起算、一起比較、一起丟；layout 以 `(元素, PseudoId)` 識別，不用新的節點種類；Stylo 是這樣存的 | 之後要 pseudo-element 的物件（UI Events 的 `pseudoTarget`、`Element.pseudo()`）時要另外加 |
| P2 照 Blink 建 `PseudoElement`（`dom` 的 GC 物件，有 `NodeId`，不在樹裡） | 和 Blink 一樣，layout 的盒子有自己的 node；之後的 `pseudo()` API 直接有物件 | `dom` 要多一種不在樹裡的節點，生命週期跟著元素的 style（`content` 變 `none` 時要丟），所有走 DOM 的程式要確認不會走到它；hit test 照樣要轉回元素 |

### 盒子和 fragment 的識別（第 3 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **I1 `node` 是元素，多 `pseudo: Option<PseudoId>`（選這個）** | hit test、`text-overflow` 的省略號、paint 的 client 不用改（都回答元素）；Blink 的 pseudo-element 的 layout object 的 node 也是「屬於這個元素的」 | 找元素自己的盒子的地方（`element_box`、`client_rects`、paint 的 `element_bounds`、`elements` 這張表）要跳過 `pseudo` 是 `Some` 的 |
| I2 `node` 是 `None`，另外記元素 | 找元素的盒子的地方不用改 | hit test、paint 的 client、事件的 target 都要另外查元素，`None` 現在的意思（匿名 block）被混在一起 |

### outside marker（第 6 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **M1 照 Blink 的 `LayoutOutsideListMarker`（選這個）**：marker 排成一個 atomic inline 大小的盒子，不進斷行；list item 排好之後，放在第一行的起點左邊（`margin-inline-end` 照 Chrome 的 UA），baseline 對齊那一行 | 和 Chrome 一樣：不影響斷行、`text-indent`、float 旁的行寬；list item 只有 block 子盒子時也照第一個後代的第一行 | 要在 list item 排完之後找它的第一行（可能在後代的 IFC 裡），另外放一個盒子 |
| M2 第一行開頭的行內盒子，負的 margin 拉到外面 | 簡單，就是 inside marker 加 margin | 會影響斷行（marker 的寬度先佔掉再拉回）、`text-indent`、float 旁的行；list item 沒有行時沒有地方放 |

### counter 什麼時候算（第 7 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **C1 建 box tree 時（選這個）** | counter 只算有盒子的元素（`display: none` 不算，照規範），box tree 本來就照樹的順序走；Blink 的 `CountersAttachmentContext` 也在 layout tree 建構時 | 局部更新時要知道哪些值變了：整份重算、比較，只重建變了的 |
| C2 style 時 | style 已經走全樹 | style 的局部重算不走整棵樹；`display: none` 要另外排除；counter 的值不是 computed value，放在 style 裡不對 |

### clearfix 的 `display: table`（第 9 條）

最常見的 clearfix（`.clearfix::after { content: ""; display: table; clear: both }`）：

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| T1 照舊：`display: table` 無效 | 不碰 table | `::after` 照 inline，`clear` 對它沒有作用，容器照樣包不住 float，這份做完很多頁面還是錯的 |
| **T2 table 系列的 `display` 在 S7 之前暫時當 block**（選這個）：`table` → block、`inline-table` → inline-block、`table-row`、`table-cell` 等 → block；computed value 照樣是 `table` | clearfix 和用 `display: table` 做排版的頁面先接近 Chrome；HTML 的 `<table>` 不受影響（UA 沒有給它 `display: table`） | 一個暫時的對應，記成限制；`display: table-cell` 的並排會變成上下疊（現在是 inline，連同 cell 的內容全部在一行，也不對） |
