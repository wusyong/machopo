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

第二頁：fixed 盒子（`top: 10px`）裡一個 `<svg>` 和其中的 `<rect y=5>`；文件裡另一個 `<svg>`（700）當對照。

| 情況 | 讀的 | machopo | Chrome |
| --- | --- | --: | --: |
| 頁面捲 500，fixed 裡的 `<svg>` | rect 的 `top` | −490 | 10 |
| 同上，裡面的 `<rect>` | rect 的 `top` | −485 | 15 |
| 同上 | `getScreenCTM().f` | −490 | 10 |
| 頁面捲 500，文件裡的 `<rect>`（對照） | rect 的 `top` | 205 | 205 |

`scrollIntoView()`（範圍外，另外修；修之前量的）：頁面在 500 時對 `top: 10px` 的 fixed 盒子自己呼叫，machopo 的 `scrollY` 變成 10，Chrome 留在 500；對它的 child 呼叫兩邊都是 500。`bring_into_view` 碰到目標就回傳，走不到「fixed 盒子外面不捲」的那一段。

### 縮小範圍時的重量

2026-11-22，machopo a2f1722 的 `perf` binary、Chrome 158。每一列讀一次（ms）：

| 列數 | `offsetHeight`，machopo | Chrome | `getBoundingClientRect()`，machopo | Chrome |
| --: | --: | --: | --: | --: |
| 100 | 0.05 | 0.023 | 0.15 | 0.042 |
| 1000 | 2.39 | 0.220 | 10.78 | 0.409 |
| 3000 | 21.80 | 0.639 | 95.50 | 1.226 |
| 10000 | 339.60 | 2.215 | 1329.80 | 4.028 |

根那一步用隨機頁面反證：六個 seed 各 120 棵樹（巢狀的 fixed、absolute、sticky、relative，transform、contain、filter、捲過的容器、SVG），頁面捲 500，`getClientRects()` 的 `top`。「套上」是把 machopo 現在回的值照那一步補回多扣的捲動，路徑是不是到 fixed 照 containing block 的鏈算：是對規則的驗證，**不是實作**。

| 元素 | 個數 | 現在和 Chrome 相同 | 套上之後相同 |
| --- | --: | --: | --: |
| 路徑經過 fixed 到 viewport 的盒子 | 3876 | 571 | 3858 |
| 同上，SVG 裡的圖形 | 364 | 0 | 362 |
| 其他盒子（對照） | 9906 | 9805 | 9805 |
| 同上，SVG 裡的圖形（對照） | 922 | 909 | 909 |

套上之後還不同的 134 個，在頁面沒捲時就差同樣多：和這一步無關，不用特例。

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

原型**沒有**驗證：renderer 裡兩棵樹的一致性（沒跑 renderer 的測試、WPT 和情境）、幾個盒子的排序、sticky 和捲動套在路徑上的結果、inline 索引的成本、inline 和文字的座標。

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
| `offsetY` | `AbsoluteToLocalPoint`、`MapAncestorToLocal`：同一條鏈反過來 | `element_box` 扣頁面的捲動 | 不改（移出範圍） |
| inline 元素 | `InlineCursor::MoveTo`：`FragmentItemsContainer()` 裡從 `FirstInlineFragmentItemIndex()` 開始 | 走整棵 | 到盒子，再掃它的行 |
| `offsetWidth` | `Element::OffsetWidth` → 盒子的大小 | 連 `scrollable_overflow` 一起算 | 盒子的大小（S1） |
| `scrollWidth` | `LayoutBox::ScrollWidth` 讀存好的 overflow | 每次走子樹 | 每次走子樹（S1），之後再快取 |

## 移出範圍的滑鼠事件座標

2026-11-22 移出範圍（README 的「已決定的事項」11），留給之後的 RFC：選過的不用重選，但**動手之前要先重驗**。下面的百分比都是規則的模型對 Chrome 的，沒有實作過；這些規則寫下之後推翻過好幾次。「第幾階段」和「一起修」說的是縮小之前的範圍。

### 決定

編號是移出之前在 README 的「已決定的事項」裡的。1 移出的那一半：`offsetX`、`offsetY` 用根那一步同一個對應的反函數。反函數經過 transform 是三個引擎的做法（讀原始碼）：CSSOM View 的字面是不看元素和祖先的 transform，三個引擎都把它當成元素自己沒有變形過的座標。

