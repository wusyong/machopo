# RFC 0032 的進度：測試流程

開始日期：2026-10-10；完成：2026-10-10，Windows 11（32 個 thread，rust-lld）。整套的測試（`matrix`、`fuzz`、`wpt`、`wdspec`、`reftest`、`scenarios`）由使用者設定的每天的 CI 跑（README 的第 16 條），這份的驗收不包括它。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 第 1 段：`ci.py third-party` 的同步 | 完成 | 2026-10-10 |
| 第 2 段（原本）：nextest 的實測 | Windows 量完之後拿掉（README 的第 15 條） | 2026-10-10 |
| 第 3 段（原本）：mutation test 的範圍 | 開始時拿掉（第 15 條；掃描的結果在下面） | 2026-10-10 |
| 第 2 段：本機的測試規則配合每天的 CI | 完成 | 2026-10-10 |

## 驗收

**`ci.py third-party` 的同步**（手動確認，README 的第 6 條；`docs/test-gaps.md` 的「只做過手動確認」）

| 項目 | 怎麼驗證 |
| --- | --- |
| 缺的專案 clone 下來 | 這台沒有 speedometer：`ci.py third-party` clone 到 pin |
| 有沒 commit 的改動的 checkout 不動 | parley 改一個檔案、pin 和 HEAD 不同：留在原處，印出檔名和怎麼做，exit code 1；其他專案照樣同步 |
| 有 `origin` 沒有的 commit 的 checkout 不動 | parley 在暫時的 branch 上做一個空的 commit：留在原處，exit code 1；`origin_has` 對 push 過的 `4cc636c`、WPT 的淺 clone 的 HEAD 回答有，對不存在的 commit 回答沒有 |
| 乾淨、已經 push 的切到 pin | parley 在 `4cc636c`、pin 是 `8f8f6a4`：切到 `8f8f6a4` |
| 讀工作目錄裡的 pin | `third_party.toml` 改成 `4cc636c`、沒 commit：切回 `4cc636c` |
| `test.py` 的指令先同步 | 一致時 `test.py scenarios typing` 照樣跑；parley 有沒 commit 的改動、pin 對不上時印出原因、什麼都不跑，exit code 1；`test.py mutate`、`wpt` 開始時也看得到同步 |

**本機的測試規則配合每天的 CI**

| 項目 | 怎麼驗證 |
| --- | --- |
| `CLAUDE.md` 的「測試」、`docs/testing.md` 的「測試的種類」和「CI」、`docs/README.md` 的「一份 RFC 做完時」、`crates/dom` 的 README 和 `soundness.md` 寫的是同一套規則 | 讀過；`sanity.py docs` |

**量的時候修好的不穩定的測試**

| 項目 | 怎麼驗證 |
| --- | --- |
| `IdleDeadline.timeRemaining()` 在格點上（kun 的 bug） | `dom_js` 的 `bindings::idle_time_remaining_is_on_the_grid` 修之前每次都失敗；mutation test 兩條都抓到；`browser` 的 `idle_callbacks` 修好之後連跑 30 次都過 |
| `webdriver` 的 implicit wait 不依賴時間 | mutation test：`find` 一律只查一次時失敗 |

## 做過的檢查

2026-10-10，Windows 11。

