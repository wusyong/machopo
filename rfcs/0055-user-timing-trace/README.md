# RFC 0055：User Timing 寫進 trace，Speedometer 的計時區間照 runner 的 mark

- 開始日期：2026-10-18
- 狀態：✅ 已完成：2026-10-18（`performance.mark` 是 renderer main thread 上的 instant、`measure` 是它底下自己的 track 上的 slice，`blink.user_timing`，和 Chrome 152 一樣；fork 的 `kun-v152.2.0-11`；`--sample` 的計時區間照 runner 的 mark，沒有 mark 時照 runner 的 frame，報告寫明用哪一種；三段，在 macOS 上做）
- 細節：`details.md`（原本的計時區間算多了多少、照 frame 找的準確度、Chrome 的 user timing 事件）
- 進度和驗收：`progress.md`

## 摘要

`ci.py speedometer` 分析 kun 的 trace 和 `--sample` 的取樣時，只算 Speedometer 的 runner 計時的區間，原本的區間由 `scripts/speedometer.py` 的 `timed_windows` 從 trace 的結構猜，多算了約 39%。runner 自己用 `performance.mark` 標出每個 test 的開始和結束，Chrome 把這些 mark 寫進 trace（`blink.user_timing`）。這份 RFC 讓 kun 也把 `performance.mark` 和 `performance.measure` 寫進 trace，和 Chrome 一樣：mark 是 thread 的 track 上的 instant 事件，measure 是自己的 track 上的 slice，用 entry 的時間；`timed_windows` 照 runner 的 mark 找區間（20 個 suite 的每個 test 都和 runner 回報的時間一致），trace 裡沒有 mark 時（`--against` 錄的舊版本）照 runner 的 frame 找。

## 動機

**原本的 `timed_windows` 算多了**（2026-10-18，`details.md` 的「`timed_windows` 算多了多少」）：它從 renderer main thread 上任何一個 `FireAnimationFrame` 算到之後第一個 `TimerFire` 的結束、重疊的合併，會包進頁面自己的 rAF 的 frame，和 frame 之間等 `BeginFrame` 的時間。20 個 suite 的 kun trace（各 3 個 iteration）照它算 9393 ms，照 runner 的結構只有 6767 ms。用到它的：
- `--sample` 只留區間裡的樣本（`sample.md`、folded stacks），所以混進 runner 計時以外的工作；RFC 0051 用它找瓶頸、用 `--against` 比兩個版本。
- RFC 0012 的 `progress.md` 裡「計時區間裡的 self time」的分析用同樣的定義（trace processor 算的），kun 和 Chrome 兩邊都多算。

量測表的數字（`measure.md`）不受影響：那是 runner 自己用 `performance.now()` 量、頁面回報的。

**只看 trace 的結構猜不準**：照 runner 的 frame 找（runner 在同一個 frame 排兩個 rAF）之後，18 個 suite 每個區間和 runner 自己量的 sync + async 差在 ±0.7 ms 內，但頁面自己的 timer 排在 runner 的 timer 之前時區間提早結束（React-Stockcharts-SVG 的 PanTheChart 每次少約 11 ms），頁面剛好在一個 frame 排兩個 rAF 時多出一個區間（TodoMVC-Preact-Complex-DOM 的 iteration 之間，32 ms）。runner 的 mark 不必猜。

**kun 的 trace 沒有 User Timing**：`performance.mark`、`performance.measure` 只進 performance timeline，trace 裡看不到；Chrome 的 trace 有（`blink.user_timing`），Perfetto 的 UI 上看得到每個 mark 和 measure，網頁作者和 Speedometer 都靠它。

**目標**：
- kun 的 trace 有 `performance.mark` 和 `performance.measure`，形式和 Chrome 一樣（分類、track、時間戳）。
- `timed_windows` 照 runner 的 mark 找區間，和 runner 量的同一段；沒有 mark 的 trace 照 runner 的 frame 找。
- Chrome 的 trace 也錄 `blink.user_timing`，兩邊能用同樣的區間分析。

