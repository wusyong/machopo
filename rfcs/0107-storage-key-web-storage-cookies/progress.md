# RFC 0107 的進度：持久資料的 key，Web Storage 和 cookie

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-20

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
| 4e：WPT 的 `cookies/`、第 4 階段的檢查和文件 | 未開始 | |
| 5：cookie 到 DOM 和自動化 | 未開始 | |
| 6：檢查和文件 | 未開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| registrable domain：public suffix 加一個 label（萬用字元和例外、punycode）、private 的後綴和不認得的頂層網域也算、public suffix 本身、IP 位址、有空的 label 的沒有、最後的點留著 | `cargo test -p site` |
| 網站：同一個 registrable domain 的 host 是同一個、不看 port、scheme 不同就不同、沒有 registrable domain 的 host 自己是網站、opaque 的 origin 只和自己相等 | `cargo test -p site` |
| storage key：最上層的 frame 是第一方；別的網站的 frame 是第三方，嵌在不同的最上層網站底下各是一個 key；中間隔著別的網站的 frame 時是第三方；opaque 的 origin 和 opaque 的最上層是第三方 | `cargo test -p site` |
| DevTools 的 `domainAndRegistry`：frame 的網址的 host 的 registrable domain（private 的後綴也算），位址、public suffix 本身、單一 label、沒有 host 的是空的 | `cargo test -p renderer --lib page_agent::`；`cargo test -p shell --test it a_frames_domain_and_registry` |
| profile：只在記憶體的沒有資料夾；資料夾沒有就建、鎖檔寫著是誰的；同時只給一個（clone 都放掉之後才放開），被用著是 `InUse`、訊息說資料夾和 `--user-data-dir`；建不出來的是錯誤；各平台預設的資料夾（空的、相對的 `XDG_CONFIG_HOME` 不算） | `cargo test -p browser --test it profile::` |
| 哪個 profile：視窗預設是使用者的、自動化在記憶體、`--user-data-dir` 的照啟動時的工作目錄；第二個 machopo 開同一個資料夾時拒絕啟動，第一個照常 | `cargo test -p shell --bin machopo -- user_data`；`cargo test -p shell --test it a_second_machopo` |
| 資料庫：記憶體和 profile 的資料夾，關掉再開還在；照 Chromium 的預設（`truncate` 的 journal、exclusive locking、被鎖住時馬上失敗）；schema 只跑新的 migration、較新的拒絕、失敗的退回；不是資料庫和 schema 壞掉的檔案移到旁邊、重新開始 | `cargo test -p sql` |
| frame 樹的 key：文件 commit 時、子 frame 建立時（parent 的 origin）照它所在的 frame 算，最上層的網站是 main frame 的；重設之後是 opaque 的 | `cargo test -p browser --lib frame_tree::` |
| 同步等 Browser：main thread 只拿自己的編號的回答，其他 task 照順序留著、別的呼叫的回答留給那個呼叫；Browser 說要結束或 main thread 不在了時放棄 | `cargo test -p renderer --lib task_queue` |
| storage key 的序列化：照 Chromium 的 `StorageKey::Serialize`（origin 的網址、第三方的 `^0` 加最上層的網站或 `^31`），opaque 的 origin 和最上層的網站不寫 | `cargo test -p site` |
| `localStorage` 的 area：一個 renderer 拿一次項目、之後只拿編號，別的 renderer 拿得到改過的、照 key 的順序；不是自己的 area 的改動不算；每個 key 一份（別的 origin、別的最上層的網站、中間隔著別的網站）；opaque 的 origin 沒有；忘掉之後重新拿；profile 的資料夾留著（關掉時等寫完），opaque 的最上層的網站只在記憶體 | `cargo test -p browser --test it dom_storage::` |
| 頁面的 `localStorage`：成員（`length`、`key()`、`getItem` 等、named property 不蓋過成員、同一個物件）；另一個分頁讀得到、別的 origin 讀不到、opaque 的 origin 是 `SecurityError`；同一個 renderer 裡同 key 的文件共用、同步讀得到；不在 frame 裡的 window 是 null；profile 的資料夾留到下一次；換成 wasm 的 engine 再換回來之後照樣在 | `cargo test -p browser --test it dom_storage::`；`cargo test -p shell --test it local_storage_outlasts` |
| `storage` 事件經過 Browser：每個打開這個 area 的 renderer 都收到改動（舊值、有沒有改到），做出改動的那個帶著它的 `Storage` 的編號；`sessionStorage` 的改動不通知 | `cargo test -p browser --test it dom_storage::` |
| DOM 的 area：同一個 area 的 window 共用；改動在 Browser 傳回來之前算還沒確認，別的 renderer 在它之前的改動（同一個 key、或這裡的 clear 之前的任何改動）不套用，別人的 clear 留下這裡的 key；`storage` 事件排給那個 area 的每個 window、做出改動的除外，沒改到的不發；`sessionStorage` 的當場發；window 的 `storage` listener 要打開它的 area | `cargo test -p dom --test it storage::` |
| 頁面的 `storage` 事件和 `sessionStorage`：別的分頁收到 `localStorage` 的事件（key、新舊值、做出改動的頁面的網址、自己的 `storageArea`、trusted、不 bubble），做出改動的 window 沒有；同一個分頁裡只有 `<body onstorage>` 的 iframe 也收得到（`sessionStorage` 的先到）；`sessionStorage` 每個分頁一份（同一個分頁的下一頁有、別的分頁和之後開的分頁沒有、不發到別的分頁），分頁關掉時拿掉 | `cargo test -p browser --test it dom_storage::` |
| 上限：renderer 的（10 MiB，每個 UTF-16 code unit 算 2 bytes，只在項目變大時檢查；拿掉的、清掉的不再算；從 Browser 拿到的超過上限時改小照樣可以），超過丟 `QuotaExceededError`（code 22、是 `QuotaExceededError`）、不改，`localStorage` 和 `sessionStorage` 各算各的；Browser 的（多容許 100 KiB），超過時拒絕、只告訴做出改動的 renderer，renderer 改回 Browser 的值（後面還有這個 key 的改動時不改） | `cargo test -p dom --test it storage::`；`cargo test -p browser --test it dom_storage::` |
| 成批寫進磁碟：到期之前檔案不變，到期時一起寫，結束時寫完；延遲是 5 秒和兩個速率限制中最長的，速率從 area 第一次有改動起算 | `cargo test -p browser --lib dom_storage`；`cargo test -p browser --test it dom_storage::` |
| WPT：`webstorage/` 0 個意外的結果（`window.open`、`BroadcastChannel`、單獨的 surrogate 失敗；`set.window.js` 和 `storage_builtins.window.js` 失敗的項目和 Chrome 一樣）；`createEvent('StorageEvent')` | `python3 scripts/test.py wpt webstorage/ dom/nodes/Document-createEvent.https.html dom/events/EventTarget-dispatchEvent.html` |
| site for cookies：最上層的 frame 是自己的網站；frame 和中間的每一層都是最上層的網站時是它，有一層不是、或有 opaque 的時是 null；網址照 scheme 比（`ws:` 算 `http:`、`wss:` 算 `https:`） | `cargo test -p site` |
| 請求的來歷：每份文件 commit 時一個 Browser 給的 loader（文件的 origin、site for cookies、最上層的網站，照它所在的 frame 算）；IO thread 照編號核對（沒給的 loader、不是 worker 的 loader、`initiator` 不是 loader 的 origin、renderer 自己帶了可信的參數都是壞掉的訊息；opaque 的 `initiator` 照收），填上 loader 的可信參數；跨 origin 的 iframe 的圖片、iframe 一開始的 `about:blank` 的 `fetch()`、worker 的 `fetch()` 都經過各自的 loader | `cargo test -p browser --lib -- loader request_takes workers_request`；`cargo test -p browser --test it loaders::` |
| cookie：解析 `Set-Cookie`（終止字元和控制字元整行拒絕、tab 也是，名稱和值的大小、屬性值太長只略過、沒有 `=` 的第一段是沒有名稱的值、屬性不分大小寫、最後一個算）；`Expires` 的日期（Chromium 的 `ParseCookieExpirationTime`）；cookie 本身（host-only 和 domain cookie、public suffix 和 IP 位址、預設的 path、`Max-Age` 先於 `Expires`、伺服器的時差、最多 400 天、`__Secure-`、`__Host-`、`__Http-` 前綴、`Secure` 只從安全的或可信的網址、`SameSite=None` 要 `Secure`、`Partitioned` 要 `Secure`）；store（同名同 domain 同 path 的取代、值一樣時留著建立的時間、過期的刪掉、script 蓋不了 `HttpOnly`、leave secure cookies alone、domain 和 path 的比對、`SameSite` 的設定和送出、Lax-allowing-unsafe 的 2 分鐘、送出的順序）；預設的選項不做 Lax-allowing-unsafe | `cargo test -p net --test it cookies::`；`cargo test -p net --lib cookies` |
| 請求的 cookie：每一跳送 jar 裡它拿得到的、留下回應設的（redirect 的也是）；沒有可信參數的（Browser 自己的）和 `omit` 的不送不留，`same-origin` 只到 initiator 的 origin；`SameSite` 照請求的情境（Chromium 的 `ComputeSameSiteContextForRequest`、`ForResponse`：別的網站發起的 main frame 導覽是 Lax、不安全的 method 只有 `None` 的，Browser 自己的導覽是 Strict，同一個網站的頁面裡別的網站發起的子資源是跨網站）；第三方（site for cookies 是別的網站或 null，不看 scheme）預設不送不留，`--allow-third-party-cookies` 照 Chromium（`SameSite=None` 的送、留，Lax-allowing-unsafe 的 2 分鐘）；導覽的可信參數（main frame 的是網址的網站，子 frame 的是 parent 的 loader 的，initiator 是發起的 frame 的 origin）；頁面設的 cookie 它的圖片送出、第三方的 iframe 設的只在照送的旗標時留下 | `cargo test -p site`；`cargo test -p net --test it cookie_loads::`；`cargo test -p browser --lib -- navigations_params loaders_params`；`cargo test -p browser --test it cookies::`；`cargo test -p shell --bin machopo -- third_party_cookie` |
| `Partitioned`：分區是最上層的網站和中間有沒有隔著別的網站（main frame 的導覽是自己的網址的網站、同網站的），cookie 只跟著那個分區的請求，同名同 domain 同 path 的在不同分區是不同的 cookie；請求有分區時才是分區的、要 `Secure`，沒有時照沒標的；第三方的 `Partitioned` cookie 兩種政策都照送照收 | `cargo test -p net --test it -- cookies:: cookie_monster:: cookie_loads::`；`cargo test -p net --lib cookie_jar` |
| 上限和淘汰：一個 registrable domain 超過 180 筆時刪到 150（過期的先刪，再刪最久沒用的、不是 `Secure` 的在前），分區的 cookie 不算；全部超過 3300 筆時刪到 3000，最近 30 天用過的不刪，不是 `Secure` 的在前；分區的每個網域 180 筆、10 KiB；讀到的 cookie 隔一分鐘以上才記成用過 | `cargo test -p net --test it -- cookie_monster:: cookies::` |
| `Cookies` 資料庫：持久的 cookie 每個欄位存回來一樣（分區也是），session 的和 opaque 分區的不寫；改動成批寫（30 秒、512 筆、結束時），讀的時候刪掉過期的、略過不是 cookie 的列；開不了時只在記憶體；請求等資料庫讀完才帶 cookie；network context 換一個之後持久的還在，profile 的資料夾經過 `StoragePartition` 交給 `net` | `cargo test -p net --lib -- sqlite_persistent cookie_jar`；`cargo test -p net --test it cookie_loads::`；`cargo test -p browser --test it cookies::` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a：`site` crate 的 `Site` 和 registrable domain

