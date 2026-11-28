# RFC 0129：不讓別人嵌：`X-Frame-Options` 和 CSP 的 `frame-ancestors`

- 開始日期：2026-11-28
- 狀態：✅ 已完成：2026-11-28
- 進度和驗收：`progress.md`
- 細節：`details.md`

RFC 0122 路線圖的 F1（M1 的最後一份）。

## 摘要

網站現在不能拒絕被別的網站放進 iframe：machopo 不看 `X-Frame-Options`，也沒有 CSP。這份讓 Browser 在子 frame 的導覽拿到最後的回應、commit 之前，照 Chromium 的 `AncestorThrottle` 檢查兩個 header：

- **CSP 的 `frame-ancestors`**：每一個祖先 frame 的 origin 都要合它的來源清單。
- **`X-Frame-Options`**：`DENY` 一律不准，`SAMEORIGIN` 要每一個祖先都和回應同源。有強制的 `frame-ancestors` 時不看它。

不准的子 frame 不載入那份回應，改 commit 錯誤頁。main frame 不檢查。兩個都在 Browser 檢查，renderer 不用知道。被擋的原因在 DevTools 看得到，和 Chrome 一樣：Network 的 `loadingFailed` 帶原因，console 有 Chromium 的原文；RFC 0128 的 CORP 一起帶上原因，ORB 照 Chrome 不寫 console。

## 動機

- **L1 底下正是它管的事**（RFC 0122 的「順序」）：OOPIF 之前，被嵌的網站和嵌它的頁面在同一個 renderer。網站送 `X-Frame-Options: DENY` 或 `frame-ancestors 'none'`，就是在說「不要把我放進別人的頁面、也就是別人的 process」。machopo 現在照放。
- **clickjacking**：machopo 不送第三方的 cookie，所以被嵌進別的網站的頁面大多是沒登入的狀態，clickjacking 大多做不成（RFC 0122 的已決定的事項第 6 條）。但同一個網站的子網域互相嵌時 cookie 照送，內網的頁面也不用 cookie；之後放寬第三方 cookie 時也要靠它。
- **和 Chrome 不同**：網站照 Chrome 的行為設這兩個 header，machopo 照樣把它們顯示在 iframe 裡。

**目標**

- 被 Browser 擋下來的（`X-Frame-Options`、`frame-ancestors`，和 RFC 0128 的 ORB、CORP）在 DevTools 看到的和 Chrome 一樣：Network 的 `loadingFailed` 和它的 `blockedReason`，console 的訊息照 Chrome 實測的原文和 frame（已決定的事項第 3、5 條）。
- 子 frame 的導覽在 commit 之前照 `frame-ancestors` 和 `X-Frame-Options` 檢查，規則和 Chromium 的 `AncestorThrottle` 一樣，不准的 commit 錯誤頁。
- WPT 的 `x-frame-options/`、`content-security-policy/frame-ancestors/` 能跑的都跑，逐項和 Chrome 比（RFC 0128 的做法：wpt.fyi 的 `report.json`）。

**不在範圍內**

- **完整的 CSP**：`script-src`、`default-src`、`<meta>` 的 CSP、`securitypolicyviolation` 事件、回報（`report-uri`、`report-to`）。這份只解析 `Content-Security-Policy` header 裡的 `frame-ancestors`（`<meta>` 的 `frame-ancestors` 照規範本來就不算）。
- **iframe 的 `sandbox`**（路線圖的 F2）、CSP 的 `sandbox`。
- **Chromium 的「embedding requires opt-in」**：沒有任何 header 的跨源 frame 也擋。Chromium 有這個 feature，預設關著。
- fenced frame（machopo 沒有）。
- 回報（`report-uri`、`report-to`）：console 的訊息在這份（已決定的事項第 3、5 條，report-only 的也寫），送給伺服器的回報不在。
- CSP 的解析警告（「does not support the source expression …」）和 DevTools 的 `Audits` issue（已決定的事項第 5 條）。

要解決的 `docs/limitations.md` 的限制：「WebDriver 和自動化」裡 DevTools 的 `Log` 那一條（「只有載入的兩種訊息（Chrome 還有 CSP、mixed content……）」）的一部分：`Log` 多了 Browser 替 frame 加的 `X-Frame-Options`、`frame-ancestors` 的訊息，ORB 擋的不再寫；那一條照這個改寫了，其他的 CSP 訊息留在那裡。

