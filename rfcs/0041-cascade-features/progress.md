# RFC 0041 的進度：cascade 的功能

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-13。完成：2026-10-13，Windows 11（13th Gen Intel Core i9-13900K；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0041` 上做五段，各段做完、審核之後 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 自訂 property 和 `var()`（cascade 先找勝出的宣告） | 完成，merge 回 `main`（`d7b83d46`） | 2026-10-13 |
| 2a 長度和百分比的 `calc()`、`min()`、`max()`、`clamp()`（`layout`、`paint`、taffy） | 完成，merge 回 `main`（`25e85bd4`） | 2026-10-13 |
| 2b 數字、整數、角度的 `calc()` | 完成，merge 回 `main`（`25e85bd4`） | 2026-10-13 |
| 2c viewport 單位和它的重算 | 完成，merge 回 `main`（`25e85bd4`） | 2026-10-13 |
| 3a stylesheet 的 `@media`、`@supports`、`<style media>` / `<link media>` 和重新判斷 | 完成，merge 回 `main`（`d8dbc624`） | 2026-10-13 |
| 3b CSSOM：`CSSGroupingRule`、`CSSMediaRule`、`CSSSupportsRule`、`MediaList` | 完成，merge 回 `main`（`d8dbc624`） | 2026-10-13 |
| 3c `matchMedia()` 和 `MediaQueryList` | 完成，merge 回 `main`（`d8dbc624`） | 2026-10-13 |
| 4a `:hover`、`:active` 和滑鼠的邊界事件 | 完成，merge 回 `main`（`c2ee3618`） | 2026-10-13 |
| 4b `:lang()`、`:read-only` / `:read-write`、`:required` / `:optional`、`:target` | 完成，merge 回 `main`（`c2ee3618`） | 2026-10-13 |
| 5 邏輯屬性 | 完成 | 2026-10-13 |

## 驗收

### 自訂 property 和 `var()`

| 項目 | 怎麼驗證 |
| --- | --- |
| `StyleVariables`：設定、讀取、取代、沒有值的項目；複製之後共用、改了才複製路徑（同樣的值不複製）；比較和建立的順序無關、沒有值和沒有名稱不同；列出每個名稱一次；整個 hash 相同的項目放在同一格 | `cargo test -p computed_style variables::` |
| 代換和繼承；fallback（巢狀、空的、本身失敗時整個值 invalid）；invalid at computed-value time 是 `unset`（繼承的取父元素的值、其他的取初始值，不是前面輸掉的宣告）；循環（自己、兩個、在循環之後才走到的不算、失敗之後還找得到的、沒用到的 fallback 不算）和引用循環的照自己的 fallback；cascade 的順序、`!important`、名稱分大小寫；`initial`、`inherit`、`unset` 和代換之後的 CSS-wide 關鍵字；shorthand 代換進每個 longhand、代換失敗時每個 longhand 都是 `unset`；token 的代換（`var(--n)px` 不是 `10px`，`/**/`）；`em` 在用到的元素算；長度上限；`style` attribute 和 SVG 的 presentation attribute；有自訂 property 的 style 的比較 | `cargo test -p style --test it variables::` |
| CSSOM：含 `var()` 的值照原文、去掉頭尾的註解；shorthand 的值是它的原文、longhand 讀回空字串；`cssText` 寫得成 shorthand 時寫 shorthand，否則照 Chrome 把 longhand 寫成空的值；`var()` 和 fallback 的語法；`CSS.supports` 的自訂 property 和 `var()` | `cargo test -p style --test it variables::` |
| inline style 存解析好的宣告：建立元素、`setAttribute`、`Attr`、`removeAttribute` 時解析（別的 namespace 的 `style` 不算）；`set_inline_style` 存給它的宣告、attribute 寫成 `cssText`、不再解析；之後再設定 attribute 就照文字解析 | `cargo test -p dom --test it style_data::the_inline_style` |
| CSSOM 改了之後 attribute 是 Chrome 的文字、宣告留著（`length` 照舊）；cascade 讀 CSSOM 留下的宣告 | `cargo test -p dom_api --test it api::inline_style_keeps_what_its_text_cant_say`、`cargo test -p style --test it variables::the_cascade_reads_the_inline_style_cssom_left` |
| 重算時新算的自訂 property 和舊的一樣，沿用舊的 map（子孫的也共用） | `cargo test -p style --test it variables::styled_again_the_same_custom_properties_keep_their_map` |
| `getComputedStyle` 讀自訂 property（代換過、沒有值是空字串、分大小寫），`length` 和 `item()` 列在 longhand 之後、依名稱排序 | `cargo test -p dom_api --test it computed_style::it_reads_and_lists_custom_properties` |
| `LonghandId::ALL` 和 enum 一致（補上了 `position` 和四個 inset） | `cargo test -p css properties::tests::` |
| 改成只套用勝出的宣告之後，既有的 cascade 結果不變 | `cargo test -p style` |
| WPT 的 `css/css-variables/`（新匯入，預期結果的原因寫在每份 `.ini`） | `python3 scripts/test.py wpt css/css-variables/` |
| `about:demo` 的 `var()` 按鈕：方塊的顏色和寬度跟著根元素的自訂 property 變，拿掉之後回到 fallback | `python3 scripts/test.py scenarios demo-script` |

### `calc()` 和 viewport 單位

| 項目 | 怎麼驗證 |
| --- | --- |
| 解析和化簡：數字乘進去、除以數字、同單位的項加起來（絕對長度換成 px）、同單位的 `min()` / `max()` / `clamp()` 算出來、巢狀和括號、`pi`、`e`；序列化照 css-values-4 排序、負的寫成 ` - `、根是 `min()` / `max()` / `clamp()` 的照寫 | `cargo test -p style --test it calc::parsed_simplified_and_written_back` |
| 不收的：`+`、`-` 兩邊沒有空白，除以 0、長度乘長度、型別不合、`min()` 混長度和百分比、不收百分比的 property 的百分比 | `cargo test -p style --test it calc::what_doesnt_parse` |
| computed value：只有長度的算成 px、只有百分比的是 `Percent`、兩者都有的是 `Calc`；`em`、`rem` 在元素上算；不能是負的 property 夾到 0；`var()` 代換之後的 `calc()` | `cargo test -p style --test it calc::computed calc::with_var` |
| `getComputedStyle` 照 Chrome 寫 `calc(50% - 10px)`；`background-position` 從右邊或下面算的換成從左邊 | `cargo test -p style --test it calc::computed_values_are_written_as_chrome_writes_them` |
| 數字、整數、角度在解析時算成一個值（整數四捨五入、小於下限的夾到下限，`z-index`、`order`、`opacity`、`flex`、`font-weight`、`rotate()`……） | `cargo test -p style --test it calc::numbers_integers_and_angles_are_worked_out_when_parsed` |
| layout：block、flex、grid（`minmax()` 裡也是）的 `calc()` 經過 taffy 的 handle 解析，不能是負的夾到 0；handle 對齊、同一個值共用一個 | `cargo test -p layout --test it calc::`、`cargo test -p layout --lib calc::` |
| viewport 單位：16 個單位的值、`em` 的子元素拿父元素的 px、旗標只記在用到的元素；viewport 變了只重算有旗標的元素和它們的子樹，其他的沿用原本的 style，同樣的 viewport 不重算 | `cargo test -p style --test it calc::viewport_units` |
| `sizes` 的 `calc()`、viewport 單位，只有一個 `calc()` 時是值不是條件 | `cargo test -p style --lib media::tests::sizes` |
| `scrollIntoView` 不捲動 `position: fixed` 外面的 viewport | `cargo test -p renderer --lib scroll::tests::nothing_past_a_fixed_box_scrolls_into_view` |
| renderer 只換 viewport、其他都不變時，用 viewport 單位的元素重算 | `cargo test -p renderer --lib frame_view::tests::a_new_viewport_styles_viewport_units_again` |
| 視窗改大小時 `calc(10vw - 20px)` 的寬度跟著變 | `python3 scripts/test.py scenarios resize`（`about:demo` 的 `calc()` 按鈕：`demo-script`） |
| WPT：`css/css-values/` 的 92 份（新加進清單），其他目錄因此整份通過的 24 份拿掉預期結果 | `python3 scripts/test.py wpt css/css-values/` 和各目錄 |

### `@media`、`@supports` 和 `matchMedia`

| 項目 | 怎麼驗證 |
| --- | --- |
| `@media` 照 viewport 和 device pixel ratio 生效，可以巢狀；空的 list 符合、解析不了的 query 是 `not all`、不認得的 feature 不符合；`@MEDIA` 不分大小寫 | `cargo test -p style --test it media::media_rules_apply_while_their_queries_match` |
| `@supports` 照條件生效（`not`、`and`、`selector()`、巢狀的 `@media`），條件解析不了的整條丟掉 | `cargo test -p style --test it media::supports_rules_apply_if_their_condition_holds` |
| grouping rule 裡的規則照原文的位置排 cascade 的順序 | `cargo test -p style --test it media::group_rules_keep_their_place_in_the_cascade` |
| `<style media>`、`<link media>`、SVG 的 `<style media>` 是整份的條件；`media` 改了重新判斷 | `cargo test -p style --test it media::style_and_link_elements_apply_while_their_media_matches` |
| 環境改變：結果都一樣時不重算、不重建，有一個不同才整份重算；device pixel ratio 也算 | `cargo test -p style --test it media::a_new_environment_restyles_only_if_a_query_comes_out_otherwise` |
| adopt 的 stylesheet 的 `media` 和它裡面的 `@media`，環境改變時重新判斷（內容的 `RuleSet` 快取換掉） | `cargo test -p style --test it media::adopted_sheets_apply_while_their_media_matches` |
| renderer 只換 viewport 或 device scale factor 時，套用另一邊的 `@media` | `cargo test -p renderer --lib frame_view::tests::a_new_environment_applies_other_media_rules` |
| 規則的解析和 Chrome 的 `cssText`、`conditionText`（grouping rule 照 Blink 的 `AppendCSSTextForItems`），其他 at-rule 丟掉；media query 的序列化（WPT 的 `serialize-media-rule.html`、`MediaList-parsing.html` 的例子）；`appendMedium`、`deleteMedium`；grouping rule 的 `insertRule`、`deleteRule`、換規則，往上換到 sheet 的內容 | `cargo test -p style --test it stylesheet::` |
| dom 的規則物件：grouping rule 的 `cssRules` 和子規則的物件、`MediaList`、往上換規則；GC 撐住誰（每種物件、巢狀的） | `cargo test -p dom --test it script_objects:: style_sheets::`、`cargo test -p dom --test incremental_stress`（模型多了 media rule 和它裡面的修改） |
| CSSOM：`CSSMediaRule`、`CSSSupportsRule` 的 `type`、`conditionText`、`cssRules`、`parentRule`、`parentStyleSheet`（拿掉之後沒有）、`insertRule` / `deleteRule` 的錯誤和物件的搬動、裡面的規則改了傳到 sheet；`MediaList` 的每個成員；改了都標記 document 的 style | `cargo test -p dom_api --test it css_style_sheet::` |
| JS：原型鏈（`CSSMediaRule` → `CSSConditionRule` → `CSSGroupingRule` → `CSSRule`）、`String(media)`、`[PutForwards=mediaText]`、`mediaText = null` | `cargo test -p dom_js --test it style_sheets::group_rules_and_media_lists` |
| `MediaQueryList`：document、query、結果；document 的 list 照建立的順序；有 listener（或 event handler）的 script 放掉也留著，沒有的被回收 | `cargo test -p dom --test it media_query_lists::` |
| `matchMedia`、`matches` 重新判斷並記下、「evaluate media queries and report changes」記下新的結果 | `cargo test -p dom_api --test it media_query_list::` |
| JS：`matchMedia` 每次新的、`addListener` / `removeListener` / `onchange`、`MediaQueryListEvent` | `cargo test -p dom_js --test it style_sheets::match_media` |
| 視窗改大小時 `@media` 的寬度跟著變、`change` 只在結果變了時發（`about:demo` 數 `change` 的次數） | `python3 scripts/test.py scenarios resize` |
| WPT | 見「做過的檢查」 |

### pseudo-class

| 項目 | 怎麼驗證 |
| --- | --- |
| document 的 hover 和 active element：設定時狀態沿祖先移動、共有的祖先不動；移除它或包含它的節點時 parent 接手（不是元素時清掉），移除別的節點不動 | `cargo test -p dom --test it focus::hover_and_active_elements_and_their_states` |
| `:hover`、`:active` 比對 element state，只有規則用到時才重算 | `cargo test -p style --test it invalidation::hover_and_active_match_and_style_only_what_rules_use` |
| 邊界事件的順序、`relatedTarget`、冒泡，`:hover` 是滑鼠下的元素和祖先 | `cargo test -p shell --test it mouse::boundary_events_follow_the_mouse` |
| 按下任何鍵在 `mousedown` 之前設 `:active`，全放開時在 `mouseup` 之前清掉；按著時 `:hover` 只到和 active element 共同的祖先 | `cargo test -p shell --test it mouse::a_press_is_active_until_the_release` |
| 滑鼠下的元素被移除：`mousemove` 給 parent、不發 `mouseout` | `cargo test -p shell --test it mouse::what_the_mouse_was_over_taken_out`、WPT 的 `mousemove_after_mouseover_target_removed.html` |
| layout 移動了滑鼠下的東西：frame 之後重新判斷，只發邊界事件 | `cargo test -p shell --test it mouse::a_layout_moves_what_is_under_the_mouse` |
| 子 frame：iframe 也 hover、各 frame 自己的邊界事件、跨 document 沒有 `relatedTarget`、離開的 frame 沒有 `:hover` | `cargo test -p shell --test it mouse::boundary_events_across_frames` |
| 視窗裡 `:hover`、`:active` 的樣式和 `mouseenter`、`mouseleave`（`about:demo` 的色塊） | `python3 scripts/test.py scenarios demo-script` |
| `:lang()`、`:read-only`、`:read-write`、`:required`、`:optional`、`:target` 的解析和序列化：`:lang()` 的 range 是 identifier 或 string，照原文寫回，名稱不分大小寫，數字讓整條規則無效 | `cargo test -p style --test it stylesheet::pseudo_classes_serialize_as_written` |
| `:lang()` 照 extended filtering 比對：range 比對同樣和更長的 tag、不分大小寫、`*` 符合任何子標籤、少的子標籤跳過但不越過 singleton、list 裡任一個；未知的語言和子標籤超過 8 個字元的 tag 不符合；content language 改了整份重算 | `cargo test -p style --test it invalidation::lang_matches_the_language` |
| 節點的語言：最近的 `xml:lang` 或 `lang`（同一個元素上 `xml:lang` 優先）、空的是未知、shadow tree 經過 host 往上，都沒有時是 document 的 content language；content language 改了整份重算、一樣的不重算；`contenteditable` 的 editable；插入 document 的 `<meta http-equiv>` 照樹順序記下（shadow tree 裡的、沒有 `http-equiv` 的、別的 namespace 的不記） | `cargo test -p dom --test it languages::` |
| 插入的 `<meta http-equiv="content-language">` 設定 content language（pragma 不分大小寫、最後插入的勝出；沒有 `content`、別的 pragma、同一次呼叫裡又拿掉的不算） | `cargo test -p dom_api --test it api::a_content_language_pragma_sets_the_documents_language` |
| response 的 `Content-Language`（第一個逗號之前）、子 frame 用自己的 response 的、之後插入的 `<meta>` 取代 | `cargo test -p shell --test it network::content_language_comes_from_the_response` |
| `:read-write` / `:read-only`：`readonly` 適用的 `<input>` 和 `<textarea>`，沒有 `readonly`、不是 disabled（自己或 fieldset）；`contenteditable` 的 editing host 和 editable 的元素，裡面的控制項照自己的規則 | `cargo test -p dom_api --test it forms::read_write_and_read_only` |
| `:required` / `:optional`：`required` 適用的 `<input>`、`<select>`、`<textarea>`，其他元素都不符合 | `cargo test -p dom_api --test it forms::required_and_optional` |
| 表單的這幾個狀態只在規則用到時重算，`contenteditable` 改了重算子樹 | `cargo test -p style --test it invalidation::the_form_states_match_and_style_only_what_rules_use` |
| document 的 target element：`TARGET` 只在元素自己，換了時兩個都記下；移除、被別的 document adopt 之後仍然是 target | `cargo test -p dom --test it focus::the_target_element_and_its_state` |
| `:target` 比對 document 的 target element，換了只重算那兩個元素（和子樹），規則用到時才重算；adopt 到別的 document 時不符合、搬回來又符合 | `cargo test -p style --test it invalidation::target_matches_the_target_element` |
| 同一份文件內導覽到 fragment 時當場設定 target element（`id`、解碼過的、`<a name>`；指不到東西的和 top 清掉） | `cargo test -p dom_api --test it location::a_fragment_navigation_sets_the_target_element` |
| parse 完（`DOMContentLoaded` 之後）設定 target、導覽到 fragment 和 traversal 跟著改，`pushState` 不改 | `cargo test -p shell --test it history::the_target_element_follows_the_fragment` |
| 視窗裡點連結到 fragment 之後 `:target` 的樣式（`about:demo` 的色塊） | `python3 scripts/test.py scenarios demo-script` |
| WPT | 見「做過的檢查」 |

### 邏輯屬性

| 項目 | 怎麼驗證 |
| --- | --- |
| 30 個 longhand 各自設定對應的物理 longhand（block-start 是 top、inline-start 是 left、inline size 是 width），讀回一樣的值 | `cargo test -p style --test it logical::each_sets_its_physical_longhand` |
| shorthand：兩個值的（只給一個時兩邊一樣）、`border-block-start` 這類一邊的、`border-block` / `border-inline` 兩邊的；省略的照初始值 | `cargo test -p style --test it logical::shorthands_set_both_sides` |
| 和物理屬性一起照 cascade 的順序：同一個區塊裡後寫的勝出（shorthand 也是）；跨區塊照 specificity、`!important`、`style` attribute；別組不受影響 | `cargo test -p style --test it logical::logical_and_physical_go_by_the_cascade_order` |
| `var()`（shorthand 也是）、`inherit`、`initial` | `cargo test -p style --test it logical::var_and_css_wide_keywords` |
| CSSOM：照原文讀回，shorthand 從 longhand 合回（兩邊一樣時一個值，`border-block` 兩邊一樣才寫得成）、`cssText` 合成 shorthand，camelCase 的名稱 | `cargo test -p style --test it logical::cssom_reads_them_back` |
| `setProperty`：後面有同一組、對應方式不同的宣告時移到最後（值一樣也移），別組或同一種對應的留在原處 | `cargo test -p style --test it logical::set_property_moves_past_the_other_mapping` |
| `getComputedStyle` 列出它們，照物理的 property 解析（`is_layout_dependent`、`physical_property`） | `cargo test -p style --test it logical::computed_style_lists_and_resolves_them` |
| used value：`margin-inline-start`、`padding-block-end`、`inline-size`、`block-size`、`margin-block` 是盒子的值 | `cargo test -p dom_api --test it computed_style::resolved_values_come_from_the_box` |
| 視窗裡 `margin-inline-start` 和 `inline-size` 移動、加寬方塊，之後設的 `margin-left` 勝出（`about:demo` 的按鈕） | `python3 scripts/test.py scenarios demo-script` |
| WPT 的 `css/css-logical/`（新匯入，預期結果的原因寫在每份 `.ini`） | `python3 scripts/test.py wpt css/css-logical/` |

## 做過的檢查

都在 Windows 11 上做（2026-10-13）。

### 對照 Chromium 的原始碼

- **自訂 property**：`StyleInheritedVariables` 已經沒有 `root_`，改成 `StyleVariables` 的 `HashTrieNode`，第 2 條因此重新決定；`getComputedStyle` 的 `length` 含自訂 property，連 invalid 的也算（`CSSEnumeratedCustomProperties`）；代換的上限 `kMaxVariableBytes` 是 2 MiB；SVG 的 presentation attribute 和 CSS 走同一個 `CSSPropertyParser::ParseValue`，所以也收 `var()`（一開始以為不收、加了過濾，查過之後拿掉）。
- **`calc()`**：computed value 照 `CalculationValue` 的 `PixelsAndPercent` 存 px 和百分比，不能是負的照 `ValueRange` 在用的時候夾（第 5 條）；viewport 單位的重算照 `Document::MarkViewportUnitDirty`（第 7 條）；`getComputedStyle` 的 `calc()` 和 `background-position` 從右邊算的值照 Chrome 的寫法。
- **`@media`**：`RuleSet::AddRulesFromSheet` 建的時候跳過不符合的 `@media`、記下每個 query 和結果，`DidMediaQueryResultsChange` 判斷要不要重建；`StyleSheetContents::EnsureRuleSet` 只快取一個（選項的描述原本以為依結果分開快取，照原始碼改，第 10 條）；`@supports` 的條件解析時就算好，`conditionText` 是空白簡化過的原文；`CSSMediaRule::cssText` 和 `AppendCSSTextForItems` 的格式；`MediaQueryList` 的 `matches` 每次重新判斷並記下（`UpdateMatches`），`MediaFeaturesChanged` 只在結果不同時發 `change`，有 listener 時算 pending activity。
- **滑鼠**：`BoundaryEventDispatcher::SendBoundaryEvents` 的順序（`mouseout`、從子到父的 `mouseleave`、`mouseover`、從父到子的 `mouseenter`，到共同的祖先為止，祖先鏈到 document）；`MouseEventManager::element_under_mouse_` 追蹤移除（移除的不發 `mouseout`，`mousemove` 給還在的祖先）；`Document::UpdateHoverActiveState`（按著鍵時 hover 只到 active chain，frame owner 也 hover）；任何鍵的按下都是 `HitTestRequest::kActive`；`RecomputeMouseHoverState` 只發邊界事件。
- **`<meta http-equiv>`**：`HTMLMetaElement::ProcessHttpEquiv` 和 `HttpEquiv::Process` 用 `EqualIgnoringAsciiCase` 照原樣比較 `http-equiv`、不去掉空白，`content` 整個交給 `SetContentLanguage`。原本的程式先 trim，mutation test 抓不到「不 trim」，查過之後拿掉 trim。
- **邏輯屬性**：`MutableCSSPropertyValueSet::FindInsertionPointForID` 在設定的 property 後面有同一組、對應方式不同的宣告時，把舊的拿掉、新的放到最後；`StylePropertySerializer::AsText` 沒有規範那條「中間夾著同一組、對應方式不同的宣告就不合成 shorthand」，`border-block` 照 `BorderPropertyValue` 要兩邊一樣才寫得成它。

### 實作時改的設計

- **inline style 存解析好的宣告**（第 1 段審核之後，第 4 條）：`cssText` 改回 Chrome 的寫法，WPT 的 `variable-cssText.html` 的 target9 通過；`variable-definition.html` 的「CSSOM.setProperty with space」1、2 因此從通過變成失敗：`setProperty` 收名稱裡有空白的自訂 property（Blink 的 `IsValidVariableName` 只看開頭的 `--`），現在和 Chrome 一樣讀得到；Chromium 自己的預期結果也是這兩項失敗，寫進 `.ini`。
- **3b 的兩個決定**（第 11、12 條）：先列了選項，使用者要求比較優缺之後才決定。
- **`:target` 的元素移除、被別的 document adopt 之後仍然是 target**：README 原本寫「移除時清掉（Blink 的 `Element::RemovedFrom`）」，實作時照 WPT 的 `target-pseudo-after-reinsertion.html`、`target-pseudo-after-adoption.html`（whatwg/html#10029；前一份是 Chromium 修 crbug 1252507 時加的）改成 document 一直指著它、`:target` 看元素目前的 document，README 跟著改。
- **`kun:dom` 的 WIT**（第 3 段）：只多了 resource 和函式（`media-list`、`css-grouping-rule`、`css-condition-rule`、`css-media-rule`、`media-query-list`、`media-query-list-event`、`match-media`、iframe 的 `width` / `height`），順序因為 `core_events` 排到 `core_css` 之前而變了（`MediaQueryListEvent` 的父介面在那裡），不升版；用 `ci.py guests` 重新產生 Lua guest。

### WPT

先用 `test.py wpt --all` 試跑相關的目錄，整份通過的加進清單（`tests/wpt/include.txt`），預期失敗的原因寫在每份 `.ini`。每段做完時清單裡相關的目錄都是 0 個預期外（Windows 上一次跑整個 `css/` 的命令列太長，WinError 206，分批跑）。

- **自訂 property**：`css/css-variables/` 整個目錄：249 個測試，110 個 reftest 通過，11 個等到 timeout 的動畫和 transition 測試 `disabled`；第 1 段做完時 testharness.js 的子測試 558 個裡 328 個通過。之後 `@supports` 讓 65 個 `variable-supports` reftest 通過，邏輯屬性讓 `variable-reference-visited.html` 通過。還失敗的：window 的 named property、`revert` 和註冊的自訂 property、pseudo-element 和 counter、`@font-face`、kun 沒有的 property，以及 css-variables-2 較新的規則（`var()` 的名稱代換、只有空白的值保留一個空白，`variable-supports-30`、`64` 也是這個；Chromium 自己的預期結果也是失敗）。`css/cssom/` 和 `css-conditional/js/` 的 7 份因此有子測試通過，其中 5 份整份通過。
- **`calc()` 和 viewport 單位**：`css/css-values/` 的 512 個裡，和 math function、長度、角度、viewport 單位有關、整份通過的 92 個（目錄不整個加：其他的大多等 kun 沒有的 property、單位和 math function）；其他目錄因此整份通過的 24 份拿掉 `.ini`。`the-img-element/` 的 `move-element-and-scroll.html`（原本因為沒有 `vh` 結果不定、`disabled`）和 `lazy-out-of-band-load.html` 連跑 5 次都整份通過，拿掉 `.ini`。
- **`@media`、`@supports`、`matchMedia`**：`css/css-conditional/` 102 個、`css/mediaqueries/` 32 個（container query 以外整份通過的），`css/cssom/` 1 份、`css/cssom-view/` 8 份整份通過；其他的失敗：container query、`document.styleSheets`、`@namespace`、`@font-face`、nesting、css-conditional-5 的 `at-rule()` 和 kun 沒有的 media feature。`css/cssom/` 的 `MediaList`、`CSSGroupingRule`、`serialize-media-rule.html` 這些用 `document.styleSheets` 或 `<style>.sheet`，kun 跑不了；`serialize-media-rule.html` 和 `MediaList-parsing.html` 的例子抄進 `style` 的 `stylesheet::` 測試，記進 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」。補 `HTMLIFrameElement` 的 `width`、`height` 之後 `the-img-element/` 也有 2 份整份通過。
- **pseudo-class**：`uievents/mouse/` 的 5 份、`mousemove-between.html`、`css/selectors/active-after-relayouts.html`；`css/selectors/i18n/` 整個目錄和其他 `:lang()` 的測試、`:read-write` 和 `:required` 的 reftest 和 testharness.js 測試、`:target` 的兩份。其他的失敗：Pointer Events、window 的 named property、dialog 和 popover、shadow tree 的 layout、`offsetX`、`::slotted`、`designMode`、XHTML 走 HTML parser（`xml:lang` 不在 XML namespace）。`dom/events/non-cancelable-when-passive/` 的 4 份 `*-event-listener-on-body.html` 因為 viewport 單位（頁面的 `height: 200vh` 讓滾輪的位置落在 `body` 上）整份通過；`the-autofocus-attribute/` 的兩份各多一項通過（`querySelector(':target')`），都連跑 3 次。
- **邏輯屬性**：`css/css-logical/parsing/` 整個目錄，和 box 屬性、大小、`var()` 的那幾份：`horizontal-tb`、`ltr` 的子測試都通過，其他 writing mode 的失敗；其他的失敗等 `ex`、最小和最大的大小的 `min-content` / `max-content`、`will-change` 和 table（物理屬性的同一類測試也是）。`css/cssom/` 的 `cssstyledeclaration-setter-logical.html` 整份通過、`cssstyledeclaration-csstext.html` 和 `getComputedStyle-resolved-colors.html` 多了通過的子測試；`the-img-element/sizes/sizes-auto.html` 的 `inline-size` 那幾項通過，`min-inline-size: 10px` 那兩項從通過變成失敗：以前它被丟掉，還沒載入的圖寬度是 0，kun 照 default object size 當成 300px，現在寬度是 10px（Chrome 是 300px，kun 的 layout 給還沒載入的圖 0 的寬度，記進 `docs/limitations.md`）。
- **RFC 做完時跑清單裡 `css/` 以外的目錄**：前幾段只跑了相關的目錄，這次找到它們讓別處通過、還沒更新的：`dom/nodes/` 的 `:lang()`、`:target`（`querySelector`、`matches` 的測試和 `query-target-in-load-event.html`）、`html/dom/reflection-embedded.html` 的 iframe 的 `width`、`height`、`intersection-observer/` 的 18 份（頁面用 `calc(100vh + 100px)` 這類的高度）、`select-required-attribute.tentative.html`（`:required`），更新預期結果。另外幾項預期外的通過和 CSS 無關、`main` 上就是這樣，沒有動：`FileAPI` 的 `FrozenArray<MessagePort>`、`url/` 的 structured serialize、`setHTMLUnsafe`、canvas 的 flat tree、`fragment-parse-form`、`selection/caret/after-designMode-off.html`、`canvas-descendants-focusability-004.tentative.html`。
- **RFC 0043 留下的**：第 5 段跑 `css/cssom/` 和 `css/css-variables/` 時，RFC 0043 第 1 段的 text property（已經在 `main` 上）讓 `serialize-values.html` 的 `letter-spacing` 這些、`variable-presentation-attribute.html` 和 `variable-substitution-basic.html` 的 `text-indent` 那一項通過，預期結果還沒更新，一起更新。

### 找到的問題

都先修好、補了測試。

- **自訂 property**（WPT）：值的前面一個 `var()` 失敗之後就停止代換，後面引用的自訂 property 沒走到、循環看不到（`variable-substitution-variable-declaration.html` 的 target5；改成失敗之後繼續看主要的名稱，但不再走 fallback，target6）；循環的範圍照 Blink 的 `CascadeResolver` 記開始和結束（原本只記開始，在循環之後才走到的也被當成在循環裡）；fallback 的最外層不能有 `!`、`;`；代換之後只剩一個 CSS-wide 關鍵字的照那個關鍵字；含 `var()` 的值去掉頭尾的註解；`cssText` 原本把 shorthand 寫在第一個 longhand 的位置，重新解析時被後面的 shorthand 蓋掉，改成最後一個（之後 inline style 改存宣告，回到 Chrome 的寫法）。
- **`calc()`**（WPT）：`sizes` 只有一個 `calc()` 時被當成不認得的 media condition；`vw` 讓頁面可以捲動之後，`position: fixed` 裡面的元素捲進畫面時連外面的 viewport 也捲了，改成 fixed 的容器捲完就停（`scrollIntoView-fixed.html`）。taffy 0.14 的 block layout 用父元素的高度解析子元素上下的 padding 和 border 的百分比，記成限制。
- **`@media`**：`MediaQueryList-addListener-handleEvent.html` 讓 renderer 當掉：分派 `change` 時沒有 script 在跑，`TypeError` 沒有位置，`dom_js` 算 column 時 `+ 1` 溢位，改用 `checked_add`；不穩定的 `adoptedstylesheets-cascade-order.html`（6 次裡失敗 3 次）：`getComputedStyle` 只更新 style，主 frame 在第一次 layout 之前還沒拿到視窗的大小，`FrameView::update_style_for` 改成先拿視窗的大小和 device scale factor，之後連跑 8 次都通過。`cargo test --workspace`：`shell` 的 `sites::gutenberg_contents_link_to_their_chapters` 點的座標因為存下來的頁面的 `@media screen` 生效（Chrome 也是）而改；`invalidation::other_adopted_contents_style_everything_again` 的 helper 每次做新的 `media`。
- **pseudo-class**：`mouseover` 的 listener 移除了目標，`mousemove` 照舊給被移除的節點，改成還在 document 裡的最近的祖先；只按修飾鍵、滑鼠沒動時，layout 之後的邊界事件帶舊的修飾鍵；非主鍵的按下也設 `:active`；換文件的種類時整個 DOM 換掉，renderer 記下的滑鼠下的元素還是舊 DOM 的 handle，`shell` 的測試 process 當掉（`DomIsolate dropped with 5 live root(s)`），換 DOM 和 commit 時丟掉它們（Blink 的 `EventHandler::Clear`）；language tag 的子標籤超過 8 個字元時不是合法的 tag（`fr-ninechars`）。`style` 的 `cascade::what_is_ignored` 用 `p:read-only` 當不支援的 pseudo-class，改用 `:in-range`。

### mutation test

`mutants-1.toml` 35 條、`mutants-2.toml` 28 條、`mutants-3.toml` 31 條、`mutants-4.toml` 83 條（4a 24、4b 59）、`mutants-5.toml` 21 條，最後全部抓到。第一次沒抓到的：

- **等價的**，從 spec 拿掉：「子元素不繼承自訂 property」（cascade 一定重新設定，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）；「代換失敗時不套用 `unset`」（程式因此簡化成直接不套用）；「長度乘長度」（後面的 `match` 本來就擋掉，`parse_product` 因此簡化）；「主 frame commit 時不丟掉滑鼠的狀態」（舊的 document 已經 discard，下一次滑鼠的更新什麼都不做，script 看不到）；「CSS-wide 關鍵字的 `copy` 不換成物理的 longhand」（`apply` 已經換過，拿掉 `copy` 裡多餘的那一行）。
- **補了測試的**：fallback 本身失敗時整個值 invalid；`calc((1 + 1) * 2px)`；renderer 只換 viewport 時重算（`about:demo` 的動畫每一幀都讓 style 變髒，蓋過去了）；只有 adopt 的 sheet 自己的 `media` 翻的情況；`about:demo` 的 `matchMedia` 筆記多了 `change` 的次數；離開的 frame 的 `:hover`；layout 之後重新判斷（headless 加了 `run_testharness_after_input`，只送移動、等 frame 之後的事件）；第一個事件就是按下的 shell 測試；`:lang("de-*-DE")` 和 `:lang("*")`；只有 `READ_WRITE` 變、只有 `:read-only` 規則；`contenteditable="false"` 的 SVG 元素裡的 HTML 元素；readonly 的 `<textarea>`；`border-block` 兩邊的顏色不同。
- **改掉實作的**：`<meta http-equiv>` 的名稱不 trim（見「對照 Chromium 的原始碼」）。
- **寫錯、照新的程式改寫的**：「`font-size` 不先套用」、「`ALL` 又漏了 `position`」（改了陣列的長度、編不過）、「viewport 和 device scale factor 太晚給 style」（抽出 `take_environment` 之後）、「只有 hover element 自己 hover」（抽出 `chain()` 之後）。

### 量測

`ci.py speedometer --trace --browser kun --iterations 3`，`perf` profile；renderer main thread 上 `Document::UpdateStyle` 的合計，用一個加總 trace 裡 span 的小腳本。第 1 段把 `main` 的 `db76aa3c` 放在 `git worktree` 裡當基準，量完拿掉；第 2 到 4 段沒有各自量，第 5 段做完時（`main` 已經併進 RFC 0042、0043）再量一次，沒有基準：

| Suite | `main` | 第 1 段，沿用舊 map 之前 | 第 1 段 | 第 5 段 |
| --- | --- | --- | --- | --- |
| TodoMVC-Svelte-Complex-DOM | 1212 ms（64 次） | 3992 ms | 1267 ms | 1187、1246、1193 ms |
| TodoMVC-Preact-Complex-DOM | 1219 ms（61 次） | 4068 ms | 1193 ms | 1458、1185、1184 ms |
| TodoMVC-React-Complex-DOM | 1300 ms（361 次） | 1268 ms | 1245 ms | 701 ms（64 次） |
| TodoMVC-jQuery | 1004 ms（995 次） | 1084 ms | 1056 ms | 27 ms（106 次） |

- 第 1 段第一次量到 Svelte 和 Preact 的 Complex-DOM 變成 3.3 倍。用 Complex-DOM 的頁面（`big-dom-generator/dist/index.html`，CSS 放進 `<style>`）在 `style` 的臨時測試裡整份重算（release，不 commit）：`main` 每次 65 ms；第 1 段第一次 77 ms，之後每次 866 ms。差在之後有舊的 style 要比較：新舊的自訂 property 內容一樣、各自建的，每個元素逐項比較根元素的約 1600 個。改成逐節點比較、共用的子樹跳過，重算時相等就沿用舊的 map 之後，每次 79 ms（多出的約 20% 是原本丟掉的 2399 個含 `var()` 的宣告實際的代換和解析），Speedometer 上回到和 `main` 一樣。
- 第 5 段：Complex-DOM 的兩個和第 1 段做完時差不多（Preact 第一次的 1458 ms 是雜訊，再量兩次都是 1185 ms），style 的時間沒有因為後面幾段失控。React 和 jQuery 的 `UpdateStyle` 次數少了很多（361 次變 64 次、995 次變 106 次），這段期間 `main` 併進了別的 RFC，和這份的改動比不出關係，沒有細查。

### 跑過的測試

每段做完時都跑了 `cargo test --workspace`、`cargo fmt --check`（`sdk/lua/generator` 也是）、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`，和那段相關的 `test.py wpt`、`test.py reftest`、`test.py scenarios`；第 3 段另外讓 `dom` 的 `incremental_stress` 換 8 個 seed。視窗那一層沒有改，沒有跑 `test.py automation`。最後一次（第 5 段做完時）：`cargo test --workspace --no-fail-fast` 43 個 binary、2589 個通過；`fmt`、clippy 沒有問題；`test.py wpt` 清單裡的全部（分批；更新上面那些預期結果之後，除了那幾項和 CSS 無關的，0 個預期外）；`test.py reftest`（155 個，0 個預期外）；`test.py scenarios demo-script`；Speedometer（見「量測」）。第 4 段 rebase 到 RFC 0043 的第 1、2a 段之後重跑過一次（沒有衝突，`css/CSS2/`、`css/css-inline/`、`css/css-text/` 也跑了）。第 5 段 rebase 到 RFC 0043 的 2b 段之後（沒有衝突）重跑 `fmt`、clippy、`cargo test --workspace --no-fail-fast`（43 個 binary、2592 個通過）、`test.py reftest`、`scenarios demo-script` 和 `wpt css/css-logical/`，都沒有問題。
