# RFC 0117 的細節：renderer process 關進作業系統的 sandbox

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## Linux 的探測

2026-11-26，`52dc5adc`，Fedora 45（kernel 7.3.3，x86-64），debug build。做法：`probe.py`（這個資料夾）用 `strace -f` 啟動 `machopo --automation-pipe`，經過 pipe 載入一頁 `data:` 的文件，等 4 秒、讀標題、截一張圖、`Browser.close`；只統計 renderer process（`execve` 帶 `--type=renderer` 的那個 pid 和它 `clone3` 出來的 thread，從 `execve` 起算）。跑兩次：系統字型、`--test-fonts`。

測試頁：`serif`、`sans-serif`、`monospace`、`system-ui` 的文字（中日韓、阿拉伯文、希伯來文、泰文、emoji：要 fallback）、粗斜體、`<input>` 和 `<button>`、canvas 的 `fillText`、`srcdoc` 的 iframe、一段三百萬次的迴圈（讓 V8 編譯）、`Intl.DateTimeFormat` 的時區和 `Date`、`toLocaleString`、一個空的 `WebAssembly` 模組、`crypto.getRandomValues`、一個 `Worker`。標題讀到 `Asia/Tokyo`、`en-US`。

**renderer process**：22 個 thread（main、`Chrome_ChildIOThread`、`Compositor`、V8 的 worker、raster、`CanvasRaster`、`DedicatedWorker`……）。

**路徑**（`open`、`openat`、`stat` 類、`access`、`readlink`、`getdents64`）：

| 種類 | 系統字型 | `--test-fonts` | 說明 |
| --- | --- | --- | --- |
| 執行檔 | 1 | 1 | `execve` 和 `/proc/self/exe` 的 `readlink` |
| loader | 2 | 2 | `/etc/ld.so.cache`、`/etc/ld.so.preload`（沒有） |
| 共用函式庫 | 15 | 15 | `libfontconfig`、`libfreetype`、`libxml2`、`libharfbuzz`、`libglib-2.0`、`libgraphite2`、`libpng16`、`libbrotli*`、`libpcre2-8`、`libz`、`libbz2`、`libm`、`libc`、`libgcc_s`；都在 `main` 之前 |
| `/proc` | 3 | 3 | `/proc/sys/vm/overcommit_memory`、`/proc/self/maps`、`/proc/self/exe` |
| `/sys` | 4 | 4 | `/sys/kernel/mm/transparent_hugepage/enabled`、`/sys/devices/system/cpu/online`、`/sys/devices/system/node/node1`（沒有）、`.../cpu0/tsc_freq_khz`（沒有） |
| `/dev` | 1 | 1 | `/dev/urandom` |
| 時區 | 4 | 4 | `/etc/localtime`（`openat` 和 `readlink`）、`/usr/share/zoneinfo/Asia/Tokyo` 一路的 `readlink` |
| fontconfig 的設定 | 274 | 0 | `/etc/fonts/fonts.conf`、`/etc/fonts/conf.d/*`、`/usr/share/fontconfig/conf.avail/*` |
| fontconfig 的 cache | 約 75 | 0 | `/usr/lib/fontconfig/cache/*`、`~/.cache/fontconfig/*`、`~/.fontconfig/*`（沒有） |
| 字型的目錄和檔案 | 約 165 | 0 | `/usr/share/fonts/**`、`~/.local/share/fonts/*`；`~/.fonts`、`/usr/local/share/fonts`、flatpak 的字型目錄（都沒有） |
| 使用者的 fontconfig 設定 | 2 | 0 | `~/.config/fontconfig/fonts.conf`、`conf.d`（都沒有） |
| 合計 | 580 | 32 | |

沒有任何一個是寫的（沒有 `O_WRONLY`、`O_RDWR`、`O_CREAT`，沒有 `unlink`、`mkdir`、`rename`）：fontconfig 的 cache 是新的，沒有重建。cache 過期時 `libfontconfig` 會不會在 renderer 裡重寫 `~/.cache/fontconfig`，要另外試。

**沒有出現的**：`socket`、`connect`、`bind`、`socketpair`、`memfd_create`、`fork`、`vfork`（`execve` 只有它自己的那一次）、`ptrace`、`kill`、`io_uring_*`、`bpf`、`perf_event_open`、`setuid` 類、`mount`、`chroot`、`unshare`。

**system call**（次數；4 秒、一頁）：

