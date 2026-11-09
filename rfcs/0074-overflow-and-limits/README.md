# RFC 0074：整數運算寫明溢位時怎麼辦，信任邊界上設上限

- 開始日期：2026-10-22
- 狀態：✅ 已完成：2026-10-22
- 進度和驗收：`progress.md`
- 細節：`details.md`（2026-10-22 的盤點：lint 的數量、找到的問題、已經做對的寫法）

## 摘要

用 clippy 的四個 lint（`arithmetic_side_effects`、`cast_possible_truncation`、`cast_sign_loss`、`cast_possible_wrap`）擋住沒有寫明溢位時怎麼辦的整數運算和轉型：每一處依用途選 checked、飽和、wrapping，或用 `#[expect(..., reason)]` 說明為什麼不會溢位。workspace 的每個 crate 都受管（測試除外），`sanity.py numerics` 檢查例外的寫法。共用的 helper 放在新的 `base` crate（Chromium 的 `base/`）：`numerics` 和用完時 panic 的 `SequenceNumber`、`AtomicSequenceNumber`。lint 看不出來的（整條路上沒有上限）另外補：canvas 對 GPU 的大小、viz 收 frame 時的驗證、CSS 長度和字級的範圍（連同 `calc()` 的 `infinity`、`NaN`）、表格和 grid 的格數、automation 的視窗大小。release 不打開 overflow-checks。

## 動機

- **release 溢位時悄悄 wrap**：`Cargo.toml` 沒有 `[profile.release]`，release 和 `perf` 的 overflow-checks 是關的；測試（`cargo test`、WPT、情境）都用 dev profile，溢位在那裡會 panic，但只有測試碰到的值才抓得到。
- **寫法不一致**：`dom`、`dom_api` 處理網頁輸入的地方大多已經用了 `checked_*`、`saturating_*`、`try_from`、i64 的中間值或 Blink 的上限（`details.md` 的「已經做對的寫法」），但沒有規則，新的程式要靠審查才會照做；ID 和計數器有的 `checked_add(1).expect(..)`，有的直接 `+= 1`。
- **2026-10-22 的盤點找到的問題**（`details.md` 的「找到的問題」），大多不是某一行的溢位，而是整條路上沒有上限：
  - 網頁能讓程式結束或卡住：canvas 每邊允許到 65535，比 GPU 的 texture 上限大，wgpu 驗證失敗就 panic（`crates/dom/src/canvas.rs`、`crates/viz/src/gpu_context.rs`）；`drawImage` 一個沒有 snapshot 的 canvas 時，不檢查大小就配置 `width * height` 個像素（`crates/dom_api/src/canvas.rs`）；表格的總欄數沒有上限（`crates/layout/src/table.rs`）；`grid-template-areas` 不檢查就轉成 `u16`、找名字是平方的時間（`crates/css/src/grid.rs`）；很粗的 `double` border 的整數溢位（`crates/paint/src/lib.rs`）。
  - release 下行為錯：CSS 長度和字級沒有範圍，會變成 inf、NaN；`<select size>` 很大時 PageDown 溢位（`crates/dom_api/src/select_type.rs`）；timer 的 handle 是 `u32`、`clearTimeout` 收 `i32`，2³¹ 次之後清不掉，2³² 次之後重複（`crates/dom/src/timers.rs`）。
  - 拆成 process 之後的風險：viz 不驗證 `TransferableResource` 的像素長度和大小（`crates/protocol/src/viz.rs`、`crates/viz/src/wgpu_renderer.rs`），被攻破的 renderer 能讓 browser 結束。

目標：

- workspace 的每個 crate 都受這四個 lint 管（測試除外）；例外都是 `#[expect]` 並寫明理由。
- 上面列的問題都修好，各自有測試。
- 寫法的規則寫在 `base` 的 README，新的程式照著寫，`sanity.py` 擋住沒有理由的例外。

不在範圍內：

- 浮點數的運算本身（lint 不管 f32、f64）：只在 float 轉整數和 CSS 長度的範圍上處理。
- `LayoutUnit` 那樣的定點數（Blink 的 layout 單位）：layout 照舊是 taffy 的 f32，只在算出 computed value 時限制範圍（第 6 條）。
- 圖片解碼後的大小和 tile 總量的記憶體預算（Chromium 的 `ImageDecodeCache`、`TileManager` 的上限）：另外做（「之後的可能」）。
- `sdk/` 底下的 guest：不在 workspace 裡，跑在 wasm 的 sandbox 裡。
- 測試的程式碼（第 3 條）。
- release 的 overflow-checks（第 4 條）。

要解決的 `docs/limitations.md` 裡的限制：無（上面的問題是這次盤點才找到的，沒有記在那裡）。

## 設計概要

做完之後，改這部分程式的人寫整數運算時，clippy 要求寫明溢位時怎麼辦：

```rust
// 大小、配置、跨 process 的資料：溢位就是錯，回錯誤或不畫。
let bytes = width.checked_mul(height)?.checked_mul(4)?;

// CSS 和 layout 的整數值：照規範飽和或 clamp。
let span = span.saturating_add(1);

// ID、序號：用完時 panic，或照明確的規則循環。
let id = self.next_id.get_next();             // base::SequenceNumber

// float 轉整數：飽和轉型，NaN 變 0，寫明是故意的。
let x = base::numerics::clamp_round::<i32>(offset);

// 證明過不會溢位：改寫不了時，寫明理由。
#[expect(clippy::arithmetic_side_effects, reason = "i < items.len(), so i + 1 <= len")]
let next = i + 1;
```

網頁看到的行為：上面列的問題改成和 Chrome 一樣（不會結束、不會卡住，大的值照 Blink 的上限 clamp）。其他地方行為不變。

## 詳細設計

### 1. lint 和推行

