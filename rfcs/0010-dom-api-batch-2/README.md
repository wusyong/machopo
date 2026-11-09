# RFC 0010：DOM API 的第二批（`innerHTML`、`MutationObserver`、`AbortController`、traversal）

- 開始日期：2026-10-06
- 狀態：✅ 已完成：2026-10-07（macOS arm64；範圍和順序 2026-10-06 確定，「已決定的事項」第 1 條；`Range` 和 `StaticRange` 2026-10-07 移到之後的 RFC，第 19 條）
- 細節：`details.md`；進度：`progress.md`

這份 RFC 接著 RFC 0008 的「已決定的事項」第 50 條：RFC 0008 做完時，`innerHTML`、`TreeWalker` / `NodeIterator`、`MutationObserver`、`AbortController` 和 `Range` 被留下來另開 RFC，因為它們都是新的機制，不只是加成員。RFC 0009 的第 1 階段（每個 window 的 realm、timer 記得 window）和 2a（看不見的 iframe）先做完了，這一批照 RFC 0009 `details.md` 第 10 節的約束寫。

## 摘要

補上 DOM 和 HTML 規範裡四組還沒有的 API，每組同時給 JS（`dom_js`）和 wasm（`kun:dom`）：

1. **HTML 的片段**：fragment parsing 和序列化，`innerHTML`、`outerHTML`、`insertAdjacentHTML` 等，`DOMParser` 的 `text/html`。
2. **`MutationObserver`**，加上 `queueMicrotask`。
3. **`AbortController` 和 `AbortSignal`**，加上 `addEventListener` 的 `signal`。
4. **traversal**：`TreeWalker`、`NodeIterator`、`NodeFilter`。

原本的第五組 `Range`（和 `StaticRange`）2026-10-07 移到之後的 RFC（「已決定的事項」第 19 條）。

分四個階段，照 WPT 的收益和彼此的依賴排（「分階段」）。每一組都先補了一塊 kun 還沒有的基礎：parser 和 `dom_api` 的依賴方向、新增 GC 型別的成本、engine 無關的 microtask、會同步呼叫回 script 的成員、callback 的例外交還呼叫者、「節點移除之前」的 hook（「詳細設計」的「共用的基礎」）。每個階段開始前，只決定那一段要決定的事。

## 動機

**現況（2026-10-06）**：
- **WPT 已經匯入的目錄裡，這五組擋住的 subtest 最多**：`rfcs/0008-dom-wpt/progress.md` 的「做過的檢查」列的剩下的失敗裡，iframe 之後就是這幾組；RFC 0009 的 2a 解開 iframe 之後，它們是 `dom/` 剩下的失敗裡最大的一群。每一組各擋住多少、還有哪些沒匯入的目錄等著它們，見 `details.md` 的「WPT 的調查」。
- **常用的頁面寫法做不到**：沒有 `innerHTML`，script 只能一個一個建節點；沒有 `MutationObserver`，框架和 polyfill 偵測 DOM 改變的方法都沒有；沒有 `AbortController`，`addEventListener` 不能一次移除一批 listener。`docs/limitations.md` 的「Bindings 和 DOM API」和「Parser 和載入」記著這幾項。
- **html5lib 的 fragment parsing 測試全部跳過**（`crates/html/tests/tree_construction.rs`）：parser 還沒有 fragment 的入口。
- **基礎還缺幾塊**（`details.md` 的「kun 現在的樣子」）：
  - `html`（parser）依賴 `dom_api`（為了屬性改變時的 event handler），所以 `dom_api` 的 `innerHTML` 不能直接呼叫 parser。
  - `dom` 和 `dom_api` 不能排 microtask，只有 engine 在 task 之後做 checkpoint。
  - callback 丟出的例外一律當場回報，不能丟回呼叫者（`NodeFilter` 要丟回去）；callback interface 的方法名稱寫死是 `handleEvent`。
  - 參數還不能是 sequence（`MutationObserverInit` 的 `attributeFilter`）。
  - 修改操作分散在 `dom` 的原語和 `dom_api` 的演算法，沒有一個地方知道「這次操作」的全部節點和舊的值。
  - timer 只能執行 script 的 callback（`AbortSignal.timeout` 要在時間到時做 DOM 自己的事）。

目標：
- 這五組 API 照規範運作，由 WPT 驗證；匯入它們解開的 WPT 目錄（每一階段列在「分階段」）。
- 每組的成員同時在 JS 和 `kun:dom`，`any` 的照 RFC 0008 的規則只給 JS。
- 補上的基礎照 Chromium 的分層，之後的 API（shadow DOM 的 `slotchange`、`Selection`、custom element 的 reaction）可以接上去。

不在範圍內：
- **XML 的 parser 和序列化**：`DOMParser` 的 XML 類型、`XMLSerializer`、XML document 裡的 `innerHTML`。`domparsing/` 裡這部分的測試要 XML parser（`details.md` 的「WPT 的調查」）。另開 RFC。
- **shadow DOM 有關的部分**：`getHTML` 的 `serializableShadowRoots` 和 `shadowRoots`、declarative shadow DOM（`<template shadowrootmode>`）、`getComposedRanges`、`slotchange`。shadow DOM 另開 RFC（RFC 0009 的 `details.md` 第 9 節），這裡只照約束留位置。
- **`Selection` 和 `getSelection()`**：要編輯、游標和 layout，`selection/` 的測試多半也要 `contenteditable` 或 testdriver。
- **`Range` 和 layout 有關的成員**：`getClientRects`、`getBoundingClientRect`（CSSOM View）。
- **`element.style`**：traversal 和 `Range` 的測試常在最後一行用它（`dom/traversal` 有 3 個檔案的 subtest 全部通過，只是最後一行失敗）。
- **`Range` 和 `StaticRange`**：原本是第 5 階段，2026-10-07 移到之後的 RFC（第 19 條，「之後的可能」）。
- **Sanitizer API（`setHTML`）、streaming 和位置的 HTML API**（`domparsing/tentative`）、Observable（`dom/observable`）、custom element、mutation event。

## 設計概要

**給寫網頁的人**：
- `el.innerHTML = '<b>hi</b>'`、`el.outerHTML`、`insertAdjacentHTML`、`insertAdjacentElement`、`insertAdjacentText`、`getHTML()`、`setHTMLUnsafe`、`Document.parseHTMLUnsafe`、`new DOMParser().parseFromString(s, 'text/html')` 都有了。插入的 `<script>` 照規範不執行。
- `new MutationObserver(cb).observe(node, {childList: true, subtree: true})`：修改之後，在 microtask 裡收到 `MutationRecord`；`takeRecords`、`disconnect`。`queueMicrotask(cb)`。
- `const c = new AbortController(); el.addEventListener('click', f, {signal: c.signal}); c.abort()`；`AbortSignal.abort()`、`timeout()`、`any()`、`throwIfAborted()`。
- `document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT, filter)`、`createNodeIterator`；filter 丟出的例外從走訪原樣丟出。

**給 wasm 的 guest**：同樣的 API 在 `kun:dom` 裡（0.7.0，從 0.3.0 經過四次不相容的改動）。會同步呼叫回 guest 的成員（`dispatchEvent`、`AbortController.abort()`、會呼叫 filter 的 traversal 方法）是 `async func`（第 17 條）；guest 的 `callback` export 換成每種 callback 一個 export（`handle-event`、`run`、`frame`、`mutations`、`accept-node`；「已決定的事項」第 10 條）。`any` 的成員和參數照 RFC 0008 的規則只給 JS（`AbortSignal` 的 `reason`）。

**給改 kun 的人**：
- 加 API 的流程不變（`crates/dom_api/README.md` 的「怎麼新增一個 API」）；新的 GC 型別照 `crates/dom/README.md` 的清單（第 8 條之後的做法）。
- 新的機制各有一個入口：修改操作的紀錄（`MutationObserver`，第 9 條）、「節點移除之前」的 hook（`NodeIterator` 的 pre-removing steps，之後 `Range` 沿用）、`ScriptHost` 排 microtask（第 11 條）、`async method`（第 17 條）、呼叫 callback 時交還例外（第 18 條）。之後的修改操作和 callback 要經過它們，規則寫在 `crates/dom/README.md` 和 `crates/dom_api/README.md`。

## 詳細設計

