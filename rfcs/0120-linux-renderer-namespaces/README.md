# RFC 0120：Linux 的 renderer sandbox 的下一級：namespace 和 chroot

- 開始日期：2026-11-27
- 狀態：🚧 已排程：2026-11-27（三段都做完，待收尾：Landlock 只在要讀時放行時區的兩個路徑；renderer 在自己的 user 和 network namespace 裡、chroot 到一個已經不在的目錄；進度在 `progress.md`）
- 細節：`details.md`

## 摘要

Linux 的 renderer process 現在關在 Landlock（檔案照路徑唯讀，只放行四個）加 seccomp-bpf（system call 的白名單）裡。這份在上面再加三層，接近 Chromium 的 Linux renderer 那一級：renderer 在自己的 user 和 network namespace 裡（沒有網路介面，chroot 的權限從這裡來；PID namespace 不做，第 6 條）；Landlock 一個路徑都不放（CPU 的數量、亂數、時區改成不用讀檔）；最後 chroot 到空的目錄（連路徑在不在都探不出來，`/proc` 整個看不到）。機器不允許沒有特權的 user namespace 時（Ubuntu 23.10 起預設不允許），退回現在的 Landlock 加 seccomp。三段做，每段可以先 merge。

## 動機

現況（2026-11-27 的 `main`，`360a4c8f`）：

- renderer 在 `main` 一開始、還只有一條 thread 時進 sandbox（`crates/shell/src/renderer_sandbox.rs` 的 `linux::policy`，機制在 `sunaba::linux`）：Landlock 只放行 `/sys/devices/system/cpu/online`、`/dev/urandom`、`/etc/localtime`、`/usr/share/zoneinfo` 唯讀；seccomp 只放行清單上的 system call，`socket`、`connect`、`ptrace`、`process_vm_readv`、`kill`（對別人）、`mount`、`unshare`、`setns` 這些都擋。
- renderer process 是 Browser 用 `std::process::Command` 啟動的（`ipc::platform::linux::channel::spawn`），和 Browser 在同一個 user、PID、network、mount namespace 裡。
- 留著的（`docs/limitations.md` 的「Linux 的 sandbox 擋不住的」）：(1) 看得到檔案在不在、多大、什麼時候改的（`stat`、`access`、`readlink`：Landlock 只管打開）；看得到每個 process 的 pid（`/proc` 的目錄名）；「別的 process」和「網路」只有 seccomp 的白名單一層。

**目標**：

1. renderer 在自己的 user 和 network namespace 裡：它沒有網路介面，這一層不靠 seccomp 的清單寫對；user namespace 給第 3 段的 chroot 要的權限。原本也打算放進自己的 PID namespace（外面的 process 在它看來不存在），定了不做（第 6 條）。
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

- renderer 在 chroot 之後打不開任何檔案：要讀的東西在進 sandbox 之前讀好，或由 Browser 給。panic 的 backtrace 解不出函式名（要讀執行檔），除錯時用 `--no-sandbox`。

## 詳細設計

下面每一節是選項和比較；沒有對應的條目的「看法」是 2026-11-27 的。探測的結果在 `details.md`。

### 不能用 user namespace 的機器

| | 做法 | 說明 |
| --- | --- | --- |
| U1 | **退回現在的 Landlock 加 seccomp，stderr 說一次** | 和現在一樣安全；Chromium 在這種機器上沒有 setuid 的 helper 時是起不來（「No usable sandbox」），machopo 比它寬鬆 |
| U2 | 退回，什麼都不說 | 使用者不知道少了兩層 |
| U3 | 不啟動 renderer，要 `--no-sandbox` | 和 Chromium 一樣，但 Ubuntu 的預設設定上 machopo 就不能用了，`--no-sandbox` 反而連 Landlock 和 seccomp 都沒有 |

定了 **U1**（已決定的事項第 5 條）。

### 哪幾種 namespace、誰建

