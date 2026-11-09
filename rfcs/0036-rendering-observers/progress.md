# RFC 0036 的進度：`ResizeObserver` 和 `IntersectionObserver`

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-12。完成：2026-10-12，Windows 11（x86_64；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0036` 上分三段做，第 1、2 段做完、經過審核時 merge 回 `main`。動到 GC 而要跑的 `test.py matrix`、`fuzz` 在 merge 之後看 CI。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a GC 型別、`Window` 的弱集合、建構子、`observe`、`unobserve`、`disconnect`（JS） | 完成，merge 回 `main` | 2026-10-12 |
| 1b rendering update 的迴圈：`content-box`、`border-box`、SVG、沒有 box、深度、loop error、要 main frame、focus fixup 的順序 | 完成，merge 回 `main` | 2026-10-12 |
| 1c `device-pixel-content-box` | 完成，merge 回 `main` | 2026-10-12 |
| 1d WPT、文件、限制、`about:demo` 和情境、Speedometer | 完成，merge 回 `main` | 2026-10-12 |
| 2a GC 型別、`Window` 的弱集合、建構子和 option、`observe`、`unobserve`、`disconnect`、`takeRecords()`（JS） | 完成，merge 回 `main` | 2026-10-12 |
| 2b `paint::intersection` | 完成，merge 回 `main` | 2026-10-12 |
| 2c rendering update 的計算、entry 和 task、implicit root 經過 iframe、跨源 | 完成，merge 回 `main` | 2026-10-12 |
| 2d WPT、文件、限制、`about:demo` 和情境、Speedometer | 完成，merge 回 `main` | 2026-10-12 |
| 3a `kun:dom` 0.14、兩個 observer 進 WIT、guest 的 `resizes`、`intersections`、所有 guest 補上空的實作 | 完成 | 2026-10-12 |
| 3b Lua runtime 的兩個 export，C、Rust、Lua 的測試 | 完成 | 2026-10-12 |
| 3c Rust 的 demo、文件、限制、第 3 段做完的檢查 | 完成 | 2026-10-12 |

## 驗收

### `ResizeObserver`

| 項目 | 怎麼驗證 |
| --- | --- |
| observation 照建立的順序（unobserve 空出的格子被新的拿走也排最後）、同一個 box 再 observe 一次不做事、換了 box 是取代（排到最後、清掉回報）、記下上次的回報、unobserve 和回報沒觀察的元素沒有作用、兩個 observer 各有各的、`disconnect` | `cargo test -p dom --test it resize_observers::observations_keep_their_order` |
| window 的 observer 是弱的、照建立的順序（新的拿走前面空出的格子也排最後），別的 window 的不算 | `cargo test -p dom --test it resize_observers::a_windows_observers_are_weak_and_in_order` |
| 被觀察的元素撐住它的每個 observer，observer 不撐住元素，unobserve、disconnect 之後放掉 | `cargo test -p dom --test it resize_observers::what_keeps_what_alive` |
| entry 撐住 target，讀回建立時的 rect 和三個大小，每次同一個物件 | `cargo test -p dom --test it resize_observers::an_entry_keeps_what_it_was_made_with` |
| 三個新型別的 slot 和 wrapper（只當自己的型別讀回、release 之後回收） | `cargo test -p dom --test it script_objects::`、`script::every_type_unwraps_only_as_itself` |
| GC 的工作負載有只靠元素撐住的 observer 和只靠 entry 撐住的 target | `cargo test -p dom --test it`（`detached_marking::`、`standalone_heap::`）、`cargo test -p dom --test incremental_stress` |
| JS 的三個 interface（只有 `ResizeObserver` 能建構，要 `new` 和函式）、`observe` 的 `box` 和取代、`unobserve`、`disconnect`、參數的 `TypeError` | `cargo test -p dom_js --test it resize_observers::` |
| 宣告表、WIT 和規範的 IDL（`resize-observer.idl`） | `cargo test -p dom_api --test it api::`、`wit::`、`spec::` |
| 第一次通知：每種 target 一次（有 box 的、`display: none`、inline、0×0、SVG 元素的 bounding box），照 observe 的順序，`this` 和第二個參數是 observer；三種大小、content rect 在 padding 的位置、大小的串列每次同一個、frozen | `cargo test -p shell --test it resize_observers::the_first_notification` |
| 之後只有觀察的盒子的大小變了才通知，在下一個 frame；transform、移動、另一種盒子的改變不算；換了 box 再 observe 一次重新回報；unobserve、disconnect 之後不再通知 | `cargo test -p shell --test it resize_observers::what_a_change_notifies` |
| `observe()` 要一個 main frame（idle callback 裡、沒有別的變化時也通知） | `cargo test -p shell --test it resize_observers::observing_asks_for_a_frame` |
| 一個 frame 裡的順序：animation frame callback、`ResizeObserver`、focus fixup（callback 裡剛停用的按鈕還有 focus）、下一個 task | `cargo test -p shell --test it resize_observers::the_order_in_a_frame` |
| 深度的迴圈：新觀察的更深的元素同一個 frame 送；改了不更深（更淺、一樣深）的留到下一個 frame，發 loop error | `cargo test -p shell --test it resize_observers::the_depth_loop_and_its_error` |
| 一次送出好幾個深度時用最淺的，它改了的更深的元素同一個 frame 照新的 layout 再送 | `cargo test -p shell --test it resize_observers::the_shallowest_depth_broadcast` |
| 子 frame 的 observer 在它的 realm 呼叫（entry 是它的）；callback 的例外在 callback 的 realm 回報，下一個 observer 照樣呼叫 | `cargo test -p shell --test it resize_observers::a_child_frame_and_an_exception` |
| device pixel 的 content box 從 border box 的 paint offset 對齊（小數位置上的寬度多一個 pixel，不用 content box 的位置）；callback 移動之後，同一個 frame 的下一輪照新的 pre-paint；子 frame 裡的元素也是 | `cargo test -p shell --test it resize_observers::the_device_pixel_content_box`、`resize_observers::a_child_frame_and_an_exception` |
| WPT 的 `resize-observer/`（5 個逾時的 `disabled`，其他照 `.ini`）；focus fixup 的順序 | `python3 scripts/test.py wpt resize-observer/`、`python3 scripts/test.py wpt html/interaction/focus/processing-model/` |
| `about:demo` 的 `ResizeObserver`：改視窗大小、加 padding，說明跟著 content box 變 | `python3 scripts/test.py scenarios resize-observer` |
| Speedometer 的盤點：Charts-chartjs 跑完、沒有例外（M4） | `python3 scripts/ci.py speedometer --inventory` |

### `IntersectionObserver`

| 項目 | 怎麼驗證 |
| --- | --- |
| observation 照建立的順序、再 observe 一次不做事（不要 frame）、`observe` 要 frame、記下上次算出的、unobserve 和記錄沒觀察的元素沒有作用、兩個 observer 各有各的、`disconnect` 留著 queue | `cargo test -p dom --test it intersection_observers::observations_keep_their_order` |
| root（implicit、元素、document）和 option 讀回；observer 弱指著 root，被回收之後是 `Gone` | `cargo test -p dom --test it intersection_observers::the_root_and_the_options`、`an_observer_keeps_its_root_weakly` |
| window 的 observer 是弱的、照建立的順序；被觀察的元素撐住 observer，observer 不撐住元素，unobserve、disconnect 之後放掉 | `cargo test -p dom --test it intersection_observers::a_windows_observers_are_weak_and_in_order`、`what_keeps_what_alive` |
| entry 排進 queue、第一個 entry 要求排 task，等著的 observer 由 window 撐住、照順序、各一次，task 拿走之後放掉、flag 清掉；entry 撐住 target、讀回建立時的值 | `cargo test -p dom --test it intersection_observers::the_queue_and_the_observers_waiting_for_the_task`、`an_entry_keeps_what_it_was_made_with` |
| 兩個新型別的 slot 和 wrapper；GC 的工作負載有只靠元素撐住的 observer、只靠 window 等著 task 的串撐住的 observer 和 entry | `cargo test -p dom --test it script_objects::`、`script::every_type_unwraps_only_as_itself`、`cargo test -p dom --test it`、`cargo test -p dom --test incremental_stress` |
| `rootMargin`、`scrollMargin` 的解析和讀回（一到四個值、絕對單位換成 px、取整數；`em`、數字、`calc()`、超過四個不收） | `cargo test -p style --lib intersection_observer` |
| JS 的兩個 interface（沒有 v2 的成員）、option 讀回、建構子的錯誤、`observe`、`unobserve`、`disconnect`、`takeRecords()`、entry 的建構子 | `cargo test -p dom_js --test it intersection_observers::` |
| 宣告表、WIT（getter 的 `root` 是另一個 union）和規範的 IDL（`intersection-observer.idl`，v2 的成員記在 `spec-missing.txt`） | `cargo test -p dom_api --test it api::`、`wit::`、`spec::` |
| `paint`：implicit root 是 viewport（捲動過的），target 被切、邊相接（空的框）、沒有交集、沒有 box；document 當 root | `cargo test -p paint --test it intersection::the_viewport_as_the_root` |
| `paint`：root margin（px、上下照高、左右照寬的百分比、負的）；不套用時是原本的框 | `cargo test -p paint --test it intersection::the_roots_margin` |
| `paint`：捲動容器的 clip（捲動過的）和 scroll margin（px、照 clip 自己大小的百分比），不改 root 的框 | `cargo test -p paint --test it intersection::a_scroll_container_and_the_scroll_margin` |
| `paint`：transform（和 `element_bounds` 一樣）、fixed 不跟著 viewport 捲動、inline 元素的框是它每行的框合起來、SVG 的圖形（加上一半的 stroke）和容器、`<svg>` 的裁切、變換不可逆的 clip 沒有交集 | `cargo test -p paint --test it intersection::transforms`、`fixed_to_the_viewport`、`an_inline_element`、`svg_elements`、`a_clip_that_cant_be_mapped` |
| `paint`：元素的 root（會裁切的是 padding box，否則 border box；它外面的 clip 不切；不是子孫、自己、沒有 box 的不相交）；捲動過的元素 root 留在原地，裡面的 target 跟著移動 | `cargo test -p paint --test it intersection::an_element_as_the_root`、`a_scrolled_element_root` |
| `paint`：子 frame 裡的 target：被 iframe 的內容框切、框在自己的 viewport；自己的 document、裡面的元素當 root；主 frame 的 document、iframe 當 root 不相交；scroll margin 擴大子 frame 的 viewport | `cargo test -p paint --test it intersection::a_child_frame` |
| 第一次每個 target 都有 entry（在 viewport 裡、一部分、邊相接、下面、沒有 box），一次 callback、在 frame 之後的 task，`this` 和第二個參數是 observer，同一個時間、`rootBounds` 是 viewport；沒有 box 的 target 的 `rootBounds` 也是 0；implicit root 的 target 在沒有 frame 的 document 裡時沒有 entry，進了 document 之後才有 | `cargo test -p shell --test it intersection_observers::the_first_entries`、`a_target_in_no_frame` |
| 之後只有 threshold 的位置或相不相交變了才有 entry（捲動、移動）；低於第一個 threshold 不算相交（ratio 和交集照給）；unobserve、disconnect 之後沒有 | `cargo test -p shell --test it intersection_observers::what_a_change_brings` |
| `observe()` 要一個 main frame；一個 frame 裡 rAF、`ResizeObserver`、`IntersectionObserver`（之後的 task）的順序 | `cargo test -p shell --test it intersection_observers::observing_asks_for_a_frame`、`the_order_in_a_frame` |
| 元素的 root（padding box、裡面的捲動容器裁切）、root margin、scroll margin、不在 root 裡的 target（沒有框）、document 當 root | `cargo test -p shell --test it intersection_observers::an_element_root_and_the_margins` |
| 子 frame 裡的 target（被 iframe 切、框在子 frame 的 viewport、`rootBounds` 是最上層的），父 frame 和子 frame 的 observer（各在自己的 realm），子 frame 捲動 | `cargo test -p shell --test it intersection_observers::a_child_frame` |
| callback 的例外照樣回報，下一個 observer 照樣呼叫 | `cargo test -p shell --test it intersection_observers::an_exception` |
| `update_intersections`：threshold 的位置、沒有面積的 target 的 ratio 是 1、`rootBounds` 不給是 `null`、一樣的不再給、一個 window 一個 task、被 `takeRecords()` 拿光的 observer 不呼叫、不再觀察的不給 | `cargo test -p dom_js --test it intersection_observers::the_rendering_updates_entries` |
| WPT 的 `intersection-observer/`（`v2/` 和一個逾時的 `disabled`，其他照 `.ini`）；`resize-observer/ordering.html` 和兩個 lazy image 的測試 | `python3 scripts/test.py wpt intersection-observer/`、`resize-observer/`、`html/semantics/embedded-content/the-img-element/` |
| `about:demo` 的 `IntersectionObserver`：滾輪捲動清單，說明跟著 threshold 變 | `python3 scripts/test.py scenarios intersection-observer` |
| Speedometer：NewsSite-Nuxt、Editor-CodeMirror 不再缺 `IntersectionObserver`；kun 的 NewsSite-Nuxt 3 輪都有效 | `python3 scripts/ci.py speedometer --inventory --reuse-chrome`、`python3 scripts/ci.py speedometer --suites NewsSite-Nuxt --browser kun` |

### wasm

| 項目 | 怎麼驗證 |
| --- | --- |
| `kun:dom` 0.14 的 guest 多了 `resizes`、`intersections` 兩個 export，都是同步的 | `cargo test -p dom_api --test it wit::the_guest_exports` |
| C 的 guest 的兩個 observer：測試照 rendering update 跑 resize observer 的步驟、交給它算好的交集、再跑送 entry 的 task；callback 經過兩個 export，entry 是 guest 的、observer 是借的（`unobserve`、`takeRecords`），選項讀回來（`rootMargin` 展開成四邊、`threshold` 的 sequence），之後沒有留下 handle | `cargo test -p dom_wasm --test it engine::a_guests_resize_and_intersection_observers` |
| wasm 文件的 rendering update 找得到 guest 的 observer（`with_host` 的 host 帶著 realm 的 window）；必填又可以是 null 的成員（`rootBounds`）的 `none` 是 null | `cargo test -p shell --test it wasm_reftests::w9_lua`、`w6_rust` |
| Rust 的 guest：body 的 border box 和 window 一樣寬，借來的 observer 停止觀察；整個 body 在 viewport 加上 margin 裡，`rootMargin` 和 `threshold` 讀回來 | `cargo test -p shell --test it wasm_reftests::w6_rust` |
| Lua：兩個 observer 的 callback，entry 留到之後還能讀、借來的 observer 在 callback 之後不能用；init 的欄位形式（省略的是預設值、單一數字或數字的 array 的 threshold、root 的型別錯誤、巢狀的 rect、可以是 nil 的 `root_bounds`、必填的 rect 和 target） | `cargo test -p shell --test it wasm_reftests::w9_lua` |
| `about:wasm-demo` 的 Rust script 的兩個 observer：它的框的寬度跟著視窗變；視窗太矮時框不在畫面裡，捲到它之後整個在 | `python3 scripts/test.py scenarios wasm-demo` |

## 做過的檢查

三段做完時都跑過 `cargo fmt --check`（第 3 段也連同 `sdk/lua/generator` 和 `sdk/rust`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`：第 1 段 2314 個通過、第 2 段 2364 個（rebase 到 `main` 之後 2368 個）、第 3 段 2369 個（42 個 binary；rebase 到 `main` 之後 2395 個）。每一步都跑了改到的 crate 的測試、相關目錄的 WPT 和 mutation test（`mutants-1a.toml` 到 `mutants-3ab.toml`，每一份的漏網和處理寫在下面）。

