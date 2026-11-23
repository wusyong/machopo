# RFC 0108：幾何查詢從元素往上找，不走整棵樹

- 開始日期：2026-11-22
- 狀態：🚧 已排程：2026-11-22
- 細節：`details.md`

## 摘要

`offsetWidth`、`getBoundingClientRect()` 這些幾何查詢現在每次都從 fragment tree 的根往下找元素，成本跟著整頁的大小長：30000 列的頁面，什麼都沒改、只讀一列，`offsetHeight` 是 Chrome 的 657 倍、`getBoundingClientRect()` 是 1790 倍。2026-11-22 的看法：每個盒子在做 fragment 時記下「它是哪個盒子的第幾個 child fragment」；查詢時從元素的盒子沿著它往上找出路徑，再只沿這條路徑往下累加位移、transform、sticky 和捲動。頁面的捲動照 Chrome 當成路徑上根那一步的捲動，只有 fixed 到 viewport 的盒子不減（「已決定的事項」1）：這是唯一改變答案的地方。答案仍然只從 fragment tree 算，成本是元素的深度（inline 元素和文字再加上它所在那一段的行）。怎麼找到元素和其他幾項還沒確認。滑鼠事件座標的其他錯不在範圍內（「已決定的事項」11）。

## 動機

`layout` 的 `geometry`（`crates/layout/src/geometry.rs`）每個函式都從 `LayoutView` 的 fragment 開始：

| 函式 | 誰用 | 現在走多少 |
| --- | --- | --- |
| `element_box` | `offsetWidth` 一族、`clientWidth` 一族、`scrollWidth`、computed style 的用到的值、`ResizeObserver`、focus | block 找到就停；inline 元素走整棵；每次都另外算一次元素的 `scrollable_overflow`，它走過子樹 |
| `is_fixed`（第 1 階段拿掉了） | 下面幾個要不要扣 viewport 的捲動 | 找到就停，沒找到走整棵 |
| `client_rects` | `getBoundingClientRect`、`getClientRects` | 一律走整棵，block 找到了也不停 |
| `border_quads`、`box_quads` | DevTools | 整棵；`box_quads` 的 block 找到就停 |
| `text_rects` | `Range` 的幾何 | 整棵；收合的 range 找到就停 |

所以第 1 階段之前一次 `getBoundingClientRect()` 走兩趟（現在一趟，仍然是整棵；下面是之前量的），對每一列都讀一次是 O(N²)：10000 列要 1.65 秒，Chrome 是 4 ms。「改一列再讀回來」的頁面也每次都付這個成本。分函式量過，時間就在這幾趟走訪和 `scrollable_overflow`（`details.md` 的「實測」）。

目標：

1. 一個元素的幾何查詢和它的兄弟、整頁有多少盒子無關：block 是 O(深度)，inline 元素和文字是 O(深度 + 它所在的 inline formatting context)。
2. 只有問 `scrollWidth`、`scrollHeight` 的人付 scrollable overflow 的成本。
3. 除了頁面的捲動在 fixed 盒子裡算錯的（第 1 階段修，「已決定的事項」1），答案都和現在一樣。

解決 `docs/limitations.md` 裡的限制（第 1 階段做完，兩條都從那裡拿掉了）：

- `getClientRects`、`getBoundingClientRect` 那一條的後半：頁面捲動之後，fixed 盒子裡的元素的 rect 多扣了捲動的距離。原因是 `layout` 算的是文件座標，`FrameView` 事後才照元素自己是不是 fixed 決定要不要扣捲動。2026-11-22 重現過：捲 500px，machopo 回 −490、Chrome 回 10。
- 同一條最後說 `layerY` 也跟著差：它從 layer 的 client rect 算，所以跟著修好（有 `view` 的事件和真的輸入；沒有 `view` 的照舊和 Chrome 不同），不另外改它。

`docs/limitations.md` 沒有記、同一個原因，所以同一條規則一起修好的：

- fixed 盒子裡 SVG 的元素：`getBoundingClientRect()` 和 `getScreenCTM()` 同樣多扣（−485、−490，Chrome 是 15、10）。`FrameView::svg_geometry` 一律扣一次。
- 用這些查詢的：fixed 盒子裡 `<select>` 的選單位置、DevTools 的盒子模型和標示（第 1 階段用測試確認了）。

