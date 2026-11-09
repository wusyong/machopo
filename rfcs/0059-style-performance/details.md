# RFC 0059 的細節：style 的效能

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 量測

2026-10-19，macOS 26.6.2，Apple M4（4 個效能核心、6 個節能核心），接電源；kun 是 `main` 的 `a80a22dc`（`perf` profile），Chrome for Testing 152.0.7977.82。每個數字只量一次。

### 方法

- **trace**：`python3 scripts/ci.py speedometer --trace --iterations 3`（20 個 default suite，kun 和 Chrome 各一份）。分析用暫時的腳本，照 RFC 0058 的 `details.md` 的「量測」：取記著 runner 的 User Timing mark 的那一條 track（Chrome 有好幾條叫 `CrRendererMain` 的 thread，各是一個 renderer process），照 `speedometer.py` 的 `mark_windows` 找計時區間，算每個 slice 名稱在區間裡的 self time，除以 3 是每個 iteration 的。
  - **style**：kun 的 `Document::UpdateStyle`；Chrome 的 `Document::recalcStyle`、`Document::updateStyle`。
  - **建 box tree**：kun 的 `StyleEngine::RebuildLayoutTree`（`layout` 的 `BoxTree::update`，裡面也做文字的 shaping）；Chrome 的 `Document::rebuildLayoutTree`。
  - **layout**：kun 的 `Layout`；Chrome 的 `LocalFrameView::performLayout`、`Layout` 和 `InlineNode::ShapeTextIncludingFirstLine`（Chrome 的 shaping 在 layout 裡）。
- **取樣**：`python3 scripts/ci.py speedometer --sample --iterations 3 --suites NewsSite-Next,React-Stockcharts-SVG,TodoMVC-jQuery,TodoMVC-WebComponents,Editor-TipTap`（samply，1000 Hz，frame pointer；每個樣本是兩個樣本之間用掉的 CPU 時間）。從 folded stack 挑出 `StyleEngine::recalc_style` 底下的樣本（style）和 `BoxTree::update` 底下的（建 box tree），照最內層的函式和往下一層的函式分。
- **重算的元素數**：整次執行（含載入頁面，3 個 iteration）的總數。Chrome 加總 trace 裡每個 `elementCount`（`UpdateLayoutTree` 的參數）；kun 用 `RUST_LOG=info,style=debug python3 scripts/ci.py speedometer --browser kun --iterations 3 --suites <suite>`，加總 `kun.log` 裡每次 `recalc style` 的 `restyled`。kun 的 log 和 trace 的時鐘不同，沒有對齊計時區間，所以兩邊都比整次執行。
- **整份重算的原因**：暫時在 `collect_document_sheets` 加一行 debug log，印出 stylesheet 算成「變了」的原因，跑 TodoMVC-Preact-Complex-DOM，量完還原。
- **`ComputedStyle` 的大小**：暫時的單元測試印 `size_of::<ComputedStyle>()`，量完還原。

### 各階段（每個 iteration 的 ms，20 個 suite 的合計）

計時區間：kun 1181 ms，Chrome 625 ms。

| | kun | Chrome | kun 多的 |
| --- | ---: | ---: | ---: |
| script（`v8.callFunction`、`FunctionCall`、`RunMicrotasks`、`EventDispatch` 等和 GC 以外的 `V8.*`） | 604 | 305 | 299 |
| style | 151 | 34 | 117 |
| 建 box tree | 92 | 6 | 86 |
| layout | 92 | 114 | −22 |
| parse（`ParseHTML`、`HTMLDocumentParser::*`） | 95 | 74 | 21 |
| GC | 9 | 15 | −6 |

### 各 suite（每個 iteration 的 ms）

