# RFC 0003 的細節：flex 和 grid layout

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 開始時的程式

2026-10-04 的程式，2026-10-06 排程時重新對照過；這份 RFC 從這裡開始改，最後的設計見 `README.md`。

- **taffy**：crates.io 的 0.14.0，沒有 fork，workspace 的 `Cargo.toml` 只開 `std`、`block_layout`、`content_size`。`calc` 沒開，`LayoutPartialTree::resolve_calc_value` 一律回 0。
- **低階 trait**：`crates/layout/src/lib.rs` 在 `BoxTree` 上實作 `TraversePartialTree`、`LayoutPartialTree`、`CacheTree`、`LayoutBlockContainer`，style 一律回傳每個 `LayoutBox` 存的 `taffy_style`。`compute_box` 包在 `compute_cached_layout` 裡，依序分派：hidden、替換元素（leaf，尺寸已經在 style 裡）、IFC（leaf，parley 的 measure）、沒有 child（leaf）、其餘 `compute_block_layout`。
- **`taffy_style`**：把 `display` 寫死成 `Block`，只對應 `box-sizing`、size 和 min / max、margin、padding、border、`overflow`（任何會裁切的值都變成兩軸的 `Hidden`，所以建立 BFC）。替換元素由 `replaced_taffy_style` 給固定的尺寸和 `aspect_ratio`，沒有設 `item_is_replaced`。
- **box tree**：`build_block` 只接受 `Display::Block`；`collect` 對 `Display` 的三個值做窮舉的 `match`，加新的值時編譯器會指出要改的地方。`set_content` 在內容全是 inline 時把 box 自己變成 IFC（沒有 child），混合時把每段 inline 包成匿名 block，只有可合併空白的那段跳過。
- **intrinsic size**：IFC 的 measure 已經回答 `MinContent`（`content_widths` 的最小值）和 `MaxContent`（不限寬度）；`content_widths` 是 parley 的 `calculate_content_widths`，沒有快取，`break_lines` 有。atomic inline 只有 `<img>`（`InlineItem::Image`）。
- **style**：屬性的白名單在 `crates/style/src/properties.rs` 的 `LonghandId`。`display` 只接受 `block`、`inline`、`none`；size 只接受 `auto` 或長度、百分比。沒有 `position`、`z-index`。blockification 只對 root 元素做。
- **paint**：照 CSS 2 附錄 E，stacking context 裡先整棵子樹畫背景、再畫文字，子 stacking context 最後。`is_stacking_context` 只看 `opacity < 1` 和 `transform`。hit test 是同一個順序的反向（`crates/paint/src/hit_test.rs`）。
- **P4 的局部重排**：`mark_needs_layout` 清掉自己和祖先的 taffy cache，兄弟不清，由父 box 的演算法重新呼叫、輸入相同時命中 cache；taffy 的 cache key 是 `LayoutInput`，兄弟的影響（例如 flex 的剩餘空間）會反映在輸入上，所以照樣成立。`StyleDifference` 把 `display` 的變化當成 Reattach，只影響畫面的屬性當成 Paint，其餘是 Layout。
- **WPT**：`4ff8d6e:tests/wpt/reftest-include.txt` 沒有 `css-flexbox` 和 `css-grid`。`scripts/wpt_reftest_candidates.py` 的 `BAD_VALUES` 排除了 `inline-block|flex|grid|justify|min-content|max-content|fit-content`。`full-width-leading-spaces-004` 因為用了 `width: min-content` 沒有匯入（`tests/wpt/README.md`）。

## 加的屬性

taffy 0.14 的 `Style` 都有對應的欄位，kun 做的是 parse、cascade、存進 `ComputedStyle`、在 `taffy_style` 對應，並在 `StyleDifference` 分類。加一個屬性的步驟：`ComputedStyle` 的欄位和初始值、`LonghandId`（`from_name`、`is_inherited`、`parse_value`、`Value`、`apply`、`copy`）、shorthand、`taffy_style`、`StyleDifference::between`。

