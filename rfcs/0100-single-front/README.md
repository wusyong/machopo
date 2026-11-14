# RFC 0100：V8 那一側只有一個 front

- 開始日期：2026-11-14
- 狀態：🚧 已排程：2026-11-14
- 細節：無

## 摘要

`dom_js` 現在有兩個完整的 front：只拿 V8 的 callback info 的 `InfoFront`（每個成員從它開始），和拿著一個 scope 的 `WholeFront`（會跑 script 的成員、cross-origin 的屬性、legacy caller、iterator 走它）。兩者各自實作同一套 `Front`、`OpFront`、`DictFront`。這份把會跑 script 的呼叫也放到 `InfoFront` 上，其他入口改成從 callback info 開始，然後刪掉 `WholeFront`、`WholeOp` 和 `Call`。這是 RFC 0098 路線圖的 M3；目的是一致性和少一份程式，不是速度。

## 動機

- **同一件事寫了兩次**：

  | | `InfoFront` | `WholeFront` |
  | --- | ---: | ---: |
  | `Front` | 94 個方法、860 行 | 93 個方法、680 行 |
  | `OpFront` | 32 個、368 行 | 31 個、339 行 |
  | `DictFront` | 16 個、133 行 | 16 個、161 行 |

  加一種參數或回傳的型別要改兩處，行為要靠測試保證兩邊一樣。
- **走 `WholeFront` 的入口**：會跑 script 的成員（`InfoFront::whole` 開一個 scope、建一個 `WholeFront`，成員剩下的部分在它上面跑）、legacy caller（`document.all(..)`）、cross-origin 的屬性（access check 的 interceptor，兩處）、iterator 的 `next`（`pairs.rs`）。另外 `named.rs` 的 `PropertyFront` 是第三個、只實作一部分的 front（12 個方法）。
- 對網頁沒有看得到的變化。

## 設計概要

對改 binding 的人：`Front` 只有一個實作。會跑 script 的操作（呼叫 listener、custom element 的 callback、`NodeFilter`……，`ScriptFront` 的 13 個方法）是 `InfoFront` 的方法，要用到 V8 會配置或跑 script 的地方時當場開 scope（和它其他的慢路一樣）。

## 詳細設計

每一段的設計在那一段開始時寫。先記現在的結構：

- `Front::whole(call)`：`InfoFront` 的實作開一個 scope，`call.run(&mut WholeFront::new(scope, &args, rv))`。`WholeCall::run<F: ScriptFront>` 是宣告表替會跑 script 的成員產生的，`ScriptFront: Front`，所以它在 `F` 上讀參數、開操作、寫回傳值，也呼叫 script。
- `WholeFront` 是 `Scoped`（scope、參數、回傳值的位置）加 `Call`（跨過操作要留著的：預先建的 wrapper、window、拿到的 callback、`define` 讀到的 lifecycle callback、接住的例外）。`Scoped` 本身不是 front，`InfoFront` 的慢路（`with_scoped`）也用它。
- 其他入口拿到的是 rusty_v8 給的 scope、參數和回傳值，沒有 callback info：iterator 的函式是 `state_function_template` 做的，cross-origin 的是 access check 的 interceptor。

### 量法

- `bench.py calls --baseline`：`dispatchEvent`（會跑 script 的成員）和其他成員的指令、cycles、store forwarding 的衝突不增加；每個階段做完量一次。
- `bench.py census`：會跑 script 的成員每次呼叫開幾個 scope（預期變多，決定 1），記下來。
- Speedometer 在做完時量一次（RFC 0098 的決定 20）。

### 測試

- 行為：`dom_js` 的測試（events、custom elements、traversal、mutation observer、cross-origin、iterators）和相關的 WPT 目錄（`dom/events`、`custom-elements`、`dom/traversal`、`dom/nodes`、`html/browsers/origin`）。
- 重入：listener 裡再 dispatch、callback 裡丟例外、`define` 的 getter 跑 script、reaction 的順序、microtask 的時機，現有的測試先盤點，缺的補上。
- mutation test：每個搬過去的 script 操作。
- `sanity.py`：`dom_js` 只有一個 `impl Front`（做完時加）。
- 動到 GC 的 root 的寫法時 merge 之後手動開始 `gc.yml`。

### 分階段

3a：`InfoFront` 實作 `ScriptFront`，會跑 script 的成員搬過去。3b：legacy caller、iterator、cross-origin 改成從 info 開始。3c：刪掉 `WholeFront`、`WholeOp`、`Call`，加 `sanity.py` 的檢查，評估 `PropertyFront`。

## 缺點

- 動的是行為最複雜的成員（重入、例外在哪一步丟、microtask 什麼時候跑）。
- 沒有速度上的收穫，會跑 script 的成員可能多開幾個 scope。
- `InfoFront` 會變大：`Call` 留著的東西要搬進去。

## 理由和其他做法

見「當時的選項」。

## 先例

- Blink：每個成員的 callback 是產生出來的一份程式，會跑 script 的和不會的走同一套 `bindings::` 的 helper，沒有第二個「front」；要跑 script 時拿 `ScriptState`。

## 已決定的事項

2026-11-14（使用者：都照建議；每一條的其他選項和理由在下面原本的列表）：

1. **`InfoFront` 自己實作 `ScriptFront`**，每個 script 的操作當場開 scope；`whole(call)` 變成 `call.run(self)`。不選薄的包裝（仍有第二個 `impl Front`）、不選 macro 產生（編出來還是兩套、入口還是兩種）。
2. **legacy caller、iterator 的 `next`、cross-origin 的屬性都改成從 callback info 開始**，各一種 `InfoKind`。cross-origin 的 interceptor 能不能不動 fork 做到，3b 開始時確認。
3. **`PropertyFront` 最後一段再評估**。
4. **分三段**（3a、3b、3c），每段做完可以 merge。

