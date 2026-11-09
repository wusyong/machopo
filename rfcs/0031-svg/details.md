# RFC 0031 的細節：SVG

## 1. 現況（2026-10-10，`main` 的 `2ca5ac82`）

**DOM**：`dom_api` 的 `html_element::element_kind` 只看 HTML namespace，其他 namespace 的元素一律是 `ObjectKind::Element`。parser 照 HTML 的 foreign content 規則建 SVG namespace 的元素（tag 和 attribute 的大小寫照規範調整，例如 `clippath` 變成 `clipPath`），`createElementNS` 也建得出來，但它們只是 `Element`。`HTMLElement` 的 global attribute、`dataset`、`style`、event handler 都宣告在 `HTMLElement` 上（`interfaces/core_html.rs`，函式在 `html_element.rs`、`dataset.rs` 等）。`document.title` 已經分得出 SVG 的 `title`（`dom_api` 的 `title` 測試）。

**繪製**：`style` 的 selector 比對認得 namespace，但 layout 沒有 SVG 的處理：`<svg>` 和它的子孫照 CSS 的預設（`display: inline`）排版，`<text>` 裡的文字照一般的 inline 文字畫，圖形不畫。`raster` 用 Skia，已經會畫路徑（圓角、邊框樣式）。

**圖片**：`raster::image_size` 只解碼 PNG、JPEG、GIF；`<picture>` 的 `<source type="image/svg+xml">` 跳過。

**Speedometer 3.1**（2026-10-10 的盤點，macOS；`rfcs/0012-speedometer/progress.md`）：

| Suite | 在 kun 上 | 用到的 SVG（Chrome 的紀錄） |
| --- | --- | --- |
| TodoMVC-Vue | 一開始停在 `ReferenceError: SVGElement is not defined` | 只有 `container instanceof SVGElement`（Vue 3 的 `app.mount`） |
| NewsSite-Nuxt | `[nuxt] error caught during app initialization ReferenceError: SVGElement is not defined` | `createElementNS`：`svg`、`path`、`title` |
| NewsSite-Next | 停在 `crypto.getRandomValues`（RFC 0028） | `createElementNS`：`svg`、`path`、`title` |
| Charts-observable-plot | 跑完 | `createElementNS`：`svg`、`g`、`path`、`line`、`text`、`rect`、`circle`、`style`、`title` |
| React-Stockcharts-SVG | 停在 `canvas.getContext`（N） | 上面那些，加上 `clipPath`、`defs`、`tspan`、`SVGElement.style`、`onclick=`、`ownerSVGElement`、`SVGSVGElement.createSVGPoint()`、`SVGGraphicsElement.getScreenCTM()`、`SVGPoint.x` / `y` 和 `matrixTransform()`、`SVGMatrix.inverse()` |
| TodoMVC-Lit-Complex-DOM | 停在 `CSSStyleSheet`（K） | `createElementNS`：`svg`、`path`、`title` |

## 2. 整體的選項

### 範圍

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **R1（看法）** | 這份涵蓋路線圖的 O 全部：DOM、inline 的繪製、幾何的 API、SVG 圖片，分四段 | 一份文件說明 SVG 的整體設計，各段共用元素的種類、layout 物件 | 文件大；第 4 段可能離現在很遠 |
| R2 | 只做 DOM 和 inline 的繪製，幾何的 API 和圖片另開 | 每份小 | 幾何的 API 就是 inline 繪製的 layout 結果，拆開要在兩份文件裡描述同一套 layout |