4. **`layerX`、`layerY` 整個照 Chrome**（下面的「`layerX`、`layerY`」）：`pageX`、`pageY` − 頁面的捲動 − layer 的原點對應到 viewport 的位置；layer 是 inline 元素時原點是它所在區塊的（不減邊框）。所以 transform 過的 layer、inline 的 layer、沒有 `view` 的事件三處都會變，fixed 的 layer 上真的輸入維持 10。不選只照前兩處（沒有 `view` 的要多一個特例）和都不動（inline 的 `layerX` 和 `offsetX` 兩套原點）。原本決定「留著 machopo 的 10、不照 Chrome 的 −490」：−490 是量測時派發的事件沒有 `view` 造成的，照 Chrome 比較貴也不成立。決定時列的三處不完整：它們是原點和捲動怎麼算，**哪一個盒子是 layer** 另有四處不同，決定時沒有看到，在 9 補齊。
5. **往回對應碰到不可逆的 transform 時略過那一層**，連盒子在容器裡的位移一起，照 Chrome（下面的「往回對應」：`scaleY(0)` 的盒子 −100）。不選只略過 transform、位移照算（−187）：和 Chrome 不同，每一層還要把位移和 transform 拆開算。這是跟著 Chrome 選的，不是各引擎一致的行為：Firefox 照原始碼回 0、0（沒有實跑）。
6. **inline 元素的 `offsetX`、`offsetY` 照 Chrome**：從 Blink 會給它的區塊的 border box 起算，再減它自己上和左的邊框（下面的「inline 元素的 `offsetX`、`offsetY`」）。理由是相容：WebKit 的原始碼是同樣的算法（沒有實跑），WPT 的 `shadow-dom/MouseEvent-prototype-offsetX-offsetY.html` 要的也是它。**不是因為規範或成本**：CSSOM View 說的是目標自己的 padding edge，這個做法不照它；第一個行框也只要掃到第一個框，不貴（決定時寫的「規範沒說」和「最省」是錯的）。不選行框的聯集（machopo 原本的：和各引擎都不同）和第一個行框（較接近規範，Firefox 的原始碼是這樣，但和 Chrome、Safari、那份 WPT 不同）。machopo 的 box tree 怎麼給出那個區塊見 7。
7. **inline 元素的區塊分四種情況**（重新決定；下面的「區塊的四種情況」）。「自己的 block」是容器在流裡、不是從 inline 裡切出來的 block child（float 和 out-of-flow 不算，`display: contents` 裡的算）；「一串」是兩個自己的 block 之間的內容：
   - **7a 容器沒有自己的 block**：從容器的 border box 起算，不管 machopo 切了幾個匿名 block（flex、grid 的匿名 item 除外：它自己就是區塊）。
   - **7b 有，目標那一串沒被切開、目標有行**：從它所在的匿名 block 起算。
   - **7c 有，目標那一串被切開**：x 是容器的 content box，y 是前一個自己的 block 排版位置（不含 `relative` 的位移）的 margin 下緣；沒有前一個時是容器的 content box 上緣。
   - **7d 有，目標沒有行**（例如和 block 並排、只有 `::after` 的連結）：同 7c。

   隨機頁面上 7a、7b 全對，7c 是 83% 到 98%，7d 是 95% 到 100%（模型對 Chrome，不是實作後的結果）。代價：box tree 分辨自己的和切出來的 block（一兩個 bool，大小還沒量），7c、7d 往前找 block 是 O(兄弟數)（Blink 有現成的匿名 block，不用找），7d 要替沒有盒子的元素找到它在容器裡的位置，還沒寫過。不選 7c 用 B1（記下那一串從哪個 child 開始：margin 多的頁較準，其他較差，而且多一套只給它用的紀錄）和只修 7a、7b（7d 是整張卡片可點的常見寫法，現在退成 `pageX`）。同一天先決定過全部用 B1：根據是手挑的 19 頁，而且理由「`<a>` 裡包 `<div>` 也錯」不成立（滑鼠的目標是裡面的 `<div>`）。
