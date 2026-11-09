# RFC 0003 的進度：flex 和 grid layout

開始日期：2026-10-06

完成：2026-10-07，Windows 11（自動測試和真視窗的情境）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a. flex 的 layout | 完成 | 2026-10-06 |
| 1b. flex item 的 paint、`z-index`、hit test | 完成 | 2026-10-06 |
| 2. intrinsic size | 完成 | 2026-10-06 |
| 3a. grid 的 layout 和 paint | 完成 | 2026-10-07 |
| 3b. 具名的線和 areas | 完成 | 2026-10-07 |
| 4a. `inline-block` 和 atomic inline 的機制 | 完成 | 2026-10-07 |
| 4b. `inline-flex`、`inline-grid` 和 WPT | 完成 | 2026-10-07 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **style** | |
| `display` 的五個新值；flex 的屬性和 shorthand（`flex`、`flex-flow`、`gap`）的 parse、不合法的值丟掉、CSS-wide keyword | `cargo test -p style --test cascade`（`flex_properties`、`flex_shorthand`、`flex_properties_take_css_wide_keywords`、`what_is_ignored`） |
| grid 的屬性和 shorthand：track list（`repeat()`、`minmax()`、`fit-content()`、`fr`）、`auto-fill` / `auto-fit` 的限制、`grid-auto-*`、擺放、`justify-*`、`place-*`、舊的別名；線的名稱、`<grid-line>` 的名稱和 `span`、`grid-template-areas`、`grid-template` 的三種寫法、shorthand 省略的線沿用名稱；CSS-wide keyword | `cargo test -p style`（`grid::tests`；`cascade` 的 `grid_properties`、`place_and_gap_shorthands`、`grid_properties_take_css_wide_keywords`、`grid_names_areas_and_grid_template`） |
| `width`、`height` 的三個關鍵字和 `flex-basis: content`；`min-*`、`max-*`、`margin` 上的關鍵字、`stretch`、`fit-content()`、加前綴的寫法都丟掉 | `cargo test -p style --test cascade`（`intrinsic_size_keywords`） |
| blockification：flex 和 grid item、root；inline-level 的三個變成 `block` / `flex` / `grid`；父元素換 `display` 時 child 跟著重算 | `cargo test -p style --test cascade`（`flex_items_are_blockified`、`grid_items_are_blockified_with_their_z_index`、`atomic_inlines_are_blockified_as_items_and_as_the_root`）；`cargo test -p style --test invalidation`（`a_flex_containers_children_follow_its_display`） |
| `z-index` 的 parse；只對 flex 和 grid item 生效，父元素換 `display` 時跟著變；生效時建立 stacking context；只重畫 | `cargo test -p style`（`cascade` 的 `z_index_applies_to_flex_items`、`invalidation` 的 `a_flex_items_z_index_follows_its_parent`）；`cargo test -p layout --test update`（`a_z_index_only_paints_again`） |
| **flex 和 grid 的 layout** | |
| 匿名 item、只有空白（不管 `white-space`）的那段不產生 item；只有文字的 container 有一個匿名 item | `cargo test -p layout --test flex`（`inline_children_are_items_and_text_runs_anonymous_items`、`a_flex_container_with_only_text_has_one_anonymous_item`） |
| taffy 的 flex：方向、換行和 gap、grow / shrink / basis、對齊、stretch、`min-width: auto`、flex 的 root | `cargo test -p layout --test flex` |
| taffy 的 grid：track 的大小、gap、照數字、`span` 和名稱擺放、areas 和它們隱含的線、sparse 和 `dense`、對齊、`order` 和匿名 item | `cargo test -p layout --test grid` |
| `order`：照 `order` 排版、fragment 照同一個順序，相同的維持文件順序；不是 item 的不受影響 | `cargo test -p layout --test flex`（`order_lays_items_out_and_makes_their_fragments_in_order`、`order_only_applies_to_flex_items`） |
| `align-items: baseline`：IFC 的第一條 baseline、block 從第一個 child 傳上來；measure 量的是 content box | `cargo test -p layout --test flex`（`baseline_alignment_lines_up_the_first_lines`、`baselines_come_from_the_lines_at_the_final_width`、`a_padded_items_text_wraps_in_its_content_box`） |
| 當 item 的 `<img>`：flex 裡 stretch 照比例、不縮到自然寬度以下；grid 裡（包括 inline-grid）預設不拉伸，`place-items: stretch` 時拉伸，只在一軸拉伸時另一軸照比例；block 裡的照舊 | `cargo test -p layout --test update`（`an_image_flex_item_is_sized_by_taffy`）；`cargo test -p layout --lib`（`measure_fills_in_what_isnt_known`） |
| 局部更新：`order` 和擺放只重排 container、不建 box；插入和拿掉 item；container 換 `display`；插入的文字變成匿名 item；每次都和整份重建一樣 | `cargo test -p layout --test update`（`an_items_order_only_lays_its_container_out_again`、`an_item_inserted_or_removed_goes_where_its_order_says`、`a_containers_display_builds_its_items`、`text_in_a_flex_container_is_an_anonymous_item`、`a_grid_items_placement_only_lays_its_container_out_again`） |
| **intrinsic size** | |
| block 的 `min-content`、`max-content`、`fit-content`（有空間、空間不夠、比 min-content 還窄）、padding 和 border、`height` 上當成 `auto`；flex item 的 `flex-basis: content` 和關鍵字 | `cargo test -p layout --test block`（`intrinsic_size_keywords_size_a_block_to_its_content`）；`cargo test -p layout --test flex`（`intrinsic_sizes_of_flex_items`） |
| 百分比寬度的圖片在 intrinsic size 裡：max-content 時是自然寬度、min-content 時是 0 | `cargo test -p layout --test update`（`an_images_percentage_width_in_an_intrinsic_size`）；`cargo test -p layout --lib`（`sizes_follow_the_natural_ratio`） |
| 只被量過、排版命中快取的 box 沿用 fragment | `cargo test -p layout --test update`（`a_box_only_measured_keeps_its_fragment`） |
| **atomic inline** | |
| shrink-to-fit、固定和百分比寬度、margin 留下的空間；整個放到下一行；在 inline 元素和匿名 block 裡；剛好放得下的文字不換行；`inline-flex`、`inline-grid` 照 item 算寬度 | `cargo test -p layout --test inline_block` |
| baseline：最後一行、`overflow` 不是 `visible`、沒有行、裡面的 block、裡面會裁切的 block、上 margin；inline-flex 的第一個 | `cargo test -p layout --test inline_block`（`on_the_baseline`、`inline_flex_and_grid_containers`、`what_resolves_against_its_containing_block_and_its_own_width`） |
| 百分比高度（包括 `max-height` 夾過的和被拉高的 flex item 裡的）、inline-grid 的百分比 `gap` | `cargo test -p layout --test inline_block`（`what_resolves_against_its_containing_block_and_its_own_width`） |
| 局部更新：裡面的東西改了只重排那一行、不重新收集外面；只改顏色不排版；保留的 inline-block 換到新的匿名 block；視窗改了寬度和高度時百分比跟著改；圖片載入時建 box | `cargo test -p layout --test update`（`an_inline_block_changes_in_place`、`a_kept_inline_block_moves_to_the_new_anonymous_block`、`an_inline_images_percentage_follows_the_viewport`、`an_inline_blocks_percentage_height_follows_the_viewport`、`an_image_takes_its_size_when_it_arrives`） |
| inline 圖片改走同一條路：既有的圖片測試、`sites`、reftest 照舊 | `cargo test -p shell`（`reftest`、`sites`） |
| **paint 和 hit test** | |
| flex 和 grid item 在文字階段整個畫完（背景再文字），照 `order` 改過的順序；之後的 block 的背景在 item 之下 | `cargo test -p paint --test artifact`（`flex_items_paint_whole_with_the_lines`、`flex_items_paint_in_order_modified_order`、`grid_items_paint_whole_and_by_z_index`） |
| atomic inline 整個畫在 line 上它的位置、在它之後開始的 inline 元素之前；是 stacking context 的畫在 line 之後；有它的 line 重畫時照樣在它自己的 property 下 | `cargo test -p paint`（`artifact` 的 `inline_blocks_paint_whole_where_they_come_in_their_line`、`paint_controller` 的 `an_inline_block_paints_under_its_own_properties_again`） |
| stacking context 照附錄 E 和 `z-index` 排：負的在 container 的背景之下、在 stacking context 根自己的背景之上；`0` 和 `auto` 照樹的順序；正的由小到大；item 裡的 stacking context 歸到父 stacking context | `cargo test -p paint --test artifact`（`stacking_contexts_paint_by_z_index`、`a_stacking_context_in_a_flex_item_is_its_parents`、`a_negative_z_index_is_over_its_stacking_contexts_own_background`） |
| hit test 照同樣的順序；主執行緒捲動區域不把 item 和 atomic inline 之前的東西當成蓋住它 | `cargo test -p paint --test hit_test`（`flex_items_and_z_index_are_hit_as_painted`、`a_negative_z_index_is_hit_over_its_stacking_contexts_own_box`、`a_flex_item_container_is_over_the_items_before_it`、`inline_blocks_are_hit_as_painted`、`text_after_an_inline_block_is_hit_over_its_text`、`an_inline_block_container_is_over_the_inline_box_around_it`） |
| **畫面** | |
| 自己的 reftest：row 和對齊、`order`、wrap 和 column、baseline、整個畫完、`z-index`、照 danluu.com 結構縮減的日期清單；intrinsic size；grid 的 track 和擺放、常見的 areas 版面；導覽列和 baseline、inline-flex 的按鈕和 inline-grid | `python3 scripts/test.py reftest`（`flex/`、`block/b7`、`grid/`、`inline/i7`、`inline/i8`） |
| WPT 的 339 個 reftest（`css-flexbox`、`css-grid`、`css-sizing` 全部的候選，和其他目錄因為新的值而成為候選的），過不了的照 `4ff8d6e:tests/wpt/reftest-expectations.txt` | `python3 scripts/test.py reftest wpt` |
| `about:demo`：flex 列的「Move A」、疊在一起的卡片和「Raise 1」、fit-content 標籤的「Add a word」、grid tiles 的「Widen tile 1」、areas 版面的「Swap sides」、導覽列的「Rename」、inline-flex 標籤的「More」 | `python3 scripts/test.py scenarios`（`flex`、`flex-z-index`、`fit-content`、`grid`、`grid-areas`、`inline-block`、`inline-flex`）；真的視窗：`python3 scripts/test.py automation` |

