# RFC 0104：frame 樹和每個 frame 的導覽由 Browser 擁有

- 開始日期：2026-11-18
- 狀態：🚧 已排程：2026-11-18（第 1 階段；第 2 到 4 階段的問題到那個階段開始前再決定）
- 細節：`details.md`
- 進度：`progress.md`

## 摘要

Browser 的每個分頁多一棵 frame 樹（Chromium 的 `FrameTree`、`FrameTreeNode`）：renderer 建立、移除子 frame 時告訴 Browser。子 frame 換文件的導覽和主 frame 一樣，由 Browser 的 `NavigationRequest` 檢查、載入、跟 redirect，再叫那個 frame commit。session history 的每個 entry 記整棵樹（`FrameNavigationEntry`），子 frame 的導覽進歷史，上一頁回得去。

做法、範圍和 frame 的 id 確認了（「已決定的事項」）；每個階段的細節寫的是 2026-11-18 的看法，在那個階段開始前才定。

## 動機

RFC 0103 的「架構上可能要先做的事」列了六個拖越久越貴的缺口，這是第 2 個。RFC 0009 當時把它排成第 3 階段的 3a，後來移到「之後的可能」（RFC 0009 的第 22、53、59 條）；使用者當時就傾向和 Chromium 一樣。

### 現況

2026-11-18 讀程式得到的（逐項的位置在 `details.md` 的「現況」）：

- **Browser 不知道子 frame**。`WebContents` 只有一個網址、一個標題、一個 DevTools 的 frame token、一個進行中的導覽；`NavigationController` 的 entry 是一條平的清單；`protocol` 裡和導覽有關的訊息都不帶 frame。
- **子 frame 的文件由 renderer 自己載入**。iframe 的 `src`、子 frame 裡的連結、`location` 都走 `navigate_child`：renderer 送一個 `RequestDestination::Iframe` 的 `FetchResource`（和圖片、script 同一條路，在 Browser 的 IO thread 上處理），自己跟 redirect、自己解碼、自己 commit。要不要載入、能不能載入，全部由 renderer 決定。
- **主 frame 才有的檢查，子 frame 都沒有**：能導覽的 scheme、`file:` 的權限（`RenderProcessHost` 的 `file_access` 只由主 frame 的 commit 設定）、錯誤頁（子 frame 載入失敗是一份空的文件）。
- **子 frame 沒有歷史**。子 frame 換文件、`pushState`、導覽到 `#fragment` 都不進 session history；子 frame 的 `history.length` 和 `back()` 用的是主 frame 的（RFC 0019 的第 38 條，當時就記成近似）。
- **frame 的 id 是 renderer 發的**（renderer 的前綴加號碼，RFC 0021 的第 24 條），Browser 原樣轉交，不認得也不檢查。
- **DevTools 的事件分兩邊**：主 frame 的導覽（`Page.frameStartedNavigating`、`Network` 的文件請求）由 Browser 發，loader id 是導覽的 id；子 frame 的由 renderer 發，loader id 是 renderer 的請求 id。

renderer 這一側其實已經很完整：子 frame 有自己的 realm 和 `WindowProxy`，具名的 target、`_parent`、`_top`、跨源的 `location` 都做了。缺的是 Browser 這一側。

### 為什麼現在做

之後要做的這些功能，每一個都要 Browser 知道「這是哪個 frame、誰嵌著它、它現在是哪個 origin」，而且要在回應交給 renderer **之前**決定：

- `sandbox`、`X-Frame-Options`、CSP 的 `frame-ancestors`、permissions policy（iframe 的 `allow`）。
- cookie 算哪個網站（`SameSite`、最上層的網站），和持久資料的 key（origin 加最上層的網站）。
- `Sec-Fetch-Dest`、`Sec-Fetch-Site`、`Referer`。
- 跨 process 的 frame（site isolation）和跨語言的 frame（wasm 文件的 iframe）：RFC 0004 和 RFC 0009 都寫了它們要走「Browser 的 frame 樹」這條路。

現在每加一個和導覽有關的功能，就多一處「renderer 自己決定」的程式，之後都要搬。

### 目標

