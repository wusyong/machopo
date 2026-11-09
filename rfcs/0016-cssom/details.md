# RFC 0016 的細節：CSSOM（inline style 和 `getComputedStyle`）

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和看法以那邊為準，這裡不重複。

## 現在的程式

2026-10-07 讀 `main` 的程式，是 1a 之前的樣子：1a 換掉了 `DeclarationBlock`、讓 specified value 保留寫法、加上序列化（`README.md` 的「inline style 存在哪」「序列化和支援的 property」）。

### `style` 的 parser 和值

- **自己寫的 parser**：`crates/css/src/stylesheet.rs` 用 `cssparser` 的 `StyleSheetParser`、`RuleBodyParser` 解析 stylesheet 和宣告區塊；`crates/css/src/properties.rs` 的 `parse_declaration` 解析一個宣告。at-rule 全部丟掉。
- **白名單**：`LonghandId`（約 70 個 longhand）和 `Shorthand`（`margin`、`padding`、`border*`、`background`（只有顏色）、`font`、`text-decoration`（只有 line）、`overflow`、`flex`、`flex-flow`、`gap`、`grid-row` / `-column` / `-area` / `-template`、`place-*`）。名稱用 `match_ignore_ascii_case!` 對應，沒有執行中建的表。不認得的名稱是 `DeclarationError::UnsupportedProperty`，連同自訂 property（`--x`）一起丟掉、記進 debug 的 log。
- **shorthand 解析時展開成 longhand**，宣告區塊裡只有 longhand；CSS-wide 關鍵字（`initial`、`inherit`、`unset`）存成 `Value::CssWide`。
- **解析時就正規化**（序列化會用到、現在丟掉的資訊）：
  - `Length` 只有 `Px`、`Em`、`Rem`：絕對單位（`in`、`cm`、`pt`……）解析時換成 px。
  - 顏色（`parse_style_color`）：關鍵字、hex、`rgb()` 都變成 RGBA；`currentcolor` 保留。
  - `opacity` 的百分比換成 0 到 1 的數字。
  - `row-gap` / `column-gap` 的 `normal` 存成 `0px`。
  - `overflow: clip` 存成 `hidden`，`overlay` 存成 `auto`。
- **`DeclarationBlock`** 把 normal 和 `!important` 分成兩條清單，各自照原始順序；兩條之間的順序丟了。
- **沒有序列化**：值的型別只有 `Debug`；`to_css` 只有選擇器（`crates/style/src/selector.rs`，`selectors` 要的）。

### `style` attribute 怎麼解析、怎麼 invalidation

- `cascade`（`crates/style/src/lib.rs`）每次重算一個元素時，讀 `style` attribute、`DeclarationBlock::parse`，沒有快取；優先順序是 author 的 level、`style_attribute: true`，`!important` 照 level。
- attribute 的改變（`DomMutationScope::set_attribute`、`remove_attribute`，`crates/dom/src/scope.rs`）：給 `MutationObserver` 記下（有 observer 時才記 `oldValue`），`attributes_changed` 把元素標成 `needs_style_recalc`（它和整個子樹重算）、父元素標 `children_changed`（有看結構的選擇器時擴大），DOM tree version 加一。任何 attribute 都一樣，`style` 沒有特別處理。
- `classList` 的 `DomTokenList`（`crates/dom/src/dom_token_list.rs`）是 Q1 的先例：GC 物件只記元素和 attribute 的名稱，每次從 attribute 解析，改了就序列化寫回（`crates/dom_api/src/token_list.rs`）。

### `ComputedStyle`

- `crates/computed_style/src/lib.rs`：每個支援的 property 的 computed value，只放純資料（`dom` 把 `Arc<ComputedStyle>` 當 `NoTrace` 存在元素上）。長度是 px，百分比照寫的存，`currentcolor` 保留成 `StyleColor::CurrentColor`。
- 和規範的 computed value 不同、序列化時看得出來的：`overflow: clip` 存成 `hidden`（一軸 `visible`、一軸不是時，`finish` 照規範把 `visible` 算成 `auto`）；`border-*-width` 在 style 是 `none` 時是 0（和規範一樣）；`display` 已經 blockify（和 Chrome 的 `getComputedStyle` 一樣）；`z-index` 另有 `effective_z_index`，序列化用 `z_index`。
- `recalc_style` 不看 `display`：`display: none` 的子樹也有 computed style。不在 document 裡的元素沒有。

### `dom_api`、`style` 和 renderer

- **crate 的依賴**：`dom_api` 依賴 `dom`、`html`、`style`（`querySelector` 的 `SelectorQuery`）；`style` 依賴 `dom` 和 `computed_style`；`dom` 只依賴 `computed_style`。所以 `dom_api` 可以直接呼叫 `style` 的解析和序列化，但 `dom` 的 `Node` 放不進 `style` 的型別（Q2 的問題）。
- **`FrameClient`**（`crates/dom_api/src/frame.rs`）：`dom_api` 碰到 renderer 的唯一管道，renderer 實作、建 realm 時交給 engine；`dom_api` 用 `ScriptHost::frame`（執行中的 realm 的 frame）或 `document_frame`（節點的 document 的 frame）拿到它。它的文件寫明：script 執行中被呼叫，實作不能假設 renderer 自己的狀態是空的，只要求某件事發生的可以記下來之後做。
- **renderer 的實作 `FrameRequests`**（`crates/renderer/src/frame_view.rs`）只記下要求（連結、子 frame 的建立和導覽、testharness 的結果），碰不到 `FrameView`。`StyleEngine` 是 `RenderThread` 的欄位（`crates/renderer/src/render_thread.rs`），一個 renderer 一個，裡面快取「上一次解析的 author stylesheet」；`FrameView` 有 `linked_sheets` 和各階段的 dirty flag。
- **lifecycle**：`FrameView::update_all_lifecycle_phases` 在 `BeginMainFrame` 裡拿 DOM 的讀取 scope 跑 style → box tree → layout → pre-paint → paint，只跑 dirty 的階段（看 DOM tree version 決定要不要跑 style）。script 執行時，DOM 被 engine 可變借用（`run_script` 把 `&mut dom` 交給 engine），`dom_api` 的演算法經過 `ScriptHost::read()` 拿讀取 scope。所以同步更新的 hook 拿得到讀取 scope；要解決的是 `FrameClient` 怎麼碰到 `StyleEngine`、`linked_sheets` 和 dirty flag（D 的設計，X2）。
- **多個 frame**：子 frame 有自己的 lifecycle（RFC 0009 的 2b-1，2026-10-07）；`StyleEngine` 在 2b-1a 改成每個 `FrameView` 一個（Blink 的 `StyleEngine` 是每個 `Document` 一個）。

