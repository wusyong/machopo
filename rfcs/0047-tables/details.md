# RFC 0047 的細節：table

## 現況（2026-10-14）

- **style**：`display` 收 table 系列的值（`Display::Table(TableDisplay)`、`InlineTable`，RFC 0045 第 9 條），computed value 照樣；沒有 `border-spacing`、`border-collapse`、`table-layout`、`caption-side`、`empty-cells`（宣告無效）。UA stylesheet 沒有給 `<table>` 等元素 table 的 `display`（`crates/style/resources/html.css` 的開頭寫明），HTML 的 table 是行內內容；沒有 table 的 presentational hint。
- **layout**：`inline-table` 照 `inline-block`，`table-column`、`table-column-group` 不產生盒子，其他的 table 值照 `block`（taffy 拿到 `Block`）；沒有匿名的 table 物件。kun 自己的 formatting context 只有 inline formatting context（`InlineContent`，在 taffy 裡是 leaf，`compute_inline` 用 parley 斷行、經過 `AtomicInlines` 排 atomic inline）；block、flex、grid 是 taffy 0.15 的（`block_layout`、`flexbox`、`grid` 和 `float_layout` 的 feature）。
- **taffy**：沒有 table layout。
- **paint**：照 stacking context 和 CSS 2 附錄 E 的階段畫；沒有 table 的背景層次。
- **DOM**：table 的元素沒有自己的 interface（盤點的「Elements without their interface」有 `td`、`tr`，`createElement` 回傳 `HTMLElement`）；`offsetParent` 已經把 `table`、`td`、`th` 當成候選。
- **WPT**：`include.txt` 只有 `css/css-tables/` 的一個檔案；`css/CSS2/tables/` 沒有。很多其他目錄的失敗記成「The test or its reference has a table, which kun doesn't lay out as one.」（RFC 0043、RFC 0045 寫的原因）。

## Speedometer

- **Perf-Dashboard**（default suite，Speedometer 的 resources/perf.webkit.org）：測試打開 `#/charts/`，`ChartPaneStatusView` 的 shadow tree 有 `<table id="build-revision">`，每次 render 用 `element('tr', …)` 重建它的內容；元件的 CSS 有 `display: table-cell`、`display: inline-table`。元件都是 `ComponentBase`（`attachShadow`），所以 table 在 shadow tree 裡：RFC 0046 之前畫不出來。
- **6 個 Complex-DOM**：共用的靜態 DOM 有一個 `<table role="presentation" class="spectrum-Calendar-table">`（Spectrum 的日曆）；盤點裡的 `table-layout`、`border-collapse`、`-webkit-border-*-spacing` 的宣告來自它的 CSS。靜態的部分，測試的步驟不改它。

## 選項的比較

### table 的排版放在哪裡（第 1 條）

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **T1 kun 自己的 table formatting context，taffy 的 leaf（建議）**：`layout` 的 `table` 模組照 css-tables 3 和 Blink 的 `TableLayoutAlgorithm`；cell 用 taffy 的 `compute_child_layout` 量和排 | 照規範和 Blink 的演算法，WPT 能逐項對；和 inline formatting context 一樣的接法（leaf 加上 children 由自己放），已經有前例；不改 taffy | 演算法要自己寫和維護（欄寬的分配、百分比、`colspan`、`rowspan`、collapsed 的邊框） | 實作大；執行時 cell 量 min / max、排一兩次，和 Blink 同一個量級；taffy 的快取照樣用在 table 和 cell 上 |
| T2 把 table 對應到 taffy 的 grid：每欄一個 `auto` 的 track，`colspan`、`rowspan` 是 span | 實作小，taffy 排 | grid 的 `auto` track 的分配和 table 的 auto layout 不同（table 照 max-content 的比例分多出來的寬度、百分比的欄、table 的 `width`），fixed layout、`border-spacing`、collapsed 的邊框、cell 的 baseline 都要另外補；結果和 Chrome 差，WPT 大多過不了 | 小，但之後還是要換成 T1 |
| T3 fork taffy，在 taffy 裡加 table | taffy 的 API 一致 | fork 要長期維護、跟上游 rebase；table 和 kun 的 box tree（匿名物件、collapsed 的邊框要的 cell 的 style）綁得很緊，放在 taffy 裡要多傳一堆東西；上游沒有計畫 | 大，加上 fork 的維護成本 |

### 匿名的 table 物件（第 2 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **A1 照 css-tables 3 的 fixup 完整做（建議）**：缺的 row、row group、cell、table 都補（Blink 的 `AddChild`） | `display: table-cell` 的 `<div>` 並排（常見的 CSS 版面）、不完整的 HTML table 都和 Chrome 一樣；WPT 的 `css/CSS2/tables/` 有一大批 `anonymous-table-*` | box tree 多一種匿名的盒子，局部更新要跟著重做 fixup（kun 的 `collect` 已經在做匿名 block，同樣的方式） |
| A2 只補常見的（`<table>` 底下直接的 `<tr>`、`<td>`） | 少寫一些 | `display: table-cell` 單獨用的版面（RFC 0045 第 9 條要拿掉的那種）照樣不對 |

### table wrapper box（第 3 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **W1 照 Blink 不建（建議）**：caption 是 table 盒子的 child，margin、定位、`transform` 在 table 盒子上 | box tree 少一層；幾何的 API（`getBoundingClientRect` 回答 table 的盒子，Chrome 包含 caption）和 Chrome 一樣 | 和規範的模型不同，`overflow` 等 property 要決定套在哪裡（照 Blink） |
| W2 照規範建 wrapper box（Gecko 的 `nsTableWrapperFrame`） | 和規範的文字一致 | 多一層盒子；`width`、margin 要在 wrapper 和 table 之間分；和 Chrome 的幾何要對照 |