- `Cargo.toml` 的 `[workspace.lints.clippy]` 加上四個 lint，等級是 `warn`，和現有的 `undocumented_unsafe_blocks` 一樣：clippy 一律用 `-D warnings` 跑，`warn` 和 `deny` 沒有差別（第 1 條）。不開 `as_conversions`：`u32 as u64` 這種不會出錯的轉型它也擋（2026-10-22 量到 910 處）。
- **推行**：分四段改完（「分階段」）。推行期間，還沒改的 crate 在根上寫 `#![allow(clippy::arithmetic_side_effects, ..., reason = "not converted yet")]`，改完一個拿掉一個，和 `unsafe_code` 的做法一樣；新的 crate 一開始就受管。不能在 crate 的 `Cargo.toml` 裡個別打開：每個 crate 都是 `lints.workspace = true`，Cargo 不允許再加別的 lint。最後一批改完時這一行和它的例外都刪掉了（下面的 `NOT_CONVERTED`）。
- **例外**一律 `#[expect(lint, reason = "...")]`，不用 `#[allow]`：程式改了、那裡不再觸發時，`expect` 會報沒有觸發，過時的例外不會留著。Rust 穩定版的屬性不能放在單一運算式上，所以例外最小是一個 statement、`let` 或函式；能改寫（iterator、`split_last`、`checked_sub` 的 `?`）就改寫，不要一路加 `expect`。
- **`sanity.py numerics`**（和 `sanity.py unsafe` 一樣的寫法）：四個 lint 在 workspace 打開；對它們的 `allow` 只能是每個根上測試的那一行（第 3 節），每個根都要有；其他地方的例外必須是帶 `reason` 的 `expect`。沒有 crate 能整個不受管。
  - **推行期間**還沒改的 crate 列在 `sanity.py` 的 `NOT_CONVERTED`（和 `UNSAFE_ALLOWED` 一樣的寫法），只有清單裡的能用 `"not converted yet"`，每批改完從清單拿掉。最後一批改完時刪掉清單和這條例外（第 11 條）：不然以後任何 crate 都能用同一行把整個 crate 關掉。
  - **CI 跑它**：`.gitlab-ci.yml` 的 `sanity` job 加上 `python3 scripts/sanity.py numerics`（原本跑 `unsafe`、`deps`、`boundary`、`tests`、`docs` 五項）和它自己的測試 `scripts/sanity_numerics_test.py`，`CLAUDE.md` 的「測試」裡完成一段工作前要跑的 sanity 檢查也加上它；`ci.py`、`test.py` 開頭說明 `sanity.py` 有哪些檢查的地方一起改。CI 的設定由使用者維護，改動在第 1 段的 commit 裡給人審過。不用 clippy 的 `allow_attributes`、`allow_attributes_without_reason`：它們管所有的 lint，現有的 109 個 `allow`（多半是 `too_many_arguments`、`drop_non_drop`，有些在 macro 的展開裡，換成 `expect` 會在某些展開裡沒有觸發）都要跟著改，和這份無關。

### 2. 寫法的規則

寫在新的 `base` crate 的 README，依用途選：

| 用途 | 寫法 | Chromium |
| --- | --- | --- |
| 大小、配置、索引的計算，跨 process 的資料 | `checked_*`，失敗時回錯誤、不畫或丟掉訊息 | `CheckedNumeric`、`ReportBadMessage` |
| CSS、HTML 屬性、layout 的整數值 | 飽和或 clamp，照規範和 Blink 的上限 | `ClampedNumeric`、`ClampTo` |
| ID、序號、計數器 | `SequenceNumber`、跨 thread 的 `AtomicSequenceNumber`（用完時 panic），或那個 ID 自己明確的循環規則 | `base::AtomicSequenceNumber`、`IdType` |
| hash、亂數、checksum | `wrapping_*` | 無號數的 wrap |
| float 轉整數 | `saturated_cast`、`clamp_round`、`clamp_floor`、`clamp_ceil` | 同名的 `base::saturated_cast`、`ClampRound`…… |
| f64 轉 f32 | `f32_from_f64`（最接近的 f32，超出範圍是無限大） | Skia 的 `SkDoubleToScalar` |
| 整數轉整數 | 放得下的用 `From`；可能放不下的用 `try_from`，再決定錯了怎麼辦 | `checked_cast`、`saturated_cast` |
| 統計、只給測試看的計數 | `wrapping_*`（繞回只會數錯） | — |
| 證明過不會溢位 | 先試著改寫；改寫不了的 `#[expect(..., reason)]`，不寫 `checked_*().expect()`（release 多一個分支，第 18 條；版本號這種用完時就該 panic 的除外） | 審查 |

飽和不是預設：它把錯的值換成另一個錯的值，只用在規範本來就要 clamp 的地方。

### 3. 測試不套用

- 每個 lib 和 bin 的根加 `#![cfg_attr(test, allow(clippy::arithmetic_side_effects, ...))]`，每個整合測試的根（tests/ 底下的檔案、tests/it/main.rs）加 `#![allow(...)]`（第 3 條）。推行期間，還沒改的 crate 的根只放 `"not converted yet"` 那一行，改完才換成 `cfg_attr`：兩行都在時，測試的設定下同一組 lint 被 allow 兩次，clippy 的 `duplicated_attributes` 報錯。`clippy --all-targets` 另外在沒有 `cfg(test)` 的設定下檢查 lib，正式的程式照樣受管。
- `dom/src/testing.rs` 是編進 lib 的測試工具，不是 `cfg(test)`，照樣受管。
- `sanity.py numerics` 檢查每個根都有它的那一行。

### 4. `base` crate

新的 crate `base`（Chromium 的 `base/`），放在 `crates/` 底下，沒有依賴（第 2、9 條）：

