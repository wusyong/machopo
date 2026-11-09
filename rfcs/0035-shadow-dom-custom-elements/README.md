# RFC 0035：shadow DOM 和 custom element

- 開始日期：2026-10-12
- 狀態：✅ 已完成：2026-10-12（第 1 到 11 條開始前接受，第 12 到 15 條在第 1 段、第 16 到 18 條在第 2 段、第 19 到 21 條在第 3 段、第 22 到 24 條在第 4 段開始前定下，第 25、26 條在第 4 段實作時定下；四段都在 Fedora 44（Linux x86_64）上做，前三段做完時各自先 merge；動到 GC 而要跑的 `test.py matrix`、`fuzz` 在 merge 之後看 CI；驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 K-1，DOM 線接在 J（RFC 0034）之後：shadow tree、slot、事件的 retarget、custom element 的 registry、reaction 和 upgrade。`:host`、`::slotted`、style 的範圍和 flat tree 的 layout 是 K-2（CSS 線），constructable stylesheet 和 `adoptedStyleSheets` 是 RFC 0016 的第 3 段（CSS 線），都不在這份。

## 摘要

加上 `attachShadow`（`open`、`closed`）和 `ShadowRoot`、`<slot>` 的 assignment 和 `slotchange`、經過 shadow 邊界的事件路徑（`composed`、retarget 的 `target` 和 `relatedTarget`、`composedPath()`），以及 autonomous custom element：`customElements`（`define`、`get`、`getName`、`whenDefined`、`upgrade`）、`HTMLElement` 的建構、四種 lifecycle callback、`[CEReactions]` 和 upgrade。`ShadowRoot` 是 `dom` 的一種節點（`NodeKindData` 的新 variant，照 Blink 的 `ShadowRoot : DocumentFragment`），host 在 rare data 裡指向它；custom element 的 definition 和 reaction 的佇列是 `dom` 的 GC 資料，reaction 在宣告表每次呼叫的結尾（現在跑 `update_elements` 的地方）執行，那時不拿著 DOM，可以跑 script。wasm 的 guest 也能定義 custom element（第 1 條）：用 callback 的 record 定義，guest 的 export 建構元素和接 callback，wasm 呼叫的成員不算 `[CEReactions]` 的範圍，同一個 Store 的 callback 經過 backup element queue 在 microtask 執行；C、Rust 用 WIT 的 binding，Lua 用 generator 產生的 `define`。shadow tree 的繪製等 K-2：這份做完時，shadow tree 裡的節點沒有 box、沒有 style。分四段：shadow tree 和事件、custom element、slot、wasm 的 custom element。

## 動機

**現況（2026-10-12，`main` 的 `63b6ed1e`）**：
- 沒有 shadow DOM 和 custom element（`docs/limitations.md` 的「WebDriver 和自動化」的 wdspec 那一條也提到）。`dom` 的每個節點只有一條指向 document 的 edge（「Blink 的 `Node::tree_scope_`，沒有 shadow tree」），`isConnected`、`getRootNode` 都是往上走到根；`EventInit.composed`、`GetRootNodeOptions.composed` 在宣告表裡，沒有作用；`HTMLElement` 不能建構（`Illegal constructor`），合法的 custom element 名稱建出的是一般的 `HTMLElement`。
- **Speedometer**（RFC 0012 的盤點，2026-10-11；`details.md` 的「Speedometer 怎麼用」）：default 的 20 個 suite 裡有三個卡在這裡。TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM 現在卡在更前面的 `CSSStyleSheet`，解開之後接著就是 `customElements`、`HTMLElement` 的建構和 `attachShadow`。Perf-Dashboard 卡在 `alert is not defined`，原因其實是 shadow DOM：它檢查 `Element.prototype.attachShadow`，沒有就呼叫 `alert`。所以 M3 的兩個 suite 要這份加上 RFC 0016 的第 3 段，Perf-Dashboard（M4）要這份。
- **盤點的計數原本偏多**：調查時發現盤點的工具把每個 suite 載入時的呼叫記到前一個 suite 上，RFC 0012 在 2026-10-10、2026-10-11 記的「`attachShadow` 6 個 suite、custom element 4 個」實際上各是 3 個（`details.md` 的「盤點的歸屬錯誤」）。2026-10-12 在 RFC 0012 修好、重跑（它的 `progress.md`）：`attachShadow`、`customElements.define` 各 3 個 suite，`window.customElements` 4 個（多了只做 feature detection 的 Angular-Complex-DOM）。
- WPT：`dom/events/`、`dom/nodes/` 有 6 個檔案因為要 shadow DOM 或 custom element 而 `disabled`，還有一批預期失敗（`rootNode`、`Node-isConnected-shadow-dom`、`Node-appendChild-cereactions-vs-script`……）；`shadow-dom/`、`custom-elements/` 都還沒匯入。

目標：
- `element.attachShadow({ mode })`、`element.shadowRoot`（`closed` 的是 `null`）、`ShadowRoot` 的成員（`mode`、`host`、`delegatesFocus`、`slotAssignment`、`clonable`、`serializable`、`innerHTML`、`getHTML()`、`activeElement`、`getElementById` 等 `DocumentFragment` 的成員；kun 的 `DocumentFragment` 現在沒有 `getElementById`，Perf-Dashboard 在 shadow root 上用它）；shadow-including 的 connected（`isConnected`、shadow tree 裡的 `<script>` 和 `<iframe>`）、`getRootNode({ composed })`、`cloneNode` 和 `importNode` 照 `clonable` 複製。
- 事件：路徑經過 shadow root 到 host，`composed: false` 的停在 shadow root；每個 listener 看到 retarget 過的 `target`、`relatedTarget`，`composedPath()` 照 `closed` 過濾；focus 的 `activeElement` 和 focus 事件照 shadow tree 調整。
- slot：`HTMLSlotElement`（`name`、`assignedNodes()`、`assignedElements()`、`assign()`）、`Element.slot`、`assignedSlot`、named 和 manual 兩種 assignment、`slotchange`（和 `MutationObserver` 共用同一個 microtask）。
- custom element：`customElements.define` / `get` / `getName` / `whenDefined` / `upgrade`；`class X extends HTMLElement` 的 `new X()`、`createElement`（同步建構）；`connectedCallback`、`disconnectedCallback`、`adoptedCallback`、`attributeChangedCallback`（`observedAttributes`）；宣告表的 `[CEReactions]`；`define` 時、插入 document 時、clone 時的 upgrade。
- wasm 的 guest：shadow DOM 和 slot 的成員；用 callback 的 record 定義 custom element（第 1 條）。
- Speedometer 的 TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM（加上 RFC 0016 的第 3 段）、Perf-Dashboard 跑得完。

