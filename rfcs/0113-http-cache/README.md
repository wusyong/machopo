# RFC 0113：HTTP cache

- 開始日期：2026-11-24
- 狀態：👍 已接受：2026-11-24
- 細節：`details.md`

## 摘要

machopo 沒有 HTTP cache：每次載入都重新向伺服器要。這份在 `net` 裡、HTTP 的請求送出去之前加一層 cache（Chromium 的 `HttpCache`）：照 RFC 9111 決定存不存、能不能直接用、要不要先問伺服器（條件請求和 `304`），key 照 Chromium 用最上層的網站和 frame 的網站分區（已決定），`fetch()` 的 `Request.cache`、重新整理和 DevTools 的「停用 cache」照 Chromium 的 load flag 走。先做記憶體裡的 cache，再做 profile 資料夾裡的。第 1 階段的選擇都確認了（「已決定的事項」）；磁碟的格式和上限在第 2 階段開始前決定（「待決定」）。

## 動機

2026-11-24 用 machopo 跑 browserbench.org 上的 Speedometer 3.1（20 個 suite、10 個 iteration），分數是 28.0，同一份檔案從本機跑是 38.5；整頁從 23.5 s 變成 115.3 s。Chrome 兩邊是 57.0 和 58.6、26.6 s 和 15.1 s（量法和逐項的數字在 `details.md` 的「網路版的 Speedometer」）。原因有兩層：

- **每次載入都重新下載**：runner 每個 iteration 都重新載入每個 suite 的頁面，machopo 每次都從 browserbench.org 抓（每次 300 到 1400 ms，200 次合計 96 s）。Chrome 從第一個 iteration 之後就從 cache 拿：伺服器的回應沒有 `Cache-Control`、`Expires`，只有 `Last-Modified` 和 `ETag`，Chrome 照 heuristic freshness（`Last-Modified` 到現在的 10%）直接用。
- **等網路的時候 CPU 降速，計分的那段跟著慢**：在網路版上，machopo 計分的時間也慢了一倍（TodoMVC-JavaScript-ES5 從 25 到 51 ms），每個階段都一樣慢，其他 thread 沒有在忙。本機的 server 人工延遲每個回應 100 ms 就重現，Chrome 關掉 cache 時也一樣慢一倍：Apple Silicon 閒置一段時間後降頻或把 thread 排到 E-core，下一段短的工作跑得慢。有 cache 就不會在每次載入時閒置那麼久。

一般的網站也一樣：上一頁、下一頁、重新整理、同一個網站的下一頁，都重新下載全部的 script、樣式和圖片，等的時間和流量都比 Chrome 多。

### 開始時的狀況

2026-11-24 讀程式得到的（`main` 的 `0a459d17`；逐項的位置在 `details.md` 的「machopo 現在的載入」）：

- **網路**：`net` 的 `url_loader::load` 每一跳先加上 cookie、CORS 的 `Origin`，再呼叫 `http::HttpTransport::send`（reqwest）拿 `RawResponse`，之後才是 CORS 的檢查、redirect、`Content-Encoding` 的解碼和 MIME 的 sniff。中間沒有任何 cache；`net` 的 crate 文件寫著「No HTTP cache」。
- **請求的來歷**：`ResourceRequest::trusted`（`TrustedParams`）有 `site_for_cookies` 和 `top_frame_site`，沒有 frame 自己的網站（Chromium 的 `IsolationInfo` 的 `frame_origin`）。
- **`fetch()` 的 `cache`**：`Request.cache` 解析了（`dom_api` 的 `request.rs`，`dom` 的 `RequestCache`），但沒有送到 `net`。
- **DevTools**：`Network.setCacheDisabled` 和 `Network.clearBrowserCache` 回答成功、什麼都不做；回應一律不是從 cache 來的。
- **profile**：資料夾裡有 cookie 和 Web Storage 的 SQLite；記憶體裡的 profile（自動化、測試）什麼都不留。
- **量測工具的 server**：`bench.py` 的 server 回 `Cache-Control: no-store`，加了 cache 之後它量的數字不變。

