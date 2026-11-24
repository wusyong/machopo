# RFC 0112 的進度：拿掉 event loop 的外圈

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-24。完成日期：2026-11-24（Linux：Fedora 45）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 評估：盤點外圈的每一步、對照 Chromium 的原始碼，DOM 的 task 進 scheduler 的 T1、T2 原型，交給使用者決定（已決定的事項第 1 到 5 條） | 完成 | 2026-11-24 |
| 1. DevTools 的 frame 事件當場送，DOM 的變動和 console 的警告在每個 task 之後（第 6 到 9 條） | 完成 | 2026-11-24 |
| 2. 導覽在呼叫的當下送、發起它的 task 結束才 commit；parser 的 script、背景圖的請求和 blob URL 的撤銷在每個 task 之後（第 10 到 12 條） | 完成 | 2026-11-24 |
| 3. DOM 的 task 是 scheduler 的 task，主 thread 拿掉一輪的機制（第 13 到 15 條） | 完成 | 2026-11-24 |
| 4. renderer 自己的工作和 V8 的 platform task 排成 task（第 16 到 18 條） | 完成 | 2026-11-24 |
| 5. 剩下的步驟改成 task 之後的觀察者和延後的 task，拿掉外圈（第 19 到 23 條） | 完成 | 2026-11-24 |
| 6. 做完之後：不重複排的延後 task 收進 scheduler、加新工作的說明（第 24 條） | 完成 | 2026-11-24 |
| 7. 做完之後：Speedometer 卡住，記號跑它自己的那一個 task、debug build 偵測丟掉的 task（第 25 條） | 完成 | 2026-11-24 |
| 8. `requestAnimationFrame` 在通道一直有 task 時不來：要不要 main frame 在每個 task 結束時告訴 compositor，headless 等 renderer 閒下來的 frame（第 26 條） | 完成 | 2026-11-24 |

## 驗收

