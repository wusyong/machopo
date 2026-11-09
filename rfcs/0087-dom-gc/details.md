# RFC 0087 的細節

`README.md` 的補充：動機的量測、DOM 的工作有哪些特性、每個決定的完整比較、concurrent 的記憶體模型論證、接 V8 和 wasmtime 的做法、驗證的工具、量測的計畫、和其他 GC 的對照。除了「已決定的事項」列的，這裡寫的都是 2026-10-24 的看法。

## 動機的量測

2026-10-24 從 `target/bench/speedometer/sample/` 的取樣（kun ce849649，`perf` profile）算的，方法是：一個 sample 的 stack 上只要有 root 的機制（`dom::handle::Root::new` / `try_new`、`Roots`、`Handle` 和 `Root` 的 drop、cppgc 的 strong `Persistent` 和 `PersistentRegion`），就算進 root，歸給 stack 上最外層的那個 frame（包含它呼叫的東西）。

- **strong root 約佔 renderer main thread 的 2.2%**。照呼叫的地方分：大部分是建節點、操作結束時留下的 handle、event 的派送，這些換一種 root 的表示（root 表的一格、parser 建的節點不逐個 root）就能省；只有保守掃描 stack 才省得掉的最多約 0.95%。
- **RFC 0085 的來源 1 約每個 iteration 75 ms**（和 Chrome 的差距約 510 ms）：FFI 和 V8 API 那一類約 54 ms，GC 的機制（root、`Persistent`、barrier、配置經過 FFI）約 21 ms。
- **wasm 每次 host 呼叫的固定成本是 wasmtime 的**：RFC 0067 量的 `nodeType` 一次約 2184 個指令（JS 文件約 209 個），大部分在 wasmtime 的 host task 和 resource table，GC 換掉也不會變。

結論：換 GC 不是為了 JS 文件的速度（root 能省的 JS 那一側用現有的 cppgc 也做得到），而是為了 GC 的規則本身：能搬動、能分代、沒有 finalizer、配置和 barrier 能 inline、wasm 的路徑不再為了 cppgc 連結 V8，之後的優化不再被 FFI 和 cppgc 的規則擋住。

## DOM 的工作有哪些特性

GC 照這些特性設計，第 0 和第 1 段要量的也是這些：

1. **一個 heap 只有一個 mutator**：renderer 的 main thread。worker 有自己的 heap，DOM 不跨 thread 共用。所以配置、barrier 不需要同步，只有 GC 自己的 thread 和 mutator 之間要。
2. **操作短而且有邊界**：一次 DOM API 呼叫、一次 parse 的一段、一次 style 或 layout 都是一個操作，操作之間有很多自然的安全點。長的操作只有少數（parse 很大的文件、很大的 `innerHTML`）。
3. **物件的壽命分兩群**：parser 建的初始文件大多活很久；框架（React、Vue 的 reconcile）、event、`DOMRect`、`NodeList`、暫時的字串和陣列大多很短。實際的存活率不知道，第 1 段量。
4. **樹的寫入多、但 old 指向 young 的邊少**：建一棵新的子樹時，子樹內部的邊都在 young 裡；接到舊的樹上時，新產生的 old 指向 young 的邊只有父節點的 `last_child`（或 `first_child`）和前一個兄弟的 `next_sibling` 這幾條，新節點指回 parent 的是 young 指向 old，不用記。拿掉子樹時改的是 old 的欄位，marking 期間 SATB 要記被覆寫的舊值。
5. **很多參照是 weak 的**：id 表（style、layout、paint 用 id 指節點）、`MutationObserver` 的登記、live 的 `HTMLCollection` 和 `NodeList` 的快取、事件 listener 的 wrapper。
6. **大部分物件不需要 finalizer**：只要字串和陣列也放在 GC heap 上，節點死掉時不用做任何事；現在每個 Rust 物件都有 finalizer、一律回到 main thread，是 cppgc 和 Rust 的 `Drop` 對接的結果，不是 DOM 的需要。
7. **JS 文件裡每個碰過 JS 的節點有一個 wrapper**，DOM 和 JS 之間有循環（節點、listener 的 closure、wrapper、節點）。

## 每個決定的完整比較

### 搬不搬動

| | 不搬動（Oilpan、Go、Riptide） | young 搬動（V8、SpiderMonkey、G1） |
| --- | --- | --- |
| 配置 | free list 或 sweep 出的區段 | nursery 的 bump，最快 |
| 短命物件的成本 | 每個死掉的物件都要 sweep | 死掉的完全不碰 |
| 前提 | 可以保守掃描 | 每個參照都要找得到並更新 |
| 碎片 | 長期會有 | young 沒有，old 靠 compaction |

選 young 搬動：前提「每個參照都找得到」在安全點成立（操作裡的 `Gc<'gc, T>` 活不出操作），代價只有 root 表和 id 表要存索引、搬動時更新。不搬動的唯一理由是保守掃描，而保守掃描在 Rust 裡本來就做不到（`Vec`、closure、被 box 的 future 裡的值在 heap 上）。

### Barrier

marking 期間 mutator 改了圖，barrier 讓 marker 不漏標。三種：

- **Dijkstra（insertion）**：寫入的新值變灰。V8 和 cppgc 都是這種。stack 和 root 要在收尾時重掃（root 一開始被當成灰的，之後的改動沒有 barrier）；收尾的 pause 和 root 的數量成正比。
- **Yuasa（deletion，SATB）**：被覆寫的舊值變灰。開始時的快照裡到得了的都會標到；root 只在開始時掃一次，root 表的刪除也經過 barrier。收尾不重掃 root。G1、SpiderMonkey 的 incremental marking 是這種。
- **Hybrid（Go）**：兩種都做，讓 stack 不用重掃；要兩種的成本。

選 **SATB 加上 black allocation**：

- 收尾的 pause 不重掃 root 表，和 `Persistent` 的數量無關（JS 文件裡每個 wrapper 背後都有一格）。
- 配合「marking 開始前先 scavenge」，old 的 marking 不必走 young（下一節），young 裡的寫入完全不用 marking barrier。
- 代價：floating garbage 比 Dijkstra 多（一輪 marking 裡被拿掉的子樹多活一輪）；barrier 要先讀舊值（DOM 的寫入大多是覆寫，舊值本來就在 cache 裡）。

