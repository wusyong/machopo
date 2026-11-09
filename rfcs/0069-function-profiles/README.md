# RFC 0069：和 Chrome 逐個 JS 函式比 Speedometer 的時間

- 開始日期：2026-10-21
- 狀態：✅ 已完成：2026-10-22
- 進度和驗收：`progress.md`
- 細節：`details.md`（草稿時的五個實驗：trace 的 CPU profiler、CDP 的 `Profiler`、Chrome 的符號、V8 的 `--prof`、兩種 profiler 在同一個 Chrome 裡的對照；kun 被拖慢多少；閒置和共用路徑；計時的區間和頁面的時鐘的驗證；20 個 suite 的預覽）

## 摘要

`bench.py speedometer` 原本只能說每個 suite 的各階段（script、style、layout……）kun 比 Chrome 多花多少；kun 的時間花在哪個函式由 `perf` 取樣，Chrome 沒有符號，看不到。`bench.py speedometer --profile` 讓兩邊在同一個 suite 各取一份 V8 的 `CpuProfiler` 的 profile（DevTools 用的那一個，1 ms 取樣）：Chrome 經過 CDP 的 `Profiler`，kun 經過 fork 接出的 `CpuProfiler` 和 automation pipe 的同樣的 CDP 方法。只算 runner 計時的區間，照頁面的 JS 函式對照兩邊的時間：同一個函式的 JS 兩邊一樣（同一個 V8），差的是算在它身上的 DOM 呼叫、getter、setter 和等待，所以差距就定位到頁面的哪段程式在用的平台功能。

## 動機

- **script 的差距知道總量，不知道在哪**：`docs/performance-gaps.md` 的 script 一節（2026-10-21）是 kun 比 Chrome 多 181 ms，kun 那一側的分類（bindings、DOM、root……）來自 `perf` 取樣，Chrome 只有 trace 的總量；每個 suite 差在頁面的哪一段程式，要一個一個從 kun 的取樣反推。那一節的估計還建立在「JS 本身兩邊一樣」的假設上，沒有量過。
- **試過的看得出這種對照有用**：Charts-chartjs 的 script 多 84 ms，兩邊打開 V8 的 CPU profiler 之後，一般的 chart.js 函式兩邊差不多，差在 `drawPointLegend`（canvas 的 `fill`、`stroke`，kun 每個 iteration 多一百多 ms，大部分是 main thread 在等，`perf` 的 `cycles:u` 看不到）和 `draw`（context 的 setter）（`details.md`）。
- **Chrome for Testing 沒有符號**：`perf` 取樣 Chrome 只認得出 JS；V8 自己的取樣器替 JS 函式記名字，不靠符號（`details.md`）。

目標：

- 每個 suite 在 runner 計時的區間裡，兩邊各自的 JS 函式（self time，連同算在它身上的 getter、setter、DOM 呼叫和等待），ms 一個 iteration，並列、差多少；20 個 suite 合計差最多的函式。
- 兩邊用同一個 profiler、同樣的取樣間隔和分配規則（被 inline 的函式拆回來），取樣讓兩邊慢的程度相近。

不在範圍內：

- Blink 和 kun 的原生函式的對照（要有符號的 Chromium，見「之後的可能」）；JS 以外的原生工作在 profile 裡只是 `(program)`，照舊看 trace 的各階段（`phases.md`）。
- 和 Chrome 比 DOM 成員各自的時間：getter、setter 和 Chrome 用 Fast API 呼叫的成員不是單獨的 node，算在呼叫的 JS 函式身上；只有 kun 的成員另列一表（第 4 條）。
- 自動判斷變快變慢：和其他量測一樣，平常不量，使用者要求時才跑。
- 用它量完整的 20 個 suite、寫進 `docs/performance-gaps.md`：收尾之後另外做（第 18 條）。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

```sh
python3 scripts/bench.py speedometer --profile --suites Charts-chartjs,TodoMVC-React-Redux --iterations 5
```

每個 suite 在 kun 和 Chrome 各跑一次（`--iterations` 個 iteration，兩邊輪流），兩邊都打開 `CpuProfiler`。profile 在 `target/bench/speedometer/profile/`（`.cpuprofile`，DevTools 開得了），報告是 `profile.md` 和 `profile.json`（2026-10-21，5 個 iteration）：

