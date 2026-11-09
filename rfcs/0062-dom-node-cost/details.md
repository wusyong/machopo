# RFC 0062 的細節：基準頁、每個節點的成本、和 Blink 的對照

## Speedometer 裡的節點

2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370），`main` 的 `ac0ef036`。

`ci.py speedometer --trace`（kun 和 Chrome 各一次，ms 一個 iteration；錄 trace 時 Chrome 被拖慢得比 kun 多，比值約 1.6，沒錄時約 2.1）：

| 階段 | kun | Chrome | 差 |
| --- | ---: | ---: | ---: |
| script（JS、bindings、script 裡的 DOM） | 1051 | 669 | +382 |
| box tree + layout | 387 | 154 | +234 |
| canvas + paint | 240 | 166 | +73 |
| style | 167 | 96 | +71 |
| parse | 195 | 155 | +40 |
| GC | 18 | 27 | −9 |

script 的差距集中在 jQuery（+103）、ES6-Webpack（+61）、Perf-Dashboard（+50）、chartjs（+29）、ES5（+27）。Chrome 錄 trace 時（`devtools.timeline` 開著）`html_document_parser_fastpath.cc` 的 `CanUseFastPath` 回傳 false（`kFailedTracingEnabled`），parse 走完整的 parser：上表 Chrome 的 parse 比平常慢。

`ci.py speedometer --sample`，TodoMVC-jQuery（各 suite 的 ms 是 RFC 0061 開始前量的 393 ms）：innerHTML 的 setter（`set_inner_html`）約 203 ms，其中 `parse_html_fragment` 171 ms（`fast_path::build` 135、`fast_path::scan` 29），`replace_all` 31 ms。`build` 裡：`parser_append_child` 52 ms（`link_child` 47）、`create_element_of` 25、`create_node` 16.5、attribute 的 hook 14。

jQuery TodoMVC 每次改動都用 `.html(string)` 重畫整個清單（`app.js` 的 `render`），每個項目是一個 `<li>`、`<div class="view">`、兩個 `<input>`、`<label>`、`<button>` 和約 9 個文字節點：加 100 個項目重畫 1+2+…+100 次，勾選 100 次各重畫 100 個，刪除 100 次再重畫約 5000 個，每個 iteration 約 2 萬個項目、30 萬個節點。

## 基準頁

`#todo-list` 是 document 裡的 `<ul>`；`html` 是 jQuery TodoMVC 的 100 個項目的 markup（1501 個節點、600 個元素，和 Handlebars 的輸出一樣的空白）。每項先跑 20 次熱身，再跑 300 次（最後三項 3000 次）取平均；kun（`perf` profile）和 Chrome for Testing 152.0.7977.82 各開三次取中位數，沒有錄 trace。ms 一次：

| 操作 | kun | Chrome | 倍數 |
| --- | ---: | ---: | ---: |
| `list.innerHTML = html` | 0.988 | 0.298 | 3.3 |
| `document.createElement('div').innerHTML = html` | 1.098 | 0.233 | 4.7 |
| `list.textContent = ''`（清掉 1501 個節點） | 0.052 | 0.018 | 2.9 |
| `list.appendChild(template.content)` | 0.185 | 0.056 | 3.3 |
| `createElement`、`className`、`appendChild` 建同一棵樹（不在 document 裡） | 1.930 | 0.318 | 6.1 |
| 同上（在 document 裡） | 1.768 | 0.313 | 5.6 |
| `document.querySelectorAll('.toggle')` | 0.018 | 0.003 | 6 |
| `document.querySelector('#todo-list li:last-child')` | 0.011 | 0.004 | 2.8 |
| `list.getElementsByTagName('*')` 逐一取（600 個） | 0.128 | 0.014 | 9 |

## 每個節點花在哪

kun 跑 3000 次 `list.innerHTML = html`，`perf record` 的 `cycles:u`（每 1,000,000 一個樣本），`set_inner_html` 底下的樣本換算成每個節點的 cycle（約 2070）。每個樣本歸到它的 stack 上第一個符合的類別：

