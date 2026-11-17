# RFC 0103 的細節：worker 的執行環境

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 現況的調查

2026-11-17，`main` 在 `e47eaa8`。

**`OpFront::current_window` 用在哪裡**（`crates/dom_api/src/`，呼叫的地方，包括 `abort.rs` 和 `request.rs` 各自包了一層的 helper 裡的那一次）：28 個檔案、50 處。

| 檔案 | 處 | 檔案 | 處 |
| --- | --- | --- | --- |
| `ui_event.rs` | 5 | `abort.rs` | 5 |
| `response.rs` | 4 | `message.rs` | 4 |
| `event.rs` | 3 | `url.rs` | 2 |
| `request.rs` | 2 | `performance.rs` | 2 |
| `history.rs` | 2 | `file_reader.rs` | 2 |
| `animation.rs` | 2 | 其他 17 個檔案 | 各 1 |

2026-11-17 逐處讀過，分成兩半：

**要的是執行環境的，25 處**：規範裡 worker 也有的 interface，拿 window 只是為了下面這些不需要文件的東西。

| 檔案 | 成員 | 拿 window 做什麼 |
| --- | --- | --- |
| `abort.rs`（5） | `new AbortController()`、`AbortSignal.abort()`、`.timeout()`、`.any()`，和包了一層的 helper | 新物件屬於哪個 realm；`timeout()` 加一個 UA 的 timer |
| `response.rs`（4） | `new Response()`、`Response.error()`、`.redirect()`、`.json()` | 新物件屬於哪個 realm；`redirect()` 要 base URL |
| `event.rs`（3） | `new Event()`、`new CustomEvent()`、`new ErrorEvent()` | event 的時間從那個 realm 的 time origin 算 |
| `message.rs`（3） | `new MessageChannel()`、`new MessageEvent()`、incumbent realm 的 origin | port 記在哪個 global 上；origin |
| `request.rs`（2） | `new Request()`，和包了一層的 helper | base URL 和 origin |
| `file_reader.rs`（2） | `new FileReader()`、`new ProgressEvent()` | 新物件屬於哪個 realm |
| `performance.rs`（2） | `new PerformanceMark()`、`new PerformanceObserver()` | 那個 realm 的 performance 的緩衝區 |
| `url.rs`（2） | `URL.createObjectURL()`、`URL.revokeObjectURL()` | origin |
| `event_target.rs`（1） | `new EventTarget()` | 新物件屬於哪個 realm |
| `blob.rs`（1） | `text()`、`arrayBuffer()`、`bytes()` 的讀取 | 在那個 global 上排一個 task |

合起來，執行環境要給的是：realm 的身分、time origin、origin、base URL、timer、task 的佇列、message port 的清單、performance 的緩衝區。

**Window 才有的，25 處**：

- 只把 window 當成 event 的 realm 的 event 建構子，11 處：`ui_event.rs` 的 5 個（`UIEvent`、`MouseEvent`、`WheelEvent`、`InputEvent`、`KeyboardEvent`）、`focus.rs` 的 `FocusEvent`、`history.rs` 的 `PopStateEvent` 和 `HashChangeEvent`、`animations.rs` 的 `TransitionEvent` 和 `AnimationEvent`、`animation.rs` 的 `AnimationPlaybackEvent`、`media_query_list.rs` 的 `MediaQueryListEvent`。它們和 `new Event()` 走同一個建 event 的函式；那個函式改成收執行環境之後，這幾處把 window 的執行環境交給它就好。
- 真的要 window 或它的文件的，14 處：`new Image()`、`new Option()`、`new CSSStyleSheet()`、`new KeyframeEffect()`、`new Animation()`、`Text` 這類節點的建構子（`character_data.rs` 的 `realm_document`）要文件；`new Document()`、`Document.parseHTMLUnsafe()` 要「相關的 window」；`new DOMParser()`、`new ResizeObserver()`、`new IntersectionObserver()` 記在 window 上；`HTMLElement` 的建構子（custom element）要 window 的 registry；`Location` 要 entry realm 的 base URL；`window.postMessage()` 的 `source` 是呼叫的那個 window。

