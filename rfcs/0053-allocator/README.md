# RFC 0053：Rust 的 allocator 換成 mimalloc

- 開始日期：2026-10-17
- 狀態：✅ 已完成：2026-10-18（`kun` 的 Rust 那一側換成 mimalloc 的預設設定，三個平台都是；在 macOS 上量：Speedometer 快 7.9%、記憶體峰值多 12.7%，`MIMALLOC_PURGE_DELAY` 讓使用者調；`--against` 不帶 `--sample` 時比時間和記憶體峰值；Linux、Windows 上沒量，使用者自己量）
- 細節：`details.md`（kun 的記憶體放在哪裡、各家 allocator 的比較、Windows 上的基準、Linux 上拆分取樣的方法）
- 進度和驗收：`progress.md`

## 摘要

kun 原本整個 process 都用平台預設的 allocator：Linux 是 glibc 的 `malloc`，macOS 是 libmalloc，Windows 是 process heap 的 `HeapAlloc`。DOM 節點和 JS 物件在 cppgc 和 V8 自己的 heap，但節點裡的屬性、class、文字、computed style，以及 parser、style 的暫存，都在 Rust 的 heap。這份 RFC 讓 `kun` 的執行檔用 `#[global_allocator]` 換成 mimalloc（v3，預設設定），三個平台都一樣；C++ 那一側（V8、Skia、`binding.cc`）照舊。為了量，`ci.py speedometer --against` 不帶 `--sample` 時，兩個版本輪流比時間和記憶體峰值。在 macOS（Apple M4）上量：Speedometer 的時間快 7.9%（20 個 suite 都變快），記憶體峰值多 12.7%，超過事先定的記憶體門檻，看過結果決定以速度為優先；`MIMALLOC_PURGE_DELAY=0` 時快 5.2%、記憶體多 6.4%，寫進 `kun --help` 讓使用者自己調。Linux、Windows 上沒量。

## 動機

**配置是 DOM 操作的一大塊**：RFC 0051 第 2 段的 2d 之後的取樣（Linux，cycle）裡，TodoMVC-jQuery 的 script 時間有 21% 在 libc 的配置和複製，ES6-Webpack 22%、Svelte 17%，JS 本身只有 3%。RFC 0051 拿掉了其中多餘的配置（UA shadow tree 的 CSS 重複 parse、`ComputedStyle::initial()` 的 `Arc`、`Persistent` 的 `new`），剩下的是本來就要做的配置，它們的單位成本取決於 allocator。

**三個平台的 allocator 不同**：同一份程式在三個平台上的配置成本不一樣，Speedometer 在 Linux 和 Windows 上量到的比值也就不能直接比。Chromium 在所有桌面平台都把 `malloc` 換成 PartitionAlloc（PartitionAlloc-Everywhere）。

**時機**：原本打算在 Windows 上和 RFC 0051 平行做；0051 做完之後改在 macOS（Apple M4）上做（「已決定的事項」第 5 條）：這台的雜訊小，`--sample` 在 macOS 上也能用了，計時區間裡 14.2% 的 CPU 時間在 libmalloc，依呼叫者分約九成是 Rust 的配置。

**目標**：
- 量出換成 mimalloc 對 Speedometer 的每個 suite 的效果，連同記憶體的峰值。
- 照事先定的門檻（第 8 條）採用或否決，結果記在這裡。

**不在範圍內**：
- C++ 那一側的配置（V8 的 libc++ 的 `operator new`、Skia 的 `sk_malloc`、`binding.cc` 的 `new`）：要整個 process 換掉 `malloc`，Windows 上做不好（「理由和其他做法」）。
- cppgc 和 V8 的 heap：它們不經過 `malloc`。
- 減少配置的次數：RFC 0051 的範圍。
- 照用途分開的 partition、安全性的強化（Chromium 的 MiraclePtr、WebKit 的 isoheap）。
- Linux、Windows 上的量測（「之後的可能」，第 10 條）。

