# RFC 0017：HTML 元素和 Window 的雜項（H1：M1 要的最小部分）

- 開始日期：2026-10-07
- 狀態：✅ 已完成：2026-10-08（macOS Apple Silicon；接受和排程：2026-10-08；五個階段，「已決定的事項」第 1 到 21 條；驗收見 `progress.md`）
- 細節：`details.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 H1，DOM 線的第一份。2026-10-07 原本的 H 拆成兩份（RFC 0012 的「已決定的事項」第 5 條）：H1 是 M1（runner 加上 TodoMVC-JavaScript-ES5、Preact-Complex-DOM、Svelte-Complex-DOM 跑得完，第 6 條）要的最小部分，照 RFC 0008、RFC 0010 的做法；H2 是其他，之後另開（「之後的可能」）。

## 摘要

補上 Speedometer 的 runner 和 M1 的 suite 要、kun 還沒有的四組 API，照 Chromium 的類別和 Blink 的目錄放進宣告表，每組同時給 JS（`dom_js`）和 wasm（`kun:dom`）：

1. **`navigator`**（`Navigator`、`NavigatorID` 等 mixin）：runner 一開始就讀 `navigator.wakeLock`，沒有 `navigator` 時整個 run 停下。
2. **`performance` 和 User Timing**（`Performance`、`PerformanceEntry`、`PerformanceMark`、`PerformanceMeasure`）：runner 每個 test 都呼叫 `performance.mark` 和 `measure`。原本的 `performance` 是 glue 組出來、只有 `now()` 的物件。
3. **`dataset`**（`DOMStringMap`）：TodoMVC-JavaScript-ES5 切換和刪除 todo 時讀 `li.dataset.id`。
4. **`document.title`**：runner 載入時就設定它；順便讓分頁的標題跟著 script 改。

原本也在 H1 的 `Blob` 2026-10-07 移到 H2（「動機」的「範圍」）。每一組各是一個階段，可以分開決定和驗收（「分階段」）。核對 Speedometer 原始碼之後，範圍和原本的 H 最小部分有幾處不同（「動機」的「範圍」，第 2 條）；每個階段的設計在開始前確認，寫在「已決定的事項」。

## 動機

**現況（2026-10-07）**：
- **runner 跑不完**：`navigator`、`performance.mark` / `measure` 沒有就丟例外；跑完時的 `URL.createObjectURL(new Blob(...))` 也丟例外（`rfcs/0012-speedometer/README.md` 的「盤點的結果」的 runner 那張表）。
- **TodoMVC-JavaScript-ES5 跑不完**：`dataset` 沒有，讀 `dataset.id` 丟 `TypeError`（`details.md` 的「三個 suite」）。
- **`performance` 是降階的**：宣告表的 `Window.performanceNow` 加上 glue 組的物件，沒有 `Performance` interface、`timeOrigin`、`mark` / `measure`；時鐘從 renderer 啟動算起，子 frame 也共用（`docs/limitations.md` 的「Bindings 和 DOM API」「iframe」）。
- **分頁的標題只在 parse 完時算一次**：renderer 的 `crates/renderer/src/document.rs` 找第一個 `<title>`，script 改了也不變；`dom_api` 沒有 `Document.title`。
- **宣告表缺幾種形式**：named property 只有 getter、不能設定和刪除、不列在 own keys（`docs/limitations.md` 的「Bindings 和 DOM API」）；沒有 `[Replaceable]`。

**2026-10-08 對照 `main` 的變化**（草稿寫好之後，另外兩條線做完的）：
- RFC 0016 的 1c 加了匿名的 named setter 和 supported names（`CSSStyleDeclaration` 的 camelCase 屬性），`dataset` 只差 deleter 和 `[LegacyOverrideBuiltIns]`。
- RFC 0019 做完了：`URL`、`URLSearchParams`、`Location`、`History`，後兩個各是自己的 GC 型別。runner 的最後一步現在只差 H2 的 `Blob` 和 `URL.createObjectURL`。
- RFC 0018 的 3a 留下兩個用得到的先例：`(boolean or ScrollIntoViewOptions)` 由 JS 的 glue 轉成字典（宣告表的 union 還不能有字典的成員）；字典的 WebIDL 預設值交給 WIT 和 guest 的 SDK（它的第 38 條）。
- `kun:dom` 還是 0.7.0；`performance` 還是 glue 組的、`[Replaceable]`、`FrozenArray` 還沒有、UA 還是 `net` 寫死的那一個、分頁的標題還是 parse 完時算一次。

**範圍**：以 `rfcs/0012-speedometer/details.md` 的「每項功能缺的成員」為起點，對 runner 和三個 suite 的原始碼核對「沒有它時會怎樣」（`details.md` 的「M1 的核對」）。和原本的 H 最小部分的差別（`details.md` 的「和原本的 H 最小部分的差別」）：
- **`navigator`**：M1 只要物件存在、**沒有** `wakeLock`；`userAgent`、`platform` 是 M2 的 suite 要的。`NavigatorID` 等幾個 mixin 的值很小，一起做（第 2 條）。
- **`performance`**：M1 只要 `mark(name)` 和 `measure(name, startMark, endMark)` 的字串形式。`getEntries*`、`clear*`、`PerformanceMark` 的建構子、`timeOrigin` 一起做（第 2 條）（mark 本來就要保存，WPT 的 `user-timing/` 也要它們）；`PerformanceObserver`、`toJSON()`、Navigation Timing 留給 H2。
- **`document.title`**：沒有它 runner 也跑得完（只是在 wrapper 上多一個屬性），是「行為對」的層級；照原本的範圍做。
- **`Blob` 2026-10-07 整個移到 H2**（使用者決定）：它單獨在 M1 沒有用處，runner 只把它交給 `URL.createObjectURL`，而 `URL` 這個 interface 在路線圖的 B；讀內容的 `text()`、`arrayBuffer()` 還要 Promise 和 `BufferSource` 兩種新的宣告表形式，比看起來大。所以 **H1 做完之後，runner 的最後一步（`didFinishLastIteration` 裡的 `URL.createObjectURL(new Blob(...))`）仍然會丟例外**，要等 H2 的 `Blob` 和 `URL.createObjectURL`（歸 H2 還是 B 由它們決定）。M1 不包括這一步（RFC 0012 的「已決定的事項」第 9 條）。調查到的細節留在 `details.md` 的「給 H2 的筆記：`Blob`」。
- **新增 `[Replaceable]`**：`performance` 改成真的 attribute 之後，要照 WebIDL 能被蓋掉（現在的 glue 物件可以）；宣告表加這個形式很小，`self` 等的同一條限制也一起解掉。

目標：
- runner 和 TodoMVC-JavaScript-ES5 不再因為這四組丟例外（runner 的最後一步還要 H2 的 `Blob`，見上面）；成員照規範運作，由 `dom_api` 的單元測試和 WPT 驗證。
- 每組的成員同時在 JS 和 `kun:dom`，`any` 的照 RFC 0008 的規則只給 JS。
- 型別和分層照 Chromium，宣告照 Blink 的目錄放（RFC 0014）。

不在範圍內（H2，或各自的 RFC）：
- `<template>.content`、`<img>` 和 `Image`、`a.href`、`HTMLElement.title`、GlobalEventHandlers、`isConnected`、`readyState`、`getElementsByName`、`crypto.randomUUID`、`requestIdleCallback`（H2）。
- `Blob`（2026-10-07 移到 H2）、`URL.createObjectURL`、`revokeObjectURL` 和 `blob:` 網址、`File`、`FileReader`。
- `PerformanceObserver`、`performance.toJSON()`、Navigation Timing、Resource Timing。
- `navigator` 的其他 spec 的成員：`wakeLock`、`clipboard`、`geolocation`、`connection`、`scheduling`、`userAgentData`，和 `plugins`、`mimeTypes`。框架用它們偵測功能，**不能有**。
- `autofocus`、`tabIndex`（F：輸入事件和 focus）。
- structured clone（J：`MessageChannel`）。

## 設計概要

**給寫網頁的人**：
- `navigator.userAgent`、`platform`、`vendor`、`language`、`languages`、`hardwareConcurrency`、`onLine`、`cookieEnabled`；`navigator.wakeLock` 等是 `undefined`。
- `performance.mark("a")`、`performance.measure("a-b", "a", "b")`、`performance.measure("x", { start, end, duration })`、`getEntriesByName("a")`、`clearMarks()`；`performance.timeOrigin`；`new PerformanceMark("m")`。`performance` 照 WebIDL 可以被蓋掉。
- `el.dataset.fooBar` 讀寫 `data-foo-bar`，`delete el.dataset.fooBar`、`Object.keys(el.dataset)`、`for...in`。
- `document.title` 讀寫第一個 `<title>`（沒有就在 `<head>` 建一個），分頁的標題跟著變。

**給 wasm 的 guest**：同樣的 API 在 `kun:dom` 裡（`navigator`、`performance`、`dom-string-map` 等 resource）；降階的 `window.performance-now` 換成 `performance.now`（不相容的改動，見「WIT 和版本」）；`detail` 是 `any`，只給 JS。

**給改 kun 的人**：加 API 的流程不變（`crates/dom_api/README.md` 的「怎麼新增一個 API」）。宣告表多幾種形式（「宣告表的新形式」），新的 GC 型別照 `crates/dom/README.md` 的「怎麼修改」。

## 詳細設計

型別名稱照 Chromium，宣告照 Blink 的目錄放（`crates/dom_api/src/interfaces/`）：`Navigator` 在 `core_frame.rs`（Blink 的 `core/frame/navigator.idl`），`DOMStringMap` 在 `core_dom.rs`，`HTMLElement.dataset` 在 `core_html.rs`，`Document.title` 在 `core_dom.rs` 的 `Document`（RFC 0014 的第 5 條：在 interface 的尾端開一個 `// HTML` 的區塊）；`Performance` 和 entry 開新的 `core_timing.rs`（`core/timing`）。`ObjectKind` 照檔分區塊。

