# RFC 0053 的細節

## kun 的記憶體放在哪裡

一次 DOM 操作（`setAttribute`、`innerHTML`、`appendChild`）碰到的記憶體，和換 allocator 管得到哪些：

| 記憶體 | 放在哪 | 誰配置 | 只換 Rust 的（B） | 整個 process 換（C） |
| --- | --- | --- | --- | --- |
| JS 的物件（wrapper、字串、陣列） | V8 的 heap | V8 自己的 page | 不影響 | 不影響 |
| DOM 節點本身（`dom::Node` 的 parent、子節點、wrapper 這些 edge） | cppgc 的 heap | cppgc 自己的 page | 不影響 | 不影響 |
| 節點裡的資料：`NodeState` 的 `attributes: Vec<Attribute>`、`id: Option<Box<str>>`、`class_names: Box<[Box<str>]>`、文字、computed style（`crates/dom/src/node.rs`） | Rust 的 heap | Rust 的 global allocator | 換 | 換 |
| Rust 這一側的暫存：V8 字串轉成的 Rust 字串、html5ever 和 tendril 的 buffer、style 的 `Vec` 和 `Arc`、CSS 的 parse、root cell | Rust 的 heap | Rust 的 global allocator | 換 | 換 |
| `binding.cc` 替 Rust 建的 `cppgc::Persistent`（`cppgc__Persistent__CONSTRUCT` 的 `new`） | C++ 的 heap | libc++ 的 `operator new` → `malloc` | 不影響 | 換 |
| V8 內部的 off-heap 資料、Skia（`sk_malloc`） | C++ 的 heap | `malloc` | 不影響 | 換 |

`Node` 這個 struct 本身在 cppgc 的物件裡（`RustObj`），它的欄位裡的 `Vec`、`Box` 指向的 buffer 在 Rust 的 heap。cppgc 回收節點時在原地跑 Rust 的 drop，那些 buffer 經過 Rust 的 global allocator 釋放。

`Persistent` 的 `new` 在 RFC 0051 第 3 段量到佔建一個 `Persistent` 的成本約七成；之後的錨點 pool 和 `Rc` 的 root cell pool（0051 第 8、10 條）讓它們重複使用，現在應該很少。

## 跨 allocator 的釋放

Rust 用 mimalloc、C++ 用平台的 allocator 時，一邊配置、另一邊釋放就是 heap corruption。2026-10-17 在 `crates/` 裡找過（`libc::free`、`from_raw`、`into_raw`、Skia 的 release proc、rusty_v8 的 external 和 backing store）：

- `raster` 交給 Skia 的圖片資料是 `sk::Data::new_copy`（Skia 自己配置、複製一份），不是把 Rust 的 buffer 交給 Skia 釋放。
- `dom_js` 的 `ArrayBuffer` 用 rusty_v8 的 `new_backing_store_from_vec`：V8 不用了的時候呼叫 rusty_v8 的 Rust deleter，由 Rust 釋放。
- 沒有 `CString::into_raw` 交給 C、`libc::free` 這類的寫法。
- 第三方的 crate（skia-safe、rusty_v8、wgpu、reqwest）自己負責它們的 FFI；skia-safe 和 rusty_v8 的 deleter 都在 Rust 這一側。GPU 驅動配置的記憶體由驅動自己釋放，不經過 Rust。

實作時再用 debug build 跑一次 `cargo test -p shell` 和 `test.py reftest`：mimalloc 收到不是它的指標時通常會當場 crash，不會默默地壞掉。

## 各家 allocator

**平台預設**（Rust 的 `std::alloc::System`）：

| 平台 | 預設 | 說明 |
| --- | --- | --- |
| Linux | glibc 的 `malloc`（ptmalloc2） | 每個 thread 一個 tcache，再來是 arena（有 lock） |
| macOS | libmalloc 的 default zone | 小物件走 nano 或 magazine 的配置器 |
| Windows | process heap 的 `HeapAlloc` | NT heap，小物件走 Low Fragmentation Heap。Segment Heap 一般程式要在 manifest 裡宣告才用。mimalloc 雖然出自 Microsoft，不是 Windows 的預設 |

**mimalloc**（Microsoft Research）：每個 thread 有自己的 heap，同一個 size class 的物件在同一頁，每一頁有自己的 free list（free list sharding）。配置和同一個 thread 的釋放只要幾個指令、不用 lock；別的 thread 釋放的物件放進另一條 list，用 atomic 處理。同樣大小的物件放在一起，locality 比較好。Rust 的 `mimalloc` crate：

- 0.1.52（2026-05-22），repo 是 `purpleprotocol/mimalloc_rust`。
- 預設編 upstream 的 mimalloc v3；`v2` feature 換成 v2。
- 其他 feature：`secure`（guard page、隨機化、加密的 free list，效能約差一成）、`override`（換掉整個 process 的 `malloc`）、`extended`（`mi_is_in_heap_region` 這類 API）、`local_dynamic_tls`、`win_direct_tls`、`no_thp`、`debug`。
- 授權 MIT，和 kun 的 AGPL-3.0-only 相容。

**jemalloc**：碎片控制和長時間跑的記憶體是強項，工具（heap profiling、統計）完整。Rust 的 `tikv-jemallocator` 用 autoconf 的 configure 編 jemalloc，不支援 `*-windows-msvc`。rustc 只在 Linux、macOS 的發佈版開 jemalloc（建置設定的 `rust.jemalloc`），Windows 版用系統的。上游（Meta）2025 年宣布停止開發、封存了 repo，之後的情況不確定；`tikv-jemallocator` 還有人維護。

