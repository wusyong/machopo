# 和 Chrome 的效能差在哪

machopo 和 Chrome 的時間差在哪一種工作、哪幾個函式，以及可以開 RFC 的方向。這份是最近一次分析的快照（2026-11-10，Linux，兩邊同一天照同樣的方法取樣；GC 另外從兩邊的 trace 拆過）：重新量的時候整份照新的結果改寫、換日期；某個方向做完之後，重量那一部分、改寫那一節。怎麼量在 `benchmarks.md`；每次量到的原始數字記在量的那份 RFC 的 `progress.md`。

## 量法

- **環境**：Fedora 44（Linux 7.2），AMD Ryzen AI 9 HX 370（筆電，接電源），兩個瀏覽器綁在快的核心（0–3、12–15）；machopo 的 `perf` profile（ThinLTO，沒有 PGO），Chrome for Testing 152.0.7977.82（ThinLTO 加 PGO，headless）。
- **整體的比值**：`bench.py speedometer`，每個 suite 一個新的瀏覽器、10 個 iteration，每個 suite 總時間的中位數。
- **時間花在哪**：兩邊都用 `perf record` 取樣（`cycles:u`，每 100 萬個 cycle 一個樣本，frame pointer），20 個 suite 各 5 個 iteration，只留 runner 計時的區間裡 renderer main thread 的樣本。每個樣本照 stack 上最內層說得出種類的 frame 分：JS（V8 的 JIT 程式和 builtin）、DOM、style、文字、box layout、paint、GC、bindings、V8 API（machopo 或 Blink 呼叫的 V8 的 C++）、HTML parse、canvas；`memcpy` 和標準函式庫的容器算給呼叫它的，取樣本身的成本（V8 的 perf map、GC 的紀錄、trace）不算。每個 suite 各類佔樣本的比例，乘上它沒有取樣時計時的中位數，換成 ms。
- **Chrome 那一側是估計**：Chrome for Testing 的 Linux 版沒有符號，取樣的是 Fedora 的 Chromium 156（有 debuginfo）。它比 Chrome for Testing 152 慢（每個 suite 0.91 到 1.51 倍，幾何平均 1.23，應該是沒有 Google 的 PGO），所以用它各類的比例乘上 152 的時間（`bench.py speedometer --sample --browser chrome`，「怎麼重做」）；2026-10-24 在 Windows 上用 Chrome 正式版合計各類的比例核對過，兩種估法在大多數類別只差幾 ms。
- **GC 另外從 trace 拆**：`bench.py blink-perf --gc`，runner 計時的區間裡 main thread 和其他 thread 上的 GC，分成 V8 的和 DOM heap 的（`benchmarks.md`）。取樣只看 main thread 的 CPU，而且兩邊的配置算在不同的類（見「GC」）。

## 整體

Speedometer 3.1 的 20 個 suite，machopo 合計 961.5 ms、Chrome 667.7 ms；每個 suite 的比值的幾何平均 1.38。比值最大的是 Editor-TipTap（1.90）、NewsSite-Next（1.85）、Perf-Dashboard（1.66）、NewsSite-Nuxt（1.56）、TodoMVC-Lit-Complex-DOM（1.55）；最小的是 Charts-chartjs（0.94）和 TodoMVC-React-Redux（1.07）。

## 各類

每個 iteration 的 ms，20 個 suite 合計（Chrome 是上面的估計）；最後一欄是只有這一類追平 Chrome 時的幾何平均（現在是 1.384）：

| 種類 | machopo | Chrome | 差 | 只有這一類追平時 |
| --- | ---: | ---: | ---: | ---: |
| 文字 | 135 | 42 | +93 | 1.266 |
| style | 137 | 58 | +78 | 1.276 |
| JS | 312 | 248 | +63 | 1.295 |
| DOM | 111 | 73 | +39 | 1.338 |
| bindings | 33 | 6 | +27 | 1.345 |
| box layout | 81 | 54 | +27 | 1.352 |
| paint | 52 | 39 | +14 | 1.350 |
| GC | 28 | 16 | +12 | 1.373 |
| HTML parse | 17 | 15 | +2 | 1.382 |
| V8 API | 20 | 18 | +2 | 1.384 |
| 合計 | 962 | 668 | +294 | |