### 目標

1. 照 RFC 9111 的私有 cache：存得了的回應存起來；新鮮的直接用；不新鮮的帶 `If-None-Match`、`If-Modified-Since` 問伺服器，`304` 時用存的 body、更新 header；`Vary`、`no-store`、`no-cache`、`must-revalidate`、heuristic freshness。
2. key 照 Chromium 分區：同一個網址在不同的最上層網站底下是不同的 entry，第三方不能用 cache 探測使用者去過哪裡。
3. `fetch()` 的 `Request.cache` 的六種模式、重新整理（照 Chromium：只驗證主要的資源）、DevTools 的停用和清除 cache 都照 Chromium。
4. 磁碟上的 cache 在 profile 的資料夾裡，有大小上限，壞掉的檔案不讓瀏覽器壞掉。
5. WPT 的 `fetch/http-cache/` 和 `fetch/api/request/` 的 `request-cache-*` 大部分通過；網路版的 Speedometer 第二個 iteration 起不再向伺服器要 suite 的檔案。

### 不在範圍內

- renderer 裡的 memory cache（Blink 的 `MemoryCache`）、V8 的 code cache（Chromium 的 `GeneratedCodeCache`）、back-forward cache、Cache Storage 和 service worker。
- CORS preflight 的 cache（`Access-Control-Max-Age`）。
- `Range` 請求和不完整的 entry（Chromium 的 sparse entry，影音用）。
- 清除瀏覽資料的 UI、`Clear-Site-Data`。

### 解決的限制

`docs/limitations.md` 的「網路」的「沒有 HTTP cache，每次載入都重新向伺服器要」。

## 設計概要

對網頁的作者：回應照 `Cache-Control`、`Expires`、`ETag`、`Last-Modified` 被 cache，和 Chrome 一樣；`fetch(url, {cache: "no-store"})` 這類模式照規範。嵌在別的網站裡的 iframe 有自己的一份 cache。Resource Timing 和 Navigation Timing 的 `transferSize` 在從 cache 拿時是 0（之後 machopo 有 Resource Timing 時）。

對 machopo 的使用者：同一個網站的第二頁、上一頁和重新整理不再重新下載全部；磁碟上的 cache 在 profile 資料夾的 `Cache` 底下，自動化和測試的 profile 只有記憶體裡的一份，每次從空的開始。DevTools 的 Network 面板勾「Disable cache」就不用 cache。

對之後改這部分程式的人：cache 在 `net` 的 HTTP 那一層，`url_loader` 看到的仍然是 `RawResponse`；cookie、CORS、redirect 和解碼的程式不變。要 cache 的請求不用做任何事；不要 cache 的照 load flag 說。

## 詳細設計

Chromium 怎麼做、每個選項的比較在 `details.md`。這裡寫打算怎麼接，選項還沒確認的地方標「待決定」。

### 位置

新的模組 `net::http_cache`（Chromium 的 `net::HttpCache` 和它的 `Transaction`），夾在 `url_loader::load` 每一跳和 `HttpTransport::send` 之間：`url_loader` 呼叫 cache，cache 決定從 entry 回答、先送條件請求，或直接送出去。它回答的仍然是 `RawResponse`，所以之後的 cookie（cache 回答的回應不再設 cookie，存的 header 本來就拿掉了 `Set-Cookie`，Chromium 的 `PERSIST_SANS_COOKIES`）、CORS、redirect（`301` 和有新鮮度的 redirect 也存）、`Content-Encoding` 的解碼和 sniff 都照舊。存的是還沒解碼的 bytes，和 Chromium 一樣。

