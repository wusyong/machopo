# RFC 0030 的進度：testdriver

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-10。完成：2026-10-10，Windows 11（Rust 1.99.0）。四段都在 `0030` 上做完，沒有分段 commit，四段做完時一起審核。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. `postMessage` 和 `MessageEvent` | 完成 | 2026-10-10 |
| 2. User activation | 完成 | 2026-10-10 |
| 3. Perform Actions、Release Actions | 完成 | 2026-10-10 |
| 4. Element Send Keys、Element Clear 和文字輸入 | 完成 | 2026-10-10 |

## 驗收

每一段做完時都跑過 fmt（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`、`sanity.py` 的四項、`test.py wpt` 和 `scenarios`；第 2、3、4 段跑 `automation`，第 3、4 段跑 `wdspec`。machopodriver 和自動化訊息的改動由 `cargo test` 裡的 `webdriver`、`automation_pipe` 守著。

**`postMessage` 和 `MessageEvent`**

| 項目 | 怎麼驗證 |
| --- | --- |
| `postMessage` 的訊息在自己的 task 裡、trusted、不 bubble；`data` 是複本、每次讀是同一個物件；`origin`、`source`、`lastEventId`；listener 和 `onmessage` | `cargo test -p dom_js --test messaging a_message_comes_in_a_task_of_its_own` |
| `targetOrigin`：`"*"`、`"/"`（和沒給）、URL 的 origin、opaque 的 origin | `cargo test -p dom_js --test messaging the_target_origin_filters_the_message` |
| 錯誤和副作用的順序：`options` 的轉換、`SyntaxError`、`DataCloneError`；丟出錯誤時不 post | `cargo test -p dom_js --test messaging errors_come_in_the_specs_order` |
| `origin` 和 `source` 是呼叫的一方（incumbent），不是目標 | `cargo test -p dom_js --test messaging origin_and_source_are_the_callers` |
| `data` 是目標 realm 的物件，別的 realm 先讀也一樣 | `cargo test -p dom_js --test messaging the_data_is_the_target_realms` |
| 目標文件在 task 執行前被丟棄時，訊息跟著丟掉 | `cargo test -p dom_js --test messaging a_message_to_a_discarded_document_is_dropped` |
| `MessageEvent` 的建構子、`initMessageEvent`（也換掉 posted message）、`createEvent` | `cargo test -p dom_js --test messaging script_makes_and_initializes_message_events` |
| `<body onmessage>` 是 window 的 handler | `cargo test -p dom_js --test messaging body_onmessage_is_the_windows` |
| 整個瀏覽器裡，頁面和子 frame 互相 post；別的 origin 的訊息被丟掉 | `cargo test -p shell --test messaging` |
| WPT 的 `webmessaging/` 和 `moving-between-documents/` | `python3 scripts/test.py wpt`（0 個預期之外） |

**User activation**

| 項目 | 怎麼驗證 |
| --- | --- |
| user activation：`navigator.userActivation` 每次同一個；使用者的輸入讓它 5 秒內 `isActive`、之後一直 `hasBeenActive` | `cargo test -p dom_js --test user_activation user_input_activates_its_window_for_a_while` |
| script 的 `click()`、`dispatchEvent` 不 activate | `cargo test -p dom_js --test user_activation script_events_dont_activate` |
| activate 祖先（不管 origin）和同源的子孫，不 activate 兄弟和別的 origin 的子孫 | `cargo test -p dom_js --test user_activation activation_reaches_ancestors_and_same_origin_descendants` |
| 真的輸入：點擊在 `mousedown` 分派之前 activate；Escape 不 activate、其他按鍵會；換文件重設 | `python3 scripts/test.py scenarios user-activation` |
| `UserActivation` 的 GC 物件（slot 讀回、wrapper 只 unwrap 成自己） | `cargo test -p dom --test script_objects user_activation`、`--test script` |
| WPT 的 `html/user-activation/` 只靠點擊的 4 個 | `python3 scripts/test.py wpt`（0 個預期之外） |

**自動化訊息、Perform Actions 和 Release Actions**

| 項目 | 怎麼驗證 |
| --- | --- |
| `Input.dispatchKeyEvent`：`keyDown`、`rawKeyDown`、`keyUp` 和欄位；不經過瀏覽器的快捷鍵；沒有 `char` | `cargo test -p shell --test automation_pipe -- key_events_go_straight_to_the_page` |
| `Input.dispatchMouseEvent` 的 `mouseWheel`（事件、modifier、捲動、被取消的不捲）；移動分派之後才回答 | `cargo test -p shell --test automation_pipe -- wheels_scroll_the_page mouse_events_go_straight_to_the_page` |
| `Kun.getFrameViewport`、`Kun.getElementClickPoint` 的 `inViewCenterOnly`（不捲動、不檢查、frame 的 viewport 的座標、沒有框是 `move target out of bounds`） | `cargo test -p shell --test automation_pipe -- action_points_are_in_the_frame` |
| renderer 的移動在分派之後才 ack（Viz 停住時等到下一個 main frame） | `cargo test -p browser --test navigation -- an_ack_waits_for_the_frame_its_input_asked_for` |
| Actions 的鍵表（`key`、`code`、`location`、`keyCode`、打出的字） | `cargo test -p driver --lib keys_are_the_spec_tables` |
| 按著的 modifier 套到之後的鍵盤和滑鼠事件、跨 Perform Actions 一直按著；同一個鍵再按是 repeat；沒按的鍵放開不送；Release Actions 照反序 | `cargo test -p driver --lib modifiers_are_held_and_keys_repeat` |
| 連點：同一個按鈕、500 ms 內、4 px 以內；只在同一個 Perform Actions 裡；`buttons` 的位元；已經按著的不再按 | `cargo test -p driver --lib presses_in_a_row_count_up` |
| 連點的 500 ms 是動作自己的時間：renderer 每個事件都慢也照樣是雙擊，`pause` 500 ms 就不是 | `cargo test -p driver --lib presses_count_up_in_the_actions_time` |
| 座標：viewport、`pointer`、元素 origin，目前 frame 的 viewport 換成 top-level；邊界上可以、外面是 `move target out of bounds` | `cargo test -p driver --lib moves_are_from_their_origin_in_the_viewport` |
| tick 等最長的 `duration`；來源的種類不變；`pen`、`touch`、第 3、4 個按鍵是 `unsupported operation`；最大安全整數；scroll 的參數；同一個 id 兩次 | `cargo test -p driver --lib ticks_wait_and_wrong_actions_are_refused` |
| machopodriver 對真的 kun：一個鍵按著時的雙擊、元素 origin、`pause` 的時間（`300.0`）、子 frame 的座標、滾輪捲動、Release Actions 的順序和忘掉來源、`pen` 和按鍵 3 | `cargo test -p shell --test webdriver -- actions_are_the_users_input` |
| 每個視窗一份輸入狀態 | `cargo test -p shell --test webdriver -- input_state_is_each_windows` |
| WPT：原本為了 testdriver 的輸入停用的 80 個、`uievents/` 的 15 個和 `tab-table-caption.html`；wdspec 的 `perform_actions` | `python3 scripts/test.py wpt`、`python3 scripts/test.py wdspec`（0 個預期之外） |

**文字輸入、Element Send Keys 和 Element Clear**

| 項目 | 怎麼驗證 |
| --- | --- |
| 打字：`beforeinput`、`textInput`、`input`（trusted，前兩個可以取消），在 selection 插入、游標放到後面；不是文字控制項的不接手 | `cargo test -p dom_js --test editing typing_inserts_at_the_selection canceling_stops_the_change` |
| Backspace、Delete 以 grapheme cluster 刪除或刪 selection；在頭尾什麼都不做 | `cargo test -p dom_js --test editing deleting_goes_by_grapheme_cluster` |
| ←、→、Home、End 移動游標，Shift 延伸選取，Home、End 到這一行的頭尾 | `cargo test -p dom_js --test editing the_cursor_moves` |
| `maxlength`、`readonly`；Enter（`<textarea>` 換行，`<input>` commit）；commit 時值不同才 `change`，script 設的值不算 | `cargo test -p dom_js --test editing maxlength_and_readonly_hold_the_value enter_breaks_a_line_or_commits a_commit_fires_change_for_the_users_change` |
| `TextEvent`：沒有建構子、`createEvent`、`initTextEvent` 的預設值 | `cargo test -p dom_js --test editing text_events_from_script` |
| 真的按鍵：打字、Enter、←、Backspace、Home、Shift+←（`select`）、Space 不捲頁、焦點離開時 `change` | `python3 scripts/test.py scenarios typing` |
| Send Keys 的按鍵：自動 Shift、modifier 和 NULL、右側的 modifier、多個 code point 的 cluster、和 Actions 按著的鍵一起算、來源用完拿掉 | `cargo test -p driver --lib send_keys_types_each_character` |
| machopodriver 對真的 kun：Send Keys 的事件順序、游標（沒焦點時放到最後、有焦點時不動）、換控制項時前一個的 `change`、唯讀、不可輸入的控制項、檔案、不能取得焦點的、implicit wait | `cargo test -p shell --test webdriver -- keys_are_sent_to_elements` |
| Element Clear：事件、空的不動、打過字之後只有一次 `change`、`range` 清空後的值、唯讀和 checkbox、畫面外的 | `cargo test -p shell --test webdriver -- elements_are_cleared` |
| WPT：`uievents/textInput/`、焦點的 Tab、Escape 的 user activation、表單的 5 個；wdspec 的 `element_send_keys`、`element_clear` | `python3 scripts/test.py wpt`、`python3 scripts/test.py wdspec`（0 個預期之外） |

## 做過的檢查

2026-10-10，Windows 11。每一段的設計和更正寫進了 `README.md`；這裡是濃縮的紀錄。

### 第 1 段：`postMessage` 和 `MessageEvent`

- **做了什麼**：`Window.postMessage`（兩種寫法合成一個；`WindowPostMessageOptions` 只有 `targetOrigin`）、`MessageEvent`（`dom` 的 `EventKind::Message`：`data` 的 `ValueSlot`、`source` 的 `Edge<Window>`；字串和 posted message 在 `Event` 最外層的 `#[no_trace]` 欄位）、`onmessage`、`onmessageerror`、`createEvent("MessageEvent")`；`ScriptHost` 加 `serialize` 和 `incumbent_window`（`dom_js` 用 `ValueSerializer` 和 V8 的 entered context）；DOM task 加 `TaskSource::PostedMessage`。`PopStateValue` 改名 `ScriptOrSerialized`（`MessageEvent.data` 也用它）。WIT 多了 `message-event`（沒有 `data`），wasm 的 guest 和 Lua 的 runtime 照新的 WIT 重新產生（`ci.py guests`）；`spec-missing.txt` 少了 `onmessage`、`onmessageerror`、`postMessage()`，多了 `MessageEvent.ports`、`MessageEventInit.ports`。
- **新的測試**：`dom_js` 的 `messaging`（8 個：真的 V8、真的 iframe，DOM 的 task 照 renderer 的方式跑），`shell` 的 `messaging`（頁面和 HTTP 載入的子 frame 互相 post，別的 origin 的被丟掉）。
- **WPT**：試跑整個 `webmessaging/`（162 個測試 id）之後，照「目錄裡大部分能跑才整個加」加了 `with-options/`（9 個檔、6 個整個通過）、`with-ports/`（24、15）、`without-ports/`（30、19）和最上層的 11 個（10 個整個通過）；其他的要 `MessageChannel`、`BroadcastChannel`、worker 或跨源。沒過的照實記下：`ports`（第 9 條）、第三個參數（transfer）的型別檢查、跨源的 window、canvas、worker、`<body>` 的 IDL `onmessageerror`；兩個 `ports` 目錄的 `016.html`、`018.html` 等到 harness 的 10 秒逾時（`data:`、`javascript:` 的 frame 的訊息）。原本因為 `postMessage` 沒加的 `moving-between-documents/` 整個加了（56 個、44 個整個通過，沒有等到逾時的）；iframe 和 window 的目錄重試，大部分還是不過（166 個裡 36 個、98 個裡 8 個整個通過），照舊不加。
- **原本 `disabled` 的**：因為 `postMessage` 的 8 個和 testdriver 裡只靠 `click` 的 3 個，8 個現在照實記下；`Event-dispatch-on-disabled-elements.html`（等 CSS transition 的事件）和 `preload/link-header-modulepreload.html`（`Link` header 的 modulepreload）照樣等到 60 秒的長逾時，理由換成真正的原因；`location-protocol-setter-with-colon.sub.html` 見「不穩定的測試」。
- **跟著變的**：
  - 用 testdriver 的 `Actions` 的 4 個 crashtest（`insertBefore-iframe-crash.html` 和 `moveBefore/` 的 3 個）：以前回覆送不回去、馬上 ERROR；現在回覆送得回去，但 machopodriver 沒有 Perform Actions，請求被拒絕，測試等到 15 秒的逾時：`disabled`，第 3 段拿掉。
  - 十幾個既有的測試因為 `postMessage` 往前走（`createEvent("MessageEvent")`、`dispatchEvent` 的 `MessageEvent`、等子 frame 的訊息的 cssom-view、CORS 的 sandboxed iframe、`credentials.sub.html` 等），預期結果照實更新。
