# RFC 0112 的進度：拿掉 event loop 的外圈

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-24

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0a. 盤點：外圈和內圈每個 task 之後的每一步，對照 Chromium 的原始碼 | 做完，待審查 | 2026-11-24 |
| 0b. 必壞的測試：通道一直有 task 時的 timer | 做完，待審查 | 2026-11-24 |
| 0c. 原型：DOM 的 task 進 scheduler，T1 | 做完，待審查 | 2026-11-24 |
| 0d. 原型：DOM 的 task 進 scheduler，T2 | 做完，待審查 | 2026-11-24 |
| 0e. 交給使用者決定範圍和做法 | 做完：做法第 3 條（T1），範圍第 4 條（A），分段第 5 條 | 2026-11-24 |
| 1. DevTools 的事件（第 6 到 9 條） | 做完，待審查 | 2026-11-24 |
| 2. 導覽在呼叫的當下送（第 10 到 12 條） | 做完，待審查 | 2026-11-24 |
| 2a. 必壞的測試：script 忙完之前 Browser 就收到導覽；同一個 task 的兩個導覽之間忙一陣，只 commit 第二個 | 做完，待審查：第一個在原本的程式上壞、2b 之後過；第二個在 2b 之後壞、2d 之後過 | 2026-11-24 |
| 2b. script 的導覽（`location`、`history`、連結）由 hook 當場送，帳在呼叫之後對照（第 10 條）；同一份文件內的 traversal 在 `popstate` 之前告訴 browser | 做完，待審查 | 2026-11-24 |
| 2c. parser 的 script 和背景圖的請求在每個 task 之後送，接著 blob URL 的撤銷（第 12 條） | 做完，待審查 | 2026-11-24 |
| 2d. 取消窗口（第 11、12 條）：發起導覽的 task 結束前，browser 不送它的 commit | 做完，待審查 | 2026-11-24 |
| 2e. iframe 的 `src`、`srcdoc` 的導覽也由 hook 當場送；frame 在載入中由 engine 當場告訴 DOM | 做完，待審查 | 2026-11-24 |
| 2f. 文件、mutation test | 做完，待審查 | 2026-11-24 |
| 3. DOM 的 task 是 scheduler 的 task（第 3、13 到 15 條） | 做完，待審查 | 2026-11-24 |
| 3a. 必壞的測試：通道一直有 task 時 timer 照樣按時間跑（第 0 階段的原型的）；設 timer 之後、跑過它的時間的 task 裡 post 的訊息在 timer 前面 | 做完，待審查 | 2026-11-24 |
| 3b. `dom` 的 `TaskWaker`：排 task 時送帶著它是哪一個的記號，timer、UA 的 timer、idle callback 的 timeout 送到期的時間 | 做完，待審查 | 2026-11-24 |
| 3c. scheduler 的 `TaskQueue::Dom` 和 delayed 的記號；engine 照記號跑一個 task | 做完，待審查 | 2026-11-24 |
| 3d. 主 thread 拿掉一輪的機制、`run_due_tasks`、`run_queued_tasks`；`until_next_due` 不問 DOM 的 timer | 做完，待審查 | 2026-11-24 |
| 3e. 文件、mutation test | 做完，待審查 | 2026-11-24 |
| 4. renderer 自己的工作排成 task | | |
| 5. 剩下的步驟，拿掉外圈 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 盤點 | `details.md` 的表；不改程式，所以沒有測試 |
| 原型 | 各自的 patch 在 `prototypes/`；通道一直有 task 時的 timer 的必壞測試必過，比較改動、成本、兩個 engine 各要做什麼 |

## 做過的檢查

### 第 0 階段（2026-11-24，Linux：Fedora 45、debug build，`0a459d1` 之上）

**盤點**：一個 subagent 唯讀地從 Gitiles 讀 Chromium `main` 的 `28312288d3ac`（2026-11-23）的原始碼，對了 `details.md` 的表的每一列和三個問題（scheduler 怎麼挑、同一個 task 的兩個導覽、iframe 插入時 DevTools 的順序）；它的關鍵說法（DevTools 的 DOM 變動每個 task 之後才送、delayed task 到期時才拿到 enqueue order）我對過它引的段落。沒對到的三項列在 `details.md`。

**必壞的測試**：沒改 13 次壞 13 次；Chrome for Testing 152 跑同一頁 5 次都是對的那一邊（`details.md` 的「原型」）。

