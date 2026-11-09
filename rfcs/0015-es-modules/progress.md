# RFC 0015 的進度：ES module 和 script 的載入

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-07。完成：2026-10-08，Windows 11（Rust 1.99.0）。四個階段都做完；第 1 到第 3 階段做完時各自 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a engine：修掉 realm 的洩漏；`bindings` 的 `ModuleRecords`、`dom_js` 的 `JsRealm`、`ScriptEngine` 的新方法；只用記憶體裡的 source 測 | 完成 | 2026-10-07 |
| 1b renderer：`prepare`、`Modulator`、`ModuleMap`、`ModuleTreeLinker`、MIME、`Scripts` 的順序、script 元素的 `load` / `error`、`import.meta.url`（`compile_module` 的 `base_url`） | 完成 | 2026-10-07 |
| 1c WPT（第 22 條）、`onerror`（第 23 條）、`about:demo` 和情境、`docs/limitations.md` | 完成（第 1 階段 merge 回 `main`） | 2026-10-07 |
| 2a engine：`import()` 的 callback、referrer 的 host-defined options、`ScriptEngine` 和 `FrameClient` 的新方法 | 完成 | 2026-10-07 |
| 2b renderer：`FrameRequests`、`Modulator` 抓 `import()` 的 graph、event loop 的處理 | 完成 | 2026-10-07 |
| 2c WPT、`about:demo` 和情境、mutation test、文件；每個 `import()` 自己 link | 完成 | 2026-10-07 |
| 2d renderer：heap 的 task 之後做 microtask checkpoint、DOM 變了就要求 main frame（2c 碰到的 `standalone_heap` 不穩定的原因） | 完成（第 2 階段 merge 回 `main`） | 2026-10-07 |
| 3a `HTMLLinkElement`：`dom` 的 `rel_list`、`dom_api` 的 interface 和 reflection、`relList.supports()`、WIT、`spec-differences` | 完成 | 2026-10-07 |
| 3b renderer：認得 `<link rel="modulepreload">`、抓取、`load` / `error`、`shell` 的端到端測試、WPT 的 `preload/` | 完成 | 2026-10-07 |
| 3c `about:demo` 和情境、mutation test、文件 | 完成（第 3 階段 merge 回 `main`） | 2026-10-07 |
| 4a `dom`：三個 flag、記下觸發點；`HTMLScriptElement`；`prepare` 搬到 `dom_api`；`Scripts` 的 in-order 佇列 | 完成 | 2026-10-08 |
| 4b renderer：script 插入的外部和 module script 的抓取、佇列、`load` / `error`；`document.currentScript` | 完成 | 2026-10-08 |
| 4c engine：inline classic script 的同步執行；iframe 和 script 照樹的順序（第 45 條） | 完成 | 2026-10-08 |
| 4d WPT、`about:demo` 和情境、mutation test、文件 | 完成 | 2026-10-08 |

## 驗收

每個階段做完時都在 Windows 11 上跑過 fmt、clippy（沒有警告）、`cargo test --workspace`、`sanity.py deps`、`boundary`、`unsafe`、`docs`、`test.py wpt`、`reftest`、`scenarios`（沒有平台視窗的情境）和 `test.py automation`（真的視窗），都通過；數字在「做過的檢查」。RFC 完成時的 GC 檢查（第 13 條）也在那裡。

**engine：`bindings` 和 `dom_js`**

