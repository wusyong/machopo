# RFC 0116 的進度：automation pipe 改用 fd 3、4 和 Chrome 的開關

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-26。完成日期：2026-11-26（macOS 26.6.2、Windows 11；Linux 是 CI 的 `cargo test`）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. `ipc` 的兩個入口：`inherited_pipe`、`spawn_with_pipe`（Unix、Windows）和它們的測試（已決定的事項第 4 到 8 條） | 完成 | 2026-11-26 |
| 2a. `shell` 的開關、拿 pipe、reader 和 writer、renderer 的 stdout；machopodriver、`shell` 的測試和情境；`ipc` 照第 10 條不看 Unix 上的 handle 開關 | 完成 | 2026-11-26 |
| 2b. Python 的工具（`start_with_pipe`、`MachopoPipe`、`ChromePipe`、`raster.py`）和 bench 工具的四份測試 | 完成 | 2026-11-26 |
| 2c. 文件和限制 | 完成 | 2026-11-26 |
| 3. Windows：第 1、2 段的測試、mutation test 和 bench | 完成 | 2026-11-26 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| `ipc` | child 讀寫得到它的兩端；host 關掉輸入時 child 結束，host 兩秒內讀到結尾 | `cargo test -p ipc --test it inherited_pipe::`：`a_child_reads_and_writes_its_ends` |
| `ipc` | 收下的兩端不傳給 child 的 child（close-on-exec、不可繼承） | `a_childs_own_child_has_no_end_of_the_pipe`：孫 process 活 4 秒，host 兩秒內讀到結尾 |
| `ipc` | 同時啟動的 child 互相拿不到對方的兩端；Windows 上通道的 child 和 pipe 的 child 同時啟動時也一樣（共用一把鎖） | `children_started_at_once_each_have_their_own_ends_alone`（八條 thread）；Windows 的 `a_channels_child_started_meanwhile_has_no_end_of_a_pipe`（各 4 條 thread、每條啟動 6 個） |
| `ipc` | 一個 process 只問一次 | `the_ends_are_taken_once` |
| `ipc` | shell 交的 pipe、Node 那樣的 socketpair 都收；3、4 是空的 process 啟動得了有 pipe 的 child | `ends_a_shell_puts_at_3_and_4_are_taken`、`a_socket_pair_at_3_and_4_is_taken`、`a_process_with_3_and_4_free_starts_a_child_with_a_pipe` |
| `ipc` | 沒開著、不是 pipe、Windows 上沒有或不對的開關、輸出那一端不是 pipe、兩端是同一個 handle，各自回對的原因；macOS、Linux 上給了 handle 的開關照樣讀寫 3、4 | `a_child_started_without_a_pipe_says_what_is_missing`、`handles_named_on_macos_and_linux_are_let_be`；Windows 的 `a_child_named_no_pipe_says_why`、`a_childs_output_is_looked_at_as_its_input_is` |
| `shell` | `--remote-debugging-pipe`、`--headless`、`--remote-debugging-io-pipes` 的解析；`--automation-pipe`、`--window` 說明換成什麼；`--remote-debugging-pipe=cbor` 報錯 | `cargo test -p shell --bin machopo` |
| `shell` | 沒有 pipe 時說出原因（附上 `3<&0 4>&1`）、以 2 結束 | `cargo test -p shell --test it automation_pipe::without_its_pipe_machopo_says_so` |
| `shell` | stdin 是開著、沒人寫的 pipe（Puppeteer 的樣子）時照常運作，pipe 的輸入結束就結束 | `automation_pipe::standard_input_left_open_is_no_part_of_the_pipe` |
| `shell` | renderer 的 stdout 是 Browser 的 stdout；renderer 沒有 pipe 的兩端（Linux） | `renderer_process::a_renderers_standard_output_is_the_browsers`（Linux 比對 `/proc/<pid>/fd/1`，macOS 比對 `lsof` 說的 pipe 的另一端）；`automation_pipe::a_renderer_process_hasnt_the_pipe` |
| `shell` | 現有的 pipe 的測試全部經過新的開關和接法 | `cargo test -p shell`（`automation_pipe`、`devtools_*`、`webdriver`、`remote_debugging`、`renderer_process`） |
| 真的視窗 | 沒有 `--headless` 時是平台的視窗，pipe 照樣驅動它 | `MACHOPO_WINDOW_TESTS=1` 的 `automation_pipe::the_real_window_is_driven_the_same`；`python3 scripts/test.py scenarios --window` |
| machopodriver | 經過 `spawn_with_pipe` 啟動 machopo，WPT 照舊 | `cargo test -p driver`；`python3 scripts/test.py wpt dom/lists/ dom/abort/` |
| 量測工具 | Python 的工具照 Chrome 的方式接 machopo 的 pipe（和 `ChromePipe` 共用 `start_with_pipe`），經過取樣器也一樣 | `python3 scripts/bench/speedometer/speedometer_test.py PipeTests`（讀 3、寫 4 的假 browser）、`blink_perf_test.py` 的 `taskset` 的測試；Windows 上 `RunTests`、`GcRunTests` 經過 handle 驅動真的 Chrome 和 machopo |
| 量測工具 | `--against` 的舊版本（還是 `--automation-pipe`）照舊經過 stdin、stdout | `PipeTests` 的 `test_a_version_is_known_by_its_help`、`test_an_against_version_from_before_its_pipe_was_chromes`、`test_each_version_is_started_its_way` |
| 全部 | 守著這些的測試擋得住弄壞的實作 | `python3 scripts/test.py mutate --keep-going rfcs/0116-automation-pipe-fds/mutants-<名稱>.toml`：`1`、`2a`、`2b`（macOS、Linux），`3`（Windows） |

