# RFC 0021：WebDriver（classic 的子集）

- 開始日期：2026-10-08
- 狀態：✅ 已完成：2026-10-09（「已決定的事項」第 1 到 13 條 2026-10-08 接受；第 1b 段的第 14 到 18 條、第 2 段的第 19 到 23 條 2026-10-08，第 3 段的第 24 到 26 條、第 4 段的第 27、28 條 2026-10-09 確認；第 0 步和四段都做完，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`

## 摘要

kun 實作 W3C WebDriver（classic，HTTP 加 JSON）裡 wptrunner 跑 testharness.js 測試、reftest 和 crashtest 要用的指令：session、導覽、分頁和 frame 的切換、視窗大小、執行 script、找元素、點擊元素、截圖。這是「改用上游的 `wpt run`」的第一份 RFC：接上 wptrunner、換掉 Rust 的 runner 是下一份，Actions 和 testdriver 是再下一份（「之後的可能」）。

2026-10-08 確認的：server 是獨立的 driver 程式（`machopodriver`），經過 stdin 和 stdout 的 pipe 控制它啟動的 kun（像 Ladybird 和 chromedriver），pipe 上的訊息照 CDP 的格式，CDP 有的方法用 CDP 的，WebDriver 在 renderer 裡的部分在 kun 自己的 `Kun` domain；情境的執行器也改用同一套訊息；指令的解析用 Mozilla 的 `webdriver` crate（geckodriver 和 Servo 用的），HTTP 那一層 driver 自己做；執行 script、JSON 的轉換和元素參照在 renderer 裡用 Rust 照規範的演算法做（像 Ladybird 和 Servo），經過 `protocol` 的新訊息；瀏覽器用沒有平台視窗的 `BrowserWindow`（情境用的那一個），一個 window handle 是一個分頁。

## 動機

**現況（2026-10-08）**：WPT 由 Rust 的 runner（`4ff8d6e:crates/shell/tests/wpt_testharness.rs`、`4ff8d6e:crates/shell/tests/reftest.rs`）在 headless 的 kun 裡跑，經過 kun 自己的 `testRunner` 交出結果。RFC 0020 換成了上游的 wptserve，但控制瀏覽器和收結果的那一半還是自己的。和上游的 `wpt run`（wptrunner 透過 WebDriver 控制瀏覽器）的差別見 `rfcs/0020-wptserve/details.md` 的「做之前的跑法和上游的差別」。代價：

- **testdriver 沒有路可走**：`testdriver.js` 的輸入（`click`、`send_keys`、`Actions`）要有人在頁面外面送。`4ff8d6e:tests/wpt/testharness-skip.txt` 裡整組 testdriver 的測試、`dom/events` 約一半，以及 cssom-view、焦點和捲動裡要輸入的都在等它。
- **跑法是 kun 自己的**：結果不能輸出成 wptreport、不能和其他瀏覽器在 wpt.fyi 上比較；reftest 的 chain 和 `fuzzy` 是照 wptrunner 重寫的（`4ff8d6e:crates/shell/src/reftest.rs`），兩邊的判斷可能不同。
- Chromium 自己的方向也是 WebDriver：它的文件說 `run_web_tests.py`（content_shell 加 protocol mode，kun 現在學的就是這一套）不鼓勵，要用 `run_wpt_tests.py`（wptrunner 加 chromedriver）。

2026-10-08 比較過 testdriver 的幾種做法（renderer 裡注入、browser 合成輸入、runner 代勞、做 WebDriver），選了做 WebDriver：和上游、Chromium 的主力一致，WebDriver 本身之後也有用（Selenium、外部的自動化工具）。代價是 testdriver 要等三份 RFC 都做完才解得開。

**目標**：

- wptrunner 跑 testharness、reftest、crashtest 要用的 WebDriver 指令（`details.md` 的「wptrunner 用到的指令」），照規範的行為和錯誤。
- WPT 自己的 WebDriver 一致性測試（`webdriver/tests/classic/`，pytest 寫的 wdspec）裡，對應這些指令的目錄能跑，當作這份 RFC 的驗收。
- wptrunner 在頁面裡執行的 script 需要、kun 還沒有的 DOM：`document.readyState`、`window.outerWidth` 和 `outerHeight`（`details.md` 的「wptrunner 在頁面裡執行的 script」）。

**不在範圍內**：

- **接上 wptrunner、換掉 Rust 的 runner**（下一份 RFC）：kun 的 wptrunner product、預期結果的格式（`-expected.txt` 或 wptrunner 的 `.ini`）、測試清單、CI。這份只做到「wptrunner 能用 kun 跑完一個 testharness 測試和一個 reftest」的冒煙測試；wdspec 要的 product 在這份做（「已決定的事項」第 5 條）。
- **Actions、Element Send Keys、testdriver**（再下一份 RFC）：Perform Actions、輸入來源的狀態、文字輸入（kun 還沒有可以打字的元素）。這份只有 Element Click，和不做事的 Release Actions（wptrunner 每個測試前都呼叫）。
- **WebDriver BiDi**、cookie（kun 沒有 cookie）、user prompt（kun 沒有 `alert`、`confirm`、`prompt`）、Print、最小化和全螢幕、shadow root 的參照（kun 沒有 shadow DOM）、`css selector` 和 `tag name` 以外的找元素方式（第 23 條）、各家的擴充指令（權限、感測器等）。收到時回答規範的錯誤（`unknown command` 或 `unsupported operation`）。

## 設計概要

**給用 WebDriver 的人**：

```sh
machopodriver --port 4444 --binary target/debug/machopo
```

（名字照 chromedriver 的習慣，第 9 條。）在 `127.0.0.1:4444` 上提供 WebDriver；每個 session 啟動一個 kun，瀏覽器是沒有平台視窗的那一個（不開視窗、不要 GPU，和情境測試一樣）；要看得到的話給 kun `--window`（`machopodriver --binary-arg --window`，「已決定的事項」第 3 條）。任何 WebDriver 的 client 都能用（capabilities 給空的，或 `browserName: "kun"`），直接用 HTTP 也行：

```sh
curl -s -X POST http://127.0.0.1:4444/session -d '{"capabilities": {}}'
# {"value": {"sessionId": "…", "capabilities": {"browserName": "kun", …}}}
curl -s -X POST http://127.0.0.1:4444/session/<id>/url -d '{"url": "https://example.com/"}'
curl -s -X POST http://127.0.0.1:4444/session/<id>/execute/sync \
  -d '{"script": "return document.title", "args": []}'
