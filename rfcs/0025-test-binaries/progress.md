# RFC 0025 的進度：每個 crate 一個整合測試的 binary

開始日期：2026-10-10；完成：2026-10-10，Windows 11（32 個 thread，rust-lld）。`test.py matrix`、`fuzz` 之後在 macOS 上跑（README 的第 13 條，「做過的檢查」的最後）。每次跑測試的時間記在 `test-times.tsv`（README 的第 5 條），摘要在最後；原始的檔案在完成之後刪掉（第 22 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 第 0 段：原型和實測（合併 `dom` 和 `shell`） | 完成（決定做下去，README 的第 7 條） | 2026-10-10 |
| 第 1 段：`dom`、`dom_api`、`dom_js`、`bindings` | 完成 | 2026-10-10 |
| 第 2 段：其他 crate | 完成 | 2026-10-10 |
| 第 3 段：守住和文件 | 完成 | 2026-10-10 |

四段都在 `0025` 上做完才一起 merge（文件的整體說明在第 3 段）。

## 驗收

**測試的行為**

| 項目 | 怎麼驗證 |
| --- | --- |
| 測試數不變 | 每個 crate 合併前後的 `cargo test -p` 的 passed 合計相同（`dom` 237、`dom_api` 275、`dom_js` 233、`bindings` 26、`shell` 299、`browser` 95、`layout` 127、`paint` 132、`style` 93、`html` 31、`dom_wasm` 34、`protocol` 9、`net` 31），`cargo test --workspace` 合併前後都是 1853 |
| 合併的 binary 的 V8 flag 不取決於順序 | 拿掉 `initialize_v8_before_main!`、單一 thread 讓不帶 flag 的測試先跑，用 `gc()` 的測試失敗（`dom` 的 `cross_isolate` 先跑、`events`、`gc_stress`；`dom_js` 的 `abort` 先跑、`bindings`、`gc_stress`；`bindings` 的 `cppgc_member` 先跑、`cppgc_traced`、`v8_smoke`，`test.py mutate`）。不篩選時抓不到：照字母順序第一個（`dom` 的 `abort`）自己就帶 `--expose-gc` |
| 不帶 flag 的測試在 `--expose-gc` 底下照樣通過 | `dom` 的 5 個檔、`dom_js` 的 14 個檔、`bindings` 的 `cppgc_member` 合併之後全過 |
| 合併的 binary 穩定 | `dom` 的連跑 5 次、`shell` 的 3 次，加上每一段的 `cargo test --workspace` |
| `test.py matrix` | macOS 上 8 種設定都通過（每種 856 個測試、17 個 binary，debug 的 V8）；`fuzz --iterations 20` 也是 20 組都通過 |

**守住**

| 項目 | 怎麼驗證 |
| --- | --- |
| `sanity.py tests` | 手動放 `crates/layout/tests/stray.rs`（直接在 `tests/` 底下）和 `crates/paint/tests/it/forgotten.rs`（沒在 `main.rs` 宣告），兩個都報錯；拿掉之後通過 |
| `--test it <模組>::` 的模組名稱 | 手動把 `docs/testing.md` 的 `spec::` 改成 `specs::`，`sanity.py docs` 報錯；改回之後通過。`dom`、`dom_api`、`dom_js` 的 README 裡寫成 `<模組>` 的佔位也被它擋下，改成真的模組 |

**時間**（3 次的中位數；合併前在 `main` 的 `5bd761e1` 的 worktree 量，它自己的 target 目錄先完整編一次）

| 項目 | 合併前 | 合併後 |
| --- | --- | --- |
| 改 `dom` 之後 `cargo test --workspace --no-run` | 38.7 秒 | 13.8 秒（−64%） |
| `cargo test --workspace`（編好之後） | 34.3 秒 | 21.3 秒（−38%） |
| workspace 的測試 binary | 159 個（140 個連結 V8，8.0 GB） | 44 個（30 個，1.7 GB） |

每個 crate（(b) 改實作之後 `cargo test -p <crate> --no-run`，依賴 `dom` 的改 `dom` 的一個 `expect` 的字串，`bindings`、`protocol`、`net` 改它們自己的一個常數；(c) 只改一個測試檔之後同一個指令；(d) 編好之後 `cargo test -p <crate>`）：

