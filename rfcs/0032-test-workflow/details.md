# RFC 0032 的細節

## 測試的成長

`origin/main` 每天最後的 commit 上，`git grep -c '#\[test\]' -- 'crates/*.rs'` 的合計（2026-10-10 量）：

| 日期 | commit | `#[test]` |
| --- | --- | --- |
| 2026-10-01 | `229e2971` | 269 |
| 2026-10-03 | `e99f8e22` | 495 |
| 2026-10-05 | `23c97455` | 758 |
| 2026-10-07 | `2ed1d7bf` | 1218 |
| 2026-10-09 | `c279e64c` | 1677 |
| 2026-10-10 | `53b48e45` | 1880 |

WPT 經過 wptrunner 跑的清單（`tests/wpt/include.txt`，不算註解和空行）在 10-10 是 2179 行，`test.py wpt` 約 2 分鐘（Windows，32 個 process）。

## RFC 0025 的紀錄

（時間紀錄拿掉了，README 的第 1 條；這一節留著當 nextest 和 mutation test 的背景。）

RFC 0025 實作期間，每個測試和檢查的指令經過一個包裝腳本跑，每次一列記進那份 RFC 的資料夾裡的 `test-times.tsv`（完成之後刪掉，原始紀錄在 commit `a2ffb990`；開始的時間、階段、為什麼跑、指令、cargo 的 `Finished … in` 的合計、libtest 的 `finished in` 的合計、總時間、exit code、passed 和 failed、備註）。摘要在那份 RFC 的 `rfcs/0025-test-binaries/progress.md` 的「跑測試的時間的摘要」：

- 439 次、56 分鐘，其中 78% 是那份 RFC 自己的量測和熱身（它要量合併前後的時間）。
- 一般的工作流程 86 次、12 分鐘：跑一個 crate 的測試 32%、`test.py wpt` 20%（一次）、`cargo test --workspace` 19%、`test.py automation` 10%、`sanity.py` 8%。
- 那份 RFC 不改行為，沒有 mutation test（只用 `test.py mutate` 驗了 3 條）、情境和 reftest，所以這個分布不代表一般的 RFC。

## `../third_party` 對不上的那一次

2026-10-10，macOS 上的 `test.py fuzz` 開始前的 `ci.py third-party` 檢查失敗：本機的 rusty_v8 fork 在 `origin/kun` 最新的 `5c8e7a1`，`third_party.toml` 釘的是另一台機器（Fedora）改成的 `8703527`，那台的 fork 還沒有 `5c8e7a1`。兩者只差 fork 的 `README.machopo.md`；最後把 pin 往前移，並請 Fedora 那台先 fetch fork（`rfcs/0025-test-binaries/progress.md` 的「做過的檢查」）。同一天，Windows 上改了 parley fork 的 `README.machopo.md`（RFC 0025 的指令），沒有 commit 到 fork。

`fuzz` 已經在開始前檢查（所有專案，包括它用不到的 speedometer）；`matrix` 只記下 kun 和 rusty_v8 的 commit（`test.py` 的 `revisions`），`wpt`、`wdspec` 不檢查。`ci.py third-party` 只 clone 缺少的（`--clone`），已經存在但對不上的 checkout 不動，因為「它們可能有 fork 上的工作」（它的說明）。Windows 這台的 parley fork 在 `4cc636c`（fork 上只改了 `README.machopo.md`，已經 push），kun 釘的是 `8f8f6a4`；這台也沒有 ../third_party/speedometer。

## 其他做法

### mutation test：cargo-mutants

自動對每一行產生 mutant、跑測試。

- 好處：不用手寫 spec。
- 壞處：條數比現在更多（每一行、每一種運算子）；要靠設定排除不會有結果的種類；kun 的 spec 寫明每條在驗什麼，結果和審查看得懂。和「照風險挑」的方向相反。

### nextest 直接換，不先量

- 壞處：V8 每個 process 初始化一次的成本、`harness = false` 的 binary 怎麼處理，都還不知道；換了之後 `CLAUDE.md`、`docs/testing.md`、`test.py` 都要改，量了再決定比較便宜。

### 一致性：pin 放寬成「只差 README 也算」

`ci.py third-party` 在 fork 的 checkout 比 pin 新、而且只差 `README.machopo.md` 時只警告。

- 好處：只改文件的 fork commit 不必更新 kun。
- 壞處：檢查要讀 fork 的 diff；「只差文件」之後很容易變成「只差一點點」；精確的 pin 才保證每台機器編的是同一份程式。

## 參考資料

- Goran Petrović、Marko Ivanković，*State of Mutation Testing at Google*，ICSE-SEIP 2018。
- Goran Petrović、Marko Ivanković、Gordon Fraser、René Just，*Practical Mutation Testing at Scale: A view from Google*，IEEE TSE 2021。
- cargo-nextest：<https://nexte.st/>（每個測試一個 process、`slow-timeout`、JUnit 的報告）。
- Chromium 的 CQ：Chromium 的 docs/infra/cq.md；test launcher：base/test/launcher/；`gclient` 和 `DEPS`：depot_tools。
- V8 的 `tools/run-tests.py`（`--variants`、平行、每個測試的時間）。
