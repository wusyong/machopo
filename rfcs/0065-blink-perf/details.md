# RFC 0065 的細節：perf_tests 的盤點、取樣、runner.js

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-10-20，Chromium 的 `main`（gitiles 的 `+archive/refs/heads/main/third_party/blink/perf_tests.tar.gz`，約 30 MB）。每個目錄底下的 `.html`（不算 `resources/`、`test_data/`）：用 `perf_tests/resources/runner.js` 的；其中靠 tracing 量的（`tracingCategories` 或 `traceEventsToMeasure`）；其他用了 `internals.`、`testRunner.`、`gc()` 的；剩下不靠 Telemetry 就能跑的。

| 目錄 | 頁 | 用 runner.js | tracing | Chromium 專用 API | 能跑 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `layout` | 137 | 137 | 0 | 0 | 137 |
| `css` | 81 | 81 | 24 | 0 | 57 |
| `bindings` | 53 | 53 | 0 | 3 | 50 |
| `shadow_dom` | 39 | 39 | 0 | 0 | 39 |
| `parser` | 32 | 32 | 0 | 0 | 32 |
| `MotionMark` | 31 | 2 | 0 | 0 | 2 |
| `paint` | 28 | 28 | 0 | 1 | 27 |
| `accessibility` | 27 | 27 | 27 | 0 | 0 |
| `svg` | 24 | 24 | 0 | 1 | 23 |
| `dom` | 19 | 19 | 0 | 2 | 17 |
| `editing` | 15 | 15 | 0 | 2 | 13 |
| `webaudio` | 14 | 14 | 1 | 4 | 9 |
| `display_locking` | 12 | 12 | 0 | 0 | 12 |
| `webcodecs` | 12 | 12 | 0 | 0 | 12 |
| `image_decoder` | 10 | 10 | 0 | 0 | 10 |
| `owp_storage` | 10 | 10 | 10 | 0 | 0 |
| `service_worker` | 9 | 9 | 0 | 0 | 9 |
| `mutation` | 8 | 8 | 0 | 0 | 8 |
| `webgpu` | 7 | 7 | 0 | 0 | 7 |
| `events` | 6 | 6 | 0 | 0 | 6 |
| `loading` | 4 | 4 | 0 | 0 | 4 |
| `webgl` | 4 | 4 | 0 | 0 | 4 |
| `base64` | 3 | 3 | 0 | 0 | 3 |
| 其他（`intersection-observer`、`speedometer21`、`view_transitions`、`websocket`、`xml_http_request`、`performance_apis`） | 9 | 9 | 2 | 0 | 7 |
| **合計** | **594** | **565** | **64** | **13** | **488** |

和 kun 現在的範圍相關的（第 2 段跑一次、挑出頁面清單的範圍，`README.md` 第 5 條）：`layout`、`css`、`bindings`、`shadow_dom`、`parser`、`paint`、`svg`、`dom`、`editing`、`display_locking`、`mutation`、`events`、`base64`，約 424 頁。

頁面用 `runner.js` 的方式（次數是呼叫的處數）：`measureRunsPerSecond` 200、`measureTime` 161、`startMeasureValuesAsync` 53 和 `measureValueAsync` 48（自己量、自己交數值）、`measurePageLoadTime` 22（iframe 載入一個檔案）、`measureTimeAsync` 16。`forceLayout` 233 處。

## runner.js 怎麼量

- 沒有 `window.testRunner` 時跑 20 次（`iterationCount = test.iterationCount || (window.testRunner ? 5 : 20)`），第一次是熱身、記一行「Ignoring warm-up run」不算；有 `warmUpCount` 的照它。
- 每次的數值記在 `results`，跑完 `finish()` 呼叫 `logStatistics(results, unit, "Time:")`（中位數、平均、標準差、最小、最大），寫在頁面的 `<pre id="log">`；有 `internals` 時另外記 JS heap。
- `measureTime`、`measureTimeAsync` 的單位是 ms（越小越好），`measureRunsPerSecond` 是 runs/s（越大越好），自己交數值的頁面自己給單位。
- 有 `tracingCategories` 的頁面，沒有 Telemetry 時只寫一行「Tracing based metrics are specified but…」，量不到東西。
- 結果不檢查：頁面的 `run` 丟例外會記一行、照樣繼續；不支援的 API 被頁面自己的 `try` 吞掉時什麼都看不出來。