- `numerics` 模組的 `saturated_cast::<T>(x)`、`clamp_round`、`clamp_floor`、`clamp_ceil`：float 轉整數（`as` 本來就是飽和的、NaN 變 0，這裡集中寫一次 `expect`、寫明是故意的）和整數之間的飽和轉型（十種整數，包括 `isize`、`usize`，經過 `i128` 夾住）；`f32_from_f64`（4a 從 `gfx` 搬過來的，在 f64 裡算精度、再回到 f32 的幾何用它）。
- `SequenceNumber<T>`、`AtomicSequenceNumber<T>`（Chromium 的 `base::AtomicSequenceNumber`，`base/atomic_sequence_num.h`）：`get_next()`（Chromium 的 `GetNext`；叫 `next` 會和 `Iterator::next` 混淆，clippy 不准）回下一個值，用完時 panic（`"… exhausted"`）。前者取代 `next_x += 1`（`dom` 的 microtask、abort steps、port、opaque origin 的編號，compositor 的 resource id 和 frame token（第 19 條），automation 和 machopodriver 的編號……），後者取代產生 ID 的 `fetch_add(1, ..)`（`LayerId`、`PaintId`、`ImageData`、mailbox、renderer 的 id）。要循環的 ID（timer 的 handle）不用它們，寫自己的規則。版本號（`dom` 的 `version::bump`、`style` 的 stylesheet 版本）用完時 panic，不 wrap 回快取還留著的值。

`base` 之外（第 9 條）：

- `gfx::Size::checked_area()`、`checked_byte_size()`：回 `Option<usize>`（Chromium 的 `gfx::Size::GetCheckedArea()`、Skia 的 `SkImageInfo::computeByteSize`），取代約 23 處的寬 × 高 × 4。
- `clippy.toml` 的 `disallowed-methods` 加上 atomic 整數的 `fetch_add`（`std::sync::atomic::Atomic::fetch_add`：`AtomicU32` 等現在是 `Atomic<T>` 的別名，clippy 認不得別名的路徑，泛型的這條連有號數的也擋，它們一樣悄悄 wrap）：`fetch_add` 是方法呼叫，overflow-checks 和 `arithmetic_side_effects` 都看不到它悄悄 wrap。產生 ID 的改用 `AtomicSequenceNumber`；單純統計、wrap 也無妨的計數器寫 `#[expect(clippy::disallowed_methods, reason = "...")]`。2026-10-22 有 18 處，逐一判斷（測試裡另有 7 處，也一樣寫 `expect` 或改用 `AtomicSequenceNumber`）。
- `arithmetic-side-effects-allowed` 先留空：上面的型別都用方法，不用 `+`、`*` 運算子。之後有用運算子、把規則寫在型別裡的型別（例如 `LayoutUnit`）才列進去。
- 時間不做型別：`Instant + Duration` 約 21 處，來源是 kun 自己或 V8（timeout、frame 的間隔），逐處 `checked_add` 或 `expect`；同一個檔案裡有好幾處的，集中到一個 helper（Viz thread 的 `after`、automation 的 `deadline` 把 timeout 夾到一百年、machopodriver 的 `after_ms` 用 `checked_add`）。wasm 的 guest 傳的等待時間（`crates/dom_wasm/src/wasi.rs` 的 `Duration::from_nanos(u64)`）加到 `Instant` 上不會 panic（2026-10-22 在 macOS 上試過 `u64::MAX` 奈秒，`Instant` 存的是 i64 的秒）。

### 5. lint 看不出來的上限

每一項都是一個 bug 修正，照 Chromium 的上限：

1. **canvas 對 GPU**（第 7 條）：canvas 的 bitmap 的寬或高超過 GPU 的 texture 上限時不當成 layer，當成一般的 display item 畫進所在 layer 的 tile（Blink：`Canvas2DResourceProvider` 超過 `max_texture_size` 時建不出 shared image，`IsComposited()` 是 false，`HTMLCanvasPainter` 把它當成圖片畫）。
   - 上限是 `protocol::viz::MAX_TEXTURE_SIZE`：固定的 8192（WebGPU 保證的 `max_texture_dimension_2d`），viz 建 device 時要求至少這麼大（第 13 條）。renderer 啟動時 device 還沒建，不能等它的真正上限。放在 `protocol::viz`，不放 `LayerTreeSettings`：它是 renderer 和 Viz 之間的約定，兩邊都讀；renderer 的 `PaintArtifactCompositor` 在 main thread 上，拿不到 compositor 的設定（測試把它設小）。
   - `paint/src/layerize.rs` 的 `canvas_layer` 在 bitmap（display item 的圖片的大小）超過它時回 `None`，`layerize` 多收這個上限。要在 layerize 決定：renderer 的 `compositing.rs` 才放棄的話，那個 chunk 已經被當成 layer、不會畫進 tile，canvas 會不見。
   - 不在 canvas 的資源那一側檢查（Blink 的位置）：kun 的 canvas 一律在 CPU 上畫，要不要合成由 layerize 決定，效果一樣。
   - wgpu 的驗證錯誤照舊 panic：這一項和第 3 項之後，正常的路上不會有過大的 texture，還發生就是 kun 的 bug，讓它明顯地出現。
