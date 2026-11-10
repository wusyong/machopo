# RFC 0093 的進度：wasm 文件接回 renderer：用 `dom_wasm` 做 TodoMVC

開始日期：2026-11-10，macOS（Apple Silicon）。在 `0093` 上做。

完成日期：2026-11-10，macOS（Apple M4）。

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a guest 和 `dom_wasm` 的測試換成 WASI 0.3 和 async 的 Store（決定 5、6） | 完成 | 2026-11-10 |
| 1b WASI 0.3 的 linker：自己的時鐘，stdout 和 stderr 交給 sink（決定 7）；一輪的 poll（決定 9） | 完成 | 2026-11-10 |
| 1c-1 engine 擁有的 DOM：`WasmDom::take`、`lend`（決定 10） | 完成 | 2026-11-10 |
| 1c-2 `dom_wasm` 的 engine：`ScriptEngine`、`ScriptHost`、需要時才跑的 event loop、`start(document)`（決定 3、4、8、9） | 完成 | 2026-11-10 |
| 1d renderer：prescan `<meta>`、選 engine、換頁時換 engine、載入 `application/wasm` 的 script（決定 2） | 完成 | 2026-11-10 |
| 1e 靜態的 TodoMVC 的 guest、情境、`shell` 的測試；第 1 段的檢查（決定 11） | 完成 | 2026-11-10 |
| 2a handle 的表指任何物件、記 interface（決定 12） | 完成 | 2026-11-10 |
| 2b WIT 的清單加 TodoMVC 用到的成員，`list<string>`、可省略的 dictionary（決定 13）；第 2 段的檢查 | 完成 | 2026-11-10 |
| 3a wasmtime 的 fork：set 的 Store 在 fiber 上接受同步的呼叫（決定 14） | 完成（fork 的 `ef4e892`） | 2026-11-10 |
| 3b listener：`add`/`remove`、`handle-event`、事件的成員（決定 15、16）；第 3 段的檢查 | 完成 | 2026-11-10 |
| 4 互動的 TodoMVC 的 guest、兩份情境（決定 17）；第 4 段的檢查 | 完成 | 2026-11-10 |

## 驗收

**TodoMVC**

| 項目 | 怎麼驗證 |
| --- | --- |
| wasm 的 TodoMVC 能新增、勾選、全部勾選、編輯（Enter、Escape、離開欄位、改成空的就刪除）、刪除、清除已完成、三種篩選 | `python3 scripts/test.py scenarios todomvc-app-wasm` |
| 同樣的步驟對 JS 版結果一樣 | `python3 scripts/test.py scenarios todomvc-app-js` |
| 兩份情境除了頁面以外逐行相同 | `cargo test -p shell --test it wasm_documents::the_todomvc_scenarios_of_both_apps_are_the_same_steps` |
| 在真的視窗裡也一樣 | `python3 scripts/test.py automation todomvc-app wasm-listeners wasm-document todomvc-wasm`（要顯示器和 GPU） |
| guest 畫的 TodoMVC 和同樣的 todo 寫死在 markup 裡畫出來逐像素相同，而且和沒有 todo 的 JS 版不同 | `cargo test -p shell --test it wasm_documents::the_wasm_todomvc_is_drawn_as_its_markup_written_out`；`python3 scripts/test.py scenarios todomvc-wasm` |
| JS 版載得起來（樣式表和七份 script） | `cargo test -p shell --test it sites::todomvc_loads_its_style_sheets_and_scripts` |

**listener 和事件**

| 項目 | 怎麼驗證 |
| --- | --- |
| 頁面分派的事件呼叫 guest 的 listener：capture 和 target、重複加的只有一次、每次分派都呼叫；兩個 script 各自在自己的 Store 裡被呼叫 | `cargo test -p dom_wasm --test it engine::the_pages_dispatch_calls_a_guests_listeners`、`engine::each_listener_is_called_in_its_own_store` |
| guest 自己的 `click()` 在回來之前照順序呼叫它的 listener，不呼叫別的 script 的 | `cargo test -p dom_wasm --test it engine::a_guests_own_dispatch` |
| `preventDefault()`、`stopPropagation()`、`remove` 要同樣的 callback 和 capture | `cargo test -p dom_wasm --test it engine::a_listener` |
| listener 的 trap：Store 結束、其他 script 照常；在 guest 自己的分派裡 trap 時 `start` 跟著結束、回報的是原本的 trap | `cargo test -p dom_wasm --test it engine::a_listener_that_traps` |
| listener 裡再 `click()`（頁面的分派裡巢狀的分派），它印的一行到 console | `cargo test -p dom_wasm --test it engine::a_listener_dispatches_within_the_pages_dispatch` |
| 事件的 handle 只在那次呼叫裡是 guest 的（兩條路各一個） | `cargo test -p dom_wasm --test it engine::an_events_handle` |
| 真的滑鼠點擊走 capture、target、bubble 三個 listener（帶 `button`、`detail`），按鍵的 `keydown` 帶 `key` | `python3 scripts/test.py scenarios wasm-listeners` |
| fork：set 的 Store 在 fiber 上接受同步的呼叫（巢狀三層），不在 set 裡的照舊被拒絕 | 在 `../third_party/wasmtime`：`cargo test --features gc-elise --test all elise::` |

