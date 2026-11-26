# RFC 0116 的進度：automation pipe 改用 fd 3、4 和 Chrome 的開關

開始日期：2026-11-26

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. `ipc` 的兩個入口：`inherited_pipe`、`spawn_with_pipe`（Unix、Windows）和它們的測試（已決定的事項第 4 到 8 條） | 做完（Windows 的那一份留到第 3 段才編譯和測試） | 2026-11-26 |
| 2. 換過去（已決定的事項第 1、9 到 11 條） | 做完（macOS） | 2026-11-26 |
| 2a. `shell` 的開關、拿 pipe、reader 和 writer、renderer 的 stdout；machopodriver、`shell` 的測試和情境；`ipc` 照第 10 條忽略 Unix 上的 handle 開關 | 做完 | 2026-11-26 |
| 2b. Python 的工具（`MachopoPipe`、`ChromePipe`、`raster.py`）和 bench 工具的四份測試 | 做完 | 2026-11-26 |
| 2c. 文件和限制 | 做完 | 2026-11-26 |
| 3. Windows：在使用者的裝置上跑第 1、2 段的測試和 bench | 測試、mutation test 和 bench 跑完；`hung_renderers_take_the_limit_once` 不穩定的原因是推測的（pid 重用）、沒有當場證實，測試沒有改；Python 的 `start_with_pipe` 在 Windows 上的 mutant 還沒決定要不要寫（下面的「第 3 段」） | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| child 讀寫得到 fd 3、4；host 關掉輸入時 child 結束，host 兩秒內讀到結尾 | `crates/ipc/tests/it/inherited_pipe.rs` 的 `a_child_reads_and_writes_its_ends` |
| 收下的兩端不傳給 child 的 child | `a_childs_own_child_has_no_end_of_the_pipe`：孫 process 活 4 秒，host 兩秒內讀到結尾 |
| 同時啟動的 child 互相拿不到對方的兩端 | `children_started_at_once_each_have_their_own_ends_alone`（八條 thread） |
| 一個 process 只問一次 | `the_ends_are_taken_once` |
| shell 交的 pipe、Node 那樣的 socketpair 都收 | `ends_a_shell_puts_at_3_and_4_are_taken`、`a_socket_pair_at_3_and_4_is_taken` |
| 沒開著、不是 pipe，各自回對的原因；Unix 上給了 handle 的開關照樣讀寫 3、4（第 10 條，2a 改的） | `a_child_started_without_a_pipe_says_what_is_missing`、`handles_named_on_macos_and_linux_are_let_be`；Windows 的是 `a_child_named_no_pipe_says_why` 和 `a_childs_output_is_looked_at_as_its_input_is`（輸出那一端不是 pipe、兩端是同一個 handle） |
| Windows：通道的 child 和 pipe 的 child 同時啟動時，通道的 child 拿不到 pipe 的兩端（兩邊共用啟動 child 的那把鎖） | `a_channels_child_started_meanwhile_has_no_end_of_a_pipe`（各 4 條 thread、每條啟動 6 個） |
| 3、4 是空的 process（新的 pipe 建在 3、4）啟動得了有 pipe 的 child | `a_process_with_3_and_4_free_starts_a_child_with_a_pipe` |
| `--remote-debugging-pipe`、`--headless`、`--remote-debugging-io-pipes` 的解析；`--automation-pipe`、`--window` 說明換成什麼；`--remote-debugging-pipe=cbor` 報錯 | `shell` 的 `main.rs` 的單元測試 |
| 沒有 pipe 時說出原因（附上 `3<&0 4>&1`）、結束碼 2 | `crates/shell/tests/it/automation_pipe.rs` 的 `without_its_pipe_machopo_says_so` |
| stdin 是開著、沒人寫的 pipe（Puppeteer 的樣子）時照常運作，pipe 的輸入結束就結束 | `standard_input_left_open_is_no_part_of_the_pipe` |
| renderer 的 stdout 是 Browser 的 stdout | `crates/shell/tests/it/renderer_process.rs` 的 `a_renderers_standard_output_is_the_browsers`（Linux 比對 `/proc/<pid>/fd/1`，macOS 比對 `lsof` 說的 pipe 的另一端） |
| 經過 machopodriver 的 WPT | `test.py wpt dom/lists/ dom/abort/` |
| 現有的 pipe 的測試全部經過新的開關和接法 | `cargo test -p shell`（連同 `devtools_*`、`webdriver`、`remote_debugging`、`renderer_process`） |
| Python 的工具照 Chrome 的方式接 machopo 的 pipe（和 `ChromePipe` 共用 `start_with_pipe`），經過取樣器也一樣 | `scripts/bench/speedometer/speedometer_test.py` 的 `PipeTests`（一個讀 3、寫 4 的假 browser）、`blink_perf_test.py` 的 `taskset` 的測試；用真的 machopo 跑 `machopo_gpu`、`Browser.getVersion` 和 `Browser.close` |
| `--against` 的舊版本（還是 `--automation-pipe`）照舊經過 stdin、stdout | `PipeTests` 的 `test_a_version_is_known_by_its_help`、`test_an_against_version_from_before_its_pipe_was_chromes`、`test_each_version_is_started_its_way` |

