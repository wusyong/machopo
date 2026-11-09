# RFC 0060 的細節：GC 的入口、借用結構和數字

2026-10-19 查的，V8 是 rusty_v8 fork 的 `kun-v152.2.0-11`（`../third_party/rusty_v8/v8`）。下面的路徑都相對於 `v8/`。

## GC 的入口

cppgc 的物件只有兩種情況會被釋放：GC 完成時（atomic pause，接著 sweep），和配置時順手的 sweep（只處理上一輪 marking 沒標到的物件）。逐一看 scope 裡可能碰到的入口，以及打開 `cppgc::subtle::NoGarbageCollectionScope`（`HeapBase` 的 `no_gc_scope_` 計數，`IsGCAllowed()` 在它大於 0 時是 false，`src/heap/cppgc-internal/heap-base.cc`）之後的行為：

| 入口 | 在哪裡 | no-GC scope 底下 |
| --- | --- | --- |
| standalone heap 長到 atomic GC 的上限：`CollectGarbage` | `heap-growing.cc` 的 `AllocatedObjectSizeIncreased` → `heap.cc` 的 `Heap::CollectGarbage` | 一開始 `if (!IsGCAllowed()) return;` |
| standalone heap 長到 incremental 的上限：開始 marking | 同上 → `Heap::StartIncrementalGarbageCollection` | `if (IsMarking() \|\| in_no_gc_scope()) return;` |
| `CppHeap` 回報配置量：可能開始 V8 的 marking，或超過太多時 `FinalizeIncrementalMarkingAtomically`（當場做完一次 unified GC） | `src/heap/cppgc-js/cpp-heap.cc` 的 `ReportBufferedAllocationSizeIfPossible` | 一開始 `if (!IsGCAllowed()) return;`（只累積，之後再回報） |
| `CppHeap` 配置失敗：`CollectGarbage`（V8 的 `CollectAllGarbage`） | `cpp-heap.cc` 的 `CppHeap::CollectGarbage` | `if (!IsGCAllowed()) return;` |
| 配置失敗時的重試 | `cpp-heap.cc` 的 `CppHeap::RetryAllocate` | 回傳 false：不 GC，配置失敗就是 OOM |
| 配置時推進 marking | `marker.cc` 的 `IncrementalMarkingAllocationObserver` → `AdvanceMarkingOnAllocation` | 照樣做。只標記；標完時 `ScheduleIncrementalMarkingTask` 排一個 task 收尾，不當場收尾 |
| 配置時順手 sweep | `object-allocator.cc` 的 `SweepForAllocationIfRunning`、`FinishIfRunning` | 照樣做，會執行 finalizer。只處理上一輪沒標到的物件 |
| V8 自己配置（JS 物件、字串、wrapper）觸發的 GC | `src/heap/heap.cc` | **不看 cppgc 的 no-GC scope**：unified GC 照樣進 `CppHeap::EnterFinalPause`，那裡只 `CHECK(!IsGCForbidden())`（`DisallowGarbageCollectionScope`，會 crash，不是跳過） |
| GC 的 task（cppgc 的 incremental marking、V8 的 GC job） | 從 kun 的 platform 執行 | 只在 `run_platform_tasks` 裡（要 `&mut DomIsolate`，`soundness.md` 的 A3、I2） |
| 背景 thread | concurrent marker、sweeper | 不釋放有 finalizer 的物件（A1），每個 Rust 物件都有 finalizer |

配置時的 sweep 為什麼碰不到 scope 拿得到的物件：

- 上一輪沒標到的物件，在那一輪的 atomic pause 時就沒有活的物件指著它（marking 完整），之後寫進的 edge 都經過 Dijkstra barrier；weak 參照（`WeakMember`、`WeakPersistent`，`NodeIdMap` 也是）在 pause 時清掉。所以從 root、edge、id 表都拿不到它。
- pause 之後才配置的物件不在那一輪的 sweep 範圍內：現在 `Handle` 撐著的新物件也依賴這點（`Persistent` 不會讓 sweep 跳過物件，sweep 只看上一輪的 mark bit）。

沒有 no-GC scope 時在 mutation scope 裡配置，最壞是當場做完一次 GC，所以 `soundness.md` 的 I3 要求配置的操作要 `&mut` scope，配置時手上的指標都先 root。

## kun 的借用結構