- **和這段無關、做的時候發現的**：`fetch/api/request/request-bad-port.any.html` 在 Windows 上等到 harness 的逾時（第一個 bad port 13 的請求沒有結束）；把這段的改動 stash 起來、在 `main`（`86fc7811`）上跑也一樣，所以是 `main` 上本來就有的（kun 沒有擋 Fetch 的 bad port）。預期結果寫成只有 `if os == "win"` 時 TIMEOUT，限制記進 `docs/limitations.md` 的「網路」。
- **不穩定的測試**：
  - `location-protocol-setter-with-colon.sub.html`：整套跑四次失敗一次，單獨跑 6 次都通過；和 iframe 多的目錄一起反覆跑（wptrunner 的 `repeat`）重現了，5 次失敗 2 次。暫時加的除錯輸出顯示每次都只有子頁第一次載入時的兩則訊息：測試改子 frame 的 `location.protocol`，要子 frame 導覽到另一份文件再送一次，kun 不會把子 frame 導覽到另一份文件，所以只有在第一次的訊息比父頁的 `load`（它才開始聽）晚送到時碰巧通過。照舊 `disabled`，理由寫明。
  - `moving-between-documents/after-prepare-createHTMLDocument-success-external-module.html`：整套跑時有一次預期之外的通過。12 個 after-prepare 的測試靠擋住 script 的 stylesheet，把 script 留在「準備好、還沒執行」時搬走；kun 的 stylesheet 不擋 script（`docs/limitations.md` 的「Parser 和載入」），script 通常先執行（失敗），偶爾搬動先發生。失敗的子測試記成 `[FAIL, PASS]`，限制那一條補上。
  - 之後整套連跑兩次，都是 0 個預期之外。
