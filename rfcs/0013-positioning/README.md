# RFC 0013：`position`

- 開始日期：2026-10-07
- 狀態：✅ 已完成：2026-10-09（A2、Z1、F1、S1，三段都做完：relative 和 `z-index`、absolute、fixed；2026-10-07 接受、做完第 1 段之後退回草稿、實作反轉，2026-10-09 重新確認設計、接回第 1 段，再做完第 2、3 段。`sticky` 另一份 RFC。驗收見 `progress.md`，限制見 `docs/limitations.md`）
- 細節：`details.md`

## 摘要

支援 `position: relative`、`absolute`、`fixed` 和 `top` / `right` / `bottom` / `left`，以及有 `position` 的元素上的 `z-index`。絕對定位的 box 照 Chromium 的 LayoutNG 由它的包含塊（最近的有 `position` 的祖先）排，不是 taffy 預設的父 box；paint 照 CSS 2 附錄 E 的第 8 步畫有 `position` 的元素；`position: fixed` 不跟著 viewport 捲動，compositor 捲動時不動它。`sticky` 不在這份裡（「已決定的事項」第 1 到 4 條）。

## 動機

**現況**：`style` 不認得 `position` 和 `top` / `right` / `bottom` / `left`，宣告被丟掉，所以有 `position` 的元素照一般的 flow 排（`docs/limitations.md` 的「Style」和「Layout」）。`z-index` 只對 flex 和 grid item 生效（RFC 0003 第 9 條）。paint 的 stacking context 只來自 `opacity`、`transform` 和 item 的 `z-index`。

**問題**：`position` 是真實網站排錯最常見的原因之一：
- 下拉選單、tooltip、徽章、卡片角落的按鈕：`position: absolute` 放在 `position: relative` 的容器裡。
- 固定在畫面上方的導覽列、浮動的按鈕、cookie 橫幅：`position: fixed`。
- 疊在一起的層：有 `position` 的元素上的 `z-index`。

這些在 kun 上會排成一直往下的 block，或疊錯順序；固定的導覽列跟著頁面捲走。

**要解決的 `docs/limitations.md` 裡的限制**：「Layout」的沒有 `position`；「Style」的 `getComputedStyle` 的 resolved value 沒有 `top` / `left` 等；「Bindings 和 DOM API」的 `offsetParent` 只會是 `body`、`table`、`td`、`th`；「Compositor、layer 和 input」的範圍的不變式在加上 `position: fixed` 時要重新檢討。`rfcs/` 以外的文件不寫這份的編號，做完時照這份清單刪掉或改寫。

目標：
- `position: relative`：照常排版之後，照 `top` / `left` 等位移，不影響其他 box。
- `position: absolute`：從 flow 拿掉，對包含塊（最近的 `position` 不是 `static` 的祖先的 padding box；沒有的話是初始包含塊）定位和定大小；`auto` 的位置用 static position。
- `position: fixed`：包含塊是 viewport，捲動時不動；compositor 捲動 viewport 時它照樣不動。
- 有 `position` 的元素的 `z-index`、建立 stacking context；附錄 E 第 8 步的順序（`z-index: auto` 的有 `position` 的元素照樹的順序畫在一般內容之後）。
- hit test 照 paint 的順序；P4 的局部重排照樣成立。
- `css/CSS2/positioning`、`css/css-position` 的 WPT reftest 子集。

不在範圍內：
- `position: sticky`：要 compositor 照捲動位置算位移（Chromium 的 sticky constraints），另外寫一份。
- float、table、`calc()`、`inset` 以外的邏輯屬性（`inset-inline-start` 等）。
- 絕對定位的元素用 `anchor()` 定位（CSS Anchor Positioning）。

## 設計概要

給網頁的作者：`position: relative` 的容器裡的 `position: absolute` 元素，照 `top` / `right` / `bottom` / `left` 放在容器的 padding box 裡；`position: fixed` 的導覽列留在畫面上，捲動時不動；`z-index` 對有 `position` 的元素有作用，數字大的疊在上面。

