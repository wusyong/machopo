# RFC 0067 的進度：有型別的 bindings

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-20。完成：2026-10-21，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0067` 上做；第 4 段只做了 4a（第 27 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型：手寫六個成員的專用路徑，量每一項省多少，決定核心入口的形狀 | 完成 | 2026-10-21 |
| 2 生成：`declare!` 替每種形狀的成員生成核心入口和 V8 的轉接 | 完成 | 2026-10-21 |
| 3 wasm 的轉接 | 完成 | 2026-10-21 |
| 4a `Window` 的成員走專用的路；成員的 function 讓 V8 檢查 `this` 能不能存取 | 完成 | 2026-10-21 |
| 4b–4d | 不在這份做（第 27 條） | |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| JS 的專用路徑 | 每種宣告的形狀（讀的成員、getter、`[CEReactions]` 的 setter、方法、預設值、dictionary 參數、可為 null 的、數字、布林、字串）不經過共用的 callback；`Window` 的成員也是 | `cargo test -p dom_js --test it bindings::typed_paths_skip_the_shared_callback`（`testing::shared_calls` 的計數） |
| JS 的專用路徑 | 和共用的路行為一樣：錯的 `this` 被 `Signature` 擋下、缺參數、轉換的錯誤和它丟的例外、DOM 的錯誤、`[CEReactions]` 在回來之前、`after_call` | `bindings::typed_members_check_this_and_behave_as_the_shared_path`、`mutation_observers::one_call_queues_the_callback` |
| JS 的專用路徑 | `this` 和 interface 參數只用它的介面的 tag unwrap | `bindings::this_is_unwrapped_as_its_interfaces_type` |
| root | 讀的成員不 root `this`，回傳已有 wrapper 的節點不 root | `bindings::an_existing_wrapper_is_returned_without_its_realm`（`dom::testing` 的 root 計數） |
| `Window` 的成員 | `this` 是 global proxy：直接讀、`.call(window)`、`.call(undefined)`、不帶 receiver、同源的另一個 window | `bindings::typed_paths_skip_the_shared_callback`、`realms::only_the_same_origin_reaches_another_realm` |
| 跨源 | 自己 realm 的成員用在跨源的 window 上（getter、setter、方法，兩條路，`EventTarget` 的）是 `TypeError`；static 的方法和 namespace 的成員照常執行 | `realms::only_the_same_origin_reaches_another_realm` |
| wasm 的專用路徑 | 六個成員不經過共用的 `call`；`nodeType` 不建 root，`firstChild` 每次三個（回傳的節點、新 handle 的 slot、guest drop 時的 holder） | `cargo test -p dom_wasm --lib engine::typed_host_functions_take_no_shared_path` |
| wasm 的專用路徑 | 回傳只給 JS 的介面的物件、`as` 改名的形式 | `engine::typed_host_functions_at_their_edges`（guest `typed_edges`） |
| wasm 的清單 | `src/typed_list.rs` 是宣告表的 | `typed::tests::the_list_is_the_tables`（`MACHOPO_UPDATE_GOLDEN=1` 重寫） |
| 量測 | 每次呼叫的指令數，JS 和 wasm | `python3 scripts/bench.py calls`；工具的測試 `python3 scripts/bench/calls/calls_test.py` |
| WPT | `dom/`、`html/dom/`、`webidl/`、`html/browsers/the-window-object/`、`html/browsers/history/the-location-interface/`、`dom/events/` 沒有非預期的結果 | `python3 scripts/test.py wpt <目錄>` |

## 做過的檢查

**完成時（2026-10-21，Fedora 44）**：`cargo test --workspace` 3402 個通過（含 wasm 的 reftest；fork 的 `kun-v152.2.0-13` 還沒發佈，設 `RUSTY_V8_ARCHIVE` 連結本機編的 release V8）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`（連同 `sdk/lua/generator`；`cargo fmt` 改了 4a 的一個測試的排版之後）、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`。每一段做完時也各跑過一次（第 1 段 3398 個、第 2 段 3399 個、第 3 段 3402 個）。rebase 到 `main` 的 `c7e83257`（RFC 0064、0066、0068 做完）之後全部再跑一次：`cargo test --workspace` 3477 個通過，其他照樣通過；`main` 換了 parley 的 pin，`ci.py third-party` 先把 `../third_party/parley` 切過去。

**每一段做了什麼**：

- **第 1 段：原型**：
  - `dom` 的 `BindingContext` 加上 `read_node`、`read_node_for_node`（用 `Node` 的 tag unwrap `this`，在 read scope 裡交給 closure，不 root；回傳的節點有 wrapper 時在同一個 scope 裡讀出來，也不 root）和 `unwrap_node`；論證寫進 `crates/dom/soundness.md`。
  - `dom_api` 的核心：`node::node_type_for`、`element::get_attribute_in`（找屬性時借用、不複製整份、不替每個屬性組名字；通用的路也改用它）、`custom_elements::with_reactions`（`[CEReactions]` 的步驟，`call_member` 也改用它）。
  - `dom_js` 的 `typed`：六個成員（`Node.firstChild`、`nodeType`、`appendChild`，`Element.getAttribute`、`setAttribute`，`Document.createElement`）手寫的 V8 callback，裝的時候帶 interface 的 template 的 `Signature`。
- **第 2 段：生成**：
  - `dom_api` 的 `typed`：`TypedMember`（每個成員一個零大小的型別，`call::<F: Front>`）、`Front`（前端的一次呼叫：數參數、拿參數、`this`、DOM、讀節點、交回結果、丟例外）、`TypedVisitor` 和 `visit_typed`、讀的成員的結果（`ReadOutput`、`Plain`、`ReadValue`）。`declare!` 的 `@typed` 對每種宣告的形式都有一條規則（第 13 條），生成 getter、setter、方法、讀的成員的路；宣告加上 `read attribute`、`read method`（`firstChild`、`nodeType`、`getAttribute` 改用它們，函式拿 `&DomReadScope` 和 `NodeRef`）。`ApiType` 多了 `take_arg`、`give`（第 12 條），`DomString`、數字、布林、可為 null 的和 interface 覆寫成前端自己的轉換。
  - `dom_js` 的 `typed`：`V8Front` 和走訪器，建 templates 時替接的成員各做一個 function（callback 是 `v8_callback::<M>`，建 templates 的程式不泛型，第 15 條），帶 `Signature`（setter 也是）；不接 overload、回傳 promise、`FrozenArray`、observable array、可變參數、只有 WIT 有的、`glue.js` 的、`[Global]` 的。手寫的原型拿掉。`testing::shared_calls` 數經過共用 callback 的呼叫。
  - `dom` 的 `BindingContext::read_node_into` 取代原型的 `read_node`、`read_node_for_node`、`unwrap_node`，論證改寫在 `crates/dom/soundness.md`。
  - `setAttribute` 等找屬性時借用、只複製找到的那一個（`attribute_by_name`、`attribute_by_namespace`，第 10 條）。
  - 成員的對應：`TypedMember` 帶 `EXPOSURE`，找成員時比對名字、種類、exposure，不找 `as` 改名的；兩條路對到同一個成員時建 templates 就 panic（`define` 的 JS 和 WIT 兩個形式同名同種類）。
- **第 3 段：wasm 的轉接**：
  - `bench.py calls`（第 16 條）：JS（`dom_js` 的 `bindings::bench_calls`）和 wasm（`dom_wasm` 的 `engine::bench_calls`，guest `crates/dom_wasm/tests/guests/calls.c`）各一組固定次數的迴圈，`perf stat` 量每次呼叫的指令數；工具的測試 `scripts/bench/calls/calls_test.py`。
  - `dom_api` 的 `wire`：每種表的型別在呼叫邊界上的形狀（`ApiType::Wire`，`from_wire`、`into_wire`，預設經過 `Value`）；`TypedMember` 多了 `Args`、`Ret`、`call_with`（參數已經轉好、直接回傳結果）和 `MEMBER`；`Front` 多了 `read_this` 和 `CE_REACTIONS`。成員的型別是每個介面的 `M<J>`，在 `typed_paths::<介面>`（第 21 條）。
  - `dom` 的 `DomReadScope::held_node`：wasm 的讀的成員不 root `this`（第 18 條），論證在 `crates/dom/soundness.md`。
  - `dom_wasm` 的 `typed`：每種形狀對應 wasmtime 的型別，參數和回傳都是簡單型別的成員用 typed 的 `func_wrap`（照簽名去重，約 120 種），`src/typed_list.rs` 由測試從宣告表生成（1235 個成員）。`[CEReactions]` 的 reaction 和共用的路一樣不在呼叫裡跑。`testing::shared_calls` 數經過共用的 `call` 的呼叫。
- **4a：`Window` 的成員**：`[Global]` 的成員也走專用的路（第 24 條）；成員的 function 讓 V8 檢查 `this` 能不能存取（第 26 條：fork 接出 `FunctionTemplate::set_accept_any_receiver`，`dom_js` 的 `templates::checks_receiver` 套在屬性的 getter、setter、方法、`[PutForwards]` 和 `[Replaceable]` 的 setter、iterable 的方法上，static 的方法和 namespace 的成員不套）。cached accessor 做了原型、不採用（第 25 條）。

**量測**（`details.md`）：
- **第 1 段**：讀的成員快 2–4 倍（`firstChild` 4.1、`nodeType` 3.7、`getAttribute` 2.0），和 Chrome 的倍數從約 13 倍到約 3 倍；mutation 的成員 1.09–1.45 倍，`appendChild` 和 Chrome 一樣快。
- **第 2 段**：每次呼叫的指令數，`perf stat` 量固定次數的迴圈，比 runs/s 穩得多（同設定三次差 0.2%）、跨 build 可比。生成的路照型別轉換之後和原型差 0–110 個指令（`appendChild` 多 300：節點參數經過 `object_of` 和種類檢查），`setAttribute` 不複製屬性之後比原型少 650。
- **第 3 段**：wasm 每次呼叫的指令數 `nodeType` 4152 → 2184、`firstChild` 6747 → 4287、`getAttribute` 6566 → 3962、`setAttribute` 8154 → 5946、`appendChild` 10743 → 7900、`createElement` 10569 → 8150；JS 照舊（`nodeType` 209）。取樣：剩下的一半是 wasmtime 每次 host 呼叫建 task 的記帳和 resource 表（「之後的可能」）。
- **4a**：`document` 每次 2727 → 2561 個指令；cached accessor 的原型 2519 → 約 0，但 Speedometer 裡 global 的 getter 的 V8 慢路和 binding 加起來每次迭代約 0.5 ms。順帶看到 NewsSite-Next 的 `innerHeight` 每次更新 frame 自己的 layout，約每次迭代 38 ms（Chrome 的子 frame 只更新 parent 的），不在這份 RFC 的範圍。
- **Speedometer**（第 3 段之後，第 22 條）：script 的差距 220.5 → 181.2 ms，減少的幾乎都在 bindings（131.5 → 96.8 ms）；總分 1.52 和第 2 段之前的 1.47 差在雜訊裡（`--against dc5000da`）。

**編譯的成本**（`details.md`）：
- **第 2 段**：一開始 release 的 `dom_js` 從 6 秒變成 127 秒、dev 加一個成員要 150 秒；建 templates 的程式改成非泛型、`[CEReactions]` 的 closure 只寫一次之後是 17 秒和 28 秒（第 15 條）。另外試的 callback 改成成員型別的方法（再省兩成，要 `bindings` 的 `transmute`）和每個介面一個模組（沒有貢獻）不採用，對照實驗三輪輪流量。
- **第 3 段**：release 的 `dom_wasm` 10.5 → 26.5 秒、dev 加一個成員 19.8 → 41.0 秒（`dom_api` 19.6 → 20.7、`dom_wasm` 10.6 → 31.9）；不照簽名去重時 release 的 `dom_wasm` 是 100 秒。成員的型別先用了所有成員共用的 `TypedPath<I, J>`，`dom_api` 多了 14 秒（coherence checking 13.2 秒，`-Ztime-passes`），改成每個介面的 `M<J>` 之後 3.1 秒。release 的 `dom_js` 沒有變慢（14–15 秒，第 2 段 17 秒）。量法：一次對照實驗先把兩個 package 一起熱身、再單獨量，cargo 的 feature 組合不同、相依重編，數字不能用，丟掉。

**找到的 bug 和實作時的發現**：
- **第 4 條**：isolate 的狀態改用私有的 thread-local 先讀，同一個 binary 比沒有差別，不改。一次比兩個 build 的實驗看起來差一成，是 build 之間的差，量法記在 `details.md`。
- **跨源的 window**：跨源的 window 能用自己 realm 的成員碰到（`Object.getOwnPropertyDescriptor(window, 'document').get.call(other)`、`getSelection.call(other)`、`addEventListener.call(other, …)`、`fetch.call(other, …)`）：V8 的 function template 預設不檢查 receiver。共用的路也一樣，不是這一段造成的；第 26 條修好。
- **不做的**：第 8 條的交叉檢查改成不做（第 14 條）。

**測試**：
- **第 1 段**：`dom_js` 的 `bindings::typed_members_check_this_and_behave_as_the_shared_path`（錯的 `this` 被 `Signature` 擋下、`getAttribute` 的大小寫和帶 prefix 的名字、缺參數、DOM 的錯誤、`setAttribute` 和 `appendChild` 的 `[CEReactions]` 在回來前）、`mutation_observers::one_call_queues_the_callback`（只呼叫 `setAttribute` 也排 observer 的 microtask）；`bindings::an_existing_wrapper_is_returned_without_its_realm` 改成專用的路一個 root 都不建（`firstChild` 0、通用的路的 `childElementCount` 1）。`Signature` 擋下的 `TypeError` 只說「Illegal invocation」，通用的路的會說型別。
- **第 2 段**：`bindings::typed_paths_skip_the_shared_callback`（每種形狀都不經過共用的 callback：讀的成員、getter、`[CEReactions]` 的 setter、方法、預設值、dictionary 參數、`Option` 和數字、布林、字串的結果、setter 直接呼叫的回傳值；字串參數的 `ToString` 和它丟出的例外、interface 參數是別的種類或不是平台物件時的錯誤；可變參數的和 `[Global]` 的走共用的路）；`bindings::this_is_unwrapped_as_its_interfaces_type` 加上 interface 參數只用它的 tag unwrap（`appendChild` 的節點、`compareBoundaryPoints` 的 `Range`）；`typed_members_check_this_and_behave_as_the_shared_path` 加上 `[CEReactions]` 的 setter；`mutation_observers::one_call_queues_the_callback` 加上只設一個屬性。
- **第 3 段**：`engine::typed_host_functions_take_no_shared_path`（六個成員多呼叫二十次，經過共用的 `call` 的次數不變；`nodeType` 不建 root，`firstChild` 每次三個：回傳的節點、替新的 handle 佔 slot 和 guest drop 它時的 holder，`this` 不 root）、`engine::typed_host_functions_at_their_edges`（guest `typed_edges`：`ownerSVGElement` 回傳只給 JS 的介面的物件、`as` 改名的 `stroke-path2d`）、`typed::tests::the_list_is_the_tables`（清單是宣告表的）。
- **4a**：`bindings::typed_paths_skip_the_shared_callback` 改成 global 的成員不經過共用的 callback（直接讀、`.call(window)`、`.call(undefined)`、不帶 receiver），錯的 `this` 是 `Illegal invocation`；`realms::only_the_same_origin_reaches_another_realm` 加上同源的另一個 window 當 `this`（getter 和方法拿到那個 window 的），跨源的當 `this`：專用的路和共用的路的 getter、setter、方法和 `EventTarget` 的方法都是 `TypeError`，static 的方法和 namespace 的成員照常執行。

**mutation test**（spec 不放 repo，scratchpad 的 `mutants/0067-{1,2,3,4a}.toml`）：
- **第 1 段**：17 條：第一次 11 條抓到，漏 6 條。補了測試之後抓到 4 條：`getAttribute` 只比 local name、不檢查冒號（補 `href`、`xlink_href`）、`appendChild` 沒有 `[CEReactions]`（補 `connectedCallback`）、`setAttribute` 沒有 `after_call`（補 `one_call_queues_the_callback`）。等價的 2 條：`createElement` 不包 `[CEReactions]`、不呼叫 `after_call`（`docs/test-gaps.md`）。量測用的開關（環境變數 `MACHOPO_GENERIC_BINDINGS`）是整個 process 共用的全域狀態，`sanity.py boundary` 不讓 renderer 那一側有，量完拿掉。
- **第 2 段**：25 條：第一次 23 條抓到，漏 2 條，補了測試之後都抓到：setter 回傳 `null`（setter 直接呼叫時看得到回傳值，補 `set.call`）、interface 參數照每個型別試（節點的 tag 排在第一個，節點參數數不出差別，補 `Range` 的參數）。第 15 條改了之後整份重跑，25 條都抓到。
- **第 3 段**：12 條：第一次 10 條抓到，漏 2 條，補了 `typed_edges` 之後都抓到：回傳的物件用宣告的介面當 resource（沒有 guest 呼叫過回傳只給 JS 的介面的成員）、`as` 改名的形式拿第一個形式的函式（沒有 guest 用過那幾個函式，wasmtime 只檢查 guest import 的）。
- **4a**：10 條：5 條抓到，漏 5 條都是等價的：namespace 的成員也檢查（namespace 的操作本來就是 static，那個條件多餘，拿掉了）；`[PutForwards]`、`[Replaceable]` 的 setter 和 iterable 的方法不檢查（`docs/test-gaps.md`：setter 接著碰 `this` 的屬性時被 V8 擋下，`Window` 不是 iterable）。

**WPT**：
- **第 1 段**：`dom/nodes/` 418 個測試、15199 個 subtest，沒有非預期的結果。
- **第 2 段**：`dom/`、`html/dom/`、`webidl/` 800 個測試、122170 個 subtest，沒有非預期的結果（第 15 條之前和之後各跑一次）。
- **4a**：`html/browsers/the-window-object/`、`html/browsers/history/the-location-interface/`、`dom/events/` 285 個測試、970 個 subtest（176 秒），`html/dom/`、`webidl/` 45 個測試、59840 個 subtest，沒有非預期的結果。

**fork**：
- `set_accept_any_receiver` 是 fork 的 `7d7fc555`，發成 `kun-v152.2.0-13`（`-12` 是之前一個已經 revert 的改動用掉的），kun 的 `RUSTY_V8_MIRROR_TAG` 跟著改。fork 的 workflow 編好之前，本機的 build 設 `RUSTY_V8_ARCHIVE` 連結本機編的 release V8；本機的 debug V8 沒有重編（只有 `V8_FORCE_DEBUG=1` 用，等 release 的）。
