# RFC 0107 的進度：持久資料的 key，Web Storage 和 cookie

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-20。完成日期：2026-11-20（macOS，Apple M4）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：`site` crate：`Site`（HTML 的「obtain a site」）和 registrable domain（`psl`） | 完成 | 2026-11-20 |
| 1b：`StorageKey`，frame 樹在文件 commit 時算出每個 frame 的 key | 完成 | 2026-11-20 |
| 1c：DevTools 的 `domainAndRegistry`，第 1 階段的檢查和文件 | 完成 | 2026-11-20 |
| 2a：profile 的資料夾（位置、`--user-data-dir`、哪些模式寫磁碟、同一個資料夾開兩次） | 完成 | 2026-11-20 |
| 2b：磁碟上的儲存（格式、讀寫的介面） | 完成 | 2026-11-20 |
| 2c：第 2 階段的檢查和文件 | 完成 | 2026-11-20 |
| 3a：同步等 Browser 的路、Browser 的 storage thread 和資料庫、`Storage` 介面和 `localStorage` | 完成 | 2026-11-20 |
| 3b：`storage` 事件、`sessionStorage` | 完成 | 2026-11-20 |
| 3c：上限、寫進磁碟的批次、WPT 的 `webstorage/`、第 3 階段的檢查和文件 | 完成 | 2026-11-20 |
| 4a：請求的來歷（Browser 給的最上層的網站和 site for cookies，核對 `initiator`） | 完成 | 2026-11-20 |
| 4b：解析 `Set-Cookie`、cookie 本身（`CanonicalCookie`）、記憶體裡的 store | 完成 | 2026-11-20 |
| 4c：接上每一跳請求、`SameSite`、第三方的 cookie 的預設和照送的旗標、導覽的可信參數 | 完成 | 2026-11-20 |
| 4d：`Partitioned`、上限和淘汰、寫進 profile 的 `Cookies` 資料庫 | 完成 | 2026-11-20 |
| 4e：WPT 的 `cookies/`、第 4 階段的檢查和文件 | 完成 | 2026-11-20 |
| 5a：`document.cookie` 和 `navigator.cookieEnabled` | 完成 | 2026-11-20 |
| 5b：自動化（WebDriver 的 cookie 指令、CDP 的 `Network` 的 cookie、testdriver） | 完成 | 2026-11-20 |
| 5c：WPT 的 `cookies/`、第 5 階段的檢查和文件 | 完成 | 2026-11-20 |
| 6：檢查和文件 | 完成 | 2026-11-20 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 網站和 key | registrable domain：public suffix 加一個 label（萬用字元和例外、punycode）、private 的後綴和不認得的頂層網域也算、public suffix 本身、IP 位址、有空的 label 的沒有、最後的點留著 | `cargo test -p site` |
| 網站和 key | 網站：同一個 registrable domain 的 host 是同一個、不看 port、scheme 不同就不同、沒有 registrable domain 的 host 自己是網站、opaque 的 origin 只和自己相等 | `cargo test -p site` |
| 網站和 key | site for cookies：最上層的 frame 是自己的網站；frame 和中間的每一層都是最上層的網站時是它，有一層不是、或有 opaque 的時是 null；網址照 scheme 比（`ws:` 算 `http:`、`wss:` 算 `https:`） | `cargo test -p site` |
| 網站和 key | storage key：最上層的 frame 是第一方；別的網站的 frame 是第三方，嵌在不同的最上層網站底下各是一個 key；中間隔著別的網站的 frame 時是第三方；opaque 的 origin 和 opaque 的最上層是第三方 | `cargo test -p site` |
| 網站和 key | storage key 的序列化：照 Chromium 的 `StorageKey::Serialize`（origin 的網址、第三方的 `^0` 加最上層的網站或 `^31`），opaque 的 origin 和最上層的網站不寫 | `cargo test -p site` |
| 網站和 key | frame 樹的 key：文件 commit 時、子 frame 建立時（parent 的 origin）照它所在的 frame 算，最上層的網站是 main frame 的；重設之後是 opaque 的 | `cargo test -p browser --lib frame_tree::` |
| 網站和 key | DevTools 的 `domainAndRegistry`：frame 的網址的 host 的 registrable domain（private 的後綴也算），位址、public suffix 本身、單一 label、沒有 host 的是空的 | `cargo test -p renderer --lib page_agent::`；`cargo test -p shell --test it a_frames_domain_and_registry` |
| profile | profile：只在記憶體的沒有資料夾；資料夾沒有就建、鎖檔寫著是誰的；同時只給一個（clone 都放掉之後才放開），被用著是 `InUse`、訊息說資料夾和 `--user-data-dir`；建不出來的是錯誤；各平台預設的資料夾（空的、相對的 `XDG_CONFIG_HOME` 不算） | `cargo test -p browser --test it profile::` |
| profile | 哪個 profile：視窗預設是使用者的、自動化在記憶體、`--user-data-dir` 的照啟動時的工作目錄；第二個 machopo 開同一個資料夾時拒絕啟動，第一個照常 | `cargo test -p shell --bin machopo -- user_data`；`cargo test -p shell --test it a_second_machopo` |
| profile | 資料庫：記憶體和 profile 的資料夾，關掉再開還在；照 Chromium 的預設（`truncate` 的 journal、exclusive locking、被鎖住時馬上失敗）；schema 只跑新的 migration、較新的拒絕、失敗的退回；不是資料庫和 schema 壞掉的檔案移到旁邊、重新開始 | `cargo test -p sql` |
| Web Storage | 同步等 Browser：main thread 只拿自己的編號的回答，其他 task 照順序留著、別的呼叫的回答留給那個呼叫；Browser 說要結束或 main thread 不在了時放棄 | `cargo test -p renderer --lib task_queue` |
| Web Storage | `localStorage` 的 area：一個 renderer 拿一次項目、之後只拿編號，別的 renderer 拿得到改過的、照 key 的順序；不是自己的 area 的改動不算；每個 key 一份（別的 origin、別的最上層的網站、中間隔著別的網站）；opaque 的 origin 沒有；忘掉之後重新拿；profile 的資料夾留著（關掉時等寫完），opaque 的最上層的網站只在記憶體 | `cargo test -p browser --test it dom_storage::` |
| Web Storage | 頁面的 `localStorage`：成員（`length`、`key()`、`getItem` 等、named property 不蓋過成員、同一個物件）；另一個分頁讀得到、別的 origin 讀不到、opaque 的 origin 是 `SecurityError`；同一個 renderer 裡同 key 的文件共用、同步讀得到；不在 frame 裡的 window 是 null；profile 的資料夾留到下一次；換成 wasm 的 engine 再換回來之後照樣在 | `cargo test -p browser --test it dom_storage::`；`cargo test -p shell --test it local_storage_outlasts` |
| Web Storage | `storage` 事件經過 Browser：每個打開這個 area 的 renderer 都收到改動（舊值、有沒有改到），做出改動的那個帶著它的 `Storage` 的編號；`sessionStorage` 的改動不通知 | `cargo test -p browser --test it dom_storage::` |
| Web Storage | DOM 的 area：同一個 area 的 window 共用；改動在 Browser 傳回來之前算還沒確認，別的 renderer 在它之前的改動（同一個 key、或這裡的 clear 之前的任何改動）不套用，別人的 clear 留下這裡的 key；`storage` 事件排給那個 area 的每個 window、做出改動的除外，沒改到的不發；`sessionStorage` 的當場發；window 的 `storage` listener 要打開它的 area | `cargo test -p dom --test it storage::` |
| Web Storage | 頁面的 `storage` 事件和 `sessionStorage`：別的分頁收到 `localStorage` 的事件（key、新舊值、做出改動的頁面的網址、自己的 `storageArea`、trusted、不 bubble），做出改動的 window 沒有；同一個分頁裡只有 `<body onstorage>` 的 iframe 也收得到（`sessionStorage` 的先到）；`sessionStorage` 每個分頁一份（同一個分頁的下一頁有、別的分頁和之後開的分頁沒有、不發到別的分頁），分頁關掉時拿掉 | `cargo test -p browser --test it dom_storage::` |
| Web Storage | 上限：renderer 的（10 MiB，每個 UTF-16 code unit 算 2 bytes，只在項目變大時檢查；拿掉的、清掉的不再算；從 Browser 拿到的超過上限時改小照樣可以），超過丟 `QuotaExceededError`（code 22、是 `QuotaExceededError`）、不改，`localStorage` 和 `sessionStorage` 各算各的；Browser 的（多容許 100 KiB），超過時拒絕、只告訴做出改動的 renderer，renderer 改回 Browser 的值（後面還有這個 key 的改動時不改） | `cargo test -p dom --test it storage::`；`cargo test -p browser --test it dom_storage::` |
| Web Storage | 成批寫進磁碟：到期之前檔案不變，到期時一起寫，結束時寫完；延遲是 5 秒和兩個速率限制中最長的，速率從 area 第一次有改動起算 | `cargo test -p browser --lib dom_storage`；`cargo test -p browser --test it dom_storage::` |
| Web Storage | WPT：`webstorage/` 0 個意外的結果（`window.open`、`BroadcastChannel`、單獨的 surrogate 失敗；`set.window.js` 和 `storage_builtins.window.js` 失敗的項目和 Chrome 一樣）；`createEvent('StorageEvent')` | `python3 scripts/test.py wpt webstorage/ dom/nodes/Document-createEvent.https.html dom/events/EventTarget-dispatchEvent.html` |
| 請求的來歷 | 請求的來歷：每份文件 commit 時一個 Browser 給的 loader（文件的 origin、site for cookies、最上層的網站，照它所在的 frame 算）；IO thread 照編號核對（沒給的 loader、不是 worker 的 loader、`initiator` 不是 loader 的 origin、renderer 自己帶了可信的參數都是壞掉的訊息；opaque 的 `initiator` 照收），填上 loader 的可信參數；跨 origin 的 iframe 的圖片、iframe 一開始的 `about:blank` 的 `fetch()`、worker 的 `fetch()` 都經過各自的 loader | `cargo test -p browser --lib -- loader request_takes workers_request`；`cargo test -p browser --test it loaders::` |
| cookie | cookie：解析 `Set-Cookie`（終止字元和控制字元整行拒絕、tab 也是，名稱和值的大小、屬性值太長只略過、沒有 `=` 的第一段是沒有名稱的值、屬性不分大小寫、最後一個算）；`Expires` 的日期（Chromium 的 `ParseCookieExpirationTime`）；cookie 本身（host-only 和 domain cookie、public suffix 和 IP 位址、預設的 path、`Max-Age` 先於 `Expires`、伺服器的時差、最多 400 天、`__Secure-`、`__Host-`、`__Http-` 前綴、`Secure` 只從安全的或可信的網址、`SameSite=None` 要 `Secure`、`Partitioned` 要 `Secure`）；store（同名同 domain 同 path 的取代、值一樣時留著建立的時間、過期的刪掉、script 蓋不了 `HttpOnly`、leave secure cookies alone、domain 和 path 的比對、`SameSite` 的設定和送出（沒有 Lax-allowing-unsafe）、送出的順序）；預設的選項是 script 的、跨網站的 | `cargo test -p net --test it cookies::`；`cargo test -p net --lib cookies` |
| cookie | 請求的 cookie：每一跳送 jar 裡它拿得到的、留下回應設的（redirect 的也是）；沒有可信參數的（Browser 自己的）和 `omit` 的不送不留，`same-origin` 只到 initiator 的 origin；`SameSite` 照請求的情境（Chromium 的 `ComputeSameSiteContextForRequest`、`ForResponse`：別的網站發起的 main frame 導覽是 Lax、不安全的 method 只有 `None` 的，Browser 自己的導覽是 Strict，同一個網站的頁面裡別的網站發起的子資源是跨網站）；第三方（site for cookies 是別的網站或 null，不看 scheme）不送不留（`SameSite=None` 的也是）；導覽的可信參數（main frame 的是網址的網站，子 frame 的是 parent 的 loader 的，initiator 是發起的 frame 的 origin）；頁面設的 cookie 它的圖片送出、第三方的 iframe 設的不留 | `cargo test -p site`；`cargo test -p net --test it cookie_loads::`；`cargo test -p browser --lib -- navigations_params loaders_params`；`cargo test -p browser --test it cookies::` |
| cookie | `Partitioned`：分區是最上層的網站和中間有沒有隔著別的網站（main frame 的導覽是自己的網址的網站、同網站的），cookie 只跟著那個分區的請求，同名同 domain 同 path 的在不同分區是不同的 cookie；請求有分區時才是分區的、要 `Secure`，沒有時照沒標的；第三方的 `Partitioned` cookie 照送照收 | `cargo test -p net --test it -- cookies:: cookie_monster:: cookie_loads::`；`cargo test -p net --lib cookie_jar` |
| cookie | 上限和淘汰：一個 registrable domain 超過 180 筆時刪到 150（過期的先刪，再刪最久沒用的、不是 `Secure` 的在前），分區的 cookie 不算；全部超過 3300 筆時刪到 3000，最近 30 天用過的不刪，不是 `Secure` 的在前；分區的每個網域 180 筆、10 KiB；讀到的 cookie 隔一分鐘以上才記成用過 | `cargo test -p net --test it -- cookie_monster:: cookies::` |
| cookie | `Cookies` 資料庫：持久的 cookie 每個欄位存回來一樣（分區也是），session 的和 opaque 分區的不寫；改動成批寫（30 秒、512 筆、結束時），讀的時候刪掉過期的、略過不是 cookie 的列；開不了時只在記憶體；請求等資料庫讀完才帶 cookie；network context 換一個之後持久的還在，profile 的資料夾經過 `StoragePartition` 交給 `net` | `cargo test -p net --lib -- sqlite_persistent cookie_jar`；`cargo test -p net --test it cookie_loads::`；`cargo test -p browser --test it cookies::` |
| cookie | header：cookie 是 bytes（`Set-Cookie` 的值照 UTF-8 讀，`Cookie` 送 UTF-8 的 bytes）；回應裡不是 header 的那一行只丟掉那一行；`Content-Type` 是每個值裡最後一個能用的（`*/*` 不算，charset 留同一種的最後一個有的，引號裡的逗號不分開） | `cargo test -p net --test it -- cookie_loads::a_cookie_is_utf8_bytes load::the_last_usable`；`cargo test -p net --lib mime::` |
| script 的 cookie | `document.cookie`：讀的是 script 看得到的（沒有 `HttpOnly` 的），寫的跟著之後的請求送出、馬上讀得到，不能寫 `HttpOnly` 的或蓋過它；`about:blank` 的 iframe 用建立它的文件的網址（同一段 script 裡也是）；不在 browsing context 裡、拿出頁面的 frame 的、`file:` 的文件讀到空的、不丟錯；重開之後第一次讀也等資料庫；第三方照政策（預設只有 `Partitioned` 的） | `cargo test -p net --test it cookie_loads::`；`cargo test -p browser --test it cookies::` |
| script 的 cookie | `navigator.cookieEnabled`：第一方是 true，第三方的 iframe 是 false；cookie-averse 的文件是 true | `cargo test -p net --test it cookie_loads::`；`cargo test -p browser --test it cookies::` |
| script 的 cookie | script 的 cookie 經過文件的 loader：網址是 loader 的 origin 的才照它的可信參數；別的 origin、opaque 的 loader、沒給的 loader 是壞掉的訊息 | `cargo test -p browser --lib script_cookies` |
| 自動化 | cookie 的零件（DevTools、自動化的）：host-only 的是網址的 host，有點開頭的 domain 是 domain cookie；path 照給的或網址的預設；`Secure` 的當成從安全的網址設的；`Set-Cookie` 寫不出的拒絕（`;`、終止字元、前後的空白、名稱裡的 `=`、不在其中的網域、相對的 path、前綴、讀起來像一對的沒有名字的值、沒有 `Secure` 的分區） | `cargo test -p net --test it cookies::a_cookie_from` |
| 自動化 | Browser 的 cookie 管理：網址的 cookie 每種情境的、`HttpOnly` 的都算，含給的最上層 frame 的分區，每個一次，不記成用過；全部的；設的當成從自己的 `https:` 網址設的（`HttpOnly` 的也可以）；刪的照名稱、確切的 domain、path、分區 | `cargo test -p net --test it -- cookie_loads::the_browsers cookie_monster::an_all` |
| 自動化 | CDP：`Network.getCookies`（分頁的 frame 的網址、或給的）、`getAllCookies`、`setCookie`、`setCookies`（全部或一個都不設）、`deleteCookies`（沒給分區時只刪沒分區的）、`clearBrowserCookies`，`Storage.getCookies`、`setCookies`、`clearCookies`；`Network.Cookie` 的欄位（`expires`、`session`、`size`、`sameSite` 只在有時、`partitionKey`）；參數做不出 cookie 時是錯誤 | `cargo test -p shell --test it devtools_cookies::` |
| 自動化 | WebDriver：Get All、Get Named（`no such cookie`）、Add（目前的文件的網址，`sameSite` 不對是 `invalid argument`，網域不對或不是 `http:` 的文件是 `invalid cookie domain`）、Delete、Delete All Cookies；沒標 `SameSite` 的回報 `Lax`；WPT 的 wdspec 的 5 個目錄 | `cargo test -p shell --test it webdriver::cookies`；`python3 scripts/test.py wdspec /webdriver/tests/classic/add_cookie /webdriver/tests/classic/delete_all_cookies /webdriver/tests/classic/delete_cookie /webdriver/tests/classic/get_all_cookies /webdriver/tests/classic/get_named_cookie` |
| WPT 的 cookie | WPT：`cookies/` 整個目錄（89 份，43 份整份通過）、`fetch/api/cors/` 的 credentials、worker 和 dynamic import 的 credentials、`document-cookie.html`、`navigatorcookies-cookieenabled-true.html` 0 個意外的結果；runner 在每個測試之後用 machopodriver 的 `machopo/cookies/clear` 刪光 cookie（每個網站的，Delete All Cookies 只刪目前的文件的） | `python3 scripts/test.py wpt cookies/ fetch/api/cors/ workers/modules/ html/semantics/scripting-1/the-script-element/module/dynamic-import/ html/dom/documents/resource-metadata-management/ html/webappapis/system-state-and-capabilities/the-navigator-object/`；`cargo test -p shell --test it webdriver::clearing_cookies` |

