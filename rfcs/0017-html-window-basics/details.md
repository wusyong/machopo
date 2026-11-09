# RFC 0017 的細節：HTML 元素和 Window 的雜項（H1）

這份是 `README.md` 的補充，放支撐它的細節：M1 實際用到什麼的核對、每一項和 Chromium 的對照、選項的完整比較、WPT 的調查。`README.md` 裡的看法以那邊為準，這裡不重複。

## M1 的核對

2026-10-07 讀 Speedometer `release/3.1` 的 `1386415`（RFC 0012 的「已決定的事項」第 1 條）的原始碼：runner 的 `main.mjs`、`benchmark-runner.mjs`、`tests.mjs`（都在 Speedometer 的 `resources` 底下），和三個 suite 的 `dist/`。範圍的出發點是 `rfcs/0012-speedometer/README.md` 的「盤點的結果」和 `rfcs/0012-speedometer/details.md` 的「每項功能缺的成員」；盤點記的是 Chromium 上呼叫過的成員，這裡再對原始碼確認「沒有它時會怎樣」。層級沿用 RFC 0012 的分法：跑得完（沒有它會丟例外、卡住）、行為對（沒有它不丟例外但做錯事）。

### runner

| 成員 | 在哪裡、怎麼用 | 沒有時 | 層級 |
| --- | --- | --- | --- |
| `navigator` | `benchmark-runner.mjs` 的 `WakeLock.request()`：`if (!navigator.wakeLock) return;`，這一行在 `try` 外面；`_runAllSuites` 一開始 `await` 它 | `ReferenceError`，`_runAllSuites` 的 promise 被 reject，整個 run 停下 | 跑得完 |
| `navigator.wakeLock` | 同上；有的話 `request("screen")`，失敗只 `console.error` | **要沒有**（`undefined`）：runner 就跳過。kun 不做 Screen Wake Lock | — |
| `navigator.clipboard` | `main.mjs` 結果頁的「複製」按鈕 | 只有使用者按按鈕才用到 | 不需要 |
| `performance.mark(name)` | 每個 test 4 個（sync 的開始和結束、async 的開始和結束），每個 suite、iteration、prepare、finalize 各 2 個；名稱都是字串，回傳值沒用到 | `TypeError`（`performance.mark is not a function`） | 跑得完 |
| `performance.measure(name, startMark, endMark)` | 每個 test 2 個（`<suite>.<test>-sync`、`-async`），每個 suite、iteration 等各 1 個；三個參數都是字串，回傳值沒用到 | 同上 | 跑得完 |
| `performance.now()` | 量 sync 和 async 的時間（kun 已經有，glue 做的） | — | 已有 |
| `performance.timeOrigin`、`getEntries*`、`PerformanceObserver` | runner 沒用到 | — | 不需要 |
| `document.title = ...` | `main.mjs` 的 `_updateDocumentTitle`，由 `_showSection` 呼叫，頁面載入時（建構子）就跑；值是 `Speedometer 3.1` 加上那一節的 `data-title`（`getAttribute` 讀的） | 不丟例外：在 document 的 wrapper 上多一個普通的屬性；分頁的標題不變 | 行為對 |
| `new Blob([json], { type })`、`URL.createObjectURL(...)` | `main.mjs` 的 `_populateDetailedResults`，由 `didFinishLastIteration` 呼叫：**`startAutomatically` 跑完時自動執行**，做結果頁的下載連結（`jsonLink.href = URL.createObjectURL(new Blob(...))`，三個） | 沒有 `URL` 時 `ReferenceError`（`URL.createObjectURL` 在 `new Blob` 之前求值）；有 `URL` 沒有 `Blob` 時也是 `ReferenceError` | 跑得完（最後一步） |
| `a.href = ...` | 同上，設定下載連結 | 不丟例外：普通的屬性，連結沒有 `href` | 行為對（H2） |

`Blob` 和 `URL.createObjectURL` 只在同一個運算式裡一起用；runner 不讀 `Blob` 的內容，也不載入 `blob:` 網址（連結只有使用者點了才下載）。

### 三個 suite

| suite | H1 的成員 | 細節 |
| --- | --- | --- |
| TodoMVC-JavaScript-ES5 | `dataset` | `view.js` 的 `_itemId` 讀 `li.dataset.id`（`parseInt`），切換和刪除每一個 todo 時都會呼叫（`CompletingAllItems`、`DeletingAllItems` 兩步）；沒有 `dataset` 時 `TypeError`（讀 `undefined` 的 `id`）。編輯（`dblclick`、`blur`）時還會寫 `this.dataset.iscanceled = true`、讀它，Speedometer 的步驟不編輯。模板用 `data-id="{{id}}"` 寫進 HTML 字串，`dataset` 只用來讀。 |
| TodoMVC-Preact-Complex-DOM | 無 | 不讀 `navigator`、`performance`、`document.title`、`dataset`、`Blob`。Preact 設定屬性時先看 `name in dom`（有這個成員就設 property，沒有才 `setAttribute`），所以加了成員會改變它走的路，但 H1 的成員它都不設。 |
| TodoMVC-Svelte-Complex-DOM | 無 | 盤點的「HTML 元素的成員」是 `input.autofocus = true`（property 賦值，沒有時不丟例外），屬於路線圖的 F（focus）。屬性經過 `setAttribute` 的 helper。 |

三個 suite 都不讀 `navigator.userAgent`、`platform`：盤點裡用到它們的 8 個 suite 是 React-Redux、Angular-Complex-DOM、Vue、NewsSite 的兩個、Editor 的兩個和 Perf-Dashboard（M2 以後）。

### 和原本的 H 最小部分的差別

原本的 H 最小部分（`rfcs/0012-speedometer/roadmap.md` 的「每條線的順序」，DOM 線的第 1 項）：`navigator`、`dataset`、`document.title`、`performance.mark` / `measure`、`Blob`。核對之後：

