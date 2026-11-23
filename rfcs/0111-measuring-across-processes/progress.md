# RFC 0111 的進度：量測工具跟著 process 走

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-23。完成日期：2026-11-23（macOS 26.6.2、Windows 11、Linux：Fedora 45）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. process 和 trace：automation pipe 的 `SystemInfo.getProcessInfo`、工具讀一次執行的每一份 trace、Speedometer 的 `--trace` 和 `--sample`、`blink-perf --gc`、量不到 renderer 時報錯 | 完成 | 2026-11-23 |
| 2. 記憶體和 `census`、`--single-process` | 完成 | 2026-11-23 |
| 3. Linux 和 Windows | 完成 | 2026-11-23 |

## 驗收

工具的測試是 `python3 scripts/bench/<工具>/<工具>_test.py <類別>`（`speedometer`、`blink_perf`、`raster`、`census`）。

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| process | `SystemInfo.getProcessInfo`：Browser 先、每個分頁的 renderer process 各一筆（不同的 pid、不是 Browser 的）；`--single-process` 時只有 Browser | `cargo test -p shell --test it automation_pipe::the_process_info` |
| trace | 一次執行的 renderer 的 trace（`common.renderer_trace`）：照 `getProcessInfo` 的 pid（不是這次的 process 的不讀）；renderer process 沒寫時是錯誤；renderer 在 Browser 裡時是 Browser 的；沒有這個方法的版本照檔名；好幾個時 runner 的 mark 最多的；上一次留下的刪掉；`getProcessInfo` 找不到時是 `None`、其他錯誤照樣丟 | `speedometer_test.py RendererTraceTests` |
| trace | `--trace`：讀 renderer process 的；它沒寫、計時區間裡什麼都沒有時是錯誤，不寫 `phases.md` | 同上（`test_trace_*`，mock 的 machopo）；真的 machopo：`MeasureTests.test_traces_of_both` |
| trace | 真的 machopo 的計時區間（runner 的 mark）在 renderer 的 trace 裡，和它回報的時間一致；`run_in_machopo` 交回 process | `MachopoTests.test_the_windows_are_the_runners_times` |
| trace | `blink-perf --gc`：machopo 的 GC 讀 renderer process 的 trace，它沒寫時是錯誤 | `blink_perf_test.py GcReportTests`；真的兩個瀏覽器：`GcRunTests` |
| trace | `raster` 照舊讀 renderer 的 trace（函式搬到 `common.py`）；Browser 的 thread 在 Windows 的 trace 裡是整個名字（`VizCompositorThread`），照 15 個位元組讀 | `raster_test.py`（`ReadingTests.test_a_threads_name_is_cut_as_linux_and_macos_cut_it`；Windows 上 `RunTests`） |
| 取樣 | `--sample`：計時區間來自 renderer 的 trace；renderer process 的 trace 沒有 runner 的 mark、樣本沒有 renderer 的 pid、計時區間裡沒有樣本時是錯誤 | `speedometer_test.py RendererTraceTests`（`test_samples_*`） |
| 取樣 | 三個平台的取樣器都取到 renderer process 的樣本（macOS 的 samply、Linux 的 `perf`、Windows 經過 ETW 的 samply）；開著 trace 時 renderer process 不做 `log` 的紀錄 | `SampleTests`（Windows 上要系統管理員的終端機） |
| 記憶體 | peak memory 照 process：Browser 和每個 renderer process 各自的峰值，合計是它們的和；renderer process 讀不到、版本列不出 process 時合計是空的；renderer 在 Browser 裡時合計是 Browser 的；`--against` 的舊版本照 git 的歷史判斷（`renderers_are_threads`）；每個平台讀得到另一個 process 的峰值 | `speedometer_test.py MemoryTests` |
| 記憶體 | `compare.md` 的記憶體：合計和它的變化，照 process 的表；舊版本的合計由 Browser 的補上 | `CompareTests.test_times_and_memory_compared`、`test_without_sample_the_versions_are_timed_in_turn` |
| 記憶體 | 真的 machopo：Browser 和 renderer process 都有、合計是兩者的和；`--single-process` 時只有 Browser 的 process、trace 和記憶體 | `MachopoTests.test_machopos_peak_memory_with_its_run`、`test_with_single_process_the_renderer_is_the_browsers` |
| `--single-process` | 交給認得它的 build，`--against` 的舊版本不交；報告寫明每一邊的 renderer 在哪裡 | `SingleProcessTests` |
| `census` | renderer process 寫 `MACHOPO_CENSUS` 加 `.<pid>`；工具把一頁的每一份加總、其他檔案不算；renderer process 沒寫時那一頁是錯誤 | `cargo test -p base --features census census`、`cargo test -p shell --features census --test it automation_pipe::a_renderer_process_counts`、`census_test.py` |
| 全部 | 守著這些的測試擋得住弄壞的實作 | `python3 scripts/test.py mutate --keep-going rfcs/0111-measuring-across-processes/mutants-<階段>.toml`（`1`、`2`、`3`） |

