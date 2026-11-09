# RFC 0016：CSSOM（inline style 和 `getComputedStyle`）

- 開始日期：2026-10-07
- 狀態：✅ 已完成：2026-10-12（第 1 條 2026-10-07 排第 1 段，第 2 到 8 條在第 1 段、第 9 條在拆出第 2 段時、第 10 到 15 條在第 2 段、第 16 到 24 條在第 3 段定下：第 18 條在 3b 開始前從 C2 改寫成 C3，第 21 條在 3b、第 22 到 24 條在 3c 開始前；第 1 段 2026-10-07 在 macOS Apple Silicon、第 2 段 2026-10-09 在 Fedora 44、第 3 段 2026-10-12 在 Windows 11 做完，前兩段照階段 merge 回 `main`）
- 細節：`details.md`；進度：`progress.md`

## 摘要

RFC 0012 路線圖（`rfcs/0012-speedometer/roadmap.md`）的 C，CSS 線的第一份：讓 script 讀寫樣式。第 1 段是 inline style 的 `CSSStyleDeclaration`（`element.style`、`setProperty`、`getPropertyValue`、`removeProperty`、`cssText`、`style.backgroundColor` 這類 camelCase 的屬性）；第 2 段是 `getComputedStyle`，排在 D（CSSOM View）的同步更新的 hook 之後；第 3 段是 constructable stylesheet（`new CSSStyleSheet()`、`replace`、`replaceSync`，和它的規則的 CSSOM）和 `Document`、`ShadowRoot` 的 `adoptedStyleSheets`，排在 shadow DOM（路線圖的 K-1）之後。M1 只要 `element.style` 和 `setProperty`，M2 要 `getComputedStyle`，M3 要第 3 段。型別照 Chromium 取名（`InlineCSSStyleDeclaration`、`CSSComputedStyleDeclaration`、`MutableCSSPropertyValueSet`）。

已決定（「已決定的事項」）：`style` attribute 的字串照舊是唯一的來源，CSSOM 每次解析、修改、序列化寫回 attribute（Q1）；值從解析好的值序列化，照 Chrome 的輸出（R1）；只認 kun 白名單裡的 property（U1），自訂 property 存起來但還不生效（V1）。camelCase 的屬性照 Blink 用 named interceptor（Y2）；`getComputedStyle` 要的同步 style 更新用 D（CSSOM View）的 hook，排在 D 之後（X2）；JS 和 wasm 都給（W1）。第 3 段：constructed stylesheet 連同規則的 CSSOM（A2）；`adoptedStyleSheets` 照規範是 `ObservableArray`（B1）；解析好的規則由 `CSSStyleSheet` 帶著，型別在 `dom` 下面新拆出的 `css` crate（C3）；document 的 adopted stylesheet 排在它自己的之後，改了整份重算（D1）；`ObservableArray` 在 `dom_api` 是逐項的操作（O1），清單就地改（S1），`dom_js` 的 Proxy 每個 realm 共用一個 handler（H2）；`CSSStyleRule` 繼承 `CSSRule`（G2）。驗收和做過的檢查見 `progress.md`，仍然成立的限制見 `docs/limitations.md`。

## 動機

**現況（2026-10-07）**：
- **script 碰不到樣式**：沒有 `element.style`、`getComputedStyle`、`CSSStyleDeclaration`（`docs/limitations.md` 的「Style」）。要改樣式只能 `setAttribute('style', …)` 或換 `class`。
- **`style` attribute 只在 cascade 裡用**：每次重算那個元素時從字串解析（`crates/style/src/lib.rs` 的 `cascade`），沒有快取；attribute 改了，元素連同子樹重算（`docs/limitations.md` 的「Invalidation」）。
- **`style` 是自己寫的**（在 `cssparser` 上，不是 Stylo，`docs/architecture.md` 的「Crate 選型」）：支援的 property 是 `crates/css/src/properties.rs` 的白名單，清單外的宣告解析時就丟掉；解析時就把值正規化（絕對長度換成 px、顏色換成 RGBA），沒有任何序列化（`details.md` 的「現在的程式」）。
- **Speedometer**：runner 用 `frame.style` 擺 iframe、`style.setProperty` 設 CSS 變數；default 的 20 個 suite 裡 15 個用到 `element.style` 或 `getComputedStyle`（RFC 0012 的「盤點的結果」）。M1 的 TodoMVC-JavaScript-ES5 用 `element.style.display`；M2 的 jQuery、Backbone 等用 `getComputedStyle`。
- **WPT 的 CSS 測試大多靠 CSSOM**：`css/*/parsing/` 的 `-valid`、`-computed` 用 `element.style[property]` 和 `getComputedStyle` 檢查 parser 和序列化；P3 挑 reftest 時有一批因為用了 `.style` 被排除（`tests/wpt/README.md`）。

**第 3 段開始前的現況（2026-10-12）**：
- **script 建不出 stylesheet**：沒有 `CSSStyleSheet`、`CSSRule`、`CSSRuleList`，shadow root 和 document 都沒有 `adoptedStyleSheets`（`docs/limitations.md` 的 shadow DOM 那條：「也沒有 `adoptedStyleSheets`」；`crates/dom_api/tests/spec-missing.txt`）。
- **stylesheet 只在 `style` 裡面**：每份 document 的 `StyleEngine`（renderer 的 `FrameView` 擁有）每次重算都走過 light tree 收集 `<style>` 和 `<link>` 的原文，和上次比，有一份不同就全部重新解析、整份重算（`crates/style/src/lib.rs` 的 `style_sheet_texts`、`recalc_style`）。解析好的型別（`crates/css/src/stylesheet.rs` 的 `Stylesheet`、`StyleRule`）是 `style` 私有的；`style` 依賴 `dom`，`dom` 不能引用它們。
- **bindings 沒有 `ObservableArray`**：宣告表有 `sequence`、`FrozenArray`、`Promise<T>`、帶 dictionary 的建構子和 indexed getter，沒有 set / delete 時執行演算法的陣列（`details.md` 的「bindings 和 `dom` 的現況」）。
- **Speedometer**：M3 的 TodoMVC-WebComponents、TodoMVC-Lit-Complex-DOM 卡在 `ReferenceError: CSSStyleSheet is not defined`（RFC 0012 的 `progress.md`）。前者 `new CSSStyleSheet()`、`replaceSync`、`shadowRoot.adoptedStyleSheets = [...]`，Complex-DOM 的版本還 `push`；Lit 先看 `"adoptedStyleSheets" in Document.prototype && "replace" in CSSStyleSheet.prototype`，不成立就改在 shadow root 裡插 `<style>`，和 Chrome 走的路不同（`details.md` 的「Speedometer 用到的」）。

目標：
- `element.style`：`cssText`（讀寫）、`length`、`item()` 和 `[i]`、`getPropertyValue`、`getPropertyPriority`、`setProperty`（含 `important`）、`removeProperty`；camelCase（`backgroundColor`）和 dashed（`style['background-color']`）的寫法（`webkitFoo` 等 `-webkit-` 的寫法和 `cssFloat` 跟著第一個 `-webkit-` property 和 `float`，「camelCase 的屬性」）；`element.style = "…"`（`[PutForwards=cssText]`）。改了之後下一個 frame 畫出來，`getAttribute('style')` 和 `MutationObserver` 看到的和 Chrome 一致。
- `getComputedStyle(element)`：唯讀、live，讀的時候同步更新 style（需要時連 layout），`width` 等照 CSSOM 的 resolved value。
- 第 3 段：`new CSSStyleSheet(options)`、`replace`、`replaceSync`、`cssRules`、`insertRule`、`deleteRule`，`CSSStyleRule` 的 `selectorText` 和 `style`；`Document` 和 `ShadowRoot` 的 `adoptedStyleSheets` 是 `ObservableArray`（`push`、`splice`、`arr[i] = sheet` 都生效）。document 的 adopted stylesheet 下一個 frame 畫出來；shadow root 的存起來，K-2 讓 style 進 shadow tree 時生效。M3 的兩個 suite 跑得完，Lit 走和 Chrome 一樣的 adopted 那條路。
- 序列化（`cssText`、`getPropertyValue`、computed value）照 Chrome 的輸出，以 WPT 為準。
- 同時給 JS 和 wasm（`rfcs/0012-speedometer/README.md` 的「已決定的事項」第 7 條）。

不在範圍內：
- **白名單本身**：新的 property 和值由各自的 RFC 加（RFC 0013、路線圖的 S2 到 S7）；這份讓 CSSOM 認得白名單裡的全部，之後加 property 的 RFC 順便加它的序列化。
- **`var()` 的替換、自訂 property 的 cascade 和繼承**：路線圖的 S4。
- **CSSOM View 和 script 觸發的 layout**（`getBoundingClientRect`、`offset*`……）：路線圖的 D；這份只用它的 hook（「`getComputedStyle` 和同步的 style 更新」）。
- **不是 constructed 的 stylesheet 的 CSSOM**（`document.styleSheets`、`<style>.sheet`、`<link>.sheet`、`HTMLStyleElement`）：第 3 段只做 constructed stylesheet（A2，第 16 條）。`<style>` 的原文和 CSSOM 的修改要互相同步，和 `StyleEngine` 照原文比對的做法衝突，另外做（「之後的可能」）。
- **style 進 shadow tree、`:host`、`::slotted`、照 tree scope 收集 stylesheet**：K-2（路線圖），shadow root 的 adopted stylesheet 那時才生效。
- CSS Typed OM（`attributeStyleMap`、`computedStyleMap`）、`CSS.supports`、`CSS.escape`。
- `SVGElement`、`MathMLElement` 的 `style`：kun 還沒有這些 interface（`docs/limitations.md` 的「Bindings 和 DOM API」），等路線圖的 O。

## 設計概要

給網頁的作者（照 U1、V1）：

```js
const box = document.querySelector('.box');
box.style.backgroundColor = 'red';             // 同 setProperty('background-color', 'red')
box.style.setProperty('margin', '4px 8px', 'important');
box.style.getPropertyValue('margin');          // "4px 8px"
box.style.cssText;                             // "background-color: red; margin: 4px 8px !important;"
box.getAttribute('style');                     // 同上；MutationObserver 收到 `style` 的紀錄
box.style.setProperty('--accent', 'teal');     // 存起來、讀得回來；還沒有 var()，畫面不變
box.style.cursor = 'pointer';                  // kun 還不支援 `cursor`：當成一般的 JS 屬性，不進 cssText
getComputedStyle(box).display;                 // "block"：讀之前先更新 style
getComputedStyle(box).width;                   // "120px"：有 box 時是 layout 之後的值
```