**不在範圍內**：
- 重算 RFC 0012 以前的 trace 分析：之後要看時用新的定義重錄。
- 其他 Web Performance API 的 trace 事件（Resource Timing、Long Tasks、`PerformanceObserver`）。
- `--sample` 在 Linux 以外的平台。

沒有要解決的 `docs/limitations.md` 裡的限制；新的限制寫在那裡：User Timing 的事件不帶參數，`--against` 比較這份之前的版本時那一邊照 frame 猜。

## 設計概要

網頁的行為不變。對看 trace 的人：

- `MACHOPO_TRACE` 的 trace 多一個分類 `blink.user_timing`：
  - `performance.mark("x")`：renderer main thread 的 track 上一個名稱是 `x` 的 instant 事件，時間是 mark 的 `startTime`（`markOptions.startTime` 給了就是它）。
  - `performance.measure("y", …)`：一條自己的 track 上、名稱是 `y` 的 slice，從 measure 的開始到結束（兩個時間都可能在呼叫之前）。
  - 和 Chrome 的 trace 一樣（`details.md` 的「Chrome 的 user timing 事件」），Perfetto 的 UI 上看起來相同。
- `ci.py speedometer --trace` 的 Chrome 那一份也錄 `blink.user_timing`。
- `--sample` 的 `sample.md` 和 folded stacks 只有 runner 計時的區間裡的樣本：每個 test 從 `<suite>.<test>-start` 到 `<suite>.<test>-async-end`。

## 詳細設計

改到的地方：`../third_party/rusty_v8`（fork 的 `src/machopo_trace.cc`、`src/machopo_trace.rs`、`README.machopo.md`；`531a3ae`、`aa29a24`，release `kun-v152.2.0-11`）、`crates/bindings/src/trace.rs`、`crates/dom_api/src/performance.rs`、`crates/shell/src/trace.rs`、`crates/shell/src/main.rs`（`kun --help`）、`scripts/speedometer.py`、`scripts/speedometer/fake/resources/main.mjs`、`scripts/speedometer_test.py`、`third_party.toml`（拿掉 `rev`）、`.cargo/config.toml`（`RUSTY_V8_MIRROR_TAG`），測試在 `crates/dom_api/tests/it/performance.rs`、`crates/shell/tests/it/trace.rs`（和它借用的 `automation_pipe.rs` 的 `Browser`）。

### fork：instant 和自己的 track 上的 slice

`machopo_trace.cc` 原本只有 `kun` 分類和 `Begin`、`End`（thread 的 track 上的巢狀 slice，時間是呼叫的當下）。加上：

- 分類 `blink.user_timing`（`PERFETTO_DEFINE_CATEGORIES_IN_NAMESPACE` 多一個）。
- `kun__Trace__Mark(name, name_len, ago_ns)`：這個 thread 的 track 上一個 instant 事件（`TRACE_EVENT_INSTANT`），時間是 Perfetto 的時鐘的現在減 `ago_ns`。
- `kun__Trace__Measure(name, name_len, start_ago_ns, end_ago_ns)`：一條新的 track 上的 slice（`TRACE_EVENT_BEGIN` / `TRACE_EVENT_END` 帶 `perfetto::Track` 和時間戳），開始和結束一樣從現在往回算。一次呼叫只取一次 Perfetto 的現在，measure 的長度才精確（第一版開始和結束各取一次，結束晚了寫 begin 事件的時間）。每個 measure 一條 track，parent 是呼叫的 thread 的 track（`perfetto::Track(id, ThreadTrack::Current())`），和 Chrome 一樣：同名的 measure 可以任意重疊，共用一條 track 時 slice 不巢狀，trace 就壞了（「已決定的事項」第 9 條）。id 是計數用 splitmix64 打散的 64 位元：track 的 uuid 是 id 和 parent 的 uuid 的 xor，而每個 thread 的 track 只差 thread id 那幾個 bit，從 1 數的 id 讓第一個 measure 的 track 就是 compositor thread 的 track（端到端測試找到的；Chrome 的 id 也是雜湊值）。
- 結束在開始之前的 measure、未來的時間照寫，和 Chrome 一樣：每個 measure 自己一條 track，弄不壞別的 slice。
- 不帶參數（Chrome 的 mark 有 `data`：`startTime`、`callTime` 等）：`timed_windows` 用不到（第 11 條）。

