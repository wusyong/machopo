# RFC 0119：renderer process 的環境變數只給它要的

- 開始日期：2026-11-27
- 狀態：✅ 已完成：2026-11-27（Linux、macOS、Windows 三段；驗收和檢查在 `progress.md`）
- 細節：`details.md`

## 摘要

renderer process 原本繼承 Browser 的整份環境變數，使用者放在環境裡的 token、API key、socket 的路徑，被攻破的 renderer 都讀得到，哪個平台的 sandbox 都擋不住：它們在 process 啟動時就在它的記憶體裡。這份讓 Browser 啟動 renderer process 時清空環境，只放回清單上的：machopo 自己的開關、Rust 和 mimalloc 的開關、語系和時區，和各平台的系統要的那幾個。清單漏一個時壞得安靜（`Intl` 的語系、時區不對），所以每個平台在自己的機器上量過、由 sandbox 裡外的比較守著。三個平台都做，分段，Linux 先、macOS、Windows。

## 動機

現況（2026-11-27 的 `main`，`9aa39380`）：

- Browser 啟動 renderer process 的命令（`crates/shell/src/lib.rs` 的 `renderer_launcher`）沒有碰環境：Linux 和 macOS 上 `std::process::Command` 照預設繼承整份，`ipc` 的 `spawn` 再加上通道用的（Linux 的 `MACHOPO_IPC_FD`）；Windows 上 `sunaba::windows::Command::from_std` 先抄一份 Browser 的整份環境，再加上 `ipc` 的 `MACHOPO_IPC_CHANNEL`。
- renderer 只用到其中十幾個（`details.md` 的「renderer 讀了哪些變數（Linux，量的）」）。開發機的 shell 裡有七十幾個，其中有 SSH agent 和 D-Bus 的 socket 的路徑、別的程式的 token。
- seccomp、Landlock、Seatbelt、AppContainer 擋的是 renderer 拿這些東西去做什麼（連 socket、開檔），擋不住它讀到之後，經過它本來就有的路交出去：網頁的請求（由 Browser 替它送）、給 Browser 的其他訊息。

**目標**：

1. sandbox 裡的 renderer process 的環境只有清單上的變數；Browser 的其他變數一個都不在。
2. 網頁看到的不變：`Intl` 的預設語系、時區、`navigator` 的值、文字，在做完前後、sandbox 裡外、thread 上的 renderer 都相同。
3. 開發用的開關照舊作用在 renderer 上：`RUST_LOG`、`MACHOPO_V8_FLAGS`、`MACHOPO_TRACE`、`MIMALLOC_PURGE_DELAY`……（`machopo --help` 的「Environment」）。
4. 包著整個 machopo 跑的工具（`LD_PRELOAD` 的 profiler、這份量測用的 `getenv_log.c`）照樣進得了 renderer。

**不在範圍內**：

- Browser 自己的環境：它要的多（GPU 的驅動、視窗系統、fontconfig），而且 Browser 不在 sandbox 裡。
- `--single-process` 的 renderer：它是 Browser 的 thread，和 Browser 是同一份環境，沒有東西可以分開。
- renderer 自己啟動的 process：它啟動不了（seccomp、AppContainer 都擋）。
- 時區和語系改成由 Browser 決定、照訊息給（不靠環境和 `/etc/localtime`）：那是 Linux 的 chroot 的前提，在那一份（「之後的可能」）。

要解決的 `docs/limitations.md` 裡的限制：「隔離」的「renderer 拿到 Browser 的整份環境變數」。

## 設計概要

給 machopo 的使用者：看不到改變。

給改 machopo 的人：

- renderer 要讀一個新的環境變數時（新的 `MACHOPO_` 開關以外的），要把它加進清單（`shell::renderer_sandbox` 的清單，連同理由），不然它在 renderer process 裡是空的；`--single-process` 的 renderer 和 Browser 讀得到，所以只在 process 的 renderer 上壞。
- 在 renderer 裡讀環境的地方（函式庫的也算）在哪個平台上新增了，在那個平台上量一次：Linux 和 macOS 用 `getenv_log.c`，Windows 用 `getenv_trace.py`（`details.md` 寫了怎麼跑）。

