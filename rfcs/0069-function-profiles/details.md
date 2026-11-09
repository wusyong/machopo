# RFC 0069 的細節：取樣 Chrome 的五個實驗

這份是 `README.md` 的補充，放支撐它的量測。結論和決定以 `README.md` 為準。

2026-10-21，Fedora 44（AMD Ryzen AI 9 HX 370），兩個瀏覽器綁在最快的 CPU（0–3、12–15）上；kun `5551510d`（`perf` profile），Chrome for Testing 152.0.7977.82。實驗的程式在 scratchpad，沒有進 repo。

## 1. trace 的 CPU profiler

兩邊的 trace 加上 `disabled-by-default-v8.cpu_profiler`（V8 的 `TracingCpuProfilerImpl`，每個 isolate 都有；kun 的 V8 有 Perfetto，所以 kun 的 trace 也有），profile 是 `Profile` 和 `ProfileChunk` 事件裡的 JSON（`cpuProfile` 的 node、sample、`timeDeltas`）。

- **讀法的坑**：每個 process 的 profile 都從 1 開始編號，事件又不在 thread 的 track 上；照 id 合在一起會把 Chrome 的幾個 process（包括工具列的 WebUI）的 node 混在一起，名字對錯，看起來像「stack 被截斷」。`Profile`（帶 `startTime`）和 `ProfileChunk` 由同一個 process 的不同 thread 寫，要照 sequence 描述的 thread 的 pid 分。
- **取樣間隔寫死**：`tracing-cpu-profiler.cc` 的 `sampling_interval_us = 100`，V8 的 flag 改不了。
- **讓 Chrome 慢的程度**（ms 一個 iteration，3 個 iteration）：React-Redux 不開 95.1、開 145.0–148.3；Charts-chartjs 不開 195.9、開 304.9–307.7（約 +55%）。只開 profiler 和 User Timing、不開其他分類也一樣，是 profiler 本身。
- **getter、setter 和 Chrome 的 Fast API 呼叫不是單獨的 node**：兩邊的 profile 裡都沒有 getter、setter 的 node；Chrome 沒有 canvas 的 `arc`、`fillText`、`measureText` 和 `setAttribute` 的 node（Fast API 或 TurboFan 直接呼叫），kun 有。
- **看到的**：Charts-chartjs 的 chart.js 函式兩邊差不多（`_resolveElementOptions` kun 27.9、Chrome 25.6 ms，3 個 iteration 加總），差在 kun 一個沒有名字的 API 函式（`drawPointLegend` 呼叫的，`perf` 的取樣看出是 `dom_api::canvas::fill` 和 `stroke`：overload，走共用的路，function 沒有名字）約 175 ms 一個 iteration；`perf` 的 `cycles:u` 只看到其中約 7 ms，其餘是 main thread 在等。

## 2. CDP 的 `Profiler`

Chrome：tab 的 session 一直掛著（`Target.attachToTarget`），`Profiler.enable`、`setSamplingInterval`、`start`，跑完 `stop` 拿 profile。V8 的 profiler 是每個 isolate 的，iframe 裡的 suite 的程式也在裡面。10 個 iteration、兩輪輪流，每格 20 個 iteration 的中位數（ms）：

| | 一般 | 只掛著 session | 1 ms | 100 µs |
| --- | ---: | ---: | ---: | ---: |
| React-Redux | 66.3 | 66.8 | 75.5（+14%） | 78.5（+18%） |
| Charts-chartjs | 169.1 | 169.4 | 186.9（+11%） | 194.6（+15%） |

- 掛著 session 不會變慢；取樣頻率多 10 倍只多慢約 4 個百分點：大部分是「開著 profiler」本身（記下每段程式），不是每次取樣。
- 1 ms 的 profile：React-Redux 約 1600 個樣本，Charts-chartjs 約 2700 個。
- kun 的 `--automation-pipe` 只有 machopodriver 要的方法（`protocol::automation`），沒有 `Profiler`；rusty_v8 只接了 `CpuProfiler::CollectSample` 和 `UseDetailedSourcePositionsForProfiling`，沒有開始、停止和讀 profile 的 API。