**不在範圍內**（同一次量測找到的另外三個成本，見「之後的可能」）：

- style 和 box tree 的更新從 document 開始走。
- layout 之後走整棵樹的幾趟（文件大小、捲動容器、sticky）。
- relayout boundary 和 simplified layout。

也不在範圍內：

- **滑鼠事件座標的其他錯**（「已決定的事項」11）：`offsetX`、`offsetY` 在 fixed、捲動容器、transform、inline 元素和 SVG 上的錯，`layerX`、`layerY` 其他和 Chrome 不同的地方，`MouseEvent` 座標的取整。量測和當時的決定留在 `details.md` 的「移出範圍的滑鼠事件座標」。
- `scrollIntoView()`：它有自己的走訪；fixed 的目標捲動頁面的 bug 是另一個原因，另外修；不看 transform 和 sticky 的位移的等這份做完再修（「已決定的事項」2）。

## 設計概要

對網頁沒有 API 的變化，下面這種寫法不再隨列數的平方變慢：

```js
for (const row of rows) total += row.getBoundingClientRect().height;
```

捲動之後，fixed 的頁首裡的按鈕回報它在畫面上的位置：

```js
scrollTo(0, 500);
button.getBoundingClientRect().top;  // 10；第 1 階段之前是 -490
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
let scrolled = if child.fixed_to_viewport { PointF::default() } else { viewport_scroll };
let mapping = Mapping { to_root: Transform::IDENTITY, scrolled };
// 在終點：rect 是 to_root.map_rect(r) 再減 scrolled，quad 每個點都減。
```

捲動帶在 transform 旁邊，不乘進去（第 1 階段實作時定）：`map_rect` 對應四個角再取最小和最大，乘進去之後捲得越遠，rect 的寬高取整得越不一樣。帶在旁邊時，不在 fixed 盒子裡的答案和改之前每個 bit 都相同。

- **子孫不用知道自己在不在 fixed 盒子裡**：它們的路徑一定經過這一步。fixed 到 viewport 的 fragment 一定是 `LayoutView` 的 child（out-of-flow 掛在 containing block 底下）；被 transform 包住的 fixed 盒子沒有這個標記，掛在那個盒子底下，照常跟著頁面捲。
- **規則在共用的那一步**：照舊走訪的（有 pseudo 的 `pseudo_box_quads`、`svg_geometry` 經過的 `box_to_viewport`）也一樣對，也不依賴怎麼找到元素。
- **兩種座標分清楚**：
  - 對應過的（`client_rects`、`box_quads`、`border_quads`、`text_rects`、SVG 的 rect 和 CTM）：viewport 座標，吃 `viewport_scroll`。
  - layout 的位置（`element_box`：`offsetTop` 一族）：照舊是文件座標，不吃 transform 和捲動，像 Blink 把 `PhysicalLocation()` 加起來。
- **要文件座標的傳 0**：捲動錨定（`crates/layout/src/scroll_anchor.rs`）用 `client_rects`、`text_rects` 比 layout 前後的位置，傳 0 時答案和現在完全一樣。
- **事後的修正全部拿掉**：`layout::is_fixed`、`FrameView::viewport_origin_of`、`client_rects` 和 `text_rects` 裡的判斷、`svg_geometry` 最後扣的一次。`FrameLayout::text_rects` 只為這個判斷而有的 `element` 參數也拿掉。

滑鼠事件不改（「已決定的事項」11）：`offsetX`、`offsetY` 照舊從 `element_box` 的文件座標扣頁面的捲動，答案不變；`layerX`、`layerY` 的算法也不變，它讀的 `client_rects` 對了，fixed 盒子裡的就跟著對。

### `scroll_size`

`ElementBox` 的 `BlockBox::scroll_size` 每次都算，所以只有路徑時 `body.offsetWidth` 仍然是 O(子樹)：30000 列時 `element_box(body)` 的 723 µs 幾乎全是它。選 S1（「已決定的事項」13）：從 `BlockBox` 拿掉，`dom_api::FrameLayout` 多一個 `scroll_size(node)`，只有 `scrollWidth`、`scrollHeight` 和 `is_user_scrollable` 呼叫。

### 一致性

