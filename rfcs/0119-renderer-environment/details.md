# RFC 0119 的細節：renderer process 的環境變數

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## renderer 讀了哪些變數（Linux，量的）

2026-11-27，Fedora 45（kernel 7.3.3，x86-64），debug build，`9aa39380`。

**做法**：`getenv_log.c`（這個資料夾）是一個 `LD_PRELOAD` 的小函式庫：攔 `getenv` 和 `secure_getenv`，把名字和有沒有值寫進 `$ENVPROBE_DIR/<renderer 或 browser>.<pid>`（檔案在載入時、進 sandbox 之前就開好，之後只寫已經開著的 fd）。編法和用法：

```sh
cc -shared -fPIC -O2 -o getenv_log.so rfcs/0119-renderer-environment/getenv_log.c -ldl
LD_PRELOAD=$PWD/getenv_log.so ENVPROBE_DIR=/tmp/envprobe \
  cargo test -p shell --test it -- renderer_sandbox::a_sandboxed renderer_sandbox::every_thread
```

這兩個測試啟動真的 Browser 和 sandbox 裡、外的 renderer process，載入多種 script、generic family、時區和 `Intl` 的頁面（`--test-fonts` 和系統字型都有）。

**renderer 讀的**（三個 renderer process 合起來；「有值」是這台機器的環境）：

| 變數 | 誰讀 | 這台有值 |
| --- | --- | --- |
| `MACHOPO_IPC_FD` | `ipc`（通道） | 有 |
| `MACHOPO_TRACE`、`MACHOPO_V8_FLAGS`、`MACHOPO_V8_THREAD_POOL_SIZE`、`MACHOPO_GC_OPTIONS` | `shell`、`bindings`、`dom_js` | 沒有 |
| `RUST_LOG` | `tracing_subscriber` 的 `EnvFilter` | 有（cargo test 設的） |
| `RUST_MIN_STACK` | 標準庫開 thread 時 | 沒有 |
| `NO_COLOR` | log 的輸出 | 沒有 |
| `LANG`、`LC_ALL`、`LC_MESSAGES` | V8 裡的 ICU（預設語系） | `LANG` 有 |
| `TZ`、`ICU_TIMEZONE_FILES_DIR`、`ICU_DATA` | ICU（時區、資料檔） | 沒有 |
| `SIMDUTF_FORCE_IMPLEMENTATION` | V8 裡的 simdutf | 沒有 |
| `OPENSSL_ia32cap` | 某個帶 OpenSSL 式 CPU 功能偵測的函式庫（沒查是哪一個） | 沒有 |

**量不到的**：

- 不經過 `getenv` 的：glibc 自己讀的（`tzset` 讀 `TZ`、`GLIBC_TUNABLES`、`MALLOC_*`），動態連結器讀的（`LD_*`，在 `main` 之前），mimalloc 自己掃 `environ` 讀的 `MIMALLOC_*`，`std::env::vars()` 那種整份走一遍的。清單照文件和原始碼補（`README.md` 的「清單放什麼」）。
- 這兩個測試沒走到的路：wasm 的文件、worker、canvas 的文字、WebDriver、`census` 的 build（`MACHOPO_CENSUS`）。第 1 段再用整套 `shell` 的測試量一次。
- macOS：「renderer 讀了哪些變數（macOS，量的）」；Windows：在那一段量。

**Browser 讀的**比 renderer 多很多（fontconfig、expat、Mesa、Vulkan 的 loader、`HOME`、`XDG_*`、`LD_LIBRARY_PATH`……），都不在 renderer 裡：它們是 Browser 的字型、GPU 和視窗要的。

**這台 shell 的環境**：79 個變數，renderer 現在全部拿到。其中有 SSH agent 的 socket（`SSH_AUTH_SOCK`）、D-Bus 的 socket（`DBUS_SESSION_BUS_ADDRESS`）、X 的認證檔（`XAUTHORITY`），和別的程式放在環境裡的 token。

**清空環境的 renderer 照樣啟動**：`env -i target/debug/machopo --type=renderer` 進了 sandbox、因為沒有通道而結束（exit 2，「there is no browser to render for」）。machopo 連結的動態函式庫都在系統的目錄（`ldd`），不靠 `LD_LIBRARY_PATH`。

