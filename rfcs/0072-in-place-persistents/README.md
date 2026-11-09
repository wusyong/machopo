# RFC 0072：原地建的 persistent：id 表和 root 不再各自配置

- 開始日期：2026-10-22
- 狀態：✅ 已完成：2026-10-22（Windows 11）
- 進度和驗收：`progress.md`
- 細節：`details.md`（Windows 上的取樣、cppgc 的 persistent 的版面、其他做法的比較、Blink 的對照）

## 摘要

rusty_v8 的 fork 每建一個 `cppgc::Persistent` 或 `WeakPersistent`，都在 C++ 裡 `new` 一個，放掉時 `delete`。原因是 persistent region 的 node 記著 persistent 自己的位址，而 Rust 的 move 只是複製 bytes，C++ 不知道（RFC 0051 的第 5 條因此把建立和刪除留在 C++）。kun 在兩個熱的地方大量建 persistent：每建一個節點，id 表（`NodeIdMap`）就多一個 `WeakPersistent`；沒有 spare pool 的型別的 `Root`（canvas context 這類 receiver、inline style 的 handle）每次都建一個 `Persistent`。這份分兩邊處理（第 1 條）：fork 提供原地建的 `WeakPersistentSlot<T>`（建在呼叫的人給的、不搬動的記憶體裡，和 fork 已有的 `MemberSlot` 同一套 `construct_in_place`），id 表改成一塊塊固定的 slab，每格一個 slot，空格子從編號最小的發；`Root` 的 spare pool 從節點和 collection 擴大到所有型別，暖機之後建 root 只是把留著的 `Persistent` 改指到新的物件。persistent 的語意不變，仍是 cppgc 的 persistent region 和它的 root 掃描。Windows 上 8 個 suite 的 NT heap 時間從 36.8 降到 19.3 ms／iteration，persistent 造成的部分幾乎歸零（`progress.md`）。

## 動機

- **每個節點一次 C++ 的配置**：Windows 上（2026-10-22，`bench.py speedometer --sample`，`details.md` 的「量測」），8 個差最多的 suite 每個 iteration 在 Windows 的 NT heap 裡共 36.8 ms，約三分之二來自 persistent 的建立和刪除：id 表的 `WeakPersistent` 14.2 ms（全部來自 `create_node`），`Root` 的 `Persistent` 約 10 ms（建 8.6、刪 1.4）。TodoMVC-jQuery 一個 suite 就 12.2 ms，是它和 Chrome 差距（+124 ms）的一成。其他平台也付這次配置，只是 `malloc` 便宜一些：Linux 上 RFC 0058 量過 id 表的 `WeakPersistent` 佔 jQuery 的 2.19% cycle，RFC 0062 量到它佔 innerHTML 每個節點的 cycle 的 6%。
- **以前的路都走過了**：
  - 「少建」：id 表延後登記（RFC 0058 的 2b）不做，jQuery 建的節點 99.8% 都被拿過 id；`Root` 的 spare pool 涵蓋節點（RFC 0051）和 collection（RFC 0060），但那時其他型別的 root 少，現在 canvas context 每次繪圖呼叫都 root `this`。
  - 「換掉 persistent」：kun 自己的強 root 表放在 GC heap 上（RFC 0058 的 3b），marking 中寫入當場標記，floating garbage 讓 jQuery 慢約 9%，收進 `git stash`；弱的 slot 表（3c）沒開始。
  - 這份不換掉 persistent：GC 看到的還是 cppgc 自己的 persistent，所以沒有 3b 的 floating garbage 問題（`details.md` 的「其他做法」）。

目標：

- `NodeIdMap` 的 persistent 不經過 C++ 的 `new` 和 `delete`：建立是 persistent region 拿一個 node，刪除是還回去。
- 所有型別的 `Root` 在 pool 有留著的 cell 時，不建也不刪 persistent。
- fork 提供原地建的 `WeakPersistentSlot<T>`，型別擋住「有值之後被搬動」。
- persistent 的語意、GC 的行為和 `crates/dom/soundness.md` 的論證（I1、I7、I8）不變，只多一條「slot 有值時不搬動」的論證。

不在範圍內：

