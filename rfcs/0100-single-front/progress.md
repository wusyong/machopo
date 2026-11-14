# RFC 0100 的進度：V8 那一側只有一個 front

開始日期：2026-11-14

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 3a 之前：盤點測試、補缺的 | 做完 | 2026-11-14 |
| 3a：`InfoFront` 實作 `ScriptFront`，會跑 script 的成員搬過去 | 未開始 | |
| 3b：legacy caller、iterator、cross-origin 從 callback info 開始 | 未開始 | |
| 3c：刪掉 `WholeFront`、`WholeOp`、`Call`；`sanity.py` 的檢查；評估 `PropertyFront` | 未開始 | |

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
