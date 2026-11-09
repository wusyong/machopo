# RFC 0055 的細節

## `timed_windows` 算多了多少

2026-10-18，Windows 11，`main` 的 `1b3eba12`，`python3 scripts/ci.py speedometer --trace --browser kun --iterations 3`（20 個 default suite）。三種區間的定義，對同一批 trace：

| 定義 | 區間數 | 合計 | main thread 閒著（沒有 span 開著） |
| --- | --- | --- | --- |
| 原本的 `timed_windows`：任何一個 `FireAnimationFrame` 到之後第一個 `TimerFire` 的結束，重疊的合併 | 187 | 9393 ms | 2214 ms |
| 任何有 `FireAnimationFrame` 的 frame，到 frame 之後第一個 `TimerFire`，中間沒有別的 frame | 187 | 7046 ms | 79 ms |
| 有兩個以上 `FireAnimationFrame` 的 frame（runner 的兩個 rAF），同上 | 174 | 6767 ms | 1.6 ms |

最後一種每個 suite 9 個區間（3 個 iteration × 3 個 test；兩個 Editor 是 2 個 test，6 個），扣掉無效的 Editor-CodeMirror 合計 6679 ms；2026-10-18 的比較表（另一次 run）19 個有效 suite 的中位數合計 2220 ms，乘 3 是 6660 ms。

原本的定義多算的例子（TodoMVC-React-Complex-DOM 的第一個區間，65.4 ms）：一個只有頁面自己一個 rAF callback（0.24 ms）的 frame，接著 main thread 閒了 20 ms（compositor 在 raster），才是 runner 的 frame（sync 的 callback 20.25 ms、排 timer 的 0.08 ms），frame 結束後 timer 馬上跑。runner 量的只有後面那個 frame 的 callback 到 timer。

## 照 frame 找的準確度

同一次 run 裡每個區間和 runner 自己量的那個 test 的 sync + async 逐一比（腳本用 `run_in_kun` 錄 trace，同時拿到 `measure.mjs` 回報的每個 test 的時間；`perf` profile，各 3 個 iteration）：

| suite | 區間 / test | 區間合計 / runner 合計（ms） | 每個 test 的差（區間 − runner，ms）：中位數、最小、最大 |
| --- | --- | --- | --- |
| TodoMVC-JavaScript-ES5 | 9 / 9 | 313.3 / 312.9 | +0.04、−0.02、+0.10 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 9 / 9 | 540.4 / 539.5 | +0.12、−0.13、+0.20 |
| TodoMVC-WebComponents | 9 / 9 | 187.6 / 186.4 | +0.09、+0.02、+0.51 |
| TodoMVC-React-Complex-DOM | 9 / 9 | 217.2 / 216.4 | +0.07、+0.04、+0.19 |
| TodoMVC-React-Redux | 9 / 9 | 142.8 / 142.1 | +0.08、+0.04、+0.10 |
| TodoMVC-Backbone | 9 / 9 | 158.7 / 160.5 | −0.20、−0.43、−0.02 |
| TodoMVC-Angular-Complex-DOM | 9 / 9 | 185.8 / 184.8 | +0.11、+0.01、+0.27 |
| TodoMVC-Vue | 9 / 9 | 128.4 / 127.6 | +0.07、+0.01、+0.20 |
| TodoMVC-jQuery | 9 / 9 | 1516.3 / 1514.8 | +0.12、+0.04、+0.54 |
| TodoMVC-Preact-Complex-DOM | **10 / 9** | | |
| TodoMVC-Svelte-Complex-DOM | 9 / 9 | 120.3 / 119.6 | +0.10、−0.13、+0.22 |
| TodoMVC-Lit-Complex-DOM | 9 / 9 | 157.1 / 156.0 | +0.11、+0.02、+0.21 |
| NewsSite-Next | 9 / 9 | 513.2 / 512.5 | +0.08、+0.00、+0.14 |
| NewsSite-Nuxt | 9 / 9 | 389.5 / 388.1 | +0.09、+0.03、+0.68 |
| Editor-CodeMirror | 6 / 6 | 90.2 / 89.5 | +0.02、−0.05、+0.66 |
| Editor-TipTap | 6 / 6 | 444.4 / 444.3 | +0.02、−0.04、+0.12 |
| Charts-observable-plot | 9 / 9 | 198.9 / 198.2 | +0.07、+0.02、+0.14 |
| Charts-chartjs | 9 / 9 | 633.0 / 632.8 | −0.01、−0.09、+0.30 |
| React-Stockcharts-SVG | 9 / 9 | 335.3 / 369.0 | +0.04、**−12.23**、+0.72 |
| Perf-Dashboard | 9 / 9 | 464.8 / 464.2 | +0.05、−0.04、+0.19 |

