# RFC 0090 的進度：配置的快路

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-07，Linux 7.2、Fedora 44（AMD Ryzen AI 9 HX 370）。在 `0090` 上做。

完成日期：2026-11-08，同一台。第 1 段是為原本的範圍「GC 不再搬動」量的（決定 1、2），之後範圍改成配置的快路（決定 3）。

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 量法（六項和使用者要求的 minor GC 的模擬；為原本的「GC 不再搬動」做的，決定 2；之後範圍改了，決定 3） | 完成 | 2026-11-08 |
| 2 配置的快路（決定 4） | 完成 | 2026-11-08 |
| 3 量測、文件 | 完成 | 2026-11-08 |

## 驗收

都在 Linux 7.2、Fedora 44（AMD Ryzen AI 9 HX 370）上驗。第 2 段做完時 `cargo test --workspace` 59 個 binary、3940 個過、1 個 ignored，`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 沒有問題；之後只改了文件。整套的 WPT、reftest、情境、`matrix`、`fuzz` 由每天的 CI 跑；這份動到 GC，merge 之後看 CI 的 `matrix`、`fuzz`。

**結果**（Chrome 是 Chrome for Testing 152）

| 項目 | 開始 | 完成 |
| --- | --- | --- |
| 取樣的 GC 那一類（Speedometer 的 20 個 suite 合計，每個 iteration） | 37.3 ms（Chrome 17.8） | 28.0 ms（GC 和 DOM 合計 146.0 → 139.5：配置 inline 進 DOM 的函式） |
| Speedometer 的比值（每個 suite 的比值的幾何平均） | 1.432 | 1.428（suite 合計 1005.0 → 992.3 ms） |
| `bench.py calls` 的 `createElement` | 2237 條指令（456 cycle） | 2103（450）；其他成員 ±3 條 |
| blink-perf | | `gc-forest` −8.9%、`gc-mini-tree` −10.4%、`create-element` +7.1%、`gc-tree` +1.2%（起伏裡），幾何平均 −6.3% |

**配置的快路**

| 項目 | 怎麼驗證 |
| --- | --- |
| 沒有 nursery 的 heap 裡同一級的物件只在換區段時走慢路，`HeapVector` 也是 | `gc` 的 `alloc::a_heap_with_no_nursery_allocates_by_a_bump`、`a_vector_is_allocated_by_a_bump`（`slow_allocations` 等於 `old_ranges`）；`mutants-2.toml` |
| `used_bytes` 是物件的格子，不含 cursor 的尾巴；GC、engine 驅動的 marking 之後也對 | `alloc::a_heap_with_no_nursery_allocates_by_a_bump`、`js::a_driven_marking_ends_with_the_live_bytes`、heap verifier 的帳；`mutants-2.toml` |
| 門檻小的 heap 照樣先開始 concurrent marking | `sweep::the_send_finalizers_run_on_a_worker`；`mutants-2.toml`（卡住） |
| 沒有 nursery 的 heap 不數 site，有 nursery 的照樣數 | `alloc::a_heap_with_no_nursery_counts_no_site` |
| 快路的有 drop glue 的物件 finalize 一次，`LocalExternal` 的在 heap 的 thread 上 | `alloc::the_fast_paths_objects_with_drop_glue_are_finalized_once`、`the_fast_paths_local_drops_run_on_the_heaps_thread`、verifier 比頁上的計數和 bit；`mutants-2.toml` |
| `dom_js` 每次操作之後不算精確的大小 | `bench.py calls`；沒有測試守著，記在 `docs/test-gaps.md` |

**第 1 段的量測**（原本的範圍）

| 項目 | 結果 |
| --- | --- |
| JS 文件的 heap 要不要 nursery | 不要：有 wrapper 的 DOM 物件死得慢，不搬動的 minor GC 在 1 到 4 MB 只收得掉 16% 到 25%；拿掉 nursery 一樣快、記憶體不變或更低（RFC 0085 的 5b） |
| 沒有 engine 的 heap 沒有分代時 | 短命的工作慢 5% 到 16%，整棵樹活著的快 4% 到 38% |
| 整棵樹活著的頁沒有 nursery 時慢約 3% | 一部分是資料的位址排列，不是配置的位置 |

## 做過的檢查

### 1：量法（2026-11-07）

暫時的探針（`gc` 的 `heap.rs`、`bindings` 的 `unified_heap.rs`、`dom_js` 的 `State`、`dom` 的一個測試，`MACHOPO_GC_PROBE` 打開；量完還原，沒有 commit）：每次 full GC 標完、sweep 之前，看上次 full GC 之後在 old 配置的每個物件有沒有被標到（marking 期間配置的黑物件不算），照型別加總；V8 的 scavenge 丟掉 wrapper 時（`Roots::reset`）記下物件，看它到 full GC 時死了沒有；配置問的大小和 size class 的大小；marking 開始時的碎片（`measure_fragmentation`）。

- **第 1 項，JS 文件裡的物件的壽命**（Speedometer 的 20 個 suite 各 10 個 iteration，216 秒；探針讓每次配置變慢，GC 的次數和時間不能用，只看比例；13 個 suite 有 full GC）：兩次 full GC 之間在 old 配置了 914.6 MB；到下一次 full GC 時，有 JS 邊的型別（節點這類）300.2 MB 裡死了 88.7%，其他型別 593.6 MB 裡死了 84.4%。死得最多的是 `Node` 256 MB、`Attribute` 127、`ElementData` 113、`CharacterData` 108、`MemberArray` 49、`InputType` 37、`FormAssociation` 30。V8 的 scavenge 丟掉 wrapper 的 181 萬個物件，到 full GC 時死了 57.5%；各 suite 差很多：TodoMVC-jQuery 95.8%、ES5 96.0%、TipTap 98.0%，ES6-Webpack 44.8%、Angular 16.8%、React-Complex-DOM 0.1%。RFC 0087 在錄製上量到 JS 文件 95% 活下來，在今天的 Speedometer 上不成立：用 `innerHTML` 或框架一直重建的頁，節點建了就丟。這是上限：「到 full GC 時死了」不等於「在 minor GC 時就死了」。
- **第 2 項，事件多的長時間頁面**（scratchpad 的一頁：200 個有 listener 的元素，每個 frame 分派 300 個 bubble 的 `mousemove`、listener 讀 event、偶爾改 class，120 秒，`kun --run-web-tests`）：headless 沒有 vsync，120 秒跑了約 15.5 萬個 frame、4670 萬個事件，比真的頁面嚴苛得多。沒有 nursery：155543 個 frame、1328 次 full GC、每輪進 old 約 8.4 MB；有 nursery（暫時把 renderer 的 heap 改回預設）：154223 個 frame（−0.8%，在起伏裡）、1275 次 full GC、每輪進 old 約 2.3 MB（幾乎都是有 wrapper 的 `Event`）。full GC 的次數兩邊差不多：決定它的是 V8 那一側的 JS 配置（事件的 wrapper、closure），不是 DOM 的短命資料。
- **第 3 項，碎片**：marking 開始時，有物件的頁上空格子的比例（每個 suite 最高的那一次）：TodoMVC-jQuery 34.5%、ES5 57.4%、NewsSite-Nuxt 50.9%、Perf-Dashboard 61.8%、React-Complex-DOM 8.6%（Charts-chartjs 94% 但 heap 只有 0.2 MB）。
- **第 4 項，`calls` 的基準**（`bench.py calls`，探針拿掉之後，150 秒）：`firstChild` 176 條指令（32 cycle）、`nodeType` 154（29）、`getAttribute` 700（152）、`setAttribute` 1250（245）、`appendChild` 1316（224）、`createElement` 2237（456）、`addEventListener` 865（177）、`dispatchEvent` 3467（757）。
- **第 5 項，沒有 engine 的 heap**（wasm 文件的 guest 沒有 benchmark 的工作，RFC 0087 的錄製和 `MACHOPO_GC_RECORD` 跟著舊的 DOM 拿掉了；改用 `dom` 的一個暫時的測試在 `Dom::with_options` 的 heap 上跑同樣形狀的工作，release，綁在快的核心，各 3 次的中位數；第一次沒綁核心、剛編完跑的那一輪差到兩倍，不算）：`create-element` 式（建了就丟，200 萬個）有 nursery 55.6 ms、沒有 61.0（+10%）；`gc-tree` 式（10 萬個 div 的樹整棵活著再放掉，20 次）109.3、105.3（−4%）；`innerHTML` 式的 churn（100 個 item 帶屬性、input、文字，換 2000 次）153.5、175.0（+14%）。full GC 的門檻（`min_allocation_bytes`，預設 2 MiB）改成 16 MiB：+9%、−22%、+5%；64 MiB：+12%、−38%、+16%。短命的工作慢 5% 到 16%，整棵樹活著的反而是沒有 nursery 快（不用複製）。
- **第 6 項，size class 和局部性**：配置的 bytes 比物件問的多 4.5%（各 suite 3.7% 到 9.3%）。局部性見下面的「整棵樹活著的頁為什麼慢」：第一次量到的 9% 和「L2 的 miss 多一倍」（用 L2 的請求減命中算的）都不對。
- **第 7 項，不搬動的 minor GC 能提早收多少**（使用者要求多量的一項；暫時的探針：在 `Heap::mutate` 的安全點，上次模擬或 full GC 之後在 old 配置的物件到了門檻時，從 root 表、所有比它們舊的物件的邊（乙的 remembered set：死掉、還沒 sweep 的舊物件的邊也算，乙的 minor GC 也一樣）和 wrapper 還設著的新物件（V8 的 scavenge 丟掉的 wrapper 會被清掉）走一遍，走不到的是乙在那時收得掉的；活下來的照乙算成 old。Speedometer 的 20 個 suite 各 10 個 iteration）：門檻 1 MB 時新物件能收掉 16.2%（有 JS 邊的型別 18.7%、其他 15.0%），4 MB 時 25.2%（28.5%、23.6%），16 MB 時 43.4%（多半被 full GC 打斷，只模擬到 322 MB）。4 MB 時各 suite：TodoMVC-jQuery 14.9%、ES5 68.1%、ES6-Webpack 71.3%、Perf-Dashboard 39.7%、WebComponents 36.7%，React、Preact、Svelte、Angular、NewsSite 5% 到 7%。第 1 項的「到 full GC 時死了 85% 到 89%」大多是 minor GC 之後才死的：物件要等 V8 丟掉 wrapper、頁面放掉它，jQuery 這類頁的垃圾要到接近一整個 full GC 週期才死。乙的 minor GC 在 1 到 4 MB 時，新物件有 75% 到 84% 還活著。
- **整棵樹活著的頁為什麼慢**（使用者要求查清楚；照 `gc-tree` 寫的那一頁，兩份 kun 輪流各 6 次）：沒有 nursery 平均 2184 ms、有 nursery 2118（+3.1%），不是第一次量的 9%。兩份的 full GC 都是 3 次（暫停合計不到 0.1 ms，沒有撞到上限收尾，背景 marking 都約 35 ms）。排除了的：DOM heap 的配置（這頁只配 `Node`，有 JS 邊，兩份都直接配在 old，完全一樣）、V8 的 GC（`--trace-gc`：scavenge 114 次、164 對 163 ms，mark-compact 3 次、97 對 95 ms）、指令數（517 億，一樣）、page fault（14.3 萬，一樣）、L2 的 miss（`l2_cache_req_stat.ic_dc_miss_in_l2` 取樣 6976 對 7092，一樣）、dTLB 的 miss（216 萬對 216 萬）、huge page（這台的 THP 是 `madvise`，`AnonHugePages` 0）。多出來的 cycle 散在碰新節點和 wrapper 的函式（`c_fn`、V8 的 `CppHeapPointerTable` 的 `TryAllocateEntry`、`ret_new_node`、`Node::__init`）；有 nursery 時多一些我們的 marking 和 V8 那張表的 sweep 在 worker 上做。在沒有 nursery 的那份的 heap 一開頭墊一個活著的大物件（暫時的探針），只是讓之後的頁換位置：墊 1 MB 2228 ms（+1.5%）、墊 2 MB 2168（−1.3%）。所以這頁對資料的位址敏感，差的一部分是排列；剩下約 1.8% 沒有找到原因，不是配置的位置策略（兩份配在同樣的地方，只是位址不同）。RFC 0085 收尾時 blink-perf 的 `gc-tree` +6% 在這個範圍裡（那份的 4d 量過同樣的程式碼加一行就差 9% 到 10%）。

### 2：配置的快路（2026-11-08，決定 4）

- **改了什麼**：`gc` 的 `Mutation::alloc_at` 在沒有 nursery 的 heap（或一定配 old、沒有 site 的物件）上，型別有 size class、那一級的 cursor 有空間時，`Inner::bump` 之後 `publish_bumped`（寫 header；有 drop glue 的物件經過 `MarkTable::publish` 設那一頁的 bit，頁上的計數記在 cursor 的 `finalizable`、`main`，`refill`、sweep 交出頁之前和 verifier 之前由 `flush_cursor_counts` 加回）；其餘走 cold 的 `allocate_slowly`（`HeapStats::slow_allocations`）。`Inner::allocate`（`HeapVector`、`MemberArray`）也先 bump。小物件的配置量在 `refill` 時照整段算（`count_allocated`），large object 照舊逐個；`used_now`、`Heap::used_bytes` 扣掉 cursor 的尾巴（`unused_tails`），GC 清掉 cursor 時從 `used` 扣掉尾巴，marking 開始時 `black_bytes` 從尾巴算起。`alloc_in`、`alloc_beside` 在沒有 nursery 的 heap 直接用 site 0。這裡用頁號經過 mark table 拿那一頁的 bitmap，沒有在 cursor 裡放指標：沿用 `MarkTable::get` 的論證，不多一處 `unsafe`。
- **做的時候找到的**：
  - 門檻照整段算時，`min_allocation_bytes` 小的 heap（測試的 64 KiB）一次 `refill`（新的頁是 256 KiB）就越過整個門檻，safepoint 直接做阻塞的 full GC、永遠到不了 concurrent marking 的一半（`sweep::the_send_finalizers_run_on_a_worker` 卡住）：門檻改成 `allocated_past`，整段算的值到了才扣尾巴確認，平常的 safepoint 不多做事。
  - engine 驅動的 marking 收尾時 `used` 是 marked 加上 `black_bytes`（含 cursor 的尾巴），sweep 清掉 cursor 之後尾巴是空格子，`used` 多算（`js::a_driven_marking_ends_with_the_live_bytes`）：清 cursor 時從 `used` 扣掉尾巴。
  - `bench.py calls` 第一次量到每個會回報大小的成員多約 510 條指令（`getAttribute` 700 → 1213）：`dom_js` 每次操作之後讀 `used_bytes`，它現在要走過 51 個 cursor。加了 `Heap::counted_bytes`、`Dom::counted_bytes`（不扣尾巴，一次讀完）給「長了多少」的判斷，要報給 V8 時才用 `used_bytes`（`docs/test-gaps.md` 記了）。
- **測試**：`gc` 的 `alloc` 模組 5 個（同一級的物件只在換區段時走慢路、`used_bytes` 是物件的格子；沒有 nursery 的 heap 不數 site、有的照樣數；快路的有 drop glue 的物件死了 finalize 一次；`HeapVector` 也走快路；`LocalExternal` 的 drop 在 sweep 由 worker 跑時照樣在 heap 的 thread 上）。`gc` 的測試都過（9 秒），`dom`、`html`、`dom_api`、`dom_js`、`bindings`、`gc_bench` 都過（71 秒）。`dom_js` 的 `abort::a_timeout_aborts_as_its_timer_fires` 在整套平行跑時失敗一次：它在同一段 script 建兩個 `AbortSignal.timeout(10)`，只跑到第一個到期加 1 ms，機器忙的時候第二個晚建超過 1 ms 就還沒到期；改成跑到第一個到期加 1 秒（到期前不觸發的檢查照舊），不是這次的改動造成的。
- **mutation test**（`mutants-2.toml`，14 個，`gc` 的整合測試，timeout 180 秒）：12 個第一次就抓到（「門檻用整段算的值」卡住，算抓到）；「快路的物件沒算成 heap 的 thread 的」沒抓到：每次換區段的第一個物件走慢路、慢路的 `publish` 直接加頁上的計數，測試先配的就是有 `LocalExternal` 的物件，頁上總有一個；改成先配一個同一級、沒有 drop glue 的物件拿區段之後抓到。「`allocate` 沒有快路」沒抓到：加了 `HeapVector` 的測試之後抓到。等價、沒寫 mutant 的：`alloc_in` 在沒有 nursery 時照樣查 site 的編號（只多一次 atomic 的讀，`docs/test-gaps.md`）。`test.py mutate --restore` 沒有要還原的。
- **量測**（AC、綁在快的核心）：`bench.py calls`（151 秒）和第 1 段的基準比：`createElement` 2237 → 2103 條指令（−6.0%）、456 → 450 cycle，其他成員差 ±3 條。blink-perf（`--against HEAD`，各一輪，372 秒）：`gc-forest` 100.6 → 91.6 ms（−8.9%）、`gc-mini-tree` 122.9 → 110.1（−10.4%）、`gc-tree` 132.1 → 133.6（+1.2%，在起伏裡）、`create-element` 2281 → 2444 runs/s（+7.1%），幾何平均 −6.3%。Speedometer 的取樣還沒跑（使用者要求時）。
- **階段做完的檢查**：`cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`（沒有警告），`cargo test --workspace` 59 個 binary、3940 個過、1 個 ignored（113 秒）；`sanity.py` 的 numerics、unsafe、boundary、deps、spans、tests、docs 都過（docs 剩原本的 3 個 `elise` 的問題）。

### 3：量測和文件（2026-11-08）

- **Speedometer**（使用者同意；`bench.py speedometer --browser kun` 212 秒，再 `--sample --iterations 5` 55 秒，kun `3a5cba4f`；Chrome 那一側照舊用 Chromium 156 的樣本）：和 RFC 0085 收尾時（5b 的樣本配同一份程式碼的計時）比，取樣的 GC 那一類 37.3 → 28.0 ms（Chrome 17.8：差 19.5 → 10.2），DOM 108.7 → 111.5（配置 inline 進呼叫它的 DOM 的函式，一部分樣本跟著算到 DOM：GC 和 DOM 合計 146.0 → 139.5），bindings 35.0 → 32.4、HTML parse 17.9 → 16.1，其他在兩次取樣之間的起伏裡；合計 1005.0 → 992.3 ms，比值的幾何平均 1.432 → 1.428。GC 那一類最多的剩下 inline 的 `alloc_at`（`Attribute`、`CharacterData`、`ElementData` 各 1 到 2.5 ms）、`Inner::allocate`（`HeapVector` 和它的 store，2.4 ms）和 V8 自己的 scavenge、traced handle。
- **文件**：`docs/performance-gaps.md` 的整體、各類、每個 suite、GC 最多的函式和可以開 RFC 的方向照這次改寫；`docs/limitations.md` 的「GC 和 `dom`」裡「renderer 的 DOM heap 沒有 nursery」那一條、`docs/test-gaps.md` 的兩條、`crates/gc/README.md`（配置、測試）、`crates/gc/soundness.md`（I6 和配置的審查清單）在第 2 段改了。