2. **`drawImage`、`createPattern` 的 canvas 來源**：大小不合法（`is_valid_bitmap_size`）時不配置記憶體，`drawImage` 什麼都不畫、不丟例外，`createPattern` 照樣回傳圖案、畫出來什麼都沒有（2026-10-22 在 Chrome 152 實測，Blink 的 `kInvalidSourceImageStatus`）；沒畫過的透明像素用 `try_reserve_exact`，和 `pixels_mut` 一樣。
3. **viz 收 frame 時驗證**（`Display::submit_compositor_frame`，`create_shared_image` 照同樣的 `pixels_fit`；第 15 條）：`TransferableResource` 的 `pixels.len()` 等於 `width * height * 4`（checked）、大小在 texture 的上限內；不符合就丟掉那個 frame（Chromium 的 `ReportBadMessage`）。現在 renderer 和 viz 在同一個 process，這是拆開之前要有的檢查。
4. **CSS 長度和字級的範圍**（第 6 條）：在算出 computed value 時 clamp（Blink 的 `CSSPrimitiveValue::ClampToCSSLengthRange`），inf 和 NaN 不會進 layout、paint 和 parley。helper 放在 `computed_style`（`clamp_css_length`、`MAX_CSS_LENGTH`、`MAX_FONT_SIZE`；CSS 專用的規則，不放進 `base`；`css` 依賴它，`computed_style::LengthPercentage::resolve` 也要用，所以不放在 `css`），四個位置，另外 `line-height` 的數字乘上字級時（排版用 `LineHeight::number_px` 夾；`getComputedStyle` 照 Chrome 只保持有限，`3.40282e+38px`）。calc 的長度葉節點用不夾的 `Length::unclamped_px`，整個函式算完才夾（Blink 也是）：
   - `css::Length::to_px`：絕對單位、`em`、`rem`、viewport 單位、長度的 `calc()`；HTML 的尺寸屬性（`width=` 等）轉成 `Length::px`，也經過這裡。
   - `css::LengthPercentage::compute` 的 `Calc` 分支：帶百分比的 `calc()` 的 px 和百分比。
   - `computed_style::LengthPercentage::resolve`：用到時才算成 px 的百分比（Blink 的 `CalculationValue::Evaluate`）。
   - `FontSize::compute`（`crates/css/src/font.rs`）：`em` 和 `%` 直接乘父元素的字級，不經過 `to_px`。

   數值：長度和百分比 clamp 到 ±33554428（f32 能表示、不超過 Blink 的 `kMaxValueForCssLength` = `INT_MAX / 64 - 2` 的值），NaN 變 0；字級 clamp 到 0 到 10000（Blink 的 `kMaximumAllowedFontSize`）。百分比也 clamp，因為 taffy 自己乘百分比，它要是有限的數。clamp 之後幾個大的值加起來仍然可能超過範圍（Blink 的 `LayoutUnit` 讓總和也飽和，kun 的 f32 不會），但仍然有限，不做 `LayoutUnit`。
   - `calc()` 收 css-values 4 的 `infinity`、`-infinity`、`NaN` 和除以 0（第 17 條），specified value 照 Chrome 寫成 `calc(infinity * 1px)`；數字太大的 token 在 parse 時夾到 ±`f32::MAX`（Chrome 讀回 `3.40282e+38px`）。`sizes` 屬性裡算出 NaN 或無限大的長度跳過那個 size，不夾成 0（`parse_length_px`，WPT 的 `parse-a-sizes-attribute`）。
   - 數字、角度、時間的 `calc()`（第 10 條）：算出非有限的值時，原本整個宣告無效（`crates/css/src/calc.rs` 的 `parse_absolute`）；改成照 Blink（`CSSMathFunctionValue::ClampToPermittedRange`、`CSSValueClampingUtils`）：NaN 變 0，±inf 變成 ±`f32::MAX`（Blink 是 double 的最大值，kun 的這些值是 f32），再照那個 property 允許的範圍（例如不能是負的）。角度照 `ClampAngle` 夾到 360 的倍數（`sin` 還是 0）：Blink 的 `kApproxDoubleInfinityAngle`（2867080569122160）換成 f32 之後不是 360 的倍數，`rotate()` 會轉一個怪角度，所以用 f32 裡最接近它、是 360 的倍數的 `2867079917076480`（`MAX_ANGLE`），寫出來和 Chrome 一樣是 `2.86708e+15deg`。
5. **表格和 grid**：放 cell 時照 Blink 的 `ColspanCellTabulator` 只記跨列的 cell（第 8 條）：`taken` 換成 rowspan 大於 1 的 cell 的清單（起點、span、還剩幾列），找空的欄只看這份清單，每一列結束時只減它們，成本和 cell 數成正比、和總欄數無關；不設總欄數的上限，collapsed border 照舊是密的陣列（Blink 的 `TableBorders` 也是）。實作時確認 layout 沒有其他每一列都走過全部欄的迴圈。`grid-template-areas` 的 computed value 改成 `u32`、照寫的保留（Chrome 152 的 `getComputedStyle` 讀回 70000 格的 template 照原樣，Blink 到一百萬條 track），交給 taffy 時才飽和成 `u16`（taffy 自己再夾到它的 10000），名字用 hash map 找（Blink 的 `NamedGridAreaMap`）；讀回的字串每格只填一次，不再每格走過所有的 area。
6. **其他網頁碰得到的溢位**：`double` border 的寬度在 i64 裡算；`<select size>` 的 PageDown 用飽和運算（到最遠的那一個；Blink 的 `int` 溢位時 Chrome 152 跳到不一定的地方，沒溢位的和 Chrome 一樣）；timer 的 handle 照 Blink 在 `i32::MAX` 繞回 1、跳過還在用的，`setTimeout` 照舊回傳 `u32`（值都在 `long` 的範圍內，交給 script 的數一樣，不用改 `kun:dom` 的 WIT）；`setTimeout`、`setInterval`、`requestAnimationFrame`、`requestIdleCallback` 和所有 window 照舊共用一個編號（第 14 條）。
7. **automation 的視窗大小**：WebDriver 的 Set Window Rect、CDP 的 `Browser.setWindowBounds` clamp 到 texture 的上限（第 16 條；在 `automation_session` 的 `Browser.setWindowBounds` 夾，machopodriver 經過它）。

改寫時另外修的（原本會溢位、行為只在溢位時不同）：compositor 的 resource id 和 frame token、Viz 重新編的 resource id 改成 u64（第 19 條）；字型的 bounding box 寬度在 f32 裡算（兩個 `i16` 相減，很寬的字型溢位）；`popup_menu` 很大的滾輪位移捲到最後一頁；collapsed border 的陣列大小、像素的配置用 checked；automation 和情境的逾時夾到一百年。

### 6. 做完時要更新的文件

工具擋不住的規則（下面「缺點」的「lint 和工具擋不住的」）寫在現況文件裡，以後的人才讀得到：RFC 做完之後不再更新，`rfcs/` 以外也不引用它（第 11 條）。最後一段（4d）更新了：

