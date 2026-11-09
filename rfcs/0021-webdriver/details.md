# RFC 0021 的細節：WebDriver（classic 的子集）

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

除非另外註明，wptrunner 和 wdspec 的檔案都是 `../third_party/wpt`（RFC 0020 的 commit）裡的，路徑從 `tools/wptrunner/wptrunner/` 算起；2026-10-08 整理。

## wptrunner 用到的指令

### 啟動和 session

- **啟動**（`browsers/base.py` 的 `WebDriverBrowser`）：用 product 的 `make_command()` 啟動 driver，port 由 wptrunner 找一個空的；`wait_for_service`（`environment.py`）只反覆試著用 TCP 連上，連上就算好了。`stop()` 是 kill（等 5 秒）；process 層級的 `is_alive` 是 process 還在不在。
  - Ladybird（`browsers/ladybird.py`）：`[webdriver_binary, "--port", N] + webdriver_args`，capabilities 是 `{}`，用通用的 `WebDriverTestharnessExecutor`、`WebDriverRefTestExecutor`、`WebDriverCrashtestExecutor`，wdspec 用 `PytestExecutor`。
  - Servo（`browsers/servo.py`）：driver 就是瀏覽器本身：`[binary, "--webdriver=N", "--hard-fail", "--window-size", "800x600", "about:blank", …]`；另外用 `GET /status` 判斷活著、用擴充的 `DELETE /session/x/servo/shutdown` 正常結束、每個測試之間用 `servo/cookies/reset` 清 cookie（`executors/executorservo.py`）。
- **session**（`executors/executorwebdriver.py` 的 `WebDriverProtocol.connect`）：`POST /session`，`{"capabilities": {"alwaysMatch": caps}}`。回答要有 `sessionId`（字串）和 `capabilities`（物件），不能有 `webSocketUrl`（`tools/webdriver/webdriver/client.py` 的 `Session.start`）。結束時 `DELETE /session/{id}`。
- **活著嗎**（`WebDriverProtocol.is_alive`）：`GET window/handles`，2 秒逾時。出錯時，HTTP 500 加上 `error` 是 `disconnected` 或 `tab crashed`（chromedriver 的，不在規範裡）記成 CRASH；其他的錯誤，process 還在時是 INTERNAL-ERROR，不在時是 CRASH。

### 每個測試

- **逾時**：`POST timeouts {"script": 測試的逾時 × multiplier + 5}`（測試的逾時 10 秒，`long` 是 60 秒）；Python 那一側另外看門 `timeout + 10`。`script timeout` 和 `timeout` 的錯誤記成 EXTERNAL-TIMEOUT。
- **runner 的分頁**（`load_runner`）：導覽到 `/testharness_runner.html`（內容幾乎是空的），`GET window` 記下它的 handle，`execute/sync` 執行 `runner.js`（只設 `document.title`）。

**testharness**（`do_testharness`）：

1. `close_old_windows`：`DELETE actions`（Release Actions）；runner 以外的每個 handle 都 `POST window` 切過去、`DELETE window` 關掉，回答的 handle 清單裡不能再有它；`POST window` 切回 runner。
2. `create_test_window`：`POST window/new {"type": "tab"}`、切過去；`execute/async` 執行 `window-loaded.js`（等 `load`）；`execute/sync` 執行 `return document.documentElement`，回答要是元素參照；`POST element/{id}/click` 讓這個分頁取得焦點。
3. `POST window/rect`，大小是一開始 `GET window/rect` 拿到的（出錯不管）。
4. `testdriver.run`：`POST url` 導覽到測試（`normal` 的 page load strategy，等到載入完才回答）；然後反覆 `execute/async` 執行 `testharness_webdriver_resume.js`，參數是 `[path]`：它把最後一個參數（resolve）存成 `__wptrunner_testdriver_callback`，呼叫 `__wptrunner_process_next_event()`。頁面的 `/resources/testharnessreport.js` 換成 wptrunner 的版本，測試結束時把 `{type: "complete", tests, status}` 放進 `__wptrunner_message_queue`（`executors/message-queue.js`）；callback 拿到 `[url, "complete" 或 "action", payload]`。
   - `action`（testdriver 的請求）：Python 執行對應的 WebDriver 指令，再 `execute/sync` 執行 `window.postMessage({cmd_id, type: "testdriver-complete", status, message}, "*")` 回覆，接著重新進入 resume 的 script。這一步要 `postMessage`，是 Actions 那一份 RFC 的事。
   - 回答是 `null`（執行期間跳出了 user prompt）時跳過；不是三個元素的陣列時檢查 `is_alive`。

**reftest**（`WebDriverRefTestExecutor`）：

- 不開新分頁，直接在 runner 的分頁導覽。
- 每個測試：`execute/sync` 拿 `[outerWidth - innerWidth, outerHeight - innerHeight]`；`POST window/rect {x: 0, y: 0}`；`POST window/rect {width: 800 + dw, height: 600 + dh}`，讓 viewport 剛好 800×600。
- 測試和每個參考頁：導覽，再用 `test-wait.js` 跑 resume 的迴圈：等 `load`、`document.fonts.ready`（沒有 `document.fonts` 時跳過）、兩次 `requestAnimationFrame`，有 `reftest-wait` 時用 `MutationObserver` 等它拿掉（並分派 `TestRendered`），再送 `complete`。
- `GET screenshot`：viewport 的 PNG，base64（可以帶 `data:image/png;base64,` 的前綴，會被拿掉）；圖做 hash 再比對。viewport 要是 800×600、device pixel ratio 是 1。

**crashtest**：和 reftest 一樣，等的 class 是 `test-wait`；沒有出錯就是 PASS。

