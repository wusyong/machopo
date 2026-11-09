# RFC 0035 的細節：shadow DOM 和 custom element

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## Speedometer 怎麼用

2026-10-12 讀 `../third_party/speedometer`（`release/3.1` 的 `1386415`）的 suite 原始碼，對照 RFC 0012 的盤點（Chrome for Testing 152 的 `chrome.json`）。default 的 20 個 suite 裡真的用到 shadow DOM 或 custom element 的只有三個：

| 功能 | TodoMVC-WebComponents | TodoMVC-Lit-Complex-DOM | Perf-Dashboard |
| --- | --- | --- | --- |
| `attachShadow` 的 mode | `open` | `open`（`shadowRootOptions`） | `closed`（存在元件的 `_shadow`） |
| `delegatesFocus`、`slotAssignment` | 沒有 | 沒有 | 沒有 |
| `element.shadowRoot` | runner 的 `querySelectorInShadowRoot` 讀 | `this.shadowRoot ?? this.attachShadow(...)`：沒有 shadow root 時要是 `null` | 沒有 |
| `ShadowRoot` 的成員 | `append`、`adoptedStyleSheets=`；runner 用 `querySelector(All)` | `adoptedStyleSheets=`、`appendChild`、`querySelector` | `appendChild`、`getElementById`、`querySelector` |
| `<slot>`、`assignedNodes`、`slotchange` | 沒有 | 沒有 | 只在兩個 analysis 頁的元件模板裡有 `<slot>`，量的 charts 頁應該用不到 |
| `customElements.define` | 5 個元件 | 5 個（`todo-item`、`todo-list`、`todo-form`、`todo-footer`、`todo-app`） | 每個元件一個 |
| `get`、`whenDefined`、`upgrade` | 沒有 | 沒有 | 沒有 |
| `connectedCallback`、`disconnectedCallback` | 有 | 有 | 有 |
| `attributeChangedCallback`、`observedAttributes` | 有（todo-item、todo-list、topbar、bottombar） | 有（Lit 的 reactive property） | 沒有 |
| `adoptedCallback` | 沒有 | 沒有 | 沒有 |
| customized built-in、`ElementInternals`、declarative shadow DOM | 沒有 | 沒有 | 沒有 |
| `composed`、`composedPath()` | 沒有（`CustomEvent` 是 `bubbles`、不是 `composed`） | `composed: true` 的事件要跨兩到三層 shadow 邊界傳到 `todo-app`；沒有 `composedPath()` | 沒有 |
| constructable stylesheet | 每個 style 模組的最上層 `new CSSStyleSheet()`、`replaceSync` | 偵測 `ShadowRoot`、`Document.prototype.adoptedStyleSheets`、`CSSStyleSheet.prototype.replace` 都有時用 adopted，否則在 shadow root 裡加 `<style>` | 沒有，shadow tree 裡的 `<style>` |
| `:host` | `:host`、`:host([dir="rtl"])`、`:host(:last-child)` | `:host`、`:host([data-priority="N"])`、`:host(:last-child)` | `:host` |

