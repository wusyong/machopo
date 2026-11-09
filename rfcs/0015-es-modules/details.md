# RFC 0015 的細節：ES module 和 script 的載入

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和看法以那邊為準，這裡不重複。

## M1 要的

2026-10-07 讀 Speedometer 的 `release/3.1`（RFC 0012 釘住的 `1386415` 是這個分支最後的 commit）：
- `index.html` 只有一個 script：`<script src="resources/main.mjs" type="module"></script>`，外部的、沒有 `async`。另外兩個 `<link>` 是 stylesheet 和 icon。
- `main.mjs` 靜態 import 六個相對網址的 module（`./benchmark-runner.mjs`、`./statistics.mjs`、`./tests.mjs`、`./metric-ui.mjs`、`./params.mjs`、`./developer-mode.mjs`），`benchmark-runner.mjs` 再 import `./metric.mjs`、`./params.mjs`（和 `main.mjs` 共用一個）。
- 這兩個檔案都沒有動態 `import()`、`import.meta`、top-level await。其他 runner 的檔案還沒逐一讀過；第 1 階段做完之後在 kun 上跑，缺的會直接以錯誤出現（RFC 0012 的 1b）。

所以 M1 只要：外部的 module script、相對網址的靜態 import、多個 module 共用一個、parse 完執行、`DOMContentLoaded` 等它。`import.meta.url` 和 script 元素的 `load` / `error`（`README.md` 的 I1、F1）是 WPT 和一般網站要的。

suite 這邊（`rfcs/0012-speedometer/details.md` 的「對照表」）：用 module script 的是 TodoMVC-WebComponents、Lit-Complex-DOM、Angular-Complex-DOM、NewsSite-Nuxt、Editor-CodeMirror、Editor-TipTap、Charts-chartjs、Charts-observable-plot；其中 5 個另外用動態 `import()` 或 `modulepreload`（M4）；NewsSite-Next 用 script 建立 `<script>`（M5）。`<script type="application/json">`、`text/template` 的 `<script>` 只讀 `textContent`，不執行，現在就是對的。

## Chromium 的流程

依 Chromium 的 `third_party/blink/renderer/core/script/` 和 `bindings/core/v8/`（2026-10-07 依記憶和公開的原始碼整理，類別名稱以 Chromium 的 main 為準，細節實作時再對照）。

**parser 插入的 `<script type="module" src>`**：
1. `HTMLParserScriptRunner` 在 `</script>` 呼叫 `ScriptLoader::PrepareScript`：判斷種類是 module，建 `ModulePendingScript`，交給 `Modulator::FetchTree`。
2. `ModuleTreeLinker`（由 `ModuleTreeLinkerRegistry` 撐住）照 HTML 的「fetch a module script graph」：`FetchRoot` → `ModuleMap::FetchSingleModuleScript`。
3. `ModuleMap` 以（網址、module 的種類）為鍵：已經有的直接回傳或等它；沒有的建 `ModuleMap::Entry`，交給 `ModuleScriptLoader`，它用 `DocumentModuleScriptFetcher` 經過 `ResourceFetcher` 抓（`cors` mode、destination `script`），檢查 MIME，`JSModuleScript::Create` 編譯（`ModuleRecord::Compile`，失敗時記成 parse error）。
4. linker 對每個 module 的 `ModuleRecord::ModuleRequests` 解析 specifier（`ModuleScript::ResolveModuleSpecifier`，有 import map 時經過 `ImportMap`），抓還沒抓的子孫（`FetchDescendants`），記下已經走過的網址（處理循環）。
5. 全部抓完、沒有錯時 `Instantiate`：`ModuleRecord::Instantiate` 呼叫 V8 的 `InstantiateModule`，resolve callback 經過 `ModuleRecordResolverImpl`：從 `v8::Module` 找回 `ModuleScript`（它記著每個 `v8::Module` 對到哪個 `ModuleScript`），解析 specifier，從 module map 拿子孫的 record。
6. `ModulePendingScript` 收到 graph 完成的通知，變成「ready」；`HTMLParserScriptRunner` 的「parse 完執行」的佇列輪到它時，`ModuleScript::RunScript` → `ModuleRecord::Evaluate`，回傳的 promise 被 reject 時 `ModuleRecord::ReportException`。