- 換掉 C++ 那一側的 allocator（mimalloc 或 PartitionAlloc 接管 `malloc`、`operator new`）：Skia 和 V8 自己的配置（Windows 上約 9 ms）不在這份（`details.md` 的「其他做法」B）。
- id 表的結構（GC heap 上的 weak map、照 Blink 的 `DOMNodeIds`）和 kun 自己的 root 表：見上面的 RFC 0058。
- 其他建 persistent 的地方：event listener 的 `EventListenerHandle`、`StoreTable` 的 holder 和 root、錨點，取樣裡看不到它們，照舊用 fork 原本的 `Persistent`、`WeakPersistent`（第 1 條，「之後的可能」）。
- 原地建的強 persistent（`PersistentSlot`）：這份沒有地方用它，等有需要再加。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

網頁看不出差別。改 `dom` 的人看到的是：

- `NodeIdMap` 的每一項是一塊 slab 裡的一格 `WeakPersistentSlot<Node>`，map 存的是格子的編號；節點被回收時 GC 清空那一格，prune 把它放回 free list，新節點從編號最小的空格子拿。slot 是 `NodeIdMap` 的私有欄位，`dom` 的其他程式碼看不到它。
- `Roots` 對每一種 root 的型別留一個 pool（以前只有節點和 collection）。建 root 時有留著的 cell 就拿來、改指到新的物件；丟掉的 root 的 cell 放回 pool。
- kun 裡因此有兩種 persistent：原本的（`new` 出來的，可以搬）和原地建的 slot（有值時不能搬）。預設用原本的；只有建立的成本在取樣裡看得到、而且本來就有不搬動的記憶體的地方才用 slot。這條規則寫在 `crates/dom/README.md`。

## 詳細設計

### 1. fork：原地建的弱 persistent

- **C++**（`src/binding.cc`、`src/binding.hpp`）：
  - `binding.hpp` 匯出 `cppgc::WeakPersistent<RustObj>` 的 `sizeof` 和 `alignof`，和 `cppgc__Member_SIZE` 一樣經過 bindgen：cppgc 開著 object names（`cppgc_enable_object_names`，桌面平台預設開著，release 也是）時用 `KeepLocationPolicy`，persistent 多一個 `SourceLocation`，大小跟著 build 的設定，所以不在 Rust 裡寫死。Windows 的 release 是 32 bytes（`raw_`、`node_` 各 8，`SourceLocation` 16；2026-10-22 本機編的 V8）。
  - 新增建立空的 `WeakPersistent`（用 fork 已有的 `construct_in_place`，建進 `MaybeUninit`）和只呼叫解構子、不 `delete` 的函式。`Assign`、`Get` 本來就收指標，照用。
  - 改了 C++，所以重編 V8、發了新的 release（`kun-v152.2.0-17`，`759eedc`，第 5 條）。之後只改格式的 `b9667c5` 在 tag 之後，kun 的 `third_party.toml` 用 `rev` 指過去。
- **Rust**（`src/cppgc/slot.rs`，在 `MemberSlot` 旁邊）：`WeakPersistentSlot<T>`。
  - bytes 在 `UnsafeCell` 裡、照 C++ 的 alignment 對齊：GC 清掉弱的 persistent 時會寫它的 `raw_`，所以不能有 Rust 的 `&` 指著那些 bytes 同時被 C++ 寫（和 `MemberSlot` 同一個理由）。
  - API（第 2 條）：
    - `empty() -> Self` 照值建，和 `MemberSlot::empty()` 一樣。空的 persistent 不拿 region 的 node、不記自己的位址，所以建好之後交出去、搬到最後的位置都沒關係。
    - `set(self: Pin<&Self>, target)`、`clear(self: Pin<&Self>)`：設值之後 region 的 node 記著 slot 的位址，`Pin` 保證它從此不搬動，而且記憶體被重用之前一定先 drop（把 node 還給 region）。型別帶 `PhantomPinned`。
    - `unsafe fn get(&self)`：照 `MemberSlot::get`，條件是用的期間沒有 GC（弱的目標可能被回收）。
    - drop 時在原地呼叫解構子：有值的話把 node 還給 region。
    - 不是 `Send`、`Sync`：persistent 只在建立它的 thread 上用，GC 只在 pause 裡讀它，所以不需要 atomic 的讀寫，thread 的條件也由型別擋住，方法不必為了它是 `unsafe`。
    - 一個安全的投影：從 `Pin<&[WeakPersistentSlot<T>]>` 取出第 `i` 格的 `Pin<&WeakPersistentSlot<T>>`。釘住的陣列的元素本來就是釘住的，這個投影是 sound 的；`unsafe` 收在 fork 裡，kun 那一側用 slot 不需要任何 `unsafe`。
  - 讀取照 RFC 0051 的 B5 在 Rust 裡讀 `raw_`，debug build 每次和 C++ 的 `Get` 比對。
  - 多一條升級 V8 時要確認的假設：空的 persistent 搬動 bytes 沒有問題（`raw_`、`node_` 都是 null，`SourceLocation` 也只是資料）。和上游的 `Member` 依賴「搬動沒有問題」同一類，寫進 `soundness.md` 的 A5。
  - 上游的 `Persistent`、`WeakPersistent` 不動。
