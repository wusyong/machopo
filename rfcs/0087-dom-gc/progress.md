# RFC 0087 的進度：kun 自己的 GC：為 DOM 設計的分代、concurrent GC

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-24，Windows 11（x86_64）。在 `0087` 上做。

完成日期：2026-10-26，macOS 26.6.2（Apple M4）。第 0 段的 0a 到 0g 在 Windows 11（Intel Raptor Lake）上做和量，第 0 段的驗收和之後的都在這台 Mac 上。

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。做的時候的「待決定」，做完時分到 `docs/limitations.md` 的「GC 和 `dom`」和 `README.md` 的「之後的可能」（決定 40 第 4 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0a cppgc 的 pause：trace 錄 `cppgc` 類別（JS 文件和 wasm 文件的 heap，Chrome 那一側也錄），Speedometer 的階段報告算進 GC | 完成 | 2026-10-24 |
| 0b rusty_v8 的 fork 接出 cppgc 的 `CollectStatistics`（重編 V8） | 完成（fork 的 commit 已經 push；收尾時發 `kun-v152.2.0-21`（2026-10-26 push 了 tag，GitHub 上編好之後 kun 才換過去、拿掉 `rev`，在 merge 之前），決定 40 第 2 條） | 2026-10-24 |
| 0c 測試用的物件模型的 cppgc 版和 microbenchmark、`bench.py gc` | 完成 | 2026-10-25 |
| 0d 和 V8 的 GC 比的 microbenchmark（N 個邊的一般物件、同樣的配置模式） | 完成 | 2026-10-25 |
| 0e 錄製：`dom` 的記錄掛點和格式 | 完成（fork 的 GC 計數，同 0b） | 2026-10-25 |
| 0f 重播：在 cppgc 上重播錄下的工作 | 完成 | 2026-10-25 |
| 0g 基準：真的 DOM 的工作（C 的 guest、blink-perf 的 DOM 頁）的數字和錄製，Windows 和 Apple Silicon | 完成（fork 的 Perfetto 共用記憶體，同 0b） | 2026-10-25 |
| 1a TLA+ 模型：SATB 和 Dijkstra 各一份，模型的 mutation | 完成 | 2026-10-25 |
| 1b 錄製上的 barrier 次數：`gc_bench analyze` 模擬一輪 marking | 完成 | 2026-10-25 |
| 1c safe 的 API 原型在 Miri 下 | 完成 | 2026-10-25 |
| 1d 接 V8 的 stub 原型：同步和交換兩種 | 完成（要不要 fork V8 記在 `README.md` 的「之後的可能」，第 9 段接手） | 2026-10-25 |
| 2a `crates/gc` 和 `gc_macros`：API 和 derive、cage、單 thread 的 mark-sweep | 完成 | 2026-10-25 |
| 2b page、size class、large object、sweep 之後的空區段 | 完成 | 2026-10-25 |
| 2c 平行 marking（worklist、worker）、shuttle 和 loom | 完成 | 2026-10-25 |
| 2d heap verifier、fuzz（錄製當種子）、Verus 和 Kani 的試用 | 完成 | 2026-10-25 |
| 2e `gc_bench` 的新 heap：microbenchmark 和錄製重播，和 cppgc 比 | 完成 | 2026-10-26 |
| 3a nursery、單 thread 的 scavenge、generational barrier 和 slot set、young 的 finalizer 清單 | 完成（整頁升上去沒有做） | 2026-10-26 |
| 3b 平行的 scavenge、shuttle 和 loom | 完成 | 2026-10-26 |
| 3c nursery 的大小（照 V8）、pretenuring 的 API | 完成 | 2026-10-26 |
| 3d 重播照安全點分操作、和 cppgc 及 V8 比 | 完成（full GC 的最小門檻改成 2 MiB，決定 26） | 2026-10-26 |
| 3e 照配置的地方 pretenure，第 3 段的驗收 | 完成（決定 27） | 2026-10-26 |
| 4a concurrent marking 的核心：SATB、black allocation、atomic 的 `Member`、marking 期間的 scavenge、收尾 | 完成（刪 root 的 barrier 是多餘的，保留，決定 29 第 10 條） | 2026-10-26 |
| 4b weak：`WeakMember`、`WeakPersistent`、weak 讀取的 barrier | 完成（決定 30） | 2026-10-26 |
| 4c ephemeron：`EphemeronPair` | 完成（決定 31） | 2026-10-26 |
| 4d GC 管的 id 表 | 完成（決定 32） | 2026-10-26 |
| 4e 量測和第 4 段的驗收 | 完成（加了 marking 期間的 overshoot margin，決定 33、34） | 2026-10-26 |
| 5a 空頁的記憶體交給 worker 還給作業系統 | 完成 | 2026-10-26 |
| 5b concurrent sweeping 的核心 | 完成（決定 36） | 2026-10-26 |
| 5c 三種 finalizer | 完成（決定 37） | 2026-10-26 |
| 5d 量測和第 5 段的驗收 | 完成（`used` 和門檻改成 sweep 做完才定，決定 36 第 4 條的補充） | 2026-10-26 |
| 6a 量碎片：old 的頁上的空格子、照 V8 的挑法能放掉的頁、配置的慢路徑 | 完成（不做 compaction，決定 38、39） | 2026-10-26 |

## 驗收

效能的數字都是 Apple M4（macOS 26.6.2，接電源）上 `perf` profile 量的，存在這個資料夾的 `bench-*-apple-m4.json`；之後用 `bench.py gc --heaps gc,gc-parallel --baseline <json>` 比。量法見 `docs/benchmarks.md`。

**量測的基礎和基準**