**動態 `import()`**：V8 呼叫 `V8Initializer` 設定的 `HostImportModuleDynamically`，從 host-defined options 讀回 `ReferrerScriptInfo`（base URL、credentials、nonce、parser state），交給 `Modulator::ResolveDynamically`；`DynamicModuleResolver` 走 linker，完成時 evaluate、用 namespace resolve promise，錯誤時 reject。

**`import.meta`**：V8 呼叫 `HostGetImportMetaProperties`，`Modulator::HostGetImportMetaProperties` 給 `url` 和 `resolve`。

**`modulepreload`**：`PreloadHelper::ModulePreloadIfNeeded` 呼叫 `Modulator::FetchSingle`，結果進同一個 module map；`<link>` 上分派 `load` / `error`。

**script 插入的 `<script>`**：`ScriptLoader` 在元素變成 connected（`DidNotifySubtreeInsertionsToDocument`）、子節點改變、`src` 設定時 prepare；async 的和 in-order 的交給 `ScriptRunner`。

**kun 的對應和差別**：
- `core/script` 在 kun 是 `renderer`（`script.rs` 已經是 `PendingScript` 和佇列）加上 `dom_api`（`ScriptEngine` 的介面）；`bindings/core/v8` 在 kun 是 `bindings` 和 `dom_js`。
- Blink 的 `ModuleMap`、`ModuleScript` 是 Oilpan 上的物件，`v8::Module` 用 `TraceWrapperV8Reference` 拿著；kun 不放上 cppgc（`README.md` 的已決定事項第 3 條，「C：V8 的 Module 放在哪裡」）。
- Blink 的 `ModuleScript` 同時拿著 record 和 URL；kun 分成兩半：renderer 的 `ModuleScript` 只拿編號，V8 的值在 engine 的表（renderer 不能出現 V8 的型別，`python3 scripts/sanity.py deps`）。

## rusty_v8 的 API

2026-10-07 讀 `../third_party/rusty_v8`（`kun` 分支的 `3c34cd5`，起點是上游的 `v152.2.0`），都有，不用改 fork：

| 要的 | fork 裡 | 備註 |
| --- | --- | --- |
| 編譯 module | `v8::script_compiler::compile_module`，`ScriptOrigin::new` 的 `is_module` | origin 的 `host_defined_options` 放 referrer 的資訊（第 2 階段） |
| import 的清單 | `Module::get_module_requests` → `ModuleRequest::get_specifier`、`get_import_attributes`、`get_phase` | |
| link | `Module::instantiate_module(scope, callback)` | callback 是 `ResolveModuleCallback`（`extern "C"`，不能帶 closure）；要開 scope 就要 `v8::callback_scope!`，是 `unsafe` |
| 執行 | `Module::evaluate`，`get_status`、`get_exception`、`is_graph_async`、`get_stalled_top_level_await_message` | evaluate 回傳 promise |
| namespace | `Module::get_module_namespace` | 第 2 階段 resolve `import()` 的 promise |
| 找回是哪個 module | `Module::get_identity_hash`、`script_id` | hash 不保證唯一，要再比 `Global` |
| 動態 import | `Isolate::set_host_import_module_dynamically_callback`（`HostImportModuleDynamicallyCallback`，`UnitType` 的 closure，fork 替它開好 scope） | 可以是 safe 的 Rust，放在 `dom_js` 或 `bindings` |
| `import.meta` | `Isolate::set_host_initialize_import_meta_object_callback`（`unsafe extern "C" fn`） | 放 `bindings` |
| 合成的 module | `Module::create_synthetic_module` | JSON、CSS module（之後） |
| 其他 | `HostImportModuleWithPhaseDynamicallyCallback`、`ResolveSourceCallback`（source phase import）、`UnboundModuleScript`（code cache） | 不在範圍內 |

## A：誰走訪 module graph

| | A1：renderer | A2：engine |
| --- | --- | --- |
| 抓取 | renderer 照舊送 `FetchResource`，回應照舊進 `did_receive_resource` | engine 經過一個 trait 請 renderer 抓，回應再轉回 engine |
| document 被丟掉時 | 現在的做法：`requesting_frame` 找不到就丟掉回應 | engine 也要知道哪些請求作廢了 |
| `ScriptEngine` 的介面 | 多四、五個方法（下一節） | 多「抓 graph」「執行」和 fetch 的 trait |
| wasm 的 engine | 不實作這幾個方法（`runs_js` 是 false，不會被呼叫） | 同樣不實作 |
| 和 Chromium | `core/script` 的 linker 在 V8 的那一層之上 | 不同 |
| 順序和 `load` | 和 classic、wasm script 共用 `Scripts` | 要把「graph 好了」通知回 renderer |

