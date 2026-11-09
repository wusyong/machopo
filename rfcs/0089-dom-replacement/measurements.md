# 換成新的 DOM 之後的量測：和舊的 DOM、Chrome 比

2026-11-06，RFC 做完、merge 回 `main` 之後的收尾的量測，給使用者審核、決定接下來怎麼處理。每一段的量測記在 `progress.md`，這份把它們和這次重量的放在一起。

## 量法

- **機器**：Fedora 44（Linux 7.2），AMD Ryzen AI 9 HX 370，接電源、擋住休眠；兩個瀏覽器綁在快的核心（0–3、12–15）。Chrome for Testing 152.0.7977.82。
- **新的 DOM**：`main` 的 `37520412`（RFC 做完），`perf` profile，連結 `kun-v152.2.0-27`。
- **舊的 DOM**：`fb61d682`（決定 166 的 commit，5a 把新的 DOM 換成預設之前的最後一個 commit：每份文件還是舊的 DOM），在 `../kun-against` 用自己的 target 目錄編。它的 fork 是 `kun-v152.2.0-25`（-27 拿掉了舊的 DOM 用的 cppgc 的介面），所以量的時候把 `../third_party/rusty_v8` 暫時切回 -25，量完切回 -27。不能用 `--against`：它要求兩邊釘同樣的 `../third_party`。
- **同一天、同樣的條件**：Speedometer 舊的那一次只跑 kun（`--browser kun`），Chrome 用新的那一次的；blink-perf 兩次都跑 Chrome，兩次的 Chrome 多半差在 2% 以內（`css/CSSPropertySetterGetter` 2.5%、`layout/line-layout-line-height` 6%），可以當這兩次之間的雜訊的參考。
- 每樣量一次（Speedometer 每個 suite 10 個 iteration 的中位數，blink-perf 每頁 runner 的 iteration 的中位數，`calls` 3 次的中位數）。

## Speedometer 3.1

`bench.py speedometer`，20 個 default suite 都有效。每個 suite 總時間的中位數（ms）：

| suite | 舊的 DOM | 新的 DOM | 新/舊 | Chrome | 新/Chrome | 舊/Chrome |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| TodoMVC-JavaScript-ES5 | 49.4 | 35.9 | 0.73 | 24.6 | 1.46 | 2.01 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 63.7 | 67.6 | **1.06** | 25.8 | 2.62 | 2.47 |
| TodoMVC-WebComponents | 42.2 | 34.3 | 0.81 | 18.6 | 1.84 | 2.27 |
| TodoMVC-React-Complex-DOM | 40.4 | 34.1 | 0.84 | 25.8 | 1.32 | 1.57 |
| TodoMVC-React-Redux | 35.8 | 31.3 | 0.87 | 26.4 | 1.19 | 1.36 |
| TodoMVC-Backbone | 30.7 | 25.7 | 0.84 | 20.5 | 1.25 | 1.50 |
| TodoMVC-Angular-Complex-DOM | 40.0 | 36.7 | 0.92 | 23.8 | 1.54 | 1.68 |
| TodoMVC-Vue | 25.5 | 22.4 | 0.88 | 17.0 | 1.32 | 1.50 |
| TodoMVC-jQuery | 186.9 | 280.9 | **1.50** | 76.3 | 3.68 | 2.45 |
| TodoMVC-Preact-Complex-DOM | 19.7 | 17.9 | 0.91 | 11.3 | 1.58 | 1.74 |
| TodoMVC-Svelte-Complex-DOM | 19.4 | 15.7 | 0.81 | 10.1 | 1.55 | 1.92 |
| TodoMVC-Lit-Complex-DOM | 29.0 | 23.1 | 0.80 | 14.8 | 1.56 | 1.96 |
| NewsSite-Next | 150.0 | 130.1 | 0.87 | 64.4 | 2.02 | 2.33 |
| NewsSite-Nuxt | 83.2 | 80.5 | 0.97 | 49.4 | 1.63 | 1.68 |
| Editor-CodeMirror | 22.9 | 21.0 | 0.92 | 17.8 | 1.18 | 1.29 |
| Editor-TipTap | 95.2 | 99.8 | **1.05** | 45.0 | 2.22 | 2.12 |
| Charts-observable-plot | 54.0 | 48.1 | 0.89 | 35.0 | 1.37 | 1.54 |
| Charts-chartjs | 69.9 | 58.7 | 0.84 | 59.2 | 0.99 | 1.18 |
| React-Stockcharts-SVG | 101.3 | 95.2 | 0.94 | 62.1 | 1.53 | 1.63 |
| Perf-Dashboard | 103.7 | 86.9 | 0.84 | 44.7 | 1.94 | 2.32 |

