# RFC 0023 的進度：表單（G）

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-09。完成：2026-10-09，macOS Apple Silicon（Rust 1.99.0）。五段都在 `0023` 上做完，每一步做完時 commit、審核；第 1 到 4 段先 rebase 到 `main`、merge 回去（`main` 那時多了 `getComputedStyle`、`position` 和 RFC 0024 的引用規則），第 5 段接著在同一個 branch 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 十四個元素的 interface 和 reflection、`new Option()`（第 11、12 條） | 完成 | 2026-10-09 |
| 1b form owner、`form`、`elements`、`options`、`labels`、`control`（第 10、14 到 16 條） | 完成 | 2026-10-09 |
| 2a `<input>` 的狀態：value mode、`value`、`checked`、`indeterminate`、type change、radio group、cloning（第 17、18 條） | 完成 | 2026-10-09 |
| 2b 行為：checkbox 和 radio 的 activation 和事件、`<label>`、disabled、focus、鍵盤（第 19、20 條） | 完成 | 2026-10-09 |
| 3a `<select>` 和 `<option>`：selectedness、`selectedIndex`、`value`、`HTMLOptionsCollection` 的修改（第 21 條；`kun:dom` 0.11.0） | 完成 | 2026-10-09 |
| 3b `<textarea>`、選取範圍、`select` 事件（第 22 條） | 完成 | 2026-10-09 |
| 3c `reset()`、`<output>` 的值、按鈕的 activation（第 23 條） | 完成 | 2026-10-09 |
| 4 表單的 pseudo-class（第 25 條） | 完成 | 2026-10-09 |
| 5 `document.forms`、`about:demo` 和情境、RFC 完成時的驗證（第 24、26、27 條） | 完成 | 2026-10-09 |

## 驗收

每一步做完時跑 `cargo test --workspace`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`docs` 和 `test.py wpt`；fmt、clippy 和 `test.py automation` 在每一段做完時跑；改了 style 時跑 `test.py reftest`，寫了情境時跑 `test.py scenarios`（`CLAUDE.md`）。

**interface 和 reflection**

| 項目 | 怎麼驗證 |
| --- | --- |
| 十四個元素各是自己的 interface；reflection 的每種規則（字串、boolean、URL、列舉、限非負的 `long`、`unsigned long`、限正數和 with fallback、`double`）；`action` / `formAction` 的退路 | `cargo test -p dom_api --test forms` |
| 從元素算出的成員：`<select>` 的 `type`、`<option>` 的 `text` / `label` / `value`、`<textarea>` 的 `defaultValue`、`<progress>`、`<meter>` 的夾值、`<output>` 的 `htmlFor` | `cargo test -p dom_api --test forms` |
| HTML 解析數字的規則、`double` 寫回 JS 的 `Number::toString` | `cargo test -p dom_api --lib reflection` |
| `new Option()`：宣告表的 `legacy_factory`、realm 的 document、attribute 和文字；JS 的 `Option` 是全域、不可列舉、要 `new`、`prototype` 是 `HTMLOptionElement` 的 | `cargo test -p dom_api --test forms the_option`、`cargo test -p dom_js --test bindings the_option` |
| 宣告表和規範的 IDL 比對（還沒做的成員在 `spec-missing.txt`）；`kun:dom` 的 golden file 和 Lua runtime | `cargo test -p dom_api --test spec --test wit` |

**form owner 和集合**

| 項目 | 怎麼驗證 |
| --- | --- |
| form owner：最近的祖先 form（在任何樹裡）、`form` attribute（connected、第一個有那個 ID 的、要是 form、空的 ID 不算）、跟著插入、移除、ID 和 attribute 的改變重設；parser 關聯的插入時保留、移動或 form 被移走時重設；owner 改了集合重算、經過 GC 還在 | `cargo test -p dom_api --test forms` |
| `elements`、`length`、`form[i]`、`namedItem` 和 `form[name]`（`RadioNodeList`），fieldset 的 `elements`、select 和 datalist 的 `options`、`option.index`、`control`、`labels`、各元素的 `form`；`document.forms`（樹的順序、SVG 的 `form` 和不在樹裡的不算、live、每次同一個） | `cargo test -p dom_api --test forms` |
| parser 照 form element pointer 關聯（不是祖先的 form、`form` attribute 和 `</form>` 之後不關聯、foster parenting、被 script 移出 document 的 form 不關聯） | `cargo test -p html --test parse associates`、`cargo test -p html --test parser form` |
| 同一次插入的 script 看得到 form owner；整份 document 的 parser 的關聯 | `cargo test -p shell --test inserted_scripts form_owners` |
| JS：form 的控制項當 indexed 和 named property（`[LegacyOverrideBuiltIns]`）、iterable、`RadioNodeList` 和三個集合的 interface | `cargo test -p dom_js --test bindings forms_controls` |
| `kun:dom`：回傳 union 的 case 擁有 resource、匿名的 indexed getter 是 `item`；C 的 guest 拿到對的 case | `cargo test -p dom_api --test wit`、`cargo test -p dom_wasm --test engine forms` |

