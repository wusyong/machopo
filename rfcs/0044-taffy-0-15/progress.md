# RFC 0044 的進度：升級到 taffy 0.15

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-13。完成：2026-10-13，Windows 11（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0044` 上做兩段，做完時一起 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 升級和絕對定位改用 taffy 的 hoisting | 完成 | 2026-10-13 |
| 2 行內的絕對定位盒子 | 完成 | 2026-10-13 |

## 驗收

### 升級和絕對定位（第 1 段）

| 項目 | 怎麼驗證 |
| --- | --- |
| 絕對定位由 taffy 的 hoisting 排：包含塊照它的 padding box、static position、百分比和兩邊的 inset、`position: fixed` 對 viewport；fragment 只掛在包含塊底下一次；inset 改變時包含塊重排、旁邊的 in-flow 盒子留著 fragment | `cargo test -p layout --test it position::`、`update::an_absolute_box_lays_its_containing_block_out_again` |
| `justify-self` / `align-self` 對齊絕對定位的盒子；flex container 的絕對定位子盒子的 static position 照對齊；grid container 的照它的 grid 區域 | `cargo test -p layout --test it position::absolute_boxes_are_aligned_and_take_grid_areas` |
| inline-block 的 baseline 不看 out-of-flow 的 child（就算它已經排好） | `cargo test -p layout --test it inline_block::on_the_baseline` |
| block 上下的百分比 padding 對著寬度 | `cargo test -p layout --test it block::vertical_padding_percentages_are_of_the_width` |
| grid：只有 ident 的位置先找區域的邊；item 的 `normal` 不是 `auto`；有自然大小的 replaced item 在 `normal` 下不拉伸；`fit-content()` 的 `calc()` 的百分比 | `cargo test -p layout --test it grid::` |
| 畫面和以前一樣（或更接近 Chrome） | `python3 scripts/test.py reftest position/ flex/ grid/ block/ inline/`；`python3 scripts/test.py wpt css/<目錄>/`（逐個目錄跑：一次跑整個 `css/` 在 Windows 上命令列太長） |

### 行內的絕對定位盒子（第 2 段）

| 項目 | 怎麼驗證 |
| --- | --- |
| `original_display`：blockify 之前的 `display` | `cargo test -p style --test it cascade::out_of_flow_boxes_keep_their_original_display` |
| 行內的 out-of-flow 盒子不切開行、空白照它不在處理；static position：inline-level 在它在行裡的位置、行的頂端，block-level 在行的起點、前面同一行有內容（文字或 atomic inline）時在下一行；只有 out-of-flow 盒子的容器 | `cargo test -p layout --test it position::out_of_flow_boxes_in_inline_content` |
| 有 block 子盒子時，跟著前面那段行內內容；前面是 block 或只有空白時是 block 子盒子（static-position rectangle 是 content box 的寬） | `cargo test -p layout --test it position::out_of_flow_boxes_go_with_the_inline_content_before_them` |
| 照 `justify-self`（`auto` 用容器的 `justify-items`）和 `align-self` 在 static-position rectangle 裡對齊；inline-level 的 rectangle 是一個點和行的高度 | `cargo test -p layout --test it position::out_of_flow_boxes_in_inline_content_are_aligned` |
| inline formatting context 自己是包含塊；atomic inline 傳上來的經過它的行往上傳（連同它的 `position: relative`） | `cargo test -p layout --test it position::inline_content_lays_out_its_own_out_of_flow_boxes`、`position::absolute_boxes_in_an_atomic_inline_go_up_through_the_line`、`position::a_moved_atomic_inline_passes_up_from_where_it_is` |
| 行內的 out-of-flow 盒子改了 inset，包含塊重排 | `cargo test -p layout --test it update::an_absolute_box_in_inline_content_lays_its_containing_block_out_again` |

## 做過的檢查

- **做了什麼**：
  - 第 1 段（第 1 到 4 條）：`Cargo.toml` 的 taffy 換成 0.15（feature 不變）。`layout` 的 `taffy_style`：`Position` 直接對到 0.15 的四種，inset 一律給；四個對齊的 `normal` 是 `NORMAL`，`justify-self` / `align-self` 的 `normal` 是 `Some(NORMAL)`；`fit_content_calc`；grid 裡有自然大小的 replaced item 設 `item_is_replaced`；`grid_line` 的單獨 ident 是 `NamedLine(name, 0)`。絕對定位：拿掉佔位盒子、兩趟排版和 hoisting 的走訪，`BoxTree` 實作 `LayoutContainingBlock` 和 `set_detailed_grid_info`，`compute_box` 在 cache 裡、`PerformLayout` 時呼叫 `compute_oof_layout`，`fragment()` 和 `last_baseline` 跳過自己的 out-of-flow child。`AtomicInlines::lay_out` 一律扣掉 margin。
  - 第 2 段（第 5 條）：`computed_style` 的 `original_display`。`layout`：`InlineItem::OutOfFlow`（`set_content` 照第 5 條分配，`adopt_atomics` 收成 child）、parley 的 `OutOfFlow` box（空白照它不在處理，只有它也建 inline content）、`static_positions` 算 static-position rectangle、`compute_inline` 設 `oof_positioning_area` 並由 `passed_up` 照樹的順序組候選（自己的照 self-alignment 對齊，atomic inline 存在 `LayoutBox::oof_candidates` 的平移到它的位置）。
  - 文件：`docs/architecture.md` 的 `layout` 一列；`docs/limitations.md`（拿掉：行內的絕對定位盒子的 static position 和切開行、inline-block 落到行下面、絕對定位的 `justify-self` / `align-self`、百分比的 `flex-basis`、`1fr` 列的 `max-height: 100%`、block 上下的百分比 padding、`fit-content()` 的 `calc()`、`position: fixed` 的根元素；改寫：grid 的絕對定位子盒子（排版照區域，`getComputedStyle` 的 inset 還照 padding box）；加上：高度不確定的 grid 裡加起來小於 1 的 `fr` 列不重排、`position: relative` 的 inline 元素不是包含塊（用一個暫時的測試確認 kun 把它對著外面的 positioned 盒子排，Chrome 對著那個 span））；`docs/test-gaps.md` 的「等價、不用處理的 mutation」。
- **找到並修的**：
  - `grid::items_placed_in_areas` 失敗：0.15 的 `NamedLine(name, 1)` 是「第一條叫這個名字的線」，`grid-area: head` 找不到 `head-start`，改成 0。
  - 第 1 段的回歸（第 2 段開始時發現）：inline-block 裡、包含塊在 inline-block 外面的絕對定位盒子沒有被排（`AtomicInlines::lay_out` 丟掉了它 `LayoutOutput` 裡的候選；0.14 時 kun 自己的 hoisting 處理得到，WPT 沒有測到）。候選改成經過 atomic inline 所在的行往上傳。
  - 第 2 段的第一版 static position 是一個點：WPT 的 `css-align/abspos/align-self-static-position-001` 回歸（只有一個絕對定位的 child 的 block 變成 inline formatting context），改成照 css-position 3 的 static-position rectangle 對齊；改了之後 `align-items-static-position-001.tentative` 失敗：`compute_inline` 排版期間把 taffy style 拿出 box，讀到預設的 `justify-items`，改成先放回去。
  - 測試的預期值算錯兩次（`ZZ` 的 x、atomic inline 之後的行高要加上 strut 的 descent），照實際的排版改正。
- **測試**：`layout` 的 `position::absolute_boxes_are_aligned_and_take_grid_areas`、`out_of_flow_boxes_in_inline_content`、`out_of_flow_boxes_go_with_the_inline_content_before_them`、`out_of_flow_boxes_in_inline_content_are_aligned`、`inline_content_lays_out_its_own_out_of_flow_boxes`、`absolute_boxes_in_an_atomic_inline_go_up_through_the_line`、`a_moved_atomic_inline_passes_up_from_where_it_is`，`block::vertical_padding_percentages_are_of_the_width`，`grid::normal_self_alignment_is_not_auto`、`fit_content_of_a_calc`，`inline_block::on_the_baseline` 加一個案例，`update::an_absolute_box_in_inline_content_lays_its_containing_block_out_again`；`update::an_absolute_box_lays_its_containing_block_out_again` 的 `built` 從 4 變 2（沒有佔位盒子）、`shaped` 從 0 變 1（只有一個絕對定位的 child 的 `#deep` 是 inline formatting context）。`style` 的 `cascade::out_of_flow_boxes_keep_their_original_display`。
- **mutation test**：
  - `mutants-1.toml`（13 條）：第一次 1 條編不過（mutant 自己的錯）；「量大小時也呼叫 `compute_oof_layout`」等價（量大小時 taffy 不設 `oof_positioning_area`，`compute_oof_layout` 直接 return），從清單拿掉、記在 `docs/testing.md`。重跑 12 條抓到，「out-of-flow 的 child 給 inline-block baseline」沒抓到：測試的包含塊是 LayoutView，排 inline-block 時那個 child 還沒排、沒有行；補了讓 inline-block 自己當包含塊的案例之後抓到。
  - `mutants-2.toml`（18 條）：第一次 15 條抓到；「只有空白之後的 out-of-flow 盒子也進行內內容」和「只有 out-of-flow 盒子時不建 inline content」在現有的案例裡排在同一個位置（差別在 static-position rectangle：補了 `justify-self: end` 的案例），「out-of-flow 盒子不是 inline content 的盒子的 child」只影響局部重排（補了 `update::` 的測試，那條 mutant 的測試指令加上 `update::`）；補了之後那 3 條都抓到。
