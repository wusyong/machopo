# RFC 0022 的進度：輸入事件和 focus（F）

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-08。完成：2026-10-09，macOS Apple Silicon（Rust 1.99.0）。五段都在 `0022` 上做完，每一段做完時 commit、審核，最後一起 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 事件的 interface 和 GlobalEventHandlers（第 9、10 條；`kun:dom` 0.9.0） | 完成 | 2026-10-08 |
| 2 `click()` 和 activation behavior 的架構、default event handler（第 11 到 13 條） | 完成 | 2026-10-09 |
| 3 focus 模型、元素狀態和 `:focus*`、`SetFocus`（第 14、15 條） | 完成 | 2026-10-09 |
| 4a 按鍵：路徑改到 main thread、`keydown` / `keypress` / `keyup`、預設動作、Tab、`TakeFocus`（第 16 到 18 條） | 完成 | 2026-10-09 |
| 4b 滑鼠：`mousedown` / `mouseup` / `mousemove` / `dblclick` / `contextmenu`、按下移動 focus（第 19 到 22 條） | 完成 | 2026-10-09 |
| 4c 滾輪：`wheel`、passive by default、wheel event handler region、`mousewheel`（第 23 到 27 條；`kun:dom` 0.10.0） | 完成 | 2026-10-09 |
| 5 收尾：`about:demo` 和情境、`matrix` 和 `fuzz`、文件 | 完成 | 2026-10-09 |

## 驗收

每一段做完時都跑過 `cargo test --workspace`、`sanity.py deps`、`boundary`、`unsafe`、`docs`、`test.py wpt`，動到視窗或情境時另跑 `test.py scenarios`；第 1 到 4c 段也跑了 fmt 和 clippy（第 5 段起改成整個階段做完時才跑，`CLAUDE.md`）。第 4 段和 RFC 完成時跑 `test.py automation`（真的視窗）。

**事件的 interface 和 GlobalEventHandlers（第 1 段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| UI 事件的資料在事件上、legacy 的 `init*Event` 改得了但種類不變；`view` 和 `relatedTarget` 是 edge，經過 GC 還在 | `cargo test -p dom --test events a_ui_event`、`cargo test -p dom --test gc_stress` |
| 六個事件的建構子和成員、`which`、`getModifierState`、`init*Event`（分派中不改）、`pageX` 和 `offsetX`（測試的 `LayoutClient`）、`createEvent` 的名稱 | `cargo test -p dom_api --test ui_events`、`cargo test -p dom_api --test events create_event` |
| `GlobalEventHandlers`：Blink 的那一組（沒有 pointer 和 touch 的），在 `Element`、`Document`、`Window` 上只給 JS；`kun:dom` 的 `event-handler` 這一對；`<body>` 的 Window-reflecting handler；content attribute | `cargo test -p dom_api --test ui_events event_handler`、`cargo test -p dom_api --test events parsed_event`、`cargo test -p dom_js --test dispatch global_event` |
| JS：建構子的轉換（`view`、`relatedTarget`）、`createEvent` 的新名稱 | `cargo test -p dom_js --test bindings ui_event_constructors`、`cargo test -p dom_js --test bindings create_event` |
| 宣告表和規範的 IDL 比對（Pointer Lock、CSS 動畫和 transition、Selection API 的 IDL）；`kun:dom` 的 golden file（0.9.0，到第 4c 段的 0.10.0），具名的 handler 不在 WIT | `cargo test -p dom_api --test spec --test wit` |
| wasm：`event-handler` 這一對、不是 handler 的型別是代碼 7；W6（Rust）、W7（C）、W9（Lua，含 `InputEvent` 的 `option<string>` 欄位） | `cargo test -p dom_wasm --test engine`、`python3 scripts/test.py reftest wasm` |
| WPT：`uievents/` 不用 testdriver 的 8 個檔案（5 個全過，其他照 baseline），`dom/events/` 的 3 個變成全過 | `python3 scripts/test.py wpt uievents dom/events` |