| suite | style：kun | Chrome | 差 | 建 box tree：kun | Chrome | 差 | layout：kun | Chrome |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Editor-TipTap | 1.6 | 0.5 | 1.1 | 51.4 | 0.1 | 51.3 | 34.0 | 22.6 |
| NewsSite-Next | 21.2 | 4.9 | 16.3 | 7.0 | 1.1 | 5.8 | 21.5 | 8.9 |
| NewsSite-Nuxt | 17.5 | 4.5 | 13.1 | 6.0 | 1.0 | 5.1 | 20.2 | 8.2 |
| React-Stockcharts-SVG | 15.1 | 3.4 | 11.6 | 5.9 | 0.3 | 5.6 | 2.1 | 1.8 |
| TodoMVC-jQuery | 11.4 | 1.2 | 10.2 | 1.7 | 0.4 | 1.3 | 0.7 | 7.0 |
| TodoMVC-WebComponents | 9.4 | 1.3 | 8.2 | 2.7 | 0.3 | 2.4 | 0.9 | 6.9 |
| Charts-observable-plot | 9.6 | 2.1 | 7.5 | 1.0 | 0.4 | 0.6 | 0.5 | 1.5 |
| TodoMVC-Lit-Complex-DOM | 8.4 | 1.6 | 6.8 | 1.0 | 0.2 | 0.8 | 0.6 | 6.7 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 7.3 | 1.4 | 5.9 | 1.3 | 0.2 | 1.2 | 0.9 | 0.9 |
| TodoMVC-Backbone | 6.4 | 1.1 | 5.4 | 1.4 | 0.3 | 1.1 | 0.6 | 1.1 |
| TodoMVC-Angular-Complex-DOM | 6.8 | 1.5 | 5.3 | 1.3 | 0.2 | 1.1 | 0.9 | 6.9 |
| TodoMVC-React-Complex-DOM | 6.3 | 1.5 | 4.8 | 1.3 | 0.1 | 1.1 | 0.9 | 6.7 |
| TodoMVC-Preact-Complex-DOM | 6.1 | 1.4 | 4.7 | 1.2 | 0.1 | 1.1 | 0.8 | 6.7 |
| Perf-Dashboard | 5.7 | 1.2 | 4.5 | 2.9 | 0.5 | 2.3 | 2.3 | 4.7 |
| TodoMVC-Svelte-Complex-DOM | 5.9 | 1.5 | 4.4 | 1.2 | 0.1 | 1.1 | 0.8 | 6.5 |
| TodoMVC-Vue | 5.1 | 1.6 | 3.5 | 1.6 | 0.2 | 1.3 | 0.9 | 7.1 |
| TodoMVC-JavaScript-ES5 | 3.0 | 1.0 | 2.0 | 1.1 | 0.2 | 0.9 | 0.5 | 1.4 |
| TodoMVC-React-Redux | 2.6 | 1.1 | 1.5 | 1.0 | 0.1 | 0.8 | 0.5 | 6.9 |
| Editor-CodeMirror | 1.2 | 0.5 | 0.7 | 1.1 | 0.1 | 1.0 | 2.1 | 1.7 |
| Charts-chartjs | 0.1 | 0.1 | −0.1 | 0.2 | 0.0 | 0.2 | 0.1 | 0.2 |

style 的差距分散在每個 suite，kun 是 Chrome 的 3 到 5 倍。建 box tree 的差距一半以上是 Editor-TipTap。

### 重算的元素數（整次執行，3 個 iteration）

| suite | Chrome 的 `elementCount` | kun 的 `restyled` | 倍數 | kun 整份重算的次數 |
| --- | ---: | ---: | ---: | ---: |
| NewsSite-Next | 19686 | 19599 | 1.0 | 18 |
| React-Stockcharts-SVG | 14656 | 16881 | 1.15 | 12 |
| TodoMVC-jQuery | 4596 | 6705 | 1.46 | 26 |
| TodoMVC-WebComponents | 4557 | 6255 | 1.37 | 24 |
| TodoMVC-Preact-Complex-DOM | 17880 | 78044 | 4.4 | 24 |

NewsSite 重算的元素數和 Chrome 一樣，style 的時間卻是 4 倍：每個元素比較貴。Preact 多重算了 4.4 倍：每個 iteration 約 8 次整份重算，Complex-DOM 的頁面很大（每次約 3200 個元素）。

