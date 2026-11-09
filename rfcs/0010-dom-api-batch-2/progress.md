# RFC 0010 的進度：DOM API 的第二批

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-06。完成：2026-10-07，macOS arm64（Rust 1.99.0）；第 5 階段 `Range` 移到之後的 RFC（README 的「已決定的事項」第 19 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a parser 的依賴方向、fragment parsing、html5lib 的 fragment 測試 | 完成 | 2026-10-06 |
| 1b 序列化 | 完成 | 2026-10-06 |
| 1c-1 `innerHTML`、`outerHTML`、`insertAdjacent*`、`getHTML()`、`setHTMLUnsafe` | 完成 | 2026-10-06 |
| 1c-2 宣告表的 `static` 和 `enum`、`Document.parseHTMLUnsafe`、`DOMParser` | 完成 | 2026-10-06 |
| 1d 匯入 WPT、baseline、mutation test、文件 | 完成 | 2026-10-06 |
| 2a 降低新增 GC 型別的成本：`ScriptObject` trait、slot 的 `TaggedEdge`、前端的 `FetchObject` / `VisitObject`、泛型的 GC 測試 | 完成 | 2026-10-06 |
| 2b microtask（`ScriptHost::queue_microtask`、`queueMicrotask`、wasm 的 checkpoint）；guest 的 export 換成 `handle-event`、`run`、`frame` | 完成 | 2026-10-06 |
| 2c-1 `dom` 這一層：weak 的集合、`MutationObserver`、registration、`MutationRecord`、在原語裡記、批次、transient registration、待通知的 observer（第 9、12 條） | 完成 | 2026-10-07 |
| 2c-2 `dom_api` 和 JS：`observe`（選項的檢查）、`disconnect`、`takeRecords`、在 microtask 裡通知（`Microtask::NotifyMutationObservers`）、三種演算法開批次、parser 之後排通知、sequence 參數（`attributeFilter`） | 完成 | 2026-10-07 |
| 2d-1 wasm：`MutationObserver` 和 `MutationRecord` 進 `kun:dom`（0.5.0）、`mutations` export、C 和 Rust 的 guest；W6、W7 各加一格 observer 的檢查 | 完成 | 2026-10-07 |
| 2d-2 Lua 的產生器和 runtime 接 `mutations`、W9 加一格；`about:demo` 加 JS 的示範、`about:wasm-demo` 由 Lua 的 script 觀察點擊次數，和情境（C、Rust 只在 reftest，2026-10-07 決定） | 完成 | 2026-10-07 |
| 2e 第 2 階段的驗收：WPT（`dom/nodes` 的 `MutationObserver-*` 本來就在跑，baseline 在 2c-2 更新了）、mutation test、文件 | 完成 | 2026-10-07 |
| 3a `AbortController`、`AbortSignal`、`abort()`、`reason`（第 14 條）、`onabort`、`throwIfAborted`（`ApiError` 多「丟出 script 的值」）；`addEventListener` 的 `signal` | 完成 | 2026-10-07 |
| 3b `AbortSignal.abort()`、`timeout()`（第 13 條）、`any()`（第 15 條）；GC 的測試、`matrix`、`fuzz` | 完成 | 2026-10-07 |
| 3c wasm（`kun:dom`）、Lua；`about:demo` 的示範和情境（C 的 guest 測 engine、W9 加一格，W6、W7 不加；示範只有 JS，2026-10-07 決定）；`async method`（第 17 條） | 完成 | 2026-10-07 |
| 3d 匯入 WPT 的 `dom/abort`、baseline；第 3 階段的驗收 | 完成 | 2026-10-07 |
| 4a `dom` 這一層：`NodeIterator`、`TreeWalker` 兩個 GC 型別；document 弱指著它的 iterator（第 12 條的 weak 集合）；移除節點的原語裡跑 `NodeIterator` 的 pre-removing steps；GC 的測試、`matrix`、`fuzz` | 完成 | 2026-10-07 |
| 4b `dom_api` 和 JS：traversal 的演算法；呼叫 callback 時交還例外（第 18 條），filter 的例外往外丟、filter 執行中再進入是 `InvalidStateError`；`NodeFilter` 的介面物件和常數（帶常數的 callback interface）；JS 的測試；`about:demo` 的示範和情境；guest 的 `accept-node` export（宣告表一加 traversal，WIT 就跟著變，所以從 4c 移過來） | 完成 | 2026-10-07 |
| 4c wasm 和 Lua 的測試：C 的 guest 測 engine（`accept-node`、filter 失敗時的代碼 14）、W9 加一格（Lua 的 filter、錯誤丟給呼叫者）；W6、W7 不加（2026-10-07 決定） | 完成 | 2026-10-07 |
| 4d 匯入 WPT 的 `dom/traversal`、baseline；第 4 階段的驗收 | 完成 | 2026-10-07 |
| 第 5 階段 `Range` 和 `StaticRange` | 移到之後的 RFC（第 19 條，2026-10-07） | |

## 驗收

第 1 階段：2026-10-06 在 macOS arm64 上，fmt、clippy（沒有警告）、`cargo test --workspace` 901 個（`wpt_testharness` 另計：617 個都和預期相符、138 個不跑）、`sanity.py deps`、`unsafe`、`docs`、`test.py automation`（真的視窗，9 份情境都通過）。GC 的檢查（1c-2 的 `DomParser`）：`test.py matrix` 通過，`fuzz` 的兩組失敗和 `DomParser` 無關（見 1c-2 的「做過的檢查」），其中 `standalone_heap` 那一組同一天修正（「單獨的 heap 的 marking 步驟」），之後 `matrix` 和 `fuzz` 的 20 組都通過。

