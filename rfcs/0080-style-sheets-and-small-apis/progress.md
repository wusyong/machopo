# RFC 0080 的進度：文件的 stylesheet 的 CSSOM，和幾個小的 API

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-23，Fedora 44。在 `0080` 上做。

完成日期：2026-10-24，Fedora 44（AMD Ryzen AI 9 HX 370）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 小的 API：`toJSON`、`layerX`/`layerY`、`focus()` 的捲動（第 2、3 條） | 完成 | 2026-10-23 |
| 2 CSS 數學函式（第 4 條） | 完成 | 2026-10-23 |
| 3 文件的 stylesheet 由 `dom` 持有（第 5 到 7 條） | 完成 | 2026-10-24 |
| 4 CSSOM：`StyleSheetList`、`sheet`、改動影響 cascade（第 8 到 12 條） | 完成 | 2026-10-24 |

## 驗收

**小的 API**（預期值是 Chrome 152 的，`details.md`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `DOMRectReadOnly.toJSON()`（`DOMRect` 繼承）：一般的物件，屬性照 interface 的順序；只有 JS 有 | `cargo test -p dom_js --test it -- geometry::rects_to_json`；WPT 的 `css/geometry/` |
| `MouseEvent.layerX/Y`：從往上第一個有定位或是 stacking context 的盒子（root element 也算）的 border box，捲動容器和其他裁切不算，沒有就是 `pageX/Y`；讀的時候算，捨去、夾成 `long`，先對齊 1/64 px；沒有盒子的目標從往上第一個有盒子的祖先；retarget 之後的目標 | `cargo test -p shell --test it -- geometry::mouse_event_layer_coordinates`；WPT 的 `uievents/mouse/layer-coords-transform.html`（加進清單） |
| `focus()` 照 `CenterIfNeeded`：整個看得到不捲、部分看得到捲到最近的邊、看不到置中，兩個軸、捲動容器也是；`preventScroll` 不捲 | `cargo test -p shell --test it -- focus::focusing_scrolls_into_view_if_needed`；`cargo test -p renderer --lib -- scroll::tests::each_alignment_exposes_a_span_as_blink_does` |
| 文字控制項和 editing host 捲 caret（在 border 和 padding 裡面、在 anonymous block 裡也是）；kun 畫不出 caret 時捲元素的盒子 | `cargo test -p shell --test it -- focus::focusing_scrolls_into_view_if_needed`；WPT 的 `contenteditable-near-cursor`、`textarea-scroll-selection`、`position-sticky-input-box-gets-focused-after-scroll`（整份通過） |
| editing host 拿到 focus 時 caret 在 caret 能在的第一個位置：第一個排版出來的字元、保留的換行、`<br>` 或 caret 不能進去的元素之前，不能編輯的元素的 parent 的開頭，或 host 的開頭 | `cargo test -p shell --test it -- focus::an_editing_hosts_caret_on_focus`；WPT 的 `selection/contenteditable/initial-selection-on-focus.tentative.html`（151 個 subtest 通過 141 個，剩下的是空的 inline 的 editing host，`.ini` 的註解） |

**CSS 數學函式**（預期值是 Chrome 152 的，`details.md`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `round()`（四種 strategy）、`mod()`、`rem()`、`abs()`、`sign()`、三角函數、`pow()`、`sqrt()`、`hypot()`、`log()`、`exp()`：參數的 kind 照 css-values-4，標準單位的在解析時算出來，相對長度和百分比的留到元素上算；0、無限大、NaN 和正負零照規範 | `cargo test -p style --test it -- calc::the_other_math_functions_written_back calc::the_other_math_functions_computed calc::the_other_math_functions_that_dont_parse`；WPT 的 `css/css-values/` 的 invalid 六份、crashtest 三份、`signed-zero.html`（加進清單） |
| 乘上、除以只有元素算得出來的 number（`calc(10px * sign(1em))`），寫回照 Chrome：葉節點合成一個放前面，加總裡的乘積加括號，減掉的寫在 ` - ` 之後，乘積裡的加總加括號 | `cargo test -p style --test it -- calc::the_other_math_functions_written_back calc::parsed_simplified_and_written_back` |
| 只收百分比的地方收只有百分比的函式；number、角度、時間裡要元素才算得出來的無效 | `cargo test -p style --test it -- calc::the_other_math_functions_written_back calc::the_other_math_functions_that_dont_parse` |
| 很大的 `scale()` 繞著 transform-origin 的乘積不溢位成無限大（之後不會變成 NaN） | `cargo test -p gfx --lib -- a_product_stays_finite` |

