# RFC 0071：拿掉 `glue.js`：每個成員都從宣告表來

- 開始日期：2026-10-22
- 狀態：✅ 已完成：2026-10-22
- 進度和驗收：`progress.md`
- 細節：`details.md`（Chrome 152 和 kun 的行為對照）

## 摘要

`dom_js` 每個 realm 都先執行一段 JS（`dom_js` 的 `glue.js`），定義 `DOMException`、`QuotaExceededError`、`IdleDeadline` 三個 JS class，並包住 12 個成員，把 JS 的形式轉成宣告表的降階形式。Blink 沒有這一層：每個成員都由 IDL 生成 C++ 的 callback，`DOMException` 是 C++ 的物件。這份把它們全部移進 `dom_api` 的宣告表、`DOMException` 這些改成 platform object，拿掉 `glue.js`。

## 動機

- **和 Blink 的分層不同**：`glue.js` 是宣告表還沒有 union、overload、`Promise<T>` 時的權宜做法（`crates/dom_api/README.md`：「已有的、JS 才有的形式照舊由前端處理」）。現在這些都有了，留著它只是歷史。
- **行為和 Chrome 不同**（`details.md`）：
  - timer 的 callback 丟出的例外在錯的 realm 回報：膠水在 `setTimeout` 的 realm 包了一層 function（`docs/limitations.md`）；
  - `new DOMException()` 有自己的 `stack`（膠水在建構子裡 `Error.captureStackTrace`），Chrome 只有平台丟出的才有；
  - `structuredClone(new DOMException(...))` 拿回一個 `name`、`message` 都是 `null`、不是 `DOMException` 的物件（膠水的 class 用 private field 存資料，`ValueSerializer` 看不到），Chrome 的 `DOMException` 是 `[Serializable]`。
- **效能和量測**：這些成員多一次 JS 的跳轉，又落在共用路徑上；`bench.py speedometer --profile` 的成員表裡，經過膠水的 `scrollTo` 和 canvas 的 `fill`、`stroke` 混在同一列。建 realm 時還要先建 `EventTarget`、`Node`、`Element`、`Performance` 的 interface object（膠水要讀它們），並執行這段 script。
- **兩個前端不一致**：wasm 的 guest 只看得到降階的形式，JS 的形式只在膠水裡。

目標：

- `glue.js`、`run_glue`、`GLUE_MEMBERS` 都不存在；每個 JS 看得到的成員和 interface 都來自宣告表。
- 上面三個和 Chrome 不同的行為改成和 Chrome 一樣。
- 建 realm 時只建 global 自己的 interface object（`Window`，和它繼承的 `EventTarget`）。

不在範圍內：

- 替 overload、union、可變參數、回傳 promise 的成員生成專用路徑：它們照舊走共用路徑（第 7 條）。
- wasm 的 guest 的 `requestIdleCallback`（`kun:dom` 沒有它）。

要解決的 `docs/limitations.md` 裡的限制：

- 「Timer 的 callback 丟出的例外在 timer 所在的 window 回報」（「Script」）。
- 「Timer 的 handler 只接受 function，不支援字串」（「Script」，第 6 條），連同 module script 那一條裡「字串形式的 `setTimeout` 不支援，所以裡面的 `import()` 也沒有」。
- `requestIdleCallback` 那一條裡「`IdleDeadline` 是 `glue.js` 的 JS class」的部分（「Script」）。

## 設計概要

網頁看到的行為和 Chrome 152 一樣（`details.md` 的對照表）；改的是它們從哪裡來。

- `DOMException`、`QuotaExceededError`、`IdleDeadline` 是宣告表裡的 interface，物件是 `dom` 的 script object（和 `ResizeObserverEntry` 一樣），JS 照 interface 建 wrapper。`DOMException.prototype` 的 `[[Prototype]]` 是 `%Error.prototype%`（WebIDL）。
- bindings 丟出 DOMException 時建一個 `DOMException` 物件，再替它記下 stack（V8 的 `Exception::CaptureStackTrace`，Blink 的 `V8ThrowDOMException`）；`new DOMException()` 沒有 `stack`。
- `addEventListener(type, callback, options)` 的 `options` 是宣告表的 union `(AddEventListenerOptions or boolean)`；`scrollIntoView` 的是 `(ScrollIntoViewOptions or boolean)`。
- `scroll`、`scrollTo`、`scrollBy` 是兩個 overload（`(options)`、`(x, y)`），回傳 `Promise<undefined>`。
- `setTimeout(handler, timeout, ...arguments)`：native 存著額外的參數，到時以 window 為 `this` 呼叫 handler；callback 丟出的例外在 callback 的 realm 回報；字串的 handler 當 classic script 執行。
- `requestIdleCallback` 的 callback 收到 native 建的 `IdleDeadline`，屬於 callback 的 realm。

