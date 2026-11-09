# RFC 0027 的進度：`fetch`

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-10。完成：2026-10-10，macOS（Apple Silicon）。三段都在 `0027` 上做完，每一小段做完時 commit、審核；1a、1b 和 3a、3b 應要求先 merge 回 `main`，3a、3b merge 前 rebase 到 RFC 0029 之後的 `main`（「做過的檢查」）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. Promise 的形式和讀取（第 7 到 14 條） | 完成 | 2026-10-10 |
| 1a. 宣告表的 `Promise<T>`、DOM 的 task runner、`Response` 的建構子和 `text()` | 完成 | 2026-10-10 |
| 1b. `ByteString` 和 `Headers` | 完成 | 2026-10-10 |
| 1c. `Response` 的其他成員和 `json()` | 完成 | 2026-10-10 |
| 1d. `fetch()`：`protocol`、`net`、送出和回應、WPT、情境 | 完成 | 2026-10-10 |
| 2. `Request` 和送出（第 16 到 20 條） | 完成 | 2026-10-10 |
| 2a. `Request`、body 的 mixin、`fetch(RequestInfo)` | 完成 | 2026-10-10 |
| 2b. `net` 的 method、body 和 redirect 的模式 | 完成 | 2026-10-10 |
| 2c. `AbortSignal` 和取消 | 完成 | 2026-10-10 |
| 3. CORS（第 21 到 25 條） | 完成 | 2026-10-10 |
| 3a. `fetch()` 的 CORS | 完成 | 2026-10-10 |
| 3b. preflight | 完成 | 2026-10-10 |
| 3c. 子資源（module script、`crossorigin`） | 完成 | 2026-10-10 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **宣告表的 `Promise<T>`** | |
| realm 的 promise 表：交給 script 和 settle 都做了才拿掉（兩種順序）、只 settle 一次、realm 的表丟掉之後什麼都不在等、沒有表的 context 不建 promise | `cargo test -p bindings --test promise_resolvers` |
| 動態 `import()` 改用同一張表 | `cargo test -p dom_js --test dynamic_import` |
| JS：`text()` 回傳的 promise 在 DOM 的 networking task 裡才 resolve；null body 在回傳之前就 resolve；讀過的 body 和 `this` 不對時回傳被拒絕的 promise（`TypeError`），不丟出；document 丟棄時它的 task 跟著丟掉、promise 永遠不 settle | `cargo test -p dom_js --test fetch` |
| WIT：回傳 promise 的成員是 `async func`，`Promise<any>` 不在 `kun:dom` 裡；`kun:dom` 只有新增（`response`、`body-init`、`response-init`、RangeError 的代碼 18），不升版 | `cargo test -p dom_api --test wit` |
| wasm：guest 在 `text()` 裡等到 DOM 的 task 跑完、Store 被叫醒才拿到字串，等待中 Store 不被回收；之後的 `body-used`、`TypeError`（1）、`RangeError`（18）、null body 的空字串 | `cargo test -p dom_wasm --test engine a_guests_response` |
| Lua 的 runtime 照新的 WIT 產生（回傳字串的 `async func`、interface 和字串的 union、有預設值的整數欄位） | `python3 scripts/ci.py guests`、`cargo test -p dom_api --test wit the_lua_runtime_is_built_from_this_wit` |
| **DOM 的 task** | |
| 照延遲到期、同時的照排的順序、負的延遲是 0、`clearTimeout` 清不掉、document 丟棄時清掉、`DomIsolate` 拆掉時還有拿著 root 的 task 也拆得掉 | `cargo test -p dom --test timers`（`the_doms_tasks`、`the_dom_goes_with_a_task_pending`） |
| `AbortSignal.timeout()` 改成 DOM 的 task：JS、wasm 和 renderer 照舊 | `cargo test -p dom_js --test abort`、`-p dom_wasm --test engine a_guests_abort_controller`、`python3 scripts/test.py wpt dom/abort/` |
| **`ByteString` 和 `Headers`** | |
| guard：`immutable` 的改動是 `TypeError`，`request` 不理 forbidden request-header（瀏覽器的名稱、`Proxy-`、`Sec-`、method override），`response` 不理 `Set-Cookie`、`Set-Cookie2`；header list（大小寫沿用第一個、`set`、去掉頭尾空白、`get` 接起來、名稱是 token）；入口檢查 `ByteString` | `cargo test -p dom_api --test headers` |
| JS：init 是配對、record、另一個 `Headers`；iteration 排序和合併、`getSetCookie`；`TypeError`（三項的配對、不合法的名稱和值、超過 U+00FF、`null`） | `cargo test -p dom_js --test fetch headers` |
| wasm：`new`（配對、沒有 init）、`get`、pair iterable 的成員、`get-set-cookie`、`TypeError`（代碼 1） | `cargo test -p dom_wasm --test engine a_guests_headers` |
| record 的 key 有自己的型別（`Record<T, K>`） | `cargo test -p dom_api --test records`、`--test url` |
| GC 型別 | `cargo test -p dom --test script_objects headers`、`--test script every_type_unwraps_only_as_itself`、`--test gc_stress` 等 |
| **`Response`** | |
| `headers`（每次同一個、`response` guard、init 的之後補 body 的 `Content-Type`）、`type`、`url`、`redirected`、`clone()`、`json()`（在 task 裡 parse、null body 當場拒絕）、static 的 `error()`、`redirect()`、`json()` | `cargo test -p dom_js --test fetch`（`response_headers_and_clone`、`json_parses_the_body`、`the_static_members`） |
| wasm：`headers`、`type`、`clone`、static 的 `error`、`redirect` | `cargo test -p dom_wasm --test engine a_guests_response` |
| Lua：`Headers` 的 init（配對、record、`nil`）、`ResponseInit` 的 `headers` 和預設的 `status`、`text()` 的等待 | `python3 scripts/test.py reftest wasm/w9` |
| 建構子：預設值、`ok` 是 200 到 299、200 到 599 以外是 `RangeError`、不是 reason phrase 的 `statusText` 和 null body status 有 body 是 `TypeError`；`text()` 的 UTF-8 decode（去掉 BOM）、`URLSearchParams` 的 body | `cargo test -p dom_js --test fetch`（`the_constructor`、`text_settles_in_a_task`） |
| GC 型別：slot 撐住、只當自己讀回、wrapper 只 unwrap 成自己；工作負載裡只靠它撐住的 window 和 document、body 經過 GC 照樣讀得到 | `cargo test -p dom --test script_objects response`、`--test script every_type_unwraps_only_as_itself`、`--test gc_stress`、`--test incremental_stress`、`--test standalone_heap`、`--test detached_marking` |
| 宣告表和規範的 WebIDL 比對（讀 `fetch.idl`；kun 沒有的成員：`body`、bytes 的形式、`RequestInit` 的 `window`，在 `spec-missing.txt`） | `cargo test -p dom_api --test spec` |
| **`Request`** | |
| 建構子：預設值、init 的每個成員、method 的 normalize；網址、帳號密碼、`navigate`、`only-if-cached`、method、`GET` 的 body、`no-cors` 的 method、`referrer` 不合法是 `TypeError` | `cargo test -p dom_js --test fetch the_request_constructor` |
| 從另一個 `Request` 建：資料和 headers 沿用、init 有成員時重設 referrer、body 移過來（原本的不能再用）、signal 跟著（`signal: null` 不跟著，`clone()` 跟著） | `cargo test -p dom_js --test fetch a_request_from_a_request` |
| `no-cors` 的 headers（`request-no-cors` guard）；CORS-safelisted 的規則 | `cargo test -p dom_js --test fetch no_cors_headers`、`cargo test -p protocol --test network` |
| body：`text()`、`json()`、`clone()` 共用、`URLSearchParams` 的 `Content-Type`、null body；`fetch()` 一個 `Request` | `cargo test -p dom_js --test fetch`（`a_requests_body`、`fetch_of_a_request`） |
| wasm（Lua）：record 裡 optional 的 enumeration 和「物件或字串」的 union、`none` 是沒給（signal 跟著 input 的）、`fetch` 一個 `Request` | `python3 scripts/test.py reftest wasm/w9` |
| GC 型別 | `cargo test -p dom --test script_objects request`、`--test script every_type_unwraps_only_as_itself`、`--test gc_stress` 等 |
| **送出（任何 method 和 body）** | |
| `net`：任何 method 和 body、空的 `POST` / `PUT` 送 `Content-Length: 0`、`HEAD` 沒有 body（`data:` 也是）、不是 token 和被禁止的 method 擋下、有 initiator 的非 `GET` / `HEAD` 送 `Origin` | `cargo test -p net --test load`（`a_request_has_a_method_and_a_body`、`origin_and_head`） |
| `net` 的 redirect：301 / 302 的 `POST` 和 303 的 `GET` / `HEAD` 以外改成 `GET`、去掉 body 和它的 header，307 / 308 保留，到別的 origin 拿掉 `Authorization`；`error` 是網路錯誤，`manual` 交回 3xx（沒有 `Location` 的也算） | `cargo test -p net --test load`（`a_redirect_changes_the_method`、`redirect_modes`） |
| `fetch()` 交給 frame 的 method、body、redirect mode | `cargo test -p dom_js --test fetch fetch_of_a_request` |
| 整個瀏覽器：字串和 `Request` 的 `POST`、`PUT` 和 body、303 改成 `GET`、`error` 是 `TypeError`、`manual` 是 opaque-redirect 的 `Response` | `cargo test -p shell --test fetch methods_bodies_and_redirect_modes` |
| **`AbortSignal` 和取消** | |
| `dom` 的 abort steps：照順序交回、取回的不跑、document 丟棄時清掉 | `cargo test -p dom --test timers abort_steps` |
| signal 已經 abort 的 `fetch()` 以 reason 拒絕、不送；等待中 abort 以 reason 拒絕、frame 取消一次；`Request` 的 signal 也算；response 之後 abort 不取消、promise 不變，讀 body（和 clone 的）每次都以 reason 拒絕、不標成讀過；response 來時 abort steps 拿掉 | `cargo test -p dom_js --test fetch`（`fetch_aborts`、`an_abort_after_the_response`） |
| renderer：請求的 id 和共用的計數、取消只送一次、只取消被丟棄的 document 的 | `cargo test -p renderer --lib fetches_are_sent_and_cancelled` |
| `net`：取消的載入不回呼，伺服器看到請求被放棄；丟掉 handle 的照常完成 | `cargo test -p net --test load a_load_is_cancelled` |
| 整個瀏覽器：等待中 abort 馬上拒絕、伺服器看到放棄，`AbortSignal.timeout()`，response 之後 abort 讀 body；別的 realm（父 frame）abort iframe 的 `fetch()`、移除 iframe 取消它的 `fetch()`，主 frame 的照常完成 | `cargo test -p shell --test fetch`（`an_aborted_fetch`、`an_abort_from_another_realm`） |
| wasm（Lua）：signal 已經 abort 的 `fetch` 是 `AbortError`（代碼 12） | `python3 scripts/test.py reftest wasm/w9` |
| **CORS（`fetch()`）** | |
| `cors`：送 `Origin`；沒有、或是別的 origin 的 `Access-Control-Allow-Origin` 是錯誤；`*` 沒有 credentials 才算；有 credentials 要 `Access-Control-Allow-Credentials: true`；只留 safelisted 和 exposed 的 header（沒有 credentials 時 `*` 是全部），沒有 `Set-Cookie`；要 preflight 的和 forbidden header 是錯誤、什麼都沒送 | `cargo test -p net --test load cors_requests` |
| `no-cors` 的跨源是 opaque（`fetch()` 的什麼都沒有，子資源的留著 body），redirect 一定要 follow；`same-origin` 的跨源是錯誤、什麼都沒送；`file:` 在 `cors` 下是錯誤（opaque 的 origin 也是）；同源和 `data:`（任何 mode）是 basic，`GET` 沒有 `Origin` | `cargo test -p net --test load no_cors_and_same_origin_modes` |
| redirect：同源轉到跨源照 CORS；redirect 的 response 也要過 CORS；轉到第三個 origin 或轉回來就 taint（`Origin: null`），轉回來照樣是 CORS；location 帶帳號密碼是錯誤 | `cargo test -p net --test load cors_redirects` |
| `fetch()` 交給 frame 跨源的請求，帶著 mode 和 credentials mode | `cargo test -p dom_js --test fetch fetch_hands_its_frame_the_request` |
| 整個瀏覽器：同源的 redirect（`redirected`、最後的網址去掉 fragment）；不允許的跨源是 `TypeError`（直接或經過 redirect），允許的是 `cors`、header 過濾；`no-cors` 是 `opaque`（status 0、網址空的）；`file:` 是 `TypeError`、`data:` 是 `basic`；對方收到這個 origin 的 `Origin` | `cargo test -p shell --test fetch where_a_fetch_may_go` |
| **preflight** | |
| CORS-unsafe request-header names：小寫、排序、去重，safelisted 的值超過 1024 byte 時全部算 | `cargo test -p protocol --test network` |
| 什麼時候送、送什麼（`Origin`、`Accept`、`Access-Control-Request-Method` 和 `-Headers`，沒有 body 和 script 的 header）；不快取；回應要過 CORS check、2xx（允許了請求的 redirect 也不算）、清單是 token；method（safelisted 的不用、`*` 沒有 credentials 才算、大小寫要對）和 header（`*` 沒有 credentials 才算、`Authorization` 要明列）；同源的不送 | `cargo test -p net --test load preflights` |
| 整個瀏覽器：跨源的 `PUT` 加自訂 header 先過 preflight 再送，不允許的 `DELETE` 是 `TypeError`、沒送出 | `cargo test -p shell --test fetch a_preflighted_fetch` |
| 沒有盡頭的 preflight 的 body 不讀 | `python3 scripts/test.py wpt fetch/api/cors/cors-preflight-unterminated-body` |
| **CORS（子資源）** | |
| `crossorigin` 的 CORS settings：module 一定照 CORS、classic script 有屬性才照、wasm 不照；`use-credentials` 不分大小寫是 `include`，其他值和沒有屬性的 module 是 `same-origin`；inline module 也帶著 | `cargo test -p dom_api --lib script`（`prepare_takes_crossorigin`） |
| 一個 graph 的 module 都用 root 的 credentials mode 抓（inline 的 root 也是） | `cargo test -p renderer --lib modulator`（`descendants_take_the_roots_credentials`） |
| 整個瀏覽器：跨源的 module script（和它 import 的）、`modulepreload`、`import()`、帶 `crossorigin` 的 classic script 要對方允許，沒有 `crossorigin` 的 classic script 照 `no-cors`；`use-credentials`（inline module 的 import 也是）時 `*` 不算；每個 CORS 請求帶這個 origin 的 `Origin` | `cargo test -p shell --test modules subresources_from_another_origin_go_by_cors` |
| `file:` 頁面的 module 和 `modulepreload` 擋下，`data:` 的 module 和檔案的 classic script 照樣載入 | `cargo test -p shell --test modules a_file_pages_modules_are_blocked` |
| **`fetch()`** | |
| 參數：網址照 document 的 base URL 解析，不合法的和帶帳密的是 `TypeError`；method 照規範 normalize，不是 token 和 forbidden 的是 `TypeError`；`file:` 不交給 frame；同一個名稱的 header 合成一個；initiator 是 document 的 origin（opaque 是 `null`）；沒有 frame 時是網路錯誤 | `cargo test -p dom_js --test fetch`（`fetch_checks_its_arguments`、`fetch_hands_its_frame_the_request`） |
| 整個瀏覽器：同源的 status、status text、`type`、`url`、`redirected`、headers（沒有 `Set-Cookie`、immutable）、`text()`、`json()`；404 照樣 resolve；204 的 body 是 null | `cargo test -p shell --test fetch a_same_origin_fetch` |
| 送出的 header：script 給的（forbidden 的不送）、預設 `Accept: */*`，script 給了 `Accept` 時用它的 | `cargo test -p shell --test fetch the_requests_headers` |
| `fetch()` 不延後 `load`；和頁面的子資源共用請求 id 的計數（樣式表的回應不會被當成 `fetch()` 的） | `cargo test -p shell --test fetch`（`a_fetch_does_not_delay_load`、`a_fetch_and_the_pages_loads`） |
| `net`：送出 request header、擋下 forbidden header、`data:` 的 `Content-Type`、`Location` 當 UTF-8 讀 | `cargo test -p net --test load`（`a_fetch_sends_its_headers`、`a_data_url_has_a_content_type`、`a_location_is_utf_8`） |
| Lua（wasm 的 `async func`）：`fetch` 一個 `data:` 網址，`file:` 是錯誤 | `python3 scripts/test.py reftest wasm/w9` |
| 視窗裡：`about:demo` 的 `fetch()` 按鈕讀一個 `data:` 網址，把 `json()` 的結果寫進頁面 | `python3 scripts/test.py scenarios demo-script` |
| **WPT** | |
| `fetch/api/` 的 `basic`、`headers`、`request`、`response`、`redirect`、`abort`、`cors`；`url/` 裡用 `fetch()` 讀資料的和 `url/idlharness.any.js`；之前停在 `fetch` 的 `idlharness`；module 的 `crossorigin.html`、`dynamic-import/dynamic-imports-fetch-error.sub.html` 整份通過。跳過的（`disabled` 和理由）：`abort/general.any.js`（每次等滿 60 秒）、`cors/cors-keepalive.any.js`（同樣等滿逾時）、`basic/response-null-body.any.js`（`docs/limitations.md` 的「網路」） | `python3 scripts/test.py wpt`（0 個不符） |