**`<input>`、`<select>`、`<textarea>` 的狀態**

| 項目 | 怎麼驗證 |
| --- | --- |
| `<input>`：value mode 的 `value`、sanitization、`checked`、`indeterminate`、跟著 attribute 到 dirty 為止、type change steps、radio group、cloning steps；合法的浮點數；JS 的 `value`（`null`）、`checked`、radio group | `cargo test -p dom_api --test forms`、`cargo test -p dom_api --lib input`、`cargo test -p dom_js --test bindings input_values` |
| `<select>` 和 `<option>`：selectedness setting algorithm、`selected`、`selectedIndex`、`value`、`selectedOptions`、插入和移除（一次的、逐一的、同一次呼叫裡連續的）、`length`、`select[i] = option`、`add()`、`remove()`、`new Option()` 和複製；JS 的 indexed setter、`add(x, x)` 沒有 mutation record | `cargo test -p dom_api --test forms`、`cargo test -p dom_js --test bindings selects_from` |
| `<textarea>` 的 raw value 和 API value、dirty flag、`textLength`；選取範圍（適用和不適用的 type、夾值、方向、改了才排 `select`、設值移到結尾、複製不帶）；`setRangeText` 的四種 mode；JS 的 `null` 和 enum | `cargo test -p dom_api --test forms`、`cargo test -p dom_js --test bindings text_controls` |
| `select` 事件在之後的 task 裡分派（trusted、bubbles、不能取消） | `cargo test -p shell --test forms select_events` |
| `kun:dom` 0.11.0（`html-select-element` 的 `remove(index: option<s32>)`）和只有新增的成員；Lua 的 union 參數、`option<s32>`、`option<u32>` | `cargo test -p dom_api --test wit` |

**行為**

| 項目 | 怎麼驗證 |
| --- | --- |
| checkbox 和 radio 的 activation：pre-activation 翻轉、listener 看到的值、`input` 和 `change`（trusted、`composed`、順序）、取消時還原（原本勾選的那個、移出同一組的、改了 type 的）、已經勾選的 radio、disconnected 的、點擊後 dirty；只有 `MouseEvent` 的 `click` | `cargo test -p dom_js --test bindings checkbox_and_radio` |
| disabled：`click()`、`<fieldset>` 和第一個 `<legend>`；`<label>` 的點擊轉給 control（例外：control 上、其他互動內容上、disabled、label 在 control 裡）；控制項能 focus（hidden、disabled、`<fieldset>` 的例外） | `cargo test -p dom_js --test bindings disabled_controls`、`cargo test -p dom_api --test focus form_controls` |
| `reset()`：`reset` 事件、取消、listener 裡的 `reset()`、各控制項的 reset algorithm、只重設 form owner 是它的；`<output>` 的 `value` 和 `defaultValue`；哪些按鈕送出或重設（type、大小寫、form owner、disabled） | `cargo test -p dom_api --test forms resetting`、`an_outputs`、`cargo test -p dom_api --lib reset::`、`cargo test -p dom_js --test bindings resetting` |
| 整個瀏覽器：點擊 checkbox 和它的 label、disabled 的控制項不分派 `click`、`<a>` 裡的文字框；Tab、Space（不捲動）、Enter；reset 按鈕的點擊和 Space、擋住外層的連結，沒有 form 的按鈕照樣跟隨 | `cargo test -p shell --test forms` |

**表單的 pseudo-class**

