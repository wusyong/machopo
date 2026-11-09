# RFC 0031：SVG

- 開始日期：2026-10-10
- 狀態：✅ 已完成：2026-10-11（R1、G1、O1、A1、B1、M1、X1 在 2026-10-10 確認；分四段，每段和它的小段開始前確認第 8 到 57 條；路徑、漸層、`DrawGlyphs` 的型別（2a-0、2d-0、2e-0）和第 1 到 3 段先 merge 回 `main`；四段都做完，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`
- 進度：`progress.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 O，雜項線接在 L（RFC 0027）之後。和 N（Canvas 2D）互不依賴。

## 摘要

讓 kun 有 SVG：先是 SVG 的 DOM（每個 SVG 元素實作自己的 interface，`SVGElement` 和 HTML 元素共用的成員），再來是 inline `<svg>` 的繪製（基本圖形、`<g>`、`transform`、填色和線條、文字），之後是要幾何的 API（`getBBox()`、`getScreenCTM()`、`createSVGPoint()`）和 SVG 當圖片（`<img src="x.svg">`、`background-image`）。繪製照 Chromium：SVG 在 kun 自己的 layout tree 裡有自己的 layout 物件，路徑交給現有的 paint 和 Skia 畫，不交給外部的 SVG 函式庫；SVG 特有的語法用 `svgtypes`，幾何用 `kurbo`，規範的演算法（基本圖形轉路徑、單位、漸層的繼承、`marker` 的角度、文字的定位等）從 usvg 移植。分四段，第 1 段（DOM）不畫任何東西，就能讓 TodoMVC-Vue 和 NewsSite-Nuxt 往下跑。

## 動機

**現況（2026-10-10，`main` 的 `2ca5ac82`；細節在 `details.md` 第 1 節）**：
- SVG namespace 的元素（parser 建的或 `createElementNS`）都只實作 `Element`：沒有 `SVGElement` 和它的子 interface，`window.SVGElement` 不存在（`docs/limitations.md` 的「Bindings 和 DOM API」）。SVG 元素沒有 `dataset`、`style`、`className`、`onclick` 這些和 HTML 元素共用的成員（同一節）。
- `<svg>` 不畫：layout 把 SVG 元素當成不認得的元素排版，`<text>` 裡的字照一般文字畫出來，圖形看不到。`<img>` 和背景圖不能是 SVG（`docs/limitations.md` 的「圖片」）。
- SVG 的 `<script>` 不執行（`docs/limitations.md` 的「Script」）。
- **Speedometer 的盤點**（2026-10-10，`rfcs/0012-speedometer/progress.md`）：TodoMVC-Vue（M2）和 NewsSite-Nuxt（M5）在一開始就停在 `ReferenceError: SVGElement is not defined`（Vue 3 的 `app.mount` 把 `container instanceof SVGElement` 傳給 render）。Charts-observable-plot、NewsSite-Next、NewsSite-Nuxt、React-Stockcharts-SVG、TodoMVC-Lit-Complex-DOM 都用 `createElementNS` 建 SVG（`svg`、`path`、`title` 最多）；React-Stockcharts-SVG 另外用 `getScreenCTM()`、`createSVGPoint()`、`SVGPoint.matrixTransform()`、`SVGMatrix.inverse()`（M4）。路線圖的 S2（背景和邊框）也等 SVG 的圖片。

目標：
- 每個 SVG 2 定義的元素實作自己的 interface，原型鏈照規範（`SVGRectElement` → `SVGGeometryElement` → `SVGGraphicsElement` → `SVGElement` → `Element`），JS 和 wasm 都有（RFC 0012 的第 7 條）。
- `SVGElement` 有不需要 layout 的成員：`className`、`ownerSVGElement`、`viewportElement`，和 HTML 元素共用的 `dataset`、`style`、`tabIndex`、`focus()`、`blur()`、event handler。
- inline `<svg>` 的繪製：`rect`、`circle`、`ellipse`、`line`、`polyline`、`polygon`、`path`、`g`、`defs` 和 `use`、`text` 和 `tspan`；`fill`、`stroke` 和它們的 presentation attribute；`transform`、`viewBox`；hit test 和事件。
- 要幾何的 API：`getBBox()`、`getCTM()`、`getScreenCTM()`、`createSVGPoint()` 和點、矩陣的型別。
- SVG 當圖片：`<img>`、`background-image`。
- WPT 的 `svg/` 裡用得到的部分。

不在範圍內（「之後的可能」）：
- SMIL 動畫（`<animate>`、`<set>`、`<animateMotion>`、`<animateTransform>`）：只有 interface，不動。
- filter（`<filter>`、`fe*`）、`clipPath`、`mask`、`pattern`、`marker`：paint server 只有純色和兩種漸層（第 11 條）。
- 各元素的 `SVGAnimated*`（`x.baseVal`、`viewBox.baseVal`、`transform.baseVal`，第 36 條）。
- 獨立的 SVG 文件（`.svg` 頂層導覽到的文件，第 50 條）：要 frame 裡的 XML 文件、SVG 的 `<script>`；`DOMParser` 的 XML 類型（第 49 條）。
- SVG 的 `<script>`（第 7 條）。
- SVG 字型、`textPath`、`foreignObject` 的繪製。
- MathML 的 `MathMLElement`。

## 設計概要

對寫網頁的人：
- `document.createElementNS("http://www.w3.org/2000/svg", "rect") instanceof SVGRectElement` 是 `true`；`svg.dataset.x`、`path.style.fill = "red"`、`circle.onclick = …`、`rect.className.baseVal` 照規範。
- `<svg width="100" height="100"><circle cx="50" cy="50" r="40" fill="green"/></svg>` 畫出圓；`transform`、`viewBox` 照規範縮放和移動；`<use>`、漸層、`<text>` 照 Chrome 畫；點到圖形會分派事件，SVG 的 `<a>` 點了或 focus 之後按 Enter 換頁。
- `el.getBBox()`、`svg.createSVGPoint().matrixTransform(el.getScreenCTM().inverse())`（D3、React-Stockcharts 把滑鼠的位置換成圖的座標）、`path.getTotalLength()`、文字的方法照 Chrome 152 的值。
- `<img src="icon.svg">`、`<picture>` 的 `image/svg+xml` 和 `background-image: url(icon.svg)` 畫成向量，大小照 Chrome。

對改 kun 的人：
- 宣告表的 interface 可以寫 `svg_tags(...)`，和 HTML 的 `tags(...)` 一樣決定元素的種類；`dom_api` 的 `element_kind` 先看 namespace。新的檔 `interfaces/core_svg.rs`（Blink 的 `core/svg`）。
- `HTMLElement` 和 `SVGElement` 共用的成員照規範的 mixin（`HTMLOrSVGElement`、`ElementCSSInlineStyle`、`GlobalEventHandlers`）放在共用的函式，兩個 interface 各自宣告。
- SVG 特有的 attribute 語法（path data、`transform`、`points`、`viewBox`、`preserveAspectRatio`）用 `svgtypes` 解析，CSS 的值照舊由 `style` 解析；規範的計算從 usvg 移植，檔案開頭寫出出處（`details.md` 第 4 節）。
- 繪製：最外層的 `<svg>` 在 CSS 的排版裡是 replaced element（`Replaced::svg`），它的子孫是 `layout` 的 `svg` 模組抓下的另一棵樹（Blink 的 `LayoutSVGShape`、`LayoutSVGContainer`、`LayoutSVGText` 的角色），在自己的座標系統裡；pre-paint 替 `viewBox`、`transform`、`opacity`、巢狀 `<svg>` 的裁切建 property node，`paint` 把路徑交給 display list，raster 照舊用 Skia。
- SVG 圖片：`html` 的 `xml` 模組（`xml5ever`）把 SVG 檔解析成網頁的 `DomIsolate` 裡的一份文件，renderer 的 `SvgImage` 照每個框或圖塊的大小用同一套 style、layout、paint 錄成 display list，raster 重播（`PaintOp::DrawRecord`、`ShaderImage::Record`）。

## 詳細設計

（整體的選項在 `details.md` 第 2 節；各段的選項和理由在「已決定的事項」。）

