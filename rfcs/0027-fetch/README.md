# RFC 0027：`fetch`

- 開始日期：2026-10-10
- 狀態：✅ 已完成：2026-10-10（「已決定的事項」第 1 到 6 條 2026-10-10 接受；第 1 段開始前第 7 到 12 條、開始後第 13、14 條，第 2 段開始前第 16 到 20 條，第 3 段開始前第 21 到 25 條；三段（各分成 a、b、c、d 或 a、b、c）都做完，只在 macOS（Apple Silicon）上做；1a、1b（第 15 條）和 3a、3b（第 26 條）先 merge 回 `main`；驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`
- 進度：`progress.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 L，雜項線的下一項（A 是 RFC 0015、B 是 RFC 0019，都做完了）。M4 的 Perf-Dashboard 用 `fetch` 載入 JSON。

## 摘要

加上 Fetch Standard 的 `fetch()`、`Request`、`Response`、`Headers`。請求照現有的子資源的路走：renderer 經過 `protocol` 交給 Browser 的 `net`，回應整份收完再交回 renderer 的 task；`net` 多了 method、request header、body 和 redirect 的模式，可以取消；CORS（連同 preflight）放在 Browser 的 network 層（Chromium 的 `network::CorsURLLoader`），module script、`modulepreload` 和帶 `crossorigin` 的 classic script 的抓取也一起照 CORS，`file:` 頁面的 module 照 Chrome 擋下。宣告表多一種 `Promise<T>` 的回傳：JS 是 promise，WIT 是 `async func`，引擎中立的 resolver 在 `dom_api`（Blink 的 `ScriptPromiseResolver`）。沒有 `ReadableStream`、XHR、cookie 和 HTTP cache；bytes 的形式（`arrayBuffer()`、`BufferSource` 的 body、`blob()`）跟 `Blob` 一起在 H2。

## 動機

**現況（2026-10-10，`main` 的 `6fcfd03`；細節在 `details.md` 第 1 節）**：
- 頁面沒有任何發出請求、讀回應的 API：沒有 `fetch`、`XMLHttpRequest`，也沒有 `Request`、`Response`、`Headers`。
- `net` 只有 `GET`，請求只帶 `Accept` 和 User-Agent，沒有 request header 和 body（`docs/limitations.md` 的「網路」的「只有 `GET`」）。
- 沒有 CORS：module script 的抓取不檢查 CORS，跨源和 `file:` 頁面的 module 照樣載入、錯誤訊息不 mute（`docs/limitations.md` 的「Script」）。RFC 0015 的 G1 把它留給這份：「CORS 應該放在 Browser 的 network 層，和 `fetch` 一起做一次」。
- 宣告表沒有 Promise 的形式：現在的 promise 都在宣告表之外手寫（`scroll` 系列在 `dom_js` 的 glue 包一層 `Promise.resolve()`，動態 `import()` 的 resolver 在 `bindings`，只有 V8）。RFC 0017 的筆記留下的看法是「Promise 的形式由先需要它的 RFC 設計」，`Response.text()` 是第一個。
- **等著它的**：Speedometer 的 Perf-Dashboard 用 `fetch` 和 `Response.text()` 載入 JSON（RFC 0012 的盤點）；RFC 0019 的 `url/` 的 WPT 用 `fetch` 讀資料，等這份做完才加；WPT 的 idlharness 用 `fetch` 讀 IDL；`AbortSignal.any()` 的 GC 和每份文件的延遲 task runner 都等「有 `fetch` 之後」（RFC 0010 的「之後的可能」）。

目標：
- `fetch()`、`Request`、`Response`、`Headers` 照 Fetch Standard，成員照 Blink 的 `core/fetch/` 放進宣告表，JS 和 wasm 都有（RFC 0012 的第 7 條；JS 才有的成員見「詳細設計」）。
- 任何 method、request header、字串和 `URLSearchParams` 的 body、`redirect` 的三種模式、`AbortSignal`。
- CORS 在 Browser 的 `net`：`Origin`、simple request 和 preflight、`mode`、response tainting（`basic`、`cors`、`opaque`）；module script 的抓取也照它。
- WPT 的 `fetch/api/` 裡用得到的部分，和 `url/` 裡用 `fetch` 的。

不在範圍內（「之後的可能」）：
- `ReadableStream` 和串流的 body（`Response.body`、`Request.body`、`duplex`）：要 Streams Standard，和 `net` 的串流（RFC 0004 的 data pipe）一起。
- bytes 的形式：`arrayBuffer()`、`bytes()`、`blob()`、`formData()`、`BufferSource` / `Blob` / `FormData` 的 body，跟 `Blob` 一起在 H2。
- `XMLHttpRequest`：盤點裡只有 jQuery、Backbone 偵測它的建構子，沒有真的送出。
- cookie、`credentials` 的效果（kun 沒有 cookie，`credentials` 只影響 CORS 檢查的條件）、HTTP cache 和 `cache` 選項、`keepalive`、`priority`、Service Worker、CSP、mixed content、SRI（`integrity` 只存起來）、referrer policy。

## 設計概要

對寫網頁的人：
- `const r = await fetch("data.json"); const data = await r.json();` 照規範：同源、`data:` 的網址直接讀；跨源的照 CORS，伺服器沒有 `Access-Control-Allow-Origin` 時 promise 以 `TypeError` 拒絕。`fetch("file:///…")` 以 `TypeError` 拒絕，和 Chrome 一樣。
- `fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(x), signal })`；`new Request(…)`、`request.clone()`、`new Response("text", { status: 201 })`、`Response.json(x)`、`Response.error()`；`headers.get()`、`append()`、`for (const [k, v] of headers)`。
- `response.body` 不存在（沒有 Streams）；`arrayBuffer()`、`blob()` 要等 H2。

