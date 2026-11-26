# RFC 0117：renderer process 關進作業系統的 sandbox

- 開始日期：2026-11-26
- 狀態：👍 已接受：2026-11-26（第 1 段之前要定的九條；macOS、Windows 的在各自的那一段定）
- 細節：`details.md`

## 摘要

renderer 已經是自己的 process（RFC 0004），但沒有被關起來：被攻破的 renderer 照樣用使用者的身分開檔案、連網路。這份把 renderer process 關進作業系統的 sandbox：Linux、macOS、Windows 各用自己的機制，在 child 的 `main` 一開始、任何其他程式碼之前就鎖（RFC 0004 的「為 sandbox 留的條件」假設的時機），套不上就不跑。分段做，Linux 先：Landlock（檔案照路徑唯讀放行）加 seccomp-bpf（只放行清單上的 system call，其餘當掉並說是哪一個）；字型照舊由 renderer 讀，放行的路徑由 Browser 問 fontconfig；程式在新的 crate `sunaba`；開關是 `--no-sandbox`。macOS（Seatbelt）和 Windows 的機制在各自的那一段、探測之後定。2026-11-26 在 Linux 上做了第一次探測（真的 renderer 向 kernel 要了什麼），結果在 `details.md`。

## 動機

現況（2026-11-26 的 `main`，`52dc5adc`）：

- `docs/limitations.md` 的「renderer 是獨立的 process，但沒有被關起來」：V8 或 DOM 的 heap（elise）、wasmtime 的 JIT、`bindings` 的 `unsafe` 裡任何一個記憶體錯誤被利用之後，攻擊者拿到的是使用者的身分：讀寫家目錄、連網路、啟動程式、碰同一個使用者的其他 process。
- `crates/ipc/soundness.md` 的 K5（「同一個使用者的其他 process」）靠審查：Windows 啟動 child 時的繼承（`docs/limitations.md` 裡 `ipc` 在 Windows 上啟動 child 的那一條：要自己呼叫 `CreateProcessW`、指定繼承的清單，寫明和 sandbox 一起做）、pipe 建立時的保護（`docs/test-gaps.md` 寫「sandbox 做的時候重看一次」）。
- RFC 0004 的設計照「之後會有 sandbox」做（renderer 不碰檔案系統、網路、GPU，不建共用記憶體），IPC 的那一半在原型驗過；renderer 自己的那一半（字型、V8 的 JIT、wasmtime、elise 的 worker pool 在 sandbox 裡要什麼）沒驗過，探測移到這份當第一步。

**目標**：

1. 被攻破的 renderer process 做不到：讀或寫使用者的檔案（它要用的那幾樣之外）、連網路、啟動其他程式、碰其他 process（訊號、ptrace、它們的記憶體）、開新的 IPC（有名字的 socket、pipe、共用記憶體）。它剩下的路只有控制通道：Browser 對收到的每則訊息的檢查（`docs/architecture.md` 的「Process 的邊界」第 6 條）。
2. 三個平台都做，做到的保證寫得出來、各平台差在哪也寫明。
3. 鎖在最前面：renderer 的程式碼沒有一行在 sandbox 外面跑（不暖機）。套不上時 renderer 不跑（不退回沒有 sandbox）。
4. 平常的行為不變：`shell`、`browser` 的測試、WPT、情境照舊通過；Speedometer 不變慢。
5. 開發時用得到的東西照舊能用：trace、`bench.py` 的工具、DevTools、debugger（要關掉 sandbox 的寫明怎麼關、哪些工具自己會關）。

**不在範圍內**：

- V8 的 sandbox（V8 自己在 process 裡的隔離）：另一份；它不需要這份，這份也不需要它（「之後的可能」）。
- GPU process、network process、site isolation：Browser 這一側是 Rust，照 RFC 0004 的「要拆幾種 process」不拆。
- iframe 的 `sandbox` attribute（HTML 的功能，和這份無關）。
- `--single-process` 和在測試的 process 裡建分頁的測試：renderer 是 Browser 的 thread，關不起來，照舊寫在 `docs/limitations.md`。

要解決的 `docs/limitations.md` 裡的限制：「renderer 是獨立的 process，但沒有被關起來」的作業系統的那一半（V8 的那一半留著）；「`ipc` 在 Windows 上啟動 child 時」那一條（Windows 的那一段）。

## 設計概要

給 machopo 的使用者：看不到改變。renderer 當掉的畫面照舊；多一種當掉的原因（做了 sandbox 不許的事，被 kernel 結束）。

給改 machopo 的人：