| 類別 | cycle | % |
| --- | ---: | ---: |
| 配置觸發的 GC 工作（`OutOfLineAllocateGCSafePoint` 底下：開始 incremental marking 時 `EnsureSweepingCompleted` 117、`CollectGarbage` 49、marking 的步驟 18；`SweepForNormalAllocation` 75） | 270 | 13.0 |
| tokenizer（`fast_path::scan`、`decode`） | 222 | 10.7 |
| `link_child` 自己 | 174 | 8.4 |
| root（`Root::try_new`、`Handle` 的 drop、`Persistent`） | 150 | 7.2 |
| 樹的指標寫入（`cppgc__Member__Assign` 的 FFI） | 136 | 6.6 |
| `NodeIdMap`（`WeakPersistent` 加 hash 的插入） | 123 | 5.9 |
| malloc / free | 111 | 5.4 |
| attribute 的 hook（`attribute_changed`、`uncheck_others_but`） | 109 | 5.3 |
| `walk_moved_subtree` | 87 | 4.2 |
| `create_node` 自己 | 79 | 3.8 |
| cppgc 的配置（不含 GC 的部分） | 69 | 3.4 |
| iframe、script、title（`record_frame_owners`） | 69 | 3.3 |
| `fast_path::build` 自己 | 69 | 3.3 |
| slot（`signal_slot_changes`） | 65 | 3.1 |
| attribute 的複製（`Vec<Attribute>`、`QualName` 的 clone） | 62 | 3.0 |
| style、layout 的標記（`mark_ancestors` 等） | 46 | 2.2 |
| class 和 id 的解析 | 45 | 2.2 |
| `create_element_of` 自己 | 39 | 1.9 |
| 移除舊的子節點（`detach_with`，扣掉上面已經算的） | 31 | 1.5 |
| mutation record | 24 | 1.2 |
| 表單的 pseudo-class | 19 | 0.9 |

結構（每個節點的 cycle）：`parse_html_fragment` 1836（`build` 1541：`parser_append_child` 438、`create_element_of` 391、文字節點的 `create_node` 364、attribute 的 hook 117；`scan` 216），`replace_all` 234（`detach_with` 118、`insert_before` 88）。

Chrome 一個節點約 200 ns，換成這台機器的 cycle 約 550；kun 的 parse 本身那一類（25%，約 520 cycle）就和它差不多。

## 一個節點在 kun 被登記幾次

innerHTML 設一個 TodoMVC 的清單，新的節點：

1. `fast_path::build` 對每個節點 `parser_append_child`：一次完整的 `link_child`（在 fragment 裡，`walk_moved_subtree` 只走它自己，因為它是剛建的）。`link_child` 的每一項：slot 的改變（`slot_changes_before_insert`、`signal_slot_changes`）、兄弟的位置、live range、mutation observer、`browsing_context_document`（從節點走到根）、`walk_moved_subtree`（表單的 owner、表單的 pseudo-class、`<link>`）、`<picture>`、`form_states_children_changed`、style sheet 的擁有者、`<base>`、`set_needs_style_recalc`、`set_children_changed`、`set_needs_reattach`、`dom_tree_version`、`is_connected`（再走到根）、`flush_mutations`。
2. `replace_all` 對 fragment 的每個頂層的子節點 `remove_without_record`：一次完整的 `detach_with`，`walk_moved_subtree` 走它的整棵子樹（記下表單的 `Removed`）。
3. `replace_all` 對每個頂層的子節點 `append_child`：一次完整的 `link_child`，`walk_moved_subtree` 走它的整棵子樹（記下表單的 `Inserted`）。

`dom_api` 的 `pre_insert`（`insertAdjacentHTML`、`appendChild` 一個 fragment）和 `replace`（`outerHTML`）一樣先 `remove_without_record`。

## 和 Blink 的對照

2026-10-20 讀 chromium/src 的 `main`（路徑在 `third_party/blink/renderer/core/` 底下）：

