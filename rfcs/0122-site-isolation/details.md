# RFC 0122 的細節：site isolation 的路線圖

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## machopo 的現況（盤點）

2026-11-27 讀的（`main`，`ecac4005`）。

### process 和 frame

- **一個分頁一個 renderer process，從頭用到尾**：`browser` 的 `web_contents.rs` 只在 renderer 不在了之後才啟動下一個（`crashed`：「the next navigation launches another」）。同一個分頁從 A 網站導覽到 B 網站，B 在同一個 renderer 裡；renderer 的 thread-local 的表（名稱表、屬性值的表、圖片的快取）跨網站留著（`docs/limitations.md` 的「隔離」）。
- **跨網站的 iframe 和頁面在同一個 renderer**，只靠 V8 的 security token 擋住 script 互相存取（`docs/limitations.md` 的「iframe」）。
- **子 frame 沒有自己的 frame sink**：子 frame 照 iframe 的內容框跑 style、layout，畫進父 frame 的 paint，同一個 compositor、同一個 `CompositorFrame`。Viz 的 `SurfaceAggregator` 會嵌入 surface（`SurfaceDrawQuad`），但現在只有 Browser 的 UI 嵌分頁的 surface 這一層；renderer 送的 frame 裡沒有嵌別的 surface 的 quad。
- **Browser 的 frame 樹**（`frame_tree.rs`）記每個 frame 的 parent、children、網址、origin、session history 的位置；沒有「這個 frame 在哪個 process」（都在分頁的那一個）。
- **沒有 `window.open`**，`opener` 一律是 `null`；連結的 `target` 沒有對應的 frame 時開新分頁（新的 renderer），兩邊沒有 script 的關聯。所以現在每個分頁就是一個 browsing context group。
- `--single-process` 和在測試的 process 裡建的分頁（`RendererLauncher::InProcess`）：renderer 是 Browser 的 thread；`RendererLauncher::Detached` 是沒有 renderer 的（測試和 fuzz 直接餵訊息）。

### renderer 能向 Browser 要什麼、哪些檢查過

`protocol::renderer::RendererHostMsg` 的訊息（約 43 種），照「要的是誰的資料」分：

| 訊息 | Browser 現在怎麼知道是誰的 | 檢查 |
| --- | --- | --- |
| `FetchResource`、`FollowRedirect`、`CancelResource` | 照 loader：Browser 在每份文件 commit 時建一個 loader，帶著那份文件的 origin 和 `IsolationInfo`（`frame_tree.rs`，從 Browser 自己的 frame 樹算最上層的網站和 site for cookies） | 沒給過的 loader、initiator 和 loader 的 origin 不同、帶了只有 Browser 能給的參數、要的是文件：壞訊息（`UnknownLoader`、`InitiatorMismatch`、`TrustedParamsFromRenderer`、`DocumentRequest`） |
| `GetCookies`、`SetCookie`、`CookiesEnabled` | 照 loader 的 origin | 網址和 loader 的 origin 不同、opaque 的 origin：壞訊息 |
| `OpenStorageArea`、`ChangeStorage` | 照 frame：Browser 從 frame 樹取那個 frame 的 origin 和最上層的網站，組成 `StorageKey` | frame 不在樹上時回答空的（`docs/test-gaps.md`） |
| `DidCommitNavigation`、`DidCommitSameDocumentNavigation` | Browser 送去 commit 的那一筆 | 不是 Browser 送的、網址不同、同一份文件內換到別的網址：壞訊息 |
| `DidCreateChildFrame`、`DidDetachFrame`、`DidChangeSrcDoc`、`UpdateState` | frame 的 token | 不在樹上的 frame、重複的 token：壞訊息 |
| `RegisterBlobUrl`、`RevokeBlobUrl` | renderer 給的網址字串（`blob:<origin>/<uuid>`），記在 renderer 的編號底下 | 網址裡的 origin 沒有對照 renderer 有的文件 |
| `DedicatedWorkerCreated` | worker 的編號，Browser 抓主 script、建它的 loader | worker 的請求走別的 loader：壞訊息 |
| `BeginNavigation`、`OpenUrl`、`GoToEntryAtOffset` | 發起的 frame | 不在樹上的 frame：壞訊息；發起者的 origin 有沒有另外對，沒查 |
| `GetFontFamily`、`GetFontFallback`、`GetFontFiles` | 不分網站 | 編號是 Browser 給過的 |
| 其餘（游標、title、input 的 ack、測試和 DevTools 的回答） | 不涉及別的網站的資料 | |

- **現有的檢查是「照文件」的**（loader 的 origin），不是「照 process」的：同一個 renderer 拿著它每一份文件的 loader（跨網站的 iframe 的也是），被攻破的 renderer 可以用任何一個。
- **`window.postMessage`、`MessagePort`、`BroadcastChannel`**：都在 renderer 裡（同一個 renderer 的 window 之間），不經過 Browser；沒有 `BroadcastChannel`。
- **壞訊息的處理**（`web_contents.rs` 的 `received_bad_message`）：結束那個 renderer、分頁顯示 sad tab。

