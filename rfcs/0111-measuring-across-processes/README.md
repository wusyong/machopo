# RFC 0111：量測工具跟著 process 走

- 開始日期：2026-11-23
- 狀態：🚧 已排程：2026-11-23（第 1 階段 merge 回 `main`；第 2 階段做完、待審查）
- 細節：`details.md`；進度：`progress.md`

## 摘要

RFC 0004 讓 renderer 預設是每個分頁一個 process 之後，`scripts/bench/` 的幾個工具只看 Browser 那個 process：Speedometer 的 `--trace`（各階段的表）、`--sample` 和 peak memory，`census`，照程式看 `blink-perf --gc` 也是。它們不出錯，而是交出 0、照 frame 猜的區間、少了一半的記憶體（2026-11-23 的盤點，`details.md`）。這份 RFC 讓每個工具在預設的組態下量到 renderer：一次執行有哪些 process 由一個共用的地方回答，trace 讀那次執行的每一份，取樣和記憶體照 process 的種類分開、再加總，`census` 每個 process 寫自己的一份；量不到 renderer 時報錯，不交出看起來正常的數字。照 Chromium 的工具，量的是預設的 multi-process，不改用 `--single-process`；trace 不合成一份（Chrome 由 tracing service 收成一份），工具讀幾份就夠了。

## 動機

**現況**（2026-11-23，`details.md` 的「盤點」）：

- `MACHOPO_TRACE` 一個 process 一份：Browser 的是那個路徑，renderer 的是它加上 `.<pid>`（`crates/shell/src/main.rs` 的 `main`），兩者用同一個時鐘。
- `scripts/bench/common.py` 的 trace 函式（`perfetto_slices`、`perfetto_marks`、`perfetto_thread_tracks`……）和 `scripts/bench/speedometer/speedometer.py`、`scripts/bench/blink_perf/blink_perf.py` 的呼叫都只讀一個路徑；`peak_memory` 只讀 `machopo.process.pid`。
- `scripts/bench/raster/raster.py` 的 `renderer_traces` 已經一起讀 `<trace>.<pid>`，也有 `--single-process`。
- `base::census`（`crates/base/src/census.rs`）每個 process 把自己的整張表寫到 `MACHOPO_CENSUS` 那一個路徑。

**問題**：

- **量錯，而且看起來像對的**：`phases.md` 每格 0.0；`--against --sample` 的 current 是 0 個樣本、計時區間照 frame 猜，`compare.md` 寫 −100.0%；peak memory 每個 suite 少三到六成，像是優化。查 Speedometer 的退步時（`rfcs/0012-speedometer/progress.md` 的 2026-11-23）因此只能在 RFC 0004 之前的兩個 commit 之間取樣。
- **沒有東西守著**：工具的測試抓得到 trace 和取樣的（2026-11-23 跑 `speedometer_test.py` 的那幾個，trace 和取樣的都失敗，`details.md` 的「盤點」），但它們不在 CI 裡（`docs/testing.md` 的表），預設改了時沒有人重跑；記憶體的測試只要求超過 20 MB，少了 renderer 也通過。
- **量不到多 process 自己的成本**：只有 `raster` 看得到 IO thread 處理訊息、tile 從共用記憶體上傳；Speedometer 的各階段和取樣看不到 renderer，就算看到了，也看不到 Browser 那一側為它做的事。

要解決的 `docs/limitations.md` 裡的限制：

- 「`MACHOPO_TRACE` 的 trace 只有……」那一條的最後一句：「一個 process 錄一份：renderer 拆成自己的 process 之後會變成好幾份（Chrome 由 browser process 收成一份）」。這份 RFC 讓工具讀幾份；那一句改成只說「一個 process 一份、工具一起讀」。
- 「平台和建置」的「量測工具在 renderer 是自己的 process 時（`machopo` 的預設）量不到它，而且不報錯」（2026-11-23 加的）。

**目標**：

