# RFC 0066：Windows 上的取樣和綁核心

- 開始日期：2026-10-20
- 狀態：✅ 已完成：2026-10-21
- 進度和驗收：`progress.md`
- 細節：`details.md`（samply 在 Windows 上怎麼錄、原型的紀錄、沒選的做法）

## 摘要

`bench.py speedometer --sample` 在 Windows 上也能用：和 macOS 一樣用 samply（Windows 上它經過 xperf 錄 ETW），沿用讀 samply profile 的程式，只補 Windows 的差別：V8 的旗標（`--enable-etw-stack-walking`，JS 的名稱經過 ETW 交出）、樣本的時間從 kernel ETL 的 header 起算（和 trace 一樣是 QPC）、`perf` profile 的 exe 記 PDB 的絕對路徑（samply 才找得到 kun 的符號）、Windows 自己的 DLL 錄完之後經過 `samply load` 和 Microsoft 的 symbol server 補名稱、ETW 要系統管理員（取樣的工作整份交給一個以系統管理員跑的自己，一次執行只跳一次 UAC）。另外，Windows 上的兩個量測工具（Speedometer、Blink 的 perf tests）也把瀏覽器綁在最快的那組 CPU 上（`GetSystemCpuSetInformation` 的 efficiency class），和 Linux 上的 `taskset` 一樣。分三段：先手動做原型，再綁核心，最後接進工具，都在 Windows 11（i9-13900K）上做。

## 動機

- 在 Linux 上用取樣做過「kun 和 Chrome 差在哪」的分析（時間花在哪些函式），Windows 上只有 `--trace`（各階段的時間），拆不到函式。開發機有一台是 Windows，那台上看到的慢要換一台機器才能追。
- Windows 上的量測沒有綁核心：開發機的 i9-13900K 有 8 顆 P-core、16 顆 E-core，瀏覽器整頁落在哪一種核心上，數字就差很多，兩個瀏覽器各自落，倍數也跟著跳（Linux 上量過，見 `docs/benchmarks.md` 的「效能的量測」）。

目標：

- `bench.py speedometer --sample`（連同 `--against`）在 Windows 上照其他平台的方式跑：只取 renderer main thread 在 runner 計時的區間裡的樣本，fold 之後寫 `sample.md`。kun 的函式由 PDB 命名，JS 的由 V8 經過 ETW 命名。
- Windows 上 `bench.py speedometer`、`bench.py blink-perf` 的兩個瀏覽器都跑在最快的那組 CPU 上，報告寫出是哪些。

不在範圍內：

- 取樣 Chrome（Chrome for Testing 沒有符號，和其他平台一樣）。
- 指令數這種和頻率無關的計數：samply 只取 stack，和 macOS 一樣以 CPU 時間計（`docs/benchmarks.md` 寫了它比 Linux 的 `perf` 吵）。
- Windows on Arm。

要解決的 `docs/limitations.md` 裡的限制：「平台和建置」裡 `MACHOPO_TRACE` 那一條的「Windows 上沒有，要自己對 `perf` profile 的 build 取樣（Windows x64 沒有 frame pointer，Chrome 也沒有），也不會只算計時的區間」（做完時換成 Windows 上取樣的限制）。

## 設計概要

```sh
cargo install --locked samply                         # 加上 Windows Performance Toolkit 的 xperf
python scripts/bench.py speedometer --sample --suites TodoMVC-jQuery
```

在一般權限的終端機裡跑：工具先照常 build kun，要開始取樣時跳一次 UAC，按「是」之後整次執行不再問（`--against` 的每一輪也在裡面）；提權的那個 process 的輸出照樣印在原本的終端機裡。已經是系統管理員時不跳。結果和其他平台一樣在 `target/bench/speedometer/sample/`：kun 的函式由 PDB 命名，Windows 自己的 DLL（ntdll 的 heap、vcruntime140 的 `memcpy`、kernel）由 Microsoft 的 symbol server 命名，JS 的帶網址和行號。

綁核心不用做什麼：在有大小核的 CPU 上，工具自己找出 P-core，兩個瀏覽器都在上面跑；報告的環境一欄寫出是哪些 CPU。

## 詳細設計

照原型（第 1 段，`details.md` 的「原型」）的結果定的，見「已決定的事項」的第 6 到 10 條。

### 取樣