- **`DomIsolate`**：`mutate()`、`read()` 和所有 GC 的入口都要 `&mut self`（I2）。
- **`BindingContext`**（`crates/dom/src/v8_engine.rs`）：`mutate()`、`read()`、`scope()`（V8 的 `PinScope`）都要 `&mut self`，所以 DOM scope 活著時不能呼叫 V8（I13）。`v8_engine.rs` 內部同時用到 DOM 狀態和 V8 scope 的地方（`wrapper_of`、`wrap_fresh`、`bind_window`）只拿 root 過的 handle；`node_realm_global` 在 read scope 裡讀節點，只建 V8 的 `Local`，兩個 heap 都不配置。
- **`dom_api` 的 `async fn`**（34 個）：拿著 `&mut dyn ScriptHost` 跨過 `.await`（例如 `traversal::filter` 的 `call_async`）；DOM scope 從 `cx.mutate()` 開，借用 `cx`，所以借用著 scope 的東西跨不過需要 `cx` 的呼叫。
- **foreign engine**（`DomAccess`，`crates/dom/src/foreign.rs`）：`mutate()`、`read()` 要 `&mut DomAccess`（I15）。

所以只要 `NodePtr` 活不過借用著 scope 的 closure，它就活不過 V8 的呼叫、script 的執行和 GC 的入口：`with_ptrs` 的 `'b` 是 closure 專屬的 brand。

## handle 的成本

2026-10-19，TodoMVC-jQuery，`ci.py speedometer --sample`（RFC 0058 第 2 段之後的取樣），佔 cycle 樣本的比例：

| 函式 | % |
| --- | ---: |
| `Root::try_new`（自己 1.11） | 1.47 |
| `cppgc__Persistent__Assign` | 0.85 |
| `cppgc__Persistent__CONSTRUCT` | 0.63 |
| `cppgc__Persistent__DESTRUCT` | 0.31 |
| `Rc<RootCell<…>>` 的 drop | 0.35 |
| `cppgc__WeakPersistent__CONSTRUCT`（`NodeIdMap`，不在範圍內） | 2.19 |

`Root::try_new` 的呼叫者：`create_node` 0.40、`html::fast_path::build` 0.24、`replace_all` 0.12、`create_element_of` 0.11、`update_elements` 0.11、`collection::item` 0.11、`form_states::Affected::element` 0.11，其餘零散。

## API 的範圍

2026-10-19 的 `main`（`3f60d106`）：

- `DomMutationScope` 的公開函式接 `&NodeHandle` 或 `&DocumentHandle` 的：61 個（`crates/dom/src/scope.rs`）；`dom` 的公開函式簽名裡有 `NodeHandle` 的：100 個。
- `dom_api` 的函式簽名裡有 `NodeHandle` 的：313 個；`NodeHandle` 出現的次數：`dom` 349、`dom_api` 1268、`html` 70、`renderer` 64、`dom_wasm` 7、`dom_js` 3。
- `html` 的 sink：`type Handle = NodeHandle`（`crates/html/src/sink.rs`）；fragment parse 每個元素 clone handle 約四次、root 約一次（`html` 的 `a_fragment_parse_roots_each_node_about_once`）。

## 保守掃描

為什麼不照 Blink 用裸指標加保守掃描 stack：

- **型別限制不了位置**：Rust 沒有「只能在 stack 上」的型別性質；任何值都能進 `Box`、`Vec`、`Rc`，被 closure 捕捉再裝成 `Box<dyn Fn>`，或在 `async fn` 裡跨過 `.await`（future 被 box 或 spawn 就在 heap 上）。auto trait（`Send`、`Sync`、`Unpin`）管不到位置。Blink 的 `STACK_ALLOCATED` 和「heap 上只能用 `Member`」靠 clang plugin（blink_gc_plugin）檢查；Rust 對應的是自訂的 lint，看不穿 generic、closure 和 future，漏掉就是 use-after-free。
- **`dom_wasm`**：fork 的 standalone heap 不掃 stack（`kNoConservativeStackScan`）；wasmtime 的 async 呼叫跑在自己的 fiber stack 上，暫停中的 fiber 的 stack 要另外登記給 cppgc，cppgc 沒有公開這種 API（V8 為 wasm 的 stack switching 有內部的支援）。
- **lifetime 能做到同樣的事，而且可以證明**：保守掃描要解決的是「GC 時 stack 上的裸指標要當 root」，lifetime 讓 GC 時根本沒有活著的裸指標。

## 參考資料

- gc-arena：<https://github.com/kyren/gc-arena>
- cppgc 的 `NoGarbageCollectionScope`：`include/cppgc/heap-consistency.h`
- Blink 的 GC plugin：Chromium 的 `tools/clang/blink_gc_plugin/`
