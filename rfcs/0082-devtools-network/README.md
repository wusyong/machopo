# RFC 0082：DevTools 的頁面和網路：`Network`、`Log` 和頁面的生命週期

- 開始日期：2026-10-23
- 狀態：✅ 已完成：2026-10-24（排程：2026-10-23；「已決定的事項」第 1 到 3 條，第 1 段的第 4 到 15 條，第 2 段的第 16 到 24 條，第 3 段的第 25 到 30 條）
- 進度和驗收：`progress.md`
- 細節：`details.md`（Chromium 的對照、DevTools 前端連上時送的命令、RFC 開始前（2026-10-23）kun 的載入怎麼走、各個選項的完整比較）

## 摘要

Chrome 的 DevTools 連上 kun 之後，Network 面板列出頁面的每個請求：文件、script、stylesheet、圖片、iframe 和 `fetch()`，有狀態碼、類型、發起者、大小和時間，點開看得到 header 和回應的內容；載入失敗的請求在 Console 有 Chrome 的那一行「Failed to load resource」；Sources 的檔案樹列出子資源。Playwright 和 Puppeteer 等頁面載入要的生命週期事件（`init`、`DOMContentLoaded`、`load`、`networkIdle`……）和 `Page.navigate` 的回答時機也照 Chrome（它們真的連上 kun 還要 isolated world，路線的第 4 項）。做法照 Chromium 的分工：子資源的事件和回應的內容由 renderer 的 network agent 送和保留（Blink 的 `InspectorNetworkAgent`），導覽的請求和回應由 browser 送（`NetworkHandler`），`Log` 和生命週期也在 renderer（`InspectorLogAgent`、`InspectorPageAgent`）。cookie、HTTP cache、限速、攔截請求（`Fetch`）、`Emulation` 和 Elements 面板不在這份。

## 動機

**現況**：0075 做了 CDP 的骨架和 V8 的 inspector，DevTools 的 Console、Sources、Memory 和 Profiler 能用，但 Network 面板是空的：查 Speedometer 或網站的問題時，看不到頁面要了什麼、伺服器回了什麼、哪個請求失敗，只能在 `net` 開 `RUST_LOG`。Playwright、Puppeteer 等頁面載入時看 `Page.lifecycleEvent` 和 `Page.navigate` 的回答，kun 現在只送 main frame 的 `DOMContentLoaded` 和 `load`，`Page.navigate` 一開始導覽就回答，`waitUntil: 'networkidle'` 等不到。

這是 0075 的「已決定的事項」第 1 條的路線第 2 項（頁面和網路）。那條路線寫的 frame 的事件和 `Page.getResourceTree` 在 0075 的第 4 段已經做了，這份做剩下的。

**目標**：
- DevTools 的 Network 面板：每個請求的 `Network.requestWillBeSent`、`responseReceived`、`dataReceived`、`loadingFinished`、`loadingFailed`，redirect 的每一跳，`Network.getResponseBody`、`getRequestPostData`；DevTools 連上時送的 `Network` 命令都有回答（`details.md` 的「DevTools 前端連上時送的命令」）。
- 請求的發起者（`Network.Initiator`）：parser、script（連同 stack）。
- `Log` domain：載入失敗和 4xx、5xx 的回應，照 Chrome 的文字，DevTools 的 Console 顯示。
- Sources 的檔案樹列出子資源（`Page.getResourceTree` 的 `resources`），`Page.getResourceContent` 給得出它們的內容。
- 生命週期：`Page.lifecycleEvent` 照 Chrome 的種類，每個 frame 都送；`Page.domContentEventFired`、`loadEventFired`、`frameStartedLoading`、`frameStoppedLoading`；`Page.navigate` 照 Chrome 在 commit 前回答，失敗時帶 `errorText`。
- `Target` 補齊：`Target.setAutoAttach`、`detachFromTarget`，flat session 的分頁關掉時送 `Target.detachedFromTarget`。

**不在範圍內**：
- cookie、HTTP cache（kun 都沒有：`Network.getCookies` 回答空的，`setCacheDisabled` 的效果本來就成立）；
- 限速和離線（`emulateNetworkConditions` 的實際效果）、封鎖請求（`setBlockedURLs` 的實際效果）、`setExtraHTTPHeaders`、`Fetch` 的攔截、`Emulation`、isolated world、`waitForDebuggerOnStart`：Playwright 的 RFC（路線第 4 項）；
- 原始的 header 和 cookie 的明細（`requestWillBeSentExtraInfo`、`responseReceivedExtraInfo`）：送 `hasExtraInfo: false`，DevTools 照樣能用（`details.md` 的「Chromium 的對照」）；
- CORS preflight 在 Network 面板的那一列（Chrome 由 browser 另外送，類型 `Preflight`）：之後；
- WebSocket、EventSource、worker 和 service worker 的請求：kun 的網頁還沒有這些；
- TLS 和 TCP 分開的時間：reqwest 的 connector 一起做，分不開，SSL 包在連線裡（第 4 節）；
- Elements 面板（路線第 3 項）、screencast。

**要解決的 `docs/limitations.md` 的限制**（「WebDriver 和自動化」）：
- 其他的 domain 回答找不到方法、Network 面板是空的（`Network` 的部分）；
- frame 樹不列子資源、`Page.getResourceContent` 只給 frame 的文件；
- `Page.lifecycleEvent` 只有 main frame 的 `DOMContentLoaded` 和 `load`；`Page.navigate` 開始導覽就回答、不回報 `errorText`；
- 沒有 `Target.setAutoAttach`、`detachFromTarget`，flat session 的分頁關掉時那個 client 不會收到 `Target.detachedFromTarget`。

## 設計概要

**DevTools 的使用者**：開了 Network 面板之後重新載入頁面，看到的和 Chrome 一樣：
- 每個請求一列：名稱、狀態、類型（`document`、`script`、`stylesheet`、`png`、`fetch`）、發起者（`(index):12` 或 `app.js:30`，滑過去看到 script 的 stack）、大小、時間、瀑布圖；藍線和紅線是 `DOMContentLoaded` 和 `load`。
- 點開一列：Headers（請求和回應的 header、狀態、遠端位址）、Preview 和 Response（回應的內容，圖片顯示成圖）、Initiator、Timing。Timing 分出 DNS、連線、等待伺服器和下載內容；SSL 包在連線裡，沒有另外一段（Chrome 畫在連線裡面）；重用的連線沒有 DNS 和連線，和 Chrome 一樣。
- redirect：每一跳一列（301 → 200），和 Chrome 一樣。
- 失敗的請求（連不上、CORS 擋下）是紅的，Console 有「Failed to load resource: net::ERR_CONNECTION_REFUSED」；404 的那一列是紅的，Console 有「Failed to load resource: the server responded with a status of 404 (Not Found)」。
- DevTools 開著之前的請求看不到（和 Chrome 一樣：Network 打開之後才記），要重新載入。
- Sources 的檔案樹列出圖片（stylesheet 是 DevTools 從 `CSS` domain 拿的，kun 沒有），點開看得到內容。
- Initiator 的 stack、Console 的「Failed to load resource」、Application 的 Frame Creation Stack Trace 和 Chrome 的一樣。

**自動化工具**：`Page.lifecycleEvent`（每個 frame 的 `init` 到 `networkIdle`）、`Page.navigate` 的回答（ready-to-commit，失敗帶 `errorText`）和 `Network` 的事件照 Chrome，`test.py cdp` 的 `lifecycle`、`network`、`log` 和 Chrome 一樣：Puppeteer `goto(…, {waitUntil: 'networkidle0'})` 等的東西都有了。Puppeteer、Playwright 本身還連不上 kun：它們要 browser 層級的 `Target.setAutoAttach` 和 isolated world（「已決定的事項」第 29 條，路線的第 4 項）。machopodriver 的 Navigate To 載入失敗時照 chromedriver 回答 `unknown error`。