## 詳細設計

### 誰來清

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| E1 | **Browser 啟動 renderer 時清空，只放回清單上的**（允許清單） | renderer 從第一個指令起就只有清單上的；新出現的、不認得的變數預設不給 | 清單漏一個時壞得安靜，要每個平台量過、有測試守 |
| E2 | Browser 啟動時拿掉看起來像秘密的（名字有 `TOKEN`、`KEY`、`SECRET`、`PASSWORD`……；拒絕清單） | 漏不了 renderer 要的 | 秘密不一定叫這些名字（`DATABASE_URL` 裡帶著密碼、`SSH_AUTH_SOCK`）；保證不了什麼 |
| E3 | renderer 啟動後自己清掉 | 不改啟動的那一側 | 沒有用：process 啟動時的環境字串在它自己的 stack 上，`unsetenv` 不抹掉它們，被攻破的 renderer 照樣讀得到 |

定了 **E1**（已決定的事項第 1 條）。對照 Chromium：Windows 上 sandbox 裡的 child 只留七個（`Path`、`SystemDrive`、`SystemRoot`、`TEMP`、`TMP`、`LOCALAPPDATA`、crashpad 的 pipe 名），是允許清單；Linux 和 macOS 上不過濾（`details.md` 的「Chromium」：zygote 和它 fork 的 renderer 繼承 Browser 啟動時的整份，macOS 只多加一個 `OS_ACTIVITY_MODE`）。所以這份在 Linux 和 macOS 上比 Chromium 緊。

### 清單放什麼

原則：**不是秘密、而且改變 renderer 怎麼跑的**才放。照類別：

| 類別 | 變數 | 為什麼 |
| --- | --- | --- |
| machopo 自己的 | `MACHOPO_` 開頭的全部 | 開發用的開關（`MACHOPO_V8_FLAGS`、`MACHOPO_TRACE`、`MACHOPO_GC_OPTIONS`……）；新的開關不用記得加進清單。通道用的（`MACHOPO_IPC_FD`、`MACHOPO_IPC_CHANNEL`）由 `ipc` 在清空之後加 |
| Rust 和 mimalloc | `RUST_LOG`、`RUST_BACKTRACE`、`RUST_LIB_BACKTRACE`、`RUST_MIN_STACK`、`NO_COLOR`；`MIMALLOC_` 開頭的 | log、panic 的 backtrace、thread 的 stack；mimalloc 自己讀它的選項（`MIMALLOC_PURGE_DELAY` 在 `--help` 裡） |
| 語系和時區 | `LANG`、`LC_ALL`、`LC_` 開頭的、`TZ`、`ICU_DATA`、`ICU_TIMEZONE_FILES_DIR` | ICU 用它們決定 `Intl` 的預設語系和時區（量到 renderer 讀 `LANG`、`LC_ALL`、`LC_MESSAGES`、`TZ`）；拿掉的話 `Intl` 的預設語系安靜地變成 `und`、日期的格式跟著變（量的：`LANG`、`LC_ALL` 是空的時），使用者設的 `TZ` 不作用、改用 `/etc/localtime`。ICU 的資料位置（`ICU_DATA`、`ICU_TIMEZONE_FILES_DIR`）也在：量到 renderer 讀它們，平常沒有值 |
| 動態連結器 | Linux：`LD_PRELOAD`、`LD_LIBRARY_PATH`；macOS：`DYLD_INSERT_LIBRARIES`、`DYLD_LIBRARY_PATH`，和 samply 的 `SAMPLY_BOOTSTRAP_SERVER_NAME` | 不是秘密；拿掉的話包著 machopo 跑的工具（profiler、Guard Malloc、量測）進不了 renderer。machopo 自己不靠它們（`env -i` 的 renderer 照樣啟動）。macOS 的 samply 預載的函式庫照後者找 samply 的 Mach service、交出 task port |
| 平台的系統 | Windows：`SystemRoot`、`SystemDrive`；macOS：`__CF_USER_TEXT_ENCODING` | Windows：系統的元件照 `%SystemRoot%` 展開登錄裡的 DLL 的路徑，量到 renderer 不讀、為了預防而給（已決定的事項第 13 條）。macOS：CoreFoundation 的初始化讀它，沒有時向目錄服務查使用者、讀家目錄的 `~/.CFUserTextEncoding`（量的） |