- renderer 這一側（`docs/architecture.md` 的「Process 的邊界」列的 crate）的程式要向作業系統要新的東西（開檔案、讀 `/proc`、新的 system call）時，在 sandbox 裡會失敗或被結束；`shell` 的測試預設在 sandbox 裡跑 renderer，所以測試會先壞。要放行就改那個平台的 policy，寫明為什麼要。
- `machopo --no-sandbox` 關掉，給 debugger 和還不能在 sandbox 裡用的工具；啟動時印一行警告。沒有環境變數。

## 詳細設計

下面每一節是選項和比較；定了的寫明是哪一條決定，其餘是 2026-11-26 的看法。Chromium 的 Linux 那一部分對過原始碼了（`details.md` 的「Chromium 的 Linux sandbox」）；macOS、Windows 的還是照記憶寫的，那兩段定案之前對（`details.md` 的「要對原始碼的地方」）。

### 現在 renderer 向作業系統要什麼（Linux 的探測）

2026-11-26，Fedora 45（kernel 7.3），`strace -f` 底下跑 `machopo --automation-pipe` 載入一頁（幾種字型和文字、script、canvas、iframe、時區、WebAssembly、worker），只看 renderer process（22 個 thread）。完整的結果在 `details.md` 的「Linux 的探測」。

- **沒有寫過任何檔案**，沒有 socket、沒有連線、沒有自己建共用記憶體、沒有啟動程式。和 RFC 0004 留的條件一致。
- **不列系統字型時（`--test-fonts`）碰 32 個路徑，全部唯讀**：執行檔和 18 個共用函式庫（`libfontconfig` 和它帶進來的 freetype、harfbuzz、glib……，在 `main` 之前由 loader 載入）、`/proc/self/maps`、`/proc/sys/vm/overcommit_memory`、`/sys/kernel/mm/transparent_hugepage/enabled`、`/sys/devices/system/cpu/online`、`/dev/urandom`、`/etc/localtime`（和它指到的 `/usr/share/zoneinfo/...`）。
- **列系統字型時多 550 個路徑，全是 fontconfig 的**：`/etc/fonts` 和 `/usr/share/fontconfig` 的設定（274 個）、cache（`/usr/lib/fontconfig/cache`、`~/.cache/fontconfig`）、字型的目錄和檔案（`/usr/share/fonts`、`~/.local/share/fonts`）。
- **system call 55 種**（`--test-fonts` 52 種）：thread 和記憶體（`clone3`、`futex`、`mmap`、`mprotect`、`madvise`……）、控制通道（`sendmsg`、`recvmsg`、`epoll_*`）、`getrandom`、`prctl`（thread 的名字）。V8 的 JIT 用 memory protection key（`pkey_alloc`、`pkey_mprotect`，可執行的區域是 `PROT_READ|PROT_WRITE|PROT_EXEC` 加 pkey）。

這一頁沒走到的（之後補）：wasm 的文件（`dom_wasm`，wasmtime 的 JIT）、很多 worker、GC 的壓力、`MACHOPO_TRACE`、crash 時的處理。

### 共同的形狀

- **時機**：child 的 `main` 看到 `--type=renderer` 之後第一件事就是套用，在讀控制通道的第一則訊息之前（RFC 0004 的「啟動」）。Linux 上 loader 在 `main` 之前已經把共用函式庫載入，policy 不用開放它們。
- **Browser 要先交給 renderer 的東西**在啟動時給（繼承的 fd、命令列），不靠「鎖之前先拿好」。
- **套不上就不跑**（已決定的事項第 4 條）：達不到最低要求時 renderer 結束、分頁顯示當掉並說明原因；不安靜地退回沒有 sandbox。要退回由使用者明說（`--no-sandbox`）。Linux 的最低要求是 Landlock 的檔案規則加 seccomp；之上的幾層 kernel 有就套，沒有的由 seccomp 擋同一件事。
- **違規時**（已決定的事項第 5 條）：預設當掉並在 stderr 說是哪個 system call，一小份名單回錯誤。Chromium 預設是當掉（`SIGSYS` 的 handler 在 stderr 留一行），檔案系統的呼叫、`fork` 這幾種回錯誤，`clone3` 回 `ENOSYS` 讓 libc 退回看得到旗標的 `clone`。

### Linux 的機制

