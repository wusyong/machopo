# RFC 0047：table

- 開始日期：2026-10-14
- 狀態：✅ 已完成：2026-10-15
- 細節：`details.md`

## 摘要

RFC 0012 路線圖的 S7（CSS 線）：table layout。HTML 的 `<table>` 和 CSS 的 table 系列 `display` 照 css-tables 3 和 CSS 2 § 17 排版：缺的 table 物件（row、row group、table）照 fixup 補上匿名的；欄寬照 auto 或 fixed 的 table layout 算（`colspan`、column 和 column group），列高照內容（`rowspan`），`border-spacing` 和 `border-collapse`、cell 的 `vertical-align`、caption、`visibility: collapse`，畫的層次照 Blink。演算法照 Blink LayoutNG 的 `table_layout_utils`、`TableBorders`、`TablePainter`（第 2 段起對照它的原始碼寫）。排版是 kun 自己的 table formatting context，在 taffy 裡是一個 leaf（和 inline formatting context 一樣），cell 是一般的 block 盒子交給 taffy 排。RFC 0045 第 9 條「table 系列的 `display` 暫時當 block」拿掉；UA stylesheet 給 `<table>` 等元素 table 的 `display` 和 HTML 的 presentational hint。

## 動機

- **Speedometer**：default suite 的 Perf-Dashboard（perf.webkit.org）在測試打開的 charts 頁面上，每次 render 都重建 `chart-pane-status-view` 的 `<table>`（盤點記到 `createElement('tr')`、`td`），它的 CSS 還有 `display: table-cell`、`inline-table`。RFC 0045 的第 9 條寫「`table-cell`、`inline-table` 只在不屬於 default suite 的 perf.webkit.org」是錯的：Perf-Dashboard 就是 perf.webkit.org，在 default suite 裡（2026-10-14 的盤點）。它的元件是 web component，table 在 shadow tree 裡，所以要 RFC 0046（K-2）之後才畫得出來。6 個 Complex-DOM 共用的靜態 DOM 有一個 Spectrum 日曆的 `<table>`（`table-layout`、`border-collapse` 的宣告也是它的）。盤點看不出 table：它只記頁面的 CSS 裡 kun 丟掉的宣告，而 `<table>` 的 `display` 來自 UA stylesheet。
- **一般網頁**：HTML 的 table 在 kun 上是行內內容（UA stylesheet 沒有給它 table 的 `display`），排版和 Chrome 完全不同；用 table 排版的舊網頁、資料表格都不對。CSS 的 `display: table-cell` 排成上下疊的 block（RFC 0045 第 9 條的暫時對應）。
- **WPT**：`css/CSS2/tables/`（1235 個檔案）、`css/css-tables/`（283 個）幾乎都沒加進清單；很多其他目錄的 reference 用 table。

要解決的 `docs/limitations.md` 的限制：

- 「只有 block、inline、flex 和 grid layout……沒有……table。table 的 `display` 值都收……其他照 `block` 排……HTML 的 `<table>` 照舊是行內內容」的 table 部分。
- 「`visibility: collapse` 和 `hidden` 一樣（kun 沒有 table）」（第 2 段）。
- UA stylesheet 開頭的「The table display types are laid out as blocks, so this sheet gives tables none: they are inline.」。

**目標**：table 的盒子照 css-tables 3 建（含匿名物件）；auto 和 fixed 的 table layout；`colspan`、`rowspan`；`border-spacing`、separated 的邊框；`border-collapse: collapse`；cell 的 `vertical-align`（`baseline`、`top`、`middle`、`bottom`）和 baseline；caption（`caption-side`）；column、column group 的寬度和背景；table 的 paint 的層次（css-tables 3 的 table、column group、column、row group、row、cell）；HTML 的 table 的 presentational hint；幾何的 API、hit test、`offsetParent`；局部更新。