| 項目 | 怎麼驗證 |
| --- | --- |
| cppgc 的停頓（`CppGC.*`）在 JS 文件和 wasm 文件的 trace 的 main thread 上；Speedometer 的階段報告兩邊都把它算進 GC | `cargo test -p shell --test it trace::a_js_documents_cppgc_phases_are_traced trace::a_wasm_documents_cppgc_phases_are_traced`；`python3 scripts/bench/speedometer/speedometer_test.py -k test_each_browser_names_its_phases` |
| cppgc 的 heap 大小在三種 heap 上：元素讓 `used` 長、拿掉並 GC 之後降回去，只剩 skeleton 時 `used` 不到 `committed` 的四分之一 | `cargo test -p dom --test it heap_statistics::`（要 fork 的 `CollectStatistics`） |
| 物件模型（`dom` 的 `bench`）的連結和 GC；四個 DOM 的工作、六個配置模式在 standalone、isolate、V8 的 heap 上，三種留下的物件數一樣；`bench.py gc` 讀 trace、V8 的 `--trace-gc-nvp`、百分位數 | `cargo test -p dom --test it bench::`；`cargo test -p gc_bench --test it workloads::`；`python3 scripts/bench/gc/gc_bench_test.py` |
| 錄製：配置、寫邊（offset、型別、邊數）、root、wrapper、安全點、turn、各種 GC 照順序錄下；讀檔拒絕讀不了的；`gc_bench inspect` | `cargo test -p dom --test it recording::`；`cargo test -p gc_bench --test it recording::` |
| 重播：邊和 root 撐住的就是它們撐住的，跳過的都計數，物件拿到最後一次用到；`analyze` 的 minor、full GC、barrier 和 floating garbage 照手算的錄製 | `cargo test -p gc_bench --test it replay:: analyze::` |
| blink-perf 的六頁的 wasm 版（C 的 guest）和 JS 版：量測的區間、區間裡的 GC、wasm 版有效的條件、留最大的錄製；trace 掉了事件時停下 | `python3 scripts/bench/blink_perf/blink_perf_test.py`；`gc_bench_test.py` |
| 第 0 段的基準在 Apple Silicon 上（microbenchmark、blink-perf 的六頁、錄製的重播和存活率），和 Windows 的對照 | `bench.py gc`、`bench.py blink-perf --gc`、`bench.py gc --replay target/bench/gc/recordings/*-wasm.kungc`、`bench.py gc --heaps isolate --replay target/bench/gc/recordings/*-js.kungc`、`gc_bench analyze`（`RUSTY_V8_ARCHIVE` 連結本機編的 V8）。gc-tree 的 wasm 錄製在 isolate 上的重播在 Mac 上量不到（trace 掉了 worker 的事件），其他的都有數字（「做過的檢查」的「基準」） |

**協定和原型**

| 項目 | 怎麼驗證 |
| --- | --- |
| 兩種 barrier 的協定在 TLA+ 的模型上不漏標，拿掉其中任何一部分 TLC 找得到反例（Dijkstra 的 black allocation 除外：它的安全性不靠它） | `python3 rfcs/0087-dom-gc/tla/check.py`（要 Java） |
| `gc_bench analyze` 在錄製上模擬 concurrent marking，數兩種 barrier 的 slow path、收尾的工作和 floating garbage | `cargo test -p gc_bench --test it analyze::`；`target/perf/gc_bench analyze <錄製>` |
| safe 的 API 在會搬動物件的 heap 上 sound（Miri 的 Stacked 和 Tree Borrows、strict provenance），擋住不安全的寫法（compile-fail） | 原型在 `de8ea619` 的 `rfcs/0087-dom-gc/prototype/`（收尾時刪掉）：在那裡 `cargo test`、`cargo miri test -p gc --test model`（見它的 README） |
| 兩種對齊的做法在 V8 的 major GC 裡收掉跨兩邊的循環、留住 JS 留著的和 strong root 的、V8 marking 期間被搬動的節點的 listener 不掉；一百萬個節點的停頓 | 原型在 `de8ea619` 的 `rfcs/0087-dom-gc/prototype/`（收尾時刪掉）：在那裡 `python3 v8/build.py`、`cargo test`、`cargo run --release -p v8_bridge --example pause [-- --unified]`（見它的 README） |

**heap 的核心**

| 項目 | 怎麼驗證 |
| --- | --- |
| `gc` 的 heap（cage、size class 的頁、安全點的 mark-sweep）上 safe 的 API sound，擋住不安全的寫法（compile-fail） | `cargo test -p gc -p gc_macros`；Miri 的指令在 `crates/gc/README.md` 的「測試」 |
| size class 到 32 KiB 和 large object、sweep 只讀 bitmap、空區段的 bump、先用最滿的頁、空頁的 pool 和 discard | `cargo test -p gc --test it heap::` |
| 平行 marking 和只在一個 thread 上標的留下、釋放的一樣，worklist 的結束判斷在每種 interleaving 都對、不 deadlock | `cargo test -p gc --test it parallel::`；`python3 scripts/test.py gc-models`（loom、shuttle） |
| 每次 GC 之後 heap verifier 都滿意；fuzz 對著參考模型、從錄製的種子跑沒有失敗；頁的 bitmap 搜尋由 Verus 證明 | `cargo test -p gc`；`cargo test -p gc_bench --test it seeds::`；`python3 scripts/test.py gc-fuzz`、`gc-verus` |
| 新 heap 在 `gc_bench` 上（`--heap gc`、`gc-parallel`）和 cppgc 比：單 thread 的 marking 每秒的物件數是 cppgc 的 1.07 到 1.68 倍（build、三個 high），peak used 0.50 到 0.93 倍，wasm 錄製的重播 0.52 到 0.56 倍 | `cargo test -p gc_bench`；`bench.py gc --heaps standalone,gc,gc-parallel`；`bench-2e-apple-m4.json` |

**young generation**

| 項目 | 怎麼驗證 |
| --- | --- |
| scavenge 只 copy 活的、活過兩次才升；write barrier 和 slot set 留住 old 指向的年輕物件（`Member::set`、init、`HeapVector`）；nursery 的 finalizer；verifier 在 scavenge 之後檢查 | `cargo test -p gc --test it young::`；`gc-fuzz` |
| 平行的 scavenge：三個 thread 搶一個 header 只有一份 copy 贏；worker 處理 slot set 的頁和 root 時留下、搬到哪裡、finalize 的都對 | `test.py gc-models`（loom、shuttle） |
| nursery 照留下的長大、到上限停住，full GC 前減半；`pretenured` 的配置放 old | `cargo test -p gc --test it young::` |
| 配置的地方（`Site`）照 V8 的規則 tenure、full GC 時撤回，site 經過 copy 和升級還在；重播照錄到的安全點分操作 | `cargo test -p gc --test it sites::`；`cargo test -p gc_bench --test it replay::` |
| 第 3 段：決定 23 第 7 條（決定 26 第 3、4 條改過）的每一條在 Mac 上同一次執行都過：全部活著的 minor GC p99 1.6 到 2.9 ms（V8 2.6 到 8.7），gc-tree 的重播 958 ms（cppgc 1893），high 的模式是 V8 的 0.65 到 0.84 倍，low 的是 0.76 到 1.24 倍 | `bench.py gc --runs 5`、`bench.py gc --runs 5 --heaps standalone,gc,gc-parallel --replay target/bench/gc/recordings/*-wasm.kungc`；`bench-3-apple-m4.json` |