marking 沒在進行時，兩種 barrier 都只是一個 flag 的檢查；generational barrier 另外算。第 1 段在 0g 的錄製上模擬一輪 marking，比兩種的 barrier 次數和收尾要重掃的 root（`README.md` 的已決定的事項 17），第 4 段開始前定（`README.md` 的待決定 1）。

第 1 段的結果（數字在 `progress.md` 的 1a、1b）：

- **協定**（TLA+）：兩種都不漏標。SATB 除了寫入，刪 root 和 weak 讀取也要 barrier，black allocation 不能少（新物件寫進掃過的物件時，覆寫的舊值是空的）；拿掉任何一個 TLC 都找到反例。Dijkstra 少了收尾的 root 重掃就漏標，但安全性不靠 black allocation（新物件寫進欄位時 barrier 會標它，放進 root 表的由重掃標），black allocation 對它只省 marker 的工作。
- **成本**（錄製上的模擬）：欄位寫入的 slow path 兩邊都幾乎是 0（每輪幾十萬到幾百萬次寫入裡 1 到 6 次），成本在 fast path 的檢查，兩邊一樣。差別在收尾：Dijkstra 的重掃和 root 表一樣大，JS 文件每個 wrapper 一格，每次幾十萬到上百萬格；SATB 不重掃，代價是 marker 落後時的 floating garbage。wasm 的錄製（gc-tree 以外）存活率是 0，分代自己收完，模擬裡沒有一輪 old 的 marking。

**weak 的讀取也要 barrier**：marking 期間從 id 表、`WeakMember`、`WeakPersistent` 讀出一個還沒標過的物件，等於讓快照裡已經到不了的物件復活，SATB 保證不到它。讀取時把目標標上（G1 的 `Reference.get` 的 keep-alive barrier、V8 的 weak 處理也是同樣的道理）。

### 分代

- **要**：DOM 的短命物件（特性 3）在 nursery 裡死掉不用任何成本；scavenge 的 pause 只和存活的物件成正比。
- **風險**：存活率高（大部分新節點都接到樹上、活很久）時，scavenge 白做。對策：照配置的地方 pretenure（parser 建的初始文件直接放 old）、nursery 的大小照存活率調、量到某類配置大多活下來就直接配置在 old（V8 的 allocation site pretenuring）。
- **remembered set 的大小**：特性 4 說插入子樹只產生少數 old 指向 young 的邊；0g 在錄製上量到每次 minor GC 最多 1 到 4 條（`README.md` 的待決定 2），所以記成精確的 slot set（每格一個 bit）。
- **另一種分代**：Riptide 和 cppgc 的 young generation 用 sticky mark bit、不搬動。省掉複製，但配置和回收照舊是 sweep 的成本。

### Finalizer

| | 現在（cppgc） | 這份 |
| --- | --- | --- |
| 哪些物件有 | 每個 Rust 物件 | 只有欄位裡有 `External<T>`、而且 `needs_drop::<T>()` 的 |
| 在哪裡跑 | main thread | `Send` 的在 sweeper，標出來的在 main thread |
| 能做什麼 | 由 `soundness.md` 的規則限制 | 只有 `External` 裡的 `T` 的 `Drop`，碰不到 GC 物件 |

- **finalizer 只來自 `External<T>`**：欄位的型別只有 `Member`、`HeapVector`、純資料和 `External<T>`（README 的「物件模型」），前三種都沒有 `Drop`；物件型別自己實作 `Drop` 由 derive 擋掉（照 pin-project 的做法：derive 產生一個和 `impl<T: Drop>` 衝突的 impl）。所以 sweep 時要跑的只有 `External` 裡的 `T` 的 `Drop`；換成 side table（乙）之後，同一個 `Drop` 改在回收那一格時跑，分類不變。
- **`T` 碰不到 GC 物件**：`Member` 沒有能拿出一個 heap 外的值的 API（沒有公開的建構子、不是 `Clone`、沒有 `Default`；寫入是 `set(cx, value)`），所以 `External<T>` 的 `T` 裡不可能有 `Member`（marker 看不到 `External` 的內容，這條也保證它不會漏標）；`Persistent` 可以放進 `T`（它是 root，不是 heap 的邊），但讀它要 `Mutation`，`Drop` 拿不到。
- **在 sweeper 上跑的條件**是 `T: Send`，在 `External<T>` 建立時由型別決定；不是 `Send` 的要明確標出、交回 main thread。

### 壓縮指標

照 V8：一個 heap 一個 cage，`Member` 存相對於 cage 基底的 32 位元 offset。

- **好處**：`Member` 和 `HeapVector` 的元素小一半，cache 裡放得下的節點多；V8 量到 heap 小了約四成（V8 的部落格 "Pointer compression in V8"，2020）。
- **代價**：讀出來時加上基底（一個加法，基底在暫存器或 thread-local）；cage 有上限。
- **上限**：shift 0 是 4 GiB；物件 8 byte 對齊時 shift 3 可以到 32 GiB，但每次讀多一個 shift。一個文件的 DOM 超過 4 GiB 的情況很少，`README.md` 的待決定 3 在第 2 段定。

### 怎麼走過物件

- **layout 描述**（選）：型別表記著每個 `Member` 的 offset 和 `HeapVector` 怎麼走，marker、scavenger、compaction、verifier 共用一份資料、不呼叫型別的程式碼。marker 讀的只有那些 offset，其他欄位（純資料、`External<T>`）它根本不讀，所以型別不需要是 `Sync`，也沒有 cppgc 那樣「`Trace` 和 mutator 同時讀一個欄位」的問題（`crates/dom/soundness.md` 的 C1）。
- **每個型別的 `trace` 函式**（cppgc、gc-arena）：彈性大（可以照狀態決定走哪些欄位），但 marker 在別的 thread 上跑使用者的程式碼，soundness 要靠每個 `trace` 的寫法。
- 要有條件地走的情況（例如 enum 的 variant 裡才有 `Member`）用 `HeapVector` 或 `Option<Member>` 表達，不讓 layout 依賴 tag。

