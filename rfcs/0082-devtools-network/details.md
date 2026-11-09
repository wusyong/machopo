# RFC 0082 的細節

## RFC 開始前（2026-10-23）kun 的載入怎麼走

- **導覽**（main frame）：browser 的 `WebContents` 建 `ResourceRequest`（`RequestDestination::Document`），交給 `net` 的 `NetworkContext::fetch`；回應整份收完之後，連同 body 放進 `RendererMsg::CommitNavigation` 交給 renderer。browser 有 navigation id（`Page.lifecycleEvent` 和 `Page.navigate` 的 `loaderId`），renderer 看到的是 commit 的參數（網址、MIME 類型、charset、body、`since_navigation_start`），沒有 header 和狀態碼。
- **子資源**：renderer 送 `RendererHostMsg::FetchResource { request_id, request }`，browser 的 IO thread（`render_process_host.rs`）直接交給 `net`，不經過 UI thread；回答是 `RendererMsg::ResourceResponse { request_id, result }`，`CancelResource` 取消。送出的地方有兩個：`render_thread.rs` 的 `fetch`（script、stylesheet、圖片、iframe 的文件，`frame_view.rs` 決定 destination）和 `loader.rs` 的 `FetchLoader::start`（`fetch()`）。收到的地方一個：`RenderThread::did_receive_resource`。`request_id` 在 renderer 裡唯一（`RequestIds`）。
- **iframe 的文件**：renderer 自己發請求（`RequestDestination::Iframe`），commit 也在 renderer 裡；`srcdoc` 和 `about:blank` 不經過網路。
- **`net`**（`crates/net/src/url_loader.rs`）：scheme 有 `http`、`https`（reqwest）、`file`、`data`、`blob`。redirect 在 `load` 的迴圈裡跟（最多 20 次），CORS（`cors.rs`，包括 preflight）也在這裡。回應整份在記憶體（`ResourceResponse::body: Vec<u8>`），沒有串流、cookie、HTTP cache。reqwest 開了 `gzip`、`brotli`、`deflate`，body 是解完的。`ResourceResponse` 現在有：最後的網址、狀態、狀態文字、`redirected`、tainting、MIME 類型、charset、header、body；沒有時間、遠端位址、HTTP 版本、每一跳。
- **時鐘**：browser 的 session 用一個 `Instant`（`AutomationSession::origin`）當 `Page.lifecycleEvent` 的 `timestamp` 的起點；renderer 的 `Clock`（`clock.rs`）從 renderer 啟動算毫秒。兩者現在沒有對齊。
- **生命週期**：browser 收到 renderer 的 `DidDispatchDOMContentLoaded`、`DidFinishLoad` 時送 main frame 的 `Page.lifecycleEvent`（`automation_session.rs` 的 `lifecycle`）；`Page.setLifecycleEventsEnabled` 由 browser 回答。`Page.navigate` 在 `WebContents::navigate` 之後立刻回答。machopodriver 的導覽用回答的 `loaderId` 和 `Kun.waitForLoad` 等。
- **paint timing**：kun 沒有（沒有 `PerformancePaintTiming`、`first-paint`）。

## Chromium 的對照

2026-10-23 對照 Chromium 的 `main`（路徑相對於 `chromium/src`）和 devtools-frontend 的 `main`。

