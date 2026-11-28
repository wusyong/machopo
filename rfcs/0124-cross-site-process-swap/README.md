# RFC 0124：跨網站的導覽換 renderer

- 開始日期：2026-11-27
- 狀態：✅ 已完成：2026-11-28（Linux：Fedora 45）
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

- 「隔離」的「名稱表和屬性值的表在同一個 renderer 裡共用」：同一個分頁先後導覽過的**不同網站**不再共用（各自的 process）；同一個網站的網頁之間、頁面和它跨網站的 iframe 之間照舊。改了。
- 「隔離」的「新文件畫出來之前留著舊的畫面，不看是不是同一個 origin」：照 Chromium 的條件（已決定的事項第 7、19 條），刪掉了。
- 「GC 和 `dom`」的「名稱表只增不減……同一個分頁導覽到別的網頁也不清」：跨網站的導覽之後跟著舊的 process 還給系統，同一個網站裡的照舊。改了。

新的限制：舊頁面在 `pagehide`、`unload` 裡送出的請求跟著舊的 process 取消（第 6 條）、`--single-process` 時卡在原生程式碼裡的舊 renderer 留著（第 11 條），寫在 `docs/limitations.md`。

## 設計概要

使用者在 `https://a.example/` 點了一個到 `https://b.example/` 的連結：

1. Browser 照舊自己載入 `b.example` 的文件（跟著 redirect）。
2. response 到了、要 commit 時，Browser 看最後的網址：它的 site 和這個分頁的 renderer 現在的 site 不同，就啟動一個新的 renderer process，把 commit 送給它。
3. 舊的 renderer 收到「你的文件被換掉了」：分派 `pagehide`、`unload`，回報做完；Browser 收掉它。它不回報的話，等一段時間之後結束它。
4. 分頁的 frame sink 換成新的 renderer 的；大小、可見、focus 再送一次；DevTools 的 session 接到新的 renderer 上。

同一個 site 裡的導覽（`a.example/x` 到 `a.example/y`、`www.a.example` 到 `shop.a.example`）和現在一樣，不換。新分頁的第一次導覽也不換：分頁一開始的 renderer 還沒裝過任何網站，直接用它。

寫 machopo 的人：`WebContents` 的 `process` 不再是「這個分頁的 renderer」，而是「現在這個」；跟著 renderer 的狀態（frame sink、loader、DevTools 的 agent）在換的時候重建，要跟著分頁的東西放在 `WebContents` 或 Browser 的其他地方。

## 詳細設計

開始時的盤點、Chromium 的做法和量到的數字在 `details.md`。

### 現在最接近的路：`relaunch`

renderer 當掉之後的下一次導覽，`WebContents::relaunch` 原本就會「換一個 renderer」：新的 frame sink、啟動新的 `RenderProcessHost`、放掉舊的、把分頁的狀態（DevTools 的 token、大小、螢幕上的位置、可見、focus）再送一次。跨網站的換從它推廣（兩者共用 `replace_renderer`），多出來的是：

- **決定的時機**：`relaunch` 在導覽開始時；跨網站的要看到最後的網址（redirect 之後）才知道。導覽原本在開始時綁定 renderer 的 `Committer`，換成只有參數的 `CommitParams`，commit 時由分頁當時的 renderer 送（`RenderProcessHost::commit_navigation`）；renderer 在那之前不在了，就先 `relaunch`（第 13 條）。
- **換的順序**：commit 時換 renderer，要在 frame 樹記下「正在 commit 哪一筆」（`FrameTree::committing`）之前清空 frame 樹、換好 renderer，否則新的 renderer 回報 commit 時 Browser 找不到那一筆，當成壞訊息；網址列在那一段顯示的是那一筆（`visible_url`）。
- **舊的 renderer 還活著**：要叫它跑 `unload`、等它、逾時結束它；它在這段時間送來的訊息不能被當成新的 renderer 的（`RendererId` 和 `is_current_renderer` 已經在擋）。
- **舊的畫面**：`relaunch` 直接作廢舊的 sink（那時本來就是 sad tab）；跨網站的換照第 7、18、19 條（「畫面」）。
- **DevTools 的 session**：原本靠 `is_crashed` 從 `true` 變回 `false` 才重新接上，換 process 沒有「當掉」這一步。自動化改成記著每個分頁上次的 renderer（`WebContents::renderer_id`），不同了就處理還在路上的命令（第 15 條）；session 本身由 `WebContents` 接到新的 renderer 上（「DevTools 的 session」）。`Page.navigate` 和 `Machopo.waitForLoad` 在 renderer 不在了時，若等的是繼續的那一次導覽就繼續等。
- **`headless` 和視窗**：`headless`（reftest 走的路）的畫面跟著換 sink、視窗清掉跟著舊 renderer 的 UI 狀態（游標、滑鼠的 capture、選單）；`relaunch` 時不用，那時 `headless` 本來就以錯誤結束、視窗在 `RendererGone` 時清掉了（「換」）。

### 規則