原本打算 user、PID、network 三種都要。PID namespace 要的東西比預期多：namespace 裡的 PID 1 對沒有 handler 的訊號有特別的待遇（量到它呼叫 `abort()` 時以 `SIGSEGV` 結束，`details.md`），renderer 不能直接是 PID 1，前面要有 init；外面要有一個 process 把結束的狀態原樣交回 Browser（量過，行得通）；renderer 在裡面的 pid 不是外面的，trace、census 的檔名和量測工具都要改，Browser 要另外知道它外面的 pid（量過 `SCM_CREDENTIALS` 行得通）。而它帶來的大多已經有了：碰不到別的 process 是 seccomp 擋的（只能對自己送訊號，`ptrace`、`process_vm_readv` 都擋），看不到別的 process 是 Landlock（`/proc` 底下開不了）和第 3 段的 chroot（`/proc` 整個看不到）；chroot 要的權限是 user namespace 給的，不用 PID namespace。

| | 做法 | 一個分頁幾個 process | 好處 | 代價 |
| --- | --- | --- | --- | --- |
| K5 | **只要 user 和 network**：`ipc` 的 `spawn` 在 fork 之後、exec 之前（`pre_exec`，通道的設定也在這裡）`unshare` 這兩個、寫好 uid、gid 的對應；能不能用，Browser 在一個 child 裡試一次、記住（第 5 條） | 1 | process 和 pid 都和現在一樣，trace、census、工具不用改，不用知道外面的 pid；沒有 PID 1 的問題；renderer 沒有網路介面；chroot 做得成 | 沒有 PID namespace 這一層：「別的 process」只靠 seccomp（現在就是）；chroot 之前還看得到 pid 的清單（`/proc` 的目錄名） |
| K3 | 三種都要，Browser 用帶旗標的 `clone` 啟動，renderer 是 PID 1、裝訊號的 handler | 1 | 有 PID namespace | `ipc` 要自己 `clone` 加 exec（新的 `unsafe`，在多執行緒的 Browser 裡）；PID 1、外面的 pid、檔名都要處理 |
| K1 | 三種都要，renderer 在 `main` 一開始 fork 出進新 namespace 的 child，child 是 renderer、是 PID 1（裝 handler），原本的 process 在外面等 | 2 | 不改 Browser 和 `ipc` | renderer 的 `abort()` 被 handler 接走、沒有 core dump；V8、wasmtime 也裝訊號的 handler；外面的 pid、檔名 |
| K2 | 同 K1，但 child 是 init（PID 1），init 再 fork 出 renderer（PID 2） | 3 | renderer 是普通的 process（量過：`nsinit.c`） | 10 個分頁 30 個 process；外面的 pid、檔名 |

定了 **K5**（已決定的事項第 6 條）。對照 Chromium：三種都用，每個 renderer 是自己的 PID namespace 的 PID 1（裝 handler），zygote 是所有 renderer 的父 process，等、收屍、回報結束的狀態（`details.md` 的「Chromium 的 namespace sandbox」）；machopo 沒有 zygote，PID namespace 的那些事要由每個 renderer 自己扛。

要做的：

- **在哪裡**：`ipc::platform::linux::channel::spawn` 的 `pre_exec`（fork 之後、exec 之前，只能做 async-signal-safe 的事：`unshare` 和寫 `/proc/self/uid_map` 這類 system call，不配置記憶體）；`unsafe` 在 `ipc`，照 `crates/ipc/soundness.md` 寫論證。要不要 namespace 由啟動的一方告訴 `spawn`（只有 renderer 要，machopodriver、測試的 child 不要）。
- **權限**：uid 和 gid 在 namespace 裡對到自己（不是 0），renderer 在裡面是同一個使用者、不是 root，exec 時 kernel 照「不是 root、程式沒有自己的權限」重算，user namespace 給的權限全部沒了（量過：`CapPrm`、`CapEff` 都是 0）。第 3 段的 chroot 要權限時，在那一段定怎麼留。seccomp 本來就擋 `unshare`、`setns`、`mount`、建 socket。
- **退路**：Browser 第一次啟動 renderer 之前在一個 fork 出來的 child 裡試一次（`unshare` 和寫對應都成功），記住結果；不行的話照現在的做法啟動，Browser 在 stderr 說一次（第 5 條）。
- **機制在 `sunaba`**（`sunaba::linux::Namespaces`：`new` 在 fork 之前寫好對應，`enter` 在 child 裡做，`available` 試），`ipc` 的 Linux 上 `Sandbox` 就是它，`spawn_in` 在 `pre_exec` 裡呼叫 `enter`；`shell::renderer_sandbox::sandbox()` 在 Linux 上給它（`--no-sandbox` 時不給）。

