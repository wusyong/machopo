# RFC 0116 的進度：automation pipe 改用 fd 3、4 和 Chrome 的開關

開始日期：2026-11-26

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. `ipc` 的兩個入口：`inherited_pipe`、`spawn_with_pipe`（Unix、Windows）和它們的測試（已決定的事項第 4 到 8 條） | 做完（Windows 的那一份留到第 3 段才編譯和測試） | 2026-11-26 |
| 2. 換過去：`shell` 的開關（第 1 條）、reader 和 writer、renderer 的 stdout、所有啟動 machopo 的程式、文件和限制 | 沒開始 | |
| 3. Windows：在使用者的裝置上跑第 1、2 段的測試和 bench | 沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| child 讀寫得到 fd 3、4；host 關掉輸入時 child 結束，host 兩秒內讀到結尾 | `crates/ipc/tests/it/inherited_pipe.rs` 的 `a_child_reads_and_writes_its_ends` |
| 收下的兩端不傳給 child 的 child | `a_childs_own_child_has_no_end_of_the_pipe`：孫 process 活 4 秒，host 兩秒內讀到結尾 |
| 同時啟動的 child 互相拿不到對方的兩端 | `children_started_at_once_each_have_their_own_ends_alone`（八條 thread） |
| 一個 process 只問一次 | `the_ends_are_taken_once` |
| shell 交的 pipe、Node 那樣的 socketpair 都收 | `ends_a_shell_puts_at_3_and_4_are_taken`、`a_socket_pair_at_3_and_4_is_taken` |
| 沒開著、不是 pipe、Unix 上給了 handle 的開關，各自回對的原因 | `a_child_started_without_a_pipe_says_what_is_missing`、`handles_are_windows_alone`；Windows 的是 `a_child_named_no_pipe_says_why`（第 3 段跑） |
| 3、4 是空的 process（新的 pipe 建在 3、4）啟動得了有 pipe 的 child | `a_process_with_3_and_4_free_starts_a_child_with_a_pipe` |

## 做過的檢查

### 第 1 段（2026-11-26，macOS 26.6 Apple Silicon）

- **新的測試**：`crates/ipc/tests/it/inherited_pipe.rs` 的 10 個（上面的表）。測試把自己再執行一次當成 child；參數以 `--` 結尾，Windows 上 `spawn_with_pipe` 加在最後的開關才是多的一個測試名稱，而不是測試框架不收的選項（還沒在 Windows 上跑過）。shell 的那幾個把 pipe 當成 `sh` 的 stdin、stdout 交過去、再由它搬到 3、4，測試裡不用 `unsafe`。
- **mutation test**（`mutants-1.toml`，macOS 和 Linux 的那一份）：10 個都抓到：收下的兩端沒設 close-on-exec、可以問兩次、什麼都當成 pipe、socket 不算、沒開著當成不是 pipe、不看輸出那一端、Unix 上也收 handle 的開關、child 的兩端沒先搬到 4 以上（只有 3、4 是空的 process 的那個測試抓得到）、輸出沒放到 4、host 留著 child 的兩端。最後一個一開始是測試卡住、跑到時限（300 秒）才算抓到：讀到結尾的地方改成兩秒的時限（`ends_soon`）之後當場失敗。
- **改掉的寫法**：`TAKEN` 原本先 `load` 再 `swap` 兩道，連續呼叫時其中一道一定是多餘的（mutant 抓不到）；改成一開始 `swap` 一次，不管結果，之後問都是 `Taken`。
- **Windows**：`starting_a_child` 的鎖從通道的 `spawn` 搬出來，兩邊共用；`inherited_by_children` 改收 `BorrowedHandle`。Windows 的程式在這台機器上沒有編譯過（沒有裝 Windows 的 target），第 3 段在使用者的裝置上編譯、跑測試和 mutation test。
- **檢查**：`cargo test -p ipc` 通過（94 + 110）；`sanity.py` 的 `docs`、`unsafe`、`tests`、`numerics`、`deps`、`spans`、`boundary` 通過；階段結束的 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過。
- **`cargo test --workspace --no-fail-fast`**：4922 個通過、9 個失敗、1 個 ignored。9 個失敗是同一個原因，和這一段無關（把這一段的改動 stash 起來、在 `main` 的 `d9232722` 上一樣失敗）：它們都用 `net::test_server::refused_port()` 拿一個「會被拒絕」的 port（`net` 的 `client::a_load_without_a_response_tells_only_of_its_end`、`load::connection_failures`，`browser` 的 `navigation::failed_loads_commit_an_error_page`，`shell` 的 `devtools_lifecycle`、`devtools_log` 兩個、`devtools_network` 兩個、`webdriver` 各一個），預期 `ERR_CONNECTION_REFUSED`，拿到 `ERR_TIMED_OUT`。`refused_port()` 留著一個 bind 好、沒有 listen 的 socket 占住 port；在這台 macOS 26.6.2 上，連這樣的 port 時 kernel 不回 RST，約 7.8 秒後是 `ETIMEDOUT`（Python 試過：關掉 socket 之後再連就立刻被拒絕）。每次都重現，不是不穩定。修 `refused_port()` 是另一件事，不在這份 RFC 裡。