**concurrent marking、weak、ephemeron、id 表**

| 項目 | 怎麼驗證 |
| --- | --- |
| old 的 marking 和 mutator 同時跑：收尾時到得了的都標過（TLA+ 窮舉；拿掉 barrier、black allocation、升上來的算黑、開始前的 scavenge 各找得到反例），發布的順序和取消在 loom 的每種 interleaving 都對，shuttle 隨機排程 marking 和 mutator，fuzz 在操作之間做一段 marking，verifier 每次收尾檢查 floating garbage | `python3 rfcs/0087-dom-gc/tla/check.py --spec SatbYoung`；`cargo test -p gc --test it concurrent::`；`test.py gc-models`、`gc-fuzz` |
| weak 的邊和 root：full GC 清掉指向死掉的 old 物件的、scavenge 清掉指向死掉的年輕物件的，活的跟著搬；marking 期間 weak 讀出的物件被標上；TLA+ 上拿掉讀取的 barrier、收尾或 scavenge 的清除各找得到反例 | `cargo test -p gc --test it weak:: fuzz::`；`check.py --spec SatbYoung`；`gc-models`、`gc-fuzz` |
| ephemeron：value 只在 key 活著時留下（value 指回 key 也收得掉），鏈照任何順序都標完，key 死了一起清掉；scavenge 把 value 當 strong、key 當 weak；TLA+ 上拿掉固定點或寫 value 的 SATB 各找得到反例 | `cargo test -p gc --test it ephemeron:: fuzz::`；`check.py --spec SatbYoung`；`gc-fuzz` |
| id 表：id 找得到活的物件、物件死了之後永遠找不到，full GC 放掉死光的塊，scavenge 搬動和清掉，查詢比對型別、marking 期間標上找到的 | `cargo test -p gc --test it ids:: fuzz::`；`gc-fuzz` |
| marking 期間 old 可以超過門檻 `max(門檻/2, min_overshoot_bytes)` 才在停頓裡收尾 | `cargo test -p gc --test it concurrent::a_marking_runs_past_the_limit_by_the_margin concurrent::at_the_limit_the_pause_finishes_the_marking` |
| 第 4 段：收尾的標記 p99 最多 0.38 ms（ids：走一百萬格的 id 表），其他 14 µs 以內（加 margin 之前 graph-high 5.3 ms、gc-tree 的重播 4.3 ms）；high 的模式是 V8 的 0.19 到 0.47 倍；其他各條照第 3 段 | 同第 3 段的指令，`ids` 只在 gc、gc-parallel 上；`bench-4-apple-m4.json` |

**sweeping 和 finalizer**

| 項目 | 怎麼驗證 |
| --- | --- |
| 空頁由 worker 還給作業系統，做完的在安全點才拿回來；沒有 worker 的 platform 在停頓裡還 | `cargo test -p gc --test it heap::a_worker_discards_the_pooled_pages heap::the_discarded_pages_come_back_at_any_safepoint` |
| concurrent sweeping：sweeper 只讀 mark bit，heap 的 thread 在安全點和配置的慢路徑套用、沒有掃好的頁就自己掃一頁；下一輪 marking 和 `collect` 前掃完；sweep 期間的 scavenge 不升到還沒掃的頁；TLA+ 的 `SlotsLive` | `cargo test -p gc --test it sweep::`；`test.py gc-models`；`check.py --spec SatbYoung` |
| 三種 finalizer：`External<T: Send>` 在 sweep 的 worker 上、`LocalExternal<T>` 在 heap 的 thread 上、nursery 的在 scavenge 裡；`Drop` panic 往上拋、那一頁之後重掃；`External` 放不是 `Send` 的值編不過 | `cargo test -p gc --test it sweep::`；`cargo test -p gc --test compile_fail`（`external_needs_send`）；`gc-models`、`gc-fuzz` |
| `used` 和門檻在 sweep 做完時由 sweep 找到的活的定 | `cargo test -p gc --test it sweep::the_heap_counts_what_the_sweep_finds_live` |

**碎片**

| 項目 | 怎麼驗證 |
| --- | --- |
| 照 V8 的挑法能放掉的頁、有物件的頁上的空格子、配置的慢路徑，sweep 做完和 marking 開始時各量一次 | `cargo test -p gc --test it fragmentation:: sweep::fragmentation_is_measured_once_the_sweep_is_done`；`gc_bench_test.py` |
| marking 開始時每個工作能放掉的頁的中位數都不到 10%（churn 放大 10 倍 0%），照決定 39 不做 compaction | `bench.py gc` 加上 churn 放大 10 倍；`bench-6a-apple-m4.json` |

**效能目標的驗收**（決定 16，照決定 23 第 7 條、決定 26、33 改過；最後的狀態，gc-parallel，`bench-5-apple-m4.json`）

指令：`caffeinate -i python3 scripts/bench.py gc --runs 5`（microbenchmark 在 v8、v8-single、isolate、standalone、gc、gc-parallel 上），`caffeinate -i python3 scripts/bench.py gc --runs 5 --heaps standalone,gc,gc-parallel --replay target/bench/gc/recordings/*-wasm.kungc`（isolate 的重播在這台機器上會掉 trace 的事件）。

| 條件 | 結果 | |
| --- | --- | --- |
| **含 sweep 的收尾停頓 p99 ≤ 1 ms** | 微基準 5 到 10 µs，ids 402 µs（走一百萬格的 id 表），重播 5 到 44 µs | 過（第 4 段 gc-tree 6.4 ms） |
| 存活率低的 minor GC p99 ≤ 1 ms | 最多 0.72 ms（build 在 gc 上） | 過 |
| 全部活著的 minor GC p99 ≤ 5 ms | chain-high、tree-high、graph-high 的最長停頓 2.0、2.2、3.0 ms，gc-tree 的重播最長 3.3 ms | 過 |
| 存活率低的重播 GC ≤ cppgc 的 1/3 | 0.007 到 0.079 倍 | 過 |
| gc-tree 的重播 ≤ cppgc | 45.1 對 1908 ms（第 4 段 129 ms） | 過 |
| peak used ≤ cppgc 加 nursery 最大的容量加 2 MiB | 全部 | 過 |
| low 的模式 ≤ V8 的 1.5 倍、high ≤ V8 | 0.67 到 0.79、0.19 到 0.47 倍 | 過 |
| marking 每秒的物件數 ≥ cppgc（單 thread 的 gc） | build 46.1、chain-high 269.6、tree-high 216.4、graph-high 62.4 百萬 | 過（cppgc 25.6、185.1、159.3、62.3；graph-high 只多 0.2%） |
| 寬的圖平行比較快 | 第 4 段量：build 的 scavenge 1.55、churn 1.85 ms（單 thread 2.02、2.56 ms） | 過 |