| Suite | 函式 | kun | Chrome | kun − Chrome |
| --- | --- | ---: | ---: | ---: |
| Charts-chartjs | `drawPointLegend /resources/charts/dist/assets/chartjs-7fd89fd7.js:1579:25` | 168.8 | 11.5 | +157.3 |
| Charts-chartjs | `draw /resources/charts/dist/assets/chartjs-7fd89fd7.js:9815:7` | 41.7 | 3.1 | +38.6 |

另外有每個 suite 和 20 個 suite 合計的 JS 函式、`(program)`、`(idle)`、GC、區間內和區間外的總和，和 kun 的 DOM 成員表。

讀法：同一個函式兩邊差很多，差的是算在它身上的平台功能（偶爾是 kun 的物件讓 V8 的最佳化退回去，例如 global 的 getter）；接著看 kun 的成員表，或用 `--sample` 看它呼叫的 DOM 成員裡面是什麼。

## 詳細設計

### 1. 取樣

- **Chrome**：tab 的 CDP session 一直掛著，`Profiler.enable`、`Profiler.setSamplingInterval`（1000 µs）、`Profiler.start`，報告送回之後 `Profiler.stop` 拿 profile（`.cpuprofile`：node、sample、`timeDeltas`）。V8 的 profiler 是每個 isolate 的，iframe 裡的 suite 也在裡面。
- **kun**：fork 的 `CpuProfiler`（`New`、`SetSamplingInterval`、`StartProfiling`、`StopProfiling` 之後 `Serialize`，第 5 條；還在取樣時先停再 `Dispose`），`bindings` 的 `V8PerIsolateData` 一次一個，`dom_api` 的 `ScriptEngine` 的開始、停止（第 6 條）。automation pipe 的 CDP `Profiler` 轉給分頁的 renderer（`RendererMsg::Profiler`），`Profiler.stop` 的回應帶著同樣格式的 `.cpuprofile`（第 2、7 條）；`enable` 和 Chrome 一樣只記下來（第 9 條）。取樣時 renderer 照 Blink 標閒置：等下一個 task 時是 `(idle)`，task 裡、不在 JS 時是 `(program)`（第 11 條）。
- **開始的時機**（`common.py` 的 `open_profiled`）：導覽之後，等頁面好了（`measure.mjs` 定義了 `__kunStart`）才開始取樣，接著頁面跑 `__kunPageClock`（下面的第 3 節），再呼叫 `__kunStart()` 讓 runner 開始：`measure.mjs` 把 `benchmarkClient.start` 包起來，等到這時才開始，並送回開始的時間，工具不算 runner 沒等的執行。suite 的程式都在 profile 開始之後才編譯（第 10 條：V8 不記 profile 開始之前執行中最佳化的程式）。
- 兩邊一樣的：取樣照實際經過的時間（main thread 在 DOM 呼叫裡等也算）；被 inline 的函式照原始碼位置拆回來；打開 profiler 時 V8 把呼叫 API 的最佳化程式換成會記下 callback 的版本，API 函式因此是單獨的 node（除了 getter、setter 和 Fast API）。取樣讓兩邊各慢約一成（第 8、9 條）。

### 2. 讀 profile

- 每個 sample 算給最內層、有網址的 JS 函式（API 函式和沒有網址的 node 併進呼叫它的 JS 函式，Chrome 的 Fast API 本來就併在那裡，兩邊才對稱）；沒有 JS 的照 `(program)`、`(garbage collector)`、`(idle)` 算，stack 上沒有有網址的 JS 函式的是 `(no JS function)`。
- 函式照「名字、網址的路徑、行、列」對照兩邊。
- kun 的 API 函式另外照名字統計（DOM 成員表）：JS 函式正下方的原生 node，穿過 kun 的 `glue.js`（沒有網址的 JS），不算 V8 的 regular expression。走共用路徑的成員共用一個 callback，在 profile 裡是同一個 node，名字是 V8 最後記下的任一個（第 12 條）。

### 3. 計時的區間

- 區間是 runner 的 mark 的：每個 test 從 `-start` 到 `-async-end`，和 `--trace`、`--sample` 一樣（第 13 條）；`measure.mjs` 把 mark 的時間（頁面的 `performance.now()`）送回。
- 頁面的時鐘放到 profile 的時鐘上（第 16 條）：`__kunPageClock` 空轉 10 ms，回傳開始和結束時的 `performance.now()`（profile 的 `pageClock`），落在它裡面的樣本限制頁面的 0 在 profile 上的範圍，取中間（第 17 條），誤差上限是範圍的一半，寫在報告裡。沒有樣本或對不上時不算那次執行。
- 樣本的時間是它和前一個之間（`timeDeltas`），跨過區間的邊界時照比例分；區間外的是 `(outside the timed windows)`（第 15 條）。

