# RFC 0113 的進度：HTTP cache

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-24。完成日期：2026-11-24（macOS 26.6.2、Apple M4；Windows 上只跑過 `ipc` 的 `disk_space`）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 記憶體的 cache 和規則：新鮮度和驗證、記憶體的 backend、key 和 `frame_site`、接進 `url_loader`（條件請求、`304`、作廢、同時的請求）、load flag（`fetch()`、重新整理、上一頁、DevTools、強制重新整理的快捷鍵）、WPT 的 `fetch/http-cache/` 和它要的 `btoa`、`atob` | 完成 | 2026-11-24 |
| 2. 磁碟的 backend：SQLite 的 store、`HttpCache` 的兩種 backend 和磁碟的 index、照磁碟剩下的空間算大小、平台的 cache 資料夾 | 完成 | 2026-11-24 |
| 3. 重量網路版的 Speedometer | 完成 | 2026-11-24 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 規則 | 回應的 header：值怎麼列（逗號、引號、不合併的 header）、新鮮度（`max-age`、`Expires`、heuristic、永久的狀態碼、`stale-while-revalidate`）、年齡、要不要驗證、能不能存、條件請求的 header、`304` 的合併 | `cargo test -p net --lib http_cache::headers` |
| 規則 | `Vary` 的比對（`*`、`Authorization`、沒有的 header、值照原樣比）；存到磁碟時只存值的 SHA-256，照樣比對 | `cargo test -p net --lib http_cache::vary`、`http_cache::entry` |
| 規則 | request header 對 cache 的影響（`If-Match` 這類、`Range`、`no-cache`、`max-age=0`）和呼叫者自己的驗證 | `cargo test -p net --lib http_cache::request_headers` |
| key | 子資源照最上層和 frame 的網站，主框架的導覽照網址的網站，子 frame 的文件加 `s_`，別的網站發起的主框架導覽加 `cn_`；opaque 和沒有 `trusted` 的沒有 key；fragment、帳號、密碼不算 | `cargo test -p net --lib http_cache::key` |
| key | Browser 給子資源的 loader 它自己的網站（opaque 的沒有），導覽不給 | `cargo test -p browser --lib frame_tree` |
| 載入 | 經過 cache 的載入（`NetworkService` 對著本機的測試 server）：新鮮的不上網路、heuristic、`no-store` 和沒有依據的每次都上網路、不新鮮的帶 `ETag` 和 `Last-Modified` 驗證、`304` 帶來的新鮮度和 `no-store`、`Vary`（含 transport 加的 `Accept`）、照最上層和 frame 的網站分開、沒有網站的不經過、成功的 `POST` 拿掉 entry（失敗的不）、`HEAD`、永久的 redirect、`Content-Encoding` 照原樣存、太大的不存、`Range` 不經過、`206` 不存、同時的請求只到 server 一次 | `cargo test -p net --test it http_cache` |
| 載入 | load flag：`LOAD_VALIDATE_CACHE`、`BYPASS_CACHE`（送出時加 `Pragma: no-cache` 和 `Cache-Control: no-cache`，請求自己有的留著）、`SKIP_CACHE_VALIDATION`、`ONLY_FROM_CACHE`（沒有能用的是 `ERR_CACHE_MISS`）、`DISABLE_CACHE`；request header 對到的 flag 和請求的 flag 合起來；清空時正在存的也不留 | 同上 |
| 載入 | `304` 更新 entry 的時間、header 和 `Vary`，存的 header 不留 `Set-Cookie` | `cargo test -p net --lib http_cache::transaction` |
| 記憶體 | 上限 50 MiB、一個 entry 最多 1/8、超過時照最近用過的順序淘汰到 90%、entry 的大小（header、`Vary`、status text、body） | `cargo test -p net --lib http_cache::memory`、`http_cache::entry` |
| 磁碟 | `sql` 的資料庫可以打開成 `synchronous=OFF`，`raze` 刪掉檔案和 journal、照原本的設定重來 | `cargo test -p sql` |
| 磁碟 | store：寫進去的重新打開之後讀得回來（header、時間、`Vary`、body），body 邊收邊寫、每滿 512 KiB 一列，沒收完的不讀、下次啟動時刪掉，放棄的刪掉、留著上一個，完成的取代上一個，換掉 head，刪、更新使用的時間、清空（正在寫的也是），`Vary` 的值不在檔案裡，壞掉的資料庫清掉重來，打不開的什麼都不留 | `cargo test -p net --lib http_cache::sql_store` |
| 磁碟 | index 和淘汰：超過上限的 95% 時照「多久沒用 ×（大小 + 300）」淘汰到 90%，一個 entry 最多上限的 1/8 或 5 MiB，讀到的是用過（也寫回去），`304` 讓 entry 變大時也淘汰，store 沒有的 entry 從 index 拿掉；`Recorder` 在磁碟上邊收邊寫，太大、沒收完時當場放棄；大小照 Chromium 的 `PreferredCacheSize`（Windows 以外和 Windows 的每一段、讀不到剩下的空間時） | `cargo test -p net --lib http_cache::disk`、`http_cache::tests` |
| 磁碟 | network context 的 cache 在磁碟上（`http_cache_directory`、`http_cache_max_size`）：重新啟動之後照樣用（照樣分區）、`304` 的更新留在磁碟上、清空也清掉磁碟（正在寫的也是）、照上限淘汰、中途斷掉的不留 | `cargo test -p net --test it http_cache` |
| 磁碟 | 磁碟剩下的空間（`ipc::platform::available_disk_space`）：和 `df -kP` 的一樣，不在的路徑是錯誤；Windows 上也跑過 | `cargo test -p ipc --test it disk_space` |
| profile | cache 資料夾：macOS 的 `~/Library/Caches`、其他 Unix 的 `$XDG_CACHE_HOME`（`~/.cache`）底下、profile 在使用者資料夾底下的同一個位置；Windows、不在那底下的、使用者資料夾本身是 profile 自己的資料夾；相對和空的 XDG 變數不算 | `cargo test -p browser --test it profile::` |
| profile | profile 的 HTTP cache 在它的 cache 資料夾的 `Cache` 裡，重新打開 profile 之後頁面和圖片從那裡來 | `cargo test -p browser --test it a_profiles_http_cache_outlasts_its_storage` |
| renderer | 子資源的 flag：`fetch()` 的 `cache` 照 Blink 的 `GetLoadFlagsForWebURLRequest`，其他的照文件的載入（`DetermineFrameCacheMode`：強制重新整理的文件在 `load` 之前繞過、上一頁的不驗證，子 frame 跟著 parent），條件請求的驗證；DevTools 停用 cache 時繞過（只能用 cache 的變成找不到） | `cargo test -p renderer --lib loader`、`cargo test -p dom --test it frames::a_documents_load` |
| 整條路 | `fetch()` 的 `cache` 的每一種模式經過 renderer、Browser、`net` | `cargo test -p shell --test it fetch::a_fetchs_cache_mode` |
| 整條路 | 重新整理、強制重新整理（CDP 的 `Page.reload` 的 `ignoreCache`）、上一頁：文件、stylesheet、script、iframe 和它的 script、`fetch()`、圖片各自照 Chromium 的 flag 送出或從 cache 拿 | `cargo test -p shell --test it automation_pipe::reloads_and_history_use_the_http_cache` |
| 視窗 | 強制重新整理的快捷鍵（Shift+Cmd+R、Shift+F5、Ctrl+F5、Ctrl+Shift+R）：分派、繞過 cache、在視窗裡重新載入頁面 | `cargo test -p shell --lib browser_view::tests::shortcuts_per_platform`、`cargo test -p shell --test it browser_view::shift_reload_goes_past_the_http_cache`、`python3 scripts/test.py scenarios navigation` |
| DevTools | `fromDiskCache`；`Network.setCacheDisabled` 讓導覽和子資源（parser、stylesheet、圖片、`fetch()`）繞過 cache，`Network` 沒打開時是錯誤，`disable`、session 離開、client 斷線時取消，別的分頁的 session 不影響；`Network.clearBrowserCache` 清完才回答 | `cargo test -p shell --test it devtools_network::devtools_sees_the_http_cache` |
| WPT | `fetch/http-cache/` 和 `fetch/api/request/request-cache-*`：失敗的子測試和 Chrome 158 的一樣（`.ini` 寫著原因），除了沒有 `No-Vary-Search`、不從完整的回應給 `Range` 和沒有 `window.open`（`split-cache.html`）；`btoa`、`atob`（window 和 worker） | `python3 scripts/test.py wpt fetch/http-cache/ fetch/api/request/ html/webappapis/atob/` |
| 全部 | 守著這些的測試擋得住弄壞的實作 | `python3 scripts/test.py mutate --keep-going rfcs/0113-http-cache/mutants-<段>.toml`（`1a` 到 `1f`、`2a` 到 `2d`） |

