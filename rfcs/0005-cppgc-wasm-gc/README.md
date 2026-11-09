# RFC 0005：kun 自己的 GC 當 wasmtime 的 GC

- 開始日期：2026-10-04（原本是 RFC 0001 的第二層，2026-10-05 拆出來）
- 狀態：✅ 已完成：2026-11-08（2026-10-05 起延後，2026-11-01 重新打開：原本用 cppgc，改成 kun 自己的 GC（elise）；2026-11-02 起暫停，等 RFC 0089（已決定的事項 20），2026-11-08 重做 4a、做完第 4、5 段；驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`（2026-11-01 的研究：wasmtime 50 的 collector 介面和 sandbox 的約定、比較過的做法、sandbox 的取捨、GC 和 fork 各要補的東西）。cppgc 版的研究在 RFC 0001 的 `rfcs/0001-wasm-scripts/details.md`（「未來考慮：wasmtime 加上 cppgc collector」）和 `rfcs/0001-wasm-scripts/wasmtime-questions.md`（Q13–Q17）；原型在這個資料夾：`q14-prototype/`（GC 的 lowering）、`q15-prototype/`（Store 執行中的 GC）、`q16-prototype/`（cppgc 上的 wasm GC 物件），2026-10-04 為 cppgc 版寫的，不合併

## 摘要

fork wasmtime（`../third_party/wasmtime` 的 `kun` 分支，基底 v50.0.1），加上一個用 kun 自己的 GC（elise，`../third_party/elise`）的 collector：編成 wasm GC 的語言配置的物件（`struct.new`、`array.new`）配置在 DOM 的 heap 上，和 DOM 由同一個 GC 回收。`dom_wasm` 的 Store 借 DOM 的 heap，屬於 fork 的 `StoreSet`，記憶體安全靠 fork 和 elise，不靠 kun 的 safe 的程式。這個 heap 不當 sandbox：wasmtime 的驗證器、Cranelift 和 fork 裡我們寫的銜接層，和 rustc、LLVM 一樣算是可信任的基礎。

## 動機

RFC 0089 把 DOM 換到 kun 自己的 GC 之後，wasm 文件的 DOM 在這個 heap 上；wasm 的 script 原本跑在原版的 wasmtime（49.0.2）上，guest 用 WIT 的 handle 拿 DOM 物件。wasmtime 自己的 GC heap（預設是 copying collector：semi-space、stop-the-world）和 DOM 的 GC 是兩個互相看不到的 GC；只用 linear memory 的 guest（C、Rust、Lua）碰不到這件事。

目標：
- wasm GC 的物件和 DOM 在同一個 heap、同一個 GC：記憶體一起算，wasm 的物件也得到分代、concurrent marking 和 concurrent sweeping。
- 不跑 V8 的設定裡，kun 的 GC 是唯一的 GC。
- fork 跟得上上游：改動盡量是新增的檔案。

不在範圍內：
- **DOM 參照變成 GC 參照**（guest 直接拿著指向節點的參照，而不是 handle）：Component Model 的 canonical ABI 只能用 handle 傳 resource，要等「GC 版 canonical ABI」，或離開 Component Model 另定一種用 `externref` 的介面（RFC 0001 的第 10 條不選）。沒有這一步，跨 wasm 和 DOM 的循環照樣以 Store 為單位回收，handle 的限制（要 drop、沒有身分）都還在（RFC 0001 的「Store 和 GC」）。
- **GC heap 當 sandbox**：已決定的事項 3。
- **接上 renderer**：kun 還沒有 wasm 的文件；「文件的 heap」現在是一個 thread 的 `WasmDom` 的（已決定的事項 23 第 0 點）。

## 設計概要

對網頁的作者沒有看得到的變化：用 wasm GC 的語言編的 guest 照樣是 component，用 WIT 的 handle 碰 DOM；它自己的物件改由 DOM 的 heap 回收，記憶體算在那份 DOM 上。

## 詳細設計

### fork 裡的 collector

一個 collector 在 wasmtime 裡分成四塊（`details.md` 的「wasmtime 50 的現況」）：`wasmtime` 的 `GcRuntime` / `GcHeap`、`wasmtime-environ` 的 `GcTypeLayouts`、`wasmtime-cranelift` 的 `GcCompiler`、`config.rs` 封閉的 `Collector` enum。fork 照 null、DRC、copying 的樣子各寫一份 elise 的版本，`Collector::Elise` 放在 cargo feature `gc-elise` 後面（不是 `Auto`）；原有的檔案只改必要的地方，標 `kun:`，記在 fork 的 `README.machopo.md`。kun 用 path 依賴 elise 和 fork，`third_party.toml` 釘它們的 commit（已決定的事項 18、19）；同一個 build 裡只有一份 elise。

### heap 和參照

- **heap 是 DOM 的**：`Dom` 擁有 heap、借出它（`Dom::heap_mut`）。fork 的 `EliseHeap` 有兩種：沒有 provider 時照舊擁有一個 `raw::EngineHeap`（一個 Store 一個 heap，上游的用法和 fork 的測試）；設了 `Config::elise_heap_provider` 時借 embedder 的 heap，provider 依鑰匙借出一個 thread 的 heap（`dom_wasm` 的 `DomHeap`：thread-local 的 `WasmDom`，鑰匙是它的序號）。wasmtime 配置的那塊 `vm::Memory` 只拿著、還回去（已決定的事項 13、24）。
- **參照的形式不用改**：elise 是 4 GiB 的 cage、邊是 32 位元的 offset，和 `VMGcRef` 一樣寬；產生的碼照舊是「基底 + zext(ref)」，`vmmemory` 回傳 cage 的基底和大小。0 是 null，最低位元是 1 的是 `i31ref`，都和 wasmtime 一樣。
- **物件格式**：elise 的 header（型別編號和 flags，8 byte）在前，資料從 wasmtime 的 `VMGcHeader`（kind 和 `VMSharedTypeIndex`）開始，array 的長度在 wasmtime 的位置；每個物件多 8 byte，合成一個是「待決定」。物件 8 對齊（`v128` 的欄位照 copying collector 的做法，已決定的事項 16）。
- **型別**：engine 的 type registry 登記 GC 型別時，elise 照它的 layout 登記執行時的型別（每個 engine 一段編號），registry 放掉時退休；Store 拿著它配置的每個型別，所以沒有 heap 有放掉的型別的活物件（已決定的事項 14）。`externref` 是一個型別，host data 留在 Store 的表、用 elise 的 id 知道物件死了（已決定的事項 15）。

### elise 給 embedder 的能力

都在 elise 裡做、在它自己的測試裡驗證（elise 的 `docs/api.md` 的 `raw`）：
- **執行時登記的型別**和照編號配置（清成 0）；**可能放著 `i31ref` 的邊**：marker、scavenge、verifier、兩種 barrier 都跳過。
- **外面給的 root 的 GC**（`*_with_roots`）：root 讀一次、GC、照同樣的順序寫回；**frame 的計數**（`enter_frames`）：frame 在 stack 上時安全點不做要讀 root 的 GC（nursery 滿了配置到 old）；**bits 的入口**（`raw::alloc_bits`、`write_bits_in` 等）：從不 GC，該 GC 時回 `GcDue`。
- **heap 之外的 root 的來源**（`Heap::set_roots_source`、`RootsSource`）：每個讀 root 的 GC 都問它，給不了時不做（已決定的事項 23）。
- **借 heap 的 engine 用的**：`Heap` 上的 `raw::cage_in`、`size_at`（只從物件讀大小）、`barrier_state_in`、`new_id_in`、`satb_in`、`remember_in`，`HeapAlive`（heap 活著、是不是同一個）；拿掉來源、離開 frame 是 `unsafe`，在 frame 裡 drop 的 heap leak（已決定的事項 25）。

### GC 的時機和 root

- **Store 的 GC**（wasm 配置要的、`Store::gc`）：wasmtime 收這個 Store 的 root（stack map、table、global、host 的 `Rooted`、暫停中的 fiber）交給 elise，`StoreSet` 的來源交其他 Store 的，DOM 的物件一起收。沒有 GC heap 的 Store 不交 root（它沒有 GC 參照）。
- **`StoreSet`**（fork）：擁有借同一個 heap 的 Store（不在裡面的拿不到 GC heap），經過 closure 借出 `StoreContextMut`；借出時 heap 算 frame。來源交沒借出的 Store 的 root；有別的 Store 借出中（正在 GC 的那個除外）時回給不了（已決定的事項 25）。
- **DOM 的安全點**：沒有 Store 借出時照常 GC，向來源要 Store 的 root；有 Store 借出時（guest 在 host 呼叫裡）不做要 root 的 GC，由會配置的 host 函式做完 DOM 操作後經過 Store 做（`StoreContextMut::gc`，已決定的事項 26）。
- **檢查**：借來的 heap 的每次讀寫、進入 wasm、恢復 fiber 都確認在 heap 的 thread 上、heap 活著、是 set 的那一個（`HeapAlive::is`），provider 做錯只會讓呼叫失敗。

### barrier 和配置

wasm 碼直接寫欄位、不經過 host，所以 barrier 在 `GcCompiler` 產生的碼裡，照 elise 在 Rust 這一側的做法（已決定的事項 16）：
- **分代的 barrier**：寫進的值在 young 的頁、被寫的格子不在時，記進那一頁的 slot set；判斷是讀兩次 young 的表（inline），記錄是 libcall（`elise_remember`）。新物件的欄位初始化也做（大的 array 和 frame 裡 nursery 滿了時直接配置在 old）。
- **SATB 的 barrier**：marking 進行中，寫之前把舊值交給 libcall（`elise_satb`）；沒在 marking 時只讀一個旗標。
- **root 不經過 barrier**（global、table）：GC 時交出。**讀取沒有 barrier**：wasm GC 沒有 weak 參照。
- **runtime 的寫入**：GC 參照經過 `GcHeap::write_gc_ref_in`、`init_gc_ref_in`（elise 的 `write_bits`），數值欄位經過 `VMGcObjectData` 的 cell，不建 `&mut`，背景的 marker 同時讀得安全（已決定的事項 22）。
- **配置經過 libcall**（`gc_alloc_raw`）；在 JIT 裡直接 bump nursery 是「之後的可能」。

### 信任的基礎

GC heap 不當 sandbox（已決定的事項 3）：heap 裡的內容只有 elise、host 的 DOM 程式碼和 wasmtime 產生的碼會寫，產生的碼守不守 GC 的約定（root 完整、邊合法、該有的 barrier 都有），由 wasmtime 的驗證器、Cranelift 和 fork 裡的銜接層保證，kun 信任它們。所以：
- `External<T>` 照舊直接放在物件裡，DOM 的欄位型別不受限制，GC 不把 heap 的內容當成不可信任，解引用時不比對型別。
- wasmtime 和 GC、exceptions、驗證有關的安全公告，在 kun 裡當成可能寫壞 host 記憶體的 bug，盡快升級 kun 釘的版本（已決定的事項 4）。
- fork 裡的銜接層（`GcCompiler`、型別的 layout、root 的交接、`GcHeap` 和 `GcRuntime` 的實作、`StoreSet`）也在信任的基礎裡，照 GC 的標準測；kun 的 safe 的程式用錯它們只會失敗或 panic，不會寫壞 heap（已決定的事項 25）。

### 測試

驗收表和結果在 `progress.md`。
- **elise 的新能力**：elise 自己的 `tests/it/raw.rs`、`engine.rs` 和 verifier 的單元測試，照它原本的做法跑 Miri、loom、shuttle。
- **fork 的銜接層**：`tests/all/elise.rs`（host 的 API、產生的碼的每一種 barrier、marking 一步一步走）和 `tests/all/elise/shared.rs`（借來的 heap）；上游的 GC 測試和 GC 型別的 spec test 用 elise 跑（`machopo/elise_tests.py`，每次 GC 之後跑 verifier，和每次配置都 GC 的 stress）；固定種子的差分 fuzz（`gc_ops` 對 copying collector）。長時間的 cargo-fuzz 還沒跑（已決定的事項 28）。
- **kun 的測試**：`dom_wasm` 的 `tests/it/shared.rs`：C 的 guest 和用 GC 型別的 WAT 的 Store 在同一個 heap。和 DOM 之間經過 handle 的循環要等 `externref` 指向 DOM 節點（「待決定」），驗收不涵蓋。
- 新的行為都跑了 mutation test：弄壞 barrier、漏報 root、跳過 i31 的判斷、借錯 heap、少算 frame。

### 分階段

1. elise 這一側的能力（不需要 fork）：1a 執行時登記的型別、1b `i31ref`、1c 外面給的 root（已決定的事項 5）。
2. fork 的 runtime，每個 Store 自己的 heap，經過 host 的 API 測：2a 不 GC 的部分、2b GC（已決定的事項 13 到 15）。
3. `GcCompiler`：barrier 和配置（已決定的事項 16）。
4. 和 DOM 共用 heap（已決定的事項 17）：4a kun 換成 elise、4b kun 用 fork 的 wasmtime（已決定的事項 18 到 21）；4c runtime 不建 `&mut`（已決定的事項 22）；4d-1 elise 的 root 的來源、4d-2 fork 的 `StoreSet` 和借來的 heap、4d-3 `dom_wasm` 接上（已決定的事項 23 到 26）；4e 不加上限（已決定的事項 27）。
5. 驗收（已決定的事項 28）。

## 缺點

- **長期維護 wasmtime 的 fork**：wasmtime 大約每個月一個大版本，GC 這一塊還在快速變動（`details.md` 的「wasmtime 50 的現況」），每次升級都要 rebase；fork 改到上游的呼叫路徑（進入 wasm、恢復 fiber、`do_gc`）各一處。
- **打破 wasmtime 對 `GcHeap` 的安全約定**：wasmtime 要求每個 GC heap 是一個小 sandbox，壞掉的 heap 永遠不能害到 host；這個 collector 不守這條，所以永遠不能進上游。wasmtime 那些上游評為 medium、只會寫壞 GC heap 的 bug（例如 2026-10-02 的 CVE-2026-115379、CVE-2026-115383），在 kun 裡可能寫壞 host 的記憶體；renderer 現在是 Browser process 裡的 thread，外面沒有 OS 的 sandbox（RFC 0004）。
- **GC 多了給 embedder 的介面**：執行時的型別、bits 的入口、外面給的 root、root 的來源都在 safe 的 API 之外，elise 的 `unsafe` 和 soundness 的論證跟著變大。
- **每個 wasm 物件多 8 byte**：兩個 header 並存（「待決定」）。
- **guest 在 host 呼叫裡時，DOM 的安全點不 GC**：要等會配置的 host 函式經過 Store 做；兩個 Store 同時借出時都不做，配置的進 old。
- **沒有 heap 的上限**：cage 滿了 panic（`docs/limitations.md`）。
- **沒有前例**：V8 的 wasm GC 物件在 V8 自己的 heap（在 V8 sandbox 裡），Blink 的 Oilpan 不讓 JIT 的碼直接寫。

## 理由和其他做法

比較過的做法（`details.md` 的「比較過的做法」）：
- **甲、不 fork**：wasm 物件留在 wasmtime 的 copying collector，兩個 GC 互相看不到。
- **乙、fork，kun 的 GC 當 collector，但每個 Store 自己一個 heap、照 wasmtime 的 sandbox 約定**：不打破上游的約定，fork 最容易同步；但 DOM 和 wasm 之間的循環收不掉，記憶體分成兩邊算。2026-11-01 的建議，不選。
- **丙、fork，wasm 物件和 DOM 同一個 heap**：選了。heap 要不要當 sandbox 另外比過：以整份文件的 heap 為 sandbox 的邊界做得到（像 V8 sandbox），代價是 `External` 換成 side table、GC 驗證從 heap 讀出的值、heap 裡的欄位型別限制成任何 bit pattern 都合法；不選（已決定的事項 3）。
- **丁、fork 只開放介面**（`Collector` 讓 embedder 提供）：compiler 那一半要公開 Cranelift 的 IR builder，上游內部一改外面就壞，同步的成本沒有少。
- **cppgc 當 collector**（這份 RFC 原本的方向，2026-10-04）：RFC 0087 做了 kun 自己的 GC 之後改掉；cppgc 版的 Q14–Q16 裡和 cppgc 無關的結論（lowering 的形狀、只在安全點 GC 時的 root、`i31ref` 的對齊）照樣用得上。

## 先例

- **V8**：wasm GC 的物件在 V8 自己的 heap，和 Blink 的 cppgc 組成 unified heap；V8 sandbox 把 cage 裡的資料當成不可信任，native 指標經過 external pointer table。
- **wasmtime**：內建的 collector（DRC、copying、null）各自一塊 GC heap，照「indexed heaps」的約定（2024-04 的 wasmtime PR #8196）：heap 是連續的記憶體、參照是 32 位元的 index、heap 裡的東西不可信任。
- **Blink 的 Oilpan**：不在 V8 sandbox 裡，heap 裡直接放 native 指標，但 JIT 的碼不直接寫它。

## 已決定的事項

在 RFC 0001 決定的（那邊的編號）：第 13 條（2026-10-04，最終目標是 cppgc 當 wasmtime 的 GC；2026-10-05 由第 54 條改成這份 RFC，2026-11-01 由這裡的第 1 條改成 kun 自己的 GC）、第 15 條（2026-10-04，接受 sandbox 變弱；2026-11-01 由這裡的第 3 條取代）。

1. **fork wasmtime，collector 用 kun 自己的 GC，跟著上游同步**（2026-11-01，使用者決定）。取代 cppgc 的方向：RFC 0087 之後 DOM 在 kun 自己的 GC 上，wasm 物件和 DOM 同一個 GC 就是同一個 heap。不選不 fork（甲）：wasmtime 沒有讓 embedder 提供 collector 的介面，也沒看到上游的計畫；不選只開放介面（丁）：同步的成本沒有少。
2. **wasm GC 的物件和 DOM 在同一個 heap，每份文件一個**（丙，2026-11-01，使用者決定）。不選每個 Store 自己一個 heap（乙）：DOM 和 wasm 之間的循環收不掉，記憶體分兩邊算。
3. **GC heap 不當 sandbox：wasmtime 的驗證器、Cranelift 和 fork 裡的銜接層算是可信任的基礎，和 rustc、LLVM 一樣**（2026-11-01，使用者決定）。理由：產生的碼守不守 GC 的約定由這些元件保證，選 wasmtime 就是信任它們，不然不如用 V8 的 wasm；GC 本身已經經過 TLA+、Verus、loom、shuttle、Miri 和 fuzz 的驗證。所以 `External<T>` 照舊放在物件裡，RFC 0087 已決定的事項 7 留到「wasm GC 的物件放進這個 heap 時量過再決定」的部分，在這裡定成不換 side table；DOM 的欄位型別不受限制；GC 不驗證從 heap 讀出的值、解引用時不比對型別。不選以整份文件的 heap 為 sandbox 的邊界（`details.md` 的「sandbox 的取捨」）：要 side table、GC 的驗證和欄位型別的限制，防的是這些可信任的元件出 bug。
4. **跟著第 3 條要做的事**（2026-11-01，使用者確認）：
   1. wasmtime 和 GC、exceptions、驗證有關的安全公告，在 kun 裡當成可能寫壞 host 記憶體的 bug，盡快升級 kun 釘的版本；fork 的改動盡量放在新增的檔案，才跟得上。
   2. fork 裡的銜接層也在信任的基礎裡：照 GC 的標準測（「測試」的差分 fuzz、stress 模式、wasmtime 的 GC fuzz target）。
5. **第 1 段（GC 這一側的能力）在 elise 做，分三段**（2026-11-01，使用者照建議確認）：1a 執行時登記的型別和照型別編號配置、1b 可能放著 `i31ref` 的邊、1c 在 mutation 的中途 GC（root 由外面給、搬動之後寫回；名字由第 11 條改正：不是在操作的中途）。fork 依賴 elise（MIT OR Apache-2.0）；丙要 collector 和 DOM 同一個 heap，所以第 4 段之前 kun 要換成 elise。不選在 kun 的 `gc` crate 做、再手動同步到 elise：fork 會依賴 AGPL 的 kun，fork 和 kun 互相依賴。fork 的基底到第 2 段開始時再定（2026-11-01 的看法：當時最新的 release tag，不用 main）。
6. **1a 的型別表：static 和執行時的型別同一張表、同一個編號空間，執行時的格子可以取消登記、重用，編號由 embedder 指定**（2026-11-01，使用者確認）。trace 和 scavenge 查表照舊讀一次 static 的表，和只有 static 型別時一樣；表放大（例如 2^16 格以上）放在 .bss，沒用到的頁不佔實體記憶體，照 cppgc 的 `GCInfoTable`（kun 釘的 V8 裡 `kMaxIndex = 1 << 14`、需要時 `Resize()`）。`MAX_TYPES` 以下照舊是 static 型別（bindings 用它遮出 wrapper 的 tag），執行時的型別在它上面。embedder 為每個 Engine 保留一段連續的編號、在那段裡用自己的 id 登記，所以之後可以讓 elise 的編號是「Engine 的基底 + `VMSharedTypeIndex`」、header 合成一個（每個 wasm 物件省 8 byte），也可以照舊各發各的；第 3 段定 header 時再選（「待決定」）。每一格留住 embedder 給的東西（wasmtime 的 `RegisteredType`），取消登記之後，到下一次完整的 GC 做完才放掉、重用（`details.md` 的「型別的生命週期」）。不選：
   - leak 進現有的表（甲）：瀏覽器每次載入頁面都編新的 module，型別只增不減，撞到上限；
   - 兩張表、用一個 bit 區分（乙）：熱路徑多一個分支，DOM 和 wasm 的物件混在一起時不好預測；
   - 型別描述是 heap 裡的物件（丙，V8 的 `Map`）：每個物件多讀一次描述、多一次標記的檢查；
   - 直接用 `VMSharedTypeIndex` 當 elise 的編號（乙''）：它是每個 Engine 各自的，kun 每個 renderer 一個 Engine，同一個 process 裡會撞。
7. **1a 的 API：elise 的公開模組 `raw`，照 `js.rs` 的做法一直編進去**（2026-11-01，使用者照建議確認）。檢查得了的做成 safe：登記型別時 elise 檢查 layout（offset 在物件裡、對齊、邊不重疊、大小在上限內），在冷路徑上，檢查過之後 elise 不會因為錯的 layout 出錯；檢查不了的才是 `unsafe fn`、寫明前提：配置之後寫進物件、寫邊（barrier 的約定），之後 1c 交出的 root。名字用通用的 `raw`：執行時的型別不只 wasm 用（重播錄到的 heap 也要）。elise 的 soundness 文件的「需要人工審查的地方」加一節給它。對照 cppgc：給 embedder 的危險 API 在公開、分開的 `cppgc::subtle`，不用編譯選項擋。不選放在 cargo feature 後面：feature 不是存取控制，CI 和 Miri 還要多跑一組；不選 safe 的動態 API（每次存取檢查 offset）：wasmtime 產生的碼直接寫記憶體、繞過它，host 這一側的檢查是白付的成本。
8. **執行時的型別什麼時候放掉：每個 heap 都做完一次退休之後才開始的完整 GC**（甲，2026-11-01，使用者確認）。marker 和 scavenge 只讀活的物件的型別，例外是進行中的 marking 的快照（SATB）和死掉的 old 物件的 slot set 讓年輕的物件被 copy；sweep 只讀死掉、有 drop glue 的物件的型別（執行時的型別沒有）；nursery 的 from-space 每次 scavenge 之後整塊丟掉。所以 elise 記著所有活的 heap 和各自做完的完整 GC 的次數，`raw` 的退休記下當時的次數，每個還活著的 heap 都多做完一次之後才放掉那一格、讓它可以重用。退休是 `unsafe fn`：之後不再配置那個型別、也沒有活的物件是那個型別（Component Model 下 Store drop 之後就成立，fork 保證，在信任的基礎裡，第 3 條）。熱路徑不變；冷路徑上 heap 的建立、drop 和完整的 GC 結束時多碰一次全域的表。表 2^18 格（約 2 MiB 的 .bss，沒用到的頁不佔實體記憶體；cppgc 是 2^14）。不選讓退休是 safe 的（marker 和 scavenge 讀到退休的型別時記下還有活的、延後放掉）：每個物件多一個分支；不選永遠不放掉（撞到上限，第 6 條的甲）。
9. **1a 的實作**（2026-11-01，使用者確認）：
   1. 陣列也在 1a 做：layout 是固定的部分（大小、邊的 offset，遞增、4 byte 對齊）加上可有可無的陣列（長度是固定部分裡的一個 `u32`，元素 1、2、4、8 或 16 byte，可以每個元素都是邊）。配置時整個清成 0，長度在物件公開之前寫好。wasm 的 array 要用到，1b、1c 不用再改 layout。
   2. 配置回傳 `Gc<'gc, Erased>`：root 和邊用現有的 `Persistent<Erased>`、`Member<Erased>`，Rust 的物件和執行時的型別的物件互相指得到；`downcast` 成任何 Rust 型別都是 `None`（編號在 `MAX_TYPES` 以上）。不另外做一組 raw 的 handle。
   3. embedder 給每一格留住的東西是 `Box<dyn Send + Sync>`，那一格放掉時才 drop，在鎖外 drop（它可能呼叫回 elise）。
   4. 保留的一段編號（`TypeRange`）drop 時，空的和已經放掉的編號還回去；還登記著、沒退休的格子一直佔著：elise 不知道它們的物件還在不在，不能替 embedder 退休。
   5. 執行時的型別沒有 drop glue：sweep 和 heap 的 drop 不讀它們，第 8 條的論證只要看 marker、scavenge 和 verifier。
   6. elise 內部查型別的 `types::entry`、`info` 改成 `unsafe fn`，前提是「這個物件 GC 還可能讀到」，每個呼叫的地方寫明理由。查表用編號直接取那一格，不把整張表借出去：2 MiB 的表每查一次就整個借出去，Miri 每次都檢查整段，整套 Miri 慢幾十倍（2026-11-01 在 Miri 下跑新的測試時發現）。
   7. 退休的時機的整合測試在子程序裡單獨跑（同一個 process 裡別的測試的 heap 會擋住放掉），在 Miri 下 ignore；時機的規則另有不靠全域狀態的單元測試。