**wasm 文件和 engine**

| 項目 | 怎麼驗證 |
| --- | --- |
| wasm 文件跑 wasm、不跑 JS，guest 加的東西排版出來；在 wasm 文件和 JS 文件之間換頁（連結、網址）各跑各的 script；等待的 guest 接著跑 | `python3 scripts/test.py scenarios wasm-document` |
| script 跑了而且畫出來；MIME 不對和不存在的是 failed subresource；沒宣告的文件不抓也不跑 wasm；wasm 文件不抓也不跑 JS；`focus()`、`blur()` 的畫面 | `cargo test -p shell --test it wasm_documents` |
| `<meta name="script" content="wasm">` 的 prescan | `cargo test -p renderer --lib a_meta_declares` |
| `start` 拿到頁面的 document；每個 script 自己的 Store；等待的 script 到期才接著跑；trap 和不是 component 的 bytes 報到 console；guest 印的到 console；新的 window 丟掉前一份文件的 script；讓出的 guest 同一輪跑完；window 和 location | `cargo test -p dom_wasm --test it engine` |
| engine 擁有的 DOM：借出期間 guest 才碰得到、engine 手上的 full GC 留著 guest 拿著的東西、unwind 也交還 | `cargo test -p dom_wasm --test it lend` |

**handle、WIT 和 WASI**

| 項目 | 怎麼驗證 |
| --- | --- |
| handle 指任何物件、記 interface，`this` 的檢查，過時的 word 不算數 | `cargo test -p dom_wasm --lib` |
| 加進 WIT 的成員（`value`、`checked`、`classList`、`style`、集合、不是節點的 handle 當節點是 TypeError） | `cargo test -p dom_wasm --test it guest` |
| WIT 的 golden 和清單的規則 | `cargo test -p dom_wasm --test it wit` |
| 時鐘的等待是期限、到期才醒、降到 100 µs；stdout 和 stderr 一行一行給 sink | `cargo test -p dom_wasm --test it wasi` |

## 做過的檢查

都在 2026-11-10、macOS（Apple Silicon）上。

**開始前的原型和工具鏈**

- 重入和等待的原型（`reenter-prototype/`，結果在 `details.md` 的「重入的原型」）：手寫的 WAT component，wasmtime 的 fork 50.0.1。
- wasi-sdk 34.0（macOS arm64，release 的 sha256 核對過）放在 `../third_party/wasi-sdk-34.0`，`wit-bindgen` 0.62.0；`scripts/ci.py` 的 guest 的 target 改成 `wasm32-wasip3`。

**實作中確認的事**

- stdout、stderr 沒有 tokio 的 runtime 也能用（guest 的 `printf` 經過 wasmtime-wasi 的 p3 到我們的 `AsyncWrite`）。
- 等待中的 guest 留在 Store 裡，經過 `StoreSet` 的借出和 elise 的 collector 也一樣；`run_concurrent` 的 future 丟掉之後，下一輪接著跑它。
- wasmtime 只 poll 被叫醒的 host 函式的 future：`GuestClock` 要留著 waker、到期時叫醒（`wake_due`）。
- C 的 guest 的 `async func` 入口照 callback ABI 寫得出來；讓出（`YIELD`）時同一輪接著跑完（決定 9 提到的 wasmtime 49 的 bug 照這個做法碰不到）。
- guest 的呼叫在 fiber 上跑時，GC 經過 Store、`StoreSet` 交出別的 Store 的 root。

**找到的規則和 bug**

- `StoreSet` 和每個 Store 要在 DOM 借出期間丟掉；拿著 DOM 的 root 的人要比 engine 先 drop（否則 elise panic：`roots outlive their heap`）。寫在 `crates/dom_wasm/README.md`。
- `sanity.py boundary` 抓到：wasmtime 的 `Engine` 和 linker 不能放在 process 共用的 `static`，改成每個 `WasmEngine` 自己的。
- 接上 renderer 才發現的兩件事，補了：連結點了沒反應（wasm 的 host 沒有把 `ScriptHost::navigate` 變成 frame request）；等待的 guest 時間到了不會醒（renderer 跑的是 `run_due_tasks`，不是 `run_timers`）。
- guest 的操作沒有帶頁面的 layout（`focus()` 在 renderer 裡沒有效果，`shell` 的畫面測試抓到）：加了 `WasmDom::lend_page`，借 DOM 時把 layout 一起借。
- release 時清不掉物件的 engine word（手上只有 `Gc<Erased>`）：改成不清、用的時候對過才算數。
- wit-bindgen 的 C 名字撞名兩處：`event-target` 的函式和 `EventTarget` 的型別別名（別名不產生，簽名寫 `object`）；world 自己 `use` 的 `event` 和 async runtime 的 `script_event_t`（`handle-event` 的參數寫 `object`）。
- 一個物件在一個 Store 裡只有一個 handle：TodoMVC 的 guest「用完就 `release`」會放掉外層還拿著的 handle（勾選時 trap）。guest 改成最外層的 export 回來時一起放。
- 照現在的行為寫進測試、沒有改的：`click()` 的事件的 `detail` 是 1（Chrome 是 0；DOM 那一層的行為，JS 文件也一樣）。

