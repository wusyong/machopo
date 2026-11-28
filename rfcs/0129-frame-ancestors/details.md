# RFC 0129 的細節：不讓別人嵌：`X-Frame-Options` 和 CSP 的 `frame-ancestors`

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-28 讀的（`main`，`25276386`）。

- **沒有任何 CSP、`X-Frame-Options` 的解析或檢查**。唯一提到 `x-frame-options` 的是 `net` 的 HTTP cache 不讓 `304` 更新的 header 清單。WPT 的 `html/browsers/browsing-the-web/navigating-across-documents/failure-check-sequence.https.html` 的 4 個 subtest（CSP 在 `X-Frame-Options` 之前、COEP 的順序）預期是 `FAIL`。`tests/wpt/include.txt` 沒有 `x-frame-options/`、`content-security-policy/`。
- **導覽的路**（`browser` 的 `web_contents.rs`）：子 frame 的 `BeginNavigation` → `begin_renderer_navigation`（`page_may_navigate_to`）→ `start_navigation`（`about:blank`、`about:srcdoc` 當場 commit，其他的 `Iframe` 的請求，`net` 跟 redirect 時等 Browser 說繼續）→ `on_navigation_response_started` 把 head 和 body 記在節點的 `NavigationRequest` 上 → `read_body_start` 讀開頭 1024 bytes → `commit_response`。
  - 回應的 header 在 `head.headers`（名字是小寫的），從拿到 head 到 `commit_response` 拿走那個導覽之前都在；`commit_response` 已經用它讀 `content-language`、`content-encoding`。main frame 和子 frame 走同一條。
  - redirect：`on_navigation_redirected` 只記每一跳的網址和 timing，跟著走；每一跳的整個 head 在 `RedirectInfo.response` 裡。
  - **沒有 navigation throttle 那種掛點**。錯誤頁只在沒拿到回應時 commit（`on_navigation_completed`：`NetError` → `loader::error_page` → `CommittedDocument::error_page`，origin 是 opaque 的，網址記著失敗的那個；renderer 用 `chrome-error://chromewebdata/` 建文件）。有 head 的一律 commit（`a_body_that_fails_at_once_commits_no_error_page`）。被拒的 `about:srcdoc` 也是 commit 錯誤頁（`InvalidUrl`）。
- **frame 樹**（`frame_tree.rs`）：`parent`、`origin`（renderer 回報 commit 時更新）、`committed_origin`；往上走祖先的寫法已經有（`storage_key_in`、`loader_params` 的 `std::iter::successors`）。父 frame 自己在導覽時，它的 `origin` 還是現在顯示的那份文件的。
- **`about:blank`、`about:srcdoc`、`javascript:`** 不載入，沒有 response 的 header；`data:`、`blob:` 有 `net` 造的 head，沒有伺服器的 header。
- **嵌入的人看得到的**：DevTools 的 Network 只在沒拿到回應的失敗送 `loadingFailed`（`shell` 的 `automation_session.rs`），拿到 head 之後才失敗的現在什麼都不送；console 只有 status 400 以上的網路訊息。錯誤頁的子 frame 照一般的文件跑 `load`，父 frame 的 `load` 等它（`a_child_frames_failed_navigation_commits_an_error_page_of_no_origin`）。
- **沒有 iframe 的 `sandbox`**，也沒有 `<meta http-equiv="Content-Security-Policy">`。

## Chromium

2026-11-28 讀的，`chromium.googlesource.com` 的 `main`。

### `content/browser/renderer_host/ancestor_throttle.cc`（讀完）

