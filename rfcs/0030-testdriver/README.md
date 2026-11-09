# RFC 0030：testdriver（`postMessage`、Actions、Send Keys、user activation）

- 開始日期：2026-10-10
- 狀態：✅ 已完成：2026-10-10（「已決定的事項」第 1 到 5 條開始前接受，第 6 到 33 條在各段開始前和實作時定下、四段做完時一起審核，第 34 條是完成之後補的（2026-10-11）；只在 Windows 11 上做；動到 GC 而要跑的 `test.py matrix`、`fuzz` 之後在 macOS 上跑過、都通過；驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`

## 摘要

讓 WPT 裡用 `testdriver.js` 的測試能在 kun 上跑：`test_driver.click`、`bless`、`send_keys`、`Actions`，以及靠它們的 user activation。testdriver 的實作本來就在上游：測試頁把請求放進 wptrunner 的佇列，wptrunner 用 WebDriver 叫 machopodriver 執行，再在測試頁執行 `window.postMessage` 交回結果（`details.md` 的「testdriver 怎麼走」）。kun 這一頭補了四樣，分四段做：頁面的 `window.postMessage` 和 `MessageEvent`（同一個 renderer 裡同源的 window 之間）；HTML 的 user activation 和 `navigator.userActivation`；machopodriver 的 Perform Actions、Release Actions（照規範在 driver 裡算，像 chromedriver，每個 top-level 視窗一份輸入狀態），和自動化訊息的鍵盤、滾輪；Element Send Keys、Element Clear，和它們要的文字控制項的最小編輯（`beforeinput`、`textInput`、`input`、`change`）。WPT 裡為了 testdriver 停用的測試大多能跑了，`uievents/`、焦點、user activation、表單裡用 testdriver 的測試挑能整個通過的加進清單；wdspec 多了 `perform_actions`、`element_send_keys`、`element_clear`。

## 動機

**開始時的現況**（2026-10-10）：WPT 改由上游的 wptrunner 跑之後，testdriver 的請求已經會經過 machopodriver，卡在 kun：

- **結果交不回去**：wptrunner 在測試頁執行 `window.postMessage({type: "testdriver-complete", ...}, "*")`，kun 沒有 `postMessage`，Execute Script 回報 JavaScript 錯誤，測試變成 ERROR。連 machopodriver 本來就有的 Element Click（`test_driver.click`、`bless`）也走不通；上一次跑 `test.py wpt` 有 8 個測試是這樣（4 個是 crashtest）。
- **指令沒有**：Perform Actions、Element Send Keys、Element Clear 回答 `unknown command`（`docs/limitations.md` 的「WebDriver 和自動化」）。wptrunner 把它轉成請求失敗，測試的 promise 被拒絕，不會卡住。
- **打字進不了控制項**：按鍵只到網頁的 `keydown`、`keypress`、`keyup`，`<input>`、`<textarea>` 的值不會變，沒有 `beforeinput`、`input`（當時 `docs/limitations.md` 的「捲動」）。
- **沒有 user activation**：沒有 `navigator.userActivation`，也沒有 transient、sticky activation 的狀態（`docs/limitations.md` 的「Bindings 和 DOM API」）；`bless` 的測試多半在檢查它。

**影響**：WPT 裡用 testdriver 的測試檔約 1750 個，其中用 `click`、`send_keys`、`bless`、`Actions` 的約 1500 個（html 395、css 248、pointerevents 134、uievents 48、dom 38…，`details.md` 的「WPT 裡的用量」）。我們的清單裡因此 `disabled` 的有 83 個檔（都在 `dom/`：`dom/events/non-cancelable-when-passive/` 40 個、`dom/events/scrolling/` 30 個…），`uievents/`、`html/interaction/focus/`、表單的目錄挑測試時也都把用 testdriver 的排除了（`tests/wpt/include.txt` 的註解）。

**目標**：

- `window.postMessage` 和 `MessageEvent`，夠 testdriver 用，也是一般網頁的同源 window 之間的訊息。
- machopodriver 的 Perform Actions（`key`、`pointer` 的 `mouse`、`wheel`、`none`）、Release Actions（照輸入來源的狀態放開）、Element Send Keys、Element Clear。
- 自動化訊息加上鍵盤（`Input.dispatchKeyEvent`）和滾輪（`Input.dispatchMouseEvent` 的 `mouseWheel`）。
- `<input>`、`<textarea>` 的最小文字輸入：在 selection 插入和刪除字元、移動 selection，`beforeinput` 和 `input`。
- HTML 的 user activation（transient、sticky、在 frame 之間的傳遞），`navigator.userActivation`。
- 拿掉那 83 個 `disabled`，把 `uievents/`、`html/user-activation/` 等目錄裡現在能跑的加進清單；wdspec 加上 `perform_actions`、`element_send_keys`、`element_clear`。

**要解決的限制**（`docs/limitations.md`）：「WebDriver 和自動化」的「沒有 Perform Actions、Element Send Keys、Element Clear」；「捲動」的「沒有 `beforeinput` / `input`」（只有文字控制項的部分）；「Bindings 和 DOM API」的「沒有 `userActivation`」；「事件」的 event handler 沒有 `onmessage`。

**不在範圍內**：

- **Pointer Events、touch、pen、拖放**：kun 沒有 `PointerEvent`、`TouchEvent`。Perform Actions 的 `pointerType` 是 `touch` 或 `pen` 時回答 `unsupported operation`。`pointerevents/` 的測試等之後的 Pointer Events。
- **`mouseover`、`mouseout`、`mouseenter`、`mouseleave`、`:hover`**：滑鼠移動照舊只有 `mousemove`（`docs/limitations.md` 的「事件」）。
- **`contenteditable`、`Selection`、IME 和 composition、游標和控制項的繪製**：路線圖的 P（編輯）和表單的第二段。文字控制項現在畫成 0×0，Element Click 點不到它們（Element Send Keys 只要能取得焦點，不受影響）。
- **`MessageChannel`、`MessagePort`、transfer、`structuredClone()`、跨源 window 的 `postMessage`**：路線圖的 J（第 3 條）。
- **testdriver 其他的動作**：`set_permission`、`generate_test_report`、虛擬的 authenticator、sensor 等 WebDriver 的擴充，cookie，最小化，BiDi 才有的部分。它們照舊回答 `unknown command`，測試的 promise 被拒絕。
- **wasm 文件**：wptrunner 跑不了 wasm 文件（要在頁面裡執行 script），`postMessage` 先只有 JS 的；wasm 的 guest 之後跟著宣告表拿到。

## 設計概要

**給寫測試、跑 WPT 的人**：清單裡用到 `test_driver.click`、`bless`、`send_keys`、`new test_driver.Actions()` 的測試照常跑，不用另外設定：

```js
// testdriver 的典型用法，在 kun 上可以跑
await test_driver.bless("open a popup");              // 點一個按鈕，給 transient activation
assert_true(navigator.userActivation.isActive);

