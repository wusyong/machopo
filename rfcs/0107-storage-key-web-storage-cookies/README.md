# RFC 0107：持久資料的 key，Web Storage 和 cookie

- 開始日期：2026-11-20
- 狀態：🚧 已排程：2026-11-20
- 細節：`details.md`

## 摘要

machopo 現在不在磁碟上存任何東西，沒有 cookie、`localStorage`、`sessionStorage`，也算不出「網站」。這份先定下每種持久資料共用的基礎：網站的算法（public suffix list）、儲存的 key（origin 加最上層的網站，Chromium 的 `StorageKey`）、profile 的資料夾，和 renderer 同步讀 Browser 管的資料的路；再用兩種功能把它用過一遍：Web Storage（`localStorage`、`sessionStorage`、`storage` 事件）和 cookie（`Set-Cookie`、`Cookie`、`SameSite`、`document.cookie`、WebDriver 和 DevTools 的 cookie 命令）。HTTP cache、IndexedDB 之後建在同一套基礎上。範圍和 key 的形狀定了（第 1、2 條），其他的還在比較，見「要決定的事（依序）」。

## 動機

RFC 0103 的「架構上可能要先做的事」第 6 個：cookie、HTTP cache、`localStorage`、IndexedDB 要用同一把 key（origin 加最上層的網站），第一份儲存的 RFC 定下來；晚定的話，每種儲存各自長出一套「這份資料是誰的」，之後要一起改。另外，沒有 cookie 就不能登入任何網站，沒有 `localStorage` 的頁面很多一開就丟例外（它是 `window` 上的屬性，script 直接讀）。

### 現況

2026-11-20 讀程式得到的（`main` 的 `e2d10d6`；逐項的位置在 `details.md` 的「machopo 現在的狀況」）：

- **網站**：沒有 public suffix list，算不出 registrable domain（DevTools 的 `domainAndRegistry` 一律是空的）；程式裡沒有「網站」的型別。origin 直接用 `url::Origin`。
- **誰的請求**：`ResourceRequest` 只有 `initiator`（renderer 填的 origin 字串）和 `destination`，沒有最上層的網站、沒有 frame；renderer 的 `FetchResource` 不帶 frame，Browser 不核對 `initiator`。Browser 的 frame 樹知道每個 frame 的 origin 和 parent，但導覽的請求只帶網址和 destination。
- **cookie**：`net` 沒有任何 cookie 的程式（reqwest 沒開 `cookies`，redirect 是 `net` 自己跟的）；`Set-Cookie` 在回應交出去之前濾掉；`CredentialsMode` 只影響 CORS 的檢查。`navigator.cookieEnabled` 是 `false`，沒有 `document.cookie`。machopodriver 的 cookie 指令回答 `unknown command`，CDP 的 `Network.getCookies` 回答空的。
- **Web Storage**：沒有 `Storage`、`localStorage`、`sessionStorage`、`StorageEvent`（只有 `onstorage` 這個 handler 屬性）。
- **同步的呼叫**：renderer 的 main thread 從來不等 Browser 的回答（訊息都是送了就走）；只有 worker 的 `importScripts` 在自己的 thread 上等回應。
- **磁碟**：沒有 profile 的資料夾，沒有 `--user-data-dir`，不寫任何持久的檔案。每個分頁一個 renderer，所有分頁共用一個 `NetworkContext`（視窗開的時候建一次）。

### 目標

1. 「網站」算得出來，和 Chromium 的 `SchemefulSite` 一樣（scheme 加 registrable domain）。
2. 一個 key 的型別，每種儲存都用它；由 Browser 從 frame 樹算，renderer 不能自己宣稱。
3. profile：資料存在哪、哪些模式要存、測試和自動化怎麼不互相影響。
4. Web Storage 和 cookie 做到 WPT 的 `webstorage/`、`cookies/` 大部分通過，一般網站的登入和偏好設定能用。
5. 之後的 HTTP cache、IndexedDB 不必再改 key 和 profile。

### 不在範圍內

- HTTP cache、IndexedDB、Cache Storage、service worker、`navigator.storage`（quota 的 API）。
- Cookie Store API（非同步的 `cookieStore`）、Storage Access API、`Clear-Site-Data`。
- 無痕模式、多個 profile、同步、清除瀏覽資料的 UI。
- session restore（重開之後還原分頁和 `sessionStorage`）。
- blob URL 照 key 分（現在照登記它的 renderer；「之後的可能」）。

### 要解決的限制

`docs/limitations.md` 的：「網路」的「沒有 cookie」那一半（HTTP cache 留著）；`navigator` 的 `cookieEnabled` 是 `false`；`fetch()` 的 `credentials`「因為沒有 cookie，只改變 CORS 要求對方的條件」；WebDriver 的沒有 cookie 指令；DevTools 的 frame 的 `domainAndRegistry` 一律是空的；CDP 的 cookie 命令一律是空的。

## 設計概要

