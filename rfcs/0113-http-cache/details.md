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
- **key**：`HttpCache::GenerateCacheKey`（2026-11-24 讀 `main` 的 `net/http/http_cache.cc`）：分區時是 `_dk_`，子 frame 的文件加 `s_`，跨網站的最上層導覽（發起的和網址不同網站）加 `cn_`，再加 `NetworkIsolationKey::ToCacheKeyString()`（各段以空格隔開）、空格和網址；最前面還有 credential（`0/`、`1/`）和上傳資料的編號。`NetworkIsolationKey`（`net/base/network_isolation_key.h`）存最上層的網站、frame 的網站、nonce 和 partition，沒有跨網站的 bit。key 是 transient 的（有 nonce 的，opaque）時不存（`IsTransient()`）。`net/base/features.cc` 的 `kSplitCacheByNetworkIsolationKey` 寫的是預設關閉，Chrome 實際上是開的（下一節的實測），開的方式原始碼裡看不出來。
- **新鮮度**：`HttpResponseHeaders::GetFreshnessLifetimes`：`max-age`、`Expires` 減 `Date`；都沒有時，預設可以 cache 的狀態碼（`200` 等；確切的清單，和永久的 redirect、`410` 沒有明確期限時的特別規則，實作前確認）用 `Date` 減 `Last-Modified` 的 10%；`must-revalidate`、`no-cache`、`no-store`。`stale-while-revalidate` 在新鮮期之後的那段時間先用舊的、背景驗證（async revalidation）。
- **存的 header**：`HttpResponseInfo::Persist` 用 `PERSIST_SANS_COOKIES` 等選項，拿掉 `Set-Cookie`、`Set-Cookie2`。
- **load flag**：`LOAD_VALIDATE_CACHE`、`LOAD_BYPASS_CACHE`、`LOAD_SKIP_CACHE_VALIDATION`、`LOAD_ONLY_FROM_CACHE`、`LOAD_DISABLE_CACHE`。Blink 的 `FetchCacheMode` 對到它們：`no-store` 是 `LOAD_DISABLE_CACHE`，`reload` 是 `LOAD_BYPASS_CACHE`，`no-cache` 是 `LOAD_VALIDATE_CACHE`，`force-cache` 是 `LOAD_SKIP_CACHE_VALIDATION`，`only-if-cached` 是 `LOAD_ONLY_FROM_CACHE | LOAD_SKIP_CACHE_VALIDATION`。重新整理只對主要的資源 `LOAD_VALIDATE_CACHE`（2016 年起）。
- **backend**：`disk_cache::Backend` 的實作有 blockfile、Simple Cache（每個 entry 一到三個檔案加一個 index）和記憶體的 `MemBackendImpl`（無痕模式）。
- **記憶體的上限**（2026-11-24 讀 `main` 的 `net/disk_cache/memory/mem_backend_impl.cc`、`mem_entry_impl.cc`）：`kDefaultInMemoryCacheSize` 10 MiB、`kMaxMemoryCacheSize` 50 MiB；`CalculateDefaultMaxSize` 在讀不到實體記憶體時 10 MiB，2500 MiB 以上 50 MiB，其他是實體記憶體的 2%。`MaxFileSize` 是上限的 1/8，寫超過的回 `ERR_FAILED`；總量超過時 `ERR_INSUFFICIENT_RESOURCES`。寫入讓總量變大時當場 `EvictIfNeeded`：超過上限就從 LRU 清單最舊的一端淘汰到上限的 90%，正在用的跳過；讀寫都把 entry 移到最新的一端。記憶體吃緊時上限在 1/10 到 1/2 之間調。HTTP cache 實際傳給它的上限沒有查。
- **磁碟的上限**（同一天讀 `net/disk_cache/cache_util.cc` 的 `PreferredCacheSize`）：預設 80 MiB，非 Windows 的 HTTP 磁碟 cache 乘 4（320 MiB）；照可用空間分級（100 MiB 以下用 80%、800 MiB 以下 80 MiB、2000 MiB 以下 10%、20000 MiB 以下 200 MiB、以上 1%），結果不到可用空間的 1/5 時乘上同樣的倍數、最多到 1/5；上限是預設的 4 倍（非 Windows 1280 MiB）。讀不到可用空間時用 320 MiB。
- **同時的請求**：`HttpCache::ActiveEntry` 一個時間只讓一個 transaction 寫；後到的排隊（`add_to_entry_queue`），header 決定之後可以一起讀寫入中的 body（shared writers）。

這些是照 Chromium 的類別和函式名稱記下的，實作前對照 Chromium 的原始碼逐條確認。

### Chrome 的分區，實測

2026-11-24，Chrome for Testing 152.0.7977.82（headless、一個 profile、`--host-resolver-rules=MAP *.test 127.0.0.1`）。一個本機的 server 給一張圖（`http://c.test/img.png`，`Cache-Control: max-age=3600`），頁面都是 `no-store`；依序導覽，每一步看 server 有沒有再收到圖的請求：

| 誰載入圖 | 向 server 要 |
| --- | --- |
| 最上層 `a.test` | 要（第一次） |
| 最上層 `a.test`，再一次 | 不要 |
| 最上層 `b.test` | 要 |
| `a.test` 裡的 iframe `x.test` | 要 |
| 同上，再一次 | 不要 |
| `a.test` 裡的 iframe `y.test` | 要 |
| `a.test` 裡的 iframe `a.test` | 不要 |

最上層和 frame 的網站都在 key 裡；同網站的 iframe 和最上層共用一個 entry。

同一天、同樣的做法，opaque 的 frame（`a.test` 裡的 `<iframe sandbox>`，沒有 `allow-same-origin`，載入同一張圖）：

| 誰載入圖 | 向 server 要 |
| --- | --- |
| 最上層 `a.test` | 要（第一次） |
| `a.test` 裡的 `sandbox` iframe | 要 |
| 同上，再一次 | 要 |
| 最上層 `a.test`，再一次 | 不要 |

opaque 的 frame 的請求不讀也不寫 cache。

Chrome 2020 年的說明（`https://developer.chrome.com/blog/http-cache-partitioning`）：Chrome 86 起分區；量到的代價是 miss rate 多約 3.6%、從網路來的 bytes 多約 4%、FCP 多約 0.3%；當時 Safari 用最上層的 eTLD+1，Firefox 打算用最上層的網站（現在的做法沒有查）。

### Chrome 的同時的請求，實測

2026-11-24，Chrome for Testing 152（headless）。本機的 server 對 `/data`（`Cache-Control: max-age=3600`，200 KB）等 1 秒才回應，body 分兩半、相隔 0.5 秒送；同一頁同時發兩個 `fetch('/data')`。server 只收到一次，兩個 `fetch()` 都拿到完整的 200 KB，從開始到兩個都完成約 1.5 秒（一個回應的時間）。

## 其他做法

- **reqwest 或 hyper 的 cache 外掛**（`http-cache-reqwest`、`http-cache-semantics`）：前者的 key 只有網址和自己的模式，要分區得改它；machopo 的 reqwest 只在 `net::http`，外掛會讓它的型別進到 cache 的介面。後者只算 RFC 7234 的規則、不管儲存，和 Chromium 有幾處不同，不用它（已決定的第 4 條）。
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