| 項目 | 怎麼驗證 |
| --- | --- |
| 丟掉的 realm 連同它的 document 被回收（用過 `DOMException` 之後也是）；`DOMException` 的 constructor 在 context 的 embedder data | `cargo test -p dom_js --test realms`（`a_dropped_realm_is_collected_with_its_document`） |
| module 的 import 清單照原始碼的順序、重複的合併、帶 import attributes | `cargo test -p bindings --test modules`（`requests_come_once_in_source_order_with_their_attributes`） |
| graph 照 embedder 給的解析結果 link、每個 module 只執行一次（兩個 graph 共用的也是）、循環和 TDZ | `cargo test -p bindings --test modules`（`a_graph_links_and_runs_each_module_once`、`cycles_link_and_see_uninitialized_bindings`） |
| 錯誤都是 evaluate 的 promise 的 reject：找不到的 export（link 失敗）、graph 裡的語法錯誤（不執行任何 module）、embedder 設的 parse error、執行時的例外、top-level `await` 之後的例外 | `cargo test -p bindings --test modules`（`errors_reject_the_evaluation`、`a_set_parse_error_is_rethrown`） |
| `import.meta.url` 是 module 的 base URL，`import.meta` 每次是同一個物件；錯誤訊息用另一個網址（inline module：document 的 base URL 和網址） | `cargo test -p bindings --test modules`（`import_meta_url_is_the_modules_url`、`import_meta_url_is_the_base_url`） |
| module 由 `ModuleRecords` 撐住：只丟掉 context 時 context 還在，`ModuleRecords` 也丟掉之後被回收；丟掉的 realm 的 module 撐住的節點連同 document 被回收（C1） | `cargo test -p bindings --test modules`（`modules_go_with_their_realm`）、`cargo test -p dom_js --test modules`（`a_dropped_realms_modules_are_collected`） |
| 經過 `ScriptEngine` 執行的 module 拿得到 DOM 和 `import.meta.url`；module 的錯誤在 window 分派 `error`、不是 `unhandledrejection`，同步丟出的在緊接著的 checkpoint，位置是建立錯誤的地方；renderer 設的 `TypeError` 和 `SyntaxError` | `cargo test -p dom_js --test modules` |
| `import()`：frame 拿到 specifier、執行中的 script 的 base URL（classic script 的、module 的；event handler 沒有）和 attributes；graph 回來之後每個 `import()` 自己 link、用 namespace resolve；frame 不收的在下一個 microtask 之前 reject；抓取失敗是帶網址的 `TypeError`，執行時的例外是 reject，沒人處理時 `unhandledrejection`；等 top-level `await`；不在等的編號什麼都不執行；realm 丟掉時等待中的 promise 跟著放掉 | `cargo test -p dom_js --test dynamic_import` |
| `window.onerror` 收到 `ErrorEvent` 時拿到五個參數、回傳 `true` 時取消；`<body onerror>` 是 window 的；元素的 `onerror` 是一般的 handler | `cargo test -p dom_js --test errors`（`window_onerror_gets_the_members_and_true_cancels`、`body_onerror_is_the_windows`、`an_elements_onerror_is_a_plain_handler`） |

**`dom`、`html`、`dom_api`：script 元素、`<link>` 和 DOM 這一層**

| 項目 | 怎麼驗證 |
| --- | --- |
| script 的 flag：新元素是 script 建的那種（force async）；parser 建的 parser inserted、不 force async，fragment parser 和 scripting 關掉的 parse（`DOMParser`）建的另外 already started；複製的帶著 already started | `cargo test -p dom --test tree`（`script_flags`）、`cargo test -p html --test parse`（`parser_scripts_are_flagged`）、`cargo test -p dom_api --test api`（`a_cloned_script_keeps_already_started`） |
| 要 prepare 的時機：`<script>` 在有 browsing context 的 document 裡變成 connected（照樹的順序）、有節點插進 connected 的 `<script>`（在帶來的 script 之後）、設了 `src`；移出、不 connected、沒有 browsing context 的不記；iframe 和 script 記在同一個佇列，照樹的順序 | `cargo test -p dom --test tree`（`scripts_are_recorded_as_they_connect_and_get_children`、`iframes_and_scripts_are_recorded_in_tree_order`）、`cargo test -p dom_api --test api`（`setting_a_scripts_src_is_recorded`） |
| `currentScript`：每份 document 一個堆疊，module 推入的「沒有」蓋住外層的 | `cargo test -p dom --test tree`（`current_scripts_stack_per_document`） |
| 「prepare the script element」：`type="module"`（不分大小寫、去掉空白）、wasm、`nomodule`、`for` / `event`、空的 `src`、`type` 只有空白的不執行；parser 的 script 是 parser-blocking、`defer` 或 `async`，script 建的盡快、`async = false` 的照插入順序；已經 started 的不再 prepare；空的 parser script 之後插進文字時當成 script 插入的；不執行的種類不算 started | `cargo test -p dom_api --lib script` |
| `HTMLScriptElement`：`src`（URL）、`type` 等字串、`defer`、`noModule`、`crossOrigin`、`text`（文字子節點，含 CDATA）、`async`（force async 或有 attribute）、`supports()` | `cargo test -p dom_api --test api`（`script_elements`）、`cargo test -p dom_js --test bindings`（`script_elements`） |
| `relList`：每個元素一個 `DomTokenList`，在 `rel` 上，不是 `classList` 的；經過 GC 和 stress 的工作負載還是同一個 | `cargo test -p dom --test tree`（`class_lists`）、`cargo test -p dom --test gc_stress` |
| `HTMLLinkElement`：`href` 照 document 的網址解析；`as`（Fetch 的 potential destination）和 `crossOrigin` 只認得已知的值；`relList.supports()` 只認得 `modulepreload`、`stylesheet` | `cargo test -p dom_api --test api`（`link_elements`）、`cargo test -p dom_js --test bindings`（`link_elements`） |
| HTML 的 document base URL：第一個有 `href` 的 `<base>`，對 fallback 解析 | `cargo test -p dom_api --test api`（`base_url_is_the_first_base_with_href`） |

