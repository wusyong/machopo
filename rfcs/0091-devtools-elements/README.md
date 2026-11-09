# RFC 0091：DevTools 的 Elements：`DOM`、`CSS` 和 `Overlay`

- 開始日期：2026-11-09
- 狀態：👍 已接受：2026-11-09（「已決定的事項」第 1 到 7 條：整份的範圍和分段、第 1 段；第 2、3 段的待決定在那一段開始前決定）
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
  - 停在斷點時：`setPausedInDebuggerMessage`，「Paused in debugger」和繼續、逐步執行的按鈕。
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
  - **Styles**：由上而下列出 inline style、套用的規則（選擇器、宣告，被蓋掉的劃掉，`user agent stylesheet` 標出來）、繼承來的規則、`::before` 等 pseudo element 的規則。規則旁邊的「檔名:行」點下去跳到 Sources 的那一行。
  - **Computed**：每個 property 的值，以及 box model 的圖。
  - **Event Listeners**：listener 的類型、函式和來源。
  - **Console**：`$0` 是這個節點。
- **停在斷點時**：頁面上方有「Paused in debugger」和繼續、逐步執行的按鈕，按下去和在 DevTools 裡按一樣。

**machopo 和 Chrome 不同的地方**（第 7 節）：
- machopo 解析時丟掉的東西，Styles 裡看不到：不支援的 property、無效的值、不支援的 at-rule（`@layer`、`@container`、`@font-face` 等）。Chrome 把無效的劃掉並加上警告。
- 第 7 節的選項 R2 能把它們從原文找回來。

### 改這部分程式的人

- **三個新的 agent**：renderer 的 `render_thread` 加 dom、css、overlay 三個 agent，和 page、network、log agent 一樣：
  - 每個 session 各自有狀態；
  - 命令先問它們，問不到才交給 V8 的 inspector；
  - 停在斷點時也在 `answer_here` 那一串裡。
- **browser 不用改**：`DOM`、`CSS`、`Overlay` 原樣轉給 renderer。
- **DOM 的改動**：只在有 session 打開 `DOM` 時，`dom` 才把改動記進一個 buffer，renderer 拿走之後轉成事件。這和 DevTools 抓 stack 的做法一樣（`Dom::set_stacks_wanted`）。沒打開時，每個掛點只多一個判斷。
- **要 V8 的部分**：`resolveNode`、`requestNode`、`$0`、listener 經過 `dom_api::ScriptEngine` 新的方法，V8 的實作在 `dom_js` 和 `bindings`。renderer 和 `dom_api` 照舊和 engine 無關。
- **套用的規則**：由 `style` 新的一條「說明」的路算。它照一般的 cascade 重新 match 一個元素，但不寫回任何快取，也不動 selector 的 flag。
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
  - 每個 session 一個 `Persistent<HeapVector<Node>>` 留住綁著的節點，加上 nodeId 和 `NodeId` 兩張 FxHash 的表。
  - 還在樹上的節點本來就活著，所以實際多留住的只有不在樹上的節點：`requestNode` 一個 detached 的物件，或在 Console 裡 `inspect()` 一個拿掉的元素。它們（連同子樹和 wrapper）活到解除為止，和 Blink 的 `Member<Node>` 一樣。
  - root 要比 heap 早 drop（elise 的規則）：session detach 時清掉，engine 收掉之前 agent 先放手。
- **pseudo element**：`::before`、`::after`、`::marker` 只在 style 和 layout 裡，不是 DOM 的物件，沒有 `NodeId`。agent 給（元素, `PseudoId`）一個 nodeId 和 `backendNodeId`，`backendNodeId` 從另一段數字發，和 `NodeId` 不重疊。

### 3. 樹和 lazy push

照 Blink 的 `InspectorDOMAgent`（`details.md` 的「Chromium 的對照」第 1 項）：
- **送多少**：
  - `getDocument` 送到給的深度；每個沒送子節點的容器帶 `childNodeCount`。
  - 預設的深度照 Blink 的實作。Blink 的程式和 pdl 寫的不同，開始時用 Chrome for Testing 152 錄過確認。
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

