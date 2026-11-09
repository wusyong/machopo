# RFC 0023：表單（G）

- 開始日期：2026-10-09
- 狀態：✅ 已完成：2026-10-09（第 1 到 9 條接受；第 1 段的第 10 到 16 條、第 2 段的第 17 到 20 條、第 3 段的第 21 到 24 條、第 4 段的第 25 條、第 5 段的第 26、27 條確認，都在 2026-10-09；五段都做完，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`（開始前的盤點和比較）

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 G 的第一段，DOM 線的第三份，接在 F（RFC 0022）之後：用 F 的 activation behavior 架構和 focus 模型，補上表單控制項的 DOM 和行為。M1 要的（`<input>` 的 `value`、`checked` 和 checkbox 的 activation）在第 2 段做完。

## 摘要

表單控制項的 interface 和行為，讓 script 和使用者的點擊照 HTML 的規範改變控制項的狀態：

1. **元素的 interface**：`HTMLFormElement`、`HTMLInputElement`、`HTMLButtonElement`、`HTMLSelectElement`、`HTMLOptionElement`、`HTMLOptGroupElement`、`HTMLDataListElement`、`HTMLTextAreaElement`、`HTMLLabelElement`、`HTMLFieldSetElement`、`HTMLLegendElement`、`HTMLOutputElement`、`HTMLProgressElement`、`HTMLMeterElement` 和三個集合（`HTMLFormControlsCollection`、`HTMLOptionsCollection`、`RadioNodeList`）；content attribute 的 reflection、`new Option()`、form owner、`elements`、`labels`、`document.forms`。
2. **控制項的狀態**：`<input>` 的 value mode、value、dirty flag、checkedness、`indeterminate`、type change steps、value sanitization；`<select>` 的 selectedness setting algorithm、`selectedIndex`、`value` 和 `HTMLOptionsCollection` 的修改；`<textarea>` 的 raw value；文字控制項的選取範圍 API 和 `select` 事件（只有狀態）；`<output>` 的 `value`。
3. **行為**：checkbox 和 radio 的 activation（切換、radio group、取消時還原，分派 `input` 和 `change`），`<label>` 把點擊轉給它的控制項，disabled（含 `<fieldset>`），控制項可以 focus、用 Space 和 Enter 操作，`form.reset()` 和 reset 按鈕。
4. **選擇器**：`:checked`、`:indeterminate`、`:disabled`、`:enabled`、`:default`、`:placeholder-shown`。

控制項畫不出來：路線圖把繪製放在 G 的第二段（CSS 線），這份只做 DOM 和行為（第 6 條）；表單送出、constraint validation 也不在這份（第 3、8 條）。

## 動機

**開始前（2026-10-09，`main` 的 `32adc89`；盤點見 `details.md` 的「kun 現在」）**：
- 沒有任何表單元素的 interface：`<input>`、`<button>`、`<select>` 等都是一般的 `HTMLElement`；`input.value`、`input.checked` 是 expando。
- activation behavior 只有 `<a>`：RFC 0022 做了架構，沒有控制項接上；沒有任何地方分派 `input`、`change`。
- 控制項不能 focus：`supports_focus` 只認 `tabindex` 和 `<a href>`，`tabIndex` 的預設值卻對控制項回傳 0。
- `:checked`、`:disabled` 等解析不了，UA stylesheet 對控制項沒有規則。
- `click()` 不檢查 disabled 的控制項。

**M1 要什麼**（`rfcs/0012-speedometer/details.md`）：runner 設 `.new-todo` 的 `value` 再分派 `change` 或 Enter；`.toggle` 是 checkbox，`click()` 之後 listener 讀 `checked`（沒有 activation 時 suite 照樣跑完、項目卻從來沒被勾選）；三個 suite 都對 `.new-todo` 呼叫 `focus()`。

**M2 要什麼**（`details.md` 的「Speedometer 的盤點」）：`type`、`name`、`defaultValue`、`defaultChecked`、`disabled`、`placeholder`、`autocomplete`、`min` / `max` / `step`、`selectionStart` / `selectionEnd`；`<button>` 和 `<fieldset>` 的 `disabled`；`<label>` 的 `htmlFor`；`<select>` 的 `options`、`value`、`multiple`、`disabled`，`HTMLOptionsCollection.length`；`<option>` 的 `selected`、`value`；`<textarea>` 的 `defaultValue`。沒有一個 suite 用 `form.submit()`、`requestSubmit()`。

**解決了的 `docs/limitations.md` 裡的限制**（開始時的）：「Style」的 `:checked`、`:disabled` 等 pseudo-class 不支援；「事件」的 focus 只有有 `tabindex` 的元素和有 `href` 的 `<a>`、disabled 的表單控制項照樣分派（還沒有表單控制項）。做的時候加進去的限制（控制項畫不出來、過程中還沒有的 `document.forms` 等）在 `docs/limitations.md` 的「Bindings 和 DOM API」，`document.forms` 做完時刪掉了。

## 設計概要

**給寫網頁的人**：
- 十四個元素和三個集合的成員照 HTML（constraint validation、表單送出、`valueAsNumber`、`showPicker()` 這類除外）；`document.forms`。
- `checkbox.click()` 切換 `checked`，分派 `input` 和 `change`；listener 呼叫 `preventDefault()` 時還原、不分派。radio 勾選時同一組的其他 radio 取消勾選。點 `<label>` 等於點它的控制項。
- `input.focus()` 有效；Tab 會走到控制項，Space 點擊 checkbox、radio 和按鈕，Enter 點擊按鈕；disabled 的控制項不能點、不能 focus。
- `form.reset()` 和 reset 按鈕把控制項放回預設；submit 按鈕什麼都不做（還不送出），但和 Chrome 一樣擋住外層連結的 click。
- `input:checked`、`:disabled`、`:placeholder-shown` 等選擇器成立（`querySelector`、`matches()` 和樣式都是）。

**給 wasm 的 guest**：同樣的 interface 和成員在 `kun:dom` 裡（`html-input-element` 等 resource）。`html-select-element` 的 `remove` 是 `ChildNode` 的 `remove()` 和 options 的 `remove(index)` 的多載，`remove(index: option<s32>)`，`kun:dom` 升到 0.11.0；其他都只新增。

**給使用者**：視窗裡還看不到控制項（第 6 條）；點 label、Tab、Space、Enter 和頁面自己畫的控制項都有作用，`about:demo` 有示範（第 27 條）。

**給改 kun 的人**：控制項的狀態在 `dom` 的元素上，規則在 `dom_api`：`forms`（成員、form owner 的查詢、disabled、`<label>`）、`listed_element`（重設 form owner）、`input`、`select`、`text_control`、`reset`、`form_states`（pseudo-class）。

## 詳細設計

型別照 Chromium，宣告照 Blink 的目錄放：新的 interface 在 `core_html.rs`（Blink 的 `core/html/forms/`），`tags(...)` 寫在宣告上。

### 1. 元素的 interface 和 reflection

- 十四個元素各是自己的 interface（第 11 條：`<datalist>`、`<output>`、`<progress>`、`<meter>` 也有，WPT 的 `reflection-forms` 檢查它們），成員照 Blink 的 `.idl`，不含 constraint validation（第 8 條）。reflection 用 `html_element` 的 helper，加上 HTML 的列舉、`long` / `unsigned long` / 限正數 / `double` 的規則（`reflection` 模組）。`<a>` 的 `href` 不在這份（第 13 條）：`reflection-forms` 裡 URL 類的子測試照 baseline 失敗。
- **`new Option()`**（第 12 條）：宣告表的 `legacy_factory`（WebIDL 的 `[LegacyFactoryFunction]`）；JS 是 window 上的 `Option`，`kun:dom` 是 `html-option-element` 的建構函式。
- **form owner**（第 10、14 條，Blink 的 `ListedElement::form_`）：`dom` 的元素上一條強指著 form 的 edge 和 parser inserted flag。`dom` 在任何樹裡記下可能改變它的事（`FormChange`：插入、移除、`form` 和 `id` attribute），`dom_api` 在每次呼叫宣告表結束時、parser 的每一步之後照 HTML 的「reset the form owner」重設，而且先於同一次插入的 script（它們是插入的當下就做的 insertion steps）。parser 的關聯經過 html5ever 的 `associate_with_form`，照 HTML 保留。兩個開關讓沒有表單的頁面不付成本。
- **集合**：`form.elements`（`HTMLFormControlsCollection`）、`form[i]`、`form[name]`（第 16 條，沒有 past names map），多個同名的是 `RadioNodeList`（第 15 條）；fieldset 的 `elements`、select 和 datalist 的 `options`、`labels`、`document.forms`（第 26 條，document 上同一個 `HTMLCollection`）。都是 rare data 裡的 live 集合，項目由 `dom_api` 算。`<select>` 的 list of options 照 customizable select 之後的 nearest ancestor select（`<div>` 裡的也算）。

### 2. 控制項的狀態

- **在 `dom` 的元素上，第一次需要時才配置**（第 2、17 條）：不被 trace 的 `Option<Box<FormControlData>>`：`<input>` 的 value、dirty flag、checkedness、indeterminate、上次的 value mode、選取範圍；`<option>` 的 selectedness 和 dirtiness；`<textarea>` 的 raw value、dirty flag、選取範圍；`<output>` 的 default value override；`<form>` 的 locked for reset。沒配置時從 attribute 算，大部分的控制項不多佔記憶體。HTML 的 attribute change steps 在 `handlers::attribute_changed` 當場做；cloning steps 複製狀態，但不複製選取範圍（`<output>` 和 `<form>` 的也不複製）。
- **`<input>`**：value mode 照 `type`，type change steps；value sanitization 做文字類、`url`、`email`、`number`、`range`（第 9 條）。
- **radio group**（第 18 條）：同一個 form owner（或都沒有）、同一棵樹、相同而非空的 `name`，需要時從樹走一遍找；勾選、插入（connected 時）、改名、換 form owner 都會取消同一組的其他。
- **`<select>`**（第 21 條）：selectedness setting algorithm 在 attribute 的 hook 和 `selected` 的 setter 裡當場跑，插入和移除的在呼叫結束時跑（經過 `FormChange`）；一次插入好幾個 selected 的留最後一個。`selectedIndex`、`value`、`selectedOptions`、`length`、indexed setter、`add()`、`remove()`。
- **選取範圍**（第 7、22 條）：`selectionStart` 等和 `setSelectionRange()`、`select()`、`setRangeText()`，位置是值的 UTF-16 code unit，只有狀態；改變時排一個 element task 發 `select`（`dom` 的 `queue_element_task`，renderer 在每個 task 之後分派）。
- **`<output>`**：`value` 是後代的文字，`defaultValue` 是 default value override 或那段文字。

### 3. 行為

- **checkbox 和 radio**：`ActivationBehavior` 的三步。legacy-pre-activation 切換 checkedness（radio：勾選它、記下同一組原本勾選的那個），狀態放進 `ActivationState`；取消時還原（改了 type 的照 Blink 的 `DidDispatchClick`）；沒取消時分派 `input`、`change`（trusted、冒泡，`change` 不 composed），已經勾選的 radio 不分派。只有 checkbox、radio 和送出、重設的按鈕有 activation behavior，所以 `<a>` 裡的文字框不搶走連結的 click。
- **按鈕**（第 23 條）：有 form owner、不是 disabled 的 `<button>`（預設 `submit`）和 submit、reset、image 的 `<input>` 才是 activation target，和 Chrome 的 `SetDefaultHandled` 一樣；reset 重設 form owner，submit 和 image 什麼都不做（第 3 條）。
- **`<label>`**：`DefaultEventHandler` 把 click 轉成它的 control 的合成 click，也 focus control；點在 control 上、其他互動內容上、control 是 disabled 時不轉。
- **disabled**：`disabled` attribute 和 `<fieldset disabled>`（第一個 `<legend>` 裡的除外）；`click()` 什麼都不做，使用者的 `click` 不分派到它和它裡面，不能 focus。
- **focus 和按鍵**：控制項是 focusable area；為了有 box，UA stylesheet 照 Chrome 給它們 `display: inline-block`（第 20 條，畫出來之前大小是 0）。Space 在 keyup 時點擊 checkbox、radio、按鈕，它的 keypress 是控制項的（不捲動）；Enter 在 keypress 時點擊按鈕（第 19 條）；方向鍵不在 radio 之間移動。
- **`form.reset()`**：分派 trusted、冒泡、能取消的 `reset`，沒被取消就對 form owner 是它的 input、output、select、textarea 跑 reset algorithm；locked for reset 擋住 listener 裡的 `reset()`。
- **`input` 和 `change`**：只有 activation 分派；script 設 `value`、`checked` 不分派（照規範），使用者打字、選下拉選單還沒有（第 5、6 條）。

### 4. 選擇器

- `:checked`、`:indeterminate`、`:disabled`、`:enabled`、`:default`、`:placeholder-shown` 是 `dom` 的 `ElementState` 的 bit（`u16`），`style` 照 bit 比對、照 RFC 0022 的失效只在規則用到時重算（第 4、25 條）。
- **誰算、什麼時候算**（第 25 條）：`dom` 記下可能改變它們的事（`FormStateChange`：建立、`set_form_control`、form owner、`<textarea>` 的文字、插入、移除；attribute 的由 `dom_api` 的 hook 記），`dom_api::form_states` 在 `update_elements` 的最後、renderer 每個 frame 和 hit testing 之前重算，擴大到受影響的元素（radio 和 submit 按鈕的整棵樹、fieldset、select、optgroup 的後代、fieldset 的第一個 legend）。Blink 在比對時問元素、在每個改變的地方 `PseudoStateChanged`；kun 的 `style` 只看得到 `dom`，規則在 `dom_api`。
- 定義照 HTML：整組都沒勾選的 radio 和沒有 `value` 的 `<progress>` 也是 `:indeterminate`；form 的 default button 是樹順序第一個 submit 按鈕；disabled 的 select 裡的 option 和 optgroup 也是 `:disabled`（whatwg/html#12205，2026-05，只影響 pseudo-class）；空的 `placeholder` 也算。
- `:read-only` / `:read-write`、`:required` / `:optional`、`:valid` / `:invalid`、`:in-range` 和 `::placeholder` 不在這份。

### 5. 和 process 的邊界

沒有新的訊息：控制項的狀態和 pseudo-class 都在 renderer 裡，renderer 在畫 frame 和 hit testing 前呼叫 `dom_api::update_form_states`。

### 測試

- **語意**（`crates/dom_api/tests/forms.rs`、`focus.rs` 和單元測試，不經過 engine）：每個 interface 的成員和 reflection、form owner 和集合、狀態、radio group、select、選取範圍、reset、pseudo-class。
- **bindings**（`dom_js`）和 **wasm**（`dom_wasm` 的 C guest、`kun:dom` 的 golden file、Lua runtime）。
- **parser**（`html`）：form element pointer 的關聯。
- **整個瀏覽器**（`crates/shell/tests/forms.rs`、`inserted_scripts.rs`）：使用者的點擊和按鍵、`select` 事件的 task、reset 按鈕和連結、pseudo-class 和 hit testing。
- **情境**：`form-pseudo-classes`（比 frame）、`demo-forms`（`about:demo`）。
- **WPT**：`reflection-forms`、`html/semantics/forms/` 不用 testdriver、不等送出的檔案、`html/semantics/selectors/pseudo-classes/` 和 `css/selectors/` 的表單 pseudo-class（`tests/wpt/README.md`）。
- **GC**：`test.py matrix` 和 `fuzz`（form owner 的 edge 經過 GC 的測試在 `dom_api` 裡）。
- **mutation test**：每一步都跑，共 323 條（`progress.md`）。

### 分階段

1. **interface 和 reflection**（1a、1b）：十四個元素、三個集合、form owner、`labels`、`new Option()`。
2. **`<input>` 和行為**（2a、2b）：value mode、dirty flag、checkedness、type change；checkbox 和 radio 的 activation 和事件；`<label>`；disabled；focus 和按鍵。M1 的 G 在這裡完成。
3. **`<select>`、`<textarea>`、reset**（3a、3b、3c）：selectedness、`HTMLOptionsCollection` 的修改、raw value、選取範圍和 `select` 事件、`reset()`、`<output>` 的值、按鈕。
4. **選擇器**：六個 pseudo-class 和失效。
5. **收尾**：`document.forms`、`about:demo` 和情境、文件的核對、`matrix` 和 `fuzz`。

## 缺點

- **控制項看不到**：網頁上的表單還是空白，使用者只能透過 `<label>`、鍵盤或頁面自己畫的樣式操作；在 G 的第二段之前，大部分的行為只有 script 和測試看得到。
- **控制項的資料讓 `dom` 的元素變大**：每個元素多一個指標（`Option<Box<...>>`）。
- **form owner 和 pseudo-class 的更新晚到呼叫結束**：script 看不出差別，但直接用 `dom` 改樹的 Rust 程式要呼叫 `update_elements`（`docs/limitations.md`）。
- **沒有使用者打字**：視窗裡的文字框不能輸入；Speedometer 不需要。

## 理由和其他做法

- **一份 RFC 做 G 的第一段整份**（第 1 條）：元素之間互相依賴（form owner、label、radio group、`<fieldset>` 的 disabled），分成好幾份時每份都要描述別份的介面；分段讓 M1 要的先做完。
- **照 Chromium 的分層**：狀態在元素上、行為照每種元素分，activation 接上 RFC 0022 的架構。和 Chromium 刻意不同的地方：form owner 和 select 的演算法晚到呼叫結束（第 10、21 條）、radio group 每次從樹找（第 18 條）、pseudo-class 存成 bit（第 25 條）、沒有 past names map（第 16 條）。
- 開始前每一項的比較見 `details.md`。

## 先例

- **Chromium**（`third_party/blink/renderer/core/html/forms/`）：
  - `html_input_element.cc`（value、checkedness、`UpdateType`）、`input_type.cc` 和 `checkbox_input_type.cc`、`radio_input_type.cc`（`WillDispatchClick` / `DidDispatchClick`）、`text_control_element.cc`（選取範圍）。
  - `html_form_control_element.cc`、`listed_element.cc`（form owner、disabled 和 `<fieldset>`）、`html_form_element.cc`（`elements`、`reset`）、`radio_button_group_scope.cc`。
  - `html_label_element.cc`、`html_select_element.cc`（`ResetToDefaultSelection`）、`html_option_element.cc`、`html_text_area_element.cc`、`html_field_set_element.cc`、`html_button_element.cc`、`html_output_element.cc`。
  - `core/css/selector_checker.cc`（`:checked`、`:disabled` 等讀元素的狀態）。
- **kun 自己的**：RFC 0022（activation 的三步、`ActivationState`、default event handler、focus、元素狀態和 style 的失效）、RFC 0010（`Collection`、`NodeList`）、RFC 0017（reflection 的 helper）、RFC 0014（宣告表、`tags`）。

## 已決定的事項

2026-10-09 確認（接受）：

1. **範圍是 G 的第一段整份**：十個元素和兩個 collection 的 DOM 和行為、控制項的狀態、checkbox 和 radio 的 activation、`<label>`、disabled、focus、`reset()`、表單的 pseudo-class；分五段，M1 要的在前兩段。元素之間互相依賴（form owner、label、radio group、`<fieldset>` 的 disabled），分成好幾份時每份都要描述別份的介面。不選只做 M1 要的（`<input>` 的 `value`、`checked` 和 checkbox 的 activation）、其他元素另開 RFC。（做完時是十四個元素、三個集合，第 11、15 條。）
2. **控制項的狀態放在 `dom` 的元素上**：一個不被 trace 的 `Option<Box<FormControlData>>`，`dom` 提供讀寫和 cloning 時的複製，規則在 `dom_api`。照 Blink（狀態是 `HTMLInputElement` 等的欄位），和 RFC 0022 的 `click_in_progress`、`ElementState` 同一個地方；元素被回收時一起不見。不選 `dom_api` 照 `NodeId` 查的表（要處理回收、cloning、`adoptNode`）。比較見 `details.md` 的「控制項的狀態」。
3. **表單送出不在這份**：`submit()`、`requestSubmit()`、`<button type=submit>` 和 implicit submission、`submit` 事件、`FormData` 之後另開 RFC。Speedometer 不用；它要 `net` 的 POST、經過 browser 的導覽和 `target`，WPT 多半要 iframe 和 `form-echo.py`。不選這份做 GET 的送出。
4. **表單的 pseudo-class 在這份**：`:checked`、`:indeterminate`、`:disabled`、`:enabled`、`:default`、`:placeholder-shown`，用 RFC 0022 的 `ElementState` 和它的第 14 條的失效；狀態是這份的，比對很小，RFC 0022 的 `:focus*` 也是這樣。路線圖的 S4 留 `:hover`、`:active` 和其他的。不選全部留給 S4。
5. **使用者輸入文字不在這份**：在文字框打字要 caret、選取和編輯，是路線圖的 P；控制項也還看不到（第 6 條）。script 設的 `value` 和選取範圍照樣有效。不選做最小的打字（收 `keypress` 的字、Backspace，沒有 caret）。
6. **控制項的繪製另開 RFC**：照路線圖，G 的第二段在 CSS 線（文字框的內部結構或 replaced 的畫法、`appearance`、`::placeholder`）；這份做完時控制項在畫面上還看不到，`about:demo` 和情境用 `<label>` 和 `:checked` 的樣式展示。不選這份先在 UA stylesheet 給控制項邊框和 `inline-block`（`<input>` 的值和勾還是看不到，到時要改掉）。（第 20 條後來為了 focus 給了 `inline-block`，沒有邊框。）
7. **選取範圍的 API 在這份，只有狀態**：`selectionStart`、`selectionEnd`、`selectionDirection`、`setSelectionRange`、`select()`、`setRangeText` 和 `select` 事件；M2 用 `selectionStart` / `selectionEnd`，WPT 的 `textfieldselection/` 13 個。沒有畫面，P 再接上。不選等 P 一起做。
8. **constraint validation 不在這份**：`validity`、`checkValidity()`、`required`、`:valid` / `:invalid` 之後；Speedometer 不用。不選這份做基本的部分。
9. **value sanitization 做文字類和 `number`、`range` 的基本規則**：去換行、`number` 不合法時是空字串、`range` 的預設值和夾在 `min` / `max` 之間；日期、時間、`color`、`email` 的之後。不選照規範全部做。（做的時候 `url` 和 `email` 去頭尾空白也一起做了，規則很短。）

2026-10-09 確認（第 1 段）：

10. **form owner 是 `dom` 的元素上的一條 edge，在每次呼叫結束時重設**：控制項插入、移除，`form` 和 `id` attribute 改變，`<form>` 插入、移除時，`dom` 記一筆 `ElementChange`，`dom_api` 在每次呼叫宣告表結束時、parser 的每一步之後照 HTML 的「reset the form owner」重設（和 iframe、script、autofocus 一樣）；parser 的關聯經過 html5ever 的 `associate_with_form`（kun 的 sink 補上），記成 parser 關聯的，照 HTML 保留。Blink 是 `ListedElement::form_`，在插入、移除的當下重設；kun 晚到呼叫結束，script 看不出差別，但直接用 `dom` 改樹的 Rust 程式要呼叫 `update_elements`。`form` attribute 讓任何元素的 `id` 改變都可能換掉 form owner：沒有帶 `form` attribute 的控制項時跳過，有時重算那棵樹裡帶它的控制項（Blink 用 `IdTargetObserverRegistry` 只通知相關的）。不選在 `dom` 的修改原語裡當場重設（`dom` 要知道表單的規則）；不選每次讀的時候從樹算（parser 的關聯和 form owner 的黏性做不出來）。（WPT 的 `Node-appendChild-script-and-button-from-div` 找到：同一次插入的 script 要看得到 form owner，`update_elements` 先做 form owner 的。）
11. **`<datalist>`、`<output>`、`<progress>`、`<meter>` 也有 interface 和 reflection**（沒有行為）：Chrome 都有，WPT 的 `html/dom/reflection-forms.html` 也檢查它們。不選維持一般的 `HTMLElement`。
12. **`new Option()` 在第 1 段，宣告表多 legacy factory function 的形式**：WebIDL 的 `[LegacyFactoryFunction]`（Blink 的 `HTMLOptionElement::CreateForJSConstructor`）：JS 是 window 上的 `Option`，`prototype` 是 `HTMLOptionElement.prototype`；`kun:dom` 是 `html-option-element` 的另一個建構函式。之後 H2 的 `Image` 也用它。不選等第 3 段；不選不做。
13. **`<a>` 的 `href` 不在這份**：`reflection-forms` 用 `<a>.href` 拆網址算 URL 類的預期值，那些子測試照 baseline 失敗；`href` 留給路線圖的 H2。不選這份順便加 `HTMLAnchorElement` 的 `href`（和表單無關）。

2026-10-09 確認（1b）：

14. **form owner 的變更在每一棵樹都記**：HTML 在分離的樹裡也有 form owner（`form.appendChild(input)` 之後 `input.form` 就是那個 form，不管 form 在不在 document 裡），而 iframe 和 script 的 `ElementChange` 只在有 browsing context 的 document 裡記。兩個開關省下沒有表單的頁面的成本：isolate 建過表單的元素之後才記插入和移除；有帶 `form` attribute 的控制項之後，`id` 的改變和帶 `id` 的元素的插入、移除才重算那棵樹裡帶它的控制項。移除時 `dom` 記下被移走的子樹的根和原本的 parent，`dom_api` 照 HTML 判斷誰離開了 form owner 的樹；form 被移走時，指著它、但不在它的子樹裡的控制項也重設（Blink 的 `HTMLFormElement::RemovedFrom`）。不選只在 browsing context 裡記：分離的樹裡的 `input.form` 永遠是 `null`，和規範、WPT 都不合。
15. **`RadioNodeList` 在 1b**：`form.elements.namedItem(name)` 有不只一個時照規範回傳它（Blink 的 `RadioNodeList`，live 的 `NodeList`）；它的 `value` 要 radio 的 checkedness，第 2 段才加。不選整個留到第 2 段、1b 先回傳第一個：和規範不同，第 2 段還要改回傳的型別。（`value` 最後沒有做，記在 `docs/limitations.md`。）
16. **`<form>` 的 indexed 和 named getter 在 1b，沒有 past names map**：`form[0]`、`form.username`（舊網站常用）照 HTML 的 `HTMLFormElement` 的 supported property names。past names map（控制項改名或移走之後，`form.舊名字` 還拿得到它，Blink 的 `past_names_map_`）不做：要一張會長大、GC 追蹤的名字到元素的表，`dom` 沒有這種結構；Speedometer 和一般的用法都不靠它，記進 `docs/limitations.md`。不選連它一起做（`dom` 多一種 GC 結構和它的論證）；不選兩個 getter 都不做。

2026-10-09 確認（第 2 段）：

17. **控制項的狀態第一次需要時才配置，attribute 的改變在 `handlers::attribute_changed` 當場套用**：`FormControlData` 照元素分種類（`<input>` 的 value、dirty value flag、checkedness、dirty checkedness flag、indeterminate 和上次的 value mode；`<textarea>`、`<option>`、`<output>`、`<form>` 的在第 3 段）。script 設 `value`、`checked`，使用者點擊，radio 被同一組的取消勾選時才配置；還沒有時，狀態就是從 attribute 算出的（value 是 `value` attribute、checkedness 是有沒有 `checked`），沒碰過的控制項不多佔記憶體。HTML 的 attribute change steps（沒有 dirty 時 `checked`、`value` 跟著改，`type` 的 type change steps，`name` 和 radio group）在 iframe、script 已經用的入口當場做：`setAttribute`、`Attr.value`、reflection、parser 都經過它。cloning steps 在 `dom_api` 的 `clone_node`。Blink 每個 `<input>` 建立時就有這些欄位（`HTMLInputElement`、`InputType`），`ParseAttribute` 當場更新；差別只在晚一點配置。不選建立時就配置（每個控制項多一次配置）。
18. **radio group 每次從樹走一遍找**：同一個 form owner（或都沒有、在同一棵樹）、相同而非空的 `name`；只在 radio 被勾選、插入、改名、換 form owner 或 `type` 時找。Blink 的 `RadioButtonGroupScope` 維護名字到一組的表；不選它：多一份要保持一致的狀態，表裡的元素要 GC 追蹤。（插入只在 connected 時取消同一組的，WPT 的 `radio-disconnected-group-owner` 找到。）
19. **鍵盤的 activation 做 Space 和 Enter，不做方向鍵**：checkbox、radio、button 有 focus 時，Space 在 keyup 時點擊（Blink 的 `BaseCheckableInputType::HandleKeyupEvent`、`HTMLButtonElement::DefaultEventHandler`），button 的 Enter 在 keypress 時點擊；方向鍵在同一組 radio 之間移動不在這份，記進 `docs/limitations.md`。不選連方向鍵一起做；不選都不做（能 focus 卻不能用鍵盤操作，和 Chrome 不同）。（照 Blink，focus 在 button 裡的元素時按鍵也點擊 button；renderer 照 keypress 有沒有被處理決定要不要捲動。）
20. **`input`、`textarea`、`select`、`button` 在 UA stylesheet 是 `display: inline-block`**（2026-10-09，第 2 段做的時候發現）：HTML 的 focusable area 要 being rendered，kun 以有沒有 box 判斷；控制項是空的 inline 時沒有 box，`focus()`、Tab、按鍵都沒有作用。照 Chrome 的 `html.css` 給它們 `display: inline-block`，畫出來之前（第 6 條，另一份 RFC）大小是 0，有 box、能 focus。一行裡只有空的控制項時那一行有行高（空的 inline-block 照 CSS 產生 line box），比較接近 Chrome（它的控制項還有固有大小）。不選 focus 的判斷對控制項例外（沒有 box 就沒有捲進畫面和 focus ring 的位置，和 Chrome 的模型不同）；不選等繪製的 RFC（這一段的 focus 在一般頁面上沒有作用）。

2026-10-09 確認（第 3 段）：

21. **`<select>` 的 selectedness setting algorithm 在呼叫結束時對受影響的 select 跑**：和 form owner 一樣（第 10 條），`<option>` 的插入、移除經過 `dom` 已經記下的 `FormChange`，`dom_api` 在呼叫結束時、parser 的每一步之後跑；`selected`、`multiple`、`size` attribute 的改變和 `option.selected` 的 setter 在 attribute 的 hook 裡當場跑。Blink 在 `OptionInserted` / `OptionRemoved` 當場做；script 看不出差別。不選讀的時候才從存的狀態推（規範的演算法會改狀態，推出來的在一連串插入、移除之後和規範不同）。（WPT 找到：option 和 optgroup 的 `disabled` 也要重跑；一次插入好幾個 selected 的留最後一個，Chrome 的 `ResetToDefaultSelection` 也是。）
22. **`select` 事件排成 task**：`dom` 記下要發的事件（元素和名稱），renderer 在每個 task 之後把它們各自當一個 task 分派（和 `fire_link_events` 一樣）；HTML 的「queue an element task」，Chrome 也非同步。之後的 `toggle` 等也能用。不選在呼叫結束時同步發（listener 在呼叫的 script 繼續之前就跑，和 Chrome 的順序不同）；不選不發。
23. **按鈕只在會送出或重設時是 activation target**（第 3c 段）：`<button>`（預設 `submit`）和 `submit`、`reset`、`image` 的 `<input>` 有 form owner、不是 disabled 時才有 activation behavior；`reset` 重設 form owner，`submit` 和 `image` 什麼都不做（第 3 條）。和 Chrome 一樣：`HTMLButtonElement::DefaultEventHandler`、`SubmitInputType` / `ResetInputType` / `ImageInputType` 的 `HandleDOMActivateEvent` 只在有 form、不是 disabled 時 `SetDefaultHandled`，外層的 `<a>` 才不處理那個 click；所以 `<a href>` 裡沒有 form 的 `<button>` 照樣跟隨連結（網頁常這樣寫）。不選照 HTML 讓每個按鈕都有 activation behavior（那些連結就失效了，和瀏覽器不同）。
24. **`document.forms` 在第 5 段加**（3c 做完時）：WPT 的 `resetting-a-form/reset-form.html`、`reset-form-event-realm.html`、`the-button-element/button-events.html` 在任何結果之前就讀它，表單的測試常用。原本不在範圍裡（第 1 條）。不選留給之後的 RFC。

2026-10-09 確認（第 4 段）：

25. **表單的 pseudo-class 存成 `ElementState` 的 bit，由 `dom_api` 在改變之後重算**：`dom` 記下表單狀態可能變了的元素（`set_form_control`、form owner、相關的 attribute、表單元素的建立、插入和移除、`<textarea>` 的文字），`dom_api` 在每次呼叫宣告表結束時、parser 的每一步之後（`update_elements`，和第 10 條一樣），以及 renderer 算樣式和 hit testing 之前（使用者的點擊不經過宣告表）重算它們，擴大到受影響的元素（同一棵樹的 radio 和按鈕、`<fieldset>`、`<select>` 和 `<optgroup>` 的後代），bit 變了才標記樣式。Blink 在比對時呼叫元素的方法（`IsChecked()`、`IsDisabledFormControl()`、`MatchesDefaultPseudoClass()`），在每個改變的地方 `PseudoStateChanged`；kun 的 `style` 只看得到 `dom`，規則在 `dom_api`（第 2 條），所以存結果。`:indeterminate` 照 HTML 和 Blink 包括整組都沒勾選的 radio 和沒有 `value` 的 `<progress>`；`:default` 包括 form 的 default button（送出還沒做，第 3 條）；`ElementState` 擴大成 `u16`。不選比對時從 `style` 回呼 `dom_api`（一樣要找出誰可能變了，還多一個跨 crate 的 hook）；不選每次呼叫結束時重算整份 document（每次 O(n)）。（radio group 的 key 要含樹：移出 document 的勾選的 radio 不算原本那一組的。）

2026-10-09 確認（第 5 段）：

26. **`document.forms` 是 document 上快取的同一個 `HTMLCollection`**：document 的 rare data 裡的集合（和 `elements`、`labels` 一樣），document 裡的 HTML `<form>`，照樹的順序，live；名稱照 `HTMLCollection` 的規則（`id`、`name`）。WebIDL 是 `[SameObject]`，Blink 的 `Document::forms()` 也是快取的 collection（`kDocForms`），WPT 檢查 `document.forms === document.forms`。不選每次建新的（像 kun 的 `getElementsByTagName`，不符合 `[SameObject]`）。
27. **`about:demo` 的表單示範由頁面的樣式讓狀態看得到**：控制項還畫不出來（第 6 條），頁面自己給它們大小、邊框，`:checked`、`:disabled` 換背景色（標明是頁面的樣式，不是 UA 畫的）；checkbox 和它的 label、一組 radio、disabled 的 `<fieldset>`、reset 按鈕，一行由 `change`、`reset` 更新的狀態；情境 `demo-forms`。不選只放 label 和狀態文字（看不出控制項的狀態）。

## 待決定

沒有。

## 之後的可能

- **G 的第二段：控制項的繪製**（CSS 線）：文字框、checkbox、radio、按鈕、`<select>`，`appearance`、`::placeholder`、`accent-color`；之後才有使用者在視窗裡點得到、看得到的表單。
- **表單送出**：`submit()`、`requestSubmit()`、`submit` 事件、implicit submission、`formdata`、`FormData`，GET 和 POST（`net` 要 POST 的 body），`target`；submit 按鈕的 activation 已經是 activation target，接上就好。
- **constraint validation**：`validity`、`checkValidity()`、`reportValidity()`、`:valid` / `:invalid`、`invalid` 事件。
- **編輯（P）**：使用者在文字框打字、caret、`beforeinput`、IME；選取範圍 API 的狀態已經在。
- **還沒做的小東西**（`docs/limitations.md`）：`RadioNodeList.value`、`autocomplete` 的正規化、日期和 `color` 的 sanitization、`valueAsNumber` 等、方向鍵在 radio 之間移動、past names map。
- **狀態的恢復**：上一頁時恢復輸入過的值（Blink 的 `FormController`）。
- **testdriver**：RFC 0021 的 WebDriver 做完後，`html/semantics/forms/` 用 testdriver 的 123 個可以跑。