**renderer：module graph、`import()`、`modulepreload`、script 的佇列**

| 項目 | 怎麼驗證 |
| --- | --- |
| graph（L2）：每個 module 抓完才完成；兩個 graph 共用、循環都只抓一次；有一個抓失敗就失敗、不 link，map 記住失敗；第一個 parse error 照深度優先；redirect 之後照回應的網址解析；import 的檢查（bare specifier 是 `TypeError`；`type` 以外的 attribute 靜態是 `SyntaxError`、`import()` 是 `TypeError`） | `cargo test -p renderer --lib modulator` |
| `import()` 的 graph 和 script 的共用 module map、抓取不延後 `load`；preload 只抓那一個 module（不抓子孫、不 link），語法錯誤也算抓到，抓取不延後 `load` | `cargo test -p renderer --lib modulator`（`an_import_shares_the_map_and_delays_no_load`、`a_preload_fetches_the_module_alone`） |
| `Scripts`：`defer` 的 module、classic、wasm 照文件順序交錯；照插入順序的佇列；抓失敗的也輪到它（為了 `error`） | `cargo test -p renderer --lib script` |

**整個瀏覽器（`shell`，HTTP 的測試 server）**

| 項目 | 怎麼驗證 |
| --- | --- |
| module script 在 parse 完照文件順序和 `defer` 的 classic、inline module 交錯執行；共用的只抓一次；MIME、2xx、UTF-8；`load` / `error` 在輪到時分派；錯誤在 window 回報；`nomodule`；redirect 之後的網址 | `cargo test -p shell --test modules`（第 1 階段的 6 個） |
| `import()`：照執行中的 script 或 document 的 base URL 解析；錯誤；不延後 `load`；iframe 的 module map 是自己的；一起等的各自 link | `cargo test -p shell --test modules`（`import_*`、`an_iframe_imports_in_its_own_realm`、`imports_waiting_together_get_their_own_link_errors`） |
| `modulepreload`：只抓那一個、事件、`as`、不延後 `load`、填進 module map | `cargo test -p shell --test modules`（`modulepreload_*`） |
| script 插入的 script：盡快或照插入順序、`load` / `error`、延後 `load`、不執行的情況（`innerHTML`、複製的、被搬走的、不執行的種類）、iframe 的 document（`srcdoc`、初始 `about:blank`、不跑 script 的 frame、被移除的 frame）、inline module、inline classic 當場執行（`currentScript`、例外、microtask、iframe 的 realm、base URL）、iframe 和 script 照樹的順序、parser 的 script 的 `currentScript` | `cargo test -p shell --test inserted_scripts` |
| wasm 的 script 元素也有 `load` / `error`；script 插入的 wasm script 執行；wasm 文件不抓 `modulepreload` | `cargo test -p shell --test wasm_scripts` |