# {"value": "Example Domain"}
curl -s -X DELETE http://127.0.0.1:4444/session/<id>
```

- 一個 window handle 是一個分頁；New Window 開新分頁（`type` 只有 `tab`）。
- 一次一個 session（和 chromedriver、geckodriver 一樣）；Delete Session 結束那一個 kun，下一個 session 啟動新的。
- `navigator.webdriver` 在這個模式下是 `true`。
- 不支援的指令回答 `unknown command` 或 `unsupported operation`，有的指令見 `crates/driver/README.md` 的「有的指令」。

**給改 kun 的人**：多一個 `driver` crate（binary `machopodriver`；HTTP、session、逾時，對應 chromedriver）；kun 多一個 `--automation-pipe` 的模式和一組照 CDP 格式的自動化訊息（`protocol::automation`：開分頁、導覽、視窗大小、截圖、輸入，和 `Kun` domain），renderer 多一組 WebDriver 的訊息（執行 script、找元素、元素的位置、frame、點擊的位置）。情境的執行器也改用同一套自動化訊息（第 12 條）。

## 詳細設計

server 的位置見「已決定的事項」第 7 條。

### driver、kun 和 session

```text
wptrunner / client ──HTTP──▶ driver ──pipe（CDP 的格式，\0 分隔）──▶ kun 的 UI thread ──protocol──▶ renderer
                              session、逾時、           分頁、導覽、視窗、截圖、輸入      執行 script、JSON、
                              implicit wait、等載入                                         元素參照、找元素