- **合計**：舊的 DOM 1262.9 ms、新的 DOM 1245.9 ms、Chrome 672.6 ms。和 Chrome 比：合計的比值 1.88 → 1.85，比值的幾何平均 **1.78 → 1.61**。
- **新/舊的幾何平均 0.90**：20 個 suite 裡 17 個變快（3% 到 27%）。合計只少 1.3%，因為 jQuery 一個就多了 94 ms。
- **變慢的三個**（粗體）：jQuery +50%、ES6-Webpack +6%、TipTap +5%。慢的都是改既有的項目的 test，加項目的 test 反而變快了（sync / async 的中位數，ms）：

  | suite | test | 舊的 DOM | 新的 DOM | Chrome |
  | --- | --- | --- | --- | --- |
  | jQuery | Adding100Items | 44.5 / 5.9 | 57.9 / 4.0 | 19.2 / 2.4 |
  | jQuery | CompletingAllItems | 85.7 / 7.8 | 152.5 / 4.2 | 32.8 / 2.4 |
  | jQuery | DeletingAllItems | 41.0 / 2.0 | 56.5 / 0.9 | 17.8 / 0.6 |
  | ES6-Webpack | Adding100Items | 26.2 / 5.8 | 16.9 / 5.0 | 10.3 / 3.3 |
  | ES6-Webpack | CompletingAllItems | 14.0 / 5.6 | 20.6 / 4.8 | 5.0 / 2.8 |
  | ES6-Webpack | DeletingAllItems | 9.7 / 2.1 | 17.5 / 2.2 | 3.2 / 1.0 |
  | TipTap | Long | 45.1 / 3.4 | 45.0 / 9.2 | 22.9 / 2.9 |
  | TipTap | Highlight | 43.7 / 2.6 | 42.9 / 2.8 | 18.8 / 1.7 |

  - jQuery 和 ES6-Webpack 多的都在 sync，也就是 script 本身和它強迫做的 style、layout。
  - TipTap 多的在 Long 的 async（3.4 → 9.2 ms），是 frame 的工作。
- **原因還沒查到**。兩個 suite 的 `--trace`（每個 iteration 的 ms，舊/新）裡 style、box tree、layout、paint 兩邊差不多：
  - jQuery：style 4.6 / 3.6、box tree 3.2 / 3.2、layout 1.0 / 1.0；
  - ES6-Webpack：style 4.0 / 3.3。

  所以多的在 script 裡，也就是 DOM 的呼叫。
- **補了 trace 的 script slice 之後重量**（同一天，新的 DOM；舊的 DOM 用上面那一次的）：每個 iteration 的 script，jQuery 舊的 87.3、新的 201.4、Chrome 52.7 ms；ES6-Webpack 34.6、45.4、17.2 ms。parse 反而少了（jQuery 90.0 → 56.3 ms）。多的確定在 script 裡；是哪幾個 DOM 的呼叫，要用 `--sample` 看（見下面「量測工具的缺口」，取樣也修好了）。

## Blink 的 perf tests

`bench.py blink-perf --pages …`，26 頁：預設清單的 10 頁，加上 Blink 的 `bindings/` 裡對應 `calls` 的成員的頁、`bindings/gc-*`，和這份 RFC 期間在新的 DOM 上量過的幾頁的原頁（`events/EventsDispatching`、`parser/tiny-innerHTML`、`parser/innerHTML-setter`、`css/CSSPropertySetterGetter`、`dom/child-list-mutation-with-unrelated-observer`）。兩個 DOM 都是 26 頁有效。「新/舊」是快了幾倍，大於 1 是新的快：