8. **特別的事件目標：修滑鼠點得到的和 document**（重新決定）：
   - **SVG 裡的元素**：在最外層 `<svg>` 的排版裡有節點時（`svg_geometry` 的 `find` 找得到：圖形、`<g>`、`<text>` 和裡面的 `<tspan>`、`<use>`、`<defs>` 裡的圖形……，也就是 hit test 打得到的和藏起來但有排版的容器）從那個 `<svg>` 起算，再減它的邊框；沒有時照舊是 `pageX`。隨機的 SVG 樹上修好 Chrome 那樣回的 92.7%，現在對的弄錯 0.2%（下面的「SVG 的邊界」；machopo 那一側是照程式寫的模型）。
   - **空的 inline 元素**：沒有自己的規則，就是 7a 和 7d。
   - **document**：viewport。
   - **記成已知限制**：text node 和 `<br>`（真的輸入測過的都打不到，只有 script 派發的事件遇得到）；Chrome 有 LayoutObject、machopo 沒有排版節點的 SVG 元素（漸層、`<clipPath>`、`<mask>`、`<marker>`、`<pattern>`、`<filter>`，和還不畫的 `<image>`、`<textPath>`、`<foreignObject>`）；machopo 畫了、Chrome 不畫的（`<switch>` 選錯的 child、巢狀的 `<a>`：它們現在剛好和 Chrome 一樣是 `pageX`，會變成不同，根源是 SVG 的排版）。

   不選五種都修（text node 和 `<br>` 要替沒有盒子的節點找位置，被收合掉的空白文字不在 `texts` 裡，還要多存一份）和不修 document（只省一個判斷）。SVG 也不用元素名稱判斷：「除了幾種元素都從 `<svg>` 起算」在同一批樹上把現在對的弄錯 97.5%。同一天先決定過五種都修：沒有先看哪些真的打得到，經過 `texts` 的做法也漏了收合掉的空白。
9. **哪一個盒子是 layer 也照 Chrome（L1）**，補齊 4：root element 一律是 layer；目標是 document 時是 viewport；inline 元素只有 `transform` 時不是 layer；沒有盒子的目標沿 DOM 的 parent 往上，不穿過 shadow root。成本可忽略。不選只做前兩處和都記成限制。`will-change`、`perspective` 這些 machopo 沒有的 property 造成的 layer 不在內，等那些 property。
10. **`MouseEvent` 的座標不取整，記成已知限制（R1）**：Chrome 把 `clientX`、`screenX`、`pageX` 捨去、`offsetX` 四捨五入，`PointerEvent` 除了 `click`、`auxclick`、`contextmenu` 都留著小數。等有 `PointerEvent` 時一起做。不選整組照 Chrome（改到這份沒碰的屬性，而且在有 `PointerEvent` 之前頁面拿不到有小數的座標）和只取整 `offsetX`、`offsetY`（和同一個事件的 `clientX` 不一致）。

### 當時的設計

`offsetX`、`offsetY` 照 Blink 的 `AbsoluteToLocalPoint` 用同一個對應的反函數：`FrameLayout` 多一個 `local_point(node, point)`，把 `clientX`、`clientY` 換到 border box 的座標，`dom_api` 再減邊框；`padding_origin` 就不用自己扣捲動，transform 和捲動容器也因此一起對了。inline 元素照 Chrome：點在**Blink 會給它的區塊**的 border box 裡的位置，再減它自己上和左的邊框，不看它的行框和它自己 `relative` 的位移（「決定」的 6）。那個區塊不一定是 machopo 裝著它的行的盒子：inline 裡夾 block 時 machopo 切出匿名 block，Blink 不切，所以 box tree 分辨容器自己的 block 和切出來的，照四種情況找區塊（「決定」的 7）。空的 inline 元素也照這四種情況；SVG 裡有排版節點的元素先換成最外層的 `<svg>`，document 是 viewport；text node 和 `<br>` 照舊是 `pageX`（「決定」的 8）。transform 不可逆時照 Blink 的 `TransformState::FlattenWithTransform`：點原樣通過那一層，連盒子在容器裡的位移一起略過（「決定」的 5），所以往回對應是一層一層反推，不能只反轉累積的矩陣。

box tree 因此要分辨切出來的 block（一兩個 bool），多一個節點到它所在區塊的查詢，`docs/architecture.md` 也要記下「把 block 從 inline 裡切出來的新路要標出它是切出來的」。