**testdriver 的動作**（`executors/actions.py`，Actions 那一份 RFC 的範圍）：`click` 是 `POST elements`（css selector）加 Element Click；`send_keys` 是 Element Send Keys；`action_sequence` 是 `POST actions`；另外有 window rect、最小化、cookie、`get_computed_label` 等。帶 `context` 的動作經過 `switch_to_window`（`protocol.py`）：走遍每個 handle 和 frame（`POST frame {id: 元素}`、`POST frame/parent`），在每個 frame 裡執行回傳 `[window.__wptrunner_id, iframe 元素的陣列]` 的 script，所以元素的陣列和以元素切換 frame 都要有。沒做的指令回答 `unknown command`，測試看到的是「not implemented」。

### 最少要有的指令

| 指令 | testharness | reftest、crashtest | wdspec 的目錄（`webdriver/tests/classic/`） | 這份的哪一段 |
| --- | --- | --- | --- | --- |
| New Session、Delete Session、Status | ✓ | ✓ | `new_session`、`delete_session`、`status` | 1 |
| Get / Set Timeouts | ✓ | ✓ | `get_timeouts`、`set_timeouts` | 1 |
| Navigate To、Get Current URL | ✓ | ✓ | `navigate_to`、`get_current_url` | 1 |
| Back、Forward、Refresh、Get Title | | | `back`、`forward`、`refresh`、`get_title` | 1 |
| Get Window Handle(s)、Switch To Window、Close Window、New Window | ✓ | | `get_window_handle(s)`、`switch_to_window`、`close_window`、`new_window` | 3（Get Window Handle 在 1） |
| Get / Set Window Rect | ✓ | ✓ | `get_window_rect`、`set_window_rect` | 3 |
| Execute Script、Execute Async Script | ✓ | ✓ | `execute_script`、`execute_async_script` | 2 |
| Find Element(s)（`css selector`） | testdriver | | `find_element(s)`、`find_element(s)_from_element` | 2 |
| Get Element Rect、Tag Name、Attribute、Property | | | `get_element_rect` 等 | 2 |
| Switch To Frame、Switch To Parent Frame | testdriver | | `switch_to_frame`、`switch_to_parent_frame` | 3 |
| Element Click | ✓ | | `element_click` | 4 |
| Take Screenshot、Take Element Screenshot | | ✓ | `take_screenshot`、`take_element_screenshot` | 4 |
| Release Actions | ✓ | | `release_actions` | 4（沒有 Actions，不做事） |

wdspec 的目錄裡還有測 user prompt（`user_prompts.py`）、shadow root、跨來源 frame 的檔案：kun 沒有對應的功能，預期失敗。

## wptrunner 在頁面裡執行的 script

| script | 用到、kun 還沒有的 | 沒有時會怎樣 |
| --- | --- | --- |
| `executors/window-loaded.js` | `document.readyState` | `undefined != "complete"`，改等 `load`；`about:blank` 的 `load` 已經發生過，一直等到逾時 |
| `executors/test-wait.js` | `document.readyState`（`document.fonts` 有檢查，沒有也行） | 同上：每個 reftest 和 crashtest 都等到逾時 |
| reftest 的 `[outerWidth - innerWidth, …]` | `window.outerWidth`、`outerHeight` | `NaN`，JSON 變成 `null`，Python 算 `800 + None` 出錯 |
| `executors/message-queue.js`、`testharness_webdriver_resume.js` | 無 | |
| testdriver 的回覆 | `window.postMessage` | Actions 那一份 RFC 的事 |

`requestAnimationFrame`、`MutationObserver`、`Event` 的建構子 kun 都有。這些 script 都沒有讀 `navigator.webdriver`。

## 規範的重點

規範：<https://w3c.github.io/webdriver/>。

- **回答的格式**：成功是 `{"value": …}`；錯誤是 `{"value": {"error", "message", "stacktrace"[, "data"]}}`。HTTP 404：`invalid session id`、`no such element`、`no such frame`、`no such window`、`stale element reference`、`unknown command` 等；400：`invalid argument`、`invalid selector`；500：`javascript error`、`script timeout`、`timeout`、`unexpected alert open`、`unable to capture screen`、`unknown error`、`unsupported operation`。
- **逾時的預設**：script 30 000 ms、pageLoad 300 000 ms、implicit 0。
- **page load strategy**：`none`、`eager`（等 document 的 readiness 到 `interactive`）、`normal`（到 `complete`）。Navigate To 對 http(s) 的網址等到 readiness 到了才回答，超過 `pageLoad` 回答 `timeout`。
- **window handle 和 frame**：handle 代表一個 top-level browsing context。New Window 開 `about:blank`（noopener）、不取得焦點，回答 `{handle, type}`。Close Window 回答剩下的 handle，一個都不剩時結束 session。Switch To Frame：`id` 是 `null` 回到 top-level；是 0 到 2^16−1 的數字時是 `window[n]`，沒有就 `no such frame`；是 `frame` 或 `iframe` 的元素參照時切到它的 content，不是就 `no such frame`；其他的型別是 `invalid argument`。
- **Execute Script**：`{script, args}`；`args` 先反序列化（元素和 window 的參照換回物件）。「execute a function body」：在目前 browsing context 的 realm 裡把 `script` 當成 FunctionBody 解析（有 `"use strict"` 才是 strict mode、不受 CSP 影響、解析失敗時是 `null`）；結果包成 promise，reject 是 `javascript error`，超過 script 的逾時是 `script timeout`。非同步的版本在 `arguments` 最後多一個 resolve。執行期間跳出 user prompt 時立刻回答 `null`。
- **JSON clone**：`undefined`、`null` 是 `null`，基本型別照原樣；元素是 `{"element-6066-11e4-a52e-4f735466cecf": id}`，shadow root 是 `{"shadow-6066-11e4-a52e-4f735466cecf": id}`，top-level 的 `WindowProxy` 是 `{"window-fcc6-11e5-b4f8-330a88ab9d7f": handle}`，frame 的是 `{"frame-075b-4da1-b6ba-e579c2d3230a": id}`；`Array`、`NodeList`、`HTMLCollection` 等集合變成陣列；有可以呼叫的 `toJSON` 時用它；其他物件取可列舉的自有屬性；循環是 `javascript error`，stale 的元素是 `stale element reference`。
- **元素參照**：session 有一張「browsing context group node map」（節點到 UUID，弱參照），加上「navigable seen nodes map」（哪個 navigable 看過哪個 id）。目前的 navigable 沒看過的參照是 `no such element`；節點不在 document 裡、或 document 不是 active 的，是 `stale element reference`。
- **Find Element**：方式有 `css selector`、`link text`、`partial link text`、`tag name`、`xpath`；照 document 的順序，implicit wait 期間重試。
- **Get Element Rect**：`{x, y, width, height}`，bounding client rect 加上捲動的位移，CSS px。
- **Element Click**：捲進畫面，取在畫面裡的中心點；被別的元素擋住是 `element click intercepted`，不在畫面裡是 `element not interactable`；經過 Actions 的機制送 pointer 的 move、down、up，之後等可能開始的導覽。
- **Get / Set Window Rect**：`{x, y, width, height}`，每項可以省略或是 `null`；離開全螢幕或最小化，回答實際的大小。Get 回答 `screenX`、`screenY`、`outerWidth`、`outerHeight`。
- **Take Screenshot**：在下一個 animation frame，截 top-level 的 document element 的範圍、裁到 viewport，PNG、base64；範圍是 0 或拿不到 bitmap 時是 `unable to capture screen`。
- **user prompt**：預設的處理是「dismiss and notify」，回答 `unexpected alert open`。

