# RFC 0020：用上游的 wptserve 跑 WPT

- 開始日期：2026-10-07
- 狀態：✅ 已完成：2026-10-08（Linux；macOS 和 Windows 交給 `main` 上每天的 CI，「已決定的事項」第 5 條）
- 細節：`details.md`；進度：`progress.md`

## 摘要

WPT 的 testharness.js 測試和 reftest 由上游的 wptserve（`../third_party/wpt` 的 `wpt serve`）提供，取代 `net::test_server` 模仿 wptserve 的那一部分和 reftest 的 `file:`；runner、headless 的 kun、`testharnessreport.js` 的 hook、Chromium 格式的 `-expected.txt` 和 reftest 的 expectations 都不變。Rust 的 runner 自己啟動和結束 wptserve（`shell::wptserve`）。kun 這一側只多一份「web test 用的 network 設定」（`net::NetworkContextParams`）：把 wptserve 的網域對應到本機（Chromium 的 `--host-resolver-rules`），並信任 WPT 的測試用根憑證。因為 server 而跳過的測試重新試跑，能跑的都移出 skip 清單。接上 wptrunner 和 WebDriver（上游完整的 `wpt run`）不在這份的範圍，留在「之後的可能」。

這份是 RFC 0008 的第 10 條和第 28 條留下的「之後要 `.sub`、`.headers` 時可以改跑上游的 wptserve，到時候再決定」。2026-10-08 接受（主要的四個選擇：誰啟動 wptserve、網域怎麼解析、HTTPS、port），同一天做完；各步開始前決定的細節在「已決定的事項」第 6 到 13 條。做的過程中在 `net` 找到兩個 bug（憑證錯誤的分類、沒有 `close_notify` 的 HTTPS body），在 renderer 找到一個（經過 redirect 的 classic script 的網址），都修了（`progress.md`）。

## 動機

**做之前（2026-10-07）**：RFC 0008 選了照 Chromium 的 `run_web_tests` 做一套自己的跑法（第 3、10 條）：Rust 寫的 runner（`4ff8d6e:crates/shell/tests/wpt_testharness.rs`）用 `net::test_server` 以 checkout 為根提供檔案，自己照 wptserve 包裝 `.any.js` 和 `.window.js`。和上游的正式跑法（`wpt run`：wptserve 加上 wptrunner，透過 WebDriver 控制瀏覽器）的差別見 `details.md` 的「做之前的跑法和上游的差別」。其中代價最大的是 server：

- **wptserve 的功能都沒有**：`.sub` 的替換、`.headers` 檔、`.py` handler、pipe（`?pipe=trickle(...)`）、別的 host（跨來源）、HTTPS。`4ff8d6e:tests/wpt/testharness-skip.txt` 的 165 行裡，約 30 行就是因為這些（`details.md` 的「因為 server 而跳過的測試」），主要在 RFC 0015 的 module 和 `import()`；之後的 fetch、CORS、`location`、`postMessage`、iframe 的跨來源，比例只會更高。
- **port 每次都不同**：`net::test_server` 綁 `127.0.0.1:0`，失敗訊息裡有網址的測試寫不成 baseline，只能跳過（`concurrent-failed-imports.any.js`、`import-meta-resolve-importmap.html`、`modulepreload-*referr*`）。
- **自己維護一份 wptserve 的行為**：`.any.js` 的包裝、`META: global=` 的判斷、MIME 都是照 wptserve 重寫的（`4ff8d6e:crates/shell/src/testharness.rs`）；上游改了不會跟著變，`// META: variant=` 還沒做（RFC 0008 的第 55 條）。
- **reftest 從 `file:` 載入**：以 `/` 開頭的路徑（`/css/support/...`、`/common/reftest-wait.js`、`/fonts/ahem.css`）都載不到，`scripts/wpt_reftest_candidates.py` 只好整個濾掉用到它們的測試（`tests/wpt/README.md` 的「How the reftests were picked」）。

目標：
- testharness.js 測試由上游的 wptserve 提供，用上游的網域（`web-platform.test` 等）和固定的 port，和其他瀏覽器跑 WPT 時看到的網址一樣。
- 因為 server 而跳過的測試重新試跑：能跑的移出 skip 清單（通過或寫成 baseline），還需要別的 API 的照舊跳過、換成新的理由。
- `cargo test --workspace`（CI 設 `MACHOPO_SLOW_TESTS`）和 `python3 scripts/test.py wpt` 的用法不變。
- **不改 `../third_party/wpt`**：venv、設定檔、Python 的 bytecode 都放在 checkout 外面，跑完 checkout 連 ignored 的檔案都沒有多（`git status --porcelain --ignored` 是空的）。
- 第 4 步：WPT 的 reftest 也從 wptserve 載入，以 `/` 開頭的參考頁和樣式表不再擋住候選。

