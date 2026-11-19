# RFC 0106 的進度：串流的載入

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-20

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：data pipe（`ipc::data_pipe`） | 完成 | 2026-11-20 |
| 1b：回應分成 head 和 body，`net` 的 `URLLoaderClient`，`fetch(done)` 包在上面 | 未開始 | |
| 1c：HTTP 的 body 一段一段讀、解壓、sniff，`file:`、`data:`、`blob:` 走管道 | 未開始 | |
| 1d：第 1 階段的檢查和文件 | 未開始 | |
| 2：Browser 提早 commit | 未開始 | |
| 3：renderer 邊收邊 parse | 未開始 | |
| 4：舊畫面的時限、檢查和文件 | 未開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| data pipe：照順序、上限照 byte 嚴格守住、背壓、兩端的結束、`watch` 的通知、跨 thread | `cargo test -p ipc --test it data_pipe::` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a：data pipe

macOS、Apple M4，2026-11-20。

- 新的測試：`crates/ipc/tests/it/data_pipe.rs` 的 14 個。producer 的 `write` 用手動的 poll（no-op 的 waker，或記下有沒有被喚醒的 waker）測，等不等、有沒有被喚醒是確定的，不靠 thread 和時間；另外一支跨 thread 的經過 4 KiB 的管道送 300 KB。
- mutation test（`mutants-1a.toml`）17 個，全部抓到。第一輪沒抓到「容量 0」（`create(0)` 的管道永遠寫不進去），補了 `a_pipe_holds_at_least_a_byte`；一開始用 thread 測「consumer 不在了，等著的 write 被喚醒」會和 drop 的時機競爭，改成記下喚醒的 waker。
- 跑過的檢查：`cargo test -p ipc`、`cargo clippy -p ipc --all-targets -- -D warnings`；`sanity.py` 的 `numerics`、`deps`、`tests`、`docs`。
