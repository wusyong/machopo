# RFC 0097 的進度：binding 每次呼叫的成本往 Chrome 靠

開始日期：2026-11-11；完成日期：2026-11-13。Linux x86_64，AMD Ryzen AI 9 HX 370；對照的 Chrome 是本機的 Chromium headless（第 3 段的 blink-perf 用 Chrome for Testing 152.0.7977.82）。

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。「做過的檢查」照做的時候的名字寫：`V8Front`、`V8Op` 在 4m-3b 改名成 `WholeFront`、`WholeOp`，`Kept` 在同一段刪掉；`PLAIN`（成員能不能走只拿 info 的 front 的標記）和它的檢查在 4m-2 刪掉，現在每個成員都從 `InfoFront` 開始。收尾對 Chrome 量的整份數字在 `measurements.md`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型和量測（不合併） | 完成 | 2026-11-11 |
| 2 字串參數已經是字串就不轉換 | 完成 | 2026-11-11 |
| 3 front 只拿 callback 的 info：參數和結果單純的成員 | 完成 | 2026-11-11 |
| 4a 結果怎麼帶出操作：`RetType::Pending`、`finish`，front 定 `PendingString`、`PendingNode`、`PendingNumber` | 完成 | 2026-11-11 |
| 4b 新的 front 補上數字、布林、有預設值的參數，回傳不是節點的物件 | 完成 | 2026-11-11 |
| 4c 預先建 wrapper 的結果（`createElement`、`cloneNode`、`createTextNode`） | 完成 | 2026-11-12 |
| 4d handle 的機制（`Param` 多 front 的型別參數）、callback 和 `any`；函式拿 front 的成員照型別算 `PLAIN` | 完成 | 2026-11-12 |
| 4e rest、`any`、dictionary 的 handle、節點或字串；event handler 的 getter 和回傳 `any` 的；其餘的參數型別留到 4j（決定 32） | 完成 | 2026-11-12 |
| 4f 其餘的回傳型別（promise、陣列、dictionary、第一次讀才做的值）：由回傳型別帶出操作（決定 33） | 完成 | 2026-11-12 |
| 4g static 成員和 overload 的外殼；目前的 realm 的 window 從 isolate 讀（決定 34） | 完成 | 2026-11-12 |
| 4h 之前：新的 front 補齊操作這一側的方法（`OpFront` 裡它沒有、成員的函式卻呼叫得到的 9 個：刪掉或實作；決定 35） | 完成 | 2026-11-12 |
| 4h constructor 的外殼（做到一半時停下來做上一列） | 完成 | 2026-11-12 |
| 4i-1 indexed getter 的 interceptor（`list[i]`、`children[i]`、`classList[i]`、`attributes[i]`；決定 36） | 完成 | 2026-11-12 |
| 4i-2 named 的 getter 和 setter 的 interceptor（`dataset.a`、`children.px`、`attributes.id`；決定 37）；空字串不做字串 | 完成 | 2026-11-12 |
| 4i-3 只被這次呼叫持有的字串不進字串的快取（決定 38） | 完成 | 2026-11-12 |
| 4j-1 `KeptObject` 的編號和 `Uint8ClampedArg` 換成 handle；UI 事件的 constructor 和 `ImageData`（決定 39） | 完成 | 2026-11-12 |
| 4j-keys dictionary 的成員名稱是名稱表的固定名稱，key 從名稱的 V8 字串的快取拿（決定 40） | 完成 | 2026-11-12 |
| 4j-2 操作前讀成 Rust 值的參數型別（`fetch` 的 init、`postMessage`、keyframes…），連同卡在這些型別的 constructor、method、setter 和 static（決定 39） | 完成 | 2026-11-12 |
| 4k 研究：一次呼叫多出來的成本一類一類拆開、和 Chrome 對照、列每條冷的路該呼叫的 V8 API（決定 41、42） | 完成 | 2026-11-12 |
| 4k-A 回傳非整數的數字：直接設回傳值（決定 42） | 完成 | 2026-11-12 |
| 4k-B 回傳現算的字串：不經過 `DomString`、`Box`、`V8Front`；fork 加從 callback info 借的 token | 完成 | 2026-11-12 |
| 4k-B2 回傳關鍵字和現算的文字的函式改用 `DomStr::fresh`（不做 `DomString`） | 完成 | 2026-11-13 |
| 4k-C1 33 個會呼叫 script 的成員搬到新的外殼（`Front::whole`；決定 43） | 完成 | 2026-11-13 |
| 4k-C2 「先試的普通函式」的機制（`#[first(..)]`）和走訪的 9 個成員的快路（沒有 filter 時一個操作） | 完成 | 2026-11-13 |
| 4k-C3 剖析 `inp.select()`、`abort()`（Chrome 的 1.29 倍） | 完成 | 2026-11-13 |
| 4k-D 新的 wrapper：同一個 realm 的節點直接複製樣板物件、不經過 `V8Front`（root 還在） | 完成 | 2026-11-13 |
| 4k-E 不借字元的字串參數（非 ASCII、還沒攤平的）用 isolate 複製，不經過 `V8Front`；Latin-1 原地讀 | 完成 | 2026-11-13 |
| 4k-F 對 Chrome 量 dictionary、例外、陣列、promise（都不落後）；只做 `getAttributeNames` | 完成 | 2026-11-13 |
| 4k-G 最熱的路本身：對 Chrome 數指令；不影響安全性的寫法調整（wrapper 的 tag 比一次） | 完成 | 2026-11-13 |
| 4l cross-origin 的成員、要找別的 realm 的 window 的、legacy factory 的 callback 只拿 info（整個呼叫在一個 scope 裡）；4i 留下的 interceptor 不動 | 完成 | 2026-11-13 |
| 4m-2 成員還用 rusty_v8 的 trampoline 的幾處改掉（cached getter、legacy caller、好幾個 constructor 的那一組的 `Call`），刪 `PLAIN`、它的檢查和成員舊的 callback（`Member<M>`、`bindings` 的 `StateMethod`） | 完成（`Front` 的預設實作留到 4m-3c） | 2026-11-13 |
| 4m-3a 新的 front 還交給 `V8Front` 的方法搬到只有 scope、參數、回傳值的 `Scoped` 上；讀 dictionary 成員的方法分成 `DictFront` | 完成 | 2026-11-13 |
| 4m-3b script 那一側留著自己的 front（決定 46）：改名 `WholeFront`，只有它實作 `ScriptFront`；`Kept` 刪掉 | 完成 | 2026-11-13 |
| 4m-3c `Front`、`OpFront` 會 panic 的預設實作在編譯時檢查 front 允不允許留空（`FrontTypes::STUBS`）；整個階段的檢查和對 Chrome 的收尾量測 | 完成 | 2026-11-13 |
| 收尾：`nodeType`、`getAttribute` 的 callback 和 Chrome 的逐條指令對照，列出剩下的差（決定 20） | 完成（`measurements.md`） | 2026-11-13 |

## 驗收

`dom_js` 的測試用 debug build 的計數（`bindings::info_scopes_for_testing`、`info_handles_for_testing`，`dom_js` 的 `wrappers_ahead`、`fresh_strings`、`cached_strings`、`string_conversions`）看一次呼叫建了幾個 scope、向 V8 要了幾次 handle。