要注意的寫法：
- **TodoMVC-WebComponents**（`../third_party/speedometer/resources/todomvc/vanilla-examples/javascript-web-components/dist/components/`）：`todo-app` 的 constructor 裡 `document.importNode(template.content, true)`，接著馬上用 clone 出來的 `<todo-list>` 的方法：clone 裡的 custom element 要在 `importNode` 回來之前 upgrade（`[CEReactions]`）。`todo-list` 用 `new TodoItem()` 直接建構，在插進 document 之前 `setAttribute`（`attributeChangedCallback` 要在不在 document 裡時也呼叫），再插進兩層 shadow tree 裡（`connectedCallback` 要照 shadow-including 的 connected）。
- **TodoMVC-Lit-Complex-DOM**（`../third_party/speedometer/resources/todomvc/architecture-examples/lit-complex/dist/index.js`）：沒有 `ShadowRoot` 或 adopted 時改在每個 shadow root 裡加 `<style>`，跑得完，但 DOM 的摘要（RFC 0012 的有效性檢查算每個元素，包括 `<style>`）和 Chrome 不同，量測無效。所以 Lit 要 RFC 0016 的第 3 段有 `replace` 和 `Document.prototype.adoptedStyleSheets` 才量得到。刪除和勾選經過 `composed` 的事件，事件傳不出 shadow tree 時 model 不更新、DOM 也不同。另外它用 `document.createTreeWalker(document, 129)`、把 `currentNode` 設成不在 document 裡的 fragment，和 `class extends EventTarget`（kun 都已經有）。
- **Perf-Dashboard**（`../third_party/speedometer/resources/perf.webkit.org/public/v3/`）：`main.js` 先檢查 `Element.prototype.attachShadow`，沒有就 `alert(...)`，kun 現在卡在這裡的 `alert is not defined`。`components/base.js` 的 `ComponentBase` 的 constructor 呼叫 `document.createElement(name)`，所以 `createElement` 要同步建構 custom element，而且 constructor 會重入；`useNativeCustomElements = !!window.customElements`，沒有的話改聽 `DOMNodeInsertedIntoDocument`（kun 沒有 mutation event），所以 `attachShadow` 和 `customElements` 兩個都要有。測試的 hook 經過元件存的 `_shadow` 進 closed 的 shadow root。canvas 的大小取決於 shadow tree 裡的 layout，量測要有效可能要等 K-2（推測，還沒驗證）。
- **只做 feature detection 的**：Editor-CodeMirror、Editor-TipTap（`node.assignedSlot || node.parentNode`、`elt.shadowRoot`）、Charts-chartjs（`!target.shadowRoot`、`String(parent) === "[object ShadowRoot]"`）、Angular-Complex-DOM（zone.js 看到 `window.customElements` 就包住 `define`，名單裡有 `adoptedCallback`；Angular 自己的 `attachShadow` 只在 `ViewEncapsulation.ShadowDom`，這個 bundle 沒有用到）。加了 `customElements` 之後要確認 Angular 照樣跑完。
- **runner**（`../third_party/speedometer/resources/benchmark-runner.mjs`）：`node.shadowRoot ?? node` 再 `querySelector`。

解開 `CSSStyleSheet` 之後，WebComponents 依序會碰到：`replaceSync`、`customElements`、`<todo-app>` 的 upgrade 和 `super()`、`importNode` 的 upgrade、`attachShadow`、`adoptedStyleSheets=`、callback；Lit 依序是 `customElements.define`、`HTMLElement` 的建構、`attachShadow` 和 adopted、`composed` 的事件。

### 盤點的歸屬錯誤

`scripts/speedometer/inventory.mjs` 的 `willRunTest` 先 `takeFrame()` 才把 `current` 換成新的 suite；那時 iframe 已經是新 suite 的頁面，它載入和 prepare 時的呼叫和 CSS 記到了前一個 suite 上。`chrome.json` 看得出來：ES6-Webpack-Complex-DOM 記到 `customElements.define` 5 次、`attachShadow`、`:host`（其實是 WebComponents 的），Svelte-Complex-DOM 記到 `define` 5 次、`trustedTypes`（Lit-Complex-DOM 的），React-Stockcharts-SVG 記到 `define` 31 次、`attachShadow`（Perf-Dashboard 的）；這三個 suite 的 `dist/`、`build/` 裡沒有 `attachShadow`、`customElements`、`CSSStyleSheet`。2026-10-10、2026-10-11 用這個工具的盤點的計數都受影響（2026-10-07 的對照表是另一套記錄，沒有）。2026-10-12 在 RFC 0012 修好（`willRunTest` 只換 `current`）、重跑，修正之後的計數在它的 `progress.md`，和上面讀原始碼的結果一致。

## WPT

`../third_party/wpt` 的 `0f195499f`，2026-10-12 用檔名分類的近似數字（`variant` 會讓 test 更多）：

