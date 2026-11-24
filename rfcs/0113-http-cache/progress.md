# RFC 0113 的進度：HTTP cache

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-24

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 記憶體的 cache 和規則 | 做完，待審查 | 2026-11-24 |
| 1a. 新鮮度和驗證的規則（照 Chromium 的 `HttpResponseHeaders`、`HttpVaryData`、`http_cache_util`） | 做完，待審查 | 2026-11-24 |
| 1b. 記憶體的 backend（上限、淘汰，已決定的第 5、7 條） | 做完，待審查 | 2026-11-24 |
| 1c. key：`TrustedParams` 加 `frame_site`（已決定的第 1、2 條） | 做完，待審查 | 2026-11-24 |
| 1d. cache 接進 `url_loader`：查、條件請求和 `304`、存、作廢、同時的請求（已決定的第 3、8、9、10 條） | 做完，待審查 | 2026-11-24 |
| 1e. load flag：`fetch()` 的 `cache`、重新整理、上一頁、DevTools（已決定的第 11 條） | 做完，待審查 | 2026-11-24 |
| 1f. WPT 的 `fetch/http-cache/`、情境、文件、mutation test；`btoa`、`atob`（已決定的第 12 條） | 做完，待審查 | 2026-11-24 |
| 2. 磁碟的 backend | | |
| 3. 重量網路版的 Speedometer | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 回應的 header 的規則：值怎麼列（逗號、引號、不合併的 header）、新鮮度（`max-age`、`Expires`、heuristic、永久的狀態碼、`stale-while-revalidate`）、年齡、要不要驗證、能不能存、條件請求的 header、`304` 的合併 | `cargo test -p net --lib http_cache::headers` |
| `Vary` 的比對（`*`、`Authorization`、沒有的 header、值照原樣比） | `cargo test -p net --lib http_cache::vary` |
| request header 對 cache 的影響（`If-Match` 這類、`no-cache`、`max-age=0`）和呼叫者自己的驗證 | `cargo test -p net --lib http_cache::request_headers` |
| cache 的 key：子資源照最上層和 frame 的網站，主框架的導覽照網址的網站，子 frame 的文件加 `s_`，別的網站發起的主框架導覽加 `cn_`；opaque 和沒有 `trusted` 的沒有 key；fragment、帳號、密碼不算 | `cargo test -p net --lib http_cache::key` |
| Browser 給子資源的 loader 它自己的網站（opaque 的沒有），導覽不給 | `cargo test -p browser --lib frame_tree` |
| 經過 cache 的載入（`net` 的 `NetworkService` 對著本機的測試 server）：新鮮的不上網路、heuristic、`no-store` 和沒有依據的每次都上網路、不新鮮的帶 `ETag` 和 `Last-Modified` 驗證、`304` 帶來的新鮮度和 `no-store`、`Vary`（含 transport 加的 `Accept`）、照最上層和 frame 的網站分開、沒有網站的不經過、成功的 `POST` 拿掉 entry（失敗的不）、request header 的 `no-cache`、`max-age=0`、`If-Match`、呼叫者自己的驗證、`HEAD`、永久的 redirect、`Content-Encoding` 照原樣存、太大的不存、同時的請求只到 server 一次 | `cargo test -p net --test it http_cache` |
| `304` 更新 entry 的時間、header 和 `Vary`，存的 header 不留 `Set-Cookie` | `cargo test -p net --lib http_cache::transaction` |
| 記憶體的 backend：上限 50 MiB、一個 entry 最多 1/8、超過時照最近用過的順序淘汰到 90%、entry 的大小（key、header、`Vary`、status text、body） | `cargo test -p net --lib http_cache::memory`、`http_cache::entry` |
| cache 的 load flag：`LOAD_VALIDATE_CACHE`、`BYPASS_CACHE`（送出時加 `Pragma: no-cache` 和 `Cache-Control: no-cache`，換掉請求自己的）、`SKIP_CACHE_VALIDATION`、`ONLY_FROM_CACHE`（沒有能用的是 `ERR_CACHE_MISS`）、`DISABLE_CACHE`；request header 對到的 flag 和請求的 flag 合起來；清空 cache 時正在存的也不留 | `cargo test -p net --test it http_cache` |
| renderer 給子資源的 flag：`fetch()` 的 `cache` 照 Blink 的 `GetLoadFlagsForWebURLRequest`，其他的照文件的載入（`DetermineFrameCacheMode`：強制重新整理的文件在 `load` 之前繞過、上一頁的不驗證，子 frame 跟著 parent），條件請求的驗證；DevTools 停用 cache 時繞過（只能用 cache 的變成找不到） | `cargo test -p renderer --lib loader`、`cargo test -p dom --test it frames::a_documents_load` |
| `fetch()` 的 `cache` 的每一種模式經過整條路（renderer、Browser、`net`） | `cargo test -p shell --test it fetch::a_fetchs_cache_mode` |
| 重新整理、強制重新整理（CDP 的 `Page.reload` 的 `ignoreCache`）、上一頁：文件、stylesheet、script、iframe 和它的 script、`fetch()`、圖片各自照 Chromium 的 flag 送出或從 cache 拿 | `cargo test -p shell --test it automation_pipe::reloads_and_history_use_the_http_cache` |
| 強制重新整理的快捷鍵（Shift+Cmd+R、Shift+F5、Ctrl+F5、Ctrl+Shift+R）和它的效果 | `cargo test -p shell --lib browser_view::tests::shortcuts_per_platform`、`cargo test -p shell --test it browser_view::shift_reload_goes_past_the_http_cache` |
| WPT 的 `fetch/http-cache/` 和 `fetch/api/request/request-cache-*`：失敗的子測試和 Chrome 158 的一樣（`.ini` 寫著原因），除了沒有 `No-Vary-Search`、不從完整的回應給 `Range` 和沒有 `window.open`（`split-cache.html`） | `python3 scripts/test.py wpt fetch/http-cache/ fetch/api/request/` |
| 帶 `Range` 的請求不經過 cache，`206` 不存；送出時的 `Pragma`、`Cache-Control` 只在請求自己沒有時加 | `cargo test -p net --test it http_cache` |
| `btoa`、`atob`（window 和 worker） | `python3 scripts/test.py wpt html/webappapis/atob/` |
| 強制重新整理的快捷鍵在視窗裡重新載入頁面 | `python3 scripts/test.py scenarios navigation` |
| DevTools：`fromDiskCache`；`Network.setCacheDisabled` 讓導覽和子資源（parser、stylesheet、圖片、`fetch()`）繞過 cache，`Network` 沒打開時是錯誤，`disable`、session 離開、client 斷線時取消，別的分頁的 session 不影響；`Network.clearBrowserCache` 清完才回答 | `cargo test -p shell --test it devtools_network::devtools_sees_the_http_cache` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a（2026-11-24，macOS 26.6.2、Apple M4、debug build，`5094ac2` 之上）