`shell` 的測試是 `cargo test -p shell --test it <名稱>`，其他 crate 的同樣是 `cargo test -p <crate> --test it <名稱>`。「必壞」：改之前在原本的程式上每次都失敗。

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 評估 | 盤點和原型 | `details.md` 的表和「原型」；patch 在 `prototypes/` |
| DevTools | 新 frame 的 `childNodeInserted`、`frameAttached`、`executionContextCreated` 照 Chrome 的順序（必壞）；同一個 task 裡建了又拿掉的 iframe 也收到 `frameAttached` 和 `frameDetached`（必壞）；拿掉的 frame 先送裡面的 | `devtools::a_new_frames_events_come_in_chromes_order`、`a_frame_made_and_removed_at_once_is_told`、`a_frame_gone_is_told_after_the_frames_in_it` |
| DevTools | 子 frame 的 `frameNavigated` 在它的 script 之前（必壞），`javascript:` 網址的文件沿用初始 `about:blank` 的 context；新 frame 和載入中的 frame 的初始文件；繼承 origin 的 `about:blank` | `devtools::a_child_frames_document_is_told_before_its_script`、`a_new_frames_initial_document_is_told_as_its_task_ends`、`a_loading_frames_initial_document_is_told_before_its_own`、`an_about_blank_of_its_initiators_origin_is_told_so`；`dom_js` 的 `frames::the_frame_client_hears_of_a_child_frames_new_document` |
| DevTools | 一個 task 的 DOM 變動在下一個 task 之前（必壞），DOM 排著的 task 的也是；沒有 session 的頁面照樣印出警告 | `devtools_dom::a_tasks_changes_are_told_before_the_next_task`、`queued_tasks_changes_are_told_before_the_next_task`；`devtools_log::script_warnings_are_logged_without_a_session` |
| 導覽 | `location.href` 之後忙一陣，Browser 在忙完之前收到導覽（必壞）；插入有 `src` 的 iframe 也是（必壞）；wasm engine 的 client 也當場聽到 | `devtools::a_navigation_reaches_the_browser_as_script_asks`、`an_iframes_navigation_reaches_the_browser_as_it_is_inserted`；`dom_wasm` 的 `engine::a_navigation_is_told_to_the_frame_client_as_it_is_asked` |
| 導覽 | 同一個 task 的兩個導覽之間忙一陣，只 commit 第二個（頂層、子 frame、第一個是 `about:blank`）；錯誤頁也等 task 結束才 commit；通道一直有 task 時取消窗口和撤銷在 task 之後 | `devtools::of_two_navigations_in_a_task_the_second_alone_commits`、`a_failed_navigation_commits_its_error_page_once_its_task_is_over`、`what_a_task_leaves_for_the_browser_goes_as_it_ends`；`browser` 的 `navigation::links::blobs` |
| 導覽 | 同一次呼叫裡導覽子 frame、再拿掉父文件最後等著的 iframe，父文件的 `load` 等它（原本就有的問題，必壞）；子 frame 到 fragment 的導覽留在同一份文件 | `devtools::a_frame_navigated_as_the_last_wait_goes_delays_the_load`、`a_child_frames_navigation_to_a_fragment_stays_in_its_document`；`browser` 的 4 個 history 的測試（`popstate` 之前送 traversal 的 commit） |
| DOM 的 task | 通道一直有 task 時 timer 照樣按時間跑（必壞）；設 timer 之後、跑過它的時間的 task 裡 post 的訊息在 timer 前面（必壞，Chrome 的順序）；閒著的頁面的 timer、interval、UA 的 timer、忙著的頁面的 idle callback 的 timeout | `event_loop::a_timer_runs_while_responses_keep_coming`、`a_timer_takes_its_turn_as_it_is_due`、`an_idle_page_wakes_for_its_timer`、`an_interval_comes_again`、`a_ua_timer_comes`、`an_idle_callbacks_timeout_comes_while_the_page_is_busy` |
| DOM 的 task | 轉送的 port 的訊息照順序、啟用之前送到 port 的訊息；worker 關閉之前送的訊息和錯誤照樣送到 | `event_loop::a_moved_ports_messages_come_in_order`、`a_ports_message_waits_for_it_to_be_enabled`、`what_a_worker_posted_before_it_closed_comes`、`what_a_worker_threw_before_it_closed_comes`；WPT 的 `webmessaging/`、`workers/WorkerGlobalScope-close.html` |
| renderer 的 task | 通道一直有 task 時 parser 照樣續跑（必壞）；caret 照樣閃；延後的 iframe `load`；新內容的逾時 | `event_loop::a_parser_parses_on_while_responses_keep_coming`；`test.py scenarios caret`；`browser` 的 `navigation::a_cross_origin_frames_fragment_navigation_fires_its_iframes_load`、`a_new_document_that_cant_be_drawn_shows_empty_after_the_delay` |
| platform task | V8 的 `FinalizationRegistry` 的 cleanup 在 GC 之後跑（主 thread 和 worker） | `event_loop::a_finalization_registrys_callback_runs_after_a_gc` |
| 觀察者 | 通道一直有 task 時 `toBlob` 照樣完成（必壞）；選單、WebDriver 的結果、network idle、DOM 變了要 frame、一個 draw 只多一個 frame | `event_loop::a_canvas_blob_comes_while_responses_keep_coming`；`browser_view::a_selects_dropdown_is_over_the_page` 等選單的測試、`webdriver::`、`devtools_lifecycle` 的 `networkIdle`、`canvas::a_draw_asks_for_a_frame` |
| scheduler | 一直被要求的延後 task 只排最早的一個：同一個、比較晚、早不到 1 ms 的不再排，更早的再排，取出之後重新排 | `renderer` 的單元測試 `task_queue::tests::a_task_asked_for_again_is_posted_once` |
| 丟掉的 task | thread 忙過一個 timer 的時間之後再設一個已經到期的，兩個都跑，順序和 Chrome 一樣（`b,a`，必壞：改之前卡滿 30 秒）；debug build 在 loop 要等之前檢查沒有到期卻沒有記號的 DOM task | `event_loop::a_timer_set_after_an_earlier_ones_time_runs`；整套測試都經過 `assert_no_dom_task_lost` |
| main frame | 通道一直有 task 時 `requestAnimationFrame` 照樣來（必壞：改之前 3 次都是 `the frame came after -1`）；input 留下的 task（連結的 `hashchange`）在 headless 拿的 frame 之前跑完；sheet、body 之前沒畫、alternate 的 sheet 不擋畫面、canvas 的 draw 有自己的 frame | `event_loop::an_animation_frame_comes_while_responses_keep_coming`；`forms::the_users_clicks_check_boxes` 等 5 個；`network::a_document_is_drawn_once_it_has_a_body`、`a_stylesheet_blocks_rendering_until_it_loads`，`documents::a_linked_sheet_blocks_rendering`、`an_alternate_sheet_doesnt_block_or_apply`，`canvas::a_draw_asks_for_a_frame` |
| 全部 | 守著這些的測試擋得住弄壞的實作 | `python3 scripts/test.py mutate rfcs/0112-no-outer-loop/mutants-<段>.toml`（`1` 到 `5`） |

