# RFC 0128：跨源的回應不交給不該拿的 renderer（ORB 和 CORP）

- 開始日期：2026-11-28
- 狀態：👍 已接受：2026-11-28
- 細節：`details.md`

RFC 0122 路線圖的 N（M1 的第二份）。

## 摘要

`no-cors` 的跨源請求（`<img>`、classic `<script>`、沒有 `crossorigin` 的 stylesheet、`fetch(…, {mode: 'no-cors'})`）的回應，現在不管是什麼內容，都整份交給 renderer。網頁看不到它，但它在 renderer 的記憶體裡：被攻破的 renderer，或用 Spectre 這類旁路讀自己記憶體的頁面，讀得到。這份讓 `net`（network service）在把這種回應交給 renderer 之前先判斷它是不是 renderer 該拿的東西：

- **ORB**（opaque response blocking）：照 Chromium 現在出貨的 ORB v0.1，看 MIME type、`X-Content-Type-Options: nosniff`、`Range`，再 sniff 開頭的 1024 bytes。看起來是 HTML、XML、JSON 的就擋掉，看不出來的放行。
- **`Cross-Origin-Resource-Policy`**（CORP）：伺服器說 `same-origin` 或 `same-site` 的，不交給別的 origin、別的網站的 `no-cors` 請求。
- **請求一定帶發起者**：renderer 送來的請求不帶發起者是壞訊息；走不走 CORS 改成照請求的 `mode`。這是 ORB 和 CORP 的前提（RFC 0127 的已決定的事項第 4 條）。`data:` 網址的 worker 照 Chromium 有自己的 opaque 參數，文件的 loader 收到發起者 `"null"` 的請求也照 opaque 的 context 處理，兩者都不帶 cookie（已決定的事項第 3 條）。

## 動機

- **鎖擋不住的那一塊**：RFC 0127 之後，renderer 鎖在一個最上層的網站，拿不到別的網站的第一方資料。但它照樣可以用 `<img src=https://b.example/secret.json>` 把別的網站的回應要進自己的記憶體。在 machopo 裡，這種跨網站的請求不帶 cookie（第三方的 cookie 一律不送），所以拿到的是沒登入的使用者看得到的東西。剩下的風險有兩種：
  - 靠網路的位置認證的資源：內網的頁面、`localhost` 上的服務、照 IP 放行的 API。不用 cookie，誰在這台機器上都拿得到。
  - 之後放寬第三方 cookie 時（RFC 0122 的已決定的事項第 4 條：L1 靠的是不送第三方 cookie）。
- **伺服器說了不要的沒有照做**：伺服器送 `Cross-Origin-Resource-Policy: same-origin` 時，Chrome 不把它交給別的 origin 的 `no-cors` 請求，machopo 照交。
- **renderer 可以不帶發起者**（`details.md` 的「盤點」）：
  - 正常的 renderer 每個子資源都帶，`no-cors` 的也是；RFC 0127 第 4 條寫的「`no-cors` 的子資源不帶」是誤讀，在這裡更正。
  - 但 Browser 照收不帶的（`check_request`），`net` 又用「有沒有發起者」決定要不要走 CORS（`Cors::new`）。被攻破的 renderer 拿掉發起者，就沒有 CORS、回應是 basic 的，ORB 和 CORP 也看不出它是跨源的。

**目標**

- `no-cors` 的跨源回應裡，看得出是 HTML、XML、JSON 的，不進 renderer 的記憶體；照 Chromium 的 ORB v0.1，看不出的放行。
- CORP 照 Fetch 和 Chromium 檢查 `no-cors` 的請求。
- renderer 的每個請求都帶發起者，`net` 照 `mode` 決定 CORS；opaque 的 context 的請求不帶 cookie，和 Chrome 一樣。
- WPT 的 `fetch/orb/`、`fetch/cross-origin-resource-policy/` 能跑的都跑，結果和 Chrome 對得上。

**不在範圍內**