macOS、Apple M4，2026-11-20。

- 決定了第 3 條（`psl` crate）和第 4 條（新的 crate `site`）。量了兩個 crate（README 的第 3 條），讀了 Chromium 的 `SchemefulSite::ObtainASite`（opaque 的照原樣；有網路 host 的 scheme 才查 registrable domain：http、https、ftp、ws、wss；沒有的用 host）和 `GetDomainAndRegistryImpl`（`INCLUDE_UNKNOWN_REGISTRIES`：不認得的頂層網域照樣當後綴）。
- 試了 `psl` 2.1.258 的邊界：punycode 的後綴、萬用字元和例外都對；它不轉小寫、把 IP 位址的數字當 label（`127.0.0.1` 的「網域」是 `0.1`）、有空的 label 時給 `.com` 這樣的結果、最後的點留著（和 Chromium 一樣）。所以 `registrable_domain` 收 `url::Host`（位址直接沒有），自己擋空的 label。
- 新的 crate `site`：`registrable_domain`、`Site`（`of`、`of_url`、`is_opaque`、`scheme`、`registrable_domain_or_host`、`serialize`）。opaque 的網站拿著那個 `url::OpaqueOrigin`，只和自己相等。WebSocket 的 scheme 不換成 http、https（Chromium 先換；machopo 還沒有 WebSocket），沒有測試釘住。
- 新的測試：`crates/site/tests/it/site.rs` 的 8 個。
- mutation test（`mutants-1a.toml`）7 個，全部抓到。
- 文件：`crates/site/README.md`、`docs/architecture.md` 的 crate 表。行為還沒有變（還沒有人用這個 crate），沒有新的限制和情境。
- 跑過的檢查：`cargo test -p site`、`cargo clippy -p site --all-targets -- -D warnings`；`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`docs`。

### 1b：`StorageKey`，frame 樹算每個 frame 的 key

macOS、Apple M4，2026-11-20。

- 讀了 Chromium 的 `RenderFrameHostImpl::CalculateStorageKey`、`GetAncestorChainForStorageKeyCalculation`（從這個 frame 往上到最上層）和 `StorageKey` 的建構子：最上層的網站是最上層 frame 的 origin 的網站；這個 frame 和每個祖先都是那個網站時 ancestor chain bit 是 `kSameSite`；origin 或最上層的網站是 opaque 時是 `kCrossSite`。
- `site`：`StorageKey`（`of_frame`、`first_party`、`origin`、`top_level_site`、`ancestor_chain_bit`、`is_third_party`）和 `AncestorChainBit`。
- `browser`：`FrameTreeNode` 加上 `storage_key`，在 `did_commit`（新的 origin）、`add_child`（最初的 `about:blank` 拿 parent 的 origin）和 `reset`（opaque）時沿著 parent 往上算；`FrameTree::storage_key`。frame 換文件時它裡面的 frame 跟著不在，所以祖先的 origin 在子 frame 有文件的期間不變，不用重算。還沒有人用它。
- 新的測試：`site` 的 `storage_key.rs` 4 個；`browser` 的 `frame_tree` 單元測試 1 個（main frame、同網站和別的網站的子 frame、中間隔著別的網站的孫 frame、重設）。
- mutation test（`mutants-1b.toml`）8 個，全部抓到。第一輪 9 個抓到 6 個：「opaque 的 origin」和「opaque 的最上層網站」兩個判斷互相涵蓋（opaque 的網站只和自己相等，只有 opaque 的最上層 frame 自己要靠判斷擋），拿掉一個、加上註解，mutant 改成拿掉剩下那個；「只看 parent」時孫 frame 照樣是第三方、但最上層的網站錯了，測試補上檢查最上層的網站。
- 跑過的檢查：`cargo test -p browser -p site`（187 個）、`cargo clippy -p site -p browser --all-targets -- -D warnings`；`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`boundary`、`docs`。

