# RFC 0120 的細節：Linux 的 renderer sandbox 的下一級

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 探測（第 0 步）

2026-11-27，Fedora 45（kernel 7.3.3，glibc 2.44，x86-64），debug build，`360a4c8f`。

### 這台能不能用沒有特權的 user namespace

- `user.max_user_namespaces` 是 253811；沒有 `kernel.unprivileged_userns_clone`（Debian 的）和 `kernel.apparmor_restrict_unprivileged_userns`（Ubuntu 的）。
- `unshare --user --map-root-user --pid --net --fork --mount-proc sh -c ...`：在裡面 `$$` 是 1，網路介面只有一個（loopback），`/proc` 裡只有 3 個 process。

### machopo 現在的 sandbox 在 namespace 裡還裝不裝得上

sandbox 的 probe（`machopo --type=renderer-sandbox-probe -- <attempt>`）照常跑一次、在 `unshare -Urpn --fork` 裡跑一次：

| attempt | 照常 | user、PID、network namespace 裡 |
| --- | --- | --- |
| `nothing`、`thread nothing` | `ok 0` | `ok 0` |
| `read /etc/passwd`、`list /` | `error 13` | `error 13` |
| `read /etc/localtime`、`read /usr/share/zoneinfo/Asia/Tokyo` | `ok 309` | `ok 309` |
| `read /sys/devices/system/cpu/online` | `ok 5` | `ok 5` |
| `syscall 62 1 0`（`kill(1, 0)`） | `SIGSYS`（1 號不是自己） | `ok 0`（probe 自己就是 1 號） |

Landlock 和 seccomp 在 namespace 裡照樣裝得上、擋的一樣；seccomp 的「只能對自己送訊號」照 `std::process::id()` 算，在 namespace 裡算的是 namespace 裡的 pid，和 kernel 看到的一致。

（第一次量這張表時 `target/debug/machopo` 還是下一節時區的試驗 build 的，那時 policy 暫時拿掉了時區的兩個路徑，所以時區的兩列是 `error 13`；重新 build 之後是上表。另外，probe 的 `read` 讀到檔案的結尾，對 `/dev/urandom` 用它會一直讀下去，被 kill 掉，所以表裡沒有它。）

### namespace 裡的 PID 1 和 `abort()`

一個只呼叫 `abort()` 的 C 程式：

- `unshare -Urpn --fork ./abort1`（它是 namespace 裡的 PID 1）：`Segmentation fault`，exit 139。kernel 對 namespace 的 init 忽略沒有 handler 的訊號，`SIGABRT` 沒有作用，glibc 的 `abort` 退到別的方法，以 `SIGSEGV` 結束。
- `unshare -Urpn --fork sh -c ./abort1`（它是 PID 2，`sh` 是 init）：exit 134（`SIGABRT`），照常。

所以 renderer 前面要有一個 init。renderer 的 `about:crash` 走的是 `abort()`（`renderer` 的 `child_process.rs` 的 `crash`），測試看它的結束是 `SIGABRT`。

### CPU 的數量讀不到時

一個印出 `get_nprocs`、`get_nprocs_conf`、`sysconf(_SC_NPROCESSORS_ONLN)`、`sched_getaffinity` 的數量的 C 程式，在 mount namespace 裡把 `/sys/devices/system/cpu` 蓋成空的 tmpfs、`/proc/stat` 蓋成空檔案：

| | `get_nprocs` | `get_nprocs_conf` | `sysconf` | affinity |
| --- | --- | --- | --- | --- |
| 照常 | 24 | 24 | 24 | 24 |
| 檔案藏起來 | 24 | 24 | 24 | 24 |
| `taskset -c 0-3`，照常 | 24 | 24 | 24 | 4 |
| `taskset -c 0-3`，檔案藏起來 | 4 | 4 | 4 | 4 |

glibc 2.44 讀不到時退回 `sched_getaffinity`：平常相同，process 被限制在部分 CPU 上時變成那幾個。比較舊的 glibc（CI 的 Ubuntu 24.04 是 2.39）的退路沒量。machopo 給 V8 的 worker 數是 `MACHOPO_V8_THREAD_POOL_SIZE`，沒設時是 0（V8 自己決定，經過 `get_nprocs`；`crates/bindings/src/platform.rs`）。

### 時區不讀檔