| 項目 | 怎麼驗證 |
| --- | --- |
| `:checked`、`:indeterminate`、`:default`（checkbox、radio 的 group、option、`<progress>`，建立時就比對）、`:disabled`、`:enabled`（fieldset 和第一個 legend、插入的子樹、optgroup、disabled 的 select 裡的）、default button、`:placeholder-shown` | `cargo test -p dom_api --test forms` |
| 使用者的點擊之後，下一次點擊的 hit testing 用新的樣式；畫面跟著 `input:checked ~` 的規則變（比 frame） | `cargo test -p shell --test forms pseudo_classes`、`python3 scripts/test.py scenarios form-pseudo-classes` |

**`about:demo`、WPT 和 GC**

| 項目 | 怎麼驗證 |
| --- | --- |
| `about:demo` 的表單：label、radio、Space、disabled 的 fieldset、reset；方框照 `:checked`、`:disabled` 上色 | `python3 scripts/test.py scenarios demo-forms`（真的視窗：`test.py automation`） |
| WPT：`html/dom/reflection-forms.html`、`html/semantics/forms/` 不用 testdriver、不等送出的檔案、`html/semantics/selectors/pseudo-classes/` 和 `css/selectors/` 的表單 pseudo-class；baseline 和跳過的理由見 `tests/wpt/README.md` | `python3 scripts/test.py wpt` |
| form owner 的 edge 和控制項的資料在 debug 的 V8、各種 GC 設定下 | `python3 scripts/test.py matrix`、`fuzz --iterations 20` |

## 做過的檢查

2026-10-09，macOS Apple Silicon。每一步的設計和更正寫進了 `README.md`；這裡是濃縮的紀錄。

- **WPT**：從 1648 個執行（1a）到 1912 個（第 5 段），每一步都 0 個不符。加進清單的：`reflection-forms`（1a）；`html/semantics/forms/` 的 form、label、fieldset 等目錄的 48 個（1b，30 個全過）；`the-input-element/` 的 98 個（2b）；`the-select-element/` 的 30 個和 `option-selected`（3a）；`textfieldselection/`、`the-textarea-element/` 的 24 個（3b）；`resetting-a-form/`、`the-output-element/`、`the-button-element/`（3c）；pseudo-class 的 19 個（第 4 段，`getComputedStyle` 之後 10 個全過）。等表單送出、image input 的圖片或 `valueAsNumber` 會 timeout 的 11 個跳過。順帶往前走的：`dom/nodes/insertion-removing-steps/` 的兩個表單的檔案、`html/interaction/focus/` 的 14 個和之前因為 `<input>` 不能 focus 跳過的 3 個、`dom/events/` 的 activation、`dom/nodes/` 的 `Element-matches-disabled-pseudo-class`。`document.forms` 讓 5 個表單的檔案全過、`button-events` 跑到子測試。
- **Mutation test**：共 323 條（1a：26、1b：47、2a：41、2b：41、3a：44、3b：36、3c：39、4：45、5：4，第 5 段起用 `test.py mutate`），都抓到或說明了等價，每次跑完都確認檔案還原了。
  - 一開始沒抓到、補了測試之後抓到的：1a 的 `<textarea>` 的 `defaultValue` 算進註解；1b 的 parser 不檢查同一棵樹、空的 ID；2a 的空字串的 `name`、有 namespace 的 `checked`；2b 的 radio 的點擊不設 dirty、disabled 的 button 裡的按鍵、點在 control 上不轉；3a 的沒存狀態時的 `selected`、設前面的 `selected`、`add(x, x)`、之後才插入的 option；3b 的兩種夾值和 `<input>` 的複製；3c 的 reset 不 sanitize（讀的時候會再 sanitize，從 type change 寫回 attribute 的值看出來）；第 4 段的插入的子樹只算根、renderer 在 frame 之前不重算（情境原本用 `wait-change`，它查詢元素的 box，查詢先走了 hit testing 那條路；改成比 frame）。
  - 等價的，拿掉了多餘的程式：`<label>` 的 `form` 檢查 control 是不是 form-associated、換成 file 時清空值、type 改了之後 sanitize、radio group 檢查同一棵樹、`<label>` 的重入保護、`add()` 的祖先檢查、`multiple` 時的 display size、textarea 的 reset 清掉 raw value。留著、說明等價的：`document.forms` 存在哪一個 rare data 的 edge（document 沒有 `labels`，換成那一個也一樣）。
  - 改成照 Blink：按鍵的 target 不必是元素自己（button 裡的元素的 Space 點擊 button）。