- 異常值：innerHTML 的重播有一次收尾停頓 6.0 ms（其中標記 6.0 ms），293 次 marking 乘 5 次裡只有這一次，p99 13 µs；前幾次量都沒有出現，原因沒有查。
- gc-tree 的重播最長的停頓 3.3 ms，是 marking 開始的停頓（開始前的 scavenge，`docs/limitations.md`）。

## 做過的檢查

2026-10-24 到 25 在 Windows 11（Intel Raptor Lake）上做第 0 段的 0a 到 0g；2026-10-25 到 26 在 macOS 26.6.2（Apple M4：4 個 P-core、6 個 E-core，24 GB，接電源）上做第 0 段的驗收和第 1 到 6 段。動到 fork 的 C++ 之後一直用 `RUSTY_V8_ARCHIVE` 連結本機編的 V8（`ci.py v8 --profile release`，只重編改過的檔案）：fork 的 `33236d6`（0b）、`d906727`（0e）、`c1c62f0`（0g）。

- **基準**（第 0 段；Mac 的數字，括號是 Windows 的）：
  - microbenchmark：存活率低的配置模式 V8 的 GC 1.5 到 2.9 ms（2.2 到 3.8），standalone 的 cppgc 14.4 到 25.3 ms（27 到 45）、isolate 的 7.5 到 9.1 ms（12 到 16）：cppgc 沒有新生代的代價。存活率高的 V8 反而慢（scavenge 要複製存活的，graph-high 55.7 對 isolate 9.8 ms）。
  - blink-perf 的六頁：JS 文件的 gc-tree 最差（V8 的 atomic 2.6 秒、最長停頓 370 ms（543 ms））；wasm 文件的 sweeping 在 main thread 上多（0.7 到 2.3 秒）；insert-before 不配置，wasm 版慢 2.3 倍是呼叫的成本。
  - 重播：wasm 錄製在 standalone 上 GC 972 到 1773 ms、最長停頓 43 到 66 ms、peak used 319 到 366 MiB；同一份在 isolate 上 GC 少、但停頓長、peak 大兩到三倍（V8 照自己的 heap 排程 CppHeap）。錄製 12 份共 28 GB，類別往上取多 2.6%。
  - 存活率（`gc_bench analyze`，nursery 1、4、16 MiB）：沒有被 JS 撐住時 DOM 的垃圾幾乎都在第一次 minor GC 前死掉（create-element、gc-forest、gc-mini-tree 的 wasm 錄製 0%，innerHTML 33%、11%、3%）；留著的整棵樹一起留著（gc-tree 100%）；JS 的錄製 95% 以上（wrapper 撐到 V8 宣告它死）。老物件指向年輕物件的邊每次 minor GC 最多 1 到 4 條。
  - 1b 的 barrier 次數：欄位寫入的 slow path 兩種 barrier 每輪都只有幾次；Dijkstra 收尾重掃的 root 在 JS 文件上每次 25 萬到 120 萬格（最多 210 萬）；SATB 的代價是 marker 慢時的 floating garbage。1d 的停頓：同步的做法一百萬個節點 4.5 到 6.6 ms，交換的在 1 ms 的步驟、unified schedule 下 1.0 ms（決定 17、`details.md`）。
- **之後的段落量到、改了做法的**（數字是 gc-parallel，或註明）：
  - 2e：單 thread 的 marking 34% 在設 mark bit（等記憶體，換成非 atomic 沒有差別）；型別註冊時算好邊的 offset 清單（graph-high 57 → 65.5 百萬/秒）；門檻照 cppgc 的 heap growing；平行 marking 在 tree 上比單 thread 慢 5 倍，改成只叫醒一個 worker、一段 512 項、沒有段可拿就不叫醒之後持平。之後一定要付的成本（atomic 讀邊、分代的檢查、寫邊看 marking）量過，graph-high 還比 cppgc 快 8%。兩邊量 peak 的時間點原本不同，改成都在 task 之前（決定 22）。
  - 3b：slot set 一有就發布時平行慢 1% 到 18%，改成湊滿一段才發布之後持平；1 MiB 的 nursery 平行沒有變快（V8 也大約每 MB 一個 scavenge task）。
  - 3d、3e：照年齡 copy 時存活率高的退步（gc-tree 的重播 4318 ms），照配置的地方 pretenure 和門檻 2 倍之後 958 ms（決定 26、27）；標記時數 site 讓 chain-high、tree-high 慢約一成（決定 27 第 8 條）。
  - 4e：撞到門檻時剩下的在停頓裡標（graph-high 收尾的標記 p99 5.3 ms）；比了放寬門檻、V8 的 margin、更早開始、安全點幫忙標，選 V8 的 margin（決定 34），之後 p99 2 到 14 µs。
  - 5a：gc-tree 的重播收尾的停頓 6.4 ms 幾乎都是 discard 頁（約 6.1 ms），交給 worker 之後 0.44 ms。5b：sweep 移到背景之後 36 µs。
  - 5d：5b 的「marker 邊標邊數 bytes」讓單 thread 的 graph-high 每秒標的物件低於 cppgc，比了預先算格子大小、在標上時數、`Marks` 對齊 cache line 都不夠；改成 sweep 做完才定 `used` 和門檻（決定 36 第 4 條的補充）。
  - 6a：sweep 剛做完時 churn 放大 10 倍能放掉的頁 11% 到 12%，但 peak 時 committed 只比 used 多約 0.5 MiB；改成也在 marking 開始時量（V8 挑頁的時間點），中位數都不到 10%（決定 39）。量測本身的成本看不出來。
