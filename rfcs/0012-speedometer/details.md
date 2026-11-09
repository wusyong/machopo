# RFC 0012 的細節：用 Speedometer 3 和 Chromium 比較效能

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 版本

2026-10-07 看的上游（`https://github.com/WebKit/Speedometer`）：
- **`release/3.1`**：最後一個 commit 是 `1386415be8fef2f6b6bbdbe1828872471c5d802a`（2025-03-11）。這是 browserbench.org 上的 Speedometer 3.1。runner 在 `resources/` 底下（`main.mjs`、`benchmark-runner.mjs`、`tests.mjs`、`params.mjs`），每個 suite 的 app 也在 `resources/` 底下，連同編好的 `dist/` 一起 commit，伺服器直接提供靜態檔案就能跑。
- **`main`**：2026-09-23 的 `b0bc16e`。結構改了：runner 共用的部分搬到 `shared/`（`benchmark.mjs`、`step-runner.mjs`、`step-scheduler.mjs`……），suite 改由 `config.json` 列出（目前只有一個 `NewsSite-PostMessage`，`type: "remote"`），還有 `suites-experimental/`。這是還沒發布的下一版，量測方式可能還會變，所以不用它。
- 另有 `release/3.0`、`release/2.1`。

## Speedometer 3.1 的結構

讀 `release/3.1` 的 `1386415` 的原始碼（2026-10-07）。

**頁面和 runner**：
- `index.html` 用 `<script type="module">` 載入 runner（`main.mjs`）；`main.mjs` 靜態 import `benchmark-runner.mjs`、`statistics.mjs`、`tests.mjs`、`metric-ui.mjs`、`params.mjs`、`developer-mode.mjs`，最後設定 `globalThis.benchmarkClient`。
- `params.mjs` 從 `location.search` 讀參數（`URLSearchParams`）：`startAutomatically`、`iterationCount`（預設 10）、`suites`、`tags`、`viewport`（預設 800x600）、`measurementMethod`（`raf` 或 `timer`，預設 `raf`）、`shuffleSeed`（預設 `off`）、`warmupBeforeSync`、`waitBeforeSync` 等。參數錯誤時用 `alert`。
- `tests.mjs` 列出 suite。沒有 `disabled: true` 的有 `default` 的 tag。

**iframe**：`BenchmarkRunner._appendFrame` 用 `document.createElement("iframe")` 建立，`frame.style` 設 `width`、`height`、`border`、`position: absolute`、`left: 50%`、`top: 50%`、`transform: translate(-50%, -50%)`，加 `scrolling="no"`，插在 `body` 的最前面。每個 suite 設定 `frame.src = "resources/<suite 的 url>"`，用 `frame.onload` 等它載入，再跑 suite 的 `prepare(page)`（通常是 `page.waitForElement(...)`，用 `requestAnimationFrame` 輪詢）。

**每個 test**（`_runTestAndRecordResults`，`RAFTestInvoker`）：
1. `requestAnimationFrame(runSync)`：`performance.mark` 開始；`performance.now()` 前後包住 `test.run(page)`，差值是 sync；接著記下 async 的開始時間。
2. 同一個 frame 的第二個 `requestAnimationFrame` callback：`setTimeout(measureAsync, 0)`。在 Chrome 裡，這個 timer 在這個 frame 的 rendering update 之後才執行。
3. `measureAsync`：呼叫 `frame.contentDocument.body.getBoundingClientRect()` 強制 layout（註解說有的瀏覽器在 paint 之前不更新 layout），`performance.now()` 減去開始時間是 async；再 `performance.measure` 兩段。
4. 再一個 `setTimeout(0)`：記錄結果（`_recordTestResults`），呼叫 `client.didRunTest(suite, test)`。

`measurementMethod=timer` 時改用 `TimerTestInvoker`（`setTimeout` 包住 sync，再一個 `setTimeout` 加 rAF）。

**分數**：每個 iteration 裡，每個 suite 的時間是它所有 test 的 sync + async 的總和；iteration 的 geomean 是各 suite 時間的幾何平均，分數是 `1000 / geomean`。最後的分數是各 iteration 的平均，附信賴區間（`statistics.mjs`）。suite 的時間是 0 時 `_validateSuiteTotal` 丟例外。

**給自動化的 hook**（`benchmarkClient` 上，`BenchmarkRunner` 有就呼叫）：`willStartFirstIteration`、`willAddTestFrame(frame)`、`willRunTest(suite, test)`、`didRunTest(suite, test)`、`didRunSuites(measuredValues)`、`handleError(error)`、`didFinishLastIteration(metrics)`。`main.mjs` 的 `MainBenchmarkClient` 實作了它們，`_formattedJSONResult` 可以輸出 JSON。

**測試步驟用的 `PageElement`**（`benchmark-runner.mjs`）：`setValue`（設 `value`）、`click()`、`focus()`、`dispatchEvent`（`new Event(...)`；`submit` 用 `createEvent` 加 `initEvent`）、`enter`、`dispatchKeyEvent`（`new KeyboardEvent(type, {keyCode, which, key, bubbles, cancelable})`）、`dispatchMouseEvent`（`getBoundingClientRect`、`contentWindow.screenX`、`new contentWindow.MouseEvent(...)`）、`querySelectorInShadowRoot`（`shadowRoot`）。`Page.layout()` 呼叫 `getBoundingClientRect` 和 `document.elementFromPoint`。

**其他**：`WakeLock` 先檢查 `navigator.wakeLock`，沒有就跳過；`navigator.clipboard` 只在結果頁的按鈕用到。

**default 的 suite（20 個）**：TodoMVC-JavaScript-ES5、TodoMVC-JavaScript-ES6-Webpack-Complex-DOM、TodoMVC-WebComponents、TodoMVC-React-Complex-DOM、TodoMVC-React-Redux、TodoMVC-Backbone、TodoMVC-Angular-Complex-DOM、TodoMVC-Vue、TodoMVC-jQuery、TodoMVC-Preact-Complex-DOM、TodoMVC-Svelte-Complex-DOM、TodoMVC-Lit-Complex-DOM、NewsSite-Next、NewsSite-Nuxt、Editor-CodeMirror、Editor-TipTap、Charts-observable-plot、Charts-chartjs、React-Stockcharts-SVG、Perf-Dashboard。