### 1. `navigator`

細節和 UA 的比較見 `details.md` 的「navigator」。

- **成員**：`NavigatorID`（`userAgent`、`appCodeName`、`appName`、`appVersion`、`platform`、`product`、`productSub`、`vendor`、`vendorSub`）、`NavigatorLanguage`（`language`、`languages`）、`NavigatorOnLine`（`onLine`）、`NavigatorCookies`（`cookieEnabled`，`false`：kun 沒有 cookie）、`NavigatorConcurrentHardware`（`hardwareConcurrency`）。值見 `details.md` 的「成員和值」。
- **UA**：U2，和 Chrome 的 reduced UA 完全一樣（第 6 條；版本是連結的 V8 的 milestone）：用 UA 判斷瀏覽器的框架（CodeMirror、ProseMirror、Vue）走和 Chrome 一樣的路，Speedometer 的比較才公平。代價是伺服器和網站認不出 kun（記進 `docs/limitations.md`）。M1 的 runner 和三個 suite 都不讀 UA；M2 之後的 suite 依 UA 走不同路徑的程度，2026-10-07 只讀了幾個，沒有逐一核對。`vendor` 等照 Chrome 的那組值（WPT 的 `navigator.any.js` 照 UA 決定預期）。
- **UA 從哪裡來**：只有一個來源。Browser 這一側算出來，`net` 送出的 HTTP `User-Agent` 標頭用它（`NetworkContextParams::user_agent`，Chromium 的同名欄位；原本是 `net` 寫死的 `USER_AGENT`），啟動 renderer 的 `RendererFlags` 帶著它；renderer 經過 `FrameClient` 交給 `dom_api`（Blink 的 `LocalFrameClient::UserAgent()`）。符合 `docs/architecture.md` 的「Process 的邊界」：只多一個能序列化的字串。
- **GC**：新的 GC 型別 `Navigator`（realm 的 window、wrapper），window 第一次讀時建、撐住它（`[SameObject]`）。2026-10-08：RFC 0019 的 `Location`、`History` 各是自己的 GC 型別，`Navigator` 照同樣的做法，不再考慮「window 擁有的物件共用一個型別」（`details.md` 的「GC」那張表）。