不在範圍內：
- **繪製和 style**（K-2）：flat tree 的 layout、`:host`、`::slotted`、`::part`、style 的範圍（`:defined` 原本也在這裡，第 18 條改成這份做）。這份做完時，shadow tree 裡的節點不排版、不畫，host 照舊畫它的 light DOM 子節點（第 9 項；Chrome 畫的是 shadow tree）。
- constructable stylesheet、`adoptedStyleSheets`（RFC 0016 的第 3 段）。
- customized built-in element（`is=`、`define` 的 `extends`）、form-associated custom element 和 `ElementInternals`、declarative shadow DOM（`<template shadowrootmode>`、`setHTMLUnsafe` 的 shadow root）、scoped registry（`customElementRegistry`）、`::part` 和 `exportparts`、`delegatesFocus` 的 focus 委派和 shadow tree 裡的 Tab 順序（存下旗標，不改 focus 的行為）、`moveBefore` 的 `connectedMoveCallback`。Speedometer 都用不到（`details.md`），各自列在「之後的可能」。
- parser 同步建構 custom element（第 8 項的 P1）：主文件的 parser 碰到已經 define 的名稱時，先建成 undefined、排 upgrade（第 8 條的 P2）。
- wasm 的 guest 的 callback 和 JS 同樣在回到 guest 之前執行（`[CEReactions]` 的成員在 WIT 都改成 `async func`，第 1 條不選的 X3）。

## 設計概要

對寫網頁的人：

```js
class TodoItem extends HTMLElement {
  static observedAttributes = ["completed"];
  constructor() {
    super();
    this.attachShadow({ mode: "open" }).innerHTML = `<li><slot></slot></li>`;
  }
  connectedCallback() { /* 插進 document（包括別的 shadow tree 裡）之後 */ }
  attributeChangedCallback(name, oldValue, newValue) { /* setAttribute 之後、回到 script 之前 */ }
}
customElements.define("todo-item", TodoItem);   // document 裡已經有的 <todo-item> 這時 upgrade

const item = new TodoItem();                     // 或 document.createElement("todo-item")
item.shadowRoot.querySelector("li");             // 只找 shadow tree 裡的
item.dispatchEvent(new Event("toggle", { bubbles: true, composed: true }));
// 外面的 listener 看到的 event.target 是 host，不是 shadow tree 裡的節點
```

和 Chrome 一樣的地方：`closed` 的 shadow root 從外面拿不到（`shadowRoot` 是 `null`、`composedPath()` 不含它裡面的節點）；callback 在修改的 API 回到 script 之前呼叫（`[CEReactions]`），不是立刻、也不是 microtask；`define` 之前已經在 document 裡的元素在 `define` 時 upgrade，不在 document 裡的在插進 document 時 upgrade。

和 Chrome 不同（這份做完時）：shadow tree 不畫，host 畫它的 light DOM 子節點，shadow tree 裡的元素沒有幾何（`getBoundingClientRect` 是 0）、`getComputedStyle` 的值是空的，等 K-2；主文件的 parser 不同步建構（第 8 項）；wasm 的 guest 的 `create-element` 不當場建構、callback 在 microtask 執行（第 10 項）。

對改 kun 的人：
- `dom`：shadow root 是 `NodeKindData::DocumentFragment` 帶著 mode 等不變的設定（`host` 是 fragment 的 host edge），host 的 rare data 有 `shadow_root`；「shadow-including」的祖先、根和 connected 從節點往上走、在 shadow root 跳到 host（第 2 項）；slot 的 assignment 存在 shadow root 上、需要時才重算（第 6 項）；custom element 的 state、definition 和 reaction 的佇列（第 7 項）。
- `dom_api`：`shadow_root`、`slot`、`custom_elements` 模組；分派照 DOM 的「dispatch」記下每一項的 shadow-adjusted target 和 `relatedTarget`；宣告表多一個 `[CEReactions]` 的旗標。
- `dom_wasm` 和 WIT：shadow DOM 和 slot 的成員；`CustomElementRegistry` 的 `define` 收一個 callback 的 record，guest 多五個 export（`construct`、`connected`、`disconnected`、`adopted`、`attribute-changed`；`kun:dom` 0.13）。
- `dom_js`：`HTMLElement` 的 constructor（照 `new.target` 找 definition、upgrade 時換掉既有 wrapper 的 prototype）、`define` 讀 constructor 的 `prototype` 和 callback。

## 詳細設計

（比較在 `details.md` 的「選項」，項目的編號對應它；各段開始前定下的細節在「已決定的事項」。）