### bindings 的 interceptor

`crates/dom_js/src/properties.rs`：宣告表的 method 標了 `[IndexedGetter]` / `[NamedGetter]` 時，instance template 裝 V8 的 interceptor。named 的是 `NON_MASKING | ONLY_INTERCEPT_STRINGS`，只有 getter 和 query，沒有 setter、deleter、enumerator；設定會在物件上建一般的屬性（`docs/limitations.md` 的「Bindings 和 DOM API」）。`[NamedGetter]` 只能掛在既有的方法上（WIT 裡是一般的 method）。`[PutForwards=x]` 已經有（`classList`，RFC 0008 第 42 條），`element.style = "…"` 直接用。

### bindings 和 `dom` 的現況

第 3 段開始前（2026-10-12）看的。
- **stylesheet 的流向**：`style::style_sheet_texts`（`crates/style/src/lib.rs`）照 tree order、只沿著 `children()` 走 document，收 `<style>`（直接的 Text 子節點接起來）和載好的 `<link rel=stylesheet>`（`LinkedStyleSheets`，renderer 的 `FrameView::start_loads` 和 `did_receive_resource` 載的）；`media` 都不看。`StyleEngine::recalc_style` 每次重算都重新收集，原文的清單和上次不同就全部重新解析（`AuthorSheets`）、所有元素重算；規則攤平成一條，照 UA、author 的順序和 source order，cascade 對每個元素線性掃過全部規則。UA stylesheet 是 `Rc<Stylesheet>`，每個 render thread 解析一次。
- **shadow tree**：shadow root 的 `parent` 是空的、掛在 host 的 rare data，style、layout、paint 都只走 `children()`，沒有地方呼叫 `shadow_root()`；dirty flag 往上標也停在 shadow root。
- **解析好的型別**：`crates/css/src/stylesheet.rs` 的 `Stylesheet`（origin、`Vec<StyleRule>`、base URL）、`StyleRule`（`SelectorList<Impl>` 和 `CssPropertyValueSet`），都是 `pub(crate)`；at-rule 一律丟掉。`selectors` 的 `Impl` 的型別是 `CssString`、`CssIdent` 和兩個 enum，3a 用 static assertion 確認 `StyleSheetContents` 是 `Send + Sync`。
- **宣告表**（`crates/dom_api/src/declare.rs`）：`Sequence<T>` 和 `FrozenArray<T>` 都是 `Vec`，JS 收任何 iterable；`FrozenArray` 的結果凍結、快取在 wrapper 的 private property（同樣的長度、每一項 `strict_equals` 時回傳同一個）；還沒有 attribute 的 setter 收 list，但 macro 和兩個前端都接受。`Promise<T>` 回傳 `PromiseResolver`，WIT 是 `async func`。indexed getter 是 `[IndexedGetter] item` 加 `length`（`DOMRectList` 是 `CSSRuleList` 的樣子）。沒有任何 Proxy、陣列的 exotic object 或 set / delete 時執行演算法的 hook，`ObservableArray` 是新的形式。`DocumentOrShadowRoot` 不是 mixin，成員在 `Document` 和 `ShadowRoot` 各宣告一次、指向同一個收 `&NodeHandle` 的函式（`activeElement`）。
- **`dom` 的 GC**：`dom` 不能引用 `style` 的型別；非 GC 的資料放 `#[no_trace]` 的欄位，型別要是 `NoTrace`（別的 crate 的型別用 `Send + Sync + 'static` 的 assertion，`ComputedStyle`、`url::Url` 的先例）。可以增刪的 GC 指標清單沒有現成的：`EdgeChunks` 只往後加、從不刪（RFC 0008 第 17 條），其他可變的清單是 `Edge` 串起來的 linked list（listener、custom element 的 definition）。

## Chromium 的做法

依 2026-10-07 對 Chromium 原始碼（`third_party/blink/renderer/core/css/`、`core/dom/`）的了解整理；名稱和細節在實作每一步前要對照原始碼確認。

### 類別

| 類別 | 是什麼 |
| --- | --- |
| `CSSStyleDeclaration` | 抽象的基底，ScriptWrappable；IDL 的成員和 camelCase 的 named getter / setter（`AnonymousNamedGetter`、`AnonymousNamedSetter`、`NamedPropertyQuery`、`NamedPropertyEnumerator`） |
| `AbstractPropertySetCSSStyleDeclaration` | 在 `PropertySet()`（`MutableCSSPropertyValueSet`）上實作 CSSOM 的方法；改之前 `WillMutate`、改之後 `DidMutate(MutationType)` |
| `PropertySetCSSStyleDeclaration`、`StyleRuleCSSStyleDeclaration` | 獨立的 property set；`CSSStyleRule.style` |
| `InlineCSSStyleDeclaration` | `element.style`：`ParentElement()` 是元素，property set 是元素的 inline style；`DidMutate` 通知元素 |
| `CSSComputedStyleDeclaration` | `getComputedStyle`：元素、pseudo-element 的 id；唯讀，每個值即時算 |
| `MutableCSSPropertyValueSet` / `ImmutableCSSPropertyValueSet` | 宣告的清單（`CSSPropertyValue`：property、值、`IsImportant`）；parser 建的是 immutable 的，第一次 CSSOM 修改時換成 mutable 的 |
| `StylePropertySerializer` | `cssText` 和 shorthand 的 `getPropertyValue`：longhand 合回 shorthand 的規則 |
| `CssPropertyInfo`（`css_style_declaration.cc`） | camelCase、`webkit` 前綴、`cssFloat` 的名稱到 `CSSPropertyID`，結果放進一張 static 的快取 |

