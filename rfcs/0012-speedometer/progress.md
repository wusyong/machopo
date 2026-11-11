# RFC 0012 的進度：用 Speedometer 3 和 Chromium 比較效能

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-07（1a、1c 在草稿時做完），2026-10-10 排程。完成：2026-10-16。各步驟在 macOS（Apple Silicon）、Fedora 44 和 Windows 11 上做（「做過的檢查」和「量測的紀錄」寫明每一段的平台）；最後的檢查（`cargo test --workspace`、`cargo fmt --check`、clippy）在 Fedora 44 上跑（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0012` 上做，每個階段做完、經過審核之後 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 第一次盤點 | 完成 | 2026-10-07 |
| 1b 盤點的工具放進 repo、在 kun 上跑 | 完成 | 2026-10-10 |
| 1c 每一項功能的負責 RFC（`roadmap.md`） | 完成 | 2026-10-07 |
| 2 量測的工具（第 14 到 19 條） | 完成 | 2026-10-11 |
| 2a server 和注入、兩邊開頁面的方式、有不有效的檢查（第 20 到 24 條） | 完成 | 2026-10-11 |
| 2b `perf` profile、輪流跑、比較表（第 25 到 30 條） | 完成 | 2026-10-11 |
| 2c `MACHOPO_TRACE`、span、`--trace`（第 31 到 36 條） | 完成 | 2026-10-11 |
| 2d-1a fork 用 Perfetto 編 V8（第 40 條） | 完成 | 2026-10-16 |
| 2d-1b fork 包 Perfetto 的錄製，`MACHOPO_TRACE` 錄得到 V8 的事件（第 41 條） | 完成 | 2026-10-16 |
| 2d-1c kun 的 span 寫進同一個錄製，`--trace` 和分析改用它；fork 的 release（`kun-v152.2.0-8`） | 完成 | 2026-10-16 |
| 2d-2 span 照 Chrome 的 trace 的名稱和層次（第 42 條） | 完成 | 2026-10-16 |
| 2d-3 Linux 的 `perf` 取樣、frame pointer、paint 之後的 span（第 43 條） | 完成 | 2026-10-16 |

## 驗收

### 盤點

| 項目 | 怎麼驗證 |
| --- | --- |
| Chrome for Testing 152 跑完 32 個 suite，記下每個 suite 用到的 API、CSS 宣告、選擇器、at-rule 和 `createElement` 做出的元素；kun 回答其中哪些它沒有；kun 一次跑一個 default suite，記下跑到哪裡和前幾個例外；結果寫進 `target/speedometer-results/` | `python3 scripts/ci.py speedometer --inventory` |
| Chrome 的記錄照 suite 分開：頁面在載入時（第一個 test 的 hook 之前）用到的 API 和 CSS 算給那個 suite，不算給前一個 | `python3 scripts/speedometer_test.py ChromeTests`（`test_the_recording_gives_each_suite_what_its_page_did_while_loading`） |
| 盤點的 kun 那一邊（probe、每個 suite）也經過 `--automation-pipe` 和 `POST` | `python3 scripts/speedometer_test.py` 的 `test_the_probe_posts_what_kun_lacks`、`test_the_inventorys_runs_post_how_far_they_got`；`python3 scripts/ci.py speedometer --inventory` |
| 報告只列 kun 真的沒有的：用 `var()` 的 shorthand 記成那一塊 `cssText` 裡的 shorthand（不記值是空字串的 longhand）；at-rule 由 kun 從原文試、只列它丟掉的；解析不了的 selector list 拆開、只列單獨也解析不了的元兇；namespace 裡的元素照 `createElementNS` 建 | `python3 scripts/speedometer_test.py InventoryReportTests ChromeTests.test_the_recording_has_what_kuns_probe_tries KunTests.test_the_probe_posts_what_kun_lacks` |

### 量測和比較表

| 項目 | 怎麼驗證 |
| --- | --- |
| 量測：每個 suite 在新的瀏覽器裡跑 `--iterations` 次，交回每個 iteration 的時間和每個 test 之後的 DOM 摘要（元素、class、`<style>` 和 `<script>` 以外的文字長度，經過 open shadow root）；沒跑完、有例外、摘要和 Chrome 第一個 iteration 的不同（亂數產生的 class 名稱當成同一個）時無效；兩邊的頁面都把結果 `POST` 回 server，kun 經過 `--automation-pipe` | `python3 scripts/speedometer_test.py`（假的 runner 和 suite，真的 kun 和 Chrome） |
| Chrome 跑 20 個 default suite（10 個 iteration）兩次，每次都有效、兩次的摘要相同 | `python3 scripts/ci.py speedometer --browser chrome` |
| 以 suite 為單位接著跑（kun 先、Chrome 後，一輪，2026-10-16 起）；每一邊都和 Chrome 的第一個 iteration 比，任何一個無效就不給數字；中位數、不假設分布的 95% 區間、比值、第一個 iteration、每個 test 的 sync 和 async；環境 | `python3 scripts/speedometer_test.py`（`ComparisonTests`、`MeasureTests.test_both_browsers_in_turn`）；`python3 scripts/ci.py speedometer` |

### trace（`MACHOPO_TRACE`、`--trace`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `MACHOPO_TRACE`：一份 Perfetto 的 protobuf，kun 的 span（進入時開始、離開時結束，log 的事件不在裡面）和 V8 的事件在同一個 thread 上；V8 啟動後不開始 | `cargo test -p shell --lib trace::`、`cargo test -p bindings --lib trace::`、`python3 scripts/speedometer_test.py MeasureTests.test_traces_of_both` |
| `--trace`：兩邊各錄一份，kun 的 span 用 Chrome 的名稱（task、呼叫 JS 的入口、timer、rAF、hit test、lifecycle 的各段），main frame 在 task 裡；Chrome 的 protobuf 寫得完 | `python3 scripts/speedometer_test.py MeasureTests.test_traces_of_both`；`python3 scripts/ci.py speedometer --trace --suites …` |
| span 照 Chrome 的 trace 的名稱和層次：每次呼叫 JS 是 `v8.callFunction`，stack 上沒有 script（呼叫、建構、classic script、module、microtask）時裡面才有 `FunctionCall`；建構是 `v8.callAsConstructor`；script 設的 markup、文件的每一段 parse 是 `ParseHTML`；每個元素的 reaction、`define`；每個 dispatch（async 的入口也是）是 `EventDispatch`；main frame 包住 rAF，每一步在它該在的 span 裡 | `python3 scripts/speedometer_test.py MeasureTests.test_traces_of_both`、`cargo test -p html --test it spans::`、`cargo test -p bindings --lib trace::` |

### 取樣（`--sample`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `--sample`（Linux）：`perf` 的樣本和 trace 的計時區間對齊、只留 renderer main thread 和區間裡的；JS 的函式有名字、展開穿過 JIT 的 frame；開 trace 時 `log` 的紀錄不會被做出來；`/tmp` 和工作目錄不留檔案；`perf` profile 在 Linux、macOS 有 frame pointer；paint 之後的四個 span | `python3 scripts/speedometer_test.py SampleReadingTests SampleTests MeasureTests.test_traces_of_both` |

### 量測之後在 kun 上的修正

| 項目 | 怎麼驗證 |
| --- | --- |
| live collection 的長度和第幾個從沿用的項目讀、只查那一個節點，和整份共用一次計算、DOM 變了才重算 | `cargo test -p dom --test it collections::the_length_and_an_item_share_the_kept_items`、`cargo test -p dom_js --test it bindings::html_collections` |
| 屬性照 local name 借用、不建名稱不複製；`QualName::is_html`、`is_svg` 照 namespace | `cargo test -p dom --test it style_data::an_attribute_borrowed_by_name tree::names_are_of_their_namespace` |
| label 裡的 label 和 button 拿走 click（名稱改成 atom 時補的） | `cargo test -p dom_js --test it bindings::disabled_controls_and_labels` |
| `focus()` 在元素已經有 focus 時不捲動、不更新 layout | `cargo test -p dom_api --test it focus::moving_the_focus` |
| `MouseEvent` 的 `pageX`、`pageY` 不更新 layout，分派中的 `offsetX` 會 | `cargo test -p dom_api --test it ui_events::page_and_offset_coordinates` |
| 表單的簿記：一個呼叫裡改兩次的子樹、搬進稍早插入的子樹的 checked radio；default button 離開和回到 form，沒有 form 的 button 不是 default | `cargo test -p dom_api --test it forms::a_subtree_changed_twice_in_one_call forms::a_forms_default_button` |
| 結尾的強制換行（`<br>`、保留的換行）之後不排一行（`<div>a<br></div>` 一行高，float 旁邊也是）；text control 的 inner editor 照樣有那一行（textarea 的值以換行結尾）；parley 的 `Layout::set_line_after_final_break` | `cargo test -p layout --test it inline::a_final_forced_break_makes_no_line form_controls::a_text_areas_final_newline_has_a_line_after_it parley_inline_box::the_line_after_a_final_break_is_made_only_if_asked` |
| column 和 column group 的背景圖載入之後，它們的 table 重畫 | `cargo test -p paint --test it artifact::a_loaded_background_image_paints_its_columns_table_anew` |
| `innerWidth`、`innerHeight` 不排版自己的 document：子 frame 只排版外面的 frame、從 iframe 拿大小，main frame 只在第一次 layout 之前排版自己（Chromium `main` 的 `LocalDOMWindow::GetViewportSize`；RFC 0018 照「先更新 layout」做，讀錯了它）。NewsSite-Next 的 `resize` handler 每個 iteration 約 38 ms（RFC 0067 的取樣）。用 transition 看 style 有沒有算，和 Chrome for Testing 152 一樣（`0 / 1 / 1 / 1 / 1 / 0 / 0`） | `cargo test -p shell --test it geometry::reading_the_size_styles_nothing`、`cargo test -p renderer --lib frame_view::tests::the_main_frames_viewport_size_lays_it_out_only_first`、`cargo test -p dom_api --test it geometry::the_window_lays_out_for_its_scroll_not_its_size` |
| `Window.scrollTo` 捲到 (0, 0) 時不更新 layout（Blink 的 `LocalDOMWindow::scrollTo`：(0, 0) 不會被夾住）；只有一軸、非有限的數、`scrollBy` 和 `Element.scrollTo` 照舊先 layout。NewsSite-Nuxt 每次換頁都捲到頂，但 Speedometer 的時間不變：省下的 layout 移到下一個 frame（量測的紀錄的「`Window.scrollTo(0, 0)` 不更新 layout」） | `cargo test -p dom_api --test it geometry::the_window_scrolls_to_its_origin_without_a_layout`、`cargo test -p shell --test it geometry::scrolling_the_window_to_its_top_before_a_layout` |
| SVG 圖片用 frame 的 UA stylesheet，不再每張圖從文字 parse 一次（2026-10-21 Windows 的取樣：每次約 0.3 到 1 ms，TodoMVC 的勾選框是 SVG 圖片，12 個 TodoMVC suite 每個 iteration 合計約 9 ms） | `cargo test -p renderer --lib frame_view::tests::an_svg_image_shares_the_user_agent_style_sheet`（debug build 的 `style::user_agent_style_sheets_parsed_for_testing`） |

## 做過的檢查

- **做了什麼**：
  - 1b（2026-10-10，Fedora 44；第 11 到 13 條）：盤點的工具放進 repo（`ci.py speedometer --inventory`、`scripts/speedometer.py`、`inventory.mjs`）。Chrome for Testing 152.0.7977.82（`--headless=new`）記錄 32 個 suite 用到的東西，kun 有什麼由 kun 自己回答（`probe.js`，第 13 條），kun 一次跑一個 default suite。整份約 77 秒。Chrome 跑兩次比對：API 只差在 NewsSite-Nuxt（`IntersectionObserver`、`requestIdleCallback`：它的 hydration 看時間）和 TodoMVC-WebComponents-Complex-DOM 的 `Node.parentNode`，選擇器只差在 Charts-observable-plot 亂數的 class 名稱，CSS 宣告相同，兩次都沒有例外；kun 跑兩次結果相同。
  - 2a（2026-10-11，macOS；第 14 到 24 條）：`scripts/speedometer.py` 的量測：每個 suite 一個新的瀏覽器（第 24 條）跑 `--iterations` 次，runner 的頁面注入 `measure.mjs`（每個 iteration 的時間、每個 test 之後的 DOM 摘要、例外、卡住和逾時），結果 `POST` 回 server；kun 經過 `KunPipe`（`--automation-pipe`，`Browser.setWindowBounds`、`Target.attachToTarget`、`Page.navigate`），Chrome 把網址放在命令列上；`run_problems` 判斷有不有效。盤點的 kun 那一邊（`kun.mjs`、`probe.js`）改走同一條路，拿掉 `shell` 的 `speedometer` 測試 binary 和 `MACHOPO_SPEEDOMETER_*`（改第 12 條）；`ci.py speedometer` 多了 `--browser`、`--iterations`。摘要的容許差異（第 21 條）：class 名稱裡 10 個以上連續的十六進位字元當成同一個（Charts-observable-plot 每次亂數產生 `plot-` 加 12 到 14 個字元，也寫進它的 `<style>`），文字長度不算 `<style>` 和 `<script>`。盤點用新的路重跑（`--reuse-chrome`），`inventory.md` 逐字相同，40 秒（原本 3 分 51 秒，含重編）。工具的測試 `scripts/speedometer_test.py`（第 22 條）：假的 runner（`scripts/speedometer/fake/`）照 Speedometer 的順序呼叫 `benchmarkClient` 的 hook，suite 是幾頁小的頁面（固定的、亂數的、丟例外、丟例外後卡住、一直不結束、runner 放棄、open shadow root、量視窗的寬度），kun 和 Chrome 都跑。
  - 2b（2026-10-11，macOS；第 25 到 30 條）：`Cargo.toml` 的 `[profile.perf]`（release 加 ThinLTO、`codegen-units = 1`、行號表；第一次編 2 分 26 秒）；`schedule` 以 suite 為單位輪流、`suite_problem`、`median`、不假設分布的 `median_interval`（二項分布的順序統計量）；`measure.md` 的 suite 表（中位數和 95% 區間、比值、第一個 iteration 的中位數）和 test 表（sync、async 的中位數），開頭記下環境（`environment`：commit、profile、Chrome、OS、CPU、核數、電源），沒接電源時警告；結果的 JSON 帶著環境和每一次的 run。寫測試時發現 4 個樣本算不出 95% 區間（至少要 6 個）。
  - 2c（2026-10-11，macOS；第 31 到 36 條）：`shell` 的 `trace` 模組，`main` 改成 registry 加兩層（log 的 `EnvFilter` 只管 log 那一層），`MACHOPO_TRACE` 寫進 `--help`、README。span：renderer 的每個 task（`ThreadControllerImpl::RunTask`）、`TimerFire`、`PageAnimator::serviceScriptedAnimations` 和 `FireAnimationFrame`、`ProxyMain::BeginMainFrame`、`LocalFrameView::UpdateLifecyclePhases`、`LocalFrameView::RunStyleAndLayoutLifecyclePhases`；`dom_js` 的 `FunctionCall`、`RunMicrotasks`；`dom_api` 的 `EventDispatch`；`paint` 的 `PrePaint`、`HitTest`、`Paint`；`layout` 的 `Layout`、`StyleEngine::RebuildLayoutTree`；`style` 的 `Document::UpdateStyle`；`bindings` 的 `EvaluateScript`。`ci.py speedometer --trace` 兩邊各跑一次、各存一份 trace。
  - 2d-1（2026-10-16，Fedora 44；第 40、41 條）：fork 加上 `v8/DEPS` 釘的 `third_party/perfetto`（`bd153cb9`，215 MB）、`third_party/protobuf`（`f4b11030`，58 MB）、`third_party/zlib`（`42c2f19a`，5 MB）、`tools/protoc_wrapper`（`418c6578`），`.gn` 設 `v8_use_perfetto = true`、`v8_use_perfetto_json_export = false`、`enable_js_protobuf = false`（不然要 Chromium 的 closure compiler）、`enable_perfetto_zstd = false`（zstd 不在 `v8/DEPS`）；`ci.py v8 --profile release` 一次編過（2298 秒），release 的靜態庫 198.8 → 217.0 MB，5915 個 `perfetto::` 的符號。fork 的 `src/machopo_trace.cc`：`kun__Trace__Start` 初始化 in-process backend、註冊 `kun` category、開一個 session（ring buffer 64 MB、每秒寫出，`Setup(config, fd)` 邊錄邊寫），`Stop`、`Begin`、`End`，Perfetto 自己的訊息只印錯誤；`src/machopo_trace.rs` 包成安全的函式。kun：`bindings::trace`（V8 啟動後不開始）、`dom_js::trace` 轉手、`shell::trace` 的 layer 在 span 進入、離開時 begin / end，拿掉 `tracing-chrome`；`--trace` 的 kun 檔名改成 `.pftrace`。release：fork 的 `2db724b`（四個 submodule 登記進 `.gitmodules`）發成 `kun-v152.2.0-8`，fork 的 CI 編過五個組合（Linux、macOS 的 release 和 debug，Windows 的 release；Windows 和 macOS 不用改什麼），最久 96 分鐘；gzip 之後 release 的靜態庫約 44.5 MB，debug 的 102 MB（Linux）、125 MB（macOS）；`RUSTY_V8_MIRROR_TAG` 換過去、`third_party.toml` 拿掉 `rev`。
  - 2d-2（2026-10-16，Fedora 44；第 42 條）：名稱和層次查 Chrome 的 trace 定（`ParseHTML` 在 `HTMLDocumentParser::append` 底下；listener 是 `EventDispatch` > `v8.callFunction`，`FunctionCall` 只在最外層；Chrome 在 callback 結束時、還在它的 `FunctionCall` 裡跑 microtask；upgrade 是 `CustomElementReactionQueue::invokeReactions` > `v8.callAsConstructor`；`ProxyMain::BeginMainFrame` 包住 rAF）。`bindings::trace::enter_script` 用 thread-local 的深度記下進入 script 的入口（`v8.callFunction` 和最外層的 `FunctionCall`、`v8.callAsConstructor`、`RunMicrotasks`、`EvaluateScript`、`Evaluation::evaluate`）；`ParseHTML` 包住 `dom_api` 的 fragment 和 `html` 的 `Parser::resume`、`finish`、`parse_document_with`；`CustomElementReactionQueue::invokeReactions`、`CustomElementRegistry::define`；`begin_main_frame` 整個是 `ProxyMain::BeginMainFrame`，底下 `WebFrameWidgetImpl::BeginMainFrame`、`LocalFrameView::NotifyResizeObservers`、`WebFrameWidgetImpl::UpdateLifecycle`（`dom_api::update_form_states`、`canvas::flush_shown`、lifecycle）、`PendingAnimations::Update`、`LocalFrameView::UpdateViewportIntersectionsForSubtree`、`LayerTreeHost::DoUpdateLayers`、`ProxyMain::BeginMainFrame::commit`、`RenderThread::lazy_images_due`、`RenderThread::update_intersections`、`RenderThread::settle_animation_notices`、`MouseEventManager::RecomputeMouseHoverState`。`LocalFrameView::UpdateLifecyclePhases` 也在 script 強制 layout 時出現，不列成只在 frame 裡的步驟。`sanity.py boundary` 分不出 thread-local，`SCRIPT_DEPTH` 照 `dom` 的測試計數器的做法列進 `STATICS_ALLOWED`，註明是 thread-local。
  - 2d-3（2026-10-16，Fedora 44；第 43 條）：`ci.py speedometer --sample`（Linux、只跑 kun）：kun 在 `perf record -e cycles:u -k monotonic --call-graph fp` 底下跑，同時錄 `MACHOPO_TRACE`；`MACHOPO_V8_FLAGS` 加 `--perf-basic-prof --interpreted-frames-native-stack`（`/tmp/perf-<pid>.map` 讀完就刪）。讀 Perfetto protobuf 的程式搬進 `speedometer.py`（`perfetto_slices`），`timed_windows` 照 trace 分析的算法找計時的區間（runner 的 `FireAnimationFrame` 到之後第一個 `TimerFire` 的結束，重疊的併起來），`read_perf_script` 只留 `CrRendererMain` 的樣本，`fold` 只留區間裡的，寫成 folded stacks 和 `sample.md`（每個 suite 的 self、inclusive 前 25 名）。`perf` profile 在 Linux 和 macOS 由 `build_env` 的 `RUSTFLAGS` 加 `-C force-frame-pointers=yes`。`update_all_lifecycle_phases` 在 paint 之後補 `PaintArtifactCompositor::Update`、`FrameView::take_canvas_textures`、`paint::main_thread_scroll_hit_test_region`、`FrameView::set_wheel_event_handler_region`。
  - 之後改的工具：拿掉 `--rounds`（2026-10-16，Fedora 44；第 38 條）：每個 suite 在 kun 跑完接著在 Chrome 跑；`schedule` 只排 `(suite, browser)`，`kun-runs.json`、`chrome-runs.json` 每個 suite 一次 run、沒有 `rounds`，無效的原因寫成「kun: …」；舊格式的 `chrome-runs.json` 讀不了，`--reuse-chrome`、`--browser kun` 之前要重跑一次 Chrome。盤點的四個誤報（2026-10-16，Windows 11）：值是空字串的宣告改記 `cssText` 裡的 shorthand；記錄每種 at-rule 第一個最外層規則的原文（`atRuleTexts`，nesting 也算一種），probe 用 constructed stylesheet（`replaceSync`）試，報告只列「At-rules kun drops」（原本的「At-rules (kun keeps none)」照抄 Chrome 看到的每一種）；解析不了的 selector list 照最外層的逗號拆開、每一項的 pseudo-class 和 pseudo-element 各自試，只列 `culprits`；`createElementNS` 的 key 帶上 namespace（`element:svg|circle:SVGCircleElement`，SVG、MathML、HTML 以外的是空的、一律 `Element`）。舊的 `chrome.json` 沒有 at-rule 的原文，那時不試。
  - 量測找到、在 kun 上修的（2026-10-12 到 2026-10-13，Fedora 44；數字見「量測的紀錄」）：
    - TipTap 的空白摺疊：`layout::text::InlineContent::new` 在每個字元都建一次 `TextSource`、數一遍整段原文的 UTF-16 長度，長的段落變成平方（`perf` 40% 在它自己）；改成每個 run 開始時才數一次（`crates/layout/src/text.rs`）。
    - live collection：`dom` 的 `collection_length`、`collection_item` 從沿用的 id 讀長度和第幾個（Blink 的 `CollectionIndexCache` 的作用；kun 照舊在 DOM 變了時整份重算），`dom_api` 的 `length`、`item` 和 indexed getter 經過它們。原本每次都複製整份 id 的清單，jQuery 的 `.html()` 用 index 走過 `getElementsByTagName("*")` 是平方。
    - 表單的簿記讀屬性：`perf` 改用 frame pointer（DWARF 的堆疊在 glibc 斷掉）之後看到 `dom_api::frame::update_elements` 佔 ES5 43.7%、jQuery 33.4%、ES6-Webpack-Complex-DOM 27.1%、React-Complex-DOM 13.9%，每個元素的 `input_type`、`button_type` 經過 `attribute()`（建 `QualName`、複製值、比字串），`is_html` 每次先比 28 bytes 的 namespace URL。加 `NodeRef::attr_ref(local)`、`has_attr(local)`（借用、不建名稱）給表單的那幾處用；`QualName::is_html`、`is_svg` 先比 local name。
    - 收 `&str` 名稱的 helper 和 `match` 名稱改成 atom：reflection 的 helper 收 atom、用 `attr_ref`；`reflected!`、`meter_attribute!` 用 `attr_key!`（約 90 處）；`dom`、`dom_api`、`layout`、`style` 其他收 `&str` 名稱的 helper；`match` 名稱的 5 處改成比 atom 常數或先對到 `SvgName`；`dom::name` 加 `Unlisted`、`LocalNameKey`、`unlisted::ENTERKEYHINT`。還照字串建名稱的只剩 script 給的和測試。
    - 架構盤點（`main` 的 `9e9e6913`）：兩邊各錄 20 個 suite 的 trace 比 style 和 layout 的次數，jQuery、ES5 的 `perf` 按架構分類，讀 Blink 對應的程式；方法、7 項差異和建議的順序在 `architecture-audit.md`。結論：jQuery、ES5 的差距在 JS 呼叫的 DOM API（handle 的模型約 25%、表單的簿記 jQuery 31.8% / ES5 23.0%、`innerHTML` jQuery 38.6%），不在 rendering pipeline；React-Redux、React-Complex-DOM 被 `pageX`、`pageY` 強制的整頁 style 拖慢（次數是 Chrome 的十幾倍）。
    - `focus()`：TodoMVC-jQuery 每次 render 都 `$('#new-todo').focus()`，kun 先 layout 整頁再看有沒有 box（`is_focusable_area` inclusive 34%），已經有 focus 也一樣，還捲進畫面；Chrome 1663 次 `Document::UpdateStyleAndLayout` 只有 49 次 `performLayout`。照 Blink 的 `Element::Focus`，已經是 focused element 時直接 return；和 HTML 不同（HTML 照樣捲進畫面）記進 `docs/limitations.md`，WPT 沒有測它。
    - `MouseEvent` 的 `pageX`、`pageY`（`architecture-audit.md` 第 1 項）：經過 `scrollX`、`scrollY` 每次更新整份 style 和 layout，React 的 SyntheticEvent 讀每個屬性。`dom_api::ui_event` 的 `view_scroll` 直接問 layout client 的 `viewport`；照舊在讀的時候讀（不照 Blink 在建立時算好）：WPT 的 `css/cssom-view/mouseEvent.html` 要捲動之後讀到新的位置。
    - 表單的簿記走子樹（`architecture-audit.md` 第 3 項）：jQuery 每個 batch 約 800 筆紀錄，`form_states` 走了 2.30 M 個節點、不重複的 0.62 M；找 default button 每個 batch 走整份文件（0.86 M，`<button class="destroy">` 預設是 submit）；`listed_element` 的 `listed_in`、`inserted_at` 各 1.38 M。2026-10-13 列了 A0 到 A3、B、C，做 A1（子樹或祖先走過就沿用、剪掉走過的）、A2（只有有 form owner 的 submit button 依賴它的樹，照 Blink 的 `HTMLFormElement::FindDefaultButton`；`form_states::form_owner_changed`）、A0（`descendants` 沿 first child、next sibling 走，加上能剪枝的 `walk`）；`listed_element` 的重複之後照 B（`dom` 在插入、移除時只記和表單有關的元素，Blink 的 `ListedElement::InsertedInto`），不選 A3（倒著處理、剪掉走過的子樹：radio 和 option 的「後插入的勝出」要靠處理順序的論證）；C（pseudo-class 比對時才算）牽涉 crate 的分層，現在不做。
  - 量測找到、在 kun 上修的（2026-10-18，Windows 11）：Editor-CodeMirror 一直無效（kun 的第 0 個 iteration 的 Long 之後 139 個 `div`、Chrome 141 個）。原因：CodeMirror 的空行是 `<div class="cm-line"><br></div>`，kun 把 `<br>` 交給 parley 成 `'\n'`，parley 在最後一個 `'\n'` 之後再開一行（給 editor 放游標），它的 `finish_line` 在那一行 commit 一個空的 run，`height()` 扣掉空的最後一行的判斷因此看不到它：每個以 `<br>`、保留的換行結尾的 block 都多一行（`<div><br></div>`、`<pre>a⏎</pre>` 40 對 20px；後面有空的 inline 元素時那一行沒有高度，所以 `<span>a<br></span>` 本來就對）。前 60 行裡 7 個空行各多 18.2px，CodeMirror 照量到的高度在 viewport 裡少放一行（61 對 62 行）。用 `--automation-pipe` 和 Chrome 開同一個 800×600 的 iframe、比 CodeMirror 的 `viewState` 找到；Chrome 的 `#editor` 留了 15px 的捲軸（kun 沒有捲軸，`docs/limitations.md`），內容寬 711 對 726px，但前 62 行的換行不受影響（補上 15px 的 padding 照樣 61 行）。修法（2026-10-18 確認，選 A：parley 加選項；不選 B 只改 kun、在每個讀 line 的地方跳過那一行，不選 C 照 Blink 在 textarea 的值後面放 placeholder `<br>`）：fork 加 `Layout::set_line_after_final_break`（預設照 upstream 開那一行），不開時那一行 commit 之後馬上拿回、不 yield，一行一行斷（float 旁邊）的也看不到；kun 的 `InlineContent::new` 只在 text control 的 inner editor（`empty_line` 的那個 box，Blink 的 `HasLineIfEmpty`）開它（Blink 在以換行結尾的值後面加 placeholder `<br>`，`AddPlaceholderBreakElementIfNecessary`）。之後 `<div>a<br></div>`、`<pre>a⏎</pre>`、`white-space: pre` 的 `"a\n"`、`inline-block` 裡的、`<div contenteditable>a<br></div>` 的高度和 `scrollHeight` 都和 Chrome 一樣，textarea 的 `"a\n"`、`"\n"` 照樣兩行。`<br>` 的 `Range.getClientRects()` 是空的（Chrome 給一個 rect）是另一條限制（`docs/limitations.md` 的「Bindings 和 DOM API」），沒有動。