- **`base` 的 README**：寫法的規則（第 2 節的表）、`numerics` 和兩個序號型別的用法、lint 看不到的方法（`pow`、`abs`、`Iterator::sum`、`product`、atomic 的 `fetch_sub`、`Duration::from_secs_f64`）要自己注意、`expect` 的理由怎麼寫、複合賦值包成區塊、不用 `checked_*().expect()` 代替（第 1 段起寫）。
- **`CLAUDE.md` 的「程式碼」**：一條規則，寫法和「`unsafe` 只在……」那幾條一樣：整數運算照 `base` 的 README 的規則寫明溢位時怎麼辦，例外是帶理由的 `#[expect]`，指向那份 README；「測試」的 sanity 檢查清單在第 1 段加上了 `numerics`。
- **`docs/architecture.md`**：
  - crate 的清單加上 `base`（對應 Chromium 的 `base/`，第 1 段）；canvas 的 layer 的條件加上 texture 的上限（第 2 段）；
  - 「Process 的邊界」的第 6 條：viz 驗證 renderer 送來的 frame（resource 的大小、像素的長度、texture 的上限），不符合就丟掉；新的訊息欄位要跟著驗證；
  - 「快取和增量：加新東西時要跟著改的地方」：新的 CSS 長度要經過 computed value 的 clamp（`to_px` 或同一個 helper），新的要上傳到 GPU 的資源要檢查 `protocol::viz::MAX_TEXTURE_SIZE`。
- **`crates/dom_api/README.md` 的「怎麼新增一個 API」**：參數裡的整數和大小照 `base` 的規則（索引和 offset 先比長度、丟規範要的例外，大小用 checked），大小由網頁決定的配置用 `try_reserve`。
- **`docs/test-gaps.md`**：「只靠審查」加上「溢位的路徑只靠 lint 和審查」（行為不變的改寫是等價的 mutation，見「測試」）、「lint 看不到的方法」，各寫之後怎麼處理。
- **`docs/limitations.md`**：極端的 collapsed border 表格耗盡記憶體時整個程式結束，併進「renderer panic 時整個程式結束」那一條；各段做的時候另外寫了：timer 的 handle 共用一個計數器（第 14 條）、grid 只排到 taffy 的 10000 條 track、calc 是 f32（第 17 條）、`LayoutUnit` 的加總、視窗大小和不合法的 frame（第 15、16 條）。

### 測試

各段做了哪些測試、mutation test 的結果和 WPT 的變化見 `progress.md`。計畫是：

- **lint 本身**：clippy 在階段做完時跑（`CLAUDE.md`），CI 每次 push 跑 clippy 和 `sanity.py numerics`；`sanity.py numerics` 有自己的測試 `scripts/sanity_numerics_test.py`，CI 也跑（違規的範例檔案報錯、合規的不報；推行期間不在 `NOT_CONVERTED` 裡的 crate 用 `"not converted yet"` 報錯，清單刪掉之後任何 `"not converted yet"` 都報錯）。
- **`base`**：`numerics` 的單元測試（邊界值、NaN、inf）、`SequenceNumber` 和 `AtomicSequenceNumber` 用完時 panic；`gfx::Size::checked_area`、`checked_byte_size` 在乘積溢位時回 `None`；都跑 mutation test。`sanity.py numerics` 的測試之外，`disallowed-methods` 由 clippy 在階段做完時檢查。
- **第 5 節的每一項**：用很大的值重現問題的測試，修好之前失敗（那個 crate 的整合測試；畫得出來的用 kun 自己的 reftest），跑 mutation test。能在 WPT 找到的（例如 `css/css-values` 的長度範圍、`html/semantics/embedded-content/the-canvas-element/` 的大小）跑相關的目錄。
- **`calc()` 的數字、角度、時間**：`calc(1e39)`、`calc(infinity)`、`calc(NaN)`、`calc(-infinity * 1deg)`、`calc(infinity * 1s)` 用在幾個 property 上，`getComputedStyle` 讀回的值和 Chrome 152 一樣（預期值先在 Chrome 上跑）；WPT 的 `css/css-values/` 裡 `infinity`、`NaN` 的測試。
- **canvas 對 GPU**：renderer 的單元測試把 `PaintArtifactCompositor` 的上限設成 4，確定過大的 canvas 不是 layer、畫進頁面的 layer；`shell` 的 `limits::` 用整個瀏覽器畫 16385 寬的 canvas，不 panic，畫出來和 8192 寬的一樣。
- **表格**：一列 1000 個 `colspan=1000`、很多列的表格在時間上限內排完（layout 的整合測試）；rowspan、colspan 交錯的表格放出來的位置不變（現有的表格測試和 WPT 的 `css/css-tables`）；弄壞 `FindNextFreeColumn`、`EndRow` 對應的地方，確認有測試失敗。
- **CSS 長度的範圍**：`getComputedStyle` 讀回 `1e39px`、`3e38in`、`calc(1e39px + 10%)`、巢狀的 `font-size: 1000%`、很長的 `width=` 的數字，預期值先在 Chrome 152 上跑同樣的 script（`details.md`）；用這些值的 layout 和 paint 不 panic（包括 `double` 的 border）。
- **改寫成 checked、飽和的地方**：在網頁碰得到的範圍內行為不變的（例如要 2³² 次才溢位的計數器），改寫前後的 mutation 是等價的，不逐一跑；在 `docs/test-gaps.md` 記一條「溢位的路徑只靠 lint 和審查」。行為會變的（原本會 wrap、現在回錯誤），照一般的新行為加測試、跑 mutation test。

### 分階段