10. **1b：可能放著 `i31ref` 的邊**（2026-11-01，使用者照建議確認）。`i31ref` 是塞在參照的格子裡的 31 位元整數，GC 不配置它，只要在走邊時認出來、跳過；wasmtime 的 `VMGcRef` 最低位元是 1 的就是它（`value << 1 | 1`，Cranelift 產生的碼自己編解碼），它的 collector 每條邊用 `is_i31()` 跳過，V8 的 Smi 也是用最低位元分辨。elise 的物件的 offset 都 4-aligned，用同一種編碼不會衝突，只有執行時的型別的邊會放它。
   1. 每條執行時的型別的邊都可能放著 i31，layout 不標（甲）：marker、scavenge 和 verifier 讀到值之後多一次位元檢查，Rust 的型別的邊不受影響。不選在 layout 標出哪些邊可能是 i31（乙）：省下的只有那次位元檢查，漏標的話 GC 會把整數當物件讀，登記時檢查不出來，第 7 條「檢查過的 layout 就是安全的」不再成立。
   2. scavenge 經過 slot set 的值在共用的 `evacuate` 裡跳過 i31（甲）：slot set 不知道格子是哪一種物件的，記下之後可能被寫成 i31；Rust 的物件的格子在 GC thread 上也多一次位元檢查，mutator 的路不變。不選執行時的型別的物件另記一份 slot set（乙）：barrier 要分辨格子是哪一種物件的，記憶體和程式都多一份。
   3. API 是 `u32` 的 bits 一層，上面包一層有型別的（甲加丙）：`read_bits`、`write_bits`（`unsafe`：寫進的是 0、i31，或這次操作裡拿到的同一個 heap 的活物件的 offset）是 fork 的銜接層要用的形式（wasmtime 的 `GcHeap::write_gc_ref` 收的是可能是 i31 的 `VMGcRef`），寫時經過和 `Member::set` 一樣的 barrier、i31 除外；`read_edge`、`write_edge` 改用 `Option<Ref>`（`Object` 或 `I31`），包在它上面；`I31` 照 wasmtime 的 `I31`。對照 cppgc：`subtle::HeapConsistency` 把裸指標的 barrier 開放給 embedder，平常用的還是有型別的 `Member`。`u32` 變回 `Gc`（當 root、交給 Rust）留到 1c：它的意義要等 GC 能在 mutation 的中途發生時才定。不選只做有型別的（甲）：fork 要走的路到 1c 才有，1b 測的不是之後用的那條；不選只做 bits（丙）：測試和其他的 embedder 要自己編解碼。`raw` 在 elise 0.2.1 之後才加、還沒發布，改 1a 的 signature 沒有相容的成本。
