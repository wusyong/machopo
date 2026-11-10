# RFC 0091：DevTools 的 Elements：`DOM`、`CSS` 和 `Overlay`

- 開始日期：2026-11-09
- 狀態：✅ 已完成：2026-11-10（接受、排程：2026-11-09；「已決定的事項」第 1 到 7 條：整份的範圍和分段、第 1 段；第 8 到 10 條、第 14 到 18 條：第 2 段；第 11、12、19 到 29 條：第 3 段）
- 進度和驗收：`progress.md`
- 細節：`details.md`（RFC 開始前（2026-11-09）machopo 的 DevTools、DOM、style 和 paint 有什麼；Chromium 的對照；DevTools 前端送的命令；各個選項的完整比較；其他實作）

## 摘要

Chrome 的 DevTools 連上 machopo 之後，Elements 面板能用：
- **DOM 樹**：列出頁面的 DOM 樹，script 改動時跟著更新；iframe 的文件、shadow root、`::before` 和 `::after` 都在樹裡。
- **在頁面上框出節點**：滑過樹裡的節點時，頁面上框出它的 content、padding、border、margin，旁邊有標籤和大小；也可以用「選取元素」的按鈕直接在頁面上點。
- **選了節點之後**：
  - Styles 列出套用的規則和繼承來的；
  - Computed 列出 computed style 和 box model；
  - Event Listeners 列出 listener；
  - Console 的 `$0` 就是它，Console 裡的節點也能跳回 Elements。
- **其他面板**：Sources 列出 stylesheet；停在斷點時，頁面上有「Paused in debugger」。

做法照 Blink 的 `InspectorDOMAgent`、`InspectorCSSAgent`、`InspectorOverlayAgent`：三個 agent 都在 renderer，和現有的 page、network、log agent 並列，browser 只轉送。這份只讀、不改頁面，照 0075 定的路線「先唯讀」。編輯 DOM 和 style、undo 留到之後；唯一放進來的是 `CSS.forcePseudoState`（Styles 的 `:hov`，「已決定的事項」第 1 條）。

## 動機

**現況**：
- 0075 和 0082 做了 CDP 的骨架、V8 的 inspector、`Page`、`Network`、`Log`。DevTools 的 Console、Sources、Network、Memory 和 Profiler 能用。
- `DOM`、`CSS`、`Overlay` 一律回答找不到方法，所以 Elements 面板是空的。
- 要查 layout 或 style 和 Chrome 不同的地方，只能在 Console 裡一次問一個值（`getComputedStyle`、`getBoundingClientRect`），或比截圖。DevTools 連上 Chrome 時理所當然的「點一下看規則」在 machopo 上做不到。

這是 0075 的「已決定的事項」第 1 條的路線第 3 項（Elements）。前兩項（0075、0082）做完了。第 4 項 Playwright 和這份無關，不是 DevTools 的面板。

**目標**：
- **DOM 樹**：
  - 命令：`DOM.enable`、`getDocument`、`requestChildNodes`，以及 `setChildNodes` 事件。
  - 改動的事件：`childNodeInserted`、`childNodeRemoved`、`childNodeCountUpdated`、`attributeModified`、`attributeRemoved`、`characterDataModified`、`inlineStyleInvalidated`、`shadowRootPushed`、`shadowRootPopped`、`documentUpdated`、`pseudoElementAdded`、`pseudoElementRemoved`。
  - 樹的內容和 Chrome 一樣：iframe 的 `contentDocument`、open、closed 和 user-agent 的 shadow root（前端預設不顯示 user-agent 的）、`<template>` 的內容、`::before`、`::after`、`::marker`；只有空白的文字節點預設不送。
- **節點和 JS 的連接**：
  - 命令：`DOM.resolveNode`、`requestNode`、`describeNode`、`setInspectedNode`（`$0`）、`pushNodesByBackendIdsToFrontend`、`pushNodeByPathToFrontend`。
  - Console 裡的節點顯示成 `div#app.main`，滑過時頁面上框出它；`inspect(node)` 跳到 Elements。
  - Properties 面板經過 `resolveNode` 加上現有的 `Runtime` 就能用。
- **查詢和幾何**：
  - `DOM.querySelector`、`querySelectorAll`；
  - `performSearch`、`getSearchResults`、`discardSearchResults`（Elements 的 Ctrl+F）；
  - `getBoxModel`、`getContentQuads`、`getNodeForLocation`、`getTopLayerElements`。
- **Event Listeners**：`DOMDebugger.getEventListeners`。
- **Styles 和 Computed**：
  - stylesheet：`CSS.enable`，以及 `styleSheetAdded`、`styleSheetRemoved` 事件。
  - 套用的規則：`getMatchedStylesForNode`，包括套用的規則、繼承來的、pseudo element 的、inline style 和 attribute 的 style。
  - computed style 和 inline style：`getComputedStyleForNode`、`getInlineStylesForNode`。
  - 其他命令：`getStyleSheetText`、`getPlatformFontsForNode`。
  - Sources 的檔案樹列出 stylesheet。
- **Overlay**：
  - 框出節點：`Overlay.enable`、`highlightNode`、`hideHighlight`。
  - 在頁面上點選：`setInspectMode` 的 `searchForNode`，以及 `nodeHighlightRequested`、`inspectNodeRequested` 事件。
  - 停在斷點時：`setPausedInDebuggerMessage`，「Paused in debugger」（繼續、逐步執行的按鈕沒做，第 8 節）。
  - DevTools 連上時送的其他 `Overlay` 命令都有回答。
- **`CSS.forcePseudoState`**（「已決定的事項」第 1 條）：Styles 的 `:hov` 強制 `:hover`、`:focus` 等。

**不在範圍內**：
- **編輯**：`DOM.setAttributeValue`、`setAttributesAsText`、`removeAttribute`、`setNodeValue`、`setOuterHTML`、`removeNode`、`moveTo`、`copyTo`、`undo`、`redo`，以及 `CSS.setStyleTexts`、`addRule`、`setRuleSelector`、`setMediaText`、`createStyleSheet`。路線寫「先唯讀」；這些命令照舊回答找不到方法，DevTools 的編輯會失敗、畫面不變。
- **斷點**：DOM 的斷點（`DOMDebugger.setDOMBreakpoint`）、事件的斷點。
- **其他面板和功能**：
  - Accessibility 面板；
  - Layers、Animations、CSS Overview、Coverage；
  - `DOMSnapshot`；
  - grid、flex、container query、scroll snap 的 overlay；
  - highlight 的 accessibility 資訊和對比度；
  - `Emulation`；
  - Performance 面板（`Tracing`）。
- **Playwright 和 Puppeteer**：它們的 `ElementHandle` 會用到這份的 `DOM` 命令，但真的連上還要路線的第 4 項。
- **renderer 重開之後還原 `DOM`、`CSS`、`Overlay` 的狀態**：照現在 `Network`、`Log` 的做法，client 要再送一次。

**要解決的 `docs/limitations.md` 的限制**（「WebDriver 和自動化」）：
- 其他的 domain（`DOM`、`CSS`、`Overlay` 等）回答找不到方法，Elements 這些面板是空的；
- Sources 的檔案樹不列 stylesheet（DevTools 從 `CSS` domain 拿）；
- 停在斷點時，畫面上沒有 Chrome 的「Paused in debugger」（沒有 `Overlay`）。

## 設計概要

### DevTools 的使用者

連上之後打開 Elements，看到的和 Chrome 一樣：
- **樹**：
  - 一開始顯示到 `<body>`，點開節點時才拿它的子節點。
  - 頁面的 script 加、刪節點或改屬性時，樹跟著變，改到的地方閃一下。
  - iframe 下面是它的文件。
  - 有 shadow root 的元素下面是 `#shadow-root (open)`；表單控制項的 user-agent shadow root 要在設定裡打開「Show user agent shadow DOM」才看得到，和 Chrome 一樣。
- **滑過和選取**：
  - 滑過樹裡的節點時，頁面上用 Chrome 的顏色框出 content（藍）、padding（綠）、border（黃）、margin（橘），旁邊有 `div.card 320 × 48` 的標籤。
  - 按「選取元素」的按鈕之後，在頁面上移動滑鼠會框出底下的元素，點一下選中它。這時頁面本身收不到這些滑鼠事件。
- **選了節點之後**：
  - **Styles**：由上而下列出 inline style、套用的規則（選擇器、宣告，被蓋掉的劃掉，`user agent stylesheet` 標出來）、`::before` 等 pseudo element 的規則、繼承來的規則和 `@keyframes`。解析不了的宣告劃掉，註解掉的是沒打勾的一條，包著規則的 `@media`、`@supports` 寫在上面。規則旁邊的「檔名:行」點下去跳到 Sources 的那一行。`:hov` 強制 `:hover`、`:focus` 和表單的狀態。
  - **Computed**：每個 property 的值，以及 box model 的圖。
  - **Event Listeners**：listener 的類型、函式和來源。
  - **Console**：`$0` 是這個節點。
- **停在斷點時**：頁面蓋上一層，上方有「Paused in debugger」（沒有 Chrome 提示上的繼續、逐步執行的按鈕，DevTools 自己有）；停住時 Console 改的東西、框的節點照樣畫。

**machopo 和 Chrome 不同的地方**（第 7 節，其他的在 `docs/limitations.md`）：
- 解析時丟掉的宣告照 Chrome 劃掉，包括 machopo 還不支援、Chrome 支援的 property 和值（Chrome 不劃）；machopo 丟掉的 at-rule（`@import`、`@layer`、`@container`、`@font-face` 等）和裡面的規則不在 Styles 裡。
- script 用 CSSOM 改過的 stylesheet，原文換成序列化的規則（Chrome 把改的規則寫進原本的原文）。
- 強制的 `:visited` 不生效；停在斷點時 CSS 動畫不往前走。

### 改這部分程式的人

- **三個新的 agent**：renderer 的 `render_thread` 加 dom、css、overlay 三個 agent，和 page、network、log agent 一樣：
  - 每個 session 各自有狀態；
  - 命令先問它們，問不到才交給 V8 的 inspector；
  - 停在斷點時也在 `answer_here` 那一串裡。
- **browser 不用改**：`DOM`、`CSS`、`Overlay` 原樣轉給 renderer。
- **DOM 的改動**：只在有 session 打開 `DOM` 時，`dom` 才把改動（連同 style 告訴它的 pseudo element 的來去）記進一個 buffer（`dom` 的 `probe`），renderer 拿走之後轉成事件。這和 DevTools 抓 stack 的做法一樣（`Dom::set_stacks_wanted`）。沒打開時，每個掛點只多一個判斷。強制的 pseudo-class 也存在 `dom`，match 時先看一個旗標。
- **要 V8 的部分**：`resolveNode`、`requestNode`、`$0`、listener 經過 `dom_api::ScriptEngine` 新的方法，V8 的實作在 `dom_js` 和 `bindings`。renderer 和 `dom_api` 照舊和 engine 無關。
- **套用的規則**：由 `style` 新的一條「說明」的路算。它和一般的 cascade 共用收集 block 的程式，重新 match 一個元素，但不寫回任何快取，也不動 selector 的 flag。規則的位置由 `css` 的同一個 parser 的記位置的模式對原文重新解析。
- **停在斷點時**：main thread 的 task 是一個 scheduler，停住的迴圈只取 DevTools 和畫 frame 的；畫 frame 要的狀態在 `Frame` 裡，停住的迴圈經過 isolate 借得到，照樣畫。
- **框出節點**：overlay 是 commit 時附加在頁面的 layer 上面的一個 layer，不經過頁面的 paint。框的位置改了只要重畫這個 layer，不重新 layout，也不重畫頁面。

## 詳細設計

### 1. agent 的位置和分派

照 Blink：每個 frame 的 agent 在 renderer，browser 的 `DevToolsSession` 只轉送。機制和 0075、0082 的一樣（`details.md` 的「RFC 開始前（2026-11-09）machopo 有什麼」第 1 項）。

