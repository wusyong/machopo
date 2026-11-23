# RFC 0111 的進度：量測工具跟著 process 走

開始日期：2026-11-23。在 `0111` 上做，每個階段做完、經過審核之後 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. process 和 trace：automation pipe 的 `SystemInfo.getProcessInfo`、工具讀一次執行的每一份 trace、Speedometer 的 `--trace` 和 `--sample`、`blink-perf --gc`、量不到 renderer 時報錯 | 做完，merge 回 `main` | 2026-11-23 |
| 2. 記憶體和 `census`、`--single-process` | 做完、待審查 | 2026-11-23 |
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
| peak memory 照 process：Browser 和每個 renderer process 各自的峰值，合計是它們的和；renderer process 讀不到、版本列不出 process 時合計是空的；renderer 在 Browser 裡時合計是 Browser 的；`--against` 的舊版本照 git 的歷史判斷（`renderers_are_threads`） | `speedometer_test.py MemoryTests`（`common.memory_by_process`、`with_renderers_in_browser`、`renderers_are_threads`） |
| `compare.md` 的記憶體：合計和它的變化，照 process 的表；舊版本的合計由 Browser 的補上 | `CompareTests.test_times_and_memory_compared`、`test_without_sample_the_versions_are_timed_in_turn` |
| 真的 machopo 的記憶體：Browser 和 renderer process 都有、合計是兩者的和；`--single-process` 時只有 Browser 的 process、trace 和記憶體 | `MachopoTests.test_machopos_peak_memory_with_its_run`、`test_with_single_process_the_renderer_is_the_browsers` |
| `--single-process`：交給認得它的 build，`--against` 的舊版本不交；報告寫明每一邊的 renderer 在哪裡 | `SingleProcessTests` |
| `census`：renderer process 寫 `MACHOPO_CENSUS` 加 `.<pid>`；工具把一頁的每一份加總、其他檔案不算；renderer process 沒寫時那一頁是錯誤 | `cargo test -p base --features census census`、`cargo test -p shell --features census --test it automation_pipe::a_renderer_process_counts`、`python3 scripts/bench/census/census_test.py` |

## 做過的檢查

**第 1 階段**（2026-11-23，macOS 26.6.2，Apple M4）：

