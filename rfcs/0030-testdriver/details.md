# RFC 0030 的細節：testdriver

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## testdriver 怎麼走

2026-10-10 讀 WPT 的 checkout（`0f195499`）和 kun 的程式整理的。

1. **測試頁**：`/resources/testdriver.js` 提供 `test_driver.*`；wptserve 另外插入 `testdriver-vendor.js` 和 wptrunner 的 `testdriver-extra.js`（`tools/wptrunner/wptrunner/testdriver-extra.js`）。`test_driver.click(el)` 等先在頁面裡做檢查（`click` 會 `scrollIntoView`、用 `elementsFromPoint` 確認點得到，不然以 "element click intercepted" 拒絕），再把請求推進 `window.__wptrunner_message_queue`（元素用 selector 傳：跳脫過的 `#id`，或從 `:root` 起的 `*|localName:nth-child(n)` 路徑）。
2. **wptrunner**：executor 反覆用 Execute Async Script 執行 `testharness_webdriver_resume.js`，從佇列取出下一個請求；`actions.py` 的每個動作對應一個 WebDriver 指令（下表），經過 machopodriver 執行。machopodriver 沒有的指令回答 `unknown command`，wptrunner 的 `CallbackHandler` 把它變成失敗的回覆，測試的 promise 被拒絕，不會卡住。
3. **交回結果**：`executorwebdriver.py` 的 `send_message` 在目前的 browsing context（測試的 window）用 Execute Script 執行 `window.postMessage({"cmd_id": ..., "type": "testdriver-complete", "status": ..., "message": ...}, '*')`；`testdriver-extra.js` 用 `window.addEventListener("message", ...)` 收，讀 `event.data`，resolve 那個請求的 promise，再 `stopImmediatePropagation()`。
4. **從別的 window 或 iframe 發的請求**：先以 `test_driver.message_test({type: "testdriver-command"})` 傳給測試的 window，測試的 window 處理完用 `event.source.postMessage(data, "*")` 回覆。這要 `MessageEvent.source` 和同源的 `WindowProxy`。

wptrunner 的 WebDriver executor（testharness、reftest、crashtest）都是 `supports_testdriver = True`，kun 的 product 不用改。

**`test_driver.bless(intent, action)`**（WPT 的 `/resources/testdriver.js`）：在 body 加一個 `<button>`，`test_driver.click(button)`，等它的 `click` 事件之後執行 `action`。user activation 完全靠那次點擊的真實輸入產生，線上沒有專門的訊息。

### 動作和 WebDriver 指令

| testdriver | WebDriver 指令 | kun 現在 |
| --- | --- | --- |
| `click` | Find Elements（css）、Element Click | 有，卡在 `postMessage` |
| `bless` | 同 `click` | 同上，另外沒有 user activation |
| `send_keys` | Element Send Keys | `unknown command` |
| `action_sequence`（`Actions.send()`） | Perform Actions；同一個測試第二次起先 Release Actions | Perform Actions 沒有；Release Actions 是空的 |
| `set_window_rect`、`get_window_rect` | Set / Get Window Rect | 有 |
| `create_window` | New Window | 有 |
| `minimize_window` | Minimize Window | 沒有 |
| `get_all_cookies`、`get_named_cookie`、`delete_all_cookies` | cookie 的指令 | 沒有（kun 沒有 cookie） |
| `set_permission` | 擴充 `POST /session/{id}/permissions` | 沒有 |
| `generate_test_report` | 擴充 `POST /session/{id}/reporting/generate_test_report` | 沒有 |
| 其他（authenticator、sensor、FedCM、device posture、pressure、SPC…） | 各自的擴充 | 沒有 |

`actions.py` 共 52 個動作，`asyncactions.py` 另有 20 個 BiDi 的。

## WPT 裡的用量

2026-10-10，WPT 的 checkout 裡 `.html`、`.htm`、`.js` 檔（不含 `tools/`、`resources/`；各目錄底下的 `resources/` 的輔助檔還是算進去），用 ripgrep 數有出現的檔：

| 用法 | 檔數 |
| --- | --- |
| `test_driver.click(` | 376 |
| `test_driver.send_keys(` | 181 |
| `new test_driver.Actions` | 671 |
| `test_driver.bless(` | 380 |
| `test_driver.set_permission(` | 126 |
| 任何 `test_driver.` | 1756 |

