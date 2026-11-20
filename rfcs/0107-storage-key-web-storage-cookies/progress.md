# RFC 0107 的進度：持久資料的 key，Web Storage 和 cookie

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-20

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：`site` crate：`Site`（HTML 的「obtain a site」）和 registrable domain（`psl`） | 完成 | 2026-11-20 |
| 1b：`StorageKey`，frame 樹在文件 commit 時算出每個 frame 的 key | 未開始 | |
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
