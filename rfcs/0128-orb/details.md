# RFC 0128 的細節：跨源的回應不交給不該拿的 renderer（ORB 和 CORP）

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-28 讀的（`main`，`2b284fe2`）。

### 請求

- `protocol::network::ResourceRequest`：`destination` 只有 `Document`、`Iframe`、`Style`、`Image`、`Script`、`Worker`、`Empty`（沒有字型、媒體、`object`）；`mode` 是 `SameOrigin`、`NoCors`、`Cors`（沒有 `Navigate`）；`initiator` 是發起者的 origin（序列化，opaque 的是 `null`）；`trusted` 是 Browser 填的可信參數。
- renderer 的子資源，`mode`、`destination`、`initiator`：

  | 從哪裡 | `mode` | `destination` | `initiator` |
  | --- | --- | --- | --- |
  | `<img>`、CSS 的 `background-image` | `NoCors`（`crossorigin` 反映了但不用） | `Image` | 文件的 origin |
  | classic `<script>` | `NoCors`，有 `crossorigin` 時 `Cors` | `Script` | 文件的 origin |
  | module script、`modulepreload` | `Cors` | `Script` | 文件的 origin |
  | `<link rel=stylesheet>` | `NoCors`，有 `crossorigin` 時 `Cors` | `Style` | 文件的 origin |
  | `fetch()` | 照 `mode`（`navigate` 當 `cors`） | `Empty` | `fetch()` 的 origin |
  | worker 的主 script | `SameOrigin` | `Worker` | 建它的文件的 origin |
  | `importScripts()` | `NoCors` | `Script` | worker 的 origin |

  文件的 origin 由 `dom_api` 的 `load_origin` 給：opaque 的是 `null`，找不到文件時是 `None`（這時請求照樣送）。
- 沒有的：`<video>`、`<audio>`、`<object>`、`<embed>`、`XMLHttpRequest`、`sendBeacon`、`prefetch`、`@font-face`、`@import`。
- 導覽由 Browser 載入（`Document`、`Iframe`），`initiator` 是 `None`（`trusted` 裡有發起的 frame 的 origin）；renderer 要文件是壞訊息（`DocumentRequest`）。
- **Browser 照收不帶發起者的請求**：`render_process_host.rs` 的 `check_request` 收 `None`、`"null"` 和 loader 的 origin。**`net` 照有沒有發起者決定 CORS**：`cors.rs` 的 `Cors::new` 在 `initiator` 是 `None` 時回答 `None`，`url_loader::load` 就跳過 tainting、`Origin`、preflight、檢查和過濾，tainting 留在 basic。

### 回應

- `url_loader::load`：每一跳 `Cors::start`（tainting：同源和 `data:` 是 basic，`NoCors` 是 opaque，`Cors` 是 cors）、cookie、`Origin` 和 preflight、HTTP cache、`cors.check`、redirect（跨源的 redirect 標成 tainted）。
- `Cors::filter` 只設 `head.tainting`；opaque 而且 destination 是 `Empty`（`no-cors` 的 `fetch()`）時丟掉 body。其他 opaque 的（圖片、script、stylesheet）body、header、status 整份留著，交給 renderer。註解寫著「machopo has no opaque response blocking to keep a renderer from it otherwise」。
- renderer 的子資源走 `NetworkContext::fetch_with_redirects`，`collect` 把 body 整份收齊，一則訊息送給 renderer（不串流）。導覽才用 data pipe 串流（`stream_body`）。
- sniff：`respond` 在沒有 `Content-Type` 時讀開頭的 `SNIFF_LENGTH`（1024）bytes，`mime::sniff` 只分 `<` 開頭（`text/html`）和其他（`text/plain`）。沒有 `X-Content-Type-Options: nosniff` 的處理（renderer 只對 worker 的 script 看它）。
- `Range`：script 能在 `fetch()` 裡設；`blob:` 回答 `206`；HTTP 的 `206` 經過 cache。
- 搜不到 `Cross-Origin-Resource-Policy`、CORB、ORB、`Sec-Fetch`、`Cross-Origin-Embedder-Policy`。

### renderer 這一側對 opaque 的回應