**文件的 stylesheet 由 `dom` 持有**

| 項目 | 怎麼驗證 |
| --- | --- |
| `<style>` 的 sheet 第一次要時才解析（文字分幾次加進來也只解析一次），之後同一份、`url()` 照當時的 base URL；`media` 改了不變，文字、`type` 改了、移除和插入時重新做 | `cargo test -p dom --test it -- style_elements`（`dom::testing::style_sheets_parsed`）；`cargo test -p style --test it -- invalidation::a_style_is_parsed_when_its_sheets_are_collected` |
| 同樣的文字和 base URL 共用，沒人用了就丟，清的時候留著還在用的 | `cargo test -p dom --test it -- style_elements::the_same_text_and_base_url_share_their_contents style_elements::the_cache_keeps_the_sheets_in_use`；`cargo test -p style --test it -- shadow::the_same_text_is_parsed_once` |
| `<link>` 載入的文字交給 `dom`，照它來的 URL 解析、標記 tree scope；載入之後 `disabled`、`rel` 改了照樣不套用、改回來再套用 | `cargo test -p dom --test it -- style_elements::a_links_sheet_once_it_has_loaded`；`cargo test -p style --test it -- invalidation::a_linked_sheet_that_arrives_is_collected invalidation::a_loaded_links_sheet_follows_its_attributes media:: cascade::` |
| 畫面不變 | kun 的 reftest 全部（189 份）、情境全部（70 個）、WPT（下面） |
| style 的成本 | `bench.py blink-perf --dirs css --against bf09c473`（`details.md`） |

**CSSOM**（預期值是 Chrome 152 的，`details.md`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `<style>`、`<link>`、SVG 的 `<style>` 的 `sheet`：connected、CSS 的 `type`、`<link>` 載入了才有；是元素的 sheet 的期間同一個物件，`media`、`title` 改了照舊；文字、`type`、`<link>` 的 `rel`、`href`、`disabled` 改了，插入、移除時換一個，舊的 `ownerNode` 是 `null`、改動留在它自己 | `cargo test -p dom --test it -- style_elements`；`cargo test -p dom_js --test it -- style_sheets::element_sheets_and_style_sheets`；WPT 的 `css/cssom/`、`html/semantics/document-metadata/` 的 26 份 |
| 經過 sheet 的改動（規則、規則的 `style`、`media`、`disabled`）是元素的，照規則重算可能配到的元素；共用內容的另一個元素不受影響；`<style>.disabled` 和 sheet 的是同一個旗標 | `cargo test -p dom --test it -- style_elements::an_edit_through_the_object_is_the_elements`；`cargo test -p style --test it -- invalidation::an_edit_through_a_style_elements_sheet_styles_what_it_may_match`；`cargo test -p shell --test it -- style_sheets::a_style_elements_sheet_edited_by_script_styles_the_page`；情境 `css-in-js` |
| `styleSheets`：每次同一個，tree order，不含 shadow tree 的和 adopted 的，DOM 沒變時不再走一次 tree scope | `cargo test -p dom --test it -- style_elements::the_list_is_its_tree_scopes_sheets`（`dom::testing::style_sheet_list_walks`）；`dom_js` 的同一個 |
| 跨 origin 的 `<link>`：沒有 `crossorigin` 照樣套用、讀規則丟 `SecurityError`；`crossorigin` 照 CORS，允許的讀得到，不允許的沒有規則；載入失敗的有空的 sheet；`href` 是要求的網址；alternate 的照樣載入、在清單裡、不套用，也不擋 parser 的 script | `cargo test -p shell --test it -- style_sheets::link_sheets_from_this_origin_and_others style_sheets::an_alternate_sheet_keeps_no_script_waiting` |
| preferred style sheet set：第一個有 title、不是 alternate 的；之後不變；title 不同的、沒有 title 的 alternate 不套用；shadow tree 不看 title；改 `title` 等下次收集才算 | `cargo test -p style --test it -- invalidation::only_the_preferred_sets_sheets_apply`；`cargo test -p dom --test it -- style_elements::media_and_title_keep_the_object` |
| 元素的 sheet 不能 `replace`、`replaceSync`、adopt（`NotAllowedError`）；舊的 `rules`、`addRule`、`removeRule` | `cargo test -p dom_js --test it -- style_sheets::` |
| 規則多的 sheet 各自一份 `RuleSet`，連續的小 sheet 合成一份；沒變的 set 和它的特徵不重新算；set 的 media 和 media query 照 viewport；每個 set 的特徵、dependency 合併 | `cargo test -p style --test it -- invalidation::a_large_sheet_is_a_set_of_its_own invalidation::small_sheets_are_one_set invalidation::a_large_sheets_media_takes_its_set_or_leaves_it_out invalidation::a_runs_media_rules_follow_the_viewport invalidation::every_sets_rules_count_for_invalidation invalidation::every_sets_selectors_count_for_what_changes_style`（`style::rules_added_for_testing`、`sets_looked_through_for_testing`） |
| `insertRule` 和規則的改動只解析新的那條的 `url()` | `cargo test -p style --test it -- stylesheet::an_edit_resolves_only_its_rules_urls`（`style::rules_resolved_for_testing`） |
| style 的成本 | `bench.py blink-perf --dirs css --against dce731f4`（`details.md` 的「第 4 段的量測」） |