`browser` 的 `site_info.rs`（第 17 條）：`SiteInfo::of_url`（`about:blank` 沒有 site；其他 `about:` 照名字；`file:` 一個；其他照 `site::Site`，解析不了的是新的 opaque）和 `swaps`（這個 renderer 的 `SiteInfo`、導覽的 `FrameLoadType`、最後的網址的 `SiteInfo`）：重新整理不換；任一邊沒有 site 不換；其餘不同就換。`WebContents::commit` 對 main frame 的文件問它（錯誤頁照它要去的網址），換了之後第一次有 site 的 commit 把 site 記在 `RenderProcessHost` 上。

### DevTools 的 session

- 狀態（第 14 條）：renderer 的六個 agent 和 V8 的 inspector（rusty_v8 的 fork 接出的 `V8InspectorSession::state()`）在命令改了狀態時寫出，renderer 送 `RendererHostMsg::DevToolsSessionState`（超過 1 MiB 是壞訊息）；`WebContents` 記著 session 和最後的狀態，換 renderer 時在送 commit 之前用 `DevToolsAttach` 交還，新文件的第一個請求已經在 `Network` 的眼裡。依網址的斷點由 `bindings` 記下 `setBreakpointByUrl`、重新接上時用同樣的 id 重送；script 暫停或被打斷時 V8 處理的命令也回報狀態；同一個 renderer 裡換 engine（JS 和 wasm）時照最後的狀態接上。
- 還沒回答的命令（第 15、16 條）：自動化記著每個命令送給了哪個 renderer；當掉時以 `"Target crashed"` 失敗，換了時可能跑 script 的五個以 `"Inspected target navigated or closed"` 失敗、其餘的送給新的 renderer（`forward_to_renderer`）；分頁沒有 renderer 時送來的立刻以 `"Target crashed"` 回答。
- 自動化自己送給 renderer、等它回答的請求（`Machopo.queryElement`、visual state、捲動、WebDriver 的方法）照第 21 條：記著送給了哪個 renderer 和重送要的參數（`SentToRenderer`），換了時會跑 script 或改頁面的 WebDriver 方法失敗，其餘的照順序重送（`resend_to_renderer`）。送給被換掉的 renderer 的 input 不會有 ack：`Machopo.waitForInputHandled` 在分頁的 `input_in_flight` 歸零時（`swap_renderer` 歸零）回答，不等 ack。

### 換

- `WebContents::swap_renderer`：`replace_renderer`（和當掉之後的 `relaunch` 共用：新的 frame sink、新的 `RenderProcessHost`、把分頁的狀態和 DevTools 的 session 先告訴它），舊的交給 `RenderProcessHost::unload`；frame 樹清成只剩 main frame（舊文件的子 frame 跟著走）、還沒 ack 的 input 歸零；commit 的參數拿掉 renderer 自己的導覽編號（`begin_id` 是舊 renderer 的）。啟動失敗時導覽停掉，分頁留在原來的頁面（第 6 條）。
- `RenderProcessHost::unload`：IO thread 不再把它的訊息往上轉，送 `RendererMsg::Unload`，在一條 `UnloadTimer` thread 上等它結束：process 最多 500 ms（之後 kill），thread 等 500 ms 之後每 10 ms 打斷它的 script，5 秒還不結束就放著（第 11 條）。`Detached` 的不等。
- renderer：`Unload` 和 `Shutdown` 一樣結束暫停、讓等著 Browser 回答的呼叫放棄（`task_queue`），main thread 對最上層的文件跑 `unload_document`（`pagehide`、`unload`，再來子 frame 的），然後離開 event loop。
- 嵌入的人：`headless` 收到新 sink 的 `RegisterFrameSinkId` 時把輸出的 root 換成它（`Display::set_root_frame_sink`），舊的 `InvalidateFrameSinkId` 時拿掉；視窗在每個分頁事件之後問 `WebContents::take_renderer_replaced`，換了就清掉游標、滑鼠的 capture 和選單（選單不回答：問的 renderer 不在了）。
- 自動化：還沒回答的命令照第 15 條（「DevTools 的 session」）。

### 畫面

- renderer 在最上層的文件第一次有 sticky activation 時（input 分派之後看）送 `RendererHostMsg::DidReceiveUserActivation`，每份文件一次；`WebContents` 記著（`main_frame_activated`），main frame 的新文件回報 commit 時歸零。
- `WebContents::allows_paint_holding`（第 19 條）：分頁還沒 commit 過、舊頁面被操作過、或新舊同一個 origin。`commit` 對 main frame 算一次：結果放進 `CommitParams`（`CommitNavigation` 的 `paint_holding`，renderer 不能留時把 4 秒的計時改成現在，`end_paint_holding`），換 renderer 時也交給 `swap_renderer`。
- 換的時候能留、舊的 sink 畫過（`sink_drawn`，Viz 在 sink 第一個 frame 到時經過 `BeginFrameObserver::with_first_frame` 送 `ContentsEvent::FirstFrame`）：舊的 sink 不作廢，記成 `fallback_sink`，`WebContents::fallback_surface_id` 給視窗嵌進 `SurfaceLayer`（`fallback_surface_id`）、compositor 放進 `Material::Surface` 的 `fallback`；aggregator 先找 primary 的 frame，沒有時畫 fallback 的（第 18 條）。新的 sink 的第一個 frame、下一次換、renderer 不在了時作廢它。舊的 sink 沒畫過時什麼都不留（Chromium 的 `TakeFallbackContentFrom` 也是）。
- `headless` 不留：輸出只看最後的畫面，換的時候直接改畫新的 sink。