11. **1c：外面給的 root 的 GC**（2026-11-01，使用者確認）。查過之後：kun 的新 DOM 裡 wasm 的碼一定在兩個 DOM 的操作之間跑（每個 host function 各自進一次 `mutate`，listener 也在操作之間呼叫），所以 elise「一次操作裡不 GC」的前提不變，`mutate` 不多付任何成本。難處在 wasm 呼叫 DOM 時，wasm 的 frame 還在 stack 上、拿著參照，elise 在那次操作的 safepoint 自己開始的 GC 看不到它們。
   1. 只在給得出完整 root 的地方開始要讀 root 的 GC 工作（甲）：wasm 配置失敗或 `Store::gc`（wasmtime 收齊 root），和沒有 wasm 的 frame 時（task 之間）。wasm 的 frame 在 stack 上時，DOM 的操作的 safepoint 不開始 scavenge、marking 的開始和收尾，nursery 滿了改配置到 old；sweep 和 marker 的步驟照跑。DOM 的 safepoint 的檢查不加東西：embedder 進出 wasm 時設一次狀態，只有 nursery 滿了的慢路徑看它。代價是 wasm 呼叫 DOM 時配置的物件直接進 old。對照 V8：任何配置的地方都能 GC，JS 和 wasm 的 frame 用 safepoint table、Blink 的 C++ stack 用 conservative 的掃描；kun 刻意不做 conservative 的掃描。不選 elise 每次要 GC 都向 embedder 要 root（乙）：host function 拿著 store 的借用、DOM 在 `borrow_mut()` 裡，再進 store 走 stack 會重入，每一種 frame 都要給得出準確的 root；不選 wasm 執行中都不 GC、配置失敗就長大 heap（丙）：一次呼叫裡大量配置的程式記憶體沒有上限。
   2. 外面的 root 是 GC 的呼叫帶的走訪函式，一格一格交出 `&mut u32`（甲，`unsafe`）：GC 跳過 0 和 i31，標記它們，搬動之後寫回；scavenge 和完整的 GC 都收。對應 wasmtime 每次 GC 給的 `GcRootsIter`（stack map 的格子、`VMGcRef`、`ValRaw`），一次性、不用登記；marking 的收尾也只在給得出 root 的地方（第 1 點），所以再給一次。不選把外面的格子長期登記在 elise 裡（乙，像 `Persistent`）：stack 上的格子每次都不一樣，登記和取消的成本落在每次呼叫上。
   3. wasm 的 GC 指令要的配置和寫邊的 barrier 的慢路徑，是一組不經過 `mutate` 的 bits 入口，在 `&mut Heap` 上（甲）：配置失敗回 `None`、不自己 GC，GC 交給 wasmtime 收齊 root 之後呼叫。每個 wasm 物件不付 `mutate` 的固定成本；`&mut Heap` 讓它在操作裡呼叫不到。bits 活過 GC 由 wasmtime 的 stack map 保證，在信任的基礎裡（第 3 條）；交出 bits 是因為 elise 的邊和 `VMGcRef` 逐位元相同（「heap 和參照」）。wasm 呼叫 DOM 的 host function 照舊進 `mutate`：DOM 的演算法要 `Gc<'gc>` 的編譯期保證。JS 沒有對應的需要：JS 的物件在 V8 的 heap 裡，JS 進 elise 只經過 DOM 的 binding。對照 V8：wasm 的 `struct.new` 由 JIT 的碼或 runtime 的 stub 直接配置，不經過 Blink；cppgc 的低階入口在 `subtle`。glue 怎麼拿到 heap 留到第 2 段。不選每次配置都經過 `mutate`（乙）：每個物件付一次固定成本，而且它的 safepoint 會自己開始 GC，違反第 1 點。
12. **1c 的實作**（2026-11-01，實作時定的，使用者審核確認）：
   1. 名字：frame 的狀態是 `Heap::enter_frames`、`leave_frames`（計數，可以巢狀：wasm 呼叫 DOM、DOM 的 listener 又進 wasm）、`in_frames`；交出 root 的 GC 是 `safepoint_with_roots`（frame 外的安全點這時會做的）、`scavenge_with_roots`、`collect_with_roots`；不經過 `mutate` 的入口是 `raw::alloc_bits`、`alloc_array_bits`、`read_bits_in`、`write_bits_in`。frame 裡呼叫 `Heap::collect`、`scavenge` 會 panic（看不到 frame 的 root）。
   2. 走訪函式（第 11 條第 2 點）被呼叫兩次：先讀出每個 root 的 bits，GC 做完再寫回，所以契約多一條「兩次交出同樣的 root、同樣的順序」（數目不同時 panic）。不選存下 `&mut u32` 的位址之後寫回：要在 elise 裡經過裸指標寫 embedder 的格子，契約一樣長，多一處 `unsafe` 的寫。wasmtime 的 `GcRootsIter` 只走一次，銜接層先收成一份清單再交（第 2 段）。
   3. 配置失敗分兩種，回 `Result<u32, AllocError>`，不是第 11 條第 3 點寫的 `None`：`GcDue`（engine 收齊 root、GC 之後重試）和 `TooLarge`（陣列太大，先檢查；engine 直接 trap），對應 wasmtime 的 `GcHeap::alloc_raw` 的兩種失敗。只有 `None` 的話，太大的陣列會讓 engine 白做一次 GC 才知道。
   4. 交出 root 的 GC 之後緊接著的那一次 bits 的配置不再看 GC 該不該做：wasmtime 在 GC 之後只重試一次，失敗就 trap；`stress` 下 GC 每次都該做，不這樣的話永遠配置不到。
   5. marking 的收尾不從 frame 的 root 標：SATB 下 frame 在開始之後拿到的物件，是快照裡的、新配置的或 young 的，和之後加的 `Persistent` 一樣（TLA+ 的模型裡拿掉刪 root 的 barrier 也不漏標）。收尾仍然只在交出 root 的地方做（第 11 條第 1 點），verifier 從它們走。對照 V8：它的 barrier 是 Dijkstra，收尾時要重掃 stack。
   6. 第 10 條第 3 點留到 1c 的「`u32` 變回 `Gc`」是 `raw::from_bits`（`unsafe`）：在一次操作裡把 frame 拿著的 bits 變回 `Ref`（wasm 傳給 host function 的參數），之後照一般的 `Gc` 用、要留過操作就建 `Persistent`。操作裡不會 GC，所以 bits 在整個操作裡有效。