### CPU 的數量、亂數、時區

Landlock 放行的四個路徑，各自改成不用讀檔（誰在讀的是 renderer sandbox 那份 RFC 用 `strace -k` 查的）：

| 路徑 | 誰讀 | 看法 |
| --- | --- | --- |
| `/sys/devices/system/cpu/online` | glibc 的 `get_nprocs`：V8 的 worker 數、absl、malloc 的 arena | 定了**讓它讀不到**（第 2 條）：glibc 2.44 讀不到時退回 `sched_getaffinity`，平常數字相同，process 被限制在部分 CPU 上時是那幾個（比較準）。CI 的 glibc（2.39）的退路要確認 |
| `/dev/urandom` | V8 的亂數產生器的種子（`fopen`），讀不到時用時間 | 定了**給 V8 一個 entropy source**（第 3 條）（rusty_v8 的 `V8::set_entropy_source`），用 `getrandom`（已經在 seccomp 的清單裡）；V8 的註解也說這是 embedder 的責任 |
| `/etc/localtime`、`/usr/share/zoneinfo` | ICU 的時區 | 定了**Browser 給 `TZ`**（使用者沒設時；第 4 條）：Browser 照 `/etc/localtime` 解出 IANA 的名字（例如 `Asia/Tokyo`），ICU 用它內建的時區資料、不讀檔。量到：`/etc/localtime` 是 symlink 時，ICU 現在其實靠 `readlink` 就拿到名字，Landlock 不擋 `readlink`；chroot 之後就不行了。`/etc/localtime` 是一般檔案（有些容器）時 ICU 要開檔比對，Browser 也要能解出名字 |

### chroot

- **權限**（第 8 條）：Browser 在 `pre_exec` 裡只把 `CAP_SYS_CHROOT` 留成 ambient capability（`Namespaces::keeping_chroot`），renderer exec 之後只有它。
- **在哪裡、怎麼做**（第 9 條）：sunaba 的 `enter`，照順序：確認只有一條 thread（讀 `/proc`，所以在 chroot 之前）、`tzset()`、policy 要 chroot 而且 process 有那個權限時，一條 thread chroot 到自己的 `/proc/thread-self/fdinfo`、`chdir("/")` 再結束（Chromium 的 `ChrootToSafeEmptyDir`）、確認 `/proc` 看不到了、丟掉全部的權限（ambient 的也是）、Landlock、seccomp。chroot 的 policy 不讀任何路徑（`enter` 拒絕讀的）。
- **哪些 renderer chroot**：時區不用讀檔的（`TZ` 是 ICU 收的名字：Browser 照 `/etc/localtime` 的連結或複本給的，或使用者的）而且在自己的 namespace 裡的；不能用 namespace 的機器（沒有權限）、`/etc/localtime` 是自己做的時區檔的機器（第 10 條），照第 1、2 段的做法。
- **時區**：`tzset()` 讓 glibc 在還讀得到時讀好 `TZ` 的時區（短的名字，ICU 拿 glibc 的時差比對的，不用再放行檔案）；`/etc/localtime` 是複本時，Browser 比對 zoneinfo 找出名字（第 10 條）。
- **之後還會開檔的**（量的，`details.md` 的「第 3 段的探測」）：chroot 之前就已經讀不到或本來就不在的那幾個，沒有新的；panic 的 backtrace 讀不到執行檔，解不出函式名（`docs/limitations.md`）。找不到的錯誤碼是 `ESRCH`：`shell` 的整套測試在 chroot 的 renderer 上照樣通過。