## 做過的檢查

2026-10-10，macOS（Apple Silicon）。每一段的設計和更正寫進了 `README.md`；這裡是濃縮的紀錄。

- **設計和規劃不同的地方**（都寫進了 `README.md` 的「詳細設計」）：
  - Promise 的表原本打算「函式不在建立 promise 的那次呼叫裡 settle」，但 Fetch 的 "consume body" 對 null body 當場 resolve，所以改成「交給 script」和「settle」都做了才拿掉，兩種順序都可以（1a）。
  - `dom` 的 `TimerTask` 拿掉：`AbortSignal.timeout()` 搬到 DOM 的 task 之後只剩一種（1a）。`ResponseInit.status` 需要 `RangeError`：`ApiError` 多了 `Range`，WIT 多了代碼 18（只是新增，`kun:dom` 不升版）。
  - `text()` 的 task 排在執行中的 realm 的 document（promise 所在的 realm），不是 `Response` 的 relevant global：兩者只在跨 realm 呼叫 `Response.prototype.text` 時不同，這時照 promise 的 realm 才 settle 得到。
  - `ByteString` 原本只在入口檢查，WPT 的 `headers-record.any.js` 抓到和 WebIDL 的順序不同（不合法的值要在讀下一個屬性之前丟），照第 9 條原本的寫法改回 JS 的前端在轉換時檢查（1b）。record 的 key 也是 `ByteString`，宣告表的 `Record<T>` 改成 `Record<T, K = DomString>`。
  - `request-no-cors` guard 從 1b 延到 2a（要 CORS-safelisted 的規則，放在 `protocol::network`，`net` 的 CORS 也用）。
  - `RequestInit` 的 `signal: null` 要拿掉 input 的 signal，宣告表多了 `Present<T>`（2a）；guest 的 record 表示不出 `signal: null`（`docs/limitations.md`）。`RequestInit` 的 `window` 不宣告。`kun:dom` 升到 0.12.0（`fetch` 的 `input` 變成 `request-info`）。
  - abort steps 原本打算放在 GC 物件上、要求 `Send` 來保證沒有 root，但 `v8::Global` 是 `Send`；改成 algorithm 只記編號，steps 放在 heap 外的 `DomTasks`，不用新的 `unsafe impl`（2c）。steps 在 response 來時取回（否則長命的 signal 每次 `fetch()` 都留一個）；別的 realm 的 abort 排一個 networking 的 task 去拒絕；iframe 被移除也取消它的 `fetch()`；多做了 renderer 結束時 IO thread 取消它所有的載入。
  - 3a 原本把要 preflight 的請求當成網路錯誤，3b 換成真的 preflight；preflight 不讀回應的 body（WPT 抓到）。
  - `file:` 頁面的 module（M1）不用另外寫：origin 是 opaque、`file:` 不是 HTTP(S)，`net` 的 CORS 就擋下（3c）。