## 做過的檢查

### 第 1 段（2026-11-26，macOS 26.6 Apple Silicon）

- **新的測試**：`crates/ipc/tests/it/inherited_pipe.rs` 的 10 個（上面的表）。測試把自己再執行一次當成 child；參數以 `--` 結尾，Windows 上 `spawn_with_pipe` 加在最後的開關才是多的一個測試名稱，而不是測試框架不收的選項（還沒在 Windows 上跑過）。shell 的那幾個把 pipe 當成 `sh` 的 stdin、stdout 交過去、再由它搬到 3、4，測試裡不用 `unsafe`。
- **mutation test**（`mutants-1.toml`，macOS 和 Linux 的那一份）：10 個都抓到：收下的兩端沒設 close-on-exec、可以問兩次、什麼都當成 pipe、socket 不算、沒開著當成不是 pipe、不看輸出那一端、Unix 上也收 handle 的開關、child 的兩端沒先搬到 4 以上（只有 3、4 是空的 process 的那個測試抓得到）、輸出沒放到 4、host 留著 child 的兩端。最後一個一開始是測試卡住、跑到時限（300 秒）才算抓到：讀到結尾的地方改成兩秒的時限（`ends_soon`）之後當場失敗。
- **改掉的寫法**：`TAKEN` 原本先 `load` 再 `swap` 兩道，連續呼叫時其中一道一定是多餘的（mutant 抓不到）；改成一開始 `swap` 一次，不管結果，之後問都是 `Taken`。
- **Windows**：`starting_a_child` 的鎖從通道的 `spawn` 搬出來，兩邊共用；`inherited_by_children` 改收 `BorrowedHandle`。Windows 的程式在這台機器上沒有編譯過（沒有裝 Windows 的 target），第 3 段在使用者的裝置上編譯、跑測試和 mutation test。
- **檢查**：`cargo test -p ipc` 通過（94 + 110）；`sanity.py` 的 `docs`、`unsafe`、`tests`、`numerics`、`deps`、`spans`、`boundary` 通過；階段結束的 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過。
- **`cargo test --workspace --no-fail-fast`**：4922 個通過、9 個失敗、1 個 ignored。9 個失敗是同一個原因，和這一段無關（把這一段的改動 stash 起來、在 `main` 的 `d9232722` 上一樣失敗）：它們都用 `net::test_server::refused_port()` 拿一個「會被拒絕」的 port（`net` 的 `client::a_load_without_a_response_tells_only_of_its_end`、`load::connection_failures`，`browser` 的 `navigation::failed_loads_commit_an_error_page`，`shell` 的 `devtools_lifecycle`、`devtools_log` 兩個、`devtools_network` 兩個、`webdriver` 各一個），預期 `ERR_CONNECTION_REFUSED`，拿到 `ERR_TIMED_OUT`。`refused_port()` 留著一個 bind 好、沒有 listen 的 socket 占住 port；在這台 macOS 26.6.2 上，連這樣的 port 時 kernel 不回 RST，約 7.8 秒後是 `ETIMEDOUT`（Python 試過：關掉 socket 之後再連就立刻被拒絕）。每次都重現，不是不穩定。修 `refused_port()` 是另一件事，不在這份 RFC 裡（2026-11-26 在 `main` 上修了：macOS 上拿到 port 就放掉）。

