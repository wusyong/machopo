# RFC 0115 的進度：每個 frame 的狀態照 content 和 Blink 的界線各有一個擁有者

開始日期：2026-11-25

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. renderer 那一側收成 `RenderFrame`（已決定的事項第 2 到 10 條） | 做完，待審查 | 2026-11-25 |
| 1a. `render_frame.rs`：`RenderFrames`、`RenderFrame`、`NavigationRequest`，`FrameDocument` 搬進來；最上層的 `document` 和 `ChildFrames::documents` 改成表上的 `document`（第 2、3、4 條） | 做完，待審查 | 2026-11-25 |
| 1b. 導覽的帳（`ChildFrames::navigations`、`main_origin_source`）、`owner_loads`、`devtools_main_frame_token` 搬進 `RenderFrame`，每種轉換一個方法（第 3、5 條）；frame 不見時一起拿掉（第 8 條） | 做完，待審查 | 2026-11-25 |
| 1c. debug build 在讀導覽的帳時檢查沒分派的 `FrameRequest`（第 6 條，做法照第 9 條） | 做完，待審查 | 2026-11-25 |
| 2a. body 的解碼和 parse、parser 的 script、`<link>` 的樣式表的請求和帳搬到 engine 那一側（已決定的事項第 11 到 20 條） | 做完 2a-1 到 2a-3，待階段的檢查 | |
| 2a-1. `dom_api` 的 `LocalFrame`：解碼器、parser、parser 的輸入；engine 的 `body_data`、`body_ended` 和「parse 到 script 或沒輸入為止」，renderer 只交 pipe 讀到的 bytes（parse 照舊由 renderer 的 `ParseOn` 排，碰到 script 是回傳值） | 做完，待審查 | 2026-11-25 |
| 2a-2. `<link>` 的樣式表：掃描、media 的判斷、請求（`LoadSink` 的 hook）、「擋 script、擋 render、`load` 等著」的帳搬到 DOM 和 `dom_api`（第 18 條）；回應照舊由 renderer 收、解 CSS、交給 DOM。parse 的迴圈還在 renderer，跑 script 之前叫 engine 送出樣式表的請求 | 做完，待審查 | 2026-11-25 |
| 2a-3. parser 擋住的和 deferred 的 script 搬進 `DocumentLoader`（DOM 自己排的 script 的那條路），`advance_document` 的迴圈搬過去；分頁的標題經過 `FrameClient` 的 hook（第 19 條） | 做完，待審查 | 2026-11-25 |
| 2a-4. 「接著 parse」是 DOM 的一種 task，讓出時問 `InputMonitor`；拿掉 `InternalTask::ParseOn` | 移到 2c（第 20 條） | 2026-11-25 |
| 2a 的停損點：新增的 hook 和問 engine 的方法、Speedometer 的 Editor 和 NewsSite、parse 迴圈的過期讀法（「信心和未知數」）。第 1 項：2a-2 時跨界線的新方法到了 5 個（`ScriptEngine` 的 `document_loader`、`set_document_loader`、`send_sheet_fetches`，`LoadSink::start_sheet_fetch`，`FrameLayout::media_environment`），使用者 2026-11-25 看過：多出來的是第 18 條把原本 2b 的樣式表併進來，可以接受，不用算得更細；2a 做完時 7 個（多了第 19 條的 `tell_title`、`title_changed`）。第 2 項：Speedometer 幾何平均 +0.2%，都在區間裡（README 的「第 2 段的停損點」）。第 3 項：不在了 | 三項都順 | 2026-11-25 |
| 2b. 樣式表剩下的：回應的解碼和 parse、`<link>` 的 `load` 和 `error`、DevTools 的原文 | 併進 2c（第 21 條） | 2026-11-25 |
| 2c 的依賴盤點：「the end」、iframe 的 `load`、「跑完 script 之後 frame 還在不在」、樣式表的回應、「接著 parse」碰到哪些 renderer 的狀態（`details.md` 的「2c 的依賴盤點」） | 做完；決定的事項第 22 到 28 條 | 2026-11-26 |
| 2c-1. 基礎：「要畫面」的旗標和計數搬到 `Frame` 的 widget（`FrameLayout::schedule_frame`，第 23 條）；擋不擋 render 要用時從 DOM 算，拿掉 `update_child_blocking`、`parsing`、兩份 `body_parsed`（第 26 條）；失敗的子資源全部記在 DOM，拿掉 `FrameDocument::failed`（第 27 條）。行為不變 | 做完，待審查 | 2026-11-26 |
| 2c-2. 「the end」搬到 engine 那一側：`Interactive`、deferred script 之後的 DOMContentLoaded、捲到 fragment、`load`、中止的文件；DevTools 的 lifecycle 經過 hook（第 22 條），`DidFinishLoad` 和 idleness 由 renderer 問；`parse` 的回傳只剩排程要的 | 做完，待審查 | 2026-11-26 |
| 2c-3. 「接著 parse」是 DOM 的 task，拿掉 `InternalTask::ParseOn`；`yields`、`parse_due` 搬到 `DocumentLoader`；`dom_api` 的 `run_dom_task` 先看是不是 parse 的（第 14、20、24、28 條）。原本排在回應之後，2026-11-26 對調：回應的函式裡接著 parse 時可能讓出，讓出要先是 DOM 的 task，回應的函式才不用把「讓出了」交回 renderer | 做完，待審查 | 2026-11-26 |
| 2c-4. 樣式表和 script 的回應交給 `dom_api` 的函式：解碼、parse 成 CSS、設 sheet、`load` 和 `error`、接著 parse；renderer 只交 bytes（第 21、24、27 條） | 做完，待審查 | 2026-11-26 |
| 2c 的停損點：決定之外又多 3 個以上的 hook 或問 engine 的方法；要共用的 `Rc<RefCell>`，或新的「呼叫之後記得做」；DevTools 的 lifecycle 順序、WPT 或 `shell` 退步而修不乾淨；Speedometer 在使用者要求時量（2c 做完時提醒）。評估在 README 的「2c 做完時」：多兩個、沒有共用的狀態和新的呼叫之後的步驟、沒有退步 | 三項都順；Speedometer 未量 | 2026-11-26 |
| 第 3 段的依賴盤點（`details.md` 的「第 3 段的依賴盤點」） | 做完；決定的事項第 29 到 32 條 | 2026-11-26 |
| 3a. 導覽的帳在 engine 的 frame 上：`NavigationRequest` 搬到 `dom_api`，`FrameClient` 送出導覽時 engine 當場記在那個 frame 上（`dom_js` 的 `Frame`，換文件時帶過去；`dom_wasm` 的最上層），Browser 的 commit 和 `NavigationDropped` 經過 engine 拿；取消窗口結束由 client post 的 task 送（第 30 條）；同文件的歷史計數共用（第 31 條）。拿掉 `FrameRequest::Navigation`、`navigations_asked`、`RenderFrame` 的導覽、第 1 段的檢查 | 做完，待審查 | 2026-11-26 |
| 3b. iframe 的 `src`（`FrameRequest::Navigate`）在 engine 那一側：fragment 的照現在的行為，經過 `FrameLayout` 捲、要畫面，traversal 的捲動也是（第 32 條）；`javascript:` 的 commit 記在 frame 上導覽的同一格，經過 `FrameClient::commit_here` 叫 renderer 跑（第 33 條）；晚一點的 `load` 是 DOM 延後的 element task（第 25 條）。拿掉 `RenderFrames` 的 `commits`、`owner_load_at` 和 `OwnerLoads` | 做完，待審查 | 2026-11-26 |
| 3c. 拿掉只為了對帳的 `handle_frame_requests`（12 處裡的大約 11 處），還在的四處過期的讀法改成讀 engine 的 frame 的文件 | 做完，待審查（12 處拿掉 11 處，只留 `after_task` 的） | 2026-11-26 |
| 3d. engine 的 frame 清單是唯一的一份（第 34 條）：`doc`、`doc_mut` 只回傳 engine 還有的 frame 的、初始文件的那一份第一次用到時建；走表的四處照 engine 的 frame；`ScriptEngine::frame_of_document`；`follow_child_frames` 只剩 task 之後的清理，`commit_child` 裡的那一次拿掉。停損點：要新的 hook、或要第 31 條之外的共用可變狀態；有地方在 task 結束之前就要初始文件的資料而晚建不行；DevTools、WPT、`shell`、`browser` 退步而修不乾淨 | 做完，待審查（停損點都沒碰到） | 2026-11-26 |
| 第 3 段小步的停損點：決定之外又多 3 個以上的 hook 或問 engine 的方法；要第 31 條之外的共用可變狀態，或新的「呼叫之後記得做」；DevTools、WPT、`shell`、`browser` 退步而修不乾淨；Speedometer 在使用者要求時量（做完時提醒）。做完再評估完整的那一半（第 29 條） | — | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

