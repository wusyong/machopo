# RFC 0020 的細節：用上游的 wptserve 跑 WPT

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

除非另外註明，wptserve 的行號和設定都是 `../third_party/wpt` 在 `0f195499`（2026-09-30）的版本。

## 做之前的跑法和上游的差別

2026-10-07 整理。「上游」指 `wpt run`（wptrunner 加上 wptserve）。

| | 上游 | kun（RFC 0008 起） |
| --- | --- | --- |
| 提供檔案的 server | wptserve（`tools/serve/serve.py`） | `net::test_server` 的目錄模式，以 checkout 為根；`.any.js`、`.window.js` 的包裝由 `4ff8d6e:crates/shell/src/testharness.rs` 產生 |
| 控制瀏覽器、收結果 | wptrunner 透過 WebDriver（或各瀏覽器的 executor），讀 `add_completion_callback` 的結果 | Rust 的 runner（`4ff8d6e:crates/shell/tests/wpt_testharness.rs`）用 headless 的 kun；kun 的 `testharnessreport.js` 經過 `testRunner` hook 交出結果 |
| 預期結果 | wptrunner 的 `.ini` metadata | Chromium 的 `-expected.txt`（`4ff8d6e:tests/wpt/baselines/`） |
| 跑哪些測試 | `MANIFEST.json` 加上 include / exclude | `tests/wpt/` 的 include 清單和 skip 清單 |
| 逾時 | `explicit_timeout`，runner 呼叫 `timeout()` | harness 自己計時，runner 另外有 deadline（RFC 0008 的第 54 條） |
| 網域和 port | `web-platform.test` 等，固定的 port（HTTP 8000、HTTPS 8443……） | `127.0.0.1`，每次不同的 port |
| HTTPS、跨來源、`.sub`、`.headers`、`.py`、pipe | 有 | 沒有 |
| reftest | 經過 wptserve 載入，wptrunner 截圖比對 | 從 `file:` 載入；chain 和 `fuzzy` 照 wptrunner 重寫（`4ff8d6e:crates/shell/src/reftest.rs`） |
| 結果的格式 | wptreport JSON，可以上傳 wpt.fyi | 沒有 |

這份 RFC 只換掉「提供檔案的 server」和「網域和 port」兩列，連帶解決「HTTPS、跨來源……」那一列；reftest 在第 4 步。

## 因為 server 而跳過的測試

2026-10-07 的 `4ff8d6e:tests/wpt/testharness-skip.txt`（161 行，不含註解和空行）裡，理由和 server 有關的：

- **「wptserve's `.sub` substitution」那一組**：5 行（`dom/nodes`、`dom/events`、`domparsing` 的 `.sub.` 測試）。
- **「RFC 0015: what the test server doesn't do of wptserve's」那一組**：22 行（`.py` handler、`.sub`、pipe、別的 host、HTTPS；其中 `modulepreload-*referr*` 三行失敗訊息裡有 port）。
- **「RFC 0015's stage 2」那一組的一部分**：`.sub.` 的 `dynamic-import` 測試、pipe 和 HTTPS 的；同一組也有 workers、`Blob`、`fetch` 等其他 API 的，換了 server 也還是要跳過。
- **因為 port 每次不同而寫不成 baseline 的**：`concurrent-failed-imports.any.js`、`import-meta-resolve-importmap.html`。

合計約 30 行。其中有些換了 server 之後還是會因為其他 API（CORS 照 RFC 0015 的 G1 本來就會失敗、`location`、workers）而失敗或逾時：它們從「不能跑」變成「能跑、有 baseline」，或換成新的跳過理由。第 3 步實際試跑了 75 行（上面這些，加上理由是 `location` 的）：結果在 `progress.md` 和 `tests/wpt/README.md`。

reftest 這一側，`scripts/wpt_reftest_candidates.py` 當時拒絕以 `/` 開頭的網址（`/fonts/ahem.css` 除外）。第 4 步拿掉這個限制，在用過的目錄裡重新篩，多出來的候選很少（`progress.md`）：限制候選的主要是支援的 CSS property 和 script 的成員。

## wptserve 的設定

