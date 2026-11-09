# RFC 0085：GC、binding、JS、DOM 往 Chrome 靠

- 開始日期：2026-10-24
- 狀態：✅ 已完成：2026-11-07（第 1 到 5 段；驗收和做過的檢查見 `progress.md`。GC 不再搬動的部分另寫 RFC，見「之後的可能」）
- 細節：`details.md`（換 DOM 之後兩邊用同樣方法取樣的分析：量法、Chrome 的估計、各類和每個 suite 的表、每一類最多的函式；換 DOM 之前（2026-10-24）的分析和逐項對照）

## 摘要

Speedometer 3.1 的 20 個 suite，kun 的時間是 Chrome 的 1.50 倍（每個 suite 的比值的幾何平均，2026-11-06，Linux、AMD Ryzen AI 9 HX 370）。換 DOM 之後兩邊照同樣的方法取樣（`details.md` 的「換 DOM 之後的分析（2026-11-06）」），差最多的是文字、JS、style、DOM、GC、box layout。這份只做其中的 GC、bindings、JS、DOM，分五段：兩邊的取樣和分類做進 `bench.py`；DOM 的 `innerHTML` 的路、找子 frame、屬性的字串參數；給 V8 的設定（照 Chrome 打開 `--sparkplug-plus` 等 JIT 和 IC 的四項）；binding 的固定成本和差距大的 blink-perf 頁（`document` 的 cached accessor、collection 的快取、shadow tree 的 retarget、`element.style`）；GC（配置的 size class、節點的資料跟著節點配在 old、renderer 的 heap 不用 nursery）。結果（2026-11-07）：Speedometer 是 Chrome 的 1.50 → 1.43 倍（suite 合計 1067 → 998 ms，取樣的 GC 那一類 58 → 38 ms），blink-perf 這四類的 26 頁 2.27 → 1.55 倍（`progress.md` 的「驗收」）。文字、style、box layout 不在這份；GC 不再搬動另寫 RFC。

## 動機

**現況**（2026-11-06；Chrome 的數字是估計：取樣的是 Fedora 的 Chromium 156，各類的比例乘上 Chrome for Testing 152 的時間，見 `details.md` 的「Chrome 的估計」）。每個 iteration 的 ms，20 個 suite 合計：

| 種類 | kun | Chrome | 差 | 只有這一類追平時的幾何平均 | 主要在哪 |
| --- | ---: | ---: | ---: | ---: | --- |
| JS | 349 | 248 | +101 | 1.381 | 同一批 JS 函式和 IC 的 builtin 各慢 1.2 到 1.5 倍 |
| DOM | 116 | 73 | +43 | 1.471 | 名稱和屬性的查找（`names::find_fixed`、`attribute_value_by_id`、`class_names`、`intern_value`），插入時的 `link`、`child_content_documents`、`form_subtree_inserted` |
| GC | 58 | 19 | +39 | 1.473 | `gc` 的配置（`Inner::allocate`、`publish`）比 scavenge 還多，write barrier 的 `SlotSet::insert` |
| bindings 和 V8 API | 35 | 24 到 38 | −3 到 +11 | 約 1.51 | 每次呼叫的固定成本 |
| HTML parse（`innerHTML` 的 fast path） | 22 | 15 | +7 | 1.519 | `fast_path::scan`、`decode` |
| 全部 | 1078 | 671 | +407 | 1.523 | |

- 幾類一起追平時：GC、bindings、DOM 1.41（加上 HTML parse 1.40），再加 JS 1.26。
- 每次呼叫（`bench.py calls` 的指令數，`main`）：`nodeType` 167、`firstChild` 189、`getAttribute` 751、`addEventListener` 886、`setAttribute` 1300、`appendChild` 1332、`createElement` 2308、`dispatchEvent` 3481。
- blink-perf 裡這四類的頁，kun 慢幾倍（2026-11-06）：`bindings/get-elements-by-tag-name` 14.3、`get-element-by-id` 5.88、`css/CSSPropertySetterGetter` 4.16（`element.style` 的 binding）、`bindings/gc-forest` 3.87、`create-element` 3.74、`events/EventsDispatchingInV1ShadowTrees` 2.91、`bindings/gc-mini-tree` 2.77、`gc-tree` 2.44、`id-setter` 2.31、`node-type` 2.08、`first-child` 2.00、`set-attribute` 1.90、`id-getter` 1.80、`get-attribute` 1.56。改樹的（`append-child`、`insert-before`）、`child-list-mutation-with-unrelated-observer`、`tiny-innerHTML` 已經比 Chrome 快。
- Speedometer 呼叫 DOM 的分布（RFC 0088 數的）：`nodeType` 8.4%、`setAttribute` 7.8%、`parentNode` 4.0%，最多的 17 個成員佔 45%。

**問題**：

