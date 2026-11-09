# RFC 0074 的細節：整數運算寫明溢位時怎麼辦，信任邊界上設上限

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## lint 的數量（2026-10-22）

在 `main`（`9fd73098`）上跑：

```sh
cargo clippy --workspace --all-targets --message-format=json -- \
  -W clippy::arithmetic_side_effects -W clippy::cast_possible_truncation \
  -W clippy::cast_possible_wrap -W clippy::cast_sign_loss -W clippy::as_conversions
```

依檔案、行、欄去重，只算 `crates/*/src`（不含 `tests/`）：

| crate | `arithmetic_side_effects` | `as_conversions` | `cast_possible_truncation` | `cast_possible_wrap` | `cast_sign_loss` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `animation` | 13 | 3 | 0 | 0 | 0 |
| `bindings` | 6 | 3 | 1 | 0 | 1 |
| `browser` | 14 | 2 | 0 | 0 | 0 |
| `compositor` | 12 | 21 | 10 | 0 | 6 |
| `compositor_paint` | 7 | 11 | 3 | 0 | 1 |
| `computed_style` | 16 | 7 | 4 | 0 | 1 |
| `css` | 38 | 45 | 31 | 0 | 12 |
| `dom` | 91 | 59 | 8 | 0 | 0 |
| `dom_api` | 136 | 159 | 84 | 1 | 25 |
| `dom_js` | 13 | 28 | 8 | 1 | 5 |
| `dom_wasm` | 6 | 9 | 4 | 0 | 2 |
| `driver` | 17 | 18 | 15 | 0 | 4 |
| `gfx` | 22 | 51 | 31 | 0 | 3 |
| `html` | 53 | 1 | 0 | 0 | 0 |
| `layout` | 127 | 75 | 38 | 1 | 2 |
| `net` | 7 | 6 | 2 | 0 | 0 |
| `paint` | 80 | 49 | 11 | 7 | 6 |
| `protocol` | 1 | 3 | 3 | 0 | 2 |
| `raster` | 22 | 58 | 8 | 8 | 6 |
| `renderer` | 47 | 11 | 4 | 0 | 3 |
| `shell` | 62 | 102 | 42 | 4 | 17 |
| `style` | 49 | 21 | 10 | 0 | 1 |
| `viz` | 18 | 24 | 5 | 0 | 4 |
| 合計 | 857 | 766 | 322 | 22 | 101 |

- 三個 cast 的 lint 去掉同一處的重複是 365 處：float 轉整數 226、整數之間 139。
- `tests/` 底下另外有 `arithmetic_side_effects` 149、`as_conversions` 144、三個 cast 的 lint 共 110（只算 `tests/`；`src/` 裡 `#[cfg(test)]` 的模組算在上表裡，所以測試不套用之後上表會少一些）。
- `arithmetic_side_effects` 的 857 處，依被標出的運算式粗分：`± 1` 489、其他加減 221、乘法 71、和 `.len()` 有關的 51、除法和餘數 15、取負 9、位移 1。
- 現有的寫法：`checked_*`、`saturating_*`、`wrapping_*`、`overflowing_*` 共約 100 處（`saturating_sub` 25、`saturating_duration_since` 16、`saturating_add` 14、`checked_add` 10、`wrapping_mul` 9……）。
- `as_conversions` 有 766 處，包括 `u32 as u64` 這種不會出錯的轉型，所以不開。

## 找到的問題

三個子領域分開讀程式找的（DOM 和 bindings、style 和 layout、繪圖和網路），每一項讀過程式確認。行號是 `9fd73098` 的。

### 網頁能讓程式結束或卡住

