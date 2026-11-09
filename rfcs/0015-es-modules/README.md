# RFC 0015：ES module 和 script 的載入

- 開始日期：2026-10-07
- 狀態：✅ 已完成：2026-10-08（Windows 11；設計 2026-10-07 接受，「已決定的事項」第 1 到 13 條；四個階段 2026-10-07 到 2026-10-08 做完，第 1 到第 3 階段做完時各自 merge 回 `main`）
- 細節：`details.md`；進度：`progress.md`
- 來源：RFC 0012 路線圖的代號 A（`rfcs/0012-speedometer/roadmap.md` 的「要開的 RFC」）

## 摘要

讓 JS 文件執行 module script：`<script type="module">` 和它的靜態 `import`（module graph）、動態 `import()`、`<link rel="modulepreload">`，以及 script 插入的 `<script>`（classic、module 和 wasm 的都算），連同 `document.currentScript`、`HTMLScriptElement` 和 `HTMLLinkElement` 的成員。四個階段對應 Speedometer 3.1 的里程碑 M1（靜態 `import`，runner 要的）、M4（`import()`、`modulepreload`）和 M5（script 插入的 `<script>`）。設計照 Chromium 分層：抓取和走訪 module graph 的 `Modulator`（Blink 的 `ModuleTreeLinker`、`ModuleMap`）在和 engine 無關的 renderer，每個 realm 一份；V8 的 `Module` 留在 engine 這一側，由 realm 擁有、跟著 realm 一起放掉，不是新的 GC 型別；module script 排進 `defer` 的佇列，照文件順序執行。「prepare the script element」在 `dom_api`，parser 的和 script 插入的共用；script 插入的在插入它的呼叫結束時 prepare，inline classic script 當場在元素的 realm 執行，iframe 和 script 照樹的順序處理（DOM 的 post-connection steps）。靜態 `import`、`import()` 和 `modulepreload` 是 JS 的功能，wasm 文件沒有；script 插入的 `<script>` 兩邊都有。

## 動機

**開始時的現況（2026-10-07）**：
- renderer 只執行 parser 插入的 classic script 和 wasm 文件的 `type="application/wasm"`（`crates/renderer/src/script.rs`）；`type="module"` 是不認得的種類，不抓、不執行；script 插入的 `<script>` 不執行（`docs/limitations.md` 的「Script」）。`nomodule` 的 classic script 照樣執行（kun 是「不支援 module 的瀏覽器」）。
- V8 的 isolate 沒有設定 module 相關的 host callback，`import()` 不會載入任何東西；`import.meta` 沒有內容。
- 外部的 script 經過 Browser 的 `net` 非同步抓取（`RendererHostMsg::FetchResource`，回應是 renderer 的 task），classic script 抓到就能執行；module 要先抓完整個 graph，抓的過程要 parse 才知道下一層要什麼。
- script 元素沒有 `load`、`error` 事件，沒有 `document.currentScript`。

**問題**：Speedometer 3.1 的 `index.html` 只有一個 `<script src="resources/main.mjs" type="module">`，沒有 module，kun 連 runner 都跑不起來，也就看不到後面真正的錯誤（RFC 0012 的 `roadmap.md` 的「跨線的依賴」）。20 個 default suite 裡 8 個用 module script，5 個另外用動態 `import()` 或 `modulepreload`，NewsSite-Next 用 script 建立 `<script>`（`rfcs/0012-speedometer/details.md` 的「對照表」）。真實的網站也一樣：打包工具的輸出越來越多是 module。

目標：
- 第 1 階段（M1）：`<script type="module">`（外部和 inline）、相對網址的靜態 `import` 和 `export`、module 的循環、`defer` 和 `async` 的時機、`nomodule`、錯誤的回報；Speedometer 的 runner 能開始執行。
- 第 2 階段（M4）：動態 `import()`，在 classic script 和 module script 裡都可以用。
- 第 3 階段（M4）：`<link rel="modulepreload">`。
- 第 4 階段（M5）：script 插入的 `<script>` 照 HTML 的「prepare the script element」執行（classic、module、wasm）。
- WPT 的 `html/semantics/scripting-1/the-script-element/` 裡相關的測試（見「測試」）。

不在範圍內：
- **import map**（`<script type="importmap">`）和 bare specifier（`import "lodash"`）：bare specifier 照規範是解析失敗。盤點沒有記到 Speedometer 用 import map（要在 kun 上跑 M4、M5 的 suite 時再確認，見「之後的可能」）。
- **JSON module、CSS module**（`import x from "./a.json" with { type: "json" }`）：要先有 CORS（見「MIME 和 CORS」）。
- **module worker**、`import.meta.resolve`、source phase import、wasm 的 ESM integration（JS `import` 一個 `.wasm`）。
- **V8 的 code cache、在別的 thread 上編譯或串流編譯**：先在 main thread 上編（J）。
- **`document.write`**：另一個限制，和這份無關。

## 設計概要

**給網頁的作者**：

```html
<script type="module" src="main.mjs"></script>   <!-- 不擋 parser，parse 完照文件順序執行 -->
<script type="module">import { f } from "./lib.mjs"; f();</script>
<script nomodule src="legacy.js"></script>       <!-- 不再執行 -->
```

- module script 一律是 `defer` 的（inline 的也是），加 `async` 的話 graph 抓完就執行；`DOMContentLoaded` 等非 async 的 module script 執行完才分派。
- module 照網址只抓一次、只執行一次：同一份文件裡兩個 graph 都 import 同一個網址時共用。
- 抓不到（網路錯誤、非 2xx、MIME 不是 JavaScript）時在 `<script>` 上分派 `error`；語法錯誤、找不到 export、執行時丟出的例外在 window 上回報（`window.onerror`、`error` 事件）。
- `await import("./x.mjs")`；`<link rel="modulepreload" href="x.mjs">` 先抓好 module（不抓它 import 的）。
- script 插入的 `<script>` 照規範執行：`document.body.append(Object.assign(document.createElement("script"), { src: "x.js" }))` 載入後盡快執行（`async = false` 的照插入順序），inline 的在 `append` 回傳之前就執行完；`document.currentScript` 是執行中的 classic script 的元素。

**給之後改 renderer 和 engine 的人**：renderer 的 `script.rs` 從「一個 script 元素對一個 source」變成「一個 script 元素對一個要抓完的 graph」：`ModuleTreeLinker` 抓根、把 source 交給 engine 編譯、拿回 import 的 specifier、解析成網址、抓下一層，全部抓完時請 engine link，script 輪到時請 engine evaluate。engine 這一側只認得編號（`ModuleScriptId`），V8 的值都留在 engine 的表裡。

## 詳細設計

### 對應 Chromium 的元件

| Chromium（`third_party/blink/renderer/`） | kun | 做什麼 |
| --- | --- | --- |
| `core/script/script_loader.h` 的 `ScriptLoader::PrepareScript` 和它的 flag（`already_started_`、`parser_inserted_`、`non_blocking_`） | `dom_api` 的 `script::prepare`（parser 的和 script 插入的共用）；flag 是 `dom` 的元素狀態的 `ScriptFlags` | 「prepare the script element」：種類、時機、要不要抓 |
| `Node::DidNotifySubtreeInsertionsToDocument`（`HTMLScriptElement`、`HTMLFrameOwnerElement` 的） | `dom` 記下的 `ElementChange`（iframe 和 script 同一個佇列），`dom_api::update_elements` 在每個呼叫結束時、renderer 在 parser 每一步之後處理 | DOM 的 post-connection steps：照樹的順序建子 frame、prepare script |
| `Document::current_script_stack_` | `dom` 的 `push_current_script` / `pop_current_script`、`DomReadScope::current_script` | `document.currentScript` |
| `Document::BaseURL` | `dom_api::document::base_url` | HTML 的 document base URL（`<base>`、fallback） |
| `PendingScript`、`ModulePendingScript` | `renderer` 的 `PendingScript`，用 renderer 自己的編號，拿著元素（`NodeHandle`，第 20 條） | 一個等著執行的 script；module 的是「graph 還在抓」；抓失敗的也輪到它，分派 `error` |
| `HTMLParserScriptRunner`（parser-blocking、parse 完執行的） | `renderer` 的 `Scripts` | 現有的三個佇列，module 排進 `defer` 或 `async` 的 |
| `ScriptRunner`（script 插入的 async 和 in-order） | `renderer` 的 `Scripts` 的 `as_soon_as_possible` 和 `in_order` 佇列 | script 插入的 script 的順序 |
| `Modulator`、`DocumentModulatorImpl` | `renderer` 的 `modulator.rs` 的 `Modulator`，每個 `FrameView` 一個；經過 `ModuleHost`（`FrameView` 實作：engine 和抓取）做 engine 的事 | 每個 realm 的 module 系統的入口：module map、linker、之後的動態 import |
| `ModuleMap` | `Modulator` 的 `map` | 請求的網址 → 抓取中（等它的 linker），或抓完的 module（或失敗） |
| `ModuleTreeLinker`、`ModuleTreeLinkerRegistry` | `Modulator` 的 `Linker`，每個 graph 一個（第 18 條） | 抓整個 graph、處理循環和錯誤、最後 link |
| `ModuleScript`、`JSModuleScript` | `Modulator` 的 `ModuleScript` | engine 的編號、每個 import 解析成的網址（有 parse error 時沒有）；record、parse error、error to rethrow 在 engine |
| `bindings/core/v8/module_record.h` 的 `ModuleRecord`；`ModuleScript` 的 V8 那一半（record、parse error、error to rethrow） | `bindings` 新增的 `ModuleRecords`，每個 realm 一份，由 `dom_js` 的 realm（`JsRealm`）拿著 | V8 的 compile、instantiate、evaluate |
| `ModuleRecordResolverImpl` | `bindings` 的 `ModuleRecords` 放在 context 的 slot 裡的那一半 | `v8::Module` ↔ 編號、每個 module 的 import 解析成哪個 module、`import.meta.url` |
| `DynamicModuleResolver`、`V8Initializer` 的 `HostImportModuleDynamically` | `dom_js` 的 `import_dynamically`（callback）、`bindings` 的 `begin_dynamic_import`、`finish_dynamic_import`，和 renderer 的 `FrameRequests` | `import()` |
| `ReferrerScriptInfo` | 編譯時的 host-defined options（`ScriptLocation::base_url`，第 25 條） | `import()` 的 referrer 的 base URL |
| `core/loader/preload_helper.cc` 的 `ModulePreloadIfNeeded`（`Modulator::FetchSingle`） | `renderer` 的 `FrameView::start_module_preloads`，`Modulator` 的 `GraphFor::Preload` | `modulepreload` |