| 目錄 | 檔案 | 用 testdriver | 和這份的關係 |
| --- | --- | --- | --- |
| `shadow-dom/`（最上層） | 101（testharness 67） | 8 | 核心：`Element-interface-attachShadow*`、`ShadowRoot-interface`、`Extensions-to-Event-Interface`、`event-composed*`、`event-inside-shadow-tree`、`event-with-related-target`、`slots*`、`slotchange*`、`imperative-slot-api*`、`HTMLSlotElement-interface`、`Slottable-mixin`、`getElementById-dynamic-*`、`Node-prototype-cloneNode`、`Document-prototype-importNode` / `adoptNode` |
| `shadow-dom/untriaged/` | 59 | | 相關的：`shadowroot-object`（12）、`upper-boundary-encapsulation`（14）、事件的 retarget 和 `relatedTarget`（11） |
| `shadow-dom/declarative/` | 57 | 0 | 不在範圍 |
| `shadow-dom/focus/`、`focus-navigation/` | 40、45 | 31、44 | 不在範圍（`delegatesFocus`、Tab 順序） |
| `shadow-dom/reference-target/` | 15 | 12 | 不在範圍 |
| `custom-elements/`（最上層） | 51 | | 核心：callback、`CustomElementRegistry`、`HTMLElement-constructor`、`upgrading` |
| `custom-elements/reactions/` | 57 | | 每個 IDL 成員的 `[CEReactions]` |
| `custom-elements/upgrading/`、`htmlconstructor/`、`parser/` | 7、2、12 | | `parser/` 有一部分要同步建構（第 8 項的 P1） |
| `custom-elements/registries/`、`form-associated/` | 40、24 | | 不在範圍 |

kun 現在跑的（`tests/wpt/include.txt`）沒有 `shadow-dom/`、`custom-elements/`。`dom/events/`、`dom/nodes/` 整個目錄都跑，裡面 6 個檔案因為 shadow DOM 或 custom element `disabled`（`shadow-relatedTarget.html`、`relatedTarget.window.js`、`attach-shadow-realm-after-adoption`、`create-element-realm-after-adoption`、`moveBefore-shadow-boundary-id-change`、`moveBefore/slotchange-events`），還有一批預期失敗（`rootNode`、`Node-isConnected-shadow-dom`、`adoption.window.js`、`Node-appendChild-cereactions-vs-script`……）。`css/css-shadow/`（`css-scoping` 改名之後）、constructable stylesheet 的測試是 K-2 和 RFC 0016 的第 3 段。

## kun 的現況

2026-10-12，`main` 的 `63b6ed1e`：
- **樹**：`Node`（`crates/dom/src/node.rs`）是一個 `GcObject`，`parent`、`first_child` 等是 `Edge<Node>`，`document` 是 node document；`NodeKindData` 的 `DocumentFragment { host }` 的 `host` 只給 `<template>` 的 content 用。`kind` 建立之後不變，所以 attach 之後才有的 shadow root 放在 rare data（`crates/dom/src/rare_data.rs`）。
- **connected 和根**：`dom_api` 的 `get_root_node`、`script::is_connected`、`listed_element::root`、`dom` 的 `browsing_context_document`、WebDriver 的那一份，都往上走到根。
- **修改的記錄**：`dom` 的修改原語（`scope.rs` 的 `insert_before`、`detach_with`、`set_attribute`、`attributes_changed`、`adopt`）把要做的事記成 `ElementChange` 等，宣告表的每次呼叫在結尾跑 `dom_api::frame::update_elements`、`queue_mutation_observer_microtask`（`declare.rs` 的方法、setter、special operation、async method；static method 和 constructor 沒有）。parser 每一步之後 renderer 也跑一次。
- **script**：拿著 DOM 的 scope 時不能跑 script（compile-time 的保證，`crates/dom/tests/ui/`），`dom_api` 的演算法要先放掉 scope 才能 `cx.call`。`ScriptHost` 只有 `call`，沒有 construct 和讀 property。
- **wrapper**：`dom_js` 的 `constructor_callback` 把新的物件 wrap 進 `args.this()`（prototype 來自 `new.target`），所以能建構的 interface 已經能被 JS 繼承；`BindingContext::wrap_fresh` 拒絕 wrap 第二次，upgrade 時要換 prototype、不是重新 wrap。
- **事件**：`dom_api::event::event_path` 從 target 往上到 document 和 window，沒有 shadow root 那一步；`composedPath()` 回傳整條；`Event.composed` 有存、沒有作用；`relatedTarget` 不 retarget。
- **選擇器**：`style` 的 `selectors::Element` 實作裡 `containing_shadow_host` 等回傳沒有（K-2 改）。`querySelector` 走自己的子孫，`getElementById` 線性地找。
- **style 和 layout**：`StyleEngine::recalc_style`、`BoxTree::build` 都走 DOM 的子節點，沒有 flat tree、沒有 `display: contents`。
- **parser**：html5ever 的 `TreeSink`（`crates/html/src/sink.rs`）每次呼叫各自借 DOM；`Parser` 放在 `FrameView` 裡，renderer resume 它時拿著 `frame_view`（`render_thread.rs` 的 `layout!`），sink 裡沒有 engine，不能跑 script。html5ever 0.40 有 `attach_declarative_shadow`，kun 沒有實作（照一般的 `<template>`）。
- **wasm**：callback 是 guest 的 id，guest export 固定的幾個入口（`handle-event` 等）；同步的 host function 不能回呼同一個 store，只有 `async method` 可以（`dom_wasm/src/host.rs`）。