給之後改 `layout` 和 `paint` 的人：
- box tree 照舊是 DOM 結構，out-of-flow 的盒子也還是它 DOM 父元素那個盒子的 child，但**交給 taffy 的 child 清單**把它移到包含塊底下（第 11 條），所以 taffy 的絕對定位算法就是對包含塊解的，fragment 也落在包含塊的 fragment 底下（Chromium 的 `OutOfFlowLayoutPart` 要的結果）。原本的位置留一個 0×0 的佔位盒子，它在 flow 裡的位置就是 static position。
- paint 和 hit test 走 fragment tree：有 `position` 的元素和 stacking context 一樣從一般的 flow 拿出來，照附錄 E 排在後面。
- `position: fixed` 的盒子掛到它那個 frame 的 `LayoutView`；主 frame 的 fixed 內容自成一層、帶「釘在 viewport」的標記，compositor 畫的時候把捲動位移加回去（第 16 條）。

## 詳細設計

- **style**：`position`（`static`、`relative`、`absolute`、`fixed`；`sticky` 丟掉，第 4 條）、`top` / `right` / `bottom` / `left` 和 `inset` shorthand（長度、百分比、`auto`）。`position: absolute` 和 `fixed` 的元素被 blockify（css-display，和 RFC 0003 的 item 一樣在 style 做）。`z-index` 對 `position` 不是 `static` 的元素生效（`effective_z_index`，RFC 0003 第 9 條的延伸）。`StyleDifference`：進出 out-of-flow（`is_out_of_flow()` 變了）是 Reattach——父盒子交給 taffy 的 child 清單和佔位盒子都要重建；其他的是 Layout，`relative` 的位移只重排那個 box。
- **relative**：taffy 的 `Position::Relative` 在 block、flex、grid 排版後都套上位移，不影響其他 box。taffy 的預設 position 就是 `Relative`，所以只有 `position: relative` 的 box 把 `top` / `left` 這些交給 taffy，`static` 的不給。atomic inline 的位移由 IFC 放它時加上。inline 元素（一行裡的 inline box）只有位移：它在每一行的 inline box 和裡面的內容跟著移動，照原本 line 的順序畫，`z-index` 不生效（第 5 條）。
- **inline 元素的位移**（第 5 條）：`InlineContent::fragments` 走每一行時，開著的 inline 元素的位移加總起來（巢狀的累加）：它在這一行的 inline box、文字和裡面的 atomic inline 跟著移動。inline box 的範圍先照沒移動的位置算，再整個移動。百分比的 `left` / `right` 對 IFC 的寬度，`top` / `bottom` 對它確定的高度，不確定時是 `auto`。
- **out-of-flow 的排版**（第 1、11 條）：每次 layout 從 `LayoutView` 走一遍，重新決定每個盒子交給 taffy 的 child 清單——in-flow 的照 DOM 順序，out-of-flow 的掛到它的包含塊（`absolute` 找最近的 `is_containing_block()` 祖先，`fixed` 一律是那個 frame 的 `LayoutView`，第 17 條），原本的位置換成 0×0、沒有 margin 的佔位盒子。重算而不是增量維護，所以盒子被拿掉、搬家、`position` 變了都不用各留一套帳。
  - **包含塊排兩趟**：第一趟只排自己 in-flow 的內容（佔位盒子跟著就位），接著把兩邊都 `auto` 的那一軸換成佔位盒子的位置（padding box 相對，所以要減掉包含塊自己的邊框——它自己的 `layout` 在這個時候還沒寫好，要從 `taffy_style.border` 拿），第二趟連 out-of-flow 的一起排。
  - flex 和 grid container **自己的** out-of-flow 子盒子不注入 static position：css-flexbox 1 § 4.1 要的是容器的對齊，taffy 本來就會做。
  - 佔位盒子自己不是 out-of-flow，不要再被掛一次。