| | Blink | kun |
| --- | --- | --- |
| innerHTML | `Element::SetInnerHTMLInternal` → `ParseHTMLFragment`：`DocumentFragment::Create(document)`（不 connected），`TryParsingHTMLFragment`；成功時 `SetHoldsUnnotifiedChildren(true)`、`ParserFinishedBuildingDocumentFragment(kSkip)`，失敗時用完整的 parser 另建一個 | `set_inner_html` → `html::parse_fragment`：context 文件的 `DocumentFragment`，fast path 認得的走 `fast_path::build`，否則 html5ever |
| 往 fragment 裡接 | `ParserAppendChildInDocumentFragment`：`AppendChildCommon`、`ChildListMutationScope`（沒有 child list 的 observer 時建構子立刻返回）、`probe::DidInsertDOMNode`；不呼叫 `ChildrenChanged`、`InsertedInto`、`NotifyNodeInserted`、node list 的 invalidation、style 的標記 | `parser_append_child` → 完整的 `link_child` |
| fragment 建完 | `NotifyNodeAtEndOfBuildingFragmentTree`：葉節點直接返回；`kSkip` 不呼叫 `InsertedInto`；container 呼叫 `ChildrenChanged(kFinishedBuildingDocumentFragmentTree)`，只 `IncDOMTreeVersion` 和 `NotifyChangeChildren` | 無（每個節點已經登記過） |
| 拿出 fragment 的子節點 | `CollectChildrenAndRemoveFromOldParent`：`HoldsUnnotifiedChildren` 時 `ForgetChildren`，只清指標，沒有 `RemovedFrom`、`ChildrenChanged` | 每個頂層的子節點 `remove_without_record`（完整的 `detach_with`） |
| 接上 document | `InsertNodeVector`：每個頂層的子節點 `AdoptAndAppendChild`、`NotifyNodeInsertedInternal`（整棵子樹的 `InsertedInto`，虛擬函式，元素照種類覆寫）；`DidInsertNodeVector` 每個頂層的子節點一次 `ChildrenChanged`（`InvalidateNodeListCachesInAncestors`、`SetStyleChangeOnInsertion`） | 每個頂層的子節點 `append_child`（完整的 `link_child`，`walk_moved_subtree` 走整棵子樹） |
| 移除舊的子節點 | `RemoveChildren`：document 層級的事（focus、range、iterator、selection、`ChildFrameDisconnector`）一次，每個子節點 `RemoveBetween` 和 `NotifyNodeRemoved`，最後 `ChildrenChanged(kAllChildrenRemoved)` 一次 | 每個子節點 `remove_without_record`（完整的 `detach_with`：focus、hover、range、iterator、iframe、表單的走訪各一次） |
| 不在 document 裡的早退 | `isConnected()`（`kIsConnectedFlag`）、`InActiveDocument()`：style engine、`SetNeedsStyleRecalc`、`DetachLayoutTree` 都先看它 | `is_connected`、`browsing_context_document` 每次從節點走到根 |
| 建節點 | `MakeGarbageCollected<T>(document)`；建構子只有 `InstanceCounters` 和 `probe::NodeCreated`；`DOMNodeIds::IdForNode` 被要求時才登記 | `create_node`：cppgc 的配置（FFI）、`NodeIdMap` 放一個 `WeakPersistent`、root、document 的 edge |
| attribute | `ParserSetAttributes` 一次存好（`ShareableElementData`），每個 attribute `AttributeChanged(kByParser)`；`class` 解析一次，style 的 invalidation 因為不在 active document 裡早退 | `Vec<Attribute>` 複製一次給 hook，每個 attribute 一次 `attribute_changed` |
| `<input>` | fast path 用 `ByFragmentParser` 建，`InputType` 在 `ParserDidSetAttributes` 建一次；UA shadow tree 延到下一次 style 的更新（`ScheduleShadowTreeCreation`，移除時 O(1) 取消）；checkbox 不需要 shadow tree | 每個 attribute 的 hook（`input_type`、`uncheck_others_but`） |
| 指標的寫入 | `Member` 的 write barrier 是 inline 的 | `cppgc__Member__Assign` 的 FFI |
| GC | cppgc 的 incremental 和 concurrent marking；conservative stack scanning，stack 上的指標不用 root | incremental marking（`dom` 的安全論證不允許和 mutator 同時 trace）；handle 是 `Persistent` 的 root |

`ContainerNode::ChildrenChanged` 對一次插入呼叫 `SetStyleChangeOnInsertion`（「`SetNeedsStyleRecalc` 的輕量版」）：只標記插入的節點和祖先，parent 沒有 `ComputedStyle` 時跳過，後代不標記（parent 的 style 從 null 變成有，style recalc 自己會走到）。

## 第 1 段的估計

