# RFC 0021 的進度：WebDriver（classic 的子集）

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-08。完成：2026-10-09，Windows 11（Rust 1.99.0）。第 0 步和四段都在 `0021` 上做完，每一段做完時 commit、審核；最後 rebase 到 `main`（RFC 0017 的第 4 階段、RFC 0022 等 26 個 commit）之後一起 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 實測：`webdriver` crate、kun 的 wptrunner product | 完成 | 2026-10-08 |
| 1. 骨架：driver、`kun --automation-pipe`、自動化訊息的 host、session、導覽、`document.readyState` | 完成 | 2026-10-08 |
| 1b. 情境改用自動化訊息、`Kun` domain、`--automation-pipe --window` | 完成 | 2026-10-08 |
| 2. 執行 script 和元素 | 完成 | 2026-10-08 |
| 3. 分頁、frame 和視窗 | 完成 | 2026-10-09 |
| 4. 截圖和點擊、wptrunner 的冒煙測試 | 完成 | 2026-10-09 |

## 驗收

每一段做完時都跑過 fmt、clippy、`cargo test --workspace`、`sanity.py deps`、`boundary`、`unsafe`、`docs`；第 1 段起每段也跑 `test.py wdspec`、`scenarios`、`automation`（真的視窗，第 1b 段起連同 `pipe --window`），第 4 段起跑 `test.py smoke`。

**第 0 步的原型**

| 項目 | 怎麼驗證 |
| --- | --- |
| `webdriver` crate 用 kun 的 toolchain 能編 | 在 `rfcs/0021-webdriver/step0-prototype/`：`CARGO_TARGET_DIR=../../../target/step0 cargo build --release` |
| wptrunner 從 checkout 外面載入 kun 的 product、跑 wdspec，checkout 不變 | 照 `step0-prototype/run_wptrunner.py` 開頭的說明，在 venv 裡裝好 requirements 和 `step0-prototype/wptrunner_machopo`，跑 `webdriver/tests/classic/status` 和 `delete_session`；之後 `git -C ../third_party/wpt status --porcelain --ignored` 是空的 |

**網頁這一側（renderer、DOM）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `document.readyState`（loading、interactive 在 deferred script 之前、complete 在 `load` 之前，`readystatechange` 是 trusted、不 bubble；script 建的、iframe 的初始 `about:blank` 是 complete）、`navigator.webdriver`（automation 時 `true`，子 frame 也是）、main frame 的 `DOMContentLoaded` 告訴 browser（子 frame 的不告訴） | `cargo test -p browser --test navigation -- readiness navigator_webdriver dom_content_loaded` |
| `kun --automation-pipe` 的分頁從 `about:blank` 開始，是 initial entry | `cargo test -p browser --lib -- a_new_entry_takes_the_initial_entry_s_place`、`cargo test -p shell --test automation_pipe -- a_session_goes_through_the_history` |
| `screenX`、`screenY`、`screenLeft`、`screenTop`、`outerWidth`、`outerHeight` 問 window 自己的文件的 frame、不問 layout，沒有 frame 時是 0 | `cargo test -p dom_api --test geometry -- the_window_s_outer_size_is_its_frame_s` |
| `kun:dom` 的 WIT 多了 `document-ready-state`、`ready-state`、`webdriver`、`screen-x`、`screen-left`、`screen-y`、`screen-top`、`outer-width`、`outer-height`（只新增，不升版），guest 和 Lua 的 runtime 重新編過；spec 比對多讀 WPT 的 `webdriver.idl` | `cargo test -p dom_api --test wit --test spec` |