A2 的好處是 V8 的值和 map 在同一邊、resolve callback 直接查 map；A1 讓 engine 的表多一份「每個 import 解析成哪個編號」，由 renderer 在 link 前交給它。

## ScriptEngine 的介面

1a 定下的方法（`crates/dom_api/src/engine.rs`；`README.md` 的已決定事項第 17 條）：

```rust
pub struct ModuleScriptId(u32);               // 一個 realm 裡的編號，renderer 只拿編號
pub struct CompiledModule { pub id: ModuleScriptId, pub requests: Option<Vec<ModuleRequest>> }
pub struct ModuleRequest { pub specifier: String, pub attributes: Vec<(String, String)> }
pub enum ModuleParseError { Syntax(String), Type(String) }

fn compile_module(&mut self, dom, realm, source: &str, url: &str, line: u32) -> CompiledModule;
    // requests 是 None：語法錯誤，錯誤留在 engine 的表裡
fn set_module_parse_error(&mut self, dom, realm, module, error: ModuleParseError);
    // specifier 解析不了（TypeError）、type 以外的 import attribute（SyntaxError）
fn set_module_requests(&mut self, realm, module, resolved: Vec<ModuleScriptId>);
    // 照 requests 的順序，每個 import 解析成哪個 module；每個 module 一次
fn link_module(&mut self, dom, realm, root, parse_error: Option<ModuleScriptId>);
    // 有 parse_error 時不 link，它的 parse error 記成根的 error to rethrow；link 失敗的例外也是
fn run_module_script(&mut self, dom, realm, root);
    // 錯誤都在 evaluate 的 promise 被 reject 時回報
```

- specifier 的解析（相對網址、bare specifier 失敗）是 HTML 的演算法，和 engine 無關，放在 renderer；「第一個 parse error」（規範的「find the first parse error」）也由 renderer 走 graph 時找。
- V8 的 import 清單每個（specifier、attributes）只出現一次，重複的 `import` 合併（ECMAScript 的 [[RequestedModules]] 也是），所以 `requests` 也是。resolve callback 照（specifier、attributes）找到它是第幾個 request，再找到解析成的 module。
- `link_module`、`run_module_script` 不回傳結果：link 的錯誤是 error to rethrow，執行的錯誤由 promise 的 handler 回報，renderer 都不需要知道（`<script>` 照樣分派 `load`，見「錯誤的分類」）。

**和 Chromium 對照**（2026-10-07 讀 Chromium main 的 `module_record.cc`、`module_record_resolver_impl.cc`、`v8_initializer.cc`、`v8_script_runner.cc`、`module_script.h`）：

| | Chromium | kun |
| --- | --- | --- |
| compile | `ModuleRecord::Compile` 回傳 `v8::Module`；`ModuleRecord::ModuleRequests` 給 specifier、位置、import attributes、phase | `compile_module` 一次給編號和清單 |
| specifier 的解析 | `ModuleScript::ResolveModuleSpecifier`（`core/script`），解析不了時 `core/script` 建 `TypeError` 當 parse error | renderer 解析，請 engine 建錯誤（`set_module_parse_error`）：renderer 碰不到 V8 的值 |
| resolve callback 怎麼找到表 | 從 context：`ScriptState::From(isolate, context)`（context 的 embedder data 裡的指標）→ `Modulator::From` → `GetModuleRecordResolver()->Resolve()`，在 link 的當下解析網址、從 module map 拿 record | 從 context 的 slot 的 `Linkage`：renderer 在 link 前交給 engine 的解析結果 |
| 從 `v8::Module` 找回 module | `record_to_module_script_map_`，鍵是包住 `v8::Module` 的 `BoxedV8Module`；不隨 GC 清掉，`ContextDestroyed()` 明確清空。註解說應該讓鍵是 `v8::Module` 的弱參照 | 鍵是 `v8::Weak<v8::Module>`，用 identity hash 找，跟著 context 的 slot 釋放 |
| record、parse error、error to rethrow | `ModuleScript` 的 `TraceWrapperV8Reference<v8::Data> record_`、`WorldSafeV8Reference<v8::Value>` 的 `parse_error_`、`error_to_rethrow_`，在 Oilpan 上被 trace | `bindings::ModuleRecords` 的 `v8::Global`，由 realm（`JsRealm`）拿著 |
| `import.meta` | `HostGetImportMetaProperties`：`Modulator::From(script_state)` → `HostGetImportMetaProperties(module)`，給 `url` 和 `resolve` | `initialize_import_meta`：從 context 的 slot 找網址，只給 `url` |
| link | `ModuleRecord::Instantiate`，開 `V8DoNotRunMicrotasksScope`，回傳例外 | `ModuleRecords::link`，例外記成 error to rethrow；microtask 照 explicit policy 本來就不跑 |
| evaluate | `V8ScriptRunner::EvaluateModule`：進入 context、`CanExecuteScripts` 檢查、`V8RunMicrotasksScope`、`TryCatch`，在回傳的 promise 掛 `ModuleEvaluationRejectionCallback` 回報 | 經過 `with_binding_context`（入口），在 promise 掛 reject 的 handler 回報；microtask 由 renderer 緊接著的 checkpoint 跑 |
| 錯誤的位置 | `V8ScriptRunner::ReportException` 用 `Exception::CreateMessage` | 一樣（`Caught::from_value`），所以是建立錯誤的位置，不是 `throw` 的 |