- 兩邊的取樣和分類還不在 repo 裡：Chrome 那一側是暫時的腳本，每一段做完要用同樣的量法驗收。
- binding 的固定成本在 Speedometer 裡不大（取樣約 35 ms，和 Chrome 差不多），但 blink-perf 的單一操作還差 2 到 14 倍：呼叫越少做事的成員，固定成本佔得越多。
- JS 是差最多的一類，但大半在 V8 本身；kun 用的 V8 照現在的設定編（RFC 0086 否決），能動的只有給 V8 的設定。

**目標**：

- 這四類往 Chrome 靠（沒有定比值的目標：2026-11-06 的建議是四類合計的差距減半，約 1.40；做到 1.43，GC 的剩下的另寫 RFC）。
- 這四類的 blink-perf 頁盡量追上 Chrome（第 4 段）。
- 兩邊的取樣、分類和換算成 ms 的比較表做進 `bench.py`，成為固定的量法。

**要解決的 `docs/limitations.md` 的限制**：無。

**不在範圍內**：

- 文字、style、box layout、paint：數字和最多的函式在 `details.md`，另寫 RFC。
- 重編 V8（official build、PGO）：RFC 0086 否決，kun 用的 V8 照現在的設定編。
- V8 本身的效率（JIT 的程式碼、IC、runtime）。
- selector 比對的頁（`css/PseudoClassSelectors`、`parser/query-selector-all-class`）：算在 style。
- 新的功能。

## 設計概要

每一段改一類的程式，做完時：

- 跑那一段的測試和 mutation test；優化盡量有計數的測試守著（做了多少事），守不住的記進 `docs/test-gaps.md` 的「沒有計數守著的優化」；
- `bench.py calls` 一輪、受影響的 blink-perf 頁各一次；
- 段落收尾時，使用者要求時重新取樣兩邊，看那一類的差（Speedometer 平常不量）。

## 詳細設計

### 量法

照 `details.md` 的「換 DOM 之後的分析（2026-11-06）」：兩邊用 `perf record`（`cycles:u`，每 100 萬一個樣本，frame pointer，V8 的 perf map），20 個 suite 各 5 個 iteration，只留 runner 計時的區間裡 renderer main thread 的樣本；照最內層說得出種類的 frame 分類；每個 suite 各類的比例乘上它沒有取樣時計時的中位數。Chrome 是 Fedora 的 Chromium 156（有 debuginfo）的比例乘上 Chrome for Testing 152 的時間。

### 第 1 段：量測工具

- `bench.py speedometer --sample` 也能取樣 Chrome：Chromium 的執行檔（預設 Fedora 的 `/usr/lib64/chromium-browser/chromium-browser`）在 `perf record` 底下跑，加 `--no-sandbox`（V8 寫 perf map）和 V8 的 `--perf-basic-prof --interpreted-frames-native-stack`；trace 只錄 `blink.user_timing`（完整的 category 讓 Chrome 慢一倍）；renderer 的 main thread 照 trace 的 thread descriptor 裡 `CrRendererMain` 的 tid 挑（Linux 上 Chromium 不給 main thread 命名）；Chrome 的 trace 是 `CLOCK_MONOTONIC`。
- 分類做進 `bench.py`：kun 照 crate、Blink 照類別名稱（規則照 `details.md` 的「量法」，C++ 的名稱去掉回傳型別、template 和參數），兩邊共用 V8、JS、配置的規則。
- 報告：各類和每個 suite 的 ms（比例乘上計時的中位數），「只有這一類追平時的幾何平均」，每一類最多的函式。
- 先核對 Chromium 156 和 Chrome for Testing 152 的時間（報告裡列出每個 suite 的比）。
- 測試：分類的規則（兩邊各幾條真的 stack）、tid 的挑法、時鐘、換算；`speedometer_test.py`。

### 第 2 段：DOM

原本的範圍（名稱和字串、改樹時的工作、查找和 collection、`innerHTML` 的 fast path）照取樣重排成三項（決定 8），查找和 collection 移到第 4 段：

- **2a 找子 frame**：renderer 每轉一圈 loop 問動畫要不要 frame 時，engine 照 frame 建立的順序看（`frames_as_made`），不走樹；rendering update 開始時找一次文件（`begin_rendering_update`），每一步共用（決定 8）。
- **2b `innerHTML` 的路**：fast path 的元素帶著屬性建（`create_html_element_with_attributes`：名稱用編號、屬性不先找同名的、清單一次配到大小、input 照 `type` 建，Blink 的 `HTMLFastPathParser`、`InitializeTypeInParsing`）；屬性的值和文字只解碼一次（決定 9）。
- **2c 屬性的成員和字串參數**：先和 Chrome 比，字串換成名稱編號那部分 Chrome 不比 kun 快（短的字面值 V8 不讓 externalize，Blink 每次查 `AtomicStringTable`），所以不做 V8 字串到編號的快取；做：id 的值沒變不動 id 表、轉小寫的判斷照旗標、字串參數的內容只借一次（決定 10）。

### 第 3 段：JS（給 V8 的設定）