**WPT 和情境**

| 項目 | 怎麼驗證 |
| --- | --- |
| `the-script-element/` 的 `module/`（含 `dynamic-import/`）、`microtasks/`、`import-attributes/` 和頂層 33 個檔案 | `python3 scripts/test.py wpt the-script-element` |
| `preload/` 的 `modulepreload*.html`、`reflected-as-value.html`，`the-link-element/link-rellist.html` | `python3 scripts/test.py wpt preload link-rellist` |
| `about:demo`：module script（從 `data:` 網址 import）、`import()` 按鈕（module 只執行一次）、`modulepreload`、「Insert scripts」 | `python3 scripts/test.py scenarios demo-script` |

## 做過的檢查

2026-10-07 到 2026-10-08，Windows 11。每一步的細節（和設計的更正）寫進了 `README.md` 和 `details.md`；這裡是濃縮的紀錄。

- **WPT**：從 712 個（1c）到 964 個（4d）執行，每個階段連跑兩到四次結果一樣。每個階段先把候選的目錄試跑一次（第 22、33、39 條），結果和沒加的原因在 `details.md` 的「WPT」。
  - 第 1 階段：`module/`、`microtasks/`、`import-attributes/` 和頂層 16 個，72 個裡 61 個全過；`onerror` 讓 3 個既有的測試全過。
  - 第 2 階段：`dynamic-import/` 的 38 個裡 18 個執行（9 個有 baseline），20 個進 skip。
  - 第 3 階段：`preload/` 的 14 個 `modulepreload*.html`，2 個全過、10 個照失敗寫 baseline（`URL`、JSON / CSS / text module、SRI）。
  - 第 4 階段：頂層再 17 個；`insertion-removing-steps/` 等 9 份變成全過。`moving-between-documents/` 要 `.py`，不加。
  - 進 skip 的原因：WPT server 沒有的 wptserve 功能（`.py`、`.sub`、pipe、別的 host、HTTPS）、`document.write`、其他 RFC 的 API（`location`、worker、`fetch`、`innerText`、`dataset`）、失敗訊息裡有 server 的 port。
- **Mutation test**：共 139 個（1a 12、1b 18、1c 7、2a 10、2b 10、2c 3、2d 8、3a 14、3b 12、3c 1、4a 18、4b 13、4c 11、4d 2），跑完都確認檔案還原了。
  - 一開始沒抓到、補了測試或改寫之後抓到的：2a 不在等的編號也 evaluate；2b `import()` 延後 `load`（加一張比 module 快的圖片）；3b event loop 最上層不分派 `<link>` 的事件（改在 timer 裡插入）；4a CDATA 不算 script 的文字；4b 外部 script 不 pop `currentScript`（在 `load` listener 裡讀）、iframe 的 frame 不看 `runs_scripts`、被丟掉的 frame 不清掉 prepare 好的 script；4c 在執行中的 realm 執行、例外在執行中的 realm 回報（改用父 realm 的 `append`）、最後一個 `<base>` 算數（第一次的弄法等價）。1b 的「找第一個 parse error 時推進堆疊就標記」第一次也弄成了等價的。
  - 沒抓到、記進 `docs/testing.md` 的：1a identity hash 碰撞時拿錯 module（只靠審查）；2b 處理 `import()` 的迴圈只跑一輪、2d 因 DOM 變了而要求時不記 `dom_tree_version`、4b parser 每一步之後不取走 script 的紀錄（等價）；2d `update_rendering` 不記 `dom_tree_version`、3b 回應的 task 裡不分派 `<link>` 的事件（能補、還沒補）。
  - 一次 mutation test 的指令寫錯（4a，`--lib` 重複，每一項都「異常結束」），改正後重跑。