## 做過的檢查

Linux（Fedora 45）、debug build，2026-11-24。RFC 的草稿是 `0a459d1`；第 0 到 2 段（`970c695` 到 `937061c`）先 merge 回 `main`，第 3 到 5 段（`b52b562`、`5f243e5`、`7fbb149`）在它之上。

### 第 0 階段：評估

盤點由一個 subagent 唯讀地從 Gitiles 讀 Chromium `main` 的 `28312288d3ac`（2026-11-23），對了 `details.md` 的表的每一列和三個問題；它的關鍵說法我對過它引的段落，沒對到的三項列在 `details.md`。必壞的測試（通道一直有 task 時的 timer）沒改 13 次壞 13 次，Chrome for Testing 152 跑 5 次都是對的那一邊；T1、T2 的原型各 5 次都通過，相關 crate 的整組也通過（`renderer` 127、`dom` 256、`dom_api` 228、`dom_js` 556、`shell` 779、`browser` 227 個）。原型不是要合併的程式，沒有 mutation test、clippy、WPT，效能沒有量。

### 實作時找到的

- **第 1 段，子 frame 的 `frameNavigated`**：第 8 條原本選 B（task 結束時比對），以為 commit 自己是一個 task；實際上子 frame 的 commit 在同一個 task 裡建 realm、跑 inline script，`console` 先到（臨時的測試量到），改成 C1（engine 的 hook）。
- **第 2 段**：
  - `popstate` 的 handler 發起的導覽比 traversal 的 commit 先到 browser（4 個 history 測試）：traversal 改在 `popstate` 之前送。
  - browser 的 `blobs` 測試整組跑時逾時：導覽當場送之後，browser 在外圈送出撤銷之前就 commit 了（暫時的 probe 印出訊息才找到），第 12 條。
  - 載入中的 frame 的初始文件的 `init` 排到 `frameStartedNavigating` 後面：改成 hook 送導覽之前由 Page agent 送。
  - 父文件的 `load` 不等同一次呼叫裡導覽的子 frame：原本就有的問題，engine 送出時當場告訴 DOM frame 在載入中。
- **第 3 段**：
  - WPT 的 `webmessaging/`：port 的訊息照佇列的順序拿編號、啟用時重送記號（用暫時的 shell 測試印出順序才找到）。
  - WPT 的 `webmessaging/message-channels/worker*`、`workers/WorkerGlobalScope-close.html`：worker 自己關閉時它之前送的訊息和錯誤被丟掉，分開自己關閉和 `terminate()`。
  - 到期的 timer 照原型在 post 時排進去，`a_timer_takes_its_turn_as_it_is_due` 還是壞：改成 scheduler 挑下一個時才排（Blink 的 `SelectNextTask`）。
  - `forms::reset_buttons_and_links` 改成讀 `hashchange` 的 `newURL`（兩個 click 照 scheduler 的先後）。
- **第 4 段**：caret 和新內容的逾時的 task 提早被叫到時重新排自己，每次重新設定多一條鏈，改成只在設定的地方排；caret 的情境抓到 caret 來了、移了的地方沒排 task。`bindings` 改用 custom platform 之後 worker 的 loop 也跑 V8 的 platform task（以前不跑）。
- **第 5 段**：
  - 要不要 main frame 在每個 task 之後決定時，`shell` 的 6 個測試多畫一個 frame：一步一步拿掉試才確定，改成要等下一個 task 之前決定。
  - WPT 的 `webmessaging/*/018.html` 逾時：第 4 段把 `javascript:` 的 commit 排成 task，它排在頁面的 `postMessage` 前面、建了新的 window（第 4 段沒有跑 `webmessaging/`），第 23 條。

### Mutation test