**自動化的訊息（`protocol::automation`、`kun --automation-pipe`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| JSON：CDP 的外框兩個方向、每個方法和事件、CDP 的錯誤碼，不收多的參數和屬性 | `cargo test -p protocol --test automation` |
| 分頁和 session、loader 和 lifecycle 的事件、history、reload、discovery 和 crash、錯誤、只送有要的事件、網頁的輸出不進 pipe、`Browser.close` 和 stdin 結束時結束 | `cargo test -p shell --test automation_pipe` |
| `kun` 的選項（`--automation-pipe`、`--window`、`--host-resolver-rules`、`--additional-trust-anchors`）和 help；`--window` 只在 `--automation-pipe` 時收 | `cargo test -p shell --bin kun` |
| `Kun` domain（網址列、等載入、點擊、按鍵和它的修飾鍵、滾輪、查元素、捲進畫面、截圖和 `now`、錯誤的輸入）；`Browser.getWindowForTarget`、`setWindowBounds`（只給一邊、`left`、`top`，別的視窗、不收的參數，`windowState` 是 -32602） | `cargo test -p shell --test automation_pipe -- kun_domain the_window_has_bounds_to_set` |
| `Kun.waitForLoad` 等目前的分頁最新的導覽，別的分頁的載入和 renderer 結束不算 | `cargo test -p shell --test windowless -- waiting_for_a_load` |
| `Target.createTarget`（在其他分頁之後、`background` 時在後面，`about:blank` 是 initial entry，沒有 `newWindow`）、`activateTarget`（顯示的分頁才畫：rAF，背景分頁的 `load` 等它顯示才到）、`closeTarget`（最後一個時 kun 結束）、錯誤 | `cargo test -p shell --test automation_pipe -- tabs_are_opened_shown_and_closed` |
| `outerWidth`、`outerHeight` 是視窗的大小（子 frame、改大小之後、renderer 當掉重開之後也是），`innerHeight` 比它小；沒有平台視窗時 bounds 是 CSS px | `cargo test -p shell --test automation_pipe -- the_outer_size_is_the_windows`、`cargo test -p shell --test windowless -- the_windows_bounds_are_css_px` |
| `Page.captureScreenshot`（網頁 view 的 PNG、physical px，script 剛改的照樣在；只有 PNG、沒有 `clip`；不是顯示的分頁是 -32000）、`Kun.captureElementScreenshot`（捲進畫面、在畫面裡的那部分、不在 x 0 的元素也裁對，沒有框是 `unable to capture screen`） | `cargo test -p shell --test automation_pipe -- pages_are_captured_as_png` |
| `Input.dispatchMouseEvent` 直接到分頁（網頁 view 的 CSS px、`buttons`、`clickCount`、修飾鍵；網頁收到 `mousemove`、`mousedown`、`mouseup`、`click` 和 `detail`；只有 kun 有的種類和按鈕，其他 -32602） | `cargo test -p shell --test automation_pipe -- mouse_events_go_straight_to_the_page` |

**WebDriver 在 renderer 裡的工作（`Kun.executeScript` 等）**

| 項目 | 怎麼驗證 |
| --- | --- |
| Execute Script 的值（基本型別、物件和陣列、集合、`arguments`、`toJSON`、循環、元素和 window 的參照、不是元素的節點）、promise 和 `await`、錯誤；網頁改掉的 `JSON`、`Array.prototype` 不影響 | `cargo test -p shell --test automation_pipe -- scripts_run_and_their_values_are_cloned` |
| Execute Async Script 由最後一個參數第一次被呼叫時 settle（晚一點呼叫、給它 promise、呼叫兩次），函式本體只在 settle 前 reject 時算數 | `cargo test -p shell --test automation_pipe -- async_scripts_settle_when_called_back` |
| 參數的反序列化（JSON、元素和 window 的參照）、`no such element`、`invalid argument`、`no such window`、`stale element reference`（參數和結果）；別的 renderer 的前綴、沒看過的文件是 `no such element` | `cargo test -p shell --test automation_pipe -- script_arguments_are_deserialized references_are_only_this_renderers` |
| 找元素（`css selector`、`tag name`，從文件或元素開始）、`invalid selector`、`unsupported operation`、`invalid argument`；元素的框（捲動之後也是文件的座標）、qualified name、attribute（boolean attribute）、property；導覽之後是 stale | `cargo test -p shell --test automation_pipe -- elements_are_found_and_read` |
| 沒有 session 時找不到方法、wasm 的文件是 `unsupported operation`；還沒 settle 的 script 在文件不見、renderer 結束（包括 `about:crash`）時回答錯誤 | `cargo test -p shell --test automation_pipe -- webdriver_methods_need_a_session_and_js a_script_left_waiting_is_answered` |
| `Kun.switchToFrame`（照 index、照 iframe、都沒給時是 frame 自己、`no such frame`、`invalid argument`、top-level 的號碼不是子 frame 的 id）；WebDriver 的方法在 `frame` 裡做；子 frame 的 window 是 frame 參照（結果和參數）；元素參照是它的 frame 的（別的 frame 是 `no such element`，iframe 移除後也是；同一個 frame 換了文件是 `stale element reference`）；frame 導覽後 id 不變，iframe 移除、上層換了文件之後是 `no such window` | `cargo test -p shell --test automation_pipe -- frames_are_switched_to_and_kept_through_their_navigations` |
| `Kun.getElementClickPoint`：捲進畫面（`block: end`）、第一個框在畫面裡那部分的中心、取整數（floor）、經過 iframe；在網頁的 viewport 裡但不在 frame 的 viewport 裡、沒有框是 `element not interactable`，被蓋住是 `element click intercepted`，點 iframe 本身不算被蓋住；跨兩行的連結取第一行 | `cargo test -p shell --test automation_pipe -- click_points` |