每個節點的 cycle：`parser_append_child` 438（`link_child` 394）剩下四次 `Member` 的寫入（約 100）；`replace_all` 的 `detach_with`（118）裡拆 fragment 的頂層子節點那一部分（它們的整棵子樹，約一半）也省掉。合計約 400 cycle，innerHTML 的 2070 的約 19%。jQuery 的 innerHTML 約 200 ms 一個 iteration，約省 38 ms，整個 suite 約 −10%。只是估計，做完量。

## 第 1 段之後

2026-10-20，基準頁的 `list.innerHTML = html` 跑 3000 次，`main`（`4e353bf9`）和第 1 段（`152be6f5`）各取樣一次，`set_inner_html` 底下每個節點的指令數（指令數每次差不多；cycle 受配置時觸發的 GC 的時機影響，同一個版本兩次可以差 14%）：5735 → 4932（−14%；cycle 那一次是 −23%）。

| 類別 | 第 1 段之前 | 之後 |
| --- | ---: | ---: |
| tokenizer（`fast_path::scan`：自己 442、`decode` 210、找引號 90、名稱的 atom 88、`Vec` 和 malloc 約 130） | 1077 | 1071 |
| attribute 的 hook（`attribute_changed`：自己 146，每個 attribute 都問一遍七個模組） | 303 | 292 |
| attribute 的複製（`Vec<Attribute>` 的 clone）和 class、id 的解析 | 324 | 294 |
| 配置觸發的 GC 和配置 | 507 | 529 |
| `Member` 寫入的 FFI | 328 | 328 |
| `NodeIdMap` | 329 | 263 |
| 子樹的走訪（`record_frame_owners`、`walk_moved_subtree`） | 620 | 417 |
| malloc / free | 270 | 252 |
| root | 150 | 223 |
| `fragment_append_child`（往上走到根，看 fragment 有沒有標記） | 0 | 135 |
| `link_child`、slot、dirty 標記、mutation record | 790 | 136 |

每個元素平均約 1.6 個 attribute（每個節點 0.63 個）：hook 每個 attribute 約 440 個指令，`class`、`data-*` 這類沒有任何步驟的也一樣。

## 第 2 段之後

2026-10-20，`main`（`d743b815`），基準頁的三項各取樣一次，指令數（`instructions:u`）。

`list.innerHTML = html`，每個節點約 5530：`set_inner_html` 底下 4789，加上呼叫結束時的 `after_call` 744（之前只看 `set_inner_html`，沒算到這段）。

| 類別 | 指令 | % |
| --- | ---: | ---: |
| tokenizer | 982 | 18 |
| 呼叫結束時的表單處理（`update_form_states` 519、form owner 約 153，`after_call` 裡） | 744 | 13 |
| 配置觸發的 GC 和 cppgc 的配置 | 518 | 9 |
| 子樹的走訪（插入：`record_frame_owners` 153、`walk_moved_subtree` 83；移除：107、79） | 422 | 8 |
| `Member` 寫入的 FFI | 336 | 6 |
| `NodeIdMap` | 293 | 5 |
| root | 179 | 3 |

表單處理裡：每個表單元素插入和移除各記一筆（form owner 的 `FormChange`、pseudo-class 的 `FormStateChange`），TodoMVC 的 checkbox、text input、button 都是 `nothing_to_do` 和 `is_plain` 判斷沒事的，每筆還是約 1300 個指令（用 id 找回節點、hash、判斷）。parser 在 fragment 裡設 `type`、`checked`、`value` 也記了 `FormStateChange::Tree` 和 `Element`；處理時元素已經在 document 裡，`Tree` 讓 `trees()` 走一遍整份 document（`depends_on_tree` 每個節點 30）。

`createElement`、`className`、`appendChild` 建同一棵樹（在 document 裡）：`after_call` 佔 20%（`update_elements` 17%，其中 `update_form_states` 10%），每次 `appendChild` 之後的處理約 3370 個指令，插入本身（`pre_insert`）約 2030；全域的 `document` 每次走 V8 的 `LoadGlobalIC_Slow`（9%，一次約 184 ns，Chrome 0.9 ns）。

`list.textContent = ''`，每個節點 373：`detach_with` 326，其中兩趟子樹的走訪 189。

