# RFC 0121 的進度：Windows 的 renderer sandbox 的下一級：restricted token 和另一個 desktop

開始日期：2026-11-27。完成日期：2026-11-27（Windows 11，10.0.26300，x86-64；三段都在這台上做和驗。Windows 10 和 ARM64 沒試；V8 的 OOM 改到的 Linux、macOS 由 CI 的測試看）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、探測（`details.md`）、第 1 段的五條決定 | 做完 | 2026-11-27 |
| 1a. `sunaba`：lockdown 加 AppContainer 的 primary token、第一個 thread 帶 same-access 加 AppContainer 的 token、process 暫停著建、`LoaderThreads` 寫成 1；`lower` 要一次亂數、還回 token；延後載入失敗的 hook；從外面讀 token（`AccessToken`）和 `loader_threads` | 做完 | 2026-11-27 |
| 1b. 測試（斷言舊行為的改過來）、mutation test、第 1 段的檢查和文件 | 做完 | 2026-11-27 |
| 2 之前. 探測（`details.md` 的「探測：desktop 和記憶體上限（第 2 段之前）」）、第 2 段的決定（已決定的事項第 6 到 9 條） | 做完 | 2026-11-27 |
| 2a. desktop：`sunaba` 第一次照 policy 啟動時建 window station 和它上面的 desktop、之後的都用它（允許 win32k 的不用）；從外面讀 `DesktopInfo`（`desktop`）；`sunaba_probe` 延後載入 `user32`；測試 | 做完 | 2026-11-27 |
| 2b. job 的記憶體上限（實體記憶體的一半，最少 4 GB）、job 的 completion port、Browser 存的結束帶「碰過記憶體上限」、sad tab 的「記憶體不夠」；測試 | 做完 | 2026-11-27 |
| 2c. V8 的 OOM：`bindings` 設 `set_oom_error_handler`、固定的結束碼、sad tab 對到「記憶體不夠」（三個平台）；測試 | 做完 | 2026-11-27 |
| 2d. 第 2 段的檢查和文件 | 做完 | 2026-11-27 |
| 3 之前. 探測（`details.md` 的「探測：handle、CSRSS、shim（第 3 段之前）」）、第 3 段的決定（已決定的事項第 11 條） | 做完 | 2026-11-27 |
| 3a. 降權時關掉 `\Device\KsecDD`、空出的位置放 event 的複本（`sunaba` 的 `windows/handles.rs`）；從外面列 process 的 handle（`handles`）；測試 | 做完 | 2026-11-27 |
| 3b. 建立時把 `pShimData` 寫成 0（x64 在 ARM64 上模擬時不寫）、開了 CIG 時加 module tampering protection；從外面讀 `pShimData`（`shim_data`）；測試 | 做完 | 2026-11-27 |
| 3c. 第 3 段的檢查和文件；renderer process 的 KsecDD 和 `pShimData` 加進 `shell` 的測試 | 做完 | 2026-11-27 |

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
| 記憶體上限 | policy 帶上限時，process 要超過它的記憶體被拒、starter 從 job 知道它碰過上限；上限以內的照常拿到、不告知；沒有上限的不限 | `cargo test -p sunaba --test it windows::`（`a_process_with_a_memory_limit_is_refused_more_and_its_starter_told`） |
| 記憶體上限 | renderer 的上限是實體記憶體的一半、最少 4 GB，renderer 的 policy 帶著它 | `cargo test -p shell --lib renderer_sandbox`（`a_renderer_may_commit_half_the_machines_memory_and_4_gib_at_least`） |
| 記憶體上限 | Browser 存下的 renderer 的結束帶「碰過上限」：一直要記憶體的 renderer 有，同樣上限底下 `about:crash` 的沒有 | `cargo test -p browser --test it renderer_process::`（`a_renderer_process_that_ran_out_of_its_memory_is_told_apart`） |
| V8 的 OOM | V8 用完 heap 時 process 當場以結束碼 6 結束（緩衝裡的不寫出、測試框架的結尾不出現），stderr 有一行說是 heap、V8 給的位置 | `cargo test -p bindings --test it out_of_memory::`（`a_script_that_runs_v8_out_of_its_heap_ends_its_process_saying_so`） |
| sad tab | 碰過上限的（不管結束碼）、以結束碼 6 結束的都說記憶體不夠，字和當掉的不同；sandbox 進不去、DLL 載不進來的照舊 | `cargo test -p shell --lib sad_tab`（`a_renderer_that_ran_out_of_memory_is_told_apart_whatever_it_exited_with`、`a_renderer_that_couldnt_enter_its_sandbox_is_told_apart_from_a_crash`） |
| handle | 降權之後沒有 `\Device\KsecDD` 的 handle，空出的位置是同一個 event 的複本，亂數照常；policy 底下要過亂數、沒有降權的 process 開著它；renderer process 也沒有 | `cargo test -p sunaba --test it windows::`（`a_process_that_lowered_itself_has_no_handle_to_the_kernels_security_device`）；`shell` 的 `a_renderer_process_is_in_the_sandbox` |
| shim | policy 底下的 process 的 `pShimData` 是 0（沒有 policy 的讀得到）；renderer process 的也是 | `sunaba` 的 `a_process_under_the_policy_is_given_no_compatibility_shim`；`shell` 的 `a_renderer_process_is_in_the_sandbox` |
| mitigation | 只載入 Microsoft 簽章的 DLL 時，建立的 mitigation 帶 module tampering protection，允許別人的 DLL 時不帶 | `cargo test -p sunaba --lib`（`a_policy_asks_for_every_mitigation_but_what_it_was_told_to_allow`） |
| 探測的三件 | handle、CSRSS、shim 和另外查的 module tampering protection、registry 的快取各有結論：做了的有上面的測試，不做的（斷開 CSRSS、清 registry 的快取）寫了理由 | 已決定的事項第 11 條；`details.md` 的「探測：handle、CSRSS、shim（第 3 段之前）」 |

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