- **fixed**（第 16、17、19 條）：包含塊是那個 frame 的 `LayoutView`（大小就是 viewport），所以百分比和 `bottom` / `right` 都對 viewport 算。它不隨文件捲動，所以不算進 scrollable overflow。paint 這一側：子 frame 的 viewport 本來就是 property tree 裡的 scroll node（RFC 0009 的 2b-2），fixed 的子樹接在它的 scroll translation 之上；主 frame 的 viewport 沒有節點（P4 讓 compositor 用 `visible_rect` 直接捲），所以改用一個帶 `fixed` 標記的 transform 節點，`layerize` 讓標過的 chunk 自成一層，compositor 的 `layer_origin_in_pixels` 把目前的捲動位移加回那一層的原點。tile 是每層自己內容空間的，所以捲動不失效、不重 raster、不用提交。
- **paint 和 hit test**（第 2 條）：附錄 E 第 8 步：`z-index: auto` 或 `0` 的有 `position` 的元素，照樹的順序畫在 line 階段之後，`z-index: auto` 的當成 stacking context 畫，但它裡面有 `position` 的元素和真正的 stacking context 歸到外面的 stacking context。
  - `ComputedStyle::is_stacked`（Blink 的 `PaintLayer::IsStacked`）：stacking context 或有 `position` 的元素。背景和 line 階段跳過的是 stacked 的 box（原本只跳過 stacking context），item 和 atomic inline 整個畫完時也一樣。
  - `stacking_children` 收集 stacked 的 box：碰到 stacking context 停下，碰到有 `position` 但不是 stacking context 的，收集它之後繼續往裡面收，它裡面的歸給外面；以 `z-index` 穩定排序，`auto` 當成 0，所以和 `z-index: 0` 的 stacking context 照樹的順序排。只有真的 stacking context（和 `LayoutView`）有自己的清單：「假的」那種畫起來就是自己的背景、背景階段、line 階段。
  - hit test 照同樣的順序反過來走，`covered_scrollers` 走同一個 walker；走進 fixed 的子樹時把 viewport 的捲動加回去（第 18 條）。
- **DOM**（第 9、12、13 條）：`offsetParent` 改成 CSSOM View 的演算法——往上走找最近的有定位的祖先（`position` 用 RFC 0016 第 2 段那條路讀），`offset*` 從它的 padding box 算；`position: fixed` 的元素沒有 `offsetParent`（規範的第 1 步），它的 `offset*` 就是文件原點相對的，也就是 viewport 相對的。`BlockBox` 多一個 `inset`（used 的位移），所以 `getComputedStyle` 的四個位移是 resolved value：給了值的保留 computed value（百分比絕對化），`auto` 的才取 layout 算出來的——over-constrained 時 CSSOM 要的就是宣告的值。fixed 的盒子的 client rect 不扣 viewport 的捲動（`layout::is_fixed`）。
- **捲動範圍**：`docs/limitations.md` 原本記著「成為 layer 的捲動容器的範圍的不變式在加上 `position: fixed` 時要重新檢討」，結論是第 19 條（fixed 的不算進範圍、`covered_scrollers` 跳過它）。順手修掉一個從第 1 段起就在的 bug：完全沒有面積的盒子照舊撐大捲動範圍（`css/css-overflow/overflow-empty-child-box.html`），改成沒有面積的盒子自己不算、裡面的東西照算，只有一軸是 0 的照舊算另一軸。

### 測試

驗收的項目和結果見 `progress.md`：WPT（`css/CSS2/positioning`、`css/css-position`、`css/CSS2/zindex` 的 reftest 和 `getComputedStyle-insets-*`）、自己的兩個 reftest、`layout` 的 `position` 和 `update`、`paint` 的 `artifact` / `hit_test` / `layerize`、`compositor` 的 fixed layer、`shell` 的 `geometry`，和 `about:demo` 的三個情境（`position-relative`、`position-absolute`、`position-fixed`）。

