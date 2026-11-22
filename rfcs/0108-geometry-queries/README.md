# RFC 0108：幾何查詢從元素往上找，不走整棵樹

- 開始日期：2026-11-22
- 狀態：📝 草稿
- 細節：`details.md`

## 摘要

`offsetWidth`、`getBoundingClientRect()` 這些幾何查詢現在每次都從 fragment tree 的根往下找元素，成本跟著整頁的大小長：30000 列的頁面，什麼都沒改、只讀一列，`offsetHeight` 是 Chrome 的 657 倍、`getBoundingClientRect()` 是 1790 倍。2026-11-22 的看法：每個盒子在做 fragment 時記下「它是哪個盒子的第幾個 child fragment」；查詢時從元素的盒子沿著它往上找出路徑，再只沿這條路徑往下累加位移、transform、sticky 和捲動。頁面的捲動照 Chrome 當成路徑上根那一步的捲動，只有 fixed 到 viewport 的盒子不減（「已決定的事項」1）。答案仍然只從 fragment tree 算，成本是元素的深度（inline 元素和文字再加上它所在那一段的行）。怎麼找到元素和其他幾項還沒確認。

## 動機

`layout` 的 `geometry`（`crates/layout/src/geometry.rs`）每個函式都從 `LayoutView` 的 fragment 開始：

| 函式 | 誰用 | 現在走多少 |
| --- | --- | --- |
| `element_box` | `offsetWidth` 一族、`clientWidth` 一族、`scrollWidth`、computed style 的用到的值、`ResizeObserver`、focus | block 找到就停；inline 元素走整棵；每次都另外算一次元素的 `scrollable_overflow`，它走過子樹 |
| `is_fixed` | 下面幾個要不要扣 viewport 的捲動 | 找到就停，沒找到走整棵 |
| `client_rects` | `getBoundingClientRect`、`getClientRects` | 一律走整棵，block 找到了也不停 |
| `border_quads`、`box_quads` | DevTools | 整棵；`box_quads` 的 block 找到就停 |
| `text_rects` | `Range` 的幾何 | 整棵；收合的 range 找到就停 |

所以一次 `getBoundingClientRect()` 走兩趟，對每一列都讀一次是 O(N²)：10000 列要 1.65 秒，Chrome 是 4 ms。「改一列再讀回來」的頁面也每次都付這個成本。分函式量過，時間就在這幾趟走訪和 `scrollable_overflow`（`details.md` 的「實測」）。

目標：

1. 一個元素的幾何查詢和它的兄弟、整頁有多少盒子無關：block 是 O(深度)，inline 元素和文字是 O(深度 + 它所在的 inline formatting context)。
2. 只有問 `scrollWidth`、`scrollHeight` 的人付 scrollable overflow 的成本。
3. 除了第 3 階段修的座標（頁面的捲動、`offsetX`、`offsetY`），答案和現在一樣。

解決 `docs/limitations.md` 裡的限制：

- `getClientRects`、`getBoundingClientRect` 那一條的後半：頁面捲動之後，fixed 盒子裡的元素的 rect 多扣了捲動的距離。原因是 `layout` 算的是文件座標，`FrameView` 事後才照元素自己是不是 fixed 決定要不要扣捲動。2026-11-22 重現過：捲 500px，machopo 回 −490、Chrome 回 10。
- 同一條最後說 `layerY` 也跟著差：rect 修對了它**仍然**和 Chrome 不同（連 fixed 盒子自己現在就不同），要另外決定（「待決定」6），不能照著刪。

2026-11-22 找到、`docs/limitations.md` 沒有記的，同一個原因或要同一個對應，所以一起修（`details.md` 的「座標」）：