- **和原本的計畫不同的**：
  - 第 40 條原本選 G2（fork 把舊的 `TracingController` 介面轉給 Rust，事件變成 `tracing` 的 span）。試做時從原始碼編 release 的 V8（2179 秒），錄 jQuery 一個 iteration，V8 的事件只有 `V8.DeserializeContext`（5 次）和 `V8.DeserializeIsolate`（1 次）：`v8_use_perfetto = false` 時新式的 `TRACE_EVENT(...)` 和 `TRACE_GC*`（382 處）是空巨集，舊式的 `TRACE_EVENT0/1/2` 只有 16 處。改成 V8 用 Perfetto 編（G2'），試做的改動都還原。
  - kun 的 trace 原本照第 31 條用 `tracing-chrome` 寫成 trace event JSON（2c 時和 `tracing-perfetto` 0.1.5 比過：要多帶 `prost`、`chrono` 等六到八個 crate、`rand` 會有兩個版本、時間戳是牆上的時鐘；查的時候 `cargo info` 把兩個 crate 下載進 cargo 的 cache，事先沒有說，事後告知）；2d-1 改成 fork 的 Perfetto 錄製（第 41 條），kun 的 span 和 V8 的事件在同一份 protobuf 裡，`tracing-chrome` 拿掉。
  - Perfetto 的 buffer 原本用 `DISCARD`，trace processor 的統計出現 `config_write_into_file_discard`（`write_into_file` 配 `DISCARD` 會造成看不出來的資料遺失），改成 ring buffer。
  - 量測原本預設 3 輪、每輪換先後（第 17、26、28、29 條），實際幾乎都用 `--rounds 1`；2026-10-16 拿掉 `--rounds`，一律一輪（第 38 條）。
  - 盤點的 kun 那一邊原本經過 `shell` 的 `speedometer` 測試和 `testRunner.reportHarnessResults`（第 12 條），2a 改走 `--automation-pipe` 和 `POST`（第 20 條）。
  - 原本每一組 suite 解開時量一次，2026-10-13 改成只在使用者要求時量（第 37 條）。
  - 主要的比較原本定在 macOS（第 18 條）；2026-10-12 起的比較表都在 Fedora 44 上量（AMD Ryzen AI 9 HX 370，這台的 Chrome 比 Apple M4 慢 2 到 3 倍，比值和 macOS 上的接近）。
  - 第 43 條寫 `perf` 用 `-k boottime`；硬體事件的樣本不能用 CLOCK_BOOTTIME（`cycles` 是「wrong clockid」），改用 `-k monotonic`，讀的時候加上休眠過的時間（BOOTTIME 減 MONOTONIC，這台 309407 秒）；trace 的第一個 span 和第一個樣本差 0.1 ms。Perfetto 的 SDK 的 incremental clock 從 BOOTTIME 的讀數開始，不用換算。
- **找到並修的**：
  - **`MACHOPO_TRACE` 讓 html5ever 為每個 token 做出 log 的紀錄**（2d-3）：trace 的 layer（`filter_fn(is_span)`）沒有層級的提示，tracing-subscriber 的 `init` 照全部 layer 的最高層級把 `log` crate 的最高層級設成 TRACE，html5ever 的 tree builder 每個 token 一次 `log::debug!`（`to_escaped_string` 把 token 做成字串）送進 `tracing` 再丟掉：jQuery 的計時區間裡 41% 的樣本是它。`shell` 在 `init` 之後把 `log` 的最高層級設回 log 那一層（`EnvFilter`）的（`tracing-log` 成為 `shell` 的依賴）；jQuery 3 個 iteration 的樣本 22171 → 12445 個。「2d-2 之後的 trace」的 `ParseHTML` 因此被高估；量時間的那次不開 trace，比較表不受影響。
  - **假的 runner 沒有計時的結構**（2d-3）：原本在 `load` 之後的 microtask 裡直接跑 test，trace 上沒有計時的區間。改成照 Speedometer 的 `RAFTestInvoker`：一個 rAF 的 callback 跑 test，同一個 frame 的下一個 callback 排的 timer 結束 async 的時間；test 回傳 promise 時（`hangs`、`stalls`）timer 等它。test 同步丟出的例外包成被拒絕的 promise、建的時候就標成有人處理（不然中間的 microtask checkpoint 讓 `broken` 的第一個錯誤變成 `unhandledrejection`，Chrome 和 kun 一樣）。
  - **V8 開了 `--perf-basic-prof` 在 kun 的工作目錄寫 `isolate-*-v8.log`**（2d-3）：改成寫到取樣的資料夾（`--no-logfile-per-isolate --logfile=…`）、讀完就刪。
  - **`click()` 這類從 async 入口（`dispatch_async`）發的事件沒有 `EventDispatch`**（2d-2，Chrome 有）：改成在每個 dispatch 都經過的 `dispatch_with` 用 `Instrument` 包（future 等著時不算在 span 裡）。kun 的 runner 的 test 在 rAF 之後的 `RunMicrotasks` 裡跑，所以 microtask 也算一層 script（和 Chrome 在 `FunctionCall` 裡跑 microtask 的結果一樣）。
  - **Chrome 的 trace**：被結束時 JSON 格式寫不出來（結束時才轉換），改錄邊錄邊寫的 protobuf，收到結束的要求後要 5 到 15 秒（5.1、14.9、5.1 秒），等 60 秒（第 35 條，2c）。Linux 上 headless 的 Chrome 收到 SIGTERM 建好暫存檔（`.org.chromium.Chromium.*`，0 byte）就結束、`.pftrace` 沒寫（2026-10-12；用 `data:` URL 比較：SIGTERM 0.0 秒結束、沒有檔案，SIGINT 0.2 秒、6 MB），改成在 POSIX 上用 SIGINT（`end()` 的 `interrupt`；macOS 上沒有重新確認）。Windows 上 `terminate()` 直接殺掉，拿不到 Chrome 的 trace。
  - **`KunPipe.close`**（2c）：送 `Browser.close` 之後馬上結束 kun，`main` 沒走完、trace 截斷在字串中間；改成先等 kun 自己結束，順便關掉 pipe（`ResourceWarning`）。
  - **盤點的歸屬錯誤**（2026-10-12，Fedora 44）：`inventory.mjs` 的 `willRunTest` 換 suite 時先 `takeFrame()` 才換 `current`，但 runner 在第一個 test 的 hook 之前就載入了新 suite 的頁面，每個 suite 載入和 prepare 時的呼叫和 CSS 都記到前一個 suite（例如 WebComponents 的 `customElements.define`、`attachShadow`、`:host` 記到 ES6-Webpack-Complex-DOM）。2026-10-10、10-11 的三次盤點的「最多 suite 用到的」因此偏多（`attachShadow` 寫 6 個，實際 3 個）；1a 的對照表是另一套記錄，沒有這個錯誤。改成 `willRunTest` 只換 `current`，載入時做的和第一個 test 一起在 `didRunTest`（或 `report()`）拿。
  - **盤點本身的四個誤報**（2026-10-16，Windows 11，見上面「做了什麼」）：Chrome 的 CSSOM 把用 `var()` 的 shorthand 展開成值是空字串的 longhand（pending substitution），probe 設空字串當然讀不回來；「At-rules (kun keeps none)」沒有問 kun；selector 整串一起試，同一串的 `::placeholder` 連累 `:hover`、`:focus`；SVG 的元素用 `createElement` 試，一定是 `HTMLUnknownElement`。
  - **1b 的工具**：`createElement` 的參數原本在「不計數」裡面記，一個元素都沒記到；頁面自訂的元素（`analysis-category-toolbar` 這類）不和探測頁比。
  - **2a 寫測試時**：假的 suite 在 timer 裡丟例外，test 先結束、frame 移走，timer 就不跑了（改成等 timer 丟完）；測試的期限短於 `STALL`（5 秒）時卡住被當成逾時；`<b>` 不能 attach shadow root。
  - **Windows 上 Chrome 的量測等不到報告**（2026-10-12）：Chrome 沒有發出任何請求，log 第一行是 `Sandbox cannot access executable ...chrome.exe ... 存取被拒`，network service 接著當掉：sandbox 的子程序以 AppContainer 執行，讀不到 `target/` 底下的 `chrome.exe`。加 `--no-sandbox` 或用 `icacls` 給 `ALL APPLICATION PACKAGES`、`ALL RESTRICTED APPLICATION PACKAGES` 讀取和執行的權限之後 0.7 秒交回；但同一天稍早的盤點不用改就跑完，權限不是全部的原因（2026-10-16 在 AI 助手的 sandbox 外跑也一樣，原因還沒查）。決定不修工具，Chrome 那一側不在 Windows 上跑，記進 `docs/limitations.md` 的「平台和建置」；工具的修改和權限都撤掉。2026-10-16 在 Windows 上驗證盤點的修正時，用一個不放進 repo 的 wrapper 替 Chrome 加 `--no-sandbox`。
  - **WPT 的兩個 UNEXPECTED-PASS**（2026-10-13）：`form-elements-filter.html`、`label-attributes.sub.html`（shadow DOM 裡的表單元素和 label），拿掉當時的改動也一樣，是 `main` 上更早的改動讓它們通過；用 `MACHOPO_UPDATE_EXPECTATIONS=1` 更新，兩個 `.ini` 刪掉。
  - **`compare` 在 Chrome 的紀錄沒有某個 suite 時丟 `KeyError`**（2026-10-12，Windows 上只跑 kun 的 WebComponents 時看到，收尾時修）：Chrome 先前的 run 沒有的 suite 沒有東西可以檢查 kun 的，改成這個 suite 沒有數字、寫明原因；`ComparisonTests` 測，mutation test 一條抓到。
  - **WPT 的 `css/CSS2/tables/table-backgrounds-b{c,s}-{column,colgroup}-001.xht` 時好時壞**（2026-10-18，Windows 11，跑結尾換行的修正的相關目錄時看到，和那個修正無關）：整個 `css/CSS2/tables/` 一起跑時三次裡失敗 4、3、4 個，只跑這四個時六次裡一次。失敗的截圖裡 `background-color` 的 table 有畫，`background-image` 的 column 和 column group 都是空的。原因：圖片晚於第一次 paint 才到時，renderer 照背景用到它的元素 invalidate（`col`、`colgroup`），但 column 和 column group 的背景是 table 的 box 在 cell 裡畫的（`column_backgrounds`），item 在 table 的紀錄裡，`Previous::is_invalidated` 只看 fragment 自己的 node 和行上的 inline 元素，table 就照舊沿用沒有圖的那次。row 和 row group 的背景是它們自己的 box 畫的，沒有這個問題。改成也看 table 的 `columns`（Blink 的 `LayoutTableColumn::ImageChanged` 讓 table 重畫）；之後整個目錄三次都 0 個和預期不同，`css/css-tables/` 339 個也是。
- **還沒解決的**：
  - V8 的 `--logfile=/dev/null` 讓 kun 啟動時卡住（30 秒沒回應 automation pipe），沒有追原因；取樣改寫到取樣的資料夾。
  - Perf-Dashboard 從 99.7 變成 226.7 ms（2026-10-16 的比較表）、async 比 Chrome 少，可能有東西沒畫，記進 `docs/limitations.md` 的「Paint 和 raster」。
  - `browser` 的 `navigation::links::idle_callbacks` 不穩（下面「不穩定的測試」），記進 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」。
- **不穩定的測試**：
  - `shell` 的 `transitions::a_removed_element_cancels` 和 `transitions::a_frame_shows_the_values`（2d-1 時看到，`main` 上就有）：transition 在畫出它的 frame 開始、事件在下一個 frame 送，297 個測試同時跑時兩個 frame 相隔太久。`a_removed_element_cancels` 的 transition 只有 30 ms，`transitionstart` 和 `transitionend` 在同一個 frame 排進佇列，listener 移除元素也來不及（Blink 也是這樣排事件）；整套跑 6 次都失敗，沒有這次改動的 `4b243972` 也一樣。之後 `main` 上改成 2 秒、從一半開始的 transition。`a_frame_shows_the_values` 的盒子在 300 ms 的最後 100 ms 才進到 100px 寬的 root 裡，frame 相隔超過 100 ms 時 `IntersectionObserver` 在 `transitionend` 之後才看到（6 次裡 1 次；第 2d 階段的 `cargo test --workspace` 第一次也失敗）：root 改成 280px、transition 600 ms，走到十五分之一就進來、還有約 560 ms 才結束；`shell` 的整合測試整套跑 4 次都通過。
  - 取樣的測試（2d-3，10 次失敗 4 次）：原本找 `parseMarkup` 和 `V8Host` 在同一個 stack 裡，dev profile 沒有 frame pointer，樣本在 `innerHTML` 的 Rust 裡時展開走不到 JS，`parseMarkup` 自己的 JS 只有 0 到 2 個樣本。改成找 `makeGarbage`（每次 43 到 46 個樣本），markup 的迴圈從 200 次加到 1000 次（parse 的樣本 50 到 70 個）；之後連跑 5 次都通過。
  - `browser` 的 `navigation::links::a_click_on_the_old_page_doesnt_reach_the_new_one`（2026-10-13）：整個 binary 平行跑時偶爾等「the load」等到 30 秒（卡住，不是慢）。原因：測試的 `DrivenViz::run_until` 每次等待都拿走 renderer 的訊息，等別的東西時收到的 `DidFinishLoad` 就丟了，之後的 `until_loaded` 等不到（暫時的探針 15 次裡 2 次在 `until_loaded` 開始前就收過）。`DrivenViz` 記下每次等待看到的完成的載入（`loaded`），`until_loaded` 先看它；之後整個 binary 跑 120 次，記下是哪個測試失敗的 90 次裡它沒再失敗。
  - `browser` 的 `navigation::links::idle_callbacks`（同上的 90 次裡 2 次，沒修）：第一個 `chain` 的 `timeRemaining()` 不在 (0, 50]。推測有 frame 要畫時 idle period 結束在下一個 frame（`render_thread.rs` 的 `idle_period_deadline`），只要還沒過就開始，剩下的可能不到 1 ms，平行跑時 callback 晚一點就過了期限。要確認 Chrome 的 `IdleHelper` 會不會開這麼短的 idle period，再決定要不要設最短的長度。
  - WPT 的 `focus-dynamic-type-change-on-blur.html` 在整套平行跑時 TIMEOUT（22 個 NOTRUN）；單獨跑有沒有改動都 80/80 通過（2.8 到 6 秒，期限 10 秒），推測是平行時太慢，還沒確認。