## C：V8 的 Module 放在哪裡

| | C1：realm 擁有的表 | C2：cppgc 的 GC 型別 | C3：context 的 slot |
| --- | --- | --- | --- |
| 拿著 module 的 | `v8::Global<v8::Module>`（root） | `TracedSlot`（`JsEdge` 的新種類），被 trace | `v8::Global`，放在 context 的 slot |
| 什麼時候放掉 | `Realm` 被 drop 時 | 沒人撐住、GC 時 | context 被回收時：但它不會被回收 |
| 回收不到的情況 | realm 活著時 module 一直在（規範的 module map 也不移除） | map 撐住每個 module，一樣回收不了 | `Global` → module → module 的 context → slot → `Global` 的循環，V8 看不到 slot 裡的 root，整個 context 漏掉 |
| 成本 | `dom_js` 的 realm 型別多一個表 | `crates/dom/README.md` 的「怎麼修改」整套：`script_objects!`、`Object`、測試、`gc_stress`、soundness 的 I14 等、`test.py matrix` 和 `fuzz` | 最少，但不能用 |
| 和 Blink | 不同（Blink 的 map 在 Oilpan 上） | 相同 | — |

C1 的前提：`Realm` 一定比 `DomIsolate` 先 drop（`dom_api` 的 `Realm` 已經這樣要求），而 renderer 在導覽、移除 iframe 時確實 drop 舊的 realm（`FrameView` 拿著它），而且 drop 之後沒有別的 root 撐住 context。最後這點原本不成立：`DOMException` 的 constructor 在 context 的 slot 裡，正是 C3 的循環，每個 realm 都收不掉；1a 先修掉（`README.md` 的已決定事項第 14 條）。測的是「document 丟掉之後，只被 module 的環境撐住的 DOM 節點會被回收」（`dom_js` 的 `modules` 的 `a_dropped_realms_modules_are_collected`）。

C3 的變形「強參照放 context 的 embedder data」（V8 會 trace，和修洩漏的做法一樣）不行：embedder data 只收 `v8::Value`，`v8::Module` 不是。所以 slot 裡只放 `v8::Weak<v8::Module>`，強參照放在 realm（K1，已決定事項第 15 條）。

C1 和 C2 都讓 module 活得和 realm 一樣久；只有 realm 本身被 GC 決定存活時（例如 detached 的 iframe 的 window 還被別的 frame 的 JS 拿著）兩者才有差別：kun 的 realm 是 renderer 明確丟掉的（RFC 0009 的已決定事項第 15 條的 `discard_document`），不靠 GC，所以沒有差別。

## E：module script 在 `Scripts` 裡