| 項目 | M1 真的要的 | 2026-10-07 的看法 |
| --- | --- | --- |
| `navigator` | 物件存在、沒有 `wakeLock` | 加上 `NavigatorID` 等幾個 mixin 的值（`userAgent`、`platform`、`vendor`、`language`、`hardwareConcurrency`、`onLine`、`cookieEnabled`）：都是固定或從 browser 拿的字串和數字，很小；M2 的 suite 就要讀 `userAgent`、`platform`。 |
| `performance` | `mark(name)`、`measure(name, start, end)` 的字串形式 | 加上 `getEntries*`、`clearMarks`、`clearMeasures`、`PerformanceMark` 的建構子、`timeOrigin`：mark 本來就要保存（`measure` 照名稱找），查詢只是讀出來；WPT 的 `user-timing/` 大多要它們。`PerformanceObserver`、`toJSON()`、Navigation Timing 不做（H2 或之後）。 |
| `dataset` | getter（ES5 的 `dataset.id`） | 整個 `DOMStringMap`（setter、deleter、列舉）：getter 本身就要新的宣告表形式，setter 和 deleter 是同一套。 |
| `document.title` | 不擋跑得完 | 照做，順便讓分頁的標題跟著 script 改（見「document.title」）。 |
| `Blob` | 只跟 `URL.createObjectURL` 一起用 | **2026-10-07 整個移到 H2**（`README.md` 的「已決定的事項」第 1 條）：單獨在 M1 沒有用處，`URL.createObjectURL` 要 `URL` 這個 interface（路線圖的 B）；讀內容要兩種新的宣告表形式。H1 做完之後 runner 的最後一步仍然丟例外。調查見「給 H2 的筆記：`Blob`」。 |
| （新增）`[Replaceable]` | — | `performance` 變成 `Window` 的 attribute 之後，要照 WebIDL 可以被蓋掉（現在 glue 做的物件可以，`dom_js` 有測試）；宣告表加這個形式很小，`self`、`frames`、`parent`、`length` 的限制也一起解掉。 |

## navigator

### Chromium

- `Navigator`（`core/frame/navigator.idl`、`navigator.h`）是 `ScriptWrappable`，`LocalDOMWindow::navigator()` 第一次讀時建、存在 window 上（`[SameObject]`）。它繼承 `NavigatorBase`（`core/execution_context/navigator_base.h`），`NavigatorID`、`NavigatorLanguage`、`NavigatorOnLine`、`NavigatorConcurrentHardware` 這些 mixin 在 `NavigatorBase` 實作，worker 的 `WorkerNavigator` 也用它。
- `userAgent`：`NavigatorBase::userAgent()` → `ExecutionContext::UserAgent()` → `LocalFrame::Loader().UserAgent()` → `LocalFrameClient::UserAgent()`；字串由 browser 算（`content::GetUserAgent`、`ContentBrowserClient::GetUserAgent`），啟動 renderer 時交給它（`blink::UserAgentMetadata`、`RendererPreferences`），和 HTTP 的 `User-Agent` 標頭同一個來源。
- `platform`：`NavigatorID::platform()`，macOS 固定是 `MacIntel`（Apple Silicon 也是），Windows 是 `Win32`，Linux 是 `Linux x86_64` 等；`appVersion` 是 UA 去掉開頭的 `Mozilla/`；`vendor` 是 `Google Inc.`；`productSub` 是 `20030107`。
- 其他 spec 的成員（`wakeLock`、`clipboard`、`geolocation`、`connection`、`scheduling`……）是各自 module 的 `partial interface Navigator`，Blink 用 supplement 掛上去。

### 成員和值

| 成員 | 值（2026-10-07 的看法） | 來源 |
| --- | --- | --- |
| `userAgent` | 見下面的「UA 的選項」 | browser，經過啟動 renderer 的參數 |
| `appCodeName`、`appName`、`product` | `Mozilla`、`Netscape`、`Gecko`（規範的常數） | 常數 |
| `appVersion` | `userAgent` 去掉 `Mozilla/` | 從 UA 算 |
| `platform` | 照編譯的平台：`MacIntel`、`Win32`、`Linux x86_64` / `Linux aarch64`（和 Chrome 一樣） | 常數 |
| `productSub`、`vendor`、`vendorSub` | `20030107`、`Google Inc.`、空字串（WPT 的 `navigator.any.js` 看 UA 有沒有 `Chrome` 決定預期值，UA 像 Chrome 就要這組） | 常數 |
| `language`、`languages` | `en-US`、`["en-US", "en"]`：kun 沒有語言的設定；`languages` 是 `FrozenArray`，要回傳同一個陣列（JS 由前端快取，WIT 是 `list<string>`） | 常數，之後有設定再從 browser 拿 |
| `onLine` | `true` | 常數（沒有網路狀態的通知） |
| `cookieEnabled` | `false`：kun 沒有 cookie（`docs/limitations.md` 的「網路」） | 常數 |
| `hardwareConcurrency` | `std::thread::available_parallelism`（Chrome 也是實際的核心數） | renderer 自己讀 |
| `javaEnabled()`、`pdfViewerEnabled`、`plugins`、`mimeTypes` | 不做（H2）：`plugins`、`mimeTypes` 要兩個新的集合型別 | — |
| `wakeLock`、`clipboard`、`geolocation`、`connection`、`scheduling`、`userAgentData` | **不做，也不能有**：框架用 `in` 或真假值偵測，有了會走 Chrome 的路但做不到 | — |

### UA 的選項

