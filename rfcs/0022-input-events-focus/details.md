# RFC 0022 的細節

## kun 現在

2026-10-08 盤點 `main`（RFC 0017 做完時）。

### 輸入的路徑

- **shell**：`window.rs` 的 `window_event` 把 winit 的事件交給 `BrowserView` 的 `pointer_moved`、`mouse_input`、`wheel`、`key`、`set_modifiers`。按鍵只處理按下（`ElementState::Pressed`），放開的丟掉；滑鼠的 Back / Forward 鍵是上一頁、下一頁。
- **按鍵的路由**（`BrowserView::key`）：先比對瀏覽器的快捷鍵（`command_for`：F5、F6、Ctrl/Cmd+L/T/W/R、切換分頁、上一頁）；網址列有 focus 時交給它；按著 Cmd、Ctrl 或 Alt 時丟掉；其他只有 9 個會捲動的鍵（方向鍵、PageUp / PageDown、Space、Home、End）轉成 `InputEvent::Key { key, shift }` 送給最後一個被點的網頁 view（`BrowserView::focused`，aura 的 `FocusClient`）。文字鍵和 Tab 都不送；IME 只給網址列。
- **browser**：`WebContents::forward_input_event` → `RenderProcessHost::send_input_event` → `CompositorMsg::HandleInputEvent`；renderer 回 `InputEventAck`。browser 不告訴 renderer focus 的狀態，只有 `WasShown` / `WasHidden`。
- **`protocol::input::InputEvent`**：`MouseMove { x, y }`（沒有修飾鍵）、`MouseDown` / `MouseUp`（`button`、`Modifiers { shift, ctrl, alt, meta }`）、`MouseWheel { x, y, delta }`（沒有修飾鍵）、`Key { key, shift }`（9 個鍵）。
- **compositor thread**（`compositor::input_handler::handle_input_event`）：滑鼠一律交給 main thread（`DidNotHandle`）；滾輪在能捲的 layer 上自己捲，否則交給 main；**按鍵在 compositor 捲 viewport，從不送到 main thread**（註解寫明之後 `keydown` 要先到 main thread）。
- **main thread**（`render_thread.rs` 的 `handle_forwarded_input` → `handle_input_event`，先 `update_lifecycle_to_pre_paint`）：`MouseMove` 只決定游標（`link_at`、`SetCursor`）；`MouseDown` 記下按在哪個元素（`FrameView::mouse_down`）；`MouseUp` 找兩處的共同祖先（`FrameView::mouse_up`），`dispatch_click` 分派 trusted 的 `click`（左鍵）或 `auxclick`（中鍵、右鍵），`clientX/Y`、`button`、修飾鍵；`MouseWheel` 由 `FrameView::wheel` 直接捲；`Key` 什麼都不做。
- **hit test**：`FrameView::element_at`（`paint::hit_test` 加上 viewport 的捲動）；`elementFromPoint` 走 RFC 0018 的 `hit_test_start`。不看 `pointer-events`、`visibility`。
- **捲動量**：`compositor::scroll_utils` 的 `wheel_scroll_delta`（一行 40 px）、`key_scroll_delta`（方向鍵一行、PageUp / PageDown 和 Space 捲 87.5% 的 viewport、Home / End 到頭尾）。

### 事件和 API

- `dom::EventKind`：Plain、Ui、Mouse、Custom、Error、PromiseRejection、HashChange、PopState；`MouseEventData` 有 `client_x/y`、`button`、四個修飾鍵。`create_event` 收 `trusted`。
- 宣告表（`core_events.rs`）：`Event`、`UIEvent`（空的，不能建構）、`MouseEvent`（建構子、`clientX/Y`、`button`、四個修飾鍵）、`CustomEvent`、`ErrorEvent`、`PopStateEvent`、`HashChangeEvent`、`PromiseRejectionEvent`；`UIEventInit`（空的）、`EventModifierInit`（四個修飾鍵）、`MouseEventInit`（`clientX/Y`、`button`）。`createEvent` 認得 `Event`、`UIEvent`、`MouseEvent`、`CustomEvent`。
- event handler（`dom_api::handlers`）：`onclick`、`onload`、`onerror`、`onscroll`、`onhashchange`、`onpopstate`。
- activation behavior（`dom_api::event` 的 `activation_target`、`has_activation_behavior`、`activation_behavior`）：`click`，或 trusted 的中鍵 `auxclick`；只有 `<a>`，經過 `FrameClient::follow_link`。
- `spec-missing.txt` 裡有：`Document.activeElement`、`hasFocus()`、`HTMLElement.autofocus`、`blur()`、`click()`、`focus()`、`hidden`、`inert`、`tabIndex`；`MouseEvent` 的 `screenX/Y`、`pageX/Y`、`offsetX/Y`、`x/y`、`layerX/Y`、`buttons`、`relatedTarget`、`getModifierState()`、`initMouseEvent()`；`UIEvent` 的 `view`、`detail`、`which`、`initUIEvent()`。