## 做過的檢查

2026-10-23 到 24，Fedora 44（AMD Ryzen AI 9 HX 370），四段都在這台上做。第 2 段開始之前 `0080` rebase 到 `main`（RFC 0075 做完、0081 和 0082 排程之後）。

- **做了什麼**（設計見 `README.md` 的「詳細設計」）：
  - 第 1 段：`dom_api` 的 `geometry::to_json`（`[JsOnly]`：Lua 的 generator 沒有 `list<tuple<string, f64>>` 的回傳，guest 有屬性）、`ui_event::layer_x`、`layer_y`，`focus` 改用 `ScrollAlignment::CenterIfNeeded`、文字控制項和 editing host 先 `scroll_caret_into_view`，`selection::focused` 的初始位置；`dom` 的 `ScrollAlignment::CenterIfNeeded`、`LayoutClient::scroll_caret_into_view`；renderer 的 `scroll.rs` 的 `expose_axis` 的新對齊、`FrameView::scroll_caret_into_view`（`selection::caret_rect`，和 `paint::caret_rect` 畫 caret 的同一個位置）。
  - 第 2 段：`css` 的 `calc.rs`：`Node::Function`、`Multiply`、`Divide`、`Negate`，`Function::apply`（`round`、`modulus` 照 css-values-4 的特殊情況），`math_function` 的 type checking 和在解析時算出來的條件，`needs_element`（`parse_absolute` 不收要元素的，`parse_percentage` 收只有百分比的），serialize；`gfx` 的 `Transform::then` 溢位時改用 f64。
  - 第 3 段：`dom` 的 `style_element.rs`（`StyleSheetTexts`、`DomReadScope::style_sheet_contents`、`DomMutationScope::set_linked_style_sheet`）、`<style>`、`<link>` 建立時配置 rare data、`enable_style_sheet` 丟掉解析好的內容；`style` 的 `collect_document_sheets` 和 `shadow_scope_sheets` 從 `dom` 拿，`LinkedStyleSheets`、`style_texts`、`SheetText` 拿掉，`recalc_style` 少一個參數（117 個測試的呼叫跟著改）；renderer 的 `FrameView::did_receive_resource` 交給 `dom`。`soundness.md` 的 I3 加上 `<style>`、`<link>` 的 rare data 和 `set_linked_style_sheet`。
  - 第 4 段：
    - `css`：`insertRule`、規則的改動只解析新的那條的 `url()`；`MediaQueryList::is_empty`；`RuleSet`、`url()` 的計數。
    - `dom`：`StyleSheetData`（元素的 sheet、script 的物件和 `styleSheets` 的 edge，取代第 3 段的 box）、`ElementSheet` 的 media 和 `generation`、`style_sheet`、`style_sheet_list`、`has_style_sheet`、`style_sheet_owners`、`style_sheet_attribute_changed`，`stylesheet_link_href` 和 `is_css_type` 從 `style` 搬來（alternate 的也載入）；`CssStyleSheet` 的 `owner_node` 和 `SheetOwner`，`css_style_sheet_changed` 交回元素；`css_rules_of` 換成 `css_rule_count`、`css_rule_at`（不複製整份規則）；`StyleSheetList` 這個新的 script 物件。`soundness.md` 加上 `style_element` 的一節。
    - `style`：document 的 sheet 分組（`SheetGroup`、`group_sheets`，門檻 512 條）、每個 set 的特徵（`SetFeatures`、`SetFeaturesCache`、`FeatureSets::merge` 等）、media 照 `dom` 的、preferred set。
    - `dom_api` 和 IDL：`styleSheets`、`sheet`、`StyleSheetList`、`href`、`title`、`ownerNode`、`rules`、`addRule`、`removeRule`、`<link>` 的 `media`、`type`、`disabled`，`SecurityError` 和 `NotAllowedError`。
    - renderer：stylesheet 的請求帶文件的 origin、`crossorigin` 走 CORS、origin-clean、載入失敗是空的 sheet、alternate 的不擋 render 和 script。
  - 新的計數：`dom::testing::style_sheets_parsed`、`style_sheet_list_walks`，`style::rules_added_for_testing`、`sets_looked_through_for_testing`、`rules_resolved_for_testing`（都是 thread-local，加進 `sanity.py boundary` 的例外清單）。
