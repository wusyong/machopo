# RFC 0103 的進度：worker 的執行環境

開始日期：2026-11-17。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 的原型：`#[window]` 的成員拿得到 window、其他成員拿不到，exposed 寫在表上 | 做完（程式留著，隨 1a 定下來） | 2026-11-17 |
| 1a：執行環境從 `Window` 分出來、成員改成拿執行環境、global 照 exposed 建 | 做完，merge 回 `main` | 2026-11-17 |
| 1b：dedicated worker | 做中：開始前的決定（第 8 到 12 條）做完；打斷 script 的原型做完（`details.md` 的「1b 的原型：打斷 script」） | |
| 1b-1：被打斷的 script 不被 `dom_js` 回頭呼叫 script 的地方吞掉 | 做完（「已決定的事項」第 13 條） | 2026-11-17 |
| 1b-2：`dom` 的 worker 的 global（自己的執行環境、event target），執行環境的 timer 和 task | 做完（第 14、15 條） | 2026-11-17 |
| 1b-3：`dom_api` 的 `Worker`、`WorkerGlobalScope`、`DedicatedWorkerGlobalScope` 和 worker 也有的 timer 等成員 | 還沒開始 | |
| 1b-4：worker 的 engine（`dom_api` 的 trait、`dom_js` 的實作，global 照 exposed 建） | 還沒開始 | |
| 1b-5：`renderer` 的 worker 的 thread 和 event loop、兩個方向的 `postMessage`、`terminate()`、`close()`、`onerror` | 還沒開始 | |
| 1b-6：Browser 的登記和主 script 的載入、子資源的請求帶 client、`importScripts()` | 還沒開始 | |
| 1b-7：WPT 的 `workers/`、情境、`about:demo`、文件、整個階段的檢查 | 還沒開始 | |
| 2：worker 裡的 `fetch()`、`MessagePort` 跨 thread、module worker | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 1a：`#[window]` 只能寫在只有 `Window` 的 interface 上，產生的程式把 window 交給函式；`#[exposed]` 的解析 | `cargo test -p dom_api_macros` |
| 1a：每個 interface 的 exposed 和規範一致（WPT 的 `interfaces/*.idl`） | `cargo test -p dom_api --test it table::interfaces_are_exposed_in_their_globals` |
| 1a：執行環境（time origin、timer、task、訊息、port、`fetch()`、blob、performance）在 `Window` 和成員之間行為不變 | `cargo test -p dom -p dom_api -p dom_js -p dom_wasm`；WPT 的相關目錄（「做過的檢查」的 1a 那幾段） |
| 1a：worker 也有的物件的事件從它的 realm 的 time origin 算 | `cargo test -p dom_js --test it engine::` |
| 1b-1：從別的 thread 打斷的 script 在 DOM 回頭呼叫的每種 callback 裡停下，呼叫它的 script 也停下、之後的一行都不跑，不回報錯誤；下一個 task 的 script 照常跑 | `cargo test -p dom_js --test it termination::`、`cargo test -p bindings --test it termination::`；debug 的 V8 加上 `--strict-termination-checks` 跑同樣的測試 |
| 1b-2：worker 的 global 的執行環境回答它自己的 URL、base URL 和 origin；它的 timer、task、port 的訊息是它的執行環境的，和同一個 `Dom` 裡的 window 分開；它是自己的 event target | `cargo test -p dom --test it worker_global::`、`events::every_target_holds_listeners`、`tasks::` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a 的原型（2026-11-17，macOS，Apple Silicon）

- 新的測試：`dom_api_macros` 的 `a_window_constructor_is_given_the_window`、`only_a_window_interface_is_given_the_window`、`an_interface_is_exposed_in_a_known_global`、`window_marks_a_constructor`；`member_attributes_go_where_they_may` 的錯誤訊息跟著加上 `#[window]`。
- 擋不擋得住：只有 `O: OpFront` 的函式呼叫 `<_ as Front>::op_window(op, cx)` 是 `E0283`（試完拿掉，見 `details.md` 的「2A 的原型」）。
- 跑過的：`cargo test -p dom_api_macros` 20 個通過；`cargo test -p dom_api` 219 個通過；`cargo test -p dom_js --test it style_sheets::` 12 個通過；`python3 scripts/test.py wpt` 跑 `css/cssom/CSSStyleSheet-constructable-*` 的 12 份，沒有和預期不同的結果；`python3 scripts/sanity.py docs` 通過。
- mutation test 還沒跑：原型的程式會在 1a 改完 50 處時一起定下來，那時一起跑。

