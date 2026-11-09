# RFC 0010 的細節：DOM API 的第二批

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## WPT 的調查

2026-10-06，WPT 的 commit 照 `third_party.toml`。分成兩部分：已經匯入的目錄裡，baseline 的失敗各卡在哪一組（讀 `4ff8d6e:tests/wpt/baselines/` 的每個 `-expected.txt` 的訊息，模糊的對照測試的原始碼）；還沒匯入的目錄，用 `rfcs/0008-dom-wpt/details.md` 的「WPT `dom/` 的調查」同樣的算法（以檔案為單位、本地的 helper 跟到 3 層、去掉 `//` 的註解行，因為 traversal 的測試裡有註解掉的 `innerHTML`），是 regex 的估計。

五組的代號：G1 `MutationObserver`（和 `queueMicrotask`）、G2 HTML 的片段（`innerHTML`、`outerHTML`、`insertAdjacent*`、`DOMParser`、`getHTML`、`setHTMLUnsafe`）、G3 traversal、G4 `AbortController` / `AbortSignal`、G5 `Range` / `StaticRange` / `Selection`。

### 已經匯入的目錄

skip 清單裡沒有因為這五組才跳過的檔案；用到它們的 7 個檔案另外要 shadow DOM、testdriver 或 `.sub`。

| 組 | 檔案 | 失敗的 subtest | 其中只差這一組的 |
| --- | --- | --- | --- |
| G1 | 19 | 141（另外 3 個整個檔案是 harness 的錯誤） | 135 |
| G2 | 29 | 220 | 146（14 個檔案） |
| G3 | 2 | 5 | 5 |
| G4 | 3 | 14 | 14 |
| G5 | 5 | 17 | 6（另外 11 個同時要 G1） |

- **G1**：`dom/nodes/MutationObserver-*` 佔大部分（attributes、childList、characterData、sanity 最多），另有 `ParentNode-replaceChildren`、`insertion-removing-steps/` 的兩個、`moveBefore/mutation-observer`，和 `html/webappapis/timers/timer-nesting-not-inherited-in-microtask`（`queueMicrotask`）。
  - `MutationObserver-document` 要 parser 插入的節點也有紀錄。
  - `MutationObserver-inner-outer` 同時要 G2；`MutationObserver-characterData`、`-childList` 有 11 個 subtest 用 `createRange`（G5）。
  - `processing-instruction-attributes` 的 2 個做完也不會過（要 processing instruction 有屬性，`docs/limitations.md`）。
- **G2，只差 HTML 的片段的 146 個**：`ChildNode-after`、`before`、`replaceWith`（讀 `innerHTML` 比對結果，三個檔案佔 111 個）、`insert-adjacent`、`Element-insertAdjacentElement`、`Element-insertAdjacentText`、`Document-getElementById`、幾個 `ParentNode-querySelector*`；`DOMParser` 的 `text/html` 只有 4 個（`ParentNode-querySelector-dupe-id`、`Node-cloneNode-document-with-doctype`、`Document-createElement-namespace`）。
- **G2，同時要別的**：`node-realm-*`（HTML 元素的 interface、`customElements`）、`moveBefore/relevant-mutations`（圖片的載入）、`remove-next-sibling-during-replace-with`（window 的 named access）；要 XML parser 的 52 個 subtest 和 2 個整個檔案（`DOMParser` 的 XML 類型），所以 `DOMParser` 只做 `text/html` 時，約 56 個 `DOMParser` 的失敗只解開 4 個。
- **G3**：`Document-createTreeWalker`、`moveBefore/moveBefore-nodeiterator`。
- **G4**：`AddEventListenerOptions-signal.any`（`signal: null` 要丟 `TypeError` 的 2 個也在裡面）、`event-global-event-targets`、`event-listener-not-fully-active-target`（iframe 的 `AbortController`）。
- **G5**：`Node-normalize-2`、`node-creation-realm`、`moveBefore/live-range-updates`，和上面 G1 的 11 個。

