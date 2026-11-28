# RFC 0128 的進度：跨源的回應不交給不該拿的 renderer（ORB 和 CORP）

開始日期：2026-11-28。完成日期：2026-11-28（Linux：Fedora 45，x86-64，AMD Radeon 890M）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「opaque 的 context 和 cookie」「Chromium 的 ORB v0.1」「CORP」）、已決定的事項第 1 到 5 條 | 做完 | 2026-11-28 |
| 1. 發起者和 CORP：`None` 是壞訊息、`data:` worker 有自己的參數、文件的 loader 收到 `"null"` 不帶 cookie（第 3 條）；CORP（第 4 條）；被攻破的 renderer 的角色；`fetch/cross-origin-resource-policy/` 的 WPT | 做完 | 2026-11-28 |
| 2. ORB：MIME type 的清單和 Chromium 的 `Content-Type` 解析、`nosniff`、`206`、sniffer、被擋時的回應（第 1、2 條）；`fetch/orb/`、`fetch/corb/` 的 WPT，逐項和 Chrome 比；收尾 | 做完 | 2026-11-28 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **發起者** | |
| renderer 的請求不帶發起者是壞訊息 | `cargo test -p browser --lib render_process_host::tests::a_request_takes_its_loaders_params`；被攻破的 renderer（自己的 process）：`cargo test -p browser --test it renderer_process::a_renderer_registers_no_blob_url_but_its_documents_own` |
| 發起者 `"null"` 經過 tuple origin 的 loader、opaque 的 worker 的請求：沒有 site for cookies，最上層的網站照舊 | `render_process_host::tests::a_request_takes_its_loaders_params`、`render_process_host::tests::a_workers_request_goes_through_its_loader` |
| `data:` 網址的 worker 帶 credentials 的請求不帶 cookie、`Origin` 是 `null`（照 Chrome 實測的）；它的 module 的靜態 import（發起者是建它的頁面的）也不帶 | `cargo test -p browser --test it cookies::a_data_url_workers_requests_send_no_cookies`、`cookies::a_data_url_module_workers_imports_send_no_cookies` |
| **CORP** | |
| 每一種值、大小寫、兩個 header、`same-site` 的 scheme 和 IP 位址 | `cargo test -p net --lib corp::` |
| `no-cors` 的每一跳、redirect、`fetch()`、圖片、script，iframe 不查 | `python3 scripts/test.py wpt fetch/cross-origin-resource-policy/` |
| **ORB** | |
| 每一條規則：放行和從不 sniff 的清單、`nosniff`（只看第一個值）、從中間開始的 `206`、HTML（空白、BOM、註解、大小寫）、XML、JSON 和 JavaScript 跑不了的前綴、沒有 `Content-Type` 的放行、看不出的放行；Chromium 的 `Content-Type` 解析；只看 renderer 的、`no-cors`、opaque、不是 `fetch()` 的 | `cargo test -p net --lib orb::` |
| 和 Chrome 158 stable 逐項一樣，但 machopo 沒有的 `<audio>`、`<video>`、`<link rel=preload>`、`ping`，和子資源的 blob URL 不照 origin 分開 | `python3 scripts/test.py wpt fetch/orb/ fetch/corb/` |
| 沒有誤擋：圖片、script 元素的 WPT | `python3 scripts/test.py wpt html/semantics/embedded-content/the-img-element/ html/semantics/scripting-1/the-script-element/` |

## 做過的檢查

2026-11-28，Linux：Fedora 45，x86-64，AMD Radeon 890M。

- **先確認的**：machopo 的 `data:` worker 帶 credentials 的請求，改之前帶著頁面的 cookie（`lax=1; plain=1`），`Origin` 是 `null`；Chrome 不帶（`details.md` 的「opaque 的 context 和 cookie」）。測試先寫成要的行為，改之前失敗。
- **改的設計**（`README.md` 都改寫了）：
  - `net` 的 `Cors::new` 沒有改：它照「有沒有發起者」分的正是「renderer 的、Browser 自己的」（導覽不該走 CORS），renderer 的不帶已經在 Browser 這一側擋成壞訊息。
  - ORB 的 MIME type 照 Chromium 的寬鬆解析（`image/svg xml` 是 `image/svg`）：照 Fetch 解析時，WPT 的 `fetch/corb/script-resource-with-json-parser-breaker` 的 3 個 `image/svg+xml`（query 裡的 `+` 成了空白）被當成沒有 `Content-Type`、不 sniff 就放行，和 Chrome 不同。renderer 看到的 MIME type 照舊照 Fetch。
  - 不 sniff 圖片和影音：Chromium 先照特徵放行它們、記下影音的網址；machopo 沒有媒體元素，那些特徵沒有一種是 HTML、XML、JSON 的開頭，放不放行結果一樣（mutation test 抓不到），拿掉了。做媒體元素時和「記住影音的網址」一起加。
  - `docs/limitations.md` 的「網路」的「`null` 的發起者照收，不像 Chromium 核對 precursor」拿掉：Chromium 也不核對，machopo 又多了不帶 cookie。