**Preact 的 24 次整份重算的原因**（`collect_document_sheets` 暫時的 log）：

| 原因 | 次數 |
| --- | ---: |
| 新的 document 第一次算 style（`first`、`other_document`） | 14 |
| `<link>` 的 stylesheet 到了（`linked_changed`，收集到的文字和上次不同） | 7 |
| DOM 標了 stylesheet 可能變了（`marked`：`<style>` 改了） | 3 |

觸發點見下一節。

### 整份重算的觸發點

第 1 段查的（2026-10-19）：在 `collect_document_sheets` 算成「變了」的地方加暫時的 log，記 document 的 URL、已經收集到的 stylesheet 數和 backtrace，跑 TodoMVC-jQuery 和 TodoMVC-Preact-Complex-DOM 各 3 個 iteration（`RUST_LOG=info,style=debug`），查完拿掉；原因的那一行 log 留著（README 第 9 條）。

| | TodoMVC-jQuery | TodoMVC-Preact-Complex-DOM |
| --- | ---: | ---: |
| 新的 document 第一次算 style | 14 | 14 |
| `<link>` 的 stylesheet 到了 | 10 | 7 |
| `<style>` 改了 | 0 | 3 |
| media 變了 | 2 | 0 |

**`<link>` 到了的那幾次**：backtrace 全部在 `FrameView::start_loads`，不是 script 讀幾何。`start_loads`（找新插入的 `<link>`、script、圖片要載入的東西）每次都先呼叫 `update_own_style` 把 style 算到最新，再拿 `take_background_image_urls` 找要載入的背景圖片；它不看 render-blocking 的 stylesheet 到了沒有。jQuery 的頁面有 3 份 `<link>` 的 stylesheet，每個 iteration 在已經有 0、1、2、3 份時各整份算一次（4 次，Chrome 是 1 次）；Preact 的頁面在 `<style>` 之外還有 `<link>` 的 stylesheet，每到一份就整份算一次，每次約 3200 個元素。Blink 的背景圖片是在 lifecycle 的 style recalc 裡發現、交給 `ElementStyleResources` 載入的，render-blocking 的 stylesheet 沒到之前不做 lifecycle 的更新，所以只算一次。

**新 document 的第一次**：每個 iteration 約 5 次，大多是便宜的 document：還沒有 URL 的 frame（每個 iteration 2 次）、runner 的頁面（hash 變成 `#running` 時）、`about:blank`，加上 suite 的頁面本身（那次還沒有任何 stylesheet）。suite 的頁面這一次省不掉；其他的元素很少，不是時間的來源。

### 4b 前剩下的整份重算

第 4a 段之後（2026-10-19）：每個 default suite 單獨用 `RUST_LOG=info,style=debug python3 scripts/ci.py speedometer --browser kun --iterations 3 --suites <suite>` 跑，統計 `style sheets changed` 的原因。新 document 的第一次 8 到 14 次（每個 iteration 載入的新頁面，元素很少）；`<link>` 到了或 `<style>` 改了，多數 suite 每個 iteration 約 1 次；media 變了每個 iteration 約 1 次；Charts-observable-plot 的 `<style>` 改了 12 次（每張圖插一份 `<style>`）。

TodoMVC-Svelte-Complex-DOM 每個 iteration：suite 的頁面 commit 之後 parse 出 `<style>`，整份重算一次（約 6,600 個元素），這時它的兩個 `<link>` 還沒到；`<link>` 到了又整份重算一次（約 6,650 個）；app 第一次 render 時再一次約 6,650 個的子樹重算（不是整份）。整次執行 3 個 iteration 重算 64,549 個元素。暫時加 backtrace 的 log 查第一次是誰觸發的：`dom_api::media_query_list::environment`，media query 的判斷要知道子 frame 的大小，子 frame 還沒排版就先把它排版（`geometry::with_layout`），連帶用還不齊的 stylesheet 算了 style。Chrome 在 stylesheet 到齊前不算，這兩次只會有一次。