- **完整的 ORB**（規範的草稿：parse 整份回應看它是不是 JavaScript，看不出來的擋掉）：Chromium 也還沒做（「先例」），已決定的事項第 1 條。
- **COEP**、cross-origin isolated、`SharedArrayBuffer`：CORP 只做沒有 COEP 時的樣子（「CORP」，已決定的事項第 4 條）；它們是 RFC 0122 路線圖的 X。
- **`<video>`、`<audio>`**：machopo 還沒有。ORB 為媒體留的路（媒體的 `Range` 請求、照 URL 記住的媒體）等它們做的時候一起補，這份先照「沒有媒體」寫。
- **`Sec-Fetch-*`**（路線圖的 FM，RFC 0127 的已決定的事項第 5 條）。
- DevTools 的 Network 面板顯示「被 ORB 擋掉」、console 的警告：之後的可能。

要解決的 `docs/limitations.md` 的限制：沒有直接的一條。「隔離」裡沒有寫 ORB；`net` 的 `cors.rs` 的註解寫著「machopo has no opaque response blocking」，做完時改。

## 設計概要

給使用網頁的人：

- `<img>` 指到別的網站的 JSON、HTML，現在就畫不出來，之後也一樣，只是 renderer 拿到的是網路錯誤，不是那份內容。
- `<script src>` 指到別的網站的 JSON（JSONP 以外的 API）：現在 renderer 拿得到內容、執行時語法錯誤；之後是網路錯誤，`onerror` 照樣觸發。
- `fetch(url, {mode: 'no-cors'})` 拿到的照樣是 opaque 的回應，看不到內容。
- 伺服器送 `Cross-Origin-Resource-Policy: same-origin` 的圖片、script，別的網站用不了，和 Chrome 一樣。
- `data:` 網址的 worker 發的請求不再帶 cookie，和 Chrome 一樣（現在帶）。
- 被擋掉時頁面看到的，和 Chrome 一樣：`no-cors` 的 `fetch()` 照樣是 opaque 的回應，`<img>`、`<script>` 是網路錯誤（已決定的事項第 2 條）。

給改 machopo 的人：

- 擋在 `net` 裡，renderer 什麼都不用知道：被擋的回應在 `net` 就換成網路錯誤（或空的回應），內容不送過 process 的邊界。
- renderer 這一側的檢查照舊（classic script 不收 `image/`、`audio/`、`video/`、`text/csv`；opaque 的 script 的錯誤訊息 muted；canvas 的 tainting）：那是網頁看得到的行為，ORB 是另外一層。

## 詳細設計

### 發起者（已決定的事項第 3 條）

- **不帶的是壞訊息**：Browser 的 `check_request` 收到 renderer 的子資源請求 `initiator` 是 `None` 的，是壞訊息（新的 `BadMessageReason`，或擴大 `InitiatorMismatch`）。renderer 在文件不在時（`load_origin` 找不到 document）現在送 `None`，改成不送那個請求。
- **CORS 照 `mode`**：`net` 的 `Cors::new` 不再用「有沒有 `initiator`」決定要不要走 CORS，改成照 `mode`，和 Fetch 的「main fetch」一樣。`initiator` 是 `None` 的只剩 Browser 自己的請求（導覽），那些不走 CORS，也不走 ORB（Chromium 的 `IsOpaqueResponse`：「Browser-initiated requests are never opaque」）。
- **opaque 的 worker 有自己的參數**：`data:` 網址的 worker（和它開的 worker）的 origin 是 opaque 的，現在用的是建它的文件的 loader，所以照那個文件的可信參數帶 cookie，Chrome 不帶（`details.md` 的「opaque 的 context 和 cookie」）。改成照 Chromium 的結構：每個 dedicated worker 有自己的參數（Chromium 的 `CreateNetworkFactoryForSubresources`），origin 是 opaque 的拿 opaque 的可信參數，site for cookies 是空的（Chromium 的 `UpdateIsolationInfoAndStorageAccessForOpaqueOrigin`），最上層的網站照舊（HTTP cache 的分區不變）。Browser 在 `DedicatedWorkerCreated` 時知道它的主 script 的網址和建它的是誰，就知道它是不是 opaque 的；worker 的請求本來就帶 worker 的編號，IO thread 照編號換上它的參數，protocol 不用多一則訊息。
- **文件的 loader 收到 `"null"`**：照收（Chromium 的 `VerifyRequestInitiatorLock`：「Opaque |initiator| is always allowed」，factory 被 opaque 的 frame 沿用時會這樣），但當成 opaque 的 context 發的：可信參數的 site for cookies 換成空的，不帶 cookie。這樣被攻破的 renderer 拿文件的 loader、寫 `"null"`、對信任 `Origin: null` 的伺服器發帶 credentials 的 CORS 請求，拿到的和任何網頁用 `data:` worker 拿到的一樣（沒有 cookie）。`docs/limitations.md` 的「網路」裡「請求的 `initiator` 是 opaque 的（`null`）時照收」那一條跟著改寫。

