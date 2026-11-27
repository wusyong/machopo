# RFC 0119 的進度：renderer process 的環境變數只給它要的

開始日期：2026-11-27。

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

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 清單：照名字和前綴放行，不認得的、名字的一部分、大小寫不同的、不是 Unicode 的不給；`LD_PRELOAD` 只在 Linux | `cargo test -p shell --lib renderer_sandbox::`（`a_renderer_is_given_the_variables_listed_and_no_other`、`a_renderers_environment_is_the_listed_part_of_the_browsers`） |
| Linux：sandbox 裡和 `--no-sandbox` 的 renderer process 的環境（`/proc/<pid>/environ`）只有清單上的：Browser 的假秘密、`HOME` 不在，`MACHOPO_` 開頭的、`LANG`、`TZ`、通道的 `MACHOPO_IPC_FD` 在；網頁看到的語系和時區是傳下去的（`ja-JP`、`Asia/Tokyo`），和 thread 上的 renderer 相同 | `cargo test -p shell --test it renderer_sandbox::a_renderer_process_has_only_the_listed_part_of_its_browsers_environment` |
| Linux：系統的語系和時區下，sandbox 裡、外和 thread 上的 renderer 看到的 `Intl` 的預設語系、數字的格式、時區相同 | `renderer_sandbox::a_sandboxed_renderer_shows_a_page_as_one_outside_does` |
| 清單：macOS 上另給 `__CF_USER_TEXT_ENCODING`、`DYLD_INSERT_LIBRARIES`、`DYLD_LIBRARY_PATH`、`SAMPLY_BOOTSTRAP_SERVER_NAME`，不給 `TMPDIR`、`XPC_*`、`DYLD_FRAMEWORK_PATH`、`Malloc*` | `cargo test -p shell --lib renderer_sandbox::`（同上） |
| macOS：sandbox 裡和 `--no-sandbox` 的 renderer process 的環境（`KERN_PROCARGS2`）只有清單上的：假秘密、`HOME` 不在，`MACHOPO_` 開頭的、`LANG`、`TZ`、`__CF_USER_TEXT_ENCODING` 在；網頁看到的語系和時區是傳下去的，和 thread 上的 renderer 相同 | `cargo test -p shell --test it renderer_sandbox_macos::a_renderer_process_has_only_the_listed_part_of_its_browsers_environment` |
| macOS：系統的語系和時區下，sandbox 裡、外和 thread 上的 renderer 看到的字的寬、`Intl` 的預設語系、數字的格式、時區相同 | `renderer_sandbox_macos::a_sandboxed_renderer_shows_a_page_as_one_outside_does` |
| `sunaba::macos::environment`：sandbox 裡的 process 啟動時的環境，值裡的空白和 `=` 照原樣，沒有別的；結束了的是 `None`；參數區的格式（補齊的 NUL、系統的字串、太短的、沒有參數的） | `cargo test -p sunaba`（`macos::a_sandboxed_processs_environment_is_seen_from_outside`、單元測試 2 個） |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 0（2026-11-27，Fedora 45，Linux 7.3.3，x86-64）

- `getenv_log.c` 的量測、清空環境的 renderer 照樣啟動、Chromium 的做法：`details.md`。

### 1a 到 1c（2026-11-27，同一台）

- **新的測試**：`shell` 的單元測試 2 個（清單；從 Browser 的環境挑出 renderer 的）；`renderer_sandbox.rs` 的 `a_renderer_process_has_only_the_listed_part_of_its_browsers_environment`（Browser 帶著 `SOME_API_TOKEN`、`MACHOPO_TEST_SWITCH`、`LANG=ja_JP.UTF-8`、`TZ=Asia/Tokyo` 啟動：thread 上的 renderer 看到 `ja-JP`、`Asia/Tokyo`、-540，sandbox 裡外的 renderer process 看到的一樣；從 `/proc/<pid>/environ` 讀它們的環境，每個名字都在清單上，秘密和 `HOME` 不在，開關、語系、時區和通道的變數在）。`a_sandboxed_renderer_shows_a_page_as_one_outside_does` 多比 `Intl` 的預設語系和 `Intl.NumberFormat` 的格式。`automation_pipe.rs` 的 `Browser::start_in`、`renderer_pids` 改成 `pub(crate)`。
- **量到的**：
  - 整套 `shell` 的測試在 `getenv_log.c` 底下跑（清單已經打開；shim 改成沒有 `ENVPROBE_DIR` 時用編譯時給的目錄）：renderer 讀的變數和第 0 步量到的一樣，沒有新的，全部在清單上，只有 `SIMDUTF_FORCE_IMPLEMENTATION`、`OPENSSL_ia32cap` 不在（強制 CPU 的路的除錯開關，本來就沒有值；README 的「清單放什麼」）。那一輪 `automation_pipe::without_its_pipe_machopo_says_so` 失敗，是 shim 自己造成的：它載入時開的 log 檔佔了 fd 3，而那個測試看的就是「fd 3 上沒有東西」；不帶 shim 時通過。
  - 沒有語系的變數時（`LANG`、`LC_ALL` 是空的），thread 上的 renderer 的 `Intl` 的預設語系是 `und`，`new Date(0).toLocaleString()` 是 `1970-01-01 9:00:00 AM`；所以清單漏了它們的話，網頁看到的會安靜地變（README 原本寫的預期 `en-US-u-va-posix` 改成量到的）。
