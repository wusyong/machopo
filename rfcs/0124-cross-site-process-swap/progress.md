# RFC 0124 的進度：跨網站的導覽換 renderer

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「Chromium」）、量 renderer 啟動的時間、已決定的事項第 1 到 12 條 | 做完 | 2026-11-27 |
| 1. 鋪路：commit 時才定送給哪個 renderer（`CommitParams`、`RenderProcessHost::commit_navigation`）；renderer 在導覽途中不在了照已決定的事項第 13 條（`WebContents::renderer_gone`）；自動化照分頁的 renderer 重新接上 DevTools 的 session（`WebContents::renderer_id`），繼續的導覽的 `Page.navigate`、`Machopo.waitForLoad` 繼續等 | 做完，待審核 | 2026-11-27 |
| 2a. 已決定的事項第 14 到 16 條；`DevToolsSessionState`（`RendererHostMsg::DevToolsSessionState`、`DevToolsAttach` 帶著它）；六個 agent 各自的 `state`、`restore`，renderer 在命令改了狀態時告訴 Browser；`WebContents` 記著它的 session 和狀態、換 renderer 時在 commit 之前接上（自動化不再自己重新接上、重送 `Page.enable`）；還沒回答的命令記著送給了哪個 renderer，當掉時以 `Target crashed` 失敗、換了時照第 15 條（換的那一半要第 3 段才測得到），renderer 不在時送來的立刻以 `Target crashed` 回答 | 做完，待審核 | 2026-11-28 |
| 2b. V8 的狀態（rusty_v8 的 fork 接出 `V8InspectorSession::state()`，fork 的測試 `inspector_session_state_round_trip`）、依網址的斷點（`bindings` 記下、重送，回答不給 client）、script 暫停或被打斷時 V8 處理完的命令也回報、新的 engine（下一個 renderer 的、另一種的）照最後回報的接上 | 做完，待審核 | 2026-11-28 |
| 3. 規則和換：跨網站的 commit 在新的 renderer；舊文件的 `unload`、逾時；thread 的 renderer 一樣換 | 還沒開始 | |
| 4. 收尾：量測工具、WPT、`--window` 的情境、文件、整個 workspace 的測試 | 還沒開始 | |

第 1 段開始之前先做 RFC 0125（網址列的偽造，2026-11-27 使用者決定）。

在別的機器上要做的：macOS 和 Windows 上量 renderer process 啟動的時間（已決定的事項第 5 條），明顯比這台 Linux 的 11 ms 慢時重新考慮提前啟動和預備的 renderer。

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| renderer 在 Browser 發起的導覽途中不在了（自己結束、壞訊息、`about:crash`）：導覽繼續，commit 時啟動新的 renderer（新的 frame sink）再送，新的 renderer 的 commit 收下 | `cargo test -p browser --test it renderer_gone::the_browsers_navigation_commits_in_a_new_renderer` |
| 頁面發起的導覽跟著 renderer 取消，history 不變、不為它啟動 renderer | `renderer_gone::a_pages_navigation_goes_with_its_renderer` |
| 頁面的 `history.back()` 跟著取消，Browser 的上一頁繼續 | `renderer_gone::a_pages_traversal_goes_with_its_renderer_and_the_browsers_goes_on` |
| 已經送出 commit 的跟著 renderer 丟掉，網址列回到目前的頁面 | `renderer_gone::a_navigation_sent_to_commit_goes_with_the_renderer` |
| 自動化：載入途中 renderer 被結束，`Page.navigate` 在新的 renderer commit 時回答，session 接到新的 renderer 上、命令照常 | `cargo test -p shell --test it automation_pipe::a_navigation_on_its_way_as_the_renderer_goes_commits_in_a_new_one` |
| 等著的東西在 renderer 不在了時失敗，繼續的導覽的 load 除外 | `windowless::a_renderer_gone_fails_what_waits_for_it` |

其餘的段做到時補上。

## 做過的檢查

### 第 0 步（2026-11-27，Linux：Fedora 45，x86-64，AMD Radeon 890M）

