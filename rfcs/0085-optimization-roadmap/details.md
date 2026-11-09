# RFC 0085 的細節：和 Chrome 的差距在哪

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和看法以那邊為準，這裡不重複。

## 換 DOM 之後的分析（2026-11-06）

DOM 換成照 `crates/gc` 設計的之後，兩邊照 2026-10-24 的分類（「換 DOM 之前的分析」的「量法」）重新取樣。Chrome 那一側用的不是 Chrome for Testing 152（Linux 版沒有符號），是 Fedora 的 Chromium 156，所以 Chrome 的數字是估計，見「Chrome 的估計」。

### 量法

- **環境**：Fedora 44（Linux 7.2），AMD Ryzen AI 9 HX 370，接電源、擋住休眠，兩個瀏覽器綁在快的核心（0–3、12–15）。kun `f21c5cac`（程式和 `331fe288` 一樣：換 DOM 的收尾和之後的優化都在），`perf` profile。
- **取樣**：兩邊都是 `perf record -e cycles:u -e instructions:u -c 1000000 --call-graph fp -k monotonic`，V8 加 `--perf-basic-prof --interpreted-frames-native-stack`；20 個 suite 各 5 個 iteration，只留 runner 計時的區間（User Timing 的 mark）裡 renderer main thread 的樣本。kun 用 `bench.py speedometer --sample`。Chrome 用暫時的腳本（不在 repo 裡）呼叫 `bench.py` 的函式：
  - Chromium 加 `--no-sandbox`：renderer 的 sandbox 擋住 V8 寫 `/tmp/perf-<pid>.map`；
  - trace 只錄 `blink.user_timing`：用 `bench.py` 預設的那些 category（`blink`、`v8`、`cc`、`devtools.timeline`……）時，Chrome 的計時區間是平常的兩倍；
  - Linux 上 Chromium 不給 process 的 main thread 命名（命名會改掉 process 的名字），`perf` 看到的 renderer main thread 叫 `chromium-browse`，所以照 trace 的 thread descriptor 裡 `CrRendererMain` 的 tid 挑樣本；
  - Chrome 的 trace 的時間是 `base::TimeTicks`，Linux 上就是 `CLOCK_MONOTONIC`，和 `perf -k monotonic` 同一個時鐘（kun 的 trace 是 `CLOCK_BOOTTIME`）。
- **分類**：照「換 DOM 之前的分析」的「量法」的規則（最內層說得出種類的 frame；配置、`memcpy` 和標準函式庫的容器算給呼叫它的；V8 自己的 frame 往外看；GC 含 `gc` 和 cppgc 的配置）。取樣本身的成本另外拿掉：V8 寫 perf map（`CodeEventLogger`）、`--trace-gc-nvp` 的紀錄、perfetto。Blink 照類別名稱分，有少數類別沒分到，留在「其他」。
- **換成 ms**：取樣讓兩邊各慢不同的幅度（kun 約 6%，Chrome 一到兩成），所以不直接比 cycle：每個 suite 各類佔樣本的比例，乘上那個 suite 沒有取樣時計時的中位數。kun 的是換 DOM 的收尾之後重量的那一次（`rfcs/0089-dom-replacement/measurements.md`，Editor-TipTap 用 `focus()` 修好之後的 96.3 ms），Chrome 的是同一天的 Chrome for Testing 152（`bench.py speedometer` 的量法，20 個 suite 合計 670.8 ms）。

### Chrome 的估計

Fedora 的 Chromium 156 比 Chrome for Testing 152 慢：同一天各跑一次 20 個 suite，每個 suite 慢 12%（ES5）到 47%（NewsSite-Next），幾何平均 23%；取樣要的 flag（`--no-sandbox` 和 V8 的 perf map）只多 1.4%。差的應該是 Google 的 official build 的 PGO：Fedora 版的 `.text` 285 MB，Chrome for Testing 225 MB（PGO 把冷的程式碼照大小編）。每個 suite 慢的幅度不同，所以 156 各類的比例只是近似。兩種估法：

- **A**：Chromium 156 每個 suite 各類的比例，乘上 Chrome for Testing 152 那個 suite 的時間。
- **B**：2026-10-24 在 Windows 上 Chrome 155 正式版（有 PGO）20 個 suite 合計各類的比例（下面的「20 個 suite 的分類」），乘上這台 Chrome for Testing 152 的合計 671 ms。只有合計，沒有每個 suite 的。

兩種估法在大多數類別只差幾 ms。文字差最多（Windows 和 Linux 的字型系統不同）；bindings 和 V8 API 兩邊的分法不同，合起來看。

### 各類

每個 iteration 的 ms，20 個 suite 合計；最後一欄是只有這一類追平 Chrome（估計 A）時，20 個 suite 比值的幾何平均（現在 1.523，計時的 1.50）：

| 種類 | kun | Chrome（A） | 差（A） | Chrome（B） | 差（B） | 只有這一類追平時 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 文字 | 175 | 41 | +134 | 91 | +84 | 1.354 |
| JS | 349 | 248 | +101 | 252 | +97 | 1.381 |
| style | 136 | 59 | +77 | 66 | +70 | 1.416 |
| DOM | 116 | 73 | +43 | 77 | +39 | 1.471 |
| GC | 58 | 19 | +39 | 22 | +36 | 1.473 |
| box layout | 96 | 59 | +37 | 56 | +40 | 1.485 |
| paint | 53 | 37 | +16 | 39 | +14 | 1.488 |
| bindings | 18 | 6 | +12 | 28 | −10 | 1.509 |
| HTML parse | 22 | 15 | +7 | 16 | +6 | 1.519 |
| V8 API | 17 | 18 | −1 | 10 | +7 | 1.528 |
| canvas | 3 | 35 | −32 | 6 | −3 | 1.556 |
| 其他（`renderer`、`gfx`；Chrome 是沒分到的 Blink） | 36 | 62 | −26 | 8 | +28 | 1.572 |
| 合計 | 1078 | 671 | +407 | 671 | +407 | 1.523 |

- 幾類一起追平（估計 A）：GC、bindings、V8 API、DOM 1.407，再加 HTML parse 1.400，再加 JS 1.261。
- canvas：Chrome headless 在 Linux 上的 canvas 2D 在 main thread 上 raster，kun 的在 canvas raster thread 上（RFC 0081），不是差距。

### 每個 suite

估計 A，每個 iteration 的 ms：

