# RFC 0112 的進度：拿掉 event loop 的外圈

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-24

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0a. 盤點：外圈和內圈每個 task 之後的每一步，對照 Chromium 的原始碼 | 做完，待審查 | 2026-11-24 |
| 0b. 必壞的測試：通道一直有 task 時的 timer | 做完，待審查 | 2026-11-24 |
| 0c. 原型：DOM 的 task 進 scheduler，T1 | 做完，待審查 | 2026-11-24 |
| 0d. 原型：DOM 的 task 進 scheduler，T2 | 做完，待審查 | 2026-11-24 |
| 0e. 交給使用者決定範圍和做法 | 做完：做法第 3 條（T1），範圍第 4 條（A），分段第 5 條 | 2026-11-24 |
| 1. DevTools 的事件（第 6 到 9 條） | 做完，待審查 | 2026-11-24 |
| 2. 導覽在呼叫的當下送 | | |
| 3. DOM 的 task 是 scheduler 的 task | | |
| 4. renderer 自己的工作排成 task | | |
| 5. 剩下的步驟，拿掉外圈 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 盤點 | `details.md` 的表；不改程式，所以沒有測試 |
| 原型 | 各自的 patch 在 `prototypes/`；通道一直有 task 時的 timer 的必壞測試必過，比較改動、成本、兩個 engine 各要做什麼 |

## 做過的檢查

### 第 0 階段（2026-11-24，Linux：Fedora 45、debug build，`0a459d1` 之上）

**盤點**：一個 subagent 唯讀地從 Gitiles 讀 Chromium `main` 的 `28312288d3ac`（2026-11-23）的原始碼，對了 `details.md` 的表的每一列和三個問題（scheduler 怎麼挑、同一個 task 的兩個導覽、iframe 插入時 DevTools 的順序）；它的關鍵說法（DevTools 的 DOM 變動每個 task 之後才送、delayed task 到期時才拿到 enqueue order）我對過它引的段落。沒對到的三項列在 `details.md`。

**必壞的測試**：沒改 13 次壞 13 次；Chrome for Testing 152 跑同一頁 5 次都是對的那一邊（`details.md` 的「原型」）。

**原型**：T1、T2 的必壞測試各 5 次都通過；整組通過：T1 是 `renderer` 127 個、`dom` 256 個、`dom_api` 228 個、`dom_js` 556 個、`shell` 779 個、`browser` 227 個；T2 是 `renderer` 127 個、`dom_api` 228 個、`dom_js` 556 個、`shell` 779 個、`browser` 227 個（T2 沒有改 `dom`）。原型不是要合併的程式，沒有 mutation test、clippy、WPT，效能沒有量；patch 都套得回 `0a459d1`。

### 第 1 階段（2026-11-24，Linux：Fedora 45、debug build，`e297d88` 之上）

**必壞的測試**：在第 1 段之前的程式上各跑 3 次、3 次都壞：新 frame 的三個事件的順序、建了又拿掉的 iframe、task 的 DOM 變動先於下一個 task。子 frame 的 `frameNavigated` 在它的 script 之前（`a_child_frames_document_is_told_before_its_script`）：第 8 條原本的做法（B）下，臨時的測試量到 `console` 先到；mutation test 的「子 frame 的 commit 沒有在引擎建文件之前告訴 Page agent」就是 B，這個測試抓到它。

**mutation test**（`mutants-1.toml`，24 個）：22 個抓到，2 個沒抓到，都是現在的程式裡別的地方也做了同一件事：
- 「frame 建立或拿掉時沒設旗標」：新的 frame 有 `src` 時，子 frame 送導覽之前照舊先比對；沒有 `src` 時，它的 `about:blank` 的 commit 設旗標；拿掉的 frame 由 hook 當場送 `frameDetached`。留著它，因為旗標的意思就是 frame 變了，第 2 段改了導覽、第 4 段把 commit 排成 task 之後可能就要靠它。
- 「頂層的 commit 沒設旗標」：DOM agent 在頂層 commit 時清掉每個 session 的節點，`documentUpdated` 之後 client 用命令重新要文件，回答命令之前一定先比對；Page agent 在 `create_window` 之前已經直接拿到新文件。留著它，讓 DOM agent 的 frame 清單跟著 DOM。

跑的過程裡補的測試：拿掉的 frame 先送裡面的 frame（`a_frame_gone_is_told_after_the_frames_in_it`，連同一個 task 裡建了外層和裡層又拿掉外層）、`javascript:` 的空文件的 loaderId（原本以為 `srcdoc` 是 renderer 自己 commit 的，其實也交給 browser）、繼承 origin 的 `about:blank`（`an_about_blank_of_its_initiators_origin_is_told_so`）、DOM 排著的 task 的變動先於下一個 task（`queued_tasks_changes_are_told_before_the_next_task`：第二個命令要在第一個跑完之前到，所以第一個命令忙 200 ms；時機不對時它照樣通過、只是抓不到）、新 frame 初始的 `about:blank` 不用等下一個命令（`a_new_frames_initial_document_is_told_as_its_task_ends`）、載入中的 frame 初始文件的 `init` 先到（`a_loading_frames_initial_document_is_told_before_its_own`）、沒有 session 的頁面照樣印出警告，timer 的也是（`script_warnings_are_logged_without_a_session`）。

過程中改的實作：拿掉「導覽開始或結束時設旗標」（只改 `navigating`，比對只在新的 frame 用到它，mutation test 看出來的）；外圈最後原本無條件的 `tell_frames`、`tell_dom_changes`、`log_console_warnings` 換成同一個「task 之後」（看旗標）；`log_console_warnings` 移到「沒有 session 就不做」之前（原本沒開 DevTools 的頁面的警告會一直堆在 DOM 裡）；`tell_document_created` 裡多餘的頂層判斷拿掉（呼叫它的地方只在子 frame 上，那個 mutant 和原本的程式等價）。

**相關 crate 的測試**：`dom`、`dom_api`、`dom_js`、`renderer`、`shell`、`browser` 都通過。

**階段結束的檢查**：`cargo test --workspace` 通過（60 個 binary，4701 個通過、1 個 ignored）；`cargo fmt --check` 和 `cargo clippy --workspace --all-targets -- -D warnings` 通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。

