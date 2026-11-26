# RFC 0117：renderer process 關進作業系統的 sandbox

- 開始日期：2026-11-26
- 狀態：🚧 已排程：2026-11-26（第 1 段 Linux、第 2 段 macOS 做完；Windows 的在它的那一段定）
- 細節：`details.md`

## 摘要

renderer 已經是自己的 process（RFC 0004），但沒有被關起來：被攻破的 renderer 照樣用使用者的身分開檔案、連網路。這份把 renderer process 關進作業系統的 sandbox：Linux、macOS、Windows 各用自己的機制，在 child 的 `main` 一開始、任何其他程式碼之前就鎖（RFC 0004 的「為 sandbox 留的條件」假設的時機），套不上就不跑。分段做，Linux 先：Landlock（檔案照路徑唯讀放行）加 seccomp-bpf（只放行清單上的 system call，其餘當掉並說是哪一個）；字型照舊由 renderer 讀，放行的路徑由 Browser 問 fontconfig；程式在新的 crate `sunaba`；開關是 `--no-sandbox`。macOS 是 Seatbelt 的 profile（`deny default`，只放行 renderer 要的那幾樣），renderer 的 `main` 第一件事就套，字型照舊由 renderer 讀、加上系統的字型服務（已決定的事項第 17 到 20 條）。Windows 的機制在它的那一段、探測之後定。兩個平台的探測（真的 renderer 向系統要了什麼）在 `details.md`。

## 動機

現況（2026-11-26 的 `main`，`52dc5adc`）：

- `docs/limitations.md` 的「隔離」（這份開始時的第一條：renderer 是獨立的 process，但沒有被關起來）：V8 或 DOM 的 heap（elise）、wasmtime 的 JIT、`bindings` 的 `unsafe` 裡任何一個記憶體錯誤被利用之後，攻擊者拿到的是使用者的身分：讀寫家目錄、連網路、啟動程式、碰同一個使用者的其他 process。
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

下面每一節是選項和比較；定了的寫明是哪一條決定，其餘是 2026-11-26 的看法。Chromium 的 Linux 和 macOS 那兩部分對過原始碼了（`details.md` 的「Chromium 的 Linux sandbox」「Chromium 的 macOS sandbox」）；Windows 的還是照記憶寫的，那一段定案之前對（`details.md` 的「要對原始碼的地方」）。

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

定了 **M1**（已決定的事項第 17 條）。探測（`details.md` 的「macOS 的探測」）回答了原本的未知數：CoreText 列字型和讀字型檔不用系統的字型服務，但找 fallback 的字型要（`com.apple.fonts`，少了它 CJK 選到別的字型）；V8 和 wasmtime 的 JIT、elise 的 worker pool 在 `deny default` 底下不用放行任何東西。

和 Linux 不同的地方：

- **鎖是 `main` 的第一件事，前面沒有準備**：Seatbelt 是整個 process 的（不管有幾個 thread），字型在裡面找得到，所以不用像 Linux 先初始化字型再鎖；trace 和 census 的檔案照舊在鎖之前開（第 16 條）。
- **被擋的呼叫回錯誤，不結束 process**（`EPERM`；Mach 服務的查詢是 launchd 回「沒有權限」），kernel 在系統的 log 記一行（`log stream --predicate 'sender == "Sandbox"'` 看得到是哪個 process、哪種操作、對象是什麼）。第 5 條的「當掉並說是哪一個」是 seccomp 的；系統的函式庫在 sandbox 裡本來就會試很多東西（`details.md` 列了），失敗了各有退路，Chromium 的 profile 也是回錯誤。
- **照種類放行，不照 system call**：profile 說的是「哪些路徑能讀」「哪些 Mach 服務能查」「哪些 sysctl 能讀」。Seatbelt 也能照 system call 的號碼擋，這份沒有用（Chromium 的 renderer 也沒有；「之後的可能」）。
- **路徑的 metadata 也擋**（Linux 的 Landlock 不管 `stat`）：只放行用到的（第 19 條）。但路徑在不在還是探得出來：不存在的回 `ENOENT`，被擋的回 `EPERM`。

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

Linux 定了 **F1**（已決定的事項第 3 條）；macOS 也是 F1，另外放行系統的字型服務（第 18 條）；Windows 等它的探測。對照 Chromium：macOS 的 renderer 直接讀系統字型、連 `com.apple.fonts`（F1，`common.sb` 的 `allow-font-access`）；Linux 和 Windows 經過 Browser 的字型服務（F2，因為 L3 的 chroot 和 DirectWrite 的字型 cache 服務）。

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
1. **Linux**（做完）：Landlock 加 seccomp（已決定的事項第 2 條）、字型、其他讀的東西、套不上時怎麼辦、工具、測試。
2. **macOS**（做完）：先探測（真的 renderer 在 `deny default` 底下被擋了什麼），再定 profile 和字型（第 17 到 20 條）。
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

Chromium 的 Linux 和 macOS 的部分 2026-11-26 對過原始碼（`details.md` 的「Chromium 的 Linux sandbox」「Chromium 的 macOS sandbox」），其餘照記憶寫的，定案前對：