### ORB

位置：`net` 的 `url_loader`，在回應的 tainting 定下來（`Cors::filter`）之後、交給呼叫的人之前。只看 **renderer 的、`no-cors`、tainting 是 opaque 的** 請求：同源的、CORS 的、Browser 自己的都不看（Chromium 的 `IsOpaqueResponse`）。renderer 的子資源的 body 現在本來就整份收齊才送（`NetworkContext::fetch_with_redirects` 的 `collect`），所以判斷的時候 body 已經在手上，不用另外等。

判斷照 Chromium 的 `orb_impl.cc`（ORB v0.1，已決定的事項第 1 條；`details.md` 的「Chromium 的 ORB v0.1」）：

1. MIME type 是「opaque 的也放行」的（`text/css`、`image/svg+xml`、`audio/*`、`video/*`、`application/ogg`、`application/dash+xml`、`application/vnd.apple.mpegurl`、`text/vtt`）：放行。
2. MIME type 是「從不 sniff」的（`application/pdf`、`application/zip`、`text/event-stream`、`text/csv` 這類，清單照 Chromium 的 `orb_mimetypes.cc`）：擋。
3. HTML、JSON、`text/plain`、XML，又有 `nosniff`：擋。
4. `206` 而且不是從頭開始（`Content-Range` 的第一個 byte 大於 0）：擋。Chromium 對之前記住是媒體的 URL 放行，machopo 沒有媒體，一律擋。
5. sniff 開頭的 1024 bytes（`net` 的 `SNIFF_LENGTH`）：
   - 是圖片、音訊、影片的特徵：放行；
   - 是 HTML（`<!doctype html`、`<script`、`<html` 這類，`<!--` 不算）、XML（`<?xml`）、JSON（`{"…":` 的開頭，或 `)]}'`、`{}&&`、`for(;;);` 這類 JavaScript 跑不了的前綴）：擋；
   - 都不是：放行（Chromium 的「fail open」，規範的完整 ORB 在這裡 parse JavaScript）。

sniff 要的 HTML、XML、JSON 的特徵是新寫的（`net` 的 `mime::sniff` 現在只分 `<` 開頭和文字），照 Chromium 的 `orb_sniffers.cc`。圖片的特徵（PNG、JPEG、GIF、WebP……）照 `mime sniff` 規範的「image type pattern」。

被擋的回應（已決定的事項第 2 條）：destination 是空的（`fetch()`）換成空的回應，其他的換成網路錯誤。

### CORP

位置和 ORB 一樣，但每一跳都看（Fetch 的「HTTP fetch」：每一個回應，包括 redirect）。只看 **`no-cors` 的** 請求（Chromium 的 `CrossOriginResourcePolicy::IsBlocked`：「If request's mode is not "no-cors", then return allowed」）。

- header 照 Fetch 的語法，只認大小寫完全相同的 `same-origin`、`same-site`、`cross-origin`，其他的值等於沒有。
- `same-origin`：發起者和回應的網址同 origin 才放行。
- `same-site`：同一個網站（schemeless 的 same site），而且發起者是 `https` 或回應不是 `https`。IP 位址照 Chromium 不算同一個網站（「Different sites might be served from the same IP address」）。
- 沒有 header、`cross-origin`：放行。
- 被擋：網路錯誤。
- **沒有 COEP**（已決定的事項第 4 條）：COEP 是讓「沒有 header」變成 `same-origin` 的那一半，machopo 沒有 COEP，所以 embedder policy 一律是 `unsafe-none`；判斷寫成 embedder policy 是一個輸入，路線圖的 X 接上它。iframe 的導覽只有 COEP 時才檢查 CORP（Fetch 的 `forNavigation`），所以導覽不檢查。

