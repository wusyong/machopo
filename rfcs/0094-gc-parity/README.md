# RFC 0094：GC 和 Chrome 持平：量測（`bench.py blink-perf --gc`、V8 的記憶體 counter、elise 的 worker 的事件）和評估

- 開始日期：2026-11-10
- 狀態：✅ 已完成：2026-11-10（三段：量測的工具、elise 的 worker 的事件、評估；評估的結論是不做優化的段，決定 11。驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`

## 摘要

`bench.py blink-perf --gc` 讓每一頁在兩個瀏覽器裡各錄幾次 trace，從 runner 計時的區間讀 renderer 的 GC：main thread 上的總時間，分成 V8 的和 DOM heap 的（elise 對 Oilpan）、依種類分、停頓，其他 thread 的時間；兩邊的 trace 都錄 V8 的 `v8.memory`，報告多寫 full GC 和 scavenge 的次數、marking 開始的原因和超過上限多少、full GC 之後嵌入端的大小。elise 的 worker 照 cppgc 發 trace 事件，machopo 在背景的 DOM heap 時間才量得到。不開 trace 時 elise 的每一段 phase 只讀一個旗標。最後一段用這些工具評估了各種頁和 Speedometer 的 GC 那一部分離 Chrome 多遠：回收那一側已經持平或更好（DOM heap 的總 CPU 是 Oilpan 的 2% 到 36%，Speedometer 上回收差 2 ms），剩下的差距大半是配置，GC 全部追平對整體只有 1.384 → 1.373，所以不做優化的段（決定 11），候選留在「之後的可能」。

## 動機

**現況**（2026-11-10，Linux、AMD Ryzen AI 9 HX 370；量法和數字見 `details.md` 的「2026-11-10 的量測」）：

- 和 Chrome 比 GC 的工具只有兩種：`bench.py gc`（elise 的 microbenchmark，沒有 Chrome）和 `bench.py speedometer --sample`（取樣的 GC 那一類，只看 main thread 的 CPU、配置算給誰兩邊不同）。以前的 `bench.py blink-perf --gc`（RFC 0088 用的）跟著舊的 DOM 刪掉了（RFC 0089 的 5b-1），`common.py` 的 `CHROME_GC_TRACE_CATEGORIES` 沒有人用。
- 2026-11-10 用暫時的工具量了 blink-perf 的六頁，看到：main thread 上 GC 的總時間 machopo 都比 Chrome 少（DOM heap 那一側少很多）；三個 `gc-*` 頁上 V8 那一側 machopo 多 30 到 57 ms，因為 full GC 比較多次。原因要另外錄 `v8.memory` 才看得到：V8 在每次 marking 開始時記下原因、上限和用量，Chrome 每次都在上限加上 loading 時的 overshoot margin 才開始，machopo 一跨過上限就開始（machopo 沒有呼叫 `Isolate::SetIsLoading`）。這些是用暫時的程式和腳本量的，沒有留下來。
- elise 的 worker 不發 trace 事件：machopo 在背景做的 marking 和 sweep 量不到，「DOM heap 在 main thread 上比 Oilpan 便宜」說不出總 CPU 是贏是輸（Oilpan 在背景的時間量得到）。

**目標**：

- 和 Chrome 比 GC 有一個能重複的工具：同一份頁面、同一個區間、同樣的分類，改了 GC、bindings 或 V8 的設定之後照樣量、照樣比。
- 報告說得出 full GC 為什麼比較多或比較少（觸發的原因、上限、用量），不只是時間。
- machopo 的 DOM heap 在其他 thread 上的時間量得到，和 Oilpan 的比。
- **評估 GC 的部分能不能、值不值得和 Chrome 持平**：blink-perf 的頁（`--gc` 量）和 Speedometer 的 GC 那一類（`speedometer --sample`；2026-11-08 machopo 28 ms、Chrome 18 ms，`docs/performance-gaps.md`）；每個候選估能省多少、成本多少，和其他類別的差距比。做不做在評估之後決定（決定 5）。

**要解決的 `docs/limitations.md` 的限制**：無。

**不在範圍內**：

- machopo 的 trace 錄 `disabled-by-default-v8.gc`（決定 4）。
- Speedometer 每個 suite 從 trace 拆 GC（`--gc` 的切法用在 Speedometer 上）：第 3 段用取樣的 GC 那一類量 Speedometer（決定 5）。
- GC 以外的差距（bindings、DOM、JS）：GC 那一類持平之後，整頁的成績不一定持平。
- 優化本身：評估之後再決定（決定 5）。

## 設計概要

```sh
python3 scripts/bench.py blink-perf --gc --pages bindings/gc-forest.html,bindings/gc-tree.html
```

每一頁照 `--runs`（預設 3）在 machopo 和 Chrome 裡輪流跑，每次一個新的瀏覽器、開著 trace，trace 留在 `target/bench/blink_perf/gc/`（Perfetto UI 打得開），報告是 `target/bench/blink_perf/gc.md`（和 `gc.json`）。報告的每一格是「machopo / Chrome」：

- 成績（開著 trace 的）、計時的區間、main thread 上的 GC 和佔的比例、其中 DOM heap 的和 V8 的、DOM heap 裡 `CppGC.*` 的（Chrome 的全部；machopo 的是空的 `CppHeap`）、其他 thread 的 GC 和其中 DOM heap 的。
- 依種類：incremental marking、atomic、young（V8 的 scavenge）、sweeping。
- 停頓：每次的個數和其中 1 ms 以上的、中位數、p95、最長的，和最長那次裡花最多時間的 phase。
- V8 的排程（`v8.memory`）：full GC 和 scavenge 的次數、marking 開始的原因、開始時用量是上限的幾倍、full GC 之後嵌入端的大小。

`--gc` 和 `--against` 不能一起用。選頁的規則和平常的 `blink-perf` 一樣（清單、路徑、`--pages`、`--dirs`）。

## 詳細設計

### 計時的區間和 GC 的切法

- **區間**：runner 的 `console.time("blink_perf")`（第一個計時的 iteration 到最後），`hook.js` 已經把它變成 User Timing 的 mark 和 measure；mark 所在的 thread 是 runner 的 thread（Chrome 有好幾個 renderer），measure 在自己的 track 上，照 Blink 的寫法。Chromium 的 `tools/perf/benchmarks/blink_perf.py` 也照這個區間讀 trace。
- **GC 的 slice**：沿用 `bench.py gc` 的 `gc_bench.window_gc`（最外層的 GC slice、種類、相隔不到 10 µs 的算同一次停頓、其他 thread 的時間）。只算 runner 的 thread 所在的 process（Chrome 的 browser 和其他 renderer 也有 V8）。V8 處理 GC 請求的那一層（`V8.GCHandleGCRequest`，Chrome 的 full GC 多半包在裡面）拿掉，裡面的 GC 才是最外層、照它的種類算。trace 本來的順序配對 begin 和 end：Chrome 的時間只到 µs，同一刻開始和結束的 slice 很多，照時間重排會配錯（2026-11-10 試過）。
- **DOM heap 的**：每個 thread 上最外層的 `CppGC.*` 和 `Gc.*`（elise 的）slice，在 V8 的 GC 裡或自己一段。machopo 的 `CppGC.*` 是空的 `CppHeap`（V8 照樣推它），另外一欄。

### V8 的排程（`v8.memory`）

- machopo 的 trace（`bindings` 的 `trace.rs` 的 category）和 `--gc` 的 Chrome 都多錄 `v8.memory`：V8 的 `GlobalConsumedBytes`、`OldGenerationConsumedBytes`、`ExternalMemoryBytes` 和兩個上限的 counter。所有用 `MACHOPO_TRACE` 的量測都會多錄這些 counter（決定 2）。
- `V8.GCIncrementalMarkingStart` 本身帶著原因、上限和用量（`reason`、`global_allocation_limit`、`global_consumed_bytes`……），報告讀它：原因的次數，開始時全域用量是上限的幾倍（中位數）。
- full GC 之後嵌入端的大小：每次 `MajorGC` 結束後的第一筆 counter，全域減掉 old generation 和 external。Oilpan 之後才慢慢 sweep，GC 剛結束時用量還沒降，報告要寫明這一點（`gc-tree` 這種 heap 很大才 GC 的頁比不了，見 `details.md`）。
- `common.py` 要能讀 Perfetto 的 counter（`TrackEvent` 的 `TYPE_COUNTER`，名稱在 track descriptor 的 `static_name`）和 debug annotation（巢狀的 dict）。

### elise 的 worker 的事件

- elise 的平行工作都經過 `Platform` 的 job：marking（posted 的和停頓裡一起標的）、sweep、平行的 scavenge、把空頁還給作業系統（discard）。worker 每跑一次 job，job 自己經過 `Platform::trace_begin` 發一段 span，照 cppgc 的 `StatsCollector::EnabledConcurrentScope`（`CppGC.ConcurrentMark`、`CppGC.ConcurrentSweep`）：`Gc.ConcurrentMark`、`Gc.ConcurrentSweep`、`Gc.ParallelScavenge`、`Gc.Discard`（決定 8）。joining 的 thread（heap 的）不發：它跑的那一份已經在 `Gc.Mark`、`Gc.Sweep`、`Gc.Scavenge` 裡。span 由 drop 結束（run panic 時也結束）。不開 trace 時每一段（原本的 GC phase 和 worker 的 run）只多一次讀取和判斷（決定 9；machopo 的 `V8GcPlatform` 經過 V8 的 Perfetto，任何 thread 都能呼叫）。
- elise 的 `platform.rs` 的 `WorkerSpan`；四種 job（`heap/concurrent.rs` 的 `Marker`、`heap/sweep.rs` 的 `Sweeper`、`heap/young.rs` 的 `Scavenging`、`heap/discard.rs` 的 `Discards`）各自拿著 heap 的 platform。
- 在 elise 改（`../third_party/elise`，照它的 `../third_party/elise/docs/contributing.md`），push 到它的 `main` 之後更新 machopo 的 `third_party.toml`。
- `--gc` 的報告的「其他 thread 上的 DOM heap」，machopo 這一側從此有數字。

### 評估（第 3 段）

用第 1、2 段的工具和 Speedometer 的取樣量一次（決定 10），只量測、不改程式。結果（數字和拆法在 `progress.md` 的「量測和評估」）：

- **DOM heap 的總 CPU（main thread 加其他 thread）elise 比 Oilpan 少**：blink-perf 的五頁是 Oilpan 的 2% 到 36%；main thread 上的 GC 六頁都比 Chrome 少，最長的停頓不比 Chrome 長，記憶體的高峰比較低。
- **V8 那一側在短的 `gc-*` 頁多 19 到 77 ms**：Chrome 在 V8 的 loading 狀態（`Isolate::SetIsLoading`，最多 7 秒），超過上限一個 margin 才開始 marking，full GC 比較少次；超過 7 秒的頁上兩邊一樣。
- **Speedometer 取樣的 GC 那一類 27.8 對 16.1 ms**：其中 9.7 ms 是 DOM heap 的配置（Chrome 的 cppgc 配置 inline、算在 DOM），回收那一側 18.1 對 16.1 ms；V8 的 scavenge 和 handle 多的約 4 ms 裡，重設被丟掉的 wrapper 的 root 的路約 1.6 ms、字串快取的 weak global 約 0.5 ms。GC 全部追平時整體的比值 1.384 → 1.373（文字、style、JS 各自追平時是 1.266、1.276、1.295）。
- `create-element` 上 machopo 沒有 scavenge 不是問題：full GC 的節奏和 Chrome 一樣，新生代還沒滿就被 full GC 清掉。

結論：不做優化的段（決定 11）；候選和各自的收益、成本在「之後的可能」。

### 測試

- `blink_perf_test.py`：區間（mark 的 thread、measure 的結束、不是 User Timing 的同名 slice）；main thread 和 process 的過濾、DOM heap 的時間（巢狀的、切在區間上的、`CppGC.*` 的那一欄）、拿掉 `V8.GCHandleGCRequest`；每次 machopo 先、Chrome 用 GC 的 category；報告的格子（中位數、停頓的分佈、1 ms 以上的）；counter 和 annotation 的讀法（自己寫的 protobuf）；`--gc` 和 `--against`。真的瀏覽器：fake 的頁（`fake/gc/`）建、丟一大串元素，兩邊在區間裡都有 scavenge。
- elise：worker 的 span 有開始有結束、在 worker 上（elise 的測試的 `Platform` 記下 `trace_begin`）。
- mutation test 照 `docs/testing.md`。量測：六頁量一次，結果寫進 `progress.md`。
- 第 3 段只量測：`--gc` 量那幾頁、`speedometer --sample` 量 GC 那一類。量的時候發現 `categories.py` 還照 GC 的 crate 的舊名字分類（elise 的樣本掉進「other」），修好、`speedometer_test.py` 的斷言跟著改。

### 分階段

| 段 | 內容 |
| --- | --- |
| 1 | `bench.py blink-perf --gc`，兩邊錄 `v8.memory`，報告的 V8 的排程 |
| 2 | elise 的 worker 的事件，`--gc` 讀它們 |
| 3 | 評估：重量、估每個候選能省多少和成本；結論是不做優化的段 |

## 缺點

- 所有 `MACHOPO_TRACE` 的 trace 多了 `v8.memory` 的 counter（2026-11-10 的 `gc-forest` 約 3300 筆，Chrome 約 6400 筆）。
- 開著 trace 量：成績和平常的 `blink-perf` 不同，machopo 錄的 category 比 Chrome 多。
- 要讀 Perfetto 的 counter 和 annotation：`common.py` 自己的 protobuf 解析多一塊要維護。
- 第 3 段之後若決定做，候選有幾項要改 rusty_v8 的 fork 的 C++（要重編 V8，共用的 artifact 跟著換），或改 elise。

## 理由和其他做法

- **加在 `blink-perf` 上，不另開一個指令**：選頁、server、hook、有效的判斷（兩邊的 document 摘要）都沿用。不選另一個指令（重複一份）。
- **留在 `main` 上，不放在不 merge 的分支或 patch**：分支要一直 rebase，patch 在 `blink_perf.py` 改了之後默默套不上（2026-11-10 的討論）。
- **elise 的事件，不從 `HeapStats` 讀背景的時間**：`HeapStats` 只有合計（`gc_bench` 用它），沒有時間點，切不出 runner 的區間，也和 Chrome 的 trace 不同一套。
- 不錄 `disabled-by-default-v8.gc`：見決定 4。

## 先例

- Chromium 的 `tools/perf/benchmarks/blink_perf.py`：blink_perf 的頁在 runner 的 `blink_perf` 區間裡讀 trace；Catapult 的 `blink_gc_metric`、`v8_gc_latency` 從 `CppGC.*` 和 V8 的 GC 事件算 GC 的時間和停頓。分類照它們的說法。
- Chrome 的 trace 有 `v8.memory` 這個 category（Perfetto UI 的 V8 記憶體 counter）；cppgc 在 worker 上發 `CppGC.ConcurrentMark`、`CppGC.ConcurrentSweep`。
- `bench.py gc`：同樣的 GC slice 的切法（`gc_bench.window_gc`）。

## 已決定的事項

2026-11-10（使用者確認）：

1. **寫成 RFC 做**，工具留在 `main` 上：`bench.py blink-perf --gc`。
2. **兩邊的 trace 都錄 `v8.memory`**（machopo 寫在 `bindings` 的 trace 的 category 裡，所有 `MACHOPO_TRACE` 都錄）。
3. **elise 的 worker 發 trace 事件**，另外一段做（第 2 段）。
4. **machopo 的 trace 不錄 `disabled-by-default-v8.gc`**：多的是 `EmbedderSpeed` 這類速度，machopo 的被空的 `CppHeap` 扭曲（約 2,000 bytes/ms，Chrome 約 1,500,000），讀了會誤導；GC 的 phase 已經在 `devtools.timeline` 裡錄到。
5. **最後一段是評估，不是優化**：重量、估每個候選的收益和成本，做不做到時候再決定。第 3 段照使用者的要求量 Speedometer（`speedometer --sample` 的 GC 那一類）。理由：Speedometer 上 GC 的差距（+10 ms）在 323 ms 的總差距裡是第二小的一類，GC 追平時比值約 1.48 → 1.42，文字、style、JS 各自追平時是 1.27、1.32、1.33（2026-11-08，`docs/performance-gaps.md`）；但 elise 在背景的 CPU 還量不到，要等第 2 段才知道總 CPU 是贏是輸。

第 1 段開始前（2026-11-10，使用者說開始，照看法）：

6. **報告的 V8 的排程四欄**：full GC 和 scavenge 的次數、marking 開始的原因、開始時用量是上限的幾倍、full GC 之後嵌入端的大小。
7. **頁面照平常的選法**（清單、路徑、`--pages`、`--dirs`），不另外加一份 GC 的清單；`docs/benchmarks.md` 建議 `bindings/gc-*`、`create-element`、`insert-before`、`parser/innerHTML-setter`。

第 2 段開始前（2026-11-10，使用者說繼續，照看法）：

8. **elise 的 worker 的 span 照 cppgc 命名，每次 run 一段**：`Gc.ConcurrentMark`、`Gc.ConcurrentSweep`、`Gc.ParallelScavenge`，加上做的時候看到的第四種 job，`Gc.Discard`（把空頁還給作業系統）。只在 worker 上發（joining 的 thread 跑的那一份在 GC 的 phase 裡）。不切到每一塊工作：不開 trace 時的成本最小，cppgc 也是每次 task 一段。

第 2 段做到一半（2026-11-10，使用者選甲）：

9. **不開 trace 時，elise 的每一段 phase 只付一次讀取和判斷，和 machopo 自己的 span 一樣**：`Platform::tracing` 交給 heap 一個 `&'static AtomicBool`（heap 建立時問一次，沒有 trace 的 platform 預設給永遠是 false 的），heap 和 job 在 `trace_begin` 之前先 relaxed 讀它；開始了的 phase 一定結束（`Phase`、`WorkerSpan` 記著自己有沒有開始），旗標中途變了也成對。machopo 的 `bindings::trace` 在開始錄時設自己的旗標（`recording()`），不改 fork 的 C++。照 cppgc 的 `TracingController::GetCategoryGroupEnabled`。原本每段是一次虛擬呼叫加進到 C++ 才判斷（幾十 ns，量不出來）：改的理由是一致，之後在比較細的地方加 span 不會把虛擬呼叫放上熱的路。不選只加回傳 bool 的查詢（乙：查詢本身還是虛擬呼叫）、維持現狀（丙）。旗標跟的是「有沒有在錄」，不是 `machopo` 這個 category：machopo 的 category 是固定的一組，兩者等價。