### 測試

測試的清單和 mutation test 的結果在 `progress.md`。

- 規則的函式：`site_info` 照表的單元測試（每一種網址、每一種導覽）。
- `browser` 的整合測試：`process_swap.rs`（跨網站的 commit 在新的 renderer、回到之前的網站又是新的、留著的幾種、舊文件的子 frame、fallback 的 sink）、導覽的 paint holding、storage 跨過換 renderer、DevTools 的狀態（`devtools_session.rs`）、renderer 在導覽途中不在了（`renderer_gone.rs`）。
- `shell` 的整合測試（真的 renderer，`127.0.0.1` 和 `localhost` 兩個網站）：`pagehide`、`unload` 有跑（process 和 thread 兩種、在 debugger 裡暫停的、不結束的被結束）、還在路上的 DevTools 命令、換過去的 input、視窗清掉舊的 UI、換的中間畫出來的像素、`headless` 的輸出。
- 情境 `sites.txt`（第 20 條的 `navigate-site`：`a.test` 和 `b.test`）：點連結到別的網站、上一頁和下一頁、新的 renderer 收得到 input、`sessionStorage` 留著；`--window` 時截圖給人看換的中間。
- WPT：跨 origin 導覽的目錄經過 machopodriver 跑的是真的 process；`unloading-documents/` 整個通過的 12 個加進清單。
- 量測工具的測試（`scripts/bench`）：它們照 process 找 renderer，都只導覽到一個 site，不會碰到換。

### 分階段

四段（已決定的事項第 12 條）：

1. **鋪路**：commit 時才定送給哪個 renderer，renderer 在導覽途中不在了照第 13 條（原本卡在 sad tab 的這一種行為改了，其餘不變）；自動化照分頁的 renderer 是哪一個重新接上 DevTools 的 session，不再靠 `crashed` 變回來（`headless` 和視窗在第 3 段，見「現在最接近的路：`relaunch`」）。用「當掉之後重新載入」和「載入途中當掉」驗證。
2. **DevTools 的 session 的狀態跨過換 renderer**：同樣先用當掉之後重新載入驗證。
3. **規則和換**：跨網站的 commit 在新的 process；舊文件的 `unload`；中間的畫面和同一個 renderer 裡跨 origin 的 paint holding（第 7 條）；`headless` 跟著換 sink、視窗清掉跟著舊 renderer 的 UI 狀態。
4. **收尾**：量測工具、WPT 的預期結果、情境（第 20 條）、文件。

每一段做完 `main` 上的行為都是一致的：前兩段不改變什麼時候換，第三段打開時工具已經接得上。第 3 段做的時候分成 3a（換）和 3b（畫面），分開審查。renderer 啟動的時間（第 5 條）做完之後在 macOS 和 Windows 上量了（2026-11-28，約 12 ms 和 11 ms，和 Linux 差不多），不用重新考慮提前啟動和備一個（第 5、9 條）。

## 缺點

- 每次跨網站的導覽多啟動一個 process：Linux 和 Windows 上約 11 ms，macOS 上約 12 ms（`details.md`）。
- 換的那一小段時間有兩個 renderer process。
- 跟著 renderer 的狀態在跨網站的導覽時沒了（renderer 裡的圖片快取）：回到前一個網站時要重新解碼。同一個網站裡的導覽不受影響。
- `browser` 多一種狀態（舊的 renderer 在跑 `unload`）要維護。

## 理由和其他做法

- **不做**：上面「動機」的洞留著，process lock 做不了。
- **每次導覽都換**（不看 site）：規則最簡單，但同一個網站裡每點一次都多 11 ms、丟掉快取，沒有換到安全上的好處（同一個 site 本來就互相讀得到）。
- **只換 Browser 發起的**（網址列、書籤；Chromium 沒有 site isolation 時的做法）：擋不住頁面自己設 `location` 把分頁導覽走的情況，正是動機裡的那個洞。