| 項目 | 怎麼驗證 |
| --- | --- |
| **模型** | |
| V8 在操作裡配置的三條規則在模型上成立，各拿掉一條有反例 | `python3 rfcs/0097-binding-call-cost/tla/check.py` |
| **字串參數** | |
| 本來就是字串時不呼叫 `ToString`，其他的轉換一次、結果和例外照舊 | `cargo test -p dom_js --test it strings::a_string_argument_is_converted_only_if_it_isnt_a_string` |
| 不借字元的字串（cons string、Latin-1、漢字）當場複製，不建 scope，內容和長度正確 | `cargo test -p dom_js --test it info_front::a_plain_call_that_allocates_or_throws_makes_a_scope` |
| 節點或字串的參數：節點不建 scope，字串和要轉換的值照參數順序 | `info_front::nodes_or_strings_are_taken_as_they_are` |
| **只拿 callback info 的 front** | |
| 當場回傳的呼叫不建 scope；要配置、丟例外的建，結果、錯誤、setter 的值照舊；receiver 由 signature 檢查 | `cargo test -p dom_js --test it info_front::a_plain_`（四個測試） |
| 數字、布林、物件的參數當場讀；要轉換的（字串當數字、`valueOf`、超過 32 位元、不是 finite 的 `double`）結果一樣 | `info_front::numbers_booleans_and_objects_need_no_scope`、`info_front::numbers_booleans_and_objects_converted_or_made_make_a_scope` |
| callback、`any`、rest、`signal` 是參數的 handle；listener 照 callback 和 capture 移除 | `handlers::a_callback_is_its_arguments_handle`、`handlers::a_value_rest_arguments_and_a_signal_are_handles` |
| event handler 的 getter 在一個操作裡給 callback；原始碼第一次編譯、之後同一個函式 | `handlers::a_handlers_getter_gives_its_callback_in_the_operation` |
| static 和 overload 只拿 info：照參數選形式、`this` 是任何值、目前的 window 從 isolate 讀 | `handlers::static_operations_and_overloads_are_given_the_info_alone` |
| constructor 只拿 info：`new.target` 的 prototype、沒用 `new` 和參數不夠的 `TypeError`、新物件的 wrapper 有計數 | `handlers::a_constructor_is_given_the_info_alone`、`info_front::a_constructed_objects_wrapper_is_counted` |
| 操作這一側的方法每個 front 都有：abort 的 reason 原物件丟出、`ImageData` 的像素、custom element 的建構和 upgrade | `handlers::what_a_function_asks_of_the_operations_front_end_every_front_end_has` |
| indexed getter 的 interceptor：項不建 scope，過了最後一個是 `undefined`，`in`、`Object.keys`、唯讀照舊 | `info_front::an_indexed_getter_is_given_the_info_alone` |
| named 的 getter 和 setter 的 interceptor：照名字讀不建 scope，空字串是值，不收的名字變成一般屬性 | `info_front::a_named_getter_and_setter_are_given_the_info_alone` |
| **冷的路和 scope** | |
| engine 一定要配置的結果（陣列、frozen array、dictionary、record、promise、第一次讀才做的值）在操作後做 | `handlers::a_result_to_allocate_for_is_made_after_the_operation` |
| 操作前讀成 Rust 值的參數（`structuredClone`、`Blob`、`Headers`、keyframes、`getRandomValues`…）在一個 scope 裡讀，值正確 | `handlers::an_argument_read_whole_is_read_in_a_scope` |
| 物件的 handle（`view`、`relatedTarget`、`signal`、body、`ImageData` 的陣列）照原樣拿，型別不對是 `TypeError` | `handlers::an_objects_handle_is_taken_as_it_is` |
| 不是小整數的數字（分數、-0、NaN、超過 31 bits）不建 scope | `handlers::a_number_that_is_no_small_integer_is_returned_with_no_scope` |
| 現算的字串、只被呼叫持有的 `DomString`、常數、名稱：不建 scope、向 V8 要一次；現算的和常數不做 `DomString` | `strings::a_new_string_is_made_with_no_scope`、`strings::a_keyword_or_computed_text_is_given_as_it_is` |
| 只被這次呼叫持有的字串不進字串的快取；DOM 持有的只登記一次 | `strings::a_string_only_the_call_holds_isnt_cached` |
| callback 型別不對的 `TypeError` 寫的是第幾個參數 | `handlers::a_callback_that_isnt_one_is_named_by_its_position` |
| cross-origin 的成員、要找別的 realm 的 window 的、legacy factory、`document.all(0)`、`window.window`、`window.document` 只拿 info、當場建 scope | `handlers::a_member_that_needs_a_scope_is_given_the_info_and_makes_it`、`cross_origin::a_cross_origin_getters_object_is_its_receivers_realms` |
| blob 的讀取是 blob 自己的 realm 的 task | `frames::a_blob_read_is_a_task_of_the_blobs_realm` |
| **會跑 script 的成員** | |
| 整個呼叫在一個 scope 裡（`dispatchEvent` 一個、`click()` 一到兩個） | `info_front::a_member_that_runs_script_makes_one_scope` |
| 沒有 filter 的走訪（walker 的七個方法、iterator 兩個方向）在一個操作裡走完、不建 scope；有 filter 的呼叫 filter、走到同樣的節點 | `info_front::a_traversal_without_a_filter_walks_in_one_operation` |
| 第一次讀才做的值、有 filter 的走訪給的節點在操作後做 | `handlers::first_reads_and_a_filtered_walks_nodes_are_made_after_the_operation` |
| custom element 的 reaction 在成員回傳時跑 | `cargo test -p dom_js --test it custom_elements::` |
| **dictionary 和名稱** | |
| dictionary 成員的 key 每個名稱只做一次（之後從名稱的 V8 字串的快取拿）；getter 照名稱的順序各叫一次 | `strings::a_dictionary_members_key_is_made_once`、`strings::a_names_v8_string_is_made_once` |
| 成員名稱的清單和原始碼裡 dictionary 的成員一致、都是固定名稱 | `cargo test -p dom_api --test it members::the_listed_member_names_are_the_dictionaries` |
| 不是 local name 的固定名稱當元素的名字時照樣轉大寫（`<click>`） | `cargo test -p dom --test it names::a_fixed_name_that_is_no_local_name_is_uppercased_too` |
| `getAttributeNames` 沒有前綴的名稱用名稱自己的 V8 字串（和 `localName` 同一個） | `strings::attribute_names_are_the_names_own_strings` |
| 一個 dictionary 只建一個 scope | `handlers::a_constructor_is_given_the_info_alone` |
| **wrapper 和 unwrap** | |
| 建節點的成員用上預先建的 wrapper；interface 猜錯的浪費一個、結果照樣對 | `info_front::a_new_nodes_wrapper_is_made_ahead`、`info_front::a_wrapper_made_ahead_of_another_interface_is_made_again` |
| 同一個 realm 沒有 wrapper 的節點複製樣板物件、不建 scope；別的 interface 的第一個才建；屬性過了 full GC 還在 | `info_front::a_nodes_wrapper_is_copied_from_its_realms_boilerplate_with_no_scope` |
| unwrap 遮掉標記 bit、tag 只比一次：標記中的 wrapper 解得出來、型別分得出來 | `python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4kG.toml`（fork 的檔案，兩個 mutant） |
| 九種節點的 `nodeType` | `cargo test -p dom --test it kinds::every_kind_has_its_node_type` |
| **編譯期的保證** | |
| 成員走得到一個 front 沒實作的方法時 build 失敗（`FrontTypes::STUBS`；檢查在單態化之後，`cargo check` 看不到） | `cargo build -p dom_js`；試過拿掉 `InfoFront::doubles_arg`，失敗在 `<InfoFront as Front>::doubles_arg::{constant#0}` |
| 讀 dictionary 的程式只碰得到 `DictFront` 的方法；script 那一側的方法只有 `ScriptFront`（`WholeFront`）有 | 型別系統（`cargo build -p dom_api -p dom_js`） |
| 不在清單裡的成員名稱編不過（`MemberName::of` 的 const 求值） | 編譯器 |
| **對 Chrome 的量測** | |
| 每次呼叫的時間（63 項和 3 項對照）、指令數（9 項）、`nodeType` 和 `getAttribute` 的 callback 逐條指令 | `measurements.md`；`prototypes/chrome-compare.py`、`prototypes/chrome-instructions.py`、`prototypes/step-callback.py` |