### 2a（2026-11-26，macOS 26.6.2 Apple Silicon）

- **改了什麼**：`main.rs` 的開關照第 1 條拆成 `--remote-debugging-pipe` 和 `--headless`，收 `--remote-debugging-io-pipes`（第 10 條）；`--automation-pipe`、`--window` 說明換成什麼。解析完開關之後、開 log 和 trace 之前拿 pipe（第 11 條）。`automation_pipe.rs` 改名 `devtools_pipe_handler.rs`，reader 和 writer 收兩端（`Pipe`），`window.rs` 等到視窗起來再拿。renderer 的 stdout 不再轉到 stderr。machopodriver、`shell` 的測試（`start_machopo`）和真的視窗的情境用 `ipc::platform::spawn_with_pipe`；`driver` 多依賴 `ipc`。程式碼裡提到舊開關的註解一起改了。
- **`ipc` 照第 10 條改**：`InheritError::HandlesOnlyOnWindows` 拿掉，Unix 上給了 handle 的開關照樣讀寫 3、4（`handles_named_on_macos_and_linux_are_let_be`）；`mutants-1.toml` 拿掉對應的 mutant，重跑 9 個都抓到。
- **原本打算的和實際的**：RFC 原本寫 renderer 的 stdout 在 macOS 上比不了（要 `unsafe`），實際上用 `lsof` 比得了：它對 pipe 印出另一端的位址，兩個 process 拿著同一條 pipe 的同一端時一樣。所以沒有記進 `docs/test-gaps.md`。
- **mutation test**（`mutants-2a.toml`）：7 個都抓到：renderer 的 stdout 又轉到 stderr、沒有 pipe 時不附 shell 的寫法、`--headless` 當成沒給、沒有 pipe 也收 `--headless`、headless 的視窗也拿 `--scale` 當真的視窗的、`--automation-pipe` 變成一般的不認得的開關，和「真的視窗變成 headless」（會開出真的視窗，`MACHOPO_WINDOW_TESTS` 的 `the_real_window_is_driven_the_same` 抓到）。
- **檢查**：`cargo test -p ipc -p shell -p driver` 1072 個通過；`test.py wpt dom/lists/ dom/abort/` 18 個測試、261 個子測試，沒有意外的結果；`clippy`（`ipc`、`shell`、`driver`）、`cargo fmt --check` 和 `sanity.py` 的 `docs`、`unsafe`、`tests`、`numerics`、`deps`、`spans`、`boundary` 通過。
- **真的視窗**（`window.rs` 和情境的 `--window` 那一路改了）：`test.py scenarios --window` 74 個裡 73 個通過；`demo-busy` 失敗（第 19 行：主執行緒忙的時候，滾輪沒讓第二個盒子那一區的畫面變），單獨跑 3 次都一樣，把 2a 的改動 stash 起來、在第 1 段的程式上跑也一樣，所以和這一段無關（log 在開頭有一次 Viz 的 `occluded`，原因沒有追；RFC 0107 那時在 macOS 的真視窗裡它通過，之間的改動和這台機器的環境都可能是原因）。因為它失敗，`scenarios --window` 停在 pipe 驅動真的視窗的那個測試之前：另外用 `MACHOPO_WINDOW_TESTS=1` 跑了 `automation_pipe::the_real_window_is_driven_the_same`，通過。看了截圖（`grid`、`wasm-listeners`），視窗的 UI 和頁面都畫出來了。
- **2b 的事**：`test.py wpt` 印出的 `machopo draws with: unknown`：`speedometer.machopo_gpu` 經過 Python 的 `MachopoPipe` 啟動 machopo，還在用舊的開關，問不到 GPU（有 GPU 的機器不受影響；CPU adapter 的機器上 wptrunner 的 `--processes` 算不出來）。

### 2b（2026-11-26，macOS 26.6.2 Apple Silicon）