### CI

GitHub 的 `ubuntu-24.04` runner（machopo 的 CI 都在上面）照 Ubuntu 的預設應該是不允許沒有特權的 user namespace（推論：runner 的映像檔沒有查證）。

| | 做法 | 說明 |
| --- | --- | --- |
| C1 | **CI 的 job 開始時 `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`** | CI 測的是 namespace 的那條路；Chromium 的文件說這會關掉 Ubuntu 的一項保護，在用完就丟的 runner 上沒有關係 |
| C2 | 給 machopo 的執行檔寫一份 AppArmor profile | 和 Ubuntu 給 Chrome 的一樣；但 CI 的執行檔在 `target/` 裡，路徑每次不同 |
| C3 | 不處理，CI 只測退路 | namespace 的那條路只在開發機上測 |

定了 **C3**（已決定的事項第 7 條）：測試兩種設定都會過，不允許的 runner 上測的是退路。

### 測試

- **renderer 真的在 namespace 裡**：從外面看真的 renderer process：`/proc/<pid>/ns/{user,net}` 和 Browser 的不同；probe 從裡面看：網路介面只有 loopback（`/proc/net` 讀不到，改用 system call），權限都丟掉了。
- **chroot**：從外面看真的 renderer process 的根目錄是一條已經結束的 thread 的 `fdinfo`（`/proc/<pid>/root`），權限都是 0；sunaba 的 probe 從裡面看，`/` 以外的路徑都是 `ESRCH`；`/etc/localtime` 是複本時，renderer 拿到名字、chroot、看到的時差和 thread 上的相同；短的名字（`CET`）在 chroot 的 renderer 裡照樣對。
- **退路**：user namespace 開不了時 renderer 照舊啟動、在 Landlock 加 seccomp 裡、stderr 說了一次。
- **網頁看到的不變**：現有的 sandbox 裡外和 thread 上的比較（時區、`Intl`、寬度）、`navigator.hardwareConcurrency`；用 `TZ` 和一般檔案的 `/etc/localtime` 各跑一次時區。
- **crash**：`about:crash` 照舊是 `SIGABRT`、分頁顯示 sad tab。
- **工具**：量測工具的測試（`CLAUDE.md`：改了 renderer 怎麼啟動）。
- mutation test：不開 namespace、不寫對應、退路的判斷反了、不 chroot、不丟權限。

### 分階段

0. 草稿和探測（做完；`details.md`）。
1. **Landlock 拿掉四個路徑**（做完，2026-11-27；時區的兩個只在要讀時放行，第 4 條）：V8 的 entropy source；Browser 給 `TZ`；CPU 的數量讓它讀不到（先確認 CI 的 glibc）。和 namespace 無關，最小。
2. **namespace**（做完，2026-11-27）：user、network（第 6 條）；退路（第 5 條）；CI 先不處理（第 7 條）。
3. **chroot**（做完，2026-11-27；第 8 到 10 條）：要第 1、2 段（在 user namespace 裡才有 chroot 的權限，檔案都不用讀了才 chroot 得了）。

每一段做完可以先 merge（`rfcs/README.md` 的「階段的 merge」）。

## 缺點

- 啟動一個 renderer 多一次 `unshare` 和寫對應：約 0.5 ms（量的：`unshare --user --net` 加上去的中位數，`details.md`）；Browser 第一次啟動 renderer 時多一次 fork 試。
- 新的啟動程式是 machopo 自己的程式碼：丟權限的順序、fork 之後只能做的事，寫錯了就是洞或 renderer 起不來。
- chroot 之後 panic 的 backtrace 解不出函式名；找不到的錯誤碼是 `ESRCH`、不是 `ENOENT`。
- Browser 多一段照 ICU 的方法比對 zoneinfo 的程式，挑出的名字可能是同一份規則的別名（時間一樣）。
- Ubuntu 的預設設定上收不到這兩層的好處（U1），兩種設定都要測。
- 收益是多一層：network namespace 擋的「網路」seccomp 已經擋；只有 chroot 擋住現在擋不住的（路徑在不在、`/proc`）。沒有 PID namespace，「別的 process」還是只有 seccomp 一層（第 6 條）。

