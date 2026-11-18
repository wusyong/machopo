# RFC 0104 的進度：frame 樹和每個 frame 的導覽由 Browser 擁有

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-18

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：`FrameToken`、engine 抽 id、renderer 送建立和 detach 的訊息、DevTools 和 WebDriver 的 frame 的 id | 完成 | 2026-11-18 |
| 1b：Browser 的 `FrameTree`、檢查、壞掉的訊息 | 完成 | 2026-11-18 |
| 1c：第 1 階段的檢查和文件 | 完成 | 2026-11-18 |
| 2：子 frame 的導覽經過 Browser | | |
| 3：joint session history | | |
| 4：收尾 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a（2026-11-18，macOS、Apple M4）

- **做了什麼**：`protocol` 加 `FrameToken`（16 個 byte，寫成 32 個大寫 hex）；`RendererMsg::DevToolsMainFrameToken` 改用它；兩則新訊息 `DidCreateChildFrame`、`DidDetachFrame`（Browser 在 1b 才處理，現在收下不做事）。engine 建子 frame 時抽 token（`dom_api::new_frame_token`），存在 engine 的 `Frame` 上，`FrameRequest::Created`、`Removed` 帶著它；主 frame 換文件時 engine 也對它的子 frame 排 `Removed`（之前只有子 frame 換文件時才排）。engine 的介面拿掉 `set_devtools_frame_ids` 和它的 closure，換成 `set_main_frame_token`。renderer 把 token 記在自己的子 frame 表（`ChildFrames`）上，DevTools 的子 frame 的 `frameId` 和 WebDriver 的 frame 參照都換成 token；renderer 的前綴照舊用在元素參照、DevTools 的請求和 style sheet 的 id（`new_id_prefix`）。
- **和計畫不同的**：WebDriver 從 id 找 frame 查的是 renderer 的子 frame 表，不是 engine 的清單；engine 的介面只多了 `set_main_frame_token`（RFC 的「DevTools 和自動化」那一段已經改成這樣，寫了理由）。
- **新的測試**：`protocol` 的 `frame_token::a_token_is_written_as_32_hex_digits`、`what_isnt_a_token`；`dom_js` 的 `frames::a_frame_goes_with_its_token`（每個子 frame 的 token 不同；iframe 被拿掉、子 frame 換文件、主 frame 換文件時 `Removed` 帶著對的 token，子孫不另外送）；`browser` 的 `links::the_browser_hears_of_child_frames_coming_and_going`（Browser 依序聽到三個子 frame 的建立和它們父 frame 的 token、iframe 被拿掉時一則 detach、主 frame 換頁時只有最外層那個子 frame 的 detach）。改的測試：`dom_js` 的 `devtools` 兩個測試改用固定的主 frame 的 token 和 `Created` 帶的 token；`shell` 的 `devtools::every_frame_has_a_context_that_goes_with_its_document` 改成檢查子 frame 的 `frameId` 是一個 token（原本檢查舊格式的 `.`）。
- **mutation test**（`python3 scripts/test.py mutate`，15 個）：抓到 14 個：每個子 frame 同一個 token、token 不是亂數（`dom_js` 的 `frames::`）；主 frame 的子 frame、子 frame 換文件時它的子 frame、被拿掉的 iframe 的 `Removed` 不排（`dom_js` 的 `frames::`）；inspector 拿不到子 frame 的 id、engine 忘掉主 frame 的 token（`dom_js` 的 `devtools::`）；子 frame 的父 frame 一律說成主 frame、不送建立、不送 detach（`browser` 的 `the_browser_hears_of_child_frames_coming_and_going`）；新的 engine 沒拿到主 frame 的 token（`shell` 的 `devtools::`）；WebDriver 從 id 找不到 frame（`shell` 的 frame 測試）；token 寫成小寫、hex 的兩位數接受正負號（`protocol` 的 `frame_token`）。沒抓到 1 個：token 到的時候 engine 已經在、當場告訴它的那條，是等價的（Browser 送的第一則訊息就是 token，那時還沒有 engine），記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。
- **跑過的檢查**：`cargo test -p protocol -p dom_api -p dom_js -p dom_wasm -p renderer -p browser -p shell -p driver` 全部通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。`cargo test --workspace`、fmt 的整體檢查和 clippy 照規則在第 1 階段做完時跑。