### 分階段

分三段（第 4 條），每段開始前再決定那一段的細節，三段都做完：
1. **relative 和 `z-index`**（第 5 到 8、10 條）：`position: relative`、位移、有 `position` 的元素的 `z-index` 和附錄 E 第 8 步的 paint 順序、hit test；照「疊在一起的頭像和通知徽章」驗收。
2. **absolute**（第 11 到 15 條）：包含塊、static position、out-of-flow 的排版和 fragment；連帶補 `offsetParent` 和 inset 的 resolved value（第 9 條）。
3. **fixed**（第 16 到 19 條）：viewport 的包含塊、不跟著捲動的 layer、compositor 的捲動和捲動範圍。

## 缺點

- 絕對定位不走 taffy 的預設做法（第 11 條把 child 清單改掛到包含塊），所以「交給 taffy 的樹」和 box tree 不一樣，包含塊還要排兩趟；之後讀 `layout` 的人要先知道這件事。
- 局部重排保守（第 15 條）：絕對定位的盒子改了位移，它的包含塊整個重排。
- paint 的順序多一種「從 flow 拿出來的東西」，hit test 和 `covered_scrollers` 要跟著維持同樣的順序。
- fixed 牽動 compositor：多一個 layer 的標記和一條「畫的時候把捲動加回去」的規則，而不是一棵純粹的 property tree（第 16 條的取捨）。

**踩過的坑**（詳情見 `progress.md`）：`position` 變了的時候，父盒子那份 in-flow 清單如果是增量維護的會過期（所以每次 layout 重算，並算進 `Reattach`）；包含塊自己的邊框在它排版途中還不在 `layout` 裡，static position 會多算一次；佔位盒子自己也會被當成 out-of-flow 掛一次；`getBoundingClientRect` 走的是 `layout::client_rects` 而不是 `paint::element_bounds`（RFC 0018）。

## 理由和其他做法

2026-10-07 確認了整份的四項（第 1 到 4 條），各段的細節在那一段開始前確認（第 5 到 19 條）。完整的比較見 `details.md`。

- **A. 絕對定位的 box 誰來排**：
  - **A1. 照 taffy 的做法**：`Position::Absolute` 的 box 由它的父 box 排，對父 box 定位。簡單，但包含塊不是父 box 時（常見：`relative` 的容器裡隔好幾層的 `absolute`）位置錯。
  - **A2. 照 Chromium，由包含塊排**（Blink 的 `OutOfFlowLayoutPart`）：父 box 只替它算 static position（taffy 已經會算），不排它；包含塊排完自己的內容之後，排它的 out-of-flow 後代，fragment 是包含塊的 child。
  - **A3. fork taffy、讓它對包含塊定位**：RFC 0003 第 14 條決定不 fork。
  - 決定：A2（第 1 條）。怎麼做到見 B。
- **B. A2 怎麼落實**（第 2 段）：
  - **B1. 掛到包含塊的 taffy child 清單**，原本的位置留 0×0 的佔位 box 當 static position：taffy 的算法一律對父 box 解，所以把 OOF box 在「交給 taffy 的 child 清單」裡移到包含塊底下，位移推大小、shrink-to-fit、`auto` margin、over-constrained 都照用 taffy 的。
  - **B2. 包含塊自己排**（最貼近 `OutOfFlowLayoutPart`）：要重寫上面那一整套，bug 也自己養。
  - **B3. taffy 先在父 box 排好、包含塊不同時再修**：大小和百分比先對錯的盒子解一次，等於 A1 加補丁。
  - 決定：B1（第 11 條）。