- fixed 盒子裡 SVG 的元素：`getBoundingClientRect()` 和 `getScreenCTM()` 同樣多扣（−485、−490，Chrome 是 15、10）。`FrameView::svg_geometry` 一律扣一次。
- `offsetX`、`offsetY`：`padding_origin` 拿沒有經過 transform 和捲動容器的 `element_box` 再一律扣頁面的捲動，所以在 fixed 盒子自己和它裡面（510，Chrome 是 10）、捲過的捲動容器裡（40，Chrome 是 70）、有 transform 的元素上（`scale(2)`：−300，Chrome 是 −150）都不對。

**不在範圍內**（同一次量測找到的另外三個成本，見「之後的可能」）：

- style 和 box tree 的更新從 document 開始走。
- layout 之後走整棵樹的幾趟（文件大小、捲動容器、sticky）。
- relayout boundary 和 simplified layout。

也不在範圍內：`scrollIntoView()`（它有自己的走訪；fixed 的目標捲動頁面的 bug 是另一個原因，另外修；不看 transform 和 sticky 的位移的等這份做完再修，「已決定的事項」2）。

## 設計概要

對網頁沒有 API 的變化，下面這種寫法不再隨列數的平方變慢：

```js
for (const row of rows) total += row.getBoundingClientRect().height;
```

捲動之後，fixed 的頁首裡的按鈕回報它在畫面上的位置：

```js
scrollTo(0, 500);
button.getBoundingClientRect().top;  // 10；現在是 -490
// 點在按鈕的上緣往下 5px：
event.offsetY;                       // 5；現在是 505
```

對改 layout 的人：一句話是「讀幾何從元素往上找，不要從根往下找」。要一個元素的 fragment 或位置時向 box tree 要它的路徑，不另外寫從根開始的走訪；要畫面上的位置時也向它要，不自己扣頁面的捲動。

## 詳細設計

### 盒子記下自己的 fragment 在哪

`LayoutBox`（`crates/layout/src/lib.rs`）多一個欄位：它的 fragment 是哪個盒子的 fragment 的 `children` 的第幾個。`BoxTree::fragment` 組 `children` 時寫進每個 child；清單的外側 marker 是最後加進去的，在那裡寫。

- **是 fragment tree 的父層，不是 `LayoutBox::parent`**：out-of-flow 盒子的 `parent` 是 box tree 裡的父層，它的 fragment 卻是 containing block 的 child。幾何要照後者，像 Blink 照 `LayoutObject::Container()`。
- **記索引，不記位移**：位移只有 `FragmentLink::offset` 一份。`Layout::location` 不能直接用：marker 的位移不是它。
- **沿用的 fragment 不用重寫**：父層的 fragment 沿用時，它的 `children` 沒變，索引仍然成立。
- **過期的認得出來**：往下走時每一層比對 child 和盒子自己的 fragment 是不是同一個 `Arc`（只比指標，不動計數）。不同就是這個盒子不在這棵 fragment tree 裡（例如 table 的 column 沒有 fragment），和現在走訪找不到一樣回沒有盒子。

### 從節點到盒子

- **block、atomic inline、replaced**：已經有 `BoxTree::elements`。
- **text node、`<br>`**：已經有 `BoxTree::texts`。
- **inline 元素**：新的索引 `NodeId` → 它在哪些盒子的 inline content 裡（被 block 切開時不只一個）。和 `texts` 在同樣的地方維護（`index_texts`、`forget_texts`），來源是 `InlineContent` 的 `elements`。幾個盒子照路徑上的索引排序，就是現在走訪的順序，`first`、`last` 和 union 都不變。
- **沒有盒子的元素**（`display: none`、SVG 裡面的元素）：索引查不到就回答，不走樹。

### 查詢

```rust
// 借用 FrameView 已經擁有的兩者，沒有 Rc、沒有執行時的借用檢查。
let geometry = box_tree.geometry(&fragment, viewport_scroll, &scroll, &sticky);
geometry.element_box(node);
geometry.client_rects(node);
geometry.local_point(node, point); // 反過來：viewport 的點在盒子裡的哪裡
```

（名稱是示意。）一次查詢分三步：

