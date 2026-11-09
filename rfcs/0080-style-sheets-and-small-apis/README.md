# RFC 0080：文件的 stylesheet 的 CSSOM，和幾個小的 API

- 開始日期：2026-10-23
- 狀態：✅ 已完成：2026-10-24（驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`（Chrome 152 的探測、原本的做法、Speedometer 和 WPT 用到的地方、`RuleSet` 怎麼分和各段的量測）

## 摘要

照 CSSOM 和 Blink（`StyleSheetList`、`StyleElement`、`LinkStyle`、`CSSStyleSheet` 包著 `StyleSheetContents`）：`<style>` 和 `<link rel=stylesheet>` 有 `sheet`，`document.styleSheets` 和 `shadowRoot.styleSheets` 列出它們。script 透過它們改的規則（`insertRule`、`deleteRule`、規則的 `style`、`disabled`、`media`）會影響畫面。做法是讓 `dom` 持有每個 `<style>`、`<link>` 解析好的內容（`Arc<StyleSheetContents>`，和 constructed stylesheet 同一種），style engine 從 `dom` 讀，不再自己從文字重新解析。同一份 RFC 也補幾個 WPT 和網站常碰到的小缺口，各自照 Chrome 152：
- `DOMRectReadOnly.toJSON()`；
- `MouseEvent` 的 `layerX`、`layerY`；
- `focus()` 捲進畫面的方式；
- CSS 的 `round()`、`mod()`、`rem()` 等數學函式。

## 動機

**現況**：

- **文件的 stylesheet 沒有 CSSOM**：
  - 只有 constructed stylesheet（`new CSSStyleSheet()`、`adoptedStyleSheets`）有 CSSOM。
  - `document.styleSheets` 是 `undefined`，`<style>` 和 `<link>` 沒有 `sheet`。
  - style engine 每次從 `<style>` 的文字和 `<link>` 載入的文字解析出 `css::Stylesheet`，自己留著，script 碰不到。
- **小的缺口**：
  - `JSON.stringify(element.getBoundingClientRect())` 是 `{}`。
  - `event.layerX` 是 `undefined`。
  - `focus()` 一律捲到最近的邊。Chrome 是在元素不在畫面裡時把它捲到中間；`<input>`、`<textarea>` 和 editing host 捲的是 caret，不是整個元素。
  - `width: round(117px, 10px)` 這類宣告整條無效。

**問題**：

- **網站**：CSS-in-JS 函式庫在 production 模式用 `style.sheet.insertRule` 加規則（emotion、styled-components 的 speedy 模式）。emotion 在 `<style>` 沒有 `sheet` 時改到 `document.styleSheets` 裡找自己的 `<style>`，在 kun 上丟 `TypeError`。讀 `document.styleSheets[i].cssRules` 的程式也一樣。
- **WPT**：用 `document.styleSheets` 或 `.sheet` 的測試有 367 份，例如：
  - `css/cssom/` 的 `MediaList`、`CSSGroupingRule`；
  - `css/css-counter-styles/cssom/`、`css/css-conditional/`；
  - `html/semantics/document-metadata/`。
  
  它們在第一步就丟 `TypeError`。其他的缺口也各卡住一些測試：
  - 沒有 `round()`：例如 `css-scroll-anchoring/adjustment-followed-by-scrollBy.html` 的 div 因此沒有高度。
  - `focus()` 捲的方式不同：例如 `css-scroll-anchoring/contenteditable-near-cursor.tentative.html` 的每個捲動位置都多 8px。
- **Speedometer**：它的程式沒有用到 `layerX` 和 rect 的 `toJSON`（grep 過），用到 `document.styleSheets` 的只有 react-stockcharts 匯出圖的路徑，量測時不會走到。這幾個缺口是之前在 Speedometer 的頁面上和 Chrome 比對畫面時找到的：自己寫的探測頁面讀到它們。所以這份 RFC 主要是為了網站和 WPT，不期待 Speedometer 的分數改變。

**目標**：

1. `document.styleSheets`、`ShadowRoot.styleSheets`（`StyleSheetList`），以及 `<style>`（HTML 和 SVG）、`<link rel=stylesheet>` 的 `sheet`。行為照 Chrome 152：
   - 每次讀是同一個物件；
   - `ownerNode`、`href`、`title`、`media`、`disabled`；
   - 文字改了換一個新的 sheet，之前的改動消失；
   - 改 `media`、`title` 屬性還是同一個 sheet；
   - 離開 document 時 `sheet` 是 `null`；
   - 不收 adopted 的。
2. 透過 `sheet` 的改動（`insertRule`、`deleteRule`、規則的 `style` 和 selector、`disabled`、`media`）影響 cascade，失效的範圍和現在改 `<style>` 的文字一樣以規則為單位。
3. shadow tree 裡同樣文字的 `<style>` 照舊共用解析的結果；改其中一個的 `sheet` 不影響其他的（copy-on-write）。
4. CSSOM 的舊成員：`rules`、`addRule`、`removeRule`；`<link>` 的 `media`、`type`、`disabled` reflect 屬性。
5. title 照 Chrome：第一個有 `title` 的 sheet 決定 preferred set，`title` 不同的 sheet 不套用。alternate 的 `<link>` 照樣載入、在清單裡，但不套用。
6. `DOMRectReadOnly.toJSON()`：照 `[Default]` 的順序，`x`、`y`、`width`、`height`、`top`、`right`、`bottom`、`left`。
7. `MouseEvent.layerX`、`layerY`：照 Blink，相對於目標所在的 layer。
8. `focus()` 照 Blink 的 `CenterIfNeeded`：
   - 整個在畫面裡時不捲；
   - 部分在畫面裡時捲到最近的邊；
   - 不在畫面裡時捲到中間。
   
   `<input>`、`<textarea>` 和 editing host 捲的是 caret。
9. CSS 數學函式：css-values-4 的全部（`round()`、`mod()`、`rem()`、`abs()`、`sign()`、三角函數、`pow()`、`sqrt()`、`hypot()`、`log()`、`exp()`；「已決定的事項」第 4 條）。
10. 更新相關的 WPT 的預期結果，把它們的目錄加進清單。

**要解決的 `docs/limitations.md` 的限制**：

- 「Style」：
  - stylesheet 的 CSSOM 只有 constructed 的（沒有 `document.styleSheets`、`sheet`）；
  - `round()`、`mod()` 等數學函式沒有。
- 「Bindings 和 DOM API」：
  - `HTMLStyleElement` 沒有 `sheet`，`HTMLLinkElement` 的成員；
  - `DOMRectReadOnly` 沒有 `toJSON()`。
- 「事件」：`MouseEvent` 沒有 `layerX/Y`。
- 「事件」的 focus 那條：`focus()` 捲到最近的邊；部分在畫面裡的元素也捲，這句的說法不對，要改寫：Chrome 也捲，差別在 editing host。

**不在範圍內**：

- `@import`（`CSSImportRule`）：要載入、巢狀的 cascade、`load` 事件和循環的處理。另外寫 RFC（「已決定的事項」第 12 條）。
- shadow tree 裡的 `<link rel=stylesheet>` 的載入：照舊不載入。
- `<link>` 的 `href` 改變、重新插入時重新載入：每個 `<link>` 照舊只載入一次；`rel`、`href`、`disabled`、`type` 改了、移除和插入時用已經載入的文字重新做 sheet（Chrome 重新載入，`docs/limitations.md`）。
- `DOMPoint`、`DOMQuad`、`DOMMatrix`：kun 沒有這些 interface。
- 傳統的捲軸佔的寬度：kun 的捲軸不佔版面，Chrome 的佔。所以 `focus()` 捲到的位置可能差捲軸的寬度，這次不處理。
- `CSSStyleSheet.replace()` 和 `replaceSync()` 用在文件的 stylesheet 上：照 Chrome 丟 `NotAllowedError`（只是一個檢查，在範圍內）。

## 設計概要

```html
<style id="s" media="screen">p { color: green }</style>
<link rel="stylesheet" href="a.css">
<script>
  document.styleSheets.length          // 2（document 順序）
  s.sheet === document.styleSheets[0]  // true，每次讀同一個物件
  s.sheet.ownerNode === s              // true；href 是 null，<link> 的是它的網址
  s.sheet.insertRule("p { color: red }", 1);   // <p> 變紅，s.textContent 不變
  s.sheet.cssRules[1].style.color = "blue";    // <p> 變藍
  s.media = "print";                   // 同一個 sheet，改過的規則還在，media 變 print
  s.textContent += " ";                // 文字變了：新的 sheet，改過的規則消失