- **元素的 interface**（第 1 段）：`dom_api` 的 `html_element::element_kind` 改成先看 namespace：HTML 照舊；SVG namespace 照宣告表的 `svg_tags(...)` 查（Blink 的 `svg_tag_names.json5` 和 `SVGElementFactory`），查不到的是 `SVGElement`（規範：SVG namespace 裡未知的元素是 `SVGElement`）；其他 namespace 照舊是 `Element`。`ObjectKind` 多一個 SVG 的區塊；每個 SVG 2、Filter Effects、SVG Animations 定義的元素各一種，中間層（`SVGGraphicsElement`、`SVGGeometryElement`、`SVGTextContentElement`、`SVGTextPositioningElement`、`SVGGradientElement`、`SVGAnimationElement`、`SVGComponentTransferFunctionElement`）是抽象的 interface。
- **`SVGElement` 的成員**（第 1 段）：`className`（`SVGAnimatedString`：`baseVal` 讀寫 `class` attribute，`animVal` 沒有動畫時等於 `baseVal`）、`ownerSVGElement`、`viewportElement`（最近的 `<svg>` 祖先；最外層的是 `null`）。和 `HTMLElement` 共用的 mixin 從 `html_element.rs` 搬到共用的位置：`HTMLOrSVGElement`（`dataset`、`nonce`、`autofocus`、`tabIndex`、`focus()`、`blur()`）、`ElementCSSInlineStyle`（`style`）、`GlobalEventHandlers` 和 `DocumentAndElementEventHandlers`。SVG 元素什麼時候可以 focus、`tabIndex` 的預設值照 Chrome（開始時實測）。
- **繪製**（第 2 段，第 10 到 23 條）：`<svg>` 是 CSS 排版裡的 replaced element（大小照 `width`、`height` 和 CSS，`viewBox` 給比例，預設 300×150，照 Chrome 152 的實測）；子孫不參加 CSS 的 box 排版，建 box 時由 `layout` 的 `svg` 模組抓成 `SvgSource`（元素的種類、computed style、`viewBox` 這些 attribute），layout 時照內容框算出 `SvgFragment`：每個元素的變換、巢狀 `<svg>` 的 viewport 和裁切、每個圖形的 `gfx::Path`（圓、橢圓、圓角和弧線照 Blink 是 Skia 的 conic）和 bbox；子樹有任何變動時整個重抓（R1）。幾何的值和 `fill`、`stroke` 等是 CSS property，attribute 是它們的 presentation attribute（specificity 0、在 author 規則之前），經過 kun 的 cascade；`transform` attribute 是 CSS `transform`，origin 固定是 `0 0`。pre-paint 替內容框和 `viewBox`、每個元素的 `transform`、`opacity` 的 effect、巢狀 `<svg>` 的裁切建 property node（P2、O2，照 Chrome）；paint 照文件順序，每個圖形一個 display item，先 fill 再 stroke；`<defs>`、沒被用到的 `<symbol>`、`display: none` 的 `<g>` 是隱藏的容器，不畫但算得出幾何。hit test 在 `paint` 裡用 `kurbo` 算 fill 的 winding number 和 stroke 的外框，`pointer-events` 的全部值照規範（第 24、25 條）。
- **`<use>`、`<symbol>`、`<switch>`**（2c，第 27 條）：`style` 的 `svg_use` 模組在 style 重算之後，替這次重算的 `<use>`、和參考或複製的元素被改到的 `<use>` 重做複本：照 id 找目標（樹的順序的第一個；`href` 優先，沒有時 `xlink:href`，只認 `#id`），每個元素用 `StyleElement::in_copy` 比對（比對到複本的根為止），父節點的 style 是 `<use>` 或複本裡的父元素，結果是 `dom::SvgUseInstance` 的樹，存在 `<use>` 上，並讓 layout 重抓。engine 記下每個 `<use>` 上次的目標和複製的元素（Blink 的 `IdTargetObserver` 和 shadow tree 的失效）。`layout` 抓 `<svg>` 的子樹時，`<use>` 的子節點是它的複本，用複本的 style、畫成 `<use>`（hit test 回報它）；`<use>` 照 `x`、`y` 平移，參考 `<symbol>`（畫成它的 viewport）或 `<svg>` 時照 `<use>` 的 `width`、`height` attribute（不是 property，和 Chrome 一樣）。`<switch>` 只抓第一個條件成立的子元素；`systemLanguage` 照 `navigator.languages`、`requiredExtensions` 照 Chrome 認得的 namespace。paint 的 `svg_states` 改成照 layout 節點的位置記，同一個元素的不同複本各有 property node。
- **漸層**（2d，第 28 到 30 條）：`compositor_paint` 的漸層照 `cc::PaintShader`（2d-0）。`layout` 的 `svg_gradient` 模組在抓 `<svg>` 時替圖形的 `fill`、`stroke` 的 `url(#id)`（片段 percent-decode，`dom::fragment_id`）找漸層：照 `href` 鏈每個 attribute 取第一個有它的（幾何只從同一種的拿），stop 取第一個有 stop 的，顏色乘上 `stop-opacity`；layout 時照圖形的 tight bbox（`gfx::Path::tight_bounds`）或 viewport 換算成 user space 的漸層（`SvgPaintServer`），長度是 0 或一個 stop 的變成一個顏色，bbox 沒有寬或高的用 fallback。paint 把它變成不用 tile 的 shader（unpremultiplied，放射的從 focal 圓到結束的圓），opacity 放在 flags 的 alpha。`style` 的 `svg_resources` 模組記下圖形和 `<use>`（它的 copy 的 style）參考到的元素，它們被改到、或鏈換了元素時讓圖形重抓。`gradientTransform` 是漸層元素的 `transform` presentation attribute（2a-1 就照 Chrome 這樣對應），layout 從它的 computed style 讀。
- **文字**（2e，第 31 到 34 條）：`compositor_paint` 的 `DrawGlyphs` 帶 `PaintFlags` 和每個 glyph 的角度（2e-0）。`style` 加 `text-anchor`。`layout` 的 `svg_text` 模組在抓 `<svg>` 時（建 box tree 時，有 `TextContext`）收集 `<text>` 和它的 `<tspan>`、`<a>` 的文字：空白照 Chrome 152 的實測，跨元素合併、去掉頭尾（`xml:space="preserve"` 和不合併的 `white-space` 保留，換行和 tab 變成空格）；每個字元記下它所在的最內層元素，`x` / `y` / `dx` / `dy` / `rotate` 照「最內層有值的元素」（`rotate` 的最後一個值延續到那個元素的結尾）；整段用 parley shape 成一行（`text::shape_line`），記下每個 grapheme cluster 的 advance 和 glyph。layout 時照 SVG 2 的演算法放每個 cluster，`x` 或 `y` 開始一個 chunk，chunk 照它第一個字元的 `text-anchor` 移，結果是 `SvgText`：每段同字型、同元素的 run（paint 用 `DrawGlyphs` 先填色再描邊）和每個字元的格子（hit test，回報它所在的元素鏈；bbox 是 `objectBoundingBox`）。`<use>` 複製的 `<text>` 照 copy 的 style、畫成 `<use>`。
- **幾何的 API**（第 3 段，第 36 到 45 條）：`SVGMatrix`、`SVGPoint`、`SVGRect` 照 Chrome 是各自的 interface（`dom` 的 GC 型別）。`getBBox()`、`getCTM()`、`getScreenCTM()`、SVG 元素的 client rect、`getTotalLength()`、`getPointAtLength()`、`isPointInFill()`、`isPointInStroke()` 和文字的方法經過 `LayoutClient`（renderer 先同步 layout，和 CSSOM View 一樣），由 `layout` 的 `svg_geometry` 模組從 layout 的 SVG 樹算，不跑 pre-paint（Chrome 也只更新到 layout）；外面的 CSS transform 和捲動照 `getClientRects` 的走法。路徑長度照 Chrome：基本圖形照 Skia 的 `SkContourMeasure`，`<path>` 照 Blink 的 `PathTraversalState`（`gfx` 的 `path_measure`）；文字的方法照 UTF-16 的 code unit 編號。沒畫出來的元素照 Chrome 152 的實測（第 40、45 條）。SVG 元素的 focus 照 Chrome（有連結的 `<a>`、有 `tabindex` 的、有 focus listener 的圖形元素），`outline` 畫在元素自己的座標裡（第 41、42 條）。
- **SVG 當圖片**（第 4 段，第 46 到 57 條）：照 Chromium 的 `SVGImage`。`image/svg+xml` 的 response 照 XML 的規則解碼（BOM、header、`<?xml encoding?>`、UTF-8），`html` 的 `parse_xml_document` 用 `xml5ever` 建進 HTML 共用的 `DomSink`；交給它之前先拿掉 BOM、`<?xml ?>` 和 DOCTYPE 的內部宣告，展開宣告的一般 entity（深度和總量有上限）。格式錯誤的照 `xml5ever` 修復後的樹畫（Chrome 是壞圖，第 51 條）。文件放在網頁的 `DomIsolate` 裡，沒有 window、不跑 script；根元素不是 SVG 的 `<svg>` 是壞圖。renderer 的 `SvgImage` 跑一次 style（SVG 的 `<style>` 也算，第 56 條），固有大小照根 `<svg>` 的 `width`、`height`、`viewBox`（`Replaced` 的每一邊各自可以沒有）；要畫某個框或圖塊的大小時，有固有大小的照它排版、沒有的照框排版，`layout::svg_image_fragment`、`pre_paint`、`paint`、`to_display_list` 錄成一份 display list，每個大小一份（`SVGImageForContainer`）。paint 經過 `BackgroundImages::svg_image` 拿；還沒錄的大小記下來，renderer 錄好之後讓問的元素再 paint 一次。`<img>` 畫成 `PaintOp::DrawRecord`（raster 裁切、縮放到框再重播），背景的圖塊是 `ShaderImage::Record`（錄進 picture shader 鋪滿），都是向量；背景的 `background-size` 對沒有固有大小的圖片照 CSS 的 default sizing algorithm。
- **wasm**：每個 interface 照宣告表進 `kun:dom`，`kun:dom` 照 `crates/dom_api/README.md` 的規則升版。
- **Process 的邊界**：都在 renderer 裡；沒有新的訊息。SVG 圖片的編號是文件的 id 加上 frame 的 `ImageTable` 自己的計數，沒有 process 共用的全域狀態。