| | 做法 | 擋得住 | 代價 |
| --- | --- | --- | --- |
| L1 | 只有 seccomp-bpf：照探測的清單放行 system call | 新的 socket、`execve`、`ptrace`、大多數危險的呼叫 | 檔案只能整個 `openat` 擋或放：分不出路徑（BPF 讀不到字串），要開字型就等於開整個檔案系統的讀 |
| L2 | Landlock（檔案照路徑、TCP 和 UDP 的 port、訊號和 abstract socket 的範圍）加 seccomp-bpf | 檔案照目錄放行（唯讀）；其餘同 L1 | 檔案的規則要 kernel 5.13，之後每一版多一層（`details.md` 的「Linux 的機制的比較」；開發機是 ABI 11）；舊的要用 seccomp 補或算不上。`stat` 這一族 Landlock 不管：探得出路徑在不在，讀不到內容 |
| L3 | PID、network namespace、chroot 到空的目錄，加 seccomp-bpf：Chromium 的做法（`details.md` 的「Chromium 的 Linux sandbox」） | 檔案系統整個看不到、看不到其他 process、沒有網路介面 | namespace 是啟動時由別人建的（Chromium 是 zygote，或 setuid 的 helper），不是 child 自己鎖：machopo 沒有 zygote，renderer 是重新 exec 的；有的發行版關掉沒有特權的 user namespace（照記憶：Ubuntu 24.04 的 AppArmor 限制）；chroot 之後字型和時區要另外給（Chromium 用 Browser 那一側的 font service 和算時區的 process） |
| L4 | 外部的工具（bubblewrap）包著啟動 | 同 L3 | 多一個執行期的依賴，policy 不在我們手上 |

定了 **L2**（已決定的事項第 2 條）。Landlock 是自己對自己套的（`no_new_privs` 之後 `landlock_restrict_self`），不用特權、不用改啟動的方式，剛好是「child 的 `main` 一開始就鎖」（那時只有一個 thread，之後建的都繼承）；檔案照路徑唯讀放行，字型可以照舊由 renderer 讀（下一節的 F1）；seccomp 擋 Landlock 管不到的（建 socket、`execve`、`ptrace`、`io_uring`、`bpf`……）。Rust 有現成的 crate：`landlock`（Landlock 的專案自己的）和 `seccompiler`（rust-vmm 的），授權合、沒有 C 的相依（`details.md` 的「調查過的 crate」）。不照 Chromium 的 L3：它的 namespace 那一層綁著 zygote，和 `ipc::platform::spawn` 現在的形狀差得多；Chromium 自己的 `sandbox/linux/README.md` 寫「We currently don't use Landlock, but we'd like to」。L3 換到而 L2 沒有的：renderer 連路徑在不在都探不出來（Landlock 不管 `stat`），和整個看不到其他 process（L2 靠 Landlock 擋 `/proc` 的內容和訊號的範圍）。

### macOS 的機制

| | 做法 | 說明 |
| --- | --- | --- |
| M1 | Seatbelt 的 profile（`sandbox_init_with_parameters`，`deny default`），child 一開始自己套 | Chromium、Firefox 的做法；RFC 0004 的原型試過 IPC 的那一半（rendezvous 的名字、控制通道、共用記憶體）；API 是私有的但各家都用 |
| M2 | App Sandbox 的 entitlement（簽章時給） | 要簽章和 container，粒度粗，開發時的 build 不好用 |

2026-11-26 的看法：**M1**。未知數（要在 macOS 上探測）：CoreText 列字型要不要連系統的字型服務、V8 的 JIT（`MAP_JIT`）、elise 的 worker pool。

### Windows 的機制

| | 做法 | 說明 |
| --- | --- | --- |
| W1 | restricted token、低的 integrity level、job 的限制、process 的 mitigation（renderer 不用 GUI，可以關掉 win32k 的 system call） | Chromium 的 renderer 的做法；由啟動的一方（Browser）在 `CreateProcess` 時給 |
| W2 | AppContainer（或 LPAC） | 比較新的機制，照 capability 放行；Chromium 用在部分 process |

還沒有看法：要先在 Windows 上探測，並且和「自己呼叫 `CreateProcessW`、指定繼承的 handle」（現在的限制）一起設計。job 已經有了（`platform/windows/job.rs`，child 不比 host 活得久）。

### 字型

現在 renderer 自己列系統的字型（parley 的 fontique：Linux 經過 `libfontconfig`，macOS 經過 CoreText，Windows 經過 DirectWrite）。

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| F1 | sandbox 開放字型要讀的路徑（唯讀） | 不用改文字的程式；Linux 的探測顯示全是唯讀的讀檔 | 開放的範圍不小（Linux：fontconfig 的設定、cache、`/usr/share/fonts`、家目錄裡的 `~/.local/share/fonts`、`~/.config/fontconfig`、`~/.cache/fontconfig`）；renderer 讀得到使用者裝了哪些字型（本來網頁就量得出來）；macOS、Windows 的系統服務可能不只是讀檔 |
| F2 | Browser 列字型，把字型的資料（唯讀的共用記憶體）和 fallback 的答案經過通道給 renderer | sandbox 最緊：renderer 一個檔案都不用開 | 要一個同步的查詢（fallback：這個字、這個語言用哪個字型）；fontique 要能從別人給的清單建 collection 和 fallback；三個平台都要做 |
| F3 | 平台各自選：能只靠讀檔的用 F1，要系統服務的用 F2 | 工作量照實際需要 | 兩套做法 |