用 `click`、`send_keys`、`bless`、`Actions` 其中之一的共 1503 個；依最上層的目錄：html 395、css 248、pointerevents 134、soft-navigation-heuristics 51、uievents 48、clipboard-apis 44、editing 41、dom 38、selection 29、infrastructure 29、fullscreen 28。

**kun 的清單裡**：83 個 `.ini` 因為 testdriver `disabled`（變體分開算是 107 節），都在 `dom/`：

| 目錄 | 檔數 |
| --- | --- |
| `dom/events/non-cancelable-when-passive/` | 40 |
| `dom/events/scrolling/` | 30 |
| `dom/events/`（最上層） | 7 |
| `dom/nodes/moveBefore/` | 5 |
| `dom/nodes/insertion-removing-steps/` | 1 |

另外 8 個因為等 `postMessage`、子 frame 的導覽等而 `disabled`（`preload/`、`the-location-interface/`、`the-script-element/`、`moveBefore/iframe-document-preserve.window.js`），做完第 1 段時重新看。`uievents/`、`html/interaction/focus/`、`the-input-element/`、`the-select-element/`、`textfieldselection/`、`the-textarea-element/`、`resetting-a-form/`、`the-button-element/` 和表單的 pseudo-class 挑測試時都排除了用 testdriver 的（`tests/wpt/include.txt` 的註解）。wdspec 的清單有 `element_click`、`release_actions`，沒有 `perform_actions`、`element_send_keys`、`element_clear`。

## kun 現在的輸入

- **machopodriver**（`crates/driver/src/session.rs`）：Element Click 先問 renderer 點擊的位置（`Kun.getElementClickPoint`：捲進畫面、第一個 client rect 的 in-view center 取 floor，或 `element not interactable`、`element click intercepted`），再送三個 `Input.dispatchMouseEvent`：`mouseMoved`、`mousePressed`（`left`、`buttons` 1、`clickCount` 1）、`mouseReleased`。Release Actions 不做事。其他沒有的指令回答 `unknown command`。
- **自動化訊息**（`crates/protocol/src/automation.rs`）：輸入只有 `Input.dispatchMouseEvent`（`mouseMoved`、`mousePressed`、`mouseReleased`；`button`、`buttons`、`clickCount`、`modifiers`）和情境用的 `Kun.dispatchWindowInput`（視窗層級的 `click`、`dblclick`、`wheel`、`key`）。`dispatchMouseEvent` 等 renderer 的 ack 才回答。
- **`InputEvent`**（`crates/protocol/src/input.rs`）：`MouseMove`、`MouseDown`、`MouseUp`、`MouseWheel`（像素或行）、`Key`（`down`、`key`、`code`、`key_code`、`text`、`location`、`repeat`、`modifiers`）。沒有 touch、pointer、IME。
- **renderer**：滑鼠分派 `mousemove`、`mousedown`（之後移動焦點）、`mouseup`、`click` / `auxclick`、`dblclick`、`contextmenu`，滾輪分派 `wheel`；鍵盤對焦點元素分派 `keydown`、`keyup`，會打字的鍵在 `keydown` 沒被取消時分派 `keypress`，預設動作只有 Tab 和捲動的鍵，Space、Enter 的 `keypress` 會點按鈕、checkbox、radio。沒有文字插入、`beforeinput`、編輯的 `input`。
- **序列化**：`dom_js` 的 `StructuredSerializeForStorage`（V8 的 `ValueSerializer`，host object 丟 `DataCloneError`），`history.state` 和 performance mark 的 `detail` 在用（`Base::Serialized`）。沒有 transfer。
- **user activation**：沒有。也沒有會用掉它的 API（`window.open`、`requestFullscreen` 等都沒有）。

## Actions 的演算法要點

WebDriver 的「Actions」一章（2026-10 的版本）：