await test_driver.send_keys(input, "abc");            // 聚焦、逐字 keydown / keypress / beforeinput / textInput / input / keyup
assert_equals(input.value, "abc");

await new test_driver.Actions()
  .pointerMove(0, 0, {origin: target})
  .pointerDown().pointerUp()
  .keyDown("\uE008").keyDown("a").keyUp("a").keyUp("\uE008")   // Shift+A
  .scroll(0, 0, 0, 100, {origin: scroller})
  .send();
```

**給網頁的作者**：`window.postMessage(data, "*")` 和 `addEventListener("message", ...)` 在同一個 window、子 frame、parent 之間可以用（同源）；`navigator.userActivation.isActive` 和 `hasBeenActive` 跟著真的使用者輸入變。在 `<textarea>` 和文字類的 `<input>` 打字會分派 `beforeinput`、`textInput`、`input` 並改值，Backspace、Delete 分派 `beforeinput`、`input`，←、→、Home、End 移動 selection（不分派這些事件）；焦點離開時分派 `change`（還畫不出游標和控制項）。

**給改 machopodriver 的人**：Perform Actions 照規範在 driver 裡算：driver 記每個視窗的輸入狀態（輸入來源、按著的鍵和按鈕、指標的位置），一個 tick 一個 tick 送出 `Input.dispatchMouseEvent`、`Input.dispatchKeyEvent`，像 chromedriver（`crates/driver/src/actions.rs`）。

## 詳細設計

### `postMessage` 和 `MessageEvent`

- `Window.postMessage(message, targetOrigin)` 和 `postMessage(message, options)` 宣告成一個：`postMessage(message: any, options: (DOMString or WindowPostMessageOptions))`，`WindowPostMessageOptions` 只有 `targetOrigin`（預設 `"/"`）。沒有 transfer（J）：第一種寫法的第三個參數被忽略，不合法的值也不丟 `TypeError`。JS 的 `undefined`、`null` 照 union 裡的字典處理，所以 `postMessage(message)` 是 `"/"`。
- 步驟照 HTML 的「window post message steps」（第 6 條）：先解析 `targetOrigin`（`"*"` 是任何；`"/"` 是呼叫的一方的 origin；其他照 URL 解析，失敗是 `SyntaxError`，opaque 的 origin 不會是任何文件的），再呼叫 `ScriptHost::serialize`（`dom_js` 用現有的 `StructuredSerializeForStorage`，V8 的 `ValueSerializer`，`history.state` 已經在用；不能序列化的丟 `DataCloneError`）。`origin` 和 `source` 取自 `ScriptHost::incumbent_window()`（第 7 條）。Chromium 也是 V8 的序列化（`SerializedScriptValue`）。
- 在目標 window 的文件排一個 DOM task（`TaskSource::PostedMessage`，`dom_api` 的 `Task::PostMessage`）：task 執行時目標文件的 origin 不是 `targetOrigin` 就默默丟掉（規範），不然分派 trusted 的 `message`（不 bubble、不可取消）。文件在 task 執行前被丟棄時，task 跟著丟掉。
- `MessageEvent`（`dom` 的 `EventKind::Message`）：`data` 是建構子、`initMessageEvent` 給的值（`ValueSlot`），或是 posted message 的 bytes，第一次讀時在 event 的 realm 反序列化、之後都是同一個物件（第 8 條；cache 在 event 上，所以別的 realm 先讀也是目標 realm 的物件）；`origin`、`lastEventId`（空字串）和 posted message 在 `Event` 最外層的 `#[no_trace]` 欄位（`initMessageEvent` 會改）；`source` 是 `Edge<Window>`（第 9 條）。沒有 `ports`（J）。`createEvent("MessageEvent")` 也有。
- `onmessage`、`onmessageerror` 是 window 的 handler，`<body>` 的也是 window 的（`WindowEventHandlers`）。`messageerror` 在 kun 不會發生（第 8 條）。
- `data` 是 `any`，所以 `postMessage`、`MessageEvent.data` 和 `initMessageEvent` 的 `data` 不進 WIT（wasm 的 guest 拿到 `MessageEvent` 的其他成員）；wasm 的 guest 讀寫 `any` 是路線圖的 U。
- 目標：同一個 renderer 裡的 window（自己、子 frame、`parent`、`top`）。跨源的 window 照舊擋住全部成員（`docs/limitations.md` 的「iframe」），之後在 J 開放 `postMessage` 等規範允許的成員。
- 對應 Chromium：`DOMWindow::postMessage`、`PostMessageHelper`，`LocalDOMWindow::SchedulePostMessage`，`MessageEvent`。

### 自動化訊息的鍵盤和滾輪