時間用「多久以前」傳：kun 的 performance 的時鐘是 Rust 的 `Instant`，Perfetto 的時鐘在 Linux 是 `CLOCK_BOOTTIME`、其他平台各有不同；兩邊都有「現在」，差值不必換算時鐘（`TrackEvent::GetTraceTimeNs()` 減去差值）。差值是有號的：`markOptions.startTime` 和 measure 的結束可以在未來；算出來在時鐘的 0 以前時是 0。時間戳往回走的事件，Perfetto 的 SDK 自己改寫成絕對時間加 clock id。

照 fork 的規則：`machopo_trace.cc` 整個檔案是 kun 的（開頭的 `kun:`），記在 `README.machopo.md` 的「Local modifications」，fork 自己釘的 toolchain（1.91.0）也編得過。改了 C++ 要重編 V8（`python3 scripts/ci.py v8`，第一次 debug 和 release 共約 46 分鐘，之後的修正增量約 10 秒），fork 的 CI 發新的 release（Windows 的 prebuilt 只能從那裡來），kun 的 `.cargo/config.toml` 換 `RUSTY_V8_MIRROR_TAG`、`third_party.toml` 拿掉 `rev`（tag 就是 fork 的 commit）。

### kun：誰寫事件

- **`bindings::trace`**：包 fork 的兩個新函式（`mark(name, ago)`、`measure(name, start_ago, end_ago)`，ns），`CATEGORIES` 加 `blink.user_timing`。和 `begin`、`end` 一樣，沒有 trace 時是一次 load 和 branch。
- **`dom_api` 的 `performance.mark()` 和 `performance.measure()`**：把 entry 加進 buffer 之後發一個 `tracing` 的事件（target `blink.user_timing`；mark 的欄位是 `mark`（名稱）和 `ago_ns`，measure 的是 `measure`、`start_ago_ns` 和 `end_ago_ns`），不碰 V8：`dom_api` 和 engine 無關，本來就用 `tracing` 發 span。`new PerformanceMark()` 不發：它不進 performance timeline，Blink 也只在加進 timeline 時寫 trace。「距離現在多久」是 frame 沒有 coarsen 的時鐘（`frame.now() - time_origin()`，沒有 frame 時是 timer 的時間）減 entry 的時間，和文件的 time origin 無關：事件落在 entry 的時間上，和 Blink 用 time origin 加 entry 的 `startTime` 一樣（「已決定的事項」第 12 條）。只在有東西收這個事件時才算（`tracing::enabled!`）。
- **`shell::trace` 的 layer**：原本只收 span；它的 filter 多收 target 是 `blink.user_timing` 的事件，讀出欄位，呼叫 `bindings::trace::mark` 或 `measure`。log 的預設 filter 是 `info`，看不到這些事件（`RUST_LOG=trace` 才會）。`kun --help` 的 `MACHOPO_TRACE` 寫了 User Timing。

### 計時區間