### 被擋掉之後

- `net` 記一筆 log（URL、為什麼擋、哪一條規則），DevTools 的 Network 和 console 之後再說（「之後的可能」）。
- renderer 收到的和網路錯誤一樣（或空的回應），它不知道是被擋的：和 Chromium 一樣，不讓頁面從錯誤的種類分辨出內容是什麼。

### 測試

- `net` 的單元測試：每一條 ORB 的規則（MIME type 的清單、`nosniff`、`206`、每一種 sniff 的特徵、fail open）、CORP 的每一種值和 `same-site` 的 scheme、IP 位址。
- WPT：`fetch/orb/tentative/`、`fetch/cross-origin-resource-policy/`、`fetch/corb/` 能跑的加進 `tests/wpt/include.txt`（要 `<video>`、COEP、HTTPS 的另外看），預期結果照 Chrome。
- 被攻破的 renderer：不帶發起者的請求是壞訊息（`rogue_on_commit` 加一個角色）；拿文件的 loader 寫 `"null"` 的 CORS 請求不帶 cookie。
- `data:` worker 帶 credentials 的請求不帶 cookie、`Origin` 是 `null`（照 Chrome 實測的）。
- 整合測試：跨網站的 `<img>`、classic `<script>` 拿到 JSON、HTML 時是網路錯誤，CSS、圖片、JavaScript 照常；`no-cors` 的 `fetch()` 照樣 resolve 成 opaque 的回應。
- mutation test：每一條規則。

### 分階段

兩段（已決定的事項第 5 條），每一段做完都可以 merge 回 `main`：

1. **發起者和 CORP**：`None` 是壞訊息、CORS 照 `mode`、`data:` worker 有自己的參數、文件的 loader 收到 `"null"` 不帶 cookie（第 3 條）；CORP（第 4 條）；被攻破的 renderer 的角色；`fetch/cross-origin-resource-policy/` 的 WPT。
2. **ORB**：MIME type 的清單、`nosniff`、`206`、sniffer、被擋時的回應（第 1、2 條）；`fetch/orb/`、`fetch/corb/` 的 WPT；收尾。

## 缺點

- **ORB v0.1 是 fail open 的**：看不出是什麼的照樣交給 renderer（例如沒有 `Content-Type`、開頭不是 HTML、JSON 特徵的私密文字）。Chromium 也是，完整的 ORB 要 parse JavaScript，太慢也太難（規範的作者自己說「infeasible」）。
- **相容性**：伺服器把 JavaScript 標成 `text/html`、`application/json` 時，跨網站的 `<script>` 會壞掉。Chromium 從 CORB（2018）到 ORB 都這樣擋，網站大多早就改了，machopo 照 Chromium，壞的範圍和 Chrome 一樣。
- 多一層每個跨源 `no-cors` 請求都要走的檢查：sniff 最多 1024 bytes，body 本來就在記憶體裡，花費很小。

## 理由和其他做法

- **不做的話**：鎖住的 renderer 照樣把別的網站沒有 cookie 的回應整份要進自己的記憶體，內網的資源也是。
- **在 renderer 裡擋**：沒有意義，被攻破的 renderer 不會照做。一定要在 Browser 這一側（`net`）。
- 各題的選項在「待決定」。

## 先例

`details.md` 的「Chromium」「Fetch 的規範」：