Linux 定了 **F1**（已決定的事項第 3 條）；macOS 和 Windows 等各自的探測（2026-11-26 的看法：傾向 F3，能只靠讀檔的用 F1）。對照 Chromium：macOS 的 renderer 直接讀系統字型（F1）；Linux 和 Windows 經過 Browser 的字型服務（F2，因為 L3 的 chroot 和 DirectWrite 的字型 cache 服務）。

### 其他讀的東西

`/etc/localtime` 和 zoneinfo（時區）、`/proc/self/maps`、`/proc/sys/vm/overcommit_memory`、`/sys/kernel/mm/transparent_hugepage/enabled`、`/sys/devices/system/cpu/online`、`/dev/urandom`：每一個要嘛照路徑唯讀放行，要嘛讓它失敗（讀的一方本來就有 fallback 的話），要嘛改成由 Browser 給。逐項定，第 1 段開始前用 `strace -k` 找出是誰讀的。

### 量測和除錯的工具

- renderer 自己寫的檔案：`MACHOPO_TRACE` 的 trace（renderer 的是 Browser 的檔名加 `.<pid>`）、`MACHOPO_CENSUS`（`census` feature 的 build）。定了（已決定的事項第 6 條）：Browser 開好檔案、啟動時交給 renderer（sandbox 不擋已經開著的），做好之前工具先加 `--no-sandbox`。
- 從外面看 renderer 的（`perf`、samply、debugger、`bench.py` 讀 `/proc/<pid>` 的記憶體）：sandbox 不擋別人看它；要確認 Landlock、seccomp 不影響 `perf` 的取樣。
- `about:crash`、renderer 被 kill 的路照舊。

### 程式放哪

新的 crate `sunaba`（已決定的事項第 7 條）：各平台的機制和 renderer 的 policy，自己的 `soundness.md`；`unsafe` 的規則和 `sanity.py unsafe` 的允許清單加上它。`shell`（child 的 `main`）和啟動 renderer 的那一側用它，renderer 這一側的 crate 不依賴它。

### 測試

- **整套現有的測試就是第一層**：`shell`、`browser` 的測試、`test.py wpt`、情境預設在 sandbox 裡跑 renderer process，renderer 需要而 policy 沒放行的東西會讓它們失敗。
- **每個平台一組「做不到」的測試**（已決定的事項第 8 條）：`sunaba` 裡一個只給測試用的小執行檔，`main` 第一件事進 sandbox（和 renderer 同一個函式、同一份 policy），再照命令列試一件事（讀家目錄的檔案、建檔案、開 socket、連線、啟動程式、對 parent 送訊號、開 `/proc/<parent>/mem`……），每一項都要失敗；policy 放寬時這些測試會壞。
- **真的 renderer 有套上**：`shell` 的測試從外面讀 renderer process 每個 thread 的 `/proc` 狀態（seccomp 已裝上、`NoNewPrivs`）。
- **套不上的路**：模擬機制不能用（舊 kernel 的回傳值），renderer 不跑、分頁說明原因。
- mutation test：policy 的每一條規則拿掉一條、放寬一條。
- 三個平台的 CI 都要跑到（`docs/testing.md` 的「CI」）；Linux 的 CI 的 kernel 版本和有沒有 Landlock 要先確認。

### 分階段

0. 草稿和 Linux 的探測（這一版）。
1. **Linux**：Landlock 加 seccomp（已決定的事項第 2 條）、字型、其他讀的東西、套不上時怎麼辦、工具、測試。開發機是 Linux，先做它。
2. **macOS**：先探測（真的 renderer 在 `deny default` 底下被擋了什麼），再定 profile 和字型。
3. **Windows**：先探測；`CreateProcessW` 和繼承的清單、token、integrity、job、mitigation、字型。

每個平台做完可以先 merge（`rfcs/README.md` 的「階段的 merge」）：沒做的平台照舊沒有 sandbox，`docs/limitations.md` 寫明哪些平台有。

## 缺點