- **canvas 大過 GPU 的 texture 上限**：`crates/dom/src/canvas.rs` 的 `is_valid_bitmap_size` 允許每邊到 65535、面積到 32768 × 8192（Chromium 的 `IsValidImageSize`），但 viz 向 wgpu 要的是 `Limits::downlevel_defaults().using_resolution(adapter.limits())`（`crates/viz/src/gpu_context.rs`），Apple Silicon 上每邊 16384。當成 layer 的 canvas 經過 `create_shared_image` 到 `device.create_texture`，驗證失敗；repo 裡沒有 `push_error_scope`、`on_uncaptured_error`，wgpu 預設的處理是 panic。例：`<canvas width=16385 height=1>` 畫一次。沒有 GPU 時 pixels 放在 `TransferableResource` 裡、由 `WgpuRenderer` 上傳，也一樣。
- **`drawImage`、`createPattern` 的 canvas 來源**：`crates/dom_api/src/canvas.rs` 的 `source_image` 在 `snapshot()` 是 `None`（沒有 context、還沒畫過、大小不合法）時 `background.repeat(width as usize * height as usize)`；大小來自 `canvas_size()`，解析到 `i32::MAX`，沒有經過 `is_valid_bitmap_size`。2³¹ × 2³¹ 時 `Vec` 的 capacity overflow panic；65535 × 65535 時不經 `try_reserve` 配置約 17 GB。同一個檔案的 `bitmap_to_export` 和 `dom` 的 `pixels_mut` 都先檢查大小、用 `try_reserve_exact`。
- **表格的總欄數**：`crates/layout/src/table.rs` 放 cell 時，`taken` 長到最寬的一列的欄數，之後每一列都 `for t in &mut taken { *t = t.saturating_sub(1) }`。每個 span 有上限（`MAX_COLSPAN` 1000，Blink 的值），總欄數沒有：一列 1000 個 `colspan=1000` 加上一萬個空的 `<tr>`（不到 200 KB 的 HTML）約 10¹⁰ 次。`border-collapse: collapse` 時 `vec![edge; (rows + 1) * (columns + 1) * 2]`（16 byte 一個）在 1000 列 × 100 萬欄時約 32 GB；Blink 的 `TableBorders` 也是密的，問題在總欄數。Blink 的 `ColspanCellTabulator` 用區段記被佔用的欄，成本和 cell 的數量成正比。
- **`grid-template-areas`**：`crates/css/src/grid.rs` 不檢查就 `r0 as u16 + 1`、`c1 as u16 + 2`、`rows.len() as u16`、`columns as u16`；65536 格（約 131 KB）時 debug 溢位 panic、release wrap，超過時截斷，再交給 taffy。taffy 的上限是 `MAX_GRID_TRACKS`（10000），Blink 是 `kGridMaxTracks`（1000000）。同一個函式用 `areas.iter_mut().find(..)` 找名字，成本是格數乘以名字數。
- **`double` 的 border 和 outline**：`crates/paint/src/lib.rs` 的 `d = (side.width * dsf).round() as i32` 飽和，接著 `d * 2 / 3` 溢位；border 的寬度從 CSS 到這裡沒有 clamp。`border: 1.5e9px double` 在 debug 時 panic，release 時比例錯、畫錯（不影響記憶體安全）。

### release 下行為錯

- **CSS 長度和字級沒有範圍**：`Length::parse`（`crates/css/src/values.rs`）照收 cssparser 的 f32（`1e39px` 是 inf）；`to_px` 的乘法不 clamp；長度的 `calc()` 不檢查是不是有限的（數字、角度、時間的 `calc()` 會拒絕）；`font-size`（`crates/css/src/font.rs`）不 clamp；HTML 的尺寸屬性（`crates/style/src/hints.rs`）解析成 f32。inf 再組成 NaN（`width: 1e39px; margin-left: -1e39px`）。Blink：`kMaxValueForCssLength`、`LayoutUnit` 飽和在約 ±3355 萬 px、字級有上限。在 style 和 layout 裡沒有找到因此 panic 的地方，影響落在 parley、paint 和 raster（double border 是其中一個）。
- **`<select size>` 的 PageDown**：`crates/dom_api/src/select_type.rs` 的 `page_away` 是 `size - 1 + start`（i32），`size` 到 `i32::MAX`；`<select size=2147483647>` 加上一個合成的 PageDown 的 `keydown`（`default_event_handler` 不看 `isTrusted`）在 debug 時 panic，release 時跳到第一個 option。
- **timer 的 handle**：`crates/dom/src/timers.rs` 的 `new_handle` 是 `u32` 的 `+= 1`，`setTimeout`、`setInterval`、`requestAnimationFrame`、`requestIdleCallback` 和所有 window 共用。`setTimeout` 回傳 `u32`（IDL 是 `long`）、`clearTimeout` 收 `i32`：超過 2³¹ 次之後清不掉；2³² 次時 debug panic、release 從 0 開始重複，`insert` 蓋掉還活著的 timer。Blink 在 `INT_MAX` 繞回 1、跳過還在用的。
- **`NodeId`**：`crates/dom/src/node_id.rs` 用 `checked_add(1).expect(..)`，ID 不重用，所以不會 use-after-free；建了 2³² 個 node 之後 renderer panic。是刻意的設計，留著。