其餘的選擇在「已決定的事項」。

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
5. **response 到了才決定換不換，當場啟動新的 process（T1）**（2026-11-27，使用者決定）：redirect 跟完、要 commit 時，照最後的網址和第 1 到 4 條的規則決定；要換就啟動新的 renderer、commit 直接送給它（訊息排在通道裡）。載入期間只有舊的 renderer，舊的頁面照常顯示、照常能用。理由：不會猜錯、沒有要丟掉的 process，Browser 不多一種「猜的那個 renderer」的狀態（被取消、被取代、redirect 到別的網站時怎麼收）；代價是啟動的時間（這台 Linux 約 11 ms）排在 response 之後，不能和網路請求同時。不選開始時先猜、先啟動、response 到了再確認（T2，Chromium 的 speculative `RenderFrameHost`：猜對時省下那 11 ms，但經過轉址的連結、http 轉 https、登入後跳轉都會猜錯，白啟動一個；載入期間多一個 process）；之後可以升級成它，決定的函式是同一個。不選只在開始時決定（T3：redirect 到別的網站時 B 的文件落在照 A 猜的 process 裡）。**要做的**：在 macOS 和 Windows 量一次啟動的時間，明顯比較慢時重新考慮 T2。2026-11-28 都量了：macOS 約 12 ms、Windows 約 11 ms，和 Linux 差不多，不用重新考慮；Windows 上換 process 的導覽比同一個 site 裡的多約 12.5 ms（`details.md` 的「量的：啟動一個 renderer process 要多久」）。
6. **送出 commit 的同時換過去、叫舊的跑 `unload`，不等它（U1）**（2026-11-27，使用者決定）：決定要換的那一刻一次做完：啟動新的 renderer、送 commit，分頁從此算它的；同時告訴舊的 renderer 它的文件被換掉了。舊的分派 `pagehide`、`unload`、回報做完，Browser 收掉它；500 ms 沒回報就結束它（Chromium 的 `kUnloadTimeout`）。新的 renderer 啟動失敗（當場知道）時留在舊的頁面。理由：換只發生在一個時間點，是 `relaunch` 的推廣，Browser 不用管「舊的還算目前的、新的正在 commit」的中間狀態；新頁面不等舊頁面的 handler，卡住的舊頁面擋不住導覽。代價：新的 renderer 在 commit 之前當掉時是 sad tab，不是留在舊的頁面。不選新的回報 commit 之後才換（U2，Chromium 的順序：舊的頁面多留幾十毫秒、新的當掉時留在舊的，但 Browser 要同時認兩個 renderer；Chromium 提前啟動新的 process，並存的是整段載入，第 5 條之後這裡只有幾十毫秒）。不選等舊的跑完才 commit（U3：舊頁面的 handler 拖住導覽）、不跑直接結束（U4：靠 `pagehide` 存狀態的頁面壞掉）。知道的後果：舊頁面在 `pagehide`、`unload` 裡送出的網路請求在舊的 process 收掉時取消（沒有 `sendBeacon`，`keepalive` 沒有作用）；寫 `localStorage` 不受影響（在回報做完之前就到 Browser）。記進 `docs/limitations.md`。
7. **換的中間：使用者操作過舊頁面時留著舊的畫面，沒有時不留（P2）；同一個 renderer 裡跨 origin 的導覽照同一條規則**（2026-11-27，使用者決定；同一天先定了 P0，是照一個錯的事實定的，改了）：
   - **現況**（`details.md` 的「畫面：paint holding」）：machopo 的 renderer 有 paint holding：main frame 的新文件 commit 之後、能畫出來之前（body 還沒開始、被 stylesheet 擋住），不交新的 frame，畫面上留著舊頁面，最多 4 秒（Blink 的 `kNewContentRenderingDelay`），之後交一個只有背景的 frame。不看新舊是不是同一個 origin、舊頁面有沒有被使用者操作過：Chromium 修掉的偽造（issue 40942531：「Origin spoof caused by navigation that doesn't paint any content」：網址列是 B、畫面是 A 留下的，最多 4 秒，重複導覽還能延長）machopo 現在就有。
   - **換 process 時**：新的 sink 還沒有 frame 時，Viz 先畫舊的 sink 的最後一個 frame（Chromium 的 fallback surface），**只在舊頁面有過使用者的操作（sticky user activation）時**；沒有時直接顯示新的 sink（白色的底，到新頁面畫出來）。期限不另外做：新的 renderer 自己最多 4 秒就交出 frame。Browser 在換的時候要知道舊頁面有沒有被操作過：renderer 告訴它。
   - **同一個 renderer 裡**（同 site、跨 origin，例如 `a.example` 到 `shop.a.example`）：同一條規則（判斷改由 Browser 做、commit 時告訴 renderer，見第 19 條）放在 renderer：新文件和舊文件不同 origin、舊頁面沒有被操作過時，不留舊的畫面。和換 process 在第 3 段一起做。
   - 規則照 Chromium 的 `navigator.cc`（`allow_main_frame_paint_holding`）：新舊同一個 origin、舊頁面有過使用者的操作、或從新分頁的初始空白文件出發時才留；註解：「Commonly used attacks are done without user activation -- which will not enable paint holding.」
   - 理由：點連結離開（最常見的）照樣不閃白；頁面自己用 script 導覽走時不留，偽造做不成。不選換的那一刻就顯示新的 sink（P0：點連結到別的網站會閃白，Chromium 那時留著舊的畫面）。不選一律留著（P1：把 Chromium 修掉的偽造帶過去）。
