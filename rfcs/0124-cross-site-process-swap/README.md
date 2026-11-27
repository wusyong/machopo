# RFC 0124：跨網站的導覽換 renderer

- 開始日期：2026-11-27
- 狀態：🚧 已排程：2026-11-27
- 進度和驗收：`progress.md`
- 細節：`details.md`

RFC 0122 路線圖的 C（M2 的第一份）。

## 摘要

一個分頁現在從頭到尾用同一個 renderer process。這份讓 main frame 導覽到別的網站時，新的文件在一個新的 renderer process 裡 commit，舊的 process 跑完舊文件的 `unload` 就收掉：一個 renderer process 的一生裡只裝過一個最上層的網站。這份只做「換」，「Browser 照 process 的網站檢查它要的資料」是下一份（路線圖的 B）。

## 動機

- **現在就有的洞**（RFC 0122 的已決定的事項第 6 條）：攻破 renderer 的網頁 A，等這個分頁導覽到網站 B（使用者點的，或 A 自己設 `location`）。B 是最上層的網站，cookie 照送，它的資料就在被攻破的那個 process 裡。第三方 cookie 的限制擋不住這個，因為 B 這時是第一方。
- **鎖需要它**：process lock 是 process 一輩子不變的，一個先後裝過好幾個網站的 process 鎖不了（RFC 0122 的「順序」）。
- **附帶的**：renderer 裡只增不減的表（名稱表）、圖片的快取、V8 的 heap，跟著舊的 process 一起還給系統。

**目標**

- main frame 的跨網站導覽在新的 renderer process 裡 commit；規則寫成一個函式，測試照表對。
- 舊的文件照樣收到 `pagehide`、`unload`；舊的 process 不留下來。
- 使用者和工具看不出換了：session history、`sessionStorage`、大小和 focus、DevTools 和 WebDriver 的 session 都接得上。

**不在範圍內**

- **process lock 和 Browser 的檢查**（路線圖的 B）：Browser 現在的檢查是照文件的（loader 的 origin、frame 樹上的 origin），不是照 process 的；這份不改它們，哪些還漏、怎麼照 process 鎖是 B 的事。這份改的是「這個 process 的記憶體裡有誰的東西」。
- **跨網站的 iframe**：照舊和頁面在同一個 process（OOPIF，路線圖的 M4）。
- `window.open`、`opener`、COOP（路線圖的 W）；back-forward cache；`beforeunload`（已決定的事項第 8 條）。

要解決的 `docs/limitations.md` 的限制（做完時照這份清單改）：

- 「隔離」的「名稱表和屬性值的表在同一個 renderer 裡共用」：同一個分頁先後導覽過的**不同網站**不再共用（各自的 process）；同一個網站的網頁之間、頁面和它跨網站的 iframe 之間照舊。
- 「GC 和 `dom`」的「名稱表只增不減……同一個分頁導覽到別的網頁也不清」：跨網站的導覽之後跟著舊的 process 還給系統，同一個網站裡的照舊。

## 設計概要

使用者在 `https://a.example/` 點了一個到 `https://b.example/` 的連結：

1. Browser 照舊自己載入 `b.example` 的文件（跟著 redirect）。
2. response 到了、要 commit 時，Browser 看最後的網址：它的 site 和這個分頁的 renderer 現在的 site 不同，就啟動一個新的 renderer process，把 commit 送給它。
3. 舊的 renderer 收到「你的文件被換掉了」：分派 `pagehide`、`unload`，回報做完；Browser 收掉它。它不回報的話，等一段時間之後結束它。
4. 分頁的 frame sink 換成新的 renderer 的；大小、可見、focus 再送一次；DevTools 的 session 接到新的 renderer 上。

同一個 site 裡的導覽（`a.example/x` 到 `a.example/y`、`www.a.example` 到 `shop.a.example`）和現在一樣，不換。新分頁的第一次導覽也不換：分頁一開始的 renderer 還沒裝過任何網站，直接用它。

寫 machopo 的人：`WebContents` 的 `process` 不再是「這個分頁的 renderer」，而是「現在這個」；跟著 renderer 的狀態（frame sink、loader、DevTools 的 agent）在換的時候重建，要跟著分頁的東西放在 `WebContents` 或 Browser 的其他地方。