```

- **driver**（`driver` crate，binary `machopodriver`，第 9 條；依賴 `protocol`、不帶 server 的 `webdriver` crate，第 8 條，和 `hyper`、`tokio`，第 10 條）：HTTP 那一層是 driver 自己的：`hyper` 的 HTTP/1.1 server 在一條 thread 上跑單執行緒的 tokio，收請求、用 `webdriver` crate 的 `standard_routes()` 比對路徑、`WebDriverMessage::from_http` 解析成指令、檢查 session id，再經過 channel 依序交給另一條 thread 上處理指令的那一部分；回答和錯誤的 JSON 也自己產生，所以能回答規範以外的 `tab crashed`、`disconnected`（第 6、7 條）。處理指令的部分管 session、capabilities、逾時、implicit wait 的重試、page load 的等待和 window handle 的對照，每個指令變成給 kun 的一個或幾個自動化訊息。crate 是 lib（`driver::start`）加 `machopodriver` bin：shell 的整合測試在 process 裡啟動 driver，對 `kun` 的執行檔（別的 package 的測試拿不到 `CARGO_BIN_EXE_machopo`）。
- **kun**：`kun --automation-pipe` 從 stdin 讀自動化的訊息、從 stdout 回答和送事件。stdout 只能有 pipe 的訊息：kun 的 log 改到 stderr，網頁的輸出（`console.log`、沒接住的例外、wasm guest 的 stdout 和 stderr）也是。原本只打算改 log，跑 wdspec 時發現網頁的輸出也在 stdout：一行 `[uncaught] …` 夾在兩則訊息之間，下一則就解析不了、被丟掉（那次是 `Page.lifecycleEvent`，導覽等到逾時）；Chrome 也把 console 的訊息記在它的 log（`--enable-logging=stderr`），不在 stdout。WPT 的網域和根憑證用 kun 的 `--host-resolver-rules`（Chromium 的 switch，語法相同）和 `--additional-trust-anchors` 給，只在 `--automation-pipe` 時收（RFC 0020 的 web test 設定，headless 的 runner 也用同一組）。UI thread 上的 host（`shell` 的 `automation_session`）直接處理自動化訊息（下一節），pipe 是它的第一個入口（`automation_pipe` 模組，對應 Chromium 的 `DevToolsPipeHandler`）；情境的執行器是第二個入口，在同一個 process 裡經過 channel 送同樣的訊息、不序列化成文字，原本的 `Request` 拿掉了（第 12 條）。host 的回答和事件都放進一個 channel（outbox）：pipe 的寫 thread 把它寫到 stdout，情境的執行器直接讀；host 能晚一點回答一個命令（等載入、等輸入處理完、等畫面畫出來）。`--window` 時 host 在真的視窗的事件迴圈裡，pipe 的讀、寫 thread 和沒有平台視窗時一樣。要 DOM 的指令經過 `protocol` 轉給分頁的 renderer。stdin 關掉時 kun 結束。
- **啟動和結束**：New Session 時 driver 啟動 `kun --automation-pipe`（加上 `machopodriver --binary-arg` 給的參數），Delete Session 時送 `Browser.close`、關掉 stdin、等它結束（5 秒內沒結束就 kill）。kun 當掉時 driver 讀到 EOF，之後的指令回答 `disconnected`（chromedriver 的，wptrunner 記成 CRASH）。kun 的分頁從 `about:blank` 開始（原本在導覽之前沒有文件，還沒導覽就執行的 script 無處可跑），是 initial entry：第一次新增 entry 的導覽取代它，所以上一頁回不到它，和 Chrome 新分頁的 initial entry 一樣（`NavigationEntryImpl::IsInitialEntry`）。
- **只聽 `127.0.0.1`**：WebDriver 等於遠端控制瀏覽器，不對外；kun 自己不開 port。driver 另外照 `webdriver` crate 的 server 的規則檢查 `Host` 和 `Origin` header、拒絕 form 和 `text/plain` 的 POST（擋住網頁從瀏覽器裡對它發的請求）。
- **session**：New Session 照規範比對 capabilities（`webdriver` crate 的 `SpecNewSessionParameters`），回答 `browserName: "kun"`、`pageLoadStrategy`、`timeouts`、`unhandledPromptBehavior` 等。每個 session 是一個新的 kun，session 之間不留狀態。
- **錯誤**：規範的錯誤用 `webdriver` crate 的 `ErrorStatus` 決定 HTTP status 和錯誤碼，JSON 由 driver 產生（第 8 條）。分頁的 renderer 當掉時（sad tab）kun 送出通知，之後對那個分頁的指令回答 `tab crashed`（「已決定的事項」第 6 條）。

### 自動化的訊息

driver 和 kun 之間、情境的執行器和 kun 之間的訊息（`protocol::automation`，第 11 條）。照 CDP：

- **格式**：命令 `{id, method, params, sessionId}`，回答 `{id, result}` 或 `{id, error: {code, message}}`，事件 `{method, params, sessionId}`；錯誤碼用 JSON-RPC 的（不是 JSON 或少了欄位是 -32600，找不到方法是 -32601，參數不對是 -32602，其他是 -32000），沒有的 `sessionId` 是 CDP 的 -32001。在 pipe 上每則訊息之後接一個 `\0`，和 Chromium 的 `--remote-debugging-pipe` 一樣（只有 fd 不同：kun 用 stdin 和 stdout，第 7 條）。Rust 這一側是 `Method` 的 enum，方法名稱和它的參數之間的轉換是手寫的（`Method::from_name`、`params`），參數是 serde 的 struct（`deny_unknown_fields`）。
- **名字相同，語意就相同**：CDP 有對應的方法就用它的名字、參數和回答，kun 沒做的參數回答 -32602，不默默忽略。
- **分頁和 session**：照 CDP 的 `Target` domain：`Target.createTarget`、`closeTarget`、`getTargets`、`activateTarget`、`getTargetInfo`（網址、標題），用 `Target.attachToTarget`（`flatten: true`）拿到 `sessionId`，之後對那個分頁的指令都帶它。`targetId` 由 `TabId` 產生。
- **導覽和載入**：`Page.navigate`（回答 `frameId`、`loaderId`；`loaderId` 是 kun 的 navigation id）、`Page.reload`、`Page.getNavigationHistory` 和 `Page.navigateToHistoryEntry`；事件 `Page.lifecycleEvent`（`name` 是 `DOMContentLoaded` 或 `load`，帶 `loaderId`，第 13 條），driver 照 `pageLoadStrategy` 等它。
- **視窗、截圖、輸入**：`Browser.getWindowForTarget`、`Browser.setWindowBounds`；`Page.captureScreenshot`（網頁 view 的 PNG、base64）；`Input.dispatchMouseEvent`（Element Click，第 27 條；收 CDP 的 `buttons`、`clickCount`）。
- **當掉**：事件 `Target.targetCrashed`（分頁的 renderer）；整個 kun 當掉時 pipe 斷掉。
- **`Kun` domain**：CDP 沒有對應的。WebDriver 在 renderer 裡的部分（`Kun.executeScript`、`Kun.findElements`、`Kun.getElementRect`、`Kun.switchToFrame`、`Kun.getElementClickPoint`、`Kun.captureElementScreenshot` 等，第 2 條），經過分頁的 session（第 21 條）；和情境要的視窗層級的操作（第 14 條）：`Kun.navigateFromAddressBar`、`Kun.waitForLoad`、`Kun.dispatchWindowInput`、`Kun.waitForInputHandled`、`Kun.queryElement`、`Kun.scrollIntoViewIfNeeded`、`Kun.captureWindow`、`Kun.getTitle`，不經過 session、作用在目前的分頁。清單見 `crates/shell/README.md` 的「自動化的 pipe」。

### 分頁和 frame

- **window handle**：每個分頁一個，是分頁的 target id（由 `TabId` 產生，不重用）；Get Window Handles 照分頁列的順序。New Window 開 `about:blank` 的新分頁（initial entry，`Target.createTarget` 的 `background`）、不切換過去（規範：新的分頁不取得焦點）；Switch To Window 把分頁切到前面（第 25 條）；Close Window 關掉目前的分頁，關完最後一個時 kun 結束、session 也結束。分頁原本只由 `BrowserView` 的快捷鍵指令開關（`Command::NewTab` 等），`BrowserView` 加了以 `TabId` 為參數的入口（`open_tab`、`close`、`activate`）。
- **目前的 browsing context**：driver 記住「哪個分頁」和「從分頁的 top-level 一路切下來的子 frame」（chromedriver 的 frame 堆疊）。frame 原本跨過邊界時只用 iframe 的 selector 路徑表示（`QueryElement` 的 `frames`），WebDriver 的 Switch To Frame 給的是 iframe 的元素參照或 `window[n]` 的索引：renderer 把它換成 frame 的 id（`Kun.switchToFrame`，renderer 的前綴加上 frame 的號碼；號碼是 frame 第一份文件的 `DocumentId`，frame 導覽時沿用，第 24 條），之後的 `Kun` 方法帶著它。子 frame 被移除或它的上層換了文件時，指令回答 `no such window`（規範）；導覽、切換視窗回到 top-level。元素參照是它所在 frame 的：在別的 frame 用是 `no such element`（wdspec 的 `test_no_such_element_from_other_frame`，iframe 已經移除的也是），同一個 frame 換了文件之後是 `stale element reference`。
- **導覽**：Navigate To、Back、Forward、Refresh 照 `pageLoadStrategy` 等：`normal` 等 `DidFinishLoad`（`load` 事件之後），`none` 不等；`eager` 等 `DOMContentLoaded`：renderer 送新的 `RendererHostMsg::DidDispatchDOMContentLoaded`，kun 轉成 `Page.lifecycleEvent`（第 13 條）。逾時是 session 的 `pageLoad`。Get Title 是分頁的標題（`Target.getTargetInfo`，分頁列上顯示的），不在網頁裡讀 `document.title`。

### 執行 script、JSON 和元素參照

都在 renderer 裡（「已決定的事項」第 2 條），driver 的請求經過 kun 的 UI thread 轉成新的訊息：

```text
RendererMsg::WebDriver { request_id, frame, command }        // browser → renderer
RendererHostMsg::WebDriverReply { request_id, result }       // renderer → browser
```

`command` 是 Execute Script（同步或非同步）、Find Element(s)、Get Element Rect、Element 的 attribute 和 property、Element Click 前的準備（捲進畫面、算中心點、檢查有沒有被擋住）等；參數和結果是和 JSON 同形狀的值（`protocol` 的型別，可以序列化），元素是參照的字串，window 只標出「是這個分頁的 window」，由 browser 換成分頁的 target id（renderer 不知道自己的分頁，第 20 條）。只在 `RendererFlags` 開了 automation 時處理，和現在 automation 的訊息一樣。driver 經過分頁的 session 送 `Kun` 的方法（第 21 條）；WebDriver 的錯誤（`stale element reference` 等）放在 CDP 錯誤的 `data`。

- **執行 script**：照規範的「execute a function body」：在目前 frame 的 realm 裡把 `script` 當成 async 函式的本體編譯（規範現在的寫法，本體裡可以用 `await`）、用反序列化好的參數呼叫，結果本來就是 promise；非同步的版本多一個 resolve 的參數。promise 等到 settle 或 session 的 `script` 逾時才回答（逾時由 driver 計，之後才來的回答丟掉；renderer 只負責 settle 時回報）。
- **JSON 的轉換**（規範的 JSON clone 和反序列化）：走訪 JS 的值，元素換成 `{"element-6066-11e4-a52e-4f735466cecf": id}`、`window` 換成 window 或 frame 的參照、陣列和集合換成陣列、有 `toJSON` 的呼叫它、循環參照是 `javascript error`。要走訪 V8 的值，所以在 `dom_js`（V8 專屬的程式只能在 `bindings`、`dom_js` 和 `dom` 的 `src/v8_engine.rs`）；renderer 經過 `ScriptEngine` 的新方法呼叫，wasm 的 engine 回答不支援（`--script wasm` 時 WebDriver 的 script 指令都是 `unsupported operation`）。engine 和 renderer 之間是 `dom_api` 裡和 engine 無關的值，元素是 `NodeId`；參照的編碼和 stale 的檢查在 renderer（第 20 條）。
- **元素參照**：id 本身帶著位置：renderer 的隨機前綴、document 的 id（`DocumentId`）和節點的 `NodeId`，renderer 不存元素的表（第 19 條；只記得每份給過參照的文件是哪個 frame 的，用來分辨 stale 和 `no such element`）。找節點用 `dom` 現有的弱參照表（`NodeId` 到節點，回收了就查不到），不用新的 GC 機制。前綴不是這個 renderer 的、或 document 不是目前的 browsing context 的，是 `no such element`（規範的「這個 navigable 沒看過」）；document 不是現在的、節點被回收或不在 document 裡，是 `stale element reference`。
- **找元素**：`css selector`（`querySelectorAll` 的同一套 selector）和 `tag name`（第 23 條）；implicit wait 由 driver 重試。
- **元素的框**：Get Element Rect 是 `getBoundingClientRect()` 加上捲動的位移，走 `dom_api` 的 geometry，需要時先更新 layout（第 22 條）。

### 視窗大小和截圖

- **視窗**：Get Window Rect 回答整個 `BrowserWindow` 的位置和大小（包括分頁列和工具列），Set Window Rect 改給了的部分（原本照較舊的規範，`width`、`height` 都給才改大小、`x`、`y` 都給才改位置；wdspec 的 `test_partial_input` 要各自改）（`Browser.setWindowBounds` 的 `left`、`top`、`width`、`height`：`BrowserWindow::resize`，位置在 `BrowserView` 裡；沒有平台視窗時只記下來，真的視窗用 winit 的 `set_outer_position`，之後的 `Moved` 再告訴 `BrowserView`）。`window.outerWidth`、`outerHeight` 是同一個大小，`screenX`、`screenY` 是同一個位置（`RendererMsg::UpdateScreenRects`，第 26 條；每個分頁都收到，背景的和 renderer 重開之後的也是），`innerWidth`、`innerHeight` 是網頁 view 的：wptrunner 的 reftest 用兩者的差，把網頁 view 調成 800×600。
- **截圖**：Take Screenshot 是 `Page.captureScreenshot`：等下一個顯示目前狀態的 frame（`RequestVisualState`，和情境的 `screenshot` 一樣），從 Viz 讀回整個視窗、裁成網頁 view 的範圍（physical px），在 Viz 的 thread 上編成 PNG、base64；不是顯示中的分頁回答錯誤（只有顯示的分頁有 frame，第 25 條）。Take Element Screenshot 是 `Kun.captureElementScreenshot`：renderer 先把元素捲進畫面、回答它在 top-level 的 viewport 裡的那部分，kun 再照樣截、裁成那個範圍。CDP 的 `Page.captureScreenshot` 也有 `clip`，但它的座標是文件的（加上捲動的位移），所以元素的截圖另外放在 `Kun` domain，`Page.captureScreenshot` 不收 `clip`。元素沒有框時是 `unable to capture screen`，HTTP 500（規範；`webdriver` crate 寫成 400，driver 自己蓋過，跑 wdspec 時發現的）。

### Element Click

照規範：捲進畫面、取元素在畫面上的中心點、確認那一點最上面的是這個元素或它的子孫（不然 `element click intercepted`），再從 browser 送 `MouseMove`、`MouseDown`、`MouseUp`，走和平台輸入一樣的路（compositor thread、main thread），等 ack；點了之後開始導覽的話，照 `pageLoadStrategy` 等。情境的 `click` 已經做了前兩步的一部分（`automation.rs` 的 `Runner`），這裡改成在 renderer 一次算好。

做法：driver 先送 `Kun.getElementClickPoint`，renderer 用 `scrollIntoView`（`block: end`、`inline: nearest`，經過外面的 frame）捲動、更新到 pre-paint，取元素的第一個 client rect（經過 iframe，top-level 的 viewport 的 CSS px）在 viewport 裡的部分的中心，取 floor；原本用整個元素的框、不取整數，跑 wdspec 時 `center_point.py` 的 `test_css_pixel_rounding` 和跨兩行的連結（`navigate.py` 的 `test_multi_line_link`）找到；沒有任何部分在 viewport（和外面每個 frame 的 viewport）裡是 `element not interactable`，那一點 hit test 到的（子 frame 裡的換成它的 iframe，像 `elementsFromPoint`）不是元素或它的子孫是 `element click intercepted`。driver 再對分頁送三個 `Input.dispatchMouseEvent`（第 27 條；照 chromedriver，按下時 `buttons` 1、`clickCount` 1，放開時 `buttons` 0），每個都等 renderer 的 ack；放開的回答到的時候，renderer 已經處理完 `click`，它開始的導覽（`Page.frameStartedNavigating`）也已經在回答之前送到，driver 照 page load strategy 等它的 loader。

### DOM：`document.readyState`、`outerWidth`、`outerHeight` 和 `screenX`、`screenY`

wptrunner 每個測試在頁面裡執行 `window-loaded.js` 和 `test-wait.js`，它們先看 `document.readyState`：kun 沒有它時是 `undefined`，script 會改等 `load`，而 `load` 已經發生過，於是一直等到逾時。reftest 用 `outerWidth - innerWidth` 算視窗的邊框，kun 沒有 `outerWidth` 時是 `NaN`。兩個都照規範做，`readyState` 跟著 parser 和 `load` 改變、分派 `readystatechange`（「已決定的事項」第 4 條）。`screenX`、`screenY`（和 `screenLeft`、`screenTop`）原本不在範圍：規範的 window rect 的 x、y 就是它們，wdspec 的 `get_window_rect` 拿它們比對，而第 26 條已經把位置送到 renderer，所以一起做了。`outerWidth` 等都問 window 自己的文件的 frame（`FrameClient::window_screen_rect`），不問 layout，`kun:dom` 的 WIT 也有。

### 測試

- **driver 的整合測試**（`crates/shell/tests/webdriver.rs`）：在 process 裡啟動 driver（它再啟動沒有平台視窗的 kun；放在 `shell` 是因為別的 package 的測試拿不到 `CARGO_BIN_EXE_machopo`），用 HTTP 送指令、比對回答，涵蓋每個指令的正常路徑和錯誤；頁面用 `net::test_server` 提供。driver 自己的單元測試是路由、`Host` 等檢查和參數。跟著 `cargo test --workspace` 跑。
- **kun 這一側的自動化訊息**（`crates/shell/tests/automation_pipe.rs`）：不經過 driver，直接從 pipe 送訊息給 `kun --automation-pipe`，涵蓋分頁、導覽、視窗、截圖、輸入、`Kun` domain，以及分頁的 renderer 當掉、`Browser.close` 和 stdin 關掉時的結束（整個 kun 結束時 driver 回答 `disconnected` 沒有測試，`docs/test-gaps.md` 的「能補、還沒補的測試空缺」）；renderer 那一側（JSON 的轉換、元素參照的 stale 和別的 navigable、frame、點擊的位置）也在這裡經過 pipe 測，和規範的錯誤一起比對。
- **WPT 的 wdspec**（`webdriver/tests/classic/` 裡對應這份範圍的目錄，`details.md` 的「WPT 的 WebDriver 測試」）：當作和規範一致的驗收，經過 kun 的 wptrunner product 跑（「已決定的事項」第 5 條）。不經過 `wpt run`（它要系統的 hosts 檔有 WPT 的網域），由 kun 的腳本直接呼叫 wptrunner，像 Chromium 的 blinkpy；wptserve bind 在 `127.0.0.1`，manifest 和 bytecode 都不寫進 checkout（`details.md` 的「第 0 步的實測」）。
- **冒煙測試**：`python3 scripts/test.py smoke` 用 wptrunner 跑一個 testharness 測試和一個 reftest，兩個都要 PASS（第 28 條），確認整條路通：session、每個測試的新分頁、`window-loaded.js` 和 `test-wait.js`、Element Click、把 viewport 調成 800×600 的 Set Window Rect、Take Screenshot。
- **情境**：情境的執行器經過自動化的 session 跑（第 12 條），全部情境（`python3 scripts/test.py scenarios`）和真的視窗（`python3 scripts/test.py automation`）就是這條路的回歸測試；執行器自己的測試在 `crates/shell/tests/windowless.rs`。不新增情境：這份在視窗裡看不到新的行為（`--window` 只是換個瀏覽器的模式）。`--window` 經過 pipe 的測試要顯示器，只在設了 `MACHOPO_WINDOW_TESTS` 時跑，`test.py automation` 跑完情境之後跑它。
- mutation test 照 `docs/testing.md`。

### 分階段

每一段做完時 commit、審核；四段都做完之後 rebase 到 `main`、一起 merge 回去。

0. **實測**（原型在 `step0-prototype/`，不是 workspace 的一部分）：`webdriver` crate 0.54 用 kun 的 toolchain 能不能編、帶進多少新的依賴和 binary 的大小；wptrunner 能不能從 checkout 外面載入 kun 的 product（`wptrunner.products` 的 entry point），跑一個 wdspec 的目錄。結果在 `details.md` 的「第 0 步的實測」：兩件都可以；`webdriver` crate 的錯誤碼是封閉的，因此不用它的 server（第 8 條）。
1. **骨架**：`driver` crate、`kun --automation-pipe`、自動化訊息的 host（`Target`、`Page` 的導覽和 `lifecycleEvent`、`Target.targetCrashed`）、server、session、Status、Timeouts、Navigate To、Get Current URL、Back、Forward、Refresh、Get Title、單一分頁的 window handle；`navigator.webdriver`；`document.readyState`。kun 的 wptrunner product 和跑 wdspec 的 `test.py wdspec`（`tests/wpt/README.md`），這一段的 wdspec 目錄和它們的預期結果。
1b. **情境改用自動化訊息**：情境的執行器在同一個 process 裡經過 channel 送自動化訊息，拿掉 `automation.rs` 的 `Request`；`Kun` domain 補上情境要的視窗層級的操作；`kun --automation-pipe --window`（真的視窗，第 3 條）。驗收是全部情境和真的視窗都過。`--window` 原本在第 1 段，它要改 `window.rs` 的事件迴圈（視窗那一層），和這段改寫情境的入口一起做。開始前決定第 14 條；做的時候 `wait-loaded` 改成 `Kun.waitForLoad`（第 16 條），做完時決定第 18 條。
2. **執行 script 和元素**：renderer 的訊息、Execute Script（同步和非同步）、JSON 的轉換、元素參照、Find Element(s)（也從元素開始找）、Get Element Rect、Tag Name、Attribute、Property。開始前決定第 19 到 23 條，做的時候多了 `about:blank` 的 initial entry（「driver、kun 和 session」）。
3. **分頁、frame 和視窗**：New Window、Close Window、Switch To Window、Get Window Handles、Switch To Frame、Switch To Parent Frame、Get / Set Window Rect；`outerWidth`、`outerHeight`。開始前決定第 24 到 26 條；做的時候多了 `screenX`、`screenY`（和 `screenLeft`、`screenTop`）：規範的 window rect 的 x、y 就是它們，wdspec 的 `get_window_rect` 拿它們比對，而第 26 條已經把位置送到 renderer。session 從這一段起回答 `setWindowRect: true`。
4. **截圖和點擊**：Take Screenshot、Take Element Screenshot、Element Click、Release Actions；wptrunner 的冒煙測試。開始前決定第 27、28 條。

四段做完之後 rebase 到 `main`：`main` 在舊的情境 `Request` 上加的 `dblclick` 和 `wait-title` 移到自動化訊息上（`Kun.dispatchWindowInput` 的 `type: "dblclick"`、`Kun.getTitle`）；RFC 0022 的滑鼠事件帶 `buttons` 和連點次數，`Input.dispatchMouseEvent` 跟著收 CDP 的 `buttons`、`clickCount`。

## 缺點

- **工程量大，testdriver 要等很久**：這份做完還不能解開任何 testdriver 的測試，要再等接上 wptrunner 和 Actions 兩份。
- **一個新的對外介面**：WebDriver 是遠端控制瀏覽器的 port，只聽 `127.0.0.1`、只在執行 driver 時開，但仍然是要長期維護、要注意安全的東西。
- **兩個 binary 和一套內部的協定**：driver 和 kun 要一起編、一起測，自動化的訊息兩邊各實作一次（同一個 repo、一起改，不用處理版本的相容）。
- **新的依賴**：不帶 server 的 `webdriver` crate 帶進 7 個 workspace 沒有的 crate（`cookie`、`time` 等，第 8 條）；授權是 MPL-2.0（以檔案為單位的 copyleft：改了它的檔案要公開，只是依賴不受影響）。
- **跨 process 邊界的新訊息**：driver 和 kun 之間的自動化訊息，以及執行 script 的結果、元素參照、frame 的 id 都要能序列化。元素參照不另外存表，靠 `dom` 現有的 `NodeId` 弱參照表（第 19 條），沒有新的 GC 機制。

## 理由和其他做法

完整的比較在 `details.md` 的「已決定的事項的選項」。

- **為什麼是 WebDriver，不是 kun 自己的 testdriver 通道**：見「動機」。
- **為什麼 classic，不是 BiDi**：wptrunner 跑 testharness、reftest、crashtest 只要 classic（Ladybird 的 product 就是 classic）；BiDi 要 WebSocket 和事件，只有 testdriver 的少數 API（權限、emulation 等）要它。
- **為什麼先做這一份、不和接上 wptrunner 合併**：換掉整套跑法時，WebDriver 已經被 wdspec 驗證過，結果的差別才能歸到 runner 而不是 WebDriver 的 bug。

## 先例

各家的做法比較在 `details.md` 的「各家的做法」。

- **Chromium**：chromedriver 是獨立的 process，經過 CDP 控制 Chrome；執行 script 用注入的 JS（`call_function.js`，元素的快取也在裡面）；視窗、截圖、輸入在 browser process（`Target.createTarget`、`Page.captureScreenshot`、`Input.dispatchMouseEvent`）。照做的部分：獨立的 driver（「已決定的事項」第 7 條）。照做的部分還有：driver 和 kun 之間的訊息照 CDP 的格式和方法（第 11 條）。不照做的部分：kun 只做 WebDriver 和情境用到的 CDP 方法，執行 script 不經過 CDP 的 `Runtime`，在 `Kun` domain（第 2、11 條）；注入 JS 的元素快取看得到（「已決定的事項」第 2 條）。
- **Servo**：server 做在瀏覽器裡（`--webdriver`），用 `webdriver` crate；指令經過 channel 交給 embedder，要 DOM 的交給 script thread（`webdriver_handlers.rs`）。`webdriver` crate 和 Servo 一樣；server 的位置不照做（「已決定的事項」第 7 條）。
- **Ladybird**：獨立的 `WebDriver` 程式啟動瀏覽器，指令經過 IPC 交給 WebContent，規範的演算法幾乎都在 WebContent 裡用 C++ 做（`LibWeb/WebDriver/`）；元素參照存在節點上。照做的部分：獨立的 driver 在 New Session 時啟動瀏覽器、規範的演算法在 renderer 裡用原生的程式做。不照做的部分：kun 的 driver 只和 browser 溝通，由 browser 轉給 renderer（Ladybird 的 driver 直接連每個 WebContent）。
- **Firefox**：geckodriver（Rust、`webdriver` crate）轉成 Marionette 的 JSON 協定，script 在 content process 的 sandbox 裡執行，元素參照在每個 process 的 `NodeCache`（弱參照加 UUID）。kun 不照這個存表，id 本身帶著位置（第 19 條）。
- **WebKit**：`WebKitWebDriver` 經過 remote inspector 的 Automation domain，視窗和輸入在 UIProcess，script 和元素參照在 WebProcess（注入的 JS）。

共同點：執行 script、JSON 的轉換和元素參照都在 DOM 旁邊（renderer 那一側），視窗、截圖和輸入在 browser 那一側（Ladybird 的輸入除外）。這份照這個分工。

## 已決定的事項

2026-10-08 確認：

1. **HTTP 和指令的解析用 Mozilla 的 `webdriver` crate**（H1），不自己寫（H2）：解析、錯誤的格式和 capabilities 的比對不是 kun 要做出差別的地方，wdspec 會逐條檢查；geckodriver 和 Servo 在同樣的位置用它。第 0 步確認用 kun 的 toolchain 能編（`details.md` 的「第 0 步的實測」）；它的錯誤碼是封閉的，所以只用它的指令解析、型別和 capabilities，不用它的 server（第 8 條）。
2. **renderer 這一側用 Rust 照規範的演算法執行 script、做 JSON 的轉換、放元素參照**（R1，像 Ladybird 和 Servo），不注入 JS（R2，Chromium、WebKit）：kun 沒有 isolated world，注入的 JS 和網頁共用全域，網頁看得到、改得到它（Chromium 早期的 `$cdc_` 就被拿來偵測 chromedriver），網頁改掉的 `Array.prototype`、`JSON` 也會影響結果；Rust 的版本容易寫單元測試。走訪 V8 的值的部分在 `dom_js`。
3. **瀏覽器預設是沒有平台視窗的 `BrowserWindow`**（W1），`--window` 換成真的視窗（W3）；不用 headless 的 `WebContents`（W2，沒有分頁，New Window 做不出來）。W1 和 W3 是同一個 `BrowserWindow`，WebDriver 的程式不分兩套；CI 上只跑 W1。
4. **`document.readyState`、`window.outerWidth`、`outerHeight` 在這份做**（P1），不加進 RFC 0017（P2，已接受，改範圍要重新確認），也不另開 RFC（P3）：三個都小，WebDriver 的導覽本來就要 document 的 readiness。
5. **kun 的 wptrunner product 在這份做，只用來跑 wdspec；wdspec 的預期結果用 wptrunner 原生的 `.ini`**（T1），不把 wdspec 全部留到下一份（T2）：只靠自己寫的測試很難說和規範一致。testharness 和 reftest 的格式留給下一份 RFC。第 0 步確認 product 能從 checkout 外面載入（上游有文件的 `wptrunner.products` entry point），但 `wpt run` 對內建以外的 product 要求系統的 hosts 檔有 WPT 的網域，所以由 kun 的腳本直接呼叫 wptrunner（`details.md` 的「第 0 步的實測」）。
6. **分頁的 renderer 當掉時回答 chromedriver 的 `tab crashed`**（C1，HTTP 500），不回答規範的 `unknown error`（C2）：wptrunner 看到它才記成 CRASH，能分辨 renderer 當掉和 WebDriver 的 bug。`webdriver` crate 的錯誤碼是封閉的，所以錯誤的 JSON 由 driver 自己產生（第 8 條）。
7. **server 是獨立的 driver 程式，自動化訊息經過 stdin 和 stdout 的 pipe**（S2，像 Ladybird、WebKit、Firefox；Chromium 也是獨立的 chromedriver）。不做在 shell 裡（S1，Servo）：瀏覽器當掉時 server 會一起消失，回答不出錯誤，session 之間也要另外清掉狀態；S2 每個 session 一個新的 kun，kun 當掉時 driver 還在，回答 `disconnected`。不用 CDP 的子集（S3，Chromium）：CDP 太大、是 Chromium 的形狀，而且它的 `Runtime` 只回答 RemoteObject，JSON 的轉換和元素參照會落到注入的 JS，和第 2 條衝突。不先放在 shell 裡再拆（S4）：省下的只有傳輸和啟動，不多。傳輸不用額外的 fd（Chromium 的 `--remote-debugging-pipe`，要 `unsafe`）、本機的 TCP（多一個 port、要驗證對方）或還沒有的 `ipc` crate（RFC 0004）；pipe 只有 driver 連得到，stdin 關掉時 kun 結束。代價是 kun 的 log 和網頁的輸出（`console.log`、沒接住的例外）要從 stdout 改到 stderr（「driver、kun 和 session」）。訊息的格式原本定為「一行一個 JSON、kun 自己的訊息」，第 11 條改成 CDP 的格式、`\0` 分隔；這裡不選的 S3 是「driver 經過 CDP 的 `Runtime` 執行 script」，第 11 條只借 CDP 的格式和 CDP 有的方法，執行 script 照第 2 條在 `Kun` domain。
8. **不用 `webdriver` crate 的 server，driver 自己做 HTTP 那一層**（E2，`default-features = false`，只用它的指令解析 `standard_routes()`、`WebDriverMessage::from_http`、型別和 capabilities；2026-10-08，第 0 步之後）。第 0 步找到它的錯誤碼是封閉的 enum，用它的 server 回答不了第 6、7 條的 `tab crashed`、`disconnected`，而 wptrunner 只在這兩個錯誤碼時把測試記成 CRASH。用它的 server、回答規範的錯誤（E1，geckodriver、Servo）：分頁當掉時 wptrunner 記成 INTERNAL-ERROR，和 WebDriver 自己的 bug 分不開；kun 和 Chromium 一樣是「分頁當掉、瀏覽器還在」，回報要和 chromedriver 一致。不改 crate（E3）：多維護一個 fork，規範沒有這兩個錯誤碼，上游不會收。HTTP 那一層小（crate 的 `server.rs` 約 300 行做的事），而且少 12 個依賴（warp 等）；chromedriver 也是自己的 HTTP server。
9. **driver 是 `crates/` 底下的 `driver` crate，binary 叫 `machopodriver`**（N3）；kun 這一側是 `--automation-pipe`、`protocol::automation`、`shell` 的 `automation_pipe` 模組。binary 的名字照 chromedriver 的習慣；crate 叫 `driver`，和 workspace 裡其他 crate 一樣用功能命名（不叫 `machopodriver`、N1；也不叫 Servo 的 `webdriver_server`、N2）。
10. **driver 的 HTTP/1.1 用 `hyper`**（H-a，`hyper::server::conn::http1`，一條 thread 上跑單執行緒的 tokio，處理指令的部分在另一條 thread），不只用標準函式庫（H-b）：workspace 已經有 `hyper`、`hyper-util`、`tokio`（`reqwest` 帶進來的），只多開 server 的 feature，不多 crate；keep-alive、chunked 的 body、pipelining 交給它，wdspec 也有直接送原始 HTTP 的測試。Chromium 的 chromedriver 用 Chromium 自己的小型 HTTP server（`net/server/http_server.h`）。
11. **自動化訊息照 CDP**（M-a）：格式是 CDP 的（`{id, method, params, sessionId}`、`{id, result}` 或 `{id, error}`、事件 `{method, params, sessionId}`），pipe 上用 `\0` 分隔（和 Chromium 的 `--remote-debugging-pipe` 一樣）；CDP 有對應的方法就用它的名字和語意（`Target`、`Page`、`Browser`、`Input`），名字相同、語意就相同，沒做的參數回答錯誤；CDP 沒有的放 kun 自己的 `Kun` domain（WebDriver 在 renderer 裡的部分、情境的視窗層級操作）。不選只有一問一答、kun 不主動送訊息（M-b）：等載入的邏輯會搬到 kun，和第 7 條的分工不合，分頁當掉、網頁自己開分頁也通知不了。熟悉 CDP 的人一看就懂，之後接 Puppeteer 或 DevTools 時有一部分現成。代價：`Target` 和 session 多一層（先 attach、拿 `sessionId`），`frameId`、`loaderId` 是字串。
12. **情境和 pipe 共用同一個 host 和同一套自動化訊息**（A-c），分兩段：第 1 段的 host 直接處理自動化訊息、pipe 是第一個入口，情境照舊；第 1b 段情境的執行器改用自動化訊息（同一個 process 裡經過 channel，不序列化），拿掉 `automation.rs` 的 `Request`。不選擴充現在的 `Request`（A-a）或 pipe 另外一個 host（A-b，Chromium 的 interactive_ui_tests 和 CDP 是兩條路，只在 `WebContents`、`RenderWidgetHost` 會合）：只留一個 host、一套協定，情境也成為自動化協定的回歸測試。分兩段是為了不讓改寫情境（視窗那一層唯一的回歸測試）和 WebDriver 的骨架混在一起，兩套並存的時間只有一段。代價：情境大多是視窗層級的操作，要放進 `Kun` domain；改寫執行器要跑全部情境和真的視窗。
13. **`eager` 等 `DOMContentLoaded`，renderer 送 `RendererHostMsg::DidDispatchDOMContentLoaded { navigation_id }`**（Q-a），kun 轉成 `Page.lifecycleEvent`；不當作 `normal`（Q-b，回答得比規範晚）。對應 Chromium 的 `DidDispatchDOMContentLoadedEvent`（`RenderFrameHostImpl`，`WebContentsObserver::DOMContentLoaded`）和 CDP 的 `Page.domContentEventFired`；renderer 本來就知道這個時間點，第 1 段也要照 parser 的進度更新 `document.readyState`。
14. **情境的操作放進 `Kun` domain，一個方法對應情境的一種請求**（A，2026-10-08，第 1b 段開始前）：`Kun.navigateFromAddressBar`、`Kun.dispatchWindowInput`（點擊、滾輪、按鍵，視窗的 CSS px）、`Kun.queryElement`（selector 加 iframe 的 selector 路徑，回答視窗座標的框和 `textContent`）、`Kun.scrollIntoViewIfNeeded`、`Kun.waitForInputHandled`、`Kun.captureWindow`，都不經過 session、作用在目前的分頁；視窗的大小用 CDP 的 `Browser.getWindowForTarget`、`Browser.setWindowBounds`。不盡量用 CDP（B：`DOM` domain 的 node id、`Runtime`、`Input.dispatch*`，只有分頁列、工具列、網址列和整個視窗的截圖放 `Kun`）：要做 `DOM` domain 和 `Runtime`（後者和第 2 條衝突），每個分頁各管一個 session，情境的 frame 路徑要改寫成 node id 的走訪；情境測的本來就是視窗層級的行為。Chromium 的 interactive_ui_tests 也不走 CDP（`ui_controls` 從 OS 層送輸入、`ui_test_utils` 查狀態）。
15. **視窗的截圖照 JSON 傳：寬、高和 base64 的原始 RGBA**（P1），pipe 和同一個 process 裡同一條路；不另外開不經過 JSON 的二進位欄位（P2）。CDP 的截圖也一律是 base64 的字串。做的時候量過：每張截圖多 base64 和 JSON 的 2 到 16 ms（800×600 到 1600×1200），每張本來就等一個 frame，全部情境的時間看不出差別，所以照 P1（數字在 `progress.md`）。
16. **等待的分工**：等輸入處理完、等畫面畫出來只有 browser 知道，是 `Kun` 的方法，條件到了才回答（host 要能晚一點回答一個命令）。情境的 `wait-loaded` 原本定為在執行器裡用事件等（`Page.frameStartedNavigating`、`Page.lifecycleEvent`），和第 7 條 driver 的等待一樣；**做的時候改成 `Kun.waitForLoad`**（2026-10-08，第 1b 段）：事件只送給 attach 了的 session，執行器看不到 attach 之前、或別的分頁裡開始的導覽，例如連結開的新分頁的第一次載入（情境 `link-tabs`），也就等不到它；`wait-loaded` 要的是「目前的分頁最新的導覽載入完了」，這只有 host 知道（每個分頁最後一次 `DidFinishLoad` 的 navigation id），和原本 `Request::WaitLoaded` 的做法相同。driver 的等待照舊用事件：它等的是自己開始的導覽，`loaderId` 在回答裡。
17. **`--window`（真的視窗）被自動化驅動時，擋掉平台的輸入**，和 `kun --automation` 一樣：自動化的結果才穩定，`--window` 是讓人看它在做什麼。不照 Chrome 被 chromedriver 驅動時那樣讓人一起操作。
18. **`Browser.getVersion` 的 `product` 照 Chrome 的 headless 模式分開**（A，2026-10-08，第 1b 段做完時）：沒有平台視窗是 `Headlesskun/版本`（Chrome 是 `HeadlessChrome/…`），`--window` 的真視窗是 `kun/版本`；網頁的 user agent 兩種都一樣（Chrome 的 headless 連 UA 也改，kun 不改：情境和 WPT 的頁面看到的要相同）。為了讓測試分得出兩種瀏覽器：第 1b 段的 mutation test 把 `--window` 弄壞（一律用沒有平台視窗的）時，兩者對 pipe 的回答完全一樣，測試抓不到。不選從 OS 外面查頂層視窗（B：每個平台一套，和 pipe 無關），也不選讀 stderr 的 log（C：測試綁在 log 的文字上）。machopodriver 不讀 `product`。

19. **元素參照的 id 本身帶著位置，renderer 不存表**（E-a，2026-10-08，第 2 段開始前；RFC 原本留到這段決定）：id 是 renderer 的隨機前綴（每個分頁一個 renderer，當掉之後是新的）、document 的 id 和節點的 `NodeId`，節點用 `dom` 現有的 `NodeId` 弱參照表找。對應 Chromium：chromedriver 現在的元素 id 也是 frame、document（loader）和 backend node id 拼起來的，不在網頁裡快取；Ladybird 也是。不選照規範字面的 UUID 加 renderer 的表（E-b，Firefox 的 `NodeCache`）：表要跟著導覽和 DOM 的重建（換 script 語言時 `NodeId` 重新編號）處理，跟著 document 丟掉的話導覽之後舊的參照會變成 `no such element`，wdspec 要的是 `stale element reference`；E-a 的 document id 不重用，舊的參照自然是 stale。不選在 `dom` 新增公開的弱參照型別（E-c）：要新的 GC 論證，沒有好處。
20. **engine 和 renderer 之間是 `dom_api` 裡和 engine 無關的值**（J-a）：null、bool、數字、字串、陣列、物件、元素（`NodeId`）、window。engine 只做 JS 的值和它之間的 JSON clone（走訪、`toJSON`、循環）；參照的編碼、stale 的檢查在 renderer，window 參照的 handle 由 browser 填（renderer 不知道自己的分頁）。參數反方向：renderer 先把參照換回節點、錯誤先回答，再交給 engine。對應 Chromium：V8 的 inspector 深層序列化值、節點只給 backend node id，外面那層再換成 WebDriver 的參照。不選 engine 直接產生最後的 JSON（J-b）：每個 engine 都要重寫一次參照的規則。
21. **WebDriver 的 `Kun` 方法經過 session**（K-a）：`Kun.executeScript`、`Kun.findElements` 等帶 `sessionId`，作用在 attach 的分頁，和 `Page.*` 一樣；driver 照 window handle 管分頁。和第 14 條（情境的方法作用在目前的分頁）不同：情境測的是視窗，WebDriver 要指定分頁。對應 Chromium：chromedriver 的 `Runtime.callFunctionOn` 也經過分頁的 session。不選作用在目前的分頁（K-b）。WebDriver 的錯誤放在 CDP 錯誤的 `data`（CDP 的錯誤本來就有這一欄）。
22. **Get Element Rect 照規範算：`getBoundingClientRect()` 加上捲動的位移**（G-a），走 `dom_api` 的 geometry（需要時先更新 layout），和網頁的 script 看到的一樣；chromedriver 也是在頁面裡呼叫 `getBoundingClientRect()`。不用情境的 `queryElement` 那一套（G-b）：是上一次 pre-paint 的框，可能是舊的，也不是規範的定義。
23. **找元素支援 `css selector` 和 `tag name`**（L-b）：`tag name` 和 `getElementsByTagName` 同一套比對，幾乎不多寫，wdspec 的 find 測試大多會對每種方式各跑一次。`link text`、`partial link text` 要元素畫出來的文字（`innerText`），kun 還沒有；`xpath` 要 XPath。這三種回答 `unsupported operation`（L-c 不選）。Chromium 五種都支援（`xpath` 用頁面的 `document.evaluate`，連結文字用 `innerText`）。
24. **frame 的 id 是 renderer 給每個 frame 的號碼，跨過那個 frame 自己的導覽**（F-b，2026-10-09，第 3 段開始前；RFC 原本留到這段決定）：renderer 建子 frame（iframe 的 browsing context）時給它一個號碼，子 frame 導覽、換上新的 `FrameView` 時沿用；對外的 id 是 renderer 的前綴加上號碼，找 frame 時走 frame tree，不另外存表。driver 記住 session 從最上層一路切下來的 frame（像 chromedriver 的 frame 堆疊），WebDriver 的 `Kun` 方法帶著目前的那一個；子 frame 的 window 參照（`{"frame-075b-…": id}`）也是這個 id。對應 Chromium 的 frame token（`FrameTreeNode` 在 frame 導覽時不變，chromedriver 用的 CDP `frameId` 就是它）和 Firefox 的 `BrowsingContext` id。不選父 document 的 `DocumentId` 加 iframe 的 `NodeId`（F-a，不加任何狀態）：同一個 iframe 移除又插入時 HTML 建新的 browsing context，這個 id 卻還指得到它（規範要 `no such window`）。不選子 frame 目前 document 的 `DocumentId`（F-c）：子 frame 一導覽 id 就失效，wdspec 的 `test_find_element_while_frame_is_still_loading` 先切進還在初始 `about:blank` 的 iframe，等它載入另一份文件再找元素。
25. **Switch To Window 把那個分頁切到前面**（T-a，2026-10-09，第 3 段開始前）：driver 送 `Target.activateTarget`，kun 讓它成為目前的分頁。背景分頁收到 `WasHidden`，沒有 `BeginFrame`：rAF 不跑，截圖也要畫出來的分頁。chromedriver（`ActivateWebView`）和 Firefox 切換時也這樣做。New Window 照規範在背景開，不切換、也不切到前面。不選只記在 session（T-b）：比較接近規範字面（不改 OS 層級的焦點），但被切換到的背景分頁 rAF 和截圖都會卡住。
26. **`outerWidth`、`outerHeight` 來自新的 `RendererMsg::UpdateScreenRects`**（O-b，2026-10-09，第 3 段開始前）：browser 在視窗的位置或大小改變時，把視窗的 rect（CSS px）送給每個分頁的 renderer，對應 Chromium 的 `blink.mojom.Widget::UpdateScreenRects` 的 `window_screen_rect`（Blink 的 `ChromeClient::RootWindowRect`）；`screenX`、`screenY` 也用它（做的時候一起加了）。不選加進 `VisualProperties`（O-a，改動最少）：和 Chromium 的分法不同，只移動視窗也得重送整份。
27. **Element Click 的滑鼠事件直接送給分頁**（I-a，2026-10-09，第 4 段開始前）：CDP 的 `Input.dispatchMouseEvent`，經過分頁的 session，座標是網頁 view 的 CSS px；kun 交給那個分頁的 `WebContents::forward_input_event`，renderer 處理完（ack）才回答。對應 Chromium DevTools 的 `InputHandler`，它直接送給 `RenderWidgetHost`，不經過瀏覽器的 UI。不選經過視窗（I-b，像情境的 `Kun.dispatchWindowInput`，交給 `BrowserView` 當成平台的輸入）：要靠分頁正在顯示、網頁 view 在視窗裡的位置，也不是 CDP 的形狀。
28. **wptrunner 的冒煙測試是 `test.py` 的固定指令**（K-b，2026-10-09，第 4 段開始前）：用 wptrunner 跑一個 testharness 測試和一個 reftest，兩個都要 PASS；下一份接上 wptrunner 的 RFC 取代它之前，改到 driver、自動化訊息或 kun 的 product 時拿它確認整條路還通。預期結果的格式照第 5 條留給下一份 RFC，這裡只要求 PASS。不選只在第 4 段結束時手動跑一次（K-a）：之後改到這條路時沒有現成的檢查。

## 待決定

沒有。

## 之後的可能

- **接上 wptrunner**（下一份 RFC）：kun 的 product、預期結果的格式、測試清單、CI 和分片；用 `wpt run` 跑 testharness、reftest、crashtest，換掉 `4ff8d6e:crates/shell/tests/wpt_testharness.rs` 和 `reftest.rs`，輸出 wptreport。
- **Actions 和 testdriver**（再下一份）：Perform Actions（pointer、key、wheel）、Element Send Keys、Element Clear；要先有文字輸入和 `postMessage`（wptrunner 回覆 testdriver 的動作時用；焦點 RFC 0022 做了）。HTML 的 user activation（testdriver 的 `bless`）也在那時候決定。
- **WebDriver BiDi**：testdriver 只有 BiDi 才有的部分（權限、emulation、user prompt 的事件）。
- **cookie、user prompt、Print**：kun 有了 cookie、`alert`、分頁的 layout 之後。
- **Puppeteer 和 DevTools**：自動化訊息已經是 CDP 的格式；還缺 Puppeteer 的傳輸（fd 3 和 4 的 pipe，或 WebSocket）和它用到的其他方法（`Runtime`、`DOM` 等）。