- 盤點讀的是 `main` 的 `acdd0704`；Chromium 的原始碼是當天的 `main`，由另一個 agent 抓下來讀、整理，關鍵的引文（`kUnloadTimeout`、`GetFrameHostForNavigation` 叫兩次、初始的 `SiteInstance`、paint holding 的條件、`file:` 的 site、WebUI 的規則、`about:` 改寫成 `chrome://`、`--single-process`）對著抓下來的檔案抽查過。
- renderer 啟動的時間：release、sandbox 開著，新分頁到跑出第一句 script 的中位數 12.3、12.4 ms（兩輪各 30 次），已有 renderer 的分頁導覽 1.3、1.4 ms（`details.md` 的「量的：啟動一個 renderer process 要多久」）。
- 沒有改程式。`python3 scripts/sanity.py docs` 通過。

### 第 1 段開始之前（2026-11-27，Linux）

- 臨時的測試（加在 `crates/browser/tests/it/visible_url.rs`，試完還原）：Browser 發起的導覽還在載入時 renderer 不在了（`RendererGone`），之後回應到了：commit 照樣送給不在了的那個 renderer（導覽在開始時就綁了它），分頁一直是 sad tab、什麼都沒 commit，網址列一直顯示要去的網址（那一次導覽一直在「送出 commit、還沒回報」）。RFC 0125 的已決定的事項第 3 條說要寫的測試，那時漏了；第 1 段改成 commit 時才定送給哪個 renderer 時一起處理。Chromium 的做法在 `details.md` 的「renderer 在導覽的途中不在了」。

### 第 1 段（2026-11-27，Linux：Fedora 45，x86-64，AMD Radeon 890M）

- **新的測試**：`browser` 的 `tests/it/renderer_gone.rs`（4 個；renderer 另一端沒有東西，三種結束的方式各跑一次），`shell` 的 `automation_pipe::a_navigation_on_its_way_as_the_renderer_goes_commits_in_a_new_one`。
- **改了的測試**：`windowless::a_renderer_gone_fails_what_waits_for_it` 原本預期 Browser 發起、還在路上的導覽的 `Machopo.waitForLoad` 也失敗：照第 13 條那次導覽繼續，它的 load 照樣等，其餘四種照舊失敗。
- **和原本的計畫不同**：`headless` 跟著換 sink、視窗清掉跟著舊 renderer 的 UI 狀態移到第 3 段（`README.md` 的「現在最接近的路：`relaunch`」）：這一段結束時換 renderer 只在 renderer 不在了之後，那時 `headless` 本來就以錯誤結束，視窗在 `RendererGone` 時就清掉了，做了也測不到。
- **mutation test**（`mutants-1.toml`，9 個）：全部抓到。
- **段落結束的檢查**：`cargo test --workspace` 5,042 個通過（1 個 ignored）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（抓到測試裡一個太複雜的型別，改成型別別名）、`sanity.py` 的每一項都通過。改了啟動 renderer 的時機和 commit 怎麼送，跑了量測工具的測試：四個都通過。
- **WPT**：`html/browsers/history/the-location-interface/`、`browsing-the-web/overlapping-navigations-and-traversals/`、`browsing-the-web/history-traversal/` 共 153 個測試。不符預期的只有 `overlapping-navigations-and-traversals/cross-document-traversal-cross-document-nav.html`：第一個子測試穩定地通過（RFC 0125 的 `progress.md` 記過，`main` 上也是）；第二個（「fast cross-document traversal and then slow cross-document navigation」）在跑整個目錄時通過了一次，單獨跑這個 branch 11 次都照預期失敗，`main`（`c527c2ee`）8 次裡通過 1 次：`main` 上本來就有的偶發。兩個都照 2026-11-27 使用者的決定不追。

### 第 2a 段（2026-11-28，Linux）