## 做過的檢查

macOS 26.6.2（Apple M4，第 1、2 階段）、Windows 11 Home 10.0.26300（Intel Core i9，Python 3.11.9，samply 0.13.1）、Linux（Fedora 45，kernel 7.3.3，AMD Ryzen AI 9 HX 370，Python 3.14.10，`perf` 7.3.3，`perf_event_paranoid` 2），2026-11-23。branch `0111` 從 `main` 的 `9e7d93d` 開出來，第 1 階段先 merge 回 `main`，做完時 rebase 到 `45e8c7f`（RFC 0110 做完之後的 `main`）。

### 改之前

在 `9e7d93d` 上，`MachopoTests.test_the_windows_are_the_runners_times`、`MeasureTests.test_traces_of_both`、`SampleTests.test_the_stacks_in_the_timed_windows` 失敗（`details.md` 的「盤點」）：`phases.md` 每格 0.0，`--sample` 0 個樣本、照 frame 猜的區間，記憶體只有 Browser 的。

### 真的量測

| 平台 | `--trace`（TodoMVC-JavaScript-ES5，`perf` profile，每個 iteration 的 ms） | `--sample` | 記憶體 |
| --- | --- | --- | --- |
| macOS | script 9.2、style 1.7、box tree 0.8、layout 0.6、paint 0.8、parse 4.3、GC 0.9（之前每格 0.0） | 226.1 ms 的 CPU 時間、30 個照 runner 的 mark 的計時區間（之前 0、照 frame 猜） | `--against 9691a36 --runs 1`：TodoMVC-JavaScript-ES5 280 對 282 MB（+0.7%）、TodoMVC-jQuery 427 對 444 MB（+4.0%），current 的 Browser 161、162 MB，renderer 122、282 MB；只讀 Browser 的時候是 −35.4% 和 −59.7% |
| Windows | script 15.6、style 2.0、box tree 1.2、layout 0.8、paint 1.2、parse 6.1、GC 1.2 | 266.3 ms 的 CPU 時間、30 個區間，樣本是 renderer process 的；machopo 的函式由 PDB、JS 由 V8 的 ETW 命名 | 真的 machopo（dev）：Browser 706 MB（包括 GPU 驅動程式的）、renderer 38 MB |
| Linux | script 15.2、style 2.1、box tree 2.0、layout 1.0、paint 1.4、parse 6.5、GC 1.4 | 1403 個 cycles、4841 個 instructions 的樣本（IPC 3.45）、30 個區間；`perf.data` 裡最多的是 renderer process 的 `CrRendererMain`；JS 由 V8 的 perf map 命名 | `MachopoTests.test_machopos_peak_memory_with_its_run`（`VmHWM`） |

`bench.py census blink-perf --dirs bindings`（macOS、Linux）：每一頁都是 renderer process 的 `<頁>.tsv.<pid>`，沒有一頁報 renderer 沒寫，Browser 的 process 沒有 binding 的呼叫、沒寫檔；Linux 上 40 頁計到，另外 13 頁是頁面自己的錯誤（沒有 `WebGLRenderingContext`、worker 裡沒有 `XMLHttpRequest`）。

三個平台的取樣器都照預設跟著子 process：不用 attach 到 renderer 的 pid，也不用 `--renderer-cmd-prefix`。Linux 上 `perf_event_paranoid` 是 2 也取得到（工具只取 user space 的）。

### 找到的 bug（都修好了）