對網頁的作者：`localStorage.setItem("theme", "dark")` 之後，關掉 machopo 再開，同一個網站讀得回來；另一個分頁的同源文件收到 `storage` 事件。`sessionStorage` 只在那個分頁裡，分頁關了就沒了。伺服器的 `Set-Cookie` 留下來，之後對那個網站的請求帶 `Cookie`；`document.cookie` 讀得到不是 `HttpOnly` 的。嵌在別的網站裡的 iframe（第三方）有自己的一份儲存，和它自己當最上層時的不是同一份。

對 machopo 的使用者：登入的狀態和網站的設定留在 profile 的資料夾裡；`--user-data-dir` 換一個資料夾。自動化和測試用暫時的 profile，每次從空的開始。

對之後改這部分程式的人：要存東西時，向 Browser 要「這個 frame 的 key」，不自己從網址算；renderer 送來的讀寫只帶 frame，Browser 用 frame 樹換成 key。

## 詳細設計

還是草稿：下面是 2026-11-20 的看法，還沒確認的寫在「要決定的事（依序）」和「待決定」。Chromium 怎麼做、machopo 現在的程式在 `details.md`。

### 基礎

- **網站**：`Site`（Chromium 的 `net::SchemefulSite`）：opaque 的 origin 各自是一個網站；有網路 host 的 scheme 是 scheme 加 registrable domain（沒有 registrable domain 的，例如 IP 和 `localhost`，是 host），不看 port。registrable domain 要 public suffix list。
- **key**：`StorageKey`（`blink::StorageKey`）：origin、最上層的網站、ancestor chain bit（origin 和最上層同一個網站，但中間隔著別的網站的 frame 時算第三方）。沒有 nonce（machopo 沒有 credentialless 的 iframe）。opaque 的 origin 的 key 不能序列化：它的儲存只在記憶體裡、跟著文件走。
- **誰算**：Browser 在文件 commit 時從 frame 樹算出那個 frame 的 key（它的 origin、root 的網站、ancestor 之間的關係），記在 frame 的節點上。renderer 的儲存訊息只帶 frame 的 token。
- **profile**：一個資料夾，裡面每種儲存各一份檔案。視窗模式用平台的使用者資料夾底下的 `machopo`（或 `--user-data-dir`）；headless、情境、`--automation-pipe`、測試預設只在記憶體裡。同一個 profile 同時只給一個 machopo（Chromium 的 `ProcessSingleton`）。

### Web Storage

照 Chromium 的 `CachedStorageArea`：文件第一次碰 `localStorage` 時，renderer 同步向 Browser 要整個 area（那個 key 的全部 key/value），之後的讀都在 renderer 自己的那一份上；寫先改自己的那一份、再告訴 Browser，Browser 寫進磁碟、通知其他看著同一個 area 的文件（它們更新自己的那一份、發 `storage` 事件）。每個 area 的上限是 10 MiB（Chromium 的 `kPerStorageAreaQuota`），超過丟 `QuotaExceededError`。`sessionStorage` 多一層：每個分頁一個 namespace，不寫磁碟。

這是 renderer 的 main thread 第一次同步等 Browser：等的方式是「要決定的事（依序）」第 6 個。

### cookie

cookie 存在 `net` 那一側（Chromium 的 `CookieMonster` 在 network service）：每一跳請求（redirect 是 `net` 自己跟的）照網址、`SameSite`、`Secure`、credentials mode 決定帶哪些 `Cookie`，每個回應的 `Set-Cookie` 照 RFC 6265bis 和 Chromium 的規則收下，照舊不交給 renderer。要判斷 `SameSite` 和第三方，請求得帶著可信的來歷：最上層的網站、site for cookies、是不是 main frame 的導覽、發起者的網站。這些由 Browser 從 frame 樹填，不由 renderer 填：`FetchResource` 要帶 frame，Browser 核對 `initiator`。

`document.cookie` 同步向 Browser 問（Chromium 的 `RestrictedCookieManager::GetCookiesString` 是 `[Sync]`），`HttpOnly` 的不給；寫是送了就走。持久的 cookie 寫進 profile，session cookie 只在記憶體裡。

### 要決定的事（依序）

1. ~~範圍~~：基礎、Web Storage、cookie 都做，分階段（第 1 條）。
2. ~~key 的形狀~~：第三方的儲存照最上層的網站分開（第 2 條）。
3. ~~public suffix list 從哪來~~：`psl` crate（第 3 條）。
4. ~~profile~~：位置和哪些模式寫磁碟（第 5 條）、第二個 machopo 拒絕啟動（第 6 條）。
5. ~~磁碟上的格式~~：SQLite（第 7 條）。
6. ~~renderer 怎麼同步等 Browser~~：回答走一般的訊息，到 scheduler 裡等回答的位置（第 9 條）。
7. ~~Web Storage 的細節~~：~~`storage` 事件怎麼到其他分頁~~（第 11 條）、~~`sessionStorage` 的 namespace 跟著什麼~~（第 12 條）、~~上限怎麼算~~（第 13 條）。
8. ~~cookie 的實作~~：自己照 RFC 6265bis 和 Chromium 寫（第 15 條）。
9. ~~請求的來歷~~：每份文件一個 Browser 給的 loader 編號（第 16 條）。
10. ~~第三方的 cookie~~：預設擋掉沒標 `Partitioned` 的，加一個照送的旗標（第 17 條）。
11. **`document.cookie` 的同步讀**：每次都問 Browser，或照 Chromium 留一個版本號、沒變就用上一次的。
12. **自動化**：WebDriver 的 cookie 指令、CDP 的 `Network.getCookies`、`setCookie`、`deleteCookies`、`clearBrowserCookies`，`DOMStorage` 和 `Storage` 做哪些。