| system call | 系統字型 | `--test-fonts` |
| --- | --- | --- |
| `mprotect` | 581 | 435 |
| `openat` | 567 | 37 |
| `access` | 550 | 2 |
| `close` | 451 | 57 |
| `mmap` | 394 | 182 |
| `read` | 351 | 47 |
| `readlink` | 280 | 7 |
| `munmap` | 267 | 57 |
| `newfstatat` | 217 | 16 |
| `fcntl` | 213 | 36 |
| `statx` | 177 | 0 |
| `futex` | 168 | 151 |
| `rt_sigprocmask` | 113 | 113 |
| `sched_yield` | 91 | 69 |
| `fstat` | 85 | 49 |
| `madvise` | 85 | 89 |
| `fstatfs` | 68 | 0 |
| `sendmsg` | 61 | 51 |
| `gettid` | 53 | 53 |
| `epoll_pwait` | 51 | 42 |
| `recvmsg` | 50 | 41 |
| `prctl` | 35 | 35 |
| `write` | 27 | 27 |
| `set_robust_list` | 22 | 22 |
| `rseq` | 22 | 22 |
| `clone3` | 21 | 21 |
| `lseek` | 18 | 18 |
| `sigaltstack` | 17 | 17 |
| `getrandom` | 14 | 13 |
| `ioctl` | 14 | 14 |
| `pkey_mprotect` | 11 | 11 |
| `sched_getaffinity` | 9 | 9 |
| `rt_sigaction` | 6 | 6 |
| `getpid` | 6 | 6 |
| `exit` | 5 | 5 |
| `getdents64` | 4 | 0 |
| `brk` | 3 | 3 |
| `sched_getparam` | 3 | 3 |
| `sched_getscheduler` | 3 | 3 |
| `prlimit64` | 2 | 2 |
| `open` | 2 | 2 |
| `getsockopt` | 2 | 2 |
| `epoll_ctl` | 2 | 2 |
| `clock_nanosleep` | 2 | 2 |
| `execve` | 1 | 1 |
| `arch_prctl` | 1 | 1 |
| `set_tid_address` | 1 | 1 |
| `sysinfo` | 1 | 1 |
| `poll` | 1 | 1 |
| `epoll_create1` | 1 | 1 |
| `eventfd2` | 1 | 1 |
| `uname` | 1 | 1 |
| `pkey_alloc` | 1 | 1 |
| `shutdown` | 1 | 1 |
| `exit_group` | 1 | 1 |

只在系統字型時出現：`statx`、`getdents64`、`fstatfs`（fontconfig 掃目錄）。

**值得記的**：

- V8 的 JIT：`pkey_alloc(0, PKEY_DISABLE_WRITE)`，可執行的區域是 `pkey_mprotect(..., PROT_READ|PROT_WRITE|PROT_EXEC, 1)`；V8 的幾張表（`v8-pointer-table`）是 `pkey_mprotect(..., PROT_NONE, 1)`。`mmap` 帶 `PROT_EXEC` 的只有 loader 載入函式庫的 15 次。seccomp 要放行 `pkey_alloc`、`pkey_mprotect` 和帶 `PROT_EXEC` 的 `mprotect`。
- `prctl`：`PR_SET_NAME`（thread 的名字）、`PR_SET_VMA_ANON_NAME`（mimalloc、absl、V8 給區域取名，這個 kernel 回 `EINVAL`）、`PR_GET_AUXV`。
- `ioctl(5, TCGETS2)`：對 stderr 問是不是 terminal（`EINVAL`：是檔案）。
- 控制通道：`sendmsg`、`recvmsg`、`epoll_pwait`、`eventfd2`，結束時 `shutdown`。
- `madvise` 的 `MADV_GUARD_INSTALL`（thread 的 stack 的 guard page，kernel 6.13 起）、`MADV_DONTFORK`、`MADV_HUGEPAGE`、`MADV_DODUMP`。

**沒走到的**：wasm 的文件（`dom_wasm`、wasmtime 的 JIT）、`MACHOPO_TRACE`（renderer 寫自己的 trace 檔）、`census` 的 build、crash（`about:crash`、panic 的訊息）、很多 worker、記憶體的壓力（GC 還記憶體給 kernel 的路）、列印和下載（現在沒有）。第 1 段開始前補。

### 零星的讀取是誰讀的

2026-11-26，`strace -f -k`（帶呼叫的堆疊）只看這幾個路徑的 `open`、`openat`、`readlink`：

| 路徑 | 誰讀的 | 什麼時候 | 第 1 段怎麼辦（看法） |
| --- | --- | --- | --- |
| `/proc/sys/vm/overcommit_memory`、`/sys/kernel/mm/transparent_hugepage/enabled` | mimalloc 的初始化（`_mi_prim_mem_init`） | `main` 之前（`__libc_start_main` 裡） | 鎖之前就讀完了，不用放行 |
| `/proc/sys/vm/overcommit_memory`（第二次） | glibc 的 malloc 在 thread 結束時要不要縮 heap（`_int_free_maybe_trim`） | 鎖之後 | 讓它失敗（glibc 讀不到就照預設） |
| `/proc/self/maps` | Rust 的 runtime 找 main thread 的 stack（`std::rt::lang_start_internal` 的 `pthread_getattr_np`） | `main` 之前 | 不用放行 |
| `/sys/devices/system/cpu/online` | glibc 的 `get_nprocs`：V8 的 platform 和 absl 問 CPU 數、glibc 的 malloc 建 arena | 鎖之後，好幾個 thread | 唯讀放行這一個檔案（讀不到時 glibc 的退路和 V8 拿到的數字要另外確認） |
| `/dev/urandom` | V8 的亂數（`v8::base::RandomNumberGenerator` 用 `fopen`） | 鎖之後 | 唯讀放行（讀不到時 V8 用弱的種子），或改給 V8 一個用 `getrandom` 的 entropy source |
| `/etc/localtime` | 這一輪沒載入頁面，沒走到（上一輪：ICU 的時區） | 鎖之後 | 唯讀放行它和 zoneinfo，第 1 段再看一次堆疊 |
| `/proc/self/exe` | Browser 啟動 renderer 時問自己的路徑（`std::env::current_exe`），不是 renderer | — | — |