- **renderer process 開著 trace 時做每一筆 `log` 的紀錄**：讀得到 renderer 的樣本之後 `SampleTests` 抓到，`crates/shell/src/main.rs` 的 renderer 那條路沒有 `tracing_log::log::set_max_level`（Browser 的有）。
- **測試的 Mock 讓之後的測試都用 `--single-process` 跑**：`blink_perf_test.py` 用 `mock.Mock` 當 `args`，`getattr(args, "single_process", False)` 是 truthy 的 Mock，`common.SINGLE_PROCESS` 從此是真的；Mock 補上 `single_process=False`。
- **Windows 上第一次跑的四個**：`raster.py` 的 f-string 裡有反斜線，Python 3.12 之前 parse 不了；`raster` 讀不到 Browser 的 Viz thread（Windows 的 trace 保留整個 thread 名字，Linux 和 macOS 截成 15 個位元組），數字是 0 而不報錯；假的 runner 沒有 `iteration-start` 的 mark，第一個 test 的計時區間多 0.1 到 0.2 ms；`SampleTests` 找的函式名字在 Windows 的 inline 之後不一樣（測試改成接受 `dom_js::listeners::` 底下任一個 `call_animation_frame`）。另外 `RendererTraceTests` 的取樣測試寫到 tree 的 `target/bench/speedometer/`，新的 checkout 沒有那個目錄。
- **記憶體的測試在 Windows 上不跑**：匿名的 `mmap` 在 Windows 上不算 private bytes，改成碰過每一頁的 `bytearray`，`MemoryTests` 和 `MachopoTests` 的記憶體的檢查每個平台都跑。

### Mutation test

| spec | 條數 | 抓到 |
| --- | --- | --- |
| `mutants-1.toml` | 19 | 19 |
| `mutants-2.toml` | 18 | 18 |
| `mutants-3.toml` | 5 | 5 |

各條弄壞的是什麼寫在 spec 的名字裡：例如 `getProcessInfo` 不列 renderer、`renderer_trace` 不照檔名找、`--sample` 收別的 process 的樣本、合計不加 renderer、`census` 只讀 Browser 的、thread 的名字不截。`SampleTests` 在 Windows 上放寬的名字比對沒有跑 mutation test（要系統管理員，每一條都要人在終端機跑）：放寬的只是名字，stack 要穿過 JS 到 machopo 的那一段照舊檢查。Linux 上沒有改程式，沒有新的 mutation test。

### 不穩定的測試

`speedometer_test.py` 整套跑時 `MeasureTests.test_profiles_of_both` 在 macOS 上失敗一次，在 Chrome 那一側：profile 裡沒有 suite 自己的程式碼。單獨跑 3 次都通過，Linux 上也通過。Chrome 那條路這份沒有改；同樣的失敗在 2026-10-23 就記過（`rfcs/0075-remote-debugging/progress.md` 的「不穩定的測試」），可能是假的 `list` suite 太小、Chrome 每 1 ms 取一次樣，一個樣本都沒落在 suite 的 JS 裡。沒有在這份修。

做完時的 `cargo test --workspace` 裡，`shell` 的 `documents::idle_callbacks` 失敗一次（`first false false`，要 `first false true`）：整組並行跑時 callback 開始時期限已經過了，`timeRemaining()` 是 0，規範允許。是 RFC 0109 的盤點的 B3（測試等錯了東西，排在它的第 2 階段改測試）；單獨跑 5 次都通過，和這份的改動無關（這份沒有碰 idle callback）。

### 跑過的檢查

- 工具的測試：每個平台都跑了 `speedometer_test.py`、`blink_perf_test.py`、`raster_test.py`、`census_test.py`。Linux 上包括要 Chrome for Testing 的（152.0.7977.82）；Windows 上沒有 Chrome for Testing，那幾組跳過；`SampleTests` 在 Windows 上由使用者在系統管理員的終端機跑。
- cargo：`cargo test -p shell --test it automation_pipe::the_process_info`、`cargo test -p base --features census census`、`cargo test -p shell --features census --test it automation_pipe::a_renderer_process_counts`。
- `python3 scripts/sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs。
- 做完時（Linux，rebase 到 `45e8c7f` 之後）：`cargo test --workspace` 4690 個通過、0 個失敗、1 個 ignored（`--no-fail-fast`；第一次跑時 `shell` 的 `documents::idle_callbacks` 失敗一次，見「不穩定的測試」）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 和開著 `census` 的 `cargo clippy -p base -p shell --all-targets --features base/census,shell/census -- -D warnings` 沒有問題；四個工具的測試和 `sanity.py` 的七項通過。
