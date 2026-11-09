# RFC 0008 的進度：DOM API 擴充和 WPT 的 testharness 測試

開始日期：2026-10-05。完成：2026-10-06，Windows 11（Rust 1.99.0）。動到 GC 的步驟（0b、1、2、3、4、5）用 debug 的 V8 跑的 `test.py matrix` 和 `fuzz` 在 macOS 上（Windows 只有 release 的 V8 prebuilt）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0a `kun:dom` 退回 0.1.0：WIT、golden file、`dom_wasm`、Rust SDK、重新編 repo 裡的 guest | 完成 | 2026-10-05 |
| 0b harness 的前置 API：`window.parent`、`getElementsByTagName` 和 `HTMLCollection`、`CharacterData.data`、HTML 元素的 interface、`any`、`ErrorEvent` 和 `PromiseRejectionEvent`、window 的 `error` 和 `unhandledrejection` | 完成 | 2026-10-05 |
| 0c runner：HTTP server 的目錄模式、`.any.js` / `.window.js` 的包裝、kun 的 `testharnessreport.js`、`testRunner`、`-expected.txt` 的比對和重新產生 | 完成 | 2026-10-05 |
| 0d 匯入 `dom/collections`、`dom/lists`、`html/webappapis/timers`，實測時間，決定跑在哪裡 | 完成 | 2026-10-05 |
| 1 集合、`ParentNode`、`ChildNode`、`Element`；匯入 `dom/nodes` | 完成 | 2026-10-05 |
| 0e WPT 改讀 `../third_party/wpt` 的上游 checkout（第 1 步之後做，第 28 條） | 完成 | 2026-10-05 |
| 2 事件補齊；匯入 `dom/events` | 完成 | 2026-10-06 |
| 3 `Node` 和 `CharacterData` | 完成 | 2026-10-06 |
| 4 namespace 和 `Attr` | 完成 | 2026-10-06 |
| 5 document 的模型 | 完成 | 2026-10-06 |
| 6 RFC 做完：宣告表和規範的比對、runner 的排程和 `timeout_multiplier`、收尾 | 完成 | 2026-10-06 |

## 驗收

2026-10-06 在 Windows 11 上全部通過：fmt、clippy（沒有警告）、`cargo test --workspace` 829 個（`wpt_testharness` 另計：429 個都和預期相符、139 個不跑；reftest 的 `harness/chain-broken*` 照舊是預期中的失敗）、`sanity.py deps`、`unsafe`、`docs`。

**`kun:dom` 和 wasm 的前端**

| 項目 | 怎麼驗證 |
| --- | --- |
| WIT 是 `kun:dom@0.3.0`（0a 退回 0.1.0，2a、4c 各升一次）；Lua 的 binding 從同一份 WIT 產生；只給 JS 的成員（`any`）不在 WIT 裡 | `cargo test -p dom_api --test wit` |
| repo 裡的 guest 都照現在的版本重新編，照舊能跑；W9（Lua）經過新的 API：`list` 的結果、`createEvent`、guest 自己的 `EventTarget`、`window.event`、`get_attribute_names`、`attributes`、`Attr`、`class_list():remove(...)`、`implementation():create_html_document` | `cargo test -p dom_wasm`、`python3 scripts/test.py reftest`（wasm 的 reftest，W9）、`cargo test -p shell --test wasm_scripts` |
| 照舊版本編的 guest 在 compile 時失敗，錯誤寫出版本，而且不執行 | `cargo test -p dom_wasm`（`a_guest_of_another_version_fails_to_load`、`a_guest_of_another_version_doesnt_run`） |
| wasm 的集合、`node-list`、`children`、`list<node-or-string>`；Lua 的可變參數 | `cargo test -p dom_wasm`（`a_guest_uses_a_collection`、`a_guest_appends_nodes_and_strings`）；W9 的第 5 項 |

**runner 和 WPT 的 checkout**