- **讀 trace**：`perfetto_track_events` 讀出每個 track event（種類、名稱、分類：interned 的 iid 或字串）和 thread 的名稱；`perfetto_slices` 照舊只給 slice（measure 在它自己的 track 的 uuid 底下），`perfetto_marks` 給每個 thread 上 `blink.user_timing` 的 instant。
- **`timed_windows(slices, marks)`**：`mark_windows` 找得到區間就用它，不然用 `frame_windows`；同時回傳用的是哪一種。
- **照 mark（`mark_windows`）**：每個 `<suite>.<test>-async-end` 配它之前最近、還沒配過的同名 `<suite>.<test>-start`，一個 test 一個區間（runner 的 `_runTestAndRecordResults` 打的 mark：`-start`、`-sync-end`、`-async-start`、`-async-end`）；runner 的其他 mark（suite 的、iteration 的）沒有 `-async-end`，不算。
- **沒有 mark 時照 runner 的 frame（`frame_windows`）**：有兩個以上 `FireAnimationFrame` 的 `ProxyMain::BeginMainFrame`，從它的第一個 callback 到 frame 之後第一個 `TimerFire` 的結束，中間沒有別的 frame 開始。這是 `--against` 錄的舊版本（這份 RFC 之前的 kun 不寫 mark）用的；它的準確度和限制在 `details.md` 的「照 frame 找的準確度」和 docstring，原型是 `frames-fallback.patch`（2026-10-18 在 Windows 上寫好，第 3 段套上，之後刪掉，在 `main` 的 `6d47b962` 找得到）。
- **報告寫明用哪一種**：`sample.md` 每個 suite 一句「The windows by …」，`compare.md` 寫兩邊各用哪一種（第 14 條：每份 trace 用自己的）。
- **Chrome**：`CHROME_TRACE_CATEGORIES` 加 `blink.user_timing`。Chrome 的 trace 有好幾個 renderer，`perfetto_slices` 照 thread 的名字分組，讀不了它，照舊用 trace processor 分析；mark 讓那時的區間和 kun 的一樣。

### 假的 runner

`scripts/speedometer/fake/resources/main.mjs` 照真的 runner 打同樣的四個 mark 和兩個 measure（`<suite>.<test>-sync`、`-async`），工具的測試才能用 mark 找區間。假的 test 可以回傳 promise：sync 的結束和它的兩個 mark 在 promise 結束時（`startTime` 給那時的 `performance.now()`）。

### 測試

測試的清單和驗收在 `progress.md`：
- **fork**：不加測試（「已決定的事項」第 10 條）；第 1 段用一個小程式錄一份 trace、讀回確認，之後由 kun 的端到端測試守（它也找到了 track 撞號的 bug）。
- **`dom_api`**：`mark()` 和 `measure()` 送的事件和欄位、時間是沒有 coarsen 的時鐘減 entry 的時間、建構子和出錯的不送。測試 binary 裝一個全域的 subscriber（只開這個 target），照 thread 分開收：thread 自己的 subscriber（`with_default`）在其他測試平行註冊 callsite 時，會被 `tracing` 重算 interest 時漏掉、事件被關掉（單獨跑通過、一起跑收不到）。
- **`shell`**：layer 的單元測試；端到端的 `crates/shell/tests/it/trace.rs` 起一個 `kun` process（trace 一個 process 只能開一次），讀回 mark、measure 的種類、track 和時間（第 13 條）。
- **工具**：`speedometer_test.py` 的照 mark、照 frame 找區間、`perfetto_marks`（手寫的小 trace：別的分類的 instant 和 user timing 的 slice 不算）、報告寫明用哪一種；用假的 runner 的 trace，照 mark 找的區間等於 runner 回報的時間、measure 在自己的 track 上、Chrome 的 trace 有 runner 的 mark 的 instant。
- **驗收**：20 個 suite 的 kun trace（各 3 個 iteration），照 mark 找的每個區間和 runner 回報的 sync + async 差在 −0.013 到 +0.2 ms（`performance.now()` coarsen 的一兩格）。
- mutation test：三段共 37 條，全部抓到。fork 的 C++ 那幾條每條重編 V8：V8 用 `-Werror` 編，編不過的 mutation 不能被算成抓到，測試指令在編不過時讓它算成沒抓到。

### 分階段

1. **fork**：`blink.user_timing`、`Mark`、`Measure`，重編 V8（macOS），fork 發 release，kun 換 pin 和 tag。
2. **kun 寫事件**：`bindings::trace`、`dom_api`、`shell::trace` 和它們的測試。
3. **工具**：`timed_windows` 照 mark、沒有 mark 時照 frame，`perfetto_slices` 讀 instant，Chrome 錄 `blink.user_timing`，假的 runner 打 mark，`speedometer_test.py`；20 個 suite 的驗收。