1. Browser 有每個分頁的 frame 樹：哪些 frame 存在、誰是誰的子 frame、每個 frame 現在的網址和 origin，以 Browser 的為準。
2. 任何 frame 換文件的導覽都經過 Browser 的 `NavigationRequest`：同一套檢查、同一套 redirect 的處理、同一份錯誤頁。之後新的檢查只有一個地方要加。
3. session history 記整棵樹（joint session history）：子 frame 的導覽和 `pushState` 進歷史，上一頁、下一頁、重新整理照 entry 還原每個 frame。
4. 協定帶 frame 的 id，Browser 檢查 renderer 給的每一個（`docs/architecture.md` 的「Process 的邊界」第 6 條）。
5. 現有的行為不退步：iframe 的 WPT、情境、DevTools 和 WebDriver 的 frame。

### 不在範圍內

- 跨 process 的 frame、遠端 frame 的佔位（`RemoteFrame`）、site isolation：RFC 0004 之後。這份只保證做完之後它們有地方接。
- `sandbox`、`X-Frame-Options`、CSP、permissions policy、cookie、`Referer`、fetch metadata：各自的 RFC。這份只提供它們要掛的地方（`NavigationRequest` 和 `FrameTreeNode`）。
- `window.open` 和新的分頁之間的關係（opener、具名的 target 找到別的分頁）。
- back-forward cache、還原捲動位置、`beforeunload`、表單送出造成的導覽。
- 串流的載入（文件照舊一次收完）。
- wasm 文件裡的子 frame。

### 要解決的限制

做完時從 `docs/limitations.md` 刪掉或改寫：

- 「iframe」：「子 frame 的導覽由 renderer 自己載入……沒有子 frame 的歷史」那一條；「載入失敗的子 frame 是一份空的 document」那一條。
- 「連結導航和 hit testing」的 `history` 那一條裡，子 frame 的 `pushState`、`length`、`back()` 的部分。
- 「Bindings 和 DOM API」的 Navigation Timing：子 frame 的 entry 沒有 `back_forward`。

## 設計概要

對使用者：

- 在 iframe 裡點連結換了頁，按上一頁是 **iframe 回到上一份文件**，不是整頁離開。
- iframe 裡的 `history.pushState()` 之後按上一頁，iframe 收到 `popstate`。
- 重新整理、或上一頁回到某一頁時，裡面的 iframe 回到當時各自的網址，不是 `src` 寫的那個。
- iframe 載入失敗時顯示和主 frame 一樣的錯誤頁，不是一片空白。

對網頁的作者：`history.length` 在每個 frame 都是整個分頁的 joint session history 的長度，和 Chrome 一樣；子 frame 的 Navigation Timing 的 `type` 有 `back_forward`。

對之後改這部分程式的人：

- 「這個 frame 能不能載入這個網址、回應能不能給它」只有一個地方：Browser 的 `NavigationRequest`。
- `WebContents` 有 frame 樹可以問：某個 frame 的父 frame、origin、最上層的網站。
- 所有和 frame 有關的訊息都帶 frame 的 id；renderer 給了 Browser 不認得的 id，就是壞掉的訊息。

留在 renderer 的事（Chromium 也是）：子 frame 一建立就有的初始 `about:blank`（同步）、`javascript:` 網址、同一份文件內的導覽（renderer 先做，再告訴 Browser）。

## 詳細設計

以下是方向，每一塊的細節在它那個階段開始前才定（見「分階段」和「待決定」）。Chromium 的對應寫在「先例」和 `details.md` 的「Chromium 的做法」。

### frame 樹（`browser`）