- `Input.dispatchKeyEvent`（CDP 的名字和欄位）：`type`（`keyDown`、`keyUp`、`rawKeyDown`；沒有 `char`：CDP 的 `keyDown` 就是 Chromium 的 `RawKeyDown` 加上 `Char`，kun 的按鍵本來就是這樣）、`key`、`code`、`text`、`modifiers`、`location`、`autoRepeat`、`windowsVirtualKeyCode`。送到分頁的 `WebContents`（和 Element Click 一樣不經過視窗，RFC 0021 的第 27 條；瀏覽器的快捷鍵也不攔），renderer 收到的是現有的 `InputEvent::Key`；`rawKeyDown` 的 `text` 丟掉。
- `Input.dispatchMouseEvent` 加上 `mouseWheel`（`deltaX`、`deltaY`，像素），成為 `InputEvent::MouseWheel`，經過 compositor 的 blocking listener 判斷和捲動，和真的滾輪一樣。
- 回答都等 renderer 的 ack（現有的 `dispatch_page_later`），所以下一個 tick 送出前，上一個事件的 listener 已經跑完。滑鼠移動在下一個 main frame 才分派（Chromium 的 rAF-aligned input），renderer 原本收到就 ack，改成分派之後才 ack（第 21 條），像 Chromium 的 `MainThreadEventQueue`。等 ack 的時間會拉開兩次按下，所以連點的 500 ms 用動作自己的時間算（第 34 條）。
- `Kun.getElementClickPoint` 加 `inViewCenterOnly`（第 15 條），`Kun.getFrameViewport` 回答一個 frame 的 viewport 在 top-level 裡的位置和大小（第 19 條）；WebDriver 的錯誤多了 `move target out of bounds`。

### Perform Actions 和 Release Actions

- machopodriver（`crates/driver/src/actions.rs`）照 WebDriver 的「Actions」一章：先檢查整個請求、建立它提到的輸入來源、按 tick 分組（`process`），再每個 tick 依序分派（`perform`），tick 的時間是其中最長的 `duration`（`pause`、`pointerMove`、`scroll`）。輸入狀態（輸入來源、按著的鍵和按鈕、指標的位置、input cancel list）每個視窗一份（第 16 條），以 window handle 為 key 存在 session 上，關掉視窗時丟掉；Release Actions 照 input cancel list 的反序送出放開的事件，再丟掉那個視窗的狀態。
- `key`：每個鍵的 `key`、`code`、`location` 照規範的「normalised key value」、「code」和「key location」表，`windowsVirtualKeyCode` 照美式鍵盤（第 13 條）；會打字的鍵送 `keyDown` 加上 `text`（Enter 是 `\r`），其他送 `rawKeyDown`；modifier 鍵更新那個來源的 modifiers，所有鍵盤來源的 modifiers 一起套到之後的鍵盤、滑鼠和滾輪事件。
- `pointer`（`mouse`）：`pointerMove` 的 `origin` 是 `viewport`、`pointer` 或元素（元素的 in-view center point，第 15 條），只送一個移到終點的事件，`duration` 只決定 tick 的長度（第 2 條）；`pointerDown`、`pointerUp` 帶按鈕和 `buttons`（所有指標按著的），`clickCount` 照兩次按下的時間和距離算（第 17、20 條）。座標是目前的 browsing context 的 viewport 的，送出時加上它在 top-level 裡的位置（第 19 條）；超出 viewport 回答 `move target out of bounds`。`pointerType` 是 `touch`、`pen`，或按鈕是 3、4 時回答 `unsupported operation`（第 14 條）。
- `wheel`：`scroll` 的 `origin`（`viewport` 或元素）、`x`、`y`、`deltaX`、`deltaY`，送一個 `mouseWheel`，`duration` 一樣只決定 tick 的長度（第 2 條）。
- `none`：只有 `pause`。
- 規範的整數是值為整數的 Number：wptrunner 的 `duration` 是 float 的倍數乘出來的（`100.0`），`webdriver` crate 不收；machopodriver 先把 Perform Actions 的 body 裡這種數字寫成整數（第 18 條）。crate 也不檢查最大安全整數，`actions` 自己檢查。
- 對應 chromedriver 的 `ExecutePerformActions`：它也在 driver 裡算 tick，送 `Input.dispatch*`。

### Element Send Keys、Element Clear 和文字輸入

- Element Send Keys（第 22、23 條）：renderer 的 `Kun.prepareSendKeys` 把元素捲進畫面，不是 keyboard-interactable（能取得焦點、`<body>` 或 `<html>`）就是 `element not interactable`（driver 在 implicit wait 內再問）；沒有焦點時跑 focusing steps，文字控制項的游標放到值的最後（有焦點時不動，照規範）。不可輸入的控制項（color、日期時間那幾種、range）照規範直接設值；`<input type=file>` 是 `unsupported operation`（kun 沒有檔案）。其他的由 machopodriver 送按鍵：照規範的「dispatch actions for a string」，一個暫時的鍵盤來源加進那個視窗的輸入狀態，一個 grapheme cluster 一對按鍵，美式鍵盤要 Shift 的字自動按放 Shift，modifier 按著到 `\uE000`（NULL）或結束，用完拿掉那個來源。
- Element Clear：renderer 的 `Kun.clearElement`。不是可以編輯的控制項（唯讀、disabled、按鈕這類，kun 沒有 `contenteditable`）是 `invalid element state`；捲進畫面後「在畫面裡」的檢查見第 29 條；照規範的「clear a resettable element」：空的不動（kun 沒有 constraint validation），不然聚焦、清空值（input 照 IDL 設成空字串再清理，`<textarea>` 的值設成空的）、分派 `input` 和 `change`、blur，使用者之前的改變不再 commit。
- 文字控制項的編輯（`dom_api` 的 `editing`，第 24 到 27 條）：`keypress` 沒被取消之後 `type_text`：`beforeinput`（`insertText` 或 `<textarea>` 的 Enter 的 `insertLineBreak`）、`textInput`（`TextEvent`，Enter 是 `\n`），兩個都能取消，再以字取代 selection、游標放到後面、分派 `input`；`maxlength` 只放得下的部分（以 grapheme cluster 切），一個都放不下時什麼都不做；`<input>` 的 Enter commit 它的改變。`keydown` 沒被取消之後 `edit_key`：Backspace、Delete 刪 selection 或游標前後的 grapheme cluster（`deleteContentBackward`、`deleteContentForward`），←、→、Home、End 移動游標，加 Shift 時延伸選取（排 `select`）。唯讀、disabled 的不改值、游標照樣移動。控制項接手的鍵不再做別的事（Space、←、→ 不捲頁）；Ctrl、Alt、Cmd 按著時不編輯。
- `change`（第 25 條）：`dom` 的表單狀態多了「上次 commit 之後、使用者第一次改之前的值」（`value_before_edit`，Blink 的 `text_as_of_last_form_control_change_event_`）；commit 時和現在的值不同就分派 `change`。focus 的更新步驟在 `blur` 之前 commit（HTML）。
- 這段也讓真的使用者在視窗裡打字有用（雖然還看不到控制項）；情境的 Enter 和平台一樣帶 `\r`（第 33 條）。