**對照的原始碼**：Chromium `main` 的 `0e8fa24e`（2026-11-24）的 `net/http/http_response_headers.cc`、`.h`、`http_vary_data.cc`、`http_cache_transaction.cc`、`http_cache_util.cc`、`http_util.cc`，下載到本機逐條讀。和原本記得的不同、改寫了 `details.md` 和程式的：存的條件只有 `no-store`（任何狀態碼都存，能不能用交給新鮮度）；`Expires` 解析不了時當成沒有（不是過期）；`Vary` 有 `Authorization` 時和 `Vary: *` 一樣；`Cache-Control: immutable` 在 Chromium 預設不改變 `Pragma: no-cache`（`kCacheControlImmutable` 預設關）。

**新的測試**：`net` 的 `http_cache` 的單元測試 23 個，全部通過；寫的過程中補了「時鐘往回走時，沒有新鮮度的回應照樣要驗證」（沒有它，「新鮮度和 staleness 都是 0 就要驗證」那一行拿掉也不會失敗）。

**mutation test**（`mutants-1a.toml`，35 個）：35 個都抓到。第一次跑時「`304` 留著被取代的舊行」沒抓到：那條 mutation 寫成 `updated.is_empty() || !updated…`，和原本的程式等價，改成 `true || !updated…` 之後抓到。

