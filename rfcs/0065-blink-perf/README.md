# RFC 0065：用 blink_perf 和 Chrome 比

- 開始日期：2026-10-20
- 狀態：✅ 已完成：2026-10-20
- 進度和驗收：`progress.md`
- 細節：`details.md`（`perf_tests/` 的盤點、取樣跑的 24 頁、`runner.js` 怎麼量、Chromium 怎麼跑它）

## 摘要

Chromium 的 `third_party/blink/perf_tests/`（Telemetry 的 `blink_perf.*`）是按領域分的微基準：一頁只量一種操作（`getElementsByTagName`、事件分派、改 `className`、parse 一段 HTML……），用 `perf_tests/resources/runner.js`（`PerfTestRunner`）跑幾十次、取時間或每秒次數。這份 RFC 加了 `bench.py blink-perf`：把這些頁面在 kun 和 Chrome for Testing 裡各跑一次，每頁比對兩邊的結果、確認兩邊做了一樣的事，寫出按目錄分的比較表；`--against REV` 改成比 kun 的兩個版本。預設只跑一份明確列出、照領域分段的頁面清單（從 10 頁開始），量的時候可以只量一個領域；kun 補上功能、頁面跑得穩之後再一頁一頁加進去。Linux 上兩個瀏覽器綁在最快的那組 CPU 上（Speedometer 也一樣）。和 `bench.py speedometer` 共用開瀏覽器、server、摘要、`--against` 的 build 這些部分（`scripts/bench/` 底下的 `common.py`、`summary.mjs`）。

## 動機

- **Speedometer 的比值是平均過的**：2026-10-20 的量測，kun 和 Chrome 的比值約 1.74–1.9（開不開視窗、怎麼算不同），但它看不出是哪一種操作慢。RFC 0063 開始前 ES6-Webpack 慢 4 倍，追下去才知道是 `querySelector` 從頭走到尾；這種事每次都要靠取樣和 trace 從一整個 suite 裡挖出來。
- **blink_perf 直接指出單一操作**：取樣跑的 24 頁裡（`details.md` 的「取樣」），`svg/SvgHitTesting` 慢 52 倍、`bindings/get-elements-by-tag-name` 17 倍、`events/EventsDispatching` 15 倍、`dom/modify-element-classname` 8 倍、`css/PseudoClassSelectors` 7 倍；layout、paint 的幾頁在 1–1.6 倍。這些在 Speedometer 裡被平均掉了。Chrome 團隊用同一組頁面守 Blink 的效能回退，頁面的寫法和 Blink 的熱路徑對得上。
- **頁面夠多**：594 頁裡 488 頁不靠 Telemetry 就能跑，和 kun 現在的範圍相關的 457 頁（`details.md` 的「盤點」估 424，沒算子目錄裡的）。
- **要先確認兩邊做了一樣的事**：blink_perf 自己不檢查結果。取樣裡 `editing/textarea-dom` 顯示 kun 快 100 倍，幾乎一定是 kun 少做了。沒有檢查，這種頁面會讓比較表誤導人。

目標：

- `bench.py blink-perf` 在 kun 和 Chrome 裡跑清單上的頁面（或其中一個領域、或指定的頁面），每頁兩邊的中位數、比值、有不有效和原因；按目錄的幾何平均。
- 頁面清單照 kun 的功能一步步擴增：清單外的頁也能指定來跑，看哪些可以加進清單。
- 有效的檢查：兩邊都跑完、沒有例外，而且做了一樣的事。
- `--against REV`：kun 的兩個版本輪流比（和 `bench.py speedometer --against` 一樣），優化時用。
- 固定 `perf_tests/` 的版本，換版本時才有得比。

不在範圍內：

- 靠 tracing 量的頁面（要 Telemetry 開 trace）和用 Chromium 專用 API（`internals`、`testRunner`、`GCController`、`gpuBenchmarking`、`gc()`）的頁面：排除，報告裡列出。
- kun 還沒有的功能的頁（`webgl`、`webgpu`、`webaudio`、`webcodecs` 的目錄，和其他目錄裡用到 kun 還沒有的 API 的頁）：不在清單上，指名時照跑；補上功能的 RFC 把它們加進清單。
- MotionMark（`perf_tests/MotionMark/`）、`speedometer21`：前者和 GPU 有關，之後另外看；後者已經有 Speedometer 3.1。
- 開視窗量（`--window`，第 16 條）、放進 CI、自動抓效能回退：之後再看（「之後的可能」）。
- 修 kun 跑不起來的頁面：這份只做工具；報告列出原因，修在各自的 RFC。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