### 1a：欄位搬進 `ExecutionContext`（2026-11-17，macOS，Apple Silicon）

- `crates/dom/src/execution_context.rs`：`ExecutionContext` 拿著 time origin、timer、UA 的 timer、task 的佇列、排著的訊息和 port、`fetch()`、blob 的讀取、`crypto`、`performance`、回報例外中的旗標；`Window` 拿著一個（`context`），`Cx::window_context` 拿到它。`dom` 裡原本讀 `Window` 這些欄位的 48 處改成經過它，函式的簽名沒變。
- 跑過的：`cargo test -p dom` 228 個通過。

### 1a：物件改記執行環境（2026-11-17，macOS，Apple Silicon）

- `dom`：`AbortSignal`、`new EventTarget()`、`FileReader`、`MessagePort` 的「屬於哪裡」從 window 改成執行環境；建 event 的五個函式收執行環境（`now_in`）；`target_context`；執行環境指回它的 window（`context_window`，只在 `dom` 裡），`context_base_url`、`context_origin`；算 origin 的邏輯從 `dom_api` 的 `frame` 搬進 `dom`（`origin_document`、`document_origin`）；target 的 task 和 UA 的 timer 收執行環境。
- `dom_api`：`OpFront::current_context`（暫時經過 `current_window`）；`AbortController`、`AbortSignal`、`Event` 和各種 event 的建構子、`FileReader`、`MessageChannel`、`new EventTarget()` 改用它。
- 跑過的：`cargo check --workspace --all-targets` 通過；`cargo test -p dom -p dom_api -p dom_api_macros` 467 個通過；`cargo test -p dom_js` 514 個通過（1 個原本就 ignore）。

### 1a：成員只拿執行環境、`#[window]`、exposed（2026-11-17，macOS，Apple Silicon）