- 三個平台三套機制，每一套都要長期跟著 kernel 和系統的版本維護；renderer 這一側升級相依的函式庫（V8、wasmtime、fontique、mimalloc）可能多用一個 system call，測試會壞在 sandbox 上。
- Linux 選 L2 的話，保證跟著 kernel 的版本變：舊的 kernel 上少幾層，要寫清楚最低要求。
- 字型選 F1 的話，renderer 讀得到字型的目錄（包括家目錄裡的字型和 fontconfig 的設定）。
- 開發時多一個「在 sandbox 裡不能做」的限制；debugger 要 `--no-sandbox`。

## 理由和其他做法

- **不做的話**：拆 process 換到的只有穩定性，安全上最大的一塊沒有（RFC 0004 的「缺點」）。
- **為什麼三個平台一起寫、分段做**：RFC 0004 把 sandbox 移出去的理由就是「只做一個平台的話三個平台拿到的東西不一樣，三套機制各自值得比」。
- 各節的選項比較在上面；完整的在 `details.md`。

## 先例

Linux 的部分 2026-11-26 對過原始碼（`details.md` 的「Chromium 的 Linux sandbox」），其餘照記憶寫的，定案前對：

- **Chromium**：renderer 在每個平台都在 sandbox 裡。Linux：zygote 建的 namespace 那一層加 seccomp-bpf 的 policy（違規預設當掉），檔案系統的呼叫一律擋，字型經過 Browser 那一側的 font service；macOS：Seatbelt 的 profile，child 一開始就套；Windows：restricted token、integrity level、job、mitigation，字型經過 DirectWrite 的 proxy。
- **Firefox**：content process 在 Linux 是 seccomp-bpf 加（有的話）namespace，檔案經過一個 broker 照 policy 開；macOS 是 Seatbelt；Windows 和 Chromium 用同一套 sandbox 的程式。
- **Landlock**：Chromium 和 Firefox 的 Linux sandbox 都比它早（kernel 5.13 才有）；Chromium 的 README 說還沒用、想用，有一份 GPU process 的 Landlock policy。其他用它的先例待查。

## 已決定的事項