沒有寫指令的列都是 `cargo test -p dom_js --test it <模組>::<名稱>`。

## 做過的檢查

都在上面那台機器上，日期見步驟表。

### 量法

- 指令數（cycle）：`bench.py calls`，或同一段 script 在兩個 binary 上各跑幾次取最小的 `perf stat`（4d 以前三次，4e 起七次：每個 binary 的空迴圈基準各量一次，三次取最小時基準自己有 ±10 條的誤差、整組一起偏）。cycle 的雜訊比指令數大，看指令數。
- 對 Chrome 的時間：`prototypes/chrome-compare.py`（Chrome 和 machopo 的 binary 各跑三次取最小、釘在一個核心上，ns 一次）。不釘核心、只跑一次時同一個呼叫差到兩倍（`nodeType` 10.1 和 4.5）。4k-C 起腳本拿掉 selection 的兩項（下面「沒查出原因的」）。
- 對 Chrome 的指令數：`prototypes/chrome-instructions.py`（兩千萬圈和六千萬圈的 `perf stat` 相減、除以四千萬、扣掉空迴圈）。第一次量錯：給 Chrome 的 script 把變數放在全域的 `let`，空迴圈一圈 113 條、`nodeType` 算出來是負的；兩邊包進同一個函式之後空迴圈是 8.0 和 5.4。
- 取樣在很短的函式上不能信：`perf record` 把 `nodeType` 的 88% 算在 V8 的 `CallApiCallbackOptimizedNoProfiling`、我們的 callback 10%，和數出來的 50 對 57 條不合；4e 找 `removeEventListener` 的退步時樣本和迴圈對齊、都落在同一條指令上。這一類要直接數機器碼，或比幾種寫法的 `perf stat`。

### 第 1 段：原型和量測（都量完還原，patch 在 `prototypes/`）

- 模型：`tla/check.py` 在 2×2×1 上七個版本都照預期（`details.md` 的「模型」）；`inop` 在 2×2×2 上成立。
- `nodeType` 原本的 callback 熱的路約 100 條指令（`bench.py calls` 的 153 條裡其餘是 V8 的 builtin 和 JS 的迴圈）：進出函式約 17、建 `CallbackScope`、`V8Front`、`Call` 約 26、借狀態約 5、unwrap 約 22、marking 2、`nodeType` 本身約 9、寫回傳值約 8、結束時的檢查約 12。
- 原型 1（getter 不建 scope、`V8Front`、`Call`；`raw-getter.patch`）：`nodeType` 151（30）→ 114（23），省 37 條、7 個 cycle；machopo 自己的部分（扣掉 V8 的地板約 11 cycle）約 19 → 約 12 cycle，Chrome 約 6。
- 原型 2（unwrap 不檢查；`unwrap-unchecked-*.patch`）：再到 89（19），再省 25 條、3 個 cycle（RFC 0085 決定 14 估 1 到 2）。查證後 Blink 照樣比 tag，所以修正成只拿掉 cage 的檢查：`nodeTypeRaw` 118（23）→ 113（22），只省 5 條、約 1 個 cycle；另外的 20 條是比 tag 和 null，Blink 也付。
- 「V8 在操作裡配置」會省到哪裡（沒改程式）：`createElement` 不會受惠（本來就預先建 wrapper）；回傳還沒有 wrapper 的節點每個約 1700 條、410 cycle，其中 machopo 的 `keep`、`finish_kept`、root 合計約 23%（約 390 條），其餘是 V8 建 wrapper 的成本、兩邊都付。Speedometer 的取樣裡 `finish_kept` 4.7 ms、`keep` 1.1 ms（每個 iteration）：上限約 5 到 6 ms。
- 字串的原型 1（已經是字串就不轉換；`string-already-a-string.patch`）：每個字串參數省約 57 條，`getAttribute` 749（162）→ 692（158）、`setAttribute` 1350（271）→ 1238（255）、`createElement` 2148（449）→ 2091（446）。
- 字串的原型 2（查名稱和值的表的上限；`last-string-upper-bound.patch`）：每查一次名稱約 80 條、20 多個 cycle，查一次值約 120 條（`getAttribute` −78 條、`setAttribute` −199、`createElement` −87）。原型 3（64 格的小快取，正確的版本；`recent-strings-cache.patch`）拿到上限的五到七成：每個名稱約 40 到 47 條、每個值約 95 條（−41、−139、−47）。
- 字串的原型 4（external string；`external-strings.patch`）：進來，長的名稱省很多（`getAttribute('data-long-name')` 949 → 663、`setAttribute` 兩個長字串 1624 → 1089、`createElement('blockquote')` 2295 → 2069），改不了的短字串每個多約 32 條（每次問一次 `CanMakeExternal`）；在沒有 sandbox 的 V8 上短字串改不成 external（要 8 個字元以上）。出去不複製到 1000 個字元都比較慢（10 個字元 3888 → 4297、1000 個 5234 → 6881，讀出來再比較 6529 → 13967），只有「讀出來的值原樣傳回去」省到（1346 → 1238）。V8 在 GC 的 worker 上放掉 external 字串的緩衝區（原型第一版在 destructor 檢查 thread、立刻 abort）。Chrome 實際拿到的（`getAttribute` 兩千萬次）：隨便取的字面值 1 到 8 個字元都約 20 ns，`'id'` 23.3、`'title'` 25.0（推測是 V8 內建的名稱、改不了）、執行時組出來的長字串 30.7：Chrome 在改得了的名稱上省約 3 到 5 ns。
- `dispatchEvent`（目標不在樹上、一個空的 listener，每次 3497 條）：DOM 的派發演算法約 46%、成員的 callback 13.7%、machopo 呼叫 listener 的那一側約 18%、V8 進 JS 約 14%、其他約 4%。最大的一塊是 DOM 的演算法（約 1600 條），不是 binding；「操作不能巢狀」的成本估（沒有量）一次派發約 200 到 350 條、6% 到 10%。blink-perf 的 `events/EventsDispatching` machopo 慢 1.24 倍。
- 第 3 段的原型（front 只拿 info；`info-only-front.patch`）：`nodeType` 153（31）→ 118（23）、`firstChild` 172 → 134、`hasAttribute` 571 → 505、`getAttribute` 661 → 560、`setAttribute` 1188 → 1095、`appendChild` 1317 → 1234；有參數的成員省 66 到 101 條、12 到 33 個 cycle。blink-perf 同一輪，慢幾倍：`node-type` 1.85 → 1.33、`first-child` 1.67 → 1.36、`get-attribute` 1.58 → 1.36、`set-attribute` 1.41 → 1.32。

### 第 2 到 4j 段：每段量到的（指令數（cycle），改之前 → 之後）