1. **範圍和切法**（R；第 2 條的 R1）：一份 RFC、四段（第 11 條），範圍見「動機」。shadow DOM 和 custom element 雖然各自成立，Speedometer 的三個 suite 兩者都要；custom element 的 connected 也要先有 shadow-including 的 connected。
2. **tree scope**（S）：shadow root 是 `NodeKindData::DocumentFragment` 帶著 shadow root 的設定（`shadow: Option<ShadowRootInit>`），`host` 是 fragment 原本就有的 host edge（規範的 `DocumentFragment` 的 host，`<template>` 的內容也用它），不是新的 GC 型別（`crates/dom/README.md`：新的節點種類比新的 GC 型別便宜）。原本打算另開 `NodeKindData::ShadowRoot`（草稿時的寫法），1a 實作時改成沿用 `DocumentFragment`：`NodeKind::DocumentFragment` 的 20 處檢查（`nodeType` 是 11、插入時搬子節點、序列化、`querySelector` 的根）對 shadow root 照規範都是對的，Blink 的 `ShadowRoot` 也是 `DocumentFragment`。`ObjectKind::ShadowRoot` 讓 script 看到 `ShadowRoot` 的 interface；host 的 `NodeRareData` 有 `shadow_root` 的 edge（Blink 的 `ElementRareDataVector::shadow_root_`）。每個節點照舊只有 `document` 的 edge；要 tree scope（`getRootNode`、事件路徑、`getElementById`、slot 的 assignment）時往上走到根，要 shadow-including 的根和 connected 時在 shadow root 跳到 host 繼續走。現在的 `isConnected`、`browsing_context_document`、`get_root_node` 已經是往上走，改成跨過 shadow root。第 3 條（S2）。
3. **shadow tree 和修改的操作**（第 1 段）：`attachShadow` 照 DOM 的檢查（合法的 host、已經有 shadow root 時 `NotSupportedError`）；插入、移除、adopt、clone 的既有記錄（`FrameOwnerChange`、`ScriptChange`、`FormChange`……）在 shadow tree 裡照 shadow-including 的 connected 判斷，所以 shadow tree 裡的 `<script>` 和 `<iframe>` 照樣執行和載入；adopt 一起搬 shadow tree（Blink 的 `TreeScopeAdopter`）；`cloneNode` 照 `clonable` 複製 shadow root。`querySelector` 本來就只走自己的子孫，不進 shadow tree，不用改；`getElementById` 照舊線性地找，範圍是那個 tree scope。
4. **事件路徑**（E；第 1 段）：照 DOM 的「get the parent」：節點在 shadow root 時下一步是 host，`composed: false` 的事件停在 shadow root（路徑的根是 shadow root 時）；路徑的每一項記下 shadow-adjusted 的 target、`relatedTarget`、是不是 closed tree 的根、是不是 closed tree 裡的 slot，listener 看到的 `target` 和 `relatedTarget` 是那一項的值，`composedPath()` 照它過濾。`relatedTarget` retarget 之後和 `target` 相同時不分派（`mouseover` 這類，kun 還沒有）。focus：`activeElement` 照 tree scope 調整（Blink 的 `TreeScope::AdjustedFocusedElement`），focus 事件照樣 retarget。第 4 條（E1）。
5. **slot**（第 3 段）：`<slot>` 改成 `HTMLSlotElement`；named（`slot` attribute）和 manual（`assign()`）兩種 assignment，`assign()` 的節點在 slot 的 rare data 的弱集合（`WeakSet`，Blink 的 `manually_assigned_nodes_`），節點記著它的 manual slot（弱 edge）。`slotchange`：修改的操作直接找 assigned nodes 會變的 slot（host 的子節點的插入、移除、`slot` attribute，沒有 assigned nodes 的 slot 的子節點是它的 fallback；slot 本身的插入、移除、`name`、`assign()` 比較受影響的 shadow root 前後的 assignment），signal slots 放在 `MutationObserver` 那份 agent 的狀態（`MutationState`），和 mutation record 用同一個 microtask。第 20 條（C1）。事件從被 assign 的節點往上走時經過 slot（規範的「get the parent」）。
6. **slot assignment 的時機**（A；第 3 段）：assignment 的結果是 heap 外的快取（`DomState` 的 `SlotAssignments`，以節點的 id 存每個 slot 的 assigned nodes 和每個 slottable 的 slot），帶著算出它時的 `dom_tree_version`，`assignedNodes()`、`assignedSlot`、事件路徑和之後 K-2 的 flat tree 用到時、樹改過才重算（Blink 的 `SlotAssignment::RecalcAssignment`）。第 5 條（A1）、第 19 條（S2）。
7. **custom element 的 reaction**（C、W；第 2 段）：
   - **資料**：`CustomElementRegistry` 是每個 `Window` 一個的 script 物件；definition 是 `dom` 的 GC 資料，用 `CallbackSlot` 拿著 constructor、四個 callback，加上 `observedAttributes`、名稱和 construction stack。元素的 custom element state（`undefined`、`failed`、`uncustomized`、`custom`、`precustomized`）放在元素的狀態裡（每個元素都有，Blink 放在節點的 flag；2b 時從 rare data 改過來，parser 建的大量 undefined 的元素不用配置 rare data），definition 放在 rare data（traced 的 edge）。每個元素的 reaction 佇列、element queue 的堆疊和 backup element queue 放在 heap 外的 `DomState`，用 root 拿著元素和 definition（第 16 條；原本打算放在 rare data、traced，照 Blink 的 `CustomElementReactionStack`）。
   - **排進佇列**：`dom` 的修改原語像現在記 `ElementChange` 那樣記下 reaction：插入時是 connected、upgrade，移除時是 disconnected，adopt 時是 adopted，`set_attribute`、`remove_attribute`（`attributes_changed`）時是 attributeChanged（只記 `observedAttributes` 裡的）。
   - **執行**：宣告表的成員多一個 `[CEReactions]` 的旗標，帶它的成員進來時 push 一個 element queue，結尾（現在的 `update_elements` 之後、`queue_mutation_observer_microtask` 之前）pop、依序呼叫 callback（Blink 的 `CEReactionsScope`）；不在 `[CEReactions]` 裡排進來的（parser 的 upgrade 等）進 backup element queue，用一個 microtask 跑。哪些成員有 `[CEReactions]` 用規範的 WebIDL 比對（`crates/dom_api/tests/it/spec/webidl.rs` 現在跳過 extended attribute，改成讀出 `[CEReactions]`）。結尾時不拿著 DOM，callback 跑 script 不違反「拿著 DOM 時不跑 script」的規則（`crates/dom/soundness.md`）。inserted 的 `<script>` 在 `update_elements` 裡先跑，和規範的 post-connection steps 在 reaction 之前的順序一致，RFC 0009 的第 25 條（iframe 的 insertion steps 在操作結束時才做）不用改。第 6 條（C1：每個成員的旗標）、第 7 條（W1：資料在 `dom`，constructor 和 prototype 的處理在 `dom_js`）。
   - **建構和 upgrade**：`HTMLElement` 的 constructor 在 `dom_js`（要 `new.target`，宣告表的 `ConstructFn` 拿不到）：照 `new.target` 找 definition；construction stack 有元素時（upgrade 和 `createElement` 的同步建構）回傳那個元素，wrapper 已經存在時把它的 prototype 換成 definition 的（規範的 `[[SetPrototypeOf]]`，Blink 的 `V8HTMLConstructor` 也這樣做）。`ScriptHost` 多一個 construct（`[[Construct]]`）；`define` 讀 `prototype` 和四個 callback、`observedAttributes` 也在 `dom_js`。`createElement`、`createElementNS`、`new X()` 同步建構；`define` 時照 shadow-including 的樹的順序 upgrade document 裡的元素；`whenDefined` 用 promise 表。upgrade 的 `Construct` 經過 `ScriptHost::construct_upgrading`（預設是 construct：JS 的 `super()` 從 construction stack 拿到元素；wasm 的交給 guest 的 `construct`）。`:defined` 是元素狀態（`ElementState::DEFINED`），state 改變時 style 重算（第 18 條）。