kun 和同一版 V8 的 Chrome for Testing 152 給 V8 的 flag 逐項比（renderer 差 14 項）；同一版的 Chrome 取樣之後看得出差在 Sparkplug：Chrome 照 feedback 呼叫特化的 IC builtin、把比較和算術 inline（`--sparkplug-plus`）。`bindings` 的 `CHROME_FLAGS` 照 Blink 的 `gin::V8Initializer` 把 Chrome 的 renderer 打開的 JIT 和 IC 的四項設成預設，放在呼叫者的 flag 和 `MACHOPO_V8_FLAGS` 之前（決定 11）。V8 的 heap 的六項、`verify-bytecode-light` 不跟；JIT 的程式碼和 builtin 剩下的 1.1 到 1.3 倍是 V8 的 build（PGO、official build），不重編 V8 時到這裡為止。

### 第 4 段：binding 和 blink-perf 的頁

一頁一頁找原因，分六項（決定 12）：

- **4a `document` 的 cached accessor**：全域的 `document` 每讀一次多 65 ns（global object 是 dictionary mode，V8 的 IC 對它上面的 API accessor 只給 slow stub）。照 Blink 的 `[CachedAccessor]`：rusty_v8 的 fork bind `FunctionTemplate::NewWithCache`，`Window` 的 `window`、`document` 的值放在 global proxy 的 private property，換 document 時更新（Blink 的 `LocalWindowProxy::UpdateDocumentProperty`）。
- **4b `getElementsByTagName` 的 collection**：照 Blink 的 `NodeListsNodeData`，根的 rare data 上弱弱地留著最近的 8 個，同一個根再問同一個名字給同一個物件。
- **4c getter 的固定成本**：逐層拿掉量（決定 13）；做了 `Call` 的 drop 不做事時 inline 跳過（每個成員少 12 到 14 條指令）。unwrap 照 Blink 的 `ToWrappableUnsafe` 先不做（決定 14）。
- **4d 建節點**：GC 的配置（第 5 段的第一步，決定 6）提前到這裡（決定 15）；`createElement` 逐層拆，剩下的大多是 V8 建、接上 wrapper 的機制。
- **4e `dispatchEvent` 經過 shadow tree**：retarget 照 Blink 的 `EventPath::BuildRelatedNodeMap`，一個 tree scope 算一次（`Retargeter`）。
- **4f `element.style`**：只在宣告有沒解析的 `url()` 時才建 URL 的環境、`about:` 先用字串判斷、小於 10⁶ 的整數直接寫。名稱查找記住的表試過、還原了。

### 第 5 段：GC

2026-11-07 從第 2 段移到最後（決定 7：第一步讓 GC 那部分變少，整頁的時間卻被程式碼的排列蓋過）。

- **配置**（決定 6，在 4d 做）：型別的 size class 和「有沒有 JS 邊」在編譯時算好，慢路移出快路，`publish` 的 finalizable bit 用一般的讀寫。
- **5a 資料跟著擁有者配在同一代**（決定 16、17）：節點因為有 JS 邊一律配在 old，它的資料卻配在 nursery，每次掛上記一筆 old 到 young，scavenge 一定要搬兩次（TodoMVC-jQuery 每 10 個 iteration 搬 246 MB）。`gc` 加 `Mutation::alloc_beside`、`HeapVector::new_beside`（vector 長大時的 store 跟著 vector），`dom` 掛在節點上的資料都用它。
- **5b renderer 的 heap 不用 nursery**（決定 18、19）：5a 之後 nursery 裡剩下的短命物件不多，不用 nursery 一樣快、記憶體的高峰不變或更低（Chrome 的 Oilpan 也沒有開 young generation）；gc 的分代留給其他的 heap 和測試。
- 配置的快路（`publish` 不借 `space`、快路 inline、`Site::index` 移出快路）和分代剩下的成本，照「GC 不再搬動」的方向另寫 RFC（「之後的可能」）：快路的形狀取決於還有沒有 young。

### 測試

每一段照 `docs/testing.md`：新的行為有測試、跑 mutation test，結果寫進 `progress.md`；優化盡量有計數的測試，守不住的記進 `docs/test-gaps.md`。量測照「設計概要」；Speedometer 和取樣只在使用者要求時跑（`docs/benchmarks.md`）。

### 分階段

在 branch `0085` 上做，每段開始前只決定那一段要決定的事。

| 段 | 內容 |
| --- | --- |
| 1 | 量測工具：Chrome 的取樣、兩邊的分類、ms 的比較表進 `bench.py` |
| 2 | DOM：找子 frame（2a）、`innerHTML` 的路（2b）、屬性的成員和字串參數（2c）；查找和 collection 移到第 4 段 |
| 3 | JS：給 V8 的設定 |
| 4 | binding 的固定成本、建節點、`dispatchEvent`、`element.style`，追 blink-perf 的頁（4a 到 4f） |
| 5 | GC：配置（在 4d 做，決定 15）；5a 資料跟著擁有者配在同一代（決定 17）；5b renderer 的 heap 不用 nursery（決定 18、19）；其餘另寫 RFC |

## 缺點

