# RFC 0104 的細節：frame 樹和每個 frame 的導覽由 Browser 擁有

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 現況

2026-11-18 讀程式得到的（`main` 的 `5bd5273`）。函式和型別的名字是當時的。

### renderer 這一側

- **frame 由四個部分組成**，沒有單一的「browsing context」物件：
  - DOM 的連結（`crates/dom/src/frame.rs`）：iframe 的 `content_document`、文件的 `frame_owner`、`discarded`。
  - engine 的 frame 清單（`crates/dom_js/src/frames.rs` 的 `Frames`、`Frame`）：每個 frame 的 realm、目前的文件、父 frame、iframe 元素。整個分頁的 frame 共用一個 isolate 和一個 DOM 的 heap。
  - id（`crates/dom_api/src/frame.rs` 的 `FrameId`）：engine 發的號碼，跨過那個 frame 自己的導覽；主 frame 是 0。
  - renderer 的紀錄（`crates/renderer/src/render_thread/documents.rs` 的 `ChildFrames`、`FrameDocument`）和 layout 的樹（`crates/renderer/src/frame_view.rs` 的 `FrameView` 的 `children`）。
- **建立**：iframe 插入時同步建（`dom_api` 的 `create_child_frame` → `dom_js` 的 `frames::create_child`）：新的 realm、初始的 `about:blank`、下一個 `FrameId`。接著照屬性導覽（`process_iframe_attributes`）；沒有 `src` 也沒有 `srcdoc` 時，iframe 的 `load` 當場分派。
- **移除**：`discard_child` 丟掉那個 frame 和它的子孫、結束它們的 worker。
- **子 frame 換文件，每一條路都只在 renderer 裡**（`documents.rs` 的 `frame_navigated`、`navigate_child_as`）：
  - `src` 改了、子 frame 裡的連結、`location.href`、`assign`、`replace`（從父 frame 設的也一樣）、`location.reload()`。
  - 只有 fragment 不同：在 renderer 裡捲動。
  - `about:` 的網址和 `srcdoc`：排一個 task 之後 commit（`queue_child_commit`、`run_child_commits`），不經過網路。
  - 其他：`ResourceRequest::new(url, RequestDestination::Iframe)`，用 `RendererHostMsg::FetchResource` 送出，和子資源同一條路。沒有 `initiator`，mode 是 `NoCors`。
  - redirect 由 renderer 回 `FollowRedirect`；回應到了由 `commit_child` commit：先 `pagehide`、`unload`，再建新的 window（從初始的 `about:blank` 換到繼承 origin 的文件時沿用 window），丟掉舊文件的子 frame。失敗時 commit 一份空的 `text/html`。
  - `javascript:` 的網址在那個 frame 裡跑（`run_javascript_url`），不經過 Browser。
  - `history.back()`：送 `GoToEntryAtOffset`，走的是主 frame 的歷史。
- **同一份文件內的導覽**（fragment、`pushState`、`replaceState`）：renderer 自己做；只有主 frame 的會送 `DidCommitSameDocumentNavigation`。
- **歷史**：`dom` 的 `SessionHistory`（index、length）整個 DOM 一份，由主 frame 的 commit 設定；每個 window 的 `History` 拿著自己的 state。
- **origin**：`about:` 的子文件拿 iframe 所在文件的 origin；V8 的 security token 照 origin 設（`frames::set_security_token`）。沒有 sandbox 的旗標。
- **父文件的 `load` 等子 frame**：`finish_load` 在 `ChildFrames::loading_in` 是 true 時不往下做（有子 frame 的請求、排著的 commit、或還沒 `Loaded` 的子文件）。
- **對外的 frame id**：`{renderer 的前綴:016x}.f{號碼}`（`render_thread/webdriver_commands.rs` 的 `child_frame_id`），DevTools 和 WebDriver 共用；主 frame 的是 Browser 給的 token（`RendererMsg::DevToolsMainFrameToken`）。

### Browser 這一側

