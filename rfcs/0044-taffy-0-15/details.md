# RFC 0044 的細節：升級到 taffy 0.15

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## float 的位置

2026-10-13，Windows 11，scratchpad 裡的一個小 crate 直接用 `TaffyTree`（feature 加上 `float_layout`、`taffy_tree`），根是 300px 寬的 block：

| 盒子 | style |
| --- | --- |
| A | `height: 50px; margin-bottom: 20px` |
| B | `height: 10px; margin-top: <0 或 10px>`，裡面一個 `float: left` 的 F（100×100） |
| C | `height: 92px`（墊高，讓 D 從 y 172 開始） |
| D | `height: 10px; overflow: hidden`（BFC，不能和 float 重疊） |

B 的上 margin 和 A 的下 margin 摺疊，B 在 y 70，F 畫在 70 到 170；D 在 172，應該不受 F 影響（x 0，Chrome 也是）。

| 版本 | B 的 `margin-top` | B | F 在 BFC 裡記的範圍 | D |
| --- | --- | --- | --- | --- |
| 0.14.0 | 0 | (0, 70) | 70 到 170 | (0, 172) |
| 0.14.0 | 10 | (0, 70) | **80 到 180** | **(100, 172)** |
| 0.15.0 | 0 | (0, 70) | 70 到 170 | (0, 172) |
| 0.15.0 | 10 | (0, 70) | 70 到 170 | (0, 172) |

原因（0.14 的 `compute/block.rs`，`perform_final_layout_on_in_flow_children`）：同一個 BFC 的子 block 的 `BlockContext` 是 `block_ctx.sub_context((y_offset_for_absolute + item_non_auto_margin.top).max(clear_pos), insets)`，`y_offset_for_absolute` 已經加上前一個的下 margin（`committed_y_offset + active_collapsible_margin_set.resolve()`），再加自己的上 margin，等於沒有摺疊。子 block 實際的位置用摺疊後的 margin，所以子 block 裡放的 float 和行內內容向 BFC 要空間時，y 都多了 `min(前一個的下 margin, 自己的上 margin)`（第一個 child 和 parent 的上 margin 摺疊時是自己的上 margin）。同一個 block 裡的 float 和文字誤差一樣、互相抵消；不同 block 之間（float 在前一個 block、文字在後一個 `<p>`）就錯。0.15 改成 `committed_y_offset + active_collapsible_margin_set.collapse_with_margin(item_non_auto_margin.top).resolve()`（child 自己往下摺疊的 margin 還不知道，所以是估計；Blink 用推測的 BFC offset 排，猜錯了重排一次）。

## API 的差異

2026-10-13 把 `Cargo.toml` 換成 0.15、`cargo check -p layout`（之後還原）：5 個錯誤。

- `compute_root_layout` 要 `LayoutContainingBlock + CacheTree`：`BoxTree` 沒有實作 `LayoutContainingBlock`。
- `justify_items`、`justify_content`、`align_content`、`align_items` 不再是 `Option`（4 個錯誤）：`normal` 是 `AlignItems::NORMAL`、`AlignContent::NORMAL`。

編譯錯誤以外、行為改變的（0.15 的 changelog，`https://github.com/DioxusLabs/taffy/blob/main/CHANGELOG.md`）：

| 改變 | kun 要做的 |
| --- | --- |
| `Position` 多 `Static`（預設）、`Fixed`、`Sticky`；static 忽略 inset | `taffy_style` 直接對應；拿掉 static 給 `auto` inset 的轉換 |
| 絕對定位的盒子 hoist 到 containing block；自訂的 tree 要實作新的 trait、在 `compute_cached_layout` 裡呼叫 `compute_oof_layout` | `README.md` 的「升級和 taffy 的 hoisting」 |
| 絕對定位的 `Layout::location` 相對於 containing block 的 border box，算進它的 scrollable overflow | kun 的 fragment tree 本來就把它掛在 containing block 底下 |
| 確定的 `available_space` 是給 border box 的：parent 扣掉 child 的 margin，leaf 和 flex 不再扣自己的 | `AtomicInlines::lay_out` 的 margin 一律由 kun 扣 |
| `LayoutOutput` 不再是 `Copy`，多 `oof_candidates`、`oof_positioning_area` | 用到 `LayoutOutput` 的地方改成 clone 或 move |
| `RoundTree` 多三個方法 | kun 不用 `round_layout`，不影響 |
| `CoreStyle::is_compressible_replaced()` 包含表單控制項；新的 `is_replaced()`、`Style::item_is_compressible_replaced`；grid 在 `normal` 的對齊下不拉伸 replaced item | kun 現在不設 `item_is_replaced`，自己把 grid 裡有自然大小的 replaced item 的 `justify-self` / `align-self` 改成 `start`（`replaced_taffy_style`）；0.15 設了旗標就不用這個繞法，第 1 段確認後拿掉 |
| block 的 in-flow child 的 `Layout::margin` 是它自己的 margin，不是摺疊後的 | `fragment()` 的 `margin` 照 CSSOM 本來就該是自己的；`last_baseline` 用到下 margin 的地方確認 |
| `fit-content()` 收 `calc()`（`Dimension::fit_content_calc`） | `track_size`、`sized` 有百分比的 `calc()` 時用它 |
| `compute_root_layout` 用 viewport 大小的 initial containing block，照根元素的 `position`、inset、margin | kun 的根是 `LayoutView`，大小是 viewport，照舊 |

