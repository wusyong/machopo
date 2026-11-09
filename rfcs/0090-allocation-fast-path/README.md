# RFC 0090：配置的快路

- 開始日期：2026-11-07
- 狀態：✅ 已完成：2026-11-08（第 1 到 3 段；2026-11-08 從「GC 不再搬動」改成這個範圍，決定 3；驗收和做過的檢查見 `progress.md`）
- 細節：無（第 1 段的量測在 `progress.md`）

## 摘要

renderer 的 DOM heap（V8 驅動的那個）從 RFC 0085 起不用 nursery，但每次配置還是走分代的整條路：先試 nursery、`Site` 的統計、逐物件的計數，`publish` 還要找頁；取樣裡 GC 那一類 kun 37.3 ms、Chrome 17.8 ms，差的幾乎都是配置。這份照 cppgc 的 `MakeGarbageCollected` 重做配置的快路：size class 的 LAB 只 bump、寫 header，計數和 `Site` 的工作移到換 LAB 的慢路，沒有 nursery 的 heap 不做分代的判斷，`publish` 不借 `space`。GC 照舊分代、會搬動：RFC 0005 的 wasm GC 的物件要它（決定 3）。結果（2026-11-08）：取樣的 GC 那一類 37.3 → 28.0 ms，`calls` 的 `createElement` 2237 → 2103 條指令，blink-perf 的 `gc-forest` −8.9%、`gc-mini-tree` −10.4%、`create-element` +7.1%（`progress.md` 的「驗收」）。

## 動機

**現況**（2026-11-07，Linux、AMD Ryzen AI 9 HX 370；RFC 0085 的收尾和這份的第 1 段）：

- 取樣的 GC 那一類（Speedometer 3.1 的 20 個 suite 合計，每個 iteration）kun 37.3 ms、Chrome 17.8 ms。kun 多的幾乎都是配置：`Inner::allocate_in` 9.5、`publish` 5.8、`Site::index` 1.2、`alloc_at` 的 init 約 1。Chrome 的 cppgc 配置 inline 在 Blink 裡，算在 DOM。
- `bench.py calls`：`createElement` 2237 條指令（456 cycle），RFC 0085 的 4d 量過其中配置 DOM 物件約 390 條（那時之後做了 size class 的編譯時計算）。
- renderer 的 heap 每配置一個物件：`alloc_at` 查型別、決定 `old`；`allocate_in` 對有 site 的配置（DOM 幾乎都有）`borrow_mut` 一次 `Site` 的表，查 tenure、記一筆 `entered_old`，再看能不能進 nursery；`allocate_old` 更新 `allocated`、`used`，marking 期間加 `black_bytes`；`publish` 寫 header、查 `is_young` 和 `marking_active`，有 finalizer 的物件借 `space`、找頁、設 finalizable 的 bit。
- Chrome（cppgc 的 `ObjectAllocator::AllocateObject`）：LAB 夠就 bump、在原地建 `HeapObjectHeader`（GCInfo 的編號和大小），不夠才走 `OutOfLineAllocate`；配置的量在換 LAB 時才告訴 `StatsCollector`；finalizer 由 sweep 照死物件的 GCInfo 找，沒有逐物件的 bit。

**目標**：

- 沒有 nursery 的 heap 的配置快路照 cppgc：bump、寫 header、少數的條件，其餘在慢路。
- 有 nursery 的 heap（測試、`gc_bench`，之後的 wasm 文件和 RFC 0005 的 wasm GC 的物件）不變慢：分代的判斷照舊，只是不放在沒有 nursery 的 heap 的路上。
- GC 那一類往 Chrome 靠（差 19.5 ms）；`calls` 的 `createElement` 的指令數減少。

**要解決的 `docs/limitations.md` 的限制**：「GC 和 `dom`」的「renderer 的 DOM heap 沒有 nursery」那一條的後半（分代的成本還在配置的路上）。

**不在範圍內**：

- 拿掉分代、GC 不再搬動（決定 3：RFC 0005 要它）。
- JIT 裡直接 bump（RFC 0005 的「之後的可能」）：這份的快路的形狀要讓它之後用得上，但不做。
- write barrier：沒有 nursery 時 young 的檢查只讀一次表，第 1 段沒看到它的成本。
- compaction、碎片（第 1 段量了，沒看到時間上的損失）。

## 設計概要

對 `gc` 的使用者沒有變化：`alloc`、`alloc_in`、`alloc_beside` 照舊。`Heap::used_bytes` 仍然是精確的（扣掉 cursor 還沒用的尾巴，要走過每一級的 cursor）；新的 `Heap::counted_bytes` 不扣，一次讀完，給每次操作之後看 heap 長了多少的人（`dom_js` 報給 V8 之前，`Dom::counted_bytes`）；`HeapStats::slow_allocations` 數走慢路的配置。DOM 的程式不用改。

## 詳細設計

（決定 4）

