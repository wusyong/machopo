# RFC 0120：Linux 的 renderer sandbox 的下一級：namespace 和 chroot

- 開始日期：2026-11-27
- 狀態：🚧 已排程：2026-11-27（第 1 段做完：Landlock 只剩時區的兩個路徑，只在要讀時放行；第 2 段：namespace；進度在 `progress.md`）
- 細節：`details.md`

## 摘要

Linux 的 renderer process 現在關在 Landlock（檔案照路徑唯讀，只放行四個）加 seccomp-bpf（system call 的白名單）裡。這份在上面再加三層，到 Chromium 的 Linux renderer 那一級：renderer 在自己的 user、PID 和 network namespace 裡（看不到、指不到外面的 process，沒有網路介面）；Landlock 一個路徑都不放（CPU 的數量、亂數、時區改成不用讀檔）；最後 chroot 到空的目錄（連路徑在不在都探不出來，`/proc` 整個看不到）。機器不允許沒有特權的 user namespace 時（Ubuntu 23.10 起預設不允許），退回現在的 Landlock 加 seccomp。三段做，每段可以先 merge。

## 動機

現況（2026-11-27 的 `main`，`360a4c8f`）：

- renderer 在 `main` 一開始、還只有一條 thread 時進 sandbox（`crates/shell/src/renderer_sandbox.rs` 的 `linux::policy`，機制在 `sunaba::linux`）：Landlock 只放行 `/sys/devices/system/cpu/online`、`/dev/urandom`、`/etc/localtime`、`/usr/share/zoneinfo` 唯讀；seccomp 只放行清單上的 system call，`socket`、`connect`、`ptrace`、`process_vm_readv`、`kill`（對別人）、`mount`、`unshare`、`setns` 這些都擋。
- renderer process 是 Browser 用 `std::process::Command` 啟動的（`ipc::platform::linux::channel::spawn`），和 Browser 在同一個 user、PID、network、mount namespace 裡。
- 留著的（`docs/limitations.md` 的「Linux 的 sandbox 擋不住的」）：(1) 看得到檔案在不在、多大、什麼時候改的（`stat`、`access`、`readlink`：Landlock 只管打開）；看得到每個 process 的 pid（`/proc` 的目錄名）；「別的 process」和「網路」只有 seccomp 的白名單一層。

**目標**：

1. renderer 在自己的 PID 和 network namespace 裡：外面的 process 在它看來不存在，它沒有網路介面。這一層不靠 seccomp 的清單寫對。
2. Landlock 一個路徑都不放：CPU 的數量、V8 的亂數、時區都不讀檔。
3. chroot 到空的目錄：路徑在不在、檔案多大、`/proc` 都看不到。
4. 網頁看到的不變（時區、`navigator.hardwareConcurrency`、文字、`Intl`），sandbox 裡外、thread 上的 renderer 都相同。
5. 不能用 user namespace 的機器上照現在的做法跑，renderer 不會因為這份而起不來。
6. 量測和除錯的工具照樣作用：trace、census、`bench.py`、`perf`、從外面讀 `/proc/<pid>`。

**不在範圍內**：

- setuid 的 helper（renderer sandbox 那份 RFC 定了不做）：不能用 user namespace 的機器上不另外補。
- macOS、Windows。
- seccomp 的清單本身（它照舊在最裡面）。
- 語系：ICU 的資料編在執行檔裡，`LANG` 和 `LC_*` 照環境傳下去（renderer 的環境那份 RFC 的第 3 條），chroot 之後也不用讀檔。

要解決的 `docs/limitations.md` 裡的限制：「Linux 的 sandbox 擋不住的」的 (1)。

## 設計概要

給 machopo 的使用者：看不到改變。Ubuntu 23.10 以後的預設設定上，renderer 沒有 namespace 和 chroot 這兩層（照現在的做法），stderr 說一次為什麼和怎麼打開（`details.md` 的「Ubuntu 和 CI」）。

給改 machopo 的人：

- renderer 在 namespace 裡看到的自己的 pid 不是外面的（不是 1 就是 2）：要寫 pid 的地方（檔名、trace）用 Browser 告訴它的那一個。
- renderer 在 chroot 之後打不開任何檔案：要讀的東西在進 sandbox 之前讀好，或由 Browser 給。panic 的 backtrace 解不出函式名（要讀執行檔），除錯時用 `--no-sandbox`。

## 詳細設計