完整的流程和逐項比較見 `details.md` 的「Chromium 的流程」。

### 抓取和 module graph

A1（已決定的事項第 1 條）：
- **抓取照舊非同步**：每個 module 都經過 `RendererHostMsg::FetchResource`（`RequestDestination::Script`），回應是 event loop 的一個 task。那個 task 裡：檢查狀態和 MIME（見「MIME 和 CORS」）、照 UTF-8 解碼（module 不看 document 的編碼和 `charset`）、請 engine 編譯、拿回 import 的 specifier、照 HTML 的「resolve a module specifier」用這個 module 的網址解析、還沒在 module map 裡的就抓。不在 main thread 上等網路：renderer 不碰網路（「Process 的邊界」），同步等也會卡住 event loop。
- **module map 去重**：同一個網址已經在抓的，等它的結果，不再抓；抓完的直接用。抓失敗的也記下來（規範：map 裡存 null），同一個網址之後不再抓。
- **graph 抓完**（每個子孫都抓完、或其中一個失敗）時：有失敗就不 link，script 是「抓失敗」；否則照規範的順序（深度優先、照 import 的順序）找第一個 parse error，交給 `link_module`，沒有的話先把每個 module 的解析結果交給 engine（`set_module_requests`，每個 module 一次），再 link（V8 的 `InstantiateModule`，失敗是 error to rethrow）。然後告訴 `Scripts` 這個 script 「已經準備好」。
- **抓到的 module 在哪裡交給 engine**：`FrameView` 收到回應時只檢查、解碼，放進一個清單；render thread 接著把它們交給 `Modulator`（`FrameView::modules_arrived`），那時才拿得到 engine 和 DOM，和 wasm script 的「收到 → render thread 交給 engine 編譯」一樣。
- **import 的檢查**（規範的「create a JavaScript module script」）：`type` 以外的 import attribute 是 `SyntaxError`（`import()` 的是 `TypeError`，見「動態 `import()`」）；specifier 照「resolve a module specifier」（`/`、`./`、`../` 開頭的照 module 的 base URL 解析，否則要是完整的網址；沒有 import map），解析不了是 `TypeError`；有 `type` 的（JSON、CSS module，`javascript` 本身不能寫）是 `TypeError`。都記成那個 module 的 parse error（`set_module_parse_error`）。規範對不支援的 `type` 是在載入時失敗、變成 graph 的 error to rethrow，不是 parse error：只有同一個 graph 有好幾個錯誤時，回報的那一個可能不同（`docs/limitations.md`）。原本打算當成抓取失敗（`<script>` 上分派 `error`），1b 對照規範時改掉：規範是在 window 上回報的錯誤。
- **engine 的介面**（已決定的事項第 17 條）：`ScriptEngine` 多五個方法：`compile_module`（回傳 `ModuleScriptId` 和 import 的清單，語法錯誤時清單是 `None`、錯誤留在 engine 的表裡）、`set_module_parse_error`（renderer 發現 specifier 解析不了、或有 `type` 以外的 import attribute 時，請 engine 記下 `TypeError` 或 `SyntaxError`）、`set_module_requests`（每個 import 解析成哪個 `ModuleScriptId`）、`link_module`（graph 有 parse error 時不 link，把第一個 parse error 記成根的 error to rethrow；「第一個」由 renderer 走 graph 時照規範找）、`run_module_script`。V8 的 resolve callback 在 link 的當下問「這個 import 是哪個 module」，不能呼叫回 renderer，所以 renderer 在 link 之前把解析的結果交給 engine；同一個 module 的 import 的解析結果是固定的，每個 module 交一次。Chromium 的 resolve callback 則在 link 的當下由 `core/script` 解析。形狀和逐項比較見 `details.md` 的「ScriptEngine 的介面」。

### module map 和 V8 的 `Module` 的生命週期

- **每個 realm 一份 module map**（B1）：kun 每份 document 一個 realm（RFC 0009 的已決定事項第 4、5、26 條），所以 `Modulator` 放在 `FrameView`，和 realm 一起建立、一起丟掉。iframe 的子 frame 有自己的。和規範（每個 settings object 的 module map）、Blink（每個 `ScriptState` 一個 `Modulator`）一樣。
- **`v8::Module` 由 realm 擁有**（C1）：`dom_js` 的 realm 從 `ScriptState` 改成 `JsRealm`（`ScriptState` 加上 `bindings` 的 `ModuleRecords`），用 `v8::Global<v8::Module>` 拿著每個 module、parse 錯誤和 error to rethrow，renderer 只拿編號。`Realm` 被 drop（導覽換掉 document、移除 iframe）時一起放掉，和現在 `ScriptState` 的 `v8::Global<v8::Context>` 一樣。module 的環境撐住的 JS 物件（和經過 wrapper 撐住的 DOM 節點）活得和 realm 一樣久：規範的 module map 也從不移除項目，Blink 的 `ModuleMap` 由 `Modulator` 撐住，壽命也是那個 `ScriptState` 的。
- **context 的 slot 只放不撐住 V8 值的資料**（K1，已決定的事項第 15 條）：slot 裡的 `Global` 是 root，撐住 module，module 撐住它的 context，context 再撐住 slot，形成 V8 收不掉的循環（`details.md` 的「C：V8 的 Module 放在哪裡」）。V8 的 resolve callback 和 `import.meta` 的 callback 不能帶 closure，但都拿得到 context，所以表的另一半放在 context 的 slot：每個 module 的 `v8::Weak<v8::Module>`、網址、import 的清單和解析成的編號，用 record 的 identity hash 找（hash 不唯一，再比對 `Weak`）。兩個 callback 任何時候都找得到：`import.meta` 第一次被讀時才呼叫，可能在 link 很久之後（例如 event handler 裡）。和 Chromium 一樣從 context 找（`ScriptState::From(context)` → `Modulator` → `ModuleRecordResolverImpl`）；Chromium 的表的鍵是強參照，要在 context 被銷毀時明確清掉，它的註解說「應該讓鍵是 `v8::Module` 的弱參照」，kun 就是這樣做。原本打算從 isolate 的 slot 找（只在 link 的期間放進去，`import.meta` 另外放一張弱參照的登記表），改掉是因為 `import.meta` 的時機，而且從 context 找不用在 isolate 的 slot 裡放進拿出。用 identity hash 而不用 `Module::script_id`：module 執行時丟出例外之後，V8 不再給它的 `script_id`，之後才第一次讀 `import.meta` 的程式就找不到網址。
- **同一條規則的既有違反（1a 修掉）**：`dom_js` 原本把 `DOMException` 的 constructor 用 `v8::Global` 放在 context 的 slot，所以每個 realm 丟掉之後都不會被回收，連同它的 window、document 和整棵樹（2026-10-07 開始 1a 前的實驗發現；`progress.md` 的「做過的檢查」）。改成 context 的 embedder data（V8 把它當成 context 的一部分 trace，已決定的事項第 14 條），和 Chromium 的 `V8PerContextData::constructor_map_`（被 trace 的 `TraceWrapperV8Reference`）一樣不是 root。
- **isolate 拆掉時 slot 裡的 `Weak`（RFC 完成時的 `fuzz` 找到）**：`bindings` 的測試以前沒有 context 放 slot，`ModuleRecords` 是第一個。上游 rusty_v8 在 isolate 拆掉的期間釋放 context 的 slot（和看著 context 的 `Weak<Context>`），那時 `Weak` 不 reset V8 的 handle 就釋放自己的資料，V8 拆除時的 GC 再用它呼叫 callback（use-after-free，`--stress-compaction` 底下每次都發生）。在 fork 修（已決定的事項第 46 條）；論證在 `crates/dom/soundness.md` 的「找到並修正過的問題」，fork 的改動記在它的 `README.machopo.md`。
- **不新增 GC 型別**：module 不在 cppgc 上，不改 `crates/dom/soundness.md` 的 GC 不變式。
- **執行 script 的入口**：evaluate module 會執行頁面的 JS，要經過 `DomIsolate` 的入口，binding 才拿得到 DOM（`crates/dom/README.md` 的「讀寫規則」）；編譯和 link 不執行頁面的 JS，但會配置、可能 GC，也經過入口（D1）。三者都在 `dom_js` 裡經過現有的 `DomIsolate::with_binding_context`（W1，已決定的事項第 16 條）：它本來就是 soundness.md 的 I13 列出的入口，Rust 呼叫 event listener 也是這樣執行 JS 的，所以 `dom` 不用新增入口，I13 的清單不變。原本打算在 `dom` 的 `src/v8_engine.rs` 新增三個和 `evaluate_classic_script` 並列的入口，多三個要審查的入口，好處只有寫法對稱。evaluate 時不拿著 `ModuleRecords` 的借用：先取出 record（`Evaluation`），再執行，頁面的 JS 才不會在借用期間呼叫回 engine。
- **錯誤在 promise 被 reject 時回報**：照規範（「run a module script」的 upon rejection）和 Chromium（`V8ScriptRunner::EvaluateModule` 在 promise 上掛 `ModuleEvaluationRejectionCallback`），evaluate 回傳的 promise 掛一個 reject 的 handler，在 window 上回報例外；掛上 handler 也讓 promise 算成已處理，不會另外分派 `unhandledrejection`。同步丟出的例外也一樣，在緊接著的 microtask checkpoint 回報。handler 在 checkpoint（本身是入口）裡執行，拿得到 DOM。位置照 Chromium 用 V8 的 `Exception::CreateMessage` 從錯誤物件取，所以是 `new Error` 的位置，不是 `throw` 的。
- **`unsafe` 只在 `bindings`**：V8 的 resolve callback（rusty_v8 替它轉成 safe 的 Rust 函式）和 `import.meta` 的 callback（`unsafe extern "C"`）都要用 `v8::callback_scope!` 開 scope（和 `promise_reject_callback` 一樣），放在 `bindings`，附 `// SAFETY:`；這些 callback 不開 DOM scope。

