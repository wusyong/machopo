# RFC 0066 的細節：Windows 上的取樣和綁核心

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## samply 在 Windows 上怎麼錄（2026-10-20 讀的原始碼，samply 0.13.1）

- **錄**：samply 找 xperf（PATH，或 Windows Kits 底下的 Windows Performance Toolkit；找不到就停下、請人安裝），開兩個 session：kernel 的 `xperf -on PROC_THREAD+LOADER+PROFILE+CSWITCH -stackwalk PROFILE+CSWITCH`（`-SetProfInt` 照 `-r` 的頻率，最短 0.1221 ms），和有 user provider 時的 `SamplySession`。
- **權限**：samply 用 `runas` 把自己以 `run-elevated-helper` 重新啟動，提權由 UAC 決定；helper 失敗時說 Windows 上的 profiling 要系統管理員。
- **`--browsers`**：打開 `Microsoft-JScript:0x3`（JIT 的符號）和 Chrome 的 provider（只有 `blink_user_timing` 這個 keyword）。解析 ETL 的地方處理 `V8.js/SourceLoad/Start`、`V8.js/MethodLoad/Start` 和 Microsoft-JScript 的同類事件，只收錄著的 process 的。V8 的 `V8.js` provider 和 Chakra 的 Microsoft-JScript 是同一個 GUID（`57277741-3638-4A4B-BDBA-0AC6E45DA56C`，WPA 才認得它的事件），所以 `--browsers` 打開的就是 V8 的：原型裡 JS 的名稱都在（「原型」）。
- **時間**：每個樣本的時間是它的 ETW 時間戳（QPC）減掉 kernel ETL 的 header 事件的時間戳；profile 的 `meta.startTime` 卻是 samply 啟動時的 `SystemTime::now()`，和前者沒有關係（「原型」）。
- 原始碼：<https://github.com/mstange/samply/tree/main/samply/src/windows>（`xperf.rs`、`chrome.rs`、`etw_gecko.rs`、`elevated_helper.rs`），`samply record --help`。

## kun 這一側

- 開發機的 V8（`../third_party/rusty_v8_artifacts` 的 Windows 版）裡有 `enable-etw-stack-walking` 這個旗標的字串，ETW 的 JIT logger 有編進去（V8 的 `v8_enable_etw_stack_walking` 在 Windows 上預設打開）。
- `perf` profile 是 `debug = "line-tables-only"`，MSVC 上照樣產生 PDB，函式名稱從它來。
- 開發機有 Windows Performance Toolkit（`xperf.exe`、`wpr.exe`）。

## 原型

2026-10-20，Windows 11（i9-13900K），samply 0.13.1，kun 的 `perf` profile，TodoMVC-jQuery 跑 5 個 iteration。`prototype/sample.py` 直接呼叫 `speedometer.py` 的 `sample_suites`，只換掉取樣器（samply 加 `--browsers --keep-etl`、V8 的旗標）和讀樣本的 offset；從一般權限的 session 用 `Start-Process -Verb RunAs` 啟動它。

