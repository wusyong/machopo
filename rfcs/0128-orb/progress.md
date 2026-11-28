# RFC 0128 的進度：跨源的回應不交給不該拿的 renderer（ORB 和 CORP）

開始日期：2026-11-28。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「opaque 的 context 和 cookie」「Chromium 的 ORB v0.1」「CORP」）、已決定的事項第 1 到 5 條 | 做完 | 2026-11-28 |
| 1. 發起者和 CORP：`None` 是壞訊息、CORS 照 `mode`、`data:` worker 有自己的參數、文件的 loader 收到 `"null"` 不帶 cookie（第 3 條）；CORP（第 4 條）；被攻破的 renderer 的角色；`fetch/cross-origin-resource-policy/` 的 WPT | 做完 | 2026-11-28 |
| 2. ORB：MIME type 的清單、`nosniff`、`206`、sniffer、被擋時的回應（第 1、2 條）；`fetch/orb/`、`fetch/corb/` 的 WPT；收尾 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| renderer 的請求不帶發起者是壞訊息 | `cargo test -p browser --lib render_process_host::tests::a_request_takes_its_loaders_params`；被攻破的 renderer（自己的 process）：`cargo test -p browser --test it renderer_process::a_renderer_registers_no_blob_url_but_its_documents_own` |
| 發起者 `"null"` 經過 tuple origin 的 loader、opaque 的 worker 的請求：沒有 site for cookies，最上層的網站照舊 | `render_process_host::tests::a_request_takes_its_loaders_params`、`render_process_host::tests::a_workers_request_goes_through_its_loader` |
| `data:` 網址的 worker 帶 credentials 的請求不帶 cookie、`Origin` 是 `null`（照 Chrome 實測的）；它的 module 的靜態 import（發起者是建它的頁面的）也不帶 | `cargo test -p browser --test it cookies::a_data_url_workers_requests_send_no_cookies`、`cookies::a_data_url_module_workers_imports_send_no_cookies` |
| CORP 的每一種值、大小寫、兩個 header、`same-site` 的 scheme 和 IP 位址 | `cargo test -p net --lib corp::` |
| CORP 照 WPT（`no-cors` 的每一跳、redirect、`fetch()`、圖片、script、iframe 不查） | `python3 scripts/test.py wpt fetch/cross-origin-resource-policy/` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 第 1 段：發起者和 CORP（2026-11-28，Linux：Fedora 45，x86-64，AMD Radeon 890M）

- **先確認的**：machopo 的 `data:` worker 帶 credentials 的請求，改之前帶著頁面的 cookie（`lax=1; plain=1`），`Origin` 是 `null`；Chrome 不帶（`details.md` 的「opaque 的 context 和 cookie」）。測試先寫成要的行為，改之前失敗。
- **改的設計**（`README.md` 改寫了）：
  - `net` 的 `Cors::new` 沒有改：它照「有沒有發起者」分的正是「renderer 的、Browser 自己的」（導覽不該走 CORS），renderer 的不帶已經在 Browser 這一側擋成壞訊息。
  - `docs/limitations.md` 的「網路」的「`null` 的發起者照收，不像 Chromium 核對 precursor」拿掉：Chromium 也不核對，machopo 又多了不帶 cookie。
- **新的錯誤碼**：`NetErrorCode::BlockedByResponse`（`ERR_BLOCKED_BY_RESPONSE`，Chromium 對 CORP 擋下的用的）。
- **WPT**：`fetch/cross-origin-resource-policy/` 加進 `tests/wpt/include.txt`。意外的結果記成預期的：`SharedWorker`、service worker 的變體（machopo 沒有）是 `ERROR`；`image-loads.html` 的兩個 CORS 的圖片的 subtest 失敗，因為 `<img crossorigin>` 沒有作用，圖片一律 `no-cors`，這一條本來就是，但 `docs/limitations.md` 沒寫，補進「圖片」。
- **mutation test**（`mutants-1.toml`，15 個）：第一次跑 13 個抓到、2 個沒抓到。
  - 「a data: URL's worker isn't opaque」：`data:` worker 的 `fetch()` 的發起者本來就是 `null`，「`null` 不帶 cookie」那一條就拿掉了 cookie，worker 自己的旗標沒被測到。它有作用的是發起者不是 `null` 的請求：module worker 的靜態 import（HTML 的 outside settings，發起者是頁面的）。補了 `cookies::a_data_url_module_workers_imports_send_no_cookies`，重跑抓到了。
  - 「the renderer sends a request from no document」：要 DOM 排著的載入在送出前文件就不在，測試做不出這個時機，記進 `docs/test-gaps.md` 的「只靠審查」。
  - 跑 WPT 的兩個 mutant 不能帶 `--no-build`（不然跑的是沒編進 mutant 的執行檔，假的通過），寫設定時拿掉了。
  - 跑完確認每個 mutant 的 `from` 都還在檔案裡（檔案都還原了）；`cargo fmt` 之後也確認了一次。
- **WPT 的預期結果**：`workers/dedicated-worker-in-data-url-context.window.js` 的 4 個 subtest 預期 `FAIL`，現在通過。這一段之前的 `main`（`eb370848`）上也通過：它早就會過，只是之前沒跑到。用 `MACHOPO_UPDATE_EXPECTATIONS` 更新了。
- **跑過的**：
  - 改到的 crate 的測試（`protocol`、`net`、`browser`、`renderer`、`shell`）；
  - sanity 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；
  - `cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`；
  - `cargo test --workspace`：5,074 個通過、0 個失敗、1 個 ignored；
  - `test.py wpt fetch/api/cors/ fetch/cross-origin-resource-policy/ workers/ FileAPI/url`：419 個測試，意外的只有上面那一個；
  - `test.py scenarios --window`（階段的驗收）：75 次，0 個失敗。
- 量測工具的測試沒跑：renderer 的啟動、trace、census 和 automation pipe 的 CDP 方法都沒改。