**不在範圍內**：table 的 DOM API（`HTMLTableElement.rows`、`insertRow()`、`HTMLTableCellElement.cellIndex` 等，和盤點的「沒有自己的 interface」的 `td`、`tr`：另外一份 RFC）；`<table>` 在 multicol、分頁裡的切割；`table-layout` 以外的 css-tables 3 的實驗性的部分（`tentative/`）；直排（kun 沒有 writing mode）。

## 設計概要

網頁的作者看到的：

- `<table border=1 cellspacing=2><tr><td>a<td colspan=2>b</table>` 和 Chrome 一樣排：欄寬照內容分配，`colspan` 的 cell 跨兩欄，邊框、間距照 attribute。
- `.row { display: table-row } .cell { display: table-cell; vertical-align: middle }` 的版面並排、垂直置中（現在是上下疊）。
- `table-layout: fixed; width: 300px` 照第一列的寬度分欄，不看其他列的內容。
- `border-collapse: collapse` 的相鄰邊框合成一條，照 css-tables 3 的規則決定哪一條勝出（第 2 段）。
- `getBoundingClientRect`、`offsetWidth` 對 table、row、cell 回答它們的盒子；點 cell 打到 cell。

改 kun 的人看到的：`layout` 多一個 table formatting context（`table` 模組）：table 盒子的 `LayoutBox` 帶 table 的結構（section、row、cell 和它們的格子位置），在 taffy 裡是 leaf，它的 compute 函式自己算欄寬和列高，cell 經過 taffy 照一般的 block 排；`collect` 照 fixup 補匿名的 table 物件。

## 詳細設計

### 盒子和 fixup

- **display**：`Display::Table(TableDisplay)`、`InlineTable` 照樣（RFC 0045 已經收），layout 不再當 block：`table`、`inline-table` 建 table 盒子（table formatting context 的根，`inline-table` 是 atomic inline），`table-row-group` 等是它裡面的結構。
- **fixup**（css-tables 3 的「Fixup」，CSS 2 § 17.2.1，Blink 的 `LayoutTable*::AddChild` 建匿名的 wrapper；第 2 條）：照 flat tree 的子節點（RFC 0046 的 `flat_tree_children`），`display: contents` 的元素的子節點當成它的父元素的（css-tables 3 的 fixup 也這樣看）。table 的直接子節點不是 row group、row、caption、column 的，包進匿名的 row group 和 row；row group 裡不是 row 的包進匿名的 row；row 裡不是 cell 的包進匿名的 cell；不在 table 裡的 cell、row 由外往內補匿名的 row、row group、table（`display: table-cell` 的兩個 `<div>` 並排，因為它們被包進同一個匿名的 table 和 row）；table 結構裡只有空白的文字丟掉。
- **wrapper box 和 caption**（第 3 條）：照 Blink 不建 table wrapper box：caption 是 table 盒子的 child，排在格子的上面或下面（`caption-side`）；margin、定位在 table 盒子上。
- **column、column group**（第 14 條）：table 裡的 `table-column`、`table-column-group` 是 box tree 裡沒有 fragment 的盒子（Blink 的 `LayoutTableColumn`），column group 的內容只有它的 column，table 外面的照舊不產生盒子。它們的 `width`、`min-width` 和 `span`（`<col span>`、`<colgroup span>`）照 Blink 的 `CreateColumn` 參與欄寬（column group 的固定寬度給它沒有寬度的 column），在哪裡記在 table 的 fragment 上（Blink 的 `TableFragmentData::ColumnGeometry`），背景畫在從它們的欄開始的 cell 裡。column 改了 style（包括只改畫的），table 重排；改了 `span`，column 重建。

### 排版（第 1 條）