- `WillRedirectRequest`：算一次、不寫 console，一律 `PROCEED`（TODO 是 crbug 835465）。`WillProcessResponse`：寫 console、會擋。
- `ProcessResponseImpl` 的順序：
  1. `IsInOutermostMainFrame()`：`PROCEED`（「Allow main frame navigations.」）。
  2. 檢查回應、而且 `!response_should_be_rendered()`：`PROCEED`（「204/205 responses and downloads are not sent to the renderer」）。
  3. 檢查回應、而且 `EvaluateFrameAncestors` 是 `BLOCK`：`BLOCK_RESPONSE`。
  4. `EvaluateXFrameOptions`：`BLOCK` 就 `BLOCK_RESPONSE`。
  5. `EvaluateEmbeddingOptIn`：`BLOCK` 就 `BLOCK_RESPONSE`（`kEmbeddingRequiresOptIn` 關著時一律 `PROCEED`）。
- 祖先：從父 frame（`GetParentForFrameAncestors`，到 fenced frame 的邊界為止）往上每一層，`frame-ancestors` 拿每一層的 `GetLastCommittedOrigin().GetURL()` 去比；`SAMEORIGIN` 要每一層 `IsSameOriginWith(url::Origin::Create(url))`；`DENY` 不用走。
- `EvaluateXFrameOptions` 一開始：`disposition != kNone && disposition != kAllowAll && HeadersContainFrameAncestorsCSP(...)` 就 `PROCEED`：強制的 `frame-ancestors` 蓋過 `DENY`、`SAMEORIGIN`、衝突、無效；`HeadersContainFrameAncestorsCSP` 只看 `kEnforce` 的。
- `kConflict`：擋；`kInvalid`：寫 console、`PROCEED`（TODO 是 crbug 1153274）；`kDeny`：擋；`kSameOrigin`：照祖先；`kNone`、`kAllowAll`：`PROCEED`。
- console 的訊息（`kError`，`NavigationRequest::AddDeferredConsoleMessage`）：
  - 「Refused to display '%s' in a frame because it set 'X-Frame-Options' to '%s'.」
  - 「Refused to display '%s' in a frame because it set multiple 'X-Frame-Options' headers with conflicting values ('%s'). Falling back to 'deny'.」
  - 「Invalid 'X-Frame-Options' header encountered when loading '%s': '%s' is not a recognized directive. The header will be ignored.」
- 單元測試的名字：`ParsingXFrameOptions`、`ErrorsParsingXFrameOptions`、`IgnoreWhenFrameAncestorsPresent`。

### `services/network/public/cpp/x_frame_options_parser.cc`（讀完）

- 每一個 `x-frame-options` header（`EnumerateHeader`），值 `TrimWhitespaceASCII`，不分大小寫：`deny`、`allowall`、`sameorigin`，其他是 `kInvalid`；不照 `,` 切，所以一個 header 裡的 `deny, sameorigin` 是 `kInvalid`。
- 合併：第一個就是它；之後不同的就 `kConflict`，之後一直是 `kConflict`。所以 `deny` 加無效的是衝突（擋），兩個不同的無效值還是 `kInvalid`（放行）。
- 註解：不支援 RFC 7034 的 `ALLOW-FROM`。

### CSP（`content_security_policy.cc`、`csp_source_list.cc`、`csp_source.cc`，讀完）

- 每一個 `content-security-policy`、`content-security-policy-report-only` header 分開；每個值照 `,` 切成 policy（RFC 7230 的合併），照 `;` 切 directive，名字不分大小寫，重複的忽略（「Ignoring duplicate Content-Security-Policy directive '%s'.」）。
- `frame-ancestors`：report-only 支援（`SupportedInReportOnly` 是 true），`<meta>` 不支援（「is ignored when delivered via a <meta> element」）；不支援的來源：「The Content-Security-Policy directive 'frame-ancestors' does not support the source expression '%s'」。
- `CheckContentSecurityPolicy`：`frame-ancestors` 不走 extension 那些 scheme 的例外（crbug 1115590：「A document should be able to deny being embedded inside an extension」），沒有 fallback 的 directive；沒有這個 directive 就准；report-only 的一律回答准（只回報）。
- `CheckCSPSourceList`：
  1. `*`：`http`、`https` 的網址准；網址的 scheme 等於回應自己的 scheme 也准（「Wildcards match network schemes ('http', 'https', 'ftp', 'ws', 'wss'), and the scheme of the protected resource.」）。
  2. `'self'`：照回應的 origin 比（`CheckCSPSource`），`http` 對 `https`、`ws` 對 `wss` 算 upgrade、准。
  3. 來源清單：任何一個合就准。主機要相等，`*.a.example` 要以 `.a.example` 結尾（不合 `a.example`），沒寫 port 的合預設的 port，`:*` 合任何 port，80 對 443 算 upgrade；路徑在跟過 redirect 時不看。scheme 的來源（`https:`）只比 scheme（`http:` 也合 `https`）。
  4. 都不合：擋。`'none'` 是空的清單。
