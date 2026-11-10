# RFC 0093 的細節：wasm 文件接回 renderer：用 `dom_wasm` 做 TodoMVC

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## TodoMVC 用到的成員

2026-11-10 對 `crates/dom_api/src/interfaces.rs` 的宣告表和 `crates/dom_wasm/wit/machopo-dom.wit` 看的。「型別」是表上寫的；是不是 plain 以第 2 段開始時 `for_each_member!` 算出來的為準，這裡只是照型別看。

### WIT 已經有的

| 用途 | 成員 |
| --- | --- |
| 建和改樹 | `createElement`、`createTextNode`、`appendChild`、`insertBefore`、`removeChild`、`innerHTML`、`insertAdjacentHTML`、`textContent` |
| 屬性 | `getAttribute`、`setAttribute`、`removeAttribute`、`hasAttribute`、`id`、`className` |
| 找節點 | `getElementById`、`querySelector`、`closest`、`matches`、`firstElementChild`、`lastElementChild`、`childElementCount`、`parentElement`、`firstChild`、`nextSibling` |

### 只是被凍結擋住的（plain）

2026-11-10（第 1 段做完後）用 `wit::is_plain` 對整張表算的：表上 266 個 interface、2799 個成員（不算常數），plain 的 163 個，其中 68 個在 WIT 裡（凍結的那些）。放開凍結多 95 個，TodoMVC 用得到的：

| 成員 | TodoMVC 用在哪 |
| --- | --- |
| `ChildNode.remove()`（`element-remove`） | 刪掉一個 todo |
| `Document.body` | 入口 |
| `Element.nextElementSibling` | 走過清單（不用 `children` 時） |

### 元素自己的 interface 上的（`this` 要看元素的名稱）

現在的 `is_plain` 把元素的 interface（`HTMLElement`、`HTMLInputElement` 這些，表上有 `element` 的）整個排除：Store 的表每一格只記節點的種類（`NodeKind`），「是不是 `<input>`」要讀 DOM 才知道。這一類不是凍結擋住的，要先讓表記得每個 handle 的 interface（`README.md` 的「待決定」第 5 條）。這份草稿原本把 `value`、`checked`、`blur()` 列在上一節，是錯的。

| 成員 | 表上的型別 | TodoMVC 用在哪 |
| --- | --- | --- |
| `HTMLInputElement.value` | `LegacyNullToEmptyString` | 新 todo 的文字、編輯中的文字 |
| `HTMLInputElement.checked` | `bool` | 完成的勾選、全部勾選 |
| `HTMLElement.blur()` | 沒有參數，`#[calls_script]` | 結束編輯 |
| `HTMLElement.click()` | 沒有參數 | （用不到，同一類） |

`blur()` 會同步分派事件：碰到重入（「已決定的事項」第 5 條：宣告成 `async func`）。

### 要先有節點以外的物件的 handle

| 成員 | 物件 | TodoMVC 用在哪 | 沒有它時 guest 能怎麼繞 |
| --- | --- | --- | --- |
| `Element.classList` | `DOMTokenList`（`add`、`remove` 拿 variadic 的字串，`toggle`、`contains`） | `completed`、`editing`、`selected`、`hidden` | 自己改 `className` 的字串 |
| `HTMLElement.style` | `CSSStyleProperties` | 顯示和隱藏 footer、main | `setAttribute("style", …)` |
| `HTMLElement.dataset` | `DOMStringMap`（named getter） | todo 的 id | `getAttribute("data-id")` |
| `ParentNode.children` | `HTMLCollection` | 走過清單 | `firstElementChild` 加 `nextSibling` |
| `querySelectorAll` | `NodeList` | 篩選的連結 | 一個個 `querySelector` |
| `Event` 的 `type`、`target`、`currentTarget`、`preventDefault()` | `Event`；`target` 是 `Option<EventTarget>`（可能不是節點） | 每個 listener | 沒有 |
| `KeyboardEvent.key` | `KeyboardEvent` | Enter、Escape | 沒有 |
| `Window.location`、`Location.hash` | `Window`、`Location` | 篩選的路由 | 沒有 |
| `Window` 當 `hashchange`、`load` 的 target | `Window` | 篩選的路由 | 沒有 |