## renderer 讀了哪些變數（macOS，量的）

2026-11-27，macOS 26.6.2（arm64），debug build，`360a4c8f`。

**做法**：同一個 `getenv_log.c`，macOS 上用 dyld 的 interpose（`__DATA,__interpose`）攔 `getenv`（macOS 沒有 `secure_getenv`），用 `DYLD_INSERT_LIBRARIES` 載入；是不是 renderer 看 `_NSGetArgv`。debug build 是 linker 的 ad-hoc 簽章、沒有 hardened runtime，SIP 不擋，sandbox 裡的 renderer 也寫得進載入時開好的檔案。SIP 會從**系統的**執行檔（`/usr/bin/caffeinate`、`/usr/bin/env`……）的環境拿掉 `DYLD_*`，它們再啟動的程式就沒有了：所以在它們裡面才設。編法和用法：

```sh
cc -shared -O2 -o getenv_log.dylib rfcs/0119-renderer-environment/getenv_log.c -DENVPROBE_DEFAULT_DIR='"/tmp/envprobe"'
caffeinate -i zsh -c "DYLD_INSERT_LIBRARIES=$PWD/getenv_log.dylib cargo test -p shell"
```

libSystem 自己裡面的呼叫看不到（例如 libmalloc 直接讀 `envp` 的 `Malloc*`），mimalloc 掃 `environ` 讀的也看不到，和 Linux 一樣。

**renderer 讀的**（整套 `shell` 的測試，413 個 renderer process 合起來，清單還沒打開時；「有值」是這台機器的環境）：

| 變數 | 誰讀 | 這台有值 |
| --- | --- | --- |
| `MACHOPO_TRACE`、`MACHOPO_V8_FLAGS`、`MACHOPO_V8_THREAD_POOL_SIZE`、`MACHOPO_GC_OPTIONS`、`RUST_LOG`、`RUST_MIN_STACK`、`NO_COLOR` | 和 Linux 一樣 | 測試設的時候有 |
| `LANG`、`LC_ALL`、`LC_MESSAGES`、`TZ`、`ICU_DATA`、`ICU_TIMEZONE_FILES_DIR` | V8 裡的 ICU，和 Linux 一樣；`TZ` 也是 CoreFoundation 的初始化讀 | 沒有（這個 shell 沒有 `LANG`） |
| `__CF_USER_TEXT_ENCODING` | libxpc 和 CoreFoundation 的初始化（`main` 之前） | 有：uid、文字的編碼和地區三個數字 |
| `USER` | CoreFoundation 的初始化 | 有 |
| `OSLogRateLimit` | libtrace 的初始化 | 有（這個環境設的） |
| `TZFILE`、`CFProcessPath`、`CF_CHARSET_PATH`、`CFNumberDisableCache`、`CFStringDisableROM`、`NSZombieEnabled`、`NSDebugEnabled`……、`OS_ACTIVITY_*`、`OS_LOG_FAULT_REPORTS`、`LIBDISPATCH_*`、`DYLD_IMAGE_SUFFIX`、`IPHONE_SIMULATOR_ROOT`、`FEATUREFLAGS_*`、`MallocStackLogging` | CoreFoundation、Foundation、objc、libtrace、libdispatch 的除錯開關（初始化時讀） | 沒有 |
| `OPENSSL_armcap` | 帶 OpenSSL 式 CPU 功能偵測的函式庫（同 Linux 的 `OPENSSL_ia32cap`） | 沒有 |

renderer 不讀 `HOME`、`TMPDIR`、`PATH`、`XPC_*`。只跑 sandbox 那幾個測試量到的也是同一組，整套多跑的沒有新的。

**沒有 `__CF_USER_TEXT_ENCODING` 時**（`env -i` 的 renderer，另外攔 `getpwuid_r` 和 `open`）：CoreFoundation 的初始化向目錄服務查使用者（`getpwuid_r`，opendirectoryd）、打開 `~/.CFUserTextEncoding`，各兩次；只給它時這兩件都沒有。都在 `main` 之前，所以 sandbox 擋不到，也不會有 sandbox 的拒絕。拿掉 `USER` 看不出差別。清空環境的 renderer 照樣啟動、因為沒有通道而結束（exit 2）。