kun 現在只有 HTTP 的標頭（`crates/net/src/http.rs` 的 `USER_AGENT`）：`Mozilla/5.0 (compatible) kun/<版本>`。`navigator.userAgent` 要和它一樣（WPT 的 `navigator.any.js` 用 `XMLHttpRequest` 比對，kun 還沒有 XHR，但規範是同一個值）。

| 做法 | 例子（macOS） | 優點 | 缺點 |
| --- | --- | --- | --- |
| U1. 維持現在的 | `Mozilla/5.0 (compatible) kun/<版本>` | 誠實、不冒充 | 用 UA 判斷瀏覽器的程式（CodeMirror、ProseMirror 判斷 `chrome`、`webkit`、`mac` 決定輸入的處理；Vue 判斷 `isIE`、`isEdge`、`isIOS`）走「不認識的瀏覽器」的路，和 Chrome 上量到的不是同一段程式，比較不公平；WPT 的 `navigator.any.js` 要 Gecko 的那組值 |
| U2. 和 Chrome 一樣 | `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36` | 和 Chrome 走同一條路；Chrome 的 reduced UA 本來就是固定的格式，版本號照連結的 V8 的 milestone（`.cargo/config.toml` 的 V8 15.2 對應 152） | 認不出是 kun；網站以為是 Chrome，用 kun 沒有的功能時壞得更徹底 |
| **U3. Chrome 的加上 `kun/<版本>`**（2026-10-07 的看法） | U2 的字串後面加 ` kun/<版本>` | 和 U2 走同一條路（`/Chrome\/(\d+)/` 照樣對得到，Edge 的 `Edg/` 就是這樣加的）；伺服器和 log 看得出是 kun | 字串長一點；一樣有 U2 的「以為是 Chrome」的問題 |

不管選哪一個，UA 只有一個來源：Browser 這一側算出來，HTTP 的標頭用它，啟動 renderer 的 `RendererFlags` 帶著它（能序列化的字串，照 `docs/architecture.md` 的「Process 的邊界」第 5 條），renderer 經過 `FrameClient` 交給 `dom_api`（Blink 的 `LocalFrameClient::UserAgent()`）。`platform` 照 renderer 自己編譯的平台（Chrome 也是 renderer 自己決定的），不用傳。

Chromium 的 UA 版本號：reduced UA（Chrome 101 起）只留 major，minor 寫 `0.0.0`；版本號跟著 Chrome 的 milestone。kun 換 V8 的時候（`docs/README.md` 的「其他會牽動文件的變更」）跟著改。

### GC

| 做法 | 說明 | 2026-10-07 的看法 |
| --- | --- | --- |
| N1. 新的 GC 型別 `Navigator` | 只有 realm 的 window 的 edge（建 wrapper 時要知道 realm）、wrapper、canary；window 多一條 edge，第一次讀時建（Blink 的 `LocalDOMWindow::navigator_`） | 建議 |
| N2. 一個「window 擁有、自己沒有狀態的物件」的通用 GC 型別，照種類分 interface | 像 `Node` 一個型別多種 interface；之後的 `Crypto`（H2）、`Screen`、`Location`、`History`（B）可以共用，少幾個 GC 型別 | `Location`、`History` 其實有狀態（`History.state` 是 `any`），共用的好處不確定。2026-10-08：RFC 0019 把 `Location`、`History` 各做成自己的 GC 型別，不採用 |
| N3. 降階：成員放在 `Window` 上，glue 組出 `navigator` | 現在 `performance` 的做法；不用 GC 型別 | 不建議：`navigator instanceof Navigator` 不成立，WIT 也是降階的，之後還要再換一次（`performance` 正要從這個做法換掉） |

## performance 和 User Timing

### Chromium

- `Performance`（`core/timing/performance.idl`、`performance.h`）繼承 `EventTarget`；window 用的是子類別 `WindowPerformance`（`window_performance.h`），由 `DOMWindowPerformance`（supplement）存在 `LocalDOMWindow` 上，第一次讀時建。
- 時間：`Performance::now()` 是 `MonotonicTimeToDOMHighResTimeStamp(time_origin_, CurrentTime(), ...)`，先減去 time origin 再由 `TimeClamper` 降精度（沒有 cross-origin isolation 時 100 µs，加上 jitter）。`WindowPerformance` 的 `time_origin_` 是那份 document 的 `DocumentLoadTiming::ReferenceMonotonicTime()`，也就是導覽開始的時間（browser 在導覽開始時記下，`CommonNavigationParams::navigation_start` 傳給 renderer）；iframe 的 document 有自己的。`timeOrigin` 是 time origin 換成 Unix epoch 的毫秒、降精度。
- `PerformanceEntry`（`performance_entry.h`）是基底類別，`PerformanceMark`、`PerformanceMeasure` 是子類別，都是 `ScriptWrappable`、GC 物件；`detail` 存成 `SerializedScriptValue`（structured clone 之後），讀的時候在讀的 realm 裡 deserialize、快取。
- `UserTiming`（`user_timing.cc`）放 mark 和 measure 的表（名稱 → entry 的串列），`Performance` 另有整體的 buffer（`performance_entry_buffer`，`getEntries` 用）；`measure` 的開始和結束是 mark 的名稱時，取那個名稱最新的一個；名稱是 `PerformanceTiming` 的 attribute（`navigationStart` 等）時，用 Navigation Timing 的值。
- `Event.timeStamp`、`requestAnimationFrame` 的時間：Blink 存 `base::TimeTicks`，讀的時候用那個 realm 的 `Performance` 換算（`Event::timeStamp(ScriptState*)`）。

### 時間原點

kun 現在：renderer 的 thread 啟動時記一個 `Instant`（`crates/renderer/src/render_thread.rs` 的 `start`），每份 document 的 `FrameClient::now()`（`crates/renderer/src/frame_view.rs`）都從它算，子 frame 也是（`docs/limitations.md` 的「iframe」）；`performance.now()`、`Event.timeStamp`、rAF 的時間、WASI 的 monotonic clock 都經過 `dom_api::window::now` 降到 100 µs（`coarsen`，沒有 jitter）。