## 做過的檢查

macOS、Apple M4，2026-11-20。

### 測試和 mutation test

- 新的 crate `site`、`sql` 各有自己的整合測試；`net` 的 `cookies`、`cookie_monster`、`cookie_loads`，`browser` 的 `profile`、`dom_storage`、`loaders`、`cookies`，`dom` 的 `storage`，`shell` 的 `devtools_cookies` 是新的模組，`webdriver`、`load`、`devtools`、`automation_pipe` 和幾個 crate 的單元測試加了測試。
- 每一段都照 `docs/testing.md` 跑了 mutation test（`python3 scripts/test.py mutate`，spec 是這個資料夾的 `mutants-*.toml`），共 258 個：等價的 3 個（`sql` 打開時不讀 schema、opaque 的文件也去問 `localStorage` 的 host、已經過期的 cookie 也存進去），只有送了不是自己的 frame 的 renderer 走得到的 1 個（不在的 frame 不回答），都記在 `docs/test-gaps.md`，其他都抓到。第一輪沒抓到的都補了測試或改了程式，主要是：
  - 測試的值選得剛好讓 mutant 也對：同時到的兩個回答（`HashMap` 的順序剛好拿對，改成別的回答先到）、thread 結束前已經寫完、iframe 比 parent 先打開、改小之後剛好又在上限內、只在 IP 位址上測網域 cookie、存的那一列不是 `Secure` 被別的檢查擋掉、淘汰的第二輪沒有要刪的、提早淘汰時沒有東西可刪。
  - 互相涵蓋、多餘的檢查：opaque 的 origin 和 opaque 的最上層網站、`is_fully_active` 本來就看 window、零件的名稱的 `=`（`check_name_value_pair` 已經擋）、main frame 的導覽的分區（`CookiePartitionKey::for_request` 拿掉了那個參數）；拿掉多的、mutant 改成弄壞剩下的。
  - 外層的提前離開和內層的檢查一樣（淘汰的兩層上限）：一次弄壞兩層。