1. 從盒子沿著連結往上到 `LayoutView`，得到路徑。
2. 從根沿著路徑往下，每一層做現在 `walk` 做的那一步：`stuck`、`box_transform`、減掉捲動、加上 child 的 offset；離開根的那一步減的是頁面的捲動（下一節）。這一步抽成和 `walk`、`box_of` 共用的函式；累加的順序相同，浮點的結果也相同。
3. 在終點取答案：盒子的 fragment；inline 元素和文字是那一個 fragment 的 `lines`。

`used_inset` 要的父層 fragment 和其中的 offset，是路徑的倒數第二層。

指定 pseudo-element 的 `pseudo_box_quads` 只有 DevTools 用，它的盒子不在索引裡，照舊走訪。

### 頁面的捲動是根那一步

對應到 viewport 時，`LayoutView` 和別的捲動容器一樣把內容移動 `-捲動`，只有一個例外，和 Blink 的 `OffsetFromScrollableContainer` 一樣：

```rust
// 從 LayoutView 走到它的一個 child
let by = if child.fixed_to_viewport { PointF::default() } else { viewport_scroll };
to_viewport = to_viewport.then(&Transform::translate(-by.x, -by.y));
```

- **子孫不用知道自己在不在 fixed 盒子裡**：它們的路徑一定經過這一步。fixed 到 viewport 的 fragment 一定是 `LayoutView` 的 child（out-of-flow 掛在 containing block 底下）；被 transform 包住的 fixed 盒子沒有這個標記，掛在那個盒子底下，照常跟著頁面捲。
- **規則在共用的那一步**：照舊走訪的（有 pseudo 的 `pseudo_box_quads`、`svg_geometry` 經過的 `box_to_root`）也一樣對，也不依賴「待決定」1 選哪個。
- **兩種座標分清楚**：
  - 對應過的（`client_rects`、`box_quads`、`border_quads`、`text_rects`、SVG 的 rect 和 CTM、`local_point`）：viewport 座標，吃 `viewport_scroll`。
  - layout 的位置（`element_box`：`offsetTop` 一族）：照舊是文件座標，不吃 transform 和捲動，像 Blink 把 `PhysicalLocation()` 加起來。
- **要文件座標的傳 0**：捲動錨定（`crates/layout/src/scroll_anchor.rs`）用 `client_rects`、`text_rects` 比 layout 前後的位置，傳 0 時答案和現在完全一樣。
- **事後的修正全部拿掉**：`layout::is_fixed`、`FrameView::viewport_origin_of`、`client_rects` 和 `text_rects` 裡的判斷、`svg_geometry` 最後扣的一次。`FrameLayout::text_rects` 只為這個判斷而有的 `element` 參數也拿掉。

`offsetX`、`offsetY` 照 Blink 的 `AbsoluteToLocalPoint` 用同一個對應的反函數：`FrameLayout` 多一個 `local_point(node, point)`，把 `clientX`、`clientY` 換到 border box 的座標，`dom_api` 再減邊框；`padding_origin` 不再自己扣捲動，transform 和捲動容器也因此一起對了。inline 元素的原點見「待決定」7；transform 不可逆時回什麼，第 3 階段前讀 Blink 的 `TransformState` 決定。

### `scroll_size`

`ElementBox` 的 `BlockBox::scroll_size` 每次都算，所以只有路徑時 `body.offsetWidth` 仍然是 O(子樹)：30000 列時 `element_box(body)` 的 723 µs 幾乎全是它。2026-11-22 的看法：S1，從 `BlockBox` 拿掉，`dom_api::FrameLayout` 多一個 `scroll_size(node)`，只有 `scrollWidth`、`scrollHeight` 和 `is_user_scrollable` 呼叫（「待決定」2）。

### 一致性