| 做法 | 說明 | 優點 | 缺點 |
| --- | --- | --- | --- |
| O1. 維持 renderer 的時鐘 | `timeOrigin` 是 renderer 啟動時的 Unix 時間；所有 window 共用 | 什麼都不用換算，H1 最小 | 規範要每個 global 自己的 time origin；同一個分頁換頁之後 `now()` 一直變大；`hr-time/test_cross_frame_start.html` 這類測試不會過；之後改成 O2 時，`timeOrigin` 和 `now()` 的值 script 看得出差別（沒有相容性的問題） |
| **O2. 每個 window 一個 time origin**（2026-10-07 的看法） | window（`dom` 的 `Window`）記下自己的 time origin（renderer 時鐘上的毫秒）；`performance.now()` 是「現在減去 time origin」再降精度；`timeOrigin` 是 time origin 的 Unix 時間（renderer 啟動時一起記下 `SystemTime`，之後只加 `Instant` 的差，不受系統時鐘調整影響），降精度；rAF 的時間、`Event.timeStamp` 讀的時候照它的 realm 的 window 換算（照 Blink：存 renderer 時鐘上的值，讀時換算）；DOM 內部的 timer 清單照舊用 renderer 的時鐘；WASI 的 monotonic clock 的原點本來就不規定，不改 | 照規範和 Chromium；iframe 有自己的；換算只有三處 | 動到事件和 rAF（`dom_api` 的 `event.rs`、`timers.rs`），測試要跟著改（`dom_api` 的 `events` 測試、`dom_wasm` 的 guest 讀到的 `performance.now()`） |

O2 的 time origin 取在哪一刻：

| 時間點 | 說明 |
| --- | --- |
| **renderer 開始 commit 那份 document 時**（2026-10-07 的看法） | 建 window 和 realm 的時候（`FrameRequests::new` 那一刻）；不用改 `protocol`。和 Chrome 的差別：Chrome 從導覽開始算，包括網路的時間，kun 的 `now()` 在 script 開始時比較小。記成限制 |
| 導覽開始時（Chromium） | 主 frame 的導覽由 Browser 的 `net` 載入，renderer 不知道什麼時候開始；`Instant` 不能序列化，要由 browser 送「導覽開始到現在過了多久」（`Duration`），renderer 往回推。要改 `protocol` 的導覽訊息，和 B（導覽）同一塊，建議等 B 一起做；子 frame 由 renderer 自己載入，可以照實記 |

Speedometer 只用 `now()` 的差，兩種原點都不影響 M1。

精度維持 100 µs、沒有 jitter（`dom_api` 的 `coarsen`，`performance.now()`、`Event.timeStamp`、rAF 的時間、WASI 的 monotonic clock 共用）；Chrome 加 jitter 是為了防 timing attack 對齊邊界，kun 之後要做 site isolation（RFC 0004）時再一起看。

### entry 的保存和查詢

- **performance entry buffer**：mark 和 measure 都放進 window 的 `Performance` 的串列（規範的 "performance entry buffer"；User Timing 沒有上限，Chrome 也沒有）。`getEntries()` 照 `startTime` 排序（同一個時間照加入的順序），`getEntriesByType(type)`、`getEntriesByName(name, type)` 是篩選；不認識的 type 回傳空的。`clearMarks(name)`、`clearMeasures(name)` 移除全部或那個名稱的。
- **同一個物件**：`performance.mark("a") === performance.getEntriesByName("a")[0]`（WPT 的 `mark-measure-return-objects`，Chrome 也是），所以 entry 從建立起就是 GC 物件，buffer 強指著它們。
- **記憶體**：Speedometer 每個 test 加 6 個 entry，每個 suite、iteration 再多幾個；跑完 20 個 suite、10 個 iteration 是幾千個小物件，Chrome 一樣留著。
- **`PerformanceObserver`**、`supportedEntryTypes`：不做（H2）；runner 不用，Angular-Complex-DOM 和 NewsSite-Next 用（M2、M5）。它要 callback、自己的佇列和 buffered 的旗標，是另一個機制。
- **其他 entry 的種類**（`navigation`、`resource`、`paint`、`longtask`……）：不做，`getEntriesByType` 回傳空的串列；Speedometer 的盤點用 `getEntriesByType("resource")` 只是盤點工具自己的。

### `mark` 和 `measure` 的步驟

照 User Timing Level 3：
- `mark(name, { startTime, detail })`：window 上，名稱是 `PerformanceTiming` 的 attribute 名稱時丟 `SyntaxError`；`startTime` 是負的時 `TypeError`；沒給時是 `now()`。`new PerformanceMark(name, options)` 一樣，但不放進 buffer。
- `measure(name, startOrOptions, endMark)`：第二個參數是字串（mark 的名稱）或 `PerformanceMeasureOptions`（`start`、`end` 是 mark 的名稱或時間，`duration`，`detail`）；選項和 `endMark` 一起給、選項裡三個都給、選項是空的卻有 `endMark` 以外的組合，照規範丟 `TypeError`。mark 的名稱找不到時 `SyntaxError`；名稱是 `PerformanceTiming` 的 attribute 時規範用 Navigation Timing 的值，kun 沒有 Navigation Timing，2026-10-07 的看法是當成找不到的 mark（`SyntaxError`），記成限制（WPT 的 `measure_navigation_timing` 等會失敗）。
- 宣告表要多的：`(DOMString or PerformanceMeasureOptions)`（union 的成員是字典）、`(DOMString or DOMHighResTimeStamp)`（union 的成員是 `double`），見「宣告表的新形式」。

### `detail`

