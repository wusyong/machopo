# RFC 0020 的進度：用上游的 wptserve 跑 WPT

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-08。完成：2026-10-08，Fedora 44（Linux x86_64，Rust 1.99.0）；只在 Linux 上驗證，macOS 和 Windows 交給 `main` 上每天的 CI（README 的「已決定的事項」第 5 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0 原型和實測（只在 Linux 上） | 完成 | 2026-10-08 |
| 1 `NetworkContextParams`：host 的對應規則和額外的根憑證 | 完成 | 2026-10-08 |
| 2 testharness 改用 wptserve | 完成 | 2026-10-08 |
| 3 移出 skip 清單 | 完成 | 2026-10-08 |
| （第 3 步找到的 bug：經過 redirect 的 classic script 的網址） | 完成 | 2026-10-08 |
| 4 reftest 改用 wptserve | 完成 | 2026-10-08 |

## 驗收

2026-10-08 在 Fedora 44（Linux x86_64）上，rebase 到 `main` 的 `928b0d4`（RFC 0018 的第 3 段、RFC 0019 的第 3 段）之後：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 1401 個都通過、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`docs`；`test.py wpt` 1529 個執行、0 個不符、206 個跳過（連跑 2 次一樣，約 62 秒，多出來的時間主要是 `main` 新加的測試）；`test.py reftest` 542 個通過、67 個照預期失敗、0 個意外；每次 run 之後 WPT 的 checkout 都沒變（`git status --porcelain --ignored` 是空的）。沒有動到視窗那一層和 GC，沒有跑 `test.py automation`、`matrix`、`fuzz`。

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| `net` | host 的對應規則：Chromium 的語法、第一條符合的生效、`*` 跨過 `.`、`^NOTFOUND`；沒有規則的網域照常問系統、沒設規則時 `.test` 解析不到 | `cargo test -p net`：`host_mapping` 的單元測試、`load` 的 `host_mapping_rules` |
| `net` | 額外的根憑證：WPT 的憑證經過 HTTPS 通過；名稱不符照樣被拒絕；沒設時是 `CertAuthorityInvalid`；PEM 的解析 | `cargo test -p net`：`load` 的 `additional_trust_anchors`、`trust_anchors_come_from_pem`（讀 `../third_party/wpt/tools/certs/`） |
| `net` | 靠關閉連線結束、沒有 `close_notify` 的 HTTPS body 照樣完整；有長度而提早斷掉、或 TLS 出錯的照樣是錯誤 | `cargo test -p net`：`load` 的 `bodies_that_end_with_the_connection` |
| testharness.js | 由 wptserve 提供：`.sub`、`.headers`、`.https.`、kun 的 `testharnessreport.js` 和 `/kun/` 的 alias | `python3 scripts/test.py wpt`：`4ff8d6e:tests/testharness/` 的 9 頁（`substitution.sub.html`、`headers.html`、`secure.https.html` 是新的） |
| testharness.js | 因為 server 而跳過或沒列進清單的測試：能跑的照 baseline 執行，其他的照新的理由跳過或不列 | `python3 scripts/test.py wpt`；`4ff8d6e:tests/wpt/testharness-skip.txt` 的「RFC 0020, step 3」、`testharness-include.txt` 的「RFC 0020」 |
| reftest | WPT 的 reftest 從 wptserve 載入，以 `/` 開頭的參考頁相對於 checkout 的根目錄解析 | `python3 scripts/test.py reftest`；`cargo test -p shell --lib reftest` 的 `root_relative_references_in_a_site` |
| wptserve | 設定、網址、pid 檔的判斷、log 的結尾、對應規則涵蓋 WPT 的網域；`.https.` 的測試走 HTTPS | `cargo test -p shell --lib wptserve`；`testharness` 的 `pages_and_expected_results` |
| wptserve | process：結束整個 process group、runner 被強制結束後下一次 run 清掉、同時跑兩份時第二份失敗而不動第一份；checkout 不變 | 手動（「做過的檢查」）；checkout 每次 run 都由 runner 檢查 |
| script | 經過 redirect 的外部 classic script 的網址和 base URL 是 redirect 之後的 | `cargo test -p shell --test modules` 的 `a_classic_scripts_url_is_its_response_url`；WPT 的 `dynamic-import/alpha/base-url.sub.html` |