`FrameView` 的 `box_tree` 和 `fragment` 在 `update_style_and_layout_only` 裡一起更新：box tree 變了就一定接著 layout、換掉 `fragment`。中間只有捲動錨定讀幾何（2026-11-22 讀程式確認）：

- layout 前 `pick_scroll_anchors` 讀**舊的** fragment。`find_scroll_anchor` 是它自己的走訪，不呼叫 `geometry` 的查詢，也不碰 box tree。它不能改用路徑的查詢。
- layout 後 `scroll_anchor_offset` 用 `client_rects`、`text_rects` 讀**新的** fragment，這時和 box tree 一致，只是還沒存進 `FrameView`：要用路徑的查詢時把這一個傳進去。

新的 API 把這個前提寫在註解裡，用 debug assertion 檢查根是同一個 fragment。

### 改到的地方

- `crates/layout/src/lib.rs`：連結、inline 元素的索引（第 2 階段）。
- `crates/layout/src/fragment.rs`：每個行內元素在 fragment 的第幾行到第幾行（「已決定的事項」14）。
- `crates/layout/src/tree_builder.rs`：增量更新時維護它們。
- `crates/layout/src/geometry.rs`：照路徑的查詢、共用的一步（含根那一步）；刪掉 `is_fixed`。
- `crates/layout/src/scroll_anchor.rs`、`crates/layout/src/svg_geometry.rs`：傳 viewport 的捲動（前者傳 0）。
- `crates/renderer/src/frame_view.rs`：改用它；刪掉各處自己扣的捲動和 fixed 的判斷。
- `crates/dom_api/src/layout.rs`：`text_rects` 少一個參數；`scroll_size`。`crates/dom_api/src/range.rs` 跟著改。
- `crates/dom_api/src/geometry.rs`、`crates/gfx/src/element_box.rs`：拿掉 `scroll_size`。
- `docs/architecture.md`：「快取和增量：加新東西時要跟著改的地方」加上「新的放 child fragment 的路要寫連結」和「建行內內容的新路要進 inline 元素的索引」；「從 fragment tree 算位置的新走訪」那一條加上根那一步。

### 測試

- **和走訪逐一比對**：從根走訪的版本留給測試當對照。`layout` 測試的 `Page` 每次檢查時，對每個元素和 text node 比較兩者的答案，現有的 layout 和 update 的測試頁因此都成為案例。原型用這個方法驗證過路徑（`details.md` 的「原型」）。
- **連結本身**：`Page` 也檢查走訪看得到的每個 fragment 剛好有一個盒子沿連結找得到它，像原型一樣。只比答案時，漏寫外側 marker 的連結抓不到：沒有一個查詢走到 marker（它是 pseudo-element，`pseudo_box_quads` 照舊走訪）。2026-11-23 更正：原本的測試計畫只比答案，卻把這條列進 mutation test。
- **補上的案例**：transform、sticky、巢狀的捲動容器的組合；absolute 和 fixed 換 containing block；`order`；float 和 atomic inline 混排；外側 marker；被 block 切開和跨行的 inline 元素；table 的 caption 和 column；盒子釋放後索引被重用。
- **計數**：兄弟多十倍時查詢碰到的 fragment 數不變；`body` 的 `offsetWidth` 不算 scrollable overflow。時間不放進測試。
- **座標**：先找 WPT（`css/cssom-view/` 已經整個在清單裡），沒有涵蓋的寫測試，頁面和捲動容器都捲過：
  - fixed 盒子自己、它的 block、inline、absolute 子孫、文字的 `Range`、SVG 裡面的元素、pseudo-element 的 quads；被 transform 包住所以照常捲的 fixed；fixed 裡面有 transform 或 sticky 的。
  - fixed 盒子裡的 layer 的 `layerY`、`<select>` 的選單位置（寫成情境）、DevTools 的 quads。
  - 對照：同一頁的 `offsetX`、`offsetY` 和改之前一樣。
  - 不變式：viewport 的捲動是 0 時，答案和改之前一樣（捲動錨定靠它）；fixed 到 viewport 的 fragment 只在根的下一層（`Page` 每次檢查）。
