# RFC 0008 的細節：DOM API 擴充和 WPT 的 testharness 測試

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

除非另外註明，WPT 都是 commit `0f195499f06d0833e768b01a1a8ce942b8622fc8`（`third_party.toml` 固定的那一個），行號指那個版本的檔案。調查在 2026-10-05 做，用 sparse checkout 和 grep，腳本沒有放進 repo。

## testharness.js 需要什麼

`/resources/testharness.js` 共 5209 行。下面分成「一定會碰到」「有 feature check 保護」「只在失敗或顯示結果時用到」三類，前提是 output 關掉（`setup({output: false})`），在最上層的 window 裡。

### 環境的判斷

- 整個檔案是 `(function(global_scope){...})(self)`：`self` 一定要是 global，對外的函式由 `expose()` 寫到它上面。
- `create_test_environment()`（523 行）：`'document' in global_scope` 就是 `WindowTestEnvironment`；否則依序判斷各種 worker、ShadowRealm，最後是只靠 `Promise` 的 `ShellTestEnvironment`。

### 一定會碰到的

- **`WindowTestEnvironment`**（51–113 行）用 `window` 這個名稱（不是 `self`）呼叫 `addEventListener('load')` 和 `('message')`。
  - `load` 之後用 `setTimeout(…, 0)` 設 `all_loaded`；**沒有分派 `load`，harness 永遠不會完成**，只能等到逾時。
- **`Tests` 的建構子**（3665 行）呼叫 `test_timeout()`（225 行），讀 `document.getElementsByTagName("meta")` 的 `length`、`[i]`、`.name`、`.content`。
  - 沒有 `name`、`content` 不會壞，只是 `<meta name=timeout content=long>` 沒有效果。
- **5047 行**：global 有 `addEventListener` 時，註冊 `error` 和 `unhandledrejection`。
  - 要把未捕捉的例外變成 harness 的錯誤，需要 `ErrorEvent` 的 `message`、`error`、`filename`、`lineno`、`colno`，以及 `PromiseRejectionEvent` 的 `reason`。
- **`_dispatch` / `_forEach_windows`**（115–162 行），每個 start、result、completion 都跑：
  - `while (w != w.parent)`：`window.parent` 一定要有，最上層時是自己，`undefined` 會丟 `TypeError`；
  - 讀 `window.opener`，`null` 或 `undefined` 都可以；
  - 只有 `w !== self` 時才 `postMessage`。
  - `setup({message_events: []})` 可以連這段都不掛上。
- **`test()` 的名稱**：從單行 arrow function 的原始碼推出（`get_test_name`，577 行），需要 `Function.prototype.toString` 保留原始碼（V8 有）。

### 有 feature check 保護的

- **`AbortController`**（2755 行）：有的話每個 test 都 `new AbortController()`，cleanup 時呼叫 `abort("Test cleanup")`。**只做一半的 `AbortController` 會讓每個測試在這裡壞掉**，所以要嘛不做，要嘛做完整。
- **`get_title()`**（5013 行）：測試沒有名稱時讀 `document.getElementsByTagName("title")[0].firstChild.data`（刻意不用 `document.title`），其次是 `location.pathname`。
- **`setTimeout`**：沒有時改用以 `Promise` 做的 `fake_set_timeout`。

### 只在失敗、或顯示結果時用到

- **斷言失敗時**，`get_stack()`（4799 行）用 `new Error().stack`，加上 `document.getElementsByTagName("script")` 和 `.src`（解析過的絕對 URL）去掉 harness 自己的 stack frame；找不到時有退路。
- **`format_value`**（1418 行）只在組失敗訊息時呼叫：
  - 它用 duck typing 判斷節點：`nodeType`、`nodeName`、`nodeValue`、`childNodes` 四個都在，就讀全域 `Node` 的常數，元素再讀 `localName`、`attributes[i].name` / `.value`、`innerHTML`，文字和註解讀 `data`。
  - **風險**：這四個屬性都有、但還沒有 `innerHTML` 時，失敗訊息會變成 `TypeError`（測試照樣是失敗，只是訊息沒用）。第 3 步加上 `childNodes` 時要注意。
- **output 打開時**（預設），`Output` 要 `getElementById`、`createElementNS`、`querySelectorAll`、`readyState` 等，每次斷言也都要 `format_value` 和 stack。kun 的 `testharnessreport.js` 關掉它。

### 結果、逾時和其他 API