Speedometer（RFC 0061 第 2 段時的取樣，`ef83f611`，計時區間的指令數）：`update_elements` 全部 3.1%（jQuery 10.4、ES5 9.4、ES6-Webpack 8.1、Backbone 4.9）；兩趟走訪 1.4%（jQuery 5.6、ES5 3.5）；`LoadGlobalIC_Slow` 0.2%。NewsSite-Next 計時區間的 cycle 有 31% 在 `innerWidth`、`innerHeight`（`geometry::viewport` 每次 style 和 layout），不在這份 RFC 的範圍。

## 完成時

2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370），`main` 的 `625c6437`（第 3 段），Chrome for Testing 152.0.7977.82。

`ci.py speedometer`（沒錄 trace，各 suite 10 個 iteration 的中位數，ms）。上一次完整的量測是 RFC 0059 做完時（`cedd95d5`，2026-10-19），在 RFC 0061 和這份之前；兩次之間的差是兩份 RFC 合起來的。Editor-TipTap 這次在 Chrome 的第 5 個 iteration 沒有 `ProseMirror-focused`、不算，兩次都只比其他 19 個：

| | `cedd95d5` | `625c6437` |
| --- | ---: | ---: |
| kun 各 suite 的幾何平均 | 78.8 | 65.4（−17%） |
| Chrome 各 suite 的幾何平均 | 36.6 | 38.9 |
| kun / Chrome | 2.15 | 1.68 |

| Suite | kun | Chrome | kun / Chrome | `cedd95d5` 時 |
| --- | ---: | ---: | ---: | ---: |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 116.2 | 29.3 | 3.96 | 5.40 |
| TodoMVC-Lit-Complex-DOM | 40.1 | 15.1 | 2.66 | 2.90 |
| TodoMVC-jQuery | 206.5 | 81.6 | 2.53 | 4.52 |
| TodoMVC-JavaScript-ES5 | 76.0 | 32.0 | 2.37 | 3.44 |
| TodoMVC-WebComponents | 36.9 | 15.6 | 2.37 | 2.38 |
| Perf-Dashboard | 125.9 | 57.5 | 2.19 | 2.93 |
| TodoMVC-Preact-Complex-DOM | 28.4 | 14.6 | 1.94 | 2.56 |
| TodoMVC-Svelte-Complex-DOM | 23.0 | 12.1 | 1.90 | 2.00 |
| TodoMVC-Backbone | 48.7 | 27.2 | 1.79 | 2.72 |
| Charts-chartjs | 289.1 | 161.6 | 1.79 | 2.28 |
| NewsSite-Next | 114.6 | 69.8 | 1.64 | 1.43 |
| NewsSite-Nuxt | 102.6 | 66.3 | 1.55 | 1.20 |
| TodoMVC-Angular-Complex-DOM | 50.8 | 37.4 | 1.36 | 1.17 |
| TodoMVC-Vue | 34.9 | 25.9 | 1.35 | 2.48 |
| TodoMVC-React-Redux | 48.3 | 38.8 | 1.24 | 1.08 |
| React-Stockcharts-SVG | 165.9 | 135.1 | 1.23 | 1.22 |
| TodoMVC-React-Complex-DOM | 46.9 | 50.8 | 0.92 | 2.15 |
| Editor-CodeMirror | 21.9 | 23.9 | 0.92 | 2.38 |
| Charts-observable-plot | 58.7 | 70.1 | 0.84 | 1.19 |

每個 suite 只量一次，Chrome 自己的 95% 區間常常很寬（ES5 23.7–42.0），單一 suite 的比值看大概；React-Complex-DOM、CodeMirror 這類從 2 以上掉到 1 以下的多半是雜訊。jQuery 的 kun 393 → 206 ms，是這份瞄準的 innerHTML。ES6-Webpack 剩下的差在 CompletingAllItems、DeletingAllItems（kun 32、29 ms，Chrome 6、4 ms 的 sync），是 script 逐一改節點的路，不是 innerHTML。

基準頁（輪流各開五次，中位數，ms；「開始時」是上面「基準頁」那一節的數字，三次的中位數）：

