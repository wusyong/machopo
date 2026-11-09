# shell

瀏覽器的視窗和 `machopo` 這個執行檔。怎麼 build、怎麼跑見 repo 根目錄的 `README.md`。

## 鍵盤和滑鼠

視窗模式的快捷鍵和滑鼠操作。改了 `crates/shell/src/browser_view.rs` 的 `command_for`、`BrowserView::key`、`BrowserView::mouse_input`，`crates/shell/src/tab_strip.rs` 的 `TabStrip::mouse_down`、`TabStrip::mouse_up`，`crates/dom_api/src/hyperlink.rs` 的 `follow_hyperlink`，`crates/shell/src/omnibox.rs` 的 `Omnibox::key`，`crates/shell/src/keyboard.rs`（按鍵換成網頁的 `key`、`code`、`keyCode`）、`crates/shell/src/mouse.rs`（連點的次數），或 `crates/shell/src/window.rs` 裡滑鼠按鍵的對應時，這些表要跟著改。

**導航**（網址列有沒有 focus 都可以用，例外見表）

| 動作 | macOS | Windows、Linux |
| --- | --- | --- |
| 上一頁 | Cmd+[，Cmd+←（網址列沒有 focus 時），滑鼠的上一頁鍵 | Alt+←，滑鼠的上一頁鍵 |
| 下一頁 | Cmd+]，Cmd+→（網址列沒有 focus 時），滑鼠的下一頁鍵 | Alt+→，滑鼠的下一頁鍵 |
| 重新整理 | Cmd+R，F5 | Ctrl+R，F5 |
| 移到網址列（全選） | Cmd+L，F6 | Ctrl+L，Alt+D，F6 |

工具列的上一頁、下一頁、重新整理按鈕，要在同一個按鈕上按下又放開主按鍵才算按。

**分頁**（網址列有沒有 focus 都可以用）

