# RFC 0104：frame 樹和每個 frame 的導覽由 Browser 擁有

- 開始日期：2026-11-18
- 狀態：🚧 已排程：2026-11-18（第 1 階段做完了；第 2 階段的決定定了，開始做；第 3、4 階段的問題到那個階段開始前再決定）
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

- renderer 的訊息帶著送出它的 renderer（`ContentsEvent::Renderer` 多帶 `RenderProcessHost` 的編號），`WebContents` 只處理目前那個 renderer 的：被結束或放棄的 renderer 晚到的訊息都不算（第 9 條）。
- renderer 要某個 frame 換文件時送 `BeginNavigation { frame, initiator, kind }`（Chromium 的 `BeginNavigation`），主 frame 和子 frame 同一則：`kind` 是「到某個網址（取代目前的 entry 與否）」或「重新載入」，帶著 renderer 給的編號；`initiator` 是發起的 frame，Browser 從自己的樹查它的 origin。主 frame 現在的 `OpenUrl { CurrentTab }` 和 `Reload` 併進來，`OpenUrl` 只留給開新分頁；子 frame 現在的 `FetchResource`（`RequestDestination::Iframe`）拿掉（第 10 條）。
- 不是初始的 `about:blank` 和 `about:srcdoc` 也經過 Browser，不發網路請求、直接 commit；`srcdoc` 的內容由 renderer 用 `DidChangeSrcDoc { frame, srcdoc }` 告訴 Browser（iframe 的屬性改了、或建立時就有），記在節點上，commit 時帶回去。只差 `#fragment` 的導覽留在 renderer（第 11 條）。
- Browser 在那個節點上建 `NavigationRequest`（現在的 `PendingNavigation` 加上 frame、發起者和 renderer 的編號）：
  1. 檢查網址：scheme 的白名單（`http`、`https`、`file`、`about`、`blob`，子 frame 加上 `data:`；頁面能導覽到的 `about:` 只有 `about:blank`、`about:srcdoc`）；`file:` 要發起的 frame 目前的文件是 `file:`（第 14 條）。
  2. 載入：`net` 的請求，`RequestDestination` 照 frame 是 `Document` 或 `Iframe`；redirect 回到 UI thread 決定。`about:` 的不發請求。
  3. 回應到了：之後的檢查（`X-Frame-Options` 等）掛在這裡。
  4. 叫那個 frame commit：同一則 `CommitNavigation` 多帶 `frame` 和 renderer 的編號；Navigation Timing 的載入資料主 frame 和子 frame 都由 Browser 給。失敗時 commit 錯誤頁（`loader::error_page`），帶著「這是錯誤頁」的旗標，renderer 給它 opaque 的 origin（文件的網址是 `chrome-error://chromewebdata/`，第 21 條），主 frame 的也一樣（第 13 條）。導覽到 `about:srcdoc` 照 Chromium 的四條規則檢查，不放行的 commit 錯誤頁（第 23 條）；`about:blank` 的文件是發起的文件的 origin，renderer 在請求導覽時記下發起的文件（第 24 條）。