## 設計概要

給使用網頁的人：

- `<iframe src="https://bank.example/">` 嵌進 `evil.example`，`bank.example` 送 `X-Frame-Options: DENY` 或 `Content-Security-Policy: frame-ancestors 'none'` 時，iframe 裡是錯誤頁，不是銀行的頁面。和 Chrome 一樣。
- `SAMEORIGIN`、`frame-ancestors 'self' https://partner.example` 照規範：每一層祖先都要准，不只是直接的父 frame。
- 網站自己嵌自己、或沒有送這兩個 header 的，照舊。

給改 machopo 的人：

- 檢查在 Browser 的 `WebContents`，子 frame 的導覽拿到最後的回應（redirect 跟完）之後、commit 之前；祖先的 origin 從 Browser 的 frame 樹取。
- `net` 多一個解析 `Content-Security-Policy` 的模組，這份只用到 `frame-ancestors`，之後的 CSP 接在它上面（已決定的事項第 1 條）。
- 導覽在拿到 head 之後失敗、改 commit 錯誤頁，是新的一條路（原本只有沒拿到回應時才 commit 錯誤頁）。
- Browser 替 frame 加 console 訊息，是給 renderer 的兩個新訊息（`AddMessageToConsole`、`ReportCspViolation`，照 Chromium 的兩個介面），renderer 記進它的 `Log`（已決定的事項第 6 條）。
- `NetError` 帶被擋的原因（`BlockedByResponseReason`，Chromium 的 `blocked_by_response_reason`），DevTools 的 `errorText`、`blockedReason` 和 console 的訊息都從它來。

## 詳細設計

開始時的盤點、Chromium 和規範的細節在 `details.md`。

### 在哪裡檢查

- **只看子 frame**：main frame 的導覽不檢查（Chromium：「Allow main frame navigations」；HTML：「If navigable is not a child navigable, then return true」）。
- **只看最後的回應**：redirect 的每一跳不擋。Chromium 在 redirect 時算一次、只記 metrics（`WillRedirectRequest` 一律 `PROCEED`）；規範也只看最後的回應。
- **不交給 renderer 的回應不看**：`204`、`205`（Chromium 的 `response_should_be_rendered`）。machopo 現在沒有下載。
- **時機**：`WebContents` 的 `commit_response` 一開始，回應的 head 在手上、還沒 commit。祖先是子 frame 在 Browser 的 frame 樹上的每一層父 frame，用它們最後 commit 的 origin（Chromium 的 `GetLastCommittedOrigin`）；父 frame 自己正在導覽時，照它現在顯示的那份文件的 origin。

### `frame-ancestors`

照 Chromium 的 `content_security_policy.cc`、`csp_source_list.cc`、`csp_source.cc`（`details.md` 的「Chromium」）：

- **解析**：每一個 `Content-Security-Policy` header，每個值照 `,` 切成好幾個 policy，每個 policy 照 `;` 切成 directive，directive 的名字不分大小寫，重複的以第一個為準。`Content-Security-Policy-Report-Only` 的也解析，不擋、只寫 console 的訊息（machopo 沒有送給伺服器的回報）。
- **來源照 Chromium 的 `ParseSource` 解析**：scheme、host 轉小寫，host 的每一段只能是字母、數字、`-`（只有最後一段可以是空的），port 只能是數字或 `*`，path 解開 `%` 編碼、`?`、`#` 之後的丟掉；不合語法的 token 丟掉。它決定違規的訊息裡印出的清單（第 5 條）。
- **每一個強制的 policy 都要准**：policy 沒有 `frame-ancestors` 就准（它沒有 fallback，`default-src` 不管它）。
- **每一層祖先都要合**，從父 frame 往上到最上層，每一層對每一個 policy 檢查（Chromium 的 `CHECK_ALL_CSP`）：不合的寫一條違規的訊息給那一層；有強制的 policy 不合時擋下，再上面的不看。
  - `'none'`：空的清單，什麼都不合。
  - `'self'`：回應的網址的 origin；`http` 的 `'self'` 也合 `https` 的同一個主機（Chromium 的 scheme upgrade）。
  - `*`：合任何 `http`、`https` 的祖先；不合 `data:`、`file:` 和 opaque 的祖先。Chromium 的 `*` 也合回應自己的 scheme（給 `chrome-extension:` 這類），machopo 不寫這一支：`url` 給不是 special 的 scheme 的是 opaque 的 origin，祖先是文件，special 的 scheme 裡除了 `http`、`https` 只剩 `file:`（也是 opaque），走不到。
  - 主機的來源：`https://a.example`、`*.a.example`（不合 `a.example` 本身）、port（沒寫的合預設的 port、`:*` 合任何 port）；路徑對祖先的 origin 沒有意義。
  - scheme 的來源：`https:`。
  - opaque 的祖先（`data:` 的 iframe、錯誤頁）：任何強制的清單都不合，`*` 也不合（Chromium 拿 opaque origin 的 `GetURL()` 去比，照程式推是空的網址、什麼都不合）。WPT 裡 Chrome 擋 sandbox 的祖先（`frame-ancestors-nested-cross-in-sandboxed-cross-url-block.html`、`frame-ancestors-sandbox-same-origin-self.html`），合這一條；machopo 沒有 iframe 的 `sandbox`，祖先不是 opaque 的，這兩個比不了（預期的失敗），由 `net` 的單元測試守著。