- **實作時改的**（記在 `README.md` 的「已決定的事項」）：
  - 第 2 條：Chrome 152 的 `overflow: hidden` 和捲動容器不算 layer，和 Blink 的 `LayerTypeRequired` 不同，照 Chrome。`<html>` 有定位或是 stacking context 時也是 layer：第一版把 root element 排除了，mutation test 沒抓到那個 mutant，探測之後發現是 bug。
  - 第 3 條：editing host 拿到 focus 時 caret 原本放在第一個葉節點，host 的開頭是收起來的空白時那裡畫不出 caret、也捲不到（`contenteditable-near-cursor` 因此多捲 8px），改成 caret 能在的第一個位置。第一版只略過沒有盒子的文字，`initial-selection-on-focus` 有 18 項由通過變失敗（`<br>`、`<input>`、不能編輯的元素在前面時），照 Chrome 的探測補上這些規則之後全部回來。
  - 第 8 條：sheet 重新做的地方不能寫 heap，改用 `generation` 判斷舊的物件；edge 放進 `StyleSheetData`。
  - 第 11 條：使用者先選照 Chromium 每個 sheet 一份，補上比對的成本後改成大的各自一份、小的合成一份；實作時特徵也每個 set 算一次、存著（只分 `RuleSet` 不夠，profile 見 `details.md`）。
