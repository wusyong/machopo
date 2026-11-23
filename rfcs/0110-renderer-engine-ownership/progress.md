# RFC 0110 的進度：renderer 和 engine 的狀態只有一個擁有者

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-23。完成日期：2026-11-23（Linux：Fedora 45）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 評估：盤點、三種擁有權的做法的原型、兩次審查，交給使用者決定（已決定的事項第 5 到 7 條） | 完成 | 2026-11-23 |
| 1. 請求和 loader 交給 `dom`（第 7 條的第 1 步、第 10 條） | 完成 | 2026-11-23 |
| 2a. A1、A5 的必壞測試 | 完成 | 2026-11-23 |
| 2b. frame 的表只有 engine 一份，renderer、WebDriver、DevTools 向它查（第 11 條） | 完成 | 2026-11-23 |
| 2c. 建立和移除時告訴 Browser、改 child view 由 hook 當場做（第 12 條） | 完成 | 2026-11-23 |
| 2d. 文件的載入狀態和「載完了沒」的規則在 DOM（第 13 到 16 條） | 完成 | 2026-11-23 |
| 2e. 拿掉 iframe 時先 unload 再拆、當場檢查 parent；iframe 的 `load` 在子 frame 的 `load` 的 task 裡（第 17 到 21 條） | 完成 | 2026-11-23 |
| 2f. 拿掉 `Created`、`Removed` 的便條（第 22 條） | 完成 | 2026-11-23 |
| 3. 捲到 fragment 在 engine 的呼叫裡做，文件的網址只有 DOM 一份（第 7 條的第 3 步） | 完成 | 2026-11-23 |
| 4. `javascript:` 網址排在文件上（第 23 條） | 完成 | 2026-11-23 |
| 5. intersection 的通知是 DOM 的 task（第 7 條的第 5 步） | 完成 | 2026-11-23 |
| 6. parser 的行號、圖片和 DOM 排著的 script 的請求由 engine 在 Blink 的時機送出，記錄在 DOM（第 24 條） | 完成 | 2026-11-23 |

## 驗收