- 不認得的來源（`'unsafe-inline'` 這些）丟掉；`'none'` 和別的來源一起時不算（Chromium 忽略它）。

### `X-Frame-Options`

照 Chromium 的 `x_frame_options_parser.cc` 和 `ancestor_throttle.cc`：

- **解析**：照 HTML 的「get, decode, and split」，每一個 header 的值照 `,` 切開，每個值去掉前後的空格和 tab、不分大小寫：`deny`、`sameorigin`、`allowall`，其他的是無效。照 Chromium 的 `ParseXFrameOptions` 把每個值分類，兩個值的類不同就是衝突；都是無效的是「無效」（不擋，但寫一條訊息）。和 HTML 的「去掉重複之後兩個以上、其中有關鍵字」擋的一樣。不支援 `ALLOW-FROM`。
  - 一個 header 裡的 `DENY, SAMEORIGIN` 是衝突、擋：Chrome 實測是這樣（`details.md` 的「Chrome 實測」）。Chromium 的 `x_frame_options_parser.cc` 本身不切，切開應該是它列舉 header 的 `EnumerateHeader` 做的（照結果推的，沒讀到）。
- **判斷**：
  - 有強制的 `frame-ancestors`：不看 `X-Frame-Options`（`DENY`、衝突、無效都一樣），只照 `frame-ancestors`；report-only 的不算。
  - `deny`、衝突：擋。
  - `sameorigin`：每一層祖先都要和回應的網址同源，有一層不是就擋。
  - `allowall`、無效、沒有：放行。

### 被擋之後（已決定的事項第 2 條）

照 Chromium：導覽失敗，子 frame commit 錯誤頁（opaque 的 origin，記著要去的網址），錯誤碼 `ERR_BLOCKED_BY_RESPONSE`，iframe 的 `load` 照樣觸發（Chrome 實測的，`details.md` 的「Chrome 實測」）。回應的 body 不送給 renderer。machopo 現在只有「沒拿到回應」才 commit 錯誤頁（`on_navigation_completed`），這是新的一條：在 `commit_response` 拿到 head、還沒 commit 時改走錯誤頁，回應的 body 丟掉、載入停下（`net::LoadHandle::cancel`，不再回呼）。

### DevTools 看得到的（已決定的事項第 3、5、6 條）

照 Chrome 156 實測（`details.md` 的「第 2 段：console 的訊息」）和 Chromium 的原始碼（「第 2 段讀的 Chromium」）：