### 2a 到 2d（2026-11-27，同一台）

- **做法**：探測的原型（沒有 commit）改寫成 `sunaba` 的 `alternate_desktop`：一個 `Mutex<Option<Vec<u16>>>` 存名字，第一次照 policy 啟動時建（`CreateWindowStationW` 不給名字，暫時 `SetProcessWindowStation` 過去，`CreateDesktopW` 名字是 `sunaba-<PID>`，再換回來），handle 不關；建不了時 `spawn` 回錯誤、不啟動，下一次再試；允許 win32k 的 process 不放上去。window station 的 handle 照 Chromium 給 `GENERIC_READ | WINSTA_CREATEDESKTOP`（只給後者時讀不到名字，`sunaba` 的測試全部失敗）。`desktop(id)` 讀 process 的參數的 `DesktopInfo`（x64 的 `+ 0xC0`）。`Policy::limit_memory` 讓 job 帶 `JOB_OBJECT_LIMIT_PROCESS_MEMORY` 和一個 completion port，`Child::exceeded_memory_limit` 不等待地收完 port 上的通知；`browser` 存的 renderer 的結束改成 `ChildProcessTerminationInfo`（結束碼和「碰過上限」）；`shell` 的上限是實體記憶體的一半、最少 4 GB，sad tab 多一種「記憶體不夠」。查了 Chromium（`gin/v8_initializer.cc`、`gin/isolate_holder.cc`、Blink 的 `v8_oom.cc`）之後，`bindings` 的 `oom` 在每個 isolate 上設 `set_oom_error_handler`，往 stderr 分段寫一行（不配置記憶體），以 `OUT_OF_MEMORY_EXIT`（6）當場結束，sad tab 照它說記憶體不夠（已決定的事項第 10 條）。
- **`user32` 的延後載入**：`sunaba` 呼叫 `user32` 之後，連結它的 `sunaba_probe` 啟動時就載入 `user32`，關著 win32k 起不來；`sunaba` 加了 `build.rs`，讓 `sunaba_probe` 和 machopo 的執行檔一樣 `/DELAYLOAD:user32.dll`。
- **新的測試**：`sunaba` 的整合測試 2 個（policy 底下的 process 在 starter 建的 desktop 上、沒有 policy 的和允許 win32k 的不在；有上限時多要的被拒、starter 被告知，以內的和沒有上限的照常）、unit test 1 個（建完之後 starter 的 window station 不變、只建一次）；`browser` 的整合測試 1 個（一直要記憶體的 renderer 結束時帶「碰過上限」，`about:crash` 不帶）；`bindings` 的整合測試 1 個（`--max-old-space-size=16` 底下一直配置的 script：結束碼 6、stderr 那一行、緩衝裡的字沒有寫出來）；`shell` 的 unit test（上限的算法、sad tab 照「碰過上限」和結束碼 6 說），`renderer_sandbox_windows` 的 3 個加上 desktop 的檢查。
- **Mutation test**（`mutants-2a.toml` 9 個、`mutants-2b.toml` 10 個、`mutants-2c.toml` 8 個，都 `--keep-going`）：2a 全部抓到（不給 `lpDesktop`、允許 win32k 的也放上去、不換回、不存、名字不帶 PID、讀錯位置等）。2b 抓到 9 個，沒抓到的是 `browser_view` 只把結束碼交給 sad tab（`shell` 沒有讓分頁的 renderer 碰到上限的測試）。2c 抓到 6 個；照一般的方式結束（`std::process::exit`）的那一個補了 stdout 緩衝的檢查之後單獨再跑抓到，不寫 V8 的細節的那一個測不到（heap 用完時 V8 不給細節）。沒抓到的記進 `docs/test-gaps.md` 的「只靠審查」；另一個 desktop 的 DACL 擋住連上 win32k 的 process 記進「只做過手動確認」。每次跑完確認檔案都還原了。
- **一次失敗的整套**：2b 第一次跑 `shell` 的整合測試時，`automation_pipe` 的 12 個在同一刻等滿 30 秒逾時，其他 743 個通過；那時這台機器正在跑 VR 遊戲（VR 的 compositor 佔住 GPU），這幾個測試每一格都等 wgpu。單獨跑、只跑 `automation_pipe::`、再跑整套兩次（一次在重新 link 之後）都通過。GPU 被別的程式佔住時，碰 GPU 的測試的結果不能用。
- **情境**：沒有加。「記憶體不夠」的 sad tab 是 Browser 畫的，不在 DOM 裡，情境也沒有辦法給 renderer 一個小的 heap；`--window` 的那時沒跑（GPU 被 VR 遊戲佔住）。字和當掉的不同由 `sad_tab` 的 unit test 守著。
- **跑過的檢查**：各段的 `cargo test -p sunaba -p ipc -p browser -p bindings -p dom_js -p shell`（都通過，除了上面那一次）；`sanity.py` 的七項；第 2 段做完的 `cargo fmt --check`（2b 的三個檔案的排版，改了）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個測試 binary，5012 個通過、2 個 ignored，沒有失敗）。

