# RFC 0129：不讓別人嵌：`X-Frame-Options` 和 CSP 的 `frame-ancestors`

- 開始日期：2026-11-28
- 狀態：🚧 已排程：2026-11-28
- 進度和驗收：`progress.md`
- 細節：`details.md`

RFC 0122 路線圖的 F1（M1 的最後一份）。

## 摘要

網站現在不能拒絕被別的網站放進 iframe：machopo 不看 `X-Frame-Options`，也沒有 CSP。這份讓 Browser 在子 frame 的導覽拿到最後的回應、commit 之前，照 Chromium 的 `AncestorThrottle` 檢查兩個 header：

- **CSP 的 `frame-ancestors`**：每一個祖先 frame 的 origin 都要合它的來源清單。
- **`X-Frame-Options`**：`DENY` 一律不准，`SAMEORIGIN` 要每一個祖先都和回應同源。有強制的 `frame-ancestors` 時不看它。

不准的子 frame 不載入那份回應，改 commit 錯誤頁。main frame 不檢查。兩個都在 Browser 檢查，renderer 不用知道。

## 動機

- **L1 底下正是它管的事**（RFC 0122 的「順序」）：OOPIF 之前，被嵌的網站和嵌它的頁面在同一個 renderer。網站送 `X-Frame-Options: DENY` 或 `frame-ancestors 'none'`，就是在說「不要把我放進別人的頁面、也就是別人的 process」。machopo 現在照放。
- **clickjacking**：machopo 不送第三方的 cookie，所以被嵌進別的網站的頁面大多是沒登入的狀態，clickjacking 大多做不成（RFC 0122 的已決定的事項第 6 條）。但同一個網站的子網域互相嵌時 cookie 照送，內網的頁面也不用 cookie；之後放寬第三方 cookie 時也要靠它。
- **和 Chrome 不同**：網站照 Chrome 的行為設這兩個 header，machopo 照樣把它們顯示在 iframe 裡。

**目標**

- 被 Browser 擋下來的（`X-Frame-Options`、`frame-ancestors`，和 RFC 0128 的 ORB、CORP）在 DevTools 看得到原因：Network 的 `loadingFailed`，console 的訊息照 Chrome 的原文（已決定的事項第 3 條）。
- 子 frame 的導覽在 commit 之前照 `frame-ancestors` 和 `X-Frame-Options` 檢查，規則和 Chromium 的 `AncestorThrottle` 一樣，不准的 commit 錯誤頁。
- WPT 的 `x-frame-options/`、`content-security-policy/frame-ancestors/` 能跑的都跑，逐項和 Chrome 比（RFC 0128 的做法：wpt.fyi 的 `report.json`）。

**不在範圍內**

- **完整的 CSP**：`script-src`、`default-src`、`<meta>` 的 CSP、`securitypolicyviolation` 事件、回報（`report-uri`、`report-to`）。這份只解析 `Content-Security-Policy` header 裡的 `frame-ancestors`（`<meta>` 的 `frame-ancestors` 照規範本來就不算）。
- **iframe 的 `sandbox`**（路線圖的 F2）、CSP 的 `sandbox`。
- **Chromium 的「embedding requires opt-in」**：沒有任何 header 的跨源 frame 也擋。Chromium 有這個 feature，預設關著。
- fenced frame（machopo 沒有）。
- 回報（`report-uri`、`report-to`）：console 的訊息在這份（已決定的事項第 3 條），送給伺服器的回報不在。

要解決的 `docs/limitations.md` 的限制：「WebDriver 和自動化」裡 DevTools 的 `Log` 那一條（「只有載入的兩種訊息（Chrome 還有 CSP、mixed content……）」）的一部分：做完時 `Log` 多了被擋的四種（已決定的事項第 3 條），CSP 的只有 `frame-ancestors` 的；照這個改寫那一條。

## 設計概要

給使用網頁的人：