1. **範圍：三個平台都在這一份，分段做，Linux 先**（2026-11-26，使用者決定；A）：共同的部分（什麼時候鎖、套不上怎麼辦、開關、測試的形狀）只定一次；每個平台是「分階段」的一段，做完照 `rfcs/README.md` 的「階段的 merge」先進 `main`，沒做的平台照舊沒有 sandbox、`docs/limitations.md` 寫明哪些平台有。Linux 先做（現在的開發機），再 macOS、Windows。不選這一份只做 Linux、另外兩個平台各開一份：共同的決定要在三份之間對齊，RFC 0004 把 sandbox 移出去的理由也是三個平台要一起比。
2. **Linux 的機制：Landlock 加 seccomp-bpf（L2），renderer 在 `main` 一開始自己套**（2026-11-26，使用者決定）：Landlock 管檔案（照路徑唯讀放行，其餘都擋）、TCP 和 UDP、訊號和 abstract socket 的範圍；seccomp 只放行清單上的 system call，擋 Landlock 管不到的（建 socket、`execve`、`ptrace`、`io_uring`、`bpf`……）。用 `landlock` 和 `seccompiler` 兩個 crate（`details.md` 的「調查過的 crate」）。理由：不用特權、不用改啟動的方式，剛好是 RFC 0004 定的「child 的 `main` 一開始就鎖」；探測顯示 renderer 要的全是唯讀的讀檔；擋得住所有能造成損害的事（寫檔案、連網路、啟動程式、碰其他 process）。做完還留著的（寫進 `docs/limitations.md`）：renderer 探得出路徑在不在和它的大小、時間（Landlock 不管 `stat` 這一族），讀得到放行的字型目錄，看得到有哪些 pid。對照 Chromium：它是 zygote 建的 namespace 那一層加 seccomp-bpf，自己的 README 寫「We currently don't use Landlock, but we'd like to」。不選只有 seccomp（L1：分不出路徑，要讀字型就等於開整個檔案系統）。不選這份就加 PID 和 network namespace（討論過：它不多擋一種事，是在「其他 process」和「網路」上多一層不靠清單的保護；要改啟動 child 的那一段、多 fork 一次、丟掉 user namespace 裡的特權，有的發行版不許沒有特權的 user namespace；放進「之後的可能」，和完整的 L3 同一個方向）。不選完整的 L3（chroot 到空的目錄：連路徑都探不出來，但字型和時區要改由 Browser 給，沒有 user namespace 的機器上只剩 seccomp；Chromium 在那種機器上的退路是 setuid root 的 helper，不要）。不選包一層 bubblewrap（L4：多一個執行期的依賴）。
3. **Linux 的字型：sandbox 開放字型要讀的路徑，唯讀（F1）；清單由 Browser 問 fontconfig**（2026-11-26，使用者決定 F1 和下面的順序；清單的來源照建議）：renderer 照舊自己經過 `libfontconfig` 列字型，Landlock 唯讀放行它要讀的路徑。放行哪些由 Browser 在啟動 renderer 之前問 fontconfig（實際的設定檔、字型目錄、cache 目錄），啟動時交給 renderer（鎖之前就要知道：命令列或繼承的 fd，第 1 段定）：使用者在 fontconfig 的設定裡另外加的目錄也對。不選固定的清單（`/usr/share/fonts`、`~/.local/share/fonts` 這些）：設定裡自訂的目錄（NixOS、flatpak 這類把字型放在別處的系統）會安靜地漏掉。做完留著的：renderer 讀得到這些目錄的內容，包括家目錄裡的字型和 fontconfig 的設定、cache。對照 Chromium：Linux 上是 Browser 那一側的 font service（F2），因為它的 renderer 在 chroot 裡。**不選現在做 F2，也不先做字型服務再做 sandbox**（討論過）：字型服務自己不帶來安全（renderer 沒被關起來之前誰讀字型都一樣）；它是比較大、比較有風險的那一個（改 parley 的 fork、排版要等 Browser 回字型檔、三個平台三種後端）；它要證明的「renderer 一個檔案都不開、文字照樣一樣」要有 sandbox 才測得出來；它的設計要 macOS、Windows 探測的結果（那兩個平台的字型服務在 sandbox 裡連不連得到）。F1 之後要拿掉的只有那幾條放行的規則和問 fontconfig 的那一段。之後的順序記在「之後的可能」。
4. **套不上時不跑；最低要求是 Landlock 的檔案規則加 seccomp，之上的有就套**（2026-11-26，使用者決定；A）：renderer 在 `main` 一開始問 kernel 的 Landlock ABI。達不到最低要求（沒有 Landlock：kernel 比 5.13 舊、沒編進去、開機時關掉；或 seccomp 的 filter 裝不上）時 renderer 不跑：結束並說原因，分頁顯示當掉和原因，提示 `--no-sandbox`；不安靜地退回沒有 sandbox。最低要求之上、後來的 ABI 才有的幾層（TCP 和 UDP 的規則、訊號和 abstract socket 的範圍、有路徑的 unix socket）kernel 有就套（kernel 的文件建議的 best-effort，`landlock` crate 有這種相容的程度），沒有的由 seccomp 擋同一件事（不許建 socket；`kill` 一族的對象只能是自己，Chromium 的 `RestrictKillTarget`）：舊一點的 kernel 上保證不變弱，只是少了重複的那一層。對照 Chromium：sandbox 起不來時拒絕啟動，要使用者明說 `--no-sandbox`。不選能套多少算多少、套不上照常跑只印警告（沒有 sandbox 時使用者不會注意到）。不選一定要全部的功能（等於要求 6.12 以上的 kernel）。要確認的（第 1 段）：還在支援期的發行版的 kernel 是不是都有 5.13 和編進去的 Landlock；macOS、Windows 的「最低要求」在各自的那一段定，原則一樣（套不上不跑）。
5. **清單以外的 system call：預設當掉並說是哪一個，一小份名單回錯誤**（2026-11-26，使用者決定；A）：seccomp 的預設動作是送 `SIGSYS`（`SECCOMP_RET_TRAP`），renderer 一開始裝的 handler 在 stderr 寫一行（哪個 system call），再讓 process 當掉；Browser 照 renderer 當掉處理（分頁顯示當掉）。另外一份名單回錯誤、不當掉：函式庫會自己試、失敗了有退路的（第 1 段寫 policy 時定；預計有 `clone3` 回 `ENOSYS`，讓 libc 退回看得到旗標的 `clone`，照 Chromium；探測裡本來就失敗的那幾種）。Landlock 擋的檔案存取一律是回錯誤（kernel 的行為）。理由：清單要跟著相依的函式庫（V8、wasmtime、mimalloc、fontconfig）升級維護，漏了什麼要讓測試馬上失敗、而且看得出是哪一個；被攻破的 renderer 可以換掉 handler 讓自己不當掉，但那個 system call 照樣不執行，最差等於回錯誤。對照 Chromium：一樣（baseline policy 的預設是 `CrashSIGSYS()`，檔案系統的呼叫和 `fork` 回 `EPERM`，`clone3` 回 `ENOSYS`）。不選 kernel 直接殺掉 process（`SECCOMP_RET_KILL_PROCESS`：攻擊者一試就死，但開發時不知道是哪個 system call，要另外用 `strace` 重跑）。不選一律回錯誤（壞得安靜：功能走了比較慢或不對的路，測試不一定看得出來）。handler 要 `sigaction` 的 FFI：放在哪個 crate 跟著「程式放哪」定。
6. **renderer 自己寫的量測檔案：Browser 先開好、啟動時交給 renderer（T2）**（2026-11-26，使用者決定）：`MACHOPO_TRACE` 的 trace 和 `MACHOPO_CENSUS`，renderer 是自己的 process 時由 Browser 在啟動它之前開好輸出的檔案，開好的檔案在啟動時交給 renderer（繼承；sandbox 不擋已經開著的），renderer 往裡面寫，不自己開；檔名由 Browser 定（現在 renderer 的 trace 是 Browser 的檔名加 `.<pid>`，工具照 pid 找：怎麼對上在第 1 段定）。理由：量到的要是有 sandbox 的 renderer（sandbox 自己造成的成本要量得到）；renderer 鎖之前不用做任何事，要的東西由 Browser 在啟動時給。「啟動時多交一個東西給 child」三個平台都要做，Windows 的和自己呼叫 `CreateProcessW`、指定繼承的清單一起設計。做好之前工具先加 `--no-sandbox`（T1），做好就拿掉。對照 Chromium（`details.md` 的「Chromium 的 tracing」）：producer 把資料寫進和 tracing service 共用的記憶體，service 擁有 buffer、寫出檔案，IPC 只做交握和通知；那是給常駐的 tracing 的，machopo 的 trace 是開發時才開的量測。不選工具一律 `--no-sandbox`（量到的不是有 sandbox 的 renderer）。不選照 Chromium 寫進共用記憶體（要一套分塊和提交的協定，等於重寫 trace 的後半段；記進「之後的可能」）。不選 renderer 在 `main` 裡先開好檔案再鎖（打破「鎖之前不跑任何程式」，Windows 不一定做得到）。
7. **程式放在新的 crate `sunaba`，`unsafe` 的規則多它一個**（2026-11-26，使用者決定；P2；名字是使用者取的，日文的「砂場」）：各平台的機制和 renderer 的 policy 在新的 crate `sunaba`（Chromium 的 `//sandbox` 和 `//sandbox/policy`）；它可以有 `unsafe`（Linux 的 `SIGSYS` handler 的 `sigaction`、macOS 的 Seatbelt 的 FFI、Windows 的 token、job、`CreateProcessW`），每一處有 `// SAFETY:`，論證在它自己的 `soundness.md`（要保證的是「被攻破的 renderer 做不到什麼」，和 `ipc` 的「通道對端送來的東西怎麼驗」是兩份論證）。專案的規則跟著改：`CLAUDE.md` 的「`unsafe` 只在 `bindings` 和 `ipc`」和 `sanity.py unsafe` 的允許清單加上 `sunaba`，在第 1 段實作時一起改。依賴的方向：`shell`（child 的 `main`）和啟動 renderer 的那一側用它；renderer 這一側的 crate 不依賴它。不選放進 `ipc::platform`（規則不用改，但 `ipc` 變成兩件事、一份 soundness 文件講兩種論證）。不選 safe 的在新 crate、`unsafe` 的幾塊留在 `ipc`（同一件事拆兩處，Windows 幾乎全是 FFI）。Windows 上 `sunaba` 和 `ipc::platform::spawn` 怎麼接（誰呼叫 `CreateProcessW`）在 Windows 那一段定。
8. **「做不到」的測試用 `sunaba` 裡一個輔助的執行檔（S2），加上從外面看真的 renderer**（2026-11-26，使用者決定）：測試三層。(1) policy 的內容：`sunaba` 有一個只給測試用的小執行檔，`main` 第一件事呼叫和 machopo 的 renderer 同一個「進 sandbox」的函式（同一份 policy、單一 thread、`main` 的最前面），再照命令列試一件被禁止的事（讀家目錄的檔案、建檔案、開 socket、啟動程式、對 parent 送訊號……）、說出結果；`sunaba` 的整合測試每項跑它一次，每一項都要失敗，policy 放寬一條對應的測試就壞。(2) 真的 renderer 有套上：`shell` 的測試啟動 machopo，從外面讀 renderer process 每個 thread 的 `/proc/<pid>/task/*/status`，每一個都要是 seccomp 已裝上、`NoNewPrivs` 是 1（證明在建任何 thread 之前就鎖了）。(3) policy 沒擋到 renderer 要的：現有的 `shell`、`browser` 的測試、WPT、情境預設在 sandbox 裡跑 renderer。不選測試的執行檔把自己再啟動一次當 child（`ipc` 的跨 process 測試的做法：套 policy 時已經在測試框架裡、有好幾個 thread，和真的 renderer 啟動的方式不同）。不選在 renderer 裡留測試用的命令（正式的執行檔多一個「去開這個檔案」的入口）。對照 Chromium：照記憶有一組獨立的 sandbox 測試（`sandbox_linux_unittests`），第 1 段對原始碼。
9. **開關只有命令列的 `--no-sandbox`，沒有環境變數**（2026-11-26，使用者決定；N1）：和 Chromium 同名。Browser 把它傳到 child 的命令列上（child 要在讀第一則訊息之前知道要不要鎖；命令列是 Browser 給的）；啟動時在 stderr 印一行警告。經過別的程式啟動 machopo 的工具各自傳下去（machopodriver 的 `--binary-arg`、`bench.py` 組的命令列）。`--single-process`（renderer 是 Browser 的 thread）關不起來：它的說明寫明、啟動時印同一種警告。測試預設在 sandbox 裡跑，個別要關的自己傳。不選再加一個環境變數（設在 shell 的設定檔裡忘了，之後每次都沒有 sandbox、不容易發現）。對照 Chromium：照記憶只有 `--no-sandbox`、關掉時有警告，第 1 段對原始碼。

