# RFC 0117：renderer process 關進作業系統的 sandbox

- 開始日期：2026-11-26
- 狀態：✅ 已完成：2026-11-26（Linux、macOS、Windows 各在自己的機器上做完；第 3 段改到的共用程式在 Linux 上由 CI 跑過，在 macOS 上沒有重跑）
- 進度和驗收：`progress.md`
- 細節：`details.md`

## 摘要

renderer 已經是自己的 process（RFC 0004），但沒有被關起來：被攻破的 renderer 照樣用使用者的身分開檔案、連網路。這份把 renderer process 關進作業系統的 sandbox：Linux、macOS、Windows 各用自己的機制，在 child 的 `main` 一開始、任何其他程式碼之前就鎖（RFC 0004 的「為 sandbox 留的條件」假設的時機），套不上就不跑。分段做，Linux 先：Landlock（檔案照路徑唯讀放行）加 seccomp-bpf（只放行清單上的 system call，其餘當掉並說是哪一個）；字型照舊由 renderer 讀，放行的路徑由 Browser 問 fontconfig；程式在新的 crate `sunaba`；開關是 `--no-sandbox`。macOS 是 Seatbelt 的 profile（`deny default`，只放行 renderer 要的那幾樣），renderer 的 `main` 第一件事就套，字型照舊由 renderer 讀、加上系統的字型服務（已決定的事項第 17 到 20 條）。Windows 是 AppContainer（不給任何 capability）加 job 和 process 的 mitigation（連同關掉 win32k 的 system call），由 Browser 在建立 renderer process 時給，renderer 再自己降到 Untrusted integrity；字型照舊由 renderer 讀（已決定的事項第 21 到 27 條）。三個平台的探測（真的 renderer 向系統要了什麼、加上限制之後壞了什麼）在 `details.md`。

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

下面每一節是選項和比較；定了的寫明是哪一條決定，其餘是 2026-11-26 的看法。Chromium 的三個平台都對過原始碼了（`details.md` 的「Chromium 的 Linux sandbox」「Chromium 的 macOS sandbox」「Chromium 的 Windows sandbox」）；Firefox 的 Linux 和 macOS 還是照記憶寫的（`details.md` 的「要對原始碼的地方」）。

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
| W1 | restricted token（最緊的一級：什麼都開不了）、低的 integrity level、job 的限制、process 的 mitigation | Chromium 的 renderer 的做法；由啟動的一方（Browser）在 `CreateProcess` 時給，child 啟動後再自己降完。探測：系統字型整頁空白（DirectWrite 連不上系統的字型服務），要先有 Browser 那一側的字型服務 |
| W2 | AppContainer，加 job 和 mitigation | 應用程式的那一套隔離，照 capability 放行；Chromium 的 renderer 預設不用（開的時候是疊在 W1 上）。探測：不給任何 capability 時系統字型照常，使用者的檔案、寫檔、網路、其他 process 都擋；讀得到開放給所有應用程式的地方（`C:\Windows` 這類） |
| W3 | AppContainer 的 less privileged（LPAC） | 連「所有應用程式」那一組都拿掉。探測：系統字型同樣整頁空白 |

定了 **W2**（已決定的事項第 21 條），token 照 Chromium 自己建、每個 renderer 一個 AppContainer（第 22 條）。探測（`details.md` 的「Windows 的探測」）顯示 W1 和 W2 都做得到，差在字型：W1 要等字型服務，做好之前 Windows 上只有 `--test-fonts` 能進 sandbox。W2 之後可以變成 Chromium 實驗中的組合（AppContainer 疊在 restricted token 上，「之後的可能」）。

和 Linux、macOS 不同的地方：