- **權限**：整次只跳一次 UAC（啟動原型的那一次）。已經提權的 process 啟動 samply 時，它的 elevated helper 不再跳 UAC。提權的 process 寫出的檔案，一般權限的 process 照樣讀得到、搬得動（都在使用者自己的資料夾底下，權限從資料夾繼承）。整個 suite 連 samply 寫 profile 約 10 秒。
- **thread**：名稱都在（`CrRendererMain`、`Compositor`、`VizCompositorThread`……；沒取名的是 `Thread <tid>`），每個樣本都有 `threadCPUDelta`（µs），和 macOS 一樣，`read_samply_profile` 不用改。
- **JS 的名稱**：有，V8 經過 ETW 交出，samply 的 `--browsers` 收得到（「JS JIT」這個 library）：`render …/app.js:68:26`、`y.handle …/jquery.min.js:2:41091`、`RegExp.< src: '<script|<style|<link' flags: 'i'`，函式名稱加上網址、行和欄。所以 `README.md` 的第 4 條（拿不到名稱時接受）用不到。
- **kun 的函式名稱**：一開始沒有，全部是 `fun_10b1030` 這種位址（samply 從 exe 的 `.pdata` 切出函式的範圍、自己取的名字）。原因：rustc 在 MSVC 上一律給 linker `/PDBALTPATH:%_PDB%`，exe 的 CodeView 只記 `kun.pdb` 這個檔名；samply 找不到它，就用 profile 記著的 `kun.exe`，有了 `.pdata` 就不再找 PDB。`--symbol-dir target/perf` 沒用（exe 先找到了），在 `target/perf` 底下啟動也沒用。拿 `samply load` 的 symbolication API 一個個試過：PDB 本身讀得出名稱、inline 和行號（`line-tables-only` 也可以，用小程式比過 `debuginfo=2`），profile 的 debugPath 是 PDB 的絕對路徑時就查得到。lld-link 用最後一個 `/PDBALTPATH`，所以 build 時多給 `-C link-arg=/PDBALTPATH:<絕對路徑>` 就把絕對路徑寫進 exe。這樣重編 `perf` profile（rustflags 變了，整個 profile 重編，約 6 分鐘）之後再錄一次，不用 `--symbol-dir`：kun 的函式全部有名稱（`dom::scope::DomMutationScope::create_node()`、`html::fast_path::decode()`，V8 的 `cppgc::LivenessBroker::IsHeapObjectAliveImpl`、`Builtins_LoadIC_Megamorphic`，mimalloc 的 `mi_malloc_aligned`），`.syms.json` 裡 kun 的符號沒有一個是 `fun_`。
- **時鐘**：samply 的 `meta.startTime` 是 samply 自己啟動時的 `SystemTime::now()`，樣本的時間卻是從 kernel ETL 的 header 事件的 QPC 起算（samply 的 `profile_context.rs` 的 `handle_header`），兩者差的是 samply 啟動 helper 和 xperf 花的時間：兩次分別是 +533 ms 和 −184 ms，所以照 macOS 的做法用 wall clock 換算對不準（第一次只有 51% 的樣本落在 main thread 的 slice 裡，區間裡的 CPU 時間只算到 507 ms）。kun 的 trace 在 Windows 上是 QPC（Perfetto；trace 的時間和 Python 的 `perf_counter_ns` 是同一個刻度）。改成讀 kernel ETL 的第一個事件（72 bytes 的 buffer header 之後的 `SYSTEM_TRACE_HEADER`，時間戳在第 16 byte；`TRACE_LOGFILE_HEADER` 的 `PerfFreq` 換算成 ns）當基準：兩次的最佳偏移都在 1 ms 以內（−0.5 ms，一個樣本記的是它之前那 1 ms 的 CPU 時間），94–96% 的樣本落在 slice 裡，區間裡的 CPU 時間 992 ms。ETL 一個 suite 約 67 MB，讀完 header 就可以刪。第三次（帶絕對路徑的 build）的最佳偏移也是 0 ms（94%）。
- **還沒有名稱的**：Windows 自己的 DLL。main thread 的 self time 裡，kun 78.3%、ntdll 8.7%、kernel（`ntkrnlmp` 和驅動程式）5.2%、vcruntime140 3.8%、JS JIT 3.6%；沒有名稱的 frame 佔計時區間的 self time 13%，最多的是 ntdll 的 `fun_32750`（5%）和 vcruntime140 的 `fun_1cb40`、`fun_1da00`（大概是 `memcpy`、`memset` 這一類）。samply 沒有設定 symbol server，只拿得到 export 的名稱（`RtlFreeHeap`、`NtWaitForAlertByThreadId`）；加上 `--windows-symbol-server https://msdl.microsoft.com/download/symbols` 才有這些 DLL 的 PDB（第一次要下載，之後放在 samply 的快取）。
- **kernel 的時間**：ETW 的 stack 連 kernel 一起錄（Linux 的 `cycles:u` 只算使用者空間），main thread 的 self time 有 5.2% 在 kernel（page fault、驅動程式），在報告裡是 kun 的函式底下的 frame。

## 沒選的做法

- **WPR / xperf 直接錄、自己解析 ETL**：錄的資料和 samply 的相同。解析 ETL 的選擇：TraceEvent（.NET，要多一個 runtime）、`xperf -a dumper` 的文字輸出（格式沒有承諾、很大）、`wpaexporter`（要先在 WPA 裡定好表格的設定）；三者都還要自己把位址對到 PDB 的符號和 V8 的 MethodLoad。換來的是可以多錄其他 provider（例如 GPU、記憶體），目前用不到。
- **自己寫取樣器**：另一條 thread 定時 `SuspendThread`、`GetThreadContext`、`RtlVirtualUnwind` 展開 stack（Chromium 的 `StackSamplingProfiler` 的做法）。user mode 的計時器頻率低、抖動大，暫停 thread 本身就有成本，JIT 的 frame 和符號化也要自己做。

## 參考資料

- samply：<https://github.com/mstange/samply>
- V8 和 Chromium 在 Windows 上用 ETW 取樣：<https://blogs.igalia.com/dape/2022/11/29/native-call-stack-profiling-2-3-event-tracing-for-windows-and-chromium/>