### inline style 的儲存和同步

- inline style 存在 `ElementData` 的 property set（`Element::InlineStyle()`、`EnsureMutableInlineStyle()`）。
- **attribute → property set**：`Element::StyleAttributeChanged` 呼叫 `SetInlineStyleFromString`，用 `CSSParser::ParseInlineStyleDeclaration` 解析，`SetNeedsStyleRecalc(kLocalStyleChange)`（只重算元素自己；子元素在繼承的值變了時才重算）。
- **property set → attribute（延遲）**：CSSOM 改了之後 `Element::InlineStyleChanged()` 標 style、`SetStyleAttributeIsDirty(true)`，不立刻序列化。讀 attribute 的入口（`getAttribute`、`attributes`、`hasAttribute`、序列化……）先呼叫 `SynchronizeAttribute`，`SynchronizeStyleAttributeInternal` 用 `AsText()` 序列化、以「同步」的理由寫進 attribute，不再觸發一次 `StyleAttributeChanged`。
- **`MutationObserver` 和 custom element**：`StyleAttributeMutationScope` 包住 CSSOM 的修改，最外層的 scope 記下改之前的 attribute 值，有改變時排 `style` 的 attribute 紀錄、`attributeChangedCallback`。沒有改變（`DidMutate` 收到沒有改變）就沒有紀錄。
- `Element::SetInlineStyleProperty`、`RemoveInlineStyleProperty`：Blink 內部（編輯、`<font>` 之類）直接改 inline style 的入口，最後一樣走 `InlineStyleChanged`。

### `getComputedStyle`

- `LocalDOMWindow::getComputedStyle` 建一個新的 `CSSComputedStyleDeclaration`（`[NewObject]`），pseudo-element 的字串解析成 id。
- 讀一個值（`GetPropertyCSSValue`）：先 `Document::UpdateStyleAndLayoutTreeForElement`（只到 style，必要時只更新那個元素的祖先鏈），property 是 layout-dependent（`CSSProperty::IsLayoutDependent`：`width`、`height`、`margin-*`、`padding-*`、`top` 等，元素有 layout object 時）時再 `Document::UpdateStyleAndLayoutForNode`；拿 `Element::EnsureComputedStyle()`（`display: none` 的子樹沒有 style，臨時算一份存在 rare data），再用 `CSSProperty::CSSValueFromComputedStyle` 轉成 `CSSValue`、`CssText()`。
- 文件沒有 frame 時，值是空字串。
- `length` 和 `item()` 列出「computable」的 property（`ComputableProperties`），照固定的順序。

### camelCase 的屬性

Blink 不在 prototype 上產生每個 property 的 attribute，而是用 named interceptor：getter 和 setter 先用 `CssPropertyInfo` 認名稱，不是 CSS property 就不攔截。所以 `'color' in el.style` 是 true（query），但 `Object.getOwnPropertyDescriptor(CSSStyleDeclaration.prototype, 'color')` 沒有東西，這點和規範（及 Gecko、WebKit）不同。

### 自訂 property

自訂 property 的宣告是 `CSSCustomPropertyDeclaration`，值是 `CSSVariableData`（token 的原文）；computed 的放在 `ComputedStyle` 的 `StyleVariables`，`getComputedStyle` 讀得到（包括繼承來的）。

### constructable stylesheet

`CSSStyleSheet::Create`（`new CSSStyleSheet(options)`）、`replace`（回傳 promise，不能有 `@import`）和 `replaceSync`；`TreeScope::adoptedStyleSheets`（`Document` 和 `ShadowRoot`，WebIDL 的 `ObservableArray<CSSStyleSheet>`，binding 產生 `V8ObservableArrayCSSStyleSheet`）；`StyleEngine` 照 tree scope 收集有效的 stylesheet（`DocumentStyleSheetCollection`、`ShadowTreeStyleSheetCollection`），adopted 的排在 tree scope 自己的 stylesheet 之後。

第 3 段開始前（2026-10-12）依記憶補的，實作時對原始碼確認：
- **內容和 wrapper 分開**：`StyleSheetContents` 是解析好的規則（`StyleRuleBase` 的清單），內容相同的 stylesheet 可以共用一份，CSSOM 要改時先複製（`CSSStyleSheet::WillMutateRules`）。`CSSStyleSheet` 是 CSSOM 的 wrapper，`child_rule_cssom_wrappers_` 平行地記著每條規則的 `CSSRule`，`cssRules` 的 `CSSRuleList` 每次同一個。`CSSStyleRule` 帶著自己的 `StyleRule`，拿掉之後照樣讀得到。
- **constructed**：`CSSStyleSheet::Create` 記下 constructor document 和 `IsConstructed`；`replace` 和 `replaceSync` 都同步解析（`SetText`，`@import` 丟掉並警告），`replace` 的 promise 當場 resolve。
- **adopted**：`TreeScope` 的 `adoptedStyleSheets` 是產生的 `V8ObservableArrayCSSStyleSheet`，set 的演算法（`TreeScope::OnAdoptedStyleSheetSet`）檢查 constructed 和 constructor document，delete 的演算法通知 `StyleEngine`；`CSSStyleSheet` 記下 adopt 它的 tree scope（`adopted_tree_scopes_`），改了時通知它們。JS 那一側是 `ObservableArrayExoticObject`：Proxy，handler 的 trap 是 native 的，讀寫 C++ 的清單。