### 測試

- 網站和 key：單元測試（public suffix 的邊界情況、ancestor chain bit），Browser 的 frame 樹測試（巢狀的 iframe 的 key）。
- Web Storage：WPT 的 `webstorage/`（54 份）；`browser` 的測試（兩個分頁的 `storage` 事件、第三方的 iframe 分開、重開之後還在）。
- cookie：`net` 的測試伺服器（`Set-Cookie`、redirect 的每一跳、`SameSite`）；WPT 的 `cookies/`（約 89 份）和 `fetch/api/cors/` 裡因為沒有 cookie 而失敗的；`document.cookie` 的 `browser` 測試；machopodriver 的 cookie 指令（WPT 的 wdspec）。
- 情境：`about:demo` 加一段記住設定的示範（改了之後重新載入還在）。
- 只能靠審查的：renderer 不能宣稱別人的 key（Browser 那一側的檢查，測試用壞掉的訊息打）。

### 分階段

2026-11-20 的看法，每個階段開始時再定那一段要決定的事：

1. **網站和 key**：public suffix list、`Site`、`StorageKey`，frame 樹在 commit 時算出每個 frame 的 key。看得到的只有 DevTools 的 `domainAndRegistry`。分三段：1a `site` crate 的 `Site` 和 registrable domain；1b `StorageKey`，frame 樹在 commit 時算；1c DevTools 的 `domainAndRegistry`、檢查和文件。
2. **profile 和磁碟**：資料夾、`--user-data-dir`、哪些模式只在記憶體、磁碟上的格式和它的讀寫。
3. **Web Storage**：同步等 Browser 的路、`localStorage`、`sessionStorage`、`storage` 事件、上限。分三段：3a 同步等 Browser 的路、Browser 的 storage thread 和資料庫、`Storage` 介面和 `localStorage`；3b `storage` 事件、`sessionStorage`；3c 上限、寫進磁碟的批次、WPT 的 `webstorage/`、檢查和文件。
4. **`net` 的 cookie**：請求的來歷、cookie 的儲存、`Set-Cookie` 和 `Cookie`、`SameSite`、持久。分五段（2026-11-20 從四段改成五段：第三方的 cookie 預設擋，要和接上請求同一段做）：4a 請求的來歷（Browser 給的、renderer 宣稱不了的最上層的網站和 site for cookies，核對 `initiator`）；4b 解析 `Set-Cookie`、cookie 本身（`CanonicalCookie`）和記憶體裡的 store；4c 接上每一跳請求（`Cookie`、`Set-Cookie`）、`SameSite`、第三方的 cookie 的預設和照送的旗標、導覽的請求的可信參數；4d `Partitioned`、上限和淘汰、寫進 profile 的 `Cookies` 資料庫；4e WPT 的 `cookies/`、檢查和文件。
5. **cookie 到 DOM 和自動化**：`document.cookie`、`navigator.cookieEnabled`、WebDriver、CDP。
6. **檢查和文件**。

## 缺點

- machopo 第一次有跨執行留下來的狀態：格式要能升級，壞掉的檔案要能處理，測試之間不能互相影響。
- renderer 的 main thread 第一次同步等 Browser：Browser 卡住時頁面跟著卡住，拆 process 時要一個同步的 IPC。
- 每個請求都要帶著來歷，renderer 送來的要核對：漏了一條路，cookie 就送錯地方。
- cookie 的規則多、而且各家不完全一樣（`SameSite` 的預設、第三方的 cookie），照 Chrome 做的那些之後會跟著 Chrome 變。
- public suffix list 會過期，要定期更新。

## 理由和其他做法

不做的話：登入不了任何網站，用 `localStorage` 的頁面壞掉；之後每種儲存各自決定「資料是誰的」。

- **只做基礎，不做功能**：key 和 profile 沒有被實際用過，設計對不對不知道。不選。
- **只做 Web Storage 或只做 cookie**：比較小，但 cookie 的 key（網域，加上分區）和儲存的 key 不一樣，只做一種驗證不到另一半（第 1 條）。
- 其他的比較在各條「要決定的事（依序）」定下來時寫進「已決定的事項」，完整的放 `details.md`。

## 先例