- **`WebContents`**（`crates/browser/src/web_contents.rs`）：一個 `url`、`title`、`devtools_frame_token`、`navigation: Option<PendingNavigation>`、`navigation_ids`、`document_navigation_id`。
- **主 frame 的導覽**：`begin_navigation` 建 `ResourceRequest`（`RequestDestination::Document`）、`network.fetch_with_redirects`；redirect 回到 UI thread（`on_navigation_redirected`）；完成時 `on_navigation_completed` 交給 `Committer::commit`，失敗時 commit `loader::error_page`。`Committer::commit` 順便設定 `file_access`。
- **Browser 對 renderer 要求的導覽做的檢查只有 `open_url`**：scheme 是 `http`、`https`、`file`、`about`、`blob` 之一；`data:` 不行；`file:` 要目前的文件也是 `file:`。
- **Browser 沒檢查的**：`DidCommitNavigation` 的網址和 id（不認得的 id 只記警告、當成新的 entry）、`DidCommitSameDocumentNavigation` 的網址（沒有同源的檢查）。
- **`NavigationController`**（`crates/browser/src/navigation_controller.rs`）：`NavigationEntry { url, title, document, state, manual_scroll_restoration, initial }` 的清單；`document` 是 commit 那份文件的導覽 id（當成 document 的序號），沒有 item 的序號。
- **子資源的請求**在 IO thread 上處理（`crates/browser/src/render_process_host.rs`），UI thread 看不到；`RequestDestination::Iframe` 只影響 `Accept` 和 DevTools 的資源種類。`net` 不知道請求是哪個分頁、哪個 frame 的（只有 blob 用的 `renderer_id`）。
- **DevTools 和自動化**（`crates/shell/src/automation_session.rs`）：target 的 id 就是主 frame 的 token；Browser 自己發主 frame 的 `Page.frameStartedNavigating`、`Page.navigatedWithinDocument`、`Network` 的文件請求；`Page.getFrameTree` 和 frame 的事件由 renderer 的 Page agent 回答；`Page.navigate` 沒有 `frameId`。WebDriver 的 `frame` 字串由 Browser 原樣轉給 renderer。
- **輸入**：Browser 只分到分頁，renderer 自己 hit test 進子 frame。這份不動它。

### 假設「一個分頁只有一個 frame」的地方

做第 1 階段時逐一處理：

- `browser`：`WebContents` 的 `url`、`title`、`devtools_frame_token`、`navigation`、`document_navigation_id`、`is_navigating`；`RenderProcessHost` 的 `file_access`；`NavigationController` 的平的清單；`ContentsEvent` 的 `NavigationRedirected`、`NavigationCompleted` 只帶導覽的 id。
- `protocol`：`CommitNavigation`、`CommitSameDocumentNavigation`、`DidCommitNavigation`、`DidCommitSameDocumentNavigation`、`SameDocumentNavigationFailed`、`OpenUrl`、`Reload`、`GoToEntryAtOffset`、`UpdateState` 都不帶 frame。
- `shell`：`AutomationSession` 的每個表都以分頁為鍵（`loaders`、`starting`、`committed`、`loaded`），事件的 `frameId` 一律填 target 的 id。
- 只有主 frame 才有、而且之後也不變的：`DidFinishLoad`、`UpdateTitle`、testharness 的結果、sad tab、網址列。

### 守著現在行為的測試

要改寫或補的，從這些開始：

- `crates/browser/tests/it/navigation.rs`：`a_child_frame_s_location`（守的是 Browser 聽不到子 frame 的導覽）、歷史的那一組（`push_state_entries_are_traversed_within_the_document` 等）、`failed_loads_commit_an_error_page`。
- `crates/shell/tests/it/`：`navigation_timing.rs` 的子 frame 的測試、`devtools_network.rs` 的 `an_iframes_document_is_its_own_request`、`devtools_lifecycle.rs` 的 `the_child_frames_are_told`、`network.rs` 的 `a_srcdoc_document_commits_after_the_parser_goes_on`、`fetch.rs` 的 `a_frame_navigated_away_cancels_its_documents_fetches`、`automation_pipe.rs` 和 `webdriver.rs` 的 frame 測試。
- `crates/dom_js/tests/it/frames.rs` 和 `links.rs`：renderer 這一側的行為，應該不變。
- 情境：`tests/scenarios/` 的 `iframes.txt`、`iframe-loads.txt`、`frames.txt`、`frame-input.txt`、`navigation.txt`。

## Chromium 的做法

標「查過」的是 2026-11-18 讀 Chromium `main` 的文件得到的（`navigation.md` 等四份在它的 `docs` 目錄）；標「照記憶」的沒有查證，用到它的那個階段開始前要查。

### 查過的

