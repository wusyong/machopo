# RFC 0100：V8 那一側只有一個 front

- 開始日期：2026-11-14
- 狀態：✅ 已完成：2026-11-14
- 細節：無

## 摘要

`dom_js` 原本有三個 front：只拿 V8 的 callback info 的 `InfoFront`（每個成員從它開始）、拿著一個 scope 的 `WholeFront`（會跑 script 的成員、cross-origin 的屬性、legacy caller、iterator 走它，和 `InfoFront` 各自實作同一套 `Front`、`OpFront`、`DictFront`），和 named property 的 interceptor 用的 `PropertyFront`。現在只有 `InfoFront`：會跑 script 的呼叫在它上面跑（每次呼叫 script 當場開 scope），其他入口都從 callback info 開始，`WholeFront`、`WholeOp`、`Call`、`PropertyFront`、`PropertyOp` 刪掉了。這是 RFC 0098 路線圖的 M3；目的是一致性和少一份程式，不是速度。做了什麼、量到什麼在 `progress.md`。

## 動機

- **同一件事寫了兩次**：

  | | `InfoFront` | `WholeFront` |
  | --- | ---: | ---: |
  | `Front` | 94 個方法、860 行 | 93 個方法、680 行 |
  | `OpFront` | 32 個、368 行 | 31 個、339 行 |
  | `DictFront` | 16 個、133 行 | 16 個、161 行 |

  加一種參數或回傳的型別要改兩處，行為要靠測試保證兩邊一樣。
- **原本走 `WholeFront` 的入口**：會跑 script 的成員（`InfoFront::whole` 開一個 scope、建一個 `WholeFront`，成員剩下的部分在它上面跑）、legacy caller（`document.all(..)`）、cross-origin 的屬性（access check 的 interceptor，兩處）、iterator 的 `next`（`pairs.rs`）。另外 `named.rs` 的 `PropertyFront` 是第三個、只實作一部分的 front（12 個方法）。
- 對網頁沒有看得到的變化。

## 設計概要

對改 binding 的人：`Front` 只有一個實作（`sanity.py deps` 檢查）。會跑 script 的操作（呼叫 listener、custom element 的 callback、`NodeFilter`……，`ScriptFront` 的方法）是 `InfoFront` 的方法，要用到 V8 會配置或跑 script 的地方時當場開 scope（和它其他的慢路一樣）。

## 詳細設計

- **會跑 script 的成員**：`InfoFront` 實作 `ScriptFront`，每個操作 `info.with_scope` 開一個 scope、呼叫 `listeners` 和 `front` 裡做那件事的函式；`Front::whole(call)` 是 `call.run(self)`。沒有呼叫到 script 的呼叫不開 scope（`dispatchEvent` 沒有 listener 被呼叫時 0 個）。
- **跨過 script 要留著的東西照值傳**（決定 5）：`dom_api` 的 `ScriptHost` 有 `Callback`、`Lifecycle` 兩個型別，`define` 把 constructor 和讀到的 lifecycle callback 明白地傳給它的每一步；script 丟的例外留在 V8 裡，成員的錯誤是 `ApiError::Thrown`（front 拿到它什麼都不丟）。traversal 的 filter 丟的例外接住之後立刻在 V8 裡重丟。front 沒有一次呼叫的狀態。
- **其他入口**：各是 `InfoFront` 的一種（`InfoKind`）：legacy caller（`Called`：`this` 是被呼叫的物件，整個檢查）、pair iterator 的一步（`Pair`：函式改成只拿 info 的，info 帶著 target 和 index，成員跑完把 info 拿回來）。cross-origin 的屬性的成員本來就是只拿 info 的 method，直接用 `Method`；getter 不先進 receiver 的 realm（決定 7）。
- **named property 的 interceptor**：`bindings` 的 callback 全部是 V8 原始的（`StateInfoNamed`），回傳值不是 `Value` 的由 trampoline 照結果寫；`InfoKind` 的 `NamedHas`（query 和 delete 問 getter 有沒有，不做 V8 的值）、`NamedDelete`、`Names`、`IndexedSet`，define 和數字的 set 用 `NamedSet`。數字當名稱時名稱是它的數字的字串，在 trampoline 做。
- `front.rs` 剩下的是 `Scoped`：當場開的 scope、參數和回傳值的位置，要 V8 配置、丟例外、轉換的函式是它的方法，不是 front。
- 沒有動 rusty_v8 的 fork。

