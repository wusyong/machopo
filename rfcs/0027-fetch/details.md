# RFC 0027 的細節：`fetch`

## 1. 現況的盤點

2026-10-10，`main` 的 `6fcfd03`。

**`net`**（`crates/net`）：
- `NetworkContext::start` 開一個 `NetworkService` thread，裡面是 current-thread 的 tokio；`NetworkContext::fetch(ResourceRequest, done)` 不阻塞，`done` 在 `NetworkService` thread 上被呼叫。
- scheme：`http`、`https`（reqwest，只在 `src/http.rs` 裡）、`file:`（`spawn_blocking` 讀檔，MIME 看副檔名）、`data:`（`data-url` crate）；其他是 `UnknownUrlScheme`。
- `HttpTransport::get(url, accept)` 只送 `Accept` 和 User-Agent，沒有 request header 和 body。`Accept` 依 `RequestDestination` 選。
- redirect 由 `URLRequest` 自己跟（reqwest 的關掉），最多 20 次，301 / 302 / 303 / 307 / 308；轉到 `http(s)` 以外是 `UnsafeRedirect`；fragment 照 Fetch 保留。
- body 整份收完，沒有 `Content-Type` 時 sniff markup 和純文字；連線 30 秒、讀取 60 秒逾時。錯誤對應到 Chromium 的 `NetErrorCode`。
- 沒有 cookie、HTTP cache、CORS、`Origin`、referrer、credentials。
- `net::test_server`：只用標準函式庫的 HTTP/1.1 server（照 Chromium 的 `EmbeddedTestServer`），也有 HTTPS；`Route` 設 status、header、body、延遲；`requests()` 讀回收到的請求和 header。

**`protocol`**（`crates/protocol/src/network.rs`）：`ResourceRequest { url, destination }`（「一律是 GET」）、`RequestDestination { Document, Iframe, Style, Image, Script }`、`ResourceResponse { url, status, mime_type, charset, headers, body }`（header 是小寫、照順序的 `Vec<(String, String)>`，body 是整份的 `Vec<u8>`）、`LoadResult = Result<ResourceResponse, NetError>`。

**子資源的路**：renderer 的 main thread 送 `RendererHostMsg::FetchResource { request_id, url, destination }`（`render_thread.rs` 的 `fetch`），Browser 的 `Chrome_IOThread`（`render_process_host.rs`）攔下來、不交給 UI thread，`fetch_for_renderer` 只檢查一件事（`file:` 的子資源只在目前的 document 是 `file:` 時准許，照 `ChildProcessSecurityPolicy`），交給 `net`；回應是 `RendererMsg::ResourceResponse { request_id, result }`，包成 `MainThreadTask::Browser` 放進 main thread 唯一的佇列。`FrameView::did_receive_resource` 依 id 找 `pending` 裡的 `PendingLoad`（`StyleSheet`、`Image`、`Script`、`Module`）。各種回應的檢查在 renderer（classic script 要 2xx、不能是圖片等；module 要 JavaScript 的 MIME；wasm 要 `application/wasm`；stylesheet 不是 quirks 時要 `text/css`）。

**script 發起、之後才完成的請求**：動態 `import()` 是現在唯一的例子。`dom_js` 的 `import_dynamically` 經過 `bindings::begin_dynamic_import` 建 `v8::PromiseResolver`，以編號存在每個 context 的 `Linkage.imports`；請求經過 `FrameClient::import_module` 記在 `FrameRequests`，render thread 在迴圈開頭（`start_dynamic_imports`）送出；回應之後 `finish_dynamic_imports` 呼叫 `ScriptEngine::finish_dynamic_import`，`bindings` 依編號 settle，再 microtask checkpoint。realm 消失時 resolver 跟著丟掉。這條路整段是 V8 專屬的（resolver 在 `bindings`）。

**宣告表**（`crates/dom_api/src/value.rs`）：沒有 `Promise<T>`、`ByteString`、`ArrayBuffer`、`BufferSource`、`long long`。宣告表的 `async method` 不是 promise：它是「會同步呼叫回 script」的成員（`dispatchEvent` 等），WIT 是當場完成的 `async func`。`[PairIterable]`（RFC 0019 的第 13 條）、字典、有字典以外成員的 union、`Record<V>` 都有。`scroll` 系列在宣告表裡沒有回傳值，`dom_js` 的 glue 包成 `Promise.resolve()`。

**`AbortSignal`**（`dom_api/src/abort.rs`、`dom/src/abort.rs`）：有 `abort()`、`timeout()`、`any()`、`reason`、`throwIfAborted`；abort algorithm 只有一種（`AbortAlgorithm { target, listener }`，移除 listener）。

**`dom_wasm` 的等待**：guest 的 `start` 是 `async func`；WASI 的 `monotonic-clock` 的等待是 engine 佇列裡的延遲 task，到了叫醒 Store，Store 再 poll `run_concurrent`（`crates/dom_wasm/README.md`）。WIT 裡還沒有 `future<T>` 和 `stream<T>`。

## 2. Chromium 的做法

