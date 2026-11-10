# Don't measure the items' heights for the width of a single-line column

fork 的 commit：`Don't measure the items' heights for the width of a single-line column`（`src/compute/flexbox.rs`、`tests/hand_written/column_width.rs`、`tests/hand_written/caching.rs`）。

## 問題

flex container 只被問寬度時（`RunMode::ComputeSize`、`RequestedAxis::Horizontal`，例如它是 row 的子項、row 在算 flex base size），row 的在算完 main size 之後就回傳，column 的寬度是 cross size，要跑到第 15 步。途中的 `determine_flex_base_size` 量每個子項的高度（在一個之後就丟掉的寬度下），對文字來說是每一段多斷一次行。

常見的結構：`display: flex` 的 body 裡有一個 column 的 app 外框，裡面是很多段文字。

## 做法

在 `determine_available_space` 之後，條件都成立時直接算 cross size 回傳：

- `ComputeSize`、只要寬度、column、不換行；
- 沒有子項帶 `aspect-ratio`（比例會把 flex 之後的高度換成寬度）。

用的是原有的函式：`collect_flex_lines`、`determine_hypothetical_cross_size`、`calculate_cross_size`、`determine_container_cross_size`。`determine_hypothetical_cross_size` 多一個參數，表示 main size 還沒解出來：這時子項的 known main size 是它自己的 style 的高度（夾在 min、max 裡、不小於 padding 加 border），沒有就不給；可用的 main 空間在容器高度已知時是它減掉子項的 margin，否則是 max-content。

規格的依據：單行容器的 intrinsic cross size 是子項貢獻的最大值（<https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes>）。Blink 的 `FlexLayoutAlgorithm::ComputeMinMaxSizes` 對 column 也是直接取子項的貢獻。

## 測試

- 全部的 xml fixture（6689 個）預期不變。
- `caching.rs` 的 `measure_count_flex_in_grid`：column 的 measure 次數從 10 變 6。
- `column_width.rs`：最寬的子項加 margin 和容器的 padding；有比例的子項走完整的演算法；換行的 column 走完整的演算法；子項用 style 的高度量（含 max-height、padding 的下限）；沒有高度的子項的可用高度。

## reviewer 可能問的

- **結果會不會變？** fixture 都沒變，但不是嚴格等價：完整的演算法量子項的寬度時給的是 flex 之後的高度。寬度會跟著給的高度變、又不是經由 `aspect-ratio` style 的子項（measure function 自己用 known height 算寬度的 leaf、換行的 column 子項），結果會不同。`column_width.rs` 的 `item_available_height` 的第一個斷言就是這種情況（完整演算法是 10，捷徑是 70）。上游在意的話有兩條路：捷徑只在 trait 說可以時走（多一個 opt-in 的方法），或者接受（瀏覽器算 intrinsic 寬度時也不用 flex 之後的高度）。
- **為什麼不把容器自己的 `aspect-ratio` 排除？** 有比例而且高度已知時，寬度在 `compute_flexbox_layout` 更早的地方就定了，走不到這裡。
- **row 被問高度呢？** 對稱的情況（只問高度的 row）要子項的寬度才能量高度，省不掉。

## CHANGELOG

建議放在 `Unreleased` 的 performance 類（上游沒有這一類的話放 Changed）。