- **和原本的計畫不同**：重新接上 session 改由 `WebContents` 在換 renderer 的當下做（commit 之前，Chromium 的 `UpdateFrameHost` 也是），不再由自動化事後去接：事後才接，新的 renderer 在接上之前發出的請求 `Network` 看不到。第 2c 段（第 15 條）提前和 2a 一起做：2a 單獨做時，renderer 不在時送來的 `Page.enable` 沒人記，舊的「Browser 替 client 重送」拿掉之後就不見。原本以為 Chromium 會把當掉期間送來的命令送給下一個 renderer，讀了 `RenderFrameDevToolsAgentHost::UpdateFrameAlive` 才知道是重新載入時一起以 `Target crashed` 失敗（第 16 條）。
- **新的測試**：`browser` 的 `tests/it/devtools_session.rs`（4 個：命令改了狀態才回報、新的 renderer 不用再開就有 `Network` 的事件、六個 agent 都照狀態還原、太大的狀態是壞訊息）；每個 agent 的單元測試 `a_sessions_*_goes_on_in_another_agent`（6 個）；`shell` 的 `devtools::a_session_outlives_its_renderer` 改成當掉之前開 `Page` 和 lifecycle、重新載入之後不用再開就有 `load`，當掉期間的命令回答 `Target crashed`。
- **改了的測試**：`devtools::a_command_left_waiting_is_answered_when_the_renderer_goes` 的錯誤字串改成 `Target crashed`；`renderer` 的 `task_queue` 的測試裡 attach 的印法。
- **mutation test**（`mutants-2a.toml`，15 個）：全部抓到。換 renderer 時重送或失敗還沒回答的命令（`automation_session.rs` 的 `forward_to_renderer`）要有不當掉的換才測得到，第 3 段補測試。
- **不穩定的測試**：整套跑時 `shell` 的 `script::an_idle_deadline_is_the_callbacks`、`web_animations::a_replaced_animation_is_removed` 各失敗一次，單獨跑都通過：前者是 RFC 0012、0076 的 `progress.md` 記過的 idle callback 的 `timeRemaining()` 在 CPU 忙時落在 (0, 50] 之外，後者 RFC 0109 的 `progress.md` 記過（453 次裡 1 次）；都不碰 DevTools。
- 改到的 crate 的測試（`protocol`、`dom_api`、`dom_js`、`renderer`、`browser`、`driver`、`shell`）：除了上面兩個不穩定的和改好的 `task_queue` 的測試，都通過；`sanity.py` 的每一項都通過。

### 第 2b 段（2026-11-28，Linux；連結本機從 fork 的 checkout 編的 V8，`RUSTY_V8_ARCHIVE`）

- **fork**：`../third_party/rusty_v8` 接出 `V8InspectorSession::state()`（`src/binding.cc`、`src/inspector.rs`，`README.machopo.md` 的「Local modifications」），fork 的測試 `inspector_session_state_round_trip`（`cargo test --features v8_enable_pointer_compression --test test_api inspector_` 17 個通過）。改了 glue：release 之前 machopo 要連結本機編的 V8（`python3 scripts/ci.py v8 --profile release`，這次只重編改過的，108 秒）。還沒 commit、push、發 release。
- **新的測試**：`dom_js` 的 `devtools::a_session_goes_on_in_another_engine_from_its_state`、`a_breakpoint_removed_goes_from_the_state`、`a_state_that_isnt_one_starts_the_session_afresh`；`shell` 的 `devtools::a_breakpoint_set_while_paused_goes_on_in_the_next_renderer`、`the_pause_on_exceptions_goes_on_in_the_next_renderer`。
- **mutation test**（`mutants-2b.toml`，12 個）：全部抓到。第一次跑漏掉 3 個「回報狀態」的路：每次回報算的都是 V8 當下完整的狀態，原本的端到端測試用 `Runtime.evaluate` 讓 script 暫停，暫停結束後它從一般的路回來又回報一次，蓋掉了暫停那條路的缺漏（反過來也是）。改成兩個各自只走一條路的測試之後都抓到。script 跑著時打斷它的命令那條路沒有測試，記進 `docs/test-gaps.md`。
- 改到的 crate 的測試（`bindings`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`browser`、`driver`、`shell`）：2,230 個通過，0 個失敗。
- **段落結束的檢查**（連結本機編的 V8）：`cargo test --workspace` 5,057 個通過（1 個 ignored）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（抓到 `bindings` 的一個沒寫明溢位的負號、`shell` 的一個可以合併的 `if`，改了；改了寫法的 mutant 重跑，抓到）、`sanity.py` 的每一項都通過；`test.py wpt html/browsers/history/the-location-interface/`（改了 `automation_session`）0 個非預期的結果；量測工具的測試：`blink_perf`、`raster`、`census` 通過，`speedometer_test.py` 的 `test_the_windows_are_the_runners_times` 失敗一次，是 RFC 0125 的 `progress.md` 記過、`main` 上也會的那個（不追）。
- **merge 之前**：第 2b 段改了 fork 的 C++，現在的 prebuilt V8（`machopo-v152.2.0-30`）沒有 `state()`：CI 和其他機器連結 prebuilt，merge 之前要發 fork 的新 release、把 `RUSTY_V8_MIRROR_TAG` 和 `third_party.toml` 移過去（fork 的 `README.machopo.md` 的「Releases」）。