第 2 階段：2026-10-07 在 macOS arm64 上，fmt、clippy（沒有警告）、`cargo test --workspace` 945 個都通過（`wpt_testharness` 617 個都和預期相符、138 個不跑；reftest 193 個通過，3 個是預期中的失敗；沒有平台視窗的情境 9 份各兩個平台）、`sanity.py deps`、`unsafe`、`docs`、`ci.py guests`（全部重編）、`test.py automation`（真的視窗，9 份情境都通過，看過 `markup` 和 `wasm-demo-lua` 的截圖）。GC 的檢查：`test.py matrix` 8 組、`test.py fuzz` 預設的 20 組（seed `0xf9ef6dd9`）都通過。WPT 的 `dom/nodes/MutationObserver-*` 有 7 個檔案全部通過，其他的失敗都是還沒有的東西（2c-2）。期間查到並修好情境執行器的點擊競爭（RFC 0006，2d-2 的「不穩定的測試」），之後 `windowless` 不再失敗。

第 3 階段：2026-10-07 在 macOS arm64 上，fmt、clippy（沒有警告）、`cargo test --workspace` 967 個都通過（`wpt_testharness` 623 個都和預期相符、138 個不跑；reftest 193 個通過，3 個是預期中的失敗；沒有平台視窗的情境 9 份各兩個平台）、`sanity.py deps`、`unsafe`、`docs`、`ci.py guests`（3c，全部重編）、`test.py automation`（真的視窗，9 份情境都通過，看過 `markup` 的截圖）。GC 的檢查：`test.py matrix` 8 組、`test.py fuzz` 預設的 20 組（seed `0xe04862bf`）都通過。WPT 的 `dom/abort` 6 個檔案裡 4 個全部通過，另外 2 個只因為沒有 Window 的具名屬性失敗（3d）。期間找到並修好兩個 bug：guest 自己 `abort()` 時它的 `abort` listener 不跑（第 17 條）、還有 `timeout()` 的 timer 時關掉頁面會 abort process（3c）。

第 4 階段：2026-10-07 在 macOS arm64 上，fmt、clippy（沒有警告）、`cargo test --workspace` 980 個都通過（`wpt_testharness` 641 個都和預期相符、138 個不跑；reftest 193 個通過，3 個是預期中的失敗；沒有平台視窗的情境 9 份各兩個平台）、`sanity.py deps`、`unsafe`、`docs`、`ci.py guests`（4b、4c，全部重編）、`test.py automation`（真的視窗，9 份情境都通過，看過 `markup` 的截圖）。GC 的檢查：`test.py matrix` 8 組、`test.py fuzz` 預設的 20 組（seed `0xbccb33b5`）都通過。WPT 的 `dom/traversal` 18 個檔案裡 13 個全部通過，3 個的 subtest 全部通過、只卡在最後一行的 `.style`，2 個卡在 Window 的具名屬性和 `template.content`（4d）；`Document-createTreeWalker` 也全部通過（4b）。期間找到並修好：pre-removing steps 的第一步（我記錯了規範）、規範新的「正在 filter 的位置」、filter 用自己的 iterator 時的 panic、realm 不能跑 script 的 filter 照樣被呼叫（4d）。