- `dom`：`Request`、`Response`、performance 的物件（`performance`、entry、observer）、blob 的讀取改記執行環境；`context_url`、`context_navigation_timing`、`context_unix_time_origin`、`register_context_blob_url`（blob URL 的登記，文件只在 `dom` 裡）；從 performance 的物件找 window 的函式回傳 `Option`（「已決定的事項」第 7 條）。
- `dom_api`：50 處的其他 31 處：worker 也有的（`Request`、`Response`、`fetch()` 的回應、blob 的讀取、`URL.createObjectURL`、`revokeObjectURL`、`PerformanceMark`、`PerformanceObserver`、`MessageEvent`）改用 `current_context`；`Location` 的 entry realm 的 base URL 改從它的執行環境拿；window 才有的 15 個成員（`new Text()`、`new Comment()`、`new DocumentFragment()`、`new Document()`、`new Range()`、`new DOMParser()`、`new KeyframeEffect()`、`new Animation()`、`new ResizeObserver()`、`new IntersectionObserver()`、`HTMLElement` 的建構子、`new Image()`、`new Option()`、`Document.parseHTMLUnsafe()`、`window.postMessage()`）標 `#[window]`。
- `OpFront::current_window` 拿掉了：`OpFront` 只給執行環境（`current_context`），window 只經過 `Front::op_window`；`dom_js` 的 `InfoFront` 和 `dom_api` 的測試的 front 各自實作。
- `declare!`：`#[window]` 也能標在 method 上（一般的、static、legacy factory），和 `#[calls_script]`、`#[then_script]`、`#[realm]` 不能並用。
- exposed：宣告表 266 個 interface，照 WPT 的 `interfaces/*.idl`（`0f195499f`）的 `[Exposed]` 寫：36 個標 `#[exposed(Window, Worker)]`（IDL 是 `*`、`(Window,Worker)` 或 `(Window,Worker,AudioWorklet)`），其他只在 Window（`SVGMatrix`、`SVGPoint`、`SVGRect` 在 IDL 裡沒有自己的 `[Exposed]`，是 SVG 2 的舊別名，只在 Window）；`InterfaceDesc::exposed`；`dom_js` 的 `install` 只裝 exposed 在 window 的。
- 新的測試：`dom_api_macros` 的 `window_marks_a_constructor_or_a_method`、`a_window_method_is_given_the_window`；`dom_api` 的 `table::interfaces_are_exposed_in_their_globals`。
- 新的測試：`dom_js` 的 `engine::a_signal_s_event_counts_from_its_realm_s_origin`（mutation test 沒抓到的那一條補的，見下一條）。
- mutation test（`rfcs/0103-worker-global-scope/mutants-1a.toml`，`python3 scripts/test.py mutate rfcs/0103-worker-global-scope/mutants-1a.toml`）：17 條。第一次跑 16 條抓到，沒抓到的是「從 abort signal 找不到它的執行環境」（`target_context` 少了 abort signal 那一項）：signal 的 `abort` event 改從 DOM 建立時起算，沒有測試看它的時間。補了上面那個測試（window 在 DOM 建立 30 ms 之後才有 time origin，`abort` event 的 `timeStamp` 不能比 time origin 之後經過的時間多），重跑那一條抓到。抓到每一條的測試：`declare!` 的錯（`#[window]` 寫在 worker 也有的 interface 上、`#[exposed]` 被丟掉）由 `dom_api_macros` 和 `dom_api` 的 `table::interfaces_are_exposed_in_their_globals`；`#[window]` 的建構子和 method 拿不到 window、V8 的 front 不給 window 或執行環境、event 的時間不從 time origin 算、執行環境沒有 base URL 或 origin、blob URL 沒登記、target 的 task 丟了它的 target、port 不被 owner 留著或收到的 port 沒有 owner、沒有 navigation timing、`Location` 不照 entry realm 的 base URL、window 的 realm 一個 interface 都不裝，由 `dom_js` 的測試。
- WPT（`python3 scripts/test.py wpt dom/events/ dom/abort/ fetch/api/ FileAPI/ hr-time/ user-timing/ performance-timeline/ navigation-timing/ webmessaging/ url/ html/browsers/history/the-location-interface/ dom/nodes/ dom/ranges/ domparsing/ custom-elements/ css/cssom/ web-animations/ intersection-observer/ resize-observer/ html/semantics/embedded-content/the-img-element/ html/semantics/forms/the-option-element/ WebCryptoAPI/`）：2880 份，5 個 subtest 和預期不同，都和這次改動無關：
  - `dom/events/scrolling/` 的 `scrollend-event-fired-after-sequence-of-scrolls.tentative.html`（兩個）、`scrollend-event-for-user-scroll.html` 和 `scrollend-event-fires-to-iframe-window.html` 的 touch dragging 各一個意外通過：測試在 `navigator.platform` 有 `MAC` 時直接 return，machopo 在 Mac 上是 `MacIntel`；預期結果是 Linux 的。單獨重跑兩次一樣。
  - `intersection-observer/v2/delay-test.html` 失敗一次（`expected 2 but got 1`），單獨跑 12 次都通過：只在二十個目錄平行跑時失敗。測試在 rAF 裡比較 `performance.now()` 和第一筆 entry 的時間，判斷 delay 的計時器是不是在上一個 frame 之前到期，機器忙、frame 拉長時判斷會和 observer 的更新錯開。這是推測，沒有在 `main` 上同樣的負載下比較過。
