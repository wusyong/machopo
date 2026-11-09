# RFC 0062：DOM 節點的成本

- 開始日期：2026-10-20
- 狀態：✅ 已完成：2026-10-20（parser 建 fragment 時只接指標、接上目標時一次登記；fast path 的 scan 只記位置、attribute 只交給有步驟的 hook；插入和移除只走一趟子樹，沒事的表單紀錄在 `dom` 就不記。基準頁的 innerHTML 和 Chrome 的倍數 3.3 → 2.4，TodoMVC-jQuery 393 → 206 ms；Speedometer 的幾何平均和 Chrome 的比值 2.15 → 1.68（含 RFC 0061）。tokenizer、配置時的 GC、`Member` 的 FFI 沒做，留給重新評估；在 Linux 上做）
- 進度和驗收：`progress.md`
- 細節：`details.md`（基準頁的數字、每個節點的成本拆解、和 Blink 的逐項對照、各段之後的取樣、完成時的 Speedometer、節點的指標）

## 摘要

kun 的 innerHTML 每個節點約 660 ns，Chrome 約 200 ns（3.3 倍）；jQuery TodoMVC 每個 iteration 建立、插入約 30 萬個節點，這就是它和 Chrome 差最多的原因。成本沒有單一的大頭：GC 的架構約 36%、每個節點的登記 31%、parse 本身 25%、malloc 5%，每一塊單獨就接近 Chrome 處理一個節點的全部成本，所以之前的優化每次只省零頭。這份 RFC 照 Blink 改變工作的形狀，而不是把每一步削快一點，做了三段，行為都不變：

- **第 1 段，fragment 只接指標**：parser 建 fragment 時只接指標，不跑任何登記，fragment 的子節點接上目標時也只拆指標，登記和通知在接上目標時一次做完（Blink 的 `ParserAppendChildInDocumentFragment` 和 `HoldsUnnotifiedChildren`）。
- **第 2 段，parser 的配置和 attribute 的 hook**：fast path 掃的時候只記位置，建節點時才複製一次；attribute 只交給可能有步驟的 hook。
- **第 3 段，表單的紀錄和子樹的走訪**：插入和移除只走一趟子樹；不會改變 form owner 的、不在 document 裡的、沒有 pseudo-class 要更新的，`dom` 就不記，呼叫結束時不再一筆一筆查過才知道沒事。

基準頁的 innerHTML 從 Chrome 的 3.3 倍到 2.4 倍，TodoMVC-jQuery 一個 iteration 393 → 206 ms。剩下的 tokenizer、配置時的 GC、`Member` 寫入的 FFI 不是只有 innerHTML 才付的成本，Speedometer 差最多的也已經不是 innerHTML（ES6-Webpack 是 script 逐一改節點），這份到此為止，下一步對整套 Speedometer 重新取樣再排（第 5 條）。

## 動機

- **差最多的 suite 都是 DOM 重的**：2026-10-20 的 trace 和取樣（`main` 的 `ac0ef036`），JS 重的 suite 和 Chrome 接近（React-Redux 1.08、Angular 1.17、observable-plot 1.19、Stockcharts 1.22），差最多的 jQuery 4.5、ES6-Webpack 5.4、ES5 3.4 的取樣裡 DOM 佔 41–58%。jQuery 每個 iteration 393 ms 裡，innerHTML 的 setter 約 200 ms（parse 171 ms、`replace_all` 31 ms），Chrome 整個 iteration 87 ms（`details.md` 的「Speedometer 裡的節點」）。
- **同一個操作直接比**：TodoMVC 100 個項目的 HTML（1501 個節點）設進 `#todo-list` 的 `innerHTML`，kun 0.99 ms、Chrome 0.30 ms；只 parse 進不在 document 裡的元素 1.10 ms 對 0.23 ms；`createElement` 和 `appendChild` 建同一棵樹 1.93 ms 對 0.32 ms（`details.md` 的「基準頁」）。Chrome 的數字是沒錄 trace 的：Chrome 錄 trace（`devtools.timeline`）時關掉 HTML parser 的 fast path，之前用 trace 比的 parse 對 Chrome 不公平。
- **每個節點的 2070 個 cycle**（kun 的 innerHTML 取樣，`details.md` 的「每個節點花在哪」）：

  | 類別 | 佔比 | 內容 |
  | --- | ---: | --- |
  | GC 的架構 | 36% | 配置時觸發的 GC 工作 13%、handle 的 root 7%、`Member` 寫入的 FFI 7%、`NodeIdMap` 的 `WeakPersistent` 6%、配置 3% |
  | 每個節點的登記 | 31% | `link_child` 自己 8%、attribute 的 hook 5%、`walk_moved_subtree` 4%、iframe 和 script 3%、slot 3%、dirty 標記 2%、mutation record 1% |
  | parse 本身 | 25% | tokenizer 11%、attribute 的複製 3%、class 和 id 的解析 2%、建節點的其他部分 6% |
  | malloc / free | 5% | |