## 做過的檢查

**新的測試**：`crates/layout/tests/flex.rs`、`grid.rs`、`inline_block.rs`；`layout` 的 `update` 加了 13 個、`replaced` 的單元測試；`style` 的 `grid` 模組的單元測試、`cascade` 和 `invalidation`；`paint` 的 `artifact`、`hit_test`、`paint_controller`；`tests/reftests/` 的 `flex/` 7 個、`grid/` 2 個、`block/b7`、`inline/i7`、`i8`；7 份情境；WPT 339 個（每一階段挑了哪些、為什麼，在 `tests/wpt/README.md`）。

**WPT 找到的 bug**（都先修了才匯入）：
- flex：只有空白的那段在 `white-space: pre` 時產生了 item（`whitespace-in-flexitem-001`）；當 flex item 的 `<img>` 量出來是 0×0，item 縮成 0、stretch 不照比例（9 個）。
- intrinsic size：`<img width=50%>` 的 IFC 被夾成 0 寬（`inline-replaced-width-006`）。
- grid：只有 `repeat()` 裡有名稱時，名稱不見了（`grid-placement-using-named-grid-lines-004`）；只在一軸拉伸的圖片，另一軸沒照比例（`replaced-alignment-with-aspect-ratio-006` 到 `-009`）。
- atomic inline：裡面有會裁切的 block 時的 baseline（`baseline-block-with-overflow-001`）、百分比高度沒有基準（`max-height-applies-to-017`）、inline-grid 的百分比 `gap` 是 0（`grid-gutters-015`）。