## 做過的檢查

macOS 26.6.2（Apple M4），debug build，2026-11-24；branch `0113` 從 `main` 的 `5094ac2` 開始。Windows：push `69179e0` 之後，使用者在自己的 Windows 機器上跑了 `cargo test -p ipc --test it disk_space`，通過；其他部分沒有在 Windows 和 Linux 上跑過（每天的 CI 在 Linux 上跑）。

### 對照的原始碼

每條規則對著 Chromium `main`（2026-11-24）的原始碼寫：`net/http/` 的 `http_response_headers.cc`、`http_vary_data.cc`、`http_cache.cc`、`http_cache_transaction.cc`、`http_cache_util.cc`、`http_util.cc` 下載到本機逐條讀；`net/disk_cache/`（記憶體的、Simple Cache、SQL backend、`cache_util.cc`）、Blink 的 load flag（`web_url_request.cc`、`request_conversion.cc`、`frame_fetch_context.cc`、`fetch_manager.cc`、`inspector_network_agent.cc`）、content 的 `navigation_request.cc`、`browser_initiated_resource_request.cc`、`devtools_instrumentation.cc` 和 `chrome/common/chrome_paths_*` 下載或由 agent 讀 Gitiles。和原本記得的不同、照原始碼改了的：

- 能不能存只看 `no-store`（任何狀態碼都存，能不能用交給新鮮度）；`Expires` 解析不了時當成沒有；`Vary` 有 `Authorization` 時和 `Vary: *` 一樣；`Cache-Control: immutable` 預設不改變 `Pragma: no-cache`。
- 分區由 `SplitCacheFeatureEnableByDefault` 打開（agent 一開始從 `features.cc` 讀到「預設關」，和 Chrome 的實測不一致）。
- DevTools 停用 cache 是繞過（`LOAD_BYPASS_CACHE`），不是 `LOAD_DISABLE_CACHE`；繞過和驗證時送的 `Pragma`、`Cache-Control` 只在請求自己沒有時加（Fetch；照記得的寫成了換掉，WPT 的 `cache-mode.any.js` 抓到）。