- 這台的 `/etc/localtime` 是 symlink（`../usr/share/zoneinfo/Asia/Tokyo`）。
- 暫時把 policy 的 `/etc/localtime` 和 `/usr/share/zoneinfo` 拿掉（沒有 commit，量完還原），跑 `renderer_sandbox::a_renderer_process_has_only_the_listed_part_of_its_browsers_environment`（Browser 帶 `TZ=Asia/Tokyo`）和 `renderer_sandbox::a_sandboxed_renderer_shows_a_page_as_one_outside_does`（沒有 `TZ`，系統的時區）：兩個都通過，sandbox 裡的 renderer 看到 `Asia/Tokyo`、-540。
- 所以：有 `TZ` 時 ICU 用它內建的時區資料、不讀檔；沒有 `TZ` 時 ICU 從 `readlink("/etc/localtime")` 解出名字，Landlock 不擋 `readlink`（它只管打開）。chroot 之後 `/etc/localtime` 不在了，兩個之中只剩 `TZ` 行得通。
- 沒試的：`/etc/localtime` 是一般檔案時（ICU 要打開它、和 zoneinfo 裡的檔案比對）。
- 第 1 段做的時候查到的（rusty_v8 帶的 ICU，`putil.cpp`、`timezone.cpp`；2026-11-27）：
  - ICU 收不收 `TZ` 照 `isValidOlsonID`：不含數字和逗號，或最後最多兩個數字（`Etc/GMT+11`、`JST-9`），或 `PST8PDT`、`MST7MDT`、`CST6CDT`、`EST5EDT`；收下時拿掉開頭的 `:` 和 `posix/`、`right/`。不收的（`AST4ADT`、`CST6CDT5,J129,J131/19:30`）它改去讀 `/etc/localtime`。
  - `TimeZone::detectHostTimeZone` 收下名字之後還呼叫 glibc 的 `tzset`、拿 glibc 的時差（`uprv_timezone`），名字是三或四個字、兩邊的時差對不上時，把名字當成有歧義的縮寫丟掉，改用「glibc 的時差、沒有夏令時間」的時區。glibc 的時差要讀 zoneinfo 裡那個檔案（`strace -k`：`detectHostTimeZone` 的 `tzset` 開 `/usr/share/zoneinfo/<TZ>`），sandbox 裡讀不到時是 0：`TZ=CET` 的 renderer 會是 UTC+0、沒有夏令時間；`Asia/Tokyo` 這種長的名字不受影響。所以短的名字照舊放行時區的檔案（`README.md` 的第 4 條）。ICU 把 `CET` 叫做 `Europe/Brussels`。
  - 長的名字時 glibc 自己的時區（`localtime` 那一類）在 sandbox 裡是 UTC：網頁看到的時間都經過 ICU，不受影響。

### V8 的亂數

- `v8/src/base/utils/random-number-generator.cc`（rusty_v8 帶的 V8）：先問 embedder 給的 entropy source；沒有的話 Linux 上 `fopen("/dev/urandom")` 讀 8 個 byte；再不行用時間（註解：「This is weak entropy ... it is the responsibility of the embedder to install an entropy source using v8::V8::SetEntropySource()」）。
- rusty_v8 有 `v8::V8::set_entropy_source`（`src/V8.rs`）；machopo 沒有用它。`getrandom` 在 seccomp 的清單上。

### 用到自己的 pid 的地方（renderer 這一側）

- `shell/src/main.rs`：trace 的檔名是 Browser 的檔名加 `.<pid>`。
- `base/src/census.rs`：census 的檔名帶 pid。
- `shell/src/renderer_sandbox.rs` 的 `linux::policy`：`kill`、`tgkill` 只能對 `std::process::id()`。
- Browser 這一側找 renderer 用的是它啟動的 child 的 pid（CDP 的 `SystemInfo.getProcessInfo`，`bench.py` 照它讀 `/proc/<pid>`），namespace 之後那是外面那一層（init 或等著的那個）的。
- 候選：renderer 經過通道送一次 `SCM_CREDENTIALS`（Unix socket 的 ancillary data），kernel 把裡面的 pid 換成收的一方的 namespace 裡的；沒有試。

## Chromium 的 CPU 的數量

2026-11-27 讀的（`main`）：