### API 的模型

- **gc-arena 式的 brand 加上「只在操作之間 GC」**（選）：`Gc<'gc, T>` 是裸指標，編譯器保證它活不出操作；不掃 stack、不需要 no-GC scope，也不需要靜態的 hazard analysis。
- **SpiderMonkey 的 `Rooted<T>` 加 hazard analysis**：操作中途也能 GC，但每個區域變數要 root、分析是額外的工具。
- **RFC 0060 的 `NodePtr` 加 no-GC scope**：在 cppgc 上的折衷；新的 GC 本來就只在安全點收，不需要它。
- 要在操作中途回收記憶體的情況（一次 parse 很大的文件）：parser 把工作切成幾個操作（每個 chunk 一個），中間是安全點。

### Root 的表示

- `Persistent<T>`：root 表的一格，存索引；表是一個陣列加上 free list，建和刪都是 O(1)、不經過 FFI。scavenge 和 compaction 搬動物件時直接改表。
- SATB：刪掉一格（`Persistent` 的 drop）在 marking 期間把舊值放進 buffer；新的一格的值一定是快照裡的、新配置的、或經過 weak barrier 的物件（上一節），不用 barrier。
- id 表同理：以 id 為索引的位址陣列，是 GC 的一部分（weak），scavenge、compaction 改它，收尾和 scavenge 清掉死的。

## 記憶體模型

### 為什麼 old 的 marking 不必走 young

1. marking 開始時緊接著一次 scavenge，這時 nursery 是空的，所有活的物件都在 old；快照 S 是這時 root 表和 old 的圖到得了的物件。
2. SATB 的不變式：S 裡的每個物件，到收尾時都已經標過。理由是標準的：一個 S 的物件沒被標到，表示它在快照時的每一條路徑都在 marker 走到之前被切斷了；切斷是一次覆寫（或 root 的刪除），barrier 把被覆寫的舊值記下來，那條路徑上第一個被切的物件會被標，從它往下的路徑還在，所以矛盾。
3. 開始之後才出現在 old 的物件：直接配置在 old 的、scavenge 升上來的，一律算標過（black allocation）。
4. 收尾時 old 裡到得了的物件只有三種來源：S（2 保證）、開始後出現的（3 保證）、經過 young 的物件指到的。最後一種指到的 old 物件本身不是 S 的就是開始後出現的（mutator 拿得到的指標只來自到得了的圖、新配置的、或 weak 的讀取，而 weak 的讀取有 barrier），所以也標過。
5. 所以 old 的 marker 遇到指向 young 的欄位直接跳過（young 是 cage 裡已知的一段位址，一次比較），不必走 young；young 裡的寫入不需要 marking barrier。

### marking 期間的 scavenge

marking 可以跨過好幾次 scavenge。scavenge 在安全點做：

- marker 的 thread 先在一個物件的邊界停下（每走完一個物件檢查一個 flag），scavenge 做完再繼續；停下和繼續經過 mutex，之後 marker 看得到 scavenge 寫的所有東西。
- marker 的 worklist 裡只有 old 的物件，scavenge 不搬動 old，worklist 不用更新；升上來的物件已經標過，不放進 worklist。
- SATB 的 buffer 裡只有 old 的舊值（young 的舊值不記，見上一節），也不受 scavenge 影響。

### marker 和 mutator 同時讀寫

- marker 只讀 layout 描述列出的 `Member` 欄位，用 atomic 讀；mutator 寫 `Member` 也一律 atomic（marking 沒在進行時是 relaxed；進行中是 release）。所以兩邊同時碰同一個欄位不是 data race。
- marker 讀到的值可能是覆寫前或覆寫後的，兩種都對：覆寫前的值本來就在快照裡；覆寫後的值是 young（跳過）、開始後出現的（已經標過）、或 S 的（2 保證另一條路會被記下）。
- **發布**：marker 讀到一個開始後才出現的 old 物件時，必須看得到它的 mark bit 已經設好，否則會去讀一個 mutator 剛初始化、還沒同步的物件。做法：black allocation 在物件可以被別人指到之前設好 mark bit；marking 期間寫 `Member` 用 release，marker 用 acquire 讀，所以 marker 看到指標時一定看到 mark bit，直接跳過、不讀它的欄位。scavenge 升上來的物件經過上面的 mutex 發布。
- marking 期間的 release 寫入在 ARM 上是 `stlr`，比一般的寫入貴一些；只在 marking 進行中，第 4 段量。
- 這些協定由 loom 窮舉（下面「驗證的工具」），Miri 跑整個 GC 時也會檢查 data race。

### 收尾

收尾的 pause 在安全點：mutator 交出 SATB 的 buffer；marker 把 buffer 和 worklist 做完（mutator 不再改圖，所以會結束）；處理 ephemeron（重複到沒有新的標記為止）；清 `WeakMember`、weak 的 root、id 表裡死的；決定 compaction 的 page；開始 concurrent sweeping。收尾的時間主要是 buffer 裡剩下的量，排程讓 marker 在 buffer 小的時候才收尾。

### Sweeping 和 remembered set

- sweeper 在背景照 mark bitmap 找出空的區段、跑 `Send` 的 `Drop`、把要在 main thread 的放進佇列。
- **死掉的 old 物件的 slot**：scavenge 照 slot set 讀 old 的格子時，還沒 sweep 的 page 上可能有死掉的物件，它的 `Member` 指向的 young 物件可能早就不在了。所以 scavenge 只讀 mark bit 是活的物件的格子（sweep 完的 page 上，死掉的區段的 bit 在 sweep 時清掉）。V8 在 marking 之後過濾 remembered set 也是這個原因。
- 下一輪 marking 開始前 sweep 要做完（還沒做完的在那次 pause 裡做完）。

### Compaction

- marking 開始時選好 evacuation 的 page；marker 走到指向這些 page 的欄位時記下來（每個 page 一份 slot set），marking 期間 mutator 新寫的、指向這些 page 的欄位也由 barrier 記下（V8 的 marking barrier 也做這件事）。
- 收尾的 pause 裡平行搬空這些 page，照 slot set、root 表、id 表更新指標。
- 代價：barrier 在 marking 期間多一個檢查、slot set 的記憶體；pause 變長。所以第 6 段先量碎片，不夠多就不做。

