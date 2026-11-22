# RFC 0108 的細節：幾何查詢從元素往上找，不走整棵樹

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 實測

2026-11-22，machopo a2f1722（`layout`、`style`、`renderer`、`dom_api` 和 8e5dc20 相同），i9-13900K，Windows 11。Chrome 是裝在機器上的正式版 158.0.8127.36，**不是** `bench.py` 用的 Chrome for Testing 152。量測用的是一次性的腳本，沒有進 repo，頁面和量法寫在下面。

### 只讀，什麼都沒改

頁面：一個 `<div>` 裡 N 個 `<div class=r><span>row i</span></div>`（`.r { height: 20px }`，Arial 16px）。`load` 之後 300 ms，先讀一次讓 layout 做完，再計時：重複次數加倍到一批至少 20 ms，取五批的中位數。machopo 是 `perf` profile、`--automation-pipe`，Chrome 是 `--headless=new`；各跑五次、每次新開，下表是中位數 [最小、最大]，兩邊讀到的值相同。

| 操作 | N | machopo（µs） | Chrome（µs） | 倍數 |
| --- | --: | --: | --: | --: |
| 中間那列的 `offsetHeight` | 1000 | 2.466 [2.454、2.527] | 0.204 [0.203、0.207] | 12.1 |
| 同上 | 10000 | 38.477 [37.402、41.992] | 0.198 [0.193、0.201] | 194.7 |
| 同上 | 30000 | 132.812 [132.031、134.375] | 0.202 [0.201、0.207] | 656.9 |
| 中間那列的 `getBoundingClientRect()` | 1000 | 10.596 [10.498、10.645] | 0.389 [0.386、0.397] | 27.2 |
| 同上 | 10000 | 143.359 [135.937、151.562] | 0.395 [0.381、0.403] | 362.7 |
| 同上 | 30000 | 715.625 [693.750、906.250] | 0.400 [0.398、0.401] | 1790.0 |
| 每一列的 `offsetHeight` | 1000 | 2525 [2450、2600] | 212.5 [207.0、214.8] | 11.9 |
| 同上 | 3000 | 23200 [22800、26900] | 646.9 [631.3、665.6] | 35.9 |
| 同上 | 10000 | 382700 [378900、402000] | 2081 [2025、2213] | 183.9 |
| 每一列的 `getBoundingClientRect()` | 1000 | 10850 [10800、11050] | 400.0 [390.6、407.8] | 27.1 |
| 同上 | 3000 | 98500 [97600、108300] | 1228 [1200、1250] | 80.2 |
| 同上 | 10000 | 1651000 [1635100、1858400] | 4050 [3950、4175] | 407.7 |

Chrome 的單次查詢不隨 N 變；machopo 是線性，每列讀一次是平方。

### 分函式

為了確認時間在哪：一個只連結 `layout`、`style`、`html`、`dom` 的程式（release），同樣形狀的頁面（高度寫在 `style` 屬性），直接呼叫函式，量九批取中位數（µs）。

| N | 元素 | `element_box` | `client_rects` | `is_fixed` | `scrollable_overflow` |
| --: | --- | --: | --: | --: | --: |
| 1000 | 中間那列 | 2.467 | 7.267 | 1.295 | |
| 1000 | 它的 `<span>` | 4.916 | 7.306 | 1.207 | |
| 1000 | `<body>` | 8.047 | 7.068 | 0.004 | 9.655 |
| 10000 | 中間那列 | 61.5 | 155.6 | 27.4 | |
| 10000 | 它的 `<span>` | 116.3 | 153.0 | 26.8 | |
| 10000 | `<body>` | 123.5 | 156.8 | 0.004 | 127.2 |
| 30000 | 中間那列 | 186.9 | 1013.5 | 106.7 | |
| 30000 | 它的 `<span>` | 594.7 | 1217.1 | 115.6 | |
| 30000 | `<body>` | 723.2 | 1045.2 | 0.004 | 773.9 |

- `client_rects` 和元素在哪無關：連第三層的 `<body>` 也走整棵。`getBoundingClientRect()` 是 `is_fixed` 加 `client_rects`。
- `element_box` 對 inline 元素走整棵（中間那列的兩到三倍）；對 `<body>` 幾乎全是 `scrollable_overflow`，所以只改成照路徑找還不夠。
- 1000 列時和網頁裡量到的對得上（2.467 對 2.466；7.267 + 1.295 = 8.6，對 10.6）。更大時這個程式比網頁裡慢到約 1.6 倍（heap 的排列不同），只拿來看時間在哪個函式，倍數以上一節為準。