## 各家的做法

| | server 在哪裡 | 和瀏覽器的溝通 | 執行 script | 元素參照 | 輸入 | 截圖 |
| --- | --- | --- | --- | --- | --- | --- |
| Chromium | 獨立的 chromedriver（C++） | CDP，經過 `--remote-debugging-port` 或 pipe | `Runtime.callFunctionOn` / `Runtime.evaluate`，包在注入的 `js/call_function.js` 裡（參數和結果的轉換） | 注入的 JS 裡的快取（現在的格式沒有確認） | `Input.dispatchMouseEvent` / `dispatchKeyEvent`，在 browser process 的 `RenderWidgetHost` 注入 | `Page.captureScreenshot` |
| Firefox | geckodriver（Rust、`webdriver` crate）加上瀏覽器裡的 Marionette | TCP 上的 JSON：`[0, id, "WebDriver:ExecuteScript", params]` | content process 的 sandbox（`evaluate.sys.mjs`），經過 JSWindowActor | 每個 process 的 `NodeCache`（弱參照加 UUID） | `Actions.sys.mjs`，在 child 裡用 windowUtils 合成 | parent 的 `drawSnapshot` |
| Ladybird | 獨立的 `WebDriver` 程式（`Services/WebDriver/`），New Session 時啟動 `Ladybird --headless --webdriver-browser-path=<socket>` | 自己的 IPC：driver 的 `WebContentConnection` 和 WebContent 的 `WebDriverConnection` | WebContent 裡，`LibWeb/WebDriver/` 照規範的 C++ | 存在節點上，id 是「navigable id、process 的 UUID、節點的 id」，navigable 的前綴代替 seen nodes map | WebContent 裡直接分派 | `render_screenshot` 畫進 `<canvas>`，`toDataURL` |
| Servo | 瀏覽器裡（`components/webdriver_server`，`webdriver` crate），`--webdriver=N` | crossbeam channel 交給 embedder；要 DOM 的轉給 script thread | `components/script/webdriver_handlers.rs`：把 script 包成 `(async function(){…})(args)` 的字串 | 節點 id 的字串，在 script thread 解讀 | `WebDriverCommandMsg::InputEvent` 交給 embedder，和真的輸入同一條路 | 先用 async script 等 rAF，embedder 截 RGBA、編成 PNG |
| WebKit | 獨立的 `WebKitWebDriver`（Safari 是 `safaridriver`），啟動瀏覽器時加 `--automation` | remote inspector 的 socket，`Automation` domain | WebProcess 的 `WebAutomationSessionProxy.js`（注入的 JS） | 注入的 JS 裡的對照表 | UIProcess 的 `SimulatedInputDispatcher` 模擬平台的事件 | UIProcess 和 WebProcess 的 snapshot |

共同點：執行 script、JSON 的轉換和元素參照都在 DOM 那一側；分頁和視窗、截圖、輸入（Ladybird 除外）在 browser 那一側。

## `webdriver` crate

- Mozilla 的，原始碼在 mozilla-central 的 `testing/webdriver`，和 geckodriver 一起維護；0.54.0 是 2026-06-02 發的（之前 0.53.0 是 2025-04、0.52.0 是 2025-02）。MPL-2.0。用的人：geckodriver、Servo。
- 提供：`server::start(addr, allow_hosts, allow_origins, handler, extension_routes)`；`trait WebDriverHandler<U>`（`handle_command(&mut self, &Option<Session>, WebDriverMessage<U>) -> WebDriverResult<WebDriverResponse>` 和 `teardown_session`）；全部指令的解析（`command::WebDriverCommand`、`httpapi` 的路由、擴充的路由）；`common` 的型別（`WebElement`、`ShadowRoot`、`WebFrame`、`WebWindow`、`LocatorStrategy`、識別字串的常數）、`actions`、`capabilities`（`SpecNewSessionParameters`、`BrowserCapabilities` 的比對）；`error::{ErrorStatus, WebDriverError}`（產生規範的 HTTP status 和 JSON）。
- **thread**：「webdriver server」thread 在單執行緒的 tokio runtime 上跑 warp；「webdriver dispatcher」thread 經過 `std::sync::mpsc` 依序呼叫 handler。指令一次處理一個，handler 可以 block。
- **請求的檢查**：`Host` header：IP 一律允許，網域要在清單裡，port 要相符；`Origin` 要在 `allow_origins` 裡；form 和 `text/plain` 的 POST 一律拒絕（擋住網頁從瀏覽器裡發的請求）。
- **依賴**：base64、bytes、cookie、http 1、`icu_segmenter` 2、log、serde 和 serde_json、thiserror 1、time、url，加上預設的 `server` feature 帶進的 tokio 1 和 warp 0.4。kun 的 workspace 已經有 tokio 1、http 1、hyper 1（`reqwest` 帶進來的）、serde_json、`icu_segmenter`（parley 帶進來的）；實際多了哪些見「第 0 步的實測」。