## 接 V8

只在 JS 文件。V8 的 `EmbedderHeapTracer` 已經拿掉，embedder 能參與 V8 major GC 的方式只剩 `CppHeap`（和只處理 root 的 `EmbedderRootsHandler`），所以橋接一定經過 cppgc。

### Stub

每個 wrapper 的 internal field 指向一個很小的 cppgc 物件（stub），stub 存著 root 表的索引（和快取的位址，給 binding 快速取到物件）。

- **JS 指向 DOM**：V8 的 major GC 標到 wrapper 時標到 stub；stub 的 `Trace` 讓我們的物件被標到。
- **DOM 指向 JS**（listener、`dataset` 這類存著 JS 值的欄位）：存在我們 heap 上的一個特殊欄位（相當於 `TracedReference`），我們標到含有它的物件時，要把它交給 V8 的 marking。
- **循環**：節點、listener 的 closure、wrapper、stub、節點，要兩邊在同一輪裡一起判斷才收得掉。所以 JS 文件的 old marking 不是我們自己決定開始的，是 V8 的 major GC 的一部分（我們的 heap 長大時請 V8 做一次 major GC）；stub 在我們的 old marking 裡不是 strong root。
- **scavenge**：我們的 scavenge 把活著的 stub 指到的物件當成 strong root（V8 的 scavenge 也把 embedder 的參照當成 root），搬動之後更新 stub 快取的位址。stub 死掉時（cppgc 的 pre-finalizer，在 main thread）釋放那一格。
- **規則**：DOM 的操作裡不呼叫 V8，V8 的 GC（和 V8 的 safepoint）只發生在我們的安全點；所以 V8 標 stub 的那一刻，我們的 heap 沒有沒 root 的指標。

### 兩邊的 marking 怎麼對齊

`README.md` 的待決定 6，第 1 段的原型要回答：

- **甲、同步**：stub 的 `Trace`（在 V8 的 marking thread 上）直接走我們的 heap，碰到 DOM 指向 JS 的欄位就交給 V8 的 visitor。簡單、循環自然收得掉；但我們的 marker 變成 V8 的 marker 的一部分，排程、平行度、barrier 都要配合 cppgc（marking 期間 mutator 寫我們的 heap 也要讓 V8 知道），JS 文件用不上自己的 concurrent marker 的設計。
- **乙、各自 marking、在安全點交換**：V8 開始 major GC 時我們也開始 marking；V8 標到的 stub 由 main thread 交給我們（stub 的 `Trace` 只記下索引），我們標到的 DOM 指向 JS 的欄位，在 main thread 上補給 V8（例如寫進 cppgc 這一側的一個 `TracedReference` 清單）；重複到兩邊都沒有新的為止，才讓 V8 收尾。我們的 marker 保持自己的設計，但要能延遲 V8 的收尾，需要 V8 的 `CppHeap` 在收尾前多問 embedder 一次（V8 的 API 有沒有這個掛點、要不要改 fork，原型要查）。
- **丙、wrapper 的 internal field 直接存我們的指標，不經過 cppgc**：V8 沒有掛點讓 embedder 參與 marking，循環收不掉。不可行，列出來是為了說明為什麼一定要 stub。

stub 的成本：每個 wrapper 多一次 cppgc 的配置（約 16 byte）和一次 FFI；binding 取 DOM 物件時讀 stub 快取的位址，多一次間接。

第 1 段的原型（1d，數字在 `progress.md`）甲、乙都做了：

- **兩種都對**：跨兩邊的循環在一次 V8 major GC 裡收掉，只被 JS 留著的、strong root 的、V8 marking 期間被搬動的節點的 listener 都留著。
- **甲**不用改 V8：stub 的 `Trace` 用 cppgc 的 `DeferTraceToMutatorThreadIfConcurrent` 延到 main thread（rusty_v8 的 glue 接出它，約 10 行），當場走我們的 heap。我們這一側用 Dijkstra（和 V8 一樣）；V8 沒有掛點，barrier 排進來的工作要經過 cppgc 自己的 write barrier 交回去（操作之後把一個新的 cppgc 物件寫進一個被留著的 slot，cppgc 追它時走完），strong root 由一個 cppgc root 的 `Trace` 走。代價是一個 stub 背後的子樹在一次 `Trace` 裡走完：一百萬個節點 4.5 ms。
- **乙**要改 V8 本身：`CppHeap` 加上 embedder 的 marking（`StartMarking`、`AdvanceMarking`、`FinishMarkingAndProcessWeakness` 各呼叫我們一次，兩邊都做完才算做完），約 90 行在 3 個檔案。我們這一側用 SATB 時，**經過 wrapper 那一格讀出的物件要標上**：V8 是 Dijkstra，JS 刪掉 wrapper 的最後一個參照不經過 barrier，而那個物件不在我們的快照裡（和 weak 讀取的 barrier 同一個道理）。預算夠時每步不超過預算（1 ms），收尾 0.09 ms；但 V8 照自己和 cppgc 的大小排程、照自己的工作決定收尾，看不到我們的 heap，照 V8 自己的排程時我們的工作和甲一樣都落在收尾。要真的切小停頓，掛點還要讓 V8 的排程知道我們剩多少（以前的 `EmbedderHeapTracer` 回報 embedder 的大小和速度），這部分還沒做。

**和 Chrome 的 unified heap 比**（2026-10-25 討論，照 V8 和 Blink 的設計，還沒有逐行對照原始碼）：Blink 的 DOM 本身是 cppgc 的物件，wrapper 的 internal field 直接指 `ScriptWrappable`，DOM 指向 JS 是 `TraceWrapperV8Reference`（包著 `TracedReference`）；V8 的 major GC 和 cppgc 用同一套 marking：worklist 互相丟工作、一起判斷結束、排程把 cppgc 的大小算進去、兩邊的 concurrent marker 一起跑，barrier 都是 Dijkstra 加 black allocation。演變是各自 GC 加 object grouping、`EmbedderHeapTracer` 的 wrapper tracing（兩個 marker 互相交換，和乙同一種介面）、再到 unified heap，之後 `EmbedderHeapTracer` 被拿掉。分三層看「合成一套」：