**檢查**：`cargo test -p net --lib http_cache` 通過；`sanity.py` 的 `numerics`、`tests`、`unsafe`、`deps`、`docs` 通過。

### 1b（2026-11-24，macOS 26.6.2、Apple M4、debug build，`5094ac2` 之上）

**對照的原始碼**：同一份 Chromium 的 `net/disk_cache/memory/mem_backend_impl.cc`、`mem_entry_impl.cc`（由 agent 讀 Gitiles，關鍵的常數和淘汰的規則它引了原文）：上限、`MaxFileSize` 是上限的 1/8、寫入讓總量變大時淘汰到 90%。和 Chromium 不同的：上限固定 50 MiB（已決定的第 7 條）；讀出來的是 body 的 `Bytes` 的副本（共用同一塊記憶體），所以淘汰不用跳過正在讀的 entry；一個新的 entry 太大時，它要取代的舊 entry 也拿掉（Chromium 的交易這時已經把舊的換掉）。

**新的測試**：`http_cache::memory` 5 個、`http_cache::entry` 1 個；寫 mutation 時發現 entry 的大小只有 body 有測，補了 header、`Vary`、status text 的那一個。`http_cache` 合計 29 個，全部通過。

**mutation test**（`mutants-1b.toml`，14 個）：14 個都抓到。

**檢查**：`cargo test -p net --lib http_cache` 通過。

### 1c（2026-11-24，macOS 26.6.2、Apple M4、debug build，`5094ac2` 之上）

**對照的原始碼**：Chromium `main` 的 `0e8fa24e` 的 `net/http/http_cache.cc`（`GenerateCacheKey`、`SplitCacheFeatureEnableByDefault`、`IsSplitCacheEnabled`）和 `net/base/features.cc`。之前 agent 從 `features.cc` 讀到分區「預設關」，和實測不一致；原因是 Chrome 啟動時另外打開它。

**新的測試**：`net` 的 `http_cache::key` 5 個；`browser` 的 `frame_tree` 兩個現有的測試加上 `frame_site` 的檢查。`TrustedParams` 的新欄位讓 `net` 的 `cookie_jar` 和 `tests/it/cookie_loads.rs` 的幾個測試要補上它（照它們的 initiator 給）。`cargo test -p net`（212 個）、`cargo test -p browser --lib frame_tree` 通過；`cargo check --workspace --all-targets` 沒有錯誤和警告。

**mutation test**（`mutants-1c.toml`，12 個）：12 個都抓到。第一次跑時多一條「opaque 的 initiator 當成同網站」沒抓到：serialize 過的 opaque origin 一律是 `null`，parse 不了，而且 `url_site` 在前面已經確定不是 opaque，opaque 的網站不會等於它，所以那個 `!initiator.is_opaque()` 是多餘的，拿掉之後那條 mutation 也拿掉。

### 1d（2026-11-24，macOS 26.6.2、Apple M4、debug build，`5094ac2` 之上）

**做法**：`net::http_cache::transaction::send` 接在 `url_loader::load` 每一跳的 `transport.send` 的位置（cookie、CORS 的 `Origin` 和 preflight 之後，CORS 的檢查和 redirect 之前）。body 經過 `HttpBody` 時，記錄器（`http_cache::Recorder`）留下線上還沒解碼的 chunk，整份收完才存；出錯或被取消時記錄器跟著 body drop，什麼都不存，並放開等候的載入。從 cache 回答的 `HttpBody` 是存的 chunk，經過同一個解碼器。被跟著走的 redirect 的 body 不讀，`done_reading` 把它當成完整的（Chromium 的 `DoneReading`），所以 `301` 存得下來。測試用的 server 加了 `Route::revalidated`、`with_304_header`：照 `ETag`、`Last-Modified` 回 `304`，可以帶不同的 header。

