# RFC 0051 的細節：量測的證據和程式的位置

這份放 `README.md` 的根據：最後一次的 trace 和取樣的細項、每一項在程式裡的位置（2026-10-16 的 `main`，`7b57866f`）、RFC 0012 的 `architecture-audit.md` 的 7 項的去向。

## 量測

### 來源

- **trace**：`main` 的 `d9f9511b`，`python3 scripts/ci.py speedometer --trace --iterations 3`（20 個 default suite，兩邊各一份），Chrome for Testing 152.0.7977.82，Fedora 44，AMD Ryzen AI 9 HX 370。分析：Perfetto 的 trace processor 算 renderer main thread 上每個名稱的 self time，只算計時的區間（runner 的 `FireAnimationFrame` 到之後第一個 `TimerFire` 的結束）。數字和做法在 `rfcs/0012-speedometer/progress.md` 的「收尾前最後一次的 trace」。
- **取樣**：同一天 `ci.py speedometer --sample --suites TodoMVC-jQuery --iterations 3`（`perf` profile 加 frame pointer、修掉錄 trace 時 html5ever 的 log 紀錄之後），計時區間裡 12445 個樣本。
- 下面「每次的成本」「看不到的時間」是為這份 RFC 從同一批 trace 另外算的。

### 各 suite（計時區間的 self time，ms，kun / Chrome）

| suite | JS 和 DOM API | parse | style | layout | paint | 其他 |
| --- | --- | --- | --- | --- | --- | --- |
| TodoMVC-jQuery | 885 / 182 | 1061 / 286 | 141 / 10 | 4 / 17 | 7 / 7 | 8 / 8 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 588 / 82 | 328 / 93 | 162 / 15 | 8 / 8 | 112 / 20 | 43 / 20 |
| Charts-chartjs | 610 / 269 | 0 / 0 | 3 / 2 | 1 / 3 | 324 / 19 | 1 / 127 |
| NewsSite-Nuxt | 230 / 114 | 0 / 0 | 500 / 54 | 170 / 72 | 86 / 47 | 61 / 33 |
| NewsSite-Next | 311 / 260 | 0 / 0 | 412 / 120 | 137 / 194 | 71 / 86 | 123 / 68 |
| Editor-TipTap | 110 / 85 | 8 / 3 | 332 / 5 | 214 / 156 | 21 / 27 | 6 / 7 |
| TodoMVC-JavaScript-ES5 | 331 / 86 | 195 / 64 | 81 / 10 | 6 / 18 | 11 / 10 | 11 / 25 |
| TodoMVC-Preact-Complex-DOM | 81 / 30 | 0 / 0 | 231 / 21 | 27 / 19 | 144 / 19 | 85 / 12 |
| TodoMVC-React-Complex-DOM | 269 / 138 | 0 / 0 | 152 / 21 | 10 / 24 | 129 / 23 | 56 / 15 |
| TodoMVC-Svelte-Complex-DOM | 72 / 24 | 0 / 0 | 120 / 9 | 8 / 12 | 114 / 12 | 38 / 8 |
| TodoMVC-Angular-Complex-DOM | 247 / 150 | 0 / 0 | 146 / 22 | 8 / 29 | 117 / 18 | 46 / 15 |
| TodoMVC-Lit-Complex-DOM | 72 / 43 | 1 / 1 | 54 / 12 | 7 / 15 | 75 / 14 | 17 / 10 |
| React-Stockcharts-SVG | 352 / 315 | 0 / 0 | 175 / 50 | 23 / 27 | 9 / 12 | 6 / 11 |
| TodoMVC-React-Redux | 185 / 100 | 0 / 0 | 45 / 9 | 5 / 16 | 8 / 8 | 12 / 8 |
| TodoMVC-Backbone | 168 / 73 | 23 / 12 | 90 / 9 | 6 / 9 | 6 / 7 | 11 / 8 |
| TodoMVC-WebComponents | 109 / 51 | 0 / 0 | 121 / 13 | 6 / 17 | 33 / 12 | 6 / 12 |
| TodoMVC-Vue | 118 / 46 | 0 / 0 | 86 / 12 | 9 / 12 | 37 / 9 | 14 / 10 |
| Charts-observable-plot | 215 / 125 | 0 / 0 | 82 / 21 | 5 / 15 | 10 / 15 | 7 / 4 |
| Editor-CodeMirror | 107 / 65 | 0 / 0 | 33 / 4 | 14 / 19 | 3 / 6 | 3 / 9 |
| 合計 | 5060 / 2240 | 1615 / 459 | 2965 / 418 | 667 / 681 | 1318 / 371 | 554 / 411 |