**原型**：T1、T2 的必壞測試各 5 次都通過；整組通過：T1 是 `renderer` 127 個、`dom` 256 個、`dom_api` 228 個、`dom_js` 556 個、`shell` 779 個、`browser` 227 個；T2 是 `renderer` 127 個、`dom_api` 228 個、`dom_js` 556 個、`shell` 779 個、`browser` 227 個（T2 沒有改 `dom`）。原型不是要合併的程式，沒有 mutation test、clippy、WPT，效能沒有量；patch 都套得回 `0a459d1`。

### 第 1 階段（2026-11-24，Linux：Fedora 45、debug build，`e297d88` 之上）

**必壞的測試**：在第 1 段之前的程式上各跑 3 次、3 次都壞：新 frame 的三個事件的順序、建了又拿掉的 iframe、task 的 DOM 變動先於下一個 task。子 frame 的 `frameNavigated` 在它的 script 之前（`a_child_frames_document_is_told_before_its_script`）：第 8 條原本的做法（B）下，臨時的測試量到 `console` 先到；mutation test 的「子 frame 的 commit 沒有在引擎建文件之前告訴 Page agent」就是 B，這個測試抓到它。

**mutation test**（`mutants-1.toml`，24 個）：22 個抓到，2 個沒抓到，都是現在的程式裡別的地方也做了同一件事：
- 「frame 建立或拿掉時沒設旗標」：新的 frame 有 `src` 時，子 frame 送導覽之前照舊先比對；沒有 `src` 時，它的 `about:blank` 的 commit 設旗標；拿掉的 frame 由 hook 當場送 `frameDetached`。留著它，因為旗標的意思就是 frame 變了，第 2 段改了導覽、第 4 段把 commit 排成 task 之後可能就要靠它。
- 「頂層的 commit 沒設旗標」：DOM agent 在頂層 commit 時清掉每個 session 的節點，`documentUpdated` 之後 client 用命令重新要文件，回答命令之前一定先比對；Page agent 在 `create_window` 之前已經直接拿到新文件。留著它，讓 DOM agent 的 frame 清單跟著 DOM。

跑的過程裡補的測試：拿掉的 frame 先送裡面的 frame（`a_frame_gone_is_told_after_the_frames_in_it`，連同一個 task 裡建了外層和裡層又拿掉外層）、`javascript:` 的空文件的 loaderId（原本以為 `srcdoc` 是 renderer 自己 commit 的，其實也交給 browser）、繼承 origin 的 `about:blank`（`an_about_blank_of_its_initiators_origin_is_told_so`）、DOM 排著的 task 的變動先於下一個 task（`queued_tasks_changes_are_told_before_the_next_task`：第二個命令要在第一個跑完之前到，所以第一個命令忙 200 ms；時機不對時它照樣通過、只是抓不到）、新 frame 初始的 `about:blank` 不用等下一個命令（`a_new_frames_initial_document_is_told_as_its_task_ends`）、載入中的 frame 初始文件的 `init` 先到（`a_loading_frames_initial_document_is_told_before_its_own`）、沒有 session 的頁面照樣印出警告，timer 的也是（`script_warnings_are_logged_without_a_session`）。

過程中改的實作：拿掉「導覽開始或結束時設旗標」（只改 `navigating`，比對只在新的 frame 用到它，mutation test 看出來的）；外圈最後原本無條件的 `tell_frames`、`tell_dom_changes`、`log_console_warnings` 換成同一個「task 之後」（看旗標）；`log_console_warnings` 移到「沒有 session 就不做」之前（原本沒開 DevTools 的頁面的警告會一直堆在 DOM 裡）；`tell_document_created` 裡多餘的頂層判斷拿掉（呼叫它的地方只在子 frame 上，那個 mutant 和原本的程式等價）。

**相關 crate 的測試**：`dom`、`dom_api`、`dom_js`、`renderer`、`shell`、`browser` 都通過。

**階段結束的檢查**：`cargo test --workspace` 通過（60 個 binary，4701 個通過、1 個 ignored）；`cargo fmt --check` 和 `cargo clippy --workspace --all-targets -- -D warnings` 通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。

### 第 2 階段（2026-11-24，Linux：Fedora 45、debug build，`27be26c` 之上）