**改這部分程式的人**：Network 的事件從兩個地方來，和 Chromium 一樣：
- **browser**（UI thread 的 session）送導覽的 `requestWillBeSent`、`responseReceived`，導覽失敗時的 `loadingFailed`：browser 自己發導覽的請求，renderer 只收到結果。
- **renderer**（`render_thread` 的 network agent）送子資源的全部事件（包括 iframe 的文件：kun 的 iframe 導覽由 renderer 發請求），以及導覽的文件 commit 時的 `dataReceived`、`loadingFinished`；回應的內容留在 renderer，`getResponseBody` 由它回答。

`net` 每走到一個 redirect 就停下來，通知發出請求的一方（子資源是 renderer，導覽是 browser 的 UI thread），等它說繼續才往下走（Chromium 的 `OnReceiveRedirect` 和 `FollowRedirect`）；回應也多帶時間（送出、收到 header、收完）。

## 詳細設計

### 1. 誰送 Network 的事件

照 Chromium（`details.md` 的「Chromium 的對照」第 1 項；「已決定的事項」第 2 條）：

| 事件 | 導覽（main frame） | 子資源和 iframe 的文件 |
| --- | --- | --- |
| `requestWillBeSent`（含 redirect 的每一跳） | browser | renderer |
| `responseReceived` | browser | renderer |
| `dataReceived`、`loadingFinished` | renderer（commit 時） | renderer |
| `loadingFailed` | browser（導覽失敗）；renderer（commit 之後） | renderer |

- **browser**：`WebContents` 的導覽開始時、收到回應時、失敗時，session 對打開 `Network` 的 client 送事件（打開的狀態照 `Page.enable` 的做法：browser 記下來、命令照樣轉給 renderer）。導覽的 `requestId` 和 `loaderId` 都是 browser 的 navigation id（`Page.lifecycleEvent` 已經用它當 `loaderId`），類型 `Document`。
- **renderer**：`render_thread` 加一個 network agent（Blink 的 `InspectorNetworkAgent`），和 0075 的 page agent 並列，DevTools 的命令先問它們。送出子資源請求的兩個地方（`render_thread.rs` 的 `fetch`、`loader.rs` 的 `FetchLoader::start`）在 `loader::RequestIds` 記下請求（只在有 session 打開 `Network` 時，複製一份；`fetch()` 的連同它的文件，取消之後就查不到了），每個 task 結束時、收到 redirect 和回應時，render thread 找出發出它的 frame 交給 agent，相當於 Blink 的 probe。請求的 `frameId` 是發出它的 frame 的 id（0075 的 frame id），`requestId` 是 renderer 的前綴加上它的 request id（Chrome 也是 process 的編號加上序號）；iframe 的文件的請求的 `requestId` 也是它帶來的文件的 `loaderId`（Chrome 的導覽的 `requestId` 和 `loaderId` 都是導覽的 token），`Page` agent 的子 frame 的 `loaderId` 跟著改成它（原本是文件的號碼；沒有請求帶來的初始 `about:blank` 照舊）。
- 沒有 client 打開 `Network` 時，agent 不記任何東西，只多一個判斷。

### 2. 回應的內容

照 Blink 的 `NetworkResourcesData`（「已決定的事項」第 3 條）：renderer 在 `Network` 打開時留著每個回應的內容，總共 200 MB、每個 20 MB（Chrome 的預設，`Network.enable` 的 `maxTotalBufferSize`、`maxResourceBufferSize` 可以改），超過時丟掉最舊的；`Network.disable`、main frame 導覽時清掉。`getResponseBody` 照 MIME 類型給文字或 base64，找不到、被丟掉時照 Chrome 的錯誤文字。導覽的文件的內容在 commit 時記（browser 不留）。`Network.enable` 的 `maxPostDataSize` 決定 `requestWillBeSent` 帶多少 POST 的 body（Chrome 的預設 0，DevTools 給 65536）。

回應的位元組共用，不複製（「已決定的事項」第 6 條）：`ResourceResponse::body` 從 `Vec<u8>` 改成 `bytes::Bytes`。`net` 照舊一塊一塊接到 `Vec<u8>` 上，收完時 `Bytes::from` 交出去（不複製；改成 `Arc<[u8]>` 反而要複製一次，因為 `Arc` 要在資料前放計數），network agent 記的時候只多一個參考。只借用 `&[u8]` 的地方（script、stylesheet 的解碼）不用改；拿走 body 的地方跟著改：`fetch()` 的 `FetchedResponse`（`dom_api`）、wasm 的 bytes、iframe 的文件、導覽的 `CommitNavigation`、圖片的 `ImageData`（`Bytes` 本身就是 `AsRef<[u8]>`）。`protocol` 開 `bytes` 的 `serde`。

### 3. redirect

照 Chromium 的 `URLLoaderClient::OnReceiveRedirect` 和 `URLLoader::FollowRedirect`（「已決定的事項」第 4 條）：`net` 的 `url_loader` 照舊自己決定能不能跟（redirect mode、CORS、不准到 `file:`、最多 20 次），能跟的時候先停下來，把這一跳（新的網址和方法、這一跳的狀態、header、時間）交給發出請求的一方，等它呼叫 `LoadHandle::follow_redirect` 才送下一個請求；停住時照樣能取消。DevTools 的同一個 `requestId` 的另一個 `requestWillBeSent`（帶上一跳的回應，`redirectResponse`）就在收到這一跳時送。

- **子資源**：IO thread 把這一跳送給 renderer（`RendererMsg::ReceivedRedirect { request_id, redirect }`），renderer 在收回應的同一個地方告訴 network agent，再回答 `RendererHostMsg::FollowRedirect { request_id }`，IO thread 叫那個載入繼續。renderer 一律回答：它取消了的請求，IO thread 已經沒有它；IO thread 收到不認得的 id 時忽略（renderer 不被信任）。kun 現在沒有 renderer 要每一跳判斷的事（Blink 在這裡查 CSP、mixed content），所以 renderer 一律繼續，不經過 script、stylesheet、圖片、`fetch()`、module 各自的程式；之後有了這類檢查就加在這裡。
- **導覽**：交給 UI thread（Chromium 的 `NavigationRequest::OnRequestRedirected` 也在 UI thread 上），session 送 DevTools 的事件之後叫它繼續。網路 thread 叫醒 UI thread 的路是外層程式（視窗、headless……）給 `WebContents` 的 callback，它原本只帶 renderer 的訊息；改成帶 browser 這一側的事件（`ContentsEvent`：renderer 的訊息，和導覽的事件），外層程式照舊轉到 UI thread 交給 `WebContents`（「已決定的事項」第 8 條）。導覽的事件不放進 `RendererHostMsg`：被攻破的 renderer 會偽造它。新的導覽開始時，取消還沒完成的舊導覽的載入（Chromium 也取消舊的 `NavigationRequest`），它停在 redirect 時不會一直等著。
- **行為的改變**：每一跳多一來一回。renderer 的 main thread 在跑 script 時，子資源的 redirect 等它跑完；停在斷點時也停住（Chrome 停在斷點時頁面的載入也暫停）。renderer 不在時，停住的載入和其他的一起取消。
- **之後的限制**：main thread 同步等一個請求時（之後的同步 `XMLHttpRequest`），它的 redirect 不能等 main thread 回答，要在別處決定（Chromium 的同步載入也另外處理）。kun 現在沒有同步等請求的地方。
- `net` 自己的測試和不在乎每一跳的呼叫者用一個自動繼續的版本。

### 4. 時間

`Network.ResourceTiming` 的每個欄位都要有，不知道的給 -1（`details.md` 的「Chromium 的對照」第 6 項）。`net` 量的（「已決定的事項」第 5 條）：