`FrameView` 的 `box_tree` 和 `fragment` 在 `update_style_and_layout_only` 裡一起更新：box tree 變了就一定接著 layout、換掉 `fragment`，中間沒有人查詢幾何。新的 API 把這個前提寫在註解裡，用 debug assertion 檢查根是同一個 fragment。

### 改到的地方

- `crates/layout/src/lib.rs`：連結、inline 元素的索引。
- `crates/layout/src/geometry.rs`：照路徑的查詢、共用的一步（含根那一步）；刪掉 `is_fixed`。
- `crates/layout/src/scroll_anchor.rs`、`crates/layout/src/svg_geometry.rs`：傳 viewport 的捲動（前者傳 0）。
- `crates/renderer/src/frame_view.rs`：改用它；刪掉各處自己扣的捲動和 fixed 的判斷。
- `crates/dom_api/src/layout.rs`：`local_point`；`text_rects` 少一個參數；選 S1 時的 `scroll_size`。`crates/dom_api/src/range.rs` 跟著改。
- `crates/dom_api/src/geometry.rs`、`crates/gfx/src/element_box.rs`：只在選 S1 時。
- `crates/dom_api/src/ui_event.rs`：`offsetX`、`offsetY` 經過 `local_point`（第 3 階段）。
- `docs/architecture.md`：「快取和增量：加新東西時要跟著改的地方」加上「新的放 child fragment 的路要寫連結」和「建行內內容的新路要進 inline 元素的索引」；「從 fragment tree 算位置的新走訪」那一條加上根那一步。

### 測試

- **和走訪逐一比對**：從根走訪的版本留給測試當對照。`layout` 測試的 `Page` 每次檢查時，對每個元素和 text node 比較兩者的答案，現有的 layout 和 update 的測試頁因此都成為案例。原型用這個方法驗證過路徑（`details.md` 的「原型」）。
- **補上的案例**：transform、sticky、巢狀的捲動容器的組合；absolute 和 fixed 換 containing block；`order`；float 和 atomic inline 混排；外側 marker；被 block 切開和跨行的 inline 元素；table 的 caption 和 column；盒子釋放後索引被重用。
- **計數**：兄弟多十倍時查詢碰到的 fragment 數不變；`body` 的 `offsetWidth` 不算 scrollable overflow。時間不放進測試。
- **座標**：先找 WPT（`css/cssom-view/` 已經整個在清單裡），沒有涵蓋的寫測試，頁面和捲動容器都捲過：
  - fixed 盒子自己、它的 block、inline、absolute 子孫、文字的 `Range`、SVG 裡面的元素、pseudo-element 的 quads；被 transform 包住所以照常捲的 fixed；fixed 裡面有 transform 或 sticky 的。
  - 同一組的 `offsetX`、`offsetY`，加上捲動容器裡和 `translate`、`scale` 的；`layerY`。
  - 不變式：viewport 的捲動是 0 時，答案和改之前一樣（捲動錨定靠它）；fixed 到 viewport 的 fragment 只在根的下一層（`Page` 每次檢查）。
- **mutation test**：不寫 marker 的連結、用 `parent` 代替 containing block、漏掉索引的拿掉、不比對 `Arc`、漏 sticky 或捲動、路徑反過來、根那一步一律減或一律不減、根以外也減、捲動錨定不傳 0、`local_point` 不反轉 transform。
- **本機跑**：`cargo test -p layout -p paint -p dom_api -p renderer` 和 `python3 scripts/test.py wpt css/cssom-view`。
- **量測**：做完時用 `details.md` 的同一組頁面再量一次。不排 Speedometer。

### 分階段

1. **元素**：連結、inline 元素的索引、比對的測試；`element_box`、`client_rects`、`is_fixed`、`border_quads`、沒有 pseudo 的 `box_quads`；`scroll_size`。答案不變（`is_fixed` 到第 3 階段為止仍然只看終點）。
2. **文字**：`text_rects` 經過 `texts`。答案不變。
3. **座標**：根那一步、拿掉事後的修正、`local_point` 和 `offsetX`、`offsetY`；更新那條限制。這是唯一改變行為的階段，所以分開（順序見「待決定」5）。

