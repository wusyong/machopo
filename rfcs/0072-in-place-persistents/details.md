# RFC 0072 的細節

## 量測

2026-10-22，Windows 11，Intel Core i9-13900K，kun `f06bce27`（`perf` profile），`bench.py speedometer --sample`（samply 經過 ETW，以 CPU 時間計、連 kernel 的 frame），差最多的 8 個 suite（TodoMVC-jQuery、Perf-Dashboard、Charts-chartjs、Editor-TipTap、NewsSite-Next、NewsSite-Nuxt、ES6-Webpack-Complex-DOM、React-Stockcharts-SVG）各 5 個 iteration。量測的紀錄在 RFC 0012 的 `progress.md`，分析在 2026-10-23 那一版的 `docs/performance-gaps.md`（2026-11-06 整份改寫之前）裡「Windows 上」那一節。

**Windows 的 heap 從哪裡來**：stack 上有 `RtlpLowFragHeapAllocFromContext`、`RtlFreeHeap` 這類 NT heap 的 frame 的樣本，照它上面第一個不是 CRT 的 `malloc`、`free`、`operator new` 的呼叫者分，每個 iteration、8 個 suite 合計：

| 來源 | ms |
| --- | ---: |
| `cppgc__WeakPersistent__CONSTRUCT` | 13.1 |
| `cppgc__Persistent__CONSTRUCT` | 7.8 |
| `free_base`（大多是 `Rc::drop_slow<RootCell<…>>`，root 被丟掉時刪 `Persistent`） | 5.9 |
| Skia（`SkPathData::MakeTransform`、`SkTDStorage::insert`、AA clip 的 blitter……） | 5.4 |
| V8 和 cppgc 自己的（Scavenger 的 worklist、`TracedHandles`、Maglev） | 3.9 |
| 其他 | 0.5 |
| 合計 | 36.8 |

`free_base` 那一列裡，root 的約 3.3 ms，其餘是 Skia。persistent 的建立和刪除合計約 24 ms，約三分之二。

**依 suite**：jQuery 的 persistent 12.2 ms（`WeakPersistent` 11.8，全部來自 `DomMutationScope::create_node`：fast path 的 innerHTML 6.4、`create_element_of` 5.4）；Charts-chartjs 3.4（`dom_js::typed` 的 `this`：每次呼叫 canvas 的繪圖成員都 root context）；ES6-Webpack 2.6；Perf-Dashboard 1.8（inline style 的 `Handle::new`、canvas context）；NewsSite-Next 0.8。

**強的 `Persistent` 從哪裡建**（`cppgc__Persistent__CONSTRUCT` 的呼叫者，8 個 suite 合計 8.8 ms，包括 region 的 node 和 NT heap 以外的部分）：Charts-chartjs 的 canvas context 的 `this` 2.2、Perf-Dashboard 的 inline style 的 handle 1.0 和 canvas context 0.8、ES6-Webpack 的 `dom_js::properties::call` 0.9、Charts-chartjs 的 `call_member` 0.6、NewsSite-Nuxt 的 inline style 0.6。這些都是沒有 spare pool 的型別（pool 只有節點和 collection）。

**慢的是次數，不是每一次**：RFC 0058 數過 jQuery 跑 10 個 iteration 建約 310 萬個節點（當時的版本），一個 iteration 約 31 萬個；`WeakPersistent` 的 NT heap 時間 11.0 ms 除下來，每次約 35 ns，是 LFH 正常的成本。所以換一個 heap 的設定（Segment Heap）不會有大改變，要省的是那次配置本身。

**其他平台**：Linux 上 RFC 0058 的取樣，id 表的 `WeakPersistent` 佔 jQuery 的 2.19% cycle；RFC 0062 的 innerHTML 取樣，佔每個節點的 cycle 的 6%（連同 persistent region 的 node 和 `malloc`）。

## cppgc 的 persistent 的版面

（V8 152 的 `include/cppgc/persistent.h`、`include/cppgc/internal/persistent-node.h`）