- 開始、收到 header（reqwest 的 `send` 回來）、收完 body；
- DNS：一律用 kun 自己的 resolver（`dns_resolver`；現在只有 host mapping 的規則時才用，沒有規則時改用同樣在 blocking thread 上呼叫系統解析的版本），量解析的時間；
- 建立連線（TCP 加 TLS）：reqwest 的 `connector_layer` 包一層量時間。TLS 分不開，`sslStart`、`sslEnd` 是 -1，`connectEnd` 包含 TLS；
- 時間記到哪個請求：DNS 和連線是 hyper-util 的連線池叫的，不是請求自己。hyper-util（0.1.21 的 `one_connection_for`）在請求自己的 future 裡同時等空閒的連線和開新的連線，新的連線就在那個 future 裡開，所以 `net` 用 tokio 的 task-local 把時間記到當時的請求上；空閒的連線先到時，新的連線改到背景開完，那段時間不屬於這個請求，丟掉。沒有記到的請求是重用的連線：DNS 和連線是 -1，`connectionReused` 是 true；
- proxy、worker、push 都是 -1。

2026-10-23 的原型（`timing-prototype/`）確認了這個做法：HTTP/1.1 依序、同時、競爭（新連線還在開時舊的先空出來）、HTTP/2 cleartext 和 HTTPS 由 ALPN 選的 HTTP/2，記到的都是請求自己開的連線，重用的請求都沒記到；競爭的情況裡沒用上自己那條連線的請求記到了 DNS、沒記到連線，所以「只記到 DNS 不算」這條規則是必要的。`net` 的測試伺服器原本每個回應之後就關掉連線、測不到重用，加了 keep-alive（`start_keep_alive`）。

時間是作業系統的單調時鐘的讀數（`ipc::TimeTicks`，Chromium 的 `base::TimeTicks`；「已決定的事項」第 12 條）：每個 process 讀的是同一個時鐘，`net`、browser 的 session 和 renderer 的 DevTools 事件不用交接起點就在同一條時間軸上；`requestTime` 和事件的 `timestamp` 是它換成秒。Rust 的 `Instant` 讀不出原始值：Linux、macOS 用 `rustix` 的 `clock_gettime(CLOCK_MONOTONIC)`，Windows 用 `QueryPerformanceCounter`（`unsafe`，所以放在 `ipc`）。（一開始 `LoadTiming` 是「從 `NetworkContextParams::time_origin` 起過了多久」，要 browser 交接起點；換成 `TimeTicks` 之後不用了。）`performance.now()` 的時鐘（renderer 的 `Clock`）照舊用 `Instant`。

`Network.Response` 的其他欄位：`protocol`（reqwest 的 HTTP 版本）、`remoteIPAddress` 和 `remotePort`（reqwest 的 `remote_addr`）、`encodedDataLength`（「已決定的事項」第 13 條：`net` 自己解壓，body 是解壓前的長度，header 照 HTTP/1 的寫法估算）、`securityState`（`https` 是 `secure`，其他照 Chrome）。`fromDiskCache`、`fromServiceWorker` 一律 false。

### 5. 發起者

照 Blink 的 `BuildInitiatorObject`：module 的 import 是 `script`（import 它的 module 的網址；Blink 還帶 import 那一行的位置，kun 的 module 載入沒有記，`docs/limitations.md`），CSS 要的（`@import`、背景圖、字型）是 `parser`（stylesheet 的網址），script 跑著時發的（`fetch()`、script 插入的 `<img>`）是 `script` 帶著當時的 stack，parser 發的是 `parser` 帶著文件的網址和行號，其他的 `other`。stack 只在 DevTools 送了 `Network.setAttachDebugStack`（它一連上就送）時抓，用 V8 的 inspector 抓（`V8Inspector::captureStackTrace`、`buildInspectorObject`，fork 加 binding；「已決定的事項」第 16 條），和 Chrome 一樣有 async 的呼叫鏈（`await`；計時器和事件的 callback 接不上排程它的 stack：renderer 沒有把它們告訴 inspector）。`buildInspectorObject` 只能序列化成 CBOR，fork 原本就有的 `crdtp::cbor_to_json` 轉成 JSON（原型先確認了這條路，release `kun-v152.2.0-19`）。script 建立的 iframe 的 `Page.frameAttached` 也帶著這個 stack（第 19 條）。kun 的 `<link>`、`<script>`、`<iframe>`、`<img>` 在 script 跑完或 parser 停下之後才一起發請求，所以 stack 在 dom_api 要求載入的當下抓、依元素記著，發請求時才用，一輪沒用上的丟掉（第 20、21 條）：`<img>` 在排「update the image data」的 microtask 時抓（microtask 跑時 stack 已經空了；Blink 的 `ImageLoader` 也在排程時用 `AsyncTaskContext` 記下），`<link>` 的變動是 `dom` 記的，多一種 `ElementChange::Link` 讓 dom_api 告訴 frame；parser 發的帶元素的開始標籤的行，沒有欄（第 22 條；parser 一次讀完整份文件、在請求發出之前就結束，行號留到那一輪的請求發完）；CSS 要的帶 `url()` 所在的 stylesheet 的網址（第 23 條）。

### 6. `Log`

照 Blink 的 `InspectorLogAgent`：renderer 留著最近 1000 則（Chrome 的上限），`Log.enable` 時補送，main frame 導覽時清掉；`Log.clear`、`disable`。這份只有 `network` 的兩種，文字照 Chrome：
- 回應的狀態是 400 以上：「Failed to load resource: the server responded with a status of 404 (Not Found)」（`FrameConsole::ReportResourceResponseReceived`）；
- 載入失敗（取消的不算）：「Failed to load resource: net::ERR_CONNECTION_REFUSED」（`FrameConsole::DidFailLoading`）。

每則帶 `url` 和 `networkRequestId`（DevTools 用它連到 Network 面板的那一列）。這兩種訊息也印到 stderr，和 `console` 的同樣格式（`[console] 文字 (網址)`；Chrome 的 `--enable-logging=stderr` 也把它們當 console 訊息印，「已決定的事項」第 17 條）。kun 做不到的 `Network` 命令回答錯誤時，另外送一則 warning 等級的 `Log.entryAdded`（來源 `other`，「方法: kun can't …」，例如「Network.overrideNetworkState: kun can't take the network offline or throttle it」；不留、不印），DevTools 的 Console 看得到：Chrome 沒有，是 kun 自己加的；browser 自己回答的那幾個由 browser 的 session 送，它也記下 `Log.enable`、`disable`。kun 自己其他的警告（解析錯誤、CSP 之類）之後再接。

### 7. Sources 的檔案樹

`Page.getResourceTree` 的 `resources` 列出每個 frame 的文件載入過的子資源（網址、類型、MIME 類型），一律記（很少的資料），和 Network 有沒有打開無關，和 Chrome 一樣。內容（`Page.getResourceContent`）從 `Network` 留著的 body 拿（「已決定的事項」第 18 條）：DevTools 一連上就打開 `Network`，之後載入的都拿得到；連上之前就載入的，Chrome 從快取拿，kun 沒有快取，給不出來（要重新整理）。DevTools 的 Sources 只從這份清單取圖片、字型、文件、fetch（前端的 `ResourceMapping.acceptsResource`），stylesheet 從 `CSS` domain 拿：kun 沒有，所以不列。

### 8. 生命週期

照 Blink 的 `InspectorPageAgent::LifecycleEvent`（「已決定的事項」第 25 條）：renderer 的 page agent 在 `Page.setLifecycleEventsEnabled` 打開、而且 `Page` 也打開時（Chrome 也要兩個都開），對每個 frame 送：
- `init`：新的文件，在 `frameNavigated` 之前；iframe 的初始 `about:blank` 也有；
- `DOMContentLoaded`、`load`：事件觸發時；
- `networkAlmostIdle`、`networkIdle`：照 Blink 的 `IdlenessDetector`（`renderer` 的 `idleness_detector`）：從 `DOMContentLoaded` 起，文件進行中的請求（子資源和 `fetch()`）不超過 2 個、0 個，持續 500 ms；時間是安靜開始的那一刻。render thread 在每個 task 之後數（Blink 在請求開始、結束時數），下一次可能到的時間也排進事件迴圈。