- **Chromium**：2018 年出貨 CORB，之後換成 ORB v0.1（`services/network/public/cpp/orb/`）：MIME type 的清單加上 sniff 開頭 1 KiB，看不出的放行，跳過規範 ORB 的第 10、11、13、15 步（「Skipping these steps minimizes the risk of shipping the initial ORB implementation」）。只看有發起者的 `no-cors` 請求。被擋的：`fetch()` 拿到空的回應，其他的拿到網路錯誤（`ShouldHandleBlockedResponseAs`）。CORP 在 `services/network/public/cpp/cross_origin_resource_policy.cc`，只看 `no-cors`。
- **Fetch 的規範**：ORB 的草稿（whatwg/fetch PR #1442，`https://github.com/annevk/orb`）在讀到的 living standard 裡還沒有；規範裡有的是比較簡單的「依 MIME type 擋」「依 `nosniff` 擋」（script 類的 destination 不收 `audio/`、`image/`、`video/`、`text/csv`，machopo 的 renderer 已經照做）。CORP 在規範裡（「cross-origin resource policy check」）。
- **Firefox**：照記憶有 ORB（沒有對文件）。

## 已決定的事項

1. **ORB 照 Chromium 的 ORB v0.1（O1）**（2026-11-28，使用者決定）：MIME type 的清單（opaque 也放行的、從不 sniff 的）、`nosniff`、從中間開始的 `206`，再 sniff 開頭的 1024 bytes：HTML、XML、JSON 和 JavaScript 跑不了的前綴擋掉，看不出的放行（fail open），照 `orb_impl.cc` 和 `orb_sniffers.cc`。理由：行為和 Chrome 一樣，WPT 的預期結果照 Chrome；只看開頭 1 KiB，renderer 的子資源的 body 在 `net` 裡本來就收齊了，花費很小；Chromium 跳過完整 ORB 的那幾步是為了相容性的風險，machopo 照網頁對 Chrome 的樣子寫，風險一樣。代價：看不出是什麼的照樣交給 renderer（沒有 `Content-Type`、開頭不像 HTML 或 JSON 的私密文字）。不選規範草稿的完整 ORB（O2：要 parse 整份 body 看是不是 JavaScript，每個跨源的 script 多 parse 一次，Chromium 還沒出貨，規範的作者自己說部分 parse「infeasible」，PR 還沒進 living standard）。不選只做 CORB（O3：Chromium 已經換成 ORB）。
2. **被擋的回應照 Chromium：`fetch()` 拿到空的回應，其他的拿到網路錯誤（E1）**（2026-11-28，使用者決定）：destination 是空的（`fetch()`）時換成空的回應，其他的（`<img>`、`<script>`、stylesheet、`importScripts()`）換成網路錯誤，內容都不送過 process 的邊界（Chromium 的 `ShouldHandleBlockedResponseAs`；它的 `README.md` 寫「一律空的回應」，和程式不同，照程式）。理由：和 Chrome 一樣；opaque 的 `fetch()` 本來就丟掉 body，`fetch(…, {mode: 'no-cors'})` 照樣 resolve 成 opaque 的回應，頁面分不出它被擋了，不洩漏「這個網址回的是 HTML 或 JSON」；`<img>`、`<script>` 照樣觸發 `onerror`。不選一律網路錯誤（E2，規範草稿：`no-cors` 的 `fetch()` 會 reject，和 Chrome 不同，頁面也能從 resolve 或 reject 分辨出被擋了）。
3. **發起者：不帶的是壞訊息；opaque 的 worker 有自己的參數；文件的 loader 收到 `"null"` 照收但不帶 cookie（A）**（2026-11-28，使用者決定；這一題的看法改過三次，最後讀了 Chromium 的原始碼、實測 Chrome 才定）：`None` 是壞訊息、CORS 照 `mode`；`data:` 網址的 worker 和它開的 worker 拿 opaque 的可信參數（site for cookies 是空的）；文件的 loader 收到 `"null"` 照收，site for cookies 換成空的（「發起者」）。理由：
   - 被攻破的 renderer 能用 `"null"` 做的一件事：拿文件的 loader 對同一個網站裡別的 origin 發帶 credentials 的 CORS 請求，`Origin` 是 `null`、cookie 照 loader 帶，信任 `Origin: null` 的伺服器（有名的 CORS 錯誤設定）就讓它讀。跨網站的本來就沒有 cookie。
   - Chrome 對 `data:` worker 帶 credentials 的請求不帶 cookie（2026-11-28 用 Chromium 156 實測，`SameSite=Lax`、`Strict` 和沒標的都沒帶，`Origin` 是 `null`）；HTML 的「has cross-site ancestor」對 `data:` worker 回答 true。machopo 照程式看會帶。
   - Chromium 的結構：每個 dedicated worker 有自己的 factory，origin 是 opaque 時 `SiteForCookies` 是空的；factory 的鎖對 opaque 的發起者一律放行、不核對 precursor（`VerifyRequestInitiatorLock` 的 TODO）。所以照收 `"null"` 和 Chromium 一樣，不會誤殺合法的 renderer；不帶 cookie 擋住上面那一招，結果和 opaque 的 worker 一樣。

   不選只擋 `None`（I1：那一招留著，`data:` worker 照舊帶 cookie、和 Chrome 不同）。不選核對 `"null"` 有沒有 opaque 的 context 用著那個 loader（I2：攻擊者自己建一個 `data:` worker 就過了；原本說「這是 Chromium 的做法」是錯的，Chromium 不核對 precursor）。不選文件的 loader 收到 `"null"` 是壞訊息（B：比 Chromium 嚴，之後做 iframe 的 `sandbox` 時可能有沿用 loader 的 opaque 文件，會誤殺）。不選照收也照帶 cookie（C：那一招留著）。沒讀到、不影響這一條的：Chromium 的 network service 用哪一份 site for cookies 決定 cookie（`url_loader.cc`），`data:` worker 的 storage key 在 Chromium 裡是不是 opaque 的。