- `<iframe src="https://bank.example/">` 嵌進 `evil.example`，`bank.example` 送 `X-Frame-Options: DENY` 或 `Content-Security-Policy: frame-ancestors 'none'` 時，iframe 裡是錯誤頁，不是銀行的頁面。和 Chrome 一樣。
- `SAMEORIGIN`、`frame-ancestors 'self' https://partner.example` 照規範：每一層祖先都要准，不只是直接的父 frame。
- 網站自己嵌自己、或沒有送這兩個 header 的，照舊。

給改 machopo 的人：

- 檢查在 Browser 的 `WebContents`，子 frame 的導覽拿到最後的回應（redirect 跟完）之後、commit 之前；祖先的 origin 從 Browser 的 frame 樹取。
- `net` 多一個解析 `Content-Security-Policy` 的模組，這份只用到 `frame-ancestors`，之後的 CSP 接在它上面（已決定的事項第 1 條）。
- 導覽在拿到 head 之後失敗、改 commit 錯誤頁，是新的一條路（現在只有沒拿到回應時才 commit 錯誤頁）。

## 詳細設計

開始時的盤點、Chromium 和規範的細節在 `details.md`。

### 在哪裡檢查

- **只看子 frame**：main frame 的導覽不檢查（Chromium：「Allow main frame navigations」；HTML：「If navigable is not a child navigable, then return true」）。
- **只看最後的回應**：redirect 的每一跳不擋。Chromium 在 redirect 時算一次、只記 metrics（`WillRedirectRequest` 一律 `PROCEED`）；規範也只看最後的回應。
- **不交給 renderer 的回應不看**：`204`、`205`（Chromium 的 `response_should_be_rendered`）。machopo 現在沒有下載。
- **時機**：`WebContents` 的 `commit_response` 一開始，回應的 head 在手上、還沒 commit。祖先是子 frame 在 Browser 的 frame 樹上的每一層父 frame，用它們最後 commit 的 origin（Chromium 的 `GetLastCommittedOrigin`）；父 frame 自己正在導覽時，照它現在顯示的那份文件的 origin。

### `frame-ancestors`

照 Chromium 的 `content_security_policy.cc`、`csp_source_list.cc`、`csp_source.cc`（`details.md` 的「Chromium」）：

- **解析**：每一個 `Content-Security-Policy` header，每個值照 `,` 切成好幾個 policy，每個 policy 照 `;` 切成 directive，directive 的名字不分大小寫，重複的以第一個為準。`Content-Security-Policy-Report-Only` 的解析、但不擋（Chromium 的 report-only 不擋，只回報；machopo 沒有回報，等於不看）。
- **每一個強制的 policy 都要准**：policy 沒有 `frame-ancestors` 就准（它沒有 fallback，`default-src` 不管它）。
- **每一層祖先都要合**，從父 frame 往上到最上層：
  - `'none'`：空的清單，什麼都不合。
  - `'self'`：回應的網址的 origin；`http` 的 `'self'` 也合 `https` 的同一個主機（Chromium 的 scheme upgrade）。
  - `*`：合任何 `http`、`https` 的祖先；不合 `data:`、`file:` 和 opaque 的祖先。Chromium 的 `*` 也合回應自己的 scheme（給 `chrome-extension:` 這類），machopo 不寫這一支：`url` 給不是 special 的 scheme 的是 opaque 的 origin，祖先是文件，special 的 scheme 裡除了 `http`、`https` 只剩 `file:`（也是 opaque），走不到。
  - 主機的來源：`https://a.example`、`*.a.example`（不合 `a.example` 本身）、port（沒寫的合預設的 port、`:*` 合任何 port）；路徑對祖先的 origin 沒有意義。
  - scheme 的來源：`https:`。
  - opaque 的祖先（`data:` 的 iframe、錯誤頁）：任何強制的清單都不合，`*` 也不合（Chromium 拿 opaque origin 的 `GetURL()` 去比，照程式推是空的網址、什麼都不合）。WPT 裡 Chrome 擋 sandbox 的祖先（`frame-ancestors-nested-cross-in-sandboxed-cross-url-block.html`、`frame-ancestors-sandbox-same-origin-self.html`），合這一條；machopo 沒有 iframe 的 `sandbox`，祖先不是 opaque 的，這兩個比不了（預期的失敗），由 `net` 的單元測試守著。
