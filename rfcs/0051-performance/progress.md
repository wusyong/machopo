# RFC 0051 的進度：效能的優化

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-16（草稿和第 1、2 條），同一天排程。完成：2026-10-18，Fedora 44（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0051` 上做，每段做完、經過審核之後 merge 回 `main`（fast-forward）；第 2 段的 2c rebase 到 RFC 0052 之後的 `main`（`invalidation.rs` 兩邊都在檔尾加測試，留兩邊）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 全樹走訪的跳過和 hash：event loop 頂端的 span、A1、A2、A5、B1、B2（第 2 條） | 完成 | 2026-10-16 |
| 2 style 的範圍：基準（第 11 條）；2a feature 收集、改了什麼和 D1，2b base URL（第 13 條），2c structural 的插入和移除（第 14 條），2d 兄弟位置的 invalidation set（第 15 條）；D5 和後代的 invalidation set 延後 | 完成 | 2026-10-17 |
| 3 handle 的成本：B5 cppgc 的讀取在 Rust 裡 inline（第 5 條）；`insert_before` 不為比較建 root、`Persistent` 重複使用（第 6 條） | 完成 | 2026-10-16 |
| 4 fragment 的 parse：C2、C1（第 7 條） | 完成 | 2026-10-16 |
| 3（續）root 的計數、錨點 pool、不查 id 表建 root（第 8 條） | 完成 | 2026-10-17 |
| 量測的方法：cycle 和指令數、`--against` 的交替比較（第 9 條） | 完成 | 2026-10-17 |
| 3（續）clone 只加 count：pool 化的 `Rc` root cell（第 10 條） | 完成 | 2026-10-17 |
| `SoftwareRenderer` 的 1:1 tile 和純色走捷徑（第 16 條） | 完成 | 2026-10-17 |
| 5 每個 task 的收尾和 style 的快取（第 17 條）：5a 每次 script 之後的走訪和 UA shadow tree 的 style，5b B3 和 UA shadow tree 的時機（第 18、19 條），5c D4（第 20 條），5d D7（第 21 條）；A3、D6 延後 | 完成 | 2026-10-18 |
| 6 增量的 paint（第 22 條）：E2（6a1、6a2），E3（第 23 條；6b1、6b2），E1（第 24 條；E1a、E1b），E4（第 25 條） | 完成 | 2026-10-18 |
| 7 便宜的兩項（第 26 條）：7a bindings 取 `this` 的 wrapper，7b `querySelector` | 完成 | 2026-10-18 |
| 8 fragment 的 parse（第 26、27 條）：C3；文字放進之後的可能 | 完成 | 2026-10-18 |

## 驗收

### 全樹走訪和 event loop

| 項目 | 怎麼驗證 |
| --- | --- |
| event loop 在 task 之間的工作是 `RenderThread::between_tasks`（最外層），platform task 是它底下的 `RenderThread::run_platform_tasks` | `python3 scripts/speedometer_test.py MeasureTests.test_traces_of_both` |
| 節點 id 表長到上次 prune 留下的兩倍（至少 1024 項）才 prune，prune 時清掉死掉的節點 | `cargo test -p dom --test it tree::the_id_table_is_pruned_as_it_doubles` |
| document 的 base URL 第一次被問到時找、存著，`<base>` 插入、移除或 `href` 改了才重找 | `cargo test -p dom_api --test it api::base_url_follows_its_bases` |
| 插入或改了屬性的 `<link>` 由 embedder 取走、各載入一次（shadow tree 裡的不載入）；背景圖片的網址在算 style 時記下，不走整份文件（`display: none` 裡的不載入） | `cargo test -p shell --test it modules::a_link_loads_as_it_is_inserted_or_changed network::background_images_script_shows_load` |
| 控制項的 UA shadow tree 延後到 style 更新前才建或同步（之前被移除的不建）；script 馬上讀 style、只畫 frame 時照樣有 | `cargo test -p dom_api --test it forms::a_controls_ua_shadow_tree_waits_for_style`、`cargo test -p shell --test it forms::a_controls_style_from_script_takes_its_ua_shadow_tree forms::a_frame_shows_a_controls_value` |

### DOM 的 handle 和 bindings

| 項目 | 怎麼驗證 |
| --- | --- |
| 回傳已經有 wrapper 的節點時不查 interface 和 realm：比回傳一個數字只多那個節點的 root | `cargo test -p dom_js --test it bindings::an_existing_wrapper_is_returned_without_its_realm` |
| cppgc 的 `Member`、`Persistent` 在 Rust 裡讀；從 `&T` 建 root 不查 id 表；debug build 每次和 C++ 的 `Get`、id 表的那一項比對 | `cargo test -p dom`（每次讀取都比對） |
| 同一份 document 裡插入、移動不建 root；丟掉的 `Persistent` 指回錨點、留著重用，GC 之後不撐住原本的節點；pool 空了照樣有錨點；被回收的節點建不出 root | `cargo test -p dom --test it tree::inserting_within_a_document_roots_nothing tree::dropped_roots_are_reused_and_root_nothing tree::the_spare_roots_anchor_outlives_an_empty_pool tree::no_root_is_made_for_a_collected_node` |
| root 的 clone 只加 count、共用同一個 cell，最後一個丟掉才放回 pool；拆掉 isolate 時 `Roots` 被釋放 | `cargo test -p dom --test it tree::clones_share_their_root`、`cargo test -p dom --lib handle::tests::the_roots_go_with_the_isolate` |
| 成員的 `this` 照它的 interface 可能的型別取，每次只 unwrap 一次；別的型別的 `this` 是 `Illegal invocation`；每個 GC 型別都屬於某個 kind | `cargo test -p dom_js --test it bindings::this_is_unwrapped_as_its_interfaces_type`、`cargo test -p dom_api --test it api::each_kind_says_its_types` |

### fragment 和文件的 parse

| 項目 | 怎麼驗證 |
| --- | --- |
| parser 的 append 跳過拆下、adopt 和祖先的檢查，結果和 `insert_before` 一樣；會接成環的、Document、`Attr` 走 `insert_before` 被拒絕 | `cargo test -p dom --test it tree::the_parsers_append_is_insert_before` |
| fragment 直接 parse 進 context 的文件的 `DocumentFragment`（quirks mode 照 context，context 和它的文件不被動到），屬性的 hook 拿不到 document；fragment 的 `<body onload>` 不設 window 的 handler | `cargo test -p html --test it parse::a_fragment_takes_the_contexts_quirks_mode parse::a_fragment_runs_the_element_hooks_with_no_document`、`cargo test -p dom_js --test it html_element::fragments_are_the_contexts_documents` |
| fragment 的 parse 每個節點大約只建一個 root | `cargo test -p html --test it parse::a_fragment_parse_roots_each_node_about_once` |
| 常見的 markup（fragment 和簡單的整份文件）走 fast path，其他的交給 html5ever；樹和 hook 拿到的行號和完整的 parser 一樣（debug build 每次比對） | `cargo test -p html --test it fast_path::` |

### style

| 項目 | 怎麼驗證 |
| --- | --- |
| 改了屬性或 state 只重算元素自己，規則在 combinator 左邊用到它時才重算子樹；子元素照 D1 的條件重算；改了什麼存到 style 取走；每次改動之後和整份重算比對 | `cargo test -p style --test it invalidation::for_children`、`cargo test -p dom --test it style_flags::what_changed_of_the_attributes_is_kept_until_taken` |
| base URL 改了只重算用文件的 base URL 解析 `url()` 的元素 | `cargo test -p style --test it invalidation::another_base_url_styles_what_resolved_against_it` |
| 子節點進出、兄弟的 state 改了只重算 selector flag 和 `+` 的長度碰得到的兄弟（跳過 text 和 comment） | `cargo test -p style --test it invalidation::siblings`、`cargo test -p dom --test it style_flags::a_change_among_siblings_flags_those_around_it` |
| 兄弟 combinator 左邊的 class、id、屬性、state 用 sibling invalidation set，只重算它找得到的兄弟和後代；set 說不出的照 structural 重算 | `cargo test -p style --test it invalidation::sibling_sets form_controls::a_parts_rule_across_its_hosts_siblings_styles_it_again` |
| UA shadow tree 的部件的 style 由 UA stylesheet 的 `::-internal-` 規則給 | `cargo test -p style --test it form_controls::the_inner_parts_take_the_ua_stylesheets_style` |
| 表單的 pseudo-class 在比對時才算，改變只標記可能受影響的元素，快取在沒有改變時留著；frame 的 box tree 也照 HTML 的規則 | `cargo test -p dom --test it style_flags::form_states_changed_marks_without_storing style_flags::the_form_state_cache_lasts_while_nothing_changes`、`cargo test -p dom_api --test it forms::a_change_marks_the_elements_whose_form_states_it_may_change`、`cargo test -p shell --test it forms::a_frame_shows_a_check_box_checked` |
| `MatchedPropertiesCache`：相同的區塊和 parent 命中，`style` 屬性、`inherit` 了非繼承屬性的不存，viewport、root 的 font size、base URL、stylesheet 改了重算（debug build 每次命中都重新 cascade 比對）；`inherit` 的角照原本的角 | `cargo test -p style --test it matched_properties:: computed::border_radius_values` |
| `querySelector`、`querySelectorAll`、`closest` 的一次走訪只建一份 matching 的狀態 | `cargo test -p dom_api --test it api::a_selector_query_matches_its_walk_with_one_matcher` |

### layout

| 項目 | 怎麼驗證 |
| --- | --- |
| 清單照 Blink 以清單為單位編號，項目改了只重編那個清單；其他的 counter 只在參與者改變時走整份文件；清單外的 `list-item` 由整份文件的 pass 算（debug build 每次和整份文件的 pass 比對） | `cargo test -p layout --test it update::lists_number_their_own_items update::other_counters_are_worked_out_when_they_change update::other_uses_of_list_item_are_numbered_by_the_whole_document update::a_markers_strings_follow_its_style` |
| 文字、影響 shaping 的 style、inline box 的位置都一樣的段落沿用上次的 shaping（debug build 每次命中都再 shape 比對） | `cargo test -p layout --test it shaping::` |

### paint 和 hit test

| 項目 | 怎麼驗證 |
| --- | --- |
| property node 的 id 跨 pre-paint 穩定：新的 node 不讓其他 box 的 id 和 item 位移，空位給之後的 node（排在 parent 之後），空位太多從頭編號（debug build 每次和從頭做的比對） | `cargo test -p paint --test it paint_controller::a_new_node_keeps_the_other_boxes_ids_and_items` |
| pre-paint 只重走改變的子樹，捲動只就地更新；不見的 box 的 node 和 SVG 的狀態被放開；child frame 照它的 layout 和捲動重走 | `cargo test -p paint --test it paint_controller::an_unchanged_subtree_isnt_walked_again paint_controller::scrolling_walks_nothing_again paint_controller::a_gone_boxs_nodes_and_svg_states_are_let_go_of`、`cargo test -p paint --test it child_frames::a_container_in_a_child_frame_scrolls_in_place` |
| 裡面沒有改變的 stacking context 整段沿用上一次的 item，待重畫的元素只讓它所在的 stacking context 重畫；從頭做的 pre-paint 不沿用 | `cargo test -p paint --test it paint_controller::an_unchanged_stacking_context_is_taken_whole paint_controller::an_invalidated_element_paints_only_its_stacking_contexts_again paint_controller::a_pre_paint_from_scratch_takes_no_stacking_context_whole` |
| child frame 不見的 iframe 不搬上一次 paint 的 item | `cargo test -p paint --test it child_frames::an_iframe_whose_child_frame_went_paints_none_of_it` |
| hit testing 找得到的目標在 paint 時記下；蓋住 composited scroller 的東西從記下的目標找，不走樹（debug build 每次 paint 比對記下的目標和走訪找到的）；iframe 的 `pointer-events` 決定它的子 frame 的目標 | `cargo test -p paint --test it hit_test::`、`cargo test -p paint --test it child_frames::child_frames_viewports_in_the_scroll_hit_test_region`、`cargo test -p paint --test it paint_controller::a_child_frames_targets_follow_its_iframes_pointer_events`、`cargo test -p paint --test it svg::an_svgs_targets_are_its_shapes_and_bounding_box_containers` |
| 在 blocking `wheel` listener 範圍裡是 style 的 bit（照 flat tree 繼承，passive 的不算，只重算加減 listener 的元素和它底下的）；沒有 listener 時 wheel event handler region 是空的，有的時候從 paint 記下的、bit 是 true 的目標算，不走樹 | `cargo test -p style --test it invalidation::a_blocking_wheel_listener_styles_what_it_is_over invalidation::a_shadow_roots_wheel_listener_is_over_its_tree`、`cargo test -p paint --test it paint_controller::a_wheel_listener_paints_again_the_boxes_it_is_over child_frames::a_child_frames_listener_is_over_its_canvas`、`cargo test -p renderer --lib frame_view::tests::the_wheel_event_handler_region_follows_the_listeners` |
| raster invalidation 以 chunk 為單位：照原樣畫的 chunk 整個沿用舊的紀錄，改了的只重新 raster 改了的 item；來去、換了順序、狀態不同的 chunk 整個重新 raster | `cargo test -p paint --test it paint_controller::the_chunks_painted_as_they_were_are_taken_whole paint_controller::a_chunk_changed_anyhow_isnt_taken_whole`、`cargo test -p paint --test it raster_invalidation::chunks_that_came_went_or_swapped_are_rastered_again` |
| `SoftwareRenderer` 的整數位置、1:1 的 tile 和純色走捷徑，畫出來和每個像素取樣逐像素一樣 | `cargo test -p viz --lib software_renderer::tests::the_shortcuts_draw_what_sampling_does` |

### 量測工具

| 項目 | 怎麼驗證 |
| --- | --- |
| `--sample` 取 cycle 和指令數兩個事件，每個 suite 各一份 folded 和報告；`--against REV` 兩邊輪流跑、寫 `compare.md`，pin 不同時不 build | `python3 scripts/speedometer_test.py SampleReadingTests CompareTests` |

## 做過的檢查

每段都跑了改到的 crate 的測試、sanity 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`，碰到 style、layout、paint 的段落也跑了 `test.py reftest`（167 個）、`test.py scenarios`（54 到 56 個）和相關目錄的 WPT；每個階段做完時 `cargo test --workspace`（45 個 binary）、`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy 都通過。取樣是 `ci.py speedometer --sample`（第 9 條之後是 `--against`，各 3 次），數字是指令數、cycle 的變化。mutation test 的 spec 是 `mutants-*.toml`，等價的 mutant 記在 `docs/testing.md`。

### 第 1 段（2026-10-16）

- event loop 的 span、A1（沒有 wheel listener 時 region 是空的）、A2、A5（id 表的 prune）、B1（FxHash）、B2（有 wrapper 時不查 interface 和 realm）。`mutants-1.toml` 9 條全部抓到；B1 只改效能，沒有 mutant。
- 取樣（做之前、之後各一次）：jQuery −10.9%、Preact-Complex −6.4%、NewsSite-Next −5.3%。Preact 的 wheel region 146 → 0 個樣本；jQuery 的 SipHash 5.3% → 1.0%。A2 對 Preact 沒有效果（它有 composited scroller，等 E1）；A5 在這些 suite 幾乎是 0。
- trace 上沒被 span 蓋到的 25% 是 kun 做完一個 frame 之後等下一個 frame 的時間；`start_loads` 算的 style 就是那個 frame 的，多出來的是四次全樹走訪（NewsSite-Next 約 10%），放回第 5 段。

### 第 2 段（2026-10-17）

- **基準**（`ci.py speedometer --sample`，`main` 是 `ef93f56c`）：`StyleEngine::recalc_style` 佔 cycle 最多的是 NewsSite-Nuxt 34.1%、Backbone 31.9%、Svelte-Complex 29.9%、WebComponents 29.2%、NewsSite-Next 27.6%、Preact-Complex 27.2%；最少的是 jQuery 3.9%、TipTap 2.2%、Charts-chartjs 0.1%。比較用 NewsSite-Nuxt、Backbone、NewsSite-Next、Preact-Complex（第 11 條預計的 TipTap 不用）。
- **2a**：feature 收集、改了什麼、D1。寫的時候抓到：feature 不分規則的類別時 UA 的 list box 和 spin button 的規則讓每次 focus 重算整棵子樹；漏了 UA 部件的 `::placeholder`、表格的 cell hint、keyframe 的 `inherit`。WPT 抓到 `css-animations/display-none-prevents-starting-in-subtree.html`（D1 只往下一層），`display` 在 `none` 和其他值之間變時改成整棵子樹。`mutants-2a.toml` 第一次 20 條漏 2 條，補了 `:is(.big, .wide)` 和 shadow tree 的 hover 的測試，最後 21 條全部抓到。取樣：Preact-Complex −9.4%（style 765 → 410）；Backbone 一度 style 727 → 792（每次重收 UA sheet 的 feature），改成 UA sheet 只收一次之後持平。
- **2b 開始前的統計**（暫時在 `recalc_style` 裡數每個重算的元素的原因，對上 trace 的 `Document::UpdateStyle`，沒有 commit）：四個 suite 的計時區間裡祖先位置的屬性都是 0；NewsSite 是 base URL 改了的整份重算和 `:checked ~`，Preact、Backbone 是插入加上 structural selector（第 13 條）。
- **2b**：base URL 改了只重算用到它的元素。`layout` 的 `update::what_only_paints_changes_in_place` 抓到 style 自己的簿記被當成排版的差別。`mutants-2b.toml` 8 條全部抓到。三版取樣都沒有效果：NewsSite 換頁時 base URL 和整頁的內容一起換，重算的是新節點（統計改成新節點優先之後 base URL 是 0）。
- **2c**：子節點進出只重算碰得到的兄弟。寫完之後抓到（都先寫了會失敗的測試）：中間是空白的 text 時 `:last-child` 留著舊值（改成跳過 text 和 comment）；`p:not(:nth-child(2n))` 的 reach 是 0；`:host(:nth-child(3))` 不在 parent 的依賴裡（2c 之前就漏了）。`mutants-2c.toml` 最後 19 條全部抓到；`HAS_EMPTY_SELECTOR` 那條等價、拿掉。取樣：Backbone −15.5%（style −38%）、NewsSite-Nuxt −7.7%（style −23%）、Preact-Complex 持平（它的 structural 大多是新節點）。
- **2d**：兄弟位置的 sibling invalidation set。寫的時候抓到：2d 自己的 `a:link + p`、`:lang(fr) + p`、`:is(.x, .y) ~ p`、`<use>` 的 touched 少了 parent（`layout` 的 `update::an_svg_takes_its_content_again_when_its_gradients_change` 抓到）；2c 造成的 `:first-child + p`；2c 之前就有的 `:empty + p`、`.x ~ slot::slotted(span)`。`mutants-2d.toml` 第一次 61 條漏 6 條（兩條等價、三條補測試、一條拆測試），最後 61 條全部抓到（546 秒）。取樣：NewsSite-Nuxt −2.3%（style −8.5%）、NewsSite-Next −1.3%（style −6.5%），Preact-Complex、Backbone 持平；set 只認 `span` 不分第幾個（和 Blink 一樣），比估的少。
- D5 和後代的 invalidation set 延後：照 trace 量之後（和 Chrome 的比較），style 以外的差距更大，計時區間裡也量不到後代的 set 的範圍。

### 第 3 段（2026-10-16 到 2026-10-17）

- **B5**（rusty_v8 的 fork，release `kun-v152.2.0-9`）：`Member` 和 `Persistent` 在 Rust 裡讀，debug build 每次和 C++ 的 `Get` 比對（`dom` 等 10 個 crate 一次也沒有不同）。`mutants-3-b5.toml` 4 條（fork 的檔案）全部抓到。取樣：jQuery −17.1%、Preact-Complex −14.2%，讀取的 FFI 從 1283 個樣本變成 13。fork 的 CI 六個 job 都成功，cargo 下載的 prebuilt 上 10 個 crate 都通過。
- **`insert_before` 和 `Persistent` 的重複使用**：`mutants-3-roots.toml` 7 條全部抓到；`dropped_roots_are_reused_and_root_nothing` 第一版建一個丟一個、抓不到放回前沒清空，改成先 GC 再重用。總樣本的雜訊比差別大；root 相關的樣本 jQuery 14.3–16.2% → 9.7–10.3%，malloc、free 19.5% → 12.8%。
- **錨點 pool 和 `from_value`**：`mutants-3c-anchor.toml` 5 條、`mutants-3c-from-value.toml` 2 條全部抓到；`try_new` 拿到空的來源照樣建 root 是等價的（`NodeIdMap::get` 本來就濾掉）。root 相關的 jQuery 10.2% → 5.0–5.9%。這次發現 `cycles:u` 的總數在不同的 build 之間差到一兩成，比改動大，所以有了第 9 條的量法。
- **clone 只加 count**：`Handle` 從 16 bytes 變成 8，拿掉 `retype` 的 `unsafe`。`mutants-3d-cells.toml` 6 條全部抓到。fragment parse 新建的 root 488 → 103。取樣的總數看不出差別（jQuery −0.2%，A/A 要超過約 2%）；函式層級的 `Assign` 等約少 1.4%，malloc 的指令數隨 heap 的狀態變。
- **不穩定的測試**：`html` 的 `spans::each_run_of_the_parser`（400 次失敗 3 次）。tracing-core 0.1.36 的 callsite interest 是整個 process 共用的快取，同一個 binary 裡沒有 subscriber 的 parse 測試可能讓它存成 never。改成這兩個測試各自用 `current_exe()` 在子 process 裡單獨跑自己，之後 400 次沒有失敗。

### 第 4 段（2026-10-16）

- C2（插入、移除的子樹走一趟；parser 的 append）、C1（fragment 直接 parse 進 context 的文件）。和 Chrome 152 不同的一點：fragment 的 `<body onload>` 不設 window 的 handler（照 HTML；用 Chrome for Testing 確認過 Chrome 會設）。
- `mutants-4-walks.toml` 7 條全部抓到；`mutants-4-append.toml` 7 條第一次漏了「Document 可以被接上」，補了測試之後全部抓到（三條讓樹接成環、測試卡住到逾時，`timeout` 降到 60 秒）；`mutants-4-fragment.toml` 第一次 6 條漏 2 條，是走不到的程式，刪掉，剩 4 條全部抓到。
- 取樣：jQuery 的 `innerHTML` 底下約少 5.5%（append −30%、parse 完的 adopt 歸零）；html5ever 的 tokenizer 和 tree builder 約 27%，C1、C2 不碰。

### 量測的方法（2026-10-17）

- `--sample` 取 `cycles:u` 和 `instructions:u`，`--against REV` 兩邊輪流跑。`speedometer_test.py` 57 個測試通過；`mutants-measure.toml` 10 條全部抓到。
- A/A（`--against HEAD`）：jQuery 的指令數中位數 +0.4%、cycle −5.6%；Preact 的工作量不固定（指令數每次差 2450 到 3108）。之後的比較看指令數，超過約 2% 才算有差，cycle 要差超過約 6%。

### `SoftwareRenderer`（2026-10-17）

- 照階段量時看到 main thread 在等 Viz 的 `Display::DrawAndSwap`（NewsSite-Nuxt 一個 frame 92 ms）：整數位置、1:1 的 tile 和純色走捷徑。`mutants-viz.toml` 10 條：第一次漏了純色的 opacity（測試的對照也走純色的捷徑），改了之後全部抓到。
- `DrawAndSwap` 的中位數 NewsSite-Nuxt 92 → 6.8 ms、Svelte 84 → 7.7 ms；`--browser kun` NewsSite-Nuxt 374.8 → 298.9 ms、Preact 98.2 → 88.0 ms。剩下的是 compositor 的 raster（`TileManager::PrepareTiles`）。

### 第 5 段（2026-10-17 到 2026-10-18）

- **5a**：`<base>`、`<link>`、背景圖片不再在每次 script 之後走整份文件；UA shadow tree 的部件的 style 搬進 UA stylesheet。`mutants-5a.toml` 第一次漏 4 條，補了測試，23 條全部抓到。取樣：NewsSite-Next −11.1%、NewsSite-Nuxt −9.3%、Svelte-Complex −5.6%、jQuery −5.1%。
- **5b1**：UA shadow tree 延後到 style 更新前。`mutants-5b1.toml` 第一次 6 條漏 3 條：兩條補了測試，hit test 前那一條測不到（headless 和情境都是畫出 frame 才送輸入），記進 `docs/testing.md`，最後 5 條全部抓到。取樣：jQuery −10.6%。
- **5b2**：表單的 pseudo-class 在比對時才算。第一版 ES6-Webpack +5.7%：`type` 的改變把整份文件的 radio 和 submit 按鈕都標記了，改成只標記有 form 的 submit 按鈕。`mutants-5b2.toml` 20 條全部抓到（第一次漏了 frame 的 box tree，補了 `a_frame_shows_a_check_box_checked`）。取樣（加 5b1）：jQuery −12.2%、Backbone −3.3%。
- **5c1**：cascade 的浪費（initial style 只建一次、`initial` 不 clone 整個 style、transition 的時間只算一次）。`mutants-5c1.toml` 5 條全部抓到（補了 `inherit` 的左上角）。取樣：NewsSite-Nuxt −10.8%、Vue −7.0%、Stockcharts −5.4%、Svelte-Complex −4.6%。
- **5c2**：`MatchedPropertiesCache`。第一版 Stockcharts +220%（屬性不同的 SVG 元素同一個 hash 串在一起）、observable-plot +36%，照 Blink 每個 hash 一條、hints 摘要進 hash、base URL 比位置、FxHash 之後修好。sanity 的 `boundary` 擋下兩個 `LazyLock`，改成 `thread_local!`。`mutants-5c2.toml` 第一次 13 條漏 5 條（兩條補測試、三條等價），最後 10 條全部抓到。取樣（加 5c1）：20 個 suite 合計指令數 −4.8%、cycle −5.9%（Preact-Complex −23.1%、NewsSite-Nuxt −14.6%、Backbone −14.1%、NewsSite-Next −13.6%）。
- **5d1**：清單照 Blink 以清單為單位編號（`ListItemOrdinal`）。第一版照 `MarkCountersDirty` 縮小觸發條件，NewsSite 反而變慢（`display: none` 的 `<ul>` 當成重建；選單的 `<ul>` 每一步切換 `display`），查了 Blink 之後照它拆開。WPT 的 `css/css-lists/marker-dynamic-content-change.html` 第一次失敗（marker 的字串從上一次的 pass 拿），修了。`mutants-5d1.toml` 第一次漏 3 條，補了測試，18 條全部抓到。取樣：NewsSite-Nuxt −8.4% 到 −9.3%（counters 442 → 9）、NewsSite-Next −6.3% 到 −7.4%，20 個 suite 合計指令數 −1.3%。
- **5d2**：parley fork 的 shaping 快取。`test.py mutate` 的原檔路徑有 `..` 時還原不了，fork 的 6 條用暫時的腳本跑，都抓到（inline box 的位置那條只在 hash 碰撞時有差，暫時固定 hash 確認）。取樣（暫時讓快取一律沒有命中來對照）：WebComponents、NewsSite-Nuxt −4.0%、Stockcharts −3.2%、Preact-Complex −2.6%。
- **第 5 段做完時**：clippy 抓到 5c2 的測試用 `drop(m)` 結束借用。`test.py automation`（Fedora 44、Wayland）三個情境是 parley 的 debug 比對觸發（快取的 shaping 拿著字型檔的舊副本，內容一樣），改成字型照資料比較之後通過；另外五個是這台 Linux 的視窗大小（`docs/limitations.md` 的平台和建置那一節記過，`main` 上也一樣）。

### 第 6 段（2026-10-18）

- **6a1**：property node 的 id 穩定、不走第二次 pre-paint。拿掉「parent 在前」的第一版讓 Vue 的 `to_root` 變慢，使用者決定改回（第 22 條）；空位太多就從頭編號。`mutants-6a1.toml` 11 條全部抓到，「拿掉的 transform node 不清空」等價。取樣：Complex-DOM 的 TodoMVC −7.2% 到 −9.4%（paint 的快取不再因為 id 位移而沒命中）。
- **6a2**：只重走改變的子樹、捲動就地更新。`mutants-6a2.toml` 第一次漏 4 條：child frame 的 viewport 就地更新的 2 條是多餘的程式、拿掉，換成 2 條新的、補了測試；另外 2 條在目前的 layout 下等價（暫時的紀錄在測試、reftest、情境和 4285 個 WPT 測試裡都沒碰到）。最後 15 條全部抓到。取樣：NewsSite-Nuxt −8.1%（pre-paint 901 → 209）、NewsSite-Next −4.4%；Vue +0.4%（每一步幾乎每個項目都變，沿用幫不上）。
- **6b1**：paint 的 subsequence 快取。`mutants-6b1.toml` 第一次 16 條漏 6 條：補了 3 個測試，`before` 那條連同程式拿掉，2 條等價；最後 13 條全部抓到。取樣：Complex-DOM 的 TodoMVC −4.9% 到 −7.8%、NewsSite-Nuxt −3.7%。
- **6b2**：沿用的 item 共用 glyph 和多邊形（`Arc<[T]>`）。`gfx::Path` 也共用的版本讓 Charts-chartjs +1.0%，拿掉。沒有新的行為、沒有 mutation test。取樣：Angular −1.8%、Svelte −1.2%。
- **E1a**：hit test 的目標在 paint 時記下，covered scroller 從它算。debug 的比對抓到 layout 給空的行上的文字無限大的位置和 NaN 的寬度（WPT 的 4 個測試），矩形改成照 bit 比。`mutants-e1a.toml` 第一次 20 條漏 5 條：補了 4 個測試，空矩形不進外框那條等價、連同程式拿掉；原本記成等價的 mutant（找不到成為 layer 的容器自己的盒子時當成沒被蓋到）其實走得到（`pointer-events: none`），補了測試。最後 19 條全部抓到。取樣：Complex-DOM 的 TodoMVC −5.2% 到 −7.6%。
- **E1b**：wheel 的範圍是 style 的 bit，region 從記下的目標算；照 flat tree，不再照 DOM 的 parent。順帶修掉：child frame 不見的 iframe 照樣從上一次 paint 搬舊的 child frame 的 item（情境 `iframe-loads` 在 debug 的目標比對時發現）。`mutants-e1b.toml` 第一次 20 條漏 3 條，補了 SVG、子 frame 的 canvas、整份文件的測試，最後 21 條全部抓到。取樣：React-Complex −5.1%（root 有 blocking 的 listener，原本每次 paint 走整棵樹）。
- **E4**：raster invalidation 以 chunk 為單位。`mutants-e4.toml` 第一次 12 條漏 9 條（原本的 raster 測試幾乎都只有一個 chunk），補了測試，一條多餘的條件連同程式拿掉，兩條等價；最後 9 條全部抓到。`LayerSpace::to_layer` 改成只算 chunk 用到的 node。取樣：raster invalidation 的樣本約剩三分之一，Angular −3.5%。
- **第 6 段做完時**：`cargo fmt --check` 改了排版；clippy 抓到 6b2 多餘的 `.into()`、`scroll_nodes()` 多餘的 `into_iter()`、E1b 的 `unsafe` 少了貼在前一行的 `SAFETY`，都修了。

### 第 7 段（2026-10-18）

- **7a**：`this` 照 interface 的型別取（`Templates::receivers`），`ApiWrapper` 只確認一次；`crates/dom/soundness.md` 的 I14 跟著改。`mutants-7a.toml` 5 條全部抓到。取樣：Charts-chartjs −20.5%、Perf-Dashboard −11.4%。
- **7b**：selector 查詢的一次走訪只建一份 `SelectorCaches` 和 `MatchingContext`（只共用 caches 的第一版 −12.4%）。`mutants-7b.toml` 3 條全部抓到。取樣：ES6-Webpack −14.0%（`querySelector` 的樣本 6012 → 3615）。

### 第 8 段（2026-10-18）

- **C3**：`html` 的 fast path，debug build 每次收下時也用完整的 parser 比對（WPT 的 parse 相關目錄 1955 個測試裡沒有一次不同）。`mutants-8-c3.toml` 第一次 13 條漏 2 條：「大寫的名字也收」是多餘的檢查、連同程式拿掉，`\r` 那條補了行號的測試；最後 12 條全部抓到。取樣：jQuery −24.6%、ES6-Webpack −13.6%。
- **整份文件的 fast path**：mutation test 加 4 條，16 條全部抓到；大多數 crate 的測試頁面是沒有 DOCTYPE 的 `parse_document`，debug 的比對每次都跑。取樣：ES5 −17.7%。
- **第 8 段做完時**：clippy 抓到 `fast_path::same_tree` 裡可以併進 `match` 的 `if`，修了。

### 收尾（2026-10-18）

- `cargo test --workspace`（45 個 binary）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、sanity 五項都通過。`perf` profile 的 build 抓到兩處只有 debug build 用到、release 報沒有用到的：hit test 的 `Target` 的 `container`、`wheel`（第 6 段，`cfg_attr` 拿掉 warning）和 fast path 的 `NodeKind` 的 import（只在 debug 的樹比對裡用），都修了；clippy 和 `cargo test` 是 debug build，看不到這種 warning。
- 文件：`docs/architecture.md` 加上加新東西時要跟著改的地方，`docs/testing.md` 加上快取和增量的交叉檢查，`CLAUDE.md` 指過去；`docs/limitations.md` 的 `MatchedPropertiesCache` 和 shaping 的快取改成現在的樣子。

### 不穩定、之前就失敗的測試

都在 `0051` 之前的 commit 上單獨跑過，不是這份 RFC 造成的；當時還沒查的照舊沒查。

- `custom-elements/reactions/Animation.html` 3 項意外通過（預期是別的平台上寫的；`9dc1f003` 上也一樣）。
- `css/css-transitions/event-dispatch.tentative.html` 在平行跑的負載下逾時、單獨跑都通過（`main` 上整組跑兩次也逾時）；`css/css-pseudo/marker-hit-testing.html` 逾時（`main` 上一樣）。
- `html/semantics/forms/the-input-element/focus-dynamic-type-change-on-blur.html` 整批跑時逾時、單獨跑通過；`focus-dynamic-type-change.html` 是標成 known intermittent 的那一個。
- `shadow-dom/scroll-to-the-fragment-in-shadow-tree.html` 的一個子測試只在 2c rebase 之後的第一次 FAIL（取決於捲動和 timer 的先後），之後和單獨跑都通過，沒有標 known intermittent。
- `css/cssom-view/` 的捲動 promise 和 smooth scroll 10 個、`intersection-observer/transform-animation.html`（`43d186cb` 上一樣）。
- `css/CSS2/tables/table-backgrounds-bc-colgroup-001.xht`、`-bs-colgroup-001.xht` 時好時壞，`-bc-column-001.xht` 失敗；`html/canvas/element/fill-and-stroke-styles/2d.fillStyle.parse.system.html` 意外通過（`43d186cb`、`21f4006f`、`aefb8cac`、`089b785f` 上一樣）。
- `dom/events/scrolling/` 的 `wheel-event-transactions-target-move.html`、`-target-removal.html`、`scrollend-event-for-user-scroll.html`、`scrollend-event-fires-for-repeat-key-ending-after-scroll-container-end-is-reached.html`（`87cd9026`、`f3ed9262` 上一樣）。

### 和 Chrome 的比較

`ci.py speedometer`：20 個預設的 suite，兩邊各 10 個 iteration，每個 suite 開新的瀏覽器，kun 是 `perf` profile，Chrome for Testing 152.0.7977.82，取每個 iteration 的 suite 總時間的中位數。Editor-CodeMirror 一直無效（kun 的第 0 個 iteration 少兩個 `div`，139 對 141），2d 之後的 Editor-TipTap 也無效（Chrome 自己少了 `ProseMirror-focused`）。

- **比值**（合計 kun / Chrome，各 suite 比值的幾何平均）：2d 之後 3.54、3.52（18 個）→ 第 6 段之後 2.98、2.82（19 個；和 2d 同樣的 18 個是 2.91、2.77，kun 合計 −27%）→ 第 7 段之後兩次 2.85、2.59 和 2.86、2.55。兩次的合計差 10%（兩邊一起變快變慢），單一 suite 兩次差到 20–30%，只看整體。
- **Charts-chartjs**：完整量測時 310–323 ms，只跑 kun 時連三次約 220 ms（指令數也少兩成）；它跑在 Chrome 那一輪之後，原因還沒查。
- **差距在哪裡**（每個 test 的 sync、async 的中位數加總）：2d 之後 sync 多 1962、async 多 882；第 6 段之後 sync 多 1424、async 多 480，剩下的七成五在 sync（JS 和它呼叫的 DOM）。

第 7 段之後第二次的各 suite（ms），和前兩次的比值：

| suite | kun | Chrome | 比值 | 第 6 段之後 | 2d 之後 |
|---|---:|---:|---:|---:|---:|
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 249.0 | 39.1 | 6.37 | 9.52 | 8.48 |
| TodoMVC-jQuery | 758.2 | 122.6 | 6.18 | 7.74 | 9.05 |
| TodoMVC-JavaScript-ES5 | 150.6 | 42.1 | 3.58 | 5.04 | 5.30 |
| TodoMVC-WebComponents | 71.0 | 20.8 | 3.41 | 4.69 | 4.28 |
| TodoMVC-Preact-Complex-DOM | 35.8 | 11.7 | 3.06 | 3.13 | 5.17 |
| Editor-TipTap | 214.5 | 73.1 | 2.93 | 3.96 | 無效 |
| Perf-Dashboard | 175.4 | 62.5 | 2.81 | 2.11 | 2.54 |
| TodoMVC-Svelte-Complex-DOM | 41.6 | 15.2 | 2.74 | 4.03 | 6.99 |
| TodoMVC-Lit-Complex-DOM | 56.9 | 21.1 | 2.70 | 2.60 | 5.31 |
| NewsSite-Nuxt | 173.2 | 69.2 | 2.50 | 1.78 | 4.10 |
| TodoMVC-Backbone | 70.2 | 28.2 | 2.49 | 2.91 | 2.72 |
| Charts-chartjs | 309.6 | 127.2 | 2.43 | 2.80 | 2.81 |
| TodoMVC-Vue | 56.7 | 26.0 | 2.18 | 2.18 | 3.05 |
| NewsSite-Next | 170.4 | 80.7 | 2.11 | 3.16 | 2.53 |
| React-Stockcharts-SVG | 190.8 | 105.9 | 1.80 | 1.25 | 2.35 |
| TodoMVC-Angular-Complex-DOM | 87.8 | 52.1 | 1.69 | 1.60 | 2.82 |
| TodoMVC-React-Redux | 73.4 | 45.7 | 1.61 | 1.77 | 1.30 |
| TodoMVC-React-Complex-DOM | 90.1 | 62.6 | 1.44 | 1.61 | 2.32 |
| Charts-observable-plot | 77.1 | 62.7 | 1.23 | 1.30 | 1.64 |

第 6 段之後照階段分（`ci.py speedometer --trace --iterations 3`，暫時的腳本算 renderer main thread 上每個 span 在計時區間裡的 self time，Chrome 的 trace 時間是 µs、乘 1000；19 個 suite，Perf-Dashboard 的計時區間不對、不算；只看比例），每個 iteration 的 ms：

| | script | parse | GC | style | layout | paint | canvas | 其他 | 等 Viz |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| kun | 1338 | 394 | 32 | 656 | 225 | 123 | 99 | 97 | 338 |
| Chrome | 639 | 181 | 44 | 116 | 191 | 127 | 4 | 79 | 155 |
| 多的 | 699 | 214 | −11 | 540 | 34 | −5 | 95 | 17 | 182 |
| 佔差距 | 40% | 12% | −1% | 31% | 2% | 0% | 5% | 1% | 10% |

- 和 2d 之後比（同樣 17 個 suite）：差距 2582 → 約 1585；paint 116 → 0、等 Viz 536 → 183、style 628 → 414、script 844 → 679。
- 取樣裡的大項：Charts-chartjs 的 script 有 29% 是 canvas 的 Skia raster、約 18% 是取 `this` 的 wrapper（第 7 段的 7a）；ES6-Webpack 的 17% 是 selector 查詢（7b）；jQuery 的 46% 在 parse（第 8 段）；TipTap 的 style、layout 幾乎都是 parley、harfrust 的 shaping、選字型和斷行；React-Stockcharts 的 script 有 31% 是 V8 的 megamorphic IC（JS 本身）。判斷寫在 README 的第 26 條。
