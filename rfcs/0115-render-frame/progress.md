# RFC 0115 的進度：每個 frame 的狀態照 content 和 Blink 的界線各有一個擁有者

開始日期：2026-11-25
完成日期：2026-11-26（Linux：Fedora 45，debug build；Speedometer 是 release）

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. renderer 那一側收成 `RenderFrames`（1a 八張表收成一張、1b 每種轉換一個方法、frame 不見時一起拿掉、1c debug build 檢查沒對帳就讀；已決定的事項第 2 到 10 條）。1c 的檢查在 3a 拿掉（帳在送出時就記好，沒有東西會過期） | 完成 | 2026-11-25 |
| 2a. body 的解碼和 parse、parser 擋住的和 deferred 的 script、`<link>` 的樣式表的請求和帳、分頁的標題搬到 engine 那一側（`dom_api` 的 `DocumentLoader`；第 11 到 20 條） | 完成 | 2026-11-25 |
| 2b. 樣式表剩下的 | 併進 2c（第 21 條） | 2026-11-25 |
| 2c. 「要畫面」是 widget 的、擋不擋 render 和失敗的子資源是 DOM 的；HTML 的「the end」和 DevTools 的 lifecycle（hook）；「接著 parse」是 DOM 的 task；script 和樣式表的回應交給 `dom_api`（第 21 到 28 條） | 完成 | 2026-11-26 |
| 3a. 導覽的帳在送出的那次呼叫裡記在 engine 的 frame 上；取消窗口由 client post 的 task 結束；同文件的歷史計數共用（第 29 到 31 條） | 完成 | 2026-11-26 |
| 3b. iframe 不送給 Browser 的導覽（fragment、`javascript:`、晚一點的 `load`）和 traversal 的捲動在 engine 那一側（第 32、33 條） | 完成 | 2026-11-26 |
| 3c. `handle_frame_requests` 的 12 處呼叫拿掉 11 處；task 之間讀 frame 的地方讀 engine 的 | 完成 | 2026-11-26 |
| 3d. engine 的 frame 清單是唯一的一份，renderer 的表是附屬的資料（第 34 條） | 完成 | 2026-11-26 |
| 4. fragment 的捲動 | 併進第 3 段（第 32 條） | 2026-11-26 |

每一段的停損點都沒碰到（README 的「分階段」）。

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 導覽 | 送出的導覽在同一次呼叫裡記在 engine 的 frame 上，換文件時帶著；照編號拿；iframe 的 `no-referrer`、`about:blank` 的 origin 的來源 | `dom_js` 的 `frames::the_navigations_sent_are_booked_on_their_frames`；`dom_wasm` 的 `engine::` |
| 導覽 | 一個 frame 一個導覽：`javascript:` 和一般的 `src` 兩種先後都是最後的那個 commit | `shell` 的 `documents::an_iframes_latest_navigation_commits` |
| 導覽 | iframe 的 fragment：網址、捲過去、不重新載入；別的 origin 的晚 100 ms 發 `load`、一串只發一次 | `shell` 的 `documents::an_iframes_fragment_navigation_scrolls_its_frame`；`browser` 的 `navigation::a_cross_origin_frames_fragment_navigation_fires_its_iframes_load` |
| 導覽 | `javascript:` 的空文件在自己的 task 裡 commit，父 frame 的 `load` 等它 | `shell` 的 `documents::a_parents_load_waits_for_an_iframes_javascript_url` |
| 導覽 | 同文件的導覽和 traversal 要畫面（`:target` 和 DOM 都沒變時） | `shell` 的 `browser_view::a_navigation_to_a_fragment_is_drawn`、`a_traversal_within_the_document_is_drawn` |
| 導覽 | 取消窗口、歷史計數、Browser 的 commit 和 drop | `browser` 的 `navigation::`、`shell` 的 `history::`（mutation test 3a） |
| 文件的載入 | parser 讓出、預算、body 的 pipe、`srcdoc` | `browser` 的 `navigation::a_parser_lets_timers_run_between_its_slices` 等；`shell` 的 `network::a_document_is_drawn_before_its_body_has_all_come` |
| 文件的載入 | parser 的 script：移走的不跑、microtask 的先後、inline module 的行號 | `shell` 的 `documents::a_parsers_script_moved_away_before_it_is_prepared_doesnt_run` 等 5 個 |
| 文件的載入 | 樣式表擋 script、擋 render、`load` 等著；DOMContentLoaded 時要的樣式表延後 `load`；同源的失敗讀不到規則 | `dom` 的 `link_sheets`；`dom_api` 的 `link_style`；`shell` 的 `network::stylesheets_block_the_parsers_scripts`、`network::a_child_frames_sheet_asked_for_later_blocks_its_rendering`、`documents::a_sheet_asked_for_at_dom_content_loaded_delays_the_load`、`documents::a_sheet_that_failed_from_its_own_origin_is_readable` |
| 文件的載入 | 子 frame 的 parser 被樣式表擋住時，`<link>` 拿掉就接著跑；被擋住渲染的子 frame 沒有 intersection | `shell` 的 `documents::a_child_frames_script_goes_on_once_its_blocking_sheet_is_gone`、`documents::a_render_blocked_child_frame_has_no_intersections` |
| DevTools | lifecycle（子 frame 的、DOMContentLoaded 和 `load` 只有主 frame 的一次）、網路是否安靜、log | `shell` 的 `devtools_lifecycle::`、`devtools_network::`、`devtools_log::a_sheet_that_fails_is_logged_with_its_url` |
| frame 的表 | 最上層先、子 frame 照建立的順序、換 engine 時 token 留著 | `renderer` 的 `render_frame::tests` |
| automation pipe | 輸入結束之前送的命令都有回答 | `shell` 的 `automation_pipe::the_commands_before_the_inputs_end_are_answered` |
| 整套 | `cargo test --workspace`（4900 個）、fmt、clippy、`test.py scenarios`（98 份）、`test.py reftest tests/reftests`、sanity | 第 3 段做完時跑 |
| WPT | `html/browsers/`、`the-iframe-element/`、`the-script-element/`、`html/syntax/parsing/`、`css/cssom/`、`intersection-observer/`、`resize-observer/`、`navigation-timing/`、`encoding/`、`dom/nodes/`、`dom/events/`：0 個退步 | 每一段跑（下面「原本就有的非預期的結果」） |
| 效能 | Speedometer 3.1：整份 +0.3%；每個 task 之後問的東西不走 frame tree | 下面「Speedometer」；`dom_js` 的 `frames::what_is_asked_after_every_task_walks_no_frame_tree` |