- 每個 `WebContents` 一棵 `FrameTree`，節點是 `FrameTreeNode`：frame 的 id（`protocol` 的 `FrameToken`，第 3 條）、父節點、子節點（照建立的順序）。第 2 階段加上目前的文件（網址、Browser 自己算的 origin、是不是初始的空白文件、是不是錯誤頁）和進行中的 `NavigationRequest`（最多一個），第 3 階段加上歷史要用的鍵（第 8、5 條）。
- 主 frame 是根節點，它的 id 就是現在 DevTools 的主 frame 的 token，由 Browser 抽。網址列的網址（Chromium 的 visible URL）和標題留在 `WebContents`；進行中的導覽第 2 階段搬到節點上（第 8 條）。
- 子 frame 的 id 由 engine 建子 frame 時抽（第 4 條），engine 內部的 `FrameId` 照舊當索引。
- renderer → Browser 的新訊息：`DidCreateChildFrame { parent, frame }` 和 `DidDetachFrame { frame }`，後者拿掉那個 frame 和它的子樹（第 5 條）。建立是 renderer 發起的：iframe 插入時 script 馬上要拿到 `contentWindow`，不能等 Browser。
- 一個 renderer 的訊息照順序到（RFC 0004：控制通道只有一條），所以 Browser 一定先聽到某個 frame 建立，才聽到它的導覽。
- 子節點只在 renderer 送 `DidDetachFrame` 時拿掉，Browser 不在 commit 時自己推斷：renderer 換文件時本來就對舊文件的每個子 frame 送一次（第 6 條）。renderer 重開時，Browser 把樹清回只剩根節點。
- Browser 檢查：父 frame 存在、新的 id 沒有用過、被移除的 frame 存在而且不是根。不合的訊息照 Chromium 的 `bad_message::ReceivedBadMessage` 處理：記錯誤、結束那個 renderer，分頁顯示 sad tab（第 7 條）。

### 導覽（`browser`、`protocol`、`renderer`）

- renderer 要某個 frame 換文件時，送「開始導覽」（Chromium 的 `BeginNavigation`）：哪個 frame、網址、是不是 replace、是不是 reload、發起的 frame。子 frame 現在的 `FetchResource`（`RequestDestination::Iframe`）拿掉；主 frame 現在的 `OpenUrl { CurrentTab }` 併進同一則訊息。
- Browser 在那個節點上建 `NavigationRequest`（現在的 `PendingNavigation` 加上 frame 和發起者）：
  1. 檢查網址：現在 `WebContents::open_url` 的那幾條，照 frame 分（例如頁面不能把主 frame 導覽到 `data:`，子 frame 可以）。
  2. 載入：`net` 的請求，`RequestDestination` 照 frame 是 `Document` 或 `Iframe`；redirect 回到 UI thread 決定。
  3. 回應到了：之後的檢查（`X-Frame-Options` 等）掛在這裡。
  4. 叫那個 frame commit：`CommitNavigation` 多一個 frame 的 id。失敗時 commit 錯誤頁（`loader::error_page`，和主 frame 同一份）。
- renderer commit 之後回報（`DidCommitNavigation` 帶 frame 的 id），Browser 更新節點和歷史。
- 一個 frame 同時只有一個導覽：新的取消舊的；父 frame commit 新文件時，子孫的導覽全部取消。
- 父文件的 `load` 等子 frame：renderer 記著「這個 frame 有導覽在 Browser 那邊」，直到 commit 到了，或 Browser 說這次導覽沒有結果（被擋下、被取消）。後者是一則新的 Browser → renderer 訊息。
- 導覽的 id 照舊是每個分頁一個序列，子 frame 的也從它拿。DevTools 的 loader id 就是它，主 frame 和子 frame 一致。
- 同一份文件內的導覽照舊由 renderer 先做，回報多一個 frame 的 id。Browser 檢查回報的網址和那個 frame 現在的文件同源（現在不檢查）。

### session history（`browser`、`dom`、`dom_api`）

- `NavigationEntry` 從「一個網址」變成一棵 `FrameNavigationEntry` 的樹，每個 frame 一個：網址、`history.state`、捲動還原的模式、item 的序號、document 的序號（現在的 `document` 欄位）。
- commit 時照 Chromium 的分類：
  - 主 frame：和現在一樣（新的 entry、取代、reload、回到既有的 entry）。
  - 子 frame 建立之後的第一次導覽：併進目前的 entry，不多一個 entry（上一頁不會回到「iframe 還沒載入」的狀態）。
  - 子 frame 之後的導覽：複製目前的 entry、換掉那個 frame 的那一格，當成新的 entry（前進的 entry 照舊丟掉）。
- 上一頁、下一頁：比較目標 entry 和現在的樹，找出要動的 frame。item 的序號不同、document 的序號相同的，送同一份文件內的 traversal；document 的序號不同的，那個 frame 重新載入，它底下的不用看（新文件會自己建）。
- 回到某個 entry、或重新整理之後，新文件建出來的子 frame 照 entry 裡記的網址載入，不是照 `src`。要一個「同一個位置的 frame」的鍵（Chromium 的 unique name）。
- `history.length`、`history.go()` 在每個 frame 都照整個分頁的清單。