## 3. Chrome 的符號

- Chrome for Testing 的 `chrome` 沒有 `.symtab`，`.gnu_debuglink` 指向 `chrome.debug`（build-id `7385c0f9b7300e9c4d170af824332c65e3480fc2`）；`chrome-for-testing-public` 底下沒有它（`chrome-linux64-symbols.zip`、`chrome.debug` 都是 404），elfutils 和 Fedora 的 debuginfod 也沒有。
- frame pointer 有保留（前 4 MB 的程式有 2583 個 `push %rbp; mov %rsp,%rbp`）：`perf` 拿得到完整的 stack，只是原生的 frame 沒有名字。
- Fedora 44 的 `chromium` 是 155.0.8059.84，有 debuginfo；版本和設定（系統的函式庫、多半沒有 PGO）都和量的 Chrome 不同。

## 4. V8 的 `--prof`

兩邊都帶 `--prof --prof-sampling-interval=1000 --logfile=<檔案>`：kun 經過 `MACHOPO_V8_FLAGS`，Chrome 經過 `--js-flags`、加上 `--no-sandbox`（基準也加）。10 個 iteration、兩輪輪流，每格 20 個 iteration 的中位數（ms）：

| | 不取樣 | `--prof` 1 ms | 多 |
| --- | ---: | ---: | ---: |
| React-Redux，Chrome | 66.1 | 75.4 | +9.3（+14%） |
| React-Redux，kun | 89.5 | 98.3 | +8.8（+10%） |
| Charts-chartjs，Chrome | 169.6 | 185.3 | +15.7（+9%） |
| Charts-chartjs，kun | 322.2 | 335.1 | +12.9（+4%） |

- **log**：每個 isolate 一份（`isolate-<位址>-<pid>-…`；Chrome 每個 process 都有，跑頁面的 renderer 的最大）。`code-creation` 有 JS 函式（帶網址和行列）和 V8 的 builtin 的名字（`perf` 沒有符號時認不出 builtin），`tick` 有 pc、時間、VM 的狀態和 stack 上的返回位址。
- **VM 的狀態分不出 DOM 呼叫**：kun 的 Charts-chartjs 3229 個 tick 裡 `JS` 3009、`EXTERNAL` 25，`fill`、`stroke` 的時間幾乎都記成 `JS`；最佳化的程式經過 `CallApiCallbackOptimizedNoProfiling` 呼叫 API，不切換狀態。
- **main thread 不在 V8 裡時沒有 tick**：Charts-chartjs 的 log，Chrome 1272 個 tick 跨 3.19 s、有 37 個 20 ms 以上的空檔，kun 3229 個跨 3.98 s、2 個。
- **照最內層的 JS 函式算**（整次執行，10 個 iteration；ms 一個 iteration = tick ÷ 10）：

  | 函式 | kun | Chrome |
  | --- | ---: | ---: |
  | `drawPointLegend` chartjs-7fd89fd7.js:1579 | 141.7（第二輪 134.5） | 6.5（6.3） |
  | `draw` chartjs-7fd89fd7.js:9815 | 27.4（27.6） | 0.8（2.1） |
  | `_resolveElementOptions` chartjs-7fd89fd7.js:3810 | 13.8 | 5.8 |
  | `computeDistance` chartjs-7fd89fd7.js:14118 | 4.8 | 4.5 |
  | `prepare` chartjs-7fd89fd7.js:14133 | 4.3 | 4.8 |

  差最多的和第 1 個實驗一致：`drawPointLegend`（canvas 的 `fill`、`stroke`）和 `draw`（context 的 setter）。`computeDistance`、`prepare` 兩邊差不多；`_resolveElementOptions` kun 多一倍多，第 1 個實驗裡兩邊差不多（27.9 對 25.6 ms），原因還沒看（可能是算在它裡面的 getter，或兩種取樣的差別）。