- **回呼**（4231–4262 行）：`add_start_callback`、`add_test_state_callback`、`add_result_callback`、`add_completion_callback(tests, harness_status, asserts_run)`。
  - `Test`：`name`、`status`（PASS 0、FAIL 1、TIMEOUT 2、NOTRUN 3、PRECONDITION_FAILED 4）、`message`、`stack`。
  - harness 的狀態：`status`（OK 0、ERROR 1、TIMEOUT 2、PRECONDITION_FAILED 3）、`message`、`stack`。
  - 測試名稱重複時，harness 的狀態是 ERROR。
- **`setup()` 要在第一個 test 之前呼叫**：`testharnessreport.js` 緊接在 `testharness.js` 之後載入，時機沒問題。
- **逾時**：預設 10 秒，`<meta name=timeout content=long>` 是 60 秒。`explicit_timeout: true` 時 harness 不自己計時，由外面呼叫全域的 `timeout()`（4205 行），wptrunner 就是這樣做。
- **`promise_test`**：所有 `promise_test` 串成一條依序執行，需要 microtask 的 checkpoint 確實排空。
- **`assert_throws_dom`**（2270 行）：要 `self.DOMException`，丟出的例外的 `constructor === self.DOMException`、有 `code` 和 `name`。kun 的 `DOMException` 是 `glue.js` 定義的，要確認這幾點。
- **`EventWatcher`**（956 行）：`addEventListener`、`removeEventListener`、`event.type`、`Promise`。
- **harness 本身不用**：`document.write`、iframe、`XMLHttpRequest`、`queueMicrotask`。`fetch` 只在 `fetch_json` 和 idlharness 裡。

### 上游和各家的 `testharnessreport.js`

- 上游的版本只有 32 行，在 try/catch 裡讀 `window.opener.testharness_properties`，不需要 DOM。
- wptrunner 的版本：`setup({output, timeout_multiplier, explicit_timeout, debug, message_events: ["completion"]})`，`add_completion_callback` 把結果推進一個佇列，由 WebDriver 讀。
- Servo 的版本：在完成時 `console.log("ALERT: RESULT: " + JSON.stringify(...))`。

### `.any.js` 和 `.window.js` 的包裝（`tools/serve/serve.py`）

- `X.window.js` 由 `X.window.html` 提供（`WindowHandler`，304 行），依序是：`<!doctype html>`、`<meta charset=utf-8>`、META 產生的 `<meta>` / `<title>`、`testharness.js`、`testharnessreport.js`、每一行 `// META: script=` 的 `<script>`、`<div id=log>`、`X.window.js` 本身。
- `X.any.js` 在 window 裡由 `X.any.html` 提供（`AnyHtmlHandler`，464 行）：一樣，只是前面多一段 inline script 定義 `self.GLOBAL`（`isWindow()` 是 true）。
- META：`timeout=long` 變成 `<meta name=timeout content=long>`；`title=` 變成 `<title>`；`global=` 決定有哪些變體（預設是 window 和 dedicated worker；kun 只跑 window）；`variant=` 產生帶 query 的多個 URL。
- `/resources/WebIDLParser.js` 不存在於 repo，是 wptserve 改寫到 `/resources/webidl2/lib/webidl2.js`（serve.py 883 行）。
- `dom/` 的 HTML 測試全部用 `/resources/testharness.js` 這種從根目錄開始的路徑；`.sub.` 的檔案要 wptserve 替換。

## WPT `dom/` 的調查

### 怎麼算

- 範圍是 `dom/` 底下的檔案，不含路徑裡有 `resources/`、`support/`、`tools/`、`crashtests/`、`reference(s)/` 的，也不含 `-manual`、`-ref`、`-notref`。
- HTML（`.html`、`.htm`、`.xhtml`、`.xht`、`.svg`）內容有 `/resources/testharness.js` 才算；`.any.js`、`.window.js`、`.worker.js` 都算。以檔案為單位，不展開 variant。
- 判斷一個檔案「用到」什麼時，把它經過 `<script src>` 或 `// META: script=` 引入的本地 helper 遞迴併進來（最多 3 層，不算 testharness 和 testdriver）。
- 都是 regex 的估計：只表示「用到的名稱都存在」，不表示 subtest 會過；例外的種類、選擇器的完整度、live 集合、`[SameObject]` 才決定實際的通過率。

### 目錄