### 第 3 段做完時（2026-11-26，Linux：Fedora 45，3d 和 automation pipe 的修正之上，未 commit）

- `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（3c 之後修了一個：`dom_js` 的 `navigate_child` 的 `let ... else` 改成 `?`）：通過。
- `cargo test --workspace --no-fail-fast`：4900 個通過，0 個失敗（3c 之後第一次跑時停在下面的 `a_client_not_reading_doesnt_hold_machopo_up`）。
- `test.py scenarios`（98 份）、`test.py reftest tests/reftests`（3c 之後）、`test.py wpt dom/events/`（改了 automation pipe：217 個，0 個非預期）、sanity 的 deps、spans、boundary、unsafe、tests、numerics、docs：通過。
- **`automation_pipe::a_client_not_reading_doesnt_hold_machopo_up` 的原因和修正**（2a-3 記的那個）：automation pipe 的 reader 把命令送進 `commands`、讀到 EOF 時在另一個 channel 送結束，`windowless::drive` 用 crossbeam 的 `select!` 同時等兩個，兩邊都有東西時隨機挑：機器忙、主迴圈落後時，排著幾百個命令也可能先挑到結束、提早返回，寫出去的回答不夠填滿 pipe，writer 沒卡住，沒有那一行（另一次「結束得比 5 秒慢」是同一處的時序）。和這份 RFC 無關，原本就有；使用者決定在 0115 上另一個 commit 修：`drive` 收到結束時先照順序處理 `commands` 裡排著的（`run_command`，兩處共用），Chromium 的 pipe 也照順序處理訊息。新增 `automation_pipe::the_commands_before_the_inputs_end_are_answered`（送 1000 個命令後關掉輸入、讀完輸出，每個都有回答）：把舊的行為放回去當 mutant，三次都抓到；修好之後那個測試單獨跑 10 次、整套 `shell` 跑 3 次都通過。
- 停損點：hook 和問 engine 的方法比原本多 `FrameClient::commit_here`（第 33 條）和 `ScriptEngine::navigation`、`take_navigation`、`take_commit_here`、`frame_of_document`；共用的可變狀態只有第 31 條的計數；沒有新的「呼叫之後記得做」（拿掉了 `FrameRequest` 的兩種導覽、`navigations_asked`、11 處 `handle_frame_requests`、第 1 段的檢查，`follow_child_frames` 只剩 task 之後的清理）；DevTools、WPT、`shell`、`browser` 沒有退步：都沒碰到。Speedometer 還沒量（使用者要求時量）。

### 3d：engine 的 frame 清單是唯一的一份（2026-11-26，Linux：Fedora 45，debug build，3c 之上，未 commit）

**做了什麼**：`doc`、`doc_mut` 先問 engine 有沒有那個 frame（`has_frame_document`：最上層的是 commit 時建的那一份；子 frame 的要 engine 還有它，初始文件的那一份第一次用到時建，`initial`、預設的計數和偵測，和原本 task 之後建的一樣），`doc` 改成 `&mut self`。走表的地方照 engine 的 frame（`frames_with_documents`：`frames_in_tree_order()`）：`scripts_blocked_by_sheets`、`check_network_idle`；`frame_of_document` 問 engine（新的 `ScriptEngine::frame_of_document`，`dom_js` 照 `Frames::of_document`、`dom_wasm` 照它唯一的文件）；`next_idleness_due`（`&self`）照舊走表（不見的 frame 留著的只會讓 loop 早醒一次）。`follow_child_frames` 改成 `forget_gone_frames`：只在 task 之後放掉 engine 沒有的 frame 的資料、取消不見的文件的 fetch、設 DevTools 的「frame 變了」；建的那一半、`commit_child` 裡的那一次、要 main frame 的那一行拿掉。test-gaps 拿掉兩條不再成立的（task 中間過期的清單、`commit_child` 不當場對帳）和一節早就過時的（`place_of` 已經不在了）。

**測試**：`shell` 新增 `documents::a_child_frames_script_goes_on_once_its_blocking_sheet_is_gone`（子 frame 的 parser 被一個 5 秒的樣式表擋住，父 frame 拿掉它的 `<link>`，script 馬上跑）。`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`shell`（744 個）、`browser` 的測試和 sanity 通過；WPT 同 3c 那一組：只有之前看過、原本就有的。

**mutation test**（`mutants-3d.toml`，10 個，timeout 240 秒）：第一輪 12 個裡 6 個抓到；「只重試最上層的 frame 被樣式表擋住的 script」補了上面的測試之後抓到；「不照建立的順序走」（只差子 frame 之間重試和偵測的先後）和「清理時要 main frame」（iframe 加進、拿掉本身就讓 DOM 變了）是等價的，拿掉排序和那一行，不在清單裡。照舊沒抓到的三個：「`doc` 不先問 engine」（現在沒有地方在 frame 拿掉之後、task 結束之前讀它）和「不放掉不見的 frame 的資料」（只是記憶體）記進 `docs/test-gaps.md`；「frame 增減時不讓 DevTools 重新比對」是 test-gaps 裡原本就有的那一條。

### 3c：只在 task 之後跟上 frame，之間讀的是 engine 的 frame（2026-11-26，Linux：Fedora 45，debug build，3b 之上，未 commit）

**做了什麼**：`handle_frame_requests` 的 12 處呼叫拿掉 11 處，只留 `after_task` 的（跟上 engine 建了、丟了的 frame，和沒有 load sink 的 engine 的 blob URL）。拿掉的：`SetHistoryOffsetAndLength`（計數在送出時就算了，第 31 條）、輸入的 ack（導覽的畫面在 engine 的呼叫裡要了）、子資源的回應、traversal、`parser_stopped`（`start_loads` 只叫 engine 的 `send_sheet_fetches`）、`parse_document`、deferred script、送 script 和圖片的請求、`finish_load`、script 的回應：都是為了先記好導覽的帳，或在自己的 task 裡跑、前一個 task 的 `after_task` 已經跟上了。盤點列的過期的讀法改成讀 engine 的 frame：`finish_loads` 照 `frames_in_tree_order()`，`finish_load`、`frame_render_blocked` 和給 intersection 的被擋住渲染的子 frame 的文件讀 `engine_document`（子 frame 的：iframe 的 content document）；traversal 的那一處 3b 已經拿掉。`RenderFrames::ids` 只剩測試用，搬進測試；`child_documents` 改成私有。

**測試**：`shell` 新增 `documents::a_render_blocked_child_frame_has_no_intersections`（子 frame 的樣式表晚 400 ms 到，它的 observer 的第一個 entry 在那之後）。`renderer`、`shell`（743 個）、`browser` 的測試通過；WPT 的 `html/browsers/`、iframe、`the-script-element/`、`html/syntax/parsing/`、`css/cssom/`、`intersection-observer/`、`resize-observer/`、`navigation-timing/`：之前的 unexpected pass 之外，多兩個：`module/credentials.sub.html`、`script-crossorigin-network.sub.html`，`git stash` 回到 3a 之前的程式照樣是：原本就有的，這台機器上。

