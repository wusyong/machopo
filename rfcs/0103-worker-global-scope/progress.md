# RFC 0103 的進度：worker 的執行環境

開始日期：2026-11-17。完成日期：2026-11-18（macOS，Apple Silicon）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 的原型：`#[window]` 的成員拿得到 window、其他成員拿不到，exposed 寫在表上 | 做完（程式留著，隨 1a 定下來） | 2026-11-17 |
| 1a：執行環境從 `Window` 分出來、成員改成拿執行環境、global 照 exposed 建 | 做完，merge 回 `main` | 2026-11-17 |
| 1b：dedicated worker | 做完（1b-1 到 1b-7），merge 回 `main` | 2026-11-18 |
| 1b-1：被打斷的 script 不被 `dom_js` 回頭呼叫 script 的地方吞掉 | 做完（「已決定的事項」第 13 條） | 2026-11-17 |
| 1b-2：`dom` 的 worker 的 global（自己的執行環境、event target），執行環境的 timer 和 task | 做完（第 14、15 條） | 2026-11-17 |
| 1b-3：`dom_api` 的 `Worker`、`WorkerGlobalScope`、`DedicatedWorkerGlobalScope` 和 worker 也有的 timer 等成員 | 做完（第 16、17 條） | 2026-11-17 |
| 1b-4：worker 的 engine（`dom_api` 的 trait、`dom_js` 的實作，global 照 exposed 建） | 做完（第 18 條） | 2026-11-17 |
| 1b-5：`renderer` 的 worker 的 thread 和 event loop、兩個方向的 `postMessage`、`terminate()`、`close()`、`onerror` | 做完（第 19 條） | 2026-11-17 |
| 1b-6：Browser 的登記和主 script 的載入、子資源的請求帶 client、`importScripts()` | 做完（第 20 條） | 2026-11-17 |
| 1b-7：WPT 的 `workers/`、情境、`about:demo`、文件、整個階段的檢查 | 做完（第 21、22 條） | 2026-11-18 |
| 2：worker 裡的 `fetch()`、`MessagePort` 跨 thread、module worker、巢狀的 worker、`WorkerNavigator`、`WorkerLocation`、`performance` | 做完（2a 到 2f），merge 回 `main` | 2026-11-18 |
| 2a：worker 裡的 `location`、`navigator`、`performance` | 做完（第 23 到 26 條） | 2026-11-18 |
| 2b：worker 裡的 `fetch()`、worker 建的 blob URL | 做完（第 27、28 條） | 2026-11-18 |
| 2c：`MessagePort` 跨 thread | 做完（第 29 到 31 條） | 2026-11-18 |
| 2d：worker 裡建的 worker | 做完（第 32 到 34 條） | 2026-11-18 |
| 2e：module worker | 做完（第 35 到 37 條） | 2026-11-18 |
| 2f：WPT、文件、整個階段的檢查 | 做完（第 38 條） | 2026-11-18 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 宣告表 | `#[window]` 只能寫在只有 `Window` 的 interface 上，產生的程式把 window 交給函式；interface 和成員的 `#[exposed]`（成員只能比 interface 少、overload 一樣） | `cargo test -p dom_api_macros` |
| 宣告表 | 每個 interface 和成員的 exposed 和規範一致（WPT 的 `interfaces/*.idl`）；dedicated worker 的 realm 只有它有的 interface 和成員；`WorkerGlobalScope` 的成員在它的 prototype 上，`DedicatedWorkerGlobalScope` 的是 global 自己的 | `cargo test -p dom_api --test it table::`；`cargo test -p dom_js --test it worker::a_worker_s_global_is_its_realm_s`、`a_worker_s_realm_has_the_interfaces_exposed_there`、`a_worker_s_realm_has_only_the_members_exposed_there` |
| 執行環境 | time origin、timer、task、訊息、port、`fetch()`、blob、performance 從 `Window` 搬到 `ExecutionContext` 之後行為不變；worker 也有的物件的事件從它的 realm 的 time origin 算；worker 的 global 回答自己的 URL、base URL、origin，它的 timer、task、訊息和同一個 `Dom` 裡的 window 分開 | `cargo test -p dom -p dom_api -p dom_js -p dom_wasm`（`dom` 的 `worker_global::`、`tasks::`、`events::every_target_holds_listeners`，`dom_js` 的 `engine::a_signal_s_event_counts_from_its_realm_s_origin`）；WPT 的相關目錄 |
| 打斷 script | 從別的 thread 打斷的 script 在 DOM 回頭呼叫的每種 callback 裡停下，呼叫它的 script 也停下、之後的一行都不跑，不回報錯誤；下一個 task 的 script 照常跑 | `cargo test -p dom_js --test it termination::`、`cargo test -p bindings --test it termination::`；debug 的 V8 加上 `--strict-termination-checks` 跑同樣的測試 |
| `Worker` 和 worker 的 global | `new Worker()` 的網址、名稱、`type`、`credentials`，錯的值和沒有 host 時的例外；`postMessage()`、`terminate()` 當場經過 host；worker 的 global 的 timer、`name`、`self`、`crypto`、`location`、`navigator`、`performance`（time origin 是 `new Worker()` 的時刻）、`postMessage()`、`close()`、`onerror`、`importScripts()` | `cargo test -p dom_api --test it worker::`、`cargo test -p dom --test it worker::`、`cargo test -p dom_js --test it worker::` |
| thread 和結束 | 每個 worker 一條 thread：兩個方向的訊息、例外到 `Worker` 和 window（沒被取消時）、`terminate()` 停得了跑不停的、`close()` 之後沒有、文件不在時結束、renderer 結束時等每條 thread；忙的 worker 不擋住頁面 | `cargo test -p shell --test it workers::`、`cargo test -p renderer --lib worker_thread`、`python3 scripts/test.py scenarios demo-worker`（和 `--window`） |
| 載入 | 主 script 經過 Browser（同源、redirect 之後的網址是 global 的、照 Chrome 寬鬆地檢查 MIME type 和 `nosniff`、狀態不是 2xx 或不能 parse 是 `Worker` 上的 `error`）；`importScripts()` 照順序同步載入、執行（`no-cors`，muted 的錯誤是 `NetworkError`）；worker 不在時 Browser 關掉它的載入 | `cargo test -p shell --test it workers::`、`cargo test -p dom_js --test it worker::` |
| `fetch()` 和 blob URL | worker 的 `fetch()` 的請求（自己的 origin、CORS、`no-cors`、redirect、abort），回應在 worker 的 task 裡 settle；worker 建的 blob URL 給它自己和頁面的載入，worker 結束時撤銷 | `cargo test -p dom_js --test it worker::a_worker_s_fetch_goes_to_its_host`、`a_worker_s_blob_urls_go_to_its_host`；`cargo test -p shell --test it workers::a_worker_s_fetch_and_blob_urls`、`a_worker_s_aborted_fetch_has_its_load_closed`、`a_worker_s_fetch_is_redirected_and_answered_while_it_imports` |
| `MessagePort` | port 帶著還沒送的訊息到另一條 thread，之後的訊息叫醒收的 thread、照那邊的順序編號；一端被放掉（`close()`、被 GC 收掉、DOM 不在、它所在的訊息沒送到）時另一端收完之前到的就放掉；doomed；port 進出 worker，兩個 worker 不經過 main thread 互傳；`structuredClone` 的 transfer | `cargo test -p dom --test it message_ports::`、`cargo test -p dom_js --test it messages::`、`cargo test -p shell --test it workers::ports_go_to_and_from_a_worker`、`two_workers_talk_through_a_port_while_the_page_is_busy`、`a_port_s_message_from_a_worker_wakes_the_page` |
| worker 裡建的 worker | 父 worker 的 thread 收子 worker 的事件（頁面忙的時候照樣）；例外一層一層往上報；父 worker `terminate()` 或 `close()` 時子 worker 跟著結束；renderer 結束時不卡住 | `cargo test -p shell --test it workers::a_worker_s_worker_talks_to_it_while_the_page_is_busy`、`a_worker_s_worker_s_error_goes_up`、`a_worker_s_workers_end_with_it`、`a_busy_worker_s_worker_ends_with_the_renderer`、`a_worker_and_its_worker_talk_through_a_port` |
| module worker 和 `import()` | module worker 的根由 Browser 同源載入、照 module 檢查，它 import 的照建它的一方的 origin 抓，graph 好了才跑；抓不到、不能 parse、根不是 JavaScript 或別的 origin 是 `error`；module worker 的 `importScripts()` 是 `TypeError`；classic 和 module worker 的 `import()`（muted 的 script 照 `about:blank` 解析） | `cargo test -p dom_api --lib module::`；`cargo test -p dom_js --test it worker::a_module_worker_runs_its_graph`、`a_module_worker_s_graph_that_can_t_run`、`a_worker_imports_modules`、`scripts::a_muted_scripts_import_has_no_base_url`；`cargo test -p shell --test it workers::a_module_worker_s_graph_is_fetched_and_run`、`a_module_worker_whose_graph_fails_errs`、`a_worker_imports_a_module`、`a_data_module_worker_imports_for_its_page` |
| WPT | `workers/`（shared、service worker 之外）、`webmessaging/` 的 channel 和 port、清單裡每個目錄的 worker variant 照預期結果 | `python3 scripts/test.py wpt workers/ webmessaging/` 和清單裡有 worker variant 的 114 項 |