`tools/serve/serve.py` 的 `ConfigBuilder._default`（1425 行起）：

- `browser_host`：`web-platform.test`；`alternate_hosts`：`{"alt": "not-web-platform.test"}`。
- `ports`：`http: [8000, "auto"]`、`https: [8443, 8444]`、`h2: [9000]`、`ws`、`wss`、`webtransport-h3`、`http-local`、`http-public`、`https-local`、`https-public` 都是 `auto`、`dns: [8053]`。`"auto"` 由 wptserve 啟動時挑空的 port（`tools/wptserve/wptserve/config.py` 的 `_get_ports`），測試用 `.sub` 的 `{{ports[http][1]}}` 這種寫法讀到實際的值。
- `check_subdomains: True`：啟動時用 urllib 連每個網域（`check_subdomains`，1080 行），連不上就 `sys.exit(1)` 並提示改 hosts。kun 的對應規則只在 kun 裡面，所以要設成 `false`。
- `ssl.type: "pregenerated"`：`tools/certs/web-platform.test.pem` 和 `.key`，由 `tools/certs/cacert.pem` 簽發。2026-10-07 時 `web-platform.test.pem` 的 `notAfter` 是 2027-09-12；上游用 `wpt regen-certs` 定期更新，換 WPT 的 commit 時跟著換。`openssl` 這個類型才會寫 `_certs/`，`pregenerated` 不寫。
- `aliases`：`[{"url-path": ..., "local-dir": ...}]`；`url-path` 以 `/` 結尾時掛整個目錄，否則掛單一檔案（`get_route_builder`，994 行）。
- 命令列：`--config`（外部設定檔，會和預設值合併）、`--no-h2`、`--dns`、`--alias_file`、`--exit-after-start`、`--verbose`（`get_parser`，1543 行）。

### Python 環境

`wpt serve` 由 `tools/wpt/wpt.py` 啟動，`tools/serve/commands.json` 設了 `"virtualenv": true`：沒有 `--venv` 時，venv 建在 checkout 裡的 `_venv3/`（`tools/wpt/wpt.py` 的 `venv_dir`）。`--venv <path>` 指定位置，`--skip-venv-setup` 跳過安裝。`install_requirements`（`tools/wpt/virtualenv.py` 171 行）先用 `pkg_resources` 的 working set 檢查，已經滿足就不呼叫 pip。

`tools/serve/requirements.txt` 只有 `cryptography==50.0.1`（給 `.py` handler 用；起草時寫成也有 `dnslib`，第 0 步核對時更正）。`tools/serve/commands.json` 的 `conditional_requirements` 讓 `--dns` 再裝 `requirements_dns.txt`（`dnslib==0.9.26`）、`--webtransport-h3` 再裝 `tools/webtransport/requirements.txt`（`aioquic==1.2.0`）；但 `tools/wpt/wpt.py` 的 `install_command_flag_requirements` 檢查的是旗標的名字在不在 `kwargs` 裡，沒給的旗標也在（值是 `False`），所以兩份一律都裝。venv 裡共 11 個套件，33 MB。其他依賴（`h2`、`html5lib`、`six`、`pywebsocket3`……）放在 `tools/third_party/`，由 `tools/localpaths.py` 加進 `sys.path`，不用 pip。

### 會寫進 checkout 的東西

`.gitignore` 列了 `_venv*/`、`/MANIFEST.json`、`/_certs`、`/config.json`、`*.py[co]`。照 README 的做法，venv 用 `--venv` 放到 `target/`，設定檔用 `--config` 放到 `target/`，不產生 `MANIFEST.json`（`wpt serve` 不需要它），`pregenerated` 的憑證不寫 `_certs/`；剩下的是 Python 的 bytecode。2026-10-07 起草時，在 checkout 裡 import 一次 `tools/serve`（只是為了讀設定），就留下了 `tools/serve/__pycache__/`（`git status --porcelain --ignored` 看得到），所以要設 `PYTHONPYCACHEPREFIX`（或 `PYTHONDONTWRITEBYTECODE=1`）。那個 `__pycache__` 已經刪掉，checkout 恢復成沒有 ignored 的檔案。