- **登記的形狀和 Blink 不同**：innerHTML 建的 fragment 裡的每個節點，在 kun 跑三次完整的登記：
  - parser 接進 fragment 時一次完整的 `link_child`（八種登記，每個節點一次）；
  - `replace_all` 把 fragment 的子節點拿出來時，每個頂層的子節點一次完整的 `detach_with`（走它的整棵子樹，記下表單的 `Removed`）；
  - 接上目標時，每個頂層的子節點一次完整的 `link_child`（再走整棵子樹）。

  Blink 建 fragment 時只接指標（`ParserAppendChildInDocumentFragment`），建完對 container 呼叫一次幾乎是空的 `ChildrenChanged`；接上 document 時 fragment 的子節點只拆指標（`ForgetChildren`），每個頂層的子節點的子樹走一趟 `InsertedInto`，`ChildrenChanged` 每個頂層的子節點一次（`details.md` 的「和 Blink 的對照」）。

目標：

- innerHTML、`insertAdjacentHTML`、`outerHTML` 這類 fragment 的 parse，每個節點的登記只做一次，在接上目標時。
- 行為不變：DOM、HTML 的語意（表單的 owner、radio 的 group、mutation observer、custom element、圖片）和現在一樣，現有的測試、WPT 守著；debug build 交叉檢查。
- 之後的段落照量出來的數字決定。

不在範圍內：

- bindings（RFC 0061 做完）；`createElement` 和 `appendChild` 走的是一般的插入，第 1 段不碰。
- 完整的 parser（html5ever 的 tree builder）：fast path 不認得的 markup 照舊。
- style、layout、paint。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

對網頁和使用者：行為不變，只是變快。

對之後改 `dom` 的人：

- `DomMutationScope` 多一種 append：只給 parser 往自己剛建、還沒給過任何人的 fragment 裡接新建的節點，只寫指標，不跑登記；這樣的 fragment 記著「子節點還沒通知」。
- 這樣的 fragment 的子節點被接到別處時，只拆指標，接上目標時照一般的插入做完整的登記（走整棵子樹一次）。
- 新增一種「插入時要做的事」時，照舊寫在一般的插入裡；只接指標的路不用改，因為它保證這些事接上目標時都會做到（debug build 檢查）。
- 插入和移除的子樹只走一趟（`walk_moved_subtree`）：新的「對子樹裡每個節點要做的事」加在這一趟裡，不另外走。
- 表單的紀錄在 `dom` 先過濾：一個元素的插入、移除會不會改變 form owner、pseudo-class，`dom` 照它的種類、attribute 保守地判斷，判斷沒事的不記。`dom_api` 的 form owner 或 pseudo-class 多看一種狀態時，`dom` 的判斷要跟著改；漏改的話 debug build 把跳過的交給 `dom_api` 再判斷一次，會擋下。
- attribute 的 hook 只收可能有步驟的 attribute：模組的 attribute change steps 多看一個名稱時，加進它的 `ATTRIBUTES`。

## 詳細設計

### 第 1 段：fragment 只接指標