- **agent 和分派**：`crates/renderer/src/render_thread/` 加 `dom_agent`、`css_agent`、`overlay_agent`。
  - `devtools_message` 的那一串加上它們，順序是 page、network、log、dom、css、overlay。
  - 停在斷點時的 `DevToolsQueueHandle::answer_here` 也加上（第 5 節）。
  - `devtools_detach` 時清掉那個 session 的狀態。
- **每個 session 各自有狀態**：照 0082 的「已決定的事項」第 10 條（S2）和 Chrome 每個 session 一份 agent，每個 session 各自有：
  - nodeId 的表、哪些節點的子節點送過；
  - 有沒有打開；
  - 強制的 pseudo-class；
  - 選取元素的模式和 highlight。
- **依賴**：照 Blink：
  - `CSS.enable` 要先有 `DOM.enable`，否則回答「DOM agent needs to be enabled first.」；
  - `Overlay.enable` 也要先有 `DOM.enable`；
  - `DOM.getDocument` 自己打開 `DOM`。
- **browser**：`DOM`、`CSS`、`Overlay` 都是 `Method::Other`，原樣轉給 renderer，不用改。renderer 重開之後不還原這三個 domain 的狀態，和 `Network`、`Log` 一樣（記在 `docs/limitations.md`）。

### 2. 節點的 id

- **`backendNodeId`**：用 `dom::NodeId`。它是 renderer 裡的 `Dom` 給的 weak id，第一次被問時才發，不重用，跨文件、跨 iframe 也不重複。這和 Blink 的 `DOMNodeIds` 一樣：weak、整個 process 一份、不受 agent 重設影響。WebDriver 的 element reference 用的也是它。
- **`nodeId`**：每個 session 一份，節點第一次送給前端時才發（Blink 的 `Bind`）。從 1 開始，同一個 session 裡不重用。
- **什麼時候清掉**：照 Blink 的 `DiscardFrontendBindings`：`getDocument`、`documentUpdated`、`disable` 時全部清掉；節點從樹上移除時，它和送過的子孫都解除（Blink 的 `Unbind`）。
- **節點怎麼留住**：照 Blink 用 strong 的（「已決定的事項」第 3 條）。
  - 每個 session 只留住不在文件的樹裡的子樹的根（每個一個 `NodeRoot`），加上 nodeId 和 `NodeId` 兩張表：綁著的節點不是在文件的樹裡（文件留著它），就是在這樣一個子樹裡（根留著它），從樹上移除時就解除。第 3 條原本寫一個 `Persistent<HeapVector<Node>>` 留住每個綁著的節點，行為一樣，但 `HeapVector` 解除一個節點要搬動後面的（O(n)），這樣不用，root 的數量也只有前端要過的 detached 的子樹那麼多。
  - 還在樹上的節點本來就活著，所以實際多留住的只有不在樹上的節點：`requestNode` 一個 detached 的物件，或在 Console 裡 `inspect()` 一個拿掉的元素。它們（連同子樹和 wrapper）活到解除為止，和 Blink 的 `Member<Node>` 一樣。
  - root 要比 heap 早 drop（elise 的規則）：session detach 時清掉，engine 收掉之前 agent 先放手。
- **pseudo element**（第 26 條）：`::before`、`::after`、`::marker` 只在 style 和 layout 裡，不是 DOM 的物件，沒有 `NodeId`。agent 給（元素, 種類）一個 nodeId 和 `backendNodeId`：`backendNodeId` 是 2^32 乘上種類（1 到 3）加上元素的 id，`NodeId` 是 32 位元，不會撞到；不另外存表，從 id 就讀得回元素和種類。有沒有它照 Blink 的 `PseudoElement::CanCreate` 的意思（`::before`、`::after` 有 `content` 而且不是 `display: none`，list item 的 `::marker`），送的順序 `::marker`、`::before`、`::after`。來去經過 style 的掛點（`set_pseudo_styles_changed`）記下；判斷要 layout（pseudo element 的樣式從 frame 拿），沒有 layout 的時候（task 之後、回答命令之前）先放著，lifecycle 之後一起處理，送 `pseudoElementAdded`、`pseudoElementRemoved`。`describeNode`、`getBoxModel` 和 `Overlay` 的框都認得它。

### 3. 樹和 lazy push

照 Blink 的 `InspectorDOMAgent`（`details.md` 的「Chromium 的對照」第 1 項）：
- **送多少**：
  - `getDocument` 送到給的深度；每個沒送子節點的容器帶 `childNodeCount`。
  - 預設兩層，照 Blink 的實作（Blink 的程式和 pdl 寫的不同，用 Chrome for Testing 152 錄過）。
  - 只有一個文字子節點的元素直接帶上它。
  - shadow root、`<template>` 的內容、pseudo element 一律帶到下一層。
- **`requestChildNodes`**：在回答之前同步送 `setChildNodes`。前端在回答之後就讀子節點，晚送的話節點點不開。
- **只有空白的文字節點**：不送，除非 `DOM.enable` 的 `includeWhitespace` 是 `all`。數子節點、找前一個兄弟節點也跳過它們。
- **iframe**：`contentDocument` 和外面的文件在同一個 id 空間。machopo 沒有 out-of-process iframe，所以前端不用 `DOM.getFrameOwner`。
- **shadow root**：放在 `shadowRoots`，帶 `shadowRootType`（`open`、`closed`、`user-agent`）。user-agent 的一律送，前端決定顯不顯示。
- **`documentUpdated`**：照 Blink，導覽之後在文件解析完時送；子 frame 換了文件時，對 `<iframe>` 送 `childNodeRemoved` 再送 `childNodeInserted`。

### 4. 改動的事件

**事件從哪裡來**：`dom` 的掛點加旗標的 buffer（M1，「已決定的事項」第 4 條，完整的比較在 `details.md` 的「改動的事件的選項」）。
- `dom` 的掛點只在旗標打開時，把（種類、節點和相關節點的 `NodeId`）推進一個 buffer，和 `dom/src/initiator.rs` 抓 stack 的做法一樣。
- 掛點都在 `crates/dom/src/style.rs` 的 `Cx`：
  - `inserted`、`removed`；
  - `attribute_changed`、`data_changed`；
  - `shadow_attached`；
  - `set_inline_style`：CSSOM 改 inline style 不經過 `attribute_changed`，照 Blink 的 `inlineStyleInvalidated`。
- renderer 經過 `ScriptEngine::dom_mut()` 拿走 buffer，不經過 `dom_api`。旗標在有 session 打開 `DOM` 時打開，最後一個關掉時關掉。

**什麼時候送**：
- 一批一批地送（「已決定的事項」第 6 條）。renderer 拿走 buffer、轉成事件的時候：每個 task 之後、microtask checkpoint 之後、回答每個 `DOM`、`CSS` 命令之前，以及進入停住的迴圈時（script 先改了 DOM 再停住，前端停住時就看得到）。Blink 在改動的當下就送。
- 照 buffer 的順序轉，節點照轉的那一刻的樣子序列化；插入的節點在同一批裡之後的屬性改動已經在序列化裡，可以略過。
- 轉的時候只看前端知道的節點：
  - 父節點沒送過的，不送；
  - 父節點的子節點沒送過的，送 `childNodeCountUpdated`；
  - 都送過了，才送 `childNodeInserted`、`childNodeRemoved`。
- 同一批裡插入之後又移除的節點，兩個都不送。
- `inlineStyleInvalidated` 一批只送一次，Blink 也是用一個延遲 0 的 timer 併起來送。

**和 Chrome 的差別**：
- `childNodeInserted` 帶的節點是轉的時候的樣子，不是插入當下的。之後同一批的屬性改動照樣送，前端最後看到的一樣。
- user-agent shadow tree 裡的改動也送，例如在文字框打字時 inner editor 的文字，和 Chrome 一樣。

**成本**：沒打開時每個掛點多讀一次旗標。插入、改屬性在 Speedometer 很熱，所以旗標盡量和掛點現有的判斷（例如 MutationObserver 的 `observed()`）放在一起。Chrome 的每個 probe 在呼叫點也先判斷一次：整個 process 一個 atomic 的 agent 位元組（`CoreProbeSink::HasAgentsGlobal`，acquire 的 load），任何一個分頁開了 DevTools，同一個 process 的頁面都走進 probe 的實作；machopo 的旗標是 `Dom` 自己的、不是 atomic，只影響開了 DevTools 的分頁。

### 5. 停在斷點時

Chrome 停在斷點時，Elements 照樣能用：Blink 在 pause 的巢狀迴圈裡照樣分派 DOM、CSS 的命令。

照 Chrome，停住時 `DOM`、`CSS` 的命令都回答，包括要 layout 和 style 的（P1+，「已決定的事項」第 5 條，比較在 `details.md` 的「停在斷點時的選項」）：
- **借得到什麼**：`Dom` 在 isolate 的 state 裡（`dom_js/src/state.rs`），JS 呼叫 DOM API 時每次從那裡拿，dispatch 事件時呼叫每個 listener 之前也放開，所以停在 JS 的敘述時沒有 Rust 的程式借著它。layout（renderer 的 `Frame`，裡面是 `FrameView`）也在 isolate 的狀態裡，不在 `RenderThread` 上。決定第 5 條時以為 `FrameView` 是 `RenderThread` 的欄位、停住時拿不到，要重新安排擁有權，所以第 1 段先做了原型；原型確認不用。
- **怎麼借**：`bindings` 的 `run_message_loop_on_pause` 把 isolate 交給停住的迴圈，`answer_here` 和 `paused` 帶著它；`dom_js` 用它做一個 page（`dom_api::DevToolsPage`）交給 renderer。不停住時 renderer 的命令也用同樣的 page（`ScriptEngine::devtools_page`）。page 給 `dom_and_layout`（DOM 和 layout）和 `objects`（`DevToolsObjects`：第 6 節的 `resolveNode`、`requestNode`、`$0`、Event Listeners）；兩者都從 isolate 的狀態借，借用跟著 page 的 `&mut self`，agent 拿著 DOM 時不能呼叫 inspector。（原型是一個帶著 `&mut Dom` 和 layout 的 struct；停住時也要呼叫 inspector，struct 拿著 DOM 的借用就不能再經過 isolate 借，所以是 trait。）
- **layout**：照 operation 借它的方式（`LayoutCell` 的 take 和 set）拿出來、回答完放回去；呼叫停住的 script 的 operation 拿著它時是空的，那時要 layout 的命令照 P2 排著。`dom_api` 借 layout 的地方（`with_layout`、`with_style`、`lend_layout`）都不在借著時執行 JS，所以實際上一直拿得到。script 執行到一半時重算 style 和 layout 本來就有（`getBoundingClientRect` 經過 `ApiCx::with_layout` 強制 layout），停住時重算是同一條路。
- 論證寫在 `crates/bindings/soundness.md` 的「停在斷點和打斷時的 DevTools 命令」：停在 JS 的敘述時，外層的 Rust 手上沒有狀態的借用；借用在 `answer_here` 回來時結束，在下一則交給 session 之前。
- 測試停在一般的 script、`click()` 呼叫的 listener、`focus()` 呼叫的 `focus` listener、`createElement` 裡的 custom element constructor、MutationObserver 的 callback、`setTimeout`、`requestAnimationFrame` 和 ResizeObserver 的 callback，都讀得到當時的 DOM、拿得到 layout，重算之後的 box 跟著 script 的改動。
- 停住時照樣畫新的 frame（框出節點、「Paused in debugger」）在第 8 節。

### 6. 和 JS 的連接