8. **parser 遇到已經 define 的名稱**（P）：規範和 Blink 在主文件的 parser 裡同步呼叫 constructor（之前先跑一次 microtask checkpoint）；kun 的 parser 在 `frame_view` 裡，renderer 拿著 `frame_view` 時 sink 不能跑 script，要先改 renderer 的結構。第 8 條（P2）：parser 照舊建出 undefined 的元素、排 upgrade（backup element queue），parser 的那一步結束時 upgrade；同步建構列在「之後的可能」。fragment parsing（`innerHTML`、`DOMParser`）照規範本來就不同步建構，只排 upgrade。Speedometer 的三個 suite 都在 module script 裡 define，元素在那之前已經 parse 完，走的都是 upgrade。
9. **繪製**（V）：這份做完時，style 和 layout 照舊走 DOM 的子節點、不進 shadow tree，host 畫它的 light DOM 子節點；shadow tree 裡的元素沒有 box，幾何的 API 照 `display: none` 的子樹回答（0，不出錯）。`getComputedStyle` 的值是空字串，不像 `display: none` 的子樹算得出值：style 不走進 shadow tree（4d 時確認，`crates/shell/tests/it/geometry.rs` 的 `elements_in_shadow_trees_have_no_boxes` 守著，記進 `docs/limitations.md`）。K-2 把 style 和 layout 換成 flat tree。第 9 條（V1）。
10. **wasm**（X；第 1、4 段）：shadow DOM 和 slot 的成員同時給 JS 和 wasm（不是 `any`，照 RFC 0008 的規則加進 WIT，只加不改，那兩段 `kun:dom` 不用升版）。custom element 也給 wasm（第 1 條），但 guest 沒有 class 和 `new.target`，定義的方式不同：
    - **定義**：`define(name, definition)`，`definition` 是 record（`CustomElementDefinitionRecord`）：`construct`、`connected`、`disconnected`、`adopted`、`attribute-changed` 是 guest 的 callback id（`construct` 以外可以沒有），加上 `observed-attributes`（只在有 `attribute-changed` 時留著）。宣告表裡 JS 的 `define`（`[CEReactions, JsOnly]`，收 class）和 WIT 的（`[WitOnly]`，`define_for_guest`）是兩個成員，共用檢查和加進 registry 之後的步驟；只有同一個前端都有的形式才算 overload。`get`、`get-name`、`when-defined`、`upgrade` 照 JS 的語意，`get` 和 `when-defined` 的值是 `construct` 的 id。
    - **export**：guest 多五個同步的 export（第 22 條，`kun:dom` 升到 0.13）：`construct(id, element) -> bool`（第 25 條：false 或 trap 時 upgrade 失敗、元素是 failed）、`connected(id, element)`、`disconnected(id, element)`、`adopted(id, element, old-document, new-document)`、`attribute-changed(id, element, name, old-value, new-value, namespace)`；元素和 document 借給那次呼叫。
    - **建構**：guest 沒有 `new X()`，用 `create-element`；`construct` 拿到 host 已經建好的元素，相當於永遠走 upgrade 的路（JS 的 constructor 在 upgrade 時拿到的也是既有的元素）。`create_element` 碰到外部 engine 的建構子時照沒有 synchronous custom elements flag 的做：元素是 undefined、排 upgrade reaction。
    - **時機**：同步的 host function 不能回呼同一個 Store（只有 `async method` 可以），所以 wasm 呼叫的成員不算 `[CEReactions]` 的範圍：期間排進來的 reaction 進 backup element queue（規範給不在 `[CEReactions]` 裡排進來的 reaction 用的機制），在 microtask 執行，也就是 guest 這次的入口（`start`、`handle-event`……）結束之後，由 renderer 的 entry host 呼叫 guest 的 export。callback 在別的 Store 時，照現在別的 Store 的 listener 的做法同步呼叫。所以 guest 的 `create-element("x-foo")` 回來時元素還沒建構、`append-child` 回來時 `connected` 還沒跑，和 JS 不同，記進 `docs/limitations.md`。
    - **SDK**：C 和 Rust 用 WIT 的 binding。Lua 的 `define` 由 generator 從 WIT 產生（第 23 條，L1：record 的欄位可以是函式、`observed_attributes` 是字串的 table）；runtime 的五個 export 把元素和 document 換成 Lua 自己的 handle（第 26 條，可以留到 callback 之後），函式出錯時回報、`construct` 回 false（Lua 的 runtime 是同一個 Store 跑所有 Lua script，trap 會停掉全部）。`about:wasm-demo` 的 C、Rust、Lua 各定義一個 custom element。
11. **和 RFC 0016 第 3 段的協調**（K）：M3 的兩個 suite 都要 `CSSStyleSheet`（建構、`replaceSync`，Lit 要走 adopted 那條路還要 `replace` 和 `Document.prototype.adoptedStyleSheets`），而 `ShadowRoot.adoptedStyleSheets` 要先有 `ShadowRoot`。第 10 條（K1）。

### 測試