- **WPT 和審查抓到、修掉的**：
  - 1d：送給網路的同名 header 合成一行（`header-value-combining`）；response header 的值用 isomorphic decode（`request-headers-nonascii`）；HTTP/2 沒有 reason phrase；`net::test_server` 讀 request 的開頭改成 Latin-1。
  - 2b：`HEAD` 一個 `data:` 網址沒有 body；`POST` 帶 `Origin`；`Location` 裡不是 UTF-8 的 byte 照 Chrome percent-encode（`redirect-location-escape.tentative.any.js`）；沒有 body 的 `PUT` 送 `Content-Length: 0`（`net` 的測試抓到）。
  - 2c：abort 之後讀 body 每次都以 reason 拒絕，不標成讀過（`general.any.js`）。
  - 3a：`data:` 在 `same-origin` mode 下是 basic，Fetch 先判斷 `data:`（`scheme-data.any.js`），`net` 的測試原本也照錯的行為寫。
  - 3b：preflight 的回應原本整份讀完 body，沒有盡頭的讓請求卡住（`cors-preflight-unterminated-body.any.js`），transport 多了不讀 body 的選項。
  - bindings（不是這份的功能，有了 `fetch()` 之後 `idlharness` 跑得到才找到，使用者決定在這個 branch 上修）：interface 的 prototype 唯讀（`read_only_prototype()`）、attribute 的 getter 和 setter 名稱是 `get x`、`set x`，`[Replaceable]` 的 setter 檢查 `this`（`dom_js` 的 `templates.rs`，照 Blink）。測試 `cargo test -p dom_js --test bindings interface_prototypes_and_accessor_names`。
  - Lua 的產生器：W9 的新項目找到 enum 的值的舊錯（`cors "` 先比對到 `no-cors "no-cors"`），改成找前面有空白的。
