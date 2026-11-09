# RFC 0059 的進度：style 的效能

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-19，macOS（Apple M4）。完成日期：2026-10-19，macOS 26.6.2（Apple M4），`rust-toolchain.toml` 釘住的 rustc 1.99.0。在 `0059` 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 查清楚和量測的工具：`--trace` 的各階段報告、stylesheet 算成「變了」的原因的 log、整份重算的觸發點（D4）、`recalc_style` 本身的時間 | 完成 | 2026-10-19 |
| 2 便宜的：2a rule set 和快取的 FxHash（B2a），2b `start_loads` 不在 stylesheet 到齊前算 style（D5a），2c `rendered` 跟著走樹帶下來（T1），2d presentation attribute 存在元素上加共用快取（C1a、C2a），2e `url()` 在解析時解析（U2a） | 完成 | 2026-10-19 |
| 3 bloom filter（B1），和名字預先算好的雜湊（B2 的另一半，H1） | 完成 | 2026-10-19 |
| 3 之後：`forget_moved` 只看走訪遇到的被移除的根（第 25 條） | 完成 | 2026-10-19 |
| 4 stylesheet：4a D1，4b 子 frame 的環境（K1a、L1），4c document 的 stylesheet 改動用 invalidation set，4d shadow root 和 CSSOM 改動，4e class、id、attribute、狀態的改動 | 完成 | 2026-10-19 |
| 5 `ComputedStyle`：5a A4（S1a、S2-key、S3a）、5b A3（三個 `Arc` 組）；5c 不做（第 57、59 條） | 完成 | 2026-10-19 |
| 6 量測和決定：6a V2、V4（第 58 條），之後結束（第 59 條） | 完成 | 2026-10-19 |

## 驗收

### 量測的工具

| 項目 | 怎麼驗證 |
| --- | --- |
| `ci.py speedometer --trace` 寫出 `phases.md` 和 `phases.json`：每個 suite 計時區間裡各階段（script、style、建 box tree、layout、paint、parse、GC、canvas）的 self time，kun 和 Chrome 並列、差多少，加上合計；沒分類到的 slice 列前 10 名 | `python3 scripts/speedometer_test.py PhasesTests` |
| 只算 runner 的 mark 最多的那個 thread、只算計時區間裡的部分；巢狀的 slice 時間歸最裡面那個 | 同上（`test_each_phase_takes_its_slices_self_time_in_the_windows`） |
| kun 和 Chrome 各自的 slice 名稱對到分類（V8 的 GC 算 GC、Chrome 的 `InlineNode::` 算 layout） | 同上（`test_each_browser_names_its_phases`） |
| 真的 kun 和 Chrome 的 trace 都分得出 script 的時間 | `python3 scripts/speedometer_test.py MeasureTests.test_traces_of_both`（Chrome for Testing 下載過才跑） |

### Style