**K1a 之後**（同一天，同樣的跑法）：只在子 frame render-blocked 時改成只排版父 frame，Svelte 3 個 iteration 共重算 64,549 → 57,714 個元素、React-Complex-DOM 44,670 → 37,794，Preact 51,254 → 57,894（多一次），ES6-Webpack、Angular 沒變。再加 backtrace 的 log 看：多數 iteration 的 `matchMedia` 發生在子 frame 的 `<link>` 接上之後、render thread 的 `start_loads` 開始載入之前，那時還不算 render-blocked，照舊排版了子 frame。把「已經接上、還沒開始載入的 `<link>`」也算成在等之後，`matchMedia` 那次沒了，但 Svelte 每個 iteration 還是兩次整份重算（6,651 和 6,656 個）：stylesheet 到了之後 `FrameView::start_loads` 為了找背景圖片算了子 frame 的 style，那時子 frame 沒排版過、環境是 0×0；接著排版（這裡是 focus 的判斷要 layout）拿到 iframe 的大小，media 變了，又整份重算。以前是 `matchMedia` 先把子 frame 排版了，`start_loads` 那次的環境已經對了，所以次數一樣，只是換了位置。

**L1 之後**（同一天，同樣的跑法）：render thread 開始載入時，先把還沒排版、這次會算 style 的子 frame 的大小給它（第 33 條）。3 個 iteration 重算的元素：

| suite | 4a 之後 | K1a 之後 | L1 之後 |
| --- | ---: | ---: | ---: |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 44,757 | 44,757 | 31,126 |
| TodoMVC-React-Complex-DOM | 44,670 | 37,794 | 24,254 |
| TodoMVC-Angular-Complex-DOM | 44,937 | 44,937 | 24,521 |
| TodoMVC-Preact-Complex-DOM | 51,254 | 57,894 | 30,837 |
| TodoMVC-Svelte-Complex-DOM | 64,549 | 57,714 | 44,132 |
| TodoMVC-Lit-Complex-DOM | 26,432 | 26,420 | 26,420 |

其他 suite 差不到 1%。Complex-DOM 的 TodoMVC 每個 iteration 剩一次 stylesheet 造成的整份重算（`<link>` 到了，和 Chrome 一樣一次）；Svelte 另外還有 app 第一次 render 時約 6,650 個元素的子樹重算。Lit 每個 iteration 多一次 `<style>` 改了的整份重算。

暫時加 log 看 stylesheet 的增減：Lit 那一次是 inline SVG 圖示各自帶的 `<style>`（`.Smock_TaskList_18_N_svg__fill{fill:#464646}` 這類，一個圖示一份，文字相同的有幾十份）；`<link>` 到的那一次多了 299KB 的 Spectrum CSS，裡面有 `:root`、`*` 這類沒有特徵的規則，照 Blink 的做法也是整份重算。

**4c 之後**（同一天，同樣的跑法）：多數 suite 和 L1 之後一樣或少幾個到幾百個（observable-plot 15,485 → 15,281、Stockcharts 16,802 → 16,739）。Lit 的 3 次 `<style>` 改動變成只重算部分（`everything=false`），但總數只少 48 個（26,420 → 26,372）：那幾次和 app render 的子樹重算是同一次重算，元素本來就要重算，要省得靠 4e。Preact-Complex-DOM 反而多了一次約 6,640 個的子樹重算（30,837 → 37,477，重跑兩次一樣）：它的 stylesheet 改動全部是整份重算，4c 的路沒有走到；iteration 跑 1 次時 `<link>` 到了和 app 第一次 render 合成一次重算，跑 3 次時前兩個 iteration 分成兩次。把 4c 暫時關掉（每次改動都整份重算）的 build 是 30,837，所以是時機的差別（多半是收集 stylesheet 時多做的比對讓時機移了一點），兩種結果都正確。

