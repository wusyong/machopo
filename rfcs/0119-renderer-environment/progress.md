# RFC 0119 的進度：renderer process 的環境變數只給它要的

開始日期：2026-11-27。完成日期：2026-11-27（Linux：Fedora 45，Linux 7.3.3，x86-64；macOS：26.6.2，arm64；Windows：Windows 11 10.0.26300，x86-64）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、Linux 的量測（`details.md`）、設計的五條決定 | 做完 | 2026-11-27 |
| 1a. 清單和清空的機制（`shell::renderer_sandbox` 的 `passes_to_renderer`、`renderer_environment`、`limit_environment`），Linux 的 renderer process（sandbox 裡外）切過去 | 做完 | 2026-11-27 |
| 1b. 測試：renderer 的環境只有清單上的；網頁看到的語系和時區照環境傳下去，和 thread 上的 renderer 相同 | 做完 | 2026-11-27 |
| 1c. 第 1 段的檢查和文件 | 做完 | 2026-11-27 |
| 2a. macOS 的量測（`getenv_log.c` 的 macOS 版）、這一段的三條決定 | 做完 | 2026-11-27 |
| 2b. macOS 的清單（`__CF_USER_TEXT_ENCODING`、`DYLD_INSERT_LIBRARIES`、`DYLD_LIBRARY_PATH`、`SAMPLY_BOOTSTRAP_SERVER_NAME`），renderer process（sandbox 裡外）切過去；`sunaba::macos::environment` | 做完 | 2026-11-27 |
| 2c. 第 2 段的測試、檢查和文件 | 做完 | 2026-11-27 |
| 3a. Windows 的量測（`getenv_trace.py`）、這一段的四條決定 | 做完 | 2026-11-27 |
| 3b. `sunaba::windows::Command::env_clear`、`RendererProcess::clear_environment` 和 ipc 的 `spawn_handing`；Windows 的清單（`SystemRoot`、`SystemDrive`，名字不分大小寫），renderer process（sandbox 裡外）切過去；`sunaba::windows::environment` | 做完 | 2026-11-27 |
| 3c. 第 3 段的測試、檢查和文件；RFC 改寫成完成時的樣子 | 做完 | 2026-11-27 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 清單 | 照名字和前綴放行，不認得的、名字的一部分、不是 Unicode 的不給；Linux 和 macOS 上大小寫不同的不給，Windows 上照樣給（`rust_log`、`Lang`、`Machopo_V8_Flags`、`mimalloc_purge_delay`）；平台的那幾個只在那個平台：Linux 的 `LD_PRELOAD`、`LD_LIBRARY_PATH`，macOS 的 `__CF_USER_TEXT_ENCODING`、`DYLD_INSERT_LIBRARIES`、`DYLD_LIBRARY_PATH`、`SAMPLY_BOOTSTRAP_SERVER_NAME`，Windows 的 `SystemRoot`、`SystemDrive`；哪裡都不給的：`TMPDIR`、`XPC_*`、`DYLD_FRAMEWORK_PATH`、`Malloc*`、`PATH`、`Path`、`TEMP`、`TMP`、`LOCALAPPDATA`、`USERPROFILE`、`APPDATA`、`SystemRootX` | `cargo test -p shell --lib renderer_sandbox::`（`a_renderer_is_given_the_variables_listed_and_no_other`、`a_renderers_environment_is_the_listed_part_of_the_browsers`） |
| renderer process 的環境 | sandbox 裡和 `--no-sandbox` 的 renderer process 的環境只有清單上的：Browser 的假秘密（`SOME_API_TOKEN`）、`HOME`（Windows：`USERPROFILE`、`PATH`、`TEMP`、`LOCALAPPDATA`）不在，`MACHOPO_` 開頭的、`LANG`、`TZ` 在；網頁看到的語系和時區和 thread 上的 renderer 相同（Linux、macOS 上是傳下去的 `ja-JP`、`Asia/Tokyo`）。Linux 從 `/proc/<pid>/environ` 讀，通道的 `MACHOPO_IPC_FD` 在；macOS 從 `KERN_PROCARGS2` 讀，`__CF_USER_TEXT_ENCODING` 在；Windows 從 PEB 讀，大小寫不同的 `Machopo_Test_Cased` 在，`SystemRoot`、`SystemDrive` 是 Browser 的值，renderer 拿到通道後拿掉的 `MACHOPO_IPC_CHANNEL` 不在 | `cargo test -p shell --test it`：`renderer_sandbox::`、`renderer_sandbox_macos::`、`renderer_sandbox_windows::a_renderer_process_has_only_the_listed_part_of_its_browsers_environment` |
| 網頁看到的不變 | 系統的語系和時區下，sandbox 裡、外和 thread 上的 renderer 看到的 `Intl` 的預設語系、數字的格式、時區相同（macOS 另比字的寬） | 三個平台的 `a_sandboxed_renderer_shows_a_page_as_one_outside_does` |
| 從外面讀環境 | `sunaba::macos::environment`：sandbox 裡的 process 啟動時的環境，值裡的空白和 `=` 照原樣，沒有別的；結束了的是 `None`；參數區的格式（補齊的 NUL、系統的字串、太短的、沒有參數的） | `cargo test -p sunaba`（`macos::a_sandboxed_processs_environment_is_seen_from_outside`、單元測試 2 個） |
| 從外面讀環境 | `sunaba::windows::environment`：sandbox 裡的 process 現在的環境，空的值照原樣，清空後設的以外沒有別的；結束了的是 `None`；環境的區塊讀到第一個空字串為止，名字的第一個字元可以是 `=`；`sunaba::windows::Command::env_clear` 之後只有之後設的 | `cargo test -p sunaba`（`windows::a_sandboxed_processs_environment_is_seen_from_outside`、單元測試 2 個） |