- **找到、修好的錯**：
  - 0b：`used` 的註解原本寫「垃圾在 sweeping 時才扣」，讀 V8 的原始碼改正（marking 結束時就扣）。
  - 0c：churn 可能挑到沒有子節點的節點（越界）、重建的子樹比拆掉的小；`bench.py gc` 的百分位數用了 Python 的 `round`（.5 取偶數），改成 `math.ceil`。standalone 的 heap 工作結束後空等約 99 ms 的 delayed task，改成另外記 `idle`。
  - 0d：scavenge 被算成 atomic，加上 young。
  - 0f：一條 edge 是 4 byte（壓縮的指標），類別表照 8 byte 排，重播會 panic。
  - 0g：wasm 文件的 guest 用 `usleep(0)` 等下一輪，整個量測在一個 task 裡，cppgc 收不了（`Oilpan: Out of memory`），改成等 1 ms。trace 掉了事件：Perfetto 給每個 thread 寫入的共用記憶體（SDK 預設 256 KB）在 cppgc 每秒幾千次的配置時 marking 下滿了，fork 改成 32 MB（`c1c62f0`），54 次重播沒有掉。`long-sibling-list`、`deep-tree-clonenode` 兩頁拿掉（決定 15 第 1 條的補充）。
  - 1c、1d（原型）：`alloc` 推不出型別，改成 init struct 說出自己的物件；SATB 和 V8 的 Dijkstra 混用時，經過 wrapper 讀出的物件要標上（`Persistent::get` 加讀取的 barrier）。
  - 2a：配置改成先寫完物件才寫 header 的型別編號（審查找到，safe 的 API 寫不出會 panic 的 init，是多一層保險）。
  - 2c：root 原本照段發布到全域，小 heap 也叫醒 worker，改成呼叫的 thread 本地的工作。
  - 2d：`test.py mutate` 把 Verus 的證明失敗歸成 build failed，改成歸成抓到。
  - 3a：fuzz 第一分鐘找到 ASan 的 use-after-poison：scavenge 之後給整頁 from-space 填毒值時，沒配置過的部分還被 ASan 標著，改成先解開（`fuzz/regressions/young-scavenge.bin`）。2e 加的 `ThreadPool::run` 開工時的叫醒寫成 `max_concurrency(1) > 1`，要一個 worker 的工作一個都不叫醒、偶爾卡住，改成 `max_concurrency(0) > 0`（marking 沒有被影響）。
  - 3b：`HeapVector::new` 寫自己的 `store` 沒經過 barrier（3a 就有）；fuzz 找到 slot set 的頁在升上去的 copy 記了新的一格之後才被處理時記錄掉了，改成看 young 的表（`young-slot-order.bin`）。
  - 3c：nursery 縮小時沒有把累積留下的歸零，下一次 scavenge 馬上又長回去。
  - 4a：stop-the-world 的標記變慢，A/B 找到是 `MarkTable::get` 的 acquire，改成 relaxed；`gc_bench` 的 `Traced` 沒轉交 `posts_jobs`；`run` 結束後 `notify_all` 讓小的 scavenge 變慢；debug build 的 `publish` 在 marking 期間逐個標黑，蓋住了整段標黑（mutation test 找到）；`Worklist::rejoin` 是多餘的，拿掉。
  - 4b：concurrent marking 收尾時 nursery 不是空的，「sweep 釋放的格子上沒有 slot set 的記錄」原本的理由不成立；推了新的論證（沒有錯），改寫 `crates/gc/soundness.md` 的 I10，verifier 加上檢查。
  - 4d：fuzz 的參考模型在操作之後才把到不了的記成可能被收掉，但同一個操作裡的讀取看得到開始時那次 GC 的結果（4b 起就在），改成 `mutate` 之前記下（`read-after-a-safepoint-gc.bin`）。
  - 4e：第一次完整量測時存活率低的重播的 GC 是第 3 段的 20 倍：`update_weak` 每次 scavenge 把 weak 和 ephemeron 兩張 slot set 表的 16384 頁整張走一遍，改成只走用過的頁。
  - 5a：verifier 偶爾說 `Discarding` 的頁不在 job 手上（worker 拿出、還沒放進「做完」），加了 `busy`。
  - 5b：fuzz 找到 ASan 的 use-after-poison：main thread 掃完有 finalizer 物件的頁、套用時先毒化空格子，接著 finalizer 讀死物件就踩到，改成 finalizer 和 debug 的毒值之後才毒化（`finalize-before-poisoning-a-swept-page.bin`）。第一版的 sweeper 照 marker 每頁一個 worker：9 個 worker 搶鎖、收尾叫醒 9 次，GC 時間比 5a 多 60%，照 cppgc 改成最多一個 worker。
  - 5c：`Drop` panic 時頁的 finalizable 計數對不上，改成套用時從 bitmap 重數；verifier 在收尾後檢查頁時 worker 正在清那一頁的 bit，改成套用時才檢查。
  - 收尾：Miri 找到頁的 `Marks` 在 `Box` 裡、`MarkTable` 另外指向它的 Stacked Borrows 違規，改成 `OwnedMarks`（「Miri」）。
- **工具和平台上看到的**（寫進 `docs/benchmarks.md`、`docs/limitations.md` 的已經寫了）：
  - isolate 的 heap 在沒有 JS 配置時幾乎不收（Windows 上 build、churn、list 回合期間一次也沒有；replace 長到 62.3 MB）；Mac 上會收：V8 照自己的 heap 排程 CppHeap，兩台的時機不同。
  - standalone 的 heap 的 GC 大多在 cppgc 自己 post 的 task 裡整個做完，kun 的 `force_gc` 和 marking 的狀態都看不到；大的 heap 才 incremental。錄製改用 fork 的 GC 計數（決定 13 第 9 條）。
  - V8 的 `--single-threaded-gc` 的 scavenge 照樣交給 worker，main thread 在等。
  - V8 的 `used` 會多算背景編譯的配置區（`GetHeapStatistics` 只釋放 main thread 的 linear allocation area）：`gc_bench` 的測試關掉背景編譯，`bench.py gc` 用預設。
  - wasm 文件的 GC 只在 event loop 的頂端收尾（單獨的 heap 不掃 stack）；JS 文件的錄製只有幾個 turn（`run_due_tasks` 一個接一個跑到期的計時器），只能在 isolate 上重播。
  - 錄製關閉時掛點的檢查本身讓 microbenchmark 慢 1% 到 8%（每個操作只有幾 ns）；`#[cold]` 之後留著，完全沒有成本要另外編一個 kun，沒有做。
  - Mac 上 gc-tree 的 wasm 錄製在 isolate 上重播時，main thread 寫了 378 萬個事件、佔滿 32 MB 的共用記憶體，worker 的事件掉了；使用者決定記成量不到、不改工具。之後的重播只跑 standalone、gc、gc-parallel。
  - 每個 process 第一次重播有一兩次 2 到 4 ms 的 scavenge：停頓裡一次普通的配置進了 kernel，是 mimalloc 延後的歸還（`MIMALLOC_PURGE_DELAY=-1` 時最長 15.6 µs），記進 `docs/limitations.md`。
  - Windows 的這台機器上 Python 兩次出現不可能的錯誤（`i += 1` 丟出 context manager 的錯誤），重跑就過；CPU 是 Raptor Lake（Family 6 Model 183），這一代有已知的不穩定，還沒查證。
  - TLC 1.7.4 的 `-config` 給絕對路徑時找不到檔案，`check.py` 都放進 `target/tla/` 再跑。Miri 預設隔離檔案系統，讀不到 `fuzz/regressions/`，加 `-Zmiri-disable-isolation`。
  - Kani 和 Verus 的試用（2d）：Kani 在一整頁的 bitmap（256 個 word）400 秒內沒做完，`free_ranges` 回傳 `Vec` 時 16 格就用 13.4 GB；Verus 對任何長度 0.4 秒證完。使用者選 Verus。兩個都抓到 2b 的「第一個 word 不跳過 `from` 之前的 bit」。