**4e 之後**（同一天，同樣的跑法）：ES6-Webpack-Complex-DOM 31,117 → 24,154、Preact-Complex-DOM 37,477 → 23,989、Vue 7,127 → 5,418、Editor-TipTap 3,939 → 2,903、ES5 4,460 → 3,652、React-Redux 4,310 → 3,502，其他多半少約 200 個（runner 的頁面上 `html` 的 `data-visible-section` 改了不再重算整份）。Svelte-Complex-DOM 44,132 → 43,924：`ul.todo-list` 的 class 改了之後，set 只重算 200 個 `li`，但 `li` 的 style 變了、往下繼承，後代照樣重算。

### 5a 前：matched properties cache 的命中

第 4 段之後（2026-10-19）：暫時加計數，每個 default suite 跑 1 個 iteration，數 `cascade_blocks` 的次數、能用快取的、命中的，和命中後依位置的調整（`adjust`）一個欄位都沒改的。

| suite | cascade | 命中 | 命中且調整不改 |
| --- | ---: | ---: | ---: |
| TodoMVC-Svelte-Complex-DOM | 18,626 | 16,299（88%） | 5,791 |
| TodoMVC-ES6-Webpack-Complex-DOM | 10,509 | 8,927（85%） | 3,080 |
| TodoMVC-Lit-Complex-DOM | 11,185 | 7,917（71%） | 2,868 |
| NewsSite-Next | 19,506 | 13,997（72%） | 5,084 |
| NewsSite-Nuxt | 19,673 | 14,621（74%） | 4,792 |
| TodoMVC-jQuery | 2,970 | 2,121（71%） | 364 |
| TodoMVC-WebComponents | 5,532 | 3,317（60%） | 1,861 |
| Perf-Dashboard | 3,862 | 2,560（66%） | 908 |
| Charts-observable-plot | 5,540 | 391（7%） | 248 |
| React-Stockcharts-SVG | 6,026 | 489（8%） | 351 |

其他的 TodoMVC 命中 70% 到 85%。命中後調整改了哪個欄位（Svelte / NewsSite-Next / jQuery）：`original_display` 7,508 / 4,824 / 1,735（調整一律把它設成 blockification 前的 `display`，快取存的是調整前的值，和位置無關），`display`（flex、grid 的子元素等的 blockification）4,712 / 4,088 / 227，`effective_z_index` 1,528 / 334 / 0，`effective_appearance` 24 / 0 / 4，其他欄位都沒有。

### 5b 前：`ComputedStyle` 的欄位

5a 之後（2026-10-19）：`ComputedStyle` 105 個欄位、1,472 bytes。大的是 `svg`（`SvgStyle`）216、`animations` 128、`border_radius` 96、`transitions` 80、`margin`、`inset`、`padding` 各 48、grid 的 11 個欄位共 200、背景的 8 個清單共 128（每個 `Arc<[…]>` 16）、counter 三個共 48、`content` 和 `list_style_type` 各 24；其餘 80 多個欄位 1 到 12 bytes。

暫時加計數，每個 default suite 跑 1 個 iteration，每次 cascade 一個元素就看每個欄位：非繼承的是不是初始值，繼承的是不是和 parent 不同（`svg` 的子欄位分開看），另外看幾組裡任何一個不同的。共 104,040 次 cascade，整體的比例（括號裡是最高的 suite）：

| 欄位或組 | bytes | 不同的比例 |
| --- | ---: | ---: |
| `animations` | 128 | 0.04%（Perf-Dashboard 1.2%） |
| grid 的 11 個 | 200 | 0.87%（Charts-chartjs 1.8%） |
| `border_radius` | 96 | 2.5%（Perf-Dashboard 6.7%） |
| `box_shadow` | 16 | 1.9%（jQuery 16%、Backbone 16%） |
| `transform` | 16 | 0.96%（observable-plot 13%） |
| outline 的 4 個 | 14 | 0.03% 以下 |
| counter 三個和 `content` | 72 | 2.8%（NewsSite-Next 4.7%） |
| 背景的 8 個清單 | 128 | 3.0%（WebComponents 17%，NewsSite 0%） |
| `transitions` | 80 | 13%（NewsSite-Next 23%，Charts、Editor 0%） |
| `svg` 整個 | 216 | 30%（HTML 為主的 TodoMVC 6% 到 13%，Complex-DOM 35%，Stockcharts 86%） |
| `svg.fill`（繼承，和 parent 比） | | 25%（Complex-DOM 34%：Spectrum 的圖示） |
| flex 的 14 個 | 約 60 | 11%（NewsSite-Nuxt 36%） |
| `margin`、`padding`、`inset` | 各 48 | 13% 到 19% |
| `width`、`height` | 各 12 | 25% 到 30% |
| `display` | 1 | 68% |