</script>
```

CSS-in-JS 的函式庫：

```js
const style = document.head.appendChild(document.createElement("style"));
style.sheet.insertRule(".css-1x2y { display: flex }", style.sheet.cssRules.length);
```

小的 API：

```js
JSON.stringify(el.getBoundingClientRect())  // {"x":…,"y":…,"width":…,"height":…,"top":…,"right":…,"bottom":…,"left":…}
el.addEventListener("click", e => e.layerX)  // 相對於目標所在的 layer（例如有定位的祖先）
button.focus()   // 不在畫面裡時捲到中間，部分在畫面裡時捲到最近的邊
```

```css
.box { width: round(up, 111px, 10px); margin-top: mod(17px, 5px); }   /* 120px、2px */
```

## 詳細設計

### 1. 文件的 stylesheet 由 `dom` 持有

「已決定的事項」第 5 到 9、11 條。

**原本的做法**：

- `style` 的 `StyleEngine::collect_document_sheets` 每次重算 style 時走過 document，收 `<style>` 的文字和 `<link>` 載入的文字（renderer 的 `FrameView::linked_sheets`，以 link 的 `NodeId` 為鍵）。
- 文字和上次一樣的沿用，否則解析成 `css::Stylesheet`。
- 所有 sheet 的規則放進一個 `RuleSet`，規則有變的照規則做失效。
- shadow tree 的 `<style>` 解析成 `Arc<StyleSheetContents>`，同樣的文字共用一份（`style_texts`）。

**改成**（對應 Blink 的 `StyleElement`、`LinkStyle` 持有 `CSSStyleSheet`，`CSSStyleSheet` 包著可能共用的 `StyleSheetContents`；`dom` 的 `style_element.rs`）：

- 每個 `<style>`、`<link>`（和 document、shadow root）在 `dom` 的 rare data 裡有一個 GC 物件（`StyleSheetData`）。只有這幾種節點有，其他有 rare data 的元素只多一個指標：每個有 `classList` 的元素都有 rare data，多兩個欄位的第一版讓 `ChangeStyleCSSVariableRecalc` 慢了 4%。它有：
  - 解析好的 `Arc<StyleSheetContents>`，和 constructed stylesheet 一樣；
  - sheet 的 media（`media` 屬性的，或 script 經過 sheet 設的）；
  - `<link>` 載入的文字、它來的網址和 origin-clean（`set_linked_style_sheet`，`FrameView` 交給它）；
  - 「sheet 做過幾次」的編號；
  - script 的 `sheet` 物件、document 或 shadow root 的 `styleSheets` 的 edge。
- 這兩種元素建立時就配置 rare data：解析發生在 read scope 裡，那時不能配置。
- `DomReadScope::style_sheet_contents` 第一次要的時候才解析，之後同一份，`url()` 照當時的 base URL（和 Chrome 一樣）；sheet 重新做的時候（`enable_style_sheet`：文字、`type` 改了，插入、移除；`<link>` 再載入）丟掉。
- 同樣的文字和 base URL 共用一份（`DomState::style_sheet_texts`，`Weak`，表數到上次留下的兩倍時拿掉沒人用的）。
- style engine 的 `collect_document_sheets` 和 shadow tree 的收集都從 `dom` 拿內容，比 `Arc` 的指標和 `media`（指標不同時比內容：重新插入、文字一樣的 `<style>` 的 media 是另一個 `Arc`）。engine 自己的文字快取（`style_texts`）、`LinkedStyleSheets` 和文件的 sheet 的 `css::Stylesheet` 拿掉。
- **document 的 `RuleSet`**（第 11 條）：規則 512 條以上的 sheet 各自一份，快取在它的 `StyleSheetContents` 上；夾在它們之間、連續的小 sheet 合成一份，其中一個變了只重建那一份。每個 set 的 invalidation 特徵和 dependency 也算一次、存著（`SetFeaturesCache`，Chromium 的每個 `RuleSet` 帶著 `RuleFeatureSet`），tree scope 合併各 set 的。規則的失效照舊。
- **CSSOM 的改動**：`insert_css_rule` 這些照舊產生新的內容（`StyleSheetContents` 不就地修改），物件是元素的 sheet 的期間，`css_style_sheet_changed` 把它的內容、media 和 `disabled` 交回元素，標記它的 tree scope 的 stylesheet 變了。共用的內容因此自然是 copy-on-write。`media`、`title` 屬性改了交給物件（改 `title` 照 Chrome 不標記 tree scope）。
- **文字改了、元素離開 document**：丟掉內容，「sheet 做過幾次」加一。這發生在不能寫 heap 的地方，所以不清 edge：舊的物件記著它是第幾次做的，不同了就不是元素的 sheet（`ownerNode` 是 `null`、改動留在它自己）。下次需要時從文字重新解析，得到新的 sheet。
- **什麼時候解析**：第一次需要時（「已決定的事項」第 6 條）。kun 等到重算 style 或 script 讀 `sheet` 才解析，連續改好幾次文字只解析一次；Blink 每次改文字就解析。

### 2. `StyleSheetList` 和 `sheet`（`dom_api`、IDL）

- `DocumentOrShadowRoot.styleSheets`：一個 live 的清單（每次讀同一個物件），依 tree order 列出 tree scope 裡：
  - 有 sheet 的 `<style>`：CSS 的 `type`，在 document 裡；
  - 載入了的 `<link>`。
  
  不收 adopted 的（Blink 的 `TreeScope::StyleSheets`）。
- `LinkStyle.sheet`（`HTMLStyleElement`、`HTMLLinkElement`、`SVGStyleElement`）。
- `CSSStyleSheet` 的 `ownerNode`、`href`、`title` 原本回傳 `null`，改成照 sheet 的來源（`href` 是 `<link>` 要求的網址）；`parentStyleSheet` 照舊是 `null`（沒有 `@import`）。
- `media` 從元素的屬性初始化；之後改屬性時更新 sheet 的 `media`，改過的規則留著。
- `disabled` 和 `<style>.disabled` 是同一個旗標（`<link>` 的 `disabled` 是 reflect 的屬性，改了重新做 sheet）。
- 文件的 sheet 上呼叫 `replace()`、`replaceSync()` 丟 `NotAllowedError`。
- `rules`、`addRule`、`removeRule` 加在 `CSSStyleSheet` 上，constructed stylesheet 也有。
- `<link>` 的 `media`、`type`、`disabled` 是 reflect 的屬性。
- **title 和 alternate**：照 Chrome 152 的探測（`details.md`）。
  - 第一個有 `title`、不是 alternate 的 sheet 的 title 是 preferred set；`title` 不同的 sheet 在清單裡，但不套用。
  - alternate 的 `<link>` 照樣載入，在清單裡，但不套用。
  - Chrome 的這兩種 sheet 的 `disabled` 都是 `false`，照它。

### 3. 小的 API

- **`toJSON`**：`DOMRectReadOnly` 的 `[Default] object toJSON()`。回傳照屬性順序的 `Value::Record`：`Value::Dictionary` 會照名稱排序，不能用。
- **`layerX`、`layerY`**：照 Blink 的 `MouseEvent::ComputeRelativePosition`，page 座標減掉目標所在的 layer 的位置，沒有 layer 的是 page 座標。哪些盒子算 layer，照 Chrome 152 的探測在 kun 的盒子上判斷：有定位的、stacking context（transform、`opacity < 1`、filter、`contain: paint`、flex 項目的 `z-index` 等）；overflow 會裁切的（`hidden`、捲動容器）不算；root element 有定位或是 stacking context 時也算，都沒有時是 page 座標（第 2 條）。kun 的 layout 是 `f32`，差捨去之前先對齊 Blink 的 1/64 px。值在每次讀的時候照當時的 layout 算，和 kun 的 `offsetX` 一樣（Blink 第一次讀時算好、之後沿用）。
- **`focus()` 捲進畫面**：`dom_api` 的 `focus` 改用 `CenterIfNeeded`：
  - 整個看得到不捲；
  - 部分看得到捲到最近的邊；
  - 看不到捲到中間。
  
  比元素還小的視窗照 Blink：視窗被元素整個蓋住時算看得到。renderer 的 `scroll.rs` 的對齊多一種。`<input>`、`<textarea>` 和 editing host 捲的是 focus 之後的 caret 所在的位置，不是元素的盒子（Blink 的 `FrameSelection::RevealSelection`；第 3 條）：renderer 多一個把 caret 的位置捲進畫面的呼叫，caret 的位置用畫 caret 的 `selection::caret`。
- **數學函式**：`css` 的 `calc.rs` 的樹多幾種節點（範圍見「已決定的事項」第 4 條）：
  - `Function`：其他的數學函式，記著參數的 kind（三角函數的 number 是弧度、角度是度）。參數同一種 kind，照 css-values-4 的 type checking：三角函數收 number 或角度，反三角函數和指數函數收 number，其他的任何一種；`round()` 只有 number 能省略步長。
  - `Multiply`、`Divide`：乘上、除以只有元素算得出來的 number（`10px * sign(1em)`），乘積的葉節點合成一個、放最前面。
  - `Negate`：加總裡減掉的非葉節點（css-values-4 的 Negate），照 Chrome 寫在 ` - ` 之後；加上一個負的乘積寫成 ` + (-2 * …)`。
  - 參數都是標準單位的葉節點（px、deg、s、dppx、number）時在解析時算出來；百分比和相對長度照 Chrome 留著函式（`round(7em, 2em)`），在元素上算。計算用 f64、結果轉回 f32；0、無限大、NaN 和正負零照 css-values-4（`round()` 到 0 的正負號、`mod()` 帶除數的正負號，`min()`、`max()` 和比較時分辨 `-0`）。
  - 在解析時就算出來的 number、角度、時間（`parse_absolute`）裡要元素才算得出來的無效；只收百分比的地方收只有百分比的函式（`round(10%, 3%)`）。
  - 混合長度和百分比的照現在的限制不解析：`round(33.3%, 10px)` 在 Chrome 有效，在 kun 照舊無效。
  - serialize 照 Chrome：最外層的函式寫成它自己，加總裡的乘積加括號，乘積裡的加總加括號，`nearest` 省略。
- **`gfx` 的 `Transform::then`**：`scale()` 拿到很大的數（`log(0)` 是 -infinity，夾成最大的 f32）時，繞著 transform-origin 的乘積在 f32 裡溢位成無限大，下一次相乘變成 NaN（debug build 的 pre-paint 檢查因此 panic）。結果不是有限值時改用 f64 算、夾回 f32 的範圍（Blink 的矩陣是 double）；一般的乘積和原本一樣。

### 測試

- `dom`：`tests/it/style_elements.rs`，元素的 sheet 跟著文字、屬性、插入、移除改變，第一次要時才解析、共用和清快取，script 的物件和元素的同一性、經過物件的改動、`styleSheets` 的內容和不再走 tree scope（`dom::testing::style_sheets_parsed`、`style_sheet_list_walks`）。
- `style`：`tests/it/invalidation.rs`（收集 sheet、`<link>` 載入之後跟著屬性、經過 sheet 的改動的失效範圍、preferred set、`RuleSet` 的分組和每個 set 的特徵，`rules_added_for_testing`、`sets_looked_through_for_testing`），`stylesheet.rs`（改動只解析新的那條的 `url()`），`calc.rs`（數學函式的解析、計算、serialize、無效的寫法）。
- `dom_js`：`tests/it/style_sheets.rs`、`geometry.rs`（`toJSON`），預期值是 Chrome 152 的（`details.md` 的表）。
- `shell`：`tests/it/style_sheets.rs`（script 改的 sheet 畫出來、跨 origin 和 `crossorigin`、alternate 不擋 script），`geometry.rs`（`layerX`），`focus.rs`（`focus()` 的捲動和 caret）；`renderer` 的 `scroll.rs` 的單元測試（對齊）。
- 情境：`tests/scenarios/css-in-js.txt`（`<style>` 加上 `insertRule`）。
- WPT：`css/cssom/` 整個目錄、`html/semantics/document-metadata/` 的 26 份、`css/css-values/` 的數學函式、`uievents/mouse/layer-coords-transform.html` 加進清單；之前因為這些失敗的重跑、更新預期結果和註解。
- mutation test：`mutants-1.toml` 到 `mutants-4.toml`。

### 分階段

1. **小的 API**：`toJSON`、`layerX`/`layerY`、`focus()` 的捲動。
2. **CSS 數學函式**。
3. **文件的 stylesheet 由 `dom` 持有**：`<style>` 和 `<link>` 的內容、style engine 改從 `dom` 讀；沒有新的 API，畫面不變，量測 style 的成本。
4. **CSSOM**：`StyleSheetList`、`sheet`、改動影響 cascade、title 和 alternate、舊的成員和 `<link>` 的屬性；WPT 的預期結果。

## 缺點

- style engine 拿 sheet 的方式是每個頁面都走的路。比較的東西從文字換成指標，但 `<link>` 的文字搬進 `dom`、rare data 多一個指標，document 的 cascade 分成幾個 set。`blink-perf` 的 CSS 51 頁和改之前的幾何平均是 0.999，但 `ChangeStyleCSSVariableRecalc` 慢 4.1%、`ClassInvalidation` 慢 5.3%：指令數沒有變多、同樣的事另外量兩邊一樣，看作編譯出來的差別（`details.md` 的「第 4 段的量測」）。
- CSSOM 的改動每次產生新的內容（複製規則的指標，O(規則數)）。一個 `<style>` 裡用 `insertRule` 加幾千條規則是 O(n²) 的指標複製，Blink 每次插入也要搬 vector，同一個數量級。那個 sheet 的 `RuleSet` 也每次重建（20000 條約 3 ms，「之後的可能」）。
- `dom` 多了 GC 物件和 rare data 的欄位：只有 script 讀了 `sheet` 的元素才有 CSSOM 物件。
- alternate 的 `<link>` 多一次載入（和 Chrome 一樣）。

## 理由和其他做法

- **sheet 由 `dom` 持有**（選這個）：script 改的和 engine 用的是同一份內容，不用對照兩份。`sheet` 的同一性（文字改了才換新的）自然成立，shadow tree 的共用也自然變成 copy-on-write。constructed stylesheet 已經是這樣做的。
- **engine 照舊從文字解析，script 改過的 sheet 另外疊一份**：程式改得少，但同一份 stylesheet 有兩個來源，要記得「改過的蓋過文字、文字改了才丟掉」。`sheet` 的物件要自己再解析一次文字，`<link>` 的文字也還是要交給 `dom` 才能讀。不選。
- **每次讀 `sheet` 從 engine 的結果包一個新的物件**：同一性不對（Chrome 每次是同一個），改動也回不到 engine。不選。
- **小的 API 和數學函式各自不寫 RFC 直接改**：它們各自都小，但新的行為要有 mutation test 和記錄的地方；放在同一份 RFC 的前兩段（「已決定的事項」第 1 條）。

## 先例

- **Chromium**：
  - `StyleElement::ProcessStyleSheet` 在 `<style>` 的文字改變時建立新的 `CSSStyleSheet`（`StyleEngine::CreateSheet`）。同樣的文字共用 `StyleSheetContents`（`text_to_sheet_cache_`），script 要改時 `CSSStyleSheet::WillMutateRules` 先複製。
  - `LinkStyle` 在載入完成時建立 `CSSStyleSheet`。
  - `TreeScope::StyleSheets` 回傳 `StyleSheetList`，由 `TreeScopeStyleSheetCollection` 依 tree order 收集。
  - `StyleSheetCollection` 照 preferred stylesheet set（第一個有 title 的）決定哪些套用。
  - `DOMRectReadOnly::toJSONForBinding`；`MouseEvent::ComputeRelativePosition` 的 layer 座標。
  - `Element::focus` 用 `ScrollAlignment::CenterIfNeeded`。editing host 的 focus 經過 `FrameSelection` 把 caret 捲進畫面。
  - `CSSMathExpressionNode` 的 `round`、`mod`、`rem`（`CSSMathOperator::kRoundNearest` 等）。
- **規範**：
  - CSSOM 的「`StyleSheetList`」「`LinkStyle`」「create a CSS style sheet」、preferred CSS style sheet set；
  - Geometry Interfaces 的 `toJSON`；
  - CSSOM View 的 `layerX`（沒有規範，Blink 的註解說它「poorly defined」）；
  - HTML 的 focusing steps；
  - css-values-4 的 stepped value functions。

## 已決定的事項

2026-10-23 確認：

1. **範圍**：一份 RFC 分四段，小的 API 和數學函式在前兩段，文件的 stylesheet 的 CSSOM 在後兩段。不選把 CSSOM 單獨寫一份 RFC、小的 API 各自直接改：那樣小的 API 的 mutation test 和檢查沒有地方記錄。

2026-10-23 確認第 1 段的事（使用者：「都照建議」）：

2. **`layerX`、`layerY` 照 Blink 的 layer**：相對於目標往上第一個算 layer 的盒子（有定位的、stacking context，root element 也算；都沒有時是 page 座標），用 kun 的 computed style 判斷。值在每次讀時照當時的 layout 算，和 kun 的 `offsetX` 一樣（Blink 第一次讀時算好）。不選的做法：只看有定位的祖先（簡單，但 transform、`opacity` 的情況和 Chrome 不同）。
   - 2026-10-23 實作時改：確認時的條件照 Blink 的 `LayerTypeRequired` 包括 overflow 會裁切的盒子，但 Chrome 152 的探測裡 `overflow: hidden` 和捲動容器裡的目標的 `layerX` 是 page 座標（`details.md`），所以照 Chrome，不算它們。
3. **`focus()` 照 Blink 的 `CenterIfNeeded`，`<input>`、`<textarea>` 和 editing host 捲 caret**：整個看得到不捲，部分看得到捲到最近的邊，看不到捲到中間；那三種捲 focus 之後的 caret 所在的位置（Chrome 152 的探測，`details.md`）。不選的做法：一律捲元素的盒子（長的 editor 和 textarea 捲到的位置和 Chrome 不同，WPT 的 `contenteditable-near-cursor` 照舊失敗）。
   - 2026-10-23 實作時加：editing host 拿到 focus 時 caret 放的位置原本是「第一個葉節點」，host 的開頭是收起來的空白時那裡畫不出 caret、也捲不到（`contenteditable-near-cursor` 就是這樣），所以照 Chrome 152 改成 caret 能在的第一個位置（Blink 的 `FirstPositionInOrBeforeNode` 加上 canonical position；`details.md`）。

2026-10-23 確認第 2 段的事（使用者：「好」）：

4. **數學函式做 css-values-4 的全部**：`round()`、`mod()`、`rem()`、`abs()`、`sign()`、`sin()`、`cos()`、`tan()`、`asin()`、`acos()`、`atan()`、`atan2()`、`pow()`、`sqrt()`、`hypot()`、`log()`、`exp()`（Chrome 152 都有）。能在解析時算出來的照舊算出來；有相對單位的（`sign(1em)`、`round(1em, 3px)`、`hypot(1em, 3px)`）留到 element 上算，`calc(10px * sign(1em))` 這種長度乘上還沒算出來的 number 也是。數值照舊是 `f32`（Chrome 是 double），混合百分比和長度的照舊不解析（和 `min()`、`max()` 一樣）。不選的做法：只做 `round()`、`mod()`、`rem()`，或再加上 `abs()`、`sign()`（三角函數和指數函數大多在解析時就算完，多做的成本不高）。

2026-10-23 確認第 3 段的事（使用者：「都照建議」）：

5. **文件的 stylesheet 由 `dom` 持有**：每個 `<style>`、`<link>` 的 rare data 有一份 `Arc<StyleSheetContents>`（和 constructed stylesheet 同一種），`<link>` 載入的文字由 `FrameView` 交給 `dom`；style engine 從 `dom` 讀，比較時比 `Arc` 的指標，文件的 sheet 也從 `css::Stylesheet` 換成 `StyleSheetContents`。Chromium 一樣（`StyleElement`、`LinkStyle` 持有 `CSSStyleSheet`，包著 `StyleSheetContents`）。不選的做法：engine 照舊從文字解析、script 改過的另外疊一份（同一份 sheet 兩個來源，`sheet` 物件要自己再解析一次，`<link>` 的文字照樣要交給 `dom`）。
6. **第一次需要時才解析**：重算 style 或 script 讀 `sheet` 時，由 `dom` 解析、快取在 rare data，文字一改就丟掉。不選的做法：照 Blink 每次文字改變就解析（`StyleElement::ProcessStyleSheet`）：parser 插入文字節點的路不經過 `dom_api`，要在每條改變文字的路上加 hook，連續 append 文字的頁面（emotion 的 development 模式）是 O(n²)。
7. **共用解析結果的快取搬進 `dom`**：以（文字、base URL）為鍵、`Weak<StyleSheetContents>` 為值，文件和 shadow tree 的 `<style>` 都先查它（Blink 的 `StyleEngine::text_to_sheet_cache_`）；script 改其中一個時換成新的 `Arc`，其他的不受影響。不選的做法：只有 shadow tree 用快取（兩條路）。量測照「詳細設計」：`bench.py blink-perf --dirs css --against main`，Speedometer 只在使用者要求時跑。

2026-10-24 確認第 4 段的事（使用者：「第四點選乙，其他照建議」）：

8. **`sheet` 物件沿用 `CssStyleSheet`**：同一個 GC 型別多一個 `owner_node`，元素的 rare data 有一個指向它的 edge，script 第一次讀 `sheet` 或 `styleSheets` 的項目時建立；改動由 `dom` 的同一個函式同時換掉物件和元素的 `Arc`、標記 tree scope，style engine 照第 5 條從元素讀；sheet 重新做的時候 `ownerNode` 變 `null`，物件和元素分開。Chromium 一樣（一個 `CSSStyleSheet`，`owner_node_` 分來源）。rare data 多一個指標，要量 `ChangeStyleCSSVariableRecalc`，變慢時放進 `<style>`、`<link>` 才有的 box。
   - 2026-10-24 實作時改：sheet 重新做的地方（插入、移除、文字改了）不能寫 heap，不清 edge，改用「sheet 做過幾次」的編號判斷舊的物件是不是元素的 sheet。物件和 `styleSheets` 的 edge 一起放進 `<style>`、`<link>`、document、shadow root 才有的 GC 物件（`StyleSheetData`，取代第 3 段的 box），rare data 的大小和第 3 段一樣（`details.md` 的「第 4 段的量測」）。
   - 不選的做法：文件的 sheet 用新的 GC 型別（CSSOM 的每個方法分兩條路）；建立物件後把內容搬進物件（engine 的路分有沒有物件兩種）。
9. **`styleSheets` 的清單有快取**：每個 tree scope 記著它的 `<style>`、`<link>`（tree order），這些元素插入、移除、sheet 重新做的時候作廢（Blink 的 `TreeScopeStyleSheetCollection` 也是 dirty 了才重算）。不選的做法：每次讀都走 tree scope（`for (i < styleSheets.length)` 是 O(節點數 × sheet 數)）。
10. **跨 origin 的 `<link>` 照 Chrome 不能讀規則**：載入後最後的網址和文件不同 origin 的不是 origin-clean（`data:` 的是），讀 `cssRules`、`rules`，`insertRule`、`deleteRule`、`addRule`、`removeRule` 丟 `SecurityError`；有 `crossorigin` 的 `<link>` 改用 CORS 載入（和 script 的 `cors_request` 同一條路），通過的是 origin-clean，沒通過的照 Chrome 不套用。不選的做法：只比 origin、`crossorigin` 照舊（CDN 上 SRI 寫法的 sheet 在 Chrome 讀得到）；不檢查（script 讀得到別的 origin 的 CSS）。
11. **大的 sheet 各自一份 `RuleSet`，連續的小 sheet 合成一份**：規則數到門檻的 sheet 的 `RuleSet` 快取在它的 `StyleSheetContents` 上，沒變就不重建；夾在它們之間、連續的小 sheet 合成一份，其中一個變了只重建那一份。document 的 cascade 由這些 set 依序組成，比對時查的 set 最多約「大 sheet 數 × 2 + 1」個。門檻用 microbenchmark 定（`details.md`）。
    - 2026-10-24 實作時加：門檻是 512 條（一個 set 讓一次全部重算每個元素多約 30 ns，重建一條規則 0.15 到 0.2 µs）。只分 `RuleSet` 不夠：每次 sheet 變了，全部規則的 invalidation 特徵和 dependency 也重新算一次，比建 `RuleSet` 還貴，所以也照 Chromium（每個 `RuleSet` 帶著它的 `RuleFeatureSet`）每個 set 算一次、存著（`SetFeaturesCache`），tree scope 合併各 set 的。media 改成比指標之後，重新插入、文字一樣的 `<style>` 會被當成新的 sheet（`SelectorCountScaling` 慢一倍），所以指標不同時比 media 的內容。
    - 確認前量過（`details.md` 的「第 4 段：`RuleSet` 怎麼分」）：每個 sheet 一份時，2000 個元素的重算在 20 個 sheet 以內量不出差別，200 個慢 22%、1000 個慢 68%；大的框架 sheet（5000 條）加上變了的小 sheet（500 條）時，全部重建 1.01 ms，只建變了的 0.055 ms；把各 sheet 的 bucket 接起來每條規則約 0.12 µs，和從頭建（0.15 到 0.2 µs）差不多。
    - 使用者先選了照 Chromium 每個 sheet 一份（Blink 的 `ScopedStyleResolver` 逐一查各 sheet 的 `RuleSet`），補上比對的成本之後，2026-10-24 改選這個（使用者：「乙三」）。
    - 不選的做法：照 Chromium 每個 sheet 一份（幾百個 `<style>` 的頁面每次重算都慢）；各 sheet 快取、document 的那份由它們的 bucket 接起來（接的成本和重建差不多，變了的 sheet 大時比現在慢）；照舊每次全部重建（大的框架 sheet 跟著每次 `insertRule` 重建）。
    - 量測時也發現 `insertRule` 本身是 O(sheet 的規則數) 而且常數大（每次複製、丟掉整個規則的清單，每條規則都重新檢查 `url()`，讀 `cssRules.length` 也複製一次）：20000 條的 sheet 每個 frame `insertRule` 5 條要 16.6 ms，`RuleSet` 不到 1%。這段一起修，用計數的測試守住。
12. **`@import` 不在這份**：另外寫 RFC（要載入、`CSSImportRule.styleSheet`、`load` 事件、循環）。

## 待決定

沒有。

## 之後的可能

- `@import` 和 `CSSImportRule`。
- 插在一個 sheet 最後的規則直接加進它既有的 `RuleSet`（CSS-in-JS 的一個大 sheet 一直 `insertRule` 時，現在那份 `RuleSet` 每次重建：20000 條約 3 ms）。
- shadow tree 裡的 `<link rel=stylesheet>` 的載入。
- `<link>` 的 `href` 改變時重新載入、`load` 和 `error` 事件。
- `DOMPoint`、`DOMQuad`、`DOMMatrix`。
- `CSSStyleDeclaration` 以外的 CSSOM：`CSSFontFaceRule`、`CSSPageRule` 等。這要 parser 先收那些 at-rule。