### fontconfig 的 cache 不在時

2026-11-26，`XDG_CACHE_HOME` 指到空的目錄再跑一次系統字型的探測。**renderer process 寫了檔案**：`libfontconfig` 發現使用者的字型目錄（`~/.local/share/fonts`）沒有 cache，就在 cache 目錄建 `<hash>-le64.cache-12.TMP-*`、`.NEW`、`rename` 成 cache 檔、建和刪 `.LCK`、刪舊版的 cache（`.cache-9` 到 `-11`），還試了 `chmod` 系統的 cache 目錄（`EPERM`）和 `unlink` 字型目錄裡的 `.uuid`（沒有）。Browser 自己也用 fontconfig（UI 的文字），它的 process 也寫了同一批。

對設計的意思：

- sandbox 裡這些寫入會失敗。`libfontconfig` 寫不了 cache 時照理是自己在記憶體裡掃一遍、照樣能用（要在 1b 用真的 sandbox 確認），但每個 renderer 啟動都要重掃。
- 所以要 Browser 先把 cache 建好：Browser 啟動 renderer 之前本來就要問 fontconfig 的路徑（已決定的事項第 3 條），那時它初始化 fontconfig、cache 就寫好了；renderer 進來時 cache 是新的，不會想寫。
- seccomp 的清單要讓這幾個檔案的呼叫（`unlink`、`rename`、`chmod`、`mkdir`、可寫的 `openat`）回錯誤而不是當掉：cache 剛好過期時 `libfontconfig` 會叫它們（Chromium 也是檔案系統的呼叫一律回 `EPERM`）。Landlock 擋的本來就是回錯誤，seccomp 放行這幾個讓 Landlock 擋就可以。

### wasm 的文件、trace、crash

2026-11-26，`--test-fonts`，和第一次的 `--test-fonts` 比多出來的（`PROBE_URL` 換頁面）：

| 跑法 | 多的 system call | 多的路徑 | 說明 |
| --- | --- | --- | --- |
| wasm 的文件（`dom_wasm` 的 `page.wasm`，`file:` 的頁面） | `memfd_create`、`statx` | `/proc/self/cgroup`、`/sys/fs/cgroup/.../cpu.max` 一路往上 | wasmtime 建 `memfd_create("wasm-memory-image", MFD_CLOEXEC\|MFD_ALLOW_SEALING)`（記憶體的初始映像）；JIT 的 code 是 `mprotect(PROT_READ\|PROT_EXEC)`（先寫再改成可執行，不是 V8 的 pkey）；cgroup 的是問可以用幾個 CPU（Rust 的 `available_parallelism`，讀不到時有退路） |
| `MACHOPO_TRACE` | `writev`、`dup`、`geteuid`、`fdatasync` | renderer 自己建 `<檔名>.<pid>`（`O_WRONLY\|O_CREAT\|O_TRUNC`）；`/proc/self/stat`、`/proc/self/cmdline`、`/sys/devices/system/cpu/possible` | 那個檔案就是已決定的事項第 6 條要改成 Browser 開的；其餘是 trace 的 metadata |
| `about:crash` | `tgkill` | 無 | `tgkill(自己的 pid, 自己的 tid, SIGABRT)`：`abort`。seccomp 要許對自己的；`SIGSEGV` 的 handler 是 V8 裝的（`rt_sigaction`） |

「只有 Browser 建共用記憶體」沒有被打破：renderer 的 `memfd_create` 是 wasmtime 自己用的匿名記憶體，不交給別人（`machopo-region` 的都是 Browser 的 process 建的）。

### kernel 和 CI

2026-11-26（`man7.org` 的 `landlock(7)`，man-pages 6.19；GitHub 的 runner image 的說明）：