1. 每個工具在 machopo 的預設組態（每個分頁一個 renderer process）下量到 renderer：Speedometer 的 `--trace`、`--sample`、peak memory 和 `--against` 的這三種，`blink-perf --gc`，`census`。
2. 量不到時停下或在報告裡寫成錯誤：沒有 renderer 的 trace、renderer 的 trace 裡沒有 runner 的 mark（而那個版本會寫）、計時區間裡沒有樣本、各階段整欄是 0、記憶體少了 renderer。
3. 記憶體照 process 的種類分開列（Browser、renderer、合計），和 Telemetry 的 `browser_process`、`renderer_processes`、`all_processes` 對得上。
4. `--against` 的兩邊 process 模型不同時（RFC 0004 之前的 commit 只有 single-process），報告寫明每一邊是哪一種。
5. 工具的測試在預設的組態下守住上面這些，並且改到相關的地方時有人跑（已決定的事項第 2 條）。

**不在範圍內**：

- trace 合成一份（Chrome 的 tracing service）：見「理由和其他做法」的 C 和「之後的可能」。
- 量 Chrome 的記憶體來比較：見「待決定」第 3 條。
- 這次查到的兩個退步（attribute 的 bloom filter 照名稱的編號、`place_line` 每一行都跑）：使用者 2026-11-23 決定先不處理。
- sandbox、GPU process：machopo 還沒有。之後多了別種 process 時照同一個介面加。

## 設計概要

給跑量測的人：指令不變，量的是 `machopo` 平常的樣子。

- `bench.py speedometer --trace` 的 `phases.md` 是 renderer main thread 的各階段，和以前 renderer 在 Browser 裡時一樣；`trace/` 底下每個 suite 有 Browser 和 renderer 各一份 `.pftrace`，Perfetto UI 可以一起開。
- `--sample` 的 `sample.md` 是 renderer main thread 的樣本；開頭寫明樣本來自哪個 process。
- peak memory 列三欄：Browser、renderer、合計。`compare.md` 的變化照合計，另外兩欄看是哪一邊變的。
- `--against` 比 RFC 0004 之前的 commit 時，`compare.md` 寫「against：renderer 在 Browser 的 process 裡；current：renderer 是自己的 process」，記憶體的合計兩邊都能比，分欄只有一邊有。
- 量不到 renderer 時，那個 suite 在報告裡是錯誤，原因寫明（例如「renderer 的 trace 沒有 runner 的 mark」），不是 0。

## 詳細設計

### 一次執行有哪些 process

`common.py` 加一個共用的介面（暫名 `MachopoProcesses`），由啟動 machopo 的 `MachopoPipe` 交出：Browser 和每個 renderer 的 pid、種類，和它們各自的 trace。怎麼知道 renderer 的 pid 比過三種做法，用 P3 加 P1（已決定的事項第 1 條）：

- **P1：從 trace 的檔名**。`<trace>.<pid>`，`raster` 現在的做法。只在有 trace 時有；記憶體要在 process 結束前讀，那時 trace 還沒寫完，檔名已經在了（開始錄時就建檔）。
- **P2：從 OS 的 process 樹**。macOS 的 `proc_listchildpids`、Linux 的 `/proc/<pid>/task/*/children`、Windows 的 Toolhelp32 snapshot。不用 machopo 配合，但種類要另外猜（命令列有 renderer 的 switch）。
- **P3：CDP 的 `SystemInfo.getProcessInfo`**。machopo 的 automation pipe 加這個方法，照 Chrome 回傳 `{type, id, cpuTime}`。種類由瀏覽器自己說，同一個呼叫也問得到 Chrome 的（之後比 Chrome 的記憶體時用得到）；Chrome 的也只在 browser target 上。