- `base/system/sys_info_posix.cc` 的 `SysInfo::NumberOfProcessors()`：一個第一次呼叫時算好的 static（註解：「This value is cached to avoid computing this value in the sandbox, which doesn't work on some platforms」）；先 `sysconf(_SC_NPROCESSORS_CONF)`（註解：在 sandbox 裡呼叫時可能是 -1，「which should never happen」），Linux 上再用 `sched_getaffinity` 數，成功的話就是 affinity 的數量。
- `sandbox/linux/seccomp-bpf-helpers/baseline_policy.cc` 和 `syscall_parameters_restrictions.cc` 的 `RestrictSchedTarget`：renderer 的 seccomp 放行 `sched_getaffinity`，只能問 pid 0 或自己；檔案系統的 system call（`SyscallSets::IsFileSystem`）一律回錯誤，所以進 sandbox 之後讀不到 `/sys`，和有沒有 chroot 無關。
- `content/zygote/zygote_main_linux.cc` 和 `sandbox/policy/linux/sandbox_linux.cc`：進 seccomp 之前先問好的是記憶體（`AmountOfTotalPhysicalMemory`、`AmountOfVirtualMemory`，註解：「Cache the resource limit before turning on the sandbox」），沒有看到先問 CPU 數。
- 沒讀的：V8 在 Chrome 裡怎麼決定 worker 數；有沒有東西在進 sandbox 之前呼叫 `NumberOfProcessors()`。

## Chromium 的 V8 的亂數

2026-11-27 讀的（`main`）：

- `gin/v8_initializer.cc`：`GenerateEntropy(unsigned char* buffer, size_t amount)` 用 `base::RandBytes` 填、回 `true`；`V8Initializer::Initialize` 在 `InitializePlatform` 之後呼叫 `v8::V8::SetEntropySource(&GenerateEntropy)`，外面沒有平台的條件（註解：要早，V8 的虛擬記憶體的 cage 已經用它的亂數）。
- `base/rand_util_posix.cc`：`base::RandBytes` 走 BoringSSL 的 `RAND_bytes`（「Any error aborts the program」）；`getrandom` 的 system call 和預先開好的 `/dev/urandom`（註解：「since we may not even be able to reopen it if we are later put in a sandbox」）只在不配置記憶體的那條路上用。BoringSSL 的 `RAND_bytes` 在 Linux 上用什麼沒讀。
- machopo 的 `crypto.getRandomValues()` 已經用 `getrandom` 這個 crate（`dom_api` 的 `crypto.rs`），在 sandbox 裡的 renderer 上通過測試。

## Chromium 的時區

2026-11-27 讀的（`main`）：

- `base/i18n/icu_util.cc` 的 `InitializeIcuTimeZone`：Linux、ChromeOS、Android 上 Browser 啟動時呼叫 `icu::TimeZone::createDefault()`（註解：「the default time zone cache in ICU has to be populated on starting up」），在任何 renderer 的 sandbox 之前。
- `services/device/time_zone_monitor/time_zone_monitor_linux.cc`：沒有 `TZ` 時監看 `/etc/localtime`、`/etc/timezone`、`/etc/TZ`；變了的話用 ICU 重新決定，和現在的不同才通知（`UpdateIcuAndNotifyClients`）。有 `TZ` 時不監看。
- `third_party/blink/renderer/core/timezone/timezone_controller.cc`：renderer 經過 mojo 的 `TimeZoneMonitor` 登記自己（`AddClient`），收到 `OnTimeZoneChange(timezone_id)` 時 `icu::TimeZone::createTimeZone`、`adoptDefault`，再 `isolate->DateTimeConfigurationChangeNotification()`，也通知 worker、發 `timezonechange` 事件。renderer 一開始的時區是它自己在 constructor 裡 `createDefault()` 的（`kLazyBlinkTimezoneInit` 預設關著）。
- 沒讀的：那個 `createDefault()` 是不是在 renderer 進 sandbox 之前（renderer 是從 zygote fork 的，zygote 進 sandbox 之前初始化了 ICU，可能直接繼承）；`AddClient` 時會不會先送一次目前的時區。

## 第 2 段的探測

### 自己開 namespace、init、外面的 pid、crash 的狀態（量的）

2026-11-27，同一台。`nsinit.c`（這個資料夾）：一個 process（Browser）fork 出 child（Browser 啟動的 renderer process）；child 在只有一條 thread 時 `unshare(CLONE_NEWUSER | CLONE_NEWNET | CLONE_NEWPID)`，拒絕 `setgroups`、寫 `uid_map`、`gid_map`（自己的 uid、gid 對到 0），fork 出 init（新的 PID namespace 的 PID 1），init 再 fork 出「renderer」；renderer 經過 Browser 給的 socket pair 送一次 `SCM_CREDENTIALS`（寫的是自己在 namespace 裡的 pid）。init 等 renderer，把它的結束編成 exit code（訊號是 128 加訊號），child 等 init，是訊號的話用同一個訊號結束自己（child 不在新的 PID namespace 裡，不是 PID 1，訊號照常有作用）。