`detail` 是 `any`，照 RFC 0008 的「已決定的事項」第 8 條只給 JS：`PerformanceMarkOptions` 的 `detail`、`PerformanceMark.detail` 不在 `kun:dom` 裡，其他成員照樣在。規範要 structured clone（存的是 `StructuredSerialize(detail)`，每次讀 deserialize 出新的物件，不能 clone 的丟 `DataCloneError`）：

| 做法 | 說明 | 2026-10-07 的看法 |
| --- | --- | --- |
| D1. 存原本的值，不 clone | 像 `CustomEvent.detail`（`dom` 的 slot 裡的 script 值）；mark 之後改了原本的物件，`detail` 也跟著變 | 建議：runner 和 M1 的 suite 都不用 `detail`；WPT 的 `structured-serialize-detail` 和 `mark-l3` 的幾個 subtest 會失敗，記成限制 |
| D2. 用 V8 的 `ValueSerializer` / `ValueDeserializer` clone | `dom_js` 裡 clone 一次（不含 host object），存 clone 的結果 | J（`MessageChannel`、`postMessage`）一定要 structured clone，那時做成共用的機制（含 host object、跨 realm），`detail` 改用它；H1 先做會做兩次 |

## dataset

### Chromium

- `DOMStringMap`（`core/dom/dom_string_map.idl`）：`[LegacyOverrideBuiltIns]`，匿名的 named getter、setter、deleter；`DatasetDOMStringMap`（`core/dom/dataset_dom_string_map.cc`）是它的子類別，拿著元素，每次從元素的 attribute 算（不快取），存在元素的 `ElementRareData`（`[SameObject]`）。
- 名稱的轉換（HTML 的 "name of a dataset property"）：
  - 讀和列舉：attribute 名稱是 `data-` 開頭、後面沒有 ASCII 大寫字母，`-` 加小寫字母換成大寫字母（`data-foo-bar` → `fooBar`）；只看 HTML namespace 的 null namespace 的 attribute，列舉照 attribute 的順序。
  - 寫：名稱裡有 `-` 後面接小寫字母時丟 `SyntaxError`；大寫字母換成 `-` 加小寫，前面加 `data-`；結果不是合法的 attribute 名稱時丟 `InvalidCharacterError`；然後 `setAttribute`。
  - 刪：同樣轉換之後 `removeAttribute`（沒有就什麼都不做）。
- binding 由 IDL 產生：V8 的 named property interceptor（getter、setter、deleter、query、enumerator、definer），`[LegacyOverrideBuiltIns]` 是 masking 的（先問 interceptor，再看 prototype）。

### JS

`dom_js` 現在的 named interceptor（`crates/dom_js/src/properties.rs`）只有 getter 和 query，flag 是 `NON_MASKING`（prototype 上有的名稱優先，給 `HTMLCollection` 的 `[LegacyUnenumerableNamedProperties]`），名稱不列在 own keys 裡。`DOMStringMap` 要：
- **setter、deleter**：WebIDL 的 [[Set]]：有 named setter 的物件，字串的 key 一律經過 setter（`dataset.toString = "x"` 也是設 `data-to-string`）；[[Delete]] 經過 deleter；[[DefineOwnProperty]] 也是（V8 的 definer）。
- **supported property names**：[[OwnPropertyKeys]] 列出它們（enumerator），可列舉（沒有 `[LegacyUnenumerableNamedProperties]`）：`for...in`、`Object.keys(dataset)`。
- **`[LegacyOverrideBuiltIns]`**：masking 的 interceptor，`data-to-string` 存在時 `dataset.toString` 是那個字串。

這三樣做成宣告表的通用形式（見「宣告表的新形式」），不寫只給 `DOMStringMap` 的特例；集合的 named property 不能設定、不列在 own keys 的限制（`docs/limitations.md` 的「Bindings 和 DOM API」）有了這些形式之後也比較好解，但不在這份的範圍。

### wasm

匿名的特殊操作在 WIT 裡要有名字。2026-10-07 的看法：照 Blink 給 C++ 的名字的意思，宣告表寫 `[NamedGetter] anonymous method get(name)`、`[NamedSetter] anonymous method set(name, value)`、`[NamedDeleter] anonymous method delete(name)`、`[SupportedNames] anonymous method names() -> list`；`anonymous` 表示 JS 不裝成 prototype 上的方法（規範沒有這些名字），WIT 照名字是一般的函式（`dom-string-map.get` 等）。`spec-differences.txt` 寫明。

### GC

| 做法 | 說明 | 2026-10-07 的看法 |
| --- | --- | --- |
| G1. 新的 GC 型別 `DatasetDomStringMap` | 元素的 edge、wrapper、canary；元素的 `NodeRareData` 撐住它（像 `DomTokenList`） | 建議：照 Blink 的類別，`Object` 和 `ObjectKind` 各一項 |
| G2. 和 `DomTokenList` 合成「元素的 live view」一個 GC 型別，照種類分 interface | 少一個 GC 型別 | 兩者的語意沒有共通的部分（token 的解析 vs 名稱的轉換），合起來只省 struct 和清單的一行；RFC 0010 第 8 條之後新增一個型別的成本主要在 `Object` 的分支和測試，合併也省不掉 |

## document.title

### 規範和 Chromium