做完之後，`crates/dom_api/README.md` 的「怎麼新增一個 API」不再有「JS 的形式要改 `glue.js`」這一條。

## 詳細設計

### 1. `DOMException` 和 `QuotaExceededError`

- 宣告表：`interface DOMException`，`constructor(message = "", name = "Error")`，`name`、`message`、`code` 三個 attribute，25 個常數；`interface QuotaExceededError : DOMException`，`constructor(message = "", options)`，`quota`、`requested`。照 Chrome 152：`quota`、`requested` 是負數或 `requested` 小於 `quota` 時不是 `RangeError`。
- 物件：`dom` 的 script object（新的 `ObjectKind`），存 name、message（和 quota、requested）（第 2 條）。
- `[[Prototype]]`：建 interface object 時（RFC 0070 的每個 context 的快取）把 prototype 的 `[[Prototype]]` 接到這個 context 的 `%Error.prototype%`。宣告表的 interface 標記 `[ExceptionPrototype]` 說要這樣接（第 3 條）；Blink：`WrapperTypeInfo` 的 `kWrapperTypeExceptionPrototype`，`V8PerContextData` 建 interface object 時接上。
- bindings 丟出：`dom_api` 的 `DomException`（名字和訊息）在 `dom_js` 換成建一個 `DOMException` 物件、`Exception::CaptureStackTrace`、丟出。現在存在 context 的 embedder data 裡的 constructor（`DOM_EXCEPTION_CONSTRUCTOR`）不用了。`AbortSignal` 預設的 reason（`V8Host::create_dom_exception`）同樣建物件、記下 stack（Chrome 152 的也有，第 9 條）。
- structured clone（第 4 條）：`DOMException` 是第一個 Serializable 的 platform object。`dom_js` 的 `serialize.rs` 對它寫一個 tag 加上名字和訊息（`QuotaExceededError` 再加 quota、requested），讀回時在目標的 realm 建新的物件，沒有 `stack`；其他 platform object 照舊是 `DataCloneError`。Blink 的 `V8ScriptValueSerializer::WriteDOMObject` 照型別寫 tag（`kDOMExceptionTag`）。
- wasm：guest 的錯誤照舊是 `kun:dom` 的錯誤代碼，`DOMException` 不進 WIT（`[JsOnly]`）。
- `Error.isError`：V8 問 embedder 一個 API wrapper 是不是 native error（`IsJSApiWrapperNativeErrorCallback`），`dom` 在 `DomIsolate::new` 設 callback，照 wrapper 的 tag 認兩種（第 12 條；fork 加這個 binding）。

### 2. `addEventListener`、`removeEventListener`

`options` 改成 `unions!` 的 `AddEventListenerOptionsOrBoolean`、`EventListenerOptionsOrBoolean`。`union_from_js` 已經照 WebIDL 把 `undefined`、`null`、物件轉成字典成員，其餘轉成 boolean；boolean 在 `dom_api` 裡當成 `{ capture }`。含有沒有必填成員的字典的 union 參數照 WebIDL 是 optional（`declare::required_params`），`length` 照舊是 2。

### 3. 捲動

- `scroll`、`scrollTo`、`scrollBy`（`Element`、`Window`）：兩行 overload，`(options: ScrollToOptions)` 和 `(x: UnrestrictedF64, y: UnrestrictedF64)`（第二行用 `as` 給 WIT 的名稱），`resolve_overload` 照參數個數選；回傳 `Promise<()>`，native 捲完當場 resolve，參數轉換和 `this` 的錯誤變成 rejection（宣告表的 `Promise<T>` 本來就這樣）。
- `scrollIntoView`：`(ScrollIntoViewOptions or boolean)`，`true` 是 `{ block: "start", inline: "nearest" }`、`false` 是 `{ block: "end", inline: "nearest" }`（Blink 的 `ScrollIntoViewOptionsOrBoolean`），回傳 `Promise<()>`。
- WIT 照規範（第 5 條）：overload 是另一個函式（`as` 的名稱），回傳 promise 的是 `async func`，union 是 `variant`；`kun:dom` 升一次版，`python3 scripts/ci.py guests` 重新產生 guest 的 binding。

