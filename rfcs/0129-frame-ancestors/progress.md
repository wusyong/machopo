# RFC 0129 的進度：不讓別人嵌：`X-Frame-Options` 和 CSP 的 `frame-ancestors`

開始日期：2026-11-28。完成：2026-11-28，Linux（Fedora 45，x86_64）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「Chromium」「Chrome 實測」）、已決定的事項第 1 到 4 條 | 做完 | 2026-11-28 |
| 1. 檢查：CSP 的解析（第 1 條）、`frame-ancestors`、`X-Frame-Options`、被擋時 commit 錯誤頁的新路（第 2 條）、Network 的 `loadingFailed`；`x-frame-options/`、`content-security-policy/frame-ancestors/` 的 WPT，逐項和 Chrome 比 | 做完 | 2026-11-28 |
| 2. console 的訊息：實測 Chrome、已決定的事項第 5、6 條；Browser 替一個 frame 加 console 訊息的路、被擋的原因；收尾 | 做完 | 2026-11-28 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 檢查 | 跨源、同源的 iframe 照 `X-Frame-Options`、`frame-ancestors` 擋或不擋，被擋的 commit 錯誤頁、iframe 的 `load` 照樣觸發；main frame、204 不檢查 | `browser` 的 `navigation::a_frame_its_response_refuses_commits_an_error_page` |
| 檢查 | 每一層祖先都算，redirect 的中間一跳不算 | `browser` 的 `navigation::every_ancestor_counts_and_only_the_final_response` |
| 檢查 | CSP 的解析、來源的解析和比對、`X-Frame-Options` 的值 | `net` 的 `csp::tests`、`frame_ancestors::tests` |
| 檢查 | WPT 逐項和 Chrome 一樣，不同的有原因 | `x-frame-options/`、`content-security-policy/frame-ancestors/`（`tests/wpt/include.txt` 的註解） |
| DevTools | `loadingFailed`：被擋的 frame 在 `responseReceived` 之後一次，`net::ERR_BLOCKED_BY_RESPONSE`、`blockedReason` `other` | `shell` 的 `devtools_network::a_refused_frame_fails_after_its_response` |
| DevTools | `loadingFailed`：CORP 的帶原因（`.NotSameOrigin`、`.NotSameSite` 和 `blockedReason`），ORB 的沒有 | `shell` 的 `devtools_network::a_blocked_response_fails_with_its_reason` |
| DevTools | `Log`：`X-Frame-Options` 的訊息是那個 frame commit 的文件的（錯誤頁、無效時的文件，行號 0），`frame-ancestors` 的違規是 `security` 的、沒有網址，原文照 Chrome 實測 | `shell` 的 `devtools_log::what_the_browser_refuses_is_logged_as_chrome_logs_it` |
| DevTools | 每一句訊息的原文、給哪一層祖先、report-only 的、`*` 的說明；清單照 Chromium 序列化 | `net` 的 `frame_ancestors::tests`、`csp::tests::a_list_is_written_as_chromium_writes_it` |
| DevTools | ORB 擋的不寫 console，CORP 擋的照通用的訊息帶原因 | `shell` 的 `devtools_log::orb_is_not_logged_and_corp_is_with_its_reason` |

## 做過的檢查

- **Chrome 實測**（Chromium 156，headless，`--remote-debugging-pipe`）：第 1 段前看被擋的 frame 的 Network 和 `load`（`details.md` 的「Chrome 實測」）；第 2 段前 attach 每個 target，記下每一條 `Log.entryAdded` 的 session、`source`、`url`、`lineNumber`，包括同網站、跨網站、巢狀的 frame，ORB、CORP 和一般的載入失敗（「第 2 段：console 的訊息」）。和原本預想的不同：無效的 `X-Frame-Options` 也寫、report-only 的 `frame-ancestors` 也寫、ORB 不寫、CORP 帶原因，`frame-ancestors` 的違規給不准的那個祖先（讀了 `ancestor_throttle.cc` 確認，不一定是父 frame）。這些成了已決定的事項第 5 條。
- **WPT**：`x-frame-options/`、`content-security-policy/frame-ancestors/` 加進清單（40 個檔案，193 個 subtest），用 wpt.fyi 的 Chrome 的結果逐項比，不同的 6 個都有原因：
  - `frame-ancestors-nested-cross-in-sandboxed-cross-url-block.html`、`frame-ancestors-sandbox-same-origin-self.html`：machopo 沒有 iframe 的 `sandbox`，祖先不是 opaque 的（Chrome 擋，machopo 不擋；opaque 的祖先不合由單元測試守著）。
  - `frame-ancestors-from-serviceworker.https.html`：沒有 service worker（ERROR）。
  - `report-blocked-frame.sub.html`、`report-only-frame.sub.html`：沒有違規的報告。
  - `get-decode-split.html`：Chrome 在 wpt.fyi 上 TIMEOUT，machopo 全過；看過它測的值（`,SAMEORIGIN,,DENY,` 衝突，`\x0BDENY`、`\x0CDENY` 不是關鍵字、放行），照 HTML 的 get, decode, and split。
  - 清單裡其他送這兩種 header 的：`html/browsers/browsing-the-web/navigating-across-documents/`（`failure-check-sequence.https.html`）跑過，0 個非預期；wdspec 的 `classic/navigate_to/frame.py`（`DENY` 的 iframe 不讓 navigate 失敗）本來就通過、沒有預期結果，被擋之後 navigate 照樣成功，沒有跑（wdspec 由 CI 跑）。
  - 第 2 段改了來源的解析之後重跑，0 個非預期。