GC（136 / 144）和 custom element（9 / 11）沒有列。Perf-Dashboard 不算進合計（它的 app 自己用 rAF，kun 那邊多抓了區間）。分類：JS 是 `v8.callFunction`、`FunctionCall`、`RunMicrotasks`、`EventDispatch` 等的 self（包含 JS 呼叫、沒有自己的 span 的 DOM API）；style 是 kun 的 `Document::UpdateStyle`、`StyleEngine::RebuildLayoutTree`、`dom_api::update_form_states`，Chrome 的 `Document::recalcStyle`、`UpdateLayoutTree`；paint 包括 pre-paint、paint 之後的四步、canvas 的 raster。

### paint 這一路的細項（ms）

| 步驟 | kun | Chrome |
| --- | --- | --- |
| `raster::draw_into`（canvas，幾乎都是 chartjs） | 321 | 0（在 GPU process） |
| `Paint` | 266 | 108 |
| `PrePaint` | 220 | 4 |
| `paint::main_thread_scroll_hit_test_region` | 204 | 沒有這一步 |
| `FrameView::set_wheel_event_handler_region` | 204 | 沒有這一步 |
| `PaintArtifactCompositor::Update` | 102 | 39 |
| `LayerTreeHost::DoUpdateLayers` | 3 | 135 |

kun 的 `PaintArtifactCompositor::Update` 裡已經包括把 chunk 轉成 display list（Chrome 在 `DoUpdateLayers` 的 `PictureLayer::Update` 做），兩邊合起來比：kun 105、Chrome 174。兩個 region 集中在 Complex-DOM 的 suite，每個 suite 20 到 38 ms。

### 每次的成本（次數、合計 ms；「強制」是在 script 裡的，其他是 frame 裡的）

| suite | kun 的 style | Chrome 的 style | kun 的 layout tree | Chrome 的 `UpdateLayoutTree` | kun 的 layout | Chrome 的 layout |
| --- | --- | --- | --- | --- | --- | --- |
| NewsSite-Nuxt，強制 | 459、345.4 | 462、27.2 | 465、151.8 | 465、37.7 | 465、148.7 | 456、34.6 |
| NewsSite-Next，強制 | 459、263.2 | 469、86.9 | 465、137.2 | 477、114.7 | 465、136.7 | 465、146.2 |
| Editor-TipTap，強制 | 27、15.5 | 21、2.7 | 30、316.8 | 24、4.6 | 33、214.5 | 12、155.7 |
| Preact-Complex-DOM，frame | 22、97.9 | 16、19.3 | 22、96.4 | 16、22.8 | 22、26.3 | 17、18.8 |
| jQuery，frame | 18、100.1 | 13、6.4 | 20、20.8 | 13、8.8 | 20、3.9 | 15、16.2 |

次數兩邊差不多，每次的成本差 4 到 13 倍：kun 每次重算的範圍大。TipTap 的 layout tree 每次約 10 ms：kun 的 `RebuildLayoutTree` 裡有 inline 內容的 shaping（Chrome 算在 `Layout`），兩項加起來 531 對 160 ms。

### 看不到的時間

計時區間裡沒有被任何最外層的 span 蓋到的時間（區間的合計、蓋到的、沒蓋到的比例）：