Chrome for Testing 152 的實測：最上層換網站、或只換 iframe 的網站都重新要（三段的 key）；opaque 的 frame 不經過 cache；同時的請求只到 server 一次；強制重新整理時 iframe 的文件也帶 `no-cache` 重新要。

### 找到的 bug（都修好了）

- 從 cache 回答的 `HEAD` 帶著 body：以為 `url_loader` 丟掉每個 `HEAD` 的 body，其實只有 `file:`、`data:`（整合測試抓到）。
- 從 cache 回答的 body 算進 `encoded_body_length`（那個欄位寫著沒有經過網路時是 0）。
- 清空時正在存的回應寫完照樣存進去：cache 改成記著清過幾次，記錄器存之前比對（Chromium dooms 正在寫的 entry）。
- 帶 `Range` 的請求的 `206` 被存下來，之後要別的範圍、或要整份的請求拿到它（WPT 的 `partial.any.js`）：改成 `Range` 不經過 cache，`206` 不存。
- 繞過 cache 時換掉請求自己的 `Cache-Control`、`Pragma`（上面）。
- 上一頁、下一頁照 Chromium 從 cache 拿之後，`browser` 的三個導覽的測試和 `shell` 的 `webdriver::a_session_navigates` 數 server 收到的次數各少一次：照 Chromium 改了預期（1e 做完時只跑了 `cargo check`，整套的測試跑到才看到）。

### Mutation test

`mutants-1a.toml` 到 `mutants-2d.toml`，合計 250 個：1a 35、1b 14、1c 12、1d 30、1e 71、1f 9、2a 27、2b 29、2c 14、2d 9。沒抓到的都補了測試（呼叫者自己的驗證不相符時不存、空的 validator、不受信任的 header、端到端的圖片和 `fetch()`、快捷鍵的分派、別的分頁的 DevTools session、`raze` 留著 journal、`304` 讓 entry 變大時的淘汰、讀到時更新最後用的時間並寫回、中途斷掉時當場刪掉列、相對的 `XDG_CONFIG_HOME`……），或是等價的：寫 mutation 之前拿掉了會變成等價 mutation 的多餘程式（`ExternalValidation` 的 `is_ok`、記錄器出錯時設成 `None`、只能用 cache 又繞過的提早檢查、子 frame 的 commit 的 `bypass_cache`、讀 index 時的 `WHERE complete = 1`、`Recorder` 的 `done`）。`ipc` 的 `available_disk_space` 用 `f_bfree` 代替 `f_bavail` 在 macOS 上等價（APFS 沒有保留給 superuser 的空間），記在 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。測不到的：磁碟的 store 用到一半出錯（清掉重來時忘掉正在寫的、`HttpCache` 清空 index、完成的交易失敗、寫一段失敗），記在 `docs/test-gaps.md` 的「只靠審查」；`Profile::cache_dir` 接上目前的平台的那一行（要用使用者真的資料夾才測得到）。

