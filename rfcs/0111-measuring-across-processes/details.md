# RFC 0111 的細節

## 盤點

每個量測工具在 renderer 是自己的 process 時，2026-11-23 在 macOS 上看的。machopo `9e7d93d`（`main`），macOS 26.6.2、Apple M4，`perf` profile。renderer 預設是每個分頁一個 process（`0f36205` 起）；`MACHOPO_TRACE` 的 trace 一個 process 一份，renderer 的是 Browser 的檔名加 `.<pid>`（`crates/shell/src/main.rs` 的 `main`）。

| 工具、模式 | 怎麼量 | 在 multi-process 下 | 看到的 |
| --- | --- | --- | --- |
| `bench.py speedometer` 的時間 | 頁面裡的 runner 計時、`POST` 回 server | 正常 | 和 11-19 的數字相近 |
| `speedometer --profile` | automation pipe 的 CDP `Profiler`（V8 的 `CpuProfiler`） | 正常 | `profile.md` 有 machopo 的 JS 函式（TodoMVC-JavaScript-ES5，timed 22.3 ms） |
| `speedometer --trace` 和 `phases.md` | 讀 `<suite>-machopo.pftrace` 一份 | 壞了，沒有警告 | renderer 的 `.pftrace.89608`（1.8 MB）寫了沒讀；`phases.md` 每格 0.0、「沒有對到的 slice：none」 |
| `speedometer --sample` | 照 trace 的計時區間挑 `CrRendererMain` 的樣本 | 壞了，沒有警告 | current 的 CPU 時間 0、計時區間「by the runner's frames」；`compare.md` 寫 −100.0% |
| `speedometer` 的 peak memory（`--against`） | `peak_memory(machopo.process.pid)`：Browser 的 pid | 壞了，沒有警告 | 每個 suite 少了 26.8% 到 59.7%，看起來像是變少了 |
| `blink-perf --gc` | `trace_gc(path)` 讀一份 trace，只算 runner 的 thread 所在的 process | 照程式看會壞（沒有跑） | GC 在 renderer 的那份裡 |
| `census` | `base::census` 每個 process 把整張表寫到 `MACHOPO_CENSUS`（寫到 `.partial` 再 rename） | 照程式看會壞（沒有跑） | Browser 和 renderer 寫同一個路徑，後寫的蓋掉先寫的 |
| `raster` | `renderer_traces(trace)` 讀 `<trace>.<pid>` | 正常 | 已經跟著 process 走，也有 `--single-process` |
| `calls`、`gc` | 測試 binary 和 `gc_bench`，在自己的 process 裡 | 不受影響 | |

**samply 跟著子 process**（macOS）：2026-11-23 的 `--against 9cd286d --sample` 留下的 current 的 profile（`target/bench/speedometer/compare/current-1/`，`9e7d93d`）有兩個 process：Editor-TipTap 是 77355（Browser，12 條 thread）和 77356（16 條，有 `CrRendererMain`），77356 正是 renderer 的 trace 的檔名（`Editor-TipTap-machopo.pftrace.77356`）；TodoMVC-JavaScript-ES6-Webpack-Complex-DOM 是 77259 和 77260，一樣。`speedometer.read_samples(profile, "CrRendererMain")` 已經只挑到 renderer 的 pid，Editor-TipTap 的 1006 個樣本裡 824 個有 JS 的 frame、有名字（`JS:^click …/benchmark-runner.mjs:135:10` 這類：V8 在 renderer process 裡寫的 `/tmp/perf-<pid>.map` 讀到了），讀完之後 `/tmp` 沒有留下 perf map。所以 `--sample` 缺的只是計時區間（在 renderer 的 trace 裡）。

**為什麼沒有被發現**：

- 工具的測試（`scripts/bench/speedometer/speedometer_test.py` 等）會抓到大部分，但 `docs/testing.md` 寫它們「不在 CI 裡」，renderer 的預設改了時沒有重跑。2026-11-23 在 `9e7d93d` 上跑其中和 trace、取樣、記憶體有關的幾個：`MachopoTests.test_the_windows_are_the_runners_times`（`KeyError: 'CrRendererMain'`）、`MeasureTests.test_traces_of_both`（`phases.json` 沒有 machopo 的 `script`）、`SampleTests.test_the_stacks_in_the_timed_windows`（計時區間裡沒有 stack）失敗；`MachopoTests.test_machopos_peak_memory_with_its_run` 通過：它只要求超過 20 MB，Browser 一個 process 就超過了。
- 壞掉的樣子都像正常的結果：0、照 frame 猜的區間（工具本來就為沒有 User Timing 的舊版本留了這條路）、「記憶體少了一半」。
- 發現的經過：2026-11-23 為了查 Speedometer 的兩個退步用 `--against --sample`，current 那一邊是 0，改在 RFC 0004 之前的兩個 commit 之間取樣才查下去（`rfcs/0012-speedometer/progress.md` 的 2026-11-23）。

