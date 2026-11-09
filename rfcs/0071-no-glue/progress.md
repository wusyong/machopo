# RFC 0071 的進度：拿掉 `glue.js`

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-22。完成：2026-10-22，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0071` 上做，四段各一個 commit；rusty_v8 fork 多了一個 commit（`Isolate::set_is_js_api_wrapper_native_error_callback`，`0212ab6`），發了 `kun-v152.2.0-16`，kun 換成它（第 1 段到第 2 段連結本機重編的 V8，第 3 段起用 release 的 prebuilt）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `DOMException`、`QuotaExceededError`：script object、`[[Prototype]]`、平台建的有 `stack`、structured clone、`Error.isError`（第 1 到 4、8 到 12 條） | 完成 | 2026-10-22 |
| 2 `addEventListener`、`removeEventListener`、捲動：union、overload、promise；`kun:dom` 0.15（第 5、13 到 15 條） | 完成 | 2026-10-22 |
| 3 計時器：額外參數、`this`、例外的 realm、字串的 handler（第 6、16 到 18 條） | 完成 | 2026-10-22 |
| 4 `IdleDeadline`，拿掉 `glue.js`：建 realm 只建 `Window` 和 `EventTarget`（第 19 到 22 條） | 完成 | 2026-10-22 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| `DOMException` | `DOMException`、`QuotaExceededError` 照 WebIDL：建構子、`name`、`message`、`code`、常數、`toString`、要 `new`、getter 檢查 `this`；`QuotaExceededError` 的 quota、requested（照 Chrome 不檢查負數） | `cargo test -p dom_js --test it -- bindings::dom_exception_follows_webidl crypto::quota_exceeded_error_is_chromes`；WPT 的 `webidl/ecmascript-binding/es-exceptions/` |
| | 平台建的（丟出的、abort 的 reason）有 `stack`，`new` 建的、clone 出來的沒有 | `bindings::a_dom_exception_has_a_stack_if_the_platform_made_it` |
| | structured clone：名字、訊息、quota、requested，子類別讀回來是 `DOMException`，存起來的（`history.state`）也是；其他 platform object 照舊是 `DataCloneError` | `bindings::a_dom_exception_clones` |
| | `DOMException.prototype` 的 `[[Prototype]]` 是 realm 的 `%Error.prototype%`，先建 wrapper、頁面換掉 `Error` 也一樣；`Error.isError` 對它們是 `true` | `bindings::dom_exception_extends_the_realms_error_prototype`、`bindings::a_dom_exception_is_an_error` |
| 監聽和捲動 | `addEventListener`、`removeEventListener` 的 `options` 是 `(… or boolean)`，boolean 是 `capture`；`length` 2；捲動的方法的 `(options)` 和 `(x, y)`，回傳 resolve 的 promise、錯誤是 rejection、`length` 0；`scrollIntoView` 的 boolean；這些成員是 native function，`addEventListener` 走專用路徑 | `cargo test -p dom_js --test it -- bindings::add_event_listener_options bindings::listener_scrolling_and_timer_members_are_native bindings::typed_paths_skip_the_shared_callback geometry::scrolling_from_script geometry::scroll_into_view_from_script`；`cargo test -p dom_api --test it -- geometry::` |
| | `kun:dom` 0.15：`options` 是 variant、捲動是 `async func`、`*-xy` 的 overload；C、Rust、Lua 的 guest 照它重編，Lua 的 boolean 的 options | `cargo test -p dom_api --test it -- wit::`；`cargo test -p shell --test it -- wasm_reftests::`（`w9-lua`）；`cargo test -p dom_wasm` |
| 計時器 | function 以 window 為 `this`、帶 timeout 之後的參數（`undefined` 也算）；字串（和轉成字串的值）是沒有網址的 classic script，錯誤的 `filename` 是 document 的、`import()` 照 document 的 base URL，`setInterval` 每次重新執行；別的 frame 的 function 的例外在它的 frame 回報；`setTimeout`、`setInterval` 是 native function、`length` 1、檢查 `this` | `cargo test -p shell --test it -- script::timer_handlers_run_as_chromes`（預期值是同一頁在 Chrome 152 跑的結果）；`bindings::the_globals_timer_members`；WPT 的 `html/webappapis/timers/`、`html/semantics/scripting-1/the-script-element/module/dynamic-import/` |
| `IdleDeadline` | interface 的形狀、不能建構、callback 拿到的（`this` 是 `undefined`、一個參數）、成員檢查 `this`；`timeRemaining()` 在 100 µs 的格子上、照 window 的時鐘倒數、過了期限或 timeout 叫的是 0 | `bindings::idle_deadline_is_chromes`、`bindings::idle_time_remaining_is_on_the_grid`；`cargo test -p browser --test it -- idle_callbacks`；WPT 的 `requestidlecallback/` |
| | 別的 frame 的 function 拿到那個 frame 的 `IdleDeadline`；晚建的 iframe 的期限照它自己的 time origin | `cargo test -p shell --test it -- script::an_idle_deadline_is_the_callbacks`（Chrome 152 上跑過同一頁） |
| 膠水 | 沒有 `glue.js`；建 realm 只建 `Window` 和 `EventTarget` 的 interface object；頁面換掉內建函式不影響以前經過膠水的成員 | `cargo test -p dom_js --test it -- interface_objects:: bindings::the_bindings_resist_a_page_replacing_builtins` |
| 宣告表 | interface 可以有好幾個標記，只有 `DOMException` 是 `[ExceptionPrototype]`；三種 script object 和其他的一樣（slot、unwrap 只成自己）；`spec` 測試和 WebIDL 比對（計時器和 `IdleDeadline` 和規範一樣了） | `cargo test -p dom_api --test it -- api:: spec::`；`cargo test -p dom --test it -- script::every_type_unwraps_only_as_itself script_objects::` |
| 效能 | 建 realm 的時間 | `realms::realm_creation_time`（`#[ignore]`，手動跑；下面「做過的檢查」） |