下面每一節是選項和比較；沒有對應的條目的「看法」是 2026-11-27 的。探測的結果在 `details.md`。

### 不能用 user namespace 的機器

| | 做法 | 說明 |
| --- | --- | --- |
| U1 | **退回現在的 Landlock 加 seccomp，stderr 說一次** | 和現在一樣安全；Chromium 在這種機器上沒有 setuid 的 helper 時是起不來（「No usable sandbox」），machopo 比它寬鬆 |
| U2 | 退回，什麼都不說 | 使用者不知道少了兩層 |
| U3 | 不啟動 renderer，要 `--no-sandbox` | 和 Chromium 一樣，但 Ubuntu 的預設設定上 machopo 就不能用了，`--no-sandbox` 反而連 Landlock 和 seccomp 都沒有 |

看法：**U1**。

### 誰建 namespace、誰是 PID 1

namespace 裡的 PID 1 對沒有 handler 的訊號有特別的待遇：量到它呼叫 `abort()` 時 `SIGABRT` 被忽略，glibc 退到別的方法，process 以 `SIGSEGV` 結束（`details.md`）。所以 renderer 不能是 PID 1，前面要有一個只負責收屍、照 renderer 的結果結束的 init。

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| S1 | **Browser 啟動時就在新的 namespace 裡**：`ipc` 的 `spawn` 在 fork 之後、exec 之前（`pre_exec`）`unshare` user 和 network，寫好 uid、gid 的對應；renderer 的 `main` 一開始 `unshare` PID、fork 兩次：第一個 child 是 PID 1（init），它的 child 是 renderer（PID 2） | 改的地方集中在 `ipc` 和 `shell` 的啟動；`std::process::Command` 照用 | 一個 renderer 三個 process（外面那個等著、init、renderer）；init 也要進 sandbox |
| S2 | Browser 用 `clone`（帶 namespace 的旗標）自己啟動 child，不經過 `std::process::Command`；child 是 PID 1、當 init，它 fork 出 renderer | 少一層 process | `clone` 之後到 exec 之前的那段要自己寫（多執行緒的 process 裡 fork 之後只能做 async-signal-safe 的事），是新的 `unsafe`；`ipc` 現在的 `spawn` 要改寫 |
| S3 | renderer 自己在 `main` 一開始 `unshare` 全部、fork 出 init 和 renderer，Browser 不變 | Browser 和 `ipc` 都不改 | user namespace 要在多執行緒之前 `unshare`（renderer 在 `main` 一開始還是一條 thread，做得到）；三個 process 的結構同 S1 |

看法：先在第 2 段的探測比 S1 和 S3（差在 user 和 network namespace 由誰開），S2 只在前兩個做不到時考慮。對照 Chromium（照記憶，沒有對原始碼）：Browser 用 `clone` 帶 namespace 的旗標啟動 zygote，zygote 在 namespace 裡當 init，renderer 是它 fork 出來的。

不管哪一個，都要做的：

- **外面的 pid**：renderer 在 namespace 裡不知道外面看到的自己的 pid。Browser 照舊從啟動拿到 child 的 pid，但那是外面那一層的；renderer 的 pid 要另外知道（工具照它找 renderer、讀 `/proc/<pid>`）。候選：renderer 經過通道送一次 `SCM_CREDENTIALS`，kernel 把裡面的 pid 換成收的一方的 namespace 的（`details.md`）。
- **trace 和 census 的檔名**：現在 renderer 用自己的 pid（`.<pid>`）命名，namespace 裡每個都是 2，會撞名。改成 Browser 給的名字或外面的 pid。
- **seccomp 的「只能對自己送訊號」**：照 `std::process::id()` 算，要在進 PID namespace 之後算（量到在 namespace 裡算的和 kernel 看到的一致）。
- **權限**：在 user namespace 裡 renderer 對它自己的 namespace 是 root，在跑任何網頁的程式之前全部丟掉，再裝 seccomp；seccomp 本來就擋 `unshare`、`setns`、`mount`、帶 namespace 旗標的 `clone`。

### CPU 的數量、亂數、時區

Landlock 放行的四個路徑，各自改成不用讀檔（誰在讀的是 renderer sandbox 那份 RFC 用 `strace -k` 查的）：

