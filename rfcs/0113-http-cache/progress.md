# RFC 0113 的進度：HTTP cache

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-24

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 記憶體的 cache 和規則 | | |
| 1a. 新鮮度和驗證的規則（照 Chromium 的 `HttpResponseHeaders`、`HttpVaryData`、`http_cache_util`） | 做完，待審查 | 2026-11-24 |
| 1b. 記憶體的 backend（上限、淘汰，已決定的第 5、7 條） | 做完，待審查 | 2026-11-24 |
| 1c. key：`TrustedParams` 加 `frame_site`（已決定的第 1、2 條） | 做完，待審查 | 2026-11-24 |
| 1d. cache 接進 `url_loader`：查、條件請求和 `304`、存、作廢、同時的請求（已決定的第 3、8、9、10 條） | 做完，待審查 | 2026-11-24 |
| 1e. load flag：`fetch()` 的 `cache`、重新整理、上一頁、DevTools | | |
| 1f. WPT 的 `fetch/http-cache/`、情境、文件、mutation test | | |
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