**`click()` 和 activation（第 2 段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `click()`：合成的 click 的各項（不是 trusted、冒泡、可取消、composed、`view`、`detail` 1）、click in progress flag（重入什麼都不做、結束後再用）、沒有 window 的 document | `cargo test -p dom_api --test ui_events click` |
| activation 的三步和 default event handler 的順序：pre 在 listener 之前，沒取消時 activation、取消時 canceled（拿到 pre 留下的狀態），default event handler 只在沒取消時、只給 trusted 事件和 click、照 bubble 沿路徑到處理了為止 | `cargo test -p dom_api --lib activation` |
| 連結：`click()` 開在目前的分頁、`target=_blank` 不開；使用者中鍵的 `auxclick` 從連結裡的元素冒泡上來也開（只開最裡面的連結）、不冒泡時不開；中鍵的 `click` 只開一次 | `cargo test -p dom_api --test events a_click_follows` |
| JS：`click()` 只在 `HTMLElement` 上、重入 | `cargo test -p dom_js --test dispatch click` |
| wasm：`html-element.click` 是 `async func`，guest 自己的 listener 在裡面跑（W9） | `cargo test -p dom_api --test wit`、`python3 scripts/test.py reftest wasm/w9` |
| WPT：`click-in-progress-flag`、`html/editing/activation/click` 全過；`dom/events/` 的 7 個往前走 | `python3 scripts/test.py wpt dom/events click` |