## 做過的檢查

**完成時（2026-10-22，Fedora 44，AMD Ryzen AI 9 HX 370）**：每一段做完時各跑過 `cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（第 1 段 3521 個，第 2 段 3522，第 3 段 3523，第 4 段 3526，都通過，各跳過 3 個；第 4 段 4 分 48 秒）和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`。

**realm 建立的時間**：`realms::realm_creation_time`（先建一個 realm 讓 template 建好，再建 500 個、都留著，取平均）。不固定核心時兩邊都在約 350 µs 和約 500 µs 兩群之間跳（大小核），改成第 3 段和第 4 段的 binary 交錯、都釘在同一個快的核心（`taskset -c 2`）各跑 12 次：改之前 422–433 µs，改之後 446–457 µs，約多 4%。`perf stat` 的指令數每次都一樣地多約 10%（32.8 億 → 36.1 億，整個 process）；`perf record` 分開看，GC 以外的指令少約 5%（取樣 647 → 617），GC 的多約 36%（223 → 304）：`--trace-gc` 顯示改之後 incremental marking 拖到 66.9 MB 才做完（之前 38.6 MB，276 步對 26 步），marking barrier 和搬移的時間落在量測裡。建 realm 本身少做了事（不跑膠水的 script，不建 `Performance` 和膠水的物件），整體反而變慢是這個量測留著 500 個 realm 時 GC 的時機不同；量測的方法沒改。