**預設不跑的（`disabled: true`，12 個）**：TodoMVC 的 JavaScript-ES5-Complex-DOM、JavaScript-ES6-Webpack、WebComponents-Complex-DOM、React、React-Redux-Complex-DOM、Backbone-Complex-DOM、Angular、Vue-Complex-DOM、jQuery-Complex-DOM、Preact、Svelte、Lit。它們是 default 那些的另一種變體（有沒有 Complex-DOM），用 `suites=` 指定時可以跑；解開某個框架時，兩種都跑可以分出「框架的開銷」和「大的 DOM 帶來的 style、layout 的開銷」。

## kun 跑不起來的地方

讀 runner 和 TodoMVC-JavaScript-ES5 的原始碼得到的（2026-10-07），補盤點記不到的部分（例如 `location`）；盤點的結果見「對照表」。這一節記的是 RFC 開始時 kun 的樣子，出處是當時的 `docs/limitations.md` 和 `interfaces.rs`（RFC 0014 拆成 `crates/dom_api/src/interfaces/`）；每一項由哪份 RFC 做見 `README.md` 的「盤點的結果」，2026-10-16 的盤點裡 20 個 default suite 都跑得完。

**runner（任何 suite 之前）**：

| 用到的 | kun 當時 |
| --- | --- |
| `<script type="module">`、靜態 `import` | 不支援 module，只執行 parser 插入的 classic script |
| `element.style`（CSSOM 的 `CSSStyleDeclaration`） | 沒有 `style` |
| iframe 有內容、有 layout 和 paint，rendering update 包括子 frame | RFC 0009 的 2a：子 frame 不畫，也沒有 lifecycle；async 會少算子 frame 的 style、layout、paint |
| `getBoundingClientRect`、script 強制的同步 layout | 沒有；layout 只在 rendering update 裡做 |
| `performance.mark`、`measure` | `performance` 只有 `now()` |
| `URLSearchParams`、`location.search` | 沒有 `URL`、`URLSearchParams` |
| `position: absolute`、`left`、`top`、`transform` 的 iframe | 沒有 `position`；有 `transform` |
| `iframe.onload`、script 建立並設定 `src` 的 iframe | 有（RFC 0009 的 2a） |
| `requestAnimationFrame`、`setTimeout` | 有 |
| `document.elementFromPoint`（`Page.layout()`） | 沒有 |
| `alert`、`console.warn` / `error` / `assert` | 參數錯誤時才用到 `alert` |

**TodoMVC-JavaScript-ES5**（讀它的 `dist/`）：
- 資料放在記憶體裡（`MemoryStorage`），不用 `localStorage`。
- 畫 todo 用 `new DOMParser().parseFromString(html, "text/html")` 再 `replaceChildren(...body.childNodes)`：`DOMParser` 是 RFC 0010 第 1 階段，在 `rfc-0010-dom-api-next` 分支上，還沒併進 main。
- `<input>` 的 `value`、checkbox 的 `checked`；`focus()`、`blur` 事件、`dblclick`、`keypress` 的 `keyCode`；`dataset`；`element.style.display`；`className`；`hashchange` 和 `location.hash`；`autofocus`。
- 測試步驟：`setValue`、`dispatchEvent("change")`、`enter("keypress")`（`KeyboardEvent`）、對 `.toggle` 和 `.destroy` 呼叫 `click()`。
- 樣式：todomvc-common 的 `base.css`、`index.css`；畫面上的差別不影響有不有效，但影響 async 的時間（例如 checkbox 的背景圖是 `data:` 的 SVG，kun 沒有 SVG 圖片）。

其他 suite 要的功能見「對照表」（盤點的結果）。

## 盤點的做法

2026-10-07 的第一次盤點（「已決定的事項」第 3 條）。這一節記的是那一次的做法；2026-10-10 起工具放進 repo（`ci.py speedometer --inventory`，「分階段」的 1b），記錄的方式相同，不同的地方是：用 Chrome for Testing 152（第 11 條）；kun 有什麼由 kun 載入探測的頁面自己回答，不和宣告表、白名單比對（第 13 條）；還在 kun 上一次跑一個 default suite（見 `README.md` 的「盤點」）。下面「記不到或會多算的」對工具也成立。

**環境**：Windows 11 上的 Microsoft Edge 154（Chromium 154）`--headless=new`；Speedometer 是 `release/3.1` 的 `1386415`；`index.html?startAutomatically&iterationCount=1&tags=all`，32 個 suite 都跑（default 的 20 個加上預設不跑的 12 個），約 13 秒。同樣的設定跑兩次，缺的成員的清單相同。

**怎麼記錄**：
- 本機的 server（Python 標準函式庫）提供 Speedometer 的檔案，每個 HTML 頁面在 `<head>` 的最前面注入一段 classic script，`index.html` 在 `main.mjs` 之後再注入一個 module script。
- 記錄的 script 在頁面的 script 執行前，把 `window` 上每個 interface object 的 prototype 和 static 成員、`window` 自己的成員（`[Global]`）、`console` 換成包一層的版本：方法、getter、setter（記成 `member=`）各自計數；interface object 換成 `Proxy`，記下 `new`。包過的 function 的 `toString` 照舊回傳原本的（框架用它判斷是不是原生的）。`createElement`、`createElementNS`、`addEventListener`、`dispatchEvent`、`setAttribute`、`attachShadow`、`customElements.define`、`getContext` 等另外記下參數。每個 frame 各記一份，key 是那個 frame 的網址。
- runner 的 hook（`didRunTest`）在每個 test 之後走過 iframe 的 document 和 open shadow root：stylesheet（含 `adoptedStyleSheets`）的每條規則的 property、值、選擇器、at-rule；每個元素的 computed 的 `display`、`position` 等；`<script>` 的種類；`performance.getEntriesByType("resource")` 的資源。記錄的 script 自己的呼叫不算（記錄時關掉計數）。
- 結果 POST 回 server，再和當時的 `interfaces.rs` 的宣告表（main 和 `rfc-0010-dom-api-next` 分支）、`crates/css/src/properties.rs` 的白名單比對。`dom_js` 的 glue 給的（`performance.now`、`NodeList` 的 `forEach`）和 V8 的 `console` 算有。