- **2**（字串參數不轉換）：`getAttribute` 726（156）→ 688（151）、`setAttribute` 1335（265）→ 1228（252）、`createElement` 2130 → 2087、`addEventListener` 898 → 880；每個字串參數約 40 到 55 條。
- **3**（`InfoFront`）：`PLAIN` 的成員省 33 到 95 條：`nodeType` 156（31）→ 116（24）、`firstChild` 172（34）→ 139（28）、`tagName` 308 → 244、`id` 318 → 252、`getAttribute` 661（152）→ 566（135）、`setAttribute` 1188（253）→ 1092（224）、`appendChild` 1317 → 1267；不是 `PLAIN` 的在 20 條以內。2799 個成員的路裡 1307 個是 `PLAIN`。blink-perf 對 Chrome for Testing（runs/s，慢幾倍，之前 → 這一段）：`node-type` 1.85 → 1.41、`first-child` 1.67 → 1.43、`get-attribute` 1.58 → 1.26、`set-attribute` 1.41 → 1.20、`id-getter` 1.69 → 1.25、`id-setter` 1.58 → 1.50；`append-child` 工具標成可疑、不比。Speedometer（使用者要求；數字在 RFC 0012 的 `progress.md`）：`main` 對第 3 段做完的 `6db8168` 各 3 次，20 個 suite 的幾何平均 −0.9%，中位數的合計 984.2 → 976.1 ms，多數的 95% 區間重疊。
- **4a**（結果帶出操作）：`nodeType` 115（24）→ 107（20）、`firstChild` 140（28）→ 127（22）、`parentNode` 139 → 124、`getAttribute` 565（133）→ 559（126）。最簡單的 getter 整個約 20 個 cycle（Chrome 約 17），扣掉 V8 的地板 machopo 自己約 9、Chrome 約 6。
- **4b**（數字、布林、物件）：`childNodes` 299（64）→ 219（42）、`classList` 269 → 190、`list.item(0)` 416（89）→ 260（50）、`event.target` 192 → 150、`toggleAttribute('hidden', true)` 819 → 737。`PLAIN` 的成員 1307 → 1726。
- **4c**（預先建的 wrapper）：`cloneNode(false)` 2279（509）→ 1897（384）、`cloneNode(true)` 4017（955）→ 3615（793）、`createTextNode` 2301 → 2223、`createElement('div')` 2056 → 1988。`cloneNode` 從三步變成一步。
- **4d**（callback 的 handle）：`t.onclick = f` 566（112）→ 500（96）、`t.onclick = null` 368 → 306、`removeEventListener('y', f)` 767（155）→ 590（126）。`PLAIN` 的成員 2152。
- **4e**（`any`、rest、handler 的 getter）：`x = t.onclick` 808（178）→ 280（50）、`x = t.oninput` 729 → 267、`ce.detail` 192 → 133、`addEventListener('x', f)`（已有同一個 listener）861（176）→ 634（128）、`n.append(c)` 2007 → 1955。`PLAIN` 的成員 2152 → 2581（能走新的 front 的 2745 個裡）。
- **4f**（回傳型別帶出操作）：`getAttributeNames()`（兩個屬性）1900（420）→ 1707（361）、`mo.takeRecords()` 907（171）→ 696（127）。`OpFront` 拿掉 21 個方法；`Kept` 30 種剩 9 種。`PLAIN` 的成員 2581 → 2646。
- **4g**（static、overload）：`ctx.setTransform(…)` 1309（271）→ 755（142）、`ctx.fill()` 2929 → 2632、`ctx.isPointInPath(1, 1)` 2979 → 2628、`URL.canParse(…)` 2305 → 2252。實際走新的 front 的成員 2575 → 2632。
- **4h**（constructor）：`new Event('x')` 2761（691）→ 2612（671）、`new Event('x', init)` 5489 → 5350、`new EventTarget()` 1877 → 1786、`ctx.putImageData(img, 0, 0)` 1531 → 1324、`addEventListener('x', f, opts3)`（三個成員）4718（922）→ 4338（798）。`OpFront` 39 → 34 個方法。走新的 front 的成員 2632 → 2678。
- **4i-1**（indexed getter）：`list[0]` 686（143）→ 440（84）、`children[0]` 761 → 515、`classList[0]` 791 → 521、`attributes[0]` 785 → 538。`list[0]` 還比 `list.item(0)` 多 183 條：V8 的 indexed interceptor 那條路本身，加上從 info 讀 isolate、holder、回傳值的三個 C++ 呼叫。2678 → 2682。
- **4i-2**（named）：`children.px` 1873（404）→ 834（155）、`attributes.id` 1848 → 814、`dataset.a` 1310 → 1042、`dataset.a = '1'` 1814 → 1576、`style.color`（空字串）4052（3293）→ 1544（338）。2682 → 2692。
- **4i-3**（現算的字串不進快取）：`style.color`（`'red'`）4712（2695）→ 2192（544）、`style.cssText` 13438（4594）→ 10901（2434）。
- **4j-1**（handle 取代編號）：`new MouseEvent('x')` 2689 → 2568、`new ImageData(u8, 2)` 2947（620）→ 2603（533）；帶 init 的 `new MouseEvent` 32216 → 32045，幾乎都在讀 dictionary。2692 → 2699。
- **4j-keys**（成員名稱是固定名稱）：`new MouseEvent('x', { clientX: 1 })` 32045（5890）→ 23451（4874）、`new KeyboardEvent('x', { key: 'a' })` 31500 → 23380、`new UIEvent('x', { detail: 1 })` 8227 → 6585、`new Event('x', init)` 5348 → 4612、`addEventListener('z', f, opts)` 4370 → 3338。剖析：V8 的 `Object::Get` 約 69%、把成員名稱做成 internalized 字串約 17%。
- **4j-2**（操作前讀成 Rust 值的參數）：換外殼省 0 到 150 條（`c.getHTML(options)` 3525 → 3381、`c.hidden = true` 969 → 895、`new Path2D('M0 0')` 4196 → 4079），成本在讀 JS 物件本身。2699 → 2736；不是 `PLAIN` 的剩 45 個。

### 第 4k 段：研究和各小段