| 頁 | 單位 | 舊的 DOM | 新的 DOM | Chrome | 舊比 Chrome 慢 | 新比 Chrome 慢 | 新/舊 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `bindings/create-element` | runs/s | 293 | 966 | 3616 | 12.3 | 3.74 | 3.30 |
| `bindings/insert-before` | runs/s | 99.2 | 446 | 147 | 1.51 | 0.33 ※ | 4.50 |
| `bindings/set-attribute` | runs/s | 417 | 1190 | 2266 | 5.33 | 1.90 | 2.85 |
| `bindings/append-child` | runs/s | 125 | 578 | 156 | 1.24 | 0.27 ※ | 4.62 |
| `bindings/first-child` | runs/s | 865 | 1192 | 2388 | 2.77 | 2.00 | 1.38 |
| `bindings/node-type` | runs/s | 987 | 1363 | 2830 | 2.87 | 2.08 | 1.38 |
| `bindings/get-attribute` | runs/s | 209 | 303 | 471 | 2.23 | 1.56 | 1.45 |
| `bindings/get-elements-by-tag-name` | runs/s | 89.1 | 78.6 | 1127 | 12.7 | **14.3** | **0.88** |
| `bindings/get-element-by-id` | runs/s | 45.1 | 98.1 | 577 | 12.7 | 5.88 | 2.18 |
| `bindings/id-getter` | runs/s | 274 | 668 | 1201 | 4.40 | 1.80 | 2.44 |
| `bindings/id-setter` | runs/s | 112 | 310 | 717 | 6.37 | 2.31 | 2.77 |
| `bindings/gc-forest` | ms | 677 | 223 | 57.6 | 11.5 | 3.87 | 3.04 |
| `bindings/gc-mini-tree` | ms | 788 | 259 | 93.5 | 8.34 | 2.77 | 3.04 |
| `bindings/gc-tree` | ms | 937 | 276 | 113 | 8.14 | 2.44 | 3.39 |
| `css/PseudoClassSelectors` | runs/s | 2832 | 4387 | 18002 | 6.30 | 4.10 | 1.55 |
| `css/CSSPropertySetterGetter` | runs/s | 257 | 5176 | 21539 | 81.9 | 4.16 | 20.1 |
| `dom/modify-element-id` | ms | 148 | 36.9 | 52.1 | 2.83 | 0.71 | 4.01 |
| `dom/child-list-mutation-with-unrelated-observer` | runs/s | 179 | 752 | 352 | 1.96 | 0.47 ※ | 4.20 |
| `events/EventsDispatchingInV1ShadowTrees` | runs/s | 2256 | 60840 | 176988 | 78.3 | 2.91 | 27.0 |
| `events/EventsDispatching` | runs/s | 168 | 2413 | 3016 | 17.7 | 1.25 | 14.4 |
| `layout/chapter-reflow-once` | runs/s | 211 | 214 | 332 | 1.55 | 1.55 | 1.01 |
| `layout/flexbox-row-nowrap` | runs/s | 2348 | 2420 | 4589 | 1.92 | 1.90 | 1.03 |
| `layout/line-layout-line-height` | ms | 192 | 278 | 159 | 1.14 | **1.76** | **0.69** |
| `parser/query-selector-all-class` | runs/s | 243 | 386 | 1584 | 6.50 | 4.10 | 1.59 |
| `parser/tiny-innerHTML` | runs/s | 9.66 | 39.2 | 21.2 | 2.17 | 0.54 | 4.06 |
| `parser/innerHTML-setter` | runs/s | 1993 | 6347 | 6776 | 3.36 | 1.07 | 3.18 |

※ kun 快 2 倍以上，工具標「可疑」，不算進幾何平均：要人工確認兩邊做的事一樣。這三頁舊的 DOM 不是可疑的，舊的那一次算進去了，所以兩次的幾何平均不是同一組頁。

- **比 Chrome 慢的幾何平均**：舊的 DOM 5.23（26 頁），新的 DOM 2.31（23 頁，去掉 3 頁可疑的）。
- **變慢的兩頁**（粗體）：
  - `layout/line-layout-line-height` 192 → 278 ms（+45%）。這一頁是新舊 DOM 共用的 layout，差在新的 DOM 的 box tree 和文字的輸入。`docs/benchmarks.md` 記過這頁在同樣的程式的兩個 build 之間差 15%，這次的差比那個大，要再確認。
  - `bindings/get-elements-by-tag-name` 89.1 → 78.6 runs/s（−12%），是 Chrome 的 14.3 倍。