### 2. `performance` 和 User Timing

細節見 `details.md` 的「performance 和 User Timing」。

- **interface**：`Performance`（`now()`、`timeOrigin`、`mark`、`measure`、`clearMarks`、`clearMeasures`、`getEntries`、`getEntriesByType`、`getEntriesByName`）、`PerformanceEntry`（`name`、`entryType`、`startTime`、`duration`）、`PerformanceMark`（建構子、`detail`）、`PerformanceMeasure`（`detail`）；`Window.performance` 是 `[Replaceable]`。宣告表拿掉降階的 `performanceNow`，`dom_js` 的 glue 拿掉組 `performance` 的那一段。
- **GC**：`Performance` 是 `EventTargetImpl` 的一種（第 10 條）（規範是 `Performance : EventTarget`，照 RFC 0010 的「已決定的事項」第 16 條的 `AbortSignal`），拿著 entry 的串列；entry 是新的 GC 型別 `PerformanceEntry`，照種類分 `PerformanceMark`、`PerformanceMeasure`（像 `Event`，第 15 條）；buffer 是新的在前的鏈。`mark()` 回傳的物件就是 `getEntries()` 裡的那一個，所以 entry 從建立起就是 GC 物件。
- **時間原點**：O3，每個 window 一個 time origin，照 Chrome 從導覽開始算（第 9 條）：browser 開始導覽時記下時間，`CommitNavigation` 帶「從導覽開始到送出 commit 經過多久」，renderer 往回推成自己時鐘上的時刻；子 frame 由 renderer 自己開始導覽時記下，初始的 `about:blank` 和 `srcdoc` 取建立的時刻，同一份文件內的導覽不換。`now()` 是現在減去它再降到 100 µs，`timeOrigin` 是它的 Unix 時間；`Event.timeStamp` 照事件的 relevant global 的原點換算（建立它的 realm 的；embedder 建的取第一次分派的 target 的。一開始照 Blink 的 `Event::timeStamp(ScriptState*)` 用讀它的 realm，WPT 的 `Event-timestamp-cross-realm-getter` 退步才改）；rAF 的時間是 renderer 開始 main frame 的時刻，一樣換算（第 12 條）；DOM 內部的 timer 照舊用 renderer 的時鐘。比較見 `details.md` 的「時間原點」。
- **`mark` 和 `measure`**：照 User Timing Level 3 的步驟（`details.md` 的「`mark` 和 `measure` 的步驟」）。`measure` 的開始或結束是 `PerformanceTiming` 的 attribute 名稱時，`navigationStart` 是 0，其他的丟 `InvalidAccessError`（第 16 條，記成限制）。
- **`detail`**：照規範 structured clone（第 14 條）：用 RFC 0019 的 `SerializedScriptValue`（宣告表的 `Base::Serialized`），mark 時 serialize、不能 clone 的丟 `DataCloneError`，讀時 deserialize、快取在 entry 上（同一個 mark 每次讀到同一個物件）；WIT 是 guest 自己的 bytes，所以 `detail` 也給 wasm。
- **buffer**：沒有上限（規範和 Chrome 都沒有），`getEntries()` 照 `startTime` 排序。`PerformanceObserver` 不做。

### 3. `dataset`

細節見 `details.md` 的「dataset」。

- `HTMLElement.dataset`（`[SameObject]`）回傳 `DOMStringMap`。名稱的轉換照 HTML（`fooBar` ↔ `data-foo-bar`，寫入時的 `SyntaxError`、`InvalidCharacterError`），每次從元素的 attribute 算，不快取（和 Blink 的 `DatasetDOMStringMap` 一樣）。
- **GC**：新的 GC 型別 `DatasetDomStringMap`（元素、wrapper），元素的 `NodeRareData` 撐住它，照 `element.style` 的 `InlineCssStyleDeclaration`（第 17 條）。
- **bindings**：`DOMStringMap` 的 getter、setter、deleter 是匿名的特殊操作，加上 supported property names 和 `[LegacyOverrideBuiltIns]`，做成宣告表的通用形式（第 18 條）：`[NamedDeleter] deleter(name)` 和 interface 的 `[LegacyOverrideBuiltIns]`。JS 照 WebIDL 的 legacy platform object：`[LegacyOverrideBuiltIns]` 的 named interceptor 是 masking 的，有 named setter 的裝 definer（值經過 setter、accessor 拒絕；`style` 的也是），有 deleter 的裝 deleter；有 named property、沒有 indexed getter 的 interface 另外裝一個 indexed interceptor，把陣列的 index（`dataset[9]`）當成名稱轉過去（WPT 的 `dataset-binding` 找到的，Blink 產生的 binding 也這樣做）。WIT 裡匿名的特殊操作叫 `get`、`set`、`delete`、`names`（第 19 條）。不選「`glue.js` 用 `Proxy` 包 `getAttribute`」：不是 `DOMStringMap` 的實例、不是 `[SameObject]`，wasm 也沒有。

### 4. `document.title`

細節見 `details.md` 的「document.title」。