```sh
python3 scripts/bench.py blink-perf                     # 清單上的頁，kun 和 Chrome 輪流，每頁一個新的瀏覽器
python3 scripts/bench.py blink-perf dom/ layout/        # 只跑清單上這些路徑底下的頁
python3 scripts/bench.py blink-perf --dirs layout,dom   # 這些目錄的每一頁（清單外的也跑）：看哪些可以加進清單
python3 scripts/bench.py blink-perf --pages bindings/get-elements-by-tag-name.html
python3 scripts/bench.py blink-perf --against main      # kun 的兩個版本輪流（只有 kun）
```

結果在 `target/bench/blink_perf/`：

- `report.md`：清單上、這次無效的頁在最前面（回退）；每個目錄一張有效的頁的表（頁、單位、kun、Chrome、kun 慢幾倍、要人工看的），目錄的幾何平均和全部的幾何平均（不算可疑的）；無效的頁和原因（kun 丟了例外、少做了事、逾時，Chrome 那一側的問題）；排除的頁和原因。
- `runs.json`：每頁每個瀏覽器交回的東西（數值、單位、runner 最後幾行的 log、document 的摘要）。
- `--against` 時是 `compare.md` 和 `compare.json`。

比值一律寫成「kun 慢幾倍」：單位是 ms 的（`measureTime`）是 kun ÷ Chrome，runs/s 的（`measureRunsPerSecond`）是 Chrome ÷ kun。

## 詳細設計

### 1. `perf_tests/` 從哪裡來（第 3 條）

固定在某個 Chromium 的 commit，和 `../third_party/` 的其他專案一樣：`third_party.toml` 的 `[blink_perf]` 記那個 commit 和 `archive = "third_party/blink/perf_tests"`，`ci.py third-party` 用 gitiles 的 `+archive` 下載這一個目錄的 tarball（約 30 MB）到 `../third_party/blink_perf/`，寫一個 `.kun-rev` 記下 commit；commit 換了時下載到旁邊的新資料夾再換過去，沒有 `.kun-rev` 的資料夾（不是它下載的）不動、說明原因。不 clone Chromium（太大）。

### 2. 怎麼跑