### 4. 報告

`profile.md`：20 個 suite 合計和每個 suite 的總表（JS 函式、`(program)`、`(idle)`、GC、`(no JS function)`、區間內的合計、區間外），kun 多花最多的函式（合計 30 個，每個 suite 10 個加上 Chrome 多花的 5 個），kun 的 DOM 成員表（前 30 個和呼叫它們最多的三個函式），每個 suite 的 runner 量到的時間（中位數）和區間的誤差上限。`profile.json` 也存整次執行的。`--against` 不支援（比兩個 kun 用 `--sample`）。

### 測試

- `bindings` 的 `cpu_profiles`：profile 的 JSON、跑過的函式、取樣的間隔、一次一個、isolate 走時還在取樣、閒置和忙著的順序。
- `shell` 的 `automation_pipe`：分頁的 renderer 取樣頁面的 script、錯誤、renderer 不在；等下一個 task 和畫 frame 分得開。`protocol` 的 JSON，wasm 的 engine 不能取樣。
- `speedometer_test.py`：開始的順序（`OpenProfiledTests`）；讀 profile、成員、區間、頁面的時鐘、報告（`ProfileReadingTests`，手寫的 profile）；兩個瀏覽器真的取樣假的 suite（`MeasureTests.test_profiles_of_both`，Chrome for Testing 還沒下載時跳過）。
- 區間的誤差：同時錄 trace 手動比過（`details.md` 的第 8 節，`docs/test-gaps.md`）。

### 分階段

1. **fork 接出 `CpuProfiler`，kun 寫出 profile，量 kun 被拖慢多少**：和 Chrome 差 5 個百分點以內（第 8 條）。
2. **`--profile`：兩邊取樣、讀 profile、整次執行的對照**；閒置的標記（第 11 條）。
3. **只算計時的區間**：頁面的時鐘，用同時錄的 trace 驗證（第 16、17 條）。
4. **20 個 suite 跑 1 個 iteration 的預覽**（第 18 條）：完整的量測和 `docs/performance-gaps.md` 在收尾之後。

## 缺點

- fork 多一段 C++（`CpuProfiler` 的 binding），發了 `kun-v152.2.0-14`，升級 V8 時要跟著檢查。
- 取樣讓兩邊各慢約一成，多的時間不一定平均分給每個函式；只看差得多的。打開 profiler 時被量的程式也和平常略有不同（呼叫 API 的最佳化程式換成記下 callback 的版本、開始時重編一輪），兩邊一樣。
- 以 JS 函式為單位：同一個函式差很多時，還要看 kun 的成員表或用 `--sample` 看它裡面是什麼；kun 走共用路徑的成員在成員表裡分不開（第 12 條）。
- 區間的邊界約有半個取樣間隔的誤差，每個 iteration 約 1 ms 算到相鄰的函式（第 17 條）；頁面的時鐘讓每次執行在區間外多約 10 ms。
- kun 的 profile 的 `startTime`、`endTime` 被 V8 截成 32 位元（`docs/limitations.md`）：工具只用相對的時間，和 trace 放在一起看時要補回高位。

## 理由和其他做法

`details.md` 有五個實驗的數字。

- **兩邊都用 `CpuProfiler`**（建議）：DevTools 用的同一個 profiler，被 inline 的函式拆回來，API 函式是單獨的 node；兩邊的取樣間隔、分配規則和讀法都一樣。
- **V8 的 `--prof`**：兩邊都只是 V8 的 flag，kun 和 fork 都不用改，讓兩邊慢的程度也相近；但在同一個 Chrome、同一個 suite 裡和 `CpuProfiler` 對照，按函式的分配差很多（`toRadian` 2.7 對 0.0 ms：被 inline 進 `computeDistance`，`--prof` 算給外層；React-Redux 的函式差到 1.5–2.5 倍），V8 標成閒置時又不取樣（Chrome 會標、kun 不會）。inline 由執行時的情形決定，兩邊不一定一樣，逐個函式比會失真。不選。
- **trace 的 CPU profiler**（`disabled-by-default-v8.cpu_profiler`）：同一個 `CpuProfiler`，但取樣間隔寫死在 V8 裡（100 µs），讓 Chrome 慢約 55%。不選。
- **`perf` 取樣 Chrome**：沒有符號，只認得出 JS（`--perf-basic-prof`），V8 的 builtin 和 Blink 都是位址；`cycles:u` 也看不到等待。不選。
- **有符號的 Chromium**：Chrome for Testing 沒有公開的符號；Fedora 的 `chromium`（155）有 debuginfo，但版本和設定和量的 Chrome 不同；自己照正式版的設定編要上百 GB、好幾個小時。只在要看 Blink 內部時才值得（「之後的可能」）。

