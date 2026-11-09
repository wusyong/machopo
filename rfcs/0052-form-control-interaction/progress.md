# RFC 0052 的進度：表單控制項的互動

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-17。完成：2026-10-17，Windows 11（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0052` 上做，三段做完時一起 merge 回 `main`（由人審核）；merge 前 rebase 到 RFC 0051 第 2a 段之後的 `main`（只有 `style` 的 `invalidation` 測試衝突，兩邊都在檔尾加了測試），之後補上 `:open` 的 attribute feature（見「找到並修的」）；收尾之後 `main` 又多了第 2b 段，再 rebase 一次（同一個檔尾的衝突），檢查重跑過。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 listbox 的滑鼠和鍵盤、收起來的選單的鍵盤 | 完成 | 2026-10-17 |
| 2 清除鈕、上下鈕和 step 的規則 | 完成 | 2026-10-17 |
| 3 下拉清單和 `showPicker()` | 完成 | 2026-10-17 |

## 驗收

### listbox

| 項目 | 怎麼驗證 |
| --- | --- |
| listbox 的按下：主按鍵 focus 它（沒有 focus ring）、選游標下的 option 一個（沒有 `multiple` 時 Shift、Ctrl 也是），按在沒有 option 的地方、停用的 option、次按鍵都不改；放開時選取變了才發 `input`、`change`；按到的 option 排著捲進看得到的地方 | `cargo test -p dom_api --test it select_type::a_press_selects_the_option_under_it` |
| `multiple` 的按下：Shift 是 anchor 到它的一段（第一次從第一個選了的，沒有時只有它），Ctrl（Mac 是 Cmd）加減一個、成為 anchor；範圍裡停用的不動 | `cargo test -p dom_api --test it select_type::shift_and_ctrl_presses_with_multiple` |
| 拖曳：按著移動時移動範圍的 end，範圍外的回到按下時的選取（Ctrl 按在選了的上時拖曳是取消），放開時發一次 `input`、`change`；沒有 `multiple` 時選游標下的；沒有按著、按下不在它上面（它失焦之後）時不改；拖曳中加進來的 option 取消，option 的數目變了時只發 `change` | `cargo test -p dom_api --test it select_type::dragging_moves_the_end` |
| listbox 的鍵盤：↓↑ 從 end（沒有時從最後、第一個選了的）走到下一個不是停用的，PageDown/Up 是 `size − 1` 個 list item（`<hr>` 也算，沒有 `size` 時 4 列），Home、End；選它一個、當下發 `input`、`change`、捲進看得到的地方，鍵是它的；沒有地方走時是頁面的；Enter 的 `keypress` 是它的 | `cargo test -p dom_api --test it select_type::a_list_boxs_keys` |
| `multiple` 的鍵盤：Shift 從 anchor 延伸（第一次從走到的那個），Ctrl 只移 end（`:-internal-multi-select-focus`），之後 Space、Ctrl+Space 加減它；沒有修飾鍵時又只選它一個 | `cargo test -p dom_api --test it select_type::shift_and_ctrl_keys_with_multiple` |
| disabled 的 select（和 disabled 的 fieldset 裡的）不處理滑鼠和鍵盤；按在 option 的子元素、UA shadow tree 裡的標題上也是那個 option；`cloneNode` 的 select 不帶著按下時記的東西 | `cargo test -p dom_api --test it select_type::a_disabled_select_does_nothing select_type::a_press_in_an_option select_type::a_copy_keeps_nothing_of_the_press` |
| Ctrl+方向鍵移到的 option 在 listbox 有 `:focus-visible` 時畫 focus ring（UA 規則，`outline-offset: -1px`），按下 focus 的沒有；它改變時只重算那個 option；`:-internal-multi-select-focus` 只在 UA stylesheet 裡收 | `cargo test -p style --test it form_controls::a_list_boxs_multi_select_focus_has_a_ring invalidation::a_multi_select_focus_styles_its_option_again stylesheet::internal_pseudo_classes_are_the_ua_stylesheets` |
| layout 之後的捲動：等下一次 layout，只捲那個捲動框、到最近的邊，同一個捲動框只做最後一個 | `cargo test -p renderer --lib a_scroll_after_layout_waits_and_keeps_the_last` |

### 收起來的選單和下拉清單

| 項目 | 怎麼驗證 |
| --- | --- |
| 收起來的選單的鍵：沒有修飾鍵的 ↓→、↑←、PageDown/Up（3 個）、Home、End 選下一個可選的 option，選到別的才發 `input`、`change`，鍵是它的（沒有地方走時也是）；有修飾鍵、別的鍵、`keyup` 不處理；不捲動 | `cargo test -p dom_api --test it select_type::a_menu_lists_keys` |
| 打開清單：主按鍵按下時 focus、請 frame 打開（list item：option 的顯示 label 去頭尾、折疊空白，optgroup 的 `label`，`<hr>`，停用的；選到的那個），`:open` 成立，開著時再按下關上；Alt+↓↑、F4（Alt+F4 不算）、Space、Enter 打開，次按鍵不打開，已經開著時不再開；`multiple`、disabled 的不打開；失焦時關上 | `cargo test -p dom_api --test it select_type::a_menu_lists_dropdown_opens_and_closes` |
| 清單的結果：選了能選的 option 時選它、不同時發 `input`、`change`，再發 `mouseup`（鍵盤選的不發）和 `click`；同一個、停用的、optgroup 的標題不改選取（`click` 照發）；取消什麼都不做；沒開著時收到的結果丟掉 | `cargo test -p dom_api --test it select_type::the_dropdowns_pick` |
| `showPicker()`：沒有 activation 是 `NotAllowedError`（用掉之後再呼叫也是），沒有 box 是 `NotSupportedError`，disabled 是 `InvalidStateError`，跨 origin 的 frame 是 `SecurityError`；選單打開清單、listbox 不做事 | `cargo test -p dom_api --test it select_type::show_picker` |
| `:open`：清單開著的 select（state）、有 `open` 的 `<details>`、`<dialog>`（attribute），別的元素的 `open` 不算；改變時重算它和它的子元素，`<details>` 自己的 style 沒變時，用它的 `:open` 的下面的元素也重算 | `cargo test -p style --test it invalidation::open` |
| Browser 畫的清單：在 select 下面、左邊對齊、至少和它一樣寬，下面放不下而上面比較寬時放上面，最多 20 列、放在視窗裡；選到的先 highlight（停用的不算）；鍵盤移動跳過不能選的，Enter、Tab、F4、Alt+↓ 選、Esc 取消；游標 hover、在列上放開選；滾輪捲列；畫邊框、背景、highlight 的列和文字 | `cargo test -p shell --lib popup_menu` |
| 在視窗裡（真的 tab）：點選單打開清單，畫在網頁的 surface 上面；↓、Enter 選下一個，頁面收到；點一列選它；在工具列按下、在網頁別處按下、滾輪、視窗大小改變都關上；開著時在選單上按下關上，不再打開；不是顯示中的分頁要的不顯示；select 移走、失焦時 renderer 關上 | `cargo test -p shell --test it browser_view::a_selects_dropdown_is_over_the_page browser_view::only_the_tab_shown_has_a_dropdown browser_view::a_blurred_selects_dropdown_closes browser_view::a_removed_selects_dropdown_closes` |
| 標了 `over_surfaces` 的 layer 畫在 surface layer 前面 | `cargo test -p compositor --lib a_layer_over_the_surfaces_is_drawn_in_front_of_them` |

### step 的規則、清除鈕和上下鈕

| 項目 | 怎麼驗證 |
| --- | --- |
| `stepUp()`、`stepDown()`：number 預設的 step 是 1、沒有值時從 0、`n` 步，夾在 `min`、`max` 裡、對齊 step base（`min`，沒有時 `value` attribute）；不是正數的 step 是預設的；小數的 step 照寫的小數位數（0.3）；不在 step 上的值先到路上對齊的那個、算一步（Blink）；超過最大值時是它下面對齊的值；`step="any"`、text、日期的型別丟 `InvalidStateError`；最小值大於最大值時不改；range 也是 | `cargo test -p dom_api --test it input_steps::step_up_and_step_down` |
| `valueAsNumber`：number、range 的值的數字，沒有值或別的型別是 `NaN`；設它改值，`NaN` 清空（range 回到中間），無限大是 `TypeError`，別的型別 `InvalidStateError` | `cargo test -p dom_api --test it input_steps::value_as_number` |
| range 的值對齊 step（從最小值，一半時往上），夾在最小值和最大值裡，最近的超過最大值時退一步；最大值小於最小值時是最小值 | `cargo test -p dom_api --test it input_steps::a_ranges_value_is_aligned_to_its_step forms::values_are_sanitized`、`cargo test -p dom_api --lib step_range` |
| number 的 ↑↓：`step="any"` 也照預設的 step，發 `input`、`change`；沒有數字時從 0 或 step 留得住的最近的值，低於最小值時是最小值，在最大值時往上不動；唯讀、disabled 的和別的鍵是頁面的 | `cargo test -p dom_api --test it input_steps::a_numbers_keys` |
| range 的鍵：↑→ 一步、↓← 退一步（`any` 是範圍的百分之一）、PageUp/Down 十分之一、Home、End，值變了才發 `input`、`change`，鍵都是它的 | `cargo test -p dom_api --test it input_steps::a_ranges_keys` |
| 上下鈕：按下 focus 欄位、照按下的那一半加減（`input`）、拿走滑鼠；按著 250 ms 之後每 50 ms 一次，游標在按下的那一半時才加減（Mac 照現在的那一半）；放開時 commit（`change`）、還回滑鼠，之後不再加減；馬上再按時舊的那次的重複不算；失焦、變成唯讀時停下；唯讀、disabled 的不做事 | `cargo test -p dom_api --test it input_steps::the_spin_button_steps_while_it_is_held input_steps::a_spin_button_press_ends` |
| 拿著滑鼠的元素：在 document 裡（連同 shadow tree 的 host）時才算；renderer 把滑鼠的事件送給它（移到外面放開時 `mouseup` 也是），按鈕都放開之後放掉 | `cargo test -p dom --test it focus::mouse_capture_is_an_elements_while_it_is_connected`、`cargo test -p shell --test it forms::a_spin_button_keeps_the_mouse_while_pressed` |
| 清除鈕的 `click` 清空 search 欄位、發 `search`：有焦點時 `input`（`change` 在失焦時），沒有時 `change`，空的時只有 `search`；有值時的 Escape 一樣；唯讀的不做事，頁面自己帶同樣 shadow pseudo id 的元素也不算 | `cargo test -p dom_api --test it input_steps::the_cancel_button_and_escape_empty_a_search_field` |
| 文字控制項的 inner editor 空的也有一行高（沒有顯示 placeholder 時），上下鈕和它一樣高 | `cargo test -p layout --test it form_controls::a_text_controls_value_is_laid_out_in_its_size` |

### 情境和 WPT

| 項目 | 怎麼驗證 |
| --- | --- |
| 在視窗裡：點 listbox 的 option、Shift+↓ 延伸，走到 3 列外的那個時 listbox 捲動（頁面不捲），Home 捲回去；`about:demo` 多一個可以選的 listbox 和選單，和 number、range、search 的欄位：number 的 ↑ 照 step 加、range 的 → 移、search 的 Escape 清空；選單點了打開清單，↓、Enter 選、點一列選、在別處按下關上 | `python3 scripts/test.py scenarios list-box demo-forms`（截圖 `range`、`menu-open`、`menu`、`picked`、`stepped`） |
| WPT：`html/semantics/forms/` 的 `stepUp`、`stepDown`、`valueAsNumber`、range 的 step 那些通過，日期和時間的型別的失敗寫了原因；`show-picker-user-gesture.html` 和 `details-open-pseudo-001.html` 通過，加進清單；`html/rendering/widgets/`、`css/selectors/` 照預期 | `python3 scripts/test.py wpt html/semantics/forms/ html/rendering/widgets/ css/selectors/` |

## 做過的檢查

- **做了什麼**（第 1 到 5 條）：
  - `dom_api` 的 `select_type`：select 的 default event handler（listbox 的滑鼠和鍵盤、收起來的選單的鍵盤和下拉清單、`showPicker()`、`popup_menu_closed`）；default event handler 拿到分派時的 target。`step_range`（step 的規則）、`inner_elements`（清除鈕、上下鈕、search 的 Escape）、`input::set_value_for_user`；`user_activation::consume_activation`；`FrameClient` 的 `show_popup_menu`、`hide_popup_menu`；宣告表的 `stepUp()`、`stepDown()`、`valueAsNumber`、`showPicker()`。
  - `dom`：`SelectState`（anchor、end、比對的選取，清單開著沒有）、`SpinState`；`ElementState::MULTI_SELECT_FOCUS`、`OPEN`（state 變成 32 位元）；拿著滑鼠的元素（`set_mouse_capture`、`mouse_capture`）；`LayoutClient::scroll_into_view_after_layout`。
  - `css`、`style`：`:-internal-multi-select-focus`（只有 UA stylesheet）、`:open`，UA stylesheet 的 multi-select focus 的 focus ring。
  - `layout`：空的 inner editor 也有一行。
  - `protocol`、`renderer`、`browser`：`ShowPopupMenu`、`HidePopupMenu`、`PopupMenuClosed`；renderer 算清單的位置、字級和顏色，在網頁上按下時關上清單，移走、停用了的 select 的清單關上；滑鼠事件送給拿著滑鼠的元素；layout 之後的捲動。
  - `compositor`：layer 的 `over_surfaces`。`shell`：`popup_menu`（清單的版面、輸入、畫法），`BrowserView` 把它放在最上面、開著時輸入先給它。
  - `about:demo` 多一個 listbox、選單和 number、range、search 的欄位；情境 `list-box`（新的）和 `demo-forms` 的最後一段。
  - 文件：`docs/limitations.md`（表單控制項的行為、`<select>` 的下拉清單、pseudo-class 的清單、日期和時間的 step），`docs/architecture.md`（`compositor` 的 `over_surfaces`、`shell` 的 `popup_menu`），`docs/testing.md`（等價的 mutation、capture 的放掉），`dom`、`dom_api`、`shell` 的 README。
- **和原本的計畫不同的**：
  - 第 4 條的 W2：有焦點的 number 的滾輪之後再做（設計概要原本列在目標裡）。
  - Esc 關上清單時不保留清單裡用鍵盤移過的（設計原本照 Chrome 保留），記進限制。
  - Ctrl+Space 在 kun 沒有 `keypress`（Chrome 有），改在 `keydown` 加減。
- **找到並修的**：
  - 空的文字欄位的 inner editor 是 0px 高，上下鈕和清除鈕按不到（`shell` 的測試抓到）：照 Blink 的 `HasLineIfEmpty` 補上一行，有顯示 placeholder 時不補（Blink 的 `HandleTextControlPlaceholder`）。上下鈕在欄位沒有 hover、focus 時是 `pointer-events: none`（Chrome 的 UA 規則），`shell` 的測試先移過去再按。
  - Browser 的 compositor 一律把網頁的 surface 畫在其他 layer 上面，清單蓋不過網頁：layer 多一個 `over_surfaces`（Chromium 的 Browser 的 layer tree 裡 popup 和網頁是兄弟，照順序畫）。
  - Chrome 的 `showPicker()` 會用掉 user activation（WPT 的 `show-picker-user-gesture.html` 檢查），kun 原本沒有「用掉」，加上 `consume_activation`。
  - rebase 到 RFC 0051 第 2a 段（改了的 attribute 只重新算那個元素的 style，除非下面的元素的規則用到它：`AncestorFeatures`）之後，`<details>` 開關時 `:open > summary` 的 summary 不跟著重新算：`:open` 只被記成 state，`<details>`、`<dialog>` 的卻是 `open` attribute。`AncestorFeatures` 把 `:open` 也記成 `open` attribute（Blink 是 `details` 的 `open` 改變時呼叫 `PseudoStateChanged(kPseudoOpen)`）。
  - 情境的 `key` 不等之前的輸入處理完，點選單之後馬上按鍵會先到網頁、清單還沒開：情境在按鍵之前先 `capture`。
  - `dom_api` 的 incremental 快取讓 rustc 在 codegen 時 access violation（`cargo check` 沒事，`CARGO_INCREMENTAL=0` 編得過）；刪掉 `target/debug/incremental/dom_api-*` 之後正常。
  - mutation test 抓到的到不了的檢查（disabled 的 range 的鍵、UA 的 shadow root 的判斷）拿掉了。
- **不穩定的測試**：`html/semantics/forms/the-input-element/focus-dynamic-type-change.html`。整套平行跑 `html/semantics/forms/ html/rendering/widgets/ css/selectors/` 時，第 2a 段之後 6 次裡有 2 次、第 2b 段之後 2 次裡有 1 次失敗：第一個子測試裡 script 的 `focus()` 之後就不是 `:focus-visible`，整份 ERROR（之後的子測試留下的 `blur` listener 再丟 `TypeError`）。單獨跑 10 次都通過。`origin/main`（RFC 0051 第 2a 段，沒有這份 RFC 的改動）同樣跑 6 次也有 1 次一樣的失敗，所以不是這份 RFC 造成的。同一個 kun 之前跑的測試都沒有滑鼠的輸入（`PageFocus::focus_visible` 只有滑鼠的 focus 會關掉），原因還沒找到，另外查。
- **mutation test**（每段一份，`mutants-1.toml`、`mutants-2.toml`、`mutants-3.toml`，`python3 scripts/test.py mutate <spec>`）：
  - 最後一次（2026-10-17，rebase 之後照最後的程式）：`mutants-1` 58 個、`mutants-2` 68 個、`mutants-3` 60 個，共 186 個，沒抓到的只有 `mutants-2` 的 6 個：五個等價的、一個補不了的（下一條）。後面的段落改過的程式，前面的 spec 的 `from` 跟著改過（`mutants-1` 的 4 個、`mutants-2` 的 1 個）。`mutants-1` 的「選單收 Alt 加其他的鍵」第 3 段之後沒抓到（`a_menu_lists_keys` 的 Alt+↓ 改成打開清單，沒有別的 Alt 的鍵了），補上 Alt+End 是頁面的之後抓到。
  - 每段第一次沒抓到的：第 1 段沒有；第 2 段 70 個裡 10 個，補了兩個測試（`stepUp()` 從最大值以上不往下、清除鈕只在 `click` 清空），兩個是到不了的檢查（程式和 mutant 拿掉），五個等價（記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」），一個補不了（renderer 在按鈕都放開之後放掉 capture：現在唯一拿滑鼠的上下鈕自己還回，記進「測試守不住的地方」）；第 3 段 59 個裡 1 個（清單的 label 不折疊空白：測試的 `label` 改成 `" C  D "`），rebase 之後加的 `:open` 的 feature 的 1 個抓到。
  - 第 1 段沒排進 mutation 的（等價、或碰不到）：`update_selected_state` 的「停用的不選」（停用的 option 按下和按鍵都到不了那裡）、`target_option` 確認 option 屬於這個 select（target 在 select 裡時一定是）、`focus` 之後檢查 select 還在 document 裡（要 listener 移走它，Blink 也只是不再處理）、拖曳時沒有 anchor 就不延伸（按下之後一定有 anchor）。
- **WPT**（清單加上 `show-picker-user-gesture.html`、`css/selectors/selectors-4/details-open-pseudo-001.html`）：
  - 變通過的：`the-input-element/` 的 14 個檔案的 `stepUp`、`stepDown`、`valueAsNumber`、range 的 step 的子測試（`input-stepup.html`、`input-stepdown-02.html`、`input-valueasnumber*.html`、`range.html`、`range-2.html`、`input-step-float.html`、`input-step-realignment.html`、`text.html`、`hidden.html` 等）、`show-picker-user-gesture.html`、`details-open-pseudo-001.html`。
  - 變失敗的：日期和時間的型別的 `valueAsNumber` 的 setter 和 `input-stepdown.html` 的三個（之前沒有這些成員，測試碰巧通過；現在照第 4 條的 D1 丟 `InvalidStateError`），`.ini` 的開頭寫了原因。
  - 還不能跑的：其他的 `show-picker-*.html`、`show-picker-cross-origin-iframe.html` 用 window 的 named access，kun 沒有；`css/selectors/open-pseudo.html` 要 `will-change` 和 `<dialog>` 的 `show()`；`details-open-pseudo-002`、`-003` 是 `<details>` 的畫法；`list-box-important-colors.html` 的 select 的 focus ring 看 `autofocus` 和 testdriver 的點擊誰先到，沒加進清單；`select-option-focusable`、`select-tab-navigation` 要 base-select，`select-restore-invalid-option` 要表單送出。
- **跑過的**：每段做完時的 `cargo test --workspace`（第 1 段 3063 個、第 2 段 3075 個、第 3 段 3089 個，rebase 到第 2a 段之後 3106 個、到第 2b 段之後 3107 個測試通過，45 個 binary）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py scenarios`（rebase 之後全部，56 次，0 個失敗）；`test.py wpt html/semantics/forms/ html/rendering/widgets/ css/selectors/`（4278 個檢查，除了上面的不穩定的 0 個非預期，兩次 rebase 之後都跑過）；`test.py reftest`（167 個，0 個非預期）。宣告表每段的新增：`kun:dom` 的 golden file 和 `spec-missing.txt` 重新產生（只有新增，不升版），wasm 的 guest 用 `ci.py guests` 重編。
- **沒跑的**：`automation`（沒有動到視窗那一層，清單畫在 `BrowserView` 的 layer 裡，由 `browser_view` 的測試看像素）；`matrix`、`fuzz`（沒有新的 GC 型別、沒有新的 `unsafe`）；整套的 `wpt`、`wdspec`；`ci.py speedometer`（只在使用者要求時跑）。`cargo clean`（merge 之後這次不跑，由人決定）。