## 做過的檢查

**mutation test**（每一段一份 `mutants-*.toml`，在這個資料夾）：

- 1a（9 個）、1b（14 個）、1c（5 個）：都抓到，除了 1c 的「hook 送出時不設旗標」（只少檢查一次；1c 的檢查在 3a 拿掉了）。等價的拿掉：子 frame commit 之前先拿掉舊文件、`is_navigating` 多的那一行。
- 2a-1（10 個）、2a-2（30 個）、2a-3（36 個）：補了測試（2a-2 的 `shell` 3 個、`dom` 和 `dom_api` 的單元測試；2a-3 的 `shell` 5 個）、`browser` 的 `navigation::` 加進指令之後，2a-3 剩 5 個沒抓到：3 個只靠審查（有輸入在等時讓出、body 裡的 script 之後縮短預算、wasm 的 script 的 `load`）、2 個等價（外部 classic script 之前的 checkpoint、中止時放掉 deferred 的 pending script），記在 `docs/test-gaps.md`。
- 2c-1（12 個）、2c-2（29 個）、2c-3（8 個）、2c-4（23 個）：沒抓到的：中止時不設「載入結束」、DOMContentLoaded 之後和中止時和樣式表的回應之後要畫面（測試裡總有別的要了畫面）、排著 task 時資料來了照樣 parse、一輪開始時不清排著的 task、預算不隨讓出變大、`ParseOn` 認成 `Fire`、只等圖片時送圖片的請求：都記在 `docs/test-gaps.md`。
- 3a（26 個）：沒抓到的兩個是 3b 拿掉的暫時的佇列。3b（23 個）：「Browser 開始的 commit 不取代排著的 `javascript:`」沒抓到（測試排不出那個先後），記在 `docs/test-gaps.md`。3c（5 個）：「載完時父 frame 先於子 frame」等價（父 frame 等子 frame 的 `load` task 跑完）。3d（10 個）：「`doc` 不先問 engine」（現在沒有地方在 frame 拿掉之後、task 結束之前讀它）、「不放掉不見的 frame 的資料」（只是記憶體）記在 `docs/test-gaps.md`，「frame 增減時不讓 DevTools 重新比對」是原本就有的那一條。
- 等價而拿掉程式的：3a 送出導覽時設「frame 變了」、讀到的導覽一定已經拿走的兩處 `navigating`；3b iframe 的 fragment 要畫面；3d 走 frame 時照建立的順序排、清理時要畫面。
- timeout：1a、2a-1 第一輪 900 秒（等不到結果的頁面每個 `shell` 測試等滿 30 秒），改成 120 到 150 秒（正確的程式跑一次的兩倍多）；第 3 段 240 秒（改 `dom`、`dom_api`、`dom_js` 的 mutant 要重編它、重新 link 後面每個測試 binary，約 140 秒）。

**找到的 bug**：

