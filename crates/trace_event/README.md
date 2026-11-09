# trace_event

對應 Chromium 的 `base/trace_event`：machopo 的 span，名字照 Chrome 的 trace event。span 本身是 `tracing` 的，filter、欄位和 `shell` 的 trace layer 都和直接用 `tracing` 時一樣；差在沒開 trace 時什麼都不做。

## 為什麼不直接用 `tracing` 的 span macro

`tracing::trace_span!` 在 trace 沒收那個 level 時，照樣建一個空的 span、enter、drop，每次走到都是幾十條指令。放在每次 DOM 呼叫都會走到的地方（事件的分派、callback 的呼叫）時量得出來：`bench.py calls` 的 `dispatchEvent` 多約 85 條指令。Chrome 的 `TRACE_EVENT0` 先看 category 的 enabled flag，沒開時只有一次讀取和一個分支；這裡的 macro 也是：先看 `tracing::level_enabled!`（一次 relaxed 的讀取），開了才建 span。

## 用法

```rust
// 到 scope 結束為止（`_span` 要有名字：`let _ = …` 會馬上 drop）。
let _span = trace_event::trace_event!("Layout");
let _span = trace_event::trace_event!("raster::draw_into", w = size.width, h = size.height);

// 只包一個函式（`tracing::Span::in_scope` 的寫法）。
let drawn = trace_event::in_scope(trace_event::trace_event!("canvas::flush_shown"), || flush());

// log 的 filter 也可能收的（導覽的 commit，帶 URL）。
let _span = trace_event::info_event!("commit_navigation", %url);
```

- `trace_event!` 是 TRACE level，`info_event!` 是 INFO level，參數和 `tracing::trace_span!`、`info_span!` 一樣（名字，再來是欄位）。回傳 `Option<EnteredSpan>`，`None` 時沒有 span。
- 要提早結束就 `drop` 它；要在 span 外面做的事（例如交給 compositor 的步驟不算在 paint 裡）放在 `drop` 之後。
- **machopo 的程式不直接用 `tracing` 的 span macro 和 `#[instrument]`**：`python3 scripts/sanity.py spans` 檢查。例外只有這個 crate 和 `shell` 的 trace layer 自己的測試（它要測 `tracing` 的 span）。`tracing::trace!` 這類 event（例如 User Timing 的 mark）不受影響。

## 測試

`cargo test -p trace_event`：沒有 trace、trace 只收 INFO、收 TRACE 時各建不建 span，建了的 enter 到值 drop 為止、帶著欄位，`in_scope` 在 span 裡跑它的函式。`tracing` 先看的 level 是所有 subscriber 裡最高的（`LevelFilter::current`），subscriber 走了也不會降回來，所以這些都在同一個測試裡照順序做。trace 裡的名字和層次由 Speedometer 工具的測試（`speedometer_test.py` 的 `MeasureTests.test_traces_of_both`）守著。