| spec | 條數 | 抓到 | 沒抓到的 |
| --- | --- | --- | --- |
| `mutants-1.toml` | 22 | 20 | frame 建立或拿掉、頂層的 commit 不設旗標：別的地方也做了同一件事 |
| `mutants-2.toml` | 26 | 24 | 先結束取消窗口、再送撤銷：等價；task 之後不送 parser 的 script 和背景圖的請求（`docs/test-gaps.md`） |
| `mutants-3.toml` | 18 | 15 | 只看 DOM 自己的時鐘、worker 關閉時丟的錯誤（`docs/test-gaps.md`）；關閉了的 worker 照樣收訊息：等價 |
| `mutants-4.toml` | 16 | 12 | loop 等待時為 renderer 的延後 task 醒來、intersection 的 `delay` 提早時再排、V8 延後的 platform task、worker 為它醒來（`docs/test-gaps.md`） |
| `mutants-6.toml`（第 25、26 條） | 8 | 4 | element task 照編號找、engine 跑完時清掉 `target_task`：等價；engine 找不到時清掉（只有 debug 的偵測看得到）、headless 等那張 frame 畫出來（兩條通道的先後排不出）：`docs/test-gaps.md` |
| `mutants-5.toml` | 17 | 13 | `CSS` 的命令當場抓 sheet：等價；lifecycle 之後的 sheet、overlay 要 frame、wasm guest 的延後 task（`docs/test-gaps.md`） |

- 每一段第一輪沒抓到的，多半補了測試或改了 mutant 的測試命令（caret 的情境、`browser` 的延後的 iframe `load` 和新內容的逾時的測試、`webdriver::`）之後重跑抓到；補的測試在「驗收」。
- 第 3 段第一輪的上限設 1800 秒，第一個 mutant 卡滿 30 分鐘：停掉、`--restore` 還原，上限改成 240 秒，會讓測試一直等的 mutant 只跑 `event_loop::`。
- 做完時 spec 照最後的程式更新：拿掉外圈之後沒有對應程式的 2 條刪了（`mutants-1.toml` 的「off the channel」、`mutants-3.toml` 的 guest 的等待），程式搬到 `after_task` 或改了寫法的 4 條改寫，照新的 spec 重跑：3 條抓到，「task 之後不送 parser 的 script 和背景圖的請求」照舊沒抓到（同上）。`mutants-3.toml` 的「task 之後不掃描 port」原本是等價的（外圈也掃描），拿掉外圈之後重跑抓到。107 條的 `from` 在檔案裡都剛好出現一次（`mutants-5.toml` 決定要不要 main frame 的 2 條，第 26 條改掉了那段程式，刪了）。
- `docs/test-gaps.md` 裡 parser 停下時馬上開始載入的那一列：loop 一次跑一批時 `browser` 的 `a_child_frame_waits_for_its_stylesheets` 拿掉它時約六次失敗一次，拿掉外圈之後同樣的 mutant 跑 12 次都通過（停下的 task 之後馬上就是觀察者），改寫了那一列。
- `docs/test-gaps.md` 裡另外兩列在拿掉外圈之後有測試守著了，臨時的 mutant 確認之後刪掉：主 thread 不跑 V8 的 platform task（`shell` 的測試抓到）、task 之後不看標題（`shell` 或 `browser` 的測試抓到；以前一批的最後也看）。
- 做完之後的整理（第 24 條）：三處各自記著排了哪個時間的改成 scheduler 的 `post_internal_once_at`，`mutants-5.toml` 改寫 network idle 和 guest 的 2 條（照舊一條抓到、一條沒抓到），加 3 條：不看排過的、不留 1 ms 的餘裕、取出時不清掉，都抓到（餘裕的那條第一次沒抓到：單元測試只試了晚一點的，那本來就不排；改成試早一點的之後抓到）。`mutants-4.toml` 的 intersection 的 2 條照新的程式重跑，結果和第 4 段一樣（一條抓到，提早時再排一次的那條照舊沒抓到）。
- 每次跑完檔案都還原了。

### WPT

每一段跑相關的目錄：第 3 段 `html/webappapis/timers/`、`html/webappapis/scripting/event-loops/`、`webmessaging/`、`workers/`、`html/browsers/history/`、`the-iframe-element/`、`html/syntax/parsing/`（632 個）；第 4 段 parsing、`the-script-element/`、`intersection-observer/`、iframe、timers、`workers/`（798 個）；第 5 段再加 `the-select-element/`、`requestidlecallback/`（836 個）。最後一次沒有非預期的失敗。更新的預期結果：`history_pushstate_url_rewriting.html` 的 `javascript:'foo'?newsearch`（第 23 條，重跑兩次都通過）。和這份無關的非預期 PASS（在改之前的 commit 上也一樣，沒有改）：`negative-settimeout.any.worker.html`、`workers/baseurl/alpha/xhr-in-moduleworker.html`、`script-crossorigin-network.sub.html`、`module/credentials.sub.html`；`same-hash.html` 是標著的 known intermittent。