### User activation

- 照 HTML 的「User activation」（第 10 到 12 條）：每個 `Window`（`dom`）記「last activation timestamp」，`#[no_trace]` 的 `Cell<f64>`，沒有過是 +∞；kun 一份文件一個 window，換文件自然重設。Chromium 的 `UserActivationState` 在 `Frame` 上、導覽時清掉，行為一樣。
- **notify**：renderer 在分派 trusted 的 `mousedown`（任何按鍵）和 Escape 以外的 `keydown` 之前，呼叫 `dom_api::user_activation::notify_activation`（對應 `LocalFrame::NotifyUserActivation`）：事件目標的文件的 window、所有的祖先 frame（不管 origin）和同源的子孫 frame（`FrameTreeNode::NotifyUserActivation` 的「Same-origin Visibility」）記下現在的時間。瀏覽器自己的快捷鍵不會到 renderer；WebDriver 的 Element Click（`test_driver.click`、`bless`）走同一條輸入；script 的 `click()`、`dispatchEvent` 不經過這裡。之後有 Pointer Events、touch 時加 `pointerdown`、`touchend`。
- **查詢**：sticky 是時間不是 +∞；transient 是現在在那之後、不到 5 秒（`TRANSIENT_ACTIVATION_DURATION`，Blink 的 `kActivationLifespan`）。時間用文件的 frame 的時鐘（event loop 的時鐘，script 執行時也會走），沒有 frame 時用 event loop 的時間。
- `navigator.userActivation`：`[SameObject]` 的 `UserActivation`（`hasBeenActive`、`isActive`），`dom` 的新 GC 型別，指向它的 window、由 `Navigator` 保存（和 `Navigator` 由 window 保存一樣）；兩個成員是 `bool`，WIT 也有。
- **consume 先不做**（第 11 條）：kun 還沒有會用掉 activation 的 API（`window.open`、`requestFullscreen`、控制項的 popup），第一個來的時候照規範做整棵 frame 樹的 consume。
- **狀態只在 renderer 裡**（第 1 條）：Chromium 在 browser 也有一份（`FrameTreeNode`、`RenderFrameHostImpl`），給跨 process 的 frame 和 browser 自己的檢查；kun 先不做，什麼時候要補記在 `docs/limitations.md`。notify 和之後的 consume 各只有一個入口，補的時候改動集中在那裡。

### 測試

- **WPT**：為了 testdriver 停用的 84 個檔（開始時的 83 個，第 1 段處理了只靠 `click` 的 3 個、又停用了 4 個要 Actions 的 crashtest）裡 80 個拿掉 `disabled`、照實寫預期結果（還等到逾時的照舊停用，寫明真正的原因）；`webmessaging/`、`moving-between-documents/`、`html/user-activation/`、`uievents/`、`html/interaction/focus/`、表單目錄裡用 testdriver 的，照「目錄裡大部分能跑才整個加」的規則挑（每一段開始前定候選，第 5、28 條）。wdspec 加上 `perform_actions`、`element_send_keys`、`element_clear`。
- **kun 自己的**：`dom_js` 的 `messaging`、`user_activation`、`editing`（真的 V8、真的 iframe，renderer 的呼叫直接做）；`shell` 的 `messaging`（整個瀏覽器）；`automation_pipe` 的鍵盤、滾輪、元素 origin 和 frame 的 viewport；`webdriver` 的 Actions、每個視窗的輸入狀態、Send Keys、Clear；`driver` 的單元測試（鍵表、tick、連點、座標、Send Keys 的按鍵，假的視窗和時鐘）；`browser` 的 navigation（移動的 ack）；情境 `user-activation` 和 `typing`（真的輸入）。
- 每一段跑 mutation test（`mutations-stage1.toml` 到 `mutations-stage4.toml`，結果在 `progress.md`）。

### 分階段

1. **`postMessage` 和 `MessageEvent`**（完成）：`test_driver.click`、`bless` 通了；`webmessaging/`、`moving-between-documents/`。
2. **User activation**（完成）：`navigator.userActivation`、`html/user-activation/` 只靠點擊的。
3. **Perform Actions、Release Actions**（完成）：自動化訊息的鍵盤和滾輪；`dom/events/` 的捲動和滾輪、`uievents/` 的滑鼠、wdspec 的 `perform_actions`。
4. **Element Send Keys、Element Clear 和文字輸入**（完成）：`uievents/textInput/`、焦點的 Tab、表單的 `send_keys`、wdspec 的兩個目錄。

## 缺點

- **範圍大**：一份裡有 DOM（`postMessage`、user activation、文字輸入）、自動化訊息和 machopodriver 三層；分段做，每段都能單獨 merge。
- **`postMessage` 分成兩份做**：這份只有 window 之間、同源、沒有 transfer；`MessageChannel` 和跨源的成員留給 J（第 3 條），J 要接在這份的序列化和 `MessageEvent` 上。
- **文字輸入是最小的**：沒有游標、IME 和 composition、undo、剪貼簿、以字為單位的移動，↑、↓ 不處理，email、number 和日期時間的 `<input>` 打不進字，控制項還是 0×0；之後的編輯和表單的繪製要接在這上面，或改寫它。WPT 裡很多鍵盤的測試先點 `<input>`，在控制項有大小之前照舊失敗。
- **和規範、Chromium 不同的地方**：`pointerMove` 和 `scroll` 不插中間的事件（和 Chrome 一樣，和規範不同，第 2 條）；user activation 只在 renderer（第 1 條）；輸入狀態每個視窗一份、Send Keys 的 modifier 不留到下一次（和規範一樣，和 chromedriver 不同，第 16、23 條）；Element Clear 不做 hit test（第 29 條）。都記進 `docs/limitations.md`。
- **Pointer Events 還沒有**：`pointerevents/`（134 個用 testdriver 的檔）和很多 `uievents/` 的測試照舊跑不了。