每個 interface 在規範裡的 exposed 是起草時記得的；對照過 Chromium 的 IDL 的只有 `AbortController` 和 `EventTarget`（「Chromium 的對應」）。1a 寫表時每個 interface 對照它的規範。

**global 怎麼建**：`crates/dom_js/src/window.rs` 的 `create_window_realm` 在宣告表裡找名稱是 `Window` 的 interface，用它的 template 的 instance template 當新的 context 的 global template，在 DOM 裡建一份文件和它的 window、包起來，再呼叫 `crates/dom_js/src/templates.rs` 的 `install` 把表上的 interface 裝到 global 上。interface 的 template 是每個 isolate 一份，第一次用到時建。

**thread 和 isolate 怎麼建**：`crates/renderer/src/render_thread.rs` 的 `spawn_renderer_main_thread` 建 compositor thread 和 main thread；engine 由啟動的一方給的函式在 main thread 上建（`ScriptEngineFactory`）。isolate 在 `crates/bindings/src/script_state.rs` 建，V8 的 platform 是整個 process 一個（`v8::V8::get_current_platform()`）。

**訊息**：`crates/dom/src/message.rs` 的 `SerializedMessage` 是 `bytes: Vec<u8>` 和 `transferred: Option<Box<dyn Any + Send>>`，本身可以送到別的 thread。排著的訊息（`PostedMessage`）帶著來源的 window 和傳過來的 port（GC 的指標），這些不能跨 heap：跨 thread 時只送 `SerializedMessage` 和 port 的身分，到了那一邊再建。

**子資源的請求**：renderer 送給 Browser 的請求帶 `request_id`，回應是 `RendererMsg::ResourceResponse`，送到 renderer 的 main thread 的 scheduler（`crates/protocol/src/renderer.rs`）。

## `Window` 的狀態分到哪裡

`crates/dom/src/window.rs` 的 `Window` 的欄位，照「worker 的 global 要不要」分。這是 2026-11-17 讀欄位的註解分的，1a 開始前逐一確認。

| 去向 | 欄位 |
| --- | --- |
| 執行環境（兩種 global 都要） | `time_origin`、`timers`、`last_timer_id`、`nesting`、`native_timers`、`element_tasks`、`posted_messages`、`message_ports`、`fetches`、`blob_reads`、`crypto`、`performance`、`reporting`。`native_timers`（UA 自己的 timer）和 `element_tasks`（task 的佇列）是 2026-11-17 搬的時候加的：`AbortSignal.timeout()` 用前者，`FileReader`、blob 的讀取、performance timeline 的 task 用後者，worker 都有 |
| 留在 `Window` | `listeners`、`wrapper`（「已決定的事項」第 5 條）、`document`、`location`、`history`、`navigation_timing`、`hash_changes`、`animation_frames`、`last_animation_frame`、`intersection_observers`、`pending_intersection_observers`、`resize_observers`、`idle_callbacks`、`last_idle_callback`、`pending_images`、`page_focused`、`focus_visible`、`page_hidden`、`last_activation`、`custom_elements`、`current_event` |
| 要再看的 | `navigator`（worker 的是 `WorkerNavigator`，成員是 `Navigator` 的一部分） |

## 2A 的原型

2026-11-17，branch `0103`。要回答的：成員的函式能不能被擋在 window 之外，而 `declare!` 產生的程式拿得到。

- `declare!` 的 interface 收 `#[exposed(Window, Worker, DedicatedWorker)]`（沒寫是只有 `Window`），建構子收 `#[window]`；`#[window]` 寫在 exposed 不只 `Window` 的 interface 上是表的錯誤。
- `Front::op_window(op: &mut Self::Op<'o>, cx)` 是 `Front` 的 associated function。`#[window]` 的建構子在操作之前 `prepare_window`，操作裡由產生的程式呼叫 `<F as Front>::op_window(op, cx)`，把結果交給函式。
- `new CSSStyleSheet()` 改成 `#[window]`：函式不再拿 `op`，改拿 `window: Option<Gc<Window>>`。
- 試著在一個只有 `O: OpFront` 的函式裡寫 `<_ as Front>::op_window(op, cx)`：編譯錯誤 `E0283`（推不出 front 的型別）。成員改寫成 `fn f<F: Front>(op: &mut F::Op<'_>)` 也一樣：產生的程式呼叫它時推不出 `F`。試完拿掉了，沒有留在程式裡。