**改掉的判斷**：原本以為 `url_loader` 對每個 `HEAD` 都丟掉 body，所以從 cache 回答 `HEAD` 時不用另外清空；整合測試抓到：它只對 `file:`、`data:` 這麼做，HTTP 的 `HEAD` 靠 server 不送 body，所以從 cache 回答時要清空。另外，從 cache 回答的 body 原本也算進 `encoded_body_length`，改成只算線上來的（那個欄位寫著沒有經過網路時是 0）。

**新的測試**：`net` 的 `tests/it/http_cache.rs` 20 個，`http_cache::transaction` 的單元測試 3 個；`net` 合計 235 個，全部通過，cache 的整合測試連跑 3 次都通過。cache 接上之後 `browser` 和 `shell` 的測試照樣全部通過（現有的測試 server 多半不帶可以 cache 的 header；重新整理和上一頁的 load flag 在 1e）。

**mutation test**（`mutants-1d.toml`，30 個）：30 個都抓到。第一次跑時「呼叫者的驗證和存的 entry 不相符時也照相符處理」沒抓到：沒有測「不相符時回應不存」，補了 `the_callers_own_validation_of_nothing_stored_isnt_kept`。「等候的載入沒被叫醒」要等到載入的 30 秒期限才失敗（33 s）。寫 mutation 之前拿掉了三處多餘的程式（`ExternalValidation` 的 `is_ok` 的檢查、body 出錯時把記錄器設成 `None` 的兩行），它們會變成等價的 mutation。

**檢查**：`cargo test -p net`、`cargo test -p browser -p shell` 通過；`sanity.py` 的 `numerics`、`tests`、`unsafe`、`deps`、`boundary`、`spans`、`docs` 通過。


### 1e（2026-11-24，macOS 26.6.2、Apple M4、debug build，`fb27fa9` 之上）

**對照的原始碼**：Chromium `main`（2026-11-24）的 `net/base/load_flags_list.h`、`net/http/http_cache_transaction.cc`；Blink 的 `platform/exported/web_url_request.cc`、`platform/loader/fetch/url_loader/request_conversion.cc`（`GetLoadFlagsForWebURLRequest`）、`core/loader/frame_fetch_context.cc`（`DetermineFrameCacheMode`）、`core/fetch/fetch_manager.cc`、`core/inspector/inspector_network_agent.cc`；content 的 `browser/renderer_host/navigation_request.cc`、`browser/loader/browser_initiated_resource_request.cc`、`browser/devtools/devtools_instrumentation.cc`（`ApplyNetworkRequestOverrides`），下載到本機逐條讀。和 RFC 原本寫的不同、改了 RFC 的：DevTools 停用 cache 是繞過（`LOAD_BYPASS_CACHE`，回應照樣存），不是 `LOAD_DISABLE_CACHE`。繞過和驗證時送出的 `Pragma`、`Cache-Control`（Chromium 的 `HttpNetworkTransaction` 加的）是照記得的寫的，沒有讀原始碼，1f 的 WPT `fetch/api/request/request-cache-*` 會驗證。Chrome for Testing 152 的實測：強制重新整理時 iframe 的文件也帶 `no-cache` 重新要（`frame_fetch_context` 的 frame 的 cache 模式跟著 parent）。