「能繞」的那幾列表示第 2 段的最小集合比整張表小：`Event`、`KeyboardEvent`、`Window`、`Location` 是一定要的，`DOMTokenList`、`CSSStyleProperties`、`DOMStringMap` 和兩種集合可以晚一點。繞著寫的 TodoMVC 和 JS 版就不是同一種寫法，要不要接受在第 2 段開始前定。

### 因為別的型別不是 plain 的

| 成員 | 擋住它的型別 |
| --- | --- |
| `EventTarget.addEventListener`、`removeEventListener` | callback（`Option<EventListener>`）、`DictOrBool<AddEventListenerOptions>` |
| `HTMLElement.focus(options?)` | `FocusOptions`（dictionary），`#[calls_script]` |
| `ParentNode.replaceChildren(nodes?)` | `NodesOrStrings`（union、variadic） |
| `DOMTokenList.add(tokens?)`、`remove` | `Strings`（variadic） |
| `HTMLElement.hidden` | `Hidden`（`bool` 或 `"until-found"`） |

machopo 沒有的：`localStorage`（JS 那一側也沒有，TodoMVC 的 todo 不保存）。

## 重入的原型

`reenter-prototype/`（不合併、不在 workspace 裡；`cargo run`，要另一個 target 目錄時設 `CARGO_TARGET_DIR`）。2026-11-10，macOS（Apple Silicon），wasmtime 的 fork（50.0.1）預設的設定。guest 是寫在原始碼裡的 WAT component：`run(depth)` 呼叫 host 的 `dispatch(depth)`，host 在那個呼叫裡呼叫同一個 instance 的 export `listener(depth)`，`listener` 把計數加一、`depth` 大於 1 時再呼叫 `dispatch`。

### 入口和 host 函式的九種組合

`cargo run -- matrix <host> <entry> <depth>`（`sync`、`async`、`concurrent`），巢狀一到三層：

| host 的 `dispatch` 怎麼定義、怎麼呼叫 `listener` | 入口 `call` | 入口 `call_async` | 入口 `run_concurrent` 加 `call_concurrent` |
| --- | --- | --- | --- |
| `func_wrap`，`call` | 可以 | 失敗：`cannot do blocking call from an async context` | 同左 |
| `func_wrap_async`，`call_async` | 失敗：要用 `*_async` 的入口 | 失敗：`Recursive StoreContextMut::run_concurrent calls not supported` | 同左 |
| `func_wrap_concurrent`，`call_concurrent`（import 的型別要是 `async func`） | 失敗：要用 `*_async` 的入口 | 可以 | 可以 |

「可以」的三格，`listener` 都在 `dispatch` 回來之前跑完，巢狀三層由內而外回來；guest 用不帶 `async` 的 `canon lower` 呼叫 `async func` 的 import 也行（當場完成）。

### trap（同步的入口和 host）

| 情況 | 結果 |
| --- | --- |
| `listener` trap，host 把錯誤往上傳 | 最外層的 `run` 失敗；之後這個 instance 的 export 都是 `cannot enter component instance` |
| `listener` trap，host 吞掉錯誤、回傳成功（`cargo run -- swallow`） | wasmtime panic（`BUG: current thread is not a host thread`）：trap 一定要一路傳到最上層 |

### 等待中的 guest，event loop 丟掉再重建

`cargo run -- wait`。guest 的 `run` 是 async 的 export（stackful：停在自己的 stack 上），它呼叫的 `dispatch` 在 host 等一個 channel。

| 步驟 | 結果 |
| --- | --- |
| 第 1 輪：`run_concurrent` 裡開始 `run`，poll 到不動，把 future 丟掉 | host 的 `dispatch` 停在等待；沒有錯誤 |
| 沒有任何 future 活著時，channel 收到值 | |
| 第 2 輪：新的 `run_concurrent`，自己沒有事要做，只讓 event loop 轉 | `dispatch` 醒來、呼叫 `listener`、回來；guest 的計數是 1 |

同一個案例裡 `run` 是普通的（不是 async 的）export 時，第 1 輪就 trap：`cannot block a synchronous task before returning`。

### 結論