`details.md` 的「其他實作」：Chromium（`SchemefulSite`、`StorageKey`、`StoragePartition`、`CachedStorageArea` 和 `StorageArea`、`CookieMonster`、`RestrictedCookieManager`）。Firefox 和 WebKit 的做法還沒對著原始碼看，到用得到的階段再看。

## 已決定的事項

1. **範圍是基礎、Web Storage 和 cookie，分階段**（2026-11-20）：一份 RFC 定下網站、key、profile 和同步讀取的路，並用 Web Storage 和 cookie 兩種功能驗證。對照 Chromium：儲存用 `StorageKey`，cookie 照網域存、分區的另外帶最上層的網站，兩者都掛在 profile 的 `StoragePartition` 底下。不選只做 Web Storage、cookie 下一份再做：比較小，中途比較好插別的工作，但 cookie 那一半的 key 要到下一份才被實作驗證。不選只做 cookie：對網站的幫助最大，但驗證不到 `StorageKey`。
2. **儲存的 key 是 origin、最上層的網站和 ancestor chain bit**（2026-11-20）：嵌在別的網站底下的第三方 iframe 有自己的一份儲存，和它自己當最上層時的、和嵌在另一個網站底下時的都不是同一份；origin 和最上層同一個網站、但中間隔著別的網站的 frame 時（ancestor chain bit）也算第三方。沒有 nonce（machopo 沒有 credentialless 的 iframe）。Browser 在文件 commit 時沿著 frame 樹算一次。對照 Chromium：`blink::StorageKey` 開著第三方儲存分區時的欄位（`origin_`、`top_level_site_`、`ancestor_chain_bit_`；`details.md`），現在是預設。不選只看 origin（Chrome 舊的行為）：最簡單，但第三方的 iframe 能用共用的儲存跨網站認出使用者，之後要分開時已經存在磁碟上的資料要搬或丟；現在還沒有任何存下來的資料，定成分開不用搬。cookie 的分區和之後的 HTTP cache 也用得到最上層的網站。
3. **public suffix list 用 `psl` crate**（2026-11-20，第 1 階段）：清單由 crate 生成成程式碼，machopo 只呼叫它查 registrable domain，repo 裡不放清單；更新是升 crate 的版本（清單一變它就發版，`Cargo.lock` 釘住）。算網站時 private 的那一段也算（`github.io` 底下每個使用者一個網站），和 Chromium 的 `SchemefulSite` 用 `INCLUDE_PRIVATE_REGISTRIES` 一樣。2026-11-20 量的（Apple M4，`psl` 2.1.258）：release 第一次編 8 秒，執行檔多約 1 MB，不用在啟動時解析，查一次約 20 ns。對照 Chromium：自己的 repo 放一份 `effective_tld_names.dat`、編譯時轉成查詢用的結構，不靠外部的函式庫。不選 `publicsuffix` crate 加 repo 裡的清單檔（約 340 KB）：清單在自己手上、換清單不用等發版，但第一次用時解析約 11 ms、查一次約 66 ns，要多一個更新的腳本，crate 最後一版是 2024 年，而且既加依賴又要自己放清單。不選不加依賴、自己寫解析和查詢（最接近 Chromium）：規則不複雜（一般的後綴、萬用字元、例外，分 ICANN 和 private），但多一份要維護的程式，現成的已經做好。
4. **網站和 key 的型別在新的 crate `site`**（2026-11-20，第 1 階段）：`Site`、`StorageKey` 和算網站的函式放在一個小 crate，依賴 `url` 和 `psl`；`net`（之後的 cookie）、`browser`（frame 樹）、`renderer`（DevTools 的 `domainAndRegistry`）都用它，`psl` 只進這一個 crate。對照 Chromium：`SchemefulSite` 在 `//net/base`、`StorageKey` 在 `blink/public/common`，都是 browser 和 renderer 兩邊連結的共用程式；machopo 沒有這樣的共用層，這個 crate 是那個角色。不選放進 `protocol`：三方本來都依賴它，但它只定義資料（相當於 `.mojom`），現在連 `url` 都不依賴，要多放「從 origin 算網站」的邏輯和兩個依賴。不選照 Chromium 的位置拆到 `net` 和 `browser`：renderer 照 process 的邊界不依賴 `net`，DevTools 的欄位要由 Browser 算好送過去，renderer 自己 commit 的 `about:blank`、`srcdoc` 還要另外處理。
5. **人用的視窗預設把資料寫進平台的使用者資料夾，自動化和測試預設只在記憶體**（2026-11-20，第 2 階段）：`machopo [網址]` 的 profile 在 macOS 的 `~/Library/Application Support/machopo`、Linux 的 `$XDG_CONFIG_HOME/machopo`（沒設是 `~/.config/machopo`）、Windows 的 `%LOCALAPPDATA%\machopo\User Data`；`--automation-pipe`、headless、情境和測試的 profile 只在記憶體，結束就沒了，彼此不影響；任何一種給了 `--user-data-dir DIR` 就寫進那個資料夾。平台的資料夾直接從環境變數算（`HOME`、`XDG_CONFIG_HOME`、`LOCALAPPDATA`），不加依賴。對照 Chromium（它的 User Data Directory 文件、chromedriver 的 `chrome_launcher.cc`）：預設的位置同一種規則，`--user-data-dir` 換位置；chromedriver 沒指定時每個 session 一個暫時的資料夾（磁碟上的；這裡用記憶體，一樣每次從空的開始，不用清）。不選預設都不寫、給了 `--user-data-dir` 才寫：開發時每次開視窗都是乾淨的、家目錄不留檔案，但登入和設定預設留不住，和這份的目標不合。
6. **profile 的資料夾被另一個 machopo 開著時，第二個拒絕啟動**（2026-11-20，第 2 階段）：開 profile 時用作業系統的檔案鎖鎖住資料夾裡的一個檔案（標準庫的 `File::try_lock`，process 當掉時鎖自己放掉，不留過期的鎖；不加依賴、沒有 `unsafe`），拿不到就印出資料夾被用著、要同時開兩個請加 `--user-data-dir`，然後結束。只在記憶體的 profile 不用鎖。對照 Chromium：`ProcessSingleton` 讓一個 user data dir 只有一個 browser process，第二個把命令列交給第一個、由它開視窗，自己結束（Linux 用資料夾裡的 Unix socket，Windows 用看不見的視窗）。不選照 Chromium 把網址交給已經在跑的那個：用起來最順，但要一條兩個 process 之間的通道、三個平台各一種，已經在跑的那個要能收、能開分頁，是另一份工作（「之後的可能」）。不選第二個改用記憶體的 profile 照樣啟動：開發時同時開兩個不會被擋，但第二個的登入和設定留不住，只有 stderr 的一行警告。
7. **磁碟上的儲存用 SQLite**（2026-11-20，第 2 階段）：`rusqlite`，連同 SQLite 的原始碼一起編（`bundled`），設定照 Chromium 的 `sql::Database` 的預設：exclusive locking（打開它的 process 之外沒有人能開）、`journal_mode=TRUNCATE`（rollback journal）、`synchronous` 用 SQLite 預設的 FULL。（同一天改的：原本寫 WAL 模式，那是起草時自己加的細節；Chromium 的註解說 WAL 還在開發中、正式的功能還不用，預設是上面這組。）當掉、斷電時檔案不壞，有 transaction；cookie 要照網域查、找過期的，有 SQL；格式幾十年沒有不相容的改動，換版本不用搬資料。2026-11-20 量的（Apple M4，`rusqlite` 0.43）：release 第一次編 12.4 秒，執行檔多約 1.9 MB，一個 transaction 寫 1 萬筆 32 ms。machopo 本來就要 C 的編譯器（mimalloc 等），不多新的建置需求。對照 Chromium：cookie 存在 SQLite（`SQLitePersistentCookieStore`）；`localStorage`、`sessionStorage` 原本在 LevelDB，現在的程式有從 LevelDB 搬到 SQLite 的遷移（`DomStorageDatabase`），收斂到 SQLite；`localStorage` 成批地寫（預設等 5 秒，每小時最多 60 次）。不選 `redb`（純 Rust 的 key-value 資料庫，release 4.9 秒、多約 1.1 MB、1 萬筆 105 ms）：不碰 C，但只有 key-value，cookie 的查詢要自己建索引，檔案格式在大版本之間改過（5.0 是 2026-11-18 才發的），長期放在使用者電腦上的資料要搬。不選自己的檔案格式（每個 area 一個檔案、整份寫進暫存檔再改名）：不加依賴，但防當機、格式的升級、cookie 的查詢都要自己寫和測，HTTP cache 還要再設計一次。
8. **SQLite 包在新的 crate `sql`**（2026-11-20，第 2 階段）：包住 `rusqlite`（re-export 它，`rusqlite` 只是這個 crate 的依賴）：在 profile 的資料夾裡打開一個資料庫，只在記憶體的 profile 開在記憶體裡、走同一套程式；設定；schema 的版本和升級；壞掉的檔案。每種資料各開自己的檔案、在自己的 thread 上用自己的連線：cookie 在 `net` 的 network thread，Web Storage 在 Browser。對照 Chromium：`//sql` 是共用的包裝，每個功能各自的資料庫和 page cache（它的 `//sql` 文件寫的「full per-feature isolation」），cookie 是 `Cookies`、`localStorage` 是 `LocalStorage`。不選 `net` 和 `browser` 各自直接用 `rusqlite`：不多 crate，但開檔、設定、升級、記憶體和磁碟兩種 profile 要寫兩份。不選全部交給 `net`：SQLite 只在一處，但 Web Storage 和網路無關，它的讀寫和請求排在同一條 thread 上；Chromium 也放在另一個 storage service。