| 項目 | 怎麼驗證 |
| --- | --- |
| `test_server` 以目錄為根提供檔案：路徑解碼、照副檔名的型別、route 優先、不是檔案或離開根目錄的是 404 | `cargo test -p net`（`a_directory_is_served`、`files_stay_under_the_root`） |
| 哪些檔案是測試、`.any.js` / `.window.js` 的包裝和 META、harness 的時間、`-expected.txt` 的位置、skip 清單、比對和寫回（CRLF、全部通過時沒有檔案）、include 清單（列的檔案不是測試時失敗）、沒有測試讀的 `-expected.txt`；`timeout_multiplier` 和預期會等到逾時的測試 | `cargo test -p shell --lib testharness` |
| reftest 的 include 清單和 chain | `cargo test -p shell --lib reftest` |
| `testRunner` 只在 web test 的 realm、不可列舉，結果交給 frame；結果經過 renderer 和 browser 回到 headless，不交出結果的頁面在 deadline 之後是錯誤 | `cargo test -p dom_js --test test_runner`；`cargo test -p shell --test script`（`testharness_results_come_back_or_time_out`） |
| 端到端：全部通過、失敗和訊息、promise 的 rejection、step 裡的例外、頁面的例外變成 harness 的錯誤、harness 逾時、`.any.js` 和 `.window.js` 的包裝 | `python3 scripts/test.py wpt`（`4ff8d6e:tests/testharness/` 的六個頁面） |
| 三個 runner 讀 `../third_party/wpt`；`ci.py third-party --clone` 抓 WPT 時不轉換換行 | `python3 scripts/test.py wpt`、`--test reftest -- wpt`、`cargo test -p html --test tree_construction`；`python3 scripts/ci.py third-party`（手動確認過） |
| WPT 的 `dom/collections`、`dom/lists`、`dom/nodes`、`dom/events`、`html/webappapis/timers` 都和預期結果相符 | `python3 scripts/test.py wpt` |

**宣告表**

| 項目 | 怎麼驗證 |
| --- | --- |
| 新的形式：`[IndexedGetter]`、`[NamedGetter]`、`[LegacyNullToEmptyString]`（只有 `null` 是空字串）、`Any`、`Sequence<T>`、常數、union 和可變參數、`[ValueIterable]`、`[PutForwards]`、`[Stringifier]`、`Optional<T>`（只有 `undefined` 是沒給）；`EventListener`（callback interface）；沒給的可選參數用預設值；有必要成員的 dictionary 參數不能省略 | `cargo test -p dom_api --test api`（`the_table_describes_each_interface`、`call_entries_convert_and_check`）；`cargo test -p dom_js --test bindings`（`variadic_unions`、`constants`、`dom_token_lists`、`character_data_and_constructors`）、`--test errors`、`--test dispatch` |
| JS 的 interceptor：`[i]`、名稱、屬性描述、own key、`@@iterator`；interface object 的 chain | `cargo test -p dom_js --test bindings`（`html_collections`、`node_lists`、`interface_objects_and_prototype_chains`） |
| 宣告表和規範的 WebIDL 比對：不同的地方都列在 `spec-differences.txt` 並寫明原因，缺少的和 golden file 相符；每一種差異都找得到；測試自己的 parser | `cargo test -p dom_api --test spec` |

**樹、節點和元素（第 0b、1、3 步）**

| 項目 | 怎麼驗證 |
| --- | --- |
| HTML 元素的 interface（HTML 定義的和 custom element 的名稱是 `HTMLElement`，其他是 `HTMLUnknownElement`，別的 namespace 是 `Element`）；`HTMLMetaElement`、`HTMLTitleElement.text` | `cargo test -p dom_api --test api`（`elements_get_their_html_interface`、`meta_and_title_elements`）、`cargo test -p dom_js --test bindings` |
| `HTMLCollection`（`getElementsByTagName` 的比對、live、快取只在 DOM 沒變時沿用）、`getElementsByClassName`、`children`（`[SameObject]`）、`childNodes`（live 的 `NodeList`）、static 的 `NodeList`；`EdgeChunks`；集合撐住根、沒人參照時連根一起回收、wasm 的 slot 拿得住它 | `cargo test -p dom --test collections`、`cargo test -p dom_api --test api`、`cargo test -p dom_js --test bindings` |
| `append`、`prepend`、`replaceChildren`、`before`、`after`、`replaceWith`、`remove`、`replaceChild`（含 document 的規則）；子元素和兄弟元素；`id`、`className`、`removeAttribute`（style flag 和 DOM tree version、handler 停用）、`hasAttribute(s)`、`matches`、`webkitMatchesSelector`、`closest` | `cargo test -p dom_api --test api`（`parent_node_insertion`、`child_node_insertion`、`replace_child`、`replace_child_order`、`element_children_and_siblings`、`matches_and_closest`）、`cargo test -p dom --test style_flags`、`cargo test -p dom_js --test dispatch` |
| `Node` 的成員和常數、`tagName`、`CharacterData` 的方法（UTF-16 的位置）、`splitText`、`wholeText`、`normalize()`、建構子、`ProcessingInstruction`、`cloneNode`、`isEqualNode`、`compareDocumentPosition` | `cargo test -p dom_api --test api`（`node_basics`、`character_data_methods`、`text_nodes`、`processing_instructions`、`clone_node`、`is_equal_node`、`compare_document_position`）、`cargo test -p dom_js --test bindings`（`node_basics`、`character_data_and_constructors`、`clone_compare_and_processing_instructions`） |
| `dom` 的 processing instruction 和 CDATA section；dump 印出它們 | `cargo test -p dom --test tree`（`processing_instructions`）；`cargo test -p html --test parse`（`dumps_processing_instructions`、`dumps_cdata_sections`） |