| 項目 | 怎麼驗證 |
| --- | --- |
| 還有 render-blocking 的 stylesheet 在載入時，`start_loads` 不算 style、`load` 等著；最後一份到了之後再算一次，載入它的規則裡的背景圖片（照 stylesheet 的 URL 解析） | `cargo test -p renderer --lib -- frame_view::tests::background_images_wait_for_the_stylesheets` |
| 元素有沒有 render（動畫和 transition 用）跟著走樹帶下來，和往上走祖先的結果一樣：`display: none` 的 host 的 shadow tree、`display: none` 的 slot 拿到的元素、slot 有東西時它的 fallback；debug build 每個元素都和往上走比對 | `cargo test -p style --test it -- css_animations::`；交叉檢查在所有 debug build 的測試裡 |
| 沒有動畫、沒有 transition、表裡也沒有的元素，style change 查一次表就返回 | `cargo test -p style --test it -- transitions:: css_animations::` |
| SVG 元素的 presentation attribute 解析一次、存在元素上：attribute 改了重新解析，`class` 改了沿用；debug build 每次重新解析比對 | `cargo test -p style --test it -- svg::presentation_attributes_are_kept_until_they_change` |
| 相同的 attribute 和值共用解析的結果，`d` 不放進共用的快取；存到 4096 個值就清掉重來；debug build 和不經過快取的結果比對 | 同上，和 `svg::the_presentation_attribute_cache_starts_again` |
| `RuleSet` 的桶、matched properties cache、`custom_seen` 用 FxHash：只換雜湊函式，行為不變 | 現有的測試 |
| 祖先的 bloom filter 只略過不會符合的 selector：每個元素的 style 和每條規則都完整比對的一樣，跨過子樹之後 pop、新的祖先鏈 push，quirks 模式（不分大小寫）、`:is()`、SVG；debug build 每條略過的照樣比對確認 | `cargo test -p style --test it -- matching::the_bloom_filter_gives_the_same_styles`；交叉檢查在所有 debug build 的測試裡 |
| 真的略過：祖先沒有那個 class 時，`.x .cN` 不比對；有那個 class 的祖先已經結束之後也不比對 | `cargo test -p style --test it -- matching::few_selectors_are_matched` |
| shadow tree 的元素的祖先到 shadow root 為止；`::slotted()` 的規則不經過 bloom filter；shadow tree 之後的 light tree 的元素不受它 push 的影響 | `cargo test -p style --test it -- shadow::the_bloom_filter_follows_each_trees_ancestors` |
| 同一次重算裡兩個不同子樹深處的元素：每個比對前才同步到自己的祖先 | `cargo test -p style --test it -- invalidation::the_bloom_filter_follows_what_is_styled_again` |
| `:has()` 的 subtree filter 有子孫的名字：之後該符合的 `:has()` 不被它排除 | `cargo test -p style --test it -- matching::has_s_filter_has_the_subtree_s_names` |
| id、class 的桶照名字的雜湊，不分大小寫（quirks 模式的文件）；`dom` 在 `id`、`class` 改了時更新雜湊 | `cargo test -p style --test it -- matching::the_buckets_give_the_same_styles invalidation::` |
| recalc 結尾只把走訪遇到的被移除的根拿掉（插回來的一定會被重新算）；`dom` 只記被移除的元素；debug build 確認這份 document 裡沒有插回來卻沒被走到的 | `cargo test -p style --test it -- css_animations::what_moves_back_in_is_styled_anew`；確認在所有 debug build 的測試裡 |
| 元素移到另一份 document（iframe 的）之後，下一次 style 的改變照樣開始 transition | WPT 的 `css/css-transitions/transition-events-with-document-change.html` |
| `url()` 在解析時解析：linked sheet 用它的 URL，`<style>`、`style` attribute、presentation attribute 用當時 document 的 base URL，`var()` 替換出來的用寫 `var()` 那裡的，`<style>` 的 `@keyframes` 也是；之後 base URL 變了都不變，一份 `<style>` 改了時，沒改的（document 和 shadow tree 的）保留原本的 sheet；之後設的 `style` attribute、改了的 `<style>` 用新的 | `cargo test -p style --test it -- cascade::urls_resolve_as_they_are_parsed` |
| base URL 變了不重算任何元素 | `cargo test -p style --test it -- invalidation::another_base_url_styles_nothing_again` |
| 沒有 base URL 的 constructed sheet 的 `url()` 照寫的留著 | `cargo test -p style --test it -- cascade::adopted_sheets_cascade_after_the_documents_own` |
| 整個瀏覽器裡：之後插入的 `<base>` 不改已經解析的 `style` attribute，`element.style` 的 `setProperty` 和 `cssText` 用新的，同樣的值再設一次不算改變（沒有 mutation record）；constructed sheet 的規則用建立時的 base URL（`insertRule`、規則的 `style` 也是）；`animate()` 的 keyframe 用 document 的；`about:blank` 和 `srcdoc` 的 frame 用父 frame 的 | `cargo test -p shell --test it -- style_sheets::urls_resolve_as_they_are_parsed` |
| stylesheet 算成「變了」、要整份重算時，`style=debug` 記下原因（第一次、別的 document、被標記、`<link>` 變了、media 變了） | 只是 log，不測；第 1 段用它查出觸發點（下面「做過的檢查」） |

### Script 等 stylesheet

