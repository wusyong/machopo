# RFC 0012：用 Speedometer 3 和 Chromium 比較效能

- 開始日期：2026-10-07
- 狀態：✅ 已完成：2026-10-16（第 1 到 10 條 2026-10-07 接受，之後每一段開始前各自確認，到第 43 條；第 1 階段「盤點」和第 2 階段「量測的工具」（2a 到 2d-3）都做完、merge 回 `main`：`ci.py speedometer` 每個 suite 在 kun 和 Chrome for Testing 152 各跑一次、比較表、`--inventory`、`--trace`（兩邊都是 Perfetto 的 trace，kun 的 V8 用 Perfetto 編）、`--sample`（Linux 的 `perf`）；20 個 default suite 在 kun 上都跑得完，功能由路線圖上的各份 RFC 做；之後的量測只在使用者要求時做、記進 `progress.md`，優化另開 RFC；驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`；路線圖（RFC 的切法、平行的線、里程碑）：`roadmap.md`；和 Blink 做法不同、多花時間的地方：`architecture-audit.md`；進度：`progress.md`

## 摘要

用 Speedometer 3.1 當 kun 的效能基準，和 Chromium 比較：同一份 Speedometer、同樣的參數、在同一台機器上，每個 suite 在 kun 跑完接著在 Chrome for Testing 152 跑，比每個 suite（和每個 test 的 sync、async）的時間。這份 RFC 做了三件事：

1. **suite 和擋住它的功能的對照表**（盤點）：每一項功能歸到負責它的 RFC（`roadmap.md`），不在這裡實作。開始時 kun 連 Speedometer 自己的 runner 都跑不起來（「動機」），所以先盤點（第 3 條）；2026-10-16 的盤點裡 20 個 default suite 在 kun 上都跑得完、沒有例外。
2. **量測的工具** `python3 scripts/ci.py speedometer`：固定的版本、本機的 server 和注入的 script、兩邊的頁面都把結果 `POST` 回 server、用 DOM 的摘要檢查有不有效、中位數和比值的比較表。
3. **看得出時間花在哪的工具**：`--trace` 在兩邊各錄一份 Perfetto 的 trace（kun 的 V8 用 Perfetto 編，kun 的 span 照 Chrome 的名稱和層次，和 V8 的事件在同一份 trace 上）；`--sample` 在 Linux 上用 `perf` 對 kun 取樣，只算 Speedometer 計時的區間。

20 個 default suite 全部有效之前只比 suite 的時間，不算總分。Speedometer 平常不量，只在使用者要求時跑（第 37 條）；量到的差距由各自的優化 RFC 處理。

## 動機

**開始時的現況（2026-10-07）**：
- **kun 沒有固定的效能量測**：只有零星的實測（`docs/limitations.md` 的集合讀取約 2 µs、RFC 0001 的 wasm 編譯時間），沒有 benchmark，也沒有和 Chromium 比較的數字。程式裡有 `tracing` 的 span，但不能輸出成 trace（當時 `docs/limitations.md` 的「平台和建置」的一條，這份 RFC 解決）。
- **已知的效能簡化很多，不知道哪一個在真的頁面上最貴**：style 以子樹為單位重算、layout 的祖先整個重跑、paint 每次走過整棵 fragment tree、集合的每次讀取經過 interceptor（`docs/limitations.md` 的「Invalidation」「Bindings 和 DOM API」）。
- **Speedometer 3 量的正好是 kun 的整條 pipeline**：它在 iframe 裡跑真的框架寫的 app（TodoMVC 的各種框架、新聞網站、編輯器、圖表），每一步量 script 的時間（sync），加上到下一個 frame 的 style、layout、paint 完成的時間（async）。它由 Apple、Google、Mozilla 共同維護，Chromium 用它追蹤回歸。
- **kun 跑不起來**：Speedometer 的 runner 本身就要 ES module、`element.style`、`getBoundingClientRect` 和強制的同步 layout、`performance.mark` / `measure`；每個 suite 再各自要表單、`KeyboardEvent`、`element.click()`、shadow DOM、SVG、canvas……（`details.md` 的「kun 跑不起來的地方」）。所以要先有工具和對照表，之後每解開一個 suite 就能量。

目標：
- 同一份 Speedometer 3.1、同樣的參數，在同一台機器上跑 kun 和 Chrome，得到每個 suite、每個 test 的 sync 和 async 時間，附上中位數、信賴區間和兩邊的比值。
- 每個結果都附上**有不有效**：Speedometer 不檢查 app 做了什麼，功能缺了、app 沒有反應時，kun 反而會比較「快」。無效的 suite 不給數字。
- 一份對照表：每個 default suite 被哪些功能擋住、每項功能由哪份 RFC 負責；由盤點（Chrome 實際用到的 API 和 CSS，問 kun 有沒有）產生，不靠猜。
- 能把 kun 和 Chrome 的 trace 並排看（Perfetto），找出時間花在哪裡；trace 看不到的（JS 和 DOM API 裡面）用取樣看。

不在範圍內：
- **優化本身**：量出差距之後，針對那個瓶頸另開 RFC（RFC 0039、0042 是這樣開的；`architecture-audit.md`）。
- **讓 suite 能跑的功能**：ES module、CSSOM、表單、shadow DOM、SVG、canvas 等各自在負責的 RFC 做（「盤點的結果」指過去）。
- **總分**：20 個 default suite 全部有效之前不算。部分 suite 的 geomean 和 Chrome 的分數不能比。
- **其他 benchmark**：Speedometer 的 main（下一版，2026-09 還在改結構，`details.md` 的「版本」）、2.x、JetStream、MotionMark、blink_perf（見「之後的可能」）。
- **在 CI 上量效能**：CI 的共用機器雜訊太大。CI 上檢查「預期有效的 suite 還有沒有效」沒有決定，在「之後的可能」。

## 設計概要

**給改 kun 的人**（選項的完整說明見 `ci.py speedometer -h`）：

```sh
python3 scripts/ci.py speedometer                    # default 的 suite，每個在 kun 跑完接著在 Chrome 跑，寫出比較表
python3 scripts/ci.py speedometer --suites TodoMVC-JavaScript-ES5,TodoMVC-jQuery --iterations 20
python3 scripts/ci.py speedometer --browser kun      # 只跑一邊；kun 的和 Chrome 上次的紀錄比有不有效
python3 scripts/ci.py speedometer --inventory        # 盤點：每個 suite 用到、kun 還沒有的 API 和 CSS；每個 suite 在 kun 上跑不跑得完
python3 scripts/ci.py speedometer --trace            # 每個 suite 在兩邊各錄一份 Perfetto 的 trace（時間不放進比較表）
python3 scripts/ci.py speedometer --sample           # Linux：用 perf 對 kun 取樣，只留計時的區間
```

結果寫進 `target/speedometer-results/`：
- `measure.md`：開頭是環境（kun 的 commit 和 profile、Chrome 和 OS 的版本、CPU、有沒有接電源），每個 suite 一列：kun 和 Chrome 的時間（中位數和 95% 信賴區間）、比值、第一個 iteration；無效的寫原因（例如「kun: error in /index.html: TypeError: …」「kun: iteration 0, TodoMVC-jQuery/Adding100Items: 0 `li` elements, Chrome 100」：DOM 的摘要是整份文件裡每種元素的個數）；另一張表是每個 test 兩邊的 sync 和 async。完整的資料在 `kun-runs.json`、`chrome-runs.json`。
- `trace/`：每個 suite 兩份 `.pftrace`，用 Perfetto 的 UI 或 trace processor 開。
- `sample/`：每個 suite 的 folded stacks（speedscope、`flamegraph.pl` 讀得了），和取樣時錄的 trace、`perf` 的資料；`sample.md`：每個 suite 樣本最多的函式（self 和 inclusive）。
- `inventory.md`（依 suite 列）、`inventory.json`（全部）。

一般的工作流程（使用者要求量的時候，第 37 條）：跑比較 → 慢很多的 suite 用 `--trace` 看兩邊的 trace 差在哪個 span → 差在 JS 和 DOM API 裡面的用 `--sample` 看是哪個函式 → 對照 Blink 的做法（`architecture-audit.md`），開優化的 RFC。數字和分析記進 `progress.md`。

## 詳細設計

### Speedometer 3.1 怎麼量

細節見 `details.md` 的「Speedometer 3.1 的結構」。重點：
- 頁面的 runner 是 module script（`main.mjs`）；每個 suite 在 runner 建立的 iframe（預設 800×600，用 `element.style` 置中）裡載入，`load` 之後跑 suite 的 `prepare`，然後依序跑每個 test。
- 每個 test（`measurementMethod=raf`，預設）：一個 rAF callback 裡同步執行 test 的步驟，量到的是 **sync**；同一個 frame 的第二個 rAF callback 排一個 `setTimeout(0)`，它執行時 rendering update 已經跑完，先呼叫 iframe 的 `body.getBoundingClientRect()` 強制 layout，量到這裡是 **async**。
- 每個 suite 的時間是它的 test 的 sync + async 的總和；每個 iteration 的分數是 1000 除以各 suite 時間的 geomean。
- runner 在每個 test 量完之後呼叫 `benchmarkClient.didRunTest`（不計時），全部跑完時呼叫 `didFinishLastIteration(metrics)`。

### 固定的版本

Speedometer 放在 kun 旁邊的 ../third_party/speedometer，checkout 在上游 `WebKit/Speedometer` 的 `release/3.1` 分支的 `1386415`（2025-03-11），寫進 `third_party.toml`，由 `ci.py third-party` 同步（第 1 條）。和 WPT 一樣，kun 不改它（要改的行為由 server 注入，見下一節）。repo 裡有每個 suite 編好的 `dist/`，不用 build。

### 本機的 server 和注入的 script

`ci.py speedometer`（`scripts/speedometer.py`）用 Python 標準函式庫的 HTTP server 提供 Speedometer 的檔案（`ci.py` 只用標準函式庫），兩個瀏覽器讀同一個 server、同樣的 URL（`index.html?startAutomatically&suites=<一個 suite>&iterationCount=…`，其他參數用預設值）。server 回應時注入 `scripts/speedometer/` 的 script，兩邊注入的一樣，所以對時間的影響相同（第 15 條）：
- **每一頁 `<head>` 的最前面**：`errors.js`，記下 `error` 和 `unhandledrejection`。iframe 裡的錯誤不會傳到 runner 的頁面，所以要在 suite 的頁面裡收。
- **runner 的頁面在 `main.mjs` 之後**：一個 module script（module 照順序執行，所以這時 `benchmarkClient` 已經在了），包住 `benchmarkClient` 的 hook。量測時是 `measure.mjs`：`didRunTest` 在量完之後（不計時）把 iframe 的 document 走一遍，算出 DOM 的摘要（「有不有效」），`didRunSuites` 收每個 iteration 的時間；runner 跑完、放棄（`handleError`）、例外之後 5 秒沒有進展、或逾時時，用 `fetch` 把時間、摘要和錯誤 `POST` 回 server（第 14 條）。盤點時換成 `inventory.mjs`（Chrome）和 `kun.mjs`（kun），見「盤點」。

### 兩邊的 driver

- **交回結果**：兩邊同一段注入的程式 `POST` 回 server，server 收到後結束那個瀏覽器；逾時、process 提早結束都是無效（第 14 條）。原本的設計是 Chrome 用 CDP 的 WebSocket 讀全域變數、kun 加 `testRunner.reportBenchmarkResults` 和 `--run-benchmark` 選項，理由是 kun 的頁面送不出結果；RFC 0027 之後 kun 有 `fetch`，兩邊改走同一條路。
- **kun**：`kun --automation-pipe`（沒有平台視窗的瀏覽器視窗，Viz 自己每 16.7 ms 一個 `BeginFrame`，CPU raster），`ci.py` 從 stdin 送 `Browser.setWindowBounds`、`Target.attachToTarget`、`Page.navigate`（`protocol::automation` 照 CDP 的格式）。用 `perf` profile，見「公平的條件」。kun 沒有為了量測加的選項。
- **Chrome**：Chrome for Testing 152.0.7977.82（第 11 條，`ci.py` 下載到 `target/` 底下），每次一個全新的 profile、`--headless=new`，網址放在命令列上。不用 CDP；不選 crossbench、Puppeteer、WebDriver 的理由見「理由和其他做法」。
- **兩邊都沒有視窗**（第 17 條）：視窗 1200×900，Speedometer 的 iframe 照預設 800×600。
- **每個 suite 一個新的 process**（第 24 條）：`suites=<一個>`，連續跑 `--iterations` 次（預設 10）。kun 在一個 suite 卡住或當掉只少那一個，suite 之間的 heap 和 JIT 的狀態也影響不到別的 suite。
- **每個 suite 在 kun 跑完接著在 Chrome 跑**（第 26、38 條）：同一個 suite 的兩邊時間上靠得最近，溫度和背景工作的漂移對兩邊差不多。原本預設 3 輪、每輪換先後（`--rounds`），實際的量測幾乎都用一輪，第 38 條拿掉了輪；要更多樣本時加 `--iterations`。
- **只跑一邊**：`--browser kun` 的 run 和 Chrome 上次的 run（`chrome-runs.json`）比有不有效；`--reuse-chrome` 不跑 Chrome，沿用它上次的紀錄來檢查 kun 的 run（第 23 條；比較表沒有 Chrome 的時間和比值）。

### 有不有效

一個 suite 的一次 run 有任何一項成立就無效（第 16 條），兩邊任何一邊無效，表上就不給那個 suite 的數字（第 29 條）：
1. suite 的頁面或 runner 有沒接住的例外（`error`、`unhandledrejection`），或 runner 卡住（逾時）。
2. **DOM 的摘要和 Chrome 不同**：每個 test 之後，走過 iframe 的 document（包括 open shadow root），算出各個 tag 的元素數、`<style>` 和 `<script>` 以外的文字的總長度、有 `class` 的元素的 class 的計數；同一個 suite、同一個 test 在 Chrome 第一個 iteration 的摘要當參考（第 21 條），kun 的每個 iteration、Chrome 自己後面的 iteration 都和它比。不用完整的序列化：日期、亂數、和 kun 無關的屬性順序會不同。容許的差異是 Chrome 跑兩次之後定的（2026-10-11）：只有 Charts-observable-plot 每次亂數產生 class 名稱（`plot-7017677fbe27d8`），名稱也寫進它的 `<style>`；所以比較時 class 名稱裡 10 個以上連續的十六進位字元當成同一個，文字長度不算 `<style>` 和 `<script>`。之後 Chrome 的 20 個 suite 每次都有效、兩次之間相同。
3. Speedometer 自己的檢查：suite 的時間是 0 時 runner 丟例外。

kun 少畫了什麼（例如 iframe 不畫時 async 幾乎是 0）摘要看不出來，所以對照表裡「這個 suite 依賴、但 kun 還沒有的 rendering 功能」也要看：盤點列出 kun 丟掉的 CSS（「盤點」）。

### 比較表

- **樣本**：每個 suite 的樣本是每個 iteration 的 suite 總和，第一個 iteration 也算（第 27 條：照 Speedometer 自己的算法；正式的 Speedometer 裡每個 suite 的第一個 iteration 也是冷的）。
- **統計**（第 28 條）：中位數和不假設分布的 95% 信賴區間（順序統計量，二項分布；10 個樣本是 x(2) 到 x(9)，至少要 6 個樣本），比值是兩邊中位數的比；另外列第一個 iteration 的中位數。一張 test 的表列兩邊 sync、async 的中位數，看差距在 script 還是 rendering。不用 Speedometer 的平均和 t 分布：容易被少數慢的 iteration 拉高。
- **環境**（第 30 條）：日期、kun 的 commit 和 profile、Chrome for Testing 的版本、OS 的版本、CPU 的型號和核數、有沒有接電源（沒接時警告、照樣跑），寫進 `measure.md` 和 JSON；量到的數字記進 `progress.md`。

### 公平的條件

細節和每一項的理由見 `details.md` 的「公平的條件」：
- **同樣的 V8 世代**：kun 連結 V8 15.2（`.cargo/config.toml` 的 `RUSTY_V8_MIRROR_TAG`，完成時是 `kun-v152.2.0-8`），Chrome 用同一個 milestone 的 Chrome for Testing 152，這樣量到的差距主要在 V8 以外。V8 的設定仍有差別：Chrome 開 V8 sandbox，kun 沒開（`docs/limitations.md` 的「隔離」）。
- **kun 的 build**：`perf` profile（第 25、43 條）：繼承 release，`lto = "thin"`、`codegen-units = 1`、`debug = "line-tables-only"`（不影響產生的程式，取樣時看得到函式和行號），在 Linux 和 macOS 上加 `-C force-frame-pointers=yes`（`speedometer.py` 編的時候用 `RUSTFLAGS` 給，只有它編這個 profile），在 `target/perf/`。Chrome 的 official build 是 ThinLTO 加 PGO，在 Linux 和 macOS 上也留著 frame pointer；kun 沒有 PGO。量測只用 `perf` profile，`--inventory` 和工具自己的測試用 dev。
- **frame 的節奏**：kun 的 headless 一回答就送下一個 `BeginFrame`（虛擬的時間），沒有平台視窗的瀏覽器視窗才是每 16.7 ms（`crates/shell/src/windowless.rs`）；Chrome 的 `--headless=new` 照 60 Hz。driver 用後者，兩邊都是真的 60 Hz，不另外量兩種節奏（第 30 條）。rAF 的 callback 和 rendering update 在同一個 task 裡（`render_thread.rs` 的 `begin_main_frame`），所以 `setTimeout(0)` 在 rendering update 之後執行，和 Chrome 一樣。
- **同樣的環境**：同一台機器、接電源、同樣的電源計畫，關掉其他程式；結果記下 OS、CPU。主要的比較原本定在 macOS（Apple Silicon）上做（第 18 條）；每次量的平台記在 `progress.md`。Chrome 那一側在 Windows 上跑不了（`docs/limitations.md` 的「平台和建置」）。

### trace

`--trace` 另外跑一次（第 34 條）：每個指定的 suite 兩邊各一次（iteration 數照 `--iterations`），trace 存在 `target/speedometer-results/trace/`，時間不放進比較表（錄 trace 本身會拖慢）。兩份都是 Perfetto 的 protobuf（`.pftrace`），開的 category 一樣：`toplevel`、`blink`、`v8`、`cc`、`viz`、`benchmark`、`loading`、`devtools.timeline`（DevTools 效能面板的 `FunctionCall`、`UpdateLayoutTree`、`Layout`、`Paint`；預設的全部會讓檔案大很多），kun 再加上自己的 `kun`。V8 的 GC 事件在 `devtools.timeline,disabled-by-default-v8.gc`，開了 `devtools.timeline` 就錄得到，兩邊一樣；compile 的事件在 `disabled-by-default-v8.compile`，兩邊都沒開。

**Chrome 那一側**（第 35 條）：啟動的旗標 `--trace-startup=<category>`、`--trace-startup-file`、夠長的 `--trace-startup-duration`，不用 CDP。踩過的坑：
- 被結束時 JSON 格式的 trace 寫不出來（Chrome 結束時才轉換），Perfetto 的 protobuf 邊錄邊寫才留得下，所以 Chrome 的 trace 是 `.pftrace`。收到結束的要求後要 5 到 15 秒才寫完，錄 trace 時等到 60 秒。
- Linux 上 headless 的 Chrome 152 收到 SIGTERM 時不寫 trace 就結束（SIGINT 會寫）：在 POSIX 上用 SIGINT 結束要寫 trace 的 Chrome。Windows 上結束 process 是直接殺掉，拿不到 Chrome 的 trace（`docs/limitations.md`）。

**kun 那一側**：`MACHOPO_TRACE=<檔案>`（第 32 條：視窗、`--headless`、`--automation-pipe` 和測試都能用，寫在 `--help` 和根目錄的 README）錄一份 Perfetto 的 trace，V8 自己的事件（GC 的各個階段、deopt；compile 和 parse 的在沒開的 `disabled-by-default-v8.compile`，和 Chrome 那一側一樣）和 kun 的 span 在同一份 trace、同一個 thread 和時鐘上，和 Chrome 的 trace 同構（第 40、41 條）：
- **V8 用 Perfetto 編**（第 40 條）：rusty_v8 的 fork 加上 V8 的 `DEPS` 釘的 `third_party/perfetto`、`third_party/protobuf`、`third_party/zlib`、`tools/protoc_wrapper`，`.gn` 設 `v8_use_perfetto = true`（`v8_use_perfetto_json_export`、`enable_js_protobuf`、`enable_perfetto_zstd` 關掉），和 Chromium 一樣。release 的靜態庫大了約 18 MB；三個平台都編得過（fork 的 release `kun-v152.2.0-8`）。
- **錄製在 fork 裡**（第 41 條）：fork 的一小段 C++（照 d8：`perfetto::Tracing::Initialize` 用 in-process backend，再開一個 session）和包成安全的 Rust 模組；kun 的 `bindings::trace` 提供、`dom_js::trace` 轉手、`shell::trace` 使用，kun 這邊沒有新的 `unsafe`。buffer 是 64 MB 的 ring buffer，邊錄邊寫進檔案（`write_into_file`，每秒一次），結束時寫完剩下的。設了 `MACHOPO_TRACE` 時 `shell` 在 V8 初始化之前開始錄（V8 只在 Perfetto 已經初始化時註冊它的事件），`main` 結束時停；沒設時不碰 Perfetto。kun 的 span 經過 `shell::trace` 的 layer 在進入、離開時呼叫 fork 的 begin / end（category `kun`，名稱是動態字串）；log 的事件不進 trace。
- **錄 trace 不能改變 kun 做的事**：tracing-subscriber 的 `init` 照全部 layer 的最高層級設定 `log` crate 的最高層級，trace 的 layer 沒有層級的提示，結果變成 TRACE，html5ever 的 tree builder 為每個 token 做出 `log::debug!` 的字串再丟掉（jQuery 的計時區間裡 41% 的樣本）。`shell` 在 `init` 之後把 `log` 的最高層級設回 log 那一層的。
- **`--automation-pipe` 的結束**：`ci.py` 送 `Browser.close` 之後等 kun 自己結束，`main` 走完才寫得完 trace 的最後一段。

**span 的名稱和層次**（第 33、42 條，第 43 條的 (6)）：照 Chrome 152 的 trace 裡實際的名稱和層次，兩份 trace 逐項對得起來：
- task 和 script：renderer 的每個 task 是 `ThreadControllerImpl::RunTask`；`TimerFire`、`PageAnimator::serviceScriptedAnimations` 和 `FireAnimationFrame`、`EventDispatch`（每個 dispatch，async 的入口也是）、`RunMicrotasks`、`EvaluateScript`。每次呼叫 JS 是 `v8.callFunction`，stack 上還沒有 script 時裡面再一個 `FunctionCall`（`dom_js` 用 thread-local 的深度判斷最外層，呼叫、建構、執行 script 和 module 都算一層）；建構是 `v8.callAsConstructor`。
- 一次就很重的工作：script 設的 markup 和文件的每一段 parse 是 `ParseHTML`（fragment 的包住從 markup 到 `DocumentFragment` 的整段，搬進 fragment 的成本也算在裡面，和 Chrome 的比得起來）；custom element 每個元素的 reaction 是 `CustomElementReactionQueue::invokeReactions`，`define` 是 `CustomElementRegistry::define`。
- frame：`ProxyMain::BeginMainFrame` 包住整個 `begin_main_frame`（rAF 在它底下的 `WebFrameWidgetImpl::BeginMainFrame` 裡），每一步一個 span，有對應的用 Chrome 的名稱（`WebFrameWidgetImpl::UpdateLifecycle`、`LocalFrameView::UpdateLifecyclePhases`、`LocalFrameView::RunStyleAndLayoutLifecyclePhases`、`Document::UpdateStyle`、`StyleEngine::RebuildLayoutTree`、`Layout`、`PrePaint`、`Paint`、`PaintArtifactCompositor::Update`、`LocalFrameView::UpdateViewportIntersectionsForSubtree`、`LayerTreeHost::DoUpdateLayers`、`ProxyMain::BeginMainFrame::commit`……），沒有的用 kun 的 `Type::method`（`FrameView::take_canvas_textures`、`paint::main_thread_scroll_hit_test_region`、`FrameView::set_wheel_event_handler_region`……）。hit test 是 `HitTest`。
- 不做每個 DOM API 呼叫一個 span（第 39 條）：jQuery 一個 test 就有幾十萬次呼叫，開了 trace 的時間失真，關了也在 binding 最熱的路徑上多一次檢查；Chrome 也不這樣做。JS 和 DOM API 裡面的時間用取樣看。

**分析**：用 Perfetto 的 trace processor 算 renderer main thread 上每個名稱的 self time，只算 Speedometer 計時的區間（runner 的 `FireAnimationFrame` 到它之後第一個 `TimerFire` 的結束）；區間的長度含錄 trace 的開銷，只看比例。做法和每次的結果在 `progress.md`。

**踩過的坑**（第 40、41 條）：
- **V8 不用 Perfetto 編時錄不到它的事件**：原本選的 G2 沿用 `v8_use_perfetto = false`，fork 加一個把舊的 `TracingController` 介面轉給 Rust 的子類別。在本機編好 V8 試錄 jQuery，只錄到 `V8.DeserializeContext` 和 `V8.DeserializeIsolate`：V8 新式的 `TRACE_EVENT(...)` 和 GC 的 `TRACE_GC*`（382 處）在沒有 Perfetto 時是空巨集（`trace-event-no-perfetto.h` 的 `INTERNAL_TRACE_IGNORE`），經過舊介面的只剩 16 處、幾乎都是 snapshot。
- **不用 libplatform 的 `TracingController`**：它把 buffer 寫死成 4 MB 的 ring buffer，滿了默默覆蓋最舊的事件，一個 suite 的 Chrome trace 就有 6 MB。
- **buffer 不用 `DISCARD`**：原本選滿了就停的 `DISCARD`，trace processor 的統計出現 `config_write_into_file_discard`：配 `write_into_file` 會默默丟資料，改成 ring buffer（邊寫邊清，只要寫出的速度跟得上就不會覆蓋）。
- **2c 的 kun 那一側是 `tracing-chrome` 寫的 JSON**（第 31 條），和 V8 的事件分開；V8 的事件進來之後兩份要開兩次、和 Chrome 的不同構，第 41 條拿掉它。

### 取樣

`--sample`（第 43 條，只在 Linux、只跑 kun：Chrome for Testing 沒有符號）：每個 suite 跑一次，kun 在 `perf record -e cycles:u -k monotonic --call-graph fp` 底下跑，同時錄 `MACHOPO_TRACE`；只留 renderer main thread（`CrRendererMain`）上、計時區間裡的樣本（區間照 trace 找，和 trace 的分析同一段），寫成 `sample/` 底下的 folded stacks，和 `sample.md`。Chromium 在 Linux 上分析效能也用 `perf` 加 V8 的 perf map。
- **frame pointer**：`perf` profile 本身就有（「公平的條件」），取樣的 binary 就是量時間的那個。用 dwarf 展開時被 V8 的 JIT frame 擋住，只拿得到葉子的符號（2026-10-16）。Rust 的 std 沒有 frame pointer，展開經過它的函式可能斷掉。
- **JIT 的名稱**：取樣時 `MACHOPO_V8_FLAGS` 加 `--perf-basic-prof --interpreted-frames-native-stack`，V8 寫 `/tmp/perf-<pid>.map`，interpreter 裡的函式各自有一個 frame 和名字；只在取樣時開，所以取樣的那次和量時間的那次有一點不同。V8 跟著寫的 log 改寫到取樣的資料夾（`--no-logfile-per-isolate --logfile=…`，不然每個 isolate 一份寫在 kun 的工作目錄），map 和 log 讀完就刪；`--logfile=/dev/null` 會讓 kun 啟動時卡住。不用 `--perf-prof`（jitdump）：多一步 `perf inject`，沒有多的好處。
- **時鐘**：Perfetto 的時間是 CLOCK_BOOTTIME，但 `perf` 的硬體事件不能用它（`-k boottime` 對 `cycles` 是「wrong clockid」）：樣本用 CLOCK_MONOTONIC，讀的時候加上休眠過的時間（BOOTTIME 減 MONOTONIC）。
- 不選 `samply`：三個平台都能用，但要多裝一個工具，輸出和 trace 對時間麻煩。macOS、Windows 上的取樣是 `docs/limitations.md` 的限制。

### 盤點

找出 Speedometer 執行完需要、kun 還沒有的功能（第 3 條）。`python3 scripts/ci.py speedometer --inventory`（1b，2026-10-10），結果寫進 `target/speedometer-results/`（`inventory.md` 依 suite 列，`inventory.json` 是全部）。做法和記不到的東西見 `details.md` 的「盤點的做法」：
1. **Chromium 用到什麼**：Chrome for Testing 152（第 11 條）`--headless=new` 跑全部 32 個 suite 一次。每頁最前面注入記錄的 script（`record.js`）：包住每個 interface 的 prototype 和 static 成員、`window` 自己的、`CSS` 和 `console` 這類 namespace，記下每個 suite 用到的 `Interface.member`、建立的元素、listener 的事件、`dispatchEvent` 的事件；runner 的頁面在 `main.mjs` 之後注入 `inventory.mjs`，每個 test 之後收 suite 頁面的計數和 CSS（stylesheet 的 property 和值、選擇器、at-rule、computed 的 `display` 等）、script 的種類和載入的資源，跑完 `POST` 回 server。
2. **kun 自己回答它有什麼**（第 13 條）：kun 載入一頁探測的頁面（`probe.js`），對 Chrome 記到的每一項檢查：`Interface.member` 在不在 interface 的 prototype 鏈或 interface 上、宣告用 `element.style.setProperty` 設了讀不讀得回來、選擇器 `querySelector` 收不收（解析不了的 selector list 照最外層的逗號拆開、pseudo 各自試，只列單獨也解析不了的）、at-rule 的原文放進一份 constructed stylesheet 之後還在不在、`createElement` / `createElementNS` 做出來的是不是同一個 interface。比的是 kun 實際暴露給頁面的東西（glue 給的、V8 給的都算），不用寫解析 Rust 的程式，宣告表換了寫法也不用跟著改。代價是只看「有沒有」，看不出行為對不對。
3. **每個 default suite 在 kun 上跑不跑得完**：kun 一次一個地跑（`index.html?startAutomatically&iterationCount=1&suites=…`），runner 的頁面注入 `kun.mjs`，在 runner 跑完、放棄（`handleError`）、例外之後 5 秒沒有進展、或逾時時，把跑到哪裡和前 20 個例外 `POST` 回 server。kun 的每一頁是一個新的 `kun --automation-pipe`，和量測同一條路（第 20 條；1b 原本經過 `shell` 的 `speedometer` 測試和 `testRunner.reportHarnessResults`，第 12 條）。

盤點自己的誤報（2026-10-16 修好，`progress.md`）：用 `var()` 的 shorthand 在 Chrome 的 CSSOM 裡展開成值是空字串的 longhand（pending substitution），改記那一塊的 `cssText` 裡的 shorthand；SVG 的元素原本用 `createElement` 試、在 HTML 的文件裡一定是 `HTMLUnknownElement`，改成 key 帶 namespace、照 `createElementNS` 試；at-rule 和 selector 原本沒有問 kun、或整串一起試，改成上面的做法。

### 盤點的結果

2026-10-07 第一次盤點（第 3 條），細節（每一項缺的成員、用到它的 suite、CSS）見 `details.md` 的「對照表」。分三個層級：**跑得完**（不丟例外、不卡住；缺的方法和建構子會丟 `TypeError`、`ReferenceError`）、**行為對**（缺的 getter 回傳 `undefined`、setter 變成普通的屬性，app 不丟例外但做錯事）、**量得對**（async 的時間要包括真的 style、layout、paint）。

「負責」欄是做它的 RFC（`roadmap.md` 的「要開的 RFC」）。順序和里程碑 M1 到 M6 見 `roadmap.md`：先讓 runner 和只用 classic script 的 TodoMVC-JavaScript-ES5、Preact-Complex-DOM、Svelte-Complex-DOM 跑完（M1，第 6 條），接著 jQuery、Backbone、Vue、React，然後 Web Components，最後是 NewsSite、圖表、編輯器；M1 到 M6 在 2026-10-10 到 10-12 達成。

**runner 本身**（任何 suite 之前，跑得完）：

| 功能 | 為什麼 | 負責 |
| --- | --- | --- |
| ES module（靜態 `import`） | `index.html` 用 `<script type="module">` 載入 runner | RFC 0015 |
| `location`（`search`、`hash`、`href`）、設定 `hash` 和 `hashchange`、`history.pushState` | `params.mjs` 讀 `location.search`；`main.mjs` 用 `hash` 切換畫面 | RFC 0019 |
| `URLSearchParams` | 讀參數 | RFC 0019 |
| `element.style`、`style.setProperty` | 擺 iframe 的位置、設 CSS 變數 | RFC 0016 |
| `getBoundingClientRect`、`DOMRect`，script 強制的同步 layout | 每個 test 的 async 結束時讀 iframe 的 `body` | RFC 0018 |
| 子 frame 的 layout 和 paint | 上一項要子 frame 有 layout；async 要包括子 frame 的 rendering | RFC 0009 的 2b |
| `performance.mark`、`measure` | 每個 test 都呼叫 | RFC 0017 |
| `navigator` | `WakeLock` 先讀 `navigator.wakeLock`，沒有 `navigator` 時丟 `ReferenceError` | RFC 0017 |
| `KeyboardEvent`、`MouseEvent` 和 `WheelEvent` 的建構子和成員 | `PageElement` 用它們分派輸入 | RFC 0022 |
| `innerWidth`、`innerHeight`、`document.title` | 結果頁 | RFC 0018（`innerWidth`、`innerHeight`）、RFC 0017（`document.title`） |
| `innerHTML`、`Blob`、`URL.createObjectURL`、`<a>` 的 `href` | 跑完之後畫結果頁、下載連結 | RFC 0010（`innerHTML`）、RFC 0028（其他；量測不等它，第 9 條） |

**各組 suite**（default 的 20 個；括號是用到的 suite 數）：

| 功能 | 用到的 suite | 層級 | 負責 |
| --- | --- | --- | --- |
| `element.click()`（含 checkbox 的 activation behavior）、`focus()`、`activeElement` | 19 | 跑得完 | RFC 0022 |
| `KeyboardEvent`（`keydown`、`keyup`、`keypress`） | 13（TodoMVC 的 12 個和 Perf-Dashboard） | 跑得完 | RFC 0022 |
| 表單：`<input>` 的 `value`、`checked`、`type`，`<label>`、`<button>`、`<select>`、`<option>` | 18 | 行為對 | RFC 0023（控制項的繪製是 RFC 0050） |
| CSSOM：`element.style`、`getComputedStyle` | 15 | 跑得完 | RFC 0016 |
| 幾何：`offset*`、`client*`、`scroll*`、`elementFromPoint`、`scrollTo` | 9 | 跑得完 | RFC 0018 |
| `innerHTML`、`DOMParser` | 10 | 跑得完 | RFC 0010 |
| ES module、動態 `import()`、`modulepreload` | 8 | 跑得完 | RFC 0015 |
| `location`、`history`、`popstate` | 14（`location`，讀原始碼得到）、8（`history`） | 跑得完 | RFC 0019 |
| HTML 元素的成員：`dataset`、`a.href`、`title`、`tabIndex`、`autofocus` | 14 | 行為對 | RFC 0017（`dataset`）、RFC 0022（`tabIndex`、`autofocus`）、RFC 0028 |
| `navigator`（`userAgent`、`platform`） | 8 | 跑得完 | RFC 0017 |
| `MouseEvent` 的其他成員、`WheelEvent` | 8 | 行為對 | RFC 0022 |
| custom element、shadow DOM、`<template>.content`、slot | 7 | 跑得完 | RFC 0035（`<template>.content` 是 RFC 0028；shadow tree 的 style 和 layout 是 RFC 0046） |
| `Selection`、`contenteditable` | 6 | 跑得完 | RFC 0038 |
| `MutationObserver` | 4 | 跑得完 | RFC 0010 |
| `MessageChannel`、`MessagePort` | 4（React 的 scheduler） | 量得對（沒有時退回 `setTimeout`） | RFC 0034 |
| `Node.isConnected` | 4 | 行為對 | RFC 0028 |
| `URL`、`URLSearchParams` | 4 | 跑得完 | RFC 0019 |
| Canvas 2D | 3 | 跑得完 | RFC 0033 |
| constructable stylesheet、`adoptedStyleSheets` | 2 | 跑得完 | RFC 0016 的第 3 段 |
| `ResizeObserver`、`IntersectionObserver` | 3 | 跑得完 | RFC 0036 |
| `Range` | 2 | 跑得完 | RFC 0037（從 RFC 0010 移出，它的第 19 條） |
| `TreeWalker` | 1（Lit） | 跑得完 | RFC 0010 |
| `fetch`、`Response` | 1（Perf-Dashboard 載入 JSON） | 跑得完 | RFC 0027 |
| SVG 的 DOM（`getScreenCTM`、`createSVGPoint`） | 1（Stockcharts） | 跑得完 | RFC 0031 |
| 雜項：`crypto.randomUUID`、`requestIdleCallback`、`queueMicrotask`、`devicePixelRatio`、`readyState`、`getElementsByName` | 1 到 4 | 跑得完 | RFC 0028、RFC 0010（`queueMicrotask`）、RFC 0018（`devicePixelRatio`） |
| script 插入的 `<script>` 要執行 | NewsSite-Next | 跑得完 | RFC 0015 |

**量得對（rendering）**：suite 跑得完之後，畫面要對 async 才有意義。2026-10-07 時 default 的 suite 用到、kun 沒有的 CSS 很多，最普遍的是：`position`（17 個 suite）、`::before` / `::after` 的 `content`（16）、`background-image` 等背景（18）、`border-radius`（15）、`list-style`（15）、`vertical-align`（15）、`display: inline-block`（15）、`box-shadow`、`transition`（14）、`calc()`（13）、`float`（11）、`@media`（14）、`var()`（8，Complex-DOM 的 suite 各有約 7500 個自訂 property 的宣告）、`display: grid`（8）、`inline-flex`（8）、`@keyframes`（8）；選擇器的 `:hover`、`:focus`、`:checked`、`:not()`、`:nth-child()`、`:root`、`::placeholder`、`:host`；12 個 suite 的頁面裡有 inline 的 `<svg>`。完整的表見 `details.md` 的「對照表」。這些由路線圖的 CSS 線做：`position` 是 RFC 0013，背景和邊框 RFC 0026，cascade 的功能（`var()`、`calc()`、`@media`、pseudo-class）RFC 0041，`float`、`vertical-align` 和文字 RFC 0043，生成內容和 list RFC 0045，shadow tree 的 rendering RFC 0046，table RFC 0047，transition 和動畫 RFC 0048，SVG RFC 0031，grid 和 `inline-*` RFC 0003，表單控制項的繪製 RFC 0050。

**完成時（2026-10-16 的盤點）**：20 個 default suite 在 kun 上都跑得完、沒有例外；缺的 API 多半只是 feature detection 或有退路的。還丟掉、會改變畫面的 CSS（`transform: rotate()`、NewsSite 的 line clamp 和 `position: sticky`、visually-hidden 的 `clip`、Spectrum 元件的 system color 等）和表單控制項的繪製，記在 `progress.md`。

### 測試

- **工具本身**（第 22 條）：`scripts/speedometer_test.py`，標準函式庫的 `unittest`。一份假的 runner（`scripts/speedometer/fake/`）照 Speedometer 的順序呼叫 `benchmarkClient` 的 hook、照 `RAFTestInvoker` 計時，suite 是幾頁小的頁面（固定的、亂數的、丟例外、卡住、runner 放棄、open shadow root、CSS、`innerHTML` 和 custom element……）；用真的 kun 和 Chrome 確認結果 `POST` 得回來、例外和卡住和摘要不同都被標成無效、盤點的記錄和 probe、兩邊跑的順序和比較表、兩份 trace 裡的名稱和層次（`test_traces_of_both` 讀 kun 和 Chrome 的 protobuf）、`--sample` 的樣本（Linux、有 `perf` 時）；摘要、統計、讀 `perf script` 和計時區間用合成的資料測。
- **Rust 那一側**：`shell::trace` 的 layer、`bindings::trace`（V8 啟動後不開始、最外層的 script 入口）、`html` 的 `spans::`（`ParseHTML`）。
- **mutation test**：每一段各一份 spec（`mutants-*.toml`），弄壞摘要的比對、錯誤的收集、逾時、順序、統計、span 和錄製；fork 的 C++ 手動改壞。結果在 `progress.md`。
- 效能的數字本身沒有測試；量測用 `perf` profile 這一點、電源的判斷等測不到的，記在 `docs/test-gaps.md` 的「測試守不住的地方」。每次量的結果和環境記進 `progress.md`。

### 分階段

1. **盤點**（第 3 條）：
   - 1a 第一次盤點（2026-10-07，「盤點的結果」）。
   - 1b 盤點的工具放進 repo（`--inventory`），kun 能執行 runner 之後也在 kun 上跑、記下停在哪裡（2026-10-10，第 11 到 13 條）。
   - 1c 每一項功能確定負責的 RFC：`roadmap.md`（RFC 的切法、DOM / CSS / 其他雜項三條平行的線、里程碑 M1 到 M6、開始平行之前的前置工作，2026-10-07，第 4 到 10 條）。
2. **量測的工具**（第 14 到 19 條；等第一批 suite 在 kun 上跑得完才開始，M1 在 2026-10-10 達成）：
   - 2a server 和注入（`POST` 結果、DOM 的摘要）、兩邊開頁面的方式、有不有效的檢查；Chrome 跑完 20 個 default suite，摘要穩定（2026-10-11，第 20 到 24 條）。
   - 2b `perf` profile、兩邊接著跑和比較表（2026-10-11，第 25 到 30 條；第 38 條 2026-10-16 拿掉輪）。
   - 2c `MACHOPO_TRACE`、補上的 span、`--trace` 錄兩邊的 trace（2026-10-11，第 31 到 36 條）。
   - 2d 看得出 script 和 frame 裡的時間花在哪（第 39 條）：2d-1 V8 用 Perfetto 編，V8 和 kun 的事件在同一份 Perfetto 的 trace（第 40、41 條）；2d-2 span 照 Chrome 的 trace 的名稱和層次（第 42 條）；2d-3 Linux 的 `perf` 取樣和 frame pointer（第 43 條）。都在 2026-10-16。
3. **之後**：這份 RFC 不實作功能，也不排固定的量測：使用者要求時量，量測和比較記進 `progress.md`，分析最大的差距，開優化的 RFC（第 37 條；原本是每一組 suite 跑得完時就量一次）。

## 缺點

- **很久之後才有總分**：20 個 suite 涵蓋表單、shadow DOM、SVG、canvas、編輯，等於大半個瀏覽器；只比部分 suite 時，容易拿少數幾個 TodoMVC 的數字下結論。
- **和 Chrome 的比較不完全對等**：kun 少做的事（沒有 HTTP cache、沒有 sandbox、少畫的東西）有時讓 kun 顯得比較快；有效性的檢查只抓得到 DOM 看得出的差別。
- **要維護 Chrome 這邊的工具**：Chrome for Testing 的版本；Chrome 改了 headless 的行為時要跟著改（Linux 上結束要用 SIGINT 才寫得出 trace 就是一例）。Windows 上 Chrome 那一側跑不了。
- **V8 用 Perfetto 編**：fork 多四個 submodule、prebuilt 變大（release 的靜態庫多約 18 MB），升級 V8 時 submodule 跟著 `DEPS` 換。
- **針對 benchmark 優化的風險**：Speedometer 的工作負載很特定（例如一次加 100 個 todo），優化要確認在一般的頁面上也有收益。

## 理由和其他做法

- **Speedometer 3.1，不是 main**：main 在 2026-09 改了結構（共用的 `shared/` 目錄、`config.json`、新的 NewsSite-PostMessage），是還沒發布的下一版；3.1 是已經發布、Chrome 和其他瀏覽器都在量的版本。下一版發布之後再換（「之後的可能」）。
- **這份 RFC 只做工具和對照表，功能歸各自的 RFC**：Speedometer 擋住的功能橫跨 script 的載入、CSSOM、DOM、表單、rendering，放進一份 RFC 會變成整個瀏覽器的路線圖；功能本身各自有設計要決定。不選「一份 RFC 把 runner 要的功能做完」：ES module 和 iframe 的畫面各自都要先比較選項。
- **兩邊的頁面都把結果 `POST` 回 server，不用 crossbench、Puppeteer 或 WebDriver**（第 14 條，`details.md` 的「Chrome 的 driver」）：crossbench 是 Chromium 的 perf bots 用的工具，但它有自己的 Speedometer 網址和參數，要保證和 kun 跑同一份、同樣的注入，還是要自己接；Puppeteer 要 Node；WebDriver 要多下載 chromedriver，而量測只要「開頁面、等結果」。兩邊交結果的路一樣才對等，kun 也不用為了量測加選項。crossbench 可以留著交叉確認 Chrome 的數字。
- **不比總分**：部分 suite 的 geomean 不是 Speedometer 的分數，和 Chrome 的分數放在一起會誤導。
- **trace 和 Chrome 的同構**（第 40、41 條）：要和 Chrome 量得一樣準，trace 就要和 Chrome 的一樣：Chromium 和它的 V8 在三個平台上都用 Perfetto 的 SDK 發事件，V8 的 GC、compile、parse 和 Blink 的 `FunctionCall`、`Layout` 在同一份 trace 上。只接 V8 的 GC prologue / epilogue callback（G1）看不到 compile、parse；V8 寫一份、kun 寫一份 JSON 要開兩份。
- **粗的 span 加取樣，不做每個 DOM API 的 span**（第 39 條）：理由見「trace」。照 Blink 的 RuntimeCallStats 累計每個 binding 的次數和時間（D）留在「之後的可能」。
- **為什麼不從 blink_perf 開始**：blink_perf（Chromium 的 `third_party/blink/perf_tests`）每個測試只用一小組 DOM API、`PerfTestRunner` 也簡單，kun 很快就能跑一部分；但它量的是個別的 API，不是一個 app 從輸入到畫面的時間。2026-10-07 使用者選了 Speedometer 當標準，blink_perf 留在「之後的可能」。

## 先例

- **Chromium**：perf bots 用 crossbench（`third_party/crossbench`）跑 Speedometer 3，結果進 Chrome 的效能 dashboard；Chrome 用 Perfetto 的 trace 分析 Speedometer 的每一步，V8 用 Perfetto 編（`v8/gni/v8.gni`：`build_with_chromium` 時 `v8_use_perfetto = true`）；Linux 上用 `perf` 加 V8 的 perf map 取樣。
- **Speedometer 自己**：`benchmarkClient` 的 hook（`willRunTest`、`didRunTest`、`didFinishLastIteration`）就是給自動化用的，crossbench 也從這裡讀結果；這份 RFC 照這個做法，不改 Speedometer。
- **WPT 在 kun 裡的做法**：放在 kun 旁邊、固定 commit、不改（`tests/wpt/README.md`）；Speedometer 照同樣的方式，kun 要的東西由 server 注入。

## 已決定的事項

1. **用 Speedometer 3.1，釘在 `release/3.1` 的 `1386415`**（2026-10-07）：已經發布、Chrome 和其他瀏覽器都在量的版本，repo 裡有編好的 `dist/`。不選 main：2026-09 改了結構，是還沒發布的下一版，量測方式可能還會變（`details.md` 的「版本」）；也不選 3.0：已經被 3.1 取代。下一版發布之後再換（「之後的可能」）。
2. **這份 RFC 只做量測的工具和 suite 的對照表，讓 suite 能跑的功能歸各自的 RFC**（2026-10-07；什麼時候量，第 37 條改成只在使用者要求時）：對照表的每一項指向負責的 RFC，還沒有 RFC 的寫「之後」；這份 RFC 在每一組 suite 解開時記下量測和比較。不選「這份 RFC 把 runner 要的功能也做完」：ES module、CSSOM、iframe 的畫面各自都要先比較選項，放在一起會變成整個瀏覽器的路線圖。
3. **先盤點「執行完還缺什麼」，量測的工具和 Chrome 的比較延後**（2026-10-07）：kun 現在一個 suite 都跑不了，先知道要補哪些功能、照什麼順序，才有東西可以量；量測的工具（Chrome 的 driver、有不有效的檢查、比較表）等第一批 suite 在 kun 上跑得完再做（「分階段」第 2 階段）。盤點時用 headless 的 Chromium 記錄 suite 實際用到的 API，只為了知道要補什麼，不量時間（`details.md` 的「盤點的做法」）。不選「先做量測的工具」：沒有 suite 能跑時，kun 那一邊驗收不了。
4. **讓 suite 跑得完的工作分成 DOM、CSS、其他雜項三條平行的線**（2026-10-07）：照一份工作主要改哪些 crate 分，不照 API 的名稱，讓同一條線的工作改同一批檔案、不同線之間少衝突；每一項放在哪條線、每條線的順序和跨線的依賴見 `roadmap.md` 的「三條平行的線」。三條線在三台裝置上各自進行，透過 `origin` 協調。不選「照里程碑一份一份依序做」：M1 的關鍵路徑（A、E-1 和 D）都要先比較設計，依序做的話其他直接的工作全部卡在後面。
5. **`roadmap.md` 的代號 A 到 T 各自一份 RFC，H 拆成 H1 和 H2**（2026-10-07）：H1 是 M1 要的最小部分（`navigator`、`dataset`、`document.title`、`performance.mark` / `measure`），H2 是其他。`Blob` 原本放在 H1，同一天移到 H2：runner 只在跑完時把它交給 `URL.createObjectURL`，而 `URL` 在 B，H1 單獨做 `Blob` 在 M1 用不到，它的讀取方法還要 Promise 和 `BufferSource` 兩種新的宣告表形式。H1 小、照 RFC 0008 的做法，DOM 線的第一份能很快做完、merge，順便驗證三條線並行的流程（每個階段做完就可以 merge，見 `rfcs/README.md` 的「流程」第 5 步）。不選「H 整份一份 RFC、分階段做」：H 的成員彼此無關，放在一份裡，M1 要的部分要等整份 RFC 的設計確認。
6. **M1 是 runner 加上 TodoMVC-JavaScript-ES5、Preact-Complex-DOM、Svelte-Complex-DOM 跑得完**（2026-10-07）：這三個 suite 只用 classic script，除了 runner 要的功能只多要幾項（「盤點的結果」的「建議的順序」）；之後的里程碑見 `roadmap.md` 的「里程碑」。
7. **新的 API 照 RFC 0008 的規則，同時給 JS 和 wasm；例外在各自的 RFC 決定**（2026-10-07）：不改 RFC 0008 的規則。某個 API（例如 `location`、CSSOM）要先只給 JS 時，在負責它的 RFC 的「已決定的事項」寫明理由，並記進 `docs/limitations.md`。不選「路線圖上的新 API 一律先只給 JS」：工作量少一半，但 wasm 文件會長期落後，而且 DOM 線的 API 和 engine 無關，給 wasm 的額外成本主要在 WIT；也不選「一律兩邊都給、沒有例外」：導覽、CSSOM 這類 API 在 wasm 文件裡可能還沒有用處。
8. **M1 的兩條關鍵路徑先開 A（ES module），E-1 和 D 同時寫草稿**（2026-10-07）：A 做完 kun 就能真的執行 runner，看到實際的錯誤，盤點（「分階段」的 1b）才能在 kun 上跑；E-1 和 D 牽涉 renderer 的架構，設計要先比較，所以和 A 同時開始寫草稿。三條線的第一份 RFC：雜項線是 A（RFC 0015），CSS 線是 C（RFC 0016），DOM 線是 H1（RFC 0017）。
9. **M1 不包括 runner 的最後一步**（2026-10-07）：runner 在 `didFinishLastIteration` 用 `URL.createObjectURL(new Blob(...))` 做結果頁的下載連結，`Blob` 在 H2、`URL` 在 B，所以 M1 的功能做完時這一步仍然丟例外；那時所有的 test 都已經量完，量測的工具注入的 hook 包住 `didFinishLastIteration`，在原本的實作之前拿走結果（「本機的 server 和注入的 script」），M1 不等它。不選「把 `Blob` 的最小部分和 `createObjectURL` 提前排進 M1」：只為了結果頁，要讓 H2 和 B 的一部分提前。
10. **`kun:dom` 的 WIT 不再協調「一批只升一次版」，每次 merge 照 `crates/dom_api/README.md` 的規則各自判斷**（2026-10-07）：有不相容的改動就升 minor，只新增的不升，不看別的線是不是已經升過、還沒發布（例外照 RFC 0010 第 17 條的先例：改的是還沒發布的版本才新增的東西，不升）；後 merge 的一方 rebase 時 `crates/dom_api/tests/machopo-dom.wit` 有衝突，就重新產生 golden file 和 guest，再照同一個規則判斷。不選「三條線同一批合併只升一次」：每個階段都能 merge 之後（`rfcs/README.md` 的「流程」第 5 步），「一批」沒有明確的邊界，三台裝置之間也不好協調；版本是 0.x，多升幾次的代價只是 guest 重新編。
11. **盤點用 Chrome for Testing 152**（2026-10-10，1b 開始前）：`ci.py` 下載、釘住 `152.0.7977.82`（和 kun 連結的 V8 15.2 同一個 milestone），放在 `target/` 底下。第 2 階段的比較本來就要用它，盤點和量測用同一個 Chrome，結果可以重現，三個平台都能用同樣的方式取得。不選系統的 Chrome / Chromium：版本隨機器而變，盤點不能重現；不選 Playwright 的 Chromium：要 Node 或 Python 的套件（`ci.py` 只用標準函式庫），版本也不是 152。1a 用的是 Windows 上的 Edge 154。
12. **kun 經過現有的 `testRunner.reportHarnessResults` 交出結果，不加 `kun` 的選項**（2026-10-10，1b 開始前）：注入的 script 收集例外和進度，結束時交給 `testRunner`，由 `shell` 的 `speedometer` 測試（`headless::run_testharness`，和 WPT 的 runner 同一條路）拿回來；`ci.py` 用環境變數把頁面交給它。第 2 階段的 `testRunner.reportBenchmarkResults` 和 `kun` 的選項（「兩邊的 driver」）照原本的設計到那時再做。不選「現在就做 `--run-benchmark`」：1b 只要跑得完和第一個錯誤，提前做要先決定第 2 階段的比較組態等「待決定」；不選「經過 machopodriver（WebDriver）」：`ci.py` 要多一個 WebDriver 的 client，量時間時也多一層開銷，最後還是要換掉。
13. **kun 有什麼由 kun 自己回答，不拿宣告表比對**（2026-10-10，1b 做完時確認）：kun 載入探測的頁面（`probe.js`），對 Chrome 記到的每一項檢查：成員在不在 interface 的 prototype 鏈或 interface 上、宣告用 `element.style.setProperty` 設了讀不讀得回來、選擇器 `querySelector` 收不收、`createElement` 做出來的是不是同一個 interface。比的是網頁實際看得到的東西（`dom_js` 的 glue 給的、V8 給的都算），不用寫解析 Rust 的程式，宣告表換了寫法也不用跟著改。不選 1a 的做法（拿 `dom_api` 的宣告表和 `style` 的白名單比對）：要解析 Rust 的原始碼，還會漏掉宣告表以外的來源。兩種做法都只看「有沒有」，看不出行為對不對。
14. **R1：兩邊的頁面都把結果 `POST` 回 server，kun 經過 `--automation-pipe` 開頁面**（2026-10-11，第 2 階段開始前）：注入的 script 在 `didFinishLastIteration` 用 `fetch` 把結果送回 `ci.py` 的 server，兩邊同一段程式；Chrome 照 1b 用 `--headless=new <URL>`；kun 用 `kun --automation-pipe`（沒有平台視窗的瀏覽器視窗，真的 16.7 ms 的 frame），`ci.py` 從 stdin 送一個 `Page.navigate`。改掉原本的設計（Chrome 用 CDP 的 WebSocket，kun 加 `testRunner.reportBenchmarkResults` 和 `--run-benchmark`）：它的理由是 kun 送不出結果，RFC 0027 之後不成立；兩邊交結果的路一樣才對等，kun 也不用加選項。Chromium 的 perf bots 用的 crossbench 也是讓 Speedometer 自己跑完，再從外面拿 `benchmarkClient` 的結果。不選 R2（兩邊都走 CDP over pipe：讀結果的方法兩邊不同，Python 要接 fd 3、4）、R3（chromedriver 和 machopodriver：要多下載 chromedriver，量測用不到 WebDriver 的其他東西）、R4（原本的設計）。
15. **I1：server 在 `index.html` 和每個 suite 的頁面注入 script，不 fork Speedometer**（2026-10-11，第 2 階段開始前）：1b 已經這樣做；和 WPT 一樣不改上游，換版本時不用 rebase。
16. **V1：有不有效看例外、卡住和 DOM 的摘要**（2026-10-11，第 2 階段開始前）：每個 test 之後走過 iframe 的 document，各 tag 的元素數、文字的總長度、class 的計數和 Chrome 的比；2a 用 Chrome 跑兩次調到穩定。不選 V2（只看例外）：抓不到 app 沒有反應、卻沒有丟例外的情況，現在跑得完的 14 個 suite 裡有沒有這種就看不出來。
17. **C1：比較的組態是兩邊都沒有視窗**（2026-10-11，第 2 階段開始前）：kun 是 `--automation-pipe` 的沒有平台視窗的瀏覽器視窗（Viz 每 16.7 ms 一個 frame、CPU raster），Chrome for Testing 152 `--headless=new`（60 Hz）；兩邊的視窗 1200×900（Speedometer 的 iframe 照預設 800×600）；kun 用專門的 build profile（繼承 release，`lto = "thin"`、`codegen-units = 1`）；預設 3 輪、每輪 `iterationCount=10`，kun、Chrome 輪流，每輪新的 process，報中位數和 95% 信賴區間（輪在第 38 條拿掉，只跑一輪）。不選 C2（兩邊都開真的視窗）：kun 的真視窗要 GPU、在最上層，不適合跑很多輪，CI 也不能跑。
18. **M1：主要的比較在 macOS（Apple Silicon）上做**（2026-10-11，第 2 階段開始前）：kun 的驗收平台，最近的盤點也在這台；其他機器照樣能跑，數字記下平台。不選 M2（原本寫的 Windows 11 那台）。
19. **T1：Perfetto 的 trace 留在這份 RFC，當 2c**（2026-10-11，第 2 階段開始前）：分析差距一定要用到；Chrome 那邊用 `--trace-startup` 這類旗標寫檔，不需要 CDP；kun 那邊怎麼把 `tracing` 輸出成 trace（可能要新的依賴）在 2c 開始前列選項。不選 T2（另開一份 RFC）。
20. **S1：盤點的 kun 那一邊也改走 `--automation-pipe` 和 `POST`**（2026-10-11，2a 開始前）：拿掉 `shell` 的 `speedometer` 測試 binary 和它經過 `testRunner.reportHarnessResults` 的路（改第 12 條），工具只有一條跑 kun 的路，盤點跑的就是量測的組態。不選 S2（兩條路並存）：做同一件事的兩份程式。
21. **D1：每個 iteration 的每個 test 都算 DOM 的摘要，和 Chrome 第一個 iteration 的比**（2026-10-11，2a 開始前）：kun 的每個 iteration 和 Chrome 自己後面的 iteration 都和它比，Chrome 自己不穩定也看得出來；容許的差異等 Chrome 跑兩次、看過實際的差別再定。Speedometer 和 crossbench 都不檢查 DOM，這是 kun 這邊為了「少做事顯得比較快」加的。不選 D2（只比最後一個 iteration）：中間某一輪沒有反應時看不出來。
22. **P1：工具本身用標準函式庫的 `unittest` 測**（2026-10-11，2a 開始前）：`scripts/speedometer_test.py`，一份假的 runner 頁面呼叫和 Speedometer 一樣的 `benchmarkClient` hook，用真的 kun 和 Chrome 確認結果 `POST` 得回來、例外和卡住和摘要不同都被標成無效，另外測摘要的計算；mutation test 照樣用 `test.py mutate`。不選 P2（`shell` 的 Rust 測試呼叫 Python 的工具）：測試和工具分在兩個地方，Python 的程式大多測不到。
23. **K1：指令照「設計概要」**（2026-10-11，2a 開始前）：`ci.py speedometer` 是比較，`--browser chrome|kun` 只跑一邊，`--suites`、`--iterations`、`--inventory` 照舊，`--reuse-chrome` 在比較時沿用上次 Chrome 的摘要（只用來檢查有不有效）。2a 做完時只跑一邊和有不有效的檢查能用，輪流跑和比較表在 2b。不選 K2（先另做一個檢查的指令，2b 再合併）。
24. **每個 suite 一個 process**（2026-10-11，2a 實作中）：兩邊都用 `suites=<一個>` 開一個新的瀏覽器，連續跑 `--iterations` 次。kun 在一個 suite 卡住或當掉只少那一個；只比各 suite 的時間（不算總分，見「理由和其他做法」），suite 之間的 heap 和 JIT 的狀態影響不到別的 suite。和 Chrome 的 perf bots、browserbench.org 一個頁面依序跑全部 suite 不同。不選「一個頁面跑全部 suite」：比較接近真正的分數，但 kun 在一個 suite 卡住時整輪都沒了，要先知道哪些 suite 有效才能跑。
25. **B1：kun 用新的 `perf` profile 量**（2026-10-11，2b 開始前）：繼承 release，`lto = "thin"`、`codegen-units = 1`（第 17 條），`debug = "line-tables-only"`（不影響產生的程式，2c 或 Instruments 找熱點時看得到函式和行號），在 `target/perf/`。只有量測用它，`--inventory` 和工具自己的測試照舊用 dev。Chromium 的 official build 是 ThinLTO 加 PGO，kun 沒有 PGO，比較表註明。不選 B2（改 Cargo 內建的 `bench`：和 `cargo bench` 混在一起）、B3（只用 release：沒有 LTO，差距多算進 kun 少做的最佳化）。
26. **O1：以 suite 為單位輪流**（2026-10-11，2b 開始前；第 38 條拿掉輪之後只剩 kun 先、Chrome 後）：每一輪裡 kun 跑 S1、Chrome 跑 S1、kun 跑 S2……，下一輪換 Chrome 先；同一個 suite 的兩邊時間上靠得最近，漂移的影響兩邊差不多，先後的偏差互相抵消。不選 O2（kun 跑完全部 suite 再換 Chrome）：同一個 suite 的兩邊相隔好幾分鐘。
27. **W1：每個 iteration 都算，第一個也算**（2026-10-11，2b 開始前）：照 Speedometer 自己的算法；正式的 Speedometer 裡每個 suite 的第一個 iteration 也是冷的（2a 量到第一個 iteration 慢很多，例如 Chrome 的 TodoMVC-Vue 48.8 ms，之後約 11.6 ms）。詳細的結果另外列第一個 iteration 的中位數。不選 W2（丟掉第一個）：比較穩，但不是 Speedometer 量的時間。
28. **S1：中位數和不假設分布的 95% 信賴區間**（2026-10-11，2b 開始前）：每個 suite 的樣本是每個 iteration 的 suite 總和（預設 3 輪 × 10 個 iteration；第 38 條之後是一輪的 10 個），信賴區間用順序統計量（二項分布）算，比值是兩邊中位數的比；`measure.md` 一張 suite 的表（kun、Chrome、比值）、一張 test 的表（兩邊 sync、async 的中位數，看差距在 script 還是 rendering）。不選 S2（照 Speedometer 用平均和 t 分布）：容易被少數慢的 iteration 拉高。
29. **V1：任何一輪、任何一邊無效，那個 suite 就不給數字**（2026-10-11，2b 開始前；第 38 條之後只有一輪，任何一邊無效就不給）：表上寫第一個原因；Chrome 每一輪都和它第一輪的第一個 iteration 比，輪與輪之間也要相同。不選 V2（只用有效的輪）：數字偏向跑得順的那幾次。
30. **E1：每次記下環境**（2026-10-11，2b 開始前）：日期、kun 的 commit 和 profile、Chrome for Testing 的版本、OS 的版本、CPU 的型號和核數、有沒有接電源（沒接時警告、照樣跑），寫進 `measure.md` 和 JSON，量到的數字記進 `progress.md`。不選 E2（只記 commit 和版本）。兩種 frame 節奏不另外量：第 17 條定了兩邊都是真的 60 Hz。
31. **F1：kun 的 trace 用 `tracing-chrome` 寫成 Chrome 的 trace event JSON**（2026-10-11，2c 開始前；第 41 條改成 Perfetto 的 protobuf、拿掉 `tracing-chrome`）：`shell` 的 `tracing-subscriber` 多一層；Perfetto UI 和 `chrome://tracing` 都能開，和 Chrome 錄的同一種格式、同一個畫面（Chrome 的是 Perfetto SDK，也匯出成這種 JSON）。不選 F2（自己寫那一層，約一兩百行）、F3（`tracing-perfetto` 的 protobuf：比較少人用）。
32. **T1：kun 用環境變數 `MACHOPO_TRACE=<檔案>` 打開**（2026-10-11，2c 開始前）：視窗、`--headless`、`--automation-pipe` 和測試都能用，寫進 `--help` 和 README 的環境變數表。不選 T2（`--trace-file` 選項：每種模式都要收）。
33. **S1：補上看得出時間花在哪的 span，用 Chrome 的 trace 的名稱**（2026-10-11，2c 開始前）：renderer 的每個 task，呼叫 JS 的入口（事件 listener、timer、rAF、microtask checkpoint），pre-paint、hit test；新的和已有的小寫名稱（`recalc_style`、`layout`、`paint`、`update_box_tree`）都改用 Chrome 的 trace 裡對應的名稱（`FunctionCall`、`TimerFire`、`FireAnimationFrame`、`Document::UpdateStyleAndLayoutTree` 等），兩份 trace 對得起來。實作時照 Chrome 152 的 trace 裡實際有的名稱（2026-10-11）：`ThreadControllerImpl::RunTask`、`EvaluateScript`、`EventDispatch`、`FunctionCall`、`RunMicrotasks`、`TimerFire`、`PageAnimator::serviceScriptedAnimations`、`FireAnimationFrame`、`HitTest`、`ProxyMain::BeginMainFrame`、`LocalFrameView::UpdateLifecyclePhases`、`LocalFrameView::RunStyleAndLayoutLifecyclePhases`、`Document::UpdateStyle`、`Layout`、`PrePaint`、`Paint`；box tree 是 `StyleEngine::RebuildLayoutTree`（Blink 的函式名稱）。不選 S2（只用現有的 span：看不到 script 的時間花在哪個入口）。
34. **R1：`--trace` 另外跑一次**（2026-10-11，2c 開始前）：每個指定的 suite 兩邊各跑一次（1 輪，iteration 數照 `--iterations`），trace 存在 `target/speedometer-results/trace/`，時間不放進比較表（錄 trace 本身會拖慢）。不選 R2（量測的同時錄：數字被 trace 的開銷污染）。
35. **C1：Chrome 的 trace 用啟動的旗標錄**（2026-10-11，2c 開始前）：`--trace-startup=<分類>`、`--trace-startup-file`、夠長的 `--trace-startup-duration`，分類是 Chrome 的預設加上 `devtools.timeline`（DevTools 效能面板的 `FunctionCall`、`UpdateLayoutTree`、`Layout`、`Paint`）；結束時檔案寫不完的話改用 C2，回報再定。實作時（2026-10-11）：被結束時 JSON 格式寫不出來（Chrome 結束時才轉換），Perfetto 的 protobuf 邊錄邊寫，所以 Chrome 的 trace 是 `.pftrace`；Chrome 收到結束的要求後要 5 到 15 秒才寫完，錄 trace 時等到 60 秒；分類是 `toplevel`、`blink`、`v8`、`cc`、`viz`、`benchmark`、`loading`、`devtools.timeline`（預設的全部會讓檔案大很多）。不選 C2（經過 `--remote-debugging-pipe` 用 CDP 的 `Tracing`：Python 要接 fd 3、4，第 14 條不用 CDP 的理由之一）。
36. **J1：JS 裡面的熱點不在 2c**（2026-10-11，2c 開始前；Linux 上的取樣是第 43 條的 `--sample`）：kun 的 trace 只到呼叫 JS 的入口；接 V8 的 CPU profiler 要改 rusty_v8 的 fork（它沒有公開），留在「之後的可能」，在那之前用 Instruments 或 samply 對 `perf` build 取樣。不選 J2（2c 一起改 fork）。
37. **Speedometer 平常不量，只在使用者要求時跑**（2026-10-13）：RFC 的測試計畫不排量測，讓 suite 變有效時、優化的 RFC 做完時也不自動量；規則寫在 `docs/benchmarks.md` 的「效能的量測」。不選原本的「讓新的 suite 變有效時、優化前後都量」：kun 比 Chrome 慢好幾倍，一般改動的幾 % 看不出來，兩邊輪流量好幾輪一次就是幾十分鐘，量的次數多過它帶來的資訊（RFC 0043 第 2b 段把 `main` 和 branch 輪流量了四次，結論是量不出差別）。
38. **拿掉 `--rounds`，一律只跑一輪**（2026-10-16）：每個 suite 在 kun 跑完接著在 Chrome 跑，各一次 `--iterations` 個 iteration；`kun-runs.json`、`chrome-runs.json` 每個 suite 一次 run（不再是每輪一份的清單，也沒有 `rounds`），無效的原因不再寫第幾輪。原本預設 3 輪、每輪換先後（第 17、26、28、29 條），但實際的量測幾乎都用 `--rounds 1`（3 輪一次要半個小時以上，第 37 條），中位數和 95% 區間用一輪的 10 個 iteration 也算得出來（x(2) 到 x(9)）。不選「固定 3 輪、只拿掉選項」：預設的 3 輪正是大家避開的；不選「一輪、每個 suite 換先後」：要比的是同一個 suite 的兩邊，一輪裡換先後抵消不了什麼，保持和原本 `--rounds 1` 一樣的順序，新舊的數字比得起來。要更多樣本時加 `--iterations`。
39. **script 和 frame 裡的時間：補粗的 span 和取樣，不做每個 DOM API 的 span**（2026-10-16，2d 開始前）：2026-10-16 的 trace（`progress.md`）裡，kun 的 `FunctionCall` 的 self time 包含 Chrome 另外標出的工作（fragment 的 parse，Chrome 的 `ParseHTML`；GC；custom element 的 reaction），script 的差距因此被高估；`ProxyMain::BeginMainFrame` 裡 lifecycle 以外的步驟沒有 span，計時區間裡這段 kun 925 ms、Chrome 213 ms，看不出是哪一步。選 A：這幾種一次就很重的工作和 `BeginMainFrame` 的每一步補上 span，有對應的用 Chrome 的名稱（第 33 條），兩邊的 trace 逐項可比；加上 C：script 裡是哪個 API、哪段程式，用取樣回答（frame pointer 的 `perf` build，加 V8 的 `--perf-basic-prof` 讓 JIT 的函式有名字；2026-10-16 用 dwarf 展開被 JIT 的 frame 擋住，只拿到葉子的符號）。Chrome 也一樣：trace 只標重的工作，JS 和 DOM API 裡面的時間靠取樣（V8 的 CPU profiler）。不選 B（每個 DOM API 呼叫一個 span）：jQuery 一個 test 就有幾十萬次呼叫，很多本身不到 1 µs，開了 trace 的時間失真、檔案很大，關了也在 binding 最熱的路徑上多一次檢查；Chrome 不這樣做。D（照 Blink 的 RuntimeCallStats，每個 binding 累計次數和時間）留在「之後的可能」，取樣不夠用時再做。
40. **GC 和 V8 的其他事件：V8 用 Perfetto 編，V8 和 kun 的事件進同一份 Perfetto 的 trace（G2'）**（2026-10-16，2d-1 開始前）：要和 Chrome 量得一樣準，trace 就要和 Chrome 的一樣：Chromium 和它的 V8 在三個平台上都用 Perfetto 的 SDK 發事件（`v8/gni/v8.gni`：`build_with_chromium` 時 `v8_use_perfetto = true`），V8 的 GC 的各個階段、compile、parse、deopt 和 Blink 的 `FunctionCall`、`Layout` 在同一份 trace 上。做法：fork 加上 V8 的 `DEPS` 釘的 `third_party/perfetto` 和 `third_party/protobuf`，`.gn` 設 `v8_use_perfetto = true`（`v8_use_perfetto_json_export` 關掉，不把 Trace Processor 編進去，只寫 protobuf）；fork 包 libplatform 的錄製（`InitializeForPerfetto`、`StartTracing`、`StopTracing`）；kun 的 span 也寫進同一個 Perfetto 的錄製，成為一份 trace。原本選的 G2（2026-10-16）：沿用 `v8_use_perfetto = false`，fork 加一個把舊的 `TracingController` 介面（`AddTraceEvent`）轉給 Rust 的子類別。做完、在本機編好 V8 試錄 jQuery，只錄到 `V8.DeserializeContext` 和 `V8.DeserializeIsolate`：V8 新式的 `TRACE_EVENT(...)` 和 GC 的 `TRACE_GC*`（V8 裡 382 處）在沒有 Perfetto 時是空巨集（`trace-event-no-perfetto.h` 的 `INTERNAL_TRACE_IGNORE`），經過舊介面的只剩 16 處、幾乎都是 snapshot；V8 的 runtime call stats 在 fork 的設定裡也關著（`v8_enable_runtime_call_stats = false`）。已經還原。不選 G1（V8 的 GC prologue / epilogue callback）：只有 GC 停下 main thread 的那段，compile、parse 這些還是看不到，和 Chrome 的 trace 對不起來。代價：fork 多兩個 submodule、prebuilt 變大；rusty_v8 沒有人用 Perfetto 編過 V8，可能要修建置；V8 獨立建置用的 `v8_libperfetto` 註明「Support non-POSIX platforms」是 TODO（可能已經過時），Windows 要實際編過才知道；升級 V8 時兩個 submodule 跟著 `DEPS` 換。
41. **`MACHOPO_TRACE` 是一份 Perfetto 的 trace：fork 自己的錄製，kun 的 span 寫進去**（2026-10-16，2d-1b 開始前）：fork 加一小段 C++（照 d8：`perfetto::Tracing::Initialize` 用 in-process backend，再開一個 session），buffer 的大小由 kun 給（64 MB），邊錄邊寫進檔案（`write_into_file`，每秒一次），結束時寫完剩下的；buffer 是 ring buffer（原本選滿了就停的 `DISCARD`，做的時候 Perfetto 的統計 `config_write_into_file_discard` 說它配 `write_into_file` 會默默丟資料，改了；邊寫邊清，只要寫出的速度跟得上就不會覆蓋）；kun 的 span 經過 `shell::trace` 的 layer 呼叫 fork 的 begin / end（Perfetto 的 `TRACE_EVENT_BEGIN` / `TRACE_EVENT_END`，category `kun`，名稱是動態字串），和 V8 的事件在同一份 trace、同一個 thread 和時鐘上，拿掉 `tracing-chrome`。開的 category 和 Chrome 那一側一樣（`CHROME_TRACE_CATEGORIES`），加上 `kun`；V8 的 GC 事件在 `devtools.timeline,disabled-by-default-v8.gc` 這組，開了 `devtools.timeline` 就錄得到，和 Chrome 一樣。設了 `MACHOPO_TRACE` 時 `shell` 在 V8 初始化之前開始錄（V8 只在 Perfetto 已經初始化時註冊它的事件），`main` 結束時停；沒設時不碰 Perfetto。程式：fork 是 C++ 的四個函式和包成安全的 Rust 模組，kun 的 `bindings` 提供、`dom_js` 轉手、`shell::trace` 使用，kun 這邊沒有新的 `unsafe`。不選 libplatform 的 `TracingController`（`InitializeForPerfetto`、`StartTracing`）：它把 buffer 寫死成 4 MB 的 ring buffer（`set_size_kb(4096)`），滿了默默覆蓋最舊的事件，一個 suite 的 Chrome trace 就有 6 MB；改它要改 V8 本身，而 `Tracing::Initialize` 和輸出的檔案還是要自己寫。不選 V8 寫一份 `.pftrace`、kun 照舊寫 JSON：要開兩份，和 Chrome 的不同構。renderer 拆成自己的 process 之後，每個 process 各錄一份（Chrome 由 browser process 當 tracing service 收成一份），到時候再處理。
42. **2d-2 的 span：照 Chrome 的 trace 裡實際的名稱和層次**（2026-10-16，2d-2 開始前）：名稱和層次查 2026-10-16 錄的 Chrome trace 定。(1) **`ParseHTML`**：Chrome 的 fragment 和文件的 parse 都在 `HTMLDocumentParser::append` 底下發 `ParseHTML`（jQuery 3783 次、371 ms）；kun 的 fragment 先 parse 到暫時的 `<html>`、再把 children 搬進 `DocumentFragment`（Chrome 直接建在 fragment 裡），span 包住從 markup 到 fragment 的整段，搬的成本也算在裡面，文件的 parse 包在 `html::parse_document_with`。不選只包 `html` 的 parse：搬的時間會留在 `FunctionCall` 的 self time 裡，和 Chrome 的 `ParseHTML` 比不起來。(2) **呼叫 JS**：Chrome 每次呼叫都是 `v8.callFunction`，stack 上還沒有 JS 時裡面再一個 `FunctionCall`（listener 在 `EventDispatch` > `v8.callFunction`，沒有 `FunctionCall`），建構函式是 `v8.callAsConstructor`；kun 照做，`dom_js` 用 thread-local 的深度判斷最外層，進入 JS 的入口（呼叫、建構、執行 script 和 module）都算一層。不選維持每次呼叫都是 `FunctionCall`：合計不變，但巢狀的 `FunctionCall` 在 Chrome 那一側沒有對應。(3) **custom element**：Chrome 每個元素的 reaction 是 `CustomElementReactionQueue::invokeReactions`（callback 是它底下的 `v8.callFunction`，upgrade 是 `v8.callAsConstructor`），`define` 是 `CustomElementRegistry::define`；kun 照做。第 39 條說 reaction 算在 `FunctionCall` 的 self time 裡不完全對：callback 本來就有自己的 span，沒標出來的是 upgrade 的建構函式和 `define`。(4) **`BeginMainFrame`**：Chrome 的 `ProxyMain::BeginMainFrame` 包住整個 frame（rAF 在 `WebFrameWidgetImpl::BeginMainFrame` > `Blink.Animate.UpdateTime` > `PageAnimator::serviceScriptedAnimations`），底下是 `WebFrameWidgetImpl::UpdateLifecycle`、`LayerTreeHost::DoUpdateLayers`、`LocalFrameView::UpdateViewportIntersectionsForSubtree`、`ProxyMain::BeginMainFrame::commit`；kun 的原本只從 `update_rendering` 的中間開始。選 A：包住整個 `begin_main_frame`，每一步一個 span，有對應的用 Chrome 的名稱，沒有的用 kun 的 `Type::method`。不選 B（只在 `update_rendering` 裡補）：rAF 留在 frame 外面，兩邊的樹對不起來。每個 frame 多十個左右的 span，沒開 trace 時每個是一次已經快取的 interest 檢查。(5) **測試**：假 suite 多一個 test（`innerHTML`、custom element 的 callback 和 upgrade、`IntersectionObserver`），`test_traces_of_both` 讀 kun 的 `.pftrace` 查每個名稱和層次，mutation test 一次拿掉一個 span。不選在各個 crate 用收集 span 的 subscriber 測：散在幾個 crate，也測不到 trace 裡的名稱和層次。
43. **2d-3 的取樣：Linux 的 `perf`、frame pointer 的 `perf` profile、只算計時的區間**（2026-10-16，2d-3 開始前；`perf` 的硬體事件不能用 `-k boottime`，實作改成 `-k monotonic` 再換算，見「取樣」）：(1) **工具**：Linux 用 `perf`（已經在、`-k boottime` 和 Perfetto 同一個時鐘；Chromium 在 Linux 上分析效能也用 `perf` 加 V8 的 perf map）。不選 `samply`：三個平台都能用，但要多裝一個工具，輸出是 Firefox Profiler 的格式，和 trace 對時間麻煩。macOS、Windows 先記成限制。(2) **frame pointer**：`perf` profile 在 Linux 和 macOS 一律加 `-C force-frame-pointers=yes`（`speedometer.py` 編的時候用 `RUSTFLAGS` 給，只有它編這個 profile），和 Chrome 在這兩個平台上一樣（`build/config/compiler/compiler.gni` 在 Linux、macOS 開 frame pointer，fork 的 V8 也是），取樣的 binary 就是量時間的那個；代價是重編一次，執行慢一點（一般估計 1% 上下，Chrome 付同樣的）。Rust 的 std 沒有 frame pointer，展開經過它的函式可能斷掉。不選只在取樣時另外編一份：多一份完整的 build，取樣的也不是量時間的那個。(3) **JIT 的名稱**：取樣時 `MACHOPO_V8_FLAGS="--perf-basic-prof --interpreted-frames-native-stack"`，V8 寫 `/tmp/perf-<pid>.map`，interpreter 裡的函式各自有名字；只在取樣時開，所以取樣的那次和量時間的那次有一點不同。不選 `--perf-prof`（jitdump）：多一步 `perf inject`，沒有多的好處。(4) **樣本**：取樣時同時錄 `MACHOPO_TRACE`，只留 renderer main thread 上、在計時區間裡的樣本，和 trace 的分析同一段。不選全部：載入和 compile 會混進來。(5) **工具**：`ci.py speedometer --sample`，只在 Linux、只跑 kun（Chrome for Testing 沒有符號），每個 suite 寫 `perf.data`、計時區間裡的 folded stacks，和 `sample.md` 的 self、inclusive 前幾名。不選只在文件寫手動的指令：每次自己對時間、篩區間，容易錯。(6) **span**：`update_all_lifecycle_phases` 在 paint 之後的 layer 化是 `PaintArtifactCompositor::Update`（Chrome 的名稱），`FrameView::take_canvas_textures`、`paint::main_thread_scroll_hit_test_region`、`FrameView::set_wheel_event_handler_region` 用 kun 的名稱；`RunTask` 自己的時間用取樣看。(7) **測試**：`speedometer_test.py` 在 Linux、`perf` 能用時用假 suite 跑 `--sample`，folded stacks 裡要有假 suite 的 JS 函式和 kun 的 Rust 函式；新的 span 加進 `test_traces_of_both`。