**parser 和 HTML 的片段（第 1 階段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `html` 只依賴 `dom`：parser 設的每個屬性（新元素的、第二個 `<body>` 補上的）在設好之後經過呼叫者的 `ElementHooks`，帶著行號和 parser 建的 document（1a） | `cargo test -p html --test parser`（`runs_the_element_hooks_for_every_attribute_it_sets`） |
| `dom_api::ParserHooks`：`onclick`、`onload` 變成還沒編譯的 handler，`<body onload>` 是 window 的（從 `html` 的測試搬過來）（1a） | `cargo test -p dom_api --test events`（`parsed_event_handler_attributes_become_uncompiled_handlers`） |
| renderer 的上層 document、`srcdoc` 和 `src` 的子 document 都用 `ParserHooks` parse（`new_parser`）（1a） | `cargo test -p shell --test script --test windowless`；`python3 scripts/test.py scenarios iframe`（`iframe-loads` 的 `srcdoc` 文件的 `onload` 屬性） |
| fragment parsing：html5lib 的 `#document-fragment` 測試（含 SVG、MathML 的 context，script-on 和 script-off），也在每次配置後強制 GC 跑一次；html5ever 的三個已知失敗記在 `tests/wpt/tree-construction-expectations.txt`（1a） | `cargo test -p html --test tree_construction` |
| fragment 建在自己的新 document（quirks mode 照 context 的 document）、不改 context；context 是或在 `<form>` 裡時 form 的指標；`<noscript>`（context 和 fragment 裡的）照 scripting flag；element hooks 拿到 fragment 的 document（1a） | `cargo test -p html --test parse`（`a_fragment_*`、`noscript_follows_the_scripting_flag`） |
| 序列化（`html::serialize_children`、`serialize_node`）：元素、文字、註解；文字和屬性值的跳脫（屬性裡的 `<`、`>` 也跳脫，`'` 不跳脫）；HTML 的 raw text 元素（`<noscript>` 照 scripting flag），SVG 的 `<style>` 不算（自己或在 HTML 元素裡）；`<template>` 寫它的內容；void 元素沒有結束標籤、什麼都不寫（script 給它的子節點也不寫，`innerHTML` 是空字串，`outerHTML` 是開始標籤），別的 namespace 的同名元素不是 void；HTML、SVG、MathML 的元素用 local name，其他用 qualified name；屬性照 namespace（`xml:`、`xmlns`、`xlink:`，其他用 qualified name）；doctype、processing instruction、CDATA（當成文字）（1b） | `cargo test -p html --test serialize` |
| `innerHTML`：取代全部子節點、parse 出的節點屬於元素的 document、`null` 是空字串、`<template>` 的內容、元素是 context（`<tr>` 裡的 `<td>`）、`onclick` 是還沒編譯的 handler；`<noscript>` 照 document 有沒有 window（parse、`innerHTML`、`outerHTML`、`getHTML()`）（1c-1） | `cargo test -p dom_api --test markup` |
| XML document 的 `innerHTML`、`outerHTML`（讀寫）、`insertAdjacentHTML` 是 `NotSupportedError`，`getHTML()`、`setHTMLUnsafe` 照 HTML（1c-1） | `cargo test -p dom_api --test markup`（`an_xml_documents_markup_is_not_supported`） |
| `outerHTML`（parent 是 fragment 時用 `<body>` 當 context、沒有 parent 什麼都不做、parent 是 document 時 `NoModificationAllowedError`）、`insertAdjacentHTML`（四個位置、不分大小寫、`SyntaxError`、沒有 parent 時 `NoModificationAllowedError`、元素自己是 context、`<html>` 和 fragment 用 `<body>`、插在下一個兄弟之前）、`insertAdjacentElement`、`insertAdjacentText`、`getHTML()`、`setHTMLUnsafe`（1c-1） | `cargo test -p dom_api --test markup` |
| `innerHTML` 插入的 iframe 有子 frame、照 parse 出的 `srcdoc` 導覽，換掉時丟棄；沒有 browsing context 的 document（沒有 window、已經丟棄）不記 iframe 的插入、移出和屬性改變（1c-1） | `cargo test -p dom_api --test frames`（`markup_inserts_and_removes_iframes`）；`cargo test -p dom --test tree`（`iframes_outside_a_browsing_context_are_not_recorded`） |
| JS：`null` 和 `undefined`、`getHTML()` 不給參數、parse 出的 `<script>` 不執行、`onclick` 在頁面的 realm 編譯、`DOMException` 的名稱和 legacy code（`NoModificationAllowedError` 7、`SyntaxError` 12）（1c-1） | `cargo test -p dom_js --test bindings`（`markup`） |
| `about:demo` 的 `innerHTML` 按鈕：markup 建出的文字和讀回來的 markup（1c-1） | `python3 scripts/test.py scenarios demo-script` |
| `Document.parseHTMLUnsafe()`：新的 HTML document、`about:blank`、沒有 window（`<noscript>` 是 markup）、realm 是 interface object 的、屬性的步驟有跑（1c-2） | `cargo test -p dom_api --test markup`（`parse_html_unsafe_is_a_static_operation`）；`cargo test -p dom_js --test realms` |
| `DOMParser`：建構子要有 realm（沒有是 `InvalidStateError`）；`text/html` 的 document 有 parser 的 window 的 document 的網址和 origin、realm 是那個 window、沒有 window、scripting 關閉；XML 類型是 `NotSupportedError`、別的字串是 `TypeError`；wrapper 和 expando 經過 GC 還在；跨 realm 呼叫時照 parser 自己的 realm（1c-2） | `cargo test -p dom_api --test markup`（`dom_parser_parses_html`）；`cargo test -p dom_js --test bindings`、`--test realms` |
| WPT：`domparsing/`（`tentative/` 不跑）、`html/syntax/` 的 fragment parsing 和序列化、`dynamic-markup-insertion` 的 `innerHTML`、`outerHTML`、HTML-unsafe-methods、`the-template-element`；MathML 的 `annotation-xml` context 照 `encoding` 算 integration point（1d） | `python3 scripts/test.py wpt`；`cargo test -p html --test parse`（`an_annotation_xml_context_is_an_integration_point_by_its_encoding`） |

**宣告表、前端和 `kun:dom`**