1. **基礎**：`base` crate 和 `numerics`、lint 的設定、所有 crate 的根加上 `"not converted yet"` 和測試的 `cfg_attr`、`sanity.py numerics` 和它的 `NOT_CONVERTED`，`.gitlab-ci.yml` 和 `CLAUDE.md` 加上它；改寫 `dom`、`bindings`、`protocol`（有 `unsafe` 的和跨 process 的）。
2. **網頁能讓程式結束的**：第 5 節的 1、2、5、6。
3. **邊界的驗證和範圍**：第 5 節的 3、4、7。
4. **其餘的 crate 分批改寫**（第 18 條）：每批拿掉那幾個 crate 根上的 `allow`，每批一個 commit。四批，實際各約 236、334、340、235 處：繪圖（`gfx`、`raster`、`viz`、`compositor`、`compositor_paint`、`paint`）、style 和 layout（`css`、`style`、`computed_style`、`layout`、`animation`）、DOM（`dom_api`、`dom_js`、`dom_wasm`、`html`）、其他（`renderer`、`browser`、`shell`、`driver`、`net`、`dom_macros`）。最後一批刪掉 `NOT_CONVERTED` 和 `"not converted yet"` 的例外，更新第 6 節的文件。改寫由幾個平行的 agent 照 crate 或檔案分工做，會改變行為、要選做法的回報出來決定（第 19 條就是這樣來的）。

## 缺點

- **改寫的量**：2026-10-22 正式的程式裡 857 處算術、365 處轉型（`details.md`），大多是 `+= 1` 和索引；實際改了約 1250 處（第 1 段約 120、第 4 段約 1145），碰到很多檔案，和其他同時在做的 RFC 容易衝突，所以分批、每批短。
- **新的程式多一點寫法上的負擔**：每個整數運算要想一下屬於哪一類；`expect` 的理由要人審。`x += 1;` 這種複合賦值的語句在穩定版不能直接加屬性，要包成 `#[expect(...)] { x += 1; }`，讀起來囉嗦。
- **效能**：checked 多一個分支、飽和多一個 cmov；熱的迴圈（raster、layout）大多是浮點數，不受影響。改寫成 iterator 的地方通常反而少一次邊界檢查。不排量測；有疑慮的地方用 `bench.py` 的 microbenchmark 比。
- **lint 和工具擋不住的**：寫法選得對不對（到處寫 `saturating_add` 一樣通過）、`expect` 的理由對不對、lint 看不到的方法（`pow`、`abs`、`Iterator::sum`、`product`、`fetch_sub`、`Duration::from_secs_f64`）、浮點數的 NaN 和 inf（clamp 點以外）、新的信任邊界（新的 `protocol` 欄位、新的 GPU 資源、新的大小由網頁決定的配置）。這些靠文件（第 6 節）和審查；測試在 dev profile 下跑，溢位會 panic，測試踩到的才抓得到。
- **多一個 crate**：`base` 沒有依賴，編譯的成本可以忽略。

## 理由和其他做法

完整的比較在 `details.md` 的「其他做法」。

- **全部改成 `saturating_*`**：把錯的值換成另一個錯的值，上面的問題一個都解決不了，還把 bug 藏起來。不選。
- **只在信任邊界設上限，不開 lint**：處理得了找到的問題，但新的程式還是要靠審查；使用者 2026-10-22 選了 lint 加上限。
- **release 打開 overflow-checks**：Chromium 在 2025-01-23 拿掉了 Rust 的 `-Coverflow-checks=on`（效能成本比預期高、替第三方 crate 維護例外很麻煩）；lint 做完之後，沒寫明的運算只剩 `expect` 的地方，dev profile 的測試本來就檢查它們。不選（第 4 條）。
- **helper 放在 `gfx`**：`html`、`net`、`animation`、`browser` 沒有直接依賴 `gfx`；Chromium 的 gfx 是 `ui/gfx`，不是 base。不選（第 2 條）。

## 先例

- **Chromium**：C++ 的 `base/numerics`（`CheckedNumeric`、`ClampedNumeric`、`saturated_cast`、`checked_cast`、`ClampRound`），靠型別和審查，沒有 lint；layout 用飽和的 `LayoutUnit`；CSS 長度 clamp 到 `kMaxValueForCssLength`；viz 用 `ReportBadMessage` 擋掉不合法的 frame；canvas 有 `max_texture_size` 的檢查。Rust 的部分 2025-01-23 起不再打開 overflow-checks。
- **Firefox**：Rust 的 release 是否打開 overflow-checks，2026-10-22 沒有查到確定的資料。

照做：`numerics` 的名字和分類、各個上限的數值、`ReportBadMessage` 的做法。不照做：`LayoutUnit`（範圍外）；C++ 沒有的 lint 是 Rust 才有的。

## 已決定的事項

2026-10-22（使用者）：

