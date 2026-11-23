# RFC 0111 的進度：量測工具跟著 process 走

開始日期：2026-11-23。在 `0111` 上做，每個階段做完、經過審核之後 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. process 和 trace：automation pipe 的 `SystemInfo.getProcessInfo`、工具讀一次執行的每一份 trace、Speedometer 的 `--trace` 和 `--sample`、`blink-perf --gc`、量不到 renderer 時報錯 | 做完、待審查 | 2026-11-23 |
| 2. 記憶體和 `census`、`--single-process` | | |
| 3. Linux 和 Windows | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `SystemInfo.getProcessInfo`：Browser 先、每個分頁的 renderer process 各一筆（不同的 pid、不是 Browser 的）；`--single-process` 時只有 Browser | `cargo test -p shell --test it automation_pipe::the_process_info` |
| 一次執行的 renderer 的 trace（`common.renderer_trace`）：照 `getProcessInfo` 的 pid（不是這次的 process 的不讀）；renderer process 沒寫時是錯誤；renderer 在 Browser 裡時是 Browser 的；沒有這個方法的版本照檔名；好幾個時 runner 的 mark 最多的；上一次留下的刪掉；`getProcessInfo` 找不到時是 `None`、其他錯誤照樣丟 | `python3 scripts/bench/speedometer/speedometer_test.py RendererTraceTests` |
| `--trace`：讀 renderer process 的；它沒寫、計時區間裡什麼都沒有時是錯誤，不寫 `phases.md` | 同上（`test_trace_*`，mock 的 machopo）；真的 machopo：`MeasureTests.test_traces_of_both` |
| `--sample`：計時區間來自 renderer 的 trace；renderer process 的 trace 沒有 runner 的 mark、樣本沒有 renderer 的 pid、計時區間裡沒有樣本時是錯誤 | 同上（`test_samples_*`）；真的 machopo 和 samply：`SampleTests` |
| 開著 trace 時 renderer process 不做 `log` 的紀錄 | `SampleTests.test_the_stacks_in_the_timed_windows` |
| 真的 machopo 的計時區間（runner 的 mark）在 renderer 的 trace 裡，和它回報的時間一致；`run_in_machopo` 交回 process | `MachopoTests.test_the_windows_are_the_runners_times` |
| `blink-perf --gc`：machopo 的 GC 讀 renderer process 的 trace，它沒寫時是錯誤 | `python3 scripts/bench/blink_perf/blink_perf_test.py GcReportTests`；真的兩個瀏覽器：`GcRunTests` |
| `raster` 照舊讀 renderer 的 trace（函式搬到 `common.py`） | `python3 scripts/bench/raster/raster_test.py` |

## 做過的檢查

**第 1 階段**（2026-11-23，macOS 26.6.2，Apple M4）：

- 新的測試：`shell` 的 `automation_pipe::the_process_info_tells_the_browser_and_each_renderer`、`the_process_info_of_a_single_process`；`speedometer_test.py` 的 `RendererTraceTests`（`renderer_trace` 和 `machopo_processes` 的單元測試，`--trace` 和 `--sample` 的報錯用 mock 的 machopo）。改了的：`MachopoTests.test_the_windows_are_the_runners_times`（讀 renderer 的 trace、檢查 process 的清單）、`MeasureTests.test_traces_of_both`、`SampleTests.test_the_stacks_in_the_timed_windows`（多一份 renderer 的 trace）、`blink_perf_test.py` 的 `test_each_run_in_machopo_then_chrome_with_a_trace`（renderer 的 trace 和它沒寫時）、`GcRunTests`（counter 在 renderer 的 trace 裡）。改之前在 `9e7d93d` 上，`MachopoTests.test_the_windows_are_the_runners_times`、`MeasureTests.test_traces_of_both`、`SampleTests.test_the_stacks_in_the_timed_windows` 失敗（`details.md` 的「盤點」）。
- **找到的 bug**：讀得到 renderer 的樣本之後，`SampleTests.test_the_stacks_in_the_timed_windows` 抓到 renderer process 開著 trace 時做每一筆 `log` 的紀錄：`crates/shell/src/main.rs` 的 renderer 那條路沒有 `tracing_log::log::set_max_level`（Browser 的有）。補上之後通過（README 的「取樣」）。
- mutation test（`mutants-1.toml`，`python3 scripts/test.py mutate --keep-going`）：19 條全部抓到。`getProcessInfo` 不列 renderer、把 renderer 叫成 browser、Browser 列兩次（`automation_pipe::the_process_info`）；`renderer_trace` 不照檔名找、renderer process 沒寫 trace 不報錯、renderer 的 trace 不算自己的 process、好幾個時挑 mark 最少的、把 Browser 當成 renderer，`machopo_processes` 把找不到的方法當錯誤，上一次的 renderer trace 留著（`RendererTraceTests`）；`run_in_machopo` 不交回 process（`MachopoTests.test_the_windows_are_the_runners_times`）；`--trace` 讀 Browser 的、什麼都沒有時寫成 0，`--sample` 照 frame 猜 renderer process 的區間、收別的 process 的樣本、沒有樣本時寫成 0（`RendererTraceTests`）；`blink-perf --gc` 讀 Browser 的、renderer 沒寫 trace 不報錯（`GcReportTests`）；renderer process 不設 `log` 的上限（`SampleTests`）。
- 真的 Speedometer（TodoMVC-JavaScript-ES5，machopo 的 `perf` profile，renderer 是自己的 process）：`bench.py speedometer --trace --browser machopo` 讀 `.pftrace.800`，`phases.md` 每個 iteration 的 script 9.2、style 1.7、box tree 0.8、layout 0.6、paint 0.8、parse 4.3、GC 0.9 ms（之前每格 0.0）；`--sample` 226.1 ms 的 CPU 時間、30 個照 runner 的 mark 的計時區間（之前 0、照 frame 猜）。
- 跑過的：`cargo test -p protocol -p shell`、`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py` 全部通過（跳過的只有 Windows 和 Linux 才跑的）；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
- 階段的檢查：`cargo test --workspace` 4681 個通過，`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 沒有問題。