## WPT 的 WebDriver 測試

- 在 `webdriver/tests/classic/<指令>/`，一個指令一個目錄，pytest 寫的。
- 經過 wptrunner 的 `PytestExecutor`（`executors/base.py`、`executors/pytestrunner/runner.py`）跑，`--test-type=wdspec`：wptrunner 照常啟動 driver，把 host、port 和 capabilities 寫進 `WDSPEC_CONFIG_FILE` 指的 JSON，`webdriver/tests/support/fixtures.py` 的 fixture 經過 `tools/webdriver` 開 session。測試的頁面由 wptserve 提供。
- 對應這份範圍的目錄見「最少要有的指令」的表。

## 第 0 步的實測

2026-10-08，Windows 11（26200，x86_64），Rust 1.99.0（`rust-toolchain.toml`），Python 3.11.9，`../third_party/wpt` 在 `0f195499`。原型在 `step0-prototype/`，不是 workspace 的一部分：

- `src/main.rs`：用 `webdriver` crate 0.54.0 的 server，只回答 Status、New Session、Delete Session、Set Timeouts，其他都是 `unknown command`；session 說 `setWindowRect: false`。
- `wptrunner_machopo/`：kun 的 wptrunner product（Python 套件，`wptrunner.products` 的 entry point），照 Ladybird 的 product：wptrunner 用 `--port` 啟動 driver。
- `run_wptrunner.py`：不經過 `wpt run`、直接呼叫 wptrunner（下面的「wptrunner」）。

### `webdriver` crate

- **編譯**：用 kun 的 toolchain 直接能編，沒有警告。原型單獨從頭 release build 約 17 秒，執行檔 2.6 MB（Windows）。
- **新的依賴**（和 workspace 的 `Cargo.lock` 比，同名不同版的不算）：
  - 預設的 `server` feature：19 個，`warp`、`headers`、`headers-core`、`httpdate`、`mime`、`mime_guess`、`unicase`、`serde_urlencoded`、`sha1`、`signal-hook-registry`、`tokio-macros`、`ryu`、`cookie`、`time`、`time-core`、`time-macros`、`deranged`、`num-conv`、`powerfmt`，加上 `webdriver` 自己。`tokio`、`hyper`、`http`、`icu_segmenter`（parley 帶進來的）workspace 已經有。
  - `default-features = false`（沒有 server，只有指令的解析、型別和錯誤）：7 個，`cookie`、`time`、`time-core`、`time-macros`、`deranged`、`num-conv`、`powerfmt`。
- **錯誤碼是封閉的**：`ErrorStatus` 是固定的 enum，`error_code()` 回傳規範的字串（`"unknown error"` 等），沒有自訂的。用它的 server 時，driver 回答不了 chromedriver 的 `tab crashed` 和 `disconnected`（第 6、7 條），因此不用它的 server（`README.md` 的「已決定的事項」第 8 條）。
- **server 的行為**：dispatcher 自己檢查 session id（沒有 session 時只接受 New Session 和 Status，已經有 session 時拒絕第二個 New Session），handler 回答 Delete Session 或關掉最後一個視窗時呼叫 `teardown_session`。路由（`standard_routes()` 的 method 和路徑樣式）和 warp 的 filter 都在 `server.rs`；沒有 `server` feature 時，`httpapi::standard_routes()` 和 `WebDriverMessage::from_http` 還在，路徑的比對要自己做。

### wptrunner

- **product 從 checkout 外面載入**：照上游的文件（`tools/wptrunner/docs/plugins.rst`），在 venv 裡 `pip install` 一個有 `wptrunner.products` entry point 的套件，`get_product()` 回傳 `wptrunner.products.Product`。checkout 不用改。`pip install` 會在套件的目錄留下 `build/` 和 `*.egg-info`，裝完要刪（或之後用 `--no-build-isolation` 等方式避開，第 1 段決定）。
- **`wpt run` 不能用**：`tools/wpt/run.py` 的 `check_environ` 對內建的 product 以外的都檢查系統的 hosts 檔（Windows 是 `System32\drivers\etc\hosts`），沒有 WPT 的網域就停下來，要人用管理者權限改 hosts。kun 的網域只在 kun 裡對應（RFC 0020），所以改成直接呼叫 wptrunner（`wptcommandline.create_parser()`、`check_args`、`GlobalLogger`、`wptrunner.start`），Chromium 的 blinkpy 也是直接呼叫 wptrunner。
- **wptserve 要 bind 在 `127.0.0.1`**：wptrunner 自己啟動的 wptserve 預設 bind 在 `web-platform.test`，這台機器解析不到（`getaddrinfo failed`），所有 server 都起不來。product 的 `env_options` 設 `server_host: "127.0.0.1"` 之後正常，和 RFC 0020 的 `server_host` 同一個原因。
- **venv 要的套件**：`tools/wptrunner/requirements.txt`、`tools/requirements_pytest.txt`（wdspec 的 pytest）、`tools/wpt/requirements.txt`（manifest 要 PyYAML 解析 test262 的測試），加上 kun 的 product。
- **manifest 放在 checkout 外面**：`--manifest <target 底下>`。第一次產生整個 checkout 的 manifest，加上跑一個目錄約 80 秒；之後每次 run 檢查 manifest 有沒有變，跑一個目錄約 20 秒。
- **checkout 不變**：要設 `PYTHONDONTWRITEBYTECODE=1`，pytest 改寫 assertion 時寫的 `.pyc` 也照它；不能用 `python -I`（它忽略所有 `PYTHON*` 的環境變數，第一次試的時候 bytecode 寫進了 checkout 的 57 個 `__pycache__/`，都已經刪掉）。設好之後每次 run 完 `git -C ../third_party/wpt status --porcelain --ignored` 都是空的。
- **Windows 的 multiprocessing**：manifest 用 `multiprocessing`，Windows 上的子 process 重新 import 主程式，`run_wptrunner.py` 沒有 `if __name__ == "__main__"` 時每個子 process 都重跑一次，卡住超過 10 分鐘。
- **wdspec 的結果**：`webdriver/tests/classic/status`、`delete_session` 的 5 個 subtest，4 個 PASS；`delete.py` 的 `test_accepted_beforeunload_prompt` 要 Navigate To，原型沒有。`session` fixture（`webdriver/tests/support/classic/fixtures.py`）開 session 後一定呼叫 Set Timeouts，`setWindowRect` 是 `true` 時再 Set Window Rect（800×600）；結束時的 `cleanup_session` 吞掉所有錯誤。所以第 1 段回答 `setWindowRect: false`，第 3 段做了 Set Window Rect 才改成 `true`，不必把它提前。

