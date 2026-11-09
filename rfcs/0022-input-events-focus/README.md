# RFC 0022：輸入事件和 focus（F）

- 開始日期：2026-10-08
- 狀態：✅ 已完成：2026-10-09（第 1 到 8 條 2026-10-08 接受；第 1 段的第 9、10 條 2026-10-08，第 2 段的第 11 到 13 條、第 3 段的第 14、15 條、第 4 段的第 16 到 27 條 2026-10-09 確認；五段都做完，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 F，DOM 線的第二份，接在 H1（RFC 0017）之後；之後的 G（表單）依賴它。

## 摘要

補上鍵盤、滑鼠、滾輪和 focus 的事件與 API，讓 script 建的事件和使用者的輸入都照規範分派：

1. **事件的 interface**：`KeyboardEvent`、`FocusEvent`、`WheelEvent`、`InputEvent`，補完 `UIEvent` 和 `MouseEvent`，加上 HTML 的 GlobalEventHandlers（`onkeydown`、`onchange` 等）。
2. **`element.click()` 和 activation behavior 的架構**：合成的 `click`，以及 G 的 checkbox 會接上的 legacy-pre-activation 和 canceled-activation 步驟。
3. **focus 模型**：每份 document 的 focused area、`focus()`、`blur()`、`activeElement`、`hasFocus()`、`tabIndex`、`autofocus`、`focus` / `blur` / `focusin` / `focusout`，page 的 focus 從 browser 來，`:focus`、`:focus-within`、`:focus-visible` 成立，Tab 在頁面裡移動 focus。
4. **使用者的輸入分派事件**：按鍵先到 main thread 分派 `keydown` / `keypress` / `keyup`，沒被取消才捲動；滑鼠分派 `mousedown` / `mouseup` / `mousemove` / `dblclick` / `contextmenu`，按下時移動 focus；滾輪分派 `wheel`，有不是 passive 的 listener 時 compositor 等 main thread 決定。

M1 只要其中一小部分（`KeyboardEvent` 的建構子和 `key`、`click()`、可以呼叫的 `focus()`、元素上的 `onkeydown` 和 `onchange`），但它們和其餘的部分共用同一套機制。範圍、分段和各項做法 2026-10-08 確認（「已決定的事項」第 1 到 8 條）；每個階段的細節在那個階段開始前確認。

## 動機

**現況（2026-10-08，盤點見 `details.md` 的「kun 現在」）**：
- **頁面收不到鍵盤**：browser 只把會捲動的 9 個鍵（方向鍵、PageUp / PageDown、Space、Home、End）送給 renderer，compositor thread 直接拿來捲 viewport，不送到 main thread；放開按鍵的事件在 shell 就丟了。文字鍵和 Tab 都不送。沒有 `KeyboardEvent`。
- **滑鼠只有 `click` 和 `auxclick`**：放開時找按下和放開兩處的共同祖先，分派 trusted 的 `MouseEvent`（`clientX/Y`、`button`、四個修飾鍵）。沒有 `mousedown`、`mouseup`、`mousemove`、`dblclick`、`contextmenu`、`mouseover` 這些；滾輪直接捲，沒有 `wheel` 事件，`preventDefault` 擋不了。
- **沒有 focus**：頁面裡沒有 focused element；`focus()`、`blur()`、`activeElement`、`hasFocus()`、`tabIndex`、`autofocus` 都沒有；browser 不告訴 renderer 視窗或分頁拿到、失去 focus（只有 `WasShown` / `WasHidden`）。`:focus` 等選擇器解析得了但永遠不成立，節點上沒有元素的狀態。
- **沒有 `element.click()`**；activation behavior 只有 `<a>`（`dom_api::event` 的 `has_activation_behavior`）。
- **event handler 只有幾個**：`onclick`、`onload`、`onerror`、`onscroll`、`onhashchange`、`onpopstate`。
- `docs/limitations.md` 的「事件」有兩句和程式不一致：`MouseEvent` 已經有修飾鍵、使用者的右鍵和中鍵已經分派 `auxclick`。

**M1 要什麼**（`details.md` 的「M1 的核對」，對照 Speedometer 3.1 的原始碼）：
- runner 的 `PageElement` 呼叫 `element.click()`、`element.focus()`，用 `new KeyboardEvent(type, {keyCode, which, key})` 送按鍵：沒有它們丟例外，suite 中止。
- TodoMVC-Preact-Complex-DOM 用 `"onkeydown" in element` 決定 listener 的事件型別：元素上沒有 `onkeydown`、`onchange` 時，它註冊成永遠不會發生的 `"KeyDown"`、`"Change"`，一個 todo 都加不進去；**suite 照樣「跑完」，量的卻是空的**。
- 三個 suite 都在 `prepare` 呼叫 `.new-todo` 的 `focus()`；Svelte 設 `autofocus`；Preact、Svelte 讀 `event.key`，ES5 不讀按鍵事件（靠 `change`）。
- 勾選和刪除靠 `click()`：`.destroy` 是 `click` 的 listener（F）；`.toggle` 要 checkbox 的 activation behavior（切換 `checked`、分派 `input` 和 `change`），那是 G。
- `blur` / `focus` 事件、`dblclick`、`activeElement` 只在 TodoMVC 的編輯流程用，M1 的步驟走不到（M2 之後）。

**範圍**：路線圖的 F，加上兩項原本在別處的：
- **GlobalEventHandlers 提前到這份**（路線圖原本放在 H2）：Preact 的判斷是 M1 的問題，而 event handler 的機制和這份的事件一起做最自然。HTML 的整組 GlobalEventHandlers 一次加上（Chrome 都有），不只這份的事件（第 2 條）。
- **元素的狀態**（`:focus` 要的）在 `dom` 加一個通用的形式，`:hover`、`:active` 之後（路線圖的 S4）也用它。