- **還差 Chrome 最多的**（4 倍以上）：
  - `get-elements-by-tag-name` 14.3、`get-element-by-id` 5.88（collection 和 id 的查找）；
  - `css/CSSPropertySetterGetter` 4.16（`element.style`，「待辦」有一條）；
  - `css/PseudoClassSelectors` 4.10、`parser/query-selector-all-class` 4.10（selector 比對：新的 DOM 沒有改它，比舊的快一點是 DOM 的讀取變快）；
  - `bindings/gc-forest` 3.87、`bindings/create-element` 3.74。

## 每次呼叫的指令數（`bench.py calls`）

`perf` profile，每個成員一百萬次，3 次的中位數；指令數，括號是 cycles。舊的 DOM 和 5a 之前的新的 DOM 在 `fb61d682` 用 `--engines js,js-next` 同一輪量，現在的在 `main`：

| 成員 | 舊的 DOM | 新的 DOM，`fb61d682` | 新的 DOM，`main` | 新的 DOM 最少時（哪一段） |
| --- | ---: | ---: | ---: | ---: |
| `firstChild` | 259 (58) | 189 (37) | 189 (37) | 170（4g-1a） |
| `nodeType` | 171 (39) | 168 (32) | 167 (33) | 131（4f-2a） |
| `getAttribute` | 920 (187) | 751 (147) | 751 (148) | 734（4g-1a） |
| `setAttribute` | 3706 (827) | 1277 (251) | 1279 (248) | 1038（2h-1b） |
| `appendChild` | 4009 (822) | 1310 (220) | 1319 (223) | 1034（2h-1b） |
| `createElement` | 8843 (3318) | 2225 (467) | 2308 (486) | 2223（5a） |
| `addEventListener` | 2511 (642) | 886 (182) | 886 (182) | 862（4g-1a） |
| `dispatchEvent` | 11709 (3558) | 3373 (785) | 3424 (758) | 3170（4g-1a） |

- 和舊的 DOM 比，改樹、建節點和事件的 method 少 2.8 到 3.8 倍，`getAttribute` 少 18%，getter 少 2% 到 27%。
- **從最少時多出來的**（每一段的數字和原因在 `progress.md` 的「做過的檢查」）：
  - `setAttribute` +241、`appendChild` +285：功能加上去的檢查，例如 observer、tree scope、slot、shadow tree 的 sheet、`[CEReactions]`、form、SVG 的 `<use>`、iframe。「待辦」那條判斷都不是寫錯或多做的。
  - 每個成員 +9 到 24：4g-4c-3a 加了跨 origin 的檢查。
  - 每個成員 +12 到 17：4h-5c-2 加了 accessor 的函式名稱，「待辦」列了候選的原因。
  - 決定 162 的單型化之後減了 8 到 22。
- **5a 之後的兩個**（「待辦」寫「收尾時一起看」）：
  - `createElement` +72（5d-4b）：先建 wrapper 之前比 receiver 的 creation context；
  - `dispatchEvent` +52（5d-4b）：每個 listener 前的 fully active 檢查，改過之後剩下的。

## 「待辦」裡還開著的效能的條目

都在當時 `progress.md` 的待辦裡，原本都說好收尾時一起看（收尾時分到 `../0085-optimization-roadmap/README.md`，決定 184）：
- 字串參數換成名稱的快取；
- getter 和 Chrome 的差距：getter 是 Chrome 的 2 倍左右，上限不在 V8，在 kun 自己的步驟（「getter 的成本拆開量」）；
- `element.style` 的讀寫（`CSSPropertySetterGetter` 4.16 倍）；
- `innerHTML` 只有文字時不建節點；
- `calls/set-attribute` 頁面慢了 6%，沒有解釋；
- `appendChild` 的功能成本（+203，17%）；
- 上面每一段沒解釋的幾條指令；
- `window.event` 讓 `dispatchEvent` 多 179；
- 4h-5c-2 每個成員多 12 到 17；
- 5d-4b 的兩個。

「待辦」也記著使用者說的「RFC 做完之後，看要不要另開一份優化的 RFC」。

## 量測工具的缺口

