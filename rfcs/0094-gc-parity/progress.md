# RFC 0094 的進度：GC 和 Chrome 持平

開始日期：2026-11-10；完成日期：2026-11-10。Linux（Fedora 44，kernel 7.2.11），AMD Ryzen AI 9 HX 370，接電源；量測時兩個瀏覽器綁在 CPU 0-3、12-15。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `bench.py blink-perf --gc`，兩邊錄 `v8.memory`，報告的 V8 的排程 | 完成 | 2026-11-10 |
| 2 elise 的 worker 的事件，`--gc` 讀它們；不開 trace 時 elise 的 phase 只讀一個旗標 | 完成（elise `3bda47c`） | 2026-11-10 |
| 3 評估（結論：不做優化的段，決定 11） | 完成 | 2026-11-10 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `--gc`：runner 計時的區間、只算那個 process 的 GC、拿掉 `V8.GCHandleGCRequest`、DOM heap（最外層、切在區間上、`CppGC.*` 那一欄）、停頓、報告的格子；不能和 `--against` 一起用 | `python3 scripts/bench/blink_perf/blink_perf_test.py -k SpanGcTests`、`-k GcReportTests`、`-k CommandTests` |
| V8 的排程：區間裡的 full GC 和 scavenge、runner 的 thread 上的 marking 開始的原因和倍數、full GC 結束後第一次記的嵌入端的大小、只用 runner 的 thread 的 sequence 寫的 counter | `blink_perf_test.py -k test_v8s_schedule_in_the_span`、`-k test_a_trace_read_once` |
| Perfetto 的 counter（int、double、`static_name`、`name`）和 debug annotation（巢狀的 dict、陣列、字串、double、interned 的名稱） | `blink_perf_test.py -k test_a_traces_counters_and_annotations` |
| 兩邊的 trace 都有 `v8.memory` 的 counter，真的瀏覽器裡區間有 GC | `blink_perf_test.py -k GcRunTests`（dev build 和 Chrome for Testing） |
| `common.py` 抽出 `perfetto_track_packets` 之後，其他工具照舊 | `python3 scripts/bench/speedometer/speedometer_test.py`、`python3 scripts/bench/gc/gc_bench_test.py` |
| elise 的 worker 每跑一次 job 是 trace 的一段，在 worker 上、照 job 命名、有開始有結束，heap 的 thread 不發；span 包住 job 自己計的時間 | elise：`cargo test -p elise --test it parallel::a_workers_runs_are_spans_of_the_trace`、`concurrent::a_marking_finishes_at_the_safepoint_after_it_is_done`、`heap::a_worker_discards_the_pooled_pages` |
| `--gc` 的「其他 thread 上的 DOM heap」machopo 這一側有數字 | 下面的「量測和評估」 |
| 取樣的分類認得 elise 的 frame（GC、`elise::vector` 算給呼叫它的） | `python3 scripts/bench/speedometer/speedometer_test.py -k kind` |

## 做過的檢查

都在 2026-11-10、上面那台機器上。

### 測試和 mutation test