要守的測試：

- fixed、捲動容器、`translate`、`scale` 的 `offsetX`、`offsetY`；7 的四種情況（連同插入、拿掉 block 之後的增量更新）；SVG 裡有和沒有排版節點的元素；`::after` 蓋滿的空連結（連同真的輸入）；document；9 的四處。
- WPT：`shadow-dom/MouseEvent-prototype-offsetX-offsetY.html` 的 inline 目標要的就是 Chrome 的原點，現在兩個 subtest 是預期的 FAIL；`uievents/mouse/layer-coords-transform.html` 改 layer 之後要照樣通過；`css/cssom-view/mouseEvent-offsetXY-svg.html` 只檢查 ≥50，守不住 SVG 的原點。

### 往回對應

「實測」的「座標」第一頁：

同一頁對各元素 `dispatchEvent` 一個 `clientY` 是 20、**沒有 `view`** 的 `mousemove`（`layerY` 因此不是真的輸入時的答案，見「`layerX`、`layerY`」）：

| 目標 | 屬性 | 捲之前，兩邊 | 捲之後，machopo | 捲之後，Chrome |
| --- | --- | --: | --: | --: |
| fixed 盒子自己 | `offsetY` | 10 | 510 | 10 |
| 裡面的 block | `offsetY` | 10 | 510 | 10 |
| 裡面的 absolute | `offsetY` | −60 | 440 | −60 |
| fixed 盒子自己 | `layerY` | 10 | 10 | −490 |
| 裡面的 block | `layerY` | 10 | 10 | −490 |
| 裡面的 absolute | `layerY` | −60 | 440 | −560 |

`offsetY` 是 `ui_event` 的 `padding_origin` 一律減掉 viewport 的捲動。`layerY` 從 layer 的 client rect 算，所以 rect 修對之後 absolute 那一列會是 −60：和 Chrome 對有 `view` 的事件的答案一樣（三列是 10、10、−60）。其他表的 `offsetX`、`offsetY` 用有 `view` 的事件重量過，一個都沒變：Blink 從 `absolute_location_`（就是 client 座標）算它們。

第二頁（「實測」的「座標」）另外有捲動容器裡 `margin-top: 60px` 的盒子；300 的 `translate(0, 20px)`、400 的 `scale(2)`（原點在左上）；`border-top: 7px` 的 block 和它後面第二行的 `<span>`。`offsetY`，事件的 `clientY` 是 100：

| 情況 | 讀的 | machopo | Chrome |
| --- | --- | --: | --: |
| 頁面捲 500，fixed 裡的 `<svg>` | `offsetY` | 590 | 90 |
| 容器捲 30，裡面的盒子 | `offsetY` | 40 | 70 |
| 沒捲，`translate(0, 20px)` | `offsetY` | −200 | −220 |
| 沒捲，`scale(2)` | `offsetY` | −300 | −150 |
| 沒捲，有邊框的 block（對照） | `offsetY` | −427 | −427 |
| 沒捲，第二行的 `<span>` | `offsetY` | −467.5 | −447 |

不可逆的 transform（2026-11-22，Chrome 158 和 a2f1722）：`clientX` 33、`clientY` 100，`offsetX offsetY`：

| 情況 | machopo | Chrome |
| --- | --: | --: |
| `scale(0)` 的盒子自己，在 body 的原點 | 33 −107 | 33 −107 |
| `scaleY(0)`、離 body 的原點往下 87 的盒子 | 33 −187 | 33 −100 |
| 它的 child（`margin-left: 40px`） | −7 −187 | −7 −100 |
| 後兩個，頁面捲 150 | 33 −37、−7 −37 | 33 50、−7 50 |

Blink 的 `FlattenWithTransform` 求不出反矩陣時點不動，而那一個矩陣（`GetTransformFromContainer`）已經含著盒子在容器裡的位移，所以位移也一起沒算。

### inline 元素的 `offsetX`、`offsetY`

2026-11-22，Chrome 158 和 a2f1722，對元素 `dispatchEvent` 一個 `MouseEvent`。Chrome 的規則（`ComputeRelativePosition`）：`AbsoluteToLocalPoint` 給出點在它所在區塊的 border box 裡的位置，再減元素自己的 `BorderOutsets().Offset()`。第一列 `clientX` 100、`clientY` 10，其他的兩個都是 100、頁面捲 50：