只有 `http:`、`https:` 的 `GET` 經過 cache（Chromium 的 cache 也存 `POST` 給上一頁用，這份不做）；`POST`、`PUT`、`DELETE` 成功時，把同一個 key 的 entry 作廢（RFC 9111 的 4.4，WPT 的 `invalidate.any.js`）。

### key

照 Chromium（已決定的第 1 條）：最上層的網站、frame 的網站和網址（去掉 fragment）。`TrustedParams` 加上 `frame_site`，由 Browser 的 `frame_tree.rs` 在 `loader_params`（子資源：frame 自己的 origin 的網站）和 `navigation_params`（導覽：要去的網址的網站）給，和 `top_frame_site` 一樣不信任 renderer。worker 的請求用它的 owner 的。最上層或 frame 的網站是 opaque 的（`sandbox` 沒有 `allow-same-origin` 的 iframe、`data:` 的頁面），和沒有 `trusted` 的請求，不經過 cache：不讀也不寫（已決定的第 2 條）。

### 新鮮度和驗證

machopo 自己寫（已決定的第 4 條），照 RFC 9111 和 Chromium 的 `HttpResponseHeaders`（`GetFreshnessLifetimes`、`RequiresValidation`）和 `HttpVaryData` 逐條對照；日期用 `httpdate`（HTTP 的三種日期格式都收）。`max-age`、`Expires`（`s-maxage` 不算，私有 cache）、沒有時對預設可以 cache 的狀態碼用 heuristic（`Date` 減 `Last-Modified` 的 10%）；`Age` 和收到時的時間算現在的年齡。不新鮮或 `no-cache` 時送條件請求，`304` 時用存的 body、合併 header、更新 entry。`Vary` 的 header 和存的請求 header 比，不同時當成沒有。`stale-while-revalidate` 之後再做（「之後的可能」）。

### load flag

`ResourceRequest` 加一個 cache 的模式（Chromium 的 `LOAD_VALIDATE_CACHE`、`LOAD_BYPASS_CACHE`、`LOAD_SKIP_CACHE_VALIDATION`、`LOAD_ONLY_FROM_CACHE`、`LOAD_DISABLE_CACHE`）：`fetch()` 的 `Request.cache` 照 Blink 對到它們；重新整理時主要的資源 `LOAD_VALIDATE_CACHE`、子資源照常（Chromium 2016 年之後的做法）；強制重新整理全部 `LOAD_BYPASS_CACHE`；上一頁、下一頁 `LOAD_SKIP_CACHE_VALIDATION`；DevTools 的 `Network.setCacheDisabled` 讓那個分頁的請求都 `LOAD_DISABLE_CACHE`，`Network.clearBrowserCache` 清掉整個 cache。DevTools 的回應帶 `fromDiskCache`。

### 儲存

兩種 backend 同一個介面（Chromium 的 `disk_cache::Backend`）：記憶體的（記憶體裡的 profile，Chromium 無痕模式的 `MemBackendImpl`）和磁碟的（profile 資料夾的 `Cache`）。磁碟的格式和上限待決定（第 1、2 條）。記憶體的照 Chromium 的 `MemBackendImpl`（已決定的第 5 條）：上限是實體記憶體的 2%，2.5 GiB 以上是 50 MiB，讀不到時 10 MiB；單一個 entry 最多是上限的 1/8，超過的不存；超過上限時照最近用過的順序淘汰到上限的 90%，正在用的 entry 跳過。entry 在 body 整份收完、寫完才算數；載入取消、出錯的不留。

### 同時的請求

已決定的第 3 條：同一個 key 已經有一個請求在網路上時，後到的等它的 header。回應不能存（`no-store`、`Vary: *` 這類）就自己送出去；能存就等它整份收完、寫進 entry，再從 entry 讀。entry 多一個「寫入中」的狀態和等候的名單；第一個失敗、被取消或不能存時叫醒它們，各自送出去。

### 測試