## 做過的檢查

2026-10-08，除了「Windows 11」那一項，都在 Fedora 44（Linux x86_64）上。

- **第 0 步的原型和實測**：手動跑 `wpt serve`（venv、設定檔和 bytecode 都在 `target/`），用一個暫時的 example 經過 wptserve 載入測試頁，結果和指令在 `details.md` 的「第 0 步的實測」。起草時沒想到、改進設計的：要設 `server_host: "127.0.0.1"`；只結束主 process 會留下 12 個佔著 port 的子 process；`wpt --venv` 自己就能建 venv；單一檔案的 alias 的 `local-dir` 是目錄；`tools/serve/requirements.txt` 只有 `cryptography`；wptserve 在每個平台都用 `spawn`。`dom/nodes`、`dom/events`、`domparsing` 有 baseline 的 190 個測試，經過 wptserve 183 個一樣、7 個變好。
- **找到的 bug**：
  - `net` 把憑證錯誤分類成 `NetErrorCode` 的那段從來沒有生效過：rustls 的錯誤包在兩層 `io::Error` 裡，`io::Error::source()` 跳過它包住的錯誤，憑證錯誤一律變成 `ConnectionFailed`（第 1 步，`additional_trust_anchors` 的「名稱不符」抓到；`next_source`）。
  - `net` 把「HTTP/1.x、沒有長度、靠關閉連線結束、沒有 TLS `close_notify`」的 HTTPS body 當成載入失敗；wptserve 的 HTTPS 回應都是這樣（第 2 步，`secure.https.html` 等到 deadline）。照 Chromium 接受。
  - 外部 classic script 經過 redirect 時，script 的網址和 `import()` 的 base URL 是請求時的網址（第 3 步，WPT 的 `dynamic-import/alpha/base-url.sub.html`）。屬於 RFC 0015 的範圍，另外一個 commit 修掉：`Source::Classic` 帶著回應的網址。
  - 上次留下的 wptserve：照 Chromium 只看 pid 檔和 port 時，會把另一個還在跑的 run 的 wptserve 殺掉（第 2 步的手動確認抓到）。pid 檔改成也記 runner 的 pid（README 的「已決定的事項」第 9 條）。
  - `4ff8d6e:tests/wpt/testharness-skip.txt` 有 2 行不是 testharness.js 測試（RFC 0019 時誤加的，runner 本來就不跑），第 3 步拿掉。