- **table formatting context**：由 kun 自己做（`layout` 的 `table` 模組，Blink 的 `TableLayoutAlgorithm`、`table_layout_utils`），table 盒子在 taffy 裡是 leaf（和 `compute_inline` 一樣），cell 是它的 children，用 taffy 的 `compute_child_layout` 量 min-content / max-content 寬度、在算好的寬度裡排；row、row group 的盒子也是 table 盒子底下的 `LayoutBox`（有 fragment，畫背景、回答幾何），由 table 的演算法放。
- **欄寬**：auto layout 照 css-tables 3 的「Computing the column measures」和「Distributing width to columns」（cell 的 min / max 寬度、`colspan` 的分攤、百分比、table 的 `width`），fixed layout（`table-layout: fixed` 且 `width` 不是 `auto`）只看第一列和 column。table 的 min-content / max-content 寬度給外面的 shrink-to-fit。
- **列高**：每列的高度是它的 cell 的內容高度和 `height` 的最大，`rowspan` 的 cell 的高度分攤到跨的列；table 的 `height` 比內容高時分給 row group（css-tables 3 的「Distributing height」，照 Blink 的 `DistributeTableBlockSizeToSections`：先給沒有固定高度的、再給有的、最後給百分比的，後兩種先給 body），row group 再分給它的列（沒有固定高度的列照高度的比例，全是 0 時平分）。`vertical-align` 決定內容放在 cell 裡的位置（`baseline` 對齊同一列的第一行）；cell 只在內容會不同時照它的列高再排一次（positioned，或有百分比高度的 child 而且 cell 或 table 有 `height`；Blink 每個 cell 都照最後的大小排），row 的 `height` 不讓 cell 的高度 definite（和 Chrome 一樣）。
- **`visibility: collapse`**（第 16 條）：照 Blink，collapse 的欄和列先照樣量（列高用 collapse 之前的欄寬算），再收起來：欄寬 0、沒有間距，table 跟著變窄；列高 0、沒有間距，有 `height` 的 table 也扣掉它；跨過它們的 cell 照縮小的寬度再量一次、照新的內容對齊（列高不變），只在 collapse 的欄裡的 cell 和 collapse 的列被藏起來（fragment 的 `hidden`，Blink 的 hidden for paint：pre-paint 給它空的 clip，不畫也打不到）。
- **邊框**：separated 的 table 有 `border-spacing`（cell 之間、cell 和 table 的邊之間）。`border-collapse: collapse`（第 15、17 條）照 Blink 的 `TableBorders`：每一段邊照 cell、row、row group、column、column group、table 的順序合併（css-tables 3 的「Border conflict resolution」：`hidden` 勝過一切，再來寬的、樣式高的，平手時先合併的；`inset` 當 `ridge`、`outset` 當 `groove`），跨欄跨列的 cell 裡面的邊不給別人；cell 的邊框是它四邊的一半（寫進它的 taffy style，Blink 從 constraint space 傳），table 的是外圍的一半，table 沒有 padding 和 `border-spacing`。結果和格子線的位置放在 table 的 fragment。
- **baseline**：table 的 baseline 是第一列的 baseline（`inline-table`、flex 的對齊用）。
- **快取和局部更新**：table 的結果照 taffy 的快取（輸入一樣就沿用）；table 的內容（row、cell 的增減，Perf-Dashboard 每次重建 `<tr>`）照現有的 `collect_again` 重新收集 table 盒子的結構，cell 的盒子照常保留；`colspan`、`rowspan`、`span` 改了重建那個盒子；column 的 style、collapse 的 table 的部分只改畫的 style 時，table 也重排（它的 fragment 帶著它們的 style）。

### paint 和 hit test

- 照 css-tables 3 的層次（Blink 的 `TablePainter`）：table 的背景和邊框只包住格子（不含 caption）；column group、column、row group、row 的背景畫在 cell 裡（column 的畫在從它的欄開始的 cell 裡），不在間距裡；再來是每個 cell 的背景和邊框、內容。`empty-cells: hide` 的空 cell 不畫（collapse 時不作用）。
- collapsed 的邊框（第 15 條）由 table 在所有 cell 的背景之後、內容之前畫（Blink 的 `PaintCollapsedBorders`）：每一段邊在格子線上、和它的邊框一樣寬，交接處照寬度、樣式、box order 決定誰占（`ComputeEdgeJoints`），每一段照它的樣式畫（`DrawBoxSide`）；cell 和 table 不畫自己的。cell、它的 row 或 row group 另外畫的時候（positioned、stacking context），畫進那個 cell 的背景裁到它的邊框以內，不蓋住 table 先畫好的邊框（Blink 的 `TableCellBackgroundClipper`）。
- layout 藏起來的（收起來的列、只在收起來的欄裡的 cell）在 pre-paint 拿到空的 clip。
- `PaintController` 不沿用新的 table 部分的 fragment 上次的 item（它們畫的東西看 child 的位置）。
- hit test 打到 cell、row 等照它們的盒子；`offsetParent` 照 HTML（`td`、`th`、`table` 已經是）。