8. **`beforeunload` 不在這份，另開一份 RFC 和對話框一起做，排在路線圖的 C、B 之後（B0、X1）**（2026-11-27，使用者決定）：換 process 不需要它：它接在導覽開始之前（Browser 發起的先問目前的 renderer，頁面發起的 renderer 自己先跑），那時舊的 renderer 還是目前的（第 5 條）。它要的是 machopo 現在沒有的：Browser 的對話框、renderer 停下來等使用者的回答、WebDriver 處理對話框的命令、關分頁和重新整理也要問；前三樣和 `alert`、`confirm`、`prompt` 是同一套，一起做只蓋一次。不選當這份的一個階段（X2：這份變大、process lock 被擋住，對話框只為它蓋一半）、現在就做排在 B 之前（X3：它是相容性的功能，B 關的是安全的洞）、只分派事件永遠不問（頁面以為攔得住使用者）。
9. **不預先備一個 renderer（R0）**（2026-11-27，使用者決定）：啟動一個約 11 ms（這台 Linux），不到一個 frame；不多養一個平常沒在用的 process（一個 V8 的 isolate，有 GPU 時加 4 MiB 的共用記憶體；佔多少沒有量），不多一組「什麼時候備、記憶體吃緊時不備、被拿走之後何時補」的規則。不選一直備一個（R1，Chromium 桌面版的預設 `kSpareRendererForSitePerProcess`：它的 renderer 啟動慢得多）。和第 5 條綁在一起：macOS 和 Windows 量到啟動明顯比較慢時，提前啟動和備一個一起重新考慮（macOS 和 Windows 量到的和 Linux 差不多，不用）。
10. **同一個 site 的分頁不共用 process（S0）**（2026-11-27，使用者決定）：每個分頁自己的 renderer，規則只看這個分頁現在的 renderer 是哪個 site 的。理由：一個分頁當掉、卡住或吃光記憶體不影響同網站的另一個；process 的生命跟著一個分頁，不用計數；安全上共不共用沒有差別（同一個 site 本來就互相讀得到）。不選共用（S1：省記憶體，但 renderer 要能同時裝好幾個分頁、process 要計數、一個分頁卡在 script 裡時同 process 的一起卡住）。Chromium 的預設也是不共用（`ProcessReusePolicy::kDefault`），超過 process 數的軟性上限才重用同一個 site 的；Chrome 只對預設的搜尋引擎例外，一個 process 最多 2 個不相干的分頁（2026-11-28 查證時補上的，`details.md` 的「預備的 renderer 和重用」）。做 `window.open` 那一份時 renderer 要能裝好幾個分頁，那時再看一般的分頁要不要共用、要不要設 process 數的上限（記在 RFC 0122 的 `roadmap.md` 的「每一份開始時要定的（先記下來）」的 W）。
11. **renderer 不是自己的 process 時一樣換（I1）**（2026-11-27，使用者決定）：Browser 這一側只有一條路，不看 renderer 是哪一種。renderer 是 Browser 的 process 裡的 thread 時（`--single-process`、`cargo test` 裡直接建的分頁、沒有平台視窗的情境、reftest），跨網站的導覽啟動一組新的 thread（新的 V8 isolate），舊的那組跑完 `unload` 結束；另一端沒有東西的（`Detached`）換一個新的。理由：machopo 大部分的測試跑在 thread 的 renderer 上，這樣它們測到的是換的那條路；下一份的 process lock 可以假設「一個 renderer 一個 site」在三種跑法下都成立，不用為 thread 開例外；`--single-process` 的行為和正式的一致。和 Chromium 的 `--single-process` 在 Browser 這一側一樣（照樣算 `SiteInstance`、換 `RenderFrameHost`、跑舊的 `unload`；`RenderProcessHost::IsProcessLimitReached` 第一行 `if (run_renderer_in_process()) return true;`，每個 `SiteInstance` 分到同一個 renderer），renderer 這一側不同：它在同一個 renderer 裡換一個新的 frame，machopo 的 renderer 只裝一個分頁（第 10 條），所以換一組新的 thread。代價：`--single-process` 換不到安全上的好處；thread 不能從外面結束，舊的 renderer 500 ms 沒回報時只能叫 V8 停掉 script、放著等它結束（machopo 關閉時對卡住的 thread 已經這樣做），卡在原生程式碼裡的會留著。不選只有自己的 process 才換（I2：Browser 兩條路，測試跑的大多是不換的那條；Chromium 沒有這種做法）。renderer 能裝好幾個分頁之後重新考慮（RFC 0122 的 `roadmap.md` 記在 W 底下）。
12. **分四段做：鋪路、DevTools 的狀態、規則和換、收尾（G1）**（2026-11-27，使用者決定）：照「分階段」的順序。理由：每一段可以單獨 merge 回 `main`，`main` 上的行為一直一致：前兩段不改變什麼時候換，用現有的「renderer 當掉之後重新載入」驗證（它已經是一次換 renderer）；第三段把換打開時，DevTools 和 WebDriver 的 session 已經接得上。不選先做換、再補工具（G2：最早看到成果，但中間 `main` 上跨網站的導覽會讓 session 掉狀態，每天的 CI 經過 machopodriver 跑 WPT，可能在那段期間失敗）。
13. **renderer 在導覽途中不在了：照 Chromium 分三種（K1）**（2026-11-27，使用者決定；第 1 段開始時，用臨時的測試確認了 machopo 原本把 commit 送給不在了的 renderer、分頁卡在 sad tab、網址列停在要去的網址）：已經送出 commit、還沒回報的丟掉；頁面發起的（包括頁面發起的上一頁、下一頁）取消；Browser 發起的（網址列、重新整理、DevTools、WebDriver）繼續，commit 時 renderer 不在了就先啟動新的再送。理由：Chromium 的 `RenderFrameHostImpl::RenderProcessGone` 丟掉已經交給那個 frame 的導覽，頁面發起的導覽因為通往發起它的 renderer 的 `NavigationClient` 斷了而取消，Browser 發起的 commit 到新的 process（`must_be_replaced_for_crash`；`details.md` 的「renderer 在導覽的途中不在了」）；renderer 可能是因為壞訊息被結束的，它要求的導覽不該在新的 renderer 裡完成；使用者輸入的網址照樣會到。commit 時換一個 renderer 也就是第 3 段跨網站換 process 要走的同一條路。不選全部取消（K2：使用者輸入的網址不見，要再輸入一次）、全部繼續（K3：被攻破的 renderer 要求的導覽照樣完成）。
14. **DevTools 的 session 的狀態由 renderer 寫出、Browser 保管、換了 renderer 時交還（R4，Chromium 的 state cookie）**（2026-11-27，使用者決定；第 2 段開始時）：V8 的那一部分用它自己的 `V8InspectorSession::state()`（rusty_v8 的 fork 接出它，要重編 V8）；machopo 的六個 agent（Page、Network、Log、DOM、CSS、Overlay）各自寫出和還原自己的狀態；依網址的斷點由 `bindings` 在 `setBreakpointByUrl`、`removeBreakpoint` 經過時記下，重新接上之後重送（V8 15.2 的 `state()` 沒有它們，只能經過 `connectShared` 的 `V8EmbedderState` 傳進去、沒有拿出來的介面；重送的 id 照網址和行列算，和原本的一樣）。狀態變了 renderer 就送給 Browser（renderer 當掉時就來不及問），Browser 不解讀，換了 renderer 時在 `DevToolsAttach` 交還；同一個 renderer 裡 JS 和 wasm 的 engine 互換時也照它還原（原本 V8 的狀態在那時就不見）。理由：V8 是三塊裡最大、最不在 machopo 控制之下的，交給它自己存，V8 升級不用跟著改；知識留在各個 agent 裡；和 Chromium 一樣。代價：改 fork 的 C++、重編 V8、共用的 artifact 換；六個 agent 各多一點程式；有 DevTools 接著、狀態變了時多一則訊息。不選 Browser 照命令的清單重送（R1：不用動 V8，但 V8 有哪些命令改狀態要列在 Browser，升級時不會知道，也修不了換 engine 的那一種）。不選只補 Browser 已經記著的三個（R3：`Runtime`、`Debugger`、DOM、CSS 每次跨網站都不見）。`details.md` 的「DevTools 的 session 的狀態（第 2 段開始時的盤點）」。
15. **換 renderer 時還沒回答的 DevTools 命令照 Chromium（P1）**（2026-11-27，使用者決定）：可能跑頁面 script 的五個（`Runtime.awaitPromise`、`callFunctionOn`、`evaluate`、`runScript`、`terminateExecution`：Chromium 的 `MayCauseJavascriptExecution`）以 `"Inspected target navigated or closed"` 失敗，其餘的在新的 renderer 還原好狀態（第 14 條）之後重送；renderer 當掉時全部以 `"Target crashed"` 失敗，`Page.reload` 除外（`DevToolsSession::ClearPendingMessages`）。錯誤照 Chrome 的字串（原始碼的註解：「Clients match against this error message verbatim」），原本當掉時 machopo 回答的是自己的 `the tab's renderer has gone`。理由：和 Chromium 一致；`DOM.getDocument` 這種在新的文件上也能回答的，不用 client 重試。不選換的時候全部失敗（P2）、讓舊的 renderer 答完（P3：答的是已經離開的文件，當掉時照樣要另外處理）。
16. **renderer 不在時送來的 DevTools 命令立刻以 `"Target crashed"` 回答（T1）**（2026-11-27，使用者決定；第 15 條沒涵蓋的一種）：Chromium（`RenderFrameDevToolsAgentHost`）在 renderer 當掉之後把命令留在待處理的清單（沒有 agent 可送），等分頁重新載入、frame 又活過來時（`UpdateFrameAlive`）才把清單裡的全部以 `"Target crashed"` 失敗、再接上新的 renderer：當掉期間送來的命令不會在新的 renderer 生效。machopo 的結果一樣（失敗、同一個字串），時間點是當下，client 不用卡到有人重新載入。原本 machopo 回答的是自己的 `the tab's renderer has gone`，而且 Browser 記著 `Page.enable` 和 lifecycle 的開關、重新接上時替 client 重送：狀態改由 renderer 寫出（第 14 條）之後，當掉期間開的不會帶到新的 renderer（和 Chrome 一樣），要在重新載入之後再開。不選照 Chromium 的時間點等到重新載入（T2：沒人重新載入時永遠沒有回答）。
17. **資料模型：只建 `SiteInfo`，記在 `RenderProcessHost` 上（M1）**（2026-11-28，使用者決定；第 3 段開始時）：`browser` 的 `SiteInfo` 是一個值（Chromium 的 `SiteInfo`）：網頁的 `Site`、所有 `file:` 的一個（第 1 條）、內建頁照名字（第 3 條）、opaque 的每次都不同（網址列輸入的 `data:`、`blob:null/…`）。`RenderProcessHost` 記著它的 `site_info: Option<SiteInfo>`，`None` 是還沒裝過網站（新分頁的第一次導覽直接用它，第 4 條），第一次 commit 網頁時寫入；「換不換」是一個不碰 process 的函式（這個 renderer 的 `site_info`、這次導覽、最後的網址）。理由：名字對得上 Chromium；下一份的 process lock 直接從它算（Chromium 的 `ProcessLock::FromSiteInfo`，記在 process 上）；沒有共用的物件，不用 `Rc`。不選現在就建 `SiteInstance`、`BrowsingInstance`（M2：分頁不共用（第 10 條）、沒有 `window.open` 時，每個分頁剛好一個 `BrowsingInstance`、每個 renderer 剛好一個 `SiteInstance`，兩層都只包著一個值，共用它們要說不出理由的 `Rc`）；不選不建型別、在 `WebContents` 裡直接比 `Site`（M3：「這個 process 是哪個 site 的」沒有名字，`file:` 和內建頁的特例散在比較裡，process lock 要重新整理出來）。做 `window.open` 那一份時補 `BrowsingInstance` 和包著 `SiteInfo` 的 `SiteInstance`（RFC 0122 的 `roadmap.md` 的 W）。
18. **換 process 時舊的畫面：嵌入的 quad 多帶一個 fallback（F1，Chromium 的 `SurfaceRange`）**（2026-11-28，使用者決定；第 3 段）：`Material::Surface` 多一個 `fallback: Option<SurfaceId>`；aggregator 先畫 primary 的 sink 最新的 frame，沒有時畫 fallback 的 sink 的，都沒有才畫背景色。`WebContents` 換 process 時，舊頁面被操作過（第 7 條）就先不作廢舊的 sink、把它記成 fallback，嵌入的人（視窗、`headless`）照 `WebContents` 給的兩個嵌入；新的 sink 第一次有 frame 時 Viz 告訴 Browser（新的通知，Chromium 的 `OnFirstSurfaceActivation`），Browser 才作廢舊的 sink（新的 renderer 最多 4 秒一定交出 frame，所以一定會作廢）。理由：和 Chromium 同一套（`SurfaceManager::GetLatestInFlightSurface` 先找 primary、找不到退回開頭那組，新舊可以是不同的 frame sink；`RenderFrameHostManager::CommitPending` 的 `TakeFallbackContentFrom`，不允許留時 `ForceFirstFrameAfterNavigationTimeout` 拿掉 fallback）；新的 frame 一到 Viz 自己換過去，不等 Browser 的下一輪，嵌入的人不用各自寫什麼時候換。代價：協定的 quad 多一個欄位、aggregator 在 primary 沒有 frame 時多查一次、多一則 Viz 到 Browser 的通知。不選 Browser 先嵌舊的、知道新的有 frame 再換（F2：一樣要那則通知，晚一個 UI 的 frame，每個嵌入的人各寫一次）；不選 Viz 把舊的 frame 複製給新的 sink（F3：資源是舊的 renderer 的，擁有權和釋放會亂；Chromium 沒有這樣做）。
19. **paint holding 能不能留由 Browser 判斷，commit 時告訴 renderer（H1）**（2026-11-28，使用者決定；第 3 段開始時查證 Chromium 之後，改第 7 條「同一個 renderer 那一半放在 renderer」）：Browser 送 commit 時用一個函式判斷（初始的空白文件、新舊同一個 origin、或舊頁面有過使用者的操作；Chromium 的 `navigator.cc` 的 `allow_main_frame_paint_holding`），換 process（第 18 條）和同一個 renderer 都用它；同一個 renderer 時結果放進 `CommitNavigation`，不能留時 renderer 不等 4 秒、馬上交一個只有背景的 frame（現在逾時時的 `commit_empty_frame`），能留時照現在。新文件的 origin 是 Browser 定的；舊頁面有沒有被操作過由 renderer 告訴 Browser（第 7 條，換 process 本來就要），點擊記下操作和它發起的導覽在同一條通道、操作的通知在前，Browser 判斷時已經知道。理由：規則只有一份；和 Chromium 一樣由 Browser 判斷（它在 renderer 回報 commit 時判斷，不能留時 `ForceFirstFrameAfterNavigationTimeout` 拿掉 fallback）；之後的 process lock 不用信 renderer 說可以留。代價：commit 多一個 bool。不選 renderer 自己判斷（H2，第 7 條原本寫的：同一條規則兩份，Browser 一份、renderer 一份）；不選整個照 Chromium 改成 Browser 和 Viz 的 fallback、Browser 的計時器（H3：Chromium 的 paint holding 整個在 Browser 和 Viz，同一個 renderer 裡也是 Viz 先畫舊的 surface；machopo 的是 renderer 暫時不交 frame。改它是重寫一個已經在用、有測試的功能，每次導覽都要換 `LocalSurfaceId`，這份不需要）。H3 留給 OOPIF 的子 frame 的畫面那一份重新考慮（RFC 0122 的 `roadmap.md` 的 O2）：那時 Browser 已經替每個 frame 管 surface，子 frame 也要用 fallback（Chromium 對子 frame 只在同一個 origin 時留），統一的理由最充分；這裡的判斷函式照用。
20. **情境用兩個主機名稱做出兩個網站（S2）**（2026-11-28，使用者決定；第 4 段開始時）：情境多一個指令 `navigate-site <主機> <路徑>`：情境自己的 server（`net` 的 test server，每個情境一個）用 HTTP 提供那個檔案所在的資料夾，所有 `*.test` 的主機名稱都對到 `127.0.0.1`（沒有平台視窗時是 `net` 的 host mapping，`--window` 時是 `--host-resolver-rules`），頁面照 `location.port` 寫出 `http://b.test:<port>/…` 的連結。`.test` 是保留給測試的頂級網域（RFC 6761），`a.test` 和 `b.test` 是不同的 site。理由：點連結到別的網站、上一頁回到前一個網站、真的視窗裡換的中間的截圖都做得到；和 Chromium 的 browser test 一樣（`host_resolver()->AddRule("*", "127.0.0.1")`、`EmbeddedTestServer::GetURL("a.com", …)`）。代價：`scenario.rs` 和 `automation.rs` 多一個指令和它的解析測試、情境的 README。不選只用現有的指令（S1：網址列在 `file:` 和 `about:demo` 之間換，沒有點連結跨網站、也就沒有被操作過時留著的舊畫面）；不選不寫情境（S3：違反「看得到的行為寫成情境」）。
21. **換 renderer 時自動化自己還在等舊 renderer 回答的請求，照第 15 條分兩種（W1）**（2026-11-28，使用者決定；第 4 段的 `--window` 情境找到的）：`Machopo.queryElement`、visual state（`captureWindow`、`Page.captureScreenshot`、元素的截圖）、`scrollIntoViewIfNeeded` 和 WebDriver 的方法原本只照分頁記著，renderer 當掉時失敗，換掉時沒人處理、一直等（舊的 renderer 收到 `Unload` 就不再回答；真的 process 比較慢，查詢比較常落在換的那一刻）。改成記著送給了哪個 renderer：會跑頁面 script 或改頁面的 WebDriver 方法（execute script、send keys、clear）以 `"Inspected target navigated or closed"` 失敗，其餘的照送出的順序交給新的 renderer，回答的是新的文件（舊文件的元素參照是 `no such element`）。理由：和 DevTools 的命令同一條規則（Chromium 的 `MayCauseJavascriptExecution`）；情境的等待和 machopodriver 不用改。代價：每個請求多記一筆，回答時拿掉。不選全部失敗（W2：簡單，但情境的 runner 和 machopodriver 要在這個錯誤時重試，client 看到 Chrome 不會給的失敗；chromedriver 在大部分命令之前先等導覽結束）；不選全部重送（W3：給舊文件的 script 跑在新的文件上）。