- **mutation test**（`mutations-stage1.toml`）：14 條都抓到。訊息當場分派而不是排 task、不檢查 target origin、opaque 的 target origin 符合任何文件、先序列化再解析 target origin、`source` 用目標的 realm、V8 的 incumbent 用 current context、事件不是 trusted、`data` 不存在 event 上、`initMessageEvent` 留著 posted message、`initMessageEvent` 不改 `source`、建構子不留 `source`、`createEvent` 沒有 `MessageEvent`、`<body onmessage>` 不是 window 的、（端到端）訊息送錯 window。第一次跑時「`data` 不存在 event 上（改存在當下的 window）」沒抓到：測試都在目標 realm 讀 `data`，兩者一樣；補了 `the_data_is_the_target_realms`（父頁用自己的 getter 讀子 frame 留下的事件）之後抓到。「`"/"` 當成 `"*"`」現在是等價的（kun 的 script 碰不到別的 origin 的 window），記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」，不放進 spec。每次跑完都確認檔案還原了。
- **順手修的**：根目錄 README 的環境變數表把 kun 自己的 reftest 的預期結果寫成 `tests/reftests/` 底下的 `metadata` 路徑，那個目錄只在跑過 `test.py reftest` 之後才有，`sanity.py docs` 在乾淨的 checkout 會失敗；改成不寫成路徑。
- **跑過的檢查**：`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`（1788 個）、`sanity.py` 的四項、`test.py wpt`（4237 個測試 id，0 個預期之外，整套連跑兩次）、`scenarios`（41 次）都通過，WPT 的 checkout 不變；跑完 `ci.py prune`。

