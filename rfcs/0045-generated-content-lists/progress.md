# RFC 0045 的進度：生成內容和 list

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-14。完成：2026-10-14，Windows 11（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0045` 上做，兩段做完一起 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `::before`、`::after`、`content`；table 系列的 `display` 暫時當 block | 完成 | 2026-10-14 |
| 2a list 和 counter 的 style：`display: list-item`、`::marker`、`list-style-*`、counter 的 property、`counter()`、HTML 的 hint | 完成 | 2026-10-14 |
| 2b layout：counter 的計算、marker 的盒子、生成文字裡的 counter | 完成 | 2026-10-14 |

## 驗收

### `::before`、`::after` 和 `content`

| 項目 | 怎麼驗證 |
| --- | --- |
| 選擇器：`::before`、`::after` 和單冒號的寫法；規則照它的元素的 compound 進桶；`querySelector` 不配對 pseudo-element | `cargo test -p style --test it matching::pseudo_element_rules_are_in_their_elements_buckets`；WPT 的 `dom/nodes/ParentNode-querySelector-All*`、`css/css-conditional/js/CSS-supports-L4.html` |
| `content` 的解析、序列化（字串、`attr()`、`counter()`、`counters()`、`normal`、`none`，其他無效）；table 系列的 `display` 的解析和 computed value（`inline-table` blockify 成 `table`） | `cargo test -p style --test it cssom::`、`computed::table_displays`；WPT 的 `css/css-content/parsing/` |
| style：pseudo-element 的規則和 specificity 照它自己的、從元素繼承、`content: normal` 是 `none`、`attr()` 的值填進去（HTML 的名稱不分大小寫）；規則和屬性改了，生成的內容跟著變；pseudo-element 的 style 沒變時 layout 什麼都不做 | `cargo test -p layout --test it update::pseudo_elements_follow_their_rules_and_attributes`、`generated::an_inline_pseudo_element_has_an_inline_box` |
| layout：行內的在元素的文字前後、有自己的 inline box；block-level 的、`inline-block`、float、絕對定位、flex item 的有自己的盒子；沒有 `content` 的不產生；clearfix（`block`、`table`）包住 float；表單控制項不產生；table 的 column 不產生盒子；其他 table 值是 block、`inline-table` 是 atomic inline | `cargo test -p layout --test it generated::`；`python3 scripts/test.py reftest generated/`（g1） |
| 識別：元素的幾何（client rects、`element_box`、`is_fixed`、行內 box 的 metrics）、paint 的 bounds 和 paint offset 都只算元素自己的盒子；生成的文字照元素的畫，hit test 回答元素 | `cargo test -p layout --test it generated::geometry_leaves_pseudo_elements_out`、`generated::an_inline_elements_box_*`；`cargo test -p paint --test it artifact::generated_text_is_painted_as_its_elements`、`hit_test::a_pseudo_element_is_hit_as_its_element`、`hit_test::an_elements_bounds_are_its_own_boxs` |
| `getComputedStyle(element, pseudo)`：`::before`、`::after`、`::marker` 存著的 style，沒有存著時是從元素繼承、blockify、`content: none`（marker 是 `normal`）的那一份；layout 相關的 property 是 computed value；kun 沒有的和 `:marker` 回空的；不以冒號開頭的是元素自己的；以冒號開頭卻解析不了的是 `SyntaxError` | `cargo test -p dom_api --test it computed_style::pseudo_element_styles`；`cargo test -p dom_js --test it computed_style::the_pseudo_element_argument`；WPT 的 `css/cssom/getComputedStyle-pseudo*.html` |
| 畫面和 Chrome 一樣 | `python3 scripts/test.py wpt css/CSS2/generated-content/ css/css-content/ css/css-pseudo/ css/cssom/getComputedStyle-pseudo.html css/cssom/getComputedStyle-pseudo-with-argument.html`（預期結果在 `tests/wpt/metadata/`，每個失敗的 `.ini` 寫了原因） |

### list 和 counter

| 項目 | 怎麼驗證 |
| --- | --- |
| 解析、序列化、computed value：`display: list-item`、`list-style-type`（預先定義的、其他名稱照寫、字串）、`list-style-position`、`list-style-image: none`、`list-style`（只寫不是初始值的）、`counter-reset`（`reversed()`）、`counter-increment`、`counter-set`（寫出預設值）、`counter()`、`counters()`（`decimal` 不寫、`none` 無效） | `cargo test -p style --test it cssom::`、`computed::shorthands_come_from_their_computed_longhands`；WPT 的 `css/css-lists/parsing/`、`css/css-content/parsing/` |
| UA stylesheet 和 HTML 的 hint：`<li>` 是 list item、清單 reset `list-item`、巢狀的符號、`<ol start>`、`reversed`、`<li value>`、`type` | `cargo test -p style --test it computed::lists_and_their_hints`；WPT 的 `html/rendering/non-replaced-elements/lists/` |
| `::marker`：只有 list item 有，只套用它能用的 property，`display` 照位置、沒有 `text-indent` | `cargo test -p style --test it matching::a_list_items_marker_has_a_style`；WPT 的 `css/css-pseudo/marker-*`、`parsing/marker-*` |
| counter：繼承（照 Chrome 的 css-lists 3）、reset / increment / set 的順序、pseudo-element 自己的 counter、`::after` 看得到子元素的、`counters()`、reversed 的起始值（連同 `<li value>`）、溢位、`display: none` 不算；counter style 的字串 | `cargo test -p layout --test it lists::list_items_are_numbered`、`lists::counters_in_content`；`cargo test -p layout --lib counters`；WPT 的 `css/css-lists/`、`css/CSS2/generated-content/` |
| marker 的排版：outside 在第一行的開頭之前（巢狀的 block、float 旁邊、`text-align`）、baseline 對齊、沒有行時在上面且 list item 至少和它一樣高；inside 是第一個行內盒子；`none` 和 `::marker { content }` | `cargo test -p layout --test it lists::`；`python3 scripts/test.py reftest generated/`（g2） |
| 局部更新：插入、移除 list item、改 `value`、`start`、`list-style-*` 之後 marker 跟著變，只重新收集文字變了的；沒有 counter 的文件加入第一個清單；移走一個盒子裡最後的 counter | `cargo test -p layout --test it update::markers_follow_their_list`、`update::a_first_list_has_markers`、`update::counters_follow_an_element_taken_away`（每一步都和重建的樹比對） |
| paint 和 hit test：marker 照它的顏色畫在第一行的最前面，點到它打到 list item | `cargo test -p paint --test it artifact::markers_are_painted_as_their_list_items`、`hit_test::a_marker_is_hit_as_its_list_item` |
| 畫面和 Chrome 一樣 | `python3 scripts/test.py wpt css/css-lists/ css/CSS2/lists/ html/rendering/non-replaced-elements/lists/ css/css-pseudo/`（預期結果在 `tests/wpt/metadata/`，每個失敗的 `.ini` 寫了原因） |

## 做過的檢查

- **做了什麼**（第 1 到 12 條）：
  - `computed_style`：`PseudoId`（`Before`、`After`、`Marker`）、`PseudoStyles`、`Content` / `ContentItem`（`attr()` 帶著填進去的值、`counter()`、`counters()`）、`ListStyleType`、`ListStylePosition`、`CounterReset`、`CounterChange`；`ComputedStyle` 的 `content`、三個 counter property 和繼承的 `list_style_type`、`list_style_position`；`Display` 加 `Table(TableDisplay)`、`InlineTable`、`ListItem` 和 `generates_no_box`。
  - `css`：`PseudoElement`；`content`、`list-style-*`、`list-style`、counter 的三個 property 的解析和序列化；`display` 的 table 值和 `list-item`；`RuleSet` 照 pseudo-element 前面的 compound 選桶；`LonghandId::applies_to_marker`。
  - `style`：同一趟配對元素和它的 pseudo-element（第二個 `MatchingContext`），每個 target 各自的 specificity；`cascade_blocks` 讓元素和 pseudo-element 用同一段 cascade；`::marker` 只替 list item 算、只套用它能用的 property、`display` 照位置、`text-indent` 是 0；`pseudo_style_without_rules`；UA stylesheet 的清單規則；`hints.rs` 的清單的 hint。
  - `dom`：`NodeState::Element` 的 `pseudo_styles`、`set_pseudo_styles`（回答有沒有變）、`LayoutTreeFlags::pseudo_styles_changed`；`CssComputedStyleDeclaration` 記 `StyleTarget`（soundness.md 和 README 跟著改）。
  - `layout`：`collect_pseudo`；`LayoutBox`、`BoxFragment`、`InlineBoxFragment`、`TextFragment` 的 `pseudo`；行內內容的 `text::Owner`；幾何跳過 pseudo-element 的盒子；`counters` 模組和 `BoxTree::counters`（建樹之前算、update 時視需要重算，文字變了的重新收集）；inside marker 進行內內容；outside marker 是 `LayoutBox::marker`（`set_marker`、`lay_out_box` 先排它、`marker_link` 照 `first_line` 放、登記成第一行的第一個 atomic inline）；沒有內容的 list item 和 marker 一樣高。
  - `paint`：bounds、paint offset、上一次 paint 的 `by_node` 跳過 pseudo-element 的盒子。
  - `dom_api`：`getComputedStyle` 的 pseudo-element 參數。
  - 文件：`docs/limitations.md`（pseudo-element、`getComputedStyle` 的參數、table 的 `display`、`display: list-item`、生成內容、「List 和 counter」、強制換行之後的元素結束邊、生成文字的 kerning）、`docs/architecture.md`（`style` 和 `layout` 的列）、`docs/test-gaps.md` 的「等價、不用處理的 mutation」；UA stylesheet 開頭的註解。
- **找到並修的**：
  - 第 1 段：layout 的測試照節點收集 fragment，pseudo-element 的蓋掉元素的；`getComputedStyle` 讀 `::before` 的 `width` 拿到元素的盒子；沒有規則的 pseudo-element 沒有 blockify；`display: none` 的元素原本不算 pseudo-element 的 style（Chrome 算）；`table-column` 的 `::before` 第一版當 block 畫（CSS 2 說不 render，`before-content-display-012` 等四個）；paint 的 `element_bounds` 把 pseudo-element 的行內 box 算進元素的範圍。
  - 第 2 段：counter 第一版照 CSS 2 的 scope，`counter-001`、`counters-scope-*` 失敗，改成 Chrome 的繼承（wpt.fyi 對照過，第 10 條）；reversed 的起始值第一版從 reference 推的式子，碰到多個 `value`、0 的 increment 不對，改照規範的演算法；increment 溢位時第一版飽和到 `i32` 的上限，WPT 要值不變；`counter(foo, none)` 第一版收下；outside marker 第一版是一般的 child，有自己的行的盒子只透過行走 children，marker 沒畫、打不到，又放在 content box 的開頭（旁邊有 float 時和 Chrome 不同）；`::marker` 繼承了 list item 的 `text-indent`、`display` 是 `inline`；`getComputedStyle(e, ':marker')` 讀成 `::marker`，沒有冒號的參數丟 `SyntaxError`（CSSOM 和 Chrome 都照元素自己的）。
  - mutation test 找到的多餘的程式：`walk.counters`（counter 相關元素的 style 變了就重算）：counter 的 property 不在只影響繪製的差別裡，style 變了一定會重新收集內容、觸發重算，拿掉。
  - 舊的測試跟著改：`style` 的 `cascade::what_is_ignored` 用 `display: table`、`cssom::invalid_values_are_not_set` 用 `counter(x)` 當不支援的值；`dom_js` 用 `::marker` 當 kun 沒有的 pseudo-element；`dom_wasm` 的 guest 讀沒有規則的 `::before` 的 `color` 預期空字串（改成繼承的紅色，guest 的註解跟著改，重編之後 `.wasm` 沒變）。
- **測試**：`style` 的 `cssom::`、`computed::table_displays`、`computed::lists_and_their_hints`、`computed::shorthands_come_from_their_computed_longhands`、`matching::pseudo_element_rules_are_in_their_elements_buckets`、`matching::a_list_items_marker_has_a_style`；`layout` 的 `generated::`（13 個）、`lists::`（7 個）、`counters::tests::counter_styles`、`update::` 的四個（pseudo-element、marker、第一個清單、移走 counter）；`paint` 的 `artifact::` 和 `hit_test::` 各兩個、`hit_test::an_elements_bounds_are_its_own_boxs`；`dom_api` 的 `computed_style::pseudo_element_styles`；`dom_js` 的 `computed_style::the_pseudo_element_argument`；reftest `generated/` 的 g1、g2。
- **mutation test**（每段一份，結果都是最後一次的）：
  - `mutants-1.toml` 36 條，35 條抓到。第一次 7 條沒抓到：補了 table 的 block 盒子、pseudo-element 的 style 沒變時不重收內容、行內元素的 box 用自己文字的 metrics、行內元素的 `element_box` 和 paint offset 不是 pseudo-element 的 atomic inline 的測試；「上一次 paint 照元素找盒子時不排除 pseudo-element 的」等價（style 一定不同，只少搬一次），記在 `docs/testing.md`。
  - `mutants-2.toml` 46 條，全部抓到；第一次沒抓到的 `walk.counters` 拿掉（上面），補了「沒有 counter 的文件加入第一個清單」的測試和它的 mutant。
  - `cargo fmt` 之後兩份各有三條的原始碼對不上，更新後重跑，都抓到。
- **WPT**（`include.txt` 加上整個 `css/CSS2/generated-content/`、`css/css-content/`、`css/css-lists/`、`css/CSS2/lists/`、`html/rendering/non-replaced-elements/lists/`，`css/css-pseudo/` 的 `::before`、`::after` 的五個和 `::marker` 的 74 個，`css/cssom/getComputedStyle-pseudo*.html` 兩個；預期結果用 `MACHOPO_UPDATE_EXPECTATIONS` 寫下，每個 `.ini` 寫了原因）：
  - 整個通過的：`generated-content/` 226 個裡 181 個、`css-content/` 78 個裡 15 個、`css-lists/` 224 個裡 162 個、`CSS2/lists/` 156 個裡 139 個、HTML 的 lists 32 個裡 30 個、`css-pseudo` 的 marker 74 個裡 40 個、`::before`、`::after` 的五個裡 1 個。
  - 失敗的原因：quote、`url()` 和 marker 的圖片、`@counter-style` 和 `symbols()`、其他預先定義的 counter style、disc 畫成字（Chrome 畫圖形；marker 的文字 property 的那幾個只有 disc 那一列不同）、table 的版面、bidi 和直排、`::first-letter` / `::first-line`、animation、shadow DOM 的 slot、`display: contents`、`inline list-item`、`ch`、`calc()` 的 counter 值、用 `content` 取代元素的內容、有型別的 `attr()`、跨 run 的 kerning（值都對）、照舊 CSS 2 scope 的兩個（Chrome 也失敗）、`::before::marker`、`::marker` 的 counter property、quirks mode 的 inside、沒有行的 list item 旁邊有 float、XHTML（`content-attr-case-002`、`content-177`）、tab（`content-white-space-002`）、`<body>` 的 `bgcolor`、`text`、強制換行之後的元素結束邊（`content-175`，`<br>` 也一樣）、window 的 named access、`document.fonts` 和 `document.styleSheets`、`Element.pseudo()`；`before-as-flex-container` 本身不對（Chrome、Edge、Firefox 也失敗）。
  - 停用的四個：`firefox-bug-1715631`（marker 繼承了 4e9 倍的 `line-height`，list item 和它一樣高，compositor 替整個 layer 建 tile 卡住）、`list-marker-position-crash`（`padding: 0 2147483652%`，換成 `display: block` 也卡住）、`list-item-counter-crash`（等 kun 不發的 `srcset` 的 `error`）、`list-marker-counter-style-mutation`（`@counter-style` 的 CSSOM）。
  - 清單裡原本就有、用到這些功能的（第 1 段：原始碼或 reference 有 `::before`、`::after` 的 17 個，和整個 `css/css-variables/`、`css/cssom-view/`、`dom/nodes/ParentNode-querySelector-All*`、`dom/events/click-on-absolute-pseudo.html`、`CSS-supports-L4.html`；第 2 段：有 list、list item、counter 的 278 個，和整個 `css/css-content/`、`css/css-variables/`、`css/cssom/`）：變通過的有 `floats-114`、`overflow-wrap-anywhere-001`、`overflow-wrap-break-word-001`、`css-variables` 的四個、`querySelector` 的 `::before`、`::after` 的 64 個 subtest、`CSS.supports` 的 `selector(::before)`、`css-content/parsing` 和 `cssom/serialize-values` 等的 counter 和 `list-style` 的 subtest、`display-valid` 的 `list-item` 和 table 值、`elementFromPoint-list-001`、`intersection-ratio-with-fractional-bounds-2`、`getComputedStyle-pseudo` 的兩項，拿掉預期結果。沒有變失敗的。
- **跑過的**：改到的 crate 的測試；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py reftest`（全部 161 個，0 個預期外）；上面的 WPT；兩段各自做完時的 `cargo test --workspace --no-fail-fast`（最後一次 2026-10-14，Windows 11，43 個 binary 全部通過）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`。
- **沒跑的**：`test.py automation`、`scenarios`（沒有動到視窗那一層，也沒有情境）；`ci.py speedometer --inventory`（`ci.py speedometer` 只在使用者要求時跑）。`cargo clean` 在 merge 回 `main` 之後跑過。