`PendingScript.loaded` 是 `Option<Option<Source>>`：`None` 是還在抓（wasm 是還在編，module 是 graph 還在抓），`Some(None)` 是失敗，`Some(Some(source))` 是可以執行。module 是 `Source::Module(ModuleScriptId)`（graph 的根），在 linker 完成時才變成 `Some`：
- graph 有抓取失敗：`Some(None)`，輪到它時在元素上分派 `error`（F1、第 19 條）。
- graph 有 parse 錯誤、link 失敗：`Some(Some(Source::Module(root)))`，執行時由 engine 回報（規範的 error to rethrow），元素上分派 `load`。
- 1b 起 `PendingScript` 的鍵是 `Scripts` 自己的編號（原本是 request id）：inline 的 module 沒有 request，wasm 的編譯結果和 classic script 的回應從 `PendingLoad::Script` 記著的編號找回它。`PendingScript` 也拿著元素（`NodeHandle`），`next_ready` 不再跳過抓失敗的，交給 render thread 分派 `error`。

`async` 的 module 進 `as_soon_as_possible`，其他進 `deferred`；`Timing::Blocking` 不會是 module。`src` 是空的或解析不了的不進佇列：元素記在 `FrameView` 的 `script_errors`，parser 這一輪停下之後分派 `error`。

## 錯誤的分類

| 發生什麼 | 規範 | `<script>` 的事件 | window 的 `error` | `unhandledrejection` |
| --- | --- | --- | --- | --- |
| 根或子孫抓取失敗、非 2xx、MIME 不對 | fetch 的結果是 null | `error` | 沒有 | 沒有 |
| 任何一個 module 有語法錯誤 | parse error，執行時 rethrow | `load` | 有（`SyntaxError`） | 沒有 |
| specifier 解析不了（bare specifier、不合法的網址） | 記成 parse error | `load` | 有（`TypeError`） | 沒有 |
| `type` 以外的 import attribute | 記成 parse error | `load` | 有（`SyntaxError`） | 沒有 |
| 不支援的 `type`（`json`、`css`） | 載入失敗，error to rethrow（kun 記成 parse error） | `load` | 有（`TypeError`） | 沒有 |
| link 失敗（找不到 export、循環裡的模糊 export） | error to rethrow | `load` | 有（`SyntaxError`） | 沒有 |
| 執行時丟出 | report the exception | `load` | 有 | 沒有 |
| top-level await 的 promise 被 reject | report the exception | `load` | 有 | 沒有（promise 標成已處理） |
| `import()` 的 graph 抓取失敗（第 2 階段） | promise reject，`TypeError` | — | 沒有 | 沒人處理時有 |
| `import()` 的 graph 有 parse 錯誤、link、執行錯誤 | promise reject，那個錯誤 | — | 沒有 | 沒人處理時有 |

同一個 graph 有好幾個錯誤時回報哪一個，照規範的「find the first parse error」（WPT 的 `choice-of-error-*`）。

## G：CORS

| | G1：不檢查 | G2：第 1 階段就做 CORS | G3：只准同源 |
| --- | --- | --- | --- |
| 改的地方 | 無 | `protocol` 的 `ResourceRequest` 帶 mode 和 origin；`net` 送 `Origin`、檢查 `Access-Control-Allow-Origin`、redirect 的規則 | renderer 比對網址的 origin |
| 跨源的 module | 載入 | 有 header 才載入（和 Chrome 一樣） | 一律失敗 |
| `file:` 的頁面 | 載入（Chrome 擋） | 擋（opaque origin），kun 的本機測試頁面不能用 module | 擋 |
| 和 `fetch`（路線圖的 L） | L 做 CORS 時 module 改走它 | 重複做一次，或 L 要等這份 | L 做 CORS 時改成 G2 |
| 暴露的東西 | 跨源 module 的錯誤訊息不 mute；跨源的 JS 本來就能用 classic script 執行 | 無 | 無 |

G1 的前提：只接受 JavaScript MIME 的回應（JSON、CSS module 不做），所以頁面拿到的只是「執行一段跨源的 JS」，和 classic script 相同。kun 的 classic script 也還沒 mute 跨源的錯誤，這是同一個限制。

## 第 2 階段的細節

- **referrer**：規範的 `import()` 用「active script」的 base URL（module 的是它的網址，classic script 的是它的 base URL，event handler 的是 document 的 base URL）。V8 把編譯時給的 `host_defined_options` 交給 callback；kun 現在編譯 classic script 時沒有給（`bindings` 的 `ScriptState::evaluate_classic_script_from` 的 `ScriptOrigin::new(..., None)`），event handler 的編譯也要帶。Blink 存的是 `ReferrerScriptInfo`（base URL、fetch 的選項）；kun 先只存 base URL。
- **沒有 active script**（例如 promise 的 reaction 直接是 `import`，WPT 的 `no-active-script-*`）：用 realm 的 document 的 base URL。
- **realm 被丟掉**：等待中的 promise 跟著 engine 的表放掉，回應被 renderer 丟掉（document 已經不在）；WPT 的 `inactive-context-import.html`。
- **promise 的 resolve**：在一個 task 裡 evaluate（經過入口），然後用 namespace resolve；evaluate 回傳的 promise 被 reject 時用它的 reason reject。之後照常 microtask checkpoint。