### HTML 的 presentational hint（第 4 條）

建議做 `<table>` 的 `width`、`height`、`border`、`cellspacing`、`cellpadding`、`align`，`<td>`、`<th>` 的 `width`、`height`、`nowrap`、`valign`、`align`，`<tr>`、section 的 `valign`、`align`，`<col>` 的 `width`：舊網頁和 WPT 的 reference 常用，Chrome 的 `HTMLTableElement::CollectStyleForPresentationAttribute` 等都有。`bgcolor`、`background`（圖片）、`bordercolor` 先不做：kun 的 `<body bgcolor>` 也還沒有，顏色的 hint 一起做比較一致。`cellpadding` 和 `border` 照 HTML 是 table 給它的 cell 的 hint（Blink 的 `AdditionalCellStyle`），不是 table 自己的。

## 第 2 段的選項

2026-10-15 寫。第 2 段做 `border-collapse: collapse`、column 和 column group（寬度、背景、`span`）、`visibility: collapse`。現況：`display: table-column`、`table-column-group` 不產生盒子（`Display::generates_no_box`），`<col width>` 的 hint 有了但沒人讀；`border-collapse` 照 `separate` 排；`visibility: collapse` 和 `hidden` 一樣。

### 分小段

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **S1 分兩小段（建議）**：2a column（盒子、`span`、欄寬、背景）和 `visibility: collapse`；2b `border-collapse`（邊框的解決、排版、繪製） | collapsed 的邊框要拿 column 和 column group 的邊框來比，column 先有；每小段的審查和 mutation test 小一些（和第 1 段一樣，`cargo test --workspace`、fmt、clippy 在兩小段都做完時跑） | 多一次停下來審 |
| S2 一次做完 | 少一次停頓 | 一次審的量和 1b 差不多大 |

### column 怎麼表示

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **C1 column 和 column group 是 box tree 裡的盒子，沒有 fragment（建議，Blink 的 `LayoutTableColumn`）**：table 的 child（column 是 column group 的 child，或 table 直接的），帶 style 和 `span`；taffy 和 table 的演算法跳過它們，table 從它們讀寬度、背景、`visibility` | style 改了照現有的局部更新（restyle、重建）走，不用另外追；畫背景和 collapsed 的邊框要的 style 就在盒子上；和 Blink 的結構一樣 | 要拿掉 `generates_no_box` 的 column 部分（只在 table 裡建；table 外面的照舊不建，Chrome 也不畫），fixup 要把它們當成 table 的 proper child；layout 和 paint 走 children 時要跳過 | 每個 `<col>` 一個 `LayoutBox`，很少 |
| C2 不建盒子，建 table 的盒子時把 column 的 style 和 `span` 收進 table 的資料 | box tree 不多一種盒子 | `<col>` 的 style 或 `span` 改了要另外讓 table 重收集（`<col>` 沒有盒子，DOM 的 flag 找不到人）；背景要另外存 style | 小，但局部更新要另外寫 |

column 的寬度照第 10 條的 G1（Blink 的 `ComputeColumnInlineConstraints`：column 的 `width` 像一個只跨那一欄的 cell 的約束，fixed layout 先看 column 再看第一列），不另外決定。

### collapsed 的邊框怎麼畫

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **P1 table 在所有 cell 的背景之後、cell 的內容之前，一次畫全部的 collapsed 邊框（建議，Blink 的 `TablePainter::PaintCollapsedBorders`）**：layout 照 Blink 的 `TableBorders` 解決每一段邊（`hidden` 勝、`none` 輸、寬的勝、樣式的優先順序、cell > row > row group > column > column group > table），結果放在 table 的 fragment；paint 照每一段的寬度、樣式、顏色畫，交接的地方照 Blink 決定誰蓋誰 | 和 Chrome 一樣的層次和交接；相鄰的 cell 共用一段邊，只畫一次 | 要在 fragment 上多一份邊框的資料；`double`、`dotted` 這類樣式在交接處要照 Blink 處理 | 解決是 O(cell 的邊)，每次 table 排版做一次（Blink 把它快取在 `LayoutTable` 上，style 改了才重算；kun 照 taffy 的快取，table 沒重排就沿用） |
| P2 每個 cell 畫它自己那一半的邊框 | paint 不用改結構 | 交接、`dashed` 的間隔、比 cell 寬的 row 或 table 的邊都和 Chrome 不同；同一段邊畫兩次 | 和 P1 差不多 |

排版照 Blink：table 的邊框是外圍的 collapsed 邊的一半，cell 的是它四邊的一半，`border-spacing` 不作用（第 10 條）。

### `visibility: collapse`

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **V1 row、row group 和 column、column group 都做（建議，Chrome 的 TablesNG 都有）**：row 的高度、column 的寬度變成 0，欄寬照樣把它們算進去（table 的寬度照算 collapse 之前的，再扣掉），跨過它們的 cell 變窄、變矮並被裁切 | 和 Chrome 一樣；WPT 的 `visibility-collapse-*` 大多能過 | column 的 collapse 要在欄寬算完之後再調整位置和跨欄的 cell |
| V2 只做 row | 比較小 | column 的照舊是 `hidden`，和 Chrome 不同 |