- **mutation test**（每步一份 spec，`mutants-*.toml`；`gc` 的都 `--keep-going`，用 `RUSTY_V8_ARCHIVE` 跑；要 loom、shuttle 的用 `test.py gc-models` 當測試指令，Verus 的用 `test.py gc-verus`）：

  | spec | 弄壞的 | 結果 | 第一次沒抓到、補了什麼；沒抓到的原因 |
  | --- | --- | --- | --- |
  | `0a` | 2 | 都抓到 | |
  | `0b` | 5 | 都抓到 | committed 當 used：強制的 GC 也把空 page 還回去，補了只剩 skeleton 時的檢查 |
  | `0c` | 27 | 26 個 | 最高值取最後一回合的：測試排不出 GC 在哪個回合收 |
  | `0d` | 26 | 25 個 | v8-single 不給 `--single-threaded-gc`：報告照樣合法，看不出用了幾個 thread |
  | `0e` | 50 | 49 個，1 個等價 | 6 條補了測試（錄製前配置的物件、`TaggedEdge`、standalone 的 marking 和 step 要 10 萬個物件……）；等價的：新物件不拿掉範圍蓋到它的舊物件 |
  | `0f` | 35 | 都抓到 | turn 不跑 heap 的 task、full GC 留著死掉的老物件 |
  | `0g` | 29 | 都抓到 | |
  | `1b` | 23 | 都抓到 | 「開始時的 scavenge 不全部升上去」等價，拿掉那個參數 |
  | `1c` | 22 | 都抓到 | `HeapVector` 移除之後的邊沒清（原型；spec 和原型一起在 `de8ea619`，收尾時刪掉） |
  | `1d` | 25 | 都抓到 | 保守掃 stack 和 V8 背景的 marker 遮住兩條，改了測試；刪 strong root 的 barrier 等價（原型開始時就標了所有 root），拿掉（同上） |
  | `2a` | 26 | 都抓到 | |
  | `2b` | 28 | 都抓到 | 一條讓 `free_ranges` 不前進，測試吃到約 43 GB，macOS 跳出記憶體用完的視窗：`free_ranges` 加 assert、測試的迴圈加上限，之後每次 mutation test 旁邊跑 4 GiB 的看門狗（都沒有砍到）；兩條等價拿掉（新物件的 bit 只在 debug 設） |
  | `2c` | 9 | 7 個，2 個卡住（逾時算抓到） | 只在某些時機出錯的兩條改用 loom；呼叫的 thread 發布時不叫醒 worker 只少了平行，拿掉 |
  | `2d` | 10 | 都抓到 | 被標的垃圾通過：補了活物件 GC 之後走不到仍標著的測試 |
  | `2e` | 21 | 都抓到 | 一條等價，換成別的寫法 |
  | `3a` | 25 | 都抓到 | 單獨的 scavenge 不跑 verifier |
  | `3b` | 13 | 12 個 | 指向 to-space 的當成 old：shuttle 的測試加上第二次 scavenge、放滿 16 頁 holder；頁尾不填空段等價（LAB 整除一頁），拿掉；slot set 的 bitmap 搶輸了還用自己的沒抓到：要兩個 thread 同時第一次記同一頁，shuttle 不在 std 的 atomic 上切換 |
  | `3c` | 8 | 都抓到 | 測試沒碰到上限和下限、縮小時剩下的剛好沒超過：改寫測試 |
  | `3d` | 13 | 都抓到 | |
  | `3e` | 29 | 都抓到 | |
  | `4a` | 30 | 29 個 | black allocation 的 4 條被 debug 的 `publish` 蓋住（修了）；刪 root 的 barrier 等價，和 TLA+ 一致，保留（決定 29 第 10 條） |
  | `4b` | 22 | 都抓到 | `WeakPersistent::get` 不過 barrier：補了接到黑的物件上的確定的測試 |
  | `4c` | 18 | 都抓到 | `Worklist::reseed` 不設 `busy` 等價，拿掉 |
  | `4d` | 14 | 都抓到 | |
  | `4e` | 22 | 21 個 | activity 從 heap 開始算、不看門檻的一半：補了測試；不看 cage 剩下的一半：要先填滿半個 4 GiB 的 cage |
  | `5a` | 16 | 15 個 | worker 沒有真的 discard：測試看不出記憶體有沒有還給作業系統 |
  | `5b` | 39 | 38 個 | 6 條補了測試；配置自己掃之前不先套用 job 的結果：只多掃頁 |
  | `5c` | 15 | 都抓到 | 有 finalizer 的頁任何 thread 都能拿、large object 的頁不放、verifier 的兩個檢查 |
  | `5d` | 9 | 都抓到 | verifier 的核對 |
  | `6a` | 25 | 都抓到 | 活的最多的先挑、同一次 sweep 取樣兩次 |

  1a 的 mutation 是模型本身：`check.py` 的 11 個檢查 × 2 種大小，每一格有預期的結果。沒抓到的、等價的和沒放進清單的（只少了平行、只多做事、會吃光記憶體或停不下來的）記進 `docs/test-gaps.md`。V8 的 patch（1d）沒有做 mutation（每條要重編 V8，由 bridge 的測試間接守著）；fork 的 C++ 在 0b、0e 弄壞過，跑完重編一次沒改過的 V8。每次跑完 `target/mutate/originals` 都不在，檔案都還原了。