## 其他實作

都是 2026-10-07 依記憶整理，引用前要對原始碼確認。

- **WebKit**：和 Blink 同源：`StyledElement` 存 inline style 的 property set，`inlineStyleChanged` 標 dirty，`synchronizeStyleAttributeInternal` 延遲序列化；`InlineCSSStyleDeclaration`、`CSSComputedStyleDeclaration`、`StyleAttributeMutationScope`。camelCase 的屬性近年改成從 property 清單產生的 IDL attribute。
- **Gecko**：`nsDOMCSSAttributeDeclaration`（inline）、`nsComputedDOMStyle`（`getComputedStyle`，讀之前 flush style 或 layout）；`style` attribute 的值（`nsAttrValue`）本身可以是解析好的宣告，要字串時才序列化。camelCase 是產生的 `CSS2Properties` 的 attribute。
- **Servo**：`CSSStyleDeclaration` 的 owner 是元素或 rule；元素的 `style` attribute 的值是 `AttrValue::Declaration`，同時帶字串和解析好的 `PropertyDeclarationBlock`，CSSOM 改了 block 之後立刻序列化成新的 attribute 值（Q3 的做法）。camelCase 是從 property 清單產生的 WebIDL attribute。`getComputedStyle` 以前是送 query 給 layout thread、等它 reflow 之後回答；kun 的 style 和 layout 都在 renderer 的 main thread，不需要這種往返。

## 選項的完整比較

### Q. inline style 存在哪

| | Q1 只存字串 | Q2 存 property set、延遲同步 | Q3 字串加解析結果的快取 |
| --- | --- | --- | --- |
| 真正的來源 | attribute | property set（attribute 可能是舊的） | attribute |
| `getAttribute`、`attributes`、`outerHTML`、`cloneNode`、`[style]` 選擇器 | 不用改 | 每個讀 attribute 的入口都要先同步 | 不用改 |
| `MutationObserver` 的 `oldValue` | `set_attribute` 照舊記 | 改動時自己序列化舊值、排紀錄 | 同 Q1 |
| 每次 CSSOM 操作 | 解析 + 序列化整段 | 只改 property set | 用快取的解析結果 + 序列化 |
| cascade | 照舊每次解析 | 直接用 property set | 用快取 |
| 型別放在哪 | `style` 裡的暫時物件 | `Node` 上：`style` 的型別要搬到 `dom` 下面的 crate，或 `dom` 存不帶型別的 `Arc<dyn Any + Send + Sync>`（`NoTrace`，要寫 soundness） | 同 Q2 |
| 和 Chromium | 不同 | 相同 | 不同（像 Servo） |

Q2 的風險在「每個讀 attribute 的入口都要先同步」：kun 讀 attribute 的地方分散在 `dom`（`attribute()`、`NamedNodeMap`、`Attr` 的值、`cloneNode`、`isEqualNode`）、`dom_api`、`html` 的序列化和 `style` 的選擇器比對，漏一處就讀到舊的值，而且只在「CSSOM 改過、還沒讀過 attribute」時出錯，不容易測到。Q1 的成本是重複解析：inline style 通常只有幾個宣告，cascade 本來就每次解析，先不優化。Speedometer 的 TodoMVC-ES5 每個 todo 只改 `style.display`。

### R. 序列化

- **R1** 照規範（「serialize a CSS value」、「serialize a CSS declaration block」）和 Blink：specified value 要保留單位（`Length` 加上絕對單位）、顏色的關鍵字（specified 的 `StyleColor` 要能記得原本是關鍵字；Chrome 的 specified 值把 hex 序列化成 `rgb()`，關鍵字照原樣）、`opacity` 的百分比。這些只改 specified value，computed value 不變。
- **R2** 存原文：`cssText` 讀回來和寫進去的一樣（大小寫、空白也一樣），和 Chrome 不同；shorthand 展開出的 longhand 沒有原文，`getPropertyValue('margin-top')` 還是要從值序列化。所以 R2 不能取代 R1，只能當 R1 寫完之前的過渡，不值得。

### U. 白名單外的 property

U1 的行為對 script 來說就是「kun 是一個不支援這些 property 的瀏覽器」，和 cascade 看到的一致，WPT 的預期結果（`-invalid` 之外）照實失敗。U3 讓 `'position' in el.style` 是 true、讀回原文，比較不會讓頁面走「不支援」的退路，但 kun 畫不出來，頁面以為畫得出來；而且名稱清單要和 Chromium 的同步。只有 U1 不用維護第二份清單。Speedometer 的 runner 設 `frame.style.position`、`left`、`top`、`transform`（`rfcs/0012-speedometer/details.md` 的「Speedometer 3.1 的結構」），只寫不讀，U1 不影響它跑完；iframe 的位置要等 RFC 0013。

### V. 自訂 property

V1 只動 parser 和宣告清單：自訂 property 的宣告是名稱加上值的 token 原文（規範：去掉頭尾空白，其他照原樣），`!important` 照一般的處理；名稱分大小寫，`setProperty('--x', '')` 等於移除。cascade 忽略它們。`getComputedStyle(el).getPropertyValue('--x')` 在 S4 之前是空字串（Chrome 讀得到，WPT 的 `getComputedStyle-root-variables.html` 等失敗，記進 baseline）。V3 要在 `ComputedStyle` 放一張自訂 property 的表（繼承、`initial`），是 S4 的前一半；S4 的設計（表的共用、`var()` 的替換時機）還沒寫，現在做會先替 S4 決定。

