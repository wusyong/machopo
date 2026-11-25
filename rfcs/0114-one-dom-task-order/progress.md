# RFC 0114 的進度：DOM 的 task 只有一套順序，worker 和主 thread 一樣

開始日期：2026-11-25

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. scheduler 的核心抽到 `base`，主 thread 改用它，行為不變（已決定的事項第 1、5、7 條） | 做完，待審查 | 2026-11-25 |
| 2. worker 的 loop 改用 scheduler | 沒開始 | |
| 3. 拿掉 `dom` 的順序 | 沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| scheduler 的核心：順序、延後的 task 取的時候才排、等待、同步等回答、`take_matching`、`put_back`、`post_with`、`close` | `base` 的 `sequence_manager::tests`（`cargo test -p base --lib sequence_manager`） |
| 主 thread 的 scheduler 行為不變 | `renderer` 的 `task_queue::tests`、`shell` 的整合測試（`event_loop::`、`workers::`、DevTools 的）照舊通過 |

## 做過的檢查

### 第 1 段（2026-11-25，Linux：Fedora 45，debug build，`8cabbc17` 之上）

**做了什麼**：`base::sequence_manager`（`Spec`、`SequenceManager`、`Closed`）放 `task_queue.rs` 原本的 queue、sequence、延後 task 的 heap、`queue_due`、`earliest`、等待、同步等回答；`MainThreadScheduler` 改成建在它上面（`MainSpec`：`Queued` 帶 `interrupt`，`MainExtra` 是 `posted_once` 和 `shutting_down`，`taken` 清掉 `posted_once`）。`post_internal_once_at` 經過 `post_with`，`take_interrupting` 經過 `take_matching`，`put_back`、`sync_call`、`close` 照舊的意思。原本只為 DOM 和 renderer 自己的 queue 的延後 task 醒來，改成為「要的 queue」的延後 task 醒來：主 thread 的延後 task 只在這兩個 queue，等價。

**測試**：`base` 的 `sequence_manager::tests` 七個。`base`、`renderer`、`shell` 的測試通過；`sanity.py` 的六項、`base` 和 `renderer` 的 clippy 通過。

**mutation test**（`mutants-1.toml`，12 個）：第一輪 8 個抓到、3 個「hung」（沒有時限的等待在 mutant 底下等不到，測試卡住到 mutation 的時限）、1 個沒抓到。把三處等待改成有時限（等延後的 task 時要在 0.5 秒內回來、同步等回答放在別的 thread 上用 `recv_timeout`），那 3 個重跑都直接失敗。沒抓到的「沒在等的 queue 的延後 task 也叫醒等待」是等價的：多醒一次、照樣回去等，記進 `docs/test-gaps.md`。