- `PersistentBase` 有兩個欄位：`raw_`（指向的物件）和 `node_`（persistent region 裡的 node）。`BasicPersistent` 另外繼承 location policy：`CPPGC_SUPPORTS_OBJECT_NAMES` 時是 `KeepLocationPolicy`，多一個 `SourceLocation`，否則是空的 `IgnoreLocationPolicy`。V8 的 `cppgc_enable_object_names` 在桌面平台的非 component build 預設開著（`gni/v8.gni`：Windows 的 component build、Android、Fuchsia 以外），和 debug 或 release 無關，所以 kun 的 persistent 是 32 bytes（x64：`raw_`、`node_` 各 8，`SourceLocation` 16；2026-10-22 在 Windows 上編的 release V8 的 `cppgc__WeakPersistent_SIZE`）。草稿時以為 release 沒有 `SourceLocation`、只有 16 bytes，是看錯了選 policy 的條件。大小從 build 產生的常數讀。
- 建立時從 heap 的 `PersistentRegion` 拿一個 node，`InitializeAsUsedNode(owner, trace)` 記下 persistent 自己的位址（`owner_`）和 trace 的 callback。GC 掃描 root 時對每個用中的 node 呼叫 `trace_(visitor, owner_)`，從 `owner_` 讀 `raw_`；弱的 persistent 在物件死掉時，經過同一個 `owner_` 把 `raw_` 設成 null。
- C++ 的 move constructor 和 move assignment 呼叫 `node_->UpdateOwner(this)` 把 node 指到新的位址。Rust 的 move 是複製 bytes，不經過它：搬過的 persistent 的 node 仍指著舊的位址，GC 會從那裡讀 `raw_`（use after free 或讀錯物件）。這就是 fork 現在用 `new` 的理由，也是這份的 slot 有值時不能搬動的理由。
- 空的 persistent 不拿 node：建構子只有在指標有效（不是 null、不是 sentinel）時才向 region 要 node，`Clear()` 和 GC 清掉弱的 persistent（`ClearFromGC`）時把 node 還回去、`node_` 設成 null。所以空的 persistent 沒有記著任何位址，搬動它的 bytes 不會讓 GC 讀錯地方；只有「有值」的那段期間不能搬。
- 解構子 `~BasicPersistent()` 呼叫 `Clear()`：有 node 就還給 region。
- region 的 node 是 256 個一塊（`PersistentNodeSlots`，`std::vector<std::unique_ptr<…>>` 只往後接），空出來的放進 free list。原地建之後，建一個 persistent 剩下的就是從這個 free list 拿一個 node。

**GC 什麼時候碰 `raw_`**：persistent 是 root，在 GC 的 pause 裡（mutator 的 thread）掃描；concurrent marker 不讀 persistent。所以 Rust 的程式只要在呼叫可能觸發 GC 的 V8 函式時，沒有拿著指向 slot 的 `&`（`UnsafeCell` 裡的 bytes 不會被 Rust 的 reference 涵蓋），就和 C++ 的寫入沒有衝突。

## id 表的記憶體

每一項（release build，x64）：

| | 現在 | 改了之後 |
| --- | --- | --- |
| hash map 的一項 | `NodeId`（4）+ 指標（8），對齊成 16 bytes | `NodeId`（4）+ 格子的編號（4）= 8 bytes |
| persistent 本身 | C++ heap 上的 32 bytes，加上 heap 自己的管理資料 | slab 裡的 32 bytes |
| region 的 node | 16 bytes | 16 bytes |

記憶體的最高點：現在的 hash map 在 `prune` 時用 `retain`，不縮小容量；改了之後 chunk 也不釋放（第 3 條），和 cppgc 的 `PersistentRegion` 一樣（`RefillFreeList` 只往後接，`nodes_` 直到 region 解構才釋放）。V8 的 handle 區塊（`kHandleBlockSize = KB - 2`，一頁）用完之後只留一塊備用（`HandleScopeImplementer::spare_`）。

## fork 的 slot

fork 在 `src/cppgc/slot.rs` 裡已經有三種 slot，這份加第四種。前三種是為了 concurrent marking：上游的 `Member`、`WeakMember`、`TracedReference` 寫入要 `&mut self`、trace 用 `&self`，bytes 是只有 1 byte 對齊的 `[u8; N]`；marker thread trace 的同時 main thread 可能在寫，Rust 裡同時有 `&` 和 `&mut` 指著同一塊 bytes 是 undefined behavior（即使 C++ 那邊都是 atomic），C++ 用 atomic 讀寫也需要對齊。slot 把 bytes 放進 `UnsafeCell`、照大小對齊，每次寫入和 trace 都用裸指標交給 C++。第四種的理由不同：persistent 只在 GC 的 pause 裡由 main thread 讀，marker thread 不碰它；它是為了不必另外配置。slot 的大小都和 C++ 物件一樣（`UnsafeCell` 不佔空間），不多佔記憶體。

