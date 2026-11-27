# RFC 0124 的進度：跨網站的導覽換 renderer

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「Chromium」）、量 renderer 啟動的時間、已決定的事項第 1 到 12 條 | 做完 | 2026-11-27 |
| 1. 鋪路：commit 時才定送給哪個 renderer（`CommitParams`、`RenderProcessHost::commit_navigation`）；renderer 在導覽途中不在了照已決定的事項第 13 條（`WebContents::renderer_gone`）；自動化照分頁的 renderer 重新接上 DevTools 的 session（`WebContents::renderer_id`），繼續的導覽的 `Page.navigate`、`Machopo.waitForLoad` 繼續等 | 做完，待審核 | 2026-11-27 |
| 2a. 已決定的事項第 14 到 16 條；`DevToolsSessionState`（`RendererHostMsg::DevToolsSessionState`、`DevToolsAttach` 帶著它）；六個 agent 各自的 `state`、`restore`，renderer 在命令改了狀態時告訴 Browser；`WebContents` 記著它的 session 和狀態、換 renderer 時在 commit 之前接上（自動化不再自己重新接上、重送 `Page.enable`）；還沒回答的命令記著送給了哪個 renderer，當掉時以 `Target crashed` 失敗、換了時照第 15 條（換的那一半要第 3 段才測得到），renderer 不在時送來的立刻以 `Target crashed` 回答 | 做完，待審核 | 2026-11-28 |
| 2b. V8 的狀態（rusty_v8 的 fork 接出 `V8InspectorSession::state()`，fork 的測試 `inspector_session_state_round_trip`）、依網址的斷點（`bindings` 記下、重送，回答不給 client）、script 暫停或被打斷時 V8 處理完的命令也回報、新的 engine（下一個 renderer 的、另一種的）照最後回報的接上 | 做完，待審核 | 2026-11-28 |
| 3a. 已決定的事項第 17 到 19 條；`SiteInfo` 和 `site_info::swaps`；跨網站的 main frame 的 commit 在新的 renderer（`WebContents::swap_renderer`，和 `relaunch` 共用 `replace_renderer`）；舊的 renderer 收到 `RendererMsg::Unload`、跑 `pagehide` 和 `unload`，最多等 500 ms（`RenderProcessHost::unload`，thread 的打斷 script）；`headless` 換畫新的 sink、視窗清掉舊 renderer 的游標、capture 和選單；自動化的 `forward_to_renderer` 有了測試 | 做完，待審核 | 2026-11-28 |
| 3b. renderer 告訴 Browser 頁面被使用者操作過（`RendererHostMsg::DidReceiveUserActivation`）；換 process 時的 fallback（第 18 條：`Material::Surface` 的 `fallback`、Viz 告訴 Browser 新 sink 的第一個 frame，`ContentsEvent::FirstFrame`）；Browser 判斷能不能留（`WebContents::allows_paint_holding`）、同一個 renderer 時放進 `CommitNavigation` 的 `paint_holding`（第 19 條） | 做完，待審核 | 2026-11-28 |
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

### 第 3 段開始時（2026-11-28）

- 查證 Chromium（今天的 `main`）：`kProcessPerSiteUpToMainFrameThreshold`（content 預設開，一個 process 最多 2 個 main frame）在 Chrome 裡只用在預設的搜尋引擎（`kProcessPerSiteForDSE`），所以第 10 條的「Chromium 的預設也是不共用」照樣成立，補上例外（`details.md` 的「預備的 renderer 和重用」）；paint holding 整個在 Browser 和 Viz（`SurfaceRange` 的 fallback、Browser 的計時器、`DidNavigate` 時判斷），定了第 18、19 條。

### 第 3a 段（2026-11-28，Linux：Fedora 45，x86-64，AMD Radeon 890M；連結本機從 fork 的 checkout 編的 V8）