- **語意上同一輪**（環收得掉）：甲、乙都做到了。
- **規則上同一種 barrier**：不用改 V8，JS 文件那一側選 Dijkstra 就和 V8 一致，不需要經過 wrapper 讀取的 barrier；代價是收尾重掃整張 root 表（1b：JS 文件 25 萬到 120 萬格）。這是待決定 1，第 4 段開始前定。
- **執行上同一套**（停頓小的原因）：V8 要把我們的 heap 當成第二個 `CppHeap`：worklist 互通、一起判斷結束（乙的掛點是它最小的版本）、排程算進我們的大小（乙缺的部分，散在 V8 的 incremental marking 的步調和 heap 上限的計算裡，量比 90 行大不少，要原型才知道）；V8 的 concurrent thread 幫我們標可以不做（我們有自己的 worker）。完整的版本就是比較大的 V8 fork，每次升級 V8 都要 rebase。

**不 fork 的中間路線（甲 + 切段，未驗證，第 9 段先試）**：推廣甲裡交回 barrier 工作的技巧（把工作包成新的 cppgc 物件、寫進被留著的 slot，cppgc 追到時呼叫我們）。V8 的 incremental marking 期間，每個安全點只要我們的 worklist 不空，就配置一個「工作 token」寫進那個 slot；cppgc 在自己的 incremental step 裡追到 token，token 的 `Trace`（延到 main thread）做一小段有預算的我們的 marking。這樣甲不必在一次 `Trace` 裡走完整棵子樹，也不用改 V8。兩個要先解決的問題：

1. **結束條件**：cppgc 收尾時，它的 worklist 裡剛好沒有 token、我們的 worklist 卻還沒空，cppgc 就會結束，活的物件沒標到，是 unsound 的。`Trace` 裡不能配置新的 token，只能事先準備一串（每個 token 還有工作時追下一個），要證明這串不會在收尾前斷掉；或者找到 cppgc 收尾前一定會呼叫的東西（例如一個 root 的 `Trace`）在那裡做完剩下的。結束條件要先寫進 TLA+ 的模型再實作。
2. **排程**：V8 一樣看不到我們的大小，切段之後工作還是可能擠到收尾（1d 的乙在 V8 自己的排程下就是這樣）。要量 `--incremental-marking-unified-schedule` 以外有沒有不改 V8 也能影響步調的方法（例如照我們的大小回報 external memory，只影響什麼時候開始，不影響每步做多少，要量）。

做得到的話，甲加切段在正確性和停頓上都接近乙、又不用 fork；做不到再評估完整 fork 的維護量。

## 接 wasmtime

- **現在的 wasm 文件**：guest 拿到的 handle 是 component model 的 resource，host 這一側存 root 表的索引（今天是 slot table 加上 cppgc 的 `Persistent`）；換過來之後就是 `Persistent<T>`，中間不經過 FFI。
- **wasm GC 的物件（「之後的可能」）**：見下一節。
- **guest 的 handle 直接是 GC 參照**：要等 component model 的 canonical ABI 支援 GC 的參照，不經過 resource table。

### 照 wasmtime 的介面實作 collector

2026-10-24 讀 wasmtime 49.0.2 的原始碼：

- **一個 collector 有兩半**：runtime 那一半是 `wasmtime` crate 的 `GcRuntime` 和 `GcHeap`（`runtime/vm/gc/gc_runtime.rs`），compiler 那一半是 `wasmtime-internal-cranelift` 的 `GcCompiler`（`func_environ/gc.rs`），它在 wasm 的機器碼裡產生 inline 的配置和 barrier。現有的三個：null、DRC、copying（`func_environ/gc/copying.rs`：inline 的 bump 配置、空間滿了才呼叫 `gc_alloc_raw`、不需要 barrier、用 stack map 找 root 並更新）。49 版的 `Collector::Auto` 預設就是 copying。兩半都是 crate 內部的介面，要接進來得 fork 這兩個 crate，每次升 wasmtime 都要 rebase（這部分的上游還在快速變動）；請上游開放的事先寫草稿給人審核。
- **模型很接近這份**：`GcHeap` 是一段連續的記憶體，`VMGcRef` 是 32 位元的 index（最低位元標 `i31ref`），和這份的 cage 加 32 位元 `Member` 是同一個做法；struct 和 array 的佈局由 collector 決定（`GcTypeLayouts`），可以用這份的 layout 描述；copying collector 已經證明 wasmtime 這一側能讓 collector 搬動物件。
- **落差一：沒有 concurrent 的介面**。`GcHeap::gc` 拿 `&mut self`，`GarbageCollection::collect_increment` 寫明 mutator 不在 increment 之間跑。concurrent marking 要在 fork 裡加：marking 的開始和收尾、`GcCompiler` 在 `struct.set` / `array.set` 產生 card 和 SATB 的 barrier（wasm 碼直接寫欄位，不經過 host，所以 barrier 一定要在產生的碼裡）。
- **落差二：沙盒**。`GcHeap` 的文件規定 heap 是一個沙盒：index 要檢查邊界，heap 壞掉時壞的範圍只能在 heap 裡，所以 heap 裡不能寫進或讀出 native 指標（`externref` 的 host 資料放在 side table、heap 裡只存 id）。DOM 的物件如果和 wasm GC 的物件在同一個 heap，就要照這條規則。2026-10-24 定的是「現在丙、API 照乙」（`README.md` 的已決定的事項 7）：DOM 的 native 資料一律經過 `External<T>`，現在直接放在物件裡，wasm GC 的物件進來時量過再決定換不換成 side table；也不綁定 wasmtime 現在的介面（它還在變動），只確保最終能以某種形式支援。比較過的選項：
  - **甲、兩個 heap**：wasm GC 的物件放在照 wasmtime 規則的 heap（同一套演算法、沙盒的模式），DOM 放在自己的 heap，互相的參照經過 root 表。簡單，但兩邊之間的循環收不掉（和今天 `externref` 加 cppgc 的問題一樣）。
  - **乙、一個 heap，DOM 照沙盒的規則設計**（API 照它，表示等 wasm GC 的物件進來時再決定）：heap 裡只有 `Member`、純資料和索引，native 的資料（`Arc<ComputedStyle>` 這類）放在 side table。循環收得掉、wasm GC 的物件和 DOM 在同一次 scavenge 裡；heap 本身也成了沙盒（多一層防禦）。代價是 DOM 的 native 資料多一次間接。host 這一側照這份的 safe API 存取，`Gc<'gc, T>` 已經保證有效，不需要每次檢查邊界。
  - **丙、一個 heap，在 fork 裡放寬沙盒的規則**（現在的表示）：wasm 碼只能經過型別檢查過的 `struct.get` 碰自己型別的物件，碰不到 DOM 物件的 bytes，所以實際上安全；但失去 wasmtime 的設計裡那一層防禦（wasmtime 產生的碼或我們的 barrier 有 bug 時，壞的會擴散到 host 的記憶體），fork 的改動也和上游的假設衝突。wasm GC 的物件進來之前沒有 wasm 碼寫這個 heap，這個風險還不存在。
  - 丙之後要換成乙，代價在 DOM 而不在 GC：每個直接放 native 資料的欄位都要改。所以 DOM 一開始就經過 `External<T>`、由 derive 檢查，換的時候只改 `External` 和 GC（side table 的一格跟著它的物件 marking、scavenge 和 sweep）。乙在 DOM 上的成本是 `External` 每次存取多一次讀取；新的 DOM 讓熱的資料（字串、屬性、旗標、樹的邊）都是 heap 裡的純資料，只有少數冷的欄位（例如 `ComputedStyle`）在 `External` 裡。
  - 甲、乙、丙的 GC 本身一樣，GC 的第 2 到 6 段不受影響。