| suite | kun | Chrome |
| --- | --- | --- |
| TodoMVC-Preact-Complex-DOM | 771、573、25.7% | 114、103、9.3% |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 1417、1269、10.4% | 246、246、0.2% |
| NewsSite-Next | 1169、1060、9.4% | 751、747、0.5% |
| TodoMVC-jQuery | 2306、2153、6.6% | 614、530、13.7% |
| Charts-chartjs | 952、952、0.0% | 439、439、0.1% |

裡面有閒置（等下一個 frame、等 timer）和沒有 span 的工作：renderer 的 event loop 在 task 之間做的事（`render_thread.rs` 的 `run` 裡 `run_due_tasks` 之後：`follow_requested_links`、`start_dynamic_imports`、`run_inserted_scripts`、`run_platform_tasks` 和它的 `prune` 等）原本不在 span 裡。第 1 段補上 span（`RenderThread::between_tasks`）之後取樣 Preact：計時區間裡 `between_tasks` 合計 1.7 ms、`run_platform_tasks` 0.65 ms，96% 的樣本在 main frame 裡，所以那 26% 幾乎都是閒置：kun 一個 frame 的工作比 Chrome 長，做完之後等下一個 frame 的時間點（`progress.md` 的第 1 段）。

### jQuery 的取樣

- fragment 的 parse（`parse_html_fragment`）佔 51%：html5ever 和 tendril 29%、cppgc 的 handle 24%、malloc / free / memmove 18%、`dom::scope` 14%、hash 4%。
- self 的前幾名：`_int_malloc` 5.4%、`cppgc__Member__Get` 5.1%、`malloc` 3.8%、`cppgc__Persistent__Get` 3.3%、`hash_one::<&StoreId>` 3.1%（JS 的頁面沒有 `StoreTable` 的項目，推測是 `NodeIdMap` 的 hash 被 linker 合併成同一個符號）、`memmove` 2.7%、`cppgc__WeakPersistent__Get` 2.6%。
- inclusive：宣告表每次呼叫結束時的 `update_elements` 12.1%、`start_loads` 3.0%（其中 `update_own_style` 2.9%：rAF callback 之後同一個 frame 裡算的，就是那個 frame 的 style，不算多出來的；`progress.md` 的第 1 段）、`realm_window` 1.5%。

## 每一項在程式裡的位置

路徑在 `crates/` 底下；行號是 2026-10-16 的 `main`。

### A. 全樹走訪

- **A1**：`renderer/src/frame_view.rs` 的 `set_wheel_event_handler_region`（約 2879 行）每次 paint 都呼叫 `paint::wheel_event_handler_region`（`paint/src/hit_test.rs` 約 138 行）；除非 root、document 或 window 有 listener，它用 hit test 的 `Walker` 走整棵樹，每個 target 呼叫 `inside_blocking_wheel_listener`（`frame_view.rs` 約 3946 行，每次新建 memo 的 `HashMap`、往上走 DOM 的祖先）。`scope.may_have_wheel_listeners()` 只在算完之後設一個旗標。
- **A2**：`paint/src/hit_test.rs` 的 `main_thread_scroll_hit_test_region`（約 80 行）無條件呼叫 `covered_scrollers`（約 218 行）：先走整棵樹收集每個 box、行和文字的 `Target`，再對每個 composited scroller 掃描之後的每個 target，每次重建 `movers()`、`clip_chain` 的 `Vec`。
- **A3**：`frame_view.rs` 的 `update_all_lifecycle_phases` 在有 composited scroller 時第二次呼叫 `paint::pre_paint_frame_tree`（約 2776 行，「unscrolled」），包在 `PrePaint` 的 span 裡。`PropertyTrees::to_root()`（`paint/src/property_trees.rs` 約 240 行）每次呼叫都重算每個 matrix，hit test、兩個 region、`painted_rect` 各自呼叫。
- **A4**：`renderer/src/render_thread.rs` 有 26 處呼叫 `start_loads`（幾乎每次 script 跑完）；`FrameView::start_loads`（`frame_view.rs` 約 1409 行）每次：`update_base_url`（`dom_api/src/document.rs` 的 `base_url` 走整份文件找 `<base>`）、`style::stylesheet_links`（`style/src/lib.rs` 約 1675 行，`for_each_in_tree_order` 整份文件）、`update_own_style`（把 style 算到最新）、`background_image_urls`（`frame_view.rs` 約 3747 行，`for_each_rendered_style` 整份文件）。
- **A5**：`dom/src/node_id.rs` 的 `NodeIdMap` 是 std 的 `HashMap<NodeId, WeakPersistent<Node>>`；`prune`（約 72 行）`retain` 整張表、每一項一次 `WeakPersistent::Get` 的 FFI。`DomIsolate::run_platform_tasks`（`dom/src/isolate.rs` 約 500 行）每次都呼叫它，renderer 的 event loop 每一輪呼叫 `run_platform_tasks`（`render_thread.rs` 約 490 行），不在任何 span 裡。