1. **用 lint 強制，等級 `warn`，例外用 `#[expect(..., reason)]`**：開 `arithmetic_side_effects`、`cast_possible_truncation`、`cast_sign_loss`、`cast_possible_wrap`，不開 `as_conversions`；workspace 打開，還沒改的 crate 在根上 allow、改完拿掉。不選 `deny`：clippy 用 `-D warnings` 跑，兩者沒有差別，`warn` 和現有的寫法一致。不選每個 crate 改完才打開：新的 crate 預設不受管。不選 `#[allow]`：過時的例外不會被發現。
2. **helper 放在新的 `base` crate 的 `numerics` 模組**（Chromium 的 `base/numerics`）。不選 `gfx`：有的 crate 沒有依賴它，分層也不對。不選只用 std：float 轉整數的例外會散在約 230 處，序號的規則每處各寫一次。
3. **測試不套用**：lib 的根用 `cfg_attr(test, allow(...))`、整合測試的 `main.rs` 用 `allow`。不選套用：多改約 400 處，測試在 dev profile 下溢位本來就會 panic、讓測試失敗。
4. **release 不打開 overflow-checks**：Chromium 2025-01-23 撤回了同樣的做法；lint 做完之後效益很小。之後碰到 `expect` 的理由錯了、在 release 裡算錯，再考慮只對 workspace 的 crate 打開（`[profile.release.package."*"]` 排除依賴），量過 Speedometer 決定。
5. **lint 之外補上信任邊界的上限**：第 5 節的七項。
6. **CSS 長度在算出 computed value 時 clamp**（Blink 的位置和數值）：第 5 節第 4 項的四個位置，長度和百分比到 ±33554428、NaN 變 0，字級到 0 到 10000。不選 parse 時：擋不住單位換算（`3e38in` 乘上 96 是 inf）、`em` 和 `%` 的累乘、`calc()`，specified value 讀回時也和 Chrome 不同。不選 layout 和 paint 讀的時候：讀的地方很多、容易漏（double border 就是 paint 直接讀寬度），`getComputedStyle` 會讀回 inf。
7. **超過 texture 上限的 canvas 不當成 layer、畫進 tile**（Blink 的做法），上限給 renderer（值和位置見第 13 條），`canvas_layer` 判斷；wgpu 的驗證錯誤照舊 panic。不選把 canvas 切成幾個 texture：Chromium 沒有這樣做，compositor、viz、protocol 都要改，只對很大的 canvas 有好處。不選縮小 snapshot：畫面模糊，和 Chrome 不同。不選把 canvas 的大小上限降到 texture 的上限：Chrome 接受的 canvas（每邊到 65535）會失敗。不選裝 `on_uncaptured_error` 只記 log：kun 的 bug 會悄悄藏起來。
8. **表格照 Blink 的 `ColspanCellTabulator` 放 cell，不設總欄數的上限**：卡住是 kun 自己的問題（每一列走過全部欄），Blink 沒有；記憶體用量和 Chrome 一樣，collapsed border 照舊是密的。極端的 collapsed border 表格耗盡記憶體時結束的是整個程式，是「renderer panic 時整個程式結束」那一類（`docs/limitations.md`），拆 process 之後和 Chrome 一樣。不選設總欄數的上限、迴圈不改：很寬的表格畫錯，列數很多時成本仍然是列數乘以上限，沒有先例。不選 collapsed border 配置失敗時退回 separate：畫錯、沒有先例、多一條難測的路。
9. **做兩種型別，`arithmetic-side-effects-allowed` 先留空**：`base` 放 `SequenceNumber`、`AtomicSequenceNumber`（用完時 panic）；`gfx::Size` 加 `checked_area()`、`checked_byte_size()`（回 `Option<usize>`）；`clippy.toml` 的 `disallowed-methods` 加上 atomic 整數的 `fetch_add`，產生 ID 的改用 `AtomicSequenceNumber`、統計用的寫 `expect`（第 4 節）。依據是 2026-10-22 的 lint 結果：有重複規則的只有 ID 和計數器（約 44 處，加上 lint 看不到的 `fetch_add`）、寬 × 高 × 4（約 23 處）；索引和其他（約 770 處）每一處的理由不同。不選只用 std 加 helper：ID 用完時怎麼辦要寫 44 次，`fetch_add` 的 wrap 沒人管。不選另外做飽和的時間型別（Chromium 的 `TimeDelta`）和欄位私有、會 clamp 的 `gfx::Rect`：時間沒有需要飽和的輸入；`gfx::Rect` 的欄位是 `pub`，改成私有動到很多檔案，lint 也沒找到它的座標溢位。
10. **數字、角度、時間的 `calc()` 算出非有限的值時 clamp，不讓宣告無效**（Blink 和 css-values 的做法）：NaN 變 0，±inf 變成 ±`f32::MAX`，角度到 360 的倍數，再照 property 的範圍；和 CSS 長度同一個階段做（第 5 節第 4 項）。不選照舊讓宣告無效：和 Chrome 不同（`calc(1e39)` 在 Chrome 是很大的值、在 kun 是沒寫）。
11. **工具擋得住的寫成檢查，擋不住的寫進現況文件**：`.gitlab-ci.yml` 的 `sanity` job 和 `CLAUDE.md` 的檢查清單加上 `sanity.py numerics`（第 1 階段）；還沒改的 crate 列在 `sanity.py` 的 `NOT_CONVERTED`，最後一批改完時刪掉，之後 `"not converted yet"` 一律報錯；第 6 節的文件在最後一個階段更新。不選只在 RFC 裡寫規則：RFC 做完之後不再更新、其他文件不引用它，以後的人讀不到。不選讓 `"not converted yet"` 一直可以用：任何 crate 都能用一行關掉整個 crate 的 lint。

2026-10-22，第 1 段開始前（使用者）：

12. **`clippy.toml` 擋 atomic 整數的 `fetch_add`，在第 1 段打開，18 處一次處理完**：產生 ID 的改用 `AtomicSequenceNumber`，單純統計的寫 `#[expect(clippy::disallowed_methods, reason = "...")]`。`disallowed-methods` 是整個 workspace 共用的一份清單（已經有 V8 的兩個方法），用 crate 根上的 allow 分批的話，那一行也會關掉 V8 的那兩條；18 處正好是 `AtomicSequenceNumber` 要取代的，和它在同一段做。不選跟著第 4 段分批：中間的 allow 關掉 V8 的檢查，最後一批之前新的 `fetch_add` 不會被擋。

2026-10-22，第 2 段開始前（使用者）：

13. **canvas 的 texture 上限是固定的 8192**（WebGPU 保證的 `max_texture_dimension_2d`），放在 `LayerTreeSettings.max_texture_size`（實作時放在 `protocol::viz::MAX_TEXTURE_SIZE`，見第 5 節第 1 項），viz 建 device 時要求至少這麼大，達不到時照建不出 device 的路結束；改寫原本第 7 條「上限是 viz 的 device 的值，經過 `LayerTreeSettings` 給 renderer」。原因：device 在 Viz thread 的 `WgpuRenderer::new` 裡才建，renderer 已經啟動了，device 遺失之後還會換一個。不選從 `SharedImageInterface` 讀真正的上限（Chromium 的 `ContextCapabilities`）：layerize 的結果要看 GPU 什麼時候起來、有沒有遺失，device 換了要讓 layerize 重來（新的失效路徑），沒有 device 的期間仍要一個保守的值，拆成 process 之後是一次同步的查詢。代價：8193 到 16384 的 canvas 在 Apple Silicon 上本來可以當成 layer，現在畫進 tile，畫出來一樣，改變時比較慢；上限低於 8192 的 GPU 不能用。
14. **timer 的 handle 照舊共用一個編號**：`setTimeout`、`setInterval`、`requestAnimationFrame`、`requestIdleCallback` 和所有 window 共用，只照 Blink 在 `i32::MAX` 繞回 1、跳過三種還在用的。清除時照舊比對 owner，一個 window 清不掉別的 window 的。不選照 Blink 每個 context 分成三組：和溢位無關，`Timers` 要按 window 和種類分開。代價：值和 Chrome 不同（規範只要求是正的、彼此不同），同一個分頁裡的 frame（包括跨 origin 的 iframe）從編號的跳躍看得到彼此建了幾個 timer，記進 `docs/limitations.md`；跨 origin 的 iframe 本來就和主頁面共用 thread 和 event loop。