目標：
- runner 和 M1 的三個 suite 不再因為這份的 API 丟例外，也不再因為少了 event handler 而量到空的工作（`.toggle` 還要 G）。
- script 建的事件和使用者的輸入照 UI Events 和 HTML 的規範分派，由 `dom_api` 的單元測試、情境和 WPT 驗證。
- 每個成員同時給 JS 和 `kun:dom`（RFC 0012 的「已決定的事項」第 7 條）。
- 型別和分層照 Chromium（Blink 的 `EventHandler`、`KeyboardEventManager`、`MouseEventManager`、`FocusController`、`Element::focus`）。

不在範圍內：
- 表單（G）：`HTMLInputElement`、`checked`、`value`、checkbox 和 radio 的 activation behavior、`label`、`button`、`input` / `change` 由誰分派、送出表單。這份只準備 activation behavior 的架構讓 G 接上。
- 文字編輯：caret、`contenteditable`、`Selection`、`beforeinput` / `input` 的編輯步驟、IME 送進頁面（composition 事件）。`InputEvent` 只做 interface。
- Pointer Events（`pointerdown` 等）、touch、drag and drop、`mouseover` / `mouseout` / `mouseenter` / `mouseleave` 和 `:hover`、`:active`（留給路線圖的 S4，第 6 條）。
- focus ring 的繪製：kun 沒有 `outline`（之後的 CSS 工作），`:focus-visible` 成立但看不出來。
- `focusgroup`、`inert`、`dialog` 的 focus 規則、跨 frame 的 focus 移動以外的 iframe 細節（`details.md` 的「focus 和 iframe」）。

## 設計概要

**給寫網頁的人**：
- `new KeyboardEvent("keydown", {key: "Enter", keyCode: 13})`、`new FocusEvent("focus", {relatedTarget})`、`new WheelEvent("wheel", {deltaY: 100})`、`new InputEvent("input", {data: "a"})`；`MouseEvent` 有 `screenX/Y`、`pageX/Y`、`offsetX/Y`、`buttons`、`relatedTarget`、`getModifierState()`；`UIEvent` 有 `view`、`detail`、`which`。
- `element.click()` 分派冒泡、可取消的合成 `click`（`isTrusted` 是 false）；`<a>` 照樣導覽。
- `input.focus()`、`blur()`；`document.activeElement`、`document.hasFocus()`；`tabIndex`、`autofocus`；`focus` / `blur` / `focusin` / `focusout` 照 UI Events 的順序；`:focus` 的樣式成立。
- 使用者按鍵時 `keydown`、`keypress`（有字元的鍵）、`keyup` 分派到 focused element（沒有時是 body）；`preventDefault()` 擋掉捲動；Tab 移動 focus；Enter 在連結上開連結。
- 使用者點擊時依序是 `mousedown`、（focus 移動）、`mouseup`、`click`，連點兩下多一個 `dblclick`；右鍵有 `contextmenu`；滾輪有 `wheel`，在不是 passive 的 listener 的元素上可以取消；只有 `mousewheel` listener 的元素收到 `mousewheel`。
- `"onkeydown" in element` 是 true，`element.onchange = f` 和 `onchange="..."` 屬性都有效。

**給 wasm 的 guest**：同樣的 interface 和成員在 `kun:dom` 裡（`keyboard-event` 等 resource、`html-element.click`、`focus`）。`kun:dom` 升了兩次：0.9.0（具名的 handler 函式換成 `event-handler` 這一對，第 9 條）、0.10.0（`add-event-listener-options` 的 `passive` 可以沒指定，第 27 條）。

**給使用者**：視窗裡可以用 Tab 在頁面的連結和元素之間移動，Enter 開連結；網頁可以攔下按鍵（例如自己處理方向鍵）。

**給改 kun 的人**：按鍵的路徑改成「compositor → main thread → 沒被取消才捲動」；元素的狀態（focus，之後 hover、active）由 `dom` 記、style 失效照 Blink 的做法。

## 詳細設計

型別照 Chromium，宣告照 Blink 的目錄放（RFC 0014）：事件在 `core_events.rs`（`core/events`），`click()`、`focus()` 等在 `core_html.rs` 的 `HTMLElement`，`activeElement`、`hasFocus()` 在 `core_dom.rs` 的 `Document`，GlobalEventHandlers 照它們現在的位置（`handlers` 模組）。

### 1. 事件的 interface

細節見 `details.md` 的「事件的 interface」。

- **`UIEvent`**：建構子、`view`（`Window` 或 `null`）、`detail`、`which`、`initUIEvent`。
- **`MouseEvent`**：補 `screenX/Y`、`pageX/Y`、`offsetX/Y`、`x/y`、`movementX/Y`、`buttons`、`relatedTarget`、`getModifierState()`、`initMouseEvent`。`pageX` 加上捲動、`offsetX` 減去 target 的 padding box，照 CSSOM View 在讀的時候算（Blink 的 `MouseEvent::offsetX` 也是讀時算，要 layout）。
- **`KeyboardEvent`**：`key`、`code`、`location`、`repeat`、`isComposing`、四個修飾鍵、`getModifierState()`，legacy 的 `keyCode`、`charCode`、`which`（建構子照 Chrome 收 `keyCode`、`charCode`、`which` 的 init）、`initKeyboardEvent`。
- **`FocusEvent`**：`relatedTarget`。**`WheelEvent`**：`deltaX/Y/Z`、`deltaMode`、常數。**`InputEvent`**：`data`、`inputType`、`isComposing`（只有 interface）。
- `document.createEvent` 照 DOM 規範的表認得新的名稱（`KeyboardEvent`、`FocusEvent`、`UIEvent(s)`、`MouseEvent(s)`）；`WheelEvent`、`InputEvent`、`KeyEvents` 不在表裡，丟 `NotSupportedError`。
- **GC**：照 RFC 0008 的 `Event` 一個 GC 型別；UI 事件的資料放在 `Event` 不被 trace 的欄位（`UiEventFields`：`Ui`、`Mouse`、`Wheel`、`Keyboard`、`Focus`、`Input`），legacy 的 `init*Event` 改它、種類不變；`view` 和 `relatedTarget` 是事件的 edge（`relatedTarget` 是 `TargetEdge`，分派時照 `target` 的規則 retarget）。