- **找到的 bug**：
  - **丟掉的 realm 不會被回收**（1a）：`DOMException` 的 constructor 用 `v8::Global` 放在 context 的 slot，是 root，撐住整個 context、document 和樹；改放 embedder data（第 14 條）。
  - **沒有位置的錯誤的 column overflow**、**script 以外建的錯誤的檔名是 `undefined`**（1a、1b）。
  - **一起等的 `import()` 拿到同一個 link 錯誤**（2c，WPT 找到）：改成每個 `import()` 自己 link。
  - **`import()` 的不支援的 attribute 是 `SyntaxError`**（2b，WPT 找到）：ECMAScript 的 `EvaluateImportCall` 是 `TypeError`。
  - **render loop 在 heap 的 task 之後的缺口**（2d）：`standalone_heap` 在機器忙時逾時的原因；platform task 之後做 checkpoint，DOM 變了就要求 main frame。
  - **`modulepreload` 的 `as`**（3b，WPT 找到）：不合法的值是 no state，照 `script` 抓。
  - **script 插入的 script 不延後 `load`**（4b，自己的測試找到）：改成在 `start_loads` 開始抓。
  - **`DOMParser` 的 script 複製進頁面會執行**（4c，WPT 找到）：scripting 關掉的 parse 標成 already started。
  - **同一次插入裡 iframe 比前面的 script 先建好**（4c，WPT 找到）：iframe 和 script 的紀錄改成同一個佇列（第 45 條）。
  - **`type=" "` 的 script 被當成 JavaScript**（4d，WPT 找到，第 1 階段起）。
  - **isolate 拆掉時的 use-after-free**（RFC 完成時，`test.py fuzz` 找到；上游 rusty_v8 的 bug）：`bindings` 的 `modules` 在 `--stress-compaction` 底下 access violation 或 `Check failed: node->IsInUse()`。追到上游的 `OwnedIsolate::drop` 在 dispose isolate 的 handle 之後才釋放 context 的 slot，`Weak` 的 `Drop` 這時不 reset V8 的 handle 就釋放資料，V8 拆除時的 GC 再用它。第一次以為是 slot 裡的 `Weak<Module>` 被釋放兩次，在 `ModuleRecords` 的 `Drop` 先放掉它們，沒有用，還原了。在 fork 修（第 46 條），加 `bindings` 的 `isolate_teardown`：拿掉修正時一般的 `cargo test` 和 debug 的 V8 都每次 crash（mutation test）。
- **不穩定的測試**：`shell` 的 `standalone_heap`（2c 的 rebase 之後一次）：71 次重跑沒有重現，查到是 render loop 的缺口（2d），修正之後新的測試不靠時序。
- **真的視窗**（`test.py automation`）：每個階段的驗收時 20 到 21 個情境都通過，看過 `demo-script` 的截圖。
- **`kun:dom`**：0.7.0 沒變（只有新增：`onerror`、`html-link-element`、`html-script-element`、`current-script`）；`async` 是 WIT 的關鍵字，產生器跳脫它（`%async`）。每次改 WIT 都用 `ci.py guests` 重編 guest。
- **rebase**：第 2、3、4 階段 merge 前各 rebase 一次（`main` 多了 RFC 0016 的第 1 段、RFC 0009 的 2b；第 4 階段時 RFC 0009 的 2b-3 和完成、RFC 0018 的接受），衝突都在文件（第 4 階段是 `docs/architecture.md` 的 renderer 和 `docs/test-gaps.md` 的「等價、不用處理的 mutation」，兩邊各加了內容，都留下），重跑檢查之後 merge。
- **RFC 完成時**（`docs/README.md` 的「一份 RFC 做完時」）：`test.py matrix` 8/8；`test.py fuzz` 第一次（seed `0x3efe3f69`）20 組裡 3 組失敗，是上面的「isolate 拆掉時的 use-after-free」，fork 修正之後同一個 seed 20/20，再用新的 seed `0xa2c2a316` 跑一次也 20/20；matrix 重跑 8/8（540 個測試，多了 `isolate_teardown`）。最後一次全部的檢查（rebase 到 RFC 0009 完成之後）：`cargo test --workspace` 1251 個、WPT 964 個執行（0 個不符）、reftest 526 個通過（62 個照預期失敗）、情境 42 次、`test.py automation` 21/21，fmt、clippy、`sanity.py` 四項都過。
