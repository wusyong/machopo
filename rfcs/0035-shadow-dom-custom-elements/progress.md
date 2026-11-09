# RFC 0035 的進度：shadow DOM 和 custom element

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-12。完成：2026-10-12，Fedora 44（Linux x86_64；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0035` 上分四段做，每段做完、經過審核時 merge 回 `main`。動到 GC 而要跑的 `test.py matrix`、`fuzz` 在 merge 之後看 CI。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a `ShadowRoot` 節點和 GC、`attachShadow`、`shadowRoot`、`ShadowRoot` 的屬性、`DocumentFragment.getElementById`、adopt 帶著 shadow tree；JS 和 wasm | 完成，merge 回 `main` | 2026-10-12 |
| 1b shadow-including 的 connected 和根、插入和移除的記錄、`clonable` 的 clone | 完成，merge 回 `main` | 2026-10-12 |
| 1c 事件路徑、retarget、`composedPath()`、`relatedTarget`、`window.event` | 完成，merge 回 `main` | 2026-10-12 |
| 1d `activeElement` 和 focus 事件 | 完成，merge 回 `main` | 2026-10-12 |
| 1e WPT、文件、限制、Speedometer 的盤點 | 完成，merge 回 `main` | 2026-10-12 |
| 2a `CustomElementRegistry`、definition、`define`、`get`、`getName`、`whenDefined` | 完成，merge 回 `main` | 2026-10-12 |
| 2b `HTMLElement` 的建構：`new X()`、`createElement` 的同步建構（construction stack 移到 2d） | 完成，merge 回 `main` | 2026-10-12 |
| 2c reaction 的佇列、`[CEReactions]`、backup element queue、四個 callback | 完成，merge 回 `main` | 2026-10-12 |
| 2d upgrade、construction stack、換 wrapper 的 prototype、`failed`、`:defined` | 完成，merge 回 `main` | 2026-10-12 |
| 2e WPT、文件、限制、Speedometer 的盤點 | 完成，merge 回 `main` | 2026-10-12 |
| 3a `HTMLSlotElement`、named assignment、`assignedNodes()`、`assignedElements()`、`assignedSlot`、`Element.slot`；JS 和 wasm | 完成，merge 回 `main` | 2026-10-12 |
| 3b manual assignment（`assign()`） | 完成，merge 回 `main` | 2026-10-12 |
| 3c `slotchange` | 完成，merge 回 `main` | 2026-10-12 |
| 3d 經過 slot 的事件路徑 | 完成，merge 回 `main` | 2026-10-12 |
| 3e WPT、文件、限制和第 3 段的檢查 | 完成，merge 回 `main` | 2026-10-12 |
| 4a WIT 升 0.13、guest 的 export、所有 guest 的空的實作、`customElements` 給 wasm（`define` 收 record、`get`、`getName`、`whenDefined`）；Lua 的 generator 支援 record 裡的 callback、`list<string>` 和 callback 的結果 | 完成，merge 回 `main` | 2026-10-12 |
| 4b 建構和 reaction：`construct`、四種 callback 經過 backup element queue、`create-element` 排 upgrade、trap 時 failed | 完成，merge 回 `main` | 2026-10-12 |
| 4c Lua 的 API、C 和 Rust 的範例和測試 | 完成，merge 回 `main` | 2026-10-12 |
| 4d 文件、限制和第 4 段的檢查 | 完成，merge 回 `main` | 2026-10-12 |

## 驗收

### shadow tree 和事件

| 項目 | 怎麼驗證 |
| --- | --- |
| shadow root 是 host 的 node document 的 `DocumentFragment`，帶著建立時的設定；它的 `host` 和 host 的 rare data 互相指著；不是 host 的子節點、沒有 parent；template 的內容不是 shadow root；一個元素只有一個 | `cargo test -p dom --test it tree::a_shadow_root_is_a_fragment_its_host_points_at`、`tree::an_element_has_one_shadow_root` |
| host 和 shadow root 互相撐住（只靠 tree 撐住 host、只靠 shadow tree 裡的節點撐住整個） | `cargo test -p dom --test it tree::a_shadow_root_and_its_host_keep_each_other` |
| host 不能移進自己的 shadow tree（多深都一樣） | `cargo test -p dom --test it tree::a_host_cant_go_into_its_own_shadow_tree` |
| adopt 帶著 shadow tree（巢狀的也是）；插進 template 的內容時是 template contents owner document 的；adopt 一個內容裡有 host 的 template，shadow tree 到新 document 的 template contents owner document | `cargo test -p dom --test it tree::a_shadow_tree_is_adopted_with_its_host` |
| GC 的工作負載和 `gc_stress` 建有 shadow root 的元素（shadow root 只靠 host 的 rare data 保住，節點也放進 shadow root），模型照 template 的內容一樣追蹤；adopt 的檢查跨過 shadow root | `cargo test -p dom --test it`（`gc_stress::`、`detached_marking::`、`standalone_heap::`）、`cargo test -p dom --test incremental_stress` |
| `attachShadow()` 的設定和預設值、回傳的 `ShadowRoot`、`shadowRoot` 只有 open 的；能有 shadow root 的元素（18 個名稱、custom element 的名稱；別的、SVG 的、第二次是 `NotSupportedError`）；`mode` 必填 | `cargo test -p dom_api --test it shadow_dom::` |
| shadow root 的 `innerHTML`、`setHTMLUnsafe()` 用 host 當 context、取代自己的子節點，`getHTML()`；host 的 light DOM 不動 | `cargo test -p dom_api --test it shadow_dom::a_shadow_roots_markup` |
| `DocumentFragment` 和 `ShadowRoot` 的 `getElementById`，document 的不進 shadow tree | `cargo test -p dom_api --test it shadow_dom::get_element_by_id_on_fragments` |
| shadow root 不能 `cloneNode`、`importNode`（`NotSupportedError`）、`adoptNode`（`HierarchyRequestError`） | `cargo test -p dom_api --test it shadow_dom::a_shadow_root_isnt_cloned_imported_or_adopted` |
| JS：`ShadowRoot` 的 interface（不能建構、prototype 是 `DocumentFragment` 的）、字典和 enum 的錯誤、wrapper 每次同一個、只有 host 撐著時 expando 經過 GC 還在、markup 和 `getElementById` | `cargo test -p dom_js --test it shadow_dom::` |
| wasm：`attach-shadow` 的 record、`shadow-root` 的成員、元素的 `shadow-root`、`NotSupportedError` 的代碼 7 | `cargo test -p dom_wasm --test it engine::a_guest_uses_a_shadow_root` |
| 宣告表、WIT 和規範的 WebIDL | `cargo test -p dom_api --test it api::`、`wit::`、`spec::` |
| shadow tree 跟著 host 變成 connected 和移除：裡面的 iframe 和 script 照 shadow-including 的樹的順序記下（host、它的 shadow tree、它的子節點），巢狀的也是、插進 connected 的 shadow tree 也是；shadow tree 裡的 `title` 不算 document 的；shadow tree 裡的 template 的內容不 connected；`parent_or_shadow_host` 不從 template 的內容跨到 template | `cargo test -p dom --test it tree::shadow_trees_connect_with_their_hosts` |
| `isConnected`、`getRootNode()` 和 `composed`（JS） | `cargo test -p dom_js --test it shadow_dom::connected_with_its_host` |
| `clonable` 的 shadow root 跟著 host 複製（設定一樣，內容不管深淺都複製）、`importNode` 也是，不 clonable 的不複製 | `cargo test -p dom_js --test it shadow_dom::a_clonable_shadow_root_is_cloned_with_its_host` |
| shadow tree 裡的 iframe 跟著 host 有子 frame、跟著 host 移除，不算 window 的 `length` | `cargo test -p dom_js --test it iframes::iframes_in_shadow_trees` |
| shadow tree 裡插入的 script 執行（inline 當場、`src` 載入之後），host 插入時它的 shadow tree 裡的也執行；不在 document 裡的 host 的、shadow tree 裡 template 的內容的不執行 | `cargo test -p shell --test it inserted_scripts::scripts_in_shadow_trees_run` |
| WebDriver 拿得到 shadow tree 裡的元素的參照，host 移出 document 之後是 stale | `cargo test -p shell --test it webdriver::elements_in_shadow_trees_are_referenced` |
| `composed` 的事件經過 shadow root 到 host 和外面，外面的 listener 看到 host 當 target（at target），不是 `composed` 的停在 shadow root；不 bubble 的只到各層的 target；capture 從外往內；分派給 shadow root 自己的停在那裡；分派完 target 是 host，或沒離開 shadow tree 時是 `null` | `cargo test -p dom_js --test it shadow_dom::events_go_out_of_shadow_trees_when_composed` |
| 巢狀的 shadow tree（Lit 的元件）每一層的 host 看到自己，document 看到最外層的 | `cargo test -p dom_js --test it shadow_dom::events_are_retargeted_through_nested_shadow_trees` |
| `composedPath()` 不給外面 closed tree 裡的節點，open 的全給，分派完是空的 | `cargo test -p dom_js --test it shadow_dom::composed_path_hides_closed_shadow_trees` |
| shadow tree 裡的 listener 不設 `window.event`，host 的設 | `cargo test -p dom_js --test it shadow_dom::window_event_isnt_set_in_shadow_trees` |
| `relatedTarget` 照 target 的規則 retarget；兩個在同一個 shadow tree 裡時外面收不到，給 host、`relatedTarget` 在它裡面時完全不分派；外面的保持原樣 | `cargo test -p dom_js --test it shadow_dom::related_targets_are_retargeted` |
| activation：shadow tree 裡的 checkbox 被 `click()` 勾選、label 裡的 span 勾選 label 的控制項、reset 按鈕裡的 span 讓 form reset（沿路徑找到 activation target） | `cargo test -p dom_js --test it shadow_dom::activation_in_shadow_trees` |
| `dom` 的事件路徑帶著 `composedPath()` 的兩個旗標 | `cargo test -p dom --test it events::` |
| focused element 可以在 shadow tree 裡（`dom` 直接設）：host 和它的祖先是 `FOCUS_WITHIN`；host 跟著子樹被移除時 focus 清掉、狀態拿掉 | `cargo test -p dom --test it focus::the_focused_element_in_a_shadow_tree` |
| shadow tree 裡的 script 執行時 `document.currentScript` 是 `null` | `cargo test -p dom --test it tree::current_scripts_stack_per_document` |
| shadow tree 裡的元素的幾何是 0（rect、offset、client、scroll，不出錯），`getComputedStyle` 的值是空字串；host 照舊有 box | `cargo test -p shell --test it geometry::elements_in_shadow_trees_have_no_boxes` |
| WPT 的 `shadow-dom/`（四個子目錄 `disabled`）、`dom/events/`、`dom/nodes/` 照預期結果 | `python3 scripts/test.py wpt shadow-dom/ dom/events/ dom/nodes/` |
| `activeElement` 照 shadow tree retarget：document 看到最外層的 host、每一層 shadow root 看到自己樹裡的、focus 不在裡面的 shadow root 是 `null`；沒有 focused element 時 document 是 body、shadow root 是 `null` | `cargo test -p dom_api --test it shadow_dom::active_element_is_retargeted` |

### custom element

| 項目 | 怎麼驗證 |
| --- | --- |
| window 的 `customElements` 每次同一個、window 和它互相撐住；definition 讀得回建立時的東西、callback 各在自己的 slot、照名稱和建構子找得到、後定義的在前；registry 撐住建構子和 callback | `cargo test -p dom --test it custom_elements::`、`script_objects::custom_element_registry`、`script::every_type_unwraps_only_as_itself` |
| `define` 的檢查和錯誤（不是建構子、名稱、名稱和建構子只能一次、`extends`、讀建構子時再 `define`）、讀建構子的順序和轉換、getter 的例外照樣丟出；`get`、`getName`；`whenDefined` 每次同一個 promise、定義時 resolve、不合法的名稱 reject；從 prototype 刪掉的 callback 經過 GC 照樣活著 | `cargo test -p dom_js --test it custom_elements::` |
| 元素的初始 custom element state（HTML namespace 的合法名稱是 undefined，其他是 uncustomized）；constructor 失敗的元素是 `HTMLUnknownElement`、failed；元素撐住它的 definition | `cargo test -p dom --test it custom_elements::` |
| `new X()` 建新的元素（`X.prototype`、realm 的 document）；`new HTMLElement()`、沒定義的、繼承別的 interface 的、沒有 `new` 是 `TypeError`；`createElement`、`createElementNS` 當場建構，別的 namespace、沒有 window 的 document 不建構；建構子丟出例外或給了不合格的元素時回報、給 `HTMLUnknownElement` | `cargo test -p dom_js --test it custom_elements::` |
| reaction 排進目前的 element queue 或 backup element queue（它的 microtask 只要一次、結束之後再要），照順序取出；只排 custom 的元素、definition 有的 callback、observed 的 attribute；拆掉時還在等的 reaction 不留下 root | `cargo test -p dom --test it custom_elements::` |
| `connectedCallback`、`disconnectedCallback`（shadow-including 的樹的順序、任何 document、`[CEReactions]` 的呼叫的所有插入之後）、`attributeChangedCallback`（各種改法、只對 observed 的）、`adoptedCallback`（兩份 document、在 connected 之前）；callback 的例外回報；巢狀的 `[CEReactions]`；backup element queue 的 microtask；`disabledFeatures` 的 `shadow` | `cargo test -p dom_js --test it custom_elements::` |
| 宣告表的 `[CEReactions]` 和規範的 WebIDL 一樣（274 個成員） | `cargo test -p dom_api --test it spec::` |
| `:defined` 跟著 state、改了讓 style 重算；upgrade 的開始和 construction stack；`define` 的候選、插入時的 upgrade、`try_to_upgrade_tree` | `cargo test -p dom --test it custom_elements::` |
| `define` 照順序 upgrade document 裡的元素（建構子拿到元素本身、prototype 換掉、接著 attribute 和 connected 的 callback）；插進 document、`innerHTML`、clone、`importNode`、`customElements.upgrade()` 的 upgrade；失敗回報、failed、不再試；第二次 `super()`；一個元素只 upgrade 一次；`:defined` | `cargo test -p dom_js --test it custom_elements::` |
| upgrade 之後 `:defined` 改變、元素重新套用 style | `python3 scripts/test.py reftest invalidation/custom-element-defined.html` |
| WPT 的 `custom-elements/`（三個子目錄 `disabled`）照預期結果 | `python3 scripts/test.py wpt custom-elements/` |
| `define` 不讀 `connectedMoveCallback`；`style` 的 camelCase 屬性是 `[CEReactions]` | `cargo test -p dom_js --test it custom_elements::`、`cargo test -p dom_api --test it spec::` |

### slot

| 項目 | 怎麼驗證 |
| --- | --- |
| named assignment（同名的第一個 slot、任何深度、不算裡面的 shadow tree、text 給 default slot、只有 HTML 的 `<slot>`），改了之後重算 | `cargo test -p dom --test it slots::` |
| `HTMLSlotElement`、`name`、`Element.slot`、`assignedNodes()`、`assignedElements()`、`assignedSlot`（closed 的是 `null`）、`flatten`（fallback、巢狀、light tree 的 slot） | `cargo test -p dom_js --test it slots::` |
| wasm 的 guest 拿到 `<slot>`（`from-element`）、它的 `assigned-nodes` | `cargo test -p dom_wasm --test it engine::a_guest_uses_a_shadow_root` |
| manual assignment（`assign()` 的順序、只 assign host 的子節點、一個節點只在一個 slot、重新 assign 換掉整組），弱的（slot 不撐住給它的節點，節點不撐住它的 slot） | `cargo test -p dom --test it slots::` |
| `assign()`（不重複、換 slot、清空、之後插進 host 的、slot 離開 shadow tree、comment 是 `TypeError`；named 的 shadow root 不理它） | `cargo test -p dom_js --test it slots::` |
| 改了 assigned nodes 的 slot 放進 signal slots（一次）、要求 mutation observer 的 microtask，拆掉時清掉 | `cargo test -p dom --test it slots::slot_changes_are_signaled` |
| `slotchange`（不同步、在 observer 之後、一次、bubbles）：host 的子節點、slot 的插入、移除、改名，fallback，`assign()`（另一個 shadow root 的也算），規範的順序，沒改變時沒有 | `cargo test -p dom_js --test it slots::` |
| 被 assign 的節點的事件經過 slot、slot 的祖先和 shadow root 到 host（`composed` 與否）；shadow tree 裡的 listener 看到那個節點；closed 的 shadow root 的 slot 不出現在外面的 `composedPath()`；沒被 assign 的照舊 | `cargo test -p dom_js --test it slots::event_paths_go_through_slots` |
| WPT 的 `shadow-dom/` 照預期結果（183 個測試、102 個全過、1010 個 subtest 過 731 個） | `python3 scripts/test.py wpt shadow-dom/` |

### wasm 的 custom element

| 項目 | 怎麼驗證 |
| --- | --- |
| wasm 的 guest 的 `customElements`：`define` 收 record（建構和 callback 是 guest 自己的 id）、`get` 給回 `construct` 的 id、`get-name` 照 id 找（別的 callback 的 id 找不到）；不合法的名稱是 SyntaxError、定義過的名稱是 NotSupportedError | `cargo test -p dom_wasm --test it engine::a_guest_defines_a_custom_element` |
| `kun:dom` 0.13 的 guest 多了 `construct`、`connected`、`disconnected`、`adopted`、`attribute-changed` 五個 export，都是同步的；JS 收 class 的 `define` 和 WIT 收 record 的 `define` 不是 overload | `cargo test -p dom_api --test it wit::the_guest_exports`、`api::overloads_are_told_apart_by_count_and_interfaces` |
| guest 定義的元素在 microtask checkpoint 經過它的 export 建構和呼叫：`define` upgrade document 裡的元素（`construct` 時還不是 `:defined`、observed 的 attribute 的 `attribute-changed`、`connected`），`define` 之後 `create-element` 的元素到 checkpoint 才建構；`disconnected`、`adopted`（舊的和新的 document）、`attribute-changed`（帶 namespace 的也是）照順序；借給呼叫的 handle 都還回去 | `cargo test -p dom_wasm --test it engine::a_guests_custom_element_is_called_back` |
| guest 的 `construct` trap 時元素是 failed、沒有 definition，頁面照樣進行 | `cargo test -p dom_wasm --test it engine::a_guests_custom_element_constructor_trapping` |
| guest 的 `construct` 回 false 時元素是 failed、沒有它的 upgrade 排的 `connected`，guest 照樣進行 | `cargo test -p dom_wasm --test it engine::a_guests_custom_element_is_called_back` |
| Lua 的 `define` 收函式的 table；`get`、`get_name`、`when_defined` 拿回 `construct` 的函式；script 期間什麼都不呼叫；upgrade 時 `construct`、observed 的 attribute、`connected` 照順序，收到的元素留得住；`construct` 出錯的元素不是 `:defined`、錯誤回報、runtime 照樣進行；`attribute_changed` 的 namespace、`disconnected`、`adopted` 的兩份 document | `cargo test -p shell --test it wasm_reftests::w9_lua` |
| Rust 的 guest 定義 custom element（record、`construct`、observed 的 attribute、`connected` 的順序，`get`） | `cargo test -p shell --test it wasm_reftests::w6_rust` |
| `about:wasm-demo` 的 C、Rust、Lua 的 custom element 在頁面上建構，點了之後 C 的 `attribute-changed`、Lua 留著的元素更新 | `python3 scripts/test.py scenarios wasm-demo` |

## 做過的檢查

四段做完時都跑過 `cargo fmt --check`（連同 `sdk/lua/generator`；第 4 段也連同 `sdk/rust`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`：第 1 段 2257 個通過（另有兩個 Linux 上字型度量造成的 canvas 失敗和一個不穩定的測試，見下面）、第 2 段 2296 個、第 3 段 2310 個、第 4 段 2313 個（42 個 binary，55 秒）。每一步都跑了改到的 crate 的測試、相關目錄的 WPT 和 mutation test（`mutants-1a.toml` 到 `mutants-4c.toml`，每一份的漏網和處理寫在下面）。

