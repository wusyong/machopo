# RFC 0064：畫面上還缺的 CSS：transform、裁切、文字效果、filter、sticky、line clamp

- 開始日期：2026-10-20
- 狀態：✅ 已完成：2026-10-21
- 進度和驗收：`progress.md`
- 細節：`details.md`（開始前，2026-10-20 的盤點、每一項在 Speedometer 裡用在哪、當時 kun 的現況、和 Chromium 的逐項對照）

## 摘要

Speedometer 3.1 的 20 個 default suite 在 kun 上都跑得完、DOM 的檢查都通過，但頁面用到的 CSS 裡還有一批 kun 丟掉或不畫的：`transform: rotate()`、`transform-origin`、`clip` 和 `clip-path`（含 SVG 的 `<clipPath>`）、`text-shadow`、`text-decoration` 的樣式和顏色、`-webkit-text-fill-color`、`filter`、`isolation`、`position: sticky`、`-webkit-line-clamp`、`object-fit`、`tab-size`、`contain`。kun 少做這些，頁面就少畫、少排，量到的比值偏好看。這份 RFC 把它們照 Blink 的做法補上，分五段做（transform、版面、裁切、文字和效果、sticky），每段開始前決定那一段的細節：transform 的 2D 函式和原點；line clamp、`-webkit-box`、`object-fit`、真正的 `overflow: clip`、`tab-size`（parley fork）、`contain` 和 out-of-flow 盒子的包含塊；`clip`、`clip-path` 和 SVG 的 `<clipPath>`（property tree 上帶路徑的 clip node）；`text-shadow`、裝飾線的樣式和顏色、`-webkit-text-fill-color`；`filter`（effect node 上的 image filter）和 `isolation`；`position: sticky`（layout 之後算限制，compositor 照 cc 移動它的 layer，捲動不重畫）。做的時候也修了一路上找到的幾個原本就有的問題（out-of-flow 盒子的繪製順序、scrollable overflow 的結尾 padding 這些，見 `progress.md`）。

## 動機

2026-10-20 在 `main`（`4a09a2c0`）上重跑 `ci.py speedometer --inventory`（Chrome 的紀錄也重錄）：沒有 suite 再因為缺 API 跑不完，剩下的分三類（`details.md` 的「盤點」）：

- **甲，畫出來會不一樣的 CSS**：這份 RFC。
- **乙，只要解析和 cascade 的 CSS**（`border-image` 的初始值、`font-variant-*`、`cursor`、`user-select` 這類）：另一份 RFC。
- **丙，DOM 和 Web API**（HTML 元素各自的 interface、`XMLHttpRequest`、`PerformanceObserver`、Trusted Types 這類）：另一份 RFC。

甲之所以先做：比較 kun 和 Chrome 的前提是兩邊做一樣的工作。kun 不畫旋轉的箭頭、不裁切 Stockcharts 的 series、不截斷 NewsSite 的標題，量到的就不是同一件事；補上之後 kun 的時間可能變多，比值先變差，但那才是真的差距。看得到、數量多的是（`details.md` 的「Speedometer 裡用在哪」）：

| 功能 | 用在哪 | 看得到的元素 |
| --- | --- | --- |
| `transform: rotate()` | 每個 TodoMVC 的 toggle-all 箭頭；Complex-DOM 的 Spectrum TreeView 打開的節點；NewsSite 的圖示（`transform-origin` 帶 `calc(var())`） | Complex-DOM 每頁約 274 個 |
| `-webkit-line-clamp`、`display: -webkit-box` | NewsSite 每篇文章的標題和內文截成 2、3 行 | 每頁幾十個 |
| `object-fit: cover` | NewsSite 的文章圖片 | 每頁幾十張 |
| `clip-path: url(#…)`、SVG 的 `<clipPath>` | React-Stockcharts-SVG 的 candlestick、bar、line 的 `<g>`（inline style） | 每個 series |
| `position: sticky` | NewsSite 的頁面導覽列；CodeMirror 的行號欄 | 各 1 個 |
| `clip: rect()`、`clip-path: inset(50%)` | visually-hidden 的 label（WebComponents 每個 todo 兩個） | 約 200 個，裁成看不見 |
| `text-shadow` | TodoMVC 頁尾的說明文字 | 幾行 |
| `isolation: isolate` | 4 個 Complex-DOM 的 `.tree-area` | 1 個 |
| `filter: drop-shadow()`、`text-decoration: underline dotted`、`-webkit-text-fill-color` | Spectrum 的 popover、date picker 和 disabled 的文字框 | 在隱藏的元素上 |

目標：
- 上表的每一項照規範和 Chrome 的行為解析、cascade、排版、繪製、hit test，`getComputedStyle` 讀得回來。
- 繪製的結果和 Chrome 的對得上：reftest 和 WPT 的 reftest，畫面的行為寫成情境。
- 新的工作不讓沒用到這些功能的頁面變慢：沒有 `clip-path`、`filter`、sticky 的頁面不多走一步。

**不在範圍內**：
- 3D 的 transform（`rotate3d()`、`perspective`、`transform-style`、`backface-visibility`）和個別的 `rotate`、`scale`、`translate` property：Speedometer 都沒用到，「之後的可能」。
- `mask`、`mix-blend-mode`、`backdrop-filter`、SVG 的 `<filter>` 和 `filter: url()`：Speedometer 的 default suite 沒用到（Perf-Dashboard 的 `-webkit-backdrop-filter` 只在 hover 的浮層），「之後的可能」。
- 選取和游標的繪製（`::selection`、`caret-color`）：新的繪製子系統（每個 text fragment 的選取範圍、只重畫改變的部分、highlight inheritance、游標的閃爍），和「文字編輯」的限制一起設計，另一份 RFC（第 2 條）。Speedometer 裡只有 CodeMirror 和 TipTap 用到，都是設成 `transparent`。
- `hyphens: auto`：要斷字的字典（Blink 在 macOS 用系統的、其他平台另外下載），單獨一份 RFC。
- 乙、丙兩類：各自的 RFC。

解決的 `docs/limitations.md` 裡的限制（已從那份文件刪掉）：
- 「CSS」：支援的 property 那一條的「沒有 `transform-origin`」「沒有 `text-decoration-color` / `-style`」；「陰影和 outline」的「沒有 `text-shadow`」；`position` 那一條的「沒有 `sticky`」；「沒有 `tab-size`」。
- 「圖片」的「沒有 `object-fit`」。
- 「SVG」的「沒有 `clipPath`」（`mask`、`filter` 留著）。
- `<canvas>` 那一條的「沒有 `object-fit`」。

## 設計概要

網頁的作者看到的是：

```css
.toggle-all + label::before { transform: rotate(90deg); }       /* 箭頭轉 90 度，點得到轉過之後的形狀 */
.icon { transform-origin: 0 0; transform: rotate(-90deg) translate(-50%, 0); }
.visually-hidden { clip: rect(0 0 0 0); clip-path: inset(50%); } /* 裁成看不見，也點不到 */
g.series { clip-path: url(#chart-area-clip-1); }                 /* SVG 的 <clipPath> 裁 HTML 或 SVG 的元素 */
.info { text-shadow: 0 1px 0 rgba(255 255 255 / .5); }
abbr { text-decoration: underline dotted red 2px; }
.popover { filter: drop-shadow(0 1px 4px rgb(0 0 0 / .15)); }
.nav { position: sticky; top: 0; }                               /* 捲動時黏在頂端，由 compositor 移動，不重畫 */
.title { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; overflow: hidden; }
img { object-fit: cover; object-position: center; }
```