| 項目 | 怎麼驗證 |
| --- | --- |
| 宣告表的 `static`：呼叫入口的 `this` 一定是 window（其他的是 `TypeError`）；JS 裝在 interface object 上（衍生的 interface object 繼承、prototype 和物件上沒有、`length`、可列舉），`this` 是 interface object 的 realm；WIT 是宣告它的 resource 的 `static func`、沒有 `self`；wasm 的 `this` 是 store 的 window；和規範比對時 static 和一般的 operation 是不同的形狀（1c-2） | `cargo test -p dom_api --test markup`（`parse_html_unsafe_is_a_static_operation`）、`--test wit`、`--test spec`（`each_kind_of_difference_is_found`）；`cargo test -p dom_js --test bindings`（`static_operations_enumerations_and_dom_parser`）、`--test realms`（`documents_without_a_window_take_their_makers_realm`）；`cargo test -p dom_wasm --test engine`（`a_guest_parses_documents`） |
| 宣告表的 `enum`：只收它的值（不轉大小寫、不是字串的值也不收）；JS 先 `ToString`；WIT 的 `enum`，註解列出每個 case 的值；wasm 和 Lua 的 guest 傳 case（Lua 傳值的字串，不是值的字串是錯誤）（1c-2） | `cargo test -p dom_api --test markup`（`enumerations_take_only_their_values`）、`--test wit`；`cargo test -p dom_js --test bindings`（`static_operations_enumerations_and_dom_parser`）；`cargo test -p dom_wasm --test engine`（`a_guest_parses_documents`）；`python3 scripts/test.py reftest w9` |
| 和規範的 WebIDL 比對時，名稱開頭的 `_` 是跳脫（`_any` 是 `any`）（3b） | `cargo test -p dom_api --test spec` |
| 宣告表的 `async method`：WIT 的 `async func` 正好是它們（每個 resource 的 `dispatch-event`、`abort-controller.abort`）；`kun:dom` 0.6.0 的 `abort` 是 `async func`（3c） | `cargo test -p dom_api --test wit` |
| 呼叫 callback 時例外交還呼叫者（第 18 條）：`V8Host::call` 交還例外的值；listener、timer、animation frame、microtask、`MutationObserver` 的例外照樣在 callback 的 realm 回報（`call_reporting`）；callback interface 的物件照呼叫者給的種類呼叫 `handleEvent` 或 `acceptNode`（4b） | `cargo test -p dom_js --test bindings`（`host_calls_listeners`）；`cargo test -p dom_js` |
| JS 的 `NodeFilter`：每個 realm 一個沒有 `prototype` 的 function，呼叫時 `TypeError`，常數唯讀、可列舉、不能刪除，不可列舉地掛在 global 上（4b） | `cargo test -p dom_js --test traversal`（`node_filter_is_an_object_of_constants`） |
| `kun:dom` 0.5.0：`MutationObserver`、`MutationRecord`、`MutationObserverInit`（`attribute-filter: option<list<string>>`）在 WIT 裡；guest 多一個 export `mutations(id, records: list<mutation-record>, observer: borrow<mutation-observer>)`（2d-1） | `cargo test -p dom_api --test wit` |
| `kun:dom` 0.6.0：`abort-controller`、`abort-signal`（`reason` 不在，`abort()` 沒有參數）、`add-event-listener-options` 的 `signal`；optional 的 `any` 參數只從函式拿掉（3a） | `cargo test -p dom_api --test wit` |
| `kun:dom` 0.7.0：`node-iterator`、`tree-walker`（會呼叫 filter 的是 `async func`）、`document` 的 `create-node-iterator`、`create-tree-walker`；guest 多一個 export `accept-node(id, node) -> option<u16>`（`none` 是 filter 失敗了，那次呼叫以代碼 14 `CallbackError` 失敗）；所有 guest 實作它（4b） | `cargo test -p dom_api --test wit`；`python3 scripts/ci.py guests` |