不在範圍內：
- **wptrunner 和 WebDriver**（`wpt run`）、wptreport 格式的結果和 wpt.fyi：要先替 kun 做 WebDriver（或寫一個 wptrunner 的 executor），工作量大得多（「之後的可能」）。
- **expectations 的格式**：照舊是 Chromium 的 `-expected.txt`（RFC 0008 的第 3 條），不換成 wptrunner 的 `.ini`。
- **要跑哪些測試**：照舊是 include 清單和 skip 清單，不改用 `MANIFEST.json`。
- **testdriver**（`testdriver-vendor.js`）：需要 runner 和頁面之間的通道，和 server 無關。
- **kun 自己的 reftest**（`tests/reftests/` 等非 WPT 的）：照舊從 `file:` 載入。
- **`net::test_server`** 本身：`net`、`browser` 等的網路測試照舊用它；只是不再當 WPT 的 server。
- HTTP/2（wptserve 的 `h2` server）、WebSocket、WebTransport 的測試：kun 還沒有對應的功能，wptserve 先關掉 `h2`（「詳細設計」）。

## 設計概要

**給跑測試的人**：指令不變。

```sh
python3 scripts/test.py wpt [FILTER...]
MACHOPO_UPDATE_EXPECTATIONS=1 python3 scripts/test.py wpt
```

差別：
- **要 Python 3 和網路（第一次）**：runner 第一次會在 `target/wptserve/venv/` 建一個 venv（交給 `wpt --venv`，「已決定的事項」第 8 條），用 pip 裝 wptserve 要的套件（`tools/serve/requirements.txt` 的 `cryptography`，wpt 另外一律裝 `dnslib` 和 `aioquic`，見 `details.md` 的「Python 環境」）；之後不再連網。第 0 步在 Linux 上實測，沒有 pip 的快取時第一次約 5 秒。CI 本來就裝了 Python 3.12。
- **不用改 `/etc/hosts`**：wptserve 的網域只在 kun 的 web test 模式裡對應到 `127.0.0.1`，系統的 DNS 和其他程式不受影響。
- **固定的 port**（wptserve 的預設：HTTP 8000、HTTPS 8443 等）：同一台機器上同時跑兩份 `test.py wpt` 會衝突，第二份會清楚地失敗（「另一個 run 的 wptserve 正在服務」），不會動到第一份。
- 測試的網址從 `http://127.0.0.1:<每次不同>/dom/nodes/x.html` 變成 `http://web-platform.test:8000/dom/nodes/x.html`，`.https.` 的測試是 `https://web-platform.test:8443/`；失敗時留下的 `-actual.txt` 和以前一樣在 `target/wpt-results/`，wptserve 的 log 也在那裡（`wptserve.log`）。
- **時間**：完整跑一次多約 1.2 秒，主要是 wptserve 的啟動和 venv 的檢查（2026-10-08 在 Linux 上量，`progress.md` 的「做過的檢查」）。

**給加測試的人**：`tests/wpt/README.md` 的流程不變（加進 include 清單、`MACHOPO_UPDATE_EXPECTATIONS=1` 跑一次、看失敗的原因）。`.sub`、`.headers`、`.py`、`.https.` 的測試不必再因為 server 而跳過；WPT 的 reftest 的參考頁和樣式表可以以 `/` 開頭。

**給改 kun 的人**：`net` 多一份 `NetworkContextParams`（Chromium 的 `network::mojom::NetworkContextParams`），只有 web test 會設：host 的對應規則和額外信任的根憑證。一般的瀏覽器視窗不設，行為不變。

## 詳細設計

### 啟動和停止 wptserve

由 Rust 的 runner 自己啟動（「已決定的事項」第 1 條）。shell 的 `wptserve` 模組（`4ff8d6e:crates/shell/src/wptserve.rs`，對應 Chromium blinkpy 的 `servers/wptserve.py`）執行 `python3 -u ../third_party/wpt/wpt --venv target/wptserve/venv serve --no-h2 --config target/wptserve/config.json`（Windows 上是 `python`），cwd 是 `target/wptserve/`：

1. **venv**：`target/wptserve/venv/`，交給 `wpt --venv`（「已決定的事項」第 8 條）：它自己建 venv、裝 requirements，已經裝好時不呼叫 pip（`tools/wpt/virtualenv.py` 的 `install_requirements`），換 WPT 的 commit 而 requirements 變了時會自動補裝。原本打算用 `python3 -m venv` 加上 pip 自己做，第 0 步實測 `wpt --venv` 就做得到。
2. **設定檔**：每次 run 寫到 `target/wptserve/config.json`，用 `wpt serve --config` 傳入（不放在 checkout 的根目錄，那裡的 `config.json` 會被 wptserve 自動讀到）：
   - `check_subdomains: false`：wptserve 啟動時會用 Python 的 urllib 連每個子網域，沒有改 hosts 時一定失敗。kun 的 host 對應只在 kun 裡面，所以關掉這個檢查。
   - `server_host: "127.0.0.1"`：起草時沒想到的，第 0 步實測才發現。不設時 wptserve 用 `browser_host`（`web-platform.test`）當 bind 的位址，系統解析不到，stash server 一啟動就失敗（`socket.gaierror`）。wptrunner 給 Firefox 和 Servo 的設定也是這一條。
   - `ports`：wptserve 的預設（HTTP 8000 和一個自動的、HTTPS 8443 和 8444、WebSocket 自動……），`h2` 不開（`--no-h2`）。
   - `aliases`：`/resources/testharnessreport.js` 指向 runner 寫好的 kun 版（`4ff8d6e:crates/shell/src/testharnessreport.js` 填上 `timeout_multiplier` 之後，寫到 `target/wptserve/resources/`）；`/kun/` 指向 `4ff8d6e:tests/testharness/`（runner 自己的測試頁）。單一檔案的 alias 的 `local-dir` 是那個檔案所在的**目錄**，不是檔案本身（第 0 步實測）。Chromium 用 alias 換掉 `testharnessreport.js`；wptrunner 是在 route builder 上另外加一條 static route（`tools/wptrunner/wptrunner/environment.py`），結果一樣。