| 路徑 | 誰讀 | 看法 |
| --- | --- | --- |
| `/sys/devices/system/cpu/online` | glibc 的 `get_nprocs`：V8 的 worker 數、absl、malloc 的 arena | 定了**讓它讀不到**（第 2 條）：glibc 2.44 讀不到時退回 `sched_getaffinity`，平常數字相同，process 被限制在部分 CPU 上時是那幾個（比較準）。CI 的 glibc（2.39）的退路要確認 |
| `/dev/urandom` | V8 的亂數產生器的種子（`fopen`），讀不到時用時間 | 定了**給 V8 一個 entropy source**（第 3 條）（rusty_v8 的 `V8::set_entropy_source`），用 `getrandom`（已經在 seccomp 的清單裡）；V8 的註解也說這是 embedder 的責任 |
| `/etc/localtime`、`/usr/share/zoneinfo` | ICU 的時區 | 定了**Browser 給 `TZ`**（使用者沒設時；第 4 條）：Browser 照 `/etc/localtime` 解出 IANA 的名字（例如 `Asia/Tokyo`），ICU 用它內建的時區資料、不讀檔。量到：`/etc/localtime` 是 symlink 時，ICU 現在其實靠 `readlink` 就拿到名字，Landlock 不擋 `readlink`；chroot 之後就不行了。`/etc/localtime` 是一般檔案（有些容器）時 ICU 要開檔比對，Browser 也要能解出名字 |

### chroot

- 時機：namespace 建好、權限丟掉之前（chroot 要那個權限），Landlock 和 seccomp 之前。目錄要是空的、renderer 寫不了的。
- 之後還會開檔的：上面的四個（第 1 段之後沒有了）；`/proc/sys/vm/overcommit_memory`（glibc 的 malloc，讀不到就照預設，現在已經是讀不到）；panic 的 backtrace 讀 `/proc/self/exe` 和它的 debug info（chroot 之後解不出函式名）。第 3 段開始時再用 `strace -k` 量一次。
- 做法（空的 tmpfs、刪掉的目錄、Chromium 的做法）在第 3 段定。

### CI

GitHub 的 `ubuntu-24.04` runner（machopo 的 CI 都在上面）照 Ubuntu 的預設應該是不允許沒有特權的 user namespace（推論：runner 的映像檔沒有查證）。

| | 做法 | 說明 |
| --- | --- | --- |
| C1 | **CI 的 job 開始時 `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`** | CI 測的是 namespace 的那條路；Chromium 的文件說這會關掉 Ubuntu 的一項保護，在用完就丟的 runner 上沒有關係 |
| C2 | 給 machopo 的執行檔寫一份 AppArmor profile | 和 Ubuntu 給 Chrome 的一樣；但 CI 的執行檔在 `target/` 裡，路徑每次不同 |
| C3 | 不處理，CI 只測退路 | namespace 的那條路只在開發機上測 |

看法：**C1**，加上一個「退路」的測試在 CI 上用 `unshare` 失敗的情況跑（第 2 段定怎麼造）。

### 測試

- **renderer 真的在 namespace 裡**：從外面看真的 renderer process：`/proc/<pid>/ns/{user,pid,net}` 和 Browser 的不同，`/proc/<pid>/status` 的 `NSpid` 有兩層；probe 從裡面看：只看得到自己和 init，網路介面只有 loopback（`/proc/net` 讀不到，改用 system call）。
- **chroot**：probe 從裡面 `stat("/")` 是空的目錄、`/proc` 不在；`docs/limitations.md` 的 (1) 那幾種探測都失敗。
- **退路**：user namespace 開不了時 renderer 照舊啟動、在 Landlock 加 seccomp 裡、stderr 說了一次。
- **網頁看到的不變**：現有的 sandbox 裡外和 thread 上的比較（時區、`Intl`、寬度）、`navigator.hardwareConcurrency`；用 `TZ` 和一般檔案的 `/etc/localtime` 各跑一次時區。
- **crash**：`about:crash` 照舊是 `SIGABRT`、分頁顯示 sad tab（renderer 不是 PID 1）。
- **工具**：trace、census 各自寫到自己的檔，`renderer_pids` 拿到的是外面的 pid，量測工具的測試（`CLAUDE.md`）。
- mutation test：不開 namespace、不 chroot、不丟權限、seccomp 的「自己」照外面的 pid 算。

### 分階段

0. 草稿和探測（做完；`details.md`）。
1. **Landlock 拿掉四個路徑**（做完，2026-11-27；時區的兩個只在要讀時放行，第 4 條）：V8 的 entropy source；Browser 給 `TZ`；CPU 的數量讓它讀不到（先確認 CI 的 glibc）。和 namespace 無關，最小。
2. **namespace**：user、PID、network；init；外面的 pid、trace 和 census 的檔名、seccomp 的「自己」；退路；CI。
3. **chroot**：要第 1、2 段（在 user namespace 裡才有 chroot 的權限，檔案都不用讀了才 chroot 得了）。