### 第 2 段：user activation

- **做了什麼**：`dom` 的 `Window` 多了「last activation timestamp」（`#[no_trace]` 的 `Cell<f64>`，沒有過是 +∞）；`UserActivation` 是新的 GC 型別（`ObjectTag` 變成 31 種），由 `Navigator` 保存；`dom_api` 的 `user_activation`（`notify_activation`、sticky 和 transient 的查詢、5 秒的期限）和宣告表的 `navigator.userActivation`、`UserActivation`；renderer 在分派 trusted 的 `mousedown` 和 Escape 以外的 `keydown` 之前呼叫 `notify_activation`。WIT 多了 `user-activation`，wasm 的 guest 和 Lua 的 runtime 重新產生；`spec-missing.txt` 少了 `Navigator.userActivation`。
- **新的測試**：`dom_js` 的 `user_activation`（3 個：可以設定的時鐘、巢狀的 iframe，把一個 iframe 的文件改成別的 origin）；情境 `user-activation`（點擊、Escape、其他按鍵、script 的 `click()` 和 `dispatchEvent`，在沒有平台視窗的視窗裡，macOS 和其他平台的快捷鍵各一次）；`dom` 的 `script_objects` 和 `script` 加上新的 GC 型別（每種型別都要一個）。
- **WPT**：試跑整個 `html/user-activation/`（19 個檔、20 個測試 id）：只靠點擊（testdriver 的 `click`、`bless`，經過 machopodriver 的真的輸入）的 `chained-setTimeout.html`、`detached-iframe.html`、`propagation-sameorigin.html`、`user-activation-interface.html` 整個通過，加進清單；其他的要 testdriver 的 `Actions` 和 `send_keys`（第 3、4 段）、`window.open`、全螢幕（`activation-trigger-mouse-left.html` 用 `requestFullscreen` 來用掉 activation、藉此判斷）或跨源，大部分不過，不整個加。清單裡原本的測試沒有因為 activation 改變的。
- **mutation test**（`mutations-stage2.toml`）：12 條都抓到。renderer 的（情境抓到）：`keydown` 不 activate、Escape 也 activate、`mousedown` 不 activate、`mousedown` 在分派之後才 activate；`dom_api` 和 `dom` 的（`dom_js` 的測試抓到）：祖先不 activate、子孫不 activate、別的 origin 的子孫也 activate、transient 多一毫秒、transient 不過期、window 一開始就 activate 過、`hasBeenActive` 是 `isActive`、`userActivation` 每次是新的物件。第一次跑時「分派之後才 activate」沒抓到，是 mutant 寫錯了（只多加一次，原本分派之前的那次還在），改正之後抓到。每次跑完都確認檔案還原了。
- **跑過的檢查**：`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`（1792 個）、`sanity.py` 的四項、`test.py wpt`（0 個預期之外）、`scenarios`、`automation`（真的視窗，第 2 段是階段的驗收）都通過，WPT 的 checkout 不變。第一次跑 clippy 和 `cargo test` 時，`dom` 的兩個測試（每種 GC 型別都要列出來的那兩個）沒有新的型別而編不過，補上之後通過。這份動到 GC 的型別：`test.py matrix`、`fuzz` 照規則在整份 RFC 完成時跑（見最後的「RFC 完成時」）。

### 第 3 段：Perform Actions、Release Actions