## 缺點

- 每個盒子多 12 bytes（`LayoutBox` 現在是 2544 bytes），做 fragment 時每個 child 多一次寫入；inline 元素多一份索引，它的記憶體和維護的時間還沒量。
- 多一個要跟著改的不變式：新的放 child fragment 的路漏了連結，那個盒子就查不到幾何。靠比對的測試抓。
- 幾何查詢從「只要一棵 fragment tree」變成也要 box tree。
- 很深的樹仍然是 O(深度)，和 Blink 一樣。

## 理由和其他做法

怎麼找到元素：

| 做法 | 一次查詢 | 額外成本 | 問題 |
| --- | --- | --- | --- |
| **A：盒子記連結（2026-11-22 的看法）** | O(深度) | 每個盒子 12 bytes | 多一個不變式 |
| A′：沿 `parent` 累加 `Layout::location` | O(深度) | 無 | out-of-flow 和 marker 是錯的；位置有兩份來源 |
| A″：沿祖先往下，每層在 `children` 裡找 | O(深度 × 兄弟) | 無 | 平的長清單仍然是線性 |
| B：layout 後第一次查詢時建整頁的表 | O(1) | 每次 layout 後 O(N)，每個節點一筆 | 「改一點、讀一次」仍然是 O(N) |
| C：用 pre-paint 的 property tree | O(property tree 的深度) | 查詢前要先跑 pre-paint | script 讀幾何時只做到 layout |
| 不做 | O(N) | 無 | 現況 |

頁面的捲動在哪裡處理（「已決定的事項」1）：

| 做法 | 問題 |
| --- | --- |
| **根那一步（Chrome 的做法，選這個）** | 要文件座標的呼叫者要明寫傳 0 |
| 先算文件座標，事後照路徑上有沒有 `fixed_to_viewport` 修正 | rect 的答案一樣，但每個呼叫者都要記得修（`svg_geometry`、`padding_origin` 就是忘了的），反過來的對應還要再寫一次 |
| 現在：事後只看元素自己 | fixed 盒子裡的子孫是錯的 |

比較在 `details.md` 的「做法的比較」。A 的原型在 layout 和 paint 的測試上和走訪找到同樣的 fragment、位置和 fixed；30000 列時只找出一個元素的 fragment 和位置約 0.035 µs（還沒有組出答案），現在整個 `element_box` 是 187 µs。

## 先例

Chromium（`details.md` 的「Blink」）：

- 元素直接有 `LayoutObject`，位置存在 `LayoutBox` 上，對應到祖先時沿 `Container()` 往上。照做從元素往上、照 containing block 的鏈；不照做的是另存一份位置：machopo 的 fragment tree 不可變，已經有位置，只缺「我在哪」。
- 頁面的捲動只是往上走時最後一個捲動容器：`IsFixedPositioned() && container->IsLayoutView()` 時不減，沒有事後的修正。`offsetY` 是同一條鏈反過來（`AbsoluteToLocalPoint`）。兩者都照做。
- inline 元素記著它在 `FragmentItems` 的第一個 item。A 先只記到盒子，再掃它的行（「待決定」3）。
- scrollable overflow 算好存起來，`ScrollWidth()` 讀它；`OffsetWidth()` 不碰它。照做後者。

## 已決定的事項

2026-11-22：

1. **頁面的捲動照 Chrome 在路徑離開根的那一步處理**：往 `fixed_to_viewport` 的 child 走時不減，其他的減；`FrameView` 事後的 fixed 判斷拿掉；`offsetX`、`offsetY` 用同一個對應的反函數。不選「算完文件座標，再照路徑上有沒有 fixed 修正」：規則會分散在每個呼叫者，現在的錯就是這樣來的。這一條不決定怎麼找到元素（「待決定」1）。
2. **`scrollIntoView()`、`focus()` 和連結的捲動不看 transform 和 sticky 的位移，等這份做完再修**（「之後的可能」5）：那時可以改用這份的路徑和對應。不選現在修：要在它自己的走訪裡另做一次同樣的對應，路徑做好之後會被換掉。