**mutation test**（`mutants-3c.toml`，5 個，timeout 240 秒）：3 個抓到；「子 frame 擋住渲染時不擋它的 intersection」補了上面的測試之後抓到；「載完時父 frame 先於子 frame」是等價的：父 frame 的 `check_completed` 等子 frame 的文件真的「載完」（它的 window 的 `load` task 跑完），同一輪 `finish_loads` 只是替它們排 task，先後不改結果。

### 3b：iframe 不送給 Browser 的導覽和 traversal 的捲動在 engine 那一側（2026-11-26，Linux：Fedora 45，debug build，3a 之上，未 commit）

**做了什麼**：`dom_api::child_navigation_here`（iframe 的導覽不送給 Browser 的種類：同一份文件的 fragment、`javascript:`）由 `dom_js` 判斷一次，`ChildNavigationAsked::here` 告訴 client 不送（client 不再自己判斷）。fragment 的由 `dom_api::navigate_child_to_fragment` 當場做（網址、經過 `FrameLayout` 捲；要畫面的是 iframe 的 attribute 的改變），別的 origin、載完的文件的 iframe 晚一點的 `load` 是 DOM 的 native timer（`NativeAction::OwnerLoad`，文件上記最後一次的編號：一串只發最後一次）。`javascript:` 的記在 `dom_js` 的 frame 上和送出的導覽同一格（`frames::Pending`），叫 `FrameClient::commit_here`（第 33 條），client post `InternalTask::ChildCommit(token)`，task 裡 `ScriptEngine::take_commit_here`。engine 拿走 frame 上的導覽時自己清掉「父 frame 的 `load` 要等它」（`take_pending`），renderer 的 `sync_child_navigating` 拿掉。traversal 的捲動和要畫面搬進 `traverse_within_document`（`popstate` 之後，照那時的網址）。拿掉 `FrameRequest::Navigate`（`FrameRequest` 只剩 blob URL）、3a 暫時的 `replaced`、`RenderFrames` 的 commit 佇列和晚一點的 `load`、`InternalTask::OwnerLoads`、`post_internal_after`、renderer 的 `scroll_to_fragment`、`navigate_to_fragment`、`child_navigation_goes_nowhere`、`same_but_fragment`；test-gaps 裡講 event loop 為晚一點的 `load` 醒來的那一列（那段程式早就不在了，native timer 照一般的 timer 叫醒 loop）。

**測試**：`shell` 新增 `documents::an_iframes_fragment_navigation_scrolls_its_frame`、`documents::a_parents_load_waits_for_an_iframes_javascript_url`、`browser_view::a_traversal_within_the_document_is_drawn`；3a 的 `a_navigation_to_a_fragment_is_drawn` 和它都改成按鍵推進（每一步是按鍵排的 timer，前一步畫出來才按：原本 200 ms 一步的 timer 在機器忙時可能錯過中間的那一格）。iframe 晚一點的 `load` 由 `browser` 的 `navigation::a_cross_origin_frames_fragment_navigation_fires_its_iframes_load` 守著。`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`shell`（742 個）、`browser` 的測試和 sanity 通過。WPT 的導覽那幾個目錄：3a 的 3 個 unexpected pass 照舊；`navigation-timing/test-timing-attributes-dependent-on-document-load.html` 偶爾通過：`redirectStart` 是第一個 redirect 的請求開始減掉 time origin、小於 0 算 0，這台機器上兩者幾乎同時，大約十幾次大於 0 一次；`git stash` 回到 3a 之前的程式各跑 10 次，兩邊都是 0 次通過，看不出差別：在 `.ini` 標成 known intermittent，附上原因。

**mutation test**（`mutants-3b.toml`，23 個，timeout 240 秒）：第一輪 24 個裡 19 個抓到。「iframe 的 fragment 不捲」補了 `an_iframes_fragment_navigation_scrolls_its_frame`、「traversal 不要畫面」補了 `a_traversal_within_the_document_is_drawn`、「`javascript:` 不擋父 frame 的 `load`」補了 `a_parents_load_waits_for_an_iframes_javascript_url` 之後抓到；「iframe 的 fragment 不要畫面」是等價的（設 `src` 本身就讓 DOM 變了、要畫面），拿掉那一行，不在清單裡；「Browser 開始的 commit 取代排著的 `javascript:`」照舊沒抓到，記進 `docs/test-gaps.md`。重跑漏抓的那幾個之前有一次基準沒通過，失敗的測試名字被我篩輸出時濾掉了；同一條指令接著跑 5 次都通過，沒有重現，原因沒找到（可能是上面改成按鍵推進的兩個視窗測試，不確定），之後再碰到時看完整的 log。

### 3a：導覽的帳在 engine 的 frame 上（2026-11-26，Linux：Fedora 45，debug build，`fb1c1d3e` 之上，未 commit）

**做了什麼**：`dom_api::NavigationRequest`（`begin_id`、iframe 的 `no-referrer`、到 `about:blank` 時發起的文件）和 `ScriptEngine::navigation`、`take_navigation`；`dom_js` 在 hook 送出的那次呼叫裡記在 `Frame::navigation`（`Frames::set` 換文件時帶著），`dom_wasm` 記在它唯一的 frame 上。renderer 在 Browser 的 `CommitNavigation`、`NavigationDropped`、DevTools 的 frame 清單裡跟 engine 拿；`RenderFrames` 只剩文件、`srcdoc` 和 `javascript:` 的 commit 佇列、晚一點的 `load`。取消窗口照第 30 條（`InternalTask::NavigationCancellationWindowEnded`，client 送出時 post），同文件的歷史計數照第 31 條（`OwnHistoryEntries` 是共用的 `Rc<Cell<u64>>`），同文件的導覽在 `history::navigate_from_requests` 裡當場 `schedule_frame`。拿掉 `FrameRequest::Navigation`、`frame_navigated`、`navigate_child`、`child_navigation_sent`、`about_blank_origin_source`、`child_no_referrer`、`navigations_asked`、`end_cancellation_windows`、第 1 段的 debug 檢查（`Asked`、booking）和為它留的 `_now` 包裝。`dom_js` 的測試改用記錄導覽的 client（`tests/it/navigations.rs`）。

**查到的**：(1) 子 frame 的 `location` 導覽到同一份文件的 fragment、或到 `javascript:` 時，engine 的 `history::navigate` 自己就做了（同文件的導覽、文件的 `javascript:` task），renderer 原本的「沒有送出的子 frame 導覽」那一支走不到：不搬，拿掉。(2) 第一版先 `javascript:` 再一般的 `src` 時 `javascript:` 贏了：一般的在呼叫裡就記好了，`javascript:` 的在呼叫之後才排、拿走了它（「送和帳分開」本身）。3b 之前先照同一批 `FrameRequest` 的順序：後面有送出的導覽的 `javascript:` 不排（`replaced`）；3b 讓它們在同一格裡，這段拿掉。(3) 送出導覽時設「frame 變了」（DevTools 的 frame 清單）是多的：`navigating` 只用在新建的 frame 的初始文件，新建的那次呼叫已經設了；拿掉。`sync_child_navigating` 和 commit 時的 `navigating` 讀到的導覽一定已經拿走了，改成不讀。

**測試**：`dom_js` 新增 `frames::the_navigations_sent_are_booked_on_their_frames`；`shell` 新增 `browser_view::a_navigation_to_a_fragment_is_drawn`（再到一次同一個 fragment：`:target` 和 DOM 都沒變，捲回去要畫面；`location.hash` 的 setter 對同一個 fragment 什麼都不做，用 `location.assign`）和 `documents::an_iframes_latest_navigation_commits`（`javascript:` 和一般的 `src` 兩種先後）。`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`shell`（739 個）、`browser` 的測試和 sanity 通過；WPT 的 `html/browsers/history/`、`browsing-the-web/`、iframe、`navigation-timing/` 有 3 個 unexpected pass（`history_pushstate_url_rewriting.html`、`cross-document-traversal-cross-document-nav.html` 的 slow 那一個、`nav2-test-timing-persistent.html` 的 Part 1），`git stash` 回到 `fb1c1d3e` 的程式照樣是：原本就有的，這台機器上，預期結果沒改。