- **探測和測試時找到的**：
  - `expose_axis` 判斷「整個看得到」用的是 `shown >= size`：caret 在 7400px 附近時 `f32` 相減差在最後幾位，整個看得到的 caret 被當成部分看得到，捲到它的上緣。改成比兩端。
  - kun 的 layout 是 `f32`：0.6px 的 layer 裡 10px 的點差 14.9999998，`layerX` 捨去成 14（Chrome 15），改成先對齊 1/64 px。
  - Chrome 的探測要讓分頁有 focus（`Page.bringToFront`）：沒有 focus 時 Selection API 不會讓 editing host 拿到 focus。headless 的 Chrome 的 viewport 是 800×456，要 `Emulation.setDeviceMetricsOverride` 才是 800×600。測試頁裡的文字的行高看字型（kun 和 Chrome 差 1px）：`layerX` 的測試量完文字的那一項就把它拿掉；caret 的邊決定的捲動位置容許 2px。
  - 數學函式：Chrome 只在參數都是標準單位時算出來（`round(7em, 2em)`、`round(10%, 3%)` 留著）；加總裡的乘積加括號，減掉的非葉節點和加上負的係數寫法不同（css-values-4 的 Negate），kun 原本的 `calc(10px - 2 * min(1px, 1em))` 讀回沒有括號，照 Chrome 改了。`min()`、`max()` 和元素上的比較不分 `-0`、`mod()` 的 0 沒有帶除數的正負號、元素上算的值的 `key()` 把 `-0` 加成 `+0`：都照 `signed-zero.html` 和 Chrome 改了。最外層的 `atan2()` Chrome 包在 `calc()` 裡，但 kun 的角度在解析時就算出來，那段程式走不到，拿掉了。
  - `exp-log-serialize.html` 讓 renderer 在 debug build 的 pre-paint 檢查 panic：`scale(calc(log(0)))` 夾成最大的 f32，繞著 transform-origin 的乘積溢位成無限大，下一次相乘的 `0 × infinity` 是 NaN。`scale(calc(-infinity))` 在 `main` 上也會這樣；改了 `Transform::then`。
  - CSSOM（`details.md` 的「第 4 段：`sheet` 的動態行為、跨 origin、preferred set」）：kun 跑同一組頁面和 Chrome 一樣，除了 `<link>` 的屬性改回來時 kun 不重新載入（`docs/limitations.md`）；第一版改 `title` 時標記 tree scope、馬上重新判斷，Chrome 等下次收集，照 Chrome 改了。CSS-in-JS 的測試頁面在 Chrome 跑出一樣的結果。
- **量測**（`details.md`）：
  - 第 3 段：第一版 rare data 多兩個欄位，`ChangeStyleCSSVariableRecalc`（20000 個有 `classList` 的元素）慢 4%，改成一個 box 之後一樣；`HasDescendantInvalidation` 慢 2.7%，`perf` 看到的差別只在 inline 的分配，第 3 段的函式不在熱點上。
  - 第 4 段：每個 frame `insertRule` 的那組 1.9 → 0.5 ms、7.1 → 1.0 ms、16.6 → 8.2 ms；`blink-perf` 的 CSS 51 頁和第 3 段比的幾何平均 0.999。`SelectorCountScaling` 慢一倍（重新插入、文字一樣的 `<style>` 的 media 是另一個 `Arc`），改成指標不同時比內容；`ChangeStyleCSSVariableRecalc` 慢 4.1%、`ClassInvalidation` 慢 5.3%，rare data 回到第 3 段的大小之後照舊，指令數不多（`ClassInvalidation` 每輪少 3.6%）、同樣的事另外量兩邊一樣，看作編譯出來的差別。
  - 量測的工具在寫報告時除以 0 結束（舊版有一頁是 0 ms），和這份無關，數字從 `compare.json` 算。