- **還和 Chrome 不同的**（都記進了 `docs/limitations.md`）：送出的 header 名稱是小寫（hyper）、HTTP/1 的 status text 是標準的 reason phrase、沒有 `Accept-Language`；header 的值有控制字元時送不出去（hyper）；421 不重送；`Origin` 不看 referrer policy；preflight 不快取；動態 `import()` 一律以 `same-origin` 抓；`no-cors` 跨源的 classic script 的錯誤不 mute。
- **不穩定的測試**：
  - `fetch/api/basic/response-null-body.any.js`（2c）：它的 `status.py` 對 204、205、304 也送 body；hyper 不讀那段 body、把連線放回連線池，下一個重用它的請求把殘留的 bytes 當成回應解析而失敗，輪到哪個請求看時機。Chromium 的 `HttpStreamParser` 在回應之後還有資料時不重用連線。使用者決定先記成限制、跳過這份（不選完全不重用連線、也不選遇到解析錯誤時重送冪等的請求）。
  - runner 的 port（RFC 0029 的，不是這份的）：wptrunner 替每個 process 的 machopodriver 挑一個空的 port、關掉之後才交給它，平行的 process 多時會撞到（`Address already in use`），那組測試被報成「沒有跑」。用 `--processes 8` 降低機率，每次核對開始和跑完的測試數一樣，被報成沒跑的單獨跑都照預期。