- Landlock 的 ABI 和 kernel：1 是 5.13、2 是 5.19、3 是 6.2、4 是 6.7、5 是 6.10、6 是 6.12、7 是 6.15、8 是 7.0、9 是 7.1（man page 列到 9；開發機的 7.3.3 回 11）。man page：「Users should use the Landlock ABI version rather than the kernel version」。
- 要用得到：kernel 編進去（`CONFIG_SECURITY_LANDLOCK`）而且開機時在 LSM 的清單裡（`CONFIG_LSM` 或 `lsm=`）。問 ABI 的那個呼叫失敗時：kernel 文件的範例把 `ENOSYS` 說成「not supported by the current kernel」、`EOPNOTSUPP` 說成「currently disabled」。
- CI 是 GitHub 的 `ubuntu-24.04`：image 的說明寫 kernel 7.0.0-1017-azure（ABI 8）。有沒有開著沒寫：1b 在 CI 上跑一次問 ABI 的呼叫確認。
- 各發行版預設開不開：查的幾頁都沒寫。第 1 段不靠這個（已決定的事項第 4 條：沒有就不跑、說原因），做完時在 `docs/limitations.md` 寫最低要求。

## macOS 的探測

2026-11-26，macOS 26.6.2（Apple M4），debug build。`probe_macos.py`（同一個資料夾）：`machopo --remote-debugging-pipe --headless` 載入和 Linux 的探測同一頁，同時從系統的 log 讀 kernel 的 sandbox 回報（`log stream --predicate 'sender == "Sandbox"'`），只算 renderer process 的。兩種跑法：

- **什麼都不擋、全部回報**：整個 machopo 在 `sandbox-exec` 的 `(allow (with report) default)` 底下跑（renderer 繼承），log 裡是 renderer 做的每一種操作。
- **renderer 自己的 profile**：log 裡是被擋的。

kernel 對同一個 process、同一種操作、同一個對象短時間內只回報一次，所以數的是「幾種」，不是「幾次」。sandbox 管的是種類（檔案、Mach 服務的查詢、sysctl、IPC 的物件、訊號、process），不是 system call：這裡看不到 renderer 做了哪些 system call（`dtruss` 要關 SIP，沒做）。

### 什麼都不擋時 renderer 做了什麼

| 種類 | 對象 |
| --- | --- |
| 讀檔案的內容（532 個路徑） | 字型檔 519 個：`/System/Library/Fonts`（372）、`/System/Library/AssetsV2/com_apple_MobileAsset_Font8/<雜湊>.asset/AssetData/`（128 個下載的字型，每個一個目錄）、`~/Library/Fonts`（18）、`FontServices.framework` 裡的 `PingFangUI.ttc`；時區（`/private/var/db/timezone/tz/<版本>/zoneinfo/Asia/Tokyo`、`posixrules`）；`/usr/share/icu/icudt78l.dat`；自己的執行檔；`/dev/dtracehelper`；`/Library/Preferences/Logging/com.apple.diagnosticd.filter.plist`；dyld 的 cache |
| 讀 metadata（560 個路徑） | 上面那些，加上它們路徑上的每一層目錄（`/`、`/System`、`/Users`、`/Users/<使用者>`……）和 `/etc`、`/var`、`/private/etc/localtime` |
| 查 Mach 服務（9 個） | Browser 的 rendezvous（`org.machopo.MachPortRendezvousServer.<pid>`）、`com.apple.fonts`、`com.apple.cfprefsd.agent`、`com.apple.cfprefsd.daemon`、`com.apple.logd`、`com.apple.diagnosticd`、`com.apple.system.notification_center`、`com.apple.distributed_notifications@Uv3`、`com.apple.system.opendirectoryd.libinfo` |
| 讀 sysctl（13 個） | `hw.ncpu`、`hw.memsize`、`hw.pagesize_compat`、`hw.optional.*` 四個、`kern.osproductversion`、`kern.osvariant_status`、`kern.iossupportversion`、`kern.bootargs`、`machdep.cpu.brand_string`、`security.mac.lockdown_mode_state` |
| 讀 POSIX 的共用記憶體（3 個） | `apple.cfprefs.501v1`、`apple.cfprefs.daemonv1`、`apple.shm.notification_center`（偏好設定和通知中心的） |
| 寫檔案、`ioctl` | 只有 `/dev/dtracehelper`（dyld 在 `main` 之前向 DTrace 登記） |

沒有：建檔案、寫檔案、socket、連線、啟動程式、對其他 process 送訊號、自己建共用記憶體、IOKit。和 Linux 的探測一致。其中一部分發生在 `main` 之前（dyld、libSystem 的初始化：`kern.bootargs`、`hw.memsize`、`/dev/dtracehelper`、dyld 的 cache），renderer 自己鎖的時候已經過了，profile 不用放行。

### 最小的 profile

從只放行 rendezvous 開始一樣一樣加，每次拿頁面的截圖和沒有 sandbox 的比（沒有 sandbox 的兩次彼此逐 byte 相同）：