- **Z. paint 順序怎麼做**：
  - **Z1. 延伸現在的 `stacking_children`**：有 `position` 的元素在 fragment tree 上和 stacking context 一樣被收集出來，`z-index: auto` 的當成「假的」stacking context（它裡面的有 `position` 的元素和 stacking context 歸到外面）。改動小，和 RFC 0003 的做法一致。
  - **Z2. 照 Blink 加上 `PaintLayer` 的樹**，順便做 P4 第 73 條的 subsequence 快取：之後 stacking context 多了比較快，但這份 RFC 的範圍和風險變大。
  - 決定：Z1（第 2 條）；`PaintLayer` 和 subsequence 快取等真的慢了再做（「之後的可能」）。
- **F. fixed 和 compositor**：
  - **F1. 照 Chromium，fixed 的內容另成 layer**：viewport 捲動時 compositor 不動它，捲動照樣不等 main thread。
  - **F2. 有 fixed 的內容時，viewport 的捲動交給 main thread**：簡單，但 JS 卡住時捲不動，和現在 viewport 捲動的保證相反。
  - 決定：F1（第 3 條）。怎麼做到見 G。
- **G. F1 怎麼落實**（第 3 段）：kun 的主 frame viewport 捲動不在 property tree 裡（P4 讓 compositor 用 `visible_rect` 捲），所以不能照 Blink 把 fixed 的 transform 接在 viewport 的 scroll 之上。
  - **G1. layer 帶「釘在 viewport」的標記**，compositor 畫的時候把捲動加回去；子 frame 走它本來就有的 scroll node。改動集中在 `layerize` 和 compositor 畫的那一步。
  - **G2. 把主 frame 的 viewport 也放進 property tree**（最像 cc）：`visible_rect`、raster 失效、protocol 的 `root_scroll_offset`、`scroll_hit_test` 和 renderer 的接線都要改，整份文件的 layer 還會換一次座標空間。
  - 決定：G1（第 16 條）。
- **S. `sticky`**：
  - **S1. 不在這份裡**，另外寫：要 compositor 照捲動位置算位移（Chromium 的 `StickyPositionConstraint`）。
  - **S2. 第 4 階段**。
  - 決定：S1（第 4 條）。

## 先例

- **Chromium（LayoutNG）**：out-of-flow 的 box 在它的父 box 排版時記下 static position，往上傳到包含塊，包含塊用 `OutOfFlowLayoutPart` 排它，fragment 是包含塊的 child；paint 用 `PaintLayer` 的樹照附錄 E 排；fixed 的元素在 `PaintPropertyTreeBuilder` 接到 viewport 不捲動的 transform，合成時另成 layer。
- **taffy 0.14**：`Position::Absolute` 對父 box 定位（父 box 就是包含塊），會算 static position；`Position::Relative` 在排版後套位移。
- **Servo**：自己的 layout，out-of-flow 的 box 也是往上傳到包含塊（`HoistedAbsolutelyPositionedBox`）。

## 已決定的事項

1. **絕對定位的 box 由包含塊排（A2，照 Chromium 的 `OutOfFlowLayoutPart`）**（2026-10-07；2026-10-09 重新確認）：父 box 只替它算 static position，包含塊排完自己的內容之後排它，fragment 是包含塊的 child。不選照 taffy 對父 box 定位（A1）：包含塊不是父 box 時位置錯，而那是最常見的寫法；不選 fork taffy（A3，RFC 0003 第 14 條）。
2. **paint 順序延伸現在的 `stacking_children`（Z1）**（2026-10-07；2026-10-09 重新確認）：有 `position` 的元素和 stacking context 一樣在 fragment tree 上被收集出來，`z-index: auto` 的當成假的 stacking context。不選這一次就加上 `PaintLayer` 的樹和 subsequence 快取（Z2）：範圍和風險變大，等真的慢了再做。
3. **fixed 的內容另成 layer（F1，照 Chromium）**（2026-10-07；2026-10-09 重新確認）：viewport 捲動時 compositor 不動它，捲動照樣不等 main thread。不選有 fixed 的內容時把 viewport 的捲動交給 main thread（F2）：JS 卡住時整個頁面捲不動，和現在 viewport 捲動的保證相反。
4. **分三段：relative 和 `z-index`、absolute、fixed；`sticky` 另外寫（S1）**（2026-10-07；2026-10-09 重新確認）：`sticky` 要 compositor 照捲動位置算位移（Chromium 的 `StickyPositionConstraint`），範圍和 fixed 一樣大，另外寫一份。