對改 kun 的人：
- 宣告表可以寫 `-> Promise<T>`：API 的函式拿到一個 `PromiseResolver`，當場或之後（另一個 task）resolve 或 reject；`dom_js` 給頁面一個 promise，`dom_wasm` 是 `async func`，guest 等它的時候 Store 讓出，resolve 時由 engine 的 task 叫醒（和 WASI 的時鐘一樣）。
- `protocol` 的 `ResourceRequest` 多了 method、header、body、mode、credentials mode、redirect mode 和 initiator 的 origin，`RequestDestination` 多了 `Empty`（`fetch()` 的 destination）；`net` 的 HTTP 送得出任何 method，CORS 在 `net` 裡、Browser 這一側。
- renderer 這一側：`dom_api` 的 `FrameClient` 在 `fetch()` 呼叫的當下登記、送出請求，回應的 task 裡 settle promise（resolver 的表和動態 `import()` 共用）。`fetch` 不延後 `load`。

## 詳細設計

（範圍和整體的選項在 `details.md` 第 3 節，理由在「已決定的事項」。）

- **Promise 的形式**（第 2、8、10 條）：宣告表的回傳型別 `Promise<T>`（`Base::Promise`、`Value::Promise`）。`dom_api` 的 `PromiseResolver<T>`（Blink 的 `ScriptPromiseResolver`，`src/promise.rs`）：函式用 `PromiseResolver::new(cx)`（`ScriptHost::create_promise`）在執行中的 realm 建一個、回傳它，留著一份（只是編號），之後 `resolve` / `reject`（`ScriptHost::settle_promise`）；可以在回傳之前就 settle（null body 的 `text()`），要當場拒絕時直接回傳錯誤。`bindings` 每個 realm 一張表（`src/promise_resolvers.rs` 的 `PromiseResolvers`，`dom_js` 的 `JsRealm` 擁有），一個 promise 要「交給 script」和「settle」都做了才從表裡拿掉，順序不拘；動態 `import()` 也改用它（原本的 `Linkage.imports` 拿掉）。`dom_js` 的 member callback 在 `TryCatch` 裡呼叫這種成員：`this`、參數轉換和函式的錯誤都變成被拒絕的 promise（WebIDL）。WIT 是 `async func -> result<T, dom-exception>`：`dom_wasm` 的 `call_promise` 跑完入口之後等 engine 的 promise 表（`src/promises.rs`；Store 的 data 必須是 `Send`，所以入口照 `FrameScope` 放在 thread-local），settle 時叫醒 Store 的 waker（Store 不在 poll 時排一個 engine 的 task，和 WASI 的時鐘一樣），等待中算進 Store 的等待、撐住它。`T` 是 `any` 的（`json()`）只給 JS（RFC 0008 的第 8 條），guest 用 `text()` 自己解析。Lua 的 runtime 的 generator 多認得回傳字串的 `async func`、interface 和字串的 union 的參數、有預設值的整數欄位。
- **DOM 的 task**（第 13 條）：`dom` 的 `DomTasks`（`src/tasks.rs`）和 `DomMutationScope::queue_dom_task`：document、task source（networking、timer）、到期的時間（event loop 的時間加上延遲）和 `dom_api` 的 `Task`（`Box<dyn Any>`）；document 丟棄時清掉它的，`DomIsolate` 拆掉時在檢查 root 之前清掉全部。`dom_api` 的 `task` 模組：`Task::AbortTimeout`（`AbortSignal.timeout()` 從 timer 清單搬過來，`dom` 的 `TimerTask` 跟著拿掉，timer 只剩 script 的 callback）、`Task::ReadBody`（讀 body 的 task：bytes 轉成文字或 JSON 再 settle）。render thread 的 `run_due_dom_tasks` 在 timer 之後取出到期的，在那份 document 的 realm 裡 `run_task`，接著 microtask checkpoint；event loop 等待的時間也算進它們。
- **`Headers`、`Request`、`Response`**：`dom` 的 GC 型別（Blink 的 `core/fetch/headers.h`、`request.h`、`response.h`），header list 和 body 的 bytes 是 `NoTrace` 的資料（Blink 的 `FetchHeaderList`、`FetchRequestData`、`FetchResponseData`）。`Response`（`dom` 的 `src/fetch.rs`）拿著 realm 的 window 的 `Edge<Window>` 和 `ResponseData`（種類、網址、status、status text、`Arc<[u8]>` 的 body、讀過了沒有）；建構子照 Fetch 的 "initialize a response"（status 不在 200 到 599 是 `RangeError`：`ApiError` 多了 `Range`，WIT 的代碼 18）。`Headers`（`dom` 的 `src/fetch.rs`：名稱和值的配對、`HeadersGuard`；演算法在 `dom_api` 的 `headers.rs`）照 RFC 0019 的 `[PairIterable]`（排序和合併在 iteration 的時候做），guard 有 `immutable`、`request`、`request-no-cors`（`no-cors` 的 `Request` 的）、`response`、`none`。宣告表新增 `ByteString`（第 9 條：JS 的前端照 WebIDL 在轉換時檢查，超過 U+00FF 是 `TypeError`，入口再檢查一次給 wasm；WIT 是 `string`）；record 的 key 有自己的型別（`Record<T, K>`，`Headers` 的 init 是 `record<ByteString, ByteString>`）。body 的 mixin（`text()`、`json()`、`bodyUsed`）是 `Request` 和 `Response` 共用的函式。`Request`（`dom` 的 `src/fetch.rs`、`dom_api` 的 `request.rs`）拿著 realm 的 window、`Headers` 和 `signal`（`Edge<EventTargetImpl>`）三條 edge 和 `RequestData`；建構子照 Fetch 的 "new Request"，`fetch()` 也先建一個再送；signal 是 input 或 init 的 signal 的 dependent（`create_dependent_abort_signal`），`signal: null` 不跟著 input 的，所以宣告表多了 `Present<T>`：字典成員給了才是 `Some`，給了 `null` 是 `Some(None)`（WIT 是 `T` 的型別，`dom_wasm` 把 record 的 nullable 欄位的 `none` 當成沒給）。CORS-safelisted 的規則在 `protocol::network`（`net` 的 CORS 也用）。body 的 mixin 是 `dom_api` 的 `body.rs`。
- **`fetch()` 的路**（第 12 條）：`dom_api` 的 `fetch()`（`src/fetch.rs`，Blink 的 `global_fetch.cc`）照 "new Request" 解析網址（document 的 base URL）、檢查帳密和 method，用 `request` guard 的 `Headers` 收 init 的 header，同一個名稱的值接成一個（Blink 的 `FetchHeaderList` 送給網路時也是這樣），算出 initiator（document 的 origin，opaque 是 `null`）；`data:` 和 `http(s):` 才送（另一個 origin 的照 CORS，由 `net` 檢查），`file:` 是 `TypeError`（Chrome 的訊息），其他 scheme 是網路錯誤。接著建 promise，交給 `FrameClient::fetch`（`FetchRequest`：網址、method、header、body、redirect mode、mode、credentials mode、initiator、promise、document、abort steps），它當場送出。renderer 的實作是 `src/loader.rs` 的 `FetchLoader`（每個 frame 共用一個，放在 `FrameSettings`）：從 render thread 的 `RequestIds`（子資源、child frame 的導覽和 `fetch()` 共用一個計數，回應的 id 才不會撞）拿 id，送出 `RendererHostMsg::FetchResource`（destination 是 `Empty`），以 id 留著 document 和 promise。回應照舊是 `RendererMsg::ResourceResponse`，render thread 先看是不是 `fetch()` 的，是的話在那份 document 的 realm 裡呼叫 `dom_api` 的 `did_receive_response`：濾掉 `Set-Cookie`、建 `immutable` 的 `Headers`，照 `net` 給的 tainting 建 `basic`、`cors` 或 `opaque` 的 `Response`（null body status 沒有 body），resolve；網路錯誤以 `TypeError`（「Failed to fetch」）拒絕，接著 microtask checkpoint（Blink 的 `FetchManager::Loader`）。document 已經不在時丟掉；登記留到回應來（`net` 一定回應，成功或錯誤）。`fetch()` 不延後 `load`。
- **`net` 的請求和回應**（第 16 條）：`ResourceRequest` 多了 `method`、`headers`、`body`、`redirect`（`RedirectMode`）、`initiator`、`mode` 和 `credentials`（`ResourceRequest::new` 是 `GET`、沒有 header 和 body、`Follow`、沒有 initiator，導覽和子資源照舊）。`HttpTransport::send` 送任何 method、request header 和 body（script 給了 `Accept` 時不用預設的 `*/*`，沒有 body 的 `POST`、`PUT` 送 `Content-Length: 0`）；`URLRequest` 先擋 forbidden request-header（`protocol::network::is_forbidden_request_header`，`dom_api` 的 `Headers` 也用它）、不是 token 和被禁止的 method，照 Fetch 的 "HTTP-redirect fetch" 改寫（301 / 302 的 `POST` 和 303 的 `GET` / `HEAD` 以外改成 `GET`、去掉 body 和 request-body-header，到別的 origin 時拿掉 `Authorization`）；`Error` 的 redirect 是網路錯誤，`Manual` 不跟、把 3xx 的 response 交回，由 `dom_api` 的 `did_receive_response` 包成 opaque-redirect 的 `Response`（Chromium 的 network service 也是交回 redirect，Blink 包）。`HEAD` 的回應不管 scheme 都沒有 body（Fetch 的 "main fetch"）。`ResourceResponse` 多了 `status_text`（HTTP/2 沒有 reason phrase，是空的）、`redirected` 和 `tainting`；header 的值照 Fetch 用 isomorphic decode（Latin-1），`Location` 再編回 bytes、當 UTF-8 讀，不是 UTF-8 的 byte 照 Chrome percent-encode；`data:` 的回應有 `Content-Type`。request header 照 Fetch 的 forbidden request-header 名單，renderer 這一側先擋一次，`net` 再擋一次（renderer 不可信，`docs/architecture.md` 的「Process 的邊界」）。
- **CORS**（第 3、21、22 條）：`net` 的 `cors.rs`（Chromium 的 `CorsURLLoader`），只管有 initiator 的請求（`fetch()` 的，和照 CORS 抓的子資源）：`mode` 是 `RequestMode`（`SameOrigin`、`NoCors`、`Cors`），`credentials` 是 `CredentialsMode`，回應帶著 `ResponseTainting`。每一步照 Fetch 的 "main fetch" 決定 tainting（同源而且還是 basic、或 `data:`：basic；`same-origin` 的跨源：網路錯誤；`no-cors`：opaque，redirect mode 一定要 `follow`；`cors`：只有 HTTP(S)，tainting 是 cors），每一步重新加 `Origin`（CORS 的請求、或 `GET` / `HEAD` 以外，taint 之後是 `null`），tainting 是 cors 時每個 response（redirect 的也是）都要過 "CORS check"（`Access-Control-Allow-Origin` 是 `*` 而且沒有 credentials，或等於請求的 origin；有 credentials 時還要 `Access-Control-Allow-Credentials: true`）。redirect 照 "HTTP-redirect fetch"：location 帶帳號密碼在 CORS 下是網路錯誤，從別的 origin 轉到第三個 origin（或轉回來）就 taint。最後照 tainting 過濾（第 22 條）：cors 的只留 CORS-safelisted 的 response header 和 `Access-Control-Expose-Headers` 的（沒有 credentials 時 `*` 是全部），永遠沒有 `Set-Cookie`；`fetch()` 的 opaque 沒有 status、header 和 body，子資源的留著 body。renderer 照 tainting 建 `Response`（`basic`、`cors`；`opaque` 的網址也是空的），`dom_api` 不擋跨源的請求（`file:` 是 `TypeError`）。kun 沒有 cookie，`credentials` 只改變 CORS 要求對方的條件。
- **preflight**（第 23 條，Fetch 的 "CORS-preflight fetch"，Chromium 的 `PreflightController`，不快取）：tainting 是 cors、method 不是 CORS-safelisted 或有 CORS-unsafe 的 header（`protocol::network::cors_unsafe_request_header_names`：safelisted 的值加起來超過 1024 byte 時全部算）的每一步，先送 `OPTIONS`（`Origin`、`Accept: */*`、`Access-Control-Request-Method`、有 unsafe header 時 `-Headers`；不帶 body 和 script 的 header，不讀回應的 body：沒有盡頭的也不會卡住），回應要過 CORS check、status 是 2xx（redirect 不算），`Access-Control-Allow-Methods` 和 `-Headers` 的每一項都是 token，允許 method（safelisted 的不用；沒有 credentials 時 `*` 也算，method 照大小寫比對）和每個 unsafe header（不分大小寫；沒有 credentials 時 `*` 也算，`Authorization` 一定要明列），才送出請求。transport 的 `send` 多了不讀 body 的選項，給 preflight 用（WPT 的 `cors-preflight-unterminated-body.any.js` 抓到原本讀完 body 會卡住）。
- **照 CORS 抓的子資源**（第 24、25 條）：module script（和它 import 的）、`modulepreload`、帶 `crossorigin` 的 classic script 以 `cors` mode 抓（Blink 的 `FetchParameters::SetCrossOriginAccessControl`），initiator 是 document 的 origin（opaque 的是 `null`），`crossorigin` 決定 credentials mode（`use-credentials` 不分大小寫是 `include`，其他值、和沒有屬性的 module 是 `same-origin`）。`dom_api` 的 `Prepared` 帶著它（`script::cors_credentials`），renderer 的 fetch closure 收 `ResourceRequest`，`frame_view` 的 `cors_request` 建請求；modulator 的每個 graph 記住 root 的 credentials mode，descendant 照它抓（HTML 的 fetch options）；動態的 `import()` 一律 `same-origin`（規範沿用 import 它的 script 的，記進 `docs/limitations.md`）。沒有 `crossorigin` 的 classic script、樣式表和圖片照舊沒有 initiator，不經過 CORS（子資源的 opaque response 留著 body，結果和 `no-cors` 一樣）。`file:` 頁面的 module 不用特別處理：origin 是 opaque、`file:` 不是 HTTP(S)，`net` 的 CORS 就是網路錯誤；`data:` 的照樣是 basic。
- **`AbortSignal`**（第 17 到 19 條）：`dom` 的 abort algorithm 多一種帶著 `dom_api` 的 abort steps 的（`add_abort_steps`）。steps 不放在 GC 物件裡：algorithm 只記編號，steps（`Box<dyn Any>`）在 heap 外的 `DomTasks`，和 DOM task 一樣在 document 丟棄、`DomIsolate` 拆掉時清掉；`v8::Global` 是 `Send`，沒辦法用型別保證 `Box<dyn Any>` 裡沒有 root，放在 heap 上就要替它論證 `NoTrace`。`fetch()` 的 signal 已經 abort 時以 reason 拒絕、不送；送出時登記 `AbortFetch`（document、promise、`FrameClient::fetch` 交回的請求 id、signal），response 來時由 `did_receive_response` 取回（Blink 在 loader 結束時放掉它的 algorithm），`Response` 記住 signal（`set_response_signal`，`clone()` 也帶著）。abort 時 `abort_fetch` 請 frame 取消（`FrameClient::cancel_fetch`：還在等才取消），取消了就以 reason 拒絕 promise（在它自己的 realm；別的 realm abort 的話排一個 networking 的 task）；response 之後 abort，讀 body 以 reason 拒絕、不標成讀過（Chrome 也是）。renderer 的 `FetchLoader::cancel` 送 `RendererHostMsg::CancelResource`，Browser 的 IO thread 以請求 id 記著 `net` 的 `LoadHandle`，`cancel()` 丟掉那個 tokio task、不回呼（Blink 的 `FetchManager::Loader::Abort`、`URLLoader` 的關閉）。document 丟棄時 render thread 取消不再 fully active 的 document 的 `fetch()`（第 19 條），renderer 結束時 IO thread 取消它所有還在跑的載入（Chromium 關掉它的 `URLLoader`）。
- **`file:`**（第 5、11 條）：`fetch()` 遇到 `file:` 的網址以 `TypeError` 拒絕（Chrome 的「URL scheme "file" is not supported」）；`data:` 照規範（`basic`，不經過 Browser 也可以，但為了一條路，照樣經過 `net`）。
- **Process 的邊界**：新的欄位和訊息都經過 `protocol`、可以序列化；CORS 和 forbidden header 的檢查在 Browser，renderer 的檢查只是提早失敗。