### DevTools 和自動化（`shell`、`renderer`）

- 跟著 `NavigationRequest` 走的事件（`Page.frameStartedNavigating`、`Page.navigatedWithinDocument`、`Network` 的文件請求）子 frame 的也由 Browser 發。
- frame 的樹和它的事件（`Page.getFrameTree`、`frameAttached`、`frameNavigated`、`frameDetached`）現在由 renderer 的 Page agent 發，先不動。
- 子 frame 的 `frameId` 和 WebDriver 的 frame 參照，第 1 階段就換成 `FrameToken` 的寫法（32 個大寫 hex，和主 frame 的一樣；第 8 條），machopodriver 拿到的字串跟著變：它只把字串原樣帶回來，不解析。WebDriver 從 id 找 frame 改成在 renderer 記著的子 frame 裡找：renderer 收到 `FrameRequest::Created` 時，把 token 和那個 frame 的父 frame、iframe 記在一起（2026-11-18 實作時定的：DevTools 和 WebDriver 讀 id 的地方不能改 engine，而 `Removed` 送到 renderer 時 engine 已經沒有那個 frame 了），engine 的介面只多了 `set_main_frame_token`。元素和文件的參照照舊用 renderer 的前綴。
- `DidFinishLoad`、標題、testharness 的結果照舊只有主 frame 的（RFC 0009 的 `details.md` 第 6 節）。

### 測試

- `browser` 的單元測試：frame 樹的建立、移除、不合法的訊息；`NavigationController` 的樹（分類、複製、丟掉前進的 entry、找出要動的 frame）。
- `browser` 的整合測試（`crates/browser/tests/it/navigation.rs`）：子 frame 的導覽經過 Browser、取消、錯誤頁、父 frame 換文件時子 frame 的導覽被取消。現在的 `a_child_frame_s_location` 守的是「Browser 什麼都沒聽到」，要改寫。
- 視窗裡看得到的行為寫成情境：iframe 裡點連結、按上一頁；iframe 載入失敗的錯誤頁。
- WPT：`html/browsers/history/` 和 `html/browsers/browsing-the-web/` 裡子 frame 的測試、`html/semantics/embedded-content/the-iframe-element/` 現在等著「子 frame 的導覽」的那些（`tests/wpt/include.txt` 的註解）。各階段開始前先試跑、看哪些目錄值得加。
- 每個階段照 `docs/testing.md` 跑 mutation test。
- 不排 Speedometer 的量測（它每個 suite 建一個 iframe、設 `src`，會走新的路；要量時由使用者要求）。

### 分階段

每個階段做完，`main` 上的行為和文件要一致，可以單獨 merge。

1. **Browser 的 frame 樹**：協定裡 frame 的 id 的型別；建立、移除的訊息；Browser 的 `FrameTree`、檢查和壞掉的訊息的處理；DevTools 和 WebDriver 的 frame 的 id 換成 token。子 frame 的載入還是 renderer 做，使用者看不出差別。決定是「已決定的事項」第 2 到 8 條。分三段：1a 是 `FrameToken`、engine 抽 id、renderer 送兩則訊息、DevTools 和 WebDriver 的 id；1b 是 Browser 的 `FrameTree`、檢查和壞掉的訊息；1c 是整個階段的檢查和文件。
2. **子 frame 的導覽經過 Browser**：開始導覽的訊息、每個節點的 `NavigationRequest`、`CommitNavigation` 帶 frame、子 frame 的錯誤頁、拿掉 renderer 自己載入的那條路、DevTools 的導覽事件。開始前要決定：`about:blank` 和 `srcdoc` 的導覽經不經過 Browser；Browser 沒有結果的導覽怎麼告訴 renderer；DevTools 的事件怎麼分。
3. **joint session history**：entry 的樹、子 frame 的分類、traversal、子 frame 的 `pushState`、回到 entry 時子 frame 照 entry 載入、frame 的名稱（第 5 條）。開始前要決定：frame 的鍵怎麼算（連同名稱怎麼送）；reload 時子 frame 怎麼辦。
4. **收尾**：Browser 對 renderer 回報的檢查補齊、WPT 的目錄、文件、RFC 改寫成完成的樣子。

第 2、3 階段都會再分小段，到時候才分。

## 缺點