### 1c：DevTools 的 `domainAndRegistry`，第 1 階段的檢查

macOS、Apple M4，2026-11-20。

- 讀了 Blink 的 `InspectorPageAgent`：`setDomainAndRegistry(GetDomainAndRegistry(url.Host(), kIncludePrivateRegistries))`，用 frame 的網址（不是 origin）的 host。
- `renderer` 的 `page_agent`：`domain_and_registry`（`site::registrable_domain`，位址和沒有 host 的是空的），取代寫死的空字串；`renderer` 依賴 `site`。
- 新的測試：`page_agent` 的單元測試 1 個（網域、private 的後綴、不認得的頂層網域、位址、`localhost`、public suffix 本身、`about:`、`file:`、不是網址的）；`shell` 的 `devtools` 1 個（`--host-resolver-rules` 把 `www.example.co.uk` 對到測試伺服器，`Page.getFrameTree` 給 `example.co.uk`）。
- mutation test（`mutants-1c.toml`）3 個，全部抓到。
- 文件：`docs/limitations.md` 拿掉 `domainAndRegistry` 一律是空的；IntersectionObserver 那一條原因裡的「沒有 public suffix list 判斷 site」改成「判斷遮擋時不分 site」（還是不分）。
- 第 1 階段的檢查：`cargo test --workspace` 4285 個通過、0 個失敗、1 個 ignored；`cargo clippy --workspace --all-targets -- -D warnings` 沒有警告；`cargo fmt --check` 抓到 1c 的 DevTools 測試的一行，照 rustfmt 改了。畫面上看不出差別（只有 DevTools 的欄位），沒有新的情境。
- 跑過的檢查：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。

### 2a：profile 的資料夾

macOS、Apple M4，2026-11-20。