- **Chromium**：renderer 在每個平台都在 sandbox 裡。Linux：zygote 建的 namespace 那一層加 seccomp-bpf 的 policy（違規預設當掉），檔案系統的呼叫一律擋，字型經過 Browser 那一側的 font service；macOS：Seatbelt 的 profile，child 一開始就套；Windows：restricted token、integrity level、job、mitigation，字型經過 DirectWrite 的 proxy。
- **Firefox**：content process 在 Linux 是 seccomp-bpf 加（有的話）namespace，檔案經過一個 broker 照 policy 開；macOS 是 Seatbelt；Windows 和 Chromium 用同一套 sandbox 的程式。
- **Landlock**：Chromium 和 Firefox 的 Linux sandbox 都比它早（kernel 5.13 才有）；Chromium 的 README 說還沒用、想用，有一份 GPU process 的 Landlock policy。其他用它的先例待查。

## 已決定的事項

1. **範圍：三個平台都在這一份，分段做，Linux 先**（2026-11-26，使用者決定；A）：共同的部分（什麼時候鎖、套不上怎麼辦、開關、測試的形狀）只定一次；每個平台是「分階段」的一段，做完照 `rfcs/README.md` 的「階段的 merge」先進 `main`，沒做的平台照舊沒有 sandbox、`docs/limitations.md` 寫明哪些平台有。Linux 先做（現在的開發機），再 macOS、Windows。不選這一份只做 Linux、另外兩個平台各開一份：共同的決定要在三份之間對齊，RFC 0004 把 sandbox 移出去的理由也是三個平台要一起比。
2. **Linux 的機制：Landlock 加 seccomp-bpf（L2），renderer 在 `main` 一開始自己套**（2026-11-26，使用者決定）：Landlock 管檔案（照路徑唯讀放行，其餘都擋）、TCP 和 UDP、訊號和 abstract socket 的範圍；seccomp 只放行清單上的 system call，擋 Landlock 管不到的（建 socket、`execve`、`ptrace`、`io_uring`、`bpf`……）。用 `landlock` 和 `seccompiler` 兩個 crate（`details.md` 的「調查過的 crate」）。理由：不用特權、不用改啟動的方式，剛好是 RFC 0004 定的「child 的 `main` 一開始就鎖」；探測顯示 renderer 要的全是唯讀的讀檔；擋得住所有能造成損害的事（寫檔案、連網路、啟動程式、碰其他 process）。做完還留著的（寫進 `docs/limitations.md`）：renderer 探得出路徑在不在和它的大小、時間（Landlock 不管 `stat` 這一族），讀得到放行的字型目錄，看得到有哪些 pid。對照 Chromium：它是 zygote 建的 namespace 那一層加 seccomp-bpf，自己的 README 寫「We currently don't use Landlock, but we'd like to」。不選只有 seccomp（L1：分不出路徑，要讀字型就等於開整個檔案系統）。不選這份就加 PID 和 network namespace（討論過：它不多擋一種事，是在「其他 process」和「網路」上多一層不靠清單的保護；要改啟動 child 的那一段、多 fork 一次、丟掉 user namespace 裡的特權，有的發行版不許沒有特權的 user namespace；放進「之後的可能」，和完整的 L3 同一個方向）。不選完整的 L3（chroot 到空的目錄：連路徑都探不出來，但字型和時區要改由 Browser 給，沒有 user namespace 的機器上只剩 seccomp；Chromium 在那種機器上的退路是 setuid root 的 helper，不要）。不選包一層 bubblewrap（L4：多一個執行期的依賴）。
3. **Linux 的字型（清單怎麼來第 14 條改了：renderer 自己在鎖之前載入）：sandbox 開放字型要讀的路徑，唯讀（F1）；清單由 Browser 問 fontconfig**（2026-11-26，使用者決定 F1 和下面的順序；清單的來源照建議）：renderer 照舊自己經過 `libfontconfig` 列字型，Landlock 唯讀放行它要讀的路徑。放行哪些由 Browser 在啟動 renderer 之前問 fontconfig（實際的設定檔、字型目錄、cache 目錄），啟動時交給 renderer（鎖之前就要知道：命令列或繼承的 fd，第 1 段定）：使用者在 fontconfig 的設定裡另外加的目錄也對。不選固定的清單（`/usr/share/fonts`、`~/.local/share/fonts` 這些）：設定裡自訂的目錄（NixOS、flatpak 這類把字型放在別處的系統）會安靜地漏掉。做完留著的：renderer 讀得到這些目錄的內容，包括家目錄裡的字型和 fontconfig 的設定、cache。對照 Chromium：Linux 上是 Browser 那一側的 font service（F2），因為它的 renderer 在 chroot 裡。**不選現在做 F2，也不先做字型服務再做 sandbox**（討論過）：字型服務自己不帶來安全（renderer 沒被關起來之前誰讀字型都一樣）；它是比較大、比較有風險的那一個（改 parley 的 fork、排版要等 Browser 回字型檔、三個平台三種後端）；它要證明的「renderer 一個檔案都不開、文字照樣一樣」要有 sandbox 才測得出來；它的設計要 macOS、Windows 探測的結果（那兩個平台的字型服務在 sandbox 裡連不連得到）。F1 之後要拿掉的只有那幾條放行的規則和問 fontconfig 的那一段。之後的順序記在「之後的可能」。
4. **套不上時不跑；最低要求是 Landlock 的檔案規則加 seccomp，之上的有就套**（2026-11-26，使用者決定；A）：renderer 在 `main` 一開始問 kernel 的 Landlock ABI。達不到最低要求（沒有 Landlock：kernel 比 5.13 舊、沒編進去、開機時關掉；或 seccomp 的 filter 裝不上）時 renderer 不跑：結束並說原因，分頁顯示當掉和原因，提示 `--no-sandbox`；不安靜地退回沒有 sandbox。最低要求之上、後來的 ABI 才有的幾層（TCP 和 UDP 的規則、訊號和 abstract socket 的範圍、有路徑的 unix socket）kernel 有就套（kernel 的文件建議的 best-effort，`landlock` crate 有這種相容的程度），沒有的由 seccomp 擋同一件事（不許建 socket；`kill` 一族的對象只能是自己，Chromium 的 `RestrictKillTarget`）：舊一點的 kernel 上保證不變弱，只是少了重複的那一層。對照 Chromium：sandbox 起不來時拒絕啟動，要使用者明說 `--no-sandbox`。不選能套多少算多少、套不上照常跑只印警告（沒有 sandbox 時使用者不會注意到）。不選一定要全部的功能（等於要求 6.12 以上的 kernel）。要確認的（第 1 段）：還在支援期的發行版的 kernel 是不是都有 5.13 和編進去的 Landlock；macOS、Windows 的「最低要求」在各自的那一段定，原則一樣（套不上不跑）。
5. **清單以外的 system call：預設當掉並說是哪一個，一小份名單回錯誤**（2026-11-26，使用者決定；A）：seccomp 的預設動作是送 `SIGSYS`（`SECCOMP_RET_TRAP`），renderer 一開始裝的 handler 在 stderr 寫一行（哪個 system call），再讓 process 當掉；Browser 照 renderer 當掉處理（分頁顯示當掉）。另外一份名單回錯誤、不當掉：函式庫會自己試、失敗了有退路的（第 1 段寫 policy 時定；預計有 `clone3` 回 `ENOSYS`，讓 libc 退回看得到旗標的 `clone`，照 Chromium；探測裡本來就失敗的那幾種）。Landlock 擋的檔案存取一律是回錯誤（kernel 的行為）。理由：清單要跟著相依的函式庫（V8、wasmtime、mimalloc、fontconfig）升級維護，漏了什麼要讓測試馬上失敗、而且看得出是哪一個；被攻破的 renderer 可以換掉 handler 讓自己不當掉，但那個 system call 照樣不執行，最差等於回錯誤。對照 Chromium：一樣（baseline policy 的預設是 `CrashSIGSYS()`，檔案系統的呼叫和 `fork` 回 `EPERM`，`clone3` 回 `ENOSYS`）。不選 kernel 直接殺掉 process（`SECCOMP_RET_KILL_PROCESS`：攻擊者一試就死，但開發時不知道是哪個 system call，要另外用 `strace` 重跑）。不選一律回錯誤（壞得安靜：功能走了比較慢或不對的路，測試不一定看得出來）。handler 要 `sigaction` 的 FFI：放在哪個 crate 跟著「程式放哪」定。
6. **renderer 自己寫的量測檔案：Browser 先開好、啟動時交給 renderer（T2）**（誰開檔第 16 條改了：renderer 自己在鎖之前開）（2026-11-26，使用者決定）：`MACHOPO_TRACE` 的 trace 和 `MACHOPO_CENSUS`，renderer 是自己的 process 時由 Browser 在啟動它之前開好輸出的檔案，開好的檔案在啟動時交給 renderer（繼承；sandbox 不擋已經開著的），renderer 往裡面寫，不自己開；檔名由 Browser 定（現在 renderer 的 trace 是 Browser 的檔名加 `.<pid>`，工具照 pid 找：怎麼對上在第 1 段定）。理由：量到的要是有 sandbox 的 renderer（sandbox 自己造成的成本要量得到）；renderer 鎖之前不用做任何事，要的東西由 Browser 在啟動時給。「啟動時多交一個東西給 child」三個平台都要做，Windows 的和自己呼叫 `CreateProcessW`、指定繼承的清單一起設計。做好之前工具先加 `--no-sandbox`（T1），做好就拿掉。對照 Chromium（`details.md` 的「Chromium 的 tracing」）：producer 把資料寫進和 tracing service 共用的記憶體，service 擁有 buffer、寫出檔案，IPC 只做交握和通知；那是給常駐的 tracing 的，machopo 的 trace 是開發時才開的量測。不選工具一律 `--no-sandbox`（量到的不是有 sandbox 的 renderer）。不選照 Chromium 寫進共用記憶體（要一套分塊和提交的協定，等於重寫 trace 的後半段；記進「之後的可能」）。不選 renderer 在 `main` 裡先開好檔案再鎖（打破「鎖之前不跑任何程式」，Windows 不一定做得到）。
7. **程式放在新的 crate `sunaba`，`unsafe` 的規則多它一個**（「renderer 的 policy 也在 `sunaba`」第 12 條改了：policy 在 `shell`）（2026-11-26，使用者決定；P2；名字是使用者取的，日文的「砂場」）：各平台的機制和 renderer 的 policy 在新的 crate `sunaba`（Chromium 的 `//sandbox` 和 `//sandbox/policy`）；它可以有 `unsafe`（Linux 的 `SIGSYS` handler 的 `sigaction`、macOS 的 Seatbelt 的 FFI、Windows 的 token、job、`CreateProcessW`），每一處有 `// SAFETY:`，論證在它自己的 `soundness.md`（要保證的是「被攻破的 renderer 做不到什麼」，和 `ipc` 的「通道對端送來的東西怎麼驗」是兩份論證）。專案的規則跟著改：`CLAUDE.md` 的`unsafe` 只在哪幾個 crate 的那一條和 `sanity.py unsafe` 的允許清單加上 `sunaba`，在第 1 段實作時一起改。依賴的方向：`shell`（child 的 `main`）和啟動 renderer 的那一側用它；renderer 這一側的 crate 不依賴它。不選放進 `ipc::platform`（規則不用改，但 `ipc` 變成兩件事、一份 soundness 文件講兩種論證）。不選 safe 的在新 crate、`unsafe` 的幾塊留在 `ipc`（同一件事拆兩處，Windows 幾乎全是 FFI）。Windows 上 `sunaba` 和 `ipc::platform::spawn` 怎麼接（誰呼叫 `CreateProcessW`）在 Windows 那一段定。
8. **「做不到」的測試用 `sunaba` 裡一個輔助的執行檔（S2），加上從外面看真的 renderer**（2026-11-26，使用者決定）：測試三層。(1) policy 的內容：`sunaba` 有一個只給測試用的小執行檔，`main` 第一件事呼叫和 machopo 的 renderer 同一個「進 sandbox」的函式（同一份 policy、單一 thread、`main` 的最前面），再照命令列試一件被禁止的事（讀家目錄的檔案、建檔案、開 socket、啟動程式、對 parent 送訊號……）、說出結果；`sunaba` 的整合測試每項跑它一次，每一項都要失敗，policy 放寬一條對應的測試就壞。(2) 真的 renderer 有套上：`shell` 的測試啟動 machopo，從外面讀 renderer process 每個 thread 的 `/proc/<pid>/task/*/status`，每一個都要是 seccomp 已裝上、`NoNewPrivs` 是 1（證明在建任何 thread 之前就鎖了）。(3) policy 沒擋到 renderer 要的：現有的 `shell`、`browser` 的測試、WPT、情境預設在 sandbox 裡跑 renderer。不選測試的執行檔把自己再啟動一次當 child（`ipc` 的跨 process 測試的做法：套 policy 時已經在測試框架裡、有好幾個 thread，和真的 renderer 啟動的方式不同）。不選在 renderer 裡留測試用的命令（正式的執行檔多一個「去開這個檔案」的入口）。對照 Chromium：照記憶有一組獨立的 sandbox 測試（`sandbox_linux_unittests`），第 1 段對原始碼。
9. **開關只有命令列的 `--no-sandbox`，沒有環境變數**（2026-11-26，使用者決定；N1）：和 Chromium 同名。Browser 把它傳到 child 的命令列上（child 要在讀第一則訊息之前知道要不要鎖；命令列是 Browser 給的）；啟動時在 stderr 印一行警告。經過別的程式啟動 machopo 的工具各自傳下去（machopodriver 的 `--binary-arg`、`bench.py` 組的命令列）。`--single-process`（renderer 是 Browser 的 thread）關不起來：它的說明寫明、啟動時印同一種警告。測試預設在 sandbox 裡跑，個別要關的自己傳。不選再加一個環境變數（設在 shell 的設定檔裡忘了，之後每次都沒有 sandbox、不容易發現）。對照 Chromium：照記憶只有 `--no-sandbox`、關掉時有警告，第 1 段對原始碼。
10. **Linux：鎖之後還會讀的檔案，會改變網頁看到的行為的才放行**（2026-11-26，使用者同意；探測在 `details.md` 的「零星的讀取是誰讀的」和「wasm 的文件、trace、crash」）：唯讀放行 `/sys/devices/system/cpu/online`（glibc 的 `get_nprocs`：V8、absl、malloc 問 CPU 數，讀不到時 V8 的 worker 數會變）、`/dev/urandom`（V8 的亂數，讀不到用弱的種子）、`/etc/localtime` 和 `/usr/share/zoneinfo`（ICU 的時區，讀不到變成 UTC）；每一個只放行那個檔案或目錄。不放行、讓它失敗（讀的一方有退路）：`/proc/sys/vm/overcommit_memory`（glibc 的 malloc 在 thread 結束時）、`/proc/self/cgroup` 和 `/sys/fs/cgroup`（問可以用幾個 CPU）。`/proc/self/stat`、`/proc/self/cmdline`（只有 trace 的 metadata）先不放行，做 trace 的那一步（1d）看少了什麼再定。`main` 之前讀的（mimalloc、Rust 的 runtime、loader 載入的函式庫）不用放行。每一個「讓它失敗」的在 1b 用真的 sandbox 確認程式照樣對。
11. **sandbox 鎖之前要知道的東西放在 child 的命令列上**（「要放行的路徑」第 14 條改了：不經過命令列）（2026-11-26，使用者決定；A）：要放行的路徑（Browser 問 fontconfig 的，第 3 條）一個一個在命令列上；Browser 開好的檔案（trace、census，第 6 條）用繼承的，命令列說它的編號（控制通道的那一端現在也是繼承、用環境變數說編號）；`--no-sandbox` 也在命令列上（第 9 條）。renderer 的 `main` 讀命令列、鎖，之後才建控制通道和它的 thread。`docs/architecture.md` 的「Process 的邊界」第 5 條跟著改：命令列除了 `--type=renderer`，只有 sandbox 鎖之前要知道的這幾樣，其餘照舊在第一則訊息（`renderer::ChildInit`）。命令列是 Browser 給的，和第一則訊息一樣可信。不選放在第一則訊息裡、讀完再鎖（鎖之前要跑通道的程式；讀通道要先有 IO thread，Landlock 對已經在跑的其他 thread 要 ABI 8、kernel 7.0 的旗標才套得上，最低要求從 5.13 變成 7.0）。不選另開一條 pipe 交清單（多一條 pipe 和鎖之前的一段程式，只換到命令列短一點；照記憶 Chromium 在 macOS 上是這樣交 Seatbelt 的參數，macOS 那一段再看）。
12. **`sunaba` 只放通用的機制，照之後獨立出去的樣子寫；renderer 的 policy 在 machopo 這一側（`shell`）**（2026-11-26，使用者決定；改第 7 條的「機制和 renderer 的 policy 都在 `sunaba`」）：使用者預計之後把 `sunaba` 變成獨立的 crate（像 elise），它的 CI 測試到時候在它自己那裡。所以 `sunaba` 不依賴 machopo 的任何 crate，只依賴外面的（`landlock`、`seccompiler`、`libc` 這類）；放的是：描述 policy 的介面（哪些路徑唯讀、哪些 system call 放行、限制參數、回錯誤）、「進 sandbox」的函式、說出被擋的 system call 的 `SIGSYS` handler、最低要求的檢查、輔助的執行檔和它自己的測試、自己的 `README.md` 和 `soundness.md`（不提 machopo）。machopo 專屬的那一份（V8、wasmtime、fontconfig 要的 system call 和路徑）在 `shell`（child 的 `main` 在那裡，只有它用；沒有 `unsafe`），之後 macOS、Windows 的多了再考慮拆成自己的 crate。對照 Chromium：`//sandbox` 是機制，`//sandbox/policy` 是各種 process 的 policy。測試跟著分兩層：`sunaba` 自己的測機制（給一份 policy，被禁止的做不到）；machopo 的測 renderer 的那一份（用 `sunaba` 的輔助執行檔套上 machopo 的 policy）。**這份不加 CI 的步驟**（原本想先在 CI 上問一次 Landlock 的 ABI）：1c 接上之後 `shell` 的測試預設在 sandbox 裡跑，runner 沒開 Landlock 的話每天的 CI 會失敗，那時再處理（CI 加 `--no-sandbox` 或換 runner）。
13. **Linux 的 seccomp 清單的原則**（原則 (3) 的「檔案的呼叫 seccomp 放行、由 Landlock 決定」實作時修正了，2026-11-26：Landlock 不管 `chmod`、`chown`、`utime`、xattr，第 3 版之前也不管 truncate，所以改變檔案系統的呼叫在 seccomp 就回 `EACCES`，`open` 帶寫入、`O_CREAT`、`O_TRUNC` 的也是；讀的照舊由 Landlock 決定。見 `progress.md` 的 1c）（2026-11-26，使用者同意）：(1) 放行、不看參數：探測到 renderer 真的用的記憶體、thread、時鐘、`getrandom`、對開著的檔案的讀寫、控制通道的 `sendmsg`、`recvmsg`、`epoll`。(2) 放行但限制參數（照 Chromium 的 baseline policy）：`kill`、`tgkill` 的對象只能是自己的 process；`prctl` 只許探測到的幾種（thread 的名字、區域的名字、讀 auxv）；`ioctl` 只許問是不是 terminal；`madvise` 只許探測到的幾種；`memfd_create` 放行（wasmtime 自己用的匿名記憶體）。(3) 回錯誤、不當掉：`clone3` 回 `ENOSYS`（seccomp 看不到它的參數；glibc 退回 `clone`，再限制 `clone` 只能建 thread）；檔案的呼叫（`open`、`openat`、`stat` 一族和寫入類的 `unlink`、`rename`、`mkdir`、`chmod`）seccomp 放行、由 Landlock 決定，擋下時是回錯誤（fontconfig 的 cache 過期時會叫寫入類的）。(4) 其餘當掉並說是哪一個（第 5 條）：建 socket、`execve`、`fork`、`ptrace`、`io_uring`、`bpf`、`mount`、`unshare`、改時鐘……。**不保證的事**（寫進 `soundness.md`）：V8 和 wasmtime 的 JIT 要能把記憶體變成可執行的，`mprotect`、`pkey_mprotect` 帶可執行的不擋，所以被攻破的 renderer 在 sandbox 裡照樣能跑它注入的程式碼，只是出不去；Chromium 的 renderer 也是，擋這一類是 V8 的 sandbox 的事。逐個的清單在程式和 `soundness.md`。
14. **Linux 的字型：renderer 先載入字型的清單、再鎖；只放行字型檔的目錄**（目錄由誰列第 15 條改了：Browser 列、經命令列）（2026-11-26，使用者決定；改第 3 條的「清單由 Browser 問 fontconfig」和第 11 條的「要放行的路徑在命令列上」）：renderer 的 `main` 在鎖之前讓 fontique 載入系統字型（fontconfig 的初始化：這時讀它的設定和 cache），從 fontique 的清單取每個字型檔所在的目錄放進 policy，再進 sandbox；之後只讀字型檔。原因：fontique 現成的 API 只給字型檔的路徑，不給 fontconfig 的設定和 cache 的位置，而 renderer 自己的 fontique 初始化時要讀這兩樣。字型服務之後一定會做（之後的可能），這一段是暫時的，所以選現在最省、之後只要刪程式的：不改 fork、不猜路徑、路徑不經過命令列，設定和 cache 的目錄完全不放行（放行的比原本的清單少），cache 過期時 fontconfig 想寫檔案也發生在鎖之前。代價：鎖不再是 `main` 的第一件事，前面多一段 fontconfig 的初始化（只讀本機的設定和 cache，還沒碰網頁的內容；process 還是一個 thread，不是的話 `sunaba` 拒絕）。不選在 fontique 的 fork 加函式回傳設定和 cache 的路徑（為了之後會拿掉的東西改 fork，留下的 commit 還要再清）；不選寫死常見的位置（設定指到別處時字型靜靜地不見）；不選先做字型服務（它在 macOS、Windows 怎麼做還沒探測）。照記憶 Chromium 進 sandbox 之前也有一輪 warm-up，沒有查證。命令列上剩 `--no-sandbox`、繼承的檔案的編號，和鎖之前就要知道的字型的設定（只用測試字型的那個開關）。字型服務做完時拿掉這段初始化，鎖回到 `main` 的第一件事。
15. **字型檔的目錄由 Browser 列一次，經命令列交給 renderer**（2026-11-26，使用者決定；1b；改第 14 條的「renderer 自己取目錄」和「路徑不經過命令列」）：實作時量到 fontique 列出每個字型檔的路徑時會把每個字型檔打開讀一次（開發機 153 個字型檔、32 個目錄：debug 212 ms，release 38 到 79 ms；初始化 fontconfig 本身 13 到 19 ms，現在每個 renderer 本來就做）。照第 14 條原樣，每開一個 renderer process 都付一次。改成 Browser 啟動時在背景列一次目錄，放在每個 renderer 的命令列上（第 11 條原本的那一項）；renderer 鎖之前只初始化 fontconfig，不多花時間。其餘照第 14 條：不改 fork，設定和 cache 的目錄不放行。Browser 執行期間新裝的字型要重開才看得到（寫進 `docs/limitations.md`）。和字型服務的方向一致：Browser 這一側「啟動時載入字型、列出清單」留下來是它的起點，做完時拿掉的是命令列的路徑和 renderer 鎖之前的初始化。不選每個 renderer 自己列（每個分頁的啟動多 40 到 80 ms，debug 的測試每個 renderer 多 200 ms；全部是暫時的）。
16. **trace 檔：renderer 在鎖之前自己開，fork 的 trace 多一個接受開好的檔案的入口**（2026-11-26，使用者決定；A；改第 6 條的「Browser 開好、啟動時交給 renderer」）：實作時發現 trace 是 rusty_v8 的 fork 裡的 C++（Perfetto）做的，它自己照路徑開檔，而且一開始就建一個 thread（`TracingMuxer`）：不能在鎖之前開始（`sunaba` 要 process 只有一個 thread），鎖之後又開不了檔案。改 fork：trace 的開始多一個接受已經開好的檔案的入口；renderer 在鎖之前自己開 `<檔名>.<pid>`（和字型一樣，鎖之前的準備），鎖之後才開始 trace。檔名和量測工具都不用改，不用 Browser 傳檔案（第 11 條的「繼承的檔案的編號」用不到了）。代價：改 rusty_v8 的 C++ 要重編 V8，共用的 artifact 跟著換，fork 多一個 commit。census 的輸出檔是 machopo 自己寫的，同樣改成鎖之前開好。不選開著 `MACHOPO_TRACE` 時 renderer 不進 sandbox（量測工具量到的永遠是沒有 sandbox 的 renderer，seccomp 的成本量不到）；不選 sandbox 放行可寫的那一個檔案（要放寬「開檔不准帶寫入的旗標」，6.2 之前的 kernel 上清空檔案的洞回來一部分）。