**machopodriver**

| 項目 | 怎麼驗證 |
| --- | --- |
| 對真的 kun：session 和 capabilities、導覽、上一頁、下一頁、重新整理、page load strategy 和逾時、`null` 的逾時、當掉的分頁、拒絕的請求和別的 session id；`Host`、`Origin` 的檢查 | `cargo test -p shell --test webdriver`、`cargo test -p driver` |
| script 和 script 的逾時、implicit wait、Find Element 找不到、元素的指令 | `cargo test -p shell --test webdriver -- scripts_run_until_the_script_timeout elements_are_found_after_an_implicit_wait` |
| New Window（分頁、在背景）、Switch To Window（顯示它、回到 top-level）、Close Window（最後一個結束 session）、Switch To Frame（index、元素、`null`）、Switch To Parent Frame（上層不在時 `no such window`）、導覽回到 top-level、Get / Set Window Rect（給了的部分才改，網頁的 `screenX`、`outerWidth` 跟著）、Navigate To 先看視窗再看網址、`setWindowRect: true` | `cargo test -p shell --test webdriver -- windows_are_tabs frames_are_switched_to_and_from the_window_rect_is_set` |
| Element Click（click 的 handler、被蓋住和不在畫面裡的錯誤和 400、子 frame 裡、連結的導覽照 page load strategy 等到載入完）、Take Screenshot、Take Element Screenshot（子 frame 裡的、沒有框是 500 的 `unable to capture screen`）、Release Actions（frame 不在時 `no such window`） | `cargo test -p shell --test webdriver -- elements_are_clicked screenshots_are_pngs frames_are_switched_to_and_from` |

**情境和真的視窗**

| 項目 | 怎麼驗證 |
| --- | --- |
| 情境經過自動化的 session（`Kun` domain）跑，全部通過；執行器自己的測試（等載入、2x 的 bounds、renderer 結束時等待中的命令和輸入失敗、結果先於 channel 的關閉、`dblclick`、`wait-title`） | `python3 scripts/test.py scenarios`、`cargo test -p shell --test windowless`、`cargo test -p shell --lib automation` |
| 同樣的情境在真的視窗裡（`kun --automation`，視窗那一層經過 session）全部通過，截圖和之前一樣 | `python3 scripts/test.py automation`，看 `target/automation-results/` |
| `kun --automation-pipe --window`：真的視窗經過 pipe 驅動（點擊、按鍵、截圖、改大小、`Browser.close` 結束），`Browser.getVersion` 的 `product` 和沒有平台視窗時不同 | `python3 scripts/test.py automation pipe`（設 `MACHOPO_WINDOW_TESTS`） |

**WPT**

| 項目 | 怎麼驗證 |
| --- | --- |
| wdspec：`tests/wpt/wdspec-include.txt` 的 34 個目錄和 `tests/wpt/metadata/` 一致，WPT 的 checkout 不變 | `python3 scripts/test.py wdspec` |
| wptrunner 的冒煙測試：上游的 wptrunner 用 kun 的 product，一個 testharness 測試和一個 reftest 都 PASS | `python3 scripts/test.py smoke` |

## 做過的檢查

2026-10-08 到 2026-10-09，Windows 11。每一段的設計和更正寫進了 `README.md`；這裡是濃縮的紀錄。

