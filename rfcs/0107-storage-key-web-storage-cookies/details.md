# RFC 0107 的細節：持久資料的 key，Web Storage 和 cookie

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## machopo 現在的狀況

2026-11-20 讀程式（`main` 的 `e2d10d6`）。

- **HTTP**：reqwest 的 client 在 `crates/net/src/http.rs` 的 `HttpTransport::new`，`redirect(Policy::none())`，workspace 的 `reqwest` 只開 `rustls`、`http2`（沒有 `cookies`）。`HttpTransport::send` 收的是每一跳的 header 清單，不是 `ResourceRequest`。redirect 的迴圈在 `crates/net/src/url_loader.rs` 的 `load`：每一跳複製 header、加 `Origin`、需要時先送 preflight、送出、看要不要跟；跨 origin 的一跳丟掉 `Authorization`。每一跳的回應的 header（包括 `Set-Cookie`）在 `split()` 之前都在手上：`Cookie` 加在那一跳要送的 header、`Set-Cookie` 從那一跳的回應讀，位置都在這個迴圈裡。
- **`Set-Cookie` 不到 renderer**：`url_loader.rs` 的 `split()` 在建 `ResponseHead` 之前拿掉 `set-cookie`、`set-cookie2`（導覽和 redirect 的 head 也是）；`protocol` 的 `header_list.rs` 有 `is_forbidden_response_header_name`，`dom_api` 的 `fetch.rs`、`headers.rs` 也擋。forbidden 的 request header 有 `cookie`。
- **`CredentialsMode`**（`crates/protocol/src/network.rs`）：`Omit`、`SameOrigin`、`Include`，預設 `Include`。只有 `net` 的 `cors.rs` 讀它（preflight 的 `*`、`Access-Control-Allow-Credentials`、露出的 header）。誰決定：`fetch()` 照 `Request` 的值（`renderer` 的 `loader.rs`）；CORS 的 script 照 `crossorigin`；`<link>` 的 sheet 照 `crossorigin`；圖片是預設的 `NoCors` 加 `Include`；worker 的各種載入在 `worker_thread.rs`；導覽是 `ResourceRequest::new` 的預設（`browser` 的 `web_contents.rs`）。
- **請求的來歷**：`ResourceRequest` 的欄位是 `url`、`method`、`destination`、`headers`、`initiator`（origin 的字串，opaque 的是 `null`，Browser 的請求沒有）、`mode`、`credentials`、`body`、`redirect`。沒有最上層的網址或網站、site for cookies、frame、referrer。`RendererHostMsg::FetchResource` 只帶 `request_id`、`worker`、`request`；Browser 的 IO thread（`render_process_host.rs`）只檢查 destination 不是文件、worker 是它的、`file:` 和 blob，不核對 `initiator`。renderer 填 `initiator` 用文件的 origin（圖片用最上層文件的）。
- **導覽**：`start_navigation` 知道 frame、是不是 root、網址、發起的 frame 和整棵 frame 樹，但請求只帶網址和 destination；`NavigationRequest` 不存 origin 和發起者。
- **frame 樹**（`crates/browser/src/frame_tree.rs`）：每個節點有 `parent`、`children`、`url`、`origin: url::Origin`（commit 時設）、`local`。有 `root()`、`parent()`、`origin(frame)`，沒有往上走到 root 的 helper，也沒有「網站」。
- **網站**：整個 workspace 沒有 schemeful site、registrable domain、public suffix 的程式；`renderer` 的 `page_agent.rs` 的 `domainAndRegistry` 寫死成空字串。
- **同步的呼叫**：`RendererHostMsg` 沒有帶回答的 channel 的訊息，都是經過 `crossbeam_channel` 送了就走。唯一會等的是 worker：`worker_thread.rs` 的 `GlobalHost::load` 送出 `FetchResource` 之後在自己的 inbox 上等對應的 `ResourceResponse`，中間到的其他訊息先放著（`importScripts`，Blink 的 `WorkerClassicScriptLoader::LoadSynchronously`）。main thread 從來不等。
- **最像的現有路徑**：blob URL。`URL.createObjectURL` 經過 `dom_api` 的 `LoadSink::register_blob_url`（renderer 的 `RendererLoadSink` 實作）送 `RendererHostMsg::RegisterBlobUrl`，Browser 的 IO thread 交給 `NetworkContext::register_blob_url`，存在 `net` 的 `BlobUrlRegistry`（照登記它的 renderer，不是照 origin）。DOM 要 renderer 做事的介面是 `LoadSink`（`dom_api/src/engine.rs`）和 worker 的 `WorkerGlobalHost`。
- **DOM**：`navigator.cookieEnabled` 回 `false`（`dom_api/src/navigator.rs`）；`Window` 和 `<body>` 有 `onstorage`，事件名稱在 `dom/event_type_names.txt`。沒有 `document.cookie`、`localStorage`、`sessionStorage`、`Storage`、`StorageEvent`。origin 從文件的網址算（`dom_api::origin`、`Cx::context_origin`），opaque 用 `url::Origin::is_tuple()` 判斷。
- **分頁**：`TabId(u32)`；每個分頁一個 renderer（`WebContents` 的 `process`），當掉時重開；分頁不共用 renderer。`NetworkContext` 在 `shell` 的 `BrowserWindow::start` 建一次、clone 給每個分頁（headless 每次執行一個）。沒有 profile 的物件。
- **磁碟**：沒有每個使用者的資料夾，不寫持久的檔案（只有 trace、census、截圖這些輸出）。`machopo` 的選項沒有 `--user-data-dir`。
- **自動化**：machopodriver 沒有 cookie 的程式，指令落到 `machopodriver doesn't do … yet`（`unknown command`）。CDP 的 `Network.getCookies`、`getAllCookies` 回空的陣列，`clearBrowserCookies` 回成功（`shell` 的 `automation_session.rs` 的 `network_answer`）；`Network.setCookie`、`deleteCookies`、`Storage.*`、`DOMStorage.*` 回找不到方法。
- **依賴**：workspace 沒有 `cookie`、`cookie_store`、`publicsuffix`、`psl`、`rusqlite`、`dirs` 這些；`cookie` 0.16 只被 `webdriver` crate 帶進來。toolchain 是 1.99.0。
- **WPT**：`webstorage/` 有 54 份自動的測試；`cookies/` 約 89 份（`samesite/` 22、`prefix/` 12、`attributes/` 10、`partitioned-cookies/` 7、`secure/` 6……）。`tests/wpt/include.txt` 都還沒列。已經在清單裡、因為沒有 cookie 而失敗或關掉的：`navigatorcookies-cookieenabled-true.html`、`fetch/api/cors/cors-cookies*.any.js`、`fetch/api/headers/header-setcookie.any.js`、`workers/same-site-cookies/`。