| 項目 | 怎麼驗證 |
| --- | --- |
| parser 的 script 等 document 自己的 parser 插入、`media` 符合、還在載入的 `<link rel=stylesheet>`：inline 的（等待時照樣是 `currentScript`）、已經載入的 external、`defer` 和 module；`async`、script 插入的 `<link>`、`innerHTML` 的 `<link>`、`media` 不符合、中途移除或 `rel` 改掉、載入失敗的都不擋 | `cargo test -p shell --test it -- network::stylesheets_block_the_parsers_scripts` |
| `Scripts` 只在 stylesheet 都到了才交出擋住 parser 和 `defer` 的 script，`async` 和照順序的不等 | `cargo test -p renderer --lib -- script::tests::the_parsers_scripts_wait_for_the_stylesheets` |
| script 插入的 stylesheet 還在載入時，script 讀到的版面照手上的樣式排 | `cargo test -p shell --test it -- geometry::a_render_blocked_document` |
| WPT：`css/css-tables/` 讀 `support/base.css` 排出來的版面、`script-text-modifications(-csp)`、`moving-between-documents/` 的 after-prepare | `test.py wpt css/css-tables/ html/semantics/scripting-1/` |

### 子 frame 的環境

| 項目 | 怎麼驗證 |
| --- | --- |
| `matchMedia` 要還沒排版的子 frame 的大小、子 frame 在等 stylesheet（render-blocked，或有已經接上、還沒開始載入的 `<link>`）時，只排版外面的 frame、拿 iframe 的內容框，不算子 frame 的 style；別的 document 的 `<link>` 不算 | `cargo test -p shell --test it -- network::a_frames_media_queries_before_its_link_loads_leave_it_unstyled`；`cargo test -p renderer --lib -- frame_view::tests::a_frame_waits_for_its_links_from_their_insertion` |
| 不在等 stylesheet 的子 frame 照舊排版：script 改了 iframe 的寬度後馬上讀 `matches`，之後照樣發 `change` | `cargo test -p shell --test it -- geometry::a_child_frames_media_queries_follow_its_iframe` |
| `start_loads` 算子 frame 的第一次 style 前先給它 iframe 的大小：寬度的 media query 第一次就符合，沒有 transition；排版過的、在等 stylesheet 的、主 frame 不給 | `cargo test -p shell --test it -- geometry::a_child_frames_first_style_sees_its_iframes_size`；`cargo test -p renderer --lib -- frame_view::tests::a_child_frame_styled_before_its_layout_is_sized_first` |
| 元素或它的任何一個 pseudo-element（包括 `::marker`）用到 viewport 單位時，viewport 改了就重算 | `cargo test -p style --test it -- media::a_pseudo_elements_viewport_units_follow_the_viewport`；WPT 的 `css/css-pseudo/marker-content-014.html` |

### stylesheet 和屬性的改動

| 項目 | 怎麼驗證 |
| --- | --- |
| document 多了、少了的 stylesheet 只重算它的規則可能符合的元素（照最右邊那一節的特徵，沒有時找左邊最近有的那一節：子樹或之後的兄弟）；只算 `@media` 條件成立的規則 | `cargo test -p style --test it -- invalidation::an_added_sheet_styles_what_its_rules_may_match invalidation::a_removed_sheet_styles_what_its_rules_matched invalidation::the_document_s_rules_find_its_own_tree` |
| 規則找不到特徵、含 `:has()` 的 sheet 被移除時整份重算 | `cargo test -p style --test it -- invalidation::what_no_feature_finds_styles_everything_again invalidation::a_removed_sheet_with_has_styles_everything_again` |
| UA shadow tree 的部件（`::placeholder`）的規則重算 host 的子樹 | `cargo test -p style --test it -- form_controls::a_changed_sheet_styles_the_parts_its_rules_match` |
| 整個瀏覽器裡：一份一份插入的 `<style>`（像 Lit 的圖示）只重算它符合的 | `cargo test -p shell --test it -- style_sheets::sheets_that_come_and_go_style_what_they_match` |
| shadow root 的 sheet：`:host` 的規則重算 host，`::slotted()` 的重算分配到 slot 的節點（slot 分配到 slot 也是）；整份重算只到那棵 shadow tree、host 和分配到 slot 的節點；sheet 的順序變了整棵重算 | `cargo test -p style --test it -- invalidation::host_and_slotted_rules_style_the_host_and_what_is_assigned invalidation::everything_of_a_shadow_tree_stops_at_its_tree invalidation::slotted_rules_reach_through_a_slot_assigned_to_a_slot invalidation::a_shadow_trees_sheets_in_another_order_style_all_of_it` |
| constructed stylesheet 被 `insertRule` 這類 CSSOM 改動時，只用增減的規則判斷（同一個 `Arc` 的規則互相抵消），document 和 shadow root 同時 adopt 也是 | `cargo test -p style --test it -- invalidation::an_edited_sheet_styles_what_its_edit_may_match`；`cargo test -p style --lib -- rule_set_invalidation` |
| class、id、attribute、狀態改了時照後代的 invalidation set 只重算規則可能符合的後代：主體沒有特徵時整棵子樹、`:is()` 的每個主體、`:hover`、host 的改變帶進 shadow tree、slot 的改變到分配到它的節點 | `cargo test -p style --test it -- invalidation::descendant_sets::` |
| debug build 的交叉檢查：stylesheet 的改動沒找到的元素逐一比對變動的規則、確認都不符合；後代的 set 跳過的元素用舊的做法（重算整棵子樹）重新 cascade、比對 style 和 pseudo-element 的 style，只為了檢查而走到的元素不套用任何 set | 在所有 debug build 的測試裡；WPT 的 `css/css-scoping/`、`css/selectors/invalidation/` 全部（`--all`）沒有 panic |