- `net` 的整合測試（`crates/net/tests/it/` 的新模組），用 `test_server` 控制回應的 header 和次數：新鮮的不送請求、不新鮮的送條件請求、`304` 合併、`Vary`、`no-store`、分區、load flag、`POST` 作廢、取消的載入不留 entry、大小上限的淘汰、磁碟上壞掉的 entry。
- WPT：`fetch/http-cache/` 加進 `tests/wpt/include.txt`（`freshness`、`heuristic`、`304-update`、`vary`、`invalidate`、`cc-request`、`cache-mode`、`status`、`split-cache` 等），`fetch/api/request/` 的 `request-cache-*` 已經在清單上，更新預期結果。
- 情境：重新整理和上一頁時不再向 server 要子資源（server 數請求的次數）。
- mutation test 照 `CLAUDE.md`。

### 分階段

1. **記憶體的 cache 和規則**：`http_cache` 模組、key 和 `TrustedParams` 的 frame 網站、新鮮度和驗證、`Vary`、作廢、load flag（`fetch()`、重新整理、上一頁、DevTools）。這時磁碟上的 profile 也先用記憶體的 backend。WPT 的 `fetch/http-cache/` 在這一階段加進清單。
2. **磁碟的 backend**：profile 的 `Cache`、格式、大小上限和淘汰、壞掉的檔案、啟動時的 index。
3. 重量網路版的 Speedometer（量法照 `details.md`），結果寫進 `progress.md`。

## 缺點

- 多一層狀態：cache 的 bug 讓頁面拿到舊的或別人的內容，比沒有 cache 難查；所以分區和 `Vary` 一開始就照 Chromium 做。
- 磁碟的 backend 要自己維護格式、損毀和並行（`sql` 的 SQLite 能省下其中一部分，見待決定的第 1 條）。
- 測試多了「上一次載入留下了什麼」：記憶體裡的 profile 每次從空的開始，測試之間不互相影響，但同一個測試裡的載入會。

## 理由和其他做法

- **放在 `net` 的 HTTP 那一層，不在 renderer**：和 Chromium 一樣，所有 renderer 和 Browser 自己的請求共用一份；renderer 拆成 process 之後不用跨 process 同步。renderer 裡的 memory cache 是另一件事（「不在範圍內」）。
- **不用 reqwest 或 hyper 的 cache 外掛**（`http-cache-reqwest` 這類）：key 要照 machopo 的分區、load flag 要照 Chromium，那些 crate 的介面只有網址和自己的模式；而且 reqwest 只在 `net::http` 裡，不讓它的型別往外漏。逐項的比較在 `details.md` 的「其他做法」。
- **不做**：網路版的 Speedometer 和一般網站的每次載入都多等網路，而且等的時候 CPU 降速，連 JS 都跑得慢。

## 先例

- **Chromium**：`net::HttpCache`、`HttpCache::Transaction` 的狀態機，`disk_cache` 的 backend（blockfile、Simple Cache、記憶體的），2020 年起照 Network Isolation Key 分區；`HttpResponseHeaders` 的新鮮度和 heuristic；存 header 時拿掉 `Set-Cookie`；load flag。照做：位置、分區、新鮮度、load flag、記憶體的 backend 給不存檔的 profile。不照做（這份）：`POST` 的 entry、sparse entry、`stale-while-revalidate` 的背景驗證、shared writers（已決定的第 3 條）。
- **Firefox**：`cache2`，每個 entry 一個檔案加一個 index；照最上層的網站分區（`partitionKey`）。
- **Ladybird**：RequestServer 有 HTTP 的磁碟 cache 的選項，WebContent 有 HTTP 的 memory cache（2026-11-24 從 2026.47.7 啟動 helper process 的參數看到的：RequestServer 的 `--http-disk-cache-mode disabled`、WebContent 的 `--enable-http-memory-cache`；它的程式沒有讀過）。

細節和其他實作的逐項比較在 `details.md` 的「Chromium 怎麼做」。

## 已決定的事項