- 違反的訊息：「Framing '$1' violates the following Content Security Policy directive: "$2". The request has been blocked.」（`$1` 是 policy 的 self origin）。
- 沒讀到：opaque origin 的 `GetURL()` 是什麼（照程式推，scheme 是空的就什麼都不合，連 `*` 也是；要實測）；好幾個 policy 的結果怎麼合（照規範是每一個都要准）。

### 沒讀到的

- `navigation_request.cc` 裡 `BLOCK_RESPONSE` 怎麼變成錯誤：錯誤碼、子 frame 的錯誤頁的網址和 origin（讀到的部分沒有這一段）。下一節實測了錯誤碼和 `load`。

## Chrome 實測

2026-11-28，Chromium 156.0.8078.79（Fedora 的 build），headless，經過 `--remote-debugging-pipe` 開 `Network`、`Log`：`http://127.0.0.1` 的頁面依序放五個 `http://localhost` 的 iframe（跨源），等每一個的 `load` 或 `error`。

| iframe 的回應 | `Network.loadingFailed` | iframe 的事件 |
| --- | --- | --- |
| `X-Frame-Options: DENY` | `net::ERR_BLOCKED_BY_RESPONSE`（type `Document`） | `load` |
| `Content-Security-Policy: frame-ancestors 'none'` | `net::ERR_BLOCKED_BY_RESPONSE` | `load` |
| 沒有 | 沒有（照常載入） | `load` |
| 一個 header 的 `X-Frame-Options: DENY, SAMEORIGIN` | `net::ERR_BLOCKED_BY_RESPONSE` | `load` |
| 一個 header 的 `X-Frame-Options: SAMEORIGIN, SAMEORIGIN` | `net::ERR_BLOCKED_BY_RESPONSE`（跨源，`SAMEORIGIN` 本來就擋，看不出解析） | `load` |

`Log.entryAdded` 只有 CSP 那一條：「Framing 'http://localhost:8768/' violates the following Content Security Policy directive: "frame-ancestors 'none'". The request has been blocked.」（level `error`）；`X-Frame-Options` 的訊息沒有出現在最上層的 session 的 `Log` 裡（`AddDeferredConsoleMessage` 大概是那個 frame 的，沒有 attach 子 frame 的 target）。

### 第 2 段：console 的訊息

2026-11-28，同一個 Chromium 156，這次對每個 attach 上的 target（`Target.setAutoAttach`，跨網站的 iframe 是自己的 target）都開 `Log`、`Network`，記下每一條 `Log.entryAdded` 是哪個 session 的、`source`、`level`、`text`、`url`、`networkRequestId`。頁面是 `http://127.0.0.1:P` 的，iframe 有跨網站的（`http://localhost:P`）和同網站、跨源的（`http://127.0.0.1:P2`）；另外一個巢狀的：跨網站的 iframe 裡再嵌一個 `frame-ancestors 'none'` 的同源 frame。

