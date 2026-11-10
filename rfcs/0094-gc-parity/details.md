# RFC 0094 的細節：GC 和 Chrome 持平

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 2026-11-10 的量測

Linux（Fedora，kernel 7.2.11）、AMD Ryzen AI 9 HX 370（兩個瀏覽器綁在 CPU 0-3、12-15）、接電源；machopo `277209df416f`（`perf` profile），Chrome for Testing 152.0.7977.82（headless）。用暫時的 `bench.py blink-perf --gc`（`README.md` 的切法，沒有 `v8.memory` 的那幾欄），六頁各 3 次、取中位數；再用暫時的程式讓 machopo 的 trace 多錄 `v8.memory` 和 `disabled-by-default-v8.gc`、Chrome 多錄 `v8.memory`，三個 `gc-*` 頁各一次。都沒有留下來。

### main thread 上的 GC

machopo / Chrome，ms（成績是開著 trace 的）：

| 頁 | 成績 | GC | DOM heap | 其中 `CppGC.*` | V8 | 其他 thread | 其他 thread 的 DOM heap | 最長的停頓 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `bindings/gc-forest` | 90.5 / 59.4 ms | 244 / 304 | 7.8 / 99 | 5.5 / 99 | 236 / 205 | 331 / 522 | 0.35 / 244 | 9.2 / 12.4 |
| `bindings/gc-mini-tree` | 108 / 95.9 ms | 240 / 311 | 7.4 / 119 | 5.2 / 119 | 233 / 193 | 304 / 644 | 0.33 / 384 | 9.7 / 15 |
| `bindings/gc-tree` | 123 / 117 ms | 278 / 326 | 9.4 / 114 | 8.5 / 114 | 269 / 212 | 539 / 781 | 0.09 / 431 | 21.7 / 25.9 |
| `bindings/create-element` | 2427 / 3555 runs/s | 1181 / 2946 | 55 / 1239 | 37 / 1239 | 1126 / 1707 | 1404 / 5255 | 2.5 / 3072 | 4.6 / 4.7 |
| `bindings/insert-before` | 449 / 146 runs/s | 3.5 / 873 | 0 / 755 | 0 / 755 | 3.5 / 117 | 0.4 / 3360 | 0 / 3163 | 0.2 / 55 |
| `parser/innerHTML-setter` | 10048 / 6544 runs/s | 425 / 2964 | 119 / 2491 | 81 / 2491 | 306 / 473 | 1294 / 6822 | 5.4 / 5787 | 1.8 / 86 |

- `insert-before` 上 machopo 快 3 倍，平常的 `blink-perf` 會標成可疑（兩邊做的事可能不同），不拿來下結論。
- **DOM heap**：elise 的 marking 大多在它自己的 worker 上，main thread 上只有很短的步驟（`Gc.Step`，一次約 1 µs）和收尾，sweep 不在 main thread 上；machopo 的 DOM heap 時間大半是空的 `CppHeap`（V8 每一步照樣推它），elise 的 `Gc.*` 只有 2 到 38 ms。Oilpan 的 sweep 有一大塊在 main thread 上（三個 `gc-*` 頁 70 到 90 ms，`create-element` 和 innerHTML 約 1 秒），收尾的 `CppGC.SweepFinish` 造成最長的停頓（innerHTML 86 ms）。
- **其他 thread**：elise 的 worker 不發事件，machopo 那一欄只有 `CppHeap` 的。
- **依種類**：三個 `gc-*` 頁 machopo 的 atomic 114、114、72 ms，Chrome 62、78、57 ms；main thread 上的 sweeping machopo 約 0.1 ms，Chrome 70 到 90 ms。`create-element` 上 machopo 沒有 scavenge（Chrome 308 ms），原因沒有查。
- **停頓**：Chrome 每次有 8000 到 18 萬個停頓，多半是 Oilpan 在 low priority task 裡 sweep（一次約 4 µs），中位數看不出什麼，所以報告另外數 1 ms 以上的。