- **mutation test**：不寫 marker 的連結、用 `parent` 代替 containing block、漏掉索引的拿掉、不比對 `Arc`、漏 sticky 或捲動、路徑反過來、根那一步一律減或一律不減、根以外也減、捲動錨定不傳 0。
- **本機跑**：`cargo test -p layout -p paint -p dom_api -p renderer`、`python3 scripts/test.py wpt css/cssom-view`，和守著 `layerX` 的 `uievents/mouse/layer-coords-transform.html`。
- **量測**：做完時用 `details.md` 的同一組頁面再量一次。不排 Speedometer。

### 分階段

1. **座標**：用現在的走訪做根那一步，拿掉事後的修正（連同 `is_fixed`）；更新那條限制。改變答案的只有這一段，所以分開、先做（「已決定的事項」3）。
2. **元素**：連結、inline 元素的索引、比對的測試；`element_box`、`client_rects`、`border_quads`、沒有 pseudo 的 `box_quads`；`scroll_size`。答案不變。
3. **文字**：`text_rects` 經過 `texts`。答案不變。

### 什麼時候停

「已決定的事項」11 的一部分：

- **第 1 階段**：除了根那一步還要一條特例才會對時，停下來問，不擴大範圍。
- **第 2、3 階段**：答案和走訪不同又修不掉，或第 2 階段做完時每列讀一次在 10000 列仍隨列數的平方變慢：停掉，只留第 1 階段的修正。

## 缺點

- 每個盒子多 12 bytes（連結；`LayoutBox` 現在是 2544 bytes），做 fragment 時每個 child 多一次寫入；inline 元素多一份索引，它的記憶體和維護的時間還沒量；每次排一段時多建一份每個行內元素在第幾行的表（每個 12 bytes）。
- 多一個要跟著改的不變式：新的放 child fragment 的路漏了連結，那個盒子就查不到幾何。靠比對的測試抓。
- 幾何查詢從「只要一棵 fragment tree」變成也要 box tree。
- 很深的樹仍然是 O(深度)，和 Blink 一樣。

## 理由和其他做法

怎麼找到元素：

| 做法 | 一次查詢 | 額外成本 | 問題 |
| --- | --- | --- | --- |
| **A：盒子記連結（選這個，「已決定的事項」12）** | O(深度) | 每個盒子 12 bytes | 多一個不變式 |
| A′：沿 `parent` 累加 `Layout::location` | O(深度) | 無 | out-of-flow 和 marker 是錯的；位置有兩份來源 |
| A″：沿祖先往下，每層在 `children` 裡找 | O(深度 × 兄弟) | 無 | 平的長清單仍然是線性 |
| B：layout 後第一次查詢時建整頁的表 | O(1) | 每次 layout 後 O(N)，每個節點一筆 | 「改一點、讀一次」仍然是 O(N) |
| C：用 pre-paint 的 property tree | O(property tree 的深度) | 查詢前要先跑 pre-paint | script 讀幾何時只做到 layout |
| 不做 | O(N) | 無 | 現況 |

頁面的捲動在哪裡處理（「已決定的事項」1）：

| 做法 | 問題 |
| --- | --- |
| **根那一步（Chrome 的做法，選這個）** | 要文件座標的呼叫者要明寫傳 0 |
| 先算文件座標，事後照路徑上有沒有 `fixed_to_viewport` 修正 | rect 的答案一樣，但每個呼叫者都要記得修（`svg_geometry` 就是忘了的） |
| 第 1 階段之前：事後只看元素自己 | fixed 盒子裡的子孫是錯的 |

比較在 `details.md` 的「做法的比較」。A 的原型在 layout 和 paint 的測試上和走訪找到同樣的 fragment、位置和 fixed；30000 列時只找出一個元素的 fragment 和位置約 0.035 µs（還沒有組出答案），現在整個 `element_box` 是 187 µs。

## 先例

Chromium（`details.md` 的「Blink」）：

- 元素直接有 `LayoutObject`，位置存在 `LayoutBox` 上，對應到祖先時沿 `Container()` 往上。照做從元素往上、照 containing block 的鏈；不照做的是另存一份位置：machopo 的 fragment tree 不可變，已經有位置，只缺「我在哪」。
- 頁面的捲動只是往上走時最後一個捲動容器：`IsFixedPositioned() && container->IsLayoutView()` 時不減，沒有事後的修正。照做。`offsetY` 在 Blink 是同一條鏈反過來（`AbsoluteToLocalPoint`），這份不做（「已決定的事項」11）。
- inline 元素記著它在 `FragmentItems` 的第一個 item。A 先只記到盒子，再掃它的行（「待決定」3）。
- scrollable overflow 算好存起來，`ScrollWidth()` 讀它；`OffsetWidth()` 不碰它。照做後者。