### 測試

- `dom_api` 的單元測試：每個 SVG 的 tag 對到的種類、未知的 SVG 元素是 `SVGElement`、別的 namespace 照舊；`className`、`viewportElement`、`ownerSVGElement`。
- `dom_js`：原型鏈（`instanceof`、`Object.getPrototypeOf` 一路到 `Element`）、`window` 上的 interface object、和 `HTMLElement` 共用的成員在 SVG 元素上一樣運作。
- 規範的 WebIDL 比對（`crates/dom_api/tests/it/spec.rs`）加讀 `SVG.idl`：沒做的成員列在 `spec-missing.txt`。
- WPT：`svg/idlharness.window.js`、`svg/historical.html`、`svg/struct/scripted/` 的 interface 部分、`html/dom/elements/global-attributes/dataset.html` 的 SVG 部分；第 2 段起 `svg/` 的 reftest（`shapes/`、`painting/`、`coordinate-systems/` 能跑的）。
- Speedometer 的盤點（`ci.py speedometer --inventory`）：TodoMVC-Vue、NewsSite-Nuxt 不再停在 `SVGElement`。
- `layout`、`paint`、`style` 的 SVG 測試，`shell` 的幾何和圖片測試（值照 Chrome 152 的實測），`html` 的 XML parser 測試。
- kun 自己的 reftest（`svg/`、`images/`、`backgrounds/`、`hidpi/` 的 SVG）和情境 `svg`（點擊、Tab 和 Enter）。
- mutation test：每一段照 `docs/testing.md` 的「Mutation test」。

### 分階段

1. **SVG 的 DOM**（1a、1b）：每個元素的 interface、`SVGElement` 的成員、和 HTML 元素共用的 mixin；`className`。
2. **inline `<svg>` 的繪製**（2a-0 路徑的型別、2a-1 style、2a-2 layout 和 paint、2b hit test、2c `use`、2d-0 漸層的型別、2d 漸層、2e-0 `DrawGlyphs` 的 flags、2e 文字）。
3. **要幾何的 API**（3a 點、矩陣、矩形，3b `getBBox()` 等和 focus，3c 長度和文字的方法）。
4. **SVG 當圖片**（4a XML parser，4b `<img>`，4c `background-image`）。

第 1 到 3 段各自做完時先 merge 回 `main`（`rfcs/README.md` 的「流程」第 5 步），2a-0、2d-0、2e-0 是給 canvas 那條線用的型別，單獨先 merge（第 14、29、32 條）。

## 缺點

- **XL**：完整的 SVG 是一大塊（Blink 的 `core/svg` 和 `core/layout/svg` 加起來數萬行）。分段做，第 1 段就有用；但第 2 段之後每段都大。
- **宣告表多約 70 個 interface**：大多沒有自己的成員，但每個都要進 `ObjectKind` 和規範的比對（WIT 只有 `SVGElement`，第 9 條）。
- **SVG 的子樹有變動時整個重抓**（第 16 條）：上千個節點、每個 frame 只動一個的圖會慢，量到時再改成每個元素自己的 dirty 標記。
- **`<use>` 的複本不是 DOM 節點**（第 27 條）：原本的元素上的 listener、複本自己的 `:hover` 和 Chrome 不同。
- **SVG 圖片和 Chrome 不同的地方**：格式錯誤的照樣畫（第 51 條），文件和網頁共用 `DomIsolate`（第 54 條），沒有網址的 fragment（`#svgView()`）和動畫。

## 理由和其他做法

- **繪製照 Chromium，在 kun 自己的 layout 裡（G1），不交給 `resvg` 這類函式庫（G2）**：inline SVG 是活的 DOM，script 隨時改 attribute 和 style、要 hit test 和事件、`getBBox()` 要 layout 的結果；外部函式庫吃的是一份靜態的文件，每次改動都要整份轉一次，CSS 的 cascade 和 kun 的也對不上。代價是要自己寫 SVG 的 layout 物件、invalidation 和文字；Skia 已經會畫路徑。fork usvg 讓它吃 kun 的 DOM 也解決不了：它從頭到尾是一次性的編譯器（整份重建、攤平節點、自己的 CSS 和字型），改到能增量更新、對回 DOM 元素等於在 fork 裡重寫，違反「fork 盡量小」。能重用的是不碰樹的部分：語法（`svgtypes`）、幾何（`kurbo`）、規範的演算法（移植約 5900 行，`details.md` 第 4 節）。SVG 當圖片（第 4 段）比較過 `resvg`，選了同一套（第 48 條）：圖片照樣要照大小重畫，和 inline 的一套實作才不會畫得不一樣。
- **先做 DOM（第 1 段）**：最小、立刻解開兩個 suite（其中 TodoMVC-Vue 是 M2 剩下的 suite 之一），而且之後每一段都要用到元素的種類。路線圖原本寫「先繪製、再 DOM」；盤點之後 DOM 的價值比較早出現。
- **每個元素都有自己的 interface，不是全部 `SVGElement`**：`instanceof SVGSVGElement` 是常見的偵測，Lit 和 D3 這類函式庫會用；只有一層的話之後還要再改一次 `element_kind`。
- 完整的比較在 `details.md` 第 2 節。

## 先例

- **Chromium**：`core/svg` 的 `SVGElement` 和每個元素的類別，`SVGElementFactory` 照 `svg_tag_names.json5` 建；屬性是 `SVGAnimatedProperty`（tear-off：`baseVal` 和 `animVal`）；layout 在 `core/layout/svg`（`LayoutSVGRoot` 是 CSS 裡的 replaced box，子孫是 `LayoutSVGShape`、`LayoutSVGContainer`、`LayoutSVGText` 等，在自己的座標系統裡）；paint 在 `core/paint/svg_*_painter.cc`；SVG 圖片是 `core/svg/graphics/SVGImage`（獨立的 `Page` 和文件）。kun 照這個分層。
- **Firefox**：SVG 的 frame 在 `layout/svg`（`SVGOuterSVGFrame` 是 CSS 排版裡的 replaced frame），同樣在自己的 layout 裡。
- **RFC 0028 的 `HTMLElement` global attribute**：`HTMLOrSVGElement` 的成員在那裡做了 HTML 的部分，這份照用、搬到共用的位置。

## 已決定的事項

2026-10-10 確認第 1 到 7 條（比較在 `details.md` 第 2 到 4 節）。