## 待決定

沒有。

## 之後的可能

- process lock 和 Browser 的檢查（路線圖的 B），直接建在這份上。
- 開始時先猜、先啟動（Chromium 的 speculative `RenderFrameHost`），或備一個 spare renderer（第 9 條）：三個平台啟動一個 renderer 都約 11 到 12 ms（第 5 條），現在不需要；renderer 啟動變慢時再考慮。
- 對話框的 RFC：`alert`、`confirm`、`prompt` 和 `beforeunload`（已決定的事項第 8 條），排在路線圖的 C、B 之後。
- `sendBeacon` 和有作用的 `keepalive`：頁面結束之後 Browser 繼續完成它離開時送出的請求（Chromium 讓 process 多留最多 30 秒，`kKeepAliveHandleFactoryTimeout`）。
- back-forward cache：舊的 renderer 不收掉、留著給上一頁。
- renderer 能裝好幾個分頁之後（`window.open` 那一份要的）：同一個 site 的分頁共用 process、process 數的上限、thread 的 renderer 照 Chromium 在同一個 renderer 裡換 frame，都重新考慮一次（RFC 0122 的 `roadmap.md` 記在 W 底下）。
- 錯誤頁有了強的能力時（例如憑證錯誤頁的「仍要前往」），所有錯誤頁改放一個專門的、沒跑過網頁內容的 process（Chromium 的做法）。
- 本機檔案之間的隔離：Browser 照檔案或資料夾限制一個 process 能要哪些本機檔案（Chromium 也列為缺口）。
