# RFC 0121 的進度：Windows 的 renderer sandbox 的下一級：restricted token 和另一個 desktop

開始日期：2026-11-27（Windows 11，10.0.26300，x86-64）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、探測（`details.md`）、第 1 段的五條決定 | 做完 | 2026-11-27 |
| 1a. `sunaba`：lockdown 加 AppContainer 的 primary token、第一個 thread 帶 same-access 加 AppContainer 的 token、process 暫停著建、`LoaderThreads` 寫成 1；`lower` 要一次亂數、還回 token；延後載入失敗的 hook；從外面讀 token（`AccessToken`）和 `loader_threads` | 做完 | 2026-11-27 |
| 1b. 測試（斷言舊行為的改過來）、mutation test、第 1 段的檢查和文件 | 做完 | 2026-11-27 |
| 2 之前. 探測（`details.md` 的「探測：desktop 和記憶體上限（第 2 段之前）」）、第 2 段的決定（已決定的事項第 6 到 9 條） | 做完 | 2026-11-27 |
| 2a. desktop：`sunaba` 第一次照 policy 啟動時建 window station 和它上面的 desktop、之後的都用它（允許 win32k 的不用）；從外面讀 `DesktopInfo`（`desktop`）；`sunaba_probe` 延後載入 `user32`；測試 | 做完 | 2026-11-27 |
| 2b. job 的記憶體上限（實體記憶體的一半，最少 4 GB）、job 的 completion port、Browser 存的結束帶「碰過記憶體上限」、sad tab 的「記憶體不夠」；測試 | 做完 | 2026-11-27 |
| 2c. V8 的 OOM：`bindings` 設 `set_oom_error_handler`、固定的結束碼、sad tab 對到「記憶體不夠」（三個平台）；測試 | 還沒開始 | |
| 2d. 第 2 段的檢查和文件 | 還沒開始 | |
| 3. 探測的三件（handle、CSRSS、shim） | 還沒開始 | |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| token | policy 底下的 process 的 token 從外面讀：使用者和每個 group（integrity 以外）都只能拒絕、限制在 null SID、是自己的 AppContainer 的；沒有 policy 的不是 | `cargo test -p sunaba --test it windows::`（`the_process_has_a_lockdown_app_containers_token`）；`cargo test -p shell --test it renderer_sandbox_windows::`（`a_renderer_process_is_in_the_sandbox`、`a_renderer_process_told_no_sandbox_is_in_none`、`a_browsers_renderer_is_in_the_sandbox_unless_the_browser_is_told_otherwise`） |
| token | 第一個 thread 降權之前讀得到自己的 token、系統的檔案（DLL 就是這樣載入的），其他 thread 一開始就讀不到；降權之後哪個 thread 都讀不到 | `sunaba` 的 `a_process_under_the_policy_is_in_an_app_container`、`nothing_of_the_systems_is_read_once_the_process_has_lowered_itself`；`shell` 的 `the_probe_is_started_as_a_renderer_is_and_lowers_itself` |
| loader | policy 底下的 process 的 `LoaderThreads` 是 1，沒有 policy 的是 0；renderer process 的也是 1 | `sunaba` 的 `the_loader_of_a_process_under_the_policy_has_one_thread`；`shell` 的 `a_renderer_process_is_in_the_sandbox` |
| 讀不到的 | 降權之後讀不到 `kernel32.dll`、`win.ini`，列不了 `Fonts`、`Program Files`，`stat` 不了 `cmd.exe`；啟動不了程式（開不了執行檔；開得了時是 child process 的限制） | `sunaba` 的 `nothing_of_the_systems_is_read_once_the_process_has_lowered_itself`、`no_program_is_started`；`shell` 的 `a_renderer_reads_nothing_of_the_systems`、`a_renderer_reaches_no_network_starts_no_program_and_opens_no_process` |
| socket | Winsock 起不來：連外面、連本機、聽本機的 port 都是 `WSAStartup` 失敗（10107），本機的 listener 沒收到連線 | `sunaba` 的 `no_socket_is_made`、`nothing_is_connected_to_on_the_machine` |
| default DACL | 從外面讀：都是允許的項目，有使用者和它自己的 AppContainer，沒有 logon session、RESTRICTED、ALL APPLICATION PACKAGES；另一個 policy 底下的 process 用 lockdown 的 token 或啟動用的 token 都開不了它的 process（安全描述來自它的 default DACL），使用者的一般 process 開得了 | `sunaba` 的 `the_process_has_a_lockdown_app_containers_token`、`what_a_process_makes_another_under_a_policy_cant_open`；`shell` 的 `a_renderer_process_is_in_the_sandbox` |
| desktop | policy 底下的 process 在 starter 建的 window station（logon session 的沒有名字的那一個）和 desktop（`sunaba-<starter 的 PID>`）上，同一個 starter 的都在同一個上；沒有 policy 的、允許 win32k 的在 starter 的上；建完之後 starter 自己的 window station 不變；renderer 在 Browser 建的上，`--no-sandbox` 的不在 | `sunaba` 的 `a_process_under_the_policy_is_on_a_desktop_its_starter_made`、`making_the_desktop_leaves_this_process_on_its_own_window_station`；`shell` 的 `a_renderer_process_is_in_the_sandbox`、`a_renderer_process_told_no_sandbox_is_in_none`、`a_browsers_renderer_is_in_the_sandbox_unless_the_browser_is_told_otherwise` |
| 延後載入 | renderer 降權之後碰到延後載入的 DLL（`ws2_32`）時以 `0xC06D007E` 結束，stderr 有一行說是哪一個 DLL、它在 sandbox 裡降權之後載不進來 | `shell` 的 `a_renderer_ends_on_a_dll_it_cant_load_and_says_which` |
| 網頁看到的 | sandbox 裡、外和 thread 上的 renderer 畫的系統字型、測試字型、時區和 `Intl` 相同；wasm 的文件在 sandbox 裡和 thread 上畫的相同；整套 `shell` 的測試在 lockdown 的 renderer 上通過 | `shell` 的 `a_sandboxed_renderer_shows_a_page_as_one_outside_does`、`a_sandboxed_renderer_runs_a_wasm_document_as_one_outside_does`、`a_renderer_process_has_the_systems_fonts_from_its_browser`；`cargo test -p shell` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 0（2026-11-27，Windows 11，10.0.26300，x86-64）