`dom_api::ScriptEngine` 加這幾個方法，wasm 的文件沒有 inspector，回答找不到：
- **`resolveNode`**：用那個 session 的 `V8InspectorSession::wrapObject`，在節點的文件所在的 frame 的 main world 的 context 裡，把節點的 wrapper（`dom_js` 的 `webdriver::node_wrapper` 已經有照 `NodeId` 拿 wrapper 的路）包成 `Runtime.RemoteObject`，帶前端給的 object group。
- **`requestNode`、`describeNode`、`highlightNode` 的 `objectId`**：用 `unwrapObject`，拿回 JS 物件之後用 bindings 的 unwrap 找到節點，再換成 `NodeId`。不是節點的回答錯誤，訊息照 Blink。
- **`setInspectedNode`**：用 `addInspectedObject`，讓 `$0` 是這個節點。它留的是 `NodeId`（weak），和 Blink 的 `InspectableNode` 留 backend id 一樣。
- **`DOMDebugger.getEventListeners`**：
  - 從節點的 listener（`Cx::listeners`）拿類型、flag 和函式。
  - 用 `v8::Function::GetScriptLocation` 拿位置，`handler` 和 `originalHandler` 包在呼叫者的 object group 裡。
  - capture 的排在前面，和 Blink 一樣。
  - 還沒編譯的屬性 handler，先編譯再回答，和 Blink 的 `GetListenerObject` 一樣。
- **Console 顯示節點**：`V8InspectorClient::valueSubtype` 對 DOM 的 wrapper 回答 `node`，`descriptionForValueSubtype` 回答 `div#app.main` 的形式（Blink 的 `ThreadDebugger`）。Console 才把它顯示成節點，滑過時送 `Overlay.highlightNode`；Console 的 `inspect()` 經過 V8 的 `Runtime.inspectRequested` 和 `requestNode` 跳到 Elements。

`resolveNode` 不管前端給的 `executionContextId`，一律在節點的 frame 的 main world 裡包：machopo 沒有 isolated world，每個 frame 只有那一個 context；rusty_v8 也沒有 `contextById` 的 binding（記在 `docs/limitations.md`）。

這些 inspector 的 API 在 rusty_v8 裡已經有 binding（`wrap_object`、`unwrap_object`、`add_inspected_object`、`value_subtype`、`description_for_value_subtype`），不用改 fork。

### 7. Styles 和 Computed

（`details.md` 的「RFC 開始前（2026-11-09）machopo 有什麼」第 3 項是開始前的現況。）

**stylesheet 的清單**：
- 每份文件和 shadow root 的 `<style>`、`<link>` 和 adopted stylesheet 各送一個 `styleSheetAdded`；`styleSheetId` 是 `CssStyleSheet` 的 id 加上 renderer 的前綴。沒有 `@import` 的子 stylesheet。
- 什麼時候送：`style` 的 `StyleEngine` 每次收集文件或 shadow root 的 stylesheet 時把一個版本加一（`sheets_version`），renderer 在每次 lifecycle 之後比較每個 frame 的版本，變了才重新列出、和每個 session 上次知道的比，送少了的 `styleSheetRemoved`、多了的 `styleSheetAdded`、內容（`Arc`）換了的 `styleSheetChanged`；沒有 session 打開 `CSS` 時不比。Blink 在 `StyleEngine` 的 active stylesheet 變了時經過 probe 通知 agent，一樣是收集的時候。
- `<style>` 的文字改了、或元素被拿掉再放回去時，machopo 建新的 stylesheet 物件，送 `styleSheetRemoved` 再送 `styleSheetAdded`，Chrome 也是（換一個新的 id）。
- header 照 Blink 的 `InspectorStyleSheet::BuildObjectForStyleSheetInfo`：`ownerNode` 是 `backendNodeId`；inline 的 `sourceURL` 是文件的網址，constructed 的是空的；`isMutable` 只有 constructed 的是 true；`loadingFailed` 只有不是 inline 的送。UA 的規則和 Chrome 一樣不送 header，規則帶 `origin: user-agent`、不帶 `styleSheetId`。

**原文**（第 11 條）：
- `<style>` 的是它現在的文字；constructed 的是 CSSOM 序列化的規則（Chrome 的 `replaceSync('.c { color: blue }')` 也給 `.c { color: blue; }\n`）。
- `<link>` 的：DevTools 連上之後載入的，留著回應（`Bytes` 共用不複製）；連上之前載入的，前端第一次要那一份的原文時照它的 `<link>` 再抓（`crossorigin` 時用 CORS，`use-credentials` 帶 credential，發起的是文件的 origin，destination 是 style），照它的 `<link>` 的方式解碼、對它的網址解析，序列化之後和現在的規則一樣才當成原文，否則給序列化的規則；結果記在那份 stylesheet 的內容上，之後不再抓。抓的時候又要的命令一起等那一次；請求在主迴圈的 task 之間送出，停在斷點時要等繼續。
- script 用 CSSOM 改過的 stylesheet（第 22 條）：`dom` 的 `CssStyleSheet` 記著 script 有沒有改過它的規則（`style_sheet_edited`，所有改動都經過 `set_style_sheet_rules`），改過的原文從此是 CSSOM 序列化的規則（一條一行），位置照它算；`insertRule` 填的空的 `<style>` 也是。沒改過的規則就是原文解析出來的，原文照用；`<link>` 補抓的仍然要比（伺服器可能換了內容）。

**位置**（第 12、21 條）：
- 前端要一份 stylesheet 的位置時，用 `css` 的同一個 parser 的記位置的模式（`css::inspector`，`TopLevelParser` 的 `Option<&mut Recorder>`）對原文再解析一次：記每條留下的規則的 prelude、每個選擇器和區塊，group rule 的子規則另外一串，`@keyframes` 的每個 keyframe；丟掉的規則不記。平常解析時是 `None`。規則照它在 stylesheet 的規則樹裡的位置（每一層的第幾條）對到重新解析出來的那一條。結果快取到那份的內容和原文換了為止。
- 宣告（第 23 條）：拿到區塊之後用同一個 `DeclarationListParser` 對區塊的文字再解析一次（`declarations_for_inspector`），記每個宣告的範圍、解析得了與否，宣告之間的註解試著解析成宣告。`cssProperties` 先照原文列每一條寫的宣告（`text`、`range`，值照寫的、`!important` 在值裡，解析不了的 `parsedOk: false`，註解掉的 `disabled: true`，shorthand 帶 `longhandProperties`），接著列解析後的每個 longhand（沒有 `text`、`range`，`!important` 的值帶著 ` !important`）。沒有原文的（UA 的）只列解析後的。
- 照 Chrome 的細節（`test.py cdp css_source` 比過）：選擇器的範圍從它的第一個 token 到它的逗號（最後一個到 prelude 的結尾），文字去掉結尾的空白；`selectorList.text` 是各個選擇器用 `, ` 接起來；最後一個沒有 `;` 的宣告的範圍到區塊的結尾；`implicit` 只在解析的結果裡、是 true 時送；值裡有 `var()` 的 shorthand 沒有 `longhandProperties`。行從 0 算、欄是 UTF-16。
- `<style>` 在 HTML 檔裡的位置（第 20 條）：html5ever 建元素時給的行是開始標籤結束的那一行（跨行的標籤是最後一行），parser 記下它；要 header 時在 `Page` agent 留著的文件原文的那一行找 `<style` 開始標籤的 `>`，有幾個時選接下來的原文和元素現在的文字一樣的那一個（原文的 CR LF 當成 LF），都不一樣（script 改過文字）時用第一個：Blink 的位置是元素的，不管文字改了沒有。沒有原文的（script、`innerHTML` 建的）是 0、0，Chrome 給 fragment parser 的位置。
- `@media`、`@supports`（第 25 條）：每條規則帶 `media`（從裡到外的每個 `@media`，最後是 stylesheet 自己的 media，它沒有 `mediaList`）、`supports` 和 `ruleTypes`，條件從規則樹往上找，位置從記下的 at-rule 的範圍來。`mediaList` 送空的，沒有 `getMediaQueries`。
- inline style（第 24 條）：每個元素一個 id（`<renderer 的前綴>.<n>`，照 Chrome 的 `<process id>.<n>`），元素還在時不變，沒有 `style` 屬性時也給（原文是空的）；不送 `styleSheetAdded`；原文是 `style` 屬性的值，宣告照上面列。它改了沒有在每次 lifecycle 之後比所有元素，只比 DevTools 要過的那幾個，改了時送一次 `styleSheetChanged`（Chrome 送兩次）。

**套用的規則**：
- `style` 有一條「說明」的路（`StyleEngine::explain`）：照 `recalc_style` 為一個元素重新跑 match，收集 block 的程式（`Walk::blocks`）和 cascade 共用，所以兩邊不會分岔；不用 matched properties cache、用新的 `SelectorCaches`、bloom filter 先放進祖先、不設 selector 的 flag（`NeedsSelectorFlags::No`；recalc 已經設過一樣的）、不寫回元素的 style。回答 block 的清單：來源的規則、層級、specificity、inline 或 attribute 的 style。
- 符合的選擇器（`matchingSelectors`）在 match 的時候記：`:host`、`::slotted()`、`::part()` 要的 matching context 和 match 時的來源綁在一起，事後另外 match 要重做那一套。`Walk` 多一個只有說明的路才有的 `matched_selectors`，一般的 match 在選擇器符合時多一次 `Option` 的判斷。同一條規則在兩個地方（同一份 stylesheet adopt 兩次）列兩次。
- 規則屬於哪一份 stylesheet（第 19 條）：index 裡的每條規則記著它的 stylesheet 在那一串（併成一個 index 的幾份）裡的第幾份；說明的路另外記下每個 block 是從哪個 index 的第幾條來的，找到那一串在文件的 stylesheet 裡從第幾份開始；shadow root 的一個 index 是一份。內容一樣的 `<style>` 共用解析的結果，也分得出來。
- 對象：`explain` 一次說明元素和它的 `::before`、`::after`、`::marker`、`::selection`（`Explained::pseudo`），符合的選擇器和 stylesheet 照對象分開記。前端拿到的 `pseudoElements` 只有元素能有的（`::marker` 要 list item）、有規則的。
- 繼承來的：每個 flat tree 上的祖先元素都說明一次，送它全部的規則（前端自己挑會繼承的 property）和它有 `style` 屬性時的 inline style，近的先；祖先的 pseudo element 只送 highlight 的（`::selection`）。
- 選了 pseudo element 的節點：它自己的規則，`inherited` 從它的元素開始，沒有 inline style（`getInlineStylesForNode` 回答 Chrome 的錯誤），`parentLayoutNodeId` 是它的元素。
- 回答的形狀照 Chrome：沒有 `style` 屬性的元素也送空的 `inlineStyle`；`parentLayoutNodeId` 是 flat tree 上的父元素（跳過 `display: contents` 的，Blink 的 `LayoutParentElement`），先把路送給前端；空的清單照 Chrome 送 `cssAtRules`，不送 `cssFunctionRules`、`cssPositionTryRules`；`shorthandEntries` 和宣告的 `important`、`implicit` 只在是 true 時送。
- inline style 是 `Cx::inline_style`；attribute 的 style 是 presentational hint 的那一個 block，HTML 的屬性和 SVG 的 presentation attribute 併在一起，和 Chrome 的 `attributesStyle` 一樣。
- 文字節點的 `getComputedStyleForNode` 是它在 flat tree 上的父元素的（Blink 的 `StyledElement`）；`getMatchedStylesForNode`、`getInlineStylesForNode` 要元素（「Node is not an Element」）。命令在 `CSS.enable` 之前回答「CSS agent was not enabled」。

**computed style**：
- 重用 `dom_api` 的 `computed_style`，也就是 `getComputedStyle` 的 property 清單和取值（包括 layout 決定的值）；pseudo element 的節點是 `getComputedStyle(element, '::before')`。
- 跟著更新（第 15 條）：`trackComputedStyleUpdatesForNode` 記下節點，每次 lifecycle 之後比較它的 `ComputedStyle` 換了沒有，換了就送 `computedStyleUpdated`。

**字型**：`getPlatformFontsForNode` 數節點自己的文字、子元素的文字和元素的產生的內容（兩層，Blink 的 `CollectPlatformFontsForLayoutObject`），從 fragment 拿實際用到的字型和 glyph 數；pseudo element 的節點數它的 `content`。順序是第一次出現的順序（Chrome 的是 hash table 的，前端自己排）。名字讀字型的 `name` table：macOS 的 Times 只有 Macintosh 的紀錄，所以 Unicode 的英文、Macintosh 的英文、第一筆依序找。

