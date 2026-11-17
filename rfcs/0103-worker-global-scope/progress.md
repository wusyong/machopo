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
| 1b-3：`dom_api` 的 `Worker`、`WorkerGlobalScope`、`DedicatedWorkerGlobalScope` 和 worker 也有的 timer 等成員 | 做完（第 16、17 條） | 2026-11-17 |
| 1b-4：worker 的 engine（`dom_api` 的 trait、`dom_js` 的實作，global 照 exposed 建） | 做完（第 18 條） | 2026-11-17 |
| 1b-5：`renderer` 的 worker 的 thread 和 event loop、兩個方向的 `postMessage`、`terminate()`、`close()`、`onerror` | 做完（第 19 條；主 script 暫時只有 `data:` 的網址） | 2026-11-17 |
| 1b-6：Browser 的登記和主 script 的載入、子資源的請求帶 client、`importScripts()` | 做完（第 20 條） | 2026-11-17 |
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
| 1b-3：`new Worker()` 照 realm 的 base URL 解析網址、帶名稱和 realm 的 origin 當場請 host 啟動 worker，錯的網址、`type` 和沒有 host 時丟對的例外；`postMessage()`、`terminate()` 當場經過 host；worker 的 global 的 timer、`name`、`self`、`crypto`、`postMessage()`、`close()` | `cargo test -p dom_api --test it worker::`、`cargo test -p dom --test it worker::`；表的 `table::` 測試 |
| 1b-4：worker 的 engine 的 realm 的 global 是 worker 的 global，只有 dedicated worker 有的 interface，`[Global]` 的成員是 global 自己的；它的 timer、`Worker` 送來的訊息是它的 task，送出的和 `close()` 經過 host，例外到它的 `onerror`（`OnErrorEventHandler` 的呼叫方式），script 之後 microtask checkpoint，從別的 thread 打斷得了 | `cargo test -p dom_js --test it worker::` |
| 1b-5：整個瀏覽器裡 `data:` 網址的 worker：兩個方向的訊息、例外到 `Worker` 和 window、`terminate()` 停得了跑不停的、`close()` 之後沒有、script 拿不到時 `Worker` 上是 `error`、renderer 結束時等 worker 的 thread；文件不在時它的 worker 結束 | `cargo test -p shell --test it workers::`、`cargo test -p dom_js --test it worker::`、`cargo test -p renderer --lib worker_thread` |
| 1b-6：worker 的主 script 經過 Browser 載入（同源、redirect 之後的網址是 global 的、照 Chrome 寬鬆地檢查 MIME type、`data:` 不檢查、狀態不是 2xx 是 `error`），script 跑之前 `Worker` 送的訊息之後才到；`importScripts()` 照順序同步載入、執行（`no-cors`，別的 origin 的例外變成 `NetworkError`，自己的例外照樣往外丟，抓不到的是 `NetworkError`，等的期間其他 task 留著）；worker 不在時 Browser 關掉它的載入 | `cargo test -p shell --test it workers::`、`cargo test -p dom_js --test it worker::` |

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

### 1b-3：`dom_api` 的 worker 的 interface（2026-11-17，macOS，Apple Silicon）