### B. DOM 的 handle 和 bindings

一次 DOM 方法的呼叫（以 `el.firstChild` 為例）：

1. `dom_js/src/templates.rs` 的 `member_callback`（約 740 行）解出 interface 和 member；template 沒有 `v8::Signature`。
2. `call_member`（約 788 行）建 `BindingContext`（`dom/src/v8_engine.rs` 約 272 行：isolate 的 slot、annex 的 FFI、TypeId 的 hash map）。
3. `this` 轉成 `Object`（`dom_js/src/convert.rs` 約 600 行）：照 `ObjectTag` 的順序逐一試（`dom/src/script_object.rs`，節點是第一個，`DOMTokenList` 要試 8 次，最多 57 個），每次 `is_api_wrapper` 加 `v8::Object::unwrap`；成功時 `Handle::new` → `Root::try_new`（`dom/src/handle.rs` 約 560 行）：`Persistent::new`（`new cppgc::Persistent`，配置）、一次 `Persistent::Get` 的 FFI、`Rc` 的計數。
4. overload：`dom_api/src/declare.rs` 的 `overloads`（約 170 行）每次呼叫線性掃描 overload 的群組。
5. 參數：`convert_args`（`templates.rs` 約 888 行）每個參數配置一個 `Vec`；字串經過 `to_rust_string_lossy` 轉碼、配置。
6. `[CEReactions]` 的 member 在 element queue 的 stack 上 push 一個 `Vec`（`dom/src/custom_element.rs` 約 608 行）；`async method`（`dispatchEvent` 等）每次 `Box::pin` 一個 future（`declare.rs` 約 1030 行）；每個 listener 的呼叫經過 `call_async` 的 `Box::pin(ready(...))`（`dom_api/src/host.rs` 約 100 行）。
7. 函式本身：`check_this` 的 `Object::kind()`（幾次 `Persistent::Get`）；元素的 `element_kind`（`dom_api/src/html_element.rs` 約 172 行）用 SipHash 查 tag 名稱。`first_child()` 是一次 `Member::Get` 的 FFI，`to_handle()`（`dom/src/scope.rs` 約 1373 行）是 `NodeIdMap` 的 SipHash、`WeakPersistent::Get`、`Persistent::new` 加 Get。
8. 回傳：`convert::to_js` → `wrapper()`（`convert.rs` 約 655 行）再算一次 `kind()`；`realm_context`（約 646 行）在 wrapper 已經存在時也算：`node.clone()`（一個 `Persistent`）、`.document().to_handle()`（再一個）、`document_realm_window`（第三個）、`window_wrapper`、`get_creation_context`。之後讀節點裡的 `WrapperSlot`（`TracedReference`，和 Blink 的 `ScriptWrappable` 一樣）。新建 wrapper 時 `index_of`（`templates.rs` 約 103 行）線性比較 interface 的名稱。回傳的字串每次 `v8::String::new`（Blink 有 `StringCache`）。暫時的 handle 各自 `Persistent::DESTRUCT`。
9. 方法和 setter 結束時：`update_elements`、`queue_mutation_observer_microtask`、`queue_backup_microtask`、`queue_selectionchange_tasks`（`declare.rs` 約 1129、1369、1049 行）。