### 改一列再讀回來

這一組包含範圍外的成本，列在這裡說明做完之後還剩什麼。改中間那列再讀一個值（µs，machopo / Chrome）：

| 頁面 | 1000 | 3000 | 10000 | 30000 |
| --- | --: | --: | --: | --: |
| 改文字，block | 161.0 / 123.4 | 554.7 / 380.2 | 2707 / 1304 | 11567 / 3950 |
| 改文字，flex column | 182.1 / 173.9 | 564.1 / 535.9 | 2577 / 1905 | 11533 / 5600 |
| 改文字，分成 √N 組 | 31.7 / 15.4 | 76.5 / 21.8 | 252.7 / 34.0 | 1188 / 53.4 |
| 改文字，`contain: strict` 的列 | 154.6 / 7.1 | 521.9 / 18.9 | 2464 / 130.1 | 10000 / 381.1 |
| 改顏色，`getComputedStyle` | 31.6 / 1.97 | 86.2 / 1.98 | 282.0 / 1.97 | 859.1 / 1.98 |

30000 列分組那一項的 trace：一步約 1207 µs，lifecycle 佔 679（Layout 66.7、UpdateStyle 26.2、RebuildLayoutTree 18.9，其餘 566 沒有 span：推測是 layout 之後的幾趟走訪，沒有分開量），lifecycle 之外約 0.5 ms 是讀 inline 元素的 `offsetWidth`。這份 RFC 只拿掉後面這 0.5 ms。

### 座標

`position: fixed; top: 10px` 的盒子，裡面一個 block（有文字）、一個 `<span>`、一個 absolute 的盒子；另有一個被 transform 包住的 fixed 盒子當對照。`scrollTo(0, 500)` 之後的 `top`：

| | 捲之前，兩邊 | 捲之後，machopo | 捲之後，Chrome |
| --- | --: | --: | --: |
| fixed 盒子自己 | 10 | 10 | 10 |
| 裡面的 block | 10 | −490 | 10 |
| 裡面的 `<span>` | 39.5（Chrome 40） | −460.5 | 40 |
| 裡面的 absolute | 80 | −420 | 80 |
| 裡面的文字的 `Range` | 9.5（Chrome 10） | −490.5 | 10 |
| transform 裡的 fixed（對照） | 905 | 405 | 405 |

`elementFromPoint(25, 20)` 兩邊在捲動前後都找到裡面的 block：hit test 是對的，幾何是錯的。

同一頁對各元素 `dispatchEvent` 一個 `clientY` 是 20 的 `mousemove`：

| 目標 | 屬性 | 捲之前，兩邊 | 捲之後，machopo | 捲之後，Chrome |
| --- | --- | --: | --: | --: |
| fixed 盒子自己 | `offsetY` | 10 | 510 | 10 |
| 裡面的 block | `offsetY` | 10 | 510 | 10 |
| 裡面的 absolute | `offsetY` | −60 | 440 | −60 |
| fixed 盒子自己 | `layerY` | 10 | 10 | −490 |
| 裡面的 block | `layerY` | 10 | 10 | −490 |
| 裡面的 absolute | `layerY` | −60 | 440 | −560 |

`offsetY` 是 `ui_event` 的 `padding_origin` 一律減掉 viewport 的捲動。`layerY` 從 layer 的 client rect 算，所以 rect 修對之後 absolute 那一列會是 −60，仍然不是 Chrome 的 −560。

第二頁：fixed 盒子（`top: 10px`）裡一個 `<svg>` 和其中的 `<rect y=5>`；文件裡另一個 `<svg>`（700）當對照；捲動容器裡 `margin-top: 60px` 的盒子；300 的 `translate(0, 20px)`、400 的 `scale(2)`（原點在左上）；`border-top: 7px` 的 block 和它後面第二行的 `<span>`。事件的 `clientY` 是 100。

