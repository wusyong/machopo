# RFC 0071 的細節

## 1. Chrome 152 和 kun 的行為（2026-10-22）

同樣的 script 在 Chrome for Testing 152.0.7977.82（headless，CDP 的 `Runtime.evaluate`）和 kun `8570a5b3`（automation pipe 的 `Kun.executeScript`）上跑：

| 項目 | Chrome 152 | kun（膠水） |
| --- | --- | --- |
| `Object.getOwnPropertyNames(new DOMException('m', 'NotFoundError'))` | `[]` | `["stack"]` |
| bindings 丟出的（`document.createElement('1')`）的 own property、`instanceof Error` | `["stack"]`、`true` | `["stack"]`、`true` |
| `DOMException.prototype` 的 `[[Prototype]]` 是 `Error.prototype`、`DOMException.length` | `true`、`0` | `true`、`0` |
| `structuredClone(new DOMException('m', 'NotFoundError'))` | `NotFoundError`、`m`、`8`、是 `DOMException`、沒有 `stack` | `null`、`null`、`null`、不是 `DOMException` |
| `QuotaExceededError` 的 `requested`、它的 `[[Prototype]]` 是 `DOMException` | `2`、`true` | （沒量） |
| `Error.isError(new DOMException())` | `true` | `false` |
| 平台建的不是丟出的（`abort()`、`AbortSignal.abort()`、`AbortSignal.timeout` 的 reason）的 own property | `["stack"]` | （沒量） |
| `structuredClone(new QuotaExceededError('q', {quota: 1, requested: 2}))` | `QuotaExceededError`、`1`、`2`、`22` | （沒量） |
| `structuredClone` 一個 `class E extends DOMException` 的物件 | `DOMException` | （沒量） |
| `window.scrollTo(0, 10)`、`element.scrollTo(0, 0)`、`scrollIntoView(true)` 的回傳 | `Promise` | `Promise` |
| `scrollTo`、`Element.prototype.scrollTo`、`scrollIntoView` 的 `length` | `0`、`0`、`0` | `0`、`0`、（沒量） |
| `addEventListener`、`setTimeout`、`setInterval`、`requestIdleCallback` 的 `length` | `2`、`1`、`1`、`1` | `2`、`1`、（沒量）、`1` |
| `new IdleDeadline()` | `TypeError`（Illegal constructor） | `TypeError`（Illegal constructor） |
| `setTimeout(function (a, b) {...}, 0, 1, 2)` 的 `this === window`、參數 | `true`、`1`、`2` | （膠水照做） |
| `setTimeout('__r(42)', 0)` | 執行字串 | `TypeError`（handler 不是 function） |

訊息的文字兩邊不同（Chrome：`Failed to execute 'createElement' on 'Document': ...`），不在這份的範圍。

## 2. 宣告表已經有的

- union 的字典成員：`dom_js` 的 `union_from_js` 照 WebIDL，`undefined`、`null`、不是 platform object 也不是 sequence 的物件轉成字典成員，其餘照字串、數字、boolean 轉。
- overload：同名的幾行，`resolve_overload` 先比參數個數。
- `Promise<T>` 的回傳：參數轉換、`this` 和函式的錯誤都變成被拒絕的 promise；WIT 是 `async func`。
- `Variadic<T>`、`Any`（只給 JS；optional 的 `any` 參數從 WIT 拿掉）。
- `ScriptHost::call(callback, interface, this, args)`：任意的 `this` 和參數。
- `dom` 的 script object（`ResizeObserverEntry` 這類）：引擎建、交給 callback 的小物件。
- rusty_v8 的 `Exception::capture_stack_trace`：fork 不用改。

沒有的：platform object 的 structured clone（`dom_js` 的 `serialize.rs` 對 API wrapper 一律丟 `DataCloneError`，只有 `MessagePort` 的 transfer）。