### 4. 計時器

- 宣告表：`[JsOnly] setTimeout(handler: FunctionOrString, timeout: i32 = 0, arguments: Variadic<Any>)` 和 `[WitOnly] setTimeout(handler: TimerHandler, timeout: i32)`，`setInterval` 一樣（第 18 條）；`kun:dom` 不變。`FunctionOrString` 是 `unions!` 的 `(TimerHandler or DOMString)`：`union_from_js` 照 WebIDL 把 function 轉成 callback function 的成員（在 buffer source 之後、sequence 和字典之前），其他的值轉成字串（`setTimeout({})` 執行 `"[object Object]"`）。
- `dom` 的計時器存 `ScheduledAction`（第 16 條，Blink 的同名類別）：function 和額外的參數（`ScriptValue`，`undefined` 是沒有值），或程式碼。
- 觸發時（renderer 的 `run_due_timer`）：function 以 timer 的 document 的 window 為 `this`、帶參數經過 `call_reporting` 呼叫，例外在 callback 的 realm 回報；程式碼經過 `ScriptEngine::run_classic_script` 在 window 的 realm 執行，沒有網址（Blink 建 `ClassicScript` 時也不給，stack 是 `<anonymous>`），base URL 是 document 目前的，照 HTML 不收額外的參數，`setInterval` 每次重新執行（第 17 條）。
- 回報例外時，沒有網址的 script 的錯誤用 document 的網址（`dom_js` 的 `report_exception_in`，Blink 的 `CaptureSourceLocation`），不只計時器。

### 5. `requestIdleCallback` 和 `IdleDeadline`

- `interface IdleDeadline`（`[JsOnly]`）：`timeRemaining()`、`didTimeout`；沒有建構子（`new` 是 `TypeError`）。
- 物件是 `dom` 的 script object，存期限（document 的時間）、`didTimeout`、它的 window 和 callback 的 realm 的 window（第 19、20 條）。`timeRemaining()` 用 window 的時鐘算到期限，差取回 100 µs 的格子，過了期限或 `didTimeout` 時是 0。
- renderer 叫 idle callback 時，在 document 的 realm 裡經過 host 建它（`dom_api::idle_deadline::create`，從 `ScriptHost::callback_window` 拿 callback 的 window），當 callback 唯一的參數，`this` 是 `undefined`；`IdleRequestCallback` 不再是降階的期限和 `didTimeout`。
- wrapper 建在 callback 的 realm（`Object::realm_of`）：別的 frame 的 function 拿到那個 frame 的 `IdleDeadline`，和 Chrome 一樣（Blink 的 callback function 在 callback 的 script state 轉換參數）。不在 `V8Host::call` 一律這樣轉換：event 的 wrapper 是 target 的 realm（Blink 的 `JSBasedEventListener`）。

### 6. 拿掉膠水

- `glue.js`、`run_glue`、`GLUE_MEMBERS`、`glue_member`、`glue_natives` 和 `install` 裡為它準備的參數，`typed.rs` 排除膠水成員的條件（第 21 條）。
- 建 realm 時只建 `Window` 和 `EventTarget` 的 interface object（WebIDL 的 `Window` 的 interface object 的 `[[Prototype]]` 是 `EventTarget` 的），`Performance`、`Node`、`Element` 不再建。`realms::realm_creation_time` 改之前和之後各量一次（`progress.md`）：建 realm 本身少做了事，但這個量測留著 500 個 realm，GC 的時機變了，整體約多 4%。

### 測試