**namespace、`Attr`、`NamedNodeMap`、`DOMTokenList`（第 4 步）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 名稱的檢查（第 39 條）、`createElementNS`、帶 NS 的 attribute 方法、attribute 照規範比對（只有 HTML document 裡的 HTML 元素轉小寫）、`getElementsByTagNameNS`、namespace 查詢、`isEqualNode` 不比 prefix | `cargo test -p dom_api --test api`（`create_element_names`、`create_element_ns`、`namespaced_attributes`、`get_elements_by_tag_name_ns`、`namespace_lookups`、`is_equal_node_namespaces`）、`cargo test -p dom_js --test bindings`（`namespaces`） |
| `Attr`：第一次需要時建立、取代時保留位置、移除後自己保存值、不能插進樹；`NamedNodeMap`；只靠 `ownerElement` 或 slot 保住的物件 | `cargo test -p dom --test tree`（`attrs`、`a_slot_holds_a_named_node_map`）；`cargo test -p dom_api --test api`（`attr_nodes`、`set_and_remove_attribute_nodes`、`named_node_maps`、`attrs_as_nodes`）；`cargo test -p dom_js --test bindings`（`attrs_and_named_node_maps`）、`--test gc_stress` |
| `DOMTokenList`（`classList` 是 `[PutForwards=value]`，`toString`、iterable、先檢查全部 token） | `cargo test -p dom --test tree`（`class_lists`）；`cargo test -p dom_api --test api`（`class_list`）；`cargo test -p dom_js --test bindings`（`dom_token_lists`、`class_list`）、`--test gc_stress` |

**document 的模型（第 5 步）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 每個節點的 document；建立節點都帶 document；插進另一份 document 的樹、`adoptNode` 時 adopt（子孫、`Attr`、template 的內容到 template contents owner document）；`ownerDocument`；建構子用 realm 的 document | `cargo test -p dom --test tree`（`node_documents_and_adoption`）；`cargo test -p dom_api --test api`（`owner_documents`、`import_and_adopt_nodes`、`text_nodes`）；`cargo test -p dom_js --test bindings`（`owner_document`） |
| `DOMImplementation`（每次同一個）、`createHTMLDocument`、`createDocument`、`createDocumentType`、`new Document()`、`XMLDocument`、CDATA section、XML document 的大小寫規則、document 的 content type / 網址 / 編碼、`doctype`、`head`、`compatMode`、`baseURI`、複製 document、`importNode` | `cargo test -p dom --test tree`（`documents_and_their_implementation`）；`cargo test -p dom_api --test api`（`dom_implementation`、`xml_documents`、`document_members`、`clone_node`、`import_and_adopt_nodes`）；`cargo test -p dom_js --test bindings`（`documents`） |
| 沒有 browsing context 的 document：event handler content attribute 不編譯、`<a>` 不換頁，移回有 window 的 document 之後照常執行 | `cargo test -p dom_api --test events`（`links_are_followed_only_in_a_document_with_a_window`）；`cargo test -p dom_js --test bindings`（`no_script_in_a_document_without_a_window`） |
| adopt 到另一份 document 時清掉 computed style、flag 重新開始 | `cargo test -p dom --test computed_style`（`adopted_elements_start_over`） |
| renderer：導覽時設定 document 的網址、content type、編碼；點擊不送給最後一次 paint 之後被移到別的 document 的節點 | `cargo test -p shell --test script`（`a_document_has_its_url_type_and_encoding`、`a_click_skips_a_node_moved_to_another_document`） |

**事件和錯誤的回報（第 0b、2 步）**