**必壞的測試**：
- `a_navigation_reaches_the_browser_as_script_asks`（`location.href` 之後忙 500 ms，browser 在忙完之前收到導覽）：原本的程式上壞（順序是回答、才開始導覽），2b 之後過。
- `an_iframes_navigation_reaches_the_browser_as_it_is_inserted`（插入有 `src` 的 iframe 之後忙 500 ms）：2e 之前壞，之後過。
- `of_two_navigations_in_a_task_the_second_alone_commits`（同一個 task 導覽兩次、中間忙 500 ms，只 commit 第二個、session history 只多一筆；頂層、子 frame、第一個是 `about:blank`）：原本的程式上過（兩個都在呼叫之後才送），2b 之後壞（頂層先 commit `/a`），2d 之後過。
- `a_frame_navigated_as_the_last_wait_goes_delays_the_load`（同一次呼叫裡導覽子 frame、再拿掉父文件最後等著的 iframe，父文件的 `load` 要等它；`src` 和 `location` 兩種）：原本的程式上就壞（`load,again`），是原本就有的問題，2e 修掉。

**過程中發現、修掉的**：
- `popstate` 的 handler 發起的導覽比 traversal 的 commit 先到 browser（browser 的 4 個 history 測試壞了）：traversal 的 commit 改在 `popstate` 之前送。
- browser 的 `blobs` 測試在整組跑時逾時（單獨跑會過；原本的程式上整組 3.5 秒都過）：導覽當場送之後，browser 在外圈送出撤銷之前就 commit 了。用暫時的 probe 印出 renderer 的訊息才找到（第 12 條）。
- 兩個 browser 的測試改了：`a_child_frame_commits_its_latest_navigation_alone` 扣住 `/b` 的 `BeginNavigation` 時也扣住它的窗口結束；`a_child_frames_commit_counts_the_new_entries_the_browser_has_not_heard_of` 的 `pushState` 移到下一個 task（同一個 task 的現在一定先到 browser）。
- `the_child_frames_are_told` 壞了：載入中的 frame 的初始文件的 `init` 排到 `frameStartedNavigating` 後面，改成 hook 送導覽之前由 Page agent 送；`a_loading_frames_initial_document_is_told_before_its_own` 改成在一個迴圈裡讀所有訊息（`init` 現在比 `Runtime.evaluate` 的回答早到）。

**mutation test**（`mutants-2.toml`，26 個）：24 個抓到，2 個沒抓到（記進 `docs/test-gaps.md`）：
- 「先結束取消窗口、再送撤銷」：等價，兩個訊息在同一次 `after_task` 裡接連送，commit 的結果要經過 renderer 才傳得出來。
- 「parser 的 script 和背景圖的請求等外圈才送」：parser 的 script（擋住 parser 的和 `async` 的）在 task 裡就由別的路送了；style 在畫 frame 時找到的背景圖可能只靠它，畫 frame 的時機不固定，沒有穩定的測試。

第一輪 17 個抓到、9 個沒抓到，補的測試：wasm engine 的 client（`a_navigation_is_told_to_the_frame_client_as_it_is_asked`）、`location` 導覽子 frame 的載入中（上面的 `load` 測試的第二種）、iframe 的 `src` 到 fragment（`a_child_frames_navigation_to_a_fragment_stays_in_its_document`；先用 `location.href` 寫抓不到，DOM 自己就當成同一份文件內的導覽）、通道一直有 task 時窗口和撤銷在 task 之後（`what_a_task_leaves_for_the_browser_goes_as_it_ends`，第 0 階段的 `fetch()` 串的做法）、`about:blank` 的 commit 被擋下（兩個導覽的測試的第三種）、擋下的錯誤頁（`a_failed_navigation_commits_its_error_page_once_its_task_is_over`）。第二輪有一個因為 spec 指定的測試命令（`browser`）不含新的測試而沒抓到，改成 `shell` 單獨重跑抓到了。第 1 段的 `mutants-1.toml` 有三個 `from` 對不上第 2 段改過的程式，更新了。