**不穩定的測試**

- TodoMVC 的像素比對在 `cargo test` 裡偶爾失敗（三次裡一次）。不同的像素都在 `label` 的文字上；「靜態頁對靜態頁」六次有三次不同、「wasm 頁對 wasm 頁」六次都一樣，所以不穩的是靜態頁自己。原因是 CSS 給 `label` 的 `transition: color 0.4s`：靜態頁的 `label` 在樣式表到之前被算過一次 style。兩個測試頁都關掉它之後，整組連跑 40 次沒有失敗。什麼時候會先算那一次沒有查，記在 `docs/limitations.md`。沒有用重跑蓋過去。

**mutation test**（spec 在這個資料夾，跑完都確認檔案還原）

| spec | 條數 | 結果 |
| --- | --- | --- |
| `mutants-1b.toml`（時鐘、輸出、一輪的 poll） | 20 | 第一次 16 條抓到、1 條卡住、3 條沒抓到；補了五個單元測試之後 20 條都抓到 |
| `mutants-1c-1.toml`（DOM 的借出） | 5 | 第一次 1 條沒抓到（`Slot::of` 不看 DOM 在不在）；補了一個測試之後都抓到 |
| `mutants-1c-2.toml`（engine） | 18 | 17 條抓到；沒抓到：trap 的 Store 沒有從 `StoreSet` 拿掉（留到換頁才丟，從外面看不出來，記在 `docs/test-gaps.md`） |
| `mutants-1d.toml`（renderer） | 13 | 都抓到 |
| `mutants-2a.toml`（handle 的表） | 11 | 都抓到 |
| `mutants-2b.toml`（加的成員、layout 的借出） | 12 | 都抓到 |
| `mutants-3b.toml`（listener） | 15 | 14 條抓到；第一次有一條沒抓到（trap 之後繼續呼叫 listener），補了斷言之後抓到；沒抓到：trap 的 listener 的 Store 留在 set 裡（同上） |
| `mutants-4.toml`（guest 的 `blur()` 呼叫 listener，測試是情境） | 1 | 抓到 |

- 沒有跑 mutation test 的步驟：1a（只改 harness 和 guest 的 target）、1e（沒有新的 Rust 實作）、3a（改的是 fork，由 fork 的測試守著）。
- 程式排版或改寫之後 spec 的原文對不上的，改了原文、重跑那幾條。

**沒有測試守著的**（都記在 `docs/test-gaps.md`）

- trap 之後 Store 從 `StoreSet` 拿掉；listener 等待時當成 trap（走不到）；guest 的分派裡再分派、兩個 Store 互相分派、很深的巢狀；wasm script 的 `load`、`error`；wasm 文件裡的捲動、滾輪、動畫、WebDriver 的輸入；DevTools 連著時換 engine。
- `document-set-body` 沒有 case；讀的那一側開操作時帶 layout 沒有成員走到。

**fork 的測試**（3a；fork 沒有 toolchain 檔，用 `cargo +1.99.0`）

- `cargo test --features gc-elise --test all elise::`：39 個通過（含新的兩個）；`--test all -- async_functions::`：24 個通過；`python3 machopo/elise_tests.py` 的 `elise` 和 `elise-stress` 各 228 個通過。
- 沒有跑：spec test（這台沒有 `tests/spec_testsuite` 的 submodule）、fork 的 clippy、miri、fuzz。

**每一段的檢查**

| 段 | `cargo test --workspace` | 其他 |
| --- | --- | --- |
| 1 | 55 個 binary，3789 個通過、1 個 ignored | fmt 排過 30 處；clippy 抓到兩種（時鐘的減法沒寫明溢位、測試裡的 `fetch_add`），修了 |
| 2 | 3796 個通過、1 個 ignored | fmt 排過 9 個檔；clippy 抓到一處多餘的 lifetime，修了 |
| 3 | 3808 個通過、1 個 ignored | fmt、clippy 乾淨 |
| 4 | 3809 個通過、1 個 ignored | fmt、clippy 乾淨；`test.py scenarios todomvc wasm` 8 次都通過；`test.py automation`（TodoMVC 兩版和三份 wasm 的情境）5 份都通過 |

- 每一段 `sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 都通過。
- 沒有跑的：整套的情境、reftest、WPT、`wdspec`、`matrix`、`fuzz`（由 CI 跑）。這份沒有改 elise。
- 視窗裡看得到的行為都寫成情境，沒有請人看的項目；使用者自己在視窗裡跑過 `todomvc-app-wasm.html`（2026-11-10），輸出沒有 trap。
- 過程中的失誤（第 4 段）：誤下 `git checkout` 還原了還沒 commit 的 `todomvc.c`，照原本的內容重寫回來；之後的測試都是對重寫後的版本跑的。
