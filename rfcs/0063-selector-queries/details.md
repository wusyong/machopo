# RFC 0063 的細節：Speedometer 裡的查詢、基準頁、Blink 的 subtree Bloom filter

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 怎麼選到這裡

2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370），`main` 的 `4a09a2c0`（RFC 0062 做完）。`ci.py speedometer`、`--sample`、`--trace` 各一次。19 個 suite（Editor-TipTap 那次在 Chrome 那邊無效）的幾何平均，kun 和 Chrome 的比值 1.68。trace 的各階段，kun 比 Chrome 多花的（ms，一個 iteration，20 個 suite 合計）：script +247、box tree +210、canvas +110、style +63、layout +12、paint −50。

看過、沒選的候選（各自的比值改善是粗估）：

| 候選 | 差距在哪 | 估計 | 為什麼沒選 |
| --- | --- | ---: | --- |
| 文字的 shape 結果沿用 | box tree 的 +210 有 129 在 Editor-TipTap | — | TipTap 的 Long、Highlight 每一步只有一次大的重建（272 KB 的文字第一次排、全部加粗），Chrome 也得全部重新 shape；沿用只救得到 React-Stockcharts 的 SVG 文字，約 20 ms 以內 |
| 文字排版的吞吐量 | TipTap 的 shaping、斷行（`LineState` 的 clone 21 ms、`shape_text` 27、逐字查 charmap 15、harfrust 72） | 約 −1% | 只改善一個 suite，而且大多要改 parley 的 fork |
| canvas 的小項（顏色的快取、Skia 的 CPU 最佳化） | Charts-chartjs 設 `fillStyle` 約 17 ms；Skia 走 `sse2` 的 pipeline | 約 −1% | 小，可以單獨做 |
| canvas 只記錄、不在 main thread 上 raster | Charts-chartjs、Perf-Dashboard | 約 −5% | 要改 canvas 和 compositor 的架構 |
| 選擇器查詢（這份） | ES6-Webpack 的 `querySelector` 34 ms | 約 −2% | |

## Speedometer 裡的查詢

`--sample` 的樣本（`cycles:u`），stack 上有那個 API 的換算成 ms（乘上各 suite 量到的 kun 的 ms），19 個 suite：

| API | ms | 最多的 suite |
| --- | ---: | --- |
| `querySelector` | 44.6 | ES6-Webpack 33.9、NewsSite-Next 4.1、ES5 2.1、jQuery 1.3 |
| `querySelectorAll` | 16.2 | ES6-Webpack 7.1、ES5 4.6、Angular 1.2、Preact 1.1 |
| `getElementsBy*` 的 collection | 6.5 | jQuery 5.5 |
| `getElementById` | 2.0 | jQuery 1.7 |
| `matches`、`closest` | 0.4 | |

ES6-Webpack（`todomvc/vanilla-examples/javascript-es6-webpack-complex`）：`index.html` 703 KB，6642 個元素，`todoapp` 在第 702334 個 byte（最後面）。app 的 view 勾選和刪除項目時用 `document.querySelector(\`[data-id="${id}"]\`)` 找那一項，換篩選時用 `.filters .selected` 和 `.filters [href="#/…"]`。每次都從 document 走到 app。

## 基準頁

那個 suite 的 `index.html`（去掉 script）設進 `document.body`，`.todo-list` 放 100 個 ES5 的 template 產生的項目，共 7137 個元素、8534 個節點。每項先跑 20 次熱身，再跑 300 次取平均；kun（`perf` profile）和 Chrome for Testing 152.0.7977.82 輪流各開五次，取中位數。ms 一次：

| 查詢 | kun | Chrome | 倍數 |
| --- | ---: | ---: | ---: |
| `querySelector('[data-id="99"]')`（最後一個項目） | 0.157 | 0.001 | 157 |
| `querySelector('[data-id="nope"]')`（沒有） | 0.159 | 0.002 | 95 |
| `querySelector('.filters .selected')` | 0.119 | 0.013 | 9 |
| `querySelectorAll('div').length` | 0.156 | 0.030 | 5 |
| `new DOMParser().parseFromString(100 個項目)` | 0.304 | 0.160 | 1.9 |
| 同上，再 `list.replaceChildren(...body.childNodes)` | 0.479 | 0.234 | 2.0 |