沒有要解決的 `docs/limitations.md` 裡的限制；新的限制寫在那裡：記憶體峰值多了，Linux 和 Windows 上沒量。

## 設計概要

網頁的行為不變，`kun` 的使用者看不出差別。對改程式的人：
- `kun` 的 `main.rs` 有一行 `#[global_allocator]`，`kun` 這個 process 裡所有 Rust 的配置（`Vec`、`String`、`Box`、`Arc`）都經過 mimalloc。
- `cargo test` 的測試 binary 照舊用平台的 allocator；經過 `kun` 執行檔的測試（WPT、reftest、情境、`shell` 裡啟動 `kun` 的測試）用 mimalloc。
- Rust 配置的記憶體不能交給 C++ 的 `free` 釋放，反過來也不行。現在沒有這種地方（`details.md` 的「跨 allocator 的釋放」），之後寫 FFI 時要照這條。
- `MIMALLOC_PURGE_DELAY`（mimalloc 啟動時自己讀）：空出來的記憶體留多久（ms，預設 10）才還給系統；`0` 立刻歸還，記憶體峰值約少一半、時間約慢 2.7%。寫在 `kun --help` 和 README。
- `ci.py speedometer --against REV` 不帶 `--sample` 時比時間和 `kun` 的記憶體峰值（`compare.md`）；帶 `--sample` 時照舊比樣本。

## 詳細設計

### 換 allocator

- 依賴：`mimalloc` crate（0.1.52；編 upstream 的 mimalloc 3.3.2，要 C 編譯器，Windows 上是 MSVC 的 `cl`）。在 workspace 的 `[workspace.dependencies]`，只有 `shell` 用。不開 `override`、`secure`；`shell` 的 dev-dependencies 開 `extended`（只有測試的 build 有：mimalloc 的統計）。
- `crates/shell/src/main.rs`：`#[global_allocator] static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;`，註解寫明為什麼（Chromium 的 PartitionAlloc 的對應）、只換 Rust 的部分。
- 之後拆 process 時，renderer 照 Chromium 用同一個執行檔（`--type=renderer` 那樣），所以一樣用 mimalloc，不用另外處理。
- `docs/architecture.md` 的「Crate 選型」有 `mimalloc` 這一列。

### 量測

原本的 `--against` 只比樣本（`--sample`）。選了 **M1**（「已決定的事項」第 3 條），讓 `--against` 不帶 `--sample` 時比時間：兩個版本輪流（另一個版本先），各跑 `--runs` 輪（預設 3），每輪每個 suite 一次（`--iterations`，預設 10）。`compare.md` 列每個 suite 兩邊**所有 iteration**的總時間的中位數和 95% 區間、變化，`kun` 的記憶體峰值（各輪的中位數）和變化，有效的 suite 的變化的幾何平均，和每一輪的中位數（看漂移）；有一輪無效的 suite 不比。三個平台都能用，之後的優化也用它比時間。

| | 做法 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **M1** | `--against` 不帶 `--sample` 時比計時 | 三個平台都能用；交替跑，抵銷機器狀態的漂移 | 改 `scripts/speedometer.py`，RFC 0051 也用它（改的地方不同，見「缺點」） |
| M2 | 不改工具，手動各 build 一次、輪流跑 `ci.py speedometer --browser kun` | 不改程式 | 兩個版本都 build 到 `target/perf`，要手動換執行檔，容易出錯；沒有記憶體 |
| M3 | Windows 上也做取樣（ETW：`samply` 或 WPR） | 拿得到 Rust 和 C++ 的配置各佔多少 | 要系統管理員的權限、新的工具和格式轉換，工作量比換 allocator 本身大很多 |

不選 M3：換 allocator 只有兩行，直接比的成本比先拆出上限還低。Linux 上已經有的取樣，等 0051 那台有空時用 `details.md` 的「Linux 上拆分取樣的方法」拆，當作補充。