- getter 和 setter 照 HTML（SVG 的 document element 用它的 `title` 子元素；HTML 的沒有 `<title>` 時建一個放進 `<head>`）。renderer 的 `document::title` 搬到 `dom_api`，只留一份實作。
- **分頁的標題**：T3，照 Blink（第 20 條）。`dom` 的修改原語在 `<title>` 可能變了時（插入或移除的子樹裡有 HTML 或 SVG 的 `title`，和 iframe 的 insertion steps 走同一次；插進或移出 `title`；它的文字改變）記下那份 document；renderer 在每個 task、每個 timer 和 parser 的每一段之後看主 frame 的 document 有沒有被記下，有就重新算，和上次送的不同才送 `RendererHostMsg::UpdateTitle`；parse 完還沒送過時送一次（沒有 `<title>` 的頁面換掉上一頁的標題）。一開始只在一批 task 之後檢查，連續的 timer 只送最後一個，改成每個 task 之後。不選 T1（分頁和 `document.title` 不一致）；不選 T2（rendering update 時重算）：沒有 `<title>` 的大 DOM 每次改都要走整棵樹。情境用新的 `wait-title` 指令檢查分頁的標題（第 21 條）。
- **process 的邊界**：沿用已有的 `UpdateTitle`，不加訊息；只送主 frame 的（Chromium 在 browser 那邊忽略子 frame 的，kun 在 renderer 就不送）；browser 照 Chromium 截斷太長的標題。

### 共用的部分

- **宣告表的新形式**（每種的 JS 和 WIT 見 `details.md` 的「宣告表的新形式」）：`[Replaceable]`（第 2 階段）；union 的成員是字典（第 2 階段，`measure` 的參數，第 13 條）；匿名的 named deleter 和 `[LegacyOverrideBuiltIns]`（第 3 階段；getter、setter、supported names 已經由 RFC 0016 做好，它的第 8 條定下的寫法，deleter 照同一個形式加）；`FrozenArray<DOMString>` 的回傳（`navigator.languages`，第 1 階段）。Promise 的回傳、`BufferSource`、`ArrayBuffer`、`unsigned long long` 的回傳跟著 `Blob` 移到 H2。
- **新的 GC 型別**：`Navigator`、`PerformanceEntry`、`DatasetDomStringMap` 三個，加上 `EventTargetImpl` 的一個種類（`Performance`），和 window、元素上新的 edge（`details.md` 的「新的 GC 型別」）。每個照 `crates/dom/README.md` 的「怎麼修改」：`script_objects!` 一行、`Object` 和 `ObjectKind`、泛型的 GC 測試、工作負載、`soundness.md`；動到 GC，RFC 完成時在 macOS 上用 debug 的 V8 跑 `test.py matrix` 和 `fuzz`。
- **JS 和 wasm 都給**（RFC 0012 的「已決定的事項」第 7 條：例外在各自的 RFC 決定）：**全部兩邊都給，沒有例外**（第 4 條）。四組都和 engine 無關，WIT 從宣告表產生，給 wasm 的額外成本主要是 `dom_wasm` 的 guest 測試；`any`（`detail`）照 RFC 0008 的規則只給 JS。可能的例外只有 `dataset`（guest 用 `getAttribute("data-…")` 一樣做得到），但匿名的特殊操作在 WIT 只是一般的函式，不值得當例外。
- **WIT 和版本**：新的 resource 和函式只是新增；拿掉降階的 `window.performance-now` 是不相容的改動：拿掉它、升一次 minor（W1，第 5 條），用到它的 guest 改用 `performance.now` 再用 `ci.py guests` 重新編。升版的時機照 RFC 0012 的「已決定的事項」第 10 條：不和另外兩條線協調「一批只升一次」，`window.performance-now` 是已經發布的版本就有的，拿掉它就升；rebase 時 golden file 有衝突，用 `MACHOPO_UPDATE_GOLDEN=1` 重新產生、重新編 guest，再照同一個規則判斷。比較見 `details.md` 的「WIT 和 `kun:dom` 的版本」。

### 測試

- **語意**：每個成員在 `dom_api` 的 `tests/` 測（不經過 engine）：`navigator` 的值（UA 從 `FrameClient` 來）；`mark` / `measure` 的每種參數組合和例外、entry 的排序和同一個物件、`clear*`、`timeOrigin` 和 `now()` 的換算（frame 的時鐘用測試的 `FrameClient` 控制）、iframe 的 window 有自己的原點；`dataset` 的名稱轉換、`SyntaxError`、`InvalidCharacterError`、只看 null namespace 的 attribute；`document.title` 的 getter 和 setter 的每種情況。
- **bindings**（`dom_js`）：新的宣告表形式各自測：`[Replaceable]`（`performance = 1` 之後是 `1`，現在的 `performance_now` 測試改寫）、union 的字典和 `double`、`DOMStringMap` 的 interceptor（設定、刪除、`Object.defineProperty`、`Object.keys`、`for...in`、masking：`data-to-string` 蓋掉 `toString`）、`languages` 每次是同一個凍結的陣列。
- **wasm**（`crates/dom_wasm/tests/`）：照 RFC 0008 的規則，每種新的宣告表形式、新的物件種類用 C 的 guest 測一次橋接（`dom-string-map` 的 `get`、`set`、`delete`、`names`；`performance` 的 `mark`、`measure` 和 entry）；`performance-now` 換成 `performance.now` 的 guest 跟著改。
- **分頁的標題**：`dom` 的測試守住標記（插入或移除含 `title` 的子樹、插進或移出 `title`、改它的文字都會標記，SVG 的也是；不相干的改動、別的 namespace、沒有 browsing context 的不會；`innerHTML`、`textContent` 經過同樣的插入和移除）；`browser` 的測試（`the_tab_title_follows_document_title`）確認 renderer 在 script 改了標題之後送出 `UpdateTitle`、沒變不送、子 frame 的不送，browser 截斷太長的標題。
- **GC**：每個新的 GC 型別加進泛型的 GC 測試、`dom` 的測試工作負載和 `gc_stress`；新的 edge 各有「GC 之後還在、沒人拿著時被回收」的測試。
- **WPT**（`../third_party/wpt`，不改它）：每個階段把它解開的目錄或檔案加進 `4ff8d6e:tests/wpt/testharness-include.txt`，跑 `MACHOPO_UPDATE_EXPECTATIONS=1 python3 scripts/test.py wpt`，跑不起來的照原因放進 `testharness-skip.txt`（做法見 `tests/wpt/README.md` 的「testharness.js tests, reftests and crashtests」）。候選（每個目錄能跑多少見 `details.md` 的「WPT 的調查」）：`html/webappapis/system-state-and-capabilities/the-navigator-object/` 的部分檔案（第 1 階段）；`user-timing/`、`hr-time/` 的部分檔案（第 2 階段）；`html/dom/elements/global-attributes/` 的 `dataset*`（第 3 階段）；`html/dom/documents/dom-tree-accessors/` 的 `document.title-*.html`（第 4 階段）。規範的 WebIDL 比對（`cargo test -p dom_api --test spec`）加讀 `user-timing.idl`、`performance-timeline.idl`，`spec-missing.txt` 和 `spec-differences.txt` 跟著更新（`any` 和匿名的特殊操作寫明原因）。
- **在視窗裡看得到的**：`about:demo`（`resources/demo.html`）的 `performance.now()` 按鈕：用 `performance.mark` 和 `measure` 量點擊之間的時間、用 `dataset` 記點擊次數、把次數放進 `document.title`；`demo-script` 情境檢查次數、`data-clicks` 和分頁的標題（`wait-title`）。第 4 階段為 `wait-title` 動到視窗那一層（`scenario.rs`、`automation.rs`），跑了 `test.py automation`。
- **mutation test**：每個階段都跑（`docs/testing.md` 的「Mutation test」），結果寫進 `progress.md`。