`shell` 的測試是 `cargo test -p shell --test it <名稱>`，其他 crate 的同樣是 `cargo test -p <crate> --test it <名稱>`。「必壞」：改之前在原本的程式上每次都失敗。

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 請求 | `fetch()` 在呼叫的當下告訴 DevTools：它的 redirect 在 script 忙的時候來也認得（A2，必壞），時間是送出時的（A3，必壞） | `devtools_network::a_fetch_is_told_as_it_is_sent_before_its_redirect`、`a_fetch_is_stamped_as_it_is_sent` |
| 請求 | 文件換掉時（同一個 engine，和換 engine）取消它還在路上的 `fetch()`；換 engine 時 unload 裡的 `fetch()` | `devtools_network::a_replaced_documents_fetches_are_cancelled`；`wasm_documents::a_js_documents_unload_may_fetch_as_a_wasm_document_replaces_it` |
| 請求 | 文件的 loader 是 DOM 的：新 frame 初始的 `about:blank` 走它的 creator 的；cookie 經過文件的 loader | `documents::a_new_frames_fetch_goes_through_its_creators_loader`；`browser` 的 `cookies::document_cookie_and_cookie_enabled` |
| 請求 | 圖片的請求在它的 microtask 裡送（必壞）；script 插入的 script 在 prepare 的當下送（必壞）；`import()` 的 module 在呼叫時送 | `devtools_network::an_image_is_requested_in_its_microtask`、`a_script_is_requested_as_it_is_prepared`；`dom_js` 的 `devtools::the_stacks_of_script_that_asks_for_loads_are_captured` |
| 請求 | iframe 拿掉時初始 `about:blank` 的圖片走 frame 的 loader，renderer 不因為 Browser 不認得的 loader 被結束；撤銷了的 blob URL 的 script 在 renderer 就失敗 | `documents::an_image_of_a_document_whose_iframe_went_is_no_bad_message`（WPT 的 `the-img-element/document-destroyed-crash.html`）；`browser` 的 `navigation::links::blobs` |
| 請求 | DevTools 的 initiator 的 parser 行號是 DOM 的，取一次 | `html` 的 `sink::the_document_parser_knows_where_its_loads_start`；`devtools_network` 的 initiator 的測試 |
| frame | 拿掉的 frame，下一個 WebDriver 指令在 renderer 處理之前就是 `no such window`（A5，必壞）；DevTools 列得到同一輪 commit 的子 frame 的 sheet（A1，必壞） | `automation_pipe::a_removed_frame_is_gone_for_the_next_command`；`devtools_css::a_new_frames_sheet_is_told_though_it_is_drawn_as_it_commits` |
| frame | engine 的查詢（token、parent、子 frame）、`take_frames_changed`；`FrameClient` 在建立和移除的當下聽到，帶 parent 的 token 和 iframe 的 `name` | `dom_js` 的 `frames::a_frame_goes_with_its_token`、`frames::the_frame_client_hears_of_frames_as_they_come_and_go` |
| frame | Browser 聽到拿掉的 frame；同一個 script 裡建了又拿掉的 frame 什麼都不留；script 拿掉自己的 frame 時 WebDriver 照樣交回不指向文件的結果 | `browser_view::the_browser_hears_of_a_removed_frame`；`automation_pipe::a_frame_made_and_removed_in_one_script_leaves_nothing`；`webdriver::frames_are_switched_to_and_from` |
| 載入 | 子 frame 的導覽被 Browser 丟掉、子 frame 的 body 收到一半中斷、背景圖的 blob URL 撤銷了：parent 照樣 `load` | `documents::a_dropped_child_navigation_delays_no_load`、`a_child_whose_body_stops_short_delays_no_load`、`a_revoked_blob_urls_background_delays_no_load` |
| 拿掉 iframe | 它和子孫的 `pagehide`、`unload` 在 `remove()` 返回之前發（必壞）；期間插入的 iframe 沒有 frame、`location.hash` 不生效（必壞），子孫 unload 時也是（必壞）；導覽離開時也是；unload 的 handler 把 iframe 插回去時新的 frame 留著 | `documents::a_removed_frame_unloads_as_it_goes`、`a_frame_unloading_as_it_goes_makes_no_frames_and_navigates_nowhere`、`a_removed_frame_navigates_nowhere_as_the_frames_in_it_unload`、`a_frame_put_back_as_it_unloads_has_a_frame`；`dom_js` 的 `frames::a_document_navigated_from_is_unloading_as_the_frames_in_it_unload`；WPT 的 `navigation-unload-same-origin-fragment.html` |
| 拿掉 iframe | 拿掉最後一個還在載入的 iframe 時 parent 的 `load` 當場排（必壞） | `documents::a_parent_loads_as_its_last_loading_frame_goes` |
| 載入 | iframe 的 `load` 緊接著子 frame 的 window `load`，在 parent 的 realm（`e.target`、`window.event`、例外） | `documents::an_iframes_load_comes_with_its_documents` |
| 導覽 | 設了 `location.hash` 之後同一個 task 讀到新的捲動位置（A4，必壞） | `documents::a_fragment_scrolls_as_it_is_navigated_to`；WPT 的 `location_hash.html`、`scroll-restoration-fragment-scrolling-samedoc.html` |
| 導覽 | `javascript:` 網址被之後的導覽取消（必壞）；它的 script 排的 task 在 `load` 之後（必壞）；文件不在了就不跑；跨 origin 的只能是目標自己的；frame 不在了就停 | `documents::a_navigation_cancels_a_javascript_url_not_run_yet`、`what_a_javascript_url_queues_comes_after_load`、`a_javascript_url_goes_with_its_document`；`dom_js` 的 `cross_origin::a_javascript_url_is_its_targets_origins_alone`、`javascript_urls_stop_once_their_frame_goes` |
| intersection | 一個 frame 算出的 entry 在下一個 frame 的 `requestAnimationFrame` 之前送到（A6，必壞） | `intersection_observers::entries_come_before_the_next_frame` |
| 全部 | 守著這些的測試擋得住弄壞的實作 | `python3 scripts/test.py mutate rfcs/0110-renderer-engine-ownership/mutants-<步驟>.toml`（`1`、`2b`、`2c`、`2d`、`2e2`、`2e3`、`2f`、`3`、`4`、`5`、`6a`、`6b`、`6c`） |

## 做過的檢查

Linux（Fedora 45）、debug build，2026-11-23。branch `0110` 從 `main` 的 `9e7d93d`（第 0 階段）開出來。

### 第 0 階段：原型

盤點照程式讀，Blink 對了 152.0.7977.82 的十一個檔（`details.md` 的「參考資料」）。三種擁有權的做法各做一個 `fetch()` 的登記的原型（patch 在 `prototypes/`，`CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`、`MACHOPO_GPU_ADAPTER=cpu`）：