- 沒有結果的導覽（網址的檢查沒過、被同一個 frame 更新的導覽取代），Browser 送 `NavigationDropped { frame, id }`，用 renderer 的編號對上是哪一次。renderer 送出 `BeginNavigation` 之後到 commit 或「丟掉了」之前，那個 frame 算在載入中，父文件的 `load` 等它（第 12 條）。
- renderer commit 之後回報（`DidCommitNavigation` 帶 frame），Browser 更新節點目前的文件（網址、origin、是不是錯誤頁）；歷史第 3 階段才照 frame 記。一個 renderer 裡任何 frame commit 了 `file:` 的文件，子資源讀 `file:` 的旗標就設起來（第 14 條；做的時候在送出 commit 時決定：main frame 的新文件決定，子 frame 的本機文件只會加上，見 progress 的 2c）。
- 一個 frame 同時只有一個導覽：新的取消舊的；某個 frame 被 detach 時，它和子孫的導覽全部取消。
- 導覽的 id 照舊是每個分頁一個序列，子 frame 的也從它拿。DevTools 的 `loaderId` 就是它，主 frame 和子 frame 一致；`frameStartedNavigating` 和文件的 `Network` 事件改由 Browser 發，所有 frame（第 15 條）。
- 同一份文件內的導覽照舊由 renderer 先做，每個 frame 都送 `DidCommitSameDocumentNavigation`（多帶 frame）：子 frame 的 Browser 只更新節點的網址（重新載入時用它），不進歷史；網址要和那個 frame 目前的文件同源，不同源是壞掉的訊息（第 16 條；照 HTML 的「can have its URL rewritten」比對節點的網址，第 25 條）。

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
2. **子 frame 的導覽經過 Browser**：開始導覽的訊息、每個節點的 `NavigationRequest`、`CommitNavigation` 帶 frame、子 frame 的錯誤頁、拿掉 renderer 自己載入的那條路、DevTools 的導覽事件。決定是「已決定的事項」第 9 到 16 條，各段開始前的是第 17 到 25 條。分六段：2a 是 renderer 的訊息帶著 renderer 的身分；2b 是 `BeginNavigation` 和網址的檢查，先只換主 frame 的路（網頁能導覽到的 `about:` 只剩 `about:blank`，其他行為不變）；2c 是子 frame 一般的導覽經過 Browser（commit 帶 frame 和 renderer 的編號、`NavigationDropped`、Navigation Timing、錯誤頁和 opaque 的 origin、`file:` 照發起的文件檢查的權限、DevTools 的事件），拿掉 renderer 自己載入的路（`NavigationDropped` 和 `file:` 的檢查原本排在 2b：主 frame 的導覽用不到前者，後者要 Browser 知道子 frame 的網址，2026-11-18 做 2b 時移過來）；2d 是 `about:blank`、`about:srcdoc` 和 `DidChangeSrcDoc`，節點記 origin，`about:blank` 照發起者繼承 origin（第 23、24 條）；2e 是子 frame 同一份文件內的導覽的回報和網址的檢查（第 25 條）；2f 是整個階段的檢查和文件。
3. **joint session history**：entry 的樹、子 frame 的分類、traversal、子 frame 的 `pushState`、回到 entry 時子 frame 照 entry 載入、frame 的名稱（第 5 條）。開始前決定的是第 26、27 條。分五段：3a 是 iframe 的名稱和 unique name；3b 是 entry 變成一棵樹、子 frame 跨文件的導覽進歷史（第一次併進目前的 entry，之後的是新的 entry），`history.length` 照整個分頁；3c 是子 frame 同一份文件內的導覽進歷史；3d 是上一頁、下一頁跨 frame 走（同一份文件內的 traversal 帶 frame、重新載入的 frame、新的子 frame 照 entry 還原）；3e 是整個階段的檢查和文件。
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
9. **renderer 的訊息帶著 renderer 的身分**（2026-11-18，第 2 階段開始前）：`ContentsEvent::Renderer` 多帶送出它的 renderer 的編號（`RenderProcessHost` 的），`WebContents` 只處理目前那個 renderer 的，`RendererGone` 也是；取代第 1 階段「crashed 的期間不處理建立和 detach」的判斷。理由：被結束或放棄的 renderer 之前送出、已經排在外層程式的佇列裡的訊息，不管什麼時候被處理都不算，沒有時序上的漏洞；第 2 階段之後晚到的 commit 更危險。Chromium 每個 renderer process 一條 IPC 通道，process 不在了，晚到的訊息找不到收的人。不選「重開前先把舊的佇列處理完」：佇列在外層程式手上，`WebContents` 碰不到；不選維持現狀。
10. **一則 `BeginNavigation { frame, initiator, kind }` 給所有 frame**（2026-11-18，第 2 階段開始前）：`kind` 是「到網址（取代與否）」或「重新載入」；主 frame 的 `OpenUrl { CurrentTab }`、`Reload` 併進來，`OpenUrl` 只留給開新分頁。`initiator` 只帶發起的 frame 的 token，Browser 從自己的樹查 origin，不相信 renderer 說的 origin。理由：任何 frame 換文件都經過同一個 `NavigationRequest`；`file:` 的檢查和之後的安全檢查都要發起者。Chromium 的 `FrameHost::BeginNavigation` 主 frame 和子 frame 共用，參數帶 `initiator_frame_token` 和 `initiator_origin`。不選主 frame 照舊、子 frame 另一則（兩條路、檢查寫兩次）；不選在 `OpenUrl` 上加 frame（一則訊息兩個意思）；不選不帶發起者（父 frame 改 `src` 時 Browser 只看得到 iframe 裡的文件）。
11. **不是初始的 `about:blank` 和 `about:srcdoc` 也經過 Browser，`srcdoc` 的內容記在 Browser 的節點上；只差 `#fragment` 的導覽留在 renderer**（2026-11-18，第 2 階段開始前）：Browser 不發網路請求、直接叫 frame commit；renderer 在 iframe 的 `srcdoc` 屬性改了（和建立時就有）時送 `DidChangeSrcDoc { frame, srcdoc }`，commit 時 Browser 帶著節點上的值。Chromium（2026-11-18 查過）：`NavigationRequest::NeedsUrlLoader()` 對 `about:` 的網址和同一份文件內的導覽回 false，`BeginNavigationImpl()`「不需要網路請求，所以馬上 commit」；`CommitNavigationParams::srcdoc_value` 從 `frame_tree_node_->srcdoc_value()` 拿，節點上的值由 `LocalFrameHost::DidChangeSrcDoc` 告訴 Browser；同步的初始 `about:blank` 是 renderer 先 commit、Browser 事後補（`CreateForSynchronousRendererCommit`）。不選 `about:blank`、`srcdoc` 留在 renderer（兩條 commit 的路，歷史和 DevTools 要另外處理）；不選 `srcdoc` 的內容跟著 `BeginNavigation` 帶上去（Browser 自己發起的 srcdoc 導覽，例如第 3 階段的上一頁、重新整理，拿不到內容）。
12. **沒有結果的導覽，Browser 送 `NavigationDropped { frame, id }`**（2026-11-18，第 2 階段開始前）：`BeginNavigation` 帶 renderer 給的編號，`CommitNavigation` 帶回它（renderer 發起的才有），「丟掉了」也帶它；renderer 送出之後到 commit 或「丟掉了」之前，那個 frame 算載入中，父文件的 `load` 等它。renderer 要取消時不另外送訊息：新的 `BeginNavigation` 取代舊的，frame 被 detach 時它的導覽跟著取消。理由：父文件的 `load` 照 HTML 要等 iframe，Browser 決定不 commit 時 renderer 要知道；同一個 frame 連續發起兩次時要對得上是哪一次。Chromium（2026-11-18 查過）：renderer 的 `RenderFrameImpl::OnDroppedNavigation()`「renderer 發起、進行中的導覽被 Browser 丟掉時呼叫」，每次導覽一條 `NavigationClient` 通道，renderer 關掉它就是取消。不選只帶 frame 的「丟掉了」（連續兩次時對錯）；不選不送（父文件的 `load` 不等 iframe）。
13. **同一則 `CommitNavigation` 多帶 frame；子 frame 也用主 frame 的錯誤頁，錯誤頁的 origin 是 opaque 的**（2026-11-18，第 2 階段開始前）：commit 多帶 `frame` 和第 12 條的編號，歷史的參數第 2 階段只對主 frame 有意義；Navigation Timing 的載入資料主 frame 和子 frame 都由 Browser 給。失敗時 commit `loader::error_page`，帶「這是錯誤頁」的旗標，renderer 給文件 opaque 的 origin；主 frame 的錯誤頁也改成這樣，網址列和歷史照舊顯示原本的網址。理由：一條 commit 的路；錯誤頁不讓同源的父 frame 讀到失敗的原因。Chromium：`CommitNavigation` 是 frame 的介面上的方法，主 frame 和子 frame 共用；錯誤頁以 `chrome-error://chromewebdata` commit，那個網址不進 `NavigationEntry`（`special_case_urls.md`）。不選子 frame 另一則 commit；不選錯誤頁的 origin 照失敗的網址（同源的父 frame 讀得到）；不選子 frame 照舊 commit 空的文件（使用者看到一片空白）。
14. **網址的檢查照 frame 分：scheme 的規則搬到 Browser 一個地方；導覽到 `file:` 看發起的文件，子資源的 `file:` 看整個 renderer**（2026-11-18，第 2 階段開始前）：scheme 的白名單所有 frame 一樣（`http`、`https`、`file`、`about`、`blob`；子 frame 加 `data:`，頁面把主 frame 導覽到 `data:` 照舊不行）；頁面能導覽到的 `about:` 只有 `about:blank`、`about:srcdoc`，debug URL 照舊只有使用者自己輸入時才算。導覽到 `file:` 要發起的 frame 目前的文件是 `file:`；子資源讀 `file:` 照舊是整個 renderer 一個旗標，改成任何 frame commit 了 `file:` 就設（現在只看主 frame）。理由：第 2 階段起 Browser 知道每個 frame 目前的文件，照發起者判斷比較準，也不用相信 renderer；子資源的請求不帶 frame。Chromium（2026-11-18 查過）是同樣的兩層：Blink 照發起的文件的 origin 擋（`SecurityOrigin::CanDisplay`、`CanLoadLocalResources`），Browser 照 process 授權（`ChildProcessSecurityPolicy`：「看 Browser 是否曾經叫這個 process 載入過 `file://`」）；machopo 把照文件的那層放在 Browser。不選照舊全部看主 frame 的旗標（`file:` 的頁面裡 `http` 的 iframe 也能開 `file:`）。
15. **跟著導覽走的 DevTools 事件改由 Browser 發，跟著文件走的留在 renderer**（2026-11-18，第 2 階段開始前）：Browser 發所有 frame 的 `Page.frameStartedNavigating` 和文件的 `Network` 事件，`loaderId` 是 Browser 的導覽 id，renderer 的 `frameNavigated` 用 commit 帶來的同一個 id；`frameAttached`、`frameNavigated`、`frameDetached`、`frameStartedLoading`、`frameStoppedLoading`、`lifecycleEvent`、`getFrameTree` 留在 renderer。Chrome（2026-11-18 查過）：Browser 的 `PageHandler::DidStartNavigating` 發 `frameStartedNavigating`、`NetworkHandler::NavigationRequestWillBeSent` 發文件的 `requestWillBeSent`（`loaderId` 和 `requestId` 是導覽的 token），都不限主 frame；`frameStartedLoading`、`frameStoppedLoading` 也由 Browser 發；Blink 的 `InspectorPageAgent` 發 `frameAttached`、`frameNavigated`、`lifecycleEvent`、`navigatedWithinDocument`，回答 `getFrameTree`；`frameDetached` 兩邊都發。不選連載入狀態也搬到 Browser（要多做 Browser 的載入狀態，和這次無關，留到「之後的可能」）。
16. **子 frame 同一份文件內的導覽在第 2 階段就告訴 Browser，只更新節點的網址**（2026-11-18，第 2 階段開始前）：每個 frame 都送 `DidCommitSameDocumentNavigation`（多帶 frame）；主 frame 的照舊（網址、歷史），子 frame 的只更新 Browser 節點目前的網址，第 3 階段才進歷史；新的網址要和那個 frame 目前的文件同源，不同源是壞掉的訊息（主 frame 的也一起補上）。DevTools 的 `navigatedWithinDocument` 子 frame 的也由 Browser 發（machopo 現在主 frame 的做法；Chrome 是 renderer 發，這次不改）。理由：第 2 階段起子 frame 的重新載入由 Browser 用節點的網址發起，沒聽到 `pushState`、`#fragment` 就會用舊的網址，比現在差。不選第 3 階段才送。
17. **每個 frame 進行中的導覽放在它的 `FrameTreeNode` 上**（2026-11-18，2c 開始前）：節點有 `navigation: Option<NavigationRequest>`，main frame 現在的 `PendingNavigation` 也搬到根節點，所有 frame 一條路；載入的事件照導覽的 id 找節點（一個分頁的 frame 很少，掃一遍）。理由：frame 被拿掉時它和子孫的導覽跟著節點消失（`LoadHandle` 一起丟掉，載入取消），不會 commit 到不在的 frame；「一個 frame 同時一個導覽」由型別保證；和第 8 條一致。Chromium 的 `FrameTreeNode` 擁有 `navigation_request_`。代價是 `frame_tree` 依賴 `net` 的 `LoadHandle`。不選 `WebContents` 另一張以 token 為鍵的表：detach 時要記得清掉整棵子樹的，同一個鍵兩個來源。
18. **renderer 發起的導覽帶著 DevTools 的 initiator**（2026-11-18，2c 開始前）：`BeginNavigation` 多帶 `devtools_initiator`（DevTools 格式的 JSON 文字，renderer 的 `Initiator::to_json`），只在 renderer 的 `Network` agent 開著時帶；Browser 發文件的 `requestWillBeSent` 時放進去，沒帶、或讀不了時是 `other`。2c 只有 iframe 的 `src` 帶（`parser` 或插入它的 script 的 stack，和 renderer 自己發請求時一樣）；`location` 和連結發起的導覽 DOM 還沒記下 stack，先是 `other`，之後記下了就能帶（2026-11-18 做 2c 時改正：原本寫主 frame 由 script 發起的導覽也因此有 stack）。Chromium（2026-11-18 查過）：`BeginNavigationParams::devtools_initiator`「renderer 發起、而且正被 DevTools 檢查時」帶 DevTools protocol 格式的 initiator，`NetworkHandler::NavigationRequestWillBeSent` 用它、沒有時是 `Other`，不限 main frame。用 JSON 文字而不是型別：Browser 只是轉交，`Initiator` 留在 renderer。不選 Browser 一律 `other`（子 frame 的 `parser`、`script` 的 initiator 退步）；不選子 frame 的文件請求照舊由 renderer 發事件（一個請求兩個來源，違反第 15 條）。
19. **renderer 自己送出文件的請求是壞掉的訊息**（2026-11-18，2c 開始前）：`FetchResource` 的 destination 是 `Document` 或 `Iframe` 時，IO thread 回 `AccessDenied`，再經過一個 renderer 送不出來的 `ContentsEvent` 叫 UI thread 結束那個 renderer（第 7 條的 `received_bad_message`）。理由：2c 之後所有 frame 的文件都由 Browser 載入，renderer 沒有理由送；送了就是 bug，而且繞過 Browser 對導覽的檢查。成本是每個請求多比一次 destination。Chromium（2026-11-18 查過）：network service 的 `CorsURLLoaderFactory::IsValidRequest` 對文件類的 destination 不是 navigate mode 的請求報 bad message，renderer 送 navigate mode 的請求要 manual redirect、帶 redirect chain（service worker 用的；machopo 沒有）。不選拒絕、只記 warn（和第 7 條不一致）；不選照舊不管。
20. **`about:` 的文件發起的導覽，`file:` 照它繼承的來源判斷**（2026-11-18，2c 開始前）：每個節點記它目前的文件是不是 `file:` 來的：Browser commit 的文件看網址（錯誤頁不算），`about:srcdoc` 照父節點，`about:blank` 照發起它的 frame，剛建立的子 frame（初始的 `about:blank`）照父節點。2d 之前子 frame 的 `about:` 文件不經過 Browser，那段期間節點上是之前的文件的值。理由：照 HTML 繼承 origin，`file:` 頁面裡 `srcdoc` 的連結能開本機的檔案、`http:` 頁面裡的不能，和 Chrome 一樣；每個節點一個 bool。Chromium：`about:blank`、`srcdoc` 繼承 origin，Blink 照發起的文件的 origin 擋（`SecurityOrigin::CanLoadLocalResources`）。第 16 條的同源檢查（2e）沿用同一套繼承，記的東西從 bool 換成 origin。不選只看文件自己的網址（`about:` 的一律不行，比 Chrome 嚴）。
21. **錯誤頁的文件網址是 `chrome-error://chromewebdata/`**（2026-11-18，2c 開始前；第 13 條的做法）：`CommitNavigation` 帶「這是錯誤頁」的旗標，renderer 用這個網址建錯誤頁的文件（`url` 把不認得的 scheme 當成 opaque 的 origin，DOM 和 engine 不用改），回報給 Browser 的照舊是失敗的網址（網址列、歷史、`DidCommitNavigation`）。錯誤頁裡的 `location.href`、`document.URL` 和 DevTools 的 `frameNavigated` 是這個網址。Chromium（2026-11-18 查過）：錯誤頁的文件網址是 `chrome-error://chromewebdata`，失敗的網址是「unreachable URL」，網址列和 session history 用後者。Firefox（`nsDocShell::LoadErrorPage`）另外載入 `about:neterror?e=…&u=…`，`documentURI` 是它，`location.href` 是失敗的網址。HTML 給錯誤的文件一個新的 opaque origin（「loading a document for inline content that doesn't have a DOM」）。不選 DOM 的文件多一個「origin 是 opaque」的旗標：`dom`、`ScriptEngine`、兩個 engine 都要改，每個從網址算 origin 的地方都要先看它，漏一處同源的父 frame 就讀得到錯誤頁。
22. **`srcdoc` 的值照 Chromium 告訴 Browser：子 frame 建好時，和屬性每次改變、拿掉時**（2026-11-18，2d 開始前）：renderer 送 `DidChangeSrcDoc { frame, srcdoc }`（沒有屬性是空字串），Browser 存在節點上，commit `about:srcdoc` 時帶給 renderer；不是發訊息的 renderer 的分頁裡的 frame 是壞掉的訊息。做的時候（2026-11-18）改成 renderer 每次照 iframe 的屬性導覽之前送（建好時、`srcdoc` 或 `src` 改了時；`srcdoc` 改了、拿掉時一定導覽），Browser 拿到的值和 Blink 送的一樣，`dom_api` 不用另外通知。理由：節點上的值和屬性一致，第 3 階段 Browser 自己發起的 `srcdoc` 導覽（上一頁、重新整理）拿得到現在的內容。Chromium（2026-11-18 查過）：`HTMLFrameOwnerElement::LoadOrRedirectSubframe` 建好 frame 之後的 `DidChangeAttributes`、`HTMLFrameElementBase::ParseAttribute`（屬性改了、frame 在時）送 `DidChangeSrcDoc`，`RenderFrameHostImpl::DidChangeSrcDoc` 只存到 `FrameTreeNode` 上，`NavigationRequest` 在 commit 時放進 `CommitNavigationParams::srcdoc_value`。不選只在請求導覽到 `about:srcdoc` 之前送：屬性拿掉之後節點上留著舊的值。
23. **導覽到 `about:srcdoc` 照 Chromium 的四條規則檢查，節點從 2d 起記 origin**（2026-11-18，2d 開始前）：每個節點記目前文件的 origin（browser 載入的照網址，錯誤頁 opaque，`about:srcdoc` 是父 frame 的，`about:blank` 是發起的 frame 的），「能不能開本機檔案」照舊另外記（`url` 的 `file:` origin 是 opaque 的，看不出來）。子 frame 導覽到 `about:srcdoc`，只有 frame 目前是 `about:srcdoc` 而且發起的 frame 和它同源（重新載入自己），或發起的 frame 和父 frame 同源時放行；擋下的照 Chromium commit 錯誤頁（`ERR_INVALID_URL`），main frame 照舊不收。理由：安全的判斷和 Chromium 一致，不靠近似；第 16 條的同源檢查（2e）本來就要 origin，第 20 條的 bool 到時要換，現在做 2e 不用回頭改。Chromium（2026-11-18 查過）：`NavigationRequest::CheckAboutSrcDoc` 擋 main frame，子 frame 放行同一份文件內的、browser 發起的、`about:srcdoc` 的 frame 被同源的發起者重新載入、發起者和父 frame 同源，其他的 `BLOCK_REQUEST`，呼叫的地方用 `ERR_INVALID_URL` 走 `OnRequestFailedInternal`（錯誤頁）。不選用 Browser 已經知道的近似（只放行父 frame 和自己：比 Chromium 嚴，2e 要回頭改）；不選不檢查（內容來自父 frame，跨源的發起者不該能叫它出現）。
24. **導覽到 `about:blank` 的文件照發起的文件繼承 origin，renderer 和 Browser 都是，2d 就做**（2026-11-18，2d 開始前）：renderer 請求導覽時（`BeginNavigation`）記下發起的文件當時的 origin，跟著請求的編號，commit 時交給 engine；Browser 自己發起的（之後的上一頁、重新整理）是新的 opaque origin。DOM 的文件建立時就決定 origin、存下來（繼承來的，或照網址，`about:` 的照舊往上找到建立它的文件），`document_origin` 和 dom_js 的 security token 都從它算，不再各自從網址算（opaque 的 origin 每個一個 token）。理由：HTML 和 Chromium 都照發起者（initiator origin）；Browser 照第 20、23 條已經這樣記，renderer 不跟上的話兩邊對同一份文件的 origin 判斷不一樣（跨源的子 frame 把自己導覽到 `about:blank` 時，父 frame 讀得到它）；2d 本來就在改 `about:blank` 的 commit。opaque 的 origin 要和發起者的是同一個才同源，值傳到 Browser 再傳回來保不住，所以在 renderer 記著。origin 只有一個來源，避開第 21 條不選 B 的那個風險（漏掉一處從網址算的地方）。Chromium：`CommitNavigationParams::origin_to_commit`，initiator origin 在導覽開始時記下。不選 2d 先維持、記成限制（不一致留到 merge 回 `main`）；不選 Browser 也照父 frame（兩邊都和規範不同，要改第 20 條）。第 21 條照舊：錯誤頁的 opaque origin 靠它的網址，和 Chrome 看得到的行為一樣。
25. **同一份文件內的導覽，Browser 照 HTML 的「can have its URL rewritten」比對節點上的網址**（2026-11-18，2e 開始前；第 16 條的「同源」怎麼檢查）：新的網址和節點目前的網址的 scheme、帳號、密碼、host、port 要一樣，`http`、`https` 之外的 `file:` 的 path 要一樣，其他 scheme 只能差在 fragment；不符合是壞掉的訊息，main frame 的也是。節點的網址要準：子 frame 一開始的 `about:blank`（沒有網址）當成 `about:blank`；renderer 自己 commit 的 `javascript:` 的 `src` 的空文件，renderer 用 Browser 不會發的導覽編號回報（`DidCommitNavigation`，Browser 當成 opaque 的文件；Chromium 的 renderer 同步 commit、Browser 事後補）。理由：只看 Browser 自己知道的網址，不信 renderer；machopo 所有網站在同一個 renderer、沒有 process lock，這是唯一可靠的檢查。之後做 site isolation 時再加 process lock（`CanCommitURL`）和 origin 不變（下面不選的 B，opaque 的 origin 能帶 nonce 跨 process 之後），三者並存。成本是每次多解析一個網址。Chromium（2026-11-18 查過）：網址的規則由 Blink 在 renderer 判斷（`CanChangeToUrlForHistoryApi`），Browser 檢查回報的 origin 和最後 commit 的一樣（`CanCommitOriginAndUrl` 的 `cross_origin_same_document`）和 process 能不能 commit 網址；被擋下的網址換回目前的（TODO crbug.com/40067230 寫著之後想當成壞掉的訊息）。不選 B（比 renderer 回報的 origin：網址全看 renderer，opaque 的 origin 傳到 Browser 比不出是不是同一個）；不選不符合時只換回目前的網址（A'，和第 7、16 條不一致，之後誤殺時再考慮）；不選不檢查。
26. **子 frame 在歷史裡的鍵照 Chromium 的 unique name 的演算法，在 Browser 算**（2026-11-19，第 3 階段開始前）：renderer 在 `DidCreateChildFrame` 多帶 iframe 的 `name` 和是不是 script 建的，`name` 之後改了另外告訴 Browser；Browser 用自己的樹算：`name` 在整頁的 frame 裡沒有重複就是它，否則是祖先的名稱加位置的路徑（`<!--framePath //<!--frame0-->-->`，還重複就加編號），太長的名稱換成雜湊，script 建的加一段隨機的記號（換了文件就對不上、不還原）；第一次 commit 之後固定。entry 的樹照它對上 frame（main frame 一律對上）。理由：整頁的樹在 Browser，「整頁有沒有重複」查自己的就知道；鍵只在一個地方算，renderer 用不到它（machopo 的歷史都在 Browser），也不能自己指定。第 5 條的「名稱」在這裡送，`window.name` 可以一起補。Chromium（2026-11-19 查過）：Blink 的 `UniqueNameHelper`（`CalculateNameInternal`、`GenerateCandidate`、`AppendUniqueSuffix`、`CalculateFrameHash`；script 建的加 `kDynamicFrameMarker` 和 `UnguessableToken`；`frozen_` 之後不再變），`NavigationEntryImpl::TreeNode::MatchesFrame` 照 unique name 比；Blink 在 renderer 算是因為它的 `HistoryItem` 和 page state 也用。不選在 renderer 算（renderer 要多一份整頁的名稱表，Browser 只能信它給的鍵）；不選只用位置（順序一變就對錯，不看 `name`）。
27. **重新整理時子 frame 照 `src` 載入，只有上一頁、下一頁照 entry 還原**（2026-11-19，第 3 階段開始前）：還原只給父文件 `load` 結束之前建的子 frame（之後 script 加的照 `src`）；重新整理之後子 frame 在 entry 裡的那幾格換成新載入的。理由：和 Chromium、HTML 一致，重新整理就是重新開這個網址；重新整理不用比對 entry 的樹。Chromium（2026-11-19 查過）：`LocalFrameClientImpl` 的 `IsBackForwardNavigationInProgress`（父 frame 的載入是 `kBackForward` 或 `kRestore`、父文件的 `load` 還沒結束）才把新的子 frame 的導覽當成歷史的（`is_history_navigation_in_new_child_frame`，Browser 的 `StartHistoryNavigationInNewSubframe`），重新整理不算。HTML 的 create a new child navigable 用新的 entry，不沿用父文件的 nested history。Firefox 一直以來保留 iframe 導覽過的網址（Mozilla bug 1139737 說是設計如此），bug 2023917 標成在 151 改了（沒有確認看得到的行為）。不選重新整理也照 entry 還原（和 Chromium、規範不同）。
28. **`window.name` 不在第 3 階段做；iframe 的名稱只在建立子 frame 時送**（2026-11-19，3a 開始前）：`DidCreateChildFrame` 多帶 iframe 那時的 `name` 和是不是 script 建的，unique name 之後不變，沒有改名的訊息。理由：unique name 第一次 commit 之後就固定，名稱只在那之前改才有影響；script 建的 iframe 的 unique name 只有隨機的部分；`window.name`（跟著 browsing context 走、跨網站的 main frame 導覽時清掉、連結的 `target` 找 frame 的名稱）和歷史無關，之後當成自己的工作做，到時加 `DidChangeName`。Chromium（2026-11-19 查過）：iframe 的 `name` 屬性只在建立時給 frame（`HTMLFrameElementBase::ParseAttribute` 只記在元素上，HTML 也是），之後只有 `window.name` 改名（`RenderFrameImpl::DidChangeName` 送 `DidChangeName(name, unique_name)`，還沒 commit 過時才重算 unique name，crbug.com/607205）。不選 3a 就做 `window.name`（大部分是和歷史無關的 DOM 功能）。
29. **entry 的樹只記鍵，frame 的那一格由 `NavigationController` 擁有、用 item 的序號找**（2026-11-19，3b 開始前）：controller 有一張 item 序號對 `FrameNavigationEntry`（網址、`history.state`、捲動模式、document 的序號）的表；每個 entry 的樹的節點只有 unique name、item 序號和子節點。子 frame 的導覽複製目前的 entry 的樹、換掉那個 frame 的節點，其他 frame 的格子自然和前一個 entry 共用；改一格（`replaceState`、捲動模式）只改表裡的那一份。丟掉 entry（前進的 entry、`replace`）時掃一次剩下的樹，刪掉沒有節點用的格子。理由：擁有者只有 controller，從它借就好，不用執行時的計數和借用檢查；`state` 可以很大，每個 entry 只存一份；更新只改一處。Chromium（2026-11-19 查過 `navigation_entry_impl.h`）：每個 entry 自己的 `TreeNode` 樹（不共用），節點用 `scoped_refptr<FrameNavigationEntry>` 指到格子，`Clone`、`CloneAndReplace` 共用沒換的格子，`AddOrUpdateFrameEntry` 改的是共用的那一份（「might be shared with other NavigationEntries」）。不選每個 entry 擁有整棵樹的複本（`state` 照 entry 的數量重複存，改一格要掃過所有 entry 找同一個 item）；不選 `Rc<RefCell<…>>` 共用格子（和 Chromium 一對一，但擁有者明確，用不到共享的擁有權，每次讀寫多一次借用檢查）。
30. **`history.state` 和捲動模式照舊是格子裡分開的兩個欄位**（2026-11-19，3b 開始前）：現在 `NavigationEntry` 的 `state`、`manual_scroll_restoration` 搬到 frame 的那一格；標題和「是不是分頁的第一個文件」留在 entry 上（main frame 的）。`UpdateState` 多帶 frame，renderer 每個 frame 的都送；`CommitNavigation` 帶的 `state` 和捲動模式是那個 frame 自己那一格的；`history.length` 和位置照舊整頁一份（`SetHistoryOffsetAndLength`）。理由：Browser 看得懂、測試看得到，改動最小；之後要記捲動位置、表單內容時再加欄位。Chromium（2026-11-19 查過 `frame_navigation_entry.h`、`page.mojom`）：`FrameNavigationEntry` 有 `blink::PageState`（renderer 編碼、Browser 不解讀，欄位上有「換成 `FrameState`」的 TODO），沒有標題；`NavigationEntry` 有標題；`PageBroadcast::SetHistoryIndexAndLength` 整頁一份。不選照 `PageState` 放一包 renderer 編碼的資料（多一份編碼格式，Browser 的測試看不到裡面，`state` 本來就是不透明的 bytes）。
31. **initial about:blank 上同一份文件內的導覽改成取代，由 renderer 照 HTML 判斷**（2026-11-19，3c 開始前）：DOM 的文件有 HTML 的「is initial about:blank」（子 frame 建立時的那份文件），`pushState` 和到 fragment 的導覽碰到它就是取代（HTML 的 URL and history update steps 第 4 步）；Browser 照 renderer 回報的 `replace` 記，那個 frame 在目前的 entry 裡還沒有位置時放進去。理由：只有 renderer 分得出「initial」的 about:blank 和之後導覽過去的；renderer 自己的 `history.length` 和 Browser 一致；頁面本來就能自己 `pushState`、`replaceState`，照收它的旗標沒有多給它能力。Chromium（2026-11-19 查過）：Blink 的 `DocumentLoader::UpdateForSameDocumentNavigation`（`IsOnInitialEmptyDocument` 時把 `kStandard` 換成 `kReplaceCurrentItem`，註解引用同一步），Browser 照 renderer 的 `did_create_new_entry` 分類；跨文件的導覽 Browser 另外檢查（`ShouldReplaceCurrentEntryForNavigationFromInitialEmptyDocumentOrEntry`，3b 照做）。不選 Browser 判斷（renderer 已經把 `history.length` 加一，兩邊不一致；`has_committed_real_load` 把導覽過去的 about:blank 也當成 initial）；不選兩邊都判斷（規則不完全一樣，不一致時 `history.length` 錯，Chromium 也沒有）。
32. **新的子 frame 要照 entry 還原，由 renderer 標記、Browser 查 entry**（2026-11-19，3d 開始前；第 27 條的做法）：renderer 記得每份文件是不是上一頁、下一頁載入的，它的 `load` 結束之前，新的 iframe 照屬性的第一次導覽在 `BeginNavigation` 帶標記；Browser 的節點記得它的文件在哪個 entry 裡 commit（entry 有 id，Chromium 的 `nav_entry_id`），收到標記時在父 frame 的那個 entry 裡照 unique name 找這個 frame 的項目，導覽到它的網址、帶它的 state，沒有項目就照 `src`。上一頁、下一頁比對 frame 照 Chromium 的 `FindFramesToNavigate`、`DetermineActionForHistoryNavigation`：目標 entry 沒有那個 frame 的位置就不動它、也不往下看；item 序號一樣就往下看子 frame；document 序號一樣是同一份文件內的 traversal；不一樣就重新載入那個 frame、不往下看；「現在的項目」是 frame 自己最後 commit 的（`last_committed_frame_entry`），沒有時用目前的 entry 的。理由：`load` 結束了沒只有 renderer 知道得準；標記最多讓那個 frame 回到它在歷史裡待過的網址。Chromium（2026-11-19 查過 `navigation_controller_impl.cc`）：`StartHistoryNavigationInNewSubframe` 用父 frame 的 `nav_entry_id` 找 entry、`GetFrameEntry` 找項目，找不到時照一般的導覽。不選 Browser 判斷（renderer 要多告訴每個 frame 的 `load` 結束，訊息到之前 script 加的 iframe 會被誤當成要還原）；不選父 frame commit 時把子 frame 的網址帶給 renderer（unique name 在 Browser 算，renderer 對不上）。

## 待決定

接受這份 RFC 前要決定的都定了。各階段開始前要決定的事列在「分階段」，到時候才問、寫進「已決定的事項」。

## 之後的可能

- 掛在 `NavigationRequest` 和 `FrameTreeNode` 上的功能：`sandbox`、`X-Frame-Options`、CSP 的 `frame-ancestors`、permissions policy、cookie 的網站、`Referer`、fetch metadata。
- 跨 process 的 frame（RFC 0004 的「之後的可能」）和跨語言的 frame（RFC 0009）：節點上多一個「這個 frame 在哪個 renderer」，其他 renderer 裡是遠端 frame 的佔位。
- `Page.getFrameTree` 和 frame 的事件由 Browser 回答；`Page.navigate` 的 `frameId`。
- `window.open`、opener、具名的 target 找到別的分頁。
- back-forward cache、還原捲動位置、`beforeunload`、表單的導覽。