- 決定了第 5 條（人用的視窗預設寫進平台的使用者資料夾，自動化和測試在記憶體）和第 6 條（第二個 machopo 拒絕啟動）。對照了 Chromium 的 User Data Directory 文件（各平台的位置、`--user-data-dir`、Linux 的 `XDG_CONFIG_HOME`）、chromedriver 的 `chrome_launcher.cc`（沒給 `--user-data-dir` 時每個 session 一個暫時的資料夾）和 `ProcessSingleton`。
- `browser`：`Profile`（`in_memory`、`open`、`dir`；`Clone`，資料夾的鎖跟著最後一個放掉：Chromium 的 `Profile` 比它的視窗活得久，之後存資料的 thread 也拿著它），鎖是標準庫的 `File::try_lock`，鎖檔寫著 process 的 id（Windows 上鎖著的檔案可能讀不到，訊息就不帶 id）；`ProfileError`；`default_user_data_dir`、`user_data_dir_on`（從環境變數算，沒有加依賴）、`Platform`。
- `shell`：`machopo --user-data-dir DIR`（說明和 usage），`profile_dir`（選資料夾）和 `profile`（打開；拿不到就印出原因、結束），`window::run` 的參數收成 `window::Options`（clippy 的參數上限），`browser_window::Options` 和 `automation_pipe::Options` 帶 profile，`WindowTabs` 拿著它。還沒有東西存進去。
- 新的測試：`browser` 的 `profile.rs` 5 個；`shell` 的 `main` 1 個（選項和選資料夾）、`help_mentions_every_option` 加上 `--user-data-dir`；`shell` 的 `automation_pipe` 1 個（第二個 machopo 開同一個資料夾時失敗、說出原因，第一個照常）。選資料夾的測試不打開它：mutant 弄壞「自動化在記憶體」時不會在家目錄建出資料夾（跑完確認過沒有）。
- mutation test（`mutants-2a.toml`）10 個，全部抓到（「被用著的資料夾照樣打開」第一次寫成編不過的，改了單獨重跑）。
- 文件：`docs/architecture.md` 的 `browser`、`crates/shell/README.md` 的自動化的 pipe、`docs/limitations.md`（第二個 machopo 拒絕啟動；環境沒說使用者的資料夾時視窗的 profile 在記憶體）。
- 改到視窗那一層（`window.rs`、`browser_window.rs`），跑了 `test.py scenarios --window`：72 次，0 個失敗（`caret` 照舊跳過），pipe 的 `--window` 也通過；家目錄底下沒有建出 profile 的資料夾（自動化在記憶體）。
- 跑過的檢查：`cargo test -p browser -p shell`（906 個）、`cargo clippy -p browser -p shell --all-targets -- -D warnings`；`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`boundary`、`docs`。

### 2b：磁碟上的儲存

macOS、Apple M4，2026-11-20。

- 決定了第 7 條（SQLite，`rusqlite`）和第 8 條（新的 crate `sql`）。比了 `rusqlite` 0.43（release 第一次編 12.4 秒、多約 1.9 MB、一個 transaction 1 萬筆 32 ms）和 `redb` 5.0（4.9 秒、約 1.1 MB、105 ms）；讀了 Chromium 的 `DomStorageDatabase`（LevelDB 搬到 SQLite 的遷移）、`LocalStorageImpl`（成批寫：5 秒、每小時 60 次）、`SQLitePersistentCookieStore` 和 `sql::Database`。
- 改了第 7 條：起草時寫了 WAL 模式，但 Chromium 的 `sql::Database` 的預設是 exclusive locking（編譯時就設、有 `static_assert`）、`journal_mode=TRUNCATE`、`synchronous` 用 SQLite 的預設，註解說 WAL 還在開發中；照這組改了，原文留下改的理由。
- 新的 crate `sql`：`Database`（`open`、`migrate`、`version`、`connection`）、`Error`（`Sqlite`、`Newer`、`Io`），re-export `rusqlite`。被鎖住時不等（`rusqlite` 預設等 5 秒；SQLite 的 C API 和 Chromium 不等）。還沒有人用它。
- 新的測試：`crates/sql/tests/it/database.rs` 的 6 個。
- mutation test（`mutants-2b.toml`）9 個，8 個抓到。第一次的 spec 有一個 mutant 的原文和 rustfmt 排的對不上、一個編不過，改了重跑；「被鎖住時等」原本只會變慢，測試加上第二條連線 1 秒內就失敗。沒抓到的「打開時不讀 schema」是等價的：在它之前的 `PRAGMA journal_mode` 準備時就讀了 schema（補了一個 schema 壞掉的檔案的測試確認打開時就換掉；記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）。
- 文件：`crates/sql/README.md`、`docs/architecture.md` 的 crate 表。
- 跑過的檢查：`cargo test -p sql`、`cargo clippy -p sql --all-targets -- -D warnings`；`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`docs`。

### 2c：第 2 階段的檢查和文件

macOS、Apple M4，2026-11-20。