1. **`DomMutationScope::fragment_append_child`**（Blink 的 `ContainerNode::ParserAppendChildInDocumentFragment`）：`parent` 在一個標成「子節點還沒通知」的 `DocumentFragment` 裡（含它自己），`child` 是剛建的節點（沒有 parent、子節點和 shadow root，在同一份文件）。只寫四個指標（`previous_sibling`、上一個的 `next_sibling` 或 `first_child`、`last_child`、`parent`），不跑 `link_child` 的登記。fast path 的 `build` 往 fragment 裡接時用它；完整的 parser 照舊。
2. **fragment 的旗標**：`create_document_fragment` 給 parser 用的版本標上「子節點還沒通知」（Blink 的 `HoldsUnnotifiedChildren`）；`dom_tree_version` 在 parse 完加一次，不是每個節點一次。
3. **拿出 fragment 的子節點**：`dom_api` 插入 fragment 的三條路（`replace_all`、`pre_insert`、`replace`，現在都先對每個子節點 `remove_without_record`，也就是完整的 `detach_with`），遇到標著旗標的 fragment，只拆指標（Blink 的 `ForgetChildren`），不經過 `detach_with` 的登記，只做 DOM 的「remove」裡 script 看得到的部分：指著它們的 live range 和 node iterator 跟著移動、mutation observer 的 transient registration（第 1 條；沒有 range、iterator 和 observer 時幾乎不花）；接上目標照舊經過 `link_child`，它走整棵子樹，該記的（表單的 owner、表單的 pseudo-class、iframe、script、`<link>`、style 的標記、custom element）都在這時記下。
4. **範圍和 Chrome 一樣**（第 1 條）：fragment 的 parse 都這樣做（`innerHTML`、`outerHTML`、`insertAdjacentHTML`、`setHTMLUnsafe`、`createContextualFragment`）；`createContextualFragment` 交給 script 的 fragment 旗標照樣留著，不另外補做（Blink 的 `CreateContextualFragment` 也一樣）。旗標只在子節點整批被拿走時用到，script 對這個 fragment 的其他操作（移除一個子節點、再接進新的節點）照一般的路走。`DOMParser` 和 `Document.parseHTMLUnsafe` 不做：Chrome 的 fast path 只 parse fragment（`TryParsingHTMLFragment` 要一個 context element），kun 的 fast path 也 parse 整份文件，那條路照舊用一般的插入。

**跳過的登記要逐項確認**：`link_child` 的每一項對一個不在 document 裡、沒有被 script 拿到過、裡面只有 fast path 認得的元素（沒有 `<form>`、`<select>`、`<option>`、`<textarea>`、`<fieldset>`、`<style>`、`<link>`、`<base>`、`<script>`、`<iframe>`、`<template>`、`<picture>`）的 fragment，是不是沒有可見的效果，或是接上目標時會照樣做：

| `link_child` 的登記 | 在只接指標的 fragment 裡 |
| --- | --- |
| slot 的改變 | fast path 不建 shadow root，沒有 slot |
| 兄弟的位置（`set_siblings_changed`） | 還沒有 style；接上之後整棵重算 |
| live range、node iterator | script 拿不到這個 fragment，沒有 range 指著它 |
| mutation observer | 同上，沒有 observer |
| iframe、script、`<title>`、`<link>` | 不在有 browsing context 的 document 裡，本來就不記；fast path 也不建這些元素 |
| 表單的 owner（`FormChange::Inserted`） | fragment 裡沒有 `<form>`，`form` attribute 只在 connected 時找；接上目標時 `walk_moved_subtree` 照樣記（要確認 radio 的 group 在 fragment 裡的行為和現在一樣） |
| 表單的 pseudo-class（`FormStateChange::Inserted`） | 還沒有 style；接上目標時照樣記 |
| `<picture>` | fast path 不建 `<picture>`、`<source>` |
| style 的標記、box 的 reattach | 還沒有 style；接上目標時整棵標記 |
| custom element 的 `connectedCallback` | 不 connected，本來就不排 |