**清單打開之後**（同一套測試，在 shim 底下，`DYLD_INSERT_LIBRARIES` 照清單傳下去）：760 個整合測試都通過；renderer 讀的變數和打開之前一樣，`USER`、`OSLogRateLimit` 變成沒有值；`MallocStackLogging` 沒有人讀了（打開之前也只有 4 個 process 讀、沒有值，大概是環境裡 libmalloc 的其他變數引起的，沒查）。

**從外面讀 process 的環境**：`sysctl` 的 `KERN_PROCARGS2` 回 process 啟動時的參數區：參數的個數、執行檔的路徑、補齊的 NUL、參數、環境，一個空字串，然後是系統的字串（`ptr_munge=`、`executable_path=`……，值被清掉）。同一個使用者的 process 讀得到，在 sandbox 裡的也是（`sandbox-exec` 的 `(deny default)`）；系統的執行檔（`/bin/sleep`）的環境被拿掉、只剩參數。`ps -E` 讀的是同一份，系統的執行檔也一樣看不到。

**samply**（`bench.py speedometer --sample` 在 macOS 上用的）：它啟動的程式的環境多了 `DYLD_INSERT_LIBRARIES`（它的預載函式庫）和 `SAMPLY_BOOTSTRAP_SERVER_NAME`（它的 Mach service 的名字，每次隨機），預載的函式庫在 `main` 之前照後者找到 samply、交出 task port。清單打開、沒有後者時 renderer 沒有樣本（`speedometer_test.py` 的 `test_the_stacks_in_the_timed_windows`：「no samples of the renderer process's main thread」）；給了就有。`getenv_log.c` 看不到它被讀：samply 的函式庫是在 shim 之外的 image。

**這台 shell 的環境**：73 個變數，清單打開之前 renderer 全部拿到。其中有別的程式的 token 和 API key、SSH agent 的 socket。

## Chromium

2026-11-27 讀的（`main`）：

- **Windows**：sandbox 給 child 的環境照允許清單過濾：`Path`、`SystemDrive`、`SystemRoot`、`TEMP`、`TMP`（註解：一般的執行和測試要的）、`LOCALAPPDATA`（AppContainer 的 process 要的）、`CHROME_CRASHPAD_PIPE_NAME`（crashpad）；只在啟動的設定說要過濾時做，否則抄一份整個環境。renderer sandbox 那份 RFC 讀的是 `sandbox/win/src/broker_services.cc`（2026-11-26）；這次在 main 的 `target_process.cc` 裡沒有看到，現在在哪個檔案、對 renderer 是不是預設打開，沒有再確認。Firefox 帶著的 Chromium sandbox 有一個 patch（`security/sandbox/chromium-shim/patches/12_allow_env_changes.patch`）讓啟動的一方能另外指定 child 的環境，改的是同一段。
- **Linux**：`content/browser/zygote_host/zygote_host_impl_linux.cc` 的 `LaunchZygote` 只設定要交接的 fd，沒有 `clear_environment`、沒有指定環境（setuid sandbox 時另外叫 `SetupLaunchEnvironment`，沒讀）。`content/browser/child_process_launcher_helper_linux.cc`：經過 zygote 啟動的 child 檢查 delegate 給的環境是空的（註解：「Environment variables could be supported in the future, but are not currently supported when launching with the zygote」），所以 renderer 拿的是 zygote 的環境，也就是 Browser 啟動時的整份（照程式推論）。不經過 zygote 的 child 照 delegate 給的環境。namespace sandbox 的啟動函式沒讀。
- **macOS**：`content/browser/child_process_launcher_helper_mac.cc` 把 delegate 給的環境放進啟動的選項，不清空；有 sandbox、而且 log 沒開到 INFO 時多加 `OS_ACTIVITY_MODE=disable`（關掉系統 log 的雜訊）。delegate 給的是什麼、`base::LaunchOptions` 怎麼合併，沒讀。
- **語系**：照記憶，Browser 把它決定的語系用 `--lang` 交給 renderer，renderer 照它設 ICU 的預設語系，不看環境；沒有對原始碼。時區沒查。

## 沒有查到的

- Firefox 的 content process 的環境有沒有過濾。
- Windows 上 renderer 實際讀了哪些變數。