3. **不寫進 checkout**：`PYTHONPYCACHEPREFIX` 設成 `target/wptserve/pycache`，Python 才不會在 `tools/` 底下留下 `__pycache__`（2026-10-07 起草時，只在 checkout 裡 import 一次 `tools/serve` 就留下了一個，`git status --ignored` 看得到）。venv 和設定檔本來就在 `target/`。
4. **log**：stdout 和 stderr 一起寫進 `target/wpt-results/wptserve.log`（「已決定的事項」第 11 條）。
5. **等它就緒**：輪詢三個固定的 port（8000、8443、8444），直到都連得上；wptserve 在那之前退出，或 180 秒（第一次要建 venv、下載套件）還沒好，就印出 log 的最後 50 行，整個 run 失敗。第 0 步在 Linux 上實測，venv 已經裝好時約 1 秒。
6. **停止**：runner 結束時（包括 panic，`WptServe` 的 `Drop`）結束 wptserve 和它啟動的 server process：Unix 上 wptserve 自成一個 process group（`process_group(0)`），結束整個 group（SIGKILL，和 Chromium 的 `kill_process` 一樣）；Windows 上 wptserve 在設了 `KILL_ON_JOB_CLOSE` 的 Job Object 裡（`win32job` crate：kun 自己的 crate 不寫 `unsafe`），另外用 `taskkill /f /t`。第 0 步實測，**只對 wptserve 的主 process 送 SIGTERM 或 SIGKILL 會留下 12 個佔著 port 的子 process**；對整個 process group 送訊號，或對主 process 送 SIGINT（wptserve 當成 `KeyboardInterrupt`，自己結束各個 server）才會全部結束。run 到一半 wptserve 自己退出時，run 失敗（結果不可信）。
7. **上次留下的 wptserve**（「已決定的事項」第 9 條）：`target/wptserve/pid` 記下 wptserve 的 pid（也是 process group 的 id）和 runner 自己的 pid。下一次啟動時，pid 檔在、記下的 runner 已經不在、而且固定的 port 有一個被佔用，才結束那個 process group。記下的 runner 還在，表示是另一個正在跑的 run：不動它，這次 run 失敗。原本照 Chromium 只看 pid 檔和 port，第 2 步實測時發現這樣會把另一個還在跑的 run 的 wptserve 殺掉（Chromium 的 `server_base.py` 也一樣），所以多記 runner 的 pid。Windows 上 Job Object 保證 runner 結束時 wptserve 跟著結束，不會留下，所以不殺，port 被佔用就失敗。
8. **checkout 不變**（「已決定的事項」第 10 條）：結束 wptserve 之後，`git -C ../third_party/wpt status --porcelain --ignored` 不是空的就讓 run 失敗，列出多出來的檔案。

`.any.js`、`.window.js` 的包裝改由 wptserve 產生，`4ff8d6e:crates/shell/src/testharness.rs` 的 `wrapper` 和 runner 註冊 route 的部分刪掉；判斷「哪些檔案是測試、在 window 裡跑哪個網址」的部分照舊（runner 還是要自己列出測試，不讀 `MANIFEST.json`），多了 `Test::https`：檔名有 `.https.` 這一段的測試從 HTTPS 的 port 載入（wptrunner 讀 manifest 也是看檔名）。`net::test_server` 的目錄模式只有 WPT 的 runner 用，一起刪掉。

**HTTPS 的回應靠關閉連線結束、沒有 `close_notify`**：第 2 步才發現的。wptserve（Python 的 `ssl`）的 HTTPS 回應沒有 `Content-Length`，body 到連線關閉為止，而且關閉時不送 TLS 的 `close_notify`。rustls 把這種結尾當成可能被截斷，回報 `UnexpectedEof`，kun 原本把整個載入當成失敗，`.https.` 的測試因此等到 deadline。Chromium 對 HTTP/1.x、沒有長度、靠關閉連線結束的 body 接受沒有 `close_notify` 的結尾，kun 照做（`net` 的 `HttpTransport::get`）；有 `Content-Length` 或 chunked 的 body 提早斷掉照樣是錯誤。真實的網站也有這種 server，所以這是 `net` 的修正，不只是測試的需要。