- **第 1 段**：`ci.py third-party` 從檢查改成同步（`clone_pin`、`fetch_pin`、`own_work`、`origin_has`），拿掉 `--clone`；完整 clone 的 checkout（自己 clone 的 fork）fetch 時不加 `--depth`，免得變成淺的。`test.py` 的每個子指令開始前先同步（`sync_third_party`；`mutate --restore` 除外，它不編東西），`fuzz` 原本自己的檢查換成它。`CLAUDE.md` 的「第三方的 fork」加上「fork 每多一個 commit 就在同一次改動裡更新 pin」，parley 的 pin 照這條更新到 `4cc636c`。手動確認時找到並修好兩個 bug：`git status --porcelain` 的輸出經過 `git()` 時整段被 strip，檔名少了第一個字；判斷「`origin` 有沒有這個 commit」原本在 checkout 裡 `git fetch origin <commit>`，commit 已經在本機時 git 直接回成功、不問遠端，沒 push 的 commit 因此被切走（測試用的空 commit，parley 照樣還原），改成在暫時的空 repo 裡 `git fetch --depth 1 --filter=tree:0 <url> <commit>`，只抓 commit 本身。
- **nextest 的實測**（拿掉，第 15 條）：`cargo install cargo-nextest --locked`，0.9.148。`cargo nextest run --workspace --no-fail-fast -E 'not (binary(=scenarios) or binary(=speedometer))'`：1880 個測試，和 `cargo test --workspace` 一樣，`bindings::initialize_v8_before_main!` 在每個 process 照樣生效。編好之後交替各跑 5 次的中位數：`cargo test --workspace` 22.0 秒，nextest 27.9 秒（慢 27%）：每個測試一個 process，Windows 上開 process 加上初始化 V8 每個測試約 0.1 秒（最快的 binary 每個測試的中位數 0.07 到 0.13 秒；全部的中位數 0.22 秒、合計 536 秒）。macOS 沒有量。最慢的 20 個測試（給之後的「慢的測試」）：`shell` 的 `windowless::a_click_follows_an_element_a_change_just_moved` 9.9 秒、`windowless::waiting_until_the_page_is_busy` 5.5 秒、`windowless::a_query_waits_for_the_input_given` 3.5 秒；`browser` 的 `navigation::links::img_elements`、`navigation::links::lazy_images` 各 3.0 秒；`net` 的 `load::connection_failures` 2.4 秒；`browser` 的 `navigation::failed_loads_commit_an_error_page` 2.3 秒；`shell` 的 `script::testharness_results_come_back_or_time_out` 2.2 秒；`browser` 的 `navigation::query_element::a_child_frame_waits_for_its_stylesheets` 2.2 秒；`shell` 的 `windowless::failures_name_the_line_and_what_was_seen` 2.1 秒；其餘 `shell` 的 `webdriver`、`fetch`、`automation_pipe`、`windowless` 和 `dom` 的 `compile_fail` 在 1.3 到 2.0 秒。
- **mutation test 的範圍**（拿掉，第 15 條）：RFC 0030 第 4 段的 33 條照第 12 條的規則分類：33 條都是那一段新的行為，沒有 log、錯誤訊息、註解、只影響效能或明顯等價的；第 22、23 條改同一行，但分別驗同一個條件的兩個方向，「同一行最多一條」會漏掉一個方向；第 21 條改的是情境執行器的 Enter，規則沒說測試工具算不算。再掃 RFC 0026 到 0030 的 814 條：只改字串或註解的 15 條（`groove` 讀回來變 `ridge`、`set-cookie` 怎麼合併、`autocapitalize` 的無效值……，都是行為），改同一處的 21 條。kun 的 mutant 本來就一條對一個行為，規則減不了多少。
- **量的時候抓到的不穩定的測試**：`shell` 的 `webdriver::elements_are_found_after_an_implicit_wait`（`cargo test` 底下 1 次）假設導覽完到第二次查詢不到 200 ms，RFC 0025 之後 `shell` 的 240 多個測試在一個 process 裡同時跑，這段時間比較容易拉長；改成確認之後才由 Execute Script 開始計時。`browser` 的 `navigation::links::idle_callbacks`（`cargo test` 和 nextest 底下各 1 次，約 10%）是 kun 的 bug：`IdleDeadline.timeRemaining()` 把兩個在 100 µs 格點上的時間在 JS 裡相減，浮點的誤差讓長的 idle period 一開始剩下 `50.00000000000001`，超過 50 ms（Blink 用整數的微秒）；修在 `dom_js` 的 glue，取回格點。一開始誤判成「機器忙、callback 晚於期限」，把測試的 `rem>0` 放寬，又失敗才找到真正的原因，放寬的改動拿掉了。`html/semantics/forms/the-input-element/focus-dynamic-type-change.html` 在整套的 WPT（32 個 process）裡收到 `blur`、不再符合 `:focus-visible`，單獨跑 3 次都過，和這份的改動無關：另外開工作找原因，不改它的預期。
- **第 2 段**：`CLAUDE.md` 的「測試」（整套由每天的 CI 跑；每段工作做完跑改到的 crate，階段做完跑 `--workspace`；`wpt`、`reftest`、`scenarios` 只跑改到的部分；`matrix`、`fuzz`、`wdspec` 本機不跑），`docs/testing.md` 的「測試的種類」的 GC 那一列、「工具和跑法」、「CI」、「測試守不住的地方」的「各平台」和「CI」，`docs/README.md` 的「一份 RFC 做完時」的驗證，`docs/limitations.md` 和 `CLAUDE.md` 的「平台」，`crates/dom` 的 README 和 `soundness.md` 的 DCHECK 那一條。
- **模擬一份 GC 型別的 RFC（新的規則，Windows）**：每一步實測（3 次的中位數；會編的步驟先改 `dom` 的一行）：每段工作的 `cargo test -p dom -p dom_api -p dom_js` 9.9 秒，`sanity.py` 五項 2.4 秒，階段做完的 `cargo test --workspace` 38.9 秒、clippy 4.5 秒、fmt 1.4 秒，相關目錄的 `test.py wpt html/user-activation/ webmessaging/` 23 秒（第 20 條之後；之前 74 秒）；mutation test 拿 RFC 0030 第 1、2 段改 `dom`、`dom_api`、`dom_js` 的 21 條（換成合併的 binary 的測試指令）實跑，21 條都抓到、209 秒，平均 10 秒一條（改 `dom_api`、`dom_js` 的 5 到 7 秒，改 `dom` 的 11 到 16 秒，跑 `shell` 的端到端的 40 秒）。照 RFC 0030 的大小（4 段、每段 3 個工作段落、94 條 mutant、每個段落實作時跑 5 次測試、8 次相關的 WPT）估：mutation test 約 16 分鐘、實作時反覆跑改到的 crate 約 10 分鐘、相關的 WPT 約 3 分鐘、段落和階段的檢查約 5.5 分鐘，合計約 35 分鐘。測試數加倍時，mutation test 和相關的 WPT 跟著 RFC 的大小而不是總數，`cargo test` 在一個 process 裡平行，估計 40 到 45 分鐘；會讓它變慢的是慢的端到端測試（`shell` 的 `it` 的時間取決於最慢的那幾個）。
- **`test.py wpt`、`wdspec` 的路徑**（第 20 條）：手動確認，`test.py wpt html/user-activation/ webmessaging/` 0 個和預期不同；清單以外的一個檔案印出說明、不跑、exit code 1；`--all html/user-activation/` 照舊全部跑（15 個和預期不同）。`docs/test-gaps.md` 的「只做過手動確認」加一列，`tests/wpt/README.md` 寫試跑要加 `--all`。
- **檢查**：第 1 段做完、`IdleDeadline` 改完時各跑一次 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（最後 1881 個測試通過，多了 `idle_time_remaining_is_on_the_grid`）、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`；`IdleDeadline` 改了，跑了 `test.py wpt`（只有上面那個不穩定的）。第 2 段只改文件：`sanity.py docs`。不需要 mutation test 的：同步（第 6 條，手動確認）、文件。