### 量法

- `bench.py calls --baseline`：`dispatchEvent`（會跑 script 的成員）和其他成員的指令、cycles、store forwarding 的衝突不增加；每個階段做完量一次。
- `bench.py census`：會跑 script 的成員每次呼叫開幾個 scope（預期變多，決定 1），記下來。
- Speedometer 在做完時量一次（RFC 0098 的決定 20）。

### 測試

- 行為：`dom_js` 的測試（events、custom elements、traversal、mutation observer、cross-origin、iterators）和相關的 WPT 目錄（`dom/events`、`custom-elements`、`dom/traversal`、`dom/nodes`、`html/browsers/origin`）。
- 重入：listener 裡再 dispatch、callback 裡丟例外、`define` 的 getter 跑 script、reaction 的順序、microtask 的時機，現有的測試先盤點，缺的補上。
- mutation test：每個搬過去的 script 操作。
- `sanity.py deps`：`dom_js` 只有一個 `impl Front`。
- 動到 GC 的 root 的寫法時 merge 之後手動開始 `gc.yml`。

### 分階段

四段，都做完了（`progress.md`）。3a：`InfoFront` 實作 `ScriptFront`，會跑 script 的成員搬過去。3b：legacy caller、iterator、cross-origin 改成從 info 開始。3c：刪掉 `WholeFront`、`WholeOp`、`Call`，加 `sanity.py` 的檢查。3d：`PropertyFront` 併進 `InfoFront`（決定 8）。

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

3b、3c，2026-11-14（使用者）：

7. **cross-origin 的 getter 不先進到 receiver 的 realm**。3b 一開始照原本的行為搬過來（`bindings` 多一個在 info 上進入另一個 context 的入口，一處新的 `unsafe` 用法，每個 getter 開一個 scope），mutation test 發現它沒有看得到的效果：這些 getter（`window`、`self`、`location`、`closed`、`frames`、`length`、`top`、`opener`、`parent`）不丟例外，給的東西的 wrapper 本來就建在物件自己的 realm；RFC 0097 在舊的 front 上也查到同一件事。拿掉之後 cross-origin 的成員和其他 method 完全一樣。不選留著：一段沒有測試守得住的程式和論證。
8. **`PropertyFront` 也併進來**（決定 3 留到最後的；使用者選併）：另開一段 3d。不選留著當 `sanity.py` 列的例外。
9. **3d 分兩步**：3d-1 是 query、delete、enumerate、indexed setter（和索引的 query 同一種做法：V8 原始的 callback，各自把結果寫成它的型別）；3d-2 是 define 和數字名稱的 get、set；都做完才刪 `PropertyFront`。named 的 query 沒有 `length` 可以比，仍然呼叫 getter 看有沒有，只是不建 wrapper、不經過 rusty_v8 的 trampoline。

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
   - A（建議）：三段，每段可以 merge。3a：`InfoFront` 實作 `ScriptFront`，會跑 script 的成員搬過去（`WholeFront` 還在，給其他入口用）。3b：legacy caller、iterator、cross-origin 改成從 info 開始。3c：刪掉 `WholeFront`、`WholeOp`、`Call`，加 `sanity.py` 的檢查。3d：`PropertyFront` 併進 `InfoFront`（決定 8）。
   - B：一段做完。

## 待決定

無。3a 開始時列的選項（跨過 script 的東西放哪裡：`InfoFront` 多一個欄位、照值傳、放在 isolate 的狀態上）和選的理由在決定 5。

## 之後的可能

- cross-origin 的物件的 access check 的 interceptor（`bindings` 的 `StateAccessCheck`）和幾個不是成員的函式（observable array、內部的函式）仍然經過 rusty_v8 先開 scope 的 trampoline：它們不呼叫宣告表的成員，不需要 front。
- `Scoped::reject_with` 的 `thrown` 參數現在一律是 none，可以拿掉。