## 5. 同一個 Chrome 裡兩種 profiler 的對照

第 2 個實驗的 CDP `CpuProfiler`（1 ms）和第 4 個實驗的 `--prof`（1 ms），同一個 Chrome、同一個 suite、各 10 個 iteration（兩次不同的執行），每個 sample 算給最內層、有網址的 JS 函式（CDP 的 API 函式和沒有網址的 node 併進呼叫它的 JS 函式），ms 一個 iteration：

| Charts-chartjs | `CpuProfiler` | `--prof` |
| --- | ---: | ---: |
| 合計 | 132.6 | 113.2 |
| `drawPointLegend` | 10.7 | 6.5 |
| `_resolveElementOptions` | 7.7 | 5.8 |
| `prepare` | 5.6 | 5.0 |
| `computeDistance` | 2.0 | 4.6 |
| `cloneIfNotShared` | 4.3 | 1.6 |
| `draw`（9815） | 3.3 | 0.8 |
| `toRadian` | 2.7 | 0.0 |
| `getMinMax` | 2.4 | 2.5 |

| TodoMVC-React-Redux | `CpuProfiler` | `--prof` |
| --- | ---: | ---: |
| 合計 | 87.8 | 77.0 |
| `rl` | 4.4 | 2.4 |
| `qr` | 2.3 | 3.0 |
| `Xn` | 2.8 | 1.1 |
| `Pu` | 2.3 | 1.4 |
| `summarize` | 2.1 | 1.9 |
| `click` | 2.0 | 2.3 |

- **inline**：`toRadian` 被 inline 進 `computeDistance` 之後，`--prof` 只看得到 `computeDistance` 的程式（2.0 + 2.7 ≈ 4.6）；`CpuProfiler` 照原始碼位置把 inline 的函式拆回來（`symbolizer.cc` 的 `GetInlineStack`）。
- **閒置**：`--prof` 在 V8 標成閒置時不取樣（`log.cc` 的 `Ticker::SampleStack`，除非 `--prof-include-idle`）；合計因此比較少。
- **打開 `CpuProfiler` 時被量的程式不同**：`is_profiling` 讓 `NoProfilingProtector` 失效，呼叫 API 的最佳化程式改用會記下 callback 的 `CallApiCallbackOptimized`（`js-call-reducer.cc`），API 函式才是單獨的 node；`--prof` 不改變產生的程式，所以 DOM 呼叫分不出來（第 4 個實驗的 VM 狀態）。
- **選 log 的坑**：Chrome 每個 process 都有 log，最大的不一定是跑頁面的 renderer（React-Redux 那次是工具列的 WebUI）；要挑記了 suite 的程式最多的那一份。

## 6. 第 1 段：kun 的 `CpuProfiler` 讓兩邊慢多少

兩邊都經過 CDP 的 `Profiler`（kun 的 automation pipe 接到 fork 的 `CpuProfiler`），先 `Page.navigate`，再 `Profiler.enable`、`setSamplingInterval`、`start`，報告送回之後 `stop`。React-Redux、Charts-chartjs，10 個 iteration，三輪輪流（第一次量兩輪），每格 iteration 的中位數（ms）。「只掛著」：session 掛著、`enable` 了、沒有 `start`；「1 s」：`start` 了，但每秒才取一次，幾乎只有打開的成本。兩個瀏覽器都綁在快核（Zen 5，0–3、12–15）上。

**第一次**：kun 的 `enable` 打開 V8 的詳細原始碼位置。

| | 只掛著 | 1 s | 1 ms |
| --- | ---: | ---: | ---: |
| React-Redux，Chrome | 65.8 | 73.0 | 73.9 |
| React-Redux，kun | 90.2 | 93.1 | 95.0 |
| Charts-chartjs，Chrome | 167.4 | 182.8 | 185.7 |
| Charts-chartjs，kun | 327.7 | 332.6 | 336.5 |