- **Network**：拿到 head 之後才失敗的子 frame 導覽，自動化（`shell` 的 `automation_session.rs`）照 Chrome 送 `Network.loadingFailed`（`net::ERR_BLOCKED_BY_RESPONSE`、type 是 `Document`、`blockedReason` 是 `other`）。原本這種導覽什麼都不送，Network 面板停在「收到回應」。`WebContents` 擋下時發一個新的 `ContentsEvent::NavigationRefused`（帶 `NetError`），自動化照它送；導覽留在「已經收到回應」的那一組裡，載入在停下之前已經結束、結束的事件還排著時，照回應之後的錯誤不再送一次（`docs/test-gaps.md`）。
- **被擋的原因**：`NetError` 帶 `BlockedByResponseReason`（Chromium 的 `blocked_by_response_reason`，machopo 有的是 CORP 的兩種）：`errorText` 和「Failed to load resource」的訊息是 `net::ERR_BLOCKED_BY_RESPONSE.NotSameOrigin`（`.NotSameSite`），`blockedReason` 是 `corp-not-same-origin`（`corp-not-same-site`）；沒有原因的 `ERR_BLOCKED_BY_RESPONSE` 是 `other`；其他的錯誤（ORB 的也是）沒有 `blockedReason`。
- **`X-Frame-Options` 的訊息**（`ancestor_throttle.cc` 的四句，回應的 origin 加 `/`，衝突和無效的帶 header 的值，幾個 header 用 `, ` 接起來）：收在導覽上，那個 frame commit 之後 Browser 送 `RendererMsg::AddMessageToConsole`（Chromium 的 `SendDeferredConsoleMessages`），renderer 記成那個 frame 的文件的（`source` `other`、`level` `error`、`url` 是文件、`lineNumber` 0：被擋的是錯誤頁的 `chrome-error://chromewebdata/`）。有強制的 `frame-ancestors` 時不寫。沒有 commit 時不送。
- **`frame-ancestors` 的違規**：「Framing '…' violates the following [report-only ]Content Security Policy directive: "frame-ancestors …". The request has been blocked.」（report-only 的是「The violation has been logged, but no further action has been taken.」），清單照 Chromium 序列化，有 `*` 時加上 Chromium 的說明，最後一個換行。檢查時當場送 `RendererMsg::ReportCspViolation` 給不准的那個祖先（Chromium 經過那一層的 CSP context），renderer 記成 `source` `security`、沒有 `url`。
- **ORB**：不寫（Blink 的 `FrameConsole::DidFailLoading` 跳過它，交給 network service 的 `Audits` issue，machopo 沒有）。原本 machopo 寫了「Failed to load resource: net::ERR_BLOCKED_BY_ORB」，拿掉了。
- **CORP**：照通用的「Failed to load resource」，帶原因。
- machopo 沒有 OOPIF，一個分頁的 frame 都在同一個 target、同一份 `Log`；Chrome 的跨網站 iframe 是自己的 target，訊息在它的 session 裡。訊息掛在哪個 frame 在 machopo 只看得到 `url`（`docs/test-gaps.md`）。

### 測試

- `net` 的單元測試：CSP 的解析（`,`、`;`、大小寫、重複的 directive、report-only）、每一種來源的解析、序列化和比對；`X-Frame-Options` 的解析（每一種值、空白、大小寫、衝突、無效）；每一句訊息的原文和給哪一層祖先；CORP 的原因。
- `browser` 的整合測試：跨源的 iframe 拿到 `DENY`、`SAMEORIGIN`、`frame-ancestors` 時 commit 錯誤頁，同源的照常；巢狀照每一層判斷；main frame、204 不檢查；redirect 的中間一跳不擋。
- `shell` 的整合測試：`loadingFailed` 的 `errorText`、`blockedReason`（XFO、ORB、CORP 的兩種）；`Log` 的訊息照 Chrome 實測的原文、`source`、`url`、`lineNumber`；ORB 不寫。
- WPT：`x-frame-options/`、`content-security-policy/frame-ancestors/`，逐項和 Chrome 比。
- mutation test：每一條規則（`mutants-1.toml`、`mutants-2.toml`）。

### 分階段

兩段（已決定的事項第 4 條），兩段都做完了（`progress.md`）：