- **改了什麼**：`scripts/bench/common.py` 的 `start_with_pipe`：照 Chrome 的方式啟動一個 browser 和它的 pipe（Linux、macOS 經過 `sh` 把兩端搬到 3、4，Windows 給 `--remote-debugging-io-pipes` 和只繼承那兩個 handle），`ChromePipe` 和 `MachopoPipe` 共用。`ChromePipe` 原本在 Windows 上把開關插在命令的第三個位置（有取樣器包著時位置不對），現在放在最後。`MachopoPipe` 給 `--remote-debugging-pipe --headless`，stdout 和 stderr 都寫 log。`raster.py` 用 `start_with_pipe`，`--window` 時不給 `--headless`。`scripts/test.py` 和 `speedometer.py` 的說明跟著改。
- **原本沒想到的**：`--against` 會建出舊版本的 machopo 來比較，那些版本還是 `--automation-pipe`、經過 stdin、stdout。照 `WITHOUT_SINGLE_PROCESS` 的前例在 `against_binaries` 標記（`WITH_STDIO_PIPE`），不過不照 commit 判斷（這份 RFC 的 commit 在 merge 前 rebase 時 hash 會變），改成問執行檔：舊版本的 `--help` 有 `--automation-pipe`，新版本沒有（`has_stdio_pipe`）。
- **測試**：`speedometer_test.py` 多了 `PipeTests` 的 4 個；`blink_perf_test.py` 的兩個照新的命令改：`taskset` 的那個看 `sh` 之後是取樣器和 machopo；Windows 的 affinity 的那個在假的 Windows 上改用舊版本那條路（新的那條要真的 `msvcrt`），新的那條和 `ChromePipe` 一起只在真的 Windows 上跑。
- **mutation test**（`mutants-2b.toml`）：5 個都抓到：新的 machopo 當成舊的、`--against` 的舊版本沒標記、每個版本都當成新的、pipe 的兩端放反、工具留著 browser 的那兩端。最後一個一開始是測試卡住、跑到時限（120 秒）才算抓到：讀到結尾改到另一條 thread、等 10 秒之後當場失敗。
- **檢查**：bench 工具的四份測試（`speedometer_test.py` 126 個、`blink_perf_test.py` 37 個、`raster_test.py` 13 個、`census_test.py` 5 個）通過；用真的 machopo 跑 `machopo_gpu` 回報 `Apple M4 (IntegratedGpu, Metal)`（2a 時是 `unknown`），`test.py wpt dom/lists/` 沒有意外的結果、印出的也是那個 GPU。Windows 上的 `start_with_pipe`（`msvcrt`、`handle_list`）在第 3 段跑。

### 2c（2026-11-26）

- **改了什麼**：`docs/architecture.md`（`AutomationPipeReader`、`AutomationPipeWriter` 讀寫的地方、machopodriver 怎麼啟動 machopo、renderer 的 stdio、`shell` 和 `driver` 的模組表、和 Chromium 刻意不同的地方的「自動化的通道」：現在和 Chrome 一樣，只剩 Windows 不收 C runtime 的 fd）、`docs/testing.md`、`docs/test-gaps.md`、`docs/benchmarks.md`（工具兩邊都經過 `--remote-debugging-pipe`，`--against` 的舊版本照舊）、`crates/shell/README.md`（「自動化的 pipe」改寫）、`crates/driver/README.md`、`tests/scenarios/README.md` 和 wptrunner 的一行註解。
- **`docs/limitations.md`**：「pipe 是 stdin 和 stdout」那一條改寫成 Playwright、Puppeteer 還差什麼（Chrome 的其他開關、pipe 時的網址、`waitForDebuggerOnStart`、isolated world、`Emulation`、`Fetch`、browser context；Windows 上的 C runtime 的 fd）；「網頁的輸出和 log 混在 stderr」那一條的理由改成照 Chrome；真的視窗不理會輸入那一條和 `about:blank` 那一條換成新的開關，前者補上和 Chrome 的不同（Chrome 經過 pipe 驅動時照樣收使用者的輸入）。
- **檢查**：`sanity.py docs` 通過；文件裡剩下的 `automation_pipe` 是測試模組（`crates/shell/tests/it/automation_pipe.rs`，沒有改名），`--automation-pipe` 只剩 `docs/benchmarks.md` 說明 `--against` 的舊版本那一處。

### 第 2 段結束的檢查（2026-11-26，macOS 26.6.2 Apple Silicon）