給之後改 kun 的人：
- 新的 interface `CSSStyleDeclaration` 和 `CSSStyleProperties`（規範把 `element.style`、`getComputedStyle` 的型別改成後者，見 `../third_party/wpt/interfaces/cssom.idl`）宣告在新的 `interfaces/core_css.rs`（Blink 的 `core/css`）；`HTMLElement.style`、`Window.getComputedStyle` 照 RFC 0014 第 5 條，在既有的宣告裡開 `// CSSOM` 的區塊。
- `style` 給 CSSOM 的公開 API：宣告清單 `CssPropertyValueSet`（`crates/css/src/property_set.rs`；Blink 的 `CSSPropertyValueSet`）的解析、CSSOM 的讀寫和 `cssText`，序列化在 `crates/css/src/serialize.rs`（Blink 的 `CSSValue::CssText`、`StylePropertySerializer`），之後加上 computed value 的序列化（`CSSProperty::CSSValueFromComputedStyle`，第 2 段）；名稱的對應是 `css_property_for_attribute`（Blink 的 `CssPropertyInfo`）。之後每加一個 property，同時加它 specified 和 computed 的序列化。
- `dom_api` 的演算法讀 `style` attribute、交給 `style` 改、再經過一般的 `setAttribute` 路徑寫回；`dom` 不用知道 CSS。

## 詳細設計

### 物件

兩個新的 GC 型別（Z1），最衍生的 interface 都是 `CSSStyleProperties`：
- **`InlineCssStyleDeclaration`**（Blink 的 `InlineCSSStyleDeclaration`）：指向元素的 edge 和 wrapper；元素的 `NodeRareData` 撐住它，`element.style` 每次同一個（`[SameObject]`，和 `classList` 的 `DomTokenList` 一樣，RFC 0008 第 38 條）。
- **`CssComputedStyleDeclaration`**（Blink 的 `CSSComputedStyleDeclaration`）：指向元素的 edge、pseudo-element 和 wrapper；每次呼叫 `getComputedStyle` 新建（`[NewObject]`），但 live：每次讀都看元素當時的 style。唯讀：`setProperty`、`removeProperty`、`cssText` 的 setter 丟 `NoModificationAllowedError`。

新增的步驟照 `crates/dom/README.md` 的「怎麼修改」；動到 GC，RFC 完成時跑 `matrix` 和 `fuzz`，`crates/dom/soundness.md` 照新型別的規則更新。

1b 做了 `InlineCssStyleDeclaration`（`crates/dom/src/inline_style.rs`，`NodeRareData` 的 `inline_style`）。宣告表的兩個 interface 都是 `as Object`（`crates/dom_api/src/interfaces/core_css.rs`），成員收到 `Object` 再取出它的 handle：第 2 段加上 `CssComputedStyleDeclaration` 時只多一個分支，宣告表和 WIT 不用改。`style` 的 wrapper 建在元素的 realm（和 `classList` 一樣）。
- **indexed property**：`item()` 超出範圍回傳空字串（`getter CSSOMString item(...)`），而 `style[i]` 超出範圍要是 `undefined`。`dom_js` 原本以「getter 有沒有回傳東西」判斷，1b 起回傳型別不能是 null 的 getter 改看 `length`（Blink 產生的 indexed getter 都看 `length`）。WebIDL 的特殊操作是繼承的：`item` 宣告在 `CSSStyleDeclaration`，`CSSStyleProperties` 的物件也要有 interceptor，`dom_js` 改成沿著 parent 找；`@@iterator` 照 WebIDL 只裝在宣告 getter 的 interface 的 prototype。
- **沒做的成員**：`cssFloat`（還沒有 `float`），`dom_api` 的 `spec` 測試從 1b 起也比對 `cssom.idl`，它列在 `spec-missing.txt`。`parentRule` 在第 3 段有了：規則的 `style` 的是那條規則，`element.style` 和 `getComputedStyle` 的是 `null`。

### inline style 存在哪

（Q1）**`style` attribute 的字串照舊是唯一的來源**。每個 CSSOM 操作：讀 attribute → `style` 解析成宣告清單（暫時的 `CssPropertyValueSet`）→ 修改 → 有改變的話序列化，經過 `dom_api` 的 `setAttribute` 路徑寫回（`MutationObserver` 的紀錄和 `oldValue`、`attributes_changed` 的 style invalidation、attribute 的 hook 都照舊）；沒有改變（不合法的值、移除不存在的 property）就不寫、不產生紀錄（WPT 的 `cssstyledeclaration-mutationrecord-*`）。讀的操作只解析。cascade 照舊從字串解析。這和 `DomTokenList` 每次從 attribute 解析是同一個做法。Chromium 存解析好的 property set、attribute 延遲同步（Q2），比較見「理由和其他做法」。

**宣告清單**（1a）：`CssPropertyValueSet` 是一條清單，每個 longhand 和自訂 property 最多一個，每個宣告（Blink 的 `CSSPropertyValue`）帶 `!important`、「shorthand 省略了它」（implicit）和「哪個 shorthand 設的」。stylesheet 的規則、`style` attribute、presentational hint 和 CSSOM 都用它，cascade 用的時候才照 importance 分。Blink 分成 immutable 和 mutable 兩種（parser 建前者，第一次 CSSOM 修改時換成後者）；kun 每次 CSSOM 操作都從 attribute 重新解析（Q1），一種就夠。原本的 `DeclarationBlock` 把 normal 和 important 分成兩條，序列化的順序會丟，所以換掉。
- **解析時的重複**照 Blink 的 `FilterProperties`：同一個 property 留最後一個，但前面有 `!important` 的時候留那一個（cascade 在同一個區塊裡本來就這樣）；結果是 normal 的在前、important 的在後，各自照留下來的那個的位置。所以 `color: red !important; width: 1px` 的 `cssText` 是 `width: 1px; color: red !important;`，和 Chrome 一樣（規範和 Firefox 保留原本的順序）。
- **`setProperty`** 照 Blink 的 `SetLonghandProperty`：已經有的就地換掉，沒有的加在最後；值、importance 和 implicit 都一樣時不算改變（不寫 attribute、沒有 `MutationObserver` 的紀錄）。shorthand 設定它的每個 longhand，有一個改變就算改變。

### 序列化和支援的 property

（R1、U1）
- **只認白名單**：`setProperty` 不認得的名稱或不合法的值什麼都不做（規範對「不支援的 property」的規定），`getPropertyValue` 是空字串；`cssText` 的 setter 丟掉不認得的宣告，所以 attribute 裡原本有、kun 不認得的宣告，第一次 CSSOM 修改之後就不見了。Chrome 對它不認得的也是這樣，只是 kun 認得的少。
- **從解析好的值序列化**（Blink 的 `CSSValue::CssText`），不是原文。原本解析時丟掉的資訊，1a 起留在 specified value 裡（computed value 不變）：長度的單位（`1in` 序列化成 `1in`，不是 `96px`）、顏色的關鍵字（`red`；hex 和 `rgb()` 寫成 `rgb()` / `rgba()`）、`thin` / `medium` / `thick`、`font-size` 的關鍵字、`font-weight` 的 `normal` / `bold`、`gap` 的 `normal`、`overflow` 的 `clip`、`transform` 的單軸寫法和省略的參數；`opacity` 的範圍在計算時才夾住（`-2` 讀回 `-2`）。兩項連 computed value 一起改：`self-start` / `self-end` 成為 `ItemAlignment` 的值（layout 照舊當成 `start` / `end`，每個 box 都是橫的、由左到右），只寫名字的 grid line 成為 `GridLine::Ident`（layout 當成第 1 條那個名字的線）：Chrome 的 computed value 也分得出它們，第 2 段用得到。computed value 另一套（`rgb(255, 0, 0)`、px），第 2 段寫的（`computed` 模組）。
- **數字**照 Chrome 的 `String::Number`：6 位有效數字、像 C 的 `%g`（1e-4 以下和 1e6 以上用指數，`2.34e+06`）。alpha 照 css-color 4：取兩位或三位小數裡，對回同一個 8-bit 值的那個。**函式和關鍵字的名稱**照 Chrome（`translateX`、`scaleX`；WPT 的 `transform-valid.html` 對 `scaleX` 預期小寫、對 `translateX` 保留大小寫，Chrome 兩個都保留）；`overflow: overlay` 照 Chrome 在解析時就變成 `auto`。
- **shorthand 只存 longhand**（parser 本來就展開，Blink 也是）：`getPropertyValue('margin')`、`cssText` 照 `StylePropertySerializer` 的規則合回 shorthand：所有 longhand 都在、importance 一樣，CSS-wide 關鍵字要嘛全部一樣、要嘛沒有；`cssText` 在第一個 longhand 的位置寫 shorthand，同一個 longhand 可以屬於幾個 shorthand 時先試 longhand 多的（`border` 先於 `border-width` 先於 `border-top`），已經寫進一個 shorthand 的 longhand 不再寫第二次。合不回去時 `getPropertyValue` 是空字串，`cssText` 寫 longhand。
- **kun 只有一部分 longhand 的 shorthand**：Chrome 的 `background`、`border`、`font`、`text-decoration` 還設 kun 沒有的 longhand（`background-image`、`border-image`、`font-variant`、`text-decoration-color`……），所以只在用那個 shorthand 設定過時才合得回去（單獨設 `background-color` 讀回 `background-color`，不是 `background`）。kun 用「哪個 shorthand 設的」模擬：其中至少一個 longhand 是那個 shorthand 設的，才合回它。`border` 那幾個 shorthand 省略的部分（implicit）合回時也省略（`border: 1px` 讀回 `1px`，不是 `1px none currentcolor`；Blink 的 implicit initial value），寫出來的 `medium none currentcolor` 照樣寫；`background: none` 讀回 `none`。
- 每種值照 Chrome 的輸出寫，預期值以 WPT 的 `css/*/parsing/*-valid.html`、`css/cssom/` 為準；逐項的注意事項見 `details.md` 的「序列化」，1a 的比對結果見 `progress.md`。

### 自訂 property

（V1）宣告清單保存 `--x` 的名稱和值的原文（規範的 token 序列，去掉頭尾空白），`setProperty('--x', …)`、`getPropertyValue('--x')`、`cssText` 讀寫得到，名稱分大小寫。cascade 不看它，`getComputedStyle` 讀不到（空字串），都等 S4。stylesheet 經過同一個 parser，也一起保存，S4 不用再改 parser；代價是 Complex-DOM 的 suite 每個有約 7500 個自訂 property 的宣告（`rfcs/0012-speedometer/details.md` 的「CSS」），解析和記憶體多一些。

### `getComputedStyle` 和同步的 style 更新