| 情況 | 讀的 | machopo | Chrome |
| --- | --- | --: | --: |
| 頁面捲 500，fixed 裡的 `<svg>` | rect 的 `top` | −490 | 10 |
| 同上，裡面的 `<rect>` | rect 的 `top` | −485 | 15 |
| 同上 | `getScreenCTM().f` | −490 | 10 |
| 頁面捲 500，文件裡的 `<rect>`（對照） | rect 的 `top` | 205 | 205 |
| 頁面捲 500，fixed 裡的 `<svg>` | `offsetY` | 590 | 90 |
| 容器捲 30，裡面的盒子 | `offsetY` | 40 | 70 |
| 沒捲，`translate(0, 20px)` | `offsetY` | −200 | −220 |
| 沒捲，`scale(2)` | `offsetY` | −300 | −150 |
| 沒捲，有邊框的 block（對照） | `offsetY` | −427 | −427 |
| 沒捲，第二行的 `<span>` | `offsetY` | −467.5 | −447 |

`scrollIntoView()`（範圍外，另外修；修之前量的）：頁面在 500 時對 `top: 10px` 的 fixed 盒子自己呼叫，machopo 的 `scrollY` 變成 10，Chrome 留在 500；對它的 child 呼叫兩邊都是 500。`bring_into_view` 碰到目標就回傳，走不到「fixed 盒子外面不捲」的那一段。

## 原型

2026-11-22，在 8e5dc20 的一份拷貝上做，不合併、沒有留在 repo：`LayoutBox` 加連結、`BoxTree` 加 inline 元素的索引，沒有改任何查詢。每次 `BoxTree::layout` 之後檢查：

- 每個盒子沿連結找到的 fragment、offset、fixed，和從根走訪到它時一樣。
- 走訪看得到的每個 fragment 剛好有一個盒子找得到它。
- 每個有元素的 fragment（不是 pseudo-element）的盒子就是 `elements` 裡那一個：一個元素沒有第二個盒子 fragment。
- 每個行上的 inline box 和 text run 所在的盒子都在索引裡。

`layout` 和 `paint` 全部的測試在這個檢查下通過。反向對照：拿掉 marker 的連結、把連結改成指 `LayoutBox::parent`，各自都讓這個檢查失敗。

根那一步另外驗證了兩件事：

- `layout` 和 `paint` 全部的測試裡，`fixed_to_viewport` 的 fragment 都在根的下一層（把這個檢查改成下兩層時會失敗，所以測試裡真的有這種盒子）。
- 照這條規則算「座標」兩頁裡 block 盒子在 viewport 的 `top`，捲 0 和 500：fixed 盒子 10、10，它的 block 10、10，absolute 80、80，transform 裡的 fixed 905、405，fixed 裡的 `<svg>` 10、10，文件裡的 `<svg>` 700、200。Chrome 有量的都一樣（只累加 offset，沒有接上查詢）。

原型**沒有**驗證：renderer 裡兩棵樹的一致性（沒跑 renderer 的測試、WPT 和情境）、幾個盒子的排序、sticky 和捲動套在路徑上的結果、inline 索引的成本、`local_point`、inline 和文字的座標。

沿連結找出 fragment、累加 offset 和 fixed 的時間（µs，中位數；每次配置一個 `Vec`；沒有 transform、sticky、捲動和答案的組成，所以不是做完的數字）：

| N | 中間那列 | 它的 `<span>` | `<body>` |
| --: | --: | --: | --: |
| 1000 | 0.031 | 0.033 | 0.027 |
| 10000 | 0.031 | 0.033 | 0.027 |
| 30000 | 0.035 | 0.037 | 0.032 |

連結是 `Option<(u32, u32)>`，12 bytes；`LayoutBox` 是 2544 bytes。

## 做法的比較

**A：盒子記連結。** 寫入在本來就走過 children 的迴圈裡，增量 layout 時只有重做的 fragment 的 children 會寫。查詢碰到的只有祖先。位置、捲動、sticky、transform 都不另存，所以捲動不需要讓任何東西失效。

**A′：沿 `parent` 累加 `Layout::location`。** 不用新欄位，但：

- out-of-flow 盒子的位置相對於 containing block，不是 `parent`。改成照 style 找 containing block 的話，要在查詢時重做一次 hoist 的規則。
- marker 的位置是 `marker_link` 算的，不在 `Layout::location`。
- 位置從此有兩個來源，box tree 上的和畫出來的 fragment 上的。