| suite | kun | Chrome（估計 A） | 倍數 | 差最多的三類（ms） |
| --- | ---: | ---: | ---: | --- |
| NewsSite-Next | 134 | 66 | 2.03 | style +20、box layout +16、文字 +16 |
| TodoMVC-jQuery | 140 | 76 | 1.86 | GC +24、DOM +22、JS +8 |
| Editor-TipTap | 96 | 44 | 2.17 | 文字 +49、JS +2、paint +1 |
| Perf-Dashboard | 87 | 46 | 1.89 | 文字 +19、JS +14、style +5 |
| React-Stockcharts-SVG | 95 | 56 | 1.68 | JS +13、文字 +11、style +8 |
| NewsSite-Nuxt | 80 | 51 | 1.57 | box layout +11、style +10、文字 +8 |
| TodoMVC-WebComponents | 34 | 13 | 2.70 | 文字 +8、style +5、DOM +3 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 41 | 25 | 1.62 | DOM +5、GC +4、JS +2 |
| Charts-observable-plot | 49 | 36 | 1.35 | JS +6、style +6、文字 +3 |
| TodoMVC-Angular-Complex-DOM | 35 | 25 | 1.41 | JS +6、paint +2、DOM +2 |
| TodoMVC-React-Complex-DOM | 34 | 26 | 1.32 | JS +5、DOM +2、paint +1 |
| TodoMVC-Lit-Complex-DOM | 22 | 15 | 1.48 | style +4、文字 +1、paint +1 |
| TodoMVC-JavaScript-ES5 | 36 | 29 | 1.25 | GC +4、JS +1、文字 +1 |
| TodoMVC-React-Redux | 31 | 25 | 1.23 | JS +5、文字 +1、style +0 |
| TodoMVC-Backbone | 25 | 20 | 1.29 | JS +4、文字 +1、style +1 |
| TodoMVC-Svelte-Complex-DOM | 16 | 10 | 1.56 | style +1、文字 +1、paint +1 |
| TodoMVC-Preact-Complex-DOM | 17 | 12 | 1.44 | paint +1、文字 +1、style +1 |
| Editor-CodeMirror | 22 | 18 | 1.22 | JS +3、文字 +1、style +1 |
| TodoMVC-Vue | 22 | 18 | 1.21 | 文字 +2、DOM +1、paint +1 |
| Charts-chartjs | 61 | 60 | 1.01 | JS +10、style +7、文字 +4 |

### 每一類最多的函式

20 個 suite 合計，最內層說得出種類的那一層（取樣的 cycle，相對的大小）：

- **文字**：kun 是 parley 的 `shape_text`、`build_into_layout`，`InlineContent::new`，fontique 每次解析字型的名稱（`NameKey::from_str`），`font_families::Stack::push_with_metric_compatible`，`Analyzer::analyze`，斷行時複製整個 `LineState`，`select_font`；Chrome 是 HarfBuzz 的 shaping（GPOS、`hb_ot_shape_internal`）、`ShapeResultView`、ICU 的斷行。kun 多的有一大塊在選字型和退回，不在 shaping 本身。
- **JS**：兩邊最多的是同一批 JS 函式和 IC 的 builtin，各慢 1.2 到 1.5 倍（和 10-24 的結論一樣）。
- **style**：selector 比對兩邊差不多（kun 的 `matches_complex_selector_internal`，Chrome 的 `ElementRuleCollector`、`SelectorChecker`）；kun 多的在 cascade（`style::Walk::cascade` 最多），和 `MatchedPropertiesCache::add`、`ComputedStyle::rare_mut`、`ComputedStyle::initial`。
- **DOM**：kun 是名稱的查找（`names::find_fixed`）、`link`、`attribute_value_by_id`、`child_content_documents`、`class_names`、`intern_value`、`form_subtree_inserted`；Chrome 是 `Element::RemovedFrom`、`InsertedInto`、`EventDispatcher::Dispatch`。
- **GC**：kun 的 `gc` 的配置（`Inner::allocate`、`publish`）比 scavenge（`Scavenging::evacuate`）還多，還有 write barrier 的 `SlotSet::insert`、V8 的 scavenge 處理 traced handle；Chrome 的配置很少出現，多的是 V8 的 scavenge。
- **box layout**：kun 是 taffy 的 flexbox、`extend_overflow`、`BoxTree::lay_out_box`、`release`；Chrome 是 `BlockLayoutAlgorithm::Layout`、`FlexLayoutAlgorithm`、`PhysicalBoxFragment::Create`。

### 限制

- Chrome 的數字是估計（上面的「Chrome 的估計」）。
- 只算 user space（`cycles:u`）：kernel 的 page fault 不在裡面（10-24 在 Windows 上兩邊都算了 kernel）。
- Chrome 的「其他」還有沒分到的 Blink 類別（`content::RendererMain`、一些匿名 namespace 的函式）。

## 換 DOM 之前的分析（2026-10-24）

下面到「參考資料」之前，是 2026-10-24 在 Windows 11 上、舊的 DOM（cppgc 上的 `dom`）量的分析，和它當時的四個來源。DOM 換成照 `crates/gc` 設計的之後，來源 1 的 cppgc 的 FFI、`Persistent`、root 和節點 id 表，來源 2 的節點和屬性的表示，來源 3 的 collection 的快取都已經不是這樣；V8 那一側、編譯設定的觀察還成立。下面的「換 DOM 之後的分析（2026-11-06）」是重新取樣的結果。

### 當時的摘要和四個來源


Speedometer 3.1 的 20 個 suite，kun 的分數是 Chrome 的 2.02 倍（2026-10-24 的比較表）。這份 RFC 第一次用**同樣的方法量兩邊**（samply 取樣 renderer 的 main thread，Chrome 從 Chromium 的 symbol server 拿符號，兩邊用同一套分類），結論是：**差距不在某個演算法或某幾個熱點，而是一層每個操作都在付的系統性成本**。V8 以外的每一類（DOM、style、layout、文字、parse、paint、GC、bindings）kun 都是 Chrome 的 1.2 到 6.8 倍，沒有一類例外，也沒有一個函式超過 35 ms；該有的快取和增量 kun 都已經有。

這層成本對照程式碼之後分成四個來源：

1. **Rust 的 DOM 和 C++ 的 V8、cppgc 之間的 FFI 邊界，加上 GC 不掃 stack 的設計**：每寫一條邊、每建一個 handle 都是一次不能 inline 的呼叫，還要 root、`Persistent` 和節點 id 表，這些 Blink 都沒有。
2. **資料的表示：每個東西各自配置、要用就複製**：節點、屬性、class、style、行內內容各自配置，Node 建一次搬三次；配置和複製的時間是 Chrome 的四五倍，而且分散在每一類、每個 suite。
3. **快取失效的粒度太粗**：例如 live collection 用全域的樹版本失效，任何改動都讓每個 collection 重算。
4. **編譯設定**：kun 的 Rust 沒有 PGO，V8 不是 official build（沒有 PGO 和 ThinLTO），allocator 把記憶體還給系統太快。

這份 RFC 不實作任何東西：它定出接下來幾份優化 RFC 的方向、順序和目標，之後的優化 RFC 從這裡分出去。2026-10-24 的看法：先做編譯設定（便宜、作用在所有 suite，另外一份 RFC），同時開一份「資料的表示和配置」的 RFC 當主線，FFI 邊界先做可行性驗證；目標是下一輪把比值從 2.0 降到 1.75 左右，中期 1.4 到 1.5；**1.0 以目前的架構做不到，不當目標**。

#### 當時的動機

當時的**現況**（量法和全部的表在 `details.md` 的「量測」）：2026-10-24，Windows 11，i9-13900K；kun `ce849649`（`perf` profile：ThinLTO，沒有 PGO），Chrome 155.0.8059.85（Chrome for Testing 152 的 PDB 不在 Chromium 的 symbol server 上；兩個版本在同樣的取樣下合計差 0.3%）。兩邊各跑 20 個預設的 suite、各 5 個 iteration，只算 runner 計時的區間裡 renderer main thread 的 CPU 時間，每個樣本照最內層說得出種類的 frame 分類，配置算給呼叫它的。每個 iteration 的 ms，20 個 suite 合計：

| 種類 | kun | Chrome | 倍數 | 只有這一類追平時的幾何平均（今天 1.815） |
| --- | ---: | ---: | ---: | ---: |
| JS（V8 本身） | 312 | 232 | 1.3 | 1.67 |
| DOM | 193 | 71 | 2.7 | 1.62 |
| style | 156 | 61 | 2.6 | 1.66 |
| 文字 | 100 | 84 | 1.2 | 1.77 |
| box layout | 87 | 52 | 1.7 | 1.76 |
| V8 API | 63 | 9 | 6.8 | 1.72 |
| HTML parse | 47 | 15 | 3.2 | 1.79 |
| paint | 46 | 36 | 1.3 | 1.78 |
| GC | 44 | 20 | 2.2 | 1.79 |
| bindings | 43 | 26 | 1.7 | 1.78 |
| 其他（`renderer`、`gfx`） | 33 | 7 | 4.7 | 1.76 |
| canvas 的 raster | 5 | 5 | 0.9 | |
| 合計 | 1130 | 618 | 1.83 | |