### 拆成 process 之後的風險

- **viz 不驗證 resource**：`crates/protocol/src/viz.rs` 的 `TransferableResource` 的 `size` 和 `pixels` 互相獨立；`crates/viz/src/wgpu_renderer.rs` 只看 `size.is_empty()` 就上傳。`pixels.len()` 小於 `w * h * 4` 時 `write_texture` 驗證失敗、panic；太大時 `create_texture` 一樣。Chromium 的 viz 用 `CheckedNumeric` 驗證 shared bitmap 的大小，不對就 `ReportBadMessage`。同一層做對的：quad 用 `.get` 找 `shared_quad_state`，surface 的嵌套用路徑擋掉循環。

### automation 的輸入（有特權，嚴重度低）

- WebDriver 的 Set Window Rect（`crates/driver/src/session.rs` 的 `width as u32`）和 CDP 的 `Browser.setWindowBounds`（`crates/shell/src/automation_session.rs`）不限制大小；windowless 時到 `Output::offscreen` 的 `create_texture` panic，interest rect 是 viewport 的三倍，tile 的數量也跟著變大。
- `crates/shell/src/popup_menu.rs` 的 `self.scroll + rows`：`rows` 由很大的 wheel 的 delta 飽和成 `usize::MAX`，溢位。

### 碰不到的、無害的

- 只在 32 位元的平台上會 wrap 的 `w * h * 4`（`gfx::ImageData::decoded` 的 `assert_eq` 用同一個乘積）：kun 目前只有 64 位元的平台；skia-safe 的 `valid_pixels` 用 `compute_byte_size` 檢查 buffer 的長度。
- `skia_backend.rs` 的 `size.width as i32`：大於 `i32::MAX` 時 `from_raster_direct` 回 `None`、`.expect` panic；canvas ≤ 65535、tile ≤ 256，碰不到。
- 要比記憶體或 V8 字串的上限（約 2²⁹）更多才會溢位的 `+ 1`：`abort.rs`、`media_query_list.rs`、`css_style_sheet.rs`、`animation_objects.rs`、`range.rs`、`text_control.rs`、`html` 的 `fast_path.rs`、layout 的文字位移（超過 4 GB 的文字）。
- 依賴 `as` 飽和、NaN 變 0 的：`(v + 0.5).floor() as i32`（`calc()` 的整數）、`.clamp(0.0, 255.0) as u8`、SVG 弧的段數、`gfx::Size::scale_to_ceiled`、srcset 的 density 是 0。都是對的，lint 打開之後改用 `numerics` 的 helper。
- 列表的插值重複到兩個長度的最小公倍數（`crates/css/src/interpolate.rs`）：規範這樣寫，Blink 也是；互質的長的列表每個 frame 配置很多。不在這份的範圍。

另外一個和溢位無關的偏差：`crates/dom_api/src/editing.rs` 的 `max_length` 用 `str::parse::<u32>`，沒有照 HTML 的 rules for parsing integers（`"10px"` 當成沒有上限），和 `reflection::parse_non_negative_integer` 不一致。

## 已經做對的寫法

改寫時照這些：