**A″：往上收集祖先，往下時每層在 `children` 裡比指標。** 不用新欄位，但平的 30000 列在 `<div>` 那一層要比 15000 次，正是量到最差的形狀。

**B：整頁的表。** layout 之後第一次查詢走一趟、建 `NodeId` → fragment 和位置。30000 列時光是走一趟就是 1 ms（上面 `client_rects` 的數字），還要加上插入。每次 layout 都作廢，所以「改一點、讀一次」的迴圈和現在一樣是 O(N)。存的若是位置，捲動也要作廢；存的若是路徑，等於 A 但每次整份重建。優點是程式集中在一處，不碰 box tree。

**C：property tree。** pre-paint 已經算了同樣的對應，但 script 讀幾何時只更新到 layout，要用它就得在查詢時先跑 pre-paint，它自己也走樹。Blink 的 `MapCoordinatesFastPath` 用 property tree，但只有呼叫的人指定 `kUseGeometryMapper` 時才走那條路；`getBoundingClientRect` 經過的 `AbsoluteQuads` 用預設的 flag，不走。

## Blink

讀的是 Chromium main 的 10ec29d7（2026-11-22）的原始碼，都在 `third_party/blink/renderer/core/` 底下：

| 問題 | Blink | machopo 現在 | A |
| --- | --- | --- | --- |
| 元素 → layout | `Node::GetLayoutObject()` | 從根走訪 | `elements`、`texts`、inline 的索引 |
| 往上照哪條鏈 | `LayoutObject::Container()`（containing block 的鏈） | 不往上 | fragment 的父層，同一條鏈 |
| 在父層的位置 | `LayoutBox::frame_location_`（`PhysicalLocation()`）：父層排完時 `UpdateChildLayoutBoxLocations` 寫進每個 child | `FragmentLink::offset` | 同一份，用索引拿；索引也在父層做 fragment 時寫 |
| 對應到 viewport | `LocalRectToAncestorQuad`、`MapLocalToAncestor` 沿 container | 走訪時累加出文件座標，事後扣捲動 | 沿路徑累加 |
| 頁面的捲動 | `OffsetFromScrollableContainer`：`LayoutView` 是鏈上最後一個捲動容器，fixed 盒子對它不減 | 照元素自己是不是 fixed | 同 Blink（根那一步） |
| `offsetY` | `AbsoluteToLocalPoint`、`MapAncestorToLocal`：同一條鏈反過來 | `element_box` 扣頁面的捲動 | 同 Blink（`local_point`） |
| inline 元素 | `InlineCursor::MoveTo`：`FragmentItemsContainer()` 裡從 `FirstInlineFragmentItemIndex()` 開始 | 走整棵 | 到盒子，再掃它的行 |
| `offsetWidth` | `Element::OffsetWidth` → 盒子的大小 | 連 `scrollable_overflow` 一起算 | 盒子的大小（S1） |
| `scrollWidth` | `LayoutBox::ScrollWidth` 讀存好的 overflow | 每次走子樹 | 每次走子樹（S1），之後再快取 |

## 參考資料

- `dom/element.cc`：`OffsetWidth`、`ClientQuads`、`GetBoundingClientRectNoLifecycleUpdate`、`scrollWidth`。
- `layout/layout_box.cc`、`layout/layout_box.h`：`QuadsInAncestorInternal`、`PhysicalLocation`、`OffsetFromContainerInternal`、`ScrollWidth`。
- `layout/block_node.cc`、`layout/layout_box_utils.cc`：前者在父層排完時呼叫後者的 `UpdateChildLayoutBoxLocations`，寫 child 的位置。
- `layout/map_coordinates_flags.h`：`kUseGeometryMapper` 的前提。
- `layout/layout_object.cc`、`layout/layout_object.h`：`MapLocalToAncestor`、`MapAncestorToLocal`、`OffsetFromScrollableContainer`、`MapCoordinatesFastPath`、`Container`、`FragmentItemsContainer`。
- `layout/layout_inline.cc`、`layout/inline/inline_cursor.cc`、`layout/inline/fragment_items.cc`：inline 元素從第一個 item 開始找。
- `events/mouse_event.cc`：`ComputeRelativePosition`（`offsetY`、`layerY`）。
- `css/style_traversal_root.h`：「之後的可能」1 的 `StyleTraversalRoot`。
- CSSOM View：<https://drafts.csswg.org/cssom-view/>。