## WPT

2026-10-07 看 `../third_party/wpt`（`0f195499`）：
- `../third_party/wpt/html/semantics/scripting-1/the-script-element/module/`：module script 的主體，順序（`execorder.html`）、循環、錯誤的種類（`error-type-*`、`instantiation-error-*`、`evaluation-error-*`、`choice-of-error-*`、`compilation-error-*`）、`load` / `error`（`load-error-events*.html`）、MIME（`errorhandling.html`）、`nomodule-attribute.html`、`single-evaluation-*`。子目錄 `dynamic-import/`（第 2 階段）、`import-meta/`、`top-level-await/`。
- 不能在 kun 的 WPT server 上跑的：檔名有 `.sub` 的（`crossorigin-*`、`referrer-*`、`credentials.sub.html`）要 wptserve 的替換；`module-fetch-tester.py`、`serve-with-content-type.py` 這類 `.py` handler；`.headers` 檔。它們進 `4ff8d6e:tests/wpt/testharness-skip.txt`，理由寫 server 的限制（CORS 的那幾份在 G1 下本來就會失敗）。
- `../third_party/wpt/html/semantics/scripting-1/the-script-element/microtasks/`：`checkpoint-after-window-onerror-module.html`、`evaluation-order-*` 的 module 版本。
- `../third_party/wpt/html/semantics/scripting-1/the-script-element/execution-timing/`：大多用 `document.write`，第 4 階段時再挑。
- `../third_party/wpt/html/semantics/scripting-1/the-script-element/json-module/`、`css-module/`、`import-attributes/`：不在範圍內，不加。
- `../third_party/wpt/preload/`：`modulepreload*.html`（第 3 階段）。
- `../third_party/wpt/import-maps/`：不在範圍內。

`.any.js` 的測試（例如 `import-meta-url.any.js`）跑 window 的版本（`tests/wpt/README.md`）。

**1c 的試跑**（2026-10-07，`README.md` 的已決定事項第 22 條）：先把整個 `the-script-element/` 暫時加進清單跑一次（445 個）。`module/` 不含子目錄的 72 個裡 39 個全過；沒過的原因：
- **WPT server 沒有的**：約 23 個 import 了 `.py`（module 抓失敗，graph 跟著失敗：`nested-imports`、`slow-cycle` 也是），或用 `.sub.`、`?pipe=trickle(...)`（`inline-async-execorder` 靠它讓一個 module 晚到）、別的 host、HTTPS；`charset-*` 也靠 `.py` 給 `Content-Type`。
- **之後的階段**：script 插入的 `<script>`（`execorder`、`errorhandling`、`instantiation-error-8` 等）、`modulepreload`、`import()`（`microtasks/evaluation-order-3`、`-4` 也用它）。
- **其他 RFC 的 API**：`document.currentScript`、`location`、`URL`、`crypto`、`customElements`、`Worker`、`postMessage`（`microtasks/checkpoint-after-window-onerror*` 在錯誤之後送一個 message，沒有它就多報一次錯、再等到逾時；microtask 的順序本身是對的）。
- **kun 這邊缺的**：`onerror`。43 個檔案用 `<script onerror>`、5 個用 `window.onerror`，`error` 事件分派了也看不到；加上 `onerror`（第 23 條）之後 `fetch-error-*`、`error-type-1` 等都過了。

沒有找到 kun 的 module 本身的 bug。加進清單、skip 和 baseline 之後的數字見 `progress.md`。