原型裡 `op_window` 的預設實作還是呼叫 `OpFront::current_window`；`current_window` 要等執行環境分出來、50 處都改完才能拿掉，那時 `op_window` 改成每個 engine 自己實作，「成員拿不到 window」才完整成立。

## Chromium 的對應

2026-11-17 對照 Chromium `main` 的 `0889dbff0b08`。路徑都在 Chromium 的 repo 裡；`core/` 是 `third_party/blink/renderer/core/`。

| machopo | Chromium | 在哪裡 | 原始碼怎麼說 |
| --- | --- | --- | --- |
| `ExecutionContext`（`Window` 和 worker 的 global 各拿著一個） | `ExecutionContext`（兩者繼承它） | `core/execution_context/execution_context.h` | 「An environment in which script can execute」：window 和 worker 共同的 base URL、security context、暫停和結束、console；「Typically, the ExecutionContext is an instance of LocalDOMWindow or of WorkerOrWorkletGlobalScope」 |
| `Window` | `LocalDOMWindow` | `core/frame/local_dom_window.h` | 繼承 `DOMWindow`、`ExecutionContext`、`WindowOrWorkerGlobalScope`、`WindowEventHandlers` |
| `WorkerGlobalScope` | `WorkerGlobalScope` | `core/workers/worker_global_scope.h` | 繼承 `WorkerOrWorkletGlobalScope`（它繼承 `EventTarget` 和 `ExecutionContext`）和 `WindowOrWorkerGlobalScope` |
| `DedicatedWorkerGlobalScope` | 同名 | `core/workers/dedicated_worker_global_scope.h` | `final`，繼承 `WorkerGlobalScope` |
| timer 的表 | `DOMTimerCoordinator` | `core/scheduler/dom_timer.cc` | `Supplement<ExecutionContext>`：「Maintains a set of DOMTimers for a given ExecutionContext」，worklet 不能有 |
| 宣告表的 exposed | IDL 的 `[Exposed]` | `core/dom/abort_controller.idl` 等 | `Window`：`Exposed=Window`；`WorkerGlobalScope`：`Exposed=Worker`；`DedicatedWorkerGlobalScope`：`Exposed=DedicatedWorker`；`AbortController`：`Exposed=(Window,Worker,ShadowRealm)`；`EventTarget`：`Exposed=(Window,Worker,AudioWorklet,ShadowRealm)`；mixin `WindowOrWorkerGlobalScope`：`Exposed=(Window,Worker)` |
| `Worker` | `DedicatedWorker` | `core/workers/dedicated_worker.h` | 文件那一側的物件；`Start` 呼叫 `CreateWorkerHost`，在 `OnScriptLoadStarted` 繼續 |
| worker 和文件之間的 channel | `DedicatedWorkerMessagingProxy`、`DedicatedWorkerObjectProxy` | `core/workers/` | README：「`MessagingProxy` is the main (parent) thread side proxy」「`ObjectProxy` is the worker thread side proxy」；訊息是 `BlinkTransferableMessage`，用 `PostCrossThreadTask` 送，早到的排在 `queued_early_tasks_` |
| worker 的 thread | `WorkerThread`、`WorkerBackingThread` | `core/workers/` | 「WorkerBackingThread represents a WebThread with Oilpan and V8」；isolate 是 `V8PerIsolateData::Initialize(..., ThreadState::Current()->ReleaseCppHeap())` |
| 在哪個 process | in-process worker | `core/workers/README.md` | 「A worker of this type always runs in the same renderer process with a parent document that starts the worker」；shared 和 service worker 是 out-of-process worker，可能在別的 renderer process |
| Browser 那一側（待決定） | `DedicatedWorkerHost` | `content/browser/worker_host/dedicated_worker_host.h` | 「A host for a single dedicated worker」，在 UI thread 上，Mojo 斷線或那個 `RenderProcessHost` 不在時自己刪掉；`WorkerScriptFetcher` 載入主 script 之後 `DidStartScriptLoad`；「Creates a network factory for subresource requests from this worker」；記著 worker 的 storage key、建立它的 frame 或 worker、擁有它的文件 |
| worker 的請求通道 | `WebWorkerFetchContext` | `third_party/blink/public/platform/web_worker_fetch_context.h` | 「a per-worker object created on the main thread, passed to a worker … and initialized on the worker thread … used to create a new URLLoader instance in the worker thread」 |
| `importScripts()` | `WorkerGlobalScope::ImportScriptsInternal` | `core/workers/worker_global_scope.cc` | `WorkerClassicScriptLoader::LoadSynchronously` |
| 結束 | `WorkerThread::Terminate` | `core/workers/worker_thread.h`、`.cc` | 「Queued tasks never run」；「forcibly terminates JavaScript execution after a certain grace period」：`kForcibleTerminationDelay` 是 2 秒，之後 `GetIsolate()->TerminateExecution()` |

