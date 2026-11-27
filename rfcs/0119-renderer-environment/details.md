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
- macOS 和 Windows：在各自的那一段量。

**Browser 讀的**比 renderer 多很多（fontconfig、expat、Mesa、Vulkan 的 loader、`HOME`、`XDG_*`、`LD_LIBRARY_PATH`……），都不在 renderer 裡：它們是 Browser 的字型、GPU 和視窗要的。

**這台 shell 的環境**：79 個變數，renderer 現在全部拿到。其中有 SSH agent 的 socket（`SSH_AUTH_SOCK`）、D-Bus 的 socket（`DBUS_SESSION_BUS_ADDRESS`）、X 的認證檔（`XAUTHORITY`），和別的程式放在環境裡的 token。

**清空環境的 renderer 照樣啟動**：`env -i target/debug/machopo --type=renderer` 進了 sandbox、因為沒有通道而結束（exit 2，「there is no browser to render for」）。machopo 連結的動態函式庫都在系統的目錄（`ldd`），不靠 `LD_LIBRARY_PATH`。

## Chromium

2026-11-27 讀的（`main`）：

- **Windows**：sandbox 給 child 的環境照允許清單過濾：`Path`、`SystemDrive`、`SystemRoot`、`TEMP`、`TMP`（註解：一般的執行和測試要的）、`LOCALAPPDATA`（AppContainer 的 process 要的）、`CHROME_CRASHPAD_PIPE_NAME`（crashpad）；只在啟動的設定說要過濾時做，否則抄一份整個環境。renderer sandbox 那份 RFC 讀的是 `sandbox/win/src/broker_services.cc`（2026-11-26）；這次在 main 的 `target_process.cc` 裡沒有看到，現在在哪個檔案、對 renderer 是不是預設打開，沒有再確認。Firefox 帶著的 Chromium sandbox 有一個 patch（`security/sandbox/chromium-shim/patches/12_allow_env_changes.patch`）讓啟動的一方能另外指定 child 的環境，改的是同一段。
- **Linux**：`content/browser/zygote_host/zygote_host_impl_linux.cc` 的 `LaunchZygote` 只設定要交接的 fd，沒有 `clear_environment`、沒有指定環境（setuid sandbox 時另外叫 `SetupLaunchEnvironment`，沒讀）。`content/browser/child_process_launcher_helper_linux.cc`：經過 zygote 啟動的 child 檢查 delegate 給的環境是空的（註解：「Environment variables could be supported in the future, but are not currently supported when launching with the zygote」），所以 renderer 拿的是 zygote 的環境，也就是 Browser 啟動時的整份（照程式推論）。不經過 zygote 的 child 照 delegate 給的環境。namespace sandbox 的啟動函式沒讀。
- **macOS**：`content/browser/child_process_launcher_helper_mac.cc` 把 delegate 給的環境放進啟動的選項，不清空；有 sandbox、而且 log 沒開到 INFO 時多加 `OS_ACTIVITY_MODE=disable`（關掉系統 log 的雜訊）。delegate 給的是什麼、`base::LaunchOptions` 怎麼合併，沒讀。
- **語系**：照記憶，Browser 把它決定的語系用 `--lang` 交給 renderer，renderer 照它設 ICU 的預設語系，不看環境；沒有對原始碼。時區沒查。

## 沒有查到的

- Firefox 的 content process 的環境有沒有過濾。
- macOS 上自己 build 的執行檔，`DYLD_INSERT_LIBRARIES` 能不能進 sandbox 裡的 renderer（量測要用）。
- Windows 上 renderer 實際讀了哪些變數。