| 群組 | 屬性 | taffy 0.14 | 備註 |
| --- | --- | --- | --- |
| display | `flex`、`grid`、`inline-block`、`inline-flex`、`inline-grid` | `Display::Flex`、`Display::Grid`、`Display::Block` | inline-level 的是 atomic inline，taffy 照 block-level 的排 |
| flex | `flex-direction`、`flex-wrap`、`flex-flow`、`flex-grow`、`flex-shrink`、`flex-basis`、`flex` | 有 | `flex` 的 shorthand 有好幾個特別的值（`auto`、`none`、單一數字） |
| 順序 | `order` | **沒有** | 排版時排序（`README.md` 的 O2） |
| 對齊 | `justify-content`、`align-content`、`align-items`、`align-self`、`justify-items`、`justify-self`、`place-*` | 有 | `baseline` 要 item 回報 baseline |
| 間距 | `gap`、`row-gap`、`column-gap` | 有 | |
| grid | `grid-template-columns`、`grid-template-rows`、`grid-template-areas`、`grid-template`、`grid-auto-flow`、`grid-auto-columns`、`grid-auto-rows`、`grid-row`、`grid-column`、`grid-row-start` / `-end`、`grid-column-start` / `-end`、`grid-area` | 有（含 named lines 和 areas，`CustomIdent = String`） | `repeat(auto-fill / auto-fit, …)` taffy 有；`grid` shorthand 和 subgrid 沒做 |
| size | `width`、`height` 的 `min-content`、`max-content`、`fit-content`；`flex-basis: content` | `Dimension::min_content()` 等 | `min-*` / `max-*` 上的放不進 taffy 的 `LengthPercentageAuto`（`README.md` 第 12 條）；`fit-content(<length>)` 沒做 |
| stacking | `z-index` | 無（paint 的事） | 只對 flex / grid item 生效，直到有 `position` |

## 實作時注意到的地方

- **flex container 裡只有文字**：原本會變成 IFC 的 leaf，改成一個匿名的 item 包住那段文字；item 自己才是 IFC。
- **blockification 和局部更新**：父元素換 `display` 時，child 的 computed `display` 跟著變。P4 的 style 以子樹為單位重算，換 `display` 的元素整棵子樹都會重算，所以涵蓋得到；`invalidation` 測試守住。
- **`order` 和局部更新**：`order` 是 Layout 等級的變化：item 重新排版時 `mark_needs_layout` 清掉祖先的 taffy cache，container 重新排版時重新排出順序，box tree 不用動（「已決定的事項」第 3 條）。
- **只被量過的 box**：flex 決定 item 的 flex base size、grid 決定 track 的大小時，會對 item 單獨跑 `ComputeSize`。`compute_box` 只在 `PerformLayout` 時標 `needs_fragment`；flex 和 grid 最後一定會對每個 item 跑 `PerformLayout`，只被量過、排版命中快取的 box 沿用 fragment（`update` 測試的 `a_box_only_measured_keeps_its_fragment`）。
- **`content_widths` 的快取**：flex 的每個 item 至少量一次 min-content 和 max-content，巢狀時次數成倍增加；記住上次的結果，atomic inline 變了才清掉。
- **baseline**：taffy 的 flex 從 `LayoutOutput` 的 `baselines.first` 拿 item 的 baseline，沒有的話用 item 的下緣；IFC 的 leaf 從 parley 第一行的 baseline 加上 padding 和 border 算出來。block container 的 baseline 由 taffy 從第一個 child 傳上來。
- **paint 的整個畫完**：P4.3d 的 display item 快取以 box 為單位、分背景和文字兩段記（`PaintedBox` 的 `backgrounds` 和 `text`）。2026-10-04 擔心整個畫完的 item 只有一段、快取的 key 要能區分；2026-10-06 對照程式後，item 整個畫完時，item 和它子孫的每個 box 還是各自畫背景、再畫文字，只是畫的時機移到父 stacking context 的文字階段（照 inline-block 的規則），每個 box 照樣有兩段，key 不用改。atomic inline 的也一樣，只是它所在的 IFC 的 line 每次重畫（`README.md` 的「paint 和 hit test」）。
- **`what_is_ignored`**：`crates/style/tests/cascade.rs` 原本檢查 `display: flex` 被丟掉，改成檢查還不支援的值（例如 `display: table`）。

## taffy 的問題

匯入 WPT 的 reftest 時找到的（2026-10-06 到 2026-10-07，taffy 0.14.0），每一項由 `4ff8d6e:tests/wpt/reftest-expectations.txt` 裡過不了的測試標出來（沒有的除外）：

