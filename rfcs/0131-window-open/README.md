# RFC 0131：`window.open` 和 `opener`

- 開始日期：2026-11-28
- 狀態：✅ 已接受：2026-11-28
- 細節：`details.md`

RFC 0122 路線圖的 W（M3）。

## 摘要

machopo 沒有 `window.open`：頁面只能靠連結的 `target=_blank` 開新分頁，開出來的分頁和開它的頁面互相碰不到（`opener` 一律是 `null`）。這份做 `window.open`、`opener`、`window.name` 和具名的 browsing context、`noopener`、`window.close()`。同一個網站的彈出視窗和開它的頁面要能同步碰到對方，所以一定在同一個 renderer：這是 machopo 第一次要讓一個 renderer 裝好幾個分頁，也是這份最大的一塊。跨網站的彈出視窗在別的 renderer、沒有 `opener`（遠端的 window 要等 OOPIF）。一份 RFC 分四段：先鋪路（Browser 的資料模型、一個 renderer 裝好幾個分頁），再做 API，最後 COOP（已決定的事項第 1 條）。

## 動機

- **網站用它**：登入的彈出視窗（OAuth）、付款、分享、「在新視窗開啟」的按鈕。現在 `window.open` 不是函式，呼叫它的 script 直接丟 `TypeError`。
- **WPT**：iframe 的 `sandbox` 的測試和 Chrome 不同的 49 個檔案裡，約 30 個只是因為沒有 `window.open`；`html/browsers/the-window-object/`、`windows/` 的大部分也是；wdspec 的 `window.open`、`window.close`。
- **路線圖留到這時的事**（RFC 0124 的已決定的事項第 10、11、17 條）：`BrowsingInstance` 和 `SiteInstance` 的資料模型；同一個 site 的一般分頁要不要共用 process；renderer 是 thread 時跨網站的導覽怎麼換。

**目標**

- `window.open(url, target, features)`、`opener`、`window.name`、具名的查找、`noopener`、`noreferrer`、`window.close()`、`closed`，照 HTML 和 Chromium。
- 同一個網站的彈出視窗和開它的頁面在同一個 renderer，互相同步存取；跨網站的沒有 `opener`。
- Browser 有 `BrowsingInstance`、`SiteInstance`；一個 renderer 能裝好幾個分頁，它的生命不再跟著一個分頁。
- 沒有使用者的啟動時不開（popup blocker），開了用掉啟動。

**不在範圍內**

- 跨網站的 `opener`、跨 renderer 的 `postMessage`（遠端的 window：路線圖的 O4）。
- 新的作業系統視窗：彈出視窗還是新分頁，`features` 的大小和位置不算（只看 `noopener`、`noreferrer`、`popup` 這些會改變行為的）。
- 跨 renderer 的 `MessagePort`（已決定的事項第 2 條：移到 O4）。
- `BroadcastChannel`、`SharedWorker`。
- cross-origin isolated（路線圖的 X；它要這份的 COOP）。

要解決的 `docs/limitations.md` 的限制：「連結開新分頁」的「沒有 `window.open`……每次都開新的、不重用……不看有沒有 user activation」；「跨源的 window」的「`close()` 什麼都不做，`opener` 一律是 `null`」；user activation 的「沒有『用掉』activation」；`postMessage` 的「只能給同一個分頁裡的 window」；`sessionStorage` 的「新分頁不複製」；iframe 的「`name`」那一條裡 `window.name` 的部分；WebDriver 的 `window.open`、`window.close`。

## 設計概要

給使用網頁的人：

- `const w = window.open('/login', 'login')` 開一個新分頁，回傳它的 window；同一個網站的可以讀寫 `w.document`、`w.location`，新分頁裡 `window.opener` 是開它的 window，兩邊可以 `postMessage`。
- 再呼叫一次 `window.open(url, 'login')` 導覽同一個分頁，不開新的；`<a target=login>` 也是。
- `window.open(url, '_blank', 'noopener')` 和 `rel=noopener` 的連結：新分頁沒有 `opener`，回傳 `null`。
- 跨網站的 `window.open`：分頁照樣開，但像 `noopener`：回傳 `null`、沒有 `opener`（和 Chrome 不同，限制）。
- script 開的分頁可以 `window.close()` 關掉自己。
- 沒有使用者的點擊就開的被擋下。

給改 machopo 的人：

- Browser：`RenderProcessHost` 不再由一個 `WebContents` 擁有：放在一張表裡（`RenderProcessHosts`），分頁拿 `Rc<SiteInstance>` 當「還在用」的憑證，最後一個放掉時 renderer 結束（已決定的事項第 3 條）；renderer 的訊息照 process 和 frame 的 token 找到分頁，不靠啟動時綁死的分頁編號。
- renderer：現在 `RenderThread` 上「一個分頁」的狀態（view、`LayerTreeHost`、frame sink、input、DevTools 的 agent）收成每個分頁一份；engine 的 frame 表能有好幾個最上層的 frame，共用一個 V8 isolate 和一個 DOM 的 heap（同步存取要的）。

## 詳細設計