讀 computed style 前要先更新 style（Blink 的 `Document::UpdateStyleAndLayoutTreeForElement`），讀 layout-dependent 的 property（`width` 等的 resolved value）還要 layout（`Document::UpdateStyleAndLayoutForNode`），和 D 的 `getBoundingClientRect` 要的是同一個 hook。`dom_api` 和 engine、renderer 無關，碰到 renderer 只經過 `FrameClient`（`crates/dom_api/src/frame.rs`）；renderer 的實作（`FrameRequests`）現在只記下要求，碰不到 `StyleEngine` 和 `FrameView` 的狀態（`details.md` 的「`dom_api`、`style` 和 renderer」）。

（X2）**hook 歸 D**：D 設計 `Document::UpdateStyleAndLayout` 這一族（`FrameClient` 多哪些方法、renderer 的 lifecycle 狀態怎麼讓它碰到、借用怎麼安排）和幾何的查詢；這份的 `getComputedStyle`（「分階段」的第 2 段，第 9 條）排在 D 的 hook 之後，只使用它。理由：路線圖上 D 是 M1、`getComputedStyle` 是 M2，順序本來如此；resolved value 本來就要 D 的 layout 和幾何；`frame_view.rs`、`render_thread.rs` 是三條線共用的檔案（路線圖的「三條線都會碰的檔案」），lifecycle 的改動只由一份 RFC 設計。D（RFC 0018）做了 `LayoutClient` 的 `update_style_and_layout` 和 `element_box`，第 2 段再加只到 style 的 `update_style` 和盒子的 padding、margin（第 10 條）。

**這份對 hook 的需求**（當時寫進 D 的草稿，都由 D 和第 10 條做到）：
1. **兩個層級**：只到 style（大部分 property），和到 layout（`width`、`height`、`margin-*`、`padding-*`，之後 RFC 0013 的 `top` 等；Blink 的 `IsLayoutDependent`）。
2. **照元素的 document 更新**，不是呼叫的 window 的（`getComputedStyle(iframe.contentDocument.body)`）。沒有 lifecycle 的 document（沒有 frame 的）不更新，值是空字串；子 frame 有自己的 lifecycle（RFC 0009 的 2b-1）。
3. **style 乾淨時不做事**；更新過之後，下一個 `BeginMainFrame` 不重做（`FrameView` 的 dirty flag 一致）。
4. **在 script 執行中、只拿 DOM 的讀取 scope 呼叫**；不執行 script、不改 DOM（「持有 DOM 的借用時絕不呼叫 JS」照樣成立）。
5. **幾何**：元素的 box 的 content box 大小、margin、padding 的 used value（`getBoundingClientRect` 也用）；沒有 box（`display: none`、不在 document 裡）時是沒有。

其他：computed value 的序列化在 `style`（`ComputedStyle` 只是資料）。`display: none` 的子樹 kun 本來就有 style（`recalc_style` 不看 `display`），不用 Blink 的 `Element::EnsureComputedStyle`。`length` 和 `item()` 列出所有支援的 longhand，照名稱排序（WPT 的 `getComputedStyle-property-order.html`）；元素沒有 computed style（不在 render 的樹上）時什麼都不列、每個值是空字串（2d 時 WPT 的 `getComputedStyle-detached-subtree.html` 抓到，照 Blink）。shorthand 從 resolved 的 longhand 合回（第 15 條）。有些 computed value kun 存的和 Chrome 顯示的不同（例如 `overflow: clip` 算成 `hidden`），先照 kun 存的輸出，記進 `docs/limitations.md`（`details.md` 的「序列化」）。

### camelCase 的屬性

（Y2，1c）照 Blink：`CSSStyleProperties` 的 instance 裝 named interceptor（Blink 的 `AnonymousNamedGetter`、`AnonymousNamedSetter`、`NamedPropertyQuery`、`NamedPropertyEnumerator`），名稱到 property 的對應由 `style` 的 `css_property_for_attribute` 查（1a；Blink 的 `CssPropertyInfo`）：camelCase 每個大寫字母換成 `-` 加小寫，dashed 照原樣，大寫和 `-` 混用的不算，結果交給白名單的 `match`。Blink 的 `webkitFoo` / `WebkitFoo`（`-webkit-foo`）和 `cssFloat` 等 kun 有第一個 `-webkit-` property 和 `float` 時再加：現在加了也測不出來（任何寫法都對應不到 kun 的 property）。不認得的名稱不攔截，照一般的 JS 屬性。要補的：
- **宣告表的新形式**（`crates/dom_api/src/declare.rs`）：沒有名字的特殊操作 `[NamedGetter] getter(name) -> Option<T>`、`[NamedSetter] setter(name, value) -> bool`、`[SupportedNames] names() -> Sequence<DomString>`，是名稱為空字串的 `Member`，用 `Special` 標出種類；不裝在 prototype 上，也不在 `kun:dom` 裡（guest 用 `getPropertyValue` / `setProperty`），`spec` 測試和 IDL 那一側一樣跳過它們。setter 回傳「收不收這個名稱」：`CSSStyleDeclaration` 只收 property 的名稱（Blink 的 `NamedPropertySetterResult`），其他的照一般的 JS 屬性。
- **`dom_js` 的 named interceptor**（`crates/dom_js/src/properties.rs`）：有 named setter 時裝 setter，值照 setter 的參數型別轉換（`[LegacyNullToEmptyString]`：`style.color = null` 移除）；有 supported names 時裝 enumerator，名稱是 own key、可列舉；query（`'color' in style`）的屬性照這兩項決定可寫、可列舉，沒有的照舊是唯讀、不可列舉（`HTMLCollection`）。interceptor 照舊是 non-masking：prototype 上的成員優先。
- **列出的名稱**：每個支援的 property 的 camelCase 名稱一個（`style::css_property_attributes`，用標準的名稱，不列別名和 dashed），和 Blink 的 `NamedPropertyEnumerator` 一樣只列 camelCase；dashed 的名稱照樣讀寫得到、`in` 是 true。
- RFC 0017（路線圖的 H1）的 `dataset`（`DOMStringMap`）照這個形式用，再加上它要的 deleter（`[NamedDeleter]`）和 `[LegacyOverrideBuiltIns]`（第 8 條）。

### constructable stylesheet 和 `adoptedStyleSheets`

第 3 段（第 16 到 24 條）。給網頁的作者：

```js
const sheet = new CSSStyleSheet();             // options：baseURL、disabled（media 存著、不生效）
sheet.replaceSync('.app { color: teal }');     // 同步解析；@import 和其他 at-rule 丟掉
await sheet.replace('.app { color: navy }');   // 同樣同步解析，promise 當場 resolve（照 Blink）
sheet.cssRules[0].selectorText;                // ".app"；同一條規則每次是同一個物件
sheet.cssRules[0].style.color = 'red';         // 第三種 declaration：改的是規則
sheet.insertRule('p { margin: 0 }', 1);
document.adoptedStyleSheets = [sheet];         // 排在 document 自己的 stylesheet 之後，下一個 frame 生效
shadowRoot.adoptedStyleSheets.push(sheet);     // ObservableArray：push、splice、[i] = 都經過 set / delete 的演算法
```

**物件**（A2，第 16 條）。新的 interface 照 `cssom.idl`，宣告在 `interfaces/core_css.rs`：`StyleSheet`、`CSSStyleSheet`、`CSSRuleList`、`CSSRule`、`CSSStyleRule`；`CSSStyleRule.style` 是 `CSSStyleProperties` 的第三種物件。新的 GC 型別（Blink 的類別）：
- **`CssStyleSheet`**（`CSSStyleSheet`）：wrapper、constructor document 的 edge（kun 只有 constructed 的 stylesheet，所以沒有 constructed 的旗標）、`disabled`、`media` 的原文、解析好的內容（C3，下面）、規則的 wrapper 的清單（lazy，`CSSRuleList` 和 `CSSStyleRule` 的 identity）。只有 constructed 的 stylesheet：`ownerNode`、`href`、`parentStyleSheet`、`title` 都是 null，`type` 是 `text/css`。
- **`CssRuleList`**（`CSSRuleList`）：指向 stylesheet，`cssRules` 每次同一個（`[SameObject]`）；`length`、`item()`、`[i]` 照 stylesheet 當時的規則（live）。
- **`CssStyleRule`**（`CSSStyleRule`）：wrapper、所屬 stylesheet 的 edge（被拿掉之後清空，`parentStyleSheet` 是 null）、它自己那條規則的內容（拿掉之後照樣讀得到 `cssText`、`selectorText`，和 Blink 的 `CSSStyleRule` 帶著 `StyleRule` 一樣）。kun 的 parser 丟掉所有 at-rule，所以規則只有這一種；`CSSRule` 的 `type` 常數照 IDL 全部有。`CSSStyleRule` 直接繼承 `CSSRule`（G2，第 21 條）：規範和 Chrome（112 起）是 `CSSStyleRule : CSSGroupingRule`，style rule 自己有放巢狀規則的 `cssRules`、`insertRule`、`deleteRule`，但 kun 沒有 CSS nesting（巢狀的規則解析時丟掉），有這些成員會讓偵測 nesting 的 script 選錯路；差別記在 `spec-differences.txt` 和 `docs/limitations.md`。
- **`StyleRuleCssStyleDeclaration`**（Blink 的 `StyleRuleCSSStyleDeclaration`）：指向規則，規則撐住它（`[SameObject]`、`[PutForwards=cssText]`）。讀寫的是規則的宣告清單（`CssPropertyValueSet`），不是 attribute，所以沒有 `MutationObserver` 的紀錄；改了就是改了 stylesheet（下面的「改動」）。`CSSStyleDeclaration` 的實作照第 1 段的 `inline()` / `computed()` 再多一個分支。

成員的規則照 CSSOM，Blink 的行為不同時照 Blink：
- **建構子**：constructor document 是目前的 global 的 document；`baseURL` 照它的 base URL 解析，失敗是 `NotAllowedError`，沒給就用 document 的 base URL（規則裡的 `url()` 照它解析）；`disabled` 照存；`media` 收字串，存著（kun 的 stylesheet 本來就不看 media，`docs/limitations.md`），也就不做 `MediaList` 和 `media` 這個 attribute，`spec-missing.txt` 記著。
- **`replaceSync`、`replace`**：不是 constructed 的、或「不准修改」的旗標在時是 `NotAllowedError`（`replace` 是 reject）；解析新的原文換掉全部的規則，舊的規則的 wrapper 全部拿掉（`parentStyleSheet` 變成 null）。`@import` 照規範丟掉，其他 at-rule 本來就丟。`replace` 照 Blink 同步解析、promise 當場 resolve，所以「不准修改」的旗標在 kun 裡看不到（規範是平行解析，期間 `replaceSync` 丟例外）；WPT 測到時照實記失敗。
- **`insertRule`、`deleteRule`**：照 CSSOM 的「insert a CSS rule」：index 超出範圍是 `IndexSizeError`，解析不出一條規則是 `SyntaxError`（kun 不認得的 at-rule 也是，Chrome 收 `@media` 這些，記進 `docs/limitations.md`），constructed 的 stylesheet 收到 `@import` 也是 `SyntaxError`。舊的寫法 `rules`、`addRule`、`removeRule` 不做（Speedometer 和 WPT 的這一批都用不到），`spec-missing.txt` 記著。
- **`selectorText` 的 setter**：解析不出選擇器時不做事；序列化用 `selectors` 的 `ToCss`，照 Chrome 的寫法（`*` 省略等）。`cssText` 是 `選擇器 { 宣告; }`，宣告照第 1 段的序列化。