## 詳細設計

現況的盤點、Chromium 的做法和量到的數字在 `details.md`。這一節先寫已經看得出來的骨架，各項的做法在「待決定」定了之後補上。

### 現在最接近的路：`relaunch`

renderer 當掉之後的下一次導覽，`WebContents::relaunch` 已經會「換一個 renderer」：新的 frame sink、啟動新的 `RenderProcessHost`、放掉舊的、把分頁的狀態（DevTools 的 token、大小、螢幕上的位置、可見、focus）再送一次。跨網站的換從它推廣，多出來的是：

- **決定的時機**：`relaunch` 在導覽開始時；跨網站的要看到最後的網址（redirect 之後）才知道。現在 `Committer`（commit 送給誰）在導覽開始時就向當時的 process 拿了，要改成 commit 時才定。
- **舊的 renderer 還活著**：要叫它跑 `unload`、等它、逾時結束它；它在這段時間送來的訊息不能被當成新的 renderer 的（`RendererId` 和 `is_current_renderer` 已經在擋）。
- **舊的畫面**：`relaunch` 直接作廢舊的 sink（那時本來就是 sad tab）；跨網站的換要決定中間顯示什麼。
- **DevTools 的 session**：現在靠 `is_crashed` 從 `true` 變回 `false` 才重新接上，只重送 `Page.enable` 和 lifecycle；換 process 沒有「當掉」這一步，client 也不知道換了。

### 規則

寫成一個不碰 process 的函式：（這個 renderer 現在的 site、這次導覽的種類、最後的網址、發起者）→ 留著或換。待決定第 1 項。

### 測試

- 規則的函式：照表的單元測試（每一種網址、每一種導覽）。
- `browser` 的整合測試（`Detached` 的 renderer，直接餵訊息）：跨網站的 commit 送給新的 renderer、舊的收到 unload 的訊息、逾時、舊的 renderer 之後送的訊息不算。
- `shell` 的整合測試（真的 renderer，用 `--host-resolver-rules` 做兩個網站）：`pagehide`、`unload` 有跑（handler 裡寫 `localStorage`，換到的頁面回到同一個網站時讀得到）、上一頁回到前一個網站、`sessionStorage` 留著、DevTools 的 session 接得上。
- 情境：使用者看得到的（點連結到別的網站、上一頁、畫面中間不閃）。
- WPT：跨 origin 導覽的那些目錄經過 machopodriver 跑的是真的 process，換壞了它會失敗。
- 量測工具的測試（`scripts/bench`）：它們照 process 找 renderer。

### 分階段

四段（已決定的事項第 12 條）：

1. **鋪路，行為不變**：commit 時才定送給哪個 renderer；「分頁換了 renderer」變成明確的一件事，DevTools 的 session 重新接上、`headless` 跟著換 sink 都聽它，不再靠 `crashed` 變回來。用現有的「當掉之後重新載入」驗證。
2. **DevTools 的 session 的狀態跨過換 renderer**：同樣先用當掉之後重新載入驗證。
3. **規則和換**：跨網站的 commit 在新的 process；舊文件的 `unload`；中間的畫面。
4. **收尾**：量測工具、WPT 的預期結果、文件。

每一段做完 `main` 上的行為都是一致的：前兩段不改變什麼時候換，第三段打開時工具已經接得上。第 4 段另外有一件要在別的機器上做的：在 macOS 和 Windows 量 renderer 啟動的時間（第 5 條）。

## 缺點

- 每次跨網站的導覽多啟動一個 process：這台 Linux 上量到約 11 ms（`details.md`），其他平台沒量。
- 換的那一小段時間有兩個 renderer process。
- 跟著 renderer 的狀態在跨網站的導覽時沒了（renderer 裡的圖片快取）：回到前一個網站時要重新解碼。同一個網站裡的導覽不受影響。
- `browser` 多一種狀態（舊的 renderer 在跑 `unload`）要維護。

## 理由和其他做法

- **不做**：上面「動機」的洞留著，process lock 做不了。
- **每次導覽都換**（不看 site）：規則最簡單，但同一個網站裡每點一次都多 11 ms、丟掉快取，沒有換到安全上的好處（同一個 site 本來就互相讀得到）。
- **只換 Browser 發起的**（網址列、書籤；Chromium 沒有 site isolation 時的做法）：擋不住頁面自己設 `location` 把分頁導覽走的情況，正是動機裡的那個洞。