- **快路**：`Mutation::alloc_at` 在沒有 nursery 的 heap（或一定配 old、沒有 site 要數的物件）上，型別有 size class、那一級的 cursor 有空間時，`Inner::bump` 之後 `publish_bumped`：寫 header；有 drop glue 的物件經過 `MarkTable::publish` 用頁號拿那一頁的 bitmap 設 bit（沿用 `MarkTable::get` 的論證，不在 cursor 裡放指標），頁上的計數（`finalizable_objects`、`main_objects`）記在 cursor 上。`Inner::allocate`（`HeapVector`、`MemberArray`）也先 bump。其餘（cursor 用完、large object、有 nursery 的 heap 的 young 配置、要數的 site、`pretenured`）走 cold 的 `allocate_slowly`。
- **cursor 上的計數**：sweep 照計數交出頁之前、cursor 換頁之前（`refill`）和 verifier 數之前加回頁上（`flush_cursor_counts`）。`main_objects` 少算時，有 `LocalExternal` 的頁會交給 sweep 的 worker（`crates/gc/soundness.md` 的 I6）。
- **配置的量**：小物件的在 cursor 拿到一段時照整段算（`count_allocated`，cppgc 的 `StatsCollector` 照 LAB 算），large object 照舊逐個。要精確的地方扣掉 cursor 的尾巴：`used_now`（`used_bytes`、`HeapStats`、verifier）；GC 清掉 cursor 時從 `used` 扣掉尾巴（engine 驅動的 marking 收尾的 `used` 含它們）；marking 開始時 `black_bytes` 從現在的尾巴算起（它們標黑了）。GC 的門檻（`allocated_past`）先看整段算的值，到了才扣尾巴確認：門檻小的 heap 一次換段就越過整個門檻，不確認的話直接做阻塞的 full GC、到不了 concurrent marking 的一半。
- **`Site`**：沒有 nursery 的 heap 不看 `Site`（`alloc_in`、`alloc_beside` 直接用 0，快路不借表、不記 `entered_old`）；有 nursery 的 heap 的 tenure 判斷和計數照舊。
- **回報給 V8**：`dom_js` 每次操作之後用 `counted_bytes` 判斷長了多少（`REPORT_BYTES`），要報時才算 `used_bytes`：每次都算時，每個會回報的成員多約 510 條指令。

### 測試

- 計數守住快路：換 LAB 的次數、走慢路的次數（debug build 的計數，像 RFC 0085 的 `*_for_testing`）；`HeapStats` 的 `used_bytes`、`allocated` 在換 LAB、GC、退回時對得上（heap verifier 的帳）。
- 有 nursery 的 heap 的行為不變：`gc` 的 `young`、`sites` 的測試照舊。
- mutation test 照 `docs/testing.md`；量測：`calls`、取樣的 GC 那一類（使用者要求時）、blink-perf 的 `create-element`、`gc-*`。

### 分階段

| 段 | 內容 |
| --- | --- |
| 1 | 量測（原本為「GC 不再搬動」做的，結果仍然用得上：JS 文件的 heap 不需要 nursery，決定 3） |
| 2 | 配置的快路：計數、`Site`、`publish`、快路和慢路的切法（決定 4） |
| 3 | 量測、文件 |

## 缺點

- 配置的量照整段算：`used_bytes` 要走過每一級的 cursor 才精確（51 級），所以有 `counted_bytes` 給熱路；報給 V8 的變化晚到一段之內才看到（一段最多一頁 256 KiB），門檻的判斷到了整段算的值才多算尾巴。
- 有 nursery 和沒有 nursery 的 heap 走不同的配置路，兩條都要測。
- 和 RFC 0005 的 JIT 配置（之後的可能）要對齊快路的形狀。

## 理由和其他做法

- **為什麼不再是「GC 不再搬動」**（決定 3）：第 1 段只證明了 DOM 物件不需要分代（有 wrapper 的不能搬、死得慢）。RFC 0005 要把 wasm GC 的物件（Kotlin、Dart、Java、OCaml 這類語言的 `struct`、`array`）配在 DOM 的 heap 上：它們和 JS 物件一樣配置量大、多半死得極快，參照由 wasmtime 的 stack map 精確地交出、搬得動，正是複製式分代划算的情形（V8 把 wasm GC 的物件放在它的 young generation，wasmtime 預設的 collector 也是複製式的）。所以 GC 照舊分代、會搬動，這份只讓沒有 nursery 的 heap 不付分代在配置上的成本。
- **為什麼不拿掉 finalizable 的 bitmap、照 cppgc 讓 sweep 讀 header 找 finalizer**：RFC 0085 的決定 6 評估過，比 cursor 記住那一頁的 bitmap 多省的只有一次寫入，要改 sweep 和照頁數 finalizer 的地方（第 2 段再確認）。
- **不搬動了也不保守掃描 stack**（原本的方向時寫的，仍然成立）：障礙是 Rust 管不了值放在哪裡（`Vec`、closure、`Box`、future 都在 malloc 的 heap 上，掃 stack 看不到），不是搬動；`docs/architecture.md` 的「和 Chromium 刻意不同的地方」照舊。

## 先例