**自己找到的 bug**：IFC 的 measure 用 border box 的寬度斷行；block 的圖片有 padding 時高度偏大（taffy 把比例套在 border box 上）；`width: fit-content` 裡會換行的文字比可用寬度窄；grid item 的圖片被拉成整格寬；`grid-template` 的字串之後接受 `repeat()`；line 上在 atomic inline 之後開始的 inline 元素被畫在它之前（舊的 inline 圖片也是）；剛好放得下的文字因為浮點誤差多斷一行；只有文字的 `inline-block` 的 margin 扣了兩次；量寬度時把 atomic inline 完整排一次、fragment 保不住。理由和修法在 `README.md` 的「詳細設計」。

**候選腳本的過濾**：每一階段放寬時，把一批過不了、但不是因為新功能的測試排除：`gradient(`（用來畫預期結果）、`ic` 單位、根目錄相對路徑的樣式表、`<fieldset>`、`white-space: break-spaces`；排除具名的線的規則原本跨過換行誤判了 2 個。細節在 `tests/wpt/README.md`。

**過不了的 40 個**（`4ff8d6e:tests/wpt/reftest-expectations.txt`，各附原因）：taffy 的問題（`details.md` 的「taffy 的問題」）、kun 既有的限制（`overflow: clip` 算成 `hidden`、不同元素的文字之間沒有 kerning、parley 的 min-content 寬度、inline 圖片的大小、圖片的 min / max 沒有保持比例）、一個 tentative 測試和 taffy 照的新版規範不同，以及 4 個四個主要瀏覽器都沒照做的 `inline-block` baseline 規則（wpt.fyi，2026-10-07）。