**focus（第 3 段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom`：document 的 focused element 和元素的 focus 狀態（新的鏈、拿掉舊的，共有的祖先不記）、style flag 記下哪些狀態變了、移除時當場清掉、autofocus 的紀錄、只有樹裡的元素能有 focus；GC 的工作負載 | `cargo test -p dom --test focus`、`cargo test -p dom --test gc_stress` |
| style：只在有規則用到那個狀態時重算（自己的子樹、`:focus-within` 的祖先、跨兄弟的選擇器），沒有 system focus 時不變；`:focus`、`:focus-within`、`:focus-visible` 的比對 | `cargo test -p style --test invalidation focus` |
| 誰能 focus、事件的順序和 `relatedTarget`、捲進畫面和 `preventScroll`、`blur()`、system focus 的取得和失去、`:focus-visible`、listener 移走 focus、`tabIndex`、`activeElement` 的退路、focus fixup（移除時同步、不能 focus 時在 rendering update）、autofocus | `cargo test -p dom_api --test focus` |
| 整個瀏覽器：autofocus 在第一次 rendering update、`display: none` 不能 focus、fixup 在下一次 rendering update；子 frame 的元素拿到 focus（父的 `activeElement` 是 `<iframe>`、window 的 `blur` / `focus`、兩份都 `hasFocus()`） | `cargo test -p shell --test focus` |
| 網址列拿到按鍵時頁面失去 system focus，點回頁面拿回來 | `cargo test -p shell --test windowless the_address_field` |
| JS：focus 的成員在 `HTMLElement` 上、`tabIndex` 和 `autofocus` 反映 attribute | `cargo test -p dom_js --test dispatch focus_members` |
| WPT：`html/interaction/focus/` 不用 testdriver 的 43 個檔案和 `uievents/` 的兩個（9 個全過；其他多半等 `<input>`、`<button>`、`window.open`，四個會 timeout 的跳過） | `python3 scripts/test.py wpt html/interaction/focus` |

**按鍵（第 4a 段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 平台的按鍵換成 DOM 的 `key`、`code`、`keyCode`（照打出的字、數字鍵盤、具名的鍵、功能鍵）、放開的鍵不帶文字 | `cargo test -p shell --lib keyboard` |
| 按鍵經過 compositor 到 main thread（compositor 不再自己捲動） | `cargo test -p compositor --lib keys_go_to_the_main_thread` |
| `keydown`、`keypress`、`keyup` 送到 focused element（沒有時 body），trusted、composed；哪些鍵有 `keypress`（Enter 有、Ctrl 的捷徑沒有、取消的 `keydown` 沒有）、listener 移走 focus 後的 `keypress`；`keyCode` 和 `charCode` | `cargo test -p shell --test keys key_events` |
| Tab 和 Shift+Tab：正的 `tabindex` 先、0 和預設能 focus 的照樹的順序、負的跳過、沒有 box 的跳過、進出子 frame、到底了交還瀏覽器、帶著 ring；Ctrl+Tab 不動 | `cargo test -p dom_api --test focus tab_order`、`cargo test -p shell --test keys tab_goes` |
| 網址列和頁面之間的 Tab（`SetInitialFocus`、`TakeFocus`）、網址列的 Shift+Tab 到最後一個；key-up 送到收到 key-down 的地方 | `cargo test -p shell --test windowless tab_goes` |
| 捲動的鍵從 focused element 往外找能動的容器（巢狀的、子 frame 的），再到 viewport；取消 `keydown` 就不捲 | `cargo test -p shell --test keys keys_scroll` |
| Enter 點 focused 的連結（trusted、帶著修飾鍵：新分頁的鍵開新分頁）；在連結裡的元素、script 的 `keydown`、`keyup`、沒有 `href` 的 `<a>` 都不點 | `cargo test -p dom_api --test events a_click_follows`、`cargo test -p shell --test keys enter` |

**滑鼠（第 4b 段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 連點的次數：同一個鍵、間隔之內、2 px 之內才加一；macOS 讀系統的間隔 | `cargo test -p shell --lib mouse` |
| shell 送出的滑鼠事件帶著按著的鍵、修飾鍵、每個鍵的連點次數（放開的是按下的） | `cargo test -p shell --test browser_view the_mouse_carries` |
| 滑鼠事件的順序和欄位（trusted、composed、`view`、`button`、`buttons`、`detail`）；第二下才有 `dblclick`，中鍵沒有；`contextmenu` 照平台在按下或放開時；取消 `mousedown` 不擋 `click` | `cargo test -p shell --test mouse mouse_events` |
| 按下移動 focus（最近能 focus 的祖先、沒有 ring；沒有時給 document）；取消 `mousedown` 不移動；按鍵之後 script 的 focus 有 ring | `cargo test -p dom_api --test focus a_press`、`cargo test -p shell --test mouse a_press_moves` |
| `mousemove` 等下一個 frame、只留最後一個，`movementX` 從上一個滑鼠事件算；別的輸入來時先分派；隱藏的分頁馬上分派 | `cargo test -p shell --test mouse moves_wait`、`cargo test -p browser --test navigation a_hidden_tab_handles` |
| 子 frame 裡按下之後，放開之前的事件都給那個 frame（外面時是它的 `<html>`），放開之後回到滑鼠下 | `cargo test -p shell --test mouse a_press_in_a_frame` |
| 沒有 focus 時按鍵從最後按下的元素捲（只認 focused frame 裡的） | `cargo test -p shell --test mouse a_key_scrolls`、`cargo test -p shell --test mouse the_pressed_element` |
| 情境的 `click`（一律是第一下）和 `dblclick` | `cargo test -p shell --test windowless clicks_and`、`python3 scripts/test.py scenarios mouse` |

**滾輪（第 4c 段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom` 的 wheel listener：有沒有、是不是 blocking，版本號（加、減、event handler，別的事件和已經有的不算），DOM 的 default passive value | `cargo test -p dom --test events -- wheel_listeners the_default` |
| wheel event handler region：盒子的範圍、成為 layer 的容器裡的在它的空間、被不動的 clip 切、整份文件 | `cargo test -p paint --test hit_test the_wheel` |
| compositor：區域裡的滾輪 blocking 地交給 main，區域外自己捲、有 listener 時 non-blocking 地轉過去（捲不動的也是），沒有 listener 時不轉；只因為挑容器而交給 main 的不能取消 | `cargo test -p compositor --lib wheel` |
| listener 加減時只換區域、沿用同一批 layer；成為 layer 的容器裡的在它的 scroll node | `cargo test -p renderer --lib the_wheel_event_handler_region` |
| `wheel` 事件：區域裡可以取消、取消了不捲，區域外不能取消；passive by default；window 上的 blocking listener 是整份文件；`deltaY`、`deltaMode`、`wheelDelta`、修飾鍵；`mousewheel` 的退路（script 的事件沒有） | `cargo test -p shell --test wheel` |
| compositor 照畫出來的 document 捲過的滾輪，不分派給還沒畫出來的新 document | `cargo test -p browser --test navigation a_wheel_goes_to` |
| shell 送出的滾輪帶著按鍵和修飾鍵 | `cargo test -p shell --test browser_view the_mouse_carries` |
| 情境的滾輪 | `python3 scripts/test.py scenarios wheel` |