### 繪製的做法

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **G1（看法）** | kun 的 layout 裡有 SVG 的 layout 物件（Blink 的 `LayoutSVGRoot`、`LayoutSVGShape`、`LayoutSVGContainer`、`LayoutSVGText`），paint 交給現有的 display list 和 Skia | script 改 attribute、style 之後照一般的 invalidation 重畫；hit test、事件、`getBBox()`、`getScreenCTM()` 直接用 layout 的結果；cascade 和 HTML 的是同一套（presentation attribute、`:hover`、`currentColor`、CSS 變數） | 要自己寫 SVG 的 layout（座標系統、`viewBox`、文字的位置）；量最大 |
| G2 | inline `<svg>` 的子樹每次變動時轉成 `usvg` 的樹，交給 `resvg` 畫成一張圖，當成 replaced element | 繪製的正確性來自成熟的函式庫，量小 | 每次改動整份轉換；CSS 的 cascade 是 `usvg` 自己的，和文件的樣式表、`:hover` 對不上；hit test 和 `getBBox()` 要再從 `usvg` 的樹算一次；多一個大的依賴 |
| G3 | inline 用 G1，SVG 圖片用 `resvg` | 不用等 kun 的 XML parser（`resvg` 自帶）；圖片不能互動，`resvg` 的弱點（動態、事件）在那裡不成問題 | 兩套繪製，同一張 SVG inline 和當圖片時結果可能不同；圖片的 CSS 是 usvg 的；不支援動畫 |
| G4 | inline 用 G1，SVG 圖片用 Skia 的 SVG 模組（skia-safe 的 `svg` feature，`SkSVGDOM`） | 不用 XML parser；畫的是同一個 Skia | Skia 裡是實驗性的模組，功能比 `resvg` 少（文字、filter、CSS 有限）；不支援動畫；Chrome 自己不用它 |

Chromium、Firefox 都是 G1（圖片也用同一套：Chromium 的 `SVGImage` 是一份獨立的、不跑 script 的文件，照用到的大小快取：`SVGImageForContainer`）。

**SVG 圖片不是靜態的**：HTML 把當成圖片用的 SVG（`<img>`、`background-image` 等）放在 secure animated mode：不跑 script、不載入外部資源（`data:` 除外）、不處理事件（`:hover` 不會觸發），但 SMIL 和 CSS 的動畫照常跑（載入中的轉圈圖示常這樣寫）。另外向量圖照用到的大小和縮放比例重畫，`icons.svg#check` 這類 fragment（`<view>`、`:target`）只顯示一部分，圖片裡的 `@media (prefers-color-scheme: dark)` 跟著系統變。G3、G4 不支援動畫、fragment 和媒體查詢也有限，用在圖片上會和 Chrome 不同；只當成「kun 還沒有 XML parser 時」的過渡，差異要記進 `docs/limitations.md`。

**為什麼 G2 要轉換，fork 也解決不了**：resvg 不在 DOM 上畫。usvg 先用 `roxmltree` 讀進一份唯讀的 XML 樹（`Tree::from_xmltree`），再編譯成簡化的、唯讀的 render tree（解析 CSS、換算單位、展開 `<use>`、圖形和文字都轉成路徑、拿掉看不到的元素、攤平 `<g>`），最後 resvg 用 tiny-skia 畫成 bitmap。活的 DOM 每次改動都要回到前兩步重來。fork 可以讓輸入層吃 kun 的 DOM、CSS 改收 kun 的 computed value、繪製改輸出到 kun 的 display list；但增量更新、每個節點對回 DOM 元素（事件、`getScreenCTM()`）、文字和 HTML 一致，都要改掉 usvg 的核心，等於在 fork 裡重寫，違反「fork 盡量小」。

### 順序

路線圖原本寫「先做 inline `<svg>` 基本圖形的繪製，再做 SVG 的 DOM」。2026-10-10 的盤點之後，DOM 先做的理由：TodoMVC-Vue（M2）和 NewsSite-Nuxt（M5）只缺 `SVGElement` 這個 interface object；第 2 段的 layout 也要先分得出元素的種類。

## 3. 第 1 段：SVG 的 DOM

### 元素

規範定義的 SVG namespace 元素和它們的 interface（WPT 的 `interfaces/SVG.idl`、`svg-paths.idl`、`filter-effects.idl`、`css-masking.idl`、`svg-animations.idl`），共 69 個，加上 `SVGElement`：

