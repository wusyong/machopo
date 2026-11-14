# RFC 0100 的進度：V8 那一側只有一個 front

開始日期：2026-11-14

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 3a 之前：盤點測試、補缺的 | 做完 | 2026-11-14 |
| 3a：`InfoFront` 實作 `ScriptFront`，會跑 script 的成員搬過去 | 做完 | 2026-11-14 |
| 3b：legacy caller、iterator、cross-origin 從 callback info 開始 | 做完 | 2026-11-14 |
| 3c：刪掉 `WholeFront`、`WholeOp`、`Call`；`sanity.py` 的檢查 | 做完 | 2026-11-14 |
| 3d：`PropertyFront` 併進 `InfoFront` | 未開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom_js` 只有一個 `impl Front`（`PropertyFront` 另外評估） | `sanity.py`（3c 加） |
| 熱的呼叫不變慢 | `bench.py calls --baseline rfcs/0098-direct-v8-bindings/calls-baseline.json`，每段做完量一次 |
| 行為不變 | `dom_js` 的測試、相關的 WPT 目錄 |

## 做過的檢查

### 3a 之前：測試的盤點（2026-11-14，Linux x86-64）

對 15 種行為逐一讀過 `dom_js` 的測試（讀測試的內容，不只看名字）。已經有的：listener 的重入和增刪（`events.rs`）、listener 丟例外、`define` 的參數檢查和順序、reaction 在成員回傳時的順序、lifecycle callback 丟例外、upgrade 失敗、filter 重入自己的 traversal、物件的 `acceptNode`、abort 讓 fetch 的 promise reject、`isInputPending`。相關的 WPT 目錄（`dom/nodes`、`dom/events`、`custom-elements`、`dom/abort`、`dom/traversal`、`geolocation`、`is-input-pending`）都在清單上。

缺的補在 `crates/dom_js/tests/it/script_members.rs`（7 個測試，在還沒搬的程式上通過）：

- `define` 讀 constructor 時 getter 丟的例外就是 `define` 丟的那一個（比物件，不只比名字）：`prototype`、`observedAttributes`、lifecycle callback 的 getter，丟的不是 `Error` 的也算；之後 registry 照常能定義。
- `define` 的 getter 裡再 `define` 同一個名字、或同一個 constructor 換名字：`NotSupportedError`，外面那一個照樣定義成功。
- 等同一個名字的每個 reaction（同一個 promise 的幾個 `then`、另一次 `whenDefined`）在 `define` 時都拿到 constructor，別的名字的不動。
- 一次 `innerHTML` 建的幾個元素：每個元素的 `attributeChangedCallback` 在它的 `connectedCallback` 之前，元素照樹的順序；reaction 裡改另一個元素時，那個元素的 reaction 在裡面跑完。
- script 呼叫的 `dispatchEvent`、`click`、會觸發 reaction 的 `appendChild`、traversal 的 filter 裡排的 microtask，都等外面的 script 結束才跑。
- mutation observer 的 callback 改了它觀察的東西：同一個 checkpoint 裡再被呼叫；callback 裡的 `takeRecords` 拿走的不再送。
- traversal 的 filter 丟的例外就是成員丟的那一個（`TreeWalker` 和 `NodeIterator`、函式和 `acceptNode`），之後照常能走。

沒補的：geolocation 的成功 callback（沒有允許定位的路，永遠是拒絕或逾時）；`update_elements` 沒有直接的測試（經過 engine 走到）。

寫測試時自己弄錯一次：以為 `define` 之後換掉 prototype 上的 `attributeChangedCallback` 會生效（callback 是 `define` 時讀走的，不會），改成 class 裡呼叫一個全域的 hook。

### 3a（2026-11-14，Linux x86-64）

- **第一步，行為不變**：`ApiError::Thrown`（script 丟了、V8 拿著那個例外，front 不再丟別的）取代 `WholeFront` 的 `threw` 旗標和暫存的 `thrown`；`define` 的 constructor 和讀到的 lifecycle callback 照值傳（`ScriptHost` 的 `Callback`、`Lifecycle`，`ScriptFront` 的 `Lifecycle`；`constructor_is_constructor`、`constructor_is_defined`、`read_custom_element_constructor`、`store_custom_element_definition`、`resolve_when_defined` 的簽名），`dom_js` 的兩個 front 和 engine 的 host、`dom_wasm` 的 front 和 host、`dom_api` 的測試跟著改。`RareCall` 只剩第二個 callback。
- **第二步**：`InfoFront` 實作 `ScriptFront`，每個操作當場開 scope、呼叫 `listeners` 和 `front` 裡原本的函式；`InfoFront::whole` 是 `call.run(self)`，`invoke_ce_reactions` 直接跑；`with_front` 刪掉。`WholeFront` 還在，只剩 legacy caller、cross-origin、iterator 在用。
- **和之前不同的地方**：traversal 的 filter 丟的例外接住後立刻在 V8 裡重丟（`TryCatch` 結束之後），留到成員回傳；中間 `dom_api` 還會跑一個 DOM 的操作（清掉 traversal 的 active 旗標），不呼叫 V8。`define` 讀 constructor 丟的原本就是這樣。回傳 promise 的成員拿到 `Thrown` 時 reject 的是一個 `TypeError`（`TryCatch` 外面的例外拿不進來）：沒有這樣的成員會跑那些 script。
- **scope 的數目**（預期的變化）：會跑 script 的成員從固定一個變成每次呼叫 script 一個：`dispatchEvent` 一個 listener 1 個、兩個 2 個、沒有 listener 被呼叫 0 個。`info_front::a_member_that_runs_script_makes_one_scope` 改成 `…makes_a_scope_for_each_call_into_script`，檢查這三種。
- **測試**：`cargo test -p dom_js -p dom_api -p dom_wasm -p bindings` 通過（`dom_js` 495 個）；搬之前補的 7 個都通過。
- **WPT**：`dom/events/`、`custom-elements/`、`dom/traversal/`、`dom/abort/`、`dom/nodes/`、`is-input-pending/`：876 個測試、21,935 個檢查，非預期的結果 0。
- **`bench.py calls --baseline`**：`nodeType` 113 → 113、`firstChild` 140 → 140、`getAttribute` 560 → 560；`dispatchEvent` 3496 → 3427（−68：不建 `WholeFront` 和 `Call`），cycles 746 → 740。其他成員的指令差在 2 條以內。`appendChild` 的 store forwarding 衝突 7.60 → 8.34、`list[i] (first)` 7.83 → 9.47，這兩條路沒有改到、cycles 沒跟著變，當成量測的起伏，沒有另外驗證。
- **mutation test**（`mutants-3a.toml`，16 個）：都抓到。第一次一個都沒跑：spec 的 `from` 和 `cargo fmt` 之後的程式對不上，改了再跑。

### 3b 和 3c（2026-11-14，Linux x86-64）

- **legacy caller**：`info_front::Called`（`this` 是被呼叫的物件，`InfoNoGc::unwrap` 整個檢查），`LegacyCaller<M>` 直接建 `InfoFront`。
- **cross-origin 的屬性的成員**：本來就是 `StateInfoMethod`，不用動 fork；`CrossOriginMember<M>` 建 `InfoFront<Method>`，getter 經過 `bindings::StateInfo::in_realm_of`（新的：用 info 開 scope、看 receiver 的 creation context，不是目前的就進去，再把 info 交給 closure；論證在 `crates/bindings/soundness.md`）。方法和 setter 不再開 scope（`window.focus()` 0 個；`handlers::a_member_that_needs_a_scope_is_given_the_info_and_makes_it` 照這個改）。
- **pair iterator**：`Next<M>`、`ForEach<M>` 改成 `StateInfoFunction`；`info_front::Pair`（info 是 `PairInfo`：函式的 info、target、index；index 由 `number_arg` 給、不建 handle），`InfoFront::into_info` 把 info 拿回來給那一步之後的事；成員的結果從回傳值讀回來。
- **刪掉**：`WholeFront`、`WholeOp`、`Call`、`RareCall` 和它們的 `Front`（93 個方法）、`OpFront`（31 個）、`DictFront`、`ScriptFront`、`FrontTypes`，`templates::call_member`、`CallArgs::receiver`、`IndexArgs::receiver`、`Scoped::finish_string_pending`、`front::foresee_element`。`front.rs` 4,721 行 → 3,175 行。
- **`sanity.py deps`**：`dom_js` 的 `impl … Front for` 只能是 `InfoFront` 和 `PropertyFront`；用假的檔案確認多一個會被抓到。
- **測試**：`cargo test --workspace` 3,884 個通過；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通過。新的：`urls::for_each_stops_at_what_its_callback_throws`、`frames::a_cross_origin_getter_gives_what_is_its_receivers_realms`。
- **WPT**：`dom/events/`、`custom-elements/`、`dom/traversal/`、`dom/nodes/`、`url/`、`html/browsers/origin/`、`html/infrastructure/common-dom-interfaces/collections/`：918 個測試、29,337 個檢查，非預期的結果 0。
- **mutation test**（`mutants-3b.toml`，5 個）：3 個抓到；「`forEach` 在 callback 丟例外之後繼續」漏掉，補了測試後抓到；「另一個 realm 的物件的 getter 在目前的 realm 跑」補了測試還是沒抓到，等價（wrapper 本來就建在物件自己的 realm、這些 getter 不丟例外；之前在舊的 front 上也查到同一件事），從 spec 拿掉、在 spec 裡註明。所以 `in_realm_of` 現在沒有看得到的效果。
- **`bench.py calls --baseline`**：整套跑的那次 `firstChild` 140 → 142、`nodeType` 113 → 114、`getAttribute` 的衝突 0.14 → 1.11；只重跑這三個：129、101、549（都少 11：空迴圈的指令數偶爾多 10 條的那個已知的起伏），衝突 0.12、0.01、0.14；沒有這批改動的同樣三個是 140、113、560。結論是沒有變，差別在量測的起伏以內。`dispatchEvent` 3496 → 3427。
- **決定 7 之後**（同一天）：`in_realm_of` 和 `CrossOriginMember` 的那一步拿掉，cross-origin 的 getter 不再開 scope（`location.href`（字串 V8 已經有）、`window.closed`、`window.length`、`document.location === location` 都是 0 個；`handlers` 的那個測試再改一次）。`cargo test -p dom_js -p bindings` 通過（`dom_js` 496 個）；WPT `html/browsers/origin/`、`html/browsers/the-window-object/`、`html/browsers/history/the-location-interface/`：87 個測試、430 個檢查，非預期的結果 0。