- **清單**比 README 的表多了 ICU 的資料位置（`ICU_DATA`、`ICU_TIMEZONE_FILES_DIR`：量到 renderer 讀，影響 `Intl`）；glibc 的 `GLIBC_TUNABLES`、`MALLOC_*` 沒給（用到時再加）。
- **Mutation test**（`mutants-1.toml`，9 個，`--keep-going`）：全部抓到：不清空環境、Linux 不清、啟動的一方不叫它、前綴照全名比、不給 `LANG`、不給 `TZ`、不給 `MACHOPO_` 開頭的、不給 `LC_` 開頭的、不是 Unicode 的名字也給。檔案都還原了（`git status`，`target/mutate/originals/` 不在）。
- **沒列成 mutant 的**：`renderer_environment` 照順序、不改值（單元測試直接比）；`limit_environment` 在 `envs` 之後還有 `ipc` 加的通道變數（`MACHOPO_IPC_FD` 在 renderer 的環境裡，整合測試看它）。
- **跑過的檢查**：`cargo test -p shell`（單元 99、`machopo` 13、整合 761，都通過）；量測工具的測試（renderer 的啟動變了：`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py`、`census_test.py`，都通過）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；第 1 段做完的 `cargo fmt --check`（改了兩個測試檔的換行，mutant 的每個 `from` 都還在）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個 binary，4979 個通過，1 個 ignored，沒有失敗）。

### 2a 到 2c（2026-11-27，macOS 26.6.2，arm64）

- **量的**：`getenv_log.c` 加了 macOS 的版本（dyld 的 interpose，`DYLD_INSERT_LIBRARIES` 載入）；整套 `shell` 的測試在它底下跑，清單打開前後各一次：`details.md` 的「renderer 讀了哪些變數（macOS，量的）」。SIP 從系統的執行檔（`caffeinate`）的環境拿掉 `DYLD_*`，所以在它裡面才設。打開之後 760 個整合測試都通過，renderer 讀的變數沒有新的。
- **新的測試**：`shell` 的清單的單元測試多了 macOS 的四個和不給的四個（`DYLD_FRAMEWORK_PATH`、`TMPDIR`、`XPC_FLAGS`、`MallocNanoZone`）；`renderer_sandbox_macos.rs` 的 `a_renderer_process_has_only_the_listed_part_of_its_browsers_environment`（同 Linux 的，多帶 `__CF_USER_TEXT_ENCODING`；從 `sunaba::macos::environment` 讀 renderer 的環境），`a_sandboxed_renderer_shows_a_page_as_one_outside_does` 多比 `Intl` 的預設語系和 `Intl.NumberFormat` 的格式；`sunaba` 的 `environment_in` 的單元測試 2 個和 `macos::a_sandboxed_processs_environment_is_seen_from_outside`。
- **找到的**：量測工具的測試 `speedometer_test.py` 的 `test_the_stacks_in_the_timed_windows` 在清單打開後失敗：samply 靠 `SAMPLY_BOOTSTRAP_SERVER_NAME` 拿 renderer 的 task port。清單加上它（已決定的事項第 9 條）之後通過。
- **Mutation test**（`mutants-2.toml`，15 個，`--keep-going`；第 15 個是加了 samply 的那一項之後單獨跑的）：14 個抓到：macOS 不清、不清空、啟動的一方不叫它、不給 `__CF_USER_TEXT_ENCODING`、`DYLD_INSERT_LIBRARIES`、`DYLD_LIBRARY_PATH`、`SAMPLY_BOOTSTRAP_SERVER_NAME`、`LANG`、`TZ`；`environment_in` 把補齊的 NUL 當參數、少跳一個參數、把系統的字串當變數、照最後一個 `=` 分、讀沒有參數的答案。漏的一個是等價的：不把 buffer 截到 `sysctl` 說寫了多少（記在 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）。檔案都還原了（`git status`，`target/mutate/originals/` 不在）。`mutants-1.toml` 的「Linux's renderers aren't limited」的 `from` 跟著 `LIMITS_ENVIRONMENT` 改了。
- **跑過的檢查**：`cargo test -p shell`（單元 100、`machopo` 13、整合 760，都通過）；`cargo test -p sunaba`（單元 2、整合 19）；量測工具的測試（`speedometer_test.py` 126 個，跳過 4 個；`blink_perf_test.py`、`raster_test.py`、`census_test.py`，都通過）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；第 2 段做完的 `cargo fmt --check`（改了一個單元測試的換行）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個 binary，4998 個通過，1 個 ignored，沒有失敗；之後加了 samply 的那一項，重跑 `cargo test -p shell`、`speedometer_test.py` 和 `shell` 的 clippy）。