- `cargo test --workspace`：4298 個通過、0 個失敗、1 個 ignored。`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 都沒有要改的。跑完之後家目錄底下沒有 profile 的資料夾（測試和自動化都在記憶體）。
- 文件：在 `docs/`、crate 的 README 裡找寫到 profile 的地方。`docs/limitations.md` 的 `--remote-debugging-port` 不寫 `DevToolsActivePort` 的理由原本是「machopo 沒有 profile 的資料夾」，改成 Chrome 寫在 user data dir 裡、machopo 的自動化預設在記憶體。其他的在 2a、2b 改好了。畫面上看不出差別（還沒有東西存進 profile），沒有新的情境。
- 跑過的檢查：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。

### 3a：同步等 Browser 的路、`localStorage`

macOS、Apple M4，2026-11-20。

- 決定了第 9 條（回答走一般的訊息，到 scheduler 裡等回答的位置）和第 10 條（renderer 照 Browser 給的 area 編號共用它的那一份）。
- 新的東西：`protocol` 的 `OpenLocalStorage`、`LocalStorageOpened`、`ChangeStorage`、`ForgetStorageAreas`；`renderer` 的 `MainThreadScheduler::sync_call` 和 `RendererStorageHost`；`site` 的 `StorageKey::serialize`；`browser` 的 `DomStorageContext`（自己的 thread、profile 的 `LocalStorage` 資料庫）和 `StoragePartition`（`WebContents::new` 原本收 `NetworkContext`，改收它）；`dom` 的 `Storage`；`dom_api` 的 `Storage` 介面、`Window.localStorage`、`StorageHost`；`dom_js` 在呼叫結束前把改動交給 host。
- 新的測試：`renderer` 的 `task_queue` 2 個、`site` 1 個、`browser` 的 `dom_storage` 8 個、`shell` 的 `browser_view` 1 個。
- mutation test（`mutants-3a.toml`）23 個，抓到 21 個。第一次跑時「回答隨便拿一個」沒抓到：兩個回答幾乎同時到，`HashMap` 的順序剛好拿對；改成別的呼叫的回答先到、自己的晚 50 ms。「context 的 thread 結束時不等它」原本也沒抓到：最後一次同步的「打開」之後 thread 已經寫完了，改成之後再送 500 筆。「另一份文件打開時清空 renderer 的那一份」原本沒抓到：測試裡 iframe 比 parent 先打開，改成 parent 先寫。剩下的兩個：「不在的 frame 不回答」只有送了不是自己的 frame 的 renderer 走得到，記進 `docs/test-gaps.md` 的「只靠審查」；「opaque 的文件也去問 host」是等價的（Browser 照 frame 樹的 key 一樣不給），記進「等價、不用處理的 mutation」。scheduler 的測試在另一條 thread 上等、最多 10 秒，讓「回答進了 queue」這類 mutant 失敗而不是卡住。
- 找到的問題：prototype 上有同名的成員時直接賦值（`localStorage.getItem = 'x'`）建出 own property、不經過 named setter（V8 的 non-masking interceptor）。WPT 的 `webstorage/set.window.js` 要它照樣存進 area、不建 own property；3c 跑 WPT 時對照 Chrome 的結果再決定，先記進 `docs/limitations.md`。
- 文件：`docs/architecture.md` 的 crate 表（`StoragePartition`、`DomStorageContext`）和「Process 的邊界」（main thread 同步等 Browser）、`docs/limitations.md`（`localStorage` 的限制，`file:` 的頁面沒有）、`crates/dom_api/README.md`。
- `python3 scripts/test.py scenarios --window`（改了 `shell` 的 `browser_window.rs`）：72 次，`demo-script` 失敗 1 次。那次的 log 最後是 Viz 的「can't draw, skipping frames」（`occluded`，視窗被別的視窗蓋住），之後沒有「drawing again」，畫面一直沒再畫，等待逾時。單獨在視窗裡連跑 4 次、不開視窗跑 1 次都通過（通過的那幾次一開始也有一兩次 `occluded`，之後接著畫）。原因是跑的期間視窗被蓋住，和這一段的改動無關（`demo-script` 不用 `localStorage`）。
- 跑過的檢查：`cargo test` 的 `site`、`sql`、`protocol`、`dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`browser`、`shell`；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。

### 3b：`storage` 事件、`sessionStorage`

macOS、Apple M4，2026-11-20。

- 決定了第 11 條（`localStorage` 的 `storage` 事件都經過 Browser）和第 12 條（`sessionStorage` 由 Browser 留在記憶體裡，每個分頁一個 namespace）。讀了 Chromium 的 `cached_storage_area.cc`（`SetItem`、`KeyChanged`、`EnqueueStorageEvent`）和 `session_storage_impl.h`（`BackingMode`）。
- 新的東西：`protocol` 的 `OpenStorageArea`（帶 `StorageKind`，取代 `OpenLocalStorage`）、`StorageAreaOpened`（原本的 `LocalStorageOpened`）、`StorageAreaChanged`，`ChangeStorage` 多了 `Storage` 的編號和文件的網址；`DomStorageContext` 留著每個 renderer 的通道、照分頁分 `sessionStorage` 的 namespace（`forget_namespace`，`WebContents::shutdown` 時）；`dom` 的 `sessionStorage`、還沒確認的改動、DOM 裡每個 `Storage` 的弱引用清單（發事件時走）、window 的 `storage` 事件的 task，加 `storage` listener（event handler 也是）時要求打開 area（Blink 的 `DOMWindowStorageController::DidAddEventListener`；WPT 的 iframe 只有 `<body onstorage>`）；`dom_api` 的 `sessionStorage`、`StorageEvent`（建構子、`initStorageEvent`）、`open_for_listeners`、`ScriptEngine::storage_area_changed`。`dom` 的 `dictionary_members.txt` 加了 `StorageEventInit` 的四個名稱。
- 測試的分頁原本都是 `TabId(0)`，兩個分頁共用同一個 `sessionStorage` 的 namespace；`browser` 的測試改成每個分頁從計數器拿一個 id。
- 新的測試：`dom` 的 `storage` 5 個，`browser` 的 `dom_storage` 5 個（2 個 context、3 個頁面）。
- mutation test（`mutants-3b.toml`）17 個，第一次抓到 15 個。「這裡還沒確認的 clear 不擋別人的改動」和「分頁關掉之後 `sessionStorage` 還在」沒抓到：補了 `dom` 的測試（clear 之後別人的 set 不套）和頁面測試的最後一段（關掉的分頁的 namespace 是空的），重跑這兩個都抓到。
- 文件：`docs/architecture.md` 的 crate 表（`DomStorageContext`）、`docs/limitations.md`（Web Storage 的限制；`storage` 從「從來不分派」的事件拿掉）、`crates/dom_api/README.md`。
- 跑過的檢查：`cargo test` 的 `dom`、`dom_api`、`dom_js`、`dom_wasm`、`renderer`、`browser`、`protocol`、`shell`（1280 個通過）；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。沒有動到視窗那一層，沒有跑 `scenarios --window`。

### 3c：上限、寫進磁碟的批次、WPT

macOS、Apple M4，2026-11-20。