| 項目 | 怎麼驗證 |
| --- | --- |
| HTML 的「report an exception」：script、listener、event handler 的編譯錯誤在 window 分派 `error`，`error` listener 自己的例外不再分派；「notify about rejected promises」：checkpoint 之後分派 `unhandledrejection`，listener 排的 microtask 在同一次 checkpoint 跑；`ErrorEvent`、`PromiseRejectionEvent` 的建構子和成員，值經過 GC 還在 | `cargo test -p dom_js --test errors`、`--test bindings` |
| `initEvent` 重設什麼、`createEvent` 的事件沒初始化；事件的路徑只在分派期間；`EventTargetImpl` 的 listener、被事件和 slot 撐住；window 的 current event、Store 記得它的 window | `cargo test -p dom --test events` |
| 舊的成員、常數、`createEvent`、`CustomEvent`、`new EventTarget()`、script 建立的事件的時間 | `cargo test -p dom_api --test events` |
| JS：常數的屬性、`CustomEvent.detail` 經過 GC、`composedPath()`、listener 物件（`handleEvent`）、`EventTarget` 的子類別、`window.event` | `cargo test -p dom_js --test bindings`、`--test dispatch`、`--test gc_stress` |

**GC**

| 項目 | 怎麼驗證 |
| --- | --- |
| GC stress 的 workload 用到每個新的 GC 型別和 edge：`Collection`、static `NodeList`、`children` / `childNodes`、`EventTargetImpl`、window 的 current event、`Attr`、`attributes`、`classList`、別的 document 和它的節點、`implementation`、template contents owner document | `cargo test -p dom`（`gc_stress`、`incremental_stress`、`standalone_heap`、`detached_marking`）；`python3 scripts/test.py matrix`、`fuzz`（macOS） |
| GC 規則由型別擋下（rare data 的 edge 不能標 `#[no_trace]`） | `cargo test -p dom --test compile_fail` |

## 做過的檢查

2026-10-05 到 2026-10-06，Windows 11；GC 的檢查在 macOS 上。

- **WPT 的結果**：五個目錄 562 個測試，跑 423 個（另外 runner 自己的 6 個），139 個不跑（testdriver、整個檔案都在等 iframe 或 shadow DOM、CSS 動畫、`.sub`、太慢的 `NodeList-static-length-getter-tampered-*`，原因寫在 `4ff8d6e:tests/wpt/testharness-skip.txt`）。215 個全部通過，208 個有 `-expected.txt`。每一步讓全部通過的測試數：第 3 步 34 個、第 4 步 23 個、第 5 步 38 個（包括最核心的 `ParentNode-append`、`Node-textContent`、`Node-compareDocumentPosition`，原本在 `dom/common.js` 的 setup 就停住）。每次更新 baseline 都連跑兩到三次確認結果一樣。

  | 步驟 | WPT 的測試數（含 runner 自己的） | `wpt_testharness` 的時間 |
  | --- | --- | --- |
  | 0d | 35 | 約 23 秒（當時沒有平行，見下面的 bug） |
  | 1 | 315（另外 72 個不跑） | 11 到 12 秒（8 個 worker） |
  | 2 到 5 | 429（另外 139 個不跑） | 約 21 到 29 秒（8 個 worker；十幾個 10 秒的逾時） |
  | 6 | 429（另外 139 個不跑） | 約 10.7 秒（預期逾時的各自一個 thread，第 52 條） |

- **剩下的失敗**（2026-10-06 數 baseline 裡的訊息；括號是測試檔數）：iframe 和另一個 window（約 490 個 subtest 讀不到 `contentDocument` / `contentWindow`）、shadow DOM（17）、`MutationObserver`（17）、CSSOM（`getBoundingClientRect`、`getComputedStyle`、`.style`，12）、`click()` / `focus()`（11）、`DOMParser`（9）、HTML 元素各自的 interface（8）、`innerHTML` / `outerHTML`（5）、`Range`（4）、`frames`（4）、`customElements`（3）、`insertAdjacent*`（3）、`AbortController`（2）、`TreeWalker`（1）；以及 `docs/limitations.md` 的已知限制（落單的 surrogate、集合的 named property 不能設定、named property 不在 own key 裡、`processing-instruction-attributes` 期望 parser 建立 processing instruction）。第 50 條的新 RFC 從這份清單和 `crates/dom_api/tests/spec-missing.txt` 排優先順序。
- **Mutation test**：共 421 個（0a 3、0b 26、0c 16、1 48、0e 11、2 39、3 79、4 87、5 81、6a 21、6b 10）。每次跑的期間使用者沒有 commit，跑完確認實作檔案都還原了。
  - 一開始沒抓到、補了測試之後抓到的：0b 1 個（通知 rejection 之後的 microtask checkpoint）、1 有 2 個（一個節點也經過 fragment、`replaceChild` 的 doctype 順序）、2 有 1 個（事件的目標換種類時沒清掉舊的 edge，補了 `target_of` 的不變式檢查）、3 有 11 個、4 有 3 個、5 有 15 個、6a 有 3 個（宣告表剛好沒有那種差異的比對分支）。多半是測試少了一種情況：例如 CDATA 在 XML document 裡的行為、`set_document_info` 的測試設的值和預設值一樣。
  - 等價、拿掉多餘程式的：1 的兩個（`remove()` 沒有 parent 時的檢查、`replaceChild` 移動 reference），0e 的一個（列的檔案不存在時先報錯），5 的兩個（複製時子節點的複本建在原本的 document，插進複本時就 adopt），以及 5e 發現沒有人用的 `NodeRef::is_xml_document_object`。
  - 編不過的：3 的一個（rare data 的 edge 標 `#[no_trace]`，`GcObject` 的 derive 不接受，compile-fail 測試守住）。
  - 沒抓到、靠審查或手動的（記在 `docs/testing.md`）：`4ff8d6e:crates/shell/tests/wpt_testharness.rs` 本身（讀清單、註冊 route、平行執行、排程、`MACHOPO_WPT_TIMEOUT_MULTIPLIER` 的接線）；`spec` 測試的 assert 裡「清單有過時的行」的條件。