### 執行時機：parser、`defer` 和 `async`

E1：
- `script::prepare`（在 `dom_api`，第 4 階段從 renderer 搬來）認得 `type="module"`（ASCII 不分大小寫，前後空白去掉）：外部的和 inline 的都是 module；沒有 `async` 的排進 `Scripts` 的 `deferred`（和 `defer` 的 classic、wasm script 照文件順序交錯），有 `async` 的排進 `as_soon_as_possible`。`defer` 對 module 沒有意義，照規範忽略。
- **module script 不擋 parser**：parser 照舊在每個 `</script>` 停下來（`html` 不用改），renderer prepare 之後立刻 `resume`。
- 「已經抓好」對 module 是「graph 抓完、link 過（或失敗）」；`next_ready` 的規則不變。
- **`nomodule`**：有 `nomodule` 的 classic script 不執行（kun 原本刻意讓它執行）；對 module script 沒有作用。這改變了原本的頁面的行為，是照規範的改正。
- **`type`**：空字串（或沒有 `type`、`language` 空的）是 JavaScript；只有空白的不是（去掉空白是空字串，不是 JavaScript 的 MIME type），和 Blink 一樣（4d 的 WPT 找到 kun 原本把它當成 JavaScript）。
- **執行**：輪到時請 engine evaluate，接著 microtask checkpoint（現在的 `run_script` 的做法，HTML 的「clean up after running script」）。有 top-level await 的 graph，evaluate 回傳的 promise 不等：後面的 script 和 `DOMContentLoaded` 照樣往下走（規範；RFC 0001 的已決定事項第 42 條的 wasm script 也是這樣）；它被 reject 時回報例外（見「錯誤怎麼回報」）。
- `DOMContentLoaded`、`load`、render-blocking 照現在的 `deferred_done`、`is_empty`：module script 自然算進去。

### 錯誤怎麼回報

照 HTML 的規範（完整的分類見 `details.md` 的「錯誤的分類」）：
- **抓取失敗**（網路錯誤、非 2xx、MIME 不對，graph 裡任何一個）：在 `<script>` 上分派 `error`，不執行，不經過 `window.onerror`。classic 和 wasm script 也有 `load` / `error`（F1；kun 原本沒有這兩個事件）。
- **parse 錯誤、解析不了的 specifier、link 失敗**（例如 import 的名稱沒有 export）：輪到執行時「report the exception」，也就是在 window 上分派 `ErrorEvent`（現有的 `dom_js` 的 `report_exception`），`<script>` 上分派 `load`。
- **執行時丟出例外**、top-level await 的 promise 被 reject：回報例外；這個 promise 標成已處理，不另外分派 `unhandledrejection`。
- **動態 `import()` 失敗**：回傳的 promise 被 reject（抓取失敗是 `TypeError`），沒人處理時走現有的 `unhandledrejection`。
- module 裡的錯誤一律不 mute（不像跨源的 classic script）：module 要過 CORS 才算載入成功。kun 不做 CORS 時這點不成立，見下一節。

### MIME 和 CORS

- **MIME 照規範嚴格檢查**：module 的回應的 MIME essence 一定要是 JavaScript MIME type（`text/javascript` 等，`dom_api::script::is_javascript_mime_type`）；沒有 `Content-Type` 時 `net` 猜成 `text/plain`，也失敗。和 classic script 不同（classic 只擋 image、audio、video、CSV），和 Chromium 一樣。`file:` 的 `.mjs`、`.js` 由 `net` 依副檔名給 `text/javascript`。
- **CORS**：G1，不檢查，跨源的 module 照樣載入，和 classic script 一樣；`crossorigin`、`integrity`、`referrerpolicy` 屬性先忽略，記成 `docs/limitations.md` 的限制。理由：kun 的 `net` 還沒有 CORS，CORS 應該放在 Browser 的 network 層（Chromium 的 `network::CorsURLLoader`），和 `fetch`（路線圖的 L）一起做一次；module 只能是 JavaScript，跨源的 JS 本來就能用 classic script 執行，不 CORS 多暴露的只有沒 mute 的錯誤訊息。JSON、CSS module 會讓頁面讀到跨源的資料，所以排在 CORS 之後（「不在範圍內」）。和 Chromium 不同的地方：`file:` 頁面的 module 在 Chromium 被 CORS 擋下，kun 照樣載入。比較見 `details.md` 的「G：CORS」。

### wasm 文件

照 RFC 0012 的已決定事項（新的 API 同時給 JS 和 wasm，例外在各自的 RFC 決定），H1（已決定的事項第 8 條）：
- **第 1 到第 3 階段只給 JS 文件，wasm 文件明確不做**：靜態 `import`、動態 `import()` 是 JS 的語法，`modulepreload` 是替 JS 的 module map 預先抓取；wasm 文件不執行 JS，`type="module"` 在 wasm 文件裡照舊是不認得的種類，`modulepreload` 不抓。wasm 的 guest 自己的程式在 component 裡已經 link 好，對 `kun:dom` 的 import 由 engine link（RFC 0001），沒有「一個 script 在執行時再載入另一個」的需求。對應物會是「一個 wasm script import 另一個 component」或 wasm 的 ESM integration，都是新的功能，留在「之後的可能」。
- **第 4 階段兩邊都有**：「script 插入的 `<script>` 要執行」是 `dom_api` 和 renderer 的事，和 engine 無關；wasm 的 guest 插入 `<script type="application/wasm" src>` 也照樣執行（wasm 文件沒有會執行的 inline script）。`HTMLScriptElement` 的成員（`src`、`async`、`text`…）加在宣告表，JS 和 wasm 一起有（RFC 0008 的做法）。
- wasm script 的時機照 module script 的規則（RFC 0001 的已決定事項第 42 條），E1 讓它們和 module script 排在同一個 `defer` 佇列，順序規則只有一份。

### Process 的邊界

照 `docs/architecture.md` 的「Process 的邊界」：
- module 的抓取照舊經過 Browser 的 `net`（規則 1）；第 1 階段不改 `protocol`（`RequestDestination::Script` 就是規範的 destination）。做 CORS 時 `ResourceRequest` 才多帶 mode 和 origin（已決定的事項第 12 條）。
- `Modulator`、`ModuleMap`、linker 是每個 `FrameView` 的狀態，engine 的表是每個 realm 的，都不是整個 process 共用的可變狀態（規則 4）；V8 的 host callback 在每個 isolate 上設定。
- 沒有跨 renderer 的 module 快取：Chromium 靠 HTTP cache 和 V8 的 code cache（存在 Browser 那邊），kun 兩者都沒有（`docs/limitations.md` 的「網路」），每份文件重新抓、重新編譯。

### 動態 `import()`

第 24 到 26 條。`V8Engine::create_dom` 在 isolate 上設 `HostImportModuleDynamicallyCallback`（`dom_js` 的 `import_dynamically`，rusty_v8 替它開好 scope，是 safe 的 Rust）。callback 建一個 promise，放在那個 realm 的 module 表（`bindings` 的 `begin_dynamic_import`，給它一個編號），把 specifier、referrer 的 base URL、import attributes 和編號交給 realm 的 `FrameClient::import_module`；frame 當場檢查，不合法的回傳錯誤，callback 立刻 reject（在回到 script 之前）。合法的由 renderer 在 script 跑完之後抓 graph（不 link），再呼叫 `ScriptEngine::finish_dynamic_import`（`DynamicImportResult::Fetched`，帶著 graph 的第一個 parse error）：它在那個 task 裡照規範的「ContinueDynamicImport」，有 parse error 就用它 reject，否則自己 link（失敗就用那次 link 丟出的例外 reject），再 evaluate，evaluate 的 promise 完成時用 module 的 namespace resolve，被 reject 時用原因 reject（Blink 的 `DynamicModuleResolver::NotifyModuleTreeLoadFinished`）。原本 renderer 照 script 的 graph 一樣先 link、engine 只 evaluate：同一個 graph 有兩個 `import()` 一起等時，兩個 linker 在同一個 module 抓到時一起完成、各 link 一次，第二次覆寫了 root 共用的 error to rethrow，兩個 promise 拿到同一個錯誤物件；規範是每個 `import()` 自己 link、各自拿到一個新的例外（WPT 的 `dynamic-import/dynamic-imports-script-error.html`，2c 試跑時抓到）；抓取失敗 reject `TypeError`（`Failed to fetch dynamically imported module: <網址>`，照 Blink 的寫法）。module 的錯誤這時不在 window 回報，是 promise 的 reject，沒人處理時走 `unhandledrejection`。
- **等待中的 promise**：放在 context 的 slot 的表裡，用 `v8::Global` 拿著，是「slot 不放強參照」的唯一例外：`ModuleRecords` 被 drop（realm 被丟掉）時清掉它們，所以撐不住已經丟掉的 realm；promise 永遠不 settle，和規範一樣。不放在 realm 擁有的那一半：callback 和 evaluate 完成時的 handler 只拿得到 context。evaluate 的 handler 用編號找回 promise（rusty_v8 不能把 `Value` 轉回 `PromiseResolver`）。
- **renderer 這一側**：`FrameRequests::import_module` 當場用 `modulator::resolve_dynamic_request` 檢查（和靜態 import 的檢查一樣，只是 `type` 以外的 attribute 是 `TypeError`：ECMAScript 的 `EvaluateImportCall`，WPT 的 `dynamic-import-with-attributes-argument` 測它；沒有 referrer 時用一個佔位的 base，因為「解析得了嗎」和 base 無關），合法的記下來。render thread 在 event loop 的最上層從整個 frame tree 取出這些請求（`FrameView::take_dynamic_imports`），照 referrer 的 base URL 或 document 當時的 base URL 解析，交給 `Modulator::fetch_tree`（`GraphFor::Import`）；graph 抓完（或已經在 module map 裡）時呼叫 engine 的 `finish_dynamic_import`，每個之後一次 microtask checkpoint；它們執行時又呼叫的 `import()` 在同一個迴圈接著處理。`import()` 為了自己抓的 module 不延後 document 的 `load`（WPT 的 `delay-load-event.html`，和 Blink 一樣）：`ModuleHost::fetch` 帶著 `delays_load`。
- **referrer 的 base URL**（第 25 條）：`bindings` 的 `ScriptLocation` 多一個 `base_url`，classic script 和 module 編譯時放進 V8 的 host-defined options（一個字串的 `PrimitiveArray`，callback 拿到時讀回來；V8 內部把它當 `FixedArray`）。`ScriptEngine::run_classic_script` 多一個 `base_url` 參數：外部的 classic script 是它的網址，inline 的和 `javascript:` 網址是 document 當時的 base URL。event handler 不帶（Blink 也不帶），由 renderer 用 document 當時的 base URL。字串形式的 timer 現在不支援（`details.md` 的「第 2 階段的細節」）。