其餘的選擇在「待決定」。

## 先例

- **Chromium**（`details.md` 的「Chromium」）：照 `SiteInstance` 換；開始時先猜、response 到了再定；新的文件 commit 之後才叫舊的跑 `unload`，等 500 ms；備一個 spare renderer。machopo 沒有 `window.open`、back-forward cache 和 OOPIF，`BrowsingInstance` 那一層現在用不到。
- **Firefox**（Fission）、**Safari**（WebKit 的 Process Swap On Navigation）：照記憶都在跨網站的導覽換 process，沒有對文件或原始碼。

## 已決定的事項

1. **所有 `file:` 的網址當成同一個 site（F1）**（2026-11-27，使用者決定）：本機檔案之間的導覽不換 process；本機檔案和網頁之間的照規則換，舊的 process 連同「能讀本機檔案」一起收掉。特例寫在決定換不換的規則裡，不改 `site` crate（cookie 和 storage 也用它，那邊 `file:` 是 opaque 的才對）。理由：和 Chromium 一樣（`site_info.cc`）；能讀本機檔案的 process 本來就可以向 Browser 要任何一個本機檔案，把兩個檔案分在兩個 process 沒有擋住誰。不選每個檔案自己一個 site（F2：規則不用特例，但每點一次本機的連結多啟動一個 process、丟掉 renderer 裡的快取，換不到隔離）。
2. **錯誤頁照它要去的網址的 site 決定（E1）**（2026-11-27，使用者決定）：載入失敗時 Browser 產生的錯誤頁，在「成功的話會去的那個 process」裡 commit：跨網站的導覽失敗是一個新的 process，同一個 site 裡的失敗留在原來的。理由：只有一條規則（成功或失敗都看網址），「這個 process 的 site」永遠是網址列顯示的網站，之後的鎖不用為錯誤頁開例外；重新整理成功時不用再換。不選所有錯誤頁共用一個專門的 process（E2，Chromium 對 main frame 的做法：多一種不屬於任何網站的 process，重新整理成功時一定再換一次；它多擋的是「同一個 site 裡的導覽失敗時，跑過那個網站內容的 process 冒充錯誤頁呼叫錯誤頁專用的訊息」，machopo 的錯誤頁現在沒有任何專用的能力）。不選留在目前的 process（E3：網址列顯示 B、內容卻由 A 的 process 畫）。之後給錯誤頁能力時，照 Chrome 綁在「這個 frame 現在顯示的是 Browser commit 的錯誤頁」上、由 Browser 檢查（`details.md` 的「錯誤頁的能力（Chrome）」），E1 不妨礙；能力強到值得時再改成專門的 process（「之後的可能」）。
3. **Browser 自己的頁面：每個內建的 `about:` 頁照名字自己一個 site（D1a）**（2026-11-27，使用者決定）：內建頁和網頁之間、兩個不同的內建頁之間都換 process；同一頁重新整理不換。`about:blank` 不是內建頁：不佔 site、不換（`about:srcdoc` 只有子 frame）。規則裡多一種 site：「內建頁，名字是什麼」（opaque 的 origin 每次都是新的，不另外定的話同一頁重新整理也會被當成換了 site）。理由：和 Chromium 一樣（它把使用者輸入的 `about:<名字>` 改寫成 `chrome://<名字>`，每個 host 一個 site、一定在自己的鎖住的 process，和網頁之間、不同的 WebUI 之間一定換；`details.md` 的「Browser 自己的頁面（`chrome://` 的 WebUI）」）；內建頁之後有能力時（設定、歷史紀錄），Browser 的檢查可以直接照「這個 process 是哪一頁的」，而且內建頁會顯示來自網頁的字串（標題、檔名），一頁出問題不賠上別頁的能力。現在只有 `about:demo`（暫時的），release 會有其他的。不選所有內建頁共用一個 site（D1b：少幾個 process，能力不同的頁在同一個 process；Chromium 只在個別頁面明講時才幾個一組共用，`WebUIConfig` 的 `process_lock_host`，之後要省 process 時照它加）。不選留在目前的 process（D2：內建頁會出現在某個網站的 process 裡）。
4. **其餘的規則照 Chromium**（2026-11-27，使用者決定）：main frame 的導覽，最後的網址的 site 和這個 renderer 現在的 site 不同就換。

   | 導覽 | 換不換 |
   | --- | --- |
   | 跨網站 | 換 |
   | 同一個 site（不同子網域、不同 port） | 不換 |
   | 新分頁的第一次導覽 | 不換：一開始的 renderer 還沒裝過網站，直接用，它從此是那個 site 的 |
   | `about:blank`（頁面發起或網址列輸入） | 不換，也不改變這個 renderer 是哪個 site 的 |
   | 重新整理 | 不換 |
   | 上一頁、下一頁 | 照那個 entry 的網址的 site |
   | redirect | 看最後的網址 |
   | 網址列輸入的 `data:`、`blob:null/…` | 換，每次都是新的（opaque 的 origin 和誰都不同 site） |
   | `blob:` | 照裡面的 origin 的 site |
   | 子 frame | 永遠不換（等 OOPIF） |

   知道的後果：site 包含 scheme，已經在 `http://a.com` 的頁面導覽到 `https://a.com` 會換（輸入 `a.com`、被 redirect 到 https 是看最後的網址，只算一次）；Chromium 也是（`IsSameSite` 先比 scheme）。上一頁、下一頁照網址重新算，不像 Chromium 在 entry 裡存當時的 `SiteInstance`：沒有 `window.open` 時結果一樣，做 `window.open` 那一份時再看要不要存。不選每次導覽都換、只換 Browser 發起的（「理由和其他做法」）。
