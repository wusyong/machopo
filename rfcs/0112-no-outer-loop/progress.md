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
| 1. DevTools 的事件 | | |
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

