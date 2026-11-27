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

## 不可信的輸入走到的地方寫明為什麼不 panic

`browser`、`viz`、`net` 三個 crate 的根和 `protocol` 的 `viz.rs` 打開 clippy 的 `unwrap_used`、`expect_used`、`indexing_slicing`、`panic`、`unreachable`：renderer 送的訊息、網路上的回應在 Browser 的 process 裡走到它們，那裡的 panic 結束所有的分頁。renderer 這一側不打開（panic 只是那個分頁的 sad tab：崩潰得早比默默畫錯好）。

- 能自然改寫成不 panic 的就改寫：`get(..)` 加上明確的處理（一個不可能的分支照壞訊息處理、或不畫那一個），鎖被 poison 時 `unwrap_or_else(|e| e.into_inner())`（拿著鎖的 thread panic 過，資料照樣是一個 map）。
- 改寫不了的寫 `#[expect(lint, reason = "...")]`，理由寫那個不變式為什麼成立、renderer 或網路為什麼破不了，例如 `reason = "a history navigation's index was checked against entries.len() above"`。和整數運算一樣：不用 `#[allow]`，屬性最小放在 `let`、statement 或函式上。
- 測試不套用：三個 crate 的根的 `cfg_attr(test, allow(...))` 包括這五個；`net` 的 `test_server` 是給測試用的 server（machopo 從不啟動它），整個模組 `#![expect(..., reason = "a test's server")]`。
- 新的 crate 或模組處理 Browser 的 process 收到的不可信的輸入時，在它的根打開同樣的五個。

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

## `sequence_manager`

一條 thread 的 task queue（Chromium 的 `base::sequence_manager::SequenceManager`，連同它的 `ThreadController` 的等待）：任何 thread 都能 post，擁有它的那一條 thread 一次取一個，取它要的那幾個 queue 裡最早 post 的。每條跑 task 的 thread 一份：renderer 的 main thread、每個 dedicated worker。

```rust
struct MySpec;
impl Spec for MySpec {
    type Queue = MyQueue;     // queue 的種類
    type Task = MyTask;
    type Reply = MyReply;     // 這條 thread 同步等的回答
    type Extra = MyExtra;     // 使用的人放在同一把鎖底下的狀態
    const QUEUES: &'static [MyQueue] = &[/* 每一種 */];
    fn index(queue: MyQueue) -> usize { /* 在 QUEUES 裡的位置 */ }
    fn taken(extra: &mut MyExtra, task: &MyTask) { /* 可以不寫 */ }
}

let manager = SequenceManager::<MySpec>::default();
manager.post(queue, task)?;                 // 任何 thread
manager.post_at(queue, task, at)?;          // 到期時才排進去
let task = manager.next(|q| wanted(q), timeout); // 擁有它的 thread
```

- **順序**：排進 queue 時編號，取要的那幾個 queue 裡號碼最小的。`post_at` 的 task 到期之後，在**取下一個 task 的時候**才排進 queue、拿號碼（不是到期的當下）：一個 task 跑過了 timer 的時間、期間 post 的 task 排在 timer 前面（Chromium 也是在挑下一個時才搬）。同時到期的照 post 的順序。
- **等待**：`next` 等到有 task、要的 queue 的延後 task 到期，或 `timeout`；`try_next` 不等；`try_next_by` 也不等，照給的時刻算哪些延後的 task 到期了（測試自己的時鐘，Chromium 的 `TaskEnvironment` 的 mock time：`dom` 的 `TaskTokens` 用）；`wait_for` 只回答最早的在哪個 queue、不取，也不看延後的 task（只給沒有延後 task 的 queue 用）。
- **在同一把鎖底下做的**：`post_with` 照使用的人的狀態決定 post 什麼；`Spec::taken` 在 task 被取走的同時告訴使用的人（例如「同一種只排一個」的紀錄要在取走時清掉，分開做的話別的 thread 剛好在中間檢查，會以為還在而不再 post）；`with_extra` 讀寫那份狀態；`take_matching` 照順序拿出一個 queue 裡符合的、其餘的順序不變；`put_back` 放回最前面。
- **同步等回答**（mojo 的 `[Sync]` 呼叫）：`sync_wait(id, give_up)` 只等那個編號的回答，task 都留在 queue 裡；thread 不在了、或 `give_up` 照使用的人的狀態說放棄時回 `None`。`post_reply` 交回答。
- **`close`**：擁有它的 thread 不在了：queue 清掉，之後 post 回 `Closed`。

## `census`

binding 呼叫的計數（`census` feature，預設關）：`count(Kind)` 記這個 thread 多做了一個 scope、root 以外的東西，`Call::enter` 到它 drop 是一個 callback 的一次執行，`label` 給正在跑的 callback 標上成員的名字。每個 callback 的那一列只算它自己做的（它裡面呼叫的 callback 做的算它們自己的），寫到 `MACHOPO_CENSUS` 指定的檔案。root 的數目由呼叫的一方給（`Call::enter` 的 `roots`）：這個 crate 沒有相依，拿不到 heap 的計數。沒開 feature 時每個函式都是空的，呼叫的地方不用寫 `cfg`。

## 測試

`sequence_manager` 的單元測試用兩個 queue 的小 `Spec`：順序、延後的 task 取的時候才排、照給的時刻取、等待、同步等回答、`take_matching` 和 `put_back`、`post_with`、`close`。

```sh
cargo test -p base
```