- **不穩定的測試，找到的原因**：
  - 0d：`gc_bench` 的 `tree_low` 偶爾 used 892 KB（平常 120 KB）：V8 多算背景編譯的配置區（上面），測試關掉三個背景編譯之後 300 次 0 次。
  - 3a：platform 的測試偶爾卡住：`ThreadPool::run` 的叫醒條件寫錯（上面），修好後連跑 60 次沒有卡住。
  - 4a：pool 的測試的斷言時機不對時失敗，失敗後 unwinding 時交出的 job 沒被取消，pool 的 drop 一直等；handle 和 pool 的 drop 改成取消，斷言改成至少一個讓出，連跑 30 次沒有失敗。
  - 4e：加了 overshoot margin 之後 `heap::a_long_vector_keeps_its_edges` 30 次失敗 2 次：預設的 platform 做 concurrent marking，32 MiB 的 margin 下撞不到門檻，有沒有收尾看 worker 標完沒有；`small_heap` 改成 `min_overshoot_bytes: 0`，連跑 40 次沒有失敗。
  - 5a、5c：verifier 和 worker 同時跑時的兩個（上面），只在真的 pool 上出現，記進 `docs/test-gaps.md`。
  - 從 5a 起每步 `cargo test -p gc` 連跑 20 次沒有失敗。
- **fuzz、模型、證明**：`test.py gc-fuzz` 每步跑 1 到 3 分鐘，找到的四個（上面）修好之後都沒有失敗；最後（6a）1 分鐘 2779 次（2d 時每秒約 550 次；3a 起每次多了 scavenge 和 verifier，66 次，之後隨操作變多降到約 46 次）；種子是 12 份錄製各 4 個（`gc_bench fuzz-seeds`）。ASan 的毒化在 2d 手動確認過有用（讓 marking 不標邊、關掉 verifier，ASan 報 use-after-poison）。最後 `test.py gc-models` 是 loom 5 個、shuttle 4 個。`check.py --spec SatbYoung` 最後一版 3 個物件 1 個欄位約一千萬個狀態、80 秒。Verus 的證明（`test.py gc-verus`）第一次 45 秒、之後約 1 秒。
- **Miri**：
  - 1c、1d 的原型、2a、2b 在 Stacked 和 Tree Borrows 下各跑一次都過（2b 的整合測試各約 695 秒）；之後平常只跑 Stacked Borrows（決定 18 第 6 條）。2c 的單元 14 個和整合 27 個在 Stacked Borrows 下都過（843 秒），是最後一次跑完整套的；Miri 底下預設的 platform 改成 `InlinePlatform`（process 共用的 pool 的 worker 永遠不結束）。
  - 第 2 段做完時的整套跑了 31 分鐘、整合測試還沒跑完，使用者決定停掉；Miri 底下不開 heap verifier、縮小隨機的樹之後重跑，整合的 33 個跑了 9 個都過、沒有 UB，又停掉。之後到收尾都沒跑（2026-10-26，使用者決定）。
  - 收尾（2026-10-26，決定 40 第 1 條）：`cargo miri nextest run` 平行跑（10 個核心）。第一次就找到一個真的 UB：頁的 `Marks` 在 `Box` 裡，`MarkTable` 另外指向它，頁跟著狀態搬動時 `Box` 在 Stacked Borrows 下重新宣稱唯一，那個指標就失效（第 4 段加 `MarkTable` 時引入；113 個測試都停在這裡），改成 `OwnedMarks`（`crates/gc/soundness.md` 的「找到並修正過的問題」）。之後四個失敗是測試在 Miri 下的寫法，只改了測試：`platform::tests::a_dropped_pool_stops_its_posted_jobs` 故意 `forget` 掉 handle（改成 pool drop 之後才 drop 它）；`sweep::a_finalizer_that_panics_finalizes_nothing_twice` 照設計 leak 掉沒 finalize 的值（計數器改用 `static`，leak 的值不再擁有記憶體）；fuzz 的 `a_written_run_with_ephemerons`、`weak_edges` 用 process 共用的 pool，worker 不結束（Miri 下每次一個 pool）。加了只在 Miri 下編的 `concurrent::the_margin_is_at_most_half_of_the_cage_left`（16 MiB 的 cage 上預設的 margin 只到剩下的一半）。
  - 時間：兩輪合起來 173 個測試裡 163 個在 Miri 下通過（都在 `OwnedMarks` 之後）；跑完的 163 個加起來 166 分鐘的 CPU 時間，最慢的 `young::an_old_objects_edges_written_at_its_allocation_keep_young_ones` 14 分鐘，`young::the_nursery_grows_with_its_survivors_and_shrinks_after_garbage`、`young::a_shrunk_nursery_counts_over` 各超過 34 分鐘沒跑完。使用者決定不分批、不為 Miri 縮小，每週定時跑一次全部，這次當成跑過。**沒在 Miri 下跑完的**：fuzz 的 7 個、上面兩個 `young::`、改過之後的 `sweep::a_finalizer_that_panics_finalizes_nothing_twice` 和新的 margin 測試，留給每週的那一次。
  - 第一次的每週 Miri（2026-10-27，Fedora 44，AMD Ryzen AI 9 HX 370：12 核、24 個 thread，64 GB，筆電接 45 W 的 USB-C 電源）：`crates/gc/README.md` 的指令加上 `-j 24`，nextest 另給一份設定（`slow-timeout` 300 秒、`fail-fast = false`、JUnit）。174 個測試（單元 65、整合 109）173 個通過、1 個失敗，**沒有 UB**；上次沒跑完的那些這次都跑完了。
    - 失敗的是 `concurrent::the_margin_is_at_most_half_of_the_cage_left`（斷言，不是 UB；第一次在 Miri 下跑到）：預期 marking 期間 old 長 7 到 9 MiB 才收尾，實際 5,580,288 bytes。`Inner::at_limit` 每次用當下的 `used` 算 cage 剩下的（`CAGE_BYTES − (used + limit)`），old 長多少，margin 就跟著縮一半，解出來在約 (C − S − L)/3（C 是 cage、S 是開始時的 used、L 是門檻）收尾，16 MiB 的 cage 上約 5.33 MiB，和實測對得上。照決定 41 改。
    - 時間（nextest 量的，不含休眠；24 個一起跑、搶 SMT，偏高）：從開始到結束 22,386 秒（6.2 小時），各測試加起來約 15 個 CPU 小時，110 個在 30 秒內。最慢的：`young::the_nursery_grows_with_its_survivors_and_shrinks_after_garbage` 21,913 秒（6.1 小時，整次就是在等它）、`fuzz::a_written_run_with_sites_agrees_with_the_model` 122 分、`young::a_shrunk_nursery_counts_over` 66 分、margin 的測試 60 分、fuzz 的 `a_written_run_agrees_with_the_model` 49 分、`a_written_run_with_ids_agrees_with_the_model` 45 分、`random_runs_agree_with_the_model` 30 分、`heap::a_large_objects_pages_are_reused` 22 分。分組：fuzz 的 7 個 289 分、`young` 83 分、`heap` 79 分、`concurrent` 73 分。
    - 電腦在 00:27 閒置休眠、03:24 才醒（GNOME 用電池時閒置 15 分鐘休眠，插電時不休眠；當時可能沒認到電源），牆上的時間 9.2 小時。nextest 量的是 monotonic 時間，不含休眠，從它的數字看不出來；醒來之後用 `systemd-inhibit` 擋住休眠到跑完。
    - 慢在哪（`-Zmiri-report-progress` 每隔固定數量的 basic block 印出被解譯的程式的 backtrace，當成取樣；gdb 取 Miri 本身的 stack；perf 在這台開不了）：
      - `young::the_nursery_grows…` 關掉 borrow tracking（`-Zmiri-disable-stacked-borrows`）也要 2,597 秒，83% 的 basic block 在 scavenge：16 輪各 8000 個帶 finalizer 的節點，先複製再升上去，是測試本身的工作量。Stacked Borrows 下再慢約 8 倍；gdb 取的 25 次裡 23 次在 Stacked Borrows 的 `grant`、`insert`、`iter_mut`（borrow stack 長，每次 reborrow 都要線性搜尋、搬移）。
      - fuzz 的 written run：`ops::apply`（`crates/gc/tests/it/ops.rs`）每個操作都從全部的 handle 重建 `objects` 和 `blobs`、讀每個的 id，成本是操作數乘 handle 數；`a_written_run_with_sites` 73% 的 basic block 在這裡。
      - margin 的測試：`std::array::from_fn(|_| Cell::new(0))` 對 2000 個元素各呼叫一次 closure，79% 的 basic block 在這裡。
    - 比較 borrow 模型（11 個一起跑，各一次）：

      | 測試 | Stacked Borrows | Tree Borrows | 不做 borrow tracking |
      | --- | --- | --- | --- |
      | `young::an_old_objects_edges_written_at_its_allocation_keep_young_ones` | 556 秒 | 634 秒 | 97 秒 |
      | `heap::a_long_vector_keeps_its_edges` | 240 秒 | 350 秒 | 69 秒 |
      | `fuzz::a_written_run_with_ephemerons_agrees_with_the_model` | 369 秒 | 356 秒 | 253 秒 |
      | `young::the_nursery_grows_with_its_survivors_and_shrinks_after_garbage` | 21,913 秒（整套裡） | 3,600 秒沒跑完 | 2,597 秒 |

      Tree Borrows 沒有比較快。Miri 的旋鈕（`young::an_old_objects…`，同一批 5 個一起跑）：預設 1,120 秒；`-Zmiri-provenance-gc=1000`（預設 10000）867 秒，快 23%；`=100` 1,470 秒，慢 31%；關掉 data race 檢查和 weak memory 的模擬 1,078 秒，只快 4%。這台筆電接 45 W 的電源，跑滿時電池還在放電，同一個設定在兩批裡差到兩倍（556 對 1,120 秒），所以只比同一批裡的。怎麼改善記在 `README.md` 的待決定 20。
    - 照決定 41 修好 margin（2026-10-27，同一台）：`cargo test -p gc` 都過；margin 的測試的初始化改成 `[const { Cell::new(0) }; 2000]` 之後，在 Miri 下單獨跑 24 秒、通過。mutation test（`mutants-margin.toml`，三條都抓到）：margin 一直是 0，`concurrent::` 的測試抓到；margin 跟著 old 長大重算（改回原本的寫法）、上限放寬到 cage 剩下的全部，都由 Miri 下的 `concurrent::the_margin_is_at_most_half_of_the_cage_left` 抓到（各 13、22 秒）。