沒放的（例）：`HOME`、`USER`、`PATH`（Linux、macOS）、`DISPLAY`、`WAYLAND_DISPLAY`、`XDG_*`、`DBUS_SESSION_BUS_ADDRESS`、`SSH_AUTH_SOCK`、`XAUTHORITY`、終端機的、別的程式的 token。renderer 不再用 fontconfig 之後，原本它要的 `HOME`、`XDG_*`、`FONTCONFIG_*` 也不用了。量到 renderer 讀、但不給的：`SIMDUTF_FORCE_IMPLEMENTATION`、`OPENSSL_ia32cap`（強制走哪一條 CPU 的路的除錯開關，結果相同），和 glibc 的 `GLIBC_TUNABLES`、`MALLOC_*`（調整 glibc 的配置器；用到時再加）。macOS 上量到 renderer 讀、但不給的：`USER`（CoreFoundation 的初始化讀，拿掉看不出差別）、系統框架的除錯開關（`NSZombieEnabled`、`CF*`、`OS_ACTIVITY_*`、`OSLogRateLimit`……，平常沒有值）、`OPENSSL_armcap`；也不給 `TMPDIR`、`XPC_*`、`DYLD_FRAMEWORK_PATH` 和 libmalloc 的 `Malloc*`（用到時和 glibc 的一起加）。Windows 上不給 Chromium 清單上的 `Path`、`TEMP`、`TMP`、`LOCALAPPDATA`（使用者的路徑，renderer 不讀、在 AppContainer 裡也寫不了），也不給 `USERPROFILE`、`APPDATA`；量到 renderer 讀、但不給的：loader 啟動時的 `PATH`（`lower()` 之後 DLL 只照系統的目錄和執行檔的目錄找）、ntdll、apphelp 和 WOW64 平常沒有值的除錯開關，和 Linux 一樣的 `SIMDUTF_FORCE_IMPLEMENTATION`、`OPENSSL_ia32cap`。Windows 的 ICU 不讀語系和時區的變數（照系統的設定），它們照舊在清單上，三個平台一份。

語系和時區照環境傳下去，還是改成 Browser 決定、照訊息給（Chromium 的語系是 Browser 給的 `--lang`，照記憶、沒有對原始碼；時區沒查）：

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| L1 | **照環境傳下去**（上表） | renderer 看到的和現在一樣；改動小 | renderer 的語系和時區還是看環境和 `/etc/localtime`，Linux 的 chroot 之前要改 |
| L2 | Browser 決定語系和時區，經過啟動的訊息給，renderer 照它設 ICU | 和 Chromium 一樣；chroot 的前提一起做了 | Browser 沒有 ICU（V8 只在 `bindings`、`dom_js`），要另外決定語系和時區、和 ICU 自己找的一致；時區變了要通知 renderer |

定了 **L1**（已決定的事項第 3 條），L2 留給 Linux 的 chroot 那一份。

### 名單的寫法

- 清單在 `shell::renderer_sandbox`（renderer 的 policy 在那裡），每一項帶理由；三個平台一份，平台的那幾個用 `cfg` 分。
- 前綴（`MACHOPO_`、`MIMALLOC_`、`LC_`）照前綴比，其他照名字比；Windows 上不分大小寫（`Path` 和 `PATH` 是同一個，mimalloc 在那裡讀的是小寫的 `mimalloc_` 開頭的）。
- 清空和放回在啟動 renderer 的命令上做（`renderer_launcher` 叫 `limit_environment`）：標準庫的 `env_clear` 再 `env`。Linux 和 macOS 上標準庫照它啟動；Windows 的 renderer 由 `sunaba::windows::Command` 啟動，`from_std` 看不到 `env_clear`（讀它的 `get_env_clear` 是 unstable），所以 `browser::RendererProcess` 另外帶著「清空了」（`clear_environment`），ipc 的 `spawn_handing` 照它從空的環境建命令、放上標準庫的命令設的變數（已決定的事項第 11 條）。通道的變數（`MACHOPO_IPC_FD`、`MACHOPO_IPC_CHANNEL`）由 ipc 在這之後加。

