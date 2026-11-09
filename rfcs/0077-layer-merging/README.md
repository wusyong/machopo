# RFC 0077：不重疊的內容併回下面的 layer

- 開始日期：2026-10-23
- 狀態：📝 草稿
- 細節：無（設計確認時再補 `details.md`：Blink 的 `Layerizer` 逐項對照、量測）

## 摘要

照 Blink 的 `PaintArtifactCompositor`（`Layerizer::LayerizeGroup`），paint chunk 不再一律依序切段：一個 chunk 落在 canvas 或捲動容器的 layer「之後」時，往回找它可以合併的 layer，中間的 layer 都不可能和它重疊時，就併進那一個。和 canvas、捲動容器不重疊的內容因此留在原本的 layer（多半是 document 的不透明 layer），不再自己成為一個透明的 layer。

## 動機

**現況**（`crates/paint/src/layerize.rs` 開頭的說明）：每個成為 layer 的容器或 canvas 把頁面切成「之前、它自己、之後」，不做重疊判斷，n 個最多 2n+1 個 layer。「之後」的 layer 是透明的：裡面抗鋸齒的像素先 raster 在透明的底上、再由 Viz 疊上去，捨入和直接畫在背景上的不同，可能差 1 個色階，差不差看平台。

**問題**：

- **畫面和 Chrome 不同**：2026-10-23 查 WPT 的 `css/css-images/object-fit-none-png-00{1,2}c`、`object-fit-scale-down-png-00{1,2}c` 為什麼在 Linux 上失敗：七個並排的 canvas，從第二個起它們的虛線邊框都畫在前一個 canvas「之後」的透明 layer 裡，每頁 552 個像素差 1。最小的重現是三個並排、畫了內容的 float canvas。現在用 `.ini` 的 `fuzzy` 容許。其他同類的 reftest 的預期結果照平台分（`margin-collapse-009`、`css/CSS2/floats/new-fc-beside-adjoining-float` 等只在 macOS 上是預期的失敗）。
- **多餘的 layer**：一頁有幾個 canvas 或捲動容器，就多出幾個「之後」的 layer，各自 raster、各自佔 GPU 的記憶體、Viz 各自疊；Chrome 把它們併回下面的 layer，通常只有 document 的那一個。
- **layer 的切法隨內容抖動**：canvas 之前多一個或少一個 chunk，之後的 layer 的範圍和 key（同一個容器下的第幾個）就跟著變，沿用不了上一次的 raster。

**目標**：

- 和 canvas、捲動容器（以及 fixed、sticky 的 layer）不可能重疊的 chunk，併回它之前、相容的 layer；可能重疊的照舊放在上面。
- 上面那 4 個 WPT 拿掉 `fuzzy` 也通過；照平台分的那幾個 reftest，凡是內容不和容器重疊的，各平台都通過。
- 畫面的結果和現在一樣（順序只在不重疊時才換，重疊時照舊），只有 layer 的數量和捨入不同。

**要解決的 `docs/limitations.md` 的限制**（「Compositor、layer 和 input」）：

- 「每個成為 layer 的容器把頁面切成『之前、容器、之後』，不做重疊判斷 ……」那一條：「之後」的 layer、照平台分的預期結果、canvas 之後的內容差 1 的部分。

**不在範圍內**：

- 使用者捲不動的容器（`overflow: hidden`、`clip`）也成為 layer（同一條限制的後半）：那是哪些容器成為 layer 的問題，不是合併。
- canvas 自己的 bitmap 和頁面上直接畫的同樣形狀差 1（同一條的最後一句）。
- 其他 Blink 合併時考慮的東西：LCD 文字（machopo 沒有次像素的文字）、blend mode、把 effect 或 transform 拆開成自己的 layer（machopo 的 transform 和 opacity 不成為 layer）。

## 設計概要

```html
<canvas class=b></canvas><canvas class=b></canvas><canvas class=b></canvas>
```

三個並排、有邊框的 canvas。現在的 layer：document（第一個 canvas 的邊框）、canvas 1、透明的「之後」（第二個的邊框）、canvas 2、透明的「之後」（第三個的邊框）、canvas 3：6 個。做完之後：document（三個邊框）、canvas 1、canvas 2、canvas 3：4 個；三個邊框都畫在不透明的 document 上，和 `<div>` 的一樣。

一個 `position: absolute` 的盒子蓋在 canvas 上時，它和 canvas 重疊，照舊放進 canvas 上面的 layer：畫面不變。

## 詳細設計

### 1. 合併

照 `Layerizer::LayerizeGroup`：