- `fetch()`：opaque 的回應是 status 0、沒有網址和 header（`dom_api` 的 `fetch.rs`）。
- classic script：`muted`（V8 的 `ScriptOrigin` 的 shared cross-origin），`importScripts()` 也是。
- 圖片：`origin_clean`，`drawImage` 讓 canvas tainted。
- stylesheet：`origin_clean = !opaque`，`cssRules` 丟 `SecurityError`。
- classic script 不收 `image/`、`audio/`、`video/`、`text/csv`（`subresource.rs`，Fetch 的「依 MIME type 擋」）；沒有對 `<script>` 的 `nosniff` 檢查。
- 圖片的快取照 URL，請求帶著第一個要它的文件的 origin（`dom` 的 `image.rs`）：同一個 renderer 裡別的文件拿到的是同一份。

### WPT

- `tests/wpt/include.txt` 收了 `fetch/corb/` 的 5 個 SVG 的 reftest（`img-svg-*.sub.html`）、`fetch/api/cors/`、`fetch/range/blob.any.js`；沒有 `fetch/orb/`、`fetch/cross-origin-resource-policy/`，也沒有註解說為什麼。
- `fetch/orb/tentative/`：`compressed-image-sniffing.sub.html`、`content-range.sub.any.js`、`hls.html`、`img-mime-types-coverage.tentative.sub.html`、`img-png-mislabeled-as-html.sub.html`、`img-png-unlabeled.sub.html`、`known-mime-type.sub.any.js`、`nosniff.sub.any.js`、`ping.optional.html`、`script-js-unlabeled-gzipped.sub.html`、`script-unlabeled.sub.html`、`script-utf16-without-bom-hint-charset.sub.html`、`status.sub.any.js`、`status.sub.html`、`unknown-mime-type.sub.any.js`。
- `fetch/cross-origin-resource-policy/`：`fetch.any.js`、`fetch.https.any.js`、`fetch-in-iframe.html`、`iframe-loads.html`、`image-loads.html`、`scheme-restriction.any.js`、`scheme-restriction.https.window.js`、`script-loads.html`、`syntax.any.js`。
- 哪些跑得起來（HTTPS、`<video>`、COEP、`sendBeacon`）要開始做時逐一看。

## opaque 的 context 和 cookie

已決定的事項第 3 條的依據。

- **machopo**：`data:` 網址的 worker 的 origin 是 opaque 的（`renderer` 的 `worker_thread.rs`：「a `data:` URL's worker is of an opaque origin」），用的是建它的文件的 loader（worker 的 loader 是建它的文件或 worker 的），所以它的請求是「發起者 `"null"`、loader 是 tuple origin」，照那個 loader 的可信參數帶 cookie。其他 opaque 的文件（`data:` 的 iframe、錯誤頁）commit 時有自己的 loader；`javascript:` 網址的文件的 origin 是它的 iframe 的文件的，不是 opaque 的。
- **Chrome 實測**（2026-11-28，Chromium 156.0.8078.79，Fedora 的 build，headless）：`http://127.0.0.1` 的頁面設了 `lax=1; SameSite=Lax`、`strict=1; SameSite=Strict`、`plain=1`，伺服器對 CORS 請求回 `Access-Control-Allow-Origin` 照收到的 `Origin`、加 `Access-Control-Allow-Credentials: true`：

  | 請求（`credentials: 'include'`、`mode: 'cors'`） | `Origin` | `Cookie` |
  | --- | --- | --- |
  | 頁面自己的 `fetch`（同源） | 沒有 | `lax=1; strict=1; plain=1` |
  | `data:` worker 的 `fetch`（同一個網址） | `null` | 沒有 |