- 不認得的來源（`'unsafe-inline'` 這些）什麼都不合；`'none'` 和別的來源一起時不算（Chromium 忽略它）。

### `X-Frame-Options`

照 Chromium 的 `x_frame_options_parser.cc` 和 `ancestor_throttle.cc`：

- **解析**：照 HTML 的「get, decode, and split」，每一個 header 的值照 `,` 切開，每個值去掉前後的空格和 tab、不分大小寫：`deny`、`sameorigin`、`allowall`，其他的是無效。去掉重複之後有兩個以上、其中有關鍵字的是衝突；都是無效的值不算（Chromium 把每個值先分類再比，無效的都是同一類，結果一樣）。不支援 `ALLOW-FROM`。
  - 一個 header 裡的 `DENY, SAMEORIGIN` 是衝突、擋：Chrome 實測是這樣（`details.md` 的「Chrome 實測」）。Chromium 的 `x_frame_options_parser.cc` 本身不切，切開應該是它列舉 header 的 `EnumerateHeader` 做的（照結果推的，沒讀到）。
- **判斷**：
  - 有強制的 `frame-ancestors`：不看 `X-Frame-Options`（`DENY`、衝突、無效都一樣），只照 `frame-ancestors`；report-only 的不算。
  - `deny`、衝突：擋。
  - `sameorigin`：每一層祖先都要和回應的網址同源，有一層不是就擋。
  - `allowall`、無效、沒有：放行。

### 被擋之後（已決定的事項第 2 條）

照 Chromium：導覽失敗，子 frame commit 錯誤頁（opaque 的 origin，記著要去的網址），錯誤碼 `ERR_BLOCKED_BY_RESPONSE`，iframe 的 `load` 照樣觸發（Chrome 實測的，`details.md` 的「Chrome 實測」）。回應的 body 不送給 renderer。machopo 現在只有「沒拿到回應」才 commit 錯誤頁（`on_navigation_completed`），這是新的一條：在 `commit_response` 拿到 head、還沒 commit 時改走錯誤頁，回應的 body 丟掉、載入停下（`net::LoadHandle::cancel`，不再回呼）。

### DevTools 看得到的（已決定的事項第 3 條）

- **Network**：拿到 head 之後才失敗的子 frame 導覽，自動化（`shell` 的 `automation_session.rs`）照 Chrome 送 `Network.loadingFailed`（`errorText` 是 `net::ERR_BLOCKED_BY_RESPONSE`、type 是 `Document`）。原本這種導覽什麼都不送，Network 面板停在「收到回應」。`WebContents` 擋下時發一個新的 `ContentsEvent::NavigationRefused`（帶 `NetError`），自動化照它送；導覽留在「已經收到回應」的那一組裡，載入在停下之前已經結束、結束的事件還排著時，照回應之後的錯誤不再送一次（`docs/test-gaps.md`）。
- **console**：Browser 替一個 frame 加一條 console 訊息（Chromium 的 `NavigationRequest::AddDeferredConsoleMessage`、`RenderFrameHost::AddMessageToConsole`），renderer 照 DevTools 的 `Log` 送出去。四種：
  - `X-Frame-Options`：Chromium 的三句原文（`details.md` 的「Chromium」），level `error`；
  - `frame-ancestors`：「Framing '…' violates the following Content Security Policy directive: "…". The request has been blocked.」，level `error`；
  - ORB、CORP（RFC 0128）：照 Chrome 實測的原文。
  - 每一種掛在哪個 frame（被擋的 frame、嵌它的頁面、發請求的文件）照 Chrome 實測。machopo 沒有 OOPIF，一個分頁的 frame 都在同一個 target，訊息都在它的 `Log` 裡；Chrome 的跨網站 iframe 是自己的 target，這一點本來就不同。