- **mutation test**（每一段一份 `mutants-*.toml`）：
  - `mutants-1b.toml`（盤點的歸屬，3 條，每條 24 秒）：換 suite 時先拿再換（原本的 bug）、`current` 一直是第一個 suite、`didRunTest` 不拿，全部抓到；修之前新的測試失敗。
  - `mutants-2a.toml`（20 條）：第一次漏了「亂數的 class 名稱不合併計數」（測試裡沒有同一份摘要出現兩個不同的亂數名稱），補上之後全部抓到。
  - `mutants-2b.toml`（11 條）：全部抓到。量測用 `perf` profile、電源的判斷測不到（測試用 dev），記在 `docs/testing.md`。
  - `mutants-2c.toml`（16 條；Rust 的每條重編 kun）：第一次 17 條裡漏了 2 條、1 條編不過。「task 不是 slice」：timer 的路上也有同名的 span，補「main frame 在 task 裡」的檢查；「Chrome 太早被殺掉」：假的 suite 的 trace 一秒內就寫完，被截斷的 protobuf 裡也有名稱，測不到，記進 `docs/testing.md`、從 spec 拿掉；編不過的改寫。2d 之後「呼叫不是 slice」「event 的 dispatch 不是 slice」「kun 拿不到 trace 的檔案」照現在的程式改、「計時器的 callback 不是 slice」修了縮排，整份重跑 16 條 15 條抓到：沒抓到「log 的事件進了 trace」（原本是 `tracing-chrome` 的 layer 的），現在的 `Spans` 只在 span 進入、離開時寫，放進來的 event 也不會被記下，filter 只剩效能上的作用（不讓每個 event 的 callsite 都打開），測不到。
  - `mutants-collections.toml`（6 條，93 秒）：全部抓到。
  - `mutants-attributes.toml`（12 條）：第一次漏了「`is_svg` 不看 namespace」，補 `tree::names_are_of_their_namespace`，重跑都抓到。
  - `mutants-name-tail.toml`（9 條）：全部抓到；第一次漏了「label 不算互動內容」，補 `bindings::disabled_controls_and_labels` 的 label 和 button；編不過的 1 條改寫；`<input>` 的 `value` 屬性改了、狀態不是存的也存一份是等價的，記進 `docs/testing.md`、從 spec 拿掉。
  - `mutants-focus.toml`（2 條，27 秒）、`mutants-pagex.toml`（3 條，30 秒）：全部抓到。
  - `mutants-form-walks.toml`（9 條，`cargo fmt` 前後各跑一次，約 100 秒）：全部抓到；等價的 3 條記進 `docs/testing.md`。
  - `mutants-one-round.toml`（6 條，411 秒）：全部抓到。
  - `mutants-inventory-false-positives.toml`（21 條，每條約 2 秒）：全部抓到。第一次漏了「selector list 在括號裡也切開」：`.e:is(.f` 這種沒關上的括號 CSS 的 parser 會補上，切開之後照樣解析得了；改成括號外面沒有 pseudo 的 `.e:is(.f, .g), .h:not-a-class`，切壞的 `.g)` 會被當成元兇。寫 spec 時拿掉幾處等價的程式：宣告名稱的切分不管括號和引號（切壞的名稱讀回來是空字串）、不認得的 namespace 不編進 key、longhand 不必另外排除。
  - `mutants-2d-1.toml`（8 條，45 秒）：全部抓到；補了 `bindings::trace` 的測試才抓到「V8 啟動後還開始」。fork 的 C++ 手動改壞、增量重編 V8 再跑 `test_traces_of_both`：不註冊 `kun`、不開任何 category、不停 session、begin 不寫，4 條都抓到；Perfetto 的訊息只印錯誤沒有測試（只影響 stderr）。
  - `mutants-2d-2.toml`（33 條，210 秒）：32 條抓到。沒抓到「建構不算一層 script」：kun 的每一次建構外面都已經有別的入口（`createElement` 的同步建構在呼叫裡，upgrade 在 `[CEReactions]` 或 backup element queue 的 microtask 裡，parser 建的元素也是在 microtask 裡 upgrade），結果一樣；留著是為了 parser 之後照 HTML 同步建構時層次還是對的。
  - `mutants-2d-3.toml`（24 條：23 條一起跑 581 秒，「每個 profile 都有 frame pointer」要整個重編 dev，450 秒；「test 丟出的例外沒人處理」之後補的、單獨跑）：23 條抓到。沒抓到「不給 `--interpreted-frames-native-stack`」：假 suite 裡樣本夠多的 JS 函式都已經最佳化、本來就有名字，記進 `docs/test-gaps.md` 的「測試守不住的地方」。這條 mutant 留下的 `isolate-*-v8.log` 已經刪掉。
  - 其他 RFC 的 spec 跟著 2d 的程式改（意思不變、逐條重跑都抓到）：RFC 0028 `mutants-4c.toml` 的「viewport 附近的不繼續載入」（用只放它的 spec、現在的測試名稱 `navigation::links::lazy_images` 跑；這份 spec 本身在 2d 之前就跑不了：預設的測試指令還是 `--test navigation`，`image.rs` 的三條對不上，沒有改），RFC 0048 `mutants-1e.toml` 的「pending 的 transition 不開始」（整份重跑 19 條全部抓到，434 秒，包括「只動 transition 的 frame 不畫」）、`mutants-3a.toml` 的「開始的動畫的 ready 不 settle」。
  - `mutants-final-break.toml`（結尾的強制換行，9 條，每條 4 到 6 秒）：全部抓到。第一次 11 條漏了 3 條：「`clear()` 不重設」要同一個 `Layout` 再 build 一次才看得出來，補了 `parley_inline_box` 的那一段；「任何換行之後的空行都拿掉」：結尾沒有 item 的一行只會在強制換行之後（`finish_line` 也照這個前提），把多餘的 `Explicit` 檢查拿掉；「`LayoutData` 的預設值」等價（每次 build 都先 `clear()`）。第二次漏了「只有一行也拿掉」：builder 給每個 layout 至少一個 item，沒有文字時也是，第一行一定有 item，等價；兩條等價的寫在 spec 的開頭和 `docs/testing.md`。
  - `mutants-column-backgrounds.toml`（column 的背景圖，2 條，每條 3 到 4 秒）：全部抓到。
  - `mutants-viewport-size.toml`（`innerWidth`、`innerHeight` 不排版自己的 document，6 條，每條 2 到 20 秒）：全部抓到。第一版讓 main frame 一律不排版，對照 Chromium 的程式才發現第一次 layout 之前它會排版自己，Chrome 上的頁面也看得出來（第一次讀時算了 style）；補上之後測試頁的第一次讀要放在 `<iframe>` 之前（kun 插入 iframe 時就排版外面的 frame）。renderer 的單元測試只能測 main frame：detached 的 DOM 裡子 frame 不是 fully active，`absorb_child_frames` 會把它從樹上拿掉，子 frame 由 shell 的測試守。
  - `mutants-svg-user-agent.toml`（SVG 圖片共用 UA stylesheet，3 條，每條 3 到 8 秒）：frame 傳一份重新 parse 的、SVG 圖片自己 parse、計數不加，全部抓到。最後一條要測試先確認建 frame 自己的 engine 時計數加一，否則計數不動時「沒有重新 parse」永遠成立。
  - `mutants-scroll-origin.toml`（`Window.scrollTo(0, 0)` 不更新 layout，7 條）：一律 layout、只看 left、只看 top、任一軸是 0、缺的軸當 0、捲到別處，由 `dom_api` 的測試抓到；不捲動由 `shell` 的測試抓到（`dom_api` 的假 client 只記錄被問了什麼）。全部抓到。
- **跑過的**：
  - `Window.scrollTo(0, 0)` 不更新 layout（2026-10-23，Windows 11）：`cargo test -p dom_api -p shell` 全部通過；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
  - `innerWidth`、`innerHeight` 不排版自己的 document（2026-10-21，Windows 11）：`cargo test -p dom -p dom_api -p dom_js -p dom_wasm -p renderer -p shell` 全部通過（`LayoutClient` 多了有預設的 `viewport_size`）；`cargo fmt -p dom -p dom_api -p renderer -p shell --check`；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py wpt css/cssom-view/` 243 個：第一次（2 分 48 秒，緊接在上面的 `cargo test` 之後、先 build release 的 kun）有 4 個 TIMEOUT（`scrollWidthHeight-child-border-within-padding.tentative.html`、`scrollParent-shadow-tree.html`、`scrollIntoView-stuck.tentative.html`、`table-with-border-client-width-height.html`，都沒有用到 `innerWidth`、`innerHeight`），單獨跑這 4 個和整個目錄再跑一次（24 秒）都是 0 個和預期不同，推測是機器忙時的逾時，沒有再追。測試頁的預期值先在 Chrome for Testing 152（headless，經過 `bench` 的 CDP pipe）跑兩次確認。沒有重量 Speedometer。
  - SVG 圖片共用 UA stylesheet（2026-10-21，Windows 11）：`cargo test -p renderer -p style`（79、4、455 個）、`cargo fmt -p renderer -p style --check`；`sanity.py deps`、`boundary`（新的 thread-local 計數加進允許的清單）、`unsafe`、`tests`、`docs`。沒有重量 Speedometer。
  - column 的背景圖（2026-10-18，Windows 11）：`cargo test -p paint -p renderer`、`cargo fmt -p paint --check`；`test.py wpt css/CSS2/tables/` 三次、`css/css-tables/` 一次，都和預期一樣。
  - 結尾的強制換行（2026-10-18，Windows 11）：`cargo test -p layout`（327 個）、`-p paint -p renderer`；`python3 scripts/sanity.py docs`、`tests`；`test.py reftest inline/ text/ floats/ generated/ block/` 46 個都和預期一樣；`test.py wpt` 跑 `css/css-text/`、`css/CSS2/generated-content/`、`floats/`、`floats-clear/`、`tables/`、`lists/`、`css/css-lists/`、`css/css-content/`、`css/cssom-view/`、`html/rendering/`、`selection/`、`dom/ranges/`（2522 個測試）：15 個和預期不同，拿掉修正重跑這 15 個，cssom-view 的 10 個（scroll 的 promise 和 smooth scroll）和 table 的 2 個照樣不同，和修正無關（table 的見上面「找到並修的」）；`letter-spacing-211.html` 通過了（控制組是 `pre-wrap` 的 `<p>A B C⏎`，原本多一行；`.ini` 寫的 `text-emphasis` 兩邊都沒有），用 `MACHOPO_UPDATE_EXPECTATIONS=1` 刪掉它的 `.ini`。`cargo test --workspace`、`cargo fmt --check` 和 clippy 沒跑（不是階段做完）。
  - 每段：改到的 crate 的測試（2d-1 `cargo test -p bindings -p dom_js -p shell`，2d-2 `-p bindings -p dom_api -p dom_js -p html -p renderer` 172 秒，2d-3 `-p renderer -p shell` 43 秒）；`python3 scripts/speedometer_test.py`（2a 28 個約 57 秒、2b 36 個約 65 秒、2c 37 個約 76 秒、2d-1 38 個、2d-2 41 個、2d-3 47 個 63 秒）；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`。2d-1c 不設 `RUSTY_V8_ARCHIVE`、下載 prebuilt 之後 `cargo test -p bindings -p dom_js -p shell`（42 秒）和 `speedometer_test.py` 38 個通過。拿掉 `--rounds` 時這台還沒下載 Chrome for Testing（26 個通過、3 個 skip），下載之後 `MeasureTests ChromeTests` 12 個通過。2026-10-16 在 Windows 上（`--no-sandbox` 的 wrapper）41 個裡 40 個通過，失敗的 `test_traces_of_both` 是 Windows 拿不到 Chrome 的 trace。
  - 2026-10-13 的修正：`cargo test -p dom -p dom_api -p dom_js`（9 個 binary、1107 個）；`cargo test --workspace --no-fail-fast` 43 個 binary、2475 個（屬性）和 2481 個（atom，620 秒，含 `cargo clean` 之後的重編）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（修了 `reset.rs` 的測試少了 `QualName` 的 import）；`cargo test -p shell --test it -- geometry mouse click pointer`、`-- keys focus`；WPT：`dom/collections/ dom/nodes/` 15656 個預期的結果；`html/semantics/forms/ dom/nodes/` 19003 個；`css/cssom-view/ uievents/` 275 個測試、2065 個檢查；`html/interaction/focus/ shadow-dom/focus/ css/selectors/focus-visible-001.html`；`html/semantics/forms/ css/selectors/ dom/nodes/ html/semantics/selectors/` 671 個測試、19144 個檢查；預期外的只有上面的 TIMEOUT 和 UNEXPECTED-PASS。
  - 第 2 階段做完（2026-10-11，macOS）：`cargo test --workspace` 2124 個全部通過，`cargo fmt --check`（修了 `shell` 的 `trace.rs` 的排版），clippy 沒有警告。
  - 第 2d 階段做完（2026-10-16，Fedora 44）：第一次 `cargo test --workspace` 192 秒，`a_frame_shows_the_values` 失敗（cargo 在那個 binary 停下），`cargo fmt --check` 是 `shell/src/main.rs` 的兩處，clippy 是 `html/tests/it/spans.rs` 多餘的 `let`；修好之後 `cargo test --workspace --no-fail-fast` 全部通過（53 秒，45 個測試 binary），`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings` 沒有問題。

## 量測的紀錄

新的在前。2026-10-13 的盤點時，Complex-DOM 和 NewsSite 在 kun 上還少算了用 `var()` 的 style 和 `::before`、`::after` 的 box，那時以前的比較表裡它們的比值偏低；「2d-2 之後的 trace」的 `ParseHTML` 被高估，以最後一次為準。

### binding 的 front 只拿 callback 的 info 之後，和之前比（2026-11-11，Fedora 44）

使用者要求量的（RFC 0097 第 2、3 段）。`bench.py speedometer --against 0097-baseline`（`main` 的 `272ffe1` 加上 `0097` 的 rusty_v8 的 pin，本地的 `7194a4f`：工具要兩個版本釘同樣的 `../third_party`，fork 的改動向下相容）對 `6db8168`，各 3 次輪流、每次 10 個 iteration。20 個 suite 的 current / against 的幾何平均 0.991（−0.9%）；各 suite 的中位數加起來 984.2 → 976.1 ms。17 個 suite 變快、3 個變慢（`TodoMVC-WebComponents` +2.1%、`Editor-CodeMirror` +1.0%、`React-Stockcharts-SVG` +0.1%），最多的是 `TodoMVC-Angular-Complex-DOM`、`TodoMVC-Vue` −2.6%；多數 suite 兩邊的 95% 區間重疊。peak memory −5.9% 到 +3.4%。沒有和 Chrome 同一輪跑。

### 換 DOM 之後兩邊用同樣的方法取樣（2026-11-06，Fedora 44）

使用者要求量的（RFC 0085 的重新取樣）。量法、兩種 Chrome 的估計、各類和每個 suite 的表在 `rfcs/0085-optimization-roadmap/details.md` 的「換 DOM 之後的分析（2026-11-06）」，摘要在 `docs/performance-gaps.md`。

- kun `f21c5cac`：`bench.py speedometer --sample --iterations 5`，2 分鐘；每個 suite 445（Preact）到 3222（jQuery）個 `cycles:u` 的樣本（每 100 萬一個），取樣時的計時區間比平常的時間長約 6%。
- Chrome：Fedora 的 Chromium 156.0.8078.79（有 debuginfo），同樣的 `perf` 設定，暫時的腳本；第一次用 `bench.py` 預設的 trace category，計時區間是平常的兩倍（jQuery 每個 iteration 197 對 92.7 ms），改成只錄 `blink.user_timing` 之後長一到兩成。
- 同一天各跑一次 20 個 suite（10 個 iteration）：Chrome for Testing 152 合計 670.8 ms，Chromium 156 845.3 ms（每個 suite 慢 12% 到 47%，幾何平均 23%），Chromium 156 加取樣的 flag（`--no-sandbox`、V8 的 perf map）856.3 ms（+1.4%）。
- 結果（每個 iteration 的 ms，20 個 suite 合計，Chrome 是估計）：kun 1078、Chrome 671；差最多的是文字 +84 到 +134、JS +101、style +77、DOM +43、GC +39、box layout +37。

### 換成照 `crates/gc` 設計的 DOM 之後的比較表（2026-11-06，Fedora 44）

`bench.py speedometer`（20 個 default suite，各 10 個 iteration；換 DOM 的 RFC 0089 的收尾），Fedora 44（Linux 7.2）、AMD Ryzen AI 9 HX 370，接電源、兩個瀏覽器綁在快的核心（0–3、12–15），kun 的 `perf` profile，Chrome for Testing 152.0.7977.82。每次 20 個都有效。逐 suite 的表、blink-perf 和每次呼叫的指令數在 `rfcs/0089-dom-replacement/measurements.md`。

- 舊的 DOM（cppgc 上的，`fb61d682`）合計 1262.9 ms，比值的幾何平均 1.78；換完的 `37520412` 是 1.61，比舊的 DOM 慢的是 TodoMVC-jQuery（186.9 → 280.9 ms）、ES6-Webpack-Complex-DOM（63.7 → 67.6）和 Editor-TipTap。
- 之後在 `main` 上追回那三個（jQuery 讀 `getElementsByTagName("*")[i]` 每次從頭走樹、ES6-Webpack 的 `querySelector` 每個元素重建比對的 context 而且不跳過子樹、TipTap 的 `focus()` 把整個編輯器的文字切成 grapheme cluster），重量：合計 1083.3 ms、Chrome 678.2 ms，比值的幾何平均 **1.50**，每個 suite 都不比舊的 DOM 慢（TipTap 修好之後 96.3 對 95.2 ms）。
- 和 Chrome 比差最多的（重量的那次，TipTap 修好之前）：Editor-TipTap 2.23、NewsSite-Next 2.09、Perf-Dashboard 1.91、TodoMVC-jQuery 1.78、TodoMVC-WebComponents 1.76；Charts-chartjs 1.00。
- 10-24 的 2.02 是另一台（Windows 11、i9-13900K），不能直接比；同一台上的前後是上面的 1.78 → 1.50。

### 用 samply 取樣兩邊：全部 20 個 suite 照同樣的分類比（2026-10-24，Windows 11）

使用者要求量的。接著下面那筆的做法，兩邊都重新取樣 20 個預設的 suite、各 5 個 iteration（kun `ce849649` 用 `--sample --iterations 5`；Chrome 155.0.8059.85 用同一支腳本），分類相同。每個 iteration 的 ms，20 個 suite 合計（kun / Chrome，差，倍數）：

| 種類 | kun | Chrome | 差 | 倍數 | 只有這一類追平時的幾何平均 |
|---|---|---|---|---|---|
| JS | 312.4 | 232.1 | +80.3 | 1.3 | 1.67 |
| DOM | 192.6 | 71.3 | +121.3 | 2.7 | 1.62 |
| style | 156.0 | 60.6 | +95.4 | 2.6 | 1.66 |
| 文字 | 100.4 | 83.6 | +16.8 | 1.2 | 1.77 |
| box layout | 87.4 | 52.0 | +35.4 | 1.7 | 1.76 |
| V8 API | 63.2 | 9.3 | +53.9 | 6.8 | 1.72 |
| HTML parse | 47.4 | 14.9 | +32.5 | 3.2 | 1.79 |
| paint | 45.8 | 36.0 | +9.7 | 1.3 | 1.78 |
| GC | 44.1 | 20.4 | +23.7 | 2.2 | 1.79 |
| bindings | 43.2 | 25.5 | +17.7 | 1.7 | 1.78 |
| 其他 | 33.3 | 7.1 | +26.2 | 4.7 | 1.76 |
| canvas 的 raster | 4.7 | 5.1 | −0.4 | 0.9 | |
| 合計 | 1130.4 | 617.8 | +512.6 | 1.83 | |