## Chromium 的做法

2026-10-12 依記憶整理，實作前對原始碼確認。

- **tree scope**：`TreeScope`（`core/dom/tree_scope.h`）是 `Document` 和 `ShadowRoot` 的共同基底，有 `IdTargetObserverRegistry`、`ScopedStyleResolver`、adopted stylesheet；每個 `Node` 有 `tree_scope_`，`GetDocument()` 經過它。節點跨 tree scope 搬動時 `TreeScopeAdopter` 走過整個子樹更新。`IsConnected`、`IsInShadowTree` 是節點的旗標，插入、移除時更新（`InsertedInto`、`RemovedFrom`）。
- **`ShadowRoot`**（`core/dom/shadow_root.h`）：`DocumentFragment` 加 `TreeScope`；host 的 `ElementRareDataVector` 拿著它。`mode`、`delegatesFocus`、`slotAssignment`、`clonable`、`serializable` 是建立時的設定。
- **slot**：`SlotAssignment`（`core/dom/slot_assignment.cc`）每個 shadow root 一份，修改的操作（`DidAddSlot`、`DidRemoveSlot`、`DidChangeHostChildSlotName`……）只標要重算，並在可能改變時排 `slotchange`；`RecalcAssignment` 在要用時（`assignedNodes`、flat tree 的走法、style 的 recalc 之前）才做。`slotchange` 排在 `MutationObserver` 的 agent 資料（`active_slot_change_list_`），和 mutation record 同一個 microtask。
- **事件**：`EventPath`（`core/dom/events/event_path.cc`）建出 `NodeEventContext` 的清單，`TreeScopeEventContext` 每個 tree scope 一份，算出那個 tree scope 看到的 target、`relatedTarget` 和 `composedPath()`；`EventDispatcher` 照它分派。結果和規範的「每一項記 shadow-adjusted target」相同，是同一件事按 tree scope 合併的版本。
- **custom element**（`core/html/custom/`）：`CustomElementRegistry`、`CustomElementDefinition`（V8 的部分在 `ScriptCustomElementDefinition`，拿著 constructor 和 callback）、`CustomElementReactionStack`（element queue 的 stack 和 backup queue）、`CustomElementReactionQueue`（每個元素一份）。IDL 的 `[CEReactions]` 讓產生的 binding 包一個 `CEReactionsScope`：進來時 push，離開時 pop、呼叫 callback。`V8HTMLConstructor`（`[HTMLConstructor]`）照 `new.target` 找 definition，construction stack 上有元素時用那個元素、把 wrapper 的 prototype 設成 definition 的。
- **parser**：`HTMLConstructionSite::CreateElement` 在主文件（不是 fragment parsing）遇到有 definition 的名稱時，先跑 microtask checkpoint，在 `CEReactionsScope` 裡同步呼叫 constructor（期間 `document.write` 丟例外）；沒有 definition 的建成 undefined，之後 `define` 時 upgrade。