4. **只做 CORP，embedder policy 一律 `unsafe-none`；COEP 在路線圖的 X（C1）**（2026-11-28，使用者決定）：只看伺服器明確送的 `Cross-Origin-Resource-Policy`，沒有 header 的放行，iframe 的導覽不查；CORP 的判斷寫成 embedder policy 是一個輸入（Chromium 的 `cross_origin_resource_policy.cc` 也是同一個函式，COEP 只把「沒有」提升成 `same-origin`）。COEP 和 cross-origin isolated、`SharedArrayBuffer` 一起是 RFC 0122 路線圖新加的 X，放在 M3：它改 `SiteInfo` 和鎖（Chromium 把是不是 cross-origin isolated 放進 process lock 的 key），依賴 W 的 COOP 和這份的 CORP。理由：效果和 Chrome 對沒送 COEP 的頁面一樣，網頁絕大多數沒送；COEP 是網頁為了拿 cross-origin isolated 才送的，machopo 兩個都沒有，只做 COEP 沒有網頁看得到的用處。不選連 COEP 一起做（C2：要跟著文件和 worker 走、report-only 和回報，範圍大很多）。
5. **分兩段：先發起者和 CORP，再 ORB（G1）**（2026-11-28，使用者決定）：照「分階段」。理由：順序是依賴定的：ORB 要發起者可信才擋得住，發起者一定在前面；第 1 段小，又修掉一個和 Chrome 不同的地方（`data:` worker 帶 cookie），做完就能 merge；ORB 的 sniffer 和清單最大，`fetch/orb/` 的 WPT 要逐一看能不能跑，單獨驗收。代價：階段的檢查跑兩次。不選一段做完（G2：發起者的修正要等 ORB 才進 `main`）。不選三段（G3：CORP 只是一個 header 的解析和判斷，單獨一段不值得多跑一次階段的檢查）。

## 待決定

沒有（已決定的事項第 1 到 5 條）。

## 之後的可能

- 完整的 ORB（parse JavaScript），等規範和 Chromium 定下來。
- `<video>`、`<audio>` 做的時候，補 ORB 為媒體留的路（照 URL 記住的媒體、`Range` 的後續請求）。
- COEP、cross-origin isolated、`SharedArrayBuffer`（路線圖的 X）。
- DevTools 的 Network 顯示被 ORB、CORP 擋掉的請求，console 的警告（Chrome 的「Response was blocked by ORB」）。