- **由 Browser 在建立 process 時給，不是 renderer 自己鎖**：token、job、mitigation、繼承哪些 handle 都是 `CreateProcess` 的參數；renderer 從第一個指令起就在裡面，`main` 之前沒有「還沒鎖」的時候。renderer 自己做的只有一步（降到 Untrusted，第 27 條）。所以「鎖之前先開好」的做法在這裡用不上：renderer 要寫的檔案由 Browser 開好、交給它（第 24 條）。
- **要自己呼叫 `CreateProcessW`**：標準庫啟動 process 時不能指定 token、屬性和繼承的清單（第 23 條）。
- **被擋的呼叫回錯誤，不結束 process**（`ERROR_ACCESS_DENIED`、Winsock 的 `WSAEACCES`；job 擋的啟動程式是 `ERROR_NOT_ENOUGH_QUOTA`），和 macOS 一樣。
- **mitigation 是另一類東西**：不是「renderer 能要什麼」，是「被攻破之後能用什麼手法」（關掉 win32k 的 system call、不許載入別人的 DLL……，第 25、26 條）。Linux 的 seccomp 清單兼有這個作用，macOS 沒有對應的。

job 原本就有一個（`ipc` 的 `platform/windows/job.rs`，child 不比 host 活得久）；renderer 的 job 另外加上限制。

### 字型

現在 renderer 自己列系統的字型（parley 的 fontique：Linux 經過 `libfontconfig`，macOS 經過 CoreText，Windows 經過 DirectWrite）。

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| F1 | sandbox 開放字型要讀的路徑（唯讀） | 不用改文字的程式；Linux 的探測顯示全是唯讀的讀檔 | 開放的範圍不小（Linux：fontconfig 的設定、cache、`/usr/share/fonts`、家目錄裡的 `~/.local/share/fonts`、`~/.config/fontconfig`、`~/.cache/fontconfig`）；renderer 讀得到使用者裝了哪些字型（本來網頁就量得出來）；macOS、Windows 的系統服務可能不只是讀檔 |
| F2 | Browser 列字型，把字型的資料（唯讀的共用記憶體）和 fallback 的答案經過通道給 renderer | sandbox 最緊：renderer 一個檔案都不用開 | 要一個同步的查詢（fallback：這個字、這個語言用哪個字型）；fontique 要能從別人給的清單建 collection 和 fallback；三個平台都要做 |
| F3 | 平台各自選：能只靠讀檔的用 F1，要系統服務的用 F2 | 工作量照實際需要 | 兩套做法 |

Linux 定了 **F1**（已決定的事項第 3 條）；macOS 也是 F1，另外放行系統的字型服務（第 18 條）；Windows 也是 F1，不用另外放行什麼（AppContainer 裡 DirectWrite 和系統的字型目錄照常，第 21 條）。對照 Chromium：macOS 的 renderer 直接讀系統字型、連 `com.apple.fonts`（F1，`common.sb` 的 `allow-font-access`）；Linux 和 Windows 經過 Browser 的字型服務（F2，因為 L3 的 chroot 和 DirectWrite 的字型 cache 服務）。

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
3. **Windows**（做完）：先探測（每一種機制底下壞了什麼），再定機制（第 21 到 27 條）。實作分幾步：
   - 3b. `sunaba::windows`：啟動 process 的函式（只繼承指定的 handle；可以不帶 policy）、policy（AppContainer 的 token、job、mitigation）、renderer 自己降 integrity；`sunaba_probe` 的 Windows 版和「做不到」的測試；README 和 `soundness.md`。
   - 3c. `ipc` 的 `spawn`、`spawn_with_pipe` 改用它：自己的 `Child`，拿掉「一次啟動一個」的鎖。
   - 3d. 接上 machopo：`shell::renderer_sandbox` 的 Windows policy、延後載入視窗和繪圖的 DLL、renderer 的 `main` 降 integrity、`--allow-third-party-modules`、從外面看真的 renderer 的測試。
   - 3e. trace 和 census 的檔案由 Browser 開好交給 renderer（rusty_v8 的 fork 的 Rust 加 Windows 的入口）。
   - 3f. 第 3 段的檢查和文件。

