# RFC 0016 的進度：CSSOM（inline style 和 `getComputedStyle`）

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-07。完成：2026-10-12，Windows 11（第 1 段 2026-10-07 在 macOS Apple Silicon，第 2 段 2026-10-09 在 Fedora 44，第 3 段 2026-10-12 在 Windows 11；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0016` 上分三段做：`getComputedStyle` 原本是第 1 段的 1e，因為要等 D 拆成第 2 段（README 的第 9 條）；第 1、2 段做完、經過審核時 merge 回 `main`。動到 GC 而要跑的 `test.py matrix`、`fuzz`：第 2 段在 2d 跑過，第 3 段在 merge 之後看 CI。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a `style` 的 CSSOM 底層：有順序、帶 `!important` 的宣告清單，specified value 保留單位和關鍵字，序列化、shorthand 合回、自訂 property、名稱的對應 | 完成，merge 回 `main` | 2026-10-07 |
| 1b `element.style`：`CSSStyleDeclaration`、`CSSStyleProperties`、`InlineCssStyleDeclaration`、attribute 的同步和 `MutationObserver`；JS 和 wasm | 完成，merge 回 `main` | 2026-10-07 |
| 1c camelCase 的屬性：宣告表的新形式、`dom_js` 的 named setter | 完成，merge 回 `main` | 2026-10-07 |
| 1d WPT 和 reftest | 完成，merge 回 `main` | 2026-10-07 |
| 2a `style` 的 computed value：每個 property 的序列化、computable 的清單和順序 | 完成，merge 回 `main` | 2026-10-09 |
| 2b hook：`LayoutClient::update_style`、`BlockBox` 的 padding 和 margin | 完成，merge 回 `main` | 2026-10-09 |
| 2c `getComputedStyle`：`CssComputedStyleDeclaration`、`Window.getComputedStyle`、resolved value；JS 和 wasm | 完成，merge 回 `main` | 2026-10-09 |
| 2d WPT、情境、文件 | 完成，merge 回 `main` | 2026-10-09 |
| 3a `style` 的 stylesheet：公開的 `StyleSheetContents`、規則和選擇器的序列化、插入和刪除，`StyleEngine` 收 document 的 adopted、比指標 | 完成 | 2026-10-12 |
| 3a' 拆出 `css` crate：`style` 裡不依賴 `dom` 的解析、值、序列化搬到新的 `css` crate，行為不變 | 完成 | 2026-10-12 |
| 3b `CSSStyleSheet` 和規則：`StyleSheet`、`CSSStyleSheet`、`CSSRuleList`、`CSSRule`、`CSSStyleRule`、第三種 declaration，`replace`、`replaceSync`、`insertRule`、`deleteRule`；JS 和 wasm | 完成 | 2026-10-12 |
| 3c `adoptedStyleSheets`：`ObservableArray`（宣告表、`dom_js` 的 Proxy、WIT）、`Document` 和 `ShadowRoot` 的清單、標記重算；JS 和 wasm | 完成 | 2026-10-12 |
| 3d WPT、reftest、情境、mutation test、Speedometer 的盤點、文件 | 完成 | 2026-10-12 |

## 驗收

### `element.style`

| 項目 | 怎麼驗證 |
| --- | --- |
| 每種 specified value 讀回 Chrome 的寫法（單位、顏色、關鍵字、數字、grid、transform、`font-family` 的引號） | `cargo test -p style --test it cssom::`（`lengths_keep_their_units` 等） |
| shorthand 從 longhand 合回、合不回去時是空字串，`cssText` 的分組和順序 | `cargo test -p style --test it cssom::`（`shorthands_*`、`css_text_groups_longhands_into_shorthands`） |
| 解析時一個 property 留一個（`!important` 優先、normal 在前），`setProperty` / `removeProperty` 回報有沒有改變 | `cargo test -p style --test it cssom::`（`a_parsed_block_keeps_one_declaration_per_property`、`set_property_tells_whether_the_block_changed`、`remove_property_returns_the_old_value`） |
| 自訂 property 照原文保存、名稱分大小寫、不合法的值不收；cascade 不受影響 | `cargo test -p style --test it cssom:: custom_properties`、`cargo test -p style --test it cascade:: custom_properties` |
| camelCase 和 dashed 的名稱對應 | `cargo test -p style --test it cssom:: attribute_names` |
| cascade 的結果不變（單位計算時才換成 px、`opacity` 計算時才夾住、`self-start` / `self-end` 的 layout） | `cargo test -p style`、`cargo test -p layout --test it flex:: self_start` |
| `InlineCssStyleDeclaration` 每個元素一個、slot 撐住它和它的元素、只當自己的型別 unwrap、在工作負載和 GC stress 裡 | `cargo test -p dom --test it tree:: inline_styles`、`--test script_objects inline_style`、`--test script every_type_unwraps_only_as_itself`、`--test gc_stress` |
| `element.style` 的成員讀寫 `style` attribute，有改變才寫（`cssText` 每次寫） | `cargo test -p dom_api --test it api:: inline_style` |
| JS：`[SameObject]` 和 expando、`[PutForwards=cssText]`、`style[i]` 到 `length` 為止、`@@iterator`、`null` 是空字串、`MutationObserver` 的紀錄和舊值、wrapper 在元素的 realm | `cargo test -p dom_js --test it bindings:: inline_style`、`--test realms` |
| wasm：`kun:dom` 的 `style` 和 `css-style-properties` | `cargo test -p dom_wasm --test it engine:: inline_style`（guest 是 `crates/dom_wasm/tests/guests/style.c`） |
| camelCase 和 dashed 的屬性讀寫、`in`、屬性描述（可寫、可列舉）、`Object.keys`、不支援的名稱和大寫混 `-` 的照一般的屬性、不在 prototype 上、`null` 移除、有改變才有紀錄；集合的 named property 照舊唯讀、不可列舉 | `cargo test -p dom_js --test it bindings:: inline_style`、`html_collections` |
| 宣告表的沒有名字的特殊操作（getter、setter、supported names），不在 WIT 裡；camelCase 的名稱清單 | `cargo test -p dom_api --test it api::`、`--test wit`；`cargo test -p style --test it cssom:: attribute_names` |
| WPT 的 `css/cssom/` 的 inline style（38 個檔案）和支援的 property 的 `css/*/parsing/`（164 個）照 baseline；reftest `css-flexbox/flexbox-dyn-resize-001` | `python3 scripts/test.py wpt`、`python3 scripts/test.py reftest wpt/css/css-flexbox/flexbox-dyn` |
| parser 不收 `span` 夾在 grid line 的數字和名稱中間、沒加引號的 `font-family` 名稱裡的關鍵字 | `cargo test -p style --lib grid`、`cargo test -p style --test it cssom:: invalid_values_are_not_set` |
| 視窗裡：`about:demo` 的「element.style」按鈕用 `style.backgroundColor` 和 `setProperty` 換方塊的顏色和寬度、顯示寫回的 `style` attribute，再按一次清掉 | `python3 scripts/test.py scenarios demo-script` |

### `getComputedStyle`

| 項目 | 怎麼驗證 |
| --- | --- |
| 每個 longhand 都寫得出 computed value（加了 property 沒補序列化就失敗）、清單是照名稱排序的 longhand、property 的名稱不分大小寫 | `cargo test -p style --test it computed::` |
| 長度絕對化、百分比留著、`currentcolor` 解析成元素的 `color`、alpha 用最短能還原的寫法、`line-height` 的數字乘上 font size、關鍵字和 grid 的 track list | `cargo test -p style --test it computed::` |
| 只更新 style 的那一級更新了這個 frame 和它的祖先、什麼都沒 layout | `cargo test -p renderer --lib a_style_update_lays_nothing_out` |
| 盒子帶著 used 的 padding 和 margin（百分比和 `auto` 都由 layout 決定） | `cargo test -p paint --test it geometry:: boxes_carry_their_used_padding_and_margins` |
| `getComputedStyle` 每次回新的 declaration、活的（元素的 `style` 改了下一次讀就變）、不看盒子的 property 只更新 style 不 layout | `cargo test -p dom_api --test it computed_style::`（`a_computed_style_is_live`） |
| resolved value 從盒子來（`width` / `height` 是 content box，`padding-*` / `margin-*` 是 used value，名稱不分大小寫）、inline 元素和沒有盒子時退回 computed value | `cargo test -p dom_api --test it computed_style::`（`resolved_values_come_from_the_box`、`without_a_box_the_computed_value_stands`） |
| `transform` 的 resolved value 是 matrix（百分比對邊框盒、函式的合成順序），沒有盒子時是 `none`；和 paint 共用同一份合成 | `cargo test -p dom_api --test it computed_style:: transform`、`cargo test -p paint --test it geometry:: transforms_are_about_the_center` |
| 唯讀（`setProperty`、`removeProperty`、`cssText`、具名 setter 丟 `NoModificationAllowedError`），不是 property 的名稱照一般的 JS 屬性；`cssText` 和 priority 是空字串、清單是 computable 的那些 | `cargo test -p dom_api --test it computed_style::`（`it_is_read_only`、`it_lists_the_computable_properties`）、`cargo test -p dom_js --test it computed_style::` |
| pseudo-element 的 declaration 沒有 style（每個值是空字串、`length` 是 0），不合法的選擇器丟 `SyntaxError`，空字串和 `null` 當沒給 | `cargo test -p dom_api --test it computed_style:: a_pseudo_element`、`cargo test -p dom_js --test it computed_style:: the_pseudo_element` |
| JS：`CSSStyleProperties` 的實例、camelCase 的屬性、`length` / `item` / 索引 / 迭代、expando、`TypeError` | `cargo test -p dom_js --test it computed_style::` |
| wasm：`kun:dom` 的 `get-computed-style`（唯讀的錯誤碼、pseudo-element、resolved 的 `width`） | `cargo test -p dom_wasm --test it engine:: a_guest_uses_computed_style`（guest 是 `crates/dom_wasm/tests/guests/computed_style.c`） |
| shorthand 從 computed 的 longhand 合回（`margin` 的 `auto` 是 layout 算出來的值），合不回去是空字串 | `cargo test -p style --test it computed:: shorthands`、`cargo test -p dom_api --test it computed_style:: resolved_values` |
| 不在 render 的樹上的元素沒有 style：`length` 是 0、每個值是空字串 | `cargo test -p dom_api --test it computed_style:: an_element_out_of_the_tree` |
| WPT 的 `css/cssom/` 的 `getComputedStyle` 和 `computed-style`（20 個檔案）照 baseline | `python3 scripts/test.py wpt css/cssom` |
| 視窗裡：`about:demo` 的「getComputedStyle」按鈕讀綠方塊的 resolved value（`element.style` 寫 `4em`，讀到 `64px`），顏色是 `rgb()` | `python3 scripts/test.py scenarios demo-script`、`python3 scripts/test.py automation demo-script` |

### constructed stylesheet 和 `adoptedStyleSheets`

| 項目 | 怎麼驗證 |
| --- | --- |
| `StyleSheetContents` 只留 style rule（at-rule、解析不了的選擇器丟掉）；規則的 `selectorText`、`cssText` 照 Chrome 的寫法；`selectorText` 換不合法的不換、規則換宣告不動舊的 | `cargo test -p style --test it stylesheet::` |
| `insertRule` 先檢查 index（`IndexSize`）再解析正好一條規則（`Syntax`：空的、兩條、後面有東西、at-rule）；`deleteRule` 要有那一條；換一條規則照 `Arc` 找；改出來的內容共用沒動的規則、留著 base URL | `cargo test -p style --test it stylesheet::` |
| adopted 的內容排在 document 自己的之後、照清單的順序，specificity 和 `!important` 照舊，`url()` 照它的 base URL（沒有時照 document 的） | `cargo test -p style --test it cascade:: adopted_sheets_cascade_after_the_documents_own` |
| 同一份清單不重算；別的 `Arc`（同樣的原文也是）、多一份、少一份、換順序、編輯過的都整份重算；只在 adopted 裡的結構選擇器也讓兄弟重算 | `cargo test -p style --test it invalidation:: adopted` |
| `style` 裡不依賴 `dom` 的解析、值、序列化在 `crates/css`，`style` 照舊 re-export 公開的 API，行為不變：所有測試照舊通過，單元測試的數目不變（`css` 16 個加 `style` 5 個，搬之前 `style` 21 個），3a 的 mutation test 照樣全部抓到 | `cargo test --workspace`、`cargo test -p css`、`python3 scripts/test.py mutate rfcs/0016-cssom/mutants-3a.toml` |
| `new CSSStyleSheet()` 是空的、constructor document 是 running realm 的 document，`baseURL` 對它的 base URL 解析（不合法是 `NotAllowedError`），沒有 realm 時不建；`StyleSheet` 的成員照 constructed 的值 | `cargo test -p dom_api --test it css_style_sheet::`（`a_constructed_sheet_is_empty_and_of_the_running_realm`、`base_url_parses_against_the_documents`、`no_realm_no_sheet`） |
| `cssRules` 是 live 的同一個 list，規則的物件每次同一個；`insertRule`、`deleteRule` 把後面的物件跟著搬、回傳 index、錯誤照 3a；`replaceSync` 把每個物件拿掉（`parentStyleSheet` 是 null、照樣讀得到 `cssText`） | `cargo test -p dom_api --test it css_style_sheet::`（`css_rules_is_live_and_its_rules_are_the_same_objects`、`insert_rule_and_delete_rule_move_the_rules_objects`、`replace_sync_takes_every_rule_out`） |
| `selectorText` 解析不了時不變；規則的 `style` 改了就換掉規則（stylesheet 的內容跟著換），`parentRule` 是那條規則，`element.style` 和 `getComputedStyle` 的是 null | `cargo test -p dom_api --test it css_style_sheet::`（`selector_text_changes_the_rule_if_it_parses`、`a_rules_style_changes_the_rule`、`other_declarations_have_no_parent_rule`） |
| 四個新的 GC 型別：只當自己的型別 unwrap、互相撐住（stylesheet 和規則的物件、規則和 `style`），在工作負載（每輪 `insertRule`、`deleteRule`、`replaceSync`、`selectorText`，比對每個物件在它的規則的位置）和 GC stress 裡 | `cargo test -p dom --test it script_objects:: css_`、`script_objects:: style_rule_declaration`、`script:: every_type_unwraps_only_as_itself`、`cargo test -p dom --test it`（`detached_marking::`、`standalone_heap::` 跑工作負載） |
| JS：建構子和 options、`cssRules` 和 `[SameObject]`、編輯規則、`replace` 的 promise resolve 成 stylesheet（拿掉的物件留著自己的規則）；原型鏈（`CSSStyleRule` 繼承 `CSSRule`）和宣告表對照 IDL | `cargo test -p dom_js --test it style_sheets::`、`cargo test -p dom_api --test it api::`、`--test it wit::` |
| wasm：`kun:dom` 的 `css-style-sheet`（options 的 record、`replace-sync`、`insert-rule` 和 `IndexSizeError` 的錯誤碼、`css-rule` 轉成 `css-style-rule`、`selector-text` 和 `style`），`replace` 是 `async func` | `cargo test -p dom_wasm --test it engine:: a_guest_constructs_a_style_sheet`（guest 是 `crates/dom_wasm/tests/guests/stylesheet.c`）、`cargo test -p dom_api --test it wit::` |
| document 和 shadow root 的清單（加在最後、換掉、拿掉最後一個，可以重複），由它撐住、拿掉的經過 GC 被回收，在工作負載和 GC stress 裡；set 的演算法只收 node document 建的（`NotAllowedError`，清單不變）；setter 先全部刪掉再逐個 set，停在被拒絕的那一個 | `cargo test -p dom --test it style_sheets::`、`cargo test -p dom --test it`（工作負載）、`cargo test -p dom_api --test it css_style_sheet::`（`adopted_style_sheets_are_the_documents_sheets`、`the_setter_replaces_the_list`、`a_shadow_root_adopts_its_documents_sheets`） |
| adopt 的清單、一份 stylesheet 的內容（`replaceSync`、`insertRule`、`deleteRule`、`selectorText`、規則的 `style`）和 `disabled` 改了，標記 constructor document 的 style 要重算；拿掉的規則改了不標記 | `cargo test -p dom_api --test it css_style_sheet:: changes_to_sheets_mark_the_documents_style` |
| 完整的瀏覽器裡 adopt 的 stylesheet 排在 document 自己的之後，改規則、`disabled`、換順序、`push`、`length = 0` 之後下一次讀 `getComputedStyle` 和盒子就變；shadow root adopt 的不影響 document | `cargo test -p shell --test it style_sheets::` |
| JS：`adoptedStyleSheets` 每次同一個、是 `Array`，`push`、`splice`、`[i] =`、`length =`、`delete` 和八個 trap 照 WebIDL 的 observable array exotic object（只能刪最後一個、超過長度回 `false`、`length` 不是整數是 `RangeError`、`defineProperty` 的每個檢查、不是 index 的 key 是 target 的、不能變成不可擴充），經過 GC 留著 expando；setter 收 iterable、轉型失敗時不變 | `cargo test -p dom_js --test it style_sheets::`（`adopted_style_sheets_is_an_observable_array`、`adopted_style_sheets_takes_sheets_of_its_document`） |
| 宣告表的 `ObservableArray<T>`：WIT 的 `adopted-style-sheets` 和 `set-adopted-style-sheets`（整份 list），`spec-missing.txt` 拿掉兩個 `adoptedStyleSheets`；wasm 的 guest adopt 兩次再讀回來 | `cargo test -p dom_api --test it wit::`、`--test it spec::`、`cargo test -p dom_wasm --test it engine:: a_guest_constructs_a_style_sheet` |
| shadow root 被 adopt 到別的 document 時清空 `adoptedStyleSheets`、放掉那些 stylesheet；搬進、搬出 `<template>` 的內容時留著 | `cargo test -p dom --test it style_sheets::`（`a_shadow_root_keeps_its_sheets_only_through_templates`、`a_shadow_root_in_another_document_lets_go_of_its_sheets`） |
| WPT 的 `css/cssom/` 的 constructed stylesheet 和 `adoptedStyleSheets`（18 個檔案）照 baseline；`adoptedstylesheets-observablearray.html` 裡跑不到的兩項（不經過 `Array.prototype` 的 setter、`defineProperty` 轉換 descriptor 時重入）在 `dom_js` 裡 | `python3 scripts/test.py wpt css/cssom`、`cargo test -p dom_js --test it style_sheets:: adopted_style_sheets_as_wpt_checks_them` |
| reftest：換掉 adopted 的內容、再 `push` 一份，兩個方塊都重畫 | `python3 scripts/test.py reftest invalidation/adopted-style-sheets.html` |
| 視窗裡：`about:demo` 的「adoptedStyleSheets」按鈕：adopt 一份 stylesheet 把方塊塗藍、`replaceSync` 換成黃色而且變寬、拿掉之後回到綠色 | `python3 scripts/test.py scenarios demo-script`、`python3 scripts/test.py automation demo-script` |
| Speedometer 的 TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM 在 kun 上跑完 | `python3 scripts/ci.py speedometer --browser kun --suites TodoMVC-WebComponents,TodoMVC-Lit-Complex-DOM --iterations 1 --rounds 1` |

## 做過的檢查

三段做完時都跑過 `cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`docs`（第 3 段也跑 `tests`）：第 1 段的 1a 到 1d 每一步都全部通過；第 2 段 1462 個（rebase 到 RFC 0022 之後 1529 個）；第 3 段 2436 個。第 2、3 段照之後的約定，只在段落做完時跑整個 workspace。

- **做了什麼**：
  - 第 1 段：1a `style` 的 CSSOM 底層：`CssPropertyValueSet`（有順序、帶 `!important`、implicit 和「哪個 shorthand 設的」），specified value 保留單位和關鍵字，每種值照 Chrome 序列化、shorthand 合回、自訂 property 照原文保存、camelCase 和 dashed 名稱的對應；1b `InlineCssStyleDeclaration` 和 `element.style`，每個成員解析 `style` attribute、有改變才序列化寫回；1c 宣告表的沒有名字的特殊操作和 `dom_js` 的 named interceptor（第 8 條，RFC 0017 照用）；1d WPT 和 reftest。
  - 第 2 段：2a `style` 的 computed value（`computable_properties`、`computed_value`）；2b `LayoutClient::update_style` 和 `BlockBox` 的 used padding、margin（第 10 條）；2c `CssComputedStyleDeclaration`、`Window.getComputedStyle`、resolved value（第 11、12 條），`transform` 的 matrix 和 paint 共用一份合成（`ComputedStyle::transform_matrix`）；2d WPT、情境、computed 的 shorthand（第 15 條）。
  - 第 3 段：3a `StyleSheetContents` 和 `StyleEngine::set_adopted_style_sheets`；3a' 拆出 `css` crate（第 18 條，行為不變）；3b `CSSStyleSheet` 和規則的物件（四個 GC 型別，`css` 的型別 `NoTrace`，`CSSStyleRule` 繼承 `CSSRule`，第 21 條）；3c `adoptedStyleSheets` 是 `ObservableArray`（宣告表的新形式、`dom_api` 的 `observable_array`、`dom_js` 的 Proxy、WIT 的整份 list，第 22 到 24 條），stylesheet 的每個修改標記它的 constructor document，renderer 在重算前把 adopted 的內容交給 `StyleEngine`；3d WPT、reftest、`about:demo`，shadow root 換 document 時清空它的清單。
- **設計在實作中的改變**（理由在 README 的對應條目）：`getComputedStyle` 從 1e 拆成第 2 段（第 9 條）；`css/*/parsing/` 的 `-computed` 跑過之後不收（第 14 條）；3a 原本要在 `recalc_style` 多一個參數，改成 `set_adopted_style_sheets`（`recalc_style` 有 56 個呼叫的地方）；解析好的規則放在哪從 C2 改成 C3（3b 開始時發現 C2 違反 `soundness.md` 的 `NoTrace` 規則），多了 3a' 的搬家；清單從寫入時複製改成就地改（第 23 條，3b 的規則的物件已經是就地搬）；`ObservableArray` 的三個函式是 `list`、`set`、`delete`，長度從 `list` 來。
- **找到的 bug**：`grid-template: none / none` 原本不合法（1a）；grid line 的 `span` 夾在數字和名稱中間、沒加引號的 `font-family` 名稱裡的關鍵字原本被接受（1d，WPT 的 `-invalid`）；不在 render 的樹上的元素，`getComputedStyle` 的 `length` 原本是 67（2d，WPT 的 `getComputedStyle-detached-subtree.html`，CSSOM 和 Chrome 是 0）；shadow root 換 document 時沒清空 `adoptedStyleSheets`（3d，WPT 的 `CSSStyleSheet-template-adoption.html`，照 Blink 的 `TreeScopeAdopter::MoveShadowTreeToNewDocument` 補）。和這份無關、順手改正的：`demo-script` 情境少了 RFC 0036 第 1 段（`53297468`）加的「More padding」按鈕。
- **寫測試時的錯**：computed 的百分比多乘 100、`"Width"` 其實要讀得到、alpha 128 的最短寫法是 `0.5`（2a）；`grid-area`、`flex-flow` 的 computed shorthand 預期寫成長格式（2d）；`dom_js` 的測試裡沒寫 `return` 的多個敘述被 harness 包成語法錯誤、`length` 的 descriptor 預期寫成不可寫（3c）；回收的測試把另一份 document 的 handle 也丟了，清不清空都通過（3d）。寫測試時發現的實作錯：`used_value` 沒把名稱轉小寫、具名 setter 先丟例外再看名稱（2c）。
- **mutation test**（跑的期間沒有 commit，跑完檔案都還原了）：
  - 第 1 段：1a 58 條、1b 10 條、1c 10 條、1d 5 條；第一輪沒抓到、補了測試的：cascade 不分 importance、`grid-template` 的 rows 有 `repeat()`、`translate(x)` 的 y（1a），「有 setter 才可寫」（1c，`html_collections`）。
  - 第 2 段：2a 6 條、2b 4 條（`FrameTreeLayout::update_style` 空實作在 2b 沒有人呼叫，2c 的測試守住）、2c 19 條（「函式的合成順序反過來」原本只有 `dom_api` 抓到，`paint` 補了兩個函式的例子）、2d 7 條。
  - 第 3 段：`mutants-3a.toml` 13 條（3a' 搬家之後照樣全部抓到）、`3b` 21 條、`3c` 27 條、`3d` 4 條，全部抓到；漏網之後補了測試的：拿掉最後一個 adopted 的 stylesheet 時留在 slot 裡（3c）、清空時只把長度歸零（3d），都補了回收的測試。
  - 等價的（記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」，從規格拿掉）：`cssText` 每個 shorthand 只試一次、cascade 跳過沒有 `!important` 的區塊的 important 那一輪（1a）；`shorthand_longhands` 的順序（2d）；`selectorText` 的 setter 不要求解析到結尾（3a）；observable array 的 `preventExtensions` 回 `true`（3c）。
  - 3b 第一次跑測試時另一個 session 同時在跑同一份規格，測試編到套用中的 mutant，看起來像不穩定；等它跑完、確認每條的原文都在之後全部通過。
- **WPT**（收錄的檔案在 `tests/wpt/include.txt` 的三段註解下，baseline 在 `metadata/`）：
  - inline style（1d）：`css/cssom/` 38 個和 `css/*/parsing/` 164 個，103 個全過；失敗的是 kun 不支援的值和 property、`CSS.supports`。1b、1c 讓 15 個既有的 baseline 往前推進或全過；`dom/events/EventListener-invoke-legacy.html` 因為 `style.animation` 不再丟例外而等到逾時（kun 沒有動畫）。
  - `getComputedStyle`（2d）：`css/cssom/` 20 個，7 個全過；`css/*/parsing/` 的 `-computed` 跑過一輪（1190 個失敗的子測試沒有一個比對到 computed value）之後不收；19 個既有的 baseline 改了、3 個刪掉。
  - constructed stylesheet（3d）：`css/cssom/` 18 個，6 個全過，`CSSStyleSheet-constructable-concat` 是 `disabled`（具名屬性先丟例外、等到逾時）；失敗的大多是 style 不進 shadow tree、`@media`、window 的具名屬性。`details.md` 列的 `CSSRuleList.html`、`CSSStyleRule.html` 等 9 個跑了才知道整份卡在 `document.styleSheets`，沒收。3b 讓 `computed-style-set-property` 的 `parentRule` 和 `style-cssText-mutations` 通過。3d 時 `css/cssom`、`shadow-dom`、`dom/nodes` 重跑：19455 個檢查，0 個不符。
  - `adoptedstylesheets-observablearray.html` 整份跑不了（`<style>.sheet`、具名屬性），它最後兩項移植進 `dom_js` 的 `adopted_style_sheets_as_wpt_checks_them`。
- **reftest**：1d 的候選腳本放行 `style` 之後多 1 個（`css-flexbox/flexbox-dyn-resize-001`）；2d 放行 `getComputedStyle` 之後沒有新的；3d 的 `invalidation/adopted-style-sheets.html`。`test.py reftest` 在 1a（504 個通過、43 個照預期失敗）、1d（505 / 43）、2d（542 / 67）、3a（kun 自己的 147 個）、3d（`invalidation/` 24 個）都沒有意外的。
- **視窗裡**：`about:demo` 的「element.style」（1b）、「getComputedStyle」（2d）、「adoptedStyleSheets」（3d）按鈕，都在 `demo-script` 情境裡；`test.py automation demo-script` 在 2d（Fedora 44，22/22）和 3d（Windows 11）的真視窗通過，截圖看過。
- **Speedometer**：第 3 段之後 TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM 在 kun 上跑完，不再卡在 `CSSStyleSheet`（3d）。Chrome 那一側照 RFC 0012 的決定不在 Windows 上跑（這次的 `--inventory` 兩次都 `no report in 600 s`）；量測工具在 Chrome 存下的結果沒有那個 suite 時丟 `KeyError`，記在 RFC 0012 的 `progress.md`。
- **guest 的重編**：WIT 改了的 1b、2c、3b、3c 都用 `ci.py guests` 重編（Windows 上要設 `WIT_BINDGEN`）；C 的 guest 和原本的一模一樣，Rust 的 guest 和 `resources/kun-lua.wasm` 跟著 WIT 的雜湊換。
- **GC**：第 2 段加了 `CssComputedStyleDeclaration`，`matrix` 8 個設定和 `fuzz` 20 個設定在 2d 跑過（Fedora 44，seed `0x066077a5`）；第 3 段加了四個 GC 型別、rare data 的清單、`adopt` 裡的清空，照約定在 merge 之後看 CI 的 `matrix`、`fuzz`。
- **rebase**：第 2 段 merge 前 rebase 到 RFC 0022 之後，處理文字（`spec-missing.txt`、`include.txt`、`tests/wpt/README.md`）、binary（WIT，重編 guest）和語意（RFC 0022 的新測試的 `LayoutClient` 要補 2b 的方法）三種衝突，重新驗收。
- **第 3 段的 rebase**（2026-10-12，Windows 11）：merge 前 `origin/main` 多了 RFC 0038（`Selection`、`Range` 的幾何）和 RFC 0039 的草稿。衝突：`ObjectTag` 的數目和 handle 的 import（`Selection` 加上這份的四個，52 → 56 種，`soundness.md` 的 I14 跟著改）、`spec-missing.txt`（它加了 `Selection` 的兩個成員）、`docs/testing.md` 的等價清單（兩邊各加幾項，都留）、`docs/architecture.md` 的 `dom`、`layout` 那兩列（各自改的部分都留）、RFC 0012 的 `progress.md`（兩邊各加一段）；binary 的照 `main` 再用 `ci.py guests` 重編（WIT 現在有兩份 RFC 的成員，guest 幾乎全部換）。`main` 沒有動到搬進 `css` 的檔案。重跑：`cargo fmt --check`、clippy（沒有警告）、`cargo test --workspace` 2464 個全部通過、`sanity.py` 五項、`test.py wpt css/cssom shadow-dom dom/nodes`（19455 個檢查，0 個不符）、`reftest invalidation/`（24 個）、`scenarios demo-script`。
- **Windows 上的工具**：用 heredoc 寫的 Rust 字串續行被 Bash 工具弄壞成一串空白（3c，改正）；`--reuse-chrome` 時 `target/speedometer-results/` 已經被另一次量測換掉、沒有 `chrome.json`（3d）。