- **server**：`blink_perf.py` 的 `Server`（繼承 `common.py` 的），根目錄是 `perf_tests/`。送出 `resources/runner.js` 時在後面接 `summary.mjs` 的 `summarize`（包成 `__kunSummarize`）和 `hook.js`。摘要的函式直接放在 runner 的 script 裡，不用 `import()`：kun 在 `dom/range-select-node` 上，頁面開頭的 `import()` 在 20 輪跑完之後再過 285 秒才 resolve（`docs/limitations.md` 的「Script」）。其他檔案照副檔名給 MIME type（`.wasm` 是 `application/wasm`）。
- **hook**：`PerfTestRunner.logStatistics` 寫 `Time:` 的統計時，把每次的數值、單位和 document 的摘要 POST 回來；`logFatalError`、`error`、`unhandledrejection` 也 POST（只送第一個）。`runner.js` 的 `logFatalError` 會先呼叫 `finish`（寫出手上的統計）再返回，所以 hook 先記下 fatal，`logStatistics` 看到 fatal 時送錯誤。每次 POST 都帶 runner 最後 10 行的 log：kun 缺 API 時，原因在那裡（`atob is not defined`）。摘要不算 runner 寫在頁面上的 log（`#log` 的文字，兩邊的時間不同）；摘要丟例外時照樣送數值，帶著原因。
- **次數**：照 `runner.js` 沒有 `testRunner` 時的預設（每頁 20 次，第一次是熱身不算；頁面自己指定 `iterationCount` 的照它）。Telemetry 有 `testRunner`，是 5 次；這裡沒有，20 次的中位數比較穩。
- **瀏覽器**：每頁 kun 和 Chrome 輪流，各開一個新的（第 6 條）；kun 是 `perf` profile 的 `--automation-pipe`，Chrome 是 headless（第 7 條）。每頁有時限（預設 90 秒，`--timeout`，第 11 條），逾時算無效。
- **CPU**（第 13 條）：Linux 上兩個瀏覽器（kun 連同取樣器）用 `taskset` 綁在最高時脈最高的那組 CPU 上（`common.py` 的 `browser_cpus`，只挑這個 process 能用的；每顆一樣快或不知道時脈時不綁），報告寫出是哪些。有大小核的 CPU 上，瀏覽器整頁落在大核或小核上，數字差上小核的時脈那麼多，兩個瀏覽器各自落，倍數跟著跳。Speedometer 的工具也一樣綁。macOS 照 QoS 排，不能綁。
- **哪些頁跑**：預設是頁面清單（`scripts/bench/blink_perf/pages.txt`，第 5、12 條）：一行一頁，照領域分段，kun 現在有那些功能、兩邊連續兩次都跑得有效的。給了路徑（`bench.py blink-perf dom/ css/`）時只跑清單上那些路徑底下的頁（資料夾或單頁），和 `test.py wpt <目錄>` 一樣；清單裡沒有那些路徑的頁時停下。`--dirs`、`--pages` 照指定的跑，不管清單（找下一批要加的頁）；目錄底下的 `resources/`、`test_data/` 是頁面載入的檔案，不跑。
- **排除**：靜態掃頁面和它用 `<script src>` 載入的檔案（`runner.js` 除外），用了 `tracingCategories`、`traceEventsToMeasure`、`internals.`、`testRunner.`、`GCController`、`gpuBenchmarking`、直接呼叫的 `gc()`（`PerfTestRunner.gc()` 不算：沒有 `GCController` 時它什麼都不做），或自己呼叫 `logStatistics` 而沒有 `"Time:"` 的（第 10 條），一律排除：指定了也不跑，報告列出。
- **清單怎麼長**：從很小的一組開始（第 12 條：10 頁，兩次的倍數差得少、對應做過優化的地方：bindings 的呼叫、DOM 節點的成本、style 的比對、selector 查詢、排版和文字、事件分派），慢慢加。第 2 段跑過一次和 kun 的範圍相關的目錄（457 頁），有效的是之後加頁的候選，其他的記下原因（`progress.md`）。要加一頁：用 `--pages` 跑兩次，兩次都有效、不可疑才加，和改 WPT 的 `include.txt` 一樣；補上一個功能的 RFC 照樣這樣加相關的頁。清單上的頁變無效是回退，報告最前面列出。

### 3. 有效的檢查（第 4、8、9 條）

一頁有效：兩邊都交了數值、沒有例外、沒有 fatal、單位相同，而且做了一樣的事。「一樣的事」照 Speedometer 的做法：送出數值時取 document 的摘要（每種元素有幾個、每個 class 有幾個元素、文字有多長，經過 open shadow root），兩邊的摘要要相同。第一次跑範圍內的目錄時，它抓到 5 頁設定 `innerText`（kun 沒有，DOM 不變）、kun 快了 25 倍的頁。摘要看不出來的兩種，報告標出來，加進清單前人工看過：

- **頁面自己 catch 例外**（第 8 條）：靜態掃頁面和它載入的檔案，有 `try {` 或 `.catch(` 的標「會吞例外」；kun 缺的 API 被吞掉時，數值和摘要都正常。`run` 裡丟出的例外 runner 自己接住、變成 fatal，不用另外記。
- **kun 比 Chrome 快 2 倍以上**（kun 慢的倍數 < 0.5，第 9 條）標「可疑」，不算進幾何平均：`editing/textarea-dom` 每次把 1000 個文字節點加進 `<textarea>` 再清空，Chrome 每次更新 textarea 的值和 `:valid`、`:invalid`，kun 多半沒做；結束時兩邊的 document 一樣是空的，摘要相同。

### 4. 兩個版本（`--against`，第 14、15 條）