每個平台做完可以先 merge（`rfcs/README.md` 的「階段的 merge」）；三段都做完了，三個平台都有。

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

Chromium 的部分 2026-11-26 對過原始碼（`details.md` 的「Chromium 的 Linux sandbox」「Chromium 的 macOS sandbox」「Chromium 的 Windows sandbox」），Firefox 的 Linux 和 macOS 照記憶寫的：

- **Chromium**：renderer 在每個平台都在 sandbox 裡。Linux：zygote 建的 namespace 那一層加 seccomp-bpf 的 policy（違規預設當掉），檔案系統的呼叫一律擋，字型經過 Browser 那一側的 font service；macOS：Seatbelt 的 profile，child 一開始就套；Windows：restricted token、integrity level、job、mitigation、另一個 desktop，字型經過 DirectWrite 的 proxy；AppContainer 對 renderer 預設關。
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

21. **Windows 的機制：AppContainer，不給任何 capability，加 job 和 mitigation（W2）；字型照舊由 renderer 讀（F1）**（2026-11-26，使用者決定；探測在 `details.md` 的「Windows 的探測」）：Browser 建立 renderer process 時給它一個 AppContainer 的 token、一個 job（一個 process、UI 的限制全開、job 關掉時結束）、建立時的 mitigation（第 25、26 條），只繼承指定的 handle（第 23 條）。探測：這樣的 renderer 的頁面截圖（系統字型、測試字型、wasm 的文件）和沒有 sandbox 時逐 byte 相同；從裡面試，使用者家目錄的檔案讀不到、哪裡都建不了檔案、連不出去（連本機的 port 也不行）、開不了其他 process、啟動不了程式（job 擋）。字型不用改：AppContainer 裡 DirectWrite 照常，`C:\Windows\Fonts` 讀得到。套不上（token 建不出來、`CreateProcess` 不收某個屬性）時照第 4 條不跑：Browser 不啟動 renderer，分頁顯示當掉和原因；沒有另外的最低要求（開發機是 Windows 11；Windows 10 上沒試）。做完還留著的（`docs/limitations.md`）：renderer 讀得到開放給所有應用程式的地方（`C:\Windows`、`Program Files` 這類：看得出裝了哪些軟體，沒有使用者的資料）；綁一個本機的 port 來聽會成功（別人連不進來）。不選 Chromium 的 restricted token（W1，`USER_LOCKDOWN`：什麼都開不了，最緊）：探測顯示 machopo 的 renderer 在它底下加一步暖機就跑得起來，但系統字型整頁空白（壞的是 DirectWrite，不是讀不到字型檔），要先有 Browser 那一側的字型服務（另一份 RFC，要改 parley 的 fork）；做好之前 Windows 上只有 `--test-fonts` 能進 sandbox，等於平常用的 machopo 沒有 sandbox。不選兩者疊加（Chromium 實驗中的 `kRendererAppContainer`，預設關）：字型的問題和 W1 一樣；字型服務做完之後把 restricted token 疊上去（「之後的可能」）。不選 LPAC（W3）：系統字型同樣整頁空白。不選 `USER_LIMITED` 那一級的 restricted token：字型照常，但連得出去、啟動得了程式。對照 Chromium：renderer 是 W1 加 job、mitigation、另一個 desktop，字型經過 Browser 的 proxy。
22. **AppContainer 的 token 照 Chromium 自己建，每個 renderer process 一個 AppContainer**（2026-11-26，使用者決定；先探測再定的）：算出 SID（`DeriveAppContainerSidFromAppContainerName`，名字裡帶一個每個 renderer 不同的部分），用 `kernelbase.dll` 匯出的 `CreateAppContainerToken` 從 Browser 自己的 token 建一個帶 AppContainer 的，`CreateProcessAsUserW` 用它。那個函式不在公開的 header 裡；Chromium 的 `AccessToken::CreateAppContainer` 一樣宣告它來用。它不在時（Windows 拿掉了）照套不上處理。理由：不在使用者的機器上留任何東西；每個 renderer 一個不用先登記，renderer 之間多一層隔離；之後疊 restricted token 時把它當成輸入就好，啟動的程式不用重寫。不選登記 profile（`CreateAppContainerProfile` 加 `CreateProcessW` 的屬性：全是公開的 API，探測過也能用，但在使用者的登錄和 `%LOCALAPPDATA%\Packages` 留一筆、machopo 沒有解除安裝的程式；每個 renderer 一個要登記很多個；之後疊 restricted token 要改寫）。不選所有 renderer 共用一個（Chromium 的 renderer 是一個固定的 SID）。原本以為自己建 token 要用 `NtCreateLowBoxToken`、自己建有名字的物件的目錄：讀了 Chromium 的原始碼，不用。
23. **`CreateProcessW` 在 `sunaba`，`ipc` 啟動 renderer 時用它**（2026-11-26，使用者決定 S1；`spawn_with_pipe` 的部分實作時修正了，同一天；第 7 條留到這一段定的）：`sunaba::windows` 提供通用的啟動（`Command`）：程式、參數、環境、要繼承的 handle 的清單，和可選的 policy；不帶 policy 時就是一般的啟動，只多了「只繼承指定的 handle」。`ipc::platform` 的 `spawn`（Browser 啟動 renderer，帶通道）在 Windows 上改用它，`ipc` 依賴 `sunaba`；多一個 `spawn_in`（帶 sandbox）和 `spawn_reading_errors`（測試要讀 renderer 的 stderr）。標準庫做不出 `std::process::Child`，所以 `ipc::platform::Child` 在 Windows 上是 `sunaba` 的（`id`、`kill`、`wait`、`try_wait`），其他平台照舊是標準庫的。命令列的引號、環境的區塊、屬性的清單這幾段 `unsafe` 只寫一次、只論證一次（`sunaba` 的 `soundness.md`）。Browser 啟動的 renderer 因此只繼承通道的那一端和 stdout、stderr，`--no-sandbox` 時也是。**`spawn_with_pipe` 沒有改**（machopodriver 和測試啟動 machopo，帶 DevTools 的 pipe）：它的呼叫端用標準庫的 `Command` 設定 stdout、stderr（接 pipe、接檔案、丟掉），再從標準庫的 `Child` 讀；那些設定從 `Command` 讀不出來，要改就得換掉它的介面和二十幾處測試。它留在標準庫，所以「一次啟動一個」的鎖還在：通道的那一端為了能放進繼承的清單要暫時設成可繼承，那段時間標準庫啟動的 child 會拿到它。Browser 自己不用 `spawn_with_pipe`，所以 `docs/limitations.md` 那一條對 Browser 解掉了，對同一個 process 裡兩種都用的（測試）還在。不選 `ipc` 自己一份沒有 sandbox 的、`spawn` 另外收一個啟動的函式（`ipc` 不依賴 `sunaba`，但同樣的 `unsafe` 寫兩份，函式要從 `shell` 一路傳到 `ipc`）。不選兩邊各一份、互不相干（Chromium 的樣子：`base::LaunchProcess` 和 `sandbox/win` 的 broker）。
24. **Windows 上 renderer 的 trace 和 census 檔由 Browser 開好、交給它；檔案先用別的名字建，process 建好再改名**（2026-11-26，使用者決定；T2 和 H1；第 6 條原本的做法，第 16 條在 Windows 上用不上）：AppContainer 是 process 一建立就在裡面，renderer 沒有「鎖之前自己開」的時候（探測：開不了檔）。量測工具照 `<檔名>.<pid>` 找，而 pid 要 process 建出來才有，所以 Browser 啟動 renderer 之前先在旁邊用暫時的名字建好檔案（開的時候允許它開著時被改名），handle 放進繼承的清單，命令列說它的值（`--trace-file`、`--census-file`）；process 建好之後 Browser 把檔案改名成 `<檔名>.<pid>`。renderer 的 `main` 照 Linux、macOS 的順序：拿到檔案（這裡是接手，不是開）、降權、才開始 trace；trace 涵蓋的範圍和其他平台一樣。沒有 sandbox（`--no-sandbox`）時 renderer 照其他平台自己開。**rusty_v8 的 fork 只改了 Rust，不用重編 V8**（原本以為要改 C++）：fork 的 C++ 收的是 C runtime 的 file descriptor，每個平台都一樣，只有 Rust 的包裝標了只限 Unix；Windows 上用 `_open_osfhandle` 把 handle 變成 descriptor。census 的檔案是 machopo 自己寫的，`base::census` 多一個接受開好的檔案的入口。不選 process 建好才開檔、經控制通道的第一則訊息給（H2：不用改名，但 trace 要等讀到那則訊息才開始，renderer 最早的那一段沒有；控制通道的協定在 Windows 上要多帶 handle）。不選量測時加 `--no-sandbox`（量到的永遠不是 sandbox 裡的 renderer；AppContainer 的成本預期在雜訊裡，但那是推論）。不選讓那一個檔案可寫（Browser 先建檔、把這個 renderer 的 AppContainer 加進它的權限：流程最繞，沒探測）。
25. **建立時的 mitigation：Chromium 的清單，連同「只許 Microsoft 簽章的 DLL」；附 `--allow-third-party-modules`**（2026-11-26，使用者決定；每一個都探測過，`details.md` 的「其餘的 mitigation」）：DEP 和不許 ATL thunk、SEHOP、heap 損壞時結束、bottom-up ASLR、不許 extension point、不許非系統的字型（kernel 不解析頁面給的字型；machopo 的 web font 在 process 裡自己解析）、不許從遠端和低 integrity 的位置載入 DLL、限制 indirect branch 的預測、關掉 KTM、關掉 FSCTL 的 system call、不許建 child process，和只許載入 Microsoft 簽章的 DLL；renderer 啟動後自己開的兩個（Windows 自己啟動 process 的過程會違反它們，建立時給不了）：DLL 只在系統的目錄找、嚴格的 handle 檢查。全部一起開時截圖和沒有 sandbox 時相同。「只許 Microsoft 簽章的」在建立時就給，不照 Chromium 底層的 sandbox 等啟動之後才開：Chromium 要等，是因為它自己的 `chrome.dll` 不是 Microsoft 簽的（它的 renderer 另外在建立時也給，配一套放行自己的 DLL 的機制）；machopo 是一個執行檔、沒有自己的 DLL，主執行檔不受這個 policy 管，所以建立時給不用放行什麼，連啟動時被注入的 DLL 都擋。它會和把 DLL 注入其他 process 的軟體（防毒、錄影、顯示卡的 overlay）衝突，開發機上沒有那種軟體、沒試到：所以附一個開關 `--allow-third-party-modules`（和 Chromium 同名）關掉這一個，renderer 起不來時分頁的說明提示它。不選不加（沒有機器可以測相容性；但 Chromium 預設就開著）。不選沒有開關（衝突時只剩 `--no-sandbox`，為一個 mitigation 丟掉整個 sandbox）。不加的（Chromium 對 renderer 也不開）：不許動態產生程式碼（和 JIT 衝突）、CET（V8 換 stack 的方式不合）。
26. **關掉 win32k 的 system call：視窗和繪圖的 DLL 改成延後載入；另一個 desktop 不做**（2026-11-26，使用者決定；W1；兩個都探測過）：`machopo.exe` 是 Browser 和 renderer 共用的執行檔，靜態連著 `user32`、`gdi32`，建立時關 win32k 則 process 起不來，啟動後關被系統拒絕。連結時把 renderer 用不到的 DLL 標成延後載入（`/DELAYLOAD`；清單在 `details.md`）：第一次呼叫到才載入，renderer 永遠不載入它們（41 個 DLL 變成 19 個），建立時關 win32k 就可以了；`dwrite` 用到時才載入，不帶進 `user32`、`gdi32`。探測：全部都開的截圖相同，有視窗的 Browser 照常。對照 Chromium：renderer 一樣關 win32k，一樣不讓 `user32` 在啟動時載入。另一個 window station 和 desktop（Chromium 的 `kAlternateWinstation`）不做：它擋的是對同一個 desktop 上其他視窗送訊息、裝 hook，那些全是 win32k 的 system call，關掉之後 renderer 叫不出去；探測顯示加上去沒有壞，但 renderer 沒有一個 thread 會連到任何 desktop，測不出它擋了什麼，而建它要暫時換 Browser 整個 process 的 window station。不做的代價（`docs/limitations.md`）：win32k 的限制是單點，被繞過時 renderer 在使用者的 desktop 上（還有 job 的 UI 限制、AppContainer 和 integrity 擋掉大部分）；之後 win32k 的限制因為任何原因要拿掉時，先補上另一個 desktop（「之後的可能」）。
27. **renderer 啟動後自己降到 Untrusted integrity，降不下去就不跑**（2026-11-26，使用者決定；U1；探測過）：AppContainer 的 token 建立時是 Low；renderer 的 `main` 第一件事把自己的 token 降到 Untrusted（之後回不去），再開第 25 條那兩個啟動後才能開的 mitigation，然後才建通道和它的 thread。任何一步失敗就結束，和 Linux、macOS 進不了 sandbox 同一條路（結束碼、分頁的說明）。多擋的是寫 Low integrity 的物件（Chromium：「Prevents the renderers from manipulating low-integrity processes」）；探測裡看不出差別（AppContainer 已經擋了試的那幾樣），截圖相同。這是 Windows 上 renderer 自己要做的唯一一段；在那之前它已經在 AppContainer、job 和建立時的 mitigation 裡。之後疊 restricted token 時，還回啟動用的 token 也在這裡。不選留在 Low（renderer 這一側完全不用做事）。