沒有 `src` 的 iframe 留在初始文件，Chrome 送它完整的一輪（Puppeteer 等每個 frame 的），kun 在它出現時送；文件要來的 iframe 的初始文件只有 `init`。

`firstPaint`、`firstContentfulPaint` 這份不做：kun 還沒有 paint timing（Blink 的 `PaintTiming`，網頁的 `PerformancePaintTiming`），連同網頁的 API 另寫一份 RFC（第 26 條）；Playwright 和 Puppeteer 等頁面載入都不看它們。Chrome 的 `firstMeaningfulPaint`、`firstMeaningfulPaintCandidate`、`InteractiveTime` 是舊的指標，不做。`commit` Chrome 執行時也不送（只在晚打開時補送），照做。同時：
- `Page.domContentEventFired`、`Page.loadEventFired`：main frame 的（DevTools 的 Network 面板畫藍線和紅線）；
- `Page.frameStartedLoading`、`frameStoppedLoading`：每個 frame。

browser 不自己送 lifecycle（原本送 main frame 的 `DOMContentLoaded`、`load`，renderer 的 `DidDispatchDOMContentLoaded` 跟著拿掉）；`Page.setLifecycleEventsEnabled` 照 `Page.enable` 的做法，browser 記下來之後轉給 renderer，分頁換了 renderer（當掉重開）時兩個都再交給新的 renderer（它補送的命令的回答不給 client）。每次 `setLifecycleEventsEnabled(true)`，renderer 照 Chrome 對每個 frame 現在的文件補送 `commit` 和已經發生的事件（`Page` 沒打開也補送）；`Page.disable` 也關掉 lifecycle 事件。這些規則開始前先用 Chrome for Testing 152 錄過。

machopodriver 的 Navigate To 等的是這兩個事件：browser 的 `load` 原本在 renderer 的 `DidFinishLoad`（顯示載入完的那個 frame 交出去）時才送，所以 machopodriver 回答時畫面已經畫好，kun 的 reftest 和情境的截圖靠它。renderer 的 `load` 照 Chrome 在 load 事件時送；machopodriver 等完 `load` 再呼叫 `Kun.waitForLoad`（browser 已經有、情境的執行器在用的命令：等分頁最新的導覽的 `DidFinishLoad`），回答時畫面一定畫好（「已決定的事項」第 30 條）。

### 9. `Page.navigate` 的回答

照 Chrome 的 `PageHandler::Navigate`（「已決定的事項」第 27 條）：browser 收到導覽的回應、要交給 renderer commit 之前才回答（Chromium 的 ready-to-commit；回應的 `Network` 事件在回答之前），回答 `frameId`、`loaderId`、`isDownload`（一律 false）；導覽失敗時帶 `errorText`（`net::ERR_NAME_NOT_RESOLVED` 這類，`NetErrorCode` 的名字）。被新的導覽取代時是 `net::ERR_ABORTED`，分頁關掉時是錯誤；Chrome 在同一份文件裡的導覽（只換 fragment）不帶 `loaderId`，kun 的 `Page.navigate` 一律重新載入文件，用不到（記在 `docs/limitations.md`）；kun 自己的 `about:` 頁沒有網路請求，交給 renderer 時回答。kun 失敗時照樣 commit 錯誤頁（Chrome 也是）。machopodriver 用回答裡的 `loaderId` 等載入完成，回答晚一點不影響它；回答帶 `errorText` 時，Navigate To 照 chromedriver 回答 `unknown error`（第 28 條）。

### 10. `Target`

- `Target.setAutoAttach`：回答成功。kun 的 iframe 都在同一個 renderer（沒有 out-of-process iframe）、沒有 worker，沒有會自動接上的 target；`waitForDebuggerOnStart` 也就沒有要停的（新分頁停住等 debugger 是 Playwright 的 RFC）。
- `Target.detachFromTarget`：拿掉 flat session，送 `Target.detachedFromTarget`。
- 分頁關掉時，連在它上面的 flat session 收到 `Target.detachedFromTarget`（直接連到分頁的連線照舊收到 `Inspector.detached`）。

### 11. 其他 `Network` 命令

DevTools 連上時送的命令（`details.md` 的「DevTools 前端連上時送的命令」）和使用者在 DevTools 裡改設定時送的都要有回答，不然它的 Network 面板會出錯。照 Chromium 分在 browser 和 renderer 回答（`details.md` 的「Chromium 的對照」第 10 項）：cookie 的命令、`clearBrowserCache`、`clearBrowserCookies`、`canClearBrowserCache`、`canClearBrowserCookies`、`emulateNetworkConditionsByRule`、`emulateNetworkConditions`、`canEmulateNetworkConditions` 由 browser 的 session 回答；`setBlockedURLs`、`overrideNetworkState`、`setAttachDebugStack`、`getRequestPostData` 由 renderer 的 network agent 回答；`enable`、`disable` browser 記下來之後也轉給 renderer；`setCacheDisabled`、`setBypassServiceWorker`、`setExtraHTTPHeaders` 由 renderer 回答。

kun 沒有的功能照參數回答（「已決定的事項」第 7 條）：

- **實際做的**：`enable`、`disable`、`getResponseBody`、`getRequestPostData`（POST 的 body 超過 `maxPostDataSize` 時 DevTools 用它補拿：network agent 留著請求的 body）、`setAttachDebugStack`（之後 script 要的請求帶 stack）。
- **要求的效果本來就成立，回答成功**：`setCacheDisabled`、`setBypassServiceWorker`（不管參數：沒有 HTTP cache、沒有 service worker）、`clearBrowserCache`、`clearBrowserCookies`（沒有東西可清）、`getCookies`、`getAllCookies`（空的清單）；`setBlockedURLs` 的清單是空的或每個 pattern 的 `block` 都是 false；`emulateNetworkConditionsByRule` 的 `matchedNetworkConditions` 是空的、不離線（回答 `ruleIds: []`）；`overrideNetworkState`、`emulateNetworkConditions` 不離線、沒有延遲和限速；`setExtraHTTPHeaders` 的 header 是空的。
- **要求 kun 做不到的事，回答錯誤**（`-32000`，訊息寫明 kun 不支援什麼）：封鎖網址、限速、離線、加 header。`Network.setUserAgentOverride` 不是 DevTools 的 Network 面板送的，照舊回答找不到方法（`Emulation` 的在 Playwright 的 RFC）。`canEmulateNetworkConditions` 照實回答 false。

DevTools 對這些命令的錯誤多半只記在自己的 log，畫面上看不到（選了「Slow 4G」選單照樣顯示選了）；另外送 `Log` 的警告讓 Console 看得到（第 6 節，「已決定的事項」第 17 條）。

### 測試

做了的測試和驗收的結果在 `progress.md`：
- **`shell` 的整合測試**：`devtools_network`（子資源和導覽的事件、redirect、失敗、`getResponseBody`、每個 session 各自的狀態、做不到的命令）、`devtools_initiators`（發起者）、`devtools_log`、`devtools_lifecycle`（生命週期、`Page.navigate`、`Target`）、`devtools` 的子資源；`webdriver`、`automation_pipe` 的 machopodriver 和 `Kun.waitForLoad`。用 `net::test_server` 給回應。
- **單元測試**：`net` 的 redirect、時間、解壓；`ipc` 的 `TimeTicks`；`bindings` 的 `stack_traces`；`renderer` 的 `log_agent`、`idleness_detector`、資源的清單。
- **`test.py cdp`**（和 Chrome 比）：`network`、`log`、`lifecycle` 和 Chrome 一樣（Chrome 自己要的 favicon 和 paint 的事件除外）。
- mutation test 每一段都跑（`mutants-1a.toml` 到 `mutants-3.toml`）。
- 手動確認（Chrome for Testing 152）：Network 面板（列表、Headers、Preview、Initiator 和它的 stack、Timing、redirect、失敗）、Console 的「Failed to load resource」和限速的警告、Sources 的子資源、Application 的 Frame Creation Stack Trace。

