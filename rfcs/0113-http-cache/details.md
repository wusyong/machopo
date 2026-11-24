# RFC 0113 的細節

## 網路版的 Speedometer

2026-11-24，macOS 26.6.2、Apple M4、接電源。machopo 是 `main` 的 `4b787490` 的 `perf` build（`machopo --automation-pipe`，沒有平台的視窗），Chrome 是 Chrome for Testing 152.0.7977.82（headless）。兩者都經過 CDP 開 `index.html?startAutomatically`（20 個預設的 suite、10 個 iteration，同一頁），跑完從頁面讀 runner 自己的結果（`benchmarkClient._measuredValuesList`）和 mark。網路版是 `https://browserbench.org/Speedometer3.1/`，本機版是 `../third_party/speedometer` 從本機的 HTTP server 給（不加任何 script，listen backlog 128）。網路上的 `index.html`、`main.mjs`、`benchmark-runner.mjs`、`tests.mjs`（runner 的四個檔案）和 TodoMVC-JavaScript-ES5 的 `index.html`、`app.js` 和本機的逐 byte 相同。

| | 分數 | 整頁 | 載入 suite 的頁面 | 計分的區間 |
| --- | --- | --- | --- | --- |
| machopo 本機 | 38.5 | 23.5 s | 8.9 s | 6.4 s |
| machopo 網路 | 28.0 | 115.3 s | 96.2 s | 8.8 s |
| Chrome 本機 | 58.6 | 15.1 s | 4.8 s | 4.1 s |
| Chrome 網路 | 57.0 | 26.6 s | 14.3 s | 4.3 s |

machopo 在網路版上每個 iteration 載入 suite 的頁面都要 300 到 1400 ms（TodoMVC-Backbone 約 1.4 s），十個 iteration 都一樣：每次重新下載。browserbench.org 的回應（HTTP/2）：

```
last-modified: Mon, 23 Nov 2026 08:01:08 GMT
etag: "2a7-660ecff0f3900"
```

沒有 `Cache-Control`、`Expires`，所以 Chrome 用 heuristic freshness：當時距離 `Last-Modified` 約 19 小時，新鮮約 1.9 小時，第一個 iteration 之後不再問伺服器。

### 計分的區間也變慢

machopo 在網路版上計分的時間也慢了（每個 suite 十次的中位數，ms）：

| suite | 本機 | 網路 |
| --- | --- | --- |
| TodoMVC-JavaScript-ES5 | 25.3 | 51.3 |
| TodoMVC-Vue | 13.8 | 31.3 |
| TodoMVC-Backbone | 16.8 | 37.5 |
| Editor-TipTap | 73.7 | 102.8 |
| TodoMVC-Svelte-Complex-DOM | 9.1 | 9.1 |

Chrome 每個 suite 兩邊差不到 5%。`MACHOPO_TRACE` 錄 TodoMVC-JavaScript-ES5 兩邊各十次，計分區間裡 renderer main thread 的 self time（ms）：script 79 → 188、parse 43 → 104、style 17 → 26、GC 8 → 17；其他 thread（V8 的 worker、compositor、Viz、IO）在計分區間裡幾乎沒事。檔案相同、工作相同，每個階段都一樣慢，像是 CPU 本身變慢。

用本機的 server 重現（每個 suite 自己的檔案的回應人工延遲，TodoMVC-JavaScript-ES5 十次的中位數）：

| | 不延遲 | 延遲 100 ms | 延遲 300 ms |
| --- | --- | --- | --- |
| machopo | 22.3 ms | 41.7 ms | 39.6 ms |
| Chrome（`--disable-cache`） | 20.5 ms | 42.1 ms | 41.4 ms |

兩個瀏覽器都慢一倍：每次載入閒置一段時間之後，計分的那幾十 ms 跑得慢（Apple Silicon 降頻或把 thread 排到 E-core；machopo 沒有設 thread 的 QoS，但 Chrome 也一樣，所以不是 QoS）。Chrome 在網路版上不受影響，是因為它從 cache 拿、不閒置。

## machopo 現在的載入

`main` 的 `0a459d17`：

- `crates/net/src/url_loader.rs` 的 `load`：檢查 header 和 method、`Cors::new`、`file:` 和 `data:` 直接回答；`http:`、`https:` 每一跳：加 cookie（`CookieJar::cookie_header`）、CORS 的 `Origin` 和 preflight，`transport.send(&method, &url, accept, &sent, body)`，`set_from_response` 存 cookie，`cors.check`，redirect。cache 接在 `transport.send` 這一行。
- `crates/net/src/http.rs`：`HttpTransport`（reqwest 的 client，連線的池）和 `send`，回答 `RawResponse`（狀態碼、header、`HttpBody`、`LoadTiming`、remote address、protocol）。body 是還沒解 `Content-Encoding` 的 bytes（`content_encoding.rs` 在 `url_loader` 那一層解）。
- `crates/protocol/src/network.rs`：`ResourceRequest`（沒有 cache 的模式）、`TrustedParams`（`site_for_cookies`、`top_frame_site`、`initiator`、`is_main_frame_navigation`）。
- `crates/dom/src/fetch.rs` 的 `RequestCache` 和 `crates/dom_api/src/request.rs` 的 `CACHES`：`Request.cache` 解析和讀回，不送出去。
- `crates/renderer/src/render_thread/network_agent.rs` 的 `Network.setCacheDisabled`、`crates/shell/src/automation_session.rs` 的 `Network.clearBrowserCache`：回答成功，不做事。
- `crates/browser/src/profile.rs`：`Profile`，資料夾或記憶體裡的；`NetworkContextParams::data_directory` 是資料夾。