量的時候發現的，2026-11-06 都在 `main` 上修，`speedometer_test.py` 整份跑過，只剩下面最後一條（跑的結果記在 `progress.md`）。這份測試不在 CI 裡，5a 換成新的 DOM 之後它就有三個測試壞了，一直沒有人跑到。

- **新的 DOM 的 trace 沒有 script 的 slice**。舊的 DOM 在每個 script 的入口打 Chromium 的 slice，新的 DOM 都沒有：`--trace` 的 `phases.md` 的 script 一欄是 0.1 ms，jQuery 的 script 全算在 `PageAnimator::serviceScriptedAnimations`（每個 iteration 247 ms）。補上的位置和名稱照舊的 DOM 和 Chrome：
  - `dom_js`：callback 的每次呼叫 `v8.callFunction`、最外層的再加 `FunctionCall`（UA 呼叫的和 script 的 `dispatchEvent()` 叫到的 listener、timer、操作裡呼叫的 lifecycle callback 和 `NodeFilter`）；custom element 的建構 `v8.callAsConstructor`（upgrade 和同步的建構）；classic script `EvaluateScript`（parser 的和其他的）；兩個 microtask checkpoint `RunMicrotasks`。callback 和 script 的入口分開（`run_callback`、`run_script`、`run_constructor`），script 不算一次函式呼叫。kun 的 span 全部改用新的 crate `trace_event` 的 macro（使用者 2026-11-06）：先看 trace 有沒有收那個 level，收才建 span，像 Chrome 的 `TRACE_EVENT0` 先看 category 的旗標；`tracing` 的 span macro 沒開 trace 時照樣建一個空的 span、enter、drop。`sanity.py spans` 檢查程式不直接用 `tracing` 的 span macro。
  - `dom_api`：`EventDispatch`、`TimerFire`、`FireAnimationFrame`（都包著之後的 microtask）、`CustomElementRegistry::define`、`CustomElementReactionQueue::invokeReactions`。
  - renderer 的 main frame：`WebFrameWidgetImpl::UpdateLifecycle`、`LayerTreeHost::DoUpdateLayers`、`ProxyMain::BeginMainFrame::commit`、`PendingAnimations::Update`、`LocalFrameView::UpdateViewportIntersectionsForSubtree`、`RenderThread::run_platform_tasks`、`commit_child`；`FrameView` 的 `take_canvas_textures`、`set_wheel_event_handler_region` 和兩個 region。`LocalFrameView::PaintTree` 只包畫的部分，交給 compositor 的步驟在 `UpdateLifecyclePhases` 底下，和 Chrome 一樣（之前 `PaintTree` 包著 `PaintArtifactCompositor::Update`）。
  - 測試（`MeasureTests.test_traces_of_both`）的 frame 的步驟照新的結構改：新的 frame 沒有的四步拿掉（表單狀態、lazy image、intersection 的回報、animation 的 notice 都不在 renderer 的 frame 裡做了）；只在有事要做時才跑的兩步（resize observer、hover 狀態）不再要求一定出現，出現時照樣檢查它在誰底下；多一條：callback 之後的 checkpoint 在 callback 的 span 裡。
  - mutation test（`mutants-wrap-up-trace.toml`，14 個，包括 `trace_event` 的三個）：13 個抓到，1 個等價（rAF callback 之後的 checkpoint 移到 span 外：V8 的 engine 在 callback 回來時就跑完了，記進 `docs/test-gaps.md`）。
  - `calls`：只有 `dispatchEvent` 變了，3424 → 3481（+57；cycles 758 → 787）。補 span 的時候直接用 `trace_span!` 是 3521；`EventDispatch`、`v8.callFunction` 先看旗標之後 3437（多的 13 是兩次旗標的讀取）；全部換成 `trace_event` 之後 3481，連量三次都一樣。多的 44 不在這條路的兩個 span：把它們換回手寫的寫法、或放進不 inline 的函式，都還是 3480 上下。可能是換了 73 處之後別處的程式碼形狀改了，連帶讓 LTO 的 inline 判斷不同；沒有再拆。