## 待決定

第 1 段（Linux）之前要定的九條都定了（已決定的事項第 1 到 9 條）。每一段開始前只定那一段的：

- **第 1 段開始時**（細節，照已決定的方向）：seccomp 放行的清單和回錯誤的名單（探測的 55 種，加上 wasm 的文件、trace、crash 的路再探一次）；`/proc/self/maps`、`/proc/sys/vm/overcommit_memory`、`/sys` 的那幾個、`/dev/urandom`、時區，每一個放行還是讓它失敗（先用 `strace -k` 找出是誰讀的）；fontconfig 的 cache 過期、寫不了時它怎麼辦；Browser 把字型的路徑和開好的 trace 檔交給 renderer 的方式（命令列、繼承的 fd）；renderer 的 trace 檔名和工具怎麼對上；還在支援期的發行版和 CI 的 kernel 有沒有 Landlock；輔助的執行檔怎麼放（`sunaba` 的 bin）。
- **第 2 段（macOS）**：探測；Seatbelt 的 profile；字型（F1 還是要系統的字型服務）；最低要求。
- **第 3 段（Windows）**：探測；`CreateProcessW` 和繼承的清單、`sunaba` 和 `ipc::platform::spawn` 怎麼接；token、integrity、job、mitigation；字型；最低要求。

