# RFC 0098 的進度

## 里程碑

| 里程碑 | 狀態 | RFC |
|---|---|---|
| M0 量測的基準和工具 | 做完（2026-11-14） | 這份 |
| M1 便宜的 root | 未開始 | |
| M2 索引的 query | 做完（2026-11-14） | 0099 |
| M3 單一 front | 未開始 | |

## M0

### 做了什麼（2026-11-14）

- 計數：`base::census`（`census` feature，`MACHOPO_CENSUS=<檔案>`）照 callback 記每次呼叫自己做的 scope、root 和操作；被它呼叫的 callback 做的算它們自己的。`bindings` 的每個 trampoline 記進出和 scope，`dom` 的 `Dom::read`、`Dom::mutate` 記操作，`dom_js` 標出成員。沒開 feature 時是空的。
- root 在 elise 裡數（`roots_made()`，它的 `census` feature）。
- 跑法：`bench.py census blink-perf|speedometer`（`scripts/bench/census/`，測試是 `census_test.py`）；下面的數字是做成指令之前用同樣的做法跑的。
- `bench.py calls` 加了冷的路（`classList (first)`、`nextSibling (first)`、`list[i] (first)`、`list[0]`、`0 in list`，各有自己的對照迴圈）和 `--baseline FILE`（和之前一次比：驗收的做法是改之前存一份 `calls.json`，改完用它比指令、cycles 和 store forwarding 的衝突）。

### 數到的（一輪）

Speedometer，20 個 suite 各一輪：263 萬次呼叫。

| | 次數 | 佔呼叫 |
|---|---|---|
| 登記 root 的呼叫 | 512,812（599,692 個 root） | 19.5% |
| 自己開 scope 的呼叫 | 232,437 | 8.8% |
| 經過 rusty_v8 trampoline 的呼叫 | 50,379 | 1.9% |
| 只為回報 heap 大小開的 scope | 342 | |
| 操作 | 讀 3,115,437、會進安全點的 1,063,483 | |

blink_perf 的 `bindings/`、`dom/`、`events/`（78 頁，55 頁有結果；其餘缺 `Worker`、`XMLHttpRequest`、WebGL，一頁是已知的深樹 stack overflow）：40.5 億次呼叫。

| | 次數 | 佔呼叫 |
|---|---|---|
| 登記 root 的呼叫 | 129,353,215（159,206,513 個 root） | 3.2% |
| 自己開 scope 的呼叫 | 277,689,719 | 6.9% |
| 經過 rusty_v8 trampoline 的呼叫 | 1,158,444 | 0.03% |

root 集中的成員（登記 root 的呼叫數）：

| 成員 | Speedometer | blink_perf |
|---|---|---|
| `Element.classList` getter（每次另有一個 scope、三個讀的操作） | 144,709 / 149,887 | 100,461 / 100,461 |
| `NodeList.item`（索引） | 127,711 / 207,750 | 19,580,549 / 139,007,299 |
| `HTMLCollection.item`（索引） | 122,725 / 124,935 | 109,000 / 148,639,225 |
| 舊的索引路徑（trampoline） | 50,127 / 50,127 | |
| `TreeWalker.nextNode` | 34,456 / 44,085 | 94,128 / 121,642 |
| `Scheduling.isInputPending` | | 83,066,862 / 83,066,862 |
| `EventTarget.dispatchEvent`（每次兩個） | 2,921 / 2,921 | 21,833,562 / 21,833,562 |
| `Document.createElementNS` | 6,295 / 6,295 | |

scope 集中的成員：`Document.createElement`（每次都開：Speedometer 11,902、blink_perf 1.6 億）、`CanvasRenderingContext2D.arc`（Speedometer 30,298，每次）、`Element.setAttribute`（Speedometer 42,410 次裡 10,744 次；blink_perf 不開）、Event 的 constructor、`createTextNode`。

### 佔多少指令

沒開計數的 `perf` build 跑同樣的 Speedometer 一輪，`perf stat -e instructions:u` 數每個 suite 的整個 machopo process（所有 thread，連同啟動和載入）：共 676 億條。用量過的單價（root 約 95 條、scope 27 條）換算：

| | 次數 | 估計的指令 | 佔整輪 |
|---|---|---|---|
| root | 599,692 | 約 5,700 萬 | 0.08% |
| scope（自己開的和 trampoline 的） | 285,128 | 約 770 萬 | 0.01% |

分母含其他 thread，只算 renderer 的 main thread 的話比例會高一些，但仍然遠低於 1%。

### 每次呼叫的基準（`bench.py calls`，2026-11-14）

一百萬次、三輪的中位數；指令（cycles）[store forwarding 的衝突]。存在 `calls-baseline.json`，M1、M2 用 `--baseline` 和它比。

| 成員 | |
|---|---:|
| `firstChild` | 140 (29) [0.11] |
| `nodeType` | 113 (22) [0.01] |
| `getAttribute` | 560 (91) [0.14] |
| `setAttribute` | 1071 (189) [2.55] |
| `appendChild` | 1328 (227) [7.60] |
| `createElement` | 2048 (448) [2.87] |
| `addEventListener` | 641 (123) [0.23] |
| `dispatchEvent` | 3496 (746) [12.96] |
| `classList (first)` | 2468 (653) [24.31] |
| `nextSibling (first)` | 1900 (419) [9.27] |
| `list[i] (first)` | 2289 (489) [7.83] |
| `list[0]` | 447 (85) [0.00] |
| `0 in list` | 935 (231) [4.90] |

看得出來的：

- 第一次回傳物件的路每次約 1,900 到 2,500 條（含建 wrapper 和它引發的 GC）；root 的約 95 條只佔 4% 到 5%，M1 在這條路上省得到的就是這個量級。
- `0 in list` 是 `list[0]` 的兩倍多（935 對 447），多出來的是 query 去跑整個 getter、登記 root 和 trampoline 的 scope：M2 的目標是讓它不比 `list[0]` 貴。
- `classList (first)` 每次有 24 次 store forwarding 的衝突，是表裡最多的，還沒查是哪裡。

### 查過的

- `classList` 幾乎每次都建 wrapper 不是 wrapper 沒被留住：同一個元素讀 10 萬次只建一次，1000 個元素各讀 100 次建 1000 次。Speedometer 上那 14 萬次是第一次讀到那個元素的 `classList`。

### 做過的檢查

- `cargo test -p base`（開和不開 `census`）、`-p bindings -p dom -p dom_js`、sanity 的各項：通過。elise 的 `cargo test --workspace`、clippy（開 `census`）、sanity：通過。
- 計數的測試只在開了 feature 時跑（`cargo test -p base --features census`；elise 的 `the_census_counts_the_roots_made` 開了才檢查到數目），CI 不開它。還沒跑 mutation test。
- M0 做完時（2026-11-14）：`cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（另外開 `census` 對 `shell` 跑一次）通過；`scripts/bench/calls/calls_test.py`（11 個）、`scripts/bench/census/census_test.py`（3 個）通過。
