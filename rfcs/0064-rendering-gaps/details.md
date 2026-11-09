# RFC 0064 的細節

## 盤點

2026-10-20，`python3 scripts/ci.py speedometer --inventory`，kun `4a09a2c0`，Chrome for Testing 152.0.7977.82（Chrome 的紀錄這次重錄：2026-10-10 那份比記錄器區分 namespace 還早，SVG 的元素會誤報）。20 個 default suite 在 kun 上都跑得完、沒有例外；12 個非 default 的 suite 沒有用到 default suite 以外的 API。

probe 判斷「kun 丟掉的宣告」：把 Chrome 讀到的宣告設在一個元素的 `style` 上，讀不回來的就是丟掉的。它只看解析，不看畫得對不對：已經解析、但畫法不完整的（例如 `text-decoration` 只有實線）看不到。

丟掉的宣告依類別（括號是用到的 default suite 數）：

- **畫出來會不一樣的（這份 RFC）**：`transform: rotate()`（14）、`text-decoration-style` / `-color` / `-thickness`（16）、`text-shadow`（12）、`clip-path`（11）、`clip`（10）、`filter`（6）、`-webkit-text-fill-color`（6）、`isolation`（4）、`position: sticky`（3）、`-webkit-line-clamp`、`-webkit-box-orient`、`display: -webkit-box`（2）、`object-fit`、`object-position`（2）、`transform-origin`（2）、`contain`（1）、`tab-size`（1）。
- **選取和游標（另一份 RFC）**：`caret-color`（2）；選擇器 `::selection`（2）。
- **只要解析（另一份 RFC）**：`border-image` 和它的 longhand（16，都是初始值）、`cursor`（16）、`font-variant-*` 等字型的 property（14 到 15）、`text-rendering`、`-webkit-font-smoothing`、`user-select`、`forced-color-adjust`、`-webkit-tap-highlight-color`、`resize`、`hyphens`、邏輯的圓角、`-webkit-border-*-spacing`、`font-synthesis-*`、`text-size-adjust`、`scroll-behavior`、`animation-timeline`、`animation-range-*`、`-webkit-user-modify`。
- **DOM 和 Web API（另一份 RFC）**：HTML 元素各自的 interface（14 個 suite）、`XMLHttpRequest`、`window.opener`、`PerformanceObserver` 和 Navigation Timing、Trusted Types、`FileReader`、`navigator.geolocation` / `connection` / `scheduling`、`console.createTask`、`MouseEvent.fromElement`、`document.all`、`document.prerendering`、`IntersectionObserver` 的 v2、runner 的 `navigator.wakeLock`。

## Speedometer 裡用在哪

讀 `../third_party/speedometer/resources/` 裡 suite 實際載入的檔案（`tests.mjs` 的對應）。Complex-DOM 的 suite 各多載入一份約 299 KB 的 Adobe Spectrum 的 CSS（`big-dom.css`，Angular、Preact、Svelte、Lit 的版本多一條 `.tree-area { isolation: isolate }`），頁面有約 3600 個 TreeView 的項目。

- **`transform`**：
  - TodoMVC：`.toggle-all + label:before { content: '❯'; transform: rotate(90deg) }`，每個 TodoMVC 一個，有 todo 時看得到（WebComponents 在 constructable stylesheet、Lit 在 shadow tree 的 CSS 字串）。
  - Spectrum：打開的 TreeView 節點的指示器 `rotate(90deg)`（約 274 個，在左邊看得到的捲動區）；checkbox 的勾 `scale(0)` / `scale(1)`；一些 `translateX(±50%)`。
  - NewsSite：圖示的 `rotate(35deg)`、`rotate(145deg)`、`rotate(±45deg)`；`.animated-icon-inner` 的 `transform-origin: calc(var(--animated-icon-width-half) * var(--language-direction-adjustment)) var(--animated-icon-height-half)`。測試步驟點 `#navbar-dropdown-toggle` 50 次，箭頭的 rotate 反覆切換。
  - Perf-Dashboard：`rotate(180deg)`（展開的按鈕）、`scaleX(-1)`；`rotate(-90deg)` 加 `transform-origin: 0 0` 在不在測試路徑上的頁面。
  - React-Stockcharts：SVG 的 `transform="rotate(c, x, y)"` attribute（kun 已經有）。
  - 沒有 skew 和任何 3D 的函式。