- `dom`：`WorkerObjectData`（`worker.rs`，「已決定的事項」第 16 條）：`EventTargetImpl` 的 `worker` 欄位，host 給的編號、是不是結束了；`create_worker_object`（屬於建它的執行環境）、`is_worker`、`worker_id`、`worker_terminated`、`terminate_worker_object`。worker 的 global 多了名稱（`worker_global_name`，`create_worker_global` 多一個參數）。`crypto` 改收執行環境（`context_crypto`）。
- `dom_api`：`WorkerHost`（`start_worker`、`post_to_worker`、`terminate_worker`）、`WorkerGlobalHost`（`post_to_owner`、`close`）、`WorkerStart`（`worker.rs`，「已決定的事項」第 17 條）；`OpFront::worker_host`、`worker_global_host`，預設沒有。表上的 `Worker`（建構子、`terminate()`、`postMessage()`、`onmessage`、`onmessageerror`、`onerror`；exposed 在 Window 和 Worker，照 WPT 的 IDL）、`WorkerOptions`（`name`、`type`）、`WorkerGlobalScope`（`#[global]`、exposed 在 Worker：`self`、`onerror`、`crypto`、timer、`structuredClone`）、`DedicatedWorkerGlobalScope`（`#[global]`、exposed 在 DedicatedWorker：`name`、`postMessage()`、`close()`、`onmessage`、`onmessageerror`）。timer、`crypto`、`structuredClone` 的 window 的版本先拿它的執行環境，worker 的 global 另有入口，走同一個函式。`interface_of`：`Worker` 對應到 `Worker`、worker 的 global 對應到 `DedicatedWorkerGlobalScope`（1b-2 暫時的 `EventTarget` 換掉了）。
- `dom_js` 的 `V8Engine` 還沒有給 host：window 裡的 `new Worker()` 現在是 `NotSupportedError`，1b-5 給 host 之後才會啟動。`wrappers::the_tables_members_are_webidl_shaped` 只看 window 有的 interface；worker 的 global 的 interface 在 1b-4 有 worker 的 realm 時照樣檢查。`dom_wasm` 的 guest 沒有這三個 interface 的成員（不是節點的 interface 整個跳過）。
- 測試的 front（`dom_api` 的 tests/it/main.rs）：dictionary 可以有字串的成員（`Arg::StrDict`、`dict_string`），`message_arg`（字串的 bytes，後面的物件是 transfer 的 port），兩個 worker 的 host 記下呼叫（`Listeners::workers`），`current_context` 也給 worker 的 global 的（`Listeners::current_worker`）。
- 新的測試：`dom_api` 的 `worker::a_worker_starts_its_worker`、`a_worker_that_can_t_start_throws`、`a_worker_posts_and_terminates_through_its_host`、`a_worker_s_global_has_its_own_timers`、`a_worker_s_global_s_own_members`；`dom` 的 `worker::a_worker_is_a_target_of_its_realm_with_its_worker_s_number`、`a_worker_s_global_has_its_name`；表的測試（interface 的順序、exposed 的清單、只在 dedicated worker 的、不在 window 的）、`dom/dictionary_members.txt` 加上 `name`。
- 沒有測到、留給之後的：worker 的 global 的 event handler（`onmessage` 等）和 `setInterval` 的「會重複」在 1b-4 有 worker 的 realm、從 JS 跑時測。
- mutation test（`rfcs/0103-worker-global-scope/mutants-1b3.toml`）：21 條全部抓到（其中一條原本的寫法編不過：最後一個分支加了 guard，`match` 就不完整，改寫之後抓到）。
- WPT（`python3 scripts/test.py wpt`）：`include.txt` 裡的 6 份 `idlharness`、預期結果提到 worker 的 5 份、`netinfo/`、`geolocation/`、`screen-wake-lock/`：2341 個檢查，沒有和預期不同的。
- 跑過的：`cargo test -p dom -p dom_api -p dom_js -p dom_wasm -p renderer` 都通過。

### 1b-4：worker 的 engine（2026-11-17，macOS，Apple Silicon）