| 放行 | 少了它 |
| --- | --- |
| 查 Browser 的 rendezvous | renderer 拿不到通道，結束 |
| 讀字型檔的目錄 | 文字用錯字型（系統字型列得到、檔案讀不到） |
| 查 `com.apple.fonts` | 頁面畫得出來，但 CJK 的 fallback 選到別的字型（字重不同）：CoreText 的 `CTFontCreateForString` 要問它 |
| `/etc`、`/var` 的 metadata，讀 `/private/etc/localtime` 和 `/private/var/db/timezone` | 時區變成 UTC（`/etc` 是 symlink，解不開） |
| sysctl：`hw.ncpu`、`hw.optional.*` | 探測的頁面看不出差別；CPU 的數目決定 thread 的數目，`hw.optional.*` 是 CPU 的功能（JIT 和標準庫照它選指令） |
| thread 的名字（`process-info-setcontrol`，對自己） | 頁面不變；從外面看（`sample`、debugger）thread 都沒有名字 |

這樣的 profile 底下截圖和沒有 sandbox 時逐 byte 相同，還被擋的（都不影響結果，不放行，已決定的事項第 20 條）：

- 查 `com.apple.diagnosticd`、`com.apple.distributed_notifications@Uv3`、`com.apple.system.opendirectoryd.libinfo`；`cfprefsd` 的兩個沒有再出現（Core Foundation 先問 sandbox 許不許，不許就不查）。
- 讀 `/Library/Preferences/.GlobalPreferences.plist`、`/private/etc/passwd`（問家目錄，7 次）、`com.apple.diagnosticd.filter.plist`、自己的執行檔所在的目錄。
- metadata：`/`、`/System`、`/System/Cryptexes/App`、`/System/Cryptexes/OS`、`/System/Volumes/Data`、`/Users/<使用者>`、`/private`。
- `process-info-pidinfo`（自己）、sysctl `kern.proc.pid.<自己>`、`kern.osproductversion`；`/usr/share/icu/icudt78l.dat`（系統的 ICU 的資料檔，Core Foundation 的）。
- 對自己送訊號：不用放行，`abort` 照樣以 `SIGABRT` 結束。

最後三樣（ICU 的資料檔、系統的版本、對自己送訊號）原本照「看到 renderer 做了」放行，mutation test 顯示拿掉也沒有測試失敗，截圖也不變，就拿掉了。

試過、沒有用的：放行 `cfprefsd`、使用者的偏好設定檔、`/etc/passwd`、`opendirectoryd`、整個檔案系統的 metadata，各自都不能取代 `com.apple.fonts`（截圖還是不同）。

### wasm 的文件、trace、crash

同一份 profile，`PROBE_URL` 換頁面：

| 頁面 | 結果 | 多被擋的 |
| --- | --- | --- |
| wasm 的文件（`dom_wasm` 的 `page.wasm`） | 截圖和沒有 sandbox 時相同 | 沒有：wasmtime 的 JIT（`MAP_JIT`、`mprotect`）不經過 sandbox 管的種類 |
| `about:crash` | Browser 照 renderer 當掉處理 | 沒有 |
| `MACHOPO_TRACE` | renderer 的 trace 寫出來（檔案是鎖之前開的） | sysctl `hw.machine`、`hw.memsize`、`kern.hostname`、`kern.osrelease`、`kern.ostype`、`kern.version`（Perfetto 的 `uname`：stderr 一行「Unable to read Utsname information」，trace 少那幾欄） |

### Seatbelt 的行為（實測）

- **被擋的呼叫回 `EPERM`**（檔案、socket、`fork`、`posix_spawn`、`kill`、`sysctl`、`shm_open`）；查 Mach 服務是 launchd 回 1100（`BOOTSTRAP_NOT_PRIVILEGED`），放行但沒有那個服務是 1102。process 不會被結束。
- **不存在的路徑回 `ENOENT`、被擋的回 `EPERM`**：擋 metadata 擋不住「路徑在不在」。
- **`(deny default)` 之後還允許的**：問同一個使用者的其他 process 的資料（`proc_pidinfo`）；要明寫 `(deny process-info*)` 才擋。Chromium 的 `renderer.sb` 另外列了 `iokit-get-properties`、`nvram*`、`distributed-notification-post`（沒有各自實測）。
- **進去之後再套一份**：一模一樣的 profile 成功（沒有效果）；不同的被拒絕（`EPERM`），不管是比較鬆還是比較緊。
- **參數**（`(param "NAME")`）的值是字串：裡面的括號和引號不會變成規則。
- **整個 process 的**：鎖之前就在跑的 thread 也在裡面。
- **`sandbox_check(pid, NULL, 0)`**：從外面問一個 process 在不在 sandbox 裡（測試用它看真的 renderer）；pid 不存在時也回「在」。
- symlink：規則比的是解開之後的路徑（`/etc/localtime` 要寫 `/private/etc/localtime`），路徑上的 symlink 本身要 metadata（`/etc`）。

## Linux 的機制的比較

2026-11-26 對過 kernel 的文件（`docs.kernel.org/userspace-api/landlock.html`，7.4-rc4 的版本）；seccomp 和 namespace 的版本是照記憶寫的：