## 選項

### 範圍和切法（R）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **R1**（選了，第 2 條） | 一份 RFC：shadow tree、事件、slot、autonomous custom element；不含 customized built-in、form-associated、declarative shadow DOM | 三個 suite 兩者都要；connected 的語意兩者共用；分段 merge，不用等整份 | 一份 XL 的 RFC |
| R2 | 拆成兩份（shadow DOM；custom element） | 每份小一點，可以分給兩個人 | `[CEReactions]` 和 shadow-including 的 connected 在兩份之間來回引用；兩份的分段和 R1 的三段差不多 |
| R3 | 只做 Speedometer 用到的（沒有 slot、`relatedTarget`、`adoptedCallback`、`upgrade()`） | 最快解開 M3 | slot 是 K-2 的 flat tree 一定要的，晚點還是要回來；WPT 的大部分檔案會一起用到這些 |
| R4 | R1 加上 customized built-in | Chrome、Firefox 有 | Safari 不做，Speedometer 不用；`element_kind` 要看 `is` 的值，`createElement` 多一個參數 |

### tree scope 怎麼找（S）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| S1 | 照 Blink，每個節點多一條 tree scope 的 edge（和旗標） | 找 tree scope 是 O(1) | 跨 tree scope 搬節點時要走過子樹更新；每個節點多一個 traced 的 edge；現有的往上走的地方也要改 |
| **S2**（選了，第 3 條） | 照現在的做法往上走，在 shadow root 跳到 host | 改動最小；現在的 `isConnected`、`getRootNode` 已經是這樣 | O(深度)；`isConnected` 在 shadow tree 裡還要跨過 host 繼續走 |

### 事件路徑（E）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **E1**（選了，第 4 條） | 照規範：路徑的每一項記 shadow-adjusted target、`relatedTarget`、closed tree 的旗標 | 和規範逐行對得上，WPT 的細節（`composedPath()` 的過濾、`relatedTarget` 的 retarget）好查 | 每一項多幾個欄位；Blink 是按 tree scope 合併的 |
| E2 | 照 Blink 的 `TreeScopeEventContext` | 同一個 tree scope 的項目共用一份 | 多一層結構；kun 的路徑短，省下的不多 |

### slot assignment 的時機（A）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **A1**（選了，第 5 條） | 修改時只標要重算、記下要發 `slotchange` 的 slot；用到時才重算 | 照 Blink；大量插入時不重算很多次 | 「可能改變」的判斷要和重算的結果一致（WPT 的 `slotchange` 測試會抓到） |
| A2 | 照規範的字面，每次插入、移除、改 `slot` 都跑「assign slottables」 | 簡單、直接對應規範 | 每次修改都走 host 的子節點；Speedometer 的頁面有幾百個 shadow root |

### reaction 在哪裡執行（C）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **C1**（選了，第 6 條） | 宣告表的成員多一個 `[CEReactions]` 的旗標，只有它們 push / pop element queue；其他的進 backup queue；用規範的 WebIDL 檢查旗標 | 照規範和 Blink 的 `CEReactionsScope`；WPT 的 `reactions/` 逐個成員測 | 要在宣告表標幾百個成員（有規範的 WebIDL 比對，不會漏） |
| C2 | 不分成員，每次呼叫的結尾都跑 reaction（和 `update_elements` 一樣） | 不用標 | 不是 `[CEReactions]` 的成員排進來的 reaction 會提早跑（規範是 microtask），和 Chrome 不同 |

### custom element 的資料和 constructor 放在哪（W）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **W1**（選了，第 7 條） | registry、definition、reaction 的佇列在 `dom`（GC），`dom_api` 照規範排 reaction；`HTMLElement` 的 constructor、讀 `prototype` 和 callback、換 prototype 在 `dom_js`；`ScriptHost` 多 construct | 照 Blink 的分法（`CustomElementDefinition` 在 core、`ScriptCustomElementDefinition` 在 bindings）；reaction 由修改原語驅動，和 `MutationObserver` 一樣 | 改到 GC，要寫論證 |
| W2 | 大部分寫在 `glue.js`（JS 的 registry），DOM 只通知 | GC 的改動少 | reaction 要在修改的當下記下來，JS 那邊看不到；upgrade 的順序、`[CEReactions]` 都要穿過邊界 |