盤點（現在哪裡假設一個 renderer 一個分頁）和 Chromium 的做法在 `details.md`。下面是大綱，每一塊的設計在那一段開始前定。

### 資料模型（Browser）

Chromium 的 `BrowsingInstance`（一組互相碰得到的 browsing context：一個分頁和它開的彈出視窗）和 `SiteInstance`（`BrowsingInstance` 裡一個網站的文件，對到一個 process）。同一個 `BrowsingInstance`、同一個網站的文件一定在同一個 process；跨網站的導覽、COOP 換 `BrowsingInstance`，換了就斷開 `opener`。

誰擁有什麼（已決定的事項第 3 條）：

- **`RenderProcessHosts`**：一張表，擁有每個 `RenderProcessHost`，照 renderer 的編號查（Chromium 的 `GetAllHosts()`）。放在 `BrowserView`（它擁有所有的 `WebContents`：分頁和 DevTools 的 web view）；測試和 headless 各自建一張。表也記「同一個 `BrowsingInstance` 的這個網站是哪個 `SiteInstance`」（`(BrowsingInstance, 網站) → Weak<SiteInstance>`，網站在第一次 commit 定了時登記）。
- **`SiteInstance`**：`Rc`，只放建立時就定、之後不變的：屬於哪個 `BrowsingInstance`、renderer 的編號。分頁拿著它，表只拿 `Weak`：最後一個分頁放掉時，表就知道那個 renderer 可以結束（不用手動加減計數；Chromium 的 `SiteInstance` 是 `base::RefCounted`，不是 atomic 的）。
- **process lock 留在 `RenderProcessHost` 上**（和現在一樣；Chromium 的 `SiteInstance` 也是建好之後才 `SetSite`）。
- **`WebContents`**：自己留一份 sender 的把手（`web_contents.rs` 用到 host 的 47 處裡 31 處只要它）；真的要碰 host 的（結束、換 renderer、讀 pid 和怎麼結束的、鎖）由呼叫的人把表（`&mut`）傳進來：69 個公開方法裡 22 個多一個參數。整個設計沒有內部可變性。
- **一個分頁的壞訊息、當掉結束的是整個 renderer**，用它的每個分頁都顯示 sad tab（Chromium 的 `ShutdownForBadMessage` 結束 process、`RenderProcessExited` 通知每個 frame）。現在的 `kill` 會關掉轉送（連 `RendererGone` 都不送，呼叫的人就是唯一的分頁）：共用之後由表記下「這個 renderer 不在了」，`BrowserView` 處理完之後問表、告訴用它的每個分頁（像 `take_opened()`）；細節和訊息怎麼找到分頁一起定。

### 一個 renderer 裝好幾個分頁

現在的假設（`details.md` 的「盤點（2026-11-28）」）：`WebContents` 擁有它的 `RenderProcessHost`，啟動時綁死分頁的編號和一個 frame sink；頻道上的訊息很多不帶 frame（resize、焦點、input、DevTools、標題）；renderer 這一側 engine 的 frame 表只有一個 `TOP`，commit 最上層的文件會清掉其他所有 frame；`RenderThread` 只有一個 widget、一個 `LayerTreeHost`、一組 DevTools 的 agent。

### `window.open` 和它周圍的 API

`window.open`、`opener`（可以設成 `null`）、`window.name`、具名的查找（現在只找 iframe 的 `name`，照 HTML 的「choose a navigable」也找同一個 browsing context group 的分頁）、`noopener` 和 `noreferrer`、`close()` 和 `closed`、初始的 `about:blank`（`window.open` 回傳時就要碰得到）、啟動的用掉和 popup blocker、sandbox 的旗標（`allow-popups`，RFC 0130 鋪好的路）、`sessionStorage` 的複製。

### COOP

`Cross-Origin-Opener-Policy`：回應要求時換 `BrowsingInstance`、斷開 `opener`。

### 測試

每一段定的時候寫。原則：看得到的行為先找 WPT（`html/browsers/the-window-object/`、`windows/`、sandbox 那 30 個、`html/cross-origin-opener-policy/`），逐項和 Chrome 比；Browser 的資料模型和 process 的生命用 `browser` 的整合測試；每一條規則 mutation test。

### 分階段

四段（已決定的事項第 1 條），每一段做完都可以 merge 回 `main`：

1. **Browser 的資料模型和共用的 renderer**：`BrowsingInstance`、`SiteInstance`；`RenderProcessHost` 不由一個 `WebContents` 擁有、照計數活著；renderer 的訊息照 process 和 frame 找到分頁；壞訊息和當掉對同一個 renderer 的每個分頁。這一段做完時還沒有分頁真的共用。
2. **renderer 裝好幾個分頁**：每個分頁一份 view、compositor、frame sink、input、DevTools；engine 有好幾個最上層的 frame；不帶 frame 的訊息帶上它。用連結開的同網站新分頁（有 `opener` 關係的那種）先共用 renderer 來驗證。
3. **`window.open` 和它周圍的 API**：`window.open`、`opener`、`window.name`、具名的查找、`noopener`、`noreferrer`、`close()`、`closed`、初始的 `about:blank`、popup blocker 和用掉啟動、`sessionStorage` 的複製；WPT 逐項和 Chrome 比。
4. **COOP 和收尾**：`Cross-Origin-Opener-Policy`；重新考慮 RFC 0124 的已決定的事項第 10、11 條（同一個 site 的一般分頁共不共用、renderer 是 thread 時怎麼換）；收尾。

