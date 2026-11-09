# RFC 0005 的細節

`README.md` 的補充：2026-11-01 重新打開時的研究，wasmtime 是 50.0.1（kun 釘的是 49.0.2）。cppgc 版（2026-10-04）的研究在 RFC 0001 的 `rfcs/0001-wasm-scripts/wasmtime-questions.md`（Q13–Q17），這裡只寫和它不同、或之後查到的。

## wasmtime 50 的現況

- **collector 還是封閉的**：`Config::collector()` 只能選 `Auto`（現在是 copying）、`DeferredReferenceCounting`、`Null`、`Copying`；四塊的介面（`GcRuntime` / `GcHeap`、`GcTypeLayouts`、`GcCompiler`、`Collector`）都還是 crate 內部的。上游的 issue 裡沒有找到讓 embedder 提供 collector 的計畫。
- **`GcHeap` 的形狀沒變**：`attach` / `detach` 帶一塊 `vm::Memory`、`alloc_raw` 回傳需要長大的大小、`gc(roots, …)` 回傳一段一段做的 `GarbageCollection`（`collect_increment` 之間 mutator 不跑），沒有 concurrent 的介面。
- **上游的 GC 還在快速變動**：`runtime/vm/gc` 2026-06 到 10 月有十幾個 commit，其中幾個改到介面這一層：改用 `LinearMemory` trait（#14885，2026-10-21）、GC header 和 array 長度的 alias region（#14353）、stack switching 和 GC 的整合（#14140）、從 GC heap 讀出的 ID 加上更多縱深防禦的檢查（#14450，2026-10-01）。
- **版本**：2026-10-20 出了 50.0.0，10-29 同時出了 50.0.1、49.0.5、48.0.8、36.0.20（安全修正）。

## 型別的生命週期

wasmtime 的 engine 把所有 module 的型別登記在 `TypeRegistry`（`runtime/type_registry.rs`），以 recursion group 為單位、用 reference count 管：module 的 `TypeCollection` drop 時取消登記，`VMSharedTypeIndex` 是 slab 的 id、之後會重用。GC 物件不各自留住自己的型別：Store 的 `ModuleRegistry` 留住它的 module，host 配置的型別記在 Store 的 `gc_host_alloc_types`，所以 Store 活著時它的物件的型別都在；原版的 GC heap 跟著 Store 一起丟掉，不會讀到取消登記的型別。

和 DOM 共用 heap 之後，Store drop 時它的物件還在文件的 heap 裡（變成垃圾，等下一次 GC），進行中的 marking 的 worklist 也可能還有它們；wasmtime 這時取消登記、重用那個 id，marker 就會照錯的 layout 讀。所以 GC 這一側的型別表要自己留住型別（留一份 `RegisteredType`），到 Store drop 之後第一次完整的 GC 做完才放掉：在現在的 Component Model 下，DOM 和其他 Store 都沒有指向它的物件的邊，那次 GC 之後它的物件都收掉了。DOM 參照變成 GC 參照之後，「Store drop 了」不再表示物件都死了，那時要重新想。

## sandbox 的約定

- **2023 年的 GC RFC**（bytecodealliance/rfcs 的 `accepted/wasm-gc.md`）的目標就是「可以換的 collector」：動機寫長時間跑的程式要分代的 nursery、concurrent 的 mutation 和回收、壓縮；草稿的介面用原始指標和 `unsafe fn`。
- **2024-04 的 #8196**（加 `GcRuntime` 和 `GcCompiler`）定下「indexed heaps」，原文是所有 collector 實作都必須共用的假設：
  1. GC heap 是一段連續的記憶體，GC 物件都配置在裡面；
  2. 參照是 32 位元的 offset，存取時檢查邊界（明確檢查，或靠虛擬記憶體）；
  3. heap 裡的東西一律不可信任，heap 裡不放 native 指標（`externref` 的 host 資料放在 side table）。

  理由：collector 的 bug 最多讓 guest 讀錯 heap 裡的物件，碰不到 host；collector 幾乎不用 `unsafe`，不必維護「整個系統都要成立、包裝不起來」的不變量。
- **現在寫在 `GcHeap` 這個 `unsafe trait` 的 Safety 一節**：index 要檢查邊界，「Every heap is a mini sandbox」，壞掉的 `GcHeap` 永遠不能害到 host。
- **它擋住的 bug**：2026-10-02 的兩個安全公告，CVE-2026-115379（產生的碼裡跨過 `try_call` 的 GC 參照有時沒有登記成 root，callee 觸發 GC 之後參照懸空）和 CVE-2026-115383（component 的驗證放過了 tag 的 import 用子型別，guest 能把型別錯的 payload 寫進 exception 物件），都寫成「GC heap corruption」，評為 medium；公告說 GC heap「is implemented to be resilient to corruption」，最壞是 panic。兩個都不在 collector 裡。
- **它擋不住的**：Cranelift 自己的邊界檢查編錯，例如 2026-04-09 的 CVE-2026-34971（aarch64 上 guest 的記憶體存取編錯，逃出 sandbox，critical）。