### parser 遇到已經 define 的名稱（P）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| P1 | 照規範和 Blink，主文件的 parser 同步建構（先跑 microtask checkpoint） | 和 Chrome 一樣；`custom-elements/parser/` 能過 | renderer 要在 resume 時把 parser 從 `frame_view` 拿出來、給 sink 一個能跑 script 的 hook；建構時 `document.write` 要丟例外 |
| **P2**（選了，第 8 條） | parser 照舊建成 undefined、排 upgrade（backup queue），那一步結束時 upgrade | 不動 renderer 和 `html`；Speedometer 都走 upgrade | 已經 define 的元素的 constructor 晚於子節點的 parse；`custom-elements/parser/` 有一部分失敗 |

### 繪製（V）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **V1**（選了，第 9 條） | style 和 layout 不動，host 畫 light DOM，shadow tree 沒有 box | 不碰 CSS 線的檔案；K-2 一次做對 | K-2 之前畫面和 Chrome 不同 |
| V2 | host 的子節點都不畫 | 不會畫出 Chrome 不畫的 light DOM | 要改 `layout`；畫面一樣不對（什麼都沒有） |
| V3 | 這份就做 flat tree 的 layout，沒有 style 的範圍 | 畫得出 shadow tree | document 的 stylesheet 會套進 shadow tree，`:host` 不生效，畫面錯得更難懂；K-2 的工作拆到兩條線 |

### wasm（X）

第 1 條選了 X4。

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| X1 | shadow DOM 和 slot 兩邊都給；custom element 只給 JS | shadow DOM 的成員不是 `any`，加進 WIT 只加不改；最少 | wasm 文件不能定義 custom element |
| X2 | 全部只給 JS | 更少 | 違反 RFC 0008 的規則卻沒有理由 |
| X3 | 全部兩邊都給，`[CEReactions]` 的成員在 WIT 都改成 `async func` | callback 的時機和 JS、規範一樣（回到 guest 之前） | 幾百個成員變成 async，`kun:dom` 升版，C、Rust、Lua 的 guest 和測試都要改；C 的每個修改都要等 subtask；沒有 custom element 的頁面每次修改也多一個 async subtask |
| **X4**（選了，第 1 條） | 全部兩邊都給；wasm 呼叫的成員不算 `[CEReactions]` 的範圍，同一個 Store 的 callback 經過 backup element queue 在 microtask 跑 | WIT 只加 registry、record 和 guest 的 export；guest 的成員照舊同步；用的是規範本來就有的機制 | 同一個 Store 的 callback 比 JS 晚（guest 這次的入口結束之後）；新的 guest export 可能要升版（第 4 段決定） |

guest 定義的方式：沒有 class 和 `new.target`，`define(name, definition)` 的 `definition` 是 callback id 的 record（`construct`、`connected`、`disconnected`、`adopted`、`attribute-changed`）加 `observed-attributes`；`construct` 拿到 host 建好的元素，和 JS 在 upgrade 時一樣。Chromium 沒有可以對照的做法（wasm 碰不到 DOM）。

### 和 RFC 0016 第 3 段的先後（K）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **K1**（選了，第 10 條） | 這份的第 1 段 merge 之後，CSS 線開始 RFC 0016 的第 3 段，和這份的第 2、3 段並行 | 照路線圖的分線；`ShadowRoot.adoptedStyleSheets` 有地方接 | 兩條線都要排得進來，M3 才達成 |
| K2 | 這份順便做最小的 `CSSStyleSheet`（建構、`replaceSync`、不套用） | M3 不等 CSS 線 | Lit 要 `replace` 和 `Document.prototype.adoptedStyleSheets` 才量得到；做一半的 CSSOM 之後還是要改寫 |