## 先例

- Chrome 的 DevTools 的 Performance 面板：同樣的 `CpuProfiler`（CDP 的 `Profiler`、trace 的 `ProfileChunk`），一樣把 getter、setter 算在呼叫的函式身上。
- Node 的 `--cpu-prof`：寫出同樣格式的 `.cpuprofile`。

## 已決定的事項

2026-10-21，草稿之後（使用者，逐條審）：

1. **兩邊都用 V8 的 `CpuProfiler`，1 ms 取樣**：Chrome 經過 CDP 的 `Profiler`，kun 經過 fork 接出的 binding。兩邊同一個 profiler、同樣的取樣間隔和分配規則（被 inline 的函式拆回來），讀 profile 的程式也共用；DevTools、Node 的 `--cpu-prof` 也是它。不選 `--prof`：同一個 Chrome 裡按函式的分配和 `CpuProfiler` 差很多（inline 算給外層、閒置不取樣），兩邊的 inline 又不一定一樣。不選 trace 的 profiler：取樣間隔寫死 100 µs，讓 Chrome 慢約 55%。不選 kun 用 `perf`：`cycles:u` 看不到等待，分配規則也和 Chrome 的不同。代價是 fork 多一段 C++、發一次 V8 的 release。
2. **kun 經過 automation pipe 的 CDP `Profiler` 打開 profiler、交出 profile**：`protocol::automation` 加上 `Profiler.enable`、`setSamplingInterval`、`start`、`stop`，`automation_session` 轉給 renderer，renderer 停止後把 profile（`.cpuprofile` 的 JSON，`bindings` 轉）用 message 送回，`Profiler.stop` 的回應帶著它。`bench.py` 對兩個瀏覽器送同樣的 CDP 方法，開始和停止的時間點一樣。結構和 Chromium 一樣（DevTools 的 `Profiler` 由 renderer 裡的 V8 inspector 的 `V8ProfilerAgentImpl` 處理，經過 Browser 的 DevTools session）。不選環境變數（`MACHOPO_CPU_PROFILE`，renderer 結束時由 Browser 寫檔）：`bench.py` 要兩套程式，取樣的範圍是整個 renderer 的生命期。renderer 不碰檔案系統（「Process 的邊界」），兩種做法的 profile 都經過 message。
3. **用頁面的 `performance.now()` 對齊計時的區間**：`measure.mjs` 把 runner 的 mark（`performance.getEntriesByType("mark")`）的 `performance.now()` 連同結果送回；工具在 `Profiler.start` 之後馬上讀一次 runner 那一頁的 `performance.now()`（Chrome 用 `Runtime.evaluate`，kun 用 `Kun.executeScript`），當成 profile 開頭在頁面時鐘上的位置。誤差是一次 CDP 的往返，約 1 ms 以內。第 3 階段的原型用一次同時錄 trace 的執行驗證對齊的誤差，對不準時改用 trace 或 `console.profile`。不選同一次也錄 trace（用 mark 的時間，換算 V8 的 `CLOCK_MONOTONIC` 和 trace 的 `CLOCK_BOOTTIME`）：kun 的 trace 不能只選幾個分類，記全部的負擔比較大。不選每個區間一個 `console.profile`（DevTools 的做法，`Profiler.consoleProfileFinished`）：最準，但 kun 要做 `console.profile` 和它的事件，工作量最大。
4. **主表照 JS 函式比，另附只有 kun 的 DOM 成員表**：主表兩邊都把 API 函式的 node 併進呼叫的 JS 函式（Chrome 用 Fast API 呼叫的成員本來就沒有 node），可以相減；另一張表列 kun 的 profile 裡花最多時間的 DOM 成員和呼叫它們的 JS 函式，註明不和 Chrome 比（看得到等待，`--sample` 的 `cycles:u` 看不到）。走共用路徑的方法（overload，例如 `fill`、`stroke`）的 function 現在沒有名字，替它們設名字。不選只照 JS 函式比：每次都要另外跑 `--sample`。不選成員直接放進對照表：同一個成員 Chrome 併在呼叫者身上、kun 單獨一列，相減會誤導。