- 新的測試：`blink_perf_test.py` 的 `SpanGcTests`（區間、process、DOM heap、`V8.GCHandleGCRequest`、thread 的 track、counter 和 annotation、整份 trace 讀一次、V8 的排程）、`GcReportTests`、`GcRunTests`（fake 的 `gc/garbage.html`）、`CommandTests` 的 `--gc`。
- mutation test（`mutants-1.toml`，39 條）：39 條都抓到。先跑的 21 條（工具本身）裡原本沒抓到一條：DOM heap 的 slice 在另一個 DOM heap 的 slice 裡時也算（測試裡沒有巢狀的），補了 `Gc.Step` 裡的 `CppGC.IncrementalMark`。另外 counter 只用 runner 的 thread 的 sequence 這一條，原本只有真的瀏覽器碰得到（machopo 只有一個 process，拿掉也抓不到），補了合成的 trace（另一個 sequence 寫的同名 counter）。
- elise（`../third_party/elise`）：`platform.rs` 的 `in_worker_span` 包住四種 job 的 `run`（`Gc.ConcurrentMark`、`Gc.ConcurrentSweep`、`Gc.ParallelScavenge`、`Gc.Discard`），job 各自拿著 heap 的 platform；`docs/design.md` 補上。新的測試見驗收。
- 決定 9（不開 trace 時只讀一個旗標）：elise 的 `Platform::tracing`、`Phase`（原本的 14 處 `trace_begin`、`trace_end` 改成成對的 `Phase::begin`、`end`），`bindings::trace::recording()`，`V8GcPlatform` 和 `gc_bench` 的 platform 交出它。新的測試：elise 的 `heap::a_trace_that_doesnt_record_isnt_told_of_phases`、`a_trace_that_stops_in_a_phase_has_its_phases_ended`、`parallel::a_trace_that_doesnt_record_isnt_told_of_workers_runs`；`shell` 的 `trace::a_js_documents_cppgc_phases_are_traced` 多看 main thread 上有 `Gc.Mark`。
- mutation test（`mutants-2.toml`，16 條，含決定 9 的 7 條）：都抓到。第一版是每個 `run` 開頭一個 `let _span = WorkerSpan::begin(…)`，「寫成 `let _ =`、span 立刻結束」這一條在 sweep、scavenge、discard 上抓不到（測試只看有開始有結束）；改成 `in_worker_span(…, || { … })` 包住整個 run，那種錯寫不出來，剩下 `in_worker_span` 自己的一處由 marking 的測試守住（span 的時間不短於 job 自己計的 `background_mark_ns`）。

### 試過、改掉的，和找到的 bug

- 試過、改掉的：
  - 照時間重新套疊同一個 thread 的 slice（以為 machopo 的 V8 事件和 elise 的 span 在不同的 track）：其實在同一個 track 上；Chrome 的時間只到 µs，重排之後同一刻開始和結束的 slice 配錯，`gc-tree` 的最長停頓變成 1311 ms。改成照 trace 的順序配對，只拿掉 `V8.GCHandleGCRequest`。
  - full GC 之後嵌入端的大小，原本取 GC 裡最後一次記的：machopo 0.25 MiB、Chrome 0.99 MiB，是嵌入端的大小設好之前記的；改成 GC 結束後第一次記的（7.50、7.41 MiB，和暫時的量測一樣）。
- **找到的 bug**：`scripts/bench/speedometer/categories.py` 還照舊的 crate 名字 `gc` 分類，改名成 elise 之後它的樣本都掉進「other」：第一次算出來 GC 是 16.0 對 16.1 ms（「持平」），是錯的。改成 `elise`（`speedometer_test.py` 的三個斷言跟著改），用同一批樣本重算。

### 不穩定的和本來就失敗的測試

- `main` 上本來就失敗的：`speedometer_test.py` 的 `test_the_windows_are_the_runners_times`（dev build 的 machopo 的區間比 runner 報的時間長約 0.4 ms，容許 0.15 ms），用 `main` 的 `common.py` 也一樣，3 次都失敗；和這份的改動無關，沒有查原因。
- 不穩定的測試：`cargo test --workspace` 第一次跑時 `shell` 的 `devtools_network::requests_are_told_with_their_responses_and_bodies` 失敗（`loadingFinished` 的 timestamp 比 `requestWillBeSent` 的早），第二次整套通過，單獨跑 40 次都通過，只在整個 workspace 平行跑、機器忙的時候出現。查到的原因（沒有重現確認）：`fetch()` 的請求在 load sink 裡就送出去了，renderer 之後才在 `fetch_sent` 裡 `will_send`，`requestWillBeSent` 的 timestamp 是那時的 `TimeTicks::now()`；網路先收完時，`loadingFinished` 的 `response_end` 比它早。失敗的兩個 path（`/data.json`、`/shared`）都是 `fetch()`。Chrome 的 `requestWillBeSent` 用請求開始的時間。和這份的改動無關（`main` 上的程式），沒有修。

### 量測和評估