5. **response 到了才決定換不換，當場啟動新的 process（T1）**（2026-11-27，使用者決定）：redirect 跟完、要 commit 時，照最後的網址和第 1 到 4 條的規則決定；要換就啟動新的 renderer、commit 直接送給它（訊息排在通道裡）。載入期間只有舊的 renderer，舊的頁面照常顯示、照常能用。理由：不會猜錯、沒有要丟掉的 process，Browser 不多一種「猜的那個 renderer」的狀態（被取消、被取代、redirect 到別的網站時怎麼收）；代價是啟動的時間（這台 Linux 約 11 ms）排在 response 之後，不能和網路請求同時。不選開始時先猜、先啟動、response 到了再確認（T2，Chromium 的 speculative `RenderFrameHost`：猜對時省下那 11 ms，但經過轉址的連結、http 轉 https、登入後跳轉都會猜錯，白啟動一個；載入期間多一個 process）；之後可以升級成它，決定的函式是同一個。不選只在開始時決定（T3：redirect 到別的網站時 B 的文件落在照 A 猜的 process 裡）。**要做的**：在 macOS 和 Windows 量一次啟動的時間，明顯比較慢時重新考慮 T2。
6. **送出 commit 的同時換過去、叫舊的跑 `unload`，不等它（U1）**（2026-11-27，使用者決定）：決定要換的那一刻一次做完：啟動新的 renderer、送 commit，分頁從此算它的；同時告訴舊的 renderer 它的文件被換掉了。舊的分派 `pagehide`、`unload`、回報做完，Browser 收掉它；500 ms 沒回報就結束它（Chromium 的 `kUnloadTimeout`）。新的 renderer 啟動失敗（當場知道）時留在舊的頁面。理由：換只發生在一個時間點，是 `relaunch` 的推廣，Browser 不用管「舊的還算目前的、新的正在 commit」的中間狀態；新頁面不等舊頁面的 handler，卡住的舊頁面擋不住導覽。代價：新的 renderer 在 commit 之前當掉時是 sad tab，不是留在舊的頁面。不選新的回報 commit 之後才換（U2，Chromium 的順序：舊的頁面多留幾十毫秒、新的當掉時留在舊的，但 Browser 要同時認兩個 renderer；Chromium 提前啟動新的 process，並存的是整段載入，第 5 條之後這裡只有幾十毫秒）。不選等舊的跑完才 commit（U3：舊頁面的 handler 拖住導覽）、不跑直接結束（U4：靠 `pagehide` 存狀態的頁面壞掉）。知道的後果：舊頁面在 `pagehide`、`unload` 裡送出的網路請求在舊的 process 收掉時取消（沒有 `sendBeacon`，`keepalive` 沒有作用）；寫 `localStorage` 不受影響（在回報做完之前就到 Browser）。記進 `docs/limitations.md`。
7. **換的中間不特別處理畫面（P0）**（2026-11-27，使用者決定）：換的那一刻分頁的 surface 是新的 sink，它還沒有 frame 時 Viz 畫白色的底，到新的 renderer 畫出第一個 frame（`relaunch` 現在的做法）。理由：machopo 的 renderer 沒有 paint holding（找不到「新文件有內容之前不交 frame」的機制；沒有錄畫面確認），同一個 renderer 裡的導覽 commit 之後本來就先是白的，換 process 只多了啟動的時間（約 11 ms，不到一個 frame）；不用改 Viz；網址列顯示 B 時畫面上不留 A 的內容。不選留著舊的畫面到新的 renderer 的第一個 frame（P1：Viz 要有墊著的 surface、要通知 Browser 第一個 frame 到了、舊的 process 結束之後它的 GPU 資源還要留著，而第一個 frame 多半是白的，只把白延後不到一個 frame）。真正消掉白的是 paint holding（P2），對同一個網站裡的導覽一樣有用，是另一份工作（「之後的可能」）。Chromium 兩者都有，但 main frame 只在新舊同一個 origin、或使用者操作過舊頁面時才留舊的畫面，最多 4 秒。
8. **`beforeunload` 不在這份，另開一份 RFC 和對話框一起做，排在路線圖的 C、B 之後（B0、X1）**（2026-11-27，使用者決定）：換 process 不需要它：它接在導覽開始之前（Browser 發起的先問目前的 renderer，頁面發起的 renderer 自己先跑），那時舊的 renderer 還是目前的（第 5 條）。它要的是 machopo 現在沒有的：Browser 的對話框、renderer 停下來等使用者的回答、WebDriver 處理對話框的命令、關分頁和重新整理也要問；前三樣和 `alert`、`confirm`、`prompt` 是同一套，一起做只蓋一次。不選當這份的一個階段（X2：這份變大、process lock 被擋住，對話框只為它蓋一半）、現在就做排在 B 之前（X3：它是相容性的功能，B 關的是安全的洞）、只分派事件永遠不問（頁面以為攔得住使用者）。
9. **不預先備一個 renderer（R0）**（2026-11-27，使用者決定）：啟動一個約 11 ms（這台 Linux），不到一個 frame；不多養一個平常沒在用的 process（一個 V8 的 isolate，有 GPU 時加 4 MiB 的共用記憶體；佔多少沒有量），不多一組「什麼時候備、記憶體吃緊時不備、被拿走之後何時補」的規則。不選一直備一個（R1，Chromium 桌面版的預設 `kSpareRendererForSitePerProcess`：它的 renderer 啟動慢得多）。和第 5 條綁在一起：macOS 和 Windows 量到啟動明顯比較慢時，提前啟動和備一個一起重新考慮。
10. **同一個 site 的分頁不共用 process（S0）**（2026-11-27，使用者決定）：每個分頁自己的 renderer，規則只看這個分頁現在的 renderer 是哪個 site 的。理由：一個分頁當掉、卡住或吃光記憶體不影響同網站的另一個；process 的生命跟著一個分頁，不用計數；安全上共不共用沒有差別（同一個 site 本來就互相讀得到）。不選共用（S1：省記憶體，但 renderer 要能同時裝好幾個分頁、process 要計數、一個分頁卡在 script 裡時同 process 的一起卡住）。Chromium 的預設也是不共用（`ProcessReusePolicy::kDefault`），超過 process 數的軟性上限才重用同一個 site 的。做 `window.open` 那一份時 renderer 要能裝好幾個分頁，那時再看一般的分頁要不要共用、要不要設 process 數的上限（記在 RFC 0122 的 `roadmap.md` 的「每一份開始時要定的（先記下來）」的 W）。
11. **renderer 不是自己的 process 時一樣換（I1）**（2026-11-27，使用者決定）：Browser 這一側只有一條路，不看 renderer 是哪一種。renderer 是 Browser 的 process 裡的 thread 時（`--single-process`、`cargo test` 裡直接建的分頁、沒有平台視窗的情境、reftest），跨網站的導覽啟動一組新的 thread（新的 V8 isolate），舊的那組跑完 `unload` 結束；另一端沒有東西的（`Detached`）換一個新的。理由：machopo 大部分的測試跑在 thread 的 renderer 上，這樣它們測到的是換的那條路；下一份的 process lock 可以假設「一個 renderer 一個 site」在三種跑法下都成立，不用為 thread 開例外；`--single-process` 的行為和正式的一致。和 Chromium 的 `--single-process` 在 Browser 這一側一樣（照樣算 `SiteInstance`、換 `RenderFrameHost`、跑舊的 `unload`；`RenderProcessHost::IsProcessLimitReached` 第一行 `if (run_renderer_in_process()) return true;`，每個 `SiteInstance` 分到同一個 renderer），renderer 這一側不同：它在同一個 renderer 裡換一個新的 frame，machopo 的 renderer 只裝一個分頁（第 10 條），所以換一組新的 thread。代價：`--single-process` 換不到安全上的好處；thread 不能從外面結束，舊的 renderer 500 ms 沒回報時只能叫 V8 停掉 script、放著等它結束（machopo 關閉時對卡住的 thread 已經這樣做），卡在原生程式碼裡的會留著。不選只有自己的 process 才換（I2：Browser 兩條路，測試跑的大多是不換的那條；Chromium 沒有這種做法）。renderer 能裝好幾個分頁之後重新考慮（RFC 0122 的 `roadmap.md` 記在 W 底下）。
12. **分四段做：鋪路、DevTools 的狀態、規則和換、收尾（G1）**（2026-11-27，使用者決定）：照「分階段」的順序。理由：每一段可以單獨 merge 回 `main`，`main` 上的行為一直一致：前兩段不改變什麼時候換，用現有的「renderer 當掉之後重新載入」驗證（它已經是一次換 renderer）；第三段把換打開時，DevTools 和 WebDriver 的 session 已經接得上。不選先做換、再補工具（G2：最早看到成果，但中間 `main` 上跨網站的導覽會讓 session 掉狀態，每天的 CI 經過 machopodriver 跑 WPT，可能在那段期間失敗）。