**記憶體**：mimalloc 可能留住比較多的記憶體，所以要一起量。每個 suite 的報告送回之後、關掉 `kun` 之前，讀 `kun` 這個 process 的峰值（`peak_memory`，Python 的 `ctypes`，不加依賴）：macOS 是 `proc_pid_rusage`（`RUSAGE_INFO_V4`）的 lifetime max phys footprint（第 7 條），Linux 是 `/proc/<pid>/status` 的 `VmHWM`，Windows 是 `GetProcessMemoryInfo` 的 `PeakPagefileUsage`（private bytes 的峰值；還沒在 Windows 上跑過，`docs/test-gaps.md` 的「測試守不住的地方」）。被取樣時不讀（那時的 process 是取樣的工具）。

**判斷**（「已決定的事項」第 8 條）：20 個 suite 各 3 次（每次 10 個 iteration），兩個版本輪流。照每個 suite 的中位數比值：幾何平均快 2% 以上、沒有 suite 慢超過 5%；記憶體峰值的比值：幾何平均多不超過 10%、沒有 suite 多超過 30%。兩項都過就採用，否則否決。

**結果**（macOS，Apple M4，接電源；`progress.md`）：

| | 時間（幾何平均） | 最慢的 suite | 記憶體峰值（幾何平均） | 記憶體最多的 suite |
| --- | --- | --- | --- | --- |
| A/A（同一份程式比自己） | +0.3% | +2.4% | −1.2% | +7.4% |
| mimalloc 的預設設定（`purge_delay` 10 ms） | **−7.9%** | −3.2%（20 個都變快） | **+12.7%** | TodoMVC-jQuery +43.0% |
| `MIMALLOC_PURGE_DELAY=0` | −5.2% | −0.1% | +6.4% | NewsSite-Next +13.8% |

預設設定時間過、記憶體沒過；使用者看過結果決定照樣採用（第 9 條）。`--sample` 的比較：計時區間裡的 CPU 時間 20 個 suite 都變少，幾何平均 −8.2%。

### 測試

- **行為**：allocator 不改變行為，由現有的測試守著：`cargo test -p shell`，經過 `kun` 執行檔的 `test.py reftest`、`scenarios` 和 WPT 的幾個目錄。
- **確實在用 mimalloc**：`kun` 的單元測試（`main.rs` 的 `tests`，bin 的測試 binary 也套用 `#[global_allocator]`）配置 64 MB，確認 mimalloc 自己的統計（`MiMalloc::stats_json` 的 `committed`）漲了這麼多：安全的 API，不需要 `unsafe`（原本打算用 `libmimalloc-sys` 的 `mi_is_in_heap_region`，要 `unsafe`，而 `unsafe` 只在 `dom`、`bindings`、`ipc`）。拿掉 `#[global_allocator]` 時這個測試失敗（mutation test）。`kun --help` 的測試檢查 `MIMALLOC_PURGE_DELAY`。
- **量測工具**：`speedometer_test.py` 的 `CompareTests`（不帶 `--sample` 時兩個版本輪流計時、`compare.md` 的格式，用假的結果）、`MemoryTests`（另起一個 process 碰 64 MB 再歸還，讀它的峰值；Linux 的解析）、`KunTests.test_kuns_peak_memory_with_its_run`。
- **量測本身**：Speedometer 平常不量（`docs/benchmarks.md` 的「效能的量測」）。這份 RFC 的目的就是量測，使用者要求量（「已決定的事項」第 1、5 條），所以第 2 段排了量測。

### 分階段

1. **量測工具**（完成）：`--against` 不帶 `--sample` 時比計時，加上記憶體的峰值（M1）；A/A（`--against HEAD`）量雜訊，定「判斷」的門檻（第 8 條）。
2. **換 allocator 和量測**（完成，macOS）：`kun` 換成 mimalloc，`--against main` 量 20 個 suite；記憶體超過門檻，照樣採用（第 9 條）。
3. **其他平台和其他 allocator**（沒做，第 10 條）：移到「之後的可能」。

## 缺點