- **mutation test**：每一段一份 spec，全部抓到。
  - `mutants-1a.toml` 32 條（「之後拒絕時 reason 是 `undefined`」1a 還沒有成員會在之後拒絕，1c 的 null body 的 `json()` 抓到）、`1b` 19、`1c` 17、`1d` 30、`2a` 45、`2b` 27、`2c` 21、`bindings` 5、`3a` 35、`3b` 23、`3c` 19。
  - 一開始沒抓到、補了測試的：`take_resolver` 不檢查已經 settle（補「settle 之後、還沒交出去之前再 settle」）；名稱可以是空的；`fetch()` 自己數請求 id（補 `a_fetch_and_the_pages_loads`）；init 有成員時重設 referrer 等五個 `Request` 的檢查；clone 的 headers 保留 guard、`redirect()` 的 headers 是 immutable；移除的 iframe 的 `fetch()` 照跑、IO thread 不取消（`test_server` 加了 `abandoned`，請求要在頁面結束之前回應才不被 renderer 結束時的取消遮住）；`file:` 在 `cors` 下是錯誤、`Access-Control-Allow-Origin` 是別的 origin、轉到別的 origin 再轉回來；preflight 的 header `*` 帶 credentials、同源不送 preflight、允許了請求但 status 是 3xx。
  - 等價的（記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）：入口不照 `K` 檢查 record 的 key；`net` 擋不是 token 的 method（reqwest 也拒絕）；init 有成員時把 `navigate` 改成 `same-origin`；`request-no-cors` 的 `delete` 的名稱限制和拿掉 `Range`；`abort_fetch` 在 frame 沒有取消時不拒絕；判斷 basic 時看 tainting 還是 basic。
  - spec 的維護：每段結束時的 fmt 換了行、之後的段落又改了程式，2a 到 3b 有 21 條的原文對不上；完成第 3 段時 18 條照現在的程式改寫、單獨重跑都抓到，要改的程式已經不在的 3 條拿掉（3a 的「不需要 preflight」兩條、2a 的「`fetch()` 送任何 method」）。六份 spec 的每一條原文現在都剛好出現一次。Lua 產生器的改動沒有列 mutation（重編 guest 會改到 repo 裡的 `.wasm`），由 W9 守住。