**記不到或會多算的**：
- `[LegacyUnforgeable]` 的成員不能換：`window.location`、`document.location` 和 `Location` 的成員、`window.document`、`Event.isTrusted`。`location` 的用法改用 grep 各 suite 的 `dist/` 補上（「對照表」）。
- 用 symbol 當 key 的成員（`[Symbol.iterator]`）沒有記。
- runner 用自己的 realm 建立事件（`new KeyboardEvent(...)`），suite 讀這些事件的成員時記在 runner 那一份。
- 只記得到 Chromium 有的路徑：框架會先檢查功能在不在，kun 沒有時可能走另一條路（React 的 scheduler 沒有 `MessageChannel` 就用 `setTimeout`），也可能用到 Chromium 上沒走到的 API。
- 檢查功能在不在的讀取也算進去：Angular 的 zone.js 為了 patch 讀了 `FileReader`、`IntersectionObserver`、`navigator.geolocation` 的成員，不代表 app 用到。
- kun 有、但行為和 Chromium 不同的成員，比對看不出來。
- stylesheet 的 shorthand 被展開成 longhand（`background` 算成 8 個 property、`font` 算成十幾個），property 的數字偏多。
- 盤點時 Chromium 沒有丟例外；TodoMVC-WebComponents-Complex-DOM（預設不跑）丟了 `this.bottombar.updateRoute is not a function`，還沒確認是記錄的 script 造成的，還是它本來就有問題。

## 對照表

2026-10-07 第一次盤點的結果（「盤點的做法」），也就是 RFC 開始時 kun 缺的東西；負責的 RFC 見 `README.md` 的「盤點的結果」，之後每次重跑的結果在 `progress.md`。只列 default 的 20 個 suite 和 runner；「T-」是 TodoMVC-。標了 `*` 的在 RFC 0010。

### 每個 suite 缺的功能

由記錄到的成員分組而來；另外，`location`（盤點記不到，grep `dist/` 得到）TodoMVC 的 12 個和 NewsSite 的 2 個都用到。每個 suite 也都要 runner 要的功能（`README.md` 的「盤點的結果」）。

| suite | 缺的功能 |
| --- | --- |
| TodoMVC-JavaScript-ES5 | KeyboardEvent、CSSOM、click/focus、表單、HTML 元素的成員、innerHTML/DOMParser* |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | KeyboardEvent、CSSOM、click/focus、表單、HTML 元素的成員、innerHTML/DOMParser* |
| TodoMVC-WebComponents | module、KeyboardEvent、CSSOM、adoptedStyleSheets、click/focus、表單、HTML 元素的成員、template、Document 的成員、isConnected、innerHTML/DOMParser*、shadow DOM/custom element |
| TodoMVC-React-Complex-DOM | KeyboardEvent、CSSOM、history、URL、MouseEvent、click/focus、表單、HTML 元素的成員、Selection、MessageChannel |
| TodoMVC-React-Redux | KeyboardEvent、CSSOM、history、navigator、MouseEvent、click/focus、表單、HTML 元素的成員、Selection、MessageChannel |
| TodoMVC-Backbone | KeyboardEvent、CSSOM、offset/client/scroll、history、MouseEvent、click/focus、表單、HTML 元素的成員、Document 的成員、innerHTML/DOMParser*、fetch/XHR、opener |
| TodoMVC-Angular-Complex-DOM | module、KeyboardEvent、history、Performance、navigator、click/focus、表單、MutationObserver*、IntersectionObserver、shadow DOM/custom element、fetch/XHR |
| TodoMVC-Vue | KeyboardEvent、CSSOM、history、navigator、click/focus、表單、HTML 元素的成員、Document 的成員、innerHTML/DOMParser* |
| TodoMVC-jQuery | KeyboardEvent、CSSOM、offset/client/scroll、history、MouseEvent、click/focus、表單、HTML 元素的成員、Document 的成員、innerHTML/DOMParser*、fetch/XHR、onhashchange、opener |
| TodoMVC-Preact-Complex-DOM | KeyboardEvent、click/focus、表單 |
| TodoMVC-Svelte-Complex-DOM | KeyboardEvent、click/focus、表單、HTML 元素的成員 |
| TodoMVC-Lit-Complex-DOM | module、KeyboardEvent、adoptedStyleSheets、click/focus、表單、template、isConnected、innerHTML/DOMParser*、TreeWalker*、shadow DOM/custom element、trustedTypes |
| NewsSite-Next | CSSOM、offset/client/scroll、視窗大小、history、URL、Performance、navigator、MouseEvent、click/focus、表單、HTML 元素的成員、img、Document 的成員、isConnected、queueMicrotask*、Selection、MessageChannel、crypto |
| NewsSite-Nuxt | module、動態 import、CSSOM、offset/client/scroll、視窗大小、history、URL、navigator、click/focus、表單、HTML 元素的成員、img、Document 的成員、innerHTML/DOMParser*、requestIdleCallback、crypto |
| Editor-CodeMirror | module、動態 import、CSSOM、offset/client/scroll、視窗大小、navigator、click/focus、HTML 元素的成員、MutationObserver*、Range*、Selection、ResizeObserver、IntersectionObserver、shadow DOM/custom element |
| Editor-TipTap | module、動態 import、CSSOM、offset/client/scroll、navigator、click/focus、HTML 元素的成員、innerHTML/DOMParser*、MutationObserver*、Range*、Selection、shadow DOM/custom element |
| Charts-observable-plot | module、動態 import、視窗大小、click/focus、表單 |
| Charts-chartjs | module、動態 import、CSSOM、offset/client/scroll、視窗大小、MouseEvent、click/focus、表單、isConnected、MutationObserver*、ResizeObserver、shadow DOM/custom element、canvas |
| React-Stockcharts-SVG | CSSOM、offset/client/scroll、視窗大小、URL、MouseEvent、click/focus、表單、HTML 元素的成員、Selection、MessageChannel、canvas、SVG DOM |
| Perf-Dashboard | KeyboardEvent、CSSOM、offset/client/scroll、視窗大小、navigator、MouseEvent、表單、template、Document 的成員、innerHTML/DOMParser*、shadow DOM/custom element、canvas、fetch/XHR、onhashchange |