**做法**：`ResourceRequest` 有 `LoadFlags`，`net` 的交易把它和 request header 對到的 flag 合起來。renderer 給子資源 flag（`fetch()` 的 `cache`，其他的照文件的載入：`dom` 的文件記著它是不是強制重新整理載入的，`load` 之前的子資源繞過；上一頁的不驗證；子 frame 跟著 parent），子 frame 的導覽在 `BeginNavigation` 帶它的 flag。Browser 照導覽的種類（`FrameLoadType`）給 flag。DevTools 的 `setCacheDisabled` 由 browser 照每個 session 記著（給導覽），也轉給 renderer（給子資源）；`clearBrowserCache` 是 `NetworkContext::clear_http_cache`，正在存的回應也不留（cache 記著清過幾次，記錄器存之前比對）。和 Chromium 不同的：`Network` 沒打開時 `setCacheDisabled` 是錯誤（Blink 先記著，打開時生效），已經打開時再 `enable` 會取消它（Blink 不會），和 machopo 的 `setAttachDebugStack` 一樣照 session 的狀態。

**拿掉的程式**：只能用 cache 又要繞過的請求的提早檢查（後面的分支本來就回 `ERR_CACHE_MISS`，mutation 是等價的）；子 frame 的 commit 帶的 `bypass_cache`（永遠是 false：子 frame 的強制重新整理在 renderer 這一側照 parent 算）；`DocumentLoader` 讀回 `bypass_cache` 的欄位（沒人讀，換成 commit 時直接告訴 `dom`）。

**新的測試**：`net` 的 `tests/it/http_cache.rs` 的 load flag 和清空的測試；`renderer` 的 `loader` 的單元測試；`dom` 的 `frames::a_documents_load_says_how_its_subresources_use_the_cache`；`shell` 的 `fetch::a_fetchs_cache_mode`、`automation_pipe::reloads_and_history_use_the_http_cache`、`browser_view::shift_reload_goes_past_the_http_cache`、`devtools_network::devtools_sees_the_http_cache_and_can_go_past_it_or_clear_it`，和 `browser_view` 的快捷鍵的單元測試。現有的測試要改的：`browser` 的三個導覽的測試（`a_link_navigates_and_history_goes_back_and_forward`、`the_state_comes_back_with_the_entry`、`a_traversal_within_a_document_gone_loads_the_entry`）和 `shell` 的 `webdriver::a_session_navigates` 數 server 收到幾次，上一頁、下一頁從 cache 拿之後各少了一次，照 Chromium 改了預期（文件照樣重新載入、commit）。

**檢查**：`cargo test -p net -p protocol -p renderer -p browser -p dom -p dom_api -p dom_js -p shell` 通過（`shell` 的整合測試 694 個）；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。

**mutation test**（`mutants-1e.toml`，71 個：1e-1 的交易 16 個、1e-2 的 `fetch()` 和 1e-3 的重新整理、上一頁、快捷鍵 33 個、1e-4 的 DevTools 22 個）：71 個都抓到。沒抓到、補了測試的：空的 validator 讓 cache 停用、不受信任的 request header 不給 flag、繞過時換掉請求自己的 `Cache-Control`、端到端的測試原本沒有圖片和 `fetch()`（renderer 的三條送出的路只測到一條）、快捷鍵的分派；DevTools 的測試原本只有一個分頁，「別的分頁的 session 也算」沒抓到（那個 session 只讓它自己的分頁重算），補了「這個分頁的 session 再設一次時不被別的分頁的蓋過」。1e-4 讓文件的載入那兩條 mutation 的 `from` 對不上了（它們包含下一個函式的開頭，1e-4 在中間加了函式），改短之後重跑，照樣抓到。

### 1f（2026-11-24，macOS 26.6.2、Apple M4、debug build，`9bb29e4` 之上）

**`btoa`、`atob`**：WPT 的 `fetch/http-cache/` 的共用框架用 `btoa`，machopo 沒有它時整個目錄跑不了，所以加上（已決定的第 12 條）。解碼用 `net` 已經依賴的 `data-url` crate 的 forgiving-base64，編碼用 `dom_api` 已經有的 `base64`。WPT 的 `html/webappapis/atob/base64.any.js`（window 和 worker，760 個子測試）全部通過。