P3 給 pid 和種類（`crates/shell/src/automation_session.rs`；不給 Chrome 的 `cpuTime`：讀別的 process 的 CPU 時間要平台的呼叫，machopo 只在 `ipc` 裡做那種事，工具也用不到），P1 找 trace 的檔案（trace 的檔名本來就帶 pid，不必另外問）。一次執行問一次 `getProcessInfo` 就夠：machopo 每個分頁一個 renderer、Speedometer 和 Blink 的 perf tests 都只開一個分頁，在頁面載入之後、讀記憶體之前問；導覽換了 renderer 的情況（之後有 site isolation 時）再照 Chrome 的行為處理。

### trace

- 一次執行的 trace 是 Browser 的那份加上每個 `<trace>.<pid>`；`common.py` 的讀取函式改成接受一次執行的那幾份，`perfetto_thread_tracks` 的 `(pid, tid)` 本來就分得開 process。
- renderer main thread 照現在的規則找（名叫 `CrRendererMain`、mark 最多的那條，`perfetto_marked_thread`），只是在每一份裡找。Speedometer 的 suite 在 runner 的 iframe 裡，同源，和 runner 在同一個 renderer。
- `phases.md`、`--sample` 的計時區間、`blink-perf --gc` 的「只算 runner 的 thread 所在的 process」都從那條 thread 出發，不用改規則。
- 一次執行開始前刪掉舊的 `<trace>.*`（`raster` 已經這樣做），不然上一次的 renderer 的 trace 會混進來。

### 取樣

- 樣本照 thread 的名稱和計時區間挑（`read_samples`），區間來自 renderer 的 trace 之後就有樣本，前提是取樣器也取到了子 process：macOS 的 samply 會（2026-11-23 確認，`details.md` 的「盤點」：renderer 的 pid 和它的 trace 的檔名一樣，`read_samples` 已經挑到它的 `CrRendererMain`、JS 函式有名字）；Linux 的 `perf record` 預設跟著子 process（inherit），要在那台機器上確認；Windows 的 ETW 錄整台機器、照 pid 篩，也要確認。
- 挑樣本時連 pid 一起比（renderer 的 pid 來自上一節），不只比 thread 的名稱：之後一次執行有好幾個 renderer 時不會混在一起。
- V8 的 perf map（`/tmp/perf-<pid>.map`）每個 process 各一份，工具照樣本裡的 pid 刪（現在就是）。
- 讀得到 renderer 的樣本之後，`SampleTests.test_the_stacks_in_the_timed_windows` 的「開著 trace 時不做 `log` 的紀錄」抓到 renderer process 每一筆都做（html5ever 每個 token 一筆，做了再丟）：`crates/shell/src/main.rs` 的 renderer 那條路在 `init` 之後沒有像 Browser 的那樣 `tracing_log::log::set_max_level`，trace 那一層收每一個 level，`log` 的上限就是 TRACE。2026-11-23 補上；renderer 在 Browser 的 process 裡時（以前取樣的都是）沒有這個問題，所以以前沒看到。

### 記憶體

- 每個 process 的峰值照現在各平台的量法（macOS 的 phys footprint 的 lifetime max、Linux 的 `VmHWM`、Windows 的 peak private bytes），在報告送回、還沒關掉 machopo 時讀（renderer 那時還在）。
- 報告列 Browser、renderer、合計。**合計是各 process 峰值的和**，是整個瀏覽器峰值的上限：兩個 process 的峰值不一定同時。Telemetry 取的是 dump 的時間點、照種類加總（`details.md`），沒有整次的峰值；machopo 要接近它得定期取樣每個 process 的目前值（「待決定」第 2 條）。
- `--against` 的變化照合計算，另一張表照 process 分開列兩邊的 Browser 和 renderer（`speedometer.py` 的 `write_time_report`）；renderer 在 Browser 的 process 裡的那一邊（舊版本、`--single-process`）renderer 那一欄是空的，合計就是 Browser 的。
- renderer process 的峰值讀不到時合計是空的，不拿 Browser 的代替（`common.py` 的 `memory_by_process`）。列不出 process 的舊版本（沒有 `getProcessInfo`）也是空的，除非它的 renderer 本來就是 Browser 的 thread：`--against` 照 git 的歷史判斷，`0f36205`（renderer 預設是 process 的第一個 commit）之前的版本的合計是 Browser 的（`renderers_are_threads`），之後、`getProcessInfo` 之前的那幾個是空的。