- `cargo test --workspace`：4934 個通過、0 個失敗、1 個 ignored（第 1 段時失敗的 9 個 `refused_port` 的測試，在 `main` 修好、rebase 進來之後通過）。
- `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `unsafe`、`tests`、`numerics`、`deps`、`spans`、`boundary`、`docs` 通過。
- Windows 的程式（`ipc` 的 handle、`start_with_pipe` 的 `msvcrt`）在第 3 段在使用者的裝置上編譯和測試。

### 第 3 段（2026-11-26，Windows 11 10.0.26300，x86-64，`e5bb7ec9`）

- **編譯和測試**：Windows 的那一份第一次編譯，沒有錯誤。`cargo test -p ipc`：unit 102 個、`tests/it` 100 個通過（1 個手動的量測 ignored）。`cargo test -p shell -p driver`：`driver` 14 個、`shell` 的 lib 95 個、`machopo` 的 bin 12 個通過，`shell` 的 `tests/it` 741 個通過、1 個失敗（下面的「不穩定的測試」）。測試把開關放在 `--` 之後、讓測試框架當成多一個測試名稱的做法，在 Windows 上行得通。
- **clippy 找到的**：`crates/shell/tests/it/renderer_process.rs` 的 `use crate::automation_pipe::start_machopo;` 沒有 `cfg`，用到它的兩個測試都只在 Unix 編譯，Windows 上是 unused import；加上 `#[cfg(unix)]`。之後 `cargo clippy -p ipc -p shell -p driver --all-targets -- -D warnings` 通過。
- **經過 machopodriver 的 WPT**：`test.py wpt dom/lists/ dom/abort/` 18 個測試、261 個子測試，沒有意外的結果（2 個測試跳過）。
- **補的測試**（`crates/ipc/tests/it/inherited_pipe.rs`，Windows）：
  - `a_childs_output_is_looked_at_as_its_input_is`：原本 Windows 上不是 pipe 的值只有 `1,2` 一組，輸入那一端先失敗，輸出那一端的檢查和「兩端是同一個 handle」沒有測試走到。child 多兩個角色（`output-no-pipe`、`one-handle`）：拿到真的開關值之後，把輸出那一端換成 `1` 或輸入的值再問。
  - `a_channels_child_started_meanwhile_has_no_end_of_a_pipe`：原本沒有測試同時啟動通道的 child 和 pipe 的 child，`spawn_with_pipe` 改用自己的鎖時沒有測試失敗。child 多一個 `stay` 角色（待到被 kill）。第一版每種 child 各 8 條 thread、每條啟動一個，「pipe 用自己的鎖」的 mutant 20 次只抓到 9 次；改成各 4 條 thread、每條連續啟動 6 個之後，12 次都抓到。它靠撞上時序，漏抓時是測試通過，不會誤報失敗。
- **mutation test**（`mutants-3.toml`，Windows 的那一份）：12 個都抓到：收下的兩端沒設成不可繼承、可以問兩次、什麼都當成 pipe（不看 `GetFileType`）、沒給開關當成格式不對、不是數字當成不是 pipe、不檢查輸出那一端、兩端是同一個 handle 也收、child 的兩端沒設成可繼承、開關裡兩端放反、host 留著 child 的兩端、不拿鎖就設成可繼承（另外單獨跑 6 次都抓到）、pipe 用自己的鎖。補了測試之後 `cargo test -p ipc` 連跑 20 次都通過，`ipc` 的 clippy 和 `cargo fmt --check` 通過。
- **不穩定的測試**（原因還沒找到）：`automation_pipe::hung_renderers_take_the_limit_once` 在 `cargo test -p shell -p driver` 整套一起跑時失敗一次（只跑了一次），單獨跑 8 次都通過。失敗的是「machopo 結束之後 renderer 都不在了」那一個 assert（`tasklist` 還列得到三個 pid 之中的至少一個），前面的時限有過。這個測試和它的 `alive` 不是這份 RFC 改的，沒有在 `main` 上跑整套對照。
  - **查到的**：整套 `cargo test -p shell --test it` 再跑 8 輪重現 1 次，那次 `tasklist` 列到的是 `machopo.exe`、85,624 K。之後帶著更多診斷（parent pid、建立時間、CPU 時間）跑了 27 輪都沒有重現，所以沒有當場抓到那個 process 是誰。machopo 這一側 `end_child` 是 `kill()` 之後 `wait()`，回來時 process 已經結束。
  - **直接量的**（閒置的機器，PowerShell）：kill 並等過的 process（握著它的 handle，pid 不會被重用）400 次都沒有再出現在 process 清單裡；釋放的 pid 在接下來 40 個新 process 裡 200 次都沒有被重用，但連續啟動 process 時 6 次裡有 4 次在 209、430、623、1670 個之後回來（3.5 到 29 秒；另外 2 次 4000 個之內沒有）。pid 和 thread 的 id 用同一張表，整套測試平行跑時每秒啟動的 process 和 thread 比這個實驗多得多。
  - **推測的原因**（沒有當場證實）：pid 在 machopo 結束到測試問 `tasklist` 之間被別的測試的 `machopo.exe` 重用，`alive` 把它當成原本的 renderer。85 MB 的工作集、只在整套平行跑時出現、kill 過的 process 不再被列到，都和它吻合。
  - **沒有改**：測試照原樣。修法是不要事後用 pid 問系統（`renderers_go_with_a_killed_browser` 也用 `alive`），這是 machopo 怎麼結束的那些測試的事，不在這份 RFC 裡。
