# RFC 0106 的進度：串流的載入

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-20

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：data pipe（`ipc::data_pipe`） | 完成 | 2026-11-20 |
| 1b：回應分成 head 和 body，`net` 的 `URLLoaderClient`，`fetch(done)` 照舊 | 完成 | 2026-11-20 |
| 1c：HTTP 的 body 一段一段讀、解壓、sniff，`file:` 一段一段讀、`data:` 走管道 | 完成 | 2026-11-20 |
| 1d：第 1 階段的檢查和文件 | 完成 | 2026-11-20 |
| 2：Browser 提早 commit | 未開始 | |
| 3：renderer 邊收邊 parse | 未開始 | |
| 4：舊畫面的時限、檢查和文件 | 未開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| data pipe：照順序、上限照 byte 嚴格守住、背壓、兩端的結束、`watch` 的通知、跨 thread | `cargo test -p ipc --test it data_pipe::` |
| `net` 的 `load`：head 和 body 的管道、結束（錯誤、傳輸的大小、時間）、沒有回應的只有結束、body 沒人讀時等著、放掉管道就停、`HEAD`、等或自己跟 redirect、取消 | `cargo test -p net --test it client::` |
| body 一段一段來：head 在 body 之前、收到一段交一段（壓縮的也是）、解不開的在 head 之後從結束來、沒有 `Content-Type` 時等開頭的 1024 bytes（或結尾）、redirect 的 body 不等、`file:` 分段讀；`fetch` 收齊才交回 | `cargo test -p net --test it client::`、`load::`、`content_encoding::` |
| `Content-Encoding` 一段一段解，一次給一個 byte 和整份一樣：gzip 的 header（選用的欄位）和 trailer（CRC、長度）、`deflate` 的 zlib 和 raw、brotli、截斷和之後多出來的 | `cargo test -p net --lib content_encoding::` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a：data pipe

macOS、Apple M4，2026-11-20。

- 新的測試：`crates/ipc/tests/it/data_pipe.rs` 的 14 個。producer 的 `write` 用手動的 poll（no-op 的 waker，或記下有沒有被喚醒的 waker）測，等不等、有沒有被喚醒是確定的，不靠 thread 和時間；另外一支跨 thread 的經過 4 KiB 的管道送 300 KB。
- mutation test（`mutants-1a.toml`）17 個，全部抓到。第一輪沒抓到「容量 0」（`create(0)` 的管道永遠寫不進去），補了 `a_pipe_holds_at_least_a_byte`；一開始用 thread 測「consumer 不在了，等著的 write 被喚醒」會和 drop 的時機競爭，改成記下喚醒的 waker。
- 跑過的檢查：`cargo test -p ipc`、`cargo clippy -p ipc --all-targets -- -D warnings`；`sanity.py` 的 `numerics`、`deps`、`tests`、`docs`。

### 1b：回應分成 head 和 body，`net` 的 `URLLoaderClient`

macOS、Apple M4，2026-11-20。

- `protocol::network::ResourceResponse` 拆成 `ResponseHead` 和 `{ head, encoded_body_length, body }`（README 的第 8 條），`RedirectInfo` 帶 `ResponseHead`，DevTools 的 `response_json` 取 head。讀欄位的地方照 rustc 的錯誤位置機械地加上 `.head`。
- `NetworkContext::load(request, AtRedirects, client)` 和 `UrlLoaderClient`。這一段 loader 照舊收完整份 body，才給 head 和管道、一次寫進去；body 一段一段來在 1c。
- 新的測試：`crates/net/tests/it/client.rs` 的 8 個。
- mutation test（`mutants-1b.toml`）8 個，全部抓到。
- 跑過的檢查：`cargo test -p ipc -p net -p protocol -p browser`、`cargo test -p renderer --lib`、`cargo test -p shell --lib`、`cargo check --workspace --all-targets`；`sanity.py` 的 `boundary`、`numerics`、`deps`、`tests`、`docs`。

### 1c：body 一段一段讀

macOS、Apple M4，2026-11-20。