## 做過的檢查

macOS 26.6.2（Apple M4，第 1、2 段）、Windows 11 10.0.26300（x86-64，第 3 段），2026-11-26；Linux 只有 CI（Ubuntu 24.04）的 `cargo test`。branch `0116` 從 `main` 的 `d9232722` 開出，第 1 段之後 rebase 到 `1b2ae84f`（`refused_port` 的修正）。

### 設計和做法上的改變

- **`ipc`**：`TAKEN` 原本先 `load` 再 `swap` 兩道，連續呼叫時其中一道一定是多餘的（mutant 抓不到）；改成一開始 `swap` 一次，不管結果。Windows 上「一次啟動一個 child」的鎖從通道的 `spawn` 搬出來（`starting_a_child`），pipe 也用它。
- **renderer 的 stdout**：RFC 原本以為 macOS 上要 `unsafe` 才比得了，記進 test-gaps；實際上 `lsof` 對 pipe 印出另一端的位址，兩個 process 拿著同一條 pipe 的同一端時一樣，所以測得到。
- **`--against`**：原本沒想到工具會建出舊版本的 machopo 來比較，那些版本還是 `--automation-pipe`、經過 stdin、stdout。照 `WITHOUT_SINGLE_PROCESS` 的前例在 `against_binaries` 標記（`WITH_STDIO_PIPE`），不照 commit 判斷（這份 RFC 的 commit 在 merge 前 rebase 時 hash 會變），問執行檔的 `--help`（`has_stdio_pipe`）。
- **`ChromePipe`**：原本在 Windows 上把 `--remote-debugging-io-pipes` 插在命令的第三個位置，有取樣器包著時位置不對；共用 `start_with_pipe` 之後放在最後。
- **Windows 的測試**：把開關放在測試框架參數的 `--` 之後、讓框架當成多一個測試名稱，在 Windows 上行得通。

### 找到的 bug（都修好了）

- **Linux 的 CI**：`automation_pipe::a_renderer_process_hasnt_the_pipe` 只在 Linux 編譯，還預期 renderer 的 stdout 是 Browser 的 stderr，2a 改了這個行為時在 macOS 上看不到它；拿掉 stdout 的那一個 assert（stdout 由 `a_renderers_standard_output_is_the_browsers` 守著）。
- **Windows 的 clippy**：`renderer_process.rs` 的 `start_machopo` 的 `use` 沒有 `cfg`，用到它的測試只在 Unix 編譯；加上 `#[cfg(unix)]`。
- **在 `main` 上修的**：`net::test_server::refused_port()` 占著一個 bind、沒有 listen 的 port，macOS 上連到它約 7.8 秒後是 `ETIMEDOUT`、不是被拒絕（BSD 的 TCP 不回 reset），9 個測試在 macOS 上一直失敗（`net`、`browser`、`shell` 的 `devtools_*`、`webdriver`）。macOS 上拿到就放掉（它照順序分配 port），`main` 的 `1b2ae84f`，和這份 RFC 無關。

### Mutation test

- **`mutants-1.toml`**（macOS）：9 個都抓到：收下的兩端沒設 close-on-exec、可以問兩次、什麼都當成 pipe、socket 不算、沒開著當成不是 pipe、不看輸出那一端、child 的兩端沒先搬到 4 以上（只有 3、4 是空的 process 的那個測試抓得到）、輸出沒放到 4、host 留著 child 的兩端。最後一個一開始是測試卡住、跑到時限才算抓到，讀到結尾改成兩秒的時限（`ends_soon`）之後當場失敗。原本的第 10 個（Unix 上也收 handle 的開關）在第 10 條改掉行為時拿掉。
- **`mutants-2a.toml`**（macOS）：7 個都抓到：renderer 的 stdout 又轉到 stderr、沒有 pipe 時不附 shell 的寫法、`--headless` 當成沒給、沒有 pipe 也收 `--headless`、headless 的視窗也拿 `--scale` 當真的視窗的、`--automation-pipe` 變成一般的不認得的開關、真的視窗變成 headless（`MACHOPO_WINDOW_TESTS` 的測試抓到）。
- **`mutants-2b.toml`**（macOS）：5 個都抓到：新的 machopo 當成舊的、`--against` 的舊版本沒標記、每個版本都當成新的、pipe 的兩端放反、工具留著 browser 的那兩端（讀到結尾改到另一條 thread、等 10 秒，之前跑到 120 秒的時限）。
- **`mutants-3.toml`**（Windows）：12 個都抓到：收下的兩端沒設成不可繼承、可以問兩次、不看 `GetFileType`、沒給開關當成格式不對、不是數字當成不是 pipe、不檢查輸出那一端、兩端是同一個 handle 也收、child 的兩端沒設成可繼承、開關裡兩端放反、host 留著 child 的兩端、不拿鎖就設成可繼承（另外單獨跑 6 次都抓到）、pipe 用自己的鎖。為它們補了 Windows 的兩個測試；「pipe 用自己的鎖」靠撞上時序，第一版 20 次只抓到 9 次，加多同時啟動的 child 之後 12 次都抓到（漏抓時是測試通過，不會誤報）。
- **沒寫的**：Python 的 `start_with_pipe` 的 Windows 分支（`msvcrt`、`handle_list`）：`mutants-2b.toml` 的測試在 Windows 上跳過，由真的 browser 的測試走到；`handle_list` 拿掉時只是多繼承了 handle，寫了也抓不到。記在 `docs/test-gaps.md` 的「只靠審查」（2026-11-26，使用者決定）。