## 待決定

沒有。三段各自開始前要定的都定了（已決定的事項第 1 到 9 條、第 17 到 20 條、第 21 到 27 條）；沒做的在「之後的可能」，還留著的限制在 `docs/limitations.md` 的「隔離」。

## 之後的可能

**sandbox 收緊的順序**（2026-11-26 和使用者討論過；這份做完之後照這個順序，每一步都是在前一步上加，不用重做）：

1. **字型服務**（另一份 RFC；F2）：Browser 列字型，把字型的清單、fallback 的表和字型檔的內容經過通道給 renderer，renderer 不開任何字型的檔案。三個平台的探測都做完了：macOS 要交給 renderer 的是字型的內容（第 18 條），Windows 上 renderer 要改成不用 DirectWrite 的系統字型集合（它在更緊的 token 底下壞掉，`details.md` 的「Windows 的探測」；Chromium 在 Windows 上經過 DirectWrite 的 proxy）。做完把 Linux 放行的字型路徑（已決定的事項第 3 條）拿掉；時區和其他零星的讀取（`/etc/localtime`、`/proc/self/maps`……）一起改成由 Browser 給或不需要。那時 Landlock 一個路徑都不放行，renderer 讀不到任何檔案的內容。
2. **Linux 加 PID 和 network namespace**（機器允許沒有特權的 user namespace 時；不允許時只有 Landlock 和 seccomp，不做 setuid 的 helper）：外面的 process 在 renderer 裡沒有編號、沒有網路介面，「其他 process」和「網路」多一層不靠 seccomp 的清單寫對的保護。要改啟動 child 的那一段、多 fork 一次、丟掉 user namespace 裡的特權。
3. **Linux chroot 到空的目錄**（完整的 L3，要第 1、2 步）：連路徑在不在都探不出來、`/proc` 整個看不到。