- **main thread 的 CPU 時間的比值，20 個 suite 的幾何平均 1.815**；比較表（總時間）的 2.02。差在 Chrome 被取樣時慢了約 11%（取樣下合計 617.8，比較表的中位數合計 556.9；kun 1130.4 對 1140.0，沒有變慢），所以這裡的倍數都低估了約一成，Chrome 的 V8 flag（`--enable-etw-stack-walking --interpreted-frames-native-stack`）和 ETW 的事件是最可能的原因。
- **每個 suite**（kun / Chrome，倍數）：jQuery 188.6 / 85.3（2.21）、Next 115.8 / 66.1（1.75）、Nuxt 88.1 / 48.7（1.81）、Stockcharts 88.8 / 55.2（1.61）、TipTap 83.6 / 47.5（1.76）、Perf-Dashboard 64.4 / 36.8（1.75）、chart.js 62.2 / 31.1（2.00）、ES6-Webpack 60.6 / 26.2（2.31）、ES5 48.9 / 31.4（1.56）、observable-plot 44.3 / 30.9（1.44）、React-Complex-DOM 39.0 / 23.0（1.69）、React-Redux 37.0 / 23.9（1.55）、Angular 36.3 / 20.5（1.77）、Backbone 34.7 / 18.2（1.90）、WebComponents 30.1 / 11.8（2.54）、Lit 27.8 / 11.8（2.35）、Vue 27.2 / 15.6（1.75）、CodeMirror 21.1 / 17.1（1.24）、Preact 16.5 / 8.8（1.88）、Svelte 15.4 / 7.8（1.97）。輕的 12 個 suite 差最多的也是 DOM、style、JS、V8 API，和重的 8 個一樣，沒有別的種類冒出來。
- **和 8 個 suite 那筆比**：文字（1.2 倍）和 paint（1.3 倍）在 20 個 suite 裡差得少，TipTap 一個 suite 佔了文字的差的大半；DOM（2.7 倍）、style（2.6 倍）、V8 API（6.8 倍）、HTML parse（3.2 倍）是每個 suite 都有的差。「其他」4.7 倍是 `renderer` 的 `FrameView`（`url`、`image`、`update_all_lifecycle_phases`）、`gfx::Path` 和 `scroll::container_nodes`，Chrome 對應的 cc、base 算給呼叫它的，不完全對等。
- **kun 每一類最多的函式**：DOM 是 `create_node` 21.7、`event::dispatch_with` 9.9、`walk_moved_subtree` 8.8、`class_names_of` 5.7、`collection::item` 5.5；style 是 `cascade` 34.6、`recalc_style` 21.8、`matches_complex_selector_internal` 16.7、`SelectorQuery::query` 8.9；V8 API 是 callback 的入口 `c_fn` 12.1、`to_rust_string_lossy` 4.5、`Object::Get` 3.4、`Function::Call` 3.3、`Object::Wrap` 3.2、`ObjectTemplate::NewInstance` 3.1、`FunctionTemplate::GetFunction` 2.9；GC 是 `BasicPersistent::Assign` 8.1、`init_rust_obj` 5.5、`Root::try_new` 加 `Root::new` 7.6（都是 kun 的 root 和 persistent 的機制，Chrome 的 GC 是 Scavenger 和 cppgc 的配置本身）。
- **跨越每一類的成本**（最內層就是這類函式的時間，8 個 suite 那筆量的）：配置 inclusive 63 / 15、memcpy 這類 40 / 8、kernel 的 page fault 25 / 18、rusty_v8 的 FFI shim 11 / 0、hash table 10 / 0。
- **情境的估計**（CPU 時間的幾何平均，今天 1.815；要對應比較表的分數再乘約 1.11）：kun 的 Rust 程式快 10%（PGO、purge delay）1.69；配置、複製、page fault 的差補一半 1.75；FFI、V8 API、GC 機制的差補一半 1.77；三個一起 1.59；V8 以外每一類都做到 Chrome 的 1.3 倍 1.32；V8 以外每一類都追平 1.13；連 JS 也追平 1.00。

### 用 samply 取樣 Chrome：差最多的 8 個 suite 照 kun 的分類比（2026-10-24，Windows 11）

使用者要求量的。Chrome 和 kun 在同樣的取樣下（samply 經過 ETW，1000 Hz，以 CPU 時間計、連 kernel，只算計時區間裡 renderer 的 main thread）各跑 5 個 iteration；kun 用「RFC 0081 和 RFC 0083 之後的取樣」那次的樣本（`ce849649`）。腳本不在 repo 裡。

- **符號**：Chrome for Testing 的 PDB 不在 Chromium 的 symbol server 上（152.0.7977.82 的 `chrome.dll.pdb`、`chrome.exe.pdb` 都是 404），所以用這台機器裝的 Chrome 155.0.8059.85（V8 15.5），`chrome.dll.pdb`（5.6 GB）從 Chromium 的 symbol server 下載。同樣的取樣下 Chrome for Testing 152 也跑了一次：8 個 suite 合計 391.2 對 155 的 389.9 ms，每個 suite 差 2% 以內，JIT 出來的程式碼本身 34.3 對 33.9 ms，所以拿 155 的分類代表 152；只是 152 沒有符號，IC、builtin、runtime 的拆法沒辦法對照。
- **怎麼錄**：以系統管理員權限（ETW 要）啟動的 Chrome 會把自己重新啟動成一般權限，CDP 的 pipe 跟著斷，要加 `--do-not-de-elevate`；由 samply 啟動 Chrome 時 pipe 的 handle 也傳不過去，所以先照平常的方式啟動 Chrome，再用 `samply record -p` 接上（它跟著錄 Chrome 之後開的 process），結束時送 Ctrl+Break。samply 把 renderer 的 main thread 叫做 `chrome.exe`，用 trace 的 track descriptor 裡 `CrRendererMain` 的 tid 找出來。錄取樣時 Chrome 的合計 389.9 比沒錄時比較表的中位數合計 357.4 多 9%。
- **分類**：和 kun 的相同（最內層說得出種類的 frame；V8 自己的 frame 往外看；配置算給呼叫它的）。Blink 的 frame 照類別名稱分，先去掉 template 參數（`HashTable<cppgc::…>` 不算 GC）：painter 算 paint，style 在文字之前（`InlineCSSStyleDeclaration`、`FontBuilder`），inline layout、shaping、字型、HarfBuzz、ICU 算文字（kun 的 `layout::text` 和 parley），其他 layout 算 box layout；DOM 類別上的 `RecalcStyle`、`AttachLayoutTree` 算 style 和 box layout（kun 的這些在 `style`、`layout`）；cc、base、WTF 的字串和容器不算種類，交給呼叫它的。kun 這邊用同一個腳本重分，和上次差不到 0.6 ms（Chrome 的配置函式的名字加進了配置）。

每個 iteration 的 ms，8 個 suite 合計（kun / Chrome，差，倍數）：

| 種類 | kun | Chrome | 差 | 倍數 |
|---|---|---|---|---|
| JS | 181.0 | 133.7 | +47.3 | 1.4 |
| DOM | 120.2 | 51.8 | +68.3 | 2.3 |
| 文字 | 88.4 | 56.2 | +32.2 | 1.6 |
| style | 87.0 | 36.7 | +50.3 | 2.4 |
| box layout | 76.0 | 38.7 | +37.3 | 2.0 |
| V8 API | 40.3 | 5.7 | +34.6 | 7.1 |
| HTML parse | 39.8 | 14.0 | +25.8 | 2.8 |
| bindings | 26.6 | 13.8 | +12.8 | 1.9 |
| GC | 25.6 | 12.4 | +13.2 | 2.1 |
| paint | 23.5 | 20.0 | +3.5 | 1.2 |
| canvas 的 raster | 4.5 | 4.8 | −0.2 | 0.9 |
| 其他 | 21.3 | 2.0 | +19.3 | |
| 合計 | 734.2 | 389.9 | +344.3 | 1.88 |

每個 suite（kun / Chrome）：

| 種類 | jQuery | Next | Nuxt | Stockcharts | TipTap | Perf-Dashboard | chart.js | ES6-Webpack |
|---|---|---|---|---|---|---|---|---|
| JS | 30.5 / 20.1 | 21.1 / 16.4 | 16.2 / 13.0 | 41.8 / 35.6 | 13.9 / 8.5 | 17.6 / 11.9 | 26.1 / 19.9 | 13.9 / 8.3 |
| DOM | 67.4 / 31.1 | 8.0 / 5.2 | 5.7 / 3.1 | 7.5 / 3.2 | 1.8 / 1.6 | 9.7 / 2.5 | 5.1 / 0.0 | 14.9 / 5.1 |
| 文字 | 1.8 / 1.8 | 8.8 / 7.4 | 6.7 / 6.5 | 4.9 / 2.2 | 56.2 / 28.2 | 7.2 / 4.6 | 2.1 / 4.4 | 0.6 / 1.2 |
| style | 5.7 / 3.3 | 23.2 / 10.1 | 21.6 / 7.9 | 12.1 / 6.6 | 1.4 / 0.9 | 9.1 / 4.2 | 3.7 / 0.0 | 10.2 / 3.8 |
| box layout | 1.8 / 1.3 | 28.9 / 16.5 | 24.6 / 9.4 | 11.5 / 2.4 | 1.3 / 2.0 | 6.5 / 5.8 | 0.4 / 0.2 | 1.0 / 1.0 |
| V8 API | 14.5 / 1.2 | 3.1 / 0.3 | 2.9 / 0.6 | 2.5 / 1.0 | 1.4 / 0.3 | 4.3 / 1.1 | 9.2 / 0.0 | 2.4 / 1.2 |
| HTML parse | 34.1 / 11.0 | 0.0 / 0.0 | 0.0 / 0.0 | 0.0 / 0.0 | 0.8 / 0.6 | 0.0 / 0.2 | 0.0 / 0.0 | 4.9 / 2.3 |
| bindings | 10.5 / 6.9 | 4.3 / 1.8 | 1.2 / 0.6 | 0.6 / 0.6 | 0.6 / 0.8 | 2.6 / 1.5 | 3.9 / 0.8 | 2.9 / 0.9 |
| GC | 15.2 / 4.6 | 2.1 / 0.5 | 0.0 / 1.0 | 1.7 / 1.5 | 0.7 / 0.9 | 1.9 / 1.5 | 1.2 / 1.3 | 2.7 / 1.1 |
| paint | 0.7 / 1.4 | 5.2 / 7.3 | 6.4 / 5.0 | 1.0 / 1.8 | 5.6 / 0.7 | 0.8 / 2.0 | 0.2 / 0.2 | 3.6 / 1.6 |
| 合計 | 183.2 / 82.7 | 110.7 / 65.6 | 87.9 / 47.4 | 86.0 / 54.9 | 83.6 / 45.3 | 64.8 / 36.6 | 59.3 / 30.5 | 58.6 / 26.8 |

- **呼叫進瀏覽器的部分**（DOM、V8 API、bindings）187.1 / 71.3，差 115.8，仍是最大的一塊；「差最多的 8 個 suite 在兩邊的各階段」用 trace 估的 Chrome「最多約 51」偏低，差也從約 135 變成約 116。Chrome 的 DOM 最多的是 jQuery 的建節點和插入（`ChildrenChanged`、`ParserDidSetAttributes`、`RemovedFrom`、`SetInnerHTMLInternal`、`Text::Create`），V8 API 只有 `Object::Wrap` 1.7。
- **JS 不是一樣久**：181.0 / 133.7（1.35 倍，差 47.3）。拆開（kun / Chrome）：JIT 出來的程式碼 40.4 / 34.8、V8 的 C++ runtime 45.4 / 32.0、IC 的 builtin 33.6 / 24.3、其他 builtin 33.5 / 27.4、Sparkplug 的 builtin（`_Baseline`）12.8 / 5.2、JSON 8.9 / 5.0、編譯和 parse 6.5 / 5.0。幾乎每一項都慢 1.2–1.5 倍，沒有單一的慢路徑。runtime 裡 kun 多的：`JsonParse` 8.1 / 5.5、`CompileLazy` 6.2 / 4.3、`StackGuard` 和 `StackGuardWithGap` 5.8 / 3.9、`StartMaglevOptimizeJob` 2.9 / 1.5、`Intl.NumberFormat` 的建構 1.9（kun 沒有載入 ICU 的資料；Chrome 的不在前面）。`_Baseline` 多一倍多（`LoadICGenericBaseline` 10.1 / 4.6）表示 kun 的程式碼停在 Sparkplug 比較久，原因還沒查（兩邊綁在同樣的 CPU）。上面那筆「JS 本身兩邊差不多」看的是 `--profile` 裡個別的 JS 函式，差主要在 JIT 的程式碼以外，那種比法看不出來。
- **其他種類**：style 2.4 倍（Next、Nuxt 差 13 左右）、box layout 2.0 倍（Next、Nuxt、Stockcharts）、文字 1.6 倍（TipTap 56.2 / 28.2 佔大半；Chrome 的字型和 HarfBuzz：`SkFontGetGlyphWidthForHarfBuzz` 4.4、`SkFontGetBoundsForGlyphs` 2.3、`hb_shape_plan_execute` 2.2）、HTML parse 2.8 倍（jQuery 34.1 / 11.0，Chrome 都在 `HTMLFastPathParser`）、GC 2.1 倍（jQuery 15.2 / 4.6）；paint 和 canvas 的 raster 差不多。「其他」kun 的是 `renderer`、`cc`、`base` 這些 crate，Chrome 的 cc、base 交給呼叫它的，不完全對等。

### 每個 suite 的節點 id 表的大小（2026-10-24，Windows 11）

使用者要求量的：`--browser kun --suites <一個>`，20 個預設的 suite 各 10 個 iteration（`ce849649`，暫時在 `NodeIdMap::prune` 裡印出表的大小，已還原；時間不比）。每個 suite 都有 prune；表最大時 TodoMVC-jQuery 38.5 萬項、ES6-Webpack 30.1 萬、ES5 26.7 萬（雜湊表 4.7 MB），其他 TodoMVC 的 Complex-DOM 約 7 萬（1.2 MB），其餘更小，Charts-chartjs 不到 1024。逐個 suite 的表在 `rfcs/0085-optimization-roadmap/details.md` 的「節點 id 表的大小」。

### 差最多的 8 個 suite 在兩邊的各階段（2026-10-24，Windows 11）

同一組 suite，`--trace --iterations 5` 和 `--profile --iterations 5`（`ce849649`；使用者要求量的）。每個 iteration 的 ms，8 個 suite 加總。

- **trace 的各階段**（kun / Chrome）：script 424.2 / 232.0、style 82.3 / 31.1、box tree 79.4 / 6.6、layout 72.5 / 80.5（kun 在 box tree 裡 shaping，Chrome 在 layout 裡；兩者合計 151.9 / 87.1）、paint 26.9 / 31.8、parse 92.6 / 100.8、GC 9.8 / 11.0。
- **錄 trace 讓 Chrome 慢得多**：各階段加起來 kun 787.7（沒錄的取樣 734.2，+7%），Chrome 494.4（沒錄的中位數合計 357.4，+38%）。多在 parse：Chrome 的 TodoMVC-jQuery 的 parse 82.0 比它沒錄時整個 suite 的 76.4 還多，ES6-Webpack 18.0 對整個 suite 24.6；其他 suite 多 9–22%（chart.js 50%）。所以 Chrome 的 parse 不能用，其他階段的倍數是下限。
- **JS 本身兩邊差不多**：`--profile` 裡不呼叫 DOM 的函式兩邊相近（kun / Chrome：Perf-Dashboard 的 `SampleVarianceUpperTriangularMatrix` 3.0 / 3.7、`_renderLoop` 1.7 / 2.2，Stockcharts 的 `bt` 4.4 / 4.6，chart.js 的 `prepare` 1.4 / 1.3、`updateElements` 1.1 / 1.5），「假設 JS 本身一樣久」站得住。
- **script 裡呼叫進瀏覽器的部分**：kun 的取樣裡 DOM 119.6、V8 API 40.3、bindings 26.6，合計約 186；Chrome 的 script 232.0（錄 trace 時）減去 JS 本身 180.8，最多約 51。約 3.7 倍以上，差約 135 ms，是 8 個 suite 的差（約 377）裡最大的一塊。
- **`--profile` 的 API 函式的節點**：kun 的 getter、setter 沒有自己的節點（算在呼叫它的 JS 函式），共用路徑的成員共用一個節點（V8 記成 `fromRect`）；Chrome 的 Fast API call 也沒有節點。所以照 API 函式拆 script 兩邊都拆不乾淨，沒有用。`getBoundingClientRect`（runner 的 `layout` 用它強迫 layout）kun / Chrome：Next 39.6 / 12.2、Nuxt 36.3 / 9.8、TipTap 31.5 / 0.3、Stockcharts 24.2 / 6.8。
- `--trace` 寫完 `phases.md` 之後，把它印到 console 時在 Windows 上失敗（cp950 編不了 `−`），報告本身寫好了。

### RFC 0081 和 RFC 0083 之後的取樣：差最多的 8 個 suite（2026-10-24，Windows 11）

`bench.py speedometer --sample --iterations 5`（samply 經過 ETW，以 CPU 時間計、連 kernel，只算 renderer 的 main thread；使用者要求量的），`main` 的 `ce849649`，比較表裡差最多的 8 個 suite。計時區間裡 main thread 的 CPU 時間每個 iteration：jQuery 183.2、Next 110.7、Nuxt 87.9、Stockcharts 86.0、TipTap 83.6、Perf-Dashboard 64.8、Charts-chartjs 59.3、ES6-Webpack 58.6 ms（合計 734.2），和比較表的中位數差在 8% 以內。這 8 個 suite 在 Chrome 上合計 357.4 ms。

- **分類**（每個樣本照最內層說得出種類的 frame 分；V8 自己的 frame 往外看到 JS 的函式算 JS、看到 V8 的公開 API 算 V8 API；配置算給呼叫它的那一種，配置本身合計 64.2，DOM 12.6、HTML parse 11.4、文字 9.2、box layout 7.0、style 6.4、paint 5.7）：JS 180.8、DOM 119.6、文字 88.4、style 87.0、box layout 76.0、V8 API 40.3、HTML parse 39.8、bindings 26.6、GC 25.0、paint 23.5、其他 kun 21.3、canvas 的 raster 4.5 ms。分類的腳本不在 repo 裡。
- **和 10-23 比**（那次 7 個 suite、沒有 ES6-Webpack，合計 843.5）：canvas 的 raster 113.3 → 4.5（移到 raster 的 thread，不在 main thread 的樣本裡），文字 141.0 → 88.4（TipTap 90.2 → 51.1，不含配置）；DOM、box layout、style 和那時差不多。
- **兩邊是同一個 V8，假設 JS 本身一樣久**：kun 在 JS 以外約 553 ms，Chrome 約 357.4 − 180.8 ≈ 177 ms，約 3 倍。
- **每個 suite 最多的**：
  - jQuery：DOM 63.4，其中 `create_node` 13.6（inclusive 22.7，幾乎都是 HTML 的 fast path 建節點）、`dom_api::collection::item` 9.1（`HTMLCollection` 的 index）；HTML 的 fast path 25.1（`scan` 9.9、`build` 7.9、`decode` 5.8）；V8 API 14.3（建 wrapper 時 `interface_object` 每次 `FunctionTemplate::GetFunction` 2.7、`ObjectTemplate::NewInstance` 1.9、`Object::Wrap` 1.5）；GC 14.2。
  - Next、Nuxt：box layout 27.5、23.1（taffy 的 `compute_preliminary` 4.9、3.9，`extend_overflow` 3.1、3.5：兩個合計 6.6 裡有 3.8 是 `renderer::scroll::container_nodes` 每次重算 `scrollable_overflow`），style 21.3、19.6（`cascade` 5.5、4.7）。
  - TipTap：文字 51.1（`shape_text` 6.7、`Analyzer::analyze` 5.2、`WordRuns::flush` 5.0、`LineState` 的 `clone` 4.9）。
  - Stockcharts：JS 41.4，style 10.8，box layout 10.1（SVG 的 `stroke_bbox` 2.2）。
  - ES6-Webpack：JS 13.3（近一半是 JSON 的 parse 和 stringify），DOM 12.6，style 9.6（`SelectorQuery::query` 4.7、`has_compound` 2.2）。
  - chart.js：JS 25.7，V8 API 8.9（canvas 的呼叫轉換參數）。
  - Perf-Dashboard：JS 17.0，style 8.7，DOM 8.5（canvas 記錄 op）。

### RFC 0081 和 RFC 0083 之後的比較表（2026-10-24，Windows 11）

`bench.py speedometer`（20 個 default suite，各 10 個 iteration；使用者要求量的），`main` 的 `ce849649`，環境和 10-23 那次相同（i9-13900K、兩個瀏覽器綁在 0–15、kun 的 `perf` profile、Chrome for Testing 152.0.7977.82）。20 個都有效。Chrome 的中位數合計 556.9 ms，和 10-23 下午的 543.6 ms 差 2.4%，不是 Chrome 整段變慢的那種情況。

- 中位數合計 kun 1140.0 ms、Chrome 556.9 ms（2.05；10-23 是 1293.1 / 543.6，2.38），比值的幾何平均 2.02（10-23 是 2.19）。
- 比值（括號裡是 10-23 的）：WebComponents 2.73（2.73）、ES6-Webpack-Complex-DOM 2.57（2.50）、Lit-Complex-DOM 2.52（2.53）、TodoMVC-jQuery 2.51（2.69；191.6 / 76.4）、Charts-chartjs 2.38（5.06；57.2 / 24.1）、Svelte-Complex-DOM 2.17（2.09）、NewsSite-Nuxt 2.14（2.25；90.5 / 42.2）、ES5 2.12（2.04）、Preact-Complex-DOM 2.09（1.98）、Backbone 1.98（1.92）、Perf-Dashboard 1.97（3.85；66.3 / 33.7）、Vue 1.92（1.72）、Editor-TipTap 1.90（2.87；80.3 / 42.2）、NewsSite-Next 1.89（2.02；116.4 / 61.7）、Angular-Complex-DOM 1.82（1.78）、React-Complex-DOM 1.79（1.76）、React-Stockcharts-SVG 1.73（1.71；90.7 / 52.5）、React-Redux 1.68（1.55）、observable-plot 1.65（1.68）、CodeMirror 1.41（1.46）。
- 變多的只有 chart.js、Perf-Dashboard（canvas 的 raster 移出 main thread）和 TipTap（文字），其他的在兩次量測的起伏裡（輕的 suite 的比值兩次可以差一成以上，例如 Vue 1.72 → 1.92；這次沒有拆開看是哪一邊）。
- 每個 suite 的差（kun − Chrome，ms）：jQuery +115.2、Next +54.7、Nuxt +48.3、ES6-Webpack +38.8、Stockcharts +38.2、TipTap +38.1、chart.js +33.1、Perf-Dashboard +32.6（這 8 個合計 399.0，總差 583.1 的 68%），ES5 +28.5、WebComponents +20.5、observable-plot +18.0，其他 9 個各在 17 ms 以內。10-23 前 8 個是差距的八成，差距比較分散了。

### RFC 0083 的第 1 到 3 段之後：Editor-TipTap 的文字（2026-10-24，Windows 11）