### 不穩定的測試（都不是這份 RFC 造成的，沒有改）

- **`automation_pipe::hung_renderers_take_the_limit_once`**（Windows）：`shell` 的整套一起跑時偶爾失敗（9 輪 2 次，之後帶著診斷 27 輪沒有重現），失敗的是「machopo 結束之後 renderer 都不在了」那一個 assert，`tasklist` 還列得到一個 85 MB 的 `machopo.exe`。直接量：kill 並等過的 process 400 次都沒有再被列到；釋放的 pid 在連續啟動 process 時 6 次裡有 4 次在 209 到 1670 個之後回來。推測是 pid 在 machopo 結束到測試問 `tasklist` 之間被別的測試的 machopo 重用（沒有當場證實）；修法是不要事後用 pid 問系統（`renderers_go_with_a_killed_browser` 也用同一個 `alive`）。
- **`event_loop::an_idle_callbacks_timeout_comes_while_the_page_is_busy`**（Windows）：idle callback 的 `didTimeout` 是 `false`：頁面還在忙時 idle callback 就因為閒下來而跑了。單獨跑 10 次失敗 1 次，不是整套平行跑的負載造成的；這份 RFC 在 renderer 只改了註解，原因沒有查。
- **`demo-busy`**（macOS 的真視窗）：第 19 行，主執行緒忙的時候滾輪沒讓第二個盒子那一區的畫面變；單獨跑 3 次都失敗，在第 1 段的程式上（沒有 2a 的改動）也一樣。log 開頭有一次 Viz 的 `occluded`，原因沒有追；RFC 0107 那時在 macOS 的真視窗裡它通過。

### 真的量測

- **Speedometer**（Windows，`bench.py speedometer` 一次；machopo 的 `perf` profile 對 Chrome for Testing 152.0.7977.82，經過兩邊的 handle）：20 個 suite 兩邊都量到，machopo / Chrome 在 1.37 到 2.11 之間，幾何平均約 1.64。bench 的 HTTP server 印了 11 次 `ConnectionAbortedError: [WinError 10053]`（`do_GET` 寫回應時對面關了連線），量測沒有因此失敗；單獨跑 Chrome 時也有，沒有查。
- **GPU**（macOS）：用真的 machopo 跑 `machopo_gpu` 回報 `Apple M4 (IntegratedGpu, Metal)`；工具還用舊開關時（2a 和 2b 之間）它是 `unknown`，`test.py wpt` 也跟著印出 `unknown`。

### 跑過的檢查

- **macOS**：`cargo test --workspace` 4934 個通過、0 個失敗、1 個 ignored；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `unsafe`、`tests`、`numerics`、`deps`、`spans`、`boundary`、`docs` 通過；`test.py wpt dom/lists/ dom/abort/` 18 個測試、261 個子測試，沒有意外的結果；`test.py scenarios --window` 74 個裡 73 個通過（`demo-busy`，上面），`the_real_window_is_driven_the_same` 通過，看了截圖；bench 工具的四份測試（`speedometer_test.py` 126 個、`blink_perf_test.py` 37 個、`raster_test.py` 13 個、`census_test.py` 5 個）通過。
- **Windows**：`cargo test -p ipc`（unit 102 個、`tests/it` 100 個，連跑 20 次）、`cargo test -p shell -p driver`、`test.py wpt dom/lists/ dom/abort/`、bench 工具的四份測試（`blink_perf_test.py` 46 個沒有跳過，`speedometer_test.py` 142 個、跳過 4 個，`raster_test.py` 13 個、跳過 1 個，`census_test.py` 5 個）、`cargo test --workspace --no-fail-fast`（4924 個通過、1 個失敗：上面的 idle callback）、fmt、clippy 和 `sanity.py` 的每一項。
- **Linux**：CI（Ubuntu 24.04）的 `cargo test` 在 `fb5fe632` 通過。
