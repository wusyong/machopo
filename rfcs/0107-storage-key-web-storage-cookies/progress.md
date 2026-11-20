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
| 4：`net` 的 cookie | 未開始 | |
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