- `README.machopo.md` 的「Local modifications」記這一項。

### 2. kun：id 表

- `NodeIdMap`（`crates/dom/src/node_id.rs`）是兩部分：`FxHashMap<NodeId, u32>`（格子的編號），和一塊塊的 slab：`Vec<Pin<Box<[WeakPersistentSlot<Node>; 256]>>>` 加 free list（第 3、6 條）。chunk 用 `Box::pin(std::array::from_fn(|_| WeakPersistentSlot::empty()))` 建，一旦配置就不搬動，`Vec` 搬動的只是 `Box` 的指標；設值經過 fork 的投影拿到那一格的 `Pin<&…>`。
- `insert` 從 free list 拿編號最小的一格（沒有就接一塊新的 chunk），把那格設到新的節點；`node` 讀那格的節點、`get` 把那格交給 `NodeHandle::try_new`；`prune` 對死掉的項目只把格子放回 free list（GC 回收節點時已經清空那一格、把 node 還回去），再把 free list 由大到小排序；`clear` 丟掉所有 chunk。chunk 只在 `clear` 和 `NodeIdMap` drop 時釋放，平常空了也留著，實際上就是到分頁關掉（第 3 條）。`DomIsolate::node_id_chunks()` 給測試看格子有沒有重用。
- free list 要排序：格子是 prune 時照 hash map 的順序放回去的，散在整張表裡；不排序的話每建一個節點都是一次 cache miss（第 6 條）。
- 這和 cppgc 自己的 `PersistentRegion` 同一個形狀：node 是 256 個一塊（`PersistentNodeSlots`），只往後接、空出來的放回 free list。

### 3. kun：所有型別的 root 的 spare pool

- 節點和 collection 的 pool 是 `Roots`（`crates/dom/src/handle.rs`）的兩個欄位（`spare`、`spare_collections`）：它們最熱，`field_pool` 用 `Any` 比對，不查表。其他型別的 pool 在 `spare_others`，一張以 `TypeId` 為 key 的表（`RefCell<FxHashMap<TypeId, Box<dyn Any>>>`，值是那種型別的 `Vec<Rc<RootCell<T>>>`），**用到才建**：第一次有那種型別的 root 被放掉時才加進表裡。沒有被 root 過的型別不佔任何東西，只被 root 過一次的型別的 pool 只有那一個 cell。拿出來的 cell 的型別一定是要的那個（表照 `TypeId` 找、`downcast` 確認）。
- 節點和 collection 不併進表裡，是為了不在每次 DOM 呼叫的 root 上多一次 hash；其他型別一次查表（`TypeId` 是兩個 `u64`，FxHash）和省下的配置比很小。
- `take_spare` 拿出來時把 `Persistent` 設到新的物件，`give_back` 放回時設到錨點。`RootCell` 和 `Root` 的型別不變，persistent 照舊是 fork 原本的 `Persistent`。
- 每種型別的上限：節點和 collection 4096（`SPARE_LIMIT`），其他型別 64（`OTHER_SPARE_LIMIT`，第 4 條）。pool 只會長到那種型別同時活著的 root 數的最高點，上限只在少見的型別一次爆量時有作用；超過的那些建新的 persistent。pool 滿時 `push_spare` 把 cell 交回，`give_back` 在借用結束之後才丟（丟掉 cell 可能連 `Roots` 一起丟掉）。
- `drop_spare` 先把兩個欄位和整張表取出來再丟掉，打斷 cell 和 `Roots` 的循環，在 heap 之前（I8）。