### 2. GlobalEventHandlers

- HTML 的 GlobalEventHandlers 整組（約 90 個 `on*`，第 2 條）加在 `HTMLElement`、`Document`、`Window`，content attribute 和 IDL attribute 都經過現有的 `handlers` 機制（RFC 0010）；`onerror` 的特例和 `<body>` 的 window handler 照舊。只有 IDL attribute 存在、事件本身 kun 還不會分派的（`ontransitionend` 等），script 自己 `dispatchEvent` 時照樣觸發。比較見 `details.md` 的「GlobalEventHandlers 的範圍」。

### 3. `click()` 和 activation behavior

- **`HTMLElement.click()`**：HTML 的步驟：「click in progress」的 flag 防止重入；分派合成的 `PointerEvent`……kun 沒有 Pointer Events，照 UI Events 用 `MouseEvent`（記成和規範的差別，WPT 的 `click-event.htm` 照 baseline）；`isTrusted` 是 false，`detail` 是 1。
- **activation behavior 的架構**（第 3 條：F 做架構和 `click()`，checkbox、radio 在 G）：DOM 的 dispatch 已經有 activation target（`dom_api::event` 的 `activation_target`）；這份把它改成照 DOM 規範的三步：分派前的 legacy-pre-activation behavior、取消時的 legacy-canceled-activation behavior、沒取消時的 activation behavior，每種元素在一張表裡登記（`<a>` 現在的、G 的 checkbox 和 radio、之後的 `<label>`、`<button>`）。對應 Blink 的 `Element::ActivationBehavior` 和 `HTMLInputElement::PreDispatchEventHandler` / `PostDispatchEventHandler`。表的形式見第 11 條，「click in progress」的 flag 放在哪裡見第 12 條。
- **default event handler**（第 13 條）：activation 之後，沒被取消的 trusted 事件或 click 沿路徑交給元素的 default event handler（Blink 的 `Node::DefaultEventHandler`）。`<a>` 的使用者中鍵 `auxclick` 從原本的 activation target 搬到這裡；第 4 段的按鍵預設動作（連結上的 Enter 等）也在這裡。

### 4. focus 模型

細節見 `details.md` 的「focus 模型」。

- **focused area**：每份 document 一個 focused element（`dom` 的 document 資料多一條 edge，Blink 的 `Document::focused_element_`）；page 的 focus（`hasFocus()`、focus 事件發不發、`:focus` 成不成立）和 focused frame 是 page 的狀態（`dom_api::focus::PageFocus`，renderer 建、所有 frame 共用，Blink 的 `FocusController`），由 browser 告訴 renderer（新的 `RendererMsg::SetFocus(bool)`，Chromium 的 `WidgetInputHandler::SetFocus`）：視窗拿到、失去 focus、切換分頁、網址列拿到 focus 時（shell 的 `BrowserView` 算出哪個網頁有）；renderer 一開始沒有，headless 照 Chrome 的 headless 給它。事件照 Blink 也發舊的 `DOMFocusIn` / `DOMFocusOut`。
- **focusable**：有 `tabindex` 的元素、有 `href` 的 `<a>`、之後 G 的表單元素；`focus()` 對不能 focus 的元素什麼都不做；`tabIndex` 的 getter 照 HTML（`<a href>` 是 0、其他 -1）。
- **focus 的移動**：HTML 的「focusing steps」和「unfocusing steps」，事件照 UI Events 的順序（`blur`、`focusout`、`focus`、`focusin`），`relatedTarget` 互指。focus fixup 有兩處：focused element 被移出 document 時，`dom` 的移除當場清掉（不發事件，照 Blink 的 `Document::RemoveFocusedElementOfSubtree`）；還在 document 裡、但不能再 focus 的（例如變成 `display: none`），在 rendering update 時清掉、發 `blur`。
- **`:focus`、`:focus-within`、`:focus-visible`**：`dom` 的元素多一組狀態 flag（`ElementState`，之後 hover、active 也用），改變時記在 style flag，style 只在有規則用到時重算（第 14 條，含祖先的 `:focus-within`）；`style` 的 `match_non_ts_pseudo_class` 讀它。「being rendered」先 layout 再看有沒有 box（第 15 條）。`:focus-visible` 照 Chrome 的啟發式：鍵盤造成的 focus、script 在鍵盤之後的 `focus()` 成立，滑鼠點的不成立。
- **`autofocus`**：照 HTML 的「autofocus candidates」，rendering update 時 flush（Blink 的 `Document::FlushAutofocusCandidates`），只在主 frame 和同源的子 frame。
- **Tab**：循序 focus 導覽照 `tabindex` 的順序（正數在前、0 和 focusable 照樹的順序），`<iframe>` 在父 frame 的順序裡代表它的 document 的順序（第 18 條）；在頁面頭尾時交還給 browser，移到網址列（`RendererHostMsg::TakeFocus`，第 7 條）；網址列有按鍵時，Tab、Shift+Tab 把 focus 交給頁面的第一個、最後一個（`RendererMsg::SetInitialFocus`）。

### 5. 使用者的輸入

細節見 `details.md` 的「輸入的路徑」。