| 子目錄 | 檔案 | 組成 |
| --- | --- | --- |
| `nodes` | 391 | 376 個 HTML、15 個 `.window.js`；底下有 `moveBefore/` 56、`insertion-removing-steps/` 22、`Document-contentType/` 15 |
| `events` | 182 | 166 個 HTML、9 個 `.any.js`、6 個 `.window.js`、1 個 `.worker.js`；底下有 `non-cancelable-when-passive/` 42、`scrolling/` 39 |
| `ranges` | 67 | HTML |
| `observable` | 27 | 多數是 `.any.js` |
| `traversal` | 18 | 17 個 HTML、1 個 `.window.js` |
| `collections` | 11 | 10 個 HTML、1 個 `.window.js` |
| `dom/` 本身 | 10 | 包括 `idlharness.window.js`、`interface-objects.html` |
| `abort` | 6 | 4 個 `.any.js`、2 個 HTML |
| `lists` | 5 | HTML |

### 擋住測試的功能

「硬阻擋」是 iframe（和其他 window）、另一份 document、XML 的測試檔、shadow DOM、testdriver、網路和 worker 這六種。

| 目錄 | 檔案 | iframe | 另一份 document | XML 檔 | shadow | testdriver | 沒有硬阻擋 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `nodes` | 391 | 54 | 59 | 42 | 25 | 6 | 224 |
| `events` | 182 | 20 | 4 | 0 | 9 | 84 | 71 |
| `ranges` | 67 | 7 | 28 | 0 | 12 | 3 | 22 |
| `traversal` | 18 | 4 | 3 | 0 | 0 | 0 | 11 |
| `collections` | 11 | 0 | 0 | 0 | 0 | 0 | 11 |
| `lists` | 5 | 0 | 0 | 0 | 0 | 0 | 5 |

- **iframe 的用法**（`nodes`、`events` 等目錄裡用 iframe 的 85 個檔案）：62 個是空白或 `about:blank`，只是要另一份 document 或 window；26 個把 iframe 拆掉，測 not fully active；少數載入 XML、跨來源或用 `srcdoc`。
- **只被「另一份 document」擋住的 53 個**：只有 11 個只用 `createHTMLDocument`，其餘也用 `createDocument`、`new Document()`、`DOMParser` 或 `createDocumentType`，也就是要 `XMLDocument`。
- **最核心的 `Node` 測試都被「另一份 document」擋住**：`Node-appendChild`、`insertBefore`、`replaceChild`、`removeChild`、`cloneNode`、`textContent`、`isEqualNode`、`normalize`、`childNodes`、`ParentNode-append`、`prepend`、`replaceChildren`、`Node-compareDocumentPosition`、`Node-contains`、`Node-properties`。它們直接或經過 `pre-insertion-validation-*.js`、`dom/common.js` 用 `createHTMLDocument` 或 `createDocument`。
- **`dom/common.js`**（1089 行）不用 iframe，但 `setup()` 建一棵大的測試樹，要 `new Document()`、`createCDATASection`、`createHTMLDocument`、`createDocumentType`、`createDocument`、`createProcessingInstruction`。`ranges` 的大多數測試、`traversal` 的 `TreeWalker` 和 `NodeIterator` 用它。

### 成員的使用頻率

範圍是 `nodes`、`events`、`collections`、`lists`、`traversal`（607 個檔案），只算出現在 DOM、HTML、UI Events、CSSOM、HR-Time 的 IDL 裡的成員。「可用」是其中沒有硬阻擋的 322 個檔案裡用到的數量。前面標 B 的是 kun 已經有的。

| 成員 | 全部 | 可用 | | 成員 | 全部 | 可用 |
| --- | --- | --- | --- | --- | --- | --- |
| `body` B | 253 | 84 | | `nodeName` | 36 | 18 |
| `createElement` B | 251 | 125 | | `createElementNS` | 35 | 14 |
| `getElementById` B | 200 | 97 | | `cloneNode` | 34 | 5 |
| `appendChild` B | 193 | 88 | | `nodeValue` | 31 | 15 |
| `addEventListener` B | 191 | 66 | | `getElementsByTagName` | 28 | 19 |
| `querySelector` B | 129 | 61 | | `createEvent` | 27 | 25 |
| `remove()` | 83 | 25 | | `lastChild` | 27 | 10 |
| `id` | 67 | 43 | | `initEvent` | 25 | 22 |
| `append` | 65 | 30 | | `setAttributeNS` | 23 | 10 |
| `querySelectorAll` | 53 | 43 | | `head` | 21 | 12 |
| `nodeType` | 52 | 19 | | `className` | 19 | 18 |
| `createDocumentFragment` | 51 | 20 | | `previousSibling` | 19 | 9 |
| `childNodes` | 47 | 17 | | `MutationObserver` 的 `observe` | 19 | 14 |
| `innerHTML` | 45 | 24 | | `firstElementChild` | 18 | 7 |
| `createComment` | 43 | 22 | | `attributes` | 17 | 10 |
| `ownerDocument` | 39 | 4 | | `childElementCount` | 14 | — |
| `getElementsByClassName` | 37 | 35 | | `removeAttribute`、`matches`、`hasAttribute` | 12–14 | — |
| `createHTMLDocument` | 37 | 0 | | `children`、`lastElementChild`、`item` | 11–15 | — |

