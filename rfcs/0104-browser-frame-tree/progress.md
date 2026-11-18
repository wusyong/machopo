# RFC 0104 的進度：frame 樹和每個 frame 的導覽由 Browser 擁有

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-18

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：`FrameToken`、engine 抽 id、renderer 送建立和 detach 的訊息、DevTools 和 WebDriver 的 frame 的 id | 完成 | 2026-11-18 |
| 1b：Browser 的 `FrameTree`、檢查、壞掉的訊息 | | |
| 1c：第 1 階段的檢查和文件 | | |
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
