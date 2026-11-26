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
| 3. Windows：在使用者的裝置上跑第 1、2 段的測試和 bench | 沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| child 讀寫得到 fd 3、4；host 關掉輸入時 child 結束，host 兩秒內讀到結尾 | `crates/ipc/tests/it/inherited_pipe.rs` 的 `a_child_reads_and_writes_its_ends` |
| 收下的兩端不傳給 child 的 child | `a_childs_own_child_has_no_end_of_the_pipe`：孫 process 活 4 秒，host 兩秒內讀到結尾 |
| 同時啟動的 child 互相拿不到對方的兩端 | `children_started_at_once_each_have_their_own_ends_alone`（八條 thread） |
| 一個 process 只問一次 | `the_ends_are_taken_once` |
| shell 交的 pipe、Node 那樣的 socketpair 都收 | `ends_a_shell_puts_at_3_and_4_are_taken`、`a_socket_pair_at_3_and_4_is_taken` |
| 沒開著、不是 pipe，各自回對的原因；Unix 上給了 handle 的開關照樣讀寫 3、4（第 10 條，2a 改的） | `a_child_started_without_a_pipe_says_what_is_missing`、`handles_named_on_macos_and_linux_are_let_be`；Windows 的是 `a_child_named_no_pipe_says_why`（第 3 段跑） |
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