## 已決定的事項的選項

`README.md` 的「已決定的事項」第 1 到 13 條確認之前的比較（2026-10-08）。第 1 到 6 條照當時的看法確認；S 第一次的看法是 S1（做在 shell 裡），討論時傾向獨立的程式，重新列出可行的做法之後確認 S2；E 是第 0 步之後加的，照看法確認 E2。

### E：用不用 `webdriver` crate 的 server

第 0 步找到：`webdriver` crate 的錯誤碼是封閉的 enum（`ErrorStatus`），只有規範的那些；用它的 server 時，driver 回答不了第 6、7 條的 `tab crashed`、`disconnected`。wptrunner 只在 HTTP 500 加上這兩個錯誤碼時把測試記成 CRASH，其他錯誤再問一次 `GET window/handles`：成功就記成 INTERNAL-ERROR，失敗才是 CRASH（`executors/executorwebdriver.py`）。

| | E1：用它的 server，改回規範的錯誤 | E2：不用它的 server，driver 自己做 HTTP 那一層 | E3：改 crate |
| --- | --- | --- | --- |
| 做法 | 分頁當掉回答 `unknown error`；kun 整個當掉時 driver 結束 session，之後的指令（包括 `window/handles`）都是 `invalid session id` | `default-features = false`，只用它的指令解析（`standard_routes()`、`WebDriverMessage::from_http`）、型別和 capabilities；HTTP、路徑的比對、session id 的檢查、`Host` 和 `Origin` 的檢查自己做，錯誤的 JSON 自己產生 | fork，加上兩個錯誤碼 |
| wptrunner 看到的 | kun 當掉：CRASH（`window/handles` 失敗）。分頁當掉：INTERNAL-ERROR，和 WebDriver 自己的 bug 分不開 | 和 chromedriver 一樣，兩者都是 CRASH | 同 E2 |
| 代價 | 第 6 條改掉；分頁當掉時的結果不準 | 自己寫 HTTP 那一層（crate 的 `server.rs` 約 300 行做的事）；HTTP/1.1 用 workspace 已經有的 `hyper`（加上 server 的 feature）或只用標準函式庫 | 多維護一個 fork；規範沒有這兩個錯誤碼，上游不會收 |
| 依賴 | 多 19 個 crate | 多 7 個 crate（沒有 warp 等） | 同 E1 |
| 先例 | geckodriver、Servo（Firefox 和 Servo 都沒有「分頁當掉、瀏覽器還在」需要回報） | chromedriver（自己的 HTTP server） | |

**2026-10-08 的看法：E2**。crate 的價值在指令的解析、型別和 capabilities 的比對，這些都留著；HTTP 那一層小，而且自己做還少 12 個依賴。kun 和 Chromium 一樣是「分頁當掉、瀏覽器還在」，回報要和 chromedriver 一致，wptrunner 才分得出來。

### S：server 在哪裡

2026-10-08 的討論：放在 shell 裡也可以，但傾向獨立的程式。可行的做法：

| | S1：做在 shell 裡 | S2：獨立的 driver + kun 自己的自動化協定 | S3：獨立的 driver + CDP 的子集 | S4：先做在 shell 裡、照 process 的邊界寫 |
| --- | --- | --- | --- | --- |
| 先例 | Servo | Ladybird（`WebDriver` + IPC）、WebKit（`WebKitWebDriver` + Automation domain）、Firefox（geckodriver + Marionette） | Chromium（chromedriver + CDP） | kun 的 renderer（現在是 thread，照 process 的邊界寫） |
| driver 和 kun 之間 | 不用：server 的 thread 把請求交給 UI thread | kun 專用的訊息（`protocol` 裡，可以序列化），經過 pipe；粒度和 WebDriver 的指令差不多 | CDP（`Target`、`Page`、`Runtime`、`Input`、`Browser` 等 domain） | 和 S2 同一套訊息，先經過 channel |
| 執行 script | renderer（第 2 條） | renderer（第 2 條），browser 轉送 | CDP 的 `Runtime` 回答 RemoteObject，JSON 的轉換和元素參照落到 driver 注入的 JS：和第 2 條衝突，除非另外加 kun 專用的 domain | 同 S2 |
| 啟動 | wptrunner 直接啟動 `kun --webdriver <port>`（Servo 的 product） | wptrunner 啟動 driver，driver 在 New Session 時啟動 kun、Delete Session 時結束它（Ladybird 的 product） | 同 S2 | 同 S1，換成 S2 之後同 S2 |
| 瀏覽器當掉時 | server 一起消失，wptrunner 只看到 process 結束 | driver 還在：分頁當掉回答 `tab crashed`（第 6 條），整個 kun 當掉時 pipe 斷了，回答 `disconnected`；兩者 wptrunner 都記成 CRASH | 同 S2 | 同 S1 |
| 多出來的工作 | 無 | 協定的訊息、pipe 的傳輸和框架、啟動和結束 kun、第二個 binary | 一大套 CDP（只為了 WebDriver 用到的部分也不小），而且 CDP 的形狀是 Chromium 的 | 同 S2 的協定，傳輸和第二個 binary 之後再做 |
| 之後 | 要對外的協定（DevTools、Puppeteer）時另外做 | 協定可以再給情境的執行器用（RFC 0006 的「之後的可能」） | Puppeteer、DevTools 直接能接 | 換傳輸就變成 S2 |