- **做了什麼**：
  - 第 1 段：shadow root 沿用 `NodeKindData::DocumentFragment`（多一個 `shadow` 的設定，不另開 variant，README 的第 2 項）、host 的 rare data 的 `shadow_root`，adopt 帶著 shadow tree；shadow-including 的 connected 和根（`parent_or_shadow_host`），shadow tree 裡的 script 和 iframe 照樣執行和載入、`clonable` 的 clone；`dom_api` 的分派照 DOM 的「dispatch」重寫（`PathItem` 記 shadow-adjusted 的 target、`relatedTarget` 和 closed tree 的旗標，`composedPath()` 照它過濾）；`activeElement` 的 retarget 和 `ShadowRoot.activeElement`；WIT 只加不改。
  - 第 2 段：registry 是 window 撐住的 script 物件，definition 是沒有 wrapper 的 GC 物件（建構子和四個 callback 各在一個 `CallbackSlot`）；custom element state 在元素的 `NodeState`，definition 在 rare data，建構失敗的元素是 `HTMLUnknownElement`（`NodeKindData::Element` 的 `unknown`）；reaction 的佇列在 `DomState`（第 16 條），宣告表的 `[CEReactions]`（274 個成員，和規範的 WebIDL 比對），push 和 pop 在前端的 `custom_elements::call_member`；upgrade、construction stack（definition 的 `#[no_trace]` 欄位存元素的 id）、換 wrapper 的 prototype、`:defined`（`ElementState::DEFINED`）。
  - 第 3 段：slot 的 assignment 是 `DomState` 裡帶著 `dom_tree_version` 的快取（第 19 條），manual assignment 是 slot 的 rare data 的 `WeakSet` 和節點的弱 edge；`slotchange` 由修改的操作直接找受影響的 slot（第 20 條），少見的修改比較 shadow root 前後的 assignment，signal slots 在 `MutationState`、和 mutation observer 同一個 microtask；`parent_for_event` 經過 assigned slot。
  - 第 4 段：`kun:dom` 升到 0.13.0（第 22 條），guest 多了 `construct`（回傳 `bool`，第 25 條）、`connected`、`disconnected`、`adopted`、`attribute-changed`；`define` 分成 JS 收 class 的和 WIT 收 record 的兩個成員（`define_for_guest`，宣告表的 overload 只算同一個前端都有的形式）；upgrade 的 `Construct` 經過 `ScriptHost::construct_upgrading`，wasm 的交給 guest 的 `construct`；外部 engine 的建構子在 `createElement` 時照沒有 synchronous flag 的做（undefined、排 upgrade）；`store::call` 帶著 `this`，借元素和 document 給呼叫；Lua 的 generator 支援 record 裡的 callback 和 `list<string>`（L1，第 23 條；async 的 import 收值），runtime 的五個 export 給 Lua 自己的 handle（第 26 條）；`about:wasm-demo` 的 C、Rust、Lua 各定義一個 custom element。