完成時（2026-10-07）的設計。比較的細節在 `details.md`，驗收見 `progress.md`，還成立的限制在 `docs/limitations.md`。

### 共用的基礎

- **parser 和 `dom_api` 的依賴方向**（第 1 階段，「已決定的事項」第 4 條）：`html` 原本依賴 `dom_api`，只為了在 parser 建元素、加屬性時呼叫 `dom_api::handlers::attribute_changed`（`onclick` 這類 content attribute 變成還沒編譯的 handler），`innerHTML` 在 `dom_api` 裡要呼叫 parser，方向倒過來了：`html` 只依賴 `dom`，parser 設好每個屬性之後經過呼叫者給的 `html::ElementHooks` 跑元素的屬性步驟，`dom_api::ParserHooks` 實作它，`dom_api` 依賴 `html`；renderer 建 parser 時傳入 `ParserHooks`（`new_parser`，上層 document、`src` 和 `srcdoc` 的子 document 共用）。Blink 的 parser 和 DOM 都在 `core`，互相直接呼叫（parser 經過 `Element::ParserSetAttributes` 執行元素的屬性步驟，`innerHTML` 經過 `DocumentFragment::ParseHTML` 呼叫 parser）。比較見 `details.md` 的「parser 的依賴方向」。
- **宣告表的 `static` 和 `enum`**（第 1 階段的 1c-2，「已決定的事項」第 6 條）：`static method` 的呼叫入口的 `this` 是執行中的 realm 的 window（JS 是 interface object 的 realm 的 global，wasm 是 store 的 window），不跑 iframe 的 insertion steps；WIT 是宣告它的 resource 的 `static func`。`enums!` 的中性的值是字串，入口只收它的值；JS 先 `ToString`，WIT 是 `enum`（註解列出每個 case 的值），Lua 用值的字串。還沒有成員回傳 `enum`，所以只有參數的轉換。
- **microtask**（2b，第 11 條）：`ScriptHost::queue_microtask(Microtask)`，`Microtask` 是 `dom_api` 的 enum（`queueMicrotask` 的 callback、通知 mutation observer），engine 不解讀它，原封不動交還 `dom_api::run_microtask`。佇列在 `dom` 的 `DomState`；V8 排一個帶著編號的原生 function 當跳板（`Isolate::enqueue_microtask`，和 promise 的 job 照順序），wasm 的 engine 在自己的 checkpoint 裡從最舊的開始跑（renderer 本來就在每個 task、script、callback 之後呼叫它）。`queueMicrotask` 是 `Window` 的成員（RFC 0009 的約束）。
- **會同步呼叫回 script 的成員**（3c，第 17 條）：宣告表的 `async method`（函式是 `async fn`）。JS 照樣同步呼叫、當場跑完；WIT 是 `async func`，guest 自己的 listener 和 filter 在它的 Store 的 event loop 裡跑。`dispatchEvent`、`AbortController.abort()`、traversal 會呼叫 filter 的方法都是。
- **callback 的例外交還呼叫者**（第 4 階段，第 18 條）：`ScriptHost::call` 交還例外（`CallError::Thrown`：script 的值，加上 engine 回報要用的資訊；`Reported` 是 host 已經處理掉的 trap 或中止；`Failed` 是 guest 自己留著原因的失敗），呼叫者決定回報（`call_reporting`：listener、timer、animation frame、microtask、`MutationObserver`）還是往外丟（`ApiError::Thrown`：`NodeFilter`）。`throwIfAborted` 的 `reason` 也是 `ApiError::Thrown`（3a）。`call` 帶著 callback interface 的種類（`CallbackInterfaceType`：`EventListener`、`NodeFilter`），不是函式的物件照它呼叫 `handleEvent` 或 `acceptNode`（WebIDL 的 "call a user object's operation"）。
- **sequence 參數**（2c-2）：前端把 JS 的 iterable 轉成 `Value::List`（WebIDL 的 sequence 轉換），`attributeFilter` 和 `AbortSignal.any()` 用到。
- **「節點移除之前」的 hook**（4a，第 12 條）：`dom` 的 `detach_with` 在拆開節點之前，對節點的 document 的每個 `NodeIterator` 跑 pre-removing steps（用舊的 parent 和兄弟），parser 的移除也經過它；document 經過 rare data 的 weak 集合（`WeakSet`）找得到自己的 iterator，不撐住它們（Blink 的 `Document::node_iterators_`）。iterator 的 root 被 adopt 到另一份 document 時跟著搬（Blink 的 `MoveNodeIteratorsToNewDocument`）；為此插入時改成先從舊的 parent 移除、再換 document，和規範的 adopt 一樣。之後 `Range` 的 live range 沿用這個 hook，再加上插入、`replaceData`、`splitText`、`normalize` 的調整。
- **新的 GC 型別**（2a，第 8 條）：這份 RFC 加了 `DomParser`（第 3 條）、`MutationObserver`、`MutationRecord`、`AbortController`、`NodeIterator`、`TreeWalker`，`AbortSignal` 是 `EventTargetImpl` 的一種（第 16 條）。第 2 階段開始時先降低新增一個型別的成本：給 script 拿的 GC 型別實作一個 sealed 的 `ScriptObject` trait（tag、名稱、wrapper slot，對應 Blink 的 `ScriptWrappable` 和 `WrapperTypeInfo`），handle（`Handle<T>`）、`own`、wrapper 和 unwrap 各是一份泛型；wasm 的 slot 表每格是一條不帶型別的 edge 加上型別的 tag（對應 `Member<ScriptWrappable>`），`hold`、`held::<T>`、`release` 也是泛型；每個型別的 GC 測試是同一個泛型測試。還是每個型別一份的：wrap 的 impl（V8 的 tag 是 const generic）、`dom_api` 的 `Object` 和 `ObjectKind` 和它們的三個 `match`（`kind`、`fetch`、`visit`）；前端只實作 `FetchObject`、`VisitObject` 的泛型方法，沒有逐型別的程式碼。

### 1. HTML 的片段

2026-10-06 完成（1a 到 1d）。

- **fragment parsing**（`html::parse_fragment`，Blink 的 `HTMLDocumentParser::ParseDocumentFragment`）：照 HTML 的 fragment parsing algorithm，html5ever 的 `parse_fragment_for_element` 建在一份新的、沒有 window 的 document 裡（quirks mode 照 context 的 document），context 只被讀取：決定 tokenizer 的初始狀態、insertion mode、form 的指標（context 自己或祖先的 `<form>`），MathML 的 `annotation-xml` context 照它現在的 `encoding` 屬性算不算 integration point（Blink 的 `HTMLElementStack::IsHTMLIntegrationPoint`，1d 時 WPT 找到）。parser 的屬性步驟照常跑（`onclick` 是 handler）；`<script>` 不執行（kun 本來就只執行 parser 插入的 script）。`dom_api` 把結果移進 context 的 document 的 `DocumentFragment`（adopt），再用 DOM 的「replace all」（`innerHTML`、`setHTMLUnsafe`，`replaceChildren` 和 `textContent` 也改用它）、「replace a child」（`outerHTML`）或「pre-insert」（`insertAdjacentHTML`）放進樹，第 2 階段的 `MutationObserver` 在這幾處各記一筆。
- **scripting flag**：context 的 document 是 fully active 時算 enabled（`<noscript>` 是 raw text），parse 和序列化都照它；`parseHTMLUnsafe`、`DOMParser` 的 document 沒有 window，一律 disabled。
- **iframe**：fragment parser 建在沒有 window 的 document 裡，所以 `dom` 只在根是有 browsing context（有 window、還 active）的 document 時記 iframe 的插入和移出（HTML 的 insertion steps 在根沒有 browsing context 時不做事）；原本照「根是不是 document」，`innerHTML` 插入的 iframe 會多建子 frame（1c-1 找到，RFC 0009 的第 25 條跟著改）。
- **序列化**（`html::serialize_children`、`serialize_node`）：html5ever 的 `serialize` 加上走訪 `dom` 的那一層（「已決定的事項」第 5 條），那一層處理和規範不同的地方：`<template>` 寫內容；HTML、SVG、MathML 以外的 namespace 的元素和屬性用 qualified name；void 元素不寫任何內容（html5ever 會寫出 void 元素底下的文字，1b 找到）、`innerHTML` 是空字串；context 只有 HTML 元素才算 raw text 的 parent。processing instruction 照 html5ever 寫成 `<?target data>`，HTML 新的規則是 `<?target data?>`（已知限制）。
- **成員**（`dom_api` 的 `markup`、`dom_parser`）：`Element` 的 `innerHTML`、`outerHTML`、`insertAdjacentHTML`、`insertAdjacentElement`、`insertAdjacentText`、`getHTML()`（`GetHTMLOptions` 只有 `serializableShadowRoots`，沒有作用）、`setHTMLUnsafe`（沒有 Sanitizer 的選項）；`Document.parseHTMLUnsafe()`（static）；`DOMParser`（`DOMParserSupportedType`）。字串是 `(TrustedHTML or DOMString)`，沒有 Trusted Types，當成字串。`ShadowRoot` 的同名成員等 shadow DOM（規範的 `InnerHTML` mixin 只在 `Element` 和 `ShadowRoot` 上）；`createContextualFragment` 在 `Range` 上，跟著它到之後的 RFC。
- **XML**：XML document 的 `innerHTML`、`outerHTML`、`insertAdjacentHTML` 和 `DOMParser` 的 XML 類型丟 `NotSupportedError`（「已決定的事項」第 2 條）；`getHTML()`、`setHTMLUnsafe`、`parseHTMLUnsafe` 在任何 document 都是 HTML 的。
- **`DOMParser` 的 document**：`text/html`、網址和 origin 是 parser 的 window 的 document 的、realm 是那個 window、UTF-8；`parseHTMLUnsafe` 的是 `about:blank`、realm 是 interface object 的。

