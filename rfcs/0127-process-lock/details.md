# RFC 0127 的細節：process lock 和 Browser 的檢查

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-28 讀的（`main`，`ec34252e`）。行號是當時的。

### 訊息怎麼進來

- renderer 對 Browser 有兩條路：`RendererHostMsg`（`protocol` 的 `renderer.rs`）和給 Viz 的 frame sink。frame sink 的訊息由 `msg.check()` 檢查，綁在 renderer 自己的 `frame_sink_id` 上（RFC 0123）。
- `RendererHostMsg` 先到 IO thread 的 `IoThreadHost::on_message`（`render_process_host.rs`）。請求、blob URL、cookie、storage 的修改和字型由它自己處理，其他的轉給 UI thread 的 `WebContents::on_renderer_message`。嵌入的人丟掉不是分頁現在的 renderer 送的訊息（`is_current_renderer`）。
- 壞訊息：IO thread 把 `Err(reason)` 轉成 `ContentsEvent::BadMessage`，`received_bad_message` 結束 renderer（`RenderProcessHost::kill`：child process 直接結束，thread 的送 `Shutdown`），分頁顯示 sad tab。`BadMessageReason`（`bad_message.rs`）沒有一種是關於網站或鎖的。

### `SiteInfo` 用在哪裡

- `RenderProcessHost::site_info`：`launch` 時是 `None`。`WebContents::commit` 送出 main frame 的 commit 之前，先問 `site_info::swaps` 要不要換，再 `set_site_info`（只在網址有網站時）。之後才 `register_loader`，所以 main frame 的 loader 註冊時鎖已經定了。
- 只有換不換用到它。子 frame 的 commit 不碰它。IO thread 看不到它：`IoThreadHost` 只有 `loaders` 和 `file_access`。
- `set_site_info` 直接覆寫，不看原本的值。
- `InProcess`（`--single-process`）和 `Detached` 用同一個 `RenderProcessHost`，一樣有 `site_info`。

### 交出的權限

| 權限 | 誰算的 | 怎麼用 |
| --- | --- | --- |
| loader（`LoaderParams`：origin 和 `TrustedParams`） | Browser 在 commit 時從 frame 樹算（`frame_tree.loader_params`），worker 用建它的文件的 | 請求要經過 renderer 有的 loader，發起者要是 `None`、`"null"` 或 loader 的 origin，`TrustedParams` 由 Browser 換上（`check_request`）。cookie 要經過 loader，網址要是 loader 的 origin（`check_cookie_access`）。loader 在 renderer 結束之前都不拿掉 |
| storage area | Browser 從 frame 樹取 `StorageKey`（`frame_tree.storage_key`） | `OpenStorageArea` 帶 frame，renderer 可以指它分頁裡的任何 frame。不在樹上的 frame 回答空的。之後的 `ChangeStorage` 要用這個 renderer 開過的 area |
| commit | Browser 送的那一筆 | `DidCommitNavigation` 的網址要和送的相同，origin 不由 renderer 報（`FrameTree::did_commit`） |

### renderer 說了算的地方

- **`RegisterBlobUrl`**：只帶網址、MIME type 和內容（worker 的帶 worker 的編號）。`BlobUrlRegistry::register`（`net` 的 `blob.rs`）直接 `insert`，不看網址裡的 origin，也不看原本那一筆是誰的。子資源只拿得到自己登記、沒撤銷的。導覽（`WebContents` 的 `load_blob_url(&request, None)`）拿得到任何一筆，文件的 origin 是網址裡的那一個（`CommittedDocument::origin`）。`page_may_navigate_to` 允許頁面把 main frame 導覽到 `blob:`。
- **請求的發起者**：renderer 對 `no-cors` 的子資源不帶發起者（`net` 的 `Cors::new`：沒有發起者就不走 CORS）。`fetch()` 和 module script 帶。`check_request` 接受 `None`，也接受 loader 的 origin 不是 opaque 時的 `"null"`（跨源 redirect 之後的 `Origin: null`）。不帶的請求沒有 CORS，也沒有 `Origin` header，`credentials` 照 renderer 給的。Browser 沒有送 `Referer`，也沒有任何 `Sec-Fetch-*`。
- **`ChangeStorage` 的 `url`**：照轉給同一個 area 的每個 renderer 的 `StorageAreaChanged`，成為 `storage` 事件的 `url`（`dom_storage.rs` 的 `change`）。
- **`BeginNavigation` 的發起者**：是一個 frame token，Browser 從 frame 樹取它的 origin，renderer 可以指它分頁裡的任何 frame。main frame 的 `about:blank` 繼承這個 origin（`about_document`）。
- **`OpenUrl`**：新分頁的 `navigate` 不帶發起者（`shell` 的 `browser_view.rs`），算成 Browser 自己的導覽。`net` 的 `same_site_initiator` 對沒有發起者的導覽給 strict，所以 `target=_blank` 開到別的網站的連結會帶 `SameSite=Strict` 的 cookie。