| 問題 | 規範 | WPT | 原因（taffy 的原始碼） |
| --- | --- | --- | --- |
| 百分比的 `flex-basis` 碰到不確定的 main size，用 item 的 `height` | 當成 `content`（css-flexbox 1 的「flex-basis」）；item 的大小也就不確定，裡面的百分比高度當成 `auto` | `flex-basis-010`、`percentage-heights-016`、`-017`、`-018` | `compute/flexbox.rs` 的 `flex_basis.or(main_size)` |
| flex base size 被 `max-width` 限制 | flex base size 不限制，hypothetical main size 才限制（9.2 第 3 步） | `flex-base-size-ignores-max-width` | 待查 |
| 量 item 的 min-content 時，item 確定的 `height` 沒有傳給子孫 | `<img style="height: 100%">` 對 item 的高度解析 | `flex-minimum-width-flex-items-016` | 待查 |
| 換行的 column flex container 當 item 時，自動最小高度是它的 `height` | min(specified size suggestion, content size suggestion) | `flex-minimum-height-flex-items-029` | 待查 |
| `aspect_ratio` 套在 border box 上 | 替換元素的自然比例是 content box 的 | 沒有；kun 只在 block 的圖片寬度是百分比時用 `aspect_ratio`，其他的避開了（`README.md` 的「詳細設計」） | `compute/leaf.rs` 的 `clamped_size.width / ratio` |
| measure 拿到的已知大小是 border box 的，可用空間是 content box 的 | — | 沒有；kun 的 measure 自己扣掉（`README.md` 的「詳細設計」） | `compute/leaf.rs` 的 `measure_function` 呼叫 |
| grid container 的第一條 baseline：沒有 item 用 baseline 對齊時，用第一列第一個 item 的下緣 | 第一列 item 自己的第一條 baseline（css-grid 2 的「Grid Container Baselines」） | `grid-self-alignment-baseline-gfc-001` | `compute/grid/mod.rs` 的 `item.baseline.unwrap_or(item.height)`：`item.baseline` 只在有 item 用 baseline 對齊時才算 |
| `width: min-content` 的 grid，item 跨過 `1fr` 和固定的 track 時太窄 | 跨過 flexible track 的 item 照「Expand Flexible Tracks」分配 | `grid-flex-spanning-items-001` | 待查 |
| `1fr` 那一列的 item 的 `max-height: 100%` 沒有作用 | 列的大小決定之後，item 的百分比對它解析 | `grid-item-overflow-auto-max-height-percentage` | 待查 |
| flex container 在確定的空間裡照內容算寬度時，沒算進 item 的負 margin（`inline-flex`、`width: fit-content`） | item 的 margin box 的寬度，負的也算 | `negative-margins-001` | 待查 |
| wrap-reverse 的 column 裡，`align-items: baseline` 的 item 排在 line 的右緣 | 照 baseline 對齊在 line 的左邊（Chrome 也是，那邊才是 line 的開頭） | `align-baseline` | 待查 |
| `space-between` 溢出時退到 `start` | 這個 tentative 測試要 `flex-start`；css-align 3 現在寫的是 `safe flex-start`，taffy 照這個 | `justify-content_space-between-003.tentative` | `compute/common/alignment.rs` 的 `apply_alignment_fallback` |

過不了的其他測試不是 taffy 的問題：kun 既有的限制（`docs/limitations.md`，例如 `overflow: clip` 算成 `hidden`、不同元素的文字之間沒有 kerning、圖片的 min / max 沒有保持比例），或四個主要瀏覽器都沒照做的規則（`inline-block-baseline-003` 到 `-006`）；原因寫在 expectation 的註解裡。

## 其他實作

| | Chromium（LayoutNG） | Servo | Blitz | kun |
| --- | --- | --- | --- | --- |
| flex 演算法 | `FlexLayoutAlgorithm` | 自己寫的 | taffy | taffy |
| grid 演算法 | `GridLayoutAlgorithm` | taffy | taffy | taffy |
| 接 taffy 的方式 | — | 低階 trait | 低階 trait | 低階 trait（和 block 相同） |
| blockification | `StyleAdjuster` | style | style（Stylo） | style（B1） |
| `order` | `FlexChildIterator`，每次排版時穩定排序；fragment 照排版的順序 | — | — | 照 Chromium（O2） |
| item 的 paint | `BoxFragmentPainter::PaintAllPhasesAtomically` | — | — | 整個畫完（1b） |
| atomic inline | `BlockNode::LayoutAtomicInline`，`FragmentItems` 的 box item | — | — | box tree 的 box，IFC 斷行時排（4a） |

Chromium 的 `order` 2026-10-06 對照過原始碼（`third_party/blink/renderer/core/layout/flex/flex_child_iterator.cc`：只有某個 child 的 `order` 不是初始值時才排序，用 `std::stable_sort`）。Servo 和 Blitz 的欄位是依 2026-10-04 時的了解寫的，沒有逐一查證原始碼；「—」表示沒查。

## 參考資料

- CSS Flexible Box Layout Module Level 1：https://www.w3.org/TR/css-flexbox-1/
- CSS Grid Layout Module Level 2：https://www.w3.org/TR/css-grid-2/
- CSS Box Sizing Module Level 3（`min-content`、`max-content`、`fit-content`）：https://www.w3.org/TR/css-sizing-3/
- CSS Display Module Level 3（blockification）：https://www.w3.org/TR/css-display-3/
- taffy 0.14 的低階 API：`LayoutFlexboxContainer`、`LayoutGridContainer`（`src/tree/traits.rs`）