## 待決定

1. **怎麼找到元素**：A、B。2026-11-22 的看法：A。決定它之後才能開始，其他幾項可以在各階段開始前決定。
2. **`scroll_size`**（第 1 階段前）：
   - S1：拿掉，另一個方法，問的時候才算。改一個 trait 的介面；`scrollWidth` 仍然 O(子樹)。
   - S2：留著，在 fragment 上快取（像 Blink）。介面不變，但 layout 後第一次 `offsetWidth` 仍然付整個子樹，每個 fragment 變大；和「之後的可能」2 重疊。
   - 2026-11-22 的看法：S1。
3. **很長的一段裡有很多 inline 元素**（第 1 階段做完時）：
   - I1：掃那一段所有的行。簡單；一段裡 M 個 `<span>` 各讀一次是 O(M²)。
   - I2：連行和其中的索引一起記（像 Blink 的 `FirstInlineFragmentItemIndex`）。每次斷行都要維護。
   - 2026-11-22 的看法：先 I1，第 1 階段做完時量一段 10000 個 `<span>` 的頁再決定。
4. **其他從根找一個節點的走訪要不要一起改**（第 2 階段前）：`svg_geometry`、`caret_rect`、`child_viewport` 的 `find_fragment`。2026-11-22 的看法：不一起，列在「之後的可能」；都還沒量過。
5. **座標的修正先做還是最後做**（第 1 階段前）：
   - 最後（現在的第 3 階段）：前兩段只換找的方法，拿現在的答案比對。
   - 最先：規則在共用的一步，用現在的走訪就能做，之後不用丟掉；對的答案比較早有，比對的對照組一開始就是對的，第 1 階段也不用暫時留著 `is_fixed`。`local_point` 那時仍然是 O(N)，和現在的 `offsetY` 一樣。
   - 2026-11-22 的看法：最先（照 Chrome 改之後，原本「那段程式之後會被換掉」的理由不成立了）。
6. **`layerY` 在 fixed 的 layer 上**（第 3 階段前）：Chrome 捲 500px 之後對 fixed 盒子自己回 −490（Blink 的 `ComputeRelativePosition` 拿 frame 裡的點減 layer 的 absolute 位置），machopo 回 10。照 Chrome 的怪處做，或留著、改寫那條限制。2026-11-22 的看法：留著；Blink 自己的註解說這個屬性定義不清。
7. **inline 元素的 `offsetX`、`offsetY` 從哪裡起算**（第 3 階段前）：Chrome 從它所在的區塊起算（第二行的 `<span>`：−447），machopo 從它第一個行框起算（−467.5），CSSOM View 說的是目標的 padding edge。2026-11-22 的看法：留著 machopo 的，只讓它經過對應。

## 之後的可能

1. style 和 box tree 的更新從髒節點的共同祖先開始（Blink 的 `StyleTraversalRoot`）：改一列的顏色再讀 computed style，30000 列時是 Chrome 的 434 倍。
2. layout 之後不走整棵樹（分組的頁面改一列，lifecycle 裡 span 以外的 566 µs 推測在這裡，還沒分開量）：快取 scrollable overflow、只更新變了的捲動容器和 sticky 盒子。`scrollWidth` 也因此變成常數。
3. relayout boundary 和 simplified layout（`contain: strict` 的列現在一樣整頁重跑）。
4. 「待決定」4 的走訪改用同一條路徑。
5. `scrollIntoView()` 改用同一條路徑（這份做完之後修，「已決定的事項」2）：它自己的走訪不看 transform 和 sticky 的位移（`translateY(300px)` 裡、在 1500 的元素捲到 1500，Chrome 是 1800）。