- **HTML**：「has cross-site ancestor」對 `data:` 網址的 worker 回答 true；dedicated worker 的 top-level origin 是建它的 settings object 的。WPT 的 `workers/data-url.html` 只確認它的 origin 是 opaque 的、是跨源的，沒有測 cookie。
- **Chromium 的原始碼**（2026-11-28，`chromium.googlesource.com` 的 `main`）：
  - `content/browser/worker_host/dedicated_worker_host.cc`：`DidStartScriptLoad` 用 `CreateNetworkFactoryForSubresources` 給 worker 建自己的 factory，參數用 `URLLoaderFactoryParamsHelper::CreateForFrame` 另外算（傳入建它的那個的 origin 和 worker 的 `isolation_info_`）。worker 的 storage key 是 opaque 時，`UpdateIsolationInfoAndStorageAccessForOpaqueOrigin` 把 `isolation_info_` 換成最上層的 origin 不變、frame 的 origin 是那個 opaque 的、`net::SiteForCookies()` 是空的，請求另外標 `require_cross_site_request_for_cookies = true`。
  - `dedicated_worker_host_factory_impl.cc`：`data:` 網址的 worker 的 origin 照 `IsDataUrlInWebWorkerOpaqueOriginEnabled` 算（註解：storage key 的 origin，「except in the case of data: URL workers」）；建它的是 opaque 的時，script 的 origin 照 precursor 比。
  - `services/network/public/cpp/initiator_lock_compatibility.cc` 的 `VerifyRequestInitiatorLock`：發起者等於鎖，`kCompatibleLock`；發起者是 opaque 的，也是 `kCompatibleLock`（「Opaque |initiator| is always allowed. In particular, a factory locked to a non-opaque |lock| may be used by an opaque |initiator| - for example when the factory is inherited by a data: URL frame」），precursor 不比（上面一行的 TODO）；其他的 `kIncorrectLock`。
  - 沒讀到：`URLLoaderFactoryParamsHelper::CreateForFrame` 的內容、`CalculateWorkerStorageKey`、network service 怎麼用 site for cookies 決定 cookie（`url_loader.cc`）。
- **草稿時說錯的**：曾經說「Chromium 會核對 opaque 的發起者的 precursor」，所以把 I2 當成 Chromium 的做法。讀了 `VerifyRequestInitiatorLock` 之後確認它不核對。

## Chromium 的 ORB v0.1

2026-11-28 讀的，Chromium 的 `main`（GitHub 的鏡像）：`services/network/public/cpp/orb/orb_impl.cc`、`orb_sniffers.cc`、`README.md`。`orb_impl.h`、`orb_mimetypes.cc` 沒讀到（429）。

- **只看什麼**（`IsOpaqueResponse`）：「ORB only applies to "no-cors" requests」；沒有發起者的不看（「Browser-initiated requests are never opaque」）；經過 service worker 的只看 opaque 和 opaque-redirect 的。發起者和網址同源的放行。
- **判斷**（`Init`，註解照 ORB 的步驟編號）：
  1. 「opaque 也放行」的 MIME type（`text/css`、`image/svg+xml`、`audio/*`、`video/*`、`application/ogg`、`application/dash+xml`、`application/vnd.apple.mpegurl`、`text/vtt`）：放行。JavaScript 不在這裡，最後才放行。
  2. 從不 sniff 的：擋（`kNeverSniffedMimeType`）。
  3. HTML、JSON、`text/plain`、XML 加 `nosniff`：擋（`kNoSniffHeader`）。規範的第 iii 步（`206` 加 HTML、JSON、XML）省掉了。
  4. `206` 而且從中間開始（`Content-Range` 的第一個 byte 大於 0）：之前記住是媒體的 URL 放行，其他的擋（`kUnexpectedRangeResponse`）。
  5. 其他：再 sniff。