| crate | (b) | (c) | (d) |
| --- | --- | --- | --- |
| `dom` | 8.3 → 4.1 秒（−51%） | 1.9 → 2.4 秒 | 3.1 → 2.2 秒 |
| `shell` | 14.1 → 8.8 秒（−38%） | 2.6 → 4.3 秒 | 20.6 → 9.4 秒（−54%） |
| `dom_api` | 7.8 → 5.1 秒（−35%） | 1.5 → 2.1 秒 | 0.8 → 0.4 秒 |
| `dom_js` | 7.6 → 4.8 秒（−37%） | 1.4 → 1.7 秒 | 1.4 → 0.9 秒 |
| `bindings` | 2.0 → 1.7 秒 | 1.0 → 1.2 秒 | 0.5 → 0.4 秒 |
| `browser` | 6.0 → 6.4 秒 | 2.0 → 2.1 秒 | 3.3 → 3.3 秒 |
| `layout` | 4.3 → 4.0 秒 | 1.3 → 1.4 秒 | 0.5 → 0.4 秒 |
| `paint` | 4.3 → 4.0 秒 | 1.4 → 1.6 秒 | 0.9 → 0.9 秒 |
| `style` | 3.8 → 3.7 秒 | 1.3 → 1.4 秒 | 0.4 → 0.3 秒 |
| `html` | 3.5 → 3.6 秒 | 1.2 → 1.3 秒 | 0.8 → 0.8 秒 |
| `dom_wasm` | 6.0 → 6.1 秒 | 2.0 → 2.0 秒 | 0.8 → 0.8 秒 |
| `protocol` | 1.4 → 1.5 秒 | 0.6 → 0.6 秒 | 0.2 → 0.2 秒 |
| `net` | 1.5 → 1.7 秒 | 1.0 → 0.9 秒 | 2.4 → 2.4 秒 |

`dom` 和 `shell` 的合併前在 branch 上量（`shell` 暫時搬回去）；整個 workspace 的兩項在同一個時段量合併前和合併後。測試檔少的 crate（1 到 8 個）各自幾乎不變，效益在整個 workspace。

## 做過的檢查

2026-10-10，Windows 11。

- **第 0 段（原型）**：只合併 `dom` 時，改 `dom` 之後重編整個 workspace 的測試只從 35.9 秒到 34.0 秒。`cargo --timings` 看出原因：關鍵路徑是 `dom` → `dom_api` → `renderer` → `shell` 的 lib（約 21 秒），之後 `kun` 的執行檔和 `shell` 的 23 個測試 binary 同時 link，32 個 thread 全滿，每個 link 9 到 11 秒；`dom` 的 link 在前段還有空的 thread 時就做完了。再合併 `shell` 之後到 26.8 秒，`kun` 的 link 仍然被其他 100 個 unit 搶 CPU；沒有搶的時候，改 `dom` 之後只編 `kun` 是 7.5 秒。當時推估全部合併後 −40% 到 −47%，實際是 −64%：除了 link 的次數，剩下的 link 不再互相搶 thread。原型用 `ctor` 直接在 `main.rs` 裡設 flag（第 6 條），第 1 段改成 `bindings` 的巨集（第 10 條）。
- **搬家時碰到的**：`dom_js` 有一個測試模組叫 `bindings`，在 `it/main.rs` 裡蓋過同名的 crate，巨集寫成 `::bindings::initialize_v8_before_main!`；`include_str!`、`include_bytes!` 的相對路徑多一層 `../`（`shell` 的 `network`、`browser` 的 `navigation`）；cargo 只收一個篩選，幾個篩選放在 `--` 之後；`cargo fmt` 重排了 `use crate::common::` 之後的 import。
- **文件**：`rfcs/` 以外和還沒完成的 RFC（0004、0016、0028、0031）用腳本改（`details.md` 的「要改的指令」）；RFC 0016 的 `progress.md` 有一行在同一個指令裡給了幾個 `--test`（當時的 mutation test），改成不帶指令的文字，免得把當時的做法寫成現在的指令；parley fork 的 `README.machopo.md` 改了一行，要在 fork 自己的 repo commit。已完成的 RFC 和 `pre-rfcs/` 的 487 處不改（第 14 條）。
- **不需要 mutation test**：不改行為，測試數相同就是驗收；`test.py mutate` 只用來驗 `main` 之前的初始化（上面的驗收表）。
- **每一段做完時**：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（1853 個測試通過）、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`docs`（第 3 段起加上 `tests`）、`ci.py prune`（第 2 段清掉 110 個舊檔案，12.6 GB）；第 2 段搬了 `shell` 的 `webdriver`、`automation_pipe`，跑了 `test.py wpt`（0 個 unexpected）；完成時 `test.py automation`（`pipe --window` 的指令改成 `--test it`；35 份情境和 `pipe --window` 都過）。最後一輪 `ci.py prune` 清掉 705 個舊檔案（29.9 GB）。
- **rebase 到 `origin/main` 的 `53b48e45`**（RFC 0028 完成之後）：0028 變成已完成，對它的文件和 mutation spec 的修改全部拿掉（第 14 條，歷史紀錄）；`main` 新加的 4 個測試檔（`dom` 的 `crypto`、`dom_api` 的 `blob_urls`、`dom_js` 的 `blob`、`crypto`）照同樣的方式搬進 tests/it/，`main` 在已經搬走的檔案裡新加的 `new_dom(FLAGS)`（`timers`）和 `include_str!("resources/…")`（`browser` 的 `navigation`）跟著改；`dom_wasm` 的 README 和 `spec-missing.txt` 的衝突取 `main` 的，再改路徑、重新產生（內容和 `main` 的一樣）。之後 fmt、clippy、`cargo test --workspace`（1880 個測試通過，和 `origin/main` 的 `#[test]` 數相同）、`sanity.py` 五項都過。
- **`test.py matrix` 和 `fuzz`**（2026-10-10，macOS，Apple Silicon，`a2ffb990`）：`matrix` 的 8 種設定都通過（每種 856 個測試、17 個 binary）。`fuzz --iterations 20`（seed `0x7fa86a87`）20 組都通過。`main` 上 RFC 0030 的 GC 型別（`UserActivation` 等）原本沒有在 `matrix`、`fuzz` 底下跑過，這次一起涵蓋；RFC 0028 的 `dom_wasm` guest 的時序修正之後也是第一次跑 `fuzz`。`fuzz` 第一次在檢查 `../third_party` 時停下：本機的 rusty_v8 fork 在 `origin/kun` 最新的 `5c8e7a1`，`third_party.toml` 釘的是 RFC 0028 在 Fedora 上改成的 `8703527`（那台的 fork 還沒有 `5c8e7a1`）。兩者只差 fork 的 `README.machopo.md`，所以 pin 往前移到 `5c8e7a1`，`ci.py third-party` 確認預編的 V8 不受影響；`matrix` 是在 `5c8e7a1` 上跑的，結果同樣適用於 `8703527`。Fedora 那台要先 `git fetch` fork 才對得上。