`--browser kun --suites Editor-TipTap`，三個版本輪流各 5 次（每一輪換順序，10 個 iteration）：RFC 0083 開始之前的 `main`（`1e670a6a`）、第 2 段做完的 `b331feaa`、第 3 段做完的 `6f580a90`。三個版本釘的 parley fork 不同，`--against` 只能比釘同一份 `../third_party` 的版本，所以舊的兩個各自另開 worktree、配上自己的 fork 的 worktree（其他的 third_party 用 junction）建 `perf` profile。三個版本每個 test 之後的摘要都相同。

- 整個 suite 的中位數：120.2 ms（119.7–121.2）→ 102.2 ms（101.6–103.7，−15.0%）→ 79.8 ms（79.0–80.7，−33.6%）。每次的中位數 118.9–121.7、100.7–106.4、78.3–82.1，沒有重疊。
- 每個 test 的 sync / async：Long 59.8 / 2.9 → 46.8 / 6.4 → 38.6 / 3.2 ms，Highlight 55.1 / 2.4 → 43.0 / 6.2 → 34.7 / 2.9 ms。第 2 段把放 glyph 搬到 paint，async 多了約 3.5 ms，第 3 段的 paint 迴圈把它拿回來。
- 這次沒有量 Chrome：和 2026-10-23 的 41.3 ms 比，比值從 2.87 變成約 1.93。

`6f580a90` 第一次建 `perf` profile 時 rustc 在編 `layout` 時 crash（`STATUS_ILLEGAL_INSTRUCTION`，LTO），重建一次就過了，沒有重現。

### RFC 0081 之後：canvas 的 raster 移出 main thread（2026-10-23，Windows 11）

`--against 29781da1 --suites Charts-chartjs,Perf-Dashboard`（RFC 0081 開始之前的 `29781da1` 和做完的 `ef62ef3e` 輪流各 3 次，10 個 iteration）：Charts-chartjs 120.8 ms（119.8–123.6）→ 57.7 ms（56.6–59.2），−52.2%；Perf-Dashboard 128.5 ms（127.1–130.2）→ 68.4 ms（67.8–69.1），−46.7%；兩個 suite 的幾何平均 0.504。記憶體的高峰 535 → 531 MB、534 → 538 MB。每次的中位數之前 119.7–130.9、之後 57.3–69.0，沒有重疊。和同一天下午 Chrome 的 24.1、33.2 ms 比，從 5.06、3.85 倍變成 2.39、2.06 倍。剩下的差沒有取樣。

### `Window.scrollTo(0, 0)` 不更新 layout（2026-10-23，Windows 11）

`--against HEAD --suites NewsSite-Nuxt --runs 5`（`814baa04` 和加上這個修正的工作目錄輪流各 5 次）：97.3 ms（96.6–98.0）→ 96.9 ms（96.0–97.6），−0.4%，在起伏裡。`--profile --suites NewsSite-Nuxt`（10 個 iteration）：Nuxt 的 `Vm` 31.6 → 0 ms，JS 函式合計 95.6 → 64.8 ms，`(program)` 6.9 → 38.2 ms，計時區間 102.6 → 103.2 ms；runner 的 `layout` 35.5 → 35.6 ms 不變。原本在 `scrollTo` 裡的 layout 移到下一個 frame 的 rendering，工作量一樣：Nuxt 在 `scrollTo` 之前改了 DOM，這次 layout 不管在哪裡做都省不掉，Blink 也是在 frame 裡做。Nuxt 和 Chrome 的差在 kun 的 layout 本身：每個 iteration 兩次，各約 32 到 36 ms；Chrome 的 runner 的 `layout` 是 12.1 ms，frame 的 rendering 算在 `(program)` 的 14.4 ms 裡。

### RFC 0072 到 0078 之後，Windows 上的比較表、照 JS 函式的對照、取樣和網頁版（2026-10-23，Windows 11）

- **環境**：2026-10-23，Windows 11 Home（26200），Intel Core i9-13900K（桌機），電源設定「平衡」；使用者要求量的。`main` 的 `71e93dcf`（RFC 0072、0073、0074、0076、0078 做完，0079 剛排程）；兩個瀏覽器綁在 0–15（P-core）。kun 是 `perf` profile（release 的 prebuilt V8 `kun-v152.2.0-17`），Chrome for Testing 152.0.7977.82，kun 的 GPU 是 AMD Radeon RX 7900 XTX（DX12）。前一天 `cargo clean` 過，第一次的 build 是完整的。本機的 HTTP server 照例印了 `ConnectionAbortedError`（瀏覽器在它送完之前斷了連線），不影響結果。
- **早上那次的 Chrome 整段變慢，數字作廢**：早上的比較表（中位數合計 kun 1319.8 ms、Chrome 838.4 ms，比值的幾何平均 1.36）和 `--profile`（區間內 kun 1409.6、Chrome 957.3）裡，Chrome 每個 suite 都比下午重量的慢（合計 838.4 對 543.6），輕的 suite 慢兩倍多（Svelte-Complex-DOM 20.9 對 7.5、Preact-Complex-DOM 21.9 對 8.2、WebComponents 24.8 對 10.9），同步和非同步的時間一起慢（Svelte 的 `Adding100Items` 同步 2.2 對 0.7 ms、非同步 6.6 對 3.1 ms）；同一次量的 kun 和下午差不多（合計 1319.8 對 1293.1）。下午另外有兩次 Chrome 停在一個 suite 裡、renderer 每秒只用約 6% 的 CPU：一次在真的視窗裡跑網頁版，一次在 `--profile` 的 TodoMVC-React-Complex-DOM（等到工具的期限前停掉；當時期限是 690 秒，之後改成頁面 30 秒加每個 iteration 10 秒）。原因沒找到（Chrome 沒有換版本，也一樣綁在 P-core 上；猜測是 Windows 對 process 降頻的 EcoQoS 或螢幕關掉時的排程，沒有驗證）。下面是下午重量的數字；`--sample` 只量 kun，早上的照用。
- **`bench.py speedometer`**（下午重量，20 個 default suite，各 10 個 iteration）：20 個都有效；中位數合計 kun 1293.1 ms、Chrome 543.6 ms（2.38），比值的幾何平均 2.19。比值：Charts-chartjs 5.06（122.0 / 24.1）、Perf-Dashboard 3.85（128.2 / 33.2）、Editor-TipTap 2.87（118.6 / 41.3）、WebComponents 2.73、TodoMVC-jQuery 2.69（194.3 / 72.2）、Lit-Complex-DOM 2.53、ES6-Webpack-Complex-DOM 2.50、NewsSite-Nuxt 2.25（97.3 / 43.3）、Svelte-Complex-DOM 2.09、ES5 2.04、NewsSite-Next 2.02（125.4 / 62.0）、Preact-Complex-DOM 1.98、Backbone 1.92、Angular-Complex-DOM 1.78、React-Complex-DOM 1.76、Vue 1.72、React-Stockcharts-SVG 1.71（88.2 / 51.7）、observable-plot 1.68、React-Redux 1.55、CodeMirror 1.46。沒有比 Chrome 快的 suite。
- **`bench.py speedometer --profile`**（下午重量，20 個 suite，各 10 個 iteration，40 次都有效）：每個 iteration 的 ms、20 個 suite 加總（kun / Chrome）：JS 函式 1117.2 / 521.2、`(program)` 241.8 / 104.7、`(idle)` 7.5 / 10.0、GC 11.6 / 16.7、區間內的合計 1378.1 / 652.6（10-22 同一台機器上是 1426.3 / 666.7）；區間外 4159.6 / 1795.3。
- **每個 suite 區間內的差**（kun − Chrome）：TodoMVC-jQuery +115.6、Perf-Dashboard +94.7、Charts-chartjs +92.0、Editor-TipTap +77.4、NewsSite-Next +65.9、NewsSite-Nuxt +55.5、ES6-Webpack +37.5、React-Stockcharts-SVG +36.8（這 8 個合計 575.4，差距的八成），ES5 +22.1、WebComponents +18.9、observable-plot +16.4、Lit +16.1、React-Complex-DOM +14.1、Angular +13.9、Backbone +11.6、React-Redux +10.6、Vue +8.0、Preact +6.8、Svelte +6.4、CodeMirror +5.4。後 12 個合計約 +150：JS 函式 +96.7（分散在很多函式，沒有一個超過 4 ms，ES5 的 `View._replaceContentWithHtml` 的 innerHTML +11.0 除外）、`(program)` +59.0。
- **差最多的函式**（kun / Chrome）：chart.js 的 `drawPointLegend` 71.8 / 2.6、jQuery 的 `.html()` 104.0 / 36.3 和 `cleanData` 37.8 / 17.8、TipTap 打包的 runner 的 `layout` 51.7 / 4.2 和 `setValue` 50.1 / 14.0、Nuxt 的 `Vm` 31.6 / 0.0、Next 的 `element.scrollTo` 的 effect 40.8 / 15.1、runner 的 `layout` 在 Next 38.8 / 14.5 和 Nuxt 35.5 / 12.0、Stockcharts 的 `z` 23.9 / 4.0、ES6-Webpack 的 `render` 19.6 / 6.5、chart.js 的 `draw` 14.0 / 1.3、ES5 的 `View._replaceContentWithHtml` 21.1 / 10.1、Perf-Dashboard 的 `_updateCanvasSizeIfClientSizeChanged` 15.3 / 5.9。kun 的成員：共用路徑 173.5（canvas 的 `fill`、`stroke` 65.1，Next、Nuxt 的 `scrollTo` 40.8、31.6）、`getBoundingClientRect` 149.2（TipTap 50.9、Next 和 Nuxt 的 runner 33.7、32.3）、`querySelectorAll` 18.3、`setAttribute` 13.8、`querySelector` 10.8、`elementFromPoint` 8.1。工具注入的 `summarize`（每個 test 之後的 DOM 摘要）在兩邊的計時區間裡各有約 1–4 ms，兩邊差不多。
- **`bench.py speedometer --sample`**（早上，samply 經過 ETW，當時差最多的 7 個 suite 各 5 個 iteration，1 分鐘）：計時區間裡 main thread 的 CPU 時間每個 iteration jQuery 183.4、Next 122.3、Charts-chartjs 119.9、Perf-Dashboard 119.7、TipTap 115.0、Nuxt 95.9、Stockcharts 87.3 ms。每個樣本照最內層屬於哪一種工作分（合計 843.5 ms）：JS 本身 173.5、文字（shaping、斷行）141.0（TipTap 90.2）、DOM 128.2（jQuery 73.8）、canvas 的 raster 113.3（chartjs 61.2、Perf-Dashboard 52.1）、box layout 81.7（Next 30.9、Nuxt 29.3）、style 77.3、bindings 53.4、HTML parse 32.4（jQuery 31.3）、paint 23.5、GC 16.7。
- **網頁版，真的視窗**：Speedometer 3.1 自己的頁面（`index.html?startAutomatically`，20 個 suite 輪流在同一頁、10 個 iteration），兩個瀏覽器都是真的視窗（kun `--automation-pipe --window`，Chrome 不加 `--headless`），1200×900、綁在 P-core 上，輪流各 3 次（量的程式不在 repo 裡）：kun 18.3、18.4、18.4 分，Chrome 44.2、44.1、44.3 分（2.41 倍）。headless 的 Chrome 跑網頁版也是 44 分。和下午的比較表比：Chrome 每個 suite 差在 5% 以內；kun 在真的視窗裡慢約一成，主要是 canvas：螢幕縮放 150%，devicePixelRatio 是 1.5（工具的 headless kun 是 1），chart.js 照它放大 canvas，Charts-chartjs 122.0 對 206.7 ms、Perf-Dashboard 128.2 對 158.8 ms，其他的 suite 差 0–15%。
- 分析和方向寫在 `docs/performance-gaps.md`。

### RFC 0069 之後，Windows 上照 JS 函式的對照和取樣（2026-10-22，Windows 11）

- **環境**：2026-10-22，Windows 11 Home（26200），Intel Core i9-13900K（桌機），電源設定「平衡」；使用者要求量的。`main` 的 `f06bce27`（RFC 0070 做完、RFC 0071 的草稿之後；比 Fedora 那次的 `f292d249` 多 RFC 0070 的元素介面、global 的 interface object 在第一次讀時才建、node 的 collection 存在一個 list 裡）；兩個瀏覽器綁在 0–15（P-core）。kun 是 `perf` profile，Chrome for Testing 152.0.7977.82。機器、作業系統和 commit 都和 Fedora 那次不同，絕對的時間不能直接比。
- **第一次 build 失敗**：本機的 `../third_party/rusty_v8` 還在 `7d7fc555`，沒有 fork 的 `CpuProfiler`；`bench.py` 開始前不像 `test.py` 那樣跑 `ci.py third-party`。跑過 `python3 scripts/ci.py third-party`（切到 `9e83767e`）之後正常。
- **`bench.py speedometer --profile`**（20 個 default suite，各 10 個 iteration）：每個 iteration 的 ms、20 個 suite 加總（kun / Chrome）：JS 函式 1167.1 / 530.2、`(program)` 242.1 / 108.8、`(idle)` 4.9 / 12.8、GC 10.2 / 14.9、區間內的合計 1426.3 / 666.7（2.14 倍）；區間外 4135.1 / 2682.2。區間的誤差上限 0.42–1.03 ms。開始時本機的 HTTP server 印了三次 `ConnectionAbortedError`（瀏覽器在它送完之前斷了連線，第一個 suite 之前），20 個 suite 兩邊都完成。
- **每個 suite 區間內的差**（kun − Chrome）：TodoMVC-jQuery +124.2、Perf-Dashboard +97.9（`(program)` +60.8）、Charts-chartjs +97.8、Editor-TipTap +75.0、NewsSite-Next +62.9、NewsSite-Nuxt +55.5、ES6-Webpack-Complex-DOM +42.1、React-Stockcharts-SVG +37.0，其他在 +7 到 +22。
- **差最多的函式**：TipTap 的 `resetScrollPos` +96.4、Charts-chartjs 的 `drawPointLegend` +71.6（74.7 / 3.1）和 `draw` +15.4、jQuery 的 `.html()` +64.7（107.7 / 43.1）和 `cleanData` +25.1、Nuxt 的 `Vm` +31.3、Next 和 Nuxt 的 runner 的 `layout` +28.3、+23.7、Next 呼叫 `element.scrollTo({top: 0, left: 0})` 的 effect +22.8（38.1 / 15.2）、Stockcharts 的 `z` +18.3、ES6-Webpack 的 `render` +15.8、ES5 的 `_replaceContentWithHtml` +13.0。kun 的成員：`getBoundingClientRect` 202.3、共用路徑 176.6（canvas 的 `fill`、`stroke` 66.4，Next、Nuxt 的 `scrollTo` 37.9、30.9）、`querySelectorAll` 19.4、`setAttribute` 11.7。
- **`bench.py speedometer --sample`**（samply 經過 ETW，以 CPU 時間計、連 kernel；Charts-chartjs 和另外 7 個差最多的 suite，各 5 個 iteration）：計時區間裡 main thread 的 CPU 時間每個 iteration jQuery 193.2、Charts-chartjs 128.2、Perf-Dashboard 126.9、Next 125.6、TipTap 116.0、Nuxt 97.0、Stockcharts 87.2、ES6-Webpack 67.2 ms，和 `--profile` 的 kun 相近，時間幾乎都在用 CPU。依 crate 算的 inclusive time：Charts-chartjs 的 `renderer::canvas::flush` 62.1（57.9 是錄滿 10,000 個 op 時的 flush，59.5 在 `raster::draw_into` 的 Skia）；Perf-Dashboard 的 Skia 55.3（全在 frame 之前的 `update_rendering` 畫 canvas）；TipTap 的 `layout` 92.2（parley 68.4）；Next、Nuxt 的 `layout` 51.1、42.7（taffy 29.0、25.1）和 `style` 23.6、18.8；jQuery 的 `html` 83.2、`dom` 65.9。Windows 的 heap（`RtlpLowFragHeapAllocFromContext`、`RtlFreeHeap`）8 個 suite 合計 36.8 ms，jQuery 13.9，其中 11.8 是 `create_node` 的 `cppgc__WeakPersistent__CONSTRUCT`（每個節點一次 C++ 的 `new`）。分析和方向寫在 `docs/performance-gaps.md`。

### RFC 0069 之後，照 JS 函式的對照（2026-10-22，Fedora 44）

- **環境**：2026-10-22，Fedora 44，AMD Ryzen AI 9 HX 370，接電源，電源設定 `balanced`；使用者要求量的（RFC 0069 的第 18 條）。`main` 的 `f292d249`（RFC 0069 之後，包括 `innerWidth`、`innerHeight` 不更新 layout 的修正）；兩個瀏覽器綁在 0–3、12–15。kun 是 `perf` profile，Chrome for Testing 152.0.7977.82。開始前等 GitLab 的 runner 在這台跑完 CI。
- **`bench.py speedometer --profile`**（20 個 default suite，各 10 個 iteration，6.5 分鐘）：兩邊都每 1 ms 取樣，只算計時的區間，每個 iteration 的 ms、20 個 suite 加總（kun / Chrome）：JS 函式 2010.4 / 1401.5、`(program)` 393.5 / 353.1、`(idle)` 0.6 / 35.8、GC 21.6 / 39.5、區間內的合計 2429.4 / 1830.0；區間外 5559.0 / 2582.5。區間的誤差上限 0.31–0.53 ms。
- **每個 suite 區間內的差**（kun − Chrome）：Charts-chartjs +146.9、Editor-TipTap +90.0、React-Stockcharts-SVG +81.6、TodoMVC-jQuery +64.2、NewsSite-Nuxt +62.0、Perf-Dashboard +37.1、Charts-observable-plot +26.0；NewsSite-Next −28.9，其他在 −2 到 +19。
- **差最多的函式**：TipTap 的 `resetScrollPos` +176.9（`getBoundingClientRect`）、Charts-chartjs 的 `drawPointLegend` +163.5（canvas 的 `fill`、`stroke`）和 `draw` +38.7、NewsSite-Nuxt 的 `Vm` +65.0（`window.scrollTo`）和 runner 的 `layout` +30.1、Stockcharts 的 `z` +44.5（`getBoundingClientRect`）、jQuery 的 `.html()` +41.6 和 `cleanData` +25.4。kun 的成員：`getBoundingClientRect` 351.9、共用路徑 321.1、`setAttribute` 31.7、`parseFromString` 22.9。kun 的 Charts-chartjs（333 ms）比 10-20（約 170 ms）慢約一倍，在 canvas 的等待：同一台、第 1 段的 commit 也一樣，是機器的狀態。分析和方向寫在 `docs/performance-gaps.md`。

### RFC 0065 之後，script 和 style 的差距分析（2026-10-20，Fedora 44）

- **環境**：2026-10-20，Fedora 44，AMD Ryzen AI 9 HX 370，接電源；使用者要求量的。`main` 的 `dc5000da`（RFC 0064 的第 3 段、RFC 0065 之後）；兩個瀏覽器第一次綁在最快的 CPU（0–3、12–15）上，和之前在這台量的不能直接比。kun 是 `perf` profile，Chrome for Testing 152.0.7977.82。
- **`bench.py speedometer`**（20 個 default suite，各 10 個 iteration）：20 個都有效；中位數合計 kun 2446 ms、Chrome 1583 ms（1.54），比值的幾何平均 1.47。比值最大的：Editor-TipTap 2.23、Charts-chartjs 1.96、ES6-Webpack-Complex-DOM 1.86、jQuery 1.86、Lit-Complex-DOM 1.79；NewsSite-Next 0.77。
- **`--trace` 的各階段**（每個 iteration、20 個 suite 加總的 ms，kun / Chrome）：script 1234.7 / 1014.2、style 221.4 / 146.6、box tree 265.6 / 25.0、layout 203.5 / 228.0、paint 85.3 / 182.0、parse 167.7 / 267.4、GC 24.1 / 43.3、canvas 186.8 / 41.2。
- **`--sample` 分類之後**（kun 的 ms）：script 裡 JS 本身 662.8、呼叫 DOM 571.9（bindings 131.5、DOM 122.2、V8 API 63.4、配置 52.5、root 46.0、字串 36.4）；style 裡 cascade 64.3、selector 比對 38.2、invalidation 和走訪 31.2、`ComputedStyle` 的建立和複製 30.8。分析和方向寫在 `docs/performance-gaps.md`。

### RFC 0064 的第 3 段之後（2026-10-20，macOS）

- **環境**：
  - 2026-10-20，macOS 26.6.2，Apple M4（10 核），接電源；使用者要求量的。
  - `main` 的 `a52210c9`：RFC 0064 的第 3 段（`clip`、`clip-path`、SVG 的 `<clipPath>`），加上 out-of-flow 盒子的繪製順序的修正。
  - `python3 scripts/bench.py speedometer`，20 個 default suite，各 10 個 iteration；kun 是 `perf` profile，Chrome for Testing 152.0.7977.82。