### 分階段

1. **Network 的事件和內容**：第 1 到 4 節、第 11 節，加上 `Page.domContentEventFired`、`loadEventFired`。驗收：Network 面板列得出請求、看得到 header 和內容；`test.py cdp` 的 `network`。
2. **發起者、`Log`、Sources 的子資源**：第 5 到 7 節。驗收：Initiator 有 stack；Console 有「Failed to load resource」；Sources 列出子資源。
3. **生命週期、`Page.navigate`、`Target`**：第 8 到 10 節。驗收：`test.py cdp` 的 `lifecycle`；`shell` 的測試照 Puppeteer `goto` 的流程走一遍（`Page.navigate` 的 `loaderId`、同一個 `loaderId` 的 `init`、`networkIdle`）。真的用 Puppeteer、Playwright 連上要 isolated world 和 browser 層級的 `setAutoAttach`，在路線的第 4 項驗收（第 29 條）。

三段都做完了：第 1 段做完時 merge 回 `main`，第 2、3 段審核之後一起 merge；各段的結果在 `progress.md`。

## 缺點

- **renderer 多一個 agent，載入的路上多幾個呼叫**：沒有 DevTools 時只是幾個判斷；`Network` 打開時留著每個回應的內容（共用、不複製，但網頁用完之後還活著，最多 200 MB），每個請求抓一次 stack（DevTools 開著才抓）。
- **redirect 多一來一回**：每一跳都要等 renderer 的 main thread（或導覽的 UI thread）回答，main thread 忙的時候 redirect 跟著等（和 Chrome 一樣，第 3 節）；`net`、IO thread 和 renderer 之間多兩種訊息。
- **時間少一段**：SSL 包在連線裡，分不開；要的話得自己寫 connector（第 4 節）。DNS 和連線的時間靠 task-local 記到請求上，依賴 hyper-util 在請求的 future 裡開連線，升級 reqwest、hyper-util 時要再確認（`net` 的測試守著）。
- **事件從兩個地方來**：導覽的一半在 browser、一半在 renderer，兩邊的 id 和時鐘要一致；Chromium 也是這樣，換來的是拆成 process 之後不用改。
- **生命週期改由 renderer 送，牽動 machopodriver**：browser 的 `load` 原本等畫面畫出來才送，machopodriver 的截圖靠它；renderer 照 Chrome 在 load 事件時送，machopodriver 另外等 `Kun.waitForLoad`（第 30 條），比 chromedriver 晚約一個 frame 回答；藏在後面的分頁不畫 frame，在那裡導覽要等它顯示出來（以前也是）。

## 理由和其他做法

- **照 Chromium 分在 browser 和 renderer，不全部放在 browser**：browser 的 IO thread 看得到每個子資源請求，全部由它送看起來比較簡單，但它不知道請求是哪個 frame 發的、誰發的（stack 只在 renderer 抓得到）、是不是 parser 發的；回應的內容留在 browser 也和 Chromium 相反（拆成 process 之後要另外搬）。完整的比較在 `details.md` 的「誰送事件的選項」。
- **redirect 每一跳回來問，不只是附在最後的回應裡或只通知**：只為了 DevTools 顯示，附上每一跳或當下通知就夠，但之後 renderer 要每一跳判斷的功能（CSP、mixed content）都要停下來問；現在就做成 Chromium 的樣子，之後只在 renderer 加檢查，不用再改協定，也不留下要拆掉的東西（「已決定的事項」第 4 條）。
- **不做原始的 header（`ExtraInfo`）**：要 `net` 拿到 reqwest 實際送出的 header（它自己加 `Host`、`Accept-Encoding`），DevTools 沒有它也能用。

## 先例

- **Chromium**：照做的：browser 和 renderer 的分工（`NetworkHandler`、`InspectorNetworkAgent`）、`NetworkResourcesData` 的上限、`BuildInitiatorObject` 的順序、`InspectorLogAgent` 的補送和 `FrameConsole` 的文字、`IdlenessDetector` 的定義、`Page.navigate` 在 ready-to-commit 時回答、redirect 每一跳由發出請求的一方說繼續。不照做的：`ExtraInfo`、CORS preflight 的那一列、SSL 和連線分開的時間。細節在 `details.md` 的「Chromium 的對照」。
- **Firefox**（WebDriver BiDi 的 `network` 模組、CDP 的實作已經拿掉）和 **Servo**（devtools 是 Firefox 的協定）：協定不同，沒有參考。

## 已決定的事項

2026-10-23（使用者）：

1. **範圍照「動機」列的**：DevTools 的 Network 面板（每個請求的事件、redirect 的每一跳、`getResponseBody`、DevTools 連上時送的 `Network` 命令都有回答）、發起者、`Log` 的 `network` 訊息、Sources 列出子資源、生命週期和 `Page.navigate` 照 Chrome、`Target.setAutoAttach`、`detachFromTarget` 和 `detachedFromTarget`。不做：cookie、HTTP cache、限速和封鎖請求的實際效果、`setExtraHTTPHeaders`、`Fetch`、`Emulation`、isolated world、`waitForDebuggerOnStart`（Playwright 的 RFC）、`ExtraInfo`、CORS preflight 的那一列、DNS 和連線的時間（後來由第 5 條改成做，只有 SSL 不分開：原本以為 reqwest 拿不到，查了之後它有 `dns_resolver` 和 `connector_layer`）、Elements。`Target` 和 `Log` 放在這份：`Target` 的三個都小，路線的第 2 項本來就有；`Log` 是 DevTools 看到載入失敗的地方，和 Network 的請求連在一起（`networkRequestId`）。
2. **Network 的事件照 Chromium 分在 browser 和 renderer（N1）**：導覽（main frame）的 `requestWillBeSent`、`responseReceived` 和導覽失敗的 `loadingFailed` 由 browser 的 session 送；子資源（包括 iframe 的文件）的全部事件、導覽的文件 commit 時的 `dataReceived` 和 `loadingFinished` 由 renderer 的 network agent 送（Blink 的 `InspectorNetworkAgent`；第 1 節）。renderer 知道請求是哪個 frame、誰發的、當時的 stack 和 parser 的位置；代價是兩邊的 `requestId`、`loaderId` 和時鐘要一致。不選 N2（全部在 browser）：IO thread 不知道 frame 和發起者，renderer 才知道的東西都要經過 `FetchResource` 帶過去，等於 agent 的一半還在 renderer，回應的內容也留在 browser，和 Chromium 相反。不選 N3（全部在 renderer）：導覽的請求只能等 commit 時補送，時間不對，失敗的導覽也要另外通知 renderer。比較在 `details.md` 的「誰送事件的選項」。
3. **回應的內容留在 renderer，`Network` 打開時才留（B1）**：照 Blink 的 `NetworkResourcesData`，總共 200 MB、每個 20 MB（`Network.enable` 的參數可以改），超過時丟掉最舊的，`disable` 和 main frame 導覽時清掉；導覽的文件的內容在 commit 時記（第 2 節）。沒有 DevTools 時不多用記憶體。不選 B2（browser 的 IO thread 留著）：跟著第 2 條，事件和內容在同一邊；Chromium 的 `getResponseBody` 也由 renderer 回答。