- **多一個 C 的依賴**：mimalloc 用 `cc` 編，每台機器要有 C 編譯器（V8 和 Skia 是 prebuilt）。開發機和 CI 的三個平台都要確認有。
- **記憶體變多**：mimalloc 延後把頁還給系統，macOS 上 Speedometer 的記憶體峰值多 12.7%（TodoMVC-jQuery 多 43%）；`MIMALLOC_PURGE_DELAY=0` 時多 6.4%。
- **兩種 allocator 並存**：Rust 用 mimalloc、C++ 用平台的。跨 allocator 釋放就是 heap corruption。現在沒有這種地方，但之後的 FFI 要記得（`crates/dom/soundness.md` 這類的論證不受影響：cppgc 的物件不經過 `malloc`）。
- **基準會移動**：換了之後，之前量的數字和之後的不能直接比；用 `--against` 比的時候兩邊都要包含這個改變。
- **Linux、Windows 上沒量**：那兩個平台的系統 allocator 和 libmalloc 不同，效果可能不一樣（第 10 條）。

## 理由和其他做法

**allocator**（完整的比較在 `details.md` 的「各家 allocator」）：

| | 速度（小物件很多） | 記憶體 | Windows（MSVC） | 建置 | 決定（「已決定的事項」第 2 條） |
| --- | --- | --- | --- | --- | --- |
| 平台預設（現在） | glibc 在多 thread、小物件多時常常最慢；`HeapAlloc` 的 LFH 中等 | 好 | 是 | 不用 | 對照組 |
| **mimalloc** | 前段 | v3 比 v2 改善 | 支援 | `cc` 編幾個 C 檔 | **採用** |
| jemalloc（`tikv-jemallocator`） | 前段 | 長時間跑時的強項 | **不支援** | autoconf 的 configure | 不選：Windows 要另外一套；rustc 的 Windows 版也因此不用它 |
| snmalloc（`snmalloc-rs`） | 前段，跨 thread 的釋放特別好 | 好 | 支援 | CMake（或 `build_cc` feature） | 備選，之後可以比 |
| tcmalloc（新版） | 前段，每個 CPU 一份 cache | 好 | 實際上只有 Linux | Bazel | 不選 |
| PartitionAlloc | 為瀏覽器調過 | 好 | 支援 | Chromium 的 GN | 不選：接進 Rust 的成本太高 |

**範圍**：
- **B：只換 Rust 的部分（採用）**：兩行程式；Rust 的配置包括節點裡的資料、parser、style，是 DOM 操作的主要部分（`details.md` 的「kun 的記憶體放在哪裡」）。
- C：整個 process 換掉 `malloc`（mimalloc 的 `override`），是 Chromium 的做法，連 V8、Skia、`binding.cc` 都換。Linux 上在執行檔裡定義 `malloc` 就會蓋掉；macOS 要接 malloc zone；Windows 要 `mimalloc-redirect.dll` 和匯入的順序，和 Rust 的靜態連結搭不起來。不選：三個平台的做法不一致，而 C++ 那一側在 kun 裡比較小的一塊（`Persistent` 已經重複使用）。B 量過之後，如果 Linux 上拆出來 C++ 的配置仍然明顯，再考慮只在 Linux 上做。
- A：不換，只靠 RFC 0051 減少配置。不選：0051 拿掉的是多餘的配置，本來就要做的配置的單位成本不會因此降低，兩者是相乘的。

**放在哪裡**：
- **只在 `kun` 的執行檔（採用）**：效能只和 `kun` 有關；`#[global_allocator]` 只能在整個連結的圖裡出現一次，放在執行檔最清楚。
- 也放在 `machopodriver`：它只轉送 WebDriver 的指令，配置不多，不換。
- 放在一個 library crate，讓所有的測試 binary 都用：測試跑在 mimalloc 上，但每個 crate 的測試都要依賴它，而行為不受 allocator 影響。不選。

## 先例