### 跑過的檢查

- 第 1、2 階段做完時各跑一次 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`，都通過（clippy 要求的：`HeaderFlag` 的值不以 `Cache` 結尾；`begin_renderer_navigation` 和 `headless` 的 `Event` 帶理由的 `#[expect]`；2a 的一個測試用了會默默繞回的 `fetch_add`）。
- `sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`boundary`、`spans`、`docs` 在每一段做完時通過。
- WPT：`python3 scripts/test.py wpt fetch/api/ fetch/http-cache/ html/webappapis/atob/`（531 個測試）沒有新的失敗；多了兩個通過：`conditional-get.any.js`（cache 用 `ETag` 驗證），和 `keepalive.any.js` 的 iframe 在 `unload`、`pagehide` 送的四個請求（cache 整個關掉、在 `5094ac2` 上也通過，不是這份做的），照實更新了預期結果。
- 情境：`python3 scripts/test.py scenarios navigation`（macOS 和其他平台的快捷鍵）。
- 不穩定的測試：沒有碰到；磁碟的 cache 的整合測試連跑 5 次都通過。

### 網路版的 Speedometer（2026-11-24，用電池）

machopo 是 `0113` 的 `f450e27`（`perf` build，`machopo --automation-pipe`），Chrome 是 Chrome for Testing 152.0.7977.82（headless，新的 profile）。都經過 CDP 開 `https://browserbench.org/Speedometer3.1/?startAutomatically`（20 個預設的 suite、10 個 iteration），量法照 `details.md` 的「網路版的 Speedometer」：讀 runner 自己的結果和 mark，「載入 suite 的頁面」是每個 suite 的 `prepare-start` 到 `prepare-end` 加起來，「計分的區間」是每個 suite 的 total 加起來。五組同一個時段、同一個執行檔接連跑；沒有 cache 的那組是同一個執行檔經過 DevTools 的 `Network.setCacheDisabled`（導覽和子資源都繞過 cache）。

| | 分數 | 整頁 | 載入 suite 的頁面 | 計分的區間 |
| --- | --- | --- | --- | --- |
| machopo，沒有 cache | 27.5 | 110.7 s | 91.1 s | 9.0 s |
| machopo，記憶體的 cache（自動化的 profile） | 38.2 | 33.9 s | 17.8 s | 6.5 s |
| machopo，磁碟的 cache（新的資料夾） | 37.8 | 34.9 s | 18.5 s | 6.6 s |
| machopo，同一個資料夾，重新啟動之後 | 38.2 | 24.5 s | 9.1 s | 6.6 s |
| Chrome（新的 profile） | 57.4 | 22.7 s | 10.6 s | 4.2 s |

- 有了 cache，整頁從 110.7 秒變成 33.9 秒，分數從 27.5 變成 38.2：和 `details.md` 裡本機版的 38.5（整頁 23.5 秒）一樣。沒有 cache 的數字和 `details.md` 第一次量的（`main` 的 `4b78749`、接電源：28.0、115.3 秒）差不多。
- 計分的區間從 9.0 秒回到 6.5 秒（本機版 6.4 秒）：`details.md` 的「計分的區間也變慢」說的，每次載入在網路上閒置之後 CPU 變慢，在 suite 從 cache 來之後就沒有了。
- 第一次跑時，載入 suite 的頁面還有 17.8 秒：第一個 iteration 每個檔案都要下載（Chrome 也一樣，它是 10.6 秒）；重新啟動、從磁碟的 cache 拿時是 9.1 秒，和本機版（8.9 秒）一樣。剩下的是 machopo 自己載入、解析和編譯 suite 的頁面的時間：沒有 V8 的 code cache，每個 iteration 都重新編譯（「之後的可能」）。
- 記憶體和磁碟的 cache 差不到 3%。
- 第一次跑沒有 cache 的那組時，browserbench.org 對一個 suite 的檔案（`react-stockcharts` 的一個 chunk）回了 `503`，頁面停在那裡，等了 900 秒沒有跑完；重跑一次就沒有碰到。Chrome 那組第一次是量測的腳本的錯（沒等分頁出現就 attach），修了之後重跑。