| 情況 | machopo | Chrome |
| --- | --: | --: |
| 從第一行中間開始、斷行（第一個行框是 21.02） | 87 9.85 | 100 10 |
| 區塊有 padding 13 和 border 7：不減區塊的 | 17.01 149.85 | 70 150 |
| 自己有 border 5、3：減自己的 | 67.01 132.85 | 95 127 |
| 自己是 `relative`：不看它的位移 | 58.01 105.85 | 100 110 |
| 區塊有 `translate`、`scale(2)`：經過對應 | 78.01 89.85 | 42 45 |
| 在捲過的容器裡：經過捲動 | 67.01 29.85 | 120 40 |
| 區塊有別的 block child：從匿名區塊起算 | 24.02 −150.15 | 79 −150 |
| 在 fixed 盒子裡 | −132.99 −150.15 | −100 −200 |
| 包著 block | 79 −10.15 | 79 −10 |
| 空的 | 57.01 −170 | 79 −170 |

「所在區塊」是 Blink 的：block 在 inline 裡時 Blink 不切容器（`LayoutInline::AddChildAsBlockInInline`），只有容器自己有 block child 時才有匿名 block（`LayoutBlockFlow::AddChild`）。machopo 兩種都切。隨機頁面上照這個規則算的和 Chrome 相同的是 32978 個目標裡的 32929 個，其餘都是 inside 的 list marker 那一串。

#### 區塊的四種情況

「決定」的 7。一次性的腳本在 Chrome 158 裡產生隨機的 inline、block、float、out-of-flow 的組合（各種容器、margin、邊框、捲動），和 `PointerEvent` 沒取整的 `offsetX`、`offsetY` 比。只算 `elementFromPoint` 打得到的 inline 元素。machopo 會怎麼排是用 Chrome 的排版推算的，**沒有實作、沒有跑**。四批各三個 seed、每個 5000 頁；後三批是找到做法之後才跑的：

| 情況 | 做法 | 找做法的那批 | 沒看過的 seed | margin 多 | 沒有 margin |
| --- | --- | --: | --: | --: | --: |
| 7a | 容器的 border box | 4640/4640 | 4421/4421 | 4674/4674 | 4478/4478 |
| 7b | 它所在的匿名 block | 1020/1020 | 1000/1000 | 1000/1000 | 991/991 |
| 7c | **前一個自己的 block 的下緣** | 1054/1141 | 1023/1102 | 944/1138 | 1052/1074 |
| 7c | B1：那一串的開頭 | 984/1141 | 944/1102 | 1010/1138 | 962/1074 |
| 7c | B2：它所在的匿名 block | 617/1141 | 605/1102 | 601/1138 | 604/1074 |
| 7d | **前一個自己的 block 的下緣** | 104/108 | 76/76 | 96/100 | 93/98 |
| 7d | B1：那一串的開頭 | 26/108 | 15/76 | 26/100 | 18/98 |
| 7d | B2、現狀（`pageX`） | 0/108 | 0/76 | 0/100 | 0/98 |

真的滑鼠輸入也量了每一種（Chrome 158、machopo a2f1722；容器 padding 13、border 7、一行 20；值是原點離容器 border box 多遠）：

| 情況 | 內容 | machopo | Chrome |
| --- | --- | --: | --: |
| 7a | `<a>text<div></div></a>` | 20 20.1 | 0 0 |
| 7a | 切開的 inline 後面的 `<b>` | 20 60.1 | 0 0 |
| 7b | `<li><a>Menu</a><ul>` | 20 20.1 | 20 20 |
| 7c | `<p></p><a><div></div>text</a>` | 20 60.1 | 20 40 |
| 7d | `<p></p><a>`，`::after` 蓋滿容器 | `pageX` | 20 40 |
| 對照 | `<a><div></div></a>`：目標是 `<div>` | 20 20 | 20 20 |

模型沒涵蓋匿名 table、inline 當 containing block、writing mode。`<fieldset>` 裡的 inline 元素 Chrome 從它匿名的內容盒子起算，machopo 沒有那個盒子。

`<button>` 裡的 inline 元素 Chrome 從 button 的 border box 起算。machopo 把行放在 button 自己的盒子裡（`center_content` 只移動內容），所以不用另外處理（讀程式，還沒跑）。