- **CharacterData、`splitText`**：`offset > len` 丟 `IndexSizeError`，`start.saturating_add(count).min(len)`，長度 `u32::try_from(..).unwrap_or(u32::MAX)`。
- **Range、Selection**：`offset > node_length` 丟 `IndexSizeError`；`compareBoundaryPoints` 的 `how % 0x1_0000`。
- **文字控制項**：`setSelectionRange` 夾到長度內；`setRangeText` 用 i64 算 delta。
- **reflection**（`crates/dom_api/src/reflection.rs`）：`parse_integer` 用 i64 再 `i32::try_from`；`colSpan`、`rowSpan` 用飽和的 u64；`srcset` 的寬度飽和到 `u32::MAX`。
- **ImageData、`getImageData`、`putImageData`**：`checked_mul`、i64 的座標、`try_reserve_exact`；canvas 的 bitmap 用 u64 算面積。
- **WebIDL 的轉換**（`dom_js` 的 `convert.rs`、`dom_api` 的 `value.rs`）：`unsigned long` 用 `rem_euclid(2³²)`，`[EnforceRange]` 超出範圍丟 `TypeError`，`[Clamp]` 先 clamp 再 `round_ties_even`；wasm 的 guest 傳來的值再檢查一次。
- **counter**（`crates/layout/src/counters.rs`、`list_items.rs`）：`checked_add(..).unwrap_or(value)`、總和用 `saturating_add`、`start` 用 i64 再 clamp 到 i32；羅馬數字、字母只用在範圍內，其他退回十進位。
- **HTML 的整數屬性**（`crates/style/src/hints.rs`）：`parse_integer` 用 i64 累加、有上限、再 clamp；`<ol start>` 用 `saturating_add`。
- **表格的 span**：`MAX_COLSPAN` 1000、`MAX_ROWSPAN` 65534（Blink 的值）。
- **grid**：`repeat()` 的次數 `min(u16::MAX)`；line 和 span clamp 到 i16、u16；taffy 用飽和的運算、`MAX_GRID_TRACKS`。
- **`var()`**：`MAX_SUBSTITUTION_BYTES` 2 MB（Blink 的值）；SVG 的 `<use>` 有 `MAX_COPIES`。
- **`dom` 的 `unsafe` 周圍**：`weak_set.rs` 的 `checked_add(1).expect(..)` 和 `saturating_mul(2)`；`scope.rs` 先比 `index >= len` 才 `get`；`node_id.rs`、`foreign.rs` 的 `checked_add().expect` 和 `assert!(index < len)`。
- **繪圖**：`raster/src/encode.rs` 的 `i32::try_from`；font collection 的 `checked_add`、`u32::try_from`；`PictureLayerTiling` 用 f64 算格子；scissor 先 intersect 再轉型；截圖的裁切 clamp 到 frame。
- **其他**：`browser` 的 `GoToEntryAtOffset` 用 `checked_add_signed`；`Blob.slice` 用 `saturating_add`、`try_from`；blob 的 Range 用 `saturating_sub`；net 不用 Content-Length 預先配置。

## Blink 怎麼限制 CSS 長度（2026-10-22）

讀 Chromium `main` 的 `third_party/blink/renderer/`：

- **長度**：`core/css/css_primitive_value.cc` 的 `ComputeLength<Length>` 把算出的 px 交給 `ClampToCSSLengthRange`：先 `CSSValueClampingUtils::ClampLength`（NaN 變 0、clamp 到 double 的範圍），再 clamp 到 `kMinValueForCssLength`、`kMaxValueForCssLength`（`INT_MIN / LayoutUnit::kFixedPointDenominator + 2`、`INT_MAX / LayoutUnit::kFixedPointDenominator - 2`；分母是 64，所以約 ±33554429）。註解說減 2 是為了轉成 float 再轉回來時不超過 `LayoutUnit::Max`。parse 時不 clamp，specified value 照寫的保存。
- **`calc()`**：`core/css/css_math_function_value.cc` 的 `ClampToPermittedRange` 先 `CensorNaNToZero`；能直接算成 px 的轉成 `Length::Fixed`，帶百分比的轉成 `CalculationValue`。`platform/geometry/calculation_value.cc` 的 `Evaluate` 在用到時算，NaN 變 0、`ClampTo<float>`，再由 `LayoutUnit` 飽和（`platform/geometry/layout_unit.h`：NaN 變 0，超出範圍 clamp 到 `Min()`、`Max()`）。
- **字級**：`core/css/resolver/font_builder.cc` 的 `FontBuilder::SetSize` 用 `std::min(kMaximumAllowedFontSize, specified_size)`，負的不設；`core/style/computed_style_constants.h` 的 `kMaximumAllowedFontSize = 10000.0f`，註解說太大的字級在某些平台（例如 Windows）上會 crash。