**標籤的樣式那幾列**（`elementInfo.style`，第 10 條）：顏色一律送 `#RRGGBBAA` 加上 `-unclamped-rgba`（Blink 的 `ToHEXA`；前端的 overlay 頁面照 `colorFormat` 自己寫成 `rgb(…)` 等），只有元素自己有非空白的文字時送顏色和字型。machopo 照 `inspector_overlay` 的 `createElementDescription` 畫：Color、Font、Background、Margin、Padding，透明的顏色、0px 的字型、`0px` 的 margin 和 padding 不畫，顏色照 `colorFormat` 寫（`#FF0000` 不帶 `FF`）。

**動畫**（第 27 條）：
- `getAnimatedStylesForNode`：每個生效中的 CSS 動畫和 Web Animation 單獨套在元素的基本樣式上、取當下的時間（`animation` 的 `ElementAnimations::sampled_alone`），列出它設的 property 的值（`getComputedStyle` 的寫法）和名字；元素有動畫時一律送 `transitionsStyle`（沒有在跑的 transition 時是空的）；每個祖先各一份。前端用它在 Computed 的預設清單列出動畫給的 property；Styles 沒有動畫的區塊，Chrome 也沒有（手動確認時對照過）。
- `cssKeyframesRules`：元素的 `animation-name` 每個名字一次，照 style 的 tree-scoped 的找法（`StyleEngine::keyframes_rule`，和 cascade 共用 `find_keyframes`）找到 `@keyframes`，再在 stylesheet 的規則樹裡找它的位置。`keyText` 是序列化的（`from` 寫成 `0%`），範圍是原文的。

**強制的 pseudo-class**（第 1、28、29 條）：
- 照 Blink 的 `probe::ForcePseudoState`：每個 session 記元素強制的 pseudo-class，改了時把所有 session 的合起來寫進 `dom`（`probe` 的 `ForcedStates`，每個元素一組 bit；兩個 session 都強制時兩者都成立，一個收回時另一個的還在）。
- `style` 的 `match_non_ts_pseudo_class` 在記下 state 的依賴之後先看強制的：那是 match 的熱路，先看整個 DOM 的「有沒有元素被強制」一個旗標，沒有時只多讀一次（一份 DOM 裡強制的元素很少，有時才找那張表）。強制的名字成立，它的對立面不成立，其他照元素的狀態。
- 強制 `:focus` 或 `:focus-within` 時，flat tree 上的祖先的 `:focus-within` 成立，強制 `:focus` 的元素自己的不成立：match 時從被強制的元素往上找，不記數字。Chrome 用計數，元素離開樹時不減（祖先照樣符合）、從 `focus` 加 `focus-within` 改成只有 `focus` 時也減掉；machopo 照實際的狀態。
- 改了之後經過 `state_changed` 通知，照現有的 invalidation 只重算受影響的元素。`:required`、`:optional` 原本只跟著屬性的改動重算，多了一個狀態位元（`AttributeChanges::REQUIRED`）給強制的改動用。
- 不改 DOM 上的 hover、focus 的 bit：表單控制項的原生外觀不跟著變，祖先的 hover 鏈也不動。script 的 `matches()` 和 `getComputedStyle` 看得到強制的，和 Chrome 一樣。
- 元素離開它的文件的樹時（連同子樹的每個元素）強制的狀態就沒了，放回去也一樣（Blink 的 `DidRemoveDOMNode`）；`CSS.disable`、`DOM.disable` 和 session 離開時收回那個 session 的，收回時照樣重算（Chrome 的 `DOM.disable` 只忘了強制的狀態，算好的 style 留到元素下次重算）。
- `forcedPseudoClasses` 不是陣列時照 Chrome 回答 invalid params（`-32602`）；pseudo element 的節點回答成功、不強制（它的規則照它的元素 match）。

### 8. Overlay

**畫在哪裡**：renderer 自己畫，算和畫分開（O1，「已決定的事項」第 8 條，比較在 `details.md` 的「Overlay 的選項」）。
- overlay agent 先算出一份和 Blink 的 `InspectorHighlight::AsProtocolValue` 同樣形狀的資料（`paths`、`elementInfo`；之後的 `gridInfo`、`flexInfo`），再由 machopo 自己的畫法畫成 display list。之後要換成 Chrome 的 overlay 頁面（O2）時，只換掉畫的那一半，把同一份資料交給 `inspector_overlay`。
- renderer 在 commit 時，在頁面的 layer 後面附加一個 `PictureLayer`，位置在 viewport 的座標裡（`fixed`），框和標籤是它的 display list。頁面的 layer 照舊：框的位置改了只要重畫這個 layer，不重新 layout，也不重畫頁面；頁面的 `PaintOutput` 沒變時 commit 也看 overlay 變了沒有。
- compositor thread 上捲動時，框要到下一個 main frame 才跟上，Chrome 一樣。

**框的內容**：
- 四個框：content、padding、border、margin 的 quad 從 fragment 算，經過 transform、捲動和 sticky（`layout::geometry` 的 `box_quads`，pseudo element 的是 `pseudo_box_quads`）。顏色是前端在 `highlightNode` 的 config 裡給的。
- 畫法照 `inspector_overlay` 的 `drawHighlight`：它從 margin 往內畫，每畫一個就用 `destination-out` 挖掉裡面那個框，所以每個顏色只在自己那一圈；machopo 用 even-odd 的路徑畫出同樣的一圈。一開始把四個框依序整個填滿（margin 蓋住了裡面），做標籤時和 Chrome 的截圖並排才發現。
- 標籤（第 10 條）：元素的名字、id、class（pseudo element 的節點接 `::before`，Blink 的 `BuildElementInfo`）和大小；前端的 config 要 `showStyles` 時，再加樣式那幾列（第 7 節）。文字用 `layout` 的 `shape_canvas_text` 排。位置照 `drawElementTitle`：照所有框的範圍（包含 margin）放，放得下時在上面、放不下才在下面，左邊對齊元素、留在 viewport 裡，箭頭指著元素（窄的元素指中間），會蓋到元素又沒被它包住時不畫（第 1、2 段的手動確認之後改的：原本從 border box 的下緣往下放，margin 寬時壓在 margin 上）。
- Chrome 的標籤還有 accessibility（name、role、能不能用鍵盤 focus）和對比度，這份不做（「已決定的事項」第 1 條）。
- 盒模型（`getBoxModel`、`getContentQuads`）：行內元素是它在每一行的 box 的外框，border 和 padding 取第一個 box 的起點那邊和最後一個的終點那邊，margin 和 border 一樣（machopo 的行內 box 不留 margin；Blink 有左右的 margin）。`getContentQuads` 是每一行的 box，Blink 往下拿子節點的（`CollectQuads`）。
- `getNodeForLocation` 的點在 viewport 外面時找不到節點，和 Chrome 一樣。

**選取元素**：
- `setInspectMode` 的 `searchForNode` 打開之後，滑鼠的事件在 render thread 分派給 engine 的地方（`handle_input`，移動在下一個 main frame 分派時）攔下，不交給頁面（Blink 的 `InspectorOverlayAgent::HandleInputEvent`）：按下、放開和移動都經過那裡。
- 用 hit test 找到元素，框出它，送 `nodeHighlightRequested`（先把它的路送給前端）；點下去送 `inspectNodeRequested`。`searchForUAShadowDOM` 時不往上找到 shadow host。

**停住時照樣畫，「Paused in debugger」**（「已決定的事項」第 9、16、17 條）：
- Chrome 停在斷點時照樣畫新的 frame，只是 script 的 task 不跑（2026-11-09 用 Chrome for Testing 152 試過：停住時在 Console 改背景色，截圖跟著變；`highlightNode` 也框得出節點）。machopo 照做：停住的迴圈除了 DevTools 的 task，也取 compositor 的 main frame，只跑 style、layout、paint 和 overlay，再 commit。
- 主執行緒的 task 是一個 scheduler（`task_queue::MainThreadScheduler`，第 16 條）：照來源分成幾個 queue，每個 task 帶遞增的序號，主迴圈取全部，停住的迴圈只取 DevTools 和 rendering（compositor 的 main frame、`Page.captureScreenshot` 要的 `RequestVisualState`）的，其他的（Browser 的訊息、輸入、canvas）留著、不亂順序。DevTools 的訊息一則一個 task。
- 畫 frame 要的狀態（`LayerTreeHost`、上次 paint 的結果、overlay 的 layer、等著 frame 的 promise）在 `Frame` 裡（第 17 條），停住的迴圈經過 isolate 借到它。engine 和 renderer 之間：DevTools 的佇列的 `next()` 多一種「有 frame 要畫」（`DevToolsQueued::Render`），停住的迴圈收到時呼叫 `render(page)`；每次等下一個之前呼叫 `waiting(page)`，renderer 把 DOM 的改動告訴 session，有東西要畫時才跟 compositor 要 frame。
- 停住時什麼時候要 frame：上一個 frame 之後 DOM 變了（widget 記著畫的 tree version）、overlay 變了或框著東西、有等著 frame 的 `RequestVisualState`；都沒有時不要。少了最後一項時截圖偶爾等不到（`RequestVisualState` 要了 frame，下一個 `waiting` 看到沒有變就取消了）。
- 停住時的 frame 和一般的 frame 共用 lifecycle、overlay 和 layer 的那一段（`update_lifecycle_and_layers`）；不跑 rAF、ResizeObserver、IntersectionObserver、media query 的事件、動畫的推進（第 18 條：Chrome 停住時照樣推進，記成限制）、文件載入的 promise。停在 rAF、ResizeObserver 的 callback 裡時那個 main frame 停在半路，不畫（Chrome 的截圖這時也不回答）；layout 被借出去時也不畫。
- 「Paused in debugger」照 `inspector_overlay` 的 `tool_paused.css`：頁面蓋上 31% 的黑，訊息在上方中間的淡黃色框裡（`rgb(255 255 194)`、灰色的框線），訊息是開著 `Overlay` 的 session 給的。Chrome 提示上的繼續、逐步執行的按鈕不做：停住時輸入要另外接，DevTools 自己有這兩個按鈕。

**其他命令**：前端連上時送的 `setShowViewportSizeOnResize`、`setShowGridOverlays`、`setShowFlexOverlays` 等，參數是空的或關掉時回答成功；要求 machopo 畫它沒有的 overlay 時，照 0082 的「已決定的事項」第 7 條（E1）回答錯誤（`Overlay.setShowPaintRects: machopo can't show it`），Log 的警告不送（這些命令 DevTools 只在使用者打開對應的選項時送，錯誤留在它的 log）。`setInspectMode` 的 `captureAreaScreenshot` 當成關掉。`highlightNode` 沒有 `highlightConfig` 時照 Chrome 回答 `-32602 Invalid parameters`：Chrome 的協定那一層在交給 Blink 的 agent 之前就擋下。`outlineColor` 照 Blink 和整個透明色比（`rgba(128, 0, 0, 0)` 照樣送）。

### 測試

做了的測試和驗收的結果在 `progress.md`：
- **`shell` 的整合測試**（用 `net::test_server` 給頁面）：
  - `devtools_dom`：樹、lazy push、改動的事件、一批的順序、查詢、iframe 換文件、停在斷點時、每個 session 的編號；節點和物件的來回、`$0`、Event Listeners、搜尋；pseudo element 的節點。
  - `devtools_overlay`：盒模型、某一點的節點、框和標籤（`Page.captureScreenshot` 看框）、選取元素、停住時照樣畫、停住時的輸入和捲動。
  - `devtools_css`：computed、套用的規則、字型、跟著 computed style、標籤的樣式、停住時；stylesheet 的清單、原文和補抓；規則和宣告的位置、inline style、CSSOM 改過的；pseudo element 和祖先的規則、動畫；強制的 pseudo-class。