2026-10-21，第 1 段開始前（使用者）：

5. **fork 的 binding 只接開始、停止和寫成 JSON**：`CpuProfiler` 的 `New`、`SetSamplingInterval`、`StartProfiling`（記下 sample）、`StopProfiling`，停止後用 V8 的 `CpuProfile::Serialize` 寫成 `.cpuprofile` 的 JSON 字串交回（寫的方式沿用 rusty_v8 給 heap snapshot 用的 `OutputStream`）。`Serialize` 寫的欄位和 V8 的 inspector 組的 CDP profile 一樣（`callFrame`、`hitCount`、`children`、`deoptReason`、`positionTicks`、`samples`、`timeDeltas`），兩邊用同一個讀 profile 的程式。不選接出 `CpuProfile` 和每個 node 的讀取：只有要自己組別的格式時才需要（Chrome 的 inspector 組 CDP 的 profile、Blink 的 JS Self-Profiling API 組 `ProfilerTrace`），現在用不到；fork 的改動越小，升級 V8 時要檢查的越少。
6. **renderer 經過 `ScriptEngine` 碰 profiler**：`dom_api` 的 `ScriptEngine` 加上開始（取樣間隔）和停止（交回 JSON）兩個方法，預設是不支援；V8 的 engine 經過 `bindings` 實作，wasm 的不支援（automation 回錯誤）。renderer 不知道是哪個 engine，和執行 script 一樣。不選 renderer 直接呼叫 `dom_js`：renderer 不依賴 engine 的 crate。
7. **automation 和 `protocol` 的形狀照 CDP**：分頁的 session 的 `Profiler.enable`、`Profiler.setSamplingInterval { interval }`、`Profiler.start`、`Profiler.stop` → `{ profile }`；`protocol` 的 renderer message 是開始（帶間隔）、停止，和交回 profile 的 JSON。取樣的範圍是 renderer 的 isolate（iframe 也在裡面，和 Chrome 一樣）。
8. **kun 被拖慢多少的量法和門檻**：和 `details.md` 的第 2 個實驗一樣（React-Redux、Charts-chartjs，10 個 iteration，兩輪輪流，打開和不打開比中位數）。被拖慢的百分比和 Chrome（11–14%）差 5 個百分點以內才繼續第 2 段；差更多時先查是什麼讓 kun 慢。

2026-10-21，第 1 段量過之後（使用者）：

9. **`Profiler.enable` 和 Chrome 一樣只記下來，不做事**：第一次量時 kun 的 `enable` 先打開了 V8 的詳細原始碼位置（`UseDetailedSourcePositionsForProfiling`），React-Redux 被拖慢的百分比 kun 2.8%、Chrome 9.6%，超過第 8 條的門檻。拆開「打開 profiler」和「每次取樣」之後，每次取樣的成本兩邊差不多，差在打開：Chrome 多 7–15 ms，kun 3–5 ms。原因是 Chrome 的 `enable` 只記下旗標（`V8ProfilerAgentImpl::enable`），開始取樣後 V8 才替函式補原始碼位置（`NeedsDetailedOptimizedCodeLineInfo`，要重新 parse），kun 則在 `enable` 時就付掉了。照 Chrome 改了之後（`details.md`）：React-Redux Chrome +10.9%、kun +10.1%，Charts-chartjs Chrome +9.3%、kun +5.4%，差都在 5 個百分點以內；多花的毫秒數也接近（+7.2 對 +8.9、+15.6 對 +17.2）。V8 的 flag 兩邊只差兩個無關的。兩個瀏覽器都在快核（0–3、12–15）上。
10. **profile 在導覽之後、suite 載入之前開始**：profiler 開始時，V8 只記下掛在函式上的最佳化程式，執行中最佳化（OSR）、只在 feedback vector 上的不記（`ExistingCodeLogger`），那些程式的樣本會算給呼叫它的函式（實驗見 `details.md`）。Chrome 一樣。runner 的頁在 `Profiler.start` 之前已經跑了一點，它的程式可能有這個問題；suite 的程式在 iframe 載入之後才編譯，沒有。