- **找到的 bug**：
  - `1.e2` 解析成 1（1a，WPT）；同一次插入的 script 先於 form owner（1b，WPT 的 `Node-appendChild-script-and-button-from-div`；form owner 的重設改成先做）。
  - 一批插入的 radio 留下第一個、`number` 的 `2e308`（2a）；插進不在 document 裡的樹的 radio 取消了同一組的、改了 type 的 radio 被取消點擊時還原錯了（2b，WPT）。
  - list of options 少了 `<div>` 裡的、optgroup 的 `disabled` 不重跑演算法、一次插入好幾個 selected 的留下第一個（3a，WPT）；插入的子樹裡找最後一個 selected 的時沒排除之後才插入的（3a，mutation test）。
  - 複製帶著選取範圍（3b，WPT 的 `select-event`）；radio group 的 key 少了樹（第 4 段，自己的測試）。
  - 規範比 kun 新的：`<div>` 裡的 option（customizable select 之後的 nearest ancestor select）、disabled 的 select 裡的 option 和 optgroup 是 `:disabled`（whatwg/html#12205，2026-05 合併），都照新的做。
- **設計上做的時候才發現的**：控制項沒有 box 就不能 focus，UA stylesheet 照 Chrome 給 `display: inline-block`（第 20 條）；`<select>` 的 `remove()` 是兩個 interface 的多載，`kun:dom` 升到 0.11.0；`FormControlData` 的 variant 叫 `Option` 時 rustc 的錯誤訊息改印 `std::option::Option`、`dom` 的 compile-fail 預期輸出就不對了，改叫 `OptionElement`；使用者的點擊不經過宣告表，pseudo-class 也要在 renderer 的 frame 和 hit testing 之前重算。
- **`kun:dom`**：3a 升到 0.11.0（`<select>` 的 `remove`），其他步驟都只新增（第 4 段沒有改）；每次改 WIT 都用 `ci.py guests` 重編 guest。Lua 的產生器多了回傳 interface 的 union、union 參數、`option<s32>`、`option<u32>` 和 C 的關鍵字欄位。
- **rebase 到 `main`**（第 1 到 4 段做完時）：8 個 commit 照順序 rebase，每個解了 README、`docs/limitations.md`、WPT 清單的衝突，改了 WIT 的重新產生 golden 和 guest，各自確認能編譯、`wit` 測試通過；最後一個 commit 照 RFC 0024 拿掉 `rfcs/` 以外的引用，理由直接寫在原處，WPT 的 baseline 照 `getComputedStyle` 更新。
- **不穩定的測試**：沒有遇到。`test.py automation` 有四次第一次跑時失敗，都是螢幕關著（`pmset -g log` 對得上），螢幕亮著（`caffeinate -d`）時重跑都過。
- **每一段做完時**：fmt（連同 `sdk/lua/generator`）和 clippy 修了留下的格式和多餘的 `drop`；`test.py automation` 25/25 到 29/29，看過 `demo-input`、`form-pseudo-classes` 的截圖。
- **RFC 完成時**（`docs/README.md` 的「一份 RFC 做完時」，2026-10-09，macOS Apple Silicon）：`test.py matrix` 8/8（每組 761 個測試）；`test.py fuzz` 20/20（seed `0x8e0b0948`；第一次跑時 `../third_party/rusty_v8` 不在 kun 釘住的 commit 上，`main` 把它釘到 fork 只改了 README 的下一個 commit，fast-forward 之後再跑）；`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`（1677 個）、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`docs`（只剩 `main` 上 `rfcs/0021-webdriver/progress.md` 的一個路徑）、`test.py wpt`（1912 個執行、0 個不符）、`test.py scenarios`（37 個，`demo-forms` 是新的）。`test.py automation`（真的視窗）30/30，看過 `demo-forms` 的截圖（勾選的方框是藍色、focus 的是黃色邊框、disabled 的是灰色）；第一次要跑時螢幕關著（`pmset -g log`：23:20 關閉），等螢幕亮著才跑。文件的核對交給一個唯讀的 agent 對照程式讀了這份 RFC 改過的段落，改了它找到的 8 處（`rfcs/` 以外留下的一個段落編號、`soundness.md` 的 I3 少了 `selectedOptions` 和 `forms`、`docs/limitations.md` 少列了幾個還沒有的成員、`:placeholder-shown` 的 type 寫得太窄、`docs/architecture.md` 的 `update_elements`、WPT README 一句容易誤讀的數字、這份 RFC 的「動機」寫錯了開始時的限制，和這一段原本的佔位）。