- **單元測試**：`dom` 的 `probes`；`style` 的 `explain`；`style_resolver` 的選擇器和宣告的清單；`layout` 的 `platform_fonts`；`css` 的 `inspector`；`html` 的 `<style>` 的行；`renderer` 的 `dom_agent`、`css_agent`、`overlay_agent`、`task_queue`；`dom_js` 停住時讀 DOM 的測試。
- **`test.py cdp`**（和 Chrome 比）：`dom`、`dom_objects`、`overlay`、`css`、`css_sheets`、`css_source`、`css_pseudo`、`css_force`。`nodeId`、`backendNodeId`、`styleSheetId` 是 `VOLATILE`，DOM 的 `parentId` 是節點的編號（和 frame 的分開）；剩下的差異在 `KNOWN`，每一項附理由。
- mutation test 每一段都跑（`mutants-1a.toml` 到 `mutants-3d.toml`）。
- 手動確認（Chrome for Testing 152）：Elements 的樹、hover 和選取元素、Styles、Computed、Event Listeners、`$0`、Sources 的 stylesheet、`:hov`、停在斷點時。

### 分階段

1. **DOM 樹和 JS 的連接**（第 1 到 6 節，加上 Event Listeners）：先做停住時借 `Dom` 和 layout 的原型（第 5 條，結論是不拆），再分兩小段：
   - **1a 樹**：nodeId、lazy push、改動的掛點和事件、停住時回答，以及不用 JS 的查詢（`querySelector`、`describeNode`、`pushNodesByBackendIdsToFrontend`、`pushNodeByPathToFrontend`、`getAttributes`、`getOuterHTML`、`getFrameOwner`、`collectClassNamesFromSubtree`）。
   - **1b 和 JS 的連接**：`resolveNode`、`requestNode`、`setInspectedNode`、Console 顯示節點、Event Listeners、`performSearch`。
   - `getBoxModel`、`getContentQuads`、`getNodeForLocation` 和 `isScrollable` 要 layout 的 quad 和 hit test，和 Overlay 用同樣的東西，移到第 2 段；pseudo element 要 style 知道它在不在，和 Styles 的 pseudo element 的規則一起移到第 3 段。
2. **Overlay 和 Computed**（第 8 節，加上第 7 節的 computed style 和字型），三小段（第 9 條）：
   - **2a Overlay**：框節點、選取元素、`getBoxModel`、`getContentQuads`、`getNodeForLocation`、`isScrollable`。
   - **2b Computed**：`CSS.enable`、`getComputedStyleForNode`、`getInlineStylesForNode`、`getPlatformFontsForNode`；`getMatchedStylesForNode` 的元素自己的規則（第 14 條）；跟著更新（第 15 條）；標籤的樣式那幾列。
   - **2c 停住時照樣畫**：main thread 的 scheduler（第 16 條）、畫 frame 的狀態搬進 `Frame`（第 17 條）、「Paused in debugger」。
3. **Styles**（第 7 節的其他部分），四小段：
   - **3a stylesheet**：`styleSheetAdded`、`styleSheetRemoved`、規則的 `styleSheetId`（第 19 條）、原文（第 11 條）、`<style>` 的位置（第 20 條）。
   - **3b 位置**：記位置的模式（第 12、21 條）、選擇器和宣告的 `range`、劃掉和註解掉的宣告、`@media`、`@supports`、inline style、CSSOM 改過的（第 22 到 25 條）。
   - **3c 其他的規則**：繼承來的、pseudo element 的規則和節點（第 26 條）、`getAnimatedStylesForNode`（第 27 條；第 1、2 段的手動確認時發現 Computed 的預設清單少了動畫給的 property，加進來）、`@keyframes`。
   - **3d `:hov`**：`CSS.forcePseudoState`（第 28、29 條），它在 match 的熱路上多一次判斷，單獨一段。

三段都做完了：第 1、2 段做完、手動確認過之後 merge 回 `main`，第 3 段審核之後 merge；各段的結果在 `progress.md`。順序的理由見「已決定的事項」第 2 條：Overlay 小，樹做完之後最先想要的就是滑過時框出節點；Computed 不碰任何未決的事、查 layout 差異時最常看，跟著提前；Styles 要決定的事最多，排在最後，不擋前兩段。

## 缺點

- **DOM 的熱路上多一個判斷**：插入、移除、改屬性、改文字都多讀一次旗標（第 4 節）。
- **match 的路上多兩個判斷**：「說明」的路和一般的 cascade 共用收集 block 的程式，一般的 match 在選擇器符合時多一次 `Option` 的判斷（記下符合的選擇器），每個 pseudo-class 的 match 多讀一次「有沒有元素被強制」的旗標（第 7 節）。
- **DevTools 開著時，不在樹上的節點會被留住**：前端要過的不在樹上的節點活到解除為止（第 2 節），和 Chrome 一樣。
- **overlay 多一個 layer**：commit 的路上多一種東西，停在斷點時要另外處理（第 8 節）。
- **原文多佔記憶體，也多一次請求**：DevTools 連上之後載入的 `<link>` 留著回應（共用不複製）；連上之前載入的，第一次要原文時再抓一次，Network 面板看得到那個請求（第 11 條）。位置是要的時候重新解析原文，每份 stylesheet 多解析一次。
- **和 Chrome 不一樣的地方會留下**：Styles 看不到 machopo 丟掉的 at-rule；事件的時機是一批一批的，Chrome 是改動的當下；CSSOM 改過的 stylesheet 的原文是序列化的；強制的 `:visited` 不生效（`docs/limitations.md`）。

## 理由和其他做法

- **agent 在 renderer，不在 browser**：browser 沒有 DOM、style 和 layout。拆成 process 之後，Chromium 也是這樣分。
- **改動經過 `dom` 的掛點**：
  - 不比對前後的樹（M2）：比對的成本和前端展開的樹一樣大，每個 task 都要付，而且看不到中間的改動；之後的 DOM 斷點也需要掛點。
  - 不借用 MutationObserver（M3）：它的旗標一打開就不關，之後整頁都付建 record 的成本；它也不跨 shadow root、看不到 shadow root 的建立。
- **overlay 自己畫，不跑 Chrome 的 overlay 頁面（O2）**：
  - Chrome 的 overlay 是 devtools-frontend 的 `inspector_overlay` 的 JS 加 canvas，畫在另一個內部的頁面上。
  - machopo 要做出第二個頁面、內部的 context 和 host 的橋，還要隨 machopo 帶著那份 bundle；每次更新都要跑那個頁面的 JS 和 layout。
  - 算和畫分開，之後要 Chrome 的每一種 overlay 都一模一樣時，只換畫的那一半（「已決定的事項」第 8 條）。
- **只讀**：照路線。編輯要 undo 的歷史、`setOuterHTML` 照 Blink 的 `DOMPatchSupport` 保住節點，還要原文和位置都對得上；值得另寫一份。

完整的比較在 `details.md` 的「各個選項的比較」。

## 先例

- **Chromium**：
  - 照做的：
    - agent 的分工和依賴；
    - nodeId 和 backendNodeId 的兩層；
    - lazy push 和改動事件的規則；
    - 只有空白的文字節點的過濾；
    - `resolveNode`、`requestNode`、`$0` 經過 V8 的 inspector；
    - `getMatchedStylesForNode` 的結構，用 collecting-rules 模式的 match；
    - `forcePseudoState` 在 match 時看；
    - highlight 的顏色由前端給。
  - 不照做的：
    - overlay 頁面；
    - 改動的當下送事件；
    - 編輯和 undo。
  - 細節在 `details.md` 的「Chromium 的對照」。
- **WebKit 的 Web Inspector**：CDP 的 `DOM`、`CSS` 是從它的協定分出來的，模型相同。
- **chobitsu**（JS 寫的、在頁面裡跑的 CDP，接真的 devtools-frontend）：
  - 節點用 strong 的 `Map` 留住；
  - `getMatchedStylesForNode` 只回答套用的規則和 inline style；
  - highlight 畫在頁面裡。
  - 它是非 Blink 的 backend 接上同一個前端最接近的例子。
- **Lightpanda**：只有 Puppeteer 要的 `DOM` 的一部分，不以 Elements 面板為目標。
- **Ladybird、Servo**：講 Firefox 的 Remote Debugging Protocol，沒有參考。

## 已決定的事項

2026-11-09（使用者）：