- Chrome 那一側是估計：Fedora 的 Chromium 沒有 Google 的 PGO，比 Chrome for Testing 慢 23%，各類的比例只是近似。
- 只在一台機器上量（Linux、AMD）：AMD 的取樣有 skid，linker 合併內容相同的函式，幾十條指令的差分不出在哪個函式。
- JS 不重編 V8，能動的有限：第 3 段找到的只有 Sparkplug 那一項，其他 JIT 的程式碼和 builtin 剩下 1.1 到 1.3 倍。
- 第 4 段追的是單一操作的頁，在 Speedometer 裡佔得不多（2026-11-06 第 1 段的驗收是 bindings 35 ms，比 Chrome 多 28.9 ms），對分數的幫助小。
- 這台機器上，DOM 和 GC 的 blink-perf 頁對程式碼的排列很敏感（同一份程式碼加一行沒有意義的程式就差到 9% 到 10%），小的優化用整頁的時間證明不了：判斷要看取樣的類別時間和指令數。

## 理由和其他做法

- **為什麼這四類**：使用者選的（2026-11-06）。文字、style、box layout 另外處理。
- **為什麼一份分階段的 RFC，不分成幾份**：範圍縮到四類之後不大，四類共用同一套量法和工具；分成幾份的流程成本比內容高。
- **為什麼量測工具排第一**：每一段都用它驗收；Chrome 那一側現在是暫時的腳本。
- **為什麼 GC 移到最後**：第一步讓 GC 那部分變少了，整頁的時間卻被程式碼的排列蓋過；使用者決定先做其他段，最後再回來看（決定 7）。不處理排列（函式排序清單、BOLT、PGO）：對這份來說太深。
- **為什麼 GC 剩下的另寫 RFC**：5a、5b 之後 renderer 的 heap 等於只有 old，但分代的成本還在程式碼的路上；要省到得拿掉或換掉搬動式的分代，這推翻 RFC 0087 的設計決定，照流程另寫一份（決定 19）。
- **為什麼 binding 排最後還要做**：取樣裡不大，但 blink-perf 的單一操作還差 2 到 14 倍，使用者要盡量追上。
- **為什麼不改成保守掃描 stack**：`docs/architecture.md` 的「和 Chromium 刻意不同的地方」寫了理由。

## 先例

- **Chromium**：cppgc 的 inline bump 配置（`MakeGarbageCollected`）；`NodeListsNodeData`、`TreeScope` 的 id 表、`TreeOrderedMap`、`CEReactionsScope`；V8 字串的 externalize 和 `AtomicString`；binding 和 DOM 同一個 binary、inline 的 C++。
- **Servo、Stylo**：Rust 的引擎用 atom 處理名稱和字串。

## 已決定的事項

1. 2026-11-06：換 DOM 的 RFC 記下、還沒做的效能條目移到這份（RFC 0089 的決定 184）；條目照現在的程式寫，當時的量測留在那份的 `progress.md` 和 `measurements.md`。
2. 2026-11-06，使用者要求照換 DOM 之後的數字整個重寫，先不重量，每個數字註明是哪一次量的。
3. 2026-11-06，重新取樣（使用者要求）：Chrome for Testing 的 Linux 版沒有符號，使用者選 Fedora 的 Chromium 156 加 debuginfo（不選：沿用 Windows 上的分類、自己編 Chromium 152、只分 JS 和其他）。156 比 152 慢 23%（Fedora 版沒有 Google 的 PGO），所以 Chrome 的各類用兩種估法交叉比對（使用者選的；`details.md` 的「Chrome 的估計」）。
4. 2026-11-06，使用者決定：
   1. 範圍只有 GC、bindings、JS、DOM 四類；文字、style、box layout 另外處理。
   2. 這份改成分階段直接做的 RFC，不再分出子 RFC（不選：維持路線圖、分出子 RFC）。
   3. RFC 0086 否決：不重編 V8，JS 那一段只比給 V8 的設定（不選：把 0086 的 V8 部分併進來）。
   4. 順序：量測工具、GC、DOM、JS、binding；binding 排最後但要做，追上差距大的 blink-perf 頁。
5. 2026-11-06，第 1 段（使用者同意）：
   1. Chrome 的取樣是 `bench.py speedometer --sample --browser chrome`（不另開指令）；先不取樣地量一次那個 Chromium 的時間，報告列出它和 Chrome for Testing 152 每個 suite 的比。
   2. Chromium 的執行檔預設是 Fedora 的 `/usr/lib64/chromium-browser/chromium-browser`，`--chrome-binary` 可以換；只做 Linux（macOS、Windows 的 Chrome 取樣不在這份）。
   3. 換成 ms 用上一次 `bench.py speedometer` 存下的兩邊的時間（`kun-runs.json`、`chrome-runs.json`）；分類和比較表在新的 `scripts/bench/speedometer/categories.py`，兩邊的樣本和時間都在時，`--sample` 跑完就寫出 `categories.md`。