- 沒有特權也做得到，renderer 在裡面是 PID 2。
- Browser 那一端設了 `SO_PASSCRED`，收到的 `SCM_CREDENTIALS` 的 pid 是 kernel 換成 Browser 的 namespace 的（例如 3582438）；`/proc/3582438/status` 的 `NSpid` 是 `3582438	2`。
- renderer `_exit(7)`：Browser 看到 child 以 7 結束；renderer `abort()`：Browser 看到 child 被訊號 6（`SIGABRT`）結束，和現在 renderer 直接是 Browser 的 child 時一樣。
- 沒試的：Landlock 和 seccomp 接在這之後（第 0 步用 `unshare -Urpn` 試過，裝得上）；丟掉權限；Browser 從 child 的 pid 找 renderer 的 pid 的其他方法（`/proc/<pid>/task/<pid>/children`）。

### 多花的時間（量的）

2026-11-27，同一台：Python 啟動 200 次取中位數：`/usr/bin/true` 0.30 ms，`unshare /usr/bin/true`（不開 namespace）0.61 ms，`unshare --user --net --map-current-user /usr/bin/true` 1.08 ms：開 user 和 network namespace、寫對應多 0.48 ms。

### 不允許 user namespace 的情況（造出來的）

一個 user namespace 裡把 `/proc/sys/user/max_user_namespaces` 寫成 0，裡面的 process 再開 user namespace 就失敗（`ENOSPC`）：`unshare -Ur sh -c 'echo 0 > /proc/sys/user/max_user_namespaces && unshare -U true'`。在這裡面跑 `renderer_sandbox::` 的測試，走的是退路，都通過；`a_renderer_process_goes_without_namespaces_where_the_machine_forbids_them` 照這個方法自己造（外面允許 user namespace 時）。

### Chromium 的 namespace sandbox

2026-11-27 讀的（`main`）：

- `sandbox/linux/services/namespace_sandbox.cc`：預設的 namespace 是 `CLONE_NEWUSER | CLONE_NEWPID | CLONE_NEWNET`，user namespace 是必要的（`CHECK`）；clone 在 `base::LaunchProcess` 裡做（`launch_options.clone_flags`）；uid、gid 的對應在 exec 之前的 delegate 裡寫（先拒絕 `setgroups`）。`ForkInNewPidNamespace` 用 `CLONE_NEWPID` fork，child 是 PID 1。`InstallDefaultTerminationSignalHandlers` 給 `SIGHUP`、`SIGINT`、`SIGABRT`、`SIGQUIT`、`SIGPIPE`、`SIGTERM`、`SIGUSR1`、`SIGUSR2` 裝 handler，收到時 `_exit` 一個代表那個訊號的 exit code。
- `content/zygote/zygote_linux.cc`：有 PID 和 user namespace 時，每個 renderer 用 `ForkInNewPidNamespace` fork，各自是自己的 PID namespace 的 PID 1，裝上面那些 handler（註解：「If the process is the init process inside a PID namespace, it must have explicit signal handlers」）。renderer 對「PID oracle」的 socket ping 一次，讓 Browser 找到它真的 pid（Browser 那一側沒讀）。zygote 收屍（`waitpid`，2 秒後 `SIGKILL`），把結束的狀態和 exit code 回報給 Browser，Browser 照 exit code 換回「被殺掉」。zygote 自己是 zygote 的 PID namespace 的 PID 1。
- `sandbox/linux/services/credentials.cc`：`MoveToNewUserNS` 是在呼叫的 process 裡 `unshare(CLONE_NEWUSER)`、寫對應；`ChrootToSafeEmptyDir`：一個共用檔案系統資訊（`CLONE_FS`）的 child chroot 到自己的 `/proc/self/fdinfo/` 再結束，那個目錄跟著消失，裡面沒有子目錄、出不去；之後確認 `/proc` 看不到了、沒有開著的目錄。`DropAllCapabilities` 要只有一條 thread。
- `sandbox/policy/linux/sandbox_linux.cc` 的 `InitializeSandbox`：只有一條 thread 的檢查、先開好 `/proc`，namespace 的那一套（user namespace、chroot、丟掉權限；zygote 留 `CAP_SYS_ADMIN`）、限制記憶體（`RLIMIT_DATA`）、seccomp-bpf，照這個順序。要求 namespace 但做不到時 `CHECK` 失敗；退到 setuid 的 helper 是在啟動的地方決定的，沒讀到。

## 第 3 段的探測

2026-11-27，同一台。

### 進了 sandbox 之後還碰的路徑（量的）