### V8 那一側：full GC 的次數

三個 `gc-*` 頁，V8 的 phase 自己的時間（3 次的中位數）和次數，machopo / Chrome：

| | `gc-forest` | `gc-mini-tree` | `gc-tree` |
| --- | ---: | ---: | ---: |
| full GC 的次數 | 59 / 25 | 51 / 47 | 11 / 6 |
| scavenge 的次數 | 41 / 71 | 22 / 63 | 43 / 65 |
| `V8.GC_MC_CLEAR_JOIN_JOB`（ms） | 48.0 / 15.3 | 40.6 / 21.8 | 29.9 / 13.9 |
| `V8.GC_MC_INCREMENTAL`（ms） | 43.1 / 22.3 | 42.5 / 19.3 | 46.9 / 17.1 |
| `V8.GC_MC_EVACUATE_COPY_PARALLEL`（ms） | 12.6 / 0.04 | 13.1 / 0.07 | 13.4 / 0.02 |
| `V8.GC_SCAVENGER_TRACED_HANDLES_RESET_PARALLEL`（ms） | 46.0 / 59.7 | 39.4 / 45.5 | 85.4 / 78.4 |

- full GC 比較多次，`V8.GC_MC_INCREMENTAL`（V8 自己的 marking 步驟）合計就多，每次反而不比 Chrome 長。
- full GC 一多，新生代還沒 scavenge 就被它搬（`EVACUATE_COPY_PARALLEL`），scavenge 也比較少。
- `V8.GC_MC_CLEAR_WEAK_GLOBAL_HANDLES`（在 `CLEAR_JOIN_JOB` 裡）除了 weak 的 `v8::Global`，也清死掉的 wrapper 的 traced handle（`traced_handles()->ResetDeadNodes`）。三個頁的迴圈不把字串交給 JS，不產生字串快取的 weak handle；machopo 每個物件一個的 weak `v8::Global` 只有字串快取（`dom_js` 的 `strings.rs`，Blink 的 `StringCache` 也一樣）。清死掉的 wrapper 的工作，machopo 多半落在 full GC，Chrome 多半落在 scavenge（`TRACED_HANDLES_RESET_PARALLEL`）。
- global 加 traced handle 的用量（V8 的 `HeapStatistics::used_global_handles_size` 兩者合算；machopo 每次報大小給 V8 時記一次，Chrome 從 memory-infra 每 200 ms 一次的 `v8/main/global_handles`），中位數 machopo / Chrome：`gc-forest` 2241 / 2210 KiB，`gc-mini-tree` 2049 / 987 KiB，`gc-tree` 10303 / 6218 KiB。取樣的頻率不同，只比中位數。

### 為什麼 full GC 比較多次：loading 時的 overshoot margin

`V8.GCIncrementalMarkingStart` 的 debug annotation 帶著原因和大小。兩邊的原因都是「global allocation limit reached」，上限也差不多（`gc-forest` 大多在 10 到 11 MB），差在開始時的用量：

| | 上限（MiB） | 開始時的全域用量（MiB） |
| --- | ---: | ---: |
| machopo，`gc-forest` | 10.80 | 11.05 |
| Chrome，`gc-forest` | 11.56 | 43.57 |
| Chrome，`gc-forest` | 30.15 | 62.37 |
| Chrome，`gc-tree` | 285.91 | 429.03 |
| Chrome，`gc-tree` | 482.73 | 724.30 |

Chrome 每一次都等於上限加上 `global_overshoot_margin`（`max(上限 / 2, 32 MiB)`，`heap-controller.h`）：V8 的 `IncrementalMarkingLimitReached` 在 `ShouldOptimizeForLoadTime()`（`IsLoading()` 而且還沒超過這個 margin）時不開始 marking；`IsLoading()` 由嵌入端的 `Isolate::SetIsLoading(true)` 打開，最多 7 秒（`kMaxLoadTimeMs`）。machopo 沒有呼叫它（rusty_v8 也沒有接出它），跨過上限就開始。Chromium 在什麼時候呼叫它沒有查證。

