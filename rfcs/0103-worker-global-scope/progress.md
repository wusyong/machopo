# RFC 0103 的進度：worker 的執行環境

開始日期：2026-11-17。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 的原型：`#[window]` 的成員拿得到 window、其他成員拿不到，exposed 寫在表上 | 做完（程式留著，隨 1a 定下來） | 2026-11-17 |
| 1a：執行環境從 `Window` 分出來、成員改成拿執行環境、global 照 exposed 建 | 做完（還沒 merge 回 `main`） | 2026-11-17 |
| 1b：dedicated worker | 還沒開始 | |
| 2：worker 裡的 `fetch()`、`MessagePort` 跨 thread、module worker | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 1a：`#[window]` 只能寫在只有 `Window` 的 interface 上，產生的程式把 window 交給函式；`#[exposed]` 的解析 | `cargo test -p dom_api_macros` |
| 1a：每個 interface 的 exposed 和規範一致（WPT 的 `interfaces/*.idl`） | `cargo test -p dom_api --test it table::interfaces_are_exposed_in_their_globals` |
| 1a：執行環境（time origin、timer、task、訊息、port、`fetch()`、blob、performance）在 `Window` 和成員之間行為不變 | `cargo test -p dom -p dom_api -p dom_js -p dom_wasm`；WPT 的相關目錄（「做過的檢查」的 1a 那幾段） |
| 1a：worker 也有的物件的事件從它的 realm 的 time origin 算 | `cargo test -p dom_js --test it engine::` |

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