- **SVG 2**：`SVGSVGElement`（`svg`）、`SVGGElement`（`g`）、`SVGDefsElement`、`SVGDescElement`、`SVGMetadataElement`、`SVGTitleElement`、`SVGSymbolElement`、`SVGUseElement`、`SVGSwitchElement`、`SVGStyleElement`、`SVGPathElement`、`SVGRectElement`、`SVGCircleElement`、`SVGEllipseElement`、`SVGLineElement`、`SVGPolylineElement`、`SVGPolygonElement`、`SVGTextElement`、`SVGTSpanElement`、`SVGTextPathElement`、`SVGImageElement`、`SVGForeignObjectElement`、`SVGMarkerElement`、`SVGLinearGradientElement`、`SVGRadialGradientElement`、`SVGStopElement`、`SVGPatternElement`、`SVGScriptElement`、`SVGAElement`、`SVGViewElement`；抽象的 `SVGGraphicsElement`、`SVGGeometryElement`、`SVGTextContentElement`、`SVGTextPositioningElement`、`SVGGradientElement`。
- **CSS Masking**：`SVGClipPathElement`（`clipPath`）、`SVGMaskElement`。
- **Filter Effects**：`SVGFilterElement` 和 25 個 `SVGFE*Element`（`feBlend` 到 `feTurbulence`，`feFuncR` 等四個繼承抽象的 `SVGComponentTransferFunctionElement`）。
- **SVG Animations**：`SVGAnimateElement`、`SVGSetElement`、`SVGAnimateMotionElement`、`SVGAnimateTransformElement`、`SVGMPathElement`；抽象的 `SVGAnimationElement`。

SVG 的 tag 名稱分大小寫（`clipPath`、`linearGradient`、`feGaussianBlur`），`svg_tags(...)` 照規範的拼法；HTML 文件裡 parser 已經照規範調整大小寫，`createElementNS` 照呼叫者給的（`createElementNS(svg, "clippath")` 是未知的元素，`SVGElement`；開始時在 Chrome 確認）。

### 選項

| 題目 | 選項 | 看法 |
| --- | --- | --- |
| 元素的範圍 | **A1**：上面每一個都有 interface；A2：只做常用的（`svg`、`g`、基本圖形、`text`、`tspan`、`title`、`defs`、`clipPath`、`style`、`use`、`a`） | A1：查表的成本一樣，Chrome 全部都有；A2 的其他元素會是 `SVGElement`，`instanceof` 和 Chrome 不同，之後還要再補 |
| 第 1 段的成員 | **B1**：`SVGElement` 的成員和共用的 mixin；B2：再加各元素的 `SVGAnimated*` 屬性（`x`、`width`、`viewBox`、`transform`、`href`……） | B1：`SVGAnimatedLength`、`SVGAnimatedTransformList` 這些和 layout 的單位、矩陣有關，第 3 段才有東西可以回答；`className` 是 `SVGAnimatedString`，只反映 `class`，第 1 段就做 |
| 共用 mixin 的位置 | **M1**：`HTMLOrSVGElement`、`ElementCSSInlineStyle`、`GlobalEventHandlers` 的函式從 `html_element.rs` 搬到照 mixin 命名的模組，兩個 interface 各自宣告；M2：`SVGElement` 的宣告直接指到 `html_element.rs` 的函式 | M1：名稱和規範一致；Blink 把這些成員實作在 `Element` 上（`Element::dataset()`、`tabIndex()` 等），`HTMLElement` 和 `SVGElement` 的 IDL 各自 include 那個 mixin。和 RFC 0028 錯開：等它的 global attribute 那一段 merge 之後再搬 |
| SVG 的 `<script>` | 第 1 段一起執行；或之後 | 之後：和 Speedometer 無關，要另外處理 `href`（和 `xlink:href`）、SVG 文件裡的 script 的時機 |

### `SVGElement` 的成員（B1）

| 成員 | 規範 | 實作 |
| --- | --- | --- |
| `className` | SVG 2 | `SVGAnimatedString`（新的 GC 型別，指向元素的 edge，每次同一個）：`baseVal` 讀寫 `class`，`animVal` 等於 `baseVal` |
| `ownerSVGElement` | SVG 2 | 最近的 `<svg>` 祖先，自己是最外層的 `<svg>` 時是 `null` |
| `viewportElement` | SVG 2 | 建立 viewport 的最近的祖先（`svg`、`symbol` 的 instance；第 1 段只有 `svg`） |
| `dataset`、`nonce`、`autofocus`、`tabIndex`、`focus()`、`blur()` | HTML 的 `HTMLOrSVGElement` | 和 `HTMLElement` 共用；SVG 元素的 `tabIndex` 預設值和可以 focus 的條件照 Chrome（第 1 段開始時實測） |
| `style`、`attributeStyleMap` | CSSOM 的 `ElementCSSInlineStyle` | `style` 共用 `element.style`；`attributeStyleMap`（Typed OM）kun 沒有 |
| `onclick` 等 | HTML 的 `GlobalEventHandlers`、`DocumentAndElementEventHandlers` | 共用 |

### 測試的對象