**mutation test**（`mutants-3a.toml`，26 個，timeout 240 秒：改 `dom_js` 的要重編它、重新 link 後面每個測試 binary，約 140 秒）：24 個抓到。「換文件時帶著導覽」補了 `the_navigations_sent_are_booked_on_their_frames` 之後抓到；「同文件的導覽不要畫面」補了 `a_navigation_to_a_fragment_is_drawn` 之後抓到；`dom_wasm` 的「不記」原本的 mutant 編不過，改寫之後抓到。沒抓到的兩個都是 3b 拿掉的暫時的佇列：「拿走導覽時不丟排好的 commit」（要 Browser 的回應比 commit 的 internal task 先跑）和「排好的 commit 蓋過之後送出的導覽」（要兩次呼叫之間剛好沒跑到 commit 的 task），時序測不穩；3b 之後導覽和 `javascript:` 的 commit 在 frame 的同一格，結構上沒有這兩種。拿掉的五個（上面「查到的」(1)、(3)）不在清單裡。基準前兩次沒跑成：`git stash` 之後 `dom_js` 的增量輸出舊了，`shell` link 失敗（`undefined hidden symbol`，`cargo clean -p dom_js -p shell -p browser` 修好），之後基準要重編超過 150 秒。

### 2c 做完時（2026-11-26，Linux：Fedora 45，2c-4 之上，未 commit）

- `cargo fmt`（2c 改過的 5 個檔案）、`cargo clippy --workspace --all-targets -- -D warnings`（修了兩個：`frame_view` 的測試多一個 `mut`，`parse_document` 的參數改用 `dom_api::document_loader::ParseEntry`）：通過。
- `cargo test --workspace`：4893 個通過，0 個失敗。
- sanity 的 deps、spans、boundary、unsafe、tests、numerics、docs：通過。

### 2c-4：樣式表和 script 的回應（2026-11-26，Linux：Fedora 45，debug build，2c-3 之上，未 commit）

**做了什麼**：新的 `dom_api::subresource`：回應的型別（`SubresourceResponse`、`SubresourceResult`，和傳輸無關：status、MIME 的 essence 和 charset、URL、是不是 opaque、body；失敗的帶它的名字）和每種的解碼（`script_loaded`、`wasm_loaded`、`module_loaded`、`stylesheet_text`、`is_blocked_as_script`、`failed_url`），原本是 renderer 的；renderer 留 `loader::subresource`（`LoadResult` 轉過去），worker 的 module 和 DevTools 重抓原文也經過它，解碼只有一份。`document_loader::script_fetched`（module 的交給 module map、之後每個 frame 的 deferred script；classic 和 wasm 的解碼、交給 pending script，parser 的接著 `parse_unless_due`、deferred script；失敗的記在 DOM）、`sheet_fetched`（拿掉帳、解碼、parse 成 CSS、設 sheet、`load` 或 `error`、擋著的 parser 接著 parse 和 deferred script、`finish_load`、要畫面；DevTools 要時交回原文）。renderer 照請求的編號找出 frame 和文件的編碼交下去（回應屬於哪個 frame 的路由，`FrameDocument::encoding` 也給 DevTools 重抓用），交回的原文給 `CssAgent`；拿掉 `sheet_loaded`、`LoadedSheet`、`script_loaded`、`wasm_loaded`、`stylesheet_text`。

**測試**：`shell` 新增 `documents::a_sheet_that_failed_from_its_own_origin_is_readable`（同源的網路錯誤：body 截斷的 deflate）。`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`shell`、`browser` 的測試通過；WPT（3091 個，加上 `encoding/`）0 個退步；`test.py scenarios`（98 份）和 `test.py reftest tests/reftests` 通過。

**mutation test**（`mutants-2c4.toml`，23 個，timeout 150 秒）：21 個抓到；「同源的樣式表載入失敗時規則讀不到」補了上面的測試之後抓到；「樣式表的回應之後不要畫面」照舊沒抓到，記進 `docs/test-gaps.md` 和 2c-2 的兩個同一列（測試裡總有別的要了畫面）。

### 2c-3：「接著 parse」是 DOM 的 task（2026-11-26，Linux：Fedora 45，debug build，2c-2 之上，未 commit）

**做了什麼**：DOM 多一種 element task `ElementTaskKind::ParseOn`（以文件為目標，`Cx::queue_parse_on` 回傳它的編號；`push_task`、`push_element_task` 改成回傳編號）。parser 讓出時 `dom_api::document_loader::parse` 自己排它，`DocumentLoader` 記讓出的次數和排著的 task（`yields`、`parse_task`，原本是 renderer 的 `FrameDocument::yields`、`parse_due`），每一輪開始時清掉排著的那一個（它來時發現已經 parse 過）；`ParseRun::budget` 改成照讓出的次數算（renderer 的 `parse_run` 給 `ParserBudget::of_run`）。`dom_api` 的 `document_loader::run_dom_task` 照編號認出 parser 的 task，拿出來、parse（span `HTMLDocumentParser::PumpTokenizer` 跟著搬過來，Speedometer 的工具讀它），別的交給 `engine.run_dom_task`；`parse_unless_due` 是 body 和 parser 的 script 來時用的（排著 task 時不 parse）；`dom_api` 的 `run_task` 遇到 `ParseOn`（舊的）什麼都不做。renderer 拿掉 `InternalTask::ParseOn`、`parse_on`，`advance_document` 和 `read_body`、script 的回應經過 `parse_document`，`run_dom_task` 經過 `dom_api` 的。

和原本不同的：「接著 parse」從 renderer 的 internal queue 移到 DOM 的 element task 的 queue（第 14 條說過的先後可能變）；WPT、`shell`、`browser` 的測試沒看到差別。

**測試**：`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`shell`、`browser` 的測試通過；WPT（3091 個）0 個退步（非預期的通過是之前的 5 個）；`test.py scenarios`（98 份）通過；span 換了地方，照規則跑了量測工具的測試：`speedometer_test.py`、`blink_perf_test.py` 通過。

**mutation test**（`mutants-2c3.toml`，8 個，timeout 150 秒）：4 個抓到（沒排 task、parser 讓出的次數不算、task 沒拿出來、parser 的 task 交給 engine）。沒抓到的：排著 task 時資料來了照樣 parse、一輪開始時不清排著的 task（`docs/test-gaps.md` 第 156 列本來就有，位置改成 `parse_unless_due`，同一類寫在一起）；預算不隨讓出的次數變大（記進「只靠審查」：只差時間）；`ParseOn` 認成 `Fire`（記進「等價、不用處理的 mutation」：只有 parser 提早 parse 過的舊 task 走得到，觸發名字是空字串的事件）。

### 2c-2（2026-11-26，Linux：Fedora 45，debug build，2c-1 之上，未 commit）

