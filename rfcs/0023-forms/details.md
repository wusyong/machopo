# RFC 0023 的細節：表單（G）

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## kun 現在

2026-10-09，`main` 的 `32adc89`。

- **interface**：宣告表的 HTML interface 只有 `HTMLElement`、`HTMLUnknownElement`、`HTMLMetaElement`、`HTMLTitleElement`、`HTMLIFrameElement`、`HTMLLinkElement`、`HTMLScriptElement`（`crates/dom_api/src/interfaces/core_html.rs`）。`input`、`label` 等在 `html_element.rs` 的 `HTML_ELEMENTS` 裡，所以是 `HTMLElement`（不是 `HTMLUnknownElement`）；`html_element::element_kind` 照 `INTERFACES` 的 `html_tags` 找，新的 interface 宣告 `tags("input")` 就接上，新的 `ObjectKind` 加在 HTML 那一段的最後。
- **activation**（`crates/dom_api/src/activation.rs`）：`ActivationBehavior` 的三個方法（`legacy_pre_activation` 回傳 `Option<ActivationState>`、`legacy_canceled_activation` 拿回它、`activation`）、`DefaultEventHandler`；`activation_behavior` 和 `default_event_handler` 是照元素查的 if 鏈，只有 `<a>` 和測試用的元素。`ActivationState` 的註解已經寫了「a checkbox's checkedness, with the forms」。分派只把 `MouseEvent` 的 `click` 交給 activation target（`event.rs`）。
- **focus**（`focus.rs`）：`supports_focus` 只有合法的 `tabindex` 和 `<a href>`；`tabIndex` 的預設值對 `button`、`input`、`select`、`textarea` 等回傳 0（照 HTML），和「不能 focus」不一致。
- **元素狀態**：`ElementState` 只有 `FOCUS`、`FOCUS_WITHIN`、`FOCUS_VISIBLE`；`style` 的 `PseudoClass` 認得 `link`、`any-link`、`visited`、`hover`、`active`、`focus`、`focus-visible`、`focus-within`，其他的是解析錯誤。
- **繪製**：UA stylesheet（`crates/style/resources/html.css`）只有 `form { display: block }` 和 `fieldset`、`legend` 的 display、margin、padding；控制項是預設的 `inline`，`<input>` 是空的 void element、什麼都不畫；layout 的 replaced element 只有 `img`、`iframe`。
- **事件**：`InputEvent` 只有 interface；`onchange`、`oninput`、`onsubmit` 有 IDL attribute，但沒有任何地方分派 `input`、`change`、`submit`。html5ever 算出 fragment 的 form element pointer，沒有人用。
- **reflection**：沒有 `[Reflect]`，用 getter / setter 呼叫 helper（`html_element.rs` 的 `reflect`、`set_reflected`、`set_reflected_bool`、`reflect_url`）；enumerated attribute 有一個例子（`crossOrigin`）。

## Speedometer 的盤點

來源：`rfcs/0012-speedometer/README.md`、`details.md`、`roadmap.md` 和 RFC 0022 的 `details.md`（Speedometer `release/3.1` 的 `1386415`）。本機沒有 Speedometer 的 checkout，這裡只整理那幾份文件。

- **runner 的 `PageElement`**：`setValue` 設 `value`；`click()`、`focus()`；`dispatchEvent` 用 `new Event(...)`（`submit` 例外：`createEvent` 加 `initEvent`）；`enter` 和 `dispatchKeyEvent` 用 `new KeyboardEvent(...)`。
- **M1**：
  - TodoMVC-JavaScript-ES5：`setValue`、`dispatchEvent("change")` 加入 todo（`.new-todo` 沒有 `keypress` listener），`click()` 點 `.toggle`、`.destroy`；`.toggle` 的 delegated `click` listener 讀 `this.checked`。
  - Preact-Complex-DOM：`.new-todo` 的 `onKeyDown` 讀 `e.key`，`.toggle` 是 `onChange`。
  - Svelte-Complex-DOM：`keydown`、`change`（讀 `target.checked`）、`click`；mount 時 `focus()`、`autofocus`。
  - 三個都在 `prepare` 對 `.new-todo` 呼叫 `focus()`。
- **M2 的成員**（18 個 suite 的紀錄）：`HTMLInputElement` 的 `type`、`name`、`defaultValue`、`defaultChecked`、`disabled`、`placeholder`、`autocomplete`、`min`、`max`、`step`、`selectionStart`、`selectionEnd`；`HTMLButtonElement.disabled`；`HTMLFieldSetElement.disabled`；`HTMLLabelElement.htmlFor`；`HTMLSelectElement` 的 `options`、`value`、`multiple`、`disabled`，`HTMLOptionsCollection.length`；`HTMLOptionElement`（和 `Option`）的 `selected`、`value`；`HTMLTextAreaElement.defaultValue`。`createElement` 建的有 `input`、`label`、`button`、`select`、`option`、`fieldset`。runner 在 5 個 suite 分派 `change`、`input`，6 個分派 `keydown`。
- **沒有用到的**：`HTMLFormElement`、`form.submit()`、`reset()`、`requestSubmit()`、`selectedIndex`、`labels`、`form`。
- **樣式**：`:checked` 14 個 suite、`::-webkit-input-placeholder` 11、`::placeholder` 7、`:disabled` / `:enabled` / `:read-only` 6、`:placeholder-shown` 1；`appearance` 14。

