# RFC 0026 的細節：背景和邊框

`README.md` 的補充：property 的清單、現在的管線缺什麼、選項的完整比較、WPT 的盤點。

## 現在有什麼

- `style`：`background-color`、`border-{top,right,bottom,left}-{width,style,color}` 和它們的 shorthand；`border-style` 只收 `solid` 和 `inset`，`inset` 在 paint 時畫成 `solid`。
- `computed_style`：`ComputedStyle` 是純資料，背景只有一個顏色。
- `paint`：`DisplayItemKind::BoxDecorationBackground` 一個 item 填滿 border box（`PaintOp::DrawRect`），接著 `border()` 用 `PaintOp::DrawPath` 畫四條邊的多邊形（共用一條 path，所以邊角沒有抗鋸齒的接縫）。
- `PropertyTrees::ClipNode` 的 `rect: Option<RectF>`：`overflow: hidden` 和捲動容器的 clip 都是矩形。
- `compositor_paint::PaintOp`：`DrawRect`、`DrawRRect`（單一半徑，註解寫著「per-corner radii arrive with `border-radius`」）、`DrawLine`、`DrawPath`、`DrawGlyphs`、`DrawImage`（整張圖縮放進 `dst`，raster 時才解碼）、`ClipRect`。
- `raster`：skia-safe 的 backend，照 `PaintOp` 逐個畫；`PaintFlags` 現在只有顏色、線寬、抗鋸齒這些。
- renderer：`start_loads` 收集 stylesheet 的 `href` 和 `<img>` 的 `src` 去 fetch，`images` 表以 URL 為 key 存 `ImageData`（還沒解碼的位元組），`<img>` 的 box 拿到它之後交給 paint。

## 要加什麼

| 層 | 加的東西 |
| --- | --- |
| `style` | 背景的 8 個 longhand 和 `background` shorthand 的 layer 文法；漸層的解析（`linear-gradient`、`radial-gradient`、`repeating-*`、色停點、角度和邊角關鍵字）；`border-radius` 的四個角；`box-shadow` 的清單；`outline` 和 `outline-offset`；多出來的 border style |
| `computed_style` | 背景的每個 longhand 一份清單（用到時才合成 `BackgroundLayer`，見 `README.md` 的「詳細設計」）、`BorderRadius`、`Vec<BoxShadow>`、`Outline`；背景圖的值是 `Url(...)` 或 `Gradient(...)` |
| renderer | computed style 裡的 `url()` 也要 fetch，和 `<img>` 共用 `images` 表；還沒載到的那一層不畫，載到了標記要重畫 |
| `paint` | 一個 phase 裡的順序（陰影、每一層背景、邊框、outline）；每一層的 clip / origin / position / size / repeat；圓角進 clip node 和 hit test；陰影和 outline 讓 `visual_rect` 變大 |
| `compositor_paint` | `PaintOp` 的 clip 和 rrect 帶四個角；`PaintFlags` 帶 shader（漸層、tile 的圖片）和 blur |
| `raster` | Skia 的 `SkGradientShader`、`SkImageShader`、`SkMaskFilter::MakeBlur`、`SkRRect` |
| `layerize` / `RasterInvalidator` | layer 的範圍和失效範圍要含陰影和 outline（現在假設 item 畫在自己的盒子裡） |

## 選項的比較

### A. 怎麼分段

- **A1（四段）**：背景圖和漸層 → `border-radius` → 陰影和 outline → border style。每一段自己就能驗收（reftest 和 WPT 的子集），而且第 1 段最大、最早拿到畫面的改善；圓角放第 2 段，因為它會改 clip 的型別，接在背景之後改動比較集中。
- **A2（兩段）**：背景；圓角和其他裝飾。段落少，但第 2 段同時動 clip 的型別、visual rect 的不變式和邊框的畫法，一次要驗收的面太大。
- **A3（五段）**：A1 再把 `url()` 的圖片和漸層分開。漸層不用載入、圖片不用 shader 以外的東西，分開之後每段更小，但兩段都要碰同一批 `background-*` 的 longhand 和同一組測試，會重複。

### B. 背景圖誰去載

- **B1（renderer 走 computed style 收集）**：和現在收 `<img>` 的 `src`、stylesheet 的 `href` 同一個函式、同一張表；style 只負責「這一層的圖是這個 URL」。缺點是 renderer 要走一次樹（但它本來就為了 `<img>` 走）。
- **B2（`style` 產出請求清單）**：cascade 的時候就知道哪些 `url()` 被用到，不用再走一次；但 `style` 會多一個「對外要資源」的角色，而 Blink 也是由 document 的 fetcher（renderer 這一側）去載，不是 `StyleEngine` 自己。
- **B3（`dom_api` 載）**：`dom_api` 和 engine 無關、也不該碰網路，不考慮。