- **做了什麼**：
  - 第 1 段：`dom` 的 `resize_observer` 模組（`ResizeObserver`、不給 script 的 `ResizeObservation`、`ResizeObserverEntry`、`ResizeObserverSize`），元素的 rare data 的 observation，`Window` 的 observer 的 `WeakSet`；`WeakSet` 不保留順序，observer 和 observation 建立時各拿 isolate 的 `DomState` 的一個遞增的序號，讀回時照序號排（原本是 process 共用的 `static`，`sanity.py boundary` 擋下）。`dom_api::resize_observer` 的 `gather_and_broadcast`（大小經過 `LayoutClient` 的 `element_box`、`svg_geometry`、`paint_offset`，深度是 shadow-including 的祖先裡的元素數）和 `report_loop_error`；renderer 的 `run_resize_observer_steps` 在 `begin_main_frame` 的 rAF 之後、focus fixup 之前，每一輪 `update_lifecycle_to_pre_paint`；`paint::paint_offset`（Blink 的 `FirstFragment().PaintOffset()`）；`observe` 經過 `dom` 的 `resize_observations_version` 要 main frame。
  - 第 2 段：`dom` 的 `intersection_observer` 模組（`IntersectionObserver`、`IntersectionObservation`、`IntersectionObserverEntry`），`Window` 的 observer 的 `WeakSet`、等著 task 的 observer 的強的串（Blink 的 `pending_intersection_observers_`）；`style` 解析和讀回 `rootMargin`、`scrollMargin`；`paint::intersections` 一次走過 fragment tree，從 target 的 clip 往外一路 inclusive intersect（Blink 的 `GeometryMapper::LocalToAncestorVisualRect`）；renderer 的 `compute_intersections` 只在 pre-paint 重跑過或 observation 變了時算（第 13 條），commit 之後每個 observer 的 frame 呼叫 `dom_api::intersection_observer::update_intersections`，entry 由 observer 的 window 的 task 遞送（第 14 條）。
  - 第 3 段：`kun:dom` 升到 0.14，五個 interface 拿掉 `[JsOnly]`，guest 多了 `resizes`、`intersections`（第 16 條）；`dom_wasm` 的三種 observer 的 callback 共用一段；Lua 的 runtime 的兩個 export（第 17 條）和 generator 的四種欄位形式；C、Rust、Lua 的測試；Rust 的 demo 觀察自己的框（第 18 條）。