- **WPT 的清單**：1b 加 `fetch/api/headers/`，1c 加 `response/`，1d 有了 `fetch()` 之後整個重跑（23 個預期結果往下跑得更遠；`url/idlharness.any.js` 和因為 `fetch` 跳過的重試一次）並加 `basic/`，2a 加 `request/`，2b 加 `redirect/`，2c 加 `abort/`，3a 加 `cors/`；3c 之後 `module/crossorigin.html`、`dynamic-imports-fetch-error.sub.html` 整份通過。跳過的寫成 `disabled` 加理由（驗收表）。3a、3b 先 merge 之前 `main` 多了 RFC 0029（WPT 改由 wptrunner 跑），rebase 時 WPT 的改動照新格式重做：預期結果用 `MACHOPO_UPDATE_EXPECTATIONS=1 test.py wpt` 只對 `include.txt` 列的目錄重新產生（給 `fetch/` 會連沒有納入的測試也寫出 `.ini`）。
- **merge 回 `main`**：1a、1b（第 15 條）、第 1 段、第 2 段（連同 bindings 的修正）、3a、3b（第 26 條），每次 merge 之前跑 `cargo fmt --check`（連同 `sdk/lua/generator`）和 clippy（`-D warnings`），修在那一段的最後一個 commit；抓到的：沒有 `Drop` 的 scope 被 `drop`（幾處，改成區塊）、test module 之後還有函式、`loop` 可以寫成 `while let`、條件不會變的 `while read_body`。
- **每段的 `cargo test --workspace`**：1a 1731、1b 1739、1c 1758、1d 1769、2a 1777、2b 1782、bindings 1783、2c 1790、3a 1792、3b 1795、rebase 到 RFC 0029 之後 1779（拿掉了舊 runner 的測試）、3c 1783，每次都全部通過；每段也跑 `sanity.py deps`、`boundary`、`unsafe`、`docs`（只有 `main` 上原本就有的根目錄 README 和 RFC 0021 那兩項）、`test.py wpt`（0 個不符）、`test.py reftest wasm/w9`、動到 Lua 時 `ci.py guests`，動到 `about:demo` 時 `test.py scenarios demo-script`。
- **merge 前 rebase 到 `main`**（2026-10-10）：`main` 多了 8 個 commit（`86fc7811` machopodriver 自己挑 port，和 RFC 0028 的第 1 到 4 段）。衝突：`frame_view.rs` 的 `start_loads`（`<img>` 改由「update the image data」要圖，`PendingLoad::Image` 多了 `delays_load`；照 `main` 的寫法，換成 3c 的 `ResourceRequest`），和 `tests/wpt/README.md`、`docs/limitations.md`、`docs/architecture.md`、`crates/dom_api/README.md`、`rfcs/README.md` 的同一段落（以 `main` 的為準，套上這份的改動）。之後 `cargo fmt`（解衝突的兩行）、clippy、`cargo test --workspace` 1814 個全部通過、`test.py reftest` 108 個 0 不符、`sanity.py` 的四項（`docs` 只剩 RFC 0021 那一項）、`test.py wpt` 4321 個測試 id 跑完：1 個不符，`html/webappapis/timers/timer-nesting-not-inherited-in-microtask.html` 要 timer 在 4 ms 內觸發，量到 6.7 ms；單獨連跑 5 次都通過。這是負載下的時間誤差（machopodriver 自己挑 port 之後照預設的 process 數跑，32 個 kun 同時跑），kun 的 timer 沒有被 clamp（被 clamp 的話單獨跑也會失敗），和這份無關。merge 之後照使用者的決定，把它的子測試記成可能不穩定（`expected: [PASS, FAIL]`），並在 `shell` 的 `script` 測試補上不靠時間、照順序檢查同一件事的 `a_timer_set_in_a_microtask_is_not_nested`（mutation：timer 的 task 在 microtask 之後才結束、不 clamp，都抓到）。
- **RFC 完成時**（`docs/README.md` 的「一份 RFC 做完時」，2026-10-10，macOS（Apple Silicon））：
  - **GC**：這份 RFC 動到 GC（`Headers`、`Request`、`Response`、DOM 的 task 和 abort steps），跑了 `test.py matrix`：8 組設定（每種 marking mode 配上 V8 預設的 pool、1 個和 8 個 worker，再各加 stress 的 flag）都通過，每組 86 個 test binary 的 794 個測試；`test.py fuzz --iterations 20`：20 組隨機的設定都通過（seed `0x9e35f918`）。fuzz 要 `../third_party` 都在釘住的 commit 上，第一次因為沒有 `speedometer` 在開始前停下，使用者同意之後用 `ci.py third-party --clone` 補上再跑。
  - **automation**：`test.py automation` 的 33 份情境和 `pipe --window`（`automation_pipe` 的 `the_real_window_is_driven_the_same`）都通過，截圖在 `target/automation-results/`。第一次跑時情境都過了，`pipe --window` 在第一張截圖（`Kun.captureWindow`）逾時，之後重跑和在幾個舊的 commit 上跑也一樣，一度以為是 macOS 上原本就有的問題。用 `sample` 抓 stack、照測試的步驟手動送指令都找不到卡住的地方，之後同樣的測試又連過 5 次；`pmset -g log` 顯示螢幕在 15:24:52 閒置關掉、15:47:10 才打開，情境的最後一張截圖是 15:24:50，失敗的那幾次都在這段期間。螢幕關掉時 macOS 把視窗當成被遮住，Viz 整個 frame 跳過，截圖照設計等一個真的畫出來的 frame。`CLAUDE.md` 要的 `caffeinate -i` 只防系統睡眠：`test.py automation` 改成在 macOS 上自己跑 `caffeinate -d -i`（跑的期間 `PreventUserIdleDisplaySleep` 是 1），寫進 `docs/testing.md` 的「工具和跑法」。查的時候在共用 `target/` 的 worktree 裡編過舊的 commit，cargo 之後沿用了舊程式碼編的 workspace crate，`touch` 原始檔重編之後才對。
  - **WPT 的重試**：16 份 `.ini` 的 `disabled` 理由還寫著「no CORS yet」，暫時拿掉、重跑一次：`fetch/api/basic/mode-no-cors.sub.any.js` 的 window 版本整份通過，改成照常跑（worker 版本是 `ERROR`）；兩個 `dynamic-imports-credentials` 跑過了跨源的 `fetch()`，在有結果之前停在 `postMessage`；其他的照樣停在原本列的理由（Worker、`crypto`、字串的 `setTimeout`、`import defer`、跨源的 `Location` 丟 `TypeError`）。還是跳過，理由改寫。
  - **文件的一致性**：交給一個唯讀的 agent 對過這份 RFC 的 commit 改過的檔案和因此過時的註解，找到的都改了：`docs/limitations.md` 裡 `crossorigin`、`mode` 和 `credentials` 的說明和舊 runner 的說法（skip 清單、baseline）；`docs/testing.md` 的 CORS 這個安全邊界（加進「只靠審查」，連同 initiator 由 renderer 給的問題）、取消的測試名稱、WPT 跑了哪些、URL 資料那一列；`docs/architecture.md` 的 preflight、`dom` 的新 GC 型別和 DOM task、`bindings` 的 promise 表；`crates/dom/README.md` 和 `soundness.md` 的 `Response.signal`；`crates/dom_api/README.md` 的 `FetchRequest`、`cancel_fetch`、`Task::Reject` 和 `headers` 測試；`crates/dom_js/README.md` 的 abort 測試；`HTMLLinkElement`、`HTMLScriptElement` 的 `crossOrigin` 的註解；`tests/wpt/README.md` 的時態和方向、`include.txt` 的註解。
  - **最後的檢查**：`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace` 1783 個全部通過、`sanity.py deps`、`boundary`、`unsafe`、`docs`（只有 `main` 上原本就有的兩項）、`test.py wpt`：4113 個測試 id 跑完（開始和跑完的數目一樣），0 個不符。