- **第 1 段的量測**（照 `docs/benchmarks.md`：`python3 scripts/bench.py blink-perf --gc --runs 3 --pages bindings/gc-forest.html,bindings/gc-mini-tree.html,bindings/gc-tree.html,bindings/create-element.html,bindings/insert-before.html,parser/innerHTML-setter.html`；machopo 是這一段的 `perf` build，Chrome for Testing 152.0.7977.82；machopo / Chrome，ms，3 次的中位數）：

  | 頁 | GC | DOM heap | V8 | 其他 thread | full GC 次數 | marking 開始時，上限的幾倍 | full GC 之後的嵌入端（MiB） |
  | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
  | `gc-forest` | 246 / 320 | 8.3 / 101 | 238 / 219 | 362 / 529 | 62 / 24 | 1.02 / 3.30 | 7.5 / 7.41 |
  | `gc-mini-tree` | 243 / 324 | 9.2 / 123 | 234 / 200 | 338 / 646 | 73 / 46 | 1.02 / 3.18 | 7.5 / 7.31 |
  | `gc-tree` | 264 / 333 | 3.0 / 128 | 261 / 205 | 611 / 743 | 10 / 6 | 1.00 / 1.50 | 29.8 / 73.4 |
  | `create-element` | 1192 / 2957 | 55 / 1268 | 1137 / 1689 | 1423 / 5153 | 484 / 508 | 1.01 / 1.00 | 7.5 / 7.4 |
  | `insert-before` | 3.6 / 803 | 0 / 689 | 3.6 / 114 | 0.4 / 3363 | 0 / 90 | – / 1.00 | – / 32.6 |
  | `innerHTML-setter` | 432 / 2966 | 117 / 2499 | 315 / 468 | 1323 / 6818 | 1015 / 658 | 1.01 / 1.01 | 7.51 / 7.43 |

  - 和 2026-11-10 用暫時的工具量的（`details.md`）差不多。
  - **Chrome 只在短的頁上處於 loading 狀態**：三個 `gc-*` 頁的計時區間 1.4 到 2.7 秒，marking 開始時用量是上限的 1.5 到 3.3 倍；`create-element`、`innerHTML-setter` 約 16 秒，超過 V8 的 loading 狀態最多的 7 秒，倍數和 machopo 一樣約 1.00，full GC 的次數也差不多（`create-element` 484 / 508；innerHTML 反而是 machopo 多，1015 / 658）。所以 full GC 比較多次在 blink-perf 上主要是短的頁的現象。
  - `insert-before` 照舊不算數（machopo 快 3 倍，可疑）；`gc-tree` 的「full GC 之後」照報告的說明比不了。
- **第 2 段的量測**（elise 的 worker 的事件之後；和第 1 段同樣的指令；machopo / Chrome，ms，3 次的中位數）：

  | 頁 | main thread 的 DOM heap | 其他 thread 的 DOM heap | DOM heap 合計 |
  | --- | ---: | ---: | ---: |
  | `gc-forest` | 8.1 / 102 | 28.5 / 237 | 37 / 339 |
  | `gc-mini-tree` | 8.1 / 123 | 30.7 / 388 | 39 / 511 |
  | `gc-tree` | 8.1 / 112 | 165 / 372 | 173 / 484 |
  | `create-element` | 54 / 1276 | 51 / 3061 | 105 / 4337 |
  | `innerHTML-setter` | 120 / 2472 | 433 / 5938 | 553 / 8410 |

  - **DOM heap 的總 CPU（main 加背景）elise 也比 Oilpan 少**：是 Oilpan 的 2% 到 36%（`gc-tree` 全部活著、要標的最多，差距最小）。第 1 段只能說 main thread 上贏，這一段補上背景的。
  - main thread 的數字和第 1 段一樣（span 只加在 worker 上）。`insert-before` 照舊不算數。