- **設計在實作中的改變**（理由寫在 README 的對應項目和條目）：迴圈的每一輪都更新到 pre-paint（第 10 條，修正第 3 條；開始前對照 Chromium 的原始碼發現每個 entry 都要 device pixel 的大小）；幾份 document 共用一個迴圈、深度各自算（第 11 條，對照原始碼更正草稿的記憶）；發 loop error 之後不另外要 main frame（1b 寫 mutation test 時發現等價）；同一個 box 再 observe 不做事，照 Chrome（1d，WPT 的 `notify.html`）；沒有框的 target 的 `rootBounds` 是 0、implicit root 的 target 在沒有 frame 的 document 裡時不給 entry，照 Chrome（2d，WPT）；`IntersectionObserver` 的 getter 的 `root` 用另一個 union（3a）；Lua 的 generator 的欄位形式從 3b 提前到 3a（Lua 的 runtime 由它產生，不支援就編不出來）。
- **找到的 bug**：
  - 元素當 root 時，它的框放在 contents 的 transform 裡、跟著它自己的捲動位置移動（2d，WPT 的 `initial-observation-with-threshold.html`）：改成放在它自己的 space，只有 clip 用 contents 的。
  - `wit.rs` 的「有物件的 union 不能又傳入又傳回」只看最上層的參數，`IntersectionObserverInit` 的 `root` 沒擋下，init 會把 guest 的 handle 交出去（3a）。
  - wasm 文件的 observer 的 callback 都不會被呼叫：`EntryHost::realm_window` 一律是 `None`（3b）。
  - 必填又可以是 null 的成員（`rootBounds`）的 `none` 被當成沒有這個成員，變成 `TypeError`（3b）。