在行上沒有盒子的目標（「決定」的 8），`clientX` 160、`clientY` 100、頁面捲 70，`offsetX offsetY`：

| 目標 | machopo | Chrome | 當時決定 |
| --- | --: | --: | --- |
| SVG 的 `<rect>` | 160 170 | 48 −253，和最外層的 `<svg>` 相同 | 修 |
| text node | 160 170 | 60 −180 | 記成限制 |
| `<br>` | 160 170 | 60 −180 | 記成限制 |
| 空的 inline 元素 | 160 170 | 60 −730 | 修（7a、7d） |
| document | 160 170 | 160 100 | 修 |
| `display: none`（對照） | 160 170 | 160 170 | 不用改 |

真的滑鼠輸入（Chrome 158、machopo a2f1722）：SVG 的 `<rect>`、`<g>` 裡的、`<text>`、內層的 `<svg>`、`<use>` 都打得到，Chrome 從最外層的 `<svg>` 起算，machopo 都是 `pageX`；`::after` 蓋滿容器的空連結也打得到（「區塊的四種情況」的 7d）；測到的 text node 和 `<br>` 都打不到，目標是它們外面的元素。

#### SVG 的邊界

哪些 SVG 元素 Chrome 從最外層的 `<svg>` 起算不能照元素名稱列：machopo 認得的 63 種加一種不認得的，直接放在 `<svg>` 裡就有 40 種是 `pageX`（動畫、filter 的 primitive、不在 `<text>` 裡的 `<tspan>`……），放進別的 parent 又不同，`display: none` 的圖形也是 `pageX`（`<g>` 不是）。

隨機的 SVG 樹（Chrome 158、script 派發的事件、四個 seed 各 1500 棵、共 132502 個元素；machopo 有沒有節點是照 `capture_node` 和 `find` 寫的模型算的）：

| 規則 | Chrome 從 `<svg>` 起算的 12505 個，修好 | Chrome 是 `pageX` 的 119997 個，弄錯 |
| --- | --: | --: |
| **有排版節點** | 11595 | 244 |
| 除了 `<title>` 等六種都算 | 12505 | 116980 |
| 現狀 | 0 | 0 |

兩邊的差異用 machopo a2f1722 的 binary 各抽一頁確認過：

- machopo 畫了、打得到，Chrome 沒有：`<switch>` 第一個條件成立的 child 是 `display: none`、`<title>` 或不認得的元素時，machopo 改畫下一個；巢狀的 `<a>`。
- Chrome 畫了、打得到，machopo 沒有：`<image>`、`<textPath>`、`<foreignObject>`。machopo 的目標因此是 `<svg>` 或它底下的東西。

`<switch>` 不能單獨照 Chrome 改：machopo 還不畫 `<foreignObject>` 和 `<image>`，現在靠「第一個畫得出來的」顯示它們後面備用的內容（仿照 draw.io 的寫法等四頁都量到），改成第一個條件成立的，備用內容就會消失（讀程式推的，沒有改來試）。

`<video>` 也不同（machopo 160 170、Chrome 51 −80），但那是 machopo 沒有實作 video（`HTMLElement`、沒有 replaced 的盒子），不是座標。

Chrome 另外把 `MouseEvent` 的 `offsetX`、`offsetY` 取整（`std::round`），所以上面它都是整數。和原點、對應都無關（「決定」的 10）。`clientX` 160.7、`clientY` 100.3，`clientX pageX offsetX`：

| 事件 | machopo | Chrome |
| --- | --: | --: |
| `MouseEvent`（`mousemove`、`click`） | 160.7 160.7 53.7 | 160 160 54 |
| `PointerEvent` 的 `pointermove` | 沒有這個 interface | 160.7 160.7 53.7 |
| `PointerEvent` 的 `click` | 同上 | 160 160 54 |

### `layerX`、`layerY`

2026-11-22，Chrome 158 和 a2f1722，頁面捲 500、容器捲 30。三種事件：沒有 `view` 的 `MouseEvent`、`view: window` 的、DevTools 的 `Input.dispatchMouseEvent`（真的輸入）；後兩種兩邊各自都相同，所以併成一欄。`layerX layerY`：