**做了什麼**：HTML 的「the end」在 engine 那一側：`dom_api::document_loader` 的 `parsed`（先送出最後那段輸入帶出的樣式表的請求，再 `Interactive`、deferred script；body 中途斷掉的走 `aborted`：放掉 deferred 的 pending script、結束載入、`Interactive`、`Aborted`，子 frame 的話檢查父的載入，要一張畫面）、`run_deferred_scripts`（自己看還在 deferring 沒）、`content_loaded`（`DOMContentLoaded`，經過 `FrameLayout` 捲到 fragment、要畫面，經過 `ScriptEngine::tell_lifecycle` 告訴 frame client，`finish_load`）、`finish_load`（送樣式表的請求、`check_completed`，只等圖片時送圖片的再查一次）；`parse` 在 parser 結束時接著跑 `parsed`。window 的 `load` 的 task（`dom_api` 的 `text_control.rs`）跑完當場經過 `ScriptHost::tell_lifecycle` 告訴、要畫面；DOM 的 `note_document_loaded`、`take_loaded_documents` 拿掉。新的 hook `FrameClient::document_lifecycle`（第 22 條：frame 的 token、是不是最上層、文件和它的 loader，renderer 的實作照 `main_frame_info`、`child_info` 的規則算出 DevTools 的名字，送 `domContentEventFired`、`loadEventFired`、`lifecycleEvent`、`frameStoppedLoading`），兩個轉手（`ScriptEngine::tell_lifecycle`、`ScriptHost::tell_lifecycle`，先送標題）；`dom_js` 的 engine 和 task 的 `Host` 經過 `frames::tell_lifecycle`，`dom_wasm` 只有最上層。renderer 拿掉 `document_parsed`、`document_aborted`、`finish_loaded_documents`、`document_loaded`、DOMContentLoaded 那一段、`is_top`、`place_of`、`load_progress`、`FrameDocument::aborted`（body 結束時交給 loader：`DocumentLoader::body_ended(aborted)`）、`load_finished`（換成 `load_told`：畫面那一步看 DOM 的 `LoadProgress::Loaded`，送一次 `DidFinishLoad`）；idleness 看 DOM 的 `Cx::content_loaded`（新的）才開始；`finish_load` 叫 `dom_api` 的。

和原本不同的：DevTools 的 lifecycle 不再先 `PageAgent::update(frames)`（盤點：frame 的增減現有的 hook 已經當場告訴，lifecycle 只要 id、文件、loader）；DOMContentLoaded 和 `load` 的 listener 要的導覽、iframe 的載入在呼叫之後才處理（`handle_frame_requests`），testharness 的結果在 DevTools 的事件之後送；wasm 文件的 `load` 的 task 裡不先送標題（wasm 的 `Host` 拿不到 engine 的「上次送的標題」），task 之後照舊送。

**找到的**：第一次跑 `shell` 時 `network::stylesheets_block_the_parsers_scripts` 失敗：最後一個 `<link>`（parse 的最後一輪才做出來）的請求沒在 deferred script 之前送出（原本 renderer 在 parse 完先 `start_loads`），`parsed` 補上。mutation test 第一次的 baseline 連結失敗（`undefined hidden symbol`，incremental 的產物壞了：`cargo clean -p dom_js -p browser` 之後好了），第二次 baseline 在 150 秒的時限內編不完、沒說原因就停了（重編之後第三次跑完）。

**測試**：`shell` 新增 `devtools_lifecycle::a_child_frames_lifecycle_so_far_is_told_as_it_is_turned_on`、`documents::a_sheet_asked_for_at_dom_content_loaded_delays_the_load`，`devtools_lifecycle::a_navigation_is_told_through_its_lifecycle` 加上 `domContentEventFired`、`loadEventFired` 只有主 frame 的一次。`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`shell`、`browser` 的測試通過；WPT（3091 個）0 個退步，非預期的通過裡的 `search` 是 2a-3 查過的 `location-setter.html`；`test.py scenarios`（98 份）通過。

**mutation test**（`mutants-2c2.toml`，29 個，timeout 150 秒）：19 個抓到、3 個到時限被砍（沒有 DOMContentLoaded、DOMContentLoaded 之後不 `finish_load`、`load` 之後不要畫面），7 個沒抓到；補了上面的測試之後重跑，抓到「`finish_load` 不送樣式表的請求」「每個 frame 都當成最上層」「子 frame 的文件用主 frame 的名字」。照舊沒抓到的：中止時不設「載入結束」（`docs/test-gaps.md` 的「只靠審查」本來就有，位置改成 `aborted`）；DOMContentLoaded 之後和中止時要畫面（記進同一節：測試裡總有別的要了畫面）；只等圖片時送圖片的請求（記進「等價、不用處理的 mutation」：只差請求晚一個 task 出去）。

### 2c-1（2026-11-26，Linux：Fedora 45，debug build，`ab6695a9` 之上，未 commit）

**做了什麼**：「要畫面」的 `needs_begin_main_frame`、`main_frame_requests` 從 `RenderThread` 搬到 `FrameWidget`（`schedule_frame`；widget 在第一份文件的 frame 之前是 render thread 的，之後是 `Frame` 的，`RenderThread::widget` 照舊找它），`FrameLayout::schedule_frame`（`Frame` 的實作設 widget 的；2c-2 才有人叫）。DOM 新的 `Cx::render_blocked`（`LoadProgress::Loading` 而且沒有 body，或 `Cx::render_blocking_sheet_loading`）：子 frame 的 view 在 lifecycle 裡問，最上層由 `RenderThread::render_blocked` 讀；拿掉 `update_child_blocking`（五個呼叫點）、`FrameView::render_blocked` 和 `set_render_blocked`、`FrameDocument` 的 `parsing`、`body_parsed` 和 `note_body_parsed`（body 剛做出來時的畫面由樹變了之後的 `request_next_frame_if_dom_changed` 要）。DOM 的 `failed_sheets` 改成通用的 `Cx::note_load_failed`、`take_failed_loads`（`window.rs`），renderer 的 `note_failed` 記到 DOM，`FrameDocument::failed` 拿掉，`DidFinishLoad` 只從 DOM 拿。

和原本不同的一點：`body_parsed` 原本一旦是真就一直是真，`render_blocked` 看的是現在有沒有 body（script 在載入中拿掉 body，文件又算 render-blocked 到 parse 完）；Blink 的 `ShouldScheduleLayout` 也看現在的 `body()`。

**測試**：`frame_view` 的 `a_child_frame_whose_rendering_is_blocked_waits` 改成給子 frame 的文件記一份擋 render 的樣式表的請求（`Cx::sheet_sent`、`take_sent_sheet`），不再直接設旗標；`shell` 的 `network::a_document_is_drawn_before_its_body_has_all_come` 加上第一張畫面只有暫停之前的那一半（下面的 mutant）。`dom`、`dom_api`、`renderer`、`shell`、`browser` 的測試通過；WPT（2a-3 的那些加上 `dom/nodes/`，3015 個）0 個退步，4 個非預期的通過是之前記過的；`test.py scenarios`（98 份）通過。

**mutation test**（`mutants-2c1.toml`，12 個，timeout 150 秒）：11 個抓到；「有 body 了還在 parse 的文件也算 render-blocked」沒抓到（原本的測試只看畫了兩張以上、最後一張對），加強上面那個測試之後重跑抓到。`FrameLayout::schedule_frame` 還沒有人叫，它的 mutant 在 2c-2。

### 2a 做完時（2026-11-25，Linux：Fedora 45，2a-3 之上，未 commit）

- `cargo fmt`（只動到 2a 改過的 8 個檔案）、`cargo clippy --workspace --all-targets -- -D warnings`：通過。
- `cargo test --workspace`：4890 個通過，0 個失敗。
- Speedometer 3.1（使用者要求，`bench.py speedometer --against 66110866`，各 3 次輪流，10 iterations，AC 電源，renderer 是自己的 process）：20 個 suite 的幾何平均 1.002（+0.2%），每個 suite 的中位數都在兩邊的 95% 區間裡（最大 TodoMVC-Backbone -2.7%、TodoMVC-React-Complex-DOM +2.4%）；記憶體峰值 -1.2% 到 +3.9%（Perf-Dashboard：Browser 的 process 151 → 159 MB、renderer 270 → 278 MB）。

### 2a-3（2026-11-25，Linux：Fedora 45，debug build，2a-2 之上，未 commit）

**做了什麼**：parser 的外部 script（擋住 parser 的、`defer` 的、wasm 文件的）是 DOM 的 pending script，標成 parser 的（`Cx::queue_parser_script`）：請求和回應走 DOM 排的 script 那條路（`send_script_fetches`、`script_fetched`），抓好之後不排 task，`script_fetched` 回答是不是 parser 的；wasm 的回應是 `ScriptResult::Wasm`（`wasm_script_fetched`，`ScriptFetch`、`SentScript` 多 `wasm`）。`DocumentLoader` 拿著擋住 parser 的那一個和 deferred 的清單，`dom_api::document_loader::parse` 是原本 `advance_document` 的迴圈：每一輪先送出樣式表的請求、等或跑擋住 parser 的 script（inline 的等樣式表，外部的等 DOM 說抓好了；classic 的經過 `run_ready_script`），再跑 parser、prepare 它停下的 script；回傳 `ParseOutcome`（沒東西 parse、等 script、等 body、讓出、parse 完）。`run_deferred_scripts`、`abort_scripts`。body 到齊由 renderer 告訴（`DocumentLoader::body_ended`，原本看 `FrameDocument::body` 還在不在）。renderer 拿掉 `blocking_script`、`deferred_scripts`、`ParserScript`、`ParserSource` 和 `fetch_parser_script`、`start_parser_module`、`script_ready`、`execute_parser_script`、`prepare_script`、`run_script`、`fetch_script`、`initiator`；`advance_document` 只照回傳的結果排 `ParseOn`、等、或做「the end」；`yields`、`parse_due`、`body_parsed`（畫面那一側的）、`aborted` 和 DOMContentLoaded 之後的事照舊在 renderer（2c）。迴圈裡原本在每個 script 之前送的標題，照已決定的事項第 19 條改成 `FrameClient::title_changed`，帳搬到 `DocumentLoader`。

拿掉的：迴圈裡的 `start_loads`、`handle_frame_requests`、`send_harness_results` 改成呼叫之後各一次（同一個 task 裡，順序對頁面看不出差別）；外部 classic script 移到別的文件的檢查（`run_script_task` 本來就比 prepare 時的文件）；deferred 的迴圈裡的 deferring 檢查（renderer 呼叫前後查，呼叫裡沒有東西改它）；inline script 跑完之後還原「之前的」`currentScript`（parser 的 script 跑時沒有別的在跑，之前的一定是空的，改成還原成空的）。

**停損點第 3 項**：parse 的迴圈裡讀 renderer 的帳的地方（`advance_document` 的開頭、迴圈裡的 `note_body_parsed`）不在迴圈裡了：`note_body_parsed` 在呼叫之後讀一次 DOM。

**測試**：`shell` 新增 5 個（`documents::a_parsers_script_moved_away_before_it_is_prepared_doesnt_run`、`a_parsers_script_moved_away_while_it_waits_doesnt_run`、`a_parsers_external_script_runs_after_the_microtasks_before_it`、`an_inline_module_scripts_lines_are_its_documents`、`a_parsers_inline_scripts_microtasks_run_before_the_parser_goes_on`）。`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`shell`、`browser` 的測試通過；`test.py scenarios`（98 份）和 `test.py reftest tests/reftests` 通過。WPT（2a-2 的 1907 個加上 `html/semantics/scripting-1/`、`html/syntax/`、`html/browsers/`、`the-iframe-element/`、`html/webappapis/`、`dom/nodes/`、`encoding/`，3091 個）：0 個退步。非預期的通過多了兩個，都是原本就有的（`git stash` 回到 2a 之前各跑 6 次）：`replace-before-load/location-setter.html` 的 `href`（之前和之後都是時過時不過，一次連 `search` 也過）、`html/webappapis/timers/negative-settimeout.any.worker.html`（`.ini` 預期 `ERROR`，之前和之後都是 `OK`）。sanity 的 deps、spans、boundary、unsafe、tests、numerics、docs 通過。