第 3 段開始前（2026-11-10，使用者照建議）：

10. **評估用的頁和指標**：blink-perf 的六頁（`insert-before` 可疑，只列出、不下結論）和 Speedometer 的 20 個 suite；指標是 main thread 上的 GC（V8 的和 DOM heap 的）、GC 的總 CPU（加上其他 thread）、最長的停頓和 1 ms 以上的個數、記憶體的高峰（嵌入端的大小）。Speedometer 用 `speedometer --sample` 的 GC 那一類，machopo 的配置函式另外列出來，和 Chrome 的歸類（cppgc 的配置 inline、算在 DOM）對齊。第 1、2 段先 merge 回 `main`。

評估之後（2026-11-10，使用者照建議）：

11. **不做優化的段，RFC 到評估為止**：回收那一側已經和 Chrome 持平或更好，剩下的差距大半是配置（屬於 DOM 的資料怎麼放）；全部的候選加起來在 Speedometer 上最多省 5 到 9 ms（整體 961.5 ms 的不到 1%），其中三項要改 fork 的 C++、重編 V8，文字、style、JS 各自的差距是 63 到 93 ms。候選留在「之後的可能」和 `docs/performance-gaps.md`。

## 待決定

無。

## 之後的可能

評估過、沒有做的（Speedometer 的數字是每個 iteration、20 個 suite 合計能省的估計）：

- **配置的快路、`HeapVector` 的 store**（2 到 4 ms；`Inner::allocate` 3.2、`alloc_at::<AttributeInit>` 2.0、`<CharacterDataInit>` 1.7）：改 elise 和 `dom`，和 DOM、bindings 的優化一起看（屬性的資料怎麼放）。
- **重設被丟掉的 wrapper 的 root 的路**（約 1.5 ms）：wrapper 的邊放在固定的位置、在 fork 的 C++ 裡比對和重設，不經過 Rust；要重編 V8。字串快取的 weak `v8::Global`（約 0.5 ms）。
- **照 Blink 告訴 V8 頁面正在載入**（`Isolate::SetIsLoading`；Speedometer 最多約 1 ms，短的 `gc-*` 頁 10 到 60 ms）：要先查 Chromium 什麼時候開始和結束 loading；fork 要接出它（C++，要重編 V8）；載入期間記憶體的高峰變高。不為效能做，要照 Blink 的行為時再做。
- **拿掉空的 `CppHeap` 的開銷**（Speedometer 量不到，blink-perf 4 到 65 ms）：要改 fork 的 C++。
- Speedometer 每個 suite 從 trace 拆 GC（`speedometer --trace` 照 `--gc` 的切法）。