## 做過的檢查

- **量的**（`details.md`）：Linux 和 macOS 用 `getenv_log.c`（`LD_PRELOAD`、dyld 的 interpose），Windows 用 `getenv_trace.py`（debugger 在讀環境的 export 上下 breakpoint，已決定的事項第 10 條），整套 `shell` 的測試在它們底下跑，清單打開前後各一次：打開之後 renderer 讀的變數沒有新的，清單以外的只有強制 CPU 的路的除錯開關（`SIMDUTF_FORCE_IMPLEMENTATION`、`OPENSSL_*cap`）、系統框架和 ntdll 平常沒有值的除錯開關、macOS 的 `USER`、Windows 的 loader 啟動時的 `PATH`。沒有語系的變數時，Linux 上 thread 上的 renderer 的 `Intl` 的預設語系是 `und`（所以漏了它們會安靜地變）。
  - Linux 那一輪 `automation_pipe::without_its_pipe_machopo_says_so` 失敗，是 shim 自己造成的（它載入時開的 log 檔佔了 fd 3，那個測試看的就是 fd 3 上沒有東西）；不帶 shim 時通過。macOS 上 SIP 從系統的執行檔的環境拿掉 `DYLD_*`，所以在它裡面才設。
  - Windows 上在 debugger 底下有 34 個整合測試逾時（debugger 慢下來，不是環境；它們的 renderer 照樣啟動、量到了），不在 debugger 底下都通過；從外面讀 renderer 的環境，讀了的 252 個都讀到，沒有失敗。debugger 一開始在單步時暫停其他 thread，兩條 thread 同時碰到 breakpoint 時互相等住、整套停下，改成不暫停（代價：單步的那一下別的 thread 經過同一個 export 時會漏，只在那時讀一次的名字量不到）。清單打開、一個 Windows 系統的變數都不給時，整套 `cargo test -p shell` 也通過（所以 `SystemRoot`、`SystemDrive` 是預防，已決定的事項第 13 條）。