### 2. `MutationObserver`

2026-10-07 完成（2a 到 2e）。

- **紀錄在 `dom` 的原語記**（「已決定的事項」第 9 條，照 Blink）：`insert_before`、`remove`、`set_attribute`、`set_character_data` 等原語先看 document 有沒有這種 observer（一個 bit，Blink 的 `HasMutationObserversOfType`），有才找出要通知的 observer、排紀錄；屬性和文字的舊值在原語裡讀。規範把好幾個節點記成一筆的三種演算法（插入 fragment、`replaceChild`、replace all：`innerHTML`、`textContent`、`replaceChildren`）由 `dom_api` 開一個批次：批次擋住插入 target 的紀錄，規範要 "suppress observers" 的移除用不記的移除，結束時 `dom_api` 照規範給這一筆的內容（第 9 條）；批次外每個原語一筆（例如 `normalize` 每移除一個是一筆）。parser 經過同樣的原語，不用另外接（`MutationObserver-document`）。transient registered observer 在 `remove` 裡加，和 `NodeIterator` 的 pre-removing steps 在同一個地方。
- **GC 型別**：`MutationObserver`（callback、紀錄的佇列、經過 weak 集合找到它的註冊，第 12 條）、`MutationRecord`；每個節點的 registered observer 放在 `NodeRareData`，節點強指著註冊、註冊強指著 observer；transient registered observer 照規範，在它的 source 上。
- **每個 agent 一份的狀態**：待通知的 observer 和「已經排了 microtask」的旗標放在 `dom` 的 `DomState`（`dom` 不能排 microtask，只記下有 observer 在等；`dom_api` 在宣告表的每次呼叫結束時、和 `update_frames` 同一個地方經過 `ScriptHost` 排，renderer 跑完一段 parser 之後也做一次）（每個 heap 一份，相當於 Blink 的 `MutationObserverAgentData`），不放在 `Window`，並留位置給 `slotchange`（RFC 0009 的約束）。
- **callback**：在 microtask 裡照第一次拿到紀錄的順序通知，例外回報（`call_reporting`）、下一個照樣跑。wasm 的 guest 是 `mutations` 這個 export（第 10 條），紀錄是 `own`（guest 可以留著，和 `takeRecords()` 的結果一樣），observer 是 `borrow`；Lua 收到紀錄的陣列。

### 3. `AbortController` 和 `AbortSignal`

2026-10-07 完成（3a 到 3d）。

- **`AbortSignal` 是 `EventTargetImpl` 的一種**（第 16 條）：記著 `aborted`、`reason`、abort 演算法的串列（Blink 的 `AbortSignal::AddAlgorithm`）、`any()` 的來源和 dependent；`AbortController` 是新的 GC 型別，撐住它的 signal。`addEventListener` 的 `signal` 加一個「移除這個 listener」的演算法，已經 abort 的不加。
- **abort**：`dom` 設定 signal 和它還沒 abort 的 dependent 的 reason，`dom_api` 照規範的順序對每一個執行演算法、分派 trusted 的 `abort`（`AbortController.abort()` 是 `async method`，guest 自己的 listener 也在它回傳之前跑，第 17 條）。沒給 reason 時當場建 `AbortError`（第 14 條）：V8 經過 `ScriptHost::create_dom_exception` 在 signal 的 realm 建；wasm 沒有 script 的值，`dom` 記下名稱和訊息，guest 的 `throw-if-aborted` 回對應的代碼（`AbortError` 12、`TimeoutError` 13）。
- **`AbortSignal.abort()`、`timeout()`、`any()`**（static）：`timeout` 的參數是 `[EnforceRange] unsigned long long`（新的型別 `EnforcedU64`，WIT 的 `u64`），它的 timer 是 DOM 的 timer 清單裡的 `TimerTask::Abort`（第 13 條：撐住 signal、`clearTimeout` 清不掉、不算巢狀、跟著 document 清掉，`DomIsolate` 拆掉時也清掉）；`any()` 的來源和 dependent 互相強指著（第 15 條），dependent 攤平成它的來源。
- **`reason` 是 `any`**，只給 JS；`abort(optional any reason)` 照樣在 `kun:dom` 裡，optional 的 `any` 參數只從 guest 的函式拿掉，guest 呼叫時是 `undefined`（回傳 `any` 或有必填 `any` 參數的成員才整個不在 WIT 裡）。

### 4. traversal

2026-10-07 完成（4a 到 4d）。

- **GC 型別**：`NodeIterator`、`TreeWalker`，共用的 `TraversalBase`（Blink 的 `NodeIteratorBase`：root、`whatToShow`、filter、「執行中」的旗標）；iterator 的 reference 和 pointer before reference，walker 的 `currentNode`。document 弱指著它的 iterator（第 12 條）。
- **pre-removing steps**（「共用的基礎」的 hook）：被移除的節點是 reference 的 inclusive ancestor、又不是 root 的 inclusive ancestor 時，照規範移動 reference。規範的 traverse 還讓 iterator 記著正在 filter 的位置（script 看不到，`referenceNode` 是上一次接受的），移除時一樣調整：filter 移除它時，接受就回傳原本的節點、reference 是移到的位置，不接受就從那裡繼續（WPT 的 `NodeIterator-removal-during-filtering`，測試說 Blink、WebKit 還沒照做）。
- **走訪**（`dom_api` 的 `traversal`）：照規範的步驟；`TreeWalker.nextNode` 往上找兄弟走到沒有 parent 時回傳 `null`（照 Chromium，規範的寫法會一直重試）。會呼叫 filter 的是 `async method`。filter：「執行中」時 `InvalidStateError`（`NodeIterator` 在碰任何狀態之前檢查）；realm 不能跑 script（document 不再 fully active）的 filter 不呼叫，丟 `TypeError`（Blink 的 `IsCallbackFunctionRunnable`，它丟 `Error`）；filter 的例外原樣往外丟（第 18 條）；回傳值轉成 `unsigned short`（物件當成 0，見 `docs/limitations.md`）。
- **`NodeFilter`** 是 callback interface，不在 `INTERFACES` 裡（`CALLBACK_INTERFACES`）；JS 在每個 realm 有它的常數的介面物件（WebIDL 的 legacy callback interface object）。
- **wasm**：guest 的 export `accept-node(id, node) -> option<u16>`（第 10 條）；`none` 是 filter 失敗了，走訪停下，guest 的那次呼叫以代碼 14（`CallbackError`）失敗。Lua 的 runtime 留著 filter 的錯誤、回傳 `none`，再在呼叫以代碼 14 失敗時把它丟給呼叫者，和 JS 一樣。

### 5. `Range`