### focus、狀態和編輯

- 頁面裡沒有 focus：沒有 focused element、`activeElement`、`tabIndex`、`autofocus`、循序導覽。只有 shell 有：網址列的 `focused`、`BrowserView::focused`（最後被點的網頁 view）、Cmd/Ctrl+L、Alt+D、F6 移到網址列，焦點環只畫在網址列。
- `style` 的 `PseudoClass` 解析 `:hover`、`:active`、`:focus`、`:focus-visible`、`:focus-within`，但 `match_non_ts_pseudo_class` 除了 `:link` / `:any-link` 一律不成立；節點上沒有元素的狀態（`StyleFlags` 只是 dirty bit）。
- 沒有 caret、`contenteditable`、`Selection`、`<input>` 的 interface；沒有 `outline`。

### 測試和文件

- 輸入：`shell/tests/script.rs` 的 `a_click_is_an_event`、`browser_view.rs`、`windowless.rs`、`browser/tests/navigation.rs`、`renderer::scroll`、`compositor::proxy_impl` 的按鍵和滾輪；情境的 `click`、`click-at`、`wheel`、`key`、`type`（`automation::Input`）。
- `docs/limitations.md`：鍵盤只捲 viewport、沒有 focus（「捲動」）；事件的介面、使用者的輸入只分派 `click`（「事件」）；iframe 裡按鍵捲最上層；`:hover` / `:active` 不成立、不能用 Tab 和 Enter（「連結導航」）；沒有 `mouseout`、Tab（「工具列」）。「事件」有兩句過時：`MouseEvent` 已經有修飾鍵，右鍵和中鍵已經分派 `auxclick`。

## M1 的核對

2026-10-08 讀 Speedometer 3.1（`release/3.1`）的原始碼。

| 誰 | 用到 | 沒有時 | 哪個里程碑 |
| --- | --- | --- | --- |
| runner 的 `PageElement` | `click()`、`focus()`；`dispatchKeyEvent` 用 runner 自己 realm 的 `new KeyboardEvent(type, {bubbles, cancelable, keyCode, which: keyCode, key})` | `TypeError` / `ReferenceError`，suite 中止 | M1 |
| runner 的 `dispatchMouseEvent` | `new contentWindow.MouseEvent(...)` 的 `clientX/Y`、`screenX/Y`；WarmupSuite 傳 `WheelEvent` | M1 的三個 suite 不用 | M2 |
| 三個 suite 的 `prepare` | `.new-todo` 的 `focus()` | 同 runner | M1 |
| TodoMVC-JavaScript-ES5 | 新增靠 `change`（`.new-todo` 沒有 `keypress` 的 listener，`enter("keypress")` 只要能建構）；`.toggle` 靠委派的 `click` listener 讀 `this.checked`；`.destroy` 靠 `click` | `checked` 沒有時永遠沒勾選：跑得完，量的工作少了（G） | M1（`checked` 在 G） |
| TodoMVC-Preact-Complex-DOM | `.new-todo` 的 `onKeyDown` 讀 `e.key`；`.toggle` 的 `onChange`；`.destroy` 的 `onClick`；Preact 用 `"onkeydown" in element` 決定事件的名稱 | 元素上沒有 `onkeydown` / `onchange` 時註冊成 `"KeyDown"` / `"Change"`，永遠不觸發：一個 todo 都加不進去，跑得完但量的是空的 | M1 |
| TodoMVC-Svelte-Complex-DOM | `addEventListener` 的 `keydown`（`e.key`）、`change`（`target.checked`）、`click`；mount 時 `focus()`、設 `autofocus` | 同上 | M1 |
| 編輯的流程（三個 suite） | `dblclick`、`blur`、`focus`（capture）、`keypress` 的 `keyCode` | 步驟走不到 | M2 |

M1 要的：`KeyboardEvent` 的建構子和 `key`（`keyCode`、`which` 只要收 init）、`click()`（冒泡的合成 click）、`focus()` 可以呼叫、元素上有 `onkeydown`、`onchange`。`.toggle` 的勾選另外要 G 的 checkbox。

## 事件的 interface