- **sniff**（`Sniff`）：`net::SniffMimeTypeFromLocalData` 是音訊、影片的，記住這個 URL、放行；圖片的、什麼都看不出的放行；`SniffForHTML`、`SniffForXML`、`SniffForFetchOnlyResource` 是的擋。開頭 1 KiB（`README.md`：「first 1KiB」）。
- **body 結束了還看不出**（`HandleEndOfSniffableResponseBody`）：一律放行，註解：「Changing step 16 to fail open」。跳過的步驟：10（`nosniff`）、11（不是 ok 的 status）、13（音訊、圖片、影片的 MIME type）、15（parse JavaScript），「Skipping these steps minimizes the risk of shipping the initial ORB implementation」。
- **被擋的給什麼**（`ShouldHandleBlockedResponseAs`）：renderer 給的 destination 不是空的，給網路錯誤；是空的（`fetch()`），給空的回應。`README.md` 寫「an empty response will be injected」，和程式不同，以程式為準。
- **sniffer**（`orb_sniffers.cc`）：
  - HTML：`<!doctype html`、`<script`、`<html`、`<head`、`<iframe`、`<h1`、`<div`、`<font`、`<table`、`<a`、`<style`、`<title`、`<b`、`<p`；不含 `<!--`（「can weaken our ORB policy」，但會誤判 JavaScript）。
  - XML：去掉空白之後 `<?xml`，分大小寫。
  - JSON：`{`、一個雙引號的字串、`:`（狀態機）。
  - JavaScript 跑不了的前綴（`kScriptBreakingPrefixes`，分大小寫）：`)]}'`、`{}&&`、`{} &&`、`for(;;);`、`while(1);`、`for (;;);`、`while (1);`。
  - `kOrbSniffersInRust`：Chromium 有一份 Rust 的 sniffer，用 feature 切換。
- `README.md` 的表：音訊、影片的 MIME type，v0.1 放行、完整的 ORB 擋（除非 sniff 對）；JSON 和 JavaScript，v0.1 只看開頭 1 KiB 的特徵，完整的 parse；其他的，v0.1 放行、完整的擋。「It will accept any response for which these heuristics do not deliver a verdict.」

## CORP

- **Fetch**：header 是 `same-origin`、`same-site`、`cross-origin`，分大小寫，其他的等於沒有。「cross-origin resource policy internal check」：`same-origin` 要同源；`same-site` 要 schemelessly same site，而且發起者是 `https` 或回應不是 `https`；沒有的照 embedder policy（`unsafe-none` 放行，`require-corp` 當成 `same-origin`，`credentialless` 在帶 credentials 或導覽時當成 `same-origin`）。在「HTTP fetch」裡，「If either request's response tainting or response's type is "opaque", and the cross-origin resource policy check ... returns blocked, then return a network error」。`forNavigation` 只有巢狀的導覽、而且 embedder policy 不是 `unsafe-none` 時才擋。
- **Chromium**（`cross_origin_resource_policy.cc`）：`IsBlocked`：「If request's mode is not "no-cors", then return allowed」，沒有發起者的放行；`same-site` 用 `ShouldAllowSameSite`：IP 位址不算同一個網站（「Different sites might be served from the same IP address - they should still be considered to be different sites」），其他照 `SameDomainOrHost` 和 scheme 的規則。COEP 和 Chromium 自己的 Document-Isolation-Policy 會把「沒有」提升成 `same-origin`。`IsNavigationBlocked` 只在有 COEP 時擋。

## Fetch 的規範裡的 ORB

- living standard（`whatwg/fetch` 的 `fetch.bs`，`main`）讀到的部分沒有 ORB 的那一節。有的是「should response to request be blocked due to MIME type?」（script 類的 destination 不收 `audio/`、`image/`、`video/`、`text/csv`）和「… due to nosniff?」（script 類的不是 JavaScript、`style` 的不是 `text/css`），在「main fetch」裡，被擋的是網路錯誤。
- ORB 的草稿是 PR #1442（預覽 `https://whatpr.org/fetch/1442.html`，2022-10-27 更新），有「opaque-response-safelist check」和新的 MIME type 的集合。`https://github.com/annevk/orb` 的說明寫它「being upstreamed to the Fetch Standard through PR #1442」，最後一步是 parse 整份 body 看是不是 JavaScript、不是 JSON，並說部分 parse「infeasible」。PR 合併了沒有，沒有查到。

## 參考資料

- [ORB 的說明（annevk/orb）](https://github.com/annevk/orb)
- [Fetch PR #1442 的預覽](https://whatpr.org/fetch/1442.html)
- [Fetch Standard](https://fetch.spec.whatwg.org/)
- Chromium 的原始碼：`services/network/public/cpp/orb/orb_impl.cc`、`orb_sniffers.cc`、`README.md`，`services/network/public/cpp/cross_origin_resource_policy.cc`