canvas 不在表裡：Chrome headless 在 Linux 上的 canvas 2D 在 main thread 上 raster（35 ms），machopo 的在另一條 thread 上（3 ms），不是差距。分不到類的（「other」）machopo 32、Chrome 62 ms。Blink 的 binding 和 DOM 編在同一個 binary 裡，兩類的界線和 machopo 不完全對得上：兩類合計 machopo 144、Chrome 79 ms。

## 每個 suite

差最多的幾個 suite 和它們差最多的類別（ms）：

| suite | machopo | Chrome | 差最多的 |
| --- | ---: | ---: | --- |
| NewsSite-Next | 118 | 64 | style +22、box layout +11、文字 +11 |
| Editor-TipTap | 85 | 45 | 文字 +39 |
| TodoMVC-jQuery | 114 | 80 | DOM +18、GC +9、bindings +6 |
| Perf-Dashboard | 75 | 45 | JS +10、文字 +9、style +6 |
| React-Stockcharts-SVG | 87 | 58 | JS +9、style +7、文字 +6 |
| NewsSite-Nuxt | 74 | 48 | style +12、box layout +7、文字 +6 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 38 | 26 | DOM +4、JS +3、style +2 |

其他 13 個 suite 各差 −4 到 10 ms，多半是 JS、style 和文字。NewsSite-Next 的 machopo 每個 iteration 不穩（95% 區間 89 到 120 ms）。

## 每一類最多的函式

- **文字**：machopo 最多的是把 shaping 的結果組成 layout（parley 的 `build_into_layout`，30 ms）、`InlineContent::new` 和 `fragments`、分析文字（`analyze_text`）、斷行（`BreakLines::break_next`）、走 glyph run（`GlyphRunIter`）、fontique 找字型家族（`Inner::family`）。Chrome 是 HarfBuzz 的 shaping（GPOS）、`ShapeResultView`、ICU 的斷行，每一項都在 3 ms 以內。
- **JS**：兩邊最多的是同一批 JS 函式和 regexp，machopo 各慢 1.2 到 1.5 倍，沒有單一的慢路徑：V8 的 build 設定（machopo 用的 V8 沒有 PGO）和給 V8 的設定。
- **style**：machopo 多的在走樹本身（`style::Walk::blocks` 23 ms、`children` 13 ms）、selector 比對（9 ms）、cascade（`Walk::cascade`、`cascade_declarations`）、`ComputedStyle::rare_mut`、`MatchedPropertiesCache::add`；Chrome 是收集規則（`ElementRuleCollector`）、`RecalcStyle`、`SelectorChecker`，各 3 ms 以內。
- **DOM**：machopo 是插入時的 `link`、`attribute_value_by_id`、名稱的查找（`names::find_fixed`）、`intern_value`、`form_subtree_inserted`、建節點（`Node` 的 `__init`、`Cx::node`）、`class_names`；Chrome 是 `Element::RemovedFrom`、`InsertedInto`、`ClassAttributeChanged`、`EventDispatcher::Dispatch`。
- **bindings**：machopo 是每次呼叫的外殼（rusty_v8 的 `c_fn`、`V8Front::finish_kept`、`V8Op::keep`）、回傳字串的快取（`StringCache::insert`）、參數的字串（`ArgString::text`）。建 wrapper（`templates::new_wrapper`，3.5 ms）算在 V8 API。
- **box layout**：machopo 是 `extend_overflow`、taffy 的 flexbox 和 block、`BoxTree::fragment`、`lay_out_box`、`release`；Chrome 是 `BlockLayoutAlgorithm`、`PhysicalBoxFragment::Create`、`FlexLayoutAlgorithm`。
- **paint**：machopo 是 `PrePaint::walk` 和 `release`、`Painter` 的 `contents`、`background`、`stacking_context`、`PaintController::paint`、main thread 的 scroll hit test 區域；Chrome 是 Skia 的 glyph bounds、`PaintPropertyTreeBuilder`。
- **HTML parse**：兩邊都是 fast path（`innerHTML`），machopo 的 `scan` 10 ms，Chrome 的 `ParseAttributes` 8 ms。

## GC

**回收那一側已經和 Chrome 持平或更好；取樣的那 12 ms 大半是配置。**

