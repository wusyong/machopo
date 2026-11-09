# RFC 0046 的進度：shadow tree 的 rendering

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-14。完成：2026-10-15，Fedora Linux（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0046` 上做，第 1 段做完時先 commit（由人 commit）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a `dom` 和 style：flat tree 的走法、跨 shadow root 的 flag、slot 的分配和 tree scope 的 stylesheet 的標記；每個 tree scope 的 stylesheet；照 flat tree 繼承；`display: contents` 的 style | 完成 | 2026-10-14 |
| 1b layout：box tree 照 flat tree、`display: contents`、局部更新、counter | 完成 | 2026-10-14 |
| 1c 幾何、hit test、`elementFromPoint` 的 retarget；WPT；`about:demo` 和情境；renderer thread 的 stack | 完成 | 2026-10-14 |
| 2 `:host`、`:host()`、`::slotted()` 和跨樹的 cascade | 完成 | 2026-10-15 |

## 驗收

### `dom`

| 項目 | 怎麼驗證 |
| --- | --- |
| flat tree：host 的子節點是 shadow root 的，slot 的是分配到的（沒有時是 fallback），沒分配到的不在裡面；slot 裡的 slot；flat tree 的 parent | `cargo test -p dom --test it flat_tree::flat_tree_children_and_parents` |
| style 和 layout 的 flag 從 shadow root 接著往 host 標（從裡面的節點、從 shadow root 本身開始的都是；style 階段的 `set_style_changed` 也是）；`attachShadow` 重建 host 的盒子；進出 slot 的節點要重算、重建，它們的 slot 重新收集內容（`slot` attribute、slot 的 `name`、`assign()`） | `cargo test -p dom --test it flat_tree::` |
| tree scope 的 `style_sheets_changed`：`<style>`、`<link>` 插入、移除（連同在子樹裡的，不進 shadow tree）、文字和 attribute 改了；shadow root 搬到別的 document；constructed sheet 改了只標還 adopt 它的，不撐住它們 | `cargo test -p dom --test it flat_tree::style_sheet_owners_mark_their_tree_scope`、`flat_tree::a_shadow_root_in_another_document_collects_its_sheets_again`、`style_sheets::a_sheet_marks_its_adopters_and_doesnt_keep_them`；`cargo test -p dom_api --test it css_style_sheet::changes_to_sheets_mark_their_adopters` |

### style

| 項目 | 怎麼驗證 |
| --- | --- |
| shadow tree 的 `<style>`、`adoptedStyleSheets` 只配對那棵樹，document 的不進去；照 flat tree 繼承（slot、slot 裡的 slot、沒分配到的從 host）；同樣的文字只 parse 一次、沒人用的丟掉；`display: contents`（root 是 `block`，子節點和 pseudo-element 照最近的不是 contents 的祖先 blockify、拿 `z-index`，slot 的子節點也是） | `cargo test -p style --test it shadow::` |
| `:host`、`:host()` 只配對 host（`:host p` 配對 shadow tree 裡的元素，host 是 featureless 的：`.a :host` 不配對）；`::slotted()` 只配對分配到的元素（slot 裡的 slot 一路往上，不配對子孫、fallback、slot 本身）；`:host::before`、`::slotted()::before`；`querySelector` 裡什麼都不配對 | `cargo test -p style --test it shadow::host_rules_match_the_host`、`shadow::slotted_rules_match_what_is_assigned`、`shadow::a_hosts_pseudo_elements`、`shadow::the_host_is_featureless_and_slotted_takes_before`、`shadow::query_selector_has_no_host` |
| 跨樹的 cascade：normal 的外面的勝、`!important` 的裡面的勝，不看 specificity；style attribute 是元素自己的樹的；同時是 host 又分配到 slot 的元素，`::slotted()` 在 `:host` 之前 | `cargo test -p style --test it shadow::the_outer_trees_normal_and_the_inner_trees_important_win`、`shadow::slotted_comes_before_host`；WPT 的 `css/css-shadow/shadow-cascade-order-001.html`（64 項） |
| 局部更新：shadow tree 裡的改動只重算它改到的；shadow tree 的 `<style>` 改了只重收那個 tree scope、整棵和 host 重算（連同分配到 slot 的）；`<style>` 進來、`media` 改了；進出 slot；adopt 的 sheet 改了、放掉之後不再重收；沒標的 tree scope 不重收；`<link>` 的 sheet 到了；shadow tree 裡看結構的選擇器；每次全部重收和只收有標的結果一樣；host 的 attribute、`:host()` 和 `::slotted()` 用的狀態只讓它們的元素重算（每一步都和新的 engine 的結果比對） | `cargo test -p style --test it invalidation::` |

### layout、paint 和幾何

| 項目 | 怎麼驗證 |
| --- | --- |
| host 的盒子裝它的 shadow tree，slot 換成分配到的節點，沒分配到的沒有盒子；fallback；slot 裡的 slot；flex 的 host 的子節點是 flex item | `cargo test -p layout --test it shadow::` |
| `display: contents`：自己沒有盒子和行內盒子，子節點（block、行內）和 `::before`、`::after` 照 parent 的內容排，它的文字用它的 style 和字型（slot 的也是）；pseudo-element 的 `contents`；replaced element 和表單控制項的 `contents` 當 `none` | `cargo test -p layout --test it shadow::display_contents_lays_out_its_children_as_its_parents`、`shadow::text_in_a_contents_element_has_its_font` |
| counter 照 flat tree 的順序；`contents` 的元素不改 counter，它的 `::before` 和子節點接著外面數；被 `contents` 藏起來的不算 | `cargo test -p layout --test it shadow::counters_go_in_the_flat_trees_order` |
| 局部更新（每一步和重建的樹比對、flag 都清掉）：加上 shadow tree、shadow tree 裡插入、改、移除、`display: none`；進出 slot、fallback 出現、slot 改名；`display: contents` 開關；分配到 slot 的節點的文字、行內元素在 slot 所在的盒子裡更新；host 重建時 shadow tree 的 flag；第一份清單在 shadow tree 裡；host 重建時 shadow tree 裡的 marker | `cargo test -p layout --test it update::shadow_trees_and_slots_follow_the_dom`、`update::a_slotted_child_updates_in_its_slots_box`、`update::a_first_list_in_a_shadow_tree_has_markers`、`update::a_host_built_again_clears_its_shadow_trees_flags`、`update::a_marker_in_a_host_built_again` |
| hit test 打到 shadow tree 裡的元素；文字打到它的 run 的 owner（分配到 slot 的是 slot，`contents` 的元素自己的文字是它） | `cargo test -p paint --test it hit_test::shadow_trees_are_hit_along_the_flat_tree` |
| shadow tree 裡的元素有盒子、`offset*`、computed style；slot 沒有盒子；`elementFromPoint` / `elementsFromPoint` 照 DOM retarget（`document` 的是 host，`ShadowRoot` 的是它的樹裡的），文字回答它的 parent 元素；`offsetParent` 照 flat tree、不進 closed shadow tree；整個在 range 裡的 `display: contents` 元素的盒子是它的子節點的 | `cargo test -p shell --test it geometry::elements_in_shadow_trees_have_boxes` |
| `ShadowRoot.elementFromPoint`、`elementsFromPoint` 在 WebIDL 的表和 `kun:dom` 的 WIT 裡 | `cargo test -p dom_api --test it spec:: wit::` |
| 一千層的元素、一千層的 shadow tree 排得出來（renderer thread 的 stack 是 8 MiB） | `cargo test -p shell --test it geometry::a_thousand_levels_deep`；WPT 的 `shadow-dom/build-deep-detached-shadow-then-append-text.html` |

### 畫面和視窗

| 項目 | 怎麼驗證 |
| --- | --- |
| 畫面和 Chrome 一樣 | `python3 scripts/test.py reftest shadow/`；`python3 scripts/test.py wpt shadow-dom/ css/css-shadow/` 和 `css/css-display/display-contents-*`（預期結果在 `tests/wpt/metadata/`，每個失敗的 `.ini` 寫了原因） |
| 在視窗裡：custom element 的 shadow tree 畫出來（`:host` 的框、頁面的圓角蓋過它、`::slotted(b)` 的底線），標題在它的 slot 裡，換了 slot 之後移動 | `python3 scripts/test.py scenarios shadow-dom` |

## 做過的檢查

- **做了什麼**（第 1 到 13 條）：
  - `dom`：`DomReadScope::flat_tree_children`、`flat_tree_parent`、`find_slot`（`FlatTreeChildren`）；`mark_ancestors`、`mark_layout_ancestors`（和 `NodeRef` 的）經過 `parent_or_shadow_host`；`StyleFlags::style_sheets_changed` 和它的標記（插入、移除時走子樹找 `<style>`、`<link>`，不進 shadow tree；文字、attribute；adopt 的清單；adopt 到別的 document 的 shadow root）；`CssStyleSheet::adopters`（`WeakSet<Node>`）、`WeakSet::contains`；`signal_slot_changes` 標記進出 slot 的節點和 slot；`attach_shadow` 重建 host。
  - `css`、`computed_style`：`display: contents`（`Display::Contents`）；`:host`、`::slotted()` 的解析（`::slotted()` 後面的 pseudo-element）；`RuleSet` 的 `host_rules`、`slotted_rules`。
  - `style`：每個 tree scope 一個 `ScopeCascade`（UA 和它自己的 sheet、`dependencies`、`host_dependencies`、`slotted_dependencies`）；shadow root 的 `ShadowScope`（`<style>` 以文字共用 `StyleSheetContents`、adopted sheet，有標記或第一次碰到才重收）；document 的 sheet 也只在有標記、`<link>` 的版本變了、換了 document 或第一次時重收（`LinkedStyleSheets` 的 `version`）；recalc 照 flat tree 走（host 先收集 shadow tree 的 sheet、算自己，再走 shadow tree，再走子節點，分配到 slot 的從 slot 繼承、slot 重算時跟著重算）；`RuleSource`（樹、`host_rules`、`slotted_rules`，`current_host`、`featureless`）；`CascadePriority::tree`；`StyleElement` 的 shadow DOM 方法；`cascade_blocks` 的 `layout_parent`；UA 的 `slot { display: contents }`；測試用的 `set_collect_every_tree_scope_for_testing`、`collected_tree_scopes_for_testing`、`shadow_scopes_for_testing`。
  - `layout`：`BuildContext`、`Walk` 帶著 `DomReadScope`，`collect` 照 flat tree 收；`display: contents`（`contents_is_none`）；`walk` 跨過 shadow root、分配到 slot 的在 slot 所在的盒子裡（`container_of` 沿 flat tree）；`clear_layout_tree_flags` 進 shadow tree；`push_runs` 替 style 不是最裡面的開著的元素的 run 設 text property；counter 照 flat tree、經過 `contents` 的元素。
  - `paint`：`HitTestResult::text`。`renderer`：`element_at`（script 的）把打到的文字換成它的 parent 元素；`CrRendererMain` 的 stack 8 MiB（第 10 條）。
  - `dom_api`：`elementFromPoint`、`elementsFromPoint` 照 tree scope retarget，`ShadowRoot` 也有（`spec-differences.txt` 一行、WIT 兩個函式，`ci.py guests` 重編 guest）；`offsetParent` 沿 flat tree、跳過 closed-shadow-hidden 的；range 的盒子裡 `display: contents` 的元素算子節點的。
  - `about:demo` 的 `<kun-card>`，情境 `shadow-dom`（`demo-script` 的按鈕數跟著改）；kun 的 reftest `shadow/` 兩個。
  - 文件：`crates/dom/README.md`、`soundness.md`（I3 加上 `set_adopted_style_sheet` 的 `WeakSet` 的 chunk、adopt 的清單那一段的 `adopters`、`WeakSet::contains`）、`docs/architecture.md`（`CrRendererMain`、`style`、`layout` 的列）、`docs/limitations.md`（shadow DOM、stylesheet 的 CSSOM、`display` 的值、`display: contents` 不能 focus 和 SVG 的、Invalidation、`Range` 的幾何、`ResizeObserver` 的深度、wdspec）、`docs/test-gaps.md` 的「等價、不用處理的 mutation」、`tests/wpt/include.txt` 的註解；RFC 0012 的路線圖記下 K-2 完成。
- **找到並修的**：
  - 第 1 段：shadow root 搬到別的 document 時，`adopt` 把它的 flag 重設成 `NEW`，蓋掉剛標的 `style_sheets_changed`（`flat_tree::a_shadow_root_in_another_document_collects_its_sheets_again` 抓到）；同一個 engine 先後給兩份 document 時沒有標記就不重收（`invalidation::each_document_has_its_own_engine`），記下上次的 document。WPT 抓到的：`display: contents` 的元素（和 slot）裡的文字用了 container 的字型（`display-contents-line-height.html`）；counter 在 `display: contents` 的元素裡重新開始（`counter-order-display-contents.html`、`counter-reset-increment-set-display-contents.html`、`counter-slot-order.html`、`counter-slot-order-scoping.html`）；`elementFromPoint` 對分配到 slot 的文字回答 slot（`DocumentOrShadowRoot-prototype-elementFromPoint.html`）；`offsetParent` 停在 shadow root（`offsetParent-across-shadow-boundaries.html`）；range 的盒子沒有 `contents` 的元素的子節點（`range-bounding-client-rect-with-display-contents.html`）；999 層的 shadow tree 讓 renderer 的 stack 溢位（`build-deep-detached-shadow-then-append-text.html`；999 層的一般元素也一樣，以前 shadow tree 不排版才過）。
  - 第 2 段：tree order 原本照樹的深度，同時是 host 又分配到 slot 的元素 `::slotted()` 和 `:host` 分不出先後（`shadow-cascade-order-001.html` 的 A4 和 F6）；`.container :host` 配對到了 host（`css-scoping-shadow-host-rule.html`：host 對自己的 shadow tree 的 `:host` 規則要是 featureless 的）；`::slotted(*)::before` 解析不了（`valid_after_slotted`）。
- **舊的測試跟著改**：`style` 的 `cascade::what_is_ignored` 用 `display: contents` 當不支援的值，換成 `ruby`；`dom_api` 的 `css_style_sheet::changes_to_sheets_mark_the_documents_style` 照舊的規則（document 建的 sheet 改了就標 document），改成 `changes_to_sheets_mark_their_adopters`；`shell` 的 `style_sheets::adopted_sheets_style_the_document` 最後一步在 `<body>` 上 `attachShadow`（之後 `#t` 沒有盒子），改成在另一個元素上；`shell` 的 `geometry::elements_in_shadow_trees_have_no_boxes` 改寫成 `elements_in_shadow_trees_have_boxes`；`style` 的 `invalidation::a_shadow_trees_style_changed`、`a_shadow_roots_adopted_sheet_changed` 在第 2 段加上 host（shadow tree 的 sheet 變了 host 也重算）。
- **mutation test**（每段一份，結果都是最後一次的）：
  - `mutants-1a.toml` 49 條，全部抓到。第一次 47 條裡 4 條沒抓到：flag 往上走的三條只改了第一步，只有從 shadow root 開始標（adopt 的 sheet 改了、直接從 shadow root 移除子節點）才看得出來，補了 `flat_tree::marks_from_a_shadow_root_go_on_to_its_host`，另外加兩條改迴圈那一步的；「host 的子節點的 flat parent 是 host」用了 style 的測試指令，改成 `dom` 的。
  - `mutants-1b.toml` 16 條，15 條抓到，1 條等價（`container_of` 找正在重建的祖先時沿 DOM，理由在 `docs/testing.md`）。第一次 4 條沒抓到，補了不改分配的改動（文字、行內元素的字型大小）、改 layout style 的 host 的 shadow tree、直接在 shadow root 裡的第一份清單的測試。
  - `mutants-1c.toml` 15 條，14 條抓到，1 條等價（`InlineContent` 不追蹤開著的行內元素，只多設一次相同的 text property，記在 `docs/testing.md`）；「`contents` 的元素的 `::before` 沒算 counter」第一次沒抓到，補了測試。
  - `mutants-2.toml` 23 條，全部抓到。第一次 24 條裡「`::slotted()::before` 不是 slotted 規則」沒抓到：看 `SlotAssignment` combinator 的那一臂多餘（`::slotted()` 的 compound 本來就有 `Slotted`），拿掉程式和那一條。