2026-10-07 移到之後的 RFC（第 19 條，「之後的可能」）。原本的範圍：`AbstractRange`、`StaticRange`、`Range`，live range 接上「節點移除之前」的 hook（加上插入、`replaceData`、`splitText`、`normalize` 的調整），成員是邊界的設定、`collapse`、`selectNode(Contents)`、比較、`cloneContents`、`extractContents`、`deleteContents`、`insertNode`、`surroundContents`、`cloneRange`、`detach`、`toString`、`createContextualFragment`；邊界不寫死「根是 document 或 fragment」（RFC 0009 的約束）。

### 測試

- **語意**：每個成員照舊在 `dom_api` 的 `tests/` 測（不經過 engine），WPT 是第二層。新的基礎各自有單元測試：fragment parsing 跑 html5lib 的 `#document-fragment` 測試；microtask 的時機在 `dom_js` 測；「節點移除之前」的 hook 在 `dom` 測（parser 的移除也算）。
- **WPT**：每個階段匯入它解開的目錄、更新 baseline，數字記在 `progress.md`。
- **wasm**：新的宣告表形式和新的 GC 型別在 `crates/dom_wasm/tests/` 用 C 的 guest 測一次橋接，`MutationObserver` 和 `NodeFilter` 的 callback、`AbortController` 也是；Lua 的產生器和 runtime 由 W9 測。
- **GC**：每個新的 GC 型別加進 `dom` 的測試工作負載和 `gc_stress`；在 macOS 上用 debug 的 V8 跑 `test.py matrix` 和 `fuzz`（第 4 階段起在階段的驗收時跑一次）。
- **mutation test**：每個階段都跑，結果寫進 `progress.md`。
- **在視窗裡看得到的**：`about:demo` 的示範（`innerHTML`、`MutationObserver`、`AbortController`、`TreeWalker`）和 `about:wasm-demo` 的 Lua observer，寫成情境；每個階段的驗收在真的視窗裡跑（`test.py automation`）。

### 分階段

順序照收益和依賴（`details.md` 的「WPT 的調查」的「每一組解開多少」和「組和組之間的依賴」）：

1. **HTML 的片段**：已經匯入的目錄裡擋住最多 subtest、解開最多沒匯入的目錄（`domparsing/` 不要 XML 的部分、`html/syntax/` 的 fragment、`html/semantics/scripting-1/the-template-element/`、`html/webappapis/dynamic-markup-insertion/`）；其他組的測試也用它建樹（`MutationObserver-inner-outer`、`selection/` 的一半）。
   - 1a parser 的依賴方向、fragment parsing、html5lib 的 fragment 測試；
   - 1b 序列化；
   - 1c `innerHTML` 等成員、`DOMParser`，分成兩段：1c-1 `innerHTML`、`outerHTML`、`insertAdjacent*`、`getHTML()`、`setHTMLUnsafe`（不用新的宣告表形式，先讓 `innerHTML` 看得到）；1c-2 宣告表的 `static` 和 `enum`（第 6 條），和第一批用到它們的 `Document.parseHTMLUnsafe`、`DOMParser`；
   - 1d 匯入 WPT、baseline、mutation test、文件。
2. **`MutationObserver` 和 `queueMicrotask`**：已經匯入的目錄裡第二多；放在第 1 階段之後，`innerHTML` 這條新的修改路徑一開始就接上紀錄。
   - 2a 降低新增 GC 型別的成本（第 8 條）：`ScriptObject` trait 和泛型、slot 的 `TaggedEdge`、前端的 `FetchObject` 和 `VisitObject`、泛型的 GC 測試；現有的十個型別都改過去，行為不變；
   - 2b `ScriptHost` 排 microtask、`queueMicrotask`、wasm 的 checkpoint；guest 的 export 換成 `handle-event`、`run`、`frame`（第 10 條），重編所有 guest；
   - 2c-1 `dom` 這一層：weak 的集合（第 12 條）；`MutationObserver`、registration、`MutationRecord`；在原語裡記、批次、transient observer、待通知的 observer（第 9 條）；GC 的測試、`matrix`、`fuzz`；
   - 2c-2 `dom_api` 和 JS：`observe`、`disconnect`、`takeRecords`、在 microtask 裡通知；sequence 參數；
   - 2d wasm 的 `mutations` export（2d-1）、Lua 的產生器和 runtime、`about:demo` 和 `about:wasm-demo` 的示範（2d-2）；
   - 2e WPT、mutation test、文件。
3. **`AbortController` 和 `AbortSignal`**：小、獨立；新的 event target，`throwIfAborted` 先用到「例外往外丟」（丟的是值，不經過 callback）。
   - 3a `AbortController`、`AbortSignal`、`abort()`、`reason`、`onabort`、`throwIfAborted`；`addEventListener` 的 `signal`；
   - 3b `AbortSignal.abort()`、`timeout()`、`any()`；GC 的測試、`matrix`、`fuzz`；
   - 3c wasm、Lua、`about:demo`；`async method`（第 17 條）；
   - 3d 匯入 `dom/abort`、第 3 階段的驗收。
4. **traversal**：小；先在 `NodeIterator` 上做「節點移除之前」的 hook，之後的 `Range` 沿用。
   - 4a `dom` 這一層：`NodeIterator`、`TreeWalker`、document 的 weak 集合、pre-removing steps；
   - 4b `dom_api` 和 JS：走訪、呼叫 callback 時交還例外（第 18 條）、`NodeFilter`、`about:demo`；guest 的 `accept-node`（宣告表一加 traversal，WIT 就跟著變）；
   - 4c wasm 和 Lua 的測試；
   - 4d 匯入 `dom/traversal`、第 4 階段的驗收。
5. **`Range` 和 `StaticRange`**：最大，解開 `dom/ranges` 和 `MutationObserver` 剩下的 subtest；2026-10-07 移到之後的 RFC（第 19 條）。

## 缺點

- 新增六個 GC 型別（加上 `EventTargetImpl` 的一種）。2a（第 8 條）之後每個還是要寫 struct、`script_objects!` 一行、`Object` 和 `ObjectKind` 和 `dom_api` 的幾個分支，比 Blink 從 IDL 產生的多；2a 本身改動了所有現有的型別，並多一處「比對 tag 再轉型」的 unsafe。
- `dom` 的修改原語多做兩件事（`MutationObserver` 的紀錄和 `NodeIterator` 的 pre-removing steps，第 9、12 條），所有路徑都經過它們，不會漏接；代價是 `dom` 變大，每個原語多一個「有沒有 observer」的判斷，kun 自己內部的樹操作（建 `about:blank`、複製 template 的內容）在有人觀察時也會產生紀錄，不該產生的要加測試守住。只有規範把好幾個節點記成一筆的三種演算法要記得開批次。
- `kun:dom` 的 guest export 改形狀，所有編好的 guest 要重新編（`ci.py guests`）；每種 callback 一個 export（第 10 條），每加一種 callback，每個直接實作 export 的 guest（C 的測試 guest、SDK 的 C 和 Rust 範例、reftest 的 guest）都要多實作一個，用不到的也要寫一個（4b 的 `accept-node` 改了 11 個檔案）。
- `ScriptHost::call` 的介面改了兩次（交還例外、帶著 callback interface 的種類，第 18 條），每個 `ScriptHost` 的實作和呼叫點都跟著改，測試的 host 也是。
- XML 的部分留到之後，`DOMParser` 和 XML document 的 `innerHTML` 和瀏覽器不同。

## 理由和其他做法

- **五組放在一份 RFC**：它們共用同一批基礎（修改操作的入口、microtask、callback、新的 GC 型別），分成五份 RFC 時這些設計會散在各份裡；每組各是一個階段，一樣可以分開決定和驗收。不選「每組一份 RFC」。`Range` 最大，第 4 階段做完時拆了出去（第 19 條）。
- **先做 HTML 的片段**：數字最大，其他組的測試也用它；而且它是新的修改路徑，`MutationObserver` 在它之後做，就不用回頭補。不選「先做 `MutationObserver`」：已經匯入的目錄裡它也很多，但 `innerHTML` 之後還要補一條路徑。也不選「先做小的（`AbortController`、traversal）」：基礎可以先在小的 API 上試，但收益最慢。
- **XML 另開 RFC**：要一個 XML parser（Chromium 用 libxml2；Rust 有 xml5ever、quick-xml 等，各有不合規範的地方），`domparsing/` 的 XML 測試也幾乎都同時要 parser 和序列化；只做序列化的話，WPT 幾乎沒有收益。