只有 window 能用的地方，Blink 在執行時轉型：`DynamicTo<LocalDOMWindow>(context)`（`core/scheduler/dom_timer.cc`、`core/workers/dedicated_worker.cc`），不是 window 時得到 null；`DowncastTraits<LocalDOMWindow>` 看 `context.IsWindow()`（`core/frame/local_dom_window.h`）。binding 把執行環境交給實作是 `[CallWith=ExecutionContext]` 或 `[CallWith=ScriptState]`（`third_party/blink/renderer/bindings/IDLExtendedAttributes.md`）；同一份文件寫 `[Exposed]` 可以寫在 interface 上，也可以寫在個別的 method、attribute、常數上。

和起草時憑記憶寫的不同的兩處：timer 的表不是 `ExecutionContext` 的欄位，是掛在它上面的 supplement；Browser 知道每個 dedicated worker，而且是 Browser 開始載入主 script。

## `terminate()` 之後多久打斷正在跑的 script

2026-11-17 查的，回答「待決定」第 8 條要選的事。

**規範**：`terminate()` 的步驟是 terminate a worker，它和 worker 的 main loop 平行跑：設 closing flag、丟掉排著的 task、「Abort the script currently running in the worker」、清空頁面那一側的 port message queue，沒有等待的時間。規範留給 user agent 決定時間的只有另一種情形：worker 不再被需要（例如它的文件不在了）而 closing flag 設了之後還在跑時，user agent「may」terminate 它。

**Chromium 為什麼等 2 秒**：2016 年以前 `terminate()` 是馬上呼叫 V8 的 `TerminateExecution()`。之後改成先排一個收尾的 task，2 秒後 worker 還沒開始收尾才打斷（codereview 2011763002，「Worker: Attempt to gracefully terminate WorkerThread as much as possible」，crbug 487050）。理由：被打斷之後，正在進行的 V8 呼叫會回傳空的 handle，沒處理這種情形的 C++ 程式會 crash；先讓 worker 在兩個 task 之間自己收尾，只有跑不停的 script 才強制打斷。2 秒是沿用更早的強制結束機制的時間（code review 裡 reviewer 的建議，第一版是 3 秒），程式裡留著「Adjust the delay based on UMA」的 TODO。所以這是實作上的選擇，不是規範要求的。

**WPT**（`../third_party/wpt` 的 `0f195499f`）：`workers/` 裡呼叫 `terminate()` 的有 12 份，測的是頁面看得到的效果，沒有一份要求 worker 在多久之內停下：