## 待決定

這份 RFC 層級的（第 1 到 12 條）定了。剩下的在各段開始時定：

- **DevTools 的 session 的狀態怎麼跨過換 renderer**：那一段開始時定（Browser 記下再重播，或照 Chromium 讓 agent 回報狀態、重新接上時交還）。
- **資料模型的名字**：照 Chromium 建 `SiteInstance`、`BrowsingInstance`，還是先只記「這個 renderer 的 site」；做規則的那一段開始時定。

## 之後的可能

- process lock 和 Browser 的檢查（路線圖的 B），直接建在這份上。
- 開始時先猜、先啟動（Chromium 的 speculative `RenderFrameHost`），或備一個 spare renderer：其他平台量到啟動明顯比較慢時。
- 對話框的 RFC：`alert`、`confirm`、`prompt` 和 `beforeunload`（已決定的事項第 8 條），排在路線圖的 C、B 之後。
- paint holding：新文件有內容之前留著舊的畫面（renderer 不交 frame，或 Viz 墊著舊的 surface），對所有的導覽；跨 origin 時照 Chromium 的限制一起考慮。
- `sendBeacon` 和有作用的 `keepalive`：頁面結束之後 Browser 繼續完成它離開時送出的請求（Chromium 讓 process 多留最多 30 秒，`kKeepAliveHandleFactoryTimeout`）。
- back-forward cache：舊的 renderer 不收掉、留著給上一頁。
- renderer 能裝好幾個分頁之後（`window.open` 那一份要的）：同一個 site 的分頁共用 process、process 數的上限、thread 的 renderer 照 Chromium 在同一個 renderer 裡換 frame，都重新考慮一次（RFC 0122 的 `roadmap.md` 記在 W 底下）。
- 錯誤頁有了強的能力時（例如憑證錯誤頁的「仍要前往」），所有錯誤頁改放一個專門的、沒跑過網頁內容的 process（Chromium 的做法）。
- 本機檔案之間的隔離：Browser 照檔案或資料夾限制一個 process 能要哪些本機檔案（Chromium 也列為缺口）。