### X. 同步的 style 更新和 D 的分工

| | X1 這份先做只到 style 的 hook | X2 hook 歸 D | X3 hook 獨立一份 RFC |
| --- | --- | --- | --- |
| `getComputedStyle` 什麼時候能做 | 不等 D | D 的 hook 之後 | hook 的 RFC 之後 |
| 誰設計 lifecycle 的改動 | 兩份各改一次（style 一次、layout 一次） | D | 第三份 |
| resolved value（`width` 等） | 還是要等 D | 一起有 | 等 D 的幾何 |
| 路線圖的順序 | `getComputedStyle`（原本的 1e，現在的第 2 段）可以提前到 M1 之前，但 M1 不需要它 | `getComputedStyle` 在 M2，D 在 M1，符合 | 多一份 RFC 要排 |

hook 要解決的問題（給 D 參考）：`FrameClient` 的實作要碰到 `StyleEngine`、`FrameView` 的 `linked_sheets` 和 dirty flag，這些現在在 `FrameView` 裡（`StyleEngine` 原本在 `RenderThread`，RFC 0009 的 2b-1a 搬進 `FrameView`），而 script 執行時 `FrameRequests` 只拿得到自己。可能的方向：把 document 的 lifecycle 狀態（Blink 的 `LocalFrameView` 和 per-document 的 `StyleEngine`）放進 `FrameRequests` 和 `FrameView` 共用的 `Rc<RefCell<…>>`，或讓 `FrameClient` 的 hook 只記下「需要同步更新」，由 renderer 在呼叫 engine 時事先交出 lifecycle 的借用。要注意：script 執行時 renderer 自己有沒有借著 `FrameView`（例如 parser 在 `FrameView` 裡呼叫 script），以及 hook 跑 style 時不能再執行 script。

### Y. camelCase 的屬性

| | Y1 prototype 上的 attribute | Y2 named interceptor |
| --- | --- | --- |
| 規範 | 一樣（camel-cased、webkit-cased、dashed attribute） | 不同：不在 prototype 上 |
| `in`、列舉 | 對 | query 和 enumerator 做得到 |
| 加 property 時 | 宣告表跟著變（從 `style` 的清單用巨集產生） | 不用改 bindings |
| WIT | 要排除幾百個成員 | 本來就不在 WIT |
| 新的 bindings 形式 | 「從清單產生成員」 | 沒有名字的 named setter（`dataset` 也要） |
| V8 的效能 | accessor，快 | interceptor，每次存取經過 callback（`docs/limitations.md` 的集合讀取的數字是同一類的代價） |

Y2 的效能代價：`el.style.display = 'none'` 每次經過 interceptor 和名稱的 `match`。Chrome 也是 interceptor（加上 `CssPropertyInfo` 的快取），量到慢時可以在 `dom_js` 裡快取名稱（每個 isolate 一份，不是 process 的 static）。

### Z. GC 型別

Z1：兩個型別各自的欄位（inline 只有元素；computed 有元素和 pseudo-element），`ObjectKind` 兩種、都列在 `CSSStyleProperties` 的方括號裡（方括號是一個清單，宣告表的 `own_kinds`；現在還沒有 interface 列兩種，1b 時確認前端照樣處理）。第 3 段的 `CSSStyleRule.style` 是第三種，edge 指向 rule。Z2：一個型別的 owner 是「元素或 rule」的 enum，`dom` 多一個「指向好幾種物件之一」的 edge 分支（`crates/dom/README.md` 的「怎麼修改」第 1 步提到的那一種），反而比多一個型別麻煩。

### W. JS 和 wasm

W1 的工作：宣告表的成員自動成為 `kun:dom` 的函式（`html-element` 的 `style`、`css-style-declaration` 的 resource、`window` 的 `get-computed-style`）；golden file 的 diff 只新增，不升版；`ci.py guests` 重新產生 Lua 的 binding、重編 guest；`dom_wasm` 一個 guest 測試。沒有 `any`、callback、`async method`，不碰 wasm 的特殊路徑。W2 省下的只有這些，代價是 wasm 文件的 guest 只能自己拼 `style` 字串。

### A. 第 3 段的範圍

| | 做什麼 | Speedometer | WPT | 代價 |
| --- | --- | --- | --- | --- |
| A1 | 建構子、`replace`、`replaceSync`、兩個 `adoptedStyleSheets` | 兩個 suite 都夠（Lit 的 feature detection 要 `Document` 的和 `replace`） | `CSSStyleSheet-constructable*` 大多讀 `cssRules`，收得到的很少 | 最小 |
| A2 | A1 加上 `cssRules`、`CSSRuleList`、`CSSStyleRule`（`selectorText`、`style`）、`insertRule`、`deleteRule` | 同 A1 | constructable 和 adopted 的大部分、`CSSRuleList.html`、`CSSStyleRule*.html`、`insertRule-*` 裡不靠 at-rule 的 | 規則的 wrapper 和 identity、第三種 declaration |
| A3 | A2 加上 `document.styleSheets`、`<style>.sheet`、`HTMLStyleElement` | 同 A1 | 再多 `StyleSheetList*`、`insertRule-from-script` 這類 | `<style>` 的原文和 CSSOM 的修改互相同步：`StyleEngine` 現在照原文比對，CSSOM 改了 `<style>` 的 sheet 之後原文不變，要改成 Blink 那樣以 sheet 為準 |

### B. `adoptedStyleSheets` 的型別