| 操作 | kun | Chrome | 倍數 | 開始時的倍數 |
| --- | ---: | ---: | ---: | ---: |
| `list.innerHTML = html` | 0.752 | 0.318 | 2.4 | 3.3 |
| 只 parse 進不在 document 裡的元素 | 0.848 | 0.273 | 3.1 | 4.7 |
| `list.textContent = ''` | 0.030 | 0.026 | 1.1 | 2.9 |
| `list.appendChild(template.content)` | 0.093 | 0.063 | 1.5 | 3.3 |
| `createElement`、`className`、`appendChild`（不在 document 裡） | 1.577 | 0.313 | 5.0 | 6.1 |
| 同上（在 document 裡） | 1.440 | 0.367 | 3.9 | 5.6 |
| `document.querySelectorAll('.toggle')` | 0.018 | 0.005 | 3.8 | 6 |
| `document.querySelector('#todo-list li:last-child')` | 0.011 | 0.005 | 2.4 | 2.8 |
| `list.getElementsByTagName('*')` 逐一取 | 0.129 | 0.017 | 7.5 | 9 |

後三項這份沒有碰，倍數的變化是 Chrome 這次量得慢一點。

## 之後的候選

完成時的狀態（指令數是第 2 段之後的取樣，`list.innerHTML = html` 每個節點；第 3 段沒有碰這幾塊）：

| 候選 | 每個節點 | Blink | 這份做了什麼 |
| --- | ---: | --- | --- |
| tokenizer | 982（18%） | `HTMLDocumentParserFastPath` 也是自己掃，數字沒量 | 第 2 段只省了第二次複製，掃描本身沒變 |
| 配置時的 GC 和 cppgc 的配置 | 518（9%） | concurrent marking；這裡先查 sweeping 為什麼沒在背景做完、GC 多常跑 | 沒做 |
| `Member` 寫入的 FFI | 336（6%） | inline 的 write barrier | 沒做 |
| `NodeIdMap`、root | 293 + 179 | `DOMNodeIds` 被要求時才登記；conservative stack scanning | 沒做（RFC 0058、0060 試過） |
| `is_connected`、browsing context 改用節點上的旗標 | 第 3 段之後每次插入、移除只走到根一次 | `kIsConnectedFlag` | 第 3 段只把三次合成一次 |
| `RemoveChildren` 的批次 | `textContent = ''` 已經和 Chrome 接近 | document 層級的事一次 | 沒做 |
| attribute 的 hook 和複製 | 第 2 段之後 hook 222、複製 26 | `ParserSetAttributes`，hook 照種類早退 | 第 2 段做了 |
| 節點的形狀（下一節） | 小 | 四個指標 | 沒做 |

GC、`Member` 和 `NodeIdMap` 不是 innerHTML 才有的成本，每次建節點、寫指標都付；script 建節點的路（`createElement`、`appendChild`，基準頁上還是 4–5 倍）也在這裡。

## 節點的指標

kun 的 `Node`（`crates/dom/src/node.rs`）每個節點都有 `parent`、`first_child`、`last_child`、`previous_sibling`、`next_sibling` 五個樹的 `Edge`，加上 `document`、`first_listener`、`rare_data`（cppgc 的指標壓縮開著，每個 4 bytes）。

Blink（2026-10-20 的 `main`）：`Node` 有 `parent_or_shadow_host_node_`、`previous_`、`next_`、`tree_scope_`、`layout_object_`、`data_`，`ContainerNode` 只多 `first_child_`：

```cpp
  // We do not store lastChild() explicitly; it is stored in
  // first_child_->previous_.
```

`previous_` 在第一個子節點上指向最後一個子節點，`lastChild()` 是 `first_child_->PreviousSiblingCircular()`。文字節點不是 `ContainerNode`，沒有 `first_child_`。

影響：

- **插入和移除的寫入次數一樣**：append 在 kun 寫 `previous_sibling`、上一個的 `next_sibling`（或 `first_child`）、`last_child`、`parent`；Blink 寫 `previous_`、上一個的 `next_`（或 `first_child_`）、第一個子節點的 `previous_`（當作 last child）、`parent`。都是四次。
- **差在每個節點的大小和 GC**：kun 的文字節點也帶著兩個空的子節點指標，每個節點多 4–8 bytes；marking 時每個 `Edge` 是一次 `cppgc__Visitor__Trace__Member` 的 FFI。基準頁的 marking 步驟每個節點約 18 cycle，少一兩個 edge 的影響在 1% 以內。
- **結論**：不是主要的成本；主要的是每次寫入都經過 FFI（6.6%）和登記的次數。可以和之後的候選一起看（例如文字節點不帶子節點的指標）。