## 跑測試的時間的摘要

原始紀錄到最後一輪檢查為止的 439 次，一共 56.1 分鐘（指令本身的時間，不含中間的思考和編輯）：

| 為什麼跑 | 次數 | 時間 | 佔比 |
| --- | --- | --- | --- |
| 量測（這份 RFC 的數字） | 264 | 24.9 分 | 44% |
| 準備（量測前的熱身，和 worktree 第一次完整編，170 秒） | 89 | 19.1 分 | 34% |
| 驗收 | 62 | 9.6 分 | 17% |
| 實作中 | 19 | 2.4 分 | 4% |
| 重跑 | 5 | 0.1 分 | 0% |

扣掉量測和準備（這份 RFC 自己的工作），一般的工作流程是 86 次、12.1 分鐘：

| 指令 | 次數 | 時間 | 佔比 |
| --- | --- | --- | --- |
| `cargo test -p <crate>`（跑） | 23 | 231 秒 | 32% |
| `test.py wpt` | 1 | 148 秒 | 20% |
| `cargo test --workspace`（跑） | 4 | 139 秒 | 19% |
| `test.py automation` | 1 | 70 秒 | 10% |
| `sanity.py` | 36 | 58 秒 | 8% |
| `cargo clippy` | 4 | 26 秒 | 4% |
| `test.py mutate` | 3 | 22 秒 | 3% |
| `cargo test -p <crate> --no-run` | 3 | 16 秒 | 2% |
| `ci.py prune` | 3 | 11 秒 | 2% |
| `cargo fmt --check` | 8 | 8 秒 | 1% |

這份 RFC 不改行為，沒有一般 RFC 的 mutation test（每份 71 到 228 條）和情境、reftest，所以上面的分布不代表一般的 RFC；要評估一般的流程，得在下一份有實作的 RFC 用同樣的方式記。紀錄本身的缺口：第 1 段開始前三次用了 `-q`，cargo 的 `Finished … in` 被藏起來，記不到編譯時間；改文件時直接跑了幾次 `sanity.py docs`，沒經過包裝腳本；log 的檔名原本只到秒，同一秒的幾次互相蓋掉 log（`test-times.tsv` 的列不受影響）。