### 1b（2026-11-18，macOS、Apple M4）

- **做了什麼**：`browser` 加 `FrameTree`（`frame_tree.rs`：以 token 為鍵，每個節點記父節點和照建立順序的子節點）和 `BadMessageReason`（`bad_message.rs`）。`WebContents` 的主 frame 的 token 改成樹的根；收到 `DidCreateChildFrame`、`DidDetachFrame` 時更新樹，接不上的是壞掉的訊息：記 error、`kill`、標成 crashed、樹清回只剩根（`received_bad_message`）。renderer 掛掉（`RendererGone`）、`about:crash` 時也清樹；crashed 的期間不處理舊 renderer 送來的建立和 detach。
- **和計畫不同的**：renderer 重開時不另外清樹：會走到重開的三種情況都已經清過，留著只是多一個等價的 mutant。
- **找到的問題**：舊 renderer 結束前已經排進 Browser 佇列的訊息，在新的 renderer 開始之後才被處理時，會被當成新 renderer 的（訊息不帶是哪個 renderer 送的），接不上樹的會把新的 renderer 也結束掉。第 1 階段沒有處理，記在 RFC 的「待決定」，第 2 階段開始前決定。
- **新的測試**：`browser` 的單元測試 `frame_tree::tests::frames_come_and_go_with_the_frames_in_them`、`what_the_tree_cant_take_is_a_bad_message`；整合測試 `links::a_bad_message_ends_the_renderer`（四種壞訊息各一次：renderer 結束、分頁 crashed、樹只剩根、之後舊 renderer 的 detach 不算數，重新整理之後新的 renderer 的子 frame 重新進樹）、`links::a_renderer_gone_takes_its_frames`（`about:crash` 和 `RendererGone` 都清樹）；`the_browser_hears_of_child_frames_coming_and_going` 加上樹的形狀的檢查。
- **mutation test**（14 個）：抓到 13 個：樹接受用過的 token、不在的父 frame、拿掉不在的 frame、拿掉主 frame；拿掉時留下子孫、留在父節點的子節點裡；清樹時留著子節點（`browser` 的 `frame_tree` 單元測試）；`WebContents` 不把建立、detach 放進樹；壞訊息不結束 renderer、不清樹；`about:crash`、`RendererGone` 不清樹（`browser` 的整合測試）。沒抓到 1 個：crashed 的期間照樣處理舊 renderer 的建立和 detach，是等價的（記進 `docs/test-gaps.md`）。
- **跑過的檢查**：`cargo test -p browser -p shell -p driver` 全部通過。

### 1c：第 1 階段的檢查（2026-11-18，macOS、Apple M4）

- **整個 workspace**：`cargo test --workspace` 4093 個通過、1 個 ignored；`cargo fmt --check` 通過。`cargo clippy --workspace --all-targets -- -D warnings` 抓到三處，在這個 commit 裡修好：`FrameToken::parse` 的 `chunks_exact(2)` 換成 `as_chunks::<2>()`；`dom_js` 的 `Frame::new` 和 `reuse_initial_window` 的參數太多：改成 `Frames::set` 換掉同一個 frame 時沿用它的 token（frame 的 token 跨過它自己的導覽本來就不變，像 modules 一樣沿用），只有 `create_child` 設新的 token，engine 換文件的兩條路不再傳 token。修完之後 clippy 全部通過，`cargo test -p dom_js`、`-p protocol` 重跑通過；加一個 mutant（`Frames::set` 不沿用 token），`dom_js` 的 `devtools::` 抓到（子 frame 換文件之後 DevTools 的 id 還是同一個 token）。
- **情境**：`python3 scripts/test.py scenarios frames iframes iframe-loads frame-input sad-tab navigation` 9 個執行全部通過；階段驗收時在真的視窗裡跑 `--window frames iframes iframe-loads frame-input sad-tab` 5 個全部通過，看過 `sad-tab` 的截圖。
- **WPT**（WebDriver 的 frame 參照換了寫法，經過 machopodriver 跑）：`python3 scripts/test.py wpt html/semantics/embedded-content/the-iframe-element/ html/browsers/history/the-location-interface/ html/browsers/windows/`：74 個測試、334 項檢查，沒有非預期的結果。
- **sanity**：`deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。