1. **誰送事件**：
   - main frame 的導覽：`requestWillBeSent` 由 browser 的 `content/browser/devtools/protocol/network_handler.cc` 的 `NetworkHandler::NavigationRequestWillBeSent` 送（`devtools_instrumentation::OnNavigationRequestWillBeSent` 呼叫），類型 `Document`，`requestId` 和 `loaderId` 都是 `devtools_navigation_token`；`responseReceived` 也是 browser（`NetworkHandler::ResponseReceived`，經過 `OnNavigationResponseReceived`）；導覽失敗的 `loadingFailed` 是 browser（`NetworkHandler::LoadingComplete`，經過 `OnNavigationRequestFailed`）。
   - 文件的 `dataReceived`、`loadingFinished` 由 renderer 的 `third_party/blink/renderer/core/inspector/inspector_network_agent.cc`（`DidReceiveData`、`DidFinishLoading`，`DocumentLoader` 的 probe）送，id 用 `IdentifiersFactory::RequestId` 對到同一個 `LoaderId`。renderer 的 `DidReceiveResourceResponse` 遇到導覽就直接回去：文件的 `responseReceived` 只有 browser 送。commit 之後 body 出錯時 renderer 送 `loadingFailed`。
   - 子資源：五種事件都是 `InspectorNetworkAgent`（`WillSendRequestInternal`、`DidReceiveResourceResponse`、`DidReceiveData`、`DidFinishLoading`、`DidFailLoading`）。
   - browser 另外送的：keepalive 的 fetch、prefetch、worker 的 main script、CORS preflight 等。
2. **`ExtraInfo`**：network service 的 `DevToolsObserver` → `content/browser/devtools/network_service_devtools_observer.cc` → `NetworkHandler::OnRequestWillBeSentExtraInfo`、`OnResponseReceivedExtraInfo`。沒有它們時送 `hasExtraInfo: false`（`responseReceived`）、`redirectHasExtraInfo: false`（`requestWillBeSent`），DevTools（`front_end/core/sdk/NetworkManager.ts` 的 `ExtraInfoBuilder.addHasExtraInfo(false)`）就不等。
3. **redirect**：同一個 `requestId` 的另一個 `requestWillBeSent`，帶 `redirectResponse`。renderer 的 `WillSendRequestInternal` 收到 redirect 的回應（307、308 時保留 POST 的 body）；導覽的由 browser 從 `commit_params.redirect_params` 組。
4. **`getResponseBody`**：renderer 的 `InspectorNetworkAgent::getResponseBody` 從 `NetworkResourcesData` 拿，沒有時從快取的 `Resource`（`InspectorPageAgent::CachedResourceContent`）或 blob 拿；錯誤是「No resource with given identifier found」和「Request content was evicted from inspector cache」。browser 的 `NetworkHandler::GetResponseBody` 只回答它自己記的（prefetch 等），其他的交給 renderer，所以文件的內容也是 renderer 給。上限：`kDefaultTotalBufferSize` 200,000,000、`kDefaultResourceBufferSize` 20,000,000（Android 是 10 MB、5 MB）；`Network.enable` 的 `maxTotalBufferSize`、`maxResourceBufferSize`、`maxPostDataSize`（預設 0）。只在打開時記（`Enable()` 才加進 instrumenting agents），`disable()` 清掉；main frame commit 時除了新的文件都清掉。
5. **發起者**：`InspectorNetworkAgent::BuildInitiatorObject`，依序：有 referrer 的 import 的 module → `script`（網址、行、欄）；CSS（包括 UA 的）要的 → `parser`（stylesheet 的網址）；`CaptureSourceLocation(...)->BuildInspectorObject(max_async_depth)` 有 stack → `script` 帶 `stack`；有 parser 在跑的文件 → `parser`（文件的網址和 parser 的位置）；其他 → `other`。stack 在發請求的當下由 `SourceLocation::CaptureStackTraceInternal` → `V8Inspector::captureStackTrace` 抓（`platform/bindings/source_location.cc`、`bindings/core/v8/capture_source_location.cc`）。`preflight` 由 browser 送。
6. **`ResourceTiming`**（devtools-protocol 的 `pdl/domains/Network.pdl`）：必要的欄位有 `requestTime`、`proxyStart/End`、`dnsStart/End`、`connectStart/End`、`sslStart/End`、`workerStart`、`workerReady`、`workerFetchStart`、`workerRespondWithSettled`、`sendStart/End`、`pushStart/End`、`receiveHeadersStart/End`。`requestTime` 是單調時鐘的秒，其他是相對它的毫秒，不知道的 -1（`resource_load_timing.cc` 的 `CalculateMillisecondDelta`）。
7. **`Log`**（`inspector_log_agent.cc`）：來源有 xml、javascript、network、storage、rendering、security、other、deprecation、worker、violation、intervention、recommendation（`console.*` 走 `Runtime.consoleAPICalled`，不在這裡）；等級 verbose、info、warning、error。`Log.enable` 時先補一則「N log entries are not shown.」（有丟掉的時候），再補送保留的（`inspector/console_message_storage.cc` 的 `kMaxConsoleMessageCount` = 1000）。404 那一行由 `core/frame/frame_console.cc` 的 `FrameConsole::ReportResourceResponseReceived` 送（狀態 400 以上，來源 network、等級 error）；載入失敗是 `FrameConsole::DidFailLoading`（「Failed to load resource: 」加上錯誤的說明，取消的不送）。每則帶 `networkRequestId`。
8. **生命週期**：renderer 的 `InspectorPageAgent::LifecycleEvent`，`setLifecycleEventsEnabled` 打開時才送。
   - `init`：`DocumentLoader::StartLoading`、`DidOpenDocument`；
   - `DOMContentLoaded`、`load`；
   - `firstPaint`、`firstContentfulPaint`、`firstImagePaint`（`paint_timing.cc`）；
   - `firstMeaningfulPaintCandidate`、`firstMeaningfulPaint`（`first_meaningful_paint_detector.cc`）；
   - `networkAlmostIdle`、`networkIdle`（`LocalFrame::NetworkBecameAlmostIdle`、`NetworkBecameIdle`，各一次）；
   - `InteractiveTime`（`interactive_detector.cc`，5 秒的窗）。
   - `commit` 執行時不送，只在 `setLifecycleEventsEnabled(true)` 晚打開時補送。
   - `loader/idleness_detector.cc`：解析完之後才算，看 `Fetcher()->ActiveRequestCount()`；不超過 2 個是 almost idle、0 個是 idle，各要安靜 `kNetworkQuietWindow`（500 ms，設定 `NetworkQuietTimeout` 可以改），task 執行的時間加進窗裡；時間記的是安靜開始的那一刻。
   - 每個 local frame 都送（iframe 也是）；巢狀 iframe 的 `DOMContentLoaded` 是父先子後、`load` 是子先父後。