**GC 和新的 GC 型別**

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom` 的 `DomParser`：記得建立它的 window；只有 slot 拿著時撐住它和那個 window 的 document，經過 GC 還在；release 之後 slot 是空的；另一個 isolate 的 handle 被拒絕；GC 的工作負載用到它（1c-2） | `cargo test -p dom --test tree`（`a_dom_parser_keeps_its_realm_window`）、`--test cross_isolate`；`cargo test -p dom`（`gc_stress` 等）；`python3 scripts/test.py matrix`、`fuzz`（macOS） |
| 單獨的 heap：GC 開始 incremental marking、第一步標不完、之後也不再配置時，只靠 event loop 的 turn 收完（包括延遲的 sweep）；heap 還在 marking 時，`run_platform_tasks` 跑完一批就排下一步（1c-2 的 fuzz 找到的問題） | `cargo test -p dom --test standalone_heap`（`finishes_a_collection_without_more_allocation`）；`python3 scripts/test.py matrix`、`fuzz`（macOS） |
| 每種 script 拿得到的 GC 型別（`ObjectTag` 的十種）經過同一組檢查：只有 slot 撐住時經過 GC 還在、只當自己的型別讀回（`held_tag`、`held`）、release 之後被回收、別的 DOM 的 slot 拒絕它而且沒拿走格子；`ObjectTag` 是清單的索引（2a） | `cargo test -p dom --test script_objects` |
| 每種型別的 wrapper 只 unwrap 成自己（各用自己的 tag）；沒綁到 global 的 window 不從 template 建 wrapper（2a） | `cargo test -p dom --test script -- every_type_unwraps_only_as_itself an_unbound_window_gets_no_made_wrapper` |
| 讀取的 scope 拒絕別的 DOM 的事件和集合（`event_state`、`event_target`、`collection_root`）（2a） | `cargo test -p dom --test cross_isolate` |
| `MutationObserver`、`MutationRecord` 經過每個型別共同的檢查（slot、wrapper 的 tag）；GC 的工作負載有一個觀察 `<body>` 的 observer（2c-1） | `cargo test -p dom --test script_objects --test script --test gc_stress --test incremental_stress --test standalone_heap --test detached_marking` |
| iterator 和 walker 撐住 root、位置和 filter，document 不撐住 iterator；GC 的工作負載有 iterator 和 walker，`gc_stress` 有 iterator（4a） | `cargo test -p dom --test traversal --test gc_stress --test incremental_stress --test standalone_heap --test detached_marking`；`python3 scripts/test.py matrix`、`fuzz` |

**microtask（2b）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `queueMicrotask`（JS）：callback 在 checkpoint 才跑、和 promise 的 job 照排進去的順序、途中排的在同一次跑完；例外分派 `error`、之後的照樣跑；不是 function 是 `TypeError`（2b） | `cargo test -p dom_js --test errors` |
| DOM 的 microtask 佇列：照編號取一次、或從最舊的取；拆掉 DOM 時清掉（握著 root 的也是）；外部 engine 的 callback 記著種類，相等要種類也相同，listener 存著它（2b） | `cargo test -p dom --test foreign` |
| wasm：`queueMicrotask` 經過 `run` export、在 checkpoint 才跑、在 guest 的 Store 的 frame 裡；engine 自己的 task 之後也做 checkpoint；W7 的 timer 經過 `run`、排的 microtask 在 animation frame（`frame`）之前跑（2b） | `cargo test -p dom_wasm --test engine` |
| W6（Rust）、W7（C）、W9（Lua）用新的 export；W6、W7 多一格 microtask 的檢查（2b） | `python3 scripts/test.py reftest w6 w7 w9` |
| 計時器的 callback 結束後才跑 microtask：microtask 裡設的計時器不繼承巢狀層數（WPT 的 `timer-nesting-not-inherited-in-microtask` 從 NOTRUN 變成通過）（2b） | `python3 scripts/test.py wpt` |

**`MutationObserver`（第 2 階段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom` 的修改原語記給 mutation observer：`childList`（前後的兄弟）、`subtree` 才記子孫、屬性（舊值只在要求時，任一個註冊要求就有；filter 只比 local name，有 namespace 的不符合；移除、`set_attr_node`）、文字（設定、parser 的附加）；沒要求的不記；同一個 observer 經過兩個註冊只有一筆；批次記成一筆；拿掉的節點保持被觀察直到通知或重新 observe；`disconnect`；待通知的順序、microtask 只要一次；被觀察的節點撐住 observer、observer 不撐住節點、紀錄撐住節點、拆掉時放掉；GC stress 下也對（2c-1） | `cargo test -p dom --test mutation_observers` |
| `dom` 的批次只擋住插入 target 的紀錄，結束時照給的內容記一筆，之後的插入照記；批次裡的移除照記，不記的移除（`remove_without_record`）不記但照樣有 transient registration；照給的內容記一筆（`record_child_list`），什麼都沒變時不記（2c-2） | `cargo test -p dom --test mutation_observers`（`a_batch_records_one_change`、`a_removal_without_a_record_stays_observed`） |
| JS 的 `MutationObserver`：callback 在 microtask 裡跑（和 promise 的 job 照順序）、拿到全部紀錄、`this` 和第二個參數是 observer；照第一次拿到紀錄的順序通知，例外分派 `error`、下一個照樣跑；`takeRecords`、`disconnect`、重新 `observe`；`observe` 的選項和 `TypeError`（規範的步驟 1 到 6）、`attributeFilter` 從任何 iterable；setter 的變動也通知；拿掉的節點保持被觀察直到通知（2c-2） | `cargo test -p dom_js --test mutation_observers` |
| 插入 fragment（開頭、中間、結尾）、`replaceChild`（換成自己、前後的兄弟、fragment）、replace all（`replaceChildren`、`textContent`、`innerHTML`）各記一筆，兄弟照規範；fragment 的子節點移出是 fragment 的一筆；"adopt" 的移出自己一筆（`replaceChild` 先做，replace all 不做，whatwg/dom#814）（2c-2） | `cargo test -p dom_js --test mutation_observers`（`many_children_changed_at_once_are_one_record`、`replacing_with_a_child_of_the_same_parent`） |
| 插入和取代在動到任何節點之前檢查循環（host-including inclusive ancestor），`HierarchyRequestError`；WPT 的 `Node-insertBefore`、`Node-replaceChild` 因此全部通過（2c-2） | `cargo test -p dom_js --test mutation_observers`（`a_cycle_throws_before_anything_moves`）；`python3 scripts/test.py wpt` |
| parser 停在 `</script>` 時先做 microtask checkpoint，observer 在 script 執行之前收到 parser 插入的紀錄；parser 跑完一段之後排通知（`MutationObserver-document` 的 parser 插入、parse 途中移除 parent）（2c-2） | `python3 scripts/test.py wpt` |
| WPT 的 `dom/nodes/MutationObserver-*`：`callback-arguments`、`callback-tricky`、`disconnect`、`inner-outer`、`overlapping-registrations`、`sanity`、`takeRecords` 全部通過，其他的失敗都是還沒有的東西（見 2c-2 的「做過的檢查」）（2c-2） | `python3 scripts/test.py wpt` |
| wasm 的 guest 的 mutation observer：callback 經過 `mutations` 在 checkpoint 跑（不在 script 裡）；紀錄照 `attributeFilter` 和舊值（guest 給的 `list<string>` 轉成 sequence）；借來的 observer 能 `takeRecords`；紀錄是 guest 的，留到之後的 microtask 還能讀，用完之後 store 拿著的格數和之前一樣（observer 的借用放掉了）；guest 的 `takeRecords`、`disconnect`（2d-1） | `cargo test -p dom_wasm --test engine`（`a_guests_mutation_observer`）；`cargo test -p dom --test foreign`（`slots` 的 `held_count`） |
| W6（Rust）、W7（C）各多一格：timer 建的 observer 收到 timer 自己附加的方塊（一筆 `childList`、加了一個節點），在 callback 裡 `disconnect`（2d-1） | `python3 scripts/test.py reftest w6 w7`；`cargo test -p dom_wasm --test engine`（`w7`） |
| Lua 的 mutation observer（W9 多兩格）：callback 收到紀錄的陣列和借來的 observer，用它 `disconnect`，callback 之後它不能用了；`attribute_filter` 是字串的陣列（放數字是錯誤），`attribute_old_value` 沒寫時是 `none`，紀錄留到 callback 之後還能讀（2d-2） | `python3 scripts/test.py reftest w9` |
| `about:demo` 的 `MutationObserver` 按鈕：一次點擊改了文字、屬性和子節點，observer 在 click handler 之後的 microtask 收到三筆，帶著舊值；`about:wasm-demo` 的 Lua script 觀察 C script 改的 `#count`，重新載入時重新開始（2d-2） | `python3 scripts/test.py scenarios demo-script wasm-demo`（截圖 `markup`、`wasm-demo-lua`） |