- **做了什麼**：machopodriver 的 `actions`（`process` 檢查整個請求、建立來源、照 tick 分組；`perform` 一個 tick 一個 tick 分派、等滿最長的 `duration`；`release` 照 input cancel list 的反序放開）、規範的鍵表和美式鍵盤的 `keyCode`、每個視窗一份的輸入狀態（session 上的 map，關掉視窗時丟掉）、Perform Actions 的 body 裡值為整數的 float 寫成整數（`http`）。自動化訊息加上 `Input.dispatchKeyEvent`、`Input.dispatchMouseEvent` 的 `mouseWheel`、`Kun.getElementClickPoint` 的 `inViewCenterOnly`、`Kun.getFrameViewport`，WebDriver 的錯誤加上 `move target out of bounds`；renderer 算 in-view center point（元素所在 frame 的 viewport 的座標）和 frame 的 viewport 在 top-level 裡的位置（`FrameView::frame_viewport`，從 pre-paint 的 transform 反推）。renderer 的滑鼠移動改成分派之後才 ack（第 21 條）。
- **試跑時發現、改掉的**（都記進「已決定的事項」第 18 到 21 條）：wdspec 第一次跑，`mousemove` 常常收不到：移動在下一個 frame 才分派，renderer 卻收到就 ack，`Input.dispatchMouseEvent` 回答時網頁還沒收到（Element Click 沒事，是因為按下會先分派排著的移動）；改成分派之後才 ack。`invalid.py` 整份逾時：`duration` 是 `2^53` 的 `pause`，crate 收下了，machopodriver 就真的等；加上最大安全整數的檢查。`key.py` 的 `pause` 被拒絕：wptrunner 的 `duration` 是 `100.0`，crate 不收；改成先寫成整數。子 frame 裡的 `origin: viewport`、`pointer` 錯了：原本當成 top-level 的座標；改照規範用目前的 browsing context 的 viewport。`release_actions/sequence_tentative.py` 的兩次 Perform Actions 被算成雙擊；連點只在同一個 Perform Actions 裡算。
- **改了既有的測試**：`browser` 的 `an_ack_waits_for_the_frame_its_input_asked_for` 原本斷言「Viz 停住時移動的 ack 馬上來」；現在移動要等分派它的 main frame（停住時不會有），改成恢復之後移動的 ack 先到（不帶 frame）、滾輪的後到（帶 frame）。`automation_pipe` 的 `mouse_events_go_straight_to_the_page` 原本把 `mouseWheel` 當成不合法，換成 `mouseDragged`，並檢查移動回答時網頁已經收到 `mousemove`。
- **新的測試**：`driver` 的單元測試 5 個（假的視窗，時鐘由測試控制：鍵表、modifier 和 repeat、連點、座標和邊界、tick 和錯誤）；`automation_pipe` 3 個（`key_events_go_straight_to_the_page`、`wheels_scroll_the_page`、`action_points_are_in_the_frame`）；`webdriver` 2 個（`actions_are_the_users_input`、`input_state_is_each_windows`）。
- **WPT**：
  - wdspec 加上 `perform_actions`。第一次試跑（上面那幾個問題還沒改）`perform_actions` 是 75 個子測試通過；改完之後 `perform_actions` 和 `release_actions` 一共 521 個通過、72 個失敗、141 個 ERROR，`release_actions/sequence_tentative.py` 兩個都通過。沒過的大多不是 Actions 的：鍵盤的測試先點 `<input>`（大小是 0，點不到，fixture 就失敗）；`test_actions.html` 的座標照 Chrome 的版面寫死（`<input>` 是 0 高，下面的元素往上移）；wheel 的測試頁先呼叫 `customElements.define`（kun 沒有，script 中斷，listener 和 iframe 都沒有）；`vw`、`vh`（kun 沒有，`test_element_larger_than_viewport`，限制補進 `docs/limitations.md`）；包著 block 的 `<a>` 沒有 client rect；`window.open`、`window.close`、`getSelection`、shadow DOM、拖放、`pen` 和 `touch`；Actions 本身的只有 `duration.py` 和 `test_interspersed_wheel_pointermove`（要求把有 duration 的移動拆成好幾個，第 2 條不拆）。
  - 為了 testdriver 的輸入停用的 84 個檔裡，不用 `send_keys`、`get_computed_role` 的 80 個拿掉 `disabled` 試跑：20 個整個通過（`non-cancelable-when-passive/` 的 16 個滾輪、`focus-event-document-move.html`、`handler-count.html`、`wheel-event-composed.html`、`insertBefore-iframe-crash.html`），大多數照實記下失敗（touch、`scrollend`、Pointer Events、smooth scroll、`getSelection`、`moveBefore`、window 的 named property、shadow DOM），9 個還是等到逾時、照舊停用並寫明原因（5 個等 `scrollend`、3 個 crashtest 要 `moveBefore`、1 個 post 到跨源的子 frame）。舊的「testdriver: input from outside the page」註解拿掉。
  - 試跑 `uievents/`、`html/user-activation/`、`html/interaction/focus/`：`uievents/` 用 `Actions` 和 `click` 的 15 個檔整個通過、加進清單（點擊、雙擊、`contextmenu`、`auxclick`、移動、滾輪），`tab-table-caption.html`（Tab 經過 `Actions`）也是；鍵盤的大多要 `send_keys`（第 4 段）或 window 的 named property，user activation 的鍵盤測試要 `send_keys`，滑鼠的要用掉 activation（全螢幕），照舊不加。
  - 跟著變的：`css/cssom-view/smooth-scrollIntoView-with-unrelated-gesture-scroll.html` 從 ERROR（`Actions` 被拒絕）變成 TIMEOUT（等 `scrollend`），一樣等滿逾時，照實更新。