**每一段對照 Chrome 152 和 Blink 定下或改掉的**（細節在 README 的「已決定的事項」和 `details.md`）：
- **`DOMException`**：`Error.isError(new DOMException())` 在 Chrome 是 `true`（V8 問 embedder），kun 原本是 `false`：fork 加了 binding（第 12 條）。`new DOMException()` 沒有 `stack`、平台建的有、clone 的沒有，都照 Chrome 實測。
- **計時器**：字串的 handler 在 Chrome 沒有網址（stack 是 `at <anonymous>:1:10`），錯誤的 `filename` 是 document 的網址，之後呼叫定義在裡面的 function 丟出的也是：kun 回報例外時，沒有網址的 script 一律用 document 的網址（第 17 條）。`setTimeout({toString})` 執行 `toString` 的結果；strict mode 的 function 的 `this` 也是 window。
- **`IdleDeadline`**：Chrome 給別的 frame 的 function 的是那個 frame 的 `IdleDeadline`（Blink 的 callback function 在 callback 的 script state 轉換參數），一開始照 window 的 realm 做錯了，mutation test 沒抓到才去查；改成物件記著 callback 的 realm（第 20 條）。試過在 `V8Host::call` 一律用 callback 的 realm 轉換參數，`messaging` 的兩個測試失敗：Chrome 的 event 的 wrapper 和 `data` 是 target 的 realm（Blink 的 `JSBasedEventListener` 用 target 的 script state），所以只改 `IdleDeadline`。

**找到的問題**：
- 0071 之前就有的：`dom/events/` 的 3 份 WPT 的預期結果舊了（`Body-FrameSet-Event-Handlers`、`event-listener-not-fully-active-adopted-node` 的 2 個、`passive-by-default` 變好）；`css/CSS2/generated-content/` 的 `content-048.xht`、`content-145.xht` 舊了（有了 `bgcolor`、`text` 之後就通過）；都更新了。
- 0071 之前就有的：`script-wasm`、`wasm-demo`、`navigation` 三個情境一起跑（四個同時）時都在等 wasm 的頁面載完時逾時，單獨跑都通過，在 `f06bce27` 上一樣（沒有再查，懷疑是同時編譯幾個 12 MB 的 Lua runtime）。
- `ci.py third-party` 在舊的 commit 的 worktree（pin 是 `-15`）跑時，把共用的 `../third_party/rusty_v8` 切回 `-15`；回到這裡再跑一次就切回 `-16`。第 1 段開發中途有一次 `cargo test` 沒設 `RUSTY_V8_ARCHIVE`，`target/debug` 的 V8 換回下載的，之後要刪掉 `target/debug/gn_out/obj/librusty_v8.a` 才換回來（第 3 段起用 prebuilt，不再有這個問題）。
- `dom_js` 的測試只用 `create_realm` 建的 realm 沒有 promise 的表（`JsRealm` 建的），回傳 promise 的成員回傳 `undefined`；網頁的 realm 都有，不影響行為，捲動的 promise 由 `geometry` 的測試守著。

**WPT**：每一段跑過改到的目錄：`webidl/ecmascript-binding/es-exceptions/`（加進清單，13 份；失敗的是 `new DOMException()` 有 `stack` 的提案，Chrome 152 也沒有，和 worker）、`dom/abort/`、`html/webappapis/structured-clone/`、`dom/events/`、`css/cssom-view/`、`html/webappapis/timers/`（字串的 handler、cross-realm 的例外兩份通過）、`dynamic-import/`（`string-compilation-*` 的 `setTimeout` 那一項通過）、`html/webappapis/scripting/`、`css/CSS2/generated-content/`、`requestidlecallback/`（`callback-timeRemaining-cross-realm-method.html` 通過）。wasm 的 reftest 8 份、`script-wasm`、`wasm-demo`、`navigation` 的情境（各自跑）通過。

**mutation test**（scratchpad 的 `mutants/0071-1.toml`、`0071-2.toml`、`0071-2b.toml`、`mutants-0071-3.toml`、`mutants-0071-4.toml`）：最後都全部抓到（14、10、9、9 個）。第一輪沒抓到的：第 2 段的用 boolean 移除、element 和 window 的 `scrollBy(x, y)` 當成 `scrollTo`（補了 `dom_js` 的測試）；第 4 段的 `IdleDeadline` 不屬於任何 realm（查了才發現 realm 做錯，見上面）、期限不減 time origin（第一份 document 的 time origin 幾乎是 0，補了晚建的 iframe 的測試）。`docs/test-gaps.md` 裡 union 轉換時 boolean 的偏好那一條改寫成剩下的一半：boolean 優先於字串（宣告表的 boolean 的 union 都沒有字串的成員）。