S4 是 S1 和 S2 之間：程式的切法和 S2 一樣（driver 在自己的 crate 裡，只用可以序列化的訊息和 browser 溝通），只是先放在同一個 process；之後換成 pipe、拆出 binary 就變成 S2。它省下的只有傳輸和啟動，S2 多做的這部分不大。

S2 再細分的選擇：

- **傳輸**：
  - stdin 和 stdout 的 pipe：driver 啟動 kun 時接上，只有 driver 能用，driver 關掉 stdin 時 kun 結束，kun 當掉時 driver 讀到 EOF。kun 的 log（`tracing_subscriber::fmt()`）現在寫到 stdout，要改到 stderr。
  - 額外的 fd 或 handle（Chromium 的 `--remote-debugging-pipe` 用 fd 3 和 4）：要在 `fork` 和 `exec` 之間設定，Rust 的標準函式庫做不到，要 `unsafe` 或平台的 API，而 `unsafe` 只能在 `dom`、`bindings`、`ipc`。
  - 本機的 TCP：kun 聽 driver 指定的 port；多開一個本機的 port，要另外驗證連進來的是 driver。
  - RFC 0004 的 `ipc` crate（Unix socket、Windows 的 named pipe）：那份延後了，還沒有。
- **格式**：一行一個 JSON（`serde_json`，好除錯，截圖的 base64 也只有幾百 KB），或二進位的 serde。
- **狀態放哪裡**：driver 放 session、capabilities、逾時、implicit wait 的重試、page load 的等待、window handle 的對照（像 chromedriver）；kun 只回答單一的動作（開分頁、導覽並回報載入完成、改視窗大小、截圖、送輸入、轉送 renderer 的指令）。
- **名字和位置**：新的 crate（不能叫 `webdriver`，和 Mozilla 的 crate 同名），只依賴 `protocol` 和 `webdriver` crate，不依賴 shell 的 winit、wgpu；binary 的名字照 chromedriver 的習慣。

**2026-10-08 的看法：S2，傳輸用 stdin 和 stdout 的 pipe、格式用一行一個 JSON**。和 Chromium、Firefox、Ladybird、WebKit 一致：driver 在瀏覽器當掉時還在，能回答正確的錯誤；每個 session 一個新的 kun，session 之間不留狀態（原本 S1 要另外決定「下一個 session 從什麼狀態開始」）。S3 的 CDP 太大，而且和第 2 條衝突；S4 省下的不多。pipe 不用 `unsafe`、不開 port，kun 的生命週期跟著 driver。

### H：HTTP 和指令的解析

| | H1：Mozilla 的 `webdriver` crate | H2：自己寫 |
| --- | --- | --- |
| 有什麼 | HTTP server（warp）、全部指令的路由和參數解析、capabilities 的比對、錯誤的 JSON、規範的常數和型別 | `serde_json` 和一個小的 HTTP/1.1 server（像 `net::test_server`，只用標準函式庫） |
| 用的人 | geckodriver、Servo；和 Firefox 一起維護（0.54.0，2026-06-02） | |
| 代價 | 新的依賴（warp 0.4、`icu_segmenter`、`cookie`、`time` 等）、MPL-2.0 | 每個指令的解析、錯誤和 capabilities 都要自己寫、自己測 |

**看法：H1**，第 0 步確認依賴和編譯沒有問題。解析和錯誤的格式不是 kun 要做出差別的地方，而且 wdspec 會逐條檢查。

### R：renderer 這一側怎麼執行 script、放元素參照

| | R1：Rust 照規範的演算法做 | R2：注入 JS |
| --- | --- | --- |
| 先例 | Ladybird（C++）、Servo、Firefox（特權的 JS，不在網頁的 realm 裡） | Chromium（`call_function.js`）、WebKit（`WebAutomationSessionProxy.js`） |
| renderer 要多什麼 | 執行函式本體、JSON 的轉換（`dom_js`，走訪 V8 的值）、參照表（`dom` 或 renderer） | 「在這個 realm 裡用 JSON 參數呼叫一段 JS、拿回 JSON」，其他都在注入的 JS 裡 |
| 元素參照放在哪 | renderer 的表，網頁看不到 | 網頁的 realm 裡（kun 沒有 isolated world），網頁看得到、改得到：Chromium 早期的 `$cdc_` 屬性就被網頁拿來偵測 chromedriver |
| 和規範的距離 | 演算法逐步對照規範，錯誤的種類精確 | 注入的 JS 也要照規範寫，但被網頁改掉的 `Array.prototype`、`JSON` 等會影響它 |

**看法：R1**。kun 沒有 isolated world，R2 的 JS 和網頁共用全域，網頁的程式會影響結果（wdspec 的 `execute_script/properties.py` 就測這種情況）；Rust 的版本也容易寫單元測試。代價是 V8 的值的走訪要寫在 `dom_js`。

### W：瀏覽器的模式

| | W1：沒有平台視窗的 `BrowserWindow` | W2：headless（只有 `WebContents`） | W3：只用真的視窗 |
| --- | --- | --- | --- |
| 現在用在 | 情境（`windowless.rs`） | `--headless`、reftest、Rust 的 WPT runner（`headless.rs`） | 平常的瀏覽器、`--automation` |
| 多個分頁 | 有（`BrowserView`） | 沒有 | 有 |
| 需要顯示器和 GPU | 不用 | 不用 | 要 |
| 對應 Chromium | `headless_shell` 和 `--headless=new`（同一個瀏覽器、不顯示） | content_shell 的 web test 模式 | 一般的 Chrome |