- `dom`：`GlobalRef`、`GlobalRoot`（「已決定的事項」第 18 條），`global_context`、`root_global`；`post_worker_global_message`（`Worker` 送給 worker 的 global 的訊息：沒有 origin、沒有 source，target 是 worker 的 global）；`take_blob_read` 改收執行環境。
- `dom_api`：跑 task 的函式（`run_due`、`run_next_due`、`run_task`、`until_next_due`、`next_due`、`run_posted_messages`、`run_element_tasks`、`report_exception`）和 `ScriptHost::call_timer`、`deserialize_message` 改收 `GlobalRoot`；idle callback 的 timeout、geolocation、wake lock 的 task 只有 window 的。`WorkerEngine` trait（第 10 條）。1b-3 的 `WorkerHost`、`WorkerGlobalHost` 的方法改成 `&self`、`OpFront` 給 `Option<&dyn …>`：`dom_js` 的操作只借得到 `State` 的 `Caches`（共享的），host 放在那裡（`Caches::worker_host`、`worker_global_host`）；送到別的 thread 的實作本來就不需要 `&mut`。
- `dom_js`：`V8WorkerEngine`（`worker.rs`）：自己的 isolate 和 DOM，realm 從 `DedicatedWorkerGlobalScope` 的 template 建；`templates::install` 收 `GlobalKind`（worker 的 realm 沒有 legacy callback interface、alias、legacy factory）；`[Global]` 的祖先（`WorkerGlobalScope`）的成員也裝在子的 instance template 上（V8 不套用 parent 的 instance template：原本只對 unforgeable 的成員這樣做，第一次跑測試時 `self`、`setTimeout` 不在 global 上）；`window::global_of`、`global_root`，跑 task 和回報例外的函式 window 和 worker 共用；`InfoOp::current_context` 也給 worker 的 realm 的；`this` 和物件參數轉成 target 時認得 worker 的 global；`onerror` 的特別的呼叫方式也給 worker 的 global（1b-2 留下的三處）；`call_timer` 的 `this`、URL 和 base URL 從 global 和它的執行環境拿。
- 新的測試：`dom_js` 的 `worker::a_worker_s_global_is_its_realm_s`、`a_worker_s_microtasks_run_after_its_script`（mutation test 沒抓到的那一條補的）、`a_worker_s_realm_has_the_interfaces_exposed_there`（表上每個 interface：dedicated worker 有的在、沒有的不在）、`a_worker_s_timers_are_its_tasks`（`setInterval` 會重複、`this` 是 global：1b-3 留下的）、`a_worker_s_messages_go_both_ways`（worker 送出的訊息當成它的 `Worker` 送來的再給它：`onmessage`、listener、`source`、`origin`：1b-3 留下的 event handler）、`a_worker_s_errors_reach_its_onerror`、`a_worker_makes_no_worker`、`a_worker_s_script_stops_from_another_thread`。
- mutation test（`rfcs/0103-worker-global-scope/mutants-1b4.toml`）：14 條。第一次 12 條抓到，沒抓到的：「script 之後不做 microtask checkpoint」（補了上面的測試，重跑抓到）、「worker 的 global 當成參數時是一般的 target」：等價，記進 `docs/test-gaps.md`（worker 有的成員沒有收 event target 參數的：收它的只有 `MouseEvent`、`FocusEvent` 的 `relatedTarget`，只在 window；window 的 realm 拿不到 worker 的 global）。1b-2 沒抓到的「發給 worker 的 global 的訊息的 target 是 window」用 `dom_js` 的 `worker::` 重跑，抓到（`mutants-1b2.toml` 的那一條改用它）。
- 找到、留給 1b-7 的：沒有 window 的執行環境裡，一般的 target 的 task 和 UA 的 timer 被丟掉（`dom` 的 `push_target_task`、`add_target_timer` 照執行環境的 window 的文件排）：worker 裡 `FileReader` 的事件、`AbortSignal.timeout()`、`Blob` 的 `text()` 等現在不會完成。要讓 task 的元素可以沒有；1b-7 跑 WPT 的 worker variant 時看用到的有哪些再改。
- WPT（`python3 scripts/test.py wpt dom/events/ html/webappapis/timers/ webmessaging/ html/webappapis/scripting/ html/webappapis/microtask-queuing/ dom/abort/ url/idlharness.any.js FileAPI/`）：3324 個檢查，和預期不同的 4 個是 1a 記下的 `scrollend` 那幾個（Mac 上 `navigator.platform` 的關係），和這次無關。
- 跑過的：`cargo test -p dom -p dom_api -p dom_js -p dom_wasm -p renderer -p bindings` 都通過。

### 1b-5：renderer 的 worker 的 thread（2026-11-17，macOS，Apple Silicon）