2026-10-21，第 2 段開始前（使用者）：

11. **kun 的 renderer 照 Blink 標閒置**：取樣時，事件迴圈開始等下一個 task 之前 `Isolate::SetIdle(true)`，等到之後 `SetIdle(false)`（`ScriptEngine` 的 `set_idle`，預設不做事；開始取樣的那個 task 也先標成不閒置：V8 的 thread 一開始就是 `IDLE`）。V8 把 `IDLE` 的樣本記成 `(idle)`、`IDLE_EXTERNAL` 記成 `(program)`（`symbolizer.cc` 的 `EntryForVMState`），兩邊的 `(idle)` 都是等下一個 task，`(program)` 都是在 task 裡、不在 JS。Blink：有 DevTools 的 session 時，`WebDevToolsAgentImpl` 是 main thread 的 task observer，每個 task 之前 `ThreadDebugger::IdleFinished`、之後 `IdleStarted`（V8 inspector 的 `idleFinished`、`idleStarted` 呼叫 `SetIdle`）。rusty_v8 已經接了 `set_idle`，fork 不用改。不選不標、報表把兩邊的 `(program)` 和 `(idle)` 合在一起：看不出 rendering 和等待各佔多少。

2026-10-21，第 2 段做的時候（使用者）：

12. **kun 走共用路徑的成員在 profile 裡合成一列**：V8 照 API 函式的 callback 的位址記它的名字（`log.cc` 的 `CallbackEvent`），走生成的路的成員各有自己的 `v8_callback::<M>`，走共用路徑的（overload、可變參數、回傳 promise 的……，例如 canvas 的 `fill`、`stroke`、`isPointInPath`）共用一個 `member_callback`，在 profile 裡是同一個 node，名字是最後記下的那一個（實驗裡是空的：`glue.js` 用的原生函式沒有名字），看不出是哪個成員。成員表把 kun 沒有名字的 API node 標成「kun 的共用路徑」，並註明共用路徑的成員合在一起、名字可能是其中任一個。不選替每個共用路徑的成員生成一個轉交的小 callback：使用者要把它們改成生成的路（「之後的可能」），那時自然分開。Blink 每個成員有自己的 callback，沒有這個問題。

2026-10-21，第 3 段開始前（使用者）：

13. **計時的區間和 `--trace`、`--sample` 一樣**：runner 的 mark，每個 test 從 `-start` 到 `-async-end`（sync 加 async），同一個 `timed_windows` 的定義，三個工具算的區間一致。async 的部分有等 rAF 和 timer，區間裡的 `(idle)` 是分數裡算到的等待，留著。不選只算 sync：和 Speedometer 的分數對不起來。
14. **錨點在 `Profiler.start` 前後各讀一次 `performance.now()`，取中間；同時錄 trace 的一次執行驗證**：兩次讀之間的時間是誤差的上限，每次都記下。用 Charts-chartjs 和 React-Redux 同時錄 trace，兩個瀏覽器都比每個區間的開頭和結尾，中位數 1 ms（一個取樣間隔）以內、最大 2 ms 以內算過；超過時照第 3 條改用 trace 或 `console.profile`，回頭再決定。
15. **報告的主表只算區間內，另外一列區間外的總和**：各 suite 和全部的表都只算計時的區間，載入和 iteration 之間的時間以「區間外」一列列出，不混進比較也不會不見；`profile.json` 兩種都存。不選區間內和整次執行並排：表太寬。

2026-10-21，第 3 段做的時候（使用者）：

16. **用頁面在 profile 裡留下的一段時間對齊，取代第 14 條的前後讀數**：`Profiler.start` 之後、runner 開始之前，頁面跑一個有名字的函式 `__kunPageClock`，空轉約 10 ms，回傳開始和結束時的 `performance.now()`；profile 裡落在它裡面的第一個和最後一個樣本把頁面的時鐘放到 profile 的時鐘上，範圍是 [最後一個樣本 − 結束, 第一個樣本 − 開始]，取中間，誤差上限是範圍的一半（約一個取樣間隔的一半），每份 profile 都記下。第 14 條的前後讀數不夠：Charts-chartjs 兩次讀數之間 kun 11.5 ms、Chrome 24.5 ms（React-Redux 3.3、15.6 ms），`Profiler.start` 本身要記下已經有的程式；V8 在一開始就記 `startTime`（`CpuProfile` 的建構子），第一個樣本晚 3–9 ms，取中間會偏晚。驗證照第 14 條同時錄一次 trace。代價是每次執行在區間外多約 10 ms。不選只用 start 之前的讀數：誤差是一次往返，但從資料本身證明不了，又依賴 V8 記 `startTime` 的時機。不選改用 DevTools 的 Performance 面板（trace 的 `disabled-by-default-v8.cpu_profiler`，和 mark 同一個時鐘，不用對齊）：V8 的 `TracingCpuProfilerImpl` 把間隔寫死成 100 µs（第 1 條）。Chromium：Performance 面板的 profile 和 trace 的事件同一個時鐘，`console.profile` 在頁面裡開始和停止；這裡同樣讓頁面自己在 profile 裡留下時間的記號。