### 測試

- `net` 的 Rust 測試（`net::test_server`）：method、header、body、redirect 的三種模式和 method 的改寫、forbidden header、CORS 的各種組合和 preflight、`Origin`、取消。
- `dom`、`dom_api`、`dom_js`、`dom_wasm` 的單元測試：新的 GC 型別、`Promise<T>` 的 resolve / reject（同一個 task、之後的 task、realm 消失）、`Headers` 的排序和合併、body 只能讀一次、wasm 的 guest（`fetch.c`：`async func` 等回應、`text()`、`headers` 的 iterable）。
- `shell` 的端到端測試：`net::test_server` 提供頁面和資料，頁面 `fetch` 之後交出結果（`testRunner`）；另一個 port 的 server 當作別的 origin，測 CORS、preflight、module 和 `crossorigin`；`file:` 頁面的 module。
- **WPT**：`fetch/api/` 的 `basic`、`headers`、`request`、`response`、`body`、`abort`、`redirect`、`cors` 裡能跑的（wptserve，`test.py wpt`），每段開始時試跑；加上 `url/` 裡用 `fetch` 讀資料的（RFC 0019 留下的）。盤點在 `details.md` 第 4 節。
- **情境**：`about:demo` 加一個 `fetch` 一個 `data:` 網址、把結果寫進頁面的按鈕。
- mutation test：每一段一份 spec（`mutants-1a.toml` 到 `mutants-3c.toml`，另有 `mutants-bindings.toml`），弄壞 CORS 的檢查、preflight、forbidden header、body 只能讀一次、resolver 的 settle、取消、子資源的 credentials mode 等；結果在 `progress.md`。