### 測試

- `net` 的單元測試：CSP 的解析（`,`、`;`、大小寫、重複的 directive、report-only）、每一種來源的比對；`X-Frame-Options` 的解析（每一種值、空白、大小寫、衝突、無效）。
- `browser` 的整合測試：跨源的 iframe 拿到 `DENY`、`SAMEORIGIN`、`frame-ancestors` 時 commit 錯誤頁，同源的照常；巢狀（同源的父 frame 裡的跨源 frame 再嵌同源的）照每一層判斷；main frame 不檢查；redirect 的中間一跳不擋。
- WPT：`x-frame-options/`、`content-security-policy/frame-ancestors/`，逐項和 Chrome 比。
- mutation test：每一條規則。

### 分階段

兩段（已決定的事項第 4 條），每一段做完都可以 merge 回 `main`：

1. **檢查**：CSP 的解析（第 1 條）、`frame-ancestors`、`X-Frame-Options`、被擋時 commit 錯誤頁的新路（第 2 條）、Network 的 `loadingFailed`；`x-frame-options/`、`content-security-policy/frame-ancestors/` 的 WPT，逐項和 Chrome 比。做完就達成 M1。
2. **console 的訊息**：「Browser 替一個 frame 加一條 console 訊息」的路，四種被擋的（`X-Frame-Options`、`frame-ancestors`、ORB、CORP）照 Chrome 實測的原文和 frame（第 3 條）；收尾。

## 缺點

- **CSP 的解析器只解讀 `frame-ancestors`**：其他 directive 只留著原文，之後做完整的 CSP 時接著解讀（已決定的事項第 1 條）。
- **相容性**：網站送錯的 `X-Frame-Options`（例如 `ALLOW-FROM`）時，Chrome 和 machopo 都當成無效、放行；網站以為擋住了。這和 Chrome 一樣。

## 理由和其他做法

- **不做的話**：網站拒絕被嵌，machopo 照嵌，L1 底下被嵌的頁面就在別人的 renderer 裡。
- **在 renderer 檢查 `frame-ancestors`**：路線圖原本記著 Chromium 在 renderer 檢查（「Compromised renderers」的文件），實際讀了 `ancestor_throttle.cc`，Chromium 現在在 Browser 檢查（`EvaluateFrameAncestors`）。在 renderer 檢查擋不住被攻破的 renderer，而且這時回應已經交給它了。所以兩個都放 Browser，和 Chromium 一樣，不用另外選。
- 各題的選項和不選的理由在「已決定的事項」。

## 先例

`details.md` 的「Chromium」「規範」：Chromium 的 `AncestorThrottle`（navigation throttle，`WillProcessResponse`）、`x_frame_options_parser.cc`、`content_security_policy.cc`；HTML 的「check a navigation response's adherence to `X-Frame-Options`」；CSP3 的「frame-ancestors Navigation Response Check」。

## 已決定的事項