- 新的測試：`shell` 的 `automation_pipe::the_process_info_tells_the_browser_and_each_renderer`、`the_process_info_of_a_single_process`；`speedometer_test.py` 的 `RendererTraceTests`（`renderer_trace` 和 `machopo_processes` 的單元測試，`--trace` 和 `--sample` 的報錯用 mock 的 machopo）。改了的：`MachopoTests.test_the_windows_are_the_runners_times`（讀 renderer 的 trace、檢查 process 的清單）、`MeasureTests.test_traces_of_both`、`SampleTests.test_the_stacks_in_the_timed_windows`（多一份 renderer 的 trace）、`blink_perf_test.py` 的 `test_each_run_in_machopo_then_chrome_with_a_trace`（renderer 的 trace 和它沒寫時）、`GcRunTests`（counter 在 renderer 的 trace 裡）。改之前在 `9e7d93d` 上，`MachopoTests.test_the_windows_are_the_runners_times`、`MeasureTests.test_traces_of_both`、`SampleTests.test_the_stacks_in_the_timed_windows` 失敗（`details.md` 的「盤點」）。
- **找到的 bug**：讀得到 renderer 的樣本之後，`SampleTests.test_the_stacks_in_the_timed_windows` 抓到 renderer process 開著 trace 時做每一筆 `log` 的紀錄：`crates/shell/src/main.rs` 的 renderer 那條路沒有 `tracing_log::log::set_max_level`（Browser 的有）。補上之後通過（README 的「取樣」）。
- mutation test（`mutants-1.toml`，`python3 scripts/test.py mutate --keep-going`）：19 條全部抓到。`getProcessInfo` 不列 renderer、把 renderer 叫成 browser、Browser 列兩次（`automation_pipe::the_process_info`）；`renderer_trace` 不照檔名找、renderer process 沒寫 trace 不報錯、renderer 的 trace 不算自己的 process、好幾個時挑 mark 最少的、把 Browser 當成 renderer，`machopo_processes` 把找不到的方法當錯誤，上一次的 renderer trace 留著（`RendererTraceTests`）；`run_in_machopo` 不交回 process（`MachopoTests.test_the_windows_are_the_runners_times`）；`--trace` 讀 Browser 的、什麼都沒有時寫成 0，`--sample` 照 frame 猜 renderer process 的區間、收別的 process 的樣本、沒有樣本時寫成 0（`RendererTraceTests`）；`blink-perf --gc` 讀 Browser 的、renderer 沒寫 trace 不報錯（`GcReportTests`）；renderer process 不設 `log` 的上限（`SampleTests`）。
- 真的 Speedometer（TodoMVC-JavaScript-ES5，machopo 的 `perf` profile，renderer 是自己的 process）：`bench.py speedometer --trace --browser machopo` 讀 `.pftrace.800`，`phases.md` 每個 iteration 的 script 9.2、style 1.7、box tree 0.8、layout 0.6、paint 0.8、parse 4.3、GC 0.9 ms（之前每格 0.0）；`--sample` 226.1 ms 的 CPU 時間、30 個照 runner 的 mark 的計時區間（之前 0、照 frame 猜）。
- 跑過的：`cargo test -p protocol -p shell`、`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py` 全部通過（跳過的只有 Windows 和 Linux 才跑的）；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
- 階段的檢查：`cargo test --workspace` 4681 個通過，`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 沒有問題。

**第 2 階段**（2026-11-23，macOS 26.6.2，Apple M4）：

- 新的測試：`speedometer_test.py` 的 `MemoryTests.test_the_browsers_and_its_renderer_processes_summed`、`test_which_versions_renderers_are_threads`、`SingleProcessTests`、`MachopoTests.test_with_single_process_the_renderer_is_the_browsers`；`census_test.py` 的 `test_each_processs_file_of_a_page_is_summed`、`test_a_renderer_process_that_wrote_no_counts`；`base` 的 `census::tests::a_process_of_its_own_file_has_its_id_after_the_name`；`shell` 的 `automation_pipe::a_renderer_process_counts_into_a_census_of_its_own`（只在 `census` feature 開著時編）。改了的：`MachopoTests.test_machopos_peak_memory_with_its_run`（以前只要求超過 20 MB，Browser 一個 process 就超過）、`CompareTests` 的兩個（記憶體照 process）。
- 真的跑：`bench.py speedometer --against 9691a36 --runs 1 --suites TodoMVC-JavaScript-ES5,TodoMVC-jQuery`：記憶體 280 對 282 MB（+0.7%）、427 對 444 MB（+4.0%），current 的 Browser 161、162 MB，renderer 122、282 MB；against（renderer 是 thread）只有 Browser 的；`compare.md` 寫明兩邊的 renderer 在哪裡。只讀 Browser 的時候（2026-11-19 之後的比較）這兩個 suite 是 −35.4% 和 −59.7%。`bench.py census blink-perf --dirs bindings`：每一頁都有 renderer process 的 `<頁>.tsv.<pid>`，沒有一頁報 renderer 沒寫，Browser 的 process 沒有 binding 的呼叫、沒寫檔。
- mutation test（`mutants-2.toml`，`python3 scripts/test.py mutate --keep-going`）：18 條全部抓到。合計不加 renderer、讀不到的 renderer 算 0、列不出 process 的版本拿 Browser 的當合計、renderer 是 thread 時沒有合計、舊版本的合計沒有補上（兩條）、每個版本都當成 thread、照 process 的表欄位對調（`MemoryTests`、`CompareTests`）；`run_in_machopo` 的 `memory` 不是合計（`test_machopos_peak_memory_with_its_run`）；`--single-process` 沒交給 machopo（`test_with_single_process_the_renderer_is_the_browsers`）、交給不認得它的 build、舊的 `--against` 版本沒排除、報告說不認得它的版本是 `--single-process`（`SingleProcessTests`）；`census` 只讀 Browser 的、讀到旁邊的其他檔、renderer 沒寫不報錯（`census_test.py`）；`file_of` 不加 pid（`base`）、renderer process 不呼叫 `own_file`（`shell` 的 census 測試）。
- **找到的 bug**（測試的）：`blink_perf_test.py` 用 `mock.Mock` 當 `args` 呼叫 `cmd_blink_perf`，`getattr(args, "single_process", False)` 拿到的是 truthy 的 Mock，`common.SINGLE_PROCESS` 從此是真的，之後的測試都用 `--single-process` 跑（`GcRunTests` 找不到 renderer 的 trace、`CpuTests` 的指令多一個 switch）。那幾個 Mock 補上 `single_process=False`，和 argparse 給的一樣。
- **不穩定的測試**：`speedometer_test.py` 整套跑時 `MeasureTests.test_profiles_of_both` 失敗一次，在 Chrome 那一側：profile 裡沒有 suite 自己的程式碼（節點的 URL 都是空的）。單獨跑 3 次都通過。Chrome 那條路這次沒有改；同樣的失敗在 2026-10-23 就記過（`rfcs/0075-remote-debugging/progress.md` 的「不穩定的測試」），原因還沒查：假的 `list` suite 很小，Chrome 每 1 ms 取一次樣，可能一個樣本都沒落在 suite 的 JS 裡。沒有在這份 RFC 修。
- 跑過的：`speedometer_test.py`（139 個，上面那一個以外都通過）、`census_test.py`、`blink_perf_test.py`、`raster_test.py`；`cargo test -p base --features census census`、`cargo test -p shell --features census --test it automation_pipe::a_renderer_process_counts`；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
- 階段的檢查：`cargo test --workspace` 4681 個通過，`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`，和開著 `census` 的 `cargo clippy -p base -p shell --all-targets --features base/census,shell/census -- -D warnings` 沒有問題。