**`about:demo`、情境和 GC（第 5 段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `about:demo` 的輸入的一塊：點了拿到 focus 的方框攔下方向鍵（點移動、頁面不捲）、按鍵的紀錄、Shift+Tab 走過連結（`:focus` 變黃，截圖）、方框上的滾輪被取消 | `python3 scripts/test.py scenarios demo-input`、`python3 scripts/test.py automation` |
| 新的 GC edge（事件的 `view`、`relatedTarget`，document 的 focused element）在 GC stress 和 debug 的 V8 底下 | `cargo test -p dom --test gc_stress`、`python3 scripts/test.py matrix`、`fuzz` |

## 做過的檢查

2026-10-08 到 2026-10-09，macOS Apple Silicon。每一段的設計和更正寫進了 `README.md`；這裡是濃縮的紀錄。

- **WPT**：從 1606 個執行（第 1 段）到 1647 個（第 3 段起），每一段都 0 個不符。
  - 第 1 段：`uievents/` 不用 testdriver 的 8 個檔案（5 個全過，`CompositionEvent`、`TextEvent`、`pseudoTarget` 照 baseline）；`dom/events/` 的 3 個變成全過，`Event-subclasses-constructors` 等少了失敗。`css/cssom-view/mouseEvent` 照規範的那一項和 Chrome 一樣失敗（`pageX` 沒有 `view` 時不加捲動）；`scrollIntoView-smooth` 換了一個子測試 timeout（它看到 `onscrollend` 就等 kun 不發的 `scrollend`）。
  - 第 2 段：`click-in-progress-flag`、`html/editing/activation/click` 全過，`dom/events/` 的 7 個往前走（剩下的多半是表單，G）；`url/javascript-urls.window.js` 和 `window-name` 跳過（`<a>` 沒有 `href` 的 IDL attribute、沒有 `window.open`）。
  - 第 3 段：`html/interaction/focus/` 不用 testdriver 的 43 個檔案和 `uievents/` 的 2 個，9 個全過，其他多半等表單、`window.open`、`contenteditable`；等不到事件、會 timeout 的 4 個跳過。
  - 第 4c 段：`dom/events/passive-by-default` 的子測試全過；它對 `<html>` 和 `<body>` 用了一樣的名稱，harness 回報重複的名稱（全過時 Chrome 也是），baseline 改成這個。
- **Mutation test**：共 228 條（1：38、2：20、3：38、4a：54、4b：37、4c：40，第 5 段只動了 `about:demo` 和文件、沒有；補了測試的另外重跑），都抓到或說明了等價，每次跑完都確認檔案還原了。
  - 等價的，拿掉了多餘的程式：第 2 段 `<a>` 的 default event handler 檢查 trusted（分派只把 trusted 事件和 click 交給它）；第 4b 段 `mouse_focus` 的「已經有 focus 就不動」和只看 focused frame（focusing steps 本來就不動，換 frame 時舊的 focused element 一定已經清掉）。
  - 一開始沒抓到、補了測試之後抓到的：第 1 段 3 條（`offsetX` 的 target 是 document、`initKeyboardEvent` 的 `detail`、`<frameset>` 的 handler）；第 2 段 2 條（巢狀的連結、中鍵的 `click`）；第 3 段 1 條（`tabindex` 的負號）；第 4a 段 14 條（放開的鍵送到哪、網址列的 Shift+Tab、帶修飾鍵的預設動作、`keypress` 的規則、`composed`、巢狀和子 frame 的容器、沒有 `href` 的 `<a>`、ring）；第 4b 段 3 條（隱藏的分頁、中鍵的連點、按下的元素在 focused frame）；第 4c 段 5 條（`wheelDeltaX`、window 的 listener 在沒有盒子的地方、瞄準舊 document 的滾輪、只為挑容器的不能取消、shell 的修飾鍵）。第 4a 段的「按鍵之後 script 的 focus 有 ring」要等第 4b 段的滑鼠關掉 ring 才測得到，在 4b 補上、驗過。
  - 測不到、記進 `docs/test-gaps.md` 的「只靠審查」：macOS 讀系統的連按間隔（測試改不了系統設定）。`window.rs` 的連點計數只在真的視窗裡跑，由 `test.py automation` 的 `mouse.txt` 守住。