- 探測和 Chromium 的做法：`details.md`。

### 1a、1b（2026-11-27，同一台）

- **做法**：原型（只為探測寫的，沒有 commit）改寫成 `sunaba` 的 `Tokens`（lockdown 和 same-access 的 restricted token 各從 Browser 的 token 建，再各做成同一個 AppContainer 的 token，後者複製成 impersonation token）、`start`（暫停著建的 process：寫 `LoaderThreads`、給第一個 thread token、讓它跑）、`lower` 最後要一次亂數、`RevertToSelf`。寫 `LoaderThreads` 之前先讀參數的結構的長度，比那個位置短時不寫、回錯誤。延後載入失敗的 hook 是 `sunaba` 的 `#[no_mangle]` 的 `__pfnDliFailureHook2`；`delayimp.lib` 的預設的（null）在它自己的 object 裡，machopo 的執行檔連結時用的是 `sunaba` 的（`shell` 的測試看到那一行）。
- **probe 的改變**：降權之後 Winsock 起不來，標準庫第一次建 socket 時 `WSAStartup` 失敗就 panic（結束碼 101、沒有錯誤碼）：probe 的 `connect`、`listen` 先自己叫 `WSAStartup`，印出它的錯誤碼（10107）。machopo 的執行檔延後載入 `ws2_32`，同樣的嘗試在它裡面走到延後載入失敗的 hook。
- **斷言舊行為、改過來的測試**：`sunaba` 的「讀得到所有應用程式能讀的」改成讀不到（降權之前的第一個 thread 讀得到）；從裡面讀自己的 token（是不是 AppContainer、integrity）改成從外面讀，裡面讀被拒絕；連線的 `WSAEACCES` 改成 Winsock 起不來；啟動程式的 `ERROR_CHILD_PROCESS_BLOCKED` 改成開不了執行檔（`--no-lower` 時照樣是 child process 的限制）。`shell` 的對應的四個一樣改。
- **新的測試**：`sunaba` 的整合測試 4 個（從外面讀 token 和 default DACL、另一個 policy 底下的 process 開不了它的 process、`LoaderThreads`、沒有 socket），改寫 5 個；`shell` 的 `renderer_sandbox_windows` 2 個（延後載入的 DLL 和 stderr 的那一行、wasm 的文件在 sandbox 裡外畫的相同），另外 3 個加上 lockdown 的 token 和 `LoaderThreads` 的檢查。
- **Mutation test**（`mutants-1.toml`，15 個，`--keep-going`）：抓到 14 個：primary token 不從 lockdown 的建（`sunaba` 的和 machopo 的 renderer 各一次）、lockdown 的 group 不只拒絕、不限制在 null SID、第一個 thread 的 token 不是 restricted 的、第一個 thread 不給 token、loader 照舊平行、不還回 token、讀 group 時看錯位元、讀不出 logon session、default DACL 的 SID 不讀、probe 不自己起 Winsock、沒有延後載入失敗的 hook、hook 不知道已經降權。沒抓到的 1 個是預期的等價：不暖機（已決定的事項第 4 條；記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）。跑完確認 `sunaba` 的檔案都還原了。
- **補不了測試的**：寫 `LoaderThreads` 前的長度檢查、hook 對「函式找不到」的說法：記進 `docs/test-gaps.md` 的「只靠審查」。
- **順手修的**：`shell` 的測試的 `Browser::start_wrapped` 只有 Linux 的測試用到，Windows 上 workspace 的 clippy 因為 dead code 失敗（不是這一段造成的）：不是 Linux 時 `expect(dead_code)`。
- **跑過的檢查**：`cargo test -p sunaba`（整合測試 25 個，都通過）；`cargo test -p shell -p ipc`（都通過）；`cargo test -p shell --test it renderer_sandbox_windows`（13 個，都通過）；`cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（都通過）；量測工具的測試（`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py`、`census_test.py`，都通過）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs（都通過）。

### 2a（2026-11-27，同一台）

- **做法**：探測的原型（沒有 commit）改寫成 `sunaba` 的 `alternate_desktop`：一個 `Mutex<Option<Vec<u16>>>` 存名字，第一次照 policy 啟動時建（`CreateWindowStationW` 不給名字，暫時 `SetProcessWindowStation` 過去，`CreateDesktopW` 名字是 `sunaba-<PID>`，再換回來），handle 不關；建不了時 `spawn` 回錯誤、不啟動，下一次再試。允許 win32k 的 process 不放上去。`desktop(id)` 讀 process 的參數的 `DesktopInfo`（x64 的 `+ 0xC0`）。window station 的 handle 要 `GENERIC_READ` 才讀得到名字（只給 `WINSTA_CREATEDESKTOP` 時 `GetUserObjectInformationW` 被拒，`sunaba` 的測試全部失敗）：照 Chromium 給 `GENERIC_READ | WINSTA_CREATEDESKTOP`。名字在建 desktop 之前讀，讀不到時不留下 desktop。
- **`user32` 的延後載入**：`sunaba` 現在呼叫 `user32` 的函式，連結它的 `sunaba_probe` 啟動時就載入 `user32`，關著 win32k 的 probe 起不來；`sunaba` 加了 `build.rs`，讓 `sunaba_probe` 和 machopo 的執行檔一樣 `/DELAYLOAD:user32.dll`。改了之後 `sunaba` 原有的測試照樣通過。
- **新的測試**：`sunaba` 的整合測試 1 個（兩個 policy 底下的 process 在同一個 starter 建的 desktop 上，名字的形式；沒有 policy 的、允許 win32k 的不在上面）、unit test 1 個（建完之後 starter 的 window station 不變、只建一次）；`shell` 的 `renderer_sandbox_windows` 的 3 個加上 desktop 的檢查（測試的 process 啟動的 renderer 在它建的上；Browser 啟動的在 `sunaba-<Browser 的 PID>` 上；`--no-sandbox` 的不在）。
- **Mutation test**（`mutants-2a.toml`，9 個，`--keep-going`）：都抓到：不給 `lpDesktop`（`sunaba` 的和 machopo 的 renderer 各一次）、允許 win32k 的也放上去、不換回 starter 的 window station、不存起來給下一次、desktop 的名字不帶 PID、`alternate_desktop_name` 說沒有、`DesktopInfo` 讀錯位置、讀成它的最大長度。跑完確認 `windows.rs` 都還原了。
- **補不了測試的**：另一個 desktop 的 DACL 擋住連上 win32k 的 process（`sunaba` 不把允許 win32k 的 process 放上去，沒有 process 是兩者都有的；探測時看過）：記進 `docs/test-gaps.md` 的「只做過手動確認」。
- **跑過的檢查**：`cargo test -p sunaba -p ipc -p shell`（`sunaba` 的 unit test 8 個、整合測試 26 個，`shell` 的整合測試 755 個，都通過）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs（都通過）。`cargo test --workspace`、fmt 和 clippy 等第 2 段做完時跑。

### 2b（2026-11-27，同一台）

- **做法**：`sunaba` 的 `Policy::limit_memory` 讓 job 帶 `JOB_OBJECT_LIMIT_PROCESS_MEMORY`（每個 process 能 commit 的上限），有上限時 job 接一個 completion port；`Child::exceeded_memory_limit` 把 port 上的通知收完（不等），看過 `JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT` 就記住。`browser` 存的 renderer 的結束從 `ExitStatus` 改成 `ChildProcessTerminationInfo`（結束碼和「碰過上限」），在 process 結束之後問。`shell` 的 renderer 的上限是實體記憶體的一半、最少 4 GB（`physical_memory` 讀 `GlobalMemoryStatusEx`），sad tab 多一種「記憶體不夠」，碰過上限的不管結束碼是什麼都說是它。
- **新的測試**：`sunaba` 的整合測試 1 個（64 MB 的上限底下要 256 MB 被拒、starter 被告知；16 MB 的照常拿到、沒有告知；沒有上限的拿得到 256 MB；`sunaba_probe` 多了 `allocate` 的嘗試和 `--memory-limit`）；`browser` 的整合測試 1 個（256 MiB 的上限底下一直要記憶體的 renderer 結束時帶「碰過上限」，同樣上限的 `about:crash` 不帶）；`shell` 的 unit test 2 個（上限的算法和 renderer 的 policy 帶著它；sad tab 照「碰過上限」說，不管結束碼）。
- **Mutation test**（`mutants-2b.toml`，10 個，`--keep-going`）：抓到 9 個：job 不設上限、有上限的 job 沒有 port、每則通知都當成碰到上限、只讀第一則通知、Browser 不問、sad tab 不看「碰過上限」、renderer 不設上限、上限是全部的實體記憶體、上限是最多 4 GB。沒抓到 1 個：`browser_view` 只把結束碼交給 sad tab（`shell` 沒有讓分頁的 renderer 碰到上限的測試）：記進 `docs/test-gaps.md` 的「只靠審查」。跑完確認改過的檔案都還原了。
- **一次失敗的整套**：第一次跑 `shell` 的整合測試時，`automation_pipe` 的 12 個在同一刻等滿 30 秒逾時（`machopo answers`），其他 743 個通過；那時這台機器正在跑 VR 遊戲（VR 的 compositor 佔住 GPU），這幾個測試每一格都等 wgpu。單獨跑、只跑 `automation_pipe::`、再跑整套兩次（一次在重新 link 之後，排除剛 link 的執行檔被掃描）都通過。GPU 被別的程式佔住時，碰 GPU 的測試的結果不能用。
- **跑過的檢查**：`cargo test -p sunaba -p ipc -p browser -p shell`（`shell` 的整合測試 755 個，和其他的都通過，除了上面那一次）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs（都通過）。`cargo test --workspace`、fmt 和 clippy 等第 2 段做完時跑。