B1 的工作：宣告表的 `ObservableArray<T>` 和它的 set / delete hook；`dom_js` 的 Proxy 和八個 trap（WebIDL 的「observable array exotic object」），getter 回傳的 Proxy 存在 wrapper 的 private property；WIT 的 get 和 set；`dom` 的清單。B2 的工作：一個可以寫的 `FrozenArray` attribute（宣告表已經接受），`dom` 的清單一樣；JS 讀到的是凍結的陣列，`push` 丟 `TypeError`。B2 之後換成 B1 時，JS 的行為和 WPT 的 baseline 都要再改一次。TodoMVC-WebComponents 只在 `window.extraTodoItemCssToAdopt` 有值時 `push`，只有 Complex-DOM 的版本設它（`../third_party/speedometer/resources/todomvc/vanilla-examples/javascript-web-components/src/components/todo-item/todo-item.component.js`）。

### C. 解析好的規則放在哪

2026-10-12 先決定了 C2，3b 開始時發現它不成立，重新比較（第 18 條）。
- C1：`dom` 的 `CssStyleSheet` 只存整份原文，`StyleEngine` 照 `Arc<str>` 的指標快取解析的結果。`cssRules` 每次讀都要從原文解析（或在 `dom_api` 另外快取一份），`insertRule`、規則的 `style` 改了都要整份重新序列化再寫回原文；規則的 wrapper 也沒有自己的內容可以帶著（拿掉之後讀不到）。
- C1'：以規則為單位存原文（`Arc<str>` 的清單，`CssStyleRule` 存自己那條），`StyleEngine` 照指標快取每條規則的解析結果，編輯只重新序列化那一條。`Arc<str>` 已經是 `NoTrace`。CSSOM 讀規則時要解析那一條；`<style>` 的 sheet（A3）和 CSSOM 要共用內容時，原文和解析結果分在兩邊。
- C2：`style` 的 `StyleSheetContents`，`dom` 存 `Arc<dyn Any + Send + Sync>`，`style` downcast。不成立：`crates/dom/soundness.md` 的「`NoTrace` impl」要遞迴地看每個欄位型別，明確排除「`Box<dyn Any>` 這類抹掉型別的欄位」，替別的 crate 的型別寫的 impl 要審查那個 crate 的整棵型別樹。`Send + Sync + 'static` 的 assertion 也擋不住：`Edge<T>` 的 `Send` 跟著 `T`（`MemberSlot` 是 `unsafe impl Sync`，`PhantomData<T>` 帶著 `T` 的 `Send`），2026-10-12 用編譯期的 assertion 在 `dom` 裡試過，`Edge<Node>` 不是 `Send`（`Node` 經過 `Window`、`Event` 有 `Rc<str>`），`Edge<Blob>` 卻是 `Send + Sync`；`v8::Global` 也是 `Send`（abort steps 因此放在 heap 外）。
- C2'：GC 物件只存編號，`Arc<StyleSheetContents>` 放在 heap 外的表（和 abort steps 的 `DomTasks` 一樣，表裡的東西不受 `NoTrace` 約束）。stylesheet 被回收時表裡的項目要清掉：cppgc 的 finalizer 不能碰 heap 外的狀態，所以要另外用 weak set 記下活著的 stylesheet，在建立新的、或重算 style 時清掉死掉的。新的生命週期程式，要論證和測試不漏、不早清。
- C3：`style` 裡不依賴 `dom` 的模組搬到 `dom` 下面的新 crate，`dom` 拿著具體的型別。2026-10-12 量的規模：

| 模組 | 行數 | 依賴 `dom` | 去向 |
| --- | --- | --- | --- |
| `values`、`background`、`font`、`grid`、`effects`、`shadow`、`svg` | 3,739 | 無 | `css` |
| `properties`、`serialize`、`property_set` | 3,684 | 無 | `css` |
| `stylesheet` | 360 | 無 | `css` |
| `selector` 的解析那一半（`Impl`、`CssIdent`、`CssString`、`PseudoClass`、`SelectorParser`） | 約 220 | 只有 `PseudoClass::state()` 用 `ElementState`，那個對應留在 `style` | `css` |
| `computed`、`canvas`、`css_namespace` | 934 | 無 | `css`（大量用到上面的型別） |
| `lib`（`StyleEngine`、cascade、`SelectorQuery`）、`selector` 的 matching（`StyleElement`）、`hints`、`svg_use`、`svg_resources`、`intersection_observer`、`media` | 約 2,800 | 有（`media` 除外） | 留在 `style` |

  留下的模組 import 的大約 20 個名字（`properties` 的 `apply`、`cascade_start`、`finish`、`LonghandId`、`Declaration`，`values` 的 `ComputeContext`、`LengthUnit`，`CssPropertyValueSet`、`StyleRule` 和 `Stylesheet` 的欄位）改成 `pub`；`style` 照舊 `pub use` 公開的 API，`dom_api`、`renderer` 不用改。`svgtypes` 只在解析時用、結果轉成 kun 的型別，不進型別樹。量到的編譯時間（Windows 11）：`touch` `crates/css/src/properties.rs` 之後重編到 `shell` 16 秒，`touch` `crates/dom/src/lib.rs` 8 秒；C3 之後改 CSS 兩者都要。
- 改用 Stylo（Servo 的 style system）：Servo 的 DOM 也拿著 `style` crate 的 `Arc<Locked<Stylesheet>>`，是 C3 的形狀；但 Stylo 的選擇器和宣告裡的名稱是 `string_cache` 的 `Atom`，動態的 Atom 在整個 process 共用的 set 裡，最後一個參考 drop 時要上鎖移除，`soundness.md` 不准 finalizer 上鎖，renderer 這一側也不准 process 共用的可變 `static`。而且要換掉 `computed_style`、layout 和 paint 讀 style 的部分，CSSOM 的序列化變成 Firefox 的寫法。`docs/architecture.md` 的「Crate 選型」不用 Stylo 的決定不變。

### D. 收集和 invalidation