- **新的測試**：`browser` 的 `site_info` 的單元測試（8 個：別的網站換、同一個 site 留著、還沒有 site 的 renderer 拿第一個、`about:blank`、內建頁、`file:`、opaque 的 origin 和 `blob:`、重新整理、上一頁）；`browser` 的 `tests/it/process_swap.rs`（7 個：別的網站在新的 renderer、回到之前的網站又是新的、重新整理和 `about:blank` 留著、每個 `data:` 一個、內建頁、錯誤頁照它的網址、舊文件的子 frame 跟著走）和 `navigation::links::input_the_renderer_before_had_isnt_in_flight`；`shell` 的 `automation_pipe::commands_on_their_way_as_another_sites_renderer_takes_the_tabs_place`（第 15 條換的那一半：會跑 script 的失敗、其他的在新的 renderer 回答）、`a_documents_unload_runs_as_another_site_takes_its_place`（process 和 thread 兩種）、`a_renderer_paused_in_the_debugger_unloads_as_another_site_takes_its_place`、`a_renderer_whose_unload_doesnt_end_is_ended`，`browser_view::another_sites_renderer_has_none_of_the_ui_the_one_before_had`，`script::the_output_follows_the_tab_to_another_sites_renderer`（headless）。
- **mutation test**（`mutants-3a.toml`，19 個）：18 個抓到。第一次跑漏了「還沒 ack 的 input 歸零」：原本的測試裡舊的 compositor 在換之前就回了 ack；改成用 `DrivenViz` 擋住 frame、讓 ack 等著，抓到。漏的 1 個（commit 拿掉舊 renderer 的 `begin_id`）是等價的：新的 renderer 第一次 commit 之前沒有自己的導覽，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。thread 的 renderer `unload` 逾時被打斷、換的時候啟動失敗，測試看不到，記進同一份的「能補、還沒補的測試空缺」。
- **不穩定的測試**：整套跑 `shell` 時 `renderer_sandbox::a_renderer_process_is_told_this_machines_time_zone_by_name` 失敗一次（`TZ` 是 `None`），之後整套跑 5 次、單獨跑 6 次都通過。原因在測試：`seen` 回傳時 browser 已經被丟掉、renderer process 跟著結束，之後才讀它的 `/proc/<pid>/environ`，CPU 忙時讀到已經結束的 process 的空環境。和換 renderer 無關（那個測試只到 `about:blank`）：改成 browser 留到讀完。
- 改到的 crate 的測試（`protocol`、`renderer`、`viz`、`browser`、`shell`）都通過；`sanity.py` 的每一項都通過；`test.py wpt html/browsers/history/the-location-interface/`、`html/browsers/browsing-the-web/navigating-across-documents/`（換 renderer 的路，machopodriver 跑的是真的 process）0 個非預期的結果。
- **和原本的計畫不同**：第 3 段分成 3a、3b：畫面的部分（第 18、19 條）要 Viz 的協定、renderer 的通知和新的判斷，和換本身分開審查。3a 結束時換 process 中間是新的 sink 的背景色（P0 的樣子），3b 補上 fallback。

### 第 3b 段（2026-11-28，Linux：Fedora 45，x86-64，AMD Radeon 890M；連結本機從 fork 的 checkout 編的 V8）

- **和原本的計畫不同**：換的時候舊的 sink 沒畫過時，原本寫的是留著再之前的 fallback；照 Chromium 的 `TakeFallbackContentFrom`（「If the other view is not showing anything, we can't obtain a fallback」）改成什麼都不留，那條路也只在競態下走得到。`docs/limitations.md` 裡「新文件畫出來之前留著舊的畫面，不看是不是同一個 origin」那一條拿掉（第 19 條修好了）。
- **新的測試**：`viz` 的 `surface_aggregator::a_surface_shows_its_fallback_till_it_has_a_frame`、`thread::a_client_is_told_its_first_frame_once`；`browser` 的 `navigation::links::a_page_of_another_origin_shows_at_once_unless_the_user_acted_on_the_one_before`（同一個 site、不同的 port：從分頁的第一份文件留著，沒被操作過的當場空白，被操作過的留到畫出來，新的文件從沒被操作過開始，renderer 那一側也是）、`process_swap::the_sink_before_stands_in_till_the_new_one_draws_if_the_user_acted`（Browser 的狀態：被操作過才留著舊的 sink，新的第一個 frame 到了就忘掉）；`shell` 的 `browser_view::the_page_before_shows_till_another_sites_renderer_draws_if_the_user_acted_on_it`（真的畫出來的像素：被操作過時換的中間是舊頁面，沒有時不是；新的畫出來、新的 renderer 不在了時舊的都不再留著）。視窗的測試的 Viz 也照 Viz 的 thread 告訴 sink 它的第一個 frame。
- **mutation test**（`mutants-3b.toml`，20 個）：第一次漏了 2 個：「分頁的第一份文件不算」（沒有測試從第一份文件直接到畫不出來的頁面）、「換 renderer 時一律留著舊的」（新的 renderer 也被告知不能留、馬上交空白的 frame，像素只差幾毫秒）；補了第一份文件那一步和 Browser 的狀態的測試之後，20 個全部抓到。
- 改到的 crate 的測試（`protocol`、`compositor`、`viz`、`renderer`、`browser`、`shell`）都通過。
- **第 3 段結束的檢查**（連結本機編的 V8）：`cargo test --workspace` 5,084 個通過（1 個 ignored）；`cargo clippy --workspace --all-targets -- -D warnings` 通過；`cargo fmt --check` 有幾處排版，`cargo fmt` 改了；`sanity.py boundary` 抓到 `protocol` 的 `BeginFrameObserver::with_first_frame` 和它的欄位（closure 不能跨 process），和同一個 observer 的 `BeginFrame`、像素回來的 closure 一樣是 Browser 和 Viz 之間的（Viz 留在 Browser 的 process），加進 `PROTOCOL_ALLOWED`；之後 `sanity.py` 的每一項都通過。量測工具的測試（改了 renderer 怎麼啟動和結束）：`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py`、`census_test.py` 都通過。