### `ComputedStyle` 的共用

| 項目 | 怎麼驗證 |
| --- | --- |
| matched properties cache 存調整完的 `Arc`，規則和位置（`Position`：root、flex 或 grid 的子元素、表單控制項、blocking 的 `wheel` listener、pseudo-element 的種類）都相同的元素和 pseudo-element 共用同一份；debug build 每次命中照樣 cascade 加調整比對 | `cargo test -p style --test it -- matched_properties::elements_alike_share_their_style matched_properties::pseudo_elements_alike_share_their_style`；交叉檢查在所有 debug build 的測試裡 |
| menu list 的內層元素（調整讀 select 的值）不進快取 | `cargo test -p style --test it -- form_controls::a_menu_lists_inner_element_follows_its_own_select` |
| 重算出來的 style 和舊的相同時留舊的 `Arc` | `cargo test -p style --test it -- matched_properties::the_same_style_again_keeps_the_old_one` |
| 少用的欄位在 `rare`、`backgrounds`、`svg` 三組：沒設的元素共用同一份組、設了的各自一份、`svg` 和 parent 相同時沿用 parent 的、比較的是內容 | `cargo test -p style --test it -- style_groups::` |
| 移回來的元素的子樹和 shadow tree 不從移除前的 style 開始 transition（被移除的根跟著走訪往下帶；debug build 照樣往上走比對） | `cargo test -p style --test it -- css_animations::what_is_under_an_element_moved_back_doesnt_transition css_animations::what_is_in_a_shadow_tree_moved_back_doesnt_transition` |

## 做過的檢查

每一段都在 macOS 26（Apple M4）上做，日期都是 2026-10-19。每段的 mutation spec 是這個目錄的 `mutants-*.toml`；「測試」只列那一段跑的，階段的檢查在每個階段做完時跑。

**第 1 段**：
- D4：暫時加 log（document 的 URL、已有的 stylesheet 數、backtrace）跑 TodoMVC-jQuery 和 Preact-Complex-DOM，`<link>` 晚到的整份重算全部從 `FrameView::start_loads` 來（不管 render-blocking 的 stylesheet 到了沒有就算 style 找背景圖片），不是 script 讀幾何（`details.md` 的「整份重算的觸發點」）。
- `recalc_style` 自己的時間：用現在的 build 重新取樣、`atos -i` 對回 inline 的函式，只有 4.2%，草稿的 16% 是舊的 build 的 inline 方式，不必加暫時的 `#[inline(never)]`。
- `speedometer_test.py` 的 `KunTests.test_kun_tells_its_gpu` 在 macOS 上原本就失敗（`MACHOPO_GPU_ADAPTER=cpu` 拿不到 CPU 的 adapter），改成 macOS 上確認 kun 不回報 adapter。
- mutation test（`mutants-1.toml`）13 條都抓到。