- **WPT**（`include.txt` 加上整個 `css/css-shadow/`（`part/` 停用）和 `css/css-display/` 的 76 個 `display-contents-*`；預期結果用 `MACHOPO_UPDATE_EXPECTATIONS` 寫下，每個失敗的 `.ini` 寫了原因）：
  - 整個通過的：`css/css-shadow/` 160 個裡 32 個（還在失敗的多數用 window 的 named property（測試用 `host` 當全域變數），其他是 declarative shadow DOM、`:has-slotted`、`:host-context()`、`@font-face`、`@keyframes`、`<style>` 的 `sheet`）；`display-contents-*` 76 個裡 50 個（跨 run 的 kerning 讓字差一個像素的 13 個、`::first-letter` / `::first-line`、window 的 named property、CSS nesting、SVG 的元素、`contents` 的元素的 focus、`<fieldset>`、`document.fonts`）；`shadow-dom/` 184 個裡 118 個。
  - 清單裡原本就有、用到 shadow DOM、`<slot>` 或 `display: contents` 的 173 個另外跑過：變通過的有 `selection/shadow-dom/cross-shadow-boundary-*` 八個、`css/cssom/adoptedstylesheets-cascade-order.html`、`CSSStyleSheet-template-adoption.html`、`CSSStyleSheet-constructable-disabled-regular-sheet-insertion.html`、`CSSStyleSheet-constructable.html` 的兩項、`getComputedStyle-pseudo.html` 的兩項 `display: contents`、`css/cssom-view/scrollIntoView-shadow.html`、`range-bounding-client-rect-with-shadow-dom.html`、`intersection-observer/shadow-content.html`、`resize-observer/eventloop.html`、`html/canvas/element/2d.text-outside-of-the-flat-tree.html`、`css/css-backgrounds/background-color-body-propagation-007.html`、`setHTMLUnsafe` 的幾項、`dom/nodes/ParentNode-querySelector-All*` 的 `:host` 的 subtest、`css/css-lists/counter-list-item-slot-order.html` 和上面四個 counter 的；`shadow-dom/` 裡的 18 個（`imperative-slot-*`、`untriaged/` 的 styles、focus、active element 等）和停用的 `untriaged/events/event-dispatch/test-003.html`（focus shadow tree 裡的元素，拿掉停用）。沒有變失敗的（上面修好的以外）。
  - 和這份無關、順便更新的：`custom-elements/reactions/CSSStyleDeclaration.html` 的 `cssFloat` 兩項（RFC 0043 做 `float` 之後就通過，預期結果一直沒更新）。
