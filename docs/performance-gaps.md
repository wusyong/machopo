# 和 Chrome 的效能差在哪

machopo 和 Chrome 的時間差在哪一種工作、哪幾個函式，以及可以開 RFC 的方向。這份是最近一次分析的快照（machopo 2026-11-08、Chrome 2026-11-06，Linux：DOM 換成照 elise 設計的之後，兩邊照同樣的方法取樣；GC、bindings、JS 的設定、DOM 的一輪優化和配置的快路之後）：重新量的時候整份照新的結果改寫、換日期；某個方向做完之後，重量那一部分、改寫那一節。怎麼量在 `benchmarks.md`；每次量到的原始數字記在 Speedometer 量測工具那份 RFC 的 `progress.md`。

## 量法

- **環境**：Fedora 44（Linux 7.2），AMD Ryzen AI 9 HX 370（筆電，接電源），兩個瀏覽器綁在快的核心（0–3、12–15）；machopo 的 `perf` profile（ThinLTO，沒有 PGO），Chrome for Testing 152.0.7977.82（ThinLTO 加 PGO，headless）。
- **整體的比值**：`bench.py speedometer`，每個 suite 一個新的瀏覽器、10 個 iteration，每個 suite 總時間的中位數。
- **時間花在哪**：兩邊都用 `perf record` 取樣（`cycles:u`，每 100 萬個 cycle 一個樣本，frame pointer），20 個 suite 各 5 個 iteration，只留 runner 計時的區間裡 renderer main thread 的樣本。每個樣本照 stack 上最內層說得出種類的 frame 分：JS（V8 的 JIT 程式和 builtin）、DOM、style、文字、box layout、paint、GC、bindings、V8 API（machopo 或 Blink 呼叫的 V8 的 C++）、HTML parse、canvas；配置、`memcpy` 和標準函式庫的容器算給呼叫它的，取樣本身的成本（V8 的 perf map、GC 的紀錄、trace）不算。每個 suite 各類佔樣本的比例，乘上它沒有取樣時計時的中位數，換成 ms。
- **Chrome 那一側是估計**：Chrome for Testing 的 Linux 版沒有符號，取樣的是 Fedora 的 Chromium 156（有 debuginfo）。它比 Chrome for Testing 152 慢 12% 到 47%（幾何平均 23%，應該是沒有 Google 的 PGO），所以用它各類的比例乘上 152 的時間（`bench.py speedometer --sample --browser chrome`，「怎麼重做」）；2026-10-24 在 Windows 上用 Chrome 正式版合計各類的比例核對過，兩種估法在大多數類別只差幾 ms。

## 整體

Speedometer 3.1 的 20 個 suite，machopo 合計 992.3 ms、Chrome 668.9 ms；每個 suite 的比值的幾何平均 1.43。比值最大的是 Editor-TipTap（2.14）、Perf-Dashboard（1.86）、NewsSite-Nuxt（1.69）、TodoMVC-WebComponents（1.67）、React-Stockcharts-SVG（1.57）；最小的是 Charts-chartjs。

## 各類

每個 iteration 的 ms，20 個 suite 合計（Chrome 是上面的估計）；最後一欄是只有這一類追平 Chrome 時的幾何平均（取樣換算的是 1.48）：

| 種類 | machopo | Chrome | 差 | 只有這一類追平時 |
| --- | ---: | ---: | ---: | ---: |
| 文字 | 170 | 44 | +127 | 1.269 |
| style | 133 | 60 | +73 | 1.319 |
| JS | 309 | 244 | +65 | 1.328 |
| DOM | 112 | 71 | +40 | 1.384 |
| box layout | 88 | 57 | +31 | 1.387 |
| bindings | 32 | 6 | +26 | 1.393 |
| paint | 51 | 38 | +13 | 1.395 |
| GC | 28 | 18 | +10 | 1.418 |
| HTML parse | 16 | 15 | +1 | 1.428 |
| V8 API | 19 | 20 | −1 | 1.431 |
| 合計 | 992 | 669 | +323 | 1.48 |

