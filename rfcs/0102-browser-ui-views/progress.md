# RFC 0102 的進度：瀏覽器 UI 的 view 樹和可以換的版面

開始日期：2026-11-16

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 第 1 階段：`ui` 的保留的樹，工具列和分頁列搬上去 | 進行中 | |
| 1a：`ui` 的樹、排版、畫、事件、主題；`ui_text` 搬進來 | 做完 | 2026-11-16 |
| 1b：工具列和分頁列改寫成 view，`BrowserView` 走樹 | 還沒開始 | |
| 1c：文件和階段結束的檢查 | 還沒開始 | |
| 第 2 階段：宣告式的一層、版面管理和側邊欄 | 還沒開始 | |
| 第 3 階段：外觀 | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `ui` 的樹：id 和世代、flex 的排版、要求重排、hit test、事件往上找、capture、hover 的順序、焦點和按鍵、游標、只重畫變了的 view 和變了的範圍（重畫、搬動、縮小、隱藏、拿掉、新的大小、新的主題、沒有 rect 的 op）、留著的錄製沒有過期 | `cargo test -p ui` |
| `ui` 只依賴允許的 crate；renderer 那一側不依賴 `ui` | `python3 scripts/sanity.py deps`、`python3 scripts/sanity.py boundary` |
| `shell` 改用 `ui` 的 `text`、`Clipboard` 之後行為不變 | `cargo test -p shell --lib`；`cargo test -p shell --test it -- browser_view:: windowless::` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a

2026-11-16，macOS（Apple Silicon）。

- **新的測試**：`ui` 的 26 個（`crates/ui/src/tests.rs` 25 個，`theme.rs` 1 個）。`shell` 的單元測試 68 個、整合測試的 `browser_view::`、`windowless::` 64 個照舊通過。
- **mutation test**（`mutants-1a.toml`，`python3 scripts/test.py mutate --keep-going rfcs/0102-browser-ui-views/mutants-1a.toml`）：第一次 32 個抓到 28 個。沒抓到的四個：
  - 拿掉 view 時不清焦點、不清 capture：讀它們的地方（`focused()`、`captured()`、`pointer()`）都先問那個 id 還在不在，清掉是多餘的。刪掉那段程式，由世代守住（拿掉的 view 的 id 查不到，位置重用時也查不到新的）。
  - 放進 layer 時用錄製記著的位置：放進之前兩者一定一樣，等價，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」，從 spec 拿掉。
  - 隱藏的 view 被 hit：排過版之後隱藏的 view 大小是 0，本來就點不到；沒排版就 hit test 時只有這個判斷擋住。補測試：隱藏之後、排版之前 hit test。
  - 同時找到的問題：view 的變了的範圍原本用 display list 的 `bounds()`，沒帶 rect 錄的 op（可能畫在任何地方）不算在裡面，範圍會太小。改成每個 op 的 visual rect 的聯集、夾在 layer 裡，補測試 `an_op_recorded_without_a_rect_damages_the_whole_layer`，spec 加兩個 mutant。
  - 改完之後重跑改到的 7 個：全部抓到。spec 現在 31 個。
- **跑過的檢查**：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`deps` 和 `boundary` 新的規則各弄壞一次（`ui` 依賴 `protocol`、`paint` 依賴 `ui`），都報錯。