### 不穩定的測試

沒有新的。

### 跑過的檢查

- 每一段結束時 `cargo test --workspace`：最後一次（第 24 條的整理之後）60 個 binary，4722 個通過、0 個失敗、1 個 ignored（`bindings` 的 `gc_platform` 印出的 panic 是測試預期的）。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`：通過。
- `python3 scripts/sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs：通過；`boundary` 的例外加了 `NEXT_BEGIN_NAVIGATION_ID`（導覽的請求編號）和 `bindings` 的 `TASKS`（V8 的 platform 的 foreground task）。
- `test.py scenarios`（沒有視窗的）：第 4、5 段和第 24 條的整理之後，98 個都通過。
- 整套的 `wpt`、`wdspec`、`reftest`、`scenarios` 和 `matrix`、`fuzz` 由 CI 跑；這份動到 `bindings` 的 platform（V8 的 foreground task），merge 之後手動開始一次 `gc.yml` 看結果。

### 做完之後：Speedometer 和丟掉的 task（2026-11-24，Linux：Fedora 45，`perf` profile）

**和 RFC 開始之前的 `79a0ecc` 比**（`bench.py speedometer --against 79a0ecc5 --runs 3`）：
- 第一次：這一版 9 個 suite 不時卡住（3 輪共 18 次「160 s 沒有報告」，`79a0ecc` 0 次）；能比的 11 個的幾何平均 +5.9%。
- 二分（只跑 `Editor-TipTap`、`NewsSite-Next`）：`937061c`（第 2 段）不卡，`b52b562`（第 3 段）卡 `NewsSite-Next`。
- 卡住時 renderer 的 main thread 停在等下一個 task（`eu-stack`），CPU 幾乎是 0；頁面的 `setInterval` 也不跑。暫時的 probe 看到記號找不到它的 task：記號 `1189` 輪到時 timer queue 的第一個是 `894`。原因和做法在第 25 條。
- 先試的「每個 task 之後就要 frame」沒有修好卡住（5 個 suite 還卡 4 個）：`requestAnimationFrame` 在通道一直有 task 時不來是另一個問題（`event_loop` 的測試 3 次都壞），第 8 步做。
- 修好之後：卡過的 9 個 suite 單獨跑都完成；完整的比較 20 個 suite × 3 輪 0 次卡住，幾何平均 +3.3%（`TodoMVC-Svelte-Complex-DOM` +16.0%、`TodoMVC-Preact-Complex-DOM` +8.4%、`TodoMVC-Backbone` +5.9%，`TodoMVC-jQuery` -0.1%）。

**變慢在哪**：
- `--sample --against`：計時區間裡 main thread 的指令數 +0.1% 到 +2.3%（大多約 1%），cycle -4.4% 到 +8.0%（同一份程式換 build 本來就可能差 5%）；這份 RFC 加的程式（`after_task`、scheduler、照編號找 task）不在變最多的函式裡。
- 兩個版本各量一次每個測試的 sync 和 async：多在 Svelte、Preact 的 `Adding100Items`（sync 約多 0.7 ms、async 約多 1.3 ms），其他的差在 0.3 ms 以內。整個 suite 裡 main thread 的使用者空間 CPU 時間一樣（`perf stat` 的 `task-clock:u`：1768 對 1745 ms）。
- 試過、排除的：scheduler 每 post 一個 task 都 `notify_one`（Linux 上每次一個 `futex`）。改成只在有 thread 在等時才 notify，Svelte、Preact 各量 3 次，數字沒有變，改動還原了。
- 還沒驗證的：async 多的那一段可能是要 frame 的時機（第 8 步）；sync 多、指令沒多的那一段要看 kernel 的時間和 context switch（這台的 `perf_event_paranoid` 是 2，看不到）。