### 資料照什麼分

- **cookie**：第三方的不送也不收（請求的 site for cookies 不是它的網址的網站時），沒有開關；`Partitioned` 的照最上層的網站分。`HttpOnly` 的不給 script。
- **`localStorage`、`sessionStorage`**：`StorageKey`（origin 加最上層的網站）。
- **HTTP cache**：照最上層和 frame 的網站分開。
- 所以**嵌在別的網站底下的 iframe 拿不到它的網站的第一方資料**：沒有 cookie、storage 是另一個分區。

### 沒有的

- **沒有 ORB（或 CORB）、沒有 `Cross-Origin-Resource-Policy`**（`net`、`browser`、`renderer` 裡都查不到）：`no-cors` 的跨源請求（`<img>`、`<script>`）的回應，不管是什麼內容都整個交給 renderer。
- **沒有 `X-Frame-Options`、CSP**（包括 `frame-ancestors`）：任何文件都能被任何網站嵌進 iframe。
- **沒有 iframe 的 `sandbox`、`allow`**；沒有 `document.domain`；沒有 COOP、COEP（也沒有 `SharedArrayBuffer`）；不分派 `beforeunload`。
- `MessagePort` 的 pipe 不能跨 renderer（`docs/limitations.md` 的「事件」）。

### Browser 會不會被 renderer 弄倒

- **wgpu**：`viz` 只裝了 device 遺失的 callback（`gpu_context.rs`），沒有 error scope、沒有 uncaptured error 的 handler：驗證錯誤照 wgpu 的預設結束程式。Viz 收到 frame 時檢查帶像素的 resource（大小、長度），沒有涵蓋所有會走到 wgpu 的值（`docs/limitations.md` 的「隔離」）。
- **panic**：`browser` 處理 renderer 訊息的幾個檔案，測試以外的 `unwrap`、`expect`、`panic!`、`unreachable!`：`render_process_host.rs` 6、`navigation_controller.rs` 8、`frame_tree.rs` 3、`dom_storage.rs` 2、`web_contents.rs` 1、`loader.rs` 0（只數了字面，沒有逐一看是不是 renderer 的輸入走得到；索引越界和整數溢位沒數）。
- 有一個 Browser 訊息的 fuzzer（`python3 scripts/test.py browser fuzz`），CI 不跑（`docs/test-gaps.md`）。

## Chromium

2026-11-27 讀的：Chromium 的文件「Process Model and Site Isolation」（下面的 P）和「Compromised renderers」（C），Chrome 的 `document.domain` 那篇（B）；網址在最後的「參考」。引文照原文。

### 名詞（P）

- **site**：「scheme plus eTLD+1, such as `https://example.com`」。為什麼不是 origin：「different origins within a given site may have synchronous access to each other if they each modify their document.domain」。
- **principal instance**（程式裡的 `SiteInstance`）：「the core unit of Chromium's process model」，同一個裡的文件「must live in the same process, because they have synchronous access to each other's content」。
- **browsing context group**（`BrowsingInstance`）：「a group of tabs and frames ... that have references to each other」，而且「there is only one principal instance per principal in a given browsing context group」。分頁在某些導覽時換 group（「due to a Cross-Origin-Opener-Policy header, browser-initiated cross-site navigations」）。
- 同一個 principal、同一個 group，就在同一個 process；理由：「If such documents were in different processes, data races or deadlocks would occur」。

### 鎖和檢查（P、C）

- **process lock**：「to restrict which sites are allowed to load in the process and which data the process has access to」；可以是 site、origin、scheme（`file://`），或「allow-any-site」。「ProcessLocks are always assigned before any content is loaded in a renderer process」，之後「remains constant for the lifetime of the RenderProcessHost」。
- **jail 的檢查**：「Ensure that a process locked to a particular site can only access data belonging to that site」。**citadel 的檢查**：「Ensure that unlocked processes cannot access data for sites that require a dedicated process」。
- Browser 「can monitor IPC messages from renderer processes to limit their actions or access to site data (e.g., using ChildProcessSecurityPolicy::CanAccessDataForOrigin)」。
- 網路：CORB 或 ORB，鎖住的 process 只從自己的 site 拿得到「sensitive data (e.g., HTML, XML, JSON)」。
- C 列的、Browser 這一側守的：跨源的 HTTP 回應（CORB/ORB，在 network service；loader 的參數由 Browser 控制）、cookie、密碼、web storage、訊息（`postMessage` 的 origin 和 targetOrigin、`BroadcastChannel`）、JavaScript 的 code cache、`Cross-Origin-Resource-Policy`、`X-Frame-Options`（「enforced in the browser process via `content::AncestorThrottle`」）、請求的 header（`Host`、`Origin`、`Sec-Fetch-Site`）。例外：「`frame-ancestors` is enforced in a renderer process」。
- C 自己列的缺口：CORB 不管的內容型別（`image/png`、`application/octet-stream`）預設不保護；`file:` 的 frame 可能共用 process；SameSite cookie 和 user activation 是「not protected today」；sandbox 的 iframe 沒有和它的 precursor origin 隔離（P 另外說桌面版從 127 起把 sandbox 的 iframe 放到別的 process，兩份沒有對起來）。

