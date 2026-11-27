# RFC 0119：renderer process 的環境變數只給它要的

- 開始日期：2026-11-27
- 狀態：👍 已接受：2026-11-27（設計的五件定了，已決定的事項第 1 到 5 條）
- 細節：`details.md`

## 摘要

renderer process 現在繼承 Browser 的整份環境變數，使用者放在環境裡的 token、API key、socket 的路徑，被攻破的 renderer 都讀得到，哪個平台的 sandbox 都擋不住：它們在 process 啟動時就在它的記憶體裡。這份讓 Browser 啟動 renderer 時清空環境，只放回清單上的：machopo 自己的開關、Rust 和 mimalloc 的開關、語系和時區，和各平台的系統要的那幾個。清單漏一個時壞得安靜（`Intl` 的語系、時區不對），所以每個平台在自己的機器上量過、由 sandbox 裡外的比較守著。三個平台都做，分段，Linux 先。

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
- 在 renderer 裡讀環境的地方（函式庫的也算）在哪個平台上新增了，用 `getenv_log.c`（Linux）量一次。

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
| 語系和時區 | `LANG`、`LC_ALL`、`LC_` 開頭的、`TZ` | ICU 用它們決定 `Intl` 的預設語系和時區（量到 renderer 讀 `LANG`、`LC_ALL`、`LC_MESSAGES`、`TZ`）；拿掉的話 `Intl` 的預設語系安靜地變成 POSIX 的（預期是 `en-US-u-va-posix`，第 1 段驗），使用者設的 `TZ` 不作用、改用 `/etc/localtime` |
| 動態連結器 | Linux：`LD_PRELOAD`、`LD_LIBRARY_PATH` | 不是秘密；拿掉的話包著 machopo 跑的工具（profiler、量測）進不了 renderer。machopo 自己不靠它們（`env -i` 的 renderer 照樣啟動） |
| 平台的系統 | Windows：`SystemRoot`、`SystemDrive`、`Path`、`TEMP`、`TMP`、`LOCALAPPDATA`（Chromium 的清單）；macOS：在那一段量 | 系統的 DLL、暫存的位置、AppContainer 的資料夾 |

沒放的（例）：`HOME`、`USER`、`PATH`（Linux、macOS）、`DISPLAY`、`WAYLAND_DISPLAY`、`XDG_*`、`DBUS_SESSION_BUS_ADDRESS`、`SSH_AUTH_SOCK`、`XAUTHORITY`、終端機的、別的程式的 token。renderer 不再用 fontconfig 之後，原本它要的 `HOME`、`XDG_*`、`FONTCONFIG_*` 也不用了。

語系和時區照環境傳下去，還是改成 Browser 決定、照訊息給（Chromium 的語系是 Browser 給的 `--lang`，照記憶、沒有對原始碼；時區沒查）：

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| L1 | **照環境傳下去**（上表） | renderer 看到的和現在一樣；改動小 | renderer 的語系和時區還是看環境和 `/etc/localtime`，Linux 的 chroot 之前要改 |
| L2 | Browser 決定語系和時區，經過啟動的訊息給，renderer 照它設 ICU | 和 Chromium 一樣；chroot 的前提一起做了 | Browser 沒有 ICU（V8 只在 `bindings`、`dom_js`），要另外決定語系和時區、和 ICU 自己找的一致；時區變了要通知 renderer |

定了 **L1**（已決定的事項第 3 條），L2 留給 Linux 的 chroot 那一份。

### 名單的寫法

- 清單在 `shell::renderer_sandbox`（renderer 的 policy 在那裡），每一項帶理由；三個平台一份，平台的那幾個用 `cfg` 分。
- 前綴（`MACHOPO_`、`MIMALLOC_`、`LC_`）照前綴比，其他照名字比；Windows 上不分大小寫（`Path` 和 `PATH` 是同一個）。
- 清空和放回在啟動 renderer 的命令上做（`renderer_launcher`）：Linux 和 macOS 是 `env_clear` 再 `env`；Windows 的 `sunaba::windows::Command::from_std` 現在看不到 `env_clear`（標準庫的 `get_envs` 不說它），要讓它照清空的命令建（`sunaba` 多一個開關，或 `ipc` 直接建它的命令）。

### 哪些 renderer

| | 做法 | 說明 |
| --- | --- | --- |
| N1 | **sandbox 裡和 `--no-sandbox` 的 renderer process 都清** | 一條路；`--no-sandbox` 的測試跑的就是真的那條。`--no-sandbox` 是除錯用的，要多給一個變數時改清單 |
| N2 | 只清 sandbox 裡的 | 除錯時 renderer 拿到全部，方便；但兩條路，清單漏了的只在 sandbox 裡壞，和「sandbox 裡外比較」分不出是 sandbox 還是環境 |

定了 **N1**（已決定的事項第 4 條）。

### 測試

- **renderer 的環境只有清單上的**：Browser 帶著一個假的秘密（例如 `SOME_API_TOKEN`）和清單上的變數啟動，從外面讀真的 renderer process 的環境（Linux 的 `/proc/<pid>/environ`，renderer 不能被 dump 時改成在 probe 裡讀；macOS 的 `KERN_PROCARGS2`；Windows 的 process 的 PEB，或 probe），秘密不在、清單上的在、沒有清單以外的。
- **網頁看到的不變**：sandbox 裡外和 thread 上的 renderer 畫出同一頁的比較（三個平台的 `a_sandboxed_renderer_shows_a_page_as_one_outside_does`）加上 `Intl` 的預設語系、`Intl.NumberFormat` 的格式；另外用 `LANG=ja_JP.UTF-8`、`TZ=Asia/Tokyo` 啟動一次，renderer 看到的是它們（證明清單真的傳下去）。
- **開關照舊**：`MACHOPO_V8_FLAGS`、`RUST_LOG` 作用在 renderer process 上（現有的 trace 和 log 的測試在 process 的 renderer 上跑）。
- mutation test：清單拿掉一項、不清空、前綴比成全名。
- 量測工具的測試（`CLAUDE.md`：改了 renderer 怎麼啟動）。

### 分階段

0. 草稿和 Linux 的量測（這一版；`details.md`）。
1. **Linux**：清單和清空的機制（不分平台的部分在這一段做出來）；renderer process 切過去；測試。
2. **macOS**：量 renderer 讀了什麼（`getenv` 的 interpose：`DYLD_INSERT_LIBRARIES`，SIP 擋不擋自己 build 的執行檔要確認）、macOS 的那幾個；切過去。
3. **Windows**：`sunaba::windows::Command` 照清空的命令建；Chromium 的清單；量 renderer 讀了什麼（API Monitor 或 `GetEnvironmentVariableW` 的 hook）；切過去。

每一段在它的平台上做和驗，做完可以先 merge（`rfcs/README.md` 的「階段的 merge」）：還沒切的平台照舊繼承整份。

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

## 待決定

開始第 1 段之前要定的五件都定了（已決定的事項第 1 到 5 條）。

各平台那一段才定的：那個平台的系統要的變數（量過之後）；從外面讀 renderer 的環境的做法；Windows 上 `sunaba::windows::Command` 怎麼照清空的命令建。

## 之後的可能

- 語系和時區由 Browser 給（L2），連同 renderer 不再讀 `/etc/localtime` 和 zoneinfo：Linux 的 chroot 的前提。
- 清單也用在之後的其他 child process（GPU process、utility process），各自一份。