## Chromium 怎麼量多 process 的瀏覽器

2026-11-23 讀的原始碼和文件（HEAD）：

- **trace**：桌面版的 tracing service 是一個獨立的 service process（`content/browser/tracing/tracing_service_controller.cc` 的 `TracingServiceController::GetService()`；只有 Android 和 CastOS 在 process 內，`kTracingServiceInProcess`），每個 process（Browser 也是）都是它的 producer，各自交一塊共用記憶體給它（`services/tracing/perfetto/producer_host.cc`；Perfetto 的模型：每個 producer 一塊，預設 256 KB，<https://perfetto.dev/docs/concepts/service-model>）。CDP 的 `Tracing.end` 之後用 `dataCollected` 或 stream 交回一份 trace；從 browser target 錄的沒有 process 的篩選，從 page target 錄的只有那個頁面的 renderer（可信任時加 Browser 和 GPU 的）（`content/browser/devtools/protocol/tracing_handler.cc`）。每個 process 的 track descriptor 有 `ProcessDescriptor`（pid、名稱）和 `ChromeProcessDescriptor` 的 `process_type`（Perfetto 的 `protos/perfetto/trace/track_event/chrome_process_descriptor.proto`）。
- **記憶體**：Telemetry 的 memory metric（catapult 的 `tracing/tracing/metrics/system_health/memory_metric.html`）的值叫 `memory:chrome:{browser_process,renderer_processes,gpu_process,…,all_processes}:…`：每一次 global dump 裡照 process 的種類加總，`all_processes` 是全部的和；`reported_by_os` 底下有 `resident_size`、`peak_resident_size`、`private_footprint_size`。值取自 dump 的時間點（每個 metric 是 N 次 dump 的分布），沒有整次執行的峰值（OS 自己的 peak RSS 和 allocator 的 `peak_size` 除外）。private footprint：macOS 是 `phys_footprint` 減掉共用的 resident，Linux 是 rss_anon 加 swap，Windows 是 private bytes（`services/resource_coordinator/memory_instrumentation/queued_request_dispatcher.cc` 的 `CalculatePrivateFootprintKb`）。
- **process 的清單**：CDP 的 `SystemInfo.getProcessInfo`（experimental）回傳 `{type, id, cpuTime}` 的陣列，`type` 是 `browser`、`renderer` 或其他子 process 的 `metrics_name`（GPU 等），只在 browser target 上（`content/browser/devtools/protocol/system_info_handler.cc`）。
- **取樣**：Chromium 的 <https://chromium.googlesource.com/chromium/src/+/HEAD/docs/linux/profiling.md> 只寫 `perf record -g -p <pid>`（attach 到已經在跑的 process）。<https://chromium.googlesource.com/chromium/src/+/HEAD/docs/profiling.md> 寫從 task manager 找 pid 再 `perf record -g -p <pid>`，或 `--renderer-cmd-prefix="tools/profiling/linux-perf-renderer-cmd.sh"` 讓每個 renderer 各寫一份 `perf.data`；`--single-process` 只出現在 valgrind 那一節（「用 single-process 是為了簡單地 profile renderer；應該可以在 multi-process 下 attach 到 renderer」）。沒有找到說 single-process 不具代表性的文字。
- **Speedometer**：Telemetry 的 `tools/perf/benchmarks/speedometer3.py` 只加可選的旗標（RCS、`V8VmFuture`、`--predictable`、`--disable-field-trial-config`），crossbench 的 Chromium 預設旗標（`crossbench/browsers/chromium_based/chromium_based.py`）也沒有改 process 模型的；`--single-process` 只在已知旗標的清單和測試裡。所以兩者都在預設的 multi-process 下跑（從沒有那個旗標推論，沒有文件明寫）。