## 取樣

2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370），kun `main` 的 `6ac0a5a3`（`perf` profile，`--automation-pipe`），Chrome for Testing 152.0.7977.82（headless）。每個能跑的目錄隨機挑兩頁（`random.seed(1)`），scratchpad 裡的 harness（server 送出 `runner.js` 時接一段 hook，把 `logStatistics` 的數值 POST 回來），每頁兩邊各一個新的瀏覽器、各跑一次，20 次的中位數。

| 頁 | 單位 | kun | Chrome | kun 慢幾倍 |
| --- | --- | ---: | ---: | ---: |
| `svg/SvgHitTesting` | ms | 856.55 | 16.30 | 52.6 |
| `bindings/get-elements-by-tag-name` | runs/s | 45.99 | 765.69 | 16.6 |
| `events/EventsDispatching` | runs/s | 168.22 | 2475.37 | 14.7 |
| `dom/modify-element-classname` | ms | 393.65 | 49.90 | 7.9 |
| `css/PseudoClassSelectors` | runs/s | 1631.40 | 12080.79 | 7.4 |
| `shadow_dom/imperative-api-custom-detail-summary` | ms | 3.70 | 0.50 | 7.4 |
| `parser/query-selector-last` | runs/s | 598.37 | 3071.80 | 5.1 |
| `dom/clone-node-containing-form-elements-inserted-into-collection` | runs/s | 1253.12 | 4797.00 | 3.8 |
| `parser/innerHTML-setter-siblings` | runs/s | 13923.98 | 45907.63 | 3.3 |
| `layout/multicol/lots-of-text-autofill` | runs/s | 134.05 | 208.57 | 1.6 |
| `svg/SvgCubics` | ms | 5.60 | 3.65 | 1.5 |
| `shadow_dom/v1-mutate-shallow-tree-then-re-layout` | ms | 10.60 | 7.00 | 1.5 |
| `paint/gap-decorations-grid-auto-repeat` | ms | 521.05 | 446.75 | 1.2 |
| `paint/modify-selection` | ms | 17.10 | 16.55 | 1.0 |
| `css/NestingIdentKnownProperty` | ms | 1.10 | 1.20 | 0.9 |
| `events/hit-test-lots-of-layers` | runs/s | 3.71 | 1.93 | 0.5 |
| `editing/textarea-dom` | runs/s | 508.94 | 4.64 | 0.01（不可信：kun 多半沒做到） |

沒有數字的：

- kun 交了空的結果，Chrome 有：`layout/multicol/tall-content-short-columns`、`base64/atob`、`base64/atob-whitespace`、`editing/many_spans_with_move_backward_character`（原因還沒查）。
- 兩邊都 180 秒內沒有回報：`mutation/inner-html`、`mutation/remove-child-deep`（第 1 段查了：它們自己呼叫 `logStatistics`、沒有 `"Time:"`，量兩輪；README 第 10 條排除）。
- `bindings/wasm_webgl_no_js`：kun 沒有 WebGL；Chrome 那邊 harness 送 `.wasm` 的 MIME type 不對（`WebAssembly.compile` 拿到 HTML）。

## 時間

取樣的 24 頁，兩邊各一次，約 15 分鐘（多數頁一兩秒，少數十幾秒到幾十秒，逾時的 180 秒）。約 424 頁照比例估 1–2 小時；逾時的頁會拉長，所以時限要能設。

## 和 Speedometer 整套連跑的對照

2026-10-20 量 Speedometer 時發現（`bench.py speedometer` 每個 suite 各開一個新的瀏覽器）：官方的整套連跑（所有 suite 在同一頁，10 個 iteration）裡，headless 的 Chrome 越跑越慢，後面的 iteration 每個 suite 都慢 35–75%，kun 沒有；Chrome 開視窗時好一些，但同一次裡照樣起伏。所以這份每頁開新的瀏覽器（`README.md` 第 6 條）。開視窗的 Chrome 在 canvas 的 suite 快很多（GPU 的 canvas），頁面清單上的頁大多不經過那裡（`README.md` 第 7 條）。