### style

- **UA stylesheet**（HTML 的 rendering 一節、Chrome 的 `html.css`）：`table { display: table; box-sizing: border-box; border-spacing: 2px; border-collapse: separate; text-indent: initial }`、`caption`、`colgroup`、`col`、`thead`、`tbody`、`tfoot`、`tr`、`td`、`th`（`font-weight: bold; text-align: -internal-center` 的近似）、`td, th { padding: 1px; vertical-align: inherit }` 等。`border-color` 照 Chrome 現在的做法不設（`currentColor`）：HTML 的 `table { border-color: gray }` 和 row group、row 的 `inherit` 都沒有，`<table border>` 給 cell 的 `1px inset` 也是 cell 自己的顏色（WPT 的 `table-bordercolor-default.html`、`table-border-color-not-inherited.html`，Chromium 的 crbug.com/494554835、crbug.com/40745409）。
- **新的 property**：`border-spacing`、`border-collapse`、`table-layout`、`caption-side`、`empty-cells`（第 5 條）；`vertical-align` 已經有。
- **HTML 的 presentational hint**（第 4 條）：`<table>` 的 `width`、`height`、`border`、`cellspacing`、`cellpadding`（給它的 cell）、`align`；`<td>`、`<th>` 的 `width`、`height`、`nowrap`、`valign`、`align`；`<tr>`、section 的 `valign`、`align`（照 Blink 的 `HTMLTablePartElement` 當成 property 的 CSS 解析，`align` 的 `middle`、`absmiddle` 是置中）、`<tr>` 的 `height`；`<col>`、`<colgroup>` 的 `width`；cell 照 HTML 的 table model 找它的 table（row 的 parent，或 row group 的）；`rowspan`、`colspan`、`<col span>` 是結構的 attribute（layout 直接讀）。

### 測試

- **`css`、`style` 的整合測試**：新的 property 的解析、序列化、computed value；UA stylesheet 和 hint（`getComputedStyle`）。
- **`layout` 的整合測試**（`tables`）：fixup（每種缺的物件、空白）、欄寬（auto：min / max、`colspan`、百分比、table 的 `width`；fixed；column）、列高（`rowspan`、table 的 `height`）、`border-spacing`、collapsed 的邊框、`visibility: collapse`、cell 的 `vertical-align`、caption、`inline-table` 的 baseline、shrink-to-fit 裡的 table、局部更新（插入、移除 row 和 cell、column、邊框，每一步和重建的樹比對）。
- **`paint`**：背景的層次、column 的背景、collapsed 的邊框（交接、樣式、`DoNotFill`、背景裁在邊框內）、hit test、`PaintController`。
- **kun 的 reftest**：`tables/` 的四份（separated、格子、column 和 collapse、collapsed 的邊框）。
- **WPT**：`css/css-tables/`、`css/CSS2/tables/` 整個（失敗的 `.ini` 寫原因）；清單裡其他用 table 的測試另外跑，RFC 0045 時記成「table 的版面」的失敗重新檢查。對照用 testharness.js 測試的 `data-expected-*` 和 Blink 的原始碼（做法見 `progress.md`）。
- **情境**：`about:demo` 加一個小表格和「Add a row」、「Collapse a column」、「Collapse borders」按鈕（`tables`），畫面由 reftest 守住。
- **mutation test**：每一小段一份。
- **量測**：不排（Speedometer 只在使用者要求時量）。

### 分階段