### 分階段

順序照 M1 的收益和大小：先做最小、runner 一開始就擋住的 `navigator`，順便驗證三條線並行的流程（RFC 0012 的第 5 條）。每個階段做完就 merge 回 `main`（`rfcs/README.md` 的「流程」第 5 步「階段的 merge」），所以每個階段都要留下能編譯、測試全過、文件一致的狀態。每個階段 merge 時要注意：
- **WIT 的版本**：第 2 階段（2a）拿掉 `window.performance-now`，是 H1 唯一不相容的改動，照「WIT 和版本」升版、重新產生 golden file、`ci.py guests` 重新編 guest；其他階段只新增 WIT。不和另外兩條線協調「一批只升一次」；rebase 時 golden file 有衝突，就重新產生再判斷（RFC 0012 的「已決定的事項」第 10 條）。
- **文件跟著階段走**：那個階段解決的 `docs/limitations.md` 的限制當場拿掉、新的限制當場加上，`progress.md` 記那個階段的驗收和 mutation test；`main` 上的這份 RFC 寫明做到哪個階段。
- **GC 的檢查**：第 1 到 3 階段各加新的 GC 型別；`test.py matrix` 和 `fuzz` 在 RFC 完成時跑（`CLAUDE.md` 的規則），不是每個階段。

1. **`navigator`**：UA 的來源（Browser 算、`RendererFlags` 帶到 renderer、`FrameClient`、HTTP 標頭改用它）；`Navigator` 的 GC 型別和成員；`FrozenArray` 的回傳；WPT 的 `the-navigator-object/` 的部分檔案。只新增 WIT。
2. **`performance` 和 User Timing**：
   - 2a 宣告表的 `[Replaceable]`；`Performance`（`EventTargetImpl` 的種類）、`now()`、`timeOrigin`、每個 window 的時間原點和 rAF、`Event.timeStamp` 的換算；拿掉 `performanceNow` 和 glue，guest 改用 `performance.now`（WIT 不相容）；`docs/limitations.md` 拿掉「`performance` 是普通物件」「`self` 不是 `[Replaceable]`」「所有 frame 共用時鐘」這幾條；
   - 2b `PerformanceEntry`、`mark`、`measure`（union 的字典和 `double`）、`clear*`、`getEntries*`、`PerformanceMark` 的建構子；WPT 的 `user-timing/`、`hr-time/` 的部分檔案。
3. **`dataset`**：宣告表的匿名特殊操作、named setter、deleter、supported names、`[LegacyOverrideBuiltIns]`（`dom_js` 的 interceptor、WIT）；`DatasetDomStringMap`；WPT 的 `dataset*`。
4. **`document.title`**：`dom_api` 的 getter 和 setter（renderer 的搬過來）、`dom` 的標記、renderer 送 `UpdateTitle`；`about:demo` 和情境；WPT 的 `document.title-*.html`。
5. **收尾**：各階段已經改過的文件（`about:demo` 和情境、`docs/limitations.md`、`crates/dom_api/README.md`、`crates/dom_js/README.md`、`soundness.md`）整體核對一次；在 macOS 上用 debug 的 V8 跑 `test.py matrix` 和 `fuzz`；RFC 改寫成完成時的樣子（`docs/README.md` 的「一份 RFC 做完時」）。

## 缺點

- **新增三個 GC 型別和一個 `EventTargetImpl` 的種類**：每個都要 struct、`Object` 的分支、GC 測試和 `soundness.md` 的規則（RFC 0010 第 8 條之後的成本），比 Blink 從 IDL 產生的多。
- **宣告表多五種形式**，每種都要在 `dom_js` 和 WIT 各寫一次、各測一次；DOM 線的 RFC 加了跨線會用到的形式（`[Replaceable]`、union 的字典），其他線要跟著知道。
- **`performance` 從 glue 物件變成 interface**：`performance.now()` 多一次 attribute getter 的呼叫（現在是讀普通物件的屬性），在 Speedometer 量的區間外面，但會稍微改變 runner 的開銷；`kun:dom` 有一處不相容。
- **UA 看起來像 Chrome**（U3）：網站以為是 Chrome，用 kun 沒有的功能時壞得更徹底；和 Chrome 的 UA 版本號要跟著 V8 的升級改。
- **時間原點和 Chrome 稍有不同**：頁面請求的導覽（點連結、`location`），Chrome 從 renderer 開始導覽算，kun 從 browser 收到請求算，差一次跨 thread 的時間（第 9 條）。
- **分頁的標題每個 task 送一次**：同一個 task 裡改好幾次只送最後一次（Chrome 每次都送）；「每個 task 之後」只有審查守住（`docs/testing.md`）。
- **`document.title` 的標記加在 `dom` 的修改原語**：和 mutation observer、iframe 的 insertion steps 一樣，每個原語多一個判斷；漏了某種改法時分頁的標題不更新，只有測試抓得到。