- **寫測試時的錯**：GC 掃描 stack 是保守的，在測試函式裡建、只靠弱參照檢查的物件沒被回收，建立的部分放進 `#[inline(never)]` 的函式（1a、2a）；loop error 的 `onerror` 的 `error` 不檢查（WPT 不檢查，`ErrorEvent` 沒有 error 時給什麼不是這份的事）、父 frame 寫的 callback 的例外在父 frame 回報（1b）；子 frame 的 callback 讀父 frame 的 `const`（不是 window 的屬性）、預設的 threshold `[0]` 從 0.4 到 1 不跨過 threshold（2c）；C binding 的 `*_free` 不放掉 list 裡的 own handle，C 的測試 guest 第一次多留了 3 個（3b，SDK 的 C 範例一起改，`dom_wasm` 的 README 寫明）；Rust 的 `observe` 的 option 是傳值、Lua 印出的 `1.0` 不等於字串 `"1"`、C 的測試 guest 少了 `callback` 的定義（3b）；Rust 的 demo 原本觀察整個框，callback 寫的字讓它變高，不比上次深，一載入就是 loop error，改成觀察它的第一行（3c）。
- **mutation test**：
  - 漏網之後補了測試的：元素的串列只留第一個 observation、window 的 observer 不照建立的順序（1a）；送出的深度取最深的、迴圈排到 focus fixup 之後、`observe()` 不要 frame（1b）；paint offset 不進子 frame（1c，子 frame 的元素改成小數的寬和位置）；root margin 的上邊的百分比照寬（2b）；被 `takeRecords()` 拿光的 observer 也呼叫（2c）；Lua 的 generator 補的欄位形式（3b，第一次寫 W9 時都沒有測到，先補了檢查才跑）。
  - 多餘的程式（拿掉程式和 mutant）：「元素算自己的子孫」的過濾（2b）。
  - 等價的：「document 的 root 讀成 element」（2a，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）；「第一輪之後不再 layout」在 1b 等價（gather 先強制 layout），1c 用 device pixel 的大小抓到。
  - 測不到、記進 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」的：跨源的 target 照樣套 root margin、給 `rootBounds`，explicit root 不在了照 viewport 算（2c：跨源的子 frame 送不出結果、沒辦法強制 GC）。
  - 沒有 mutation 的：`wit.rs` 的 `taken_types`（連 record 的欄位也看）是對宣告表的檢查，現在的表沒有違反的地方，換回只看最上層的參數產生的 WIT 一樣；要測它得有一份寫錯的宣告表。
  - Lua 的 runtime（C）和 generator 的 mutant 每條先重新產生 binding、用 `ci.py` 的同一行 clang 只重編 runtime，再跑 W9；跑完用 `ci.py guests` 從還原的原始碼重編（3b）。
  - 每段做完時 fmt 改了排版或改名的 mutant 的 `from` 跟著改、重跑：第 2 段的 `mutants-2b.toml` 兩條、`mutants-1a.toml` 和 `mutants-1b.toml` 各一條（`mutants-1a.toml` 的「an observation doesn't keep its observer」、`mutants-1b.toml` 的「the content box keeps the padding」在 `main` 上就已經對不上，沒有動）。
  - 各份：`mutants-1a.toml` 16 條、`1b` 16 條、`1c` 5 條、`2a` 30 條、`2b` 21 條、`2c` 18 條、`3ab` 22 條（跑的時候 23 條；rebase 到 `main` 時 generator 的「必填的物件欄位」改用 RFC 0037 同時加的那一個，我的那段和它的 mutant 拿掉）。