**debug build 的交叉檢查**：fast path 在 debug build 已經用完整的 parser 再 parse 一次比對樹（`html::fast_path::same_tree`）。加上：只接指標的 fragment 接上目標之後，`dom` 待處理的紀錄（`element_changes`、`form_state_changes`、`link_changes`、style 的標記）和用一般的插入建同一棵樹、接上同一個目標時一樣。

估計：每個節點的 `parser_append_child` 約 438 cycle（21%）剩下四次指標的寫入（約 90），`replace_all` 裡從 fragment 拆出來的那一半也省掉。innerHTML 約 −20%，jQuery 約 −8–10%（`details.md` 的「第 1 段的估計」）。做完量的：基準頁的 `innerHTML` −32%（`progress.md`）。

### 第 2 段：parser 的配置和 attribute 的 hook

第 1 段之後再取樣（`details.md` 的「第 1 段之後」），最大的是 tokenizer 和 attribute（第 3 條）：

1. **scan 只記位置**：fast path 照舊先掃完、確認收下才建節點，掃的時候名稱、attribute 的值和文字都只記在輸入裡的位置，所有元素的 attribute 放在一個 `Vec` 裡；建節點時才變成 atom、複製一次，有字元參照的才 decode。
2. **attribute 不再為了 hook 複製一份**：`ElementHooks::may_have_steps` 說有的 attribute 才交給 `attribute_changed`。`dom_api` 的判斷：`on…`、`<input>` 的任何 attribute，和 `frame`、`script`、`select`、`canvas`、`form_states`、`image` 各自的 `ATTRIBUTES`（它們的步驟一開始也用同一份）；script 改 attribute 的路也先問它。
3. **`fragment_append_child` 由 parser 傳 fragment 進來**，不再每次往上走到根看它有沒有標記；parent 不在裡面時 debug build 擋下。

做完量的：innerHTML 每個節點的指令 −3.3%、cycle −9%（`progress.md`）。省的是第二次複製，掃描本身沒變。

### 第 3 段：表單的紀錄和子樹的走訪

第 2 段之後再取樣（`details.md` 的「第 2 段之後」）：呼叫結束時的表單處理是 innerHTML 的 13%，TodoMVC 的 checkbox、text input、button 每個插入和移除各記兩筆，查過都沒事；子樹的走訪兩趟 8%（第 4 條）。

1. **一趟走訪**：`record_frame_owners` 併進 `walk_moved_subtree`，沿著 edge 走（不用堆疊），有 browsing context 時走進 shadow tree、記著在 shadow tree 裡幾層：iframe、script、autofocus、`<meta>`、`<title>` 照 shadow-including 的順序，表單、style sheet、`<base>`、`<link>` 只看 light tree。插入和移除的 shadow-including root 只找一次，connected 和 browsing context 都從它看。
2. **form owner 的紀錄**：插入的沒有 `id` 和 `form` attribute、不是 radio、owner 不會變的 form 和 listed element（祖先的 form 只在建過 `<form>` 之後找），移除的沒有 `id`、`form` attribute 和 owner 的 listed element，不記。
3. **表單的 pseudo-class 的紀錄**：只記 document 裡的元素（attribute 的改變看元素，插入、移除和 `<textarea>`、`<option>` 的文字看 parent）；建元素時不再記。不是 option、select、radio、沒有 form owner 的（`is_plain`）插入只記 UA shadow tree 要更新（而且只在它可能有的時候），移除不記。
4. **debug build 的檢查**：跳過的記在另一份清單，`dom_api` 用自己的判斷（`nothing_to_do`、`is_plain`）確認每一筆真的沒事。

做完量的：innerHTML 每個節點的指令 −14%（呼叫結束時的處理 744 → 0），`textContent = ''` −13%，script 建同一棵樹 −20%（`progress.md`）。

### 完成時的量測

`details.md` 的「完成時」：基準頁的 innerHTML 和 Chrome 的倍數 3.3 → 2.4，`textContent = ''` 2.9 → 1.1，`createElement` 和 `appendChild` 5.6 → 3.9；Speedometer 的幾何平均和 Chrome 的比值 2.15 → 1.68（和 RFC 0061 之前比，含它），TodoMVC-jQuery 4.5 → 2.5。