## Chromium 怎麼做

- **位置**：`URLRequestHttpJob`（cookie、HSTS 之後）建 `HttpTransaction`，`HttpCache` 的 factory 給 `HttpCache::Transaction`，它在 cache 和 `HttpNetworkTransaction` 之間決定：從 entry 讀、送條件請求、直接送。cache 之上的人看到的都是 `HttpResponseInfo`。
- **key**：`HttpCache::GenerateCacheKey`：分區時以 `_dk_` 開頭，加上 Network Isolation Key（最上層的網站、frame 的網站）和網址（沒有 fragment）。NIK 是 transient 的（opaque 的最上層）時不存。
- **新鮮度**：`HttpResponseHeaders::GetFreshnessLifetimes`：`max-age`、`Expires` 減 `Date`；都沒有時，預設可以 cache 的狀態碼（`200` 等；確切的清單，和永久的 redirect、`410` 沒有明確期限時的特別規則，實作前確認）用 `Date` 減 `Last-Modified` 的 10%；`must-revalidate`、`no-cache`、`no-store`。`stale-while-revalidate` 在新鮮期之後的那段時間先用舊的、背景驗證（async revalidation）。
- **存的 header**：`HttpResponseInfo::Persist` 用 `PERSIST_SANS_COOKIES` 等選項，拿掉 `Set-Cookie`、`Set-Cookie2`。
- **load flag**：`LOAD_VALIDATE_CACHE`、`LOAD_BYPASS_CACHE`、`LOAD_SKIP_CACHE_VALIDATION`、`LOAD_ONLY_FROM_CACHE`、`LOAD_DISABLE_CACHE`。Blink 的 `FetchCacheMode` 對到它們：`no-store` 是 `LOAD_DISABLE_CACHE`，`reload` 是 `LOAD_BYPASS_CACHE`，`no-cache` 是 `LOAD_VALIDATE_CACHE`，`force-cache` 是 `LOAD_SKIP_CACHE_VALIDATION`，`only-if-cached` 是 `LOAD_ONLY_FROM_CACHE | LOAD_SKIP_CACHE_VALIDATION`。重新整理只對主要的資源 `LOAD_VALIDATE_CACHE`（2016 年起）。
- **backend**：`disk_cache::Backend` 的實作有 blockfile、Simple Cache（每個 entry 一到三個檔案加一個 index）和記憶體的 `MemBackendImpl`（無痕模式）；大小上限 `disk_cache::PreferredCacheSize` 照可用空間算。
- **同時的請求**：`HttpCache::ActiveEntry` 一個時間只讓一個 transaction 寫；後到的排隊（`add_to_entry_queue`），header 決定之後可以一起讀寫入中的 body（shared writers）。

這些是照 Chromium 的類別和函式名稱記下的，實作前對照 Chromium 的原始碼逐條確認。

## 其他做法

- **reqwest 或 hyper 的 cache 外掛**（`http-cache-reqwest`、`http-cache-semantics`）：前者的 key 只有網址和自己的模式，要分區得改它；machopo 的 reqwest 只在 `net::http`，外掛會讓它的型別進到 cache 的介面。後者只算 RFC 9111 的規則（新鮮度、`Vary`、條件請求的 header），不管儲存：可以考慮拿來算規則，省下自己寫的部分，要看它和 Chromium 的差別（heuristic 的狀態碼、`Age`、`Date` 不對時）。待決定裡沒有列，實作第 1 階段時比較。
- **cache 放在 renderer**：每個 renderer 一份，拆 process 之後彼此不共用，Browser 自己的請求（導覽）也用不到。不選。

## 磁碟的格式

| | 每個 entry 一個檔案加 index | header 在 SQLite、body 在檔案 | 全在 SQLite |
| --- | --- | --- | --- |
| 寫 body | 一段一段 append，收完 rename | 同左 | blob 要整份，或拆成很多列 |
| 讀 body | 一個檔案，可以邊讀邊給 data pipe | 同左 | 一次讀進記憶體或分列讀 |
| index 和淘汰 | 自己的 index 檔，損毀時重建 | SQLite 的表 | SQLite 的表 |
| 損毀 | 單一 entry 壞掉只丟它 | 表和檔案要對得上 | SQLite 自己處理 |
| 先例 | Chromium 的 Simple Cache、Firefox 的 `cache2` | 無 | 無 |

body 是串流的大塊資料（`ipc::data_pipe` 一段一段給），整份放 SQLite 不划算；index 和淘汰的順序放 SQLite 可以省下自己的格式和損毀的處理。2026-11-24 的看法：body 放檔案，header 和 index 在第一欄和第二欄之間，第 2 階段開始前決定。