main thread 的 CPU 時間的比值，20 個 suite 的幾何平均是 1.815；比較表的分數是 2.02，差在 Chrome 被取樣時慢約 11%（kun 不受影響），所以表裡的倍數都低估約一成。

**問題**：

- **沒有熱點**。kun 最大的單一函式是 `style::cascade` 34.6 ms、`create_node` 21.7 ms，其他都在 20 ms 以下；Chrome 每一類最多的函式都在 5 ms 上下。每一類都慢一兩倍、每一類裡又平均分散，表示有一層每個操作都付的固定成本。到 RFC 0083 為止的優化 RFC 大多是修某個 suite 的某個熱點（canvas 的 raster、TipTap 的文字、innerHTML 的 parse），能修的熱點越來越少，每一份動到分數的幅度也越來越小：分數是 20 個 suite 比值的幾何平均，一個 suite 快 X%，分數只動 X% 的 1/20。
- **JS 以外的差距全部補平，分數也只到約 1.2**（`details.md` 的「情境的估計」）；JS 那 1.3 倍是 V8 的 build 設定（沒有 PGO、ThinLTO）和版本，不在 kun 的程式裡。
- **跨越每一類的成本兩邊差很多**（最內層就是這類函式的時間，差最多的 8 個 suite，kun / Chrome）：配置 63 / 15、`memcpy` 這類 40 / 8、kernel 的 page fault 25 / 18、rusty_v8 的 FFI shim 11 / 0、hash table 10 / 0。這些就是那層固定成本的樣子。

**目標**：

- 定出接下來的優化 RFC 的方向和順序，每一份針對「每個 suite 都付的成本」而不是某個 suite 的熱點，並且能用同一套量法驗收。
- 比值的目標（CPU 時間的幾何平均，對應分數再乘約 1.11）：第一輪（編譯設定、配置和複製的一半、FFI 的一半）到約 1.6（分數約 1.75）；中期把 DOM、style 的每個元素成本做完整，V8 以外每一類到 Chrome 的 1.3 倍以內，約 1.3（分數約 1.45）。
- 把兩邊同樣方法的取樣變成固定的量法（`details.md` 的「量法」），之後每一份優化 RFC 的驗收都用它。

**要解決的 `docs/limitations.md` 的限制**：無（這份不改程式）。

**不在範圍內**：

- V8 本身的效率（JIT 的程式碼、IC、runtime）：只有它的 build 設定在範圍內（第 1 個方向，另外一份 RFC）。
- main thread 以外的時間（compositor、raster、Viz）：這次的量法只看 main thread 的 CPU；分數和 CPU 的比值差的那一成裡有多少在這裡，之後再量。
- 新的功能和 Speedometer 還缺的 API（RFC 0084）。
- 把 DOM 改回 C++、或改成保守掃描 stack：見 `README.md` 當時的「理由和其他做法」。

#### 當時的設計概要

給之後規劃優化工作的人看：差距的四個來源、每個的證據、Chromium 怎麼做、kun 現在怎麼做、能怎麼改、改了大概值多少。數字是每個 iteration 的 ms、20 個 suite 合計，除非另外註明。

#### 來源 1：FFI 邊界和不掃 stack 的 GC

- **證據**：V8 API 63 對 9（6.8 倍）；GC 44 裡 kun 自己的機制（`BasicPersistent::Assign` 8.1、`init_rust_obj` 5.5、`Root::new` 和 `try_new` 7.6）合計 21，Chrome 的 GC 是 Scavenger 和 cppgc 的配置本身；rusty_v8 的 shim 函式本身 11（`v8__Object__Unwrap`、`cppgc__Member__Assign`、`v8__Value__NumberValue`、`v8__TracedReference__Get`……），每個都是一次不能 inline 的呼叫。
- **Chromium**：Blink、Oilpan、V8 編在同一個 binary 裡，ThinLTO 加 PGO，`Member` 的寫入、write barrier、handle 都 inline；Oilpan 保守地掃描 stack，stack 上放裸指標就行，不 root。
- **kun**：`Edge::set` 每次都呼叫 `cppgc__Member__Assign`，在 C++ 那邊判斷 write barrier（`crates/dom/src/gc.rs`、rusty_v8 fork 的 `src/binding.cc`）；不掃 stack，所以操作之外的指標要 root（`Persistent`），新節點要進 id 表（RFC 0060、0072 已經把一次操作裡的讀取改成不 root）。
- **能怎麼改**：
  - 甲、跨語言的 LTO：rustc 和編 V8 的 clang 用同一版 LLVM，V8 的靜態函式庫以 bitcode 連結，shim 和 write barrier 就能 inline。要改 build 系統和 artifact 的格式，兩邊的 LLVM 版本要一直對齊（現在 rustc 1.99 是 LLVM 23.1，Chromium 的 clang 是 23.0 的 git 版）。
  - 乙、把最熱的路徑搬進 Rust：write barrier 的快速路徑（讀 cppgc 的 marking 旗標，沒在 marking 就直接寫）、handle 的建立。綁死 V8 內部的結構，每次升級 V8 要重新對。RFC 0058 試過只搬 `Member` 的寫入，jQuery 的 cycle 少約 2%。
  - 丙、減少呼叫的次數：建節點時一次寫好所有的邊、parser 建的節點不逐個 root、bindings 回傳已有 wrapper 的節點不經過 `TracedReference::Get`。不動 build 和 V8 內部，收益受限於還是要穿過邊界的那些。
  - 2026-10-24 的看法：丙先做（可以併進來源 2 的 RFC），甲先花一兩天驗證編不編得起來、shim 能不能 inline，有結果再決定；乙最後。
- **值多少**：V8 API 加 GC 機制的差約 75；全部補平不可能（邊界還在），補一半約 37。

#### 來源 2：資料的表示，配置和複製