| | `MemberSlot` | `WeakMemberSlot` | `TracedSlot` | `WeakPersistentSlot`（這份） |
| --- | --- | --- | --- | --- |
| 為什麼需要 slot | concurrent marking 的 aliasing 和對齊 | 同左；GC 的 weak processing 也會寫它 | 同左；kun 只能經過 `&` 碰到 GC 物件，拿不到上游 `reset` 要的 `&mut` | 省掉上游每個一次的 `new`；GC 清掉弱的 persistent 時會寫 `raw_`，所以仍放進 `UnsafeCell` |
| 包的 C++ 型別 | `cppgc::Member` | `cppgc::WeakMember` | `v8::TracedReference` | `cppgc::WeakPersistent` |
| 指向 | GC 物件（強） | GC 物件（弱） | JS 的值 | GC 物件（弱） |
| 放在哪 | GC 物件的欄位 | GC 物件的欄位 | GC 物件的欄位 | heap 外的 Rust 記憶體（id 表的 slab） |
| 怎麼被 GC 看到 | 擁有它的物件的 `trace` | 同左 | 同左 | 自己是 root，region 的 node 記著它的位址 |
| 大小 | 4 bytes（壓縮的指標） | 4 bytes | 一個指標 | 32 bytes（x64，連同 `SourceLocation`） |
| 能不能搬 | 任何時候 | 放在物件裡就不搬 | 空的能搬，有值之後不能 | 空的能搬，有值之後不能 |
| 「不能搬」怎麼保證 | 不需要 | cppgc 不搬物件 | `unsafe fn set` 的條件：已經在最後的位置（GC 物件的欄位） | `Pin`（第 2 條） |
| 方法為什麼 `unsafe` | thread 的條件 | 同左 | thread 的條件和「在最後的位置」 | 只有 `get`（用的期間沒有 GC）；thread 由型別擋（不是 `Sync`） |
| `Sync` | 是 | 是 | 是 | 不是 |
| kun 的包裝 | `Edge`、`TaggedEdge`（`gc.rs`），`EdgeChunks<Edge>`（`chunks.rs`） | `WeakEdge`，`WeakSet` | `JsEdge`（`v8_engine.rs`） | `NodeIdMap` 的 slab（私有） |

`TracedSlot` 和這份的 slot 限制相同，fork 用 `unsafe` 的條件守它；那裡的「最後的位置」是 GC 物件的欄位，結構上就固定。這份的 slot 在 heap 外，搬動它的寫法到處都是，所以改由 `Pin` 擋住（第 2 條）。

## 其他做法

| | 做法 | 省到的（Windows，8 個 suite，每個 iteration） | 成本 | Chromium |
| --- | --- | --- | --- | --- |
| **A**（第 1 條：id 表用它） | fork 原地建 persistent，kun 放在固定的記憶體（id 表的 slab） | id 表約 14 ms，其他平台也少一次配置 | fork 的 C++、一次 release；「有值時不搬動」的不變式 | Blink 的 `Persistent` 是嵌在物件裡的值 |
| B | C++ 那一側的 `malloc`、`operator new` 換成 mimalloc（或 PartitionAlloc） | 最多 36.8 ms（連 Skia、V8） | Windows 上 mimalloc 不能靜態蓋掉 ucrt 的 `malloc`（要 `mimalloc-redirect.dll`、動態連結）；只蓋 `operator new` 要先確認 rusty_v8 的靜態庫（帶著 Chromium 的 libc++）沒有自己的定義，否則 link 時撞名；記憶體的行為跟著變（`docs/limitations.md` 裡 C++ 那一側的那條）；每個 persistent 仍各配置一次 | allocator shim 把 `malloc` 和 `new` 都導到 PartitionAlloc |
| C | id 表改成 GC heap 上的 weak map（`WeakMember`，GC 清掉死的項目） | 約 13 ms（只有 id 表） | GC 的設計改變：`node_by_id`、prune 和 soundness 的論證重寫；每次 GC 多處理整張表；RFC 0058 的 3c 沒開始 | `DOMNodeIds` 的 `WeakIdentifierMap`（GC heap 上的 `HeapHashMap`），只給被要求的節點 |
| **D**（第 1 條：`Root` 用它） | 所有型別都有 root 的 spare pool | 約 10 ms（只有 root） | 不用改 fork；pool 照型別分，要一個 `TypeId` 對到 pool 的表；pool 撐住的記憶體變多 | Blink 的 handle 不是 persistent，沒有對應 |
| E | Windows 的 Segment Heap（manifest 的 `heapType`） | 不確定 | 主要省記憶體；LFH 已經是快速路徑，慢的是次數 | Chrome 在 Windows 上用 PartitionAlloc |
| F | id 表延後登記（RFC 0058 的 2b） | 幾乎沒有 | jQuery 的節點 99.8% 都被拿過 id | `DOMNodeIds` 用到才編號 |
| G | kun 自己的強 root 表在 GC heap 上（RFC 0058 的 3b） | root 的部分 | 做過：marking 中寫入當場標記，floating garbage 讓 jQuery 慢約 9%，收在 `git stash` | — |