## 比較過的做法

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| 甲 | 不 fork | 沒有維護成本 | wasm 物件留在 semi-space 的 copying collector：每次 GC 停下來 copy 全部活的，空著一半的空間；和 DOM 兩個 GC |
| 乙 | fork，kun 的 GC 當 collector，每個 Store 自己一個 heap，照 sandbox 的約定 | 分代加 concurrent；不打破上游的約定，改動大多是新增的檔案 | DOM 和 wasm 之間的循環收不掉；GC 要把 heap 的內容當成不可信任 |
| 丙 | fork，wasm 物件和 DOM 同一個 heap（選了） | 一個 GC；循環收得掉（DOM 參照變成 GC 參照之後）；記憶體一起算 | 打破上游的約定（或照下一節做 sandbox）；GC 由 kun 發起時要向每個 Store 要 root |
| 丁 | fork 只開放介面（`Collector` 讓 embedder 提供） | patch 看起來小 | compiler 那一半要公開 Cranelift 的 IR builder；上游內部一改外面就壞 |

和 Chromium 對照：V8 的 wasm GC 物件在 V8 自己的 heap，Oilpan 只管 DOM，兩邊靠 unified heap 互相追蹤，是乙加上兩邊的 marking 接起來的形狀；丙是 Chromium 沒有走的路。

## sandbox 的取捨

丙可以照樣是 sandbox，邊界是整份文件的 heap（DOM 加上它所有 guest 的 wasm 物件），像 V8 sandbox 一個 isolate 一個 cage。wasmtime 的約定只要求壞的範圍不跑出 heap，不在乎 heap 裡有幾個 Store、有沒有 DOM。對 host 來說 guest 還在 sandbox 裡；放掉的只有同一份文件的 guest 之間、guest 和 DOM 之間的隔離（本來就經過 DOM API 互相影響）。

不可信任的 heap 只對 `unsafe` 的程式碼有威脅：safe Rust 讀到壞掉的值只會表現錯誤或 panic。要守的是 heap 和 Rust 的型別系統交界的地方：
1. heap 裡不放 native 指標：`External<T>` 換成 side table（RFC 0087 已決定的事項 7 的乙），embedder word 只放索引；
2. GC 驗證從 heap 讀出的值（型別編號、flags、forwarding 的位置、array 的長度、頁表的索引），再用來算 cage 外的位置或交給 `unsafe`；
3. heap 裡欄位的型別任何 bit pattern 都合法（`bool`、enum、`char`、`NonZero`、參照不行；`dom_next` 有 `Cell<bool>` 的欄位），由 `derive(GcObject)` 檢查；
4. cage 後面留 guard 區，小物件被當成大的型別讀時不讀出 cage（現在保留兩倍的位址空間，對齊之後 cage 後面不一定有剩）；
5. 一個 heap 只放一份文件，至少只放同一個 origin。

前四條成立時，解引用不需要比對型別：型別混淆最多讀到垃圾值。執行時的成本只有 `External` 每次存取多一次讀取（新的 DOM 熱的資料在 heap 裡，只有冷的欄位在 `External` 裡）和 GC 的路上的檢查；欄位型別是設計的限制。

GC 的驗證回答的是「GC 有沒有 bug」，sandbox 回答的是「別的地方有 bug、heap 已經被寫壞時 host 安不安全」。寫 heap 的有三方：GC 自己、host 的 DOM 程式碼（經過 safe API）、wasmtime 產生的碼（`struct.set` 是一條 store 指令，不經過 GC 的 Rust 程式碼）。GC 的約定（root 完整、邊合法、barrier 都有）由第三方的產生器保證：wasmtime 的驗證器、GC 的 lowering、Cranelift 的 stack map 和 fork 裡的 `GcCompiler`。這和信任 rustc、LLVM 是同一種選擇，差別是這些元件的輸入由攻擊者決定，而且 GC 的部分還在快速變動。

2026-11-01 定的是不做這一節的 sandbox（`README.md` 的已決定的事項 3）。

## GC 要補的能力

kun 的 GC 現在的形狀（當時 kun 的 `gc` crate，之後的 elise）和 wasm GC 要的差在：
- **型別**：型別表照 cppgc 的 `GCInfoTable`，每個型別的 layout 在 derive 產生的 static 裡，第一次配置時拿到編號，最多 4096 個；wasm 的型別要在執行時登記 layout 和邊的 offset 清單，數量也可能超過上限。
- **邊**：`Member` 是 `AtomicU32` 的 offset，0 是 null；wasm 的 `anyref`、`eqref` 欄位可能放著 `i31ref`（最低位元 1），marker 和 scavenge 要跳過。格子 16 byte 對齊，offset 的最低位元本來就是 0。
- **配置**：現在經過 `Mutation::alloc` 的 safe API 和 derive 產生的 `Init`；wasmtime 的 `alloc_raw` 只給 header 和 layout，要照型別編號配置。
- **GC 的時機**：現在只在 `mutate` 之間的安全點；wasmtime 在 wasm 碼的中途、配置失敗時要 GC，root 是外面給的 slot（stack map 裡的 32 位元值），nursery 搬動物件之後要寫回新的位置。
- **barrier**：Rust 這一側的寫入 barrier 已經是「inline 的判斷，記錄 out of line」（分代的讀兩次 young 的表、SATB 讀一個旗標），`GcCompiler` 照同樣的形狀產生。wasm GC 沒有 weak 參照，不需要讀取的 barrier。