- **第 3 段的量測**（machopo `d69a098`；照 `docs/performance-gaps.md` 的「怎麼重做」）：`python3 scripts/bench.py speedometer`、`speedometer --sample --iterations 5`、`speedometer --sample --browser chrome --iterations 5`（Fedora 的 Chromium 156.0.8078.79 和它的 debuginfo）；blink-perf 的六頁用第 2 段的量測。
- **Speedometer 的 GC 那一類**（每個 iteration，20 個 suite 合計）：machopo 27.8 ms、Chrome 16.1 ms，差 11.8 ms；整體 961.5 對 667.7 ms，比值的幾何平均 1.384，只有 GC 追平時 1.373。其他類的差：文字 +92.7、style +78.2、JS +63.4、DOM +38.9、bindings +27.0、box layout +26.8、paint +13.8。拆開（ms）：

  | | machopo | Chrome |
  | --- | ---: | ---: |
  | DOM heap 的配置（elise 的 `alloc_at`、`Inner::allocate`、`publish`） | 9.7 | 算在 DOM（cppgc 的配置 inline） |
  | DOM heap 的其他（elise 的 sweep、barrier、safepoint；`bindings` 的 wrap；Chrome 是 cppgc 和 Blink 的 GC） | 3.0 | 5.0 |
  | V8 的 scavenge、traced 和 global handle | 9.4 | 5.9 |
  | V8 的 full GC | 2.6 | 1.4 |
  | V8 的其他 | 3.0 | 3.7 |
  | 合計 | 27.8 | 16.1 |

  - **回收那一側（不算配置）是 18.1 對 16.1 ms**：DOM heap 自己少 2.0，V8 的 scavenge 和 handle 多 3.5，full GC 多 1.2。
  - 配置的 9.7 ms 裡：`Inner::allocate`（`HeapVector` 的 store）3.2、`alloc_at::<AttributeInit>` 2.0、`<CharacterDataInit>` 1.7，其餘各不到 1。
- **blink-perf 的六頁**（第 2 段的量測）：main thread 上的 GC 和 DOM heap 的總 CPU 都比 Chrome 少；V8 那一側在三個 `gc-*` 頁多 19 到 77 ms（full GC 的次數，Chrome 在 loading 狀態），超過 7 秒的頁上次數差不多；最長的停頓 machopo 不比 Chrome 長（`gc-tree` 27.4 對 22.1 ms 那一次除外，三次裡最長的）；記憶體的高峰 machopo 比較低（`gc-forest` 的嵌入端 167 對 302 MiB，2026-11-10 的暫時量測）。
- **各個候選**（決定 5 用）：

  | 候選 | Speedometer 的 GC 那一類 | blink-perf | 成本 | 看法 |
  | --- | --- | --- | --- | --- |
  | 告訴 V8 頁面正在載入 | 最多約 1 ms（full GC 一共差 1.2） | 短的 `gc-*` 頁約 −10 到 −60 ms（整個區間），長的頁沒有 | fork 的 C++、重編 V8；載入期間記憶體的高峰變高 | 不為效能做；要照 Blink 的行為時再做 |
  | 拿掉空的 `CppHeap` 的開銷 | 量不到（`CppGC.*` 0.1 ms） | −4 到 −65 ms | fork 的 C++、重編 V8 | 不做 |
  | 配置的快路、`HeapVector` 的 store | 估 2 到 4 ms（9.7 裡的一部分；`Inner::allocate` 3.2） | `create-element` 這類 | elise 和 `dom`，不動 V8 | 最值得的一項，但屬於 DOM 的資料怎麼放，和 DOM、bindings 的優化一起看 |
  | V8 的 scavenge、traced handle 多 4.1 ms | 約 2 ms（重設 root 的路 1.5、字串快取的 weak global 0.5） | | 重設 root 要改 fork 的 C++、重編 V8 | 查過了（下面）：量太小，不做 |
  | `create-element` 沒有 scavenge | | 沒有可省的 | | 查過了（下面）：不是問題 |