### 分階段

三段都在 `0027` 上做，每一小段做完時 commit、審核；1a、1b 和 3a、3b 應要求先 merge 回 `main`（第 15、26 條），其他照「流程」第 5 步在一段做完時 merge。

1. **Promise 的形式和讀取**（2026-10-10；1a 宣告表的 `Promise<T>`、DOM 的 task、`Response` 的建構子和 `text()`，1b `ByteString` 和 `Headers`，1c `Response` 的其他成員和 `json()`，1d `fetch()`）：宣告表的 `Promise<T>`（JS、WIT）和 `ByteString`；`Headers`；`Response`（建構子收字串和 `URLSearchParams`、`status`、`ok`、`statusText`、`url`、`redirected`、`type`、`headers`、`bodyUsed`、`text()`、`json()`、`clone()`、static 的 `error()`、`json()`）；`fetch(url, init)` 的 `GET`：網址是字串，init 只收 `headers` 和 `method`，`GET` 以外的 method 以 `TypeError` 拒絕（不默默送成 `GET`）；同源和 `data:`，跨源和 `file:` 是網路錯誤。`protocol` 加 `RequestDestination::Empty`、request header 和 initiator 的 origin，`net` 送得出 request header（forbidden header 兩邊都擋）。做完時 Perf-Dashboard 要的 `fetch` 都有了。
2. **`Request` 和送出**（2026-10-10；2a `Request`、body 的 mixin、`fetch(RequestInfo)`，2b `net` 的 method、body 和 redirect 的模式，2c `AbortSignal` 和取消）：`Request` 和 `RequestInfo`、其他的 `RequestInit`（任何 method、body、`redirect`、`signal`；`mode`、`credentials` 先存起來）、`net` 的 method、body 和 redirect 的模式、`AbortSignal` 和取消。
3. **CORS**（2026-10-10；3a `fetch()` 的 CORS，3b preflight，3c module script、`modulepreload` 和 `crossorigin`）：`net` 的 CORS loader 和 preflight、`mode` 和 `credentials` 生效、response tainting 和濾掉的 header；子資源照 CORS，拿掉 `docs/limitations.md` 的那一條。