- **mutation test**（`mutations-stage3.toml`）：35 條都抓到。machopodriver 的（`driver` 的單元測試）：鍵表（numpad 的 Enter、右邊的 modifier、numpad 數字的 `keyCode`、Enter 不打字、WebDriver 保留的碼位打字）、不 repeat、Shift 不算按著、沒按的鍵也放開、連點的 500 ms 和 4 px 的邊界、跨 Perform Actions 連點、中鍵的位元、Release 的順序、Release 不放鍵、tick 加總、tick 不等、viewport 的邊界、`pointer` origin、子 frame 的換算、`pen`、按鍵 3、4、最大安全整數、scroll 從 `pointer`、來源換種類、同一個 id 兩次；端到端的：`100.0` 被拒絕、整個 session 一份狀態、Release 留著來源（`webdriver`），`rawKeyDown` 打字、`location` 不檢查、滾輪的 x、y 對調、元素 origin 捲動和檢查、移動先 ack、frame 的 viewport 的符號、in-view center 不限制在 viewport 裡（`automation_pipe`）。沒放進 spec 的兩個寫在它的開頭：關掉的視窗留在 map 裡的狀態（等價：handle 不會再用到），和分頁被隱藏時分派排著的移動：試著在 `browser` 的 `hiding_a_tab_acks_the_input_waiting_for_a_frame` 停住 Viz、送一個移動再隱藏，弄壞之後照樣通過：輸入經過 compositor thread 才到 main，隱藏直接到 main，移動常在隱藏之後才到、當場分派，測試控制不了先後；測試改動還原，記進 `docs/test-gaps.md` 的「只靠審查」。`cargo fmt` 之後一條的原文變了，改了 spec 再單獨跑一次，也抓到。每次跑完都確認檔案還原了。
- **跑過的檢查**：`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`（1802 個）、`sanity.py` 的四項、`test.py wpt`（4258 個測試 id，0 個預期之外，整套跑兩次）、`test.py wdspec`（141 個測試檔，0 個預期之外）、`scenarios`（43 次）、`automation`（34 個情境和 `pipe --window`，真的視窗，第 3 段是階段的驗收；看了截圖）都通過，WPT 的 checkout 不變；跑完 `ci.py prune`。第一次跑 `cargo test --workspace` 時 `browser` 的那個 ack 測試失敗（上面「改了既有的測試」）。第一次試跑 wdspec 時，runner 說 checkout 裡多了 `webdriver/tests/support/__pycache__/`（那一次也有 `invalid.py` 被逾時中斷）；刪掉之後再跑五次都沒有再出現。

### 第 4 段：Element Send Keys、Element Clear 和文字輸入