兩輪的量法（一般、只掛著、1 ms）：React-Redux Chrome 66.5、65.5、72.9，kun 92.5、89.8、95.1；Charts-chartjs Chrome 169.2、169.5、186.4，kun 319.2、323.4、339.6。被拖慢的百分比 React-Redux 差 6.8 個百分點（Chrome 9.6%、kun 2.8%），超過第 8 條的門檻。

- 每次取樣的成本（1 s 到 1 ms）兩邊差不多：Chrome +0.9、+2.9 ms，kun +1.9、+3.9 ms。
- 打開的成本（只掛著到 1 s）不同：Chrome +7.2、+15.4 ms，kun +2.9、+4.9 ms。
- 取樣的密度兩邊一樣（每 ms 0.97–0.99 個樣本），node 的數量相近（React-Redux 632–664 對 691–716，Charts-chartjs 1008–1079 對 1108–1111）。
- V8 的 flag：`--print-flag-values` 兩邊只差 `additive-safe-int-feedback`、`single-threaded-gc-in-background`，tiering 一樣。
- Linux 上 V8 的取樣 thread 不忙等（`use_precise_sampling_` 只在 Windows 上忙等），不是它佔了 CPU。
- Chrome 的 `Profiler.enable` 只記下旗標（`v8-profiler-agent-impl.cc` 的 `V8ProfilerAgentImpl::enable`），inspector 不呼叫 `UseDetailedSourcePositionsForProfiling`。開始取樣後 V8 要每個函式的原始碼位置（`Isolate::NeedsDetailedOptimizedCodeLineInfo`），沒有的要重新 parse；kun 在 `enable` 時就打開，這個成本落在「只掛著」裡。

**改成和 Chrome 一樣、`enable` 不做事之後**：

| | 只掛著 | 1 s | 1 ms | 多（1 ms） |
| --- | ---: | ---: | ---: | ---: |
| React-Redux，Chrome | 66.3 | 73.7 | 73.5 | +7.2（+10.9%） |
| React-Redux，kun | 87.9 | 93.5 | 96.8 | +8.9（+10.1%） |
| Charts-chartjs，Chrome | 168.1 | 186.6 | 183.7 | +15.6（+9.3%） |
| Charts-chartjs，kun | 317.7 | 339.2 | 334.9 | +17.2（+5.4%） |

差都在 5 個百分點以內，多花的毫秒數接近。

**兩個在 `bindings` 裡的探查**（每 500 µs 取樣）：

- **執行中最佳化的程式**：一個 busy loop 的函式，在 profiler 開始之前已經跑過（OSR），之後每一次 profile 裡它都沒有 node，樣本（約 350 個）全算給呼叫它的匿名函式；換一個新的 profiler 或沿用同一個都一樣。第一次在 profile 裡跑的那一次是對的（`same` 353 個）。V8 的 `EnumerateCompiledFunctions`：只記下掛在 `JSFunction` 上的最佳化程式，「只在 feedback vector 上的不記」。
- **inline**：hot loop 呼叫一個被 inline 的小函式，開不開詳細原始碼位置，被 inline 的函式都是自己的 node（`inlinee` 285、295 個樣本）：TurboFan 本來就留著 inline 的位置。

**閒置**：V8 的 thread 一開始就是 `IDLE`（`thread-local-top.cc`），只有 `Isolate::SetIdle` 改它。kun 沒有呼叫，不在跑 JS 的時間都是 `(idle)`：React-Redux 的 profile 裡 kun `(program)` 33–34 ms、`(idle)` 549–571 ms，Chrome 306–313、464–483 ms（整個 profile，含載入）。

## 7. 第 2 段：閒置、共用路徑和第一份報告