canvas 不在表裡：Chrome headless 在 Linux 上的 canvas 2D 在 main thread 上 raster，machopo 的在另一條 thread 上，不是差距。Blink 的 binding 和 DOM 編在同一個 binary 裡，兩類的界線和 machopo 不完全對得上：兩類合計 machopo 144、Chrome 77 ms。machopo 的配置 inline 進了呼叫它的 DOM 的函式，一部分也算在 DOM。

## 每個 suite

差最多的幾個 suite 和它們差最多的類別（ms）：

| suite | machopo | Chrome | 差最多的 |
| --- | ---: | ---: | --- |
| Editor-TipTap | 96 | 45 | 文字 +48 |
| TodoMVC-jQuery | 118 | 77 | DOM +22、bindings +7、GC +7 |
| Perf-Dashboard | 83 | 44 | 文字 +19、JS +9、style +6 |
| NewsSite-Nuxt | 84 | 50 | style +12、box layout +11、文字 +9 |
| React-Stockcharts-SVG | 93 | 59 | 文字 +11、style +8、JS +8 |
| NewsSite-Next | 96 | 63 | style +13、文字 +11、box layout +10 |
| TodoMVC-WebComponents | 31 | 19 | 文字 +7、style +4、paint +1 |

其他 13 個 suite 各差 −1 到 12 ms，多半是 JS 和 style。

## 每一類最多的函式

- **文字**：machopo 多的有一大塊在選字型和退回，不在 shaping 本身：fontique 每次解析字型的名稱（`NameKey::from_str`）、`font_families::Stack::push_with_metric_compatible`、`select_font`，加上 parley 的 `shape_text`、`build_into_layout`、`InlineContent::new`、斷行時複製整個 `LineState`。Chrome 是 HarfBuzz 的 shaping、`ShapeResultView`、ICU 的斷行。
- **JS**：兩邊最多的是同一批 JS 函式和 IC 的 builtin，各慢 1.2 到 1.5 倍，沒有單一的慢路徑：V8 的 build 設定（machopo 用的 V8 沒有 PGO）和給 V8 的設定。
- **style**：selector 比對兩邊差不多；machopo 多的在 cascade（`style::Walk::cascade`）、`MatchedPropertiesCache::add`、`ComputedStyle::rare_mut` 和 `initial`。
- **DOM**：machopo 是插入時的 `link`、`attribute_value_by_id`、名稱的查找（`names::find_fixed`）、`intern_value`、parser 建元素（`create_html_element_with_attributes`）、`form_subtree_inserted`、`class_names`；Chrome 是 `Element::RemovedFrom`、`InsertedInto`、`AttributeChanged`、`EventDispatcher::Dispatch`。
- **bindings**：machopo 是每次呼叫的外殼（rusty_v8 的 `c_fn`、`V8Front::finish_kept`、`mutate`）、回傳字串的快取（`StringCache::insert`）、建 wrapper（`templates::new_wrapper`）。
- **GC**：machopo 剩下的是配置（inline 的 `alloc_at` 和 `HeapVector` 走的 `Inner::allocate`，各 2 到 3 ms）和 V8 自己的 scavenge、traced handle（兩邊差不多）。DOM 的 heap 的 scavenge 和 `SlotSet::insert` 不在了（renderer 的 heap 沒有 nursery），配置是 bump 加 header 的快路。Chrome 的 cppgc 配置也是 inline 的 bump，算在呼叫它的 DOM 裡。
- **box layout**：machopo 是 taffy 的 flexbox、`extend_overflow`、`BoxTree::lay_out_box`、`release`；Chrome 是 `BlockLayoutAlgorithm`、`FlexLayoutAlgorithm`、`PhysicalBoxFragment::Create`。

## blink_perf：哪一種操作慢

2026-11-07，`bench.py blink-perf --pages …` 的 26 頁，machopo 和 Chrome for Testing 152 同一次各跑一次（同一頁兩次之間約有一成的起伏）。22 頁有效頁的幾何平均：machopo 慢 1.55 倍。machopo 慢幾倍（ms 的頁 machopo ÷ Chrome、runs/s 的頁 Chrome ÷ machopo）：