- 5a（頁面那一側）：`dom` 的執行環境留著沒結束的 `Worker`（照編號找：`context_worker`、`context_workers`；結束時拿掉，「已決定的事項」第 19 條），posted message 可以指定 `Worker` 當 target（`post_worker_object_message`）；`dom_api` 的 `WorkerGlobalHost::report_error`、`WorkerError`、`worker_error`（`Worker` 上可以取消的 `ErrorEvent`，`error` 是 null，沒被取消時在它的 realm 的 global 回報），結束了的 `Worker` 的訊息不分派（HTML：`terminate()` 清空它的 port 的佇列）；`ScriptEngine` 多了 `set_worker_host`、`worker_message`、`worker_error`、`worker_closed`、`worker_script_failed`（`V8Engine` 照 frame 的 window 的執行環境找 `Worker`）；worker 那一側沒被取消的例外交給 host。`ErrorEvent.error` 沒給時是 null（`AnyOrNull`；Blink 的 `ErrorEvent::error` 也是：WPT 的 `workers/Worker_ErrorEvent_error.htm` 要 null），`dom_js` 原本測 `new ErrorEvent('x').error === undefined` 的那一條改成 null。
- 5b（renderer）：`worker_thread.rs`（「已決定的事項」第 11 條）：每個 worker 一條 thread（`DedicatedWorker thread`）、自己的 engine（`ScriptEngineFactory::worker_engine`，shell 給 `dom_js::V8WorkerEngine`），一個 channel（訊息、結束），loop 等 channel 和 engine 的下一個 task；送給 `Worker` 的經過 main thread 的 scheduler 的新 queue（`TaskQueue::Worker`、`MainThreadTask::Worker`），render thread 交給 engine，例外和 script 拿不到之後送出 harness 的結果；`terminate()`（第 12 條）：設旗標、呼叫 engine 的 terminator、送結束的 task；`close()`：這個 task 之後 loop 結束、告訴 `Worker`；文件不在時（iframe 拿掉、frame 換文件、最上層換文件，`dom_js` 的 `frames::terminate_workers`）、換 engine、renderer 結束時結束 worker，renderer 結束時等每條 thread；主 script 只有 `data:` 的網址（`data-url`，origin 是 opaque 的），其他的是 `Worker` 上的 `error`（1b-6 經過 Browser 抓）。
- 新的測試：`shell` 的 `workers::`（6 個：兩個方向的訊息、例外到 `Worker` 和 window、`terminate()` 跑不停的 worker、`close()`、script 拿不到、renderer 結束時跑不停的 worker）；`dom_js` 的 `worker::a_worker_s_uncaught_errors_go_to_its_host`、`a_window_s_worker_hears_from_its_worker`、`a_terminated_worker_is_heard_no_more`、`a_document_s_workers_end_with_it`；`renderer` 的 `worker_thread::tests::the_renderer_waits_for_its_workers_threads`（假的 engine 放掉時要 100 ms：mutation test 沒抓到的那一條補的）。
- 找到的問題：`shell` 的「script 拿不到」的測試第一次通過、之後一直逾時：render thread 處理 worker 的 task 時派了事件，但沒有像其他跑 script 的地方那樣送出 harness 的結果；第一次通過是因為頁面載入完的那個 task 剛好在它後面、順便送了。改成處理例外和 script 拿不到之後送；測試改成在 `load` 之後才建 `Worker`，不靠時序。
- mutation test（`rfcs/0103-worker-global-scope/mutants-1b5.toml`）：18 條。第一次 13 條抓到、1 條卡住（`Worker` 送出的訊息變成結束：頁面一直等，算抓到）、1 條編不過（改寫之後卡住：打斷不了跑不停的 worker，renderer 結束時一直等它的 thread，算抓到）、3 條沒抓到：「renderer 不等 worker 的 thread」（補了上面的 `renderer` 的測試，重跑抓到）、「script 拿不到之後不送 harness 的結果」（測試照時序有時通過，改成 `load` 之後建 `Worker`，重跑抓到）、「照編號找 `Worker` 時結束了的也算」（等價：清單只有沒結束的，記進 `docs/test-gaps.md`）。
- WPT（`python3 scripts/test.py wpt html/webappapis/ dom/events/ webmessaging/ FileAPI/ netinfo/ geolocation/ screen-wake-lock/` 和預期結果提到 worker 的幾份）：3828 個檢查，和預期不同的 90 個：4 個是 1a 記下的 `scrollend`；86 個是 worker 的 variant（`.any.worker.html`、`Worker-read-file-constructor.worker.html` 等）從 ERROR 變成 TIMEOUT。原因：`Worker` 有了，但 http 的 script 還不抓（1b-6），`Worker` 上的 `error` 在 worker 送出第一個訊息之前到，testharness.js 留著它等 worker 開始（`RemoteContext::remote_error` 的 `early_exception`），一直等不到。這是 1b-6 之前的暫時狀態，預期結果在 1b-7 跑 worker 的 variant 時一起更新。
- 跑過的：`cargo test -p dom -p dom_api -p dom_js -p dom_wasm -p renderer -p shell -p browser` 都通過。

### 1b-6：worker 的 script 經過 Browser 載入、`importScripts()`（2026-11-17，macOS，Apple Silicon）