- **Browser 多了一份要和 renderer 一致的狀態**。兩邊的樹對不上就是 bug，而且拆成 process 之後，對不上的訊息會結束 renderer。
- **子 frame 的每次導覽多經過 UI thread 一趟**（現在直接到 IO thread）。文件的 bytes 照舊要從 Browser 送到 renderer，這部分沒有多。
- **session history 是 Chromium 裡出名難對的部分**：分類、複製、子 frame 的鍵都有很多邊界情況，要靠 WPT 和 Chrome 的實測對。
- **要改寫的測試不少**：現在有測試守著「Browser 聽不到子 frame」，DevTools 的子 frame 事件的 loader id 也會變。
- **做完時使用者看得到的新東西不多**（iframe 的上一頁、錯誤頁）：主要的收益在之後的功能。

## 理由和其他做法

選 A（「已決定的事項」第 1 條）。完整的比較在 `details.md` 的「做法的比較」。

- **A：樹、導覽、歷史都搬到 Browser**（這份寫的）。和現在的 Chromium 一樣；之後的每個功能都直接有地方放。成本最高。
- **B：只做樹，導覽照舊由 renderer 載入**（Chromium 在 2017 年前後把導覽搬到 Browser 之前的樣子）。便宜很多，DevTools 和之後的 policy 有樹可以查。但要看回應才能決定的檢查（`X-Frame-Options`、`frame-ancestors`、sandbox 的 origin）還是只能在 renderer 做，之後照樣要搬一次；子 frame 的歷史也還是要另外想辦法。
- **C：都留在 renderer，子 frame 的歷史也由 renderer 記**，Browser 的每個 entry 帶一包 renderer 給的資料。最便宜，iframe 的上一頁馬上能做。但違反 RFC 0019 的第 6 條（session history 由 Browser 擁有），對 site isolation 和各種 policy 沒有幫助。
- **D：等到拆 process（RFC 0004）時一起做**。兩件大事疊在一起；現在 renderer 還是 thread，測試便宜、除錯容易，先做這份比較穩。RFC 0103 的評估也把它排在拆 process 之前。
- **不做**：每加一個和 frame 有關的功能，就多一處之後要搬的程式。

## 先例

- **Chromium**（2026-11-18 讀了它的 `navigation.md`、`navigation_concepts.md`、`session_history.md`、`special_case_urls.md` 和 `content/common/frame.mojom`；逐項在 `details.md` 的「Chromium 的做法」）：
  - Browser process 主導導覽：檢查、網路請求、redirect、選 renderer，再送 commit；renderer 回覆之後才算 commit。照做。
  - renderer 用 `CreateChildFrame` 告訴 Browser 新的子 frame（非同步，帶 frame token、名稱、unique name），移除時有對應的 `Detach`。照做。
  - 每個 frame 各自有 `NavigationRequest`，同一個 frame 裡新的導覽取消舊的。照做。
  - session history 是 `NavigationEntry` 的清單，每個 entry 一棵 `FrameNavigationEntry` 的樹；子 frame 的第一次 commit 併進目前的 entry，之後的才是新的 entry；用 item 和 document 兩個序號決定哪些 frame 要動。照做。
  - 同一份文件內的導覽由 renderer 做完再告訴 Browser；`javascript:` 網址不是導覽，不進歷史；錯誤頁是一次 commit。照做。
  - 不照做的：每個 frame 一個 `RenderFrameHost`、跨 process 的 proxy、speculative 的 `RenderFrameHost`、`NavigationThrottle` 的機制、back-forward cache、prerender。machopo 一個分頁一個 renderer，節點上直接記目前的文件。
- **Firefox、Servo**：Browser 這一側也有 browsing context 的樹和 session history（照記憶寫的，沒有查證；排程前要查的話再補）。

## 已決定的事項