| 頁 | machopo 慢幾倍 |
| --- | ---: |
| `css/CSSPropertySetterGetter` | 2.57 |
| `parser/query-selector-all-class` | 2.41 |
| `bindings/get-element-by-id` | 2.28 |
| `css/PseudoClassSelectors` | 2.20 |
| `bindings/get-elements-by-tag-name` | 1.96 |
| `layout/flexbox-row-nowrap` | 1.90 |
| `bindings/node-type`、`first-child` | 1.86、1.80 |
| `layout/line-layout-line-height`、`chapter-reflow-once` | 1.76、1.56 |
| `bindings/gc-forest`、`gc-mini-tree`、`gc-tree` | 1.71、1.25、1.13 |
| `bindings/id-setter`、`create-element`、`id-getter`、`get-attribute`、`set-attribute` | 1.64、1.58、1.56、1.53、1.40 |
| `events/EventsDispatching` | 1.23 |

比 Chrome 快的：`bindings/append-child`（0.27）、`insert-before`（0.33）、`dom/child-list-mutation-with-unrelated-observer`（0.46）、`parser/tiny-innerHTML`（0.48）、`dom/modify-element-id`（0.74）、`events/EventsDispatchingInV1ShadowTrees`（0.81）、`parser/innerHTML-setter`（0.96）。

剩下的 binding 頁差的多半是每次呼叫的固定成本：getter 在 V8 的地板之上 machopo 約多 17 cycle、Chrome 約 4.5（建 `V8Front` 和 `Call`、開操作、unwrap 的檢查和 `read_weak` 的 barrier）；`get-element-by-id` 還多了字串參數的轉換；`create-element` 多半是 V8 建、接上 wrapper 的機制，Chrome 也一樣。`CSSPropertySetterGetter` 是宣告集合的增刪（宣告的結構大，增刪時搬得多）和一次存取照名稱查三、四次屬性。

## 可以開 RFC 的方向

依估計的影響排。GC、bindings、JS 的設定、DOM 這四類做過一輪（不重編 V8：machopo 用的 V8 照現在的設定編，JS 剩下的是 V8 的 build）：

1. **文字**（+127 ms，TipTap 一個 suite +48）：選字型和退回每次重新解析名稱、比對 metric 相容的字型；斷行時複製整個 `LineState`。Blink 照 `FontDescription` 快取 `FontFallbackList`，shaping 的結果有 word cache（`ShapeCache`）。
2. **style 的 cascade**（+73 ms，NewsSite 兩個 suite 最多）：selector 比對追上了，cascade 和 `ComputedStyle` 的複製、初始化還慢。Chromium：`ComputedStyleBase` 的 `DataRef` 群組、`ComputedStyleBuilder`、`MatchedPropertiesCache` 命中時只複製不同的群組。
3. **box layout**（+31 ms，NewsSite 兩個 suite）：taffy 的 flexbox、overflow 的計算、box tree 的釋放。
4. **bindings**（+26 ms）：每次呼叫的固定成本（建 `V8Front` 和 `Call`、開操作、unwrap 的檢查、`read_weak` 的 barrier）；unwrap 照 Blink 的 `ToWrappableUnsafe` 要改 `bindings` 的 `unsafe` 論證。
5. **GC**（+10 ms）：剩下 V8 自己的 scavenge 和 traced handle、配置的快路本身；`HeapVector` 和它的 store 每次兩個物件（Blink 的屬性存在元素的陣列裡）。
6. **paint**（+13 ms）：還沒細看。

## 怎麼重做

1. `python3 scripts/bench.py speedometer`（整體的比值，也是換成 ms 用的兩邊的時間）。
2. `python3 scripts/bench.py speedometer --sample --iterations 5`（machopo 的樣本）和 `--sample --browser chrome --iterations 5`（Fedora 的 Chromium：`sudo dnf install chromium`、`sudo dnf debuginfo-install chromium`），同一天跑：第二個跑完寫出 `target/bench/speedometer/categories.md`，「各類」「每個 suite」「每一類最多的函式」照它改寫（`benchmarks.md`）。
3. `python3 scripts/bench.py blink-perf --pages …`（上面的頁）。