| 動作 | macOS | Windows、Linux |
| --- | --- | --- |
| 新分頁（`about:blank`，網址列有 focus） | Cmd+T | Ctrl+T |
| 關閉分頁（最後一個會關掉視窗） | Cmd+W | Ctrl+W |
| 下一個分頁（最後一個之後回到第一個） | Ctrl+Tab，Cmd+Option+→，Cmd+Shift+] | Ctrl+Tab，Ctrl+PageDown |
| 上一個分頁 | Ctrl+Shift+Tab，Cmd+Option+←，Cmd+Shift+[ | Ctrl+Shift+Tab，Ctrl+PageUp |
| 第 1 到 8 個分頁、最後一個分頁 | Cmd+1 到 8、Cmd+9 | Ctrl+1 到 8、Ctrl+9 |

連結：中鍵、Cmd+點擊（macOS）或 Ctrl+點擊（Windows、Linux）在背景開新分頁，再加 Shift 在前景開；Shift+點擊和 `target="_blank"` 的連結在前景開新分頁。從連結開的分頁放在原本的分頁右邊，關掉它時回到原本的分頁。

網頁上的連點：在系統的連按間隔之內（macOS 照「輔助使用」裡的設定，Windows、Linux 是 500 ms）、在同一處（各方向 2 px 之內）再按同一個鍵，算下一下，第二下有 `dblclick`。

renderer 當掉的分頁顯示 sad tab：按它的「Reload」按鈕、重新整理的快捷鍵或工具列的重新整理，就開新的 renderer 重新載入。在網址列輸入 `about:crash` 可以讓目前的分頁的 renderer 結束（`about:gpucrash` 讓 GPU 遺失），網頁裡的連結觸發不了它們。

視窗上方的分頁列：在分頁上按下主按鍵就切換；分頁的關閉按鈕和 `+` 按鈕要在同一個按鈕上按下又放開才算；在分頁上按中鍵關掉它。分頁窄於 100 CSS px 時，只有目前的分頁有關閉按鈕。

收起來的 `<select>` 的下拉清單（Browser 畫在視窗的最上面，網頁和工具列之上）開著時：游標在它上面時 hover 那一列，在一列上放開主按鍵選它（在選單上按下、拖到清單裡放開也是）；按鍵都給它：↑、↓、PageUp、PageDown、Home、End 移動，Enter、Tab、F4、Alt+↑、Alt+↓ 選它，Esc 關上不選；在它上面的滾輪捲它的列，別處的滾輪、在網頁以外（工具列、分頁列）按下、視窗大小改變、切換分頁、視窗失去 focus 都關上它；在網頁上按下由 renderer 關上（按下照樣送到網頁）。

**網址列**（有 focus 時）

| 動作 | macOS | Windows、Linux |
| --- | --- | --- |
| 前往 | Enter | Enter |
| 還原成目前的網址 | Esc | Esc |
| 移動游標 | ←、→；Option+← / → 以詞為單位；Cmd+← / →、Home、End 到頭尾 | ←、→；Ctrl+← / → 以詞為單位；Home、End 到頭尾 |
| 選取 | 移動游標的鍵加上 Shift | 移動游標的鍵加上 Shift |
| 刪除 | Backspace、Delete；加上 Option 刪一個詞；Cmd+Backspace 刪到開頭、Cmd+Delete 刪到結尾 | Backspace、Delete；加上 Ctrl 刪一個詞 |
| 全選、複製、剪下、貼上 | Cmd+A、C、X、V | Ctrl+A、C、X、V |

- 沒有 focus 時點一下網址列會全選；之後再點會放游標，Shift+點擊延伸選取，拖曳可以選取。點網頁會拿走 focus，網址還原。
- 貼上時換行會變成空白，頭尾的空白拿掉。
- 中文輸入法只在網址列有 focus 時打開；組字中的文字加上底線顯示在網址列裡。
- 前往時網址的補全：有 `http`、`https`、`file`、`about` scheme 的照用；存在的本機路徑變成 `file:`；其他的當成主機名稱，`localhost` 和 IP 位址補 `http://`，其他的補 `https://`。沒有搜尋。

**網頁**（網址列沒有 focus 時）

| 動作 | 按鍵 |
| --- | --- |
| 捲動 40px | ↑、↓、←、→ |
| 往下、往上捲一頁（網頁區域高度的 87.5%） | Space、Shift+Space；PageDown、PageUp |
| 捲到頂端、底部 | Home、End |
| 移到下一個、上一個可以 focus 的元素（進出 iframe；過了頭尾回到網址列） | Tab、Shift+Tab |
| 開 focus 的連結 | Enter（加上開新分頁的鍵開在新分頁） |

- 快捷鍵以外的按鍵都先送給網頁（`keydown`、`keypress`、`keyup`）；網頁的 `keydown` 呼叫 `preventDefault` 時，上表的動作都不做。捲動從 focus 的元素（沒有時從最後按下滑鼠的元素）所在的捲動容器開始，捲不動再往外，最後是整頁。
- 網頁裡的 `<textarea>` 或文字類的 `<input>` 有 focus 時，打出的字、Backspace、Delete、←、→、Home、End（加 Shift 時選取）、`<textarea>` 的 Enter 由控制項處理，上表的捲動不做；↑、↓ 照舊捲動。
- 網址列有 focus 時，Tab、Shift+Tab 把 focus 交給網頁的第一個、最後一個可以 focus 的元素。
- 滾輪和觸控板先捲游標底下的捲動容器，捲不完的再交給外層，最後是整頁。
- 滾輪的捲動由 renderer 的 compositor thread 處理，網頁的 script 忙碌時照樣能捲（`about:demo` 的「Busy for 3 seconds」）；按鍵要先經過網頁的 `keydown`，script 忙碌時要等（和 Chrome 一樣）。例外是被縮放、旋轉、半透明，或被畫在它之後的東西蓋到的捲動容器，以及在它們裡面的：滾輪要等 script 忙完（compositor 只認得成為 layer 的捲動容器，也只知道每個 layer 的範圍、不知道之後畫的 layer 哪裡真的有東西，所以這些交給 main thread 判斷）；網頁上有不是 passive 的 `wheel` / `mousewheel` listener 的元素上也是，listener 可以取消捲動（照 Chrome 的 wheel event handler region）。
- 主按鍵在同一個連結上按下又放開，才算點連結；中鍵在新分頁開（上面）；右鍵只分派 `contextmenu`，沒有選單。

## 自動化的 pipe

`machopo --automation-pipe` 讓外面的程式（machopodriver，`crates/driver/README.md`）經過 stdin 和 stdout 驅動瀏覽器，對應 Chromium 的 `--remote-debugging-pipe`（`DevToolsPipeHandler`，它用 fd 3 和 4）。瀏覽器是沒有平台視窗的 `BrowserWindow`（和情境測試的一樣），分頁從 `about:blank` 開始（initial entry：第一次新增 entry 的導覽取代它，和 Chrome 的新分頁一樣），加上 `--window` 是真的視窗（在最上層，期間不理會平台的滑鼠和鍵盤，和 `machopo --automation` 一樣）；沒有平台視窗時，`--scale F` 是 device scale factor（machopo 自己的 reftest 用它在 2 倍、1.5 倍下跑；真的視窗用螢幕的）；stdin 結束或 `Browser.close` 時結束。machopo 的 log 和網頁的輸出（`console.log`、沒接住的例外、wasm guest 的 stdout）都在 stderr：stdout 只有 pipe 的訊息。WPT 的網域和根憑證用 `--host-resolver-rules`、`--additional-trust-anchors` 給（`machopo --help`）。

訊息定義在 `protocol::automation`，處理它們的是 `automation_session`（UI thread；Chromium 的 `DevToolsSession` 和它的 handler），`automation_pipe` 讀寫 pipe（`AutomationPipeReader`、`AutomationPipeWriter` thread）。情境的執行器（`automation`）也是它的 client：在同一個 process 裡經過 channel 送同樣的命令（`automation::Client`），不經過 pipe。pipe 或情境的執行器是 session 的 driver（自動化模式才有）；`--remote-debugging-port` 的連線是另外的 client（下一節）。每個 client 的回答和事件都放進它自己的 outbox（`automation_session::Outbox`：pipe 的 writer、websocket 的連線或執行器的 channel），可以晚一點回答；要給視窗的輸入交回 embedder（沒有平台視窗的 `windowless::drive`，或 `window.rs`），由它像平台的事件那樣給視窗，再回答。

- **格式**照 CDP：命令 `{id, method, params, sessionId}`，回答 `{id, result}` 或 `{id, error: {code, message}}`，事件 `{method, params, sessionId}`，每則之後接一個 NUL。一則訊息最多 64 MiB（容得下帶整份 script 的命令），超過時 pipe 關掉，瀏覽器跟著結束。錯誤碼是 JSON-RPC 的：不是命令的訊息（沒有 `id`、`method`，多了別的屬性）-32600，沒有的方法 -32601，參數不對 -32602（包括 machopo 沒做的參數：不默默忽略），找不到 session -32001，其他 -32000。
- **名字相同，語意就相同**：CDP 有的方法用 CDP 的名字、參數和回答；沒有的放 `Machopo` domain（只在自動化模式，也就是 `--automation-pipe` 和 `--automation`：它的視窗的操作假設沒有人在用視窗；那時每個 client 都能用，一般的視窗裡是 -32601，像 Chrome 的 `HeadlessExperimental` 只在 headless 模式有）：人在視窗裡的操作，不經過 session、作用在目前的分頁（情境用的，對應 Chromium 的 interactive_ui_tests 的 `ui_controls` 和 `ui_test_utils`），和 WebDriver 在 renderer 裡的工作，經過 session、作用在它的分頁（machopodriver 用的，對應 chromedriver 經過 `Runtime.callFunctionOn` 注入的 JS；machopo 在 renderer 裡用 Rust 做，`protocol::webdriver`）。WebDriver 的方法回答 `{value}`，值是 WebDriver 的 JSON（元素是 `{"element-6066-…": id}`，分頁的 window 是 `{"window-fcc6-…": target id}`，子 frame 的 window 是 `{"frame-075b-…": frame id}`）；WebDriver 的錯誤是 -32000，錯誤碼在 `data`（`stale element reference` 等）。WebDriver 的方法都可以帶 `frame`（`Machopo.switchToFrame` 給的 frame id），在那個子 frame 裡做；沒有時在 top-level。frame id 是分頁的 renderer 給的（renderer 的前綴加上 frame 的號碼，frame 導覽時不變，Chromium 的 frame token）；frame 不在了（iframe 移除、上層的文件換了）是 `no such window`。
- **分頁是 target**，target id 是分頁建立時 `WebContents` 抽的 128 位元亂數（32 個十六進位字，和 Chrome 一樣猜不到：`/json/close/<id>` 是網頁的 `<img>` 也發得出的 GET；Chromium 的 `devtools_frame_token`），每個 client 看到的都一樣，分頁的 main frame 的 id 和它一樣（每次啟動 renderer 都交給它，renderer 當掉重開也不變）；`Target.attachToTarget`（只有 `flatten: true`）拿到 `sessionId`，`Page.*` 都要帶它。載入（loader）的 id 是分頁的 navigation id。

| 方法 | 做什麼 |
| --- | --- |
| `Browser.getVersion` | `product`（真的視窗是 `machopo/版本`，沒有平台視窗時是 `Headlessmachopo/版本`，照 Chrome 的 headless 模式的 `HeadlessChrome/…`）、`userAgent`（網頁的，兩種都一樣） |
| `SystemInfo.getInfo` | 畫圖用的 GPU，照 Chrome 的格式：`gpu.devices` 列裝置的名稱和驅動，machopo 的 adapter 類型（`Cpu` 是 CPU adapter）和 backend 在 `gpu.auxAttributes`；Viz 建好 renderer 之前 `devices` 是空的 |
| `Browser.close` | 回答之後結束 |
| `Target.getTargets`、`Target.getTargetInfo` | 分頁的 `TargetInfo`（標題、網址、有沒有 session） |
| `Target.attachToTarget` | 分頁的 session |
| `Target.createTarget` | 新的分頁，在其他分頁之後，顯示 `url`（`about:blank` 是 initial entry）；`background: true` 時不顯示（一樣在最後）。回答 `targetId`。沒有 `newWindow`（machopo 只有一個視窗） |
| `Target.closeTarget`、`Target.activateTarget` | 關掉分頁（最後一個關掉視窗，machopo 結束；回答 `success: true`）；顯示分頁 |
| `Target.setDiscoverTargets` | 打開時先告訴現有的分頁，之後有 `targetCreated`、`targetDestroyed`、`targetCrashed`（renderer 當掉，或 `about:crash`；分頁還在，顯示 sad tab） |
| `Target.setAutoAttach` | 回答成功：沒有會自己接上的 target（iframe 都在分頁的 renderer 裡，沒有 worker），browser 層級的也不接上分頁（Chrome 會） |
| `Target.detachFromTarget` | 拿掉 client 自己的 flat session（`sessionId`，或舊的 `targetId`），送 `Target.detachedFromTarget`（`sessionId`、`targetId`）；分頁關掉時，連在它上面的 flat session 也收到它（直接連到分頁的連線收到 `Inspector.detached`） |
| `Page.enable` | 之後有 `Page.frameStartedNavigating`（載入一份文件的導覽開始了，不管是誰開始的：命令、連結、`location`；`navigationType` 是 `differentDocument`、`reload` 或 `historyDifferentDocument`）和 `Page.navigatedWithinDocument`（`fragment`、`historyApi`，上一頁、下一頁是 `other`） |
| `Page.setLifecycleEventsEnabled` | 記下來之後轉給 renderer 的 `Page` agent（下一節），由它送 `Page.lifecycleEvent`；renderer 換了（當掉重開）時，browser 把 `Page.enable` 和它再交給新的 renderer |
| `Page.navigate` | 開始導覽，導覽要 commit 之前（收到回應、交給 renderer 之前，Chromium 的 ready-to-commit）才回答 `frameId`、`loaderId`、`isDownload`（一律 false）；載入失敗時帶 `errorText`（`net::ERR_…`，錯誤頁照樣 commit），被新的導覽取代時是 `net::ERR_ABORTED`；machopo 自己的 `about:` 頁立刻回答。`frameStartedNavigating` 和導覽的 `Network` 事件在回答之前；不是網址時是 -32000 |
| `Page.reload`、`Page.getNavigationHistory`、`Page.navigateToHistoryEntry` | 重新載入（`ignoreCache` 收下，machopo 沒有 cache；`loaderId` 不是顯示中的文件的、`scriptToEvaluateOnLoad` 不是空的時候是錯誤）；session history（entry 的 id 是它的 index）；到某個 entry（同一份文件的 entry 不載入） |
| `Page.captureScreenshot` | 分頁的網頁 view，下一個顯示了它目前狀態的 frame 畫出來時（physical px；renderer 一律要一個新的 frame，等它送進 Viz 才回答，沒有新的東西要畫時也一樣：上一次 commit 的 frame 不一定已經畫了），PNG 的 base64（`data`）；只有 `format: "png"`，沒有 `clip` 等其他參數；分頁要是顯示的那個，不然 -32000 |
| `Input.dispatchMouseEvent` | 直接交給分頁的 renderer（Chromium DevTools 的 `InputHandler`，不經過視窗）：`type` 是 `mouseMoved`（沒有按鈕）、`mousePressed`、`mouseReleased`（`button` 是 `left`、`middle`、`right`）、`mouseWheel`（`deltaX`、`deltaY`，CSS px；和真的滾輪一樣先經過 compositor），`x`、`y` 是網頁的 CSS px，`buttons`（按著的鈕，DOM 的位元）、`clickCount`（連按的第幾下）沒給是 0，`modifiers` 照 CDP；renderer 處理完（ack；移動在下一個 frame 分派之後）才回答 |
| `Input.dispatchKeyEvent` | 一樣直接交給分頁的 renderer，不經過瀏覽器的快捷鍵：`type` 是 `keyDown`（`text` 是它打出的字，網頁有 `keypress`）、`rawKeyDown`（不打字）、`keyUp`，沒有 `char`；`key`、`code`、`location`（0 到 3）是 DOM 的，`windowsVirtualKeyCode` 是 `keyCode`，`autoRepeat`，`modifiers` 照 CDP；renderer 處理完才回答 |
| `Browser.getWindowForTarget` | machopo 唯一的視窗：`windowId` 是 1，`bounds` 是視窗的位置和大小（CSS px，大小包括分頁列和工具列） |
| `Browser.setWindowBounds` | 改視窗的位置和大小（`left`、`top`、`width`、`height`，沒給的不變；大小每邊不超過 `protocol::viz::MAX_TEXTURE_SIZE` 實體像素，Viz 把視窗畫成一張 texture；沒有平台視窗時位置只記下來），視窗變成那個大小才回答；`windowState` 是 -32602 |
| `Machopo.navigateFromAddressBar` | 像在網址列輸入之後按 Enter |
| `Machopo.waitForLoad` | 分頁最新的導覽載入完、顯示它的 frame 畫出來（`DidFinishLoad`）才回答；已經載入完的立刻回答。帶 session 時是那個 session 的分頁，沒帶時是顯示的分頁；藏在後面的分頁不畫 frame，載入完要等它顯示出來 |
| `Machopo.dispatchWindowInput` | 給視窗輸入（視窗的 CSS px）：`move`（滑鼠移到那裡）、`click`（主按鍵按下、放開，一串連點的第一下）、`dblclick`（在那裡連點兩下，第二下是 double click）、`wheel`（`deltaX`、`deltaY`）、`key`（`key` 是 winit 的 `NamedKey` 的名字或一個字元，`modifiers` 照 CDP：Alt 1、Ctrl 2、Meta 4、Shift 8，`text` 是它打出的字）；給了才回答 |
| `Machopo.waitForInputHandled` | 給目前的分頁的輸入都處理完（ack）才回答 |
| `Machopo.getTitle` | 目前的分頁的標題，分頁列上顯示的（`title`） |
| `Machopo.getCursor` | 滑鼠所在的地方的游標，用 CSS 的關鍵字（`cursor`：`default` 是箭頭、`pointer` 是手指、`text` 是 I 形；`all-scroll` 和 `move` 一樣）：網頁上是 renderer 要的，工具列上是工具列的 |
| `Machopo.queryElement` | `selector`（和 iframe 的 selector 路徑 `frames`）找到的元素：視窗座標的框（沒有框是 `null`）和 `textContent`，以及網頁 view 在視窗裡的位置、視窗的 scale |
| `Machopo.scrollIntoViewIfNeeded` | 把元素捲進畫面，畫出捲完的 frame 之後回答 |
| `Machopo.captureWindow` | 下一個顯示了目前狀態的 frame（`now: true` 是下一個畫出來的），整個視窗：寬、高（physical px）和 base64 的 RGBA |
| `Machopo.executeScript` | session 的分頁的 top-level 文件（或 `frame` 的文件）裡，`script` 當成 async 函式的本體，用 `args`（WebDriver 的 JSON，參照換回元素和 window）呼叫；`async: true` 時最後多一個函式，第一次被呼叫時 settle。settle 之後回答 JSON clone 的值，reject 是 `javascript error` |
| `Machopo.findElements` | `using`（`css selector`、`tag name`）和 `value` 找到的元素，從文件或 `element` 開始，照樹的順序；`querySelectorAll` 的 selector 錯了是 `invalid selector`，`link text`、`partial link text`、`xpath` 是 `unsupported operation`，不認得的方式是 `invalid argument` |
| `Machopo.getElementRect`、`Machopo.getElementTagName` | 元素的 `getBoundingClientRect()` 加上捲動的位移（CSS px）；qualified name（不轉大寫） |
| `Machopo.getElementAttribute`、`Machopo.getElementProperty` | attribute 的值（HTML 的 boolean attribute 有就是 `"true"`），沒有是 `null`；script 讀到的 property，JSON clone |
| `Machopo.getElementClickPoint` | Element Click 點擊前的部分：元素不在畫面裡時捲進畫面（`block: end`、`inline: nearest`，經過外面的 frame；只露出一部分的不捲），回答它的第一個 client rect 在畫面裡那部分的中心，取 floor（top-level 的 viewport 的 CSS px，`{value: {x, y}}`）。「在畫面裡」是 hit test 在那一點找得到它或它裡面的（WebDriver 的「在自己的 pointer-interactable paint tree 裡」；控制項的 UA shadow tree 算它的）：被 `overflow` 裁掉、`pointer-events: none` 的不算（規範要假裝它收得到 pointer 事件，WPT 期待 `element not interactable`）；不在畫面裡是 `element not interactable`，那一點最上面的不是它或它裡面的（子 frame 裡的算成它的 iframe）是 `element click intercepted`。`inViewCenterOnly: true` 時是 Perform Actions 的元素 origin：不捲動、不檢查，回答 WebDriver 的 in-view center point（第一個 client rect 限制在它的 frame 的 viewport 裡的中心，取 floor，那個 viewport 的 CSS px；元素在 viewport 外時這一點也在外面），沒有框是 `move target out of bounds` |
| `Machopo.getFrameViewport` | Perform Actions 的 viewport：`frame`（沒給是 top-level）的 viewport 在 top-level 的 viewport 裡的位置和它的大小（CSS px，`{value: {x, y, width, height}}`） |
| `Machopo.prepareSendKeys` | Element Send Keys 在網頁裡的部分（`element`、`text`）：元素不在畫面裡時捲進畫面（同 `Machopo.getElementClickPoint`）；不能取得焦點（不是 `<body>`、`<html>`）是 `element not interactable`；沒有焦點時聚焦，文字控制項的游標放到值的最後。回答 `{value: true}` 時 driver 送按鍵；不可輸入的控制項（color、日期時間那幾種、range）在這裡設值，回答 `false`（唯讀、disabled 的是 `element not interactable`）；檔案輸入是 `unsupported operation` |
| `Machopo.clearElement` | Element Clear：不是可以編輯的控制項（唯讀、disabled、按鈕這類）是 `invalid element state`；不在畫面裡時捲進畫面（同 `Machopo.getElementClickPoint`），之後第一個框（大小是 0 也算）不在 viewport 裡是 `element not interactable`；空的不動，不然聚焦、清空、分派 `input` 和 `change`、blur |
| `Machopo.captureElementScreenshot` | 元素不在畫面裡時捲進畫面（同 `Machopo.getElementClickPoint`），等畫出來之後截它在畫面裡的那部分，PNG 的 base64（`{value}`）；沒有任何部分在畫面裡是 `unable to capture screen` |
| `Machopo.switchToFrame` | `frame` 裡的子 frame 的 id：`index` 是 `window[index]` 的（照樹的順序），`element` 是那個 iframe 的；沒有的話是 `no such frame`，兩個都給是 `invalid argument`。都沒給時是 `frame` 自己的 id（top-level 是 `null`），用來確認它還在 |

測試：`cargo test -p protocol --test it automation::`（JSON）、`cargo test -p shell --test it automation_pipe::`（`machopo` 的執行檔，經過 pipe；`--window` 的那一個要顯示器，只在設了 `MACHOPO_WINDOW_TESTS` 時跑，`python3 scripts/test.py automation` 會跑它）。情境（`tests/scenarios/`）經過同一個 session，也是它的回歸測試。

## 遠端除錯（`--remote-debugging-port`）

`machopo --remote-debugging-port=PORT`（視窗或 `--automation-pipe`；`0` 是讓系統挑）像 Chrome 的同名開關，用 HTTP 和 websocket 提供同一套協定，給 Chrome 的 DevTools（`chrome://inspect` 預設就找 `localhost:9222`）和其他 CDP 的工具。開始聽之後在 stderr 印 `DevTools listening on ws://127.0.0.1:PORT/devtools/browser/<id>`，和 Chrome 一字不差（Puppeteer、Playwright 讀這一行）。`devtools_http_handler` 在自己的 thread（`DevToolsHandlerThread`，Chromium 的 `DevToolsHttpHandler`；單 thread 的 tokio，HTTP 用 hyper，websocket 用 `tokio-tungstenite`）只處理 HTTP 和 websocket，請求和連線都經過 `automation_session::ClientEvent` 交給 UI thread 的 session。

| 位置 | 做什麼 |
| --- | --- |
| `GET /json/version` | `Browser`（`Browser.getVersion` 的 product）、`Protocol-Version`、`User-Agent`、`webSocketDebuggerUrl` |
| `GET /json/list`、`/json` | 每個分頁：`id`（target id）、`type`（`page`）、`title`、`url`、`webSocketDebuggerUrl`、`devtoolsFrontendUrl`（Chrome 內建的 DevTools：`devtools://devtools/bundled/inspector.html?ws=…`，machopo 沒有前端可以給） |
| `PUT /json/new?<網址>` | 開新的分頁並顯示，網址是 query（解開 `%` 的跳脫），沒有或不是網址時是 `about:blank`（照 Chrome）；回答它的那一筆。其他 method 是 405 |
| `GET /json/activate/<id>`、`/json/close/<id>` | 顯示、關掉分頁（`Target activated`、`Target is closing`）；沒有那個 id 是 404 |
| websocket `/devtools/browser/<id>` | 和 pipe 一樣：browser 的命令，自己 `Target.attachToTarget`（flat session）；`<id>` 是 stderr 那一行的，不對是 404 |
| websocket `/devtools/page/<target id>` | 一連上就是那個分頁的 session：沒有 `sessionId` 的命令都是它的，送給它的回答和事件也不帶 `sessionId`（Chrome 的每個 target 的連線）；分頁關掉時送 `Inspector.detached`（`target_closed`）再關掉連線 |

- **好幾個 client 同時連**（Chromium 的 `DevToolsAgentHost` 有好幾個 `DevToolsAgentHostClient`）：每條連線是一個 client，有自己的 session（別的 client 拿同一個 session id 是 -32001）和自己打開的事件（`Target.setDiscoverTargets`、`Page.enable`……）；連線斷了，它的 session 跟著消失。pipe 和 websocket 可以同時用（`--automation-pipe --remote-debugging-port`）。任何 client 都能 `Browser.close`。
- **擋住網頁**（port 等於整個瀏覽器的控制權；照 Chrome）：只聽 127.0.0.1；`Host` 不是 IP 也不是 `localhost` 的請求回答 500（不然把網域解析到 127.0.0.1 的網頁就和它同源：DNS rebinding）；帶著 `Origin` 的 websocket（網頁開的）是 403，除非 `--remote-allow-origins`（逗號分開，或 `*`）列了它，Chrome 的 DevTools 自己的頁面（`devtools://devtools`）一律可以，沒有 `Origin` 的工具照收；`/json/new` 只收 `PUT`；target id 猜不到（上一節）。
- **大小**：一則訊息最多 64 MiB、一個 frame 最多 16 MiB（`tokio-tungstenite` 的預設），超過時那條連線關掉，瀏覽器和其他 client 照常。
- **結束**：瀏覽器結束時先丟掉 session，server 再等 websocket 送完手上的訊息（最多 5 秒，例如 `Browser.close` 的回答），才停下。

## 分頁的 session：V8 的 inspector

browser 不處理的方法（上面的表以外的，`protocol::automation` 的 `Method::Other`），帶著分頁的 session 時照原樣交給它的 renderer（`RendererMsg::DevToolsMessage`；Chromium 的 `DevToolsSession` 把 handler 不處理的命令 fall through 給 renderer），renderer 的 V8 inspector 回答 `Runtime`、`Debugger`、`Profiler`、`HeapProfiler`、`Console`、`Schema`（`bindings::devtools`，Blink 的 `MainThreadDebugger`）。沒有 session 的是 -32601；renderer 當掉了是 -32000，交出去還沒回答的也是（Chromium 的 `DevToolsSession` 也記著它們），session 留著，renderer 重開之後接到新的 renderer 上（打開過的 domain 要再打開）；wasm 的文件沒有 inspector，renderer 回答 -32601。renderer 送回的回答和事件不解讀，只檢查：session 是那個分頁的、不超過 256 MiB、是 JSON 的物件，不合的丟掉（renderer 不可信），合的照原樣（`Message::Raw`）給 session 的 client。

- **context**：每個 frame 的 realm 是一個 context，`auxData` 是 `{isDefault: true, type: "default", frameId}`（main frame 的 `frameId` 是 target id，子 frame 的是 WebDriver 的 frame id），`origin` 是文件的 origin（opaque 的照 Chrome 是 `://`：Blink 的 `SecurityOrigin::ToRawString` 的空 scheme 和 host）；沒有指定 context 的命令在 main frame 的 realm 裡跑。文件丟棄時它的 context 跟著 `executionContextDestroyed`，main frame 的文件丟棄時整組 `executionContextsCleared`（舊頁面的 console 訊息一起清掉）。
- **console 和例外**：`console` 是 V8 內建的，經過 inspector 給 DevTools（`consoleAPICalled`；最近的 1000 則留著，`Runtime.enable` 時補送）和終端機（`[console] 文字 (網址:行)`）。沒有被接住的例外和沒有處理的 promise rejection 在沒有被 `error`、`unhandledrejection` 的 listener 取消時給 DevTools（`exceptionThrown`，位置是 stack 頂端的；之後才加上 handler 的 rejection 送 `exceptionRevoked`）。例外的完整 stack 只在有 session 打開 `Runtime` 時記（inspector 開關它，和 Chrome 一樣：每建一個錯誤都要付）。

- **停住和打斷**：renderer 收到的 DevTools 訊息不進主佇列，進它的 DevTools 佇列（主佇列只放一個叫醒它的 task；Chromium 的經過 IO thread 進 Blink 的 `InspectorTaskRunner`）。停在斷點時，V8 在頁面的 script 裡呼叫 `runMessageLoopOnPause`，inspector 只處理這條佇列，頁面的其他 task 等著；停住時才連上的 session 和它的命令等繼續之後；斷線的 session 先停止除錯（沒有別人在除錯時頁面繼續），回來之後才刪；renderer 要結束時讓頁面繼續、終止 script。script 跑著時，browser 標著的幾個方法（`Debugger.pause`、`resume`、斷點的設定、`getScriptSource`、`Runtime.terminateExecution`……；Chromium 的 `DevToolsSession::ShouldSendOnIO`）經過 `Isolate::RequestInterrupt` 當場處理，其他的等 script 跑完。
- **`Profiler`** 是 inspector 的（V8 的 `CpuProfiler`，isolate 是 renderer 的，所以分頁的每個 frame 都在裡面）：要先 `Profiler.enable`；取樣時 renderer 照 Blink 標閒置，等下一個 task 時是 `(idle)`，在 task 裡、不在 script 是 `(program)`。profiler 開始之前已經在執行中最佳化（OSR）的程式，V8 不記，它的樣本算給呼叫它的函式：要量的程式編譯之前就開始。走共用路徑的成員（overload……）共用一個 callback，在 profile 裡是同一個 node。
- **`Page` 的 frame 樹**：renderer 自己回答 `Page.getResourceTree`、`getFrameTree`、`getResourceContent`（`render_thread/page_agent`，Blink 的 `InspectorPageAgent`），不交給 inspector，停在斷點時也是。frame 的 id、父 frame、`loaderId`（main frame 的是 navigation id，和 `Page.lifecycleEvent` 的一樣；子 frame 的是帶來它的文件的那個請求的 `Network` 的 id，初始的 `about:blank` 是文件的號碼）、網址、origin、MIME type、iframe 的 `name`、secure context，和文件載入過的子資源（stylesheet、圖片、script，每個網址一個，失敗的標 `failed`；`fetch()` 不算，和 Chrome 一樣）：frame 一律記著它們，和 DevTools 在不在無關。`getResourceContent` 給 frame 的文件的原始碼（每份文件留著它解碼後的 markup，文件丟棄時跟著丟），子資源的從那個 session 的 `Network` 留著的 body 拿（`Network` 打開之前載入的沒有：machopo 沒有快取），要先 `Page.enable`（和 Chrome 一樣）。script 建立的 iframe 的 `frameAttached` 帶建立它的 script 的 stack（有 session 打開 `Page` 時，在 iframe 插入的當下抓）。`Page.enable`、`disable` 照 Chromium 的 fall through：browser 記下來（它自己的 `frameStartedNavigating`、`navigatedWithinDocument`）再轉給 renderer 回答，renderer 打開時送 `frameAttached`、`frameNavigated`（新的 iframe 的初始 `about:blank` 不送）、`frameDetached`：每個 task 結束時和上次告訴的比，main frame 的新文件在建它的 realm 之前送。
- **`Network`**：renderer 送出的請求（子資源、`fetch()`、iframe 的文件）由它的 `Network` agent 告訴打開 `Network` 的 session（`render_thread/network_agent`，Blink 的 `InspectorNetworkAgent`）：`requestWillBeSent`（`requestId` 是 renderer 的前綴加上 request id，iframe 的文件的也是它的 `loaderId`；發起者照 Blink 的 `BuildInitiatorObject`：parser 建立的元素是 `parser` 帶文件的網址和開始標籤的行，stylesheet 的 `url()` 是 `parser` 帶 stylesheet 的網址（文件裡的是文件的網址），module 的 import 是 `script` 帶 import 它的 module 的網址，script 要的在 session 送了 `Network.setAttachDebugStack` 之後是 `script` 帶 V8 的 inspector 抓的 stack（`await` 接在 async 的 parent 裡），其他的 `other`。renderer 在 script 跑完、parser 停下之後才一起發請求，所以 frame 在 dom_api 要求載入的當下抓 stack、依元素記著，發請求時取出來，一輪沒用上的丟掉；parser 的行號在它結束之後也留到那一輪的請求發完）、redirect 的每一跳（同一個 id 的 `requestWillBeSent` 帶 `redirectResponse`）、`responseReceived`（`net` 給的全部的 header、時間、遠端位址、協定）、`dataReceived`（body 一次到，送一次）、`loadingFinished`、`loadingFailed`（取消的帶 `canceled`）。每個 session 各自記：`Network.enable` 的上限（預設總共 200 MB、每個 20 MB，超過時丟掉最舊的）、留著的 body（和網頁共用，不複製；main frame 換文件時清掉）、請求的 body（`getRequestPostData`）。`getResponseBody` 文字類的 MIME 給文字，其他的 base64。machopo 做不到的命令照參數回答：效果本來就成立的成功（`setCacheDisabled`、空的 `setBlockedURLs`、不離線也不限速……），做不到的（封鎖、限速、離線、加 header）是錯誤；cookie、cache 和網路狀況的命令由 browser 的 session 自己回答（cookie 一律是空的）。main frame 的導覽由 browser 的 session 告訴（`Network.enable`、`disable` 記下來之後也轉給 renderer）：開始時的 `requestWillBeSent`（`requestId`、`loaderId` 都是 navigation id；machopo 自己的 `about:` 頁沒有）、redirect、`responseReceived` 或失敗的 `loadingFailed`，都在 UI thread 上、commit 之前；renderer 在文件 commit 時送 `dataReceived`、`loadingFinished`，留著文件的 body。`Page` agent 在 main frame 的 `DOMContentLoaded`、`load` 時送 `Page.domContentEventFired`、`loadEventFired`。
- **生命週期**：renderer 的 `Page` agent 照 Blink 的 `InspectorPageAgent::LifecycleEvent` 對每個 frame 送 `Page.lifecycleEvent`（只給打開了 `Page` 和 lifecycle 事件的 session）：新的文件 `frameStartedLoading`、`init`（在 `frameNavigated` 之前；iframe 的初始 `about:blank` 也有 `init`），`DOMContentLoaded`、`load` 在事件觸發時，`load` 之後 `frameStoppedLoading`；`networkAlmostIdle`、`networkIdle` 照 Blink 的 `IdlenessDetector`（`idleness_detector`）：從 `DOMContentLoaded` 起，文件進行中的請求（子資源和 `fetch()`）不超過 2 個、0 個，持續 500 ms，時間是安靜開始的那一刻；render thread 在每個 task 之後數，下一次可能到的時間也排進事件迴圈。每個 frame 的文件到目前為止的事件一律記著：`setLifecycleEventsEnabled(true)` 每呼叫一次，就對每個 frame 補送 `commit` 和它們（`Page` 沒打開也補送，之後的事件才要 `Page`）；`Page.disable` 也關掉 lifecycle 事件。browser 不送 lifecycle 事件。renderer 的 `load` 在 load 事件時送，畫面可能還沒畫出來：machopodriver 等完 `load` 再呼叫 `Machopo.waitForLoad`，回答時畫面一定畫好。
- **`Log`**：renderer 的 log agent（`render_thread/log_agent`，Blink 的 `InspectorLogAgent` 和頁面的 `ConsoleMessageStorage`）記下載入的訊息：回應 400 以上（「Failed to load resource: the server responded with a status of 404 (Not Found)」，main frame 的文件也算）和載入失敗（「Failed to load resource: net::ERR_CONNECTION_REFUSED」，取消的不算），每則帶網址和 `networkRequestId`；也記下 script 的呼叫引起的警告（來源 `javascript`、`warning`，`FrameClient::console_warning`，Blink 的 `ExecutionContext::AddConsoleMessage`：`PerformanceObserver.observe()` 不支援的種類），沒有網址；和 DevTools 在不在無關都記，留最近 1000 則，`Log.enable` 時先補一則丟掉了幾則、再補送留著的，然後才回答；main frame 換文件時清掉，`Log.clear` 也清。每則也照 console 的格式印到 stderr（`[console] 文字 (網址)`）。session 要求 machopo 做不到的 `Network` 命令（限速、離線、封鎖、加 header）時，回答錯誤之外另外送它一則 warning（來源 `other`，「方法: machopo can't …」，不留、不印），DevTools 的 Console 看得到：renderer 回答的由 log agent 送，browser 自己回答的（`emulateNetworkConditions`……）由 browser 的 session 送，它也記下 `Log.enable`、`disable` 再轉給 renderer。Chrome 沒有這種警告。DevTools 的時間（包括 `Page.lifecycleEvent` 的）都是 `ipc::TimeTicks` 的秒。
- **`HeapProfiler`**：heap snapshot 裡有 DOM 的物件（elise 的 heap 的，名字是它們的型別，例如 `Node`，和 wrapper 併成一個；不在任何文件的樹裡的節點是 detached）和 JS 的。

和 Chrome 比：`python3 scripts/test.py cdp` 對 machopo 和 Chrome for Testing 送同樣的命令，比較回答和事件（每次都會變的 id 和時間換成記號），列出不同的地方；已知的差異在 `scripts/cdp_compare.py` 的 `KNOWN`，附上原因。不在 CI 裡（要 Chrome）。

測試：`cargo test -p shell --test it remote_debugging::`（`machopo --automation-pipe --remote-debugging-port=0`：HTTP 的 endpoint、兩種 websocket、擋住網頁的檢查、client 之間、大小的上限）、`cargo test -p shell --test it windowless::without_a_driver`（一般的視窗的 session：沒有 `Machopo` domain）、`cargo test -p shell --lib devtools_http_handler`（`Host`、`Origin` 的判斷）、`cargo test -p shell --test it devtools::`（分頁的 session 和 inspector：context、console、例外、renderer 重開、wasm、停住、斷點和單步、打斷、停住時斷線和關分頁、heap snapshot、frame 樹、文件的原始碼和子資源、frame 的事件）、`cargo test -p shell --test it devtools_network::`（`Network`：請求、回應和 body、redirect 和失敗、每個 session 各自的狀態、iframe 的文件、main frame 的導覽和它的載入事件、做不到的命令）、`cargo test -p shell --test it devtools_initiators::`（請求的發起者：parser 的行、stylesheet、module 的 import、script 的 stack 和 `await`、`frameAttached` 的 stack）、`cargo test -p shell --test it devtools_log::`（`Log`：載入失敗和 404、補送、清掉、做不到的命令的警告）、`cargo test -p shell --test it devtools_lifecycle::`（每個 frame 的生命週期和順序、`networkIdle` 的計時、晚打開時的補送、`Page.navigate` 的回答時機和 `errorText`、`Target.setAutoAttach`、`detachFromTarget`、`detachedFromTarget`）、`cargo test -p shell --test it automation_pipe::the_profile`（`Profiler`）、`cargo test -p shell --test it windowless::a_renderers_devtools`（renderer 的訊息的檢查）。視窗裡的（`machopo --remote-debugging-port`、Chrome 的 `chrome://inspect`）只做過手動確認。