### kun 的 network 設定

`NetworkContext::start` 多一個參數 `NetworkContextParams`（預設是空的，瀏覽器視窗用預設）；headless 的 web test（`headless::run_testharness`、WPT 的 reftest 用的 `headless::render_web_test`）傳入 `wptserve::network_params`：

- **host 的對應規則**（Chromium 的 `--host-resolver-rules`，`net::HostMappingRules`）：照 wptrunner 給 Chrome 的規則（`tools/wptrunner/wptrunner/browsers/chrome.py`），`MAP nonexistent.*.test ^NOTFOUND, MAP *.test 127.0.0.1`：wptserve 的 62 個網域（`details.md` 的「wptserve 的網域」）都在 `*.test` 裡面，不必一個一個列；`nonexistent` 的子網域解析失敗。規則依序比對，第一條符合的生效，`*` 可以跨過 `.`，和 Chromium 一樣；都不符合的照常問系統。用 reqwest 的自訂 `dns_resolver` 實作（「已決定的事項」第 6 條），只在有規則時才換掉 reqwest 預設的 resolver。IDN 的子網域以 punycode 比對（kun 的 `url` 會先轉成 punycode）。
- **額外信任的根憑證**：`../third_party/wpt/tools/certs/cacert.pem`（wptserve 預設的 `pregenerated` 憑證由它簽發，目前到 2027-09-12）。用 reqwest 的 `tls_certs_merge`，加在系統的根憑證上，憑證照常檢查，只是多信任這一張；不關掉檢查（「已決定的事項」第 3 條）。

做這兩項時發現，`net::http` 把憑證錯誤分類成 `NetErrorCode` 的那段從來沒有生效過：rustls 的錯誤包在兩層 `io::Error` 裡（hyper-util 的包著 tokio-rustls 的），`io::Error::source()` 跳過它包住的錯誤，所以憑證錯誤一律變成 `ConnectionFailed`。改成走訪錯誤時，遇到 `io::Error` 就往它包住的錯誤走（`next_source`）。`net::test_server` 多了 HTTPS（`start_https`），`net` 的測試用 WPT 的憑證（「已決定的事項」第 7 條）。

這兩項都只在 Browser 這一側的 `NetworkService` thread，不經過 renderer，`protocol` 不變。

### 換掉之後的 baseline 和 skip 清單

換 server 之後，baseline 只有 server 造成的差別，都變好：wptserve 照副檔名和 `.headers` 給 `Content-Type` 和 charset（`net::test_server` 給 `text/plain`），`.py` 提供的資源載得到。`.any.js` 的包裝頁由上游產生，結果和 kun 的版本一樣。一個測試因此反而變慢（它的 `.py` 把 module 扣住，等 kun 沒有的 `fetch` 放行，每次多等 30 秒），移進 skip 清單。

skip 清單裡理由和 server 有關的（`.sub`、`.py`、pipe、別的 host、HTTPS、port 每次不同），和理由是 `location` 的（RFC 0019 已經有了），全部重新試跑：能跑的移出（通過，或照失敗寫成 baseline；以前因為 port 而寫不住的，現在寫得住），還需要 kun 沒有的東西的（`fetch`、workers、`postMessage`……）照舊跳過、換成新的理由（`4ff8d6e:tests/wpt/testharness-skip.txt` 的「RFC 0020, step 3」）。RFC 0015 的第 4 階段因為 `.py` 和 pipe 而直接沒有列進 include 清單（不是跳過）的 `the-script-element/` 的檔案和 `moving-between-documents/`，收尾時發現也一起試跑：能跑的加進清單，其他的照舊不列，理由換成 `document.write` 和 `postMessage`（`4ff8d6e:tests/wpt/testharness-include.txt` 的「RFC 0020」那一組）。數字在 `progress.md` 和 `tests/wpt/README.md`。

### reftest

WPT 的 reftest 改從 `http://web-platform.test:8000/<path>` 載入（`4ff8d6e:crates/shell/tests/reftest.rs` 的 `Pages`，用 `headless::render_web_test` 帶著 `wptserve::network_params`），以 `/` 開頭的樣式表、script 都載得到；id、`reftest-include.txt` 和 `reftest-expectations.txt` 照舊用 checkout 裡的路徑（「已決定的事項」第 12 條）。kun 自己的 reftest 照舊從 `file:` 載入。wptserve 只在篩選後有 WPT 的 reftest 要跑時才啟動，log 寫進 `target/reftest-results/wptserve.log`，結束時一樣檢查 checkout 沒變；它和 testharness 的 runner 共用 `target/wptserve/` 和固定的 port，同時跑時第二個失敗（`cargo test` 一次跑一個 test binary，不會碰到）。