**第 2 段**：
- 2a 只換雜湊函式，沒有新的測試和 mutation test。
- 2b 讓 WPT 的 `css/css-transitions/render-blocking/no-transition-from-ua-to-blocking-stylesheet.html` 通過（以前 stylesheet 到之前就算了 style，`color` 從初始值開始 transition）。
- 2e：以前 `<style>` 的 `@keyframes` 和 `animate()` 的 keyframe 裡的 `url()` 根本不解析，一起修好。DOMParser 的文件在解析之後才設 URL：改了也觀察不到，沒改。測舊行為（base URL 變了就重新解析）的三個測試換成新的。WPT 的 `css/cssom/css-style-attr-decl-block.html` 那個 base URL 的子測試在加上 `HTMLBaseElement`（另一個 commit）之後通過。
- mutation test：`mutants-2.toml` 第一輪漏了 4 條（`display: none` 的 host 的 shadow tree、`display: none` 的 slot 拿到的元素、slot 的 fallback），補了測試後 16 條抓到；沒抓到的「拿到的 slot 沒有 style 時當成沒有 slot」走不到（shadow tree 一定比 host 的子節點先走）。`mutants-2e.toml` 第一輪漏了「片段的 paint 也解析」（算的時候和解析的時候各檢查一次片段，拿掉多餘的那一道），最後 21 條都抓到。
- 測試：`test.py wpt css/css-backgrounds/ svg/painting/ css/css-animations/ web-animations/ html/semantics/document-metadata/ css/css-variables/ css/cssom/`、`test.py reftest svg/ backgrounds/ images/ shadow/ invalidation/` 沒有非預期的結果，除了 `web-animations/timing-model/animations/updating-the-finished-state.html` 的 hold time 是 `-0`：stash 掉這一段在原本的程式上也一樣，原本就有。

**第 3 段**：
- `CssIdent` 的 `PrecomputedHash` 原本固定回傳 0（`AncestorHashes` 等於沒有），改用 `name_hash`。
- `selectors` 只在同一個元素第二次查 `:has()` 時才建 subtree filter，所以測試要同一個元素對兩條以上的 `:has()`。
- mutation test（`mutants-3.toml`）：漏了「`:has()` 的 filter 拿不到名字」，補測試後抓到；「每次從根重建祖先」只變慢、結果一樣，是等價的。最後 14 條都抓到。
- 測試：`test.py wpt css/selectors/ css/css-scoping/ shadow-dom/ css/css-cascade/`、`test.py reftest shadow/ invalidation/ svg/` 沒有非預期的結果。

**第 3 段之後，`forget_moved`**（第 3 段量到 TodoMVC-jQuery 的 style 變多才查到，見下面「效能的量測」）：
- debug build 的確認抓到 Text 節點：`dom` 移除任何節點都記進 `removed`，走訪卻只會遇到元素；改成只記元素。
- WPT 的 `css/css-transitions/transition-events-with-document-change.html` 因此通過：以前一份 document 重算時會把已經移到別的 document 的元素也拿掉。
- mutation test（`mutants-3b.toml`）3 條都抓到（第一輪沒抓到的那條是多餘的檢查，拿掉了）。

**第 4a 段**：
- 主 frame 的 `media` 一開始照還是 0×0 的 viewport 判斷，改成和 `matchMedia` 一樣用視窗的大小。
- mutation test（`mutants-4a.toml`）第一輪漏了 2 條（測試裡 inline 的 script 排在 external 的後面），改了測試後 10 條都抓到。
- WPT（`css/css-tables/ html/semantics/scripting-1/ html/semantics/document-metadata/ html/interaction/focus/ css/cssom-view/ css/CSS2/ html/webappapis/`）：232 個子測試從失敗變成通過，42 個 `.ini` 拿掉。變成失敗的：`moving-between-documents/` 的 move-back 14 個檔案（測試假設 script 在串流的途中才 prepare，kun 一次 parse 整個回應；以前是碰巧通過），改成固定的 FAIL、寫明原因；css-tables 的 `bounding-box-computation-3` 一個子測試（stylesheet 套用之後露出空的 row group 的寬度）。

**第 4b 段**：
- K1a 的第一版對每個沒排版的子 frame 都只排版父 frame，WPT 的 `picture-resize-complete.html` 逾時（改了 iframe 的寬度後讀 `matches`，下一次 rendering update 就不發 `change`），改成只在等 stylesheet 時。
- 量過之後發現多數 `matchMedia` 早在 `<link>` 開始載入之前，加上「已經接上、還沒開始載入的 `<link>`」；整份重算接著移到 `start_loads` 用 0×0 的大小算子 frame，所以做 L1。
- mutation test：`mutants-4b1.toml` 第一輪漏了「每個 frame 都只排版父 frame」和「別的 document 的 `<link>` 也算」，補測試後 8 條都抓到；`mutants-4b2.toml` 5 條都抓到（只多花時間的，由單元測試直接問 `styles_before_layout`）。
- 測試：`test.py wpt css/mediaqueries/ css/cssom-view/ html/semantics/embedded-content/ css/css-transitions/` 沒有非預期的結果。