## 理由和其他做法

- **四組放在一份 RFC、各是一個階段**：照 RFC 0010 的切法。它們彼此無關，但都是 M1 要的、都照 RFC 0008 的做法，分成四份 RFC 時每份的設計都只有一兩個選項；放在一起，宣告表的新形式和 GC 型別的成本一次看得清楚，每個階段一樣可以分開決定和驗收。不選「H1 再拆小」：RFC 0012 的第 5 條已經把 M1 要的部分拆出來了。
- **範圍照 M1 的實際用法，加上很小、一起做比較自然的成員**：`navigator` 的 mixin、`getEntries*`、`DOMStringMap` 的 setter 和 deleter 都和 M1 要的部分共用同一個物件和同一套機制，分開做要回頭再改一次。比較大、M1 用不到的（`PerformanceObserver`）移出去；`Blob` 單獨在 M1 沒有用處、讀取又要新的宣告表形式，2026-10-07 整個移到 H2。
- **照 Chromium 的類別**：`Navigator`、`Performance`（`WindowPerformance`）、`PerformanceEntry` 的子類別、`DatasetDOMStringMap` 都有對應；和 Chromium 不同的地方（`Performance` 是 `EventTargetImpl` 的一種、entry 一個型別照種類分）在各段寫了理由。
- 每一項的完整比較在 `details.md`。

## 先例

- **Chromium**（`third_party/blink/renderer/`）：
  - `core/frame/navigator.cc`、`core/execution_context/navigator_base.cc`（`NavigatorID` 等 mixin）；UA 由 browser 算、經過 `LocalFrameClient::UserAgent()`。
  - `core/dom/dom_string_map.idl`、`core/dom/dataset_dom_string_map.cc`；binding 由 IDL 產生 named property interceptor。
  - `core/dom/document.cc` 的 `title()`、`setTitle`、`UpdateTitle`，`core/html/html_title_element.cc`；`RenderFrameHostImpl::UpdateTitle` 只採用主 frame 的。
  - `core/timing/performance.cc`、`window_performance.cc`、`dom_window_performance.cc`、`user_timing.cc`、`performance_entry.cc`、`performance_mark.cc`、`performance_measure.cc`；`TimeClamper`。
- **kun 自己的**：RFC 0008（宣告表、WPT、JS 和 wasm 都給）、RFC 0010（新的 GC 型別的做法、`AbortSignal` 是 `EventTargetImpl` 的一種）、RFC 0014（宣告照 Blink 的目錄）。

## 已決定的事項

1. **`Blob` 整個移到 H2**（2026-10-07，使用者決定）：H1 不做 `Blob`（建構子、`size`、`type`、`slice` 也不做），讀取的方法、`BufferSource`、`URL.createObjectURL` 也不在 H1。理由：`Blob` 單獨在 M1 沒有用處（runner 只把它交給 `URL.createObjectURL`，而 `URL` 在 B），讀內容又要 Promise 和 `BufferSource` 兩種新的宣告表形式，放在 H1 會讓 DOM 線的第一份變大。代價：H1 做完之後 runner 的最後一步仍然丟例外（「動機」的「範圍」）。不選「H1 只做 `Blob` 的建構子、`size`、`type`、`slice`」（原本的看法）：沒有 `URL.createObjectURL` 一樣解不開 runner 的最後一步，WPT 的 `FileAPI/blob/` 沒有 `text()` 也幾乎驗證不了。調查的細節留在 `details.md` 的「給 H2 的筆記：`Blob`」。

2026-10-08 確認（接受）：

2. **範圍照「動機」的「範圍」**：四組（`navigator`、`performance` 和 User Timing、`dataset`、`document.title`）加上 `[Replaceable]`；`navigator` 的 `NavigatorID` 等 mixin、`getEntries*`、`clear*`、`timeOrigin`、`PerformanceMark` 的建構子、`DOMStringMap` 的 setter 和 deleter 一起做，`PerformanceObserver`、`toJSON()`、Navigation Timing 和 `navigator` 偵測功能用的成員（`wakeLock` 等）不做。不選只做 M1 用到的（`navigator` 只要物件存在、`mark` / `measure` 只有字串形式、`dataset` 只讀）：之後要回頭改同一個物件和同一套機制，WPT 也大多過不了。
3. **照「分階段」的五段**：`navigator` → `performance`（2a `[Replaceable]`、`Performance`、時間原點、拿掉 `performanceNow`；2b entry、`mark` / `measure`）→ `dataset` → `document.title` → 收尾；每段做完、審核後 merge 回 `main`。不選先做 `performance`：它最大，又有 WIT 不相容的改動和時間原點的設計，先做最小、runner 一開始就擋住的 `navigator`。
4. **JS 和 wasm 都給，沒有例外**：四組都和 engine 無關，WIT 從宣告表產生；只有 `any`（`PerformanceMark` 的 `detail`）照 RFC 0008 的規則只給 JS。不選 `dataset` 只給 JS：匿名的特殊操作在 WIT 只是一般的函式，省不了多少。
5. **拿掉 `window.performance-now`，`kun:dom` 升到 0.8.0**（W1，2a）：照 RFC 0012 的「已決定的事項」第 10 條，不相容就升 minor，用到它的 guest 改用 `performance.now`、`ci.py guests` 重新編。不選留著、只新增（W2）：同一件事兩個入口，之後還是要拿掉一次。

2026-10-08 確認（第 1 階段）：