## 理由和其他做法

- **不做的話**：Linux 的 renderer 停在 Landlock 加 seccomp：要緊的都擋住了，留著的是資訊的洩漏（檔案在不在、pid）和只有一層的「別的 process」「網路」。2026-11-27 和使用者討論過：不急，但使用者決定做。
- **為什麼 user namespace 不會讓 renderer 更危險**：大家說它危險，指的是系統讓每個沒有特權的程式都能開一個、在裡面當 root，碰得到平常只給 root 的 kernel 程式碼。renderer 的 namespace 是在跑任何網頁的程式之前建好的，之後 seccomp 照樣擋 `unshare`、`setns`、`mount`、建 socket，權限也在那之前丟掉，被攻破的 renderer 碰得到的 kernel 程式碼還是同一份清單。要寫對、要有測試守的就是這個順序。
- **只做 chroot**：做不到，chroot 要權限，沒有特權的 process 要先在 user namespace 裡。
- 各節的選項比較在上面。

## 先例

- **Chromium**（2026-11-27 對過原始碼，`details.md` 的「Chromium 的 namespace sandbox」）：Linux 的 renderer 在 user、PID、network namespace 裡（每個 renderer 是自己的 PID namespace 的 PID 1，zygote 是它們的父 process）、chroot 到空的目錄、丟掉權限，再加 seccomp-bpf；要求 namespace 但開不了時 `CHECK` 失敗；改用 setuid 的 helper（`chrome-sandbox`）是在啟動的地方決定的（沒讀到）。時區由 sandbox 外面的 process 算（renderer sandbox 那份 RFC 的 `details.md`：「only the out-of-process localtime implementation uses the Sandbox IPC」）。
- **Firefox**：Linux 的 content process 也用 user namespace 和 chroot（照記憶，沒查）。
- **Ubuntu**：23.10 起用 AppArmor 限制沒有特權的 user namespace，給 Chrome、Firefox 的預設安裝路徑另外寫了 profile 放行（`details.md`）。

## 已決定的事項