| 部分 | Chromium | 這份 |
| --- | --- | --- |
| `fetch()` | `core/fetch/global_fetch.cc`（`WindowOrWorkerGlobalScope` 的 partial），交給 `FetchManager::Fetch` | `dom_api` 的 `fetch`，交給 `FrameClient` |
| 送出 | `FetchManager::Loader` → `ThreadableLoader` → `ResourceFetcher`（`RawResource`）→ `URLLoaderFactory`（mojo） | render thread → `RendererHostMsg::FetchResource` → Browser 的 IO thread → `net` |
| 請求的資料 | `network::ResourceRequest`（method、header、body、mode、credentials mode、redirect mode、`request_initiator`、destination），所有的子資源共用 | `protocol::ResourceRequest` 長出同樣的欄位，所有的子資源共用 |
| CORS | network service 的 `CorsURLLoaderFactory` / `CorsURLLoader`（2019 年從 Blink 搬過去，OOR-CORS）；preflight 有 cache | `net` 的 CORS loader；preflight 先不快取 |
| `Request` / `Response` / `Headers` | `core/fetch/request.h`、`response.h`、`headers.h`，資料在 `FetchRequestData`、`FetchResponseData`、`FetchHeaderList` | `dom` 的 GC 型別，資料是 `NoTrace` |
| body | `Body` mixin（`core/fetch/body.h`），`BodyStreamBuffer` 包著 `BytesConsumer`，mojo 的 data pipe 串流，`text()` 等經過 `FetchDataLoader` | 整份的 bytes，`text()` 當場解碼（但照規範在之後 settle，見第 5 節） |
| promise | `ScriptPromiseResolver<T>`（`bindings/core/v8/script_promise_resolver.h`），屬於一個 `ExecutionContext`，context 消失時不再 settle | `dom_api` 的 `PromiseResolver`，屬於一個 realm |
| abort | `FetchManager::Loader::Abort`：拒絕 promise、取消 loader，network service 關掉 `URLLoader` | 新的 abort algorithm，送取消的訊息 |
| `file:` | `fetch` 拒絕（「URL scheme "file" is not supported」） | 一樣 |

## 3. 選項的比較

### R：範圍

| | 內容 | 好處 | 壞處 |
| --- | --- | --- | --- |
| R1 | `fetch`、`Request`、`Response`、`Headers`、任何 method、字串和 `URLSearchParams` 的 body、`AbortSignal`、CORS（連同 module）；沒有 bytes 的形式、Streams、XHR | 一次把 `net` 的 request 長完整；CORS 解掉 RFC 0015 留下的限制；`fetch/api/` 的 WPT 大部分能跑 | 三段，比 Speedometer 要的多 |
| R2 | `GET`、`text()` / `json()`、同源 | 最快讓 Perf-Dashboard 的 `fetch` 能用 | `Request`、`Headers`、method、CORS 之後還要再開一份；WPT 的 `fetch/api/` 幾乎都要 `Request` 或 `Headers` |
| R3 | R1 加 `ArrayBuffer` / `BufferSource` | `arrayBuffer()` 很常用 | 和 H2 的 `Blob` 同時設計同一種形式（RFC 0017 的筆記把它們歸 H2） |

### P：Promise 的形式

| | 宣告表 | JS | WIT | 壞處 |
| --- | --- | --- | --- | --- |
| P1 | `-> Promise<T>`，函式拿到 `PromiseResolver` | binding 建 `v8::PromiseResolver`、放進 realm 的表，回傳 promise；函式同步丟的例外變成被拒絕的 promise | `async func -> result<T, dom-exception>`，host 的 future 等 resolver，settle 時排 engine 的 task 叫醒 Store | 三層各一份實作 |
| P2 | 不變；`dom_api` 的函式收一個完成時的 callback | `dom_js` 的 glue 手寫 promise（`scroll` 的做法） | 沒有 | 每個 API 各寫一次，只有 JS；違反 RFC 0008 的規則，要在這裡寫例外 |
| P3 | 同 P1 | 同 P1 | 先不給，`docs/limitations.md` 記一條 | 之後要回頭做 WIT 和 guest 的測試 |

`PromiseResolver` 的細節：
- `dom_api` 的型別，只有 id 和 realm；`ScriptHost` 多 `create_promise`、`resolve`、`reject`。settle 一次之後再 settle 不做事（Blink 的 `ScriptPromiseResolver` 也一樣）。
- `resolve` 的值是宣告表的 `Value`（`Response` 是一個 handle）；`reject` 收 `DomException` 或 `TypeError`（`fetch` 的網路錯誤是 `TypeError`）。
- GC：resolver 本身在 `bindings` 的 realm 表裡，是 V8 的 `Global`，不是 `dom` 的 GC 物件；`FrameClient` 的登記只拿 id。realm 消失時表跟著丟掉，晚到的 settle 找不到 id、不做事。
- 動態 `import()` 的 `Linkage.imports` 改成用同一張表（`README.md` 的第 8 條）。

### C：跨源

