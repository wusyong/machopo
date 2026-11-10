# RFC 0094 的進度：GC 和 Chrome 持平

開始日期：2026-11-10

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `bench.py blink-perf --gc`，兩邊錄 `v8.memory`，報告的 V8 的排程 | 完成 | 2026-11-10 |
| 2 elise 的 worker 的事件，`--gc` 讀它們 | | |
| 3 評估 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `--gc`：runner 計時的區間、只算那個 process 的 GC、拿掉 `V8.GCHandleGCRequest`、DOM heap（最外層、切在區間上、`CppGC.*` 那一欄）、停頓、報告的格子；不能和 `--against` 一起用 | `python3 scripts/bench/blink_perf/blink_perf_test.py -k SpanGcTests`、`-k GcReportTests`、`-k CommandTests` |
| V8 的排程：區間裡的 full GC 和 scavenge、runner 的 thread 上的 marking 開始的原因和倍數、full GC 結束後第一次記的嵌入端的大小、只用 runner 的 thread 的 sequence 寫的 counter | `blink_perf_test.py -k test_v8s_schedule_in_the_span`、`-k test_a_trace_read_once` |
| Perfetto 的 counter（int、double、`static_name`、`name`）和 debug annotation（巢狀的 dict、陣列、字串、double、interned 的名稱） | `blink_perf_test.py -k test_a_traces_counters_and_annotations` |
| 兩邊的 trace 都有 `v8.memory` 的 counter，真的瀏覽器裡區間有 GC | `blink_perf_test.py -k GcRunTests`（dev build 和 Chrome for Testing） |
| `common.py` 抽出 `perfetto_track_packets` 之後，其他工具照舊 | `python3 scripts/bench/speedometer/speedometer_test.py`、`python3 scripts/bench/gc/gc_bench_test.py` |

## 做過的檢查

**第 1 段**（2026-11-10，Linux，AMD Ryzen AI 9 HX 370）：

- 新的測試：`blink_perf_test.py` 的 `SpanGcTests`（區間、process、DOM heap、`V8.GCHandleGCRequest`、thread 的 track、counter 和 annotation、整份 trace 讀一次、V8 的排程）、`GcReportTests`、`GcRunTests`（fake 的 `gc/garbage.html`）、`CommandTests` 的 `--gc`。
- mutation test（`mutants-1.toml`，39 條）：39 條都抓到。先跑的 21 條（工具本身）裡原本沒抓到一條：DOM heap 的 slice 在另一個 DOM heap 的 slice 裡時也算（測試裡沒有巢狀的），補了 `Gc.Step` 裡的 `CppGC.IncrementalMark`。另外 counter 只用 runner 的 thread 的 sequence 這一條，原本只有真的瀏覽器碰得到（machopo 只有一個 process，拿掉也抓不到），補了合成的 trace（另一個 sequence 寫的同名 counter）。
- 試過、改掉的：
  - 照時間重新套疊同一個 thread 的 slice（以為 machopo 的 V8 事件和 elise 的 span 在不同的 track）：其實在同一個 track 上；Chrome 的時間只到 µs，重排之後同一刻開始和結束的 slice 配錯，`gc-tree` 的最長停頓變成 1311 ms。改成照 trace 的順序配對，只拿掉 `V8.GCHandleGCRequest`。
  - full GC 之後嵌入端的大小，原本取 GC 裡最後一次記的：machopo 0.25 MiB、Chrome 0.99 MiB，是嵌入端的大小設好之前記的；改成 GC 結束後第一次記的（7.50、7.41 MiB，和暫時的量測一樣）。
- `main` 上本來就失敗的：`speedometer_test.py` 的 `test_the_windows_are_the_runners_times`（dev build 的 machopo 的區間比 runner 報的時間長約 0.4 ms，容許 0.15 ms），用 `main` 的 `common.py` 也一樣，3 次都失敗；和這份的改動無關，沒有查原因。
- 不穩定的測試：`cargo test --workspace` 第一次跑時 `shell` 的 `devtools_network::requests_are_told_with_their_responses_and_bodies` 失敗（`loadingFinished` 的 timestamp 比 `requestWillBeSent` 的早），第二次整套通過，單獨跑 40 次都通過，只在整個 workspace 平行跑、機器忙的時候出現。查到的原因（沒有重現確認）：`fetch()` 的請求在 load sink 裡就送出去了，renderer 之後才在 `fetch_sent` 裡 `will_send`，`requestWillBeSent` 的 timestamp 是那時的 `TimeTicks::now()`；網路先收完時，`loadingFinished` 的 `response_end` 比它早。失敗的兩個 path（`/data.json`、`/shared`）都是 `fetch()`。Chrome 的 `requestWillBeSent` 用請求開始的時間。和這份的改動無關（`main` 上的程式），沒有修。
- **量測**（照 `docs/benchmarks.md`：`python3 scripts/bench.py blink-perf --gc --runs 3 --pages bindings/gc-forest.html,bindings/gc-mini-tree.html,bindings/gc-tree.html,bindings/create-element.html,bindings/insert-before.html,parser/innerHTML-setter.html`；machopo 是這一段的 `perf` build，Chrome for Testing 152.0.7977.82；machopo / Chrome，ms，3 次的中位數）：

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
- 跑過的檢查：`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；`blink_perf_test.py`、`gc_bench_test.py`、`speedometer_test.py`（上面那一個以外）；第 1 段做完的 `cargo test --workspace`（第二次全部通過，第一次見上一條）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`。