handle 的模型：`Handle<T>`（`dom/src/handle.rs` 約 41、550 行）是 `Root { Persistent<T>, Rc<Cell<usize>> }`，`Clone` 建新的 `Persistent`；`NodeRef` 是 `&Node`（`scope.rs` 約 701 行），每次走 edge 是 `Member::Get` 的 FFI（`dom/src/gc.rs` 約 180 行），寫 edge 是 `Member::Assign` 加上目標的 `Persistent::Get`。FFI 在 fork 的 `src/binding.cc`（`cppgc__Member__Get` 約 5315 行、`cppgc__Persistent__CONSTRUCT` 約 5341 行是 `new`）；V8 是預先編好的靜態庫，`perf` profile 的 ThinLTO 跨不進去。設計的理由：`dom/soundness.md` 的 I1（scope 以外不能有沒 root 的指標）、I2（read scope 裡不跑 GC）、I13（DOM 的 scope 和 JS 的執行互斥）；`dom/README.md` 的「讀寫規則」（先收集 handle、再修改）；off-heap 的資料用 `NodeId` 指節點，所以有 id 表。

節點的 finalizer（B6）：`NodeState`（`dom/src/node.rs` 約 491 行）有屬性的 `Vec`、id 和 class 的 `Box<str>`、style 的 `Arc`、文字的 `String`；drop 它們加上每個 `MemberSlot`、`TracedSlot` 的 FFI 的 destruct（每個節點約 10 次），在 main thread 上 finalize。

### C. fragment 的 parse

- 每次：`dom_api/src/markup.rs` 的 `set_inner_html`（約 101 行）把 JS 的字串轉成 `String`；`parse_html_fragment`（約 45 行）→ `html::parse_fragment`（`html/src/lib.rs` 約 91 行）往上找 `<form>`、建一份新的 `Document`（`scope.rs` 約 2679 行）、新的 tokenizer 和 tree builder，`.one(input)` 把字串複製進 `StrTendril`、建 `<html>`。
- 每個元素（`html/src/sink.rs` 約 265 行）：`attributes()` 每個屬性 `to_string()`（約 196 行），再 `attrs.clone()`（約 278 行）；`create_element_of`（`scope.rs` 約 2794 行）把 id、每個 class 再複製成 `Box<str>`；`create_node`（約 1693 行）：`Node::new` 約 10 次 `Member` 的建構和一次 `TracedReference`（`node.rs` 約 264 行）、`make_garbage_collected`、`WeakPersistent::new`、SipHash 的插入、`Persistent::new`；每個屬性跑 7 個 `attribute_changed` 的 hook（`dom_api/src/handlers.rs` 約 253 行）。
- html5ever 的 `insert_element` 每次 clone handle 4 次（每次一個 `Persistent`）；`elem_name` 每個 token 呼叫多次（每次一個 `Persistent::Get`）；end tag 的 `in_scope` 每一層 clone 一次 handle。
- 每次 append 都是完整的 `insert_before`（`scope.rs` 約 3142 行）：兩次 `document_of`、往上的祖先檢查、`detach`、約 5 次 edge 的寫入、到 root 的 `is_connected`、style 的旗標、表單和 frame 的紀錄。文字經過 `last_child` 的 `to_handle`、`create_text` 複製文字、再一次 `insert_before`。
- parse 之後：`markup.rs` 約 54 行把每個最上層的節點 `append_child` 進 context 文件的 fragment，這是 adopt（`scope.rs` 約 2974 行，走整個子樹）；`replace_all`（`dom_api/src/node.rs` 約 702 行）再從 fragment 移除、插進目標（已連上文件，`record_frame_owners`、`enqueue_connected` 走子樹）；setter 結束時 `update_elements`。