- **按鍵**（第 4 條）：browser 送所有的鍵（`InputEvent::Key` 帶 `key`、`code`、`keyCode`、`text`、修飾鍵、`repeat`、按下或放開；不再只有捲動的 9 個；`key`、`code`、`keyCode` 由 shell 從平台的鍵算好，第 16 條），快捷鍵和網址列有 focus 時照舊不送，放開的鍵只送到收到按下的地方。compositor 不再自己捲：一律交給 main thread（`DidNotHandle`），main thread 在 focused element（沒有時 body）分派 `keydown`、有字元時 `keypress`、`keyup`；`keydown` 沒被取消才做預設動作：捲動（從 focused element，沒有時從最後按下滑鼠的元素，往外找這個方向還能捲的容器，第 17 條，Blink 的 `ScrollManager::LogicalScroll`）、Tab 移動 focus、Enter 在 focused 的連結上開連結（`<a>` 的 default event handler，帶著修飾鍵的 trusted 合成 click）。代價是按鍵捲動要等 main thread（Chrome 也是）；比較見 `details.md` 的「按鍵要不要先到 main thread」。
- **滑鼠**：`mousedown`（之後移動 focus：點到的 focusable 祖先，否則清掉 focus，照 Blink 的 `MouseEventManager::HandleMouseFocus`；`preventDefault` 擋掉它）、`mouseup`、`click`（照舊在共同祖先）、`dblclick`（`click` 的 `detail` 是連點的次數，shell 照系統的連點間隔和距離算，送給 renderer；Chromium 的 `WebMouseEvent::click_count`，第 8、19 條）、右鍵的 `contextmenu`（照平台在按下或放開時，沒有選單，第 22 條）。`mousemove` 分派給滑鼠下的元素（`buttons` 照按著的鍵），只留最後一個、在下一個 `BeginMainFrame` 開頭分派（第 20 條）。在子 frame 裡按下之後，放開之前的事件都給那個 frame（第 21 條）。
- **滾輪**（第 5 條）：分派 `wheel`，照 Chrome 現在的 wheel event handler region：`window`、`document`、`<html>`、`<body>` 上沒指定 `passive` 的 listener 預設是 passive（passive by default）；有不是 passive 的 `wheel` / `mousewheel` listener 的元素，它和子孫的盒子在 paint 時記成那個 layer 的「wheel event handler region」（Blink 的 `HitTestData` 和 `cc::Layer` 的 `wheel_event_region`；listener 明確不是 passive 地掛在 `window` 或 `document` 上時是整份文件），commit 給 compositor。區域在 paint 的走訪裡算（第 23 條）；盒子移動時在下一次 paint 更新，listener 加減時照 `dom` 的版本號只重算區域、不重新 paint（第 24 條）。compositor 收到滾輪時，點不在區域裡就照舊先捲，頁面可能有 `wheel` listener 時再把事件交給 main thread 分派，不能取消；在區域裡就交給 main thread，分派完沒被取消才由 main thread 照現在的路徑捲（第 25 條）。沒有 `wheel` listener 的 target，trusted 的事件照 legacy 名稱分派給它的 `mousewheel` listener（第 26 條）。`addEventListener` 的 `passive` 照規範可以沒指定（第 27 條）。比較見 `details.md` 的「wheel 和 compositor」。

### 6. 和 process 的邊界

只多能序列化的欄位和三個訊息：`InputEvent` 的按鍵、滑鼠和滾輪帶更多欄位（`protocol::input`）、`RendererMsg::SetFocus` 和 `SetInitialFocus`；Tab 走到頁面頭尾時 renderer 告訴 browser（`RendererHostMsg::TakeFocus`，Chromium 的 `LocalFrameHost::TakeFocus`）。compositor 交給 main thread 的 input 帶著分派的方式（`InputDispatch`），在 renderer 裡面。符合 `docs/architecture.md` 的「Process 的邊界」。

### 測試

- **語意**（`dom_api` 的 `tests/`，不經過 engine）：每個事件的建構子和 init、`getModifierState`、`pageX` / `offsetX` 的換算（測試的 `LayoutClient`）、`click()` 的重入和 activation 的三步、focus 的移動和事件順序、`tabIndex` 的預設值、focus fixup、autofocus 的 flush。
- **`dom`**：focused element 的 edge 和元素狀態 flag 經過 GC（泛型的 GC 測試、工作負載）、狀態改變時 style 的失效。
- **bindings**（`dom_js`）：新的 interface 和建構子、`"onkeydown" in element`、content attribute 的 handler；**wasm**：每種新的物件一個 guest。
- **renderer 和 browser**：按鍵和滑鼠的事件順序、`preventDefault` 擋掉捲動和 focus 的移動、`dblclick` 的次數、`SetFocus` 和 `hasFocus()`、Tab 走到頭尾交還 browser；compositor 的 wheel 在有、沒有 blocking listener 時。
- **情境**（`tests/scenarios/`）：`about:demo` 加一段：Tab 在幾個連結之間移動、`:focus` 改變顏色（看得見）、一個攔下方向鍵的元素、`keydown` 和 `wheel` 的紀錄；寫成情境。動到視窗那一層（shell 的按鍵、連點），要跑 `test.py automation`。
- **WPT**：`dom/events/` 的 `Event-subclasses-constructors`、`Event-dispatch-click` 等已經在跑的 baseline 會改善；新收 `uievents/` 不用 testdriver 的 16 個、`html/interaction/focus/` 不用 testdriver 的 44 個（不含 `focusgroup/`）。用 testdriver 的（`uievents/` 的 52 個等）要 RFC 0021 的 WebDriver 或 testdriver 的 channel，不在這份。數字見 `details.md` 的「WPT 的調查」；實際收了哪些、結果見 `progress.md`。
- **mutation test**：每個階段都跑。