## 做過的檢查

都在 macOS（Apple Silicon）上，2026-11-17 到 11-18。

### 原型

- **1a 的原型**（`details.md` 的「2A 的原型」）：只有 `O: OpFront` 的函式呼叫 `<_ as Front>::op_window(op, cx)` 是 `E0283`，`#[window]` 放錯是表的編譯錯誤；程式留著，隨 1a 定下來。
- **1b 的原型**（`details.md` 的「1b 的原型：打斷 script」）：從別的 thread 打斷正在呼叫 DOM 的 script 不會 crash，engine、heap、isolate 照「已決定的事項」第 12 條的順序放得掉；但 `dom_js` 回頭呼叫 script 的地方會把打斷當成一般的例外吞掉，成了 1b-1。
- **2f 開始前**：拿掉 `WorkerGlobalScope` 的 `#[global]`，`dom_js`、`dom_api` 的測試只有寫著舊行為的那一個失敗，成了第 38 條。

### 測試和 mutation test

每一段的 mutation spec 是這個資料夾的 `mutants-<段>.toml`（`python3 scripts/test.py mutate rfcs/0103-worker-global-scope/mutants-<段>.toml`）。沒抓到、記進 `docs/test-gaps.md` 的，在那裡寫了為什麼。

| 段 | 條數 | 第一次沒抓到的，和怎麼處理 |
| --- | ---: | --- |
| 1a | 17 | 「從 abort signal 找不到它的執行環境」：補 `engine::a_signal_s_event_counts_from_its_realm_s_origin`，抓到 |
| 1b-1 | 21（三輪） | 從 task 直接呼叫的 callback 的入口檢查走不到，拿掉；巢狀的 7 道沒有測試走到，補測試時找到 V8 會清掉打斷（第 13 條）；剩下 14 條：回頭呼叫的入口的 7 道等價，`stop_again` 的 3 條、node filter、`DOMException` 前的檢查、JSON 的 3 條記進 test-gaps 的「能補、還沒補」 |
| 1b-2 | 10 | idle callback 的 timeout、讀回的 worker 的 global 補了測試；發給 global 自己的訊息的 target 到 1b-4 用 `dom_js` 的測試抓到 |
| 1b-3 | 21 | 都抓到 |
| 1b-4 | 14 | 「script 之後不做 microtask checkpoint」補測試；「worker 的 global 當成參數時是一般的 target」等價 |
| 1b-5 | 18 | 「renderer 不等 worker 的 thread」補 `renderer` 的測試；「script 拿不到之後不送 harness 的結果」測試改成不靠時序；「照編號找 `Worker` 時結束了的也算」等價 |
| 1b-6 | 26 | 兩條 mutant 寫錯或測試不足，改寫、補測試；「Browser 放行不認得的 worker 的請求」只靠審查；「import 等待時聽不到結束」等價 |
| 1b-7 | 13 | 「排錯誤時不看已經結束」是多餘的檢查，拿掉那段程式和那條 |
| 2a | 21 | 「renderer 太晚記 time origin」：測試改用慢 300 ms 的 script |
| 2b | 14 | 「renderer 不取消 worker 中止的 `fetch()`」：頁面待到 server 本來會回應之後才結束；「Browser 不收不認得的 worker 的登記」只靠審查 |
| 2c | 19 | 同一條 thread 上的 port 訊息的編號、main thread 沒被叫醒（兩條）補測試；「port 傳出去時不清掉原本的 thread」等價 |
| 2d | 10 | 都抓到（「worker 結束時不結束它建的 worker」是卡住，算抓到） |
| 2e | 16 | 「module worker 的根用 CORS 抓」：補跨源的根 |
| 2f | 5 | 「祖先的成員全部裝在每個物件上」：改用整個 `dom_js` 的測試，抓到 |

