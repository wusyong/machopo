# RFC 0024 的細節

## 現有的引用

2026-10-09 在 `0021`（d6930ae）上數的，`rfcs/` 以外、git 追蹤的檔案。「RFC NNNN」的出現次數：

| 位置 | 處數 | 檔案數 |
| --- | --- | --- |
| `docs/` | 214 | 4 |
| crate 的 README 和 `soundness.md` | 166 | 7 |
| `tests/`（WPT 的清單、情境、README、wptrunner 的 product） | 162 | 33 |
| 程式碼（`.rs`、`.py`、`.toml`） | 142 | 80 |
| `resources/`（about: 頁面的文字） | 46 | 2 |
| 根目錄的 README、`CLAUDE.md`、`sdk/` | 8 | 5 |
| 合計 | 738 | 131 |

最多的檔案：

| 檔案 | 處數 |
| --- | --- |
| `docs/limitations.md` | 84 |
| `docs/testing.md` | 65 |
| `docs/architecture.md` | 64 |
| `crates/dom_api/README.md` | 61 |
| `crates/dom/README.md` | 43 |
| `resources/demo.html` | 43 |
| `tests/wpt/README.md` | 40 |
| `4ff8d6e:tests/wpt/testharness-include.txt` | 31 |
| `tests/scenarios/demo-script.txt` | 26 |
| `crates/dom/soundness.md` | 24 |
| `crates/dom_js/README.md` | 23 |
| `4ff8d6e:tests/wpt/testharness-skip.txt` | 19 |
| `crates/browser/tests/navigation.rs` | 8 |
| `4ff8d6e:tests/wpt/reftest-expectations.txt` | 8 |
| `4ff8d6e:tests/wpt/reftest-include.txt` | 8 |
| `crates/dom_wasm/README.md` | 7 |
| `crates/dom_api/src/wit.rs` | 6 |
| `crates/driver/README.md` | 6 |
| `crates/renderer/src/frame_view.rs` | 6 |
| `crates/renderer/src/render_thread.rs` | 6 |

MVP 的文件（`pre-rfcs`、`P0.md` 到 `P4.md`、`MVP.md`、`PROGRESS.md`）：`docs/limitations.md`（13）、`docs/architecture.md`（10）、`docs/README.md`（6）、`CLAUDE.md`（5）、`docs/testing.md`（5）、`crates/dom/soundness.md`（4）、`crates/dom/README.md`（2）、`crates/dom_api/README.md`（2）、`crates/dom_js/README.md`（2）、`scripts/sanity.py`（2）、`README.md`（1）、`crates/shell/README.md`（1）。另外有「（P3.1）」「P4.2 第 2b 段」這類階段代號：照 README 的「已決定的事項」第 4 條的樣式（前後不是英數字的 `P[0-4]`，可以接 `.數字`），2026-10-09 在 `main`（974666f）上約 70 處、40 個檔案，最多的是 `docs/limitations.md`（11）、`resources/demo.html`（7）、`docs/architecture.md`（7）、`docs/testing.md`（6）、`tests/wpt/README.md`（5）；誤判只有 `4ff8d6e:tests/wpt/baselines/` 裡的一處（WPT 的輸出 `"New P1"`）。

`RFC` 加四位數字裡，不是 kun 的編號的只有 IETF 的 RFC：`RFC 4648`（`crates/browser/src/loader.rs`）、`RFC 6761`（`crates/net/tests/load.rs` 兩處），所以樣式只比對以 `0` 開頭的編號。