### 分階段

先做 M1 要的（第 1、2 段）。原本每段做完 merge 回 `main`（第 1 條）；實際上五段都在 `0022` 上做完、一起審核之後才 merge：

1. **事件的 interface 和 GlobalEventHandlers**：`UIEvent`、`MouseEvent` 補完，`KeyboardEvent`、`FocusEvent`、`WheelEvent`、`InputEvent`，`createEvent`；GlobalEventHandlers 整組。只動 `dom` 的 `EventKind`、`dom_api`、宣告表；WPT 的 `uievents/` 不用 testdriver 的部分。
2. **`click()` 和 activation behavior 的架構**：合成的 click、三步的 activation。M1 的 F 到這裡完成（`.toggle` 等 G）。
3. **focus 模型**：focused area、`focus()` / `blur()`、`activeElement`、`hasFocus()`、`tabIndex`、focus 事件、focus fixup、`autofocus`、元素狀態和 `:focus*`、`SetFocus`；WPT 的 `html/interaction/focus/` 的部分。
4. **使用者的輸入**：4a 按鍵（路徑改到 main thread、`keydown` / `keypress` / `keyup`、預設動作、Tab、`TakeFocus`）；4b 滑鼠（`mousedown` / `mouseup` / `mousemove` / `dblclick` / `contextmenu`、點擊移動 focus）；4c 滾輪（`wheel`、passive by default、paint 記下 wheel event handler region、compositor 照區域判斷）。
5. **收尾**：`about:demo` 和情境、`test.py automation`、文件的核對、`matrix` 和 `fuzz`（動到 GC）。

## 缺點

- **按鍵捲動要等 main thread**：script 忙的時候，方向鍵和 PageDown 不再立刻捲（現在由 compositor 直接捲）。Chrome 一樣；滾輪沒有 blocking listener 時照舊不等。
- **wheel event handler region 要動 paint 和 compositor**：paint 要知道哪些元素在有 blocking listener 的子樹裡（listener 加減時要重新記），compositor 要照點判斷；比文件層級的旗標大，但不相干的地方照舊不等 main thread（第 5 條）。
- **一次加上整組 GlobalEventHandlers**：大部分事件 kun 還不會分派，`"ontransitionend" in element` 是 true 會讓功能偵測以為支援；Chrome 都有，照 Chrome。
- **activation behavior 改成三步**：`<a>` 現在的路徑要搬進新的表，G 之前只有它一個使用者。
- **元素狀態放在 `dom`**：每個元素多幾個 bit，改變時的 style 失效要照 Blink 的規則（`:focus-within` 的祖先），漏了只有測試抓得到。
- **和規範的差別**：合成的 click 是 `MouseEvent`（規範是 `PointerEvent`）；沒有 `mouseover` 這一組和 `:hover`、`:active`（待決定）；focus ring 畫不出來。

## 理由和其他做法

- **一份 RFC、分五段**：事件、activation、focus、輸入彼此依賴（點擊移動 focus、Tab 是按鍵的預設動作），分成好幾份 RFC 時每份都要描述別份的介面；分段讓 M1 要的部分先做完。
- **照 Chromium 的分層**：事件的分派和預設動作在 renderer 的 main thread（Blink 的 `EventHandler`），browser 只送原始的輸入和 focus 的狀態。
- 每一項的比較見 `details.md`。

## 先例

- **Chromium**（`third_party/blink/renderer/core/`）：
  - `input/event_handler.cc`、`keyboard_event_manager.cc`（`keydown` 的預設動作、Tab）、`mouse_event_manager.cc`（`HandleMouseFocus`、click count）、`scroll_manager.cc`（按鍵捲動）。
  - `page/focus_controller.cc`（page focus、`AdvanceFocus`、`SetFocusedElement`）、`dom/element.cc`（`focus`、`IsFocusable`、`SupportsFocus`）、`dom/document.cc`（`SetFocusedElement`、`FlushAutofocusCandidates`、`RemoveFocusedElementOfSubtree`）。
  - `html/html_element.cc`（`click()` → `DispatchSimulatedClick`）、`dom/events/event_dispatcher.cc`（activation 的三步）。
  - `events/keyboard_event.cc`、`mouse_event.cc`（`offsetX` 讀時算）、`wheel_event.cc`；`content/renderer`、`cc/input` 的 wheel event handler region。
- **kun 自己的**：RFC 0008（宣告表、`MouseEvent`、`createEvent`）、RFC 0010（event handler、`dispatch`）、RFC 0009 的 2b-3（子 frame 的 hit test 和輸入）、RFC 0018（hit test、`elementFromPoint`）。

## 已決定的事項

2026-10-08 確認（接受）：