- **零件的成本**（`call-pieces-probe.patch`）：一個空的 scope 27（5）、scope 加整個 `V8Front` 62（22）、讀的操作 20（4）、寫的操作 44（8）、登記再放掉一個 root 約 110（20）、回報 heap 22（3）。走到冷的路實際多花幾百到一千多條：`getAttribute(cons string)` +936、`setAttribute` 非 ASCII +516、數字轉字串 +286、不是整數的 double 約 +370、沒有 wrapper 的節點約 +1550、現算的字串約 +660。我們自己加的是 heap 配置（`Box<Kept>`、`String`、`DomString`）、整個 `V8Front`、root 和多開的操作，不是 scope。
- **對 Chrome 的第一份對照**（`chrome-compare.js`，ns）：落後的照比例排：現算的字串結果（`st.color` 45.7 對 112.2，2.46 倍）、會呼叫 script 的成員的外殼（`tw.parentNode()` 2.38 倍）、回傳非整數的數字（2.32 倍）、新的 wrapper（1.42 倍）、非 ASCII 的字串參數（1.32 倍）、帶 dictionary 的 constructor（1.26 倍）、最熱的路本身（`nodeType` 1.36、`getAttribute` 1.25、`parentNode` 1.20）。不是差距的：window 的 getter 當全域變數讀（每次約 1700 條，V8 的 `LoadSlow`，Chrome 一樣慢：`navigator` 0.93 倍）、`dispatchEvent`（0.64 到 0.75 倍）、長的串接字串（0.45 到 0.51 倍）。
- **會呼叫 script 的 33 個成員**（window 的 realm，指令數）：`dispatchEvent` 沒有 listener 1623、一個 listener 3596、在樹上 bubble 6798；`c.click()` 6412；`tw.parentNode()` 398（普通的 getter 128 到 130）；`form.reset()` 7156；`ac.abort()`（已經 abort 過）20526，每次建預設的 `AbortError`、抓 stack trace。
- **4k-A**（非整數的數字）：`ctx.lineWidth`（1.5）26.8 → 14.9 ns（Chrome 11.7：2.29 → 1.27 倍）；原型量的指令數 481 → 303。驗收的 1.2 倍沒有到：剩下的 1.27 倍就是整數那一條的 1.33 倍（最熱的路本身），這一段自己多出來的沒有了（非整數比整數多 7.3 ns，Chrome 多 6.0 ns）。
- **4k-B**（現算的字串）：`st.color` 113.5 → 81.8 ns（Chrome 47.4：2.39 → 1.73 倍；指令數 2192 → 1803）、`c.innerHTML` 2.73 → 2.21、`c.textContent` 2.30 → 1.58、`st.cssText` 3.41 → 3.19、`ctx.lineCap` 0.85 → 0.35。驗收的 1.2 倍沒有到：binding 自己的配置只剩約 2%，剩下的在 DOM 和 CSS 那一側（`style.color` 照名稱找 property 找了三次、合計約 19%；`innerHTML`、`cssText` 是序列化本身；`textContent` 先做成 `DomString`）。
- **4k-B2**（`DomStr::fresh`，45 處，其中 22 處是常數）：`document.readyState` 24.4 → 17.1 ns（1.33 → 0.93 倍）、`opt.value` 25.8 → 18.3（1.77 → 1.25）、`document.compatMode` 0.36 → 0.29、`opt.text` 0.51 → 0.42。
- **4k-C**（會呼叫 script 的成員的外殼和走訪的快路；Chrome 的倍數，4k-B2 → 這一組）：`tw.parentNode()` 1.61 → 0.86（15.8 → 8.4 ns）、設 `currentNode` 加 `nextNode()` 1.38 → 0.94、加 `firstChild()` 1.19 → 0.77、`NodeIterator` 來回 1.32 → 0.69；有 filter 的走訪 113.0 → 124.0 ns（0.59 → 0.65）；`dispatchEvent`、`click()`、`form.reset()` 沒變（0.44 到 1.01）。4k-C3 只剖析：`inp.select()`（1.28 倍）45% 在一個 `memcmp`、`Cx::input_value` 17%、`is_focusable_area` 12%，binding 的外殼約 21%；`new AbortController().abort()`（1.27 倍）分散在 V8 替 `DOMException` 抓 stack trace 和設屬性，Chrome 也要做。兩者是 DOM 那一側，不在這份處理。
- **4k-D**（新的 wrapper）：走過 20 萬個沒有 wrapper 的節點 62.5 → 59.5 ns 一個；Chrome 這次 44.0，先前幾次 54.5、55.5（1.08 到 1.35 倍之間）；再走一次兩邊都是 9.0。剖析：最大的是 V8 把指標登記進 sandbox 的表（9.2%，Chrome 一樣付）；我們的 `finish_node` 4.7%、外殼 4.3%、判斷 realm 約 5%。
- **4k-E**（不借字元的字串參數）：指令數 Latin-1 1591 → 1284、漢字 1594 → 1550、cons string 1831 → 1781；對 Chrome：Latin-1 74.7 → 52.4 ns（1.32 → 0.92 倍）、漢字 1.21 → 1.19（剩下的是 `ValueView` 和一次 malloc）。
- **4k-F**（dictionary、例外、陣列、promise）：machopo 比較快或持平：`new Event('x', init)` 0.82 倍、`new CustomEvent` 帶 `detail` 0.64、帶 options 的 add 加 remove listener 0.39、丟例外的三種 0.37 到 0.83、`getLineDash()`、`navigator.languages`、`querySelectorAll('p')`、`n.children`、`new DOMRect(…)`；`new MouseEvent('x', init)` 1.15。落後的：`n.matches('body')` 1.73（32% 在 style 那一側的一次 `memmove`，不是 binding）、`getAttributeNames()` 1.64、`classList.contains('a')` 1.20。只做 `getAttributeNames`：指令數兩個屬性 2526 → 1365、八個 7943 → 3282；154.3 → 84.0 ns（Chrome 92.0：1.68 → 0.91 倍）。
- **4k-G**（最熱的路；每次呼叫的指令數，Chrome / machopo）：`nodeType` 72.9 / 107.5、`parentNode` 93.0 / 135.6、`tagName` 211.2 / 241.6、`getAttribute` 623.7 / 564.5、`setAttribute` 1763.1 / 1085.6。最簡單的 getter 每次固定多約 30 到 40 條，DOM 的工作多的呼叫 machopo 反而少。tag 只比一次之後每個 DOM 呼叫少 5 條：`nodeType` 107.5 → 102.6（1.48 → 1.40 倍）、`parentNode` 135.6 → 130.5（1.46 → 1.38）、`getAttribute` 564.5 → 559.5（0.90）。`nodeType` 的 callback 走到的路 57 → 51 條，剩下和 Blink 不同的：cage 的檢查 5 條、讀型別的 tag 和檢查初始化 4 條加算出要比的值 2 條、標記的旗標 2 條。

### 第 4l、4m 段

- **4l**：沒有變慢（window 的 realm，前後多 1 到 26 條）：`location.href` 6663 → 6673、`window.postMessage('m', '*')` 9375 → 9401、`blob.text()` 4612 → 4629、`nodeType` 106 → 106。換到 cross-origin 的 16 個、要找別的 realm 的 window 的 5 個、legacy factory 2 個。
- **4m-2**：沒有量（動的是很少被呼叫的成員和刪程式）。刪掉 `PLAIN`（五個 trait 上一百六十多處）、`PLAIN_FRONT_METHODS`、`PLAIN_OP_METHODS`、`dom_api` 的 `plain` 測試和 table 的名單測試、`Member<M>`、`bindings` 的 `StateMethod`。
- **4m-3a**（4l 的 binary → 這一段）：`nodeType` 100.9 → 102.5、`removeEventListener` 帶 options 1199.5 → 1196.5、`appendChild` 丟例外 27000.6 → 26990.7、`new Event('x', {bubbles: true})` 4016.7 → 4010.7、`getClientRects()` 1867.1 → 1720.7。`V8Front` 的 85 個方法連同 7 個輔助函式搬到 `Scoped`，88 處 `with_front` 剩一處（custom element reactions）。
- **4m-3b、4m-3c**：指令數沒有變（`nodeType` 100.7 → 102.5、`appendChild` 丟例外 26991.2 → 26948.1、其餘差在 ±3 條以內）。script 那一側的 13 個方法分到 `ScriptFront`；115 個本體是 `unreachable!` 的預設實作加上 `const { assert!(Self::STUBS, ..) }`。
- **對 Chrome 的收尾量測**（整份在 `measurements.md`；修好字串參數的 store forwarding 之後重跑的）：時間的 63 項（另有 3 項對照）裡 machopo 比較快或一樣的 42 項、1.0 到 1.3 倍的 12 項、超過 1.3 倍的 9 項。最熱的 getter 仍落後：指令數 `nodeType` 66.9 / 102.5（1.53 倍）、`parentNode` 1.45、`firstChild` 1.50，時間 `nodeType` 3.4 / 4.6 ns（1.35）、`parentNode` 1.30。帶字串的追上了：`getAttribute` 20.1 / 15.6 ns（0.78；指令數 617.7 / 540.5）、`setAttribute` 56.6 / 35.5（0.63）。超過 1.3 倍的其餘是 DOM 那一側（`cssText` 3.18、`innerHTML` 2.25、`textContent` 1.60、`style.color = 'red'` 1.57、`matches` 1.38）和建 wrapper（沒有 wrapper 的走訪 1.51）。會跑 script 的多半較快（`dispatchEvent` 一個 listener 0.50、`click()` 0.48），`inp.select()` 1.28、`abort()` 1.21 仍落後。
- **逐條指令的對照**（收尾那一步；gdb 單步一次已經最佳化的呼叫，`prototypes/step-callback.py`，表在 `measurements.md`）：`nodeType` 的 callback 精確是 52 條（4k-G 時 57：tag 比一次省的 5 條），V8 呼叫 callback 的那一段約 50 條；Chrome 整次 66.9，照同樣的 50 條算 Blink 的 callback 約 17 條（這個假設沒有驗證：Chromium 的執行檔沒有符號，沒辦法照樣單步）。52 條裡進出函式 12、讀 tag 和檢查它初始化 4、解出 `this` 13、cage 的檢查 5、標記的旗標 2、算 `nodeType` 9、寫回傳值 5、讀 isolate 和狀態 2。`getAttribute('id')` 的 callback 和它呼叫的函式 508 條（整次 560.5，Chrome 617.7）：找屬性 166（其中 `names::find_fixed` 74）、解出 `this` 和借字元 105、看參數能不能原地借 77、回傳快取裡的字串 69（其中 rusty_v8 的 `assert_match_isolate` 9）、收尾 48、名稱轉小寫 43。取樣在這裡不能用：`perf record` 把 `nodeType` 的 90% 算在 V8 的 builtin 上；IBS 在這台機器的權限下開不了。
- **blink_perf 的 16 頁重量，找到並修掉建節點的頁的退步**（2026-11-13，收尾時；表和查的經過在 `measurements.md`）：4m-3 做完重量，getter 的頁從 Chrome 的 1.7 到 1.8 倍降到 1.3 到 1.45，但 `create-element` 1.43 → 1.61、`gc-forest` 1.58 → 1.68。對 `main` 直接比（`blink-perf --against`）確認是這份 RFC 造成的：`create-element` 慢 11.8%、`gc-forest` 慢 9.6%；GC 的時間沒變；照 commit 二分是逐段累積的（第 2 段和 4c 掉最多）。指令數沒有變多（每次 2063 → 2027），多的是 cycle（413 → 459）和 store forwarding 的衝突（5.9 → 8.1 次）。gdb 單步、比每個讀取和它前面的寫入的範圍，找到 callback 裡新增的三處「分開寫、整個搬」：字串參數的 `LentString`（handle 加兩個 `bool`，`lends` 透過參照寫旗標）兩處，`foresee_element` 回傳的 `(Option<NameId>, usize)` 一處。修法：`LentString::lending`（先檢查、通過才建）、旗標併成一個字、`foresee_element` 給新的 front 的版本回傳兩個字。修完同一個迴圈 415 cycle、衝突 3.2 次；對 `main`：`create-element` −0.2%、`gc-forest` 快 3.8%、`get-attribute` +52.7%、`set-attribute` +33.1%、`get-element-by-id` +56.2%、`node-type` +37.8%。對 Chrome：`get-attribute` 0.96、`set-attribute` 1.02、`get-element-by-id` 1.41、`create-element` 1.50、`gc-forest` 1.58。測試：`bindings` 的 `v8_text::a_string_taken_lending_is_scanned_once_and_lends_as_any`、`an_unguarded_isolates_string_isnt_taken_lending`。跑過：`cargo test -p bindings -p dom_js`、`sanity.py docs`。試過沒用的一個改法：讓預先建 wrapper 的 scope 只回傳 wrapper（衝突 9.2 → 9.3），還原。