- **`clip`、`clip-path`**：
  - visually-hidden：`clip: rect(0 0 0 0); clip-path: inset(50%); width: 1px; height: 1px; position: absolute`。WebComponents 每個 todo 兩個 label（約 200 個），React-Complex 一個，其他只在編輯模式（測試不進）。NewsSite 也有。
  - React-Stockcharts：`<defs>` 裡每個 chart 一個 `<clipPath id="chart-area-clip-N"><rect/></clipPath>`，series 的 `<g>` 用 inline style 的 `clipPath: url(#chart-area-clip-N)`（`clip: true` 的 series：candlestick、bar、line 等）；另有 `alternating-area-clip-*`、`rsi-clip-*`。
  - Observable Plot 只在設了 `clip` 時用，這個 app 沒設。
- **`text-shadow`**：TodoMVC 的 `.info { text-shadow: 0 1px 0 rgba(255, 255, 255, 0.5) }`，頁尾的說明文字。
- **`filter`**：Spectrum 的 `.spectrum-Popover { filter: var(--spectrum-popover-filter) }`，展開是 `drop-shadow(0px 1px 4px rgba(0, 0, 0, 0.15))`；12 個 popover 都沒打開（`opacity: 0`）。Perf-Dashboard 的 `-webkit-backdrop-filter: blur()` 在 hover 或選取時的浮層。
- **`position: sticky`**：NewsSite 的頁面導覽列（`top: 0`）；CodeMirror 的 `.cm-gutters` 由 JS 設成 sticky（行號欄看得到），`.cm-panels` 只在搜尋面板打開時。
- **line clamp**：NewsSite 的 `.truncate-multiline { display: -webkit-box !important; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden }` 和 `-3`，每篇文章的標題和內文。
- **`object-fit`**：NewsSite 的 `.article-image img { object-fit: cover; object-position: center; width: 100%; height: auto }`，圖片都是 `placeholder_light.jpg`（1280×720），每個 section 幾十張。
- **`::selection`**：CodeMirror 的 `.cm-line ::selection, .cm-line::selection { background-color: transparent !important }`（自己畫選取）；TipTap 的 `.ProseMirror-hideselection *::selection { background: transparent }`（node selection 時）。
- **`text-decoration`**：Spectrum 的 `.spectrum-Calendar-dayOfWeek[title] { text-decoration: underline dotted }`（date picker 裡、隱藏）。看得到的只有 TodoMVC 完成的項目的 `line-through`、CodeMirror 的 `underline`。
- **`-webkit-text-fill-color`、`caret-color`**：Spectrum 的 disabled 文字框（頁面上沒有）；CodeMirror 的 `.cm-content { caret-color: black }` 和 `.cm-line` 的 `transparent`；TipTap 的 `caret-color: transparent`。
- **`isolation: isolate`**：4 個 Complex-DOM 的 `.tree-area`。
- 沒用到：`mask`、`mix-blend-mode`、SVG 的 `<filter>`、`backdrop-filter`（不帶前綴的）。Charts-chartjs 全部畫在 canvas 上。

## 開始前 kun 的現況（2026-10-20）

- **raster**：`raster` 用 Skia（`skia-safe`）。display list（`compositor_paint` 的 `PaintOp`）已經有 `Save` / `Restore`、`ClipRect` / `ClipRRect` / `ClipPath`、`Concat` / `SetMatrix`、`SaveLayerAlpha`、`SaveLayer`（`LayerFlags`：alpha、blend mode、`ImageFilter`；`ImageFilter` 只有 `DropShadow`，canvas 2D 用）。`PaintFlags.blur` 在 Skia 是 `MaskFilter::blur`（`box-shadow` 用）。
- **transform**：`css` 的 `effects.rs` 解析 `translate*`、`scale*`、`matrix`；`TransformOperation` 是 `Translate`、`Scale`、`Matrix([f32; 6])`；下游是 2D 的 `gfx::Transform([f32; 6])`。`layout::box_transform` 的原點固定在中心。paint 的 `PrePaint::transform_node` 建 transform node；hit test 用反矩陣（任意 affine 都可以）；`layerize` 的 `can_be_layer` 要求到 root 的 transform 都是平移。插值（`css::interpolate`）有函式逐項插值和 2D 分解。
- **clip**：paint 的 `ClipNode` 是矩形加四個圓角（overflow clip、SVG viewport）；compositor 的 clip 是軸對齊的矩形加圓角。沒有 `clip`、`clip-path`；SVG 沒有 `<clipPath>`。
- **文字**：`DrawGlyphs` 帶 `PaintFlags`、每個 glyph 的旋轉；裝飾線只有 `text-decoration-line`，用 `style.color` 和字型的底線粗細畫實心的矩形。
- **effect**：`EffectNode` 只有 `opacity`，畫成 `SaveLayerAlpha`；opacity 不成為 compositor 的 layer。
- **捲動**：捲動容器是 scroll translation 的 transform node 加 scroll node，compositor 有自己的 scroll tree，捲動不重畫；`fixed` 掛在 `fixed` 的 transform node。`sticky` 在解析時丟掉。
- **text-overflow**：`ellipsis` 有（`layout` 的 `ellipsis_fragment`，Blink 的 `LineTruncator`），line clamp 可以沿用。
- **replaced element**：`paint` 的 `contents()` 把圖片、SVG 圖片、canvas 撐滿 content box；`DrawImage` 有來源的矩形。
- **選取和游標**：`Selection` API 有，畫面上沒有選取的底色，也沒有游標。pseudo-element 有 `::before`、`::after`、`::marker`、`::placeholder` 和表單控制項的 `::-webkit-*`。

