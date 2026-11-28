# RFC 0130 的進度：iframe 的 `sandbox`

開始日期：2026-11-28。完成：2026-11-28，Linux（Fedora 45，x86_64）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md`）、已決定的事項第 1 到 3 條 | 做完 | 2026-11-28 |
| 1. 旗標的路、origin 和 script：解析、`iframe.sandbox`、新的訊息、frame 樹的 pending 旗標、commit 時的旗標和 origin、`CommitNavigation` 的欄位；origin 和 script（含 autofocus、`<noscript>`）；改樹上沒有的 frame 的旗標是壞訊息；相關的 WPT | 做完 | 2026-11-28 |
| 2. 導覽和開新分頁：實測 Chrome 的 console 訊息；導覽別的 frame、最上層；開新視窗、新分頁帶旗標；沒有 `allow-popups` 開新視窗是壞訊息；history；console 的訊息；已決定的事項第 4 條；剩下的 WPT；收尾 | 做完 | 2026-11-28 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 旗標的路 | `sandbox` 照 Chromium 的 `ParseWebSandboxPolicy` 解析（token 的對照、大小寫、空白、不認得的 token 的訊息），旗標只能更嚴 | `protocol` 的 `sandbox::tests` |
| 旗標的路 | `iframe.sandbox` 是 `DOMTokenList`、`supports()` 照 token 表；建 frame 時帶著 iframe 自己的旗標，屬性改了時告訴 host | `dom_js` 的 `frames::an_iframes_sandbox_goes_to_its_frame` |
| 旗標的路 | Browser 的 frame 樹：子 frame 的旗標是 iframe 的加上父 frame 的文件的，改了的下一次導覽才算，main frame 和不在樹上的 frame 的是壞訊息 | `browser` 的 `frame_tree::tests::a_frames_sandbox_flags_are_its_iframes_and_its_parents`、`navigation::a_bad_message_ends_the_renderer`、`navigation::a_changed_sandbox_counts_from_the_next_navigation` |
| 旗標的路 | `about:blank` 照發起者的文件的旗標（sandbox 的 frame 把最上層導覽到 `about:blank`），自己導覽的沒有 | `browser` 的 `navigation::links::an_about_blank_is_sandboxed_by_its_initiators_flags` |
| origin | 沒有 `allow-same-origin` 的文件在 Browser 是 opaque 的 origin、請求不帶 cookie；巢狀的照父 frame 的旗標 | `browser` 的 `navigation::a_sandboxed_frames_document_is_of_an_opaque_origin` |
| origin | 沒有 `allow-same-origin` 的文件和它的 realm 父 frame 碰不到；opaque 的初始 window 不被沿用 | `dom_js` 的 `frames::a_document_sandboxed_so_is_of_an_opaque_origin`、`frames::an_opaque_initial_window_is_kept_for_no_document`；WPT 的 `sandbox-disallow-same-origin.html`、`sandbox-new-execution-context.html` |
| origin | opaque 的文件的 `document.cookie` 丟 `SecurityError`，`allow-same-origin` 的讀寫得到 | WPT 的 `sandbox_029.htm`、`sandbox_024.htm` |
| script | 沒有 `allow-scripts` 時 scripting 關著：`<script>`、handler 屬性、`javascript:` 網址不跑，`<noscript>` 是 markup，autofocus 不算，lazy 的圖片不 lazy；renderer 自己 commit 的 `javascript:` 文件照樣 sandbox | `dom_js` 的 `frames::scripting_is_disabled_in_a_frame_sandboxed_so`；`browser` 的 `navigation::a_sandboxed_documents_javascript_url_doesnt_run`、`a_sandboxed_document_doesnt_autofocus`、`a_sandboxed_frames_javascript_document_stays_sandboxed`；WPT 的 `sandbox-disallow-scripts.html`、`sandbox-parse-noscript.html`、`no-sandboxed-automatic-features.html`、`image-loading-lazy-move-into-script-disabled-iframe.html` |
| 導覽 | sandbox 的 frame 導覽自己和自己底下的，兄弟和最上層丟 `SecurityError`；`allow-top-navigation` 可以導覽最上層，`-by-user-activation` 要發起的 frame 的啟動；沒有 sandbox 的都可以 | `dom_js` 的 `frames::a_sandboxed_frame_navigates_its_own_and_those_below_it`、`a_sandboxed_frame_activated_navigates_the_top_level_one`；WPT 的 `iframe_sandbox_navigate_ancestor-1.html`、`-2.html`、`_descendants`、`_itself` |
| 導覽 | `Location` 的發起者是 incumbent realm：sandbox 的 frame 呼叫父 frame 的函式導覽父 frame 可以（`href`、`assign`、`replace`），自己直接導覽不行 | `dom_js` 的 `frames::a_location_is_navigated_by_the_incumbent_realm`；`browser` 的 `navigation::links::a_changed_sandbox_counts_from_the_next_navigation` |
| history | 沒有 `allow-top-navigation` 的 frame 的 `go(-2)` 會改到最上層時不走、只改自己的 `back()` 走；有的時候走；不在樹上的 frame 的是壞訊息 | `browser` 的 `navigation::links::a_sandboxed_frame_traverses_its_own_entries_alone`、`a_bad_message_ends_the_renderer`；WPT 的 `iframe_sandbox_navigate_history_go_back.html`、`_forward.html`、`_back-2.html` |
| 新視窗 | 沒有 `allow-popups` 時找不到 target 的連結不開（script 的 `click()` 也寫 console），連到自己的照樣導覽 | `dom_js` 的 `frames::a_sandboxed_frame_opens_no_new_window` |
| 新視窗 | 使用者的點擊：沒有 `allow-popups` 不開、renderer 不被結束；中鍵的新分頁照開（`new_window` 是 false）；有 `allow-popups` 時是新視窗 | `browser` 的 `navigation::query_element::a_click_in_a_sandboxed_frame_opens_a_window_as_its_flags_say` |
| 新視窗 | Browser：新視窗帶開它的 frame 的旗標（opaque 的 origin），`allow-popups-to-escape-sandbox` 的不帶，使用者的按鍵開的不帶；沒有 `allow-popups` 要開新視窗是壞訊息 | `browser` 的 `navigation::links::a_sandboxed_frames_new_window_is_sandboxed_by_its_flags` |
| console | 被擋的 script（`<script>`、handler、`javascript:` 網址）各一條，掛在文件的網址、沒有行號；沒有 window 的文件不寫 | `dom_js` 的 `frames::a_blocked_script_is_told` |
| console | 導覽的三種原因、目標的 origin（opaque 的發起者也是）、發起的文件的網址，掛在 script 的網址和行；新視窗的掛在每次點擊的行 | `dom_js` 的 `frames::a_sandboxed_frame_navigates_its_own_and_those_below_it`、`a_sandboxed_frame_opens_no_new_window` |
| console | DevTools 的 `Log`：`source`、`level`、原文、`url`、`lineNumber` 和 Chrome 實測的一樣；使用者的點擊（沒有 script 在跑）的新視窗的訊息掛在文件的網址 | `shell` 的 `devtools_log::what_a_sandbox_blocks_is_logged_as_chrome_logs_it`、`a_new_window_a_click_doesnt_open_is_logged_at_the_document` |
| WPT | WPT 逐項和 Chrome 一樣，不同的有原因 | `html/browsers/sandboxing/`、`the-iframe-element/` 的 sandbox 測試（`tests/wpt/include.txt` 的註解） |

## 做過的檢查

- **Chrome 實測**（Chromium 156，headless，`--remote-debugging-pipe`）：第 2 段開始前 attach 每個 target，記下每一條被擋時的 `Log.entryAdded` 的原文、`source`、`url`、`lineNumber`（`details.md` 的「Chrome 實測：被擋時的 console 訊息（第 2 段開始前）」）。讀 Chromium 的原始碼確認了三件和原本預想的不同的事（已決定的事項第 4 條、`details.md` 的「第 2 段讀的（2026-11-28）」）：使用者的按鍵開的新分頁走 `OpenURL`、不看 `kPopups`；`Location` 問的是 incumbent window；history 看的是 `kTopNavigation`，不是 `kNavigation`。
- **寫的時候找到、一起修的**：
  - 第 1 段：初始 `about:blank` 的 window 照網址判斷要不要沿用，那時還不知道旗標：sandbox 之後還沿用（WPT 的 `sandbox-new-execution-context.html`）。改成建 window 時就帶著旗標（`create_sandboxed_window`）；opaque 的初始 window 也不給別的文件沿用。lazy 的圖片的「will lazy load」有三處各自算、只看有沒有 window：抽成一個（`dom` 的 `will_lazy_load`），也看 scripting（WPT 的 `image-loading-lazy-move-into-script-disabled-iframe.html` 從 TIMEOUT 變成通過）。`about:blank` 之外，`data:` 網址也照發起者的旗標繼承（Chromium 的 local scheme）。
  - 第 2 段：`Location` 的導覽的發起者原本是 entry realm（網址解析用的那一個），導覽的規則做好之後，`a_changed_sandbox_counts_from_the_next_navigation` 的父 frame 的 `load` handler 被當成 sandbox 的子 frame 擋下；改成 incumbent realm（`OpFront::incumbent_context`，網址照舊照 entry realm）。
  - 第 2 段：使用者的點擊（沒有 script 在跑）被擋時，engine 去補訊息的位置，向 V8 要空的 stack 的第 0 個 frame，renderer 的 thread segfault（rusty_v8 的 `get_frame` 回 `Option`，但不檢查範圍）：先看 frame 的數目（`state.rs` 的 `script_location`），由 `a_click_in_a_sandboxed_frame_opens_a_window_as_its_flags_say` 守著。
  - 第 2 段：原本打算 Browser 對 sandbox 的 frame 的每一個 `OpenUrl` 都照 `allow-popups` 檢查；那樣使用者在 sandbox 的 frame 裡按 Ctrl 開連結會被當成壞訊息、結束 renderer。`OpenUrl` 多了 `new_window`。
  - 第 2 段：history 原本照 navigation 的旗標擋，對照 Chromium 的 `DoesSandboxNavigationStayWithinSubtree` 改成 top navigation 的旗標，補了有 `allow-top-navigation` 的情況。
- **寫測試時的錯誤（測試的，不是程式的）**：第 1 段：`contentWindow` 是 `WindowProxy`，換了 window 也是同一個物件，不能用 `===` 分辨沿用；沒有 `allow-same-origin` 的 frame 和父 frame 不同源，父 frame 給它的 `javascript:` 網址照 HTML 什麼都不做；`javascript:` 的空文件已經有根元素。第 2 段：訊息的行數預期錯了兩次（多行的 script 裡點擊在第二、第五行）；route 帶了含空白的 query，伺服器對不上；mutant 的測試指令寫錯了 module（點擊的測試在 `query_element`，寫成了 `links`），5 個 mutant 看起來沒抓到，改了指令就抓到。
- **WPT**：`html/browsers/sandboxing/` 和 `the-iframe-element/` 的 sandbox 測試（wpt.fyi 上 Chrome 跑的）在清單裡。最後 90 個檔案裡 41 個和 Chrome 一樣，49 個不同，都是 machopo 沒有的功能：`window.open`、表單送出、modal、下載和 `XMLHttpRequest`、`<video>`、PDF、`document.domain`（`sandbox-inherited-from-initiator-*` 靠設 `document.domain` 丟例外分辨，`data:` 的文件在 Chrome 是 opaque 的才丟）、CSP 的 `sandbox`。第 2 段讓 `iframe_sandbox_navigate_ancestor-1.html`、`_history_go_back.html`、`_history_go_forward.html` 變成通過。清單裡其他用到 sandbox 的 iframe 的 24 個測試第 1 段跑過，變成通過的拿掉了預期結果；第 2 段改了導覽的發起者，`html/browsers/history/the-location-interface/`、`html/browsers/browsing-the-web/navigating-across-documents/` 跑過，0 個非預期。`document.cookie` 對 opaque 的文件丟 `SecurityError` 原本只靠審查，現在由 `sandbox_029.htm` 守著（`docs/test-gaps.md` 那一列拿掉了）。
- **mutation test**：
  - 第 1 段（`mutants-1.toml`，36 個，都抓到）：第一次 3 個沒抓到，補了測試：「文件的旗標停在建立時」（frame 樹的測試建立和 commit 給的是同樣的旗標）；「sandbox 的文件的 origin 照網址」（只測了初始 `about:blank`，補了網址是父 frame 的 origin 的文件）；「sandbox 的文件會 autofocus」（WPT 的 iframe 沒有 `allow-same-origin`，補了有的，用 rendering update 先 flush autofocus、再跑 animation frame 的順序分辨，不靠時間）。拿掉一處等價的：parser 的 `scripts_never_run`。
  - 第 2 段（`mutants-2.toml`，47 個，都抓到）：導覽的每一條規則和訊息、新視窗（renderer、Browser、新分頁的旗標）、history、`Location` 的發起者、被擋的 script 的三處訊息、訊息的位置、DevTools 的欄位。第一次 46 個裡 7 個沒抓到：5 個是測試指令的 module 寫錯（上面）；「放過位置的訊息再放一次」：兩條訊息在同一行，補了第二次點擊在另一行；「history 用發起者的 frame」是等價的（`history.go()` 的發起者就是 `History` 的文件），拿掉了 `.or(asked.initiator)`。之後補的「沒有 script 時新視窗的訊息沒有網址」也抓到。
  - 等價的：`data:` 網址照發起者的旗標（`docs/test-gaps.md` 的「等價、不用處理的 mutation」）：sandbox 的發起者能導覽的不是自己底下的（已經帶著旗標）就是最上層，最上層不能導覽到 `data:`。
- **不穩定的測試**：沒有碰到。`a_sandboxed_frame_traverses_its_own_entries_alone` 有 `allow-top-navigation` 的情況等兩個 frame 的事件（先後不定），等的是 Browser 的 history 的位置，不是事件的順序；連跑 30 次都過。
- **視窗**：沒有新的情境：看得到的（script 不跑、opaque 的 origin、被擋的導覽和新視窗）由 WPT 和整合測試守著，使用者的點擊由 `browser` 的整合測試（真的 input、hit test）守著。`test.py scenarios --window` 兩段做完時各跑一次，75 個都過。
- **量測工具的測試**：沒有跑。沒有改 renderer 的啟動、trace、`base::census` 或 automation pipe 的 CDP 方法（`Log` 的訊息是既有的路）。
- **跑過的**：
  - 第 1 段：`cargo test -p protocol -p dom -p html -p dom_api -p dom_js -p dom_wasm -p browser -p renderer -p shell` 全過；`cargo test --workspace` 5128 個通過、0 個失敗、1 個忽略，`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過（clippy 找到的：`split` 改用字元的陣列，`child_frame_created` 的參數包成 `ChildFrameMade`）。
  - 第 2 段：`cargo test -p protocol -p dom -p html -p dom_api -p dom_js -p browser -p renderer -p shell` 全過；`cargo test --workspace` 5139 個通過、0 個失敗、1 個忽略；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs 都過；WPT 的 sandbox 和導覽的目錄（290 個檔案）0 個非預期。