- **新的 DOM 自己的 GC 不算進 GC 那一欄**：kun 的 `gc` 的事件是 `Gc.*`（舊的 DOM 的 cppgc 是 `CppGC.*`，算進去了），jQuery 每個 iteration 的 `Gc.Scavenge` 9.7 ms 沒有對到階段。`slice_phase` 加上 `Gc.`。
- **`speedometer --sample` 跑不了**：`--call-graph fp` 沒有 `--user-callchains` 時連 kernel 的 call chain 也要，這台的 `perf_event_paranoid` 是 2，不准，整個 event 開不起來（以前能取樣時這台的設定比較低）。event 本來就只取 user space，加上 `--user-callchains`；測試多檢查它。取樣的測試找 kun 呼叫 JS 的那一層，換成新的 engine 的名字（`dom_js::listeners::call_animation_frame`）。
- **`perf` profile 編 `html` 有一個警告**：`fast_path_build.rs` 的 `NodeKind` 只在 `#[cfg(debug_assertions)]` 的 `same_tree` 裡用，import 移進那個函式。clippy 跑的是 debug，所以沒抓到。
- **還沒解決：`KunTests.test_the_windows_are_the_runners_times` 每次都失敗**：照 runner 的 mark 找到的區間，第一個 iteration 的第一個 test 比 runner 回報的長 0.2 到 0.4 ms（容許 0.15），其他的都在 0.1 ms 以內（連跑三次都是這樣）。假的 runner 先 `performance.mark()` 再讀 `performance.now()`，可是第一次 `performance.mark()` 量起來不到 0.1 ms（`perf` 和 dev 的 build，開不開 trace），所以不是它；`trace_entry` 換算時間的寫法和舊的 DOM 一樣。還沒找到原因，測試沒有放寬。

## 之後的優化（在 `main` 上）

量完之後照使用者排的順序做（2026-11-06）：先追回比舊的 DOM 慢的，一項一個 commit。

- **`HTMLCollection` 的 index 快取**（Blink 的 `CollectionIndexCache`，使用者選的）：jQuery 的 `cleanData` 用 `elems[i]` 讀 `getElementsByTagName("*")`，每讀一個都從頭走樹，新的 DOM 的取樣裡 `collection_item` 和 `collection_matches` 佔 43%（舊的 DOM 照樹的版本快取整份清單，沒有這個）。現在每個 collection 記著上一次找到的元素和 index、數過的長度、當時的樹的版本，順著讀是每個元素一步。同一個 commit 修了 `schedule_shadow_tree` 排 UA shadow tree 時用 `Vec::contains` 去重（插入一串 checkbox 是 O(n²)，佔 5%），改成集合。
  - TodoMVC-jQuery 280.9 → **141.5 ms**（舊的 DOM 186.9、Chrome 80.1；和 Chrome 的比 3.68 → 1.77）：Adding100Items 57.9 → 30.2、CompletingAllItems 152.5 → 73.2、DeletingAllItems 56.5 → 28.8 ms（sync）。ES6-Webpack 沒有變（70.2 ms），是下一項。
  - `calls` 沒有變；blink-perf 的 `bindings/get-elements-by-tag-name` 也沒有變（81 runs/s，Chrome 1145）：那頁只呼叫 `document.getElementsByTagName("div")` 五萬次、不讀內容。Chrome 對同一個根和名字回傳同一個 collection（`NodeListsNodeData` 的快取），kun 每次新建一個、intern 名字、配置小寫的字串。這是和 Chrome 的差距（舊的 DOM 89.1），留給之後的優化 RFC。
- **querySelector 照 Blink 的 `MatchCompound`**（使用者選的，第一步；子樹的 bloom filter 是第二步）：ES6-Webpack 改既有項目時每個 todo 都 `document.querySelector('[data-id="…"]')` 走整份文件，新的 DOM 每個元素都重建 `MatchingContext`、重查 quirks mode，再交給 `selectors` crate（取樣 1304 個，舊的 DOM 403：它有簡單複合選擇器的快路）。現在一個 selector、compound 之間只有 descendant 或 child 的，一個 compound 一個 compound 直接比，比不了的在比到的元素上再完整比對；其他的比對 context 一次查詢建一次。
  - TodoMVC-ES6-Webpack-Complex-DOM 70.2 → **53.6 ms**（舊的 DOM 63.7、Chrome 27.3）：CompletingAllItems 21.0 → 13.6、DeletingAllItems 17.8 → 10.4 ms（sync；舊的 14.0、9.7）。jQuery 141.6 ms，和上一項一樣。
  - blink-perf `parser/query-selector-all-class` 386 → 531 runs/s（Chrome 1570；4.10 → 2.96 倍）。