- 決定了第 13 條（上限由 renderer 和 Browser 都守）和第 14 條（照 Chromium 成批寫進磁碟）。讀了 Chromium 的 `storage_area_map.cc`、`storage_area.mojom`、`local_storage_impl.cc`、`storage_area_impl.cc`。
- `webstorage/` 加進 `tests/wpt/include.txt`。第一次跑 17 個意外的結果：`initStorageEvent` 的 `storageArea` 參數讓 renderer panic（`dom::Storage` 的 `ObjectType` 沒有當參數用的 `take_arg`、`arg`），同一個 machopo 裡跑的四個上限的測試跟著 CRASH；修了之後，上限的四個仍然 CRASH：還沒有上限，`while (true)` 一直寫到記憶體用完。做了上限之後剩下的都是預期的失敗，`.ini` 寫上原因：`window.open`（7 份，其中一份等它打開的視窗、逾時，`disabled`）、`BroadcastChannel`、單獨的 surrogate（machopo 的字串是 UTF-8）、`set.window.js` 的 12 項和 `storage_builtins.window.js` 的 2 項。
- 3a 留下的「prototype 上有同名的成員時直接賦值建出 own property」：Chromium 的 `set.window-expected.txt` 和 `storage_builtins.window-expected.txt` 失敗的是同樣的項目、同樣的訊息（它的讀取給出項目、不是 prototype 的），machopo 不用改，`docs/limitations.md` 照這個寫。
- `document.createEvent('StorageEvent')`（和它的大小寫）：`Document-createEvent.https.html` 的 6 項、`EventTarget-dispatchEvent.html` 的 1 項變成通過，`.ini` 更新了。
- 新的東西：`protocol` 的 `PER_STORAGE_AREA_QUOTA`、`PER_STORAGE_AREA_OVER_QUOTA_ALLOWANCE`、`storage_quota_for`、`StorageAreaChangeFailed`；`dom` 的 area 記著用了多少（`Items`）、`storage_set` 超過上限時回 false、`storage_change_failed`；`browser` 的 `CommitOptions`、`DomStorageContext::with_commit_options`，每個 area 的一批和速率（`Batch`、`CommitRate`、`commit_delay`）。
- 新的測試：`dom` 的 `storage` 2 個，`browser` 的單元測試 1 個（延遲和速率）、`dom_storage` 4 個（Browser 拒絕、到期才寫、速率、頁面的上限）。
- mutation test（`mutants-3c.toml`）14 個，抓到 13 個；沒抓到的「超過上限的項目不能改小」是測試的值選得不好（改小之後剛好又在上限內），改成改小之後仍然超過，重跑抓到。「一批用一個 transaction」寫成的 mutant 編不過，而且從外面看不出差別，拿掉了。
- 第 3 階段的檢查：`cargo test --workspace` 4327 個通過、0 個失敗、1 個 ignored，跑完之後家目錄底下沒有 profile 的資料夾；`cargo fmt --check` 抓到一個測試的換行，照 rustfmt 改了；`cargo clippy --workspace --all-targets -- -D warnings` 抓到兩個測試裡太長的型別（改成 type alias）和測試分頁的編號用了 `fetch_add`（改成 `base::AtomicSequenceNumber`）。
- 文件：`docs/architecture.md`（`DomStorageContext` 的上限和批次）、`docs/limitations.md`、`crates/dom_api/README.md`（上限、`createEvent`）。

### 4a：請求的來歷

macOS、Apple M4，2026-11-20。

- 決定了第 15 條（cookie 自己照 RFC 6265bis 和 Chromium 寫；讀了 `cookie_store` 0.22.1 的原始碼）和第 16 條（每份文件一個 Browser 給的 loader 編號；讀了 Chromium 的 `network_context.mojom` 的 `URLLoaderFactoryParams` 和 `isolation_info.h`）。
- 新的東西：`site` 的 `SiteForCookies`（Chromium 的 `ComputeSiteForCookies`、`IsSchemefullyFirstParty`）；`protocol` 的 `ResourceRequest::trusted`（`TrustedParams`：site for cookies、最上層的網站），`CommitNavigation`、`FetchResource`、`DedicatedWorkerCreated` 帶 `loader`；`browser` 的 `FrameTree::loader_params`、`RenderProcessHost::register_loader`、IO thread 的 `check_request` 和四個新的 `BadMessageReason`；`renderer` 的 `DocumentLoaders`（文件 → loader，iframe 建立時它一開始的 `about:blank` 登記成 parent 的，換 engine 時清掉）、worker 的 `WorkerLoaders`；`dom` 的 `context_document_id`（只給文件的編號，不給文件本身）、`dom_api::WorkerStart::owner_document`。可信的參數 4b 才有人用（cookie），這一段看不出它們的效果，4b 的測試守它們。
- 找到的問題：圖片的請求的 `initiator` 一直是最上層的文件的 origin（iframe 裡的圖片也是），照新的核對會讓 renderer 被結束；改成圖片所在的文件的 origin。主文件和子文件在 commit 的途中就開始 parse，裡面的 script 可能馬上發請求，所以文件的 loader 在建文件的當下就登記，不等 commit 結束。
- `browser` 的測試建分頁時原本沒有 worker 的 engine（`worker_engine: None`），worker 不會跑；改成 V8 的，和 `shell` 一樣，`browser` 的測試照樣全部通過。
- WPT：跑了 `workers/`、`fetch/api/basic/`、`fetch/api/cors/`、`fetch/api/redirect/`、`html/semantics/embedded-content/the-iframe-element/`、`html/browsers/browsing-the-web/navigating-across-documents/`、`webstorage/`，沒有 renderer 被結束。`workers/semantics/navigation/001.html` 意外通過，連跑 5 次都通過；在這一段之前的 commit（`df988a8`）上也通過，是之前就通過、預期結果沒更新，拿掉了它的 `.ini`。
- 新的測試：`site` 的 `site_for_cookies` 2 個，`browser` 的單元測試 3 個（loader 的參數、請求的核對、worker 的請求），`browser` 的 `loaders` 1 個。
- mutation test（`mutants-4a.toml`）12 個，抓到 11 個；沒抓到的「`ws:` 不算它的 `http:` 網站」是測試只測了 `wss:` 對 `https:`，補了 `ws:` 對 `http:`，重跑抓到。
- 文件：`docs/architecture.md`（`RenderProcessHost` 的 loader）、`docs/limitations.md`（loader 留到 renderer 結束、opaque 的 `initiator` 照收）。
- 跑過的檢查：`cargo test` 的 `site`、`dom_api`、`renderer`、`browser`、`shell`（1296 個通過）；`cargo fmt --check`；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。