- **mutation test**：
  - 第 1 段（`mutants-1.toml`，45 個，都抓到）：第一次 46 個裡 2 個沒抓到。「`*` 是任何 origin」的測試用了 `file:///`，它是 opaque 的、前一步就不合，補了 `ftp://b.example`。「不是 host 的 token 也當成 host」當時是等價的（只影響比對時，不合法的 host 不會等於任何祖先的），拿掉了那段檢查；第 2 段要照 Chromium 把清單寫進訊息，它不再等價，照 Chromium 的 `ParseHost` 加回來，由第 2 段的 mutant 守著。寫的時候也拿掉了三支等價的：`'none'` 的特例、`*` 合回應自己的 scheme（machopo 的祖先走不到）、沒有 scheme 時的空字串檢查。
  - 第 2 段（`mutants-2.toml`，51 個，都抓到）：清單的序列化、來源的解析（`+80`、超過 `int` 的 port 照 Chromium 不收，各補了測試）、每一句訊息、給哪一層祖先、Browser 在 commit 之後才送、renderer 的 `source`、`url`、`lineNumber`、ORB 不寫、CORP 的原因、`blockedReason`。寫的時候拿掉了兩支到不了的：「path 要以 `/` 開頭」（host 和 port 都停在 `/` 前）、「有強制的 `frame-ancestors` 而且 XFO 不是 none、allowall 時才跳過」（none、allowall 本來就什麼都不做，後半是 Chromium 的 metrics 用的）。
- **找到的事**：
  - machopo 照樣 commit 204、205 的回應（一份空的文件），HTML 和 Chrome 不 commit：本來就有的差異，暫時加程式確認過，記進 `docs/limitations.md` 的「iframe」。
  - CSP 只有 `frame-ancestors` 這件事原本沒寫在 limitations，記進「隔離」。
  - 被擋時從 `responded` 拿掉導覽，會在一個排不出來的先後裡送兩次 `loadingFailed`；改成留著它。
  - machopo 原本對 ORB 擋的寫「Failed to load resource: net::ERR_BLOCKED_BY_ORB」，Chrome 不寫；拿掉了。
  - 寫測試時的錯誤（不是程式的）：`url` 給非特殊 scheme 的是 opaque 的 origin，`custom:` 的 `*` 測試前提不成立；`b.example:` 照文法是 scheme 的來源；`example.com` 不會解析到測試的伺服器（改用 `localhost`，頁面是 IP，CORP 的 `same-site` 照樣不算同網站）。
- **只靠審查的**（`docs/test-gaps.md` 的「只靠審查」）：被擋時 `loadingFailed` 只送一次的那個先後；`frame-ancestors` 的違規送到哪個祖先的 frame、frame 不在時 renderer 丟掉 Browser 的訊息、沒 commit 時不送（一個分頁一份 `Log`，違規的訊息沒有網址，送錯 frame 看起來一樣）。
- **視窗**：沒有新的情境：被擋的 frame 走的是既有的子 frame 錯誤頁，情境的頁面是本機的檔案、送不了 header；commit 的是錯誤頁由 `browser` 的整合測試守著。`test.py scenarios --window` 兩段做完時各跑一次，75 個都過。
- **量測工具的測試**：沒有跑。這份改了自動化送的 `loadingFailed` 的內容（多了 `blockedReason`、`errorText` 的原因），沒有改 renderer 的啟動、trace、`base::census` 或 CDP 的方法。
- **不穩定的測試**：沒有碰到。
- **跑過的**：
  - 第 1 段：`cargo test -p net -p browser -p shell` 全過；`cargo test --workspace` 5104 個通過、0 個失敗、1 個忽略；fmt、clippy 通過。
  - 第 2 段：`cargo test -p net -p protocol -p browser -p renderer -p shell` 全過（net 142 + 173，protocol 13 + 16，browser 232 + 52，renderer 128，shell 105 + 13 + 786）；`cargo test --workspace` 5113 個通過、0 個失敗、1 個忽略；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs 都過。