9. **`Page.navigate`**：browser 的 `content/browser/devtools/protocol/page_handler.cc` 的 `PageHandler::Navigate`，回答存在導覽的 token 上，在 `NavigationReset`（`NavigationRequest::ReadyToCommitNavigation` 交出導覽時，或導覽被重設、失敗時）回答：ready-to-commit，不是 commit。回答 `frameId`、`loaderId`（同一份文件裡的導覽沒有）、`errorText`（`net::ErrorToString`，例如 `net::ERR_ABORTED`）、`isDownload`。
10. **`Network` 的命令在哪裡回答**：
    - browser 做事之後也轉給 renderer：`enable`、`disable`、`setCacheDisabled`、`setExtraHTTPHeaders`、`emulateNetworkConditions`、`setBypassServiceWorker`、`configureDurableMessages`、`getResponseBody`（通常）。
    - 只有 browser：cookie 的命令、`clearBrowserCache`、`clearBrowserCookies`、`canEmulateNetworkConditions`、`emulateNetworkConditionsByRule`、`loadNetworkResource`、`getSecurityIsolationStatus`、`enableReportingApi`。
    - 只有 renderer：`setAttachDebugStack`、`setBlockedURLs`、`replayXHR`、`overrideNetworkState`、`getCertificate`、`getRequestPostData`、`streamResourceContent`。

## DevTools 前端連上時送的命令