1. **cache 的 key 照 Chromium 分區：最上層的網站、frame 的網站和網址**（2026-11-24，使用者決定；原本「待決定」的第 5 條，A）：
   - `TrustedParams` 加 `frame_site`，Browser 從 frame 樹給（「詳細設計」的「key」）。
   - 對照 Chrome：Chromium `main` 的 `NetworkIsolationKey` 是最上層和 frame 的網站（triple key），cache key 以 `_dk_` 開頭；2026-11-24 在 Chrome for Testing 152 上實測，同一張可以 cache 的圖，最上層換網站、或只換 iframe 的網站（同一個最上層底下的 `x.test` 和 `y.test`）都重新向 server 要，和最上層同網站的 iframe 共用（`details.md` 的「Chrome 的分區，實測」）。
   - 理由：和 Chrome 的行為一樣，WPT 的 `split-cache.html` 和網頁看到的都對得上；同一個最上層底下，不同網站的 iframe 也不能用 cache 互相探測。代價只是兩個地方多給一個欄位。
   - 不選最上層的網站加跨網站的 bit（B，可以用 `StorageKey` 的 ancestor chain bit）：同一個最上層底下的跨網站 iframe 互相看得到，和 Chrome 不同。不選只用最上層的網站（C，2020 年時的 Safari）：嵌在同一個網站裡的第三方彼此看得到。不選不分區（D，2020 年前的 Chrome）：任何網站都能探測使用者去過哪裡。四種的效能差不多（key 多一兩段字串）；Chrome 2020 年量到分區讓 miss rate 多約 3.6%、從網路來的 bytes 多約 4%。

2. **沒有網站可分區的請求不經過 cache**（2026-11-24，使用者決定；原本「待決定」的第 3 條，A）：最上層或 frame 的網站是 opaque 的，或請求沒有 `trusted`（現在的程式裡 Browser 都會給，這條防萬一），不讀也不寫 cache。
   - 對照 Chrome：Chromium 的 key 是 transient 的（opaque 的網站、有 nonce）時不存（`HttpCache::GenerateCacheKey`）；2026-11-24 在 Chrome for Testing 152 上實測，`a.test` 裡的 `sandbox` iframe 載入同一張可以 cache 的圖，每次都向 server 要，也沒用到最上層 `a.test` 存的那份（`details.md` 的「Chrome 的分區，實測」）。
   - 理由：沒有網站就沒辦法分區；照 Chrome 不 cache 最簡單、不會洩漏。這種頁面少，多跑網路的成本小。
   - 不選每份 opaque 的文件一個暫時的 key（B）：同一份文件裡重複的載入少見（renderer 已經對圖片去重），多一套 key 的生命週期，和 Chrome 不同。不選退回網址自己的網站或不分區（C）：`sandbox` 的 iframe 能用 cache 探測，正是分區要擋的。

3. **同一個 key 同時有兩個請求：後到的等第一個的 header，能存就等它收完再從 entry 讀**（2026-11-24，使用者決定；原本「待決定」的第 4 條，C）：
   - 做法在「詳細設計」的「同時的請求」。
   - 對照 Chrome：Chromium 的 `HttpCache::ActiveEntry` 一次只讓一個 transaction 寫，後到的排隊，header 決定之後一起讀寫入中的 body（shared writers）。2026-11-24 在 Chrome for Testing 152 上實測，同一頁同時兩個 `fetch()` 要同一個可以 cache 的網址，server 只收到一次（`details.md` 的「Chrome 的同時的請求，實測」）。
   - 理由：server 看到的和 Chrome 一樣只有一次；比 shared writers 簡單，entry 不用一邊寫一邊讀。代價是後到的要等第一個整份收完才拿到資料，兩個請求差不多同時開始時差別小。量到常見、而且是大的回應時再換成 A。
   - 不選 shared writers（A）：entry 要能一邊寫一邊讀，第一個被取消時要讓後到的接手繼續收，複雜度高。不選後到的直接送出去（B，2026-11-24 原本的看法）：server 收到兩次，和 Chrome 不同。