沒做的候選（`details.md` 的「之後的候選」）：tokenizer（innerHTML 每個節點的 18%）、配置時的 GC（9%）、`Member` 寫入的 FFI（6%）、`NodeIdMap` 和 root、節點上的 connected 旗標、`RemoveChildren` 的批次、節點的形狀。GC 的 root、`Member` 的 FFI、`NodeIdMap` 的 `WeakPersistent` RFC 0058 和 0060 試過（root 表的 floating garbage、`NodePtr` 只省約 0.6% 的指令、99.8% 的節點都被拿過 id）。

### 測試

- 行為不變：`dom`、`dom_api`、`html` 的測試，`test.py wpt` 的 `dom/`、`html/syntax/parsing/`、`domparsing/`、`html/semantics/forms/`、`custom-elements/`、`html/semantics/embedded-content/the-img-element/`。
- debug build 的交叉檢查（上面）：只接指標和一般的插入，接上目標之後待處理的紀錄一樣。
- 工作量的測試：parse 一個 fragment、接上 document，每個節點的 `walk_moved_subtree` 只走一次（計數，不量時間，像 `style` 的 `matching::few_selectors_are_matched`）；插入、移除一棵子樹（含 shadow tree），每個節點只走一次。
- 第 2、3 段：parser 的 hook 只對有步驟的 attribute 說有；`dom` 記和不記的表單紀錄逐種元素列出；跳過的紀錄在 debug build 由 `dom_api` 再判斷一次。
- mutation test：只接指標的路漏接一個指標、拆指標時沒拆乾淨、指著子節點的 live range 沒跟著移動、整份文件的 parse 也只接指標，各有測試失敗。
- 效能的量測不排進測試計畫（`docs/benchmarks.md` 的「效能的量測」）；`details.md` 的基準頁在使用者要求時重量。

### 分階段

1. **fragment 只接指標**（上面的 1–4），做完量一次（基準頁和 jQuery 的取樣）。
2. **parser 的配置和 attribute 的 hook**（第 3 條）。
3. **表單的紀錄和子樹的走訪**（第 4 條）。

每段做完各自 merge 回 `main`。第 3 段之後收尾（第 5 條）。

## 缺點

- `dom` 多一條不跑登記的插入路，新的「插入時要做的事」如果只在接進 fragment 時有意義、接上 document 時不會補，會漏掉；debug build 的交叉檢查和「設計概要」的規則守著。
- fragment 多一個旗標；`createContextualFragment` 給 script 的 fragment 也帶著它，拿走它的子節點的路（`replace_all`、`pre_insert`、`replace`）都要經過只拆指標的那一條，漏了只是慢，不會錯。
- 效益集中在 innerHTML 重的頁面（jQuery、ES5、ES6-Webpack）；React 這類用 `createElement` 的只受第 3 段的影響。
- `dom` 判斷表單紀錄記不記的規則，是 `dom_api` 的判斷的保守版本，兩份要一起改；漏改 `dom` 的只在 debug build 擋下，release build 會少做表單的更新（看得到的錯）。`dom_api` 的判斷本身沒有拿掉，它照樣處理記下的紀錄。

## 理由和其他做法

- **照 Blink 改工作的形狀，不再削每一步**：之前的 RFC（0051 的 C、0058、0060、0061）每次削一塊，每塊都只佔幾 %；這裡省掉的是整組登記的兩次重複，不是讓登記變快。
- **不選讓 `link_child` 的每一項都先判斷「在不在 document 裡」再早退**：還是每個節點跑八次判斷和函式呼叫，`is_connected` 本身也要走到根；而且 fragment 裡的節點接上目標時又會再走一次。
- **不選先做 `is_connected` 的旗標**：它讓每次插入都便宜一點，但 fragment 的三次登記還是三次；旗標放進之後的候選。
- **不選只做 innerHTML 的 `replace_all` 那一半**（拆 fragment 不經過 `detach_with`）：建的時候的登記（21%）比拆的時候大。
- **第 3 段之後不再接著做 tokenizer 和 GC**：見第 5 條。