1. **範圍是路線圖的 F 整份**：事件的 interface、GlobalEventHandlers、`click()` 和 activation behavior 的架構、focus 模型、使用者的輸入，分五段，M1 要的在前兩段，每段做完 merge 回 `main`。不選只做 M1 要的、其餘另開 RFC：點擊移動 focus、Tab 是按鍵的預設動作，彼此依賴，分開時每份都要描述別份的介面。
2. **GlobalEventHandlers 整組一次加上**：HTML 的約 90 個 `on*` 都在 `HTMLElement`、`Document`、`Window`，和 Chrome 一樣，`"onX" in element` 的偵測都對（Preact 用它決定事件的名稱）；路線圖原本放在 H2，提前到這份。代價是 kun 還不會分派的事件，功能偵測也以為支援。不選只加這份和 G 會分派的：框架碰到其他事件時走錯路，之後每加一種事件都要回來加。
3. **F 做 activation behavior 的架構和 `click()`，checkbox、radio 在 G**：照路線圖；F 把 dispatch 的 activation 改成 DOM 規範的三步（legacy-pre-activation、legacy-canceled-activation、activation），`<a>` 搬進去，G 接上 checkbox、radio、`label`、`button`。M1 的 `.toggle`（`checked` 和 `change`）要等 G。不選 F 先做 checkbox 的最小部分：表單的元素會分在兩份 RFC。
4. **使用者的按鍵先到 main thread**：compositor 不再自己捲；main thread 分派 `keydown`（有字元時 `keypress`）、`keyup`，`keydown` 沒被取消才做預設動作（捲動、Tab、Enter 開連結）。照 Chrome，`preventDefault` 擋得掉；代價是 script 忙時按鍵不捲。不選 compositor 照舊先捲、事件非同步：網頁自己處理方向鍵時雙重捲動，和 Chrome 不同。
5. **`wheel` 照區域**：照 Chrome 現在的 wheel event handler region：paint 記下有不是 passive 的 listener 的子樹的盒子，compositor 只在區域裡等 main thread，其他地方照舊先捲、事件不能取消；passive by default 照 Chrome。不選文件層級的旗標（Chrome 以前的做法：一個 blocking listener 讓整頁都等，比較小）；不選一律非同步、不能取消（和規範、Chrome 不同）。
6. **`:hover`、`:active` 和 `mouseover` 這一組留給路線圖的 S4**：這份在 `dom` 做元素狀態的通用形式和 `:focus`、`:focus-within`、`:focus-visible`，S4 用同一個形式加 hover 和 active。不選在這份一起做：這份更大，hover 改變時的 style 失效和重畫也要做。
7. **Tab 走到頁面頭尾時交還 browser**：renderer 送 `TakeFocus`，focus 移到網址列，照 Chrome。不選在頁面裡繞回開頭（用鍵盤回不到網址列）。
8. **連點的次數由 shell 照系統的設定算**：shell 照系統的連點間隔和距離算出次數，放在滑鼠的訊息裡送給 renderer（Chromium 的 `WebMouseEvent::click_count`，由平台給）。不選 renderer 照固定的間隔自己算（不照使用者的系統設定）。

2026-10-08 確認（第 1 段）：