- `dom`：`ShadowRoot` 節點、host 的 edge、definition 和 reaction 的佇列的 GC（`script_objects::`、`script::every_type_unwraps_only_as_itself`、`gc_stress::`、工作負載）；shadow-including 的 connected 和根。
- `dom_api`：`attachShadow` 的檢查、事件路徑和 retarget（`open` 和 `closed`、巢狀的 shadow tree、`relatedTarget`、`composedPath()`）、slot 的 assignment 和 `slotchange`、reaction 的順序（巢狀的 `[CEReactions]`、backup queue）、upgrade 的時機。
- `dom_wasm`：guest 定義 custom element、同一個 Store 的 callback 在入口結束之後才跑、`construct` 拿到既有的元素、`construct` 回 false 和 trap；Rust 和 Lua 的在 wasm 的 reftest（W6、W9）。
- `dom_js`：`class extends HTMLElement`、upgrade 換 prototype、constructor 丟例外時 state 是 `failed`、`whenDefined`、宣告表的 `[CEReactions]` 和規範的 WebIDL 一致。
- WPT：`shadow-dom/`、`custom-elements/` 整個收，不在範圍的子目錄（`declarative/`、`focus/`、`focus-navigation/`、`reference-target/`、`form-associated/`、`state/`、`reactions/customized-builtins/`）和逾時的檔案 `disabled`；`dom/events/`、`dom/nodes/` 裡原本為了 shadow DOM `disabled` 的 6 個重新看過。理由在 `tests/wpt/README.md` 和各個 `.ini`。
- Speedometer：盤點（先修好歸屬錯誤）。Perf-Dashboard 跑得完、不再缺 shadow DOM 和 custom element；TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM 在這一側沒有缺的，照舊卡在 `CSSStyleSheet`，等 RFC 0016 的第 3 段之後才跑得完、才量測（`progress.md`）。
- 情境：畫面上看不出 shadow tree（第 9 項），`about:demo` 等 K-2；`:defined` 有一個 reftest；`about:wasm-demo` 的情境等 C、Rust、Lua 的 custom element。
- mutation test：照 `docs/testing.md`。

### 分階段

第 11 條（T1；第 4 段是第 1 條加的）：

1. **shadow tree 和事件**：`attachShadow`、`shadowRoot`、`ShadowRoot` 的成員、shadow-including 的 connected 和根、插入、移除、adopt、clone、事件路徑、retarget、`composedPath()`、`activeElement`。做完 merge 回 `main`，CSS 線可以開始 RFC 0016 的第 3 段和 K-2 的設計。Perf-Dashboard 過了 `attachShadow` 的檢查。
2. **custom element**：registry、`HTMLElement` 的建構、reaction 和 `[CEReactions]`、upgrade、四個 callback。做完時三個 suite 在 custom element 這一側沒有缺的。
3. **slot**：`HTMLSlotElement`、assignment、`slotchange`、經過 slot 的事件路徑。Speedometer 用不到，K-2 的 flat tree 要它。
4. **wasm 的 custom element**（第 1 條）：`define` 的 record、guest 的 export、backup element queue 的時機、C、Rust、Lua 的 guest 和 SDK。排在 JS 的三段之後，不擋 M3。

## 缺點

- **XL**：碰到 `dom` 的樹、修改原語、GC（新的節點種類、definition、reaction 的佇列）、事件分派和宣告表；改到 GC，照 `crates/dom/soundness.md` 寫論證。
- **K-2 之前畫面不對**：有 shadow root 的頁面畫的是 light DOM，和 Chrome 不同，量 Speedometer 時這三個 suite 的 rendering 不能比。
- **往上走找 tree scope**：`getRootNode`、事件路徑、slot 的 assignment 的成本和深度成正比；Blink 每個節點一條 tree scope 的 edge 是 O(1)。量到是瓶頸時再加（第 2 項）。
- **parser 不同步建構**：已經 define 的元素在主文件裡的建構時機和 Chrome 不同（constructor 晚於子節點的 parse，`connectedCallback` 的順序也不同）；WPT 的 `custom-elements/parser/` 有一部分會失敗。
- **wasm 的 callback 比 JS 晚**：同一個 Store 的 callback 在 guest 這次的入口結束之後才跑（第 1 條），guest 不能在 `create-element`、`append-child` 回來後馬上依賴 constructor 或 `connected` 已經跑過。

## 理由和其他做法

各項的完整比較在 `details.md` 的「選項」。重點：
- 往上走找 tree scope（第 2 項）：kun 現在的 `isConnected`、`getRootNode`、事件路徑已經往上走；多一條 edge 要在跨 tree scope 搬節點時更新整個子樹，也多一份 tracing。
- reaction 放在宣告表的結尾（第 7 項）：那裡本來就在每次呼叫之後跑 `update_elements`，是 kun 裡唯一不拿著 DOM、又還沒回到 script 的地方，對應 Blink 的 `CEReactionsScope`。
- 先不在 parser 裡同步建構（第 8 項）：Speedometer 用不到，要先改 renderer 拿著 parser 的方式；做成之後的一步，不擋 M3。
- shadow tree 先不畫（第 9 項）：flat tree 的 style 和 layout 是 CSS 線的檔案（路線圖的分線），K-2 一次做對；先做一半的繪製（沒有 style 的範圍時，document 的 stylesheet 會套到 shadow tree 裡）比不畫更容易誤導。

## 先例

- **Chromium**（`details.md` 的「Chromium 的做法」，依記憶整理，實作前對原始碼確認）：`ShadowRoot` 是 `DocumentFragment` 和 `TreeScope`，每個節點有 `tree_scope_`；`SlotAssignment` 需要時重算；`EventPath` 用 `TreeScopeEventContext` 算每個 tree scope 的 target 和 `relatedTarget`；`CustomElementRegistry`、`CustomElementDefinition`（`ScriptCustomElementDefinition` 在 bindings）、`CustomElementReactionStack`、IDL 的 `[CEReactions]` 產生 `CEReactionsScope`；`HTMLConstructionSite::CreateElement` 在主文件的 parser 裡同步建構。
- **RFC 0009**（`details.md` 第 9 項）：shadow DOM 的影響的評估；`MutationObserver` 的 agent 狀態留給 `slotchange` 的位置。
- **RFC 0010**：`MutationObserver` 接在修改操作上的做法；reaction 照同一個模式記錄。

## 已決定的事項