- **baseline 的變化**：
  - 換 server（第 2 步）：8 個不同，都變好：`Document-characterSet-normalization-1`、`-2` 和 `Document-contentType/contentType/` 的 4 個全過（wptserve 照副檔名和 `.headers` 給 `Content-Type` 和 charset，`net::test_server` 給 `text/plain`），`Document-createElement-namespace` 的錯誤換成後面的 namespace 不對，`modulepreload-fires-before-dependencies` 多過一項（`.py` 提供的 module）。`preload/modulepreload-consumed-by-script.html` 移進 skip 清單：它的 `.py` 把 module 扣住，等測試用 kun 沒有的 `fetch` 放行，每次 run 多等 30 秒。
  - 移出 skip 清單（第 3 步）：75 行重新試跑；2 行不是測試、拿掉；46 個移出（16 個全過、30 個寫成 baseline，失敗主要是 `Referer` header、`fetch`、`Blob`、workers、跨來源的存取、不檢查 CORS）；27 個繼續跳過、換成新的理由（18 個在任何結果之前出錯、8 個逾時、1 個的失敗訊息有每次不同的 uuid）。skip 清單從 203 行變成 154 行。
  - 沒列進 include 清單的（收尾時的一致性核對發現）：RFC 0015 的第 4 階段因為 `.py` 和 pipe 而沒列進清單的 `the-script-element/` 頂層檔案和 `moving-between-documents/`，共 78 個，第 3 步沒試到（它們不在 skip 清單）。試跑之後加進 22 個（頂層 18 個，11 個全過、7 個寫成 baseline；`moving-between-documents/ordering/` 4 個全過）；56 個照舊不列，理由改成 `document.write`（4 個）和 `postMessage`（`moving-between-documents/` 的其他 52 個，每個等 60 秒）。新 baseline 的失敗：`document.write`、不檢查 CORS（muted errors）、沒有 cookie、stylesheet 不擋 script（`script-text-modifications`），和之前沒記錄的 `<script charset>` 不管（`script-charset-01`、`-03`，記進 `docs/limitations.md`）。
  - rebase 到 `main`（RFC 0018 的第 3 段和 RFC 0019 的第 3 段之後）：它們新加的測試裡 3 個 baseline 不同，都是 server 的差別：`history-traversal/browsing_context_name_cross_origin_2.html`、`_3.html` 換成 wptserve 之後才真的跨來源（kun 的跨來源存取丟出 `TypeError`），`the-history-interface/007.html` 全過（它的 `.py` 載得到）。它們新加進 skip 清單、理由和 server 有關的只有 `scroll-restoration-fragment-scrolling-cross-origin.html`（失敗訊息有 port）：port 的問題沒了，但它要等 60 秒的子 frame 跨來源導覽，照舊跳過、換成這個理由。
  - reftest 換載入方式（第 4 步）：原本的 526 個通過、62 個照預期失敗都沒變，包括用 `/fonts/ahem.css` 的。重新篩候選多了 11 個：10 個通過、加進清單；`css-sizing/dynamic-change-inline-size-004.html` 從 `file:` 也失敗（Window 的具名屬性），沒加。
- **時間**（同一台機器；換之前的用一個暫時的 worktree 從 `main` 編）：`test.py wpt` 換之前 10.33 到 10.48 秒（1032 個）、換之後 11.57 到 11.69 秒（1035 個），多約 1.2 秒，主要是 wptserve 的啟動（約 1 秒）和 venv 的檢查；第 3 步之後 13.5 秒（1081 個），加進那 22 個之後 20.2 秒（1103 個；`async_007`、`async_010` 等本來就要等好幾秒）。只跑一個測試的 run 約 1.3 秒。第一次（建 venv、下載套件）約多 5 秒。`test.py reftest` 約 23 秒。2026-10-08 回答「會不會變慢」時寫的「換之前約 32 秒」是錯的：那次量到的包含重新編譯。
- **手動確認**（wptserve 的 process，第 2 步）：同時跑兩份時第二份立刻失敗、第一份照常跑完；runner 跑到一半被 SIGKILL，留下 13 個 process 佔著 3 個固定的 port，下一次 run 清掉它們、照常通過；只結束 wptserve 的主 process 會留下子 process，runner 沒有自己的 pid 檔時不動它們、報 port 被佔用。
- **Mutation test**：共 45 個，每輪跑完都確認檔案還原。
  - 第 1 步（`net`）16 個：一開始沒抓到、補了測試之後抓到的：動詞不是 `MAP` 的三段規則被接受。沒抓到：額外的根憑證取代系統的根憑證（`tls_certs_only`），要一張系統信任的憑證才測得到，記進 `docs/test-gaps.md` 的「只靠審查」。
  - 第 2 步（wptserve、runner、`net` 的 body 結尾）20 個，全部抓到；一開始沒抓到、補了測試之後抓到的：pid 檔沒記 runner（`pid_file_contents` 和 `stale_pid_files`）、任何錯誤都當成 body 結束（`corrupt_after_body`；第一版的壞 record 和 header 一起到，錯誤落在讀 header 時，改成停 200 ms 再送）。
  - classic script 的修正 2 個，都抓到。
  - 第 4 步（reftest）7 個：一開始沒抓到、補了測試之後抓到的：任何檔案的 `/` 都解析、列出的單一檔案沒有網站根目錄。沒抓到的 2 個記進 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」：WPT 的頁面改回 `file:`、chain 不給網站根目錄（目前的清單裡沒有結果取決於它們的測試）。
  - 跑 mutation 時的教訓：「bytecode 寫進 checkout」那個 mutation 真的在 `../third_party/wpt` 留下 23 個 `__pycache__`，之後跑 WPT 的 mutation 都因為 checkout 變了而失敗、看起來是抓到；清掉之後重跑受影響的那個。清理 process 的腳本一開始比對命令列，把自己的 shell 也殺了，改成看 `/proc/<pid>/exe` 和 cwd。