### `modulepreload` 和 `HTMLLinkElement`

第 27 到 33 條。`start_loads` 認得 `<link rel="modulepreload">`，只抓那一個 module、編譯、放進 module map，不抓子孫（規範允許但不要求連子孫一起抓；Chromium 的 `PreloadHelper::ModulePreloadIfNeeded` 用 `Modulator::FetchSingle`，也只抓一個），`<link>` 上分派 `load` / `error`，不等子孫（規範的寫法，WPT 的 `modulepreload-fires-before-dependencies.html`）。之後的 `<script type="module">` 和 `import()` 從 map 拿到同一個，子孫等真的有 graph 用到時才抓。沒有它頁面也能跑（只是提示），但 Chrome 在 parse 時就開始抓，量 Speedometer 時兩邊的時機會不同。
- **`HTMLLinkElement`**（第 28、30 到 32 條）：WPT 的 `preload/` 幾乎都用 `link.rel`、`link.relList.supports(…)` 建立 `<link>`，所以一起做最小的 `HTMLLinkElement`：`href`（URL 的 reflection）、`rel`、`relList`、`as`（「limited to only known values」：規範的 potential destination，轉成小寫；其他是空字串）、`crossOrigin`（列舉的 reflection：沒有時 `null`，不合法的是 `anonymous`）、`integrity`；`crossOrigin` 和 `integrity` 只有 reflection，沒有對應的行為（第 7 條）。`relList` 是 `DOMTokenList`，放在節點的 rare data 的 `rel_list`，和 `classList` 一樣每次同一個；它的 `supports()` 只對 kun 真的會處理的 `stylesheet`、`modulepreload` 回答 `true`（不分大小寫），`classList.supports()` 照舊丟 `TypeError`。
- **抓取和事件**：不延後 document 的 `load`（WPT 的 `avoid-delaying-onload-link-modulepreload.html`，和 `import()` 一樣）；抓失敗（網路錯誤、非 2xx、MIME 不對）分派 `error`，抓到就分派 `load`，有語法錯誤也算抓到（module 帶著 parse error 進 map，用到時才回報）；map 裡已經有的也在之後的 task 分派，和 Blink 一樣是非同步的。`as` 照規範是一個 destination：沒寫、或不是任何 destination 的值（「no state」）是 `script`；script-like 的（`script`、`worker` 等）抓成 JavaScript module；其他的（`image`、`fetch` 等）不抓、分派 `error`。`json`、`style`、`text` 是 kun 沒有的 module 種類，也分派 `error`（Blink 抓 JSON 和 CSS 的；3b 讀 WPT 的 `modulepreload-as.html` 時更正了原本「不合法的值也分派 `error`」的寫法）。`href` 是空的不處理，解析不了的不分派事件（規範）。parser 插入和 script 插入的 `<link>` 都處理：render thread 每次 `start_loads` 時 `FrameView::start_module_preloads` 掃 document，每個 `<link>` 只處理一次；規範在 `href` 改了、或再次變成 connected 時重新處理，kun 不會（和 stylesheet 的 `<link>` 一樣，記進 `docs/limitations.md`）。`Modulator` 多一種對象 `GraphFor::Preload`：module 抓到（或已經在 map 裡）就完成，不抓子孫、不 link，抓取不延後 `load`。事件記在 `FrameView`，render thread 在回應的那個 task 裡、和 event loop 的最上層（map 裡已經有的）分派；`<link>` 和等著的 script 一樣用 `NodeHandle` 拿著（第 20 條），script 移除它也收得到。wasm 文件不抓（第 8 條）。

### script 插入的 `<script>`

第 34 到 45 條。

- **何時 prepare**：照 HTML，不是 parser 插入的 script 元素在三個時機 prepare：變成 connected、connected 時有節點插進它、connected 時設了 `src`（照 Blink 的 `ScriptLoader::HandleSourceAttribute`，任何改變都算；規範是「原本沒有的 `src`」，只差在原本有 `src`、但種類不執行而沒 started 的元素）。`dom` 在插入的那一次走訪裡記下（`ElementChange::Script`，和 iframe 的 `ElementChange::Frame` 同一個佇列，第 45 條），`src` 由 `dom_api` 的 attribute change steps 記；`dom_api::update_elements` 在宣告表的每個呼叫結束時（`appendChild` 回傳之前，Blink 的 `DidNotifySubtreeInsertionsToDocument` 的時間點）、renderer 在 parser 每一步之後，照記下的順序處理：同一次插入裡排在 iframe 前面的 script 先執行，那時 iframe 還沒有 `contentWindow`（WPT 的 `Node-appendChild-script-and-iframe.html` 找到 kun 原本先建全部的子 frame）。
- **flag**（第 35 條）：元素記「already started」「parser inserted」「force async」（`dom` 的 `ScriptFlags`；Blink 的 `ScriptLoader` 的 `already_started_`、`parser_inserted_`、`non_blocking_`）。元素建立時 force async 是 true；parser 建的 parser inserted、force async 是 false；fragment parser（`innerHTML` 等）和 scripting 關掉的 parse（`DOMParser`、`parseHTMLUnsafe`）建的另外 already started，永遠不執行（4c 的 WPT 找到 `DOMParser` 的 script `importNode` 進頁面會執行）；`cloneNode` 複製 already started。`script.async` 的 getter 讀 force async，setter 清掉它。
- **prepare**：「prepare the script element」在 `dom_api::script::prepare`，parser 的和 script 插入的共用，照規範改 flag。沒有 `async` 屬性、也沒有 force async（`async = false`）的外部 script 和 module script 照插入順序執行（`Scripts` 的 in-order 佇列，Blink 的 `ScriptRunner`），其他的盡快執行。
- **inline classic script 當場執行**（第 34、43 條）：`update_elements` 在呼叫結束時請 engine 執行（`ScriptHost::run_inserted_script`）：renderer 這時正在執行 script、engine 被借走了，所以由 engine 這一側做，和 `dispatchEvent` 同步呼叫 listener 一樣。在元素所屬 document 的 realm 執行：`dom_js` 從 document 的 window 的 global 拿到 context、從執行中的 binding 的 scope 進入它（`bindings::run_classic_script_in`），例外在那個 realm 回報、不丟回插入它的 script，不做 microtask checkpoint（外層的 script 還在執行）。base URL 是 document 當下的（第 44 條：HTML 的 document base URL 在 `dom_api::document::base_url`，renderer 也用它；`about:blank`、`about:srcdoc` 的 fallback 由 frame 給，`FrameClient::fallback_base_url`），剛插入的 `<base>` 也算。
- **外部的和 module script**（第 40 條）：交給元素所屬 document 的 frame（`FrameClient::script_prepared`，記在 `FrameRequests`），renderer 在下一次 `start_loads`（每次 script 跑完都會到，在決定 `load` 之前）開始抓、排進佇列，所以它們延後 `load`；輪到時在 event loop 的最上層或回應到的時候執行。第 40 條原本寫「在 event loop 的最上層開始抓」，4b 的測試發現那時 parser 可能已經結束、`load` 已經觸發，所以提前到 `start_loads`。準備時的 document 和執行時元素所屬的不同（被搬走）時不執行、不分派事件。frame 跑哪種 script（`FrameClient::runs_scripts`）由 renderer 照 engine 和那個 frame 的 `runs_scripts` 給。
- **`document.currentScript`**（第 37、41 條）：照 Blink 的 `Document::current_script_stack_`，`dom` 替每份 document 記一個堆疊：執行 classic script 時推入元素，module 和 wasm 的推入「沒有」（讀到 `null`），event handler 和 timer 不推入。
- **`HTMLScriptElement`**（第 36 條）：`src`、`type`、`async`、`defer`、`noModule`、`crossOrigin`、`integrity`、`text`、`charset`、`event`、`htmlFor` 和 static 的 `supports()`（`classic`、`module`）。`async` 是 WIT 的關鍵字，`kun:dom` 寫成 `%async`。

### 測試

結果和每一步的檢查在 `progress.md`。

- **單元測試**：`bindings` 和 `dom_js` 在記憶體裡的 source 上編譯、link、evaluate（循環、TDZ、錯誤、top-level `await`、realm 丟掉時放掉 module）、`import()` 和 `onerror`；`bindings` 的 `isolate_teardown` 在 `--stress-compaction` 底下 drop context 放了 slot 的 isolate（第 46 條）；`dom` 的 script 的 flag、要 prepare 的紀錄（和 iframe 的順序）、`currentScript` 的堆疊；`html` 的 parser 設的 flag；`dom_api` 的「prepare the script element」、`HTMLScriptElement`、`HTMLLinkElement`、document base URL；`renderer` 的 `Modulator`（graph、`import()`、preload）和 `Scripts` 的佇列。
- **整個瀏覽器**：`shell` 的 `modules`、`inserted_scripts`、`wasm_scripts`，用 `net::test_server`（HTTP）。
- **WPT**（testharness.js，`python3 scripts/test.py wpt`，不改 `../third_party/wpt`）：每個階段開始時把候選的目錄試跑一次（第 22、33、39 條），能跑的加進清單、失敗的寫 baseline；`../third_party/wpt/html/semantics/scripting-1/the-script-element/` 的 `module/`（含 `dynamic-import/`）、`microtasks/`、`import-attributes/` 和頂層 33 個檔案，`preload/` 的 `modulepreload*.html`，`the-link-element/link-rellist.html`。kun 的 WPT server 沒有 wptserve 的 `.py` handler、`.sub` 的替換、pipe、別的 host 和 HTTPS，靠它們的（CORS、referrer、MIME、charset、`moving-between-documents/`）進 `testharness-skip.txt`。試跑的結果和沒過的原因見 `details.md` 的「WPT」。
- **情境**：`about:demo` 有一個 inline 的 module script（從 `data:` 網址 import、顯示 `import.meta.url`）、`import()` 按鈕（第二次 module 不再執行）、`<link rel="modulepreload">`（預先抓按鈕要 import 的 module）和「Insert scripts」按鈕（inline script 在 `append()` 回傳之前執行、`data:` 網址的 script 載入後執行）；`tests/scenarios/demo-script.txt` 確認四者（`about:` 頁面沒有可以相對解析的網址，所以用 `data:`）。
- **Speedometer**：第 1 階段做完之後，RFC 0012 的 1b 在 kun 上跑 runner（那份 RFC 的事）。
- **靠審查的**：`bindings` 的 `unsafe` callback、context 的 slot 不放強參照、在 binding 裡執行 script（`soundness.md`）；identity hash 碰撞時拿錯 module（`docs/test-gaps.md` 的「只靠審查」）。