### 4. 不安全的地方和論證

- `crates/dom/soundness.md`：一節 `src/node_id.rs`（slot 有值時不搬動，由 `Pin` 保證；prune 不呼叫 C++ 的理由；`node` 只在 read scope 裡）；A5 加上「空的 persistent 可以搬」；I8 的 id 表；「`src/handle.rs`：root 和留著重用的 `Persistent`」的「留著的 cell」改寫成所有型別，加「借用池時不丟 cell」；「需要人工審查的地方」加升級 V8 時要看的 `BasicPersistent`。
- 新的 `unsafe` 只在 fork 裡（建立、解構、slice 的投影）和 `NodeIdMap::node` 的一處讀取；`bindings` 沒有新的。

### 測試

驗收表和數字在 `progress.md`。
- **fork**：`bindings` 的 `cppgc_weak_persistent`：空的可以搬；設、改設、清掉、再設；目標死掉時 GC 清掉它；chunk 的每一格都是自己的 root；drop 之後 node 還回去（保留那塊記憶體、在 `raw_` 寫一個將死的物件再 GC，GC 不碰它）。
- **kun**：`dom` 的 `tree` 加格子重用、其他型別的 root 重用、其他型別留 64 個、節點留更多；`handle.rs` 的 `the_roots_go_with_the_isolate` 加上其他型別。`dom` 現有的 handle、id 表、GC 的測試照跑。
- **mutation test**：fork 的 `slot.rs`、id 表（`mutants-2.toml`）、root 的 pool（`mutants-3.toml`）。free list 的順序只影響快慢，計數守不住，記在 `docs/test-gaps.md` 的「沒有計數守著的優化」。
- **GC 的矩陣**：`test.py matrix`、`fuzz` 由 CI 跑（debug 的 V8，有 `DCHECK`），merge 之後看結果。
- **效能**：使用者要求時用 `bench.py speedometer --sample`（Windows）量；收尾前量過一次，也因此找到 free list 的順序（第 6 條）。

### 分階段

三段都在 2026-10-22 做完，在 Windows 11 上：
1. **fork**：`WeakPersistentSlot`、`bindings` 的測試，發 `kun-v152.2.0-17`，kun 的 `RUSTY_V8_MIRROR_TAG` 換過去。
2. **id 表**：`NodeIdMap` 改成 slab，`soundness.md` 的新論證。
3. **所有型別的 spare pool**：不依賴 fork。

fork 的 C++ 改了要重編 V8 才測得到：整個過程在本機用 `ci.py v8 --profile release` 編，`RUSTY_V8_ARCHIVE` 指到它的產物（第 5 條）。這是第一次在 Windows 上用它，從零開始 15 分鐘（i9-13900K，V8 經過 sccache），之後改 `binding.cc`、`binding.hpp` 約 20 秒。Windows 只有 release 的 V8（沒有 `DCHECK`）。量測之後的一個修正（free list 的順序）是第四個 commit。

## 缺點

- **fork 的 C++ 改動和一次 release**：這份只為了效能改 fork 的 C++，每次升級 V8 都要確認 `BasicPersistent` 的版面和解構子的行為沒變（加進 `soundness.md` 的「需要人工審查的地方」）。
- **kun 裡有兩種 persistent**（第 1 條）：讀 `dom` 的人要知道 slot 的限制只在 `NodeIdMap` 裡，其他地方照舊。全部換成 slot 可以只剩一種，但要替沒有固定記憶體的地方多包一層 `Box`、多幾處 `unsafe` 和論證，而 fork 裡上游的兩種本來就留著。
- **id 表的記憶體存取要照順序**：發格子的順序不對時，每建一個節點都是一次 cache miss，比省下的配置還貴（第 6 條，量到的 26.9 ms）。現在靠 prune 之後的排序，沒有計數守著，只能靠取樣發現它壞掉。
- **chunk 不釋放**（第 3 條）：表的記憶體停在最高的時候（每格 32 bytes，x64，連同 `SourceLocation`）。cppgc 的 `PersistentRegion` 也一樣，現在的 hash map 也一樣（`retain` 不縮小容量），所以最高點不比現在差；每一項反而少幾個 bytes（`details.md` 的「id 表的記憶體」）。
- **pool 撐住更多 cell**：每種型別都留著放掉的 root，連同它的 `Persistent`（指著錨點）。