**`AbortController` 和 `AbortSignal`（第 3 階段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom` 的 `AbortController` 和 abort signal：controller 拿著它的 signal，signal 是 `EventTargetImpl` 的一種；abort 設定 reason（script 的值，或沒有 isolate 時 `DOMException` 的名稱和訊息）、執行演算法一次；reason 經過 GC 還在；signal 在 abort 之前撐住演算法的 target；`AbortController` 經過每個型別共同的檢查（3a） | `cargo test -p dom --test abort --test script_objects --test script` |
| JS 的 `AbortController`、`AbortSignal`：signal（同一個、不能建構）、`abort()` 預設的 `AbortError`（當下建、每次讀同一個）、trusted 的 `abort` 事件只一次、`onabort`、給的 reason 原樣保留、`throwIfAborted` 丟出 reason；`addEventListener` 的 `signal`（abort 時移除、已經 abort 的不加、`null` 和別的值是 `TypeError`）（3a） | `cargo test -p dom_js --test abort` |
| WPT 的 `AddEventListenerOptions-signal.any`、`event-listener-not-fully-active-target` 全部通過，`event-global-event-targets` 只剩 `FileReader` 和 `Performance`（3a） | `python3 scripts/test.py wpt` |
| `AbortSignal.any()`（`dom`）：dependent 記下來源（各一次、dependent 攤平成它的來源）；abort 來源時一起設定還沒 abort 的 dependent 的 reason，回傳的順序是來源、再照建立的順序；已經 abort 的立刻 abort、沒有來源；來源撐住 dependent；`AbortSignal.timeout()` 的 timer（到期、撐住 signal、`clearTimeout` 清不掉、不算巢狀、跟著 document 清掉）；GC 的工作負載有 controller、dependent 和只靠演算法撐住的元素（3b） | `cargo test -p dom --test abort --test timers --test incremental_stress --test standalone_heap --test detached_marking`；`python3 scripts/test.py matrix`、`fuzz` |
| JS 的 `AbortSignal.abort()`、`any()`（事件的順序：來源先、再照建立的順序，經過 dependent 也一樣）、`timeout()`（`[EnforceRange] unsigned long long`：負數、`NaN`、無限大、2^53 和非數字是 `TypeError`，在轉換參數時檢查；時間到時以 `TimeoutError` abort、`clearTimeout` 清不掉）（3b） | `cargo test -p dom_js --test abort` |
| wasm 的 guest 的 `AbortController`、`AbortSignal`（C）：自己 `abort()` 時，自己在 signal 上的 listener 和 `onabort` 在它回傳之前跑；帶著 signal 加的 listener 被移除；第二次 `abort()` 什麼都不做；`throw-if-aborted` 的代碼（`AbortError` 12、`TimeoutError` 13）；`AbortSignal.abort()`、`any()`；`timeout` 的 `u64` 照給的設定 timer，超過 2^53 − 1 是 `TypeError`（1）；guest 呼叫 `abort()` 時補上只有 JS 有的參數（3c） | `cargo test -p dom_wasm --test engine`（`a_guests_abort_controller`） |
| Lua 的 `AbortController`（W9 多一格）：`abort()` 等 listener 跑完（script 自己的也是）、帶著 signal 的 listener 被移除、`throw_if_aborted` 是 `AbortError`；`AbortSignal.any` 的物件陣列（放別的東西是錯誤）；`timeout` 的 `u64`（負數是 Lua 的錯誤，超過 2^53 − 1 是 `TypeError`）（3c） | `python3 scripts/test.py reftest w9` |
| `DomIsolate` 拆掉時，還沒到期的 `AbortSignal.timeout()` 的 timer 先放掉 signal（3c） | `cargo test -p dom --test timers`（`the_dom_goes_with_an_abort_timer_pending`） |
| `about:demo` 的 Start、Cancel：`AbortSignal.any` 合起 controller 的 signal 和 `AbortSignal.timeout(2000)`；Cancel 立刻以 `AbortError` abort，不按就由 renderer 執行的 timer 以 `TimeoutError` abort（3c） | `python3 scripts/test.py scenarios demo-script`（截圖 `markup`） |
| WPT 的 `dom/abort`（6 個檔案）：`AbortSignal.any`、`abort-signal-any`、`event`、`timeout` 全部通過；`reason-constructor`、`abort-signal-timeout` 只因為沒有 Window 的具名屬性（`iframe` 這個全域名稱）失敗，換成 `getElementById` 之後全部通過（3d） | `python3 scripts/test.py wpt` |

**traversal（第 4 階段）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom` 的 `NodeIterator`、`TreeWalker`：初始狀態（root、`whatToShow`、filter、reference 和 pointer before reference、current）、讀寫、「執行中」的旗標；兩個型別經過每個型別共同的檢查（4a） | `cargo test -p dom --test traversal --test script_objects --test script` |
| 移除節點時的 NodeIterator pre-removing steps：不是 reference 的祖先、是 root 的祖先時不變（4d 更正：原本照記錯的規範寫成「是 root」）；pointer before 時移到下一個在 root 裡的節點（不越過 root），沒有時退回；前一個兄弟的最後一個子孫或 parent；不記的移除也一樣（4a） | `cargo test -p dom --test traversal`（`removing_a_node_moves_the_references_off_it`） |
| iterator 跟著它的 root 到另一份 document（`adoptNode`、插入時的 adopt），在那裡照樣調整；插入時先從舊的 parent 移除再換 document，舊 document 的 iterator 看得到這次移除（4a） | `cargo test -p dom --test traversal`（`iterators_follow_their_root_to_another_document`） |
| `TreeWalker` 的七種走法（skip 的子節點照走、reject 的不走，往前、往後、從兄弟都一樣）、`currentNode`、`root`、`whatToShow`（預設 `SHOW_ALL`）、`filter`；`NodeIterator` 的往前、往後（pointer 在前或後時先回傳同一個節點）、`referenceNode`、`pointerBeforeReferenceNode`、移除時調整、`detach()`（4b） | `cargo test -p dom_js --test traversal` |
| filter：function（不管它的 `acceptNode`）、物件的 `acceptNode` 每次重新讀、以物件為 `this`、不是 function 是 `TypeError`；回傳值轉成 `unsigned short`；丟出的例外原樣從走訪丟出，之後還能再走；filter 執行中再用它的 walker 是 `InvalidStateError`（4b） | `cargo test -p dom_js --test traversal`（`the_filter_and_what_it_throws`） |
| `NodeIterator` 正在 filter 的位置（規範的 traverse）：移除時和 reference 一樣調整，filter 執行中 `referenceNode` 還是上一次接受的；filter 移除它時，接受就回傳原本的節點、reference 是移到的位置，不接受就從那裡繼續；filter 執行中再用它的 iterator 不碰外層的位置（4d） | `cargo test -p dom --test traversal`（`removing_a_node_moves_the_position_being_filtered`）；`cargo test -p dom_js --test traversal`（`a_filter_removing_its_node`、`the_filter_and_what_it_throws`） |
| realm 不能跑 script（document 不再 fully active，或 window 已經沒了）的 filter 不呼叫，走訪丟出 `TypeError`（Blink 丟 `Error`）（4d） | `python3 scripts/test.py wpt`（`TreeWalker-acceptNode-filter-cross-realm-null-browsing-context`） |
| `about:demo` 的 TreeWalker 按鈕：filter 只接受按鈕，列出頁面上 9 個按鈕（4b） | `python3 scripts/test.py scenarios demo-script`（截圖 `markup`） |
| wasm 的 guest 的 filter（C）：在 traversal 的 `async func` 裡經過 `accept-node` 執行，skip 的節點往子節點走；walker 的 `filter` 是 guest 自己的 id；filter 回傳 `none` 時走訪停下、那次呼叫以代碼 14（`CallbackError`）失敗，下一次照常走（4c） | `cargo test -p dom_wasm --test engine`（`a_guests_node_filter`） |
| Lua 的 filter（W9 多一格）：skip 的節點往子節點走、`walker:filter()` 是同一個函式、filter 的錯誤原樣從 `next_node` 丟出、之後照常走（4c） | `python3 scripts/test.py reftest w9` |
| WPT 的 `dom/traversal`（18 個檔案）：13 個全部通過；`NodeIterator`、`NodeIterator-removal`、`TreeWalker` 的 subtest 全部通過，只是最後一行用到 `.style`；`TreeWalker-acceptNode-filter-cross-realm` 卡在 Window 的具名屬性，`TreeWalker-nextNode-detached-currentNode` 的一個 subtest 卡在 `template.content`（4d） | `python3 scripts/test.py wpt` |