**script 的載入**：用 module script 的有 runner 和 8 個 suite（TodoMVC-WebComponents、Lit-Complex-DOM、Angular-Complex-DOM，NewsSite-Nuxt，Editor-CodeMirror、TipTap，Charts-chartjs、observable-plot）；其中 5 個另外用動態 `import()` 或 `modulepreload`。NewsSite-Next 用 script 建立 `<script>`（Next.js 載入 chunk）。NewsSite-Next、Nuxt 用 `<script type="application/json">` 放資料，Backbone、jQuery 用 `text/template` 的 `<script>` 放模板（讀 `textContent`，不執行）。Perf-Dashboard 用 `fetch` 載入 JSON。

### 每項功能缺的成員

「suite 數」不含 runner。

| 功能 | suite 數 | 缺的成員 |
| --- | --- | --- |
| CSSOM：element.style、getComputedStyle | runner + 15 | `CSSStyleDeclaration.cssText`、`CSSStyleDeclaration.cssText=`、`CSSStyleDeclaration.getPropertyValue`、`CSSStyleDeclaration.setProperty`、`HTMLElement.style`、`SVGElement.style`、`Window.getComputedStyle` |
| constructable stylesheet、adoptedStyleSheets | 2 | `CSSStyleSheet.constructor`、`CSSStyleSheet.replaceSync`、`ShadowRoot.adoptedStyleSheets=` |
| 幾何：getBoundingClientRect、DOMRect | runner + 20 | `DOMRect.height`、`DOMRect.width`、`DOMRect.x`、`DOMRect.y`、`DOMRectList.length`、`DOMRectReadOnly.bottom`、`DOMRectReadOnly.height`、`DOMRectReadOnly.left`、`DOMRectReadOnly.right`、`DOMRectReadOnly.top`、`DOMRectReadOnly.width`、`Element.getBoundingClientRect` |
| 幾何：offset*/client*/scroll*、elementFromPoint、scrollTo | runner + 9 | `Document.elementFromPoint`、`Element.clientHeight`、`Element.clientLeft`、`Element.clientTop`、`Element.clientWidth`、`Element.scrollHeight`、`Element.scrollLeft`、`Element.scrollTo`、`Element.scrollTop`、`Element.scrollTop=`、`Element.scrollWidth`、`HTMLElement.offsetHeight`、`HTMLElement.offsetWidth`、`Window.pageXOffset`、`Window.pageYOffset`、`Window.scrollTo` |
| Window 的大小：innerWidth/Height、devicePixelRatio、screenX/Y | runner + 7 | `Window.devicePixelRatio`、`Window.innerHeight`、`Window.innerWidth`、`Window.screenX`、`Window.screenY` |
| History（pushState、state） | runner + 8 | `History.length`、`History.pushState`、`History.replaceState`、`History.scrollRestoration=`、`History.state`、`Window.history` |
| URL、URLSearchParams | runner + 4 | `URL.constructor`、`URL.createObjectURL`、`URL.hash`、`URL.pathname`、`URL.search`、`URLSearchParams.constructor`、`URLSearchParams.delete`、`URLSearchParams.get`、`URLSearchParams.has`、`URLSearchParams.keys`、`URLSearchParams.toString` |
| Performance（mark、measure、Observer、timing） | runner + 2 | `Performance.clearMarks`、`Performance.getEntriesByName`、`Performance.getEntriesByType`、`Performance.interactionCount`、`Performance.mark`、`Performance.measure`、`PerformanceEntry.name`、`PerformanceEntry.startTime`、`PerformanceNavigationTiming.activationStart`、`PerformanceNavigationTiming.type`、`PerformanceObserver.constructor`、`PerformanceObserver.disconnect`、`PerformanceObserver.observe`、`PerformanceObserver.supportedEntryTypes`、`PerformanceObserver.takeRecords`、`PerformanceObserverEntryList.getEntries`、`PerformanceResourceTiming.responseStart` |
| Navigator | runner + 8 | `Navigator.connection`、`Navigator.geolocation`、`Navigator.hardwareConcurrency`、`Navigator.platform`、`Navigator.scheduling`、`Navigator.userAgent`、`Navigator.vendor`、`Navigator.wakeLock`、`WakeLock.request`、`WakeLockSentinel.release`、`Window.navigator` |
| KeyboardEvent | runner + 13（事件由 runner 建立，見下） | `KeyboardEvent.altKey`、`KeyboardEvent.code`、`KeyboardEvent.constructor`、`KeyboardEvent.ctrlKey`、`KeyboardEvent.key`、`KeyboardEvent.keyCode`、`KeyboardEvent.location`、`KeyboardEvent.metaKey`、`KeyboardEvent.repeat`、`KeyboardEvent.shiftKey` |
| MouseEvent、WheelEvent、UIEvent 的成員 | runner + 8 | `MouseEvent.buttons`、`MouseEvent.fromElement`、`MouseEvent.movementX`、`MouseEvent.movementY`、`MouseEvent.offsetX`、`MouseEvent.offsetY`、`MouseEvent.pageX`、`MouseEvent.pageY`、`MouseEvent.relatedTarget`、`MouseEvent.screenX`、`MouseEvent.screenY`、`UIEvent.detail`、`UIEvent.view`、`UIEvent.which`、`WheelEvent.constructor`、`WheelEvent.deltaMode`、`WheelEvent.deltaX`、`WheelEvent.deltaY`、`WheelEvent.deltaZ` |
| click()、focus()、activeElement、FocusEvent | 19 | `Document.activeElement`、`Document.hasFocus`、`Document.onfocus`、`FocusEvent.relatedTarget`、`HTMLBodyElement.onfocus`、`HTMLElement.click`、`HTMLElement.focus`、`HTMLElement.onfocus`、`Window.onfocus` |
| 表單：input、button、select、option、textarea、label、fieldset | 18 | `HTMLButtonElement.disabled`、`HTMLButtonElement.disabled=`、`HTMLFieldSetElement.disabled=`、`HTMLInputElement.autocomplete=`、`HTMLInputElement.checked`、`HTMLInputElement.checked=`、`HTMLInputElement.defaultChecked=`、`HTMLInputElement.defaultValue`、`HTMLInputElement.defaultValue=`、`HTMLInputElement.disabled`、`HTMLInputElement.disabled=`、`HTMLInputElement.max=`、`HTMLInputElement.min=`、`HTMLInputElement.name`、`HTMLInputElement.placeholder=`、`HTMLInputElement.selectionEnd`、`HTMLInputElement.selectionStart`、`HTMLInputElement.step=`、`HTMLInputElement.type`、`HTMLInputElement.type=`、`HTMLInputElement.value`、`HTMLInputElement.value=`、`HTMLLabelElement.htmlFor=`、`HTMLOptionElement.selected`、`HTMLOptionElement.selected=`、`HTMLOptionElement.value`、`HTMLOptionsCollection.length`、`HTMLSelectElement.disabled=`、`HTMLSelectElement.multiple=`、`HTMLSelectElement.options`、`HTMLSelectElement.value=`、`HTMLTextAreaElement.defaultValue`、`Option.selected`、`Option.selected=`、`Option.value` |
| HTML 元素的其他成員：dataset、a.href、title、tabIndex、autofocus、contentEditable | runner + 14 | `HTMLAnchorElement.href=`、`HTMLAnchorElement.rel`、`HTMLAnchorElement.rel=`、`HTMLElement.autofocus`、`HTMLElement.autofocus=`、`HTMLElement.contentEditable`、`HTMLElement.contentEditable=`、`HTMLElement.dataset`、`HTMLElement.tabIndex=`、`HTMLElement.title=`、`HTMLLinkElement.relList`、`HTMLScriptElement.textContent`、`HTMLScriptElement.textContent=` |
| <template>.content | 3 | `HTMLTemplateElement.content` |
| <img>、Image | 2 | `HTMLImageElement.alt=`、`HTMLImageElement.complete`、`HTMLImageElement.decode`、`HTMLImageElement.height=`、`HTMLImageElement.src=`、`HTMLImageElement.width=`、`Image.alt=`、`Image.complete`、`Image.decode`、`Image.height=`、`Image.src=`、`Image.width=` |
| Document 的其他成員：title、readyState、getElementsByName、dir、all、visibilityState | runner + 7 | `Document.all`、`Document.dir`、`Document.getElementsByName`、`Document.prerendering`、`Document.readyState`、`Document.title`、`Document.title=`、`Document.visibilityState` |
| Node.isConnected | 4 | `Node.isConnected` |
| innerHTML、DOMParser（RFC 0010 第 1 階段） | runner + 10 | `DOMParser.constructor`、`DOMParser.parseFromString`、`Element.innerHTML`、`Element.innerHTML=` |
| MutationObserver（RFC 0010 第 2 階段） | 4 | `MutationObserver.constructor`、`MutationObserver.disconnect`、`MutationObserver.observe`、`MutationObserver.takeRecords`、`WebKitMutationObserver.constructor`、`WebKitMutationObserver.disconnect`、`WebKitMutationObserver.observe`、`WebKitMutationObserver.takeRecords` |
| queueMicrotask（RFC 0010 第 2 階段） | 1 | `Window.queueMicrotask` |
| TreeWalker（RFC 0010 第 4 階段） | 1 | `Document.createTreeWalker`、`TreeWalker.currentNode=`、`TreeWalker.nextNode` |
| Range（RFC 0010 第 5 階段） | 2 | `DOMRectList.length`、`Document.createRange`、`Range.getClientRects`、`Range.setEnd`、`Range.setStart` |
| Selection、contenteditable | 6 | `Document.getSelection`、`HTMLElement.contentEditable`、`HTMLElement.contentEditable=`、`Selection.anchorNode`、`Selection.anchorOffset`、`Selection.collapse`、`Selection.focusNode`、`Selection.focusOffset` |
| MessageChannel、MessagePort | 4 | `MessageChannel.constructor`、`MessageChannel.port1`、`MessageChannel.port2`、`MessagePort.onmessage=`、`MessagePort.postMessage` |
| requestIdleCallback | 1 | `Window.requestIdleCallback` |
| ResizeObserver | 2 | `ResizeObserver.constructor`、`ResizeObserver.disconnect`、`ResizeObserver.observe`、`ResizeObserverEntry.contentRect` |
| IntersectionObserver | 2 | `IntersectionObserver.constructor`、`IntersectionObserver.delay`、`IntersectionObserver.disconnect`、`IntersectionObserver.observe`、`IntersectionObserver.root`、`IntersectionObserver.rootMargin`、`IntersectionObserver.scrollMargin`、`IntersectionObserver.thresholds`、`IntersectionObserver.trackVisibility`、`IntersectionObserverEntry.intersectionRatio` |
| custom element、shadow DOM、slot | 7 | `CustomElementRegistry.define`、`Element.assignedSlot`、`Element.attachShadow`、`Element.shadowRoot`、`HTMLElement.constructor`、`Text.assignedSlot`、`Window.customElements` |
| Trusted Types | 1 | `TrustedTypePolicy.createHTML`、`TrustedTypePolicyFactory.createPolicy`、`TrustedTypePolicyFactory.emptyScript`、`Window.trustedTypes` |
| Canvas 2D | 3 | `CanvasRenderingContext2D.arc`、`CanvasRenderingContext2D.beginPath`、`CanvasRenderingContext2D.canvas`、`CanvasRenderingContext2D.clearRect`、`CanvasRenderingContext2D.clip`、`CanvasRenderingContext2D.closePath`、`CanvasRenderingContext2D.fill`、`CanvasRenderingContext2D.fillStyle=`、`CanvasRenderingContext2D.fillText`、`CanvasRenderingContext2D.font=`、`CanvasRenderingContext2D.globalAlpha=`、`CanvasRenderingContext2D.lineDashOffset=`、`CanvasRenderingContext2D.lineJoin=`、`CanvasRenderingContext2D.lineTo`、`CanvasRenderingContext2D.lineWidth=`、`CanvasRenderingContext2D.measureText`、`CanvasRenderingContext2D.moveTo`、`CanvasRenderingContext2D.quadraticCurveTo`、`CanvasRenderingContext2D.rect`、`CanvasRenderingContext2D.resetTransform`、`CanvasRenderingContext2D.restore`、`CanvasRenderingContext2D.rotate`、`CanvasRenderingContext2D.save`、`CanvasRenderingContext2D.scale`、`CanvasRenderingContext2D.setLineDash`、`CanvasRenderingContext2D.setTransform`、`CanvasRenderingContext2D.stroke`、`CanvasRenderingContext2D.strokeStyle=`、`CanvasRenderingContext2D.textAlign=`、`CanvasRenderingContext2D.textBaseline=`、`CanvasRenderingContext2D.translate`、`HTMLCanvasElement.getContext`、`HTMLCanvasElement.height`、`HTMLCanvasElement.height=`、`HTMLCanvasElement.width`、`HTMLCanvasElement.width=`、`TextMetrics.width` |
| SVG 的 DOM（SVGElement 的子 interface） | 1 | `SVGElement.onclick=`、`SVGElement.ownerSVGElement`、`SVGElement.style`、`SVGGraphicsElement.getScreenCTM`、`SVGMatrix.inverse`、`SVGPoint.matrixTransform`、`SVGPoint.x`、`SVGPoint.x=`、`SVGPoint.y`、`SVGPoint.y=`、`SVGSVGElement.createSVGPoint` |
| fetch、Response、XMLHttpRequest、FileReader、Blob | runner + 4 | `Blob.constructor`、`FileReader.constructor`、`FileReader.error`、`FileReader.onabort`、`FileReader.onerror`、`FileReader.onload`、`FileReader.onloadend`、`FileReader.onloadstart`、`FileReader.onprogress`、`FileReader.readyState`、`FileReader.result`、`Response.text`、`URL.createObjectURL`、`Window.fetch`、`XMLHttpRequest.constructor` |
| crypto.randomUUID | 2 | `Crypto.randomUUID`、`Window.crypto` |
| hashchange 的 handler、HashChangeEvent | 2 | `HashChangeEvent.newURL`、`Window.onhashchange`、`Window.onhashchange=` |
| 其他 Window 的成員：opener、event handler | 2 | `Window.onfocus`、`Window.opener` |