| | seccomp-bpf | Landlock | namespace |
| --- | --- | --- | --- |
| 管什麼 | system call 的編號和數值的參數 | 檔案（照路徑的階層）、TCP 和 UDP 的 port、abstract unix socket 和訊號的範圍、ptrace 的方向 | 看得到的檔案系統、process、網路介面、使用者 |
| 誰套 | process 自己（`PR_SET_NO_NEW_PRIVS` 之後） | process 自己（同上；文件：「For unprivileged processes, setting the no_new_privs attribute is required」） | 建 process 的一方（`clone`、`unshare`） |
| 要特權嗎 | 不用 | 不用 | user namespace 不用，但有的發行版關掉沒有特權的 |
| 看不到路徑 | 是（指標指到的字串讀不了） | 不是 | 不適用 |
| 套了之後 | 新的 thread 繼承；可以再疊更緊的 | 套的那個 thread 和它之後建的 thread、child 繼承；已經在跑的其他 thread 不會（ABI 8 起有對全部 thread 套的旗標）；拿不掉 | 不能再放寬 |

Landlock 的 ABI（文件裡每一版拿掉的限制；kernel 的版本在上面的「kernel 和 CI」）：1 檔案；2 `REFER`；3 `TRUNCATE`；4 TCP 的 bind 和 connect；5 裝置的 `ioctl`；6 範圍（`LANDLOCK_SCOPE_SIGNAL`：只能對同一個或更裡面的 domain 的 process 送訊號；`LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET`）；7 紀錄的旗標；8 對全部 thread 套；9 有路徑的 unix socket（`RESOLVE_UNIX`）；10 UDP；11 `restrict_self` 順便設 `no_new_privs` 的旗標；12 capability 和 namespace 的規則。**開發機（kernel 7.3.3）回的是 ABI 11**。

Landlock 管不到的（文件）：`stat`、`access`、`chdir`、`chmod`、`chown`、`flock`、`fcntl`、`utime`、`setxattr` 這幾族（探得出路徑在不在、讀得到 metadata，讀不到內容）；建 socket 本身（只管 TCP、UDP 的 bind 和 connect）；經過 `/proc/<pid>/fd` 拿到的 pipe 和 socket。ptrace：「a sandboxed process should have a superset of the target process's access rights」，所以關起來的 renderer ptrace 不了外面的 process。文件建議 best-effort：執行時問 ABI，只套那一版有的。

所以要 seccomp 補的：建 socket（`socket`、`socketpair`）、`execve`（Landlock 的 `EXECUTE` 也可以不給）、`io_uring`、`bpf`、`perf_event_open`、`ptrace`、`mount` 一族、`kexec`、設時鐘和排程；舊 kernel 上 Landlock 少的那幾層（訊號的範圍在 ABI 6 之前：seccomp 限制 `kill`、`tgkill` 的對象是自己）。

## Chromium 的 Linux sandbox

2026-11-26 讀的（`main`）：`sandbox/linux/README.md`、`sandbox/linux/seccomp-bpf-helpers/baseline_policy.cc`、`sigsys_handlers.cc`、`sandbox/policy/linux/bpf_renderer_policy_linux.cc`，和它的 Linux 文件裡的 `zygote.md`、`sandbox_ipc.md`。

- **兩層**：README：「Chrome uses: setuid(2) everywhere; namespaces where supported (modern Linux kernels); seccomp(2) BPF where supported」。namespace 的那一層（空的 PID、network namespace、chroot 到空的唯讀目錄、丟掉 capability）是啟動時由別人建的：`zygote.md`：「the Zygote is responsible for setting up and bookkeeping the namespace sandbox」（renderer 從 zygote fork 出來；`--renderer-cmd-prefix` 時不經過 zygote、重新 exec）。
- **Landlock**：README：「We currently don't use Landlock, but we'd like to」，連到評估它的 issue 和一份 GPU process 的 Landlock policy。
- **違規時**：baseline policy 的最後是 `CrashSIGSYS()`（「In any other case crash the program with our SIGSYS handler」）：handler 在 stderr 寫「seccomp-bpf failure」、讓 process 當掉。一部分回錯誤不當掉：檔案系統的 system call 回 `EPERM`（policy 的 `fs_denied_errno`）、`fork` 和 `vfork`、SysV IPC、`sendfile`、不許的 `madvise`（都是 `EPERM`）；`clone3` 回 `ENOSYS`（「clone3 takes a pointer argument which we cannot examine」，讓 libc 退回 `clone`，再限制 `clone` 的旗標只能建 thread）、`pidfd_open` 回 `ENOSYS`。
- **個別的限制**：`kill`、`tgkill` 限制對象（`RestrictKillTarget(current_pid)`）；`socketpair` 只許 `AF_UNIX`，其他的當掉；`mprotect`、`pkey_mprotect`、`mmap` 限制旗標；`prctl`、`futex`、`getrandom`、`pkey_alloc` 各有限制的函式；`madvise` 只許 `DONTNEED`、`WILLNEED`、`RANDOM`、`REMOVE`、`NORMAL`、`FREE`；`sched_*` 只能對自己的 thread；`memfd_create` 許；renderer 的 policy 另外許 `ioctl` 的 `TCGETS`、`FIONREAD`（其餘當掉）、`setrlimit`（WebAssembly 的位址空間）、`mremap`、`uname`、`sysinfo`、`prlimit64`（對自己）。
- **字型和檔案**：檔案系統的 system call 一律被擋（回錯誤），沒有一般的開檔案的 broker。字型經過 Browser 那一側的 font service（`components/services/font/`）；`sandbox_ipc.md`：「only the out-of-process localtime implementation uses the Sandbox IPC」（時區由外面的 process 算）。
- **zygote 另外換到的**：省 loader 的 relocation 和共同函式庫的初始化（每個 process 約 8 MB、60 ms/GHz）、renderer 和 Browser 一定是同一個執行檔的版本。