另外：`.length` 出現在 144 個檔案、`instanceof` 66 個、`for…of` 或 `Symbol.iterator` 67 個、`assert_throws_dom` 52 個、`new Event(` 45 個、`.data` 41 個。`document.title` 在 `dom/` 裡沒有用到。

### 每一批 API 解開多少

範圍是沒有硬阻擋、或只被「另一份 document」擋住的 386 個檔案。把每個檔案用到、kun 還沒有的成員歸成群組，一個檔案要的群組都有了就算「解開」，每一步選解開最多的群組：

| 步 | 加上的群組 | 新解開 | 累計 | 例子 |
| --- | --- | --- | --- | --- |
| 0 | 現有的 API（加上 harness 的前置） | 30 | 30 | `Event-dispatch-order.html`、`EventTarget-removeEventListener.any.js`、`Event-isTrusted.any.js`、`ParentNode-querySelector-*` 的一部分 |
| 1 | 集合、`ParentNode`、`ChildNode`、`Element` | 97 | 127 | `getElementsByClassName-*`、`Element-childElementCount*`、`Element-closest`、`ParentNode-children`、`ParentNode-querySelectorAll-*`、`collections/HTMLCollection-*` |
| 2 | 事件補齊 | 40 | 167 | `Event-dispatch-*`、`CustomEvent.html`、`Event-constructors.any.js`、`AddEventListenerOptions-once.any.js` |
| 3 | `Node` 的基本成員 | 22 | 189 | `Element-firstElementChild`、`Node-childNodes-cache*`、`NodeList-Iterable` |
| 4 | namespace 和 `Attr` | 23 | 212 | `Element-removeAttributeNS`、`Element-setAttribute`、`DOMTokenList-coverage-for-attributes` |
| 5 | `CharacterData`、`Text`、`Comment`、`DocumentFragment` | 29 | 241 | `CharacterData-*`、`Text-splitText`、`Node-nodeName`、`Node-nodeValue`、`DocumentFragment-constructor` |
| 6 | `innerHTML`、`outerHTML` | 11 | 252 | `ChildNode-after`、`before`、`replaceWith` |
| 7 | traversal | 11 | 263 | `TreeWalker-*`、`NodeIterator-*` |
| 8 | `MutationObserver` | 11 | 274 | |
| 9 | 另一份 document | 12 | 286 | 和第 10 列一起做的話一共 52 |
| 10 | `Document` 的其他成員、`HTMLElement` | 40 | 326 | `implementation`、`doctype`、`head`、`contentType`、`click()` |

`README.md` 的「分階段」大致照這個順序，但把第 3 和第 5 列併成一步，第 9 和第 10 列併成「document 的模型」：它們互相依賴，而且「另一份 document」解開的是最核心的 `Node` 測試，數量看不出它的分量。

### `html/` 裡現在就接近能跑的

| 目錄 | 檔案 | 沒有硬阻擋 | 還要什麼 |
| --- | --- | --- | --- |
| `html/webappapis/timers` | 16 | 13 | 字串的 handler（`evil-spec-example`）、window 的 `error`（`errors.html`）、`queueMicrotask` |
| `html/webappapis/animation-frames` | 12 | 11 | `document.hidden`、window 的 `error` |
| `html/webappapis/scripting/events` | 51 | 20 | `window.onerror`、`ErrorEvent`、`click()` |
| `html/dom/documents/dom-tree-accessors` | 55 | 33 | `document.title`、`getElementsByName`、`document.head` |

## wasmtime 和 wit-parser 的版本機制

版本：wasmtime 49.0.2；wit-parser 0.261.0（`dom_api`、Lua 的產生器）和 0.258.3（wasmtime）；`sdk/rust` 的 wit-bindgen 0.62.0（wit-parser 0.259.0）。三個 wit-parser 的 `stability()` 和 `include_stability()` 內容相同。行號指 `~/.cargo/registry/src/index.crates.io-*/` 底下的原始碼。