9. **GlobalEventHandlers 在 `kun:dom` 是一對通用的函式**：`element`、`document`、`window` 各有 `event-handler(type)` 和 `set-event-handler(type, value)`，用事件的名稱涵蓋整組，效果和 JS 的 `el.onX` 一樣（同一個 handler、同樣的順序和行為）；具名的 `onclick` 等只在 JS（JS 的 `el.onX` 和 `"onX" in el` 要名字）。現有的 6 個具名函式（`onclick` 等的 get、set）換掉，不相容：`kun:dom` 升到 0.9.0（RFC 0012 的第 10 條），用到的 guest 跟著改。不選全部照名字放進 WIT：每個繼承的 resource 都重複列出，WIT 多約 2000 個函式，Lua 的 runtime 和 guest 的 binding 跟著變大；不選加通用的函式、留著舊的 6 個：同一件事兩個入口。宣告表因此多「只在 WIT」的成員（JS 不裝成 prototype 的方法）。做法（第 1 段）：成員多一個 `exposure`（`Exposure::Both`、`Js`、`Wit`）；具名的由 `[GlobalEventHandlers]` 從 `handlers` 的一份清單展開成只給 JS 的 getter 和 setter（getter 拿到事件的型別），通用的一對寫成 `[WitOnly] method eventHandler` / `setEventHandler`，用同樣的函式；型別不是那個物件的 handler 時是 `NotSupportedError`（JS 的 `el.onfoo` 也不是 handler）。window 的 `onhashchange`、`onpopstate`（`WindowEventHandlers`）一起換成這一對。
10. **「整組」以 Blink 的 `global_event_handlers.idl` 為準**：HTML 的 GlobalEventHandlers，加上 Chrome 正式提供（不是 `RuntimeEnabled`）的其他規範的：CSS 動畫和 transition（`onanimation*`、`ontransition*`）、Selection（`onselectstart`、`onselectionchange`）、`onscrollend`，和 Chrome 留著的舊名字（`onmousewheel`、`onwebkitanimation*`、`onwebkittransitionend`）；不加 Pointer Events（`onpointer*`、`on*pointercapture`）和 touch（Chrome 在沒有觸控的桌面上也不提供）：kun 不分派它們，加了會讓偏好 pointer 事件的函式庫收不到點擊。不選只加 HTML 的 76 個（和 Chrome 不同的地方多一些）。
11. **activation behavior 是 trait 加一個照元素查的函式**（2026-10-09，第 2 段）：`dom_api::activation` 的 `ActivationBehavior`（`legacy_pre_activation`、`legacy_canceled_activation`、`activation`），`activation_behavior(element)` 照名稱回傳 `&'static dyn ActivationBehavior`；legacy-pre-activation 回傳的 `ActivationState`（enum，G 加上 checkbox 的舊值）交給 legacy-canceled-activation。對應 Blink 的 `Element::HasActivationBehavior` / `ActivationBehavior` 虛擬函式和 `HTMLInputElement::PreDispatchEventHandler` 回傳的 `EventDispatchHandlingState`。不選 fn pointer 的靜態表（狀態要用 `Box<dyn Any>` 傳，型別不受檢查，Chromium 也沒有這種寫法）；不選在原本的函式裡多加 match（每種元素的行為散在四個函式）。
12. **「click in progress flag」是 `dom` 的元素上的一個 bit**（2026-10-09，第 2 段）：照 HTML 的每個元素一個 flag，放在元素不被 trace 的狀態裡，第 3 段的元素狀態也在 `dom` 的元素上。Blink 用 thread 全域的 `HeapHashSet<Member<Node>>`（`EventDispatcher::DispatchSimulatedClick`），效果一樣。不選 `DomIsolate` 上的集合（要照顧被回收的節點）；不選 `dom_api` 的 thread_local（renderer 這一側多一個全域狀態，同一個 thread 上的幾個 `DomIsolate` 共用）。
13. **這段就做 Blink 的 default event handler**（2026-10-09，第 2 段）：`dom_api::activation` 的 `DefaultEventHandler`，分派之後、activation behavior 之後，對沒被取消的 trusted 事件或 click，先 target 的、事件 bubble 時再沿路徑往上，到有一個處理了為止（Blink 的 `EventDispatcher::DispatchEventPostProcess`、`Node::DefaultEventHandler`、`Event::SetDefaultHandled`）。`<a>` 的使用者中鍵 `auxclick` 從 activation target 搬過來（Blink 的 `HTMLAnchorElement::DefaultEventHandler` 也這樣收）；第 4 段的連結上的 Enter 接在這裡（按鈕上的空白鍵是 G）。不選先把中鍵留在 `<a>` 的 activation、第 4 段再看：到時要搬一次。
14. **focus 改變時，style 只在有規則用到那個狀態時重算**（2026-10-09，第 3 段）：style 在樣式表改變時記下用到哪幾個 focus 的 pseudo-class（和現在記結構選擇器、`:has()` 的地方一樣）；`dom` 只在元素的 style flag 記下哪些狀態變了（`state_changed`，parent 的 `children_state_changed` 給跨兄弟的選擇器），style 階段看到有規則用到才照改屬性的方式重算那個元素的子樹。是 Blink 的 `RuleFeatureSet` / invalidation set 的簡化版。不選一律重算（`:focus-within` 的祖先一路到 `<html>`，每次 focus 都重算整份文件，Speedometer 的 TodoMVC 每步都 focus）；不選照 Blink 做完整的 invalidation set（新的大子系統，屬性的失效也該一起改；記進「之後的可能」）。
15. **`focus()` 先 layout，再看元素有沒有 box**（2026-10-09，第 3 段）：「being rendered」用現有的 `LayoutClient` 入口（`update_style_and_layout`、`element_box`），結果和 Chrome 一樣、改動小；代價是 `focus()` 強迫一次完整的 layout（Blink 只更新到 layout tree），同一個 task 裡改 DOM、`focus()`、再改 DOM 時 layout 做兩次。不選加一個只更新 style 的入口（`LayoutClient` 多一個方法、要把 style 階段拆出來、自己從 computed style 判斷 being rendered；記進「之後的可能」，和 `getComputedStyle` 一起做）；不選不檢查（`display: none` 的也能 focus，和 Chrome 不同）。
16. **按鍵的 `key`、`code`、`keyCode` 由 shell 算好放在訊息裡**（2026-10-09，第 4a 段）：shell 從 winit 的邏輯鍵、實體鍵和文字算出 DOM 的 `key`、`code` 和 Windows 的 `keyCode`（字母、數字照打出的字，跟著鍵盤配置；其他照名稱或實體鍵，Chromium 的 `KeyboardCodeFromCharCode`、`KeyboardCodeFromDomCode`），`InputEvent::Key` 帶著它們、打出的文字、修飾鍵、`repeat`、按下或放開。和 Chromium 一樣：平台層在 browser process 算好 `NativeWebKeyboardEvent`，renderer 不碰平台的鍵。不選 shell 只送 `key` 和 `code`、renderer 算 `keyCode`（Blink 收到的就有 `windows_key_code`，而且要看平台的鍵盤配置）。
17. **按鍵從 focus 的元素往外捲**（2026-10-09，第 4a 段）：從 focus 的元素（含子 frame 裡的）最近、這個方向還能捲的捲動容器開始，捲不動往外，最後是 viewport；一個鍵只捲一個框（不像滾輪把剩下的往外傳）。Blink 的 `ScrollManager::LogicalScroll`；沒有 focus 時從最後按下滑鼠的節點（4b 補上）。不選只捲 focused frame 的 viewport（捲動容器裡的元素有 focus 時不捲它的容器，和 Chrome 不同）。
18. **Tab 的循序導覽進子 frame**（2026-10-09，第 4a 段）：有 document 的 `<iframe>` 在父 frame 的順序裡代表那份 document 的順序（Blink 的 `FocusNavigation` 的 scope），頭尾是整個 page 的；網址列有 focus 時 Tab、Shift+Tab 把 focus 交給 page 的第一個、最後一個（`RendererMsg::SetInitialFocus`，Chromium 的 `FocusThroughTabTraversal`）。不選只在 focused frame 裡走（用鍵盤進不了 iframe）。
19. **連點的間隔在 macOS 照系統設定，其他照 Chromium 的預設**（2026-10-09，第 4b 段）：細化第 8 條。shell 照平台的事件算連點的次數：前一次按下在間隔之內、同一個鍵、兩個方向都不超過 2 px（Chromium 的 `ui::MouseEvent::IsRepeatedClickEvent`，`kDoubleClickWidth` 4）就加一。間隔在 macOS 讀 `NSEvent.doubleClickInterval`（使用者在輔助使用改的連按速度；`objc2-app-kit` 的這個函式是 safe 的），Windows 和 Linux 用 500 ms（Chromium 的 `kDoubleClickTimeMs`，Linux 沒有 GTK 時就是它）。Windows 的 `GetDoubleClickTime` 要 `unsafe`，shell 不能有，記成限制。不選三個平台都用 500 ms（macOS 調過連按速度的人在 kun 裡連點不對，和第 8 條不符）。
20. **`mousemove` 對齊 frame 分派**（2026-10-09，第 4b 段）：renderer 只留最後一個 `mousemove`，在下一個 `BeginMainFrame` 開頭（animation frame callback 之前）分派；按下、放開、按鍵和滾輪來時先把它分派掉，順序不變；頁面看不見、沒有 frame 時馬上分派。Chrome 的 `MainThreadEventQueue` 的 rAF-aligned input。不選一來就分派（滑鼠移動得快時每個都跑 script 和 hit test）；不選只合併 channel 裡排著的（時機和 Chrome 不同）。
21. **在子 frame 裡按下之後，事件送到那個 frame**（2026-10-09，第 4b 段）：按下時記住目標所在的子 frame，放開之前的 `mousemove` 和 `mouseup` 都在那個 frame 裡 hit test，在 frame 外面時目標是它的 `<html>`。Blink 的 `EventHandler::capturing_subframe_element_`。不選一律送到滑鼠下的元素（iframe 裡的拖曳拖出邊界就收不到 `mouseup`）。
22. **`contextmenu` 照平台在按下或放開時發**（2026-10-09，第 4b 段）：Windows 在右鍵放開時，macOS 和 Linux 在按下時，照 Blink 的 `WebFrameWidgetImpl`（`BUILDFLAG(IS_WIN)`）；renderer 用 `cfg` 分。沒有選單，`preventDefault` 沒有東西可擋。macOS 的 Ctrl+左鍵當右鍵這段不做。不選一律在按下時發（Windows 上和 Chrome 不同）。
23. **wheel event handler region 在 paint 的走訪裡算**（2026-10-09，第 4c 段）：照 Blink 的 `inside_blocking_wheel_event_handler`，走訪盒子時帶著「在有 blocking `wheel` / `mousewheel` listener 的元素裡」，每個盒子的範圍照現有的 main thread scroll hit test region 的方式對到會移動它的空間（文件，或成為 layer 的捲動容器）、被 clip 切，和它一起 commit；listener 明確 non-passive 地掛在 `window` 或 `document` 上時是整份文件。不選每個有 listener 的元素一個大矩形（border box 加上子孫的盒子：範圍大，裡面有成為 layer 的捲動容器時還要另外切）。
24. **listener 加減時，區域照 `dom` 的版本號更新**（2026-10-09，第 4c 段）：`dom` 記一個版本號，加、減 `wheel` / `mousewheel` listener（和 event handler）時加一；renderer 在 rendering update 看到變了就重算區域、交給 compositor，不必重新 paint；算的時候直接問每個節點和 window 有沒有 blocking listener（Blink 的 `EventHandlerRegistry` 另外記下有 listener 的 target；kun 不另外記，被回收的 target 也就不用處理）。不選每個 frame 都重算（每個 frame 都走一次盒子樹）。
25. **區域裡沒被取消的滾輪由 main thread 自己捲**（2026-10-09，第 4c 段）：照現在 main thread scroll hit test region 裡的滾輪的路徑（`FrameView::wheel`）。Chrome 是 main thread 回 ack 說沒用掉，browser 再轉成 GestureScroll 交給 compositor；差別只在延遲，kun 少一條 compositor 接回來的路。
26. **`mousewheel` 照 Blink 補上**（2026-10-09，第 4c 段）：分派 trusted 的事件時，一個 target 沒有那個事件的 listener、有它的 legacy 名稱的 listener，就用 legacy 名稱呼叫那些（DOM 的「invoke」的 legacy type：`webkitAnimationEnd` 這些，加上 Blink 的 `wheel` 對 `mousewheel`）；`WheelEvent` 多 Chrome 的 `wheelDelta`、`wheelDeltaX`、`wheelDeltaY`；`mousewheel` 的 listener 和 `wheel` 的一樣算進區域。不選只做 `wheel`（只用 `mousewheel` 的舊網站收不到滾輪）。
27. **`addEventListener` 的 `passive` 照規範可以沒指定，`kun:dom` 升 0.10.0**（2026-10-09，第 4c 段）：DOM 的「default passive value」要分得出沒指定，`AddEventListenerOptions` 的 `passive` 改成 `Optional<bool>`；它也是 `kun:dom` 的 record，欄位變成 `option<bool>`，舊的 guest 不相容，升 0.10.0（`WheelEvent` 的 `wheelDelta` 這些一起），guest 全部重新編。兩個 engine 一樣有 passive by default。不選宣告表多一種只給 JS 的 dictionary 欄位、WIT 不變（wasm 的 guest 一律是明確的值，`window` / `document` 上的 listener 不會預設 passive，和 JS 不同）。