**解析好的規則放在哪**（C3，第 18 條）。照 Blink 的 `StyleSheetContents`：`style` 裡解析、值和序列化的部分拆成 `dom` 下面的新 crate `css`（3a'），`StyleSheetContents`、`StyleRule` 是它的公開型別；`dom` 依賴 `css`，`CssStyleSheet` 用 `#[no_trace]` 的欄位拿著 `Arc<css::StyleSheetContents>`，`CssStyleRule` 拿著 `Arc<css::StyleRule>`。`dom` 多一個 `NoTrace` 實作，照 `computed_style::ComputedStyle` 的先例：`css` 的 crate 文件寫明它只有純資料（沒有 GC 的指標或 root、沒有自己寫的 `Drop`、沒有 interning 和全域或 thread-local 的快取），`Send + Sync + 'static` 的 assertion 排除 `Rc`、`Cell` 和 raw pointer，GC 的指標和 root 由「`css` 不依賴 `dom` 和 V8」排除，其餘（`selectors`、`cssparser` 的型別樹，`url::Url`）照 `crates/dom/soundness.md` 審查，那些 crate 換版本時重看。

**設計的變化**：原本（2026-10-12 排第 3 段時）打算 `style` 公開不透明的 `StyleSheetContents`，`dom` 存型別抹除的 `Arc<dyn Any + Send + Sync>`（C2）。3b 開始時發現它不成立：`crates/dom/soundness.md` 的「`NoTrace` impl」要審查整棵型別樹、明確排除型別抹除的欄位；assertion 擋不住 GC 的指標（`Edge<T>` 的 `Send` 跟著 `T`：`Edge<Node>` 不是，`Edge<Blob>` 卻是 `Send + Sync`），也擋不住 root（`v8::Global` 是 `Send`）；型別抹除之後，審查看不到實際的型別。所以改成讓 `dom` 看得到具體型別（第 18 條改寫，比較見「理由和其他做法」的 C）。
- **共用**：內容不可變，換一份就是新的 `Arc`。Speedometer 的每個 todo-item 的 shadow root 都 adopt 同幾份 stylesheet，只解析一次；`StyleEngine` 也拿同一個 `Arc`，不複製。
- **改動**（`replaceSync`、`insertRule`、`deleteRule`、`selectorText`、規則的 `style`）一律做出新的內容（規則的清單是 `Arc` 的 vector，只換改到的那條），換進 stylesheet，再標記 constructor document 要重算 style（下面的「收集和 invalidation」）。規則的 wrapper 記著自己那條規則的 `Arc`，改了就換掉自己的、再在 stylesheet 的內容裡找到同一個 `Arc` 換掉；已經拿掉的規則只換自己的。
- **規則的 wrapper 的清單**和 adopted 的清單一樣，是 stylesheet 自己的 `EdgeChunks`，就地改（S1，第 23 條）：規則的 wrapper 放在它的規則的位置，`insertRule`、`deleteRule` 用 `Edge::set_from` 把後面的搬一格，`replaceSync` 全部清掉；chunk 本身只往後加，不用 `HeapVector`（RFC 0008 第 17 條）。

**`adoptedStyleSheets` 是 `ObservableArray`**（B1，第 17 條）。照 WebIDL 和 Blink（`V8ObservableArrayCSSStyleSheet`、`ObservableArrayExoticObject`）：
- **宣告表的新形式**：`attribute adoptedStyleSheets: ObservableArray<CSSStyleSheet>, list = …, set = …, delete = …;`，`dom_api` 照 WebIDL 的 backing list 給逐項的操作（O1，第 22 條）：`list`（整份清單，長度和每一項從它來）、`set(i, value)`（先執行 set 的演算法再存，`i` 等於長度時往後加）、`delete`（先執行 delete 的演算法再拿掉最後一項）；型別 `Base::ObservableArray` 帶著它們（`ObservableArrayDesc`），getter 給整份、setter 先全部刪掉再逐個 set（`observable_array::replace`）。set 的演算法：`value` 不是 constructed 的、或 constructor document 不是這個 `DocumentOrShadowRoot` 的 node document，丟 `NotAllowedError`；delete 的演算法這裡什麼都不做。清單本身存在 `dom`（`Document` 和 `ShadowRoot` 的 rare data，就地改的 `EdgeChunks` 加上長度，S1，第 23 條）；同一份 stylesheet 可以出現好幾次（規範允許）。
- **`dom_js`**：attribute 的 getter 回傳一個 Proxy，target 是空的 Array（`Array.isArray` 是 true），handler 是 native 的 trap（`get`、`set`、`has`、`deleteProperty`、`defineProperty`、`getOwnPropertyDescriptor`、`ownKeys`、`preventExtensions`），每個 realm 共用一個（存在 global 的 private property）；target 用 private property 記著擁有清單的平台物件和是哪個 attribute（getter 的編號），trap 從第一個參數（target）找回清單（H2，第 24 條）。照 WebIDL 的「observable array exotic object」經過上面的逐項操作讀寫 `dom` 的清單：設 `[length]` 是 index 時往後加，`length` 變短時從後面逐個執行 delete 的演算法；`push`、`splice` 都是經過這些 trap 的一般 Array 方法。Proxy 每個平台物件一個（`[SameObject]`，存在 wrapper 的 private property，和 `FrozenArray` 的快取同一個做法）。attribute 的 setter 照 WebIDL：把 iterable 轉成 sequence，清空，再逐個執行 set 的演算法。
- **WIT**：guest 沒有能就地修改的陣列：getter 回傳 `list<own css-style-sheet>`，setter 收 `list<borrow<css-style-sheet>>`，照 attribute 的 setter 的步驟換掉整份（W1，第 20 條）。

**收集和 invalidation**（D1，第 19 條）。
- renderer 在重算之前從 `dom` 讀 document 的 adopted 清單，跳過 `disabled` 的，把每一份的內容（`Arc`）用 `StyleEngine::set_adopted_style_sheets` 交給它（和 `set_base_url` 一樣），`StyleEngine` 照舊自己收集 `<style>` 和 `<link>`。原本打算由 `StyleEngine` 自己讀 `dom`、3a 先在 `recalc_style` 多一個參數；3a 時改成交進去：`recalc_style` 有 56 個呼叫的地方，大多是別的 crate 的測試，而且交進去的清單讓 `style` 的測試不用 `dom` 的清單就能測。順序照規範和 Blink：document 自己的 stylesheet（tree order）在前，adopted 的照清單的順序在後，都是 author origin。
- 和上次比的是「原文的清單」加上「adopted 的內容的 `Arc` 指標的清單」：清單換了、或其中一份的內容換了（新的 `Arc`），就和 `<style>` 的原文改了一樣，整份重算（`docs/limitations.md` 的「Invalidation」）。不用版本號：內容不可變，指標不同就是改了。
- **誰來標記要重算**：adopted 的清單改了，標記那個 document；一份 stylesheet 的內容改了，標記它的 constructor document（規範只讓 stylesheet 被 adopt 進 constructor document 和它的 shadow root，所以不用像 Blink 的 `adopted_tree_scopes_` 記下誰 adopt 了它）。沒被 adopt 的 stylesheet 改了也標記，`StyleEngine` 比出指標的清單沒變，不重算。
- **shadow root 的清單存著、不收集**：style 不進 shadow tree（`docs/limitations.md`），K-2 照 tree scope 收集（Blink 的 `ShadowTreeStyleSheetCollection`）時才生效。照 tree scope 收集和照規則的 invalidation 不在這一段（D2，「之後的可能」）。
- **shadow root 換 document 時清空它的清單**（3d 時 WPT 的 `CSSStyleSheet-template-adoption.html` 發現 3c 漏了，照 Blink 的 `TreeScopeAdopter::MoveShadowTreeToNewDocument`）：清單裡是舊 document 建的 stylesheet，到了別的 document 就不合 set 的演算法；搬進、搬出 `<template>` 的內容（一份 document 是另一份的 template contents owner document）時留著。

### JS 和 wasm

（W1）**兩邊都給，不需要例外**（RFC 0012 第 7 條）：`CSSStyleDeclaration` 的方法、`HTMLElement.style`、`Window.getComputedStyle` 都是一般的成員，宣告表自動產生 `kun:dom` 的函式（只新增，不升版，`crates/dom_api/README.md` 的「規則」）；camelCase 的屬性是 JS 的形式，本來就不在 WIT 裡。額外的成本：`python3 scripts/ci.py guests` 重新產生 Lua 的 binding、重編 guest；`crates/dom_wasm/tests/` 加一個 guest 測試（RFC 0008 的規則：新的物件種類測一次橋接）。第 3 段也兩邊都給（第 20 條）：`replace` 在 WIT 裡是 `async func`，`adoptedStyleSheets` 是整份 list 的 get 和 set（「constructable stylesheet 和 `adoptedStyleSheets`」），`CSSStyleRule.style` 和 `element.style` 是同一個 resource；都只新增，不升版。

### Process 的邊界

全部在 renderer 這一側（`dom_api`、`style`、`renderer`），不新增 `protocol` 的訊息：style 和 layout 都在 renderer 的 main thread 上，同步更新是函式呼叫，不是 IPC（Servo 以前的 layout thread 才要送 query，`details.md` 的「其他實作」）。之後用到 viewport 的值（`vw`、`@media`）來自 renderer 已經有的 visual properties。名稱的對應用 `match` 或 `const` 的表，不用 Blink 那種執行中建的快取：renderer 這一側不能有整個 process 共用的可變 `static`（`docs/architecture.md` 的「Process 的邊界」第 4 條；RFC 0014 第 6 條是同類的先例）。

### 測試

實際的測試、WPT 的範圍和結果見 `progress.md` 的「驗收」和「做過的檢查」；下面是開始時的計畫，和實際不同的地方寫在括號裡。

- **`style` 的單元測試**：每種值的解析和序列化來回、shorthand 合回、`!important` 和順序、自訂 property、名稱的對應。
- **`dom_api` 的測試**（沒有 engine）：每個方法、attribute 的同步（`cssText` 和 `getAttribute` 一致）、`MutationObserver`（有改變才有紀錄、`oldValue`）、`[SameObject]`、`[PutForwards]`；`getComputedStyle` 用測試用的 `FrameClient` 實作 hook（在讀取 scope 裡跑 `StyleEngine`）：改了之後讀到新值、live、唯讀、沒有 frame 的 document 是空字串。
- **`dom_js`**：named interceptor（camelCase、dashed、`in`、列舉、不認得的名稱照一般屬性、prototype 上的成員優先；`webkit` 和 `cssFloat` 沒做，「camelCase 的屬性」）。**`dom_wasm`**：guest 的 `setProperty` 和 `getComputedStyle`。
- **WPT 的 testharness**（`python3 scripts/test.py wpt`；不改 `../third_party/wpt`，只改 `tests/wpt/` 的清單）：`../third_party/wpt/css/cssom/` 裡 inline style 和 `getComputedStyle` 的部分；支援的 property 在 `css/*/parsing/` 的 `-valid`、`-invalid`（`-computed` 等 `CSS.supports`，第 14 條）（例如 `../third_party/wpt/css/css-flexbox/parsing/`，它們經過 `../third_party/wpt/css/support/parsing-testcommon.js` 用 `element.style[property]`）；`../third_party/wpt/css/css-variables/` 裡只測 CSSOM 的。檔案見 `details.md` 的「WPT」。
- **reftest**：`scripts/wpt_reftest_candidates.py` 允許 `.style` 和 `setProperty` 之後重跑，照 RFC 0003 的做法挑。
- **情境**：`about:demo`（`resources/demo.html`）加一個用 `element.style` 換顏色和大小的按鈕，和一行顯示 `getComputedStyle` 的結果，寫成 `tests/scenarios/` 的情境。
- **mutation test**：寫回 attribute、沒有改變時不寫、`!important`、序列化、shorthand 合回、interceptor 不攔截不認得的名稱、讀之前沒呼叫 hook（讀到舊值）。
- **第 3 段**：
  - `style`：`StyleSheetContents` 的解析、規則和選擇器的序列化、插入和刪除；adopted 的排在 document 的 stylesheet 之後、`disabled` 的不收；內容換了（新的 `Arc`）整份重算，同一份不重算。
  - `dom_api`：建構子的 options 和錯誤、`replace` / `replaceSync`（舊規則拿掉、`@import` 丟掉）、`insertRule` / `deleteRule` 的錯誤、規則的 identity 和 live 的 `cssRules`、規則的 `style` 改了 stylesheet、set 的演算法的 `NotAllowedError`（別的 document 建的；「不是 constructed 的」測不到：kun 的 stylesheet 都是 constructed 的）。
  - `dom_js`：`ObservableArray` 的每個 trap（`push`、`splice`、`[i] =`、`length` 變短、`delete`、`Array.isArray`、`ownKeys`、`[SameObject]`、set 的演算法丟例外時清單不變）。`dom_wasm`：guest 建 stylesheet、adopt 進 document。
  - WPT：`css/cssom/` 的 `CSSStyleSheet-constructable*.html`、`adoptedstylesheets-*.html` 等（`details.md` 的「WPT」；用 `document.styleSheets` 的 `CSSRuleList.html`、`CSSStyleRule*.html` 等跑了才知道整份卡住，沒收）；要 style 進 shadow tree 的照實記失敗，等 K-2。reftest：`document.adoptedStyleSheets` 換顏色。
  - 情境：`about:demo` 加一個按鈕，把一份 constructed stylesheet adopt 進 document、再 `replaceSync` 換掉它。
  - mutation test：adopted 的順序、`disabled`、指標比較、set 的演算法的檢查、Proxy 的 trap、拿掉的規則照樣讀得到、改規則沒有標記 document。

### 分階段

2026-10-07 排了第 1 段（第 1 條），同一天做完（`progress.md`）；`getComputedStyle` 原本是第 1 段的最後一步（1e），因為要等 D，拆成第 2 段（第 9 條），2026-10-09 做完。第 3 段 2026-10-12 排（第 16 到 20 條）。

**第 1 段：`element.style`**（2026-10-07 完成，merge 回 `main`）
1. **1a `style` 的 CSSOM 底層**：有順序、帶 `!important` 的宣告清單，specified value 保留單位和關鍵字，每種值的序列化、shorthand 合回、自訂 property 的保存、名稱的對應。畫面不變。
2. **1b `element.style`**：兩個 interface、`InlineCssStyleDeclaration`、`HTMLElement.style`、attribute 的同步和 `MutationObserver`；JS 和 wasm。M1。
3. **1c camelCase 的屬性**：宣告表的新形式、`dom_js` 的 named setter（和 RFC 0017 協調）。M1（runner 的 `frame.style.width = …`、ES5 的 `style.display`）。
4. **1d WPT 和 reftest**：`css/cssom/` 的 inline style 部分、`css/*/parsing/` 的 `-valid` / `-invalid`、reftest 的候選。

第 1 段不依賴 D 和 RFC 0009 的 2b-1。

**第 2 段：`getComputedStyle`**（D（RFC 0018）完成之後，2026-10-09 排、做完，merge 回 `main`；第 9 到 15 條；M2）。分成：

1. **2a `style` 的 computed value**：每個支援的 property 從 `ComputedStyle` 序列化成 Chrome 的寫法，和「computable」的清單、`length` 和 `item()` 的順序。只有 `style` 的單元測試，畫面和 DOM API 不變。
2. **2b hook**：`LayoutClient` 的 `update_style`（只到 style）和 `BlockBox` 的 padding、margin 的 used value（第 10 條），renderer 的實作；`dom_api` 這一側先不用。
3. **2c `getComputedStyle`**：`CssComputedStyleDeclaration`（唯讀、live）、`Window.getComputedStyle`（pseudo-element 的參數照第 12 條）、layout-dependent 的 property 的 resolved value（第 11 條）；JS 和 wasm。
4. **2d**：WPT（第 13 條）、情境和 `about:demo`、mutation test、文件。

**第 3 段：constructable stylesheet 和 `adoptedStyleSheets`**（K-1 之後，M3；2026-10-12 排、做完，第 16 到 24 條；設計見「constructable stylesheet 和 `adoptedStyleSheets`」）。原本的方向寫「`StyleEngine` 照 tree scope 收集 stylesheet（Blink 的 `TreeScopeStyleSheetCollection`）」，開始前改成只收 document 的（D1）：shadow tree 還沒有 style，照 tree scope 收集要等 K-2。分成：
1. **3a `style` 的 stylesheet**：公開的 `StyleSheetContents`（解析、規則和選擇器的序列化、插入、刪除、規則的宣告清單），`StyleEngine::set_adopted_style_sheets` 交進來的 adopted 內容排在 document 自己的後面、比指標。2026-10-12 完成（`progress.md`）。用 `style` 的單元測試，畫面和 DOM API 不變。
2. **3a' 拆出 `css` crate**（第 18 條）：`style` 裡不依賴 `dom` 的模組（`values`、`background`、`font`、`grid`、`effects`、`shadow`、`svg`、`properties`、`serialize`、`property_set`、`stylesheet`、`selector` 的解析那一半、`computed`、`canvas`、`css_namespace`）搬到新的 `css` crate，`style` 留下 `StyleEngine`、cascade、selector matching、presentational hint、SVG 的 `<use>` 和 paint server、`intersection_observer`、`media`，照舊 `pub use` 原本公開的 API。行為不變，所有測試照舊通過；`docs/architecture.md` 的 crate 表、`sanity.py` 的 renderer 端 crate 清單跟著改。開始前通知另外兩條線（rebase 會碰到搬家）。2026-10-12 完成（`progress.md`）。
3. **3b `CSSStyleSheet` 和規則**：`StyleSheet`、`CSSStyleSheet`、`CSSRuleList`、`CSSRule`、`CSSStyleRule`、`StyleRuleCssStyleDeclaration`，建構子、`replace`、`replaceSync`、`insertRule`、`deleteRule`；`dom` 的 `NoTrace` 和 `soundness.md`；JS 和 wasm。2026-10-12 完成。
4. **3c `adoptedStyleSheets`**：宣告表和 WIT 的 `ObservableArray`、`dom_js` 的 Proxy、`Document` 和 `ShadowRoot` 的清單、標記 document 要重算、renderer 在重算前把清單交給 `StyleEngine`；JS 和 wasm。M3 的兩個 suite 在這一步跑得完。2026-10-12 完成。
5. **3d**：WPT、reftest、情境和 `about:demo`、mutation test、Speedometer 的重跑（Chrome 那一側在 Windows 上不跑，只確認兩個 suite 在 kun 上跑完）、文件；WPT 找到的 shadow root 換 document 時清空清單也在這一步補上。2026-10-12 完成。

## 缺點

- **序列化要逐型別照 Chrome 寫**：之後每加一個 property（RFC 0013、S2 到 S7）都要多寫 specified 和 computed 的序列化和測試；`style` 的 specified value 要改成保留單位和關鍵字。
- **Q1 每次 CSSOM 操作都解析、序列化整段 inline style**；連續設好幾個 property 時，每次都寫一次 attribute、產生一筆紀錄（和 Chrome 一樣），cascade 照舊每次解析字串。inline style 通常很短，等量過再優化（「之後的可能」）。
- **invalidation 粗**：inline style 一改，元素連同子樹重算 style（`docs/limitations.md` 的「Invalidation」），Blink 只重算元素自己；`getComputedStyle` 強制同步更新時放大這個代價。
- **白名單外的 property 對 script 不存在**（U1）：`style.cursor = 'pointer'` 變成一般的 JS 屬性，讀得回來但不進 `cssText` 和 attribute；看 `'cursor' in style` 做 feature detection 的頁面會走退路。
- **bindings 多一種形式**（沒有名字的 named setter），要在 `dom_js`、宣告表和 WIT 的排除規則各做一次，加上 mutation test。第 3 段再多一種（`ObservableArray` 的 Proxy），是 bindings 裡第一個自己寫 trap 的 Proxy。
- **第 3 段之後畫面還不完全對**：shadow root 的 adopted stylesheet 要等 K-2 才生效，M3 的兩個 suite 跑得完，但畫的是 light DOM、沒有它們的 style，量 Speedometer 時 rendering 還不能比。
- **`css` crate 永遠是純資料**（第 18 條）：之後的 CSS RFC 新增的值型別不能有 interning、全域快取或自己寫的 `Drop`，要照 `soundness.md` 審查；改 CSS 會連帶重編 `dom`。
- **adopted stylesheet 改一點也整份重算**：和 `<style>` 的原文改了一樣（D1）；Blink 只重算受影響的 tree scope 和規則。

## 理由和其他做法

Q、R、U、V、Z、W、Y、X 在 2026-10-07 確認（「已決定的事項」第 2 到 9 條），第 3 段的 A、B、C、D 在 2026-10-12 確認（第 16 到 19 條）。代號只在這份裡用，和路線圖的代號無關（第 3 段的 C 不是路線圖上這份 RFC 的代號 C）；完整的比較見 `details.md` 的「選項的完整比較」。

- **Q. inline style 存在哪**：
  - **Q1. 只存 attribute 字串**，CSSOM 每次解析、改、序列化寫回：一個來源，`getAttribute`、`MutationObserver`、`cloneNode`、選擇器的 `[style]` 都不用改；`dom` 不碰 CSS 的型別。
  - **Q2. 照 Chromium 存解析好的 property set**（`ElementData` 的 inline style，`MutableCSSPropertyValueSet`），attribute 延遲同步（`SynchronizeStyleAttributeInternal`）：cascade 不用每次解析；但 `style` 依賴 `dom`，宣告的型別放不進 `Node`（要搬到 `dom` 下面的 crate，或存成不帶型別的 `Arc<dyn Any>`），而且每個讀 attribute 的地方都要先同步，漏一處就讀到舊值。
  - **Q3. Q1 加上解析結果的快取**（Servo 的 attribute 值帶著解析好的 block）：Q1 的語意，省掉重複解析；快取放哪裡有 Q2 一樣的 crate 依賴問題。
  - 決定：Q1（第 2 條）；量到解析是瓶頸再做 Q3。
- **R. 序列化**：**R1. 從解析好的值序列化**（規範和 Blink），specified value 要保留單位和關鍵字；**R2. 保存每個宣告的原文**：簡單，但 Chrome 會正規化（大小寫、`#fff` 變 `rgb()`、shorthand 合回），shorthand 展開的 longhand 也沒有原文，終究要 R1。決定：R1（第 3 條）。
- **U. 白名單外的 property**：**U1. 照規範當成不支援**；**U2. 不認得的宣告也存起來**（`cssText` 讀得回來，不生效）：script 以為支援，feature detection 選錯路，cascade 和 CSSOM 看到的不一致；**U3. 認得所有 CSS property 的名稱**（Chromium 的清單），值存原文、不生效：`in` 和讀回對了，但要維護一份和白名單分開的名稱清單，`setProperty` 也驗證不了值。決定：U1（第 4 條）；M1、M2 實際跑起來，若有 suite 依賴讀回不支援的 property 再考慮 U3。
- **V. 自訂 property**：**V1. 存起來、不生效**；**V2. 等 S4**（`setProperty('--x')` 什麼都不做）：runner 設的 CSS 變數讀不回來，WPT 的 CSSOM 測試也少一批；**V3. 連 cascade 和繼承一起做**（`getComputedStyle` 讀得到），只是不做 `var()`：是 S4 的前一半，範圍變大。決定：V1（第 5 條）。
- **X. 同步的 style 更新和 D 的分工**（決定：X2，第 9 條）：**X1. 這份先做只到 style 的 hook**，D 再加 layout；**X2. hook 歸 D**，`getComputedStyle` 排在 D 之後；**X3. hook 獨立成一份小的 RFC**（或 D 的第 0 步），C 和 D 都依賴它：多一份 RFC，只為一個介面。決定：X2（第 9 條；理由見「`getComputedStyle` 和同步的 style 更新」）。
- **Y. camelCase 的屬性**：**Y1. 照規範在 prototype 上產生 attribute**（Gecko、WebKit、Servo 的做法），每個 property 三個名字：`in` 和屬性描述都對，但宣告表要從 `style` 的清單產生成員，WIT 要排除它們；**Y2. 照 Blink 用 named interceptor**：清單只在 `style` 一處，加 property 不用動 bindings，named setter 也是 `dataset` 要的；和規範不同的是屬性不在 prototype 上（記進 `crates/dom_api/tests/spec-differences.txt`）。決定：Y2（第 8 條）。
- **Z. GC 型別**：**Z1. 照 Blink 的類別，inline 和 computed 各一個**；**Z2. 一個型別帶種類的欄位**：少一個型別，但和 Chromium 的分層對不上。RFC 0014 第 4 條之後新增型別的成本低。決定：Z1（第 6 條）。
- **W. JS 和 wasm**：**W1. 兩邊都給**；**W2. 先只給 JS**（RFC 0012 第 7 條的例外）：省掉重編 guest 和一個測試，但 wasm 文件只能用 `setAttribute('style')`。CSSOM 的成員都是一般的方法和 attribute，給 wasm 幾乎不用多做事。決定：W1（第 7 條）；第 3 段照樣（第 20 條）。
- **A. 第 3 段的範圍**：**A1. 只做 Speedometer 和 feature detection 要的**（建構子、`replace`、`replaceSync`、兩個 `adoptedStyleSheets`）：最小，但 WPT 的 `CSSStyleSheet-constructable*`、`adoptedstylesheets-*` 大量用 `cssRules` 和 `insertRule`，收不到幾個；**A2. 再加上 constructed stylesheet 的規則的 CSSOM**（`cssRules`、`insertRule`、`deleteRule`、`CSSStyleRule` 和它的 `style`）：原本方向就寫了；**A3. 再加上不是 constructed 的**（`document.styleSheets`、`<style>.sheet`）：`<style>` 的原文和 CSSOM 的修改要互相同步，和 `StyleEngine` 照原文比對的做法衝突。決定：A2（第 16 條）。
- **B. `adoptedStyleSheets` 的型別**：**B1. 照規範的 `ObservableArray`**（Chrome 99 起）：`push`、`splice` 都生效，bindings 多一個 Proxy 的形式，之後別的 `ObservableArray` 也用它；**B2. 可以寫的 `FrozenArray`**（Chrome 73 到 98）：不用新的形式，default 的兩個 suite 都夠用（default 的 TodoMVC-WebComponents 不 `push`），但 `push` 丟 `TypeError`，Complex-DOM 的版本和 WPT 失敗，之後還是要換。決定：B1（第 17 條）。
- **C. 解析好的規則放在哪**（2026-10-12 先決定了 C2，3b 開始時發現 C2 不成立，重新比較）：**C1. 整份 stylesheet 只存原文**（和 Q1 一樣）：`insertRule` 和改規則的 `style` 每次都要整份重新序列化，規則的 wrapper 也沒有自己的內容可以帶著；**C1'. 每條規則存原文**：`dom` 存每條規則的 `Arc<str>`，`StyleEngine` 照指標快取解析結果，編輯只重新序列化改到的那一條；不用新的 `NoTrace`、不用搬家，但離 Blink 最遠，之後 `<style>.sheet`（A3）和 CSSOM 難以共用內容；**C2. `dom` 存型別抹除的 `Arc<dyn Any + Send + Sync>`**：違反 `soundness.md` 的 `NoTrace` 規則（「解析好的規則放在哪」）；**C2'. 解析結果放在 heap 外的表，GC 物件只存編號**（和 abort steps 的 `DomTasks` 一樣）：結構和 C3 一樣，但 stylesheet 被回收之後清掉表裡的項目要自己寫；**C3. 解析、值和序列化拆到 `dom` 下面的 crate**：`dom` 拿著具體型別，最接近 Blink 和 Servo；代價是整個 crate 永遠綁在純資料的契約上、第三方 crate 的型別樹要審查、改 CSS 會連帶重編 `dom`。決定：C3（第 18 條）。
- **O. `ObservableArray` 在 `dom_api` 的介面**（3c 開始前）：**O1. 逐項的操作**（backing list、`set`、`delete_last`，WebIDL 的「set the indexed value」和「delete the indexed value」，Blink 的 `bindings::ObservableArray<T>` 也是逐項執行 set / delete 的演算法）；**O2. 整份讀、整份換**：trap 算出新的清單交給 `replace_all`，簡單，但沒改到的項目也重新檢查、中途丟例外時和 WebIDL 的結果不同、delete 的演算法有副作用的 attribute 不適用。決定：O1（第 22 條）。
- **S. 清單在 `dom` 的存法**（3c 開始前）：**S1. 就地改的 `EdgeChunks` 加上長度**（Blink 的 backing list 是就地改的 `HeapVector`）：不用新的 GC 型別，修改不用配置；**S2. 寫入時複製**（原本的寫法）：每次修改建一個新的清單物件，多一個 GC 型別、每次配置，而 `StyleEngine` 照內容的 `Arc` 比對，用不到清單換了一份這件事。決定：S1（第 23 條）。
- **H. Proxy 的 trap 怎麼找到清單**（3c 開始前）：**H1. 照 Blink**：handler 是 backing list 的 wrapper，每個清單一個，要清單是自己的 GC 物件（S2 的方向）；**H2. 每個 realm 共用一個 handler**，target 記著平台物件和 attribute，trap 從 target 找回清單：不用新的 GC 型別，每個 Proxy 只多一個 Array。決定：H2（第 24 條）。
- **D. 收集和 invalidation**：**D1. 照現在的粒度**：只收 document 的 adopted、排在它自己的之後，改了整份重算；**D2. 照 Blink 照 tree scope 收集（`TreeScopeStyleSheetCollection`），只重算受影響的 tree scope 和規則**：tree scope 要等 K-2 讓 style 進 shadow tree，照規則的 invalidation 是 style 的整體改進。決定：D1（第 19 條），D2 留在「之後的可能」。