17. **macOS 的機制：Seatbelt 的 profile，renderer 在 `main` 的第一件事自己套（M1）**（2026-11-26，照先前的看法和探測；沒有另一個合理的選項）：`sunaba::macos` 的 `enter` 把 profile 的文字和參數交給 `sandbox_init_with_parameters`（不在公開的 header 裡；Chromium、Firefox 一樣宣告它來用）。profile 是 `(deny default)` 加 renderer 要的那幾樣；跟著機器變的值（Browser 的 pid、家目錄、要不要讀系統字型）是參數，不拼進 profile 的文字（參數的內容不會被當成規則，`sunaba` 有測試）。參數由 renderer 自己取：parent 的 pid、環境變數 `HOME`、命令列的 `--test-fonts`；都是 Browser 啟動它時給的。套不上（profile 編不過、系統不收）時照第 4 條不跑，結束碼和 sad tab 的說明和 Linux 同一條路；macOS 沒有「kernel 沒有這個功能」的情況，所以沒有另外的最低要求。進去之後不能換 profile：再套一份不同的會被系統拒絕（測試守著）。不選 App Sandbox（M2：要簽章和 entitlement，開發時的 build 不好用，粒度粗）。不照 Chromium 由 Browser 編好 profile、經一條 pipe 交給 child（`sandbox/mac/seatbelt_exec.cc`，child 在載入自己的 framework 之前讀 pipe、套上）：它的參數（bundle 的路徑、各種目錄）要 Browser 才知道，machopo 的三個 renderer 自己就拿得到，不用多一條 pipe 和鎖之前的一段讀取。違規時回錯誤、不當掉（Seatbelt 的行為，「macOS 的機制」）。
18. **macOS 的字型：renderer 自己讀字型檔，加上系統的字型服務；目錄是固定的四處**（2026-11-26，使用者決定目錄怎麼來，問的是「之後會做字型服務的話選哪個」；其餘照探測）：放行讀 `/System/Library/Fonts`、`/Library/Fonts`、`~/Library/Fonts`、`/System/Library/AssetsV2/com_apple_MobileAsset_Font<數字>`（系統下載的字型，每個一個目錄）和 `FontServices.framework`（系統自己留著用的字型），和查 `com.apple.fonts` 這一個 Mach 服務。字型服務是探測找到的：沒有它頁面照樣畫得出來，但 CoreText 找 fallback 的字型時選到別的（CJK 的字重不同），截圖和沒有 sandbox 時不一樣。`--test-fonts` 時這些全部不放行。目錄固定而不照 Linux 由 Browser 列了經命令列給（第 15 條）：這一段在字型服務做完時要拿掉，固定的是 profile 裡的幾行；Browser 列的話 macOS 上要把五百多個字型檔各開一次、命令列多一百多個目錄（每個下載的字型一個），而之後的字型服務在 macOS 上要交給 renderer 的是字型的內容，用不到目錄的清單。代價：字型管理軟體裝在別處的字型，renderer 列得到、讀不到（`docs/limitations.md`）。對照 Chromium（`common.sb` 的 `allow-font-access`）：前三個目錄、`com.apple.fonts` 和 `com.apple.FontObjectsServer` 兩個服務，下載的字型用系統發的 extension 放行；`FontObjectsServer` 探測裡沒有人查，不放行。
19. **macOS：路徑的 metadata 只放行用到的**（2026-11-26，使用者決定）：`/etc`、`/var` 兩個 symlink（時區的路徑經過它們），和放行讀取的那些路徑。不照 Chromium 放行整個檔案系統的 metadata（`common.sb` 的 `(allow file-read-metadata (subpath "/"))`，理由是讓 `realpath` 能用）：探測顯示畫面和行為不變；之後某個函式庫要 `realpath` 時測試會壞、再加。**實作時發現、和決定時說的不同**：這擋的是路徑的大小、時間和種類，不是「在不在」：不存在的路徑回 `ENOENT`，被擋的回 `EPERM`，renderer 還是分得出來（測試寫明這個限制，`docs/limitations.md`）。
20. **macOS：系統的函式庫會試、但 renderer 不需要的，一律不放行**（2026-11-26，照探測；原則和第 10 條一樣）：使用者的偏好設定（`cfprefsd`、偏好設定的檔案）、`/etc/passwd` 和 `opendirectoryd`（問家目錄）、log 和通知中心的服務、診斷的服務、自己的執行檔、IOKit。探測裡這些全部被擋時，頁面的截圖和沒有 sandbox 時逐 byte 相同，wasm 的文件、crash、trace 都正常。Chromium 的 renderer 放行其中好幾樣（偏好設定、`/etc/passwd`、`logd`、通知中心、IOSurface）：它的 renderer 載入 AppKit 那一層、畫面經 IOSurface 交出去，machopo 的不用。sysctl 只放行 CPU 的數目和 CPU 的功能（`hw.optional.`：JIT 和標準庫照它選指令）；對自己送訊號、系統的 ICU 的資料檔、系統的版本原本放行，mutation test 顯示沒有東西需要，拿掉了（`abort` 不經過 sandbox 管的訊號那一種）；thread 的名字要放行一條（`process-info-setcontrol` 對自己），否則取樣和除錯的工具看不到名字；trace 開著時 Perfetto 問機器的名字和 kernel 的版本被擋，trace 照樣寫出來（少那幾欄，stderr 多一行）。`(deny default)` 之後還允許的幾種操作照 Chromium 明寫擋掉（問其他 process 的資料、IOKit 的屬性、NVRAM、發分散式通知；第一種有測試，其餘靠審查）。