- WPT：`svg/idlharness.window.js`（interface 和原型鏈；第 1 段之後大多失敗在 `SVGAnimated*` 等還沒有的成員，記預期結果）、`svg/historical.html`、`svg/struct/scripted/element-*.html` 的 interface 部分、`html/dom/elements/global-attributes/dataset.html` 的 SVG 兩項、`html/dom/` 的 `HTMLOrSVGElement` 相關測試。
- Speedometer：TodoMVC-Vue、NewsSite-Nuxt 不再停在 `SVGElement`（之後停在哪裡照實記下）。

## 4. usvg、resvg、svgtypes 可以重用的部分

2026-10-10 讀過 crates.io 上的 `usvg` 0.48.1（約 19400 行）、`resvg` 0.48.1（約 4600 行）、`svgtypes` 0.16.1（約 6100 行），三個都是 Apache-2.0 OR MIT。

usvg 是一次性的編譯器：`roxmltree` 讀進的文件先建成它自己的樹（`parser/svgtree/`），`parser/converter.rs` 走一遍，把每個元素轉成簡化的 render tree（`tree/`），途中解析 CSS（`simplecss`）、換算單位、展開 `<use>`、文字排版之後轉成路徑；resvg 用 tiny-skia 畫這棵樹。usvg 的函式大多是 `pub(crate)`，吃它自己的樹節點（`SvgNode`）和轉換的狀態（`converter::State`），不能從外面叫用。但多數函式和那棵樹的耦合只是「讀 attribute、換算單位」（例如 `parser/shapes.rs` 的 `convert_rect`：讀 `x`、`y`、`width`、`height`、`rx`、`ry`，照規範補值和限制，再組路徑），演算法本身可以搬。

### 直接當依賴

| crate | 用在哪裡 | 說明 |
| --- | --- | --- |
| `svgtypes` | SVG 特有的 attribute 語法：path data（`PathParser`，和把相對座標、簡寫、弧線轉成絕對的 `M`/`L`/`C`/`Q`/`Z` 的 `SimplifyingPathParser`）、`transform` 的串列、`points`、`viewBox`、`preserveAspectRatio`、數字和長度的串列、paint（`url(#g) red`）、`paint-order` | 只依賴 `kurbo`（不帶 default feature）和 `siphasher`。CSS 的值（顏色、字型、一般的長度）照舊由 kun 的 `style` 解析，不用它的，cascade 才一致 |
| `kurbo` | 弧線轉曲線、bounding box、路徑長度和路徑上的點（`getTotalLength()`、`getPointAtLength()`、`textPath`） | `svgtypes` 和 usvg 也用它 |

### 移植（照 license 保留出處，改成吃 kun 的 computed value、輸出給 kun 的 layout 物件）