- **證據**：配置的時間（inclusive）63 對 15、`memcpy` 這類 40 對 8、page fault 25 對 18（差最多的 8 個 suite）；分散在 DOM（`create_node` 底下 memcpy 7.6）、parse（`fast_path::build` 底下配置 10.1）、style、layout（`InlineContent::new` 配置 3.1）、文字、paint，每個 suite 都有。
- **Chromium**：屬性值是共用的 `AtomicString`，複製字串只是複製指標；parser 建的、屬性一樣的元素共用 `ShareableElementData`（`ElementDataCache`）；class 用 `SpaceSplitString` 的快取；`ComputedStyle` 的欄位分組（`DataRef`），寫的時候才複製；Oilpan 的配置是推指標。
- **kun**（`crates/dom/src/node.rs`、`scope.rs`、`crates/html/src/fast_path.rs`）：每個元素 `attributes: Vec<Attribute>` 一次配置，每個屬性值 `String` 各一次，`id: Option<Box<str>>`、`class_names: Box<[Box<str>]>`（一次加每個 class 一次）、`class_hashes: Box<[u32]>`；`class_names_of` 在每次建元素和改屬性時重新切字串、重新配置（5.7 加配置 3.0）；fast path 每個屬性 `decode(value).into_owned()`；Node 在 stack 上建好、傳進 `make_garbage_collected_on`、再由 `init_rust_obj` 寫進 heap，搬三次，而 Node 的大小被 `NodeState` 最大的變體（Document）撐大；行內內容每次重建都把 DOM 和 style 翻成 parley 的模型。
- **能怎麼改**：
  - 甲、屬性值和 class 改成共用的字串（string_cache 的動態 atom，或 kun 自己的 interner；RFC 0040 已經把名稱改成 atom，值沒改）：複製變成複製指標，比較變成比指標；代價是 interner 的表和它的生命週期（string_cache 的動態 atom 最後一個 `Drop` 鎖全域的表，`dom` 現在刻意不放動態 atom）。
  - 乙、parser 建的同形元素共用屬性資料（`ShareableElementData`）：jQuery 這種每次 render 重建幾萬個同樣的 `<li>` 的頁面省最多；要分「共用的」和「改過的」兩種狀態。
  - 丙、Node 在 heap 上建、建一次：`make_garbage_collected_on` 改成先配置再在原地初始化（rusty_v8 fork 的改動），Document 的變體 box 起來讓 Node 變小。
  - 丁、style 和 layout 的中間結構重用：`InlineContent` 的 `Vec` 跨次重用、`ComputedStyle` 更多的分組、`cascade` 每個元素收集的 block 不每次配置。
  - 2026-10-24 的看法：甲、乙、丙一起寫成一份 RFC（「節點和屬性的表示」），用每個 iteration 的配置次數和複製的 byte 數當指標；丁分散在 style、layout 的 RFC 裡。
- **值多少**：配置、複製、page fault 的差約 85（8 個 suite）；補一半約 40。

#### 來源 3：快取失效的粒度

- **證據**：`collection::item` 5.5 加上它重算的子樹走訪；`style::cascade` 34.6 和 `recalc_style` 21.8 對 Chrome 的 `CollectMatchingRules` 5.1、`StyleCascade::Apply` 1.4；`walk_moved_subtree` 8.8（每個進出樹的節點查 iframe、script、title、style、base、form、link 和 tree scope 的表）。
- **Chromium**：live collection 的快取由 `NodeListsNodeData` 照改動的種類和位置個別失效；`InsertedInto`、`RemovedFrom` 由各類別自己覆寫，普通的元素幾乎不做事；style 的重算每個元素的固定成本很低（`ElementRuleCollector` 只看 bucket 裡可能的規則）。
- **kun**：collection 的快取用全域的 `dom_tree_version`（`crates/dom/src/scope.rs`），任何樹的改動都讓每個 collection 失效、下次整個子樹重走（jQuery 的 `cleanData` 一邊走 `getElementsByTagName('*')` 一邊移除節點，就變成每拿一項重算一次）；`cascade` 每個元素都收集 5 組 block、建 `Recascade`；`walk_moved_subtree` 每個節點查所有的表。
- **能怎麼改**：collection 的快取照 Blink 分成「子節點的改動」「屬性的改動」「子樹的改動」三種失效；`walk_moved_subtree` 照元素的種類只做它需要的（Blink 的 `InsertedInto` 的覆寫）；`cascade` 和 `recalc_style` 的每個元素固定成本要先拆開量（哪些是收集 block、哪些是 `Recascade` 的建立、哪些是 MatchedPropertiesCache 的查找）。
- **值多少**：DOM 和 style 的差合計約 217，這類粒度的問題佔多少還要拆開量；保守估計 30 到 50。

#### 來源 4：編譯和 allocator 的設定

- **證據**：每一類都平均慢一兩倍，和「每一行程式都慢一點」的形狀對得上；kernel 的 page fault 25 對 18。
- **Chromium**：official build 是 ThinLTO 加 PGO（插樁的 build 跑 Speedometer 3、JetStream 3 等收 profile，`-fprofile-use`）；V8 另外有 builtins PGO。
- **kun**：`perf` profile 有 ThinLTO、沒有 PGO，Rust 從來沒試過 PGO；V8 的 fork 只設 `is_debug=false`，沒有 `is_official_build`、`use_thin_lto`、PGO；mimalloc 的 purge delay 預設 10 ms，釋放的記憶體很快還給系統，下一個 iteration 再 page fault 回來。
- **能怎麼改**：另外一份 RFC（RFC 0086）：Rust 的 PGO 用 rustc 的 `-Cprofile-generate` / `-Cprofile-use`，用 `bench.py speedometer` 訓練；V8 的 C++ PGO 和 ThinLTO 要重編兩次 V8、換共用的 artifact；purge delay 調大並量 page fault。
- **值多少**：Rust 專案開 PGO 常見的是 5 到 15%，實際要量；V8 的 PGO 和 ThinLTO 照 RFC 0058 的估計約 20 到 40 ms。

#### 情境的估計

CPU 時間的幾何平均（今天 1.815；對應比較表的分數再乘約 1.11）：

| 情境 | 幾何平均 | 對應的分數 |
| --- | ---: | ---: |
| kun 的 Rust 快 10%（來源 4） | 1.69 | 約 1.88 |
| 配置、複製、page fault 的差補一半（來源 2） | 1.75 | 約 1.94 |
| FFI、V8 API、GC 機制的差補一半（來源 1） | 1.77 | 約 1.97 |
| 三個一起 | 1.59 | 約 1.77 |
| V8 以外每一類做到 Chrome 的 1.3 倍 | 1.32 | 約 1.47 |
| V8 以外每一類追平 | 1.13 | 約 1.26 |
| 連 JS 也追平 | 1.00 | |


### 量法

2026-10-24，Windows 11。

- **環境**：Intel Core i9-13900K，兩個瀏覽器都綁在 P-core（0–15）；kun `ce849649`（`perf` profile：ThinLTO，沒有 PGO；release 的 prebuilt V8 15.2），Chrome 155.0.8059.85（這台機器裝的，V8 15.5；用暫時的 user-data-dir，headless）。
- **為什麼不是 Chrome for Testing 152**：它的 PDB 不在 Chromium 的 symbol server 上（`chrome.dll.pdb`、`chrome.exe.pdb` 都是 404），沒有符號就分不了類。同樣的取樣下兩個版本都跑了差最多的 8 個 suite：合計 391.2 對 389.9 ms，每個 suite 差 2% 以內，JIT 出來的程式碼本身 34.3 對 33.9 ms，所以拿 155 的分類代表 152。
- **取樣**：samply 經過 ETW，1000 Hz，以 CPU 時間計（每個樣本算該執行緒自上個樣本以來用掉的 µs，等待不算）、連 kernel 的 frame，只留 runner 計時的區間（trace 裡 User Timing 的 mark）裡 renderer main thread 的樣本。kun 用 `bench.py speedometer --sample --iterations 5`；Chrome 用同樣的做法，另外處理的事：以系統管理員權限（ETW 要）啟動的 Chrome 會把自己重新啟動成一般權限，CDP 的 pipe 跟著斷，要加 `--do-not-de-elevate`；由 samply 啟動 Chrome 時 pipe 的 handle 傳不過去，所以先照平常的方式啟動 Chrome，再用 `samply record -p` 接上，結束時送 Ctrl+Break；samply 把 renderer 的 main thread 叫做 `chrome.exe`，用 trace 的 track descriptor 裡 `CrRendererMain` 的 tid 找出來；Chrome 的 DLL 用 `samply load --windows-symbol-server`（Microsoft 的和 Chromium 的）命名，`chrome.dll.pdb` 5.6 GB。腳本不在 repo 裡（「待決定」第 3 條）。
- **取樣的負擔**：Chrome 被取樣時合計 617.8 ms，比較表（沒取樣）的中位數合計 556.9，慢約 11%；kun 1130.4 對 1140.0，沒有變慢。Chrome 的 V8 flag（`--enable-etw-stack-walking --interpreted-frames-native-stack`，兩邊都加）和 ETW 的事件是最可能的原因。所以下面的倍數都低估約一成。
- **分類**：每個樣本照最內層說得出種類的 frame 分；V8 自己的 frame（`v8::internal`、`Builtins_`）往外看：外面是 JS 的函式算 JS，外面是 V8 的公開 API（`v8::`）或 kun 的程式算 V8 API；配置（mimalloc、`RtlAllocateHeap`、PartitionAlloc、`operator new`）、`memcpy`、標準函式庫的容器算給呼叫它的。kun 照 crate 分（`layout::text` 和 parley、harfrust、fontique 算文字，taffy 和 `layout` 的其他算 box layout，`style`、`css`、`selectors` 算 style……）。Blink 照類別名稱分，先去掉 template 參數（`HashTable<cppgc::…>` 不算 GC）：painter 算 paint，style 在文字之前（`InlineCSSStyleDeclaration`、`FontBuilder`），inline layout、shaping、字型、HarfBuzz、ICU 算文字，其他 layout 算 box layout，DOM 類別上的 `RecalcStyle`、`AttachLayoutTree` 算 style 和 box layout；cc、base、WTF 的字串和容器不算種類，交給呼叫它的。kun 這邊用同一個腳本重分，和之前只分 kun 的腳本差不到 0.6 ms。