## 理由和其他做法

完整的比較在 `details.md`。

- **為什麼 Actions 在 driver 裡算（不另外加一個 `Kun.performActions`）**：chromedriver 就是這樣，規範的演算法和輸入來源的狀態是 WebDriver 的東西；kun 只要提供和真的輸入一樣的入口（`Input.dispatch*`），之後外部的 CDP 工具也能用。代價是每個事件一個來回（本機的 pipe，約 1 ms）。
- **為什麼 Send Keys 要真的改值，不只送按鍵**：只送按鍵的話，181 個用 `send_keys` 的測試幾乎都在檢查值，照樣失敗；直接設 `value`（不經過按鍵）不是規範的行為，`beforeinput` 和 `input` 也不會發生。
- **為什麼 `postMessage` 不等 J**：J 是 DOM 線的工作，還沒排程；testdriver 只要 window 的 `postMessage`。先做的這一部分正好是 J 的一部分，J 接著做 `MessageChannel`、transfer 和跨源。

## 先例

- **Chromium**：testdriver 經過 chromedriver（`run_wpt_tests.py`）；chromedriver 在 driver 裡算 Actions，送 CDP 的 `Input.dispatchMouseEvent`、`dispatchKeyEvent`、`dispatchTouchEvent`；Send Keys 先聚焦再逐字送按鍵。`postMessage` 用 `SerializedScriptValue`；user activation 是 `UserActivationState`，transient 5 秒。kun 照這幾樣做。
- **Firefox**：geckodriver 把 Actions 交給 Marionette，在瀏覽器裡（JS）合成事件；不照做（kun 的瀏覽器那一側是 Rust，driver 算比較簡單）。
- **Ladybird**：Actions 在 LibWeb 裡（WebContent process）照規範算，事件直接交給頁面。kun 的 renderer 照 process 的邊界寫，輸入要經過 browser，照 chromedriver 在 driver 算、送和真的輸入一樣的訊息比較一致。

## 已決定的事項

2026-10-10 確認：

1. **user activation 的狀態只放在 renderer，照 Chromium 的形狀寫，先不同步給 browser**。Chromium 有兩份：renderer 的 `blink::UserActivationState`（網頁的 `navigator.userActivation` 和 renderer 的判斷），以及 browser 的 `FrameTreeNode` / `RenderFrameHostImpl`；renderer 收到輸入時用 `UpdateUserActivationState` 通知 browser，browser 啟動祖先和同源的 frame、經過 proxy 告訴其他 process。browser 那一份是為了 site isolation（跨源的 iframe 在別的 process，只能由 browser 轉達）和防止被攻破的 renderer（全螢幕、控制項的 popup、開新視窗由 browser 自己檢查並用掉 activation）。kun 現在一個分頁的 frame 都在同一個 renderer 裡，browser 沒有要 activation 的功能，renderer 是 browser process 裡的 thread，這兩個理由都還不成立；多做一份只是沒有人用、測不到的狀態和訊息。不選照 Chromium 同步：等 multi-process、out-of-process iframe，或 browser 要擋 popup、全螢幕的時候再補，renderer 的 notify 和 consume 各只有一個入口，補的時候改動集中在那裡。
2. **Perform Actions 的 `pointerMove` 和 `scroll` 照 chromedriver：只送一個到終點的事件，`duration` 只決定 tick 的長度**。chromedriver（`window_commands.cc` 的 `ExecutePerformActions`）的 `pointerMove` 只送一個 `mouseMoved`、`scroll` 只送一個 wheel 事件，tick 分派完 `Sleep(tick_duration)`。規範的「perform a pointer move」要在 `duration` 裡分段送中間的位置（間隔由實作決定），Firefox 照做（Marionette 的 `Actions.sys.mjs` 線性插值，約每 17 ms 一次）。選 chromedriver：WPT 的結果要和 Chrome 比，測試是在 Chrome 上寫、通過的；kun 還沒有 `pointermove`、拖放和 `mouseover`，中間的移動現在沒有差別；每個事件都要等 renderer 的 ack，少很多來回。不選照規範：和規範不同的地方記進 `docs/limitations.md`，做 Pointer Events 或拖放時再看要不要改。
3. **window 的 `postMessage` 在這份做，J 改成其餘的部分**（P1）：這份做 `Window.postMessage`、`MessageEvent`、`onmessage`、`onmessageerror`，對象是同一個 renderer 裡同源的 window（自己、子 frame、`parent`、`top`），序列化用 `dom_js` 現有的 `StructuredSerializeForStorage`；路線圖的 J 改成 `MessageChannel`、`MessagePort`、transfer、`structuredClone()` 和跨源 window 的成員，接在這份的序列化和 `MessageEvent` 上。Chromium 的 window 的 `postMessage`（`DOMWindow::postMessage`、`PostMessageHelper`）和 `MessagePort`（mojo 的 port）本來就是兩條路，只共用 `SerializedScriptValue` 和 `MessageEvent`，照這條線切。不選先做完整份 J（P2）：J 還沒排程，transfer 和 port 的配對比較大，約 1500 個用 testdriver 的測試檔都要等。不選只做 post 給自己的（P3）：只對自己有效的 API 會誤導人，iframe 裡的 testdriver 靠 `event.source.postMessage` 也照樣不能用，J 還要重做。這一條改到 DOM 線的範圍，路線圖的 J 跟著改寫。
4. **Element Send Keys 真的打字：這份做文字控制項的最小編輯**（T1）：`<input>`、`<textarea>` 在 selection 插入字元、Backspace、Delete、方向鍵、Home、End、`<textarea>` 的 Enter，先分派可取消的 `beforeinput`、再改值、分派 `input`（`details.md` 的「文字輸入的最小範圍」）；沒有游標、IME、undo、剪貼簿。像 Firefox 給純文字控制項的獨立 `TextEditor`；Blink 是控制項裡的 shadow DOM「inner editor」走和 `contenteditable` 一樣的編輯指令（`TypingCommand`）。之後做編輯（路線圖的 P）時，再決定照 Blink 併進通用的編輯，或像 Firefox 一直分開。不選 Send Keys 只送按鍵（T2）：181 個用 `send_keys` 的測試檔大多在檢查值，照樣失敗，WebDriver 的使用者也會以為打得進去。不選另寫一份 RFC（T3）：多一份 RFC，第 4 段要等它，而最小的編輯範圍不大。
5. **每一段加進清單的 WPT 目錄，在那一段開始前決定**：先試跑候選目錄，照「目錄裡大部分能跑才整個加」挑；候選先列在「測試」。