## 其他實作

### Chromium

2026-11-20 讀原始碼（`main`）的：

- **`net::SchemefulSite`**（`net/base/schemeful_site.h`）：照 HTML 的「obtain a site」。opaque 的 origin 各自不同；有網路 host 的 scheme 相同、而且 hostname 或 registrable domain 相同就是同一個；沒有網路 host 的 scheme 看 scheme 和 host；只差 port 的是同一個。
- **`blink::StorageKey`**（`third_party/blink/public/common/storage_key/storage_key.h`）：`origin_`、`top_level_site_`、`nonce_`（選用）、`ancestor_chain_bit_`。開著第三方分區時，同一個 origin 嵌在不同的最上層網站底下是不同的 key；origin 不在最上層的網站裡、或 ancestor chain bit 是 `kCrossSite`（`a.com` 嵌在 `b.com` 裡、`b.com` 又嵌在 `a.com` 裡的那個內層）就是第三方的 key。有 nonce 的 key 不看最上層的網站。opaque 的 origin 的 key 不能序列化。`SerializeForLocalStorage` 是 `localStorage` 用的序列化。
- **Web Storage**（`third_party/blink/public/mojom/dom_storage/storage_area.mojom`）：`StorageArea` 的 `GetAll` 是 `[Sync]`，一次給整個 area、同時掛上 observer；`Put`、`Delete`、`DeleteAll` 是非同步的，帶著 client 這邊的舊值和來源；`StorageAreaObserver` 的 `KeyChanged`、`KeyDeleted`、`AllDeleted` 通知每個 client（renderer 的 `CachedStorageArea` 靠它們維持自己的那一份、發 `storage` 事件）。`kPerStorageAreaQuota` 是 10 MiB，Browser 多容許 100 KiB（幾個 renderer 同時寫）。
- **`sessionStorage`**（`components/services/storage/dom_storage/session_storage_impl.h`）：每個 profile 一個 `SessionStorageImpl`，管所有 key 和 namespace；`BackingMode` 有只在記憶體、開的時候清掉磁碟、從磁碟還原（session restore）三種。
- **cookie 給 renderer**（`services/network/public/mojom/restricted_cookie_manager.mojom`）：`RestrictedCookieManager` 綁在一個 origin 上，「必須假設所有輸入都不可信」。`GetCookiesString` 是 `[Sync]`，可以順便給一塊共用記憶體，裡面只有一個 64 bit 的版本號，renderer 看版本沒變就不必再問；`SetCookieFromString` 不等回答；兩者都帶 `site_for_cookies` 和 `top_frame_origin`，網址和綁的 origin 不合算 renderer 違規。
- **`net::SiteForCookies`**（`net/cookies/site_for_cookies.h`）：兩個網址同一個網站的條件：scheme 相容（相同，或都在 {http, ws}，或都在 {https, wss}）而且 registrable domain 或 hostname 相同；或都沒有 hostname 而且 scheme 相同。
- **`net::CookiePartitionKey`**（`net/cookies/cookie_partition_key.h`）：序列化的是最上層的網站和 `has_cross_site_ancestor`。