**第 4c 到 4e 段**：
- 4c 寫測試時發現 `@media print { … }` 的規則也被當成變動的規則，改成只算條件成立的（和 `RuleSet` 一樣）。
- 4e 開始前暫時加 log：Svelte-Complex-DOM 的子樹重算是 `<ul class="todo-list …">` 的 class 改變，`html` 的 `data-visible-section` 改了也重算整份 document。
- 4e 的交叉檢查一開始讓只為了檢查才走到的元素照常套用 invalidation set，「host 的 set 沒有讓走訪進 shadow tree」因此只有 release build 會漏、debug build 抓不到；改成只為了檢查才走到的元素不套用任何 set。
- mutation test：`mutants-4c.toml` 第一輪漏了 `:has()` 的檢查和 UA shadow tree 的部件，補測試後 20 條都抓到。`mutants-4d.toml` 14 條抓到 13 條：「slot 分配到 slot 時只重算 slot」補了 `display: block` 的 slot 之後抓到；「沒有為了規則走進 shadow tree」是等價的（`dom` 標記 shadow root 的 sheet 時也標記了 host 的祖先），留著給別的原因重收的 scope。`mutants-4e.toml` 13 條抓到 11 條，2 條等價（`any` 的 set 照樣帶下去、每個元素重算；只在 `descend` 漏算時才有差的 filter）；4e 改了走訪，4c、4d 和走訪有關的 7 條重跑，結果不變。
- 測試：`test.py wpt css/css-cascade/ css/selectors/ css/cssom/ css/css-scoping/ css/css-transitions/ shadow-dom/` 沒有非預期的結果；`css/css-scoping/` 和 `css/selectors/invalidation/` 另外用 `--all` 跑全部 144 個，沒有交叉檢查的 panic，和 4d 的結果逐一比對完全相同。

**第 5a 段**：
- 第一版把 pseudo-element 的調整留在 `cascade` 裡，取樣時 WebComponents 出現 `Arc::make_mut`（共用的 pseudo-element style 被改寫時整份複製），改成搬進 `adjust`、`Position` 帶 pseudo-element 的種類。
- 跑 WPT 的 `css/css-pseudo/` 時 `marker-content-014.html` 失敗：切到第 4 階段的各個 commit 單獨跑，4a 通過、4b 起失敗。pseudo-element 用到 viewport 單位時 viewport 改了不會重算它，以前碰巧有別的整份重算蓋過去，L1 之後露出來；改成元素或任何一個 pseudo-element 用到就重算，`::marker` 也算進去。
- mutation test（`mutants-5a.toml`）13 條抓到 11 條，2 條等價：「拿到別的位置的 entry」（hash 已經含位置，只有碰撞時有作用）、「menu list 的內層元素進快取」（UA 的規則用了 `text-overflow: inherit`，快取本來就不存；排除留著當保險）。「marker 的 viewport 單位沒算進去」第一輪沒抓到（測試裡 `::before` 也用 `vw`），補了只有 `::marker` 用的情形。
- 測試：`test.py reftest`（168 個）、`test.py wpt css/css-cascade/ css/css-display/ css/css-transitions/ css/css-animations/ css/css-pseudo/ css/css-lists/` 沒有非預期的結果。

**第 5b 段**：
- 讀寫欄位的地方照編譯錯誤用暫時的腳本改；`css` 的 `interpolate.rs` 用欄位路徑當參數的巨集手動改成分開讀和寫。outline 原本要放進 rare 組，但 `cascade_start` 每次都把 `outline_width` 設成 `medium`，每個元素都會複製那一組，留在主體。
- mutation test（`mutants-5b.toml`）5 條都抓到。
- 測試：`test.py reftest`（168 個）、`test.py wpt css/css-grid/ css/css-animations/ css/css-transitions/ css/css-backgrounds/ css/css-lists/ css/css-pseudo/ css/css-content/ svg/` 沒有非預期的結果。

**第 6a 段**：
- V1 量的時候暫時加計數：Svelte 一個 iteration 的 `attr_matches` 約 39 萬次，幾乎都是 `dir`，比到值的約 1 萬 3 千次。
- mutation test（`mutants-6a.toml`）第一輪漏了「移回來的 host 沒帶進 shadow tree」，補了測試後 4 條都抓到。
- 測試：`test.py wpt css/css-transitions/ css/css-animations/ css/selectors/ shadow-dom/` 沒有非預期的結果。