| interface | 成員 | 備註 |
| --- | --- | --- |
| `UIEvent` | 建構子、`view`、`detail`、`which`、`initUIEvent` | `which`：滑鼠事件是 `button + 1`，按鍵事件是 `keyCode` / `charCode`（Chrome） |
| `MouseEvent` | `screenX/Y`、`pageX/Y`、`offsetX/Y`、`x/y`、`movementX/Y`、`buttons`、`relatedTarget`、`getModifierState()`、`initMouseEvent` | `pageX` = `clientX` + 捲動；`offsetX` 相對 target 的 padding box，讀時算（Blink 的 `MouseEvent::offsetX` 也是，要 layout）；`screenX` 由 browser 給視窗的位置（沒有時等於 client） |
| `KeyboardEvent` | `key`、`code`、`location`、常數、`repeat`、`isComposing`、修飾鍵、`getModifierState()`、`keyCode`、`charCode`、`which`、`initKeyboardEvent` | Chrome 的建構子收 `keyCode`、`charCode`、`which` 的 init（不在規範的 IDL 裡，WPT 的 `keyboardevent-legacy` 照它） |
| `FocusEvent` | `relatedTarget` | `relatedTarget` 照 `target` 的規則 retarget |
| `WheelEvent` | `deltaX/Y/Z`、`deltaMode`、`DOM_DELTA_*` | 使用者的滾輪是 `DOM_DELTA_PIXEL`（Chrome） |
| `InputEvent` | `data`、`inputType`、`isComposing` | 只有 interface，編輯不在這份 |

`getModifierState` 認得 `Shift`、`Control`、`Alt`、`Meta`，其他（`CapsLock` 等）是 false。

## GlobalEventHandlers 的範圍

| 做法 | 說明 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **整組一次加上**（第 2 條） | HTML 的 GlobalEventHandlers 約 90 個 `on*` 都在 `HTMLElement`、`Document`、`Window` | 和 Chrome 一樣，`"onX" in element` 的偵測都對；機制一樣，多的只是宣告 | 大部分事件 kun 還不會分派，偵測會以為支援 |
| 只加這份和 G 會分派的 | `onkeydown`、`onkeyup`、`onkeypress`、`onfocus`、`onblur`、`onwheel`、`oninput`、`onchange`、`ondblclick`、`onmousedown`…… | 偵測和實際一致 | Preact 這類以 `in` 決定事件名稱的框架，碰到其他事件（`ontransitionend` 等）時走錯路；之後每加一種事件都要回來加 |

## focus 模型

- **Blink**：`Document::focused_element_`（`Member<Element>`）；`FocusController` 有 page 的 focus（`is_focused_`、`is_active_`）和 focused frame；`Element::focus(FocusParams)` 檢查 `IsFocusable`（`SupportsFocus` 加上有 layout box、不是 inert），`Document::SetFocusedElement` 分派 `blur` / `focusout` / `focus` / `focusin`；`Document::RemoveFocusedElementOfSubtree` 在元素被移走時清掉 focus（現在照 HTML 的 focus fixup 延到 rendering update）。
- **kun 的對應**：document 的資料多一條 focused element 的 edge（`dom`，強指著，和 Blink 一樣；元素被移出 document 時由 focus fixup 清掉）；page 的 focus 是 `FrameView` 的狀態，由 `SetFocus` 改；`dom_api` 的 `focus` 模組放 HTML 的 focusing steps 和 unfocusing steps。
- **focusable**（這份）：有 `tabindex` 的元素、有 `href` 的 `<a>`、有 layout box（`display: none` 的不能 focus，照 Blink 要 layout 是最新的）；G 加上表單元素。
- **事件的順序**（UI Events）：舊的元素 `blur`、`focusout`，新的元素 `focus`、`focusin`；`relatedTarget` 互指；page 失去 focus 時只有 `blur` / `focusout`（`relatedTarget` 是 null）、window 的 `blur`。
- **元素的狀態**：`dom` 的元素多一組 flag（`focused`、`focus_within`、`focus_visible`，之後 `hover`、`active`），改變時標記自己和受影響的祖先要重算 style（Blink 的 `Element::FocusStateChanged`、`FocusWithinStateChanged`）。

### focus 和 iframe

Chrome：focused frame 是 page 的狀態，`focus()` 一個子 frame 裡的元素也讓那個 frame 變成 focused frame，`document.activeElement` 在父 frame 是 `<iframe>`。這份照做到同一個 renderer 裡的 frame（kun 的子 frame 都在同一個 renderer），跨 process 的 focus 不在範圍（RFC 0004 延後）。按鍵送到 focused frame（現在一律捲最上層，`docs/limitations.md` 的「iframe」）。

## 按鍵要不要先到 main thread

| 做法 | 說明 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **一律先到 main thread**（第 4 條） | compositor 不再自己捲；main thread 分派 `keydown`，沒被取消才捲（Blink 的 `KeyboardEventManager::DefaultKeyboardEventHandler` → `ScrollManager`） | 照 Chrome；`preventDefault` 擋得掉；Tab、Enter 等預設動作在同一個地方 | script 忙時按鍵不捲 |
| compositor 先捲、事件非同步 | 現在的捲動不變，另外分派事件 | 捲動不受 script 影響 | `preventDefault` 擋不了（網頁自己處理方向鍵時會雙重捲動）；和 Chrome 不同 |