另外記到、沒有分組的：`HTMLElement` 上的 event handler 屬性（`onchange`、`oninput`、`onmouseenter`、`onmouseleave`、`onsubmit`；`onclick`、`onload` kun 放在 `Element` 上）、`DocumentFragment.getElementById`、`HTMLProgressElement` 的 `max`、`value`（runner 的進度條）、`NetworkInformation`（Nuxt，有檢查）。

**事件**：suite 註冊 listener 的事件裡，kun 現在不會自己分派的有 `input`、`change`、`dblclick`、`keydown`、`keyup`、`keypress`、`focus`、`blur`、`hashchange`、`popstate`、`selectionchange`、`beforeinput`、`composition*`、`mouse*`、`pointer*`、`touch*`、`wheel`、`scroll`、`resize`、`transitionend`、`animation*` 等。Speedometer 的步驟大多自己分派（`dispatchEvent`），所以「跑得完」主要要的是事件的建構子和預設的動作：`click()` 讓 checkbox 改 `checked` 再分派 `input`、`change`；`focus()` 改 `activeElement`、分派 `focus`、`blur`；設定 `location.hash` 分派 `hashchange`。runner 分派的有 `KeyboardEvent` 的 `keydown`（6 個 suite）、`keyup`（4）、`keypress`（3），`Event` 的 `change`、`input`（各 5），`MouseEvent` 的 `mousemove`（3）、`mousedown`、`mouseup`（各 2），`WheelEvent`（1），和 suite 自己的 `CustomEvent`。