- **找到的 bug**：
  - **runner 沒有平行**（1e）：`while let Some(case) = queue.lock().unwrap().pop_front()` 的 `MutexGuard` 活到迴圈本體結束，八個 worker 輪流跑。第一次跑整個 `dom/nodes` 要 16 分鐘才發現；0d 的「約 23 秒」也是一個一個跑的結果。
  - **interface object 沒接上父 interface**（2a）：`MouseEvent.AT_TARGET` 是 `undefined`，原本的測試只查 prototype 的 chain。
  - **轉換的兩個錯**（3c）：沒給的可選參數照型別轉換（`new Comment()` 的 data 變成 `"undefined"`）；`[LegacyNullToEmptyString]` 把 `undefined` 也當成 `null`。
  - **`normalize()` 用合併後的長度算下一個節點的位置**（3d）。
  - **`classList.toggle(t, null)` 的退步**（4c）：宣告表的 `Option<bool>` 把 `null` 當成沒給，加了 `Optional<T>`，同時解決 `toggleAttribute` 的已知限制。
  - **`PromiseRejectionEvent.length` 是 1**（6a，比對規範時找到）：有必要成員的 dictionary 參數被當成 optional。
  - **事件的時間**（2e）：script 建立的事件用 event loop 的時間，`Event-timestamp-safe-resolution` 的迴圈停不下來；改成建立當下的時間（第 30 條）。
  - **自己改呼叫點時的錯**（5a）：約 330 個 `create_*` 的呼叫用 script 改，一個 script 跑兩次、一次換掉了測試裡另一份 document 的用法，測試失敗時照 HEAD 改回。
- **不穩定的測試**：
  - `dom` 的 `collections` 偶爾以 V8 的「Must use --expose-gc」中止（0b）：V8 的旗標由第一個啟動 V8 的測試決定，改成每個測試都用同樣的旗標，之後連跑 8 次都通過。
  - `Node-baseURI` 的 baseline 每次不同（5b）：有了 `document.URL` 之後，失敗訊息裡有 runner 隨機的 port；加了 `baseURI` 之後全部通過。
  - `a_click_skips_a_node_moved_to_another_document`（5d）連跑 8 次都通過；它靠 renderer 在下一個 frame 之前處理完排隊的輸入（`docs/test-gaps.md` 的「能補、還沒補的測試空缺」）。
- **GC 的檢查**：0b、1、2、3、4、5 各在 macOS 上用 debug 的 V8（有 DCHECK）跑了 `test.py matrix` 和 `test.py fuzz --minutes 60`，都通過（2026-10-05、2026-10-06）。
- **手動確認**：`ci.py third-party` 的 `core.autocrlf` 檢查和 `--clone`（0e）；runner 對壞掉的清單和多餘的 baseline 的錯誤訊息（0e）；runner 的排程和 `MACHOPO_WPT_TIMEOUT_MULTIPLIER=3`（6b：那 12 個各 30 秒，結果都相符）。
- **工具**：wasi-sdk 34.0 和 wit-bindgen 0.62.0 在 Windows 上照 `ci.py guests` 編全部的 guest（C、Rust、Lua），不用改；Rust 的 guest 和 macOS 編的不逐 byte 相同（`docs/limitations.md` 的「平台和建置」）。
- **大小**：`kun:dom` 沒有繼承，每個 resource 帶著祖先的全部成員，0b 加了 4 個 HTML 元素之後 Lua 的 binding（`kun_dom.c`）的 Lua 函式從 211 個變成 384 個，`resources/kun-lua.wasm` 從 506 KB 變成 659 KB（2026-10-05 確認維持現狀）。