- 代價是記憶體：`gc-forest` 期間嵌入端的最高用量 machopo 167 MiB、Chrome 302 MiB。
- **同樣的節點 elise 報的大小和 Oilpan 差不多**：full GC 之後的嵌入端（全域減 old generation 和 external，GC 結束後的第一筆 counter），`gc-forest` 7.50 / 7.41 MiB、`gc-mini-tree` 7.50 / 7.32 MiB。`gc-tree` 是 35.5 / 217 MiB，但 Chrome 在 heap 很大時才 GC、Oilpan 之後才 sweep，GC 剛結束時用量還沒降，比不了。
- **嵌入端的 GC 速度**（`disabled-by-default-v8.gc` 的 `EmbedderSpeed`）machopo 約 2,000 bytes/ms、Chrome 約 1,500,000 到 2,100,000：V8 照空的 `CppHeap` 標到的 bytes 和推它的時間算。V8 的 `DynamicGrowingFactor` 在 GC 慢時給比較大的成長倍數，所以它沒有讓 machopo 的 GC 變多；但讀了會誤導（決定 4）。

## 2026-11-10 的粗估

用上面的數字粗估每個候選能省多少（main thread 上、整段計時的區間的合計，ms；只用來排第 3 段的評估，不是承諾）：

| 候選 | `gc-forest` | `gc-mini-tree` | `gc-tree` | `create-element` | `innerHTML-setter` | 把握 |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| 告訴 V8 頁面正在載入 | 約 −60 | 約 −10 | 約 −50 | ? | ? | 中 |
| 拿掉空 `CppHeap` 的開銷 | −4 到 −5 | −4 到 −5 | −6 到 −8 | 約 −30 | 約 −65 | 中高 |
| atomic 停頓裡清 handle 每次比較久 | 約 −5 | −5 到 −10 | 很小 | ? | ? | 低 |

- **loading**：full GC 的次數降到和 Chrome 一樣（`gc-forest` 59 → 25），atomic 停頓（每次約 1.9 ms）和 V8 自己的 marking 步驟照次數減；full GC 不再搬新生代，scavenge 變多，加回約 35 ms（`gc-forest`）。`gc-mini-tree` 的次數本來就差不多。loading 最多 7 秒，Speedometer 上的效果不確定。
- **空 `CppHeap`**：它在 main thread 上 5.5、5.2、8.5、37、81 ms；V8 推嵌入端的那一層要留，估七到八成。在 Speedometer 的取樣裡沒有列成主要的開銷。
- **清 handle**：每次 full GC 多約 0.25 ms，乘上次數。
- **Speedometer 的 GC 那一類**（28 對 18 ms）：配置的快路（`alloc_at`、`Inner::allocate` 各 2 到 3 ms）已經是 bump 加 header，估能省 2 到 4 ms；`HeapVector` 和它的 store 每次兩個物件。Chrome 的 cppgc 配置算在 DOM，對齊歸類之後真正的差距比 10 ms 小，多少要量。
- **換成整頁的成績**：`gc-forest` 省 60 ms，每個 iteration 約 3 ms（90.5 → 約 87 ms，Chrome 59.4 ms）；整頁慢的大多在 GC 以外。

### 工具上踩到的

- machopo 的 V8 的事件和 machopo 的 span（elise 的在裡面）在同一個 thread 的同一個 track 上，不用合併 track；試過照時間重新套疊同一個 thread 的 slice，Chrome 的時間只到 µs、同一刻開始和結束的很多，順序被打亂（零長度的 slice 被排成先結束再開始），全部配錯。
- machopo 的 stdout 是 automation pipe：`--trace-gc` 的輸出寫不出來，`--headless` 在頁面 load 完就結束、等不到 runner。要 V8 的 GC 的細節，從 trace 的 annotation 和 counter 讀。