### 量測

#### 20 個 suite 的分類

每個 iteration 的 ms，20 個 suite 合計：

| 種類 | kun | Chrome | 差 | 倍數 | 只有這一類追平時的幾何平均 |
| --- | ---: | ---: | ---: | ---: | ---: |
| JS | 312.4 | 232.1 | +80.3 | 1.3 | 1.67 |
| DOM | 192.6 | 71.3 | +121.3 | 2.7 | 1.62 |
| style | 156.0 | 60.6 | +95.4 | 2.6 | 1.66 |
| 文字 | 100.4 | 83.6 | +16.8 | 1.2 | 1.77 |
| box layout | 87.4 | 52.0 | +35.4 | 1.7 | 1.76 |
| V8 API | 63.2 | 9.3 | +53.9 | 6.8 | 1.72 |
| HTML parse | 47.4 | 14.9 | +32.5 | 3.2 | 1.79 |
| paint | 45.8 | 36.0 | +9.7 | 1.3 | 1.78 |
| GC | 44.1 | 20.4 | +23.7 | 2.2 | 1.79 |
| bindings | 43.2 | 25.5 | +17.7 | 1.7 | 1.78 |
| 其他 | 33.3 | 7.1 | +26.2 | 4.7 | 1.76 |
| canvas 的 raster | 4.7 | 5.1 | −0.4 | 0.9 | |
| 合計 | 1130.4 | 617.8 | +512.6 | 1.83 | |

main thread 的 CPU 時間的比值，20 個 suite 的幾何平均 1.815。

#### 每個 suite

kun / Chrome 的 ms，倍數，差最多的三類：

| suite | kun | Chrome | 倍數 | 差最多的 |
| --- | ---: | ---: | ---: | --- |
| TodoMVC-jQuery | 188.6 | 85.3 | 2.21 | DOM +37.1、HTML parse +25.6、JS +13.1 |
| NewsSite-Next | 115.8 | 66.1 | 1.75 | style +19.5、box layout +10.5、DOM +5.6 |
| NewsSite-Nuxt | 88.1 | 48.7 | 1.81 | style +13.5、box layout +12.1、DOM +5.6 |
| Editor-TipTap | 83.6 | 47.5 | 1.76 | 文字 +27.1、paint +3.5、JS +2.0 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 60.6 | 26.2 | 2.31 | DOM +9.7、style +9.0、HTML parse +3.8 |
| React-Stockcharts-SVG | 88.8 | 55.2 | 1.61 | style +8.4、JS +8.0、DOM +5.1 |
| Charts-chartjs | 62.2 | 31.1 | 2.00 | JS +9.7、DOM +6.9、V8 API +6.6 |
| Perf-Dashboard | 64.4 | 36.8 | 1.75 | DOM +7.9、style +7.6、V8 API +4.5 |
| TodoMVC-WebComponents | 30.1 | 11.8 | 2.54 | DOM +5.5、style +5.3、V8 API +3.4 |
| TodoMVC-JavaScript-ES5 | 48.9 | 31.4 | 1.56 | DOM +9.4、JS +4.1、GC +2.8 |
| TodoMVC-Backbone | 34.7 | 18.2 | 1.90 | JS +6.2、DOM +4.1、V8 API +2.9 |
| TodoMVC-Lit-Complex-DOM | 27.8 | 11.8 | 2.35 | style +5.1、DOM +4.2、paint +1.9 |
| TodoMVC-React-Complex-DOM | 39.0 | 23.0 | 1.69 | DOM +4.8、JS +4.6、其他 +1.5 |
| TodoMVC-Angular-Complex-DOM | 36.3 | 20.5 | 1.77 | JS +6.5、DOM +3.1、style +2.3 |
| Charts-observable-plot | 44.3 | 30.9 | 1.44 | style +7.2、JS +4.4、DOM +2.3 |
| TodoMVC-React-Redux | 37.0 | 23.9 | 1.55 | JS +5.8、DOM +4.0、V8 API +2.4 |
| TodoMVC-Vue | 27.2 | 15.6 | 1.75 | JS +4.0、DOM +2.2、style +2.0 |
| TodoMVC-Preact-Complex-DOM | 16.5 | 8.8 | 1.88 | 其他 +2.4、box layout +1.4、V8 API +1.4 |
| TodoMVC-Svelte-Complex-DOM | 15.4 | 7.8 | 1.97 | DOM +2.1、其他 +2.1、paint +1.4 |
| Editor-CodeMirror | 21.1 | 17.1 | 1.24 | JS +2.8、V8 API +1.0、box layout +0.9 |

輕的 12 個 suite 差最多的也是 DOM、style、JS、V8 API，和重的 8 個一樣。文字和 paint 整體差得少：TipTap 一個 suite 佔了文字的差的大半。

#### 每一類最多的函式

20 個 suite 合計的 ms，kun 和 Chrome：