4. **redirect 每一跳停下來問（R2）**（第 3 節）：照 Chromium 的 `OnReceiveRedirect` 和 `FollowRedirect`，`net` 每走到一個能跟的 redirect 就停下來，交給發出請求的一方：子資源經過 IO thread 交給 renderer，renderer 告訴 network agent 之後回答繼續；導覽在 UI thread 上，session 送事件之後繼續。DevTools 的每一跳當下就送。代價是每一跳多一來一回，renderer 的 main thread 忙的時候 redirect 跟著等（Chrome 一樣），估計多 250 到 350 行（多半是測試）。換來的是之後 CSP 這類要 renderer 每一跳判斷的功能只要在 renderer 加檢查。不選 R1（`net` 把每一跳附在最後的回應裡，收到時補送）：事件在請求結束時才一起出現，之後做成每一跳回來問時，附上的清單要拆掉。不選 R4（每一跳當下通知、不等回答）：成本最小、執行時的行為完全不變，但之後要停下來問時還要再改一次。不選 R3（不顯示每一跳）：查 redirect 的問題時看不出來。

5. **時間量到 DNS 和連線（T3）**（第 4 節）：`net` 量開始、收到 header、收完，DNS 用 kun 自己的 resolver 量，建立連線（含 TLS）用 `connector_layer` 量，用 tokio 的 task-local 記到當時的請求上；重用的連線 DNS 和連線是 -1、`connectionReused` 是 true。1a 一開始先做原型確認時間記到對的請求（包括 HTTP/2），不行就退回只量三個時間點（T1）。估計 150 到 250 行（含測試）。不選 T1（只量三個時間點）：DNS 和連線的時間被算進等待伺服器，第一次連某個 host 時看起來像伺服器慢。不選 T2（自己寫 connector，連 SSL 也分開）：要自己維護連線和 TLS、跟著 reqwest 升級改，比這段其他部分加起來還大，換來的只是 SSL 分開。

6. **回應的位元組改用 `bytes::Bytes`（C3）**（第 2 節）：`ResourceResponse::body`（和導覽的 `CommitNavigation` 的 body）從 `Vec<u8>` 改成 `Bytes`，network agent 記內容時只多一個參考，不複製。`net` 收完時 `Bytes::from(vec)` 不複製；`bytes` 已經在依賴樹裡（reqwest、hyper），`protocol` 開它的 `serde`。要改拿走 body 的地方，約 15 到 30 處，多半是機械式的。不選 C1（照舊 `Vec<u8>`，記的時候複製）：沒有 DevTools 時一樣、改動最少（Blink 的 network agent 也是複製），但 DevTools 開著時每個回應多一份複本。不選 C2（`Arc<[u8]>`）：從 `Vec` 變成 `Arc<[u8]>` 要複製一次，每個回應都多付，不管有沒有 DevTools。

7. **kun 做不到的 `Network` 命令照參數回答（E1）**（第 11 節）：要求的效果在 kun 本來就成立時回答成功（沒有快取時的 `setCacheDisabled`、空的封鎖清單、不離線也不限速、空的 header），要求 kun 做不到的事時回答 `-32000` 的錯誤，訊息寫明不支援什麼；`canEmulateNetworkConditions` 照實回答 false。哪個命令由 browser、哪個由 renderer 回答照 Chromium。用 CDP 的程式會收到錯誤，不會以為限速、封鎖生效了；DevTools 的畫面上多半看不到這些錯誤，要不要另外送 `Log` 的警告在第 2 段決定。不選 E2（一律成功、不做事）：程式和人都會以為限速、封鎖生效了，查問題時誤導。不選 E3（整個命令回答找不到方法）：DevTools 每次連上都送 `emulateNetworkConditionsByRule`、要讀它的 `ruleIds`，Network 面板會出錯。

8. **導覽的事件經過外層程式的 callback 交給 UI thread（N-A）**（第 3 節）：`WebContents` 給外層程式的 callback 從帶 `RendererHostMsg` 改成帶 browser 這一側的事件 enum（`ContentsEvent`）：renderer 的訊息一種，導覽的事件一種。這段先做導覽的 redirect，1c 加回應和失敗（連同把導覽的 commit 移到 UI thread，照 Chromium 的 `NavigationRequest`），DevTools 的 session 也從這裡看到導覽。外層程式（視窗、headless、windowless、`browser_view` 和測試）跟著改，多半是機械式的。不選 N-B（給 `WebContents` 一個通用的「在 UI thread 上跑這段程式」）：一樣要改外層程式，closure 看不出會發生什麼事，DevTools 的 session 要另外接，kun 現在沒有這種寫法。不選 N-C（導覽的 redirect 在網路 thread 上直接繼續）：違反第 4 條，1c 還是要有這條路。不把導覽的事件放進 `RendererHostMsg`：被攻破的 renderer 會偽造它。

9. **Fetch 的 header 過濾搬到 renderer（H-b）**（1b，2026-10-23，使用者）：照 Chromium（network service 不過濾 header，Blink 的 `FetchResponseData` 交給 script 時才過濾）：`net` 交給 renderer 的回應帶著全部的 header（`Set-Cookie`、`Set-Cookie2` 除外：原本只有 CORS 的回應拿掉，改成每個回應都拿掉，cookie 不交給 renderer）和 tainting，CORS 的回應只留 safelisted 和 `Access-Control-Expose-Headers` 列出的 header、`no-cors` 的 `fetch()` 是 status 0 沒有 header，這兩件搬到 `dom_api` 的 `fetch` 做；`no-cors` 的 `fetch()` 的 body 照舊在 `net` 清掉（kun 沒有 ORB，不把跨源的 body 交給 renderer）。DevTools 在 renderer 看得到全部的 header，和 Chrome 一樣。代價：過濾的程式和測試從 `net` 搬到 `dom_api`，renderer 拿得到跨源回應的 header（Chrome 的 renderer 也拿得到）。不選 H-a（照舊在 `net` 過濾）：DevTools 少了跨源回應的大部分 header。不選 H-c（DevTools 打開時才另外附上沒過濾的 header）：browser 要知道哪個 renderer 開了 DevTools，Chromium 沒有這種做法。
10. **每個 session 各自記 `Network` 的狀態（S2）**（1b，2026-10-23，使用者）：network agent 一個 renderer 一個，每個打開 `Network` 的 session 各自記上限、留著的回應（`Bytes` 共用，只多參考）和有沒有打開，和 Chrome 每個 session 一個 `InspectorNetworkAgent` 一樣；一個 client 的 `disable` 不影響別的。不選 S1（所有 session 共用一份記錄）：上限只能有一組，一個 `disable` 清掉別人的記錄。
11. **子資源的 `requestId` 是 renderer 的前綴加上 request id**（1b，2026-10-23，使用者）：renderer 給子 frame 的 id 用的隨機前綴，加上 renderer 裡的 request id（`<前綴>.<id>`，Chrome 是 process 的編號加序號）；renderer 重開時換前綴，不重複。導覽的照舊是 browser 的 navigation id。iframe 的文件原本定的是文件的號碼，做的時候發現請求送出時文件還沒有號碼（commit 時才建），改成那個請求的 id，同時是那份文件的 `loaderId`（和 Chrome 一樣：導覽的 `requestId` 和 `loaderId` 相同），`Page.frameNavigated` 的子 frame 的 `loaderId` 跟著改（2026-10-23）。不選另抽一個前綴：一個 renderer 兩種前綴，沒有好處。