- **mutation test**（每次跑完都確認檔案還原，`target/mutate/originals/` 不存在）：
  - `mutants-1.toml`，最後 30 個。第一次 29 個沒抓到 3 個：root element 的 layer（上面的 bug）、caret 從 block 的 border box、anonymous block 的位移（測試的 `<textarea>` 的 caret 在沒有 border 和 padding 的 inner editor 裡：給那個 editing host 加上 border 和 padding）。改了之後連同新的 2 個（不能編輯的元素）重跑 5 個，都抓到。
  - `mutants-2.toml`，最後 38 個。第一次 39 個沒抓到 5 個：反三角函數和指數函數收角度（改測 `rotate(asin(1deg))`、`opacity: exp(1deg)`）、`max()` 的零（補 `max(0px, -0px)`）、商的倍數乘到分母（補 `calc(2 * (10px / sign(-1em)))`）、乘積裡留下 1（補 `abs(sign(1em) * sign(1em) * 1)`），以及 `round()` 除以 0 是 NaN 的檢查：等價（`a / 0` 是無限大或 NaN，乘以 0 就是 NaN），拿掉了那個檢查和 mutant。重跑那 4 個都抓到。
  - `mutants-3.toml`，12 個。第一次沒抓到「任何 `<link>` 的 sheet 都套用」：補了載入之後加 `disabled`、改 `rel` 再改回來（`invalidation::a_loaded_links_sheet_follows_its_attributes`），重跑抓到。
  - `mutants-4.toml`，37 個。第一次沒抓到「`<link>` 的 `rel` 改了不重新做 sheet」：測試先改 `disabled`，那一步已經重新做了；改成每個屬性前後各拿一次物件比，重跑抓到。另外補了「alternate 的不擋 parser 的 script」的測試和 mutant（不擋 render 的那一半記在 `docs/test-gaps.md`）。
- **WPT**：
  - 第 1 段：拿 `main` 跑同一組目錄當基準（`dom/ranges/tentative/`、`selection/`、`css/css-overflow/`、`html/semantics/forms/textfieldselection/`、`shadow-dom/focus-navigation/`、`css/css-position/sticky/`、`css/css-text/text-transform/`、`custom-elements/reactions/`、`dom/events/scrolling/`、`html/rendering/widgets/`、`html/semantics/forms/the-textarea-element/`），67 項變好、沒有變差的；`css/css-scroll-anchoring/`、`css/geometry/`、`html/interaction/focus/`、`css/cssom-view/`、`uievents/`、`editing/` 沒有其他變化。拿掉 4 份 `.ini`（`DOMRect-nan`、`contenteditable-near-cursor`、`textarea-scroll-selection`、`position-sticky-input-box-gets-focused-after-scroll`），`uievents/mouse/layer-coords-transform.html` 加進清單。基準裡 `main` 本來就有 151 個非預期的結果（`selection/` 等：多數是之前的 `ReferenceError` 拿掉之後多跑出來的，也有 `stringifier_editable_element`、`selectionchange-bubble`、`collapse-pre-linestart-1`、`-2`、`cross-shadow-boundary-select-root`），和這份無關。
  - 第 2 段：`css/css-values/` 的數學函式的 24 份用 `--all` 跑過（`details.md`），整份通過的十份加進清單；`css-scroll-anchoring/adjustment-followed-by-scrollBy.html` 整份通過（它的 div 用 `round()`）。`.ini` 的註解或測試名稱寫到這些函式的 40 份重跑過。
  - 第 4 段：`css/cssom/` 整個目錄加進清單：290 份，265 份跑得起來，182 份整份通過；13 份等 `<link>` 的 `load`、`error` 事件或 `@import` 到逾時，標 `disabled`。`html/semantics/document-metadata/` 的 26 份加進清單。之前因為沒有 `sheet`、`styleSheets` 而失敗的：`css-animations/`、`css-transitions/`、`css-shadow/`、`shadow-dom/`、`css-ui/`、`css-variables/`、`domparsing/` 的 26 份整份通過；`.ini` 寫著「kun 沒有 `sheet`」的 16 份照剩下的原因改了註解（例如 `parsing-testcommon.js` 的 `_set_style` 用 `innerText`，kun 沒有）。兩次跑的結果一樣。
  - 每段結束時跑的那組（最後一次：`css/css-values/`、`filter-effects/`、`css-fonts/`、`css-easing/`、`css-transitions/`、`css-animations/`、`css-images/`、`css-backgrounds/`、`css-transforms/`、`css-scroll-anchoring/`、`css-borders/`、`css-color/`、`css-sizing/`、`cssom/`、`css-cascade/`、`selectors/`、`css-scoping/`、`mediaqueries/`、`css-shadow/`、`CSS2/`、`shadow-dom/`、`html/semantics/document-metadata/`、`css-conditional/`、`css-counter-styles/`）：46628 個檢查，非預期的只有 `main` 上本來就有的（用 `main` 跑過，結果一樣）。