**階段的檢查**：每個階段做完時 `cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`。clippy 抓到的都在那個階段的最後一個 commit 修好（型別太複雜改成具名的型別、`SheetsChange::Rules` 太大改放 `Rc`、多餘的 `&` 和 `mut`）。第 2、3 段動到 `dom` 的節點狀態（`presentation_style`、`fallback_base_url`、`id_hash`、`class_hashes`），GC 的 `matrix`、`fuzz` 照規則由 merge 之後的 CI 跑。

**完成時（2026-10-19，macOS 26.6.2，Apple M4，rustc 1.99.0）**：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings` 乾淨；`cargo test --workspace` 通過（3295 個測試）；`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs` 通過。

## 效能的量測

整份的結果（每個 iteration 的 style，ms；`ci.py speedometer --trace --iterations 3`，同一台機器，接著電源）：

| 量的時候 | style | Chrome |
| --- | ---: | ---: |
| 第 1 段（RFC 開始前的程式） | 151 | 34 |
| 第 3 段之後 | 128.7 | 34.3 |
| 第 4 段之後 | 109.7 | 34.1 |
| 5a 之後 | 98.2 | 34.4 |
| 5b 之後 | 94.5 | 34.4 |
| 6a 之後 | 92.1（−39%） | 34.5 |

建 box tree、layout、script 在這份 RFC 裡沒有變（量測的變動內）。各次的細節：

**第 3 段之後（2026-10-19，macOS，Apple M4，接著電源）**，使用者要求量的。`ci.py speedometer --trace --iterations 3`（第 3 段的 commit，`perf` profile；Chrome for Testing 152.0.7977.82），各階段的報告（`phases.md`），和 `details.md` 第 1 段量的（同樣的對應、同樣的跑法）比較：

| 階段（每個 iteration 的 ms） | 第 1 段 | 第 3 段之後 | Chrome |
| --- | ---: | ---: | ---: |
| style | 151 | 128.7（−15%） | 34.3 |
| 建 box tree | 92 | 92.6 | 6.0 |
| layout | 92 | 92.3 | 115.4 |
| script | 604 | 610.8 | 307.8 |

各 suite 的 style：NewsSite-Next 21.2 → 16.6、NewsSite-Nuxt 17.5 → 13.6、React-Stockcharts-SVG 15.1 → 11.5、TodoMVC-WebComponents 9.4 → 7.5，Complex-DOM 的 TodoMVC 各少 1.5 到 2.5 ms（ES6-Webpack 7.3 → 4.8、Svelte 5.9 → 3.9、Angular 6.8 → 4.8、Preact 6.1 → 4.6）。**TodoMVC-jQuery 反而多了**（11.4 → 15.1，單獨再錄兩次是 15.0、13.6）。

查 jQuery：整份重算從整次執行 10 次降到 4 次（D5a），重算的元素 6705 → 6406，不是算得更多。`--sample --against 3228c57b`（RFC 開始前，各 3 次）：整個 suite 的 CPU 時間 676 → 703 ms（+3.9%），`recalc_style` 底下 35.6 → 39.4 ms（3 個 iteration）。用 `atos -i` 拆 `recalc_style` 自己的時間，兩個版本都是七成以上花在 recalc 結尾的 `Animator::forget_moved`：它每次 recalc 走過整個 `Animations::removed`（頁面有過動畫之後，每個被移除的子樹的根都記在這裡，GC 回收之後才由 renderer 拿掉），每個查一次 `node_by_id`。jQuery 一直新增、刪除 todo，這個 set 很大；兩個版本的差別大概是 GC 回收的時機不同。這是原本就有的熱點，不是這幾段造成的。

**`forget_moved` 改了之後（2026-10-19，同一台機器）**：`ci.py speedometer --trace --iterations 3 --suites TodoMVC-jQuery` 兩次，style 都是 4.7 ms（改之前 15.1、15.0、13.6，第 1 段 11.4；Chrome 1.2 到 1.3）。其他 suite 沒有重量。

**第 4 段之後（2026-10-19，同一台機器，接著電源）**：`ci.py speedometer --trace --iterations 3`（4d 的 commit 加上 4e 還沒 commit 的改動，`perf` profile；Chrome 同上）。

| 階段（每個 iteration 的 ms） | 第 1 段 | 第 3 段之後 | 第 4 段之後 | Chrome |
| --- | ---: | ---: | ---: | ---: |
| style | 151 | 128.7 | 109.7（−15%） | 34.1 |
| 建 box tree | 92 | 92.6 | 90.1 | 6.0 |
| layout | 92 | 92.3 | 90.0 | 115.5 |
| script | 604 | 610.8 | 603.4 | 304.9 |

各 suite 的 style（第 3 段之後 → 第 4 段之後）：TodoMVC-jQuery 15.1 → 4.8（`forget_moved` 的修正，第 3 段之後那一筆），Complex-DOM 的 TodoMVC 各少 1 到 1.5 ms（ES6-Webpack 4.8 → 3.4、Angular 4.8 → 3.6、Preact 4.6 → 3.3、Svelte 3.9 → 3.4），NewsSite-Next 16.6 → 16.5、NewsSite-Nuxt 13.6 → 13.4、React-Stockcharts-SVG 11.5 → 11.6、TodoMVC-WebComponents 7.5 → 7.6。下降的 19 ms 裡，約 10 ms 是 jQuery 的 `forget_moved`，其餘是第 4 段少掉的重算；NewsSite、Stockcharts 這類重算數本來就和 Chrome 差不多的頁面，差距在每個元素的成本（第 5 段）。

各 suite 重算的元素（3 個 iteration，`style=debug` 的 log；`details.md` 的「4b 前剩下的整份重算」有逐段的）：4a 之後 → 第 4 段之後，ES6-Webpack-Complex-DOM 44,757 → 24,154、React-Complex-DOM 44,670 → 24,046、Angular-Complex-DOM 44,937 → 24,313、Preact-Complex-DOM 51,254 → 23,989、Svelte-Complex-DOM 64,549 → 43,924（剩下的子樹重算是 `li` 的 style 變了往下繼承）、Vue 7,237 → 5,418、Editor-TipTap 3,942 → 2,903、ES5 4,586 → 3,652、React-Redux 4,361 → 3,502，其他少幾百個。

**第 5a 段之後（2026-10-19，同一台機器，接著電源）**：`ci.py speedometer --trace --iterations 3`（`cf0a8d6c` 加上 5a 還沒 commit 的改動）。style 109.7 → **98.2 ms**（−10.5%，Chrome 34.4）；建 box tree 92.0、layout 91.7、script 618.0（這幾項不受 5a 影響，差別在量測的變動內）。各 suite 的 style（第 4 段之後 → 5a 之後）：NewsSite-Next 16.5 → 14.7、NewsSite-Nuxt 13.4 → 12.0、Backbone 6.5 → 5.4、Vue 4.2 → 3.5、jQuery 4.8 → 4.1、WebComponents 7.6 → 7.0、Lit 7.7 → 7.2、Complex-DOM 的 TodoMVC 各少 0.6 到 0.7 ms（ES6-Webpack 3.4 → 2.8、Svelte 3.4 → 2.7、Preact 3.3 → 2.7、Angular 3.6 → 3.0），React-Stockcharts-SVG 11.6 → 11.7、Charts-observable-plot 8.5 → 8.2（SVG 為主，命中率低）。

**第 5b 段之後（2026-10-19，同一台機器，接著電源）**：`ci.py speedometer --trace --iterations 3`（`ef035a3e` 加上 5b 還沒 commit 的改動）。style 98.2 → **94.5 ms**（−3.8%，Chrome 34.4）。各 suite 的 style（5a 之後 → 5b 之後）：NewsSite-Next 14.7 → 13.6、NewsSite-Nuxt 12.0 → 11.4、React-Stockcharts-SVG 11.7 → 11.1、Charts-observable-plot 8.2 → 7.7、Lit 7.2 → 7.0、WebComponents 7.0 → 6.8，其他差 0.1 到 0.2 ms。

**第 6a 段之後（2026-10-19，同一台機器，接著電源）**：`ci.py speedometer --trace --iterations 3`（`7d06f9b4` 加上還沒 commit 的改動）。style 94.5 → **92.1 ms**（Chrome 34.5）：NewsSite-Next 13.6 → 13.2、NewsSite-Nuxt 11.4 → 11.0、React-Stockcharts-SVG 11.1 → 10.8、Charts-observable-plot 7.7 → 7.2。從第 1 段的 151 ms 算起少了 39%。