## wheel 和 compositor

| 做法 | 說明 | 優點 | 缺點 |
| --- | --- | --- | --- |
| 文件層級的旗標 | renderer 告訴 compositor 文件有沒有不是 passive 的 `wheel` / `mousewheel` listener；有就等 main thread | 簡單；照 Chrome 以前的做法（passive by default 之前和之後都有） | 頁面任何地方有一個 blocking listener，整頁的滾輪都要等 main |
| **照區域**（wheel event handler region，第 5 條） | paint 時記下有 blocking listener 的元素的區域，compositor 只在區域裡等 | 照 Chrome 現在的做法；不相干的地方照舊不等 | 要 paint 記區域、compositor 照區域判斷，比較大 |
| 一律非同步、不能取消 | 事件照樣分派但 `cancelable` 是 false | 最小 | 和規範、Chrome 不同；地圖這類攔滾輪的網頁壞掉 |

passive by default：`window`、`document`、`<html>`、`<body>` 上沒指定 `passive` 的 `wheel` / `mousewheel` / `touchstart` / `touchmove` listener 是 passive（Chrome 和規範的 intervention）。

## WPT 的調查

2026-10-08 在 `../third_party/wpt` 數 testharness.js 的檔案（不含 reftest、manual、`resources/`）。kun 的 runner 沒有 testdriver（`testharness-skip.txt` 的開頭；RFC 0021 的 WebDriver 之後才有）。

| 目錄 | 檔案 | 用 testdriver | 不用的 |
| --- | --- | --- | --- |
| `uievents/` | 68 | 52 | 16：`keyboard/keyboardevent-legacy`、`constructors/inputevent-constructor`、`event-getmodifierstate`、`legacy/Event-subclasses-init`、`legacy-domevents-tests/` 的 4 個（`dispatchEvent.click.checkbox` 等）、`focusevents/blur-relatedTarget`、`interface/click-event`、`getModifierState`、`historical`、`ui_event_pseudo_target`、`textInput/api`、`order-of-events/focus-events/focus-automated-blink-webkit`、`idlharness`（要 `fetch`） |
| `html/interaction/focus/` | 197 | 147 | 扣掉實驗性的 `focusgroup/`（137 個）剩 60 個，44 個不用：`the-autofocus-attribute/` 25 個、`processing-model/` 7 個、tabindex 的 3 個、`focus-management/` 的 3 個、`document-level-apis`、頂層 4 個 |
| `html/semantics/interactive-elements/` | 104 | 51 | 大多是 `details`、`dialog`；和 `click()` 有關的 7 個 |
| `dom/events/`（已經收） | 181 | 77（skip） | 現在照 baseline 失敗、這份會改善的：`Event-subclasses-constructors`（沒有建構子、`UIEvent` 不能建構、沒有 `screenX`）、`Event-dispatch-click`（沒有 `click()`）、`Event-dispatch-detached-input-and-change`、`Event-dispatch-click.tentative`、`synthetic-events-cancelable`（沒有 `WheelEvent`）、`KeyEvent-initKeyEvent`、`element-click` 等 |

checkbox 的 activation（`html/semantics/forms/the-input-element/` 的 `checkbox-click-events` 等）屬於 G。

## 參考資料

- UI Events：https://w3c.github.io/uievents/（事件的 interface、順序、`which`、legacy 的 `keyCode`）。
- HTML：GlobalEventHandlers（https://html.spec.whatwg.org/#globaleventhandlers）、`click()`（https://html.spec.whatwg.org/#dom-click）、focus（https://html.spec.whatwg.org/#focus）、autofocus、循序 focus 導覽。
- DOM：dispatch 的 activation behavior、legacy-pre-activation behavior（https://dom.spec.whatwg.org/#concept-event-dispatch）。
- CSSOM View：`MouseEvent` 的 `pageX`、`offsetX`。
- Chromium：`third_party/blink/renderer/core/input/`、`page/focus_controller.cc`、`dom/element.cc`、`dom/document.cc`、`html/html_element.cc`、`dom/events/event_dispatcher.cc`、`events/`；`cc/input/`（wheel event handler region）；`content/browser/renderer_host/render_widget_host_impl.cc`（`SetFocus`、`TakeFocus`）。
- Speedometer 3.1（`release/3.1`，RFC 0012 的 `details.md` 的開頭）：runner 的 `benchmark-runner.mjs`、`tests.mjs`，TodoMVC 各 app 的 `dist/`。