## 先例

- **Blink**：`HTMLDocumentParserFastPath` 用 `ParserAppendChildInDocumentFragment` 往 fragment 裡接（只有 `AppendChildCommon`、沒有 observer 時立刻返回的 `ChildListMutationScope` 和一個 probe），建完 `ParserFinishedBuildingDocumentFragment(kSkip)`，`SetHoldsUnnotifiedChildren(true)`；`AppendChild` 一個這樣的 fragment 時 `ForgetChildren` 只清指標，`InsertNodeVector` 對每個頂層的子節點走一趟 `InsertedInto`，`ChildrenChanged` 一次。完整的 parser（`HTMLConstructionSite`）用 `ParserAppendChild`，每個節點都通知。

## 已決定的事項

2026-10-20，第 1 段開始前：

1. **範圍照 Chrome**：所有 fragment 的 parse，連 `createContextualFragment` 交給 script 的 fragment 也留著旗標、不補做；`DOMParser` 和 `Document.parseHTMLUnsafe` 不做（Blink 的 fast path 只有 `TryParsingHTMLFragment`，整份文件走完整的 parser）。和 Chrome 不同的一點：拆旗標 fragment 的子節點時，kun 照樣做 DOM 的「remove」裡 script 看得到的部分（live range、node iterator 的移動，mutation observer 的 transient registration）。Blink 的 `ForgetChildren` 只清指標，script 從 `createContextualFragment` 拿到 fragment、在裡面設了 range 之後再把它接到別處，range 不會跟著移動；這份 RFC 的目標是行為不變，而這些在沒有 range、iterator 和 observer 時幾乎不花。不選草稿建議的「交給 script 之前補做、清掉旗標」：多一趟走訪，Chrome 也不這樣做。
2. **交叉檢查在 debug build 每次做**：fast path 用一般的 append 再建同一棵樹，每次 append 只能記下接上目標時會再記的紀錄（表單的 owner 和表單的 pseudo-class 的 `Inserted`）。做的時候原本想比對接上目標之後的紀錄，但同一個目標不能接兩次，改成比對每次 append。fast path 在 debug build 本來就用完整的 parser 再 parse 一次比對樹，多比對紀錄的成本不大。不選只在測試裡比對：只守得住有測試的 markup。

2026-10-20，第 1 段做完、再取樣之後：

3. **第 2 段做 parser 的配置和 attribute 的 hook**（`details.md` 的「第 1 段之後」）：
   - **scan 只記位置**（A2）：fast path 照舊先掃完、確認收下才建節點，但掃的時候名稱、attribute 的值和文字都只記在輸入裡的位置（`&str`），建節點時才複製一次，有字元參照的才 decode。不選邊掃邊建（Blink 的做法）：掃到一半失敗時，已經建好的元素跑過 attribute 的 hook，kun 的 fast path 收 `<img>`，丟掉的 `<img>` 照樣抓圖、延後 `load`；Blink 的 fast path 為了這個不收 `<img>` 和 `on*` attribute。兩種省的是同一塊（字串的複製和配置），不必為了它拿掉 `<img>`。
   - **attribute 不再為了 hook 複製一份**（B）：parser 把 attribute 交給 `create_element` 之後，只把有事要做的那幾個交給 hook；有沒有事要做照 attribute 的名稱判斷（和元素是不是 `<input>`、`<select>`、`<option>`、`<optgroup>`），名稱的清單是各模組的步驟自己用的那一份，不另外列。Blink 的 `ParserSetAttributes` 一次存好，`AttributeChanged` 照元素的種類各自處理。
   - **`fragment_append_child` 由 parser 傳 fragment 進來**，不再每次往上走到根看它有沒有標記；debug build 照樣檢查。
   - 不選接上和移出時的子樹走訪（第 3 段再看，一般的插入也受益）、配置時的 GC（先另外調查）。

2026-10-20，第 2 段做完、再取樣之後（`details.md` 的「第 2 段之後」）：