- **設計在實作中的改變**（理由寫在 README 的對應項目和條目）：shadow root 不另開 variant（1a）；adopt 帶著 shadow tree、construction stack、`:defined`、`attachShadow` 對 `disabledFeatures` 的檢查在步驟之間移動（第 14、17、18 條）；custom element state 從 rare data 改到 `NodeState`（2b）；Lua 的 generator 從 4c 提前到 4a（Lua 的 runtime 由它產生，不支援就編不出來）；`construct` 改回傳 `bool`、Lua 的物件可以留著（4c 開始寫 Lua 時發現，第 25、26 條）。
- **找到的 bug**：shadow tree 裡的 script 執行時 `document.currentScript` 是那個元素（1e，WPT 的 `Document-prototype-currentScript`；修在 `dom` 的 `push_current_script`）；`define` 讀 `connectedMoveCallback`（kun 沒有 `moveBefore`）、`style` 的 camelCase 屬性少了 `[CEReactions]`（2e，WPT）；Lua 的 generator 把 async 的 import 的字串參數傳成指標（4a，之前沒有不經過 struct 的字串參數）。
- **寫測試時的錯**：`window.length` 不算 shadow tree 裡的 iframe（kun 的 0 才對）、kun 沒有 Window 的具名屬性、`join` 把 `null` 印成空字串；`slotchange` 的預期順序有三處和規範不同，照規範改；`focus` 的測試直接比對整個 `ElementState`，一般元素多了 `DEFINED`；guest 的例外代碼是 `kun:dom` 的（`NotSupportedError` 是 7）；W9 的新檢查在第三個 script 裡用了第一個 script 的 local。
- **mutation test**：
  - 漏網之後補了測試的：template 的內容裡的 host 被 adopt（1a）；target 和 retarget 後的 `relatedTarget` 相同、沿路徑找 activation target（1c）；`disable_shadow` 的來源、「不是建構子」的兩條（有 `prototype` 的 arrow function）、`Workload` 的 definition 數量（2a）；一個元素排了兩個 upgrade、construction stack 不 pop（2d）；upgrade 期間不給樹的 callback（2e，`no_callbacks_while_upgrading`）；`flatten` 時不在 shadow tree 裡的 slot（3a）；slot 改名之後拿到 host 的子節點（3c）；`attribute-changed` 的 namespace（4b 的 C guest、4c 的 W9 各補一個帶 namespace 的 attribute）、`construct` 的 trap 被忽略（4b）。
  - 換了測試指令之後抓到的：JS 和 WIT 的 `define` 算成 overload（4a，JS 看不出來，`dom_api` 的 `api::overloads_are_told_apart_by_count_and_interfaces` 抓到）；`composedPath()` 往外的那一段不過濾（1c 時等價，有了 slot 的路徑之後 `dom_js` 的 `slots::` 抓到，從 `docs/testing.md` 的等價清單拿掉）。
  - 多餘的程式（拿掉程式和 mutant）：排 reaction 時再檢查元素是 custom（2c）；`set_custom_element_state` 打開 `custom_seen`（2e）；比較 shadow root 之前把快取標成過期（3c）。
  - 等價的（記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）：在 retarget 到的 host 上找 activation behavior（1c）；upgrade 失敗時留著 definition、留在 precustomized（2e）；`assign()` 不去掉重複的節點（3b）；第一個 slot 在「target 換成它」的分支上的旗標（3d）。只靠審查的：slot 的快取丟掉被回收的 shadow root 的紀錄（3a，`docs/test-gaps.md` 的「只靠審查」）。
  - Lua runtime（C）的 mutant 每條先用 `ci.py` 的同一行 clang 只重編 runtime 再跑 W9（和 `ci.py guests` 編出來的 sha256 一樣），跑完用還原的原始碼重編、確認 sha256 不變。
  - 每段做完時 fmt 改了排版的 mutant 的 `from` 跟著改、重跑；第 4 段另外把 1b 一條在第 3 段之後重複出現的 `from`、2a 兩條（`define` 拆成 `check_definition`、`add_definition`）、4b 兩條重新對位，連同整份 4a、4b 重跑，全部抓到。