**Blink 在哪裡標閒置**：`core/exported/web_dev_tools_agent_impl.cc` 的 `WebDevToolsAgentImpl` 在有 DevTools 的 session 時是 main thread 的 task observer：`WillProcessTask` 呼叫 `ThreadDebugger::IdleFinished`，`DidProcessTask` 呼叫 `IdleStarted`，經過 V8 inspector 的 `idleFinished`、`idleStarted`（`v8-inspector-impl.cc`）呼叫 `Isolate::SetIdle`。V8 把 `IDLE` 的樣本記成 `(idle)`，`IDLE_EXTERNAL`（`SetIdle(false)` 之後、不在 JS）記成 `(program)`（`symbolizer.cc` 的 `EntryForVMState`）。kun 照做之後（第 11 條），自動化的測試裡畫一個 5000 個段落的 frame 是 `(program)`（208–584 個樣本，每 0.5 ms），之後等 200 ms 是 `(idle)`（399–608 個）。

**共用路徑的 callback**：一個呼叫 canvas 的 `arc`、`beginPath`、`fill`、`stroke`、`isPointInPath` 的迴圈，300 ms，每 0.5 ms 取樣：`arc` 12 個、`beginPath` 2 個樣本，各是自己的 node；`fill`、`stroke`、`isPointInPath`（overload，走共用路徑）合成一個沒有名字的 node，611 個。V8 照 callback 的位址記一個 code entry，名字是記下時那個函式的 `SharedFunctionInfo::DebugName`（`log.cc` 的 `CallbackEvent`），共用一個位址的函式互相蓋掉。JS 的 `name` 是對的（`fill.name` 是 `"fill"`），只有 profile 分不開（第 12 條）。

**第一份報告**（Charts-chartjs，3 個 iteration，整次執行，kun `f9ce9aa3`）：JS 函式 kun 203.1、Chrome 145.7 ms 一個 iteration；差最多的是 `drawPointLegend`（+87.9：kun 的共用路徑 88.8，canvas 的 `fill`、`stroke`）和 `draw`（+25.3：context 的 setter）；kun 的成員表是共用路徑 89.2、`measureText` 8.1、`arc` 7.0、`fillText` 2.5。kun 的 `(idle)` 372.0、Chrome 66.1，`(program)` 14.2 對 76.9：整次執行包括載入和 iteration 之間，第 3 段只算計時的區間之後再看。

## 8. 第 3 段：計時的區間和頁面的時鐘

**前後讀數不夠準**（第 14 條原本的做法）：`Profiler.start` 前後各讀一次頁面的 `performance.now()`，兩次之間 Charts-chartjs kun 11.5、Chrome 24.5 ms，React-Redux 3.3、15.6 ms。`Profiler.start` 要先記下已經有的程式；V8 在一開始就記 `startTime`（`profile-generator.cc` 的 `CpuProfile` 建構子），第一個樣本晚 3 ms（kun）到 9 ms（Chrome）。取中間會偏晚好幾 ms，也證明不了誤差。

**頁面的時鐘**（第 16 條）：取樣開始之後，頁面跑 `__kunPageClock`，空轉 10 ms，回傳開始和結束的 `performance.now()`。落在它裡面的每個樣本都限制「頁面的 0 在 profile 的哪裡」：範圍是 [最後一個樣本 − 結束, 第一個樣本 − 開始]，大約 0.1–1 ms 寬。

**runner 等 profile**：真的 Speedometer 在 `DOMContentLoaded` 時開始（`main.mjs` 的 `prepareUI`），等文件換成 runner 的、開始取樣、跑時鐘要好幾次往返，和它有競賽；假的 suite 在 Chrome 上就輸了（runner 在時鐘之前已經有 mark）。改成 `measure.mjs` 把 `benchmarkClient.start` 包起來，等工具呼叫 `__kunStart()` 才開始；工具也檢查第一個區間在時鐘結束之後。

**和 trace 比**：同時錄 trace（兩邊的 User Timing 的 mark 在 trace 裡），每個區間的開頭和結尾比 trace 的 mark。trace 的時鐘：Chrome 是 `CLOCK_MONOTONIC`（和 V8 一樣），kun 是 `CLOCK_BOOTTIME`（差機器睡眠的時間）。3 個 iteration，每份 9 個區間：