照 frame 找的版本（第 3 段的退路）2026-10-18 先在 Windows 上寫好、驗證過（原型 `frames-fallback.patch`，四條 mutation 都被它的測試抓到），第 3 段套上它再加照 mark 找，之後刪掉（在 `main` 的 `6d47b962` 找得到）。第 2 段的端到端測試找到 fork 的 track 撞號，fork 多發了一次 release（`kun-v152.2.0-11`）。

## 缺點

- **改 fork 的 C++、重編 V8**：每次改 `machopo_trace.cc` 都要重編、發 release、換 pin，其他機器跟著換 artifact（這份 RFC 發了兩次：第二次修 track 撞號）。M1（不改 fork，用 `Begin`、`End` 寫長度 0 的 slice，分類照舊 `kun`）不用，但 mark 就不是 instant、不在 `blink.user_timing`，measure 寫不了（它要自己的 track 和過去的時間戳）。
- **兩套找區間的方法**：照 mark 和照 frame。照 frame 的只在沒有 mark 時用，但要一起維護和測試。
- **`dom_api` 多一個 `tracing` 的事件**：沒有 trace 時，`tracing` 的 callsite 是關的，成本是一次 load 和 branch；有 trace 時每個 mark 多一次 FFI。

## 理由和其他做法

**找區間**（使用者 2026-10-18 選 C）：

| | 做法 | 準確度 | 成本 |
| --- | --- | --- | --- |
| A | 照 runner 的 frame（只改 Python） | 18 個 suite 每個 test ±0.7 ms；頁面自己的 timer 或 rAF 會猜錯（Stockcharts 少 9%、Preact 多 27%） | 已經寫好 |
| B | 結尾改用 runner 回報的 sync + async | 結尾準；開頭照舊靠結構，多出來的 frame 讓區間和 test 對不上 | 只改 Python |
| **C** | **kun 把 User Timing 寫進 trace，照 runner 的 mark** | **和 runner 量的同一段** | **改 fork、`dom_api`、`shell`** |

A 留著當沒有 mark 時的退路。

**mark 的形式**（「已決定的事項」第 2 條）：
- **M2：fork 加 instant 事件，分類 `blink.user_timing`（採用）**：和 Chrome 一樣，Perfetto 的 UI 和 trace processor 看起來相同。
- M1：用現有的 `Begin`、`End` 寫長度 0 的 slice，分類 `kun`：不改 fork；但不是 instant、分類不同，也只能用呼叫當下的時間（`markOptions.startTime` 在過去時對不上）。

**measure**（第 3 條）：寫。只要 mark 就找得到區間，但 measure 在 Perfetto 的 UI 上把每個 test 的 sync 和 async 畫成一段，網頁作者自己的 measure 也看得到；fork 反正要改，多一個函式。

**在哪裡寫**（第 4 條）：`dom_api` 發 `tracing` 的事件、`shell` 的 layer 轉給 `bindings`。不選 `dom_api` 直接呼叫 `bindings::trace`：`dom_api` 和 engine 無關，不能依賴 `bindings`（`sanity.py deps`）。

## 先例

- **Chromium**：Blink 的 User Timing 把 mark 寫成 `blink.user_timing` 的 instant、measure 寫成自己的 track 上的 slice（2026-10-18 用 Chrome 152 的 trace 確認，`details.md`），Perfetto 的 UI 上看得到 runner 的每個 test；時間都用 entry 的時間（不是呼叫的當下），每個 measure 一條 track，parent 是 thread 的 track（2026-10-18 確認，`details.md`）。
- **kun 自己**：`MACHOPO_TRACE` 的 span 照 Chrome 的 trace 事件的名稱和層次，這份照同樣的原則把 User Timing 也對上。

## 已決定的事項

2026-10-18 接受。