給之後改這部分程式的人：
- **transform** 仍然是 2D 的 affine（`gfx::Transform`）：`TransformOperation` 多了 rotate 和 skew，原點照 `transform-origin`。平移以外的 transform 照舊畫進所在的 layer（compositor 的 layer 只有平移）。
- **裁切**：paint 的 clip node 除了圓角矩形，還能是一條 path（`clip-path` 的 basic shape、SVG 的 `<clipPath>`）；`clip: rect()` 是一般的矩形 clip node。hit test 照同樣的形狀。
- **效果**：effect node 除了 opacity，多了 filter 的清單，畫成 Skia 的 save layer 加 image filter。
- **sticky** 照 relative 排、不移動；layout 之後為每個 sticky 盒子算出限制（`layout::StickyPositions`），捲動時 main thread 照它算出位移給幾何、hit test 和 pre-paint。pre-paint 給它一個 transform node；捲動它的是 viewport 或成為 layer 的容器、而且能成為 layer 的，compositor 照 cc 每次捲動算位移、移動它自己的 layer，和捲動一樣不重畫，其他的畫在所在的 layer 裡、捲動時重畫。
- **文字**：`DrawGlyphs` 前面多畫陰影（同一份 glyph，帶 blur）；裝飾線有樣式、顏色、粗細。

## 詳細設計

每一項最後的做法；選了什麼、不選什麼和理由在「已決定的事項」，開始前的現況和 Chromium 的對照在 `details.md`。

### A. transform 的 2D 函式和 `transform-origin`（第 6 到 11 條）

- `TransformOperation` 多了 `Rotate`（記下寫的是 `rotate()` 還是 `rotateZ()`）、`Skew`、`SkewX`、`SkewY`，存成自己的 variant，用到時才變成矩陣；角度保留單位，收不帶單位的 `0`。有 3D 的函式時整條宣告無效（第 4 條）。
- `transform-origin`（1 到 3 個值，z 收下不用）、`transform-box`（五個值）進 `rare` 組；`layout::box_transform` 和 SVG 的 `element_transform` 照 reference box 和 origin 算（Blink 的 `TransformHelper::ComputeReferenceBox`），UA stylesheet 給 SVG 裡的元素 `transform-origin: 0 0`。`-webkit-transform`、`-webkit-transform-origin` 是別名。
- 插值照 Blink：同一個 primitive 逐項插值、剩下的併成矩陣，有百分比時存成 `Interpolated`，拿到 box 的大小才解。
- 平移以外的 transform 畫進所在的 layer（compositor 的 layer 只有平移）。

### F. 版面：line clamp、`object-fit`、`tab-size`、`contain`（第 12 到 28 條）

- **line clamp**：只認 `-webkit-box` / `-webkit-inline-box` 加 `-webkit-box-orient: vertical` 加 `-webkit-line-clamp: N`。style 照 Blink 把 `display` 換成 `flow-root`（`inline-block`）、記下原本寫的是 `-webkit-box`；kun 包住 taffy 的那一層在容器排完之後，照文件順序數它的 block formatting context 裡的行，第 N 行放 `…`、之後的照排不畫（Blink 的 `IsHiddenForPaint`），容器的 auto 高度到第 N 行的底。`display: flow-root` 因此是一般的值。沒有 clamp 的 `-webkit-box` 照 Chrome 對到 flex，`-webkit-box-*` 是各自的 longhand。
- **`object-fit`、`object-position`**：replaced element 的內容照它們放（`object_rect`）；UA stylesheet 照 Chrome 給 replaced element `overflow: clip`，超出 content box 的部分裁掉。kun 因此有了真正的 `overflow: clip`：會裁但不是捲動容器。
- **`tab-size`**：parley fork 照 tab 在行裡的位置算到下一個 tab stop，tab stop 的間隔和最小前進量由 kun 照 block container 的空白算好交給它（`TabStops`）。
- **`contain`**：照規範全部做；size containment 在 kun 包住 taffy 的那一層把內容當成 0（沒有 `contain-intrinsic-size`）。`transform`、`contain: layout` / `paint`、`filter`（根元素的除外）是 absolute 和 fixed 的包含塊（Blink 的 `ComputeIsFixedContainer`），layout 把「固定在 viewport」記在 fragment 上。

### B. 裁切：`clip`、`clip-path`、SVG 的 `<clipPath>`（第 29 到 35 條）

- paint 的 `ClipNode` 除了圓角矩形，多一組可有可無的路徑（聯集），矩形是它們的外框（Blink 的 `ClipPaintPropertyNode` 的 `ClipPath`）：只要範圍的地方照舊讀矩形，display list 多 `ClipPath` / `ClipPaths`，hit test 照路徑（kurbo）。
- `clip-path` 的 basic shape（`inset()`、`circle()`、`ellipse()`、`polygon()`、`path()`、`rect()`、`xywh()`）、reference box、只寫 box，和 `url(#id)` 指向 `<clipPath>`：它的形狀子元素各自帶 `transform` 和 `clip-rule`，幾個時取聯集（raster 用 Skia 的 PathOps），裡面有 `<text>` 或 `<use>` 時不裁。`clip: rect()` 是絕對定位盒子上的矩形 clip node（Blink 的 `CssClip`）。兩者照 Chrome 插值。
- 路徑裁切底下不成為 compositor 的 layer，畫進所在的 layer。

### C. 文字：`text-shadow`、裝飾線、`-webkit-text-fill-color`（第 37 到 39 條）

- `text-shadow`：每個陰影多畫一次同樣的 glyph，帶位移和 blur（Chromium 的 `cc::DrawLooper`），範圍算進 visual rect。
- 裝飾線：style 給每段文字一份產生裝飾的盒子的清單（`applied_text_decorations`：各自的線、`text-decoration-style`、`-color`、`-thickness`、`text-underline-offset`，Blink 的 `AppliedTextDecoration`），`text-underline-position` 繼承；paint 的 `text_decoration` 模組照 Blink 的 `TextDecorationOffset`、`DecorationLinePainter` 在 device pixel 裡放每條線，畫的順序照 Blink 的 `PaintWithTextShadow`。
- `-webkit-text-fill-color` 取代 `color` 當文字的填色。

### D. 效果：`filter`、`isolation`（第 40 到 43 條）

- `filter` 的十個函式可以串接（不做 `url()`），照 Blink 的 `FilterInterpolationFunctions` 插值。
- `EffectNode` 多了 filter（`ImageFilter` 的 blur、顏色矩陣、drop shadow 和一串，照 cc 的 `RenderSurfaceFilters`），記下 filter 所在的空間和結果的 output clip（建在元素的 `clip`、`clip-path` 之後），和 opacity 畫成同一個 `SaveLayer`；raster invalidation 算進 filter 往外擴的範圍。
- `filter`、`isolation: isolate` 建立 stacking context；filter 底下不成為 compositor 的 layer。

### E. `position: sticky`（第 44 到 48 條）

- **style 和 layout**：`sticky`（和 `-webkit-sticky`）是定位的盒子、在流裡、建立 stacking context；layout 照 relative 排、不移動，只改 inset 只換 fragment。
- **限制**：fragment 記下底下有沒有 sticky 的盒子（`sticky_below`）。layout 之後，`StickyPositions` 從根沿著它們往下走一次，照 Blink 的 `ComputeStickyPositionConstraints` 為每個盒子記下它黏在哪個捲動容器或 viewport 裡、那個容器內容座標裡的 constraining rect（scrollport 扣掉 padding）、盒子和 containing block（content box 扣掉自己的 margin，containing block 是捲動容器自己時是它的 scrollable overflow；匿名的盒子跳過）的矩形、inset（百分比照 scrollport，兩個放不下時結尾那一側讓）、巢狀時外層的 sticky；表格的部分不黏。renderer 的 `ScrollState` 在每次 layout 之後重建這張表，每次捲動照它重算位移（`ComputeStickyOffset`），不走 fragment tree。
- **main thread 的幾何**：script 的幾何（client rect、`offsetTop`、SVG 元素的幾何）加上位移；`getComputedStyle` 的 inset 照 scrollport 解析、`auto` 保留。pre-paint 給 sticky 盒子一個 transform node，走完之後就地設位移，連同盒子的位置一起對齊 device pixel；hit test 走同一份 property tree。
- **compositor**：捲動它的是主 frame 的 viewport 或成為 layer 的容器、而且能成為 layer 的 sticky（`composited_sticky`），內容自成 layer（在它的空間，大小只被它裡面的 clip 裁），畫的時候位移歸零；compositor 的 transform node 帶 layout 的限制（cc 的 `StickyPositionNodeData`），每次捲動算位移（主 frame 的 viewport 的位移以 `ElementId::VIEWPORT` 給），debug build 和 layout 的比對。其他的 sticky 畫在所在的 layer、位移變了就重畫。滾輪的範圍在 sticky 的空間裡跟著它移動；sticky 可能移到成為 layer 的容器上時，它能被打到的範圍是要 main thread 的範圍，容器不算被蓋住。