`ci.py third-party` 現在只檢查 HEAD 的 commit 和 `core.autocrlf`，不檢查工作目錄；ignored 的檔案也不會讓它失敗。

## 第 0 步的實測

2026-10-08，Fedora 44（Linux 7.2，x86_64），Python 3.14.7，`../third_party/wpt` 在 `0f195499`。照 `README.md` 的「啟動和停止 wptserve」手動做一次，原型（一個呼叫 `headless::run_testharness` 的 example）用完就刪了，沒有 commit。

**啟動的指令**：

```sh
PYTHONPYCACHEPREFIX=target/wptserve/pycache \
  python3 ../third_party/wpt/wpt --venv target/wptserve-venv \
  serve --config target/wptserve/config.json --no-h2
```

`target/wptserve/config.json`：

```json
{
  "server_host": "127.0.0.1",
  "check_subdomains": false,
  "aliases": [
    {"url-path": "/resources/testharnessreport.js", "local-dir": "<kun>/target/wptserve"},
    {"url-path": "/kun/", "local-dir": "<kun>/tests/testharness"}
  ]
}
```

**結果**：

- **`server_host` 一定要設**：只設 `check_subdomains: false` 時，wptserve 在 `run`（`tools/serve/serve.py` 1640 行起）用 `(config.server_host, port)` 啟動 stash server，`server_host` 沒設就是 `browser_host`，解析不到，`socket.gaierror: [Errno -2] Name or service not known`，exit 70。設成 `127.0.0.1` 之後，每個 server 都 bind 在 `127.0.0.1`。
- **venv**：`wpt --venv` 自己建 venv、裝 requirements（「Python 環境」）。沒有 pip 的快取（`PIP_NO_CACHE_DIR=1`）時，建 venv 加上下載、安裝、啟動到 `--exit-after-start` 結束約 5 秒；有快取約 4 秒；已經裝好時約 2 秒。`cryptography` 50.0.1 在 Python 3.14 上有 wheel，不用編譯。wpt 是在同一個 Python 裡「啟用」venv（`Virtualenv.activate` 改 `sys.path` 和 `sys.prefix`），不是用 venv 的 python 重新執行，所以 server 的子 process 的執行檔是系統的 `/usr/bin/python3`，venv 的套件經由 `spawn` 傳過去的 `sys.path` 找到。
- **就緒的時間**：從啟動到 8000 和 8443 都連得上約 1.06 到 1.08 秒（三次）。
- **process**：主 process 加上 12 個子 process（`multiprocessing` 的 resource tracker、stash 的 manager、HTTP 2 個、`http-local`、`http-public`、HTTPS 2 個、`https-local`、`https-public`、`ws`、`wss`），都在同一個 process group。`--no-h2` 關掉 HTTP/2，WebTransport 和 DNS 預設就關著。實際的 port：HTTP 8000、HTTPS 8443 和 8444 固定，其他每次不同。
- **結束**：對整個 process group 送 SIGTERM，1.5 秒內全部結束、port 釋放。只對主 process 送 SIGTERM 或 SIGKILL，12 個子 process 都留下來、繼續佔著 8000 和 8443；對主 process 送 SIGINT，wptserve 當成 `KeyboardInterrupt`，約 2 秒內自己結束全部。
- **checkout 不變**：每次 run 之後 `git -C ../third_party/wpt status --porcelain --ignored` 都是空的。`PYTHONPYCACHEPREFIX` 底下的 bytecode 有 22 MB。
- **wptserve 的行為**（`curl --resolve` 把網域對應到 `127.0.0.1`）：`http://web-platform.test:8000/` 的測試頁 200；`.sub.html` 的 `{{host}}`、`{{ports[http][0]}}` 換成 `web-platform.test:8000`；`/kun/meta.any.html` 由 wptserve 產生包裝頁，`META: script=` 照樣加上；`/resources/testharnessreport.js` 是 kun 的版本（和 `4ff8d6e:crates/shell/src/testharnessreport.js` 一字不差）；`https://www.web-platform.test:8443/` 用 `--cacert tools/certs/cacert.pem` 驗證通過（`ssl_verify_result` 0）。用 `127.0.0.1:8000` 當 host 也照樣回應，wptserve 不檢查 `Host`。
- **用現在的 kun 載入**（headless 的 `run_testharness`，網址用 `http://127.0.0.1:8000/`，因為 kun 還沒有 host 的對應規則）：`4ff8d6e:tests/testharness/` 的 `pass`、`fail`、`meta.any`、`plain.window` 和現在的結果一樣。`dom/nodes`、`dom/events`、`domparsing` 有 baseline 的 190 個測試，183 個和 baseline 一字不差；7 個不同，都是變好：`Document-characterSet-normalization-1`、`-2` 全過（網頁的 charset 從 `Content-Type` 來），`Document-contentType/contentType/` 的 `contenttype_bmp`、`_xml`、`_mimeheader_01`、`_mimeheader_02` 全過（`net::test_server` 對這些副檔名和 `.headers` 都回 `text/plain`），`Document-createElement-namespace` 的錯誤從「`contentType` 不對」變成後面的 namespace 不對。這些 baseline 在第 2 步換 server 時更新。`web-platform.test` 和 `https://127.0.0.1:8443/` 都等到 deadline 也沒有結果（解析不到、憑證不受信任），是第 1 步要做的。