- **做了什麼**：`dom_api` 新的 `editing`（`type_text`、`edit_key`、`commit`，grapheme cluster 用 `icu_segmenter`）和 WebDriver 的部分（`send_keys_target`、`set_non_typeable_value`、`cursor_to_end`、`is_editable`、`clear`）；`TextEvent`（`dom` 的 `UiEventFields::Text`，`createEvent('TextEvent')`、`initTextEvent`，沒有建構子）；`dom` 的 `InputState`、`TextAreaState` 多了 `value_before_edit`，focus 的更新步驟在 `blur` 之前 commit；`focus` 加上 WebDriver 用的 `is_keyboard_interactable`、`is_focused`、`run_focusing_steps`、`run_unfocusing_steps`。renderer 在 `keypress` 之後呼叫 `type_text`、在 Backspace 等鍵的 `keydown` 之後呼叫 `edit_key`，接手的鍵不再捲頁。自動化訊息加上 `Kun.prepareSendKeys`、`Kun.clearElement`，WebDriver 的錯誤加上 `invalid element state`；machopodriver 加上 Element Send Keys（`actions::send_keys`：暫時的鍵盤來源、自動 Shift、NULL）和 Element Clear，兩者回答 `element not interactable` 時在 implicit wait 內再問。情境的 Enter 帶 `\r`（第 33 條）。WIT 多了 `text-event`，wasm 的 guest 和 Lua 的 runtime 重新產生（`ci.py guests`）。
- **讀規範時改正的**：第 26 條原本照 Chromium 寫成 `textInput` 在 `beforeinput` 之前；WPT 的 `uievents/textInput/` 斷言 `textInput` 之前已經有 `beforeinput`（UI Events 的順序），照它做、改正那一條（第 32 條）。
- **新的測試**：`dom_js` 的 `editing`（8 個：插入、取消、以 grapheme cluster 刪除、游標和選取、`maxlength` 和唯讀、Enter、commit、`TextEvent`）；`dom_api` 的單元測試 2 個（grapheme 的邊界、行的頭尾）；`driver` 的 `send_keys_types_each_character`；`webdriver` 的 `keys_are_sent_to_elements`、`elements_are_cleared`；情境 `typing`（真的按鍵：打字、Enter、←、Backspace、Home、Shift+←、Space 不捲頁、Tab 離開時的 `change`）。
- **試跑時發現、和這段無關的**：（1）kun 處理 `data:` 網址時，body 裡的 `%3F` 之後整段不見，`location.href` 也被截斷（探測 machopodriver 時發現；另開工作處理，沒有在這段改）。（2）`HTMLElement` 沒有 `hidden` 的 IDL attribute（`el.hidden = false` 只是 expando），`webdriver` 的測試改用 `removeAttribute`（rebase 到 `main` 之後，`main` 已經有它）。
- **WPT**：
  - wdspec 加上 `element_send_keys`、`element_clear`：一共 135 個子測試通過；沒過的是 kun 沒有的（檔案、`contenteditable` 和 `designMode`、`validity`、`visibility`、Get Active Element、shadow root、user prompt、`color` 的值清理）。`close_window` 和 `delete_session` 的 beforeunload 測試先用 Send Keys 改一個 `<input>`，以前這一步就失敗，現在通過（7 個）。`perform_actions` 的鍵盤測試照舊在 fixture 失敗（先點 0×0 的 `<input>`）。
  - testharness：還停用的 3 個 `send_keys` 的（`get_computed_role` 的那 1 個照舊）：`change-event.window.js` 整個通過，`scrollend-event-not-fired-on-no-scroll.html` 照實記下失敗（window 的 named property），`input-text-scroll-event-when-using-arrow-keys.html` 等到逾時（等文字控制項自己的捲動），照舊停用、寫明原因。試跑 `uievents/textInput/`、`uievents/keyboard/`、`input-events/`、`html/interaction/focus/`、`html/user-activation/` 和表單目錄裡用 `send_keys` 的 38 個檔：加了整個 `uievents/textInput/`（`<input>`、`<textarea>` 的部分都過，`contenteditable`、`getSelection`、`execCommand` 的照實記下）、`uievents/keyboard/keydown-input-events.html`、焦點的 7 個（Tab）、user activation 的 2 個（Escape）、表單的 5 個；`input-events/` 大多是 `contenteditable`，不加。
  - 跟著變的：`TextEvent` 讓 `Document-createEvent.https.html`、`EventTarget-dispatchEvent.html` 多過了幾個子測試，預期結果照實更新。
- **mutation test**（`mutations-stage4.toml`）：33 條第一次跑抓到 31 條。沒抓到的兩條都補了測試：「控制項接手的鍵照樣捲頁」（原名「renderer 在 keydown 不編輯」，但 mutant 只拿掉接手、編輯照做）情境裡頁面本來就在最上面，Home 捲不捲看不出來，改成控制項上面留空白、先把頁面捲下去，確認 Home 不捲回頂端；「Clear 留下使用者的改變」測試裡打字前的值是空的，清空後和它相同，改用有初始值的控制項。改名、補測試之後單獨重跑兩條都抓到。其他抓到的：`beforeinput`、`textInput` 不能取消、改完不分派 `input`、游標留在前面、Backspace 只刪一個 code unit、選取時的 ← 從 focus 移動、Shift 不延伸、Home 到值的開頭、不管 `maxlength`、唯讀也打得進、`<input>` 的 Enter 不 commit、commit 一律 `change`、每次改都當成基準、Delete 不刪、`createEvent` 沒有 `TextEvent`、`initTextEvent` 預設空字串、使用者選取不排 `select`、焦點離開不 commit、renderer 不打字、情境的 Enter 沒有文字（情境、`dom_js`）；Send Keys 的游標兩條、檔案、不可輸入的控制項、不能取得焦點的、driver 不等、Clear 動到空的、Clear 不看畫面（`webdriver`）；不自動 Shift、NULL 不放開、結束時不放開（`driver`）。`cargo fmt` 之後第 3 段的「整個 session 一份輸入狀態」那條的原文不再唯一（Send Keys 也有同一行），補上前一行。每次跑完都確認檔案還原了。
- **跑過的檢查**：`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`（1815 個）、`sanity.py` 的四項、`test.py wpt`（4279 個測試 id，0 個預期之外）、`test.py wdspec`（0 個預期之外）、`scenarios`（45 次）、`automation`（35 個情境和 `pipe --window`，`scenario.rs` 改了、也是階段的驗收；看了截圖）都通過，WPT 的 checkout 不變；跑完 `ci.py prune`。`test.py matrix`、`fuzz` 見最後的「RFC 完成時」。

### RFC 完成時