新的測試的名字見上面的驗收表；每一段的細節在 git 的歷史（`RFC 0103, <段>` 的 commit）裡。

### 找到的 bug

- **1b-1**：V8 會跑 script 的 API（呼叫函式、建 wrapper、reject promise、`JSON.parse`）一進來就清掉正在進行的打斷，`fetch` 被 abort 時 reject 它的 promise 讓外層的 script 繼續跑（第 13 條）。
- **WPT 找到的**（都修了，加了測試）：
  - worker 的 script 不能 parse 時 `Worker` 上是一般的 `Event`（原本是 `ErrorEvent`，window 也收到）；worker 送來的錯誤排在它之前送的訊息之後；沒有值的錯誤的 `error` 是 `null`；transfer list 的項目不是物件時是 `TypeError`；`name` 和 `performance` 是 `[Replaceable]`；`nosniff`；
  - worker 裡讀 `performance.timing` 讓 machopo 結束（成員的 exposed，第 26 條）；worker 的 `onerror` 自己丟的例外也要往外報；worker 裡 User Timing 的名稱的兩個例外；
  - `Worker` 物件上沒被取消的 `error` 在 worker 的 realm 裡沒有回報到它的 global、也沒往上傳；
  - module worker 的根用 CORS 抓，跨源的 module worker 載入成功；module worker 的靜態 import 照 worker 自己的（opaque 的）origin 抓；
  - `WorkerGlobalScope` 的成員是 global 自己的 property（第 38 條）；muted 的 classic script 裡的 `import()` 照它自己的網址解析（HTML 是 `about:blank`）。