## 之後的可能

**sandbox 收緊的順序**（2026-11-26 和使用者討論過；這份做完之後照這個順序，每一步都是在前一步上加，不用重做）：

1. **字型服務**（另一份 RFC；F2）：Browser 列字型，把字型的清單、fallback 的表和字型檔的內容經過通道給 renderer，renderer 不開任何字型的檔案。等這份的 macOS、Windows 的探測做完再設計（那兩個平台可能本來就需要它：照記憶，Chromium 在 Windows 上經過 DirectWrite 的 proxy）。做完把 Linux 放行的字型路徑（已決定的事項第 3 條）拿掉；時區和其他零星的讀取（`/etc/localtime`、`/proc/self/maps`……）一起改成由 Browser 給或不需要。那時 Landlock 一個路徑都不放行，renderer 讀不到任何檔案的內容。
2. **Linux 加 PID 和 network namespace**（機器允許沒有特權的 user namespace 時；不允許時只有 Landlock 和 seccomp，不做 setuid 的 helper）：外面的 process 在 renderer 裡沒有編號、沒有網路介面，「其他 process」和「網路」多一層不靠 seccomp 的清單寫對的保護。要改啟動 child 的那一段、多 fork 一次、丟掉 user namespace 裡的特權。
3. **Linux chroot 到空的目錄**（完整的 L3，要第 1、2 步）：連路徑在不在都探不出來、`/proc` 整個看不到。

其他：

- trace 照 Chromium（Perfetto）寫進和 Browser 共用的記憶體、由 Browser 寫檔：trace 要變成常駐、隨時可以開的功能時。
- V8 的 sandbox：開 `v8_enable_sandbox`、`bindings` 的論證改成不信任 V8 heap 裡讀到的東西。
- GPU process：顯示卡的驅動程式（別人的 C）搬出 Browser。
- site isolation：一個 renderer 只有一部分 frame 是 local 的。