**元素**：`createElement` 建立的有 `input`、`label`、`button`、`select`、`option`、`fieldset`、`template`、`canvas`、`img`、`style`、`script`、`table` 一族，和 custom element（WebComponents、Lit、Angular、Perf-Dashboard）；`createElementNS` 建立的 SVG 有 `svg`、`path`、`g`、`line`、`text`、`tspan`、`rect`、`circle`、`clipPath`、`defs`、`title`、`style`。12 個 suite 的頁面裡有 inline 的 `<svg>`。`attachShadow` 的 `mode` 有 `open`（WebComponents、Lit）和 `closed`（Perf-Dashboard）。

### CSS

當時 kun 的白名單（main）沒有的 property（括號是用到它的 default suite 數；shorthand 展開成 longhand）：`background-attachment`（18）、`background-clip`（18）、`background-image`（18）、`background-origin`（18）、`background-position-x`（18）、`background-position-y`（18）、`background-repeat`（18）、`background-size`（18）、`position`（17）、`border-image-outset`（16）、`border-image-repeat`（16）、`border-image-slice`（16）、`border-image-source`（16）、`border-image-width`（16）、`content`（16）、`cursor`（16）、`text-decoration-color`（16）、`text-decoration-style`（16）、`text-decoration-thickness`（16）、`top`（16）、`border-bottom-left-radius`（15）、`border-bottom-right-radius`（15）、`border-top-left-radius`（15）、`border-top-right-radius`（15）、`bottom`（15）、`font-feature-settings`（15）、`font-variant-ligatures`（15）、`left`（15）、`list-style-type`（15）、`right`（15）、`vertical-align`（15）、`appearance`（14）、`box-shadow`（14）、`font-kerning`（14）、`font-language-override`（14）、`font-optical-sizing`（14）、`font-size-adjust`（14）、`font-stretch`（14）、`font-variant-alternates`（14）、`font-variant-caps`（14）、`font-variant-east-asian`（14）、`font-variant-emoji`（14）、`font-variant-numeric`（14）、`font-variant-position`（14）、`font-variation-settings`（14）、`pointer-events`（14）、`text-rendering`（14）、`text-wrap-mode`（14）、`transition-behavior`（14）、`transition-delay`（14）、`transition-duration`（14）、`transition-property`（14）、`transition-timing-function`（14）、`white-space-collapse`（14）、`list-style-image`（13）、`list-style-position`（13）、`outline-color`（13）、`outline-style`（13）、`outline-width`（13）、`text-shadow`（12）、`word-break`（12）、`clip-path`（11）、`float`（11）、`clip`（10）、`overflow-wrap`（10）、`text-overflow`（10）、`fill`（8）、`grid-template-columns`（8）、`inset-inline-end`（8）、`inset-inline-start`（8）、`margin-inline-end`（8）、`margin-inline-start`（8）、`padding-inline-start`（8）、`text-transform`（8）、`visibility`（8）、`user-select`（7）、`block-size`（6）、`border-block-start-color`（6）、`border-block-start-width`（6）、`border-collapse`（6）、`border-end-end-radius`（6）、`border-end-start-radius`（6）、`border-inline-end-width`（6）、`border-start-end-radius`（6）、`border-start-start-radius`（6）、`filter`（6）、`forced-color-adjust`（6）、`grid-auto-flow`（6）、`grid-column-end`（6）、`grid-column-start`（6）、`grid-row-end`（6）、`grid-row-start`（6）、`grid-template-rows`（6）、`hyphens`（6）、`inline-size`（6）、`inset-block-end`（6）、`inset-block-start`（6）、`justify-items`（6）、`justify-self`（6）、`letter-spacing`（6）、`margin-block-end`（6）、`margin-block-start`（6）、`max-block-size`（6）、`max-inline-size`（6）、`min-block-size`（6）、`min-inline-size`（6）、`outline-offset`（6）、`padding-block-end`（6）、`padding-block-start`（6）、`padding-inline-end`（6）、`resize`（6）、`table-layout`（6）、`text-indent`（6）、`animation-delay`（4）、`animation-duration`（4）、`animation-iteration-count`（4）、`isolation`（4）、`animation-direction`（2）、`animation-fill-mode`（2）、`animation-name`（2）、`animation-play-state`（2）、`animation-range-end`（2）、`animation-range-start`（2）、`animation-timeline`（2）、`animation-timing-function`（2）、`caret-color`（2）、`font-synthesis-small-caps`（2）、`font-synthesis-style`（2）、`font-synthesis-weight`（2）、`object-fit`（2）、`object-position`（2）、`scroll-behavior`（2）、`text-size-adjust`（2）、`transform-origin`（2）、`contain`（1）、`tab-size`（1）。