2026-10-10，第 1 段開始前確認：

6. **`message` 宣告成 `any`，在 `postMessage` 裡先解析 `targetOrigin`、再序列化**（S1）：`ScriptHost` 加一個序列化的方法，`dom_js` 用現有的 `StructuredSerializeForStorage`。錯誤和副作用的順序照規範（`SyntaxError` 先於 `DataCloneError`，`options` 的字典先於 `message` 的 getter），之後 J 的 transfer 和 `structuredClone()` 也接在這個方法上；Chromium 也是在 `postMessage` 裡先算 target 的 origin、再序列化。代價：`postMessage` 和 `MessageEvent.data` 只有 JS 的（`any` 不進 WIT），wasm 文件只有自己一個 window，用處本來就小；wasm 的 guest 讀寫 `any` 是路線圖的 U。不選照 `history.pushState` 在轉換參數時就序列化（S2）：錯誤和副作用的順序和規範、Chromium 相反，而且序列化時還沒讀到 `options`，J 的 transfer 做不了，到時要改回 `any`、改 WIT 的簽名。
7. **incumbent（呼叫的一方）用 V8 的 entered context 近似**（I1）：`ScriptHost` 加 `incumbent_window()`，預設是 `realm_window()`（wasm 文件的 guest 呼叫 host 時，呼叫的 instance 就是 incumbent，預設就對）；`dom_js` 覆寫成 `get_entered_or_microtask_context` 的 global。`MessageEvent.source`、`origin` 和 `targetOrigin: "/"` 用它。和規範不同的地方：別的 realm 的函式被呼叫、再由它呼叫 `postMessage` 時，規範的 incumbent 是那個函式的 realm，entered context 是外層的；記進 `docs/limitations.md`。不選照 Chromium 在 rusty_v8 的 fork 包 `Isolate::GetIncumbentContext()`（I2）：要改 fork 的 C++、重編 V8，共用的 artifact 跟著換；WPT 的結果需要時再做，只改 `dom_js` 和 fork。
8. **`data` 照 `history.state` 存序列化的 bytes，第一次讀時在 event 的 realm 反序列化**（M1）：之後每次讀是同一個物件。序列化用的是「for storage」，不能複製的在序列化時就丟錯誤，反序列化不會失敗，所以 `messageerror` 在 kun 不會發生；`onmessageerror` 照樣宣告，限制裡寫明。不選在 task 裡分派前先反序列化（M2）：要多一個反序列化進指定 realm 的方法，換來的 `messageerror` 在 kun 走不到。
9. **`MessageEvent` 的 `source` 是 `Window`，先不宣告 `ports`**：kun 沒有 `WindowProxy`，`MessagePort` 等 J 加進 union；`MessageEventInit.ports`、`initMessageEvent` 的 `ports` 參數也等 J（`spec-missing.txt` 列出它們）。不選現在讓 `ports` 回傳空陣列：要一個 JS 專用的型別，J 做時要改掉。

2026-10-10，第 2 段開始前確認：

10. **user activation 的狀態放在 `dom` 的 `Window`**（A1）：一個 `#[no_trace]` 的欄位，上一次 activation 的時間（沒有過是 +∞，照規範的「last activation timestamp」）。照規範是每個 `Window` 的，kun 一份文件一個 window，換文件自然重設（規範的導覽重設）；`dom_api` 和 `navigator.userActivation` 直接讀，測試不經過 renderer。Chromium 放在 `Frame`（`UserActivationState`）、導覽時清掉，行為一樣。不選放在 renderer 的 frame（A2）：`dom_api` 讀不到，要多一個 `FrameClient` 的入口，測試也要經過 renderer。
11. **先不做「用掉」activation（consume）**（C1）：kun 還沒有會用掉它的 API（`window.open`、全螢幕、控制項的 popup），第一個來的時候照規範做整棵 frame 樹的 consume。不選現在就做入口和單元測試（C2）：沒有呼叫的地方。
12. **其餘照規範和 Chromium**：trusted 的 `mousedown`（任何按鍵）和 `keydown`（Escape 和瀏覽器保留的快捷鍵除外）在分派之前 activate（之後有 Pointer Events、touch 再加 `pointerdown`、`touchend`），WebDriver 的 Element Click 走同一條輸入；script 的 `click()`、`dispatchEvent` 不會。activate 這個 window、所有的祖先 frame（不管 origin）和同源的子孫 frame（Chromium 的 `FrameTreeNode::NotifyUserActivation` 也是）。transient 的期限照 Chromium 是 5 秒，用 frame 的時鐘。`navigator.userActivation` 是 `[SameObject]` 的 `UserActivation`（`hasBeenActive`、`isActive`），`dom` 的新 GC 型別、由 `Navigator` 保存；兩個成員是 `bool`，wasm 的 guest 也有。

2026-10-10，第 3 段開始前確認：