- **不穩定的**：`css/css-pseudo/marker-hit-testing.html` 在和其他八個目錄（32 個 process）一起跑時逾時一次：它在 `load` 之後依序送十組 testdriver 的動作，機器滿載時超過 harness 的時限；單獨跑三次、`css/css-pseudo/` 整個跑兩次都照預期（12/20）。和這份的改動無關（它不用 shadow DOM、`display: contents`）。
- **跑過的**：改到的 crate 的測試；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py reftest`（全部 163 個，0 個預期外）；`test.py scenarios`（全部 51 個）；上面的 WPT 和 `css/css-lists/`、`css/CSS2/lists/`、`css/css-content/`、`css/CSS2/generated-content/`、`css/css-pseudo/`、`css/cssom/`、`css/cssom-view/`、`css/selectors/`、`css/css-conditional/`、`dom/nodes/`、`dom/ranges/`、`custom-elements/`；兩段各自做完時的 `cargo test --workspace --no-fail-fast`（最後一次 2026-10-15，Fedora Linux，43 個 binary 全部通過）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`。
- **沒跑的**：`test.py automation`（沒有動到視窗那一層）；`matrix`、`fuzz`（動到 GC：`CssStyleSheet` 的 `adopters`，merge 之後看 CI 的結果）；整套的 `wpt`、`wdspec`；`ci.py speedometer`（只在使用者要求時跑）。`cargo clean` 等 merge 回 `main` 之後跑。