1. **R1：這份涵蓋路線圖的 O 全部，分四段**（2026-10-10）：SVG 的 DOM、inline 的繪製、要幾何的 API、SVG 當圖片；各段的設計在開始前定。幾何的 API 就是 inline 繪製的 layout 結果，一份文件描述同一套 layout。不選 R2（DOM 和 inline 的繪製一份，幾何和圖片另開）：兩份文件描述同一套 layout 物件。
2. **G1：inline 的繪製在 kun 自己的 layout 裡，照 Chromium；語法用 `svgtypes`、幾何用 `kurbo`，規範的演算法從 usvg 移植**（2026-10-10）：SVG 的 layout 物件（Blink 的 `LayoutSVGRoot`、`LayoutSVGShape` 等），畫進現有的 display list 和 Skia。可以移植的模組和它們的用途在 `details.md` 第 4 節；搬過來的檔案開頭寫出原始的 crate、版本、路徑和 license（kun 的 license 先不定，只標出處）。不選 G2（每次改動把子樹轉成 usvg 的樹、交給 resvg 畫）：整份重建、CSS 和文件的對不上、hit test 和 `getBBox()` 要再算一次、反鋸齒和 kun 的不同；fork usvg 讓它吃 kun 的 DOM 也一樣，要改到能增量更新、對回 DOM 元素，等於在 fork 裡重寫。SVG 當圖片的做法（G1，或 `resvg`、Skia 的 SVG 模組）在第 4 段決定：圖片不跑 script、不能互動，但照樣有動畫、照大小重畫、fragment 和媒體查詢，而 kun 還沒有讀 `.svg` 檔要的 XML parser（第 47、48 條選了 `xml5ever` 和同一套 layout）。
3. **O1：先做 SVG 的 DOM**（2026-10-10）：TodoMVC-Vue（M2）和 NewsSite-Nuxt（M5）只缺 `SVGElement` 這個 interface object，第 2 段的 layout 也要先分得出元素的種類。不選 O2（照路線圖原本的順序，先做繪製）：DOM 的價值比較早出現。
4. **A1：規範定義的每個 SVG 元素都有自己的 interface**（2026-10-10，第 1 段）：SVG 2、CSS Masking、Filter Effects、SVG Animations 的 69 個（`details.md` 第 3 節），未知的 SVG 元素是 `SVGElement`。查表的成本一樣，Chrome 全部都有。不選 A2（只做常用的）：其他元素的 `instanceof` 和 Chrome 不同，之後還要補。
5. **B1：第 1 段只做 `SVGElement` 的成員和共用的 mixin**（2026-10-10，第 1 段）：`className`、`ownerSVGElement`、`viewportElement`，和 `HTMLOrSVGElement`、`ElementCSSInlineStyle`、event handler。不選 B2（連同各元素的 `SVGAnimated*` 屬性）：`SVGAnimatedLength`、`SVGAnimatedTransformList` 和 layout 的單位、矩陣有關，第 3 段才有東西可以回答。
6. **M1：共用的成員搬到照規範的 mixin 命名的模組**（2026-10-10，第 1 段）：`HTMLElement` 和 `SVGElement` 各自宣告。等 RFC 0028 改 `html_element.rs` 的那一段 merge 之後再搬，避免衝突。不選 M2（`SVGElement` 直接指到 `html_element.rs` 的函式）：名稱和規範不一致。
7. **X1：SVG 的 `<script>` 留到之後**（2026-10-10，第 1 段）：和 Speedometer 無關，還要處理 `href`（和 `xlink:href`）。不選 X2（第 1 段一起執行）。
8. **第 1 段分成兩段**（2026-10-10，第 1 段開始前）：1a 元素的 interface（`svg_tags(...)`、`ObjectKind`、`element_kind`）和不需要新 GC 型別的成員（`ownerSVGElement`、`viewportElement`，共用的 `style`、`dataset`、`tabIndex`、`focus()`、`blur()`、`autofocus`），做完 TodoMVC-Vue、NewsSite-Nuxt 不再停在 `SVGElement`；1b `className`（`SVGAnimatedString`，新的 GC 型別，照 `dataset` 的 `DatasetDomStringMap`），GC 的改動和 soundness 的論證單獨一段、單獨審查。`nonce` 兩邊都還沒有（`HTMLElement` 也沒有），這一段不做。Chrome 的實測（`tabIndex`、哪些名稱有自己的 interface）記在 `progress.md`。
9. **只有 `SVGElement` 進 `kun:dom`**（2026-10-10，1a 實作中）：71 個 interface 全部進 WIT 時它從 6965 行變成 16212 行（WIT 的 resource 沒有繼承，每個都重複 `Element`、`Node`、`EventTarget` 的成員），其中 70 個這一段沒有自己的成員。宣告表多一種 `[JsOnly] interface`：JS 照樣有它的 interface object 和原型鏈，WIT 沒有它的 resource，型別是它的參數和回傳值換成最近有 resource 的祖先（`ownerSVGElement` 在 WIT 裡回傳 `svg-element`），guest 拿到的每個 SVG 元素都是 `svg-element`。之後某個 interface 有了自己的成員（第 3 段的 `getBBox()` 等）再決定要不要給它 resource。這是 RFC 0012 第 7 條（新的 API 同時給 JS 和 wasm）允許的例外。不選全部進 WIT：WIT、Lua 的 runtime 和 host 的 binding 多一倍以上；不選「`SVGElement` 和 `SVGSVGElement` 都進」：`SVGSVGElement` 這一段沒有自己的成員，只為了 `ownerSVGElement` 的回傳型別多一個 resource（和它重複的上百個成員），型別換成祖先就夠了。

2026-10-11 確認第 10 到 14 條（第 2 段開始前）。

10. **S1：第 2 段分成五段**（2026-10-11）：2a `<svg>` 是 replaced element（沒有大小時 300×150）、`viewBox` 和 `preserveAspectRatio`、`g`、六種基本圖形和 `path`、純色的 `fill` 和 `stroke`（連同 stroke 的其他 property）、`transform`、presentation attribute；2b hit test 和事件（`pointer-events`、fill 和 stroke 的範圍）；2c `defs`、`use`、`symbol`、`switch`；2d 漸層；2e `text`、`tspan`。每一段都畫得出、測得到東西，2a 做完 Speedometer 頁面上的圖示就看得到。不選 S2（第 2 段一次做完）：範圍太大，審查不了。
11. **P2：第 2 段的 paint server 是純色、`linearGradient` 和 `radialGradient`**（2026-10-11）：display list 已經有 CSS 背景用的漸層 shader（`PaintShader`），usvg 的 `paint_server.rs` 在移植清單裡；圖示和圖表常用漸層。不選 P1（只有純色）：漸層常見，之後還要回來接；不選 P3（連同 `pattern`）：少見，留在「之後的可能」。Chrome 三種都有（`LayoutSVGResource*`）。
12. **L1：`<svg>` 在 taffy 裡是 replaced box，子孫是另一棵 SVG 的 layout 樹**（2026-10-11）：`<svg>` 的大小照 `width`、`height` 和 CSS 算；子孫建成 `LayoutSVGRoot`、`LayoutSVGContainer`、`LayoutSVGShape` 等，不進 taffy，算出路徑、bounding box 和變換，掛在 `<svg>` 的 fragment 上。Chrome 一樣：`LayoutSVGRoot` 是 `LayoutReplaced`，子孫是不走 LayoutNG 的 `LayoutSVG*`，只有文字借用 inline layout。不選 L2（子孫也是 taffy 的節點，用絕對定位擺）：SVG 的子孫不參加 CSS 的 box 排版，座標系統、`viewBox`、路徑和路徑的 bounding box 都是 taffy 不懂的。display list 的 `DrawPath` 跟著改成收曲線和 fill rule（Chrome 的 `DrawPathOp`）。
13. **H1：幾何的值都是 CSS property**（2026-10-11）：`x`、`y`、`width`、`height`、`cx`、`cy`、`r`、`rx`、`ry`、`d` 是 SVG 2 的 geometry property，attribute 是它們的 presentation attribute，經過 cascade；`circle:hover { r: 20px }`、`rect { width: 50% }`、`path { d: path("…") }` 照規範。Chrome 十個都是 CSS property（`d` 也是），Firefox 也是。不選 H2（`d` 先直接讀 attribute）：使用者選和 Chrome 一致，`d` 的值（`path()`）用 `svgtypes` 解析，放進 `style`；不選 H3（全部直接讀 attribute）：CSS 改不了幾何，和 Chrome 不同。presentation attribute 經過 `style` 現有的 presentational hint（author 層、specificity 0、在每條 author 規則之前；Chrome 的 `CollectStyleForPresentationAttribute`）。
14. **路徑的型別由 2a 先定，單獨先 merge**（2026-10-11，2a 開始前）：2a 和 RFC 0033（Canvas 2D）的第 1 段都要把 `compositor_paint` 的 `DrawPath` 從多邊形改成一般的路徑，`PaintFlags` 都要線的端點、接合、miter 和任意長度的虛線。2a 的第一步只做這一塊：`gfx` 自己的路徑型別（Skia 的 `SkPath` 的角色：直線、二次和三次曲線、conic、閉合、fill rule，座標是 `f32`；少了起點的一段照 `SkPathBuilder` 從上一個 subpath 的起點開始；`compositor_paint` 不 link Skia，`raster` 才轉成 `sk::Path`）、`DrawPath` 和 `PaintFlags` 的擴充（端點、接合、miter、任意長度加上 phase 的虛線）、新的 `ClipPath` op（`cc::ClipPathOp`）、`raster` 的轉換和測試。conic、`ClipPath` 和少了起點的規則是 Canvas 那條線要的（canvas 的 `arc`、`ellipse` 照 Chrome 用 Skia 的 conic，像素才一致；`clip()`；`closePath()` 之後接著畫），SVG 的 `clipPath` 之後也用 `ClipPath`；canvas 的 `arcTo` 和 `BuildUnitArc` 的移植只有 canvas 用，留給它。審查之後就 fast-forward merge 回 `main`，不等 2a 做完（`rfcs/README.md` 的「流程」是階段做完才 merge，這是例外），Canvas 那條線 rebase 之後直接用。Canvas 不用停下來，只是在這之前不改那幾處。Chrome 的 canvas 和 SVG 共用 `cc::DrawPathOp` 的 `SkPath`。不選「2a 做完才 merge」：Canvas 那條線要等，或自己先做一份、之後再對齊；不選直接用 `kurbo::BezPath`：display list 之後要跨 process 序列化，canvas 照 Skia 用 conic 畫 `arc` 時也沒有 conic；不選等 Canvas 先定：2a 第一步就要畫路徑。