## 缺點

- **宣告表多一種跨三層的形式**：Promise 在宣告表、`bindings`（每個 realm 的表）和 `dom_wasm`（host 的 future 和叫醒 Store）各有一份實作，GC 和 realm 的生命週期要各自對。
- **沒有 Streams 時，`Response.body` 不存在**：偵測 `response.body` 的程式（例如逐塊讀的進度條）走別的路或壞掉；WPT 裡用 `body.getReader()` 的測試全部失敗。
- **整份收完**：大的回應整份在記憶體裡、經過 `protocol` 複製一次，`fetch` 下載大檔案時比 Chrome 慢、吃記憶體。
- **CORS 是新的安全邊界**：寫錯時跨源的資料會漏給頁面；只能靠 `net` 的測試、WPT 的 `cors/`、mutation test 和審查守住（`docs/test-gaps.md` 的「只靠審查」）。
- **做的期間比 Chrome 嚴格**：第 1、2 段的跨源 `fetch()` 是網路錯誤（第 3 條），3a 之後才照 CORS；`main` 上這段期間用 CDN 上的 API 的頁面壞掉。

## 理由和其他做法

- **CORS 放在 Browser 的 `net`，不放在 renderer**：Chromium 2019 年把 CORS 從 Blink 搬到 network service（OOR-CORS），因為 renderer 被攻破時可以跳過自己的檢查；kun 照 multi-process 的方向寫，同樣的理由成立。RFC 0015 的 G1 也是這樣定的。
- **不做 `ReadableStream`**：Streams Standard 本身是一大塊（reader、queue、backpressure、pipe），而 `net` 還是整份收完；Speedometer 和 `url/` 的 WPT 都只要 `text()` / `json()`。
- **bytes 的形式留給 H2**：`ArrayBuffer`、`BufferSource`、`Blob` 的形式 RFC 0017 已經整理好筆記、歸 H2；在這裡做會讓兩份 RFC 同時設計同一種形式。
- **Promise 用宣告表的形式，不手寫**：手寫（`scroll` 的做法）只有 JS、每個 API 各寫一次；`Blob` 的讀取、`HTMLImageElement.decode()`、之後的 `navigator.clipboard` 都要同樣的形式。
- 完整的比較在 `details.md` 第 3 節。

## 先例