## 待決定

無：第 1 到 43 條都已決定。剩下的一項「每天的 CI 上檢查預期有效的 suite 還有沒有效」2026-10-11 決定先不定，移到「之後的可能」；路線圖的待決定見 `roadmap.md`。

## 之後的可能

- **優化**：各自開 RFC。完成時最後一次的 trace（`progress.md` 的「量測的紀錄」）裡，差距由大到小是 JS 呼叫的 DOM API（節點和 wrapper 的查表、cppgc 的 handle、配置）、style 的重算、fragment 的 parse、paint 之後每個 frame 走整棵樹的兩個 region；和 Blink 的做法對照見 `architecture-audit.md`。
- **CI 上檢查預期有效的 suite**：每天的 CI 跑「預期有效的 suite 還有沒有效」（不看時間），守住已經解開的 suite 不退步。2026-10-11 決定先不定（第 2 階段開始前的第 7 項選 K2）；要它的 RFC 可以接手，那時決定預期有效的清單放在哪、CI 上有沒有 Chrome（摘要要和 Chrome 的比）。
- **Speedometer 的下一版**：發布之後換版本，對照表重新盤點。
- **blink_perf**：用 Chromium 的 `PerfTestRunner` 量個別的 DOM、CSS、layout 操作，當 Speedometer 之外的 micro benchmark，找到瓶頸之後用它驗證優化。
- **其他 benchmark**：JetStream（JS 和 wasm，主要量 V8，kun 和 Chrome 應該接近，可以檢查 bindings 和 event loop 的開銷）、MotionMark（繪製）。
- **JS 的熱點進 kun 的 trace**：接 V8 的 CPU profiler（rusty_v8 要加 binding，第 36 條）；現在 Linux 上用 `--sample`。
- **每個 binding 的次數和時間**：照 Blink 的 RuntimeCallStats（`RUNTIME_CALL_TIMER_SCOPE`）在 `dom_js` 的入口累計，結束時印出表；取樣（第 39 條）不夠用時再做。
- **macOS 和 Windows 上的取樣**：`--sample` 只在 Linux（`perf`）；其他平台要自己用 Instruments 或 samply（`docs/limitations.md`）。
- **效能的回歸追蹤**：固定一台機器，定期跑，記錄每個 commit 的數字。