- **WPT**（收錄的檔案、`disabled` 的理由和數字在 `tests/wpt/README.md`）：`resize-observer/` 整個收，39 個測試裡 34 個跑（5 個逾時的 `disabled`），104 個 subtest 過 92 個；`intersection-observer/` 收最上層（`v2/` `disabled`），114 個測試裡 113 個跑，318 個 subtest 過 188 個，剩下的大多等 window 的具名屬性、`vh` 和 `calc()`、跨源 window 的 `postMessage`、`zoom`、writing mode。`focus-fixup-rule-one-no-dialogs.html` 多過 6 個 subtest（focus fixup 在 `ResizeObserver` 之後）；`resize-observer/ordering.html` 通過；`the-img-element/` 的 `image-loading-lazy-negative-margin.html` 跑得起來、照舊失敗（第 6 條），同一個目錄四個和 observer 無關、過時的預期結果照樣更新。第 3 段沒有新的 WPT（WPT 只有 JS）。
- **Speedometer**（`ci.py speedometer`）：第 1 段之後 Charts-chartjs 跑完、沒有例外，路線圖的 M4 達成；第 2 段之後 NewsSite-Nuxt、Editor-CodeMirror 不再缺 `IntersectionObserver`（只剩 v2 的成員，Angular 的 zone.js 列舉時讀），kun 的 NewsSite-Nuxt 3 輪 × 10 個 iteration 都有效，Editor-CodeMirror 照舊因為 `getSelection` 無效。這台 Windows 上 Chrome 的量測一直沒有回報（1 個 iteration 也是 `no report in 150 s`），和這份無關，另開一個工作追，量測只量了 kun。之後查到是 Chrome 的 sandbox 讀不到使用者目錄底下的 `chrome.exe`，決定 Windows 上不跑 Chrome 那一側（RFC 0012 的 `progress.md`）。
- **guest 的重編**：WIT 和 guest 改了就用 `WIT_BINDGEN=…/wit-bindgen.exe python3 scripts/ci.py guests` 重編全部（這台的 wit-bindgen 不在 PATH 上；每次幾分鐘，大多是 Lua 的 runtime）；只改了 Rust 的 guest 時只重編它們（`ci.py` 的 `build_rust_guests`）。
- **不穩定**：`svg-img-with-external-stylesheet.html` 在 `resize-observer/`、`the-img-element/`、focus 一起跑（很多 process 平行）時失敗一次，單獨跑三次都通過；和這份無關，另開一個工作追。之後（2026-10-12）查到是 `paint` 的 bug：`PaintController` 判斷一個新的 fragment 能不能沿用上一次 paint 的內容時，比對了點陣圖（`image`）卻沒比對 SVG 圖片（`svg_image`）。這個測試的 `<img>` 有 `width`、`height`，載入前後的大小和 style 都一樣，SVG 比第一次 paint 晚到時（平行時 wptserve 回得慢）就沿用了還沒有圖的那一次，截圖裡沒有 `<img>`；單獨跑時回應快，第一次 paint 就有圖。找的方法：照原本的組合重複跑（8 輪失敗 3 次，每次截圖的 hash 都一樣），再在 renderer 暫時加 log，看到失敗時 layout 先拿到 `Loading`、之後拿到 SVG，卻再也沒有 paint 去要它的 record。修好之後同樣的組合 6 輪都通過；補了 `paint` 的 `paint_controller::an_svg_image_that_arrives_is_painted`，拿掉這個比對時它失敗。
- **第 3 段做完的檢查**：`cargo fmt`（`dom_wasm` 的測試和 W6 的 Rust guest 的排版，W6 跟著重編）；clippy 擋下測試裡 `drop` 一個沒有 `Drop` 的 read scope，改成用 block；`cargo test --workspace` 2369 個全部通過；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`cargo test -p shell --test it wasm_reftests::`（12 個）、情境 `wasm-demo`。
- **畫面**：情境 `resize-observer`、`intersection-observer`、`wasm-demo` 通過，截圖確認過；沒有動到視窗那一層，沒跑 `automation`。
- **rebase**：三段各 rebase 到 `main` 一次（第 1 段之後 `main` 多了 RFC 0035 的第 3 段，第 2 段之後多了它的第 4 段和 RFC 0037 的草稿，第 3 段之後多了整份 RFC 0037）；衝突都是兩邊改了同一處的不同地方，兩邊的都留。第 3 段的：W9 的標題（上游在前面加了一項 `Range` 的檢查，這一段的三項跟著改成第 43 到 45 項，參考檔 38 個框加上這一段的 3 個）、`rfcs/README.md` 的索引、`dom_wasm` 和 Lua 的 README，重編的 `.wasm` 用 `ci.py guests` 從合併的原始碼重編；Lua 的 generator 兩邊各加了 record 裡必填的物件欄位，用 RFC 0037 的（它的 `kun_lua_check_field_object` 多了 `required`，這一段的照舊的參數呼叫，編不過），拿掉這一段的和它的 mutant，W9 的第 45 項照樣通過；WIT 的 golden 重新產生和合併的一樣。之後重跑 `cargo test --workspace`、fmt、clippy、相關的 WPT 和情境、`sanity.py`。