- 只靠審查的：opaque 的文件的 `document.cookie` 丟 `SecurityError`、Browser 沒回答時讀到空的（`docs/test-gaps.md`）。沒有計數守著的優化：jar 的淘汰等最早的使用時間（同一份）。
- `net` 的 cookie 的第一版（4b：`crates/net/src/cookies/` 的九個檔案和它的測試、`mutants-4b.toml`）由 subagent 照 Chromium 的 `parsed_cookie.cc`、`canonical_cookie.cc`、`cookie_util.cc` 和 RFC 6265bis 寫，在 session 裡對照 Chromium 審過、自己重跑了測試和 mutation test；審查時把 `CookieOptions` 的預設改成不做 Lax-allowing-unsafe。之後的實作照使用者的要求都在 session 裡寫。
- 拿掉照送第三方 cookie 的旗標時（第 24 條），`mutants-4b.toml`、`mutants-4c.toml`、`mutants-5a.toml` 裡改的是被拿掉的程式的 mutant 照原樣留著，是當時的紀錄；留下來的規則由 `mutants-5-no-third-party.toml` 重新守住。

### 找到的 bug

- 圖片的請求的 `initiator` 一直是最上層的文件的 origin（iframe 裡的也是），照新的核對會讓 renderer 被結束；改成圖片所在的文件的 origin。文件在 commit 的途中就 parse、裡面的 script 可能馬上發請求，所以文件的 loader 在建文件時就登記（4a）。
- `browser` 的測試的分頁沒有 worker 的 engine，worker 不會跑；改成 V8 的（4a）。測試的分頁原本都是 `TabId(0)`，兩個分頁共用 `sessionStorage`；改成從計數器拿（3b）。
- `initStorageEvent` 的 `storageArea` 參數讓 renderer panic（`dom::Storage` 沒有當參數用的轉換），同一個 machopo 裡的測試跟著 CRASH（3c，WPT 找到）。
- 新 iframe 的第一份 `about:blank` 在建立它的 script 還在跑時沒有 loader；照 Chromium，經過建立者的文件問。script 寫的 cookie 原本在 network thread 的另一個 task 做，和之後的 load 的順序沒有保證；改成在主迴圈裡當場做（5a）。
- `net` 的（5c，跑整個 WPT 的 `cookies/` 時找到，都不是 cookie 自己的邏輯）：`Set-Cookie` 的值照 Latin-1 存，script 讀到亂碼；回應裡有一行壞掉的 header 時 hyper 讓整個回應失敗；`Content-Type` 取第一個 header（`postToParent.py` 送 `application/json` 再送 `text/html`，iframe 的 script 不跑、測試 timeout）。
- 起草時寫了 SQLite 的 WAL 模式，Chromium 的 `sql::Database` 其實是 exclusive locking、`journal_mode=TRUNCATE`；照它改了第 7 條（2b）。

