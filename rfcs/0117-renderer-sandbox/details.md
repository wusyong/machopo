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

## Linux 的機制的比較

2026-11-26 對過 kernel 的文件（`docs.kernel.org/userspace-api/landlock.html`，7.4-rc4 的版本）；seccomp 和 namespace 的版本是照記憶寫的：

| | seccomp-bpf | Landlock | namespace |
| --- | --- | --- | --- |
| 管什麼 | system call 的編號和數值的參數 | 檔案（照路徑的階層）、TCP 和 UDP 的 port、abstract unix socket 和訊號的範圍、ptrace 的方向 | 看得到的檔案系統、process、網路介面、使用者 |
| 誰套 | process 自己（`PR_SET_NO_NEW_PRIVS` 之後） | process 自己（同上；文件：「For unprivileged processes, setting the no_new_privs attribute is required」） | 建 process 的一方（`clone`、`unshare`） |
| 要特權嗎 | 不用 | 不用 | user namespace 不用，但有的發行版關掉沒有特權的 |
| 看不到路徑 | 是（指標指到的字串讀不了） | 不是 | 不適用 |
| 套了之後 | 新的 thread 繼承；可以再疊更緊的 | 套的那個 thread 和它之後建的 thread、child 繼承；已經在跑的其他 thread 不會（ABI 8 起有對全部 thread 套的旗標）；拿不掉 | 不能再放寬 |

Landlock 的 ABI（文件裡每一版拿掉的限制；kernel 的版本文件沒寫，1 到 7 是照記憶：5.13、5.19、6.2、6.7、6.10、6.12、6.15）：1 檔案；2 `REFER`；3 `TRUNCATE`；4 TCP 的 bind 和 connect；5 裝置的 `ioctl`；6 範圍（`LANDLOCK_SCOPE_SIGNAL`：只能對同一個或更裡面的 domain 的 process 送訊號；`LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET`）；7 紀錄的旗標；8 對全部 thread 套；9 有路徑的 unix socket（`RESOLVE_UNIX`）；10 UDP；11 `restrict_self` 順便設 `no_new_privs` 的旗標；12 capability 和 namespace 的規則。**開發機（kernel 7.3.3）回的是 ABI 11**。

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

README 的「先例」和各節「對照 Chromium」裡 macOS、Windows、Firefox 的部分是照記憶寫的。定案前對：

- Chromium 的 Linux：對過了（上面的「Chromium 的 Linux sandbox」）；還沒讀的：`RestrictKillTarget`、`RestrictMprotectFlags`、`RestrictPrctl` 這些函式的內容（`syscall_parameters_restrictions.cc`），第 1 段寫 policy 時讀。
- Chromium 的 macOS：`sandbox/policy/mac/renderer.sb`、`common.sb`（字型和 `com.apple.fonts` 的服務）、`sandbox/mac/seatbelt_exec.cc`（一開始就鎖）。
- Chromium 的 Windows：`sandbox/win/`、`sandbox/policy/win/sandbox_win.cc`（renderer 的 token、integrity、job、mitigation）、`content/child/dwrite_font_proxy/`。
- Firefox：`security/sandbox/linux/`（`SandboxFilter.cpp`、broker）、`security/sandbox/mac/SandboxPolicyContent.h`。
- 有沒有用 Landlock 的瀏覽器或類似的程式（先例）。

## 參考資料

- RFC 0004 的「為 sandbox 留的條件」，它的細節裡講字型和 Seatbelt 套用時機的兩節，它的原型的 README。
- `crates/ipc/soundness.md` 的 K5；`crates/ipc/README.md` 的平台層。
- kernel 的文件：`Documentation/userspace-api/landlock.rst`、`seccomp_filter.rst`。