**看法：W1 為預設，`--window` 換成 W3**。W2 沒有分頁，New Window 做不出來；W1 和 W3 是同一個 `BrowserWindow`，WebDriver 的程式不用分兩套（情境也是這樣，RFC 0006）。CI 上只跑 W1。

### P：`document.readyState`、`outerWidth`、`outerHeight` 在哪一份做

- P1：在這份（第 1 段做 `readyState`，第 3 段做 `outerWidth`、`outerHeight`）。
- P2：加進 RFC 0017（HTML 元素和 Window 的雜項，已接受、還沒開始）的範圍。
- P3：另外開一份小 RFC，先做完再做這份。

**看法：P1**。三個都小，而且 WebDriver 的導覽本來就要 document 的 readiness（`pageLoadStrategy`）；RFC 0017 已經接受，改它的範圍要重新確認。

### T：wdspec 怎麼跑

wdspec 要經過 wptrunner（它啟動 driver、寫好 `WDSPEC_CONFIG_FILE`，pytest 的 fixture 讀它），所以要先有 kun 的 wptrunner product，也要決定 wdspec 的預期結果記在哪裡。

- T1：kun 的 product 在這份做（放在 kun 的 repo 裡、裝進 wptserve 的 venv，不改 WPT 的 checkout），只用來跑 wdspec；wdspec 的預期結果用 wptrunner 原生的 `.ini`（wdspec 在 Chromium 也是另外一套）。下一份 RFC 再決定 testharness 和 reftest 的格式。
- T2：這份只有 Rust 的整合測試，wdspec 留到下一份 RFC 和 product 一起做。

**看法：T1**：WebDriver 的行為細節很多，只靠自己寫的測試很難說和規範一致；wdspec 是現成的、逐條對照規範的測試。第 0 步確認 product 能從 checkout 外面載入之後再定。

### C：分頁的 renderer 當掉時

kun 的分頁當掉時 browser 還在（sad tab）。規範沒有對應的錯誤；chromedriver 回答 `tab crashed`（不在規範裡），wptrunner 看到它（或 `disconnected`）時把測試記成 CRASH，其他錯誤記成 INTERNAL-ERROR。

- C1：照 chromedriver 回答 `tab crashed`（HTTP 500）。
- C2：回答規範的 `unknown error`。

**看法：C1**：wptrunner 能分辨 renderer 當掉和 WebDriver 的 bug，下一份 RFC 換跑法時有用。第 2 段開始前確認 `webdriver` crate 能不能回答自訂的錯誤碼。

### 第 1 段的選項（第 9 到 13 條）

2026-10-08，第 1 段開始前比較，N3 以外照當時的看法確認；M-a 追加了「盡量和 CDP 一致」，A-c 追加了「分成第 1 段和第 1b 段」。第 1b 段的等待後來改了一處：`wait-loaded` 是 `Kun.waitForLoad`，不在執行器裡用事件等（README 的「已決定的事項」第 16 條）。

| | 選項 | 說明 |
| --- | --- | --- |
| 名字 | N1 | crate 和 binary 都是 `machopodriver` |
| | N2 | crate `webdriver_server`（Servo 的叫法），binary `kun-webdriver` |
| | **N3** | crate `driver`，binary `machopodriver` |
| HTTP/1.1 | **H-a** | `hyper` 的 http1 server：workspace 已經有，不多 crate，keep-alive、chunked、pipelining 交給它 |
| | H-b | 只用標準函式庫（像 `net::test_server`），解析自己寫、自己測 |
| 訊息 | **M-a** | id 加上回答和事件；之後追加：格式和 CDP 有的方法照 CDP，其他放 `Kun` domain，`\0` 分隔 |
| | M-b | 只有一問一答：等載入的邏輯搬到 kun，分頁當掉等事件通知不了 |
| 和情境 | A-a | 擴充現在的 `Request`，pipe 是另一個入口 |
| | A-b | pipe 另外一個 host，只共用輔助函式（Chromium 的 interactive_ui_tests 和 CDP 就是兩條路） |
| | **A-c** | 情境也改用自動化訊息，只留一個 host；分成第 1 段和第 1b 段 |
| `eager` | **Q-a** | renderer 通知 `DOMContentLoaded`（Chromium 的 `DidDispatchDOMContentLoadedEvent`） |
| | Q-b | 當作 `normal`，回答得比規範晚 |

**和 CDP 一致到什麼程度**：CDP 有的用 CDP 的（`Target` 的分頁和 session、`Page` 的導覽、`lifecycleEvent` 和截圖、`Browser` 的視窗大小、`Input`、`Target.targetCrashed`）；做不到一致的有兩處。一是 WebDriver 在 renderer 裡的部分沒有對應的 CDP 方法：CDP 的 `Runtime.callFunctionOn` 回答 RemoteObject，套上去就是第 7 條不選的 S3，所以放 `Kun` domain。二是傳輸：Chromium 的 pipe 是 fd 3 和 4、`\0` 分隔，kun 用 stdin 和 stdout（設 fd 3 和 4 要 `unsafe`），分隔照 Chromium。

**A-c 的缺點**（確認前列出的）：情境大多是視窗層級的操作（點分頁列、瀏覽器的快捷鍵、截整個視窗、`wait-busy`、iframe 的 selector 路徑），CDP 沒有，要放 `Kun` domain；改寫的是視窗那一層唯一的回歸測試，要跑全部情境和真的視窗；執行器要改成 id 加事件的寫法；和 Chromium 不同；放進第 1 段範圍太大，所以拆出第 1b 段。

### 第 2 段的選項（第 19 到 23 條）