## 先例

- **Chromium**（`third_party/blink/renderer/core/`）：
  - 片段：`editing/serializers/serialization.cc`（`CreateFragmentForInnerOuterHTML`、`CreateMarkup`）、`editing/serializers/markup_accumulator.cc`，parser 的 `html/parser/html_document_parser.cc` 的 `ParseDocumentFragment`。
  - `MutationObserver`：`dom/mutation_observer.cc`、`mutation_observer_registration.cc`、`mutation_observer_interest_group.cc`、`child_list_mutation_scope.cc`，每個 agent 一份的 `MutationObserverAgentData`。
  - `AbortSignal`：`dom/abort_signal.cc`、`abort_controller.cc`，`AddAlgorithm`。
  - traversal：`dom/tree_walker.cc`、`node_iterator.cc`、`node_iterator_base.cc`（`NodeFilter` 的呼叫和 active flag）。
  - `Range`（之後的 RFC）：`dom/range.cc`、`static_range.cc`、`abstract_range.cc`，`Document::NodeWillBeRemoved` 等調整 live range。
- kun 照這些類型和分層，和 Chromium 不同的地方（例如紀錄在哪一層組）寫在各段的決定裡。

## 已決定的事項

1. **五組放在這份 RFC，照「分階段」的順序做**（2026-10-06；`Range` 在 2026-10-07 拆出去，第 19 條）：HTML 的片段、`MutationObserver` 和 `queueMicrotask`、`AbortController`、traversal、`Range`。XML 的 parser 和序列化、`Selection`、shadow DOM 有關的部分不在範圍內。理由見「理由和其他做法」。不選「`Range` 現在就拆出去」：它和 traversal 共用「節點移除之前」的 hook，到第 5 階段時如果太大再拆；也不選「先做小的」：收益最慢。
2. **XML document 的 `innerHTML`、`outerHTML` 和 `DOMParser` 的 XML 類型丟 `NotSupportedError`**（2026-10-06）：記成 `docs/limitations.md` 的已知限制，XML 的 parser 和序列化另開 RFC。Chromium 用 libxml2 的 `XMLDocumentParser::ParseDocumentFragment` 和 `MarkupAccumulator` 的 XML 模式。代價（1c-2 時看到）：先檢查 `"DOMParser" in window` 再用 XML 類型的測試，原本跳過，現在碰到 `NotSupportedError`（WPT 的 `Element-tagName` 有一個 subtest 從通過變成失敗）。不選「getter 先做 XML 序列化」：`domparsing/` 的 XML 測試幾乎都同時要 parser，只做一半沒有收益；也不選「這份就加 XML parser」：範圍變大很多。
3. **新增 GC 型別的成本在第 2 階段開始前再評估**（2026-10-06；1c-2 時改）：第 1 階段只加 `DOMParser`（只有 wrapper 和建立它的 window 的 edge），照現在的做法（`crates/dom/README.md` 的「新增 script 拿得到的 GC 型別」）；第 2 階段的 `MutationObserver` 開始大量加，到時候決定要不要先降低成本（RFC 0008 的「之後的可能」），`DOMParser` 是一個實際的例子。原本寫的是「第 1 階段不加 GC 型別」：開始 1c-2 時才發現 `DOMParser` 要有自己的物件（`new DOMParser()` 的身分，`parseFromString` 要它的 window 的 document 的網址），2026-10-06 確認照現有做法加。不選「`DOMParser` 延到評估之後」：`enum` 唯一的使用者就是它，1c-2 會少一半；也不選「先做評估」：第 2 階段的型別多，評估時才看得出要省什麼。
4. **parser 只依賴 `dom`，元素的屬性步驟由呼叫者傳入；`dom_api` 依賴 `html`**（2026-10-06，第 1 階段）：`html` 定義一個 trait，parser 建元素、補屬性時經過它執行元素的屬性改變的步驟（`onclick` 變成還沒編譯的 handler、iframe 的 `src`），`dom_api` 實作它；renderer 建 `Parser` 時傳入 `dom_api` 的實作，`dom_api` 的 `innerHTML` 等直接呼叫 `html` 的 fragment parsing。和 Blink 的呼叫關係一樣：`HTMLConstructionSite` 經過 `Element::ParserSetAttributes` 呼叫 virtual 的 `AttributeChanged`，`DocumentFragment::ParseHTML` 直接呼叫 `HTMLDocumentParser::ParseDocumentFragment`（`details.md` 的「Chromium 怎麼做」）；Blink 的兩邊在同一個 component，kun 拆成兩個 crate，用 trait 斷開循環。不選「parser 併進 `dom_api`」：和現在把 parser 獨立成 crate（對應 `core/html/parser`）的分法不一致，`dom_api` 變大；也不選「經過 embedder 的 hook」：Blink 的 fragment parsing 不經過 frame，沒有 frame 的 document 也要能 parse。
5. **序列化用 html5ever 的 `serialize`，kun 只寫走訪 `dom` 的樹的那一層**（2026-10-06，1b）：html5ever 已經是依賴，跟著 parser 一起更新；raw text 的元素、`<noscript>` 照 scripting flag、跳脫字元（屬性裡的 `<`、`>` 也跳脫，規範 2025 年的改動）、void 元素都照規範。和規範不同的地方由走訪的那一層處理：`<template>` 序列化它的內容；HTML、SVG、MathML 以外的 namespace 的元素和屬性用 qualified name（html5ever 只寫 local name，屬性寫成 `unknown_namespace:`）；void 元素的 `innerHTML` 是空字串；context 不是 HTML 元素時（SVG 的 `<style>`），它的文字照樣跳脫。不選「照 Blink 的 `MarkupAccumulator` 自己寫」：多維護一份 HTML 的部分，XML 的序列化另開 RFC 時再看要不要自己寫。
6. **宣告表支援 `static` 的方法和 WebIDL 的 `enum`**（2026-10-06，1c）：`Document.parseHTMLUnsafe()`、`DOMParser.parseFromString` 的 type 用到，第 3 階段的 `AbortSignal.abort()`、`timeout()`、`any()` 也是 `static`。JS 的前端把 `static` 的方法裝在 interface object 上，`enum` 照 WebIDL 轉成字串、不是成員就丟 `TypeError`；WIT 是 resource 的 `static` func 和 `enum` 型別；Lua 的產生器跟著支援。不選「`enum` 先當成 DOMString、在 `dom_api` 檢查」：JS 看起來一樣，但 WIT 失去型別，之後還要改一次（不相容的改動）；也不選「都先不加」：第 3 階段一定要 `static`。
7. **單獨的 heap 的 marking 步驟，第一步之後由 kun 自己排**（2026-10-06，1c-2 的 `test.py fuzz` 找到的問題）：cppgc 的 `IncrementalMarkingTask` 跑完不清掉自己的 handle，`MarkerBase::ScheduleIncrementalMarkingTask` 以為還排著，所以單獨的 `cppgc::Heap` 每一輪 GC 只排第一步；第一步沒標完的 GC 停在 marking（barrier 一直開著、垃圾不回收），要等配置量到了 atomic GC 的上限。上游 V8 的程式碼一樣；Blink 的 heap 都是接在 isolate 上的 `CppHeap`，marking 由 V8 的 `IncrementalMarkingJob` 推進（它的 task 一開始就清掉「已排」的標記，每一步之後重排），所以碰不到。做法：fork 的 glue 加 `StandaloneHeap::is_marking` 和 `marking_step`（做 cppgc 那個 task 做的事，標完時用 heap 開始這輪 GC 的設定收尾），`run_platform_tasks` 跑完一批之後還在 marking 就排一步，一個 turn 一步；和 V8 從外面推進 `CppHeap` 的分層一樣，V8 本身不改。不選「改 V8 的 `marker.cc`」：只要一行，是根本的修法（給上游的回報草稿是 `cppgc-marking-task-report.md`），但 fork 從沒改過 V8 本身，要多一個 v8 的 fork，或在 build 前套 patch（`ci.py v8` 和 release 的 workflow 都要改）；也不選「單獨的 heap 改成 atomic marking」：只改 kun、不用重編，但每次 GC 的停頓和活的 heap 成正比，單獨的 heap 上 marking 途中寫 slot 的覆蓋也沒了。代價：glue 用了 cppgc 內部的 `heap.h`（`Heap::FinalizeIncrementalGarbageCollectionIfRunning`），升級 V8 時由 `bindings` 的 `build.rs` 比對它和 cppgc 的那幾段。