1. **table 的盒子和 separated 的 table**（分三小段，第 8 條：1a style、1b layout、1c paint 和驗收）：fixup、UA stylesheet 和 hint、新的 property、table formatting context（auto 和 fixed、`colspan`、`rowspan`、`border-spacing`、`vertical-align`、caption、baseline）、paint 的層次（column 以外）、幾何、局部更新；拿掉 RFC 0045 第 9 條的暫時對應。
2. **`border-collapse` 和 column**（分三小段，第 13、17 條：2a column 和 `visibility: collapse`、2b-1 collapsed 的邊框的解決和排版、2b-2 繪製）：collapsed 的邊框的解決、排版、繪製；column、column group 的寬度和背景；`visibility: collapse`。

## 缺點

- 一個新的 formatting context 要 kun 自己維護（taffy 沒有 table），演算法的細節多（欄寬的分配、百分比、`colspan` 的分攤），各瀏覽器之間也有差異；照 Blink 的做法和 WPT 對照。
- cell 要量 min / max 寬度，再在分到的寬度裡排，有時再照列高排一次：cell 多的 table 每次排版的工作量是 cell 的數量乘上幾次。
- box tree 多匿名的 table 物件，局部更新時 fixup 要跟著重做。

## 理由和其他做法

完整的比較在 `details.md`。

- **不做**：HTML 的 table 一直是行內內容，Perf-Dashboard 在 K-2 之後也量不對；RFC 0045 第 9 條的暫時對應讓 `display: table-cell` 上下疊。

## 先例

- **Blink（LayoutNG）**：`LayoutTable`、`LayoutTableSection`、`LayoutTableRow`、`LayoutTableCell`、`LayoutTableColumn`；匿名物件在 `AddChild` 時建；`TableLayoutAlgorithm`（欄寬、caption、table 的高度）、`TableSectionLayoutAlgorithm`、`TableRowLayoutAlgorithm`；`table_layout_utils` 的 `ComputeColumnInlineConstraints`、`DistributeTableInlineSize`、`ComputeRowBlockSize`；`TableBorders` 處理 collapsed 的邊框；`TablePainter` 畫背景的層次。沒有 table wrapper box，caption 是 `LayoutTable` 的 child。
- **Gecko**：`nsTableWrapperFrame`（有 wrapper，caption 在 wrapper 裡）、`BasicTableLayoutStrategy`（auto）、`FixedTableLayoutStrategy`。
- **taffy**：沒有 table layout；它的 grid 能近似一部分（`colspan`、`rowspan` 是 span），但欄寬的分配不同（`details.md`）。
- **Servo**：layout 2020 有自己的 table layout（`layout/table/`），在 taffy 之外。

## 已決定的事項

2026-10-14 確認（選項的比較在 `details.md`）：