- **新的錯誤碼**：`NetErrorCode::BlockedByResponse`（`ERR_BLOCKED_BY_RESPONSE`，CORP）、`NetErrorCode::BlockedByOrb`（`ERR_BLOCKED_BY_ORB`），都是 Chromium 的名字。
- **WPT**：
  - `fetch/cross-origin-resource-policy/`、`fetch/orb/`、`fetch/corb/` 加進 `tests/wpt/include.txt`（`fetch/corb/` 原本只收 5 個 SVG 的 reftest）。
  - CORP 的失敗：`SharedWorker`、service worker 的變體是 `ERROR`；`image-loads.html` 的兩個 CORS 的圖片失敗，因為 `<img crossorigin>` 沒有作用、圖片一律 `no-cors`，這是本來就有的，`docs/limitations.md` 沒寫，補進「圖片」。
  - `fetch/orb/`、`fetch/corb/` 的每個 subtest 都和 Chrome 158.0.8127.88 stable（wpt.fyi 2026-11-27 的 run 的 `report.json`）比過。Chrome 也失敗的一樣失敗：`no-cors` 的 `fetch()` 照第 2 條拿到空的回應、不 reject；Chromium 的放行清單裡有的 `text/vtt`、`application/dash+xml`、`audio/*`；不是 ok 的 status 不擋（v0.1 跳過那一步）。不同的只有 machopo 沒有的 `<audio>`、`<video>`（`.any.html` 的 subtest 照順序跑，第一個媒體的逾時之後其餘的都 NOTRUN；`.worker` 變體只有 `fetch()` 的，全部一樣）、HLS、`<link rel=preload>`、`ping`，和 `script-html-via-cross-origin-blob-url`：子資源的 blob URL 照 renderer 找、不照 origin，同一個 renderer 裡跨網站的 iframe 做的也載得到（測試自己說「blocked both with and without CORB」，和 ORB 無關），本來就有，`docs/limitations.md` 沒寫，補進「blob URL」。
  - 意外通過的，在這份 RFC 之前的 `main`（`eb370848`）上也通過（早就會過，只是之前沒跑到），用 `MACHOPO_UPDATE_EXPECTATIONS` 更新了：`workers/dedicated-worker-in-data-url-context.window.js`、`html/semantics/embedded-content/the-img-element/image-loading-lazy.html`、`html/semantics/scripting-1/the-script-element/module/credentials.sub.html`、`html/semantics/scripting-1/the-script-element/script-crossorigin-network.sub.html`。
- **mutation test**：
  - 第 1 段（`mutants-1.toml`，15 個）：第一次 13 個抓到、2 個沒抓到。「a data: URL's worker isn't opaque」：`data:` worker 的 `fetch()` 發起者本來就是 `null`，另一條規則就拿掉了 cookie；補了 module worker 的靜態 import 的測試（發起者是頁面的），重跑抓到了。「the renderer sends a request from no document」：測試做不出那個時機，記進 `docs/test-gaps.md` 的「只靠審查」。
  - 第 2 段（`mutants-2.toml`，23 個）：第一次 20 個抓到、3 個沒抓到。「SVG is XML」等價（放行清單先放行了，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）；「images aren't let through by their magic」等價，連同整個圖片、影音的 sniff 拿掉了（上面「改的設計」），這個 mutant 跟著刪掉；「JSON's key isn't looked for」補了 `{"a", 1}` 不是 JSON 的測試，重跑抓到了。
  - 跑 WPT 的 mutant 不帶 `--no-build`（不然跑的是沒編進 mutant 的執行檔，假的通過）。
  - 每次跑完確認每個 mutant 的 `from` 還在檔案裡（檔案都還原了）；`cargo fmt` 之後也確認了，改了兩個被 fmt 重排的 `from`。
- **不穩定的測試**：`shell` 的 `devtools::the_pause_on_exceptions_goes_on_in_the_next_renderer` 在收尾的 `cargo test --workspace` 裡失敗了一次（「no Debugger.paused after 30.4s」，收到的是 `Debugger.scriptParsed`），單獨跑 10 次都過。
  - 原因（照程式碼確認）：V8 處理 `Debugger.setPauseOnExceptions` 時，回答從 `bindings` 的 `Outlet::send_response` 當場送出；session 的新狀態要等 dispatch 回來之後才由 `dispatched` 送（`bindings` 的 `devtools.rs`）。回答先到 Browser，測試一拿到就導覽到 `about:crash`；renderer 在兩則之間被排到後面時，還沒送出狀態就結束了，下一個 renderer 拿到的是舊的狀態，不會在例外上暫停。
  - 這是 RFC 0124 的 DevTools 狀態留下的競速，這份 RFC 沒有碰到它。這份做到一半時 `main` 上已經修好了（`ce94c42a`：`bindings` 在 V8 回答時算好狀態、和回答一起交出，renderer 先告訴 Browser 狀態再送回答，照 Blink 的 `DevToolsSession::SendProtocolResponse`），merge 前 rebase 上去之後重跑了 `cargo test --workspace`。
- **跑過的**：
  - 改到的 crate 的測試（`protocol`、`net`、`browser`、`renderer`、`shell`）；
  - sanity 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；
  - `cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`（第一次抓到 `orb.rs` 的 `Iterator::last`，改成 `next_back`）；
  - `cargo test --workspace`：第 1 段 5,074 個通過、0 個失敗；收尾 5,084 個通過、1 個失敗（上面那個不穩定的測試）、1 個 ignored；
  - `test.py wpt`：第 1 段 `fetch/api/cors/ fetch/cross-origin-resource-policy/ workers/ FileAPI/url`（419 個測試）；收尾 `fetch/orb/ fetch/corb/ fetch/cross-origin-resource-policy/ fetch/api/cors/ workers/ html/semantics/embedded-content/the-img-element/ html/semantics/scripting-1/the-script-element/`（863 個測試）。意外的只有上面那幾個早就會過的；
  - `test.py scenarios`：99 次，0 個失敗；
  - `test.py scenarios --window`：第 1 段、收尾各 75 次，0 個失敗。
- 量測工具的測試沒跑：renderer 的啟動、trace、census 和 automation pipe 的 CDP 方法都沒改。
- 在視窗裡看不出新的行為（被擋的跨源資源本來就畫不出來、看不到），`about:demo` 和情境沒有加東西。