6. 2026-11-06，第 2 段（使用者選甲）：取樣裡 GC 多的主要在配置：`Inner::allocate`（每次配置在 old 的物件用二分搜尋查 size class；整個慢路 inline 在同一個 frame 裡）和 `publish`（有 finalizer 的物件對頁的 bitmap 做一次 atomic 的 OR，之前借 `space`、找頁）。
   1. size class 在型別註冊時算一次（cppgc 在編譯時就知道 space）。
   2. 配置拆成 inline 的快路和不 inline 的慢路（cppgc 的 `AllocateObject` 和 `OutOfLineAllocate`）。
   3. `publish`：頁的 finalizable bitmap 用一般的讀和寫（只有 heap 的 thread 寫：交給 concurrent sweep 的頁在結果套用之前不配置），cursor 記住它那一頁的 bitmap。
   不選乙（照 cppgc 拿掉 finalizable bitmap，sweep 讀死物件的 header 找 finalizer）：比甲多省的只有一次寫入，要改 sweep 和照頁數 finalizer 的地方。做完重新取樣，再看 scavenge 和 write barrier。

7. 2026-11-07，使用者決定：第 2 段（GC）的第一步不 commit，GC 移到最後一段（第 5 段），先做 DOM、JS、binding；`bench.py` 的 `--against` 的錯單獨修好、commit。程式碼的排列不處理（函式排序清單、BOLT 都太深；PGO 是 RFC 0086 否決的）。
8. 2026-11-07，第 2 段（DOM，使用者同意）：分 2a（找子 frame 不再一直走 DOM）、2b（`innerHTML` 的路）、2c（binding 傳進來的字串換成名稱的編號）；`getElementsByTagName`、`getElementById` 這些 blink-perf 的項目移到第 4 段。判斷用取樣的類別時間、`perf stat` 和 `calls` 的指令數，整頁的時間只當參考。2a：renderer 每轉一圈 loop 問動畫要不要 frame 時，engine 照 frame 建立的順序看（`frames_as_made`），不走樹（順序在那裡不重要；Chrome 的 task 也不找每個 frame）；rendering update 開始時找一次文件（`begin_rendering_update`，HTML 的 `docs`），每一步共用。不選：每份文件數它的 iframe（甲：有 iframe 的文件照樣走）、照 DOM 的順序維護子 frame 的清單（乙，最像 Chrome 的 `FrameTree`：要在插入時找位置）、跨 update 的快取（丙：要在每個改變 frame 的地方作廢，漏一處就是畫不出新的 iframe；量了還看得到再做，「之後的可能」）。
9. 2026-11-07，2b（使用者同意）：TodoMVC-jQuery 的 `innerHTML` 每個 iteration kun 74.7 ms、Chrome 37.5；換掉子節點兩邊差不多（13.2、12.2），差在解析和建節點（61.4、25.2）。kun 的 fast path 建節點走一般的 DOM API（每次用字串查名稱、連 HTML 的命名空間字串一起，設屬性前找同名的，input 先建成 text 再照 `type` 改），Chrome 的 `HTMLFastPathParser` 照編譯時的名稱建元素，屬性收齊之後 `ParserSetAttributes` 一次設，input 在屬性之前照 `type` 定型別（`InitializeTypeInParsing`）。做：元素帶著屬性建（`create_html_element_with_attributes`：名稱用編號，屬性不先找同名的、清單一次配到大小，input 照 `type` 建，每個屬性的 change steps 照樣跑，Blink 也是）；屬性的值和文字只解碼一次（scan 檢查時解的留下）。原本提的另外兩項細看之後不做：`form_element_made` 的 3.8 ms 幾乎都是每個 input 配的三個物件（element data、表單關聯、型別），不是名稱的判斷，改成查表只省 0.4 ms；插入新的 fragment 已經沒有可以省的檢查，`link` 的時間在 GC 的 write barrier。預估從 17 到 23 ms 改成約 8 ms。不在這段做：屬性不再一個一個配 GC 物件（照 Blink 放在元素的陣列裡、`Attr` 要的時候才建：動到所有屬性的 API，和 `set-attribute`、`get-attribute` 那幾頁一起放到第 4 段）；文字節點和 input 的配置（第 5 段 GC 的配置路徑）；`scan` 本身（這段做完再看它的分布）。
10. 2026-11-07，2c（使用者同意）：先照同樣的方法取樣 Chromium 156 和 kun 的 blink-perf `bindings/get-attribute`、`set-attribute`（IBS）。Chrome 每次也把字串換成 `AtomicString`：`"id"`、`"foo"` 這種短的 internalized 字面值 V8 不讓 externalize，Blink 每次複製、查 `AtomicStringTable`，字串那部分 Chrome 不比 kun 快；名稱的查找（`find_fixed`）只有 1 到 3 ns。差在：`setAttribute` 的值沒變也更新文件的 id 表（約 18 ns；Blink 的 `Element::AttributeChanged` 比新舊的 `AtomicString`，一樣就不碰 `TreeScope` 的表）、binding 的固定成本（3 到 7 ns）、`getAttribute` 判斷要不要轉小寫（kun 6.5 ns，Blink 的 `WeakLowercaseIfNecessary` 0.1）、`ArgString::text` 再借一次字串（5 ns）。做：id 的值沒變時不動 id 表；轉小寫的判斷照旗標、小寫的名稱直接用；字串參數的內容借一次。不做：原本的 V8 字串到名稱編號的快取（甲、乙：省不到東西）；binding 的固定成本留在第 4 段。
11. 2026-11-07，第 3 段（使用者同意）：kun 和 Chrome for Testing 152（同一版 V8 15.2）給 V8 的 flag 逐項比，renderer 差 14 項。三個 JS 多的 suite 上換成 Chrome 的 flag 量不出差別（起伏 5% 之內）；同一版的 Chrome 取樣之後（它的 builtin 沒有符號，照 V8 的 perf map 換回名字）看得出來：Chrome 的 Sparkplug 照 feedback 呼叫特化的 IC builtin、把比較和算術 inline（`--sparkplug-plus`），kun 全部走通用的（`LoadICGenericBaseline`、`*_Generic_Baseline`），Sparkplug 的 builtin 49.1 對 29.1 ms（20 個 suite 合計），其他 JIT 的程式碼和 builtin 只慢 1.1 到 1.3 倍（build 的差別）。做：照 Blink 的 `gin::V8Initializer` 照 feature 設 flag，把 Chrome 的 renderer 打開的 JIT 和 IC 的四項設成 kun 的預設（`--sparkplug-plus`、`--homomorphic-ic`、`--max-valid-polymorphic-map-count=10`、`--no-additive-safe-int-feedback`），放在呼叫者的 flag 和 `MACHOPO_V8_FLAGS` 之前，可以蓋掉；用取樣驗收（計時看不出 2%）。不做：V8 的 heap 那六項（Chrome 在記憶體和速度之間的取捨，量不出差別）、`verify-bytecode-light`（安全檢查，只會變慢）、`single-threaded-gc-in-background`（只在 efficiency mode 有作用，kun 沒有背景分頁；原本的理由在換 DOM 之後不成立了，註解改成現在的理由）。
12. 2026-11-07，第 4 段（使用者同意）：分 4a（`document`、`window` 的 cached accessor）、4b（`getElementsByTagName` 的 collection）、4c（getter 的固定成本，逐層拿掉量）、4d（建節點和 wrapper）、4e（`dispatchEvent` 經過 shadow tree）、4f（`element.style`）；`gc-*` 拿掉 `document` 之後剩下的算第 5 段，`PseudoClassSelectors` 是 style，不在這份。4a 選甲：照 Blink 的 `[CachedAccessor]`（V8 的 `FunctionTemplate::NewWithCache`，getter 的結果放在 global 的 private property，IC 和 TurboFan 當成 data property 讀）：rusty_v8 的 fork bind `NewWithCache`（改 `binding.cc`，重編 artifact），window 的 document 設定、換掉時更新那個 property（Blink 的 `LocalWindowProxy::UpdateDocumentProperty`）。原因：全域的 `document` 每讀一次多 65 ns（global object 是 dictionary mode，V8 的 IC 對它上面的 API accessor 只給 slow stub），blink-perf 有六頁的大頭在這裡。不選：做成 data property（乙：WebIDL 的 `[LegacyUnforgeable]` attribute 是 accessor，WPT 會失敗）、不做（丙）。
13. 2026-11-07，4c（使用者同意先做甲，之後再考慮乙）：`nodeType` 一層一層拿掉量（`calls`）：V8 的地板加 trampoline 50 條指令、11 cycle，完整的 167、33；kun 自己的約 117 條、22 cycle 裡，`node_type`、回傳和 `finish` 17 條、3 cycle，在操作裡 unwrap（tag、`from_engine`）約 27 條、3 cycle，其餘約 73 條、16 cycle 是開操作和每次呼叫的結構（`drop_glue::<Call>` 佔 op 的 8%，建 `V8Front`、`Call` 時的 `memcpy`/`memset` 13%）。Chrome 整個 `nodeType` 約 18 cycle：Blink 產生的 getter 只有 receiver 和 `ReturnValue`，`ToWrappableUnsafe` 幾條 load，不再比型別、不檢查 cage，也沒有操作（cppgc 掃 stack）。甲：getter 走一條輕的路，不建、不 drop 完整的 `Call`。乙（之後決定）：unwrap 照 `ToWrappableUnsafe` 跳過 signature 已經檢查過的型別，cage 的檢查改用型別保證（動到 `bindings` 的 `unsafe` 論證）。不做：`read_weak` 的 barrier 和開操作本身（kun 不掃 stack 的 GC 換來的，要重新論證 concurrent marking，「之後的可能」）。
14. 2026-11-07，4c 的乙先不做（使用者決定，先做 4d）：預估每次 1 到 2 cycle，要改 `bindings` 的 `unsafe` 論證，而且「signature 檢查過」要對每一條呼叫路徑成立（`CrossOriginMember` 沒有 signature）。記進「之後的可能」。
15. 2026-11-07，4d（使用者同意先做乙，再做甲）：`calls` 的 `createElement` 的 IBS 樣本照函式分類（比例乘上總指令數 2264，估計）：把物件接上 wrapper 約 400 條、配置 DOM 物件約 390（`Inner::allocate`、`Cx::node`、`Node::__init`、`publish`、`make_element`）、binding 的 glue 約 340、V8 的 scavenge 丟掉 wrapper 約 215、clone boilerplate 約 210、名稱約 180、比 creation context 約 90。和 Chromium 156 的 callback 裡的比例比，V8 那一側差不多，kun 多的是配置（Blink 的 `HTMLDivConstructor` 加 `Node::Node` 約 6%）和 glue（約 6%）。乙：把第 5 段 GC 暫存的配置改動（決定 6、7）提前到這裡，再看 `Cx::node`、`Node::__init`、`make_element`；照決定 8 用指令數和取樣判斷。甲（之後）：glue 逐層拆。丙（`wrap` 的檢查改在 Rust 裡 inline 讀）之後再決定。
16. 2026-11-07，第 5 段（使用者同意）：第 4 段之後的取樣裡 GC 那一類 kun 52.7、Chrome 17.8 ms（20 個 suite 合計）：DOM 物件的 scavenge 約 15.4（`Scavenging::evacuate` 9.5，Chrome 0：cppgc 沒有新生代），配置約 13.9（`allocate_in`、`publish`、`Site::index`；Chrome 約 1.7），write barrier 約 4.4（`SlotSet::insert` 3.2）。使用者注意到節點本身在 old（有 JS 邊的型別在 engine 驅動的 heap 一律配 old），它的資料（`CharacterData`、`Attribute`、`ElementData` 和其他 rare 的資料）卻先配在 nursery：每次掛上是一筆 old 到 young 的記錄，scavenge 要處理、而且一定要搬。第一步：先數這樣的物件有多少、佔 scavenge 搬的多少，確定是大頭再讓資料跟著節點配在同一代（Chrome 的節點和資料都在 old）；之後做配置的快路（`publish` 不借 `space`、快路 inline、`Site::index` 移出快路）。
17. 2026-11-07，5a（使用者同意甲）：數 scavenge 搬的物件（暫時的探針，照型別，每個 suite 10 個 iteration）：TodoMVC-jQuery 28 次 scavenge 搬了 246 MB，最多的是 `Attribute` 58 MB、`CharacterData` 56、`ElementData` 45、`MemberArray` 27、`HeapVector` 20、`InputType` 20、`FormAssociation` 16，約三分之二搬了兩次（留在 nursery 一次、升上去一次）；React-Complex-DOM、NewsSite-Next 各 14 MB，ES5 21 MB。甲：資料跟著擁有者配在同一代（`Mutation::alloc_beside`、`HeapVector::new_beside`；vector 長大時新的 store 跟著 vector 所在的那一代）。擁有者在 old 就直接配 old，和有 JS 邊的型別、pretenure 走同一條路，寫邊照樣經過 barrier。節點的文字、屬性和屬性的清單、element data、rare data、classes、`childNodes` 的 list、input 的型別、表單關聯、`Attr` 留著的清單和它的資料、doctype、template 和 document 的資料都這樣配；`classList` 這類有 wrapper 的本來就在 old，不用改。不選：讓 site 的 pretenuring 自己學（同一個 site 的物件有的掛在 old 的節點上、有的短命，存活的比例到不了門檻）；全部配 old（決定 18 量了）。
18. 2026-11-07，使用者問分代在 kun 還有什麼用（看起來只多了成本）。對照（暫時把 nursery 設成 0，所有物件直接配 old，和甲之後比，各 10 個 iteration、各一次）：四個 suite 的時間都沒有變差（例如 jQuery 的 CompletingAllItems 54.3 → 54.6 ms、NewsSite-Next 的 NavigateToUS 21.1 到 22.8 → 17.7），full GC 從 jQuery 1 次變 8 次、ES5 2 次，其他 1 次，old 在 full GC 前最高到 141 MB（之後掉回 30 MB）。Chrome 的 Oilpan 沒有開 young generation（我們給 V8 的 `--no-cppgc-young-generation`），DOM 物件都配在 old，write barrier 只在 marking 時做事。使用者同意：甲先 commit；下一段（5b）只讓 renderer 的（engine 驅動的）heap 不用 nursery，gc 的分代留給其他的 heap 和測試，先量甲和不用 nursery 的記憶體高峰，Speedometer 經使用者同意再跑；成立的話，把 young generation 整個拿掉（scavenger、slot set、AGED、site 的 pretenuring、分代的 barrier 改成只在 marking 時記錄）列進「之後的可能」，另外規劃。
19. 2026-11-07，5b（使用者同意照建議做：甲、B 各量一次）：記憶體（暫時的探針，每次 GC 印 heap 的大小和 process 的 VmHWM，四個 suite 各 10 個 iteration）：不用 nursery 時 full GC 的次數和甲一樣（jQuery 8、ES5 2、其他 1），heap 的 used 高峰一樣（jQuery 141.8 MB：是 full GC 之間累積的，和分代無關），committed 少 2 到 12 MB（nursery 的頁）。Speedometer（`bench.py speedometer --against HEAD`，甲對不用 nursery，交錯各 3 次）：幾何平均 −0.3%，VmHWM −8.6% 到 +3.0%；取樣的 GC 那一類 甲 36.4、不用 nursery 38.5 ms（第 4 段之後 52.7），沒有再降：配置先試 nursery、barrier 檢查目標是不是 young 這些都還在路上，要省到得把分代整個拿掉或換掉。做：renderer 的 heap（`dom_js` 的 `State`）`nursery_bytes: 0`，gc 的分代留給其他的 heap 和測試；甲留著（沒有 nursery 時沒有作用，不是 driven 的 heap 還用得到）。之後的方向（GC 不再搬動）另寫 RFC，見「之後的可能」；第 5 段原本剩下的配置快路併進去，因為快路的形狀取決於還有沒有 young。