**相關 crate 的測試**：`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`browser`、`shell`、`protocol` 都通過。

**階段結束的檢查**：`cargo test --workspace` 通過（60 個 binary，4709 個通過、1 個 ignored）；`cargo fmt --check` 和 `cargo clippy --workspace --all-targets -- -D warnings` 通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過（`boundary` 的例外加了新的 thread-local 計數器 `NEXT_BEGIN_NAVIGATION_ID`，和請求的編號同一類）。

### 第 3 階段（2026-11-24，Linux：Fedora 45、debug build，`937061c` 之上）

**必壞的測試**（`shell` 的 `event_loop`）：
- `a_timer_runs_while_responses_keep_coming`（第 0 階段的原型的）：原本的程式上壞（`the timer ran after -1`），3d 之後過。
- `a_timer_takes_its_turn_as_it_is_due`（`setTimeout(f, 30)` 之後忙 60 ms 再 `postMessage`，訊息在 timer 前面）：原本的程式上壞（`timer,message`）；Chrome for Testing 152 跑 5 次都是 `message,timer`。3d 的第一版照原型在 post 時把到期的 timer 排進去，還是壞，改成挑下一個 task 時才排之後過（第 13 條）。

**過程中發現、修掉的**：
- WPT 的 `webmessaging/`：port 轉送之後的訊息卡住（4 個逾時）。一個是送給還沒啟用的 port 的訊息當場拿了編號，記號輪到時 port 不在清單裡，之後沒有記號了：port 啟用時重送。另一個是轉送清掉編號的訊息比後來的訊息晚拿編號，queue 的第一個不是記號的那個：同一個 port 照佇列順序拿編號。用暫時的 shell 測試印出順序才找到（`t0,t1,got1`，少了 2、3）。
- WPT 的 `webmessaging/message-channels/worker*`、`workers/WorkerGlobalScope-close.html`：worker 自己關閉時，它之前送的訊息和錯誤被丟掉（以前靠 loop 在處理 `Closed` 之前先跑完排著的 task）。
- `shell` 的 `forms::reset_buttons_and_links` 改成讀 `hashchange` 的 `newURL`（第 13 條）。
- 第一輪 mutation test 的上限設 1800 秒，第一個 mutant（排的 task 不送記號）卡滿 30 分鐘才算 hung：停掉、`--restore` 還原，上限改成 240 秒（最慢的正常的一輪含重新編譯約 190 秒），會讓測試一直等的 6 個 mutant 只跑 `event_loop::`（測試自己 30 秒就失敗）。

**WPT**（`html/webappapis/timers/`、`html/webappapis/scripting/event-loops/`、`webmessaging/`、`workers/`、`html/browsers/history/`、`html/semantics/embedded-content/the-iframe-element/`、`html/syntax/parsing/`，632 個）：沒有新的失敗；`negative-settimeout.any.worker.html` 和 `workers/baseurl/alpha/xhr-in-moduleworker.html` 是 OK、預期 ERROR，在 `937061c` 上也一樣（原本的預期結果就舊了，沒改）；`same-hash.html` 是標著的 known intermittent。

**mutation test**（`mutants-3.toml`，19 個）：第一輪 9 個抓到、11 個沒抓到；補了 `event_loop` 的 6 個測試（閒著的頁面的 timer、interval、`AbortSignal.timeout()`、忙著的頁面的 idle callback 的 timeout、啟用之前送到 port 的訊息、worker 關閉時丟的錯誤），拿掉 `PortMessages` 裡多餘的編號（它本身是 task，`after_task` 會掃描）和它的 mutant，重跑抓到 5 個。沒抓到的（記進 `docs/test-gaps.md`）：只看 DOM 自己的時鐘（造不出兩個時鐘的那一點差）、worker 關閉時丟的錯誤（`shell` 裡錯誤總是先跑；WPT 的 `WorkerGlobalScope-close.html` 抓得到）、wasm guest 的等待不叫醒 loop（畫面的 frame 照樣叫醒它）；等價的：task 之後不掃描 port（外圈也掃描）、關閉了的 worker 照樣收訊息（它的 thread 不跑了）。另外 `event_loop` 的 `a_moved_ports_messages_come_in_order`、`what_a_worker_posted_before_it_closed_comes` 把 WPT 抓到的兩件事寫成 shell 的測試。第 1、2 段的 spec 跟著更新了對不上的 `from`，第 1 段拿掉「DOM 排著的 task 之後不送」（那一段程式拿掉了）。

**相關 crate 的測試**：`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`browser`、`shell` 都通過。

**階段結束的檢查**：`cargo test --workspace` 通過（60 個 binary，4719 個通過、1 個 ignored）；`cargo fmt --check` 和 `cargo clippy --workspace --all-targets -- -D warnings` 通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。

