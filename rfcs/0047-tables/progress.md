# RFC 0047 的進度：table

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-15。完成：2026-10-15，Fedora Linux（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0047` 上做，第 1 段做完時先 commit、merge（由人 commit）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a style：新的 property、UA stylesheet、presentational hint | 完成 | 2026-10-15 |
| 1b-1 layout：box tree 的結構、fixup、auto layout 的欄寬、列高、`border-spacing`、`vertical-align`、caption、baseline | 完成 | 2026-10-15 |
| 1b-2 layout：fixed layout、`rowspan`、table 的 `height`、cell 裡的百分比高度、局部更新、shrink-to-fit 和 flex 裡的 table | 完成 | 2026-10-15 |
| 1c paint 的層次、`empty-cells`、幾何、hit test；WPT、reftest、`about:demo` | 完成 | 2026-10-15 |
| 2a column 和 column group（盒子、`span`、欄寬、背景）、`visibility: collapse` | 完成 | 2026-10-15 |
| 2b-1 `border-collapse` 的邊框的解決和排版 | 完成 | 2026-10-15 |
| 2b-2 `border-collapse` 的繪製、WPT、reftest | 完成 | 2026-10-15 |

## 驗收

### style

| 項目 | 怎麼驗證 |
| --- | --- |
| `border-collapse`、`border-spacing`、`table-layout`、`caption-side`、`empty-cells` 的解析和序列化（`border-spacing` 一樣的兩個長度寫成一個、不收負的和百分比）；computed value（`calc()` 負的時候是 0）；`table-layout` 以外都繼承，`unset`、`inherit`、`initial` | `cargo test -p style --test it tables::the_table_properties_parse_and_serialize`、`tables::the_table_properties_compute_and_inherit`；WPT 的 `css/css-tables/parsing/`（15 個） |
| UA stylesheet：table 元素的 `display`，`table` 的 `border-spacing: 2px`、`border-box`、`text-indent: initial`，row group 的 `vertical-align: middle` 傳到 row、cell，DOM 放進 table 的 row 也是 `middle`，`th` 粗體置中，cell 的 `padding: 1px`；`border-color` 都是 `currentColor`（Chrome 現在的做法） | `cargo test -p style --test it tables::the_user_agent_stylesheet_has_tables`、`tables::a_table_resets_text_indent_and_its_own_rows_are_middle`；WPT 的 `table-bordercolor-default.html` |
| presentational hint：`<table>` 的 `width`（忽略 0）、`height`、`border`（不是非負整數時是 1，不是 0 時 `outset`）、`cellspacing`（不收百分比）、`align`；cell 的 `width`、`height`（忽略 0 和 0%）、`nowrap`，它的 table 的 `border`（`1px inset`，cell 自己的顏色）和 `cellpadding`（非負整數）；row group、row、cell 的 `valign`、`align`（當成 CSS 解析，`middle`、`absmiddle` 置中）、row 的 `height`、`<col width>`；cell 照 HTML 的 table model 找 table | `cargo test -p style --test it tables::the_table_hints`、`tables::the_cell_hints`、`tables::a_cell_takes_its_own_tables_hints`、`tables::the_row_and_column_hints`；WPT 的 `html/rendering/non-replaced-elements/tables/` 的 `table-attribute.html`、`table-border-color-not-inherited.html`、`table-valign-baseline-ascii-case-insensitive.html`、`table-rules-computed-style.html`（失敗的寫在 `.ini`） |
| table 的 `cellpadding`、`border` 改了，cell 跟著重算（和全部重算的結果一樣） | `cargo test -p style --test it invalidation::a_tables_attribute_styles_its_cells_again` |

### layout

| 項目 | 怎麼驗證 |
| --- | --- |
| fixup：table 外面的 cell、row、row group、caption 包進匿名的 table（行內元素裡的是 inline-table，一個 atomic inline），中間可摺疊的空白也包進去；table、row group、row 裡不是它的部分的內容包進匿名的 row group、row、cell；table、row group、row 裡只有空白的文字節點不產生東西（`white-space: pre` 也一樣，Chrome 的）；row 的 cell 之間的 out-of-flow 盒子是 row 的 child；table 內部的值被 blockify 時是 `block` | `cargo test -p layout --test it tables::the_fixup_makes_the_missing_boxes`、`tables::white_space_between_table_parts`、`tables::a_cell_in_an_inline_element_is_an_inline_table`、`tables::what_isnt_a_rows_cell`、`generated::table_parts_go_into_an_anonymous_table`；`cargo test -p style --test it computed::table_displays` |
| 欄寬（Blink 的分配）：`auto` 的 table 是內容的最大寬度、放不下時縮到最小寬度；比內容寬時分給 `auto` 的欄（照最大寬度的比例），再來是固定寬度和百分比的欄，超過全部的最大寬度時先給 `auto` 的、再給固定的、最後給百分比的；cell 的固定、百分比 `width`（`calc()` 的百分比加 0px 也是百分比）、`min-width`、`max-width`（比 `min-width` 小時照 `min-width`）；`colspan` 的寬度照最大寬度的比例分給它跨的欄；百分比合計不超過 100；有百分比的欄的 `auto` table 寬到每個百分比都成立，但它的 max-content 寬度和對容器的貢獻是各欄最大寬度的和；沒有 cell 開始的欄併掉、沒有自己的間距；table 的 `width`、`min-width`、`max-width`，不比內容的最小寬度窄（給定的寬度也是）；量 block 容器的寬度時 table 照內容、不照自己的 `width`，排版時照 `width` 避開 float；caption 的最小寬度；cell 的百分比 padding 量欄寬時是 0（`calc()` 的長度照算），之後是 table 的內容寬扣掉外圍的間距；沒有欄時 row 和 table 的內容一樣寬 | `cargo test -p layout --test it tables::columns_are_sized_from_their_cells`、`tables::more_column_sizing`、`tables::a_tables_own_sizes`、`tables::border_spacing_and_the_parts_boxes`、`tables::captions_go_above_and_below`、`tables::intrinsic_widths_and_growth_like_chrome`、`tables::a_table_sized_to_fit` |
| 列高和 cell：row 是它的 cell 和自己的 `height` 中最高的，cell 的 `height` 是最小值、`max-height` 不作用，每個 cell 和它的 row 一樣高；`rowspan`（0 到 row group 的最後）要的高度給它跨的列；`vertical-align` 的 top、middle、bottom、baseline（其他的值當 baseline），block 內容和行一起移動，baseline 對齊的 cell 的 `height` 不往 baseline 下面推；空 cell 的 baseline 是排出來的 content box 的底（不是 `height` 的）；cell 的 margin 不作用；header group 在最前、footer group 在最後 | `cargo test -p layout --test it tables::rows_are_as_tall_as_their_cells`、`tables::vertical_align_places_a_cells_content`、`tables::a_cells_block_content_is_aligned`、`tables::more_baselines`、`tables::row_groups_and_rowspans`、`tables::heights_and_sizes_like_chrome` |
| `border-spacing`：cell 之間和外圍；row group、row 跨在欄上，不含外圍的間距 | `cargo test -p layout --test it tables::border_spacing_and_the_parts_boxes` |
| caption 在格子的上面、下面，和 table 一樣寬，`position: relative` 移動它 | `cargo test -p layout --test it tables::captions_go_above_and_below` |
| baseline：table 的是第一列的（沒有 baseline 對齊的 cell 時是第一個 cell 的；決定它的 cell 沒有行時，是那個 cell 最後的高度的 content box 的底）；inline-table 和最後是 table 的 inline-block 照它對齊 | `cargo test -p layout --test it tables::a_tables_baseline_is_its_first_rows`、`tables::more_baselines` |
| table 在 block 裡：不被拉寬、`margin: auto` 置中、避開旁邊的 float、在 inline-block 裡縮到內容寬；空的 table 是它的邊框和自己的大小，margin 不穿過它 | `cargo test -p layout --test it tables::a_table_in_a_block`、`tables::an_empty_tables_margins_dont_collapse_through_it` |
| out-of-flow：positioned 的 cell、row 是包含塊；往上傳的 static position 照 cell 的內容移動（cell 裡沒有 in-flow 的內容時留在 cell 的頂端） | `cargo test -p layout --test it tables::out_of_flow_boxes_in_cells`、`tables::a_positioned_row_is_a_containing_block`、`tables::what_isnt_a_rows_cell` |
| fixed layout（`table-layout: fixed` 而且 `width` 不是 `auto`）：只看第一列的 cell 的 `width`（跨欄的平分，不併欄；content-box 的單欄百分比 cell 的欄加上它的 padding），剩下的平分給 `auto` 的欄，沒有時照比例給固定寬度的欄（寬 0 的不長）、再沒有時照百分比給百分比的欄；比固定寬度的欄窄時百分比的欄照含 padding 的寬度分剩下的；不比固定寬度的欄窄；`width: auto` 時照 auto | `cargo test -p layout --test it tables::fixed_table_layout`、`tables::fixed_layout_like_chrome` |
| 高度的分攤（Blink 的）：分給沒有固定高度的列（row 或它的 rowspan 1 的 cell 沒有固定的 `height`），照高度的比例，全是 0 時平分（`rowspan` 偏好最後一列），都固定時才給固定的；`rowspan` 的 cell 多要的跨得少的先；table 的 `height` 多的先讓百分比 `height` 的 row group 長到它的百分比，再依序給沒有固定高度的 row group、有的（有固定的列也算）、百分比的，後兩種先給 body，空的 row group 也算；row group 的 `height` 給它的列 | `cargo test -p layout --test it tables::heights_are_spread_over_rows`、`tables::rows_are_as_tall_as_their_cells`、`tables::a_tables_own_sizes`、`tables::heights_and_sizes_like_chrome` |
| cell 照列的高度再排一次：有百分比高度的 child 而且高度有設定（cell 或 table 的 `height`，不看 row 的，和 Chrome 一樣），或 positioned（裡面的絕對定位盒子照 cell 的高度）；第二次排版的 static position；量內容時不用 style 的 `height` 解百分比 | `cargo test -p layout --test it tables::a_cell_laid_out_at_its_height`、`tables::heights_and_sizes_like_chrome` |
| flex item、絕對定位的 table 照內容寬，在最小和最大寬度之間；flex item 的百分比 `width` 是它的 basis | `cargo test -p layout --test it tables::a_table_sized_to_fit` |
| 局部更新（每一步和重建的樹比對）：插入 row、改 cell 的文字、移除 cell、改 `colspan`、`rowspan`（cell 重建）、row 和 cell 的高度、row 裡直接的文字和 row group 裡直接的 cell；只改畫的 style 時重收集繼承它的匿名 table 盒子（box 和行內元素）；重排過的 cell 的內容重新對齊（行和 block） | `cargo test -p layout --test it update::tables_follow_the_dom`、`update::anonymous_table_boxes_take_their_parents_style`、`update::a_cell_laid_out_again_is_aligned_again` |
| column 和 column group：table 裡的才有盒子（沒有 fragment）；`width` 是固定欄的最大寬度（cell 的內容能撐寬）、百分比、`min-width`、`span`；column group 的固定 `width` 給它沒有寬度的 column，沒有 column 時給自己的 `span` 欄；有寬度的 column 沒有 cell 也佔位，沒有寬度（或 0）的併掉；fixed layout 先看 column 再看第一列（跨欄 cell 的百分比只給 `auto` 的欄）；沒有固定寬度的跨欄 cell 不撐寬固定欄；column 的 style、`span`、新增的 column 局部更新 | `cargo test -p layout --test it tables::columns_give_widths`、`tables::columns_in_fixed_tables_and_spans`、`update::columns_follow_the_dom`；`cargo test -p style --test it tables::the_row_and_column_hints` |
| `visibility: collapse`：欄照樣量、再收起來（table 變窄、跨過的 cell 變窄、照新的寬度重新量和對齊、只在它裡面的 cell 藏起來），列高照 collapse 之前的欄寬算；列（和 row group 的列）高 0、沒有間距、藏起來，它的 cell 高 0，跨進來的 cell 少掉它，有 `height` 的 table 扣掉它 | `cargo test -p layout --test it tables::collapsed_columns_and_rows` |
| `border-collapse: collapse`：每一段邊取它所在的盒子（cell、row、row group、column、column group、table）裡最寬的邊框，`hidden` 勝過一切，再來是樣式高的（`inset` 當 `ridge`、`outset` 當 `groove`），平手時先合併的；跨欄跨列的 cell 裡面的邊不畫；cell 的邊框是它四邊的一半，table 的是外圍的一半；table 沒有 padding 和 `border-spacing`，沒有列時沒有邊框；切換 `border-collapse`、改 cell 的邊框局部更新，table 的部分只改顏色時 table 也重排 | `cargo test -p layout --test it tables::collapsed_borders`、`update::collapsed_borders_follow_the_dom`、`update::a_collapsed_borders_color_follows_the_dom` |

### paint、幾何、compositor

| 項目 | 怎麼驗證 |
| --- | --- |
| 層次（Blink 的 `TablePainter`，separated）：table 的背景和邊框包住格子、不含 caption；row group、row 的背景畫在每個 cell 裡（不在 border spacing 裡），順序是 row group、row、cell；`empty-cells: hide` 的空 cell 不畫（它的 row 的背景也不畫在那裡） | `cargo test -p paint --test it tables::`；kun 的 reftest `tables/t1-separated.html` |
| column group 和 column 的背景畫在從它們的欄開始的 cell 裡（group 在下），不在間距裡、不在藏起來的 cell 裡；layout 藏起來的（收起來的列、只在收起來的欄裡的 cell）不畫、打不到（pre-paint 的空 clip） | `cargo test -p paint --test it tables::column_backgrounds_go_in_the_cells_that_start_in_them`、`hit_test::hidden_table_parts_are_not_hit`；kun 的 reftest `tables/t3-columns.html` |
| collapsed 的邊框由 table 在所有 cell 的背景之後畫：每一段在格子線上、照它的寬度；交接處照寬度、樣式、box order 決定誰占（Blink 的 `ComputeEdgeJoints`）；`double`、`dashed`、`dotted`、`ridge`、`groove` 的畫法；cell 和 table 不畫自己的邊框，`empty-cells` 不作用；table 不可見時不畫；positioned 的 cell（和它的 row、row group 的背景）裁在邊框以內 | `cargo test -p paint --test it tables::a_table_paints_its_collapsed_borders`、`tables::collapsed_border_styles_and_spans`、`tables::a_positioned_cells_background_stays_inside_its_collapsed_border`；kun 的 reftest `tables/t4-collapsed.html` |
| 新的 fragment 樣式和大小一樣時沿用上次的 item，table 的部分除外（它們畫的東西看 child 的位置：caption 換邊、row 少了 cell） | `cargo test -p paint --test it paint_controller::table_parts_paint_again_when_their_children_move`、`paint_controller::collapsed_borders_paint_again`；WPT 的 `css/css-tables/paint/table-border-paint-caption-change.html`、`row-background-paint-remove-last-cell.html` |
| 幾何：table、caption、row group、row、cell 的 `getBoundingClientRect`、`offsetWidth` 等，row group 和 row 跨在欄上，點到 cell 打到 cell，cell 和 row 的 `offsetParent` 是 table | `cargo test -p shell --test it geometry::tables_have_boxes` |
| compositor 只建 interest rect 附近的 tile（照 cc 的 `TilingData`，整數的格子）：40 億 px 見方的 layer 只有視窗附近的 20 塊 tile，最後一塊在內容的邊上 | `cargo test -p compositor --lib a_huge_layer_has_tiles_only_near_the_viewport`；WPT 的 `css/css-tables/crashtests/large-border-crash.html`、`html/canvas/element/canvas-host/2d.canvas.host.size.large.html` |

### WPT、reftest、`about:demo`

| 項目 | 怎麼驗證 |
| --- | --- |
| WPT 的 `css/css-tables/`、`css/CSS2/tables/` 整個，清單裡其他用到 table 的測試（失敗的寫在 `.ini`） | `python3 scripts/test.py wpt css/css-tables/ css/CSS2/tables/`；結果見下面的「做過的檢查」 |
| kun 的 reftest：separated 的層次、欄寬和列高、fixed layout、column 的寬度和背景、`visibility: collapse`、collapsed 的邊框 | `python3 scripts/test.py reftest tables/` |
| `about:demo` 的表格（caption、header 列、`colspan`、40 px 高的列、`border`、`cellspacing`、第一欄的 `<colgroup>` 背景）、「Add a row」、「Collapse a column」和「Collapse borders」 | `python3 scripts/test.py scenarios tables`；真的視窗：`python3 scripts/test.py automation tables` |

## 做過的檢查

- **做了什麼**（第 1 到 17 條）：
  - `computed_style`、`css`：`border-collapse`、`border-spacing`（一個兩值的 longhand，Chrome 的 `-webkit-border-*-spacing` 沒做）、`table-layout`、`caption-side`、`empty-cells` 的解析、套用、繼承、序列化（`border-spacing` 一樣的兩個值寫一個，Blink 的 `kDropIdenticalValues`）；`Display::blockified` 把 table 內部的值變成 `block`。
  - `style`：UA stylesheet 的 table 規則；`hints.rs` 的 table、cell、row、column 的 hint（`<colgroup width>` 照 Chrome 也有）；cell 照 HTML 的 table model 找它的 table。
  - `layout` 的 `table` 模組：fixup（不是 table 的部分的內容包進匿名的 row group、row、cell，誤放的 table 部分包進匿名的 table）；格子（header group 最前、footer group 最後，`rowspan`、併欄，column 和 column group 的定義）；auto 和 fixed 的欄寬照 Blink 的 `table_layout_utils`；列高和高度的分攤；caption；cell 排兩次（量內容，必要時照最後的大小再排）；`vertical-align`、baseline；out-of-flow；`visibility: collapse`；`TableBorders`（collapsed 的邊框的合併和每個 cell 的一半）和格子線的幾何（`CollapsedBorders`，放在 table 的 fragment）。`lib.rs`：`LayoutBox` 的 `span`、`content_shift`、`table_baseline`、`collapsed_border`；`BoxRole::AnonymousTable`；table 交給 `compute_table`（taffy 的 leaf）；只改畫的 style 時重收集匿名 table 盒子、column 和 collapse 的 table 的部分改了 table 重排；`BoxFragment` 的 `hidden`、`columns`、`border_collapsed`、`background_inside_border`、`collapsed_borders`。
  - `paint`：照 Blink 的 `TablePainter` 的層次（table 的背景和邊框只包住格子；column group、column、row group、row 的背景畫在 cell 裡；`empty-cells`）；collapsed 的邊框（`collapsed_borders`、`edge_joints` 照 `ComputeEdgeJoints`、`border_side` 照 `DrawBoxSide`）；另外畫的 cell 的背景裁在邊框以內；`PaintController` 不沿用新的 table 部分的 fragment 上次的 item；pre-paint 給 `hidden` 的 fragment 空的 clip。
  - `compositor`（第 12 條）：`PictureLayerTiling` 照 cc 的 `TilingData`，只建 interest rect 裡的 tile。起因：`css/css-tables/crashtests/large-border-crash.html`（`<table border="2026722966">`）讓 kun 吃到 16 GB，OOM killer 連開發機的終端機一起殺掉（之後跑 WPT 都包在 `systemd-run --user --scope -p MemoryMax=24G -p MemorySwapMax=0` 裡）；40 億 px 的 layer 用 f32 一次加 256 加不動，`PictureLayerTiling::new` 是無限迴圈。改了之後那份頁面 58 MB、0.08 秒。
  - `dom`：`<td>`、`<th>` 的 `colspan`、`rowspan`，`<col>`、`<colgroup>` 的 `span` 改了重建盒子。
  - `about:demo` 的表格（caption、`<thead>`、`colspan`、`<colgroup>`、40 px 高的列、`border`、`cellspacing`）和「Add a row」、「Collapse a column」、「Collapse borders」；新的情境 `tables`，`demo-script` 的按鈕清單是 45 個。kun 的 reftest `tables/` 四份。
  - 文件：`docs/limitations.md`（table 那一條改寫成已經做的和與 Chrome 不同的地方；拿掉 `visibility: collapse`、`<colgroup width>`；stylesheet 不擋 script；layer 的 tile 改成 `LayoutUnit` 的上限；Linux 的真視窗）、`docs/architecture.md`（`layout`、`paint`、`compositor` 列）、`docs/test-gaps.md` 的「等價、不用處理的 mutation」。
- **對照 Blink 的方法**：第 1 段照 WPT 的期待值寫，用一個暫時的測試（不留在 repo 裡）把 css-tables 的 testharness.js 測試的 HTML 和 `support/base.css` 直接排版，和 `data-expected-*` 比：779 項裡不同的從 208 項修到 65 項。第 2 段起抓 Chromium 的 `table_layout_utils.cc`、`table_layout_algorithm.cc`、`table_borders.cc`、`table_painters.cc` 等（不進 repo）照著寫，第 1 段的欄寬分配和它比過、只差 `treat_target_size_as_constrained`；之後不同的降到 35 項（剩下的是直排、quirks mode、`-webkit-fill-available` 和下面「和 Chrome 不同的」）。
- **和原本的計畫不同的**：
  - `border-color`：HTML 的 rendering 一節是 `table { border-color: gray }`、cell 繼承 table 的顏色；WPT 的 `table-bordercolor-default.html`、`table-border-color-not-inherited.html`（crbug.com/494554835、crbug.com/40745409）期待 Chrome 現在的做法：都是 `currentColor`。照 Chrome。
  - `valign`、`align` 照 Blink 的 `HTMLTablePartElement` 當成 CSS 解析，只另外處理 `middle`、`absmiddle`。
  - `<colgroup width>`（第 4 條改了）：原本不做，Chrome 有、WPT 的 `tentative/colgroup-col.html` 照它。
  - column 的背景第一版照幾何相交畫，讀了 Blink 的原始碼之後改成畫在從那一欄開始的 cell 裡。
- **找到並修的**（WPT、對照 Blink 抓到的）：
  - 內在寬度：max-content 和對容器的貢獻是各欄最大寬度的和，不為了百分比的欄加寬；超過全部的最大寬度時先給 `auto` 的、再給固定的、最後給百分比的欄。fixed layout：`width` 不是 `auto` 就是 fixed；不併欄；content-box 的單欄百分比 cell 的欄加上它的 padding；寬 0 的固定欄不長。
  - 高度：量內容時不用 style 的 `height` 解百分比；row 的 `height` 不讓 cell 的高度 definite；列和 row group 的分攤照 Blink；baseline 對齊的 cell 往下只算內容；空 cell 的 baseline 不照它的 `height`，但第一列只有沒有行的 cell 時 table 的 baseline 照它最後的高度；cell 的 `max-height` 不作用。
  - 大小：table 不比內容小；量 block 容器的寬度時 table 照內容（第一版排版時也這樣，`floats-wrap-bfc-005` 抓到 table 不再避開 float，改成只在量寬度時）；cell 的百分比 padding 量欄寬時是 0；`calc()` 的百分比加 0px 是百分比；cell 的 `max-width` 比 `min-width` 小時照 `min-width`（`td-min-width-auto-layout`）。
  - 其他：table、row group、row 裡只有空白的一段才丟；caption 的 `position: relative`；cell 裡沒有 in-flow 的內容時 out-of-flow 的 static position 不跟著對齊移動；column 沒清 `needs_layout`，第二次改 column 時傳不到 table（`update::columns_follow_the_dom`）；collapse 的 table 的 padding 是 0（`collapsing-border-model-013`、`014`）；positioned 的 cell 的背景蓋掉 table 先畫的邊框（`collapsed-border-positioned-tr-td`，照 Blink 的 `TableCellBackgroundClipper` 裁在邊框以內）；`PaintController` 沿用了 caption 換邊、row 少了 cell 的 table 部分的 item（`paint/table-border-paint-caption-change.html`、`row-background-paint-remove-last-cell.html`）。
  - 驗收時在真的視窗裡看到的：跨過收起來的欄的 cell 照新的寬度重排之後，仍然照收起來之前量的內容高度對齊，`about:demo` 換成兩行的文字超出它的列；改成照新的寬度再量一次、照它對齊（列高照舊用收起來之前的），補了 `tables::collapsed_columns_and_rows` 的一個情況和一條 mutant。
  - 測試自己的錯：flex item 的測試期待值算錯（50px 的容器放得下縮小的 table），改成 30px。
- **和 Chrome 不同、記在 `docs/limitations.md` 和 `.ini` 的**：`rowspan-height-redistribution` 的 table 2、6、8（kun 照 Blink 的原始碼排出 51px，測試期待 50px）；`table-border-collapse-client-width-height`（Blink 的 `clientWidth` 含 collapsed 的邊框）；`subpixel-collapsed-borders-003`（Chrome 把 computed 的邊框寬度取整到 device pixel）；跨過收起來的欄、列的 cell 的溢出不裁掉（`visibility-collapse-rowspan-005`、`column-visibility-004`、`visibility-collapse-colspan-003`）；`col-definite-*` 的 reference 照 Firefox 丟掉 cell 以外有寬度的欄、kun 照 Blink 留著；沒有 `<table rules>`（`rules-groups`）和 `direction`（`col-paint-htb-rtl`，以前沒有 column 的背景時剛好通過）。
- **舊的測試跟著改**：`layout` 的 `generated::table_displays_are_blocks`（「table 的值照 block 排」的暫時行為）改成 `generated::table_parts_go_into_an_anonymous_table`；`tables::rows_are_as_tall_as_their_cells`、`a_tables_own_sizes`、`heights_are_spread_over_rows`、`a_cell_laid_out_at_its_height` 照 Chrome 的分攤和 definite；`style` 的 `tables::the_row_and_column_hints`（`<colgroup width>`）；`shell` 的 `sites::gutenberg_contents_link_to_their_chapters` 的點擊位置（Gutenberg 的目錄是一個 table，以前照 block 排在書背圖片下面）。
- **mutation test**（每一小段一份，都照最後的程式重跑過一次）：
  - 最後一次（2026-10-15）：`mutants-1a.toml` 34 條、`1b-1` 52 條、`1b-2` 24 條、`1c` 41 條、`2a` 33 條、`2b-2` 21 條全部抓到；`2b-1` 17 條裡 16 條抓到，1 條等價（下面）。共 222 條。
  - 每段第一次沒抓到的，多數是測試沒碰到的情況，都補了測試：`unset` 和 `initial`（值本來就等於繼承來的）、`align=absmiddle` 從 row group 繼承到同樣的值、cell 的 `width=0%`；`overflow: hidden` 的 inline-table 的 baseline、誤放的 table 部分後面的空白、`colspan` 的最小寬度；跨欄 cell 的百分比在 fixed layout、第二次排版的 static position、重排過的 cell 的 block 內容；table 不比內容小的兩道保護（只有 flex item 有 `min-width: 0` 時用得到）、只有 `rowspan` 跨進來的空列、cell 的 `max-height`（只有 block 的內容看得出來）、最後一塊 tile 切在內容的邊上（40 億 px 剛好是 256 的倍數）；column 的百分比和 `min-width`、縮窄的 cell 的百分比寬度的 block、row group 收起來但它的列設回 `visible`；`hidden` 的邊被後面更寬的蓋掉、cell 重新算 style 後留著沒變的 collapsed 邊框；collapsed 邊框交接處的三條規則。多餘的程式拿掉（和那一條）：cell 的 margin 和 row 的 padding 在 taffy style 裡歸零（它們從不經過 taffy 的父演算法放）、只改畫的 style 時看匿名 block 裡的匿名 table 盒子（那種一定在行內元素裡）、`valign` 先比對關鍵字再退回 CSS。
  - 等價的（記在 `docs/testing.md`）：算 cell 的 collapsed 邊框寬度時不看邊能不能畫（`none`、`hidden` 的 computed 寬度本來就是 0）。
  - rustfmt 和後面的段落改過的程式，前面的規格的片段跟著更新再跑。
- **WPT**（`include.txt` 的 table 那一段是 `css/css-tables/`、`css/CSS2/tables/` 整個，加上 `html/rendering/non-replaced-elements/tables/` 的五個；預期結果用 `MACHOPO_UPDATE_EXPECTATIONS` 寫下，每個失敗的 `.ini` 寫了原因）：
  - `css/css-tables/` 339 個裡 244 個整個通過（子測試 859 項裡 466 項；第 1 段做完時 179 個、410 項）；`css/CSS2/tables/` 374 個裡 367 個（第 1 段 297 個）；`css/css-tables/parsing/` 15 個全部通過。
  - 清單裡其他用到 table 的 277 個測試裡 221 個（原本記成「table 的版面」的失敗大多通過，`.ini` 刪掉）。
  - 還失敗的主要原因：讀 `support/base.css` 排出來的版面的測試在 stylesheet 套用前就量（kun 的 stylesheet 不擋 script）、直排、window 的具名屬性、`document.fonts`、quirks mode、`bgcolor` 這些沒有對應的 attribute，和上面「和 Chrome 不同的」。
- **跑過的**：改到的 crate 的測試；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；兩段各自做完時的 `cargo test --workspace`（第 1 段 2765 個、第 2 段 2778 個通過）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`；`test.py reftest`（全部 167 個，0 個預期外）；`test.py scenarios`（全部 52 個）；`test.py automation`（41 份裡 36 份通過，`tables`、`demo-script` 通過；失敗的五份（`intersection-observer`、`position-relative`、`resize-observer`、`resize`、`wasm-demo`）和第 1 段一樣，都和 Linux 上視窗的大小有關，在 `test.py scenarios` 裡通過，記在 `docs/limitations.md` 的「平台和建置」；真視窗的截圖裡 `<th>` 不是粗體（系統字型），沒有平台視窗的版本是粗體）；上面的 WPT（最後的修正之後 `css/css-tables/`、`css/CSS2/tables/` 再跑一次，0 個預期外）。
- **沒跑的**：`matrix`、`fuzz`（沒有動到 GC）；整套的 `wpt`、`wdspec`；`ci.py speedometer`（只在使用者要求時跑）。`cargo clean` 等 merge 回 `main` 之後跑。