- **第 0 步**：只有原型，沒有 kun 的程式。結果和數字在 `details.md` 的「第 0 步的實測」。wdspec 的 `status` 和 `delete_session` 的 5 個 subtest，4 個 PASS（`test_accepted_beforeunload_prompt` 要 Navigate To，原型沒有）。第一次試時用了 `python -I`，它忽略 `PYTHONPYCACHEPREFIX`，bytecode 寫進了 WPT checkout 的 57 個 `__pycache__/`（都是 gitignore 的），確認都是這次產生的之後刪掉；改用 `PYTHONDONTWRITEBYTECODE=1`、不加 `-I` 之後，checkout 不再有任何新檔案。
- **wdspec**：每一段加上它的目錄，每次都 0 個 unexpected、已經在 `tests/wpt/metadata/` 的測試沒有一個變差。
  - 第 1 段：13 個目錄、688 個 subtest，430 個 PASS（`status`、`get_timeouts`，`new_session` 的大部分全過）。
  - 第 2 段：23 個目錄、1414 個 subtest，749 個 PASS（第 1 段的目錄多過了 4 個：`navigate_to/frame.py` 整份過了）。
  - 第 3 段：30 個目錄、1648 個 subtest，959 個 PASS（前兩段的目錄多過了 frame、別的視窗的元素、`closed_window` fixture 的測試）。
  - 第 4 段：34 個目錄、114 個檔、1827 個 subtest，1035 個 PASS。rebase 到 `main` 之後 `element_click/events.py` 和 `get_title/get.py` 多過了（RFC 0022 的 `mousedown` 等事件、RFC 0017 的 `document.title`）。
  - 沒過的每一段都逐一看過，都不在範圍或是 kun 其他地方的限制（`docs/limitations.md` 的「WebDriver 和自動化」）：user prompt、`link text` 和 `xpath`、shadow DOM、Get Element Text、Perform Actions 和 Send Keys、`screen`、最大化、最小化、全螢幕、最小的視窗大小、`type: "window"` 的 New Window、`<select>` 和 Is Element Selected、`type=file`、別的 frame 用 `target` 開的連結、`window.close()`、`visibility: hidden`、包著 block 的 inline 元素、`<input>` 沒有框（`closed_frame` fixture），以及 kun 的 DOM 還沒有的 API（`document.links`、`document.all`、`<a>.href`、`<input>.value`、`<select>.options`、XML 文件、`window.name`）。
  - 時間：預期結果都符合時約十分鐘；有預期之外的結果時 wptrunner 每次重開 runner（強制結束前等 10 秒），第 3 段第一次跑約 38 分鐘。另外有兩次跑的時候停了幾十分鐘（第 3 段第一次在 `switch_to_window/alerts.py` 之後約 21 分鐘，rebase 之後在 `find_elements/user_prompts.py` 約 62 分鐘），之後照常跑完、結果都符合預期：都是電腦在測試中途休眠，不是 machopodriver 或 kun 的問題。
- **冒煙測試**：`dom/nodes/Element-hasAttribute.html`（2/2）和 `css/CSS2/linebox/inline-box-001.xht`（PASS）；wptrunner 每個 testharness 測試開新分頁、點它的 `documentElement`，reftest 把 viewport 調成 800×600 再截圖，整條路都走過。第一次要產生自己的 manifest，約兩分鐘，之後約二十秒。
- **Mutation test**：共 159 條（1：30、1b：28、2：35、3：42、4：24；補了測試的另外重跑），都抓到或說明了等價，每次跑完都確認檔案還原了（第 3 段起也比對 `git diff` 的 hash）。
  - 等價的：driver 等 `load` 時不比對分頁（WebDriver 等的一定是顯示的分頁），記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。
  - 一開始沒抓到、補了測試之後抓到的：第 1 段 1 條（接受別的 session id：測試只在沒有 session 時送錯的 id）；第 1b 段 6 條（第一輪的 5 條，以及 `--window` 被忽略：兩者對 pipe 的回答完全一樣，照 Chrome 的 headless 讓 `Browser.getVersion` 的 `product` 分開之後抓到）；第 2 段 3 條（async script 每次呼叫都 settle、qualified name 丟掉 prefix、driver 不看 kun 的錯誤碼）；第 3 段 3 條（新分頁的 `about:blank` 不是 initial entry、Switch To Window 不清掉 frame、子 frame 裡的 script 一開始就當成文件不見）。
- **找到的 bug**：
  - **網頁的輸出在 stdout**（第 1 段，wdspec 找到）：`[uncaught] …` 夾在 pipe 的兩則訊息之間，下一則解析不了、被丟掉，`navigate_to` 的 `test_seen_nodes` 等到逾時。`console.log`、沒接住的例外、wasm guest 的輸出都改到 stderr。
  - **逾時**（第 1 段，wdspec 找到）：規範現在三個都可以是 `null`，原本只讓 `script` 是；`2.0` 這種整數的 float 被當成沒給。
  - **`windowless::drive` 的 race**（第 1b 段，mutation test 時看到）：執行器送出結果之後它的 channel 才關，`select!` 兩個都準備好時隨機選，選到關閉就回報「the scenario panicked」，約 80 次出現 2 次。改成 channel 關閉時先拿結果，`what_the_runner_came_to_wins_over_its_channel_closing` 每次都重現舊的行為；修好之後連跑 40 次沒有失敗。
  - **`about:crash` 時 script 一直等**（第 2 段）：browser 自己結束 renderer，沒有 `RendererGone`；改成分頁剛當掉時也讓等它的命令失敗。
  - **Get Element Tag Name**（第 2 段，wdspec 找到）：是 qualified name、不轉大寫，不是 `tagName`。
  - **Set Window Rect 只給一部分**、**Navigate To 先看網址**（第 3 段，wdspec 找到）：原本照較舊的規範，`width`、`height` 都給才改大小；關掉的視窗上給不合法的網址回答 `invalid argument`，規範要 `no such window`。
  - **點擊的位置**、**`unable to capture screen`**（第 4 段，wdspec 找到）：in-view center point 要取 floor、用第一個 client rect（跨兩行的連結）；`unable to capture screen` 是 500（`webdriver` crate 寫成 400，driver 自己蓋過）。