- `common.py`：`sampling()` 在 Windows 上回 `WINDOWS_SAMPLING`（samply、`cpu`、`.samply.json.gz`，和報告裡的說明：ETW、unwind table、V8 的旗標）；`samply_sampler()` 在 Windows 上給 samply 加上 `--browsers`（打開 V8 的 ETW provider）和 `--keep-etl`（讀時鐘的基準，`samply_etls`）。V8 的旗標分平台：Linux、macOS 照舊用 `--perf-basic-prof`（`/tmp/perf-<pid>.map`），Windows 用 `--enable-etw-stack-walking --interpreted-frames-native-stack`。
- stack 由 ETW 照 unwind table 展開（x64 不靠 frame pointer）：Rust 的程式有 `.pdata`，V8 替 JIT 的程式登記 unwind 資訊，`--interpreted-frames-native-stack` 讓直譯的函式有自己的 frame。kernel 的 frame 也在裡面（第 10 條）。
- **kun 的符號**：`build_options`（`kun_binary` 用）在 Windows 的 `perf` profile 上多給 linker `/PDBALTPATH:<target>\perf\kun.pdb`，exe 的 CodeView 記的就是 PDB 的絕對路徑。rustc 一律給 `/PDBALTPATH:%_PDB%`，exe 裡只有檔名，samply 就找不到 PDB、改用 exe 的 `.pdata`，名稱只有位址（`--symbol-dir`、在 exe 的目錄裡跑都沒用）。經過 `cargo --config` 的 `target.x86_64-pc-windows-msvc.rustflags`（和 `.cargo/config.toml` 的合併）：`RUSTFLAGS` 會蓋掉設定檔裡的 `/NODEFAULTLIB:libcmt.lib`。`--against` 的 build 照它自己的 target 目錄給路徑。只有 `perf` profile 帶：dev 帶了會和 `cargo test` 的 build 的 rustflags 不同，兩者輪流整個重編。
- **Windows 自己的 DLL 的符號**：samply 錄的時候，DLL 在磁碟上就用 DLL 本身（export 的名稱，其他的照 unwind table 取成 `fun_<位址>`），不去 symbol server 找 PDB，ntdll 的 heap、vcruntime140 的 `memcpy`、kernel 都沒有名稱。所以 `speedometer.py` 讀樣本時（`samply_samples`）找出 main thread 上這樣沒有名稱的 frame（`nameless_frames`；不算 JIT 的），複製一份 profile、拿掉那些 DLL 的路徑，對它啟動 `samply load --windows-symbol-server …`，經過 Firefox profiler 的 symbolication API（`/symbolicate/v5`）一次查完再關掉（`windows_names`、`symbolicate`），查到的名稱優先於 samply 的（`frame_namer`）。第一次要下載 PDB（ntdll、kernel 等，2026-10-21 約 48 秒），之後在 samply 的快取裡（0.3 秒）；查不到時（沒有網路）印一行，照舊用 `fun_…`。kun 的名稱照樣在錄的時候拿到，不經過這一步。
- **時鐘**：`read_samples` 在 Windows 上也走 `samply_samples`，offset 是 profile 的 `meta.startTime` 減掉 kernel ETL 的 header 事件的 QPC（`etl_start_ns`，換成 ns）。trace 在 Windows 上就是 QPC，不用再換。不用 wall clock：samply 的 `startTime` 是它自己啟動的時間，和樣本的基準差幾百 ms、每次不同。`sample_suites` 讀完就刪掉兩個 ETL（一個 suite 約 67 MB）；刪 `/tmp/perf-<pid>.map` 和 V8 的 log 只在 Linux、macOS 做。
- **提權**：`cmd_sample` 在 Windows 上先檢查 samply 和 xperf（`xperf`：`PATH` 或 Windows Kits 的位置，和 samply 找的地方一樣）在不在，再 build kun（`--against` 時兩個版本都先 build，也先 checkout）。取樣的工作（`--against` 是每一輪、每一邊，照輪流的順序）交給 `sample_all`：沒有權限（`IsUserAnAdmin()`）時整份交給 `sample_elevated`，用 `common.py` 的 `run_elevated`（`ShellExecuteEx` 的 `runas`，console 藏起來）以系統管理員啟動 `speedometer.py --elevated <state>`。提權的 process 由系統建立、不繼承環境變數，所以 state（`elevated.json`）帶著原本的環境、目錄、工作和測試會改的全域變數；它把輸出寫進 `elevated.log`（原本的 process 一邊讀一邊印），結果（fold 好的 stack、失敗與否）寫進 `elevated-results.json`，報告由原本的 process 寫。它不碰 git 和 cargo。UAC 被拒絕或它沒交回結果時，工具停下。
- `sample.md` 開頭的說明照平台寫（Windows 上不是 frame pointer、也沒有 `--perf-basic-prof`）。

### 綁核心