1. **檢查**：CSP 的解析（第 1 條）、`frame-ancestors`、`X-Frame-Options`、被擋時 commit 錯誤頁的新路（第 2 條）、Network 的 `loadingFailed`；`x-frame-options/`、`content-security-policy/frame-ancestors/` 的 WPT，逐項和 Chrome 比。做完就達成 M1。
2. **console 的訊息**：「Browser 替一個 frame 加一條 console 訊息」的路（第 6 條），照 Chrome 實測寫的訊息和原因（第 5 條：`X-Frame-Options`、`frame-ancestors`、ORB 不寫、CORP 帶原因）；收尾。

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
5. **第 2 段照 Chrome 實測寫全部的訊息（S1）**（2026-11-28，使用者決定；實測和原始碼在 `details.md` 的「第 2 段：console 的訊息」「第 2 段讀的 Chromium」）：① `X-Frame-Options` 的四句（`deny`、`sameorigin`、衝突、無效；無效的不擋），寫在那個 frame commit 的文件裡；② `frame-ancestors` 的違規，強制的和 report-only 的都寫（report-only 的不擋），每個 policy 一條，寫在從父 frame 往上第一個不准的祖先裡，清單照 Chromium 的序列化：解析也照 Chromium 丟掉不合語法的 token（第 1 段拿掉的 host 的檢查加回來，它決定訊息裡的清單）；③ ORB 不寫（拿掉 machopo 現在的 `Failed to load resource: net::ERR_BLOCKED_BY_ORB`）；④ CORP 的錯誤帶原因（Chromium 的 `blocked_by_response_reason`）：`Failed to load resource` 的訊息和 `loadingFailed` 的 `errorText` 是 `net::ERR_BLOCKED_BY_RESPONSE.NotSameOrigin`（`.NotSameSite`），`loadingFailed` 加 `blockedReason`（CORP 的 `corp-not-same-origin`、`corp-not-same-site`，`X-Frame-Options` 和 `frame-ancestors` 的 `other`）。理由：DevTools 看到的和 Chrome 一樣；ORB 不再多一條 Chrome 沒有的訊息；這就是 Chromium 現在的做法（`AddDeferredConsoleMessage`、不准的祖先的 `RenderFrameHostCSPContext`、`FrameConsole::DidFailLoading` 跳過 ORB、`blocked_by_response_reason`）。代價：比第 3 條原本寫的多了 report-only 的訊息和 CORP 的原因（`NetError` 多一個欄位）。不選再加 CSP 的解析警告（S2：只解讀 `frame-ancestors` 時，別的 directive 的警告做不全，寫一半反而誤導，完整的 CSP 再做）。不選只寫被擋的那幾條（S3：和 Chrome 不同的地方留著，訊息少了原因）。不做 `Audits` 的 issue（machopo 沒有 `Audits`）。
6. **Browser 的訊息經過給 renderer 的訊息送到那個 frame，照 Chromium（C1）**（2026-11-28，使用者決定）：`protocol` 給 renderer 一個新的訊息，renderer 記進它的 `Log`（存著、晚開的 session 重播、stderr 照印），`url` 是那個 frame 現在的文件。`X-Frame-Options` 的收在導覽上，那個 frame commit（錯誤頁，或無效時的文件）之後才送（Chromium 的 `AddDeferredConsoleMessage`、`SendDeferredConsoleMessages`）；`frame-ancestors` 的在檢查時送給不准的那個祖先。理由：和 Chromium 一樣，訊息和頁面其他的訊息在同一份 `Log` 裡、先後對得上，拆成多個 process 時照樣成立；renderer 收到 commit 是當場做的，之後到的訊息一定落在新的文件上。代價：`protocol` 多一個訊息。不選放進 `CommitNavigation` 的參數（C2：`frame-ancestors` 的是別的 frame 的，還是要另一個訊息，兩條路做同一件事）。不選由自動化自己組 `Log.entryAdded`（C3：不在 renderer 的 `Log` 裡，晚開的 session 收不到、先後對不上、stderr 不印；Chromium 有個 TODO 往這個方向，crbug.com/40520047，還沒做）。`frame-ancestors` 的訊息帶著被嵌的網站的 policy、送到嵌它的 renderer，Chromium 也這樣（只對 `frame-src` 遮掉網址）；machopo 沒有 OOPIF，本來就是同一個 renderer。`Log` 的訊息加上 `lineNumber`（Chrome 的 `X-Frame-Options` 的是 0）。

## 待決定

沒有（已決定的事項第 1 到 6 條）。

## 之後的可能

- 完整的 CSP（`script-src`、`<meta>`、`securitypolicyviolation`、回報、解析警告）。
- DevTools 的 `Audits` 的 issue（Chrome 的 ORB、CORP、`X-Frame-Options` 都另外報一個）。
- 204、205 的導覽照 Chrome 不 commit（現在 commit 一份空的文件，`docs/limitations.md` 的「iframe」）。
- iframe 的 `sandbox` 和 CSP 的 `sandbox`（路線圖的 F2）。
- Chromium 的「embedding requires opt-in」，Chrome 預設打開時再看。