- **測試工具**：`run.py` 把測試的路徑接在 `--test-types`（收一串）後面，被它吃掉；Git Bash 把開頭是 `/` 的引數改成 Windows 的路徑（`test.py wdspec` 的 TEST 不加開頭的 `/`）；Windows 上 manifest 的 multiprocessing 重新 import `run` 模組（product 的 import 改到函式裡）；wptrunner 要 metadata 的目錄存在，沒有 `.ini` 時也建複本。`ci.py guests` 要 `wit-bindgen`，這台機器的不在 PATH，用 `WIT_BINDGEN` 指向 `../third_party/wit-bindgen-0.62.0/`。
- **改到的既有測試**：加 `serde_json` 到 `protocol` 讓 `browser` 的一個舊測試 `assert_eq!(viz.frames, [])` 推不出型別，改成 `is_empty()`。第 1b 段的一個測試原本用 callback 比間隔久的 `setInterval` 讓 main thread 忙，但 renderer 一次跑完所有到期的 timer、不再處理 input（`docs/limitations.md` 的「Script」），改用點擊時忙 1 秒的 listener。第 4 段點擊的測試頁原本用 `position` 蓋住元素，kun 還沒有，改用 negative margin 和後面的兄弟元素。
- **第 1b 段要量的截圖速度**（第 15 條）：在 process 裡每張截圖多 base64 的編碼和解碼，800×600 約 2 ms、1600×1200 約 9 ms（經過 pipe 再加 JSON 的序列化，約 4 ms、16 ms）；每張截圖本來就等一個 frame（16.7 ms），全部情境的時間看不出差別。
- **rebase 到 `main`**：每個 commit 的衝突都解了，產生的檔案（`machopo-dom.wit`、wasm guest、Lua 的 runtime）在每個 commit 照合併後的原始碼重新產生。`main` 在舊的情境 `Request` 上加的 `dblclick` 和 `wait-title` 移到自動化訊息上（`Kun.dispatchWindowInput` 的 `type: "dblclick"`、新的 `Kun.getTitle`）；RFC 0022 讓 `InputEvent` 多了 `buttons`、`click_count`，`Input.dispatchMouseEvent` 跟著收 CDP 的 `buttons`、`clickCount`，machopodriver 照 chromedriver 送。
- **不穩定的測試**：除了上面 `windowless::drive` 的 race（已修），沒有遇到。
- **RFC 完成時**（`docs/README.md` 的「一份 RFC 做完時」，2026-10-09，Windows 11）：這份 RFC 沒動到 GC（`dom` 只多了 document 的 readiness 的狀態、把 `NodeId::from_raw` 改成公開），不跑 `matrix`、`fuzz`。文件的一致性交給一個唯讀的 agent 對過這份改過的檔案，找到的都改了：CDP 的錯誤碼、Delete Session 的步驟、script 的逾時由 driver 計、async 函式的本體、點擊的位置是第一個 client rect 取 floor、`invalid argument` 和 `unsupported operation` 的分別、`docs/testing.md` 的測試範圍和 `smoke`。它也找到 `docs/testing.md` 的一條「等價的 mutation」（renderer 結束時不清掉等輸入被處理的請求）已經不等價：`Input.dispatchMouseEvent` 也在等它，driver 的 Element Click 會一直等。`a_renderer_gone_fails_what_waits_for_it` 補了等 ack 的按下，這個 mutation 抓到了，那一條拿掉。最後一次全部的檢查：`cargo fmt --check`、clippy（`-D warnings`）、`cargo test --workspace`（1590 個，142 個 test binary；`-j 4`）、`sanity.py` 的四項、`test.py scenarios`（50 次）、`automation`（25 份和 `pipe --window`）、`wdspec`（0 個 unexpected）、`smoke`（兩個都 PASS）都通過，WPT 的 checkout 不變。