### `census`

- renderer process 的 `base::census` 寫 `MACHOPO_CENSUS` 加 `.<pid>`（和 trace 一樣）：`crates/base/src/census.rs` 的 `own_file`，由 `crates/shell/src/main.rs` 在 renderer process 一開始、還沒有任何計數之前呼叫；`base` 不知道自己在哪一種 process 裡。Browser 的照舊。
- `scripts/bench/census/census.py` 把一頁的每一份加總（`count_files`）：一個 callback 只會在一個 process 裡跑，加總不會重複算。`getProcessInfo` 說有 renderer process、卻沒有它的那一份時，那一頁是錯誤（`renderer_problem`）：只有 Browser 的那份，看起來像一頁沒什麼呼叫。

### 量不到時報錯

工具現在有幾條「沒有就用別的」的路，都是為了 RFC 0004 之前的情況留的：沒有 User Timing 的舊版本照 frame 猜區間、沒有 trace 時不算各階段。改成：

- 這次執行的 machopo 有 renderer process，卻讀不到它的 trace：錯誤。
- renderer main thread 的 trace 裡沒有 runner 的 mark，而這個版本會寫 User Timing（`--against` 的舊版本才照 frame 猜，報告照舊寫明）：錯誤。
- 計時區間裡一個樣本都沒有、各階段整欄是 0：錯誤，不寫成 0。
- 記憶體讀不到某個 process：那一欄是空的、寫明原因，合計不算。

### `--single-process`

Speedometer 和 Blink 的 perf tests 的工具加 `--single-process`（`raster` 已經有），交給 machopo：比 RFC 0004 之前的版本時讓兩邊同一種組態，或看多 process 的成本（同一個版本兩種組態比）。預設不加。`common.py` 的 `SINGLE_PROCESS` 和 `machopo_switches`：`MachopoPipe` 啟動 machopo 時加上它；`--against` 的版本不認得這個 switch 時（`96a940c` 之前）不交給它，那一邊照它原本的樣子跑。不認得的版本都比 renderer 預設是 process 的版本（`0f36205`）舊，它的 renderer 本來就是 thread，兩邊還是同一種組態。報告（`measure.md`、`compare.md`）寫明每一邊的 renderer 在哪裡（`renderers_text`）。Windows 上取樣時以系統管理員跑的那一份也拿到這個設定。

### 測試

- 工具的測試在預設的組態下跑（現在就是），加上：`--trace` 和 `--sample` 的 renderer 那一份讀到了、mark 照 runner 的、各階段不是 0（`MeasureTests.test_traces_of_both`、`MachopoTests.test_the_windows_are_the_runners_times`、`SampleTests.test_the_stacks_in_the_timed_windows` 已經檢查，現在失敗）；記憶體三欄都有、renderer 那一欄不是 0、合計是兩欄的和（`test_machopos_peak_memory_with_its_run` 只檢查超過 20 MB）；`census` 的兩份加總；量不到時是錯誤（合成的 trace：只有 Browser 的那份、renderer 的沒有 mark）。
- `--single-process` 的路至少一個測試（`raster_test.py` 有類似的）。
- 誰跑這些測試：`CLAUDE.md` 的規則（已決定的事項第 2 條），不在 CI 裡。
- mutation test 照 `docs/testing.md` 的做法，對象是 Python 的工具：例如讀 trace 時只讀第一份、合計少加一欄、拿掉「量不到時報錯」的檢查。

### 分階段