### 估計和量到的不一樣的地方

- **一個 scope 值多少**：多建一個量到 17 到 20 條（`black_box` 高估）；讀機器碼之後在原本的程式裡只值約 8 條。3a 的第一版「scope 延後建」因此只淨省 2 到 4 條，有參數的成員反而多 40 到 130 條（`state::report` 不再 inline、`arg_string` 變長），沒有留下。原型 1 的 37 條是整個不經過 `V8Front` 和 `Call` 省的：那些結構因為冷的路拿著位址，編譯器只能每次都寫進記憶體。
- **4k 事先估「一個 scope 100 到 200 條」**：量到 27 條。會呼叫 script 的成員貴的是 root 和操作的個數。之後「確定都只有 27 條嗎」再量：27 條只是空的 scope，走到冷的路實際多花幾百到一千多條（上面）。
- **查名稱和值的表**：先前只看取樣裡分得出來的函式（1% 到 7%）就判斷「不值得做」，是錯的（雜湊和比對大多 inline 在 callback 裡）；原型量到每個名稱約 80 條、每個值約 120 條，比「不轉換」的 57 條還大。
- **4j-keys**：估每個成員省約 175 條，實際約 300 條（key 是 internalized 時 V8 查屬性前的處理也變便宜）。
- **4f**：說「一次做完 `Kept` 才能整個拿掉」不對，剩下的 9 種是 `V8Front` 自己在操作裡留的（4h 剩 7 種，4m-3b 才刪掉）；說可能套不進去的「操作前準備的 promise」套得進去（改成操作後才建）。
- **4k-B**：說 `dom_api` 裡「還有約 90 處」數多了：約 95 處裡約 30 處不是回傳值或是陣列的元素。
- **4k-C**：「呼叫 JS 不建 scope」沒有做（會跑 script 的路整段還在完整的 front 上、一個 scope）。說搬到新的外殼「效能預期不變」：跑到 script 的路慢了約 4 ns，有 filter 的走訪慢約 11 ns。
- **4k-G**：估「不影響安全性的寫法調整省 10 到 15 條」，實際 5 條。其餘三樣都省不到：`nodeType` 改成查表量出來 103 → 105（還原）；寫回傳值的 5 條是為了不讓 LLVM 縮成 32 bits 的 store（窄的 store 接寬的 load 不能 forward，先前量到 2.3 ns），V8 的 `SetInternal` 一樣；少存暫存器省不到（那 5 個 callee-saved 是為冷的路留的）。
- **4m-3a**：估「每次省約 35 條」不對：大多數的路差在 ±10 條以內，只有回傳物件陣列的少了 146 條。收穫是結構，不是指令數。
- **4m-3b**：決定 45 的第二項前提不對（實作 script 那一側的是「一個瘦的型別」）：`whole` 把整個成員的呼叫放到那個型別上跑，它要實作 `Front` 的全部和操作那一側、還要留著呼叫期間的東西，就是原本的 `V8Front`，瘦不下來；重選成決定 46。
- **4m-3c**：預設實作沒有照決定 45 刪掉，改成編譯時檢查（`STUBS`），效果一樣：四個只綁一部分成員的 front 各缺八十到一百個方法，補的 macro 要把每個簽名再寫一次。
- **wrapper 被收掉的疑慮收回**：4k 一度量到「第二次走同一批節點和第一次一樣貴（1673）」，是用三個不同長度的 script 的指令數相減量的，方法有問題；在 window 的 realm 裡第二趟是 9 ns，和 Chrome 一樣。
- **4k-D 的 `parentNode` 多 0.4 ns**：指令數是 136，改兩種寫法都還是 136；同一個 binary 不同次 5.4 到 5.9，當成雜訊。
- **用指令數驗收漏掉的成本**：每一段都用指令數（和少數幾個呼叫的時間）驗收，`createElement` 的指令數一路變少，但它每次多了約 2 次 store forwarding 的衝突、整頁慢了 12%，到收尾重量 blink_perf 才看到。`bench.py calls` 一直有印衝突數（開始時 4.66、4m-3 之後 8.16），沒有拿來看。

### 找到的 bug 和修掉的退步