| 輪 | Charts-chartjs kun | Charts-chartjs Chrome | React-Redux kun | React-Redux Chrome |
| --- | --- | --- | --- | --- |
| 1 | 0.28 / 0.28 | 0.03 / 0.06 | 0.22 / 0.22 | 0.10 / 0.15 |
| 2 | 0.27 / 0.27 | 0.36 / 0.42 | 0.32 / 0.32 | 0.23 / 0.30 |
| 3 | 0.26 / 0.26 | 0.45 / 0.52 | 0.11 / 0.11 | 0.17 / 0.26 |
| 4（runner 等 profile） | 0.49 / 0.49 | 0.09 / 0.18 | 0.49 / 0.49 | 0.19 / 0.25 |
| 5（runner 等 profile） | 0.42 / 0.42 | 0.08 / 0.14 | 0.47 / 0.47 | 0.03 / 0.08 |
| 6（runner 等 profile） | 0.48 | 0.43 | 0.45 | 0.17 |

（誤差的中位數 / 最大，ms；第 6 輪只記了中位數。）都在第 14 條的門檻內（中位數 1 ms、最大 2 ms）。kun 的同一份裡每個區間的誤差幾乎一樣（差 0.01 ms 以內），Chrome 的多 0.1 ms 左右的散布：頁面的 `performance.now()` 在 Chrome 粗化到 100 µs。

**誤差總是在範圍的下限**：每次 `__kunPageClock` 的最後一個樣本都不在取樣的節奏上（kun 那份離前一個 37 µs，Chrome 0.27 ms），時間就是迴圈結束的那一刻，看起來是 OSR 的程式 deopt 時 V8 記的樣本（`cpu-profiler.cc` 的 `AddDeoptStack`）。所以真正的位置是範圍的下限，取中間偏晚約半個範圍，誤差也就約等於誤差上限。用 trace 的區間當準，第 6 輪每個 iteration 算到別的函式的時間：取中間 Charts-chartjs kun 1.12、Chrome 1.28、React-Redux kun 1.36、Chrome 0.60 ms；取下限 0.02、0.09、0.02、0.43 ms；區間內的總時間三者幾乎一樣。照第 17 條維持取中間。kun 的 tool 自己的 `summarize`（每個 test 計時結束之後取 DOM 摘要）在準的區間裡也有 0.3–1.2 ms 一個 iteration：kun 的 trace 的 mark 或樣本的時間有約 0.1 ms 的偏差，沒有再追。

**V8 把 `startTime` 截成 32 位元**：kun 的 profile 的 `startTime` 是 1.06×10⁹ µs，Chrome 的是 8.6×10¹¹（`CLOCK_MONOTONIC` 的 µs）：`CpuProfileJSONSerializer::SerializeImpl` 用 `static_cast<unsigned>` 寫 `startTime` 和 `endTime`，kun 的 profile 經過它（第 5 條），Chrome 的 inspector 自己組 profile。`timeDeltas` 是對的，`--profile` 只用 `startTime` 加上 `timeDeltas` 的相對時間；和 trace 比時把高位補回來。

**報告**（Charts-chartjs 和 React-Redux，5 個 iteration，只算計時的區間）：JS 函式 kun 365.8、Chrome 183.9 ms 一個 iteration，`(program)` 21.5 對 67.5，`(idle)` 0.0 對 22.5，區間外 78.6 對 181.6。差最多的是 Charts-chartjs 的 `drawPointLegend`（+157.3）和 `draw`（+38.6）。區間的誤差上限 0.39、0.51 ms。kun 的 Charts-chartjs 這時是 335 ms（第 1 段的實驗時約 170）：沒有 profile 也是 296 ms，和第 1 段的 commit 用 `--against` 比是 318 對 322 ms，所以是機器的狀態，不是這段的改動；慢在 canvas 的 `fill`、`stroke` 的等待（Chrome 沒變，192 ms）。