- **suite 總時間的中位數**（ms，kun / Chrome / 比值）：ES5 36.9 / 19.6 / 1.88；ES6-Webpack 46.4 / 19.3 / 2.40；WebComponents 29.8 / 9.4 / 3.16；React 27.9 / 16.7 / 1.68；React-Redux 26.5 / 18.6 / 1.42；Backbone 25.5 / 14.4 / 1.76；Angular 25.2 / 15.1 / 1.67；Vue 20.0 / 13.3 / 1.50；jQuery 144.1 / 55.2 / 2.61；Preact 13.2 / 6.1 / 2.19；Svelte 12.7 / 5.4 / 2.33；Lit 20.9 / 8.2 / 2.55；NewsSite-Next 76.6 / 35.3 / 2.17；NewsSite-Nuxt 62.7 / 29.1 / 2.15；Editor-CodeMirror 14.9 / 11.1 / 1.35；Editor-TipTap 102.1 / 52.9 / 1.93；Charts-observable-plot 35.5 / 23.6 / 1.51；Charts-chartjs 72.5 / 21.3 / 3.41；React-Stockcharts-SVG 65.3 / 38.1 / 1.71；Perf-Dashboard 78.8 / 20.9 / 3.78。
- **合計**：20 個 suite 都有效。kun 938 ms、Chrome 434 ms，合計的比值 2.16，比值的幾何平均 2.07。
- **和第 2 段之後的比較**：
  - kun 的合計 934 → 938 ms（+0.4%）。
  - 合計的比值從 2.24 降到 2.16，幾乎都是 Chrome 的 Editor-TipTap 這次不穩造成的：iteration 從 33.7 到 85.5 ms，中位數 52.9 ms，上次是 34.5 ms。去掉 TipTap，上次是 2.17、這次是 2.19。
  - kun 變慢比較多的：
    - React-Stockcharts-SVG 62.7 → 65.3（+4.1%）：這一段讓它的 series 照 `<clipPath>` 裁切（路徑的 clip node），原本丟掉；
    - WebComponents 28.6 → 29.8（+4.2%）；
    - Vue 18.6 → 20.0（+7.5%）：Chrome 也從 12.4 變成 13.3，比值不變。
  - 其他 suite 在 ±3% 以內。

### RFC 0064 的第 2 段之後（2026-10-20，macOS）

- **環境**：2026-10-20，macOS 26.6.2，Apple M4（10 核），接電源。`main` 的 `386e12b8`（RFC 0064 的第 1、2 段：transform、`object-fit`、`overflow: clip`、`tab-size`、line clamp 和 `-webkit-box`、`contain`），使用者要求量的。`python3 scripts/bench.py speedometer`，20 個 default suite，各 10 個 iteration，kun 是 `perf` profile，Chrome for Testing 152.0.7977.82。
- **suite 總時間的中位數**（ms，kun / Chrome / 比值）：ES5 36.8 / 20.0 / 1.85；ES6-Webpack 47.2 / 19.3 / 2.45；WebComponents 28.6 / 9.6 / 2.99；React 27.8 / 16.8 / 1.66；React-Redux 27.1 / 18.4 / 1.47；Backbone 25.3 / 14.2 / 1.78；Angular 25.0 / 15.3 / 1.63；Vue 18.6 / 12.4 / 1.51；jQuery 144.7 / 57.1 / 2.53；Preact 13.4 / 6.2 / 2.16；Svelte 12.7 / 5.6 / 2.26；Lit 20.9 / 7.9 / 2.62；NewsSite-Next 76.8 / 34.5 / 2.22；NewsSite-Nuxt 62.2 / 29.6 / 2.10；Editor-CodeMirror 15.4 / 11.4 / 1.35；Editor-TipTap 102.6 / 34.5 / 2.97；Charts-observable-plot 35.1 / 23.7 / 1.48；Charts-chartjs 73.1 / 21.0 / 3.49；React-Stockcharts-SVG 62.7 / 38.6 / 1.62；Perf-Dashboard 78.4 / 21.0 / 3.74。
- **合計**：20 個 suite 都有效，kun 934 ms、Chrome 417 ms，合計的比值 2.24，比值的幾何平均 2.10。比值最大的是 Perf-Dashboard 3.74、Charts-chartjs 3.49、WebComponents 2.99、Editor-TipTap 2.97；最小的是 Editor-CodeMirror 1.35、React-Redux 1.47、observable-plot 1.48。
- 這台機器之前沒有完整的比較表（之前的在 Windows 和 Fedora 的機器上），沒有可比的前一次；這一段讓 kun 多畫、多排了原本丟掉的 CSS（例如 NewsSite 的 line clamp 和 `object-fit`），之後的比較以這次為起點。

### RFC 0051 和 RFC 0055 做完之後的比較表（2026-10-18，Windows 11）

- **環境**：2026-10-18，Windows 11（10.0.26200），Intel Core i9-13900K（32 核），桌機、不睡眠（報告的 power 是 unknown：沒有電池）。`main` 的 `80b7069a`（V8 是 `kun-v152.2.0-11`），`python3 scripts/ci.py speedometer`，20 個 default suite，各 10 個 iteration，Chrome for Testing 152.0.7977.82。
- **suite 總時間的中位數**（ms，kun / Chrome / 比值）：ES5 82.9 / 25.0 / 3.31；ES6-Webpack 126.9 / 24.9 / 5.11；WebComponents 52.1 / 11.5 / 4.55；React 51.5 / 20.1 / 2.57；React-Redux 43.1 / 22.0 / 1.96；Backbone 47.0 / 16.5 / 2.85；Angular 47.3 / 18.3 / 2.58；Vue 35.9 / 13.9 / 2.58；jQuery 394.4 / 72.4 / 5.44；Preact 27.6 / 8.3 / 3.33；Svelte 28.9 / 7.5 / 3.86；Lit 37.9 / 10.9 / 3.48；NewsSite-Next 154.4 / 62.2 / 2.48；NewsSite-Nuxt 115.7 / 42.8 / 2.70；Editor-CodeMirror 23.2 / 13.9 / 1.68；Editor-TipTap 139.6 / 41.8 / 3.34；Charts-observable-plot 60.4 / 26.9 / 2.25；Charts-chartjs 155.6 / 25.1 / 6.20；React-Stockcharts-SVG 116.8 / 53.0 / 2.20；Perf-Dashboard 135.8 / 34.0 / 3.99。
- **合計**：20 個 suite 都有效（第一次在完整的量測裡全部有效），kun 1877 ms、Chrome 551 ms，合計的比值 3.41，比值的幾何平均 3.13。
- **和「RFC 0051 的 5c 之後的比較表」比**（`3ac7d2f0`，同一台機器，同樣的 19 個 suite，不含 Editor-CodeMirror）：kun 2220 → 1854 ms，每個 suite 的變化的幾何平均 −15.6%，比值的幾何平均 3.81 → 3.23。變最多的是 Preact −30%、ES6-Webpack −28%、Svelte −27%、React −25%、Lit −23%、NewsSite-Nuxt、Charts-chartjs −22%、Angular −21%、ES5 −20%；React-Redux、Backbone、TipTap、observable-plot、Stockcharts 在 ±3% 內。Chrome 542 → 537 ms，各 suite 在 ±6% 內。

### 結尾的強制換行修好之後的 Editor-CodeMirror（2026-10-18，Windows 11）

`ci.py speedometer --suites Editor-CodeMirror`（10 個 iteration，kun 是 `perf` profile，`main` 的 `ab44da2b` 加上修正，Chrome for Testing 152.0.7977.82）：第一次有效。中位數 kun 23.1 ms（22.5–23.6）、Chrome 13.8 ms（13.3–14.9），比值 1.67；第一個 iteration 40.6、38.6。Long 的 sync 6.9 / 4.2、async 5.3 / 2.4，Highlight 的 sync 10.0 / 6.3、async 0.8 / 0.7 ms。20 個 default suite 現在都有效。

### tile 的 raster 算不算進 Speedometer 的時間（2026-10-18，Windows 11）

- **問題**：kun 的 tile 是 CPU raster（Skia 的 raster 後端，在 compositor thread 上，commit 一到就當場 raster、activate），Chrome for Testing 在這台是 GPU raster（`SystemInfo.getInfo`：Skia Graphite、Dawn 的 D3D11，AMD Radeon RX 7900 XTX，多條 raster thread，canvas 也加速）。raster 慢會不會反過來拖住 main thread、算進 Speedometer 的時間。
- **程式**：main thread 的 commit 只是把 `CompositorTask::Commit` 排給 compositor，不等它；compositor 處理完 commit 之前不送下一個 `BeginMainFrame`。runner 在同一個 frame 排兩個 rAF：sync 在第一個裡量，async 量到第二個裡排的 `setTimeout(0)` 執行，兩段都只算 main thread；測試之間等 rAF 的時間不計時。
- **trace**：`main` 的 `1b3eba12`（RFC 0051 的 5d 之後），`ci.py speedometer --trace --browser kun --iterations 3`，20 個 default suite。區間照 runner 的結構找：有兩個以上 `FireAnimationFrame` 的 main frame，從它的第一個 callback 到 frame 之後第一個 `TimerFire` 的結束，中間沒有別的 frame。每個 suite 9 個區間（兩個 Editor 6 個），合計 6767 ms，扣掉無效的 CodeMirror 6679 ms，和 2026-10-18 的比較表的中位數合計乘 3（6660 ms）一致。
- **結果**：計時的區間裡 main thread 沒有任何 span 開著的時間共 1.6 ms，其中 compositor 在 raster 的 1.3 ms；compositor 的 `TileManager::PrepareTiles` 合計 4076 ms，落在區間裡的 115 ms 和 main thread 的工作平行。**tile 的 raster 沒有算進 Speedometer 的時間**。算進去的是 canvas：main thread 上的 `raster::draw_into` 在區間裡 288.5 ms，chartjs 185 ms（它的區間的 29%）、Perf-Dashboard 103.5 ms（22%）；Chrome 的 canvas 是 GPU 加速的，main thread 只錄指令。
- **`timed_windows` 算多了**：工具的 `timed_windows`（`--sample` 用它只取計時的區間）從任何一個 `FireAnimationFrame` 算起、重疊的合併，會包進 app 自己的 rAF 的 frame 和 frame 之間等 `BeginFrame` 的時間：同一批 trace 合計 9393 ms，比上面多 39%，其中 main thread 閒著 2214 ms。照它算的「計時區間裡的 self time」和取樣都多算了計時以外的工作（這份 `progress.md` 之前的 trace 分析用的是 trace processor 加上同樣的定義）。

### RFC 0051 的 5c 之後的比較表（2026-10-18，Windows 11）

- **環境**：2026-10-18，Windows 11（10.0.26200），Intel 64 Family 6 Model 183（32 核），接電源、不睡眠。`main` 的 `3ac7d2f0`（RFC 0051 的 5c、RFC 0054 的第 1、2 段之後），工作目錄加上 Chrome 的分頁先 `Page.bringToFront` 的修正（不影響 kun）；`python3 scripts/ci.py speedometer`，20 個 default suite，各 10 個 iteration，Chrome for Testing 152.0.7977.82。Chrome 那一側第一次在 Windows 上經過 CDP 的 pipe 跑。
- **suite 總時間的中位數**（ms，kun / Chrome / 比值）：ES5 103.3 / 25.4 / 4.07；ES6-Webpack 177.1 / 24.5 / 7.23；WebComponents 55.9 / 11.5 / 4.86；React 68.4 / 20.8 / 3.29；React-Redux 42.7 / 21.8 / 1.95；Backbone 46.8 / 17.4 / 2.69；Angular 59.9 / 18.6 / 3.21；Vue 39.3 / 13.8 / 2.85；jQuery 486.4 / 76.4 / 6.37；Preact 39.4 / 8.5 / 4.66；Svelte 39.5 / 7.4 / 5.34；Lit 49.4 / 10.7 / 4.62；NewsSite-Next 188.9 / 62.4 / 3.02；NewsSite-Nuxt 149.2 / 42.6 / 3.50；Editor-TipTap 136.7 / 43.1 / 3.17；Charts-observable-plot 60.9 / 26.8 / 2.27；Charts-chartjs 199.4 / 24.3 / 8.21；React-Stockcharts-SVG 116.2 / 52.7 / 2.20；Perf-Dashboard 160.8 / 33.3 / 4.83。Editor-CodeMirror 無效（kun 的第 0 個 iteration 的 `Long` 少兩個 `div`，和 2026-10-17 一樣）。
- **合計**：19 個有效的 suite，kun 2220 ms、Chrome 542 ms，比值的幾何平均 3.81。
- **和 2026-10-17 同一台機器的那次比**（`e3ae99d2`，RFC 0051 的 2c；數字在 `rfcs/0053-allocator/details.md` 的「Windows 上的基準」，同樣 19 個 suite）：kun 2540 → 2220 ms，每個 suite 的變化的幾何平均 −10.7%，比值的幾何平均 4.28 → 3.81。變最多的是 NewsSite-Nuxt −28%、NewsSite-Next −26%、jQuery −20%、Backbone、Preact、Svelte −17%；ES5、ES6-Webpack、Lit、TipTap、兩個 Charts、Stockcharts、Perf-Dashboard 在 ±5% 內。Chrome 537 → 542 ms：那次 Chrome 是用命令列開網址，這次經過 CDP，合計差 1%，換了控制的方式沒有改變 Chrome 那一側的數字。
- **同一天稍早、沒有 `Page.bringToFront` 的那一次不用**：Chrome 的頁面沒有 focus，TipTap 因此無效（RFC 0054 的 `progress.md`）。

### 收尾前最後一次的 trace

- **環境**：2026-10-16，Fedora 44。`main` 的 `d9f9511b`（2d 做完、修掉錄 trace 時 html5ever 的 log 紀錄之後），`python3 scripts/ci.py speedometer --trace --iterations 3`（全部 20 個 default suite，兩邊各一份，274 秒，含編有 frame pointer 的 `perf` profile），Chrome for Testing 152.0.7977.82，AMD Ryzen AI 9 HX 370（24 核），接電源。40 份 trace 都錄完、沒有例外。分析同前：trace processor 算 renderer main thread 上每個名稱的 self time，只算計時的區間（runner 的 `FireAnimationFrame` 到之後第一個 `TimerFire` 的結束），Perf-Dashboard 不算進合計（它的 app 自己用 rAF）。區間合計 kun 14590 ms、Chrome 5204 ms（含錄 trace 的開銷，只看比例）。
- **計時區間裡的 self time**（19 個 suite 合計，kun / Chrome，ms）：JS 和它呼叫的 DOM API（`v8.callFunction`、`FunctionCall`、`RunMicrotasks`、`EventDispatch`）5060 / 2240；HTML 的 parse（`ParseHTML`）1615 / 459；custom element 9 / 11；GC 136 / 144；style 2965 / 418；layout 667 / 681；paint 1318 / 371；其他 554 / 411。
- **`ParseHTML`**（取代「2d-2 之後的 trace」被高估的 5573 / 634）：jQuery 1061 / 286（kun 1800 次 fragment 的 parse，每次 0.59 ms；Chrome 約 1890 次，每次 0.16 ms）、ES6-Webpack 328 / 93、ES5 195 / 64、Backbone 23 / 12、TipTap 8 / 3，約是 Chrome 的 3.5 倍。2d-3 的 jQuery 取樣看得到裡面：html5ever 和 tendril、cppgc 的 handle、配置、`dom::scope`。
- **style**：`Document::UpdateStyle` 1800 ms、`StyleEngine::RebuildLayoutTree` 1165 ms，Chrome 的 `Document::recalcStyle` 339 ms 加 `UpdateLayoutTree` 10 ms，約 8.5 倍；集中在 NewsSite-Nuxt 500 / 54、Next 412 / 120、TipTap 332 / 5、Preact 231 / 21。
- **paint**：kun 的 `raster::draw_into` 321 ms（chartjs 的 canvas）、`Paint` 266 / 108、`PrePaint` 220 / 4、paint 之後 scroll 的 hit test region 204 ms 和 wheel 的 handler region 204 ms（Chrome 沒有對應的步驟，兩個都是每個 frame 走整棵樹，集中在 Complex-DOM 的 suite，每個 20 到 38 ms）、`PaintArtifactCompositor::Update` 102 / 39。原本「`WebFrameWidgetImpl::UpdateLifecycle` 自己 614 ms」主要是這幾步，現在它自己剩 118 ms。
- **其他**：`ThreadControllerImpl::RunTask` 自己 321 / 30 ms（frame 以外的 task 裡沒有 span 的工作，NewsSite 最多），`WebFrameWidgetImpl::BeginMainFrame` 自己 64 ms；Chrome 的 `LayerTreeHost::DoUpdateLayers` 135 ms（cc 這一側），kun 的 3 ms。
- **結論**：差距由大到小是 JS 呼叫的 DOM API（約 2800 ms，節點和 wrapper 的查表、cppgc 的 handle、配置）、style 的重算（約 2550 ms）、fragment 的 parse（約 1150 ms）、paint 之後每個 frame 走整棵樹的兩個 region（約 400 ms）；layout、GC 已經和 Chrome 差不多。優化各自開 RFC。

### 2d-3 的 jQuery 取樣（2026-10-16，Fedora 44）

`--sample --suites TodoMVC-jQuery --iterations 3`（`perf` profile 加 frame pointer，修掉 log 之後，194 秒含編譯）：計時區間裡 12445 個樣本（約 3.1 秒，和量到的一致；修之前 22171 個，41% 是 html5ever 的 log 紀錄）。stack 從 run loop 經過 kun、V8、jQuery 的 JS（`render`、`y.handle`）再到 kun 的 DOM API 都展開得了。fragment 的 parse（`parse_html_fragment`）佔 51%，每次約 0.9 ms，Chrome 的 4 倍左右（不是 trace 看起來的 10 倍）；裡面 html5ever 和 tendril 29%、cppgc 的 handle（`Member::Get`、`Persistent::Get`、`WeakPersistent::Get`）24%、malloc/free/memmove 18%、`dom::scope`（`node_by_id`、`insert_before`、`create_node`）14%、`StoreId` 的 SipHash 4%。self 的前幾名：`_int_malloc` 5.4%、`cppgc__Member__Get` 5.1%、`malloc` 3.8%、`cppgc__Persistent__Get` 3.3%、`hash_one::<&StoreId>` 3.1%。

### 2d-2 之後的 trace（2026-10-16，Fedora 44；`ParseHTML` 被高估）

- **環境**：`0012` 的 `fc760453`，`--trace --iterations 3`（全部 default suite，311 秒，含編 `perf` profile），分析和「RFC 0046、0047 和 RFC 0048 第 1 段之後」那次一樣，kun 的也是 Perfetto 的 protobuf。server 寫回應時 4 次 `BrokenPipeError`（瀏覽器先關了連線），結果都有效。
- **self time**（19 個 suite，kun / Chrome，ms）：JS 5687 / 2233；`ParseHTML` 5573 / 634（**被高估**：錄 trace 時 html5ever 為每個 token 做出 log 的紀錄，2d-3 修掉，正確的數字見最後一次）；custom element 9 / 14；GC 190 / 148；style 3385 / 411；layout 711 / 733；paint 850 / 373；其他 1109 / 443。上一次的 script 9980 / 2524 有一半以上其實是 fragment 的 parse。
- **`ParseHTML`**（同樣被高估）：jQuery 4062 / 411（kun 1800 次，平均 2.26 ms；Chrome 約 1890 次 `HTMLDocumentParser::append`，每次約 0.22 ms）、ES6-Webpack 902 / 136、ES5 548 / 70、Backbone 35 / 13、TipTap 24 / 3。當時以為每次 parse 有和內容無關的固定成本（建新的 document 本身只是建一個節點，不是它）；修掉 log 的紀錄之後是每次 0.59 ms。
- **其他**：`WebFrameWidgetImpl::UpdateLifecycle` 自己 614 ms（Preact 119、Svelte 73、React 69、Angular 63、ES6-Webpack 61、Lit 59），是 paint 之後沒有 span 的步驟（2d-3 補上）；Chrome 的 `LayerTreeHost::DoUpdateLayers` 148 ms，kun 3 ms；`ThreadControllerImpl::RunTask` 自己 378 ms（NewsSite-Next 126、Svelte 61、Nuxt 54）；`WebFrameWidgetImpl::BeginMainFrame` 自己 63 ms；frame 的其他步驟合計不到 20 ms。style 是 Chrome 的 8 倍（NewsSite-Nuxt 425 + 157、Next 359 + 152、TipTap 的 rebuild 352、Preact 162 + 87）；`dom_api::update_form_states` 幾乎是 0。

### 2d-1 的 trace：V8 的 GC（2026-10-16，Fedora 44）

`perf` profile 錄 jQuery、Preact-Complex-DOM、ES6-Webpack-Complex-DOM 各 3 個 iteration（196 秒，含編譯），trace 0.40、0.54、0.64 MB，沒有資料遺失。V8 的事件用 Chrome 的名稱（`MinorGC`、`V8.GC_SCAVENGER_*`、`V8.GC_MARK_COMPACTOR`、`V8.GC_MC_INCREMENTAL`……），在 kun 的 span 裡、helper thread 上也有；compile 事件在 `disabled-by-default-v8.compile`，和 Chrome 那一側一樣沒錄。計時區間裡 jQuery 的 GC 合計約 50 ms，`FunctionCall` 的 self 還有 4005 ms：差距不在 GC；Preact-Complex-DOM 的區間裡沒有 GC。

### RFC 0046、0047 和 RFC 0048 第 1 段之後的比較表和 trace（2026-10-16，Fedora 44）

