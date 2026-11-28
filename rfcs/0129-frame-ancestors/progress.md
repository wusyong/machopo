# RFC 0129 的進度：不讓別人嵌：`X-Frame-Options` 和 CSP 的 `frame-ancestors`

開始日期：2026-11-28。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「Chromium」「Chrome 實測」）、已決定的事項第 1 到 4 條 | 做完 | 2026-11-28 |
| 1. 檢查：CSP 的解析（第 1 條）、`frame-ancestors`、`X-Frame-Options`、被擋時 commit 錯誤頁的新路（第 2 條）、Network 的 `loadingFailed`；`x-frame-options/`、`content-security-policy/frame-ancestors/` 的 WPT，逐項和 Chrome 比 | 做完 | 2026-11-28 |
| 2. console 的訊息：Browser 替一個 frame 加 console 訊息的路，四種被擋的（第 3 條）；收尾 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 跨源、同源的 iframe 照 `X-Frame-Options`、`frame-ancestors` 擋或不擋，被擋的 commit 錯誤頁、iframe 的 `load` 照樣觸發；main frame、204 不檢查 | `browser` 的 `navigation::a_frame_its_response_refuses_commits_an_error_page` |
| 每一層祖先都算，redirect 的中間一跳不算 | `browser` 的 `navigation::every_ancestor_counts_and_only_the_final_response` |
| CSP 的解析、來源的比對、`X-Frame-Options` 的值 | `net` 的 `csp::tests`、`frame_ancestors::tests` |
| DevTools 的 `loadingFailed`（`net::ERR_BLOCKED_BY_RESPONSE`，在 `responseReceived` 之後，只一次） | `shell` 的 `devtools_network::a_refused_frame_fails_after_its_response` |
| WPT 逐項和 Chrome 一樣，不同的有原因 | `x-frame-options/`、`content-security-policy/frame-ancestors/`（`tests/wpt/include.txt` 的註解） |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 第 1 步：檢查（2026-11-28，Linux）

- **新的程式**：`net` 的 `csp`（CSP 解析成 policy 和 directive，`frame-ancestors` 的來源清單和比對，照 Chromium 的 `CheckCSPSourceList`、`CheckCSPSource`）、`frame_ancestors`（`check`：強制的 `frame-ancestors`，沒有時 `X-Frame-Options`）；`browser` 的 `commit_response` 在 commit 之前檢查（`FrameTree::ancestor_origins`，和 `loader_params` 共用走祖先的 `ancestors`），不准的 commit 錯誤頁、發 `ContentsEvent::NavigationRefused`；`shell` 的自動化照它送 `Network.loadingFailed`。
- **新的測試**：`net` 的單元測試 12 個（`csp` 7、`frame_ancestors` 5）；`browser` 的 `navigation` 2 個；`shell` 的 `devtools_network` 1 個。
- **WPT**：`x-frame-options/`、`content-security-policy/frame-ancestors/` 加進清單（40 個檔案，193 個 subtest），用 wpt.fyi 的 Chrome 的結果逐項比，不同的 6 個都有原因：
  - `frame-ancestors-nested-cross-in-sandboxed-cross-url-block.html`、`frame-ancestors-sandbox-same-origin-self.html`：machopo 沒有 iframe 的 `sandbox`，祖先不是 opaque 的（Chrome 擋，machopo 不擋；opaque 的祖先不合由單元測試守著）。
  - `frame-ancestors-from-serviceworker.https.html`：沒有 service worker（ERROR）。
  - `report-blocked-frame.sub.html`、`report-only-frame.sub.html`：沒有違規的報告。
  - `get-decode-split.html`：Chrome 在 wpt.fyi 上 TIMEOUT，machopo 全過；看過它測的值（`,SAMEORIGIN,,DENY,` 衝突，`\x0BDENY`、`\x0CDENY` 不是關鍵字、放行），照 HTML 的 get, decode, and split。
  - 清單裡其他送這兩種 header 的：`html/browsers/browsing-the-web/navigating-across-documents/`（`failure-check-sequence.https.html`）跑過，0 個非預期；wdspec 的 `classic/navigate_to/frame.py`（`DENY` 的 iframe 不讓 navigate 失敗）本來就通過、沒有預期結果，被擋之後 navigate 照樣成功，沒有跑（wdspec 由 CI 跑）。
- **mutation test**（`mutants-1.toml`，45 個）：第一次 46 個裡 44 個抓到、2 個沒抓到：
  - 「`*` 是任何 origin」：測試的非 HTTP 祖先是 `file:///`，它是 opaque 的，在前一步就不合了。補了 `ftp://b.example`（tuple 的 origin）。
  - 「不是 host 的 token 也當成 host」：等價。祖先的 host 一定是合法的網域或位址，不合法的 source host 不會等於它、也不會是它的結尾。拿掉了那段檢查和這個 mutant。
  - 寫的時候也拿掉了另外三支等價的：`'none'` 的特例（`'none'` 本來就不是合法的來源）、`*` 合回應自己的 scheme（machopo 的祖先走不到）、沒有 scheme 時的空字串檢查（下一步的比對就是 `No`）。
  - 補過、改完之後整份重跑，45 個都抓到。
- **找到的事**：
  - machopo 照樣 commit 204、205 的回應（一份空的文件），HTML 和 Chrome 不 commit。這是本來就有的差異，和這份無關，記進 `docs/limitations.md` 的「iframe」。
  - CSP 只有 `frame-ancestors` 這件事原本沒寫在 limitations，記進「隔離」。
  - 被擋時從 `responded` 拿掉導覽，會在一個排不出來的先後裡送兩次 `loadingFailed`；改成留著它，記進 `docs/test-gaps.md` 的「只靠審查」。
- **只靠審查的**：被擋時 `loadingFailed` 只送一次的那個先後（`docs/test-gaps.md`）。
- **跑過的**：`cargo test -p net -p browser -p shell` 全過（net 136 + 173，browser 232 + 52，shell 105 + 13 + 783）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs。
- **階段的檢查**：`cargo test --workspace` 5104 個通過、0 個失敗、1 個忽略；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過；`test.py scenarios --window` 75 個都過。沒有新的情境：被擋的 frame 走的是既有的子 frame 錯誤頁，情境的頁面是本機的檔案、送不了 header，commit 的是錯誤頁由 `browser` 的整合測試守著。