13. **鍵盤的對照表是 machopodriver 自己的**（K1）：照規範的「normalised key value」、「code for key」（美式鍵盤）和 shift 的對照表，加上 `keyCode`（`KeyboardEvent.keyCode`，規範沒有）。表是規範定的、有限的，chromedriver 也是自己的表。不選 Servo 用的 `keyboard-types` crate（K2）：多一個依賴，也沒有 `keyCode`，還是要補。
14. **`pen` 和 `touch` 回答 `unsupported operation`**（P1）：kun 沒有 Pointer Events 和 touch，用它們的測試馬上失敗，不做出和 Chrome 不同的事件。滑鼠的第 3、4 個按鍵（上一頁、下一頁）kun 的輸入沒有，也一樣。不選把 `pen` 當滑鼠送（P2，chromedriver 送 `pointerType: pen` 的滑鼠事件，Chrome 再分派 pen 的 pointer event）：kun 只會出現滑鼠事件，結果看起來像別的 bug。
15. **元素當 origin 時不捲動**（O1）：`Kun.getElementClickPoint` 加一個參數，只算 in-view center，不捲動、不檢查會不會點到別的元素；不在 viewport 裡是 `move target out of bounds`。照規範和 chromedriver（它用 script 算 in-view center）。不選直接用 Element Click 那一套（O2）：Actions 不該捲動頁面。
16. **輸入來源的狀態每個 top-level 視窗一份**（S2）：照規範（session 的 top-level 視窗對 input state 的 map）；指標的座標是相對於視窗的 viewport，切換視窗之後 `origin: "pointer"` 從那個視窗自己的位置算；視窗關掉時那一份丟掉，不留下卡住的按鍵。和 chromedriver 不同（它整個 session 一份）：按著 modifier 切換視窗、按著按鍵關掉視窗時的行為不一樣，記進 `docs/limitations.md`。不選每個 session 一份（S1）：多的只是一點程式，換來和規範一致。
17. **其餘照規範和 chromedriver**：tick 照第 2 條；自動化訊息新增 `Input.dispatchKeyEvent`（CDP 的名字和欄位，經過分頁的 `WebContents`，和 Element Click 一樣），`Input.dispatchMouseEvent` 加 `mouseWheel`（和真的滾輪一樣經過 compositor）；按著的 modifier 套用到之後的滑鼠和滾輪事件；driver 照 chromedriver 算 `clickCount`（500 ms 內、4 px 以內；時間照第 34 條）；Release Actions 照 input cancel list 的反序放開，再清掉那個視窗的狀態。

2026-10-10，第 3 段實作時定的細節（照規範和 Chromium，四段做完時一起審核）：

18. **值為整數的 float 當成整數**：規範的整數是值為整數的 Number，wptrunner 送的 `duration` 是 float 的倍數乘出來的（`100.0`），`webdriver` crate 的整數欄位不收（請求變成 `invalid argument`）。machopodriver 在交給 crate 之前，把 Perform Actions 的 body 裡這種數字寫成整數；只改這個 route，其他指令照舊。crate 不檢查最大安全整數，`duration`、`width`、`height` 和 scroll 的四個數字由 machopodriver 檢查（不然 `2^53` 的 `pause` 真的會等）。同一個請求裡兩個 sequence 用同一個 id 是 `invalid argument`。
19. **座標是目前的 browsing context 的 viewport 的**（照規範；chromedriver 也加上 frame 的位移）：新的 `Kun.getFrameViewport` 回答 frame 的 viewport 在 top-level 裡的位置和大小（renderer 從 pre-paint 的 transform 反推 frame 的原點），machopodriver 用它檢查邊界、把座標換成 top-level 的再送。元素的 in-view center point 是相對於元素所在 frame 的 viewport（照規範：第一個 client rect 限制在那個 viewport 裡的中心），可能在 viewport 外，加上 offset 之後才檢查邊界（第 15 條的「不在 viewport 裡」是這一步）。指標的位置照規範記在那個 viewport 的座標，`origin: "pointer"` 從它開始；按鈕送在 top-level 的實際位置。
20. **連點只在同一個 Perform Actions 裡算**：規範還沒定（w3c/webdriver#1772），WPT 的 `release_actions/sequence_tentative.py` 期待兩次 Perform Actions 的點擊各自是單擊；同一個 Perform Actions 裡照第 17 條。
21. **滑鼠移動分派之後才 ack**：renderer 的滑鼠移動在下一個 main frame 才分派（Chromium 的 rAF-aligned input），原本收到就 ack，`Input.dispatchMouseEvent` 回答時網頁還沒收到 `mousemove`；改成分派之後才 ack，像 Chromium 的 `MainThreadEventQueue`（rAF-aligned 的事件跑完才 ack）。分頁在那之前被隱藏時（不再有 frame），立刻分派。真的視窗裡移動的 ack 因此晚到下一個 frame，browser 不靠它做別的事。

2026-10-10，第 4 段開始前確認：

22. **Send Keys 在 machopodriver 做**（S1）：renderer 的 `Kun` 方法只做事前的準備（能不能取得焦點、檔案輸入、捲進畫面、沒有焦點時聚焦並把 selection 放到最後），按鍵由 driver 用第 3 段的鍵表一個字一對送 `Input.dispatchKeyEvent`，像 chromedriver（JS 聚焦，再送 CDP 的按鍵）。不選整個在 renderer（S2）：按鍵會繞過使用者和 Actions 走的那條輸入，是另一套行為。
23. **Send Keys 的 modifier 照規範**（M1）：每次一個暫時的鍵盤來源，和 Actions 按著的鍵一起算；`\uE000`（NULL）放開所有 modifier，結束時也全部放開。chromedriver 讓 modifier 在不同次的 Send Keys 之間一直按著（`sticky_modifiers`），和它不同的地方記進 `docs/limitations.md`。不選 M2（照 chromedriver）：理由和第 16 條一樣。
24. **刪除和移動以 grapheme cluster 為單位**（G1）：`dom_api` 直接依賴 `icu_segmenter`（已經在依賴樹裡：parley、`webdriver` crate）。Blink 的移動和 Delete 也是 grapheme；Backspace 對部分文字只刪一部分（`BackspaceStateMachine`），先不做，記成限制。不選 code point（G2）或 UTF-16 code unit（G3）：會把 emoji 和組合字元拆開。
25. **文字控制項的 `change` 照 HTML**（C1）：值在聚焦期間被使用者改過時，blur 時分派；`<input>` 按 Enter 也分派（不送出表單）。Blink 在 `TextControlElement` 的 blur 和 Enter 時分派。不選不做（C2）：表單和 `element_send_keys` 的測試等它。
26. **有 `textInput`**（E1）：`TextEvent`（`data`、`initTextEvent`），打字的順序照 UI Events 和 WPT 的 `uievents/textInput/`（它的 listener 斷言 `textInput` 之前已經有 `beforeinput`）：`keydown`、`keypress`、`beforeinput`、`textInput`、`input`、`keyup`；兩個都可以取消，取消了就不插入（Blink 由 `textInput` 的預設動作插入，`EventHandler::HandleTextInputEvent`）。Enter 在 `<textarea>` 有 `textInput`（`data` 是 `\n`），在 `<input>` 沒有；Backspace、Delete 沒有。（開始時寫成 Chromium 的 `textInput` 在 `beforeinput` 之前，讀了 WPT 之後改正。）不選 E2（`keypress` 之後直接 `beforeinput`）：`uievents/textInput/` 測的就是這條路。
27. **文字控制項有焦點時，↑、↓ 先不處理**（U2），照舊捲動頁面：kun 不知道排版之後的折行，照硬換行移動在折行時是錯的，有 layout 的行之後還要重寫；和 Chrome 不同的地方記成限制。←、→、Home、End、Space 由控制項處理、不捲頁（照 Blink）。
28. **這段試跑的 WPT**：wdspec 的 `element_send_keys`、`element_clear`；`uievents/keyboard/`、`uievents/textInput/`；`input-events/` 裡 `<input>`、`<textarea>` 的（不含 `contenteditable` 的）；表單目錄裡用 `send_keys` 的；第 3 段留下的 `html/interaction/focus/` 的 Tab、`html/user-activation/` 的鍵盤和停用的 4 個 `send_keys` 的。照「目錄裡大部分能跑才整個加」挑。編輯放在 `dom_api` 新的 `editing` 模組（對應 Blink 的 `Editor`、`TypingCommand`），renderer 的按鍵預設動作只呼叫它。