## 之後的可能

- **G（表單）**：`HTMLInputElement`、checkbox 和 radio 的 activation behavior、`input` / `change`、`label`、`button`、表單送出；用這份的 activation 架構和 focus。
- **S4**：`:hover`、`:active` 和 `mouseover` 這一組，用這份的元素狀態（第 6 條）。
- **文字編輯**：caret、`contenteditable`、`Selection`、`beforeinput` / `input`、IME 送進頁面。
- **Pointer Events**：`pointerdown` 等、合成的 click 改成 `PointerEvent`。
- **`outline` 和 focus ring**。
- **testdriver**：RFC 0021 的 WebDriver 做完後，`uievents/`、`html/interaction/focus/` 用 testdriver 的那些可以跑。
- **完整的 style invalidation set**（第 14 條）：照 Blink 的 `RuleFeatureSet`，從選擇器算出一個狀態或屬性改變時要重算哪些元素（自己、後代的某些 class、兄弟），不再整個子樹重算；focus 的狀態和屬性的失效一起改。
- **只更新 style 的 `LayoutClient` 入口**（第 15 條）：`focus()` 判斷 being rendered 只算 style（像 Blink 只到 layout tree），省掉強迫的 layout；`getComputedStyle` 也要同一個入口，一起做。等 Speedometer 量出 `focus()` 的 layout 明顯時再做。