## 做過的檢查

2026-10-06 到 2026-10-07，macOS arm64（GC 的檢查用 debug 的 V8）。

- **WPT 的結果**：runner 從 476 個（1c-1 之前）到 641 個測試（138 個不跑），每個階段連跑兩次結果一樣。
  - 第 1 階段：已經匯入的目錄少了 143 個失敗的 subtest（1c-1），8 個測試全部通過；新匯入 141 個（`domparsing/` 等），34 個全部通過（1d）。剩下的失敗主要是 `HTMLTemplateElement`（約 250 個 subtest）、XML（約 160 個）、`<template for>`、`Range`、HTML 元素的 interface、`document.write`、`.style`。
  - 第 2 階段：`dom/nodes` 的 `MutationObserver-*` 七個全部通過，`Node-insertBefore`、`Node-replaceChild`（2c-2）和 `timer-nesting-not-inherited-in-microtask`（2b）也是；剩下的 `MutationObserver` 的失敗在等 `Range`、XML parser、`HTMLInputElement`、script 插入的 `<script>`、`onerror`。
  - 第 3 階段：`dom/abort` 6 個裡 4 個全部通過，2 個只差 Window 的具名屬性（換成 `getElementById` 跑過一次，全部通過）；`AddEventListenerOptions-signal`、`event-listener-not-fully-active-target` 全部通過（3a）。
  - 第 4 階段：`dom/traversal` 18 個裡 13 個全部通過，3 個只差最後一行的 `.style`，2 個卡在 Window 的具名屬性和 `template.content`；`Document-createTreeWalker` 全部通過，`moveBefore-nodeiterator` 和三個 `template-for` 往下走到還沒有的功能（4b）。