### 幾種模式（P）

- **完整的 site isolation**：「each process is locked to documents from a single site」，桌面版。
- **部分的**（2 GB 以上的 Android）：「only uses dedicated (locked) processes for some sites, putting the rest in unlocked processes」（使用者登入過的、輸入過密碼的、OAuth、有 COOP 的；「usually creates one unlocked process per browsing context group」）。
- **沒有**：記憶體小的 Android、WebView、iOS 的 Chrome；這種時候不開 OOPIF，「to avoid requiring multiple renderer processes in a given tab」，但 Browser 發起的跨網站導覽照樣換 process。
- 重用：超過軟性的上限時同一個 site 的 process 開始重用；OOPIF 平常就積極重用同一個 site 的 process。

### 特例（P）

- `about:blank`：一開始可以是「allow-any-site」的鎖，第一份真的內容 commit 時收成那個 site 的；繼承了 origin 的 `about:blank` 可以在鎖住那個 site 的 process 裡。
- `data:`：「generally keeps documents with `data:` URLs in the same process as the site that created them」。
- 錯誤頁：自己的 process（只對 main frame）。`file:`：全部當成同一個 site。
- **origin isolation**：「Origin-Agent-Cluster is now enabled by default, effectively disabling changes to document.domain unless an OAC opt-out header is used」；它是提示，「not a security guarantee」。B：Chrome 115 起逐步拿掉 `document.domain` 的 setter 的作用（不丟例外，只是沒有效果），`Origin-Agent-Cluster: ?0` 可以退出。
- **cross-origin isolated**：要 COOP 加 COEP，「ensures that all cross-origin content ... consents to being loaded in the same process」。

### 沒讀到的

P 和 C 裡沒有：`RenderFrameProxy`、`RemoteFrame`、local root 的 widget（OOPIF 的機制）、導覽時決定換不換 process 的逐步規則、`blob:` 和 `srcdoc`。要做到那幾份時讀原始碼（`content/browser/renderer_host/render_frame_host_manager.cc`、`site_instance_impl.cc`、`navigation_request.cc`、`cross_process_frame_connector.cc`）。

## 其他瀏覽器（2026-11-27）

- **Firefox**：Project Fission，照記憶 2021 年底的 Firefox 95 在桌面版預設開啟；沒有對文件。
- **Safari**（WebKit）：還在做。查到的是 2026 年 WebKit 的 bug（測試用的「模擬預設開啟」的旗標、iOS 的載入、多 process 的上一頁快取、macOS 上開啟時失敗的測試），沒有「預設開啟」的正式說法；WebKit 的文件有 Site Isolation 的專章。

## 規模的估法

拿 RFC 0004（renderer 拆成 process）當 1：56 個 commit，`rfcs/` 以外加了約 3.5 萬行（含新的 `ipc` crate）。現在 frame 相關的程式：`browser` 的 `frame_tree.rs`、`navigation_controller.rs`、`web_contents.rs`、`render_process_host.rs` 合計約 6,800 行，`FrameId` 出現在 24 個檔案、391 處。估計的不確定性大，尤其是 OOPIF 的畫面和 input 那兩份。

## 沒有查到的

- `performance.now()` 的精度、有沒有其他高精度的計時來源（Spectre 要的）。
- 請求的 `Origin`、`Sec-Fetch-*`、`Referer` 是 renderer 還是 Browser 填的。
- `browser` 的那些 `unwrap` 有幾個是 renderer 的輸入走得到的；Viz 的檢查還缺哪些值。
- Chromium 決定換不換 process 的規則和 OOPIF 的機制（上面的「沒讀到的」）。

參考：

- [Process Model and Site Isolation（Chromium）](https://chromium.googlesource.com/chromium/src/+/main/docs/process_model_and_site_isolation.md)
- [Compromised renderers（Chromium）](https://chromium.googlesource.com/chromium/src/+/main/docs/security/compromised-renderers.md)
- [document.domain 的 setter 的停用（Chrome）](https://developer.chrome.com/blog/document-domain-setter-deprecation)
- [Site Isolation（WebKit 的文件）](https://docs.webkit.org/Deep%20Dive/SiteIsolation.html)