照 `bench.py speedometer --against`：`REV` 在 repo 旁邊的 worktree（`../kun-against`）checkout、build 到 `target/against`（兩邊共用 `../third_party`，所以 `REV` 的 `third_party.toml` 和 `.cargo/config.toml` 要相同，不同時不 build），和工作目錄的 kun 輪流跑清單（或給的路徑、`--pages`、`--dirs`）各 `--runs` 次（預設 3），另一個版本先，機器的漂移落在兩邊。只有 kun，不取樣。`compare.md`：每頁兩邊所有 iteration 的中位數和 95% 區間、變化，各頁時間（current ÷ against，runs/s 倒過來）的幾何平均，每一次 process 的中位數（有的頁每次落在不同的水準）；兩個版本的單位和摘要以 `REV` 的第一次為準，新的少做了事時那頁不比、列出原因。

不判斷單頁變快、變慢（第 15 條）：同樣的程式碼的兩個 build 比 10 頁，`css/PseudoClassSelectors` 差 34%（每個 build 自己三次都很穩）、`layout/line-layout-line-height` 差 15%，幾何平均差 0.0%。兩個 build 裡程式碼的位置和對齊不同，單頁的快慢跟著變，重複再多次也在；一個 build 裡 20 輪的區間反映不了。看幾何平均，單頁的變化要遠大於這個才算數。

### 5. 共用的程式

`common.py`（`scripts/bench/` 底下）：Chrome for Testing 的下載、CDP 的 pipe（`CdpPipe`、`ChromePipe`、`KunPipe`）、`run_in_kun`、`run_in_chrome`、server 的底（`Server`：送出 root 底下的檔案和 `/__kun__/summary.mjs`、收 POST、`take`、`forget`，各工具的 server 繼承它，覆寫 `get`）、綁 CPU（`browser_cpus`）、`kun_binary`、`environment`、摘要的比對（`summary_difference`）、`--against` 的 worktree 和 build（`against_problems`、`against_tree`、`against_binaries`、`versions_text`）和報告的小工具（`interval_text`、`change`）、Perfetto trace 的解析、取樣器的設定、中位數和信賴區間。`summary.mjs`：document 的摘要，Speedometer 的 `measure.mjs` import 它，blink_perf 的 server 把它接在 runner 後面。`speedometer.py` 從 `common` `import *`，`raster.py` 照舊經過 `speedometer` 拿到；`blink_perf.py` 直接用。

### 測試

- `blink_perf/blink_perf_test.py`（`scripts/bench/` 底下）：假的 `perf_tests/`（`fake/`：最小的 `runner.js`，和 Blink 的一樣在 `logFatalError` 裡先 `finish`；幾頁量時間和次數的、一頁丟例外、一頁兩邊做的事不同、一頁用 `internals`、一頁自己交統計、一頁載入會吞例外的 script、一頁讓摘要失敗、一頁沒有 catch 的例外），用真的 kun（dev profile）和 Chrome for Testing（沒下載時 skip）跑；挑頁、靜態掃描、有效的檢查、報告、清單和路徑、綁 CPU、`--against`（兩個版本用 dev build 真的跑一次）。
- `speedometer_test.py`、`raster_test.py` 照樣通過（共用的程式拆出來、搬過去之後）。
- mutation test：四份 spec 81 條全部抓到（`progress.md`）。
- 效能的量測不排進測試計畫（`docs/benchmarks.md` 的「效能的量測」）：工具做完，量不量由使用者決定。

### 分階段

1. **共用的程式拆出來，`perf_tests/` 固定和下載，一個目錄跑得起來**：`common.py`；`third_party.toml` 和 `ci.py third-party`；`bench.py blink-perf --dirs` 跑出數值和報告。
2. **有效的檢查、排除、頁面清單**：摘要、吞掉的例外和可疑的標記、排除的掃描；跑一次和 kun 的範圍相關的目錄，無效的記下原因；清單從 10 頁開始、照領域分段、只量給的路徑；兩個瀏覽器綁 CPU。
3. **`--against`**：kun 的兩個版本輪流比；共用的 worktree 和 build 搬到 `common.py`。

## 缺點