**不穩定的測試**：`shell` 的 `automation_pipe::a_client_not_reading_doesnt_hold_machopo_up` 在整套 `cargo test -p dom -p dom_api -p renderer -p shell` 裡失敗過一次（`took < 5s` 那一行：machopo 結束得比 5 秒慢）；它不載入頁面，和這段的改動無關；單獨跑 3 次、整套 `shell` 再跑 2 次都通過。原因還沒查，和 0115 開頭記的 `webdriver::a_session_with_a_hung_page_ends_in_time` 一起之後再查；這段的 mutation test 跳過它。

**mutation test**（`mutants-2a3.toml`，36 個，timeout 120 秒）：第一輪（33 個，指令裡沒有 `browser` 的測試）沒抓到 14 個，用 WPT（`scripting-1/`、`html/syntax/`、`html/dom/`、`dom/nodes/`、`document-metadata/`、`html/webappapis/scripting/`、`browsing-the-web/`，只算非預期的失敗）再跑也都沒抓到；處理之後：
- parser 的預算和讓出的測試在 `browser` 的 `navigation::`（`a_parser_lets_timers_run_between_its_slices` 等），指令加上它：「body 之前就讓出」「沒給一片就讓出」「body 永遠算沒做出來」抓到；加上之後 `navigation::the_tab_title_follows_document_title` 失敗，找到上面的標題（第 19 條）。
- 等價、程式拿掉的 3 個（上面「拿掉的」）。
- 補 `shell` 的測試抓到的：prepare 之前被移走的、inline 的等著時被移走的、inline module 的行號、inline script 之後的 checkpoint。
- 照舊只靠審查的（`docs/test-gaps.md` 的「只靠審查」，位置改成 `dom_api` 的）：有輸入在等時讓出、body 裡的 script 之後縮短預算、wasm 的 script 的 `load`。
- 等價的（記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）：外部 classic script 之前的 checkpoint、中止時放掉 deferred 的 pending script。
最後一輪：36 個裡抓到 28 個、到時限被砍 1 個（「沒告訴 loader body 到齊了」）、上面 5 個照舊沒抓到（3 個只靠審查、2 個等價），1 個 mutant 寫錯編不過（標題送空的，改過之後重跑抓到）；inline script 之後的 checkpoint 補了測試之後重跑抓到。

### 2a-2（2026-11-25，Linux：Fedora 45，debug build，2a-1 之上，未 commit）

**做了什麼**：照已決定的事項第 18 條，`<link>` 的樣式表的請求和帳搬到 engine 那一側。`dom` 新的 `link_sheet.rs`：每個 `<link>` 在它的文件裡要一次（元素的旗標，借 script 的 already started 那個 bit，adopt 時清掉；原本是 renderer 每份文件一個 `requested_links`），送出的請求照編號記成 `SentSheet`（擋 render、擋 parser 的 script，文件的 `load` 等它；`<link>` 放在文件的 `pending_sheet_links` 裡留到回應來，原本是 renderer 的 `NodeRoot`），`sheets_block_scripts`（`<link>` 拿出文件、進 shadow tree、到別的文件、不再是 stylesheet 之後不擋；原本在 renderer 的 `script_ready`），`href` 解析不了的記在文件上（`take_failed_sheets`，和 `DidFinishLoad` 一起送，排在 renderer 自己記的失敗之後）。`dom_api` 新的 `link_style.rs`：`send_sheet_fetches`（原本 renderer 的 `start_loads_of`），media 經過新的 `FrameLayout::media_environment`，請求經過新的 `LoadSink::start_sheet_fetch`（DevTools 的 initiator 和 cache 的 flag 照圖片、script 那條路）；`ScriptEngine::send_sheet_fetches`，兩個 engine 都實作。renderer 拿掉 `pending_sheets`、`requested_links`、`LinkToLoad` 和掃描；`start_loads` 叫 engine 送、再告訴每個子 frame 的 view 擋不擋；render-blocking、擋 script、在路上的請求數、DevTools 的資源種類和失敗的網址改成問 DOM；回應照舊由 renderer 收、解 CSS（`LoadedSheet`）。parse 的迴圈照舊在 renderer，每一輪開頭的 `start_loads` 讓 parser 停下時先送出樣式表的請求。