| 種類 | kun | Chrome |
| --- | --- | --- |
| DOM | `create_node` 21.7、`event::dispatch_with` 9.9、`walk_moved_subtree` 8.8、`class_names_of` 5.7、`collection::item` 5.5、`link_child` 5.0、`detach_with` 4.9、`event_listeners` 4.1、`create_element_of` 3.2 | `Text::Create` 3.3、`SetInnerHTMLInternal` 3.1、`ElementDataCache::CachedShareableElementDataWithAttributes` 3.0、`Element::AttributeChanged` 2.8、`HTMLInputElement::ParserDidSetAttributes` 2.4、`Element::InsertedInto` 2.1、`Element::RemovedFrom` 1.8、`Node::Node` 1.8、`EventTarget::FireEventListeners` 1.8 |
| style | `cascade` 34.6、`StyleEngine::recalc_style` 21.8、`matches_complex_selector_internal` 16.7、`SelectorQuery::query` 8.9、`cascade_blocks` 8.4、`recalc_style` 的 closure 7.2、`ComputedStyle::rare_mut` 4.4、`has_compound` 3.7 | `ElementRuleCollector::CollectMatchingRulesInternal` 5.1、`SelectorChecker::MatchSelector` 3.4、`SelectorQuery::Execute` 3.1、`Element::RecalcStyle` 3.0、`StyleCascade::CollectFromMatchResult` 2.4、`StyleResolver::ResolveStyle` 1.8、`StyleCascade::Apply` 1.4 |
| V8 API | callback 的入口 `c_fn` 12.1、`String::to_rust_string_lossy` 4.5、`Object::Get` 3.4、`PostTaskImpl` 3.4、`Function::Call` 3.3、`Object::Wrap` 3.2、`ObjectTemplate::NewInstance` 3.1、`FunctionTemplate::GetFunction` 2.9、`String::ValueView` 2.1、`v8__Object__Unwrap` 2.1 | `Object::Wrap` 1.4、`Function::Call` 1.3、`Factory::CopyJSObject` 1.1、`Object::Get` 0.8、`FunctionTemplate::GetFunction` 0.8 |
| GC | `BasicPersistent::Assign` 8.1、`init_rust_obj` 5.5、`Scavenger::ScavengeObject` 4.5、`Root::try_new` 4.3、`Root::new` 3.3、`MakeGarbageCollectedTraitInternal::Allocate` 2.7 | `ScavengerCollector::CollectGarbage` 4.0、`ObjectAllocator::OutOfLineAllocateGCSafePoint` 2.4、`PageBackend::TryAllocateNormalPageMemory` 2.0、`Scavenger::Process` 1.7、`WriteBarrier::DijkstraMarkingBarrierSlow` 1.4 |
| bindings | `host::call` 7.0、`properties::call` 4.8、`typed::this` 3.4、`call_member` 2.6、`host::mutate` 2.6、`ensure_templates` 2.2、`convert_arg` 2.1、`interface_object` 2.0、`to_js` 2.0 | `ScriptWrappable::Wrap` 3.2、`V8UnionStringLegacyNullToEmptyStringOrTrustedHTML::Create` 2.9、`V8HTMLCollection::IndexedPropertyGetterCallback` 2.2、`SetAttributeOperationCallback` 1.6、`V8ScriptRunner::CallFunction` 1.2 |
| 其他 | `FrameView::update_all_lifecycle_phases` 3.7、`FrameView::url` 3.6、`FrameView::image` 3.4、`gfx::Path::bounds` 3.3、`gfx::Path::arc_to` 2.6、`scroll::container_nodes` 2.5 | `LocalFrameView::UpdateStyleAndLayout` 0.6（其他的 cc、base 算給呼叫它的） |

差最多的 8 個 suite 那次的 box layout 和文字：kun 的 box layout 是 taffy 的 `compute_preliminary` 10.1、`extend_overflow` 7.2、`compute_inline` 3.7、`geometry::walk` 3.3、`fragment` 3.2；文字是 `shape_text` 11.3、`InlineContent::new` 8.5、`Analyzer::analyze` 6.3、`break_next_line_or_box` 5.3、`WordRuns::flush` 5.0、`LineState` 的 clone 5.0、`content_widths` 4.7。Chrome 的 box layout 是 `PhysicalBoxFragment::Create` 4.3、`BlockLayoutAlgorithm::Layout` 2.5、`BlockNode::Layout` 2.3、`FlexLayoutAlgorithm::LayoutInternal` 1.8；文字是 `SkFontGetGlyphWidthForHarfBuzz` 4.4、HarfBuzz 的 GPOS 2.8、`SkFontGetBoundsForGlyphs` 2.3、`hb_shape_plan_execute` 2.2、`ComputeInkBounds` 1.8。

#### 跨越每一類的成本

差最多的 8 個 suite（kun 734.2 / Chrome 389.9），最內層就是這類函式的時間（leaf）和 stack 上任何一層有它的時間（inclusive），ms：

| | kun leaf | kun inclusive | Chrome leaf | Chrome inclusive |
| --- | ---: | ---: | ---: | ---: |
| 配置和釋放（mimalloc、`RtlAllocateHeap`；PartitionAlloc、`operator new`） | 45.1 | 63.4 | 6.8 | 14.6 |
| `memcpy`、`memmove`、`memset`、`memcmp` | 34.3 | 40.5 | 5.0 | 8.4 |
| kernel 的 page fault（`KiPageFault`、`MiInitializePfn`、`MiAddWorkingSetEntries`……） | 14.7 | 24.9 | 9.9 | 18.0 |
| rusty_v8 的 FFI shim（`v8__*`、`cppgc__*` 本身） | 10.6 | | 0 | |
| hash table（hashbrown、`FxHasher`） | 6.2 | 10.1 | 0 | |
| `clone`、`to_owned`、`to_vec`、`to_string` | 7.0 | 7.6 | | |

kun 的配置照呼叫它的函式分（inclusive）：`fast_path::build` 10.1、`InlineContent::new` 3.1、`class_names_of` 3.0、`InlineContent::fragments` 2.2、`BoxTree::allocate` 1.6、`canvas::record_op_under` 1.6、`Painter::text_run` 1.6、`shape_text` 1.5、`gfx::Path::bounds` 1.2、`indexed_item` 1.1。`memcpy` 這類：`create_node` 7.6、`matches_complex_selector_internal` 3.0、`cascade` 2.3、`record_op_under` 2.0、`BoxTree::allocate` 1.8、`compute_inline` 1.4。page fault：`create_node` 2.0、`WordRuns::flush` 1.9、`fast_path::build` 1.6、`InlineContent::fragments` 1.4、`InlineContent::new` 1.2。

shim 最多的：`v8__Object__Unwrap` 1.4、`cppgc__Member__Assign` 1.3、`v8__Value__NumberValue` 1.2、`v8__TracedReference__Get` 1.2、`cppgc__Member__CONSTRUCT` 0.8、`cppgc__Persistent__Assign` 0.5。

#### JS 那一類怎麼分（參考）

8 個 suite，kun 181.0 / Chrome 133.7：JIT 出來的程式碼 40.4 / 34.8、V8 的 C++ runtime 45.4 / 32.0、IC 的 builtin 33.6 / 24.3、其他 builtin 33.5 / 27.4、Sparkplug 的 builtin（`_Baseline`）12.8 / 5.2、JSON 8.9 / 5.0、編譯和 parse 6.5 / 5.0。幾乎每一項都慢 1.2 到 1.5 倍，沒有單一的慢路徑；runtime 裡 kun 多的是 `JsonParse` 8.1 / 5.5、`CompileLazy` 6.2 / 4.3、`StackGuard` 5.8 / 3.9、`StartMaglevOptimizeJob` 2.9 / 1.5、`Intl.NumberFormat` 的建構 1.9（kun 沒有載入 ICU 的資料）。和 RFC 0058 查過的「V8 的 C++ runtime 慢 13 到 71%」一致，是 V8 的 build 設定（沒有 PGO 和 ThinLTO）和版本（15.2 對 15.5）的事，不在這份的範圍。

#### 情境的估計

每個 suite 的 kun 時間照情境改，Chrome 不變，算 20 個 suite 的幾何平均（今天 1.815；對應比較表的分數再乘約 1.11）：

| 情境 | 怎麼算 | 幾何平均 |
| --- | --- | ---: |
| A. kun 的 Rust 快 10%（PGO、purge delay） | JS 以外乘 0.9 | 1.69 |
| B. 配置、複製、page fault 的差補一半 | JS 以外乘 (1 − 0.055) | 1.75 |
| C. FFI、V8 API、GC 機制的差補一半 | JS 以外乘 (1 − 0.035) | 1.77 |
| A + B + C | | 1.59 |
| F. V8 以外每一類做到 Chrome 的 1.3 倍 | kun 的 JS 加 1.3 倍 Chrome 的其他 | 1.32 |
| D. V8 以外每一類追平 | kun 的 JS 加 Chrome 的其他 | 1.13 |
| E. D 再加 JS 追平 | Chrome 的全部 | 1.00 |