- **取樣的 28 對 16 ms 拆開**：machopo 有 9.7 ms 是 DOM heap 的配置（elise 的 `Inner::allocate`，也就是 `HeapVector` 的 store，3.2 ms；`alloc_at` 配屬性 2.0、文字節點 1.7），Chrome 的 cppgc 配置 inline 在 Blink 裡、算在 DOM。不算配置是 18.1 對 16.1 ms：DOM heap 自己 3.0 對 5.0（elise 的 sweep、barrier 和 `bindings` 的 wrap；Chrome 是 cppgc 和 Blink 的 GC），V8 的 scavenge 和 handle 9.4 對 5.9，V8 的 full GC 2.6 對 1.4，V8 的其他 3.0 對 3.7。
- **V8 的 scavenge 和 handle 多的**：兩邊年輕的 traced handle 差不多；machopo 重設被丟掉的 wrapper 的 root 的路比較長（V8 → fork 的 C++ handler → Rust 查那個型別的 JS 邊 → 再回 C++ 比對、重設，連同它呼叫的 2.35 對 0.76 ms；Blink 的 wrapper 在物件的固定位置），字串快取每個字串一個帶 finalizer 的 weak `v8::Global`（約 0.5 ms）。
- **從 trace 拆**（blink_perf 的 `bindings/gc-forest`、`gc-mini-tree`、`gc-tree`、`create-element` 和 `parser/innerHTML-setter`，runner 計時的區間）：
  - main thread 上的 GC machopo 都比 Chrome 少（三個 `gc-*` 頁 229 到 279 對 306 到 325 ms，`create-element` 1177 對 2974，innerHTML 429 對 2945）。
  - DOM heap 的總 CPU（main thread 加其他 thread）是 Oilpan 的 2% 到 36%：elise 的 marking 和 sweep 大多在它的 worker 上，量也比較少；Oilpan 的 sweep 有一大塊在 main thread 上，收尾的 `CppGC.SweepFinish` 是 Chrome 最長的停頓（innerHTML 82 ms，machopo 0.8 ms）。machopo 在 main thread 上的 DOM heap 時間大半是空的 `CppHeap`（V8 每一步照樣推它，5 到 80 ms）。
  - V8 那一側在三個 `gc-*` 頁 machopo 多 19 到 77 ms：這幾頁的區間不到 3 秒，Chrome 在 V8 的 loading 狀態（`Isolate::SetIsLoading`，最多 7 秒），用量超過上限一個 margin（`max(上限的一半, 32 MiB)`）才開始 marking，full GC 比較少次（`gc-forest` 62 對 24）；machopo 不呼叫它，跨過上限就開始。超過 7 秒的頁上兩邊開始的時機和次數一樣。代價是記憶體：Chrome 在這幾頁的高峰比較高。
  - `create-element` 上 machopo 在區間裡沒有 scavenge：full GC 的節奏和 Chrome 一樣，新生代還沒滿就被 full GC 清掉，V8 那一側因此比較少（1123 對 1698 ms）。

## blink_perf：哪一種操作慢

2026-11-10，`bench.py blink-perf --pages …` 的 26 頁，machopo 和 Chrome for Testing 152 同一次各跑一次（同一頁兩次之間約有一成的起伏）。26 頁都有效，不算可疑的 22 頁的幾何平均：machopo 慢 1.46 倍。machopo 慢幾倍（ms 的頁 machopo ÷ Chrome、runs/s 的頁 Chrome ÷ machopo）：

| 頁 | machopo 慢幾倍 |
| --- | ---: |
| `css/CSSPropertySetterGetter` | 2.51 |
| `bindings/get-element-by-id` | 2.20 |
| `css/PseudoClassSelectors` | 2.15 |
| `parser/query-selector-all-class` | 2.14 |
| `bindings/get-elements-by-tag-name` | 1.93 |
| `bindings/node-type`、`first-child` | 1.83、1.77 |
| `bindings/id-getter`、`id-setter` | 1.69、1.59 |
| `layout/line-layout-line-height`、`chapter-reflow-once`、`flexbox-row-nowrap` | 1.68、1.54、1.48 |
| `bindings/gc-forest`、`gc-mini-tree`、`gc-tree` | 1.58、1.19、1.01 |
| `bindings/get-attribute`、`create-element`、`set-attribute` | 1.49、1.43、1.40 |
| `events/EventsDispatching` | 1.24 |

比 Chrome 快的：`parser/innerHTML-setter`（0.65）、`dom/modify-element-id`（0.72）、`events/EventsDispatchingInV1ShadowTrees`（0.77）；快 2 倍以上、工具標成可疑的（兩邊做的事可能不同，不算進幾何平均）：`bindings/append-child`（0.27）、`insert-before`（0.33）、`parser/tiny-innerHTML`（0.39）、`dom/child-list-mutation-with-unrelated-observer`（0.45）。