- getter（HTML 的 "document.title"）：document element 是 SVG 的 `svg` 時，用它第一個 SVG `title` 子元素；否則是 document 裡（樹的順序）第一個 HTML `title` 元素。取它的 child text content，去掉頭尾、合併 ASCII 空白。
- setter：document element 是 SVG 的 `svg` 時，沒有 `title` 子元素就建一個插在最前面，再 "string replace all"；document element 是 HTML namespace 時，沒有 `title` 元素、也沒有 `<head>` 就什麼都不做，沒有 `title` 就建一個 append 進 `<head>`，再 "string replace all"；其他（沒有 document element、別的 namespace）什麼都不做。
- Blink：`Document::title()` 回傳快取的 `title_`；`Document::setTitle(const String&)` 照上面的步驟改 DOM；`HTMLTitleElement::ChildrenChanged` 和 `InsertedInto` / `RemovedFrom`（經過 `Document::SetTitleElement`、`RemoveTitle`）在 title 元素或它的文字變的時候呼叫 `Document::UpdateTitle`，它更新 `title_` 並呼叫 `LocalFrameClient::DispatchDidReceiveTitle`；`RenderFrameImpl` 把它送給 browser（`LocalFrameHost::UpdateTitle`），`RenderFrameHostImpl::UpdateTitle` 只採用主 frame 的，`WebContentsImpl::UpdateTitleForEntry` 更新 navigation entry 和分頁的標題。

### kun 現在

- renderer 的 `crates/renderer/src/document.rs` 有 `title()`（第一個 HTML `title` 的文字子節點，合併空白），只在主 frame parse 完時算一次，送 `RendererHostMsg::UpdateTitle`；`browser` 的 `WebContents` 收到後更新 navigation entry 和分頁列（RFC 0011）。
- `dom_api` 有 `HTMLTitleElement.text`，沒有 `Document.title`；script 改了標題，分頁不變。

### 分頁的標題要不要跟著 script 改

| 做法 | 說明 | 優點 | 缺點 |
| --- | --- | --- | --- |
| T1. 只做 getter 和 setter，分頁照舊只在 parse 完時更新 | — | 最小，不碰 renderer | 分頁和 `document.title` 不一致；runner 一載入就改標題，分頁上看不到 |
| T2. renderer 每次 rendering update 重新算主 frame 的標題，變了才送 | 不碰 `dom`；只在 DOM 的版本（`dom_tree_version`）變了的 frame 才算 | 改動小 | 沒有 `<title>` 的 document 每次改 DOM 都要走過整棵樹找它（第一個 `title` 可能在任何地方），Speedometer 的大 DOM 在主 frame 上會多一段 main thread 的時間；script 改了沒有畫面的 document 不會有 rendering update |
| **T3. 照 Blink：`dom` 的原語記下「這份 document 的標題可能變了」，renderer 在 task 之後送**（2026-10-07 的看法） | 插入和移除時，子樹裡有 HTML `title`（或 SVG 根的 `title`）就標記那份 document（和 iframe 的 insertion steps 走同一次子樹，`dom` 的 `record_frame_owners`）；`title` 元素的子節點、文字改變也標記；renderer 在每個 task 和 parser 的每一段之後，看主 frame 的 document 有沒有標記，有就用 `dom_api` 的 getter 算、和上次送的不同才送 `UpdateTitle` | 和 Chromium 一樣只在 title 可能變時算；沒有 `<title>` 的大 DOM 不用走 | `dom` 的原語多一個判斷（和 mutation observer、iframe 的一樣在已有的分支裡）；要測試各種改法（`textContent`、`innerHTML` 換掉 `<head>`、移走 `title`） |

process 的邊界：沿用已有的 `RendererHostMsg::UpdateTitle`，不加新的訊息；只有主 frame 的標題送出去（子 frame 的忽略，和 Chromium 一樣在 renderer 這邊就不送）。Chromium 對標題的長度有上限（`content::kMaxTitleChars`，4096 個字元），kun 在 browser 那邊照它截斷。

`renderer` 的 `document::title` 搬到 `dom_api`（`Document.title` 的 getter），renderer 改呼叫它，只留一份實作。

## 給 H2 的筆記：`Blob`

2026-10-07 從 H1 移出（`README.md` 的「已決定的事項」第 1 條）。寫草稿時調查到的，留給 H2（和 `URL.createObjectURL` 所在的 RFC）當起點，都還沒確認。

- **runner 怎麼用**：只有「M1 的核對」的 runner 那張表的一列：`didFinishLastIteration` 裡的 `URL.createObjectURL(new Blob([json], { type }))`，跑完時自動執行；不讀內容、不載入 `blob:` 網址。所以 runner 跑完要 `Blob` 的建構子（字串的部分和 `type`）加上 `URL.createObjectURL`。
- **Chromium**：`Blob`（`core/fileapi/blob.idl`）拿著 `BlobDataHandle`（`platform/blob/blob_data.h`），資料在 browser process 的 `BlobStorageContext`（`storage/browser/blob/`），renderer 經過 mojo 登記和讀（`BlobRegistry`），所以讀的方法一定回傳 Promise；`slice` 指向同一份資料的一段。`URL.createObjectURL` 是 File API 的 `partial interface URL`（`core/fileapi/url_file_api.idl`、`PublicURLManager`），在 browser 的 `BlobURLStore` 登記 `blob:<origin>/<uuid>`；`URL` 本身在 `core/url/dom_url.idl`。
- **資料放在哪**：當時的看法是先放在 renderer 的記憶體（不可變的 bytes，`Arc<[u8]>` 加上範圍、`NoTrace`，`slice` 共用），等 `blob:` 網址的載入、下載、`fetch` 的 body（L）要跨 process 讀時再照 Chromium 搬到 Browser（新的 `protocol` 訊息和 Browser 的表），介面不變。
- **成員和要的新東西**：建構子（`sequence<(BufferSource or Blob or USVString)>`，`BlobPropertyBag` 的 `type`、`endings`）要 `BufferSource` 參數（bytes 複製一份，detached 的是空的；WIT 是 `list<u8>`）；`size` 要 `unsigned long long` 的回傳（WIT 是 `u64`）；`slice` 的 `long long` 參數；`text()`、`arrayBuffer()`、`bytes()` 要回傳 `Promise<T>` 的形式（JS 回傳 promise，WIT 是 `async func -> T`）和 `ArrayBuffer`、`Uint8Array` 的回傳。Promise 的形式 L（`Response.text()`）和 H2 的 `HTMLImageElement.decode()` 也要，可能由先需要它的 RFC 設計。`stream()` 要 Streams，`File`、`FileReader` 盤點裡只有 Angular 的 zone.js 為了 patch 讀過。
- **GC**：新的 GC 型別 `Blob`（realm 的 window、wrapper），bytes 是新的 `NoTrace` 型別（`soundness.md` 要寫 `SAFETY`）。
- **`URL.createObjectURL` 放在哪**：當時的看法是 B（和 `URL`、`URLSearchParams` 一起），不選「先建一個只有 static 方法的 `URL`」：`typeof URL` 成立、`new URL()` 卻不行，偵測 `URL` 的程式會走錯路。歸 H2 還是 B 由它們決定。
- **WPT**：`FileAPI/blob/` 有 16 個測試檔，幾乎都經過 `text()` 或 `arrayBuffer()` 驗證內容（`FileAPI/support/Blob.js` 的 `test_blob`），也大量用 typed array；不靠它們就能跑的只有 `Blob-constructor-endings`、`Blob-slice-overflow`。只做建構子的話，WPT 幾乎驗證不了。規範的 WebIDL 比對要加讀 `FileAPI.idl`。