6. **UA 和 Chrome 的完全一樣**（U2）：Chrome 的 reduced UA，版本是連結的 V8 的 milestone（`.cargo/config.toml` 的 `RUSTY_V8_MIRROR_TAG`），例如 macOS 上 `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36`；`navigator.userAgent` 和 HTTP 的 `User-Agent` 是同一個。依 UA 分路的程式走和 Chrome 一樣的路。代價：伺服器、網站和 log 認不出 kun，網站用 kun 沒有的功能時壞得更徹底；換 V8 時跟著改版本。不選 Chrome 的加上 `kun/<版本>`（U3，原本的建議）、不選維持現在的 `Mozilla/5.0 (compatible) kun/<版本>`（U1：依 UA 分路的程式走「不認識的瀏覽器」的路，和 Chrome 量到的不是同一段程式）。
7. **宣告表加 `FrozenArray<T>` 的回傳**（F1）：JS 的前端照 Blink 把凍結的陣列快取在 wrapper 上，值沒變時回傳同一個（`navigator.languages === navigator.languages`、`Object.isFrozen`）；WIT 是 `list<T>`。不選當成 sequence（F2：每次新的、不凍結的陣列，和規範、Chrome 不同）；不選先不做 `languages`（F3：i18n 的函式庫常用它）。
8. **`Navigator` 是自己的 GC 型別、UA 只有一個來源**：照 RFC 0019 的 `Location`（第 1 節的 GC，`details.md` 的 N1）；UA 由 browser 算，建立 network context 時交給 `net`（Chromium 的 `NetworkContextParams::user_agent`），啟動 renderer 時放在 `RendererFlags`，renderer 經過 `FrameClient` 交給 `dom_api`（Blink 的 `LocalFrameClient::UserAgent()`）。

2026-10-08 確認（2a）：

9. **時間原點照 Chrome 從導覽開始算，每個 window 一個**（O3）：browser 開始導覽時（使用者的導覽、頁面請求的導覽、重新整理、上一頁和下一頁）記下時間；`Instant` 不能跨 process，`CommitNavigation` 帶「從導覽開始到送出 commit 經過了多久」，renderer 用收到時的時刻往回推（Chromium 的 `CommonNavigationParams::navigation_start`）。頁面請求的導覽，Chrome 從 renderer 開始導覽算，kun 從 browser 收到請求算，差的是跨 thread 的時間。子 frame 由 renderer 自己載入，開始它的導覽時記下；初始的 `about:blank` 和 `srcdoc` 取建立的時刻；同一份文件內的導覽不換原點。`timeOrigin` 是它的 Unix 時間：renderer 的時鐘起點和當時的 `SystemTime` 一起記下，之後只加 `Instant` 的差。不選每個 window 取 commit 的時刻（O2，原本的建議：不用改 `protocol`，但 `now()` 不包括網路的時間，和 Chrome 不同）；不選維持共用的時鐘（O1）。
10. **`Performance` 是 `EventTargetImpl` 的一種**：照 RFC 0010 的 `AbortSignal`，分派、listener、wrapper 沿用，`performance instanceof EventTarget` 成立（Blink 的 `Performance` 繼承 `EventTarget`）；window 第一次讀時建、撐住它，2b 的 entry 也掛在它上面。不選自己的 GC 型別：不是 event target，idlharness 看得出來，之後要事件時再改。
11. **規範標了 `[Replaceable]` 的 attribute 都標**：宣告表加這個形式（JS 的 setter 在 global 上定義同名的普通 data property，WIT 不變），`performance` 和現在已經有的 `self`、`frames`、`length`、`parent`、`event`、`innerWidth`、`innerHeight`、`scrollX`、`scrollY`、`pageXOffset`、`pageYOffset` 一次標完（Chrome 都有）。不選只標 `performance`。
12. **rAF 的時間是 renderer 開始 main frame 的時刻**（A）：renderer 的時鐘減去 window 的原點，和 `performance.now()` 同一條時間軸，不會大於 `now()`，同一個 frame 的 callback 拿到同一個值（Chrome 的性質）；viz 的 `frame_time` 只留給 compositor。和 Chrome 的差別：Chrome 用 `BeginFrame` 本身的時間，比 renderer 開始處理稍早。headless 的 rAF 時間因此不再是 16 ms 的倍數。不選由 viz 送「離現在多久」、renderer 換算（C：最像 Chrome，但 headless 的合成時鐘不是真的時間，換算不了）；不選維持 viz 的時鐘（B：rAF 的時間和 `now()` 不能比較）。

2026-10-08 確認（2b）：

13. **宣告表的 union 可以有字典的成員**（U1）：照 WebIDL 的 union 轉換，`undefined`、`null`、不是 platform object 的物件（也不是 sequence 或 record 成員要的）是字典，其他照原本的規則；WIT 是 `variant` 的一個 record case，wasm 照宣告表；Lua 的 generator 補上「字串或 table」的參數和 record 裡 union 型別的欄位。`measure` 的 `(DOMString or PerformanceMeasureOptions)` 和選項裡的 `(DOMString or DOMHighResTimeStamp)` 照規範宣告。不選由 JS 的 glue 轉（U2，RFC 0018 的 `scrollIntoView` 的做法）：規範的錯誤判斷要在 glue 和宣告表各做一半，wasm 看到的是拆開的形式。
14. **`detail` 用 `SerializedScriptValue`**（D2）：草稿寫的時候還沒有它，RFC 0019 為 `history.state` 加了（它的第 30 條）。mark 時 structured clone（V8 的 `ValueSerializer`），不能 clone 的丟 `DataCloneError`；讀時 deserialize、照 `history.state` 快取在 entry 上，同一個 mark 每次讀到同一個物件。照規範和 Chrome；WIT 是 guest 自己的 bytes，`detail` 也給 wasm（第 4 條的例外不再需要）。不選存原本的值（D1）：mark 之後改了原本的物件，`detail` 跟著變，只給 JS。
15. **`PerformanceEntry` 一個 GC 型別、照種類分 interface**：像 `Event` 一個型別有 `MouseEvent`、`CustomEvent`；`name`、`entryType`、`startTime`、`duration` 共用，mark 的 `detail` 在種類的資料裡。之後的 entry 種類（Navigation Timing、Paint Timing）也加在這裡。Blink 是 `PerformanceEntry` 基底類別加子類別。不選 mark、measure 各自的型別：buffer 要放兩種，多一份 GC 的成本。
16. **`measure` 的開始或結束是 `PerformanceTiming` 的名稱時**（N2）：`navigationStart` 就是時間原點（第 9 條），相對時間是 0；其他的（`loadEventEnd` 等）kun 沒有 Navigation Timing，當成那件事還沒發生：規範和 Chrome 在值是 0 時丟 `InvalidAccessError`。記成限制。`mark` 用這些名稱時照規範丟 `SyntaxError`。不選全部當成找不到的 mark（N1，原本的看法：`SyntaxError`，和 Chrome 不同）。