### 哪些 renderer

| | 做法 | 說明 |
| --- | --- | --- |
| N1 | **sandbox 裡和 `--no-sandbox` 的 renderer process 都清** | 一條路；`--no-sandbox` 的測試跑的就是真的那條。`--no-sandbox` 是除錯用的，要多給一個變數時改清單 |
| N2 | 只清 sandbox 裡的 | 除錯時 renderer 拿到全部，方便；但兩條路，清單漏了的只在 sandbox 裡壞，和「sandbox 裡外比較」分不出是 sandbox 還是環境 |

定了 **N1**（已決定的事項第 4 條）。

### 測試

- **renderer 的環境只有清單上的**：Browser 帶著一個假的秘密（例如 `SOME_API_TOKEN`）和清單上的變數啟動，從外面讀真的 renderer process 的環境（Linux 的 `/proc/<pid>/environ`，renderer 不能被 dump 時改成在 probe 裡讀；macOS 的 `KERN_PROCARGS2`；Windows 的 process 的 PEB，`sunaba::windows::environment`；renderer 拿到通道後拿掉的 `MACHOPO_IPC_CHANNEL` 讀不到），秘密不在、清單上的在、沒有清單以外的。
- **網頁看到的不變**：sandbox 裡外和 thread 上的 renderer 畫出同一頁的比較（三個平台的 `a_sandboxed_renderer_shows_a_page_as_one_outside_does`）加上 `Intl` 的預設語系、`Intl.NumberFormat` 的格式；另外用 `LANG=ja_JP.UTF-8`、`TZ=Asia/Tokyo` 啟動一次，renderer 看到的是它們（證明清單真的傳下去）。
- **開關照舊**：`MACHOPO_V8_FLAGS`、`RUST_LOG` 作用在 renderer process 上（現有的 trace 和 log 的測試在 process 的 renderer 上跑）。
- mutation test：清單拿掉一項、不清空、前綴比成全名。
- 量測工具的測試（`CLAUDE.md`：改了 renderer 怎麼啟動）。

### 分階段

0. 草稿和 Linux 的量測（這一版；`details.md`）。
1. **Linux**（做完，2026-11-27）：清單和清空的機制（不分平台的部分在這一段做出來）；renderer process 切過去；測試。
2. **macOS**（做完，2026-11-27）：量 renderer 讀了什麼（`getenv` 的 interpose：`DYLD_INSERT_LIBRARIES`；SIP 不擋自己 build 的執行檔）、macOS 的那幾個；切過去；從外面讀 renderer 的環境（`sunaba::macos::environment`）。
3. **Windows**（做完，2026-11-27）：量 renderer 讀了什麼（debugger 在讀環境的 export 上下 breakpoint，已決定的事項第 10 條）；`sunaba::windows::Command` 照清空的命令建；Windows 的那兩個；切過去；從外面讀 renderer 的環境（`sunaba::windows::environment`）。

每一段在它的平台上做和驗，做完先 merge（`rfcs/README.md` 的「階段的 merge」），還沒做的平台照舊繼承整份。

## 缺點

- 清單漏一個時壞得安靜：只在 process 的 renderer 上、而且常常只在某些機器的設定上（某個語系、某個 `TZ`）。函式庫升級、多讀一個變數時也一樣。
- 除錯時想對 renderer 設一個清單以外的變數，要先改清單。
- 每個平台的清單要在那個平台上量和維護。

## 理由和其他做法

- **不做的話**：使用者環境裡的秘密留在每個 renderer 的記憶體裡；sandbox 再緊也拿不走它。
- **只做 Windows**（對齊 Chromium）：改動最少，但 Linux 和 macOS 上留著同一個洞，而它的成本（一份清單、一個測試）在三個平台上差不多。定了三個平台都做（第 5 條）。
- 各節的選項比較在上面。