9. **renderer 同步等 Browser 的回答走一般的訊息，到 scheduler 裡等回答的位置**（2026-11-20，第 3 階段）：要同步問的訊息帶一個編號，Browser 的回答是一則帶同一個編號的 `RendererMsg`，`RendererSender` 不把它放進 Browser 的 queue，而是放進 main thread 的 scheduler 裡一個等回答用的位置；main thread 只在那個位置上等，其他 queue 的 task 不動、順序不變；renderer 和 Browser 之間斷了時等的那一方醒來、當成失敗。訊息照樣可以序列化：拆 process 時由讀 IPC 的 thread 照樣分到那個位置，呼叫的地方不用改。`document.cookie` 的同步讀之後也走這條路。對照 Chromium：Mojo 的 `[Sync]` 呼叫擋住呼叫的 thread，只等那條 pipe 上的回答（和允許的同步呼叫），其他的非同步訊息照順序留在 queue 裡；`CachedStorageArea` 第一次用時同步的 `GetAll` 就是這樣。不選回答進 Browser 的 queue、main thread 從裡面挑（worker 的 `importScripts` 的做法）：main thread 有六個 queue，挑出來放著的 task 要照原本的先後放回去，scheduler 要多一個通用的放回去（現在只有 DevTools 的），程式比較多也比較容易弄亂順序，沒有換到什麼。不選訊息裡帶一個回答用的 channel：現在最省事、scheduler 不用改，但不能序列化，是 process 邊界的例外，拆 process 時每個呼叫的地方都要重寫。