### 對照程式碼的證據

- **每寫一條邊一次 FFI**：`crates/dom/src/gc.rs` 的 `Edge::set` 呼叫 `MemberSlot::assign`，它是 rusty_v8 fork 的 extern `cppgc__Member__Assign`，在 `src/binding.cc` 裡用 `HeapConsistency::GetWriteBarrierType` 判斷 barrier。
- **Node 搬三次**：`crates/dom/src/scope.rs` 的 `create_node` 呼叫 `Node::new(id, kind, state)`（回傳一份）、傳給 `v8::cppgc::make_garbage_collected_on`（一份），fork 的 `src/cppgc.rs` 的 `init_rust_obj` 再 `read()` 寫進 heap（一份）。`Node` 的 `state: RefCell<NodeState>` 是 enum，大小由最大的變體 `Document`（`content_type`、`url`、`fallback_base_url`、`character_set`、`content_language` 等 `Box<str>` 和 `Origin`）決定。
- **每個元素的配置**：`crates/dom/src/node.rs` 的 `NodeState::Element`：`attributes: Vec<Attribute>`（`Attribute { name: QualName, value: String }`）、`id: Option<Box<str>>`、`class_names: Box<[Box<str>]>`、`class_hashes: Box<[u32]>`；`class_names_of` 每次從 `class` 屬性切出 `Vec<Box<str>>`，在 `scope.rs` 的建元素和屬性改變兩處呼叫。
- **fast path 的配置**：`crates/html/src/fast_path.rs` 的 `build` 每個元素 `collect()` 一個 `Vec<Attribute>`，每個屬性 `decode(value).into_owned()`。
- **collection 的失效**：`crates/dom/src/scope.rs` 的 `collection_item_ids` 比對 `cache.version == self.dom_tree_version`，`dom_tree_version` 在四處樹的改動時 bump；不新鮮就 `compute`（`dom_api/src/collection.rs` 的 `compute_items`：走整個子樹）。
- **cascade 的每個元素固定成本**：`crates/style/src/lib.rs` 的 `cascade` 每個元素建 5 組 `Blocks`、對每個 `RuleSource` 收集、建 `Recascade`。
- **taffy 的 style**：`crates/layout/src/lib.rs` 的 `LayoutPartialTree for BoxTree` 把每個盒子的 `taffy_style` 存著（翻譯一次），`resolve` 是 taffy 內部的；`InlineContent::new`（`layout/src/text.rs`）每次重建都重新收集 run、element、`vertical_align`。
- **編譯設定**：`Cargo.toml` 的 `profile.perf` 繼承 release 加 `lto = "thin"`、`codegen-units = 1`，沒有 PGO；fork 的 `build.rs` 只設 `is_debug=false`，Chromium 的 `build/config/compiler/pgo/` 在 fork 裡但 `chrome_pgo_phase` 是 0，`use_thin_lto` 沒設；`crates/shell/src/main.rs` 的 `MIMALLOC_PURGE_DELAY` 預設 10 ms。

### blink-perf 的單一操作（2026-10-24，Windows 11，Chrome for Testing 152）

`bench.py blink-perf --pages`，每頁在 kun 和 Chrome 各開一個新的瀏覽器跑一次。每次呼叫的時間是 1 秒除以（runs/s 乘上每一輪的呼叫次數），包含迴圈本身。

#### bindings 頁

21 頁有效，幾何平均 kun 慢 5.57 倍；`node-list-access` 是 Chrome 90 秒內沒有結果。

| 頁 | 每一輪呼叫 | kun runs/s | Chrome runs/s | kun 慢 | kun 每次 | Chrome 每次 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `node-type`（`nodeType`） | 100000 | 1051 | 2978 | 2.83 | 9.5 ns | 3.4 ns |
| `first-child`（`firstChild`） | 100000 | 876 | 2804 | 3.2 | 11.4 ns | 3.6 ns |
| `undefined-first-child` | | 1096 | 2854 | 2.6 | | |
| `id-getter`（`id`） | 100000 | 279 | 1390 | 4.97 | 35.8 ns | 7.2 ns |
| `undefined-id-getter` | | 490 | 2801 | 5.71 | | |
| `get-attribute`（`getAttribute('id')`） | 100000 | 163 | 439 | 2.7 | 61 ns | 23 ns |
| `get-attribute-rare` | | 181 | 762 | 4.2 | | |
| `indexed-getter`（`HTMLCollection[i]`） | 1000 | 14269 | 92437 | 6.48 | 70 ns | 10.8 ns |
| `dom-attribute-on-prototoype`（14 種元素的 `baseURI`） | 140000 | 84.3 | 517 | 6.13 | 85 ns | 13.8 ns |
| `document-implementation` | 100000 | 62.1 | 1966 | 31.6 | 161 ns | 5.1 ns |
| `get-elements-by-tag-name` | 50000 | 105 | 1099 | 10.5 | 190 ns | 18 ns |
| `get-element-by-id` | 100000 | 51.2 | 554 | 10.8 | 195 ns | 18 ns |
| `undefined-get-element-by-id` | | 57.2 | 448 | 7.83 | | |
| `id-setter`（`id = 'foo'`） | 50000 | 97 | 693 | 7.14 | 206 ns | 29 ns |
| `set-attribute`（`setAttribute('id', 'foo')`） | 10000 | 387 | 2551 | 6.58 | 258 ns | 39 ns |
| `set-attribute-rare` | | 511 | 1857 | 3.64 | | |
| `create-element`（`createElement('div')`） | 5000 | 247 | 2731 | 11.1 | 810 ns | 73 ns |
| `named-property-enumerator` | | 16 | 211 | 13.2 | | |
| `sequence-conversion-array` | | 20.4 | 247 | 12.1 | | |
| `append-child` | | 119 | 147 | 1.23 | | |
| `insert-before` | | 92.1 | 132 | 1.43 | | |

`document.implementation`、`getElementById`、`getElementsByTagName` 每次都先讀 global 的 `document`：`document` 在 dictionary 模式的 global object 上，V8 的 IC 不 cache 它的 API getter，kun 還替 `Window` 建一個 root。這幾頁的倍數有一部分是 `document` 的。

bindings 這一層的固定成本，差最多的 8 個 suite 的取樣裡每個 iteration 的 ms（每個樣本從最內層往外找第一個屬於下列一項的 frame）：root（`Root`、`Handle` 的建立和放掉、`BasicPersistent`）16.5、經過 FFI 的 V8 小函式 11.7、建 wrapper（`interface_object`、`GetFunction`、`NewInstance`、`Wrap`）9.8、callback 本身（`c_fn`）5.3、共用路徑的分派 4.9、字串 3.6、照 tag 試著 unwrap `this` 3.3、global 的 getter 0.8，合計 55.9，不含它們的配置。

#### innerHTML 頁

三頁有效，幾何平均 kun 慢 2.76 倍。

| 頁 | 做什麼 | kun runs/s | Chrome runs/s | kun 慢 |
| --- | --- | ---: | ---: | ---: |
| `innerHTML-setter` | 設一棵深 10 的二元樹（1023 個 `<div>`、1024 個文字節點），再設成 `''` | 1962 | 6441 | 3.28 |
| `innerHTML-setter-siblings` | 依序設 50 個兄弟 `<div>`、10 個有重複屬性的 `<div>`、10 組 5 層的 `<div>`，再設成 `''` | 16772 | 55746 | 3.32 |
| `tiny-innerHTML` | 一個 `<div>` 設 10 萬次 `'This is a tiny HTML document'` | 10.4 | 19.9 | 1.92 |