1. **process 和 trace**：automation pipe 的 `SystemInfo.getProcessInfo` 和共用的介面、讀一次執行的每一份 trace、Speedometer 的 `--trace` 和 `--sample`、`blink-perf --gc`、量不到時報錯。
2. **記憶體和 `census`**：照 process 的記憶體和 `--against` 的比較、`census` 每個 process 一份；`--single-process`。
3. **Linux 和 Windows**：在那兩台機器上跑一次工具的測試和一次 Speedometer 的 `--sample`、`--trace`（使用者在自己的裝置上跑），確認 `perf` 和 ETW 取到 renderer process 的樣本。

## 缺點

- 工具多一層「一次執行的 process」，每個讀 trace 的地方都要改成讀一組檔案。
- 記憶體的合計是峰值的和，比真的峰值大；和以前 single-process 時的單一個峰值不能直接比（以前的數字也是 Browser 一個 process 的峰值，那時 renderer 在裡面）。
- P3 要在 automation pipe 加一個 CDP 的方法，維護它的行為要和 Chrome 的一致；它在 Chrome 是 experimental。

## 理由和其他做法

- **A：工具改用 `--single-process`**。最省事（`raster` 已經有這個選項），但量的不是 `machopo` 平常的組態，多 process 加進來的成本（訊息、共用記憶體的複製、IO thread）一個都看不到。Chromium 的 Telemetry 和 crossbench 跑 Speedometer 都不改 process 模型，Chromium 的取樣文件也是 attach 到 renderer（`details.md`）。所以只當成選項（上面的 `--single-process`），不當預設。
- **B：工具跟著 process 走**（這份 RFC）。改的都在工具和 `census` 的檔名，machopo 本身幾乎不動（P3 除外）；trace 本來就是同一個時鐘，讀幾份和讀一份一樣準。
- **C：照 Chrome 合成一份 trace**。Chrome 桌面版由獨立的 tracing service 收集每個 process 的 Perfetto 資料（`details.md`）；machopo 要做，就得讓每個 renderer 經過 IPC 交給 Browser（或一個 service），動到 rusty_v8 裡的 Perfetto 整合（現在是 in-process 的 backend）。對讀 trace 的工具來說沒有比 B 多得到什麼；DevTools 的 `Tracing` 要整個瀏覽器的 trace 時才需要，列在「之後的可能」。
- **不做**：之後每次查效能都只能看時間，或退回 RFC 0004 之前的 commit 取樣；記憶體的數字會被當成真的。

## 先例

- **Chromium**（`details.md` 的「Chromium 怎麼量多 process 的瀏覽器」）：trace 由 tracing service 收成一份，process 的種類寫在 `ChromeProcessDescriptor`；記憶體照 process 的種類加總、另有 `all_processes`，取 dump 的時間點；`SystemInfo.getProcessInfo` 列出 process；取樣 attach 到 renderer 的 pid 或用 `--renderer-cmd-prefix`；Speedometer 在預設的 multi-process 下跑。照做的：預設的組態、照種類分開再加總、`getProcessInfo` 的形式（P3）。不照做的：合成一份 trace（C）、照 dump 的時間點取記憶體（「待決定」第 2 條）。
- **machopo 自己的 `bench.py raster`**：已經讀 `<trace>.<pid>`、有 `--single-process`；這份 RFC 把它的做法搬到共用的地方。

## 已決定的事項