## 和 Chromium 的對照

- **transform**：Blink 的 `TransformPaintPropertyNode` 存 4×4 的矩陣和 origin（`transform-origin` 和 `transform-box`）；`ComputedStyle::ApplyTransform` 照順序套用 translate、rotate、scale、transform 再減掉 origin。cc 的 layer 可以有任意 transform（composited 的 rotate 動畫在 compositor 上跑）。kun 的 compositor 只有平移，非平移的子樹畫進所在的 layer。
- **`clip`**：`PaintPropertyTreeBuilder` 對絕對定位的元素建 `CssClip` 的 clip node，在 overflow clip 之前。
- **`clip-path`**：`ClipPathClipper`：能算成一條 path 的（basic shape、只有形狀的 `<clipPath>`）建 `ClipPathClip` 的 clip node（`ClipPaintPropertyNode` 的 `clip_path`），其他的畫成 mask（`ClipPathMask` 的 effect node，`SkBlendMode::kDstIn`）。hit test 照 path（`ClipPathClipper::HitTest`）。
- **`text-shadow`**：`TextPainter` 照 `TextPaintStyle.shadow` 建 `cc::DrawLooper`，同一個 text blob 照每個陰影的位移和 blur（mask filter）重畫，最後畫本體。
- **裝飾線**：`TextDecorationPainter` 和 `TextDecorationInfo`：`dotted`、`dashed` 是 stroke 加 path effect，`wavy` 是預先算好的 path tile，`double` 畫兩條；粗細照 `text-decoration-thickness`（`auto` 用字型的底線粗細，`from-font`）。
- **`filter`**：`EffectPaintPropertyNode` 有 `filter`（`CompositorFilterOperations`），沒有 composite 時在 paint 時變成 `SaveLayer` 加 `PaintFilter`；有 filter 的元素是 stacking context，也是 fixed 的 containing block。
- **sticky**：layout 算 `StickyPositionScrollingConstraints`（scroll container 的矩形、containing block 的矩形、`top` 等），`PaintPropertyTreeBuilder` 建 sticky translation 的 transform node 帶 `StickyPositionConstraint`，cc 的 `StickyPositionNodeData` 在每次捲動時算位移（`CalculateStickyOffset`），捲動不重畫。
- **line clamp**：LayoutNG 的 block layout 帶 `LineClampData`：`-webkit-line-clamp` 只在 `display: -webkit-box` 加 `-webkit-box-orient: vertical` 時生效，數到第 N 行，第 N 行用 `NGLineTruncator` 放 `…`，之後的內容不佔高度；其他的 `-webkit-box` 照 flexbox 排。
- **`object-fit`**：`LayoutReplaced::ComputeReplacedContentRect` 照 `object-fit` 和 `object-position` 算出內容的矩形，`ReplacedPainter` 裁到 content box。
- **選取**（另一份 RFC，留著給它參考）：`LayoutSelection` 算每個 text fragment 的選取範圍，`HighlightPainter` 畫底色、文字的顏色、裝飾線；`::selection` 的 style 照 highlight inheritance（元素的 `::selection` 繼承 parent 的 `::selection`）。
- **游標**（同上）：`FrameCaret` 和 `CaretDisplayItemClient` 在 paint 時畫一個矩形（`caret-color`），閃爍由 `FrameCaret` 的計時器切換、只重畫游標。