4. **新鮮度和驗證的規則自己寫，照 Chromium 逐條對照**（2026-11-24，使用者決定；A）：
   - 照 Chromium 的 `HttpResponseHeaders` 和 `HttpVaryData`，日期用已經是依賴的 `httpdate`。`http-cache-semantics` 的測試案例可以參考（BSD-2-Clause，只參考案例，不複製程式）。
   - 理由：目標是和 Chrome 的行為一樣，規則要能逐條對照；要寫的量不大（Chromium 這部分的核心是幾個函式）。
   - 不選 `http-cache-semantics` 3.0.0（B）：它照 RFC 7234，`Expires`、`Last-Modified` 只解析 RFC 2822 的格式（RFC 850 和 asctime 的日期當成已經過期，Chromium 都收），`immutable` 預設至少新鮮 24 小時，heuristic 的狀態碼照 RFC 的清單（Chromium 的還沒對照）；日期和狀態碼調不了，要 fork 或在外面補（2026-11-24 讀它的 `src/lib.rs`）。效能上兩者一樣。

5. **記憶體的 backend 照 Chromium 的 `MemBackendImpl` 定上限和淘汰**（2026-11-24，使用者決定；原本「待決定」的第 2 條的前一半，A）：
   - 數字和規則在「詳細設計」的「儲存」；系統記憶體吃緊時降低上限（Chromium 降到 1/10 到 1/2）之後再做。第 1 階段裡磁碟上的 profile 也用它。
   - 對照 Chrome：2026-11-24 讀 Chromium `main` 的 `net/disk_cache/memory/mem_backend_impl.cc` 和 `mem_entry_impl.cc`（`details.md` 的「Chromium 怎麼做」）。
   - 理由：和 Chrome 一樣；50 MiB 裝得下 Speedometer 所有 suite 的檔案（建好的檔案合計最多約 23 MB，最大的 1.9 MB），網路版的目標達得到。淘汰在寫入時順便做，成本小。
   - 不選第 1 階段給磁碟上的 profile 一個大的記憶體 cache（B，例如 Chromium 磁碟 cache 的 320 MiB）：只是過渡，平常瀏覽最多多吃 320 MB 的記憶體。不選固定 10 MiB（C）：裝不下 Speedometer，網路版照樣每次重新下載。

## 待決定

2026-11-24 的看法寫在每一條後面，還沒確認：

1. **磁碟的格式**：(a) 每個 entry 一個檔案（header 和 body）加一個 index，像 Chromium 的 Simple Cache 和 Firefox 的 `cache2`；(b) 用 `sql` 的 SQLite，header 和小的 body 放在表裡、大的 body 放檔案；(c) 全放 SQLite。看法：body 一律放檔案（它是一段一段寫進去的大塊資料，SQLite 的 blob 要整份或另外處理增量的寫入），header 和 index 在 (a) 和 (b) 之間，第 2 階段開始前決定。比較在 `details.md` 的「磁碟的格式」。
2. **磁碟的大小上限**：固定的值，或照磁碟的可用空間（Chromium 的 `disk_cache::PreferredCacheSize`，`details.md` 的「Chromium 怎麼做」）。看法：照 Chromium 的算法。第 2 階段開始前決定。

## 之後的可能

- `stale-while-revalidate`（Chromium 的 async revalidation）。
- `Range` 和 sparse entry（影音的 seek）。
- V8 的 code cache（Chromium 存在另一個 `GeneratedCodeCache`，key 也照網站分）：Speedometer 每次載入都重新編譯 suite 的 script。
- renderer 的 memory cache、back-forward cache。
- `bench.py` 加一種量法：server 回可以 cache 的 header，量有 cache 時的整體時間（現在的 `no-store` 只量沒有 cache 的）。