kun 一個元素約 22 ns（0.157 ms / 7137）。Chrome 的前兩項平均一個元素不到 0.3 ns，是剪掉了子樹。

## kun 每個元素的成本

ES6-Webpack 的 `query_selector` 底下（`instructions:u` 的樣本）：自己 70%（`descendants` 的走訪和 `with_matcher` 的 closure 內聯在一起），`matches_complex_selector_internal` 27%（其中 `attr_matches` 17%），parse 0.2%。

`perf annotate` 的 `cycles:u`：自己的樣本有 59–63% 落在同一條指令，緊接在一個 byte 的寫入（`movb $-1, -0x40(%rbp)`）和同一個位址的八個 byte 的讀取（`movq -0x40(%rbp), %rax`）之後，像是 store forwarding 失敗：每個元素建一次 `StyleElement`（`NodeRef`、`Option<NodeId>`、`Option<&DomReadScope>`、`bool`、`Option<&FormStatesIn>`），再整個複製。`NodeRef::new` 每次也讀一次節點的 canary。只是推測，第 2 段開始前要確認。

## 第 1 段之後

2026-10-20，`0063` 的第 1 段（`4893f4b9`），`perf` build。

大範圍：基準頁那一頁，`querySelector('[data-id="99"]')`、`querySelectorAll('div').length` 各跑 3000 次，`perf record` 的 `cycles:u`，那個 API 底下的樣本：

| | `[data-id="99"]` | `div` |
| --- | ---: | ---: |
| 走訪（`SelectorQuery::query` 自己，`find` 內聯在裡面） | 59% | 42% |
| 快速比對（`has_compound`：逐字比 attribute 的名稱和值，不分大小寫） | 37% | 37% |
| `with_matcher` 自己（內聯在裡面的候選的完整比對；建 context 幾乎不花） | 0% | 11% |
| 結果的 `NodeList`、handle | 1% | 5% |
| parse | 1.2% | 0.9% |
| 完整比對 | 0.2% | 0.9% |

`[data-id="99"]` 在 `.todo-list` 的 100 個 `li` 上剪不掉（每個都有 `data-id`，值不同），每個都快速比對一次。走訪的樣本分散，最多的一條指令在讀節點的 filter 之後（17%），像是讀節點時的 cache miss；第 1 段之前看到的 store forwarding 失敗（`details.md` 的「kun 每個元素的成本」）在新的走訪裡沒有。

小範圍：RFC 0062 的基準頁的清單（100 個項目），每個項目上各呼叫一次，kun 和 Chrome 輪流各開三次，中位數，µs 一次：

| 呼叫 | kun | Chrome | 倍數 | parse | `with_matcher` 裡的完整比對 | 其他（走訪、比對、handle） |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `li.querySelector('.toggle')` | 0.66 | 0.06 | 11 | 59% | 19% | 21% |
| `li.querySelector('input.toggle[type=checkbox]')` | 1.18 | 0.22 | 5.3 | | | |
| `li.matches('.completed')` | 0.50 | 0.05 | 9.4 | 72% | 11% | 17% |
| `li.closest('ul')` | 0.48 | 0.06 | 8.1 | 64% | — | 33% |

（百分比是 kun 各跑 3000 次的 `cycles:u` 樣本。第二欄一開始誤標成「建 context」：分類時把 `with_matcher` 底下 parse 以外的都算進去，其實是內聯在它裡面的完整比對。）

Speedometer（RFC 0062 做完時的取樣，`4a09a2c0`）：查詢的 API 合計約 61 ms，parse 約 1.8 ms；jQuery 的查詢時間有 21% 在 parse、Perf-Dashboard 36%（小範圍的多），ES6-Webpack 0.7%（大範圍）。

## 完成時

2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370）。

`ci.py speedometer --against main`：`main`（`74fc9861`，RFC 0062 做完）和 `0063`（`e0791acc`）的 kun 輪流各三次，10 個 iteration，各 suite 的中位數，ms：

| Suite | `main` | `0063` | 變化 | 各次（`main`；`0063`） |
| --- | ---: | ---: | ---: | --- |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 101.9 | 81.9 | −19.6% | 101.3、107.0、117.9；89.0、76.8、79.8 |
| TodoMVC-JavaScript-ES5 | 76.4 | 71.4 | −6.6% | 76.0、77.0、76.2；72.8、67.1、54.2 |
| TodoMVC-jQuery | 204.0 | 237.1 | +16.2% | 204.2、207.5、196.5；266.8、200.9、276.4 |
| TodoMVC-WebComponents | 37.1 | 53.1 | +43.3% | 37.0、37.9、36.2；40.3、53.2、53.2 |