1. **分段照前提的順序：四個路徑、namespace、chroot（O1）**（2026-11-27，使用者決定）：第 1 段讓 Landlock 一個路徑都不放，第 2 段 namespace（後來定了只要 user 和 network，第 6 條），第 3 段 chroot。理由：第 1 段小、風險低、和 namespace 無關，也修一件本來就該做的事（V8 的亂數不再靠 `/dev/urandom`，讀不到時不退到用時間當種子）；第 2 段要的探測（Chromium 的原始碼、CI 的設定、外面的 pid 怎麼知道）可以在第 1 段的期間做。代價：第 1 段在安全上幾乎沒有直接的收益，它是 chroot 的前提。不選 namespace 先（O2：收益比較大的那一層先到，但最難、要改啟動流程的那一段最先做，它的探測還沒做完）。
2. **CPU 的數量：讓它讀不到（P1）**（2026-11-27，使用者決定）：policy 拿掉 `/sys/devices/system/cpu/online`，glibc 的 `get_nprocs` 讀不到時退回 `sched_getaffinity`（這個 process 能用的 CPU）。理由：glibc 2.44 上平常數字相同，process 被限制在部分 CPU 上時更準；Chromium 一樣：它的 renderer 的 seccomp 對檔案系統的 system call 一律回錯誤，`base::SysInfo::NumberOfProcessors()` 用 `sched_getaffinity` 數（`details.md` 的「Chromium 的 CPU 的數量」）。第 1 段開始時先在 CI 的 glibc（2.39）上確認退路一樣；不一樣的話再加「V8 的 worker 數由 machopo 用 `available_parallelism` 給」（P2）。不選照舊放行（P3：第 3 段的 chroot 之後這個檔案也不在了）。
3. **V8 的亂數種子：三個平台都裝 entropy source，用 `getrandom`（R1）**（2026-11-27，使用者決定）：`bindings` 初始化 V8 時呼叫 rusty_v8 的 `V8::set_entropy_source`，用 `getrandom` 這個 crate 填（`crypto.getRandomValues()` 用的同一個來源；Linux 上是 `getrandom(2)`，在 seccomp 的清單上）；它失敗時回 `false`，V8 照它自己的做法。policy 拿掉 `/dev/urandom`。理由：V8 不再自己讀 `/dev/urandom`，讀不到時也不退到用時間當種子；三個平台同一條路；和 Chromium 一樣：`gin/v8_initializer.cc` 在所有平台上裝 `GenerateEntropy`，用 `base::RandBytes`（Linux 上是 BoringSSL 的 `RAND_bytes`；`details.md` 的「Chromium 的 V8 的亂數」）。不選只在 Linux 上裝（R2：多一條平台分支）。不選固定的 `--random-seed`（R3：每個 renderer 的亂數一樣，`Math.random()` 猜得到）。
4. **時區：使用者沒設 `TZ` 時，Browser 照 `/etc/localtime` 的連結解出 IANA 的名字、設成 renderer 的 `TZ`；解不出來的那個 renderer 照舊放行時區的兩個路徑（T1）**（2026-11-27，使用者決定）：名字取連結目標裡 `zoneinfo/` 之後的部分（`../usr/share/zoneinfo/Asia/Tokyo` 是 `Asia/Tokyo`）；`/etc/localtime` 是一般檔案（有些容器）時解不出來。理由：ICU 拿到 `TZ` 就用它內建的時區資料、不讀檔（`details.md` 的「時區不讀檔」）；大多數機器上 Landlock 一個路徑都不放，解不出來的機器時區也不會錯；改得少。代價：解不出來的機器少這一點收益，第 3 段的 chroot 時要再處理；執行期間改了時區，開著的 renderer 不跟著變（和現在一樣）。不選 Browser 照 ICU 的方法比對檔案的內容（T2：所有機器都不放，但要在 Browser 裡重寫 ICU 的那段、結果要和 ICU 一致）。不選照 Chromium 經過訊息給、renderer 設 ICU 的預設時區（T3：之後能做到時區改了跟著變，但 `bindings` 要能設 ICU 的時區，可能要改 rusty_v8 的 C++、重編 V8；「之後的可能」）。Chromium 的做法在 `details.md` 的「Chromium 的時區」。實作時查到、照這條的原則補上的（2026-11-27）：時區的兩個路徑放不放行由 renderer 照它的 `TZ` 決定，照 ICU 自己的規則：ICU 不收的 `TZ`（`putil.cpp` 的 `isValidOlsonID`：帶數字的大多不收，例如 `AST4ADT`）它會改去讀 `/etc/localtime`，所以放行；ICU 收下的名字是三或四個字（`CET`、`EST`）時，它會拿 glibc 的時差比對（`TimeZone::detectHostTimeZone`），glibc 的時差要讀 zoneinfo 裡那個檔案，讀不到時是 0，`CET` 就安靜地變成沒有時差、沒有夏令時間的時區，所以也放行（`details.md` 的「時區不讀檔」）。
5. **不能用沒有特權的 user namespace 時，退回現在的 Landlock 加 seccomp，Browser 說一次（U1）**（2026-11-27，第 2 段開始時，使用者決定）：能不能用在一個 child 裡試、確認新的 namespace 裡權限都在（Ubuntu 的限制下 `unshare` 會成功、但沒有權限），不行的話那個 child 結束，原本的 process 沒碰過 namespace，照現在的做法繼續。Browser 第一次遇到不行的時在 stderr 說一次為什麼、怎麼打開（Ubuntu 的 `sysctl` 或 AppArmor 的 profile）。原本寫的是由 renderer 在第一則訊息裡說它有沒有進 namespace；做的時候發現試的就是 Browser 自己（第 6 條：namespace 由 Browser 啟動時給），它本來就知道，renderer 不用多一個欄位（2026-11-27）。理由：不會比現在差，Ubuntu 的預設設定上照樣能用，使用者知道少了這兩層；只說一次。Chromium 也是先在 child 裡試（`CanCreateProcessInNewUserNS`）。代價：兩種設定都要測。不選什麼都不說（U2）。不選不啟動、要 `--no-sandbox`（U3：和 Chromium 一樣，但 Ubuntu 的預設設定上就不能用了，使用者加了 `--no-sandbox` 反而連 Landlock 和 seccomp 都沒有）。
6. **只要 user 和 network namespace，不要 PID namespace；Browser 啟動 renderer 時在 `pre_exec` 裡 `unshare`（K5）**（2026-11-27，第 2 段，使用者決定）：一個分頁照舊一個 process，pid 不變。理由：PID namespace 帶來的大多已經有了（碰不到別的 process 是 seccomp，看不到是 Landlock 和第 3 段的 chroot），chroot 要的權限是 user namespace 給的；而它要的東西比預期多：init（renderer 是 PID 1 時 `abort()` 變成 `SIGSEGV`，量過）、外面等著交回結束狀態的 process、trace 和 census 的檔名、量測工具、Browser 另外知道 renderer 外面的 pid；使用者看到 10 個分頁會是 30 個 process（K2）。代價：沒有 PID namespace 這一層，「別的 process」只靠 seccomp（現在就是這樣），chroot 之前 renderer 看得到 pid 的清單；和 Chromium 不同（它三種都用，靠 zygote 吸收那些事）。不選三種都要、Browser 用 `clone` 啟動、renderer 是 PID 1（K3：process 不多，但要自己 `clone` 加 exec、處理 PID 1）。不選 renderer 自己 fork 出 PID 1 的 renderer（K1：兩個 process，`abort()` 被 handler 接走）。不選再加一層 init（K2：量過行得通，但一個分頁三個 process）。PID namespace 留在「之後的可能」。
7. **CI 先不處理（C3）**（2026-11-27，第 2 段，使用者決定）：CI 的 `ubuntu-24.04` runner 照 Ubuntu 的預設多半不允許沒有特權的 user namespace，renderer 在那裡會走退路（第 5 條）。測試照 Browser 試出來的結果檢查（有 namespace 的檢查它在 namespace 裡、沒有的檢查它照現在的做法），兩種設定都會過；namespace 的那條路在不允許的 runner 上只在開發機上測，記在 `docs/test-gaps.md`。理由：使用者決定先不煩惱 CI。不選 CI 跑測試前用 `sysctl` 打開、再在限制下多跑一次 sandbox 的測試（C1：兩條路都在 CI 上測到，要改 workflow）。不選給執行檔寫 AppArmor 的 profile（C2：CI 的執行檔路徑不固定，要 root）。
8. **chroot 的權限：exec 之前只把 `CAP_SYS_CHROOT` 留成 ambient capability（A1）**（2026-11-27，第 3 段開始時，使用者決定）：`pre_exec` 裡（進了 user namespace、寫好對應之後）把它放進 inheritable 和 ambient 兩組，exec 之後 renderer 只有它；`enter` 在確認只有一條 thread 之後 chroot，接著丟掉全部的權限，再進 Landlock 和 seccomp。理由：量過行得通（exec 之後 `CapEff` 只有 `CAP_SYS_CHROOT`，`details.md` 的「第 3 段的探測」）；uid 照舊不是 root；exec 到 chroot 之間只有這一個權限，而且只對 renderer 自己的 namespace 有效。代價：`pre_exec` 多 `capset`、`prctl` 兩個 system call；`enter` 要負責丟掉，由從外面看 `CapEff` 的測試守。不選 uid 對到 0（A2：exec 到 chroot 之間有全部的權限，`getuid()` 是 0）。不選 exec 之前 chroot（A3：之後找不到執行檔和函式庫）。Chromium 的 renderer 從 zygote fork、不 exec，權限自然留著。
9. **chroot 到一條 thread 自己的 `/proc/thread-self/fdinfo`，thread 結束、目錄跟著消失（R1）**（2026-11-27，第 3 段，使用者決定）：`enter` 開一條 thread（thread 共用根目錄），它 `chroot`、`chdir("/")` 之後結束，`enter` 等它結束才繼續，所以之後照樣只有一條 thread。理由：量過行得通；不在檔案系統上建任何東西、沒有要清的；裡面什麼都沒有、也沒有子目錄爬得出去；和 Chromium 的 `ChrootToSafeEmptyDir` 同一個方法（它用 `clone(CLONE_FS)` 的 child）。代價：找不到的錯誤碼是 `ESRCH`、不是 `ENOENT`（Chromium 也是），實作時用整套測試確認沒有程式依賴 `ENOENT`；chroot 那一刻要有 `/proc`。不選在 `/tmp` 建一個空的目錄、chroot 之後刪掉（R2：錯誤碼照常，但每次啟動要寫檔案系統，當掉在中間會留下目錄）。不選系統本來就空的目錄（R3：不是每個發行版都有，保證不了是空的）。
10. **`/etc/localtime` 是一般檔案時，Browser 照 ICU 的方法比對 zoneinfo 的檔案找出名字；找不到的機器不 chroot（Z1）**（2026-11-27，第 3 段，使用者決定）：拿 `/etc/localtime` 的內容和 zoneinfo 裡的檔案比，一樣的那個的名字就是 renderer 的 `TZ`（整個 Browser 找一次、記住；跳過 `posix/`、`right/` 這類重複的）。真的找不到的（自己做的時區檔），那台機器的 renderer 照舊放行時區的兩個路徑、不 chroot。另外 chroot 之前呼叫一次 `tzset()`，glibc 讀好 `TZ` 的時區之後就不再讀檔：短的名字（`CET`）不用再放行時區的檔案，第 4 條的「三四個字的名字」那一條跟著拿掉。理由：找得到名字的機器都 chroot 得成、時區正確；找不到的極少數時區照樣正確，只少 chroot 這一層；Chromium 的 Browser 有 ICU，自己決定時區、把名字給 renderer，這等於在 machopo 的 Browser 裡補上 ICU 找名字的那一段。代價：Browser 要重寫那段比對、結果要和 ICU 的一致（同一個內容有好幾個名字時，挑法不一定和 ICU 一樣，但規則一樣、時間一樣）；那種機器上第一次啟動 renderer 時掃一次 zoneinfo。不選這種機器一律不 chroot（Z2：主要是容器，都少一層）。不選只讓 glibc 先讀好、其餘交給 ICU（Z3：ICU 拿不到名字，會照時差猜出沒有夏令時間的時區，時間安靜地錯）。

## 待決定

第 3 段要定的都定了（第 8 到 10 條）。

## 之後的可能

- PID namespace（第 6 條不做的）：照 Chromium，每個 renderer 是自己的 PID namespace 的 PID 1（裝訊號的 handler），或加一層 init；要處理外面的 pid（`SCM_CREDENTIALS` 量過行得通）、trace 和 census 的檔名、結束狀態的交回（`details.md` 的「第 2 段的探測」）。

- seccomp 的清單再收：namespace 和 chroot 之後，有些 system call（`openat`、`statx`、`readlinkat`、`getdents64`）只剩函式庫自己的退路在用，可以改成讓它失敗。
- 時區在執行期間改了時 Browser 通知 renderer（現在 renderer 啟動時決定一次）：照 Chromium，Browser 把時區的名字經過訊息給、renderer 設 ICU 的預設時區、通知 V8（第 4 條不選的 T3），三個平台一起做。