1. **範圍：只讀，加上 `forcePseudoState`（A）**：照「動機」的目標和「不在範圍內」。Elements 面板的看（DOM 樹和跟著更新的事件、節點和 JS 的連接、Event Listeners、框出節點和選取元素、停在斷點時的提示、Styles、Computed、Sources 的 stylesheet）和 Chrome 一樣；`CSS.forcePseudoState`（Styles 的 `:hov`）也放進來：它不改 DOM、不留下東西，查 `:hover`、`:focus` 的樣式時幾乎一定要用，代價是 match 的熱路上多一個旗標的判斷和一項快取的檢查（第 7 節）。不選 B（嚴格只讀、`:hov` 也留到之後）：滑鼠移到 DevTools 時 hover 就沒了，那些規則看不到。不選 C（編輯當第 4 段放進來）：DOM 的編輯要經過 engine（custom element 的 reaction、MutationObserver、iframe 和 script 的步驟）、undo 的歷史、Edit as HTML 要照 Blink 的 `DOMPatchSupport` 保住沒變的節點，改 style 要原文和精確的位置，規模和第 3 段差不多，也會把待決定第 11、12 條綁死；另寫一份時第 3 段的原文和位置已經有了，可以直接用。
2. **分三段：DOM → Overlay 和 Computed → Styles（X'）**（「分階段」）：DOM 一定第一個（其他段都要 nodeId）。Overlay 小，樹做完之後最先想要的就是滑過時框出節點。Computed 分頁和 box model 的圖只要 `CSS.enable` 回答成功（還不送 stylesheet）、`getComputedStyleForNode` 重用 `getComputedStyle`、`getPlatformFontsForNode`，不碰任何未決的事，查 machopo 和 Chrome 的 layout 差異時最常問的就是它，所以跟著 Overlay 提前到第 2 段；前端在 `CSS.enable` 失敗時 Styles 和 Computed 都是空的，所以那時就要回答成功。第 3 段的 `CSS.enable` 開始送 `styleSheetAdded`，只是增加，前端照樣接得住。Styles 要決定的事最多（原文要先量、說明的路、重新解析），排在最後，不擋前兩段。不選 X（DOM → Overlay → Styles，Computed 跟著 Styles）：Computed 要等到最後。不選 Y（DOM → Styles → Overlay）：第 2 段變成最大、最不確定的一段，期間滑過節點都沒有框。
3. **nodeId 照 Blink 用 strong 的留住節點（K1）**（第 2 節）：每個 session 一個 `Persistent<HeapVector<Node>>` 加上 nodeId 和 `NodeId` 兩張 FxHash 的表；`backendNodeId` 是 weak 的 `NodeId`，兩層都和 Blink（`id_to_node_` 和 `DOMNodeIds`）一樣。還在樹上的節點本來就活著、移除時解除，所以實際多留住的只有前端要過的不在樹上的節點（`inspect()` 一個拿掉的元素、`requestNode` 一個還沒插入的元素），到 `getDocument`、`documentUpdated`、`disable` 或移除時解除。GC 多一個 root，綁著的節點絕大多數 marking 時本來就標過，沒有 DevTools 時沒有成本；root 要比 heap 早 drop，session detach 時清掉、engine 收掉之前 agent 先放手。不選 K2（只留 weak 的 `NodeId`）：GC 完全不多花、少一個 root，但 detached 的節點可能被收掉，Elements 回答找不到節點，和 Chrome 不同。
4. **改動的事件從 `dom` 的掛點來（M1）**（第 4 節）：有 session 打開 `DOM` 時，`Cx` 的 `inserted`、`removed`、`attribute_changed`、`data_changed`、`shadow_attached`、`set_inline_style` 把改動的種類和相關節點的 `NodeId` 推進一個 buffer，renderer 經過 `ScriptEngine::dom_mut()` 拿走、轉成事件，和 DevTools 抓 stack 的做法（`dom/src/initiator.rs`）一樣；不經過 `dom_api`，不碰 V8。每個改動依序看得到，parser、innerHTML 的快路、clone、user-agent shadow tree 也都經過這些掛點；之後的 DOM 斷點在同樣的位置加同步的路。沒打開時每個掛點多讀一次 `Dom` 的旗標，盡量和現有的判斷放在一起；Chrome 的 probe 在同樣的地方判斷一個整個 process 共用的 atomic（`CoreProbeSink::HasAgentsGlobal`），所以這是 Chrome 本來就付的成本，machopo 的還只影響開了 DevTools 的分頁。不選 M2（每個 task 之後比對前後的樹）：`dom` 不用改、沒有 DevTools 時完全沒有成本，但開著時每個 task 都要走一遍前端展開過的節點、另存一份副本，看不到中間的改動（移動像沒變），也做不了 DOM 斷點。不選 M3（內部的 MutationObserver）：`observed()` 一打開就不關，DevTools 連上過一次之後整頁都付建 record 的成本；它不跨 shadow root、看不到 shadow root 的建立，callback 和頁面自己的 observer 排在一起，可能改變頁面看到的順序。
5. **停在斷點時照 Chrome 全部回答（P1+）**（第 5 節）：停住的迴圈裡，engine 把 `Dom` 借給 renderer 的 agent，`FrameView` 也交得到那裡，`getBoxModel`、Computed、Styles 必要時當場重算 style 和 layout，和 Blink 在 pause 的巢狀迴圈裡分派 `DOM`、`CSS` 一樣。決定時估計的代價是重新安排 `RenderThread` 的擁有權（以為 `FrameView` 在 `RenderThread` 上，停住時拿不到），並確認 script 執行到一半時跑 lifecycle 是安全的，所以第 1 段一開始先做原型；太大時可以拆開（使用者授權），停住時的 layout 和 style 拆成第 1 段的另一小段，先做樹。2026-11-09 的原型之後決定不拆：`Dom` 和 layout（`FrameView` 在 renderer 交給 engine 的 `Frame` 裡）都在 engine 的 isolate 的狀態裡，停住的迴圈經過 isolate 就借得到；script 中途重算 style 和 layout 本來就有（`getBoundingClientRect` 的強制 layout），是同一條路（第 5 節）。不選 P1（停住時只回答樹，要 layout 和 style 的等繼續）：停住時 Styles、Computed 空著，查「script 跑到一半時頁面的樣子」做不到。不選 P2（停住時全部排著）：停住時 Elements 凍住，和現在一樣。
6. **改動的事件一批一批地送（E1）**（第 4 節）：renderer 在每個 task 之後、microtask checkpoint 之後、回答 `DOM`、`CSS` 命令之前和進入停住的迴圈時拿走 buffer，照順序轉成事件，節點照轉的那一刻的樣子序列化。前端看到的樹和 Chrome 一樣；不同的是事件的細節：插入的節點帶著同一批裡之後的屬性，同一批裡插入又移除的節點不送，DOM 的事件排在那個 task 的 `console` 訊息之後、不交錯。`dom` 只記、renderer 來拿，和抓 stack 的做法一樣，不用往上呼叫。之後的 DOM 斷點也是這個形狀：在 `mutate` 裡停住會和第 5 條借出 `Dom` 衝突，斷點要在操作做完、回到 JS 之前停（V8 的 `schedulePauseOnNextStatement` 這一類），不像 Blink 在 probe 裡當場 `breakProgram`。不選 E2（在掛點當下就轉成事件，照 Blink）：順序和中間狀態和 Chrome 完全一樣，但 `dom` 要在改動中呼叫 renderer 的 agent（`Dom` 裡登記往上呼叫的 callback），agent 要在 `mutate` 裡、樹不一定完整時序列化節點，還要拆成從 engine 的呼叫裡也叫得到的形狀。
7. **Event Listeners 放在第 1 段（L1）**（第 6 節）：`DOMDebugger.getEventListeners` 要的 `unwrapObject`、`wrapObject`、object group 和「JS 物件找回節點」正好是 `resolveNode`、`requestNode` 建起來的那條路，一起做、一起測，原型要確認的 inspector API 也一起確認；第 1 段做完時，樹、`$0`、Properties、Event Listeners 都有了。第 1 段照第 5 條拆成小段時，它放在樹和 JS 的連接那一小段。不選 L2（和 Styles 一起放第 3 段）：和 CSS 無關，只是延後，第 3 段本來就最大。不選 L3（之後另做）：和第 1 條的範圍衝突。

2026-11-09（使用者，第 2 段）：

8. **overlay 由 renderer 畫成一個 layer，算和畫分開（O1）**（第 8 節）：overlay agent 算出和 Blink 的 `InspectorHighlight::AsProtocolValue` 同樣形狀的資料（`paths`、`elementInfo`，之後的 `gridInfo`、`flexInfo`），machopo 自己的畫法把它畫成 display list，commit 時附加在頁面的 layer 後面（viewport 的座標）；框移動時只重畫這個 layer，不重新 layout、不重畫頁面。grid、flex 的 overlay 之後照樣做得到：幾何的材料都有（taffy 的 `DetailedGridInfo` 留在每個 grid 的 box 上、flex 的行和 item 在 fragment 裡、`PaintOp` 有虛線），要自己寫的是 `inspector_overlay` 的畫法，每個顯示選項各寫一次，畫出來接近但不是每個像素都和 Chrome 一樣。哪天要每一種 overlay 都和 Chrome 一模一樣，只換掉畫的那一半成 O2，算的那一半不用重寫。不選 O2（照 Chrome 跑 devtools-frontend 的 overlay 頁面）：畫出來一模一樣、之後的工具現成，但要第二個內部頁面和它的 context、host 的橋，帶著那份 bundle，每次更新都跑它的 JS 和 layout。決定時一度說 O2 的 JS 停住時跑不了，不對：machopo 停住時照樣能執行 JS（第 1 段的測試在停住時 `Runtime.evaluate`），停住時畫不出來的是 frame 的 commit，三個選項都一樣（第 9 條）。不選 O3（browser 畫、renderer 交出 quad）：只有停住時畫得出來這一個好處，卻每次更新多一個訊息、座標要在兩邊對齊。
9. **停住時照樣畫，照 Chrome（R）**（第 8 節）：Chrome 停在斷點時照樣畫新的 frame、只是不跑 script 的 task（2026-11-09 用 Chrome for Testing 152 試過：停住時在 Console 改背景色，截圖跟著變；`Overlay.highlightNode` 也框得出節點）。machopo 照做：停住的迴圈也接 compositor 的 `BeginMainFrame`，跑 style、layout、paint 和 overlay 再 commit，不跑 rAF、ResizeObserver；`LayerTreeHost` 和上次 paint 的結果從 `RenderThread` 拆出來讓它拿得到。得到停住時框節點、「Paused in debugger」的提示（overlay 畫的文字）、停住時改的東西看得到（`docs/limitations.md` 的那一條拿掉），之後編輯 style 時停住時也看得到。提示上的繼續、逐步執行的按鈕不做：停住時輸入也進主佇列，要另外接，DevTools 自己有這兩個按鈕。動到 render thread 的主迴圈和 compositor 的 main frame 流程，放在第 2 段的最後一小段（2c），前兩小段不靠它。不選 PM1（只讓 browser 畫提示，像 `<select>` 的選單）：停住時框節點、畫面更新都做不到，是視窗那一層的程式，做了 R 之後要拆掉。不選 PM3（這份不做）：停住時照舊沒有提示，框節點要等繼續之後。
10. **標籤是名字、大小，加上樣式（L2）**（第 8 節）：元素的名字、id、class 和大小，前端要 `showStyles` 時再加 Color（帶色塊）、Font、Background、Padding、Margin，和 Chrome 的那幾列一樣；值是 computed style，和 2b 同一條取值的路，照第 8 條先放進和 Blink 一樣形狀的 `elementInfo.style` 再畫。選取元素的模式移動時最常看的就是顏色、字型和間距，多出來的只是幾列字。不選 L1（只有名字和大小）：最小，但少了 Chrome 的標籤最常看的那幾列。不選 L3（照 Chrome 全部，加 accessibility 和對比度）：和第 1 條的範圍衝突，machopo 沒有 accessibility tree，對比度要先找出背後真正的背景色。

2026-11-09（使用者，2b）：

14. **2b 的 `getMatchedStylesForNode` 回答元素自己的規則（M2）**（第 7 節）：第 2 條以為 Computed 只要 `getComputedStyleForNode`，開始 2b 前讀 devtools-frontend 的 `ComputedStyleWidget.ts` 和 `ElementsPanel.ts`，Computed 也要 `getMatchedStylesForNode`：回答是 null 時整個分頁沒有資料；沒勾「Show all」時只列元素自己的規則和 inline style 設了的 property（加上 `display`、`width`、`height`），「哪條規則給的值」也從它來。所以把第 7 節「說明」的路裡元素自己的部分提前到 2b：符合的規則（選擇器、specificity、origin、從解析的結果序列化的宣告）、inline style 和 attribute style，規則還不帶 `styleSheetId` 和 `range`；繼承來的、pseudo element 的留在第 3 段。Computed 的預設清單和來源和 Chrome 一樣，Styles 也列出規則（沒有「檔名:行」）；它不碰第 11、12 條，第 3 段跟著變小。只在選了節點時對那個元素跑，熱路上不多東西；Blink 的這個命令一定帶 `matchedCSSRules`，走的是 `ElementRuleCollector` 收集規則的模式，和這條路一樣。不選 M1（只回答 inline 和 attribute 的 style）：Computed 預設只剩三個 property 和 inline 的，沒有來源，Styles 在第 3 段之前看起來像沒有規則套用，驗收的「Computed 和 Chrome 一樣」做不到。不選 M3（回答錯誤）：Computed 是空的。
15. **Computed 跟著 style 的改動更新（U1）**（第 7 節）：前端在 Computed 打開時送 `trackComputedStyleUpdatesForNode`，Blink 之後在那個節點的 style 變了時送 `CSS.computedStyleUpdated`。machopo 在每次 lifecycle 之後比較被追蹤的節點的 `ComputedStyle` 換了沒有（指標），換了就送；重算成同樣的值也換指標，多送一次只讓前端重抓一次。Computed 打開時每個 frame 多查一次，不在重算的熱路上（Blink 在每個元素重算時經過 `DidUpdateComputedStyle` 的 probe 比較，再用計時器合併）。不選 U2（只回答成功、不送）：前端本來就在 DOM 改動、視窗大小改變時重抓，但 `:hover`、動畫和 transition、media query 改了時 Computed 停在舊的值。

2026-11-09（使用者，2c）：