20 個 suite 的幾何平均 −6.0%；其他 suite 的變化有正有負（Lit −25.5%、React-Redux −22.9%、TipTap −32.3%、Vue +3.7%……），各次之間差得和版本之間一樣多。jQuery、WebComponents 再單獨輪流各五次：`main` 的 jQuery 是 208.2、198.0、281.2、280.1、278.8，`0063` 200.2、209.9、220.8、245.7、210.7（中位數 −23.2%）；WebComponents `main` 43.8、46.4、52.5、53.7、38.2，`0063` 38.1、38.4、52.6、52.8、36.7（−12.9%）。兩個版本都在兩個水準之間跳，那兩個 suite 的變化是這台機器的雜訊，不是這份的效果（jQuery 幾乎不用 `querySelector`，innerHTML 每個節點只多 0.7% 的指令）。

只有 ES6-Webpack 每次都低：它的 `querySelector` 原本約 34 ms（RFC 開始時的取樣）。

`ci.py speedometer`（kun 和 Chrome 各一次，`e0791acc`）：ES6-Webpack 82.0 對 33.0 ms，和 Chrome 的倍數 3.97（`4a09a2c0` 時）→ 2.48。同一次其他 suite 和上一次的差（jQuery +36%、Perf-Dashboard +51%、Charts-chartjs −35%）也是雜訊，19 個 suite 的幾何平均和 Chrome 的比值 1.68 → 1.78 沒有意義。

## Blink 的 subtree Bloom filter

2026-10-20 讀 chromium/src 的 `main`（`third_party/blink/renderer/core/`）：

- `dom/element.h`：`using TinyBloomFilter = uint32_t;`、`FilterForString`（atom 的 hash 取兩段五個 bit，k=2）、`CouldMatchFilter(filter)`（`(attribute_or_class_bloom_ & filter) == filter`）。
- `dom/qualified_name.cc`：attribute 名稱的 filter 用 `LocalNameUpper()`，大小寫不同的名稱設同樣的 bit。
- `dom/element.cc`：
  - `AppendAttributeInternal`、`ClassAttributeChanged`（每個 class 名稱）、建立時的 attribute：設自己的 bit，`UpdateSubtreeBloomFilterAfterInsert`：沿 `parentElement()` 往上 OR，直到 parent 已經有全部的 bit。
  - `InsertedInto`：`UpdateSubtreeBloomFilterAfterInsert`。
  - `RemovedFrom`：parent 的 `UpdateSubtreeBloomFilterAfterChildRemoval`：數子節點，兩個以上的子元素或超過 32 個非元素的子節點就放棄；剩零或一個子元素時，用那一個的 filter 加自己的重算，往上只要 parent 也只有這一個子元素就繼續。
  - 刪 attribute 不重算（`TODO(sesse)`：擔心刪光 attribute 時的 O(n²)）。
  - `VerifyBloomFilterTreeConsistency`：DCHECK 每個元素的 filter 包含在 parent 的裡。
- `css/selector_query.cc`、`.h`：每個 compound 一個 `selector_filter`（`SelectorFilter::CollectSingleSelectorIdentifierHashes`，往下還要符合的 compound 的也併進來）；`ExecuteSearchSingleCompound` 對 `CouldMatchFilter` 不過的元素整棵跳過；只支援 id、class、tag、完全相等的 attribute 和 `:nth-child()` 的快速比對，其他的照樣交給 `SelectorChecker`；`>` 放寬成後代、`+` 放寬成 `~` 時之後再完整比對一次。

## 估計

ES6-Webpack 的 `querySelector` 的 34 ms 幾乎都是走過前面的 6600 個元素，剪掉之後剩找到 app 那一段的比對；`querySelectorAll` 的 7 ms 看它的選擇器能剪多少。假設省 35 ms：116 → 約 80 ms，和 Chrome 的倍數 3.96 → 約 2.75；NewsSite-Next 約省 4 ms。19 個 suite 的幾何平均：(2.75 / 3.96)^(1/19) ≈ 0.981，約 −2%。