參考頁照舊由 runner 讀檔決定（chain、`fuzzy`），以 `/` 開頭的參考頁相對於「包含那個檔案的網站根目錄」（WPT 的 checkout）解析（`shell::reftest` 的 `Metadata::read` 的 `site_root`）；kun 自己的 reftest 沒有網站根目錄，以 `/` 開頭照舊是錯誤。`.xht` 由 wptserve 以 `application/xhtml+xml` 提供，和 `net` 對 `file:` 給的一樣，kun 照舊用 HTML parser 解析並拿掉 CDATA 的標記（`crates/renderer/src/document.rs` 依 MIME 判斷）。換了載入方式之後，原本的結果一個也沒變，包括用 `/fonts/ahem.css` 的（從 `file:` 載入時是空的樣式表，現在載得到，結果一樣）。

`scripts/wpt_reftest_candidates.py` 不再拒絕以 `/` 開頭的參考頁和樣式表，在用過的目錄裡重新篩（「已決定的事項」第 13 條）：通過的加進清單；`css-sizing/dynamic-change-inline-size-004.html` 從 `file:` 載入也一樣失敗（它用 Window 的具名屬性 `inner` 取元素，kun 沒有，script 丟出例外，`reftest-wait` 一直不拿掉），篩選腳本只看 `.` 之後的名稱、抓不到，沒有加。

### CI

`ci.yml` 的 test job 不用多一個步驟：runner 第一次跑時自己建 venv。加的是 venv 的快取（`actions/cache`，`kun/target/wptserve/venv`，key 是平台、Python 的版本和 wpt 會裝的 requirements 檔的 hash），免得每天都從 PyPI 下載 `cryptography`。整份 RFC 只在 Linux 上驗證（「已決定的事項」第 5 條）：macOS 和 Windows 不在 merge 前另外跑，merge 回 `main` 之後由每天的 CI 跑，出了問題再開 branch 修。平台相關、Linux 上看不到的地方（之後在那裡出問題時先看這些）：Windows 結束子 process 要用 Job Object，Python 的指令是 `python`（setup-python 在 Windows 上不提供 `python3`）；venv 的路徑交給 wpt，不用自己處理。起草時以為 macOS 和 Windows 的 `multiprocessing` 用 `spawn`、Linux 用 `fork`，wptserve 啟動 server 的方式會不同；第 0 步讀程式發現 wptserve 在每個平台都明確用 `spawn`（`tools/serve/serve.py` 的 `run`），這一項的平台差別比預期小。

### 測試

- `net`：host 的對應規則（對應到的、`nonexistent` 解析失敗的、沒設規則時不受影響的）、額外的根憑證（WPT 的憑證通過、名稱不符和沒設時被拒絕）、HTTPS body 的結尾，用 `net::test_server`（加上 HTTPS）和 WPT 的 `tools/certs`（「已決定的事項」第 7 條）。
- runner：`4ff8d6e:tests/testharness/` 原本的 6 個測試頁照舊（由 `/kun/` 的 alias 提供），再加 3 頁證明真的經過 wptserve：`substitution.sub.html`（`.sub` 的替換和頁面的網址）、`headers.html`（`.headers` 給的 charset）、`secure.https.html`（從 `https://web-platform.test:8443/` 載入，kun 解析得到網域、信任 WPT 的根憑證）。`shell::wptserve` 和 `shell::reftest` 的規則有單元測試。
- 整個 WPT 和 reftest：換 server 前後逐一比對（「換掉之後的 baseline 和 skip 清單」「reftest」）。
- checkout 不變：每次 run 之後 runner 檢查 `git -C ../third_party/wpt status --porcelain --ignored` 是空的（「已決定的事項」第 10 條）。
- wptserve 的 process 的管理（強制結束 runner、同時跑兩份）靠手動確認，記在 `docs/test-gaps.md` 的「只做過手動確認」；驗收和 mutation test 的結果在 `progress.md`。

### 分階段

都在 2026-10-08 做完，一次 merge 回 `main`：

0. **原型和實測**（只在 Linux 上）：手動跑一次 `wpt serve`，量啟動時間、確認 checkout 不變、確認能結束所有子 process，用 kun 載入測試頁（`details.md` 的「第 0 步的實測」）。
1. **`NetworkContextParams`**：host 的對應規則和額外的根憑證，加上 `net` 的測試。
2. **testharness 改用 wptserve**：`shell::wptserve`、runner 改用它、刪掉 kun 的 `.any.js` 包裝、CI 的 venv 快取；全部 WPT 結果和換之前比對。
3. **移出 skip 清單**：因為 server 而跳過的測試逐一試跑（「換掉之後的 baseline 和 skip 清單」）。
4. **reftest 改用 wptserve**（「reftest」）。

## 缺點