- **寫錯過的測試頁**：`.sub` 的頁面寫錯過兩次：`{{GET[x]}}` 在沒有參數時是 wptserve 的 assertion，不是空字串；測試名稱裡的 `{{ports}}`、`{{domains}}` 也會被替換（它們是 dict，同樣是 assertion）。都在 `target/wpt-results/wptserve.log` 裡看得到。
- **Windows 11**（2026-10-08，merge 之後，在 `fix-wptserve-windows` branch 上）：
  - **找到的 bug**：`test.py wpt` 和 `reftest` 在 Windows 上都跑不起來。runner 用 `canonicalize` 取 WPT 的 checkout 的路徑，Windows 上是 verbatim 的 `\\?\C:\...`，裡面的 `..` 是名稱、不是上一層；wpt 在自己的位置後面接上 `../webtransport/requirements.txt`，Python 打不開（`OSError: [Errno 22]`），wptserve 一啟動就結束。修正：`shell::wptserve` 交給 Python 的路徑（命令列、工作目錄、`PYTHONPYCACHEPREFIX`、設定檔的 alias）都去掉 verbatim 的前綴（`for_python`；`\\?\UNC\` 的不動），呼叫的一方照舊用 `canonicalize`。
  - **結果**：`test.py wpt` 1528 個執行、0 個不符（和 Linux 的 baseline 一樣）；`test.py reftest` 542 個通過、67 個照預期失敗、0 個意外。wptserve 的 log 裡 `preload/resources/cross-origin-module.py` 的 `TypeError` 是那個 `.py` 在請求沒有 `Origin` 時出錯，和平台無關。
  - **手動確認**（wptserve 的 process）：同時跑兩份時，第二份因為 port 被佔用而失敗（Windows 上 `is_running` 一律是 false，不靠 pid 檔判斷），第一份照常跑完；runner 跑到一半被 `taskkill /f` 結束，Job Object 把 wptserve 和它的子 process 都結束了，沒有留下佔著 port 的 process；正常結束後 port 都釋放。
  - **Mutation test**：`for_python` 的 3 個（不去掉前綴、`UNC` 也去掉、設定檔的 alias 沒經過它），都被 `shell` 的單元測試 `verbatim_paths_are_plain_for_python` 抓到。命令列的那幾個參數沒經過它時，單元測試抓不到，要真的在 Windows 上啟動 wptserve；CI 每天在 Windows 上跑 WPT（`MACHOPO_SLOW_TESTS`）守住。
- **不穩定的測試**：沒有。新的 baseline 先檢查過有沒有每次不同的值（uuid、自動的 port），唯一的一個（`concurrent-failed-imports.any.js`）在第一次全部跑時就抓到、移進 skip 清單；之後 `test.py wpt` 連跑 3 次一樣。`bodies_that_end_with_the_connection` 靠 200 ms 的停頓讓錯誤落在讀 body 時，連跑 5 次穩定；停頓不夠時錯誤落在讀 header，測試照樣通過，只是那個 mutation 抓不到。