`strace -f -e trace=%file,landlock_restrict_self,clone,clone3` 跟著 `renderer_sandbox::` 的四個會載入頁面的測試（7 個 renderer process），只列 `landlock_restrict_self` 之後的：

| 路徑 | 結果 | 誰（推測） |
| --- | --- | --- |
| `""`（`statx` 自己開著的 fd） | 成功，340 次 | 標準庫的 `metadata` |
| `/proc/stat`、`/sys/devices/system/cpu/online` | `EACCES`，各 12 次 | glibc 的 `get_nprocs`（退回 `sched_getaffinity`） |
| `/proc/self/cgroup` | `EACCES` | CPU 數的另一條路 |
| `/sys/devices/system/cpu/cpu0/tsc_freq_khz`、`/sys/devices/system/node/node1` | `ENOENT` | 本來就沒有 |
| `/usr/share/zoneinfo/America/New_York`、`/usr/share/zoneinfo/Asia/Tokyo` | `EACCES` | glibc 的 `tzset`（長的名字：ICU 不看 glibc 的時差） |
| `/usr/share/zoneinfo/CET` | 成功 | glibc 的 `tzset`（短的名字：第 4 條放行的） |

chroot 之後會壞的只有最後一個：短的名字。chroot 之前呼叫一次 `tzset()`，glibc 讀好之後 `TZ` 沒變就不再讀檔；`/etc/localtime` 是一般檔案的機器另外處理。

### chroot 的權限和一個會消失的目錄（量的）

`chroot.c`（這個資料夾）：child 進 user 和 network namespace、對應到自己，把 `CAP_SYS_CHROOT` 放進 inheritable 和 ambient，exec 自己；exec 之後一條 thread `chroot("/proc/thread-self/fdinfo")`、`chdir("/")`、結束，main thread 丟掉全部的權限：

- exec 之後：`CapInh`、`CapPrm`、`CapEff`、`CapAmb` 都是 `0000000000040000`（只有 `CAP_SYS_CHROOT`）。
- chroot 之後：`stat("/etc")`、`stat("/proc")`、`opendir("/")`、再一次 `chroot("/")` 都是 `ESRCH`（根目錄是那條已經結束的 thread 的目錄），`getcwd` 是 `/`。找不到的錯誤是 `ESRCH`，不是 `ENOENT`。
- sunaba 的 `enter` 讀 `/proc/self/status` 確認只有一條 thread：chroot 要在它之後。

## Ubuntu 和 CI

2026-11-27 搜尋的：

- Ubuntu 23.10 起用 AppArmor 限制沒有特權的 user namespace，24.04 LTS 也是：沒有被限制、而且 AppArmor 的 profile 裡有 `userns` 規則的 process 才開得了。Ubuntu 給 Chrome 的預設安裝路徑（`/opt/google/chrome/chrome`）寫了 profile；裝在別處的 Chrome、Firefox 的 nightly 這類就起不來（「No usable sandbox」、「failed to find unprivileged_userns profile」）。關掉的方法是 `kernel.apparmor_restrict_unprivileged_userns=0`，Chromium 的文件說它關掉了 Ubuntu 的一項保護，建議改寫一份 profile 給那個執行檔。
- machopo 的 CI 都在 GitHub 的 `ubuntu-24.04` runner 上（`.github/workflows/`）；runner 的映像檔有沒有改這個設定沒有查。
- Debian 的 `kernel.unprivileged_userns_clone` 的預設值沒查到。

## 沒有查到的

- Chromium 的 namespace sandbox 和 chroot 的原始碼（`sandbox/linux/services/namespace_sandbox.cc`、`credentials.cc` 一帶）：第 2 段開始前讀。
- Firefox 的 Linux content sandbox 用不用 user namespace、chroot。
- `SCM_CREDENTIALS` 的 pid 換算在 machopo 的通道上行不行得通。

參考：

- [AppArmor User Namespace Restrictions vs. Chromium Developer Builds](https://chromium.googlesource.com/chromium/src/+/main/docs/security/apparmor-userns-restrictions.md)
- [Ubuntu 23.10 restricted unprivileged user namespaces](https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces)
- [Understanding AppArmor User Namespace Restriction](https://discourse.ubuntu.com/t/understanding-apparmor-user-namespace-restriction/58007)
- [unprivileged_userns profile interferes with Chromium sandbox（AppArmor #394）](https://gitlab.com/apparmor/apparmor/-/issues/394)
- [Mozilla bug 1884347](https://bugzilla.mozilla.org/show_bug.cgi?id=1884347)