**測試**：`dom` 的 `link_sheets`（5 個：送出到回應之間擋什麼、`load` 等著；`<link>` 拿走之後不擋 script；每份文件要一次、adopt 之後再要；GC 之後 `<link>` 還在；失敗的 `href` 拿一次）；`dom_api` 的 `link_style`（3 個：每種 `<link>` 擋什麼、CORS、請求的 origin、要一次；media 照 layout 的環境，沒有 layout 時算符合；沒送出去的記成失敗）；`shell` 新增 3 個（下面）。`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`shell` 的測試通過。WPT：`include.txt` 裡用到 `<link rel=stylesheet>` 的 1907 個，加上 `the-script-element/`、`html/syntax/`、`html/browsers/`，共 2512 個：0 個退步，4 個非預期的通過是之前記過的。`test.py reftest tests/reftests/link`、情境 `cursor`、`iframe-loads`、`position-absolute`、`demo-script` 通過。sanity 的 deps、spans、boundary、unsafe、tests、numerics、docs 通過。

**mutation test**（`mutants-2a2.toml`，30 個，timeout 120 秒）：第一輪 27 個抓到、3 個沒抓到，補了測試之後重跑那 3 個都抓到：
- `start_loads` 不告訴子 frame 的 view 擋不擋：parser 的 `<link>` 在 parse 結束時另外告訴了，只有載入之後 script 才要的樣式表看得出來；補 `shell` 的 `network::a_child_frames_sheet_asked_for_later_blocks_its_rendering`（載入期間子 frame 停在上次的畫面）。這條路在搬之前一樣沒有測試。
- 在路上的請求數不算樣式表：補 `devtools_lifecycle::a_sheet_on_its_way_keeps_the_network_busy`（`media=print` 的樣式表不擋 `DOMContentLoaded`，`networkIdle` 等它）。
- 樣式表的請求的網址查不到：只有失敗的回應用得到（log 和 DevTools 的資源樹），補 `devtools_log::a_sheet_that_fails_is_logged_with_its_url`。

### 2a-1（2026-11-25，Linux：Fedora 45，debug build，`66110866` 之上）

**做了什麼**：`renderer` 的 `encoding.rs` 整個搬到 `dom_api::encoding`（文件、script、樣式表的解碼，Blink 的 `TextResourceDecoder`；只依賴 `encoding_rs`）。新的 `dom_api::DocumentLoader`（`document_loader.rs`）：解碼器、`MarkupFilter`（從 renderer 的 `document.rs` 搬來）、parser 的輸入（`ParserInput`，從 `document_body.rs` 搬來）、parser、原始碼；`ScriptEngine` 多 `document_loader`、`set_document_loader`，`dom_js` 放在每個 `Frame` 上（換文件時 `Frames::set` 整個換掉，它跟著走），`dom_wasm` 放在最上層；`resume_parser`、`finish_parser` 收 `Box<dyn ScriptEngine>` 借給 parser（第 17 條）。renderer 的 `DocumentBody` 只剩 pipe、載入的狀態、DevTools 的 bytes；commit 時在 `create_window` 之後建 `DocumentLoader`（編碼從它來），pipe 讀到的 bytes 交給它；`FrameDocument` 拿掉 parser 和輸入，多一個 `parsing`（render-blocking 用）；DevTools 要原始碼時問 engine（`document_source`）。parse 的迴圈這一段照舊在 renderer。

叫做 `DocumentLoader`，不是已決定的事項第 12 條的 `LocalFrame`：2a 這一段只是文件的載入（Chromium 的名字），`LocalFrame` 之後有沒有、裝什麼，到 2b、2c 再看。

**找到的 bug**（這一段自己的）：沒有 pipe 的 body，開頭那一份已經當成最後一份解碼，之後讀 pipe 回報「結束」又解碼一次最後一份，`encoding_rs` panic；`BodyRead` 分成「這一刻才結束」（`Over`）和「早就結束」（`Ended`）。

**測試**：`MarkupFilter` 和 `ParserInput` 的單元測試跟著搬到 `dom_api`。`dom_api`、`renderer`、`shell` 的測試通過。WPT（`html/browsers/`、`html/syntax/`、`the-iframe-element/`、`the-script-element/`、`html/semantics/document-metadata/`、`encoding/`，743 個）：0 個退步；4 個非預期的通過是 1b、1c 記過的。

**mutation test**（`mutants-2a1.toml`，10 個，都抓到，3 個是到時限被砍的「hung」）：原始碼沒留、markup 沒進輸入、parser 拿不到輸入、「還有輸入」永遠說沒有、`srcdoc` 當一般的、parser 跑完沒放回去、parser 沒結束、DevTools 只在 body 全到之後讀、早就結束的 pipe 又結束一次、commit 時不算在 parse。第一輪的 timeout 是 900 秒：讓頁面一直等不到的 mutant，`shell` 的七百多個測試各等滿 30 秒才失敗，一個要四百多秒；停掉、`--restore`，剩下的改成 120 秒重跑（正確的程式跑一次約 30 到 50 秒，留兩倍多，免得沒抓到的被砍掉、誤記成抓到）。

### 第 1 段結束（2026-11-25，Linux：Fedora 45）

`cargo fmt`（改了這一段的四個檔案的排版）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（60 個 binary，4874 個通過，1 個 ignored）、`sanity.py` 的每一項通過。

### 1c（2026-11-25，Linux：Fedora 45，debug build，`de04cac5` 之上）

**做了什麼**：renderer 的 hook（`RendererFrameClient`）每個 `FrameClient` 方法先設一個旗標（`render_frame::Asked`，`Rc<Cell<bool>>`，只在 debug build），表的導覽的讀法（`navigation`、`take_navigation`、`take_any_navigation`，`is_navigating` 經過 `navigation`）斷言它沒設，`handle_frame_requests` 對帳的期間例外（深度，可以巢狀），每一輪拿走 `FrameRequest` 之前清掉。release build 沒有旗標。

**和已決定的事項不同的一處**（README 的第 6、9 條）：第 6 條原本是 `ScriptEngine::has_frame_requests()`；`dom_js` 的佇列在 isolate 的狀態裡，要 `&mut self`，讀表的地方多半只有 `&self`，改成 hook 設旗標（使用者選的，第 9 條）。第一版連子 frame 的清單也守：`shell` 找到五處、WPT 找到三處 renderer 在 engine 的呼叫之後、對帳之前就讀了清單（`docs/test-gaps.md` 的清單）；逐一先對帳修得掉，但對帳的動作跟著提早，和第 1 段「時機不變」不合，而且照這樣會變成每個會跑 script 的 engine 呼叫之後都要對帳（比較過的 (a)）。使用者選只守導覽的帳（B，第 10 條），加過的五處對帳改回原樣，八處記下來給第 2 段。

**測試**：`render_frame::tests::the_navigations_are_read_once_what_was_sent_is_booked`（送了沒對帳就讀會 panic、對帳中可以讀、拿走之後可以讀、之後再送又檢查、清單和文件不檢查）。`renderer`、`shell` 的測試通過（檢查開著）；`cargo check --release -p renderer` 通過。WPT（`html/browsers/`、`html/semantics/embedded-content/the-iframe-element/`、`html/semantics/scripting-1/the-script-element/`、`html/syntax/parsing/`、`css/cssom/`，970 個）：0 個退步；4 個非預期的通過：1b 記過的 2 個，和 `css-module`、`json-module`、`module` 的 `credentials.sub.html`、`script-crossorigin-network.sub.html` 裡的（在沒有 1c 的程式上照樣通過）。

**mutation test**（`mutants-1c.toml`，5 個，4 個抓到）：檢查失效、對帳不放行、旗標不清、對帳不結束都抓到。沒抓到的「hook 送出導覽時不設旗標」只是少檢查一次（`docs/test-gaps.md`）。第一輪另有「`is_navigating` 不檢查」沒抓到：它經過 `navigation`，自己的那一行多餘，拿掉了那一行和這個 mutant。

### 1b（2026-11-25，Linux：Fedora 45，debug build，`5b6297dd` 之上）

**做了什麼**：`RenderFrame` 多了 `navigation`（`NavigationRequest`：`begin_id`、`no_referrer`、`origin_source`，最上層和子 frame 同一種）和 `owner_load_at`；表上多了 `top_token`（Browser 給最上層的，換文件、換 engine 都留著）和跨 frame 的 `commits` 佇列。轉換是表的方法：`navigation_sent`（取代之前的導覽和排著的 commit）、`navigation`、`take_navigation`（照編號）、`take_any_navigation`（Browser 發起的 commit 拿走 renderer 在等的那個，照原本的行為）、`queue_commit`、`next_commit`、`is_navigating`、`owner_load_at`、`owner_loads_due`、`take_due_owner_loads`；`commit_top` 清掉子 frame 和它們的 commit、留著最上層在等的導覽，`remove` 連 commit 一起拿掉，`clear` 留著 token。`ChildFrames`、`ChildNavigationRequest`、`RenderThread` 的 `main_origin_source`、`devtools_main_frame_token` 拿掉。最上層每一次送出的導覽都記（原本只記要繼承 origin 的那種）：commit 時只拿 `begin_id` 對得上的，沒有 origin 的結果一樣。

和已決定的事項不同的兩處（改進了 README 的第 3、4 條）：DevTools 的 token 在表上、不在最上層的 `RenderFrame`；`FrameDocument` 的定義留在 `documents.rs`（1a 記過）。

**找到的 bug**（第 8 條）：frame 不見時它排著的 owner `load` 沒有拿掉（`ChildFrames::remove` 漏了 `owner_loads`）。看不到效果：到期時找不到 iframe 就跳過、順便清掉，只是最多多留 100 ms，`known()` 的期間把它當成還在。現在 frame 不見時一起拿掉，由單元測試守。

**測試**：`render_frame::tests` 多兩個：`a_frame_navigates_one_navigation_at_a_time`（取代之前的導覽和 commit、照編號拿、Browser 發起的拿任何一個、最上層的撐過別的 commit、子 frame 的 commit 跟著舊文件走）、`a_frame_gone_takes_its_owner_load_and_commits`（最早到期的先、拿走到期的、frame 不見時帶走它的、token 撐過換 engine）。`renderer`、`shell` 的測試通過。WPT（`html/browsers/`、`html/semantics/embedded-content/the-iframe-element/`，404 個）：0 個退步；2 個非預期的通過（`history_pushstate_url_rewriting.html`、`overlapping-navigations-and-traversals/cross-document-traversal-cross-document-nav.html` 的一個子測試）在沒有 1b 的程式上（`git stash`）三次都通過，是原本就過時的預期結果，沒改。

**mutation test**（`mutants-1b.toml`，14 個，都抓到）：每一種轉換（送出、拿走、排 commit、在不在導覽、最上層的 commit、owner `load` 的三個、frame 不見、換 engine）和「最上層的導覽沒記下來」（`shell` 的測試抓到，其他的是單元測試）。

### 1a（2026-11-25，Linux：Fedora 45，debug build，`65f6530a` 之上）

**做了什麼**：新的 `render_thread/render_frame.rs`：`RenderFrames`（`BTreeMap<FrameId, RenderFrame>` 加上最上層是哪一個）、`RenderFrame`（這一段只有 `document: Option<FrameDocument>`）。`RenderThread` 的 `document` 和 `ChildFrames::documents` 拿掉，`doc`、`doc_mut`、`is_top`、`frames`、走訪的地方都經過表；最上層 commit 用 `commit_top`（清掉舊文件的 frame），子 frame 的文件用 `set_child_document`，frame 不見時 `follow_child_frames` 也從表上拿掉，換 engine 時 `clear`。`ChildFrames` 只剩導覽的帳（1b 搬）。

和已決定的事項不同的一處：第 4 條說 `FrameDocument` 的定義搬進 `render_frame.rs`；它的三十個欄位是 `documents.rs` 私有的，搬過去要全部改成 `pub(super)`（`render_thread` 底下的每個模組都看得到），所以定義留在 `documents.rs`，表在 `render_frame.rs`。

走訪的順序：原本子 frame 的文件照 `follow_child_frames` 推進去的順序（parent 在 children 之前），commit 了新文件的子 frame 移到最後；現在照 frame 的編號（建立的順序），commit 不會移位。看得到順序的只有發子資源的請求、網路的 idle 的檢查這類，測試沒有差別。

**測試**：`renderer` 的 `render_frame::tests` 兩個（最上層在前、子 frame 照建立的順序、最上層 commit 時清掉舊的 frame；換掉子 frame 的文件、frame 不見、換 engine）。`renderer`、`shell` 的測試通過（`shell` 的 webdriver 的那一個見下一節）。

**mutation test**（`mutants-1a.toml`，9 個，都抓到）：第一輪 10 個，4 個抓到、2 個卡住（最上層永遠是「沒有」、走訪漏掉最上層：頁面等不到結果，測試等到 900 秒的時限），4 個沒抓到。沒抓到的：
- 子 frame commit 新文件之前先拿掉舊的（`drop_document`）是等價的：取消舊文件的 `fetch()` 看的是 DOM 裡文件被丟掉了沒，不讀這張表；拿掉了這一步和這個 mutant。
- 最上層 commit 時不清掉舊的 frame、frame 不見時不拿掉、子 frame 的走訪混進最上層：畫面和行為看不出來（過期的項目在下一次 `follow_child_frames` 清掉，或讀它的地方在最上層擋著畫面時不跑），守的是表自己的規則；補上面兩個單元測試之後都抓到。

### 原本就有的不穩定的測試：`webdriver::a_session_with_a_hung_page_ends_in_time`（2026-11-25，Linux：Fedora 45，debug build）

不是這份造成的，之後另外查。

- **怎麼碰到的**：1a 的 `cargo test -p renderer -p shell` 有一次失敗。那次整套 `shell` 跑了 69 秒，平常是 12 秒，機器很忙。單獨跑、沒有負載時都會過。
- **怎麼重現**：每個 CPU 跑一個空迴圈，同時跑 `cargo test -p shell --test it webdriver::`。1a 的程式 15 次失敗 11 次；沒有 1a 的程式（`65f6530a`，`git stash`）8 次失敗 7 次。
- **失敗在哪**：測試送給 machopodriver 的一個 HTTP 請求 60 秒內沒有回應，讀回應時逾時（`webdriver.rs` 的 `raw`，`read_to_string` 是 `WouldBlock`）。不是「刪 session 要在 4 秒內」那個斷言。還不知道是哪一個請求（建 session、navigate 還是刪 session）。
- **猜錯的**：以為頁面的 timer（100 ms 之後無窮迴圈）比 `load` 先觸發，navigate 等不到 `load`。改成 `load` 之後才開始迴圈，照樣 8 次失敗 7 次，所以改回原樣。
- **現在的猜測，還沒驗證**：navigate 在 `load` 之後還要 renderer 回答一次（網址、readiness 或一個畫面）。機器忙的時候 renderer 先進了迴圈，那個問題沒人回答，navigate 等到 `pageLoad` 的 300 秒時限，測試的 HTTP 60 秒就放棄了。如果是這樣，就是 machopodriver 這條路的問題：Chrome 的 navigate 收到 renderer 送的「載入完成」就回。
- **之後怎麼查**：在負載下加 log 或 backtrace，看是哪一個請求沒回、machopodriver 和 Browser 在等什麼。
- **做錯的**：查的時候兩次用 `pkill -f`、`pgrep -f` 砍負載的迴圈，樣式也比對到那條指令自己，指令被砍掉（一次是 `git stash` 沒做，一次是測試頁沒改回）；兩次都確認過狀態再重做，沒有丟掉改動。