### 第 6 段前：style 的時間花在哪

5b 之後（2026-10-19），`--sample --against ef035a3e` 的 `current` 那 3 次的取樣，`recalc_style` 底下（占 style 的比例）：

| | NewsSite-Next | React-Stockcharts-SVG | Charts-observable-plot | Svelte-Complex-DOM |
| --- | ---: | ---: | ---: | ---: |
| style 占整個 CPU | 18.9% | 14.0% | 19.8% | 15.6% |
| `Recascade::cascade`（套用宣告） | 37.9% | 13.0% | 31.6% | 13.0% |
| selector 比對（`matches_complex_selector_internal`） | 18.0% | 12.7% | 6.2% | 42.8% |
| 其中 `attr_matches`（屬性選擇器） | 不到 1% | 0.8% | | 16.9% |
| `cascade` 自己（找候選的規則等） | 11.4% | 16.2% | 15.1% | 18.9% |
| 複製 `ComputedStyle`（`clone`、`memmove`、`inherit`） | 10.6% | 9.7% | 13.5% | 幾乎 0 |
| `AncestorFeatures` 的 `uses`、`touched_by` | | 8.5% | 5.5% | |
| SVG 的 presentation attribute（`parse_d_attribute`、`svg_attribute_declarations`） | | 4.9% | 7.9% | |
| `flat_tree_parent` | 2.2% | | | 9.2% |

`Recascade::cascade` 底下分散：`cascade_blocks` 的 closure 4% 到 6%、`cascade_start` 2% 到 3%、`apply` 2% 到 4%、舊 style 的 `Arc` 釋放 2.7%（NewsSite）、自訂屬性的 `resolve` 2.4%（NewsSite），沒有單一的大頭。`attr_matches` 逐一比對元素的每個屬性的名字（字串比較）；`AncestorFeatures` 每次查都把名字轉小寫（配置一個字串）。

### style 的取樣

4 個 suite（NewsSite-Next、React-Stockcharts-SVG、TodoMVC-jQuery、TodoMVC-WebComponents）的 style 共 163.7 ms CPU（3 個 iteration），佔它們全部 CPU 的 13%：NewsSite-Next 63.2、Stockcharts 40.5、jQuery 31.7、WebComponents 28.3 ms。

**`recalc_style` 往下一層**：`style::cascade` 75.0%，`recalc_style` 自己 16.2%（這次的 build 把一些函式 inline 進去；第 1 段用現在的 build 重新取樣只有 4.2%，見下面），`AncestorFeatures` 的 `uses` 和 `touched_by` 3.2%，`ComputedStyle` 的 `eq` 1.2%，`scope_dependencies` 1.2%。

**`style::cascade` 往下一層**（佔 cascade 的 122.7 ms）：

| 函式 | 佔 cascade | 再往下 |
| --- | ---: | --- |
| `cascade_blocks`（把宣告套到 `ComputedStyle`） | 40.7% | `ComputedStyle::clone` 9.6%、`memmove` 3.5%、`RandomState` 的 `hash_one::<&u64>` 3.0%、`SvgStyle::same_inherited` 0.8% |
| `matches_complex_selector_internal`（selector 的比對） | 19.0% | `is_link` 1.6%、`memcmp` 1.5% |
| `hints::declarations`（presentation attribute） | 13.7% | `svg_attribute_declarations` 10.3%，其中解析路徑的 `css::svg::parse_d_attribute` 2.0% |
| `cascade` 自己 | 10.6% | |
| `recalc_style`（被 cascade 呼叫的部分） | 8.4% | `ElementAnimations::style_change` 5.1%、`animations::rendered` 0.9% |
| `RuleSet::collect`（找候選的規則） | 3.8% | `memcmp` 1.6%、`rule_set::lookup` 1.5% |