### 測試

每一段都有：`css` 的解析和序列化、`style` 的 cascade 和 `getComputedStyle`、插值的單元測試；layout、paint、compositor 的整合測試（hit test、layer、raster invalidation、增量：只重畫的改變不重排，debug 的交叉檢查照舊）；kun 自己的 reftest；WPT 的對應目錄用 `--all` 試過，通過的和失敗原因是 kun 沒有的功能的加進清單；每一段一份情境（轉過的箭頭、line clamp、`<clipPath>`、文字效果和 filter、sticky）；每一小段的 mutation test（`mutants-*.toml`）。結果在 `progress.md`。

### 分階段

照第 5 條，每段開始前決定那一段的細節，各自做完、經過審核就先 merge 回 `main`：

1. **transform**（A）。
2. **版面**（F）：2a `object-fit`、`object-position` 和 `overflow: clip`，2b `tab-size`，2c line clamp 和 `-webkit-box`，2d `contain` 和包含塊。
3. **裁切**（B）：3a `clip`、`clip-path` 的形狀和 box，3b SVG 的 `<clipPath>`、`url()`、`clip-rule`。
4. **文字和效果**（C、D）：4a 文字，4b `filter` 和 `isolation`。
5. **sticky**（E）：5a main thread 上的 sticky，5b compositor 移動 sticky 的 layer。

## 缺點