- **子樹的 bloom filter 的第一步：拿掉 `Node` 的 `last_child`**（使用者選的：照 Blink 放 32 bit 的 filter，先空出位置）。`Node` 的 56 bytes 用了 54（加上 header 剛好一格 64 bytes），放不下 32 bit。Blink 的 `ContainerNode` 沒有 `last_child_`：第一個子節點的 `previous_` 指向最後一個，繞成一圈。現在照做，空出 4 bytes（50 bytes 的欄位）。
  - `calls` 的 `appendChild` 1320 → 1332（+12，約 1%：移到最後時移除和插入都要更新第一個子節點的環指標），其他成員不變；blink-perf 的 `bindings/first-child` 1192 → 1164、`append-child` 578 → 555、`insert-before` 446 → 443（各一次，在雜訊左右）。
- **子樹的 bloom filter**（Blink 的 `attribute_or_class_bloom_`，32 bit，放在拿掉 `last_child` 空出的位置，`Node` 還是 56 bytes）：querySelector 走到一個子樹缺選擇器要的屬性名稱、class 的位元時整個跳過。ES6-Webpack 的 Complex-DOM 有一大片和 todo 無關、沒有 `data-id` 的節點。
  - TodoMVC-ES6-Webpack-Complex-DOM 53.6 → **40.9 ms**（舊的 DOM 63.7、Chrome 26.4；1.96 → 1.55 倍）：CompletingAllItems 13.6 → 7.7、DeletingAllItems 10.4 → 4.5 ms。jQuery 142.5 ms，不變。
  - `calls` 的 `setAttribute` 1278 → 1300（+22：每次設屬性算名字的位元、看自己有沒有）；blink-perf 的 `bindings/set-attribute` 1190 → 1192、`append-child` 555 → 563、`query-selector-all-class` 531 → 543，都在雜訊左右。