### C. 漸層在哪裡算

- **C1（raster 用 Skia 的 shader）**：`PaintOp` 帶「這塊區域用這個漸層填」，raster 時才做成 `SkShader`——和 cc 的 `PaintShader` 一樣，display list 保持小、可重放、可以在不同縮放下重新 raster 而不失真。重複的背景圖同樣用 shader 的 tile mode。
- **C2（paint 先 raster 成 bitmap）**：display list 變成一張圖，縮放時會模糊，而且每次失效都要重算一張 bitmap；好處是 raster 不用動。
- **C3（style 展開成色塊）**：只能近似，而且宣告變了就要重算一堆 item，不考慮。

### D. 圓角的 clip 做到多深

- **D1（clip node 和 `PaintOp` 都帶圓角）**：`overflow: hidden` 的圓角卡片正確，hit test 的角落也正確；代價是 `ClipNode::rect` 的型別、compositor 端的 clip、和所有碰到 clip 的測試都要改。Blink 的 `ClipPaintPropertyNode` 本來就帶 `FloatRoundedRect`。
- **D2（只有背景和邊框是圓的）**：改動小，但圓角 + `overflow: hidden` 是最常見的卡片寫法，漏直角很明顯；而且之後補 D1 時一樣要改型別，等於做兩次。

### E. border style 做多少

- **E1（全部）**：`groove` / `ridge` / `inset` / `outset` 的深淺照規範（Chrome 的做法是把顏色調亮或調暗），`double` 的三等分，`dashed` / `dotted` 的虛線長度和轉角的接法。WPT 的 `css/css-backgrounds` 有一大批在測這些。
- **E2（只做 `dashed`、`dotted`、`double`）**：其餘畫成 `solid`，和現在 `inset` 的做法一致，記進限制。真實頁面用 `groove` 這些的很少。

### F. 陰影的模糊

- **F1（Skia 的 blur mask filter）**：和 Blink 一樣（`PaintFlags::setMaskFilter` 加 `SkMaskFilter::MakeBlur`，sigma = blur / 2），raster 時才算，和縮放無關。
- **F2（paint 自己算）**：要自己寫一個分離式高斯模糊、產出 bitmap；可控但慢、也和 Chrome 的結果不一樣。

### G. 多層背景

- **G1（完整的層清單）**：`background-image: a, b` 這種寫法在真實頁面不少（圖示 + 漸層），而且 `background` shorthand 的序列化本來就要處理多層；資料結構一開始就是 `Vec` 比較省事。
- **G2（只一層）**：少數情況會少畫一層，省下的工很有限。

### H. `background-attachment`

- **H1（只做 `scroll`）**：`fixed` 要讓背景不隨文件捲動——那是 `position: fixed` 那一套（layer 的標記、compositor 把捲動加回去），但背景是「畫在 viewport 的大小上、再 clip 到盒子」，和 fixed 的盒子不完全一樣；WPT 有一個既有的 baseline（`add-background-attachment-fixed-during-smooth-scroll`）就是它。
- **H2（連 `fixed` 一起做）**：盤點裡 `background-attachment` 有 18 個 suite，但幾乎都是 `scroll`（shorthand 展開的重設值）。

## WPT 的盤點（2026-10-09）

| 目錄 | reftest | testharness.js |
| --- | --- | --- |
| `css/css-backgrounds` | 705 | 132 |
| `css/CSS2/backgrounds` | 339 | 0 |
| `css/css-images` | 468 | 42 |

照 RFC 0003 的做法：候選腳本放行這一段新增的 property 和值，在現在用的目錄上重跑，把新的候選全部匯入，過不了的記進 expectations 附上原因。這三個目錄很大，所以分段匯入（第 1 段只放行背景的 longhand 和兩種漸層，圓角、陰影、border style 各自那一段再放行）。

`css/css-backgrounds` 的 testharness.js 測試大多在測 CSSOM 的序列化（`background` shorthand、`border-radius` 的合回），和 RFC 0016 的白名單一起長。

## Speedometer 的用量（來自 `../0012-speedometer/details.md` 的盤點）

- 背景的 longhand：`background-image`、`-size`、`-repeat`、`-clip`、`-origin`、`-position-x` / `-y`、`-attachment` 各 18 個 suite。
- `border-*-radius`：四個角各 15 個 suite。
- `box-shadow`：14 個 suite。
- `url()`：13 個 suite；`linear-gradient()`：1 個 suite（漸層主要是為了真實頁面和 reftest，不是為了 Speedometer）。
- `border-image-*`：16 個 suite，但那是 `border` shorthand 展開出來的重設值，不是頁面真的用到 border image。