### 3a 到 3c（2026-11-27，同一台）

- **做法**：照已決定的事項第 11 條。`lower` 在要過一次亂數之後、開嚴格的 handle 檢查之前，列出自己的 handle（`ProcessHandleInformation`），問每個的種類和名字（pipe 不問名字：會等著別人在讀的那一次），關掉 `\Device\KsecDD` 的；每關一個，讓 thread 留在同一個處理器上，複製一個 event 的 handle 直到複本落在空出的位置（Chromium 的 `AttemptToStuffHandleSlot`），event 和那個複本留到 process 結束。查了 Chromium（`sandbox_policy_base.cc` 的 `ApplyZeroAppShimToSuspendedProcess`、`sandbox_win.cc` 開它的條件、`process_mitigations.cc`）之後，`sunaba` 的 `start` 在寫 `LoaderThreads` 之後把 PEB 的 `pShimData`（`+0x2D8`）寫成 0，寫不了時不啟動，`IsWow64Process2` 說是模擬時不寫；沒有 `allow_third_party_modules` 時建立時的第二個 mitigation 字多一個 `MODULE_TAMPERING_PROTECTION_ALWAYS_ON`。`sunaba` 加上從外面看的 `handles(id)`、`shim_data(id)`，`sunaba_probe` 加上 `random` 的嘗試。
- **新的測試**：`sunaba` 的整合測試 2 個（要過亂數、沒有降權的 process 開著那個裝置，降權的沒有、有「裝置的 handle 數加一」個 handle 指到同一個 event、亂數照常；policy 底下的 process 的 `pShimData` 是 0，沒有 policy 的只斷言讀得到：別的機器上可能本來就是 0）；mitigation 的單元測試加上 bit 12、`allow_third_party_modules` 時沒有它；`shell` 的 `a_renderer_process_is_in_the_sandbox` 從外面看 renderer process 沒有 KsecDD 的 handle、`pShimData` 是 0。探測的腳本另外看了實際的 renderer：KsecDD 從 2 個變成 0 個，`pShimData` 從非 0 變成 0，沒有 `apphelp.dll`，頁面的 `crypto.getRandomValues`、`Math.random`、`crypto.randomUUID` 照常。
- **Mutation test**（`mutants-3a.toml` 11 個、`mutants-3b.toml` 8 個，`--keep-going`；`mutants-3c.toml` 2 個）：3a 抓到 8 個；沒抓到的 3 個（不拿掉不許關的保護、問 pipe 的名字、thread 不留在同一個處理器上）是測試的 process 沒有的情況，和「複本在空出的那個位置」（handle 的值每個 process 不一樣，從外面看不出來）一起記進 `docs/test-gaps.md` 的「只靠審查」。3b 抓到 7 個；寫失敗時不當成錯誤的那一個測不到（暫停著的 process 有全部的權限），和 module tampering protection 的效果、模擬時不寫的那條路一起記進「只靠審查」。3c 的兩個（renderer 降權時不關、建立時不寫 `pShimData`）由 `shell` 的測試抓到。每次跑完確認檔案都還原了。
- **整理**：第 2 段的三份 spec 當時只留在本機，補進這個資料夾；`mutants-1.toml` 的「降權前不要亂數」那一條照 `cargo fmt` 之後的排版更新，「loader 照舊平行」那一條照 3b 在中間加的 `no_app_shim` 更新（完成時的檢查才發現）。兩條重跑都抓到；七份的每一條都對得上程式。
- **不暖機不再是等價的**：第 1 段記成等價的那一個（不在降權前要一次亂數），重跑時由 3a 的 `a_process_that_lowered_itself_has_no_handle_to_the_kernels_security_device` 抓到（只用它跑也抓到）：KsecDD 的 handle 是暖機的亂數開的，沒有暖機就沒有 handle、沒有填進去的 event。從 `docs/test-gaps.md` 的「等價、不用處理的 mutation」和 `crates/sunaba/soundness.md` 的 Windows 沒有測試、靠審查的那一串拿掉。
- **不穩定的測試**：`automation_pipe::hung_renderers_take_the_limit_once`（Windows）在第 3 段做完的第一次整套裡失敗：「machopo 結束之後 renderer 都不在了」那一個 assert，`tasklist` 還列得到三個 pid 裡的某一個；單獨跑 10 次都通過。這是已知的（另一份 RFC 的「不穩定的測試」記過同一個 assert，推測是 pid 在 machopo 結束到測試問 `tasklist` 之間被別的測試的 machopo 重用）。這次另外確認了：結束了、但還有人拿著 handle 的 process，`tasklist` 不列，所以不是有人拿著 renderer 的 handle。這份沒有改到 renderer 怎麼結束；修法（不要事後用 pid 問系統）不在這份裡做。
- **跑過的檢查**：各段的 `cargo test -p sunaba -p shell`（都通過）；`sanity.py` 的七項；第 3 段做完的 `cargo fmt --check`（沒有差異）、`cargo clippy --workspace --all-targets -- -D warnings`（一處：讀 handle 的清單時的 `chunks_exact` 改成 `as_chunks`）、`cargo test --workspace`：第一次停在上面那個失敗，用 `--no-fail-fast` 再跑一次：63 個測試 binary，5014 個通過、2 個 ignored，沒有失敗。