1. **table 的排版是 kun 自己的 table formatting context，在 taffy 裡是 leaf**（T1）：`layout` 的 `table` 模組照 css-tables 3 和 Blink 的 `TableLayoutAlgorithm`，cell 用 taffy 的 `compute_child_layout` 量和排，接法和 inline formatting context 一樣。不選對應到 taffy 的 grid（T2）：grid 的 `auto` track 的分配和 table 的 auto layout 不同，fixed layout、`border-spacing`、collapsed 的邊框、cell 的 baseline 都要另外補，結果和 Chrome 差，之後還是要換。不選 fork taffy 加 table（T3）：fork 要長期維護，table 和 kun 的 box tree（匿名物件、collapsed 的邊框要的 cell 的 style）綁得很緊。
2. **匿名的 table 物件照 css-tables 3 的 fixup 完整做**（A1）：缺的 row、row group、cell、table 都補（Blink 的 `AddChild`），`display: table-cell` 單獨用的版面和不完整的 HTML table 都和 Chrome 一樣。不選只補常見的（A2）：`display: table-cell` 的版面照樣不對。
3. **不建 table wrapper box**（W1，照 Blink）：caption 是 table 盒子的 child，margin、定位、`transform` 在 table 盒子上，幾何的 API 和 Chrome 一樣。不選照規範建 wrapper（W2，Gecko）：多一層盒子，`width`、margin 要在 wrapper 和 table 之間分。
4. **presentational hint 做 `width`、`height`、`border`、`cellspacing`、`cellpadding`、`align`、`valign`、`nowrap` 和 `<col>`、`<colgroup>` 的 `width`**（`<colgroup width>` 是 2a 加的：HTML 不對應、Chrome 對應，WPT 的 `tentative/colgroup-col.html` 照 Chrome）：`border`、`cellpadding` 照 HTML 給 table 的 cell（Blink 的 `AdditionalCellStyle`）。`bgcolor`、`background`、`bordercolor` 之後和 `<body bgcolor>` 一起做。
5. **做 `empty-cells`**：只影響空的 cell 畫不畫邊框和背景，成本小，Chrome 有。
6. **分兩段**：先 separated 的 table（fixup、hint、table formatting context、paint 的層次、幾何、局部更新，拿掉 RFC 0045 第 9 條的暫時對應），再 `border-collapse`、column 的寬度和背景、`visibility: collapse`；每段做完可以先 merge。
7. **RFC 0046 先做**：Perf-Dashboard 的 table 在 shadow tree 裡，K-2 之前畫不出來；兩份都改 `layout` 的 `collect`，同時做會互相衝突。這份的 fixup 照 0046 的 flat tree 走；第 1 段開始時照 0046 最後的 `collect` 對一次「詳細設計」，要調整的寫進這裡。

2026-10-15，第 1 段開始前確認：

8. **第 1 段分三小段**：1a style（新的 property、UA stylesheet、presentational hint；這期間 layout 照舊把 table 當 block，不 merge 回 `main`）、1b layout（box tree 的結構、fixup、table formatting context、局部更新）、1c paint 和驗收（背景的層次、`empty-cells`、幾何、hit test、WPT、reftest、`about:demo`）；每小段一份 mutation test，`cargo test --workspace`、fmt 和 clippy 在三小段都做完時跑。照第 7 條拿 RFC 0046 最後的 `collect` 對過「詳細設計」：fixup 照 `flat_tree_children` 走、`display: contents` 的元素的子節點當成父元素的，不用調整。
9. **table 的結構是 box tree 裡真的一棵樹**（B1）：table → row group → row → cell，caption 是 table 的 child，匿名的物件是沒有 node 的盒子；taffy 把 table 當 leaf，row group 和 row 的位置、大小由 table 的演算法直接給，cell 交給 taffy 排。每一層有 fragment，row 的背景、幾何、hit test、`offsetParent` 照現有的程式。和 Blink 的 `LayoutTable` → `LayoutTableSection` → `LayoutTableRow` → `LayoutTableCell` 一樣。不選把 table 的 children 攤平成 cell、row 和 row group 只當資料（B2）：row 沒有 fragment，背景、幾何、hit test 要另外做。
10. **欄寬和列高照 Blink LayoutNG 的 `table_layout_utils`**（G1）：`ComputeColumnInlineConstraints`、`DistributeTableInlineSize`、rowspan 的分攤等，大致照 css-tables 3；規範沒定的地方照 Chrome，註解寫明。WPT 和 reftest 的 reference 都是對著 Chrome 驗的。不選照 css-tables 3 的文字（G2）：規範有幾處不完整、還有沒定的 issue，和 Chrome 差得多。

2026-10-15，1b 開始前確認：

11. **1b 再分兩段**：1b-1 是 box tree 的結構（table → row group → row → cell、caption）、完整的 fixup、`inline-table`、auto layout 的欄寬（cell 的 min / max、`colspan`、百分比、table 的 `width`）、列高（不含 `rowspan`）、`border-spacing`、cell 的 `vertical-align`、caption、table 的 baseline、row 和 row group 的 fragment；1b-2 是 fixed layout、`rowspan` 的分攤、table 的 `height` 分給 row group、cell 裡百分比高度的第二次排版、局部更新、shrink-to-fit 和 flex 裡的 table。每段一份 mutation test。1b 是這份 RFC 最大的一段（Blink 的 `TableLayoutAlgorithm`、`table_layout_utils` 和 fixup），一次審太大。不選一次做完。