- **1a 的 `cargo test --workspace`**：`shell` 的 `devtools_initiators::script_asks_with_its_stack` 在 branch 上失敗：renderer 的 loop 一輪跑完 channel 上的 task 才清掉沒用到的 stack，`again()` 和 `anew()` 在同一輪時「先記的贏」。選 C：測試等一個 animation frame，`docs/limitations.md` 記下。

### 不穩定的測試

- 查過、標成 known intermittent 的 WPT（原因寫在各自的 `.ini`）：`workers/interfaces/WorkerUtils/importScripts/report-error-*.sub.any.worker.html`（6 份，頁面送結果和錯誤傳到 window 的先後）；`fetch/api/basic/keepalive` 的 worker variant（worker `close()` 時 Browser 關掉它的載入，請求有沒有先到 server 看時間）；`dynamic-import/microtasks/basic.any.js` 的 window 和 worker variant（兩個測試在同一毫秒用同一個 `Date.now()` 當 module 的網址）。
- 測試本身的時序問題（已經改掉）：import 等待中 terminate 的測試（`DedicatedWorkerGone` 比請求先到 Browser）；port 的測試照某個順序寫（不同 port 的訊息規範不保證先後）。
- 和這份無關的：`intersection-observer/v2/delay-test.html` 在二十個目錄平行跑時失敗一次；`browser` 的 `navigation::a_srcdoc_document_commits_with_nothing_else_to_wake_the_renderer` 在負載重時超過 5 秒的期限（RFC 0102 記下的那一個）。Mac 上 `dom/events/scrolling/` 的 4 個 `scrollend` 一直是意外通過（測試在 `navigator.platform` 有 `MAC` 時直接 return，預期結果是 Linux 的），每次重寫預期結果時被改到的都還原。

### WPT

- 1b-7 加整個 `workers/`（shared、service worker 和當時還沒有的 module worker、巢狀的 worker、cookie `disabled`），清單裡 20 個目錄的 worker variant 重寫預期結果，原本「machopo has no workers」的註解改成真正的原因，92 份預期結果拿掉；2a 到 2f 各自把等它們的 `disabled` 拿掉、重寫預期結果。2c 加 `webmessaging/` 的 channel 和 port（`close-event/` `disabled`：`MessagePort` 的 `close` 事件 Chrome 還在 flag 後面）。2f 把之前一起標成「needs what machopo doesn't have (postMessage, workers, crypto...)」的 15 份重跑：7 份通過，其他的寫上真正的原因。
- 照 Chrome、和 WPT 不同的（記在預期結果和 `docs/limitations.md`）：worker 的 script 寬鬆地檢查 MIME type、照 `charset` 解碼。
- 最後一次（2f）：清單裡有 worker variant 的 114 項（18565 項檢查）不更新地跑，沒有新的失敗；`workers/`、`webmessaging/`、`html/semantics/scripting-1/the-script-element/` 不更新地連跑兩次（3103 項檢查），除了上面的 known intermittent 都和預期一樣。

### 跑過的檢查

- 每一段：改到的 crate 的 `cargo test`、`shell` 的 `workers::`、`python3 scripts/sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
- 每個階段做完（1a、1b-7、2f）：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（2f：56 個測試 binary 都通過）；1b-7 和 2f 跑了 `test.py scenarios --window`（2f：69 個情境都通過，`demo-worker` 的截圖正常）。clippy 找到的都修了（回傳型別包成型別或別名、`WorkerTask::Browser` 用 `Box`、worker 的編號用 `base::AtomicSequenceNumber`、`loader.rs` 的函式搬到 test module 之前）。
- debug 的 V8（1b-1）：`V8_FORCE_DEBUG=1 CARGO_TARGET_DIR=$PWD/target/v8-debug MACHOPO_V8_FLAGS=--strict-termination-checks cargo test -p dom_js --test it termination::` 通過。
- 沒有在開發機上跑的：`test.py matrix`、`fuzz`（這份動到 DOM 的 GC 型別、每個 worker 一個 heap，merge 並 push 之後在 Actions 上手動開始 `gc.yml`，看它的結果）、整套的 `wpt`、`wdspec`、`reftest`（每天的 CI）。