| | A2 的必壞測試 | A3 的必壞測試 | `renderer` | `shell` 整組 | `browser` | `dom_js` 整組 |
| --- | --- | --- | --- | --- | --- | --- |
| 沒改（`tests.diff` 而已） | 3 次壞 3 次 | 3 次壞 3 次 | | | | |
| V0、V1、V2 | 5 次壞 0 次 | 5 次壞 0 次 | 128 個通過 | 643 個通過 | 191 個通過 | 550 個通過，1 個 ignored |
| V3 | 5 次壞 0 次 | 5 次壞 0 次 | 128 個通過 | 643 個通過 | 191 個通過 | 550 個通過，1 個 ignored；`dom` 254 個通過 |

換 engine 時 unload 裡的 `fetch()`（`tests-unload.diff`，審查之後加的）：沒改的、V0、V1 各 3 次通過；V2 3 次都在 sink 的 `expect` panic（不能 unwind 的地方，process 中止）；V3 5 次通過。V1 的請求編號從 1 重來（讀程式確認）。原型不是要合併的程式，沒有 mutation test、clippy、WPT，效能沒有量。兩次獨立的 subagent 審查（唯讀，`details.md` 的「審查」「第二次審查」），它們的關鍵說法都對過程式，V2 的 panic 重現了。

### 0109 的不穩定測試

A1 到 A6 各自寫成在原本的程式上每次都失敗的測試（「驗收」標「必壞」的）。寫法：A5 用 `postMessage` 讓 renderer 在回覆送出之後再忙一陣（只在 timer 或 microtask 裡忙的寫法都不壞）；A1 照 0109 的 `det-test.diff`（改之前 10 次壞 10 次，0109 量時 9 次壞 5 次）；A6 第一個 frame 排一個忙 100 ms 的 `postMessage`（headless 等前一個 frame 送出了才給下一個 BeginFrame，忙要在 frame 的 task 之後；只在 `requestAnimationFrame` 裡忙不壞），原因從「推論」變成確定。2e 的 iframe 的 `load` 那一個（`an_iframes_load_comes_with_its_documents`）改之前就通過：machopo 的 task 一批一批跑，這一批排的 task 下一批才跑，renderer 在這一批之後發 iframe 的 `load`，看得出差別的順序排不出來；留著守順序和 realm。

### 實作時找到的

- **2c 的 `debug_assert` 太早**：同一個 call 裡建了又拿掉的 iframe，view 當場拿掉了，renderer 之後處理它的 `Created` 時 assert 失敗（WPT 的 `initial-empty-document/iframe-nosrc.html` 等 4 個 crash，2c 的整組測試沒有這種情況）；2d 裡修（engine 沒有那個 frame 時跳過），2f 拿掉 `Created` 之後這個 assert 在對帳裡。
- **2c 的 WebDriver**：view 當場拿掉之後，script 拿掉自己的 frame 時結果變成 `javascript error`；照 WebDriver 規範（JSON clone 不再看 browsing context）改成不指向文件的值照樣交回。
- **第 14 條實作前更正**（B 改成 B'）：`Loaded` 不能從 `readyState` 推，renderer 在 iframe 的 `load` 分派之後才設；第 16 條的順序也照 renderer 原本的（前提通過了才更新 style，只剩圖片時 renderer 送了圖片再問一次）。
- **2e 的 unload counter 的範圍**：mutation test 指出 counter 在子孫 unload 之前就減回來，對照 Chrome 讀到的順序，它的 disabler 涵蓋整個 `DetachImpl`，改成等子孫都 unload 完才減，導覽離開時也是。
- **第 23 條實作前更正**：`load` 原本建議照 Blink 的 `ShouldComplete` 等 `javascript:` 網址，但 machopo 的 `load` 是 task（第 20 條），等只會把 `load` 推到那段 script 排的 task 後面；改成不等，跑完的 `check_completed` 也是多的，拿掉。
- **6b 的 `UnknownLoader`**：WPT 的 `the-img-element/document-destroyed-crash.html` 逾時：iframe 拿掉之後它初始的 `about:blank` 沒有自己的 loader，請求帶 0，Browser 把它當成 bad message 結束 renderer；原本的 `loader_of_document` 退回 frame 的 loader，共用的那一圈漏了，補回。
- **6c 撤銷了的 blob URL 的 script**：請求改成在內圈就送之後，比撤銷（外圈的 `flush_revoked`）先到 Browser，之前的 task 撤銷的 blob URL 的 script 照樣載入（`browser` 的 `navigation::links::blobs`）；renderer 先查，排一個 Browser 會給的網路錯誤。
- **我的一個錯**：2e 的第一次 WPT 是在 mutation test 還在背景跑的時候跑的，可能編到改壞的檔案，那次不算；之後的 WPT 和 mutation test 都沒有同時跑。