- `docs/README.md` 的「一份 RFC 做完時」，2026-10-10，Windows 11。第 4 段做完時的檢查就是完成時的驗證（上面）；`sanity.py deps` 也跑過（`dom_api`、`driver` 多了 `icu_segmenter` 的依賴）。
- **`test.py matrix` 和 `fuzz`**：這份動到 GC（`UserActivation` 是新的 GC 型別，`MessageEvent` 的 `ValueSlot` 和 `Edge<Window>`），照規則要跑；完成時 `matrix` 開始編 debug 的 V8 版本時照使用者的要求停下。之後在 RFC 0025 的 branch 上（2026-10-10，macOS，Apple Silicon，`a2ffb990`，包含這份和 RFC 0028）跑了：`matrix` 的 8 種設定都通過（每種 856 個測試），`fuzz --iterations 20`（seed `0x7fa86a87`）20 組都通過（`rfcs/0025-test-binaries/progress.md` 的「做過的檢查」）。
- **文件的一致性**：交給一個唯讀的 agent 對過這份改過的檔案（`git diff 86fc7811` 和新的檔案），找到的都改了：`docs/limitations.md` 的 UI Events 的 interface 和 `createEvent` 的清單（`TextEvent`、`MessageEvent`）、選取範圍（現在能用鍵盤選）、滑鼠移動的 ack；`docs/architecture.md` 的 renderer 的 ack；`docs/testing.md` 的 WebDriver 那一列；`crates/dom/README.md` 的 `UiEventFields`、`MessageEvent`、表單狀態的 `value_before_edit`、posted message 的 task source；`crates/dom_api/README.md` 的 `ScriptHost` 的 `serialize`、`incumbent_window`，`Task::PostMessage`、`ScriptOrSerialized`、`UserActivation`、`postMessage`；`crates/dom_js/README.md` 的序列化、incumbent 和三份新的測試；`crates/shell/README.md` 的按鍵表；兩處程式碼註解（`handlers.rs` 的 window 的 handler、`InputEventInit` 的理由）和 `protocol::automation` 的 `mouseWheel`；這份 RFC 裡第 10 條的欄位數、引用 `docs/limitations.md` 的節名、83 和 84 個停用檔的來由。另外把試跑整個目錄時替不在清單裡的測試寫出的 158 個 `.ini` 刪掉（`html/interaction/focus/` 和 `html/user-activation/`）。改完 `sanity.py docs` 通過。
- **rebase 到 `main`（2026-10-10，`683d1f8c`）**：`main` 在這段期間多了 `console`、HTML 的全域 attribute 和 template content、`<img>`、`HTMLAnchorElement`、CORS 的 module script 等。衝突照兩邊都留解掉：`dom` 的 `Window` 兩邊的欄位都留，task source 和 `Task` 兩邊的都留；renderer 的隱藏改成 `main` 的 `set_page_visible`，變成隱藏時先分派排著的滑鼠移動，再送欠的 ack（原本在 `was_hidden` 裡）；`spec-missing.txt` 拿掉兩邊各自補上的 `Navigator.userActivation` 和 `Node.isConnected`；`dom_js` 的兩份新測試照 `main` 的 `FrameClient::create_child_frame` 多帶網址。wasm 的 guest 和 Lua 的 runtime 重新產生。重跑：fmt、clippy、`cargo test --workspace`（1851）、`sanity.py` 四項、`test.py wpt`、`wdspec`、`scenarios`、`automation`。WPT 有 6 個和 `main` 的預期不同，都是兩邊合起來之後測試走得更遠：`console/` 的 5 個以前因為 testdriver 不通整個 ERROR，現在 testdriver 送出 `bidi.session.subscribe`、machopodriver 沒有 BiDi 而子測試 FAIL；`dom/events/pointer-event-document-move.html` 以前沒有 template content 在 fixture 就 ERROR，現在點得到、因為沒有 `pointerup` 而 FAIL。照實改了預期，改完 0 個 unexpected。

### 完成之後：連點的時間（2026-10-11，Fedora 44）

- **不穩定的測試**：在自己的 GitLab runner（Fedora 44，24 核）上，`shell` 的整合測試照預設的 24 條 thread 一起跑時，`webdriver` 的 `actions_are_the_users_input` 偶爾失敗：第二次按下的 `detail` 是 1，沒有 `dblclick`。同一個按鈕、同一個位置，只可能是兩次按下相隔 500 ms 以上：machopodriver 按下時讀時鐘，而兩次按下之間要等按下和放開各一次 renderer 的 ack，renderer 忙的時候超過 500 ms。chromedriver 也讀時鐘，但事件（除了最後一個）不等回答，所以不會這樣。改成用動作自己的時間（「已決定的事項」第 34 條）。
- **新的測試**：`driver` 的 `presses_count_up_in_the_actions_time`（假的視窗每個滑鼠事件要 400 ms：連著按照樣是雙擊，`pause` 500 ms 之後是單擊）。
- **mutation test**（手動，在另一份 checkout 上）：2 條都抓到。改回按下時讀時鐘（`presses_count_up_in_the_actions_time`）；tick 不加進動作的時間（它和 `presses_in_a_row_count_up`）。
- **跑過的檢查**：`cargo test -p driver --lib`、`cargo fmt --check`、clippy（`-p driver -p shell --all-targets -D warnings`）；`shell` 的整合測試照預設的 thread 數連跑 8 次都通過（改之前 6 次裡這個測試失敗 1 次）。