**成本**：沒打開時每個掛點多讀一次旗標。插入、改屬性在 Speedometer 很熱，所以旗標盡量和掛點現有的判斷（例如 MutationObserver 的 `observed()`）放在一起（第 1 段的實作確認）。Chrome 的每個 probe 在呼叫點也先判斷一次：整個 process 一個 atomic 的 agent 位元組（`CoreProbeSink::HasAgentsGlobal`，acquire 的 load），任何一個分頁開了 DevTools，同一個 process 的頁面都走進 probe 的實作；machopo 的旗標是 `Dom` 自己的、不是 atomic，只影響開了 DevTools 的分頁。

### 5. 停在斷點時

Chrome 停在斷點時，Elements 照樣能用：Blink 在 pause 的巢狀迴圈裡照樣分派 DOM、CSS 的命令。

**machopo 現在的情況**：
- 停住時只有 `answer_here` 那一串回答（`crates/bindings/src/devtools.rs` 的 `run_paused`），page agent 是從快取的 frame 資訊回答的。
- `Dom` 在 isolate 的 state 裡（`dom_js/src/state.rs`），JS 呼叫 DOM API 時每次從那裡拿；dispatch 事件時呼叫每個 listener 之前也放開 `Dom`（`dom_js/src/listeners.rs` 的 `call_listener`）。所以停在 JS 裡時，看起來沒有 Rust 的程式借著 `Dom`。
- layout 和 style 在 renderer 的 `FrameView` 裡，是 `RenderThread` 的欄位；停住時外層的呼叫拿著 `&mut RenderThread`，`answer_here` 拿不到它。

照 Chrome，停住時 `DOM`、`CSS` 的命令都回答，包括要 layout 和 style 的（P1+，「已決定的事項」第 5 條，比較在 `details.md` 的「停在斷點時的選項」）：
- **樹**：engine 在 `run_paused` 裡把 `Dom` 借給 `answer_here`，dom agent 照常回答。
- **layout 和 style**（`getBoxModel`、`getContentQuads`、`getNodeForLocation`，第 2、3 段的 Computed、Styles）：停住的迴圈也要拿得到 `FrameView`，必要時當場重算 style 和 layout（Blink 也會）。要重新安排 `RenderThread` 的擁有權，例如停住時由外層的呼叫把 `FrameView` 一起交給 engine，或把它從 `RenderThread` 裡拆出來另外借；還要確認 script 執行到一半時跑 lifecycle 是安全的（machopo 的 lifecycle 有沒有假設 engine 呼叫期間不重算）。
- **第 1 段一開始的原型**回答三件事：停住時 `Dom` 一定借得出來（包括停在 custom element、MutationObserver 的 callback 裡）；`FrameView` 怎麼交給停住的迴圈；停住時重算 style 和 layout 有沒有問題。
- **太大時拆開**：原型之後看規模，layout 和 style 的那部分可以拆成第 1 段的另一小段，先做樹；拆的時候寫進「分階段」和「已決定的事項」第 5 條。
- **借不到的東西**：停住時 renderer 不畫新的 frame（`LayerTreeHost` 在 render thread 上），所以停住時框出節點、「Paused in debugger」另外處理（第 8 節）。

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

這些 inspector 的 API 在 rusty_v8 裡已經有 binding（`wrap_object`、`unwrap_object`、`add_inspected_object`、`value_subtype`、`description_for_value_subtype`），預計不用改 fork；第 1 段的原型一起確認。

### 7. Styles 和 Computed

（computed style 和字型在第 2 段，其他在第 3 段；`details.md` 的「RFC 開始前（2026-11-09）machopo 有什麼」第 3 項是現況。）

- **stylesheet 的清單**：
  - 每份文件和 shadow root 的 `<style>`、`<link>` 和 adopted stylesheet 各送一個 `styleSheetAdded`。
  - `styleSheetId` 是 `CssStyleSheet` 的 id 加上 renderer 的前綴。
  - 現在沒有 `@import` 的子 stylesheet。
  - `<style>` 的文字改了、或元素被拿掉再放回去時，machopo 建新的 stylesheet 物件，所以送 `styleSheetRemoved` 再送 `styleSheetAdded`。Chrome 是同一個 id 的 `styleSheetChanged`，前端看到的結果一樣。
  - UA 的規則和 Chrome 一樣不送 header，規則帶 `origin: user-agent`、不帶 `styleSheetId`。