## 先例

- **Chromium**：`CSSStyleDeclaration` → `AbstractPropertySetCSSStyleDeclaration` → `InlineCSSStyleDeclaration`（`element.style`）、`StyleRuleCSSStyleDeclaration`（rule 的 `style`）；`CSSComputedStyleDeclaration`（`getComputedStyle`）。inline style 存在 `ElementData` 的 property set，CSSOM 改了之後 `Element::InlineStyleChanged` 標 style、把 attribute 標成 dirty，讀 attribute 時 `SynchronizeStyleAttributeInternal` 才序列化；`MutationObserver` 的紀錄由 `StyleAttributeMutationScope` 在改動時排。`getComputedStyle` 每讀一個值先 `UpdateStyleAndLayoutTreeForElement`，layout-dependent 的再 `UpdateStyleAndLayoutForNode`。camelCase 是 named interceptor。照做：類別的分法和名稱、序列化的規則、hook 的兩個層級；不照做：property set 存在元素上和延遲同步（Q1）。細節見 `details.md` 的「Chromium 的做法」。第 3 段：`CSSStyleSheet` 帶著 `StyleSheetContents`（內容相同的 stylesheet 共用，改的時候複製），規則的 wrapper 在 `child_rule_cssom_wrappers_`；`TreeScope` 的 `adoptedStyleSheets` 是 `V8ObservableArrayCSSStyleSheet`，set 的演算法在 `TreeScope` 檢查 constructed 和 constructor document；`StyleEngine` 照 tree scope 收集，adopted 的排在後面。照做：類別、內容的共用和改的時候換一份、`ObservableArray`、adopted 的順序；不照做：照 tree scope 收集和細的 invalidation（D1）。規範的 `replace` 平行解析，kun 和 Blink 一樣同步解析。
- **WebKit** 和 Chromium 同源（`StyledElement` 的延遲同步、`StyleAttributeMutationScope`）；**Gecko** 的 attribute 值本身就是解析好的宣告，camelCase 是產生的 attribute；**Servo**（Stylo）的 attribute 值同時帶字串和解析好的 block、CSSOM 改了之後立刻序列化，最接近 Q3（`details.md` 的「其他實作」）。