- **時機**：wasmtime 在 wasm 配置失敗時 GC（在 wasm 碼的中途，root 由 stack map 給）。這時一定不在一次 DOM 的操作裡（wasm 碼只在操作之間跑；host 呼叫 guest 的 callback 也要在操作的邊界），所以對這份來說也是安全點，規則不用改。

## 驗證的工具

### 要證的是什麼

1. **safety**：活的物件絕不會被回收（違反就是 use-after-free）。
2. **heap 的完整性**：每個指標都指到一個合法物件的開頭，搬動之後也一樣。
3. **API 的 soundness**：只用 safe API 的程式不可能造成 UB。
4. **liveness**：垃圾最後會被回收。違反只是記憶體洩漏，不證，用測試和量測看。

concurrent marking 漏標一個活的物件，必須同時發生兩件事（Wilson 1992）：mutator 建了一條黑指向白的邊，而且所有從灰物件通到那個白物件的路徑都被刪掉了。Dijkstra 的 barrier 擋第一件，SATB 擋第二件；1 的核心就是「marking 結束時活的物件都標過」這一條不變式。

### 分層

GC 沒辦法當成一個整體證明，每一層用不同的工具：

| 層 | 要證什麼 | 工具 |
| --- | --- | --- |
| 演算法和協定 | barrier、black allocation、安全點、結束的判斷合起來不會漏標 | TLA+ 加 TLC（有限大小的 model checking）；要無界的證明時用 TLAPS、Isabelle |
| 記憶體模型 | 弱記憶體序（ARM）下，marker 和 mutator 的 atomic 操作仍然對 | loom、Miri；GenMC 要先試它對 Rust 的支援 |
| 實作照著協定 | Rust 程式真的照模型做 | 執行期的 heap verifier（檢查和模型同一條不變式）；Verus 只用在小範圍 |
| 算術 | 壓縮指標、bitmap 的索引、size class | Kani |
| API | `Gc<'gc, T>` 活不出操作、`Member` 帶不出 heap | 型別系統、compile-fail 測試、Miri |

**TLA+ 的模型要涵蓋的**：一般的 SATB 模型（root、欄位的寫入、marker 一步一步走）之外，這份的設計裡靠手寫論證的每一點（上面的「記憶體模型」）都要是模型裡的動作：

- marking 期間的 scavenge（young 的過濾、升上來的物件算標過、marker 在物件的邊界停下）；
- weak 的讀取（id 表、`WeakMember`）在 marking 期間的 barrier；
- `Persistent` 的刪除（root 表的 deletion barrier）；
- black allocation 的發布（marker 看到指標時一定看到 mark bit）；
- 還沒 sweep 的 page 上死掉的物件的 slot；
- ephemeron 的重複處理、compaction 的 slot 記錄。

模型也要做 mutation：拿掉其中一個 barrier、或新配置的物件不算標過，TLC 要找到反例；反例改寫成 loom 和 stress 的測試。模型的一步比真的程式粗（`Write` 在模型裡一步做完，程式裡是讀 phase、跑 barrier、寫欄位幾步），這份能這樣簡化是因為 phase 只在安全點改、一個 heap 只有一個 mutator，所以 mutator 不會在寫到一半時碰到 phase 改變；這條本身也是程式要守住的前提，寫進 soundness 文件。

第 1 段就寫模型，SATB 和 Dijkstra 各一份，回答 `README.md` 的待決定 1；第 4 段定協定時再擴充成完整的版本。

### 工具和做法

`unsafe` 只在 GC 的 crate；每一項守住什麼：