- 依 paint 的順序處理 chunk。和最後一個 layer 相容（同一個捲動容器、sticky、fixed，都不是 canvas）時，照舊接在它後面。
- 不相容時（最後一個是 canvas 或別的容器的 layer），往回找：每個候選的 layer，相容就併進去、結束；不相容、而且新的 chunk 可能和它重疊時，停下來，新開一個 layer（`MightOverlap`）；不重疊就繼續往前。
- 只往回找到 `layer_merge_distance_limit` 個 layer（Blink 的 `kLayerMergeDistanceLimit`），避免每個 chunk 都掃過全部的 layer。

### 2. 重疊的判斷

- chunk 和 layer 各自在畫面上畫到的範圍（`painted_rect`，到 root 的空間，含 clip 和抗鋸齒的一像素）。
- 會動的 layer：捲動容器的內容只會在容器的框裡（用容器的框）；fixed 和 sticky 的 layer 會跟著捲動移動，範圍見「待決定」第 2 條。

### 3. 一個 layer 的 chunk 不再連續

`PendingLayer::chunks` 從一段 `Range<usize>` 改成幾段（或 chunk 的索引清單），同一個 layer 裡照 paint 的順序。要跟著改的：

- `layer.display_list`（`chunks_to_display_list`）：照順序走每一段。
- `RasterInvalidator`（`raster_invalidator.rs`）和 renderer 的 `PaintedLayer::after`：比對上一次時以 chunk 的身分為準，不靠連續的範圍。
- renderer 的 `PaintArtifactCompositor::update` 的 layer key（同一個容器下的第幾個）：合併之後「之後」的 layer 變少，key 的意義要重新確認（「待決定」第 3 條）。
- `painted_rect`、sticky 和 fixed 的範圍：照幾段算。

### 測試

- `paint` 的 `layerize` 測試：不重疊時併回、重疊時不併、相容性（不同的捲動容器、canvas、fixed、sticky 不併）、往回找的距離上限、chunk 不連續的 layer 的 display list 和 raster invalidation。
- 畫面：WPT 的 4 個 `object-fit` 拿掉 `fuzzy`；照平台分預期結果的那幾個 reftest 改成各平台都通過；machopo 自己的 reftest、情境照舊通過。
- mutation test：合併的條件、重疊的判斷、停下來的條件、距離上限。

### 分階段

一段做完：合併和 chunk 不連續的 layer 牽連在一起，分開做的中間狀態沒有用處。

## 缺點

- `PendingLayer` 的 chunk 不再連續，raster invalidation 和 layer 的沿用都要改，是 paint 和 renderer 的 compositing 之間的介面改動。
- 每個不相容的 chunk 都要做重疊判斷（有距離上限）。
- 合不合併由內容的位置決定：一個盒子移動到和 canvas 重疊時，它從 document 的 layer 換到 canvas 上面的 layer，那一刻兩個 layer 都要重新 raster（Blink 一樣）。

## 理由和其他做法

- **不做**：照舊用 `fuzzy` 和照平台分的預期結果；多的 layer 留著。
- **只對 canvas 做**：捲動容器「之後」的內容一樣有這個問題，照平台分的那幾個 reftest 都是捲動容器的，分開做要改兩次同一段程式。
- 完整的比較在設計確認時寫進 `details.md`。

## 先例

- **Chromium**：`PaintArtifactCompositor` 的 `Layerizer::LayerizeGroup`（往回找、`PendingLayer::Merge`、`MightOverlap` 停下來、`layer_merge_distance_limit_`），`PendingLayer` 的 bounds 是精確的聯集；捲動有關的重疊（`scroll_range_dependent`、`AddRangeDependentScroll`）。

## 已決定的事項

無。

## 待決定

1. **合併到哪裡**：
   - 甲：照 Blink，往回找任何相容的 layer（有距離上限）。
   - 乙：只併回 document 的 layer（第一個），其他的照舊。
   - 2026-10-23 的看法：甲。乙比較簡單，但捲動容器裡的 canvas 之後的內容（同一個容器的 layer）就併不回去。
2. **fixed 和 sticky 的 layer 的重疊**：
   - 甲：保守，一律當作可能重疊（往回找碰到就停）。
   - 乙：照 Blink 算它們隨捲動可能到的範圍（`scroll_range_dependent`）。
   - 2026-10-23 的看法：甲，之後量了再看要不要乙。
3. **layer 的 key**：現在是（捲動容器、sticky、同一個容器下的第幾個）。合併之後要不要改成以 layer 的第一個 chunk 的身分為 key（Blink 的 `PendingLayer` 用第一個 chunk 的 id 對應上一次的 cc layer），讓前面多一個 chunk 時不換 layer。
4. **距離上限的值**：Blink 的 `kLayerMergeDistanceLimit` 的預設值，實作前查。

## 之後的可能

- 使用者捲不動的容器不成為 layer（同一條限制的後半）。
- transform、opacity 的動畫成為 layer 之後（compositor 的動畫），它們也照同一套合併。