- **Mutation test**：共 282 個（1a 13、1b 13、1c-1 20、1c-2 23、1d 3、單獨的 heap 5、合成粗體 6、2a 17、2b 14、2c-1 24、2c-2 37、2d-1 6、2d-2 6、3a 20、3b 17、3c 11、4a 14、4b 18、4c 9、4d 6）。每次跑的期間使用者沒有 commit，跑完確認都還原了，之後跑 `ci.py prune`；改到 guest 或 Lua 的 runtime、產生器的，每個 mutation 都重編 guest 再跑，最後從還原的原始碼重編。
  - 一開始沒抓到、補了測試之後抓到的：1a 3 個（兩個 scripting flag、`src` 子 document 的 hooks）、1b 1 個（走不到的分支改成 `unreachable!`）、2c-1 1 個（任一個註冊要求舊值）、2c-2 4 個（批次結束之後的插入、fragment 插在中間和結尾、`adopt` 的那一筆……）、2d-1 1 個（observer 的借用不放掉：`dom` 加了 `held_count`）、3b 1 個（`[EnforceRange]` 的範圍：補了比對訊息）、4b 1 個（字串的回傳值：`'2'` 轉不成數字和 reject 一樣）。設計 mutation 時另外補的測試：1c-1 3 個、1c-2 2 個、2d-2 1 個、4a 1 個（root 後面的兄弟）、4b 1 個（reject 的子樹）、4d 2 個。
  - 等價、沒跑或拿掉多餘程式的：1c-2 的 static 入口呼叫 `update_frames`（拿掉）；2b 的 V8 跳板照編號取（等價）；3a、3b 留到 3c 的 wasm 和 Lua 的部分（3c 跑了）；4a 的 `move_node_iterators` 跳過同一份 document、`WeakSet::remove` 不做事；4b 的 `NodeIterator` 往前往後的提早結束；4c 的 `store::call` 裡 filter 回傳 `none` 的分支（走不到）；4d 的 traverse 之後不清掉正在 filter 的位置（script 看不到）。
- **找到的 bug**：
  - **iframe 經過 `innerHTML` 多建子 frame**（1c-1）：`dom` 照「根是 document」記 iframe 的插入；改成只在根是有 browsing context 的 document 時記（RFC 0009 的第 25 條跟著改）。
  - **html5ever 寫出 void 元素底下的文字**（1b）、**`annotation-xml` 的 context 不是 integration point**（1d）。
  - **單獨的 heap 每一輪 GC 只排第一步 marking**（1c-2 的 fuzz）：cppgc 的 task 不清掉自己的 handle；kun 自己排之後的步驟（第 7 條）。給上游的回報草稿是 `cppgc-marking-task-report.md`。
  - **沒有平台視窗時粗體不合成**（1c-1 的截圖）：測試字型是 500，fontique 只在差 200 以上時合成；`layout` 照 Blink 的 fallback 字型的規則補上。
  - **計時器裡的 microtask 繼承巢狀層數**（2b）、**插入和取代少了循環的檢查**（2c-2，開了批次之後才看得出來）。
  - **情境的執行器送完點擊不等 renderer 處理完**（2d-2）：`a_click_follows_an_element_a_change_just_moved` 從 1a 起時常失敗；照 Chromium 的 `in_flight_event_count_` 等 input 的 ack，記在 RFC 0006。之後不再失敗。
  - **guest 自己 `abort()` 時它的 `abort` listener 不跑**（3c）：同步的 import 不能呼叫回 guest；改成 `async method`（第 17 條）。
  - **頁面關掉時還有 `AbortSignal.timeout()` 的 timer 會 abort process**（3c）：`DomIsolate` 拆掉時不清 timer。
  - **NodeIterator 的 pre-removing steps 的第一步**（4d，4a 寫錯）：我記錯了規範（「是 root」而不是「是 root 的祖先」）；**規範新的「正在 filter 的位置」**（4d）；**filter 用自己的 iterator 時 panic**（4d，這個 panic 讓兩次重新產生 baseline 的 runner 中途 abort，當時只看了檔案清單、沒發現）；**realm 不能跑 script 的 filter 照樣被呼叫**（4d）。
- **不穩定的測試**：`shell` 的 `windowless` 的 `a_click_follows_an_element_a_change_just_moved`（1a 到 2d-2）：查到是情境的執行器沒等 input 的 ack（上面），修正之後不再失敗。另外一次 `wpt_testharness` 的 baseline 不符是 4d 的 panic，不是不穩定。
- **GC 的檢查**（`test.py matrix` 8 組、`test.py fuzz` 預設的 20 組，都通過）：1c-2（`fuzz` 那次跑了 146 組，兩組失敗：一組是 V8 在載入 snapshot 途中被隨機 GC 打斷，和 kun 無關；一組是上面的單獨 heap，修正後通過）、單獨的 heap 的修正、2a、2c-1（第一次走到 weak 的清除）、3b、第 3 階段、4a、第 4 階段的驗收。2b 只跑了 `matrix`。
- **真的視窗**（`test.py automation`）：每個階段的驗收時 9 份情境都通過，看過 `markup` 等截圖；第 3、4 階段第一次跑時螢幕鎖著，5 份畫不出 frame，解鎖之後重跑通過。
- **`kun:dom`**：0.3.0（開始時）→ 0.4.0（2b，export 換成每種 callback 一個）→ 0.5.0（2d-1，`mutations`）→ 0.6.0（3a，`abort-signal` 等；3c 的 `async func` 在 0.6.0 還沒有 commit 時改）→ 0.7.0（4b 的 `accept-node`：照 0.6.0 編的 guest 沒有這個 export。4b 時以為 0.6.0 還沒有 commit，沒有升版；收尾、寫 commit message 時才發現第 3 階段已經 commit 了，補升）。每次改了 WIT 都跑 `ci.py guests` 重編所有 guest。升到 0.7.0 之後（2026-10-07）重編所有 guest，fmt、clippy、`cargo test --workspace` 980 個、`docs` 都通過。