- **另一個不穩定的測試**（原因沒有查）：`event_loop::an_idle_callbacks_timeout_comes_while_the_page_is_busy`，預期 idle callback 的 `didTimeout` 是 `true`，拿到 `false`（頁面的 `fetch()` 串還沒做完時 idle callback 就因為閒下來而跑了，不是因為 50 ms 的 timeout 到了）。上面重跑的 35 輪裡失敗 1 次，`cargo test --workspace` 那一次也失敗；單獨跑 10 次失敗 1 次，所以不是整套平行跑的負載造成的。這份 RFC 在 renderer 和 Browser 只改了註解，沒有動到 event loop；沒有在 `main` 上或別的平台上對照。
- **bench 工具的測試**：第一次跑時這台機器沒有 Chrome for Testing，真的驅動 browser 的測試都跳過；Speedometer 下載之後重跑：`blink_perf_test.py` 46 個通過、沒有跳過（`RunTests`、`GcRunTests` 經過 `ChromePipe` 和 `MachopoPipe` 的 handle）；`speedometer_test.py` 142 個、跳過 4 個（`PipeTests` 用 shell script 當 browser 的 2 個、要 Linux perf 的 `ChromeSampleTests`、要 xperf 和 Administrator 的 `SampleTests`）；`raster_test.py` 13 個、跳過 1 個（macOS、Linux 的 trace clock）；`census_test.py` 5 個。
- **Speedometer**（`bench.py speedometer`，跑一次；machopo 的 `perf` profile 對 Chrome for Testing 152.0.7977.82，renderer 是自己的 process）：20 個 suite 兩邊都量到，machopo / Chrome 在 1.37（TodoMVC-JavaScript-ES5）到 2.11（TodoMVC-Svelte-Complex-DOM）之間，20 個比值的幾何平均約 1.64。
- **Speedometer 印出的 traceback**：bench 的 HTTP server 印了 11 次 `ConnectionAbortedError: [WinError 10053]`（`scripts/bench/common.py` 的 `do_GET` 寫回應時對面關了連線）。量測沒有因此失敗。各用一個 browser 跑 4 個 suite、3 次 iteration：只跑 machopo 時 0 次，只跑 Chrome 時 1 次；Chrome 在什麼時候關連線沒有查。`common.py` 沒有改。
- **階段結束的檢查**（`de5e47c9`）：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。`cargo test --workspace --no-fail-fast`：60 個 binary，4924 個通過、1 個失敗、2 個 ignored；失敗的是上面的 `an_idle_callbacks_timeout_comes_while_the_page_is_busy`，`hung_renderers_take_the_limit_once` 這一次通過。
- **還沒做的**：Python 的 `start_with_pipe` 的 Windows 分支（`msvcrt`、`handle_list`）沒有 mutant（`mutants-2b.toml` 的測試在 Windows 上跳過，它只由真的 browser 的測試走到）。