### 0124 的規則的特例

| 網址 | `SiteInfo::of_url` | 文件的 origin |
| --- | --- | --- |
| `about:blank` | 沒有：不換，鎖不變 | 發起者的，或 opaque |
| 其他 `about:` | `BuiltIn` | opaque，只有使用者能導覽過去 |
| `about:srcdoc` | 只在子 frame | 父 frame 的 |
| `data:` | 新的 opaque 網站，除了重新整理每次都換 | opaque，頁面不能把 main frame 導覽過去 |
| 錯誤頁 | 失敗的網址的網站 | opaque |
| `file:` | `File` | `file:`，commit 時給讀檔的權限 |
| `blob:` | 網址裡的 origin 的網站 | 網址裡的 origin |
| 重新整理 | 不換，不看網址 | 照網址 |

### 沒有的

- IndexedDB、Cache API、`BroadcastChannel`、`SharedWorker`、service worker 都沒有。`postMessage` 和 `MessageChannel` 都在 renderer 裡，不經過 Browser。
- 既有的被攻破的 renderer 的測試：
  - `crates/browser/tests/it/renderer_process.rs` 的 `rogue()`：一個照角色送偽造訊息的 child process，角色有「垃圾」「別人的 cookie」和 frame sink 的那幾種。`bad_message_of()` 等 `BadMessage`。
  - `renderer_message_fuzz.rs` 用 `Detached` 和 `deliver_from_renderer` 餵訊息。
  - `render_process_host.rs` 的單元測試涵蓋 loader、worker、cookie、blob URL。

## Chromium

2026-11-28 讀的，Chromium 的 `main`（GitHub 的鏡像）。

### 鎖

- `content/browser/process_lock.h`：invalid 的鎖表示還沒有 `SiteInstance`，不給任何東西。allow-any-site 的鎖在沒有用過之前可以收成鎖給一個網站的鎖。
- `content/browser/security/cpsp/child_process_security_policy_impl.cc` 的 `ProcessState::SetProcessLock`：`CHECK(lock_to_set.is_unused()) << "Cannot lock a process to an already used lock: "`；原本是 allow-any-site 時，`CHECK(process_lock_.is_unused()) << "Cannot lock an already used process to "`；鎖給一個網站之後，「A lock that is locked to a site is never unused」。
- Chromium 的文件「Process Model and Site Isolation」：「ProcessLocks are always assigned before any content is loaded in a renderer process, either at the start of a navigation or at OnResponseStarted time, just before a navigation commits」，之後「remains constant for the lifetime of the RenderProcessHost」。
- `site_info.cc` 的 `SiteInfo::ShouldLockProcessToSite`：「Don't lock to origin in --single-process mode, since this mode puts cross-site pages into the same process.」machopo 的 `--single-process` 每個網站一組新的 thread（RFC 0124 的已決定的事項第 11 條），所以一樣鎖。
- 錯誤頁的鎖是 `kUnreachableWebDataURL`，所有的錯誤頁一個（只對 main frame）。

### 檢查

- `CanAccessDataForOrigin` 失敗時的 crash key 有 `can_access_data_failure_reason`、`expected_process_lock`、`requested_origin`。沒有讀全的部分（模型的摘要，沒有逐字確認）：
  - 拒絕的理由有 `process_lock_is_invalid`、`no_matching_committed_origin`；
  - jail 比的是「實際的鎖等於這個 origin 該有的鎖」，citadel 對沒有用過的、或這個網站不需要專用 process 的放行；
  - 函式本身只回 `false`，結束 renderer 的是呼叫的人。