- **Chromium**：`fetch()` 在 `core/fetch/global_fetch.cc`，交給 `FetchManager`，它的 `Loader` 經過 `ThreadableLoader` 和 `ResourceFetcher` 送給 network service 的 `URLLoaderFactory`；CORS 在 network service 的 `CorsURLLoaderFactory` / `CorsURLLoader`。`Request`、`Response`、`Headers` 拿著 `FetchRequestData`、`FetchResponseData`、`FetchHeaderList`；body 是 `BodyStreamBuffer`（包著 `BytesConsumer`，用 mojo 的 data pipe 串流）。promise 由 `ScriptPromiseResolver` settle。`file:` 的 `fetch` 被拒絕。kun 照這個分層，差別是 body 整份收完、沒有 Streams（見「缺點」）。
- **RFC 0015 的動態 `import()`**：每個 realm 一張 resolver 的表、回應的 task 裡 settle，照它（表改成兩邊共用）；送出的時機不照它，`fetch()` 當場送（第 12 條）。
- **RFC 0019 的 `URLSearchParams`**：`[PairIterable]`，`Headers` 照用。

## 已決定的事項

2026-10-10 確認第 1 到 6 條（比較在 `details.md` 第 3 節）、第 1 段的第 7 到 14 條，和第 15 條。

1. **R1：範圍是 `fetch`、`Request`、`Response`、`Headers`、任何 method、字串和 `URLSearchParams` 的 body、`AbortSignal`、CORS（連同 module script）**（2026-10-10）：bytes 的形式（`arrayBuffer()`、`blob()`、`BufferSource` / `Blob` / `FormData` 的 body）留給 H2，沒有 Streams 和 XHR。`net` 本來就要長出 method 和 header，`url/` 和 `fetch/api/` 的 WPT 都要 `Request`、`Headers`；CORS 是 RFC 0015 的 G1 留下來的，`fetch` 讀得到資料，比 module 更需要它。不選 R2（只做 Speedometer 要的 `GET`、`text()` / `json()`、同源）：`Request`、method、CORS 之後一定還要再開一份；不選 R3（R1 加上 `ArrayBuffer` / `BufferSource`）：和 H2 的 `Blob` 同時設計同一種形式。
2. **P1：宣告表的 `Promise<T>`，`dom_api` 的引擎中立 resolver，JS 和 WIT 都有**（2026-10-10）：JS 回傳 promise，WIT 是 `async func -> result<T, dom-exception>`，guest 等它時 Store 讓出、settle 時由 engine 的 task 叫醒（`dom_wasm` 等 WASI 時鐘的路）。之後 `Blob` 的讀取、`HTMLImageElement.decode()` 照用。不選 P2（照 `scroll` 在 `dom_js` 的 glue 手寫，`dom_api` 用 callback）：每個 API 各寫一次，只有 JS；不選 P3（P1 但先只給 JS）：要在 `docs/limitations.md` 記一條、之後再回頭做 WIT 和 guest 的測試，而 WIT 這一側的成本不高。
3. **C1：第 1、2 段跨源是網路錯誤，第 3 段在 Browser 的 `net` 做 CORS**（2026-10-10）：照 Chromium 的 `CorsURLLoader`（2019 年從 Blink 搬到 network service，因為 renderer 被攻破時可以跳過自己的檢查）。不選 C2（CORS 做在 renderer）：同樣的理由，之後拆 process 時也要搬；不選 C3（不檢查，和現在的 module 一樣）：任何頁面讀得到跨源的資料，包括只有使用者的網路連得到的內網。
4. **B1：body 整份收完，沒有 `ReadableStream`，不宣告 `body`**（2026-10-10）：`net` 還是整份收完；Speedometer 和 `url/` 的 WPT 都只要 `text()` / `json()`。不選 B2（這份就做 Streams）：Streams Standard 比這份的其他部分加起來還大，和 `net` 的串流一起做。
5. **F1：`fetch()` 拒絕 `file:` 的網址，和 Chrome 一樣**（2026-10-10）：Fetch Standard 把 `file:` 留給實作；照 Chrome，在 kun 上能跑的頁面在 Chrome 上也能跑，kun 自己的測試都經過 `net::test_server` 或 wptserve。不選 F2（照現在的子資源，`file:` 的頁面可以讀 `file:`）：在 kun 上能跑、在 Chrome 上不能跑；`file:` 的 origin 是 opaque，CORS 的語意也要另外定。
6. **分三段**（2026-10-10）：Promise 的形式和讀取、`Request` 和送出、CORS（「分階段」）。第 1 段做完時 Perf-Dashboard 和 `url/` 的 WPT 要的 `fetch` 都有了；Perf-Dashboard 最先停在 `alert`，也要 Canvas 2D 等，M4 不只等這份。