- `protocol`：`RequestDestination::Worker`；`FetchResource`、`CancelResource`、`FollowRedirect` 和回答的 `ResourceResponse`、`ReceivedRedirect` 帶 `worker`（哪個 dedicated worker 的，request id 是它自己的）；新的 `DedicatedWorkerCreated`（帶主 script 的請求）、`DedicatedWorkerGone`、`DedicatedWorkerScript`。
- Browser 的 IO thread（`render_process_host.rs`）：載入照 `LoadKey`（main thread 或 worker 的請求、worker 的主 script）記；`DedicatedWorkerCreated` 登記 worker、抓主 script（redirect 自己跟），之後才放行它的請求，不認得的 worker 的請求回答 `ERR_ACCESS_DENIED`；`DedicatedWorkerGone` 關掉它的載入、拿掉登記。
- renderer：`MainThreadScheduler` 的 worker 路由表（另一把鎖），`RendererSender::send` 把帶 worker 的回答和 `DedicatedWorkerScript` 直接送進 worker 的 thread 的 channel；`Workers` 啟動 worker 時先加路由、再告訴 Browser，`terminate()` 和不在時拿掉路由、告訴 Browser。worker 的 thread 先等主 script（等的期間 `Worker` 送來的訊息留到 script 跑完），檢查狀態和 MIME type（第 20 條，和 `<script>` 共用 `loader::is_blocked_as_script`；`data:`、`blob:` 不檢查，和 Chrome 的 `AllowedByNosniff` 一樣），照 Chrome 用 BOM、`charset`、UTF-8 解碼，global 的網址是 redirect 之後的。`data:` 的主 script 也經過 Browser（Chromium 一樣），`renderer` 不再依賴 `data-url`。
- `importScripts()`：`dom_api` 的成員（`#[calls_script]`，先解析全部的網址）、`ScriptHost::import_script`、`ImportError`、`WorkerGlobalHost::fetch_script`、`ImportedScript`、`ExceptionName::Network`（`dom_wasm` 的代碼 27）；`dom_js` 在目前的 realm 編譯、執行，例外用 `TryCatch::rethrow` 交給成員的呼叫者，muted 的換成 `NetworkError`，被打斷時再要求一次打斷（第 13 條）；renderer 的 `GlobalHost::load` 送 `FetchResource`、跟 redirect、等回答，其他的 task 留給 loop（`Inbox`），收到結束時送 `CancelResource`。
- 新的測試：`shell` 的 `workers::` 多了 6 個（主 script 和 import 的載入、redirect、別的 origin 的 import；跨 origin、redirect 到別的 origin、404、`image/`、`text/csv` 的主 script 是 `error`，`text/plain` 和 `data:image/png` 的照樣跑；`importScripts()` 的 `NetworkError` 和例外；import 等待中的訊息；import 等待中 terminate；主 script 還沒到就 terminate、Browser 關掉載入），`a_worker_whose_script_can_t_be_had_errs` 現在是 404；`dom_js` 的 `worker::` 多了 4 個（照順序跑、解析和載入失敗、例外、import 的 script 之後丟的錯照它的 muted 回報）。
- 找到的問題：import 等待中 terminate 的測試一開始不穩定（8 次失敗 6 次）：worker 送出訊息之後才送請求，頁面收到訊息就 terminate，`DedicatedWorkerGone` 有時先到 Browser，請求被當成不認得的 worker 的而擋下，沒到 server。實作是對的，測試改成收到訊息 200 ms 之後才 terminate，之後連跑 16 次都通過。
- mutation test（`rfcs/0103-worker-global-scope/mutants-1b6.toml`）：26 條。第一次 20 條抓到、6 條沒抓到：「import 的錯不再丟給呼叫者」（mutant 寫錯了，`?` 還在；改寫之後抓到）、「import 一律 muted」（補了 `an_imported_script_s_errors_are_its_own`，抓到）、「worker 不在時不關它的載入」和「terminate 時不告訴 Browser」（import 的載入 worker 自己會關，補了主 script 還沒到就 terminate 的測試，抓到；中間一次重跑「抓到」是上面不穩定的測試碰巧失敗）、「Browser 放行不認得的 worker 的請求」（只有不照規矩的 renderer 會送，記進 `docs/test-gaps.md` 的「只靠審查」）、「import 等待時聽不到結束」（等價：停下的旗標在送結束之前就設了，兩處都看旗標，記進 `docs/test-gaps.md`）。
- 跑過的：`cargo test -p renderer -p browser -p dom_api -p dom_js -p protocol -p net -p dom_wasm`、`cargo test -p shell` 都通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 都通過。WPT 的 worker 的 variant 和預期結果在 1b-7 跑。