## wptserve 的網域

`_subdomains`（1406 行）是 `www`、`www1`、`www2`、`天気の良い日`、`élève`，`_make_subdomains_product` 取一層和兩層的所有組合（重複也算，例如 `www.www`）：5 + 25 = 30 個子網域。加上 `web-platform.test` 和 `not-web-platform.test` 本身，兩個 host 共 62 個網域，都要對應到 `127.0.0.1`。`_not_subdomains` 是 `nonexistent` 和 `nonexistent.nonexistent`：wptserve 的 hosts 檔把它們對應到 `0.0.0.0`（Windows）或不列出，測試用它們檢查「解析不到的網域」，kun 的規則要讓它們解析失敗。

IDN 的子網域在網址裡是 punycode（`xn--n8j6ds53lwwkrqhv28a`、`xn--lve-6lad`），`url` crate 解析時會先轉換，所以對應規則以 punycode 比對。

## 沒選的做法

`README.md` 的「理由和其他做法」列了結論。補充：

- **D2 的 H2（hosts 檔）**：`wpt make-hosts-file` 產生的是 62 行加上 `nonexistent` 的；Linux 和 macOS 的 CI 可以 `sudo` 寫 `/etc/hosts`，Windows 的 runner 也能寫 `C:\Windows\System32\drivers\etc\hosts`。問題在本機：每個開發者都要用 admin 權限改一次系統檔案，而且改的是整台機器的解析；WPT 的 README 也提醒信任它的根憑證要在專用的環境裡做。H1 只影響 kun 的 web test 模式。
- **D3 的 T2（不檢查憑證）**：reqwest 有 `danger_accept_invalid_certs`。它讓 web test 和正式的 TLS 路徑分開，`net::http` 把 rustls 的錯誤分類成 `NetErrorCode` 的那段在 WPT 裡就跑不到。
- **D1 的 B（`test.py` 啟動）**：CI 的 test job 現在是一個 `cargo test --workspace`（設 `MACHOPO_SLOW_TESTS`，reftest、情境和 WPT 都在裡面）；B 要把 WPT 拆成另一個步驟，或讓 `wpt_testharness` 在沒有 server 的網址時失敗，`cargo test` 單獨跑就不完整了。

## 參考資料

- `../third_party/wpt/tools/serve/serve.py`：`ConfigBuilder`、`check_subdomains`、`get_route_builder`、`get_parser`、`make_hosts_file`。
- `../third_party/wpt/tools/wpt/wpt.py`、`tools/wpt/virtualenv.py`：venv 的位置和安裝。
- `../third_party/wpt/tools/certs/README.md`：測試用的憑證和信任它們的注意事項。
- `../third_party/wpt/tools/wptrunner/wptrunner/environment.py`：wptrunner 換掉 `/resources/testharnessreport.js` 的方式（244 行起）。
- RFC 0008（`rfcs/0008-dom-wpt/README.md`）的第 3、10、28、54、55 條。