- **WPT**（`test.py wpt` 逐個目錄跑 `include.txt` 裡 `css/` 底下的 38 個目錄，約 2,200 個測試；兩段各跑一次，第 2 段改了 static-position rectangle 之後再整批跑一次）：預期結果用 `MACHOPO_UPDATE_EXPECTATIONS` 更新。
  - 變通過的 20 個：`css-flexbox` 的 `flex-basis-010`、`percentage-heights-016` 到 `018`（百分比的 `flex-basis`）；`css-backgrounds/background-size-percentage-root`（`position: fixed` 的根元素，0.15 的 `compute_root_layout` 照根的 `position` 排）、`border-{left,right}-width-{medium,thick,thin}`（在幾個 inline-block 之間、`top: auto` 的絕對定位盒子在它的行上）；`css-grid` 的 `abspos/positioned-grid-descendant-containing-block-009`、`010`、`013`（grid 區域）、`alignment/grid-content-alignment-with-abspos-001`（`justify-self`）、`grid-items/grid-item-overflow-auto-max-height-percentage`、`grid-item-minmax-img-002`；`css-align/abspos/align-self-static-position-001`；`CSS2/linebox/vertical-align-baseline-004a`、`005a`（也有絕對定位的盒子；RFC 0043 第 2a 段記下、沒查出原因的失敗）。
  - 變失敗的 2 個（第 4 條：0.15 的問題，記成限制）：`css-grid/layout-algorithm/flex-sizing-rows-indefinite-height`、`grid-template-flexible-rerun-track-sizing`。`minmax(0, .5fr)` 的列、200px 的 item，grid 的高度不確定：taffy 0.14 和 0.15 單獨排都是列 100px（scratchpad 裡的 `TaffyTree` 確認），Chrome 知道 grid 的高度之後重排一次是 50px；絕對定位的盒子照它的 grid 區域（50×100）排，`200%` 的高度露出紅色。0.14 時它對著包含塊的 padding box 排，碰巧通過。
  - `css/cssom/getComputedStyle-insets-grid.html` 照舊失敗：排版照 grid 區域了，`getComputedStyle` 的 `auto` inset 還照 padding box 算。
- **跑過的**（兩段做完）：`cargo test --workspace --no-fail-fast` 43 個 binary、2611 個通過（第 1 段做完時 2603 個）；`cargo fmt --check`（連同 `sdk/lua/generator`）；`cargo clippy --workspace --all-targets -- -D warnings`；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py reftest position/ flex/ grid/ block/ inline/ text/`（52 個，0 個預期外）；`test.py scenarios`（50 次，0 個失敗）；上面的 WPT。
- **沒跑的**：Speedometer（只在使用者要求時量）；`cargo clean` 在 merge 回 `main` 之後跑。