### 分段（T）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **T1**（選了，第 11 條） | 1 shadow tree 和事件、2 custom element、3 slot（第 1 條之後加上 4 wasm 的 custom element） | 前兩段做完時 Speedometer 這一側就沒有缺的；第 1 段先 merge，CSS 線可以開始 | slot 最後做，第 1 段的事件路徑還不經過 slot（第 3 段補上） |
| T2 | 照規範的依賴：1 shadow tree、事件和 slot，2 custom element | 事件路徑一次做完 | 第 1 段變大，M3 晚一段 |

## 第 1 段的選項

2026-10-12，第 1 段開始前。

### shadow root 的 `parent`（P）

第 15 條選了 P1。

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **P1**（選了，第 15 條） | shadow root 的 `parent` 一直是空的，host 在它的 `NodeKindData` 的 `host`（`DocumentFragment` 的，和 `<template>` 的 content 的 `host` 一樣）；要 shadow-including 的地方經過 `parent_or_shadow_host()`（Blink 的 `ParentOrShadowHostNode()`）跳到 host | `.parent()` 的 80 處（`crates`，不含測試）和 `dom` 內部讀 `parent` 欄位的修改原語都不用改，預設停在 shadow 邊界，正是 `closest`、`MutationObserver` 的祖先、style 和 layout 的走訪要的 | 要 shadow-including 的地方漏換時，shadow tree 在那裡像沒接上 document（`<script>` 不跑、host 移除時裡面的 focus 沒清掉），不會 crash，WPT 的 `Node-isConnected-shadow-dom` 和 shadow tree 裡的 script 測試會抓到；style、layout 的 dirty bit 沿 `parent` 往上標，K-2 要改成跨過 host；儲存和 Blink 不同（`docs/architecture.md` 的「和 Chromium 刻意不同的地方」） |
| P2 | 照 Blink：`parent` 欄位存 host（`parent_or_shadow_host_node_`），`parent()` 在 shadow root 上回傳空（Blink 的 `parentNode()`） | 和 Blink 的儲存一樣；dirty bit 自動跨過 host | `dom` 內部直接讀 `parent` 欄位的地方（`scope.rs` 的 `insert_before`、`detach_with`、`adopt` 等）都要檢查，漏了會把 shadow root 當成 host 的子節點移除、或往上走過頭 |

Blink 平常用的 `parentNode()` 的語意和 P1 一樣，兩者的差別在儲存，和漏改時的後果：P1 漏了是 shadow tree 像沒連上，P2 漏了是修改原語跨過邊界。

### 序列化（H）

第 12 條選了 H1。

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **H1**（選了，第 12 條） | `ShadowRoot` 的 `innerHTML`、`getHTML()`、`setHTMLUnsafe()` 只處理自己的子節點；`serializableShadowRoots`、`shadowRoots` 和 declarative shadow DOM 一起做 | 輸出和 kun 讀得回來的一致 | `getHTML` 少了 Chrome 有的 shadow root |
| H2 | 現在就序列化成 `<template shadowrootmode>` | 和 Chrome 的輸出一樣 | 讀回來是一般的 `<template>` |

### focus（F）

第 13 條選了 F1。

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **F1**（選了，第 13 條） | 第 1 段照規範寫 `activeElement` 和 focus 事件的 retarget，`set_focused_element` 接受 shadow-including 的子孫；真的 focus 進去等 K-2 的 box | retarget 和事件路徑一起做；K-2 不用回頭改 DOM 線的檔案 | K-2 之前只能在 `dom` 那一層測 |
| F2 | 整個延到 K-2 | 第 1 段小一點 | 拆開同一套 retarget 的規則，K-2 要改 DOM 線的檔案 |

## 參考資料

- DOM：shadow tree、「get the parent」、retarget、「assign slottables」、`slotchange`、「create an element」。
- HTML：custom element 的 definition、reaction（`[CEReactions]`、element queue、backup element queue）、upgrade、`[HTMLConstructor]`、parser 的「create an element for a token」。
- Chromium：`third_party/blink/renderer/core/dom/`（`tree_scope`、`shadow_root`、`slot_assignment`、`events/event_path`）、`core/html/custom/`、`core/html/parser/html_construction_site.cc`、`bindings/core/v8/v8_html_constructor.cc`。