- **原文和位置**：
  - machopo 的 `css` 解析完不留原文（`<style>` 的在 DOM 的文字裡），也不記規則和宣告的位置。
  - 原文從哪裡來：待決定第 11 條。
  - 規則的範圍（`range`，Styles 的「檔名:行」和 Sources 靠它）怎麼得到：待決定第 12 條。
- **套用的規則**：
  - `style` 加 `StyleEngine` 的一條「說明」的路：照 `recalc_style` 為一個元素重新跑 match 和排 block。
  - 為了不影響之後的 style：
    - 不用 matched properties cache；
    - 用新的 `SelectorCaches`；
    - selector 的 flag 用 `NeedsSelectorFlags::No`；
    - bloom filter 先放進祖先；
    - 不寫回元素的 style。
  - 回答 block 的清單：來源的規則、層級、specificity、inline 或 attribute 的 style。
  - 規則屬於哪份 stylesheet：一份一份 match 來決定。文件裡小的 stylesheet 在 match 時併成一個 index，同樣文字的 `<style>` 也共用規則。
  - 前端要的 `matchingSelectors`：對選擇器清單一個一個 match。
  - 繼承來的：對 flat tree 的祖先各跑一次。
  - pseudo element 的：對 machopo 有的 pseudo（`::before`、`::after`、`::marker`、`::selection` 等）各跑一次。
  - 檢查：debug build 把說明的結果再 cascade 一次，和元素存著的 `ComputedStyle` 比，「說明」和一般的 cascade 就不會分岔。
- **computed style**：重用 `dom_api` 的 `computed_style`，也就是 `getComputedStyle` 的 property 清單和取值（包括 layout 決定的值）。
- **inline style 和 attribute 的 style**：
  - inline style 是 `Cx::inline_style`。
  - attribute 的 style 是 presentational hint 的那一個 block，HTML 的屬性和 SVG 的 presentation attribute 併在一起，和 Chrome 的 `attributesStyle` 一樣是一個 block。