10. **renderer 照 Browser 給的 area 編號共用 `localStorage` 的那一份**（2026-11-20，第 3 階段）：文件第一次碰 `localStorage` 時同步送「打開」，只帶 frame 的 token；訊息照 IO thread 到 UI thread 的順序走，到的時候那份文件的 `DidCommitNavigation` 已經到了、frame 樹算好了 key。UI thread 查到 key 交給 storage thread，storage thread 回一個 area 的編號（每個 renderer、每個 key 一個），這個 renderer 還沒有這個 area 時順便帶上整份資料。renderer 照編號共用它的那一份（同一個分頁裡同 key 的文件寫了，另一份同步讀得到），之後的寫入帶著編號，從 IO thread 直接到 storage thread、不經過 UI thread。key 一直在 Browser 手上，renderer 宣稱不了別人的 key。對照 Chromium：也先在 UI thread 上經過 `RenderFrameHostImpl` 照 frame 的 key 打開 area，之後的讀寫直接走那條 pipe 到 storage service；renderer 的 `CachedStorageArea` 照 key 共用。不選 Browser 在 commit 時把 key 告訴 renderer、renderer 照 key 快取（Chromium 的 `commit_params` 的 `storage_key`）：renderer 已經有那個 area 時不用問，但 Browser 要在送 commit 之前就算好 key（現在是收到 `DidCommitNavigation` 才算），renderer 自己 commit 的文件（`about:blank`、`srcdoc`、`javascript:`）要自己算，Browser 還要核對 renderer 說的 key（Chromium 的 `CanAccessDataForOrigin`），只為了省下每份文件第一次用時的一次來回。

11. **`localStorage` 的 `storage` 事件都經過 Browser**（2026-11-20，第 3 階段）：renderer 改了自己的那一份、把改動送給 Browser 時，記下還沒確認的改動；Browser 把每個改動（舊值、新值、做出改動的文件）通知每個打開這個 area 的 renderer，包括送出它的那個；renderer 收到後對這個 area 的每一份文件發 `storage` 事件，只跳過做出改動的那一份；別的 renderer 的改動才套進自己的那一份，自己對同一個 key 還有沒確認的改動時先不套（那個改動之後會蓋過它）。對照 Chromium（2026-11-20 讀 `cached_storage_area.cc`）：`SetItem` 送 `Put`、記下 pending mutation，`KeyChanged` 回來時拿掉自己的、別人的才套用（`MaybeApplyNonLocalMutationForKey`），`EnqueueStorageEvent` 跳過來源的那一份，同一個 renderer 的文件也等這一趟。不選同一個 renderer 裡的文件由 renderer 自己馬上發、Browser 只通知別的 renderer：同一個分頁裡快一點、少一則訊息，但有兩條發事件的路，兩個分頁同時改同一個 key 時彼此看到的順序可能不同。

12. **`sessionStorage` 由 Browser 留在記憶體裡，每個分頁一個 namespace**（2026-11-20，第 3 階段）：打開的路和 `localStorage` 一樣（同步問，Browser 照 frame 的分頁找 namespace、照 frame 的 key 找 area），寫入照樣送給 Browser；`storage` 事件照 Chromium 在 renderer 裡當場排進（machopo 一個分頁一個 renderer，同一個分頁的文件都在裡面），不經過 Browser。renderer 當掉重開、頁面換 engine 時照樣在；分頁關掉時沒了；不寫磁碟。新分頁不複製（machopo 開新分頁只有連結的 `target=_blank`，等同 noopener，Chrome 這時也不複製；有 `window.open` 時再做）。對照 Chromium（2026-11-20 讀 `session_storage_impl.h`、`cached_storage_area.cc`）：每個 profile 一個 `SessionStorageImpl` 管所有 namespace，`BackingMode` 有只在記憶體、寫磁碟但開的時候清掉、寫磁碟並且還原（session restore）三種；`CachedStorageArea` 的 session storage 在 `SetItem` 時當場 `EnqueueStorageEvent`。不選只放在 renderer 的 DOM 裡：最簡單、不多任何訊息，但 renderer 當掉重開和換 engine 時同一個分頁的 `sessionStorage` 就不見了，跨 site 的 iframe 拆成別的 process 時也要重做。不選寫進磁碟：那是 session restore 用的，machopo 沒有，寫了沒有人讀。