2026-10-22，第 3 段開始前（使用者）：

15. **Viz 收到不合法的 frame 時丟掉整個 frame，照舊顯示上一個，記一筆 `tracing::error!`**：驗證 renderer 送來的帶像素的 resource（大小不超過 `MAX_TEXTURE_SIZE`、像素的長度等於 checked 的寬 × 高 × 4），`create_shared_image` 照同樣的檢查、不合法時回 `None`。不選照 Chromium 的 `ReportBadMessage` 把那個 renderer 當成壞了、顯示 sad tab：renderer 現在是同一個 process 裡的 thread，Rust 和作業系統都不能安全地從外面結束一條 thread（停在任意一行，鎖和 heap 改到一半），只能送 `Shutdown` 等它自己收拾，它卡住時 Browser 也跟著等，不等的話它還在跑、拿著它的 heap 和資源；和 Browser 共用記憶體，停掉它也沒有隔離的效果。拆成 process 之後照 Chromium 結束那個 process，記進那份 RFC。
16. **視窗大小只夾 automation 的兩個入口**（WebDriver 的 Set Window Rect、CDP 的 `Browser.setWindowBounds`）：實體像素每邊不超過 `MAX_TEXTURE_SIZE`，讀回的是夾過的大小。Chrome headless 照收（2026-10-22 實測 100000 × 100000 的 `innerWidth` 是 100000），kun 的 Viz 一個 texture 放不下。不選讓 Viz 也把自己的輸出夾到 device 的上限、只畫左上角：超出的部分在各平台的樣子要另外定義和確認，不在這份的範圍。使用者把真的視窗拉得比 GPU 的上限大時 surface 的設定照樣 panic，記進 `docs/limitations.md`。
17. **`calc()` 照 css-values 4 收非有限的值**（2026-10-22，第 3 段開始後，使用者）：`infinity`、`-infinity`、`NaN` 三個常數（不分大小寫）；除以 0 得到 ±infinity 或 NaN；specified value 照 Chrome 寫成 `calc(infinity * 1px)`、`calc(NaN)`；數字太大的寫法（cssparser 給 inf）在 parse 時夾到 ±`f32::MAX`（Chrome 讀回 `3.40282e+38px`）。computed value 照第 6、10 條夾；角度照 Blink 的 `ClampAngle`（2026-10-22 在 Chrome 152 實測 `calc(infinity * 1deg)` 是 `2.86708e+15deg`，Blink 的 `kApproxDoubleInfinityAngle`），改寫第 10 條「取 f32 最接近上限的 360 的倍數」。原因：kun 的 `calc()` 原本不認得這些常數、除以 0 無效，第 10 條和測試計畫的 `calc(infinity)`、`calc(NaN)` 做不到。不選只夾 parse 得進來的值：`calc(infinity * 1px)` 照舊整條無效，和 Chrome 不同。代價：kun 的 calc 是 f32、Chrome 是 double，時間的上限是 `3.40282e+38s`（Chrome `1.79769e+308s`），記進 `docs/limitations.md`。

2026-10-22，第 4 段開始前（使用者）：

18. **第 4 段照第 5 節的四批（4a 繪圖、4b style 和 layout、4c DOM、4d 其他和收尾）做，每批在 `0074` 上 commit 一次、接著做下一批**；碰到會改變行為、要選做法的地方才停下來問；`cargo test --workspace`、fmt、clippy 在 4d 做完時跑一次，每批只跑那幾個 crate 的測試和 `sanity.py numerics`；第 2、3 段和第 4 段整段審核過後一起 merge。證明過不會溢位的地方照第 2 節的表先改寫，改寫不了的寫 `#[expect(..., reason)]`，不寫 `checked_*().expect()`：第 4 段的 crate 有熱的迴圈（raster、layout、paint），`expect` 在 release 多一個分支；第 1 段用了不少 `checked_*().expect()`，留著（`dom`、`bindings`、`protocol` 沒有這種迴圈）。
19. **`ResourceId` 和 frame token 改成 u64，用 `SequenceNumber<u64>` 發號**（2026-10-22，4a，使用者）：compositor 的 resource id、frame token 和 Viz 的 `SurfaceAggregator` 重新編的 id 原本是 u32 的 `+= 1`（Viz 那個是 `wrapping_add`），一直在 raster 的頁面大約 41 天、frame token 在 60 fps 時大約兩年用完一圈。Viz 用 resource id 當上傳過的 texture 的快取 key，繞回時新的 resource 會撞上還在用的舊號、畫出舊的 texture；headless 比 frame token 的大小。不選繞回時跳過還在用的（像 timer 的 handle）：compositor 要另外記住活著的 id，frame token 的順序照樣會錯。不選 `wrapping_add` 跳過 0（Chromium 的 `FrameTokenGenerator`）：上面兩個問題都會發生，Chromium 另外處理了比大小。代價：每個 resource 和 frame 多 4 bytes，`protocol` 的型別跟著改。

## 待決定

（無）

## 之後的可能

- 圖片解碼後的大小、tile 的總量的記憶體預算（Chromium 的 `ImageDecodeCache`、`TileManager`）。
- `LayoutUnit`：layout 換成定點數時，它的運算規則寫在型別裡、列進 `arithmetic-side-effects-allowed`。
- 只對 workspace 的 crate 打開 release 的 overflow-checks（第 4 條）。
- 用很大、很小、NaN 的數值 fuzz CSS 和 HTML 屬性的 parser。
