# RFC 0050 的進度：表單控制項的繪製

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-16。完成：2026-10-17，Windows 11（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0050` 上做，每段做完時先 merge 回 `main`（由人審核）；第 2 段 rebase 到 RFC 0051 第 3 階段之後的 `main`（只有 `paint` 的模組清單衝突）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a style：system color、`appearance`、system font、UA stylesheet 的控制項規則、`IsControlStyled` | 完成 | 2026-10-16 |
| 1b layout：checkbox、radio、range 的大小，文字框和 `<textarea>` 的 intrinsic size、`<button>` 的內容置中、baseline | 完成 | 2026-10-16 |
| 1c 和 Chrome 實測對照，更新受影響的 reftest、WPT 的預期和情境、`about:demo` | 完成 | 2026-10-16 |
| 2a 文字控制項的 UA shadow tree、值、placeholder、`::placeholder` | 完成 | 2026-10-16 |
| 2b `<input>` 按鈕的文字、search 和 number 的部件（`::-webkit-*`） | 完成 | 2026-10-16 |
| 2c paint 的 theme（Windows、Linux 的那一套；macOS 在 Mac 上補） | 完成 | 2026-10-16 |
| 3a 收起來的選單：UA shadow tree、大小、外框和箭頭 | 完成 | 2026-10-16 |
| 3b listbox：版面、選取的顏色、`<optgroup>`、`<hr>` | 完成 | 2026-10-16 |

## 驗收

### system color、system font 和 `appearance`

| 項目 | 怎麼驗證 |
| --- | --- |

| system color：每個關鍵字（連同已經不用的和 `-webkit-link`）的宣告值讀回關鍵字，computed 是平台的表的顏色（已經不用的照 CSS Color 4 對到新的），`accentcolor`、`mark`、`inactivecaptiontext` 等是固定的；沒有表時是 Blink 的預設 | `cargo test -p css --lib values::`、`cargo test -p style --test it cascade::system_colors_are_the_engines cssom::system_colors_keep_their_keywords` |
| system color 從 Browser 到 renderer：`RendererFlags` 的表是頁面的 style 和 canvas 的顏色；canvas 的 `fillStyle`、漸層的 stop 和 `currentcolor` 照 frame 的表；Windows 的 registry 的八個系統色照 Chromium 的對應，缺的、格式不對的是預設；用測試字型的視窗用預設的表 | `cargo test -p browser --test it the_renderer_has_the_browsers_system_colors`、`cargo test -p dom_api --test it canvas_styles::system_colors_are_the_frames`、`cargo test -p shell --lib system_colors` |
| `appearance`：Chrome 152 的 14 個關鍵字，舊的不收；`-webkit-appearance` 是別名（讀回 `appearance`），CSSOM 的 `webkitAppearance`、`WebkitAppearance` | `cargo test -p style --test it cssom::appearance_and_its_alias cssom::attribute_names_map_to_properties` |
| system font：`font` 收 CSS 2 的六個和 `-webkit-*-control`，讀回關鍵字、longhand 讀回空字串，另一個 longhand 改了之後 `font` 讀不出來；computed 是 `system-ui` 和 Chrome 的大小，style、weight、line height 重設 | `cargo test -p style --test it cssom::system_fonts cascade::system_fonts_compute_to_the_system_ui_font` |

### UA stylesheet 和 effective appearance

| 項目 | 怎麼驗證 |
| --- | --- |

| UA stylesheet 的控制項規則：每種控制項的 computed value 照 Chrome 152 實測（字型、顏色、邊框、padding、margin、`box-sizing`、`white-space`……），disabled 的淡色，跟著平台的 system color（文字框的邊框是固定的灰） | `cargo test -p style --test it form_controls::` |
| checkbox、radio 的 `auto` 大小是 13px（`appearance: none` 不調整）；`appearance` 不是 `none` 的元素（控制項，和 `appearance: auto` 的 `<span>`）不是 table 的部分、不是 `inline`：`table`、`list-item` 是 `block`，`inline`、`inline-table`、table 內部的值是 `inline-block`，作者的邊框不影響，`appearance: none` 的照它自己的 | `cargo test -p style --test it form_controls::check_boxes_and_radio_buttons_are_the_themes_size form_controls::what_has_an_appearance_is_no_table_part` |
| effective appearance：每種控制項的原生外觀（`<input>` 的 type 不分大小寫、HTML 沒有的 type 是文字框、`<select>` 的 `multiple` 和 `size`），舊的關鍵字是 `auto`，search 上的 `textfield`、select 上的 `menulist-button`、`base-select`；作者的背景、邊框、圓角讓按鈕、文字框照 CSS 畫，select 留著箭頭，checkbox、radio、range 不變；pseudo-element 沒有 | `cargo test -p style --test it form_controls::` |
| UA stylesheet 的其他：hidden 的 `<input>` 是 `appearance: none`，`<mark>` 是 `Mark` 和 `MarkText` | `cargo test -p style --test it form_controls::the_ua_rules_give_controls_chromes_style cascade::user_agent_text_rules` |

### 大小、baseline 和 button layout

| 項目 | 怎麼驗證 |
| --- | --- |

| 控制項的大小（Chrome 152 用同一份 Lato 實測）：文字框照 `size`（0 和寫錯的是預設、前面的空白和後面的字不算）、字級（16px、10.4px、20px）和 type，`<textarea>` 照 `cols`、`rows`、留捲軸的 15px、它的子節點不排，range 129×16（`appearance: none` 也是）、checkbox、radio 13px；在 block 裡 `width: auto` 不撐滿、百分比的寬度不改高度，flex item 的最小大小是它自己的（`<textarea>` 是 0），grid 和 flex column 照樣 stretch；`display: inline` 的控制項是 atomic inline | `cargo test -p layout --test it form_controls::` |
| baseline：文字框和按鈕是它的文字的（固定高度時行在中間，放不下時照樣置中），checkbox、radio、range 是 border box 的底，`appearance: none` 的 checkbox 是空的 inline-block 的，空的按鈕是 content box 的頂，`<textarea>` 是它的底；flex 的 `align-items: baseline` 照文字框的 | `cargo test -p layout --test it form_controls::controls_baselines form_controls::a_flex_container_aligns_a_text_field_by_its_baseline` |
| 按鈕的內容（行或 block 的子盒子，當成一塊）在 content box 裡垂直置中、放不下時在頂端，`display: block` 也是，`flex` 的照 flex | `cargo test -p layout --test it form_controls::a_buttons_content_is_centered_in_it` |
| 按鈕（`<button>`、`<input type=submit>`）在 block 裡 `width: auto` 是 fit-content（`display` 是 `block`、`flex`、`grid` 都是，放不下時是 min-content，百分比照舊），在 flex column、grid 裡 stretch，絕對定位的照 inset；它是 flow root：避開旁邊的 float、包住自己的 | `cargo test -p layout --test it form_controls::a_button_in_a_block_is_fit_content_and_a_flow_root` |
| `::before`、`::after`：checkbox、radio、color、file、date 的 `<input>` 和 `<button>` 有（在它的盒子裡），按鈕類、image 的 `<input>`、`<select>` 沒有（Chrome 152 實測） | `cargo test -p layout --test it form_controls::which_controls_render_their_pseudo_elements` |
| DOM 改了之後：`size`、`type`、`cols`、字型改了重建盒子，`<textarea>` 的文字改了照樣不排，按鈕的內容改了照樣置中，block 的按鈕 restyle 之後照樣是 fit-content，都和從頭建的一樣 | `cargo test -p layout --test it update::form_controls_follow_the_dom` |

### UA shadow tree、值和 placeholder、部件

| 項目 | 怎麼驗證 |
| --- | --- |

| UA shadow tree：插進有 window 的 document 之後才建（沒插進去的、沒有 window 的 document 的沒有，`ensure_shadow_trees` 照樣建），inner editor 是值（照 type 淨化過），placeholder 只在有 attribute 時有、值是空的時才顯示（文字框的拿掉換行，`<textarea>` 的留著），值、placeholder、type 改了跟著改，不是文字框的 type 拿掉樹的子節點，`shadowRoot` 是 `null`；UA 的 mode 和 closed 的一樣 | `cargo test -p dom_api --test it forms::a_text_control`、`cargo test -p dom --test it flat_tree::a_user_agent_shadow_root_is_closed` |
| `::placeholder`（兩個名稱，`::slotted()` 之後也收）：UA 和作者的規則照 host 比對到 UA shadow tree 裡的 placeholder 元素，Chrome 152 的 computed value（顏色、`overflow`、`white-space`、`<textarea>` 的換行），繼承 host；open 的 shadow tree 裡的不算；host 重算時它跟著重算；password 是 `-webkit-text-security: disc !important`；`getComputedStyle` 列 property 時 `-webkit-` 的排最後 | `cargo test -p style --test it form_controls::the_placeholder_takes_its_controls_placeholder_rules form_controls::only_a_ua_shadow_trees_element_is_a_placeholder form_controls::the_placeholder_follows_its_control stylesheet::placeholder_selectors_read_back_as_written cssom::text_security computed::the_list_is_the_longhands_in_name_order`、`cargo test -p dom_js --test it computed_style::placeholder_is_the_ua_shadow_trees_element` |
| 值和 placeholder 排在控制項自己的大小裡（inline、block、flex、grid 都是），不撐開它：文字框的行在中間（放不下時照樣在中間），長的值超出 content box、被 inner editor 切掉，有值時 placeholder 不排，`<textarea>` 的子節點不排、它的 shadow tree 的排；password 的字是圓點；flex 的 baseline 照控制項的（高的文字框的行在中間）；值、placeholder、type 改了之後和從頭建的一樣 | `cargo test -p layout --test it form_controls::a_text_controls_value_is_laid_out_in_its_size form_controls::a_text_fields_inner_editor_clips_its_value form_controls::a_flex_container_aligns_a_text_field_by_its_baseline form_controls::controls_are_sized_as_replaced_elements update::text_controls_follow_their_ua_shadow_trees` |
| 滑鼠按在 shadow tree 裡（inner editor）時，焦點往上經過 host 找 | `cargo test -p dom_api --test it focus::a_press_of_the_mouse` |
| 按鈕類的 `<input>` 的文字：UA shadow tree 裡的一個文字節點，是 `value`（換行留著），沒有時是 type 的預設（`Submit`、`Reset`，英文），空的是一個零寬的空白（有一行）；排在按鈕的中間，`display` 是 `flex`、`grid` 時照樣（文字當成 block），`<button>` 的內容照 flex、grid | `cargo test -p dom_api --test it forms::a_text_controls_ua_shadow_tree_shows_its_value_and_placeholder`、`cargo test -p layout --test it form_controls::an_input_buttons_label_and_a_fields_part form_controls::a_button_in_a_block_is_fit_content_and_a_flow_root` |
| search 和 number 的部件：editing view port 和清除鈕、上下鈕在 decoration container 裡（flex），值是空的時清除鈕 `visibility: hidden`；有焦點或 hover、可以編輯時才顯示（`opacity`）；大小照 Chrome（上下鈕 15px 寬、和行一樣高，清除鈕照字級、8px 到 24px，前面 1px）；`::-webkit-` 開頭的 pseudo-element 都收（小寫）、後面收狀態的 pseudo-class，比對到 UA shadow tree 的部件，open 的 shadow tree 裡的不算 | `cargo test -p style --test it form_controls::the_parts_have_their_native_look_and_size stylesheet::placeholder_selectors_read_back_as_written`、`cargo test -p layout --test it form_controls::an_input_buttons_label_and_a_fields_part` |

### 原生外觀

| 項目 | 怎麼驗證 |
| --- | --- |

| 原生外觀（Chromium 淺色的顏色和 `NativeThemeBase` 的幾何）：checkbox（勾、未定）、radio、按鈕、文字框和 `<textarea>`、range 的軌道（值的那段到把手的中間）和把手、清除鈕的 `×`、上下鈕的箭頭；停用 > 按下 > hover > 一般，唯讀的文字框和停用的一樣；有原生外觀時不畫 CSS 的背景和邊框，`box-shadow` 照畫；作者的背景讓它變回 CSS | `cargo test -p paint --test it theme::` |
| 狀態改了重畫、不重排：checkbox 勾選、hover、停用改了之後 fragment 跟著，UA shadow tree 的部件照 host 的狀態（hover、按下照它自己的）；`dom` 只在原生外觀用到的狀態改變時標記；range 的把手照 `ratio`（值、`min`、`max` 改了跟著）放在軌道上 | `cargo test -p layout --test it update::native_looks_follow_their_controls_states`、`cargo test -p dom --test it layout_tree_flags::a_controls_state_flags_it`、`cargo test -p dom_api --test it forms::a_ranges_thumb_has_its_ratio` |

### `<select>`

| 項目 | 怎麼驗證 |
| --- | --- |

| 收起來的選單：UA shadow tree 的 inner element 是選到的 option 的 label（`label` 量的是原本的樣子、顯示的去掉頭尾的空白，空的 `label` 是文字，在 `<optgroup>` 裡量的多 4 個空白，`multiple` 又 `size=1` 是「N selected」），option 被選、插入、移除、文字（巢狀的也是）和 `label` 改了跟著改，`size`、`multiple` 改了在選單和 listbox 之間換；`dom` 在 option 裡的文字或子節點改變時記下 | `cargo test -p dom_api --test it forms::a_menu_lists_ua_shadow_tree_shows_its_option`、`cargo test -p dom --test it tree::form_state_elements_are_recorded_one_by_one` |
| 選單的大小和 baseline（Chrome 152、同一份 Lato）：最寬的 option 的 label 取整 + 4 + 16 + 邊框，高是字型的高度 + 2（`line-height` 不算），沒有原生外觀時沒有 4 和 16、高照 `line-height`；inner element 在它裡面置中（放不下時在頂端），切掉寬出來的文字；baseline 是文字的；label 改了寬度跟著變 | `cargo test -p layout --test it form_controls::a_menu_list_is_as_wide_as_its_widest_option form_controls::a_menu_lists_baseline_is_its_texts update::menu_lists_follow_their_options` |
| 選單的 style：inner element 的 margin 4px、16px 和上下 1px 的 padding（沒有原生外觀時沒有），`align-items: center` 時上下的 margin 是 `auto`；選單的 `line-height` 是 `normal`；UA 的 `white-space: pre`（listbox 是 `nowrap`）、停用的 `opacity: 0.7`；`:-internal-list-box` 只在 UA stylesheet 裡收 | `cargo test -p style --test it form_controls::a_menu_lists_inner_element_has_its_room form_controls::the_ua_rules_tell_menu_lists_from_list_boxes form_controls::disabled_controls_are_faded stylesheet::internal_pseudo_classes_are_the_ua_stylesheets` |
| 選單的外觀：文字框的圓角框（填色是 checkbox 的背景、邊框照狀態），箭頭是 8×4 的 `∨`、2 個 device pixel 的線、`color` 的顏色、在右邊 15px 的中間；作者改了背景或邊框時照 CSS 畫、箭頭在上面；沒有原生外觀時沒有箭頭 | `cargo test -p paint --test it theme::menu_lists` |
| listbox（Chrome 152、同一份 Lato）：option 照 block 排，高是 `size` 列（沒有時 4 列）、每列是最高的 option 或 optgroup 的標題（沒有時一行加 1px），自己的 `height`、`min-height` 優先；寬照內容加 15px 的捲軸；optgroup 的 option 縮 20px，`<hr>` 是半行的上 margin；`size` 改了、option 變大了跟著變 | `cargo test -p layout --test it form_controls::a_list_box_is_size_rows_high update::list_boxes_follow_their_size_and_options` |
| listbox 的 style：option 的 padding、最小高度、`line-height: normal`；選到的是灰底，有焦點時 `SelectedItem`，停用時灰字；listbox 的圓角、`overflow`、`vertical-align: text-bottom`；只有 listbox 自己的焦點改變時重算它的 option（`:-internal-list-box` 的規則的依賴只給 listbox） | `cargo test -p style --test it form_controls::a_list_boxs_options_show_their_selection invalidation::a_list_boxs_focus_styles_its_options_again` |
| `<optgroup>` 的 UA shadow tree：標題（`label` 去頭尾、折疊空白）和 slot，`label` 改了跟著改；`<option>` 的：有不是空的 `label` 時顯示它、藏起子節點 | `cargo test -p dom_api --test it forms::an_optgroups_ua_shadow_tree_shows_its_label forms::an_options_ua_shadow_tree_shows_its_label` |

### WPT 和 `about:demo`

| 項目 | 怎麼驗證 |
| --- | --- |

| `about:demo` 的表單：沒加樣式的控制項是 Chrome 的大小（range、checkbox、radio；文字框和 `<textarea>` 的跟著字型，在截圖裡），高的按鈕的文字在中間 | `python3 scripts/test.py scenarios demo-forms`（截圖 `controls`） |
| `about:demo` 的表單多一個選單和 3 列的 listbox，頁面量 listbox 是 3 列高 | `python3 scripts/test.py scenarios demo-forms`（截圖 `controls`） |
| WPT：`html/rendering/widgets/`（整個）、`css/css-ui/` 的 `appearance`、`css/css-color/` 的 system color、`css/css-fonts/parsing/font-computed.html` 照預期，失敗的 `.ini` 寫了原因 | `python3 scripts/test.py wpt html/rendering/widgets/`（`css/css-ui/`、`css/css-color/`、`css/css-fonts/parsing/` 各跑一次） |

## 做過的檢查

- **做了什麼**（第 1 到 27 條）：
  - `gfx`、`css`、`style`：system color（`SystemColors`，44 個關鍵字，computed value 是顏色，canvas 也收）和 system font；`appearance` 和 `-webkit-appearance`；`theme` 模組（effective appearance，`IsControlStyled`，`AdjustStyle` 的 `display`、checkbox 的 13px、部件的大小，選單的 inner element 的 room 和 `line-height`）；UA stylesheet 的控制項規則（Chrome 152 的 computed value）；`::placeholder` 和 `::-webkit-*` 的 pseudo-element 照 host 比對到 UA shadow tree 的元素（`StyleElement` 的 shadow pseudo 模式、`ScopeAt::outer`、`Which::ShadowPseudo`），部件的規則和 listbox 的規則的依賴只給 UA shadow tree 的 host 和 listbox；`-webkit-text-security`；`:-internal-list-box`（只有 UA stylesheet 收）。
  - `shell`、`browser`、`renderer`：Windows 從 registry 讀八個系統色，啟動 renderer 時交給它（`RendererFlags`），測試和其他平台用 Blink 的預設表。
  - `dom`：`ShadowRootMode::UserAgent`；`unlisted::PSEUDO`、`RATIO`、`LABELS`；`ElementState::CONTROL` 和 `LayoutTreeFlags::control_state_changed`；`<input>` 的 `size`、`type`，`<textarea>` 的 `cols`、`rows`，`<select>` 的 `size`、`multiple` 改了重建盒子；option 裡的文字和子節點改變時記 `FormStateChange`。
  - `dom_api` 的 `text_control_tree`：文字控制項、按鈕類的 `<input>`、search 和 number 的部件、range、選單、listbox、optgroup 和 option 的 UA shadow tree，插進有 browsing context 的 document 之後建，跟著值、placeholder、`type`、選取、option 的文字和 `label` 改；`mouse_focus` 經過 host 往上找；`getComputedStyle(el, '::placeholder')`。
  - `layout` 的 `control` 模組：控制項自己的大小和 baseline（文字框、`<textarea>`、range、選單、listbox），在 block 裡照 replaced 元素的方式算；button layout（內容置中、fit-content、flow root）；UA shadow tree 的內容排在控制項的大小裡（文字框和選單的內容置中）；range 的把手照比例放；`ControlState` 跟著 fragment，狀態改了只重做 fragment。
  - `paint` 的 `theme` 模組：checkbox、radio、按鈕、文字框、range、清除鈕、上下鈕、選單的原生外觀（`NativeThemeBase` 的幾何、Chromium 淺色的顏色），畫在 CSS 的背景和邊框的位置，`menulist-button` 的箭頭畫在它們上面；`PaintController` 比較 fragment 時看 `control_state`。
  - `about:demo` 的表單用原生外觀，多一段沒加樣式的控制項（頁面自己量和字型無關的大小、listbox 的列數），`demo-forms` 情境檢查它、截圖。
  - 文件：`docs/limitations.md`（表單控制項的那一條：原生外觀、部件和選單的行為、選單的半個 pixel、listbox 的捲軸位置、`base-select`；system color、system font），`docs/architecture.md`（`layout`、`paint` 的列），`crates/dom_api/README.md`、`crates/dom/README.md`。
- **和原本的計畫不同的**（細節在 README 對應的那一條）：
  - 第 7 條：原本以為跟著平台的 system color 只有選取的顏色和強調色，在 Chrome 量了之後改成按鈕、文字框、連結、選取和視窗的顏色都跟著，強調色反而固定。
  - 第 11 條：1b 照 replaced 元素調整 table 類的 `display`，1c 和 WPT、Chrome 對照之後改成 `LayoutTheme::AdjustStyle` 的（對象是有 `appearance` 的元素）。
  - 第 12 條：按鈕的內容原本要建匿名的 block，改成排完之後往下移；1c 補上 fit-content 和 flow root。
  - 第 24 條：選單原本要當成 flex container，改成 block 加上安全的置中（幾何一樣）。
  - 3b 多做了 option 的 UA shadow tree（listbox 顯示 option 的 `label`）：WPT 的 `option-label-and-text` 這幾份抓到，Blink 也是這樣做。
- **找到並修的**（自己的測試、mutation test、WPT 和截圖抓到的）：
  - paint 的快取在狀態改了、style 沒變時沿用舊的 display item，勾選的 checkbox 畫成沒勾（`test.py automation` 的截圖抓到；單元測試都是從頭畫）。
  - UA 的 `:focus` 規則（部件的、listbox 的）讓每次焦點改變都重算那個元素的子樹：依賴只給 UA shadow tree 的 host 和 listbox（`invalidation` 的測試抓到）。
  - 文字框和 flex item 的寬度在有內容時被撐開；grid 和 flex 的 `<input>` 按鈕的文字在頂端；range 的比例在 `min`、`max` 改了時不動；空的 `<input>` 按鈕沒有一行；listbox 改了 `size` 列數不變；listbox 自己的 `height`、`min-height` 被列數蓋掉。
  - 多餘的程式（mutation test 抓到）：`compute_box`、`lay_out` 裡控制項的大小和 baseline 的分支、`shadow_pseudo_id` 的 mode 判斷、`build_block` 設 taffy style 的那段、絕對定位的按鈕的 fit-content 條件、選單的 `line-height` 的分支，都拿掉了。
  - 量測：第一次用 `--dump-dom` 量 `<select>` 時頁面沒等 `@font-face` 的 Lato 載入，量到的是替代字型的，layout 的測試對不上才發現，改成等 `document.fonts.load()` 之後全部重量。
  - WPT 的 `.ini`：`propagate-text-decoration` 在 2b 被誤刪（原因不是表單，是裝飾線傳進 inline-block，之後修好了）；幾份原因過期的改寫。
- **不穩定的測試**：沒有新的。這台的 Windows 上 `custom-elements/reactions/Animation.html` 和 `dom/events/scrolling/` 的四個每次都和預期不同（在 `main` 的 worktree 上也一樣，預期是別的平台上寫的）。
- **mutation test**（每一小段一份，`mutants-*.toml`；每段做完時照最後的程式全部重跑過）：
  - 最後一次（2026-10-17）：`1a` 48 條、`1b` 32、`1c` 18、`2a` 30、`2b` 25、`2c` 26、`3a` 35、`3b` 20 條，共 234 條，全部抓到。
  - 每段第一次沒抓到的，多數是測試沒碰到的情況，都補了測試：用平台的表的 system color、有空白的長的值、open 的 shadow tree 的部件、40px 的清除鈕、自己有 `color` 的 range、`<textarea>` 的圓角、移除沒選到的 option、40px 高的選單的 baseline；其他是上面「多餘的程式」。
  - fmt 和後面的段落改過的程式，前面的 spec 的 `from` 跟著改過再跑。
- **WPT**（清單加上 `html/rendering/widgets/` 整個、`css/css-ui/` 的 24 個 `appearance`、`css/css-color/` 的四個 system color、`css/css-fonts/parsing/font-computed.html`、`css/css-pseudo/` 的七個 placeholder；每段跑有用到控制項的 61 到 65 個目錄，一個跑一次（好幾個一起時 Windows 的命令列太長））：
  - 變通過的：widgets 的 `button-layout/`（`grid`、`flex`、`shrink-wrap`、`anonymous-button-content-box`、`input-type-button-*` 這些）、`input-checkbox-no-centering`、`input-radio-no-centering`、`input-checkbox-disabled-checked`、`input-radio-disabled-checked`、`input-text-overflow-ellipsis*` 的三份、`textarea-scroll-restoration`、`the-select-element/` 的 option label 的四份、`select-wrap-no-spill`、`css/css-ui/appearance-menulist-button-002` 等。
  - 還失敗的主要原因（`.ini` 寫了）：window 的 named property（很多 widgets 的測試用元素的 `id` 當全域變數）、`innerText`、`all`、`field-sizing`、`contain`、`flow-root`、`::slotted()::placeholder`、有焦點時的省略號和文字框的捲動、kun 的 UA stylesheet 沒有 `@namespace`（別的 namespace 的元素也套到控制項的規則）、switch、`base-select`。
- **跑過的**：每段做完時的 `cargo test --workspace`（第 1 段 2998 個、第 2 段 3032 個（rebase 之後）、第 3 段 3047 個測試通過）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`（每段修了幾處，都在新寫的程式）；`test.py reftest`（0 個預期外）、`test.py scenarios`（全部 54 個）、`test.py automation`（43 個情境和 `pipe --window`）；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`。外觀和 Chrome 的截圖對照過：checkbox、radio、range、按鈕、文字框、search、number、選單（2 倍的截圖裡箭頭逐像素一樣）、listbox。
- **沒跑的**：`matrix`、`fuzz`（沒有新的 GC 型別；`dom` 的 `unsafe` 多了幾處讀祖先的，論證和既有的一樣）；整套的 `wpt`、`wdspec`；`ci.py speedometer`（只在使用者要求時跑）。`cargo clean` 等 merge 回 `main` 之後跑。