### 還沒匯入的目錄

「沒有硬阻擋」是不要 shadow DOM、testdriver、XML 的測試檔、worker、`fetch`、`.sub`（iframe 已經可以，RFC 0009 的 2a）。

| 目錄 | 檔案 | 沒有硬阻擋 | 要 XML parser | 用到 `.style` | 用到的組 |
| --- | --- | --- | --- | --- | --- |
| `dom/traversal` | 18 | 18 | 0 | 3 | G3 18、G5 3、G2 1 |
| `dom/abort` | 6 | 6 | 0 | 0 | G4 6 |
| `dom/ranges` | 45 | 37 | 1 | 24 | G5 45、G2 9 |
| `domparsing` | 44 | 37 | 18 | 0 | G2 44、G5 2 |
| `html/webappapis/microtask-queuing` | 4 | 4 | 0 | 0 | G1 4 |
| `html/webappapis/scripting/event-loops` | 7 | 6 | 0 | 2 | G1 3 |
| `html/webappapis/dynamic-markup-insertion/`（`the-innerhtml-property`、`the-outerhtml-property`、`html-unsafe-methods`） | 17 | 13 | 2 | 0 | G2 |
| `html/syntax/serializing-html-fragments` | 8 | 7 | 2 | 0 | G2 8 |
| `html/syntax/parsing-html-fragments` | 14 | 13 | 0 | 0 | G2 5（另外 9 個是編碼的測試） |
| `html/semantics/scripting-1/the-template-element` | 68 | 62 | 0 | 1 | G2 34、G3 4、G5 2、G1 1 |
| `selection` | 90 | 56 | 0 | 31 | G5 81、G2 44 |

另有 `dom/ranges/tentative`、`domparsing/tentative`、`selection` 的 tentative、`dom/observable/tentative`，是不在範圍內的新 API。

- **`.style`**：`ranges`、`selection`、`traversal` 的測試常在最後一行 `testDiv.style.display = "none"`，subtest 的結果照樣有，harness 以錯誤結束；但 `Range-cloneContents`、`-deleteContents`、`-extractContents`、`-insertNode`、`-surroundContents` 這 5 個大的測試在開始前就用到它，`NodeIterator-removal` 在中間用到。
- **`dom/common.js`** 的 setup 要的 `new Document`、`createCDATASection`、`createHTMLDocument` 等都有了（RFC 0008 第 5 步），`createRange` 只在它的 range helper 裡，所以 `Range` 和 traversal 的測試不要 G2。

### 每一組解開多少、組和組之間的依賴

只差那一組就沒有阻擋的檔案數（還沒匯入的目錄，不含 tentative）：

| 組 | 嚴格（另外不要 `.style`、layout、XML、編輯） | 寬鬆（允許那些） | 要和別組一起 |
| --- | --- | --- | --- |
| G2 | 64 | 67 | — |
| G5 | 33 | 66 | 8（嚴格）到 39（寬鬆）個要 G2 + G5 |
| G3 | 15 | 15 | 4 個要 G2 + G3 |
| G1 | 7 | 7 | — |
| G4 | 6 | 6 | — |

- 每次選解開最多的一組，嚴格和寬鬆的順序都是 G2、G5、G3、G1、G4；加上已經匯入的目錄的 subtest，G1 排第二（135 個只差它）。
- 依賴：G1 的部分測試要 G2 和 G5；`selection/` 的一半用 `innerHTML`；traversal 只有 1 個要 `innerHTML`；G3、G4 和其他組幾乎獨立。
- `README.md` 的順序是 G2、G1、G4、G3、G5：G1 提前，因為已經匯入的目錄裡它是第二多，而且要在 G2 加上新的修改路徑之後接上紀錄；G3、G4 小，讓基礎（例外往外丟、移除之前的 hook）先在小的 API 上做；G5 最大，最後做。

### 使用頻率