8. **新增 GC 型別的成本先降低：`ScriptObject` trait 加泛型，wasm 的 slot 每格是不帶型別的 edge 加上 tag**（2026-10-06，第 2 階段開始前，第 3 條的評估）：1c-2 加 `DomParser` 時，它自己的定義 20 行，讓 script 拿得到它要在 13 個檔案改大約 70 處：每個型別一樣的 handle（約 40 行）、`own_x`、wrapper 和 unwrap 的函式、slot 表的 edge 和 `HeldRef`、`Held`、`hold`、`release`、`held` 的分支（`release`、`held` 和 `dom_js` 的 `object()` 漏寫時編譯得過；`dom_js`、`dom_wasm` 各兩個逐型別的轉換）、`dom_api` 和兩個前端的分支，GC 測試也每個型別重寫一次；slot 表每格每種型別一條 edge、只用一條，每多一個型別，guest 的每個 handle 都變大，`release` 要清每一條。做法：
   - **trait**：給 script 拿的 GC 型別實作 sealed 的 `ScriptObject`（`ObjectTag`、名稱、wrapper slot、canary），handle 是 `Handle<T: ScriptObject>`（原本的名字留成別名），`own`、wrapper、unwrap 是泛型。wrap 的 impl 還是每個型別一行（V8 的 tag 是 const generic，泛型函式裡寫不出 `T::TAG`），一個 tag 只對應一個型別由測試斷言。
   - **slot 表**：每格是一條不帶型別的 edge（`AnyEdge`）加上 `ObjectTag`，`hold`、`held::<T>`、`release` 是泛型，前端從 `held_tag` 分派。fork 在 C++ 那一側的 edge 本來就是 `Member<RustObj>`，trace 經過物件自己的 `dyn GarbageCollected`，所以不用改 C++；marker 只讀 edge，tag 只在 heap 的 thread 上讀寫。新的 unsafe 是 `held::<T>` 比對 tag 之後的轉型，和把 `Persistent<T>` 存進不帶型別的 edge。
   - **前端沒有逐型別的程式碼**：`dom_api` 的 `Object` 和 `ObjectKind` 每個型別一項，`Object::kind`、`Object::fetch`（從 `ObjectTag` 取回 `Object`）、`Object::visit`（對裡面的 handle 做事）是逐型別的 `match`，都是窮舉的（2a 做的時候，語意上本來就逐型別的 `as_event_target` 和從 `dom_js` 搬過來的 `realm_window` 也在這裡，加上 `object_handle!` 一行）；前端實作兩個 trait 的泛型方法：`FetchObject::fetch::<T>`（`dom_js` 是 unwrap，`dom_wasm` 是 `held::<T>`）和 `VisitObject::visit::<T>`（`dom_js` 是 wrapper，`dom_wasm` 是 `hold`）。例外（`Window` 的 wrapper 是 realm 的 global、`Node` 要選最衍生的 interface template）由前端的 `visit` 先判斷，或由 trait 的方法覆寫。
   - **測試**：每個型別呼叫同一個泛型的 GC 測試（slot 撐住、經過 GC 還在、release 之後回收、拒絕別的 isolate 的），手寫的只剩它自己的語意。

   和 Chromium 的對應：`ScriptObject` 是 `ScriptWrappable`，tag 是 `WrapperTypeInfo`，不帶型別的 edge 是 `Member<ScriptWrappable>`；Blink 的 `Object` 那一層是動態轉型，kun 留著 `Object` 的列舉，換編譯期的檢查。放在第 2 階段的最前面（2a），現有的 10 個型別都改過去、行為不變，`MutationObserver` 開始用新的做法。不選「slot 寫成 Rust 的 `enum`」：換變體是不經過 write barrier 的 memcpy（incremental marking 途中放進去的物件可能沒被標到），concurrent marker 也可能讀到新的 discriminant 配舊的 payload；不帶型別的 edge 加上 tag 就是把 enum 拆成 marker 讀的 edge 和只有 Rust 讀的 tag。不選「macro 從型別清單產生每個型別的程式碼」：trait 和泛型做得到的部分不需要 macro，產生的程式碼也比較難讀；不選「照現在的做法逐一加」：之後的 8 個型別每個都要重複一次；也不選「`Object` 改成一種 handle 加上動態轉型」（完全照 Blink）：等於重寫 `dom` 和 `dom_api` 的物件模型，也失去編譯期的檢查，範圍比這份 RFC 大。

9. **`MutationObserver` 的紀錄在 `dom` 的原語記，好幾個節點合成一筆由 `dom_api` 開批次**（2026-10-06，第 2 階段開始前）：
   - **原語記**：`dom` 的 10 個修改原語（`insert_before`、`append_child`、`remove`、`adopt_node`、`set_attribute`、`remove_attribute`、`set_attr_node`、`set_attr_value`、`append_text`、`set_character_data`）先看 document 有沒有這種 observer，有才找出要通知的 observer（往上走祖先，比對 `subtree`、`attributeFilter`、要不要舊值）並排紀錄；舊值在原語裡讀。transient registered observer 在 `remove` 裡加。
   - **批次**：規範把好幾個節點記成一筆的只有插入 fragment、`replaceChild`、replace all；`dom_api` 在這三種演算法外面開批次。批次只擋住插入 target 的紀錄；規範用 "suppress observers" 的移除，`dom_api` 呼叫不記的移除（`remove_without_record`，transient observer 照加）；結束批次時 `dom_api` 照規範給這一筆的 `addedNodes`、`removedNodes`、`previousSibling`、`nextSibling`，fragment 的子節點移出是 fragment 自己的一筆（`record_child_list`）；批次外每個原語一筆。
     2026-10-07（2c-2）從「批次裡同一個 target 的變動自動合成一筆」改成這樣：`replaceChild` 和插入裡的 "adopt"（把節點從原本的 parent 移出）不 suppress，要是自己的一筆，而原本的 parent 可能就是 target（換成自己的兄弟，WPT 的 `MutationObserver-childList`），自動合併分不出哪些移除該併進來；一筆的兄弟節點規範也是在演算法開始時算的（移出 adopt 的節點之前）。所以哪些變動不記、這一筆是什麼，由照著規範步驟寫的 `dom_api` 說，`dom` 只負責不記和記下。Blink 的 `ChildListMutationAccumulator` 是累積連續的變動、不連續時先送出前一筆，結果在這三種演算法上相同。
   - **microtask**：`dom` 只在 `DomState` 記下有 observer 在等；`dom_api` 在宣告表的每次呼叫結束時（`update_frames` 的地方）、renderer 在跑完一段 parser 之後，經過 `ScriptHost` 排「通知 mutation observer」。
   - **`dom_api`**：`observe`、`disconnect`、`takeRecords` 的語意和呼叫 callback。

   Blink 也在最底層記：`ContainerNode` 的插入和移除（parser 的 `ParserAppendChild` 也是）經過 `ChildListMutationScope`，屬性在 `Element::WillModifyAttribute`，文字在 `CharacterData::DidModifyData`，連續的變動由 `ChildListMutationAccumulator` 在最外層的 scope 結束時合成一筆；沒人觀察時先問 `HasMutationObserversOfType`；transient observer 在 `NotifyMutationObserversNodeWillDetach`。那一層對應 kun 的 `dom`。不選「在 `dom_api` 的演算法裡組」（原本的看法）：parser 只依賴 `dom`（第 4 條），要經過 `ElementHooks` 另外接，`token_list`、`frame`、`dom_implementation` 這類直接呼叫 `dom` 的地方也要逐一補，之後每個新的修改操作都要記得接，漏了只有 WPT 抓得到；transient observer 在 parser 的移除也拿不到。代價：`dom` 多了 observer 的註冊和找出要通知的 observer；每個原語多一個判斷；批次的合併規則要對照 WPT 的 `MutationObserver-childList`；kun 內部的樹操作在有人觀察時也會產生紀錄，不該有的要加測試。