每一段做完可以先 merge（`rfcs/README.md` 的「階段的 merge」）。

## 缺點

- 啟動一個 renderer 多一兩個 process（init），多一次 `unshare` 和 fork；renderer 啟動多等一點（第 2 段量）。
- 新的啟動程式是 machopo 自己的程式碼：丟權限的順序、init 的收屍、fork 之後只能做的事，寫錯了就是洞或 renderer 起不來。
- pid 有兩個（裡面的、外面的），工具和 log 要分清楚；這類錯誤常常是量測工具安靜地量錯。
- chroot 之後 panic 的 backtrace 解不出函式名。
- Ubuntu 的預設設定上收不到這兩層的好處（U1），兩種設定都要測。
- 收益是多一層：namespace 擋的「別的 process」「網路」seccomp 已經擋；只有 chroot 擋住現在擋不住的（路徑在不在、`/proc`）。

## 理由和其他做法

- **不做的話**：Linux 的 renderer 停在 Landlock 加 seccomp：要緊的都擋住了，留著的是資訊的洩漏（檔案在不在、pid）和只有一層的「別的 process」「網路」。2026-11-27 和使用者討論過：不急，但使用者決定做。
- **為什麼 user namespace 不會讓 renderer 更危險**：大家說它危險，指的是系統讓每個沒有特權的程式都能開一個、在裡面當 root，碰得到平常只給 root 的 kernel 程式碼。renderer 的 namespace 是在跑任何網頁的程式之前建好的，之後 seccomp 照樣擋 `unshare`、`setns`、`mount`、建 socket，權限也在那之前丟掉，被攻破的 renderer 碰得到的 kernel 程式碼還是同一份清單。要寫對、要有測試守的就是這個順序。
- **只做 chroot**：做不到，chroot 要權限，沒有特權的 process 要先在 user namespace 裡。
- 各節的選項比較在上面。

## 先例

- **Chromium**（照記憶，第 2 段開始前對原始碼）：Linux 的 renderer 在 user、PID、network namespace 裡、chroot 到空的目錄、丟掉權限，再加 seccomp-bpf；namespace 不能用時改用 setuid 的 helper（`chrome-sandbox`），兩個都不行時不啟動。時區由 sandbox 外面的 process 算（renderer sandbox 那份 RFC 的 `details.md`：「only the out-of-process localtime implementation uses the Sandbox IPC」）。
- **Firefox**：Linux 的 content process 也用 user namespace 和 chroot（照記憶，沒查）。
- **Ubuntu**：23.10 起用 AppArmor 限制沒有特權的 user namespace，給 Chrome、Firefox 的預設安裝路徑另外寫了 profile 放行（`details.md`）。

## 已決定的事項