### WPT

- 加進清單的：`webstorage/`（3c）、整個 `cookies/`（5c，取代第 4、5 階段先列的 6 份），wdspec 的 `add_cookie`、`delete_all_cookies`、`delete_cookie`、`get_all_cookies`、`get_named_cookie`（5b）。沒過的寫在 `.ini`：`window.open`（`samesite/` 幾乎每份、`webstorage/` 的 7 份）、`BroadcastChannel`、`WebSocket`、service worker、`self.origin`（`cookies/prefix/` 的 6 份去抓 `undefined/...`）、單獨的 surrogate、照 Chrome 的行為（`webstorage/set.window.js` 等和 Chrome 失敗的一樣；cookie 的 tab、沒有名稱而值裡有 `=` 的、不綁 port）、擋掉的第三方 cookie、user prompt、沒標 `SameSite` 的回報 `Lax`（chromedriver 一樣，WPT 期待 `None`）。
- 變成通過的（拿掉或改了 `.ini`）：`fetch/api/cors/cors-cookies*`、worker 和 dynamic import 的 credentials、`navigatorcookies-cookieenabled-true.html`、`Document-createEvent.https.html` 的 `StorageEvent`、`workers/semantics/navigation/001.html`（之前就通過、預期結果沒更新）。
- 試跑沒列的：`fetch/content-type/`（renderer 那一側的 `Response` 和 blob 的 type、script 的 MIME type 還有失敗）。
- runner 的改變（第 25 條）：結果是預期的時候 wptrunner 不重開瀏覽器，在 `window.open` 停下、沒清 cookie 的測試把 cookie 留給之後的測試；machopo 的 executor 在每個測試之後用 machopodriver 的 `machopo/cookies/clear` 刪光。