- **測試多了 Python 和 pip 的依賴**：本機第一次跑要連網下載 `cryptography`（有 C 擴充，平台沒有 wheel 時要編譯）；Python 的版本太新或太舊時，可能和 WPT 釘住的套件版本不合。原本的 runner 只要 Rust。
- **多一個外部 process 要管**：啟動約 1 秒，而且 runner 被強制結束時會留下佔著 port 的 process（Unix 上由下一次 run 清掉，Windows 上由 Job Object 跟著結束）。
- **固定的 port**：同一台機器上不能同時跑兩份（`test.py wpt` 和 WPT 的 reftest 也不能同時跑）；和其他用 8000 port 的程式衝突。
- **kun 多一份只給測試用的 network 設定**：雖然預設是空的，仍然是一條「測試和正式行為不同」的路徑，要靠測試守住預設值。
- **WPT 換 commit 時，server 也跟著換**：以前換 commit 只換測試檔，之後連 wptserve 的行為都可能變；但這正是想要的（和上游一致）。

## 理由和其他做法

完整的比較在 `details.md` 的「沒選的做法」。

- **D1 誰啟動 wptserve**：
  - **A：Rust 的 runner**（選了，「已決定的事項」第 1 條）：`cargo test --workspace` 加 `MACHOPO_SLOW_TESTS` 照舊能跑完整的 WPT，CI 不用改步驟；Chromium 的 `run_web_tests` 也是自己啟動 wptserve。代價是 Rust 裡要管 Python 的 process。
  - **B：`scripts/test.py` 啟動，把網址用環境變數傳給 runner**：Python 管 Python 比較自然；但 `cargo test` 單獨跑不了 WPT，CI 要多一步 `test.py wpt`，而且 `MACHOPO_SLOW_TESTS` 在 `cargo test` 裡跑到 WPT 時要決定是跳過還是失敗。
- **D2 wptserve 的網域怎麼解析**：
  - **H1：kun 的 host 對應規則**（選了，第 2 條）：不動系統，不用 admin 權限；和 Chromium 跑 web test 的做法一樣。只對 kun 有效，wptserve 的 `check_subdomains` 要關掉。
  - **H2：hosts 檔**（`wpt make-hosts-file`，上游文件的做法）：要 admin 權限改系統檔案，本機和 CI 都要；忘了改就是難懂的失敗。
  - **H3：wptserve 的 `--dns`**（內建的 DNS server，用 `dnslib`）：kun 還是要能指定 DNS server，等於 H1 再多一個 process 和 port。
- **D3 HTTPS**：
  - **T1：多信任 WPT 的根憑證**（選了，第 3 條）：憑證照常檢查，測試和真實的 TLS 路徑一樣。
  - **T2：web test 時不檢查憑證**：最簡單，但 kun 的憑證錯誤處理（`net::http` 依 rustls 的錯誤分類）在 web test 裡就測不到；wptrunner 給 Chrome 的也不是全關，是 `--ignore-certificate-errors-spki-list` 只放行這一張的公鑰。
  - **T3：先只做 HTTP**：`.https.` 的測試和 secure context 照舊跳過；工作少一點，但 T1 本身很小。
- **D4 port**：
  - **固定**（選了，第 4 條）：網址出現在失敗訊息裡時 baseline 寫得住；和 wptrunner、Chromium 一樣。
  - **自動**（`"auto"`）：可以同時跑多份，但網址每次不同，和現在一樣寫不成 baseline。
- **整個換成 `wpt run`（wptrunner 加上 WebDriver）**：見「動機」的「不在範圍內」；做了這份之後，server 和網域、憑證的設定都和上游一致，那一步只剩 runner。
- **繼續擴充 `net::test_server`**（自己做 `.sub`、`.headers`）：`.sub` 的替換有不少語法（`{{host}}`、`{{ports[https][0]}}`、`{{domains[www]}}`、`{{GET[x]}}`……），`.py` handler 只能跑 Python，多個 host 和 HTTPS 也還是要做這份的 network 設定；越做越像重寫一份 wptserve。不選。
- **不做**：因為 server 而跳過的測試一直跳過，之後的 fetch、CORS、跨來源的 iframe 會讓這個清單越來越長。

## 先例

- **Chromium**：`run_web_tests` 自己啟動 wptserve（`blinkpy/web_tests/servers/wptserve.py`，`--skip-venv-setup`，套件由 vpython3 準備；pid 檔加上 process group 結束留下的 wptserve，`server_base.py`），用 `aliases` 換掉 `testharnessreport.js`；content_shell 以 `--host-resolver-rules` 把 `*.test` 對應到本機、`--ignore-certificate-errors-spki-list` 放行 WPT 的憑證，不改 hosts 檔。這份照它的 server 和 network 的做法；runner 和 baseline 本來就照它（RFC 0008）。
- **wptrunner**（Firefox、Servo、Chrome 在 wpt.fyi 上的跑法）：同樣啟動 wptserve，hosts 由使用者先設好（`wpt make-hosts-file`），或由 wptrunner 傳給瀏覽器的設定處理（例如 Firefox 的 `network.dns.localDomains`）。
- **Servo**：wptrunner 加上自己的 `testharnessreport.js`，`console.log` 印出結果（RFC 0008 的「先例」）。

## 已決定的事項