13. **Web Storage 的上限由 renderer 和 Browser 都守**（2026-11-20，第 3 階段）：每個 area 10 MiB，key 和 value 每個字元算 2 bytes（UTF-16），只在項目變大時檢查。renderer 在 `setItem` 時算，超過丟 `QuotaExceededError`、項目不改；Browser 照同樣的算法、多容許 100 KiB（幾個 renderer 同時寫），超過時拒絕那個改動，告訴做出改動的 renderer 把那個 key 改回 Browser 的值。`localStorage` 和 `sessionStorage` 各算各的。對照 Chromium（2026-11-20 讀 `storage_area_map.cc`、`storage_area.mojom`）：`kPerStorageAreaQuota` 是 10485760、`kPerStorageAreaOverQuotaAllowance` 是 102400，`QuotaForString` 是字元數乘 `sizeof(UChar)`，`SetItemInternal` 只在新的項目比舊的大時檢查；Browser 拒絕時送 `KeyChangeFailed`，renderer 照它改回。不選只在 renderer 守：最簡單，但 Browser 信任 renderer（違反收的一方要驗證的規則），兩個 renderer 同時寫時總和可能超過。不選只在 Browser 守：每個 `setItem` 都要同步問 Browser 才知道要不要丟錯，每次寫入多一趟同步的來回。

14. **`localStorage` 成批寫進磁碟，照 Chromium 的延遲和速率限制**（2026-11-20，第 3 階段）：每個 area 的改動先放在一批裡，到期時在一個 transaction 裡寫進資料庫；等待的時間是三者中最長的：5 秒、每小時最多 60 次、每小時最多寫 10 MiB（從那個 area 第一次有改動起算）。`DomStorageContext` 結束時（machopo 關掉時）寫完還沒寫的；browser 當掉時，最後一批的改動會丟掉（和 Chrome 一樣）。延遲和速率可以由測試設定。打開 area 不受影響（項目在記憶體裡）。對照 Chromium（2026-11-20 讀 `local_storage_impl.cc`、`storage_area_impl.cc`）：`kCommitDefaultDelaySecs` 5 秒、`kMaxCommitsPerHour` 60、`kMaxBytesPerHour` 是 `kPerStorageAreaQuota`，`ComputeCommitDelay` 取預設的延遲和兩個 `RateLimiter` 的延遲中最大的，關閉時 `ScheduleImmediateCommit`。不選只有 5 秒的延遲、不限速率：少約五十行，但頁面一直寫時一小時最多寫 720 次。不選每個改動馬上寫：browser 當掉也不丟，但每個 `setItem` 都是一個 fsync 的 transaction，頁面連續寫一千次就是一千次磁碟同步，別的分頁第一次打開 storage 時要排在這些寫入後面同步等。

15. **cookie 自己照 RFC 6265bis 和 Chromium 寫**（2026-11-20，第 4 階段）：解析照 Chromium 的 `ParsedCookie`，cookie 照 `CanonicalCookie`，存取照 `CookieMonster`；public suffix 用 `site` crate，存檔用 `sql` crate，不多依賴。對照 Chromium：`//net/cookies` 是它自己的，不用外面的函式庫。不選 `cookie_store` crate 外面再包一層（2026-11-20 讀 0.22.1 的原始碼）：domain、path 的比對和到期現成，但沒有 `SameSite` 的判斷、`Partitioned`、`__Host-` / `__Secure-` 前綴、數量和大小的上限，存檔是整個 store 一次序列化（和逐筆寫 SQLite 對不上），還帶進第二份 public suffix list（`publicsuffix`，和 `site` 的 `psl` 可能判斷不同）。不選自己寫 store、用 `cookie` crate（0.18）解析 `Set-Cookie`：省下解析的約三百行，但它的規則和 RFC 6265bis、Chromium 在細節上不同（不合法的字元、`Max-Age` 和 `Expires` 的先後、大小的上限），WPT 的 `cookies/` 正好大量測這些。