- 多一個要固定版本、要更新的第三方來源（`perf_tests/`），和一份要維護的頁面清單。
- 清單只有 kun 已經跑得好、跑得穩的頁：kun 沒有的功能、自己的數字會跳的頁（`css/AttributeDescendantSelector` 的 kun 每個 process 落在 74、125 或 230 runs/s）看不到，要等功能補上、原因查清、頁加進清單。
- 微基準放大單一操作：比值很極端（52 倍），不能直接換算成 Speedometer 的分數；用它找弱點，用 Speedometer 看整體。
- 摘要的檢查擋不住所有「少做了事」：有些頁面做完會把 DOM 清掉，兩邊的摘要都一樣是空的（所以有「可疑」的標記）。
- `--against` 看不出單頁的小變化：兩個 build 之間單頁就可能差三成。
- 跑大範圍很久：範圍內的 457 頁跑一次 2 小時 15 分（那時時限 120 秒）。

## 理由和其他做法

- **不用 Telemetry**（`tools/perf/run_benchmark blink_perf.*`）：要 Chromium 的 checkout 和它的 Python 環境，它經過 DevTools 控制「像 Chrome 的瀏覽器」，還要 `testRunner` 這類 content shell 的介面；kun 都沒有。這裡只拿頁面和 `runner.js`，控制瀏覽器用已經有的 CDP pipe。
- **不用 crossbench**（Chromium 新的 benchmark runner）：它能經過 WebDriver 控制其他瀏覽器，kun 有 machopodriver；但要多一套它的 Python 環境和設定，而這裡要的只是 blink_perf 的頁面加上已經有的 pipe。
- **自己寫 hook 接 `runner.js`，不改頁面**：頁面原封不動，換 `perf_tests/` 的版本時不用重做；hook 只接 `logStatistics` 和錯誤，`runner.js` 的量法不變。
- **不把頁面放進 repo**：30 MB、594 頁，換版本時整份換；照 WPT 的做法放在 `../third_party/`。

## 先例

- **Chromium 的 `blink_perf` benchmark**（`tools/perf/benchmarks/blink_perf.py`）：每頁一個 story，注入 `testRunner` 讓 `runner.js` 跑 5 次、把結果交給 Telemetry；有 `tracingCategories` 的頁面開 trace 再從 trace 量。perf bot 照目錄分成 `blink_perf.dom`、`blink_perf.css` 等 benchmark。這份照它挑頁的方式（`resources/` 不是測試）和分領域，不照它的注入和 trace。
- **`bench.py speedometer`**（RFC 0012、0054）：kun 和 Chrome 輪流、每次新的瀏覽器、摘要確認兩邊做了一樣的事、`--against`。這份照它。

## 已決定的事項

2026-10-20，寫草稿前（使用者）：

1. **之後的效能工作靠 blink_perf 找方向**，Speedometer 看整體。
2. **工具放在 `scripts/bench/` 底下的 `blink_perf/`，入口是 `bench.py blink-perf`，結果在 `target/bench/blink_perf/`**；和 Speedometer 共用的部分這時拆成 `scripts/bench/` 底下的 `common.py`（`scripts/bench/` 的搬家時決定，`544eeda0`）。

2026-10-20，草稿之後（使用者）：

3. **`perf_tests/` 從 `../third_party/` 來**：`third_party.toml` 固定 Chromium 的 commit，`ci.py third-party` 用 gitiles 的 `+archive` 下載那個目錄到 `../third_party/` 底下的 `blink_perf/`，和其他第三方一樣固定、同步。不選放進 repo（30 MB、換版本整份換），不選 sparse clone（Chromium 太大）。
4. **有效的檢查照 Speedometer**：數值送出時的 document 摘要兩邊相同、沒有例外和 fatal，另外記下 kun 被頁面吞掉的例外。不選只看例外：`textarea-dom` 那種少做事的看不出來。
5. **預設只跑頁面清單**（清單怎麼長，第 12 條改過）：kun 現在確定有那些功能、兩邊都跑得有效的頁，一頁一頁列出；kun 補上功能之後再加進去，一步步擴增支援的頁。不選預設跑和 kun 範圍相關的目錄（約 424 頁，草稿的建議）：裡面不少頁用到 kun 還沒有的功能，每次都是一堆無效，看不出哪些是回退；也不選全部能跑的 488 頁。
6. **每頁兩邊各開一個新的瀏覽器，輪流**。不選同一個瀏覽器連跑：前面的頁面留下的狀態會影響後面的（Speedometer 整套連跑時 Chrome 越跑越慢）。
7. **Chrome 預設 headless**（`--window` 這次沒做，第 16 條）：和 `bench.py speedometer` 一樣；清單上的頁大多不經過 GPU。不選預設開視窗：要顯示器，量的時候視窗會跳出來。