### linker 的 semver 比對

- 規則在 `wasmtime-environ-49.0.2/src/component/names.rs` 的 `alternate_lookup_key`（293–318 行）：
  - 有 prerelease 的版本不和其他版本相容；
  - major 不是 0 時，相容的鍵截到 major（`kun:dom/dom@1.1.0` 的鍵是 `kun:dom/dom@1`）；0.x 截到 minor；0.0.x 沒有相容的鍵。
- `NameMap::get` 先找完全相同的名稱，找不到再用相容的鍵；`insert` 在同一條相容線上只留最大的版本。兩個方向都成立：host 給 1.0.0、guest 要 1.1.0，或反過來，都找得到同一個 instance（`wasmtime-49.0.2/src/runtime/component/linker.rs` 51–54 行的說明）。
- guest 的 export 也走同樣的比對，所以 `crates/dom_wasm/src/store.rs` 寫死的 `kun:dom/guest@1.0.0` 也找得到 1.1 的 guest。
- host 多定義、guest 沒 import 的函式沒有影響（`matching.rs` 154–174 行）。guest import 了 host 沒有的函式，`instantiate_pre` 失敗（"function implementation is missing"）。
- wit-component 只把 core module 真的用到的函式編進 import（`wit-component-0.259.0/src/encoding.rs` 582–595 行）：照 1.1.0 編、但沒用到新函式的 guest，在 1.0.0 的 kun 上也能跑。C 的 guest 經過 wasm-ld 和 wasm-component-ld，推論相同，沒有實測。

### 動態的 host 函式幾乎不檢查型別

- `wasmtime-49.0.2/src/runtime/component/func/host.rs`（699–705 行）：`func_new` 定義的函式在連結時只檢查是不是 async。
- 所以 record 多一個欄位、enum / variant 多一個 case、函式改了參數，都不會在 instantiate 時被擋下，要到呼叫時才在 `dom_wasm` 的轉換裡失敗，或回傳時 wasmtime 依 guest 的型別檢查 `Val` 而 trap。
- wasmtime 的文件說 semver 比對假設同一個 major 裡的介面不會變；這要靠 kun 自己守（golden file）。

### `@since` 和 `@unstable`

- 語法在 `wit-parser-0.261.0/src/ast.rs`（1709–1775 行）：`@since(version = 1.1.0)`、`@unstable(feature = name)`，還有 `@deprecated(version = ...)`。
- 能標在：interface、world、interface 裡的函式和具名型別（record、variant、enum、flags、resource）、resource 的方法、static 函式、建構子。**不能**標在 record 的欄位、enum / variant 的 case 上。
- `@since` 和 `@unstable` 不能同時用；`@deprecated` 要搭配其中一個。
- `include_stability`（`resolve/mod.rs` 2004–2059 行）：
  - `@unstable`：`Resolve` 的 `features` 有它、或 `all_features` 時才保留，否則丟掉；
  - `@since`：一定保留，但 package 要有版本，而且 `since` 不能大於 package 的版本（"feature gate cannot reference unreleased version"）。所以 `@since(version = 1.1.0)` 要和 package 升成 1.1.0 一起做。
- 一個沒有 gate 的項目用到被丟掉的型別時，`Resolve` 報錯：unstable 的型別只能給同樣 unstable 的函式用。
- 打開 feature：
  - Rust：`generate!({ features: ["name"], ... })`（`wit-bindgen-rust-macro-0.62.0/src/lib.rs`），沒有 `all_features`；
  - C：`wit-bindgen c --features name` 或 `--all-features`（看 GitHub 上 v0.62.0 的 `src/bin/wit-bindgen.rs`，本機沒有原始碼）；
  - Lua 的產生器（`sdk/lua/generator/src/main.rs`）和 `dom_api` 的 `crates/dom_api/tests/wit.rs` 都用 `Resolve::default()`，unstable 的項目會被靜默丟掉。
- wasmtime 只有 `bindgen!` 那條路看 feature；kun 用 `func_new`，host 照舊定義全部的函式，component 的 import 名稱也不帶 gate 的資訊。`@unstable` 純粹是 guest 產生 binding 時的開關。

## 沒選的做法

### runner 從 `file:` 載入