## 待決定

第 1 段（Linux）之前要定的九條都定了（已決定的事項第 1 到 9 條），第 2 段（macOS）的是第 17 到 20 條。每一段開始前只定那一段的：

- **第 1 段開始時**（細節，照已決定的方向）：seccomp 放行的清單和回錯誤的名單（探測的 55 種，加上 wasm 的文件、trace、crash 的路再探一次）；`/proc/self/maps`、`/proc/sys/vm/overcommit_memory`、`/sys` 的那幾個、`/dev/urandom`、時區，每一個放行還是讓它失敗（先用 `strace -k` 找出是誰讀的）；fontconfig 的 cache 過期、寫不了時它怎麼辦；Browser 把字型的路徑和開好的 trace 檔交給 renderer 的方式（命令列、繼承的 fd）；renderer 的 trace 檔名和工具怎麼對上；還在支援期的發行版和 CI 的 kernel 有沒有 Landlock；輔助的執行檔怎麼放（`sunaba` 的 bin）。
- **第 3 段（Windows）**：探測；`CreateProcessW` 和繼承的清單、`sunaba` 和 `ipc::platform::spawn` 怎麼接；token、integrity、job、mitigation；字型；最低要求。

## 之後的可能

**sandbox 收緊的順序**（2026-11-26 和使用者討論過；這份做完之後照這個順序，每一步都是在前一步上加，不用重做）：