| 目標 | machopo，三種都一樣 | Chrome，有 `view` 和真的輸入 | Chrome，沒有 `view` |
| --- | --: | --: | --: |
| fixed 盒子自己 | 10 140 | 10 140 | 10 −360 |
| 它的 block | 10 10 | 10 10 | 10 −490 |
| 它裡面的 absolute（rect 修對之前） | 10 505 | 10 5 | 10 −495 |
| 文件裡的 absolute | 20 30 | 20 30 | 20 −470 |
| `rotate(30deg)` 的 absolute | 50 45 | 30 45 | 30 −455 |
| 捲動容器裡的 relative | 30 70 | 30 70 | 30 −430 |
| relative 的 inline 元素 | 6 7 | 70 12 | 70 −488 |

`html` 沒有 margin 時，沒有別的 layer 的目標兩邊都是 `pageX`、`pageY`（machopo 沒有 `view` 時 620 350、有時 620 850，和 Chrome 一樣）。Blink：`InitCoordinates` 只在有 `view` 時把捲動加進 `page_y_`；`ComputeRelativePosition` 再一律 `DocumentToFrame`（減捲動），減掉 layer 的 `LocalToAbsolutePoint(0, 0)`。

哪一個盒子是 layer（「決定」的 9）：Blink 從目標往 `parentNode` 找第一個有 `LayoutObject` 的，再取它的 `EnclosingLayer()->EnclosingSelfPaintingLayer()`。`clientX` 160、`clientY` 100、頁面捲 70，有 `view`：

| 情況 | machopo | Chrome |
| --- | --: | --: |
| `html` 有 margin 11、17，目標是它自己或裡面一般的 block | 160 170 | 143 159 |
| 目標是 document | 160 170 | 160 100 |
| 只有 `transform` 的 inline 元素裡面 | 29 109 | 60 110 |
| shadow root 底下直接一個 `display: none` 的元素 | 60 −6620 | 160 170 |
| `opacity`、`filter`、`isolation`、`clip-path`、`relative`、`sticky` 的 inline 元素（對照：是 layer，原點照「決定」的 4；只列 `layerX`） | 29 | 51 |
| `z-index`、`contain: paint` 的 inline 元素（對照：不是 layer；只列 `layerX`） | 60 | 60 |

block 上和 Chrome 不同的另外都是 machopo 沒有的 property（`CSS.supports` 是 false）：`will-change`、`mix-blend-mode`、`mask-image`、`perspective`、`backface-visibility`、`transform-style`、`content-visibility`、`view-transition-name`、`rotate`、`translate`、`scale`、`backdrop-filter`。

## 參考資料

- `dom/element.cc`：`OffsetWidth`、`ClientQuads`、`GetBoundingClientRectNoLifecycleUpdate`、`scrollWidth`。
- `layout/layout_box.cc`、`layout/layout_box.h`：`QuadsInAncestorInternal`、`PhysicalLocation`、`OffsetFromContainerInternal`、`ScrollWidth`。
- `layout/block_node.cc`、`layout/layout_box_utils.cc`：前者在父層排完時呼叫後者的 `UpdateChildLayoutBoxLocations`，寫 child 的位置。
- `layout/map_coordinates_flags.h`：`kUseGeometryMapper` 的前提。
- `layout/layout_object.cc`、`layout/layout_object.h`：`MapLocalToAncestor`、`MapAncestorToLocal`、`OffsetFromScrollableContainer`、`MapCoordinatesFastPath`、`Container`、`FragmentItemsContainer`。
- `layout/layout_inline.cc`、`layout/inline/inline_cursor.cc`、`layout/inline/fragment_items.cc`：inline 元素從第一個 item 開始找。
- `events/mouse_event.cc`、`events/pointer_event.cc`：`ComputeRelativePosition`（`offsetY`、`layerY`）、`FindTargetLayoutObject`（SVG）、哪些事件取整。
- `layout/layout_block_flow.cc`、`layout/layout_inline.cc`：`AddChild`（匿名 block 什麼時候有）；後者和 `layout/layout_box.cc` 的 `LayerTypeRequired`（什麼是 layer）。
- `css/style_traversal_root.h`：「之後的可能」1 的 `StyleTraversalRoot`。
- CSSOM View：<https://drafts.csswg.org/cssom-view/>。
