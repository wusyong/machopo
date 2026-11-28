# RFC 0130 的進度：iframe 的 `sandbox`

開始日期：2026-11-28。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md`）、已決定的事項第 1 到 3 條 | 做完 | 2026-11-28 |
| 1. 旗標的路、origin 和 script：解析、`iframe.sandbox`、新的訊息、frame 樹的 pending 旗標、commit 時的旗標和 origin、`CommitNavigation` 的欄位；origin 和 script（含 autofocus、`<noscript>`）；改樹上沒有的 frame 的旗標是壞訊息；相關的 WPT | 做完 | 2026-11-28 |
| 2. 導覽和開新分頁：導覽別的 frame、最上層；開新分頁、新分頁帶旗標；沒有 `allow-popups` 開新分頁是壞訊息；console 的訊息；剩下的 WPT；收尾 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `sandbox` 照 Chromium 的 `ParseWebSandboxPolicy` 解析（token 的對照、大小寫、空白、不認得的 token 的訊息），旗標只能更嚴 | `protocol` 的 `sandbox::tests` |
| `iframe.sandbox` 是 `DOMTokenList`、`supports()` 照 token 表；建 frame 時帶著 iframe 自己的旗標，屬性改了時告訴 host | `dom_js` 的 `frames::an_iframes_sandbox_goes_to_its_frame` |
| Browser 的 frame 樹：子 frame 的旗標是 iframe 的加上父 frame 的文件的，改了的下一次導覽才算，main frame 和不在樹上的 frame 的是壞訊息 | `browser` 的 `frame_tree::tests::a_frames_sandbox_flags_are_its_iframes_and_its_parents`、`navigation::a_bad_message_ends_the_renderer`、`navigation::a_changed_sandbox_counts_from_the_next_navigation` |
| 沒有 `allow-same-origin` 的文件在 Browser 是 opaque 的 origin、請求不帶 cookie；巢狀的照父 frame 的旗標 | `browser` 的 `navigation::a_sandboxed_frames_document_is_of_an_opaque_origin` |
| 沒有 `allow-same-origin` 的文件和它的 realm 父 frame 碰不到；opaque 的初始 window 不被沿用 | `dom_js` 的 `frames::a_document_sandboxed_so_is_of_an_opaque_origin`、`frames::an_opaque_initial_window_is_kept_for_no_document`；WPT 的 `sandbox-disallow-same-origin.html`、`sandbox-new-execution-context.html` |
| 沒有 `allow-scripts` 時 scripting 關著：`<script>`、handler 屬性、`javascript:` 網址不跑，`<noscript>` 是 markup，autofocus 不算，lazy 的圖片不 lazy；renderer 自己 commit 的 `javascript:` 文件照樣 sandbox | `dom_js` 的 `frames::scripting_is_disabled_in_a_frame_sandboxed_so`；`browser` 的 `navigation::a_sandboxed_documents_javascript_url_doesnt_run`、`a_sandboxed_document_doesnt_autofocus`、`a_sandboxed_frames_javascript_document_stays_sandboxed`；WPT 的 `sandbox-disallow-scripts.html`、`sandbox-parse-noscript.html`、`no-sandboxed-automatic-features.html`、`image-loading-lazy-move-into-script-disabled-iframe.html` |
| WPT 逐項和 Chrome 一樣，不同的有原因 | `html/browsers/sandboxing/`、`the-iframe-element/` 的 sandbox 測試（`tests/wpt/include.txt` 的註解） |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 第 1 步：旗標的路、origin 和 script（2026-11-28，Linux）

- **新的程式**：`protocol::sandbox`（旗標、解析、token 表）；`dom` 記旗標的 bit、「origin 是新的 opaque 的」、「scripting 關著」（`set_sandbox`），`origin_document` 停在 opaque 的文件；`dom_api` 翻譯旗標（`set_document_sandbox`、`scripting_disabled`、`iframe_sandbox`），`iframe.sandbox`，屬性改了時記 `SandboxChanged`、告訴 host；`ScriptEngine::create_sandboxed_window`（`dom_js` 覆寫：opaque 的不沿用初始 window、給新的 security token）；`FrameClient::frame_policy_changed`，`ChildFrameMade` 帶旗標；`html` 的 `Parser::with_scripting`；Browser 的 frame 樹的 `pending_sandbox`、`sandbox`、`set_frame_policy`，`CommitParams` 在導覽開始時抄旗標，commit 時換成 opaque 的 origin；`protocol` 的 `DidChangeFramePolicy`、`DidCreateChildFrame` 和 `CommitNavigation` 的旗標。
- **寫的時候找到、一起修的**：
  - 初始 `about:blank` 的 window 照網址判斷要不要沿用，那時還不知道旗標：sandbox 之後還沿用（WPT 的 `sandbox-new-execution-context.html`）。改成建 window 時就帶著旗標（`create_sandboxed_window`）；opaque 的初始 window 也不給別的文件沿用。
  - lazy 的圖片的「will lazy load」有三處各自算、只看有沒有 window：抽成一個（`dom` 的 `will_lazy_load`），也看 scripting（WPT 的 `image-loading-lazy-move-into-script-disabled-iframe.html` 從 TIMEOUT 變成通過，`docs/limitations.md` 那一條拿掉了）。
  - `about:blank` 之外，`data:` 網址也照發起者的旗標繼承（Chromium 的 local scheme）。
- **寫測試時的錯誤（測試的，不是程式的）**：`contentWindow` 是 `WindowProxy`，換了 window 也是同一個物件，不能用 `===` 分辨沿用；沒有 `allow-same-origin` 的 frame 和父 frame 不同源，父 frame 給它的 `javascript:` 網址照 HTML 什麼都不做，`load` 不會來；`javascript:` 的空文件已經有根元素。
- **WPT**：`html/browsers/sandboxing/` 和 `the-iframe-element/` 的 sandbox 測試（wpt.fyi 上 Chrome 跑的 68 個檔案）加進清單。90 個檔案裡 38 個和 Chrome 一樣，52 個不同，都是：第 2 段的（導覽祖先和最上層、`history.go`、開新分頁）；machopo 沒有的功能（`window.open`、表單送出、modal、下載、`<video>`、PDF、`document.domain`、CSP 的 `sandbox`）。清單裡其他用到 sandbox 的 iframe 的 24 個測試照樣跑過：`fetch/api/cors/sandboxed-iframe.html`、`image-loading-lazy-in-script-disabled-iframe.html`、`image-loading-lazy-move-into-script-disabled-iframe.html` 變成通過，RFC 0129 的兩個 `frame-ancestors`（sandbox 的祖先）和 `no-sandboxed-automatic-features.html` 也是，拿掉了它們的預期結果。
- **mutation test**（`mutants-1.toml`，36 個）：第一次 3 個沒抓到，補了測試之後都抓到：
  - 「文件的旗標停在建立時」：frame 樹的測試建立和 commit 給的是同樣的旗標。改成建立時沒有。
  - 「sandbox 的文件的 origin 照網址」：只測了初始 `about:blank`（它的網址本來就是 opaque 的 origin）。補了網址是父 frame 的 origin 的文件（`a_document_sandboxed_so_is_of_an_opaque_origin`）。
  - 「sandbox 的文件會 autofocus」：WPT 的 iframe 沒有 `allow-same-origin`，被「同源才算」擋了。補了 `allow-same-origin` 的（`a_sandboxed_document_doesnt_autofocus`，用 rendering update 先 flush autofocus、再跑 animation frame 的順序分辨，不靠時間）。
  - 寫的時候拿掉一處等價的：parser 的 `scripts_never_run`（`prepare_script` 在檢查 scripting 之前就設了 `already started`）。
- **還沒測的**：`about:blank`、`data:` 照發起者繼承旗標：能繼承的發起者自己碰不到別的 frame 的 origin 和 script，觀察得到的是第 2 段才擋的導覽和開新分頁，第 2 段測。
- **跑過的**：`cargo test -p protocol -p dom -p html -p dom_api -p dom_js -p dom_wasm -p browser -p renderer -p shell` 全過；`sanity.py` 全部；階段的檢查：`cargo test --workspace` 5128 個通過、0 個失敗、1 個忽略，`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過（clippy 找到的：`split` 改用字元的陣列，`child_frame_created` 的參數包成 `ChildFrameMade`），`test.py scenarios --window` 75 個都過。沒有新的情境：看得到的（script 不跑、opaque 的 origin）由 WPT 和整合測試守著。