2026-10-08 確認（第 3 階段）：

17. **`dataset` 是新的 GC 型別 `DatasetDomStringMap`**：拿著元素（edge）和 wrapper，元素的 `NodeRareData` 撐住它、每次同一個（`[SameObject]`），照 `element.style` 的 `InlineCssStyleDeclaration`；每次從元素的 attribute 算，不快取。對應 Blink 的 `DatasetDOMStringMap`（在 `ElementRareData`）。不選把 `InlineCssStyleDeclaration` 一般化成「元素的 view」、照種類分 interface：兩者是不同的 interface、沒有共用的資料，Blink 也分開。
18. **deleter 和 `[LegacyOverrideBuiltIns]` 是宣告表的通用形式**：照 RFC 0016 的第 8 條的寫法加 `[NamedDeleter] deleter(name)` 和 interface 的 `[LegacyOverrideBuiltIns]`。JS 照 WebIDL 的 legacy platform object：interceptor 多 deleter（只刪 supported name，其他的照一般的屬性）和 definer（資料描述子經過 named setter，accessor 描述子拒絕），`[LegacyOverrideBuiltIns]` 的 interceptor 是 masking 的（不加 `NON_MASKING`：`data-to-string` 存在時 `dataset.toString` 是它的值）。等同 Blink 從 IDL 產生的 interceptor；之後集合的 named property 也能用。不選只給 `DOMStringMap` 的特例：RFC 0016 已經把 getter、setter、names 做成通用的，wasm 也要另外處理。
19. **匿名的特殊操作在 WIT 是一般的函式**：匿名的 getter、setter、deleter、names 在 `kun:dom` 叫 `get`、`set`、`delete`、`names`，所有 interface 一樣：`dom-string-map` 有這四個，`css-style-properties` 也多 `get`、`set`、`names`（和 `get-property-value` 等重複，但規則一致）。只新增，`kun:dom` 維持 0.8.0；Lua 的 binding 跟著產生。第 4 條（wasm 沒有例外）因此成立。不選只給沒有具名替代方法的 interface（規則要看有沒有替代的方法），也不選 wasm 不給 `dataset`（違反第 4 條）。

2026-10-08 確認（第 4 階段）：

20. **分頁的標題照 Blink 由 `dom` 標記**（T3）：`dom` 的修改原語在 document 的標題可能變了時標記那份 document：插入或移除的子樹裡有 HTML `title`（或 SVG 的 `title`，和 iframe 的 insertion steps 走同一次子樹），`title` 元素的子節點或文字改變。renderer 在每個 task 和 parser 的每一段之後看主 frame 的 document 有沒有標記，有就用 `dom_api` 的 getter 重算，和上次送的不同才送 `RendererHostMsg::UpdateTitle`；子 frame 的不送，browser 照 Chromium 截斷太長的標題。對應 Blink 的 `HTMLTitleElement::ChildrenChanged`、`InsertedInto`、`RemovedFrom` 和 `Document::UpdateTitle`。不選 rendering update 時重算（T2：沒有 `<title>` 的大 DOM 每次改都要走整棵樹，沒有畫面的 document 不更新）；不選不連動（T1：分頁和 `document.title` 不一致）。
21. **情境加 `wait-title "<text>"` 指令**：等到目前分頁的標題（`BrowserView::title()`，分頁列顯示的那個）是這個字串，像 Chromium 的 `TitleWatcher`；測到 renderer → browser → 分頁列的整條路。動到視窗那一層（`scenario.rs`、`automation.rs`），這個階段要跑 `test.py automation`。不選用 `capture` 和 `expect-differs` 比對分頁列的畫面：只知道變了，不知道變成什麼。

## 待決定

沒有。

## 之後的可能

- **H2**（之後另開）：`<template>.content`、`<img>` 和 `Image`、`a.href`、`HTMLElement.title`、GlobalEventHandlers、`isConnected`、`readyState`、`getElementsByName`、`crypto.randomUUID`、`requestIdleCallback`；這份移出的 `PerformanceObserver`、`performance.toJSON()`、`navigator.plugins` / `mimeTypes`、`clientInformation`；和 2026-10-07 從這份移出的 `Blob`（資料放在哪、成員、讀取要的 Promise 和 `BufferSource` 形式、WPT 的觀察見 `details.md` 的「給 H2 的筆記：`Blob`」）。`URL.createObjectURL` 歸 H2 還是 B（要先有 `URL`），由 H2 和 B 決定；runner 的最後一步要它和 `Blob` 都有。
- **Navigation Timing**：`performance.timing`、`getEntriesByType("navigation")`，`measure` 的 `loadEventEnd` 等名稱（現在是 `InvalidAccessError`，第 16 條）；時間原點已經從導覽開始算（第 9 條），browser 要再傳各個時刻（和 B 一起）。
- **`blob:` 網址的載入和下載**：`Blob` 的資料搬到 Browser（Chromium 的 `BlobStorageContext`、`BlobURLStore`），`net` 讀 `blob:` 網址。
- **集合的 named property**：這份的 setter、deleter、definer、supported names 的形式也可以用來解 `HTMLCollection`、`NamedNodeMap` 的限制（不能設定、刪除時什麼都沒刪、不列在 own keys）。
- **時間的 jitter**：照 Chromium 的 `TimeClamper` 加 jitter，和 site isolation（RFC 0004）一起看。