- `http::send` 收到 header 就回來，body 是 `HttpBody`：一次讀 reqwest 的一段、解好再交出，數著傳輸的大小。`url_loader::load` 回 `Response { head, body }`，body 從 HTTP、檔案或整份來；`fetch(done)` 把它收齊（只有一段時不複製），`load` 一段一段寫進管道。
- `Content-Encoding`：flate2 的 `write::GzDecoder` 在一次只給幾個 byte 時把正常的 gzip 解錯、zlib 截斷時不報錯，和原本讀整份的不同，所以照 Chromium 的 `GzipSourceStream` 用底層的 `Decompress`，gzip 的 header 和 trailer 自己讀，`deflate` 照開頭兩個 byte 分 zlib 和 raw（原本是 zlib 失敗就整份重試 raw）；brotli 的 `DecompressorWriter` 和原本的一樣。截斷的照舊是錯誤，stream 結束之後多出來的照舊不理。
- 沒有 `Content-Type` 時先讀開頭的 1024 bytes（或到結尾）sniff，只看這 1024 bytes（`net::kMaxBytesToSniff`；原本看整份，前面有超過 1024 個空白的 markup 現在是 `text/plain`），讀了的照樣從 body 交出。redirect 的 body 不讀（原本讀完才跟），preflight 的照舊不讀；`HEAD` 和 opaque 的 `fetch()` 的 body 照舊讀完、不交出（傳輸的大小照算，HTTP/1 的連線還能重用）。
- `file:` 開檔之後在 blocking 的 thread 上一次讀 64 KiB；目錄的列表整份。`data:` 整份寫進管道。`blob:` 不經過 `load`（Browser 和 IO thread 直接呼叫 `load_blob_url`，在呼叫的 thread 上回應），它的管道在 Browser 提早 commit 時建。
- 收到一半的錯誤（解不開、連線斷了）對 `load` 是 head 之後從 `on_complete` 來，對 `fetch(done)` 照舊是整個的錯誤。整份交回的回應的 `response_end` 是收齊的時候，本機的 scheme 照舊整份算一個時刻；`load` 的 head 的 `response_end` 是 head 到的時候，結束的時間在 `CompletionStatus`。
- 測試伺服器加了 `Route::with_pause_in_body`（body 送到一半停一陣子）。
- 新的測試：`client.rs` 8 個（head 在 body 之前、分段、壓縮的分段、解不開的、沒有 `Content-Type` 的短 body 和長 body、redirect 的 body 不等、`file:` 分段），`load.rs` 2 個（分段的 body 整份交回、只 sniff 1024 bytes），`content_encoding.rs` 1 個（截斷的 gzip），`content_encoding` 的單元測試改成每份都整份和一個 byte 一個 byte 各解一次、比對，加了 5 個（輸出超過一次的空間、截斷、之後多出來的、gzip header 的選用欄位、zlib header 的判斷）；opaque 的 `fetch()` 檢查照樣讀了 body。
- mutation test（`mutants-1c.toml`）27 個，26 個抓到。第一輪沒抓到 4 個：gzip header 的 extra 不跳過（測試的 extra 裡沒有 0，接著找檔名的 0 時把它吃掉了，結果一樣；extra 裡放一個 0）、HTTP 的 body 結尾不 `finish`（沒有截斷的壓縮 body 的 HTTP 測試，補了）、丟棄的 body 不讀（沒檢查傳輸的大小，補了）、inflate 在輸入用完、輸出滿了時就停：等價，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」（miniz_oxide 輸出滿了就不再吃輸入，留著的最多是一段 258 bytes 的 match，下一段交出）。
- 跑過的檢查：`cargo test -p net -p browser`、`cargo clippy -p net --all-targets -- -D warnings`、`cargo check --workspace --all-targets`；`sanity.py` 的 `numerics`、`deps`、`tests`、`docs`、`boundary`、`unsafe`、`spans`。

### 1d：第 1 階段的檢查和文件

macOS、Apple M4，2026-11-20。

- `cargo test --workspace`：4232 個通過、0 個失敗、1 個 ignored。`cargo clippy --workspace --all-targets -- -D warnings` 沒有警告。`cargo fmt --check` 抓到 `browser` 的 `web_contents.rs` 三處讀 `.head` 的欄位的換行（1b 機械地加上的），照 rustfmt 改了。
- `test.py wpt fetch/api/basic/ fetch/api/redirect/ navigation-timing/`：`fetch/` 的都照預期。`navigation-timing/` 有 4 份意外通過（`nav2-test-document-replaced.html`、`nav2-test-timing-persistent.html` 的 Part 2、`nav2-test-navigation-type-backforward.html`、`test-navigation-type-backforward.html`），單獨重跑一次結果一樣。它們要 iframe 用 `location.href` 導覽到別的文件（`.ini` 寫著 machopo 不會），是前一份做 session history 和子 frame 導覽的 RFC 做到的、那時沒跑這個目錄，和這個階段的改動無關；預期結果還沒更新。
- 文件：這個階段改到的行為（`Content-Encoding`、sniff、redirect 的 body、`file:` 的讀法）在 `docs/` 和 crate 的 README 裡 grep 過，`docs/architecture.md` 的 `net` 那一列和 `docs/limitations.md` 在 1c 改好了，其他的描述照樣成立。畫面上看不出差別（導覽照舊收完整份 body 才 commit），沒有新的情境。
- 跑過的檢查：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