換算：`innerHTML-setter` 每一輪 kun 約 510 µs、Chrome 約 155 µs，每個節點建一次再拆一次約 250 / 76 ns；`tiny-innerHTML` 每次設定約 960 / 500 ns。這些頁在 kun 上 `create_node` 佔 innerHTML 的 54 到 70%，其中節點 id 表約 27%（blink-perf 的 runner 用 `setTimeout(task, 0)` 排下一輪，render thread 一直在 `run_due_tasks` 裡，id 表的 prune 只在 loop 回到頂端時跑，所以表長到 840 萬項）、root 約 8%、cppgc 的配置約 17%、parse 約 13%、`replace_all` 約 7%。Speedometer 裡沒有這種一直不 prune 的情況（下一節）。

#### 節點 id 表的大小

2026-10-24，`bench.py speedometer --browser kun --suites <一個>`，20 個預設的 suite 各開一個 kun、10 個 iteration；暫時在 `NodeIdMap::prune` 裡印出表每長大一倍時、和每次真的 prune 時的項目數和容量（改動已還原）。prune 在 loop 回到頂端時檢查，看到的是那時的大小。雜湊表的大小是 bucket 數乘以 9 bytes。

| Suite | 最多的項目 | 容量 | 雜湊表 MB | prune 次數 | prune 之後最多剩 |
| --- | ---: | ---: | ---: | ---: | ---: |
| TodoMVC-jQuery | 384,726 | 458,752 | 4.7 | 23 | 151,874 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 301,340 | 458,752 | 4.7 | 7 | 150,184 |
| TodoMVC-JavaScript-ES5 | 266,756 | 458,752 | 4.7 | 8 | 132,592 |
| TodoMVC-Lit-Complex-DOM | 79,354 | 114,688 | 1.2 | 5 | 38,742 |
| TodoMVC-Svelte-Complex-DOM | 71,606 | 114,688 | 1.2 | 5 | 35,390 |
| TodoMVC-Angular-Complex-DOM | 70,188 | 114,688 | 1.2 | 5 | 34,784 |
| TodoMVC-Preact-Complex-DOM | 68,722 | 114,688 | 1.2 | 5 | 34,154 |
| TodoMVC-React-Complex-DOM | 68,602 | 114,688 | 1.2 | 5 | 34,094 |
| Charts-observable-plot | 54,764 | 57,344 | 0.6 | 7 | 26,412 |
| NewsSite-Next | 36,927 | 57,344 | 0.6 | 6 | 17,991 |
| TodoMVC-WebComponents | 34,911 | 57,344 | 0.6 | 4 | 34,911 |
| TodoMVC-Backbone | 29,269 | 57,344 | 0.6 | 4 | 29,269 |
| NewsSite-Nuxt | 22,964 | 28,672 | 0.3 | 8 | 10,894 |
| Perf-Dashboard | 18,144 | 28,672 | 0.3 | 8 | 8,734 |
| Editor-TipTap | 17,235 | 28,672 | 0.3 | 5 | 8,475 |
| TodoMVC-Vue | 9,942 | 14,336 | 0.1 | 3 | 9,942 |
| Editor-CodeMirror | 4,986 | 7,168 | 0.1 | 3 | 4,986 |
| React-Stockcharts-SVG | 4,921 | 7,168 | 0.1 | 6 | 3,467 |
| TodoMVC-React-Redux | 3,590 | 7,168 | 0.1 | 2 | 3,590 |
| Charts-chartjs | 不到 1,024 | | | 0 | |

每個 suite 都有 prune。TodoMVC-jQuery 的 23 次 prune 裡有 9 次一項都沒拿掉（那時 GC 還沒回收）；它每次 render 重建整個清單，每個 iteration 建十幾萬到三十幾萬個節點。i9-13900K 的 P-core 的 L2 是 2 MB：前三個 suite 的表比它大。這個表本身是來源 1（不掃 stack）的一部分成本，由「節點和屬性的表示」那份 RFC 一起處理。

### 和 Blink 的逐項對照

| 項目 | kun | Blink |
| --- | --- | --- |
| DOM 和 JS 引擎的邊界 | Rust 的 `dom` 和 C++ 的 V8、cppgc 之間是 rusty_v8 的 C shim，每個都是一次呼叫 | 同一個 binary，ThinLTO 加 PGO，inline |
| stack 上的 GC 指標 | 不掃 stack：操作之外的是 root（`Persistent`），新節點進 id 表 | Oilpan 保守掃描 stack，裸指標 |
| 寫一條邊 | `Edge::set` 呼叫 `cppgc__Member__Assign`，C++ 判斷 barrier | `Member` 的賦值 inline，barrier 的快速路徑是幾個指令 |
| 新節點的配置 | 在 stack 上做好 `Node` 再搬進 heap（三次）；`Node` 擁有 Rust 的 `String`、`Vec`，回收時跑 `Drop` | 在 heap 上直接建；parser 建的、屬性一樣的元素共用 `ShareableElementData`（`ElementDataCache`） |
| 屬性值、class | 每個值一個 `String`，class 切成 `Box<[Box<str>]>`，每次建元素和改屬性重切 | `AtomicString`，`SpaceSplitString` 的快取 |
| live collection 的快取 | 全域的 `dom_tree_version`，任何改動都失效、整個子樹重算 | `NodeListsNodeData` 照改動的種類和位置失效，`CollectionIndexCache` |
| 插入、移除時走子樹 | `walk_moved_subtree`：每個節點查 iframe、script、title、style、base、form、link、tree scope 的表 | `InsertedInto`、`RemovedFrom`，各類別自己覆寫，普通元素幾乎不做事 |
| style 的每個元素 | `cascade` 每個元素收集 5 組 block、建 `Recascade` | `ElementRuleCollector` 只看 bucket 裡可能的規則，`StyleCascade` 重用 |
| `ComputedStyle` | `font`、`rare`、`rare_inherited`、`svg` 分組，主體整份複製 | `DataRef` 的多個群組，寫的時候才複製 |
| layout、文字 | taffy、parley：每次排版把 DOM 和 style 翻成它們的模型 | LayoutNG 直接在 `LayoutObject` 上排，inline item 直接引用 DOM 的字串 |
| 編譯 | ThinLTO，沒有 PGO；V8 不是 official build | ThinLTO 加 PGO；V8 另有 builtins PGO |
| allocator | mimalloc，purge delay 10 ms | PartitionAlloc，thread cache |

## 參考資料

- Chromium：`third_party/blink/renderer/core/dom/element_data_cache.h`、`space_split_string.h`、`node_lists_node_data.h`、`core/css/resolver/style_cascade.h`、`core/css/element_rule_collector.h`、`platform/heap/`（Oilpan）、PGO 的文件（pgo.md）、`tools/pgo/generate_profile.py`、`build/config/compiler/pgo/`。
- V8：`tools/builtins-pgo/`、`gni/v8.gni` 的 `v8_enable_builtins_optimization`。
- 之前的量測和分析：Speedometer 量測工具那份 RFC 的 `../0012-speedometer/progress.md` 的「量測的紀錄」（2026-10-24 的幾筆）、`rfcs/0058-performance-2/details.md` 的「V8 本身」、RFC 0060、0061、0067、0072（bindings 和 root 已經做過的優化）。
