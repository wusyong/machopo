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