檔案數，helper 跟到 3 層，regex 的估計。「已經匯入」是 `testharness-include.txt` 的目錄，「候選」是上表的目錄加上 tentative。

| 名稱 | 已經匯入 | 候選 |
| --- | --- | --- |
| `innerHTML` | 36 | 167 |
| `getSelection` | 7 | 124 |
| `createRange` | 7 | 100 |
| `DOMParser` | 12 | 37 |
| `MutationObserver` | 19 | 5 |
| `AbortController` | 3 | 21 |
| `outerHTML` | 5 | 17 |
| `NodeFilter` | 1 | 19 |
| `TreeWalker` / `createTreeWalker` | 1 | 18 |
| `getHTML` / `setHTMLUnsafe` / `parseHTMLUnsafe` | 0 | 16 |
| `AbortSignal` | 3 | 11 |
| `queueMicrotask` | 3 | 6 |
| `insertAdjacentHTML` | 0 | 7 |
| `NodeIterator` / `createNodeIterator` | 1 | 4 |
| `createContextualFragment` | 0 | 5 |
| `XMLSerializer` | 0 | 4 |

## kun 現在的樣子

2026-10-06 讀程式碼的結果，給每一組要接的地方。

### microtask

- V8 的 microtask 是 explicit 的：`bindings` 的 `ScriptState` 設 `MicrotasksPolicy::Explicit`，`DomIsolate::perform_microtask_checkpoint` 是執行 script 的入口之一；`dom_js` 的 engine 做 checkpoint、通知沒處理的 rejection、再做一次 checkpoint。
- renderer 在每個 task、每個 script、每個 timer 和 animation frame 的 callback、renderer 發起的每次分派之後做 checkpoint；同一次分派的 listener 之間不做（HTML 的「clean up after running a callback」每個 listener 都要做，`docs/limitations.md` 還沒記）。
- `dom` 和 `dom_api` 都不能排 microtask，`ScriptHost` 沒有這個方法；`queueMicrotask` 不在宣告表裡。
- rusty_v8 的 fork 有 `Isolate::enqueue_microtask(Local<Function>)`，沒有 C callback 的版本；`MutationObserver` 的通知可以用 `dom_js` 的一個原生 function，不用改 C++。
- wasm 的 engine 的 checkpoint 什麼都不做（Component Model 的 task 在每個 store 自己的事件迴圈裡跑），`run_tasks` 之後也不做 checkpoint（RFC 0001 的第 33 條說要做）。

### 修改操作

- **`dom` 的原語**（`DomMutationScope`）：`insert_before`（`append_child` 經過它，先 adopt，再從舊的 parent 拿掉）、`remove`、`adopt_node`、`set_attribute`、`remove_attribute`、`set_attr_node`、`set_attr_value`、`append_text`（parser 用）、`set_character_data`（整個換掉）。內部的共同點只有設 style 和 layout 的 flag、加 `dom_tree_version`；舊的值和 offset 都沒傳進來。
- **`dom_api` 的演算法**：`node.rs` 的 pre-insert（fragment 的子節點一個一個插）、replace（先移除再插入）、`textContent`、`normalize`、`adoptNode`；`parent_node.rs`、`child_node.rs` 的 `append` 等；`character_data.rs` 的 `replace_data` 是 `appendData` 等的共同點，但 `data` 的 setter、`splitText`、`nodeValue`、`textContent`、`normalize` 直接呼叫 `set_character_data`；屬性的修改有十來個入口，多數經過 `element::set` / `element::remove`（HTML 的屬性改變的步驟），`token_list` 的 `update` 直接呼叫 `dom`。
- **parser**（`crates/html/src/sink.rs`）：插入、加文字、加屬性、移除、`reparent_children`，直接呼叫 `dom`。
- 當時的看法是 `MutationObserver` 的紀錄（一筆包括好幾個節點、要舊的值）在 `dom_api` 的演算法層組、parser 另外接；第 2 階段開始前改成在 `dom` 的原語記，好幾個節點合成一筆由 `dom_api` 開批次（README 的「已決定的事項」第 9 條，照 Blink）。`Range` 和 `NodeIterator` 的調整要在移除的當下、用舊的 parent 和位置，只能在 `dom` 的原語裡做。
- iframe 的 insertion steps（RFC 0009 的第 25 條）的做法是 `dom` 記下 connected 的改變、`dom_api::update_frames` 在宣告表的每次呼叫結束時取走；`MutationObserver` 的「排 microtask」可以在同一個地方觸發。