## 已決定的事項

1. **只排第 1 段，照 1a 到 1e 的順序**（2026-10-07）：第 1 段是 inline style 和 `getComputedStyle`（「分階段」）；每一步開始前只決定那一步要決定的事：Z、W 在 1b 前，Y 在 1c 前（和 RFC 0017 協調），X 在 1e 前（看 D 的進度；之後 1e 拆成第 2 段，第 9 條）。constructable stylesheet 等 K-1，開始前再寫一次設計。不選「整份一起排」：它依賴的 shadow DOM 還沒有 RFC。
2. **inline style 只存 `style` attribute 的字串**（2026-10-07；Q1，「inline style 存在哪」）：CSSOM 每個操作解析 attribute、修改、有改變時序列化，經過 `setAttribute` 的路徑寫回；cascade 照舊從字串解析。和 `DomTokenList` 同一個做法。不選 Q2（照 Chromium 把 property set 存在元素上、attribute 延遲同步）：`style` 的型別要放進 `Node`，而且每個讀 attribute 的入口都要先同步，漏一處只在「改過、還沒讀過」時出錯，不容易測到；也不選 Q3（加上解析結果的快取）：快取放哪裡有 Q2 一樣的 crate 依賴問題，等量到解析是瓶頸再做（「之後的可能」）。
3. **從解析好的值序列化**（2026-10-07；R1，「序列化和支援的 property」）：照規範和 Blink，輸出以 Chrome 和 WPT 為準；specified value 改成保留單位、顏色的關鍵字和其他解析時丟掉的資訊，computed value 不變。不選 R2（保存每個宣告的原文）：和 Chrome 的正規化不同，shorthand 展開出的 longhand 也沒有原文，終究要 R1。
4. **只認白名單裡的 property**（2026-10-07；U1）：白名單外的名稱對 CSSOM 來說不存在，`setProperty` 不做事、`getPropertyValue` 是空字串、不進 `cssText`；script 看到的和 cascade 一致。不選 U2（不認得的也存起來）：script 以為支援，feature detection 會選錯路；也不選 U3（認得所有 CSS property 的名稱）：要維護一份和白名單分開、跟著 Chromium 的名稱清單。M1、M2 跑起來之後，有 suite 依賴讀回不支援的 property 再考慮 U3。
5. **自訂 property 存起來、不生效**（2026-10-07；V1，「自訂 property」）：parser 保存名稱和值的原文（stylesheet 也是），CSSOM 讀寫得到；cascade 忽略，`getComputedStyle` 讀不到，都等 S4。不選 V2（等 S4）：runner 設的 CSS 變數讀不回來，WPT 的 CSSOM 測試少一批；也不選 V3（連 cascade 和繼承一起做）：是 S4 的前一半，會先替還沒寫的 S4 做設計決定。
6. **inline 和 computed 各一個 GC 型別**（2026-10-07，1b；Z1，「物件」）：`InlineCssStyleDeclaration` 和 `CssComputedStyleDeclaration`，照 Blink 的類別。不選 Z2（一個型別帶種類的欄位）：和 Chromium 的分層對不上，「指向好幾種物件之一」的 edge 反而比多一個型別麻煩。
7. **JS 和 wasm 都給**（2026-10-07，1b；W1，「JS 和 wasm」）：CSSOM 的成員都是一般的方法和 attribute，宣告表自動產生 `kun:dom` 的函式。不選 W2（先只給 JS）：省下的只有重編 guest 和一個測試。
8. **camelCase 的屬性照 Blink 用 named interceptor**（2026-10-07，1c；Y2，「camelCase 的屬性」）：property 的清單只在 `style` 一處，加 property 不用動 bindings；宣告表多一種「沒有名字的 named getter / setter」，`dataset` 也用它。RFC 0017 還沒開始，這種寫法由這份先定，它照用。不選 Y1（照規範在 prototype 上產生 attribute）：宣告表要從 `style` 的清單產生幾百個成員，WIT 要排除它們。和規範不同的地方（屬性不在 prototype 上）記進 `docs/limitations.md`：原本打算記進 `crates/dom_api/tests/spec-differences.txt`，但規範的 camelCase attribute 是 prose 產生的（「for each CSS property…」），不在 `cssom.idl` 裡，`spec` 測試比不到它們，那裡的一行會被當成「已經沒有不同」。