**測試本身的錯**：幾次是我把預期值算錯或把規範記錯（`<auto-track-list>`、sparse 的擺放、row 後面的名稱、同一行有 padding 的 inline-block 讓 baseline 往下移……），都照規範和實際的算法改了測試，不是實作的問題。

**Mutation test**：用 `scripts` 以外的暫存腳本，一次弄壞一處、跑最相關的測試，還原後 `touch`，跑完用 md5 確認實作檔案還原。七段共 193 條（1a 51、1b 21、2 21、3a 28、3b 26、4a 28、4b 18），最後除了下面等價的都抓到。一開始沒抓到的，大多是測試的例子剛好讓兩種算法得到一樣的結果（`flex: 1` 沒有內容、`span` 名稱和具名的線落在同一條、只變寬不變高、`max-content` 的容器回報真正斷行後的寬度），補了能分出差別的例子。`covered_scrollers` 會把重複走到的 box 當成畫在前面，hit test 裡看起來等價的兩條（背景階段又走一次 item、atomic inline）因此不是等價的，各補了測試。

**等價、拿掉的程式**：baseline 讀之前再斷一次行；`set_content` 清掉排過的 child 清單；當 flex item 的圖片設 `item_is_replaced` 和 `aspect_ratio`；「至少和最長的一行一樣寬」；圖片的 fragment 一律用確定的寬度；建 fragment 前把 atomic inline 再排一次；`lay_out` 的 min-content 分支。等價、留著的兩條（`content_widths` 的快取、IFC 的 measure 在知道寬度時回傳那個寬度）記在 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。

**不穩定的測試**：`flex.txt` 按鈕之後先等文字、再等 A 的 box 變化，A 可能在開始等之前就已經移動；`flex-z-index.txt` 點擊的 hit test 用的是上次畫的結果。都改成按之前 `capture`、之後 `expect-differs`，各連跑三次都過；之後的情境照這個寫。

**跑過的檢查**（2026-10-07，Windows 11）：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（994 個通過；reftest 504 個通過、42 個照預期失敗）、`python scripts/sanity.py deps`、`boundary`、`unsafe`、`docs` 都過；`python scripts/test.py automation` 20 個情境都過，看過每一段新的情境的截圖（系統字型、1.5 倍）。