16. **請求的來歷：每份文件一個 Browser 給的 loader 編號**（2026-11-20，第 4 階段）：Browser 在送 `CommitNavigation` 之前登記那份文件的 loader 的參數（文件的 origin、最上層的網站、site for cookies），給它一個編號，交給 IO thread，編號跟著 commit 送給 renderer；renderer 的子資源請求（`FetchResource`）帶著編號，IO thread 用編號查參數：`initiator` 和參數的 origin 對不上是壞掉的訊息，cookie 照參數決定。先登記、後送 commit，IO thread 收到新文件的第一個請求時一定查得到。renderer 自己 commit 的文件（`about:blank`、`srcdoc`、`javascript:`）和 dedicated worker 用建立它的文件的編號。對照 Chromium（2026-11-20 讀 `network_context.mojom`、`isolation_info.h`）：Browser 在 commit 時替每份文件建 `URLLoaderFactory`，`URLLoaderFactoryParams` 帶 `request_initiator_origin_lock` 和 `IsolationInfo`（frame 的 origin、最上層的 frame 的 origin、`SiteForCookies`），factory 跟著 commit 交給 renderer，network service 照它決定 cookie、核對請求的 `request_initiator`。不選 renderer 自己填最上層的網站和 site for cookies：最簡單，但被攻破的 renderer 可以宣稱自己是最上層的網站、拿到 `SameSite=Strict` 的 cookie，違反收的一方要驗證的規則。不選子資源的請求先繞到 UI thread、從 frame 樹拿參數：不用新的編號，但每個請求多一趟 UI thread（忙著畫畫面時載入跟著慢），也還是有 IO thread 比 UI thread 先收到新文件的請求的時序問題。

17. **第三方的 cookie 預設擋掉，加一個照送的旗標**（2026-11-20，第 4 階段）：預設不送、也不收沒標 `Partitioned` 的第三方 cookie（請求的 site for cookies 是 null，或不是請求的網址的網站），`Partitioned`（CHIPS）的照最上層的網站分開，照收照送；沒標 `SameSite` 的是 `Lax`，不做 2 分鐘的 Lax-allowing-unsafe。打開照送的旗標時整組照 Chrome 現在的預設：第三方的 cookie 照送照收，沒標 `SameSite` 的是 `Lax` 加 2 分鐘的 Lax-allowing-unsafe（最上層的跨網站導覽不管 method 都帶上剛建立的）。旗標在 `NetworkContextParams`，`machopo` 的命令列和 WPT 的 runner 都能打開。理由（2026-11-20 讀 RFC 6265bis 的 editor's draft，2026-11-17 版）：§7.1 不規定做法，但 RECOMMENDED 瀏覽器採用在相容性容許的範圍內盡量嚴格的第三方 cookie 政策；§5.8.1、§7.2 容許瀏覽器完全不送 `Cookie`、套用自己的 cookie 政策；沒標 `SameSite` 的預設等同 `Lax`，§5.6.7.2 的 Lax-allowing-unsafe 是容許（MAY）的相容性例外、SHOULD 只用在 2 分鐘內建立的。WPT 兩種假設都有：`cookies/samesite/` 的 `verifySameSiteCookieState` 要 `SameSite=None` 的跨網站照送，`cookies/third-party-cookies/` 的（`.tentative`）要預設擋；Chrome 預設跑時後者失敗（它的 `-expected.txt`），另外用 `--test-third-party-cookie-phaseout` 的 virtual suite 跑，machopo 反過來：預設擋，`samesite/` 用照送的旗標跑。對照 Chromium（2026-11-20 讀 `net/base/features.cc`、`cookie_constants.cc`）：`kForceThirdPartyCookieBlocking` 預設關、`kSameSiteDefaultChecksMethodRigorously` 預設關（有 Lax-allowing-unsafe，`kLaxAllowUnsafeMaxAge` 2 分鐘）、`kCookieSameSiteConsidersRedirectChain` 預設關。不選照 Chrome 的預設：網站相容性最好，但只用到規範容許的最寬鬆那一端，第三方的 cookie 能跨網站認出使用者，而 machopo 的 storage 已經照最上層的網站分開。不選預設擋、沒有旗標：靠第三方 cookie 的網站沒有辦法用，WPT 的 `samesite/` 有一部分只能記成失敗。不選照 Chrome 但不做 Lax-allowing-unsafe：比照 Chrome 少一個特例，但預設仍然照送第三方 cookie。兩種模式下 Lax-allowing-unsafe 的取捨：照送的模式整組照 Chrome（WPT 的 `samesite/` 照 Chrome 寫），預設的模式照規範的基本行為，各自一致。

## 待決定

「要決定的事（依序）」的第 7 到 12 個，每個階段開始前定那一段的。

## 之後的可能

- HTTP cache（照網路的分區 key，Chromium 的 `NetworkIsolationKey`）、IndexedDB、Cache Storage。
- blob URL 照 key 分。
- Cookie Store API、Storage Access API、`Clear-Site-Data`。
- 清除資料的 UI、無痕模式。
- 第二個 machopo 把網址交給已經開著同一個 profile 的那個（Chromium 的 `ProcessSingleton`，第 6 條）。
- cookie 的值在磁碟上加密（Chromium 用作業系統的金鑰）。
