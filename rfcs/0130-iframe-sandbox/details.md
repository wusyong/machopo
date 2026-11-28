# RFC 0130 的細節：iframe 的 `sandbox`

## 盤點

2026-11-28 的 `main`（`0ffe2f67`）：

- **沒有 `sandbox`**：`sandbox` 屬性不解析，`HTMLIFrameElement` 沒有 `sandbox` 這個 IDL 屬性（`docs/limitations.md` 的「iframe」）。sandbox 的 iframe 照它的網址的 origin commit、照樣跑 script。
- **Browser 的 frame 樹**（`browser` 的 `frame_tree.rs`）記著每個 frame 的 origin、能不能開本機檔案、`srcdoc`、名字；子 frame 由 renderer 的 `DidCreateChildFrame` 建、`DidChangeSrcDoc` 記 `srcdoc`。commit 的 origin 由 Browser 照網址決定（錯誤頁 opaque、`about:blank` 照發起者、`about:srcdoc` 照父 frame）。
- **opaque 的 context**（RFC 0128）：initiator 是 `null` 的請求清掉 site for cookies、frame 的網站，不帶 cookie。
- **導覽的權限**：`dom_api` 的 `history::may_navigate`（Blink 的 `LocalFrame::CanNavigate` 的一部分）現在只管 `javascript:` 網址要同源。
- **machopo 有的、會被旗標管到的功能**：script（parser 的、插入的、事件的 handler、`javascript:` 網址）、`<noscript>` 的解析（`html` 的 `scripting_enabled`）、autofocus、子 frame 導覽別的 frame 和最上層（`location`、連結的 `target`）、連結開新分頁（`target=_blank`、找不到名字的）、`document.cookie`、`localStorage`、`sessionStorage`。
- **machopo 沒有的**：表單送出、`alert`、`confirm`、`prompt`、`print`、下載、pointer lock、螢幕方向、presentation、storage access、`document.domain`、`window.open`、plugin。

## Chromium

2026-11-28，`chromium.googlesource.com` 的 `main`：

- `services/network/public/mojom/web_sandbox_flags.mojom`：旗標（`kNavigation`、`kPlugins`、`kOrigin`、`kForms`、`kScripts`、`kTopNavigation`、`kPopups`、`kAutomaticFeatures`、`kPointerLock`、`kDocumentDomain`、`kOrientationLock`、`kPropagatesToAuxiliaryBrowsingContexts`、`kModals`、`kPresentationController`、`kTopNavigationByUserActivation`、`kDownloads`、`kStorageAccessByUserActivation`、`kTopNavigationToCustomProtocols`、`kAllowSameSiteNoneCookies`），組合時用 AND（只能更嚴）。
- `services/network/public/cpp/web_sandbox_flags.cc` 的 `ParseWebSandboxPolicy`：照 HTML 的空白切開，從全部的旗標開始，每個認得的 token 拿掉它的旗標（`allow-scripts` → `kAutomaticFeatures | kScripts`，`allow-popups` → `kPopups | kTopNavigationToCustomProtocols`，`allow-top-navigation` → `kTopNavigation | kTopNavigationToCustomProtocols`，`allow-popups-to-escape-sandbox` → `kPropagatesToAuxiliaryBrowsingContexts`……），不認得的記下來，錯誤訊息是「'x' is an invalid sandbox flag.」或「'x', 'y' are invalid sandbox flags.」（去掉重複、保留順序）。
- `third_party/blink/renderer/core/html/html_iframe_element.cc`：`sandbox` 屬性改了時解析，有錯誤時寫 console（`kOther`、`kError`，「Error while parsing the 'sandbox' attribute: …」），`SetSandboxFlags` 交給 frame owner（之後經過 `DidChangeFramePolicy` 到 Browser）。
- `content/browser/renderer_host/frame_tree_node.cc` 的 `SetPendingFramePolicy`：子 frame 的 pending 旗標一定 OR 上父 frame 現在的旗標（`active_sandbox_flags`），下一次導覽用。
- `content/browser/renderer_host/navigation_request.cc`：commit 時的 origin（`GetOriginForURLLoaderFactoryBeforeResponse`）照旗標，有 `kOrigin` 就是 opaque 的（帶 precursor）；最後的旗標是 frame 的、回應的 CSP 的、和繼承的（本地的 scheme）合起來（`policy_container_builder_->FinalPolicies()`）；`kDownloads` 也在這裡擋。
- `content/browser/renderer_host/sandbox_flags.md`：旗標定義在 frame（`<iframe sandbox>`）和文件（CSP 的 `sandbox`）上；從 frame 繼承到它的文件、從文件到它的子 frame 和它開的視窗。

### 擋的地方（2026-11-28 讀的）

- **renderer（Blink）**：script 是 `LocalDOMWindow::CanExecuteScripts`（`kScripts`）；導覽是 `LocalFrame::CanNavigate`：有 `kNavigation` 時只能導覽自己和自己底下的 frame，或最上層；最上層要沒有 `kTopNavigation`，或只有 `kTopNavigationByUserActivation` 而且有使用者的啟動（沒有時 `DidBlockNavigation` 告訴 Browser、寫 console）；導覽彈出視窗要是它的 opener 而且旗標會帶過去；每一種擋下的都寫 console（「The frame attempting navigation is sandboxed, and is therefore disallowed from navigating its ancestors.」等）。開新視窗是 `blink::CreateNewWindow`（`kPopups`）。
- **Browser**：`RenderFrameHostImpl::CreateNewWindow`：沒有 `allow-popups` 的 frame 要開新視窗是壞訊息（`RFH_CREATE_NEW_WINDOW_FROM_SANDBOXED_FRAME`，註解：「This should have already been checked by the renderer process (see blink::CreateNewWindow()), and this browser-side check defends against compromised renderers」）。`DidChangeFramePolicy`：只收對直接的子 frame 改的（`FindAndVerifyChild`，不是的是壞訊息 `RFH_SANDBOX_FLAGS`）。導覽最上層的 sandbox 的旗標 Browser 不再檢查（只有 fenced frame 的 `ValidateUnfencedTopNavigation`）。`navigation_request.cc` 另外記「跨源的子 frame 沒有使用者的啟動不能導覽最上層」（crbug.com/1251790），`sandbox` 沒有拿掉 `allow-top-navigation` 時例外：這是 framebusting 的介入，不是 sandbox。

## 沒讀到的

- `about:blank`、`about:srcdoc`、`javascript:` 的文件繼承旗標的細節（`PolicyContainerHost` 的繼承），和 `<noscript>` 的解析在哪裡看旗標：實作時讀。

## WPT

wpt.fyi 上 Chrome 的結果（2026-11-28 下載的 `report.json`）：`html/browsers/sandboxing/` 和 `the-iframe-element/` 檔名有 `sandbox` 的，92 個檔案，130 個 subtest 裡 Chrome 過 124 個。檔名有 navigation 的 19 個、top-navigation 15 個、popup 11 個、window-open 3 個、download 6 個、modal 4 個、script 11 個。