4. **第 3 段做表單的紀錄和子樹的走訪**（甲），不選只做走一趟、先查 GC：
   - **不在 document 裡的元素不記表單的 pseudo-class 的紀錄**：attribute 的改變、插入和移除，`dom` 只在元素（插入、移除時是 parent）connected 時記；插進 document 時本來就記 `Inserted`，整個當新的重算。Blink 的 `StyleEngine::PseudoStateChangedForElement` 對不在 active document 裡的元素也立刻返回。不選照樣記、只讓 `trees()` 不走整份 document：parser 在 fragment 裡設的 `type`、`checked`、`value` 照樣記，處理時元素已經在 document 裡，`type` 的紀錄讓每次呼叫走一遍整份 document。
   - **「插入、移除之後沒事」在 `dom` 走子樹時判斷**：form owner 沒有、沒有 `form` attribute、祖先沒有 form（isolate 沒做過 `<form>` 時不找）、不是 option、不是 checked 的 radio、沒有 id（用過 `form` attribute 時才看）的不記；表單的 pseudo-class 沒事的（`is_plain`）移除不記，插入只記 UA shadow tree 要更新。看 `type` 判斷的保守：拿不準的照樣記。debug build 把跳過的記在另一份清單，`dom_api` 用自己的 `nothing_to_do`、`is_plain` 確認每一筆真的沒事（像 `record_ua_tree_skipped`）。Blink 在 `ListedElement::InsertedInto` 當下判斷；kun 的步驟不能在 `dom` 改樹時跑，照樣排隊，只是少排。不選照樣記、讓 `dom_api` 處理得便宜一點：省不到一半。
   - **插入和移除只走一趟子樹**：`record_frame_owners` 併進 `walk_moved_subtree`，一趟 shadow-including 的走訪，帶著「在 shadow tree 裡」，表單和 style sheet 的部分照樣只看 light tree；沿著 edge 走，不用 `Vec` 當 stack。Blink 的 `NotifyNodeInserted`、`NotifyNodeRemoved` 各走一趟。不選維持兩趟、只拿掉 stack：只省一半。

2026-10-20，第 3 段做完、量過 Speedometer 之後（`details.md` 的「完成時」）：

5. **收尾，不做第 4 段**：剩下的候選（tokenizer 18%、配置時的 GC 9%、`Member` 的 FFI 6%，innerHTML 每個節點的比例）就算 tokenizer 再省三成，也只是 innerHTML 的約 5%、jQuery 一個 iteration 約 5 ms。Speedometer 差最多的已經不是 innerHTML：ES6-Webpack 約 4 倍，時間在 script 逐一改節點的 CompletingAllItems、DeletingAllItems；Lit、WebComponents 的差在哪還沒查過。GC、`Member`、`NodeIdMap` 也不是 innerHTML 才有的成本。下一步對整套 Speedometer 取樣（`ci.py speedometer --sample`），把各類的時間乘上各 suite 和 Chrome 的差，重新排出效益最高的，再開新的 RFC。不選接著做 tokenizer：剩的空間小，而且只對 innerHTML 有用。

## 待決定

無。

## 之後的可能

- `details.md` 的「之後的候選」：tokenizer、配置時的 GC、`Member` 寫入的 FFI、`NodeIdMap` 和 root、節點上的 connected 旗標、`RemoveChildren` 的批次、節點的形狀。先對整套 Speedometer 重新取樣再排（第 5 條）。
- `createElement` 和 `appendChild` 的路（基準頁上還是 Chrome 的 4–5 倍）：bindings 和一般的插入，ES6-Webpack 的差可能多在這裡（還沒取樣確認）。全域的 `document` 每次走 V8 的 `LoadGlobalIC_Slow`（`details.md` 的「第 2 段之後」）。
- `DOMParser`、`template` 的內容這類不在 document 裡的整份 parse 也只接指標。
- NewsSite-Next 的 `innerWidth`、`innerHeight` 每次跑 style 和 layout（`details.md` 的「第 2 段之後」），不在這份的範圍。