### Mutation test

| spec | 條數 | 抓到 | 沒抓到的 |
| --- | --- | --- | --- |
| `mutants-1.toml` | 16 | 16 | （`a cookie set through no loader` 第一次只跑 `shell`，改成跑 `browser` 的測試之後抓到） |
| `mutants-2b.toml` | 8 | 7 | `a gone frame taken for the top-level one`：推論走不到，`docs/test-gaps.md` 的「等價、不用處理的 mutation」；`a navigated child's frames before kept by the renderer` 2f 改成對帳之後是另一條（`mutants-2f.toml`） |
| `mutants-2c.toml` | 7 | 7 | （`the browser not told of a frame gone` 補了 `browser_view` 的測試之後抓到） |
| `mutants-2d.toml` | 12 | 10 | 中止時設「載入結束了」（`docs/test-gaps.md` 的「能補、還沒補的測試空缺」）；送了圖片之後再判斷一次（同上） |
| `mutants-2e2.toml` | 10 | 9 | 發完 `unload` 之後 counter 不減回來：等價 |
| `mutants-2e3.toml` | 5 | 3 | 子 frame 載完時當場檢查 parent（「能補、還沒補」）；discard 了的文件的 iframe 的 `load`：等價 |
| `mutants-2f.toml` | 8 | 7 | `commit_child` 當場對帳：等價 |
| `mutants-3.toml` | 5 | 5 | |
| `mutants-4.toml` | 5 | 4 | discard 了的文件不再跑之後的網址：等價（另外一條指出 `check_completed` 是多的，拿掉了程式和那條） |
| `mutants-5.toml` | 3 | 2 | 每個有 entry 的 observer 都排 task：等價 |
| `mutants-6a.toml` | 3 | 3 | |
| `mutants-6b.toml` | 9 | 9 | |
| `mutants-6c.toml` | 8 | 8 | |

第一次跑 `mutants-2d.toml` 用 900 秒的上限，會讓 `load` 永遠不發的 mutant 每條等滿，停掉改成 240 秒（停掉時留下的改動用 runner 存的原檔還原）。每次跑完 `target/mutate/originals` 都是空的。

做完時 spec 照最後的程式更新：之後的步驟拿掉的程式的 mutant 刪了（`mutants-2b.toml` 4 條：`Removed` 和 renderer 的 frame 樹，2f 拿掉；`mutants-2d.toml` 1 條：`Created` 的處理），`cargo fmt` 改了排版的 10 條照新的排版重寫，現在 94 條的 `from` 在檔案裡都剛好出現一次；重寫的 10 條照新的 spec 重跑，都抓到，`target/mutate/originals` 是空的。

### WPT

每一步跑相關的目錄（iframe、`<link>`、parsing、`html/dom/documents/`、`html/browsers/`、`html/webappapis/scripting/`、`html/semantics/scripting-1/`、`<img>`、`intersection-observer/`、`css/css-backgrounds/`、`css/cssom-view/`、canvas 的圖片、`FileAPI/`、`workers/`；第 1 步是 `fetch/` 和 cookie 的 3742 項），最後一次 0 個非預期的失敗。更新的預期結果：`scroll-restoration-fragment-scrolling-samedoc.html`、`location_hash.html`（A4，整份通過）、`module/dynamic-import/blob-url.any.js` 的「A revoked blob URL will not resolve」（6c）。和這份無關的非預期 PASS（拿掉這份的改動也通過，沒有改）：`script-crossorigin-network.sub.html`、`module/credentials.sub.html`、`workers/baseurl/alpha/xhr-in-moduleworker.html`。

### 不穩定的測試

沒有新的。必壞的測試改之後重複跑：A5、A1 各 5 次，A6 10 次，都通過。

### 跑過的檢查

- `cargo test --workspace`：4688 個通過、0 個失敗、1 個 ignored（log 裡的 panic 是測試預期的）。
- `cargo clippy --workspace --all-targets -- -D warnings`：通過（修了 `LoadFrame` 的型別、兩個 thread-local 的 `const`、一個 `?`）。
- `cargo fmt --check`：通過（`cargo fmt` 只碰到這份改過的檔案）。
- `python3 scripts/sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs：通過；`boundary` 的 `STATICS_ALLOWED` 加了 `NEXT_REQUEST_ID`、`NEXT_DOCUMENT_ID`。
- 整套的 `wpt`、`wdspec`、`reftest`、`scenarios` 和 `matrix`、`fuzz` 由 CI 跑（沒有動到 GC）。