2026-10-11 確認第 15 到 18 條（2a 開始前）。

15. **2a 分成 2a-1（style）和 2a-2（layout 和 paint）**（2026-10-11）：2a-1 是新的 property 和它們的 presentation attribute：`fill`、`stroke`、`fill-opacity`、`stroke-opacity`、`fill-rule`、`stroke-width`、`stroke-linecap`、`stroke-linejoin`、`stroke-miterlimit`、`stroke-dasharray`、`stroke-dashoffset`，geometry property `cx`、`cy`、`r`、`rx`、`ry`、`x`、`y`、`d`（`width`、`height` 已經有），`d` 的 `path()` 和 `transform` attribute 用 `svgtypes` 解析；`getComputedStyle` 驗證（WPT 的 `svg/painting/parsing/`、`svg/geometry/parsing/` 都是 `.svg` 檔，kun 沒有 XML parser 讀不了，見第 19 條）。2a-2 是 `<svg>` 的 replaced box、SVG 的 layout 樹、`viewBox`、畫進 display list。style 那一半自己有測試，和 layout 不重疊，分開審查。不選 2a 一次做完。
16. **R1：`<svg>` 的 box 帶著整棵 SVG 子樹的 layout 結果，子樹有變動時整棵重算**（2026-10-11）：每個節點的路徑、bounding box、自己的變換（pre-paint 照它建 transform node，第 17 條）；子樹裡的 style、attribute、子節點變了，`<svg>` 的 box 標成 dirty（kun 現有的 dirty box 和 taffy 的快取）。量小；重算和子樹的大小成正比，一般的圖示和圖表很小，paint 照樣沿用沒變的 display item。不選 R2（每個元素常駐的 layout 物件和自己的 dirty 標記，Chrome 的 `LayoutSVGShape`、`SetNeedsShapeUpdate`）：大很多；上千個節點、每個 frame 只動一個的圖（React-Stockcharts-SVG 可能是）量到慢時再改，R1 的結果已經一個節點對一個節點，改的範圍不大。
17. **P2：SVG 的 `transform` 和 `viewBox` 是 property tree 的 transform node，照 Chrome**（2026-10-11，2a-2 開始前改；原本 2026-10-11 選 P1）：pre-paint 走 SVG 的 layout 結果，替有 `transform` 的元素和 `viewBox`（`<svg>` 的內容的變換）建 transform node（Chrome 的 `PaintPropertyTreeBuilder` 的 `UpdateTransformForSVGChild`、`UpdateReplacedContentTransform`）；每個圖形是自己的 display item（client 是它的元素），在自己的座標裡畫。2b 的 hit test、第 3 段的 `getScreenCTM()` 和 SVG 元素的 `getBoundingClientRect()` 用現有的 property tree（`to_root`、`element_bounds`），之後的 `clipPath`、`mask`、`filter` 是 clip 和 effect node，只改 `transform` 時不用重錄 display item。代價：pre-paint、paint 和 hit test 的 walk 要認得不是 `BoxFragment` 的 SVG 節點；圖表的每個 `<g transform>` 一個 node，換 property state 就切一個 paint chunk，2a-2 做完用 React-Stockcharts-SVG 量 chunk 的數量和 paint 的時間。原本選的 P1（變換留在 layout 的結果裡，每個圖形的 item 裡 `Save`、`Concat`、`DrawPath`、`Restore`，`<svg>` 本身才經過 property tree）的理由是 kun 的 compositor 和 Viz 只平移 layer、用不到 compositor 跑 transform 動畫，pre-paint 也不必走 SVG 的子孫；列 2a-2 的決定時發現 `opacity` 也要同樣的取捨（第 23 條），而 hit test、幾何的 API、之後的裁切和效果都要一套和 property tree 平行的機制，改成 P2。第 3 段改的（2026-10-11，第 39 條）：幾何的 API 不經過 property tree，照 Chrome 只更新到 layout，從 layout 的結果算。
18. **T1：`transform` attribute 是 CSS `transform` 的 presentation attribute，SVG 的 origin 固定是 `0 0`**（2026-10-11）：SVG 2 的規定，和第 13 條一致；kun 還沒有 `transform-origin`，SVG 的 layout 用 SVG 子孫在 UA 樣式表的預設值 `0 0`（`transform-box: view-box`），作者寫的 `transform-origin` 不生效，記進限制。不選 T2（同時加 `transform-origin`、`transform-box`）：連 HTML 的 transform 一起改，超出 SVG 的範圍；SVG 的圖形大多沒寫，和 Chrome 一樣。

2026-10-11 確認第 19、20 條（2a-1 開始前，發現第 15 條原本寫的 WPT 跑不了、`visibility` 還沒有之後）。

19. **2a-1 一起做 `CSS` namespace**（2026-10-11）：`CSS.supports(property, value)`、`CSS.supports(conditionText)`、`CSS.escape()`（`dom_api` 的 namespace，Chrome 都有）。能測 presentation attribute 的 WPT `svg/styling/presentation-attributes-*.html`（四份，涵蓋每個 presentation attribute，只替 `CSS.supports` 回 true 的 property 產生子測試）每個子測試都先呼叫它；`css/*/parsing/` 的 `-computed` 那一批也因為它還沒收。不在 SVG 的範圍內，因為驗證要用才放進這一份。另外有 kun 自己的 `style` 和 `getComputedStyle` 測試、Chrome 的實測。不選只用 kun 自己的測試：presentation attribute 的清單和「哪些元素有哪些」是規範的細節，WPT 的四份正好在測；不選另開一份 RFC：namespace 小，`dom_api` 已經有做 namespace 的方式。
20. **2a 一起做 `visibility`，HTML 和 SVG 都生效**（2026-10-11）：`visible`、`hidden`、`collapse`（繼承）；`hidden` 的元素照樣排版，不畫它自己的內容、hit test 打不到，子孫是 `visible` 的照畫（Chrome 一樣）。2a-1 做 property 和 HTML 的 paint、hit test，2a-2 的 SVG paint 跳過 `hidden`。不選 2a 不做（記成限制）：使用者選一起做；不選只對 SVG 生效：和 Chrome 不一致的地方多。
21. **Window 的具名屬性先不做**（2026-10-11，2a-1 做完時）：第 19 條為了 WPT 的 `svg/styling/presentation-attributes-*.html` 加了 `CSS` namespace，跑了才發現它們還用 Window 的具名屬性拿 `<svg id=svg>`（全域的 `svg`），kun 沒有，四份還是跑不了。presentation attribute 由 kun 自己的 `style` 測試照 Chrome 的實測驗證。不選在 2a 一起做：不是 SVG 的功能，要在 bindings 的 global 上加 named property 的 hook；`docs/limitations.md` 記著的其他因為它失敗的 WPT，之後一起處理。
22. **V1：SVG 裡的 `<text>` 和 `foreignObject` 的內容，2a-2 之後先不畫**（2026-10-11，2a-2 開始前）：`<svg>` 變成 replaced element 之後，子孫不再有 CSS 的盒子；原本照一般元素排版的 `<text>` 的字（位置本來就不對）和 `foreignObject` 裡的 HTML 暫時看不到，記進 `docs/limitations.md`。`<text>` 在 2e 畫，`foreignObject` 的繪製是「之後的可能」。Chrome 兩種都畫。不選 V2（2a-2 一起做基本的 `foreignObject`，Chrome 的 `LayoutSVGForeignObject`）：把範圍外的東西拉進 2a-2。
23. **O2：SVG 的 `opacity` 是 effect node、巢狀 `<svg>` 的裁切是 clip node，照 Chrome**（2026-10-11，2a-2 開始前）：和第 17 條同一套 pre-paint。`<g opacity>` 要整個群組畫完再一起變透明，重疊的圖形才和 Chrome 一致。不選 O1（有 `opacity` 的容器整個是一個 display item，用 `SaveLayerAlpha` 包住子孫）：第 17 條改成 P2 之後沒有理由另外做；不選 O3（opacity 乘進每個圖形的顏色）：重疊時和 Chrome 不同。

2026-10-11 確認第 24 到 26 條（2b 開始前）。