### 分階段

照這個順序做完（每一步的狀態和日期在 `progress.md`）：

1. **靜態 `import`**（M1）：
   - 1a engine：修掉 realm 的洩漏（第 14 條）；`bindings` 的 `ModuleRecords`、`dom_js` 的 `JsRealm`、`ScriptEngine` 的新方法；只用記憶體裡的 source 測。
   - 1b renderer：`prepare`、`Modulator`、`ModuleMap`、`ModuleTreeLinker`（第 18 條）、MIME、`Scripts` 的順序、錯誤和 script 元素的 `load` / `error`（F1、第 19、20 條）、`import.meta.url`（I1、第 21 條）。
   - 1c WPT、情境和 `about:demo`、mutation test、`docs/limitations.md`。
2. **動態 `import()`**（M4，第 26 條）：
   - 2a engine：callback、host-defined options、`ScriptEngine` 和 `FrameClient` 的新方法。
   - 2b renderer：`FrameRequests`、`Modulator` 抓 `import()` 的 graph、event loop 的處理。
   - 2c WPT、`about:demo` 和情境、mutation test、文件；每個 `import()` 自己 link。
   - 2d render loop 在 heap 的 task 之後的缺口（2c 碰到的不穩定的測試的原因）。
3. **`modulepreload`**（M4，第 29 條）：
   - 3a `HTMLLinkElement`：`dom` 的 `rel_list`、`dom_api` 的 interface 和 reflection、`relList.supports()`、WIT、`spec-differences`。
   - 3b renderer：認得 `<link rel="modulepreload">`、抓取、`load` / `error`、`shell` 的端到端測試、WPT 的 `preload/`。
   - 3c `about:demo` 和情境、mutation test、文件。
4. **script 插入的 `<script>`**（M5，第 38 條）：
   - 4a `dom`：三個 flag、記下觸發點（connected、插入子節點、設 `src`）；`HTMLScriptElement`；`prepare` 搬到 `dom_api`，parser 的 script 改用它；`Scripts` 的 in-order 佇列。
   - 4b renderer：外部的和 module 的 script 插入的 script 的抓取、佇列、`load` / `error`，`document.currentScript`。
   - 4c engine：inline classic script 的同步執行（第 34 條）；iframe 和 script 照樹的順序（第 45 條）。
   - 4d WPT、`about:demo` 和情境、mutation test、文件。

## 缺點

- **renderer 和 engine 之間多一層來回**：graph 的每一層都要「抓 → 交給 engine 編譯 → 拿回 specifier」，`ScriptEngine` 的介面變大；A2（engine 自己走 graph）介面比較小，但抓取要從 engine 呼叫回 renderer。
- **module 活得和 realm 一樣久**：沒人再用的 module 也不回收，直到 document 被丟掉（規範和 Blink 也是）。長壽的單頁應用動態 import 很多不同的 module 時，記憶體只增不減。
- **比 Chromium 寬鬆的地方**：不做 CORS 時，跨源和 `file:` 的 module 在 kun 載入、在 Chrome 被擋；錯誤訊息不 mute。在 kun 上能跑的頁面在 Chrome 上不一定能跑。
- **`nomodule` 的改變**：依賴 `nomodule` fallback 的頁面改走 module 的路。
- **script 插入的外部 script 的 `src` 晚一點解析**：在插入它的 script 跑完之後（`start_loads`），不是插入的當下；同一個 script 裡插入之後又改了 `<base>` 時結果不同（`docs/limitations.md`）。
- **沒有 code cache、在 main thread 上編譯**：大的 graph 在 kun 上每次載入都重新編譯，量 Speedometer 時這部分會比 Chrome 慢（RFC 0012 的公平的條件要記下）。

## 理由和其他做法

2026-10-07 接受（「已決定的事項」第 1 到 10 條）。完整的比較見 `details.md`。

- **A. 誰走訪 module graph**：
  - **A1. renderer（和 engine 無關的一層）**：`ModuleTreeLinker`、`ModuleMap` 在 renderer，engine 只提供編譯、link、執行。抓取、順序、`load` 事件本來就在 renderer；和 Blink 的分層一樣（`core/script` 在 `bindings` 之上）。
  - **A2. engine（`dom_js`）**：engine 拿著 map、自己走 graph，透過一個 fetch 的 trait 請 renderer 抓。V8 的值和 map 在同一邊，但抓取、重新導覽時丟掉回應的邏輯要分到兩邊。
  - **A3. 在 main thread 上同步抓**：不行：renderer 不碰網路，等回應會卡住 event loop。
  - 選 A1。
- **B. module map 的範圍**：**B1. 每個 realm**（規範、Blink）；**B2. 每個 renderer 共用**：不同 frame 會共用 module 的 instance，違反規範。選 B1。
- **C. `v8::Module` 放在哪裡**：
  - **C1. realm 擁有的表，`v8::Global`**：跟著 `Realm` drop，不新增 GC 型別、不改 GC 的論證。
  - **C2. cppgc 的 GC 型別**（Blink 的 `ModuleScript` 用 `TraceWrapperV8Reference<v8::Module>`）：要新增 GC 型別和 `JsEdge` 的種類，`crates/dom/README.md` 的「怎麼修改」整套成本；好處是 module 沒人撐住時能回收，但 map 一直撐著它們，實際上回收不了。
  - **C3. context 的 slot**：會形成收不掉的循環。
  - 選 C1。
- **D. 編譯和 link 走不走入口**：**D1. 都經過接 `&mut DomIsolate` 的方法**，入口的清單只有一份規則；**D2. 只有 evaluate 經過入口**，編譯和 link 直接用 isolate：比較少，但多一條「這裡不會執行 JS」要靠審查。選 D1。
- **E. module script 怎麼排進執行順序**：**E1. 沿用 `Scripts` 的佇列**，「已經抓好」改成「graph 抓完」；**E2. module 另一個佇列**：規範要求 `defer` 的 classic 和 module 照文件順序交錯，兩個佇列要另外合併。選 E1，和 Blink 的 `HTMLParserScriptRunner` 一樣。
- **F. script 元素的 `load` / `error`**：**F1. 第 1 階段一起做**（classic 也有）：module 的抓取失敗只能從這裡看到，WPT 的 module 測試大多用它；**F2. 之後**：第 1 階段的錯誤只寫 log。選 F1。
- **G. CORS**：**G1. 不檢查**；**G2. 第 1 階段就做 CORS**：要改 `protocol` 和 `net`（送 `Origin`、檢查 `Access-Control-Allow-Origin`），和 `fetch` 重複；**G3. 只准同源**：比 Chrome 更嚴，CDN 上的 module 全部失敗。選 G1，CORS 和 `fetch`（路線圖的 L）一起做。
- **H. wasm 文件**：**H1. 第 1 到第 3 階段不做、第 4 階段兩邊都有**；**H2. 替 wasm 做對應物**（script 之間的 component 匯入）：新的功能，沒有使用者。選 H1（見「wasm 文件」）。
- **I. `import.meta`**：**I1. 第 1 階段做 `url`**：一個 callback，很多 module 用 `new URL("./x", import.meta.url)`；**I2. 之後**：Speedometer 的 runner 沒用到。選 I1。
- **J. 在哪裡編譯**：**J1. main thread**；**J2. 背景 thread 或邊抓邊編**（Chromium 的 `ScriptStreamer`）：要 V8 的 `StreamedSource` 和 thread 的設計。選 J1，等量出差距再做（RFC 0012）。

## 先例

- **Chromium**：分層照上面的表。`ModuleMap` 每個 `Modulator`（每個 `ScriptState`）一份，`ModuleTreeLinker` 照 HTML 的演算法走 graph，`ModuleRecord` 包 V8 的 API；module 的抓取是 `cors` mode，MIME 嚴格檢查；`file:` 的 module 被擋。`HTMLParserScriptRunner` 管 parser 插入的、`ScriptRunner` 管 script 插入的。
- **V8 的 `d8` 和 Deno**：都用同樣的 V8 API（`CompileModule`、`InstantiateModule` 加 resolve callback、`Evaluate`、`SetHostImportModuleDynamicallyCallback`）；Deno 的 `ModuleMap` 也是 embedder 自己的表，用 `v8::Global<v8::Module>` 拿著，跟著 `JsRuntime` 一起放掉，和 C1 相同。
- **HTML 規範**：「fetch a module script graph」「fetch the descendants of and link a module script」「prepare the script element」；這份照它的步驟寫。
- **kun 自己**：wasm script（RFC 0001）已經走「抓 → 在別的 thread 上編譯 → 輪到才執行」，`Scripts` 的「已經抓好」本來就不只是「抓到了」。

## 已決定的事項

