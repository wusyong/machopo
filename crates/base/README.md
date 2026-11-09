# base

對應 Chromium 的 `base/`：各個 crate 共用、和瀏覽器無關的小工具，沒有依賴。現在有整數轉型（`numerics`）和序號（`SequenceNumber`、`AtomicSequenceNumber`）。

## 整數運算寫明溢位時怎麼辦

workspace 打開 clippy 的 `arithmetic_side_effects`、`cast_possible_truncation`、`cast_sign_loss`、`cast_possible_wrap`：`+`、`-`、`*`、`/`、`%`、`<<` 這些運算子和可能丟資料的 `as` 都要改寫成寫明溢位時怎麼辦的形式。debug 下溢位會 panic，release 下悄悄 wrap，兩者都不是想要的。依用途選：

| 用途 | 寫法 | Chromium |
| --- | --- | --- |
| 大小、配置、索引的計算，跨 process 的資料 | `checked_*`，失敗時回錯誤、不畫或丟掉訊息；寬 × 高的像素數用 `gfx::Size::checked_area`、`checked_byte_size` | `CheckedNumeric`、`ReportBadMessage` |
| CSS、HTML 屬性、layout 的整數值 | 飽和或 clamp，照規範和 Blink 的上限 | `ClampedNumeric`、`ClampTo` |
| ID、序號、計數器 | `SequenceNumber`、跨 thread 的 `AtomicSequenceNumber`（用完時 panic），或那個 ID 自己明確的循環規則 | `base::AtomicSequenceNumber`、`IdType` |
| hash、亂數、checksum | `wrapping_*` | 無號數的 wrap |
| float 轉整數 | `numerics::saturated_cast`、`clamp_round`、`clamp_floor`、`clamp_ceil` | 同名的 `base::saturated_cast`、`ClampRound`…… |
| 整數轉整數 | 放得下的用 `From`；可能放不下的用 `try_from`，再決定錯了怎麼辦；規範要 clamp 的用 `saturated_cast` | `checked_cast`、`saturated_cast` |
| 證明過不會溢位 | 先試著改寫；改寫不了的 `#[expect(..., reason)]` | 審查 |

飽和不是預設：它把錯的值換成另一個錯的值，只用在規範本來就要 clamp 的地方。

- **例外**一律 `#[expect(lint, reason = "...")]`，不用 `#[allow]`：程式改了、那裡不再觸發時，`expect` 會報沒有觸發。理由寫為什麼不會溢位，而且讀的人能就近驗證，例如 `reason = "i < items.len(), so i + 1 <= len"`，不寫「不會溢位」。屬性不能放在單一運算式上，例外最小是一個 `let`、statement 或函式（`x += 1;` 這種複合賦值要包成 `#[expect(...)] { x += 1; }`）；能改寫（iterator、`split_last`、`checked_sub` 的 `?`、push 之前先記下索引）就改寫。證明過不會溢位的不寫 `checked_*().expect(...)` 代替：它在 release 多一個分支，熱的迴圈（raster、layout、paint）付不起；只有用完時就該 panic 的（版本號、elise 的 id 表的編號）才這樣寫。
- **測試不套用**：每個 lib 和 bin 的根有 `#![cfg_attr(test, allow(..., reason = "tests"))]`，每個整合測試的根（tests/ 底下的檔案、tests/it/main.rs）有 `#![allow(..., reason = "tests")]`。測試在 dev profile 下溢位本來就 panic、讓測試失敗。編進 lib 的測試工具不是 `cfg(test)`，照樣受管。
- `python3 scripts/sanity.py numerics` 檢查這些規則：lint 有打開、每個根有測試的那一行、`allow` 只在這兩種地方、其他的例外是帶理由的 `expect`。沒有 crate 能整個不受管。
- **atomic 整數的 `fetch_add`** 由 `clippy.toml` 的 `disallowed-methods` 擋：它是方法呼叫，lint 看不到它悄悄 wrap。產生 ID 的用 `AtomicSequenceNumber`；單純統計、wrap 也無妨的寫 `#[expect(clippy::disallowed_methods, reason = "...")]`。
- **lint 看不到的**要自己注意：`pow`、`abs`（`i32::MIN.abs()`）、`Iterator::sum`、`product`、atomic 的 `fetch_sub`、`Duration::from_secs_f64`（負數、NaN、太大時 panic）。lint 看得到 `Instant + Duration`（溢位時 panic）：用 `checked_add`，或寫明時間從哪裡來、為什麼不會太大。

## `numerics`

```rust
use base::numerics::{clamp_round, saturated_cast};

let x: i32 = clamp_round(offset);              // 四捨五入（.5 遠離 0），超出範圍取最近的一端，NaN 變 0
let n: u16 = saturated_cast(len);             // usize 轉 u16，超過 u16::MAX 時是 u16::MAX
```

- `saturated_cast::<T, _>(x)`：任何整數或 float 轉成任何整數（十種整數都可以是來源和目標，包括 `isize`、`usize`），超出範圍時取最近的一端，NaN 是 0。float 直接截掉小數（往 0）。
- `clamp_round`、`clamp_floor`、`clamp_ceil`：float（`f32`、`f64`）先四捨五入、往下、往上取整，再 `saturated_cast`。
- `f32_from_f64`：f64 轉成最接近的 f32（Skia 的 `SkDoubleToScalar`），超出 f32 的範圍是無限大、NaN 照舊：在 f64 裡算精度、再回到 f32 的幾何用它。不是飽和的轉換，不能有無限大的地方先夾。
- 要支援新的型別，替它實作 `SaturatedFrom<S>`。

## 序號

```rust
use base::{AtomicSequenceNumber, SequenceNumber};

static NEXT_LAYER_ID: AtomicSequenceNumber<u64> = AtomicSequenceNumber::<u64>::starting_at(1);
let id = NEXT_LAYER_ID.get_next();

let mut next_request = SequenceNumber::<u32>::new();
let request = next_request.get_next();
```

- `get_next()` 回下一個值（Chromium 的 `GetNext`；不叫 `next`，免得和 `Iterator::next` 混淆），型別裡沒有下一個值時 panic（`"sequence numbers exhausted"`）：wrap 會發出還在用的 ID。型別的最大值不會發出去。
- `SequenceNumber<T>` 在一個 thread 上用（`&mut self`），`T` 是 `u8`、`u16`、`u32`、`u64`、`usize`、`i32`、`i64`；`peek()` 看下一個值但不發出。
- `AtomicSequenceNumber<T>` 放在 `static` 裡、任何 thread 都能用（`&self`），`T` 是 `u32`、`u64`、`usize`；每個值只發一次，不保證和其他記憶體的順序（`Relaxed`）。
- 要循環的 ID（例如 timer 的 handle）不用它們，寫自己的規則。

## 測試

```sh
cargo test -p base
```