- **導覽的步驟**（`navigation.md`）：`beforeunload` → 網路請求 → redirect（每次一個 `DidRedirectNavigation`）→ 回應（狀態碼、MIME sniffing）→ Browser 選 renderer、送 commit → renderer 回覆，Browser 收到回覆的那一刻才算 commit → 載入。HTTP 的錯誤（4xx、5xx）照樣 commit 一份文件。
- **`DidFinishLoad` 是每個 frame 的，`DidStopLoading` 是整個 `WebContents` 的**，等最上層的文件、所有子 frame 和它們的子資源（`navigation.md`）。
- **每個 frame 各自有 `NavigationRequest`**（`navigation_concepts.md`）：等最後的回應的跨文件導覽每個 frame 最多一個，這個階段由 `FrameTreeNode` 擁有；ready-to-commit 到 commit 之間的由 `RenderFrameHost` 擁有。同一個 frame 裡新的導覽取消舊的；例外是沒有 user activation 的 renderer 導覽，碰到進行中的 Browser 導覽時被忽略（`Navigator::ShouldIgnoreIncomingRendererRequest`）。
- **renderer 發起的導覽比較不被信任**，不能到有特權的網址（`navigation_concepts.md`）。
- **同一份文件內的導覽**：fragment、`pushState`、`replaceState`、同一份文件內的 traversal。renderer 發起的那種，Browser 在同一個 task 裡建立又丟掉 `NavigationRequest`（`navigation_concepts.md`）。renderer 用 `DidCommitSameDocumentNavigation` 回報；Browser 要 renderer 做的用 `CommitSameDocumentNavigation`（`content/common/frame.mojom`）。
- **錯誤頁**：載入失敗（例如 DNS）和被擋下的導覽都 commit 一份錯誤頁（`navigation_concepts.md`）；網址是 `chrome-error://chromewebdata`，這個網址不存進 `NavigationEntry`（`special_case_urls.md`）。
- **client redirect**：文件 commit 之後由 meta 或 script 造成的導覽，先建的 entry 在目的地 commit 時被取代（`navigation_concepts.md`）。
- **session history**（`session_history.md`）：
  - `NavigationController` 拿著 `NavigationEntry` 的清單（平的，不是樹）；每個 entry 用一棵 `FrameNavigationEntry` 的樹記每個 frame 的狀態。
  - 新的子 frame 的第一次 commit 併進現有的 entry（auto subframe navigation）；使用者回不到那個 frame commit 之前。之後的（manual subframe navigation）才建新的 entry。
  - 取代目前的 item 而不是加一個的：`location.replace`、`replaceState`、client redirect、初始的空白文件之後第一個不是空白的網址。
  - 每個 `FrameNavigationEntry` 有 item sequence number 和 document sequence number：同一份文件內的導覽換 ISN、DSN 不變；跨文件的兩個都換。traversal 時 `FindFramesToNavigate` 遞迴走過 frame 樹，比這兩個數字決定哪些 frame 要動。
  - 回到前面、再 commit 新的導覽時，前進的 entry 被丟掉（除非是取代）。
- **特別的網址**（`special_case_urls.md`）：
  - 每個 frame 都從初始的空白文件（`about:blank`）開始；另一份文件建的 `about:blank` 的 iframe 繼承那份文件的 origin。初始的空白文件在第一個不是空白的網址 commit 時被取代。
  - `about:srcdoc` 繼承父文件的 origin。
  - `javascript:` 網址不是新的導覽，是在目標文件裡算一個 expression；只有同源的文件可以；永遠不進 session history。
  - 沒有 `allow-same-origin` 的 sandbox 的 iframe：內容照網址載入，但文件的 origin 是 opaque。
- **`content/common/frame.mojom` 的 `FrameHost`**：
  - `CreateChildFrame`：非同步；參數有子 frame 的 `LocalFrameToken`、名稱和 unique name、是不是 script 建的、frame policy、owner 元素的屬性。註解說每一則都有對應的 `FrameHost::Detach`，在 frame 從 DOM 拿掉時送。
  - `BeginNavigation`：renderer 要求導覽。
  - `DidCommitProvisionalLoad`：導覽在那個 frame commit 了。
  - `DidChangeName(name, unique_name)`：`window.name` 改了。
  - `UpdateState(PageState)`：那個 frame 有新的 session history 資料。
  - `DidStopLoading`。
- **子 frame 的 token 由 Browser 發**（`content/common/render_message_filter.mojom`）：`RenderMessageFilter` 是「到 browser process 的介面，在 IO thread 上處理」，放 renderer 一定要等的同步呼叫。`[Sync] GenerateSingleFrameRoutingInfo()` 回一個 `FrameRoutingInfo`（`routing_id`、`frame_token`、`devtools_frame_token`、`document_token`、`sandbox_origin_token`），註解說之後應該接著 `FrameHost::CreateChildFrame`；`[Sync] GenerateFrameRoutingInfos()` 一次回好幾個，留著之後用。

### 照記憶