10. **guest 的 callback 是每種 callback 一個 export**（2026-10-06，第 2 階段開始前）：原本的 `callback(id, event: option<borrow<event>>, number: f64) -> bool` 是湊出來的 union（`event` 和 `number` 只有一個有意義，`bool` 只有 event handler 用），帶不了 `MutationCallback` 的 `(records, observer)` 和 `NodeFilter` 的 `(node) -> u16`。換成照 WebIDL 的 callback 型別各一個 export，參數和回傳由 wasmtime 在連結時檢查：
    - `handle-event(id, event: borrow<event>) -> bool`：listener 和 event handler（回傳 false 取消事件，listener 的不看）；
    - `run(id)`：timer、`queueMicrotask`（`VoidFunction`）；
    - `frame(id, time: f64)`：`requestAnimationFrame`；
    - `mutations(id, records: list<own<mutation-record>>, observer: borrow<mutation-observer>)`：第 2 階段；紀錄是 `own`，guest 可以留到 callback 之後（和 JS、`takeRecords()` 的結果一樣），drop 時才放掉 slot；observer 是 guest 自己建的，`borrow`；
    - `accept-node(id, node: borrow<node>) -> option<u16>`：第 4 階段；`none` 是 filter 失敗了（Lua 的錯誤），traversal 停下，跑 filter 的那次呼叫以代碼 14（`CallbackError`）失敗，guest 自己把原因丟給呼叫者（2026-10-07 決定，第 18 條的 wasm 那一半）。

    host 要知道一個 callback 是哪一種，才選得出 export：在 guest 交出 callback 的地方（宣告表的參數型別，例如 `addEventListener` 的、`MutationObserver` 建構子的）記下種類，呼叫時照種類選，不再從參數的樣子猜（現在的 `callback_args`）；細節寫程式時定。和 Chromium 的對應：IDL 編譯器替每種 callback 產生一個類別（`V8MutationCallback::InvokeAndReportException(records, observer)`、`V8NodeFilter`），參數和回傳的型別固定。代價：每加一種 callback 多一個 export，直接實作 export 的 guest（13 個 C 的 guest、Rust 的 demo）都要多實作一個，用不到的寫空的，不然連結失敗；Lua 的 runtime 每個 export 實作一次，轉給 Lua 的函式。不選「一個 export，參數和回傳是 variant」：一個入口、加 case 時 C 的 guest 原始碼不用改，但回傳的 case 要由 host 在執行時檢查；也不選「一個 export，參數是動態的 `list<value>`」：guest 要自己檢查型別，每加一個 GC 型別 `value` 也要改。

11. **microtask 是 `dom_api` 的 enum，engine 原封不動交還；guest 的 callback 的種類由宣告表的 WebIDL callback 型別決定**（2026-10-06，2b 開始前）：
    - **microtask**：`ScriptHost::queue_microtask(Microtask)`，`enum Microtask { Callback(ScriptCallback), NotifyMutationObservers }`（`queueMicrotask` 的 callback、2c 的通知）。engine 不解讀它。佇列在 `dom` 的 `DomState`（每個 agent 一個，2b 做的時候定的：wasm 的 host 拿得到，`DomIsolate` 拆掉前清得掉裡面的 root）：V8 排一個原生的 function 當跳板（fork 的 `Isolate::enqueue_microtask` 只收 function，不收 C 函式），帶著 microtask 的編號，跳板照編號取回它交給 `dom_api::run_microtask`，所以一個跳板因為 context 不在而沒執行，不會讓後面的錯位；wasm 的 engine 在 checkpoint 時從最舊的開始一個一個交給 `dom_api::run_microtask`，途中新排的也在同一次 checkpoint 跑完。所以新增一種 microtask 只改 `dom_api`，前端沒有逐種類的程式碼（和第 8 條一樣）。
    - **callback 的種類**：宣告表的 `Callback` 帶 WebIDL 的 callback 型別（`VoidFunction`、`FrameRequestCallback`、`TimerHandler`……），guest 交出 callback 時 `dom_wasm` 照它記下要呼叫哪個 export（第 10 條），不再從參數的樣子猜。

    Chromium：`EventLoop::EnqueueMicrotask(base::OnceClosure)` 把 closure 放進 `pending_microtasks_`，再往 V8 的 queue 排一個 C 函式 `RunPendingMicrotask`，它從最前面取出 closure 執行（`platform/scheduler/common/event_loop.cc`）；`MutationObserver` 和 `queueMicrotask` 都走這條。callback 的型別由 IDL 編譯器產生（`V8VoidFunction`、`V8FrameRequestCallback`、`V8MutationCallback`），呼叫時直接用那個型別。和 Chromium 不同的是 microtask 用資料、不用 closure：closure 是 Chromium 通用的工作單位，Blink 只有一個 engine；kun 有兩個 engine 都要保存它、交還給 `dom_api`，enum 的每種工作帶什麼資料（撐住哪些 root）一目了然。不選「closure」：它的通用性要到 `dom_api` 以外的 crate 要排 microtask 時才用得到（例如圖片的「update the image data」、media 的「await a stable state」），到時候加一個 `Other(Box<dyn FnOnce(&mut dyn ScriptHost)>)` 的變體就好，engine 不用改；也不選「改 fork，加上排 C 函式的版本」：要改 C++、重編 V8，跳板的 function 已經能做到。callback 的種類不選「`dom_wasm` 照成員名稱判斷」：這份知識會藏在 `dom_wasm`，每加一個成員都要記得改。

12. **observer 弱指著它的註冊，節點強指著註冊、註冊強指著 observer**（2026-10-06，2c 開始前）：照規範（節點強指著它的 registered observer，registered observer 弱指著節點）和 Blink（節點的 `NodeMutationObserverData` 強指著 `MutationObserverRegistration`，observer 用 `HeapHashSet<WeakMember<MutationObserverRegistration>>` 弱指著它們）：被觀察的節點還在，observer 就在；observer 不讓節點活著。observer 記住註冊用一個新的 weak 的集合（一串固定位置的 `WeakEdge`，像 `EdgeChunks`，加上空格重用），`disconnect` 經過它找到每個註冊。`WeakEdge` 早就有但沒人用過，weak 的清除第一次被走到，要另外測試、跑 `matrix` 和 `fuzz`、寫進 `soundness.md`；第 4、5 階段的 `NodeIterator`、`Range` 要的「document 弱指著它的 iterator 和 range」沿用它。不選「observer 強指著節點」：最簡單，但頁面拿著 observer 時，它觀察過的節點（包括已經拿掉的）一直活著直到 `disconnect`，和規範不同，WPT 也測不出這個洩漏。

13. **`AbortSignal.timeout()` 是 DOM 的 timer 清單裡的一種內部動作**（2026-10-07，第 3 階段開始前）：timer 清單的一項是 `setTimeout` 的 callback，或「時間到時 abort 這個 signal」（像第 11 條的 `Microtask`，`dom_api` 的列舉交還給它執行）。記在呼叫者的 window（RFC 0009 的約束），document 被丟棄時一起清掉；沒有 script 看得到的 id（`clearTimeout` 清不掉），不套巢狀的下限；時間到之前，timer 撐住那個 signal。V8 和 wasm 的 engine 都經過同一份清單，renderer 不用另外接。script 看到的和 Blink 一樣。Blink 的結構不同：它往 `ExecutionContext` 的 timer task runner 排一個延遲 task，和 `setTimeout` 的 `DOMTimer` 是兩套；kun 沒有每份 document 的延遲 task runner。不選「照 Blink 先做 task runner」：現在只有這一個使用者，renderer 和 wasm engine 都要多一條排程的路（丟棄時清掉、和計時器一致的節流），也看不出通用的設計對不對；等有第二個要「延遲執行 DOM 自己的工作」的功能（「之後的可能」）再一起做，把 `timeout` 搬過去。代價：timer 清單原本只對應 HTML 的 active timers，給 id、巢狀下限、`clearTimeout` 的地方要跳過這一種；同一毫秒到期時和 `setTimeout` 的先後可能和 Chromium 不同（兩者在規範裡都是 timer task source，WPT 不測）。