- 整個階段的檢查：`cargo fmt --check`（`cargo fmt` 之後；mutation spec 的三條原文跟著改）、`cargo clippy --workspace --all-targets -- -D warnings`（修了一個 doc 註解後的空行）、`python3 scripts/sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 都通過。
- 只靠審查、記進 `docs/test-gaps.md` 的：`dom` 沒有「給執行環境、拿回 window 或文件」的公開函式。
- `cargo test --workspace`：`shell` 的 `devtools_initiators::script_asks_with_its_stack` 失敗（`/y.png` 的請求的 stack 是 `again`，不是 `anew`）。單獨跑、只跑 devtools 的 91 個都通過，整個 `shell` 的 579 個一起跑在 branch 上失敗 4 次（4 次都失敗），把改動收起來在 `main` 的程式上跑 2 次都通過；加上除錯輸出之後在 branch 上也通過了。原因（讀程式的推論，沒有在 `main` 上重現出失敗）：renderer 的 loop 一輪先跑完 channel 上已經有的每個 task，才開始載圖、清掉沒用到的 stack（`render_thread.rs` 的 `run`、`clear_request_stacks`）；測試收到 `again()` 的回應就送 `anew()`，機器忙時兩個 task 在同一輪跑，`again()` 設的圖不用載入但記下了 stack，`anew()` 對同一個元素再記時照「先記的贏」（`dom` 的 `note_request_stacks` 的 `or_insert`，`dom` 的 `initiators` 測試寫明這樣）被丟掉。loop 和「先記的贏」都是 `main` 上就有的，這次的改動只改了時序。2026-11-17 決定（選項 C）：不改「先記的贏」，測試在 `again()` 和 `anew()` 之間等一個 animation frame（它在那一輪 loop 結束之後才跑），`docs/limitations.md` 的 DevTools 那一條記下「同一個元素在同一輪裡被要求兩次時是第一次的 stack」。改了之後整個 `shell` 跑 3 次都通過；把清掉沒用到的 stack 拿掉（`dom` 的 `clear_request_stacks` 不清），這個測試照樣失敗（臨時的 mutation，沒有寫進 spec：它守的是 DevTools 的 initiator，不是這份 RFC 的程式）。
- `cargo test --workspace --no-fail-fast`（前一次在 `shell` 失敗後停下，之後的 crate 沒跑到）：56 個測試 binary、3990 個通過、0 個失敗、1 個 ignore（原本就 ignore 的）；這一次 `devtools_initiators::script_asks_with_its_stack` 也通過，和上面「時序上的競爭」的推論一致。

### 1b-1：被打斷的 script 停得下來（2026-11-17，macOS，Apple Silicon）

- `bindings::throw`：被打斷時不丟例外（V8 的 `ThrowException` 先清掉目前的例外，打斷會被換成 script 接得住的例外；Blink 的 `V8ThrowException::ThrowException`）；`dom_js` 和 `bindings` 原本直接呼叫 `throw_exception` 的 9 處都改經過它。`dom_js` 的 `new_dom_exception`、`new_quota_exceeded_error` 被打斷時不建（Blink 的 `V8ThrowDOMException`）。
- `dom_js` 的 `report_caught_of`：接住的是打斷時不回報、不 `reset`，交給 `bindings::stop_again`；node filter、解析 JSON 接住打斷時也是。
- `bindings::stop_again` 再要求一次打斷，記下 isolate；`bindings::trace::enter_script` 從頭進入 script 時先取消它（「已決定的事項」第 13 條，`details.md` 的「V8 怎麼結束、清掉打斷」）。
- 回頭呼叫 script 的入口在被打斷時直接回來：listener、event handler、編譯 event handler、升級元素、lifecycle callback、script 元素、settle promise、fetch 被 abort 時的 reject、建構子被打斷之後不建 failed element。從 Rust 的 task 直接呼叫的 callback（timer、四種 observer、animation frame、blob、idle 和 geolocation、animation 的 promise）和 parser 建 custom element 的入口一開始也加了，第一次 mutation test 都沒抓到：打斷在那裡已經被 V8 結束，從別的 thread 要求的打斷要等 script 跑才生效，走不到，拿掉了。
- `dom_js::ScriptTerminator`（`V8Engine::terminator`）：從別的 thread 打斷 engine 的 script，1b-5 的 `terminate()` 用它。
- 新的測試：`dom_js` 的 `termination::script_called_back_stops_with_what_called_it`（DOM 回頭呼叫 script 的 21 種路，加上 8 種「同一個呼叫裡接著要呼叫下一個」的：下一個 listener、listener 之後的 handler、從屬性編譯的下一個 handler、下一個要升級的元素、下一個 lifecycle callback、下一個 script 元素、dependent signal 的 listener、listener 之後被 reject 的 fetch；每種都要停下、不回報錯誤、呼叫之後的一行和 `catch` 都不跑），`termination::script_stops_at_any_point_of_its_calls_to_the_dom`（9 種一直呼叫 DOM 的迴圈，各在 0、200、5000 µs 後打斷）；`bindings` 的 `termination::a_stop_asked_for_again_stops_the_script_under_the_callback`、`termination::a_stop_asked_for_again_ends_before_the_next_script`。
- 找到的漏洞：V8 會跑 script 的 API 一進來就清掉打斷（`details.md` 的「V8 怎麼結束、清掉打斷」）。第一次 mutation test 之後補的巢狀的測試裡，`fetch` 的 signal 被 abort、listener 被打斷之後 reject `fetch` 的 promise，外層的 script 停不下來；改成再要求打斷之後停得下來，但還會多跑一行（測試改成看呼叫之後的一行之後才看到），`reject_promise_with_reason` 補了入口的檢查。
- 測試本身的問題（已經改掉）：測試的 window 沒有 `document.body`；打斷在第一行之前時 `reported` 沒設；`.catch` 的 callback 是外層被打斷之後的 microtask，照 window 的規則會跑（worker 的 loop 不跑）；`observedAttributes` 只在有 `attributeChangedCallback` 時才讀。
- debug 的 V8：`V8_FORCE_DEBUG=1 CARGO_TARGET_DIR=$PWD/target/v8-debug MACHOPO_V8_FLAGS=--strict-termination-checks cargo test -p dom_js --test it termination::` 通過。它抓不到「API 清掉打斷」（`PrepareForExecutionScope` 先清、後 DCHECK）；暫時拿掉 custom element 建構子之後的檢查時是「停不下來」失敗，不是 DCHECK。再要求打斷之前先問 V8「stack 上還有沒有 script」（`StackTrace::CurrentStackTrace`）在 debug 的 V8 上 DCHECK 失敗（`ApiNatives::InstantiateFunction` 回報目前的例外），改成一律再要求。
- mutation test（`rfcs/0103-worker-global-scope/mutants-1b1.toml`，`python3 scripts/test.py mutate rfcs/0103-worker-global-scope/mutants-1b1.toml`）：跑了三輪。第一輪 23 條抓到 4 條：從 task 直接呼叫的 11 道入口的檢查走不到（拿掉了），巢狀的 7 道沒有測試走到（補了測試，找到上面的漏洞）。照「已決定的事項」第 13 條改了之後 21 條，最後一輪抓到 7 條：打斷被回報和清掉、再要求打斷什麼都不做、再要求的打斷留到下一個 script、丟例外換掉打斷、從不判斷被打斷、建構子被打斷之後建 failed element、fetch 被 reject（`dom_js` 的 `termination::`；`stop_again` 和取消那兩條是 `bindings` 的 `termination::`）。沒抓到的 14 條：
  - 回頭呼叫 script 的入口的 7 道（下一個 listener、handler、編譯 handler、升級、lifecycle callback、script 元素、settle promise）：等價，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。拿掉之後下一個 callback 被呼叫，但再要求的打斷讓它在函式的入口就停下，看不到差別。
  - `report_caught_of`、node filter、JSON 接住打斷之後呼叫 `stop_again`（3 條，`stop_again` 本身由 `bindings` 的測試守著）、node filter 把打斷交給成員再丟一次、建 `DOMException` 和 `QuotaExceededError` 前的檢查、JSON 解析到一半被打斷時不 reject：記進 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」。在測試走得到的路上，清掉打斷的 API 都有自己的入口檢查（兩層互相遮住）；沒找到「被打斷之後同一個成員裡丟 `DOMException`」的路；`busy` 的 JSON 迴圈不一定在解析中被打斷。
- 跑過的：`cargo test -p dom -p dom_api -p dom_js -p dom_wasm`（改 `stop_again` 之前；之後 `cargo test -p dom_js -p bindings` 都通過）。

### 1b-2：worker 的 global 和收執行環境的函式（2026-11-17，macOS，Apple Silicon）

- `dom` 的 timer、task、訊息、回報例外的函式改收執行環境（「已決定的事項」第 15 條）：`add_timer`、`remove_timer`、`time_at`、`next_due`、`next_script_timer_due`、`take_due`、`timer_nesting`、`set_timer_nesting`、`take_due_native_timer`、`has_element_task`、`take_element_task`、`has_posted_message`、`take_posted_message`、`next_task`、`end_task_pass`（收執行環境的 slice）、`reporting`、`set_reporting`。`hashchange` 的 task 和 `requestIdleCallback` 的 timeout 只有 window 有，照執行環境的 global 判斷。`dom_api`、`dom_js`、`dom_wasm`、`renderer` 和測試裡約 100 處呼叫改成先拿 window 的執行環境（`window_context`）。`crypto`、`performance`、`fetch()` 的函式還收 window，第 2 階段用到時再改。
- `dom` 的 `WorkerGlobalScope`（`worker_global.rs`，「已決定的事項」第 14 條）：執行環境、URL、origin、listener、wrapper 的參照；`create_worker_global`、`worker_global_context`、`worker_global_url`、`worker_global_origin`。`ExecutionContext` 的 global 分成 `window` 和 `worker` 兩個欄位，`context_base_url`、`context_url`、`context_origin` 對 worker 的 global 回答它自己的；`context_worker_global`。`TargetRef`、`TargetRoot`、`ObjectRef`、`ObjectRoot` 多一種 `WorkerGlobal`：event 的路徑只有它自己（和 `new EventTarget()` 一樣不往上走、不 retarget），沒有 window（`target_window` 是沒有），執行環境是它自己的（`target_context`），listener 是它自己的；觸控和滾輪的 listener 不預設 passive。
- 留給之後的：`dom_api` 的 `interface_of` 把 worker 的 global 暫時對應到 `EventTarget`（1b-3 加上 `DedicatedWorkerGlobalScope` 時換掉）；把 `this` 轉成 target 的地方（`dom_api` 的 `typed.rs`、`dom_js` 的 `info_front.rs`、`devtools_page.rs`）、`onerror` 的特別的呼叫方式（`dom_js` 的 `listeners.rs` 只認 window；HTML 的 worker 的 global 的也是 `OnErrorEventHandler`）在 1b-4 有 worker 的 realm、測試走得到時改；`context_target` 對 worker 的 global 回答它（發給 global 自己的佇列的訊息的 target）在 1b-5 有 `postMessage` 時補測試。
- 新的測試：`dom` 的 `worker_global::a_worker_s_global_answers_its_url_and_origin`、`a_worker_s_global_runs_its_own_timers`、`a_worker_s_global_delivers_its_ports_messages`、`an_event_at_a_worker_s_global_is_at_it`，`tasks::an_idle_callback_s_timeout_is_a_task_once_due`（mutation test 沒抓到的那一條補的：之前沒有測試看 `next_task` 給出 idle callback 的 timeout），`events::every_target_holds_listeners` 加上 worker 的 global。
- mutation test（`rfcs/0103-worker-global-scope/mutants-1b2.toml`）：10 條。第一次 7 條抓到，沒抓到的 3 條：「idle callback 的 timeout 不是 `next_task` 的候選」（補了上面的測試，重跑抓到）、「讀回的 erased 的 worker 的 global 當成一般的 target」（補了 `an_event_at_a_worker_s_global_is_at_it`，重跑抓到）、「發給 worker 的 global 自己的訊息的 target 是 window」（現在沒有東西會把訊息排進 worker 的 global 自己的佇列，1b-5 的 `postMessage` 補測試）。
- 跑過的：`cargo test -p dom -p dom_api -p dom_js -p dom_wasm -p renderer` 都通過。