## 已決定的事項

2026-11-22：

1. **頁面的捲動照 Chrome 在路徑離開根的那一步處理**：往 `fixed_to_viewport` 的 child 走時不減，其他的減；`FrameView` 事後的 fixed 判斷拿掉。不選「算完文件座標，再照路徑上有沒有 fixed 修正」：規則會分散在每個呼叫者，現在的錯就是這樣來的。這一條不決定怎麼找到元素（12）。原本連同 `offsetX`、`offsetY` 用它的反函數，那一半移出範圍（11）。
2. **`scrollIntoView()`、`focus()` 和連結的捲動不看 transform 和 sticky 的位移，等這份做完再修**（「之後的可能」5）：那時可以改用這份的路徑和對應。不選現在修：要在它自己的走訪裡另做一次同樣的對應，路徑做好之後會被換掉。決定時路徑和對應預定一起做好；座標改成先做（3）之後，對應在第 1 階段就有，所以技術上可以更早修，時間仍照這個決定。
3. **座標先做**（第 1 階段；元素第 2、文字第 3）：根那一步的規則在共用的一步裡，用現在的走訪就能做，之後不用丟掉；對的答案比較早有，之後兩段比對的對照組一開始就是對的，也不用暫時留著 `is_fixed`。不選最後做：錯的座標要多留兩段。

4 到 10 是滑鼠事件座標的決定，11 把它們移出範圍：原文和編號留在 `details.md` 的「移出範圍的滑鼠事件座標」。

11. **範圍縮小**：只做這份原本要解的兩件事，頁面的捲動在 fixed 盒子裡算對（1）和查詢不走整棵樹（3 和「待決定」）；「什麼時候停」也是這一條。1 裡 `offsetX`、`offsetY` 的那一半和 4 到 10 移出，做的時候另寫 RFC；現在的錯記在 `docs/limitations.md`。為什麼：兩個問題重量過都成立（`details.md` 的「縮小範圍時的重量」），解它們不需要那些決定；那些規則是照 Chrome 猜的，只能拿模型比，寫下之後推翻過好幾次，留下的則有現成的對照（根那一步對 Chrome、加速對現在的走訪）。不選照原範圍做（大半的工作和風險不在這兩個問題上）和整份停掉（兩個問題都還在）。代價：`offsetX`、`offsetY` 在 fixed 盒子、捲動容器和 transform 上暫時照舊是錯的。

2026-11-23：

12. **怎麼找到元素：A，盒子記下自己的 fragment 是哪個 fragment 的第幾個 child**，照 Chrome（元素有 `LayoutObject`，從它往上找）。A 的原型（在一份拷貝上，沒有留在 repo）在 layout、paint、renderer、shell 的測試、情境和 11079 個 WPT 的靜態頁上，約 5400 萬次查詢和走訪的答案相同（renderer 裡的和大的頁面是抽樣比的）；隨機改 DOM 的 7099 組裡不同的 10 組，盒子樹本身都已經壞了（同一個盒子出現兩次，那個 bug 在 `main` 的 cc6479f 修好）。10000 列每列讀一次：`offsetHeight` 2.58 ms、`getBoundingClientRect` 4.16 ms（現在 429、1290；Chrome 158 是 2.24、3.96；Windows 11）。不選 B（每次 layout 後建整頁的表）：沒有人查也要付，只量了建表就是每次 layout 1.28 ms（10000 列）、8.9 ms（30000 列），「改一點、讀一次」仍是 O(N)；Chrome 也不這樣做。代價：每個盒子 12 bytes 和 inline 元素的索引，一個要跟著改的不變式（「缺點」）。只有 A 時前 1000 列的 `offsetTop` 仍是 113 ms（Chrome 0.42），要看「待決定」2。這些數字是 renderer 還在 Browser 的 process 裡時量的，做完時重量。