2026-10-08，第 2 段開始前比較，都照當時的看法確認。開始前先查了 `dom` 和 engine 現有的東西：`dom` 已經有 `NodeId` 到節點的弱參照表（`node_id.rs`，id 單調遞增、不重用，回收了查不到），renderer 的 `pressed`、`scroll_target` 就這樣跨 task 記節點；V8 那一側有把字串當成函式本體編譯（event handler 的屬性）、Rust 等 promise settle（`import()`）的前例；wasm 的 engine 碰到 JS 的方法是 panic，要改成回答不支援。

| | 選項 | 說明 |
| --- | --- | --- |
| 元素參照 | **E-a** | id 帶著位置（renderer 的前綴、document、`NodeId`），不存表；Chromium 的 chromedriver、Ladybird |
| | E-b | 照規範字面：UUID，renderer 每個分頁一張表，另外記哪個 navigable 看過；Firefox 的 `NodeCache` |
| | E-c | `dom` 新增公開的弱參照型別（包 `WeakPersistent`） |
| script 的結果 | **J-a** | `dom_api` 裡和 engine 無關的值，元素是 `NodeId`；參照的規則在 renderer |
| | J-b | engine 直接產生最後的 JSON，參照也在 engine 裡編 |
| 方法的對象 | **K-a** | 經過 session，作用在 attach 的分頁（和 `Page.*` 一樣） |
| | K-b | 作用在目前的分頁（和第 14 條情境的方法一樣） |
| 元素的框 | **G-a** | `getBoundingClientRect()` 加捲動的位移，`dom_api` 的 geometry |
| | G-b | 上一次 pre-paint 的框（情境的 `queryElement`） |
| 找元素 | L-a | 只有 `css selector` |
| | **L-b** | 加上 `tag name` |
| | L-c | 再加 `link text`、`partial link text`（要 `innerText`） |

### 第 3 段的選項（第 24 到 26 條）

2026-10-09，第 3 段開始前比較，都照當時的看法確認。開始前先查了現況：renderer 用 frame 目前 document 的 `DocumentId` 認 frame，子 frame 導覽時 `commit_child` 換一個新的 `FrameView` 放回原位；browser 沒有 frame tree（RFC 0009 的第 18 條）。wdspec 的 `test_find_element_while_frame_is_still_loading` 要 frame 的 id 跨過子 frame 的導覽；`test_no_such_element_from_other_frame` 要別的 frame 的元素參照（iframe 已經移除的也是）回答 `no such element`，同一個 frame 舊文件的才是 `stale element reference`。背景分頁收到 `WasHidden`、沒有 `BeginFrame`。renderer 只從 `VisualProperties` 知道網頁 view 的大小。

| | 選項 | 說明 |
| --- | --- | --- |
| frame 的 id | F-a | 父 document 的 `DocumentId` 加 iframe 的 `NodeId`：不加狀態，和元素參照（第 19 條）同一種做法；iframe 移除又插入時指到新的 frame |
| | **F-b** | renderer 給每個 frame 一個號碼，導覽時沿用；Chromium 的 frame token、Firefox 的 `BrowsingContext` id |
| | F-c | 子 frame 目前 document 的 `DocumentId`：子 frame 一導覽就失效 |
| 切換分頁 | **T-a** | 切到前面（`Target.activateTarget`）；chromedriver 的 `ActivateWebView`、Firefox |
| | T-b | 只記在 session：背景分頁的 rAF、截圖會卡住 |
| `outerWidth` | O-a | 加進 `VisualProperties` |
| | **O-b** | 新的 `RendererMsg::UpdateScreenRects`；Chromium 的 `blink.mojom.Widget::UpdateScreenRects` |

### 第 4 段的選項（第 27、28 條）

2026-10-09，第 4 段開始前比較，都照當時的看法確認。截圖（`Page.captureScreenshot`：從 Viz 讀回整個視窗、裁成網頁 view，PNG）、點擊前的檢查在 renderer 一次算好、點了之後照 page load strategy 等導覽、沒有 Actions 時 Release Actions 什麼都不做，RFC 原本就寫了。開始前查了：`WebContents::forward_input_event` 已經能把滑鼠事件直接交給分頁的 renderer（`BrowserView` 也是經過它）；kun 的 wptrunner product 已經登記了 testharness、reftest 的 executor，`run.py` 收 `--test-types`。

| | 選項 | 說明 |
| --- | --- | --- |
| 點擊的路線 | **I-a** | CDP 的 `Input.dispatchMouseEvent` 直接交給分頁的 `WebContents`；Chromium 的 `InputHandler` |
| | I-b | 經過視窗（`BrowserView`），像情境的 `Kun.dispatchWindowInput` |
| 冒煙測試 | K-a | 第 4 段結束時手動跑一次，記在 `progress.md` |
| | **K-b** | `test.py` 的固定指令，兩個測試都要 PASS |

## 參考資料

- WebDriver 規範：<https://w3c.github.io/webdriver/>
- Chromium 跑 WPT 的文件（`run_wpt_tests.py` 和 `run_web_tests.py` 的差別）：<https://chromium.googlesource.com/chromium/src/+/main/docs/testing/run_web_platform_tests.md>
- Blink 的 `testdriver-vendor.js`（content_shell 的 testdriver）：<https://chromium.googlesource.com/chromium/src/+/main/third_party/blink/web_tests/resources/testdriver-vendor.js>
- `webdriver` crate：<https://crates.io/crates/webdriver>
- Servo 的 WebDriver：<https://github.com/servo/servo/tree/main/components/webdriver_server>
- Ladybird 的 WebDriver：<https://github.com/LadybirdBrowser/ladybird/tree/master/Services/WebDriver>
- wptrunner 的 product 怎麼註冊：`tools/wptrunner/wptrunner/products.py`（`BUILTIN_PRODUCTS` 和 `wptrunner.products` 的 entry point）、`browsers/__init__.py`