- 要把 `/resources/...` 對應到 WPT 的根：改寫測試檔（匯入的檔案就和上游不同），或在 `net` 的 `file:` loader 裡為 WPT 開特例。
- `.any.js` 的包裝還是要產生，只是寫成檔案。
- `location`、`document.URL`、同源的判斷和真的瀏覽器不同，有些測試會讀它們。
- HTTP server 的成本小：`net::test_server` 已經有了，只多「以目錄為根的靜態檔案」和包裝。

### 結果經過 stdout 或 `document.title`

- stdout：headless 的 renderer 是同一個 process 裡的 thread，runner 要攔 stdout，而且和頁面自己的 `console.log`、`[uncaught]` 混在一起。Servo 能這樣做是因為每個測試一個 process。
- `document.title`：要先有 setter；頁面自己改 title 會蓋掉結果；長度和字元也要處理。

### WIT 的其他演進方式

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| 照現在：1.0.0、只新增、不標 | 什麼都不用改 | guest 看不出函式是哪一版才有的；這份 RFC 期間剛加的簽章也不能改 |
| 每次新增都升 minor，標 `@since` | 精確 | kun 沒有發布的週期，版本號沒有對外的意義；`@since` 散在很多版本 |
| `@unstable`，RFC 做完時升 1.1.0 | 期間可以改新的成員；照 1.0.0 編的 guest 一直能跑 | 既有的成員和字典不能改；guest 的作者要打開 feature；Lua 的產生器和測試要改 |
| 退回 0.1.0，不相容的改動直接做（「已決定的事項」第 1、2 條） | 什麼都能改；不用 gate | 每次不相容的改動，之前編的 guest 都要重新編 |
| 升 2.0.0 | 可以改既有成員 | 舊的 guest 不能跑，或 host 要同時提供兩個版本；現在沒有非改不可的成員 |

### 字典的成員增加

（`kun:dom` 留在 1.x 時才需要比較；退回 0.x 之後直接改。）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| record，第一次進入穩定的 WIT 時就做完整；不得已時新增 record 和函式（`new2`） | 有型別；guest 寫起來和現在一樣 | 1.0.0 裡已經不完整的字典要用 `new2` |
| record，直接改（0.x，「已決定的事項」第 2 條） | 有型別；WIT 沒有舊的形式 | 改了就不相容 |
| 名稱和值的 `list` | 可以任意增加成員 | 失去型別；每個 guest 都要自己檢查名稱和型別 |
| 字典改成 resource，加上設定每個成員的函式 | 只需要新增函式 | 寫起來囉嗦；每個字典多一種 resource 和 handle |

### 宣告表從 WebIDL 檔產生

- Blink 的做法：`.idl` 加上 extended attribute，code generator 產生 binding。
- kun 的宣告表除了 IDL 的資訊，還有 Rust 函式的路徑、降階、`async method`、`ObjectKind`；要從 IDL 產生，就要把這些寫成 extended attribute，再寫一個 generator。
- `README.md` 第 6 節的比對拿到主要的好處（差距看得見），成本小很多。

## 其他實作

- **Chromium**：
  - `third_party/blink/web_tests/resources/testharnessreport.js` 把結果轉成文字，content_shell 經過 `testRunner` 交出去，和 `-expected.txt` 比對；所有 subtest 都通過的測試沒有 `-expected.txt`。
  - WPT 放在 `web_tests/external/wpt`，由 wptserve 提供，另外有 `TestExpectations` 和 `NeverFixTests` 標記不跑的測試。
  - `LiveNodeList`、`HTMLCollection` 靠 DOM tree version 讓快取失效，`childNodes` 這類 `[SameObject]` 的集合存在節點的 `NodeListsNodeData` 上。
- **Servo**：wptrunner 啟動 servo，`testharnessreport-servo.js` 在完成時 `console.log` 結果；每個目錄有 `.ini` 的 metadata 記錄預期。
- **WASI**：0.2.x 之內用 `@since` 和 `@unstable` 演進，新的介面先是 unstable，穩定時改成 `@since`。

## 參考資料

- WPT 的 `/resources/testharness.js`、`/resources/testharnessreport.js`、`/resources/idlharness.js`、`tools/serve/serve.py`、`tools/wptrunner/wptrunner/testharnessreport.js`（commit 見開頭）。
- WIT 的 feature gate：component-model repo 的 `design/mvp/WIT.md`，Feature Gates 一節。
- wasmtime 49.0.2：`wasmtime-environ/src/component/names.rs`、`wasmtime/src/runtime/component/linker.rs`。