其他：

- **Windows 在 AppContainer 上疊 restricted token**（Chromium 的 `USER_LOCKDOWN`；字型服務做完之後）：renderer 連開放給所有應用程式的地方都讀不到。探測過：machopo 的 renderer 在它底下要在 process 的第一個 thread 上先要一次亂數（C runtime 的 `rand_s` 第一次要載入系統的 DLL），之後還回啟動用的 token；啟動的程式不用重寫（第 22 條）。
- **Windows 的另一個 window station 和 desktop**（第 26 條沒做的）：探測過、加上去沒有壞。win32k 的限制要拿掉時先補它。
- **過濾 renderer 的環境變數**（三個平台；2026-11-26 和使用者討論過，先不做）：現在 renderer 繼承 Browser 整份環境，使用者放在環境變數裡的東西（API key、token、路徑）被攻破的 renderer 讀得到，不經過任何一個平台的 sandbox：它們在啟動時就在 renderer 的記憶體裡。Chromium 在 Windows 上只留七個（`Path`、`SystemDrive`、`SystemRoot`、`TEMP`、`TMP`、`LOCALAPPDATA` 和它自己 crash 回報用的一個；`sandbox/win/src/broker_services.cc`）；Linux 和 macOS 上有沒有過濾沒查到。做法：Browser 組 renderer 的命令時清掉環境、只放回清單上的（machopo 自己的 `MACHOPO_` 開頭的、開發用的 `RUST_LOG` 這幾個、各平台的函式庫要的：Windows 的 `SystemRoot`，Linux 的 fontconfig 要的 `HOME`、`XDG_*`、`FONTCONFIG_*`，ICU 要的 `TZ`，macOS 的 profile 要的 `HOME`）。清單漏一個時壞得安靜（字型、時區不對），每個平台要在那個平台上跑過；所以沒有跟著 Windows 這一段做。
- **renderer 的記憶體上限**（Windows 的 job 可以設；Chromium 的 job 有）：不是隔離，是失控的 renderer 不拖垮整台機器。
- **macOS 照 system call 的號碼再擋一層**（Seatbelt 的 `syscall-unix`、`syscall-mach` 和 Mach 訊息的過濾）：現在的 profile 照種類放行，renderer 碰得到的 kernel 介面還是全部的 system call；Linux 的 seccomp 清單那種「只放行用到的」在 macOS 上還沒有。Chromium 的 renderer 也沒有用。要先有 macOS 上的 system call 探測（`dtruss` 要關 SIP）。
- **`sunaba` 變成獨立的 crate**（使用者 2026-11-26 的打算；像 elise：machopo 旁邊自己的專案，`third_party.toml` 釘 commit）：這份照那個樣子寫（已決定的事項第 12 條：不依賴 machopo 的 crate、只放通用的機制、自己的 README 和 soundness 文件、自己的測試和輔助的執行檔）。搬出去時要做的：它自己的 repo 和 CI（三個平台、幾種 kernel 的版本；machopo 的 CI 不跑它的測試）、授權和 `contributing` 的文件、machopo 這一側改成照 `third_party.toml` 釘的路徑相依、`sanity.py unsafe` 的允許清單拿掉它（像 elise 一樣照它自己的規則）、`CLAUDE.md` 的「第三方的 fork」那一節加上它。
- trace 照 Chromium（Perfetto）寫進和 Browser 共用的記憶體、由 Browser 寫檔：trace 要變成常駐、隨時可以開的功能時。
- V8 的 sandbox：開 `v8_enable_sandbox`、`bindings` 的論證改成不信任 V8 heap 裡讀到的東西。
- GPU process：顯示卡的驅動程式（別人的 C）搬出 Browser。
- site isolation：一個 renderer 只有一部分 frame 是 local 的。