16. **主執行緒的 task 合成一個 scheduler，停住的迴圈跑同一個（Q1）**（第 8 節）：停住時 compositor 的 `BeginMainFrame` 在主佇列（crossbeam 的 channel）裡，停住的迴圈只等 DevTools 的佇列（`Mutex` 加 `Condvar`），拿不到它；停住之前就排著的那一個 compositor 等它的回覆，不會再送，整個停住的期間沒有 frame。巢狀的迴圈本身躲不掉（V8 停住時 JS 的 stack 在主執行緒上，Chrome、Node、Deno 都是巢狀的 loop），問題在 task 分在兩個地方、各有各的等法。改成一個 `MainThreadScheduler`：照來源分成幾個 task queue（browser、compositor、DevTools、canvas……），一把鎖加一個 condvar，每個 task 帶一個遞增的序號，取的時候在允許的 queue 裡取序號最小的；主迴圈取全部（順序和現在的一條 channel 一樣），停住的迴圈只取 DevTools 和 compositor 的 main frame，其他的留在各自的 queue 裡、不亂順序；script 跑到一半要中斷的訊息照樣從 DevTools 的 queue 挑。DevTools 的訊息現在鎖一次再送一次 channel，之後每個 task 只走一把鎖。對照 Chromium：一個 `SequenceManager`、多個可以停用的 task queue，停在斷點時巢狀的 run loop 跑同一個 scheduler，只停用 page 的 script 的 queue。之後停住時處理某些輸入、task 的優先順序，都是 queue 的設定。不選 F1（`BeginMainFrame` 另外放進 DevTools 佇列旁的格子，兩邊誰先拿誰做）：改動最小，但每個 frame 多鎖一次，task 還是分在兩個地方。不選 Q2（每個來源一條 channel、用 `select!` 等）：沒有自己的鎖，但 `select!` 在幾條同時有東西時隨機挑，來源之間的順序沒了，receiver 也不能先看第一個。不選 F2（停住的迴圈定時看主佇列）：拿出來的其他 task 放不回去，還要輪詢。不選 F3（停住期間改送到 DevTools 佇列）：停住之前就排著的那一個照樣卡住。
17. **畫 frame 要的狀態放進 `Frame`（S3）**（第 8 節）：停住的迴圈在 `RenderThread` 的 `&mut` 底下的 engine 的呼叫裡，碰不到 `RenderThread`，拿得到的只有 engine 經過 isolate 借出來的 `Dom` 和 layout（renderer 的 `Frame`）。`LayerTreeHost`、上次 paint 的結果、overlay 的 layer 和版本、等著 frame 的 swap promise 搬進 `Frame`，停住的迴圈照 agent 現在的路（`DevToolsPage` 的 `dom_and_layout`）拿，一般的 frame 照 `engine_frame` 拿；第一份文件之前還沒有 `Frame` 時 `RenderThread` 留著 `LayerTreeHost`，`Frame` 建立時交給它（`TextContext` 現在就是這樣）。對照 Chromium：`LayerTreeHost` 屬於 frame 的 widget（`WebFrameWidgetImpl` → `WidgetBase` → `LayerTreeView`），是 local root frame 的，不是 `RenderThread` 的。文件載入的狀態（`FrameDocument`）留在 `RenderThread`，停住時的 frame 不碰，下一個一般的 frame 再處理。layout 被借出去時停住（ResizeObserver 的 callback 在 layout 的半路），停住的迴圈拿不到 `Frame`，不畫，和停在 rAF 的 callback 裡一樣。不選 S1（拆成一個 struct，`RenderThread` 和 DevTools 的 handle 用 `Rc<RefCell<…>>` 共有）：不改擁有者，但多一個執行時的借用檢查和一份共享。

18. **停住時 CSS 動畫不往前走，記成限制（A1）**（第 8 節）：Chrome 停住時照樣推進動畫（2026-11-09 用 Chrome for Testing 152 試過，`margin-left` 的動畫半秒走了將近兩百 px），machopo 停住時的 frame 不推進。哪一邊都沒有規範說它對：停在斷點不在 web 的規範裡，HTML 的「pause」只說不跑新的 task、如有需要更新畫面；Web Animations 的 timeline 在「update animations and send events」那一步前進，推進時間和發事件、settle promise 是同一步，Chrome 的是它停住時照樣跑 `BeginMainFrame`（推進時間在裡面）、只停 page 的 script 的 queue 的結果。動畫照時間算，繼續之後兩邊在同一個 frame 回到同一個位置，script 看到的一樣，差別只在停住時畫面上看到的。照 Chrome 做要把 engine 的 `update_animations` 拆成只推進時間和發事件、settle promise 兩段，改 `dom_api` 和 `dom_js` 的介面，換到的只是停住時的畫面；之後做 Animations 面板、或別處也要只推進時間時再一起拆。不選 A2（照 Chrome 往前走）：同上的代價。

2026-11-09（使用者，第 3 段）：

11. **stylesheet 的原文：連上之後留，連上之前載入的要用時才補抓（T5）**（第 7 節）：`<style>` 的原文本來就在 DOM 的文字裡，要決定的只有 `<link>` 的回應和 constructed stylesheet 的文字。2026-11-09 用 Chrome for Testing 152 量（`styleSheetAdded` 的 `length`）：`<link>` 和 constructed 的原文，Hacker News 7 KB、MDN 110 KB、docs.rs 206 KB、Wikipedia 213 KB、apple.com 703 KB、Stack Overflow 1282 KB、GitHub 的 repo 頁 3682 KB，中位數約 200 KB。DevTools 連上之後載入的留著原文（`<link>` 的 `Bytes` 共用不複製，constructed 的留給的文字）；連上之前載入的，前端第一次要用到那一份的原文時（Sources 打開它、Styles 要它的規則的位置）才重新抓，抓回來重新解析，和現在的規則一樣才用原文，不一樣（動態產生、改過）就退回 CSSOM 序列化。不開 DevTools 時不多花記憶體：原文對頁面沒有用途。對照 Chromium：Chrome 的 inspector 從資源的快取拿原文，那筆記憶體換到的是載入速度，DevTools 只是順便用；快取被清掉時也退回 CSSOM 序列化。machopo 有了 HTTP 快取之後，補抓改成從快取拿，設計不變。不選 T1（一律留著）：沒有 HTTP 快取時等於專門為 DevTools 付記憶體，大的頁面一個分頁幾 MB。不選 T2（連上之後留、不補抓）：最常見的用法是頁面載入之後才打開 DevTools，那時的 stylesheet 都對不上原檔。不選 T3（`CSS.enable` 時全部重新抓）：一次抓全部，Network 面板多出一串請求。不選 T4（一律 CSSOM 序列化）：永遠對不上原檔，也找不回解析時丟掉的宣告。

12. **規則的位置：需要時對原文重新解析，順便記位置（R2）**（第 7 節）：前端第一次要一份 stylesheet 的位置時（Styles 的「檔名:行」、選擇器和宣告的 `range`），拿第 11 條的原文，用 `css` 的 parser 的記位置的模式再解析一次：每條規則、選擇器、宣告的範圍，連同一般的解析會丟掉的宣告和 at-rule，存在 session 裡。同一個 parser 對同樣的原文解析出同樣順序的規則，照順序對到現在的規則；原文退回 CSSOM 序列化時對序列化出來的文字做同樣的事。不開 DevTools 時不多花，parser 的一般模式不變；開了之後每份 stylesheet 多解析一次。對照 Chromium：Blink 的 `InspectorStyleSheet::ParseText` 用 `CSSParser::ParseSheetForInspector` 和 `InspectorCSSParserObserver` 對原文重新解析（`CSSRuleSourceData`），再用 cssText 的 LCS 對到 CSSOM 的規則（`InspectorDiff::FindLCSMapping`）；machopo 的 parser 兩種模式的行為一樣，照順序對就夠。不選 R1（parser 一律記位置）：不開 DevTools 時每條宣告也多存位置，規則多的頁面多出幾百 KB，和第 11 條的方向相反，丟掉的宣告也還是找不回來。不選 R3（不給位置）：Styles 沒有「檔名:行」、看不到丟掉的宣告，「Styles 和 Chrome 一樣」做不到。

19. **規則屬於哪一份 stylesheet：建 index 時記下（S1）**（第 7 節）：Styles 的規則要帶 `styleSheetId`。style 把文件裡幾份小的 stylesheet（每份少於 512 條規則）併成一個 index，內容一樣的 `<style>` 共用解析的結果和規則物件（每個元件的 shadow root 放同樣的 `<style>`），只拿 `explain` 的規則物件分不出是哪一份。建 index 時 `RuleSet` 的每條規則填它是那個 index 裡的第幾份 stylesheet（`ActiveRule::sheet`，原本一律 0），每個 scope（文件、每個 shadow root）記一份它的 stylesheet 的 id 的清單（shadow root 的 index 照內容共用，id 記在 root 自己的清單上）；`explain` match 時記下規則來自哪個 index，照它查出確切的那一份。match 的熱路上不多東西，只有 `explain` 讀它。對照 Chromium：Blink 的 `MatchedRule` 帶著它來自第幾份 stylesheet，收集給 inspector 的規則時用 `MatchRequest` 的 stylesheet 清單找回 `CSSStyleSheet`。不選 S2（在元素的 scope 裡用規則物件比對每一份）：內容一樣的 `<style>`、`insertRule` 之後的 stylesheet 共用規則物件，會指到錯的那一份。不選 S3（每份分開 match）：慢，內容一樣的兩份都符合，還是分不出來。

20. **`<style>` 在 HTML 檔裡的位置：parser 記行，要用時從原文找欄（L1）**（第 7 節）：`<style>` 的 header 帶著文字在 HTML 檔裡開始的行和欄（`startLine`、`startColumn`），Sources 和 Styles 的行號靠它。machopo 留著每份文件的原文，但 html5ever 只給行。parser 建 `<style>` 時記下開始標籤結束的那一行（一個 `<style>` 一筆）；DevTools 要 header 時在原文的那一行找它的開始標籤的 `>`，確認接下來的原文和元素現在的文字一樣，才用那個位置，不一樣、或沒有原文（script、`innerHTML` 建的）就是 0、0。不開 DevTools 時只多那幾筆行號。對照 Chromium：Blink 的 `<style>` 建立時記下 parser 的位置（`StyleElement::start_position_`，行和欄），inspector 直接用；machopo 的 tokenizer 沒有欄，用找的補上。不選 L2（完全不記，要用時掃整份原文配對）：原文裡註解、`<script>` 的字串、`<template>` 裡的 `<style` 會讓配對出錯，要重做一半的 tokenizer。不選 L3（一律 0、0）：Sources 把 inline 的 CSS 對到檔案的開頭，Styles 的行號全錯。

2026-11-09（使用者，3b）：

21. **記位置的解析：在同一個 parser 上掛 observer（P1）**（第 7 節，第 12 條的做法）：`css` 的 `TopLevelParser`、`DeclarationListParser` 多一個 `Option<&mut dyn Observer>`，解析到規則、選擇器、宣告和 at-rule 的開始、結束時把位置告訴它，連同丟掉的宣告和規則。平常解析時是 `None`，每條規則、每個宣告只多一次判斷；DevTools 要位置時才傳 observer。同一個 parser，留下和丟掉的規則一定和頁面的一樣，位置一條一條對得起來。對照 Chromium：Blink 的 `CSSParserImpl::ParseStyleSheetForInspector` 也是同一個 parser 多傳一個 `CSSParserObserver`，`InspectorStyleSheet` 的 `StyleSheetHandler` 收事件、建 `CSSRuleSourceData`。不選 P2（另外寫一個只記位置的 parser，宣告的有效與否另外問 `parse_property`）：平常的路不動，但兩套 parser 判斷丟掉哪些規則的邏輯要一直一致，不一致時位置對到別條規則，畫面正常、行號錯，很難發現。實作時的調整（3b）：observer 是具體型別的 `Option<&mut Recorder>`，只掛在 `TopLevelParser` 和規則清單的解析上；宣告在拿到區塊之後用同一個 `DeclarationListParser` 對區塊的文字再解析一次（`declarations_for_inspector`），`DeclarationListParser` 不多欄位（第 7 節）。

22. **原文和現在的規則對不上時：整份換成序列化（M2）**（第 7 節，第 12 條的做法）：script 用 CSSOM（`insertRule`、`deleteRule`、改規則的 `style`）改過的 stylesheet，原文重新解析出來的規則和現在的對不上。原文解析出來的規則序列化之後和現在的一樣時照順序一條一條對；不一樣時，那份 stylesheet 的原文換成 CSSOM 序列化的規則（一條一行），位置從它算。CSSOM 改了時送 `CSS.styleSheetChanged`（Chrome 也送）。也補上 3a 的缺口：script 用 `insertRule` 填的空的 `<style>`（CSS-in-JS 的做法）原本的原文是空的。對照 Chromium（2026-11-09 用 Chrome for Testing 152 試過）：Blink 的 `InspectorStyleSheet::MergeCSSOMRulesWithText` 用 cssText 的 LCS 把 CSSOM 的規則對到原文，對不到的規則把序列化插進原文、接在前一條對到的後面，對不到的原文留著：`deleteRule` 之後刪掉的規則還在原文裡，改了規則的 `style` 之後新舊兩份都在；`<link>` 的新規則不插、沒有位置；空的 `<style>` 給接在一起的序列化。不選 M1（照 Chrome）：Sources 看得到頁面已經沒有的規則，還要做 LCS 和插入的位置；兩者只在 script 改過 CSSOM 的 stylesheet 上不同，M2 的差別是那份的註解和排版不見。