13. **`scroll_size`：S1，從 `BlockBox` 拿掉，`dom_api::FrameLayout` 多一個 `scroll_size(node)`，問的時候才算**。照 Chrome 的 `OffsetWidth()` 不碰 scrollable overflow：原型上只有 A 時前 1000 列的 `offsetTop` 是 113 ms，A 加上 S1 是 0.39 ms（Chrome 158 是 0.42；Windows 11）。不選 S2（留著，算過一次存在 fragment 上）：介面不變，但每次 layout 之後第一次 `offsetWidth` 仍然付整個子樹，每個 fragment 變大，又和「之後的可能」2 重疊。代價：改 `FrameLayout` 的介面，`scrollWidth`、`scrollHeight` 照舊每次走子樹；Chrome 在 layout 時算好存起來，這一半是「之後的可能」2，這份不做。
14. **一段裡有很多行內元素：I2，fragment 記下每個行內元素在它的第幾行到第幾行**（2026-11-23），照 Chrome（Blink 的 `LayoutInline` 記下它的第一個 `FragmentItem`，`FirstInlineFragmentItemIndex`）。查詢用二分搜找到元素，只看那幾行。第 2 階段做完時量的：一段 1000 個、10000 個 `<span>` 各讀一次 `offsetHeight` 是 0.68、40 ms（Chrome 158 是 0.17、2.13；Windows 11），十倍的 `<span>` 慢 59 倍。不選 I1（照舊掃整段的行）：一段裡一個 token 一個 `<span>` 的頁面（語法上色的 `<pre>`）是平方。Blink 另外讓每個 item 記下到同一個元素的下一個 item 有多遠，這裡不做：只在一行上有很多層巢狀的元素時才有差，做的話每個行內盒子都多一個欄位。代價：每次排一段時多建一份排好序的表（每個行內元素 12 bytes，O(B log B)，B 是那一段的盒子數），沒人查也要付；Chrome 也付。

## 待決定

1. **怎麼找到元素**：已決定，A（「已決定的事項」12）。
2. **`scroll_size`**：已決定，S1（「已決定的事項」13）。
3. **很長的一段裡有很多 inline 元素**：已決定，I2（「已決定的事項」14）。
   - I1：掃那一段所有的行。簡單；一段裡 M 個 `<span>` 各讀一次是 O(M²)。
   - I2：連行和其中的索引一起記（像 Blink 的 `FirstInlineFragmentItemIndex`）。每次斷行都要維護。
   - 2026-11-22 的看法：先 I1，第 2 階段做完時量一段 10000 個 `<span>` 的頁再決定。
   - 2026-11-23 第 2 階段做完時量的（Windows 11、Chrome 158）：一段 1000 個 `<span>` 各讀一次 `offsetHeight` 是 0.68 ms、10000 個是 40 ms（Chrome 0.17、2.13），十倍的 `<span>` 慢 59 倍；從根走時是 0.62、41.5 ms，第 2 階段沒有讓它變慢。
4. **其他從根找一個節點的走訪要不要一起改**（第 3 階段前）：`svg_geometry`、`caret_rect`、`child_viewport` 的 `find_fragment`。2026-11-22 的看法：不一起，列在「之後的可能」；都還沒量過。

## 之後的可能

1. style 和 box tree 的更新從髒節點的共同祖先開始（Blink 的 `StyleTraversalRoot`）：改一列的顏色再讀 computed style，30000 列時是 Chrome 的 434 倍。
2. layout 之後不走整棵樹（分組的頁面改一列，lifecycle 裡 span 以外的 566 µs 推測在這裡，還沒分開量）：快取 scrollable overflow、只更新變了的捲動容器和 sticky 盒子。`scrollWidth` 也因此變成常數。
3. relayout boundary 和 simplified layout（`contain: strict` 的列現在一樣整頁重跑）。
4. 「待決定」4 的走訪改用同一條路徑。
5. `scrollIntoView()` 改用同一條路徑（這份做完之後修，「已決定的事項」2）：它自己的走訪不看 transform 和 sticky 的位移（`translateY(300px)` 裡、在 1500 的元素捲到 1500，Chrome 是 1800）。
6. 滑鼠事件的座標（「已決定的事項」11 移出的）：另一份 RFC，從 `details.md` 的「移出範圍的滑鼠事件座標」開始。