| 工具 | 守住什麼 | 做法 |
| --- | --- | --- |
| TLA+（TLC） | concurrent 協定不會漏標（上面列的每個動作） | 模型和 GC 的 crate 放在一起；CI 跑有限大小的檢查；改協定時先改模型 |
| Miri | 整個 GC 沒有 UB：配置、搬動、指標的算術、marker 和 mutator 的 data race、弱記憶體序 | page 改用 `std::alloc` 的 backend（Miri 不支援預留虛擬位址），cage 是一大塊配置；小的 heap、少量物件；含 marker 和 sweeper 的 thread；CI 每天跑 |
| loom | concurrent 的協定：SATB buffer 的交出、worklist、marking 結束的判斷、scavenge 時 marker 的停下和繼續、發布的 release/acquire | 協定寫成可以單獨跑的小模型，用 loom 窮舉 interleaving |
| shuttle | 比 loom 大、窮舉不完的情況 | 隨機排程，跑很多次 |
| Kani | 算術：壓縮指標的轉換、bitmap 的索引、size class、slot set 的對應 | bounded 的證明 |
| Verus（之後評估） | barrier 和 marking 的不變式（「README」的待決定 9） | 協定在第 4 段定了再決定 |
| TSan | 真的 thread、真的 page 下的 data race | CI 的 nightly |
| heap verifier | marking 做完時每個到得了的物件都標過；scavenge 之後沒有指向 nursery 舊位置的指標；slot set 涵蓋所有 old 指向 young 的欄位 | debug build 和 stress 模式在每次 GC 之後跑 |
| stress | 很少見的 interleaving | 每個安全點都 scavenge、marking 的每一步、每次配置之後讓 marker 走一段 |
| 毒值 | use-after-free 變成立刻可見的錯 | debug build 把釋放的記憶體和 nursery 的舊一半填上固定的值 |
| fuzz | 物件圖和修改的組合 | 隨機的圖、隨機的寫入和 root 的增刪，在每一種 stress 模式下跑，結果和一個簡單的參考實作（不回收的 heap）比 |
| compile-fail | safe 的 API 擋住的寫法（`Gc` 帶出操作、`Member` 帶出 heap、`Drop` 碰到 GC 物件、物件自己實作 `Drop`、欄位直接放 native 的資料） | 照 `dom` 現有的 compile-fail 測試 |

**怎麼接成流程**（2026-10-25 的看法，照 bbtree 的 `design/08-fuzzing-verification.md` 和 `09-concurrency.md`；第 2 段開始前定）：

- **fuzz**：輸入是結構化的一串操作（配置、寫邊、root 的增刪、安全點、GC），解碼到 bytes 用完；參考模型是不回收的圖（到得了的物件欄位都還在、到不了的剛好 finalize 一次）；每次 GC 之後跑 heap verifier（bbtree 的 `check_invariants`，只在 `cfg(fuzzing)` 下公開）；target 照 GC 的模式分（scavenge、marking 的每一步、收尾、compaction），各分一段時間；找到的 crash 縮小後存成 regression、和修正一起進 repo，corpus 每次接續。0g 的錄製（`.kungc`：真實的 DOM 工作的配置、寫邊、root 和安全點）縮小之後當種子，fuzz 從真實的形狀出發。bug 多半是 use-after-free，開 ASan 和 debug 的毒值；Miri 太慢、不拿來 fuzz，但重播 regression。
- **DST（確定性的排程）用 shuttle**：只在共享的 atomic 上放切換點（SATB buffer 的交出、mark bit、worklist 的 push / pop / 偷、black allocation 的發布、card），`cfg(shuttle)` 開關、平常編成空的；每個測試把 heap 擺在離一個變化只差一步的形狀（marking 快收尾時 mutator 正在覆寫、marking 中途的 scavenge），兩三個 thread、各幾個操作，數有多少排程碰到那個變化、一個都沒有就失敗；測試本身也做 mutation。shuttle 一次跑一個 thread，抓不到弱記憶體序的重排，那部分照舊是 loom 和 Miri。分工：TLA+ 管協定、shuttle 管實際程式的 interleaving、loom 管記憶體序。
- **Verus 只證小範圍，證過的程式就是跑的程式**：證明和規格寫在 `verus!` 裡，一般的 build 拿掉、crate 呼叫同一段程式；放在輸入無界、錯了 sanitizer 也看不出來的地方（壓縮指標、bitmap 的索引、card 的對應、或許 barrier 的 fast path），信任不證的部分標出來。第 2 段拿其中一個模組和 Kani 比過再定（待決定 9）。不用 Verus 取代 TLA+：TLA+ 驗證設計、自動找反例、改協定只要重跑；Verus 驗證程式碼、無界但要人寫能歸納的不變式。協定要無界的證明時（待決定 9），Verus 的 state machine 是取代 TLAPS、Isabelle 的選項，等協定在第 4 段定了再評估。

論證寫在 GC 的 crate 自己的 `soundness.md`，格式照 `crates/dom/soundness.md`：假設、不變式、每一處 `unsafe` 依賴哪幾條、找到的漏洞、人工審查清單。

## 量測的計畫

2026-10-24 和使用者確認的三種比法；工作和工具是第 0 段：

1. **和 cppgc 在同一組 DOM 的工作上比**：同一組 wasm guest 的工作分別跑在現在的 cppgc heap 和新的 heap 上。工作照 DOM 的特性挑：建一棵大樹（parse）、反覆建和拆子樹（框架的 reconcile）、`innerHTML` 的替換、大量的 event 和 listener、很長的清單捲動（建新的、丟舊的）。可以參考 Blink 的 `third_party/blink/perf_tests/` 裡的 DOM 頁改寫成 guest。量：配置的速度、每次 pause 的長度（分布，不只平均）、main thread 花在 GC 的時間、記憶體的峰值。
2. **和 V8 的 GC 在同樣的配置模式上比**：同一個配置模式（樹、鏈結串列、隨機的圖；存活率高和低）分別寫成 JS 在 V8 上跑、和寫成 Rust 在我們的 heap 上跑，比 scavenge 和 marking 每秒處理的量、pause 的長度。V8 那一側用 `--trace-gc` 這類旗標取數字。
3. **目標寫成數字**（2026-10-24 的看法，第 0 段量完基準後定下來）：
   - 配置的 fast path inline，nursery 的配置只有加法、比較、寫 header；
   - minor GC 的 pause 在 1 ms 以內（典型大小的 nursery、DOM 型的存活率）；
   - old 收尾的 pause 在 1 ms 以內；
   - 同一組工作上 main thread 花在 GC 的時間不多於 cppgc；
   - 記憶體的峰值不超過 cppgc 的一定比例（SATB 的 floating garbage 會讓它高一些，比例在第 0 段定）。

量測照 `docs/benchmarks.md` 的規則（結果寫在哪、怎麼記錄機器和 commit）；Speedometer 不在這份的計畫裡。

## 和其他 GC 的對照

