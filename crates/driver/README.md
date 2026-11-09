# driver

machopodriver：machopo 的 WebDriver（classic）server，對應 Chromium 的 chromedriver。它提供 WebDriver 的 HTTP API，每個 session 啟動一個 `machopo --automation-pipe`，經過 machopo 的 stdin 和 stdout 送照 CDP 格式的訊息（`protocol::automation`，訊息的清單見 `crates/shell/README.md` 的「自動化的 pipe」）。

```sh
cargo build -p shell -p driver
target/debug/machopodriver --port 4444
```

選項見 `machopodriver --help`（預設啟動同一個目錄裡的 `machopo`；`--binary-arg` 把參數交給 machopo，例如 `--test-fonts`、WPT 的 `--host-resolver-rules`）。只聽 `127.0.0.1`。

## 怎麼運作

- **`http`**：HTTP/1.1 用 hyper（`WebDriverServer` thread，單執行緒的 tokio）。路徑照 `webdriver` crate 的 `standard_routes()` 比對（不知道的路徑是 `unknown command`，路徑對、method 不對是 `unknown method`），用 `WebDriverMessage::from_http` 解析成指令，經過 channel 交給 dispatcher，答案經過 oneshot 回來，server 照樣收別的請求。拒絕網頁從瀏覽器裡發得出的請求，和 `webdriver` crate 的 server 一樣：`Host` 不是這個 server（不是 IP 也不是 `localhost` 的網域、別的 port）、有 `Origin`、POST 的 content type 是 form 送得出的（`text/plain` 等）。
- **`session`**：dispatcher（`WebDriverDispatcher` thread）一次處理一個指令。一次一個 session；New Session 用 `webdriver` crate 的 `match_browser` 比對 capabilities（`MachopoCapabilities`），啟動 machopo、attach 到它的分頁（`Target.attachToTarget`），Delete Session 送 `Browser.close`、等 machopo 結束（5 秒內沒結束就 kill）。
- **視窗和 frame**：每個視窗是一個分頁，window handle 是分頁的 target id，順序是分頁列的；分頁第一次成為 session 的視窗時 attach（每個分頁一個 page session）。New Window 在背景開 `about:blank` 的分頁（`Target.createTarget`，machopo 只有一個平台視窗，所以不管要求的種類都是分頁），Switch To Window 把分頁切到前面（`Target.activateTarget`；背景的分頁不跑 rAF、截圖也要畫出來的分頁，chromedriver 也這樣做），Close Window 關掉它（`Target.closeTarget`），關掉最後一個時 machopo 結束、session 也結束。目前的 browsing context 是 session 從分頁的 top-level 一路切下來的子 frame 的 id（chromedriver 的 frame 堆疊，id 由 machopo 的 renderer 給、子 frame 導覽時不變，`Machopo.switchToFrame`）；`Machopo` domain 的 WebDriver 方法帶著最後一個，Switch To Parent Frame 拿掉最後一個、先確認上一層還在。導覽、切換視窗都回到 top-level。Get / Set Window Rect 是 machopo 唯一的視窗的位置和大小（`Browser.getWindowForTarget`、`setWindowBounds`；machopo 把大小每邊夾到 8192 實體像素，見 `crates/shell/README.md`）。
- **`machopo`**：一個 session 的 machopo 和讀它訊息的 thread（`MachopoPipeReader`）。`call` 送一個指令、等它的回答，期間來的事件留著（`call_until` 等到期限為止，之後才來的回答丟掉）；`wait_for` 從留著的事件開始等，看過的丟掉。分頁的 renderer 當掉（`Target.targetCrashed`）記到那個分頁再導覽為止。
- **導覽的等待**：Navigate To 的 `Page.navigate` 帶 `errorText`（載入失敗，錯誤頁照樣 commit）時照 chromedriver 回答 `unknown error`，訊息是 `net::ERR_…`。照 page load strategy 等 machopo 的 `Page.lifecycleEvent`（loader 的 id 是每個分頁自己的，所以也比對 frame）：`normal` 等那次載入（loader）的 `load`，再等顯示它的 frame 畫出來（`Machopo.waitForLoad`，帶分頁的 session：之後的截圖看得到它；chromedriver 不等畫面），`eager` 等 `DOMContentLoaded`，`none` 不等；上一頁、下一頁、重新整理先等 `Page.frameStartedNavigating`（拿到 loader）或 `Page.navigatedWithinDocument`（同一份文件內，不用再等）。超過 `pageLoad` 的逾時是 `timeout`；逾時是 `null` 時不限時。
- **script 和元素**：Execute (Async) Script、Find Element(s)、元素的框、tag name、attribute、property 是 machopo 的 `Machopo` domain 的 WebDriver 方法（經過分頁的 session），machopo 回答 WebDriver 的 JSON（元素和 window 的參照都在裡面）；driver 只管逾時：script 的逾時到了是 `script timeout`（machopo 之後才來的回答丟掉），找元素在 implicit wait 期間每 50 ms 再找一次，直到找到。
- **點擊和截圖**：Element Click 先問 machopo 指標要到哪裡（`Machopo.getElementClickPoint`：元素不在畫面裡時 machopo 把它捲進畫面，算出它的第一個 client rect 在畫面裡那部分的中心（取 floor），用 hit test 確認那一點找得到它，再檢查那一點沒有別的元素在上面），再用 `Input.dispatchMouseEvent` 移過去、按下、放開主按鍵（直接交給分頁的 renderer，像 chromedriver）；放開的回答來的時候 renderer 已經處理完點擊，點擊開始的導覽也已經告訴 driver，照 page load strategy 等它。Take Screenshot 是 machopo 的 `Page.captureScreenshot`（網頁 view，等下一個顯示目前狀態的 frame），Take Element Screenshot 是 `Machopo.captureElementScreenshot`（元素捲進畫面後在畫面裡的那部分）。
- **`actions`**：Perform Actions 和 Release Actions 照規範在 driver 裡算，像 chromedriver。輸入狀態（輸入來源的種類、按著的鍵和按鈕、指標的位置、input cancel list）每個視窗一份（規範的「input state map」；chromedriver 整個 session 一份），存在 session 上、以 window handle 為 key，關掉視窗時丟掉。先檢查整個請求、照 tick 分組（`process`），再一個 tick 一個 tick 分派（`perform`）：鍵盤是 `Input.dispatchKeyEvent`（`key`、`code`、`location` 照規範的鍵表，`keyCode` 照美式鍵盤；會打字的 `keyDown` 帶 `text`，其他的是 `rawKeyDown`），滑鼠是 `Input.dispatchMouseEvent`（`mouseMoved`、`mousePressed`、`mouseReleased`、`mouseWheel`，帶著所有鍵盤來源按著的 modifier 和所有指標按著的按鈕），每個都等 renderer 處理完；tick 分派完之後等滿它最長的 `duration`。有 `duration` 的移動和捲動一次到位，不插中間的事件（chromedriver 也是）。座標是目前的 browsing context 的 viewport 的（`Machopo.getFrameViewport`：子 frame 的 viewport 在 top-level 裡的位置和大小），送出時換成 top-level 的；超出 viewport 是 `move target out of bounds`。元素當 origin 時用它的 in-view center point（`Machopo.getElementClickPoint` 的 `inViewCenterOnly`：不捲動、不檢查會不會點到別的元素）。連點的次數（`clickCount`）照 chromedriver：同一個按鈕、500 ms 內、各方向 4 px 以內就加一，只在同一個 Perform Actions 裡算。500 ms 是動作自己的時間（兩次按下之間各個 tick 的長度加總），不讀時鐘：每個事件都等 renderer 回答，renderer 忙的時候會拉開兩次按下；chromedriver 讀時鐘，但它的事件（除了最後一個）不等回答。Release Actions 照 input cancel list 的反序放開，再丟掉那個視窗的狀態。
- **Send Keys 和 Clear**：Element Send Keys 先讓 machopo 準備元素（`Machopo.prepareSendKeys`：捲進畫面、不能取得焦點的回答 `element not interactable`、沒有焦點時聚焦並把文字控制項的游標放到最後；不可輸入的控制項直接設值，檔案輸入回答 `unsupported operation`），再照規範的「dispatch actions for a string」送按鍵：一個暫時的鍵盤來源加進那個視窗的輸入狀態（和 Actions 按著的鍵一起算），一個 grapheme cluster 一對 `Input.dispatchKeyEvent`，美式鍵盤要按 Shift 的字自動按放 Shift，modifier 按著到 `\uE000`（NULL）或結束，用完拿掉那個來源。Element Clear 是 machopo 的（`Machopo.clearElement`：不能編輯的回答 `invalid element state`，捲進畫面後第一個框不在 viewport 裡的回答 `element not interactable`，再照規範清空：聚焦、`input`、`change`、blur）。兩者回答 `element not interactable` 時，driver 在 implicit wait 內每 50 ms 再問一次。
- **整數**：規範的整數是值為整數的 Number，`webdriver` crate 的整數欄位不收 `100.0`（wptrunner 的 `duration` 乘上 float 的倍數）：Perform Actions 的 body 先把這種數字寫成整數再解析。crate 也不檢查 JavaScript 的最大安全整數，`duration`、`width`、`height` 和 scroll 的座標、位移由 `actions` 檢查。
- **錯誤**：規範的錯誤碼和 HTTP status 來自 `webdriver` crate 的 `ErrorStatus`，JSON 自己產生（`error`），所以也能回答 chromedriver 的 `tab crashed`（分頁的 renderer 當掉）和 `disconnected`（machopo 結束了），wptrunner 看到這兩個時把測試記成 CRASH。machopo 的 WebDriver 錯誤在 CDP 錯誤的 `data`（`stale element reference` 等），照它換回 `ErrorStatus`。