對 machopo 的意思：Chromium 的 namespace 那一層綁著 zygote（或 setuid 的 helper）；machopo 的 renderer 是重新 exec 的（`ipc::platform::spawn`），沒有 zygote。它的 renderer 用得到而 Chromium 特別處理的：`clone3`（探測裡 21 次：seccomp 看不到它的旗標）、`ioctl` 的 `TCGETS2`、`madvise` 的 `MADV_HUGEPAGE`、`MADV_DONTFORK`、`MADV_DODUMP`、`MADV_GUARD_INSTALL`（Chromium 的清單裡沒有）、`pkey_*`。

## Chromium 的 macOS sandbox

2026-11-26 讀的（`main`，`e7aa2e9ba30a`）：`sandbox/mac/README.md`、`sandbox/mac/seatbelt.cc`、`seatbelt_exec.cc`、`sandbox/policy/mac/common.sb`、`renderer.sb`、`content/renderer/renderer_main_platform_delegate_mac.mm`。

- **機制**：Seatbelt，每種 process 一份 `.sb`，都從 `common.sb` 的 `(deny default)` 開始。README：「all our sandboxes use `(deny default)`」。API 是 `sandbox_init_with_parameters`，或先 `sandbox_compile_string` 再 `sandbox_apply`（都自己宣告，不在公開的 header）。
- **時機**：Browser 把 profile 和參數序列化、經一條 pipe 交給 child（命令列說 fd），child 在很前面讀出來套上（`SeatbeltExecServer::InitializeSandbox`）；`RendererMainPlatformDelegate::EnableSandbox` 只檢查「已經在 sandbox 裡」，再關掉 WindowServer 和 Launch Services 的連線。README 也寫了另一種常見的做法：「first open any needed resources, then apply the sandbox policy」。
- **違規時**：回錯誤；`--enable-sandbox-logging` 讓被擋的寫進 syslog，README 教人用它列出新的 process 需要什麼。
- **字型**（`common.sb` 的 `allow-font-access`，renderer 用）：讀 `/Library/Fonts`、`/System/Library/Fonts`、`~/Library/Fonts`（內容和 xattr）；查 `com.apple.FontObjectsServer` 和 `com.apple.fonts`；系統下載的字型用 extension（`(extension "com.apple.app-sandbox.read")`）。
- **`common.sb` 給每種 process 的**：對自己送訊號；`process-info-pidinfo`（自己）；查 Browser 的 rendezvous（名字帶 Browser 的 pid）、`com.apple.logd`、`com.apple.system.logger`、`opendirectoryd.libinfo`；**整個檔案系統的 metadata**（「Allow realpath() to work」）；讀自己的執行檔和 bundle、`/System/Library/Frameworks` 等系統函式庫的目錄、`/private/etc/passwd`（CFPrefs 的 `getpwuid`）、`/dev/urandom`、`/dev/null`、時區（`/private/var/db/timezone`）、`/usr/share/icu`、全域的偏好設定檔；四十個左右的 sysctl（`hw.ncpu`、`hw.memsize`、`kern.hostname`、`kern.osversion`、`hw.optional.` 開頭的……）；明寫擋掉 `com.apple.cfprefsd.daemon`（讓偏好設定走 process 裡直接讀檔的那條路，只讀得到 profile 放行的那幾個檔案）。
- **`renderer.sb` 另外的**：先明寫擋掉 `iokit-get-properties`、`process-info*`、`nvram*`、`distributed-notification-post`（註解：「These operations are allowed by default」）；放行 `user-preference-read`、`process-info-setcontrol`（自己）、IOSurface 的 IOKit、`com.apple.system.notification_center`、`com.apple.lsd.mapdb`、`com.apple.cvmsServ`、幾個 IOKit 的屬性、V8 用的 `kern.tcsm_enable`。
- **沒有用 system call 的過濾**（README 的例子裡有 `syscall-unix`，renderer 的 profile 沒有）。