5. **第 1 段的 inline 元素只有位移**（2026-10-07）：`<span style="position: relative">` 這類 inline 元素照 `top` / `left` 位移（它在每一行的 inline box 和內容），但照原本 line 的順序畫（附錄 E 第 7 步），`z-index` 不生效；記在 `docs/limitations.md`。atomic inline（`inline-block` 等）有自己的 box，照完整的規則。不選完整照附錄 E 第 8 步：kun 的 inline 元素沒有自己的 box，要先讓 line 上的 inline box 和文字能分開畫（Blink 靠有自己 `PaintLayer` 的 `LayoutInline`），範圍大很多，排進「之後的可能」；不選第 1 段不做：微調圖示、上標這類常見的用法只要位移。同一天討論過要不要因此提早做 `PaintLayer` 和 subsequence 快取（第 2 條）：`PaintLayer` 本身不會讓 line 能拆開，subsequence 快取是效能的事，所以維持第 2 條。
6. **第 1 段的 `absolute`、`fixed`、`sticky` 宣告丟掉**（2026-10-07）：只 parse `static` 和 `relative`，其他的值整條宣告丟掉，照 `static` 排，和 RFC 0003 對不支援的值的做法一樣；各自的階段再加。不選先當成 `relative`：行為和 Chrome 不同，又比丟掉更難察覺。
7. **第 1 段的 WPT 候選全部匯入**（2026-10-07）：候選腳本加上 `position` 和位移的屬性、排除 `absolute`、`fixed`、`sticky` 之後，在用的目錄加上 `css/CSS2/positioning`、`css/CSS2/zindex`、`css/css-position` 有 208 個（`css/CSS2/positioning` 137 個，其他 71 個）。照 RFC 0003 的做法全部匯入，過不了的記進 expectations 附上原因。不選 `css/CSS2/positioning` 每個系列挑一兩個：它們跑得快，守住位移的解析。
8. **第 1 段用疊在一起的頭像和通知徽章驗收**（2026-10-07）：一排用負 margin 疊在一起的頭像，`position: relative` 和 `z-index` 決定誰在上面；按鈕角落一個用 `top` / `right` 位移的通知徽章。做成 reftest、`about:demo` 的一段和情境（改 `z-index` 換誰在上面）。不選只靠 WPT 和單元測試：情境才測得到改了 `z-index` 之後畫面和 hit test 跟著換。

9. **第 2 段連帶補 `offsetParent` 和 inset 的 resolved value**（2026-10-09）：兩件事在別的 RFC 裡等這一份——`offsetParent` 現在只會是 `body`、`table`、`td`、`th`（RFC 0018 的 2a 留的「有 `position` 之後再擴充」），`getComputedStyle` 的 `top` / `right` / `bottom` / `left` 現在回 computed value（RFC 0016 的第 11 條：resolved value 等這一份）。包含塊在第 2 段才有，兩件都要它，所以一起補，兩份 RFC 在 `docs/limitations.md` 的條目跟著刪。不選「等這份 RFC 做完再補」：中間的狀態是「`position` 有了但 `offsetParent` 還是錯的」，比較容易誤導頁面。
10. **第 1 段用接回實作的方式做**（2026-10-09）：2026-10-07 反轉的那個 commit（`0dd77b0`）重新套用——程式碼部分套得乾淨，只有 RFC 0016 的 1a、2a 刻意加的三個 exhaustive match 編不過（`position`、`top` / `right` / `bottom` / `left` 的 specified 和 computed 序列化要補），文件和 WPT 清單的衝突重解。不選重寫：那份實作當時整套驗收過（208 個 WPT reftest、reftest、情境、`about:demo`），重寫只會重新得到同一份設計，而且會失去當時的測試切入點。