1. **A1：renderer 走訪 module graph**（2026-10-07）：`Modulator`、`ModuleMap`、`ModuleTreeLinker` 在和 engine 無關的 renderer，engine 只提供編譯、link、執行，和 Blink 的分層一樣（「抓取和 module graph」）。不選 A2「engine 自己走 graph」：抓取和導覽時丟掉回應的邏輯要分到兩邊，順序和 `load` 事件要另外通知回 renderer；也不選 A3「在 main thread 上同步抓」：renderer 不碰網路，等回應會卡住 event loop。
2. **B1：每個 realm 一份 module map**（2026-10-07）：`Modulator` 放在 `FrameView`，和 realm 一起建立、一起丟掉，和規範、Blink 一樣。不選 B2「每個 renderer 共用」：不同 frame 會共用 module 的 instance，違反規範。
3. **C1：`v8::Module` 由 realm 擁有的表用 `v8::Global` 拿著**（2026-10-07）：跟著 `Realm` 一起 drop；不新增 GC 型別，不改 `crates/dom/soundness.md` 的 GC 不變式（「module map 和 V8 的 `Module` 的生命週期」）。不選 C2「cppgc 的 GC 型別」：要付新增 GC 型別的整套成本，而 map 一直撐著每個 module，實際上一樣回收不了；也不選 C3「context 的 slot」：會形成 V8 收不掉的循環（`details.md` 的「C：V8 的 Module 放在哪裡」）。
4. **D1：編譯、link 和 evaluate 都經過接 `&mut DomIsolate` 的方法**（2026-10-07）：執行 script 的入口只有一條規則。經過哪個入口見第 16 條。不選 D2「只有 evaluate 經過入口」：多一條「這裡不會執行 JS」要靠審查守住。
5. **E1：module script 沿用 `Scripts` 的佇列**（2026-10-07）：沒有 `async` 的排進 `deferred`、有的排進 `as_soon_as_possible`，「已經抓好」對 module 是「graph 抓完、link 過或失敗」；`nomodule` 的 classic script 不再執行（「執行時機：parser、`defer` 和 `async`」）。不選 E2「module 另一個佇列」：規範要求 `defer` 的 classic、wasm 和 module script 照文件順序交錯，兩個佇列要另外合併。
6. **F1：script 元素的 `load` / `error` 在第 1 階段一起做，classic script 也有**（2026-10-07）：module 的抓取失敗只能從 `error` 看到，WPT 的 module 測試大多用它們。不選 F2「之後再做」：第 1 階段的錯誤只能寫 log，WPT 大多跑不了。
7. **G1：第 1 階段不檢查 CORS**（2026-10-07）：跨源和 `file:` 的 module 照樣載入，`crossorigin`、`integrity`、`referrerpolicy` 先忽略，記成 `docs/limitations.md` 的限制；CORS 和 `fetch`（RFC 0012 路線圖的 L）一起放在 Browser 的 network 層做一次。前提是只接受 JavaScript MIME 的回應（JSON、CSS module 不做），頁面能做的和 classic script 一樣（「MIME 和 CORS」）。不選 G2「第 1 階段就做 CORS」：要改 `protocol` 和 `net`，和 `fetch` 重複；也不選 G3「只准同源」：比 Chrome 更嚴，CDN 上的 module 全部失敗。
8. **H1：第 1 到第 3 階段只給 JS 文件，第 4 階段 JS 和 wasm 文件都有**（2026-10-07）：RFC 0012 的已決定事項「新的 API 同時給 JS 和 wasm」的例外。靜態 `import`、動態 `import()` 是 JS 的語法，`modulepreload` 是替 JS 的 module map 預先抓取，wasm 文件沒有對應的需求；script 插入的 `<script>` 和 engine 無關，兩邊都有（「wasm 文件」）。不選 H2「替 wasm 做對應物」（script 之間的 component 匯入）：新的功能，沒有使用者，留在「之後的可能」。
9. **I1：第 1 階段做 `import.meta.url`**（2026-10-07）：一個 callback，`new URL("./x", import.meta.url)` 很常見，WPT 也要。不選 I2「之後再做」：Speedometer 的 runner 用不到，但省下的很少。
10. **J1：module 在 main thread 上編譯**（2026-10-07）：沒有 code cache、背景編譯和串流編譯。不選 J2（Chromium 的 `ScriptStreamer`）：要 V8 的 `StreamedSource` 和 thread 的設計，等 RFC 0012 量出差距再做。
11. **先排第 1 階段，分成 1a、1b、1c**（2026-10-07）：在 branch `0015` 上做（「分階段」）。第 1 階段的範圍包括 F1（第 6 條）和 I1（第 9 條）：Speedometer 的 runner 用不到，WPT 要。第 2 到第 4 階段等第 1 階段做完、經過審核再排；每一段開始前只決定那一段要決定的事，寫在這裡。
12. **`ResourceRequest` 做 CORS 時才多帶 mode**（2026-10-07）：第 1 階段不改 `protocol`。不選「第 1 階段先帶、還不檢查」：沒人讀的欄位測不到。
13. **RFC 完成時跑一次 `test.py matrix` 和 `fuzz`**（2026-10-07）：C1 不新增 GC 型別，但多了執行 module 的路徑和 `bindings` 裡的 V8 callback；實作期間不跑（`CLAUDE.md`）。

1a 開始前決定（2026-10-07）：

14. **realm 的洩漏在 1a 修，`DOMException` 的 constructor 改放 context 的 embedder data**（2026-10-07）：開始前的實驗發現 `dom_js` 建的 realm 丟掉之後不會被回收（「module map 和 V8 的 `Module` 的生命週期」），C1 的測試要靠它，所以當成 1a 的第一步。embedder data 是 context 的一部分，V8 會 trace，不是 root，和 Chromium 的 `V8PerContextData` 一樣；rusty_v8 的 `Context::set_embedder_data` 是 safe 的。不選「放在 global 上的 `v8::Private` 屬性」：一樣被 trace，但每次都要先拿 private key；也不選「照 Chromium 在丟掉 document 時明確切斷（`DisposePerContextData`）」：需要新的 `ScriptEngine::dispose_realm`，有一處忘了呼叫就又漏，而 kun 的 realm 由 renderer 明確 drop，沒有 debugger 和 frame 重用要照顧。也不選「`ScriptState` 改成拿 context 的弱參照（Chromium 的 `SetPhantom`）」：撐住 context 的是 slot 裡的 root，不是 realm，改了也修不好；Chromium 用弱參照是因為很多 GC 物件拿著 `ScriptState`，kun 的 realm 只有 renderer 拿著。從此 `dom_js` 的規則：context 的 slot 不放 `v8::Global`，跟著 context 的 V8 值放 embedder data（`crates/dom_js/README.md`）。
15. **K1：module 的強參照放在 realm，callback 從 context 的 slot 找**（2026-10-07）：見「module map 和 V8 的 `Module` 的生命週期」。不選 K2「link 期間放進 isolate 的 slot」：`import.meta` 的 callback 可能在 link 很久之後才被呼叫，要另外一張登記表；也不選 K3「強參照放在 context 的 slot、丟掉 document 時明確清掉」：和第 14 條的洩漏同一種風險。強參照也不能改放 embedder data：`v8::Module` 不是 `v8::Value`，放不進去。
16. **W1：編譯、link 和 evaluate 經過現有的 `with_binding_context`**（2026-10-07）：見「module map 和 V8 的 `Module` 的生命週期」的「執行 script 的入口」。不選 W2「在 `dom` 新增三個入口」：多三個要審查的入口，好處只有和 `evaluate_classic_script` 對稱。
17. **`ScriptEngine` 的五個方法**（2026-10-07）：見「抓取和 module graph」的「engine 的介面」。和 `details.md` 原本的草案比，多了 `set_module_parse_error`（Chromium 由 `core/script` 建這個錯誤，kun 的 renderer 不能碰 V8 的值），`link_module` 多了 graph 的第一個 parse error，import 的清單帶上 import attributes（不支援的由 renderer 記成 parse error，見「抓取和 module graph」）。wasm 的 engine 不實作，預設的實作 panic，和 `compile_wasm` 一樣。

1b 開始前決定（2026-10-07）：

18. **L2：graph 照 Blink 的 `ModuleTreeLinker` 走，每個 graph 一個 linker**（2026-10-07）：linker 記著走過的網址和還在抓的數目，module map 的項目抓完時通知等它的 linker，linker 再抓這個 module 的子孫；數目歸零時找第一個 parse error（規範的「find the first parse error」，Blink 的 `FindFirstParseError` 也是最後走一遍），再請 engine link。不選 L1「每抓完一個 module 就從每個還在等的 graph 的根重走一遍」：狀態只有 module map 一份、比較簡單，但每抓完一個就重走，和 Blink 的結構也不同。
19. **script 元素的 `load` / `error` 照規範在輪到它時分派，classic、module、wasm 的 script 都有**（2026-10-07）：抓失敗的（graph 裡任何一個）在輪到它時分派 `error`、不執行，外部的執行完分派 `load`，inline 的沒有 `load`（規範的「execute the script element」，Blink 的 `PendingScript::ExecuteScriptBlock`）；`src` 是空的或解析不了時也分派 `error`（規範和 Blink 是排一個 task；kun 在 parser 這一輪停下之後分派）。是 renderer 做的，和 engine 無關，所以 wasm 文件的 guest 也聽得到。不選「抓失敗的當下就分派」：順序和 Chrome 不同，WPT 會測到。
20. **等著的 script 用 `NodeHandle`（root）拿著它的元素**（2026-10-07）：元素在分派事件之前不會被回收，script 先移除它也一樣收得到事件，和 Blink 的 `PendingScript` 的 `Member<ScriptElementBase>` 一樣；root 由 `FrameView` 拿著，在 `DomIsolate` 之前 drop。不選 `NodeId`：元素被移除、又沒人拿著時事件就丟了。
21. **`compile_module` 多一個 `base_url` 參數**（2026-10-07）：`import.meta.url` 是 module 的 base URL（外部的是回應的網址，inline 的是 document 的 base URL），錯誤訊息的 `filename` 是 `url`（inline 的是 document 的網址）；只有頁面有 `<base>` 時兩者不同。和 Blink 的 `source_url`、`base_url` 一樣分開。不選「只傳 base URL」：有 `<base>` 的頁面上，inline module 的錯誤訊息會顯示 base URL。