## 待決定

無。比值的目標沒有定（「動機」的目標）；GC 剩下的在「之後的可能」，另寫 RFC。

## 之後的可能

- GC 不再搬動（另寫 RFC，決定 18、19）：renderer 的 heap 不用 nursery 之後，分代的成本（配置先試 nursery、barrier 檢查 young、`Site` 的統計、scavenge 和它的論證）還在程式裡。兩個方向：只有 old（Oilpan 的做法：barrier 平常只看 marking 有沒有在跑，配置是 size class 的 bump，可以 inline）；不搬動的分代（sticky mark bits：上次 GC 之後配的、還沒標的是 young，minor GC 從 root 和 remembered set 標、就地 sweep；JavaScriptCore 的 Riptide、cppgc 的 young generation），有 wrapper 的物件也能是 young，wasm 的文件留著分代的好處。先量：Speedometer 上有 wrapper 的 DOM 物件，有多少在 V8 的 scavenge 就丟掉 wrapper（多的話不搬動的分代對 JS 文件也有用）；wasm 的 guest 接上之後，用真的頁面量沒有分代時的 full GC 和記憶體。第 5 段剩下的配置快路（`publish` 不借 `space`、快路 inline、`Site::index` 移出快路）照選的方向做。
- `createElement` 剩下的（第 4 段 4d 的甲量完，`progress.md`）：`wrap` 照 inline 的 unwrap 在 Rust 裡做（省 `kun::Wrap` 的 C++ 包裝和 `IsApiWrapper`，約 40 條：配 `CppHeapPointerTable` 的一格要留在 V8 裡，要改 fork 和 `bindings` 的 `unsafe` 論證）；元素名稱的檢查只在名稱是特定 HTML 元素的 interface 時跳過（約 15 條，`names::find` 找得到的不一定合法）。其餘多半是 V8 建、接上 wrapper 的機制，Chrome 也一樣。
- `element.style` 剩下的（第 4 段 4f，`progress.md`）：屬性的 id 從 `css_property_for_attribute` 一路往下傳，不再照名稱查三、四次（記住查找結果的表試過，+2.6%，還原了）；宣告集合的增刪（`memmove`：宣告的結構大，可以 box 起來）；回傳字串的快取（rusty_v8 的 `FinalizerMap` 用 std 的 SipHash）。
- unwrap 照 Blink 的 `ToWrappableUnsafe`（第 4 段 4c 的乙，決定 14）：signature 檢查過的 receiver 不再比型別的 tag，cage 的檢查改用型別保證（`NoGc` 和操作的 `Mutation` 同一個 `'gc`）；每次約 1 到 2 cycle，要改 `crates/bindings/soundness.md` 的論證。getter 剩下的成本還有建 `V8Front` 和 `Call` 的約 25 個 store、開操作、`read_weak` 的 barrier（GC 不掃 stack 換來的）。
- 文字、style、box layout、paint 的差距：另寫 RFC（`details.md` 的「換 DOM 之後的分析（2026-11-06）」有數字和最多的函式）。
- script 插入的 script 少開操作：每跑一個開一個操作（幾 ns）；第一個要處理的 script 可以在插入的操作裡 prepare。量到有頁面插入大量小的 inline script 時再做。
- wasm 的 guest（renderer 還沒接上 wasm 的文件）：`nodeType` 從 wasm 呼叫每次 1656 條指令（JS 約 211），幾乎都在 wasmtime 的 Component Model 的 host 呼叫；每次呼叫 36 到 97 次 store forwarding 的衝突。
- 重編 V8（official build、PGO）：RFC 0086 否決時的設定，之後要再看時重開。
- 找子 frame 的快取（第 2 段 2a 不做的丙）：rendering update 還是每個 frame 走一次 frame tree 的文件找 iframe，有 iframe 的頁面上滑鼠的每個事件和每次重找指標底下的東西也走（`dom_api` 的 `input.rs` 的 `forget_gone_frames`）。跨 update 留著找到的文件，在 iframe 連上、拿掉、移進 shadow tree、子文件換掉時作廢（或照 Chrome 維護一棵 frame tree）；取樣裡還看得到、或有很多 iframe 的大頁面慢時再做。