D1：`StyleEngine` 多讀 document 的 adopted 清單（`dom` 的 rare data），和原文的清單一起比；改了整份重算，和 `<style>` 一樣。D2：每個 tree scope 一份收集（Blink 的 `TreeScopeStyleSheetCollection`），cascade 照 tree scope 分 rule set，一份 stylesheet 改了只重算 adopt 它的 tree scope，再照規則的選擇器的 invalidation set 縮小範圍。D2 的 tree scope 要 style 能進 shadow tree（K-2），invalidation set 是 `docs/limitations.md` 的「Invalidation」那條的整體改進。

## 序列化

逐項的注意事項（Chrome 的行為以 WPT 的 `-valid`、`-computed` 和實際在 Chrome 上的結果為準，1a 和第 2 段時逐項確認）：

| 值 | specified（`style.getPropertyValue`、`cssText`） | computed（`getComputedStyle`） | kun 現在缺的 |
| --- | --- | --- | --- |
| 長度 | 保留單位（`1in`、`2em`），數字照 Chrome 的 6 位有效數字；`0` 寫成 `0px` | px | 1a 起 `Length` 保留單位 |
| 百分比 | `50%` | 多半保留 `50%`；layout-dependent 的是 px（resolved value） | 要 D 的幾何 |
| 顏色 | 關鍵字照原樣（小寫），hex 和 `rgb()` 寫成 `rgb(r, g, b)` / `rgba(r, g, b, a)`；`currentcolor`；`background: none` 省略的顏色讀回 `rgba(0, 0, 0, 0)` | `rgb()` / `rgba()`，`currentcolor` 換成 `color` 的值 | 1a 起保留關鍵字 |
| 關鍵字 | 小寫 | 小寫；`display` 是 blockify 之後的 | — |
| `font-family` | 逗號加一個空白；有空白的名稱加雙引號 | 同 specified | — |
| `font-weight` | `bold` 照原樣 | 數字（`700`） | 1a 起保留 `normal` / `bold` |
| `line-height` | `normal`、數字、長度 | `normal` 照原樣；長度是 px | — |
| `transform` | 函式照原樣 | `matrix(...)` 或 `matrix3d(...)`、`none` | — |
| `opacity` | 數字，百分比換成數字（`50%` 是 `0.5`，WPT 的 `opacity-valid.html`），範圍外的照原樣 | 夾在 0 到 1 | 1a 起計算時才夾住 |
| grid 的 track list | 照寫的（`repeat()`、名稱） | grid container 是 used 的 track 大小（layout-dependent） | 要 D 的幾何和 grid 的 track |
| `overflow` | `hidden`、`clip` 照原樣，`overlay` 是 `auto` | 一軸 `visible` 一軸不是時，`visible` 算成 `auto`（kun 也是） | `clip` 算成 `hidden`（1a 起 specified 保留 `clip`） |
| 自訂 property | 原文 | S4 之前沒有 | 1a 起保存 |

shorthand 的合回（`getPropertyValue('margin')`、`cssText`）照 `StylePropertySerializer`：所有 longhand 都有、`!important` 一致、沒有 CSS-wide 關鍵字混用時才合回，`margin` 等四邊的照「一到四個值」縮短；`background` 這類 kun 只支援一部分 longhand 的 shorthand，只用 kun 有的 longhand 合回（`background: red` 讀回 `red`，和 Chrome 一樣）。

## Speedometer 用到的

RFC 0012 的盤點（`rfcs/0012-speedometer/details.md` 的「每項功能缺的成員」）記下的 CSSOM 成員：`CSSStyleDeclaration.cssText`（讀寫）、`getPropertyValue`、`setProperty`、`HTMLElement.style`、`SVGElement.style`、`Window.getComputedStyle`；constructable stylesheet 是 `CSSStyleSheet` 的建構子、`replaceSync`、`ShadowRoot.adoptedStyleSheets` 的 setter（2 個 suite）。

第 3 段開始前讀 suite 的原始碼（2026-10-12，`../third_party/speedometer` 的 `release/3.1`）：
- **TodoMVC-WebComponents**：每個元件的 style 是一個模組，`new CSSStyleSheet()` 再 `replaceSync`（`dist/styles/*.constructable.js`），元件的建構子 `this.shadow.adoptedStyleSheets = [...]`；todo-item 在 `window.extraTodoItemCssToAdopt` 有值時再 `new CSSStyleSheet()`、`push` 進去，只有 Complex-DOM 的版本設它。
- **TodoMVC-Lit-Complex-DOM**：Lit 的 `supportsAdoptingStyleSheets` 是 `ShadowRoot && "adoptedStyleSheets" in Document.prototype && "replace" in CSSStyleSheet.prototype`。成立時每個 `CSSResult` 第一次用到就 `new CSSStyleSheet()` 再 `replaceSync`，整份指定給 `shadowRoot.adoptedStyleSheets`；不成立就在 shadow root 裡插 `<style>`。所以缺了 `Document` 的 `adoptedStyleSheets` 或 `replace`，它照樣跑得完，但走的是和 Chrome 不同的路。

盤點的限制：它包住 prototype 上的方法和 accessor，而 Chrome 的 camelCase 屬性是 interceptor，所以 `el.style.display = …` 這類用法記不到（TodoMVC-ES5 的 `style.display` 是讀原始碼找到的，`rfcs/0012-speedometer/details.md` 的「kun 跑不起來的地方」）；`getComputedStyle` 之後讀哪些 property 也沒記。重跑盤點（RFC 0012「分階段」的 1b）時，建議在 `CSSStyleDeclaration` 的 instance 上加 Proxy 記下名稱，第 2 段開始前用它決定 resolved value 先做哪些。

## WPT