13. **第 2 段：fork 的 runtime 那一半**（2026-11-01，使用者逐條確認）：
   0. 現在開始（使用者決定）：要知道 wasmtime 能不能整合 elise、和 DOM 共用一個 heap 可不可行，做完能用就讓 kun 用。原本延後的理由（還沒有能編出 wasm GC 加上 WASI 0.3 component 的語言，做完只有測試會用到）照舊成立：2026-11-01 查的，MoonBit 的 component 用 linear memory 的後端，Kotlin/Wasm 的 component 還是實驗，Dart 沒有 component；GC 版的 canonical ABI 還沒有（Component Model 的 [issue #525](https://github.com/WebAssembly/component-model/issues/525)、wasmtime 的 [issue #10325](https://github.com/bytecodealliance/wasmtime/issues/10325) 還開著，上游 main 的 #15043 只做了純量）；component 裡的 core module 自己用 wasm GC 不受影響（2026-11-01 在 49.0.2 上跑過一個用 `struct.new` 的 component）。開始時再確認的兩件事：上游還沒有可以替換的 collector，`details.md` 的結論是在 50.0.1 上查的。
   1. fork 的基底是 v50.0.1（甲）：當時最新的 release，有 #14885、#14353 這些 GC 介面的改動。fork 的 `main` 不動，另外開 `kun` 分支。kun 在 49.0.2，第 4 段接上之前要另外升級到 50。不選 v49.0.5（乙）：GC 的介面是舊的，51 出來時要跨兩個版本 rebase；不選上游的 main（丙）：一直在變、沒發布，kun 也不能釘。對照 Chromium：DEPS 釘上游 main 的 revision，但不長期留 patch；我們的 fork 要 rebase，以 release 為單位範圍清楚。
   2. 第 2 段只做 runtime，經過 host 的 API 測（甲）：`StructRef`、`ArrayRef`、`ExternRef`、`AnyRef::from_i31`、GC 參照型別的 `Global` 和 `Table`、`Rooted` 和 `OwnedRooted`、`Store::gc`，分成 2a（不 GC 的部分）和 2b（GC）。沒有 `GcCompiler` 時用到 GC 指令的 wasm 碼不能用這個 collector 編，spec test 等第 3 段。不選第 2、3 段合成一段（乙）：一次要做、要審的很大，出錯時分不出是哪一半。對照 V8：新的 heap 能力先在 C++ 的 unit test 直接呼叫 API，之後才接 JIT 的碼。
   3. 兩個 header 都留（甲）：elise 的 header 在前，資料的開頭是 wasmtime 的 `VMGcHeader`（對 elise 只是資料的前 8 byte），`VMGcRef` 等於 elise 的資料的 offset、和 elise 的邊的 bits 一樣。elise 不用改，wasmtime 的 layout、產生的碼、runtime 照原樣讀 header；代價是每個 wasm 物件多 8 byte。合成一個留在「待決定」，量過再定。對照 V8：wasm struct 只有一個 map word，Oilpan 的物件也只有一個 header。
   4. elise 的 cage 就是 GC heap（甲）：`attach` 收到的 `vm::Memory` 只拿著，`detach`、`take_memory` 時還回去，wasmtime 的 GC heap 設定調到最小；`vmmemory()` 回 cage 的基底，長度 4 GiB（32 位元的 ref 都在範圍內，產生的碼的邊界檢查照舊成立）。`heap_slice()` 的預設會在整個 cage 上建 `&[u8]`，大部分是沒 commit 的保留區，是 UB：讀寫物件的 `gc_object_data(_mut)`、`index(_mut)` 覆寫成只切那個物件，`gc.rs` 的 `gc_heap_capacity` 用 `kun:` 改成不經過整片的 slice（上游的檔案只改這一處）。不選把 elise 的 cage 建在 wasmtime 的記憶體裡（乙）：elise 的保留、commit、discard 和頁表都要改，第 4 段還要改回來。對照 V8：wasm 物件在 V8 的 pointer compression 的 cage 裡，JIT 的碼用「基底 + 32 位元 offset」讀，形狀一樣。
   5. 第 2、3 段的 heap 預設用 `InlinePlatform`，沒有背景的 thread（甲）；測試另外用一步一步走的 platform（elise 的 `concurrent.rs` 的 `Steps` 的做法）：marking 跨過 wasm 碼和 host API 的寫入，SATB 和分代的 barrier 在第 2、3 段就測得到，marker 只在測試呼叫 `step()` 時跑，那時 runtime 手上沒有 `&mut`。真的背景 thread 和 runtime 的 `&mut` 的 aliasing 留到第 4 段（「待決定」）。不選現在就開背景的 thread（乙）：要改 runtime 讀寫物件的方式、碰上游的好幾個檔案，第 4 段面對 DOM 的 concurrent marking 時本來就要做，做一次就好。對照 V8：C++ 讀寫 heap 的欄位一律經過 `TaggedField` 和 relaxed atomic，是第 4 段的方向。
   6. fork 用 path 依賴 elise（`../elise`，甲）：兩邊在第 2、3 段常一起改，改 elise 馬上生效；kun 之後換成 elise 時三者指向同一份。fork 的 `README.machopo.md` 寫明配合 elise 的哪個 commit。不選 git 的 rev（乙）：每改一次 elise 就要改 fork 的 rev 和 lock，kun 用 path 時同一個 build 有兩份 elise；不選發版本（丙）：`raw` 的 API 還會改。對照 Chromium：`third_party` 經過 DEPS 放在固定位置，build 用相對路徑互相找到。
   7. 第 4 段 kun 開始依賴 fork 時，才把 fork 和 elise 一起加進 `third_party.toml`（甲）：第 2、3 段只在 fork 的 `kun` 分支和 elise 裡做，fork 的測試在本機跑，kun 只在 `progress.md` 記進度和 commit。「fork 每多一個 commit 就更新 `third_party.toml`」是給 kun 依賴的 fork 的，這段期間不適用。RFC 做完之後再看 repo 之間改成什麼穩定的方式（「待決定」）。不選現在就釘（乙）：kun 不 build 它，每個 commit 多一個 kun 的 commit，kun 的 CI 也不跑它的測試。對照 Chromium：DEPS 裡只有 build 和測試真的用到的東西。
   8. 名字照上游的命名（甲）：`Collector::Elise`、cargo feature `gc-elise`、新增的檔案叫 `elise.rs`（runtime 在 `runtime/vm/gc/enabled/`，compiler 在 `cranelift/src/func_environ/gc/enabled/`）。不選 `Kun`（乙）：collector 是 elise，fork 的 `kun:` 註解和 `README.machopo.md` 本來就標出我們改的地方。
14. **2a 的做法**（2026-11-01，使用者逐條確認）：
   1. 型別在 Engine 的 `TypeRegistry` 登記時一起登記到 elise（甲）：`GcRuntime` 加 `register_type`、`unregister_type`（預設什麼都不做，上游的 collector 不受影響），`type_registry.rs` 的 `insert_entry_gc_layouts`、`remove_entry_gc_layouts` 用 `kun:` 呼叫它們。每個 Engine 的 elise runtime 有一個 `TypeRange`，elise 的 id 由 runtime 自己的 free list 發（退休中的不重用），`VMSharedTypeIndex` 對到 elise 的編號的表放在 runtime，heap 照 DRC 的 `TraceInfos` 留一份快取。unregister 時 retire 是安全的：Store 拿著它用到的型別（module 的和 `gc_host_alloc_types`），物件活著時型別不會 unregister。`externref` 的物件是 runtime 建立時登記的一個固定的型別。不選第一次配置時才登記、不 retire（乙）：型別到 Engine drop 才放，`VMSharedTypeIndex` 重用時快取拿到舊的 layout；不選 elise 的 id 直接用 `VMSharedTypeIndex`（丙）：slab 重用編號時 elise 那邊可能還在退休中（`Occupied`），module 會隨機登記失敗。對照 V8：wasm 的型別在 process 裡 canonicalize（`TypeCanonicalizer`），每個 isolate 給每個型別建 `Map`、`WasmTypeInfo`。
   2. 寫物件的參照欄位經過拿得到物件和 offset 的 barrier（甲）：`GcHeap` 加 `write_gc_ref_in(object, offset, source)`、`init_gc_ref_in`，預設照上游原本的做法（讀出舊值、`write_gc_ref`、寫回），`gc_ref.rs` 的 `write_val`、`initialize_val` 寫參照的六個分支用 `kun:` 改成呼叫它們；elise 用 `raw::write_bits_in`，SATB 和分代的 barrier 一起做。初始化也要：frame 裡 nursery 滿了時物件直接配置到 old，寫進的 young 參照要記下來。寫 root 的（global、table、`pending_exception`）照舊經過 `write_gc_ref`，elise 只寫入：它們在每次交出 root 的 GC 時給，和 frame 的 root 一樣。上游只有這兩個函式把參照寫進物件；wasm 的 `struct.set`、`array.copy` 這些由 cranelift 產生，是第 3 段的 `GcCompiler`。不選把欄位的位址交給 collector（乙）：要建指到 heap 裡的 `&mut`（第 4 段的 aliasing），elise 也沒有從位址找物件的方法；甲帶的資訊比較多，之後要照位址記時 elise 自己算出位址就好。不選 wasm 的物件不分代（丙）：吃不到 nursery，和 DOM 共用 heap 時兩邊的規則不同。對照 V8：`WriteBarrier::ForValue(host, slot, value)`。
   3. `externref` 的 host data 留在 Store 的 `host_data_table`，用 elise 的 id 知道物件死了（甲）：物件是 `VMGcHeader` 加 host data 的編號，配置時給它一個 elise 的 `Id`，heap 這一側留一張「`Id`、編號」的清單，GC 之後走一遍，找不到物件的就 `dealloc`（2b）。elise 要加不經過操作的 `raw::new_id_in` 和查 id 的方法；wasmtime 的 `ExternRef` 的 API 不變。成本是每次 GC 走一遍活著的 `externref`，和 copying 一樣。不選 host data 搬進 elise 的物件、執行時的型別加 finalizer（乙）：要改上游的 `ExternRef::data()`、`data_mut()`，finalizer 在 sweep 的 thread 跑；不選 sweep 時把死掉的記進佇列（丙）：elise 多一套通知和加鎖的佇列，`externref` 多到甲太慢時再換，物件的形狀不變。第 4 段 `externref` 很可能直接是指到 DOM 節點的邊（「待決定」），那由 header 怎麼合併決定，三個選項都不往那裡走，甲到時留下的最少（清單只剩 Rust 的 host 值）。對照 V8：`externref` 是 heap 裡的任何 JS 值，C++ 的資料用 `Managed<T>`。
   4. pooling allocator 下，`detach` 時 drop elise 的 `Heap`，`attach` 時建新的（甲）：pooling 把 `Box<dyn GcHeap>` 留給下一個 Store，elise 的 heap 裡還有上一個 Store 的物件。結果正確、elise 不用改、上游用 pooling 的測試照跑，代價是拿不到 pooling 的速度（每個 Store 重新保留 cage 的位址空間）；kun 用的是 on-demand。不選 elise 加 `Heap::reset`（乙）：多一條路和契約，和 DOM 共用 heap 之後清空整個 heap 不成立，現在沒人需要；不選不准用 pooling（丙）：上游用 pooling 的測試要另外跳過。對照 Chromium：isolate 和 Blink 的 worker 都建新的 heap。
   5. 上游的測試換成 elise 跑，加上自己的測試（甲）：fork 的整合測試的 `main.rs` 裡的 `gc_engine()`（`structs`、`arrays`、`gc`、`exnrefs` 的測試大多經過它）用 `kun:` 讀 `WASMTIME_TEST_COLLECTOR=elise`，換成 `Collector::Elise`；fork 裡放一份預期失敗的清單，每一條寫明原因（要 GC 的 2b 拿掉，要編 GC 指令的第 3 段拿掉），一個小腳本把它轉成 `--skip`，也標出清單上開始通過的。elise 才有的（型別的登記和退休、frame 裡配置到 old 的 barrier、`externref` 的 id、pooling 下重建 heap、用一步一步走的 platform 讓 marking 跨過 host API 的寫入）寫在 fork 的整合測試的新模組 `elise.rs`。mutation test 照 1c 的做法用腳本弄壞 fork 的 `elise.rs` 和改到的上游函式；Miri 只跑 elise 新加的 raw API 和 fork 裡幾個 host API 的測試，規模照 1c 縮小之後的。不選只寫自己的測試（乙）：上游上百個 host API 的行為要重寫或沒人守；不選讓 `Collector::Auto` 選 elise（丙）：改的是函式庫的預設，第 3 段之前用到 GC 指令的都失敗。對照 V8：同一套 cctest、unittest 用不同的 GC 旗標（variant）跑。
   6. elise 加 `raw::EngineHeap`，fork 拿著它（乙）：wasmtime 要 `GcHeap` 是 `Send + Sync`（`Store` 可以換 thread），elise 的 `Heap` 兩者都不是。`EngineHeap` 包著一個 `Heap`，不借出它、不跑操作（`mutate`），所以不留下要在它的 thread 上 drop 的東西（`Persistent` 的 root 表、`LocalExternal` 的 finalizer）；`&self` 的方法只讀 cage 的基底、frame 的計數、用量（只有 `&mut self` 寫的 `Cell`）和物件的 bytes，其他的要 `&mut self`。`Send`、`Sync` 的論證寫在 elise 的 soundness 文件，和它依賴的內部細節放在一起。不選 fork 自己包一層、`unsafe impl Send` 和 `Sync`（甲）：論證依賴 elise 私有的細節（哪些 `Cell` 由誰寫、操作留下什麼），elise 改了內部時 fork 不會知道，fork 也多一處 `unsafe`；不選拿掉 wasmtime 對 `GcHeap` 的 `Send + Sync` 的要求（丙）：`Store` 不再是 `Send`，上游的 API 和很多程式都要改。對照 V8：isolate 一次只給一個 thread 用，在 `v8::Locker` 下可以換 thread，wasmtime 的 `Store` 也是。
   7. 實作時定下的細節（2026-11-01）：
      1. 取消登記型別的 `remove_entry_gc_layouts` 拿不到 GC runtime：`TypeRegistry` 建立時拿著 Engine 的 GC runtime 的 `Arc`（`kun:`），不把參數一路傳進去（呼叫它的地方很多）。
      2. `gc_heap_capacity` 經過 `GcHeap` 新加的 `capacity()`（預設是 `heap_slice()` 的長度，上游的 collector 不變），elise 回 cage 的大小：第 13 條第 4 點的「上游的檔案只改這一處」，多了 `gc_runtime.rs` 的這個預設方法。elise 的 `heap_slice` 是 `unreachable!`：用到它的預設方法裡，`gc_object_data(_mut)` 覆寫了，`index(_mut)` 只在各 collector 自己的程式裡用（`Self: Sized`）。
      3. elise 信任 heap 裡的 GC 參照，和它的 GC 一樣：銜接層只檢查參照在 cage 裡、8 byte 對齊，不像上游的 collector 照 heap 的 slice 檢查物件的邊界。壞掉的參照讀寫的是 cage 裡的別處（別的物件，或沒 commit 的頁、存取違規），不出 cage；這和產生的碼的 4 GiB 邊界檢查一樣，在信任的基礎裡（第 3 條）。kun 用它時記進限制。
      4. 預期失敗的清單（第 5 點）：腳本跑那幾個模組、把結果和清單比對，清單外的失敗和清單上開始通過的都算失敗，不轉成 `--skip`：開始通過的要跑了才知道。模組是測試經過 `gc_engine()` 或 `gc_store()` 的 `structs`、`arrays`、`gc`、`exnrefs`、`eqref`。
      5. Winch（另一個 compiler）也 match `Collector`：用 `kun:` 讓它拒絕為 elise 編譯（建立 codegen 時回錯誤），不只在用到 GC 指令時。kun 用 Cranelift，Winch 不需要 elise。
15. **2b 的做法**（2026-11-02，使用者逐條確認）。root 的交接照第 12 條第 2 點（`GcRootsIter` 先收成一份清單、交給 elise 兩次、GC 之後用 `GcRoot::set` 寫回），`externref` 的 host data 照第 14 條第 3 點：
   1. `GcHeap::gc` 看 elise 的 `should_collect`（甲）：要做 GC 時呼叫 `safepoint_with_roots`，elise 照自己的排程做 scavenge、marking 的開始或收尾、完整的 GC；不需要時（embedder 自己呼叫 `Store::gc`）呼叫 `collect_with_roots`，做完整的 GC。上游的介面不變，配置觸發的 GC 吃得到分代，embedder 叫的確實回收；GC 剛好該做時 embedder 叫的只做 elise 排到的那一種。一次 `collect_increment` 做完：elise 的 GC 是一次呼叫，跨安全點的 marking 由 elise 排。不選一律 `safepoint_with_roots`（乙）：embedder 叫的 GC 可能什麼都不做；不選一律 `collect_with_roots`（丙）：nursery 每次滿了都走過整個 heap；不選 fork 把原因傳給 `GcHeap::gc`（丁）：要改 trait、每個 collector 和兩個呼叫的地方，換來的只有甲的那個邊角。對照 V8：配置失敗時 heap 自己選（`Heap::SelectGarbageCollector`），`--expose-gc` 的 `gc()` 和 Blink 的 `CollectAllGarbageForTesting` 一律完整。
   2. elise 的 `HeapOptions` 和 `Platform` 經過 fork 的 `Config` 的 `kun:` 方法給（甲，只在 `gc-elise` 下）：`build_gc_runtime` 交給 collector，每次 `attach` 用它建 heap，沒設定時是預設的選項和 `InlinePlatform`。上游的測試用 `WASMTIME_TEST_COLLECTOR=elise` 時開 verifier，`elise-stress` 再加 stress，預期失敗的清單兩種共用；fork 自己的測試用一步一步走的 platform。第 4 段 heap 改成文件的時，這個方法會換掉，所以做成最小。不選 fork 讀環境變數（乙）：函式庫裡看不到的行為，也給不了 platform；不選放在 `Collector::Elise` 的資料裡（丙）：`Collector` 是 `Copy`、序列化、算進編好的 module 的相容性；不選 wasmtime 的 gc_zeal（丁）：要另一組 `RUSTFLAGS` 重編，給不了 verifier 和 platform。對照 cppgc：`cppgc::Heap::Create(platform, options)`。
   3. 進出 wasm 的 frame 的計數留到第 4 段（甲）：2b 的 heap 沒有 DOM 的操作、沒有不交 root 的 GC，bits 的配置也不看 frame，接不接行為都一樣；第 4 段 wasm 呼叫 DOM 時計數才有作用，也才測得到。不選現在就在上游的呼叫路徑上接（乙）：每次呼叫多一次計數，只測得到計數本身，第 4 段的位置可能還要換。對照 V8：GC 時走 stack、用 safepoint table 找 frame，沒有計數；計數是 kun 不做 conservative 掃描才要的（第 11 條第 1 點）。
   4. GC 之後還是配置不到時，2b 不做上限（甲）：cage 滿了 elise 照舊 panic，store 的 `ResourceLimiter` 管不到 elise 的 heap，一個 guest 可以用到 4 GiB。上限的對象是第 4 段文件的 heap，超過時 DOM 的操作沒辦法 trap，要和 kun 整體的做法一起定（「待決定」）。不選現在在 elise 加上限、超過時回錯誤讓 wasmtime trap（乙）：第 4 段多半要重新設計；不選 elise 長大時問 `ResourceLimiter`（丙）：elise 拿不到 store，limiter 是 async 的。對照 V8：超過上限時先 `NearHeapLimitCallback`，再不行 `FatalProcessOutOfMemory`，renderer 當掉、不 trap。
16. **第 3 段：`GcCompiler` 的做法**（2026-11-02，使用者逐條確認）。產生的碼寫 GC 參照只經過 `GcCompiler` 的三個方法，都只拿到格子的位址（struct 和 array 的欄位、table、global 共用 `translate_write_gc_reference`；`init_field` 只給新物件的欄位；`translate_init_gc_reference` 只給 table、global 和 elem segment 的初始化），elise 的 barrier 本來就照格子的位址算（`write_barrier`、`satb`），所以不需要物件。這一段還是沒有背景的 thread（第 13 條第 5 點），產生的碼用一般的 store；第 4 段有 concurrent marking 時和「runtime 的 `&mut` 和背景的 marker」一起看（「待決定」）：
   1. barrier 內嵌判斷，少見的情況才呼叫 libcall（甲）：elise 交出一塊 `#[repr(C)]` 的狀態（marking 的旗標、young 的頁的表），fork 的 `vmctx_gc_heap_data` 回傳它；沒在 marking 時 SATB 只讀一個旗標，marking 期間舊值經過 libcall；分代的 barrier 在新值不是 0、不是 `I31`、在 young 的頁而格子不在時，才呼叫 libcall 記進 slot set。fork 加兩個 builtin 的 libcall。不選每次寫參照都呼叫 libcall（乙）：每次寫入多 5 到 10 ns，之後要改成甲時產生碼的部分要重寫；不選連記進 slot set 都內嵌（丙）：產生的碼要知道 slot set 的格式（大的物件往前找第一頁），收益小。對照 V8：內嵌檢查頁的旗標，再呼叫 `RecordWrite` 的 stub；V8 的旗標在頁的開頭，elise 查表。
   2. heap 裡的寫入和 root 的寫入在編譯時分開（乙）：`GcCompiler` 加 `translate_write_gc_field`，預設呼叫 `translate_write_gc_reference`，寫欄位的那一處用 `kun:` 改成呼叫它；elise 只在這裡放 barrier，table 和 global 的寫入只寫入（root 在 marking 開始和收尾、每次 GC 都交出，同第 14 條第 2 點的 runtime）。沒有執行時的成本；代價是上游以後多一條寫欄位的路時，elise 會少 barrier 而沒有任何錯誤，所以每次 rebase 都要找一次寫欄位的路（記在 fork 的 `README.machopo.md`）。不選執行時看位址在不在 cage 裡（甲）：每次寫入多一個減法、比較和分支，換來的是不靠每條路都接到。對照 V8：TurboFan 的 `StoreField` 帶著 `WriteBarrierKind`，寫 stack 和 root 不產生 barrier。
   3. 配置經過 `gc_alloc_raw` 的 libcall（RFC 的「barrier 和配置」），`init_field` 寫參照時放分代的 barrier、不放 SATB（甲）：新物件的欄位原本是 0，不需要 SATB；但大的 array 直接配置在 old，第 4 段 frame 裡 nursery 滿了時也是，初始化成指向 young 的值要記進 slot set。和第 1 點同一段判斷，對錯不靠物件怎麼配置出來；null 和 `I31` 的初始值只多一個比較。不選每個物件判斷一次在不在 young（乙）：多一組分支和兩條要測的路，是量過之後的優化；不選 elise 配置到 old 時把整個物件的邊記進 slot set（丙）：大的 array 的配置變成 O(n)，scavenge 白走記下的格子。對照 V8：初始化預設有 barrier，TurboFan 證明物件剛在 young 配置時才拿掉（write barrier elimination）。
   4. 測試（甲）：測試用的 `Collector`（fork 的 test-util）用 `kun:` 加上 elise，GC 相關的 wast（GC、function references、exceptions）用 verifier 和 stress 兩種、Cranelift 和 Pulley 跑，預期失敗的清單每一條寫明原因；上游整合測試的預期失敗清單清空。fork 的 `elise.rs` 用 WAT 打到每一種 barrier（`struct.set`、`array.set`、`array.copy`、`array.fill`、大的 array 的初始化寫 old 指向 young，marking 中 wasm 覆寫參照，scavenge 搬動之後 stack 上的參照，table 和 global 不經過 barrier 照樣是 root）。`gc_ops` 的 oracle 和 elise 對 copying 的差分做成固定種子的一般測試，mutation test 抓得到；長時間的 cargo-fuzz 和 fork 的 Miri 一樣，等 macOS 或 Linux。不選 fuzz 留到第 5 段（乙）：barrier 的錯要特別的寫入順序和 GC 的時機才碰得到，差分是銜接層主要的防線；不選在 Windows 上跑真的 cargo-fuzz（丙）：libFuzzer 在 Windows MSVC 上的支援不確定。對照 V8：mjsunit 的 wasm 測試加上 `--stress-incremental-marking`、`--stress-scavenge` 再跑，長時間的 fuzz 交給 ClusterFuzz。
17. **第 4 段的分法：先做兩段不改行為的前置，再接 heap**（甲，2026-11-02，使用者決定）：4a kun 換成 elise（先把 elise 分出去之後 kun 的 `gc` crate 的三次改動同步進 elise，kun 的 `gc::` 改寫成 `elise::`、刪掉 `gc` 和 `gc_macros` 這兩個 crate）；4b kun 的 wasmtime 和 wasmtime-wasi 改用 fork 的 50.0.1，`third_party.toml` 加上 fork 和 elise，collector 照舊是預設的；4c runtime 的 `&mut` 和背景的 marker（「待決定」）；4d wasm GC 的物件配置在新的 DOM 的 heap 上（一個 heap 給好幾個 Store、兩個方向的 root、進出 frame 的計數）；4e heap 的上限和收尾。kun 的改動在 branch `0005` 上做（使用者要求），每一小段審核過就 fast-forward merge 回 `main`：RFC 0089 同時在改同一批 crate，早點 merge 讓它接著在 elise 和 wasmtime 50 上做，衝突只發生一次。不選等 RFC 0089 做完再做（乙）：0005 要停下來，elise 和 kun 的 `gc` crate 越分越遠；不選一次做完、最後才 merge（丙）：出錯時分不出是改名、升版還是 GC，和 0089 的衝突越積越多。對照 Chromium：Oilpan 從 Blink 搬進 V8 成為 cppgc 時，也是先搬、改名、行為不變，之後才接 unified heap。
18. **4a：kun 換成 elise 的做法**（2026-11-02，使用者逐條確認）。elise 已經有它分出去之後 kun 的 `gc` crate 的三次改動（2026-11-02 查過），不用另外同步：
   1. kun 用 path 依賴 `../third_party/elise/crates/elise`，`third_party.toml` 釘住 elise 的 commit（甲）：和 parley、rusty_v8 一樣由 `ci.py third-party` 同步到釘的 commit，fork 的 `gc-elise` 的 path 依賴指向同一份，4b 之後同一個 build 裡只有一份 elise。決定 13 第 7 點的「第 4 段一起釘」改成 4a 先釘 elise、4b 再釘 fork。elise 每多一個 commit 就在同一次改動裡更新 `third_party.toml`，先 push elise、再 push kun。不選 git 的 rev（乙）：kun 用 git 的、fork 用 path 的，同一個 build 有兩份 elise，改 elise 時也不能直接在 checkout 裡試；不選發布到 crates.io（丙）：`raw` 的 API 在第 4 段還會改，每改一次就要發版本。對照 Chromium：第三方的程式經過 DEPS 放在固定的位置、用相對路徑 build。
   2. GC 自己的測試全部交給 elise（甲）：kun 刪掉 `gc` 的 fuzz target 和 `test.py` 的 `gc-models`、`gc-fuzz`、`gc-verus`（連同 Verus 的版本），改 elise 時跑 elise 的 `scripts/test.py`；kun 留下 `gc_bench`（包括給 elise 的 fuzzer 做種子的 `fuzz-seeds`）、V8 的 `matrix` 和 `fuzz`（`dom`、`bindings`、`html` 和 V8 一起的 GC），和用到 elise 的 crate 的測試。釘的 commit 由 elise 的 CI 守（每個 push 到 `main` 的 commit 都跑三個平台的測試、loom 和 shuttle、Verus、TLA+ 和 Miri），所以只釘 elise 的 `main` 上推過的 commit。不選 kun 留著指令、轉呼叫 elise 的腳本（乙）：多一層要維護的轉接；不選 kun 的 CI 再對釘的 commit 跑一次 elise 的測試（丙）：和 elise 的 CI 重複。對照 Chromium：cppgc 的測試在 V8 的 CI 跑，Chromium 只跑 Blink 那一側用到它的。
   3. GC 本身的文件只在 elise，kun 只寫 kun 怎麼用它（甲）：`docs/test-gaps.md` 裡講 GC 內部的條目搬進 elise 的 `docs/testing.md` 新的一節「測試守不住的地方」；kun 的 `gc` crate 的 README 和 soundness 逐段對照過再刪（GC 本身的搬進 elise，kun 的 DOM 怎麼用它的搬進 `dom_next` 的 README）；`docs/architecture.md` 的 crate 表拿掉 `gc`、`gc_macros`，第三方的表加 elise、拿掉只有 GC 用的 Verus、libfuzzer、loom 和 shuttle；`crates/dom/soundness.md` 和 `docs/README.md` 指向 elise 的 soundness；`test-gaps.md` 裡 kun 自己的條目留著、改名字。`CLAUDE.md`：讀 elise 的 `docs/` 再改它，`unsafe` 的 crate 拿掉 `gc`，「第三方的 fork」加上 elise（不是 fork、用 `main`，改了先 commit、push elise 再更新 pin，改 GC 時跑 elise 的 `scripts/test.py` 和 `sanity.py`）。`sanity.py` 的清單跟著改。不選測試的缺口照舊留在 kun（乙）：條目寫的是另一個 repo 的程式，elise 改內部時沒人跟著改，elise 的其他貢獻者也看不到。對照 Chromium：cppgc 的設計和測試的文件在 V8，Blink 的文件只寫怎麼用 Oilpan。
   4. RFC 的 `progress.md` 裡舊的路徑和指令（丙，2026-11-02）：`sanity.py docs` 把每份 RFC 的 `progress.md` 當成歷史紀錄，不檢查它的路徑和測試的指令。`progress.md` 的「做過的檢查」記的是當時跑了什麼、在哪個檔案上，改寫成現在的路徑反而失真。不選列一張搬走的路徑的對照表、讓檢查查表（甲）：每次搬檔案都要記得加，表只會越來越長；也不選把舊的紀錄改成現在的路徑（乙）：改的是過去發生的事，當時 kun 的 `gc` crate 上跑的 mutation test，也不是在 elise 的檔案上跑的。RFC 的 README 和 `details.md` 寫的是設計，照樣檢查。
19. **4b：kun 改用 fork 的 wasmtime 的做法**（2026-11-02，使用者逐條確認）：
   1. kun 用 path 加上版本依賴 fork 的 `wasmtime` 和 `wasmtime-wasi`（甲）：`version = "=50.0.1"`、`path` 指向 `../third_party/wasmtime/crates/wasmtime` 和 `../third_party/wasmtime/crates/wasi`，和 `v8` 的寫法一樣，`third_party.toml` 釘 fork 的 `kun` 分支的 commit。fork 自己的 crate（`wasmtime-environ`、Cranelift……）經過 fork 的 path 依賴進來，fork 的 elise 和 kun 的是同一份；版本寫死，fork 換了版本 kun 就編不過，不會不知不覺換掉。不選 `[patch.crates-io]`（乙）：好處是 crates.io 上別的套件依賴的 `wasmtime-*` 也跟著換，kun 沒有這種套件；版本對不上時 patch 不套用、cargo 只印警告，會不知不覺用回 crates.io 的。不選 git 的 rev（丙）：fork 用相對的 path 依賴 elise，cargo 把 git 的依賴抓到別的地方，那裡找不到 elise；找得到也會有兩份（決定 18 第 1 點）。對照 Chromium：DEPS 釘住 `src/third_party` 底下的 checkout，GN 從那個路徑編。

   2. 4b 就開 fork 的 `gc-elise` feature，collector 照舊是預設的（甲）：`Collector::Auto` 依序選 copying、DRC、null，不選 elise，要用 elise 得在 `Config` 裡明確指定，所以 kun 的行為不變。kun 每次 build 都連 fork 接 elise 的那一層（runtime、Cranelift、environ 各一個 `elise.rs`，約 1400 行）一起編，kun 移動 elise 的 pin 時，elise 的 `raw` API 改了、fork 編不過，kun 的 build 和 CI 馬上看得到；4d 配置在 elise 上時只改 `Config`，不改依賴。不選 4d 才開（乙）：4b 到 4d 之間移動 elise 的 pin 不會檢查 fork 還編得過，兩邊對不上的地方到 4d 才一起出現。對照 Chromium：新功能先編進 build、用 `base::Feature` 這類執行時的開關關著，要用時打開。

   3. fork 自己的測試照舊在本機跑（甲）：改 fork 時跑 `cargo test --features gc-elise --test all`、`machopo/elise_tests.py` 和 clippy，改 elise 的 `raw`（fork 用的那一層）時也跑 fork 的 `machopo/elise_tests.py`，寫進 `CLAUDE.md`；kun 的 CI 只經過 kun 的 build 和 `dom_wasm`、`dom_wasm_next` 的測試用到 fork。4d 之前 kun 不用 elise 當 collector，fork 和 elise 配在一起的行為只有 fork 的測試看得到，靠人記得跑；編不過的話 kun 的 build 先看到（第 2 點）。之後怎麼做和「RFC 做完之後 repo 之間的依賴」（「待決定」）一起看。不選 kun 的 `test.py` 加一個指令、交給每天的 CI（乙）：fork 的 `--test all` 是整個 wasmtime 的測試，第一次編十幾到幾十分鐘、用 fork 自己的 target 目錄，kun 的 CI 替另一個 repo 的測試負責，和決定 18 第 2 點的方向相反。不選 fork 自己的 GitHub workflow（丙）：fork 多一個上游沒有的檔案、每次 rebase 都帶著，CI 抓的是 elise 的 `main`、不一定是 kun 釘的，wasmtime 每次 push 都要編很久；4d 之前 kun 不用 elise 的 collector，現在架不划算。對照 Chromium：V8 有自己的 CI，Chromium 的 bot 不跑 V8 自己的單元測試，roll 進 Chromium 時才跑 Chromium 的測試（較像丙，RFC 收尾時再看）。

20. **4b 起暫停，等 RFC 0089 刪掉 `dom_wasm`**（甲，2026-11-02，使用者決定）：換成 fork 的 50.0.1 之後，舊的 `dom_wasm` 有 18 個測試 abort（`BUG: deferred host scope has no call context`），是上游 50 延後建立 host task（wasmtime 的 PR #14350）的 bug，fork 沒改過那裡：動態的 host 函式（`func_new_concurrent`，參數是 `Val`、`ResourceAny`）的參數裡有兩個以上的 borrow 時，`LiftContext` 一開始複製的 scope 是 `DeferredHost`，第一個 borrow 放進 host 的表時把 host task 建出來、call context 搬進它，第二個（guest 自己 own 的資源的 borrow）照舊的 scope 找不到 call context。typed 的 `Resource<T>` 不走這條路：`dom_wasm_next`（`func_wrap`）不用改 fork 就全部通過。使用者不想為了要被刪掉的 `dom_wasm` 改它（49 到 50 也要改它的 `AccessorTask`）或改 fork，所以整個 RFC 暫停，0089 刪掉 `dom_wasm` 之後 4b 只換依賴，照第 19 條做。不選先做只動 fork 和 elise 的 4c（乙）：4c 的結果要到 4b 之後才在 kun 裡用到，順序和決定 17 不同。不選同一個 build 放兩份 wasmtime（丙，`dom_wasm` 留在 49.0.2）：wasmtime 和 Cranelift 編兩份，build 和執行檔都大很多，只是過渡。試過的 fork 的修正（`ResourceTables::current_scope` 碰到舊的 `DeferredHost`、call context 已經搬走時，拿已經建好的 host task 的）修好那 18 個，沒有留下；要回報上游時照這裡的描述寫草稿。

21. **4a 重做：elise 同步 kun 之後的 GC，kun 在現在的 `main` 上重新換成 elise**（2026-11-08，使用者照建議確認）。4b 暫停的期間，kun 的 `gc` crate 在 RFC 0085、0089、0090 又改了約 1,500 行（heap snapshot 的走訪、配置的快路、`alloc_beside`、沒有 nursery 的 heap），branch `0005` 的 4a 落後 `main` 192 個 commit；暫停的原因（舊的 `dom_wasm`）也在 RFC 0089 刪掉它時沒了。
   1. elise 用三方合併同步：基底是分岔點（kun 的 `3ca13c0d`）改名過的 `gc` crate，一邊是 elise 在第 1 段之前自己的改動（marking 收尾前限額剛好用完時 ephemeron 的 value 留到停頓裡的修正、Miri 用的測試的調整），一邊是 kun 之後的改動，再疊上第 1 到 3 段的 `raw` 和 `EngineHeap`。衝突 3 處：`ops.rs` 的一句註解、`heap.rs` 的 `used_bytes` 和 `alloc_at`（都取 kun 的，`alloc_at` 的型別照舊從 `ty.info()` 讀）。語意上要接的 3 處：執行時的型別的 `TypeInfo` 沒有 size class 和 JS 的邊（只有 Rust 型別的配置讀它們，執行時的型別照大小拿 size class）；heap snapshot 的走訪走執行時的型別的邊（`raw_edges`，跳過 `I31`）；走訪讀到的型別可能已經退休，`types::entry` 的前提照樣成立（elise 的 soundness 的走訪那一節）。不選照字面直接覆蓋、再重做第 1 到 3 段：會丟掉 elise 自己的修正，重寫約 3,800 行審核過的程式和測試，換不到什麼。
   2. kun 照第 18 條的做法在現在的 `main` 上重做 4a（從 `main` 開 branch `0005`，RFC 的文件從原本的 branch 帶過來），不 rebase 原本的 4a：那時的 crate 名字（`dom_next` 這些）都改過了，衝突比重做多。
   3. 4b 不再等：現在的 `dom_wasm` 是原本的 `dom_wasm_next`（`func_wrap`），第 20 條的 bug 碰不到。

22. **4c：runtime 讀寫 GC 物件不建 `&mut`**（乙，2026-11-08，使用者照建議逐條確認）。fork 的 elise heap 交出物件的 `&mut VMGcObjectData`，SAFETY 靠的是「沒有 worker 在跑」（`InlinePlatform`）；4d 和 DOM 共用 heap 之後背景的 marker 會同時讀物件的邊，涵蓋整個物件的 `&mut` 違反它的唯一性。
   1. 讀照舊用 `&VMGcObjectData`：marker 只讀，讀和讀不是 data race。
   2. 寫也經過 `&VMGcObjectData`（乙）：它的內容從 `[u8]` 換成 `[UnsafeCell<u8>]`，讀寫的方法收 `&self`、用 raw pointer；拿掉 `&mut VMGcObjectData`（trait 的 `gc_object_data_mut`、`From<&mut [u8]>`），預設的 `gc_object_data` 從 `vmmemory()` 的基底指標建（從 `&[u8]` 轉過去再寫是 UB），`GcStore::gc_object_data` 回傳 `&` 但照舊收 `&mut self`，同時只有一個寫入的人由 store 的獨佔借用和 `UnsafeCell` 的 `!Sync` 保證。runtime 經過它的約 20 處呼叫不用改，DRC、copying、null 吃預設的實作。不選甲（trait 加寫一個欄位、一段的方法，elise 覆寫、再改 20 處呼叫）：改得多，只修找到的那些寫入；不選丙（wasm 物件所在的 heap 不做 concurrent marking）：marking 全在 main thread 上；不選丁（runtime 碰物件時暫停 marker）：每次 host 呼叫都要和 worker 同步。對照 V8：`TaggedField` 讀寫 heap 的欄位一律經過 raw pointer 和 atomic，不建參照。
   3. 論證和它的前提寫進 fork 的 `README.machopo.md` 和 elise 的 soundness：寫邊一定經過 `write_gc_ref_in`、`init_gc_ref_in`（elise 的是 atomic、帶 barrier），其他的寫入只碰不是邊的 byte；JIT（Cranelift）的一般 store 不在 Rust 的記憶體模型裡，對齊的 32 位元 store 在 x86-64、ARM64 上一次寫完，marker 的 relaxed atomic 讀看得到舊值或新值（和第 3 條同一類的信任）。先確認 `array_new_data`、`array_new_elem` 不經過 Rust 寫邊；rebase 的檢查清單加上新的寫入走 `VMGcObjectData`、寫邊走 `write_gc_ref_in`。
   4. 驗證：fork 的 elise 測試用真的 `ThreadPool`，背景的 marker 和 host 的寫入同時跑；加一組 Miri 的小測試（使用者跑）；mutation test（寫邊換成一般的寫、拿掉 `UnsafeCell` 這類）。不選只做 stress 和 mutation test：抓不到碰巧沒出事的 race。

23. **4d：wasm 的 GC 物件配置在 DOM 的 heap 上**（2026-11-08，使用者照建議逐條確認）。查的時候的兩個前提：kun 還沒有 wasm 的文件（`dom_wasm` 沒接上 renderer，一個 thread 一個 `WasmDom`）；一個 heap 給好幾個 Store 時，nursery 共用、full GC 要走每個 Store 的物件，所以每一次要 root 的 GC（scavenge、marking 的開始和收尾、full GC）都要所有 Store 的 root，DOM 的安全點也一樣。
   0. 範圍：在現在的 `dom_wasm` 上做，「文件的 heap」是 thread 的 `WasmDom` 的，`dom_wasm` 的測試驗收；接上 renderer 另外做（甲）。不選先接上 renderer、每份文件一個 DOM（乙）：範圍大很多，4d 要等它。
   1. heap 照舊是 `Dom` 的，fork 的 `EliseHeap` 不擁有 heap，每次經過 embedder 給的入口（`Config` 的 provider）借 `&mut elise::Heap`（elise 的 bits API 本來就收 `&mut Heap`）；kun 的 provider 從 `dom_wasm` 的 thread-local 借。provider 是一把鑰匙（thread 的 DOM 的序號），Store 換了 thread 就找不到、回錯誤；JIT 的碼記著 cage 和 barrier state 的位址，所以用它的 Store 不換 thread（kun 不換），fork 在進出時檢查（甲）。對照 Blink：經過 thread-local 的 `ThreadState` 找 cppgc 的 heap。不選 `Rc<RefCell<Heap>>`（乙）：計數和借用旗標在每次配置的路上、要 `unsafe impl Send`；不選 heap 由 Store 那一側擁有（丙）：沒有 Store 時 DOM 也要 heap。
   2. 其他 Store 的 root（第 25 條改成 fork 的 `StoreSet` 和 `RootsSource`）：kun 把文件的 Store 登記在 `dom_wasm` 的表裡，要 root 的 GC 向 kun 要：沒在跑的 Store 經過 fork 開放的 `Store::trace_gc_roots`（包括暫停中的 fiber），正在跑、觸發 GC 的那個由 wasmtime 交；有別的 Store 在 host 呼叫裡（借著、frame 在 native stack 上）時拿不到它的 root，不做要 root 的 GC（nursery 滿了配置到 old，回到 task 之間再做；決定 11 的延伸）。借用旗標只在 GC 時用（甲）。不選一份文件一個 Store（乙）：一個 guest trap 時 Component Model 鎖住整個 Store，同一頁的 wasm 一起停，WASI 的 context 和資源的上限也共用（對照 Chrome：一個 script 的例外不停掉別的）；不選每個 Store 一個 heap（丙，第 2 條的乙）。
   3. DOM 的安全點照舊 GC，要 root 時向第 2 點的來源要；有 Store 在跑時照決定 11 只做不要 root 的（甲）。不選 wasm 文件的 DOM 的安全點不做要 root 的 GC、只在 task 之間由 event loop 做（乙）：一個 task 裡 DOM 配置得多時都進 old。
   4. frame 的計數：`dom_wasm` 的 host 函式（wasm 呼叫進來的）做 DOM 的操作時在「frame 裡」（巢狀的計數）（甲；第 25 條改成 fork 在進出 wasm 時算）。不選 fork 在每次進出 wasm 時算（乙）：wasmtime 沒有這個掛點，要改上游的呼叫路徑。
   5. `externref` 指向 DOM 節點不在 4d，guest 照舊用 WIT 的 handle（「待決定」）。
   6. 分三段：4d-1 elise（外面的 root 的來源）和 `dom`（借出 `&mut Heap`）；4d-2 fork（provider、`trace_gc_roots`、`EliseHeap` 不擁有 heap、thread 的檢查）；4d-3 kun 的 `dom_wasm`（provider、Store 的表、host 函式的 frame、測試）。

24. **4d-2：fork 借 DOM 的 heap 的做法**（2026-11-08，使用者照建議逐條確認）。
   1. provider 用 closure 借（甲）：fork 的 `trait EliseHeapProvider: Send + Sync`，`key()` 在 attach 時給目前的 thread 的 DOM 的序號（沒有時回錯誤），`with_heap(key, &mut dyn FnMut(&mut elise::Heap))` 借 heap（鑰匙對不上時回錯誤）；kun 的 `dom_wasm` 是 `forbid(unsafe_code)`，用 safe 的程式從 thread-local 的 `RefCell<Dom>` 借。對照 Blink：經過 thread-local 的 `ThreadState` 找 cppgc 的 heap。不選 provider 回 `*mut Heap`（乙）：kun 要寫 `unsafe`。
   2. 自己擁有 heap 的模式留著（甲）：沒有 provider 時 `EliseHeap` 照舊擁有 `EngineHeap`，有時借；fork 的測試和上游那種一個 Store 一個 heap 的用法都靠它，代價是改 heap 的操作多一個 `match`。不選只留借的（乙）：fork 的測試都要自己做 provider。
   3. 唯讀的方法不經過 provider（甲）：attach 時記下 cage 和 barrier state 的位址（heap 活著時不變），elise 加從 cage 和 bits 算物件大小的 `unsafe fn`；只有配置、寫邊、`new_id`、`satb`、`remember`、GC 經過 provider。elise 把 `EngineHeap` 專有的 `cage`、`barrier_state`、`size`、`new_id`、`id_lives`、`satb`、`remember` 改成 `&Heap`、`&mut Heap` 上的 `raw::` 函式，`EngineHeap` 呼叫它們。不選全部經過 provider（乙）：每次讀欄位都多一次 thread-local 和借用旗標。
   4. thread 的檢查在進入 wasm（`invoke_wasm_and_catch_traps`）、fiber 恢復（`resume_fiber`）和 provider 的鑰匙（甲）：`GcHeap` 加 `check_thread`（預設不做事），借 heap 的 `EliseHeap` 比對 attach 時的 `ThreadId`；JIT 的碼直接讀寫 cage 和 barrier state，要在入口擋，代價是上游的兩個呼叫路徑各一行。不選只在 `GcHeap` 的方法裡檢查（乙）：擋不住 JIT。
   5. `Store::trace_gc_roots(&mut self, visit: &mut dyn FnMut(&mut u32))`（`StoreContextMut` 也有）（甲；第 25 條改成 fork 內部用、不公開）：走 GC 的同步的 `trace_roots`，每個 root 交出 bits、visit 改了寫回；kun 的來源叫它兩次（讀、寫回），中間沒有別的事，順序一樣。正在跑、觸發 GC 的 Store 由 wasmtime 交，kun 的來源跳過它（4d-3）。不選回 root 的清單（乙）：清單是指向 Store 內部的 raw pointer。

25. **共用的 heap 的記憶體安全只靠 fork 和 elise**（甲，2026-11-08，使用者確認；改了第 23 條的第 2、4 點和第 24 條的第 5 點）。寫 4d-2 時發現：照第 23、24 條，frame 的計數、其他 Store 的 root、attach 時記下的 cage 位址三件事要 kun 的 safe 的程式做對，錯了不是 panic 而是 heap 損毀（safe 的 API 用錯有 UB）；而且 elise 的 `RootsSource` 是 `unsafe trait`，`dom_wasm` 是 `forbid(unsafe_code)`，實作不了。
   1. frame 由 fork 在進出 wasm 時算（`invoke_wasm_and_catch_traps`，和第 24 條第 4 點的 thread 檢查同一個地方，經過 provider）：wasm 在 stack 上時 DOM 的安全點一定不做要 root 的 GC，`collect` panic。`dom_wasm` 的 host 函式不算（取代第 23 條第 4 點）。代價：每次進出 wasm 多一次 provider 的查找。
   2. 用借來的 heap 的 Store 由 fork 的 `StoreSet<T>` 擁有：經過 closure 借出 `StoreContextMut`（不是 `&mut Store`：換掉它會讓表裡的指標指到別的 Store），借出的期間是「在用」；拿掉就 drop。fork 自己實作 `RootsSource`：沒借出的 Store 照 GC 的 `trace_roots` 交 root，正在 GC 的那個（fork 在 GC 時記下是誰）由 wasmtime 交、跳過，其他借出中的回給不了。不在 `StoreSet` 裡的 Store 借不到 heap（attach 時回錯誤）。`trace_gc_roots` 不公開（取代第 24 條第 5 點）。對照第 23 條第 2 點：同樣是表和只在 GC 時看的借用旗標，放在 fork 裡。
   3. elise 給 heap 的存活記號（`Heap` drop 時清掉），fork 每次讀寫前檢查它和 thread（`GcHeap` 的每個方法、進入 wasm、恢復 fiber），每次經過 provider 借時比對 cage 的位址；provider 照第 24 條第 1 點是 safe 的 trait，做錯只會回錯誤。
   4. elise 跟著改：`set_roots_source` 和 `leave_frames` 是 `unsafe`（拿掉來源、少算 frame 都會讓 GC 看不到 root），`Heap` 在 frame 裡 drop 時整個 leak（cage、頁、物件都留著，wasm 的碼還在用它的 cage；原本想 abort，但 elise 的測試在 frame 裡 panic 時會整個停掉，leak 一樣是記憶體安全的）。`EngineHeap` 不跑操作，frame 只影響 `should_collect`，它的 `leave_frames` 照舊是 safe 的。
   5. 不選照原計畫（乙）：kun 要在 `bindings` 以外寫 `unsafe`，`dom_wasm` 的錯會變成記憶體損毀，Store 的生命週期也要 kun 保證。

26. **4d-3：`dom_wasm` 接上共用的 heap**（2026-11-08，使用者照建議逐條確認）。
   1. `StoreSet` 由使用的一方擁有（甲）：`dom_wasm::configure(&mut Config)` 設好 collector、provider 和 GC 的 feature，`WasmDom::store_set::<T>` 建這個 thread 的 DOM 的 set，`WasmStore` 照舊在 Store 的 data 裡，呼叫經過 `StoreSet::with`；`dom_wasm` 不必知道 `T`，之後 renderer 每份文件擁有自己的 set。對照 Chrome：V8 的 isolate 擁有 context，Blink 在 context 上掛自己的資料。不選 `dom_wasm` 包一層 `WasmStores<T>`（乙）：thread-local 要放 `Box<dyn Any>`，API 大。
   2. host 呼叫裡 DOM 該 GC 時經過 Store 的 GC（甲）：Store 借出期間 heap 在 frame 裡，host 函式的 DOM 操作的安全點不 GC；做完 DOM 操作後 heap 說該 GC（`should_collect`）時呼叫 `StoreContextMut::gc`，wasmtime 交這個 Store 的 stack 上的 root，set 的來源交其他的，DOM 的物件一起收；別的 Store 也借出中時不做。C 的 guest 沒有 GC heap，它的 GC 由 fork 直接請 heap 做、不交 root（沒有 GC heap 的 Store 沒有任何 GC 參照）。對照 Chrome：V8 在任何配置的地方都能 GC（它掃 stack），這裡由看得到 stack 的 wasmtime 做。不選只在呼叫之間收（乙）：一個呼叫裡 DOM 的配置都進 old。
   3. 測試在同一個 set 裡放 C 的 guest 和 WAT 的 core module（用 GC 型別、物件在 global 裡）的 Store（甲）。不選手寫用 GC 又 import `kun:dom` 的 WAT component（乙）：等有能產生它的工具鏈。

27. **4e：heap 的上限和收尾**（甲，2026-11-08，使用者照建議確認）。wasm 文件的 heap 不加上限：cage 滿了 elise panic，和 JS 文件的 DOM 的 heap 一樣（kun 的 DOM 的 heap 整體都沒有上限），記進 `docs/limitations.md`；renderer 拆成 process（RFC 0004）時和 kun 整體的做法一起定，那時用完的只停掉那個 renderer。對照 Chrome：V8 超過上限時先 `NearHeapLimitCallback`，再不行 `FatalProcessOutOfMemory`，renderer 當掉、不 trap。不選 elise 加上限、超過時 bits 的配置回錯誤讓 wasm trap（乙）：DOM 的配置不能失敗，只保護一半，上限設多少也要等接上 renderer 才知道；不選 elise 長大時問 Store 的 `ResourceLimiter`（丙）：一個 heap 對好幾個 Store，limiter 是 async 的。收尾：「待決定」的「一個 heap 給好幾個 Store」由第 25、26 條解決；`limitations.md`、`architecture.md` 補上共用的 heap；第 4 階段做完跑一次 `cargo test --workspace`、`cargo fmt --check`、clippy。

28. **第 5 段：驗收**（2026-11-08，使用者確認）。
   1. `progress.md` 的「驗收」補齊第 2 到 4 段：每一列是驗證什麼、由哪個測試驗，整理既有的測試。
   2. 長時間的 cargo-fuzz（wasmtime 的 GC fuzz target 用 elise）這次不跑（丙），之後或交給 CI；固定種子的差分 fuzz（`gc_ops` 對 copying）照舊在 fork 的測試裡。不選每個 target 跑 1 小時（甲）、10 分鐘（乙）。
   3. 補共用的 heap 在 concurrent marking 中的寫入的測試：fork 的 `shared.rs` 用一步一步走的 platform，DOM 的 heap 的 marking 停在中間時 wasm 用 `struct.set` 和 global 寫邊，收尾之後物件都在；跑 mutation test。
   4. 「和 DOM 之間經過 handle 的循環」驗收不涵蓋：guest 只拿得到 WIT 的整數 handle，放進 wasm 的 struct 只是數字、不是邊，要等 `externref` 指向 DOM 節點（「待決定」）才有循環。

## 待決定

- **header 合成一個**：第 2 段起兩個 header 都留（已決定的事項 13 第 3 點：每個 wasm 物件多 8 byte，兩個欄位的 struct 從 16 byte 的格子變成 32）；合成一個是第一個 word 是 wasmtime 的 kind（高 6 bit）加上 elise 的 flags（放在 wasmtime 留給 collector 的 26 bit，elise 要約 18 bit），第二個 word 是型別編號（elise 的編號 = Engine 的基底 + `VMSharedTypeIndex`，wasm 碼讀型別時減掉基底；要確認 wasmtime 產生的碼怎麼拿到預期的型別），但兩邊對 header 的位置不同（elise 在資料前面，wasmtime 在 ref 的位置），要改 elise 的執行時物件的邊或 fork 產生的碼。有真的程式可以量時（最晚第 5 段的驗收）再定。
- **RFC 做完之後 repo 之間的依賴**：第 2、3 段 fork 用 path 依賴 elise、kun 不碰 fork（已決定的事項 13 第 6、7 點），第 4 段 kun 開始依賴時把 fork 和 elise 一起加進 `third_party.toml`；RFC 做完之後再看 kun、fork、elise 之間改成什麼穩定的方式（path、git 的 rev、發版本）。
- **同步的節奏**：2026-11-01 的看法：只在 kun 升級 wasmtime 時 rebase，不追每個月的版本；安全公告的修正照第 4 條的第 1 點，不等節奏。
- **heap 的上限**：第 4 段不做（已決定的事項 27），cage 滿了 elise panic，和 JS 文件的 DOM 一樣；renderer 拆成 process（RFC 0004）時和 kun 整體的做法一起定。
- **`externref` 指向 DOM 節點**：像 wasmtime 那樣包一個 GC 物件，還是直接指向節點（RFC 0001 的 Q14 留下的）。

## 之後的可能

- DOM 參照變成 GC 參照（見「動機」）。
- 配置在 JIT 裡直接 bump nursery，不經過 libcall。
- renderer 拆成獨立的 process（RFC 0004）之後，OS 的 sandbox 補回 GC heap 外面的一層。