9. **`getComputedStyle` 拆成第 2 段，hook 歸 D**（2026-10-07；X2，「`getComputedStyle` 和同步的 style 更新」）：原本是第 1 段的最後一步（1e，第 1 條），但 D 還沒有 RFC，CSS 線上它前面還有 RFC 0009 的 2b-1；留在第 1 段的話，第 1 段一直做不完，`element.style`（M1 要的）也不能照階段的 merge 先進 `main`。所以 1a 到 1d 是完整的第 1 段，`getComputedStyle` 成為第 2 段，排在 D 的 hook 之後；constructable stylesheet 改成第 3 段。不選 X1（這份先做只到 style 的 hook）：lifecycle 的改動分成兩份 RFC 各做一次，`frame_view.rs`、`render_thread.rs` 是三條線共用的檔案；也不選「照原計畫等 D」：第 1 段要在 branch 上等很久。

10. **hook 分兩級，盒子補上 padding 和 margin**（2026-10-09，第 2 段；X2 的落實）：D（RFC 0018）的 `LayoutClient` 多一個 `update_style(scope, document)`，只把一份 document 的 style 更新好（Blink 的 `Document::UpdateStyleAndLayoutTreeForElement`）；layout-dependent 的 property 照舊走它的 `update_style_and_layout` 和 `element_box`，而 `BlockBox` 補上 padding 和 margin 的 used value——`width` / `height` 的 resolved value 是 content box 的大小，要從 border box 扣掉邊框和 padding，`padding-*`、`margin-*` 本身也要 used value。和 Blink 的兩級一致：讀 `color` 這種不看 layout 的 property 不跑 layout（Speedometer 的 runner 量的正是強制 layout 的時間，RFC 0018 的第 3 條）。不選「一律跑 `update_style_and_layout`」：改動最小，但每次讀都多跑整個 layout；不選「另開一個 `StyleClient`」：借出的機制和 `dom` 的 `unsafe` 論證要再來一份（RFC 0018 的第 9 條），沒有好處。
11. **resolved value 照 Blink 的 `IsLayoutDependent` 清單，在 kun 支援的範圍內全做**（2026-10-09，第 2 段）：`width`、`height`、`margin-*`、`padding-*`（`top` / `right` / `bottom` / `left` 等要 RFC 0013 的 `position`，那時再加）；其他 property 回傳 computed value。WPT 的 `-computed` 測的就是這一組。不選「先重跑 Speedometer 的盤點再決定」（「待決定」原本的想法）：盤點的工具還沒進 repo（路線圖 T 的 1b），要先做那件事，而盤點記不到的 camelCase 讀取也正是這一組；不選「先不做 resolved value」：`width` 會回 `auto`，和 Chrome 差很多。
12. **`getComputedStyle(el, pseudoElt)` 對 kun 沒有的 pseudo-element 回傳沒有對應 style 的 declaration**（2026-10-09，第 2 段）：空字串或 `null` 當成沒給；合法的 pseudo-element 選擇器（`::before`、`::after`、`::marker`…，kun 都還沒有，路線圖的 S3）回傳的 declaration 每個 property 都是空字串、`length` 是 0、`cssText` 是空字串；語法不合法的丟 `SyntaxError`。這就是 Blink 在 `Element::EnsureComputedStyle(pseudo)` 拿不到 style 時的結果（`CSSComputedStyleDeclaration` 的 `GetPropertyCSSValue` 回 nullptr）。不選「忽略參數、回元素自己的 style」：`getComputedStyle(el, '::before').content` 會讀到元素的值，比空字串更容易誤導頁面；不選「丟 `NotSupportedError`」：規範沒有這條，只是順手帶參數的頁面會整個壞掉。
13. **第 2 段的 WPT 照 `details.md` 列的檔案收**（2026-10-09，第 2 段）：`css/cssom/` 的 `getComputedStyle-*`、`computed-style-*`，加上 `css/*/parsing/` 的 `-computed`；失敗照實記 baseline。要 `position` 或 pseudo-element 的（`getComputedStyle-insets-*`、`-pseudo*`）照舊不收，等各自的 RFC。不選「整個 `css/cssom/` 目錄」（RFC 0018 的 3b 那種做法）：這個目錄還有一大批 `document.styleSheets` 和 constructable stylesheet 的檔案，是第 3 段的範圍，baseline 會在第 3 段整批重寫。

14. **`css/*/parsing/` 的 `-computed` 等 `CSS.supports`**（2026-10-09，2d；修正第 13 條）：這些檔案用 `css/support/computed-testcommon.js`，它的 `test_computed_value` 第二個斷言就是 `CSS.supports(property, specified)`，而 kun 沒有 `CSS` namespace（`docs/limitations.md` 的 CSSOM 那條）。實際跑過一輪：失敗的子測試幾乎全是 `CSS is not defined`，其餘卡在它前面那個斷言（kun 不支援的 property），**沒有一個子測試真的比對到 computed value**（數字記在 `progress.md`）。所以這一批不收，`CSS.supports` 做了再一起收（「之後的可能」）；收的是 `css/cssom/` 的 `getComputedStyle-*`、`computed-style-*`（不用那支 helper，2d 實際抓到了 bug，見 `progress.md`）。不選「照第 13 條收進來、baseline 記 1190 條失敗」：baseline 量很大又驗不到任何東西，`CSS.supports` 做了還要整批重寫；不選「順手做 `CSS.supports`」：`CSS` 是 WebIDL 的 namespace，宣告表還沒有這種形式，而且 `supports(conditionText)` 要條件的 parser，是另一件事。
15. **computed 的 shorthand 讀得到，從 resolved 的 longhand 合回**（2026-10-09，2d）：`getComputedStyle(el).margin` 這類 shorthand 用 1a 的那一份序列化（`CssPropertyValueSet` 的 `property_value`）從它的 longhand 合回，而且每個 longhand 都先取 resolved value，所以 `margin: auto` 讀到的是 layout 算出來的值——和 Blink 一樣（`StylePropertySerializer` 跑在 computed 的 property set 上）。`length` 和 `item()` 照舊只列 longhand（Blink 也是）。Chrome 的 `font`、`border`、`background`、`text-decoration` 還會設 kun 沒有的 longhand，合不回去就是空字串（和第 2 條的 U1 一致）。不選「shorthand 回空字串」：`getComputedStyle(el).margin` 在 Chrome 讀得到，差別沒有道理；不選「只用 computed value 不用 resolved」：同一個 property 經過 shorthand 和 longhand 讀到不同的值。