- `ProcessState::MatchesCommittedOrigin`：非 opaque 的照 `IsSameOriginWith`，opaque 的只在網址是它的 precursor 時對得上；`file:`「Temporarily ignore hosts when comparing file origins」。
- Chromium 的文件「Compromised renderers」：`RenderFrameHostImpl::CanCommitOriginAndUrl`「verifies that the renderer process is able to commit what it claims, and kills the process otherwise」。storage 用 `CanAccessDataForOrigin`（`StoragePartitionImpl::OpenLocalStorage`），或把介面綁在 Browser 的 `GetLastCommittedOrigin()` 上。cookie 用 `RestrictedCookieManager`，只給「origins within their site」。

### 網路

- `services/network/public/mojom/network_context.mojom` 的 `URLLoaderFactoryParams`：「|request_initiator_origin_lock| locks |ResourceRequest::request_initiator| to the specified origin」，「should be present in all factories that may be vended to a Renderer process」。
- `services/network/cors/cors_url_loader_factory.cc` 的 `IsValidRequest`：
  - 發起者和鎖不合時 `mojo::ReportBadMessage("CorsURLLoaderFactory: lock VS initiator mismatch")`；
  - 沒有鎖、沒有發起者是 `NOTREACHED()`，所以給 renderer 的 factory 的請求一定有發起者；
  - renderer 帶 `Sec-` header 時 `"CorsURLLoaderFactory: Forbidden Sec- header from renderer"`。

### blob URL

`storage/browser/blob/blob_url_store_impl.cc`：`Register` 不收網址，由 Browser 用 `blink::CreateBlobUrl(renderer_origin_, …)` 產生。`renderer_origin_` 在建 store 時由 Browser 給。`BlobUrlIsValid` 檢查網址的 origin 等於 store 的 storage key 的 origin，不合時 `mojo::ReportBadMessage("URL with invalid origin passed to BlobURLStore::" …)`。

### 被攻破的 renderer 的測試

`content/browser/security_exploit_browsertest.cc`：

- 直接對 frame 呼叫偽造的方法，或綁一個 Mojo 的 remote 送偽造的參數。
- `RenderProcessHostBadIpcMessageWaiter` 等 renderer 被結束，比對 `bad_message` 的理由，例如 `RVH_WEB_UI_BINDINGS_MISMATCH`、`RFH_DOWNLOAD_URL_BLOB_WITHOUT_TOKEN`。
- `mojo::test::BadMessageObserver` 等不結束 renderer 的壞訊息字串。

machopo 的 `rogue()` 是同一種東西：一個真的 child process，送 renderer 不會送的訊息。

## L1 留下的（OOPIF 之前擋不住的）

鎖是最上層的網站，嵌在裡面的跨網站 iframe 是合法的住戶。所以被攻破的 renderer 可以：

- 拿 iframe 的 loader 發請求、讀 iframe 的分區的 storage（第三方、沒有 cookie，RFC 0122 的已決定的事項第 4 條）；
- 把 iframe 指成 `BeginNavigation` 的發起者，最上層的導覽就算成 iframe 的網站發起的，`SameSite` 的判斷跟著變。

Chromium 的 site-per-process 在這裡會擋，因為 iframe 的網站不在這個 process。

## 參考資料

- [Process Model and Site Isolation](https://chromium.googlesource.com/chromium/src/+/main/docs/process_model_and_site_isolation.md)
- [Compromised renderers](https://chromium.googlesource.com/chromium/src/+/main/docs/security/compromised-renderers.md)
- Chromium 的原始碼：`content/browser/process_lock.h`、`content/browser/security/cpsp/child_process_security_policy_impl.cc`、`services/network/public/mojom/network_context.mojom`、`services/network/cors/cors_url_loader_factory.cc`、`storage/browser/blob/blob_url_store_impl.cc`、`content/browser/security_exploit_browsertest.cc`