### callback

- `ScriptHost::call`：V8 的實作在 callback 不是 function 時找 `handleEvent`（寫死），例外一律在 callback 的 realm 回報，回傳 `Err(Reported)`；`ApiError` 只有 `Dom` 和 `Type`。
- 宣告表的 `Callback`（只有 function）和 `CallbackInterface`（任何物件）不帶方法名稱。
- sequence 只能當結果，參數是 sequence 時 `dom_js` 的轉換拒絕。
- wasm 的 guest export 的 `callback` 是 `callback(id, event, number) -> bool`，帶不了 `MutationObserver` 的 `(records, observer)` 或 `NodeFilter` 的 `(node) -> u16`；會呼叫回 guest 的成員（`async func`）本來只有 `dispatchEvent`。

### listener 和 timer

- listener 是 `Listener` 的 linked list（節點、`Window`、`EventTargetImpl` 各有一條），`ListenerOptions` 是 capture、`once`、`passive`；`add_event_listener` 不回傳新的 listener，`signal` 要它（abort 時移除這一個）。
- `EventTargetImpl` 是「不是節點也不是 window 的 event target」的先例，`AbortSignal` 照它接進 `EventTarget`、`TargetEdge`、wrap tag 和 slot 表。
- `Timers` 在 `DomState`，每個 timer 是 `ScriptCallback`，沒有「DOM 自己的動作」。

### 片段

- parser 沒有 fragment 的入口；html5ever 0.40 有 `parse_fragment` 和 `parse_fragment_for_element`（context 是已經存在的元素，sink 的 `create_element` 也會替假的 context 元素呼叫）。
- 沒有序列化；`html::dump_tree` 是 html5lib 測試的格式。

## parser 的依賴方向

`html` 依賴 `dom_api` 只為了 `handlers::attribute_changed`：parser 建元素、`add_attrs_if_missing` 時把 `onclick` 這類 content attribute 變成還沒編譯的 handler，並帶著行號。

### Chromium 怎麼做

2026-10-06 讀 Chromium 的 main（`third_party/blink/renderer/core/`）：