- **找到的 bug**：
  - **`<a>` 沒有 `href` 的 IDL attribute**（第 2 段，WPT 找到）：`anchor.href = ...` 只設了 expando；記進 `docs/limitations.md`。
  - **focus**（第 3 段，自己的測試找到）：`focus()` 沒檢查 document 有沒有 frame；blur 的 listener 移走 focus 之後整個停掉（照 Blink 照樣發 `focusout`，不帶 `relatedTarget`）；`FOCUS_WITHIN` 在新舊鏈共有的祖先上被記成變了。
  - **autofocus candidate 撐住 `DomIsolate`**（第 3 段，WPT 找到）：存 `NodeHandle` 時 renderer 拆掉會 abort，改成存 `NodeId`。
  - **數字鍵盤的 `keyCode`**（第 4a 段，mutation test 找到）：數字鍵盤的 7 是 55，Chrome 是 103（`VK_NUMPAD7`）。
  - **存著的 `mousemove` 沒分派**（第 4b 段，`browser` 的測試找到）：下一個事件瞄準舊 document 被丟掉時一起不見了，改成丟掉之前先分派。
- **改到的既有測試**：第 4b 段 `browser` 的 navigation 測試有 7 個假設「按下不要 frame、ack 立刻回來」；按下現在會跑 script、移動 focus，ack 等 frame。主執行緒忙碌的 3 個改成先送按下、等它的 ack，再送放開和滾輪，原本要測的順序照樣守住。第 4c 段 compositor 的一個預期改成只為挑容器而交給 main 的滾輪不能取消（Chrome 也是）。
- **`kun:dom`**：第 1 段 0.9.0（具名的 handler 函式換成 `event-handler` 這一對），第 2、3 段只新增（`html-element.click`、`focus` 等 `async func`），第 4c 段 0.10.0（`passive` 變成 `option<bool>`）。每次改 WIT 都用 `ci.py guests` 重編 guest；Lua 的產生器多了 record 的 `option<string>` 欄位、`async func` 的 record 參數傳值。
- **測試的寫法**：合併 `mousemove` 的測試讓 `mousedown` 的 listener 忙 100 ms，後面的移動就一定排在下一個 `BeginMainFrame` 之前（headless 一次送出所有輸入，否則誰先到不一定）；瞄準舊 document 的滾輪用 Viz 暫停、新的 document 已經 commit 時的時機（`browser` 的 `DrivenViz`）。
- **不穩定的測試**：沒有遇到。
- **第 5 段的 `about:demo`**：一開始用 `position: absolute` 和 `left` / `top` 移動方框裡的點，真的視窗的截圖裡點沒動：kun 還沒有 `position`（RFC 0013 是草稿），改用 `margin`。同一個原因，第 4c 段一個滾輪測試的頁面以為絕對定位的元素撐高了文件，其實沒有；測試要的「沒有盒子的地方」照樣成立（mutation 照樣抓到），改正頁面和說明。
- **RFC 完成時**（`docs/README.md` 的「一份 RFC 做完時」，2026-10-09，macOS Apple Silicon）：`test.py matrix` 8/8（每組 687 個測試）；`test.py fuzz` 20/20（seed `0x08458002`；從這份起照 `CLAUDE.md` 跑 20 組）；`test.py automation` 25/25（真的視窗，看過 `demo-input` 的截圖：第二個連結是 `:focus` 的黃色，方框裡的點在 2, 1）。最後一次全部的檢查：`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`（1504 個）、`sanity.py` 的四項、`test.py wpt`（1647 個執行、0 個不符、212 個跳過）、`reftest wasm`（9 個通過）、`scenarios`（50 次）都通過。文件的一致性交給一個唯讀的 agent 對過這份改過的檔案，找到的（`createEvent` 的名稱、`dom` 的 `Event` 的說明、兩份測試表少了 `focus`、shell 的 README 的滾輪和右鍵）都改了。