16. **第 3 段做 constructed stylesheet 連同它的規則的 CSSOM**（2026-10-12；A2，「constructable stylesheet 和 `adoptedStyleSheets`」）：`new CSSStyleSheet(options)`、`replace`、`replaceSync`、`cssRules`、`insertRule`、`deleteRule`、`CSSStyleRule` 的 `selectorText` 和 `style`，以及 `Document` 和 `ShadowRoot` 的 `adoptedStyleSheets`。WPT 的 constructable stylesheet 的測試大多要規則的 CSSOM。不選 A1（只做 Speedometer 要的）：收得到的 WPT 很少，規則的 CSSOM 之後還是要回來改同一批型別；不選 A3（連 `document.styleSheets`、`<style>.sheet`）：`<style>` 的原文和 CSSOM 的修改要互相同步，和 `StyleEngine` 照原文比對的做法衝突，另外做。
17. **`adoptedStyleSheets` 照規範是 `ObservableArray`**（2026-10-12；B1）：bindings 新增這個形式，`dom_js` 是 native trap 的 Proxy（Blink 的 `ObservableArrayExoticObject`），清單存在 `dom`；WIT 是整份 list 的 get 和 set。現在的規範和 Chrome（99 起）都是這樣，`push`、`splice` 都生效。不選 B2（可以寫的 `FrozenArray`，Chrome 73 到 98 的做法）：default 的兩個 suite 夠用，但 `push` 丟 `TypeError`，Complex-DOM 的 TodoMVC-WebComponents 和 WPT 失敗，之後還是要換。
18. **解析好的規則由 `CSSStyleSheet` 帶著，型別在 `dom` 下面的新 crate `css`**（2026-10-12，3b 開始時改寫；C3）：`style` 裡不依賴 `dom` 的解析、值、序列化的模組（約 9,000 行）拆成 `css`，`dom` 依賴它，拿著具體的 `Arc<StyleSheetContents>` 和 `Arc<StyleRule>`；內容不可變、改的時候換一份，所以共用和比較都只看 `Arc`。最接近 Blink（`CSSStyleSheet` 帶著 `StyleSheetContents`）和 Servo（DOM 的 `CSSStyleSheet` 拿著 `style` crate 的 `Arc<Locked<Stylesheet>>`）。`css` 整個 crate 照 `computed_style` 的契約只有純資料，`NoTrace` 照 `soundness.md` 審查。代價：之後每份 CSS 的 RFC 新增的值型別都受這個契約約束、要審查；改 CSS 會連帶重編 `dom`、`html`、`layout`、`paint`（量到改 `style` 之後重編到 `shell` 16 秒、改 `dom` 8 秒，粗估之後約 25 秒）；多一條 Blink 沒有的 `css` / `style` 界線（例如 `@media` 的型別要放 `css`）。原本決定的是 C2（`dom` 存型別抹除的 `Arc<dyn Any + Send + Sync>`），3b 開始時發現違反 `soundness.md` 的 `NoTrace` 規則（「解析好的規則放在哪」），重新比較之後改成 C3。不選 C1'（每條規則存原文、engine 照指標快取解析結果）：沒有長期代價、不用搬家，但離 Blink 最遠，之後 `<style>.sheet`（A3）和 CSSOM 難以共用內容；不選 C2'（解析結果放在 heap 外的表）：結構不比 C3 好，還要自己管 stylesheet 回收之後清掉表的生命週期。也不改用 Stylo：它的 `Atom` 用全域的 interner、drop 要上鎖，在 kun 的 `NoTrace` 規則下 `dom` 不能直接拿著它的 stylesheet，而且要換掉 `computed_style` 和 layout、paint 讀 style 的部分（要重新考慮的話另開 RFC）。
19. **只收 document 的 adopted stylesheet，排在它自己的之後，改了整份重算**（2026-10-12；D1）：`StyleEngine` 比對 adopted 的內容的 `Arc` 指標，和 `<style>` 的原文改了同一個粒度；shadow root 的清單存著，K-2 讓 style 進 shadow tree 時才收集。不選 D2（照 Blink 照 tree scope 收集、只重算受影響的 tree scope 和規則）：tree scope 要等 K-2，細的 invalidation 是 style 的整體改進，記在「之後的可能」。原本的方向寫的是照 tree scope 收集（「分階段」的第 3 段）。
20. **第 3 段 JS 和 wasm 都給**（2026-10-12；W1，同第 7 條）：`replace` 是 `async func`，`adoptedStyleSheets` 是整份 list 的 get 和 set，都只新增，不升版。
21. **`CSSStyleRule` 繼承 `CSSRule`，不是 `CSSGroupingRule`**（2026-10-12，3b 開始前；G2）：nesting 之前的形狀（Chrome 111 以前）。kun 的 parser 沒有 CSS nesting，style rule 裡巢狀的規則丟掉；照現在的 IDL 和 Chrome 繼承 `CSSGroupingRule` 的話，style rule 會有永遠空的 `cssRules`、一律丟 `SyntaxError` 的 `insertRule`，用 `'insertRule' in CSSStyleRule.prototype` 這類方式偵測 nesting 的 script 會以為 kun 支援，接著寫的巢狀 CSS 全被丟掉。和第 4 條（U1，只認白名單裡的 property）同一個原則：不讓 script 以為 kun 有它沒有的功能。原型鏈和 Chrome 不同，記進 `crates/dom_api/tests/spec-differences.txt`（`CSSStyleRule (parent)`）和 `docs/limitations.md`。不選 G1（照 IDL 和 Chrome 繼承 `CSSGroupingRule`，新增它和空的 `cssRules`）：原型鏈和 Chrome 一樣，之後的 `CSSMediaRule` 也要它，但現在只會誤導；kun 有 nesting 或做 `CSSMediaRule` 時改成 G1（「之後的可能」）。
22. **`ObservableArray` 在 `dom_api` 是逐項的操作**（2026-10-12，3c 開始前；O1）：每個 `ObservableArray` 的 attribute 給三個函式：整份 backing list（長度和每一項從它來）、`set(i, value)`、`delete_last()`（宣告表寫成 `list = …, set = …, delete = …`），`set` 先執行 set 的演算法再存（`i` 等於長度時往後加），`delete_last` 先執行 delete 的演算法再拿掉；`dom_js` 的 trap 和 WIT 的 setter（照 attribute 的 setter 先全部刪掉再逐個 set）都經過它們。照 WebIDL 的 backing list 和 Blink 的 `bindings::ObservableArray<T>`。不選 O2（整份讀、整份換）：沒改到的項目也重新檢查，中途丟例外時和 WebIDL 不同（WebIDL 是前面的已經改了），之後 delete 的演算法有副作用的 attribute 也不適用。
23. **`dom` 的清單就地改**（2026-10-12，3c 開始前；S1）：adopted 的清單和 3b 的規則的 wrapper 的清單一樣，是擁有者自己的 `EdgeChunks` 加上長度，`Edge::set`、`set_from`、`clear` 就地改（和 canvas 的 style slot 一樣）。不用新的 GC 型別、修改不用配置；renderer 每次把清單讀成內容的 `Arc` 交給 `StyleEngine`，它比的是 `Arc`，用不到「清單換了一份」。Blink 的 backing list 也是就地改的 `HeapVector`。原本寫的是寫入時複製（S2），3b 的規則的 wrapper 已經是就地搬動，這裡一起改正。
24. **Proxy 的 handler 每個 realm 共用一個，trap 從 target 找回清單**（2026-10-12，3c 開始前；H2）：target 是空的 Array，用 private property 記著擁有清單的平台物件（document 或 shadow root 的 wrapper）和 attribute；handler 的 trap 是 native 的 function，每個 realm 建一次，存在 global 的 private property（和 `iterable.rs` 的 iterator prototype 一樣）。不選 H1（照 Blink，handler 是 backing list 的 wrapper）：要清單是自己的 GC 物件和 wrapper，和第 23 條衝突。

## 待決定

（沒有）

## 之後的可能

- **Q3 的快取**：inline style 和 cascade 共用解析結果（量到解析是瓶頸時）。
- **`style` attribute 改變只重算元素自己**（Blink 的 `kLocalStyleChange`），子樹只在繼承的值變了時重算；屬於 invalidation 的整體改進（`docs/limitations.md` 的「Invalidation」）。
- 不是 constructed 的 stylesheet 的 CSSOM：`document.styleSheets`、`<style>.sheet`、`<link>.sheet`、`HTMLStyleElement`（A3）；`<style>` 的原文和 CSSOM 的修改互相同步。RFC 0008 第 17 條提過 `CSSRuleList` 可能要 HeapVector，第 3 段的清單用就地改的 `EdgeChunks` 就夠（第 23 條），規則很多、改得很頻繁時再考慮。
- **照 tree scope 收集 stylesheet 和照規則的 invalidation**（D2，第 19 條）：照 Blink 的 `TreeScopeStyleSheetCollection`（`DocumentStyleSheetCollection`、`ShadowTreeStyleSheetCollection`）每個 tree scope 一份；一份 stylesheet 改了，只重算 adopt 它的 tree scope，再照它的規則的選擇器（`RuleFeatureSet` 的 invalidation set）只重算受影響的元素。tree scope 的部分 K-2 做；規則的部分和 style 的 invalidation 的整體改進一起做（`docs/limitations.md` 的「Invalidation」）。
- **`CSSStyleRule` 繼承 `CSSGroupingRule`**（G1，第 21 條）：照現在的 IDL 和 Chrome（112 起），style rule 有放巢狀規則的 `cssRules`、`insertRule`、`deleteRule`。等 kun 支援 CSS nesting（parser 收巢狀的規則、cascade 照 `&` 比對），或做 `CSSMediaRule` 這類 grouping rule（它們也繼承 `CSSGroupingRule`）時一起改；那時拿掉 `spec-differences.txt` 的 `CSSStyleRule (parent)`。
- `MediaList` 和 `media` 的生效、`rules` / `addRule` / `removeRule`、`@import` 以外的 at-rule 的 `CSSRule`（`CSSMediaRule` 等，kun 的 parser 還不收 at-rule）。
- **`css/*/parsing/` 的 `-computed`**（第 14 條）：等的 `CSS.supports` 後來由 RFC 0031（SVG）做了，可以照第 13 條的範圍收進來、照實記 baseline。
- CSS Typed OM（`CSS.supports`、`CSS.escape` 已經有了）。
- `SVGElement`、`MathMLElement` 的 `style`（路線圖的 O）。
