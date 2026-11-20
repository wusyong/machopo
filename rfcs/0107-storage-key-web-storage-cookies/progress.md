# RFC 0107 的進度：持久資料的 key，Web Storage 和 cookie

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-20

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：`site` crate：`Site`（HTML 的「obtain a site」）和 registrable domain（`psl`） | 完成 | 2026-11-20 |
| 1b：`StorageKey`，frame 樹在文件 commit 時算出每個 frame 的 key | 完成 | 2026-11-20 |
| 1c：DevTools 的 `domainAndRegistry`，第 1 階段的檢查和文件 | 未開始 | |
| 2：profile 和磁碟 | 未開始 | |
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