7. **T1：`text()` / `json()` 排一個 task 再 settle**（2026-10-10，第 1 段開始前）：照規範的「fully read body」，Chrome 經過 data pipe，也在之後的 task。不選 T2（body 已經整份在記憶體裡，當下就 resolve）：`then` 會排在已經在佇列裡的 task 之前，頁面看得到和 Chrome 不同的順序（`details.md` 第 5 節）。
8. **Q1：`bindings` 每個 realm 一張 resolver 的表，動態 `import()` 的 `Linkage.imports` 也改用它**（2026-10-10，第 1 段開始前）：以 id 留著、之後 settle、realm 消失時丟掉，只有一份實作；改動只在 `bindings` 和 `dom_js` 的動態 `import()`，有 RFC 0015 的測試守住。不選 Q2（新的表和 `Linkage.imports` 並存）：同一件事兩份實作。
9. **Y1：`ByteString` 是宣告表新的 `Base`**（2026-10-10，第 1 段開始前）：binding 照 WebIDL 轉換，超過 U+00FF 時 `TypeError`；WIT 是 `string`，host 這一側檢查。規範的 WebIDL 比對（`crates/dom_api/tests/spec.rs`）看得到型別，錯誤也在 WebIDL 的轉換時丟，和 Chrome 的順序一樣。不選 Y2（宣告成 `DomString`、在 API 的函式裡檢查）：每個函式各檢查一次，型別和規範對不上。
10. **J1：`ScriptHost` 多一個 `parse_json`，`json()` 是 `Promise<any>`、只給 JS**（2026-10-10，第 1 段開始前）：V8 的 `JSON::Parse`，不受頁面改掉的 `JSON.parse` 影響（規範的「parse JSON from bytes」）。guest 用 `text()` 自己解析（`any` 只給 JS，RFC 0008 的第 8 條）。不選 J2（`dom_js` 的 glue 寫成 `text()` 加 `JSON.parse`）：用的是頁面看得到、改得掉的 `JSON.parse`。
11. **D1：`data:` 的網址照現在的子資源，經過 Browser 的 `net`**（2026-10-10，第 1 段開始前）：一條路，`net` 已經會解。不選 D2（renderer 自己解，Blink 的 `ResourceLoader::HandleDataUrl`）：多一份解析和回應的建法，省下的一次訊息來回對 `fetch` 不重要。
12. **K2：`fetch()` 呼叫的當下送出請求**（2026-10-10，第 1 段開始前）：和 Blink 一樣（`FetchManager::Loader::Start` 經過 `ThreadableLoader` 當場開始），同一個 task 裡接著跑很久的 script 時，請求已經在路上。`FrameClient` 的實作（`FrameView`）拿著送往 Browser 的 channel 和 render thread 共用的請求 id 的計數，登記和送出在同一個呼叫裡做。不選 K1（照動態 `import()`，記在 `FrameRequests`、render thread 在 task 結束時送）：路比較一致，但請求晚出去，量 Speedometer 時和 Chrome 的時間線不同。

13. **E1：每份文件的 DOM task runner，現在做**（2026-10-10，第 1 段開始後）：`dom` 有一份 DOM 自己的 task 清單（Blink 的 `ExecutionContext::GetTaskRunner`），每個 task 記著它的 document、task source（networking、timer）和到期的時間，內容是 `dom_api` 的（`Box<dyn Any>`，和 microtask 一樣）；render thread 每輪取出到期的，在那份 document 的 realm 裡執行，接著 microtask checkpoint。`text()` / `json()` 的 settle（第 7 條）用 networking，`AbortSignal.timeout()` 從 timer 的清單搬過來（timer）。RFC 0010 的「之後的可能」把這件事留給「第二個要延遲執行 DOM 自己的工作的功能」，`fetch` 就是；第 2 段的取消、之後的 `postMessage` 也會用它。不選 E2（照 `AbortSignal.timeout()` 的先例，在 timer 的清單加一種延遲 0 的 task）：networking 的 task 混在 timer 裡，留下的題目繼續拖；不選 E3（經過 `FrameClient` 交給 renderer 排）：沒有 frame 的 document 沒辦法用。
14. **`Response` 的建構子和 `text()` 提前到 1a**（2026-10-10，第 1 段開始後）：1a（宣告表的 `Promise<T>`）本身沒有真的 API 可以測，用它們從 JS 和 wasm 兩邊測整條路（建立、之後的 task 裡 settle、realm 消失時丟掉）。

15. **1a 和 1b 先 merge 回 `main`，不等第 1 段做完**（2026-10-10，使用者的要求）：`rfcs/README.md` 的「流程」第 5 步只讓做完的階段 merge，這是例外。merge 之前照「一個階段做完」跑了 `cargo fmt --check` 和 clippy；`main` 上有 `Promise<T>` 的形式、DOM 的 task、`Headers` 和 `Response` 的一部分，還沒有 `fetch()`（`docs/limitations.md` 的「Bindings 和 DOM API」寫著有什麼、沒有什麼）。1c、1d 做完時第 1 段照常 merge。

2026-10-10 確認第 2 段的第 16 到 20 條。

16. **第 2 段分成三段**（2026-10-10，第 2 段開始前）：2a `Request`（`dom` 的 GC 型別、建構子、getter、`clone()`、和 `Response` 共用的 body mixin、`request-no-cors` guard、`fetch(input: RequestInfo)`，網路上仍然只送 `GET`）；2b `net` 送出（任何 method 和 body、Fetch 的 redirect 規則和 `redirect` 的三種模式，拿掉只送 `GET` 的限制）；2c `AbortSignal` 和取消（第 17 到 19 條）。每段可以單獨測。`fetch()` 的參數改成 `RequestInfo` 是 `kun:dom` 不相容的改動，照 `crates/dom_api/README.md` 的規則升 minor。
17. **A2：abort algorithm 帶著 `dom_api` 的資料**（2026-10-10，第 2 段開始前）：`dom` 的 abort algorithm 除了移除 listener，多一種帶著 `dom_api` 自己的資料（`Box<dyn Any>`，不含 GC 的參照，和第 13 條的 DOM task 一樣），abort 時交還給 `dom_api` 執行；`fetch()` 的拒絕 promise、通知 frame 取消請求，Blink 的 `AbortSignal::AddAlgorithm`（`FetchManager::Loader` 有自己的 `Algorithm` 子類別）。取消經過 `RendererHostMsg::CancelResource`，Browser 的 IO thread 交給 `net`，`net` 丟掉那個請求的 task（Chromium 關掉 `URLLoader`）。不選 A1（`dom` 多一種 fetch 專用的 algorithm）：`dom` 要知道 fetch 的事，之後 streams 的 `pipeTo` 又要再加一種；不選把 closure 存在 `dom` 裡：closure 碰不到 GC 的 root，拿不住 handle。
18. **V1：response 回來之後才 abort，之後讀 body 以 abort 的 reason 拒絕**（2026-10-10，第 2 段開始前）：和 Chrome 一樣（body 還在 data pipe 裡，`BodyStreamBuffer` 被弄成錯誤），WPT 的 `abort/general.any.js` 測它。不選 V2（body 已經整份在記憶體裡，response 回來之後 abort 沒有作用）：頁面看得到和 Chrome 不同的結果。
19. **G1：document 丟棄時取消它還沒回來的 `fetch()`**（2026-10-10，第 2 段開始前）：Chrome 的 `FetchManager` 在 `ExecutionContext` 銷毀時（`ContextDestroyed`）取消它的 loader；第 17 條的取消做好之後幾乎不用多寫。不選 G2（照第 1 段，請求跑完、回應來了再丟掉）：導覽離開之後還佔著網路。
20. **H1：kun 不照做的 `RequestInit` 成員也宣告、檢查、存起來**（2026-10-10，第 2 段開始前）：`referrer`、`referrerPolicy`、`cache`、`integrity`、`keepalive`、`priority` 照規範檢查（例如 `only-if-cached` 不是 `same-origin`、`no-cors` 有 `integrity`、不合法的 `referrer` 是 `TypeError`），`Request` 的 getter 讀得回來，行為不變（沒有 `Referer`、HTTP cache、SRI），記進 `docs/limitations.md`。interface 和 Chrome 一樣，WPT 的 `request/` 大多測這些 getter 和檢查。不選 H2（只宣告有作用的成員）：其他的被 WebIDL 忽略，getter 是 `undefined`，和規範、Chrome 都不同。