1. **CSP 解析成通用的 policy 和 directive，這份只解讀 `frame-ancestors`（P1）**（2026-11-28，使用者決定）：`net` 一個 CSP 的模組，把 `Content-Security-Policy` 和 `-Report-Only` 的每個值照 `,` 切成 policy、照 `;` 切成 directive（名字不分大小寫、重複的以第一個為準），directive 留著名字和原文；只有 `frame-ancestors` 往下解讀成來源清單。理由：解析的規則（policy 的切法、大小寫、重複、report-only）這次就照 Chromium 寫好、測好，之後的 CSP（`script-src`、`default-src` 這些）接在同一個結構上，用到時才解讀它們的值。這是 Chromium 的 `ContentSecurityPolicy` 結構的前半段（network service 把整個 header 解析成 directive 對來源清單，`frame-ancestors` 在 Browser 的 `AncestorThrottle` 評估）。不選只寫一個找 `frame-ancestors` 的函式（P2：之後做 CSP 時要重寫解析）。不選完整的 CSP 解析（P3：nonce、hash、每個 directive 的來源清單，這份一個都用不到，也沒有測試守得住）。
2. **被擋的子 frame commit 錯誤頁，`ERR_BLOCKED_BY_RESPONSE`，iframe 的 `load` 照樣觸發（E1）**（2026-11-28，使用者決定）：導覽失敗，子 frame commit 錯誤頁（opaque 的 origin，記著要去的網址），回應的 body 丟掉、不送給 renderer。理由：和 Chrome 實測的一樣（Chromium 156：DevTools 的 `Network.loadingFailed` 是 `net::ERR_BLOCKED_BY_RESPONSE`，iframe 觸發 `load`，`details.md` 的「Chrome 實測」）；machopo 已經有失敗的子 frame commit 錯誤頁、`load` 照常的路，錯誤碼也有（CORP 用的）；錯誤頁的 origin 是 opaque 的，嵌它的頁面讀不到被擋的是什麼。代價：要新開「拿到 head 之後改走錯誤頁」的路（現在只有沒拿到回應時才 commit 錯誤頁）。不選 commit 空的 `about:blank`（E2：和 Chrome 不同，使用者看不出被擋了）。
3. **被擋的原因在 DevTools 看得到：Network 的 `loadingFailed` 在第 1 段，console 的訊息另成一段、四種一起（M1'）**（2026-11-28，使用者決定）：拿到 head 之後才失敗的子 frame 導覽，自動化照 Chrome 送 `Network.loadingFailed`（`ERR_BLOCKED_BY_RESPONSE`），這是第 2 條那條新路的一部分。另外開「Browser 替一個 frame 加一條 console 訊息」的路，Browser 擋下來的四種都照 Chrome 的原文送：`X-Frame-Options`、`frame-ancestors`（這份），和 RFC 0128 的 ORB、CORP；訊息掛在哪個 frame、原文是什麼，照 Chrome 實測（實測時 CSP 那一條在嵌它的頁面的 `Log` 裡，`X-Frame-Options` 的在被擋的 frame 的）。理由：四種要的是同一條路，一次做完；M1 做完時被擋的原因都看得到；它很小，放進 M1 的最後一份收尾，不另外開 RFC。代價：0129 的範圍超出 F1，回頭補 0128 的訊息（0128 的「之後的可能」裡的那一條由這份做）。不選另外開一份小的 RFC（M2：多一份流程，而它很小）。不選只寫在「之後的可能」（M3：沒有一份 RFC 保證會做；草稿原本的看法，沒說清楚這一點）。
4. **分兩段：先檢查，再 console 的訊息（G1）**（2026-11-28，使用者決定）：照「分階段」。理由：第 1 段是 F1 本身，做完就達成 M1、可以先 merge；第 2 段的路和 F1 無關（四種被擋的共用），單獨驗收，開始時先實測 Chrome 的訊息原文和掛在哪個 frame。代價：階段的檢查跑兩次。不選一段做完（G2：M1 要等 console 那條路才達成）。不選三段（G3：`X-Frame-Options` 和 `frame-ancestors` 共用同一個時機和被擋之後的路，拆開不值得多跑一次檢查）。

## 待決定

沒有（已決定的事項第 1 到 4 條）。

## 之後的可能

- 完整的 CSP（`script-src`、`<meta>`、`securitypolicyviolation`、回報）。
- iframe 的 `sandbox` 和 CSP 的 `sandbox`（路線圖的 F2）。
- Chromium 的「embedding requires opt-in」，Chrome 預設打開時再看。