12. **時間用作業系統的單調時鐘（T-b），放在新的 `ipc` crate（T-b1）**（1b，2026-10-23，使用者）：照 Chromium 的 `base::TimeTicks`，`ipc::TimeTicks` 讀作業系統的單調時鐘：Linux、macOS 用 `rustix` 的 `clock_gettime(CLOCK_MONOTONIC)`（安全的 API），Windows 用 `QueryPerformanceCounter`（`unsafe`，附 `SAFETY`）。每個 process 讀同一個時鐘，`net`、browser 的 session、renderer 的事件不用交接起點；1a 的 `LoadTiming` 改成 `TimeTicks`，`NetworkContextParams::time_origin` 拿掉。`ipc` 是之後 multi-process 的 IPC 用的 crate（`unsafe` 原本就允許在那裡），現在先放時鐘：跨 process 的時間戳是 IPC 的一部分；它的 soundness 論證和 crate 一起寫。不選 T-a（browser 建一個起點、啟動 renderer 時交接經過的時間）：不用 `unsafe`，但每個要對時間的地方都要拿到起點，renderer 的起點差了啟動訊息在路上的時間。不把 `unsafe` 放進 `base`：要改 `unsafe` 只在 `dom`、`bindings`、`ipc` 的規則。
13. **`net` 自己解壓，傳輸大小照壓縮前算（E-d）**（1b，2026-10-23，使用者）：Chrome 的 `encodedDataLength` 是 network service 在連線那一層數的每個請求實際收到的 bytes（`URLRequest::GetTotalReceivedBytes`，header 加上壓縮前的 body）。kun 數不到連線上的 bytes（hyper 的連線 task 讀，HTTP/2 好幾個請求混在一條連線上），而 reqwest 的自動解壓（tower-http）連 `Content-Encoding`、`Content-Length` 都拿掉，原本建議的「用 `Content-Length`」不成立。改成關掉 reqwest 的自動解壓，`net` 收完整份 body 之後自己解（`gzip`、`deflate`、`br`，用依賴樹裡的 `flate2`、`brotli`；壞掉的資料照 Chrome 是 `net::ERR_CONTENT_DECODING_FAILED`），`Accept-Encoding` 由 `net` 送；`ResourceResponse` 多帶壓縮前的 body 長度和估算的 header 長度（照 HTTP/1 的寫法，HTTP/2 的 HPACK 壓縮不算）。`Content-Encoding`、`Content-Length` 留在 header 裡，`fetch()` 和 Chrome 一樣讀得到（原本讀不到）。代價：約 150 行加測試，header 改變之後跑 `fetch/` 的 WPT 看預期結果。不選 E-a（用解完的長度）：壓縮過的回應的傳輸大小偏大，`fetch()` 照舊讀不到那兩個 header。不選 E-b（在連線上數）：做不到。

14. **導覽的 commit 移到 UI thread（C1）**（1c，2026-10-23，使用者）：照第 8 條原本的寫法和 Chromium 的 `NavigationRequest::OnResponseStarted`：網路 thread 把導覽的結果（回應或失敗）用 `ContentsEvent` 交給 UI thread，`WebContents` 在那裡送 DevTools 的 `responseReceived`、`loadingFailed`，再叫 renderer commit（失敗時照舊 commit 錯誤頁）。查的時候發現事件的順序其實不需要搬 commit（只通知 UI thread 也排得對），但第 3 段的 `Page.navigate` 要在 commit 前回答，之後 commit 前要做的檢查也只放在這一個地方。代價：UI thread 忙的時候 commit 跟著等（Chrome 一樣），body 經過 UI thread（`Bytes`，不複製），外層程式和測試工具多處理一種事件。不選 C2（commit 照舊在網路 thread 上，只另外通知 UI thread）：改動小，但第 3 段還是要搬。
15. **`Page.domContentEventFired`、`loadEventFired` 由 renderer 送（D1）**（1c，2026-10-23，使用者）：renderer 的 `Page` agent 在 main frame 的 `DOMContentLoaded`、`load` 時送給打開 `Page` 的 session，照 Blink 的 `InspectorPageAgent`，和第 3 段的生命週期由 renderer 送的方向一致。不選 D2（browser 收到 `DidDispatchDOMContentLoaded`、`DidFinishLoad` 時送）：第 3 段照 renderer 送生命週期時要搬。

16. **script 的 stack 用 V8 的 inspector 抓（I2）**（第 2 段，2026-10-24，使用者）：fork 加 `V8Inspector::captureStackTrace` 和把 `V8StackTrace::buildInspectorObject` 轉成 JSON 的 binding（`buildInspectorObject` 只能序列化成 CBOR，用 V8 裡的 `v8_crdtp::json::ConvertCBORToJSON` 轉），和 Chrome 一模一樣，包括 async 的呼叫鏈。改 fork 的 C++，要重編 V8、發新的 release。第 2 段一開始先做原型確認做得到，不行就退回 I1，先說明原因。不選 I1（rusty_v8 已有的 `StackTrace::current_stack_trace` 自己組）：不改 fork，但沒有 async 的部分，`await` 之後發的 `fetch()` 看不出是誰呼叫的。不選 I3（不抓）：script 發的請求只有 `other`。
17. **`Log` 用 console 的格式印到 stderr（G1），做不到的命令另外送警告（W1）**（第 2 段，2026-10-24，使用者）：「Failed to load resource」照 Chrome 的 `--enable-logging=stderr` 當成 console 訊息印（`[console] 文字 (網址)`）。kun 做不到的 `Network` 命令回答錯誤時，另外送一則 warning 等級的 `Log.entryAdded`，DevTools 的 Console 看得到（Chrome 沒有，是 kun 自己加的；browser 自己回答的由 browser 送，它也記下 `Log.enable`）。不選 G2（不印）：不開 DevTools 時看不到載入失敗。不選 G3（另一個前綴）：Chrome 沒有這種分法。不選 W2（不送警告）：限速、封鎖沒生效，DevTools 的畫面上看不出來。
18. **子資源的內容從 `Network` 留著的 body 拿（S1）**（第 2 段，2026-10-24，使用者）：子資源的清單（網址、類型、MIME）每份文件一律記；`getResourceContent` 的內容從 `Network` 留著的 body 拿，沒有 DevTools 時不多花記憶體。DevTools 連上之前就載入的給不出來（要重新整理；Chrome 從快取拿）。原本看法裡的「從文件還留著的東西拿 stylesheet」查了之後不成立：stylesheet 解析完不留原文，圖片也只留大小和狀態。不選 S2（每份文件一直留著所有子資源的 body）：和 Chrome 一樣先載入的也看得到，但沒有 DevTools 也要多留每個 stylesheet、script 的原文。
19. **`Page.frameAttached` 帶建立它的 script 的 stack**（第 2 段，2026-10-24，使用者）：script 建立 iframe 時用第 16 條的做法抓 stack，DevTools 的 frame 詳細資料顯示「Created by」。0075 留下的「之後的可能」，抓 stack 的方法有了之後只多一個欄位。
20. **engine 交給 renderer 抓 stack 的函式（A1）**（第 2 段，2026-10-24，使用者）：`dom_api::ScriptEngine` 給 renderer 一個函式，V8 的那一個拿著 isolate 的 inspector 的 `Weak`（`bindings`，不用 `unsafe`），wasm 的沒有；renderer 的 `FrameClient`（`FrameRequests`）在 dom_api 要求載入的當下呼叫它，只在有 session 打開 `setAttachDebugStack` 時。和 Blink 在 `WillSendRequest` 裡抓一樣，抓的地方和 script 的接口分開。不選 A2（`ScriptHost::capture_stack`，dom_api 傳進每個載入的呼叫）：要改的簽名多，dom_api 也得知道 DevTools 的開關。
21. **晚一步發的載入在 dom_api 要求的當下抓 stack（B1）**（第 2 段，2026-10-24，使用者）：`fetch()`、`request_image`、`script_prepared`、`navigate_child_frame`、`import_module` 的當下抓，依元素（圖片依網址，第一個要它的）記著，真正發請求時用，一輪沒用上的丟掉；`<link>` 補一個 `FrameClient` 的接口（dom_api 的屬性變動和插入的步驟呼叫）。`frameAttached` 的 stack 在 `did_create_child_frame` 抓。Blink 在插入的當下就發請求、抓 stack（`<img>` 等到 microtask 時，用 `AsyncTaskContext` 把當時的 stack 接成 async 的 parent），看到的一樣。不選 B2（只有 `fetch()` 和 `<img>`）：script 插入的 `<script>`、`<link>`、`<iframe>` 看不出是誰加的，正是動態載入時最想看的。
22. **parser 發的請求用元素的開始標籤的行（C1）**（第 2 段，2026-10-24，使用者）：parser 建立元素時已經把行號交給 dom_api 的 `attribute_changed`（`<script>` 另外記在 `script_lines`），在第 21 條的接口上和 stack 記在一起，發起者是 `parser` 帶文件的網址和 `lineNumber`（0 起算）；kun 的 tokenizer 不記欄，不帶 `columnNumber`。Blink 用 preload scanner 掃到的 token 的位置，沒有時用 parser 當下的位置。parser 插入的元素以外，沒有 stack 的是 `other`。不選 C2（kun 發請求時 parser 所在的行）：kun 在 parser 停下時才一起發，那一行往往離元素很遠。不選 C3（不帶行號）：Initiator 點下去只到文件的開頭。
23. **CSS 的網址帶著 referrer（D1）**（第 2 段，2026-10-24，使用者）：照 Blink 的 `CSSUrlData::referrer_`，`css` 的 `CssUrlData` 加 `referrer`（外部 stylesheet 的網址；`<style>`、`style` 屬性是文件的網址，不是 base URL），算好的 `BackgroundImage::Url` 改成帶網址和 referrer，`take_background_image_urls` 交出兩者；發起者是 `parser` 帶 referrer，不帶行號和 stack（Blink 對 CSS 要的不抓 stack：那可能是觸發重算 style 的 script，crbug.com/918196）。每個 `url()` 多一個 `Arc<str>`。不選 D2（`style` 另外記網址 → referrer 的表）：同一個網址在兩份 stylesheet 時不一定對，表要跟著 stylesheet 的生命週期清。不選 D3（一律文件的網址）：外部 stylesheet 要的圖是錯的。不選 D4（`other`）：看不出是 CSS 要的。
24. **fork 先發 release（E1）**（第 2 段，2026-10-24，使用者）：第 16 條的 binding 在第 2 段的其他部分之前 commit、push，打 `kun-v152.2.0-19`（release 的 workflow 要跑幾個小時）；跑完之後 kun 的 `RUSTY_V8_MIRROR_TAG` 換過去。第 3 段（生命週期、`Page.navigate`、`Target`）預計不再改 fork。不選 E2（第 2 段做完再發）：等 workflow 的時間全落在最後。