## 宣告表的新形式

每一種都要在宣告表、JS、WIT 各決定樣子（RFC 0008 的第 4 節的做法）。`Blob` 要的形式（`BufferSource`、Promise、`ArrayBuffer`、`unsigned long long` 的回傳）跟著它移到 H2（「給 H2 的筆記：`Blob`」）。

| 形式 | 誰用 | JS（`dom_js`） | WIT | 2026-10-07 的看法 |
| --- | --- | --- | --- | --- |
| `[Replaceable]` attribute | `performance`（之後 `self`、`frames`、`parent`、`length`） | 有 setter：在物件上定義同名的普通 data property | 沒有變化（guest 不會蓋掉 window 的屬性） | 第 2 階段 |
| union 的成員是字典 | `measure` 的 `(DOMString or PerformanceMeasureOptions)` | 照 WebIDL 的 union 轉換：`undefined`、`null`、不是 callable 的物件是字典，其他是字串 | `variant` 的一個 case 是 record | 第 2 階段；2026-10-08 確認宣告表的 union 還不能有字典的成員。另一個做法是 RFC 0018 的 `scrollIntoView` 的先例：宣告表收字典，JS 的 glue 照 union 轉換、把字串放進字典，WIT 只有字典的形式；2b 選了讓宣告表的 union 有字典成員（第 13 條）。字典的欄位也可以是 union（`start`、`end` 的 `(DOMString or DOMHighResTimeStamp)`），WIT 跟著定義那些 `variant` 和 `record` |
| union 的成員是 `double` | `(DOMString or DOMHighResTimeStamp)` | 數字是 `double`，其他是字串 | case 是 `f64` | 同上 |
| 匿名的 named getter、setter、deleter 和 supported names | `DOMStringMap` | interceptor 的 getter、setter、deleter、definer、query、enumerator；`[LegacyOverrideBuiltIns]` 是 masking | 一般的函式（`get`、`set`、`delete`、`names`） | 第 3 階段；2026-10-08：getter、setter、supported names 已經由 RFC 0016 的 1c 做好（`CSSStyleDeclaration`），只差 deleter |
| `[LegacyOverrideBuiltIns]` | `DOMStringMap` | 不加 `NON_MASKING` | 沒有變化 | 第 3 階段 |
| `FrozenArray<DOMString>` 的回傳 | `navigator.languages` | 回傳同一個凍結的陣列（值不變時） | `list<string>` | 第 1 階段；或先不做 `languages` |

## 新的 GC 型別

| `dom` 的型別 | interface | edge | 第幾階段 |
| --- | --- | --- | --- |
| `Navigator`（新的） | `Navigator` | realm 的 window；window 多一條到它的 edge | 1 |
| `EventTargetImpl` 的新種類 | `Performance` | entry 的串列（`EdgeChunks`）、realm 的 window；window 多一條到它的 edge | 2 |
| `PerformanceEntry`（新的），種類 mark、measure | `PerformanceMark`、`PerformanceMeasure` | realm 的 window、`detail` 的 script 值（只有 JS 有） | 2 |
| `DatasetDomStringMap`（新的） | `DOMStringMap` | 元素；元素的 `NodeRareData` 多一條到它的 edge | 3 |

- **`Performance` 是 `EventTargetImpl` 的一種**：規範的 `Performance : EventTarget`（`resourcetimingbufferfull` 等事件），照 RFC 0010 的「已決定的事項」第 16 條（`AbortSignal`）的做法：分派、listener、wrapper 沿用。另一個做法是自己的 GC 型別、不是 event target：`performance instanceof EventTarget` 不成立，WPT 的 idlharness 也看得出來；之後要事件時再改。
- **`PerformanceEntry` 一個型別、照種類分 interface**：像 `Event` 一個型別有 `MouseEvent`、`CustomEvent`。之後的 entry 種類（Navigation Timing、Paint Timing）也加在這裡。
- **每個新型別的成本**（`crates/dom/README.md` 的「怎麼修改」）：struct 和 `script_objects!` 一行、`Object` 和 `ObjectKind` 各一項和 `match` 的分支、泛型的 GC 測試各一行、測試的工作負載和 `gc_stress`、`soundness.md` 的規則、mutation test；RFC 完成時在 macOS 上用 debug 的 V8 跑 `test.py matrix` 和 `fuzz`。新的 edge（window 到 `Navigator` 和 `Performance`、`Performance` 到 entry、元素到 `DatasetDomStringMap`）每一條都要有測試守住（GC 之後還在、沒人拿著時被回收）。