- `common.py` 的 `fastest_windows_cpus`：`GetSystemCpuSetInformation`（`windows_cpu_sets`、`efficiency_classes`）的 efficiency class 最高的那些邏輯 CPU，只從工具的 process 能用的 CPU 裡挑；全部一樣、有不知道的 CPU、或 CPU 不只一個 processor group（超過 64 個，一個 affinity mask 放不下）時是 `None`，不綁。和 Linux 的「最高時脈最高的那些」（`fastest_cpus`）共用挑選的部分（`fastest_of`）。
- 啟動瀏覽器不能包一層指令（Windows 沒有 `taskset`）：`browser_affinity` 在啟動 kun（或包著它的 samply）和 Chrome 的那一刻，把工具自己的 affinity 設成那組 CPU（`process_affinity`，`SetProcessAffinityMask`），子 process 在建立時繼承，啟動完設回原本的（失敗時也是），讓 cargo 的 build 照樣用所有 CPU。Chrome 由 broker 建立的 GPU process、renderer、utility process 也繼承（第 2 段確認）。其他平台不動 affinity。

### 測試

- `blink_perf_test.py` 的 `CpuTests`：從假的 CPU set 資料挑出 P-core、只挑允許的、不綁的情況；Windows 上兩個瀏覽器啟動時的 affinity、之後和失敗時還原；Linux 上不動 affinity；在 Windows 上真的讀一次 CPU set、設一次 affinity 再設回去。
- `speedometer_test.py`：Windows 的 samply 指令和 V8 旗標、ETL 的名稱和刪除、從假的 ETL header 讀出基準、樣本的時間；`perf` profile 的 PDB 路徑（`--against` 的也是）；沒有名稱的 frame、拿掉路徑的複本、查到的名稱優先、查不到時照舊，真的 `samply load` 啟動、回答、結束；提權的那一份自己（同一個 process 代替：環境、目錄、全域變數、結果、失敗）；沒有 xperf 時停下；報告寫明怎麼取樣。測試模組把 `run_elevated` 換掉，任何測試要求提權就失敗，不會在測試中間跳 UAC。
- 真的經過 ETW 取樣的 `SampleTests` 在 Windows 上只在以系統管理員跑測試時跑；`run_elevated` 本身只手動跑過（`docs/test-gaps.md`）。
- mutation test：`mutants-2.toml`、`mutants-3.toml`，結果在 `progress.md`。

### 分階段

1. **原型**（手動，不改工具；2026-10-20）：`prototype/sample.py` 提權之後用 samply 跑一個 suite，確認 stack、kun 的函式名稱、JS 的名稱、`threadCPUDelta`、時鐘和權限；結果在 `details.md` 的「原型」。原型不合併，留在這個資料夾。
2. **綁核心**（2026-10-20）：`fastest_windows_cpus` 和 `browser_affinity`；量了綁和不綁的差別。
3. **接進工具**（2026-10-20 到 21）：取樣、提權、Windows 的 DLL 的名稱、測試、文件。

## 缺點

- 多一個要裝的東西（Windows Performance Toolkit 的 xperf）；每次取樣跳一次 UAC，提權的 process 在跑的時候，原本的終端機按 Ctrl+C 停不了它（`docs/limitations.md`）。
- samply 在 Windows 上讀 V8 的事件、算 CPU 時間、找符號的方式是它自己的，samply 改版時可能跟著變（macOS 上也一樣）；DLL 的名稱另外依賴 `samply load` 回答 Firefox profiler 的 symbolication API。
- 第一次取樣要從 Microsoft 的 symbol server 下載 PDB（約一分鐘）。

## 理由和其他做法

- **samply**（選這個）：背後也是 xperf 錄的 ETW，輸出和 macOS 同一種格式，讀 profile、fold、報告的程式全部沿用，它也幫忙對應 JS 的名稱和 PDB 的符號。
- **WPR / xperf 直接錄、自己解析 ETL**：資料最完整（可以加其他 provider），但要新寫 ETL 的解析和 PDB 的符號化，只有 Windows 用得到。
- **自己寫取樣器**（另一條 thread 暫停 main thread、`RtlVirtualUnwind`）：Chromium 的 `StackSamplingProfiler` 是這個做法，頻率低、精度比不上 ETW 的 kernel 取樣，還要自己做符號化。
- **不綁核心**：Windows 上不綁時兩個瀏覽器常常整頁在 E-core 上，一頁的倍數兩次差到 2.2 倍（`progress.md`）。

完整的比較在 `details.md` 的「沒選的做法」。

## 先例