| 被擋的 | `Log.entryAdded` | 掛在哪個 frame |
| --- | --- | --- |
| `X-Frame-Options: DENY` | `Refused to display 'http://localhost:P/' in a frame because it set 'X-Frame-Options' to 'deny'.`；`source` `other`、`level` `error`、`url` `chrome-error://chromewebdata/`、`lineNumber` 0 | 被擋的 frame（commit 了的錯誤頁）：跨網站的在 iframe 的 session，同網站的在最上層的 session |
| `SAMEORIGIN`（跨源） | 同上，`to 'sameorigin'.` | 同上 |
| `DENY, SAMEORIGIN`（一個 header） | `Refused to display 'http://localhost:P/' in a frame because it set multiple 'X-Frame-Options' headers with conflicting values ('DENY, SAMEORIGIN'). Falling back to 'deny'.` | 同上 |
| `X-Frame-Options: bogus`（不擋） | `Invalid 'X-Frame-Options' header encountered when loading 'http://localhost:P/': 'bogus' is not a recognized directive. The header will be ignored.`；`url` 是那份文件的網址 | 那個 frame（commit 了的文件） |
| `frame-ancestors 'none'`、`'self'` | `Framing 'http://localhost:P/' violates the following Content Security Policy directive: "frame-ancestors 'none'". The request has been blocked.` 加一個 `\n`；`source` `security`、`level` `error`，沒有 `url`、`lineNumber` | 嵌它的那一層：直接嵌在頁面裡的在最上層，巢狀的在中間那個 iframe 的 session |
| ORB（`<img>`、`<script>` 拿到 `application/json`） | 沒有 | — |
| CORP `same-origin`、`same-site` | `Failed to load resource: net::ERR_BLOCKED_BY_RESPONSE.NotSameOrigin`（`.NotSameSite`）；`source` `network`、`url` 是請求的網址、帶 `networkRequestId` | 發請求的文件 |
| 對照：`ERR_UNSAFE_PORT` 的圖、404 的圖 | `Failed to load resource: net::ERR_UNSAFE_PORT`；`Failed to load resource: the server responded with a status of 404 (Not Found)` | 發請求的文件 |

`Network.loadingFailed`：`X-Frame-Options` 和 `frame-ancestors` 擋的是 `net::ERR_BLOCKED_BY_RESPONSE`、`blockedReason` `other`；CORP 的是 `net::ERR_BLOCKED_BY_RESPONSE.NotSameOrigin`、`blockedReason` `corp-not-same-origin`（`same-site` 的是 `.NotSameSite`、`corp-not-same-site`）；ORB 的是 `net::ERR_BLOCKED_BY_ORB`，沒有 `blockedReason`。

錯誤頁的 iframe 共用一個 process，後 attach 的錯誤頁的 session 在 `Log.enable` 時重播了前面幾個錯誤頁的訊息；表裡只記第一次出現的。

### 第 2 段讀的 Chromium

2026-11-28，`chromium.googlesource.com` 的 `main`。