11. **out-of-flow 的 box 掛到包含塊的 taffy child 清單，原本的父 box 留一個 0×0 的佔位 box（B1）**（2026-10-09，第 2 段；第 1 條的落實）：taffy 0.14 的絕對定位算法是私有的、寫在 block / flex / grid 各自的 compute 裡，而且一律對「父 box」解（`perform_absolute_layout_on_absolute_children`），沒有「對任意包含塊排一個絕對定位 box」的公開入口。所以 box tree 照舊是 DOM 結構，但交給 taffy 的 child 清單把 OOF box 移到它的包含塊底下（和 flex 的 `order` 用另一份 child 清單同一個前例），taffy 的算法就是對包含塊解的，fragment 也剛好落在包含塊的 fragment 底下（第 1 條要的）；`auto` 的位移要「原本會在的位置」，所以在原本的父 box 留一個 0×0、沒有 margin 的佔位 box，它在 flow 裡的位置就是 static position（Chromium 也是把 static position 從父 box 往上傳給包含塊），包含塊排自己的絕對定位 child 之前，`auto` 的那一軸換成那個長度。不選「包含塊自己排」（B2，最貼近 `OutOfFlowLayoutPart`）：要自己重寫 taffy 已經有的位移推大小、shrink-to-fit、`auto` margin 置中、over-constrained 和 static position，bug 也自己養；不選「taffy 先在父 box 排好、包含塊不同時再修」（B3）：大小和百分比先對錯的盒子解一次，巢狀的要排兩趟，等於第 1 條否決的 A1 加補丁。
12. **`offsetParent` 從 style 走祖先**（2026-10-09，第 2 段）：`dom_api` 照 CSSOM View 的演算法往上走，每個祖先的 `position` 用 RFC 0016 第 2 段那條路讀（`update_style` 之後問 `ComputedStyle`），`LayoutClient` 不用多成員。不選「`element_box` 多一個 `offset_parent`」：renderer 要走一次同樣的祖先鏈，而那條鏈 `dom_api` 本來就有，多傳一個欄位只是把 DOM 的規則搬進 renderer。
13. **inset 的 resolved value 由盒子帶上來**（2026-10-09，第 2 段；第 9 條的落實）：`BlockBox` 多一個 `inset`（used 的位移，上右下左），`layout` 從 fragment 填，`dom_api` 直接讀——和 RFC 0016 的 2b 替 `padding`、`margin` 做的一樣。不選「`dom_api` 自己從包含塊的盒子減」：包含塊是誰只有 layout 知道（第 11 條的 taffy child 清單），DOM 這一側要再走一次同樣的規則。
14. **第 2 段的 WPT 候選重跑、全部匯入**（2026-10-09，第 2 段）：候選腳本放行 `position: absolute`（`fixed` 和 `sticky` 繼續擋），在第 1 段的目錄上重跑，照 RFC 0003 的做法把新的候選全部匯入，過不了的記進 expectations 附上原因。
15. **局部重排先保守**（2026-10-09，第 2 段）：絕對定位的 box 的 style 變了（位移、大小、`position` 本身），把它的包含塊標成要重排（包含塊排它，static position 又在原本的父 box 的 flow 裡），不另外做「只重排那一個 box」的捷徑；用 `layout` 的 `update` 測試守住「局部重排和整份重排結果一樣」。不選一開始就做精準的失效：包含塊、佔位 box 和 OOF box 三邊的依賴要先有測試守住，量到慢再縮小範圍（「之後的可能」）。