2026-10-10 確認第 3 段的第 21 到 25 條。

21. **第 3 段分成三段**（2026-10-10，第 3 段開始前）：3a `fetch()` 的 CORS（跨源請求的 `Origin`、`Access-Control-Allow-Origin` 和 `-Credentials` 的檢查、response tainting 和過濾的 header、`no-cors` 的 opaque response、redirect 之後 opaque 的 origin，拿掉 renderer 和 `net` 現在跨源一律網路錯誤的限制）；3b preflight（第 23 條）；3c 子資源（第 24、25 條）。kun 沒有 cookie，`credentials: "include"` 的 CORS 檢查照樣做。
22. **W1：response 在 `net` 過濾**（2026-10-10，第 3 段開始前）：renderer 收到的 `cors` response 只有可以曝露的 header，`fetch()` 的 `opaque` response 沒有 header 和 body；圖片、classic script 這類 `no-cors` 的子資源照樣有 body，標成 opaque。renderer 不可信（之後拆成 process），收不到的漏不出去；kun 沒有 Cache API 和 service worker，`fetch()` 的 opaque body 沒有用處。不選 W2（`net` 全部送出、renderer 過濾，Blink 的 CORS 過濾）：Chromium 另外有 network service 的 ORB 擋掉大部分 opaque 的 body，kun 沒有，等於把跨源的資料交給 renderer。
23. **P1：preflight 先不快取**（2026-10-10，第 3 段開始前）：不是 simple 的跨源請求每次都先送 preflight，照「詳細設計」原本的寫法，preflight cache 留在「之後的可能」；WPT 的 `cors-preflight-cache*` 因此失敗。不選 P2（照規範的 CORS-preflight cache，放在 `NetworkContext`，照 `Access-Control-Max-Age`，Chrome 預設 5 秒、上限 2 小時）：多一份要測試和維護的狀態，現在沒有頁面需要它。
24. **S1：module script、`modulepreload`、帶 `crossorigin` 的 classic script 照 CORS**（2026-10-10，第 3 段開始前）：`cors` mode，`crossorigin` 決定 credentials mode；沒有 `crossorigin` 的 classic script 照舊以 `no-cors` 載入。第 1 條定的範圍。不選 S2（再加上 `no-cors` 跨源的 classic script 的錯誤 mute，`window.onerror` 只看到「Script error.」）：另一個安全題目，放到「之後的可能」。
25. **M1：`file:` 頁面的 module script 照 Chrome 擋下**（2026-10-10，第 3 段開始前）：`file:` 的 origin 是 opaque，CORS 不適用於 `file:`；和第 5 條同一個理由，在 kun 上能跑的頁面在 Chrome 上也能跑。`about:demo` 的 module 是 `data:` 網址，照樣能讀；kun 自己的 module 測試走 `net::test_server` 和 wptserve。不選 M2（照現在，`file:` 頁面可以載入 `file:` 的 module）：和 Chrome 不同。

26. **3a、3b 先 merge 回 `main`，不等第 3 段做完**（2026-10-10，使用者的要求）：和第 15 條一樣是 `rfcs/README.md` 的「流程」第 5 步的例外。merge 之前照「一個階段做完」跑了 `cargo fmt --check` 和 clippy，修在 3b 的 commit 裡；`main` 上的 `fetch()` 照 CORS、有 preflight，子資源（module script、`crossorigin`）還沒有，`docs/limitations.md` 的「Script」照舊寫著 module 的抓取不檢查 CORS。3c 做完時第 3 段照常 merge。

## 待決定

沒有。

## 之後的可能

- `ReadableStream` 和串流的 body，連同 `net` 的串流（RFC 0004 的 data pipe）和 incremental parsing。
- bytes 的形式和 `Blob`、`FormData` 的 body（H2）。
- `XMLHttpRequest`（照 Blink，建在同一條路上）。
- cookie 和 `credentials` 的效果、HTTP cache 和 `cache` 選項、CORS 的 preflight cache。
- `AbortSignal.any()` 改成照 Blink 的 GC（RFC 0010 的「之後的可能」；同一項裡的每份文件的延遲 task runner 這份做了，第 13 條）。
- `no-cors` 跨源的 classic script 的錯誤 mute（第 24 條的 S2）。
- 動態 `import()` 照 import 它的 script 的 fetch options 決定 credentials mode（現在一律 `same-origin`）：engine 要記住每個 script 的 fetch options（V8 的 host-defined options）。
- `keepalive`（`sendBeacon`）、CSP、SRI、referrer policy。