### 4b：解析 `Set-Cookie`、cookie 本身、記憶體裡的 store

macOS、Apple M4，2026-11-20。

- 決定了第 17 條（第三方的 cookie 預設擋，加照送的旗標；預設不做 Lax-allowing-unsafe）。讀了 RFC 6265bis 的 editor's draft（2026-11-17 版）的 §5.6.7、§5.8、§7，WPT 的 `cookies/samesite/` 和 `cookies/third-party-cookies/`，Chromium 的 `features.cc`、`cookie_constants.cc` 和它跑 `third-party-cookies/` 的 virtual suite。第 4 階段從四段改成五段（第三方的 cookie 預設擋，要和接上請求同一段做）。
- 這一段由 subagent 寫（`crates/net/src/cookies/` 的九個檔案和 `crates/net/tests/it/cookies.rs`），照 Chromium 的 `parsed_cookie.cc`、`canonical_cookie.cc`、`cookie_util.cc`、`cookie_constants.cc` 和 RFC 6265bis；之後的實作照使用者的要求在 session 裡自己做。在 session 裡審了 `parsed_cookie.rs`（對照 `ParseTokenValuePairs`：屬性名稱或值的字元不合法時整行拒絕、值太長時只略過那個屬性）、`canonical_cookie.rs`（對照 `ParseExpiration`、`ValidateAndAdjustExpiryDate`、`IsSetPermittedInContext`、`IncludeForRequestURL`、`GetEffectiveSameSite`）和 `cookie_monster.rs`（對照 `SetCanonicalCookie`、`MaybeDeleteEquivalentCookieAndUpdateStatus`），自己重跑了測試、clippy、fmt 和 mutation test。
- 審查改的：`CookieOptions` 的預設原本照 Chromium 做 Lax-allowing-unsafe，照第 17 條改成不做，加了守它的測試和 mutant。
- 和 Chromium 不同、留著的（寫在程式的註解裡）：設 cookie 之前先刪掉那個網域已經過期的（Chromium 留到讀取或超過上限時；RFC 6265bis 容許隨時刪），所以過期的 `Secure` cookie 擋不住新的；`Partitioned` 一律檢查要 `Secure`（Chromium 只在請求有分區的 key 時檢查，frame 的請求都有）；沒有 host 的網址（`data:`）直接拒絕；`Set-Cookie` 的值收的是 `&str`（Chromium 收 bytes，不是 UTF-8 的 header 要看 4c 怎麼解碼）；網域照 `url::Host::parse` 正規化（Chromium 是 `CanonicalizeHost`）。沒做的：`Priority`、警告、`CookieAccessDelegate`、origin-bound cookie（預設關）、schemeless 的 `SameSite`、數量的上限和淘汰（4d）、分區的 key（4d）。
- 和 RFC 6265bis 不同、照 Chromium 的（測試裡寫明）：值裡的 tab 拒絕；`=a=b` 拒絕（`kCookieParseRejectEmptyNameAmbiguous`）；`Max-Age=+5` 照收；1601 年以前的年份算過去；日要全是數字；取代時只有值一樣才留著建立的時間；有 `__Http-`、`__Host-Http-` 前綴。
- 新的測試：`net` 的 `cookies` 52 個，單元測試 6 個（日期、分隔字元、`StringToInt64`、path 和 domain 的比對、子網域）。
- mutation test（`mutants-4b.toml`，subagent 寫、在 session 裡加了一個預設選項的）26 個，抓到 25 個；沒抓到的「已經過期的 cookie 也存進去」是等價的（讀取和設定都先刪掉過期的），記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。subagent 第一次跑時漏掉的兩個（`;\n` 的那一行、不在 label 的邊界上的 domain 比對）它補了測試。
- 文件：`docs/architecture.md` 的 crate 表（`net` 的 `cookies`）、`crates/net/src/lib.rs` 的 crate 說明。
- 跑過的檢查：`cargo test -p net`（32 個單元測試、110 個整合測試）、`cargo clippy -p net --all-targets -- -D warnings`、`cargo fmt --check`；`sanity.py` 的 `numerics`、`tests`、`docs`。

### 4c：接上每一跳請求

macOS、Apple M4，2026-11-20。

- 照第 15 到 17 條做，沒有新的決定。照使用者的要求，這一段在 session 裡自己寫。
- 新的東西：`protocol` 的 `TrustedParams` 加 `initiator`（cookie 的 initiator，和觸發 CORS 的 `ResourceRequest::initiator` 分開）和 `is_main_frame_navigation`；`site` 的 `SiteForCookies::is_first_party_schemeless`（Chromium 判斷第三方的 `IsFirstParty`）；`browser` 的 `FrameTree::navigation_params`，`WebContents` 開始導覽時填上；`net` 的 `CookieJar`（`URLRequestHttpJob` 的 cookie 步驟和 `CookieSettings`：每一跳的 site for cookies、`SameSite` 的情境、第三方的規則），`url_loader` 照 Fetch 的「include credentials」決定帶不帶；`NetworkContextParams::allow_third_party_cookies` 和 `machopo --allow-third-party-cookies`（視窗和自動化都收）。
- 和 Chromium 不同、留著的（寫在程式的註解裡）：`Partitioned` 的 cookie 在 4d 照最上層的網站分開之前，一律當第三方的；不看 redirect 的路徑（Chromium 的 `kCookieSameSiteConsidersRedirectChain` 預設也關）；被拒絕的 cookie 不告訴 DevTools。
- 新的測試：`net` 的 `cookie_loads` 4 個（兩種規則都跑），`browser` 的 `cookies` 1 個（頁面、它的圖片、第三方的 iframe、之後的導覽，兩種規則都跑），`browser` 的單元測試改寫一個、新增一個（導覽的參數），`site` 的 `site_for_cookies` 補了不看 scheme 的比對，`shell` 的 `the_third_party_cookie_policy`。
- mutation test（`mutants-4c.toml`）15 個，抓到 14 個；沒抓到的「沒有可信參數的請求也有 cookie」是預設的規則下預設的參數本來就算第三方，補了照送的旗標時沒有可信參數的請求也不送的檢查，重跑抓到。
- 文件：`docs/limitations.md`（cookie 的現況：預設擋第三方、嚴格的 Lax、旗標、`Partitioned` 當沒標的、只在記憶體、沒有上限、沒有 `document.cookie`；`credentials` 照 Fetch 帶 cookie）、`docs/architecture.md`（`net` 的 `CookieJar`、`WebContents` 的導覽的可信參數）。
- 跑過的檢查：`cargo test` 的 `site`、`protocol`、`net`、`browser`、`shell`（1107 個通過）、`shell` 的 bin 測試（12 個）；`sanity.py` 見下。`scenarios --window`（改了 `window.rs`）在第 4 階段做完時跑。