16. **fixed 的盒子接到「最近的祖先 frame 的 viewport」：主 frame 靠 layer 的標記，子 frame 走 property tree（G1）**（2026-10-09，第 3 段；第 3 條的落實）：kun 的 viewport 捲動不在 paint 的 property tree 裡——頁面照文件座標畫，compositor 用 `visible_rect` 決定看哪一塊（P4 的設計）。所以主 frame 裡的 fixed 內容自成一個 layer，帶著「釘在 viewport」的標記，compositor 畫的時候把目前的捲動位移加回它的原點；tile 是每個 layer 自己內容空間的，所以不會失效、不用重 raster，compositor 照舊不等 main thread（第 3 條要的保證）。子 frame 的 viewport 本來就是 property tree 裡的 scroll node（RFC 0009 的 2b-2），所以子 frame 裡的 fixed 盒子接在那個 scroll 的 translation 之上就對了。不選「把主 frame 的 viewport 也放進 property tree」（G2，最像 cc）：`visible_rect`、raster 失效、protocol 的 `root_scroll_offset`、`scroll_hit_test` 和 renderer 的接線都要改，整份文件的 layer 還會換一次座標空間，換來的只是寫法更像；不選「只做子 frame 的 fixed」：一般網頁的固定導覽列就在主 frame。
17. **fixed 的包含塊是它那個 frame 的 `LayoutView`（H1）**（2026-10-09，第 3 段）：沿用第 2 段的掛載機制（第 11 條），只是不找有定位的祖先，一律掛到 `LayoutView`——它的大小就是 viewport，所以百分比和 `bottom` / `right` 都對 viewport 算。不選「另做一個 viewport 包含塊的盒子」：第 2 段的機制已經夠用。
18. **hit test 和 `elementFromPoint` 走同一條規則（I1）**（2026-10-09，第 3 段）：hit test 走進 fixed 的子樹時把 viewport 的捲動加回去，和 paint 擺它的方式同一條；不另外存一份 viewport 座標的清單，免得兩邊的順序分岔。
19. **fixed 的內容不算進捲動範圍（J1）**（2026-10-09，第 3 段）：它不隨文件捲動，所以不算進文件的 scrollable overflow，`covered_scrollers` 也跳過它（`docs/limitations.md` 原本記著「範圍的不變式在加上 `position: fixed` 時要重新檢討」，這一條就是結論）。

## 待決定

- 無（第 3 段做完這份就完成）。

## 之後的可能

- `position: sticky`（第 4 條）。
- 絕對定位的盒子套用 `justify-self` / `align-self`（css-align 的 self-alignment：對齊容器是包含塊的 padding box）：Chrome 有，kun 把盒子放在 static position（`docs/limitations.md` 的 Layout）。
- inline formatting context 裡的絕對定位盒子的 static position 照它在行裡的位置（現在是父盒子的邊框盒原點，第 11 條的 0×0 佔位盒子不參與行的排版）。
- grid container 自己的絕對定位子盒子照 grid 的區域和對齊擺（現在是 padding box 的原點）。
- `PaintLayer` 和 subsequence 快取（第 2 條）：stacking context 和有 `position` 的元素變多、每次 paint 走整棵 fragment tree 收集它們的開銷明顯時，或 RFC 0012 量出 paint 是瓶頸時。
- 放在有 `position: relative` 的 inline 元素裡的 block 跟著它位移（WPT 的 `position-relative-001`、`-002`、`inline-box-002`）：box tree 把 inline 元素拆開，block 是外面的 block container 的 child，要讓它記得包著它的 inline 元素。
- 有 `position` 的 inline 元素照附錄 E 第 8 步晚點畫、`z-index` 生效（第 5 條）：讓 line 上的 inline box 和文字能分開畫，畫一般的行時跳過它在各行的那幾段，之後再畫（Blink 有自己 `PaintLayer` 的 `LayoutInline`）；可以和 `PaintLayer` 一起做。
- 有 `transform` 的元素成為 `position: fixed` 的包含塊（CSS Transforms）：現在一律是 viewport（`docs/limitations.md` 的 Layout）。做的時候 paint 也要跟著分：包含塊不是 viewport 的 fixed 盒子會跟著頁面捲動，不能帶「釘在 viewport」的標記（第 16 條）。
- CSS Anchor Positioning。