- `dom_js`：`DOMException` 的建構子、attribute、常數、`[[Prototype]]`、`toString`、`Object.prototype.toString`；`new DOMException()` 沒有 `stack`、bindings 丟出的有；`instanceof Error`；`QuotaExceededError`；structured clone（看第 3 條）；`addEventListener` 的 boolean 和字典；捲動的兩個 overload、回傳的 promise、rejection；`IdleDeadline`。預期值先在 Chrome 152 上跑同樣的 script（`details.md`）。
- `shell`：計時器的 `this`、額外參數、字串的 handler、例外的 realm，和 `IdleDeadline` 的 realm，用同一頁在 Chrome 152 跑出來的結果比。
- WPT：`webidl/ecmascript-binding/es-exceptions/`（`DOMException-*.any.js`）、`html/webappapis/timers/`（包括 `*-cross-realm-callback-report-exception.html`）、`requestidlecallback/`、`css/cssom-view/` 的捲動、`dom/events/` 的 `AddEventListenerOptions-*`：跑相關的目錄，更新預期結果。
- `dom_api` 的 WIT golden file、`spec` 測試（`spec-missing.txt`）。
- mutation test 照 `docs/testing.md`。

### 分階段

1. **`DOMException`、`QuotaExceededError`**：script object、`[[Prototype]]`、bindings 丟出時的 stack、structured clone。
2. **`addEventListener`、`removeEventListener`、捲動**：union、overload、promise。
3. **計時器**：額外參數、`this`、例外的 realm、字串的 handler。
4. **`IdleDeadline`，拿掉 `glue.js`**：建 realm 只建 `Window`，量 realm 建立的時間。

## 缺點

- `dom` 多三種 script object（`DOMException`、`QuotaExceededError`、`IdleDeadline`），bindings 丟出 DOMException 時多建一個 GC 的物件（Blink 一樣）。
- 有可變參數、overload、回傳 promise 的成員照舊走共用路徑（第 7 條），效能的好處要等專用路徑；`addEventListener` 走專用路徑。
- `realms::realm_creation_time` 沒有變快（`progress.md`）。
- WIT 照規範改形狀，guest 要跟著改，`kun:dom` 升一次版（第 5 條）。

## 理由和其他做法

- **留著膠水**：功能上可以，但和 Blink 的分層不同，造成三個和 Chrome 不同的行為，建 realm 要多做事。不選（使用者，2026-10-22：「既然 Chrome 沒有這層膠水，我們理論上也不該有」）。
- **`DOMException` 留在 JS、只修 `stack` 和 clone**：clone 要讓 `ValueSerializer` 認得一個 JS class 的 private field，比 platform object 還麻煩，而且仍然要在每個 realm 執行 script。

## 先例

- Blink：`bindings/scripts` 從 IDL 生成每個成員的 callback，overload 由生成的分派照參數決定；`DOMException` 是 `ScriptWrappable`（`core/dom/dom_exception.h`），`V8ThrowDOMException` 建它並 `v8::Exception::CaptureStackTrace`；`WrapperTypeInfo::kWrapperTypeExceptionPrototype` 讓它的 prototype 接到 `Error.prototype`；`setTimeout` 的參數存在 `ScheduledAction`；`IdleDeadline` 是 `ScriptWrappable`。
- Gecko：`DOMException` 是 C++ 的 `dom::Exception`，`[Serializable]`。

## 已決定的事項

2026-10-22（使用者）：

1. **拿掉整個 `glue.js`**：12 個成員移進宣告表，`DOMException`、`QuotaExceededError`、`IdleDeadline` 改成 platform object。在 RFC 0070 之後做，建立在它的 lazy interface object 和每個 context 的快取上。
2. **`DOMException` 的物件是 `dom` 的 script object**：宣告表的 interface，和其他 interface 同一套（Blink 的 `ScriptWrappable`）。不選 `dom_js` 自己的 V8 template：又是一個不經過宣告表的例外。
3. **`Error.prototype` 由宣告表的標記決定**：interface 標 `[ExceptionPrototype]`，`dom_js` 建 interface object 時照它把 prototype 的 `[[Prototype]]` 接到 `%Error.prototype%`（Blink 的 `kWrapperTypeExceptionPrototype`）。不選照名字寫死。
4. **`DOMException` 的 structured clone 一起做**：第一個 Serializable 的 platform object，只存名字和訊息（`QuotaExceededError` 加上 quota、requested）。不選之後再做：改成 native 之後 clone 會變成 `DataCloneError`，比現在更不像 Chrome。
5. **WIT 照規範**：union、overload、promise 都照 WebIDL 進 `kun:dom`，升一次版、重新產生 guest 的 binding；一個形式只宣告一行。不選 JS 照規範、WIT 維持現在的形式：每個成員要 `[JsOnly]`、`[WitOnly]` 各寫一行，兩個前端繼續不一樣。
6. **`setTimeout` 的字串 handler 一起做**：`TimerHandler` 反正要重寫；Chrome 執行字串，拿掉一條限制。只給 JS。
7. **專用路徑另開一份 RFC**：這份只拿掉膠水，overload、union、可變參數、回傳 promise 的成員照舊走共用路徑（RFC 0069 的「之後的可能」）。不選一起做：範圍大很多，拿掉膠水本身不需要它。