### 4d：`Partitioned`、上限和淘汰、`Cookies` 資料庫

macOS、Apple M4，2026-11-20。

- 決定了第 18 條（資料庫在背景一次全讀）、第 19 條（值存明文）和第 20 條（上限照 Chromium、不認 `Priority`）。讀了 Chromium 的 `sqlite_persistent_cookie_store.cc`、`cookie_monster.cc`（`GarbageCollect`、`PurgeLeastRecentMatches`、`GarbageCollectLeastRecentlyAccessed`、`GarbageCollectPartitionedCookies`、`InternalUpdateCookieAccessTime`）、`cookie_partition_key.cc`（`FromNetworkIsolationKey`）、`canonical_cookie.cc`（`Create` 怎麼收分區、`IsEquivalentForSecureCookieMatching`）和 `network_context.cc`。
- 新的東西：`net` 的 `CookiePartitionKey`；`CanonicalCookie` 的分區（取代原本只記有沒有標 `Partitioned`）和 `from_storage`；`CookieMonster` 的分區、上限和淘汰、`PersistentCookieStore`、`load`、隔一分鐘才記成用過；`SqlitePersistentCookieStore`（自己的 thread、`CommitOptions`、`CookieStoreThread` 由 `NetworkContext` 留著，最後一個放掉時寫完、等它結束）；`CookieJar` 每一跳算分區、照每個 cookie 套政策（第三方的 `Partitioned` 照送照收）、`wait_for_load`；`NetworkContextParams::data_directory`；`browser` 的 `StoragePartition::new` 改收 `NetworkContextParams`，自己照 profile 啟動 network context（Chromium 的 `InitNetworkContext`），呼叫的地方跟著改。
- 和 Chromium 不同、留著的（寫在程式的註解裡）：所有分區的 cookie 放在同一個網域底下（Chromium 每個分區一張表）；網域的淘汰只有 medium 的兩輪（全部當 medium，配額 50 永遠不會限制，刪到 150 就停）；最上層是 opaque 的時沒有分區（Chromium 給它一個只在記憶體的分區）；資料庫的時間是 Unix epoch 起的微秒、沒有 `encrypted_value`、`priority`、`has_expires`、`is_persistent`、`last_update_utc`，唯一的欄位是名稱、domain、path 和分區（沒有 scheme 和 port：scheme-bound、port-bound 的 cookie 預設關）；壞掉的列只略過、不刪。分區的 cookie 沒有跨網域的總數上限（和 Chromium 一樣）。
- 4b 留下的差異改掉了：`Partitioned` 只在請求有分區時才檢查要 `Secure`。
- 新的測試：`net` 的 `cookie_monster` 6 個（分區、網域的上限、`Secure` 最後刪、全部的上限、分區的上限、告訴 store 的事），`cookie_loads` 加 3 個（重開之後還在、第三方的分區、分區看中間有沒有隔著別的網站），`cookies` 改了 `Partitioned` 的測試、加了隔一分鐘才記成用過的；單元測試：資料庫 4 個（欄位存回來一樣、成批寫、過期的和不是 cookie 的列、開不了），`cookie_jar` 3 個（等資料庫讀完、沒有資料庫時不用等、導覽每一跳的分區）；`browser` 的 `cookies` 加 1 個（profile 的資料夾經過 `StoragePartition` 留下 cookie，連跑 20 次都通過：舊的 renderer 的 IO thread 在 `shutdown` 時 join，沒有留下 network context）。
- mutation test（`mutants-4d.toml`）第一次 43 個，抓到 38 個。沒抓到的：「main frame 的導覽算第三方的分區」是等價的（main frame 的導覽的 site for cookies 本來就是它網址的網站），拿掉了 `CookiePartitionKey::for_request` 的 `main_frame_navigation` 參數；「存的分區的網站不檢查」是測試的那一列不是 `Secure`、被另一個檢查擋掉，改成 `Secure` 的；「網域的淘汰刪到分區的 cookie」是測試裡第二輪沒有要刪的，在會刪 `Secure` 的情境加了更舊的分區 cookie；「一個網域 180 筆就刪」「全部 3300 筆就刪」是外層的提前離開和內層的檢查一樣，只弄壞一個看不出來，改成一次弄壞兩層，全部的那個再把測試改成第 3300 筆在 31 天後才設（原本設下時每一筆都是最近用過的，提早淘汰也沒有東西可刪）。改完重跑整份 42 個，全部抓到。「jar 的淘汰等最早的使用時間」拿掉時結果一樣，記進 `docs/test-gaps.md` 的「沒有計數守著的優化」。
- 文件：`docs/limitations.md`（cookie 的分區、存在 profile、明文、上限不認 `Priority`、讀完之前請求等著）、`docs/architecture.md`（`net` 的 cookie、`StoragePartition` 啟動 network context）、`docs/test-gaps.md`（jar 的淘汰等最早的使用時間的捷徑）。
- 跑過的檢查：`cargo test` 的 `net`、`site`、`browser`、`shell`（1111 個通過）；`cargo clippy -p net -p browser -p shell --all-targets -- -D warnings`、`cargo fmt --check`；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。