2026-10-20，第 2 段開始前（使用者）：

8. **頁面吞掉的例外用靜態掃描標出**，不讓 kun 回報被 catch 的例外。範圍內自己 catch 的只有 3 頁和 `bindings/resources/` 的幾個 structured clone 的檔案；`run` 丟出的 runner 已經變成 fatal。不選 V8 的 debug delegate（Chrome DevTools「caught exceptions 時暫停」的做法，`debug::DebugDelegate::ExceptionThrown`）：要改 rusty_v8 的 C++、重編 V8，還要一條送出 kun 的路，只為少數幾頁。
9. **kun 慢的倍數 < 0.5 的頁標「可疑」**，不自動進清單，人工確認兩邊做的事一樣才放進去。摘要和例外都看不出 `textarea-dom` 那種少做事。Telemetry 不檢查（這些頁本來只在 Chrome 跑），沒有可對照的做法。
10. **自己呼叫 `logStatistics`、沒有 `"Time:"` 的頁排除**（`mutation/` 的 5 頁：舊 WebKit 的寫法，量沒有和有 `MutationObserver` 的兩輪）。不選 hook 收最後一次 `logStatistics`：要決定取哪一輪，只為 5 頁；之後調 MutationObserver 的效能時再回頭支援。

2026-10-20，第 2 段跑完一次範圍內的目錄之後（使用者）：

11. **每頁的時限預設 90 秒**（草稿是 180 秒）。那一次以 ms 為單位的 281 次量測，量測時間超過 30 秒的只有一次（`display_locking/fieldset_container_width_changes` 在 kun 上約 74 秒），其次 27 秒；runs/s 的頁每輪約 0.75 秒，20 輪約 15 秒。180 秒時，逾時的頁每個瀏覽器白等 3 分鐘，457 頁花了 2 小時 15 分。不選 60 秒：最慢的那頁放不下。

2026-10-20，用 229 頁的清單跑第二次跑到一半時（使用者）：

12. **清單從很小的一組開始，照領域分段，量的時候只量那個領域**（改第 5 條的做法：第 2 段原本把第一次有效的 229 頁全放進清單，跑一次約 1 小時 40 分）。起點 12 頁，約 7 分鐘：bindings 的 `create-element`、`set-attribute`、`insert-before`；dom 的 `modify-element-id`、`select-single-add`；css 的 `AttributeDescendantSelector`、`PseudoClassSelectors`；`parser/query-selector-all-class`；layout 的 `chapter-reflow-once`、`flexbox-row-nowrap`、`line-layout-line-height`；`events/EventsDispatchingInV1ShadowTrees`。挑法：兩次都有效、倍數差在 1.1 倍以內（`query-selector-all-class` 只有一次，加進來之後連跑兩次確認），對應做過優化的地方（bindings 的呼叫、DOM 節點的成本、style 的比對、selector 查詢、排版和文字、事件）。綁了 CPU（第 13 條）之後連跑兩次，拿掉兩頁不穩的：`css/AttributeDescendantSelector` 的 kun 每次 process 落在 74、125 或 230 runs/s 其中一個水準（同一次裡很穩，原因還沒查），`dom/select-single-add` 的 Chrome 同一次裡在約 1900 和 4000 之間跳；剩 10 頁，約 6 分鐘。`bench.py blink-perf <路徑>…` 只跑清單上那些路徑底下的頁，像 Chromium 的 perf bot 把 blink_perf 分成 `blink_perf.dom`、`blink_perf.css`、`blink_perf.layout` 等 benchmark。加頁要連續兩次有效、不可疑：兩次之間有頁的倍數從 2.43 跳到 203（`dom/select-multiple-add`），有頁在可疑的門檻附近來回。不選留著 229 頁：太久，量一次要等一個多小時；不選照領域分檔案：一份清單和 WPT 的 `include.txt` 一樣好找。
13. **Linux 上兩個瀏覽器都綁在最快的那組 CPU 上，Speedometer 也一樣**（`common.py`：最高時脈最高的 CPU，`taskset` 啟動瀏覽器）。綁了之後 12 頁連跑兩次，10 頁的倍數差在 16% 以內（中位數 7%），另外兩頁的變動在瀏覽器自己（第 12 條）。12 頁連跑兩次，同一頁的倍數差到 1.5 倍（`bindings/set-attribute` 10.8 和 7.21）：拆開看是兩個瀏覽器各自整頁落在 Zen 5（5.16 GHz）或 Zen 5c（3.29 GHz）上，數字照時脈比例變（kun 的 `set-attribute` 209 或 314 runs/s）。都綁在快核上時兩次是 7.2 和 7.1 倍，都綁在慢核上 7.4 倍。Telemetry 在 Android 上量之前也先固定 CPU 的效能設定，為的是同一種雜訊。不選只綁 blink_perf：Speedometer 一樣受影響；代價是之後的 Speedometer 數字不能直接和以前的比。不選一頁兩邊各跑好幾次：時間加倍，雜訊照樣在。macOS 不能綁（照 QoS 排），維持原樣。