- **`dispatchEvent` 多的 44 條指令**：拿 trace 改動之前的 `37520412` 和現在的，用 `perf record -e instructions:u` 各跑一百萬次 `dispatchEvent` 照函式比：這台 AMD 的取樣有 skid，linker 也把內容相同的函式合併（ICF，名字借用別的函式），樣本落在哪個函式不可靠，分不出那 44 條。先前把這條路上的兩個 span 換回手寫的寫法、放進不 inline 的函式也都不變。還沒找到原因；要精確查需要 valgrind 的 callgrind（這台沒有裝）。
- **`layout/line-layout-line-height` 的 +45%：不是退步**。在 `--automation-pipe`（blink-perf 的跑法）下舊的 DOM 每次 190 ms、新的 278 ms；`--headless` 下兩個都是 360 ms。`layout` 的 debug 紀錄：舊的 DOM 只在第一次建了那一萬段的盒子（10001），之後每一輪都沒有重建；新的 DOM 每一輪都重建、重新 shape（21 輪，210021 個）。頁面每輪最後設 `display: none` 但沒有強迫 layout，下一輪開始又設回 `block`：舊的 DOM 兩輪之間沒有做 rendering update，`none` 從沒套用，換回 `block` 時樣式等於沒變；新的 DOM 照 HTML 的 event loop 在兩輪之間（`setTimeout` 隔開、每輪一百多 ms）更新 rendering，盒子拆掉再建，Chrome 也是。在 `display: none` 之後多讀一次 layout 讓它套用（複製出來的頁面，同一套 runner），舊的 DOM 也是 298 ms、新的 294 ms。
- **重量 Speedometer**（同一天，20 個 suite 都有效）：

  | suite | 舊的 DOM | 現在 | Chrome | 現在/舊 | 現在/Chrome |
  | --- | ---: | ---: | ---: | ---: | ---: |
  | TodoMVC-JavaScript-ES5 | 49.4 | 36.2 | 24.8 | 0.73 | 1.46 |
  | TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 63.7 | 41.3 | 26.4 | 0.65 | 1.56 |
  | TodoMVC-WebComponents | 42.2 | 34.4 | 19.6 | 0.82 | 1.76 |
  | TodoMVC-React-Complex-DOM | 40.4 | 34.0 | 25.7 | 0.84 | 1.32 |
  | TodoMVC-React-Redux | 35.8 | 31.3 | 25.8 | 0.87 | 1.21 |
  | TodoMVC-Backbone | 30.7 | 25.3 | 19.5 | 0.82 | 1.30 |
  | TodoMVC-Angular-Complex-DOM | 40.0 | 35.3 | 25.5 | 0.88 | 1.38 |
  | TodoMVC-Vue | 25.5 | 21.5 | 17.9 | 0.84 | 1.20 |
  | TodoMVC-jQuery | 186.9 | 140.4 | 78.9 | 0.75 | 1.78 |
  | TodoMVC-Preact-Complex-DOM | 19.7 | 16.6 | 11.3 | 0.84 | 1.47 |
  | TodoMVC-Svelte-Complex-DOM | 19.4 | 15.7 | 10.4 | 0.81 | 1.51 |
  | TodoMVC-Lit-Complex-DOM | 29.0 | 22.4 | 14.8 | 0.77 | 1.51 |
  | NewsSite-Next | 150.0 | 133.5 | 63.8 | 0.89 | 2.09 |
  | NewsSite-Nuxt | 83.2 | 80.4 | 50.8 | 0.97 | 1.58 |
  | Editor-CodeMirror | 22.9 | 22.0 | 18.1 | 0.96 | 1.22 |
  | Editor-TipTap | 95.2 | **101.3** | 45.4 | **1.06** | 2.23 |
  | Charts-observable-plot | 54.0 | 48.8 | 35.7 | 0.90 | 1.37 |
  | Charts-chartjs | 69.9 | 60.9 | 61.2 | 0.87 | 1.00 |
  | React-Stockcharts-SVG | 101.3 | 95.1 | 57.0 | 0.94 | 1.67 |
  | Perf-Dashboard | 103.7 | 86.9 | 45.6 | 0.84 | 1.91 |

  合計：舊的 DOM 1262.9 ms、現在 1083.3 ms（少 14%）、Chrome 678.2 ms；和 Chrome 的比值的幾何平均 1.78（舊的 DOM）→ 1.61（收尾時）→ **1.50**；現在/舊的幾何平均 0.85。還比舊的 DOM 慢的只剩 Editor-TipTap（+6%，收尾時看到多在 `Long` 的 async：3.4 → 9.2 ms）。
- **Editor-TipTap：`focus()` 把整個編輯器的文字切成 grapheme cluster**（使用者猜的）。取樣（在 `main` 的 `23bd340a` 之上：別人推的 paint 和 layout 的兩個 commit）裡 5.7% 在 TipTap 的 `focusPreventScroll` → 找 editing host 裡 caret 第一個能放的位置（`layout::first_caret_position`）→ 每個 run 的 `clusters()`：為了拿第一個 cluster 的起點，把 host 裡每一段文字的 cluster 都算出來（`OnceLock` 裡的 `TextCluster`、parley 的 `AtomGraphemes`）。現在每個 run 只算第一個（`TextFragment::first_cluster_start`：算過的就用算過的）。
  - Editor-TipTap 102.0 → **96.3 ms**（舊的 DOM 95.2、Chrome 46.0）：`Long` 的 async 9.8 → 4.5 ms（舊的 3.4）。

## 要決定的事

1. **jQuery（+50%）和 ES6-Webpack 的 Completing、Deleting**：Speedometer 上唯一明顯比舊的 DOM 慢的，多的在 script 裡（jQuery 每個 iteration 多 114 ms）。要查的話，用 `--sample` 比兩邊的函式，找出是哪幾個 DOM 的呼叫。候選：
   - collection 和 id 的查找：`get-elements-by-tag-name` 比舊的慢、是 Chrome 的 14 倍；
   - class、`checked` 改變之後的 style 的失效；
   - `remove()` 的移除的路。

   這些還沒有證實。
2. **`layout/line-layout-line-height` 的 +45%**：先在另一個 build 上確認，是真的再查。
3. **其他的差距**（和 Chrome 比）：在這份 RFC 裡做，還是照「待辦」另開一份優化的 RFC。
4. **`test_the_windows_are_the_runners_times` 的失敗**：要不要現在查（見「量測工具的缺口」的最後一條）。