| usvg 的模組 | 行數 | 內容 | 和它的樹的耦合 | 用在 |
| --- | --- | --- | --- | --- |
| `parser/shapes.rs` | 332 | `rect`（`rx`、`ry` 只給一個時補另一個、負值丟掉、上限是寬高的一半、圓角路徑）、`circle`、`ellipse`、`line`、`polyline`、`polygon` 轉路徑 | 只讀 attribute；SVG 2 的 `x`、`width`、`rx` 等是 CSS 的 geometry property，kun 改讀 computed style | 第 2 段 |
| `parser/units.rs` | 145 | 百分比照哪個方向（寬、高、對角線）換算、`objectBoundingBox` 的比例 | 讀 viewport；`em`、`in` 這些 CSS 單位 kun 的 `style` 已經會 | 第 2 段 |
| `tree/geom.rs`、`parser/use_node.rs` 的 `viewbox_transform` | 約 60 | `viewBox` 加 `preserveAspectRatio` 變成矩陣 | 幾乎沒有 | 第 2 段 |
| `parser/style.rs` | 296 | `fill`、`stroke` 的解析和 fallback、`stroke-dasharray` 的正規化（奇數個時重複一次、負值和全 0 時不畫） | 讀 attribute；值改從 kun 的 cascade 來 | 第 2 段 |
| `parser/paint_server.rs` | 1092 | 漸層和 pattern：`href` 一路繼承 attribute（哪些 attribute 只能從同類型繼承）、`stop` 的 offset 和顏色、只有一個 stop 時是純色、`objectBoundingBox` 換成 user space 的矩陣 | `href` 的繼承走它的樹，kun 改成照 id 找 DOM 元素，被參考的元素改變時要 invalidate（kun 自己的部分） | 漸層那一段 |
| `parser/marker.rs` | 489 | `marker` 放在哪些頂點、每個頂點的角度（`orient="auto"`、`auto-start-reverse`、曲線的切線，約 200 行純計算）、`markerUnits` 的縮放 | 角度的計算只吃路徑的 segment，不碰樹 | `marker` 那一段 |
| `parser/clippath.rs`、`parser/mask.rs` | 275 | `clipPathUnits`、`maskUnits` 的矩陣和區域 | 讀 attribute；裁切本身交給 Skia | `clipPath`、`mask` 那一段 |
| `parser/use_node.rs` | 371 | `use` 和 `symbol` 的 viewport 大小、巢狀 `svg` 的裁切區域 | 展開走它的樹；kun 照 Chrome 用 `<use>` 的 shadow tree，只搬大小和裁切的規則 | `use` 那一段 |
| `parser/switch.rs` | 125 | `<switch>`、`systemLanguage`、`requiredExtensions` | 讀 attribute | 第 2 段（小） |
| `parser/text.rs`、`parser/svgtree/text.rs`、`text/layout.rs` 的定位部分 | 約 2700 | 文字分 chunk、巢狀 `tspan` 的逐字 `x`、`y`、`dx`、`dy`、`rotate`、`text-anchor`、`baseline-shift`、`letter-spacing`、`word-spacing`、`lengthAdjust`、`textPath`（沿著路徑放字，用 `kurbo` 的弧長） | 定位吃的是 shape 之後的 glyph cluster；shape（`fontdb`、`harfrust`、`skrifa`）換成 kun 的 parley 和字型，這是改最多的部分 | 文字那一段 |

合計約 5900 行，大約是 usvg 的三分之一，正好是規範裡細節最多的那些。

### 之後才可能用到

| 模組 | 行數 | 說明 |
| --- | --- | --- |
| usvg 的 `parser/filter.rs`、`tree/filter.rs`，resvg 的 `filter/` | 約 5200 | filter 的每個 primitive 在 RGBA 像素上的實作（模糊、光照、turbulence、morphology、convolve、component transfer、color matrix 等）。Chrome 用 Skia 的 image filter；做 filter 時兩者再比較。不在這份的範圍內 |

### 不能用

| 模組 | 行數 | 原因 |
| --- | --- | --- |
| usvg 的 `parser/svgtree/`（`parse.rs`、`mod.rs`、`names.rs`）、`parser/converter.rs` | 約 3800 | 一次性的建樹和轉換流程，G1 用 kun 的 DOM 和 layout tree 取代 |
| usvg 的 CSS（`simplecss`） | | kun 的 cascade 取代 |
| usvg 的 `tree/mod.rs`、`tree/text.rs` | 約 2700 | 簡化過的 render tree：G1 的 layout 物件取代 |
| usvg 的 `writer.rs`、`main.rs`，resvg 的 `main.rs` | 約 2900 | 序列化成 SVG 和命令列工具 |
| usvg 的 `text/flatten.rs`、`text/colr.rs`，`text/layout.rs` 的 shape 部分 | 約 1400 | 字型的載入、shape、把 glyph 轉成路徑：kun 用自己的文字繪製 |
| resvg 的 `render.rs`、`path.rs`、`clip.rs`、`mask.rs`、`image.rs` | 約 700 | 用 tiny-skia 畫；kun 用 Skia |

### kun 自己寫的

元素的 interface（第 1 段）；SVG 的 layout 物件和座標系統；attribute 和 style 改變時的 invalidation（包括被 `href`、`url(#id)` 參考的元素）；presentation attribute 進 kun 的 cascade；畫進 display list；路徑上的 hit test（Skia 的 `contains` 和 stroke 的外框）；幾何的 API；文字接 parley；`<use>` 的 shadow tree。這些都要接到 kun 現有的 replaced element、`style`、paint 和 Skia 上，Chrome 也是這部分自己做。

### 移植的出處

搬過來的檔案在開頭寫出原始的 crate、版本、檔案路徑和 license（Apache-2.0 OR MIT 要保留原作者的版權聲明）。kun 的 repo 目前沒有 LICENSE 檔，也沒有在 `Cargo.toml` 寫 license。