- `constructors/Worker/terminate.html`、`Worker_terminate_event_queue.htm`：`terminate()` 之後頁面不再收到 worker 的 `message`（頁面那一側的 queue 清空，和 worker 什麼時候停無關）。
- `Worker-terminate-forever.html`、`Worker-terminate-forever-during-evaluation.html`（主 script、module、`importScripts()`、巢狀的 `importScripts()`、`import()`、top-level `await` 裡跑不停）：不 crash、不 hang、不觸發 `error`，被打斷的 script 之後的東西都不發生。
- `Worker-termination-with-port-messages.html`：還有上千則 port 的訊息沒送進 worker 時 terminate，不 hang、不 crash。
- `nested_worker_terminate_from_document.html` 等巢狀的：結束一串巢狀的 worker。

這些在兩種做法下都應該通過。看得出差別的是 worker 停下之前還能做的、頁面以外的事（`SharedArrayBuffer` 的寫入、同步的請求），WPT 沒有測它們。

## 1b 的原型：打斷 script

2026-11-17，branch `0103`（1a 之後），macOS（Apple Silicon）。程式在 `terminate-prototype/`。

一條新的 thread 上建 `V8Engine` 和一個 window，跑一個不會停的迴圈，另一條 thread 在 0、50、200、1000、5000、20000 µs 後呼叫 V8 的 `TerminateExecution()`；之後在那條 thread 上跑一次到期的 task、放掉 engine。9 種迴圈：建 event、建和插入節點（字串來回）、`dispatchEvent` 到一個 listener、丟和接 `DOMException`、`AbortController`（`abort()` 觸發 `onabort`）、`URL`、`Request`、`Response`、timer 和 microtask、`MessageChannel` 的訊息、`innerHTML` 和 `querySelectorAll`。

- **145 次，沒有 panic、沒有 crash**，engine 和 DOM 的 heap 在被打斷之後都放得掉。
- **兩種迴圈停不下來**：`dispatchEvent` 到 listener、`abort()` 觸發 `onabort`，打斷的時間不是 0 的每一次都停不下來。原因：`dom_js` 從 Rust 回頭呼叫 script 的每個地方（listener、timer、observer、idle callback 等，約 15 處，`listeners.rs`、`handlers.rs` 等）都用 V8 的 TryCatch 接住例外交給 `report_caught` 回報，打斷也被當成一般的例外接住、回報掉，外層的 JS 迴圈就繼續跑。

1b 要處理的：這些地方接住的是打斷時（TryCatch 的 `HasTerminated()`）不回報、讓它繼續往外傳（`ReThrow`），呼叫的那一側不再呼叫下一個 script（分派 event 時下一個 listener、跑 timer 時下一個 timer）。測試照原型的做法：每種回頭呼叫 script 的路都在被打斷時停得下來。

## V8 怎麼結束、清掉打斷

2026-11-17 做 1b-1 時讀 `../third_party/rusty_v8/v8/src` 查的，決定「已決定的事項」第 13 條。