3a，2026-11-14（使用者：都照建議；選項在「待決定」原本的列表）：

5. **跨過 script 的操作要留著的東西照值傳**，不留狀態：`dom_api` 的 `ScriptHost` 和 `ScriptFront` 的那幾個操作明白地收、回 constructor、讀到的 lifecycle callback、filter 丟的例外，「script 已經丟了」是一種錯誤。不選 `InfoFront` 多一個欄位（每次呼叫都付，熱的 getter 也付）；不選放在 isolate 的狀態上（呼叫會重入，要自己維護 stack）。對照 Blink：`CustomElementRegistry::define` 裡這些是區域變數。
6. **搬之前先補測試**：盤點重入、例外、reaction 的順序、microtask 的時機的測試，缺的先補在現在的行為上。

## 當時的選項

1. **會跑 script 的成員怎麼放到 `InfoFront` 上**
   - A（建議）：`InfoFront` 自己實作 `ScriptFront`，每個 script 的操作當場開 scope（`with_scoped`）；`whole(call)` 變成 `call.run(self)`。真的只剩一個 front。代價：一次呼叫裡 script 的操作有幾個就開幾個 scope（每個 27 條指令；`dispatchEvent` 一次約 3,500 條），`Call` 留著的東西要搬進 `InfoFront`。
   - B：`whole` 照樣開一個 scope，交給一個薄的型別（`InfoFront` 加那個 scope），它的 `Front` 全部轉給 `InfoFront`。scope 還是一個；但 `Front` 仍然有第二個 `impl`（94 個轉呼叫的方法），只是沒有邏輯。
   - C：`WholeFront` 留著，它的 `Front` 改由 macro 從 `InfoFront` 的產生。改動最小；兩份程式變成一份原始碼，但編出來的還是兩套，入口也還是兩種。
2. **其他入口（legacy caller、iterator 的 `next`、cross-origin 的屬性）**
   - A（建議）：都改成從 callback info 開始，各給一種 `InfoKind`。legacy caller 已經拿著 info；iterator 的函式改用只拿 info 的 template（`bindings` 已經有）；cross-origin 的 interceptor 改用 `getter_raw` 那一組（named 的 info 已經有，要確認 access check 的 handler 能不能這樣裝，不能的話要動 fork）。
   - B：它們留在 `Scoped` 上的一條小路，不經過 `Front`（各自只有幾個成員）。不用動 callback 的接法；但它們呼叫的是宣告表的 typed path（`M::call(&mut front)`），要一個 front，這條路其實走不通，除非替它們另外產生程式。
3. **`PropertyFront`（`named.rs`，named property 的 interceptor）**
   - A（建議）：最後一個階段再評估，先不定。它只實作 12 個方法，是不是值得併要看前面做完之後 `InfoFront` 長什麼樣。
   - B：一起併。
   - C：明確排除。
4. **分階段**
   - A（建議）：三段，每段可以 merge。3a：`InfoFront` 實作 `ScriptFront`，會跑 script 的成員搬過去（`WholeFront` 還在，給其他入口用）。3b：legacy caller、iterator、cross-origin 改成從 info 開始。3c：刪掉 `WholeFront`、`WholeOp`、`Call`，加 `sanity.py` 的檢查，評估 `PropertyFront`。
   - B：一段做完。

## 待決定

3a 的兩條已經定了（決定 5、6），當時的選項留在這裡：

1. **一次呼叫裡跨過 script 的操作要留著的東西放哪裡**。`ScriptFront` 的 13 個操作裡，10 個只要一個 scope（呼叫 listener、lifecycle callback、mutation callback、upgrade、reject promise、`pending_input`、`update_elements`……），搬過去就是「當場開 scope、呼叫 `listeners` 裡同一個函式」。另外幾個靠 `WholeFront` 的 `Call` 記著東西：`define` 的 constructor（「拿到的 callback」）、它讀到的四個 lifecycle callback、「script 已經丟了例外」的旗標；`NodeFilter` 丟的例外（之後重丟）；geolocation 的第二個 callback。`InfoFront` 沒有地方放。
   - A：`InfoFront` 多一個欄位（一個字，平常是空的）。改動最小；但每次呼叫都要初始化它、結束時檢查它（`WholeFront` 的 `Call` 為了這個特別寫過 `Drop`，量過每次約 18 條指令，壓到 2、3 條），最熱的 getter（`nodeType` 113 條）也付。
   - B（建議）：照值傳，不留狀態。`dom_api` 的 `ScriptHost` 的這幾個操作改成明白地收、回這些東西：constructor 由 `define` 傳給它們（`define` 本來就是泛型地拿著它），讀到的 lifecycle callback 當回傳值、再傳給存的那一步，「已經丟了」是一種錯誤而不是旗標，filter 丟的例外用 `ScriptHost::Value` 帶回來。沒有每次呼叫的成本，擁有權看得見；代價是動 `dom_api` 的 `ScriptHost`、`ScriptFront` 和 `FrontHost`（`dom_wasm` 的 front 如果實作了這幾個也要跟著改）。
   - C：放在 isolate 的 `State` 上。沒有每次呼叫的成本、不動 `dom_api`；但呼叫可以重入（`define` 的 getter 裡再 `define`），要自己維護一個 stack，是藏起來的全域狀態。
2. **搬之前先補測試**：先盤點重入、例外、reaction 的順序、microtask 的時機現有的測試，缺的在搬之前補在 `main` 的行為上（先補、確認通過，再搬）。建議照做。

之後的段落開始時列那一段的。

## 之後的可能

無。