- **輸入來源**：`none`、`key`、`pointer`（`pointerType`：`mouse`、`pen`、`touch`）、`wheel`；每個 id 第一次出現時建立狀態，之後同一個 id 的種類不能變（`invalid argument`）。
- **tick**：每個來源的第 n 個動作屬於第 n 個 tick；tick 裡依來源的順序分派，tick 的長度是其中最長的 `duration`，分派完等到 tick 結束。
- **key**：`keyDown` 加進按下的鍵、更新 modifier（Shift、Control、Alt、Meta）；`key` 用「normalised key value」把 `` 到 `` 的私用區字元換成 `Enter`、`Control`…，`code` 用「code for key」表，`location` 分左右；會打字的鍵送 `keyDown` 加 `text`。
- **pointer**：`pointerMove` 的目標座標 = `origin`（`viewport`、`pointer`、元素的 in-view center）加 `x`、`y`，不在 viewport 裡回答 `move target out of bounds`；有 `duration` 時在中間插入移動（間隔由實作決定）；`pointerDown`、`pointerUp` 更新按著的按鈕。
- **wheel**：`scroll` 在 `origin` 加 `x`、`y` 的位置捲 `deltaX`、`deltaY`，有 `duration` 時分段。
- **Release Actions**：照「input cancel list」的反序送出放開的事件（keyUp、pointerUp），清空狀態。
- **Element Send Keys**：元素要 keyboard-interactable；不是焦點時先聚焦並把 selection 放到值的最後；文字照字元（字形叢集）變成 keyDown、keyUp，`` 放開所有按著的鍵；`<input type=file>` 是設定檔案。

chromedriver（`chrome/test/chromedriver/window_commands.cc` 的 `ExecutePerformActions`）在 driver 裡做這些，每個事件送一個 `Input.dispatchMouseEvent`、`dispatchKeyEvent` 或 `dispatchTouchEvent`；kun 照做。它不照規範插入中間的移動：`pointerMove` 只送一個到終點的 `mouseMoved`，`scroll` 只送一個 wheel 事件，`duration` 只用在 tick 的長度，tick 分派完 `Sleep(tick_duration)`（2026-10-10 讀 `main` 上的原始碼）。Firefox（`remote/shared/webdriver/Actions.sys.mjs`）照規範在起點和終點之間線性插值，`fps60 = 17` ms 一次，滾輪也一樣。kun 照 chromedriver（`README.md` 的「已決定的事項」第 2 條）。

## Chromium 的 user activation

2026-10-10 讀 `main` 上的原始碼：

- **renderer**：每個 frame 一個 `blink::UserActivationState`（`third_party/blink/public/common/frame/user_activation_state.h`），transient 的期限 `kActivationLifespan` 是 5 秒（MSan 的 build 是 60 秒）。`LocalFrame::NotifyUserActivation` 在收到輸入時啟動，並經過 `LocalFrameHost::UpdateUserActivationState` 通知 browser。
- **browser**：`FrameTreeNode::NotifyUserActivation`（`content/browser/renderer_host/frame_tree_node.cc`）啟動自己和每個祖先的 `RenderFrameHostImpl`，再啟動整個 frame 樹裡和自己同源的 frame（「Same-origin Visibility」），經過 proxy 讓其他 process 的 frame 也知道；`ConsumeTransientUserActivation` 用掉整個 frame 樹的 transient activation。
- **browser 自己檢查的地方**（`render_frame_host_impl.cc`）：全螢幕（`EnterFullscreen` 用掉 activation 或 fullscreen token）、控制項的 popup（註解寫「we enforce on the browser side to guard against compromised renderers」）、字幕設定、開新視窗的 popup 擋截等。kun 現在都沒有這些功能，所以先不做 browser 的那一份（第 1 條）。

## 文字輸入的最小範圍

只在 `<input>`（會打字的 type）和 `<textarea>`，`keypress` 沒被取消之後：

| 按鍵 | 動作 | `beforeinput` 的 `inputType` |
| --- | --- | --- |
| 會打字的字元 | 以字元取代 selection | `insertText`（`data` 是字元） |
| Backspace | selection 是空的時刪前一個字元，不然刪 selection | `deleteContentBackward` |
| Delete | 刪後一個字元或 selection | `deleteContentForward` |
| Enter | `<textarea>` 插入換行 | `insertLineBreak` |
| ←、→、Home、End（加 Shift 時延伸） | 移動 selection | 沒有（不是編輯） |

`beforeinput` 可以取消；改了值之後更新 selection、分派 `input`（`InputEvent`，`inputType`、`data`）。`maxlength`、唯讀和 disabled 照規範擋住。不做 Ctrl+A（之後的 `Selection`）、undo、剪貼簿、IME、`<input>` 的 Enter 隱式送出（kun 還不送出表單）。