三個 `gc-*` 頁整頁還慢，但 GC 本身 machopo 比較少（上一節）：慢的在建節點和 binding 的呼叫。剩下的 binding 頁差的多半是每次呼叫的固定成本：getter 在 V8 的地板之上 machopo 約多 17 cycle、Chrome 約 4.5（建 `V8Front` 和 `Call`、開操作、unwrap 的檢查和 `read_weak` 的 barrier）；`get-element-by-id` 還多了字串參數的轉換；`create-element` 多半是 V8 建、接上 wrapper 的機制，Chrome 也一樣。`CSSPropertySetterGetter` 是宣告集合的增刪（宣告的結構大，增刪時搬得多）和一次存取照名稱查三、四次屬性。

## 可以開 RFC 的方向

依估計的影響排。GC、bindings、JS 的設定、DOM 這四類做過一輪（不重編 V8：machopo 用的 V8 照現在的設定編，JS 剩下的是 V8 的 build）：

1. **文字**（+93 ms，TipTap 一個 suite +39）：shaping 的結果組成 layout、行內內容的建立和 fragment、文字的分析、斷行。Blink 的 shaping 結果有 word cache（`ShapeCache`），`ShapeResultView` 是段落結果的一段、不重組。
2. **style**（+78 ms，NewsSite 兩個 suite 最多）：走樹本身（`Walk::blocks`、`children`）最多，其次是 cascade 和 `ComputedStyle` 的複製、初始化。Chromium：`ComputedStyleBase` 的 `DataRef` 群組、`ComputedStyleBuilder`、`MatchedPropertiesCache` 命中時只複製不同的群組。
3. **DOM 和 bindings**（合計 +66 ms）：每次呼叫的固定成本（建 `V8Front` 和 `Call`、開操作、unwrap 的檢查、`read_weak` 的 barrier；unwrap 照 Blink 的 `ToWrappableUnsafe` 要改 `bindings` 的 `unsafe` 論證）；插入時的連結、屬性和名稱的查找。**DOM 的資料怎麼配置也在這裡**：取樣算在 GC 的 9.7 ms 是配置，`HeapVector` 和它的 store 每次兩個物件（Blink 的屬性存在元素的陣列裡），屬性和文字節點各一次 `alloc_at`；估能省 2 到 4 ms。
4. **box layout**（+27 ms，NewsSite 兩個 suite）：overflow 的計算、taffy 的 flexbox、fragment、box tree 的釋放。
5. **paint**（+14 ms）：pre-paint 的走樹和釋放、`Painter`。還沒細看。
6. **GC 剩下的**（合計約 2 到 3 ms，都要改 rusty_v8 的 fork 的 C++、重編 V8，量太小，動到那一段時順便做）：重設被丟掉的 wrapper 的 root 不經過 Rust（wrapper 的邊放在固定的位置，約 1.5 ms）；字串快取的 weak global（約 0.5 ms）；照 Blink 告訴 V8 頁面正在載入（Speedometer 上最多約 1 ms，短的頁 full GC 少一半，記憶體的高峰變高：為了行為和 Blink 一樣才做）；拿掉空的 `CppHeap` 的開銷（Speedometer 上量不到）。

## 怎麼重做

1. `python3 scripts/bench.py speedometer`（整體的比值，也是換成 ms 用的兩邊的時間）。
2. `python3 scripts/bench.py speedometer --sample --iterations 5`（machopo 的樣本）和 `--sample --browser chrome --iterations 5`（Fedora 的 Chromium：`sudo dnf install chromium`、`sudo dnf debuginfo-install chromium`），同一天跑：第二個跑完寫出 `target/bench/speedometer/categories.md`，「各類」「每個 suite」「每一類最多的函式」照它改寫（`benchmarks.md`）。crate 改名、加新的 crate 時先看 `scripts/bench/speedometer/categories.py` 的對照表認不認得：不認得的掉進「other」，那一類看起來就變少。
3. `python3 scripts/bench.py blink-perf --pages …`（上面的頁）。
4. `python3 scripts/bench.py blink-perf --gc --pages …`（「GC」那一節的五頁，加上 `bindings/insert-before.html`；`benchmarks.md`）。