### D. style

- 標記（`dom/src/scope.rs`）：插入（約 3286 行）標記插入的節點 `needs_style_recalc`、parent `children_changed`，並對插入的子樹做一次 DFS 找 `<style>`、`<link>`；任何屬性改變（`attributes_changed`，約 3779 行）無條件標記元素（約 3817 行），`set_attribute_with`（約 3503 行）不比較新舊的值；文字改變標記 parent；state（`:hover`、`:focus`、`:checked`）標記元素和 parent，focus 另外替每個祖先設 `:focus-within`。旗標在 `dom/src/node.rs` 約 74 行（`StyleFlags`），沒有「只有自己」的改變。
- 走訪：`StyleEngine::recalc_style`（`style/src/lib.rs` 約 620 行）沿 `child_needs_style_recalc` 往下，重算的條件在約 813 行；`Dependencies`（約 354 行）每個 tree scope 只有一個 `structural`、一個 `relational`（`:has`）和一組 `ElementState`。重算的元素的子元素一律強制（約 976 行的 `force: restyle`）。比對用 `NeedsSelectorFlags::No`（約 671 行），不記 `ChildrenAffectedBy*`。
- 每個元素（`cascade`，約 1844 行；`cascade_blocks`，約 2169 行）：`RuleSet::collect`（`css/src/rule_set.rs` 約 225 行）、排序去重、逐一 `matches_selector`；presentational hint 每次重建；配置 `LonghandId::COUNT` 長的 `winners`；`ComputedStyle::initial()`（`computed_style/src/lib.rs` 約 1719 行，約 34 個新的 `Arc`）；`var()` 每個元素重新替換、重新 tokenize 和 parse（約 2211 行）；`animator.style_change`；新舊 style 用完整的 `PartialEq` 比（118 個欄位，約 947 行）。`display: none` 的子樹照樣往下算（約 961 行）。
- 樣式表：`style_sheets_changed` 時走整份文件複製每個 `<style>` 的文字（約 1736 行），有一個不同就全部重新 parse、重建 RuleSet、整份文件重算（約 689 行）；只有 shadow tree 的 `<style>` 有 parse 的快取。
- `RebuildLayoutTree`（`layout/src/lib.rs` 約 739 行）：`StyleDifference::between`（約 343 行，複製整個新的 style）；`collect_again` 的 `set_content` → `InlineContent::new`（約 1668 行）做空白的摺疊和 parley 的 itemize、shaping（`layout/src/text.rs` 約 792 行）；任何 `StyleDifference::Layout` 都重新收集整個 block 的 inline 內容（約 1059 行）；頁面有 list item 或 counter 時，有改變就整份文件算一次 counter（約 788 行）。
- 實例：NewsSite 的 `.dropdown-toggle:checked ~ .dropdown-label div span span:first-child` 讓每次切換重算 toggle 的 parent 的子樹；Complex-DOM 的 CSS 約 40 條 `:first-child` 類、97 個兄弟 combinator（`rfcs/0012-speedometer/progress.md` 的 Complex-DOM 盤點）；UA 的 `:focus-visible`（`style/resources/html.css` 約 349 行）讓 TipTap 每次 `focus()` 重算整個編輯器的子樹；TipTap 的 `injectCSS` 讓整份文件重算。
- 已經決定過的：RFC 0039 做了分桶，祖先的 bloom filter 量過之後不做（剩下的時間在 cascade 和套用）；`MatchedPropertiesCache`、style sharing、invalidation set 留到之後。RFC 0041 把 invalidation 留到之後（「重算整個子樹已經涵蓋」），`var()` 的快取也留到之後。

### E. paint 這一路