- **範圍大**：五段橫跨 `css`、`style`、`layout`、`paint`、`compositor`、`raster`，每段都要各自的 WPT 和 reftest。
- **快取和增量多了輸入**：新的 property 要照 `docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」改 `StyleDifference`、paint 的比對，漏了是畫面不更新。
- **非平移的 transform、path clip 和 filter 不成為 layer**：在它們底下的動畫、canvas、捲動容器和 sticky 退回畫進頁面，捲動或動畫時在 main thread 重畫。
- **kun 的時間會變多**：多畫陰影、多一層 save layer、多排 line clamp，Speedometer 的比值可能先變差。

## 理由和其他做法

- **為什麼先做甲**：量測的前提是兩邊做一樣的工作；乙、丙不改畫面（丙的 API 只改框架走哪條路）。
- **為什麼一份 RFC，不是每項一份**：每一項都小，各自一份 RFC 的流程成本比實作大；它們的共同點是「補上 Blink 有的繪製和排版」，用分階段隔開，一段做完就能先 merge。
- **為什麼不做 3D**：Speedometer 沒用到，而 4×4 要改整條 pipeline（`gfx::Transform`、property tree、compositor、hit test）。
- **不做**：比值裡混著 kun 少做的工作，優化的方向會被誤導（例如以為 NewsSite 的排版已經接近 Chrome）。

## 先例

`details.md` 的「和 Chromium 的對照」逐項列出。
- **Blink**：`TransformPaintPropertyNode`（`transform-origin`）、`ClipPathClipper`（path-based 和 mask-based）、`CssClip` 節點、`cc::DrawLooper`（文字陰影）、`TextDecorationPainter`、`EffectPaintPropertyNode` 的 filter、`StickyPositionScrollingConstraints`、LayoutNG 的 line clamp、`LayoutReplaced::ComputeReplacedContentRect`。
- **Stylo / WebRender**（Firefox）：sticky 是 WebRender 的 spatial node（`StickyFrame`），在 compositor 上算；`clip-path` 能算成形狀的走 clip chain，其他畫成 mask。

## 已決定的事項

1. **一份 RFC，分階段做**（2026-10-20，使用者）：每一項都小，各一份 RFC 的流程成本比實作大；它們改的是同一批地方（`css` 的白名單、`ComputedStyle`、`StyleDifference`、paint 的 property tree），拆開平行做衝突多。每段做完、經過審核可以先 merge 回 `main`。不選每項各一份。
2. **選取和游標另開一份 RFC**（2026-10-20，使用者）：它不是補一個 CSS property，是新的繪製子系統（Blink 的 `LayoutSelection`、`HighlightPainter`、`FrameCaret`，在 `core/editing`），和「文字編輯」的限制（游標、輸入法、undo）一起設計比較好；閃爍和游標的位置要請人看，其他段都能自動驗證；對 Speedometer 的量測幾乎沒有影響（CodeMirror、TipTap 都設成 `transparent`）。這份因此是五段。不選放進來當第六段。
3. **`contain`、`tab-size`、`isolation` 放進來**（2026-10-20，使用者）：它們都改變版面或畫面，不是只要解析；`contain` 的 paint containment 是 overflow clip 加 stacking context，和這份的裁切、效果是同一批程式。`contain` 照規範全部做（`size`、`inline-size`、`layout`、`paint`、`style`、`strict`、`content`），不只 CodeMirror 用到的 `size style`：各種 containment 的判斷在同一批地方。`contain`、`tab-size` 在第 2 段，`isolation` 在第 4 段（和 `filter` 一樣建立 stacking context）。不選歸到「只要解析」的那份。
4. **不在範圍內的都不做，記進「之後的可能」**（2026-10-20，使用者）：3D 的 transform、個別的 `rotate` / `scale` / `translate` property、`mask`、`mix-blend-mode`、`backdrop-filter`、SVG 的 `<filter>` 和 `filter: url()`、`hyphens: auto`。Speedometer 的 default suite 都沒用到，或只在測試不觸發的地方用到。`mask`、`mix-blend-mode`、`backdrop-filter` 和 mask-based 的 `clip-path` 用同一套 save layer 加合成的機制，之後一起設計；3D 和 RFC 0049 的 compositor 的矩陣一起做。不選把個別的 transform property 放進第 1 段。
5. **分五段，順序是 transform、版面、裁切、文字和效果、sticky**（2026-10-20，使用者）：看得到的多、改動小的先做，最後才動 compositor；文字和效果在 Speedometer 裡大多看不到，但都是 paint 這一層的小改動，放在 sticky 之前做完。每段做完可以先 merge。不選裁切接在 transform 後面（NewsSite 看得到的 line clamp、`object-fit` 會晚一段），也不選照看得到的數量把 sticky 放在文字和效果前面（sticky 風險最大，會讓最後一段等它）。
6. **第 1 段：rotate、skew 存成自己的 variant（T1 甲）**（2026-10-20，使用者）：`TransformOperation` 加 `Rotate`（角度），和 `Skew`（x、y 的角度）、`SkewX`、`SkewY`，`transform_matrix` 用到時才變成矩陣。skew 分三種是實作時從 WPT 的 `transform-interpolation-verify-reftests` 發現的：Chrome 的 `skewX()`、`skewY()`、`skew()` 各自只和同一種逐項插值（`skewX(0) rotate(0)` 到 `skewY(0) rotate(360deg)` 整個併成矩陣，一半時是 identity）。插值照角度：`rotate(0deg)` 到 `rotate(360deg)` 轉一整圈，`rotate(0)` 到 `rotate(270deg)` 照正方向轉（規範的要求；Spectrum 的 TreeView 指示器在 `rotate(0)` 和 `rotate(90deg)` 之間有 transition）。Chromium 的 `RotateTransformOperation`、`SkewTransformOperation` 也是各自一種。不選在 computed value 時併成 `Matrix`（矩陣分解會讓 0 到 360 度不動、270 度往反方向轉）。
7. **第 1 段：函式的範圍（T2）**（2026-10-20，使用者）：`rotate()`、`skew()`、`skewX()`、`skewY()`，加上效果和 `rotate()` 一樣的 `rotateZ()`（`Rotate` 記下寫的是哪個，序列化照寫的；兩者逐項插值）。角度收不帶單位的 `0`（css-transforms-1 允許，specified value 寫成 `0deg`），specified value 保留角度的單位（`deg`、`rad`、`grad`、`turn`、`calc()`），和 Chrome 一樣。其他 3D 的函式照第 4 條不做：清單裡有一個，整條宣告就無效；`translate3d(0, 0, 0)` 這種強制成為 layer 的寫法因此會連同同一條的 `rotate()` 一起丟掉，記進限制（Speedometer 沒有這樣寫）。
8. **第 1 段：`transform-origin`（T3）**（2026-10-20，使用者）：照規範的語法（1 到 3 個值、關鍵字可以對調、`calc()`、`var()`），初始值 `50% 50% 0`、不繼承；computed value 是兩個 `LengthPercentage` 加 z 的長度，z 收下、序列化，不參與 2D 的矩陣。`layout::box_transform` 照它（Blink 的 `ComputedStyle::ApplyTransform`），取代寫死的中心。`getComputedStyle` 照 Chrome：有 box 時回 px、z 是 0 時不寫，specified value 保留關鍵字。逐項插值。多數元素是初始值，放進 `rare` 組。一起加 Chrome 的別名 `-webkit-transform`、`-webkit-transform-origin`。
9. **第 1 段：`transform-box`（T4 甲）**（2026-10-20，使用者）：加 `transform-box` 的五個值（初始值 `view-box`）。CSS 的 box：`fill-box` 當成 `content-box`，`stroke-box`、`view-box` 當成 `border-box`；SVG 裡的元素：`content-box`、`fill-box` 是 object bounding box，`border-box`、`stroke-box` 是 stroke bounding box，`view-box` 是最近的 SVG viewport。UA stylesheet 照 Chrome 的 `svg.css` 給 SVG 裡的元素 `transform-origin: 0 0`，所以預設的畫面不變。`box_transform` 和 SVG 的 `element_transform` 都照 reference box 和 origin 算（Blink 的 `TransformHelper::ComputeReferenceBox`）。stroke bounding box 和 Chrome 一樣是近似值，斜接角可能差一點，記進限制。不選不加 `transform-box`（SVG 元素的 `transform-origin` 會沒有作用）。
10. **第 1 段：插值照 Blink（T5 乙）**（2026-10-20，使用者）：同一個 primitive 算對得上（`rotate` 和 `rotateZ`）；共同的前綴逐項插值，剩下的部分併成矩陣，短的那邊補 identity（`TransformOperations::MatchingPrefixLength`）；要併成矩陣但有百分比時，存成 `TransformOperation::Interpolated { from, to, progress }`，`transform_matrix` 拿到 box 的大小時才解（`InterpolatedTransformOperation`）。不選只做前兩項、百分比時照舊跳到終點（會留下動畫會跳的限制）。
11. **第 1 段：測試（T6）**（2026-10-20，使用者）：解析和序列化（`css` 的單元測試，WPT 的 `css/css-transforms/parsing/` 的 `transform-*`、`transform-origin-*`、`transform-box-*`、`webkit-transform-valid.tentative`）；`getComputedStyle`（`style`、`shell` 的測試）；插值（`css` 的 `interpolate` 單元測試，WPT 的 `css/css-transforms/animation/` 的 `transform-interpolation-*`、`rotate-interpolation`、`list-interpolation`、`transform-origin-interpolation`，3D 的子測試標成預期失敗）；幾何和 hit test（`shell` 的整合測試，含 SVG 的 `transform-box: fill-box`）；畫面（kun 自己的 reftest 和 WPT 的 `css/css-transforms/` 裡用到的功能 kun 都有的 reftest，含捲動容器裡轉過的元素）；改 `transform-origin`、`transform-box` 只重畫不重排；一份情境（點轉出去的那一角、transition 的截圖）；mutation test（`mutants-1.toml`）。
12. **第 2 段：line clamp 只做 `-webkit-` 的組合（V1，F1）**（2026-10-20，使用者）：`display: -webkit-box` 或 `-webkit-inline-box` 加 `-webkit-box-orient: vertical` 加 `-webkit-line-clamp: N` 時，容器當成 block container 排到第 N 行為止。Chrome 出貨的是這個，Speedometer（NewsSite）和現實的網頁用的也是；標準的 `line-clamp`（`max-lines`、`block-ellipsis`、`continue`）Chrome 還沒出貨、規範還在改，記進「之後的可能」，之後只是多一個入口。不選 F2。
13. **第 2 段：行數照 Blink 跨過子 block 一起數（V2 甲）**（2026-10-20，使用者）：clamp 的容器的 block formatting context 裡，子孫的行照順序一起數（Blink 的 `LineClampData`），第 N 行的結尾放 `…`（沿用 `text-overflow: ellipsis` 的截斷，不溢出也放），之後的行和 block 不排、不佔高度（怎麼不排見第 23 條：照排、不畫）。不選只數容器自己的行。
14. **第 2 段：沒有 clamp 的 `-webkit-box` 照 Chrome 當成 flex（V3 乙）**（2026-10-20，使用者）：`-webkit-box-orient`、`-webkit-box-direction`、`-webkit-box-pack`、`-webkit-box-align`、`-webkit-box-flex`、`-webkit-box-ordinal-group` 都照 Chrome 對到 flex 的方向、`justify-content`、`align-items`、`flex-grow`、`order`。不選只認 `-webkit-box-orient`。
15. **第 2 段：`object-fit` 超出的部分照 Chrome 裁掉，做真正的 `overflow: clip`（V4，2026-10-20 改寫）**（2026-10-20，使用者）：UA stylesheet 照 Chrome 給 replaced element（`img`、`video`、`canvas`、`embed`、`object`）`overflow: clip`，它的內容（`object-fit: cover`、`none`、`object-position` 超出的部分）裁到 content box（Blink 的 `ReplacedContentClip`，只在內容超出或有圓角時建裁切的節點）；作者寫 `overflow: visible` 才露出來。`object-fit` 用在 `<img>`、`<canvas>`、SVG 的圖片，natural size 帶到 fragment（`object_rect`）。做的時候發現 kun 把 `overflow: clip` 算成 `hidden`（捲動容器），直接加 UA 規則會讓每張圖都成為捲動容器（多一個 scroll node、flex item 的最小大小變 0），所以改成做真正的 `overflow: clip`（使用者選甲）：computed value 多 `Clip`，會裁切但不是捲動容器（不建 scroll node、script 也捲不動、不是 block formatting context、flex 和 grid item 的自動最小大小照內容、inline-block 的 baseline 照內容，Blink 的 `IsOverflowVisibleOrClip`），在一個會捲動的軸旁邊算成 `hidden`，和 `visible` 一起時只裁一個軸；傳到 viewport 時是 `hidden`。沒有 `overflow-clip-margin` 這個 property：一般的盒子裁在 padding box，replaced element 裁在 content box（Chrome 的 UA 給的值）。不選一律裁到 content box、不碰 overflow（乙：作者的 `overflow: visible` 沒有作用）。
16. **第 2 段：`tab-size` 改 parley fork（V5 甲）**（2026-10-20，使用者）：斷行時照 tab 在行裡的位置算它的寬度，到下一個 tab stop（`<number>` 是空白的 advance 加 `letter-spacing`、`word-spacing` 的倍數，或 `<length>`；Blink 的 `ShapeResult::CreateForTabulationCharacters` 在 `NGLineBreaker` 裡）。fork 的改動照 `CLAUDE.md` 的規則：程式裡標 `kun:`、記進 fork 的 `README.machopo.md`、更新 `third_party.toml`，push fork 前先問。不選 kun 這邊把 tab 換成固定寬度的 inline box（寬度要看位置，`pre-wrap` 會換行，做不對）。做的時候定下（2026-10-20）：`<number>` 乘的是 block container 的空白（它的字型的空白加它的 `letter-spacing`、`word-spacing`），tab 自己的 `tab-size` 只決定乘幾個，和 Chrome 一樣（WPT 的 `tab-size-block-ancestor.html`、`tab-size-integer-004.html`）；所以 kun 算好 tab stop 的間隔和最小的前進量（block 的空白的一半）交給 parley（`TabStops`），fork 不必知道 block 是誰，也不必替每個 run 量空白。tab stop 從 block 的內容邊緣算（縮排和 float 讓出的位置都算在 tab 的位置裡）。fork 的 shaper 把 tab 當成空白 shape：多數字型沒有 U+0009 的字形，原本畫出缺字的方框，Chrome 的 tab 不畫東西。
17. **第 2 段：`contain` 照規範，不做 `contain-intrinsic-size`（V6）**（2026-10-20，使用者）：`size`、`inline-size`（內在大小當成沒有內容：block、flex、grid、table、`inline-block`、replaced 各自的路）、`layout`（獨立的 formatting context、絕對定位和 fixed 的包含塊、stacking context、不給 baseline）、`paint`（`layout` 的效果加上裁到 padding box，不成為捲動容器）、`style`（counter 的範圍；kun 沒有 quote），`strict`、`content` 是組合；不適用的盒子（非 atomic 的 inline、table 的內部）照規範沒有作用。`contain-intrinsic-size` Speedometer 沒用到，記進「之後的可能」：沒有它時 size containment 照規範當成 `none`（大小 0）。
18. **第 2 段：fixed 的包含塊一起做（V7；2026-10-20 第 27 條擴大成 absolute 也接）**（2026-10-20，使用者）：照 Blink 的 `CanContainFixedPositionObjects`，有 `transform`、`contain: layout` / `paint` 的祖先接住 `position: fixed` 的子孫（之後 `filter` 也用同一套），layout 照那個祖先排，paint 的 fixed 不再掛在 viewport 上、跟著那個祖先捲動。順便解決限制裡「有 `transform` 的祖先不會變成 fixed 的包含塊」那一條。不選 `contain` 不接 fixed、記成限制。
19. **第 2 段分四小段（V8）**（2026-10-20，使用者）：2a `object-fit`、`object-position`；2b `tab-size`（parley fork）；2c line clamp 和 `-webkit-box`；2d `contain` 和 fixed 的包含塊。每小段各自 commit，階段的檢查在 2d 做完時跑。
20. **第 2 段：測試（V9）**（2026-10-20，使用者）：每一項的解析和序列化（`style` 的 `cssom::`、`computed::`）；排版（`layout` 的整合測試：clamp 的高度和第 N 行的 `…`、跨子 block 數行、size containment 的內在大小、包含塊）；繪製（`paint` 的 `object-fit` 畫的矩形和裁切、paint containment 的 clip）；kun 自己的 reftest；WPT 的 `css/css-overflow/line-clamp/` 的 `webkit-line-clamp-*`、`css/css-images/` 的 `object-fit*`、`object-position*`、`css/css-text/tab-size/`、`css/css-contain/` 的 `contain-*`、fixed 包含塊的 `css/css-transforms/`、`css/css-position/` 的測試，用 `--all` 試過、挑通過大半的；一份情境（clamp 的標題和 `cover` 的圖片、點 clamp 掉的地方）；每小段的 mutation test（`mutants-2a.toml` 到 `mutants-2d.toml`）。Speedometer 不排量測。
21. **第 2c 段：`display: -webkit-box` / `-webkit-inline-box` 是自己的值（W1 甲）**（2026-10-20，使用者）：`Display` 多 `WebkitBox`、`WebkitInlineBox`，`getComputedStyle` 讀回 `-webkit-box`（clamp 的組合除外，見第 25 條），`-webkit-inline-box` blockify 成 `-webkit-box`（Chrome 的 `EDisplay::kWebkitBox`、`kWebkitInlineBox`）；layout 看組合：`-webkit-box-orient: vertical` 加 `-webkit-line-clamp` 是建立 block formatting context 的 block container（第 13 條），否則交給 taffy 的 flex（第 14 條）。不選解析時換成 `flex` / `inline-flex`（`getComputedStyle` 和 `display` 的序列化和 Chrome 不同，clamp 的組合要另外記）。
22. **第 2c 段：`-webkit-box-*` 是各自的 longhand（W2 甲）**（2026-10-20，使用者）：`-webkit-box-orient`、`-webkit-box-direction`、`-webkit-box-pack`、`-webkit-box-align`、`-webkit-box-flex`、`-webkit-box-ordinal-group` 是獨立的 property（不繼承，放進 `rare` 組），`getComputedStyle` 讀得到各自的值；只有 `-webkit-box` 的容器和它的子盒子在換成 taffy style 時讀它們（`orient` 加 `direction` 是方向，`pack` 是 `justify-content`、`justify` 是 `space-between`，`align` 是 `align-items`，`-webkit-box-flex` 是 grow 和 shrink，`ordinal-group` 是 `order`），和一般 flex 不同的地方（子盒子沒有自動的最小大小、不換行、不看子盒子的 `align-self`，水平的 box 裡寬度 `auto` 的子盒子是 fit-content，做的時候從 Blink 的原始碼找到的）照 Blink 的 flex 演算法對 `-webkit-box` 的特例，用 WPT 的 `compat/webkit-box-*` 對照。Chrome 的 `ComputedStyle::BoxPack()`、`BoxFlex()` 這些。不選當成 flex property 的別名（寫在一般 flex item 上也會生效，`getComputedStyle` 讀不回，對應不是一對一）。
23. **第 2c 段：clamp 之後的內容照排、不畫（W3 甲）**（2026-10-20，使用者）：照 Blink，容器裡的內容照常排、都有 fragment，第 N 行之後的行、atomic inline 和 block 不畫、hit test 跳過（Blink 的 `IsHiddenForPaint`），不需要 `overflow: hidden`（WPT 的 `webkit-line-clamp-040.html`）；容器的 auto 高度是第 N 行的底加上它自己的 block-end padding 和 border（`webkit-line-clamp-006.html`、`compat/webkit-box-clamp-bottom-border.html`），明定的 `height` 照它；被隱藏的元素照樣讀得到幾何。taffy 不改：容器排完之後覆寫它的高度，block 的 fragment 多一個隱藏的旗標。不選真的不排（taffy 的 block layout 不 fork 停不下來，被 clamp 掉的元素讀不到幾何）。
24. **第 2c 段：clamp 的容器排完之後數行（W4 甲）**（2026-10-20，使用者）：kun 包住 taffy 的 `compute_child_layout`，在 clamp 的容器排完之後，照文件順序走它的 block formatting context 裡的盒子（不進獨立的 formatting context：BFC root、flex、grid、table、atomic inline），累加 inline 內容的行數；第 N 行所在的 inline 內容記下那一行放 `…`、之後的行隱藏，之後的盒子記成隱藏，建 fragment 時照這些記號（`…` 沿用 `truncate_line`）；容器的高度從 taffy 存好的 layout 算出第 N 行的底。只看最後排好的結果，和 taffy 量幾次、用不用快取無關；taffy 只要量大小時 clamp 的容器也實際排一次；隱藏的記號變了的盒子要重建 fragment。Blink 在排的時候數（`LineClampData` 隨 constraint space 往下傳），因為 block layout 是它自己的。不選照 Blink 在排的時候數（taffy 會量好幾次、用快取，clamp 容器裡的盒子都得關掉快取，而且 inline 內容不知道自己在容器裡的位置，排完還是要算高度）。
25. **第 2c 段：做一般的 `display: flow-root`，clamp 的組合換成它（X1 甲）**（2026-10-20，使用者）：開始做時查 Blink 的 `StyleAdjuster` 發現，`-webkit-box-orient: vertical` 加 `-webkit-line-clamp` 時，computed 的 `display` 從 `-webkit-box` 換成 `flow-root`、`-webkit-inline-box` 換成 `inline-block`，另外記下原本寫的是 `-webkit-box`（`IsSpecifiedDisplayWebkitBox`），clamp 只看這個記號（WPT 的 `webkit-line-clamp-048.html`：computed value 是 `flow-root`，specified value 改成 `flow-root` 就不再 clamp）。kun 沒有 `flow-root`，所以把它做成一般的值：`Display::FlowRoot` 是建立自己的 block formatting context 的 block container，作者寫的 `display: flow-root` 也從此有效；clamp 的組合照 Chrome 在 style 的調整裡換掉 `display`、記下記號。不選只在序列化時寫成 `flow-root`（computed value 是假的、作者的 `flow-root` 照舊無效、`-048` 做不對）。
26. **第 2d 段：size containment 在 kun 包住 taffy 的那一層做（Z1 甲）**（2026-10-20，使用者）：taffy 0.15 的 `Contain` 只有 layout 和 paint。有 size containment 的盒子，taffy 問它內容多寬（沒有給寬度：shrink-to-fit、flex 和 grid 的 contribution）時當成內容是 0，排完之後 `auto` 的高度只剩 padding 和 border（和 line clamp 改高度同一處）；kun 自己排的（行內內容、replaced、表單控制項、table）在各自量大小的地方當成沒有內容；`inline-size` 只有寬度。內容照常排、溢出照樣畫。Blink 的 `ShouldApplySizeContainment`（沒有 `contain-intrinsic-size` 時 intrinsic 的寬和高是 0）。不選 fork taffy（違反不 fork taffy 的原則，kun 自己排的部分還是要另外做）。
27. **第 2d 段：`transform`、`contain` 同時接住 absolute 和 fixed 的子孫（Z2 甲）**（2026-10-20，使用者）：照 Blink 的 `ComputeIsFixedContainer` 和 `ComputeIsAbsoluteContainer`（接住 fixed 的也接住 absolute）：有 `transform`（不是 `none`，非 atomic 的 inline 除外）、`contain: layout` / `paint`（含 `strict`、`content`，適用的盒子）的盒子是兩者的包含塊，第 4 段的 `filter` 加進同一個判斷。告訴 taffy 的方式：kun 給 taffy 的 `CoreContainerStyle` 換成自己的型別，包住 `taffy::Style`、覆寫 `CoreStyle::is_containing_block`（taffy 為這種 property 留的介面）。`transform` 在 `none` 和有值之間變的時候要重排（包含塊換了），不再只重畫。不選只接 fixed（absolute 照舊跳過有 transform 的祖先，和 Chrome 不同，卡片裡的 absolute 角標這種常見的寫法會跑出去）。
28. **第 2d 段：「固定在 viewport」由 layout 記在 fragment 上（Z3 甲）**（2026-10-20，使用者）：盒子上記能不能接住 absolute、fixed（Blink 的 `can_contain_fixed_position_objects_`，在 style 設定時算一次），taffy 的 `is_containing_block` 讀它；`position: fixed` 而且沒有祖先接住的盒子，fragment 記「固定在 viewport」（Blink 的 `LayoutBox::IsFixedToView`，`Container()` 是 `LayoutView`），paint 掛到 viewport 的 property tree、scrollable overflow 不算它、幾何的 `layout::is_fixed` 都改讀它，不再看 style。Blink 的 paint 走樹時切換 fixed 的 context（`UpdateOutOfFlowContext`），是因為它的 layout tree 把 fixed 元素留在 DOM 的位置；kun 的 fragment tree 已經把 out-of-flow 的盒子掛在包含塊底下，被接住的照一般的 parent 畫。不選讀的地方各自往上找祖先（判斷寫在好幾處，容易和 layout 的包含塊不一致）。
29. **第 3 段：路徑的裁切是 property tree 的 clip node（K1 甲）**（2026-10-20，使用者）：`ClipNode` 除了矩形和圓角，多一組可有可無的路徑（聯集，見第 32 條），矩形是它們的外框：raster invalidation、幾何、intersection observer 這些只要範圍的地方照舊讀矩形，轉成 display list 時多一個 `ClipPath`，hit test 照路徑判斷（kurbo）。HTML 元素的 `clip-path` 和 SVG 元素的都用它，和巢狀 `<svg>` 的 viewport 裁切一樣在 pre-paint 建。Blink 的 `ClipPaintPropertyNode` 就是 clip rect 加上可有可無的 `ClipPath`（`ClipPathClipper::PathBasedClip`）。不選在 paint 時直接畫 `Save` / `ClipPath` / `Restore`（hit test、layer、invalidation 都看不到它，打破 chunk 和 property tree 的模型）。
30. **第 3 段：`clip-path` 的值（K2 甲）**（2026-10-20，使用者）：basic shape 的 `inset()`、`circle()`、`ellipse()`、`polygon()`、`path()`，加上 `rect()`、`xywh()`（computed value 照 Chrome 寫成 `inset()`）；reference box 的 `border-box`（預設）、`padding-box`、`content-box`、`margin-box` 和 SVG 元素的 `fill-box`、`stroke-box`、`view-box`；只寫 box 時裁成那個 box、帶著圓角；`url(#id)` 指向 `<clipPath>`。`clip: rect()` 照規範只對絕對定位和 fixed 的元素有效（`auto`、逗號的舊語法），是一個矩形的 clip node，裁元素自己和它的子孫（Blink 的 `CssClip` 節點）。不做 `shape()`（Chrome 135 之後才有、語法大，Speedometer 沒用），記進限制。不選加上 `shape()`，也不選只做 Speedometer 用到的 `inset()`、`url()`（WPT 能放進清單的少很多）。
31. **第 3 段：`clip-path` 和 `clip` 照 Chrome 插值（K3 甲）**（2026-10-20，使用者）：同一種 basic shape、同一個 reference box 時逐項插值（Blink 的 `BasicShapeInterpolationFunctions`），其他離散；`clip: rect()` 照四個邊插值（`CSSClipInterpolationType`）。WPT 的 `css/css-masking/clip-path/animations/`、`clip/` 的插值測試因此能放進清單。不選一律離散（和 Chrome 不同，要記成限制）。
32. **第 3 段：`<clipPath>` 的內容是形狀，幾個時取聯集（K4 甲）**（2026-10-20，使用者）：子元素 `rect`、`circle`、`ellipse`、`polygon`、`polyline`、`path`（`line` 沒有面積），各自帶 `transform` 和 `clip-rule`；`clipPathUnits` 的兩種；`<clipPath>` 自己的 `transform`，它上面的 `clip-path` 是再一層 clip node（交集）。幾個子元素時 clip node 存一組路徑，raster 用 Skia 的 PathOps 合成聯集，hit test 點在任何一個裡面就算。裡面有 `<text>` 或 `<use>` 時整個不裁，記進限制（要畫成 mask）。Blink 的 `LayoutSVGResourceClipper::AsPath` 也是用 PathOps 聯集子元素的路徑，做不到時才退回 mask。不選只支援一個子元素（常見的多形狀 `<clipPath>` 會失效），也不選加上 mask 當退路（和第 4 條的 `mask` 是同一套 save layer 加合成的機制，之後一起設計）。
33. **第 3 段：路徑裁切底下不成為 layer（K5 甲）**（2026-10-20，使用者）：路徑裁切底下的捲動容器、canvas、有動畫的元素不成為自己的 compositor layer，畫進所在的 layer，裁切畫在 display list 裡，和非平移的 transform 一樣（這樣的捲動容器捲動時在 main thread 重畫）。Blink 的 `PaintArtifactCompositor` 會做合成的裁切（`SynthesizedClip`，多一個 mask layer），compositor 和 Viz 都要支援 mask，記進「之後的可能」。不選這一段就做合成的裁切。
34. **第 3 段分兩小段（K6 甲）**（2026-10-20，使用者）：3a `clip: rect()` 和 `clip-path` 的 basic shape、box、插值（HTML 元素上：路徑的 clip node、hit test、layerize、增量）；3b SVG 的 `<clipPath>`、`url()`、`clip-rule`（HTML 和 SVG 元素都能引用）。每小段各自 commit，階段的檢查在 3b 做完時跑。不選一次做完。
35. **第 3 段：測試（K7）**（2026-10-20，使用者）：解析、序列化和 `getComputedStyle`（`style` 的 `cssom::`、`computed::`）；插值（`interpolation::`）；繪製（`paint` 的 clip node 和 `ClipPath`、hit test 打在路徑外的點、路徑裁切底下不成為 layer）；改 `clip-path` 只重畫、不重排；kun 自己的 reftest；WPT 的 `css/css-masking/clip-path/`、`css/css-masking/clip/`、`css/css-masking/clip-rule/` 和 `svg/` 裡和 clip 有關的，用 `--all` 試過、挑通過大半的；一份情境（Stockcharts 那種 `<clipPath>` 裁掉的地方點不到）；每小段的 mutation test（`mutants-3a.toml`、`mutants-3b.toml`）。Speedometer 不排量測。
36. **第 4 段分兩小段（G1 甲）**（2026-10-20，使用者）：4a 文字（`text-shadow`、裝飾線、`-webkit-text-fill-color`），4b 效果（`filter`、`isolation`），各自 commit，階段的檢查在 4b 做完時跑。兩者改的地方幾乎不重疊（4a 在文字的 paint，4b 在 effect node、raster 和包含塊）。不選一次做完。
37. **第 4 段：`text-shadow` 多畫一次 glyph（G2 甲，C1）**（2026-10-20，使用者）：每個陰影多畫一次同樣的 `DrawGlyphs`，帶位移和 blur（Skia 的 mask filter，和 `box-shadow` 一樣），裝飾線也有陰影；陰影的範圍算進 display item 的 visual rect（raster invalidation 包到陰影）；插值沿用 `box-shadow` 的（沒有 `inset`、`spread`）。沒有陰影的 run 不多走一步。Chromium 的 `TextPainter` 用 `cc::DrawLooper` 照每個陰影重畫同一個 text blob。不選 C2（一個 save layer 加 drop-shadow 的 image filter：多開 layer，和 Chrome 不同）。
38. **第 4 段：裝飾線的範圍（G3 甲）**（2026-10-20，使用者）：`text-decoration-style`（`solid`、`double`、`dotted`、`dashed`、`wavy`）、`text-decoration-color`、`text-decoration-thickness`（`auto`、`from-font`、長度、百分比），`text-decoration` shorthand 收全部，加上 `text-underline-offset`、`text-underline-position`。每個產生裝飾的盒子各自的線、樣式、顏色、粗細存成一份清單往下傳，取代現在的 bitset，順便解掉「裝飾線用文字的顏色」那條限制。Chromium 的 `AppliedTextDecoration` 的 vector，`TextDecorationPainter`、`TextDecorationInfo` 畫（`dotted`、`dashed` 是 stroke 的 dash，`wavy` 是 path，`double` 兩條）。不做 `text-decoration-skip-ink`（Chrome 的預設 `auto` 讓底線在下降部斷開，要 glyph 的輪廓和線的交點，Skia 的 `getIntercepts`），記進限制。不選只加 `style`、`color`、`thickness`、顏色照舊用文字的（乙）。
39. **第 4 段：`-webkit-text-fill-color`（G4）**（2026-10-20，使用者）：繼承的 property，初始值 `currentcolor`，有值時取代 `color` 當文字的填色（Blink 的 `TextPaintStyle.fill_color`）；裝飾線的 `currentcolor` 照舊是 `color`。不做 `-webkit-text-stroke-*`（Speedometer 沒用到）。
40. **第 4 段：`filter` 的函式全做（G5 甲，D1）**（2026-10-20，使用者）：`blur()`、`brightness()`、`contrast()`、`drop-shadow()`、`grayscale()`、`hue-rotate()`、`invert()`、`opacity()`、`saturate()`、`sepia()`，可以串接；不做 `url()`（第 4 條）。插值照 Blink 的 `FilterInterpolationFunctions`：同樣的函式序列逐項插值，短的一邊補初始值，其他離散。Chromium 的 `cc::FilterOperations` 到 Skia 的 image filter。不選只做 Spectrum 用到的 `drop-shadow()`（D2，WPT 能放進清單的少很多）。
41. **第 4 段：`filter` 是 effect node 上的清單，畫成一個 save layer（G6 甲）**（2026-10-20，使用者）：`EffectNode` 多一份 filter 清單，和 opacity 畫成同一個 `SaveLayer`（先 filter 再 opacity，Skia 的 layer paint 的順序，也是規範的順序）；blur、drop-shadow 往外擴的範圍算進 raster invalidation 和 tile 的 interest rect（Blink 的 `FilterOperations::MapRect`）。`filter` 不是 `none` 時是 stacking context，也是 absolute 和 fixed 的包含塊（加進第 27 條的判斷，改變時照它重排）。filter 底下不成為 compositor 的 layer（和第 33 條一樣，filter 的動畫在 main thread 重畫），記進限制。同時有 `filter` 和 `clip-path` 時的順序（規範是先 filter 再裁，kun 的 `clip-path` 是 clip node）做的時候對照 Blink，對不上記成限制。Blink 的 `EffectPaintPropertyNode` 的 filter，沒有 composite 時是 `SaveLayer` 加 `PaintFilter`；Blink 的 filter 和 opacity 是兩個 node，kun 一個 node 畫出來的順序一樣。不選分成兩個 node（乙：node 多一倍，kun 沒有 mask 和 composited 的 filter，沒有需要分開的地方）。
42. **第 4 段：`isolation`（G7）**（2026-10-20，使用者）：`isolate` 只建立 stacking context（kun 沒有 `mix-blend-mode`，只影響 z-index 的排序），在 4b。
43. **第 4 段：測試（G8）**（2026-10-20，使用者）：解析、序列化和 `getComputedStyle`（`style` 的 `cssom::`、`computed::`）；插值（`interpolation::`）；繪製（`paint` 的陰影的 op、裝飾線各樣式的 op、filter 的 effect node 和 `SaveLayer`、filter 範圍的 raster invalidation）；改 `text-shadow`、裝飾線、`filter` 只重畫、不重排，`filter` 在 `none` 和有值之間變時重排；kun 自己的 reftest；WPT 的 `css/css-text-decor/`、`css/filter-effects/`、`css/css-text/` 裡和 `text-shadow` 有關的，用 `--all` 試過、挑通過大半的；一份情境（`filter: blur()` 的元素和 TodoMVC 頁尾的 `text-shadow` 的截圖）；每小段的 mutation test（`mutants-4a.toml`、`mutants-4b.toml`）。Speedometer 不排量測。
44. **第 5 段分兩小段（S1 甲）**（2026-10-20，使用者）：5a 在 main thread 上做完整的 sticky：解析，layout 當成在流裡、定位的盒子（stacking context、absolute 的包含塊），main thread 算位移，`getBoundingClientRect`、`offsetTop`、hit test、pre-paint 都用它，位移畫進所在的 layer（這一小段捲動時重畫）；5b sticky 的內容成為自己的 layer，compositor 的 transform node 帶著限制、每次捲動算位移，捲動不重畫（E1）。各自 commit，5b 做完、審核之後才 merge，階段的檢查在 5b 做完時跑。兩段做完和 Blink 的分工一樣：main thread 算一份位移給幾何和 paint，cc 每次捲動重算、不用 main thread 那一份。不選一次做完（5a 單獨就能用 WPT 的 testharness 和 reftest 驗證，5b 只動 compositor 和 layerize，出問題時範圍清楚）。
45. **第 5 段：限制照新版 Blink 的做法（S2 丁）**（2026-10-20，使用者）：layout 讓每個 fragment 記下它底下有沒有 sticky 的盒子（`BoxFragment::sticky_below`，和 out-of-flow 往上傳一樣在建 fragment 時算；Blink 的 `PropagatedData::sticky_descendants` 每個軸分開，碰到捲動容器時 consumed）；layout 之後，renderer 從根沿著這些 fragment 往下走一次，為每個 sticky 盒子算出限制（它黏在哪個捲動容器或 viewport 裡、那個容器的內容座標裡的 constraining rect、sticky 盒子和 containing block 的矩形、四個 inset、巢狀 sticky 的外層），存在 `ScrollState` 的一張表（`layout::StickyPositions`；Blink 的 `UpdateAllStickyConstraints` 存在 `FragmentData`，`ExecutePendingStickyUpdates` 在 layout 之後跑）；捲動時照這張表重算位移（`StickyOffsets`），不重算限制、不走 fragment tree（Blink 的 `InvalidatePaintForStickyDescendants`）；geometry 的 walk、pre-paint 和之後的 compositor 都讀它。沒有 sticky 的頁面不走、表是空的。和當初寫的不同的兩處（做的時候改的）：fragment 上只記一個「底下有 sticky」的旗標，不照軸分開、不在捲動容器 consume：kun 的捲動容器兩軸都捲（沒有只捲一軸的），旗標也讓 layout 之後的那一趟只走進有 sticky 的子樹，用不到逐個容器的清單；捲動時重算表裡全部的位移，不只那個容器的：位移只是幾個矩形的加減，sticky 盒子的數量很少，分組反而要多維護一份對照。不選在 geometry 和 pre-paint 的 walk 裡當場算（甲：捲動時不知道哪些 sticky 要更新，只能整棵樹走一遍，兩個 walk 各算一次）；不選 layout 時存進 `BoxFragment`（乙：sticky 盒子排完時捲動容器的大小還沒定，fragment 是共用的 `Arc`）；不選舊版 Blink 存在捲動容器上（丙：kun 沒有對應的物件）。
46. **第 5 段：支援的盒子（S3 甲）**（2026-10-20，使用者）：block-level 的盒子（包括 flex 和 grid 的 item）、atomic inline（`inline-block`）、巢狀的 sticky（外層的位移帶動內層的 containing block，cc 的 `nearest_node_shifting_sticky_box` / `nearest_node_shifting_containing_block`）；`fixed` 裡的 sticky、`overflow: hidden` 的容器（它也是捲動容器）照規範。表格的部分（`thead`、`tr`、`th`、`td`）和非 atomic 的 inline 先不做，記進限制，對應的 WPT 照清單的慣例不放進去。不選表格的部分一起做（NewsSite 和 CodeMirror 都用不到，containing block 的規則和表格 layout 要另外改）。
47. **第 5 段：compositor 照 cc 移動 sticky（S4 甲）**（2026-10-20，使用者）：compositor 的 `TransformNode` 多一個 `sticky: Option<StickyPositionConstraint>`（layout 的限制、捲動它的 element、外層的 sticky 的節點、盒子的位置和 device scale），`position()` 算出位移加上去（cc 的 `StickyPositionOffset`，和 layout 的 `ComputeStickyOffset` 同一套算法，各自一份，和 Chromium 一樣）；主 frame 的 viewport 沒有 scroll node，它的捲動位移以 `ElementId::VIEWPORT` 放進 compositor 算位置用的 offsets。哪些 sticky 由 compositor 移動（`paint::composited_sticky`）：捲動它的是主 frame 的 viewport 或已經是 layer 的容器（子 frame 的 viewport 也是），而且 `can_be_layer`；在 fixed 盒子裡的（frame 的 viewport 不移動它）、在沒成為 layer 的容器裡的、在 opacity 或非平移的 transform 底下的，照 5a 畫進去、捲動時重畫。這樣的 sticky 的內容是自己的 layer，空間是 sticky 的 transform node，clip 是 sticky 外面的 clip；layer 的大小照它畫的範圍，只被 sticky 裡面的 clip 裁（外面的 clip 由 compositor 在 layer 所在的位置套，sticky 還沒移動時的位置常常在外面的 clip 之外）。`unscrolled()` 把它們的位移歸零，捲動時 paint 的結果不變；位移和盒子的位置一起取整（Blink 的 `pixel_snap_offset`），compositor 和 main thread 畫在同一個像素上。debug build 在每次把 paint 的節點轉成 compositor 的時，用 main thread 的捲動位移比對兩邊算出的位移。滾輪的範圍（main thread scroll hit test region 和 wheel event handler region）也照這樣分：sticky 裡面的放在 sticky 的空間，compositor 照它當下的位移換算；sticky 可能移到一個成為 layer 的容器上（兩者被不同的 sticky 移動，各自可能移動的範圍（layout 算的 reach）重疊）時，sticky 那一邊可以被打到的範圍記成要 main thread 的範圍、跟著它移動，而不是把整個容器當成被蓋住（被蓋住的容器上所有的滾輪都要等 main thread）。Blink 的 sticky translation node 帶 `CompositorStickyConstraint`，cc 的 `StickyPositionNodeData` 在 `TransformTree` 上算；cc 的 hit test 照 layer 找到 sticky 的 layer，kun 的 compositor 只照 scroll node 和範圍判斷，所以用範圍表達。不選像 `fixed` 在 `LayerProperties` 加欄位、畫的時候加到 origin（乙：sticky 底下的裁切不會跟著動，巢狀和在 layer 容器裡的 sticky 很難做對）。
48. **第 5 段：測試（S5）**（2026-10-20，使用者）：解析和 `getComputedStyle`；layout 的限制和位移（各個 inset、巢狀、containing block 擋住、constraining rect 扣掉 padding）；捲動之後的 `getBoundingClientRect`、`offsetTop`、hit test；compositor 的位移和 layerize（5b）；改 inset 只重算限制、不重排；kun 自己的 reftest；WPT 的 `css/css-position/sticky/` 裡 kun 做得到的；情境：捲動時導覽列黏在頂端、點得到，加一個捲動容器裡的 sticky；每小段的 mutation test（`mutants-5a.toml`、`mutants-5b.toml`）。hit test 和幾何用 main thread 的捲動位置，比 compositor 晚一個 frame，和 Blink 一樣。Speedometer 不排量測。

## 待決定

（無：每一段開始前決定的事都在「已決定的事項」，還沒做的在「之後的可能」和 `docs/limitations.md`。）

## 之後的可能

- 選取和游標的繪製（另一份 RFC，第 2 條）。
- 標準的 `line-clamp`（第 12 條）和 `contain-intrinsic-size`（第 17 條）。
- 3D 的 transform 和個別的 `rotate`、`scale`、`translate` property。
- mask-based 的 `clip-path`（`<clipPath>` 裡有文字或 `<use>`）、`mask`、`mix-blend-mode`、`backdrop-filter`、SVG 的 `<filter>`。
- `clip-path` 的 `shape()`（第 30 條）。
- 路徑裁切底下也成為 compositor 的 layer：Blink 的合成的裁切（`SynthesizedClip` 的 mask layer，第 33 條）。
- 非平移的 transform、path clip、filter 底下的子樹（和其中的 sticky）也成為 compositor 的 layer（Blink 的 composited transform 和 render surface）：和 RFC 0049（compositor 的動畫，草稿）的矩陣和 affine quad 一起做。
- 表格的部分（`thead`、`tr`、`td`）和非 atomic 的 inline 的 sticky、只捲一軸的捲動容器（第 46 條）。
- `hyphens: auto`。