## 理由和其他做法

範圍見第 1 條。id 表選原地建，理由是它是最大的一塊、對所有平台都有好處、GC 的語意完全不變，fork 裡也已經有原地建 `Member` 的同一套模式；`Root` 選 spare pool，理由是暖機之後連 region 的 node 都不用拿，比原地建更省，也不用改 fork。完整的比較在 `details.md` 的「其他做法」：

- **B. 換掉 C++ 那一側的 allocator**：連 Skia、V8 的配置一起變便宜，但 Windows 上 mimalloc 不能靜態蓋掉 ucrt 的 `malloc`，只蓋 `operator new` 要先確認 rusty_v8 的靜態庫沒有自己的定義；而且每個 persistent 仍各配置一次。適合之後另外實驗。
- **C. id 表放到 GC heap 上的 weak map**（RFC 0058 的 3c、Blink 的 `DOMNodeIds`）：只解決 id 表那一半，GC 設計的改動大得多。
- **E. Windows 的 Segment Heap**（manifest）：主要省記憶體，CPU 不一定更快；現在走的 LFH（`RtlpLowFragHeapAllocFromContext`）已經是 NT heap 的快速路徑，慢的是次數，不是每一次（`details.md` 的「量測」）。
- **不做**：Windows 上 jQuery 每個 iteration 多約 12 ms，其他平台也多一次配置。

## 先例

- **Blink**：`Persistent<T>` 是一個值，嵌在擁有它的 heap 外物件裡（例如 `Persistent<Node>` 欄位），從不單獨配置；C++ 的 move 經過 move constructor，`PersistentNode::UpdateOwner` 改 node 記著的位址。Rust 的 move 沒有這個 hook，所以這份讓 slot 有值時不搬動，而不是照 C++ 那樣搬了再通知。
- **cppgc**：`PersistentRegion` 的 node 是 256 個一塊（`PersistentNodeSlots`），空出來的放回 free list；id 表的 slab 照同一個形狀。
- **`DOMNodeIds`**：Blink 只給被要求 id 的節點編號（`WeakIdentifierMap`，GC heap 上的弱表），不是每個節點一個 weak persistent。kun 的 pipeline 對每個要畫的節點都要 id（RFC 0058 的 2b），所以不照做（C）。
- **root 的重用**：Blink 的 binding 不 root receiver（保守的 stack 掃描），沒有對應；最接近的是 V8 的 `HandleScope` 重用 handle 的區塊。

## 已決定的事項

2026-10-22 確認：

1. **範圍是 id 表和 root（R4）**：id 表用原地建的 `WeakPersistentSlot`；`Root` 不換 persistent，把 spare pool 擴大到所有型別。`EventListenerHandle`、`StoreTable`、錨點照舊用 fork 原本的 persistent，kun 裡因此有兩種 persistent，新的那種只在 `NodeIdMap` 裡。不選：
   - R1（id 表和 `Root` 都原地建）：`Root` 只省掉 C++ 的 `new`，每個 root 仍要配置 `Rc`、拿 region 的 node；有了所有型別的 pool，暖機之後連這些都沒有。
   - R2（全部換）：listener 和 `StoreTable` 在取樣裡是 0，要固定位址還得各包一層 `Box`（拿一次 `new` 換一次 mimalloc），每換一處就多一處 `unsafe` 和論證；kun 只剩一種的好處只有一半，fork 裡上游的兩種本來就留著。記在「之後的可能」。
   - R3（只做 id 表）：`Root` 的約 10 ms 留著。