- `ancestor_throttle.cc`：`X-Frame-Options` 的四句（`ConsoleErrorXFrameOptions`、`ParseXFrameOptionsError`）經過 `NavigationRequest::AddDeferredConsoleMessage`，在被導覽的 frame commit 之後寫進它的 console；網址是 `url::Origin::Create(url).GetURL()`（origin 加 `/`），衝突和無效的那兩句帶 `GetNormalizedHeader("X-Frame-Options")`（幾個 header 用 `, ` 接起來）。有 `frame-ancestors` 時不看 `X-Frame-Options`，也不寫它的訊息。redirect 的那一次不寫（`DO_NOT_LOG_TO_CONSOLE`）。`frame-ancestors` 從父 frame 往上，每一層用那一層的 `RenderFrameHostCSPContext` 檢查：違規「經過不准的那個祖先回報」（第一個不合的那一層，`GetAssociatedOutsideContextCSPViolationReporter`），不一定是父 frame。
- `content_security_policy.cc`：`ReportViolation` 的訊息是 `Framing '$1' violates the following [report-only ]Content Security Policy directive: "$2". ` 加 `The request has been blocked.`（report-only 的是 `The violation has been logged, but no further action has been taken.`），清單裡有 `*` 時再加 `Note that '*' matches only URLs with network schemes ('http', 'https', 'ws', 'wss'), or URLs whose scheme matches `self`'s scheme. The scheme '<blocked URL 的 scheme>:' must be added explicitly.`，最後一個 `\n`。`$1` 是回應的 origin（`self_origin`）的網址，`$2` 是 `frame-ancestors ` 加上解析之後再序列化的清單（`csp_source_list.cc` 的 `ToString`：空的是 `'none'`，有 `*` 就只是 `*`，否則 `'self'` 在前、來源照順序；來源照 `csp_source.cc` 的 `ToString`：`scheme:`，或 `scheme://`、host（`*`、`*.host`）、`:*` 或 `:port`、path）。每一個 policy 各自檢查、各自寫（`CSPContext::IsAllowedByCsp` 用 `CHECK_ALL_CSP`，report-only 的也寫，但不擋）。
- 解析（`ParseSource`、`ParseHost`、`ParsePort`、`ParsePath`）：不合語法的 token 丟掉（並且記一條解析的警告，例如 `The Content-Security-Policy directive 'frame-ancestors' does not support the source expression '%s'`，文件 commit 時寫進它的 console）；scheme、host 轉小寫，host 的每一段只能是字母、數字、`-`（只有最後一段可以是空的），port 只能是數字或 `*`，path 解開 `%` 編碼，`?`、`#` 之後的丟掉。
- `frame_console.cc` 的 `DidFailLoading`：每個失敗的子資源寫 `Failed to load resource: <ResourceError 的描述>`；ORB 擋的不寫（「ORB loading errors are reported from the network service directly to DevTools」，`CorsURLLoader::ReportOrbErrorToDevTools`，是 `Audits` 的 issue），CORS 的錯誤在 `kDevToolsImprovedNetworkError` 開著時也不寫。

## 規範

只讀到 model 摘要的部分，引用前要再對原文：

- **HTML 7.7「The X-Frame-Options header」**（在 `speculative-loading.html`，不在 `document-lifecycle.html`）：「check a navigation response's adherence to `X-Frame-Options`」：不是 child navigable 就准；有強制的 `frame-ancestors` 就准；get, decode, and split `X-Frame-Options`，轉小寫放進一個 set；set 大於一個而且含 `deny`、`allowall`、`sameorigin` 任何一個就擋；大於一個（都是無效的）就准；`deny` 擋；`sameorigin` 照每一層 container document 的 origin 比；其他准。摘要附的表：「SAMEORIGIN, SAMEORIGIN」准同源、「SAMEORIGIN, DENY」擋、「SAMEORIGIN,」擋、「SAMEORIGIN, ALLOWALL」擋、「INVALID, INVALID」准，一個 header 用逗號或好幾個 header 都一樣。
  - 和 Chromium 不同的一處：規範先照 `,` 切，Chromium 不切；`SAMEORIGIN, SAMEORIGIN` 在一個 header 裡，規範是同源，Chromium 是無效（放行）。照 WPT 上 Chrome 的結果。
- **CSP3**：policy 的 self-origin 是「response's url's origin」（§2.2.2，讀到原文）。「frame-ancestors Navigation Response Check」（§6.4.2.1，摘要）：回應的網址是 local 的准；不是 child navigable 准；從 target 往上，每一層 container document 的 origin（序列化再 parse 成網址）照 URL matching 比，不合就擋。

## 參考資料

- Chromium 的原始碼：`content/browser/renderer_host/ancestor_throttle.cc`、`.h`，`services/network/public/cpp/x_frame_options_parser.cc`，`services/network/public/cpp/content_security_policy/content_security_policy.cc`、`csp_source_list.cc`、`csp_source.cc`
- [HTML：The X-Frame-Options header](https://html.spec.whatwg.org/multipage/speculative-loading.html#the-x-frame-options-header)
- [CSP3：frame-ancestors](https://w3c.github.io/webappsec-csp/#frame-ancestors)