- Chromium 在 Windows 上的 profiling 用 ETW：UIforETW 或 WPR 錄、WPA 看，Chrome 加上 `--js-flags=--enable-etw-stack-walking`（加 `--interpreted-frames-native-stack`）時，V8 經過 ETW 交出 JS 函式的位置和名稱，系統的 DLL 由 Microsoft 的 symbol server 命名。那是給人互動看的流程；kun 要的是可以 fold、可以比較兩個版本的文字輸出，所以用 samply 讀同一種資料。
- Chromium 的 `base/profiler/` 的 `StackSamplingProfiler` 在 Windows 上自己暫停 thread、展開 stack，用在 UMA 的低頻率取樣，不是給開發者找熱點用的。
- Chromium 的 perf bot 不綁核心，靠固定的機器；kun 在開發機上量，所以照 Linux 的做法綁。

## 已決定的事項

2026-10-20 確認：

1. **Windows 上的取樣器用 samply**，不用 WPR / xperf 加自己的 ETL 解析，也不自己寫取樣器（「理由和其他做法」）。
2. **取樣要系統管理員的權限，一次執行只要求一次**：工具自己以系統管理員啟動取樣的那一份（UAC 跳一次），使用者只要按「是」；不靠 samply 自己的 UAC，因為每個 suite 一次 `samply record`，每次都會跳一次。（原本是「要求從系統管理員的終端機跑、沒有就停下」，同一天改成這樣：之後由工具自己跑完整個流程，人只在提權時介入。）
3. **Windows 上也綁核心**，挑 efficiency class 最高的那些 CPU，Speedometer 和 Blink 的 perf tests 都綁；獨立成一段，因為它改的是 Windows 上所有的量測，不只取樣。
4. **JS 函式拿不到名稱時接受**：分析主要看 kun 的函式。（原型裡拿得到，用不到這一條。）
5. **分三段**，原型在最前面：原型的結果可能改變第 3 段的做法。

2026-10-20 原型做完之後確認（第 3 段的做法）：

6. **樣本的時間從 kernel ETL 的 header 起算**：samply 加 `--keep-etl`，讀 header 事件的 QPC 當基準，讀完刪掉 ETL；不用 samply 的 `startTime` 換算 wall clock（它是 samply 啟動的時間，每次差幾百 ms）。
7. **Windows 的 `perf` profile 的 build 帶 `/PDBALTPATH:<絕對路徑>`**，經過 `cargo --config` 的 `target.x86_64-pc-windows-msvc.rustflags`（和 `.cargo/config.toml` 的合併），samply 才找得到 kun 的 PDB；不用 `RUSTFLAGS`（會蓋掉 `/NODEFAULTLIB:libcmt.lib`），`--symbol-dir` 沒用。
8. **提權用 `ShellExecuteEx` 的 `runas`，以系統管理員跑的是取樣的工作，不是整個指令**：build（和 `--against` 的 checkout）在提權之前做完，取樣的工作整份交給以系統管理員跑的 `speedometer.py`，它交回 fold 好的結果，報告由原本的 process 寫；環境變數經過檔案傳過去，輸出經過 log 印在原本的終端機。原本（2026-10-20）是以系統管理員重新跑同樣的 `bench.py` 指令，實作時改成只交出取樣：提權的 process 不必再碰 git 和 cargo（`--against` 會 checkout），`--against` 的幾輪也只跳一次 UAC。
9. **Windows 的 DLL 經過 Microsoft 的 symbol server 命名，在錄完之後**（「詳細設計」的「取樣」）：計時區間裡沒有名稱的 self time 有 13%，大多在 ntdll 和 vcruntime140；這樣查完之後只剩沒有公開符號的驅動程式（AMD 的顯示卡驅動，0.1%）。Chromium 在 Windows 上取樣也用這個 symbol server。原本（2026-10-20）是給 `samply record` 加 `--windows-symbol-server`，實作時發現 samply 錄的時候只要 DLL 在磁碟上就不去 symbol server 找（2026-10-21），改成錄完之後用 `samply load` 查。比較過：接受沒有名稱（不知道 leaf 是 heap 還是 `memcpy`）、修 samply（不打算做）。
10. **kernel 裡的 frame 照錄**：那是這條 thread 用掉的 CPU 時間（約 5%，page fault、驅動程式），掛在 kun 的函式底下；比較兩個版本時兩邊一樣。和 Linux 只取使用者空間不同，`docs/benchmarks.md` 寫明。

## 待決定

無。

## 之後的可能

- 取樣 `bench.py blink-perf` 的頁面（Linux、macOS 也還沒有）。
- 用 samply 留下的 ETL（`--keep-etl`，現在讀完就刪）在 WPA 裡看 kun 和 Chrome 的同一頁（Chrome 的 stack 靠 Chrome 的 symbol server）。
- Windows 上 C++ 那一側（V8、Skia）的配置走系統的 heap：2026-10-21 的 TodoMVC-jQuery 裡 `RtlpLowFragHeapAllocFromContext` 的 self time 4.9%，和 Rust 那一側的 mimalloc（4.4%）差不多。