2026-10-15，1c 跑 WPT 時確認：

12. **compositor 的 tiling 照 cc 只建 interest rect 附近的 tile**（B）：WPT 的 `css/css-tables/crashtests/large-border-crash.html`（`<table border="2026722966">`）讓 kun 吃光記憶體、連開發機的終端機都被 OOM killer 殺掉。原因不在 table：`PictureLayerTiling::new` 替整個 layer 建好每一塊 tile，40 億 px 的 layer 用 f32 每次加 256 已經加不動（f32 在那裡的間距是 512），無限迴圈；就算改成整數，也要建上兆塊。一般的 `<div>` 有超大的邊框也一樣。改成照 Chromium cc 的 `TilingData`：tile 的格子用整數算，只建 interest rect（和 raster 的同一個範圍）裡的 tile，離開的放掉。不選只停用那個 crashtest、另開一份 RFC 修（A）：真的網頁碰到超大的 layer 照樣會出事；不選只把迴圈改成整數（C）：還是要建上兆塊 tile。動到視窗那一層的 compositor，驗收時跑 `test.py automation`。

2026-10-15，第 2 段開始前確認（比較在 `details.md` 的「第 2 段的選項」）：

13. **第 2 段分兩小段**（S1）：2a 是 column 和 column group（盒子、`span`、欄寬、背景）和 `visibility: collapse`；2b 是 `border-collapse`（邊框的解決、排版、繪製）。collapsed 的邊框要和 column、column group 的比，column 先有；每小段一份 mutation test，`cargo test --workspace`、fmt 和 clippy 在兩小段都做完時跑。不選一次做完（S2）：一次審的量和 1b 差不多。
14. **column 和 column group 是 box tree 裡沒有 fragment 的盒子**（C1，Blink 的 `LayoutTableColumn`）：只在 table 裡建（table 外面的照舊不建），帶 style 和 `span`；layout 和 paint 跳過它們，table 從它們讀寬度、背景、`visibility`。style 改了照現有的局部更新走。不選收進 table 的資料（C2）：`<col>` 改了要另外讓 table 重收集。
15. **collapsed 的邊框由 table 一次畫**（P1，Blink 的 `TableBorders`、`TablePainter::PaintCollapsedBorders`）：layout 解決每一段邊，結果放在 table 的 fragment；paint 在所有 cell 的背景之後、cell 的內容之前畫，交接照 Blink。不選每個 cell 畫自己的一半（P2）：交接、虛線的間隔、比 cell 寬的邊都和 Chrome 不同。
16. **`visibility: collapse` 的 row、row group 和 column、column group 都做**（V1，Chrome 的 TablesNG）：欄寬照樣算進去，之後變成 0，跨過它們的 cell 跟著變小、被裁切。不選只做 row（V2）。

2026-10-15，2b 開始前確認：

17. **2b 再分兩段**：2b-1 是邊框的解決和排版（照 Blink 的 `TableBorders`：每一段邊的勝出，cell 的邊框是它四邊的一半、table 的是外圍的，table 的 padding 和 `border-spacing` 不作用，結果放在 table 的 fragment）；2b-2 是繪製（照 `TablePainter::PaintCollapsedBorders`：每一段邊、交接、樣式，和 `visibility: collapse`）、WPT 和 reftest。每段一份 mutation test。兩邊各有幾百行，一次審太大；2b-1 期間畫的邊框位置不對，只在 branch 上。不選一次做完。

## 待決定

無（第 1 到 17 條都已決定）。

## 之後的可能

- table 的 DOM API（`HTMLTableElement` 等的 interface、`rows`、`insertRow()`、`cellIndex`）。
- table 在 multicol、分頁裡的切割。
- `bgcolor` 等顏色和圖片的 presentational hint。