**WPT**：`fetch/http-cache/` 加進清單；`fetch/api/request/request-cache-*` 本來就在清單裡，cache 的子測試現在全部通過（拿掉 152 行預期的失敗）。`fetch/http-cache/` 的失敗逐一和 wpt.fyi 上 Chrome 158（2026-11-23 的 Linux run）的結果對照：`cc-request`、`credentials`、`heuristic`、`invalidate`、`post-patch`、`pragma-no-cache-with-cache-control`、`vary` 失敗的子測試和 Chrome 一樣；Chrome 通過、machopo 失敗的是 `no-vary-search`（沒有 `No-Vary-Search`）、`partial` 的三個（從存的完整回應給較小的範圍）和 `split-cache`（用 `window.open`，machopo 沒有；分區由 machopo 自己的測試守著），都記進了 `docs/limitations.md` 或已經在那裡。每個 `.ini` 開頭寫著原因。

**WPT 找到的 bug**（都修了、補了測試）：
- 繞過和驗證時送出的 `Pragma`、`Cache-Control` 換掉了請求自己的（1e 照記得的 Chromium 寫的，連測試也是照它寫的）：Fetch 規定請求自己有時不加，`cache-mode.any.js` 的三個子測試因此失敗。改成只在沒有時加，那個測試改成檢查請求自己的留著。
- 帶 `Range` 的請求的 `206` 被存下來，之後要別的範圍、或要整份的請求拿到它（`partial.any.js`：要 `bytes=2-4` 拿到 `01234`，要整份的拿到 `206`）：machopo 不存資源的一部分，改成帶 `Range` 的請求不經過 cache（Chromium 處理不了的範圍也是 `LOAD_DISABLE_CACHE`），`206` 不存。

**情境**：cache 本身在情境裡看不到（情境的頁面是 `file:` 的，不經過 HTTP cache），由 `shell` 的整合測試守著；`navigation.txt` 加上強制重新整理的三組快捷鍵（Shift 加平台的重新整理、Shift+F5、Ctrl+F5）重新載入頁面，macOS 和其他平台的快捷鍵都通過。`about:demo` 沒有加東西：它是 machopo 自己的頁面，不經過 HTTP cache。

**清單裡其他的 WPT**：`python3 scripts/test.py wpt fetch/api/ fetch/http-cache/ html/webappapis/atob/`（531 個測試）沒有新的失敗，多了兩個通過：`fetch/api/basic/conditional-get.any.js`（cache 用 `ETag` 驗證）；`keepalive.any.js` 的 iframe 在 `unload`、`pagehide` 送的四個請求，cache 整個關掉時、在這份 RFC 開始之前的 `5094ac2` 上也通過，不是這份做的（之前的變更沒有更新預期結果），照實更新了預期結果和註解。

**階段做完時的檢查**：`cargo fmt --check`（`cargo fmt` 修了 1c 到 1e 留下的格式，都在這份改過的檔案裡）、`cargo clippy --workspace --all-targets -- -D warnings`（`HeaderFlag` 的值都以 `Cache` 結尾，改名成 `Disable`、`Bypass`、`Validate`；`begin_renderer_navigation` 多了 `load_flags` 之後超過參數的上限，照 `start_navigation` 寫帶理由的 `#[expect]`；`ResourceRequest` 多了 `LoadFlags`，`RendererHostMsg` 從 272 bytes 變成 280，`headless` 的 `Event` 的大小差超過上限，寫帶理由的 `#[expect]`：每個 renderer 的訊息都是 `Host`，box 它每個訊息都要配置一次）、`cargo test --workspace` 都通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。

**mutation test**（`mutants-1f.toml`，9 個）：9 個都抓到（`btoa`、`atob` 的 6 個由 WPT 的 `base64.any.js` 抓到）。1f 改了 1e 的一條 mutation 改的程式（加 header 的方式），`mutants-1e.toml` 的那一條改成對著新的程式；`HeaderFlag` 改名也照著改了 `mutants-1d.toml`、`mutants-1e.toml`；`cargo fmt` 改了 `renderer` 的 `loader.rs` 的兩處，1e 的兩條 mutation 的 `from` 跟著改、重跑，照樣抓到。