2026-10-12 確認第 1 到 11 條（比較在 `details.md` 的「選項」）。第 1 條先確認，第 2 到 11 條照草稿的看法。

1. **X4：custom element 也給 wasm，wasm 呼叫的成員不算 `[CEReactions]` 的範圍**（2026-10-12）：shadow DOM 和 slot 兩邊都給；wasm 的 guest 用 callback 的 record 定義 custom element（第 10 項），guest 呼叫的成員排進來的 reaction 進 backup element queue、在 microtask 執行，別的 Store 的 callback 照樣同步；排成第 4 段，在 JS 的三段之後。不選 X1（custom element 只給 JS）：wasm 文件會缺一整塊 API，RFC 0012 第 7 條的例外要有理由，這裡的理由只是時機做不到和 JS 一樣，用 backup element queue 就能給出一致、說得清楚的語意；不選 X3（`[CEReactions]` 的成員在 WIT 都改成 `async func`，時機和 JS 一樣）：幾百個成員（每個 reflect 的 attribute 的 setter 也是）變成 async，`kun:dom` 要升版、C、Rust、Lua 的 guest 和測試都要改，C 的每個修改都要等 subtask，沒有定義任何 custom element 的頁面也要付每次修改一個 async subtask 的成本；也不選 X2（全部只給 JS）。

2. **R1：一份 RFC，含 slot；不含 customized built-in、form-associated、declarative shadow DOM**（2026-10-12）：Speedometer 的三個 suite 兩者都要，custom element 的 connected 要先有 shadow-including 的 connected；slot 是 K-2 的 flat tree 一定要的。不選 R2（拆成兩份：`[CEReactions]` 和 connected 在兩份之間來回引用，分段和 R1 差不多）、R3（只做 Speedometer 用到的：slot 晚點還是要回來，WPT 的大部分檔案會一起用到）、R4（加上 customized built-in：Safari 不做、Speedometer 不用，`createElement` 和 `element_kind` 都要多看 `is`）。
3. **S2：往上走找 tree scope，在 shadow root 跳到 host**（2026-10-12）：每個節點照舊只有 `document` 的 edge，`getRootNode`、事件路徑、`isConnected` 照現在的做法往上走。不選 S1（照 Blink 的 `Node::tree_scope_` 每個節點一條 edge：跨 tree scope 搬節點時要更新整個子樹、多一份 tracing），量到往上走是瓶頸時再加（「之後的可能」）。
4. **E1：事件路徑照規範，每一項記 shadow-adjusted 的 target 和 `relatedTarget`**（2026-10-12）：和規範逐行對得上，WPT 的 `composedPath()` 過濾和 `relatedTarget` 好查。不選 E2（Blink 的 `TreeScopeEventContext`：結果相同，按 tree scope 合併，kun 的路徑短，省下的不多）。
5. **A1：slot assignment 用到時才重算**（2026-10-12）：修改時只標要重算、記下要發 `slotchange` 的 slot，照 Blink 的 `SlotAssignment`。不選 A2（照規範的字面每次修改都重算：每次修改都走 host 的子節點）。
6. **C1：宣告表的 `[CEReactions]` 旗標，reaction 在呼叫的結尾執行**（2026-10-12）：帶旗標的成員 push / pop element queue（Blink 的 `CEReactionsScope`），在 `update_elements` 之後、`queue_mutation_observer_microtask` 之前；其他時候排進來的進 backup element queue；旗標用規範的 WebIDL 檢查。不選 C2（每次呼叫的結尾都跑：不是 `[CEReactions]` 的成員排進來的 reaction 會比規範和 Chrome 早跑）。
7. **W1：custom element 的資料在 `dom`，constructor 和 prototype 的處理在 `dom_js`**（2026-10-12）：registry、definition、reaction 的佇列是 `dom` 的 GC 資料，由修改原語排 reaction；`HTMLElement` 的 constructor（要 `new.target`）、`define` 讀 `prototype` 和 callback、upgrade 換 prototype 在 `dom_js`，`ScriptHost` 多 construct。照 Blink 的 `CustomElementDefinition`（core）和 `ScriptCustomElementDefinition`（bindings）的分法。改到 GC，照 `crates/dom/soundness.md` 寫論證。不選 W2（registry 寫在 `glue.js`：reaction 要在修改的當下記下來，JS 那邊看不到）。
8. **P2：parser 遇到已經 define 的名稱時建成 undefined、排 upgrade**（2026-10-12）：不動 renderer 持有 parser 的方式和 `html`；Speedometer 的三個 suite 都在 module script 裡 define，走的是 upgrade。和 Chrome 不同的時機記進 `docs/limitations.md`。不選 P1（照規範和 Blink 同步建構：renderer 要在 resume 時把 parser 從 `frame_view` 拿出來、給 sink 能跑 script 的 hook），列在「之後的可能」。
9. **V1：shadow tree 先不畫，等 K-2**（2026-10-12）：style 和 layout 不動（CSS 線的檔案），host 畫 light DOM，shadow tree 裡的元素沒有 box，記進 `docs/limitations.md`。不選 V2（host 的子節點都不畫：要改 `layout`，畫面一樣不對）、V3（這份做 flat tree 的 layout、沒有 style 的範圍：document 的 stylesheet 會套進 shadow tree，比不畫更誤導，K-2 的工作也拆到兩條線）。
10. **K1：這份的第 1 段 merge 之後，CSS 線開始 RFC 0016 的第 3 段**（2026-10-12）：和這份的第 2、3 段並行；照路線圖的分線，`ShadowRoot.adoptedStyleSheets` 有地方接。不選 K2（這份順便做最小的 `CSSStyleSheet`：Lit 要 `replace` 和 `Document.prototype.adoptedStyleSheets` 才量得到，做一半的 CSSOM 之後還是要改寫）。
11. **T1：分四段**（2026-10-12）：1 shadow tree 和事件、2 custom element、3 slot、4 wasm 的 custom element（第 1 條）。前兩段做完時 Speedometer 這一側就沒有缺的，第 1 段先 merge 讓 CSS 線開始；第 3 段補上經過 slot 的事件路徑。不選 T2（照規範的依賴把 slot 併進第 1 段：第 1 段變大，M3 晚一段）。