- **環境**：`main` 的 `4b243972`，`ci.py speedometer`（一輪，20 個 default suite，每個 10 個 iteration，589 秒，含編 `perf` profile），Chrome for Testing 152.0.7977.82，AMD Ryzen AI 9 HX 370（24 核），接電源；`--trace --iterations 3`（105 秒）；`perf record -e cycles:u --call-graph dwarf` 錄 kun 的 jQuery 和 ES6-Webpack-Complex-DOM（V8 的 JIT frame 擋住 dwarf 的展開，只用葉子的符號）。trace 用 Perfetto 的 trace processor（`perfetto` 0.58.2 的 Python 套件）算 renderer main thread 上每個名稱的 self time，只算計時的區間（kun 的 trace 的 renderer main thread 是 tid 0，trace processor 當成 idle thread，先把 tid 加 1）；區間的長度是量到的 1 到 2 倍，只看比例；Perf-Dashboard 的 app 自己用 rAF，不算進合計。
- **比較表**（中位數 ms，kun / Chrome，比值；括號是「RFC 0040 之後」的比值）：jQuery 1057.5 / 109.9，9.62（18.83）；ES6-Webpack-Complex-DOM 355.7 / 37.9，9.39（12.15）；Preact-Complex-DOM 72.9 / 11.9，6.12（4.36）；Svelte-Complex-DOM 98.2 / 16.0，6.12（4.39）；NewsSite-Nuxt 269.3 / 54.1，4.97（3.40）；ES5 170.5 / 36.7，4.64（5.37）；Lit-Complex-DOM 75.1 / 20.2，3.72（第一次能比）；Backbone 89.0 / 26.7，3.33（3.77）；Perf-Dashboard 226.7 / 69.0，3.29（1.36）；Angular-Complex-DOM 151.9 / 46.7，3.25（3.26）；WebComponents 72.6 / 22.9，3.17（第一次能比）；Charts-chartjs 451.8 / 159.1，2.84（2.79）；React-Complex-DOM 156.5 / 56.3，2.78（7.03）；NewsSite-Next 257.1 / 99.6，2.58（2.30）；Vue 81.3 / 33.3，2.44（2.43）；Charts-observable-plot 139.0 / 68.0，2.04（1.96）；React-Stockcharts-SVG 267.7 / 131.1，2.04（1.58）；React-Redux 99.6 / 61.0，1.63（7.48）。無效：Editor-CodeMirror 照舊差 2 個 `div`；Editor-TipTap 是 Chrome 自己第 3 個 iteration 少了 `ProseMirror-focused`。
- **變化**：React-Complex-DOM、React-Redux、jQuery、ES6-Webpack 少很多（10-13 的 collection、`focus()`、表單簿記的修正）。Perf-Dashboard 從 99.7 變成 226.7 ms（Chrome 73.2 → 69.0）：sync 219.5 對 49.1 ms，`SelectingRange` 一個 test 122.9 對 14.6 ms；async 3.5 對 19.9 ms，kun 比 Chrome 少，要另外查。Preact、Svelte 的 kun 從 64.7、78.1 變成 72.9、98.2 ms，只量一輪，不能確定是退步。
- **sync 和 async**（每個 test 的中位數加起來，kun / Chrome，ms）：script 為主的是 jQuery（sync 967.8 / 106.7：`CompletingAllItems` 448.8 / 55.9、`Adding100Items` 271.1 / 25.1、`DeletingAllItems` 247.9 / 25.6）、ES6-Webpack（sync 276.1 / 27.8）、ES5（146.1 / 29.3）、TipTap（229.9 / 55.8）、chartjs（320.6 / 74.0）、Perf-Dashboard；rendering 為主的是 Complex-DOM（async：Svelte 84.9 / 13.0、Preact 63.3 / 9.3、Lit 62.8 / 15.4、Angular 81.4 / 13.3、React 81.6 / 15.7）和 NewsSite（Nuxt 146.0 / 37.3、Next 144.9 / 57.2）。
- **trace 的 self time**（19 個 suite，kun / Chrome，ms）：script（kun 的 `FunctionCall`、`RunMicrotasks`、`EventDispatch` 等，含 JS 呼叫的 DOM API；Chrome 的 `v8.*`、`FunctionCall`、`ParseHTML` 等）9980 / 2524；style 3022 / 324；layout 667 / 510；paint（含 canvas 的 `raster::draw_into`、Chrome 的 pre-paint、compositing）754 / 362；其他 925 / 213。script 的差距一半在 jQuery（4350 / 631），其次 ES6-Webpack（1625 / 252）、ES5（860 / 113）、chartjs（553 / 164）；style 集中在 NewsSite（457 / 74、451 / 33）、TipTap（375 / 4）、Preact（224 / 21）、ES6-Webpack（192 / 20）、Angular（185 / 18）、jQuery（156 / 13）、Svelte（147 / 12）；layout 只有 TipTap 明顯（240 / 98）；paint 主要是 chartjs 的 canvas（301 / 80）。「其他」集中在 Complex-DOM（Preact 136、Angular 102、ES6-Webpack 96、React 95、Svelte 85 ms，Chrome 8 到 14 ms），frame 裡一段和 DOM 大小成正比、沒有 span 的工作。
- **`perf`，jQuery**（renderer main thread 2425 個樣本，含載入）：JIT 的 JS 1.6%、V8 的 C++ 和 builtin 7.4%；malloc / free 18.7%；cppgc 的 handle 14.7%；`dom::scope` 13.1%（`node_by_id` 5.1%）；`StoreId` 和 `NodeId` 的 SipHash（`hash_one::<&StoreId>` 3.6%、`sip::Hasher::write` 1.7%）；fragment 的 parse 約 10%。**ES6-Webpack-Complex-DOM**（3603 個樣本）：`style::cascade` 6.4%、selector 的比對（`matches_complex_selector_internal` 3.7%、`SelectorQuery::matches_in_scope` 2.4%）、`RuleData` 的排序 1.6%、`recalc_style` 2.2%；malloc / free 13.1%、`memmove` 5.6%；cppgc 的 handle 8.4%、`dom::scope` 4.7%。
- **結論**：差距最大的是 JS 呼叫的 DOM API（不是 V8 本身）：查表、cppgc 的 handle、配置、innerHTML 的 parse；其次是 style 的重算和 frame 裡沒有 span 的一段；layout 接近 Chrome。由此排了 2d（第 39 條）。

### 盤點的重跑：修掉四個誤報之後（2026-10-16，Windows 11）

kun `2b5e1eff` 加上誤報的修正，Chrome 5 秒、20 個 suite 都跑完（經過 `--no-sandbox` 的 wrapper；同一天下午沒有 wrapper 的那次等不到報告，600 秒）。值是空字串的宣告沒有了（Complex-DOM 各 77 到 79 條、NewsSite 各 38 條，原本 131 到 133、85）；at-rule 沒有 kun 丟掉的；selector 的元兇只剩 `::placeholder`、`::-webkit-*` 和 `::selection`；SVG 的元素不再出現，元素只剩 HTML 的那 14 個。多出來的一條是真的缺口：Complex-DOM 的 `border-image: none`（kun 沒有 `border-image`）。

### 盤點的重跑：S2 到 S7 做完之後（2026-10-16，Windows 11）

- **環境**：`ci.py speedometer --inventory`（Chrome 重新記錄，Chrome for Testing 152.0.7977.82 `win64`，這台第一次下載），kun `2b5e1eff`（`main`：RFC 0048 做完之後）的 debug build。20 個 default suite 全部跑完、沒有例外。
- **kun 丟掉的宣告**（不算值是空字串的；括號是 2026-10-13 連空字串一起算的）：Complex-DOM 的 6 個各 76 到 78（570 到 577），NewsSite-Next、Nuxt 各 38（251、247），ES5、jQuery、Backbone、Vue、React-Redux、WebComponents 各 28 到 30（48 到 55），Editor-CodeMirror 27（68）、TipTap 19（34），React-Stockcharts-SVG 12（12），Charts 和 Perf-Dashboard 0 到 1。`var()`、`transition-*`、`@keyframes`、`::before`、`::after`、`content`、`float`、`vertical-align`、`calc()` 都不再出現。
- **還丟掉的，會改變畫面的**：`transform: rotate()`（每個 TodoMVC 的 toggle-all、NewsSite 的圖示）、`transform-origin`；NewsSite 的 `display: -webkit-box`、`-webkit-box-orient`、`-webkit-line-clamp`（會改 layout）、`position: sticky`、`object-fit`、`object-position`、`scroll-behavior`；`clip: rect()`、`clip-path: inset(50%)`（visually-hidden：React-Redux、Vue、WebComponents、Complex-DOM、NewsSite）、`clip-path: url()`（Stockcharts）；Complex-DOM（Spectrum）的 flow-relative 圓角、system color（`buttonface`、`buttontext`、`canvastext`、`graytext`、`highlight`、`highlighttext`）、`filter`、`isolation`、`hyphens`、`resize`、`-webkit-text-fill-color`、`forced-color-adjust`；每個 TodoMVC 的 `text-shadow`，大多數 suite 的 `text-decoration-style` / `color` / `thickness`；CodeMirror 的 `position: sticky`、`contain`、`tab-size`，兩個 Editor 的 `caret-color`。不改畫面或 kun 沒有對應的：`appearance`、`cursor`、`user-select`、`-webkit-font-smoothing`、`text-rendering`、`font-variant-*`、`font-feature-settings`、`border-image-*`、`animation-timeline`、`animation-range-*`、`-webkit-tap-highlight-color`。
- **解析不了的 selector**：都是 kun 沒有的 pseudo-element：`::placeholder`（Complex-DOM 的 6 個、WebComponents）、`::-webkit-input-placeholder`、`::-webkit-search-*`、`::-webkit-*-spin-button`（TodoMVC）、`::selection`（兩個 Editor）。2026-10-13 的 `::before`、`::after`、`:checked`、`:hover`、`:lang()`、`:read-only`、`:target` 都通過了。
- **kun 缺的 API**（多半只是 feature detection 或有退路的）：`XMLHttpRequest`、`window.opener`（Backbone、jQuery）、`HTMLLIElement.type`、`HTMLUListElement.type`、`HTMLBodyElement.onfocus`（jQuery、Backbone）、`MouseEvent.fromElement`（React-Redux、Stockcharts）、`document.all`（Vue）、`FileReader`、`navigator.geolocation`、`IntersectionObserver` 的 `delay`、`trackVisibility`（Angular）、`trustedTypes`（Lit）、`PerformanceObserver`、`document.prerendering`、`PerformanceNavigationTiming` 的 `type`、`activationStart`（NewsSite-Next）、`navigator.connection`、`console.createTask`（NewsSite-Nuxt）、`navigator.scheduling`（NewsSite-Next、CodeMirror）；runner 的 `navigator.wakeLock`。`<div>`、`<span>`、`<li>`、`<ul>`、`<p>`、`<h1>` 到 `<h4>`、`<br>`、`<blockquote>`、`<style>`、`<tr>`、`<td>` 在 kun 是 `HTMLElement`。
- **看得出的**：沒有 suite 再因為缺 API 跑不完；畫面和 Chrome 不同的剩 `rotate()`、NewsSite 的 line clamp 和 `sticky`、visually-hidden 的 `clip`，和這份盤點看不到的表單控制項的繪製（kun 的 `<input>`、checkbox、`<button>` 大小是 0；每個 TodoMVC 都有）。這次的報告裡另有四個誤報，同一天修掉（上一則）。

### 表單的簿記：子樹只走一次、只有 form 的 submit button 找 default button（2026-10-13，Fedora 44）

`perf stat -e instructions:u,cycles:u`（整個 kun process，5 個 iteration；`origin/main` 的 `4e98e384` 在 `../kun-main` 的 worktree 編，輪流各量兩次）：jQuery 的指令 70.3、69.3 G → 61.0、61.5 G（−12%），cycles 23.7、23.4 G → 21.3、21.4 G（−9.5%）；ES5 20.5、20.5 G → 19.8、21.0 G（在起伏裡）。時間（`--browser kun --rounds 1`，輪流各兩次）：ES5 214.9、214.4 → 193.2、195.4 ms（約 −9%）；jQuery 858.2、852.5 → 903.8、762.8 ms（起伏太大）。ES5 的 `perf`：`update_elements` 23.0% → 17.5%，`form_states` 10.9% → 7.1%。

### `MouseEvent` 的 `pageX`、`pageY` 不更新 layout（2026-10-13，Fedora 44）

`perf stat`（同上的做法，`main` 的 `9e9e6913`）：React-Redux 的指令 24.6、24.2 G → 13.6、13.5 G（−44%），cycles 8.2、7.8 G → 4.3、4.4 G（−46%）；React-Complex-DOM 58.8、59.5 G → 50.7、49.8 G（−15%，含每個 iteration 重新載入的大頁面），cycles 19.6、19.6 G → 16.6、16.4 G。時間：React-Redux 392.5 → 75.2 ms（用「RFC 0040 之後」的 Chrome 52.5 ms 算，比值 7.48 → 約 1.43），React-Complex-DOM 416.6 → 144.3 ms（Chrome 59.3 ms，7.03 → 約 2.43）。

### `focus()` 已經有 focus 的元素時不做事（2026-10-13，Fedora 44）

`perf stat`（同上的做法，`main` 的 `297bda44`）：jQuery 的指令 105.2、104.9 G → 68.6、67.8 G（−35%），cycles 38.9、38.7 G → 23.2、23.1 G（−40%）；ES5 20.6、21.7 G → 22.3、21.6 G（不重複 `focus()`，在起伏裡）。時間：jQuery 1728.7 → 849.0 ms（用「RFC 0040 之後」的 Chrome 91.8 ms 算，比值 18.8 → 約 9.2；sync：Adding100Items 209 ms、CompletingAllItems 423 ms、DeletingAllItems 206 ms），ES5 214.9 ms 不變。修之前 frame pointer 的 `perf`（jQuery，5 個 iteration）：renderer main thread 的 38.5% 是 style 和 layout，大部分在 `is_focusable_area`。之後剩下的：`innerHTML` 的解析 23.2%（Chrome 的 `ParseHTML` 也是它最大的一塊，293 ms）、style 和 layout 的其他部分、`update_elements` 17.8%、GC 7.8%。

### 盤點：kun 丟掉的 CSS（2026-10-13，Fedora 44）

- `main` 的 `297bda44`，`ci.py speedometer --inventory`（106 秒）；20 個 default suite 在 kun 上都跑完。
- **kun 丟掉的宣告**（括號是值用 `var()` 的；這次連值是空字串的誤報一起算）：Complex-DOM 的 6 個各 570 到 577（382 到 390）；NewsSite-Next、Nuxt 251、247（各 128）；ES5、jQuery、Backbone、Vue、React-Redux、WebComponents 各 48 到 55（0）；Editor-CodeMirror 68、TipTap 34、React-Stockcharts-SVG 12（`cursor`、`clip-path`），Charts 和 Perf-Dashboard 0 到 2。Complex-DOM 用 `var()` 的：`color` 72、`background-color` 55、`width` 19、`height` 17、`font-size` 17、`line-height` 16、邏輯屬性；不用 `var()` 的多是 shorthand 展開的初始值，另有 `transition-*` 3 到 6 條、`vertical-align`、`content`、`float`、`calc()` 各幾條。
- **解析不了的 selector**：Complex-DOM 各 69 到 71 個（`::before` 38、`:checked` 15、`:hover` 13、`::after` 10、`::placeholder` 10，`:lang()`、`:read-only`）；NewsSite 17 個（`::before`、`::after`、`:target`）；CodeMirror 22 個（`::after` 16）；其他 TodoMVC 5 到 9 個。at-rule：`@media` 在大多數 TodoMVC 和 NewsSite，`@keyframes` 在 Complex-DOM 和兩個 Editor。
- **看得出的**：Complex-DOM 和 NewsSite 在 kun 上少算了大量用 `var()` 的 style、`::before`、`::after` 的 box 也沒建，比值偏低，要 `var()` 和 pseudo-element 之後才量得對；jQuery、ES5 只丟了少數宣告，比值大致可信，先優化它們（2026-10-13 決定）。

### RFC 0040 之後的比較表（2026-10-13，Fedora 44）

`main` 的 `297bda44`，`ci.py speedometer --rounds 1`，只跑一輪，Chrome 的區間寬。中位數 ms，kun / Chrome，比值（括號是「RFC 0039 和 RFC 0016 之後」的）：React-Complex-DOM 416.6 / 59.3，7.03（11.38）；ES6-Webpack-Complex-DOM 430.8 / 35.5，12.15（14.54）；Svelte-Complex-DOM 78.1 / 17.8，4.39（6.50）；Preact-Complex-DOM 64.7 / 14.8，4.36（4.29）；Angular-Complex-DOM 174.0 / 53.3，3.26（3.06）；React-Redux 392.5 / 52.5，7.48（6.50）；NewsSite-Nuxt 328.8 / 96.7，3.40（3.30）；NewsSite-Next 294.2 / 128.1，2.30（4.07）；Backbone 87.0 / 23.0，3.77（3.82）；Vue 60.5 / 24.9，2.43（2.93）；jQuery 1728.7 / 91.8，18.83（34.94）；ES5 214.4 / 39.9，5.37（10.29）；Charts-observable-plot 139.8 / 71.4，1.96；Charts-chartjs 422.4 / 151.5，2.79；React-Stockcharts-SVG 221.2 / 140.2，1.58；Perf-Dashboard 99.7 / 73.2，1.36。WebComponents 1.67、Lit-Complex-DOM 2.17 照舊不能比（shadow tree 的 style）。無效：Editor-CodeMirror 差 2 個 `div`；Editor-TipTap 是 Chrome 自己第 8 個 iteration 少了 `ProseMirror-focused`。比值最大的還是 jQuery（sync 的三個 test 365 到 764 ms）、ES6-Webpack-Complex-DOM、React-Redux、React-Complex-DOM。

### RFC 0040 留下的：收 `&str` 名稱的 helper 和 `match` 名稱改成 atom（2026-10-13，Fedora 44）

`perf stat`（同上的做法，`origin/main` 的 `2f8f532b`）：ES5 的指令 21.9、22.2 G → 20.4、19.4 G（−7% 到 −12%），cycles −6% 到 −12%；jQuery 105.8、105.8 G → 104.9、105.8 G（0 到 −0.9%）；ES6-Webpack-Complex-DOM 65.8、66.0 G → 64.9、65.4 G（−0.9% 到 −1.4%）；React-Complex-DOM 60.5、60.1 G → 60.1、60.1 G（0 到 −0.7%），cycles 在起伏裡。

### 表單的簿記讀屬性時不配置、名稱先比 local name（2026-10-13，Fedora 44）

`ci.py speedometer --browser kun --rounds 1 --suites …`（jQuery 的也含 collection 的修正；和「RFC 0039 和 RFC 0016 之後」比）：ES5 409.5 → 182.4 ms（用那次的 Chrome 39.8 ms 算，比值 10.3 → 約 4.6）、jQuery 3520.5 → 1679.8 ms（34.9 → 約 16.7）、ES6-Webpack-Complex-DOM 604.0 → 461.9 ms（14.5 → 約 11.1）、React-Complex-DOM 498.5 → 457.5 ms（11.4 → 約 10.4）。修之前 `input_type` 一個就佔 jQuery 的 15.2%、ES5 的 21.8%，`memcmp` 在四個 suite 佔 6% 到 18%。

### jQuery 的 live collection：用 index 走過一遍是平方（2026-10-13，Fedora 44）

`ci.py speedometer --trace --browser kun --iterations 3 --suites TodoMVC-jQuery`：`FunctionCall` 的 self（JS 加上沒有 span 的 DOM API）從 10.0 到 12.2 秒降到 7.0 秒，`collection` 不再出現在 `perf` 的前面。剩下的：`memcmp` 16%、配置和釋放約 18%（每次 `innerHTML` 重建清單時建立元素和屬性）。

### RFC 0039 和 RFC 0016 之後的比較表（2026-10-13，Fedora 44）

- `main` 的 `dbfbaa9f`，`ci.py speedometer --rounds 1`（621 秒，含 `cargo clean` 之後重編），其他和「優化前的重新量測」一樣，只跑一輪。
- **比較表**（中位數 ms，kun / Chrome，比值；括號是優化前的 kun 和比值）：React-Complex-DOM 498.5 / 43.8，11.38（4114.4，110.60）；ES6-Webpack-Complex-DOM 604.0 / 41.6，14.54（759.3，16.42）；Svelte-Complex-DOM 117.9 / 18.1，6.50（301.5，18.05）；Preact-Complex-DOM 78.9 / 18.4，4.29（204.8，14.90）；Angular-Complex-DOM 149.4 / 48.8，3.06（306.0，8.52）；React-Redux 354.3 / 54.5，6.50（768.2，12.13）；NewsSite-Nuxt 333.6 / 101.0，3.30（584.6，9.51）；NewsSite-Next 319.1 / 78.4，4.07（512.0，5.83）；Editor-TipTap 236.5 / 95.1，2.49（335.6，3.91）；Backbone 123.3 / 32.3，3.82（165.3，5.86）；Vue 96.2 / 32.9，2.93（102.2，3.83）；jQuery 3520.5 / 100.7，34.94（4161.5，36.57）；ES5 409.5 / 39.8，10.29（393.9，7.57）；Charts-observable-plot 170.0 / 83.9，2.03；Charts-chartjs 435.0 / 143.9，3.02；React-Stockcharts-SVG 340.2 / 144.3，2.36；Perf-Dashboard 93.4 / 58.4，1.60。
- WebComponents（43.3 / 20.6，2.10）和 Lit-Complex-DOM（60.3 / 19.5，3.09）的 DOM 摘要和 Chrome 一樣、工具算成有效，但 shadow root 的 adopted stylesheet 要等 style 進 shadow tree 才生效，還不能比。Editor-CodeMirror 照舊差 2 個 `div`。
- Complex-DOM 的不計時的載入一個 8 到 13 秒（之前 49 到 83 秒）。比值最大的是 jQuery（每個 test 的 sync 820 到 1804 ms，Chrome 22 到 44 ms，live collection 的平方成本）、ES6-Webpack-Complex-DOM（Adding100Items 的 sync 368 對 16 ms）、React-Complex-DOM（DeletingAllItems 的 sync 313 ms）、ES5（Adding100Items 的 sync 335 對 21 ms）。Complex-DOM 的 async 10 到 67 ms（之前 88 到 142 ms），Chrome 1 到 7 ms。

### kun 這一側的重跑：RFC 0016 的第 3 段之後（2026-10-12，Windows 11）