- **查了「先查」的兩項**（同一批樣本和 trace）：
  - **V8 的 scavenge 和 handle 多的 3.5 ms（實際算出 10.0 對 5.9 ms）**：兩邊年輕的 traced handle 差不多（`ComputeWeaknessForYoungObjects` 1.11 對 1.02 ms），差在三處。
    1. 重設被丟掉的 wrapper 的 root（`TracedHandles::ProcessWeakYoungObjects` 連同它呼叫的，2.35 對 0.76 ms）：machopo 每個 wrapper 走 fork 的 `RustRootsHandler::ResetRoot`（0.29）→ Rust 的 `reset_root`（0.63：`on_thread` 的斷言、`elise::js_edges_of` 查型別的 JS 邊）→ 再回 C++ 的 `v8__machopo__TracedReference__ResetIfSame`（0.47）；Blink 的 `BlinkRootsHandler::ResetRoot` 是 0.30（`ScriptWrappable` 的 wrapper 在固定的位置）。
    2. 字串快取的 weak `v8::Global`（`dom_js` 的 `StringCache::make`，建和放掉約 0.5 ms；V8 的 `GlobalHandles::Create`、`Release` 合計 0.88 對 0.16 ms）。
    3. scavenge 本身搬物件（`Scavenger::ScavengeObject`、`Process`、in-place internalizable string，約多 1 ms）：沒有再往下查。
    - 能省的：第 1 項最多約 1.5 ms（wrapper 的邊放在固定位置、在 C++ 裡比對和重設，要改 fork 的 C++、重編 V8；只改 Rust 那一側省不到一半），第 2 項約 0.5 ms。合計約 2 ms，整體 961.5 ms 的 0.2%。
  - **`create-element` 上 machopo 沒有 scavenge**：不是問題。兩邊 full GC 的間隔差不多（中位數 32.0 對 29.2 ms，整份 trace 518 對 533 次），machopo 每次 full GC 之前 V8 的 heap 用得比較少（中位數 6.71 對 8.97 MiB），新生代還沒滿、full GC 就先把它清掉了；machopo 的 30 次 scavenge 都在計時的區間之前。Chrome 在兩次 full GC 之間另外 scavenge（204 次，265 ms）。machopo 這一頁 V8 那一側的 GC 因此比較少（1137 對 1689 ms）。Chrome 為什麼在同樣的間隔裡配置比較多 JS 的記憶體（wrapper 比較大，或新生代比較小）沒有查。
- **結論**：GC 的回收已經和 Chrome 持平或更好（DOM heap 的總 CPU 是 Oilpan 的 2% 到 36%，Speedometer 上回收那一側差 2 ms）；剩下的 11.8 ms 大半是配置，GC 全部追平對整體只有 1.384 → 1.373。建議這份 RFC 到評估為止，不加優化的段：配置併進之後 DOM 和 bindings 的優化；重設 root 的路和字串快取的 weak global 合計約 2 ms，記在 `docs/performance-gaps.md` 的 GC 那一節，之後動到 fork 的那一段時順便做。
- **blink-perf 的 26 頁**（`docs/performance-gaps.md` 的那些頁，`bench.py blink-perf --pages …`，兩邊各一次）：26 頁都有效，不算可疑的 22 頁 machopo 慢 1.46 倍（幾何平均）；各頁的倍數寫進 `docs/performance-gaps.md`，它照這次的結果整份改寫（整體、各類、每個 suite、最多的函式、新的「GC」一節、方向）。

### 跑過的檢查

- 跑過的檢查：`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；`blink_perf_test.py`、`gc_bench_test.py`、`speedometer_test.py`（上面那一個以外）；第 1 段做完的 `cargo test --workspace`（第二次全部通過，第一次見上一條）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`。
- machopo 的檢查（第 2 段做完）：`cargo test --workspace`、`cargo fmt --check`、clippy、`sanity.py` 八項、`blink_perf_test.py`、`gc_bench_test.py`。trace 的旗標（`bindings` 的 `RECORDING`）是整個 process 一個的 static，登記進 `sanity.py boundary` 的允許清單：trace 是 process 的，和 V8 的 platform 一樣。改成旗標之後沒有重量六頁（main thread 上只是虛擬呼叫換成讀旗標）。
- elise 的檢查：`cargo test --workspace`、`sanity.py tests`、`numerics`、`cargo fmt --check`、clippy、`test.py models`（loom、shuttle）都過。fuzz、verus、miri 沒有跑（沒有動到它們守的東西：只加了 trace 的呼叫）。
- 收尾（第 3 段只改了 Python 和文件）：`speedometer_test.py`（`main` 上本來就失敗的那一個以外）、`sanity.py docs`。