- **找到的**：macOS 上量測工具的測試 `speedometer_test.py` 的 `test_the_stacks_in_the_timed_windows` 在清單打開後失敗：samply 靠 `SAMPLY_BOOTSTRAP_SERVER_NAME` 拿 renderer 的 task port；清單加上它（已決定的事項第 9 條）之後通過。
- **Mutation test**（`--keep-going`；檔案每次都還原了：`git status`，`target/mutate/originals/` 不在）：
  - `mutants-1.toml`（Linux，9 個）：全部抓到：不清空環境、Linux 不清、啟動的一方不叫它、前綴照全名比、不給 `LANG`、`TZ`、`MACHOPO_` 開頭的、`LC_` 開頭的、不是 Unicode 的名字也給。
  - `mutants-2.toml`（macOS，15 個）：14 個抓到：macOS 不清、不清空、啟動的一方不叫它、不給 `__CF_USER_TEXT_ENCODING`、`DYLD_INSERT_LIBRARIES`、`DYLD_LIBRARY_PATH`、`SAMPLY_BOOTSTRAP_SERVER_NAME`、`LANG`、`TZ`；`environment_in` 把補齊的 NUL 當參數、少跳一個參數、把系統的字串當變數、照最後一個 `=` 分、讀沒有參數的答案。漏的一個是等價的：不把 buffer 截到 `sysctl` 說寫了多少。
  - `mutants-3.toml`（Windows，14 個）：13 個抓到：Windows 不清、啟動的一方不叫 ipc 清空、ipc 不清空、清空了不放回命令設的、`sunaba` 的 `env_clear` 什麼都不清（`shell` 和 `sunaba` 的測試各抓到一次）、不給 `SystemRoot`、`SystemDrive`、名字照大小寫比、前綴照全名比；`environment_in` 跳過空字串讀下去、名字在第一個字元的 `=` 就結束，環境的區塊晚一個字元讀。漏的一個是等價的：不確認 `ReadProcessMemory` 讀滿了。
  - 兩個等價的記在 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。三份的「這個平台不清」的 `from` 跟著 `LIMITS_ENVIRONMENT` 改到最後的樣子。
- **沒列成 mutant 的**：`renderer_environment` 照順序、不改值（單元測試直接比）；`limit_environment` 在 `envs` 之後還有 `ipc` 加的通道變數（Linux、macOS 的整合測試看它在；Windows 的看它被 renderer 拿掉了）。
- **跑過的檢查**：
  - Linux：`cargo test -p shell`（單元 99、`machopo` 13、整合 761）；量測工具的測試（`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py`、`census_test.py`）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個 binary，4979 個通過，1 個 ignored）。都通過。
  - macOS：`cargo test -p shell`（單元 100、`machopo` 13、整合 760）；`cargo test -p sunaba`（單元 2、整合 19）；量測工具的測試（`speedometer_test.py` 126 個，跳過 4 個，其他三個）；`sanity.py` 同上；`cargo fmt --check`、clippy、`cargo test --workspace`（63 個 binary，4998 個通過，1 個 ignored）。都通過。
  - Windows：`cargo test -p sunaba`（單元 7、整合 22）；量測工具的測試（`speedometer_test.py` 125 個，跳過 7 個；`blink_perf_test.py` 37 個，跳過 2 個；`raster_test.py` 13 個，跳過 1 個；`census_test.py` 5 個）；`sanity.py` 同上；`cargo test --workspace`（63 個 binary，4999 個通過，2 個 ignored，`shell` 是單元 104、整合 753）。`cargo fmt --check` 改了三個檔案的換行（mutant 的每個 `from` 都還在），clippy 抓到 `sunaba::windows::environment` 用 `chunks_exact(2)`（改成 `as_chunks`），改了之後重跑 clippy、`cargo test -p sunaba` 和 `shell` 的 `renderer_sandbox` 的測試。都通過。