24. **K1：SVG 圖形的 hit test 在 `paint` 裡用 `kurbo` 算**（2026-10-11）：fill 照 fill rule 看 winding number，stroke 用 `kurbo` 的 `stroke()` 算出外框（照端點、接合、miter 和虛線）再看；conic 換成三次曲線近似。Chrome 用 Skia 的 `Path::Contains`、`StrokeContains`（Blink 的 main thread link Skia）。不選 K2（`paint` 也 link Skia）：renderer 這一側多一個大依賴，打破 Skia 只在 `raster` 的分層，之後拆 process 也要多帶；hit test 不需要逐像素和 raster 一致。
25. **E1：`pointer-events` 的全部值，HTML 也生效**（2026-10-11）：`auto`、`none`、`visiblePainted`、`visibleFill`、`visibleStroke`、`visible`、`painted`、`fill`、`stroke`、`all`、`bounding-box`（繼承）；SVG 的圖形照各值的規則（`auto` 是 `visiblePainted`），HTML 的盒子只看 `none`（其他值當成 `auto`），和 Chrome 一樣。不選 E2（只有 `auto`、`none`，SVG 一律 `visiblePainted`）、E3（不加 property）：HTML 上的 `pointer-events: none` 很常見（蓋在上面的裝飾、tooltip）。
26. **A1：SVG 的 `<a>` 點了換頁**（2026-10-11）：照 `href`（沒有時 `xlink:href`），`target` 照 HTML 的 `<a>`；沿用 HTML `<a>` 的 `follow_link`（Blink 的 `SVGAElement::DefaultEventHandler`）。Enter 要等 SVG 的子孫能 focus（第 3 段）。不選 A2（留到之後）：打得到圖形之後，點了連結沒反應很明顯。

2026-10-11 確認第 27 條（2c 開始前）。