- Component Model 現在允許重新進入（`may_enter` 只在 trap 過之後是 false，`../third_party/wasmtime/crates/wasmtime/src/runtime/component/store.rs`；規格的沿革在 `rfcs/0001-wasm-scripts/details.md` 的「Component Model 的重新進入」）。
- **同步的入口**：普通的 `func_wrap` 就能當場呼叫回 guest。
- **async 的入口（WASI 0.3 要的）**：只有一條路，和 RFC 0001 在 wasmtime 49 上量到的一樣：會呼叫回 guest 的成員在 WIT 裡宣告成 `async func`、host 用 `func_wrap_concurrent` 定義、用 `call_concurrent` 呼叫 listener。這種 host 函式拿到的是 `Accessor`，不是 `StoreContextMut`：Store 只在兩次 `await` 之間借得到，所以 DOM 的操作不能跨過呼叫 listener 的那個 `await`。不會呼叫回 guest 的成員照舊是同步的 `func_wrap`。
- 一個 listener trap 時，那次分派排在後面的 listener、和 stack 上更外層的 guest 都不能再執行，Store 整個結束。這是規格的行為，host 改不了。
- **guest 的 task 和 host 的 future 存在 Store 裡，不在 `run_concurrent` 的 future 裡**：Store 可以只在有事要做時借出、poll 一輪、放回去，等待中的 guest 留著。第 1 輪的 waker 跟著 future 丟掉了，所以「等的東西到了」要由 host 自己排下一輪，不能靠 waker。
- **不是 async 的 export 回傳之前不能等待**（wasmtime 50 上重新確認）：listener 的 export 是同步的，guest 只能在 async 的入口裡等。
- 原型沒有涵蓋的：經過 `StoreSet` 借出的 Store、用 elise 的 collector（guest 暫停在等待時它的 stack 上的 root）、`wit-bindgen` 的 C binding 產生的 export（它的回傳區是 static 的，回傳字串或 record 的 export 重入時會互相蓋掉；只回傳 `bool` 或不回傳的不會）、async 的入口的 trap。

## C 的 guest 和 WASI 0.3 的工具鏈

2026-11-10 查的；同一天 1a 用 wasi-sdk 34.0 把 `dom.c` 編成 `wasm32-wasip3`、測試通過（`progress.md`），`async func` 的 import 和 export 還沒有實際編過（1c 的入口是第一個）。

- **版本不用換**：`scripts/ci.py` 現在釘的 wasi-sdk 34.0 和 `wit-bindgen` 0.62.0，就是 RFC 0001 編 C 的 guest 時用的（它的 `progress.md` 的開頭），那時的 target 是 `wasm32-wasip3`，入口是 `async func`、`dispatchEvent` 是 `async func` 的 import。要改的是 `ci.py guests` 的 `--target`（現在是 `wasm32-wasip2`）。
- **`wit-bindgen` 0.62.0 的 C binding 寫得出 `async func`**（在本機對一份小的 WIT 跑 `wit-bindgen c` 看產生的 header）：
  - `async func` 的 import 回傳 subtask 的狀態（`<world>_subtask_status_t`），結果寫進最後一個參數。當場完成的（host 的 `func_wrap_concurrent` 沒有等任何東西）狀態是 `RETURNED`，guest 檢查這個就好；沒完成的要拿它的 handle 放進 waitable set 等。
  - `async func` 的 export 是 callback ABI：`exports_<world>_start(...)` 回傳 callback code（`EXIT`、`YIELD`、`WAIT(set)`），之後 host 帶著事件呼叫 `exports_<world>_start_callback(event)`，做完時 guest 呼叫 `exports_<world>_start_return()`。
  - 不是 async 的 export（`handle-event`）照舊是普通的 C 函式。
- **代價**：C 沒有 `async`/`await`，要等待的 guest 得自己寫狀態機（waitable set、callback code）。不等待的 guest（TodoMVC）只多兩件事：`start` 照 callback ABI 的樣子寫（馬上 `return` 再回傳 `EXIT`）、`async func` 的 import 檢查狀態是 `RETURNED`。
- **舊的 `sdk/c/` 找不回來**：repo 的歷史只到刪掉它之後，C 的 guest 要重寫。

## 參考資料

- `crates/dom_wasm/README.md`：現在的 handle、一次呼叫、WIT 的規則。
- `rfcs/0001-wasm-scripts/README.md`：舊的 wasm 文件的設計（宣告、載入、Store 和 GC、重入、等待）。
- `rfcs/0089-dom-replacement/README.md` 的決定 33 到 37：新的 `dom_wasm` 的 handle、WIT、為什麼停在只碰節點的成員。
- `rfcs/0005-cppgc-wasm-gc/README.md`：wasm 的 GC 物件放在 DOM 的 heap。
- TodoMVC 的規格：<https://github.com/tastejs/todomvc/blob/master/app-spec.md>。