2026-10-10，第 4 段實作時定的細節（照規範和 Chromium，四段做完時一起審核）：

29. **Element Clear 的「在畫面裡」不做 hit test，大小是 0 的框也算**：捲進畫面後，元素的第一個框和 viewport 有交集（含邊上）就可以。kun 的文字控制項是 0×0，照規範的 pointer-interactable（hit test）一律點不到；WPT 的 wdspec 期待畫面外的是 `element not interactable`、被別的元素蓋住的照樣清空，這樣兩者都對（規範的「interactable」也包括 keyboard-interactable，照字面的話畫面外的也能清空，和 WPT 的期待不同）。
30. **能打字的 `<input>` 只有選取範圍 API 適用的那些**（text、search、url、tel、password）：email、number 和日期時間的值在 kun 存的是清理過的，打到一半（尾端的空白、還不是數字的字）就被清掉，游標也跟著錯；之後要分開「顯示中的文字」和值時再加。和 Chrome 不同的地方記成限制。
31. **Element Send Keys 的細節照規範的步驟**：檔案輸入不先檢查檔名（一律 `unsupported operation`）；不可輸入的控制項（color、date、datetime-local、month、range、time、week）直接設值、不分派事件，唯讀、disabled 的是 `element not interactable`；不只一個 code point 的 grapheme cluster 當成一個鍵（規範要 composition 事件，kun 沒有）。
32. **`textInput` 在 `beforeinput` 之後**：見第 26 條的改正。
33. **情境的 Enter 帶 `\r`**：平台（winit）的 Enter 帶 `"\r"`，renderer 照它分派 `keypress` 和換行；情境原本只給 Space 文字，Enter 在情境裡打不出換行。改成和平台一樣。`change` 只在頁面裡的焦點換了時 commit：分頁失去系統的焦點（Tab 到網址列）時不 commit，焦點還在控制項上。
34. **連點的 500 ms 用動作自己的時間算**（2026-10-11，完成之後補）：兩次按下相隔多久，是它們之間各個 tick 的長度（`pause`、`duration`）加總，不讀時鐘。原本照 chromedriver 在按下時讀時鐘（`GetMouseClickCount` 的 `base::TimeTicks::Now()`），但 chromedriver 的事件除了整個請求的最後一個都不等回答（`ExecutePerformActions` 的 `async_dispatch_event`，`WebViewImpl::DispatchMouseEvents` 用 `SendCommandAndIgnoreResponse`；2026-10-11 讀 `main` 上的原始碼），兩次按下之間只有它自己的處理和 tick 的 `Sleep`；kun 的每個事件都等 renderer 的 ack（上面的「Perform Actions 和 Release Actions」），兩次按下之間多了按下和放開各一次來回。renderer 忙的時候（24 核的 Linux 上 `shell` 的整合測試一起跑）超過 500 ms，`webdriver` 的 `actions_are_the_users_input` 的雙擊變成兩次單擊。用動作的時間，效果和 chromedriver 實際的一樣（連著按是雙擊，中間 `pause` 500 ms 以上就不是），也不受機器快慢影響。不選照 chromedriver 不等回答：推翻上面「下一個 tick 送出前，上一個事件的 listener 已經跑完」，第 2 條也靠它。不選時鐘扣掉等回答的時間：效果一樣，要多記每次等待，結果還是看機器。

## 待決定

沒有。

## 之後的可能

- **Pointer Events 和 touch**：`PointerEvent`、`pointerType` 的 `touch`、`pen`，`Input.dispatchTouchEvent`；解開 `pointerevents/`。
- **`mouseover` 類的事件和 `:hover`**：Actions 的移動之後就有輸入來源，可以接著做。
- **J 的其他部分**：`MessageChannel`、transfer、跨源的 `postMessage`。
- **testdriver 的擴充**：`set_permission`（WebDriver 的 Permissions 擴充）、`generate_test_report`，等 kun 有對應的功能。
- **控制項的繪製和完整的編輯**：畫出控制項和游標之後，Element Click 點得到 `<input>`，WPT 的鍵盤測試（wdspec 的 `key_reporter`）就跑得了；email、number 的打字要先分開「顯示中的文字」和值；`contenteditable`、`Selection`、composition（路線圖的 P）時再決定文字控制項的編輯併進通用的編輯，或一直分開（第 4 條）。
- **WebDriver 其他的指令**：Get Active Element、Get Element Text 這類 wdspec 用得到的。
- **WebDriver BiDi**：testdriver 只有 BiDi 才有的部分。