27. **U2：`<use>` 的複本是 style 算出的 style 和 layout 的實例，不是 DOM 節點**（2026-10-11）：style 重算時，替每個參考得到目標的 `<use>`，用它的 style 當父節點，替被參考的子樹算出複本的 style（selector 照原本的元素比對，Blink 的 corresponding element），存在 `<use>` 上（和 computed style 一樣是 lifecycle 的輸出）；記下每個 `<use>` 參考的子樹，它有變動、或那個 id 換了元素時重算，並讓 layout 重抓那個 `<svg>`。layout 照這些 style 建出複本；hit test 打到複本時回報 `<use>`，事件分派到 `<use>`（Chrome 從複本 retarget 到 `<use>`，看得到的一樣）。和 Chrome 不同、記進限制的：加在原本元素上的 listener 不會因為點到複本觸發（Blink 把它們鏡像到每個複本上），複本沒有自己的 `:hover`、`:active`（`#icon:hover` 不套到複本，`use:hover` 照樣有效）；之後要複本的幾何（`getBBox()`）時要另外對應。不選 U1（照 Chrome 在 `dom` 裡替 `<use>` 建 UA 的 shadow root、放真的 DOM 複本）：要先有 shadow DOM 的地基（shadow root 的節點和 GC edge、事件路徑的 retarget、style 認得 shadow tree），大約是 U2 的兩到三倍，動到 GC 和事件分派的核心，等於在這份 RFC 裡先定 K-1 的設計；K-1 做完後再評估要不要改成 shadow tree。不選 U3（複本用原本元素的 style）：icon sprite 的 `currentColor` 會錯。也不選把 `<use>` 延到 K-1 之後。實作時改的（2026-10-11）：Chrome 的複本在 `<use>` 的 shadow tree 裡，selector 比對到複本的根為止（根沒有 parent 和兄弟，`defs rect` 不比對複本裡的 rect），所以改成照 Chrome，不照原本的元素在文件裡的祖先比對；循環照 Blink 的 `HasCycleUseReferencing`（目標是 `<use>` 自己，或和複本裡的祖先、外面的 `<use>`、文件裡的祖先同 id）；kun 另外限制一個 `<use>` 的複本最多一萬個元素，超過整個不畫（Chrome 沒有上限，互相參考的 `<use>` 會指數地長大，記進限制）。
28. **G1：display list 的漸層照 `cc::PaintShader` 擴充**（2026-10-11）：每個漸層帶 local matrix 和 tile mode（clamp、repeat、mirror），放射的改成一般的 two-point conical（兩個圓心、兩個半徑），帶內插方式（premultiplied 或 unpremultiplied：Blink 的 `Gradient` 對 SVG 和 canvas 用 unpremultiplied，CSS 用 premultiplied）；`PaintShader` 多一種不用 tile 的漸層，背景的 tile 照舊，CSS 背景的橢圓改用 local matrix。Chrome 的 SVG 漸層經過 `PaintShader::MakeLinearGradient`、`MakeTwoPointConicalGradient`，`gradientTransform` 是 local matrix。不選 G2（SVG 專用的漸層 op）：canvas 的漸層要同樣的東西，會有兩套；不選 G3（paint 先畫成圖片）：放大會糊，也和 Chrome 不同。
29. **H1：2d 先做 2d-0（display list 和 raster 的漸層型別），單獨先 merge 進 `main`**（2026-10-11）：和第 14 條一樣，canvas 第 3 段的 `createLinearGradient`、`createRadialGradient` 直接用，不用兩邊各改 `compositor_paint`。canvas 的圓錐漸層（Skia 的 sweep）留給 canvas 加：SVG 用不到，這邊測不到。不選 H2（2d 一次做完，canvas 之後再改）。
30. **I1：`url()` 的 paint server 在 layout 抓 `<svg>` 時解析，失效沿用 `<use>` 的參考追蹤**（2026-10-11）：layout 照 id 找漸層，讀它照 `href` 繼承的 attribute 和 `<stop>` 算好的 `stop-color`、`stop-opacity`；`objectBoundingBox` 在 layout 時用圖形的 tight bbox 換算（Chrome 的 `ObjectBoundingBox()`）。style 替 `<use>` 記參考的機制也記 paint server 的參考（漸層、`href` 一路上的每一個、它們的 `<stop>`），它們改了、或那個 id 換了元素時讓參考的圖形重抓。Chrome 由 `SVGResourceClient` 在 resource 改了時讓 client 失效。不選 I2（layout 自己記每個 `<svg>` 讀過的節點）：和 style 的那一套重複，`<stop>` 的 style 改了要另外接；不選 I3（任何漸層或 `<stop>` 改了整份文件的 `<svg>` 重抓）：大的頁面慢。照 Chrome、不另外決定的：`stop-color`、`stop-opacity` 是 CSS property 也是 presentation attribute；`spreadMethod`；沒有 stop 不畫、一個 stop 是純色；`objectBoundingBox` 的圖形寬或高是 0 時不畫；參考不到 paint server 時用 fallback，沒有就不畫；沒有 `color-interpolation: linearRGB`。
31. **S2：2e 的範圍**（2026-10-11）：`<text>`、`<tspan>`（`<text>` 裡的 SVG `<a>` 當成 tspan，點了換頁）；`x`、`y`、`dx`、`dy`、`rotate` 的清單（每個字元各自的位置）；`text-anchor`；CSS 的字型 property；fill 和 stroke（顏色和漸層）；空白照 Chrome（CSS 的 `white-space` 和 `xml:space`，開始前用 Chrome 152 實測）。不做、記進限制的：`textLength` / `lengthAdjust`、`dominant-baseline` / `alignment-baseline` / `baseline-shift`、直書、`text-decoration`。圖表（d3、React-Stockcharts）常用的是 `x`、`y`、`dy`、`text-anchor`。不選 S1（每個元素一個位置）：多行的 `<tspan dy>`、清單形式的 `x` 會畫錯；不選 S3（再加 `textLength`、各種 baseline）：範圍大很多，基準線還要字型的 x-height 這些度量。Chrome 都有。
32. **G1：display list 的 `DrawGlyphs` 照 cc 的 `DrawTextBlobOp` 帶 `PaintFlags`，先做 2e-0 單獨 merge**（2026-10-11）：fill 或 stroke、shader、alpha 取代 `color`；可選的每個字的旋轉（Skia text blob 的 RSXform，SVG 的 `rotate` 用）。和第 14、29 條一樣，canvas 的 `fillText`、`strokeText` 直接用。不選 G2（SVG 的字轉成路徑用 `DrawPath` 畫）：比較慢，小字的 hinting 和 Chrome 不同；Chrome 畫的是 text blob。
33. **L1：建 box tree 時用 parley 把整個 `<text>` shape 成一行，layout 時照 SVG 2 的文字排版演算法放每個字元**（2026-10-11）：每個 `<tspan>` 是一段 style；layout 時照 `x` / `y` / `dx` / `dy` / `rotate` 放每個 addressable character，絕對位置開始新的 chunk，再照 `text-anchor` 移動。Chrome 也是 LayoutNG 的 inline layout 排成一行，再由 `SvgTextLayoutAlgorithm` 重新放。不選 L2（每個有絕對位置的段落各自 shape）：跨段落的 kerning、連字和 bidi 會和 Chrome 不同。
34. **H1：字的 hit test 和 bbox 用每個字元的格子**（2026-10-11）：advance × 字型的 ascent + descent，照 `rotate` 轉；打到時回報 `<text>` 或那個 `<tspan>`；`objectBoundingBox` 用格子的聯集。Chrome 用每個文字 fragment 的框。不選 H2（整個 `<text>` 一個 bbox）：字之間的空白、多行之間也會打到。照 Chrome、不另外決定的：`pointer-events` 的 fill 和 stroke 對文字都看格子；字型的 fallback 和頁面一樣；`getBBox()`、`getComputedTextLength()` 是第 3 段的事。
35. **C1：`elementsFromPoint` 照 Chrome，不列 SVG 的容器**（2026-10-11，2e 的實測發現）：Chrome 152 對圖形只列它自己和 `<svg>`，不列外面的 `<g>`、巢狀的 `<svg>`、`<a>`、`<use>`（`pointer-events: bounding-box` 的容器除外，那時它在裡面有東西被打到的地方列出自己）；文字只列字元所在的最內層元素，`<text>` 只為它自己的字列出。2b 原本照 WPT 的 `elementsFromPoint-svg.html` 和規範列出容器，當時寫成「照 Chrome」是沒有實測的錯誤。不選 C2（照 WPT 和規範列出容器）：和這個專案「行為照 Chrome」的原則不同，文字的部分 Chrome 和 WPT 一致，圖形的部分只有 WPT 要求。和 Chrome 不同、留著的：`bounding-box` 的容器在 Chrome 是它的 object bounding box 裡都算，kun 只在裡面有東西被打到的地方；`<a>` 自己的字上 Chrome 的 `elementsFromPoint` 什麼都不列，kun 列出 `<a>`（和 `elementFromPoint` 一致）。
36. **S1：第 3 段分 3a、3b、3c，不做 `SVGAnimated*`**（2026-10-11）：3a 點、矩陣、矩形的型別和 `SVGSVGElement` 的 `createSVGPoint()`、`createSVGMatrix()`、`createSVGRect()`；3b 靠 layout 的幾何：`getBBox()`、`getCTM()`、`getScreenCTM()`、SVG 元素的 `getBoundingClientRect()` / `getClientRects()`、SVG 子孫可以 focus（第 26 條等的 Enter 換頁）；3c `SVGGeometryElement` 的 `getTotalLength()`、`getPointAtLength()`、`isPointInFill()`、`isPointInStroke()` 和 `SVGTextContentElement` 的文字方法。各元素的 `SVGAnimated*`（`x.baseVal`、`viewBox.baseVal`、`transform.baseVal`）延續第 5 條，留在「之後的可能」。不選 S2（連同 `SVGAnimated*`）：大約多一倍，還要和 attribute 雙向同步；D3、React-Stockcharts 用到的是 `getBBox()`、`getScreenCTM()`、`createSVGPoint()`。
37. **T1：`SVGMatrix`、`SVGPoint`、`SVGRect` 照 Chrome 是各自的 interface**（2026-10-11）：Chrome 152 的 `SVGMatrix !== DOMMatrix`、`SVGPoint !== DOMPoint`、`SVGRect !== DOMRect`，`createSVGPoint()`、`getCTM()`、`getBBox()`、`getPointAtLength()` 回傳 SVG 的型別。`dom` 的 GC 物件，照 `DOMRect` 的寫法。不選 T2（照規範做完整的 `DOMMatrix` / `DOMPoint`，SVG 的名字是別名）：4×4、字串建構要解析 CSS 的 transform，比較大，而且和 Chrome 不同；canvas 的 `getTransform()` 要 `DOMMatrix` 時再做。
38. **L1：`<defs>` 和沒有被用到的 `<symbol>` 抓成隱藏的容器，算得出幾何**（2026-10-11）：不畫、打不到，但 `getBBox()`、`getCTM()` 和 Chrome 一樣（Blink 的 `LayoutSVGHiddenContainer`；Chrome 152 的 `<defs>` 裡的元素有 bbox，`display: none` 的是 0）。不選 L2（查詢時另外從 DOM 和 style 算）：第二套幾何的程式；不選 L3（沒畫的一律回傳 0）：量 icon 大小的程式會拿到 0。照 Chrome、不另外決定的：幾何經過 `LayoutClient`（先同步 layout），renderer 從 layout 的結果找元素；`getScreenCTM()` 包含頁面上的 CSS transform 和捲動；路徑長度用 kurbo 算（第 43 條改成照 Chrome 的算法）。
39. **L：`getBBox()`、`getCTM()`、`getScreenCTM()` 和 SVG 元素的 client rect 從 layout 的 SVG 樹算**（2026-10-11）：每個節點已經有自己的 `transform` 和 viewport 的變換，外面的 CSS transform 和捲動照現有的 `getClientRects`（走 fragment tree）。Chrome 只更新到 layout，沿著 layout object 換算（`LocalToAbsolute`），不跑 pre-paint。不選 P（先跑 pre-paint、用 property tree，第 17 條原本寫的）：script 每次讀幾何都多跑一步，現有的 CSSOM View 也不這樣。照 Chrome 152 的實測、不另外決定的：`getCTM()` 乘到最近的 viewport 元素（`<svg>`、`<symbol>`，它自己的變換也算；巢狀的 `<svg>` 自己的 `getCTM()` 包含它的 `x`、`y` 和 `viewBox`），最外層的 `<svg>` 是它的 `viewBox`；`getScreenCTM()` 只到自己的 frame 的 viewport，不跨 iframe；`<use>` 的 `x`、`y` 算進 bbox、不算進 `getCTM()`；容器的 bbox 是看得見的子孫的聯集（空的圖形、空的容器、隱藏的容器不算，寬或高是 0 的照算），`visibility: hidden` 的照算；client rect 是 fill 的 bbox 換算過去的一個框，沒有 bbox 的是原點的空框。
40. **C：沒有畫出來的元素照 Chrome**（2026-10-11）：`<g style="display: none">` 也是隱藏的容器（Blink 的 `SVGGElement::LayoutObjectIsNeeded`，第 38 條的延伸），裡面的元素有 bbox；沒有 layout 節點的元素（`display: none`、在 `display: none` 的 `<svg>` 裡、沒有 `<svg>` 的根、不在文件裡）bbox 是 0，自己的變換不算、祖先的照算，沒有 client rect；隱藏的容器裡的元素也沒有 client rect。不選 S（沒畫出來的一律 0 和單位矩陣）：和 Chrome 不同，kun 的 style 本來就算 `display: none` 底下的 style，成本不高。
41. **F1：SVG 元素的 focus 照 Chrome，包括 listener 的特例**（2026-10-11）：有 `tabindex` 的、有連結的 `<a>`，和 Blink 的特例：`SVGGraphicsElement` 有 `focus`、`blur`、`focusin`、`focusout` 的 listener 時也能 `focus()`（`tabIndex` 還是 -1，不在 Tab 的順序裡）；要在 SVG 的 layout 樹裡、不在隱藏的容器裡、`visibility` 看得見。focus 到 SVG 的 `<a>` 時 Enter 換頁（第 26 條留下的）。不選 F2（不做 listener 的特例）：和 Chrome 不同，`dom` 本來就查得到 listener。
42. **O1：SVG 元素的 `outline` 和 focus ring 照 Chrome 畫在元素自己的座標裡**（2026-10-11）：圍住包含 stroke 的範圍，跟著元素的變換轉。不選 O2（不畫，記進限制）：鍵盤 focus 到 SVG 元素時看不出來。
43. **K2：路徑長度照 Chrome 的算法**（2026-10-11，改第 38 條的「路徑長度用 kurbo 算」）：Chrome 152 的 `<circle>` 半徑 10 的 `getTotalLength()` 是 62.427（不是 2πr），`<rect>`、`<circle>`、`<ellipse>`、`<line>`、`<polyline>`、`<polygon>` 經過 Skia 的 `SkContourMeasure`（容差 0.5 把曲線切成線段）；`<path>` 經過 Blink 自己的 `SVGPathQuery`（path data 正規化成三次曲線，細分到控制多邊形和弦差 0.00001，同樣的弧是 62.8408）。kun 照這兩種移植：前者在 `gfx` 照 Skia 的切法（`getPointAtLength()` 也照它在線段上內插 t），後者照 `PathTraversalState`。不選 K1（kurbo 的精確弧長）：簡單，WPT 的容差也過，但圓和 Chrome 差 0.6%，沿著路徑放東西的位置對不上。照 Chrome、不另外決定的：`getTotalLength()` 不看 `pathLength`；`getPointAtLength()` 把長度夾在 0 到總長，空路徑丟 `InvalidStateError`。
44. **U1：文字方法的字元編號照 Chrome 用 UTF-16 的 code unit**（2026-10-11）：`getNumberOfChars()`、`getSubStringLength()`、`get*OfChar()`、`getCharNumAtPosition()`、`selectSubString()` 的索引是空白處理之後的文字的 UTF-16 code unit（"Abc😀d" 是 6 個），一個字形組（emoji 的 surrogate pair、組合字元）的每個 code unit 拿到同一格的位置、寬度只算一次。不選 U2（照 kun 的位置清單用 Unicode 字元）：script 用 `textContent.length`、`indexOf` 算出的索引在 BMP 以外的字元之後會指到別的字。照 Chrome、不另外決定的：超出範圍丟 `IndexSizeError`，`getSubStringLength()` 的長度夾到結尾，`getCharNumAtPosition()` 收 `DOMPointInit`、找不到是 -1。和 Chrome 不同、記進限制的：`selectSubString()` 只檢查範圍（Chrome 會選取那段文字，kun 還沒有 `Selection`）。
45. **G1：沒有畫出來的圖形照 Chrome 算得出長度**（2026-10-11）：`display: none` 的、在 `display: none` 的 `<svg>` 裡的，從它的 style 轉成路徑（百分比當成 0，和 Chrome 152 一樣）；不在文件裡的 `<path>` 照它的 `d` attribute，其他沒有 style 的丟 `InvalidStateError`；`isPointInFill()`、`isPointInStroke()` 要有 layout 節點（隱藏的容器裡的也算），沒有時回傳 `false`；文字方法要畫出來，沒有時字數是 0。不選 G2（只算畫出來的）：和 Chrome 不同，轉路徑的函式本來就在 layout。
46. **S1：第 4 段分 4a、4b、4c**（2026-10-11）：4a XML parser，把 SVG 檔解析成 DOM；4b `<img>` 的 SVG（固有大小、繪製，`<picture>` 的 `<source type="image/svg+xml">`）；4c `background-image` 的 SVG（沒有固有大小時的尺寸、圖塊）。各段開始前再列細節。
47. **X2：XML parser 用 `xml5ever`**（2026-10-11）：和 html5ever 同一個專案、同一個 `markup5ever` 的 `TreeSink`，建 DOM 的方式可以沿用 `html` crate 的。Chrome 用 libxml2（Blink 的 `XMLDocumentParser`）。不選 X1（`roxmltree`，嚴格檢查格式，已經在依賴樹裡，解析完再轉成 DOM）、X3（`quick-xml`，namespace 要自己處理）。`xml5ever` 照 XML5 規範修復錯誤，格式不對的檔案怎麼處理在 4a 決定。
48. **P1：SVG 圖片照 Chrome 的 `SVGImage`，畫成向量的 display list**（2026-10-11）：每個 SVG 圖片是一份不跑 script 的文件，照容器的大小跑 style、layout、paint，攤平成 display list（`paint::to_display_list`），用新的 paint op 讓 raster 照目標的框重播（cc 的 `DrawRecordOp`、`PaintImage` 的 paint record）；和 inline `<svg>` 同一套程式。不選 P2（在 renderer 先畫成點陣圖）：轉了或縮放會糊，renderer 多一條自己 raster 的路；不選 P3（`resvg`）：和 inline SVG 兩套實作（第 2 條）。照 Chrome、不另外決定的：照 response 的 MIME type（`image/svg+xml`）判斷；圖片裡不跑 script、外部資源只認 `data:`、不能互動；動畫不播（kun 還沒有 SMIL 和 CSS 動畫）。
49. **D2：`DOMParser` 的 XML 類型這一段不做**（2026-10-11）：要照 Chrome 做出 `<parsererror>` 的文件，還要 XML 的序列化；留在「之後的可能」。不選 D1（在 4a 一起做）。
50. **頂層導覽到的 SVG 文件不在這份 RFC 做**（2026-10-11）：要 frame 裡的 XML 文件、SVG 的 `<script>`、XML 版的 testharness；WPT 的 `.svg` 測試還是跑不了。
51. **E2：格式錯誤的 XML 照 `xml5ever` 修復後的樹畫**（2026-10-11，4a）：Chrome 152 的 SVG 圖片有任何格式錯誤都是壞圖（libxml2；實測：結束標籤對不上、標籤沒關到檔尾、attribute 值裡的 `<`、`&nbsp;`、重複的 attribute、根元素後面還有元素，都觸發 `onerror`），kun 照 `xml5ever` 的錯誤修復畫出它建的樹，記進限制。不選 E1（`xml5ever` 報的錯一律當成失敗，它不報的三種自己補檢查）：要多一層檢查。照 Chrome、不另外決定的：根元素不是 SVG 的 `<svg>` 時是壞圖（4b）。
52. **N1：DOCTYPE 裡宣告的 entity 照 libxml2 展開**（2026-10-11，4a）：交給 `xml5ever` 之前先過一遍：讀內部宣告的一般 entity（`<!ENTITY name "value">`，不認外部的和 parameter entity），把 DOCTYPE 的內部宣告拿掉，展開內容和 attribute 值裡的引用（註解、CDATA、PI 裡的不展開），展開的總量和深度設上限，防一直展開下去的檔案。Chrome 152 畫得出 `xmlns="&ns_svg;"` 這種 Illustrator 匯出的檔案；`xml5ever` 不讀 DOCTYPE 的內部宣告。不選 N2（不認，這種檔案的 namespace 錯掉，變成壞圖）。
53. **M1：XML parser 在 `html` crate 的 `xml` 模組**（2026-10-11，4a）：和 HTML 的 `DomSink` 共用建節點的部分；XML 沒有 `<template>`、表單、script、quirks。Blink 的對應是 `core/xml/parser` 的 `XMLDocumentParser`。不選 M2（新的 `xml` crate）：多一個要 link 的測試 binary。照 Chrome、不另外決定的：編碼照 XML 的規則（BOM、`<?xml encoding?>`、沒有時 UTF-8），`<?xml ?>` 宣告不是節點；建出來的文件沒有 window、不跑 script。
54. **I2：SVG 圖片的文件放在網頁的 `DomIsolate` 裡**（2026-10-11，4b）：沒有 window、script 拿不到。Chrome 每個 SVG 圖片一個獨立的 Page；kun 的 `DomIsolate` 綁著一個 V8 isolate，另開一個很重。不選 I1（每張 SVG 圖片一個 `DomIsolate`）。
55. **L1：renderer 的 `SvgImage` 跑圖片文件的 style、layout、paint**（2026-10-11，4b）：對應 Chrome 的 `SVGImage`，直接用 style、layout、paint 的介面；照容器的大小排版，攤平成 display list，每個大小一份（`SVGImageForContainer`）；主文件 layout 之後、paint 之前準備好要用的大小。不選 L2（沿用 `FrameView`）：它帶著 script、parser、導覽、模組。照 Chrome 152 的實測、不另外決定的：固有大小只看根 `<svg>` 的 attribute（絕對長度和 `em`，`em` 是 16px；百分比和沒寫的沒有，照 `viewBox` 的比例，`naturalWidth` / `naturalHeight` 補 300×150；CSS 的 `width` 不算）；照 MIME type 判斷；根元素不是 SVG 的 `<svg>` 是壞圖；`<picture>` 的 `<source type="image/svg+xml">` 和 `srcset` 會選 SVG。
56. **Y1：SVG 的 `<style>` 在 4b 一起做**（2026-10-11，4b）：style engine 也讀 SVG namespace 的 `<style>`，inline 的 SVG 和 SVG 圖片都有效（Illustrator、Figma 匯出的檔案用它加 class 上色；Chrome 支援）。不選 Y2（之後再做）。
57. **B1：背景的 SVG 照每個圖塊的大小排版**（2026-10-11，4c；實作時改的：不是 paint 之前由 renderer 算出圖塊的大小，而是 paint 問到還沒畫的大小時記下來，renderer 畫好之後讓問的元素再 paint 一次，`<img>` 也一樣，背景的幾何不必在兩個地方算）：替每個圖塊的大小錄一份；新的 shader 圖源讓 raster 把 record 錄進 picture shader 鋪滿，仍是向量。Chrome 照圖塊的大小（CSS 的 concrete object size）排版、錄成 paint record、用 picture shader 鋪。不選 B2（每張只錄一份，縮放到每個圖塊）：沒有 `width`、`height`、`viewBox` 的 SVG 會被拉伸，只有 `viewBox` 而 `background-size` 拉成別的比例時會變形（Chrome 照 `preserveAspectRatio` 留邊）。照 Chrome、不另外決定的：沒有固有大小時的 `background-size` 照 CSS 的 default sizing algorithm；背景的和 `<img>` 的共用同一份 `SvgImage`。

## 待決定

沒有。

## 之後的可能

- `DOMParser` 的 XML 類型和 XML 的序列化（第 49 條）；頂層導覽到的 SVG 文件（第 50 條）；SVG 的 `<script>`（第 7 條）。
- SMIL 動畫、filter、`clipPath`、`mask`、`pattern`、`marker`。
- 各元素的 `SVGAnimated*`（第 36 條）。
- 文字的 `textLength`、baseline 的 property、直書、`text-decoration`（第 31 條），`textPath`、`foreignObject` 的繪製、SVG 字型。
- `transform-origin`、`transform-box`（第 18 條）。
- `<use>` 的複本改成 shadow tree（第 27 條，等 shadow DOM）。
- SVG 圖片網址的 fragment（`#svgView()`、`<view>`）、`<object>` 和 `<embed>` 的 SVG、圖片裡的動畫。
- `MathMLElement` 和 MathML 的繪製。