**2c 的試跑**（2026-10-07，`dynamic-import/`，38 個）：先只拿掉 skip 清單裡的這個目錄跑一次，7 個全過。沒過的：
- **kun 的 bug**：`dynamic-imports-script-error.html`，同一個 graph 的兩個 `import()` 一起等、link 失敗時拿到同一個錯誤物件。改成 engine 在 `finish_dynamic_import` 裡每個 `import()` 自己 link（`README.md` 的「動態 `import()`」）之後全過。
- **和規範的差異**：`microtasks/basic.any.js` 的「已經在 module map 裡的 module」：kun 要等 task 結束才完成，先排的 1000 個 microtask 全部先跑（`docs/limitations.md`）。`code-cache-base-url.html`：不同 script 裡原始碼相同的 `new Function` 都照第一個 script 的 base URL 解析，原因還沒查清（`docs/limitations.md`）。
- **WPT server 沒有的**：`.sub.`、`?pipe=trickle(...)`（`delay-load-event.html` 靠它讓 module 晚到；kun 的 `load` 本身沒被延後）、HTTPS。
- **其他 API**：worker、`Blob`、`fetch`（`concurrent-failed-imports.any.js`、`repeated-imports.any.js`）、`onmousedown`（`inline-event-handler.html`）、字串形式的 `setTimeout`（7 個照失敗寫 baseline，2 個要等到逾時的進 skip）、V8 的 code cache（`v8-code-cache.html`）。
- **tentative 的 `import defer`**：3 個，V8 沒開這個語法。
- **沒有瀏覽器通過的**：`string-compilation-of-promise-result.html`（規範要用 promise job 建立時的 script 的 base URL，crbug.com/1245063），kun 和瀏覽器一樣用 document 的；它的失敗訊息裡有測試 server 的 port，每次都不同，所以進 skip，`concurrent-failed-imports.any.js` 也是。

**3a 的試跑**（2026-10-07，`HTMLLinkElement`）：把 `html/semantics/document-metadata/the-link-element/`（25 個）、`html/semantics/rellist-feature-detection.html`、`html/dom/reflection-metadata.html` 和 `preload/`（69 個，3b 的範圍）暫時加進清單跑一次：
- **通過、加進清單**：`link-rellist.html`（`relList` 的 `contains`、`supports`），和 `preload/reflected-as-value.html`（`as` 的 reflection）。後者要 `json`、`text` 也是已知的值，WPT 的 reflection 資料（`html/dom/elements-metadata.js`）只列 16 個；照規範用 Fetch 的 potential destination（24 個）之後兩邊都對。
- **`the-link-element/` 的其他檔案**：測 stylesheet 的 `load` / `error`、`getComputedStyle`、shadow DOM，和 `fetchpriority` 等沒做的成員，和這一步無關。
- **`rellist-feature-detection.html`**：要 `supports()` 回答 Chromium 支援的 `preload`、`prefetch` 等（第 32 條刻意不同），還要 `<a>`、`<area>`、`<form>` 的 `relList`。
- **`reflection-metadata.html`**：`link` 的 `as`、`crossOrigin`、`rel`、`relList`、`integrity` 沒有失敗；`href` 的失敗是測試用 `<a>` 的 `protocol` 等成員算預期的 URL（kun 沒有，預期值變成 `undefined//...`），其餘是沒做的成員和其他元素。失敗訊息裡有測試 server 的 port，不能寫 baseline，所以不加（第 33 條）。
- 本來就在清單裡的 `dom/lists/DOMTokenList-coverage-for-attributes.html` 少了 `link.relList` 的失敗、`dom/nodes/Node-cloneNode.html` 少了 `createElement(link)` 的失敗，baseline 跟著更新。

**3b 的試跑**（2026-10-07，`preload/` 的 14 個 `modulepreload*.html`，和 skip 清單裡等第 3 階段的 `module/modulepreload-*referr*.html` 3 個）：
- **通過**：`modulepreload-already-in-module-map.html`（map 裡已有的、還在抓的、語法錯誤、抓失敗的，各分派對的事件）、`avoid-delaying-onload-link-modulepreload-exec.html`。
- **照失敗寫 baseline**（都跑完、訊息穩定）：沒有 `URL` 的（`avoid-delaying-onload-link-modulepreload.html`、`link-header-modulepreload.html`、`modulepreload-consumed-by-script.html`、`modulepreload-fires-before-dependencies.html`，和 `modulepreload-multiple.html` 的 script 那一段；它們之後也要 wptserve 的 pipe、`.py`、`Link` header 或第 4 階段）；JSON、CSS、text module（`modulepreload-json.html`、`-style.html`、`-text.html`、`-multiple.html` 的另外兩段：kun 分派 `error`）；SRI（`modulepreload-sri.html`、`-sri-importmap.html`：`integrity` 不檢查，預期失敗的 preload 成功了）。
- **進 skip**：`modulepreload.html`（第一行就讀 `location.protocol`）、`modulepreload-as.html`（用 `dataset`），都在有結果之前出錯。`module/` 的 3 個 referrer 的測試要 `.py` handler，失敗訊息裡有 port，移到 skip 清單的 wptserve 那一組。
- 沒有找到 kun 的 bug。`modulepreload-as.html` 讓 `as` 的處理改正（`README.md` 的「`modulepreload` 和 `HTMLLinkElement`」）：WPT 認為不合法的值（`invalid-dest`）是 `script`，`fetch`、`image` 等才是 `error`。