**為什麼 A 沒有 G 的 floating garbage**：G 的表在 GC heap 上，寫入要經過 write barrier，marking 中新加的 root 當場被標記，之後放掉也要等下一次 GC 才回收。A 的 persistent 仍是 cppgc 的 persistent：建立不經過 barrier，root 在最後的 pause 重新掃描（`soundness.md` 的 C2），放掉的 persistent 在那之前就不在 region 裡了，和現在一樣。

**A 和 D 各用在一半**（第 1 條）：A 去掉 C++ 的配置，D 連 region 的 node 都不拿（只 `Assign`）。root 是「同一個型別一直建、一直丟」，D 比較省，`Root` 也就不必改成原地建；id 表的項目要活到節點被回收、數量跟著文件長大，pool 幫不上，用 A。

## 範圍的選項

每一處建 persistent 的成本（Windows，8 個 suite，每個 iteration；照 stack 上的 kun 函式分）：

| 地方 | 成本（ms） | 怎麼用 | 要原地建得先 |
| --- | ---: | --- | --- |
| id 表（`NodeIdMap`） | 14.2 | 每建一個節點一個 `WeakPersistent` | 改成 slab（本來就要改） |
| `Root`（沒有 pool 的型別） | 約 10（建 8.6、刪 1.4） | canvas context 的 `this`（`dom_js::typed`）、inline style 的 handle、indexed getter 的 receiver；`find_map<ObjectTag>` 底下的 5 ms 也是它（handle 的建立被 inline 進去） | `Root` 改拿 `Pin<Rc<…>>` |
| `EventListenerHandle` | 0 | 分派事件時每個 listener 一個，放在 `Vec` 裡回傳 | 每個包一層 `Box` |
| `StoreTable`（`foreign.rs`） | 0 | wasm 的 guest 的 holder 和它的 root | 項目改成 `Box` 或 slab |
| 錨點 | 0 | 一個 isolate 一個，在 `Rc<Roots>` 裡 | 不用改 |

| | 範圍 | 結果 |
| --- | --- | --- |
| R1 | id 表和 `Root` 都原地建 | 不選：`Root` 仍要配置 `Rc`、拿 region 的 node |
| R2 | 全部換 | 不選：後三處沒有收益，多幾處 `Box`、`unsafe` 和論證；記在「之後的可能」 |
| R3 | 只做 id 表 | 不選：`Root` 的約 10 ms 留著 |
| **R4** | id 表原地建，`Root` 用所有型別的 pool | 選（第 1 條） |

## Blink 的對照

- **`Persistent<T>`**（`third_party/blink/renderer/platform/heap/persistent.h`，cppgc 的 `cppgc::Persistent` 的別名）：Blink 的 heap 外物件直接把它當欄位（例如 `Persistent<Document>`），和物件一起配置、一起釋放；C++ 的 move 經過 `UpdateOwner`。沒有「每個 persistent 一次 `new`」這回事。
- **節點的 id**：`DOMNodeIds::IdForNode` 第一次被要求時才給 id，存在 `WeakIdentifierMap<Node>`（GC heap 上、兩個方向的 `HeapHashMap`），被回收的節點由 GC 的 weak processing 清掉。Blink 的 layout 和 paint 直接拿 `Node*`（同一個 heap），不需要 id；kun 的 pipeline 在 heap 外，用 id 指節點，所以每個要畫的節點都要 id（F）。
- **receiver 的 root**：Blink 的 binding 從 V8 的 receiver 拿 `ScriptWrappable*`，靠保守的 stack 掃描，不建 root。kun 會改狀態的成員 root `this`（RFC 0067），canvas 的繪圖成員因此每次呼叫建一個 root（「之後的可能」）。