`-webkit-` 的 property：`-webkit-font-smoothing`（12）、`-webkit-border-horizontal-spacing`（6）、`-webkit-border-vertical-spacing`（6）、`-webkit-tap-highlight-color`（6）、`-webkit-text-fill-color`（6）、`-webkit-box-orient`（2）、`-webkit-line-clamp`（2）、`-webkit-user-modify`（1）。

property 的值（括號是 suite 數；當時 kun 的 `display` 只有 `block`、`inline`、`flex`、`none`，`position`、`float`、`vertical-align` 都還沒有）：`display: block`（17）、`box-sizing: border-box`（16）、`display: flex`（16）、`display: none`（16）、`position: absolute`（16）、`position: relative`（16）、`display: inline-block`（15）、`list-style-type: none`（15）、`overflow-x: hidden`（15）、`overflow-y: hidden`（15）、`vertical-align: baseline`（14）、`display: inline`（13）、`float: left`（11）、`float: right`（11）、`vertical-align: middle`（11）、`overflow-x: auto`（10）、`overflow-y: auto`（10）、`text-overflow: ellipsis`（10）、`display: grid`（8）、`display: inline-flex`（8）、`box-sizing: content-box`（7）、`vertical-align: bottom`（7）、`vertical-align: top`（7）、`visibility: hidden`（7）、`display: inline-grid`（6）、`overflow-x: visible`（6）、`overflow-y: scroll`（6）、`overflow-y: visible`（6）、`position: static`（6）、`visibility: unset`（6）、`visibility: visible`（6）、`position: sticky`（3）、`display: -webkit-box`（2）、`display: list-item`（2）、`list-style-type: disc`（2）、`position: fixed`（2）、`vertical-align: text-top`（1）。

選擇器：`::after`（16）、`:hover`（16）、`::before`（15）、`+`（14）、`:checked`（14）、`:focus`（14）、`:last-child`（14）、`:active`（13）、`>`（12）、`*`（11）、`::-webkit-input-placeholder`（11）、`[attr]`（11）、`:root`（10）、`:first-child`（9）、`:not(`（9）、`:nth-child(`（8）、`~`（8）、`::placeholder`（7）、`::-ms-clear`（6）、`::-webkit-inner-spin-button`（6）、`::-webkit-outer-spin-button`（6）、`::-webkit-search-cancel-button`（6）、`::-webkit-search-decoration`（6）、`:disabled`（6）、`:empty`（6）、`:enabled`（6）、`:focus-visible`（6）、`:lang(`（6）、`:only-child`（6）、`:read-only`（6）、`::selection`（2）、`:host`（2）、`:host(`（2）、`:target`（2）、`:placeholder-shown`（1）。

函式：`rgba()`（15）、`rgb()`（14）、`rotate()`（14）、`calc()`（13）、`url()`（13）、`inset()`（10）、`rect()`（10）、`translateY()`（9）、`scale()`（8）、`translate()`（8）、`var()`（8）、`cubic-bezier()`（6）、`drop-shadow()`（6）、`hsla()`（6）、`minmax()`（6）、`translateX()`（6）、`attr()`（3）、`steps()`（2）、`linear-gradient()`（1）、`min()`（1）。

at-rule：`@media` 在 14 個 suite；`CSSKeyframeRule`（8）、`CSSKeyframesRule`（8）、`@media (max-width: 430px)`（12）、`@media screen and (-webkit-min-device-pixel-ratio: 0)`（12）、`@media (-ms-high-contrast:none), screen and (-ms-high-contrast:active)`（6）、`@media (forced-colors: active)`（6）、`@media (min-width: 899px)`（4）、`@media (max-width: 767px)`（2）、`@media (min-width: 768px) and (max-width: 1199px)`（2）、`@media (prefers-reduced-motion: reduce)`（2）。

自訂 property（`--x` 和 `var()`）：TodoMVC 的 Complex-DOM 系列每個約 7500 個宣告（Lit-Complex-DOM 約 9600），NewsSite 各約 430 個。constructable stylesheet：WebComponents、Lit-Complex-DOM 的每個 shadow root 都用 `adoptedStyleSheets`。

當時 RFC 0003 第 3 階段（grid）還沒提交，它的工作目錄裡另外有 `grid-*`、`justify-items`、`justify-self`。

## 公平的條件