第 1 段開始前，2026-10-12 確認第 12 到 15 條（比較在 `details.md` 的「第 1 段的選項」）。

12. **H1：`ShadowRoot` 的 `innerHTML`、`getHTML()`、`setHTMLUnsafe()` 只處理它自己的子節點**（2026-10-12，第 1 段）：`getHTML` 的 `serializableShadowRoots`、`shadowRoots` 照舊沒有作用，和 declarative shadow DOM 一起做（「之後的可能」）；`serializable` 照樣存。不選 H2（現在就把 serializable 的 shadow root 序列化成 `<template shadowrootmode>`，Chrome 有）：kun 沒有 parse 的那一半，輸出讀回來是一般的 `<template>`。
13. **F1：第 1 段照規範寫 focus 和 shadow tree 的部分，從 script 真的 focus 進 shadow tree 等 K-2**（2026-10-12，第 1 段）：`document.activeElement` retarget 到 host、`ShadowRoot.activeElement`、focus 事件的 retarget，`dom` 的 `set_focused_element` 接受 shadow-including 的子孫；第 9 條之後 shadow tree 裡的元素沒有 box、不是 being rendered，`focus()` 對它們沒有作用（Speedometer 的按鍵是直接 `dispatchEvent`，不受影響），測試在 `dom` 那一層直接設 focus。不選 F2（focus 的部分整個延到 K-2）：retarget 的規則和事件路徑是同一件事，拆開之後 K-2（CSS 線）要回頭改 DOM 線的檔案。
14. **第 1 段分五步**（2026-10-12）：1a `ShadowRoot` 節點和 GC、`attachShadow`、`shadowRoot`、`ShadowRoot` 的屬性、`DocumentFragment.getElementById`（JS 和 wasm），和 adopt 帶著 shadow tree（原本排在 1b；`dom` 的 GC 工作負載會把有 shadow root 的元素插進別的 document，1a 就要它）；1b shadow-including 的 connected 和根、插入和移除的記錄、`clonable` 的 clone；1c 事件路徑、retarget、`composedPath()`、`relatedTarget`、`window.event`；1d `activeElement` 和 focus 事件（第 13 條）；1e WPT、文件、限制、Speedometer 的盤點。WPT 先試跑 `shadow-dom/` 最上層和 `untriaged/` 裡和第 1 段有關的檔案、`dom/` 裡現在 `disabled` 而第 1 段就能跑的，能跑的才收。
15. **P1：shadow root 的 `parent` 一直是空的，host 存在它的 `NodeKindData`**（2026-10-12，第 1 段）：`DocumentFragment` 的 `host` edge，和 `<template>` 的 content 拿 `host` 的方式一樣（1a 時改成沿用 `DocumentFragment`，不另開 variant，見第 2 項）；要 shadow-including 的地方（connected、找 frame、事件路徑、adopt、focus 的祖先）經過 `parent_or_shadow_host()`（Blink 的 `ParentOrShadowHostNode()`）。`.parent()` 的 80 處和 `dom` 的修改原語都不用改，預設停在 shadow 邊界；漏換的地方是 shadow tree 在那裡像沒連上 document，WPT 會抓到。style、layout 的 dirty bit 沿 `parent` 往上標，K-2 要改成跨過 host。儲存和 Blink 不同，記進 `docs/architecture.md` 的「和 Chromium 刻意不同的地方」。不選 P2（照 Blink 把 host 存在 `parent` 欄位、`parent()` 對 shadow root 回傳空：語意相同，但 `dom` 內部直接讀欄位的修改原語都要檢查，漏了會跨過邊界移除或往上走過頭）。

第 2 段開始前，2026-10-12 確認第 16、17 條。

16. **Q2：reaction 的佇列和 element queue 的堆疊放在 heap 外的 `DomState`**（2026-10-12，第 2 段）：用 root（handle）拿著元素和 definition，和 `MutationObserver` 的 `MutationState` 一樣；每次 `[CEReactions]` 的呼叫結束、或 backup element queue 的 microtask 跑完時清空，`DomIsolate` 拆掉時在檢查 root 之前清掉。佇列只活到那次呼叫或下一個 microtask，不用替新的 GC 欄位寫 tracing 和論證；代價是佇列存在的期間元素被 root 住。元素的 custom element state 和 definition 照舊在 rare data。不選 Q1（每個元素的佇列在 rare data、堆疊在 `DomState`，都 traced，照 Blink 的 `CustomElementReactionStack`）：多一組 traced 的資料結構和它的論證，換來的只是佇列期間不 root 元素。
17. **第 2 段分五步**（2026-10-12）：2a `CustomElementRegistry`、definition 和 `define`、`get`、`getName`、`whenDefined`；2b `HTMLElement` 的建構（`new X()`、`createElement` 和 `createElementNS` 同步建構、construction stack）（2026-10-12 做 2b 時把 construction stack 移到 2d：只有 upgrade 推元素進去，2b 用不到也測不到）；2c reaction 的佇列、宣告表的 `[CEReactions]`（用規範的 WebIDL 檢查）、backup element queue 和四個 callback；2d upgrade（`define` 時、插進 document 時、clone 時、`customElements.upgrade()`，換既有 wrapper 的 prototype、`failed`、`disabledFeatures` 的 `shadow`）（2026-10-12 做 2c 時把 `attachShadow` 對 `disabledFeatures` 的檢查提前：HTML 照名稱找 definition，2b 之後就要）；2e WPT、文件、限制、Speedometer 的盤點。
18. **`:defined` 放在 2d**（2026-10-12）：元素的 custom element state 是 uncustomized 或 custom 時符合；`style` 多一個元素狀態，`dom` 改 state 時跟著改、讓 style 重算，和 upgrade 一起做（upgrade 才把 undefined 變成 custom）。原本不在範圍裡；web component 常用 `:not(:defined)` 避免定義之前先畫出來，也讓 2b 設的 state 從 script 看得到。

第 3 段開始前，2026-10-12 確認第 19 到 21 條。