- rusty_v8 的 `FunctionCallbackArguments::get_isolate` 把 V8 的 isolate 指標轉成 `*mut Isolate` 來用，而 `Isolate` 是包著指標的結構：一用就 segfault。fork 裡沒有人呼叫它；fork 加了正確的入口（`FunctionCallbackInfo::isolate`），不對上游回報。
- `n.style[0]`（沒有宣告）是空字串而不是 `undefined`（4i-1 的新測試）：`CSSStyleDeclaration.item` 過了最後一個給名稱表的空字串、經過 `ret_name`，沒記成「沒有這一項」；原有的 467 個測試沒有一個看這個。
- `<click>` 的 `tagName` 是 `click`（原有的問題，4j-keys 修掉）：`UPPERCASE` 表只留 local name 的那一段，其餘的固定名稱當元素的名字時走一般名稱的路轉大寫。
- 字串的快取照 `DomString` 的位址記，現算的字串每次位址不同、永遠找不到，於是每次新建 V8 的字串並登記一筆（`Weak` 加 finalizer）：`style.color` 的 cycle 異常的原因（4i-2 找到，空字串先繞過，4i-3 修掉）。
- 成員被誤放行到新的 front、呼叫到它沒有的方法而 abort，一共四次（決定 35 列了前三次）：`getImageData`（`canvas::images_and_origin_clean`）、`putImageData`（`with_array_bytes`）、`Request`、`Response` 讀 body 的十個成員（`rethrow`）、`new ImageData(1, 1)`（`canvas::image_data`；原始碼掃描沒涵蓋 constructor）。第 3 段那次掃型別還漏了帶 turbofish 的呼叫。最後由 4h 之前那一段（刪掉或實作 `OpFront` 的方法）和 4m-3c 的編譯時檢查收掉。
- custom element 的 reaction 預設的 `invoke_ce_reactions` 把 front 當 script host 用，`InfoFront` 沒有那些方法（`custom_elements` 的測試抓到）。
- 修掉的退步：`InfoFront` 的 drop glue 每次呼叫是一次函式呼叫（改成 `ManuallyDrop`；先試的 `mem::forget(self)` 搬動整個結構，getter 從 23 變成 40 個 cycle；4a 之後整格拿掉）；`DomStr::give` 沒 inline 時 `getAttribute` 多約 40 條；`TargetObject::this` 在 4e 多一個使用者之後 LLVM 不再 inline，`removeEventListener` 595 → 629，改成 `#[inline(always)]` 回到 595；新的 front 讀 dictionary 時每個成員各建一個 scope（`new Event('x', init)` 5475 → 5616），改成整個 dictionary 一個（`ReadDict<T>`、`Front::reading`；先試的關聯型別 `Reader<'r>` lifetime 寫不出來）；Latin-1 第一版逐字元轉，1591 → 2300 條，換成 simdutf 才是 1284。
- 計數原本寫成 thread 的 `static`，`sanity.py boundary` 擋下來，改放進狀態（`string_conversions`）；測試用的 `INFO_SCOPES`、`INFO_HANDLES` 登記進 `boundary` 的允許清單。
- 階段收尾時 clippy 抓到四處，一起修了：`dom_api_macros` 的 `rewrite_fronts_types` 的索引運算、`bindings` 的 named setter 的 trampoline 兩個 `unsafe` 共用一個 `SAFETY`、`dom_api::resolve_overload` 的迴圈、`canvas::get_image_data` 多的 `#[expect]`。

### 不穩定的測試和沒查出原因的

- **不穩定的測試（找到、修掉）**：4b 時 `info_front` 的「當場回傳的呼叫不建 scope」有幾條靠 V8 還留著先前做的字串或 wrapper，但測試沒有拿著它們：scavenge 丟掉之後下一次讀就重做、建 scope，`n.firstChild.tagName` 那一條失敗一次。改成測試把每個結果留在 JS 的陣列或變數裡。修完 `info_front` 的 7 個測試重複跑 1400 次都通過；4c 的 9 個重複跑 1000 次、4d 的 `info_front` 加 `handlers` 的新測試重複跑 500 次都通過。
- **沒查出原因的**：整份對照的 script 裡 `sel.selectAllChildren(n)` 在 machopo 是 62156 ns 一次（Chrome 223.5）；單獨跑或接在各種其他呼叫後面都是 140 到 300 ns，沒重現。`chrome-compare.js` 拿掉了 selection 的兩項。`inp.select()` 的 `memcmp` 是誰呼叫的沒查出來（帶 call graph 的取樣在這台機器上寫不出檔）。`'class'` 在 Chrome 17.6 ns 比其他名稱都快，原因沒查。
- 看到、沒有動的：window 的 document 的 `cloneNode()` 給的是 `Document`、不是 `HTMLDocument`（改之前就是；沒有查 Chrome）。
- 寫測試時常弄錯的（測試環境的事實）：沒有 window 的環境裡 handler 和 callback 不運作，要在 window 的 realm 測；沒有 raster，`fillRect` 之後 `getImageData` 是零、`toDataURL` 是 `data:,`；method 呼叫完回報 heap 成長的那個 scope 不是成員自己的（先呼叫一次再數）；4k-B 起 `scopes` 數的是 scope 加 `handles` 的次數，只數 scope 用 `only_scopes`。啟動 mutation test 前先在前景跑一次測試（4g 時基準就失敗、輸出在背景裡沒看到）。

### Mutation test

都用 `python3 scripts/test.py mutate rfcs/0097-binding-call-cost/<spec>` 跑，跑完檔案都還原。fork 的入口（`as_string`、`as_smi`、`isolate`、`set_traced`）不在這個 repo、沒有 mutant：debug build 各自和 V8 原本的呼叫比對（`debug_assert`）。

| spec | mutant | 抓到 | 沒抓到的、補的 |
| --- | ---: | ---: | --- |
| `mutants-2.toml` | 3 | 3 | 第一次「轉換丟了例外還繼續」沒抓到：補「成員沒有被呼叫」的斷言 |
| `mutants-3.toml` | 26 | 23 | 3 個當時等價：跨 origin 的 `PLAIN` 成員不在 receiver 的 realm 呼叫（只有 `closed`、`length`，不建 wrapper）；拿 front 的 setter 也算 `PLAIN`；負零當成零（沒有 getter 給得出負零）。另一個「失敗的呼叫留下的結果被做完」走不到，改成 `debug_assert`、拿掉 |
| `mutants-4a.toml` | 11 | 11 | |
| `mutants-4b.toml` | 14 | 14 | 沒有寫的：`target_arg`、`is_null_or_undefined`（沒有測試呼叫到的成員）、`templates.rs` 限定 kind 的條件（等價） |
| `mutants-4c.toml` | 11 | 11 | |
| `mutants-4d.toml` | 12 | 12 | |
| `mutants-4d-plain.toml` | 7 | 7 | `PLAIN` 的檢查的；第七個第一次寫成改名字、編不過，改成刪掉方法。檢查本身 4m-2 刪掉了 |
| `mutants-4e.toml` | 26 | 26 | 第一次 25 個：「省略的 unsigned 整數交給整個 front」，測試用的 `selection.collapse(d)` 不走新的 front，換成 `sheet.insertRule('b {}')` |
| `mutants-4f.toml` | 23 | 23 | 沒有寫的：把 `FilledView` 標成 `PLAIN`（等價） |
| `mutants-4g.toml` | 10 | 10 | |
| `mutants-4h.toml` | 22 | 22 | 第一次 20 個裡 19 個：「constructor 建的物件不計入 wrapper 的計數」，補 `a_constructed_objects_wrapper_is_counted` |
| `mutants-4i.toml` | 8 | 7 | 「`ret_string` 的空字串不記成沒有這一項」等價：表上沒有 indexed getter 經過 `ret_string` 給空字串；檢查留著 |
| `mutants-4i2.toml` | 9 | 9 | 第一次 5 個：四個是測試分辨不出來（量的那幾行後面接了別的會建 scope 的呼叫），改成只量 getter 或 setter 自己的那一次，加「字串的值、已有的名字不建 scope」 |
| `mutants-4i3.toml` | 4 | 4 | |
| `mutants-4j1.toml` | 15 | 15 | 第一次「null 的 signal 當成沒給」沒抓到：補「input 有 signal、init 的 signal 是 null 時不跟」 |
| `mutants-4j-keys.toml` | 5 | 5 | 「清單少一個名稱」編不過，不列；「名稱的字串不是 internalized」只是慢，記在 `docs/test-gaps.md` 的「沒有計數守著的優化」 |
| `mutants-4j2.toml` | 10 | 10 | |
| `mutants-4kA.toml` | 3 | 3 | |
| `mutants-4kB.toml` | 8 | 8 | 第一次 7 個：「現算的文字又做成 `DomString`」只多一次配置，加 `fresh_strings` 的計數 |
| `mutants-4kB2.toml` | 4 | 4 | |
| `mutants-4kC1.toml` | 7 | 7 | 第一次沒跑：一個 mutant 的原文和格式化過的程式對不上，工具在改檔案之前就停了 |
| `mutants-4kD.toml` | 6 | 6 | 兩個跨 realm 的 mutant 由原有的測試抓到 |
| `mutants-4kE.toml` | 3 | 3 | 「Latin-1 不走原地讀的路」只是慢，沒有列（`docs/test-gaps.md`） |
| `mutants-4kF.toml` | 4 | 4 | |
| `mutants-4kG.toml` | 2 | 2 | 改的是 fork 的檔案，spec 用相對路徑 |
| `mutants-4l.toml` | 2 | 2 | 第三個「cross-origin 的 getter 不進 receiver 的 realm」補了測試還是沒抓到：wrapper 本來就建在物件自己的 realm，等價，從 spec 拿掉、在 spec 裡註明 |
| `mutants-4m2.toml` | 6 | 6 | 第一次「relevant realm 的 window 換成目前的」沒抓到：補 `a_blob_read_is_a_task_of_the_blobs_realm` |
| `mutants-4m3a.toml` | 5 | 5 | 第一次「callback 的錯誤寫成 index」沒抓到：補 `a_callback_that_isnt_one_is_named_by_its_position` |
| `mutants-4m3b.toml` | 3 | 3 | 另外三個（`WholeFront` 操作後做字串、沒有 wrapper 的節點、不是 Smi 的數字）走不到：現在在 `WholeFront` 上跑的成員沒有一個給這三種結果。從 spec 拿掉、註明，記進 `docs/test-gaps.md` |
| `mutants-stlf.toml` | 3 | 3 | 另外兩個等價，在 spec 裡註明：「標成 ASCII 時忘了借出過」（ASCII 一定在第一次借出之前就標好）、「預先猜的元素名稱沒有編號」（編號只省 `createElement` 再查一次名稱） |