## WPT 的調查

2026-10-09 在 `../third_party/wpt` 數 testharness.js 的檔案（不含 reftest、`-ref`、manual、crashtest、`resources/`、`support/`）。kun 的 runner 沒有 testdriver（RFC 0021 之後才有）、`fetch`、`window.open`、shadow DOM。

| 目錄（`html/semantics/` 底下） | 檔案 | 用 testdriver | 不用的 |
| --- | --- | --- | --- |
| `forms/`（全部） | 487 | 123 | 364 |
| `forms/the-input-element/` | 127 | 30 | 97 |
| `forms/the-select-element/` | 129 | 72 | 57 |
| `forms/form-submission-0/` | 39 | 1 | 38 |
| `forms/constraints/` | 29 | 1 | 28 |
| `forms/the-button-element/` | 23 | 2 | 21 |
| `forms/the-form-element/` | 21 | 0 | 21 |
| `forms/textfieldselection/` | 15 | 2 | 13 |
| `forms/the-textarea-element/` | 15 | 2 | 13 |
| `forms/the-label-element/` | 14 | 3 | 11 |
| `forms/the-fieldset-element/` | 11 | 0 | 11 |
| `forms/the-option-element/` | 11 | 0 | 11 |
| 其他（`attributes-common-to-form-controls/`、`form-submission-target/`、`resetting-a-form/` 等） | 42 | 10 | 32 |
| `selectors/pseudo-classes/` | 31 | 2 | 29 |

- `the-select-element/` 有 86 個在 `customizable-select*/`、`filterable-select/`（`appearance: base-select`，幾乎都用 testdriver），有用的是一般的 `select-*.html`。
- 不用 testdriver 的 393 個裡：沒有 `fetch`，1 個 `window.open`，7 個 shadow DOM，42 個用 iframe，67 個提到送出（`.submit(`、`requestSubmit`、`type=submit`）。`form-submission-0/`、`form-submission-target/`（44 個能跑的）和 `the-form-element/form-action-*`、`form-requestsubmit`、`the-button-element/button-click-submits*` 屬於表單送出（第 3 條）。
- 扣掉送出和 constraint validation，這份的範圍約 280 個。
- **`html/dom/reflection-forms.html`**：不用 testdriver，一份檢查 `form`、`fieldset`、`legend`、`label`、`input`、`button`、`select`、`datalist`、`optgroup`、`option`、`textarea`、`output`、`progress`、`meter` 的 reflection（還有全域的 `title`、`lang`、`dir`、`className`、`hidden`、`inert`、`accessKey`、`tabIndex`），要 string、boolean、enum、long、unsigned long、double、tokenlist、url 的規則；url 的預期值經過 `<a>` 的 `href` 和網址的分解算出來（kun 的 `<a>` 還沒有 `href` 的 IDL attribute，RFC 0022 的 `docs/limitations.md`），子測試幾千個，照 baseline 收。
- **先收的**：checkbox 和 radio 的 activation（`the-input-element/checkbox-click-events.html`、`radio-click-events.html`、`checkbox.html`、`radio.html`、`input-type-checkbox.html`、`radio-groupname-case.html`、`radio-multiple-selected.html`、`radio-disconnected-group-owner.html`、`forms/input-change-event-properties.html`）；`type` 和 `value`（`type-change-state.html`、`valueMode.html`、`input-value-dirty-flag.html`、`default-type.html`、`defaultValue-clobbering.html`、`cloning-steps.html`）；`textfieldselection/`；label（`labelable-elements.html`、`labels-live-*.html`、`proxy-click-to-associated-element.html`）；`option-*.html`、`select-value.html`、`selected-index.html`、`select-add.html`、`select-remove.html`；`the-fieldset-element/`；`the-form-element/form-elements-*.html`、`form-length.html`；pseudo-class 的 `checked.html`、`disabled.html`、`enabled.html`、`default.html`、`indeterminate.html`、`checked-type-change.html`、`placeholder-shown-type-change.html`。

## 控制項的狀態

第 2 條的兩個做法：

| 做法 | 說明 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **元素上**（建議） | `dom` 的元素多一個不被 trace 的 `Option<Box<FormControlData>>`，`dom` 提供讀寫和 cloning 時的複製 | 照 Blink（狀態是元素的欄位）；元素被回收時一起不見；cloning steps 在 `dom` 的 clone 裡照樣做；和 RFC 0022 的 `click_in_progress`、`ElementState` 同一個地方 | `dom` 知道「控制項的資料」這個形狀（但不知道規則：規則在 `dom_api`）；每個元素多一個指標 |
| `dom_api` 的表 | `dom_api` 照 `NodeId` 存一張表 | `dom` 不動 | 要處理元素被回收（`NodeId` 不重用，但表會一直長）、cloning（`dom_api` 要攔到 clone）、跨 frame 的 `adoptNode`；和 RFC 0022 存在元素上的做法不一致 |

## 參考資料

- HTML：表單（https://html.spec.whatwg.org/multipage/forms.html）、`input` 元素和它的 type、form owner、radio button group、`label`、`select` 的 selectedness setting algorithm、文字控制項的選取 API、`reset`。
- Selectors 4 和 HTML 的 pseudo-class 定義（https://html.spec.whatwg.org/multipage/semantics-other.html#pseudo-classes）。
- Chromium：`third_party/blink/renderer/core/html/forms/`、`core/css/selector_checker.cc`。