**4d 的試跑**（2026-10-08，第 39 條）：把 `the-script-element/` 頂層還沒加的 testharness.js 檔案（46 個；`.xhtml`、`.svg` 的 runner 不跑）、`moving-between-documents/`，和 skip 清單裡等第 4 階段的 6 個一起跑：189 個裡 104 個全過。沒過的：
- **kun 的 bug**：`script-type-and-language-js.html` 的 `type=" "`：規範（和 Blink）只有 `type` 是空字串時才是 JavaScript，`" "` 去掉空白是空字串、不是 JavaScript 的 MIME type，不執行；kun 從第 1 階段起把去掉空白的空字串也當 JavaScript。修掉之後全過。
- **WPT server 沒有的**：`.py` handler（`async_003` 到 `async_010` 的 `log.py`，傳回 Python 原始碼、當 script 執行時是 `SyntaxError`；`load-error-events-1.html`、`script-charset-*`、`script-not-found-not-executed.html`、`module/errorhandling.html`；`moving-between-documents/` 的 helper 每個 script 都從 `.py` 來，56 個全部沒過（`ordering/` 的 4 個很快失敗，其餘逾時）；`script-text-modifications*.html` 的 `slow.py` 樣式表，kun 的 stylesheet 也不擋 script），pipe（`script-error-status-fires-error.html` 的 `?pipe=status(404)`），`.sub.`（`muted-errors.sub.html`、`script-crossorigin-network.sub.html`）。不加。
- **`document.write`**：`load-error-events-3.html`、`script-*-insertion-point-*.html`（逾時）。不加。
- **其他 API**：`innerText`（`moving-between-documents-during-evaluation.html` 用它設 inline script 的內容，加進清單、照失敗寫 baseline；`module/instantiation-error-8.html` 會逾時，進 skip）、`fetch`（`module/concurrent-failed-imports.html`，另外它期待抓失敗的 module 不留在 module map，見 `docs/limitations.md`；進 skip）、`referrerPolicy`（`script-referrerpolicy-idl.html`，baseline）、SVG 的 `<script>`（`svg-script-fragment-parsed-not-executed-on-clone.html`，baseline）。
- skip 清單裡等第 4 階段的 `module/execorder.html`、`inline-async-inserted-execorder.html`、`load-error-events-inline.html` 全過，拿掉。


## 參考資料

- HTML 規範：「Scripting」的「Fetching scripts」（fetch a module script graph、fetch a single module script、fetch the descendants of and link a module script）、「prepare the script element」、「HostLoadImportedModule」、「HostGetImportMetaProperties」，`https://html.spec.whatwg.org/multipage/webappapis.html#fetching-scripts`。
- ECMAScript：Cyclic Module Records、`Link()`、`Evaluate()`。
- Chromium：`third_party/blink/renderer/core/script/`（`modulator.h`、`module_map.h`、`module_tree_linker.h`、`module_script.h`、`js_module_script.h`、`script_loader.h`、`script_runner.h`、`dynamic_module_resolver.h`、`module_record_resolver_impl.h`）、`third_party/blink/renderer/bindings/core/v8/`（`module_record.h`、`referrer_script_info.h`、`v8_initializer.cc`）。
- V8 的 embedder API：`include/v8-script.h`（`Module`、`ScriptCompiler::CompileModule`）、`include/v8-callbacks.h`（`HostImportModuleDynamicallyCallback`、`HostInitializeImportMetaObjectCallback`）。
- Speedometer 3.1：`https://github.com/WebKit/Speedometer` 的 `release/3.1`，`index.html`，和 Speedometer 的 resources 目錄裡的 `main.mjs`、`benchmark-runner.mjs`。