- **Chromium**：PartitionAlloc-Everywhere 用 allocator shim 把整個 process 的 `malloc`、`free`、`operator new` 導到 PartitionAlloc，所有桌面平台和 Android 都是；換之前 Windows 用的是 Windows 的 heap（經過同一個 shim）。Blink 的 `WTF::String`、`Vector`、`HashTable` 在 PartitionAlloc 的幾個 partition 裡，DOM 節點在 Oilpan（cppgc）。V8 的 JS 物件在自己的 heap，parser 和 compiler 用 Zone。kun 的 B 對應的是「Blink 的非 GC 的資料不用系統的 allocator」這一半。
- **Firefox**：整個 process 用自己的 mozjemalloc。
- **WebKit**：bmalloc，之後是 libpas（照型別隔離的 isoheap）。
- **Rust 的專案**：rustc 在 Linux、macOS 上用 jemalloc，Windows 上用系統的；ruff、uv 在 Windows 上用 mimalloc、其他平台用 jemalloc；Bun（Zig）用 mimalloc。

## 已決定的事項

1. **先寫 RFC，在 Windows 上量**（2026-10-17，使用者）：Linux 的機器在做 RFC 0051，這份在 Windows 上平行做，第 2 段的 Speedometer 量測是使用者要求的。Linux 上的量測和拆分等那台有空。2026-10-18 改在 macOS 上做第 1、2 段（第 5 條）。
2. **mimalloc，只換 Rust 的部分，只在 `kun` 的執行檔**（2026-10-17）：
   - **mimalloc**：三個平台（包括 Windows 的 MSVC）一種就好，小物件的速度在前段，用 `cc` 編。不選 jemalloc：`tikv-jemallocator` 不支援 `*-windows-msvc`，Windows 要另外一套；不選 snmalloc：kun 的配置大多在同一個 thread 上配置和釋放，它跨 thread 的強項用不到，留到第 3 段可以比；不選 tcmalloc（只有 Linux）、PartitionAlloc（接進 Rust 的建置成本太高）。
   - **只換 Rust 的部分**（`#[global_allocator]`）：節點裡的資料、parser、style 的配置都在 Rust 這一側。不選整個 process 換掉 `malloc`（Chromium 的 PartitionAlloc-Everywhere）：Windows 上要 `mimalloc-redirect.dll` 和匯入的順序，和 Rust 的靜態連結搭不起來，三個平台的做法也不一致；C++ 那一側的 `Persistent` 已經重複使用。Linux 上拆出來 C++ 的配置仍然明顯時，再考慮只在 Linux 上做。
   - **只在 `kun` 的執行檔**：效能只和 `kun` 有關，`#[global_allocator]` 放在執行檔最清楚；經過 `kun` 執行檔的 WPT、reftest、情境都在 mimalloc 上跑。不選 `machopodriver`（只轉送 WebDriver 的指令）；不選放在 library crate 讓所有的測試 binary 都用（每個 crate 的測試都要依賴它，而行為不受 allocator 影響）。