| 條件 | 做法 | 理由 |
| --- | --- | --- |
| 同一份 Speedometer | 兩邊讀同一個 server 提供的同一份檔案、同樣的 URL 參數 | browserbench.org 的版本會變；自己提供才能注入 |
| 注入相同 | 兩邊注入同樣的 script | 注入的 script 也要執行，兩邊一樣才不影響比較 |
| V8 的世代 | Chrome for Testing 152，對應 kun 的 V8 15.2 | JS 的執行時間是 Speedometer 的大部分；同一個世代，差距才主要來自 bindings、DOM 和 rendering |
| V8 的設定 | 記下差別：Chrome 開 V8 sandbox 和 pointer compression，kun 只開 pointer compression；量時間時 `MACHOPO_V8_FLAGS` 不設（`--sample` 才加 `--perf-basic-prof` 這些） | sandbox 有一點開銷，在 Chrome 那邊 |
| kun 的 build | `perf` profile：繼承 release，`lto = "thin"`、`codegen-units = 1`、`debug = "line-tables-only"`，沒有 debug assertion；Linux 和 macOS 上加 frame pointer | Chrome 是 ThinLTO 加 PGO 的 official build，在 Linux 和 macOS 上也留著 frame pointer；kun 的 release 沒有 LTO，kun 沒有 PGO；行號表和 frame pointer 讓取樣看得到完整的呼叫鏈，取樣的就是量時間的那個 binary |
| frame 的節奏 | kun 用每 16.7 ms 送 `BeginFrame` 的沒有平台視窗的瀏覽器視窗；Chrome 用 `--headless=new` | kun 的 headless 一回答就送下一個 frame，rAF 會比 60 Hz 快；兩邊都是真的 60 Hz，不另外量兩種節奏（第 30 條） |
| 事件的順序 | 確認 rAF 的 callback 之後、同一個 task 裡做 rendering update，`setTimeout(0)` 在它之後 | kun 的 `begin_main_frame` 已經是這樣；async 的時間才包括 style、layout、paint |
| timer 的 clamp | 記下兩邊對巢狀 `setTimeout(0)` 的 clamp | Speedometer 每一步都用 `setTimeout(0)`；clamp 不同時，等待的時間不算進 sync 和 async，但影響總共跑多久 |
| raster 和合成 | 記下差別：kun 用 Skia 的 CPU raster，在 compositor thread；Chrome headless 用 GPU 或 SwiftShader | raster 不在 main thread，不算進 async；但 CPU 的競爭會影響 main thread |
| 網路 | 本機的 server；記下差別：kun 沒有 HTTP cache | 載入不算進量測的時間（`measurePrepare` 預設關），只影響總共跑多久 |
| 機器 | 同一台、接電源、同樣的電源計畫、關掉其他程式；每個 suite 兩邊接著跑、每次新的 process | 減少溫度和背景工作的漂移 |
| 統計 | 每個 suite、每個 test：所有 iteration（第一個也算）的中位數和不假設分布的 95% 信賴區間；比值用中位數 | Speedometer 自己的平均容易被少數慢的 iteration 拉高 |

## Chrome 的 driver

**CDP**（Chrome DevTools Protocol）是 DevTools 用來控制 Chrome 的協定：Chrome 用 `--remote-debugging-port` 啟動後，`http://127.0.0.1:<port>/json/version` 給出一個 WebSocket 的網址，之後在上面送 JSON 的命令（`{"id": 1, "method": "Page.navigate", "params": {...}}`），收到同樣 `id` 的回應和沒有 `id` 的事件；`--remote-debugging-pipe` 改用 fd 3、4。Puppeteer、Playwright、crossbench、ChromeDriver 都建在它上面。Chromium 這邊的實作在 `content/browser/devtools/`，協定的定義是 `third_party/blink/public/devtools_protocol/browser_protocol.pdl`。kun 的 `--automation-pipe` 用的是同樣格式的訊息（`protocol::automation`），但只有自動化要的幾個命令。

量測要的只有「開頁面、等結果」，比較過的做法（第 14 條）：

| 做法 | 優點 | 缺點 |
| --- | --- | --- |
| **R1. 頁面自己把結果 `POST` 回 server**（選了） | 兩邊同一段注入的程式、同一條路；Chrome 只要把網址放在命令列上，kun 用現有的 `--automation-pipe` 開頁面；`ci.py` 只用標準函式庫 | 要 kun 有 `fetch`（RFC 0027 之後有了） |
| R2. 兩邊都走 CDP over pipe | 一種 driver | 讀結果的方法兩邊不同；Python 要接 fd 3、4 |
| R3. chromedriver 和 machopodriver（WebDriver） | 現成的協定 | 要多下載 chromedriver，量測用不到 WebDriver 的其他東西，也多一層開銷 |
| R4. 原本的設計：Chrome 自己寫 CDP 的 WebSocket client（`Runtime.evaluate` 讀注入的 script 放的全域變數），kun 加 `testRunner.reportBenchmarkResults` 和 `--run-benchmark` | 不用 kun 的 `fetch` | 兩邊交結果的路不同；kun 要加選項；它的理由（kun 的頁面送不出結果）在 RFC 0027 之後不成立 |
| crossbench | Chromium 的 perf bots 用的工具，有 Speedometer 3.1 的 story，也能錄 trace | 它決定 Speedometer 的網址和參數；要用同一份、同樣的注入，還是要自己接；依賴它的 Python 套件 |
| Puppeteer | 成熟的 CDP client | 多一個 Node 的依賴；`ci.py` 是 Python |

crossbench 可以用來交叉確認：同一台機器上，它量到的 Chrome 的數字應該和工具量到的接近。Chrome 的 trace 也不用 CDP 的 `Tracing`：用啟動的旗標寫檔（第 35 條，`README.md` 的「trace」）。

## 參考資料

- Speedometer：`https://github.com/WebKit/Speedometer`（`release/3.1`），`https://browserbench.org/Speedometer3.1/`。
- Speedometer 3 的說明：`https://browserbench.org/Speedometer3.1/about.html`。
- crossbench：Chromium 的 `third_party/crossbench`。
- Chrome for Testing：`https://googlechromelabs.github.io/chrome-for-testing/`。
- Chrome DevTools Protocol：`https://chromedevtools.github.io/devtools-protocol/`。