0.15 的 `Cargo.toml`：`rust-version = "1.71"`。

## 0.15 的修正和 kun 的限制

`docs/limitations.md` 裡寫明是 taffy 造成的、0.15 的 changelog 有對應的修正的（是否真的解掉由 WPT 的預期結果確認）：

| kun 的限制 | 0.15 的修正 |
| --- | --- |
| block 的上下百分比 padding 和 border 對著高度 | 「Vertical percentage padding and border resolve against the container's width」（block、flex 都有） |
| 百分比的 `flex-basis` 在不確定的大小時用 `height` | 「Percentage flex bases are treated as `content` when the main size is indefinite」 |
| flex base size 被 `max-width` 限制、量 min-content 時 item 的 `height` 沒傳下去、換行的 column container 的自動最小高度 | 「Column item max-content contributions are clamped by flex base size」、「Intrinsic main size is measured at min/max-clamped cross size」、「Auto-height column containers don't wrap against ancestor available space」（相關，不一定是同一件事） |
| grid：跨過 `fr` 和固定 track 的 item 讓 min-content 的 grid 太窄、`1fr` 那一列的 item 的 `max-height: 100%` | 「Only spanned tracks determine automatic minimum size」、「Percentage tracks and gaps resolve against the container size when it was initially indefinite」等（相關） |
| 絕對定位的盒子不套用 `justify-self` / `align-self` | 「Absolutely positioned boxes support `align-self` and `justify-self`」 |
| grid container 的絕對定位子盒子不照 grid 的區域 | hoisting 加 `get_detailed_layout_info`（grid 的區域） |
| `fit-content()` 的參數是有百分比的 `calc()` | 「`fit-content()` accepts a `calc()` limit」 |

沒解掉的：`min-*` / `max-*` 的關鍵字（`min_size`、`max_size` 照舊是 `LengthPercentageAuto`）；`position: fixed` 的 containing block 照 `transform`（`is_containing_block` 有了，但 `taffy::Style` 的實作只看 `position`）。

## 絕對定位

### 現在的做法（0.14）

`crates/layout/src/lib.rs`：out-of-flow 的元素是它父盒子的 child，原本的位置另外放一個 0×0、`Absolute` 的佔位盒子；每次 layout 前 `hoist_out_of_flow` 從 `LayoutView` 往下走，把每個 out-of-flow 的盒子掛到最近的 positioned 祖先（`absolute`）；containing block 排版時先藏起它們排一趟 in-flow 的內容（`in_flow_pass`），從佔位盒子的位置算出 static position（`resolve_static_positions`，寫成 inset），再把它們接到 child 清單後面（`with_absolute`）排第二趟，讓 taffy 0.14 的 `perform_absolute_layout_on_absolute_children` 排它們。限制：flex、grid 的 static position 不照對齊（taffy 對自己的 child 才會），行內內容裡的佔位盒子不參與行的排版。

### 比較

| 做法 | 好處 | 代價 |
| --- | --- | --- |
| **taffy 0.15 的 hoisting（選這個，`README.md` 第 2 條）** | 就是 Blink 的模型；static position 由排它的演算法算（block 的假想位置、flex 和 grid 的對齊、grid 區域）；拿掉佔位盒子、兩趟排版和 hoisting 的走訪，box tree 變簡單；cache 命中時候選照樣往上傳 | kun 要照 taffy 的規則接（在 cached layout 裡呼叫 `compute_oof_layout`、記 hoisted 的清單）；行內的那一側要自己做（第 2 段） |
| 保留 kun 的佔位盒子 | 改動小，行為和現在一樣 | 0.15 的 block、flex、grid 不再自己排絕對定位的 child，kun 要自己組 `OofCandidate`、呼叫 `compute_oof_layout_for_area`；兩套模型重疊；flex、grid 的 static position 照舊不對 |

## 行內的絕對定位盒子

現在 `collect` 把 out-of-flow 的元素當成 block 子盒子，夾在行內內容裡時把它切成兩個匿名 block（行被切開），佔位盒子在兩個匿名 block 之間；它的 static position 是父盒子的邊框盒原點（佔位盒子的位置沒有接到行）。WPT 的 `css/css-backgrounds/border-{left,right}-width-*` 六個（`display: inline-block` 的絕對定位盒子在幾個 inline-block 之間，`top: auto` 落到那一行下面）因此失敗。

parley 的 `InlineBoxKind`：`InFlow`（佔空間）、`OutOfFlow`（「當成 0×0 的 inline box 給位置」，不佔空間、不是斷行的機會，`break_next` 直接放進行裡）、`CustomOutOfFlow`（交回控制權，給 float）。第 2 段用 `OutOfFlow`。