2. **slot 的 API 是 P1：空的照值建，有值時由 `Pin` 保證不搬動**（詳細設計的第 1 節）：`set`、`clear` 收 `Pin<&Self>`，fork 提供 slice 的安全投影，kun 用 slot 不需要 `unsafe`。理由：前三種 slot（`MemberSlot`、`WeakMemberSlot`、`TracedSlot`）都是 GC 物件的欄位，cppgc 不搬物件，位置在結構上就固定；這一種放在 heap 外的一般 Rust 記憶體裡，放進 `Vec`、`mem::swap`、從函式 return 都會搬動它，最容易誤用，值得由型別擋住（`details.md` 的「fork 的 slot」）。不選：
   - P2（照 `TracedSlot`：只建得出空的，`unsafe fn set` 的條件是「已經在最後的位置」）：和現有的 slot 形式一致，但條件只靠註解和審查，違反時要到 GC 讀錯地方才出事。
   - P3（只給裸指標的 `construct_at`、`destruct_at`，沒有擁有它的 Rust 型別）：drop 也要 kun 自己記得，漏掉就是 region 的 node 洩漏。
   - 一定在最後的位址上才建（不依賴「空的可以搬」）：Rust 沒有內建的原地初始化，要兩階段的建法加旗標或 `pin-init` 這類 crate，API 複雜很多，只換來少一條很穩的假設。
3. **id 表的 chunk 是 256 格，空了也不釋放**（C1、F1）：一塊 8 KiB（x64，兩頁；決定時以為每格 16 bytes、一塊一頁，量到的是 32 bytes），格數和 cppgc 的 `PersistentNodeSlots` 一樣；每 256 個節點才配置一次，配置已經不花時間，再大也省不到。chunk 只在 `clear` 和 `NodeIdMap` drop 時釋放，和 cppgc 的 `PersistentRegion`（直到 region 解構才釋放）、現在的 hash map（不縮小）一樣。不選：
   - 64 格（C2，照 `EdgeChunks`）、1024 格（C3）：差別量不到；64 格配置多 4 倍，1024 格小文件多浪費。
   - prune 時釋放整塊都空的 chunk（F2）：格子被 pin 住，活著的不能搬去集中，當時的 free list 是後進先出，空格散落各塊，很少有整塊全空；要有效得改成優先拿編號最小的格子（後來因為 cache 改成這樣了，第 6 條），還要記每塊活著幾格。記在「之後的可能」。
   - 只釋放最後面整塊都空的 chunk（F3）：碰到的機會比 F2 更少。

   重看之後維持（同一天）：`clear` 只在 `DomIsolate` drop 時執行，而一個 renderer 的 `DomIsolate` 從建立用到分頁關掉（只有換 script 的語言時換新的，導覽不換），所以 chunk 停在那個分頁看過的最重的頁面的最高點，直到分頁關掉。chunk 數有上限：被 prune 的格子放回 free list 重用，chunk 數是同時存在的項目數的最高點除以 256，和總共建過多少節點無關。現在的 hash map 一樣不縮小。考慮過 F4（優先拿編號最小的格子、prune 時釋放尾巴整塊都空的 chunk、順便縮小 hash map），先不做，記在「之後的可能」。
4. **pool 的上限：節點和 collection 4096，其他型別 64**（L2）：留著的 cell 每個約 96 bytes（`Rc` 和 `RootCell` 48、C++ 的 `Persistent` 32、region 的 node 16），留到 isolate 拆掉。熱的型別（canvas context、inline style、indexed getter 的 receiver）一次只要一兩個，64 涵蓋巢狀的呼叫；少見的型別爆量一次，最多留約 6 KiB。V8 的 handle 區塊也只留一塊備用。不選：
   - 一律 4096（L1）：少見的型別爆量一次之後，約 384 KiB 一直留著。
   - 所有型別共用一個總量（L3）：型別之間互相擠掉，要決定怎麼分，複雜度不值得。