- **cppgc**：`MakeGarbageCollected` → `ObjectAllocator::AllocateObject`：LAB 的 bump 和 header inline，`OutOfLineAllocate` 換 LAB；`StatsCollector` 在換 LAB 時記配置的量；finalizer 在 GCInfo 裡，sweep 照死物件的 header 找。
- **V8 的 JS heap**：`LinearAllocationArea` 的 bump 由產生的碼 inline，`AllocationObserver` 照步長、不逐物件；`AllocationSite` 的 pretenuring 只在新生代的配置記 memento。

## 已決定的事項

1. 2026-11-07，方向（使用者選甲）：只有 old（Oilpan 的做法）。第 1 段的數字改變它的條件：JS 文件裡有 wrapper 的 DOM 物件在 V8 的 scavenge 丟掉 wrapper 之後很快沒人參照的多（量法的第 1 項），改成乙；wasm 的 guest 沒有分代時 GC 明顯變慢（第 5 項），照樣先做甲、乙接著做；碎片在 Speedometer 上就明顯（第 3 項），compaction 排進來。不選：乙一開始就做（為還沒接上的 wasm 先寫新的 minor GC）、丙（兩種模式並存）、丁（只做配置的快路）。不依賴不搬動（「不依賴不搬動」），不保守掃描 stack。
2. 2026-11-08，第 1 段量完（使用者同意）：照決定 1 維持甲，GC 只有 old。乙的條件不成立：不搬動的 minor GC 在 1 到 4 MB 時只收得掉新物件的 16% 到 25%（DOM 物件到 full GC 時大多死了，但要等 V8 丟掉 wrapper、頁面放掉它，死得慢）；沒有 engine 的 heap（wasm 文件的情形）沒有分代時，短命的工作慢 5% 到 16%、整棵樹活著的快 4% 到 38%，不算明顯，wasm 接上 renderer 之後用真的頁面再看。整棵樹活著的頁沒有 nursery 時慢約 3%，查過：配置、兩邊的 GC、指令數、page fault、L2 和 dTLB 的 miss 都一樣，一部分是資料的位址排列，不是配置的位置策略，第 2 段不為它設計配置的位置。碎片（marking 開始時頁上空 34% 到 62%）沒看到時間上的損失，compaction 不排進來（「之後的可能」）。數字在 `progress.md` 的第 1 段。
3. 2026-11-08，範圍改成配置的快路（使用者決定）：保留分代和搬動，決定 1、2 的方向不做。原因：RFC 0005 的 wasm GC 的物件要配在 DOM 的 heap 上，它們需要複製式的分代（「理由和其他做法」）；寫這份的草稿時漏看了 RFC 0005。第 1 段的量測仍然成立、仍然用得上：JS 文件的 heap（renderer 的）不需要 nursery，DOM 物件放 old。使用者另外說明：kun 的 `crates/gc` 改了很多，之後可能直接把它搬過去取代 elise 那邊的程式再升級，所以 GC 的改動照樣做在 kun。
4. 2026-11-08，第 2 段的做法（使用者照建議確認）：
   1. 配置的量（`allocated`、`used`、marking 期間的 `black_bytes`）在 cursor 拿到新的一段時照整段算，用不完的段退回時扣掉（cppgc 的 `StatsCollector`）；GC 的門檻和報給 V8 的大小準到一個 LAB（最多一頁 256 KiB）。不選每個物件算（每次配置多三個計數的讀寫）。
   2. 沒有 nursery 的 heap 不碰 `Site`：不借統計的表、不查 tenure、不記 `entered_old`、不呼叫 `Site::index`；有 nursery 的 heap 照舊。不選留著統計（沒有人用，取樣裡約 1.2 ms）。
   3. `publish` 分成 old 和 young 兩條，old 的不查 `is_young`；有 finalizer 的物件寫進 cursor 記著的那一頁的 bitmap，不借 `space`、不找頁。不選照 cppgc 拿掉 bitmap、sweep 讀 header（多省的只有一次寫入，要改 sweep）。
   4. 一條 inline 的快路：heap 沒有 nursery 或型別一定配 old、那一級的 cursor 有空間，就 bump、寫 header；換 LAB、large object、young 的配置、`pretenured` 走一個 cold、不 inline 的慢路。有 nursery 的 heap 的 young 配置照舊，等 RFC 0005 的 JIT 配置再看；不選現在也給 young 一條 inline 的快路（現在只有測試和 `gc_bench` 用）。

## 待決定

無。沒有 driven 的 heap 的 full GC 門檻照舊（`docs/limitations.md` 的「full GC 的門檻固定」），wasm 接上 renderer 之後用真的頁面再看。

## 之後的可能

- old 的 compaction（`docs/limitations.md` 的「沒有 compaction」）：第 1 段量到 marking 開始時頁上空 34% 到 62%，但沒看到時間上的損失；長時間反覆增刪的單頁應用量到問題時，從只被 heap 的邊指著的物件（backing store）開始。
- 不搬動的分代、只有 old：第 1 段為它們量的數字在 `progress.md`；wasm GC 的物件進來之後，如果 DOM 和 wasm 的物件的需求差很多，再看要不要讓 heap 照物件的種類分（例如 DOM 物件一律 old、wasm 物件走 nursery，現在 engine 驅動的 heap 對有 JS 邊的型別已經這樣做）。