- **WPT**（收錄的檔案、`disabled` 的理由和數字在 `tests/wpt/README.md`）：`shadow-dom/` 整個收（`declarative/`、`focus/`、`focus-navigation/`、`reference-target/` 和七個逾時的檔案 `disabled`），第 1 段 184 個測試、91 個全過、960 個 subtest 過 627 個，第 3 段 183 個、102 個全過、1010 個過 731 個；剩下的大多等 Window 的具名屬性、shadow root 的 `elementFromPoint`、shadow tree 的繪製和 `:host`。`custom-elements/` 整個收（`form-associated/`、`state/`、`reactions/customized-builtins/` `disabled`）：140 個測試、33 個全過、3361 個 subtest 過 2585 個；ERROR 的是 `ElementInternals`、customized built-in、scoped registry、`document.write`、SVG 文件當測試頁、parser 同步建構。`dom/` 裡為了 shadow DOM `disabled` 的 6 個：2 個拿掉 `disabled`、4 個改寫理由；`dom/events/`、`dom/nodes/`、`html/dom/` 的預期結果隨著各步更新，沒有退步。
- **Speedometer 的盤點**（`python3 scripts/ci.py speedometer --inventory`）：第 1 段之後 Perf-Dashboard 從卡在 `alert` 變成跑完；第 2 段之後它缺的不再有 `customElements`，TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM 在 custom element 這一側沒有缺的，照舊卡在 `CSSStyleSheet`、`adoptedStyleSheets`（RFC 0016 的第 3 段，第 10 條）。
- **guest 的重編**：WIT 每次改了都用 `ci.py guests` 重編全部（每次 8 分 36 秒到 9 分 52 秒）；第 4 段 4c 的第一次編到一半發現一個測試 guest 的 `construct` 還沒改成 `bool`，停掉、改好再編；最後 fmt 改了 Rust guest 的排版（panic 的位置編進 wasm），再編一次。
- **第 1 段的失敗和不穩定**：`browser` 的 `navigation::links::canvas_2d` 和 `raster` 的 `ink_bounds_are_the_glyphs_outlines_together` 在 Linux 上失敗，在 `main`（沒有這份的改動）一樣失敗：RFC 0033 在 macOS 上做的、Linux 的字型度量不同，和這份無關；之後的段落沒有再出現。`shell` 的 `automation_pipe::frames_are_switched_to_and_kept_through_their_navigations` 有一次一個 WebDriver 指令 30 秒沒有回答，之後單獨跑、整個 `shell`、同時跑 64 次都重現不出來；這份在 renderer 只改了 WebDriver 判斷元素在不在 document 的往上走，看不出關係，再出現時留下 kun 的 log（`--log`）追。
- **README 的「待決定」的最後一項**（第 9 項：shadow tree 裡的元素上的 `getComputedStyle` 和幾何的 API）：4d 才補上確認，加了 `geometry::elements_in_shadow_trees_have_no_boxes`。幾何照 `display: none` 的子樹回答（0），但 `getComputedStyle` 的值是空字串（`display: none` 的子樹照樣算得出值）：style 不走進 shadow tree。照第 9 條等 K-2，記進 `docs/limitations.md`；這個測試記下現在的回答，不是新的行為，沒有 mutant。
- **畫面**：shadow tree 不畫（第 9 條），第 1 到 3 段沒有新的情境，第 2 段有 `:defined` 的 reftest（`invalidation/custom-element-defined.html`）；第 4 段的 `wasm-demo` 情境等三個 guest 的 custom element。`python3 scripts/test.py automation` 不用跑（沒有動到視窗那一層）。
