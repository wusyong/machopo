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
| 2c：第 2 階段的檢查和文件 | 未開始 | |
| 3：Web Storage | 未開始 | |
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