4k-C 留下的小改（會呼叫 script、沒有參數的成員登記 `this` 的 root 用讀的操作）結果一樣、沒有測試分辨得出來，記在 `docs/test-gaps.md` 的「沒有計數守著的優化」。

### 沒有做、延後、還原的

- external string（進來、出去）：原型量過（上面），沒有做；短的固定名稱要 V8 的 sandbox。名稱和值的小快取也只做了原型。
- 拿掉 unwrap 的 cage 檢查（5 條）：決定留著；不比 tag 要補 soundness 的論證，現在不拿掉。`nodeType` 改查表：多 2 條，還原。
- 3a 的「scope 延後建」：淨省 2 到 4 條，沒有留下（存成 `prototypes/lazy-scope-*.patch`）。`firstChildRaw` 的原型沒做。
- 4d 寫了又拿掉的（當時沒有成員走得到、沒有測試守得住）：`InfoOp` 的 `ret_any` 三個、`with_array_bytes`、`InfoNoGc::strong_value`、fork 的 `TracedSlot::get_unscoped`；用得到的那一段再加回來。4h 先加的 `Unplain<A>` 沒有留下。
- 4k-B、4k-B2 沒改的：`dom` 自己回 `DomString` 的結果（`textContent` 的 `cx.text_content`、`a.text`），要動 `dom` 的函式簽名。
- 4k-D 沒改的：節點以外的物件第一次拿 wrapper 還經過完整的路；節點的 root 還在；判斷 realm 那一步可以更便宜，但多個 realm 並存時的規則要先想清楚。
- 4k-F 其餘的不做（使用者）：例外、promise、dictionary、其他陣列已經比 Chrome 快。
- `HTMLElement` 的 constructor 每次建一個 scope 檢查 `new.target`：保留的成本。
- 還用 rusty_v8 的 trampoline（給 scope）的：iterator 的 pairs（`pairs.rs` 三個）、4i 留下的 interceptor（indexed setter、named 的刪除、查詢、列舉）、不是成員的內部函式（`state.rs` 三個）。`Indexed<M>` 舊的 `StateIndexedGetter::get` 沒有人呼叫了，但 `bindings` 的 indexed 的 query 和 enumerator 還要這個 trait，沒動。
- 不是 binding、要另外開題目的：`style.color` 照名稱找 property 三次、`innerHTML` 和 `cssText` 的序列化、`matches` 的 `memmove`、`inp.select()`、`dispatchEvent` 每次推一筆 `PerformanceEntry` 和一個 trace span、window 的 getter 當全域變數讀（V8 的 `LoadSlow`，有 cached accessor 這條出路；記在 README 的「之後的可能」）。
- 剩下最熱的路上可以拿掉的（`measurements.md`）：進出函式存 5 個 callee-saved 暫存器（估省 8 到 10 條，沒做過）、型別的 tag 變成編譯期的常數、`find_fixed` 的 74 條（決定 20）、rusty_v8 的 `assert_match_isolate` 9 條。

### 跑過的檢查

- 每一段：改到的 crate 的 `cargo test`（`bindings`、`dom`、`dom_api_macros`、`dom_api`、`dom_js`、`dom_wasm` 裡動到的；第 2 段時 `dom_js` 448 個通過，4f 時 463 個）、`sanity.py` 七項（`deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`）、`cargo fmt --check`，動到 fork 的段落加 `ci.py third-party`。
- 每一段的 `test.py wpt`：跑和改動相關的目錄，結果都照預期（失敗的 reftest 都是本來就預期失敗的）。從第 2 段的兩個檔案（216 個結果）、第 3 段的 `dom/nodes`（15617 個）到 4b 到 4i 的 `dom html/dom` 加各段的目錄（123056 到 148537 個）；4j 起照改到的 API 選（4j-1 4505、4j-keys 19302、4j-2 63646、4k-A 3359、4k-B 90530、4k-B2 108240、4k-C 44859、4k-D 81802、4k-E 77128、4k-F 75502、4l 4980、4m-2 18238、4m-3a 78178 個）。4k-G 只動 fork 的幾行和一個新的測試，沒有另外跑 WPT。
- 第 2、3 段做完和 4d（提前跑，為了找被誤放行的成員）：`cargo test --workspace`（55 個 binary 通過）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`。
- **整個階段做完（2026-11-13，4m-3c）**：`cargo test --workspace`（55 個 binary 都過）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（抓到的四處見上面）、`sanity.py` 七項、`test.py scenarios`（86 個都過）、`test.py wpt dom/events dom/nodes dom/traversal dom/abort html/dom custom-elements html/browsers/the-window-object html/semantics/forms/the-input-element selection`（1223 個測試、118337 個結果都照預期）。
- **收尾的修正之後再跑一次（2026-11-13）**：`cargo test --workspace`（55 個 binary 都過）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 七項、`test.py scenarios`（86 個都過）、同樣九個目錄的 `test.py wpt`（1223 個測試、118337 個結果都照預期）。
- fork（rusty_v8，都只改 Rust、不用重編 V8）：`2be356e`、`097b54c`、`5c327d3`、`b70c0fa`、`36b85bd`、`5b1b5ae`、`cc74cef`、`6dbddfd`、`b718995`、`e114259`、`2816d5d`，每個都在同一次改動裡更新 `third_party.toml` 的 `rev`、記進 `README.machopo.md`。都 push 到 fork 的 `machopo` 分支了（2026-11-13，到 `2816d5d`）。