1. **一次執行的 process 用 P3 加 P1 找**（2026-11-23，使用者同意草稿的看法；原本「待決定」的第 1 條）：machopo 的 automation pipe 照 Chrome 加 CDP 的 `SystemInfo.getProcessInfo`（browser target 上，回傳 `{type, id, cpuTime}`），工具用它拿每個 process 的 pid 和種類；trace 的檔案照檔名找（`<trace>.<pid>`，`raster` 現在的做法），不另外問。理由：種類由瀏覽器自己說，不用從命令列猜；同一個呼叫在 Chrome 上也有，之後要量 Chrome 的 process 時工具不用分兩條路。不選 P2（OS 的 process 樹）：三個平台各寫一套，種類要從 renderer 的 switch 猜，之後多了別種子 process 時也要跟著猜。不選只用 P1：沒開 trace 時（量時間和記憶體的那次）就沒有 pid。
2. **工具的測試由 `CLAUDE.md` 的規則守住，不進 CI**（2026-11-23，使用者決定；原本「待決定」的第 4 條）：`CLAUDE.md` 的「測試」寫明，改了 renderer 怎麼啟動、`MACHOPO_TRACE` 的 trace、`base::census` 或 automation pipe 的 CDP 方法時，跑 `scripts/bench/` 底下讀 trace、取樣和記憶體的工具的測試（`speedometer`、`blink_perf`、`raster`、`census`）。理由：這次漏掉是因為沒有規則說改了這些要跑它們，不是因為沒有測試；不加 CI 的時間（它們要編 `perf` profile 或 dev 的 machopo，一部分要 Chrome for Testing）。不選每週的 CI：會自動抓到，但多一組要下載 Chrome、跑很久的工作，而且壞了要等到下一週。規則在 2026-11-23 先寫進 `CLAUDE.md`；這份 RFC 做完之前，`speedometer_test.py` 的 trace 和取樣的測試照規則跑也是失敗的（「動機」）。
3. **記憶體的合計是各 process 峰值的和**（2026-11-23，使用者同意；原本「待決定」的第 2 條）：Browser 和每個 renderer process 各讀自己的峰值，合計是它們的和，報告寫明是上限。使用者先問了 Chrome 怎麼算：Telemetry 的 memory metric 兩種都有，每次 dump 照 process 的種類加總（`private_footprint_size` 這類是同一個時間點的和），`all_processes` 的 `peak_resident_size` 則是每個 process 的 OS 峰值的和（`details.md`）；這裡照後者，量的是 phys footprint 的峰值（macOS）。理由：量法和以前一樣、沒有取樣的間隔和雜訊，`--against` 比兩個版本要的是穩定。不選定期取樣、取和的峰值：要一條取樣的 thread，間隔之間的尖峰會漏掉；要看執行中怎麼變化時再做（「之後的可能」）。
4. **這份 RFC 不量 Chrome 的記憶體**（2026-11-23，使用者決定；原本「待決定」的第 3 條）：記憶體照舊只量 machopo。Chrome 的 GPU、network 這些 process 算不算進去才公平（machopo 的 Viz 在 Browser 裡），要另外決定；留在「之後的可能」。

## 待決定

1. ~~怎麼找到 renderer 的 pid~~：已決定的事項第 1 條。
2. ~~記憶體的合計~~：已決定的事項第 3 條。
3. ~~要不要也量 Chrome 的記憶體~~：已決定的事項第 4 條。
4. ~~工具的測試誰跑~~：已決定的事項第 2 條。
5. ~~macOS 的 samply 跟不跟子 process~~：跟（2026-11-23 確認，「取樣」）。Linux 和 Windows 的在第 3 階段確認；不跟時改成 attach 到 renderer 的 pid，或像 Chromium 的 `--renderer-cmd-prefix` 那樣由 machopo 在啟動 renderer 時包一層取樣器。

## 之後的可能

- **合成一份 trace**（C）：DevTools 的 `Tracing` 要整個瀏覽器的 trace，或 Perfetto UI 要一次看所有 process 時。
- **和 Chrome 比記憶體**（已決定的事項第 4 條：不在這份做）。
- **定期取樣記憶體**：照 Telemetry 的 dump，每隔一段時間讀每個 process 目前的值，看執行中怎麼變化、同一個時間點的合計（已決定的事項第 3 條沒有選它當合計）。
- **多 process 的成本當成一欄**：同一個版本 `--single-process` 和預設各跑一次，報告並列，看 IPC 的成本在哪個 suite 上最多。
- 之後多了 GPU、network 或 utility 的 process 時，照同一個介面加種類。