### 不穩定的測試

- `moving-between-documents/after-prepare-*` 和 `fetch/`、`workers/` 一起跑時偶爾整份 timeout（4e）：和 cookie 無關，原因是 wptserve 關掉 keep-alive 的連線不說；另一個 session 照 Chromium 的 `ShouldResendRequest` 在 `net` 重送，套進這個 branch（`835e4ff`），之後同樣的目錄跑兩次都沒有意外的結果。
- `cookies/partitioned-cookies/partitioned-cookies-top-level-redirect.tentative`：頁面載入時把自己導覽走再導回來，machopodriver 的 Navigate 在第一份文件載入完就回答（chromedriver 等頁面自己開始的導覽也完成），harness 的 script 有時跑在要被換掉的文件裡。和 cookie 無關，`.ini` 標成 `[OK, ERROR]`。
- `scenarios --window`：3a 時 `demo-script` 失敗 1 次（log 裡 Viz 一直是 `occluded`，視窗被蓋住）；5c 時整套跑兩次各 1 個失敗，第二次是 `demo-busy` 的「no frame was drawn」（第一次的沒有記下來），單獨跑 3 次都通過。錯誤和視窗被遮住時一樣，`demo-busy` 的那次原因沒有確認。
- 另一個 worktree 和這裡共用 `target/` 時，cargo 用 workspace 裡的相對路徑命名輸出，別的 branch 編的 crate 蓋掉了這裡的（4e）：touch 改過的 crate 之後重編。

### 跑過的檢查

- 每一段：改到的 crate 的 `cargo test`，`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；改了 WPT 的清單或預期結果、或 `net` 的載入時跑相關目錄的 `test.py wpt`，wdspec 的 5 個 cookie 目錄跑了 `test.py wdspec`。
- 每個階段做完時：`cargo test --workspace`（第 1 階段 4285 個、第 2 階段 4298 個、第 3 階段 4327 個、第 4 階段 4416 個、第 5 階段 4437 個通過，都是 1 個 ignored）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`。clippy 找到的都改了寫法：測試裡太長的型別、分頁的編號用 `base::AtomicSequenceNumber`（3c）、`mime.rs` 的 `i + 1` 加帶理由的 `#[expect]`（5c）。跑完之後家目錄底下沒有 profile 的資料夾（測試和自動化都在記憶體）。
- 真視窗的情境（`scenarios --window`，改了視窗那一層的 2a、3a、4c、拿掉旗標之後）：72 次；失敗的見上面的「不穩定的測試」。看了截圖。
- 沒有動到 GC、V8 的程式和 fork：`matrix`、`fuzz` 照 CLAUDE.md 由每週的 CI 跑。