照記憶、還沒對原始碼的（用到的階段再核對）：cookie 存在 `CookieMonster`（network service），持久的寫進 SQLite（`SQLitePersistentCookieStore`），值用作業系統的金鑰加密；`localStorage` 存在 LevelDB；這些都在 profile 的 `StoragePartition` 底下；chromedriver 預設給每個 session 一個暫時的 user data dir；同一個 profile 只開一個 browser process（`ProcessSingleton`）；Chrome 115 起預設開著第三方的儲存分區；沒標 `Partitioned` 的第三方 cookie 在一般模式照送。

### Firefox、WebKit

還沒看原始碼。知道的方向（照記憶）：Firefox 的 Total Cookie Protection 把第三方的 cookie 和儲存照最上層的網站分開；Safari 的 ITP 擋第三方的 cookie、儲存也分區。用得到的階段（第三方的 cookie 的決定）再對著原始碼和文件看。

## 可能用到的 crate

寫這份時沒有查它們的版本、維護狀況和授權，到各個階段比的時候再查：

- public suffix list：`psl`（清單編進程式）、`publicsuffix`（執行時解析清單）。
- cookie：`cookie`（解析 `Set-Cookie`）、`cookie_store`（RFC 6265 的儲存，reqwest 的 `cookies` 用它）。
- 磁碟：`rusqlite`（SQLite，可以帶 C 的原始碼一起編）、純 Rust 的嵌入式 key-value 資料庫。
- 平台的資料夾：`dirs`、`directories`。

## 參考資料

- HTML：「obtain a site」、Web Storage（`Storage`、`StorageEvent`、「storage」那一章）。
- Storage Standard（storage key、bottle、bucket）。
- RFC 6265bis（cookie：`SameSite`、前綴、`Secure`）。
- CHIPS（`Partitioned` 的 cookie）。
- public suffix list（publicsuffix.org）。