- **順帶找到的、和這份無關的**（另外處理）：
  - 記在 `docs/limitations.md`：頁面捲動之後 fixed 盒子裡的元素的 `getBoundingClientRect` 多扣了捲動；`scrollIntoView` 的置中算出的捲動位置留著小數（Chrome 取整）。
  - `main` 上本來就失敗的 WPT：`css-easing/step-timing-functions-syntax.html` 的 `steps(calc(0/0), jump-none)`（kun 讀回 `steps(2, jump-none)`，Chrome 無效）；reftest `css-transforms/group/svg-transform-nested-007.html`（邊上的像素差 1）；`css/selectors/invalidation/has-with-pseudo-class.html` 的 4 項；`html/browsers/history/the-location-interface/same-hash.html` 十幾個非預期的結果。
  - `css/css-ui/appearance-auto-non-html-namespace-001.html`（reftest）失敗：不是 HTML namespace 的 `button`、`input` 吃到 UA 的規則、畫出邊框。第 3 段的版本讀到的 computed style 一模一樣，不是第 4 段造成的，沒有更新它的預期結果。
- **跑過的**：
  - 各段改到的 crate 的測試（第 1 段 `dom`、`dom_api`、`dom_js`、`dom_wasm`、`paint`、`renderer`、`shell`；第 2 段 `css`、`style`、`gfx`、`paint`、`layout`、`dom_api`、`dom_js`；第 3 段 `dom`、`style`、`layout`、`paint`、`dom_api`、`dom_js`、`renderer`、`shell`；第 4 段 `css`、`dom`、`dom_api`、`style`、`dom_js`、`dom_wasm` 和 `shell` 的 `style_sheets::`）；kun 的 reftest：第 2 段 `tests/reftests/transforms`、`tests/reftests/svg`（8 份），第 3、4 段全部（189 份，符合預期）；情境全部：第 3 段 70 個，第 4 段 71 個（包括新的 `css-in-js`）；上面的 WPT。
  - 每段結束時：`cargo test --workspace --no-fail-fast`（第 1 段 3644 個通過、1 分 36 秒；第 2 段 3695 個、4 分 1 秒；第 3 段 3703 個、3 分 22 秒；第 4 段 3723 個、2 分鐘；都是 3 個 ignored）、`cargo fmt --check`（連同 `sdk/lua/generator`；第 4 段第一次有十幾個檔案的排版，套用了）、`cargo clippy --workspace --all-targets -- -D warnings`（第 3 段第一次抓到 `StyleSheetTexts` 的型別太複雜、測試裡多餘的 `clone`）、`sanity.py` 的 `deps`、`boundary`（新的計數加進例外清單）、`unsafe`、`tests`、`numerics`、`docs`（第 4 段第一次抓到 `limitations.md` 引用的節名寫錯；只剩 README 寫的 rusty_v8 的 build 目錄在這台不存在，和這份無關）。
  - IDL 多了成員的第 1、4 段：`MACHOPO_UPDATE_GOLDEN=1` 重新產生 `crates/dom_api/tests/machopo-dom.wit`、`crates/dom_api/tests/spec-missing.txt` 和 `crates/dom_wasm/src/typed_list.rs`（第 4 段舊的 `typed_list.rs` 讓產生它的測試編不過，先清空再產生），`python3 scripts/ci.py guests` 重編 wasm 的 guest。
- **不穩定的**：第 3 段情境全部第一次跑時 `script-wasm` 失敗在第 6 行的 `wait-loaded`（10 秒）：`about:wasm-demo` 嵌著三個 wasm script（C、Rust 和 Lua 的 runtime），整套情境平行跑時 CPU 滿載（32 秒用了 6 分鐘的 CPU），編它們超過 10 秒。它的 `<style>` 在重算 style 時才解析，不擋載入（沒有 `<link>`）。單獨跑 3 次、整套再跑 2 次都通過，第 4 段的整套也通過；CI 上也可能碰到，之後看要不要把 wasm 的情境的等待放長。其他沒有。