- **字型**：`getPlatformFontsForNode` 從元素的文字的 fragment 拿實際用到的字型和 glyph 數。
- **強制的 pseudo-class**：
  - 照 Blink 的 `probe::ForcePseudoState`（「已決定的事項」第 1 條）：每個 session 記元素強制的 pseudo-class，`style` 的 `match_non_ts_pseudo_class` 在記下 state 的依賴之後先看強制的。
  - 那是 match 的熱路：先看「這份文件有沒有強制的狀態」一個旗標，沒有時只多讀一次。
  - 改了之後經過 `state_changed` 通知，照現有的 invalidation 只重算受影響的元素。
  - 不改 DOM 上的 hover、focus 的 bit：表單控制項的原生外觀不跟著變，祖先的 hover 鏈也不動，和 Chrome 一樣。
  - 這是新的「會影響 style 的狀態」，照 `docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」檢查。

### 8. Overlay

（第 2 段。）

**畫在哪裡**：草稿的看法是 O1（待決定第 8 條，比較在 `details.md` 的「Overlay 的選項」）。
- renderer 在 commit 時，在頁面的 layer 後面附加一個 `PictureLayer`，位置在 viewport 的座標裡（`fixed`）。框和標籤是它的 display list。
- 頁面的 layer 照舊。框的位置改了只要重畫這個 layer，不重新 layout，也不重畫頁面。
- 頁面的 `PaintOutput` 沒變時，現在會跳過 commit，要把 overlay 算進去。
- compositor thread 上捲動時，框要到下一個 frame 才跟上，Chrome 一樣。

**框的內容**：
- 四個框：content、padding、border、margin 的 quad 從 fragment 算，經過 transform、捲動和 sticky。`layout::geometry` 的 `box_to_root` 要公開。
- 顏色：前端在 `highlightNode` 的 config 裡給。
- 標籤：元素的名字、id、class 和大小。文字用 `layout` 的 `shape_canvas_text` 排。
- Chrome 的標籤還有顏色、字型、accessibility 和對比度，這份不做。

**選取元素**：
- `setInspectMode` 的 `searchForNode` 打開之後，滑鼠的移動在 `pending_mouse_move` 那裡攔下，不交給頁面。
- 用 hit test 找到元素，框出它，送 `nodeHighlightRequested`（先把它的路送給前端）。
- 點下去送 `inspectNodeRequested`。
- `searchForUAShadowDOM` 時不往上找到 shadow host。

**「Paused in debugger」**：
- 停住時 renderer 不畫新的 frame，O1 的 layer 畫不出來。
- 怎麼畫和按鈕怎麼回到 `Debugger.resume`、`stepOver`：待決定第 9 條。

**其他命令**：前端連上時送的 `setShowViewportSizeOnResize`、`setShowGridOverlays`、`setShowFlexOverlays` 等，參數是空的或關掉時回答成功；要求 machopo 做不到的事時，照 0082 的「已決定的事項」第 7 條（E1）回答錯誤，並照第 17 條送 `Log` 的警告。

### 測試

- **`shell` 的整合測試**：
  - `devtools_dom`：
    - 樹、lazy push、改動的事件；
    - iframe、shadow root、pseudo element；
    - `resolveNode` 和 `requestNode` 來回、`$0`、`getEventListeners`；
    - 停在斷點時的回答；
    - detach 和 renderer 重開。
  - `devtools_css`：stylesheet 的清單、套用的規則、computed、`forcePseudoState`。
  - `devtools_overlay`：highlight 和選取元素，用 `Page.captureScreenshot` 看框。
  - 照現有的 `devtools_network` 的寫法，用 `net::test_server` 給頁面。
- **單元測試**：
  - agent 的命令：用 channel 代替 browser，照 `log_agent` 的寫法。
  - `dom` 的掛點：旗標打開時記對、關掉時不記。
  - `style` 的說明的路：結果對，不改到快取、selector 的 flag 和計數。
- **`test.py cdp`**（和 Chrome 比）：
  - 加 `dom`、`css`、`overlay` 三組。
  - `nodeId`、`backendNodeId`、`styleSheetId` 加進 `VOLATILE`。
  - `scripts/cdp_compare.py` 現在把 `parentId` 當成 frame 的 id，和 `DOM` 的 `parentId`（nodeId）撞名，要分開。
- **mutation test**：每一段都跑。
- **手動確認**：Chrome for Testing 152 的 Elements 面板，包括樹、hover、選取元素、Styles、Computed、Event Listeners、`$0` 和停在斷點時。

### 分階段

1. **DOM 樹和 JS 的連接**（第 1 到 6 節，加上 Event Listeners）。
   - 一開始先做原型：停住時借得到 `Dom`、`FrameView` 交得到停住的迴圈、停住時重算 style 和 layout 沒有問題（第 5 節）；`valueSubtype` 讓 Console 顯示節點（第 6 節）。
   - 原型之後看規模，停住時的 layout 和 style 可以拆成另一小段（「已決定的事項」第 5 條）。
   - 驗收：Elements 面板列出樹、跟著 script 更新；`$0`；Console 的節點能跳回 Elements；Event Listeners；`test.py cdp` 的 `dom`。
2. **Overlay 和 Computed**（第 8 節，加上第 7 節的 computed style 和字型）。
   - Computed 的部分：`CSS.enable` 回答成功、還不送 stylesheet；`getComputedStyleForNode`、`getPlatformFontsForNode`。
   - 驗收：滑過樹裡的節點時頁面框出它；選取元素；停在斷點時的「Paused in debugger」；Computed 分頁和 box model 的圖和 Chrome 一樣；`test.py cdp` 的 `overlay` 和 `css` 的 computed 的部分。
3. **Styles**（第 7 節的其他部分）。
   - `CSS.enable` 開始送 `styleSheetAdded`。
   - 驗收：Styles 和 Chrome 一樣，除了 machopo 解析時丟掉的東西；Sources 列出 stylesheet；`:hov`；`test.py cdp` 的 `css`。

每一段做完、經過審核就可以先 merge 回 `main`（`rfcs/README.md` 的「流程」的「階段的 merge」）。順序的理由見「已決定的事項」第 2 條：Overlay 小，第 1 段的樹做完之後最先想要的就是滑過時框出節點；Computed 不碰任何未決的事、查 layout 差異時最常看，跟著提前；Styles 要決定的事最多（第 11、12 條），排在最後，不擋前兩段。

## 缺點

- **DOM 的熱路上多一個判斷**：插入、移除、改屬性、改文字都多讀一次旗標（第 4 節）。
- **重複了一部分 cascade 的流程**：「說明」的路要和一般的 cascade 一起維護，加新的來源（例如 `@layer`）時兩邊都要改。debug build 的比對守著（第 7 節）。
- **DevTools 開著時，不在樹上的節點會被留住**：前端要過的不在樹上的節點活到解除為止（第 2 節），和 Chrome 一樣。
- **overlay 多一個 layer**：commit 的路上多一種東西，停在斷點時要另外處理（第 8 節）。
- **原文多佔記憶體**：看待決定第 11 條怎麼選。
- **和 Chrome 不一樣的地方會留下**：Styles 看不到 machopo 丟掉的宣告和 at-rule；事件的時機是一批一批的，Chrome 是改動的當下。

## 理由和其他做法

- **agent 在 renderer，不在 browser**：browser 沒有 DOM、style 和 layout。拆成 process 之後，Chromium 也是這樣分。
- **改動經過 `dom` 的掛點**：
  - 不比對前後的樹（M2）：比對的成本和前端展開的樹一樣大，每個 task 都要付，而且看不到中間的改動；之後的 DOM 斷點也需要掛點。
  - 不借用 MutationObserver（M3）：它的旗標一打開就不關，之後整頁都付建 record 的成本；它也不跨 shadow root、看不到 shadow root 的建立。
- **overlay 自己畫，不跑 Chrome 的 overlay 頁面（O2）**：
  - Chrome 的 overlay 是 devtools-frontend 的 `inspector_overlay` 的 JS 加 canvas，畫在另一個內部的頁面上。
  - machopo 要做出第二個頁面、內部的 context 和 host 的橋，還要隨 machopo 帶著那份 bundle。
  - 而且那份 JS 停在斷點時不能跑，正好在最需要它的時候。
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
5. **停在斷點時照 Chrome 全部回答（P1+）**（第 5 節）：停住的迴圈裡，engine 把 `Dom` 借給 renderer 的 agent，`FrameView` 也交得到那裡，`getBoxModel`、Computed、Styles 必要時當場重算 style 和 layout，和 Blink 在 pause 的巢狀迴圈裡分派 `DOM`、`CSS` 一樣。代價是重新安排 `RenderThread` 的擁有權（停住時 `FrameView` 怎麼交出去），並確認 script 執行到一半時跑 lifecycle 是安全的；第 1 段一開始先做原型。太大時可以拆開（使用者授權）：原型之後看規模，停住時的 layout 和 style 拆成第 1 段的另一小段，先做樹，拆的時候寫在這裡。不選 P1（停住時只回答樹，要 layout 和 style 的等繼續）：停住時 Styles、Computed 空著，查「script 跑到一半時頁面的樣子」做不到。不選 P2（停住時全部排著）：停住時 Elements 凍住，和現在一樣。
6. **改動的事件一批一批地送（E1）**（第 4 節）：renderer 在每個 task 之後、microtask checkpoint 之後、回答 `DOM`、`CSS` 命令之前和進入停住的迴圈時拿走 buffer，照順序轉成事件，節點照轉的那一刻的樣子序列化。前端看到的樹和 Chrome 一樣；不同的是事件的細節：插入的節點帶著同一批裡之後的屬性，同一批裡插入又移除的節點不送，DOM 的事件排在那個 task 的 `console` 訊息之後、不交錯。`dom` 只記、renderer 來拿，和抓 stack 的做法一樣，不用往上呼叫。之後的 DOM 斷點也是這個形狀：在 `mutate` 裡停住會和第 5 條借出 `Dom` 衝突，斷點要在操作做完、回到 JS 之前停（V8 的 `schedulePauseOnNextStatement` 這一類），不像 Blink 在 probe 裡當場 `breakProgram`。不選 E2（在掛點當下就轉成事件，照 Blink）：順序和中間狀態和 Chrome 完全一樣，但 `dom` 要在改動中呼叫 renderer 的 agent（`Dom` 裡登記往上呼叫的 callback），agent 要在 `mutate` 裡、樹不一定完整時序列化節點，還要拆成從 engine 的呼叫裡也叫得到的形狀。
7. **Event Listeners 放在第 1 段（L1）**（第 6 節）：`DOMDebugger.getEventListeners` 要的 `unwrapObject`、`wrapObject`、object group 和「JS 物件找回節點」正好是 `resolveNode`、`requestNode` 建起來的那條路，一起做、一起測，原型要確認的 inspector API 也一起確認；第 1 段做完時，樹、`$0`、Properties、Event Listeners 都有了。第 1 段照第 5 條拆成小段時，它放在樹和 JS 的連接那一小段。不選 L2（和 Styles 一起放第 3 段）：和 CSS 無關，只是延後，第 3 段本來就最大。不選 L3（之後另做）：和第 1 條的範圍衝突。

## 待決定

每一段開始前只決定那一段的事。整份的兩條和第 1 段的先決定。

**整份**：

1. **範圍**：已決定（「已決定的事項」第 1 條）。
2. **分段和順序**：已決定（「已決定的事項」第 2 條）。

**第 1 段**：

3. **不在樹上的節點怎麼留住**：已決定（「已決定的事項」第 3 條）。
4. **改動的事件從哪裡來**：已決定（「已決定的事項」第 4 條）。
5. **停在斷點時回答 `DOM`**：已決定（「已決定的事項」第 5 條）。
6. **事件什麼時候送**：已決定（「已決定的事項」第 6 條）。
7. **Event Listeners 放在哪一段**：已決定（「已決定的事項」第 7 條）。

**第 2 段**：

8. **overlay 畫在哪裡**：
   - O1：renderer 的一個 layer（第 8 節）。
   - O2：照 Chrome 跑 overlay 頁面。
   - O3：browser 畫，renderer 交出 quad。
   - 草稿的看法：O1。
9. **「Paused in debugger」**：
   - PM1：browser 畫，像 `<select>` 的選單那樣，按鈕回到 session。browser 要知道哪個分頁停住了。
   - PM2：renderer 在停住的迴圈裡也能 commit overlay 的 layer。
   - PM3：這份不做，停住時照舊沒有提示。
   - 第 2 段開始前查 PM2 要動到多少，再比較。
10. **標籤的內容**：草稿的看法是名字、id、class 和大小，不做顏色、字型、accessibility 和對比度。

**第 3 段**：

11. **stylesheet 的原文從哪裡來**：
    - T1：一律留著 `<link>` 的回應的 `Bytes` 和 constructed stylesheet 的文字。`Bytes` 共用不複製，但要多留記憶體；0082 的「已決定的事項」第 18 條為了這個沒選一律留子資源的 body。
    - T2：只在有 DevTools 連上時留，連上之前載入的用 CSSOM 序列化。
    - T3：`CSS.enable` 時重新抓。Chrome 只從快取拿，machopo 沒有快取，會真的再發請求，內容也可能不同。
    - T4：一律用 CSSOM 序列化。行號對不上原檔。
    - 第 3 段開始前量一下典型頁面的 stylesheet 原文有多大，再決定。
12. **規則的位置**：
    - R1：`css` 的 parser 一律記位置，每次解析都付記憶體。
    - R2：照 Blink，要的時候用 observer 對原文重新解析一次，順便找回 machopo 丟掉的宣告和 at-rule，讓 Styles 照 Chrome 把它們劃掉。
    - R3：不給位置，Styles 沒有「檔名:行」。
    - 草稿的看法：R2。
13. **`forcePseudoState` 放不放進這份**：已決定，放（「已決定的事項」第 1 條）。

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
- **Puppeteer、Playwright 的 `ElementHandle`**：用這份的 `DOM` 命令（`describeNode`、`resolveNode`、`getBoxModel`、`getContentQuads`、`scrollIntoViewIfNeeded`），路線的第 4 項。之後 `Machopo.queryElement` 這類代替品可能可以換掉。
- **renderer 重開之後交接 session 的狀態**：`DOM`、`CSS`、`Overlay` 和 `Network`、`Log` 一起。
- **Console 的 `getEventListeners(node)`**、`monitorEvents` 等 command line API：Blink 經過 `V8InspectorClient::installAdditionalCommandLineAPI` 另外裝進 V8，不是 Event Listeners 分頁的一部分。
- **Performance 面板**（`Tracing`）。