23. **宣告照原文列，連同註解掉的（D1）**（第 7 節）：有原文的規則和 inline style，`cssProperties` 先照原文的順序列每一條寫的宣告（`text`、`range`，值照寫的、`!important` 在值裡；解析不了的標 `parsedOk: false`；shorthand 帶 `longhandProperties`），宣告區塊裡的註解在記位置的模式下試著解析成宣告，解析得出來的列成 `disabled: true`；接著列解析後的每個 longhand，沒有 `text` 和 `range`；`shorthandEntries` 照解析的結果。沒有原文的（UA 的）照現在只列解析後的。對照 Chromium（2026-11-09 用 Chrome for Testing 152 試過）：Blink 的 `InspectorStyle::PopulateAllProperties` 就是這兩部分，註解掉的宣告由 `InspectorStyleSheet` 的 `StyleSheetHandler::ObserveComment` 解析。不選 D2（不列註解掉的宣告）：省掉註解那一段，但作者註解掉的宣告 Chrome 顯示成沒打勾的一條、machopo 沒有；註解只在記位置的那次解析時看，平常的路不受影響，省下的不多。

24. **inline style 照 Chrome 有自己的 id 和原文（I1）**（第 7 節）：有 inline style 的元素各給一個 `styleSheetId`（元素還在時不變），不送 `styleSheetAdded`（Sources 不列）；原文是 `style` 屬性的值（`getStyleSheetText` 也給它），要的時候用第 21 條的模式解析、記位置，宣告照第 23 條列；inline style 改了時對要過那個 id 的 session 送 `styleSheetChanged`。對照 Chromium（2026-11-09 用 Chrome for Testing 152 試過）：Blink 的 `InspectorStyleSheetForInlineStyle` 就是這樣，id 不公告、原文是屬性值；script 改 `el.style` 時屬性值跟著改寫，送 `styleSheetChanged` 和 `DOM.inlineStyleInvalidated`。不選 I2（不做 inline style 的位置）：`element.style` 裡寫錯的宣告看不到，shorthand 照拆開的列，和規則的樣子不一致。

25. **`@media`、`@supports`：Styles 用的都送，裝置模式用的先不做（A2）**（第 7 節）：每條規則帶 `media`（從裡到外的每個 `@media`，最後是 stylesheet 自己的 media；各帶 `text`、`source`，`@media` 的再帶 `range` 和 `styleSheetId`）、`supports`（`text`、`range`、`active`）和 `ruleTypes`，條件從 stylesheet 的規則樹往上找，位置從第 21 條記下的 at-rule 的範圍來。`mediaList` 送空的，不做 `CSS.getMediaQueries`：只有裝置模式的 media query 尺規用它們，machopo 還沒有 `Emulation`，裝置模式本來就不能用，做 `Emulation` 時再補。對照 Chromium（2026-11-09 用 Chrome for Testing 152 試過）：Blink 的 `InspectorCSSAgent::BuildMediaListChain` 建出整套，`mediaList` 是拆開的條件（feature、值、單位、換算成 px 的長度、成不成立）。不選 A1（連 `mediaList` 和 `getMediaQueries` 一起做）：要把 media query 拆成 feature 和值、換算長度，現在沒有東西用得到。

2026-11-09（使用者，3c）：

26. **pseudo element 在 DOM 樹裡：DOM agent 替它編號，style 的掛點告訴它來去（N1，掛點）**（第 1 節）：machopo 的 DOM 沒有 pseudo element 的節點，`::before`、`::after`、`::marker` 只是掛在元素上的樣式（`PseudoStyles`）。DOM agent 替每個（元素，pseudo element 的種類）給一個 backend id（和真的節點的分開），`nodeId` 照一般節點給；有沒有它照 style 有沒有幫它建出盒子判斷（`::before`、`::after` 要有 `content`，`::marker` 要元素是 `list-item`），在元素的 `pseudoElements` 裡送。出現和消失：style 在元素的 pseudo element 樣式變了時呼叫的 `set_pseudo_styles_changed` 多一個 DOM 的掛點，和第 4 條的改動事件一樣，有 session 打開 `DOM` 時才記，DOM agent 拿走之後送 `DOM.pseudoElementAdded`、`pseudoElementRemoved`。選了它之後 Styles、Computed 和框都照它的樣式和盒子。不開 DevTools 時只在 pseudo element 的樣式真的變了的那一刻多一次判斷（重算時和上次不同、推進動畫時值變了），不是每個元素都走。對照 Chromium（2026-11-09 用 Chrome for Testing 152 試過）：Blink 的 pseudo element 是真的節點（`PseudoElement`，style 重算時建立），DOM agent 經過 probe（`PseudoElementCreated`、`PseudoElementDestroyed`）送事件，probe 也是「有沒有 agent」的一次判斷。不選 N2（DOM 裡建真的 pseudo element 節點）：DOM、GC 和 script 看得到的樹都要改，為了 DevTools 太大。不選 N3（樹裡不顯示）：樹上看不到 `::before`，不能選它看 Computed 和框。不選事後比對（不加掛點，每次 lifecycle 之後比對已經展開的元素）：不開 DevTools 時零成本，但開著時每個 frame 都要看一遍展開的元素，展開越多越慢。

27. **動畫的樣式：每個動畫各自算（G1）**（第 7 節）：`getAnimatedStylesForNode` 對元素的每個 CSS 動畫和 Web Animation，把它單獨套在元素的基本樣式（cascade 的結果，沒有動畫）上，取當下的時間，列出它設的 property（`properties()`）的值，寫法和 `getComputedStyle` 一樣，各帶名字（`animationStyles`）；transition 全部一起套成一組（`transitionsStyle`）；每個祖先各一份（`inherited`）。前端用它在 Styles 多一個動畫的區塊、在 Computed 的預設清單列出動畫給的 property（3c 的手動確認時更正：Styles 沒有動畫的區塊，Chrome 自己也沒有；前端只在 Computed 用它）。只在前端問的時候算（動畫跑著時大約每個 frame 一次，只算那個元素和它的祖先）。對照 Chromium（2026-11-09 用 Chrome for Testing 152 試過）：Blink 的 `InspectorCSSAgent::getAnimatedStylesForNode` 對 `ElementAnimations` 的每個 effect 各自取當下的值，transition 合成一組，再走祖先。不選 G2（所有動畫合成一份，列出和基本樣式不同的）：前端分不出每個動畫的名字，值剛好等於基本樣式的 property 會漏掉。不選 G3（不做）：Styles 沒有動畫的區塊，Computed 的預設清單少了動畫給的 property（2b 手動確認時看到的差異）。

2026-11-10（使用者，3d）：

28. **強制的 `:visited`：收下但不生效（V2）**（第 7 節）：`CSS.forcePseudoState` 收 `visited`（回答成功、記在 session 裡），但樣式不變：machopo 的 `:visited` 一律不成立，也沒有已造訪連結的那套樣式，記成限制。其他強制的 pseudo-class 照 Chrome（2026-11-10 用 Chrome for Testing 152 試過）：只對那個元素成立（祖先的 `:hover` 不跟著），強制的 `:focus` 讓祖先的 `:focus-within` 成立，script 的 `matches()` 和 `getComputedStyle` 也看得到；每次呼叫取代那個元素的整組，不認得的名字忽略；`CSS.disable`、`DOM.disable` 時清掉。對照 Chromium：Blink 的 `ForcePseudoState` 收 `visited`，cascade 為連結另外算一份 visited 的樣式（`EInsideLink::kInsideVisitedLink`），只有 `IsValidForVisitedLink` 的 property 從 visited 的規則來（試的時候 `color` 變了、`padding-left` 沒變）。不選 V1（照 Chrome 做出 visited 的樣式）：要在 cascade 加上「`:visited` 只套顏色」那一層和新的狀態位元，等於先做一半已造訪連結的支援，現在只有 DevTools 用得到；等 machopo 支援已造訪的連結時一起做。

29. **收哪些名字：照 Chrome 收整份（F1）**（第 7 節）：收 Chrome 收的 24 個：`active`、`hover`、`focus`、`focus-within`、`focus-visible`、`target`、`visited`（第 28 條），和表單的 `enabled`、`disabled`、`checked`、`indeterminate`、`placeholder-shown`、`required`、`optional`、`read-only`、`read-write`、`open`、`valid`、`invalid`、`user-valid`、`user-invalid`、`in-range`、`out-of-range`、`autofill`；其他的（`link`、`default`、`fullscreen`、`popover-open`、不認得的）忽略。強制的名字在那個元素上成立，它的對立面（`enabled` 和 `disabled`、`required` 和 `optional`、`read-only` 和 `read-write`、`valid` 和 `invalid`、`user-valid` 和 `user-invalid`、`in-range` 和 `out-of-range`）不成立，其他的照元素本來的狀態（強制 `disabled` 時 `:read-only` 不跟著）。machopo 還不 parse 的那幾個（validity 的、`autofill`）照收，頁面上沒有用到它們的規則，看不出差別。對照 Chromium（2026-11-10 用 Chrome for Testing 152 試過）：Blink 的 `InspectorCSSAgent::ForcePseudoState` 就是這份清單和這幾組；前端選了表單元素時在 `:hov` 多列表單的狀態。不選 F2（只收上面七個）：選了 `<input>` 時前端列的表單狀態勾了沒有反應；多的只是 match 時的一張對照表，和 `:required`、`:optional` 給 invalidation 用的一個狀態位元。

## 待決定

沒有。

## 之後的可能

- **編輯**：
  - DOM 的編輯和 undo（Blink 的 `DOMEditor`、`InspectorHistory`、`DOMPatchSupport`）；
  - `CSS.setStyleTexts`、`addRule` 等；
  - 編輯要經過 `ScriptEngine`，custom element 的 reaction、MutationObserver 和 iframe、script 的步驟才照常跑。
- **DOM 的斷點**：`DOMDebugger.setDOMBreakpoint`，用第 4 節的掛點。
- **其他面板和 overlay**：
  - Accessibility 面板；
  - grid、flex、container query 的 overlay；
  - highlight 的 accessibility 資訊和對比度；
  - Animations、Layers。
- **Puppeteer、Playwright 的 `ElementHandle`**：用這份的 `DOM` 命令（`describeNode`、`resolveNode`、`getBoxModel`、`getContentQuads`），還要補 `scrollIntoViewIfNeeded`，路線的第 4 項。之後 `Machopo.queryElement` 這類代替品可能可以換掉。
- **renderer 重開之後交接 session 的狀態**：`DOM`、`CSS`、`Overlay` 和 `Network`、`Log` 一起。
- **Console 的 `getEventListeners(node)`**、`monitorEvents` 等 command line API：Blink 經過 `V8InspectorClient::installAdditionalCommandLineAPI` 另外裝進 V8，不是 Event Listeners 分頁的一部分。
- **Performance 面板**（`Tracing`）。
- **強制的 `:visited`**：machopo 支援已造訪的連結（`:visited` 只套顏色類的 property 那一層）時一起做（第 28 條）。
- **`mediaList` 和 `getMediaQueries`**：做 `Emulation`（裝置模式）時再補（第 25 條）；`specificity.components`（specificity 的提示）。
- **停住時推進 CSS 動畫**：把 engine 的動畫的推進和發事件分開（第 18 條）；`Animation` domain（Animations 面板、Styles 的動畫名字旁的小圖示）。
- **「Paused in debugger」的繼續、逐步執行的按鈕**：停住時接輸入（scheduler 的 queue 的設定）。