**最內層的函式**（佔 style）：`recalc_style` 16.2%、`matches_complex_selector_internal` 10.8%、`memmove` 8.2%、`cascade` 7.9%、`ComputedStyle::clone` 7.2%、`cascade_blocks` 6.1%、`memcmp` 3.4%、mimalloc 的 `realloc_zero` 2.6%、`css::properties::parse_longhands` 2.1%、`url::parser` 1.9%、mimalloc 的 `malloc_aligned` 1.5%、`ComputedStyle::initial` 1.5%、`svgtypes` 的 `parse_number` 1.5%、`flat_tree_parent` 1.3%、SipHash 的 `write` 1.2%、`RandomState::hash_one` 1.1%、`ComputedStyle::inherit` 1.1%、`rule_set::lookup` 1.1%、`cssparser` 1.1%。

TodoMVC-WebComponents 整體（不只 style）的最內層：`memmove` 8.1%、`ComputedStyle::clone` 4.6%、`url::parser` 的 `parse_cannot_be_a_base_path` 4.5%、`style::cascade` 4.3%、`matches_complex_selector` 3.0%、`ComputedStyle::inherit` 2.1%、`idna` 的 `to_ascii` 1.1%：`url()` 每次重算都對 base URL 解析（`docs/limitations.md` 的「Style」：`url()` 在算 style 時才解析）。

**`recalc_style` 自己的時間**（第 1 段，2026-10-19，現在的 build 重新取樣 NewsSite-Next 和 React-Stockcharts-SVG）：style 99.6 ms CPU，`recalc_style` 自己 4.2%，往下一層 `style::cascade` 86.3%。上面的 16.2% 是舊的 build 把 clone 這類工作 inline 進 `recalc_style`，現在算在 `cascade` 底下。剩下的 4.2% 用 `atos -i -o target/perf/kun -l 0x100000000 <位址>` 對回 inline 的函式和行號（perf profile 有 `line-tables-only` 的除錯資訊；samply 的符號表沒有 inline 的 frame）：`Arc` 的計數、`ScopeCascade` 的 drop、`StyleFlags` 的寫入、堆疊上的暫存，沒有單獨值得處理的。這次的最內層：`matches_complex_selector_internal` 14.6%、`memmove` 12.0%、`cascade_blocks` 11.5%、`cascade` 7.4%、`recalc_style` 4.2%、`hints::declarations` 3.6%、`css::properties::apply` 3.1%、SipHash 的 `write` 2.9%、`ComputedStyle::inherit` 2.2%、`ComputedStyle::clone` 2.1%。

`mach_absolute_time` 在取樣裡佔 0.5%，七成是 Perfetto 的 trace 自己讀時鐘（取樣時同時錄 trace），是量測的成本。

### 建 box tree 的取樣

- **Editor-TipTap**：`BoxTree::update` 底下 154.6 ms，佔它的 CPU 47%。最內層：`parley::shape::shape_text` 19.1%、harfrust 的 `layout_morx_table::apply`（macOS 系統字型的 AAT 表）15.6%、`InlineContent::new` 11.4%、parley 的字型涵蓋 6.1%、`Analyzer::analyze` 5.9%、icu 的斷行 5.4% 和 4.1%。往下一層：`set_content` 49.8%、`collect` 48.9%。幾乎都是文字。
- **NewsSite-Next、Stockcharts**：32.2 ms，佔它們的 CPU 6.5%：`memmove` 12.5%、`QualName` 的比較 12.1%、文字的 shaping 和斷行。

### RFC 開始前的程式