1c 開始前決定（2026-10-07）：

22. **WPT 加進 `module/`、`microtasks/`、`import-attributes/` 整個目錄，和頂層測第 1 階段行為的檔案**（2026-10-07）：開始前把整個 `the-script-element/` 試跑一次（445 個；`module/` 不含子目錄的 72 個裡 39 個全過），這三個目錄大部分能跑，照 `tests/wpt/README.md` 的規則整個加；頂層只加 `nomodule-*`、`script-type-*` 等測第 1 階段的檔案。`testharness-skip.txt` 照原因分組：之後的階段（`dynamic-import/`、`modulepreload`、script 插入的 `<script>`）、WPT server 沒有的 wptserve 功能（`.py` handler、`.sub.`、`?pipe=`）、其他 RFC 的 API；其餘的失敗寫進 baseline。不選「整個 `the-script-element/` 一起加」：大約一半要 skip，逾時的多，`test.py wpt` 慢很多。`json-module/`、`css-module/`、`execution-timing/`（大多用 `document.write`）不加。
23. **加上 `onerror`：元素的，和 `window.onerror`、`<body onerror>`（照規範的 `OnErrorEventHandler`）**（2026-10-07）：試跑時 43 個測試檔用 `<script onerror>`、5 個用 `window.onerror`，kun 只有 `onclick` 和 `onload`，`error` 事件分派了也看不到。`window.onerror`（`<body onerror>` 設在 window 上）收到 `ErrorEvent` 時拿到 `(message, source, lineno, colno, error)` 五個參數，回傳 `true` 時取消事件；元素的 `onerror` 是一般的 handler。和 Chromium 一樣。不選「只做元素的」：`window.onerror` 是頁面最常用來接錯誤的方法，`microtasks/` 裡第 1 階段列出的測試也用它，這份 RFC 的「錯誤怎麼回報」本來就寫到它；也不選「都不做」。

第 2 階段開始前決定（2026-10-07；第 1 階段 2026-10-07 merge 回 `main`）：

24. **P1：`import()` 經過 realm 的 `FrameClient` 交給 renderer，specifier 在那時就檢查**（2026-10-07）：`dom_js` 的 callback 建 promise，把 specifier、referrer 的 base URL、import attributes 和一個編號交給 `FrameClient`；renderer 的 `FrameRequests` 當場用第 1 階段的檢查（`resolve_request`），不合法就回傳錯誤，callback 立刻 reject，和規範、Blink 的 `DynamicModuleResolver::ResolveDynamically` 一樣在回到 script 之前 reject（WPT 的 `dynamic-import/microtasks/basic.any.js` 測這個順序）；合法的記下來，render thread 在 event loop 的最上層用 `Modulator` 抓 graph（和 `<a>` 的連結同一種「script 跑完才做」的模式），抓完請 engine evaluate、resolve。不選 P2「engine 記佇列，renderer 每次 checkpoint 之後取」：specifier 要等取出來才解析，失敗的 reject 晚到 task 結束之後；也不選 P3「在 `dom_js` 解析 specifier」：違反第 1 條。
25. **R1：referrer 的 base URL 編譯時放進 V8 的 host-defined options**（2026-10-07）：照 Blink 的 `ReferrerScriptInfo`，classic script 和 module 編譯時帶 base URL（外部的 classic script 是它的網址，inline 的是 document 的 base URL，module 是它的 base URL）；V8 呼叫 callback 時交回來。沒有的（event handler，或沒有正在執行的 script）用 document 當時的 base URL，和 Blink 的 `ExecutionContext::BaseURL()` 一樣，由 renderer 處理請求時決定。`ScriptEngine::run_classic_script` 多一個 `base_url` 參數。不選 R2「用 V8 的 `resource_name`」：inline script 和 event handler 在有 `<base>` 的頁面上會解析錯。
26. **第 2 階段分成 2a、2b、2c**（2026-10-07）：2a engine（callback、host-defined options、`ScriptEngine` 和 `FrameClient` 的新方法，只用記憶體裡的 source 和測試用的 `FrameClient` 測）、2b renderer（`FrameRequests`、`Modulator` 抓 `import()` 的 graph、event loop 的處理、`shell` 的端到端測試）、2c（WPT、`about:demo` 和情境、mutation test、文件）。WPT 的範圍照 1c 的做法，2c 先試跑 `dynamic-import/` 再決定。

第 3 階段開始前決定（2026-10-07；第 2 階段 2026-10-07 merge 回 `main`）：

27. **M1：`modulepreload` 只抓那一個 module**（2026-10-07）：抓到之後編譯、放進 module map，子孫等真的有 graph 用到時才抓，和 Chromium 的 `ModulePreloadIfNeeded`（`Modulator::FetchSingle`）一樣。原本這份 RFC 寫「傾向照 Chromium 也抓子孫」是錯的，2026-10-07 讀 Chromium 的原始碼時更正。不選 M2「連子孫一起抓」：規範允許，之後的 graph 會快一點，但抓取的時機和 Chrome 不同，量 Speedometer 時正是 RFC 0012 要避免的差異。不管選哪個，`load` / `error` 都不等子孫（規範的要求）。
28. **L1：這個階段做最小的 `HTMLLinkElement`**（2026-10-07）：WPT 的 `preload/` 的 `modulepreload*.html` 幾乎都用 `link.rel = "modulepreload"`、`link.relList.supports(…)` 建立 `<link>`，沒有它們只能靠 `shell` 的端到端測試。不選 L2「這份 RFC 不做，WPT 等 `HTMLLinkElement` 有了再加」：Speedometer 的 suite 用不到這些成員，但沒有其他 RFC 排了 `HTMLLinkElement`，`preload/` 會一直跑不了。用 Resource Timing（`performance.getEntriesByName`）數抓取次數的 5 個測試照樣跑不了（RFC 0017 不做 Resource Timing）。
29. **第 3 階段分成 3a、3b、3c**（2026-10-07）：3a `HTMLLinkElement`（DOM 線的工作，不依賴 module，可以單獨審查）、3b renderer 的 `modulepreload`（用到 3a 的 `rel`、`as`）、3c `about:demo` 和情境、mutation test、文件。原本（選 L1 之前）建議分兩段；加了 `HTMLLinkElement` 之後一段太大。engine 不用改，`Modulator` 原本就有抓單一 module 的路。
30. **`HTMLLinkElement` 只做 `href`、`rel`、`relList`、`as`、`crossOrigin`、`integrity`**（2026-10-07）：正好是 WPT 的 `modulepreload*.html` 用到的。`crossOrigin`、`integrity` 只有 reflection，沒有行為（CORS 見第 7、12 條，SRI 不做），記進 `docs/limitations.md`。不選「只做 `href`、`rel`、`relList`、`as`」：沒有 reflection 時 `link.crossOrigin = …` 只是 expando，kun 的行為一樣，但這兩個很小，Chromium 也有。`media`、`type`、`hreflang`、`sizes`、`disabled`、`referrerPolicy`、`fetchPriority`、`blocking`、`sheet` 不做。
31. **R1：`relList` 放在節點的 rare data 的 `rel_list`**（2026-10-07）：照 `classList` 的 `class_list` 做，`unsafe` 的論證一樣；一個元素只有一個 `rel`，之後 `<a>`、`<area>`、`<form>` 的 `relList` 共用這個 slot（Blink 是每個元素的 class 各有一個 `rel_list_` 成員）。不選 R2「slot 改成照 attribute 名稱存的一串（之後 `iframe.sandbox`、`link.sizes` 也放）」：現在只有一個用處，等真的需要時再一般化。
32. **`relList.supports()` 只對 `stylesheet`、`modulepreload` 回答 `true`**（2026-10-07）：規範的 supported token 是瀏覽器有實作處理的 link type，頁面用 `supports()` 做 feature detection，宣稱支援 `preload`、`prefetch` 而不抓，頁面會依賴它。不分大小寫。Chromium 回答的比這多（`preload`、`prefetch`、`preconnect`、`dns-prefetch`、`icon`、`manifest` 等），因為它都有實作；kun 實作了再加。`classList.supports()` 照舊丟 `TypeError`（`class` 沒有 supported token）。
33. **WPT 照第 22 條的做法試跑**（2026-10-07）：`preload/` 的 `modulepreload*.html`，和 `html/semantics/document-metadata/the-link-element/` 裡測 `relList` 的；用 Resource Timing、`.sub.`、`?pipe=`、SRI 的進 skip 清單。`html/dom/reflection-metadata.html` 會測所有 metadata 元素的每一個 reflection，大部分會失敗，只在試跑時看結果，不加進清單。

第 4 階段開始前決定（2026-10-07；第 3 階段 2026-10-07 merge 回 `main`）：