14. **`reason` 預設的 `DOMException` 在 abort 的當下建立**（2026-10-07，第 3 階段開始前）：照 Blink（`abort()` 沒給 reason 時當場在 signal 的 realm 建 `AbortError`，timeout 到時建 `TimeoutError`）。`ScriptHost` 多一個建 `DOMException` 的方法：V8 經過膠水在 signal 的 realm 建（kun 的 `DOMException` 是 JS 定義的）；wasm 的 engine 沒有 script 的值，記成哪一種 `DOMException`，guest 的 `throw-if-aborted` 回對應的 `dom-exception` 代碼（`reason` 是 `any`，本來就不在 `kun:dom` 裡）。不選「第一次讀 `reason` 時才建」：engine 的改動比較少，但建立的時間點和 realm 和 Blink 不同（例如 abort 之後才換掉 `DOMException`，script 看得出差別）。

15. **`AbortSignal.any()` 的來源強指著 dependent，timer 強指著 timeout 的 signal**（2026-10-07，第 3 階段開始前）：規範要求還可能被 abort、又有 `abort` listener 的 signal 不被回收；abort 只會從來源或 timer 來，所以強指著一定對。dependent 也強指著它的來源（`any()` 的參數是 dependent 時要讀它的來源，3b），兩者成環，由 GC 一起收。代價：沒有 listener 的 dependent 也活到它的來源被回收為止，一個長命的 signal 一直被拿去 `any()` 時會累積（記進 `docs/limitations.md`）；kun 還沒有 `fetch`，這種用法現在少見。Blink 是雙向 weak，加上「還有待處理的工作」（`HasPendingActivity`）撐住有 listener、還可能 abort 的 signal。不選照 Blink：`dom` 的 GC 要多一種條件式的 root，要寫 soundness、跑 `matrix` 和 `fuzz`，第 3 階段大很多；不選「來源用第 12 條的 weak 集合，dependent 有 listener 或 abort 演算法的期間改成強指著」：不會漏，但每次增減 listener 都要維護這個狀態。有 `fetch` 之後照 Blink 改（「之後的可能」）。

16. **`AbortSignal` 是 `EventTargetImpl` 的一種**（2026-10-07，3a 開始前）：`EventTargetImpl` 多一個種類，一般的 event target 或 abort signal（`aborted`、`reason`、abort 演算法的串列，3b 加上 `any()` 的來源和 dependent）；分派、listener 的串列、`TargetEdge`、wrapper 和 slot 表都沿用，script 看到的 interface 照種類決定（`EventTarget` 或 `AbortSignal`），和 `Node` 一樣（Blink 的 `Element`、`Text`、`HTMLIFrameElement` 在 kun 是同一個 GC 型別，照種類分 interface）。`AbortController` 不是 event target，照第 8 條是一個新的 GC 型別，拿著它的 signal。Blink 的 `AbortSignal` 直接繼承 `EventTarget`，各自是 C++ 的子類別。不選「`AbortSignal` 是自己的 GC 型別」（Blink 的結構）：`TargetEdge`、`EventTarget` 的列舉、分派的路徑、listener 的串列都要多一種，改動散在 `dom` 的事件系統各處，之後每多一種不是節點的 event target 都要再走一次。

17. **會同步呼叫回 script 的成員在宣告表裡寫成 `async method`**（2026-10-07，3c）：`AbortController.abort()` 在回傳之前同步分派 `abort`（和 Blink 的 `AbortSignal::SignalAbort` 一樣），在 WIT 裡卻是同步的 `func`，guest 自己 abort 時它在 signal 上的 listener 不會跑（同步的 import 不能呼叫回同一個 instance，host 回報錯誤）。照 RFC 0001 的第 17 條，它要是 `async func`；做法是宣告表的成員可以是 `async method`（函式是 `async fn`，`Member::call_async`），WIT 的 `async func` 從它來，dom_wasm 照表註冊 async 的 import（`call_back`），不再另外列一份名單（原本的 `CALLS_BACK`，只有 `dispatchEvent`、寫死在 dom_wasm）；JS 照樣同步呼叫，當場跑完。`kun:dom` 不升版：`abort` 是 0.6.0 才有的，0.6.0 沒有發佈過。不選「不改，記成限制」：自己 abort、自己的 listener 收到是 `AbortController` 最基本的用法。插入沒有 `src` 的 iframe 時同步分派的 `load` 現在碰不到這個問題：wasm 文件的 iframe 沒有子 frame（engine 在 script 執行中建不了 realm）；之後 wasm 文件有子 frame 時，插入節點的成員也要是 `async method`（記在 `docs/limitations.md` 的那一條）。

18. **呼叫 callback 時，例外一律交還呼叫者，由呼叫者決定回報還是往外丟**（2026-10-07，第 4 階段開始前）：`NodeFilter` 的 `acceptNode` 丟出的例外要原樣丟給呼叫 `nextNode()` 等方法的 script，但 `ScriptHost::call` 原本一律回報例外（HTML 的 "report an exception"），不往外傳。改成 `call` 交還例外（script 的值），listener、timer、animation frame、microtask、`MutationObserver` 的呼叫點自己回報，traversal 的呼叫點變成 `ApiError::Thrown` 往外丟。照 Chromium 的分層：Blink 產生的 callback 程式（`V8NodeFilter::acceptNode` 等）回傳 `v8::Maybe`，呼叫者用 `ExceptionState` 往外丟，或用 `TryCatch` 回報（`EventTarget::FireEventListeners`）。wasm 的 guest 不丟例外，trap 照舊由 host 回報。不選「`ScriptHost` 多一個往外丟的方法，原本的 `call` 照舊回報」：改動少，但同一件事有兩條路，每加一種 callback 都要選一條，回報的位置也藏在 engine 裡。

19. **第 4 階段做完就結束這份 RFC，`Range` 和 `StaticRange`（第 5 階段）另開 RFC**（2026-10-07，4a 之後）：`Range` 是五組裡最大的，自己有一整組要決定的事（每個修改原語都要調整 live range，比 `NodeIterator` 的一個 hook 廣得多；`.style` 擋住的測試；`StaticRange`），分階段時就寫了「開始前決定要不要拆出去」。4a 的「document 弱指著物件、移除時調整」是它的基礎，拆出去不浪費。不選「第 4 階段也不做、現在就結束」：4a 已經寫好，留著是沒有 API 用到的程式，拿掉則第 18 條跟著不做；第 4 階段剩下的小，`dom/traversal` 的收益清楚。

## 待決定

沒有。`Range` 的待決定事項跟著它移到「之後的可能」（第 19 條）。

## 之後的可能

- **`Range` 和 `StaticRange`**（原本的第 5 階段，第 19 條）：另開 RFC。上面「5. `Range`」的範圍和「WPT 的調查」的數字是起點；要決定的有：每個修改原語（插入、移除、`replaceData`、`splitText`、`normalize`）怎麼調整 live range（document 弱指著它的 range，沿用 4a 的 `NodeIterator` 的做法），`.style` 擋住的 5 個大測試要不要先放一個最小的 `style`，`StaticRange`，和 `MutationObserver` 剩下的 subtest。
- **XML 的 parser 和序列化**：`DOMParser` 的 XML 類型、`XMLSerializer`、XML document 的 `innerHTML`，解開 `domparsing/` 剩下的測試和 `dom/nodes` 裡要 XML 的測試。
- **`Selection`**：要編輯和 layout 一起做。
- **shadow DOM**：`getHTML` 的選項、declarative shadow DOM、`slotchange`（共用 `MutationObserver` 的 microtask）、`getComposedRanges`。
- **每份 document 的延遲 task runner**（Blink 的 `ExecutionContext::GetTaskRunner`）：有第二個要「延遲執行 DOM 自己的工作」的功能時（例如 `fetch` 的逾時、`postMessage`）一起設計，把 `AbortSignal.timeout()` 從 timer 清單搬過去（第 13 條）。
- **`AbortSignal.any()` 照 Blink 的 GC**：有 `fetch` 之後，來源和 dependent 改成雙向 weak，加上撐住「有 listener、還可能 abort」的 signal 的條件式 root（第 15 條）。
- **custom element 的 reaction**：和 `MutationObserver` 一樣接在修改操作上，到時候 RFC 0009 的 iframe insertion steps 的時機（「和 Chromium 不同的地方」）也要改成當下做。