5. **第 1 段開始前**（2026-10-22）：
   - C 函式照 fork 的命名：`cppgc__WeakPersistent__CONSTRUCT_IN_PLACE(uninit_t<…>*)` 建一個空的，`cppgc__WeakPersistent__DESTRUCT_IN_PLACE` 只呼叫解構子；`CONSTRUCT`、`DESTRUCT` 這兩個名字已經是 `new`、`delete` 的版本，不改它們。`binding.hpp` 匯出 `cppgc__WeakPersistent_SIZE`、`cppgc__WeakPersistent_ALIGN`。
   - 投影是 `WeakPersistentSlot::index(slots: Pin<&[Self]>, index: usize) -> Pin<&Self>`（`Pin<&[Self; N]>` 會 coerce 成 slice）；slot 也實作 `GetRustObj`，讓 `NodeIdMap` 的 `get` 照舊能交給 `Root::try_new`。
   - release 是 `kun-v152.2.0-17`：RFC 0071 用掉了 `-16`（`Isolate::SetIsJSApiWrapperNativeErrorCallback`，`0212ab6`），這份接在它後面。
   - fork 的改動先放在工作 branch `kun-0072`，測過才併進 `kun`、打 tag。不直接打 tag 試：release 的編號只往上數，試錯會用掉編號。原本打算在 Windows 上手動跑 GitHub 的 workflow 拿產物來測（以為 Windows 上不從原始碼編 V8）；實際上 `ci.py v8` 在 Windows 上編得起來（第一次 15 分鐘，之後約 20 秒），改在本機測，比每次等 workflow 幾個小時快。
6. **第 2 段開始前**（2026-10-22）：
   - 表是 `FxHashMap<NodeId, u32>`（格子的編號）、`Vec<Pin<Box<[WeakPersistentSlot<Node>; 256]>>>` 和 free list（`Vec<u32>`），從編號最小的空格子先發。不選 map 直接存 slot 的指標：多出 `unsafe`，保證一樣靠 chunk 不搬動。
     原本是後進先出（以為剛空出來的格子比較可能還在 cache 裡），量了之後改掉：格子是 prune 時照 hash map 的順序一次大量放回去的，`NodeId` 連號、FxHash 之後在 bucket 裡是散開的，後進先出就等於在好幾 MB 的 chunk 裡隨機發格子，每建一個節點的 `Assign` 都是一次 cache miss。Windows 上的 TodoMVC-jQuery，`BasicPersistent::Assign` 自己佔了 26.9 ms／iteration（每個節點約 87 ns），比省下的 NT heap 的配置還多；prune 之後把 free list 由大到小排序、pop 從最小的拿之後，剩 0.4 ms（`progress.md` 的「做過的檢查」裡的量測）。排序在 prune 裡，prune 只在表長到兩倍時跑，取樣裡看不到它。照 chunk 的順序掃一遍來重建 free list 也能得到同樣的順序、不用排序，但 hash map 的項目還是要另外清，比較複雜。
   - 讀節點經過 `NodeIdMap::node(id) -> Option<&Node>`，slot 的 `unsafe fn get` 只在這一處：呼叫它的只有 read scope 和借用它的 `NodeRef`，那時不會 GC（`soundness.md` 的 I2），回傳的參照又借用表。`get` 照舊回傳 slot（`&WeakPersistentSlot<Node>`），交給 `NodeHandle::try_new` 建 root。
   - prune 對死掉的項目只把格子放回 free list，不呼叫 C++：節點被回收時 GC 已經清空那一格、把 node 還回去（`ClearFromGC`），讀到 null 就表示那一格是空的（Rust 的讀取 debug build 每次和 C++ 的 `Get` 比對）。原本每個死掉的項目是一次 `delete`。
   - `get` 不檢查節點還活著：mutation test 發現那個檢查是多餘的（`node` 讀到空的 slot、`NodeHandle::try_new` 看到 null 都給 `None`），做的時候拿掉。
   - `clear` 丟掉所有 chunk，每一格的 `Drop` 把還有值的 node 還回去；`DomIsolate::drop` 照舊先清 id 表再拆 heap（I8）。
   - `DomIsolate::node_id_chunks()` 和 `node_id_entries()` 並列，給測試看格子有沒有重用，之後評估 F4 時也看它。
   - mutation test 用 `test.py mutate`（fork 已經停在 pin 的 tag 上，設 `RUSTY_V8_ARCHIVE` 連本機的 V8）；「`clear` 不丟 chunk」是等價的（`DomIsolate` 的欄位順序本來就讓 id 表在 heap 之前 drop），記在 `docs/test-gaps.md`。