對 machopo 的意思：機制和形狀一樣（`deny default`、child 很前面就鎖、回錯誤、字型自己讀加上字型服務、明寫擋掉預設允許的那幾種）。machopo 的 renderer 不載入 AppKit、不碰 IOSurface 和偏好設定，所以清單短得多（已決定的事項第 20 條）；metadata 比它緊（第 19 條）；參數由 renderer 自己取，不用那條 pipe（第 17 條）。

## Chromium 的 tracing

2026-11-26 讀的 Perfetto 的文件（`perfetto.dev/docs/concepts/service-model`、`buffers`；Chrome 的 tracing 用它）。Chromium 自己的 `services/tracing` 的說明兩個網址都是 404，所以「sandbox 裡的 renderer 走這條路」是照 Perfetto 的文件推論的。

- tracing service 擁有 trace 的 buffer（「Owns the trace buffers」），在 Chrome 裡是「a service in Chrome」；要資料的一方（consumer）是「an interface of the Browser process in Chrome」；產生事件的 process（producer）是「an untrusted entity」。
- 資料走共用記憶體，不放在訊息裡：「Producer(s) write tracing data ... directly into its shared memory buffer」；「The IPC is typically NOT employed to transport the protobufs for the trace」；共用記憶體的一頁寫滿時 producer 送一則 IPC，請 service 複製那一頁到它自己的 buffer；最後由 service 把 buffer 寫成 trace 檔。
- 理由（文件）：寫的那條路零複製、負擔低；producer 不可信，不能看到彼此的資料。
- 沒查到的：共用記憶體由誰建；`--trace-startup` 時 child process 的資料怎麼收。

## 調查過的 crate

2026-11-26（crates.io 的 API 和各自的 README、CHANGELOG）：

| crate | 版本和日期 | 授權 | 說明 |
| --- | --- | --- | --- |
| `landlock` | 0.4.8（2026-11-20） | MIT OR Apache-2.0 | Landlock 的專案自己的（`landlock-lsm/rust-landlock`）：「Safe abstraction for Linux Landlock sandboxing」；到 ABI 10（TCP、UDP 的 port、`Ruleset::scope()` 的範圍）；相容的三種程度（best effort、soft requirement、hard requirement）；MSRV 1.77；近期下載 800 萬 |
| `seccompiler` | 0.5.0（2025-03-07） | Apache-2.0 OR BSD-3-Clause | rust-vmm（Firecracker）的：把 Rust 寫的規則編成 BPF；x86-64、aarch64、riscv64；只依賴 `libc`（JSON 的格式是選用的）；`apply_filter_all_threads`（TSYNC）；動作有 allow、errno、kill process、kill thread、trap、log、trace；近期下載 1400 萬 |
| `libseccomp` | 0.4.0（2025-04-05） | MIT OR Apache-2.0 | C 的 libseccomp 的 binding：多一個 C 的相依 |
| `extrasafe` | 0.5.1（2024-04-16） | MIT | 包 seccomp 和 Landlock 的高階介面；用的人少 |
| `birdcage` | 0.8.1（2024-04-19） | GPL-3.0-or-later | 跨平台（Linux 用 namespace、macOS 用 `sandbox_init`）；授權不合，兩年多沒有新版 |
| `gaol` | 0.2.1（2019-10-16） | MIT/Apache-2.0 | Servo 的；沒有在維護 |

看法：Linux 用 `landlock` 和 `seccompiler`（兩個都是該機制的主要使用者自己維護的、授權合、沒有 C 的相依）。要確認的：兩個 crate 的 API 是不是 safe 的（README 這樣說，沒讀原始碼）、`seccompiler` 支不支援照參數比對（`pkey_mprotect` 的旗標、`kill` 的對象）。

## 要對原始碼的地方

README 的「先例」和各節「對照 Chromium」裡 Windows、Firefox 的部分是照記憶寫的。定案前對：

- Chromium 的 Linux：對過了（上面的「Chromium 的 Linux sandbox」）；還沒讀的：`RestrictKillTarget`、`RestrictMprotectFlags`、`RestrictPrctl` 這些函式的內容（`syscall_parameters_restrictions.cc`），第 1 段寫 policy 時讀。
- Chromium 的 macOS：對過了（上面的「Chromium 的 macOS sandbox」）。
- Chromium 的 Windows：`sandbox/win/`、`sandbox/policy/win/sandbox_win.cc`（renderer 的 token、integrity、job、mitigation）、`content/child/dwrite_font_proxy/`。
- Firefox：`security/sandbox/linux/`（`SandboxFilter.cpp`、broker）、`security/sandbox/mac/SandboxPolicyContent.h`。
- 有沒有用 Landlock 的瀏覽器或類似的程式（先例）。

## 參考資料

- RFC 0004 的「為 sandbox 留的條件」，它的細節裡講字型和 Seatbelt 套用時機的兩節，它的原型的 README。
- `crates/ipc/soundness.md` 的 K5；`crates/ipc/README.md` 的平台層。
- kernel 的文件：`Documentation/userspace-api/landlock.rst`、`seccomp_filter.rst`。