都在 `../third_party/wpt` 底下（不改它，只在 `tests/wpt/` 的清單和 baseline 加）。

- **`../third_party/wpt/css/cssom/`**：
  - inline style（1b 到 1d）：`cssstyledeclaration-csstext*.html`、`cssstyledeclaration-mutability.html`、`cssstyledeclaration-mutationrecord-00*.html`、`cssstyledeclaration-properties.html`、`cssstyledeclaration-setter-*.html`、`cssstyledeclaration-custom-properties.html`、`cssstyledeclaration-removeProperty-all.html`、`CSSStyleDeclaration-indexed-properties.html`、`CSSStyleDeclaration-iterator.html`、`css-style-attr-decl-block.html`、`css-style-attribute-modifications.html`、`css-style-declaration-modifications.html`、`style-set-property.html`、`style-set-cssText.html`、`style-cssText-mutations.html`、`style-set-custom-property*.html`、`setproperty-null-undefined.html`、`cssom-setProperty-shorthand.html`、`serialization-CSSDeclaration-with-important.html`、`shorthand-serialization.html`、`serialize-values.html`、`serialize-custom-props.html`、`MutationObserver-style.html`、`style-attr-update-across-documents.html`、`inline-style-001.html`。
  - `getComputedStyle`（第 2 段）：`getComputedStyle-basic.html`、`getComputedStyle-live.html`、`getComputedStyle-mutations.html`、`getComputedStyle-detached-subtree.html`、`getComputedStyle-display-none-00*.html`、`getComputedStyle-property-order.html`、`getComputedStyle-line-height.html`、`getComputedStyle-resolved-colors.html`、`getComputedStyle-margins-roundtrip.html`、`getComputedStyle-dynamic-subdoc.html`（子 frame；RFC 0009 的 2b-1 已經有了）、`computed-style-00*.html`、`computed-style-set-property.html`。
  - 第 3 段：`CSSStyleSheet-constructable*.html`（`-baseURL`、`-cssRules`、`-duplicate`、`-invalidation`、`-replace-cssRules`、`-replace-on-regular-sheet`、`-replace-thenable`、`-insertRule-base-uri` 等）、`CSSStyleSheet-replace-detach-old-rules.html`、`CSSStyleSheet-replace-parse-errors.html`、`CSSStyleSheet-modify-after-removal.html`、`CSSStyleSheet-template-adoption.html`、`adoptedstylesheets-cascade-order.html`、`adoptedstylesheets-modify-array-and-sheet.html`、`adoptedstylesheets-observablearray.html`、`adoptedstylesheets-invalid-assignment-crash.html`、`CSSRuleList.html`、`CSSStyleRule.html`、`CSSStyleRule-selectorText-setter.html`、`CSSStyleRule-set-selectorText.html`、`CSSRule-type.html`、`insertRule-no-index.html`、`insertRule-syntax-error-01.html`、`selectorText-modification-restyle-*.html`。用到 `document.styleSheets`、`<style>.sheet`（`StyleSheetList-constructable*.html`、`CSSStyleSheet.html`、`insertRule-from-script.html` 等）或 at-rule（`CSSGroupingRule-*`、`insertRule-font-face.html` 等）的不收，`.tentative` 的也不收；要 style 進 shadow tree 的（`CSSStyleSheet-constructable.html` 的一部分）照實記失敗，等 K-2。開始 3d 時照實際跑的結果調整。
  - 不加：`position`、`float`、pseudo-element、`@page`、`@font-face` 為主的（`getComputedStyle-insets-*`、`getComputedStyle-pseudo*`、`property-accessors.html` 要 `document.styleSheets`）等到各自的 RFC。
- **`css/*/parsing/`**（1d，`-computed` 在第 2 段）：支援的 property 的 `-valid`、`-invalid`、`-computed`，例如 `../third_party/wpt/css/css-flexbox/parsing/`、`css-grid/parsing/`、`css-box/parsing/`、`css-sizing/parsing/`、`css-backgrounds/parsing/` 的 `background-color-*`。它們用 `../third_party/wpt/css/support/parsing-testcommon.js`（`element.style[property]`，dashed 的名稱）和 `computed-testcommon.js`（`property in getComputedStyle(el)`）。檔案裡一部分的值 kun 不支援，baseline 照實記下失敗。
- **`../third_party/wpt/css/css-variables/`**：只測 CSSOM 的（`variable-reference-cssom.html` 等）；用 `var()` 的等 S4。
- **`../third_party/wpt/css/cssom-view/`** 是 D 的。

## 參考資料

- CSSOM：https://drafts.csswg.org/cssom/（`CSSStyleDeclaration`、`CSSStyleProperties`、serialization、`getComputedStyle` 和 resolved value）
- CSS Custom Properties：https://drafts.csswg.org/css-variables/
- Constructable stylesheet：CSSOM 的 `CSSStyleSheet` 建構子；`adoptedStyleSheets`（`DocumentOrShadowRoot`）
- WebIDL 的 `ObservableArray`：https://webidl.spec.whatwg.org/#idl-observable-array
- WPT 的 IDL：`../third_party/wpt/interfaces/cssom.idl`
- Chromium：`third_party/blink/renderer/core/css/css_style_declaration.h`、`abstract_property_set_css_style_declaration.h`、`inline_css_style_declaration.h`、`css_computed_style_declaration.h`、`css_property_value_set.h`、`style_property_serializer.h`；`core/dom/element.cc`（`StyleAttributeChanged`、`InlineStyleChanged`、`SynchronizeStyleAttributeInternal`、`SetInlineStyleProperty`）；`core/dom/document.cc`（`UpdateStyleAndLayoutTreeForElement`、`UpdateStyleAndLayoutForNode`）
- RFC 0012 的路線圖：`rfcs/0012-speedometer/roadmap.md`（C、D、K、S4）