kun 的 `0016` branch（constructable stylesheet、`adoptedStyleSheets`）的 debug build。`--inventory` 的 Chrome 記錄兩次都是 `no report in 600 s`，沒有盤點的表。`ci.py speedometer --browser kun --suites TodoMVC-WebComponents,TodoMVC-Lit-Complex-DOM --iterations 1 --rounds 1`：兩個 suite 都跑完（各 1 秒），不再卡在 `CSSStyleSheet`（路線圖的 M3 的這兩個 suite）；畫面還不完整，數字不採用。比較的那一步丟 `KeyError`（「還沒解決的」）。

### 優化前的重新量測：比較表、trace 和 `perf`（2026-10-12，Fedora 44）

- **環境**：`main` 的 `b58e7cb6`（RFC 0038 做完、TipTap 的修正之後），Chrome for Testing 152.0.7977.82，AMD Ryzen AI 9 HX 370（24 核），接電源。`ci.py speedometer --rounds 1`（518 秒）；`--trace --iterations 3` 錄 React-Complex-DOM、jQuery、Svelte-Complex-DOM、React-Redux（91 秒）；`perf record -p <kun>` 錄 Svelte-Complex-DOM（`--call-graph dwarf`）和 jQuery。
- **比較表**（中位數 ms，kun / Chrome，比值；只跑一輪）：React-Complex-DOM 4114.4 / 37.2（110.6）、jQuery 4161.5 / 113.8（36.6）、Svelte-Complex-DOM 301.5 / 16.7（18.1）、ES6-Webpack-Complex-DOM 759.3 / 46.2（16.4）、Preact-Complex-DOM 204.8 / 13.8（14.9）、React-Redux 768.2 / 63.3（12.1）、NewsSite-Nuxt 584.6 / 61.5（9.5）、Angular-Complex-DOM 306.0 / 35.9（8.5）、ES5 393.9 / 52.0（7.6）、Backbone 165.3 / 28.2（5.9）、NewsSite-Next 512.0 / 87.7（5.8）、Editor-TipTap 335.6 / 85.9（3.9）、Vue 102.2 / 26.6（3.8）、Charts-chartjs 447.9 / 163.6（2.7）、Charts-observable-plot 172.5 / 64.0（2.7）、React-Stockcharts-SVG 303.8 / 143.0（2.1）、Perf-Dashboard 105.0 / 57.2（1.8）。無效：WebComponents、Lit-Complex-DOM（停在 Adding100Items）、Editor-CodeMirror（139 對 141 個 `div`）。這台的 Chrome 比 Apple M4 慢 2 到 3 倍，比值和 10-11 在 macOS 上的接近。
- **不計時的載入**：Complex-DOM 的 suite 在 kun 上一個 49 到 83 秒（Chrome 2 秒）：每個 iteration 重新載入大頁面（約 6600 個元素、300 KB 的 CSS）。
- **trace**（3 個 iteration，renderer main thread 的 self time；Chrome 的用一個解 protobuf 的小腳本算）：React-Complex-DOM 33.1 秒裡 `Document::UpdateStyle` 31.0 秒（362 次），Chrome 的 `Document::recalcStyle` 59 ms（48 次）；Svelte-Complex-DOM 26.8 秒裡 25.1 秒（65 次，平均 387 ms），Chrome 62 ms；React-Redux 5.2 秒裡 4.5 秒；jQuery 20.1 秒裡 `FunctionCall` 的 self 11.6 秒、`UpdateStyle` 6.2 秒（994 次）。
- **`perf`**：Svelte-Complex-DOM 61.6% 在 `memcmp`（`NodeRef::attribute` 比對 `QualName` 的字串）、11.5% 在 `malloc`（`has_class` 每次建 `QualName::attr("class")`、複製值）、`has_class` 和 `matches_complex_selector_internal` 自己 9%：每個元素對每條規則逐一比對（657 條規則、903 個 selector），沒有照最右邊的 compound 分桶（Blink 的 `RuleSet`）、沒有祖先的 bloom filter（`SelectorFilter`），屬性名稱是字串、`class` 每次重新切；大頁面的 CSS 有 40 個 `:first-child` 這一類、97 個兄弟 combinator，重算範圍也大。jQuery：`memcmp` 17.6%、`dom_api::collection::items` 9.4%、`node_by_id` 6.3%、`StoreId` 的 hash 5.6%（live collection 的平方成本）。
- **結論**：差距最大的五個 suite 主要是 style 的比對本身慢，其次是重算的範圍；jQuery 另外有 live collection 的平方成本。

### TipTap 的效能：空白摺疊的平方成本（2026-10-12，Fedora 44）

`main` 的 `8fa406b2`（RFC 0038 做完），`--trace --suites Editor-TipTap`、`--browser kun --suites Editor-TipTap --rounds 1 --iterations 5`，`perf record -F 1999 -p <kun>`。kun 的 trace：`StyleEngine::RebuildLayoutTree` 1737 ms（33 次），`FunctionCall` 的 self 只有 180 ms；`perf` 40% 在 `InlineContent::new` 自己（`EncodeUtf16`）。修之後中位數 743.9 → 264.1 ms（Chrome 86.4 ms：8.61 倍 → 3.06 倍）；Long 的 sync 354 → 126 ms，Highlight 353 → 124 ms。剩下的：每個測試一次整份的重建（6 次 40 到 67 ms）加上 27 到 44 ms 的 `Layout`，sample 大多是 parley、harfrust、icu 的 shaping 和斷行，JIT 只有 0.55%：kun 沒有 shaping 的快取（Chrome 的 LayoutNG 沿用 inline item 的 `ShapeResult`、有 shape cache）。改成 SIGINT 結束 Chrome 之後 `--trace --suites Editor-TipTap --iterations 3` 寫出 6.7 MB 的 `.pftrace`（9 秒）。

### 盤點的重跑和兩個 Editor 的量測：RFC 0038 之後，M6 達成（2026-10-12，Fedora 44）

`--inventory --suites Editor-CodeMirror,Editor-TipTap`（12 秒）和 `--suites Editor-CodeMirror,Editor-TipTap --rounds 1 --iterations 3`；kun 的 `0038` branch。兩個 Editor 都跑完、沒有例外（上次停在 `getSelection`），M6 達成；kun 缺的只剩 CodeMirror 的 `Navigator.scheduling`。第一次兩個都無效：CodeMirror 的 Long 之後 kun 139 個 `div`、Chrome 141 個（CodeMirror 照量到的行高和字寬決定畫幾行，kun 的排版和 Chrome 不同）；TipTap 沒有 `ProseMirror-focused`：kun 在 frame 開始時就拿走每份 document 的 animation frame callback，TipTap 的 `focus()` 排在 iframe 的 rAF，Chrome 在同一個 frame（HTML 照 tree order 輪到 iframe 時才拿它的清單）、在 runner 的 `setTimeout` 之前跑，kun 晚一個 frame。RFC 0038 的 2c 修正之後 TipTap 有效：kun 743.9 ms、Chrome 86.4 ms（8.61 倍；Long 的 sync 363.8 對 48.9 ms，Highlight 360.6 對 34.5 ms）。

### 盤點的重跑：RFC 0036 的第 2 段（`IntersectionObserver`）之後（2026-10-12，Windows 11）

`--inventory --reuse-chrome`，kun 的 `0036` branch。18 個跑完、其中 2 個有例外（兩個 Editor 的 `getSelection`）、2 個卡住（WebComponents、Lit-Complex-DOM 的 `CSSStyleSheet`），NewsSite-Nuxt 也跑完。`IntersectionObserver` 不再出現在 NewsSite-Nuxt、CodeMirror，只剩 v2 的 `delay`、`trackVisibility`（Angular 的 zone.js 列舉屬性時讀的）；CodeMirror 剩 `Range` 和 `getSelection`。

### 盤點的重跑：RFC 0036 的第 1 段（`ResizeObserver`）之後，M4 達成（2026-10-12，Windows 11）

`--inventory`，Chrome for Testing 152.0.7977.82（`win64`，這台第一次下載），kun 的 `0036` branch；工具不用改就在 Windows 上跑完。16 個跑完、2 個有例外、2 個卡住（上次 15、3、2）：Charts-chartjs 跑完、沒有例外，M4 的四個 suite 都跑完；Perf-Dashboard 跑完、沒有例外（custom element 之後）。`ResizeObserver` 不再出現，`IntersectionObserver` 還在（NewsSite-Nuxt、CodeMirror、Angular 的 feature detection）。

### 盤點的重跑：RFC 0035 的第 1 段之後（2026-10-12，Fedora 44）

`--inventory`，1 分 33 秒，kun 的 `0035` branch（shadow tree、事件的 retarget、`activeElement`，還沒有 custom element）。15 個跑完、3 個有例外、2 個卡住（上次 14、3、3）。Perf-Dashboard 跑完、3 個 test、沒有例外，但它沒有 `customElements` 時改聽 kun 不發的 `DOMNodeInsertedIntoDocument`，元件可能沒有 render。kun 缺的最多：`window.customElements`（4）、`CustomElementRegistry.define`（3）；`attachShadow`、`shadowRoot` 不再出現。

### 盤點的歸屬修正之後的重跑（2026-10-12，Fedora 44）

`--inventory`（Chrome 重新記錄，`linux64`，這台第一次下載），2 分 55 秒含編 kun；kun `63b6ed1e`（`main`：RFC 0034 做完之後）。14 個跑完；有例外的 3 個：兩個 Editor 的 `getSelection`、Charts-chartjs 的 `ResizeObserver`；卡住的 3 個：WebComponents、Lit-Complex-DOM 的 `CSSStyleSheet`，Perf-Dashboard 的 `alert`（其實是缺 shadow DOM：`main.js` 先檢查 `attachShadow`，沒有就 `alert`；路線圖的 M4 加上 K-1）；和 2026-10-11 相同。kun 缺的（修正之後的 suite 數）：`window.customElements` 4；`CustomElementRegistry.define`、`attachShadow`、`shadowRoot`、`new IntersectionObserver` 各 3；`new CSSStyleSheet`、`replaceSync`、`adoptedStyleSheets=`、`assignedSlot`、`Range` 和 `getSelection`、`ResizeObserver`、`XMLHttpRequest`、`window.opener`、`MouseEvent.fromElement`、`navigator.scheduling` 各 2。`MessageChannel` 不再出現；Perf-Dashboard 另外用 kun 沒有的 `DocumentFragment.getElementById`。

### `MessageChannel` 之後的量測（2026-10-11，macOS Apple Silicon，接電源）

RFC 0034 的第 1 段之後，`ci.py speedometer`（3 輪），10 分 43 秒。NewsSite-Next：kun 207.4 ms（206.7–208.4），Chrome 34.8 ms，比值 5.96（上次 185.2 ms、5.34）：React 的 scheduler 改用 `MessageChannel`，工作算進 async，和 Chrome 一樣；sync 沒變。NewsSite-Nuxt 三輪都有效：kun 175.1 ms（174.6–175.3），Chrome 29.1 ms，6.02（有時用 `IntersectionObserver`，還會不穩定）。React-Complex-DOM（80.24）、React-Redux（20.76）不變（discrete 事件在 React 裡同步 render）；其他 suite 的變化都在 2% 以內。

### 第一次正式的量測（2026-10-11，macOS Apple Silicon，接電源）

`ci.py speedometer`（20 個 default suite，3 輪 × 10 個 iteration，以 suite 為單位輪流），10 分 42 秒；kun `53116b63`（`perf` profile：ThinLTO，沒有 PGO），Chrome for Testing 152.0.7977.82（ThinLTO 加 PGO）；macOS 26.6.2，Apple M4，10 核。有效的 13 個 suite（每個 iteration 的 suite 總和的中位數，ms，括號是 95% 區間）：

| Suite | kun | Chrome | kun / Chrome |
| --- | --- | --- | --- |
| TodoMVC-JavaScript-ES5 | 223.2（222.6–224.3） | 19.8（19.5–20.4） | 11.24 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 485.0（483.8–487.0） | 19.3（19.0–19.5） | 25.13 |
| TodoMVC-React-Complex-DOM | 1355.3（1349.7–1363.5） | 16.6（16.3–16.9） | 81.64 |
| TodoMVC-React-Redux | 383.0（380.6–384.6） | 18.4（18.2–18.6） | 20.81 |
| TodoMVC-Backbone | 54.6（54.3–55.0） | 14.0（13.7–14.2） | 3.91 |
| TodoMVC-Angular-Complex-DOM | 107.9（107.5–108.3） | 15.0（14.6–15.5） | 7.19 |
| TodoMVC-Vue | 37.3（37.1–37.5） | 11.8（11.6–12.0） | 3.17 |
| TodoMVC-jQuery | 2664.5（2654.8–2673.1） | 55.8（55.0–56.8） | 47.75 |
| TodoMVC-Preact-Complex-DOM | 77.7（77.4–78.3） | 6.4（6.2–6.4） | 12.23 |
| TodoMVC-Svelte-Complex-DOM | 82.4（82.2–82.8） | 5.6（5.5–5.8） | 14.71 |
| NewsSite-Next | 185.2（184.7–186.1） | 34.7（34.5–35.1） | 5.34 |
| Charts-observable-plot | 71.3（71.0–71.9） | 23.3（22.9–24.7） | 3.07 |
| React-Stockcharts-SVG | 120.1（119.7–120.9） | 38.2（37.0–39.2） | 3.14 |

無效的 7 個：WebComponents、Lit-Complex-DOM（`CSSStyleSheet`）、Perf-Dashboard（`alert`）、Charts-chartjs（`ResizeObserver`）、兩個 Editor（`getSelection`），和 NewsSite-Nuxt：第 3 輪丟出 `ReferenceError: IntersectionObserver is not defined`（Nuxt 的 hydration 看時間，有時才用它；第 29 條，任何一輪無效就不給數字）。和沒接電源的 2b 那次比，每個 suite 兩邊的中位數都差 2% 以內（React-Complex-DOM 81.53 和 81.64），Apple M4 用電池時沒有明顯降速。差距大多在 sync（script 和 DOM）；Preact、Svelte、Angular、Vue 的 async 比 sync 慢得多；kun 的第一個 iteration 和之後的差不多，Chrome 的第一個明顯比較慢（暖機）。

### 2c 的第一份真的 trace（2026-10-11，macOS Apple Silicon）

`--trace --suites TodoMVC-React-Complex-DOM,TodoMVC-Vue --iterations 3`（含重編 `perf` 1 分 48 秒；kun 的 2 到 4 MB、Chrome 的約 7 MB）：React-Complex-DOM 在 kun 的 renderer main thread 上 `Document::UpdateStyle` 361 次、合計 9.0 秒，`Layout` 只有 63 ms：script 一再觸發 style 的重算，是它慢的主要原因（`DeletingAllItems` 的 sync 最慢）。

### 2b 的第一次量測：沒接電源（2026-10-11，macOS Apple Silicon）

`ci.py speedometer`（3 輪 × 10 個 iteration，9 分 30 秒；kun `7c4d4023`，Apple M4，macOS 26.6.2），沒接電源（工具照第 30 條警告），只當參考。有效的 14 個 suite，kun 是 Chrome 的 3.0 到 81.5 倍：Charts-observable-plot 3.00、Vue 3.17、React-Stockcharts-SVG 3.18、Backbone 3.80；React-Complex-DOM 81.53（1349.3 對 16.6 ms，`DeletingAllItems` 的 sync 1252.6 ms）、jQuery 47.58（`CompletingAllItems` 的 sync 1412.4 ms）、ES6-Webpack-Complex-DOM 25.04。Preact 的 `Adding100Items` async 35.0 對 2.5 ms。無效的 6 個和 2a 相同。區間都很窄（例如 221.0 到 222.2 ms），3 輪 × 10 個 iteration 夠用。

### 2a 的有效性（2026-10-11，macOS Apple Silicon）

Chrome 跑 20 個 default suite（10 個 iteration）：第一次只有 Charts-observable-plot 無效（亂數的 class 名稱），定了容許的差異之後再跑兩次，20 個都有效、兩次的摘要相同，每次約 26 秒；每個 suite 的第一個 iteration 明顯比較慢（Vue 48.8 ms，之後約 11.6 ms）。kun（debug build，3 個 iteration，1 分 20 秒）：盤點時乾淨跑完的 14 個 suite 都有效；無效的 6 個和盤點的原因相同（WebComponents、Lit-Complex-DOM 卡在 `CSSStyleSheet`，Perf-Dashboard 卡在 `alert`，Charts-chartjs 的 `ResizeObserver`，兩個 Editor 的 `getSelection`，DOM 也不同：CodeMirror 少 7 個 `br`、TipTap 少 1 個 `blockquote`）。

### 盤點的重跑：RFC 0031 做完、RFC 0033 的第 1、2 段之後（2026-10-11，macOS Apple Silicon）

`--reuse-chrome`；kun `089cc089`（SVG、Canvas 2D 之後）；3 分 51 秒含 `cargo clean` 之後重編。14 個跑完、3 個有例外、3 個卡住（上次 9、3、5，另有 2 個逾時）：Vue、NewsSite-Next、NewsSite-Nuxt、React-Stockcharts-SVG 變成跑完；Charts-chartjs 改成 `ResizeObserver is not defined`；兩個 Editor 照舊 `getSelection`；WebComponents、Lit-Complex-DOM 照舊 `CSSStyleSheet`，Perf-Dashboard 照舊 `alert`。`(in promise) ReferenceError: Blob is not defined` 不見了。kun 缺的最多（這次和之前兩次的數字受歸屬錯誤影響、偏多）：`attachShadow`（6）；custom element、`ShadowRoot`、constructable stylesheet、`MessageChannel` / `MessagePort`（各 4）；`IntersectionObserver`、`ResizeObserver`、`Range` 和 `getSelection`、`XMLHttpRequest`、`document.all`、`window.opener`、Trusted Types、`scheduler`、`console.createTask`（各 2）。Canvas 2D、`crypto`、`requestIdleCallback`、`Blob`、`SVGElement` 不再出現。

### 盤點的重跑：RFC 0027、0028 的第 1 到 4 段之後（2026-10-10，macOS Apple Silicon）

Chrome for Testing 152.0.7977.82（`mac-arm64`，這台第一次下載），kun `2ca5ac82`（`fetch`、`console`、global attribute、`<a>`、`<img>` 之後）；工具不用改就在 macOS 上跑完。跑完的 10 個（ES5、ES6-Webpack、React-Complex-DOM、React-Redux、Backbone、Angular、jQuery、Preact、Svelte、Charts-observable-plot）；有例外的 3 個：兩個 Editor 的 `getSelection`，Charts-chartjs 的 `can't acquire context`（Canvas）；卡住的 5 個：WebComponents、Lit-Complex-DOM 的 `CSSStyleSheet`，Vue 的 `SVGElement`（`app.mount` 的 `container instanceof SVGElement`），React-Stockcharts-SVG 的 `getContext`，Perf-Dashboard 的 `alert`；逾時的 2 個：NewsSite-Next（`crypto.getRandomValues() not supported`）、NewsSite-Nuxt（`SVGElement`）。好幾個 suite 最後有 `Blob is not defined`。kun 缺的最多：`attachShadow`（6）、`MessageChannel`（4 到 5）、custom element、`ShadowRoot`、constructable stylesheet（各 4）、Canvas 2D（3）、`crypto`、`requestIdleCallback`、`IntersectionObserver`、`PerformanceObserver`、`Range` 和 `getSelection`（各 2）；`contentEditable`、`isConnected`、`<a>` 的 `href`、`<template>` 的 `content` 不見了。CSS 最多 suite 丟掉的是 `pointer-events`、`white-space` 的 longhand、`content`、`cursor`（16 到 17 個），選擇器 `::before` / `::after`（15、16）、`:hover`、`:checked`（14）、表單的 pseudo-class（12），at-rule `@media`、`@keyframes`（14）。

### 1b：第一次在 kun 上跑（2026-10-10，Fedora 44）

kun `90179773faa4`（RFC 0026 第 1 段之後的 `main`）的 debug build，Speedometer `release/3.1` 的 `1386415`。跑完的 10 個：ES5、ES6-Webpack-Complex-DOM、React-Complex-DOM、React-Redux、Backbone、Angular-Complex-DOM、jQuery、Preact-Complex-DOM、Svelte-Complex-DOM、Charts-observable-plot。有例外的 3 個：Editor-CodeMirror（`target.getSelection is not a function`，2 個 test）、Editor-TipTap（`reading 'getSelection'`，2 個 test）、Charts-chartjs（`console.error is not a function`）。卡住的 7 個：WebComponents、Lit-Complex-DOM（`CSSStyleSheet is not defined`）、Vue（`SVGElement is not defined`）、NewsSite-Next、NewsSite-Nuxt（`console.error is not a function`）、React-Stockcharts-SVG（`this.testCanvas.getContext is not a function`，1 個 test）、Perf-Dashboard（`alert is not defined`）。M1 達成：runner 和 ES5、Preact-Complex-DOM、Svelte-Complex-DOM 都跑完、不丟例外（runner 最後一步照第 9 條不算：它在 `didFinishLastIteration` 呼叫 `console.assert`，kun 的 `console` 只有 `log`）。kun 缺的最多：`contentEditable`（7）、`isConnected`（6）、`HTMLAnchorElement.href=`（6）、`attachShadow`（6）、`HTMLTemplateElement.content`（5）、`MessageChannel`（4）、custom element（4）、constructable stylesheet（4）、Canvas 2D（3）、`console` 的 `error`、`warn`、`assert`；CSS 丟掉最多的是 `white-space-collapse` / `text-wrap-mode`（17）、`pointer-events`（17）、`content`、`cursor`、`border-image-*`、`text-decoration-*`（16）、`border-*-radius`、`list-style-type`、`vertical-align`（15）；元素 interface 缺 30 個（`HTMLDivElement`、`HTMLSpanElement`、`HTMLAnchorElement`、`HTMLTemplateElement`、`HTMLCanvasElement` 等，和全部 SVG 的）。