1. **做法是 A，樹、導覽、歷史都在這一份做**（2026-11-18）：Browser 的 frame 樹、每個 frame 的 `NavigationRequest`、joint session history 都搬到 Browser，照「分階段」的四個階段在同一份 RFC 裡做完，歷史不拆成另一份。理由：既然要重構就一次做完；子 frame 的導覽是最容易出錯的部分，不分兩次改。不選 B（只做樹）：要看回應的檢查還在 renderer，之後還要再搬一次導覽。不選 C（都留在 renderer）：違反 RFC 0019 的第 6 條，對之後的功能沒有幫助。不選「A，但歷史另開 RFC」：entry 的設計要回頭配合前兩個階段。
2. **子 frame 的 id 由 renderer 抽**（I1，2026-11-18）：renderer 建子 frame 時抽一個 128 bit 的亂數當它的 id，在建立的訊息裡告訴 Browser；主 frame 的照舊由 Browser 抽。id 跨過那個 frame 自己的導覽，DevTools 的 `frameId` 和 WebDriver 的 frame 參照直接用它，和主 frame 的 token 同一種寫法（取代 RFC 0021 第 24 條的「renderer 的前綴加號碼」，「跨過 frame 自己的導覽」照舊）。Browser 的樹以分頁為範圍，重複的 id 是壞掉的訊息：renderer 自己挑的 id 只影響它自己的分頁。之後 frame 搬到別的 renderer 時 id 不變，由 Browser 告訴新的 renderer。不選 I2（照舊的前綴加號碼）：前綴的意思是「哪個 renderer」，frame 搬到別的 renderer 之後就不對，到時要再改一次對外的字串。不選 I3（Browser 發，Chromium 的做法：renderer 同步向 Browser 的 IO thread 要，`RenderMessageFilter::GenerateSingleFrameRoutingInfo`）：machopo 的 renderer 現在沒有任何地方同步等 Browser 的回覆，要先做那套機制（或預發一批、再處理用完的情況），換來的只是 Browser 不必檢查重複；之後有了同步的請求，要改成 I3 只動抽 id 的地方，型別和用法不變。
3. **frame 的 id 是 `protocol` 的 `FrameToken`：16 個 byte 的值**（2026-11-18，第 1 階段開始前）：主 frame 和子 frame 共用；只在給 DevTools 和 WebDriver 時寫成 32 個大寫 hex（和現在主 frame 的 token 一樣）。現在以 `String` 傳的 `RendererMsg::DevToolsMainFrameToken` 也改用它。理由：格式由型別保證，收的一方只要檢查認不認得；當樹和歷史的鍵時是固定大小、`Copy`、不用配置。Chromium 的 frame token 是 `base::UnguessableToken`（128 bit 的值），只在給 DevTools 時轉成字串。不選照舊用 `String`：每次都要檢查長度和字元，當鍵要配置。
4. **子 frame 的 token 在 engine 建子 frame 時抽**（T1，2026-11-18，第 1 階段開始前）：`dom_js` 的 `frames::create_child` 用 `dom_api` 的小函式（`dom_api` 已經依賴 `getrandom`）抽一個，存在 engine 的 `Frame` 上，`FrameRequest::Created` 帶給 renderer。engine 內部的 `FrameId`（u64 的號碼）照舊是 engine 和 renderer 內部的索引，`FrameToken` 只用在對外（Browser、DevTools、WebDriver）。理由：DevTools 在建 frame 的當下就要 id（`devtools::realm_created`），那時 renderer 還沒收到 `Created`；id 和 frame 的其他資料放在同一個擁有者身上。Chromium 也分對外的 `LocalFrameToken` 和內部的 id。不選 T2（renderer 收到 `Created` 才抽）：DevTools 當下拿不到，要一張 renderer 和 engine 共用、執行時借用檢查的表；不選 T3（renderer 給 engine 抽 id 的 closure）：只是把一個 `getrandom` 繞到 renderer 再繞回來。
5. **兩則新訊息：`DidCreateChildFrame { parent, frame }`、`DidDetachFrame { frame }`；名稱第 3 階段才送**（2026-11-18，第 1 階段開始前）：detach 一個 frame 時，它的子樹一起拿掉，一棵子樹一則訊息。理由：renderer 現在就只對直接的子 frame 排 `Removed`、子孫跟著走（`frames::discard_descendants`）；Chromium 的 `FrameHost::Detach` 也是。名稱（iframe 的 `name`）在第 3 階段的歷史之前沒有地方用到，frame 的鍵（Chromium 的 unique name）到時一起設計，所以 RFC 原本排在第 1 階段的「改名」移到第 3 階段。不選每個被拿掉的 frame 各送一則：renderer 要先展開子樹，訊息變多。
6. **子節點只照 renderer 的 `DidDetachFrame` 拿掉，Browser 不在 commit 時推斷**（2026-11-18，第 1 階段開始前）：renderer 換文件時本來就會對舊文件的每個直接子 frame 送 detach；renderer 重開時 Browser 把樹清回只剩根節點。理由：「建立和移除只有 renderer 說了才算」一條規則，Browser 才能把「拿掉不存在的 frame」一律當成壞掉的訊息；第 1 階段子 frame 的 commit Browser 也看不到。Chromium（2026-11-18 讀 `content/browser/renderer_host/`）：子 `FrameTreeNode` 由目前文件的 `RenderFrameHost` 擁有；清掉子節點的 `ResetChildren()` 只在 `RenderFrameHostImpl` 刪掉時、renderer 掛掉時（`RenderProcessGone`）和嵌入另一棵樹的特例呼叫，commit 的路上沒有；正規的移除是 renderer 的 `Detach`。不選 Browser 在 commit 時自己清（RFC 原本寫的）：第 1 階段推不出子 frame 的；也不選兩者都做：同一件事兩個來源，後到的一則要放寬檢查。
7. **renderer 送來不合理的訊息時，Browser 結束那個 renderer**（2026-11-18，第 1 階段開始前）：記一行 error（哪一種壞訊息）、`RenderProcessHost::kill`、分頁標成 crashed（sad tab、DevTools 的 `targetCrashed`），樹清回只剩根節點；用一個小函式做（Chromium 的 `bad_message::ReceivedBadMessage`），之後收的一方的各種檢查都用它。第 1 階段的範圍是 `DidCreateChildFrame`（父 frame 不在、id 用過）和 `DidDetachFrame`（不在、是主 frame）；現有的寬鬆處理（例如不認得 id 的 `DidCommitNavigation` 當成新的 entry）到第 2 階段改導覽時一起改。理由：現在壞訊息只可能是 machopo 自己的 bug，結束 renderer 讓它立刻浮出來，兩棵樹也不會繼續錯下去；Chromium 的 `RenderFrameHostImpl::Detach()` 對 detach 主 frame 就是報 bad message。不選記 log、忽略：兩棵樹悄悄地不一致，拆 process 之後被攻破的 renderer 可以故意弄錯 Browser 的樹；不選 debug build 先 panic：「Browser 擋得住壞訊息」的測試寫不了。
8. **第 1 階段的節點只放結構；DevTools 和 WebDriver 的 frame 的 id 第 1 階段就換**（2026-11-18，第 1 階段開始前）：節點有 id、父節點、子節點；根節點的 id 就是主 frame 的 token。目前的文件（網址、origin）和進行中的導覽第 2 階段才放上節點，那時 Browser commit 每個 frame，所有節點都有；網址列的網址和標題留在 `WebContents`（Chromium 的 visible URL 也是從 `NavigationController` 算的，不是 frame 的屬性）。子 frame 的 `frameId`、WebDriver 的 frame 參照在第 1 階段換成 `FrameToken` 的寫法；WebDriver 的 renderer 前綴照舊用在元素和文件的參照。理由：第 1 階段子 frame 的網址 Browser 不知道，同一個欄位一半可信、一半是空的容易誤用；同一個 frame 不要有兩種 id，而 `shell`、`browser`、`driver` 的測試和 wptrunner 都只把 id 原樣傳回去，沒有依賴舊的格式。不選第 1 階段就把網址和導覽搬到節點（RFC 原本寫的）；也不選 id 第 2 階段才換：renderer 裡兩種 id 並存一段時間。

## 待決定

接受這份 RFC 前要決定的都定了。各階段開始前要決定的事列在「分階段」，到時候才問、寫進「已決定的事項」。

## 之後的可能

- 掛在 `NavigationRequest` 和 `FrameTreeNode` 上的功能：`sandbox`、`X-Frame-Options`、CSP 的 `frame-ancestors`、permissions policy、cookie 的網站、`Referer`、fetch metadata。
- 跨 process 的 frame（RFC 0004 的「之後的可能」）和跨語言的 frame（RFC 0009）：節點上多一個「這個 frame 在哪個 renderer」，其他 renderer 裡是遠端 frame 的佔位。
- `Page.getFrameTree` 和 frame 的事件由 Browser 回答；`Page.navigate` 的 `frameId`。
- `window.open`、opener、具名的 target 找到別的分頁。
- back-forward cache、還原捲動位置、`beforeunload`、表單的導覽。