1. **D1：Rust 的 runner 自己啟動和停止 wptserve**（2026-10-08）：`cargo test --workspace` 加上 `MACHOPO_SLOW_TESTS` 照舊能跑完整的 WPT，CI 的步驟不變；和 Chromium 的 `run_web_tests` 一樣由跑測試的那一方啟動。不選「`scripts/test.py` 啟動、用環境變數把網址傳給 runner」：`cargo test` 單獨跑不了 WPT，CI 要多一步。代價是 Rust 裡要管 Python 的 process（Unix 的 process group、Windows 的 Job Object）。
2. **D2：wptserve 的網域由 kun 的 host 對應規則解析**（2026-10-08）：只在 web test 的 `NetworkContextParams` 裡，把 wptserve 的網域對應到 `127.0.0.1`、`nonexistent` 的解析失敗；wptserve 的 `check_subdomains` 關掉。和 Chromium 給 content_shell 的 `--host-resolver-rules` 一樣。不選 hosts 檔（`wpt make-hosts-file`）：每台機器都要用 admin 權限改系統檔案，影響整台機器；也不選 wptserve 的 `--dns`：kun 還是要能指定 DNS server，多一個 process 和 port。
3. **D3：HTTPS 照常檢查憑證，只多信任 WPT 的根憑證**（2026-10-08）：web test 的 `NetworkContextParams` 加上 `tools/certs/cacert.pem`。不選「web test 時不檢查憑證」：`net::http` 的憑證錯誤處理在 web test 裡就測不到，Chromium 也只放行這一張（`--ignore-certificate-errors-spki-list`）；不選「先只做 HTTP」：`.https.` 的測試和 secure context 會繼續跳過，而多信任一張根憑證的工作很小。
4. **D4：固定的 port，沿用 wptserve 的預設**（2026-10-08）：HTTP 8000、HTTPS 8443 和 8444 等固定，wptserve 預設是 `"auto"` 的照舊自動；`h2` 不開。失敗訊息裡有網址的測試才寫得進 baseline（當時因為 port 每次不同而跳過的幾個，第 3 步之後都寫成 baseline 或通過；`concurrent-failed-imports.any.js` 改成因為每次不同的 uuid 跳過），和 wptrunner、Chromium 一樣。不選全部自動：網址每次不同，和 `net::test_server` 一樣寫不住。代價：同一台機器上不能同時跑兩份，port 被佔用時 runner 印出 wptserve 的錯誤、整個 run 失敗。
5. **平台：整份 RFC 只在 Linux 上驗證**（2026-10-08 決定第 0 步只在 Linux 上實測；同一天第 2 步做完之後改成整份 RFC）：每一步的檢查和驗收都只在 Linux 上做，merge 前不對 `0020` branch 手動觸發 CI，RFC 完成後也不另外在 macOS 和 Windows 上手動確認。merge 回 `main` 之後，每天的 CI 照常在三個平台上跑；那裡、或之後在 macOS、Windows 上使用時出了問題，再開 branch 修。原本的做法是第 2 步 merge 前手動觸發 CI、RFC 完成後在兩個平台上手動確認，改掉的理由：手上只有 Linux 的環境，平台相關的部分（Windows 的 Job Object 和 `python` 指令、macOS 的憑證驗證）範圍小、出了問題也只影響跑 WPT，不影響瀏覽器本身，等每天的 CI 抓到再修就夠了。代價：`main` 上的 CI 可能會在 macOS 或 Windows 上失敗一段時間（`docs/limitations.md` 的「平台和建置」）。不選「第 0 步在三個平台上各做一次」：設計不取決於平台的差別。
6. **host 的對應規則用自訂的 `dns_resolver`，照 Chromium 的規則字串比對**（2026-10-08，第 1 步）：`MAP <pattern> <IP>` 和 `MAP <pattern> ^NOTFOUND`，依序比對、第一條符合的生效，都不符合的照常問系統；只在有規則時換掉 reqwest 的 resolver，預設的路徑不變。不選 reqwest 的 `resolve`（一個網域一條）：62 個網域要一條一條列，而且表達不了「`nonexistent.*` 解析失敗」，那些網域會落到系統的 DNS，結果看機器的設定。
7. **`net` 的 TLS 測試用 WPT 的 `tools/certs`**（2026-10-08，第 1 步）：`net` 的測試讀 `../third_party/wpt/tools/certs/` 的 `cacert.pem`、`web-platform.test.pem` 和 `.key`，測的就是 web test 實際要信任的那一張，也不用多一個 dev-dependency。代價：`net` 的這個測試要有 WPT 的 checkout（`dom_api`、`html` 的測試本來就要），而且那張憑證 2027-09-12 到期，換 WPT 的 commit 時跟著換（`details.md` 的「wptserve 的設定」）；到期前沒換，這個測試和 WPT 的 HTTPS 測試會一起失敗。不選 dev-dependency（`rcgen`）在測試時產生：多一個依賴，測的也不是 web test 用的那張。
8. **venv 交給 `wpt --venv`**（2026-10-08，第 2 步）：runner 執行 `python3 ../third_party/wpt/wpt --venv target/wptserve/venv serve ...`，venv 的建立、安裝和「requirements 變了要補裝」都由 wpt 自己做（第 0 步實測可行）。不選 runner 自己 `python3 -m venv` 加上 pip、再用 `--skip-venv-setup`：每次啟動省約 0.55 秒（實測 1.97 秒和 1.43 秒），也只裝 `cryptography`，但「requirements 變了」要自己偵測，Windows 的 venv 路徑也要自己處理。Chromium 用 `--skip-venv-setup`，是因為它的套件由 vpython3 事先準備好，kun 沒有對應的東西。代價：wpt 一律多裝 `aioquic` 和 `dnslib`（`details.md` 的「Python 環境」）。
9. **上次留下的 wptserve 照 Chromium 用 pid 檔結束，只在記下的 runner 已經不在、而且 port 被佔用時才動手**（2026-10-08，第 2 步）：Unix 上 wptserve 自成一個 process group，runner 把它的 id 和自己的 pid 寫進 `target/wptserve/pid`；下一次啟動時，pid 檔還在、記下的 runner 已經不在、而且 wptserve 的固定 port 有一個被佔用，才結束那個 process group，再確認 port 都空著。記下的 runner 還在，就是另一個正在跑的 run：不動它，這次 run 失敗。Chromium（blinkpy 的 `server_base.py`）只看 pid 檔：pid 被重用時可能結束不相干的 process group，也會殺掉另一個正在跑的 run 的 wptserve。決定時（同一天）照 Chromium 只多看 port；做完第 2 步的手動確認時發現會殺掉另一個 run 的 wptserve，才多記 runner 的 pid。Windows 上用設了 `KILL_ON_JOB_CLOSE` 的 Job Object：runner 怎麼結束，系統都會結束 job 裡的 process，不會留下。不選「port 被佔用時只印出提示」：runner 每次被強制結束都要人手動處理。
10. **checkout 不變由 runner 跑完時檢查**（2026-10-08，第 2 步）：runner 結束 wptserve 之後執行 `git -C ../third_party/wpt status --porcelain --ignored`（實測 0.1 到 0.3 秒），不是空的就讓 run 失敗，列出多出來的檔案。本機和 CI 每次 run 都檢查，寫進 checkout 的當下就知道。不選加進 `ci.py third-party`：CI 在測試之前跑它，要另外加一步，本機也要等 CI 才發現。Chromium 沒有對應的檢查：它的 WPT 在自己的 tree 裡。
11. **wptserve 的 log 寫進 `target/wpt-results/wptserve.log`**（2026-10-08，第 2 步）：stdout 和 stderr 寫進同一個檔案（wpt 兩邊都寫，合在一起才看得出先後），啟動失敗、或 run 到一半 wptserve 自己結束時，印出最後 50 行。和 Chromium 一樣寫進輸出目錄、不用 pipe（Chromium 的註解：wptserve 的 log 很多，pipe 滿了它會卡住）。不選接到 runner 的輸出：每個請求一行，會洗掉測試的結果；也不選 pipe 接起來丟掉：要另外開 thread 讀，出事時也沒有 log。
12. **WPT 的 reftest 的 id 和 expectations 照舊用路徑**（2026-10-08，第 4 步）：`reftest-include.txt` 照舊列 checkout 裡的路徑，id 照舊是 `wpt/<路徑>`，`reftest-expectations.txt` 也照舊以路徑為 key，只把載入從 `file:` 換成 `http://web-platform.test:8000/<路徑>`。Chromium 也是：WPT 的 reftest 經過 wptserve 載入，expectations 用測試的路徑（`external/wpt/...`）。不選改用網址當 id：清單和 expectations 要全部重寫，看不出好處。
13. **第 4 步重新篩一次候選，只在已經用過的目錄裡**（2026-10-08，第 4 步）：`scripts/wpt_reftest_candidates.py` 不再拒絕以 `/` 開頭的參考頁和樣式表，在 P1 到 RFC 0003 用過的目錄裡重新篩，新的候選照慣例人工挑過、加進 `reftest-include.txt`（結果見 `progress.md`）。限制候選的主要是支援的 CSS property 和 script 的成員，所以多出來的不多。不選「只換載入、不加測試」：拿掉了限制卻看不出效果。

## 之後的可能

- **wptrunner 和 WebDriver**：替 kun 做 WebDriver（或 wptrunner 的 executor），改用 `wpt run`；結果可以輸出成 wptreport、和其他瀏覽器在 wpt.fyi 上比較。
- **`// META: variant=` 和 `<meta name=variant>`**（RFC 0008 的第 55 條）：wptserve 已經能提供帶 query 的網址，runner 列出 variant 就能跑。
- **testdriver**：runner 和頁面之間有了通道之後（RFC 0008 的「之後的可能」），`testdriver-vendor.js` 也用 `aliases` 換成 kun 的。
- **WebSocket 和 HTTP/2 的測試**：kun 有了對應的功能時，打開 wptserve 的 `ws`、`h2`。