1. **字型服務**（另一份 RFC；F2）：Browser 列字型，把字型的清單、fallback 的表和字型檔的內容經過通道給 renderer，renderer 不開任何字型的檔案。等這份的 macOS、Windows 的探測做完再設計（那兩個平台可能本來就需要它：照記憶，Chromium 在 Windows 上經過 DirectWrite 的 proxy）。做完把 Linux 放行的字型路徑（已決定的事項第 3 條）拿掉；時區和其他零星的讀取（`/etc/localtime`、`/proc/self/maps`……）一起改成由 Browser 給或不需要。那時 Landlock 一個路徑都不放行，renderer 讀不到任何檔案的內容。
2. **Linux 加 PID 和 network namespace**（機器允許沒有特權的 user namespace 時；不允許時只有 Landlock 和 seccomp，不做 setuid 的 helper）：外面的 process 在 renderer 裡沒有編號、沒有網路介面，「其他 process」和「網路」多一層不靠 seccomp 的清單寫對的保護。要改啟動 child 的那一段、多 fork 一次、丟掉 user namespace 裡的特權。
3. **Linux chroot 到空的目錄**（完整的 L3，要第 1、2 步）：連路徑在不在都探不出來、`/proc` 整個看不到。

其他：

- **macOS 照 system call 的號碼再擋一層**（Seatbelt 的 `syscall-unix`、`syscall-mach` 和 Mach 訊息的過濾）：現在的 profile 照種類放行，renderer 碰得到的 kernel 介面還是全部的 system call；Linux 的 seccomp 清單那種「只放行用到的」在 macOS 上還沒有。Chromium 的 renderer 也沒有用。要先有 macOS 上的 system call 探測（`dtruss` 要關 SIP）。
- **`sunaba` 變成獨立的 crate**（使用者 2026-11-26 的打算；像 elise：machopo 旁邊自己的專案，`third_party.toml` 釘 commit）：這份照那個樣子寫（已決定的事項第 12 條：不依賴 machopo 的 crate、只放通用的機制、自己的 README 和 soundness 文件、自己的測試和輔助的執行檔）。搬出去時要做的：它自己的 repo 和 CI（三個平台、幾種 kernel 的版本；machopo 的 CI 不跑它的測試）、授權和 `contributing` 的文件、machopo 這一側改成照 `third_party.toml` 釘的路徑相依、`sanity.py unsafe` 的允許清單拿掉它（像 elise 一樣照它自己的規則）、`CLAUDE.md` 的「第三方的 fork」那一節加上它。
- trace 照 Chromium（Perfetto）寫進和 Browser 共用的記憶體、由 Browser 寫檔：trace 要變成常駐、隨時可以開的功能時。
- V8 的 sandbox：開 `v8_enable_sandbox`、`bindings` 的論證改成不信任 V8 heap 裡讀到的東西。
- GPU process：顯示卡的驅動程式（別人的 C）搬出 Browser。
- site isolation：一個 renderer 只有一部分 frame 是 local 的。