## 缺點

- **renderer 的生命變複雜**：現在一個分頁一個 renderer，關分頁就結束 renderer；共用之後要計數，一個分頁的壞訊息、當掉會帶走同一個 renderer 的其他分頁（Chromium 也是）。
- **動到最核心的擁有關係**（`WebContents`、`RenderProcessHost`、`RenderThread`）：回歸的風險比路線圖前面幾份都大。
- **跨網站的彈出視窗沒有 `opener`**，要到 OOPIF：跨網站的 OAuth 彈出視窗（最常見的用途之一）在那之前還是不能用 `postMessage` 回傳結果。

## 理由和其他做法

各題的選項和不選的理由在「已決定的事項」。不做的話：呼叫 `window.open` 的頁面直接壞掉。

## 先例

`details.md` 的「Chromium」。

## 已決定的事項

1. **一份 RFC，先鋪路、再做 API，分四段（P1）**（2026-11-28，使用者決定）：照「分階段」。理由：API 一出現就是對的（同網站的彈出視窗有 `opener`，和 Chrome 一樣）；前兩段各自有測試守著，第 2 段用連結開的同網站新分頁先共用 renderer 來驗證。代價：前兩段使用者看不到新功能。不選先做 API、一律像 `noopener`（P2：`main` 上有一段時間 `window.open` 回傳 `null`，接著用回傳值的 script 照樣丟例外，Chrome 只在 `noopener` 時回傳 `null`；很多 WPT 的預期結果要寫兩次）。不選拆成兩份 RFC（P3：前一份沒有網頁看得到的功能；renderer 怎麼裝分頁要看 `window.open` 怎麼建初始的 `about:blank`，分開寫容易做錯方向）。
2. **跨 renderer 的 `MessagePort` 移到 O4**（2026-11-28，使用者決定）：路線圖把它列在 W，但能把 port 帶到別的 renderer 的只有 `postMessage`，跨 renderer 的 window 要遠端的 window（O4）才碰得到，machopo 也沒有 `BroadcastChannel`、`SharedWorker`：這時做了測不到。`docs/limitations.md` 的那一條留著，開 O4 的 RFC 時列進它的範圍。

3. **表擁有 `RenderProcessHost`，分頁拿 `Rc<SiteInstance>` 當憑證，沒有內部可變性（C2）**（2026-11-28，使用者決定）：照「資料模型（Browser）」的「誰擁有什麼」。理由：共用之後「這個 renderer 不在了」要通知用它的每個分頁，本來就要一個知道「哪個 renderer 有誰在用」的地方，讓它順便擁有 host；誰能改 host 由編譯器檢查；host 不用拆（`unload(self)` 照舊把它從表裡拿出來、搬到 timer 的 thread）；process 那一層和 Chromium 一樣是一張表，資料模型那一層和 Chromium 一樣是計數的物件；OOPIF 時一個分頁用好幾個 process，照編號查表的寫法不用再改。代價：22 個公開方法多一個參數，`browser` 的測試約 625 處、`shell` 約 20 處機械性的改動；沒呼叫 `shutdown` 就丟掉的分頁，它的 renderer 多活到表下一次被碰到或被丟掉。不選 `SiteInstance` 擁有 host、可變的部分放 `RefCell`（A：簽名不變，但 host 要拆成共用的把手和結束用的那一包，重入只能靠論證，錯了是 Browser 的 process 裡的 panic，Chromium 的 `Cleanup()` 在同一個地方出過重入的事；而且沒有通知其他分頁的地方，還是得另外建表）。不選分頁直接共用 `Rc<RenderProcessHost>`（B：同樣的代價，資料模型和擁有關係還可能對不上）。不選全部用編號、手動登記（C1：漏了取消登記就是洩漏或太早結束，`WebContents` 的 `Drop` 拿不到表、補不了）。

## 待決定

各段開始前決定（先列著）：

- 第 1 段：訊息怎麼找到分頁、renderer 不在了怎麼告訴用它的每個分頁；history 的項目要不要記 `SiteInstance`。
- 第 2 段：renderer 裡每個分頁的狀態怎麼收（Blink 的 `Page`、`WebViewImpl`）；Compositor thread 一個分頁一條還是共用。
- 第 3 段：初始的 `about:blank` 由誰建（Chromium：renderer 先建、同步回傳，Browser 再認）；跨網站的 `window.open` 回傳什麼；popup blocker 擋下時怎麼告訴使用者。
- 第 4 段：COOP 做到哪些值；同一個 site 的一般分頁要不要共用 process；renderer 是 thread 時跨網站的導覽怎麼換。

## 之後的可能

- 跨網站的 `opener` 和 `postMessage`（O4）。
- 彈出視窗是新的作業系統視窗、照 `features` 的大小。
- cross-origin isolated（X）。