kun 照做的是位置（computed value）和數值；f32 在 2²⁵ 附近的間隔是 4，所以 kun 的上限取 33554428。

## Blink 怎麼畫過大的 canvas（2026-10-22）

- `platform/graphics/canvas_2d_resource_provider.cc`：建 shared image 的 provider 時，`size.width()` 或 `size.height()` 超過 `capabilities.max_texture_size` 就回 `nullptr`。
- `modules/canvas/canvas2d/canvas_rendering_context_2d.cc` 的 `IsComposited()`：沒有 `GetSharedImageProvider()` 時是 false，canvas 沒有自己的 cc layer。
- `core/paint/html_canvas_painter.cc`：有 `ContentsCcLayer()` 時記一個 foreign layer；沒有的話 `canvas->Paint(...)` 把 bitmap 畫進所在的 display list，由 raster 切成 tile。
- 沒有把 canvas 切成幾個 texture、也不縮小。

## Blink 怎麼放表格的 cell（2026-10-22）

讀 `core/layout/table/`：

- `table_layout_utils.h`、`.cc` 的 `ColspanCellTabulator`：只記 rowspan 大於 1 的 cell（`column_start`、`span`、`remaining_rows`）。`FindNextFreeColumn` 跳過清單裡蓋住目前這一欄的 cell；`ProcessCell` 前進 colspan 欄；`EndRow` 把清單裡的 `remaining_rows` 減 1、到 0 就移除，再照 `column_start` 排序。
- 總欄數沒有上限：`ComputeMaximumNonMergeableColumnCount` 用 `UINT_MAX` 走過 `<col>`；auto 的 layout 裡 `ComputeMaxColumn` 是 `current_column + 1`（只有 colspan 蓋到的尾端的欄是 mergeable、會被修掉），fixed 的是 `current_column + colspan`。每個 span 的上限是 `kMaxColSpan`、`kMaxRowSpan`（`layout_table_cell.cc` 的 `DCHECK_LE`）。
- `table_borders.cc`：collapsed border 的 `edges_` 是 `row_count * edges_per_row` 的密的陣列，欄數變了整個 resize。

## 其他做法

| 做法 | 優點 | 缺點 | 結論 |
| --- | --- | --- | --- |
| lint 強制，依用途選寫法，再加邊界的上限 | 新的程式也受管；每一處寫明意圖；找到的問題都處理 | 改寫的量（上面的表）；`expect` 的理由要人審 | 選（第 1、5 條） |
| 全部改成 `saturating_*` | 機械式、簡單 | 錯的值換成另一個錯的值；邊界的問題都還在；把 bug 藏起來 | 不選 |
| 只在信任邊界設上限 | 改動小、直接處理找到的問題 | 新的程式靠審查 | 不選（使用者選了 lint 加上限） |
| release 打開 overflow-checks（全部） | 改一行；wrap 變成 panic | 效能；依賴的 crate 也被檢查，要維護例外；Chromium 2025-01-23 撤回 | 不選（第 4 條） |
| 只對 workspace 的 crate 打開 | 排除依賴，例外的負擔小 | lint 做完之後只多守住 `expect` 的理由寫錯的情況；要量 Speedometer | 之後再考慮 |
| clippy 的 `allow_attributes`、`allow_attributes_without_reason` | clippy 本身擋住沒有理由的例外 | 管所有的 lint，現有的 109 個 `allow` 都要改，有些在 macro 的展開裡改不了 | 不選，用 `sanity.py numerics` 只管這四個 lint |

## 參考資料

- Chromium 拿掉 Rust 的 `-Coverflow-checks=on`（2025-01-23）：<https://chromium.googlesource.com/chromium/src/+/c3e787ece0de528f6b5a44a9c7460729380b35a9>
- Chromium 的 `base/numerics`：<https://chromium.googlesource.com/chromium/src/+/HEAD/base/numerics/README.md>
- clippy 的 `arithmetic_side_effects`：<https://rust-lang.github.io/rust-clippy/master/index.html#arithmetic_side_effects>