- **各段的收尾**（`cargo test --workspace --no-fail-fast`、`cargo fmt --check` 連同 `sdk/lua/generator`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`）：

  | 段 | `cargo test --workspace` | 收尾時修的 |
  | --- | --- | --- |
  | 0（Windows） | 3991 通過、3 ignored | `dom`、`gc_bench` 的格式；clippy：`let_and_return`、`Recorder` 裝箱、錄製檔名的計數改用 `AtomicSequenceNumber`、四個 `drop_non_drop` |
  | 1（Mac） | 4021 通過、3 ignored、1 失敗 | 沒有 |
  | 2 | 1 失敗 | 沒有 |
  | 3 | 1 失敗 | `sanity.py boundary`：`site.rs` 的 `NEXT_INDEX` 加進例外 |
  | 4 | 1 失敗 | `cargo fmt` 重排 8 個檔案；clippy 7 處（`+=`、`% 2 == 0`、多餘的借用、`then().unwrap_or_default()`） |
  | 5 | 1 失敗 | 沒有 |
  | 6 | 1 失敗 | `cargo fmt`：`gc_bench` 裡手寫的換行 |

  失敗的都是 `shell` 的 `navigation_timing::the_entry_of_a_redirected_load`（`redirectStart` 每次是 0）：這台 Mac 上 `main` 本來就有（這個分支在 `net`、`browser`、`renderer`、`shell` 和 `origin/main` 一樣，`shell` 不依賴 `gc`），使用者決定先不處理。原型不在 kun 的 workspace，它的 `cargo test`、clippy、fmt 在 1c、1d 另外跑過。
- **最後的測試數字**：`cargo test -p gc` 單元 65、整合 108、compile-fail 1；`cargo test -p gc_bench` 單元 5、整合 41；`gc_bench_test.py` 29 個；`test.py gc-models` loom 5、shuttle 4。