25. **生命週期全部由 renderer 送（L1）**（第 3 段，2026-10-24，使用者）：照 Blink，renderer 的 page agent 對每個 frame 送 `init`、`DOMContentLoaded`、`load`、`networkAlmostIdle`、`networkIdle` 和 `frameStartedLoading`、`frameStoppedLoading`；browser 拿掉自己送的 `DOMContentLoaded`、`load`，記下 `setLifecycleEventsEnabled` 之後轉給 renderer。renderer 知道每個 frame 的載入和進行中的請求，iframe 的事件、`loaderId` 和 `Page.frameNavigated` 的一致。不選 L2（browser 留著那兩個）：同一種事件從兩個地方來，iframe 的還是要 renderer 送，順序要兩邊對齊。
26. **`firstPaint`、`firstContentfulPaint` 這份不做（F3）**（第 3 段，2026-10-24，使用者）：連同網頁的 `PerformancePaintTiming` 另寫一份 RFC（compositor 的 `SwapPromise` 已經知道 commit 是哪個 frame 畫出來的）。Puppeteer、Playwright 等頁面載入都不看它們，DevTools 的 Performance 面板也不靠。不選 F1（這份做 paint timing 和網頁的 API）：是一個完整的功能，工作量像一份小 RFC。不選 F2（只送事件）：之後兩邊的定義要再對一次。
27. **`Page.navigate` 在 ready-to-commit 回答（P1）**（第 3 段，2026-10-24，使用者）：照 Chrome 的 `PageHandler::Navigate`，回答等到 UI thread 上拿到導覽的結果、交給 renderer commit 之前（第 1 段的 `ContentsEvent::NavigationCompleted`）；失敗帶 `errorText`，被取代是 `net::ERR_ABORTED`，同一份文件裡的導覽不帶 `loaderId`，kun 的 `about:` 頁交給 renderer 時回答。不選 P2（照舊一開始就回答）：client 拿不到 `errorText`，Puppeteer 的 `goto` 連不上時不會丟錯誤。
28. **導覽失敗時 machopodriver 回答錯誤（K1）**（第 3 段，2026-10-24，使用者）：`Page.navigate` 帶 `errorText` 時，Navigate To 照 chromedriver 回答 `unknown error`，訊息帶 `net::ERR_…`（geckodriver 也回錯誤）；iframe 載入錯誤頁不算（wdspec 的 `navigate_to/frame.py`）。不選 K2（照舊等錯誤頁載入完、回答成功）：和兩個主要的 driver 都不一樣。
29. **第 3 段的驗收不用 Puppeteer、Playwright（V1）**（第 3 段，2026-10-24，使用者）：做的時候查到 Puppeteer 連上要 browser 層級的 `Target.setAutoAttach` 接上分頁，每個分頁還要 `Page.addScriptToEvaluateOnNewDocument` 帶 `worldName`（isolated world），失敗時整個初始化失敗（`FrameManager.initialize`），都是路線第 4 項的事。驗收改成 `test.py cdp` 的 `lifecycle` 和照 Puppeteer `goto` 的流程走一遍的 `shell` 測試；真的連上在路線第 4 項驗收。不選 V2（這份補 browser 層級的 `setAutoAttach` 和 isolated world）：把第 4 項的一大塊提前做，範圍差不多翻倍。

30. **renderer 的 `load` 照 Chrome，machopodriver 另外等畫面（W2）**（第 3 段，2026-10-24，使用者）：做的時候發現 machopodriver 等的是 browser 的 `load`，它在 `DidFinishLoad`（顯示載入完的那個 frame 交出去）時才送。renderer 的 lifecycle `load` 照 Chrome 在 load 事件時送；machopodriver 等完 `load` 再等畫面（做的時候發現 browser 已經有 `Kun.waitForLoad`，等分頁最新的導覽的 `DidFinishLoad`，就用它，不另外加事件），回答時畫面一定畫好（WebDriver 規範只要求文件載入完，多等不違反；chromedriver 不等，晚約一個 frame）。不選 W1（renderer 在 frame 交出去時才送 `load`）：DevTools 看到的 `load` 比 `loadEventFired` 晚一個 frame，和 Chrome 不同。不選 W3（machopodriver 只等 `load`，靠 `captureScreenshot` 自己畫新的 frame）：和 chromedriver 一樣，但 load 事件之後才完成的事（圖片解碼、背景圖）可能趕不上那個 frame，reftest 和情境的截圖可能不穩。

## 待決定

沒有。

## 之後的可能

- Elements（`DOM`、`CSS`、`Overlay`）和 Playwright（isolated world、`waitForDebuggerOnStart`、`Emulation`、`Fetch`、`setExtraHTTPHeaders`），路線的第 3、4 項。Puppeteer 連上也要 isolated world 和 browser 層級的 `Target.setAutoAttach`（「已決定的事項」第 29 條）。
- **paint timing**：`firstPaint`、`firstContentfulPaint` 的 lifecycle 事件和網頁的 `PerformancePaintTiming`（第 26 條）。
- **限速和封鎖請求**：`emulateNetworkConditions`、`setBlockedURLs` 的實際效果（在 `net` 延後或拒絕請求）。
- **CORS preflight 的那一列**、`ExtraInfo`（實際送出的 header）。
- **SSL 分開的時間**：自己寫 connector（TCP 加 rustls）。
- **kun 自己的其他警告接到 `Log`**：解析錯誤、mixed content、CSP（有了之後）。
- **`Network.loadNetworkResource`**：DevTools 用它載入 source map。
- **HTTPS 的 HTTP/2 不再同時開好幾條連線**：連線池在第一條連上之前不知道會是 HTTP/2，同時發出的請求各開一條，最後只用一條（`timing-prototype/` 看到五條開了、用一條；`docs/limitations.md` 的「網路」）；Chrome 的 `SpdySessionPool` 等第一條。和 DevTools 無關，是 kun 現在就有的行為。