| | 做法 | 壞處 |
| --- | --- | --- |
| C1 | 第 1、2 段跨源是網路錯誤；第 3 段在 Browser 的 `net` 做 CORS（照 `CorsURLLoader`） | 第 3 段之前比 Chrome 嚴格 |
| C2 | CORS 做在 renderer（Blink 在 OOR-CORS 之前的做法） | renderer 被攻破時可以跳過；之後拆 process 要搬 |
| C3 | 不檢查 | 任何頁面讀得到跨源的資料，包括只有使用者的網路連得到的內網 |

kun 沒有 cookie，所以 CORS 現在擋的主要是「只有使用者的網路連得到」的資源（內網、localhost 上的服務），之後有 cookie 時才擋帶著身分的請求。

### B：body

| | 做法 | 壞處 |
| --- | --- | --- |
| B1 | 整份收完，不宣告 `body` | `response.body` 是 `undefined`；用 `getReader()` 的 WPT 失敗 |
| B2 | 這份就做 Streams Standard 的 `ReadableStream`（先用整份的 bytes 當唯一的一塊） | Streams 本身比這份的其他部分加起來還大，Speedometer 和 `url/` 的 WPT 都用不到 |

### F：`file:` 的網址

| | 做法 | 壞處 |
| --- | --- | --- |
| F1 | 照 Chrome，`fetch()` 拒絕 `file:` | 直接開本機的 HTML 檔、用 `fetch` 讀旁邊的 JSON 不行（Chrome 也不行） |
| F2 | 照現在的子資源：`file:` 的頁面可以讀 `file:` | 在 kun 上能跑、在 Chrome 上不能跑的頁面；`file:` 的 origin 是 opaque，CORS 的語意也要另外定 |

## 4. WPT 的盤點

`../third_party/wpt/fetch/api/`，2026-10-10 數的測試檔（`.any.js`、`.html`、`.window.js`），和用到這份不做的東西的檔數（粗略的 grep，一個檔可能只有幾個 subtest 用到）：

| 目錄 | 檔數 | 用到 Streams（`getReader`、`ReadableStream`、`.body`） | 用到 bytes（`arrayBuffer`、`Blob`、`FormData`、typed array） | 用到 XHR |
| --- | --- | --- | --- | --- |
| `basic` | 37 | 9 | 6 | 0 |
| `headers` | 12 | 0 | 0 | 2 |
| `request` | 28 | 6 | 7 | 0 |
| `response` | 32 | 24 | 22 | 0 |
| `body` | 3 | 1 | 3 | 0 |
| `abort` | 6 | 3 | 3 | 0 |
| `redirect` | 15 | 2 | 1 | 0 |
| `cors` | 26 | 2 | 0 | 0 |

- `headers` 幾乎都能跑（第 1 段）；`basic`、`redirect`、`request` 大部分在第 2 段之後；`cors` 在第 3 段；`response` 大多要 Streams 或 bytes，只有少數能跑。每段開始時試跑，結果記在 `progress.md`。
- `url/` 裡用 `fetch` 讀資料的：`url-constructor.any.js`、`url-origin.any.js`、`url-setters.any.js`、`url-setters-a-area.window.js`、`toascii.window.js`、`percent-encoding.window.js`、`IdnaTestV2.any.js`、`IdnaTestV2-removed.any.js`、`failure.html`（RFC 0019 的第 4 條留下的）。它們只要同源的 `GET` 和 `json()`，第 1 段就能加。

## 5. 宣告表的新形式

| 形式 | 誰用 | JS（`dom_js`） | WIT | 第幾段 |
| --- | --- | --- | --- | --- |
| `Promise<T>` 的回傳 | `fetch`、`text()`、`json()` | 回傳 promise；同步的例外變成被拒絕的 promise | `async func -> result<T, dom-exception>`；`T` 是 `any` 的只給 JS | 1 |
| `ByteString` | `Headers` 的名稱和值、`Response.statusText`、`Request.method` | 超過 U+00FF 時 `TypeError` | `string` | 1 |
| union 的成員是 interface 和字串 | `RequestInfo`（`Request or USVString`）、`HeadersInit`（`sequence<sequence<ByteString>> or record<ByteString, ByteString>`） | 照 WebIDL 的 union 轉換 | `variant` | 1（`HeadersInit`）、2（`RequestInfo`） |
| `BodyInit` | `Response` 的建構子、`RequestInit.body` | 這份只有 `USVString` 和 `URLSearchParams`，bytes 的成員在 H2 | `variant` | 1 |

`text()` 和 `json()` 要不要在呼叫的當下 resolve：body 已經整份在記憶體裡，可以當場 resolve；但規範的「fully read body」是非同步的，Chrome 經過 data pipe，要等之後的 task。頁面看得到差別：當場 resolve 時 `then` 在這次的 microtask checkpoint 就跑，排在同一個 task 裡之後才排的 `setTimeout` 之前、也排在其他已經在佇列裡的 task 之前。2026-10-10 確認照規範排一個 task 再 settle（`README.md` 的第 7 條）。