19. **S2：slot 的 assignment 的結果是 heap 外的快取，`assign()` 的節點是弱集合**（2026-10-12，第 3 段）：每個 shadow root 的 assignment（每個 slot 的 assigned nodes、每個 slottable 的 assigned slot）以節點的 id 存在 heap 外，帶著算出它時的版本，樹改了之後用到時才重算（和 `Collection` 的快取一樣只存 id：id 不重複用，舊的快取不會指到別的節點）；`assign()` 給的節點在 slot 的 rare data 的弱集合（`WeakSet`，照 Blink 的 `HTMLSlotElement::manually_assigned_nodes_`，`HeapLinkedHashSet<WeakMember<Node>>`，2026-10-12 對原始碼確認）。不選 S1（照 Blink 的 `assigned_nodes_`、`FlatTreeNodeData`，結果放在 heap 上、traced）：多一組 traced 的資料和它的論證，每次重算都改 edge；K-2 的 flat tree 量到查 id 是瓶頸時再改。
20. **C1：`slotchange` 由修改的操作直接找受影響的 slot**（2026-10-12，第 3 段）：host 的子節點的插入、移除、`slot` attribute 的改變，slot 的插入、移除、`name` 的改變，`assign()`，各自找出 assigned nodes 會變的 slot（照名稱找第一個，照樹的順序）、當場 signal，再標記要重算，照 Blink 的 `SlotAssignment::DidAddSlot`、`DidChangeHostChildSlotName`；維持第 5 條的用到時才重算。不選 C2（修改後當場重算受影響的 shadow root、比較新舊結果：最照規範的字面，但每次修改 host 都走它的子節點，等於第 5 條沒選的 A2）。
21. **第 3 段分五步**（2026-10-12）：3a `HTMLSlotElement`、named assignment、`assignedNodes()`（含 `flatten`）、`assignedElements()`、`assignedSlot`、`Element.slot`，JS 和 wasm；3b manual assignment（`assign()`、`slotAssignment: "manual"`）；3c `slotchange`；3d 經過 slot 的事件路徑（`composedPath()` 對 closed tree 裡的 slot 的過濾）；3e WPT、文件、限制和第 3 段做完時的檢查。

第 4 段開始前，2026-10-12 確認第 22 到 24 條。

22. **guest 的新 export 升 `kun:dom` 的 minor 版**（2026-10-12，第 4 段）：custom element 的 callback 是 guest 必須有的 export（照第 10 項，每種 callback 一個），`kun:dom` 升 minor 版，舊版的 guest 要重編才載入得了。不選「export 是選用的、沒有的 guest 不能定義 custom element、不升版」：host 要分兩種 guest 處理、WIT 表達不了選用的 export。
23. **L1：Lua 的 `define` 由 generator 產生**（2026-10-12，第 4 段）：generator 支援 record 裡的 callback、`option<callback>` 和 `list<string>` 欄位，Lua 照 snake_case 的 table 寫（`customElements:define("x-foo", { construct = function(el) end, connected = ..., attribute_changed = function(el, name, old, new) end, observed_attributes = {"a"} })`）；之後別的 record 帶 callback 時一樣用。不選 L2（`kun_lua.c` 裡手寫 `define`：快，但是 Lua 的 API 裡唯一手寫的 binding）。C 和 Rust 用 WIT 的 binding，沒有另外的 API。
24. **第 4 段分四步**（2026-10-12）：4a WIT 升 0.13、guest 的 export、所有 guest 先補空的實作並重編、`customElements` 給 wasm（`define` 分成 JS 收 class、wasm 收 record 兩種，`get`、`whenDefined` 給 `construct` 的 id）；4b 建構和 reaction（upgrade 時呼叫 `construct`、四種 callback 經過 backup element queue 呼叫 guest 的 export、guest 的 `create-element` 建成 undefined 的元素再排 upgrade、guest trap 時元素是 failed）；4c Lua 的 API（第 23 條）、C 和 Rust 的範例和測試；4d 文件、限制和第 4 段做完時的檢查。加了 export 之後所有 guest 都要同時有它們才載入得了，所以放在第一步。
25. **guest 的 `construct` 回傳 `bool`**（2026-10-12，4c）：`construct(id, element) -> bool`，false 時 upgrade 失敗、元素是 failed，guest 不用 trap；trap 照樣也是失敗。Lua 的函式出錯時 runtime 回報錯誤、回 false，和 Blink 的建構子丟出例外一樣只影響那個元素。不選「只有 trap」：Lua 的 runtime 是同一個 Store 跑頁面上所有的 Lua script，一個 `construct` 出錯就會停掉全部。`kun:dom` 0.13 還沒 merge 回 `main`，直接改，不再升版。
26. **Lua 的 custom element callback 收到的物件可以留著**（2026-10-12，4c）：runtime 把借來的元素和 `adopted` 的兩份 document 換成 Lua 自己的 handle（upcast 成 `Node`、再 downcast 回來，每個物件多兩次 host 呼叫），Lua 可以放進 table、在 timer 裡用，像 JS 的 `this`。不選「只在 callback 裡」（像 event 和 mutation observer）：custom element 幾乎都要留著自己的元素，每次再查一次很麻煩。

## 待決定

（無。各段收的 WPT 在試跑之後定下；shadow tree 裡的元素的 `getComputedStyle` 和幾何見第 9 項。）

## 之後的可能

- K-2：flat tree 的 style 和 layout、`:host`、`::slotted`、`::part`（CSS 線）。
- parser 同步建構 custom element（P1）。
- declarative shadow DOM：parser 的 `attach_declarative_shadow`（html5ever 已經有這個 hook）、`HTMLTemplateElement.shadowRootMode` 等、`getHTML` 的 `serializableShadowRoots`、`setHTMLUnsafe`。
- customized built-in element（`is=`）、form-associated custom element 和 `ElementInternals`、scoped registry。
- `delegatesFocus` 的 focus 委派、shadow tree 裡的 Tab 順序（WPT 的 `shadow-dom/focus/`、`focus-navigation/`）。
- wasm 的 guest 的 callback 在回到 guest 之前執行（X3：`[CEReactions]` 的成員在 WIT 改成 `async func`），有 guest 需要和 JS 一樣的時機時。
- 每個節點的 tree scope edge（S1），量到往上走是瓶頸時。