2026-10-22，第 1 段開始前（使用者）：

8. **`dom` 存兩種 script object**：`DOMException`（名字、訊息）和 `QuotaExceededError`（再加 `quota`、`requested`），宣告表每個 interface 有自己的物件種類。名字是字串，不是 `ExceptionName`：`new DOMException('m', 'Foo')` 可以是任何名字，`code` 是 0。Blink 的 `DOMException` 存 name、message 的 `String`，`QuotaExceededError` 是它的子類別。
9. **平台建的 DOMException 有 `stack`**：bindings 丟出的，和 `create_dom_exception` 建的（`abort()`、`AbortSignal.timeout` 的 reason），建好之後 `Exception::CaptureStackTrace`；`new DOMException()` 和 clone 出來的沒有（Chrome 152 實測，`details.md`）。Blink 的 `V8ThrowDOMException::CreateOrEmpty`。
10. **structured clone**：`serialize.rs` 對 `DOMException` 的 wrapper 寫一個 tag 加上名字和訊息，`QuotaExceededError` 另一個 tag，多寫 `quota`、`requested`；讀回時在目標的 realm 建新的物件。子類別 clone 回來是 `DOMException`（和 Chrome 一樣）。postMessage、`structuredClone` 和存起來的（`history.state`）都適用，其他 platform object 照舊是 `DataCloneError`。
11. **`DOMException`、`QuotaExceededError` 只給 JS**：標 `[JsOnly]`，不進 `kun:dom`，guest 的錯誤照舊是錯誤代碼。第 5 條管成員的形狀，不包括把 JS 的例外物件搬進 WIT。
12. **`Error.isError(new DOMException())` 是 `true`**（第 1 段做的時候，使用者）：V8 判斷 API wrapper 是不是 native error 時問 embedder 的 `IsJSApiWrapperNativeErrorCallback`（Blink 的 `V8Initializer` 設它），Chrome 152 是 `true`。fork 加這個 binding（`binding.cc`，本機增量重編），kun 的 callback 照 wrapper 的 tag 認 `DOMException`、`QuotaExceededError`；fork 第 1 段做完就發 release（使用者：先打 tag，workflow 期間和 RFC 做完之前用 `RUSTY_V8_ARCHIVE`）。不選記成限制：和 Chrome 不同，WPT 的 `DOMException-is-error.any.js` 失敗。

2026-10-22，第 2 段開始前（使用者）：

13. **WIT 照第 5 條**：`addEventListener`、`removeEventListener` 的 `options` 是 variant（`add-event-listener-options-or-boolean`、`event-listener-options-or-boolean`），`scroll`、`scroll-to`、`scroll-by`、`scroll-into-view` 是 `async func`，`(x, y)` 的 overload 是 `scroll-xy`、`scroll-to-xy`、`scroll-by-xy`（照現有的 overload 用描述性的名字，`strokePath`、`setTransformMatrix`）。16 份 guest 的原始碼跟著改，C 的 guest 用 `common.h` 等 `async func` 的 helper。不選 JS 照規範、WIT 不動：要 `[JsOnly]`、`[WitOnly]` 各寫一行。
14. **`kun:dom` 升到 `0.15.0`，只在這段升一次**：第 3、4 段的改動只給 JS，不再動 WIT；`ci.py guests` 重新產生 guest 的 binding（包括 Lua 的 runtime）、重編 guest。
15. **捲動回傳的 promise 和膠水一樣**：捲動都是 instant，native 回來時就 resolve；參數和 `this` 的錯誤是 rejection（宣告表的 `Promise<T>`）。Chrome 152 也回傳 promise。