34. **E1：inline classic script 在插入它的呼叫結束時同步執行**（2026-10-07；原本的待決定 2）：`dom_api` 在宣告表的每個呼叫結束時（iframe 的那條路，RFC 0009 的第 25 條）處理記下的 script，inline classic 的透過 `ScriptHost` 新的方法請 engine 在當下的 context 執行，在 `appendChild` 回傳之前，和規範、Blink（`DidNotifySubtreeInsertionsToDocument` 之後 `PrepareScript`、立刻 `ExecuteScriptBlock`）一樣。renderer 這時正在執行 script、engine 被借走了，所以只能由 engine 這一側做；V8 允許巢狀執行，`dispatchEvent` 同步呼叫 listener 已經是同一種模式。wasm 文件沒有會執行的 inline script，不需要。代價是判斷要不要執行的 `prepare` 要搬到 `dom_api`，兩邊共用。不選 E2「交給 renderer，等插入它的 script 跑完再執行」（像 `import()` 的第 24 條）：比較簡單，但 `body.appendChild(s); use(window.x)` 這種常見寫法會拿到 `undefined`，和所有瀏覽器不同，WPT 也測。
35. **already started、parser inserted、force async 放在 `dom` 的元素狀態**（2026-10-07）：`script.async` 的 getter 要讀 force async、`cloneNode` 要複製 already started、parser 插入的 script 移除再插回時也不能再執行，`dom_api` 都要拿得到；和 Blink 放在元素的 `ScriptLoader` 上一樣。parser document 只記成 parser inserted 的 bool：parser 建的元素在 parser 的 document 裡 prepare，搬到別的 document 的（例如 `DOMParser` 的）照規範也不會再被觸發。不選「renderer 照 `NodeId` 記」：`dom_api` 拿不到，`async` 的 getter 和 `cloneNode` 都做不對。
36. **`HTMLScriptElement` 做 `src`、`type`、`async`、`defer`、`noModule`、`crossOrigin`、`integrity`、`text`、`charset`、`event`、`htmlFor` 和 static 的 `supports()`**（2026-10-07）：`async` 照 force async；`crossOrigin`、`integrity` 和 `<link>` 一樣只有 reflection；`supports()` 對 `classic`、`module` 回答 `true`（kun 沒有 import map 和 speculation rules）。不做 `referrerPolicy`、`fetchPriority`、`blocking`。
37. **一起做 `document.currentScript`**（2026-10-07）：webpack 的 runtime 常用 `document.currentScript.src` 推算 publicPath，Next.js 的 chunk 就是 webpack 打包的。照 Blink 的 `Document::current_script_stack_`：執行 classic script 時推入，module 的是 `null`；renderer（parser 插入的、外部的）和 engine（第 34 條的同步執行）都在 `dom` 的 document 狀態上推、出。不選「之後再做」：留著 `docs/limitations.md` 的限制。
38. **第 4 階段分成 4a、4b、4c、4d**（2026-10-07）：見「分階段」。
39. **WPT 照第 22 條的做法試跑**（2026-10-07）：`the-script-element/` 頂層第 1 階段沒加的（`async_*`、`dynamically-created-*`、`script-*` 等）、`moving-between-documents/`、skip 清單裡標著第 4 階段的那一組（`module/` 的 `execorder.html`、`errorhandling.html` 等）。這個階段做完時跑 `test.py automation`。

4b 開始前決定（2026-10-08）：

40. **B1：script 插入的 script 在 `dom_api` 的呼叫結束時同步 prepare**（2026-10-08）：和 iframe 同一個時間點（RFC 0009 的第 25 條），在 `appendChild` 回傳之前，和 Blink 的 `DidNotifySubtreeInsertionsToDocument` 之後 `PrepareScript` 一樣；結果（`src`、種類、時機）交給元素所屬 document 的 `FrameClient`，記在 `FrameRequests`，renderer 在 event loop 的最上層開始抓、排進 `Scripts`（和 `import()` 的第 24 條同一種模式）。parser 每一步之後 renderer 也呼叫同一個函式，取走 parser 插入時記下的。和 Blink 的差別只在抓取晚一點開始（script 跑完才開始）。不選 B2「renderer 在 event loop 的最上層才 prepare」：插入之後、回傳之前改了 `src` 或 `type`，或插入又移除時，結果和規範不同，4c 的同步執行（第 34 條）也一定要在呼叫結束時 prepare。
41. **`document.currentScript` 照 Blink 的 `Document::current_script_stack_`**（2026-10-08）：`dom` 替每份 document 記一個堆疊，執行 classic script 時推入元素（parser 的 inline、外部的，和 4c 的同步執行），module 和 wasm 的 script 推入「沒有」（讀到 `null`）；event handler 和 timer 不推入。wasm 的 script 照第 42 條（RFC 0001）對齊 module：規範沒有 wasm script，Chromium 也沒有對應物；`currentScript` 在 `kun:dom` 裡，wasm 的 guest 拿得到，永遠是 `null`。不選「wasm 的 script 推入它的元素」：沒有依據。
42. **4b 不再細分**（2026-10-08）：`dom_api` 在呼叫結束時處理記下的、`FrameRequests` 記下要抓的、renderer 在最上層抓、`Scripts` 的兩個佇列和 `load` / `error`、`currentScript`、`shell` 的端到端測試。4b 期間 inline classic script 照樣 prepare（already started），還不執行，4c 接上。

4c 開始前決定（2026-10-08）：

43. **R1：script 插入的 inline classic script 在元素所屬 document 的 realm 執行**（2026-10-08）：和規範、Chromium（執行用元素的 document 的 frame 和 `ScriptState`）一樣：插進 iframe 的 document 的在 iframe 的 realm 執行，和 4b 的外部 script 交給元素所屬 document 的 frame 一致。`ScriptHost` 新的方法接 document，`dom_js` 從 document 的 window 的 JS 物件（那個 realm 的 global）拿到 context、進入它執行；經過 `BindingContext` 的 scope，和 `V8Host::call` 一樣，不新增執行 script 的入口（`soundness.md` 的 I13）。例外在那個 realm 的 window 上回報、不丟回插入它的 script；外層的 script 還在執行，不做 microtask checkpoint。不選 R2「只在執行中的 realm 同步執行，別的交給 renderer 之後執行」：跨 realm 的時機和規範不同，和 4b 不一致。
44. **U1：HTML 的 document base URL 搬到 `dom_api`**（2026-10-08）：inline script 的 base URL（裡面的 `import()` 照它解析）在 `dom_api` 處理時當下算（第一個 `<base href>`，沒有的話用 fallback base URL），renderer 也改用同一個函式；`about:blank`、`about:srcdoc` 子 frame 的 fallback（父 frame 的 base URL）由 frame 提供（`FrameClient` 新的方法）。和 Chromium 把 base URL 放在 `Document` 一樣是 DOM 這一層的事。`baseURI` 和 URL 的 reflection 也該用它，但會牽動 WPT 的 baseline，另外做，這次不改。不選 U2「frame 記一份、renderer 每次算時更新」：同一個 script 裡先插 `<base>` 再插 inline script 時拿到舊的。

4c 期間決定（2026-10-08）：

45. **O1：iframe 和 script 的紀錄放進同一個佇列，照樹的順序處理**（2026-10-08）：WPT 的 `Node-appendChild-script-and-iframe.html` 找到：同一次插入裡 script 排在 iframe 前面時，規範（DOM 的 post-connection steps）和 Chromium（每個節點依序 `DidNotifySubtreeInsertionsToDocument`）先執行 script，那時 iframe 還沒有 `contentWindow`；kun 原本先建全部的子 frame 再執行 script。`dom` 記下的 iframe 和 script 的紀錄改成同一個佇列（`ElementChange`），`dom_api::update_elements` 照順序處理，取代 `update_frames` 和 `update_scripts`；RFC 0009 第 25 條的時間點（呼叫結束時、parser 每一步之後）不變。不選 O2「先記成限制」：只有排在 iframe 前面的 script 讀那個 iframe 時看得出來，但修正只動到記錄和處理的地方。
46. **isolate 拆掉時的 use-after-free 在 rusty_v8 的 fork 修，改 `Weak` 的 `Drop`**（2026-10-08）：RFC 完成時的 `test.py fuzz` 找到（20 組裡 3 組，都是 `bindings` 的 `modules` 在 `--stress-compaction` 底下 crash）。上游的 `OwnedIsolate::drop` 先 dispose isolate 的 handle、再跑 guaranteed finalizer（釋放 context 的 annex 和裡面的 `Weak`），最後才是 V8 的拆除；`Weak` 看到 handle 已 dispose 就釋放 `WeakData`，不 reset V8 的 handle，V8 拆除時的 GC 再用它呼叫 callback。選 A：指標還在時 leak `WeakData`（和 second pass 還沒跑時一樣），只改 Rust，不用重編 V8、不用發新的 release；代價是 isolate 拆掉時還登記著的 weak handle 各留一個小的 `WeakData`。不選 B「改 `OwnedIsolate::drop` 的順序，先跑 finalizer 再 dispose handle」：不 leak，但改變上游刻意安排的拆除順序（它的註解要 handle 先看到已 dispose），也只蓋得住 finalizer 這一條路。不選 C「kun 在 realm 結束時 `clear_all_slots()`」：放 slot 的地方（`ModuleRecords`、`dom_js` 的 `Frame` 和 `ErrorReportingMode`）每一處都要記得，renderer 拆 isolate 時也有同樣潛在的問題。Chromium 不經過 rusty_v8，沒有對應。不寫上游的 issue 草稿，記在 `crates/dom/soundness.md` 的「可以回報上游的修改」和 fork 的 `README.machopo.md`。

引用的其他 RFC 的已決定事項：
- RFC 0012 的已決定事項：讓 suite 能跑的功能歸各自的 RFC（它的第 2 條）；新的 API 要同時給 JS 和 wasm，例外在各自的 RFC 決定（這份的例外見「wasm 文件」）。
- RFC 0009 的已決定事項第 4、5、26 條：每份 document 一個 realm。
- RFC 0001 的已決定事項第 42 條：wasm script 照 module script 的時機。

## 待決定

沒有。原本的待決定 2（第 4 階段 inline script 的時機）由第 34 條決定；待決定 1（Speedometer 有沒有用 import map）要等在 kun 上跑 M4、M5 的 suite，移到「之後的可能」。

## 之後的可能

- **確認 Speedometer 的 suite 沒有用 import map**（原本的待決定 1）：盤點記下的 `<script>` 種類沒有 `importmap`（`rfcs/0012-speedometer/details.md` 的「對照表」），RFC 0012 在 kun 上跑 M4、M5 的 suite 時再確認一次；有的話 import map 要提前。
- import map 和 bare specifier；`import.meta.resolve`。
- JSON module、CSS module（`with { type: … }`），在 CORS 之後。
- module worker（`new Worker(url, { type: "module" })`），等有 worker 的 RFC。
- V8 的 code cache、背景編譯和串流編譯（J2）。
- wasm 的 ESM integration、source phase import：JS 的 module import `.wasm`；wasm 文件裡 script 之間的 component 匯入（H2）。
- `HTMLScriptElement` 的 `referrerPolicy`、`fetchPriority`、`blocking`；`HTMLLinkElement` 的其他成員；`<a>`、`<area>`、`<form>` 的 `relList`。
- `baseURI` 和 URL 的 reflection（`src`、`href`）用 `dom_api::document::base_url`（看 `<base>`），會牽動 WPT 的 baseline（第 44 條）。
- 抓失敗的 module 不留在 module map（WPT 的 `concurrent-failed-imports.html` 這樣期待，還沒查證規範）；SVG 的 `<script>`。
