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
4. **profile**：資料夾的位置和 `--user-data-dir`；哪些模式寫磁碟；同一個 profile 開第二個 machopo 時怎麼辦。
5. **磁碟上的格式**：SQLite（`rusqlite`，帶 C 的原始碼；Chromium 的 cookie 用它）、純 Rust 的嵌入式資料庫、或自己的檔案格式（一份 area 一個檔案、整份重寫）。要比當掉時會不會壞、寫入的成本、依賴。
6. **renderer 怎麼同步等 Browser**：訊息帶編號、main thread 在自己的 queue 上等那一則回答（其他 task 留著，和 worker 的 `importScripts` 一樣），或訊息帶一個回答用的 channel（不能序列化，是邊界的例外）。拆 process 時兩者都要換成同步的 IPC。
7. **Web Storage 的細節**：`storage` 事件怎麼到其他分頁、`sessionStorage` 的 namespace 跟著什麼（分頁；頁面開的新分頁要不要複製）、上限怎麼算。
8. **cookie 的實作**：自己照 RFC 6265bis 和 Chromium 的 `CanonicalCookie` 寫，或用 `cookie_store` crate（reqwest 的 `cookies` 用的）。看 WPT 的 `cookies/` 要的行為它做不做得到。
9. **請求的來歷**：`ResourceRequest` 加哪些欄位、誰填、Browser 怎麼核對 renderer 送來的。
10. **第三方的 cookie**：照 Chrome 現在的預設（沒標 `Partitioned` 的第三方 cookie 照送，`SameSite` 預設 `Lax`），或一律擋掉、只留 `Partitioned`（CHIPS）的。
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
3. **Web Storage**：同步等 Browser 的路、`localStorage`、`sessionStorage`、`storage` 事件、上限。
4. **`net` 的 cookie**：請求的來歷、cookie 的儲存、`Set-Cookie` 和 `Cookie`、`SameSite`、持久。
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

## 待決定

「要決定的事（依序）」的第 4 到 12 個，每個階段開始前定那一段的。

## 之後的可能

- HTTP cache（照網路的分區 key，Chromium 的 `NetworkIsolationKey`）、IndexedDB、Cache Storage。
- blob URL 照 key 分。
- Cookie Store API、Storage Access API、`Clear-Site-Data`。
- 清除資料的 UI、無痕模式。
- cookie 的值在磁碟上加密（Chromium 用作業系統的金鑰）。