- pre-paint：`frame_view.rs` 的 `update_lifecycle_to_pre_paint`（約 2915 行）只在 `!tree_needs_pre_paint()` 時跳過（註解寫明「kun's pre-paint isn't incremental」）。任何 layout 都設 `paint_dirty` 和 `needs_pre_paint`（約 3394 行），任何 style 的改變都經過 `box_tree_dirty` 到 `layout_dirty`；每次捲動都經過 `ScrollState::note_scrolled`（`renderer/src/scroll.rs` 約 105 行）設 `needs_pre_paint`，composited 的也是。`paint::pre_paint_frame_tree`（`paint/src/lib.rs` 約 316 行）從 `PropertyTrees::root_only()` 重建，每個 fragment 一筆 `FragmentProperties`；node 的 id 是 `Vec` 的位置（`property_trees.rs` 約 14 行）。`elementFromPoint` 和 input 的 hit test 也會觸發 pre-paint。
- paint：`paint_dirty` 是整個 frame 一個 bool；`PaintController::paint`（`paint/src/paint_controller.rs` 約 156 行）每次走每個 box，`Previous::cached`（約 77 行）在同一個 `Arc<BoxFragment>` 或同樣的 style、大小、`FragmentProperties` 時複製舊的 item；每次重建 `boxes`、`by_node` 兩個 `HashMap`。模組的開頭寫明沒有 subsequence 的快取；`rfcs/pre-rfcs/P4.md` 約 427 行是當時不做的理由（root 的 stacking context 每次編輯都變、Appendix E 把 scroller 的子樹切成幾段）。
- layerization：`PaintArtifactCompositor::update`（`renderer/src/compositing.rs` 約 49 行）每次 paint 重做 `paint::layerize`（`paint/src/layerize.rs` 約 161 行）、重建 `PropertyTreeManager`、每個 layer 的 `PaintedLayer::new`；有失效的 layer 整個 display list 重建（複製每個 `PaintOp`）。
- hit-test data 在 paint chunk 裡的做法當初刻意延後（`rfcs/pre-rfcs/P4.md` 約 311、346 行）。

### F. canvas

`dom_api/src/canvas.rs` 把 2D 的指令錄成 `PaintOp`；`update_rendering` 每個 frame 呼叫 `canvas::flush_shown`（`renderer/src/canvas.rs` 約 56 行），對這個 frame 畫過、看得到的 canvas 用 `raster::draw_into`（`raster/src/lib.rs` 約 102 行，CPU 的 Skia）把整份紀錄畫進像素；錄了 10000 個指令、被當成 `drawImage` 的來源、讀像素時提早 flush。snapshot 整張複製（`dom/src/canvas.rs` 約 330 行）給 texture layer；不是 layer 的 canvas 改成設 `paint_dirty`。

## RFC 0012 的 `architecture-audit.md` 的去向

那份（2026-10-13 的盤點）的 7 項：

| 舊的 # | 內容 | 狀態（2026-10-16） | 在這裡 |
| --- | --- | --- | --- |
| 1 | `MouseEvent` 的 `pageX` 強制 layout | 2026-10-13 做完 | — |
| 2 | handle 的模型（`Persistent`、`Member::Get` 的 FFI、SipHash） | 還沒做 | B1、B4、B5 |
| 3 | 表單的簿記 | 做完 A0 到 A2 和 B（RFC 0042）；`update_elements` 在 jQuery 仍有 12.1% | B3 |
| 4 | `innerHTML` 的解析 | 還沒做 | C |
| 5 | style 的吞吐量 | 分桶做了（RFC 0039），其他沒做 | D（範圍比原本的「吞吐量」大：invalidation 和重算的範圍） |
| 6 | 節點的 finalizer | 還沒做；最後一次的 trace 的 GC 和 Chrome 差不多 | B6 |
| 7 | 每次呼叫的固定成本 | 還沒做 | B2、B3 和「一次 DOM 呼叫」的 4 到 8 步 |

這份新增的：全樹走訪（A）、paint 這一路的增量（E）、canvas（F）、`RebuildLayoutTree` 的 shaping（D7）、看不到的時間。舊的「次數的對照」那張表的方法換成這裡的「每次的成本」：次數兩邊已經差不多，差的是每次的範圍。