- 導覽在 2017 年前後從 renderer 搬到 Browser（PlzNavigate）；之前 Browser 只追蹤 frame 樹。
- 子 frame 的 commit 的分類是 `NAVIGATION_TYPE_AUTO_SUBFRAME` 和 `NAVIGATION_TYPE_NEW_SUBFRAME`（`NavigationControllerImpl::ClassifyNavigation`）；新的子 frame entry 是複製目前的 entry 的樹、換掉那個 frame 的 `FrameNavigationEntry`。
- 回到某個 entry 或 reload 之後，新建的子 frame 照 entry 裡同一個 unique name 的 `FrameNavigationEntry` 載入（`NavigationControllerImpl::StartHistoryNavigationInNewSubframe`）；unique name 是 frame 的名稱，沒有時從它在樹裡的位置產生。
- `LocalFrameToken` 是 unguessable token，跨過那個 frame 的導覽在換 `RenderFrameHost` 時才換；DevTools 的 `frameId` 是另一個每個 `FrameTreeNode` 一個的 token（`FrameRoutingInfo` 的 `devtools_frame_token`）。
- `about:blank`（不是初始的那份）和 `about:srcdoc` 的導覽也經過 Browser，只是沒有網路請求；`srcdoc` 的內容在 renderer commit 時從屬性拿。
- Browser 丟掉 renderer 要求的導覽時會告訴 renderer，讓它不再當成載入中。
- Firefox 在 parent process 有 `CanonicalBrowsingContext` 的樹和 session history，導覽的網路部分也在 parent（`DocumentLoadListener`）；Servo 的 constellation 擁有 browsing context 的樹和 joint session history。

## 做法的比較

| | A：樹、導覽、歷史都搬 | B：只做樹 | C：都留在 renderer |
| --- | --- | --- | --- |
| Browser 知道哪些 frame 存在、誰嵌著誰 | 是 | 是 | 否 |
| Browser 知道每個 frame 的網址和 origin | 是，自己從 commit 的導覽算 | renderer 回報的，Browser 沒辦法核對 | 否 |
| 導覽前的檢查（scheme、`file:`） | Browser，一個地方 | renderer；Browser 只在子資源的路上擋 `file:` | 同左 |
| 要看回應的檢查（`X-Frame-Options`、`frame-ancestors`） | Browser，交給 renderer 之前 | renderer 收到之後；或另外在 IO thread 的路上加 | 同左 |
| cookie 算哪個網站、儲存的 key | 從樹算 | 從樹算，但網址是 renderer 回報的 | 沒有來源 |
| 子 frame 的歷史 | joint session history | 可以做（Chromium 搬導覽之前就是這樣），traversal 要 renderer 配合 | renderer 自己記，Browser 的 entry 帶一包資料 |
| 子 frame 的錯誤頁 | 和主 frame 同一份 | renderer 要有自己的一份 | 同左 |
| site isolation、跨語言的 frame | 直接接上 | 還要搬導覽 | 全部重做 |
| 新的訊息 | 建立、移除、改名、開始導覽、沒有結果；commit 類的多一個 frame | 建立、移除、改名、commit 的回報 | 無 |
| 子 frame 導覽多的成本 | 經過 UI thread 一趟 | 無 | 無 |
| 改動的大小 | 最大：`browser`、`protocol`、`renderer`、`dom`、`dom_api`、`shell` | 中 | 小 |

不選 B 的主要理由：要看回應的檢查和 origin 的可信度都還在 renderer，之後還是要搬導覽，等於同一段程式改兩次；而子 frame 的導覽是這份裡最容易出錯的部分，分兩次改的風險比一次高。

不選 C 的主要理由：和 RFC 0019 的第 6 條（session history 由 Browser 擁有）相反，而且對「為什麼現在做」列的每一項都沒有幫助。

## 之前的決定

這份接著或取代的：

- **RFC 0009** 的第 22 條（iframe 的 `src` 由 renderer 載入、Browser 不知道子 frame）：第 2 階段取代。它的「和 Chromium 不同的地方」的「iframe 的導覽由誰負責」「子 frame 的錯誤頁」兩列，和「之後的可能」的 3a，就是這份。`details.md` 的第 7 節（協定帶 frame 的 id、不假設 frame 都在同一個 renderer）照舊是前提。
- **RFC 0019** 的第 6 條（session history 由 Browser 擁有，entry 記 document 的序號）：照舊，擴到每個 frame。第 38 條（子 frame 的 `pushState` 留在 renderer、`length` 和 `back()` 是主 frame 的）：第 3 階段取代。
- **RFC 0021** 的第 24 條（frame 的 id 是 renderer 的前綴加號碼，跨過 frame 自己的導覽）：「跨過 frame 自己的導覽」照舊；id 的寫法由這份的第 2 條取代。
- **RFC 0004**：訊息照舊是 `protocol` 的 serde enum、一個 renderer 一條控制通道、收的一方檢查每個欄位。site isolation 在它的「之後的可能」，這份不做。
- **RFC 0103** 的「架構上可能要先做的事」第 2 項。

## 參考資料

- Chromium 的 `docs` 目錄的 `navigation.md`、`navigation_concepts.md`、`session_history.md`、`special_case_urls.md`，和 `content/common/frame.mojom`（<https://chromium.googlesource.com/chromium/src/+/main/docs/navigation.md> 等）。
- HTML：「Navigation and session history」（<https://html.spec.whatwg.org/multipage/browsing-the-web.html>）。