## 先例

2026-11-27 讀的（`details.md` 的「Chromium」）：

- **Chromium**：Windows 的 sandbox 給 child 一份過濾過的環境（允許清單，七個）；Linux 上 renderer 從 zygote fork，zygote 啟動時沒有清環境、經過 zygote 的 child 不能另外指定環境（`child_process_launcher_helper_linux.cc` 檢查它是空的）；macOS 上照 delegate 給的環境啟動、不清空，有 sandbox 時加 `OS_ACTIVITY_MODE=disable`。
- **Firefox**：沒查。

## 已決定的事項

1. **Browser 啟動 renderer 時清空環境，照允許清單放回（E1）**（2026-11-27，使用者決定）：renderer 從第一個指令起就只有清單上的變數，新出現的、不認得的預設不給。理由：只有啟動的一方做得到（E3 沒有用：process 啟動時的環境字串在它自己的 stack 上，`unsetenv` 不抹掉）；拒絕清單（E2）保證不了什麼，秘密不一定叫那些名字。Chromium 在 Windows 上也是允許清單。代價：清單漏一個時壞得安靜，每個平台要量過、由 sandbox 裡外和 thread 上的 renderer 的比較守著。
2. **清單：不是秘密、而且改變 renderer 怎麼跑的才放；`MACHOPO_`、`MIMALLOC_`、`LC_` 照前綴，其他照名字，Linux 另給 `LD_PRELOAD`、`LD_LIBRARY_PATH`（C1）**（2026-11-27，使用者決定）：類別照「清單放什麼」的表；平台的系統要的（Windows 照 Chromium 的七個，macOS 量過之後）在各平台那一段定。理由：新加的 `MACHOPO_` 開關自動到得了 renderer，不會只在 process 的 renderer 上安靜地不作用；mimalloc 的選項幾十個，照前綴才不會漏；包著 machopo 跑的工具（`LD_PRELOAD` 的 profiler、這份的 `getenv_log.c`）照樣進得了 renderer，兩個都不是秘密，能設定 Browser 環境的人本來就控制得了 Browser。代價：前綴底下的變數全部放行，使用者自己取名 `MACHOPO_` 開頭的秘密也會過去。不選全部照名字列、由測試對照 `--help`（C2：一眼看得出 renderer 拿到什麼，但 mimalloc 的要嘛全列、要嘛漏，測試只守得到 `--help` 寫了的）。不選 C1 但不給動態連結器的（C3：工具進不了 renderer，換不到安全）。
3. **語系和時區照環境傳下去（L1）**（2026-11-27，使用者決定）：`LANG`、`LC_` 開頭的、`TZ` 在清單上（第 2 條），renderer 的 ICU 照舊從環境和 `/etc/localtime` 決定 `Intl` 的預設語系和時區。理由：renderer 看到的和現在一樣，語系和時區不是秘密，改由 Browser 給在 Linux 的 chroot 之前換不到安全。不選 Browser 決定、照啟動的訊息給（L2：Chromium 的語系是 Browser 給的，但 Browser 沒有 ICU，要另外找出和 ICU 一致的語系和時區，時區變了要通知 renderer）；L2 留給 Linux 的 chroot 那一份（「之後的可能」）。
4. **sandbox 裡和 `--no-sandbox` 的 renderer process 都清（N1）**（2026-11-27，使用者決定）：`--single-process` 的 renderer 是 Browser 的 thread，不在範圍內。理由：一條啟動的路，`--no-sandbox` 的測試跑的就是真的那條；sandbox 裡外的比較只差在 sandbox，清單漏了的裡外一起壞，和 thread 上的 renderer 比就看得出來。和字型服務那份 RFC 的第 8 條同一個理由。代價：除錯時要給 renderer 清單以外的變數，先改清單（`MACHOPO_` 開頭的照前綴已經放行）。不選只清 sandbox 裡的（N2：除錯方便，但兩條路，清單漏了的只在 sandbox 裡壞，分不出是 sandbox 擋的還是環境少了）。
5. **三個平台各一段，Linux 先（R1）**（2026-11-27，使用者決定）：清空和放回的機制、清單的結構、測試不分平台，在 Linux 那一段做出來；macOS 和 Windows 各一段，補那個平台的變數、在那台上量和驗，兩者的順序看哪台先有空。理由：清單要在那個平台上量過才知道漏了什麼，沒量過就打開是安靜地壞；這台是 Linux，量測也做了。代價：中間有一段時間三個平台不一樣（還沒切的照舊繼承整份）。不選三個平台的程式一次寫好、之後只驗（R2：macOS 和 Windows 的部分寫好了也不能先打開）。不選照 Chromium 先做 Windows（R3：不在那台上就動不了，Linux 和 macOS 的洞留得比較久）。
6. **macOS 的系統要的變數只給 `__CF_USER_TEXT_ENCODING`（M1）**（2026-11-27，使用者決定）：`USER`、`HOME`、`TMPDIR`、`XPC_*` 和系統框架的除錯開關都不給。理由：量到 libxpc 和 CoreFoundation 的初始化（`main` 之前、進 sandbox 之前）讀它；沒有它時 CoreFoundation 向 opendirectoryd 查使用者、打開家目錄的 `~/.CFUserTextEncoding`，每個 renderer 各一次（`details.md` 的「renderer 讀了哪些變數（macOS，量的）」）。它的值是 uid、文字的編碼和地區三個數字，不是秘密。`USER` 也是 CoreFoundation 的初始化讀的，拿掉看不出差別。不選 macOS 不多給（M2：清單和 Linux 一樣，但 renderer 在進 sandbox 前讀使用者的檔案、查目錄服務）。對照 Chromium：macOS 上不過濾，有 sandbox 時多加 `OS_ACTIVITY_MODE=disable`（關系統 log 的雜訊，和環境的洩漏無關，這份不跟）。
7. **macOS 給 `DYLD_INSERT_LIBRARIES`、`DYLD_LIBRARY_PATH`（D1）**（2026-11-27，使用者決定）：Linux 的 `LD_PRELOAD`、`LD_LIBRARY_PATH`（第 2 條）在 macOS 上的對應。理由：包著 machopo 跑的工具（Guard Malloc 的 `libgmalloc.dylib`、這份的 `getenv_log.c`）照樣進得了 renderer（目標 4）；不是秘密，能設定 Browser 環境的人本來就控制得了 Browser。不選再加 `DYLD_FRAMEWORK_PATH` 和 libmalloc 的 `Malloc` 開頭的除錯開關（D2：Linux 也沒給 glibc 的 `MALLOC_*`，用到時兩個平台一起加）。不選都不給（D3：工具和量測進不了 renderer，換不到安全）。
8. **macOS 的測試用 `sunaba::macos::environment(pid)` 從外面讀 renderer 的環境（R1）**（2026-11-27，使用者決定）：照 `sysctl` 的 `KERN_PROCARGS2`，和 `sunaba::macos::sandboxed(pid)` 一樣是給啟動的一方看 child 的。理由：量到它讀得到 sandbox 裡的 process 啟動時的環境（只對系統的執行檔遮掉，machopo 不是），名字和值照 NUL 分得精確。代價：`sunaba` 多一處 `unsafe`（論證在它的 `soundness.md`）。不選解析 `ps -E` 的輸出（R2：參數和環境照空白接在一起，值裡的空白分不出來，又靠 `ps` 的格式）。不選 probe 從裡面印自己的環境（R3：probe 是測試啟動的，不經過 Browser 啟動 renderer 的那條路）。
9. **macOS 給 samply 的 `SAMPLY_BOOTSTRAP_SERVER_NAME`（S1）**（2026-11-27，使用者決定）：量測工具的測試（`speedometer_test.py` 的 `test_the_stacks_in_the_timed_windows`）在清單打開後失敗：macOS 的 samply 用 `DYLD_INSERT_LIBRARIES` 預載自己的函式庫，那個函式庫照這個變數找 samply 的 Mach service、交出 process 的 task port；沒有它時 `bench.py speedometer --sample` 安靜地沒有 renderer 的樣本。理由：目標 4（包著 machopo 跑的工具照樣進得了 renderer）；值是 samply 每次隨機取的 service 的名字，不是秘密；連上在 `main` 之前，renderer 進 sandbox 之後查不到它。代價：清單多一個只為一個工具的項目。不選不給（S2：macOS 上的 `--sample` 量不到 renderer，那個測試要改成跳過）。Linux 的 `perf` 和 Windows 的 samply（ETW）不靠環境。