- **`ComputedStyle`**（`crates/computed_style/src/lib.rs`）：一個 106 個頂層欄位的結構，1472 bytes；list 類的值是 `Arc<[…]>`（clone 只加計數，但約 20 個 `Arc` 每次都要原子地加減）。每個元素的 cascade 從父的繼承值或初始值 clone 一份、把宣告套上去（`cascade_blocks`）。`layout` 直接讀它的欄位約 430 處、`paint` 約 110 處、`style` 約 100 處。`MatchedPropertiesCache`（RFC 0051）在規則相同時沿用上次的結果，但仍然 clone。
- **selector 的比對**（`crates/style/src/selector.rs`、`crates/css/src/rule_set.rs`）：`RuleSet` 照 id、class、tag 分桶，桶是 `HashMap<Box<str>, Vec<RuleData>>`（預設的 SipHash）；比對用 `selectors` 的 `matches_selector(selector, 0, None, …)`，不傳祖先雜湊、沒有 bloom filter（`selector.rs` 的開頭寫明）。
- **presentation attribute**（`crates/style/src/hints.rs`）：SVG 的 presentation attribute 每次 cascade 都用 CSS 的 parser 重新解析（`svg_attribute_declarations`），沒有快取。inline 的 `style` attribute 在設定時就解析、存成 `Arc<CssPropertyValueSet>`（`dom` 的 `inline_style_of`），有快取。
- **stylesheet**：`collect_document_sheets` 在 stylesheet 的文字、`<link>` 的版本、document 或 adopted 的 stylesheet 變了時重新收集；變了的話清掉 matched properties 的快取、整份重算（`everything`）。`:has()` 的規則在任何東西變了時也整份重算。rendering update 等 stylesheet（render-blocking），script 讀幾何時不等；parser 插入的 script 不等 `<head>` 的 stylesheet。

## 和 Chromium 的對照

- **`ComputedStyle`**：Blink 的 `ComputedStyle` 由 `make_computed_style_base.py` 產生，欄位分成好幾組（`box`、`surround`、`visual`、繼承的 rare、不繼承的 rare……），每組是一個 `DataRef<…>`（參考計數、寫的時候才複製：`Access()`）。複製一個 `ComputedStyle` 只複製幾個指標和少數常用的欄位；`ComputedStyleBuilder` 改一組時才複製那一組。Stylo（Firefox）的 `ComputedValues` 是約 25 個 `Arc<style_structs::…>`，繼承的 struct 沒改就直接和父共用，`StyleBuilder::mutate_*` 用 `Arc::make_mut`。
- **祖先的 bloom filter**：Blink 的 `SelectorFilter` 在走樹時把祖先的 tag、id、class 的雜湊放進 bloom filter，`RuleData` 存每個 selector 的祖先雜湊（descendant selector 的 identifier hashes），比對前先用 bloom filter 排除不可能的規則。Stylo 的 `StyleBloom` 一樣，`selectors` crate 的 `matches_selector` 收 `AncestorHashes`、`MatchingContext` 收 bloom filter；從樹的中間開始重算時，先把祖先放進 bloom filter。
- **presentation attribute**：Blink 的 `Element::UpdatePresentationAttributeStyle` 把 presentation attribute 算成一個 `CSSPropertyValueSet` 存在元素上，attribute 改了才重算（`presentation_attribute_style_is_dirty`）；`PresentationAttributeCache` 用 tag 和 attribute 的名字、值的雜湊共用同樣的結果。
- **stylesheet**：
  - render-blocking：`<head>` 裡的 stylesheet 擋住第一次 paint。
  - script-blocking（HTML 的「a style sheet that is blocking scripts」）：parser 插入的 script 等到之前的 stylesheet 都載入才執行（`HTMLParserScriptRunner`、`Document::IsScriptExecutionReady`），script 讀幾何時就不會用到半套的 stylesheet。
  - stylesheet 變了時：`StyleEngine` 比較新舊的 `RuleSet`（`RuleSetDiff`、`InvalidateForRuleSetChanges`），只讓受影響的元素重算，不整份重算。

## 參考資料

- RFC 0058 的 `details.md` 的「量測」（另一台機器，Fedora、AMD Ryzen AI 9 HX 370：style 差 184 ms、建 box tree 差 253 ms）。
- RFC 0051（`rfcs/0051-performance/README.md`）：matched properties cache（5c）、D5、D6、D8 的原始描述。