## 9. 20 個 suite 的預覽（1 個 iteration）

2026-10-21，kun `92133448`，`bench.py speedometer --profile --iterations 1`（4.5 分鐘）。只有第一個 iteration（JIT 還沒熱），每個數字的單位約 1 ms（每個樣本的時間），是看報告能讀出什麼的預覽，不是定論；完整的量測照第 18 條在收尾之後跑、寫進 `docs/performance-gaps.md`。

**對齊**：每個 suite 在 profile 的區間內的時間，和 runner 自己量的分數差 0.1–0.7 ms（TodoMVC-jQuery 430.9 對 430.8）。

**整體**（每個 iteration 的 ms，區間內）：JS 函式（連同它們呼叫的 DOM）kun 2523、Chrome 1848（+675）；`(program)` 566 對 558；`(idle)` 1 對 41（Chrome 在區間裡等 frame，kun 的 frame 不等 vsync）；GC 55 對 110；合計 3147 對 2556（+591）。差距集中在 5 個 suite：TodoMVC-jQuery +188、Editor-TipTap +155、Charts-chartjs +141、NewsSite-Nuxt +68、React-Stockcharts-SVG +52；其他 15 個在 ±35 ms 內，5 個 kun 比較快。

**DOM 呼叫裡強迫的 layout 是最大的一類**：kun 的 `getBoundingClientRect` 455 ms、`scrollTo` 73 ms、讀 `innerHeight` 的函式 50 ms，Chrome 對應的 JS 函式大多是 0–30 ms。
- Editor-TipTap：ProseMirror 用 `this.dom.style.overflowAnchor == null`（tiptap 的第 10256 行）判斷瀏覽器沒有 scroll anchoring，每次更新前後自己記住、恢復捲動位置（`storeScrollPos`、`resetScrollPos`），`resetScrollPos` 裡的 `getBoundingClientRect` 每次強迫 layout：kun 262 ms，Chrome 0（不走這條）。kun 沒有 `overflow-anchor`（記進 `docs/limitations.md`）；只加 property、不做 scroll anchoring 是騙過功能偵測。
- NewsSite-Nuxt：換頁時 `window.scrollTo` 捲到頂端，kun 每次更新 layout（73 ms，Chrome 0）。Blink 的 `LocalDOMWindow::scrollTo` 在捲到 (0, 0) 時不更新 layout（不會被夾住）。runner 自己的 `layout()` 多 36 ms，是這個 suite 真的 style 和 layout。
- React-Stockcharts-SVG：mousemove 的 handler 每次 `getBoundingClientRect`，kun 62、Chrome 21。
- NewsSite-Next：讀 `innerHeight` 的函式 kun 49.5、Chrome 0（`innerHeight` 先更新自己的 layout，`performance-gaps.md` 的方向）。

**其他**：
- TodoMVC-jQuery：`.html()` 設 `innerHTML`，kun 236、Chrome 126；它先呼叫的 `cleanData`（走過要換掉的元素）kun 65、Chrome 17。
- Charts-chartjs：canvas 的 `fill`、`stroke` 在等（`drawPointLegend` 155 ms），`measureText` 多約 25 ms。
- TodoMVC-JavaScript-ES6-Webpack-Complex-DOM 的 `querySelector` +12；TodoMVC-JavaScript-ES5 的 `DOMParser.parseFromString` 20 ms。

**報告的修正**：成員表一開始把 V8 的 regular expression（`RegExp: <script|<style|<link`）和 kun 的 `glue.js` 的函式（`method`、`scrollMethod`、`rest`……：沒有網址、有行號的 JS）當成成員。改成跳過 regular expression，穿過 `glue.js` 算它呼叫的原生 node，只在 `glue.js` 本身時標成「script without a URL」。共用路徑的 node 的名字仍然是任一個（`bytes`、`Blob`、`ImageData`、`toString`：第 12 條），Nuxt 的 `scrollTo` 穿過 glue 之後也落在它裡面。