兩個例外，只看結構分不出來：
- **TodoMVC-Preact-Complex-DOM**：第 1、2 個 iteration 之間多一個 32.4 ms 的區間：一個有兩個 rAF callback、之後有 timer 的 frame，不是任何 test 的（約多 27%）。
- **React-Stockcharts-SVG 的 PanTheChart**：每個 iteration 少約 11 ms（19.0 / 30.2、14.7 / 26.1、15.5 / 25.6）：frame 之後先跑的 timer 是頁面自己的，runner 的 timer 在後面（約少 9%）。

frame 之後到下一個 frame 之間的 timer 數：多數 suite 是 2 個（runner 的 async 和回報結果的），有的 3 個以上（Backbone 有一次 105 個），多的大多很短；回報結果的那個做 `measure.mjs` 的摘要，比較長。

## Chrome 的 user timing 事件

2026-10-18，Chrome for Testing 152.0.7977.82，經過 CDP 的 `Tracing.start` 只錄 `blink.user_timing`，跑 TodoMVC-Vue 一個 iteration，看 `Adding100Items` 的事件（`TrackEvent.type`：1 slice 的開始、3 instant；有沒有 `track_uuid`）：

| 名稱 | 種類 | track |
| --- | --- | --- |
| `TodoMVC-Vue.Adding100Items-start` | instant | thread 的 |
| `TodoMVC-Vue.Adding100Items-sync-end` | instant | thread 的 |
| `TodoMVC-Vue.Adding100Items-async-start` | instant | thread 的 |
| `TodoMVC-Vue.Adding100Items-async-end` | instant | thread 的 |
| `TodoMVC-Vue.Adding100Items-sync` | slice 的開始 | 自己的 |
| `TodoMVC-Vue.Adding100Items-async` | slice 的開始 | 自己的 |

slice 的結束事件沒有名稱（Perfetto 的 slice 結束不帶名稱），所以只數到開始。

**track 和時間**（2026-10-18，macOS，同一個 Chrome，經過 CDP 只錄 `blink.user_timing` 和 `toplevel`，開一個本機的小頁面：忙等 50 ms 之後 `performance.mark("past", {startTime: t0})`、`mark("now")`，`measure("m1")` 兩次（`t0` 到 `t0 + 10`、`t0 + 20` 到 `t0 + 30`），`measure("m2", "past", "now")`，`measure("m3")`（`t0 + 5` 到 `t0 + 40`，和前面的重疊），`new PerformanceMark("ctor")`，100 ms 之後的 timer 裡再一個 mark 和一個 `m1`（`t0 + 1` 到 `t0 + 2`）；讀 trace 的 `track_descriptor`、`track_uuid` 和時間戳）：

- **時間是 entry 的時間**：`past` 的 instant 在 `t0`，比 `now` 早 51.6 ms；每個 measure 的開始和結束就是給的兩個時間（`m1` 剛好 10 ms，`m2` 從 `past` 到 `now`）。事件照呼叫的順序寫進 sequence，時間戳不一定往前走（timer 裡的 `m1` 在 `t0 + 1`）。
- **mark 在 renderer main thread（`CrRendererMain`）的 track 上**，事件沒有 `track_uuid`，用 sequence 的預設 track。
- **每個 measure 一條自己的 track**：五個 measure 五條，同名的三個 `m1` 也各一條；track 沒有名稱，parent 是 renderer main thread 的 track（Perfetto 的 `perfetto::Track(id, ThreadTrack::Current())`，uuid 是 id 和 thread 的 track 的 uuid 合起來的）。開始和結束都帶 `track_uuid`。
- **`new PerformanceMark()` 不寫**：trace 裡只有 `past`、`now` 和 timer 裡的 mark。
- **負的和未來的時間照寫**（另一個頁面：`measure("neg", {start: t0 + 10, end: t0})`、`measure("fut", {start: t0, end: t0 + 1e6})`、`mark("futmark", {startTime: t0 + 1e6})`）：`neg` 的結束事件在開始之前 10 ms，`fut` 的結束和 `futmark` 在 1000 秒之後，都不夾。
- **事件的參數**：mark 有一個 `data`（`startTime`、`callTime`、`sampleTraceId`、`navigationId`），measure 的開始有 `startTime` 等三個參數。
- `blink.user_timing` 也有 Blink 自己的 navigation timing 的 instant（`navigationStart`、`domLoading`、`loadEventEnd`……），在 frame 的 track 上（`track_uuid` 0、帶 legacy 的欄位），不是 `performance.mark`。
