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
- 沒試的：`/etc/localtime` 是一般檔案時（ICU 要打開它、和 zoneinfo 裡的檔案比對）；`TZ` 是 POSIX 的格式（`JST-9`）或 `:` 開頭的路徑時 ICU 怎麼解。

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