**第 26 條**（`requestAnimationFrame`）：
- 必壞的測試改之前 3 次都壞，改之後通過。照 A 改了之後 `shell` 有 5 個 input 的測試讀到舊的標題（headless 的 harness 的假設，第 26 條），改成等 renderer 閒下來的 frame 之後都通過；算畫了幾張的 5 個測試照本意改寫，單獨跑 5 次、整組跑 3 次都通過。
- 同時找到的不穩定：`devtools_dom::queued_tasks_changes_are_told_before_the_next_task` 整組跑時失敗過一次（第 3 段起的時序，和第 26 條無關），改成等第一個命令印出一行才送第二個，5 次都通過，它守的 `mutants-1.toml` 的 2 條照樣抓到。
- Speedometer（Svelte、Preact 各 3 次）：`Adding100Items` 的 sync 1.8 到 2.0 ms、async 6.6 ms，和改之前一樣；suite 的總時間也沒變。變慢的原因還沒找到。
- mutation test（`mutants-6.toml`）：第一輪 12 條 4 條抓到；沒抓到的 5 條看出 `dom` 取出 task 時清掉 `target_task`（`task_taken`）和 engine 的清除重複，拿掉 `task_taken`；「不要 main frame 等閒下來才說」也是多的，拿掉。照新的程式改寫之後 8 條 4 條抓到，沒抓到的見上表。
- 相關的 crate、`shell` 整組跑 3 次、98 個情境、152 個 machopo 的 reftest 都通過；WPT（`html/webappapis/`、`intersection-observer/`、`resize-observer/`、`requestidlecallback/`、`webmessaging/`、`workers/`、`html/syntax/parsing/`、`html/browsers/history/`、`the-iframe-element/`，892 個）沒有新的失敗。

**驗證**：`a_timer_set_after_an_earlier_ones_time_runs` 改之前 3 次都卡滿 30 秒，改之後 3 次都通過；`cargo test --workspace` 通過（60 個 binary，4723 個通過、1 個 ignored，偵測沒有觸發）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的各項通過。還沒做：第 25 條的 mutation test、相關的 WPT 目錄和情境。

**順便查過、沒有問題的**：parser 的續跑不會沒有 `ParseOn` 排著（`parse_due` 只在 post 它的地方設）；iframe 的導覽送出之後同一次呼叫裡 frame 被拿掉，renderer 不為它結束取消窗口，但 Browser 扣著的 commit 存在 frame 的節點上，跟著 frame 一起丟掉。

### 做完之後：Speedometer 慢的 3.3% 查到哪裡（2026-11-24，Fedora 45，`perf` profile）

- `requestAnimationFrame` 的修正（第 26 條）之後，Svelte、Preact 各量 3 次：時間沒有變，要 frame 的時機不是原因。
- `--trace`（兩個版本各錄一次 Svelte、Preact）：style、box tree、layout、paint 一樣；script 和 GC 多。照 span 比計時的區間：`EventDispatch` 916 → 1565 次（Svelte）、919 → 1911 次（Preact）。暫時的 probe 數了整個 suite 分派的事件，兩個版本一樣（`click` 2000、`input` 1000、`transitionrun` 1000……），所以是落在區間裡的不同，不是多分派了；GC 也一樣：scavenge 總數 12 → 13，區間裡 1 → 5（Svelte）。
- 原因之一是 V8 的 foreground task（`ScavengeJob`）：`79a0ecc` 用 V8 預設的 platform，renderer 在外圈 pump（`run_platform_tasks` 114 次），多半在計時區間外；第 4 段之後是 scheduler 的 task，輪到就跑，和 Chrome 一樣。實驗：讓它等 scheduler 沒有別的 task 才跑，Svelte、Preact 各量 3 次，Preact 16.8 → 16.1 ms（`79a0ecc` 15.5），Svelte 16.3 → 15.7 到 16.3 ms（14.1），沒有留下這個改動。兩個版本的 platform 都不支援 idle task。
- 沒查清楚的：Svelte 的 `Adding100Items` 同步的步驟裡 `v8.callFunction` 自己的時間 0.32 → 0.72 ms，指令卻沒多。要 kernel 的時間、context switch 或 cache 的計數（`perf_event_paranoid` 要放寬），留給「之後的可能」。
- 完整的和 Chrome 比（`d9eae09`）：比值的幾何平均 1.445，數字記在 Speedometer 量測工具那份 RFC 的 `progress.md`。