devtools-frontend 的 `NetworkManager.ts`（2026-10-23）：
- 建立時：`Network.enable`（`maxPostDataSize: 65536`、`maxTotalBufferSize: 250*1024*1024`、`reportDirectSocketTraffic: true`）、`Network.setAttachDebugStack({enabled: true})`（`enable` 還沒處理時 renderer 回答「Domain must be enabled」）；設定打開時才送的：`setCacheDisabled`、`setBypassServiceWorker`、`configureDurableMessages`。
- `MultitargetNetworkManager.modelAdded`：每次都送 `Network.setBlockedURLs({urlPatterns: []})`、`Network.emulateNetworkConditionsByRule({offline, emulateOfflineServiceWorker, matchedNetworkConditions: []})`（讀回答的 `ruleIds`）、`Network.overrideNetworkState(...)`；有設定時才送 `setExtraHTTPHeaders`、`setUserAgentOverride`，攔截時才送 `Fetch.enable`。

這些都要回答成功，DevTools 的 Network 面板才不出錯（「已決定的事項」第 7 條）。

## 誰送事件的選項

| | N1：照 Chromium | N2：全部在 browser | N3：全部在 renderer |
| --- | --- | --- | --- |
| 導覽的 `requestWillBeSent` | browser，導覽開始時 | browser | renderer，commit 時補送（時間不對，失敗的導覽要另外通知 renderer） |
| 子資源的 `frameId`、類型 | renderer 知道 | IO thread 不知道 frame：`FetchResource` 要多帶 frame 的 id | renderer 知道 |
| script 發的請求的 stack | renderer 當下抓 | 要 renderer 抓好放進 `FetchResource`（每個請求都付，或 browser 告訴 renderer 現在要抓） | renderer 當下抓 |
| parser 的位置、`parser` 的發起者 | renderer 知道 | 同上 | renderer 知道 |
| 回應的內容 | renderer 留 | browser 的 IO thread 留（拆成 process 之後在 browser process） | renderer 留 |
| 和 session 的接法 | browser 的事件在 UI thread 的 session；renderer 的事件照 0075 的 raw 轉送 | IO thread 要把事件交給 UI thread 的 session | 照 0075 的 raw 轉送 |
| 拆成 process 之後 | 不用改（Chromium 就是這樣） | 被攻破的 renderer 送的 frame id 要驗證 | 不用改 |

N1 比 N2 多的是「兩個地方送」：browser 和 renderer 的 `requestId`、`loaderId` 和時鐘要一致（第 1、4 節）。N2 看起來集中，但 renderer 才知道的東西（frame、發起者、stack、parser 的位置）都要經過 `FetchResource` 帶過去，等於把 agent 的一半留在 renderer。

## 抓 stack 的選項

- **I1：`v8::StackTrace::CurrentStackTrace`**：rusty_v8 已經綁了（`StackTrace`、`StackFrame` 的網址、行、欄、函式名、script id），`bindings` 組成 `Runtime.StackTrace` 的 JSON。沒有 async 的部分：V8 的 inspector 自己記 async 的 stack（`Debugger.setAsyncCallStackDepth` 打開時），只有經過它才拿得到。現代的網頁 `fetch()` 多半在 `async` 函式的 `await` 之後，同步的 stack 只到那個函式，看不出是誰呼叫的。
- **I2：`V8Inspector::captureStackTrace` 加 `V8StackTrace::buildInspectorObject`**：fork 現在綁了 `createStackTrace`（從 `v8::StackTrace` 建）和 `exceptionThrown` 收它，沒有 `captureStackTrace`。`buildInspectorObject` 回答 `protocol::Runtime::API::StackTrace`，它只能序列化成 CBOR（`AppendSerialized`），要在 `binding.cc` 用 V8 裡的 `v8_crdtp::json::ConvertCBORToJSON` 轉成 JSON 交給 Rust；或者交出 CBOR、在 Rust 這一側轉。改 C++ 要重編 V8、發新的 release（`docs/README.md` 的「其他會牽動文件的變更」）。結果和 Chrome 一模一樣，DevTools 的 Initiator 顯示 async 的呼叫鏈。
- **I3：不抓**：script 發的請求都是 `other`，Initiator 只看得到 parser 發的。

第 2 段開始時先做 I2 的原型：確認 `binding.cc` 能 include `crdtp/json.h` 並連結，序列化的結果和 DevTools 收到的相同。