10. **Windows 上用 debugger 量 renderer 讀了哪些變數（W1）**（2026-11-27，使用者決定）：這個資料夾放一個 Python 的小 debugger（ctypes，`getenv_log.c` 在 Windows 上的對應），用 `DEBUG_PROCESS` 啟動測試，Browser 啟動的 renderer 也一起被 debug；在 `kernelbase` 的 `GetEnvironmentVariable*`、`ExpandEnvironmentStrings*`、`GetEnvironmentStringsW`，`ntdll` 的 `RtlQueryEnvironmentVariable*`，`ucrtbase` 的 `getenv` 一族下 breakpoint，記下每個 process 讀的名字。理由：可以重跑、做法和結果寫得進 `details.md`；CRT 的 `getenv` 讀的是它啟動時抄的一份（`GetEnvironmentStringsW`），不經過 `kernelbase`，只 hook 一層會漏。原型驗過：sandbox 裡（AppContainer）的 child 也被 debug、breakpoint 抓得到，probe 照常結束；mimalloc 在 Windows 上照小寫的名字經過 `GetEnvironmentVariableA` 讀（所以前綴要不分大小寫比）。代價：在 debugger 底下跑得慢，只跑 sandbox 的測試和整套 `shell` 一次；整份走一遍的只看得到 `GetEnvironmentStringsW` 被叫。不選 API Monitor（W2：第三方的 GUI，要下載、手動操作、不能重現）。不選不量、直接打開（W3：清單漏一個時壞得安靜）。對照 Chromium：Windows 的清單是固定的七個，沒看到它量過。
11. **Windows 上「清空」由 `RendererProcess` 帶到 ipc，sunaba 從空的環境建命令（S1b）**（2026-11-27，使用者決定）：`sunaba::windows::Command` 多一個 `env_clear`（和標準庫的同名同義）；`browser::RendererProcess` 多一個 Windows 才有的欄位（和 `files` 一樣），`shell` 的 `limit_environment` 在 Windows 上設它；ipc 的 `spawn_handing` 照它先清空、再放上標準庫的命令設的變數和通道的 `MACHOPO_IPC_CHANNEL`。Linux 和 macOS 照舊用標準庫的 `env_clear`。理由：Windows 的 renderer 不是標準庫啟動的（sandbox 要 `CreateProcessAsUserW` 和 `STARTUPINFOEX` 的 attribute list，連 `--no-sandbox` 也要 handle 的清單；標準庫的 `spawn_with_attributes`、`inherit_handles` 是 unstable），ipc 用 `from_std` 照 getter 讀標準庫的命令，而讀「清空過」的 `get_env_clear` 是 unstable（`command_resolved_envs`），`from_std` 看不到它，就先抄了 Browser 的整份環境；缺的只是把這一點帶到建命令的地方。清單在建命令的那一刻套用，和 Linux、macOS 的語意相同；做完的兩段不用動。代價：「清空」在 Unix 上走標準庫、在 Windows 上走 `RendererProcess`，兩條路；`get_env_clear` 穩定之後 `from_std` 照它叫 `env_clear`，欄位和參數拿掉。不選三個平台都改成啟動的選項（S1a：一條路、像 Chromium 的 `LaunchOptions` 的 `clear_environment`，但要重寫做完、驗過的兩段和 ipc 三個平台的 spawn）。不選對當時環境裡不在清單上的逐一 `env_remove`（S2：不改介面，但 `from_std` 到啟動那一刻才讀環境，中間 Browser 裡的 C 函式庫用 `SetEnvironmentVariableW` 加的會過去）。不選等 `get_env_clear` 穩定（S3）。對照 Chromium（照記憶）：Windows 上是 sandbox 的 broker 建 target process 時照允許清單另建一份，同一個位置。
12. **Windows 的測試用 `sunaba::windows::environment(pid)` 從外面讀 renderer 的環境（R1）**（2026-11-27，使用者決定）：`OpenProcess`（`PROCESS_QUERY_LIMITED_INFORMATION`、`PROCESS_VM_READ`）、`NtQueryInformationProcess` 的 `ProcessBasicInformation` 拿 PEB，`ReadProcessMemory` 讀 `ProcessParameters` 的 `Environment` 和 `EnvironmentSize`，照 NUL 分成名字和值；和第 8 條的 `sunaba::macos::environment` 一樣是給啟動的一方看 child 的。理由：讀的是 Browser 經過啟動 renderer 的那條路真的啟動的 process；同一個使用者讀得到 sandbox 裡的 renderer 的記憶體（現有的測試從外面列它的 DLL，要 `PROCESS_VM_READ`）。代價：`sunaba` 多一處 `unsafe`（論證在它的 `soundness.md`）；`RTL_USER_PROCESS_PARAMETERS` 的這兩個欄位不在公開的 `winternl.h` 裡，位移寫死（x64）；讀到的是 process 現在的環境，renderer 拿到通道後拿掉的 `MACHOPO_IPC_CHANNEL` 不在，測試照這點寫。不選 probe 從裡面印自己的環境（R2：Windows 的 probe 用 `sunaba::windows::Command::new` 啟動自己，本來就繼承整份，不經過啟動 renderer 的那條路）。不選 renderer 經過 automation 回報（R3：為了測試多一個讀 renderer 內部的 API）。
13. **Windows 的系統要的變數只給 `SystemRoot`、`SystemDrive`（V3）**（2026-11-27，使用者決定）：`Path`、`TEMP`、`TMP`、`LOCALAPPDATA` 都不給。量到的（`details.md` 的「renderer 讀了哪些變數（Windows，量的）」）：renderer 讀的系統變數只有 loader 啟動時的 `PATH` 和 ntdll、apphelp、WOW64 平常沒有值的那幾個，Chromium 清單上的都沒讀；清單打開、一個系統變數都不給時，整套 `shell` 的測試照樣通過。理由：系統的元件照 `%SystemRoot%` 展開登錄裡的路徑（量到 probe 載入 winsock、Browser 載入 `rsaenh.dll` 時），沒有它時那個 DLL 安靜地載不進來，而測試沒走到的路（之後的功能、少用的系統元件）會這樣壞、只在 process 的 renderer 上；這兩個是系統的位置，不是秘密，沒有使用者的資訊。代價：清單多兩個量到 renderer 不讀的、為了預防的項目。不選照 Chromium 給六個（V1：`TEMP`、`TMP`、`LOCALAPPDATA` 是使用者的路徑、帶著使用者的名字，`Path` 常有使用者自己的工具的目錄；renderer 在 AppContainer 裡寫不了那些地方，量到的也沒讀，換不到東西；`lower()` 之後 DLL 只照系統的目錄和執行檔的目錄找，不看 `Path`）。不選都不給（V2：最緊、和量到的一致，但測試沒走到的系統元件一用 `%SystemRoot%` 就安靜地壞）。共用的清單照舊：`LANG`、`LC_` 開頭的、`TZ` 在 Windows 上也給，Windows 上的 ICU 不讀它們（照系統的設定），留著沒有害處，三個平台一份清單。對照 Chromium：`Path`、`SystemDrive`、`SystemRoot`、`TEMP`、`TMP`、`LOCALAPPDATA` 和 crashpad 的 pipe 的名字。

## 待決定

沒有：第 1 段之前的五件是已決定的事項第 1 到 5 條，macOS 的是第 6 到 9 條（第 9 條是量測工具的測試找到的），Windows 的是第 10 到 13 條。

## 之後的可能

- 語系和時區由 Browser 給（L2），連同 renderer 不再讀 `/etc/localtime` 和 zoneinfo：Linux 的 chroot 的前提。
- 清單也用在之後的其他 child process（GPU process、utility process），各自一份。