1. **找區間用 C：kun 把 User Timing 寫進 trace，`timed_windows` 照 runner 的 mark**（2026-10-18，使用者）；照 frame 找（A）留著，trace 裡沒有 mark 時用。
2. **mark 用 M2：fork 加 instant 事件，分類 `blink.user_timing`**（2026-10-18，使用者）。不選 M1（不改 fork、長度 0 的 slice）：和 Chrome 不同，也不能用過去的時間。
3. **measure 也寫**，自己的 track 上的 slice（2026-10-18，使用者）。
4. **`dom_api` 發 `tracing` 的事件，`shell` 的 trace layer 轉給 `bindings`**；只有 `performance.mark()` 和 `measure()` 發，`new PerformanceMark()` 不發（2026-10-18，使用者）。
5. **時間用「多久以前」傳給 fork**，由 fork 從 Perfetto 的現在往回算（2026-10-18）。
6. **Chrome 的 `--trace` 也錄 `blink.user_timing`**（2026-10-18，使用者）。
7. **假的 runner 照真的 runner 打 mark 和 measure**（2026-10-18，使用者）。
8. **新開這份 RFC**（2026-10-18，使用者）：RFC 0051 在另一台機器的 branch 上做，RFC 0054 的範圍是 Chrome 經過 CDP。**在 macOS 上做**（使用者）：第 1 段要重編 V8，Windows 上不能從原始碼編。

9. **每個 measure 一條 track，parent 是呼叫的 thread 的 track**（2026-10-18，使用者，第 1 段），id 原本是 process 裡的計數器，第 2 段改成計數用 splitmix64 打散（從 1 數的 id 撞到別的 thread 的 track，見「詳細設計」）：和 Chrome 152 一樣（`details.md` 的「Chrome 的 user timing 事件」）。不選同名的共用一條（同名的 measure 可以重疊，同一條 track 上的 slice 不巢狀，trace 就壞了），也不選掛在 process 的 track 底下（和 Chrome 不同，UI 上和 main thread 分開）。
10. **fork 不加測試**（2026-10-18，使用者，第 1 段）：fork 的 `tests/` 沒有 `kun_trace` 的測試，加一個要寫 protobuf 的 reader、要自己的 test binary（Perfetto 和 V8 的 platform 一個 process 只初始化一次），違反「改 fork 要盡量小」。第 1 段用小程式錄、讀回確認，長期由 kun 的端到端測試守（第 2 段定在哪裡）。
11. **事件不帶參數**（2026-10-18，使用者，第 1 段）：Chrome 的 mark 帶 `data`（`startTime`、`callTime` 等），`timed_windows` 用不到；之後要加得再改 C++、重編 V8。
12. **「多久以前」用 frame 沒有 coarsen 的時鐘減 entry 的時間**（2026-10-18，使用者，第 2 段）：事件落在 entry 的時間上，和 Chrome 一樣，同一個 mark 的 instant 和以它為端點的 measure 對得上。不選 coarsen 過的 `window_now`：不帶時間的 mark 會在呼叫的當下，但其他的事件每個偏 0 到 100 µs、每次不同。
13. **端到端測試在 `crates/shell/tests/it/`**，起 `kun` process、讀回 trace（2026-10-18，使用者，第 2 段）：在 `cargo test` 裡、每天的 CI 跑。不選 `speedometer_test.py`：不在 CI 裡，也要等第 3 段的假 runner 打 mark。
14. **`--against` 每份 trace 用自己的區間**（2026-10-18，使用者，第 3 段）：有 mark 的照 mark，沒有的照 frame，`compare.md` 寫明兩邊各用哪一種。不選「有一邊沒有 mark 就兩邊都照 frame」：要留著每一輪的樣本和 trace，等一個 suite 兩邊都跑完才決定，`sample_suites` 的流程要改；定義不同只在比較這份 RFC 之前的版本時有，報告上看得出來。
15. **驗收跑 20 個 suite 的 kun trace，各 3 個 iteration**（2026-10-18，使用者，第 3 段）：照 mark 找的每個區間和 runner 回報的 sync + async 逐一比。

## 待決定

沒有。

## 之後的可能

- 用新的區間重錄 0012 那種「計時區間裡的 self time」的分析，看差距的排序有沒有變。
- 其他 Web Performance 的 trace 事件（Long Tasks、Resource Timing），Chrome 也有。
- mark 和 measure 像 Chrome 一樣帶參數（`startTime`、`callTime`、`navigationId`）。