3. **量測的工具：M1**（2026-10-17）：`--against` 不帶 `--sample` 時，兩個版本交替各跑 `--runs` 次，比每個 suite 的計時，加上 `kun` 的記憶體峰值。三個平台都能用，之後 RFC 0051 也能用它比時間。不選手動輪流跑（M2：兩個版本都 build 到 `target/perf`，容易換錯，也沒有記憶體）；不選 Windows 上的取樣（M3：ETW 要系統管理員的權限和新的工具，工作量比換 allocator 本身大，Linux 上的拆分留作補充）。
4. **mimalloc v3**（2026-10-17）：crate 預設的版本，upstream 現在的主線，記憶體比 v2 好。不開 `v2` feature；第 2 段量到 v3 有回歸時再比 v2。
5. **第 1、2 段改在 macOS 上做**（2026-10-18，使用者）：RFC 0051 做完了，優化的工作移到這台 Apple M4；這台的雜訊小（同一個版本連量三次，kun 合計差 1.6%、單一 suite 的差距中位數 1.9%），而且 `--sample` 在 macOS 上能用了：計時區間裡 14.2% 的 CPU 時間在 libmalloc，依呼叫者分約九成是 Rust 的配置。Windows 和 Linux 移到第 3 段（glibc、Windows 的 heap 和 libmalloc 不同，結果可能不同）。
6. **第 2 段另外跑 `--against --sample`**（2026-10-18，使用者）：判斷照 M1 的計時；樣本比較換之前和換之後 malloc 的 CPU 時間，用來解釋效果。
7. **macOS 上的記憶體峰值是 `proc_pid_rusage`（`RUSAGE_INFO_V4`）的 `ri_lifetime_max_phys_footprint`**（2026-10-18，使用者）：suite 跑完、關掉 `kun` 之前讀。是 Activity Monitor 的「記憶體」（包含壓縮的頁），Chromium 在 macOS 上的記憶體指標（private memory footprint）也用 phys footprint。不選 `getrusage(RUSAGE_CHILDREN)` 的 `ru_maxrss`（只給已結束的子 process 裡最大的，跨 suite 累積），也不選用 `/usr/bin/time -l` 包著 `kun`（要從 log 解析，和取樣的 wrapper 衝突）。
8. **採用的門檻**（2026-10-18，使用者，看過第 1 段的 A/A）：時間照每個 suite 的中位數比值，幾何平均快 2% 以上、而且沒有 suite 慢超過 5%（T1）；記憶體峰值的比值，幾何平均多不超過 10%、而且沒有 suite 多超過 30%（M-a）。兩項都過就採用。A/A（同一份程式比自己）：時間的幾何平均 +0.3%、單一 suite −2.9% 到 +2.4%；記憶體峰值的幾何平均 −1.2%、單一 suite −17.8% 到 +7.4%。不選幾何平均快 1% 以上（T2：只有雜訊的三倍多，也不檢查單一 suite 變慢）；不選原本的「沒有 suite 的峰值多 15% 以上」（M-b：A/A 裡同一份程式就有 suite 差到 18%，會被雜訊否決，要重跑確認又慢）。
9. **採用 mimalloc 的預設設定，記憶體用 `MIMALLOC_PURGE_DELAY` 調**（2026-10-18，使用者，看過第 2 段的結果）：預設設定（`purge_delay` 10 ms）時間的幾何平均快 7.9%、20 個 suite 都變快，記憶體峰值的幾何平均多 12.7%、jQuery 多 43%，**超過第 8 條的記憶體門檻**；使用者以速度為優先照樣採用。`MIMALLOC_PURGE_DELAY=0`（空出來的頁立刻還給系統）時快 5.2%、記憶體多 6.4%，兩項門檻都過：mimalloc 啟動時自己讀這個環境變數，所以不寫程式，寫進 `kun --help` 和 README，讓在意記憶體的使用者自己設。不選把 kun 預設成 0（快的部分少三分之一）；不選在 `bindings` 加 kun 自己的參數（`MACHOPO_ALLOCATOR_PURGE_DELAY` 之類：要一行 `unsafe` 呼叫 `mi_option_set`、`bindings` 多依賴 `libmimalloc-sys`，功能和 mimalloc 自己的環境變數重複）。
10. **不做第 3 段就收尾**（2026-10-18，使用者）：Linux、Windows 上的量測由使用者自己手動做，移到「之後的可能」。三個平台的 `kun` 都已經用 mimalloc（`#[global_allocator]` 沒有平台的條件）。沒有改成先只在 macOS 上開（`#[cfg(target_os = "macos")]`，等量過再開其他平台）：使用者會自己量，照現在的樣子收尾。

## 待決定

沒有。

## 之後的可能

- **Linux、Windows 上的量測**（原本的第 3 段，第 10 條）：用 `--against` 比時間和記憶體；Windows 上順便確認讀記憶體峰值的程式（`docs/testing.md`）。Linux 上用取樣拆出 C++ 的配置佔多少，決定要不要在 Linux 上整個 process 換掉 `malloc`；需要的話同一個工具比 snmalloc。
- Windows 上的取樣（ETW），讓 `--sample` 在 Windows 上也能用。
- 照用途分開的 heap（例如 parser 的字串、style 的暫存用 arena），和 RFC 0051 減少配置的方向一起看。