7. **第 3 段開始前**（2026-10-22）：
   - 其他型別的 pool 放在 `Roots` 的 `RefCell<FxHashMap<TypeId, Box<dyn Any>>>`，值是那種型別的 `Vec<Rc<RootCell<T>>>`，第一次有那種型別的 cell 放回時才建；`pool` 照舊先比對節點和 collection 的欄位，拿出來的 cell 經過 `downcast`，型別一定是要的那個。成本：其他型別每次拿、放多一次 FxHash 和查表（幾 ns 到十幾 ns，省下的是 `Rc`、C++ 的 `Persistent` 和 region 的 node 的配置和釋放、建和丟兩次 FFI）；表本身每種型別一項；只 root 過一次的型別多付一次建項目、一直留一個 cell。不選：
     - 每個型別一個 static（`thread_local!`）的 pool（T1）：pool 要跟著 isolate，一個 thread 可以有好幾個 `DomIsolate`，共用的 pool 會讓一個 isolate 拿到另一個 heap 的錨點和 node（違反 `soundness.md` 的 I7）。
     - trait 指到 `Roots` 裡每種型別自己的欄位（T2）：不用 hash，但 `Roots` 要為約七十種會被 root 的型別各開欄位、各寫 `impl`，新增 GC 型別多一件事，`handle.rs` 要知道每一種型別。
     - trait 給每種型別編號、pool 是陣列（T3）：script object 有 `ObjectTag` 可用，`CustomElementDefinition`、`Holder`、`Registration`、`IntersectionObservation` 這些內部型別沒有；仍要 `Box<dyn Any>` 和 `downcast`，只省一次 hash。`pool` 是私有的，之後取樣看得到 hash 時可以換成它，不改外面的 API。
   - 上限是兩個常數：`SPARE_LIMIT`（4096）給節點和 collection，`OTHER_SPARE_LIMIT`（64）給其他型別（第 4 條）。
   - 重入：丟掉一個 `RootCell` 不會呼叫 `give_back`（它在 `Root` 的 `Drop` 裡），唯一的路是丟掉 cell 時連帶丟掉 `Roots` 本身，所以 pool 滿時的 cell 在借用結束之後才 drop；`drop_spare` 先把兩個欄位和表整個取出來再丟掉，打斷 cell 和 `Roots` 的循環。
   - 測試：`handle.rs` 的 `the_roots_go_with_the_isolate` 加一種其他型別；`dom` 的 `tree`（節點的 spare 測試在那裡）加「同一種其他型別反覆 root 只建一次 `Persistent`」和「同時拿 100 個、放掉、再拿 100 個，只多建 36 個」。做的時候原本打算放在 `script_objects`，但那是外部 engine 的 slot 對每種型別的檢查。

## 待決定

無。

## 之後的可能

- **其他 persistent 也換成原地建**（第 1 條的 R2）：取樣裡某一處的 persistent 的建立看得到了（例如大量分派事件的網頁讓 `EventListenerHandle` 變熱），就換那一處；要換得先替它找到不搬動的記憶體（`Box`、`Rc` 的內容或 slab）。全部換完之後，kun 只剩一種 persistent。
- **id 表的記憶體不停在分頁的最高點**（第 3 條的 F4）：量記憶體時發現一個分頁的 id 表停在高點的問題才做。新節點已經從編號最小的空格子拿（第 6 條），活著的節點集中在前面；還要每塊 chunk 記活著幾格，prune 時從尾巴釋放整塊都空的 chunk，項目數少於容量的四分之一時順便縮小 hash map。格子被 pin 住，前面的 chunk 裡還有活著的節點就留著。表停在多高看 `DomIsolate::node_id_chunks()`。
- **prune 不排序**：照 chunk 的順序掃一遍來重建 free list，也能照位址的順序發格子（第 6 條沒選的做法）；排序在取樣裡看不到，先不做。
- **C++ 那一側的 allocator**（B）：Skia 的路徑和 V8 自己的配置，Windows 上約 9 ms，連同 `docs/limitations.md` 裡 C++ 那一側的記憶體。
- **canvas context 的 receiver 不 root**：讀的成員已經不 root `this`（RFC 0067），canvas 的繪圖成員會改 context 的狀態，是否一定要 root，可以另外看。