2026-10-22，第 3 段開始前（使用者）：

16. **計時器存 `ScheduledAction`**：`dom` 的計時器的 callback 改成 function（`ScriptCallback`）和額外的參數（`ScriptValue`，`undefined` 是沒有值），或程式碼（字串）；觸發時以 window 為 `this`、帶參數呼叫，例外照 callback 的 realm 回報（`call_reporting`）。Blink 的 `ScheduledAction` 一樣存 function 和參數，或程式碼。
17. **字串的 handler 由 renderer 用 `ScriptEngine::run_classic_script` 執行**：在 window 的 realm，base URL 是 document 的，從第 1 行起算，和 inline 的 `<script>` 同一條路；照 HTML 不收額外的參數，`setInterval` 每次重新執行。Blink 的 `ScheduledAction::Execute` 用 `ClassicScript`。實作時（2026-10-22）照 Chrome 152 的實測改了一點：script 沒有網址（Blink 建 `ClassicScript` 時不給網址，stack 是 `at <anonymous>:1:10`），錯誤事件的 `filename` 是 document 的網址：Blink 的 `CaptureSourceLocation` 對沒有網址的 script 用 execution context 的網址，kun 回報例外時照做（不只計時器：之後定義在這種 script 裡的 function 丟出的也一樣，Chrome 也是）。
18. **JS 和 WIT 各一行**：`[JsOnly] setTimeout(handler: (Function or DOMString), timeout = 0, ...arguments: any)`，`[WitOnly] setTimeout(handler: TimerHandler, timeout)` 維持現在的 WIT，`setInterval` 一樣；`union_from_js` 把可以呼叫的物件轉成 callback 的成員，其他的照 WebIDL 轉成字串（`setTimeout({})` 執行 `"[object Object]"`，和 Chrome 一樣）。

2026-10-22，第 4 段開始前（使用者）：

19. **`IdleDeadline` 存期限、`didTimeout` 和它的 window**（實作時多存 callback 的 realm 的 window，見第 20 條）：期限是 document 的時間（`performance.now()` 的時鐘，已經取到 100 µs 的格子）；`timeRemaining()` 用 window 的 frame 的現在時刻算，差取到 100 µs 的格子（照膠水的四捨五入），`didTimeout` 時是 0。頁面看到的值和膠水的一樣。Blink 存 `TimeTicks` 的期限，減去現在再 clamp 解析度。不選存絕對時刻、讀的時候換算：要多存 time origin，結果一樣。
20. **`dom_api` 的 `idle_deadline::create` 建它**：renderer 在 callback 的 realm 裡呼叫，物件是 callback 唯一的參數（`IdleRequestCallback` 不再是降階的兩個參數）。每次 idle callback 多一個 GC 的物件，Blink 一樣。實作時（2026-10-22）照 Chrome 152 的實測補上：別的 frame 的 function 拿到的是那個 frame 的 `IdleDeadline`（Blink 的 callback function 在 callback 的 script state 轉換參數），所以 `create` 經過 host 拿 callback 的 window（`ScriptHost::callback_window`），物件記著它當自己的 realm。不在 `V8Host::call` 一律改成 callback 的 realm：試過，event 會錯（Chrome 的 event 的 wrapper 是 target 的 realm，Blink 的 `JSBasedEventListener` 用 target 的 script state），`messaging` 的兩個測試抓到。
21. **拿掉整個膠水**：`glue.js`、`run_glue`、`GLUE_MEMBERS`、`glue_natives`、`typed.rs` 排除膠水成員的那條、`spec-missing.txt` 說明裡的膠水。建 realm 只建 `Window` 和 `EventTarget` 的 interface object：WebIDL 的 `Window` 的 interface object 的 `[[Prototype]]` 是 `EventTarget` 的，Chrome 也一樣建；`Performance` 不再建。
22. **量 realm 建立的時間**：拿掉前後各跑 `realms::realm_creation_time`，記進 `progress.md`。

## 待決定

（無）

## 之後的可能

- 共用路徑上的成員改走生成的路（overload、union、可變參數、回傳 promise 的、event handler 屬性）。
- wasm 的 guest 的 `requestIdleCallback`。
