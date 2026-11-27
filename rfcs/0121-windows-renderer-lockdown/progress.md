# RFC 0121 的進度：Windows 的 renderer sandbox 的下一級：restricted token 和另一個 desktop

開始日期：2026-11-27（Windows 11，10.0.26300，x86-64）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、探測（`details.md`）、第 1 段的五條決定 | 做完 | 2026-11-27 |
| 1a. `sunaba`：lockdown 加 AppContainer 的 primary token、第一個 thread 帶 same-access 加 AppContainer 的 token、process 暫停著建、`LoaderThreads` 寫成 1；`lower` 要一次亂數、還回 token；延後載入失敗的 hook；從外面讀 token（`AccessToken`）和 `loader_threads` | 做完 | 2026-11-27 |
| 1b. 測試（斷言舊行為的改過來）、mutation test、第 1 段的檢查和文件 | 做完 | 2026-11-27 |
| 2. desktop 和記憶體上限 | 還沒開始 | |
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