- **打斷是 isolate 目前的例外的一個特殊值**（`Isolate::is_execution_terminating` 比較 `thread_local_top()->exception_` 和 `termination_exception`），清掉例外就清掉打斷。
- **`TryCatch` 結束時**（`api.cc` 的 `TryCatch::~TryCatch`）：接住的是打斷、而且 call depth 不是 0（embedder 還在某個 V8 API 的呼叫裡，也就是 stack 上還有 script）時再丟出去；call depth 是 0 時 `Reset()`，打斷就結束了。所以從 Rust 的 task 直接呼叫的 callback 被打斷之後，下一個 callback 開始時 isolate 已經不在打斷的狀態；從別的 thread 要求的打斷（`TerminateExecution`）是一個 interrupt，要等 script 跑到檢查點（函式的入口、迴圈的回跳）才生效，Rust 剛進入時也不會已經在打斷的狀態。1b-1 一開始在這些 callback 的入口加的 11 道檢查因此走不到（mutation test 都沒抓到），拿掉了。
- **會跑 script 的 API 一進來就清掉目前的例外**（`api-inl.h` 的 `PrepareForExecutionScope::ClearInternalException`）：呼叫函式、`NewInstance`（建 wrapper、`DOMException`）、reject promise、`JSON.parse` 都經過它。它之後才 `DCHECK(!is_execution_terminating())`，所以 debug 的 V8 加上 `--strict-termination-checks` 也抓不到。listener 被打斷之後，同一個成員裡接著用了這種 API，外層的 script 就繼續跑：`fetch` 的 signal 被 abort 時，abort 的 listener 不停地跑、被打斷，接著 `fetch` 的 promise 被 reject，外層的 `for (;;)` 停不下來（`dom_js` 的 `termination::script_called_back_stops_with_what_called_it` 的「a promise settled after a listener」）。不會跑 script 的 API（`Exception::TypeError`、`String::New`、`StackTrace::CurrentStackTrace`）用 `EnterV8NoScriptNoExceptionScope`，不清。
- **打斷進行中時取 stack trace 會 DCHECK 失敗**：`CaptureDetailedStackTrace` 整理 API callback 的 frame 時要建它的函式（`ApiNatives::InstantiateFunction`），那裡回報目前的例外（`ReportPendingMessages`，`AllowExceptions::IsAllowed`）。所以「stack 上還有沒有 script」也問不了 V8；`bindings::stop_again` 一律再要求打斷，由下一次最外層的進入 script（`bindings::trace::enter_script`）取消。
- **V8 的 inspector 的 `Runtime.terminateExecution`** 也是「打斷，到最外層的呼叫結束時取消」（用 `AddCallCompletedCallback`；rusty_v8 沒有這個 API 的 binding，所以用 `enter_script` 的計數）。

**Blink 怎麼做**（Chromium `main` 的 `a5229e6c`，2026-11-17）：`WorkerThread::Terminate` 先在 worker 的 thread 上排收尾的 task，2 秒後還在跑才在 `EnsureScriptExecutionTerminates` 呼叫 `TerminateExecution`（整個 Blink 只有這一處）；收尾時 `NotifyContextDestroyed`，之後 `IsCallbackFunctionRunnable` 看到執行環境結束、不再呼叫 callback。打斷之後檢查 `IsExecutionTerminating` 的地方：listener 的入口（`JSBasedEventListener::Invoke`）、event handler 回傳之後（`JSEventHandler`）、丟例外前（`V8ThrowException::ThrowException`）、建 `DOMException` 前（`V8ThrowDOMException`）、回報錯誤和沒處理的 rejection 前（`V8Initializer`、`RejectedPromises`）；頂層的 script 的 `TryCatch::CanContinue()` 是 false 時 worker 設 `ForbidExecution()`。一般的 callback function（`V8ScriptRunner::CallFunction`）、建 wrapper、promise 都不檢查，也不再要求打斷：從程式看，Blink 也擋不住上面 `fetch` 那種情形（沒有在 Chrome 上試），但它只在 script 跑超過 2 秒時才打斷，碰到的機會少得多。

## 使用量

chromestatus.com 的 `featurepopularity`，2026-11-15 的數字，Chrome 的 page load 比例。這幾個是 Chromium 舊的計數器，`python3 scripts/features.py` 沒有自動對到它們。

| 計數器 | 比例 |
| --- | --- |
| `WorkerStart` | 22.0% |
| `ServiceWorkerControlledPage` | 18.9% |
| `SharedWorkerStart` | 3.2% |
| `WorkletAddModule` | 0.1% |

WPT 的 `WEB_FEATURES.yml` 對應到 `dedicated-workers` 的測試檔有 212 份、`shared-workers` 61 份、`service-workers` 297 份，machopo 都還沒收。`tests/wpt/metadata/` 的 `.ini` 裡有 43 處註解寫「machopo has no workers」：`.any.js` 測試的 worker variant。

## 參考資料

- HTML 的 Web workers：<https://html.spec.whatwg.org/multipage/workers.html>
- WebIDL 的 `[Exposed]`：<https://webidl.spec.whatwg.org/#Exposed>
- Chromium 的 `third_party/blink/renderer/core/workers/` 和 `third_party/blink/renderer/core/execution_context/`