- **parser 呼叫元素的行為**：`html/parser/html_construction_site.cc` 建元素之後呼叫 `element->ParserSetAttributes(token->Attributes())`；`dom/element.cc` 的 `Element::ParserSetAttributes` 對每個屬性呼叫 `AttributeChanged`（reason 是 `kByParser`），經過 virtual 的 `ParseAttribute` 到 `html/html_element.cc` 的 `HTMLElement::ParseAttribute`，它查 `AttributeTriggers` 表，`onclick` 這類就呼叫 `SetElementAttributeEventListenerFromScriptBody`。也就是 kun 的 `dom_api::handlers::attribute_changed` 做的事，在 Blink 是元素自己的方法，parser 只呼叫 `Element` 的介面。
- **元素呼叫 parser**：`Element::SetInnerHTMLInternal` 先 parse 成 `DocumentFragment` 再「replace all」；`dom/document_fragment.cc` 的 `DocumentFragment::ParseHTML` 直接呼叫 `HTMLDocumentParser::ParseDocumentFragment`，它為這個片段建一個新的 `HTMLDocumentParser`（fragment 模式），`Append`、`Finish`、`Detach`。XML 是 `DocumentFragment::ParseXML` → `XMLDocumentParser::ParseDocumentFragment`。
- **片段裡的 script**：`HTMLConstructionSite` 用 `CreateElementFlags::ByFragmentParser`，在 fragment 模式而且 `ParserContentPolicy` 要求時 `SetAlreadyStarted`，所以 `innerHTML` 插入的 `<script>` 不執行；`createContextualFragment` 等用 `kAllowScriptingContentAndDoNotMarkAlreadyStarted`。
- **序列化**：getter 是 `CreateMarkup(this, kChildrenOnly)`（`editing/serializers/serialization.cc`、`markup_accumulator.cc`），和 parser 無關。
- **所以 Blink 的 parser 和 DOM 互相依賴**：兩者都在 `core` 這一個 component 裡，C++ 允許互相 include，不需要分層。kun 把 Blink 的 `Element` 拆成兩個 crate：樹和資料在 `dom`，元素的行為（`AttributeChanged`、`ParseAttribute`、`setInnerHTML`）在 `dom_api`；`html` 對應 `core/html/parser`。照 Blink 的呼叫關係，`html` 要呼叫 `dom_api`（屬性的步驟），`dom_api` 也要呼叫 `html`（fragment parsing），Rust 的 crate 不能互相依賴，所以三個做法都是在處理這個循環：
  - A 用 trait 表達 Blink 的 virtual call：parser 只認得「元素的屬性改變了」這個介面（`Element::AttributeChanged` 的角色），實作在 `dom_api`；
  - B 照 Blink 的打包，放進同一個 crate；
  - C 不在 Blink 裡：Blink 的 fragment parsing 不經過 frame 或 embedder，沒有 frame 的 document 也一樣能 parse。

### 比較

| 做法 | 怎麼做 | 好處 | 壞處 |
| --- | --- | --- | --- |
| A：parser 只依賴 `dom`，屬性改變的步驟由呼叫者傳入（看法） | `html` 定義一個 trait（或 function 參數），renderer 和 `dom_api` 傳入 `dom_api::handlers::attribute_changed`；`dom_api` 依賴 `html` | 改動小；parser 照舊是獨立的 crate，可以單獨測；和 Blink「parser 在 DOM 之下、直接呼叫元素的屬性步驟」的方向一致 | 多一層間接；之後 parser 要呼叫的 DOM 步驟（custom element 的建立）也要加進 trait |
| B：parser 併進 `dom_api` | `html` 的模組搬進 `dom_api` | 和 Blink 的 `core` 一樣在一起，沒有間接 | `dom_api` 變大、編譯變慢；`html` 的測試（html5lib）跟著搬 |
| C：parser 經過 embedder 的 hook | `ScriptHost` 或一個登記的 function 提供 fragment parsing | 不動依賴 | 沒有 frame 的 document（`createHTMLDocument`）也要能 parse，hook 要從 engine 或全域拿，dom_api 的測試要另外接 |

## 序列化

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| html5ever 的 `serialize`（選了這個，`README.md` 的第 5 條） | 已經是依賴，跟著 parser 更新；raw text 的元素、`<noscript>` 照 scripting flag、跳脫字元都有 | 要實作它的 `Serialize` trait 走訪 `dom` 的樹（`<template>` 的內容自己處理）；規範改了時要等上游 |
| 自己寫（Blink 的 `MarkupAccumulator`） | 完全照規範，可以順便做之後的 XML 序列化 | 多維護一份；HTML 的部分約 200 行 |

## 參考資料

- DOM：mutation observer、`AbortController`、traversal、ranges 各節；HTML：「Serializing HTML fragments」、「Parsing HTML fragments」、`innerHTML` 和 DOM parsing（原本的 DOM Parsing and Serialization 已經併進 HTML）、microtask 的排程。
- Chromium：`README.md` 的「先例」列的檔案。
- html5ever 0.40.1：`src/driver.rs`（`parse_fragment`）、`src/serialize/mod.rs`。