| | 搬動 | 分代 | marking | barrier | root | finalizer |
| --- | --- | --- | --- | --- | --- | --- |
| **這份** | young 搬動，old 可選的 compaction | nursery 加 promotion | concurrent | SATB 加 black allocation，generational 的 slot set | 精確：root 表，只在安全點 GC | 預設沒有；`Send` 的在 sweeper |
| V8（JS heap） | young 搬動，old 平行 compaction | 有 | concurrent | Dijkstra 加 black allocation | 精確：`HandleScope` | 沒有（weak callback） |
| Oilpan（cppgc） | 不搬（backing store 可選的 compaction） | 可選，sticky mark bit | concurrent | Dijkstra | 保守掃描 stack 加 `Persistent` | 每個有 destructor 的物件，在 main thread |
| JavaScriptCore（Riptide） | 不搬 | sticky mark bit | concurrent | retreating wavefront | 保守掃描 stack | 有 |
| SpiderMonkey | nursery 搬動，old compaction | 有 | incremental（另有平行） | SATB 式的 pre-barrier | 精確：`Rooted` 加 hazard analysis | 有，可以在背景 |
| Go | 不搬 | 沒有 | concurrent | hybrid（Yuasa 加 Dijkstra） | 精確，stack 不重掃 | `runtime.SetFinalizer` |
| Java G1 | 搬動（region 的 evacuation） | 有 | concurrent | SATB | 精確（stack map） | 有（Cleaner） |
| Java ZGC | concurrent 搬動 | 有（JDK 21 起） | concurrent | load barrier（colored pointer） | 精確 | 有 |
| LXR | 不搬為主（Immix，可以 defrag） | refcount 承擔 young | concurrent tracing 處理循環 | refcount 的 barrier | 精確 | — |
| gc-arena | 不搬 | 沒有 | incremental，單 thread | 寫入時 back barrier | brand，只在 `mutate` 之間 GC | `Drop` |
| wasmtime | DRC 不搬；copying 搬動 | 沒有 | stop-the-world | DRC 的 refcount | stack map | — |

這份從各家拿的：V8 的整體架構和 pointer compression；Oilpan 的名稱和 DOM 的規則；G1 和 SpiderMonkey 的 SATB；gc-arena 的 API 模型；Go 的「不重掃 stack」在這份是「只在安全點 GC、root 表有 deletion barrier」的結果。不拿的：保守掃描（Oilpan、Riptide）、read barrier（ZGC，DOM 的讀遠多於寫）、refcount（LXR、WebKit 的 DOM、Gecko 的 cycle collector，樹的互指讓 refcount 收不掉、每次寫邊要改計數）。

## 參考資料

- V8 的部落格：
  - "Orinoco: young generation garbage collection"（2017）：<https://v8.dev/blog/orinoco-parallel-scavenger>
  - "Concurrent marking in V8"（2018）：<https://v8.dev/blog/concurrent-marking>
  - "Trash talk: the Orinoco garbage collector"（2019）：<https://v8.dev/blog/trash-talk>
  - "Pointer compression in V8"（2020）：<https://v8.dev/blog/pointer-compression>
  - "High-performance garbage collection for C++"（2020，cppgc）：<https://v8.dev/blog/high-performance-cpp-gc>
  - "Oilpan library"（2021）：<https://v8.dev/blog/oilpan-library>
- WebKit 的部落格，"Introducing Riptide: WebKit's Retreating Wavefront Concurrent Garbage Collector"（2017）：<https://webkit.org/blog/7122/introducing-riptide-webkits-retreating-wavefront-concurrent-garbage-collector/>
- Go 的部落格，"Getting to Go: The Journey of Go's Garbage Collector"（2018）：<https://go.dev/blog/ismmkeynote>
- Zhao、Blackburn、McKinley，"Low-Latency, High-Throughput Garbage Collection"（PLDI 2022，LXR）。
- Blackburn、McKinley，"Immix: A Mark-Region Garbage Collector with Space Efficiency, Fast Collection, and Mutator Performance"（PLDI 2008）。
- Yuasa，"Real-time garbage collection on general-purpose machines"（1990，SATB 的 deletion barrier）。
- Jones、Hosking、Moss，《The Garbage Collection Handbook》第二版（2023）。
- Wilson，"Uniprocessor Garbage Collection Techniques"（IWMM 1992，三色不變式和漏標的兩個條件）。
- Gammie、Hosking、Engelhardt，"Relaxing Safely: Verified On-the-Fly Garbage Collection for x86-TSO"（PLDI 2015，在 Isabelle/HOL 證明 concurrent GC）。
- Ericsson、Myreen、Pohjola，"A Verified Generational Garbage Collector for CakeML"（ITP 2017）。
- V8 和 Oilpan 就目前所知沒有形式化驗證，靠的是 heap verifier（`--verify-heap`）、DCHECK 和 fuzz。
- gc-arena：<https://github.com/kyren/gc-arena>
- bbtree（Rust 的 B-tree，2026）：<https://github.com/XiangpengHao/bbtree>，驗證的做法在 `design/08-fuzzing-verification.md`。資料結構本身這份用不到：root 表、id 表、bitmap、card、page 都用索引或位址算，沒有有序的查詢（要從任意位址找 page 的是保守掃描，cppgc 為此用一個 `std::map`，這份不掃 stack）。可以參考的是它把 Miri、sanitizer、shuttle、fuzz 和 Verus 都接進 CI 的方式，第 2 段建 GC 的 crate 時對照：Verus 只證一個小的 `proof` 模組，證過的程式就是 crate 實際呼叫的（`verus!` 在一般的 build 裡拿掉規格和證明），對應待決定 9 的「只做 barrier 的 fast path 這種小範圍」；fuzz 每一步之後檢查不變式，壞掉的節點在做出來時就抓到（對應 heap verifier）；fuzz 找到的 crash 縮小後存成 regression，和修正一起進 repo。2026-10-25 看的。
- 專案裡的：`crates/dom/soundness.md`（現在的 GC 的論證）、RFC 0060（no-GC scope）、RFC 0085（來源 1 的量測）、RFC 0067（wasm 每次呼叫的成本）、RFC 0001 的 `wasmtime-questions.md`（wasmtime 的 GC 和 fiber）、RFC 0005（wasm GC 用 cppgc 的方向）。