2026-10-20，第 3 段開始前（使用者）：

14. **`--against REV` 照 Speedometer 的做法**（單頁的判斷第 15 條拿掉）：只比 kun 兩個版本的時間（不跑 Chrome、這一段不取樣），輪流各跑 `--runs` 次（預設 3）；每頁兩邊所有輪的數值的中位數和 95% 區間、變化，每一次 process 的中位數（看每次落在不同水準的頁），各頁變化的幾何平均（原本區間不重疊時標變快、變慢，第 15 條拿掉）。兩個版本也比 document 的摘要（以 `REV` 為準），新的少做了事時那頁無效、不比。預設跑清單，可以給路徑、`--pages`、`--dirs`。worktree 和 build（`against_binaries` 等）搬到 `common.py` 共用。Chromium 的 Pinpoint 比兩個 build 也是輪流、重複多次，用統計檢定判斷；這裡頁少、次數少，用區間不重疊。不選只看一次：同一個 build 兩次就差好幾 %。

2026-10-20，第 3 段用同樣的程式碼試 `--against HEAD` 之後（使用者）：

15. **`--against` 不判斷單頁變快、變慢**，報告只列變化、區間和每次的中位數，看幾何平均。同樣的程式碼（`HEAD` 和工作目錄只差腳本）比 10 頁、各 3 次：5 頁的區間不重疊，被標成變快或變慢（`css/PseudoClassSelectors` +33.8%：同一個 build 三次都在 1750–1775，另一個都在 2310–2400；`layout/line-layout-line-height` +15%），幾何平均 1.000。兩個 build 在不同的目錄編，程式碼的位置和對齊不同，單頁的快慢跟著變，重複再多次也不會消失；一個 build 裡 20 輪的區間反映不了。不選拿同樣程式碼的比較當每頁的門檻：每台機器多一次約 15 分鐘的校準，換一個 build 門檻可能又變。不選留著判斷加警語：一半的頁誤標。Chromium 的 Pinpoint 用更多次和統計檢定，也一樣受 build 之間的差影響，找到的回歸要人確認。

2026-10-20，收尾時（使用者）：

16. **`--window` 這次不做**，記進「之後的可能」。第 7 條原本寫「`--window` 時兩邊都開視窗，和 `bench.py speedometer` 一樣」，但 Speedometer 的工具也沒有這個選項（草稿寫錯了），三段都沒做。清單上的頁都不經過 GPU，開視窗主要影響 canvas 這類不在清單上的頁；要做的話 kun 的 `--automation-pipe` 要開真的視窗、兩邊的數字要重新驗證。


## 待決定

無。

## 之後的可能

- `--window`：兩邊都開視窗量（第 16 條），canvas 這類經過 GPU 的頁要加進清單時再做。
- 擴充清單：第 2 段的 457 頁裡有效的是候選（`progress.md`），可疑的 22 頁要人工確認；`css/AttributeDescendantSelector` 的 kun 每個 process 落在不同水準的原因要查。
- 放進 CI：每天跑清單，和上一次比，看到回退就說（要穩定的機器）。
- `--sample`：在 `perf` 底下跑一頁，像 `bench.py speedometer --sample`。
- MotionMark：等之後處理 GPU 的時候。
- 其他 Chromium 的 benchmark（loading、rendering）：要錄好的網站，之後再看。