**snmalloc**（Microsoft Research）：一個 thread 配置、另一個 thread 釋放時，用傳訊息的方式還回去，這種情況特別快。`snmalloc-rs` 支援三個平台。kun 的配置大多在 renderer 的 main thread 上配置、釋放，這個強項用不太到。

**tcmalloc**（新版，Google）：每個 CPU 一份 cache（靠 Linux 的 rseq）、hugepage 的記憶體效率好；實際上只有 Linux，用 Bazel 建置。

**PartitionAlloc**（Chrome）：照用途分開的 partition，MiraclePtr（BackupRefPtr）這類安全機制。在 Chromium 的 GN 建置裡，要接進 Rust 的建置不容易。

**跑分**：最常被引用的 mimalloc-bench 是 mimalloc 的作者做的，有偏差。大致上 mimalloc、tcmalloc、snmalloc、jemalloc 都在前段，彼此的差距看 benchmark 從幾 % 到幾十 % 不等；glibc 在多 thread、小物件多的測試裡常常在最慢的一群。對 kun 的效果只能量。

**預期的效果**（推估，還沒量）：最多省下的是「Rust 呼叫的配置和釋放的時間」乘上 mimalloc 省下的比例。一般的經驗是配置成本省一半上下。如果 jQuery 現在 Rust 的配置佔 6% 到 8%，大約是 3% 到 5%；其他 suite 配置比較少，效果更小。複製（`memcpy`）不受影響。

## Windows 上的基準

2026-10-17，`ci.py speedometer`（kun `e3ae99d2`，也就是 RFC 0051 的 2c；`perf` profile；Chrome for Testing 152.0.7977.82）；Windows 11（10.0.26200），Intel 64 Family 6 Model 183，32 核。每個 suite 開新的瀏覽器、10 個 iteration，suite 總時間的中位數（ms，95% 區間）：

| suite | kun | Chrome | kun / Chrome |
| --- | --- | --- | --- |
| TodoMVC-JavaScript-ES5 | 103.5（100.5–106.1） | 25.0 | 4.13 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 182.3（180.3–183.8） | 24.5 | 7.44 |
| TodoMVC-WebComponents | 64.2（63.5–65.5） | 12.1 | 5.28 |
| TodoMVC-React-Complex-DOM | 78.5（76.6–80.9） | 20.0 | 3.94 |
| TodoMVC-React-Redux | 48.1（47.1–51.3） | 21.6 | 2.23 |
| TodoMVC-Backbone | 56.1（55.6–57.5） | 19.7 | 2.84 |
| TodoMVC-Angular-Complex-DOM | 68.7（66.3–72.0） | 18.1 | 3.79 |
| TodoMVC-Vue | 44.4（43.8–46.2） | 13.8 | 3.22 |
| TodoMVC-jQuery | 610.0（594.3–618.3） | 71.2 | 8.57 |
| TodoMVC-Preact-Complex-DOM | 47.7（46.1–49.2） | 7.9 | 6.04 |
| TodoMVC-Svelte-Complex-DOM | 47.6（45.9–49.3） | 7.4 | 6.43 |
| TodoMVC-Lit-Complex-DOM | 51.2（50.3–53.3） | 10.6 | 4.83 |
| NewsSite-Next | 256.1（252.1–257.2） | 62.6 | 4.09 |
| NewsSite-Nuxt | 206.2（203.5–207.8） | 42.4 | 4.86 |
| Editor-CodeMirror | 無效（kun 的第 0 個 iteration 少兩個 `div`） | | |
| Editor-TipTap | 137.5（135.3–141.9） | 41.1 | 3.35 |
| Charts-observable-plot | 60.4（59.5–64.1） | 28.7 | 2.11 |
| Charts-chartjs | 198.0（197.1–202.0） | 24.2 | 8.16 |
| React-Stockcharts-SVG | 117.5（113.0–125.8） | 51.8 | 2.27 |
| Perf-Dashboard | 162.2（161.1–167.6） | 34.0 | 4.76 |

區間大多在中位數的 ±3% 內，jQuery、Stockcharts 寬一些。這份是 0051 的 2c 時的程式，第 2 段比較時用當時的 `main` 重新量，不拿這份當對照。

## Linux 上拆分取樣的方法

`alloc_split.py`（這個資料夾裡，不合併進 `scripts/`）讀 `ci.py speedometer --sample` 寫出的 `<suite>-kun.folded`（cycle）和 `<suite>-kun.instructions.folded`。leaf 是 glibc 的配置函式的樣本，從 leaf 往上找第一個呼叫者，分成五類：

| 類別 | 怎麼判斷 | B 管得到 |
| --- | --- | --- |
| rust | 經過 `__rust_alloc`、`alloc::raw_vec` 這類 shim | 是 |
| rust? | 沒看到 shim（被 inline，或 frame pointer 跳過了），呼叫者是 Rust 的函式 | 是 |
| binding | `cppgc__*`、`v8__*`：`binding.cc` 替 Rust 做的 `new` | 否 |
| c++ | `operator new`、`sk_malloc`，或 V8、cppgc、Skia 的函式 | 否 |
| unknown | JS 的 frame、`[unknown]` | — |

`memcpy`、`memmove` 這類複製另外算（換 allocator 不影響）。輸出每個 suite 各類佔計時區間的 %、20 個 suite 合計，和每一類最多的呼叫者。2026-10-17 用假的 folded 檔驗過分類；還沒拿真的樣本跑過（樣本在 Linux 那台）。

```sh
python3 rfcs/0053-allocator/alloc_split.py target/speedometer-results/sample > alloc-split.md
```