- 1b：frame 不見時它排著的 owner `load` 沒有拿掉（`ChildFrames::remove` 漏了 `owner_loads`；第 8 條），看不到效果。
- 2a-1（這一段自己的）：沒有 pipe 的 body 的最後一份解碼兩次，`encoding_rs` panic；`BodyRead` 分成 `Over` 和 `Ended`。
- 2a-3：標題在 parser 的 script 之前改的會丟掉（第 19 條：標題經過 hook）。
- 2c-2：parse 的最後一輪才做出來的 `<link>` 的請求沒在 deferred script 之前送出（`parsed` 補上）。
- 3a（這一段自己的）：第一版先 `javascript:` 再一般的 `src` 時 `javascript:` 贏了（一般的在呼叫裡記好、`javascript:` 的在呼叫之後才排）；3b 讓兩者在 frame 的同一格。
- 2c 和第 3 段（Speedometer 量到的）：每個 task、每個 DOM task 之後問「有哪些 frame」的地方用了 `frames_in_tree_order`（給 rendering update 的：在它之外每次走過每份文件包含 shadow tree 的整棵樹找 iframe）：2c 的 `document_loader::run_dom_task` 找 parser 的 task（每個 DOM task），第 3 段的 `finish_loads`、`frames_with_documents`。原本是 renderer 自己的 `InternalTask::ParseOn` 和 `RenderFrames` 的表，用不著問。改成 engine 自己的清單（新的 `ScriptEngine::frames`，照建立的順序）；`dom_js` 的 `frames::what_is_asked_after_every_task_walks_no_frame_tree` 數 `frame_tree_walks_for_testing`，`mutants-perf.toml` 的 4 個裡 `dom_api`、`dom_js` 的兩個抓到，renderer 的兩個記在 `docs/test-gaps.md` 的「沒有計數守著的優化」。教訓：借用現成的方法時先看它在用的地方的成本；2c 做完時沒量 Speedometer（照規則等使用者要求），兩段的退步疊在一起才看出來。
- 原本就有的：automation pipe 讀到輸入結束時，`windowless::drive` 的 `select!` 可能先挑到結束、丟掉排著的命令（`a_client_not_reading_doesnt_hold_machopo_up` 偶爾失敗的原因）；使用者決定在 0115 上另一個 commit 修（`b8967eed`），新的測試把舊的行為放回去三次都抓到。

**查過、不是這份造成的**：

- `webdriver::a_session_with_a_hung_page_ends_in_time`：負載下（每個 CPU 一個空迴圈）約七、八成失敗，`65f6530a` 上一樣：測試送給 machopodriver 的一個 HTTP 請求 60 秒沒有回應。猜測（沒驗證）：navigate 在 `load` 之後還要 renderer 回答一次，renderer 進了頁面的無窮迴圈。之後在負載下加 log 查是哪個請求沒回。mutation test 跳過它。查的時候用 `pkill -f` 砍負載的迴圈，砍到指令自己兩次（確認過狀態重做，沒有丟改動）。
- 原本就有的非預期的結果（這台機器，`git stash` 回到改動之前照樣是）：`history_pushstate_url_rewriting.html`（OK）、`cross-document-traversal-cross-document-nav.html` 的 slow 那一個、`nav2-test-timing-persistent.html` 的 Part 1、`module/credentials.sub.html`、`script-crossorigin-network.sub.html`、`location-setter` 的 search 和 href（偶發）、`negative-settimeout` 的 worker；預期結果沒改。`test-timing-attributes-dependent-on-document-load.html` 偶爾通過（`redirectStart` 和 time origin 在這台機器上幾乎同時，小於 0 算 0；改動前後各跑 10 次都是 0 次通過），在 `.ini` 標成 known intermittent。
- 工具：`git stash` 之後 `dom_js` 的增量輸出舊了，`shell` 和 `browser` 連結失敗（`undefined hidden symbol`），`cargo clean -p dom_js -p shell -p browser` 修好；之後 mutation test 的基準要整個重編，先把測試指令跑一次再開始。

**Speedometer 3.1**（release，10 iterations，各 3 次輪流，AC 電源，renderer 是自己的 process）：

- 2a 做完時（`--against 66110866`）：20 個 suite 的幾何平均 +0.2%，每個 suite 的中位數都在兩邊的 95% 區間裡（最大 TodoMVC-Backbone -2.7%、TodoMVC-React-Complex-DOM +2.4%；NewsSite-Next +1.5%、NewsSite-Nuxt -0.9%、Editor-CodeMirror +0.9%、Editor-TipTap -1.9%）；記憶體峰值 -1.2% 到 +3.9%。
- 第 3 段做完時第一次（`--against ab6695a9`，2c 和第 3 段一起）：幾何平均 +6.6%，TodoMVC-Lit-Complex-DOM 23.4 → 40.0 ms、WebComponents 30.1 → 35.8、Vue 22.1 → 24.2、NewsSite-Next、Nuxt 約 +7%（上面「找到的 bug」的最後一條）；退步的 5 個對 `fb1c1d3e` 量：第 3 段 +13.3%（Lit 27.5 → 40.0），其餘是 2c 的。
- 修好之後，退步的 5 個對 `fb1c1d3e`：+0.3%。整份對 2a 之前（`--against 66110866`，2a、2c、第 3 段一起）：20 個 suite 的幾何平均 1.003（+0.3%），每個 suite 的中位數都在兩邊的 95% 區間裡（最大 TodoMVC-React-Redux +2.2%、TodoMVC-Backbone +2.1%、NewsSite-Next -3.1%；Lit +1.3%、WebComponents +0.8%）；記憶體峰值 -10.1% 到 +7.6%（Charts-chartjs 396 → 426 MB、NewsSite-Next 544 → 489 MB）。