## 有的指令

Status、New Session、Delete Session、Get / Set Timeouts、Navigate To、Get Current URL、Back、Forward、Refresh、Get Title、Get Window Handle、Get Window Handles；Execute Script、Execute Async Script、Find Element、Find Elements、Find Element From Element、Find Elements From Element、Get Element Rect、Get Element Tag Name、Get Element Attribute、Get Element Property；New Window、Close Window、Switch To Window、Switch To Frame、Switch To Parent Frame、Get Window Rect、Set Window Rect；Element Click、Element Send Keys、Element Clear、Take Screenshot、Take Element Screenshot、Perform Actions、Release Actions。其他的回答 `unknown command`。

- session 回答 `setWindowRect: true`；`acceptInsecureCerts` 是 `false`（machopo 不能被告知接受不信任的憑證）；沒有 proxy，`webSocketUrl`（BiDi）不行。其他瀏覽器自己的 capability（`goog:chromeOptions`）不管。
- Get Current URL、Get Title、Get Window Handle 在分頁的 renderer 當掉時回答 `tab crashed`（chromedriver 也是）；導覽照樣進行，它會啟動新的 renderer。
- Get Title 是分頁的標題：renderer 在每個 task 之後把改了的 `document.title` 告訴 browser。
- 找元素只有 `css selector` 和 `tag name`；`link text`、`partial link text`、`xpath` 回答 `unsupported operation`：連結文字要元素畫出來的文字（`innerText`），`xpath` 要 XPath，machopo 都還沒有。
- Set Window Rect 改給了的部分（`x`、`y`、`width`、`height` 各自，wdspec 的 `test_partial_input`），沒給的不變；沒有平台視窗時位置只是記下來，網頁的 `screenX`、`screenY` 照樣跟著它。沒有最小化、最大化、全螢幕。
- Element Send Keys 不能挑檔案（machopo 沒有檔案）。
- Perform Actions 只有 machopo 有的輸入：`pointerType` 是 `pen`、`touch`，或滑鼠的第 3、4 個按鍵（上一頁、下一頁）時回答 `unsupported operation`；同一個請求裡兩個 sequence 用同一個 id 是 `invalid argument`。
- 元素參照是它所在的 frame 的：在別的 frame 用它是 `no such element`，同一個 frame 換了文件之後是 `stale element reference`。子 frame 的 window 是 frame 參照（`frame-075b-…`），id 是 frame 的 id。`--script wasm` 的文件不執行 WebDriver 的 script（`unsupported operation`），找元素和讀元素照樣可以，property 除外（要經過 script）。