## WIT 和 `kun:dom` 的版本

2026-10-07 `kun:dom` 是 0.7.0（`crates/dom_api/src/wit.rs` 的 `PACKAGE`），2026-10-08 還是。H1 的改動：

| 改動 | 相容嗎 |
| --- | --- |
| 新的 resource（`navigator`、`performance`、`performance-entry`、`performance-mark`、`performance-measure`、`dom-string-map`）和 `window.navigator`、`window.performance`、`html-element.dataset`、`document.title` 等新函式 | 只新增，相容 |
| 拿掉降階的 `window.performance-now`（`performance.now` 取代它） | **不相容** |

| 做法 | 說明 | 2026-10-07 的看法 |
| --- | --- | --- |
| W1. 拿掉 `performance-now`，升一次 minor | 照 RFC 0008 的「已決定的事項」第 4 條；guest（`crates/dom_wasm/tests/guests/dom.c`、`sleep.c`、`microtasks.c` 等）改用 `performance.now`，`ci.py guests` 重新編 | 建議：0.x 本來就允許不相容的改動；升版的時機照 RFC 0012 的「已決定的事項」第 10 條，不和另外兩條線協調 |
| W2. 留著 `Window.performanceNow`（JS 照舊藏起來），只新增 | 不升版 | 同一件事兩個入口，之後還是要拿掉一次 |

## WPT 的調查

2026-10-07，WPT 的 commit 照 `third_party.toml`，讀測試的原始碼（`grep`），是估計。「要別的」是 kun 沒有、H1 也不做的：

| 目錄 | 測試檔 | 不用別的就能跑的 | 要別的 |
| --- | --- | --- | --- |
| `user-timing/` | 36 | 約 20：`mark`、`measure`、`clear*`、`entry_type`、`mark-measure-return-objects`、`invoke_*` | `PerformanceObserver`（`buffered-flag`、`case-sensitivity`、`mark-l3`、`measure-with-dict`、`supported-usertiming-types`）、Navigation Timing（`measure.html`、`measure_navigation_timing` 等 5 個）、structured clone（`structured-serialize-detail`）、`toJSON`、worker、idlharness（要 `fetch`） |
| `hr-time/` | 14 | `basic.any.js`、`monotonic-clock.any.js`、`clamped-time-origin.html`、`timing-attack.html` | worker、cross-origin isolation（`.https` 和 `.headers`）、Navigation Timing、`toJSON`、`test_cross_frame_start`（O2 加上 Navigation Timing） |
| `html/dom/elements/global-attributes/` 的 `dataset*` | 7 | 全部（`dataset.html` 有一個 SVG 元素的 subtest） | — |
| `html/dom/documents/dom-tree-accessors/` 的 `document.title-*` | 10 | 7 個 `.html`（`-09` 用 SVG 的 document element，`-not-in-html-svg` 也是） | 2 個 `.xhtml`（要 XML parser） |
| `html/webappapis/system-state-and-capabilities/the-navigator-object/` | 26 | `navigator.any.js`（`userAgent value` 要 XHR）、`per-global.window.js`、`navigatorlanguage.html`、`clientinformation.window.js`、`navigator-indexed.html` 等 | `registerProtocolHandler`（12 個 `protocol-handler-*`）、`plugins`、manual 的測試 |

`clientinformation.window.js` 測 `window.clientInformation`（`navigator` 的舊別名，`[Replaceable]`）：小，看第 1 階段時要不要一起加。

怎麼加進清單：照 `tests/wpt/README.md` 的「testharness.js tests, reftests and crashtests」，每個階段把它解開的目錄或檔案加進 `4ff8d6e:tests/wpt/testharness-include.txt`（目錄大半能跑才整個加，否則逐檔），跑 `MACHOPO_UPDATE_EXPECTATIONS=1 python3 scripts/test.py wpt`，跑不起來的照原因放進 `testharness-skip.txt`。規範的 WebIDL 比對（`crates/dom_api/tests/spec.rs`）加讀 `user-timing.idl`、`performance-timeline.idl`（`html.idl`、`hr-time.idl` 已經在讀）。

## 參考資料

- HTML：`Navigator`（https://html.spec.whatwg.org/#the-navigator-object）、`dataset` 和 `DOMStringMap`（https://html.spec.whatwg.org/#dom-dataset）、`document.title`（https://html.spec.whatwg.org/#document.title）。
- High Resolution Time（https://w3c.github.io/hr-time/）、Performance Timeline（https://w3c.github.io/performance-timeline/）、User Timing（https://w3c.github.io/user-timing/）。
- File API（https://w3c.github.io/FileAPI/）：給 H2 的筆記。
- WebIDL 的 legacy platform object（[[Set]]、[[Delete]]、`[LegacyOverrideBuiltIns]`）：https://webidl.spec.whatwg.org/#es-legacy-platform-objects。
- Chromium（`third_party/blink/renderer/`）：`core/frame/navigator.cc`、`core/execution_context/navigator_base.cc`、`core/dom/dom_string_map.idl`、`core/dom/dataset_dom_string_map.cc`、`core/dom/document.cc`（`setTitle`、`UpdateTitle`）、`core/html/html_title_element.cc`、`core/timing/performance.cc`、`window_performance.cc`、`user_timing.cc`、`performance_mark.cc`、`performance_measure.cc`；給 H2 的筆記：`core/fileapi/blob.cc`、`platform/blob/blob_data.cc`、`core/fileapi/public_url_manager.cc`；`content/browser/renderer_host/render_frame_host_impl.cc`（`UpdateTitle`）。
- Speedometer：`https://github.com/WebKit/Speedometer` 的 `release/3.1`（`1386415`）。