1. **分段照前提的順序：四個路徑、namespace、chroot（O1）**（2026-11-27，使用者決定）：第 1 段讓 Landlock 一個路徑都不放，第 2 段 user、PID、network namespace，第 3 段 chroot。理由：第 1 段小、風險低、和 namespace 無關，也修一件本來就該做的事（V8 的亂數不再靠 `/dev/urandom`，讀不到時不退到用時間當種子）；第 2 段要的探測（Chromium 的原始碼、CI 的設定、外面的 pid 怎麼知道）可以在第 1 段的期間做。代價：第 1 段在安全上幾乎沒有直接的收益，它是 chroot 的前提。不選 namespace 先（O2：收益比較大的那一層先到，但最難、要改啟動流程的那一段最先做，它的探測還沒做完）。
2. **CPU 的數量：讓它讀不到（P1）**（2026-11-27，使用者決定）：policy 拿掉 `/sys/devices/system/cpu/online`，glibc 的 `get_nprocs` 讀不到時退回 `sched_getaffinity`（這個 process 能用的 CPU）。理由：glibc 2.44 上平常數字相同，process 被限制在部分 CPU 上時更準；Chromium 一樣：它的 renderer 的 seccomp 對檔案系統的 system call 一律回錯誤，`base::SysInfo::NumberOfProcessors()` 用 `sched_getaffinity` 數（`details.md` 的「Chromium 的 CPU 的數量」）。第 1 段開始時先在 CI 的 glibc（2.39）上確認退路一樣；不一樣的話再加「V8 的 worker 數由 machopo 用 `available_parallelism` 給」（P2）。不選照舊放行（P3：第 3 段的 chroot 之後這個檔案也不在了）。
3. **V8 的亂數種子：三個平台都裝 entropy source，用 `getrandom`（R1）**（2026-11-27，使用者決定）：`bindings` 初始化 V8 時呼叫 rusty_v8 的 `V8::set_entropy_source`，用 `getrandom` 這個 crate 填（`crypto.getRandomValues()` 用的同一個來源；Linux 上是 `getrandom(2)`，在 seccomp 的清單上）；它失敗時回 `false`，V8 照它自己的做法。policy 拿掉 `/dev/urandom`。理由：V8 不再自己讀 `/dev/urandom`，讀不到時也不退到用時間當種子；三個平台同一條路；和 Chromium 一樣：`gin/v8_initializer.cc` 在所有平台上裝 `GenerateEntropy`，用 `base::RandBytes`（Linux 上是 BoringSSL 的 `RAND_bytes`；`details.md` 的「Chromium 的 V8 的亂數」）。不選只在 Linux 上裝（R2：多一條平台分支）。不選固定的 `--random-seed`（R3：每個 renderer 的亂數一樣，`Math.random()` 猜得到）。
4. **時區：使用者沒設 `TZ` 時，Browser 照 `/etc/localtime` 的連結解出 IANA 的名字、設成 renderer 的 `TZ`；解不出來的那個 renderer 照舊放行時區的兩個路徑（T1）**（2026-11-27，使用者決定）：名字取連結目標裡 `zoneinfo/` 之後的部分（`../usr/share/zoneinfo/Asia/Tokyo` 是 `Asia/Tokyo`）；`/etc/localtime` 是一般檔案（有些容器）時解不出來。理由：ICU 拿到 `TZ` 就用它內建的時區資料、不讀檔（`details.md` 的「時區不讀檔」）；大多數機器上 Landlock 一個路徑都不放，解不出來的機器時區也不會錯；改得少。代價：解不出來的機器少這一點收益，第 3 段的 chroot 時要再處理；執行期間改了時區，開著的 renderer 不跟著變（和現在一樣）。不選 Browser 照 ICU 的方法比對檔案的內容（T2：所有機器都不放，但要在 Browser 裡重寫 ICU 的那段、結果要和 ICU 一致）。不選照 Chromium 經過訊息給、renderer 設 ICU 的預設時區（T3：之後能做到時區改了跟著變，但 `bindings` 要能設 ICU 的時區，可能要改 rusty_v8 的 C++、重編 V8；「之後的可能」）。Chromium 的做法在 `details.md` 的「Chromium 的時區」。實作時查到、照這條的原則補上的（2026-11-27）：時區的兩個路徑放不放行由 renderer 照它的 `TZ` 決定，照 ICU 自己的規則：ICU 不收的 `TZ`（`putil.cpp` 的 `isValidOlsonID`：帶數字的大多不收，例如 `AST4ADT`）它會改去讀 `/etc/localtime`，所以放行；ICU 收下的名字是三或四個字（`CET`、`EST`）時，它會拿 glibc 的時差比對（`TimeZone::detectHostTimeZone`），glibc 的時差要讀 zoneinfo 裡那個檔案，讀不到時是 0，`CET` 就安靜地變成沒有時差、沒有夏令時間的時區，所以也放行（`details.md` 的「時區不讀檔」）。

## 待決定

第 1 段之前要定的都定了（第 1 到 4 條）。

第 2 段開始時定的：不能用 user namespace 時怎麼辦（U1 到 U3）、誰建 namespace 和 init（S1 到 S3）、外面的 pid 怎麼知道、CI（C1 到 C3）。第 3 段開始時定的：chroot 到哪裡；`/etc/localtime` 是一般檔案的機器怎麼辦（第 4 條留下的）。

## 之後的可能

- seccomp 的清單再收：namespace 和 chroot 之後，有些 system call（`openat`、`statx`、`readlinkat`、`getdents64`）只剩函式庫自己的退路在用，可以改成讓它失敗。
- 時區在執行期間改了時 Browser 通知 renderer（現在 renderer 啟動時決定一次）：照 Chromium，Browser 把時區的名字經過訊息給、renderer 設 ICU 的預設時區、通知 V8（第 4 條不選的 T3），三個平台一起做。