## 怎麼新增一個指令

1. 在 `session.rs` 的 `Dispatcher::handle` 加一個分支，照規範的步驟寫在 `Session` 的方法裡；要 machopo 做的事用 `protocol::automation` 的方法（`Machopo::call` / `call_ok`），CDP 有的用 CDP 的，沒有的加在 `Machopo` domain（先在 `protocol::automation` 加方法，再在 shell 的 `automation_session` 實作）。
2. 錯誤用規範的（`Failure::new(ErrorStatus::…)`）。
3. 測試：`crates/shell/tests/it/webdriver.rs`（driver 在 process 裡，machopo 是 `machopo` 的執行檔），再把 WPT 對應的目錄加進 `tests/wpt/wdspec-include.txt`，用 `python3 scripts/test.py wdspec` 跑（`tests/wpt/README.md`）。

## 測試

- `cargo test -p driver`：路由、`Host` 等檢查、參數；Actions 的鍵表、tick 和它的長度、連點、座標和邊界、錯誤，Send Keys 的按鍵（假的視窗，時鐘由測試控制）。
- `cargo test -p shell --test it webdriver::`：整個 driver 對真的 machopo（session、capabilities、導覽和它的等待、逾時、當掉的分頁、拒絕的請求、script 和它的逾時、implicit wait、元素的指令、視窗和 frame 的切換、window rect、點擊、截圖、Actions 和每個視窗的輸入狀態、Send Keys、Clear）。
- `python3 scripts/test.py wpt`：WPT 的 testharness.js 測試、reftest 和 crashtest，經過上游的 wptrunner，走的就是 machopodriver 這一條路。
- `python3 scripts/test.py wdspec`：WPT 的 WebDriver 測試，經過上游的 wptrunner。