17. **頁面時鐘的範圍取中間**：和同時錄的 trace 比，六輪、兩個 suite、兩個瀏覽器，區間的開頭和結尾的誤差中位數 0.03–0.49 ms、最大 0.52 ms，第 14 條的門檻內。真正的位置每次都在範圍的下限：`__kunPageClock` 的最後一個樣本落在迴圈結束的那一刻，看起來是 V8 在 OSR 的程式 deopt 時記的樣本（`AddDeoptStack`）。取中間時每個 iteration 約 0.6–1.4 ms 算到相鄰的函式（例如 tool 自己在每個 test 之後跑的 `summarize`），取下限時 0.02–0.43 ms，總時間都幾乎不變。不選取下限：依賴迴圈結束時 V8 一定記一個樣本，是分層編譯的行為，沒有保證。不選兩個函式每 0.2 ms 交替、從設計上收緊兩端：時鐘複雜（Chrome 的 `performance.now()` 粗化到 100 µs），算錯的量已經比 profiler 自己的負擔（約一成）小得多。

2026-10-21，第 4 段開始前（使用者）：

18. **先跑 1 個 iteration 看分析，收尾之後再完整跑**：20 個 suite 先各跑 1 個 iteration，看報告能讀出什麼；RFC 收尾之後，再照「整體」的量法（每個 suite 每個瀏覽器一個新的、10 個 iteration、兩邊輪流）跑，寫進 `docs/performance-gaps.md`。
19. **performance-gaps.md 改寫「script」一節和量法**：「量法」加上 `--profile`；「script」加上照 JS 函式看的表（20 個 suite 合計、kun 多花最多的函式、kun 的 DOM 成員）和結論，舊的 Charts-chartjs 那段（trace 的 100 µs profiler）換成新的數字；「可以開 RFC 的方向」只在結果指出新的方向或改變排序時改；「怎麼重做」加上 `--profile`。其他節維持原本的日期，不重量。
20. **照現在的機器狀態跑**：kun 的 Charts-chartjs 的 canvas 等待比 10-20 長約一倍（第 1 段的 commit 也一樣，Chrome 沒變），文件裡註明，原因留給 canvas 的 `fill`、`stroke` 那個方向去查。不選先查清楚再跑：會延後這一段，其他 suite 不受影響。

## 待決定

（無）

## 之後的可能

- 共用路徑上的成員（overload、可變參數、回傳 promise 的、`glue.js` 建的、event handler 屬性）改走生成的路：除了呼叫的成本，profile 和 `perf` 的取樣也才分得開它們（第 12 條）。
- 有符號的 Chromium（照正式版的設定、`symbol_level=1`），對照 Blink 和 kun 的原生函式。
- 照函式對照的結果開優化的 RFC：20 個 suite 的預覽（`details.md` 的第 9 節）看到的有 scroll anchoring（`overflow-anchor`，Editor-TipTap）、`scrollTo(0, 0)` 不更新 layout（NewsSite-Nuxt）、canvas 的 `fill`、`stroke` 讓 main thread 等（Charts-chartjs）、jQuery 的 `innerHTML` 和 `cleanData`；完整量過之後寫進 `docs/performance-gaps.md` 的方向。
- V8 的 `CpuProfile::Serialize` 把 `startTime`、`endTime` 截成 32 位元（`profile-generator.cc` 的 `static_cast<unsigned>`）：在 fork 裡改成 64 位元（要重編 V8、發 release），並替上游寫回報的草稿。現在只用相對的時間，不影響結果；要把 kun 的 profile 和 trace 放在一起看時再做。
