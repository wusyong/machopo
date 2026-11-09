# RFC 0033 的細節：Canvas 2D

## 1. 現況（2026-10-10，`main` 的 `376eefff`）

**DOM**：`dom_api` 的 `html_element` 的表裡，`canvas` 是 HTML 定義、沒有自己的 interface 的名稱，所以是 `HTMLElement`。宣告表已經有 canvas 會用到的形式：union（`unions!`，`fillStyle` 的 `(DOMString or CanvasGradient or CanvasPattern)`）、`ArrayBufferView`（`ImageData` 的 `Uint8ClampedArray` 要再確認 `ViewKind` 有沒有它）、`Promise<T>`（`toBlob` 是 callback，不用）。`style` 會解析 CSS 的 `font` shorthand（五個 longhand）。

**layout 和 paint**：replaced element 只有 `<img>` 和 `<iframe>`（`layout/src/replaced.rs`、`lib.rs` 的 `is_replaced`）。`<canvas>` 照一般的元素排版，子元素（fallback content）也畫。script 執行時 renderer 把 `LayoutClient`（`dom/src/layout_client.rs`）借給 engine，script 讀幾何時同步更新 style 和 layout。

**display list 和 raster**：`compositor_paint` 的 op 照 `cc::PaintOpBuffer`：`Save`、`Restore`、`Translate`、`ClipRect`、`ClipRRect`、`Concat`、`SetMatrix`、`SaveLayerAlpha`、`DrawColor`、`DrawRect`、`DrawRRect`、`DrawDRRect`、`DrawLine`、`DrawClipped`、`DrawShadow`、`DrawPath`（只有多邊形，non-zero）、`DrawGlyphs`、`DrawImage`。`PaintFlags` 有顏色、填或描邊（寬度）、shader、模糊、只有一組 on / off 的虛線；沒有線的端點、接合、miter、even-odd、混合模式。`gfx::ImageData` 是編碼好的圖（PNG、JPEG、GIF 的 bytes 和 id），`raster` 用到時才解碼、照 id 快取（`RasterCache`）。`raster::playback` 每次畫一張新的 bitmap。`renderer` 依賴 `raster`；`dom_api`、`dom_js`、`dom_wasm` 不依賴它，也不 link Skia（`cargo tree`）。

**WPT**：`../third_party/wpt/html/canvas/element/` 有 1821 個 `.html`，其中 886 個用 `getImageData` 或 `_assertPixel` 讀像素；另外有 `offscreen/`（不在範圍內）。

**Speedometer 3.1**（2026-10-10 的盤點，`target/speedometer-results/`，Chrome for Testing 152 的紀錄）：

| Suite | 在 kun 上 | 用到的 canvas API | 還缺的其他東西 |
| --- | --- | --- | --- |
| Charts-chartjs | 跑完，有例外：`can't acquire context from the given item` | `getContext`、`width`、`height`、`canvas`、`save`、`restore`、`setTransform`、`resetTransform`、`translate`、`rotate`、`clearRect`、`beginPath`、`moveTo`、`lineTo`、`quadraticCurveTo`、`arc`、`rect`、`closePath`、`fill`、`stroke`、`clip`、`fillStyle`、`strokeStyle`、`lineWidth`、`setLineDash`、`lineDashOffset`、`globalAlpha`、`font`、`textAlign`、`textBaseline`、`fillText`、`measureText`、`TextMetrics.width` | `MessageChannel`、`ResizeObserver`、`Element.shadowRoot` |
| React-Stockcharts-SVG | 卡住：`this.testCanvas.getContext is not a function` | 上面的一部分，加上 `scale`、`lineJoin` | SVG 的 DOM 和幾何、custom element、`attachShadow` |
| Perf-Dashboard | 卡住：`alert is not defined` | 同 React-Stockcharts-SVG | `attachShadow`（`closed`）、`alert` |

三個 suite 都沒用到 `getImageData`、`drawImage`、漸層、圖案、`Path2D`、陰影、`globalCompositeOperation`、`toDataURL`。

## 2. 整體的選項

### 範圍

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **S1（2026-10-11 選定）** | 這份涵蓋 `HTMLCanvasElement` 上的 2D context 全部，分四段 | 一份文件說明錄製、raster、畫面的整體設計 | 第 3、4 段離 Speedometer 遠 |
| S2 | 只做 Speedometer 用到的（第 1、2 段，不含 `getImageData`） | 小 | 漸層、`drawImage`、`Path2D` 很常見，之後還要一份描述同一套設計；沒有 `getImageData` 就沒有 WPT |

### 畫面怎麼到螢幕

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| P1 | 畫布的快照是一張解碼好的圖，paint 把它畫進頁面的 display list | 只多一種 replaced element 和一種圖；compositor、Viz 不改；Chromium 的 canvas 沒有 shared image 的 backing 時的做法 | canvas 每畫一次，頁面上它碰到的 tile 整塊重新 raster；快照多一次複製 |
| **P2（2026-10-11 選定）** | canvas 是自己的 compositor layer（`cc::TextureLayer`），快照當成 `TransferableResource` 送給 Viz，paint 只記一個 foreign layer | canvas 重畫不動頁面的 tile；Chrome 平常的做法（GPU raster 和 CPU raster 寫進 shared image 都是） | 新的 layer 種類、layerize 要處理 foreign layer、resource 的生命週期和回收、Viz 的 quad；kun 的 raster 在 CPU 上，好處要量過才知道 |

選 P2 也要有 P1 的路徑當 fallback：kun 的 compositor 和 Viz 只平移 layer（`paint/src/layerize.rs`：捲動容器也只在到 root 的 transform 只有平移、沒有 opacity 時成為 layer），rotate、scale、opacity 底下的 canvas 把快照畫進頁面。Chromium 的 foreign layer 不受這個限制（cc 的 layer 有任意的 transform 和 effect node）。

### 錄製和 raster

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **D1（2026-10-11 選定）** | `dom_api` 把每個呼叫錄成 `compositor_paint` 的 op，存在 `dom` 的 context 上；像素也存在那裡。renderer 在 rendering update 時用 `raster` 把 op 畫進像素；script 讀像素時經過借給 engine 的 client 當場畫 | 和 Chromium 的分層一樣（錄進 `cc::PaintRecord`、由 `CanvasResourceProvider` 畫）；Skia 照舊只在 `raster`；像素跟著元素的 GC 釋放，不用另外的表 | `dom_api` 的單元測試看不到像素（要 renderer 或 shell 的測試，和 WPT）；同步讀像素要經過 client |
| D2 | `dom_api` 直接依賴 `raster`，自己畫 | 不用 client；`dom_api` 的測試就能讀像素 | `dom_api`、`dom_js`、`dom_wasm` 的測試 binary 都要 link Skia，重編和 link 變慢；Skia 不再只在 `raster` |
| D3 | 另一個 2D 函式庫（tiny-skia、vello_cpu）在 `dom_api` 裡即時畫 | 依賴小、不用錄製 | `docs/architecture.md` 的選型是只用 Skia、不維護第二個 backend；反鋸齒和 Chrome、頁面的其他部分不同 |
| D4 | 像素存在 renderer 的表（照元素找） | `dom` 裡沒有大塊的資料 | 元素被 GC 之後 renderer 不知道，要另外的通知；文件換了也要清 |

### 文字

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **T1（2026-10-11 選定）** | 經過 client 交給 renderer 的 `TextContext`（parley），回傳 glyph 和位置，錄成 `DrawGlyphs` | 字型、fallback、度量和頁面一致；Chromium 也用頁面的 `Font` | client 要多一個方法；`dom_api` 單獨測不到排版 |
| T2 | `dom_api` 自己一份 parley 的 context | 不用 client | 兩份字型的快取和 fallback，結果可能和頁面不同；`dom_api` 多依賴 parley |

### wasm

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **W1（2026-10-11 選定）** | 照 RFC 0012 的第 7 條，JS 和 wasm 都有 | wasm 文件也能畫圖；成員都是數字、字串、union，WIT 照現有的規則產生 | WIT 和 golden file 變大，guest 重新產生 |
| W2 | 先只給 JS | 少一半的測試 | wasm 文件長期沒有 canvas，要記進 `docs/limitations.md` |

## 3. Chromium 的做法（2026-10-10 對照 `main` 的原始碼）

- **元素和 layout**：`HTMLCanvasElement`（`core/html/canvas/html_canvas_element.cc`）擁有 context（`CanvasRenderingContext`）和畫布的大小；`width` / `height` 改了就 `Reset()`。layout 物件是 `LayoutHTMLCanvas`（`LayoutReplaced`），有 script 時不建 fallback content 的 layout 物件。
- **context**：`modules/canvas/canvas2d/`：`CanvasRenderingContext2D` 繼承 `BaseRenderingContext2D`（`OffscreenCanvasRenderingContext2D` 也是）；狀態是 `CanvasRenderingContext2DState` 的堆疊（樣式、線、transform、clip、字型、陰影、合成）；路徑是 `CanvasPath`（`Path2D` 也用它），底下是 `PathBuilder`（`SkPathBuilder`），`arc` 和 `ellipse` 經過 `AddEllipse` 呼叫 Skia 的 `arcTo(oval, …)`，Skia 把弧線做成 conic；路徑存在當下的 user space（第 5 節的「路徑存在哪個座標」）。
- **錄製和 flush**：畫的呼叫錄進 `MemoryManagedPaintRecorder`（一份 `cc::PaintRecord`）；flush（畫過的 task 結束時的 `FinalizeFrame`、讀像素、錄太多時；2026-10-11 更正，原本寫成每個 frame，見第 5 節的「什麼時候畫」）把 record 交給 `CanvasResourceProvider` 畫：CPU 的是 Skia 的 raster surface，GPU 的是 shared image 和 GPU process 的 raster。`willReadFrequently` 讓 canvas 留在 CPU。
- **backing**（`CanvasRenderingContext2D` 建 backing 的地方）：GPU compositing 時，GPU raster 或 CPU raster 加上 mappable shared image（feature 開著時）建 shared image 的 provider；software compositing 時建共用記憶體的 shared image；都不行時才 `CreateSoftwareSurface`（CPU 的 bitmap，註解說它之後「上傳到 GPU 或複製到 Viz」）。小於 128×129 的 canvas 預設不用 GPU raster（`kMinimumAccelerated2dCanvasSize`），`willReadFrequently` 讓它用 CPU raster。
- **畫面**：`CanvasRenderingContext2D::IsComposited()` 是「有 shared image 的 provider，而且不是 low-latency、沒有 hibernate」。是的話 `HTMLCanvasPainter::PaintReplaced` 記一個 foreign layer（`kForeignLayerCanvas`，`cc::TextureLayer`）；不是的話 `canvas->Paint()` 把快照當圖畫進頁面的 display list。所以 Chrome 平常的 canvas（不管 GPU 或 CPU raster）多半是自己的 layer，畫進頁面是 fallback。
- **文字**：`font` 用 CSS 的 parser 解析、`CanvasFontCache` 快取；相對的單位照 canvas 元素的 computed style，元素在文件裡時先更新它的 style；排版和畫用 Blink 的 `Font`。

## 4. 各段的 API

| 段 | Interface 和成員 |
| --- | --- |
| 1 | `HTMLCanvasElement`：`width`、`height`、`getContext()`。`CanvasRenderingContext2D`：`canvas`、`getContextAttributes()`、`save()`、`restore()`、`reset()`、`isContextLost()`、`scale()`、`rotate()`、`translate()`、`transform()`、`setTransform()`（六個數字的形式）、`resetTransform()`、`globalAlpha`、`fillStyle` / `strokeStyle`（字串）、`lineWidth`、`lineCap`、`lineJoin`、`miterLimit`、`setLineDash()`、`getLineDash()`、`lineDashOffset`、`beginPath()`、`closePath()`、`moveTo()`、`lineTo()`、`quadraticCurveTo()`、`bezierCurveTo()`、`arcTo()`、`arc()`、`ellipse()`、`rect()`、`roundRect()`、`fill()`、`stroke()`、`clip()`（`CanvasFillRule`）、`clearRect()`、`fillRect()`、`strokeRect()`、`getImageData()`、`ImageData`（`width`、`height`、`data`、建構子） |
| 2 | `font`、`textAlign`、`textBaseline`、`direction`、`letterSpacing`、`wordSpacing`、`fontKerning`、`fontStretch`、`fontVariantCaps`、`textRendering`、`fillText()`、`strokeText()`、`measureText()`、`TextMetrics` |
| 3 | `createLinearGradient()`、`createRadialGradient()`、`createConicGradient()`、`CanvasGradient.addColorStop()`、`createPattern()`、`CanvasPattern`、`drawImage()`（`<img>`、`<canvas>`）、`globalCompositeOperation`、`shadowColor`、`shadowBlur`、`shadowOffsetX` / `Y`、`imageSmoothingEnabled`、`imageSmoothingQuality`、`Path2D`、`isPointInPath()`、`isPointInStroke()`、`createImageData()`、`putImageData()`、`getTransform()` 和 `setTransform(DOMMatrix2DInit)`（要 `DOMMatrix`，那一段決定做不做） |
| 4 | `toDataURL()`、`toBlob()` |

## 5. 第 1 段的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的原始碼（`canvas_rendering_context_host.cc`、`base_rendering_context_2d.cc`、`canvas_path.cc`、`platform/geometry/path_builder.cc`、`canvas_rendering_context.cc`、`canvas_rendering_context_2d.cc`、`html_canvas_element.cc`）；Skia 的 `arcTo` 做成 conic 和 `alpha: false` 的畫布從黑開始是 Skia 和 Chromium 的行為，沒有對照原始碼。

### 路徑的型別（`README.md` 第 6 條）

Chromium：Blink 的路徑是 `PathBuilder`（`SkPathBuilder`）；`arc` 和 `ellipse` 呼叫 `AddEllipse`，再呼叫 `arcTo(oval, …)`（整圈分成兩個 180°，因為 `addOval` 會開新的子路徑）；cc 的 `DrawPathOp` 直接存 `SkPath`。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **A1（選定）** | 自己的路徑型別（`SkPath` 的對應），f32，move、line、quad、conic、cubic、close；由 SVG 那邊先做、先 merge（`README.md` 第 6 條） | 和 `SkPath` 一對一，`raster` 照樣是一對一的翻譯；`arc` 和 Chrome 一樣是 conic，像素最接近；`compositor_paint`、`raster` 不多依賴 | 要自己寫弧線轉 conic（移植 `SkConic::BuildUnitArc`，約一百行）和 bounds；第 3 段的 `isPointInPath` 要自己算 winding 或經過 client 用 Skia |
| A2 | `PaintOp` 直接放 `kurbo::BezPath` | 和 SVG 同一個型別；bounds、winding、stroke 展開現成 | 沒有 conic，弧線用 cubic 近似，邊緣的像素可能和 Chrome 差一點；f64；paint op 綁在外部 crate 上 |
| A3 | 留著多邊形，canvas 把曲線攤平成線段 | `raster` 幾乎不改 | 精細度在錄的時候定了，放大時失真；接合錯；和 Chrome 差最多 |

### 什麼時候畫（第 7 條）

Chromium：context 在一個 task 裡第一次畫時把自己註冊成 task observer（`DidDraw`），task 結束時（`DidProcessTask`）`FinalizeFrame`；composited 的 canvas 在 `Canvas2dDeferredFlushEnabled` 時 `FinalizeFrame` 不 flush，延到 compositor 要它的 resource（`PrepareTransferableResource`）。讀像素時、錄的超過上限時（`FlushIfRecordingLimitExceeded`，有 layer 的時候不做）也 flush。`Canvas2dDeferredFlushEnabled` 預設開不開沒有查。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **F1（選定）** | rendering update 的 pre-paint 之前畫；讀像素時、錄的超過上限時也畫 | 同一個 frame 裡好幾個 task 畫過只 raster 一次；不用 event loop 的 hook；對應 Chromium 延到 compositor 要 resource 的做法（kun 的 commit 接在 paint 後面） | 指令在兩個 frame 之間累積比較久，要靠上限擋住記憶體；和 Chromium 平常的時機不同（script 看不到，只影響時間落在哪裡） |
| F2 | 每個畫過的 task 結束時畫 | 和 Chromium 平常的時機一樣，指令不跨 task 累積 | 要 renderer 的 event loop 在 task 結束時的 hook；一個 frame 裡多個 task 畫圖時，同步的 CPU raster 跑好幾次（Chromium 的 GPU raster 只是把指令交出去，比較便宜） |

### 路徑存在哪個座標（第 8 條）

Chromium：路徑存在當下的 user space，加點時不乘上 transform；換 transform 時把已有的路徑換算到新的 user space；transform 不可逆時改存成 device space，之後加的點才乘上 transform（`CanvasPath` 的 `!IsTransformInvertible()` 分支；`arc`、`ellipse`、`roundRect` 在不可逆時直接不加）。`fill`、`stroke`、`clip` 都是「user space 的路徑加上當下的 CTM」。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| R1 | 加點時就乘上 transform，存成 canvas 的座標（原本草稿的寫法） | 照規範的字面；加點簡單，換 transform 不動路徑；不可逆的 transform 不用特別處理 | `stroke` 要先用反矩陣把路徑換回 user space，線寬和虛線才對，點換算兩次；`Path2D` 存在 user space，兩套表示 |
| **R2（選定）** | 照 Chromium，存在 user space，換 transform 時換算 | `fill`、`stroke`、`clip`、`Path2D`、`isPointInPath` 一條程式路徑；錄下的 op 和 Chromium 一樣；stroke 不用來回換算 | 換 transform 時要走過整條路徑（通常這時是空的，例如 Chart.js 先 `setTransform` 再畫）；要照 Chromium 處理不可逆的 transform |

### 畫布大小的上限（第 9 條）

Chromium：`IsValidImageSize`：寬高不是 0、面積最多 32768×8192（268,435,456 px，RGBA 約 1 GiB；註解說 Firefox 每邊限 32767 但很慢，所以用面積）、每邊最多 65535。不合格時沒有 backing：畫的呼叫不做事，`getImageData` 的快照是空的，給全 0 的像素；`getContext` 照樣給 context，元素照樣排版。backing 在第一次需要時才建；背景分頁的 canvas 會 hibernate。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **Z1（選定）** | 上限照 Chromium，第一次畫或讀時才配置，配置失敗當成不合格 | 和 Chromium 一致；大但沒畫的 canvas 不佔記憶體 | 一張最多約 1 GiB，kun 沒有 hibernation；Rust 的配置失敗會 abort，要用可以失敗的配置 |
| Z2 | 比 Chromium 小的上限（例如 16384×16384） | 一張 canvas 吃的記憶體少 | 介於兩者之間的大小在 kun 上畫不出來、在 Chrome 上可以 |
| Z3 | 不設上限 | 最簡單 | 網頁能讓 renderer 因為記憶體不足 abort |

### context 的選項（第 10 條）

Chromium：`alpha: false` 時 `TextureLayer` 是 `SetContentsOpaque(is_opaque_)`、`SetBlendBackgroundColor(!is_opaque_)`，畫布不透明、從黑開始。`willReadFrequently` 讓 canvas 留在 CPU 上畫；沒設、又多次 `getImageData` 時 console 出警告，加速的 canvas 退回 CPU。`desynchronized` 走 low-latency，不是 `TextureLayer`。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **`alpha: false` 完整做（選定）** | 不透明的黑開始、`clearRect` 清成黑、raster 用不透明的 alpha type、layer 是 `contents_opaque` | 畫面和 Chrome 一致；Viz 不用混合 | raster 和 compositor 各多一種情況（不大） |
| `alpha: false` 只記下 | 照透明的畫 | 省事 | 畫面和 Chrome 不同，WPT 相關的測試失敗 |
| **`willReadFrequently` 只記下、回報（選定）** | `getContextAttributes()` 回報 | kun 一直在 CPU 上畫，就是 Chromium 設了它之後的狀態 | 沒有 console 的警告 |
| `desynchronized`、`colorSpace` 回報預設值（選定） | `false`、`"srgb"` | — | 要用它們的網頁看不到效果，在範圍外 |

### 讀像素時的 client（第 11 條）

Chromium：context 直接擁有 `CanvasResourceProvider`，flush 和讀像素都在 context 裡；文字要的 style 經過 document 更新（`UpdateStyleAndLayoutTreeForElement`），也就是從 document 走到 frame 的 lifecycle，kun 對應這條路的是 `LayoutClient`。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **K1（選定）** | `LayoutClient` 加上畫 canvas 的方法（第 2 段再加排版文字的） | 出借機制和 `unsafe` 的論證不動；style 的更新本來就在它上面 | trait 的名字和內容越來越不合（第 2 段決定改不改名）；`dom_api` 的單元測試的假實作多一個方法 |
| K2 | 另一個 `CanvasClient`，一起借給 engine | 職責分開，canvas 的測試只要假造一個小 trait | `dom` 多一處 `NonNull` 的出借，要更新 `crates/dom/soundness.md`；每個執行 script 的入口借兩份；文字還是要兩個 client |

### foreign layer 怎麼表示、canvas 成為 layer 的條件（第 12 條）

Chromium：paint 時由 `IsComposited()` 決定（有 shared image 的 provider、不是 low-latency、沒有 hibernate）。是的話 `HTMLCanvasPainter` 記 foreign layer 的 display item（`kForeignLayerCanvas`），自己一個 paint chunk，`PaintArtifactCompositor` 給它自己一個 cc layer；不是的話把快照當圖畫進頁面。條件和 transform、opacity 無關：cc 的 layer 什麼 transform 和 effect 都能合成。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **L1（選定）** | paint 一律記一個 canvas 的 item、自己一個 chunk，op 是 fallback 的「快照當圖」；layerize 決定它是 `TextureLayer` 或照一般的 chunk 進 picture layer | kun 的條件看 property tree，本來就是 layerize 的事，和捲動容器共用一個函式、只判斷一次；paint 不用知道結果 | 和 Chromium 決定的位置不同；raster invalidation 要略過成為 layer 的 canvas 的 item |
| L2 | paint 時決定，記兩種 item | 和 Chromium 決定的位置相同，layerize 只看 item 的種類 | paint 要重算 property tree 的條件，和 layerize 重複，兩邊可能不一致 |

### `TextureLayer` 的快照和釋放（第 13 條）

Chromium：`TextureLayer` 每個 frame 經過 `PrepareTransferableResource` 拿 resource，和目前的一樣就不更新；Viz 用完之後經過 release callback（`EndDisplayCompositorAccess`）還回來，快照靠 callback 持有的參照活著。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **V1（選定）** | 快照是 `Arc<[u8]>`、大小和遞增的版本號；版本換了 impl 端才建新的 shared image，舊的在沒有 frame 用到之後靠 `Arc` 釋放 | kun 的 tile 已經這樣做，不用新的回傳路徑 | 和 Chromium 的 release callback 不同；拆 process 時要和 tile 一起改（「Process 的邊界」的例外 1） |
| V2 | 照 Chromium 做 release callback | 和 Chromium、之後的 multi-process 一致 | kun 的 tile 沒有這條路，只為 canvas 多一套 resource 的回收 |

## 6. 1b 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas_2d_recorder_context.cc`、`canvas_rendering_context_2d.cc`、`image_data.h`。

### `clearRect`（`README.md` 第 14 條）

Chromium：`GetClearFlags()` 在有 alpha 時設 `SkBlendMode::kClear`，否則用黑色；`drawRect` 畫。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **B1-b（選定）** | `compositor_paint` 加 `ClearRect { rect }`，raster 用 `kClear` 的 paint；不透明的畫布錄成黑色的 `DrawRect` | 不碰 `PaintFlags`、和 SVG 那邊不衝突；結果和 Chrome 一樣 | cc 沒有這個 op，是暫時的形式，第 3 段改回 Chromium 的寫法 |
| B1-a | 現在就在 `PaintFlags` 加 blend mode | 一開始就是最後的形式 | 改到 SVG 那邊正在改的 struct |
| B1-c | `clearRect` 等到 1e | 不用新增 | Chart.js 每個 frame 都先 `clearRect` |

### `ImageData`（第 15 條）

Chromium：`ImageData` 是 `ScriptWrappable`，像素是 `NotShared<DOMUint8ClampedArray> data_u8_`（V8 heap 上的陣列），`data()` 每次回傳同一個。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **B2-d（選定）** | `dom` 的 GC 型別；JS 的 `data` 是 engine 的 `Uint8ClampedArray`，存在 `ValueSlot`；wasm 存 bytes、guest 拿複本 | `[SameObject]`、寫進去的留著；序列化的路和 Chrome 一樣 | 新的 GC 型別要照規則加齊測試；`ScriptHost` 多一個建 typed array 的方法 |
| B2-c | glue.js 的 class，宣告表回傳寬、高和 bytes | 不用新的 GC 型別 | 不能序列化；brand check 和形狀自己做；JS、wasm 兩套 |
| B2-a | `data` 每次一份複本 | 最簡單 | 違反 `[SameObject]` |

### `currentcolor`（第 16 條）

Chromium：設定時解析（`ExtractColorFromV8StringAndUpdateCache`），`ColorParseResult::kCurrentColor` 時 `GetCurrentColor()`：元素不在、不 connected 或沒有 inline style 時是黑色，否則解析 inline style 的 `color`。規範是元素的 computed `color`。

| 選項 | 內容 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **B3-a（選定）** | 照 Chromium | 和 Chrome 一樣，不更新 style | 從 stylesheet 繼承的 `color` 不算 |
| B3-b | 照規範，computed `color`，設定時更新 style | 照規範 | 和 Chrome 不同；設定一次更新一次 style |

### 先畫的時機（第 17 條）、`dom` 的依賴（第 18 條）

見 `README.md` 的那兩條；沒有其他實際的選項。

## 7. 1c 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `html_canvas_painter.cc`、`html_canvas_element.h`。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| 快照的型別（第 19 條） | 不是 layer 的 canvas 由 `HTMLCanvasPainter::PaintReplaced` 呼叫 `canvas->Paint()`，快照是 `StaticBitmapImage`（解碼好的 `SkImage`）；cc 的 `PaintImage` 可以包它，和編碼的圖同一個 `DrawImageRectOp` | C1-a：`gfx::ImageData` 多一種解碼好的像素，沿用 `DrawImage` | C1-b（新的 `DrawBitmap` op）：cc 沒有，縮放和取樣另外一份 |
| 快照的記憶體（第 20 條） | `makeImageSnapshot` 是 copy-on-write | C2-a：像素和快照各一份，像素變了之後第一次要快照時複製（可以失敗的配置） | C2-b（共用、copy-on-write）：畫進頁面時 display list 一直拿著上一張，成本一樣；畫的路徑多處理共用、`Arc<[u8]>` 的配置失敗會 abort |
| paint 怎麼拿到快照（第 21 條） | paint 時向元素要；畫過時 `LayoutHTMLCanvas::CanvasChanged` 只讓 paint 失效 | C3-a：layout 標出 canvas，paint 向圖片來源要，renderer 讓畫過的 canvas 的 paint 失效 | C3-b（快照放進 fragment）：每畫一次都要重新 layout |
| 要求 frame（第 22 條） | `DidDraw` 之後要求下一個 frame | C4：有 canvas 等著畫時要求 main frame | 不要求：只畫 canvas 的動畫不更新 |
| 取樣（第 23 條） | `CanvasDefaultInterpolationQuality` 是 `kInterpolationLow`（雙線性、沒有 mipmap）；`pixelated`、`crisp-edges` 是 nearest | C5-a：照 Chromium | C5-b（和 `<img>` 一樣有 mipmap）：縮小時和 Chrome 不同 |

## 8. 1d 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `html_canvas_element.cc`（`DidDraw`、`DoDeferredPaintInvalidation`、`StyleDidChange`）。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| layer 的表示（第 24 條） | `PictureLayer`、`TextureLayer`、`SurfaceLayer` 都是 `cc::Layer`，照 paint 的順序在一個 list | E1-a：`Layer` 的列舉，照 paint 的順序 | E1-b（`PictureLayer` 的內容二選一）：一個型別兩種意思 |
| 縮放時的取樣（第 25 條） | canvas 的 layer `SetFilterQuality(kLow)`（雙線性）；`pixelated`、`crisp-edges` 是 `kNone` | E2-a：`Material::Texture` 帶 `nearest_neighbor`，canvas 用雙線性 | E2-b（只有 1:1 的才是 layer）：高解析度螢幕上沒乘 `devicePixelRatio` 的 canvas 成不了 layer |
| 畫過之後（第 26 條） | `DidDraw` 只 `SetShouldCheckForPaintInvalidation`；composited 的 2D canvas 由 `DoDeferredPaintInvalidation` 呼叫 `cc_layer_->SetNeedsDisplayRect`，foreign layer 的 item 不變 | E3-b：是 layer 的 canvas 不重新 paint，renderer 直接換 layer 的快照 | E3-a（一律重新 paint 和 layerize）：每個 frame 多跑一次；E3-b 要處理「是 layer 時畫過、之後不再是 layer」：記下沒重新 paint 的 canvas，下一次 paint 時一起讓它們失效 |

## 9. 1e 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas2d/canvas_path.cc`、`canvas_2d_recorder_context.{h,cc}`（原本在 `base_rendering_context_2d.cc` 的大多搬到這裡）、`canvas_rendering_context_2d_state.cc`、`clip_list.cc`、`platform/geometry/path_builder.cc`，和 Skia 的 `SkPathBuilder.cpp`、`SkGeometry.cpp`（`SkConic::BuildUnitArc`）、`SkRRect.cpp`。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| 幾何放在哪（第 27 條） | Blink 的 `PathBuilder` 包 `SkPathBuilder`：`AddEllipse` 把 sweep 換成角度，接近 360° 時分成兩個 180° 的 `arcTo`（`arcTo(…, 360, …)` 什麼都不畫），有旋轉時先把整條路徑換到橢圓的座標再換回來；`roundRect` 是 `addRRect`，radii 由 `SkRRect::setRectRadii` 照 CSS 的方式縮 | G-a：`gfx::Path` 的方法，移植 Skia | G-b（`dom_api` 裡）：Skia 層級的幾何在 DOM 那一層 |
| clip 跨過畫完的錄製（第 28 條） | 每層狀態的 `ClipList` 存換算到 device space 的路徑和 anti-alias；`save()` 不複製上一層的清單；畫完之後 `InitializeForRecording` → `RestoreMatrixClipStack` 替每層錄 `save`、`setMatrix(identity)`、它的 clip、它的 matrix | K-a：照 Chromium | K-b（每次畫都包上全部 clip）：op 數放大 |
| 快速路徑（第 29 條） | 一條線的 `stroke` 是 `DrawLineOp`；空路徑上半徑 ≥ 1 的 `arc` 是 `DrawArcOp`；其他是 `DrawPathOp`（帶 fill type） | F-a：一律 `DrawPath` | F-b（照做兩條）：多一種 op、`closePath` 的特別處理 |
| 和規範不同的地方（第 30 條） | 空路徑上的 `lineTo` 是 `M p L p`；負的寬或高的 `roundRect` 最後 `moveTo` 正規化之後的左上角；沒有目前的點時，曲線先 `moveTo` 沒換算的控制點；`roundRect` 先檢查 radii 的個數 | Q-a：照 Chromium | Q-b（照規範）：畫面和 Chrome 不同 |

不另外決定、照 Chromium 做的：`miterLimit` 預設 10；`fill`、`stroke` 和 `clip` 都 anti-alias（`clip_antialiasing_` 在畫面上的 canvas 由 `AntialiasedClips2dCanvasEnabled` 打開，預設開）；`lineWidth`、`miterLimit` 不是有限的或 ≤ 0 時忽略；`setLineDash` 有負的或不是有限的數時整個忽略、奇數個時重複一次（`getLineDash` 回傳重複之後的）、全部是 0 時沒有虛線；`lineDashOffset` 不是有限的時忽略，它是 `SkDashPathEffect` 的 phase；虛線和線寬在 user space；transform 不可逆時 `arc`、`ellipse`、`rect`、`roundRect` 不加，`fill`、`stroke`、`clip`、`clearRect` 不畫，`moveTo`、`lineTo`、曲線和 `arcTo` 的點換算到 bitmap 的座標，`resetTransform` 不換算路徑（回到變成不可逆之前）；`strokeRect` 用端點、接合和虛線，只有寬或高是 0 時是 `M L Z` 的路徑；`closePath` 在空路徑上什麼都不做；`rect` 寬和高都是 0 時只 `moveTo`；`arcTo` 沒有目前的點時只 `moveTo(p1)`，`p1` 是目前的點、`p1 == p2` 或半徑是 0 時 `lineTo(p1)`。`lineCap`、`lineJoin` 在 Chromium 的 IDL 是 `DOMString`，kun 照規範是 enum（不認得的值一樣忽略）。

## 10. 第 2 段的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas2d/base_rendering_context_2d.{cc,idl}`、`canvas_rendering_context_2d.cc`（`WillSetFont`、`ResolveFont`）、`canvas_rendering_context_2d_state.cc`、`core/html/canvas/canvas_font_cache.cc`、`text_metrics.cc`、`html_canvas_element.cc`（`GetTextDirection`）、`core/css/parser/css_parser.cc`（`ParseFont`）、`core/css/resolver/style_resolver.cc`（`ComputeFont`）、`platform/fonts/plain_text_node.cc`、`simple_font_data.cc`、`runtime_enabled_features.json5`。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| `font` 的解析（第 31 條） | 文件沒有 frame 時 setter 不做事；先 `UpdateStyleAndLayoutTreeForElement`，同一個字串、字型還有效時跳過；`ParseFont` 只走 `font` shorthand 的文法（CSS-wide keyword 和 `var()` 不收，system font 收）；`ComputeFont` 只套 size、family、stretch、style、variant-caps、weight（line-height 丟掉），相對照元素的 computed style，不吃 zoom 和最小字級；元素不在文件裡時照 `10px sans-serif`；兩層 LRU（每份文件 50 個，每個 context 一份） | N1：設定時解析和計算，同一個字串跳過，不做 LRU | N2（畫時才解析） |
| 哪些屬性影響排版（第 32 條） | 都改 `FontDescription`：spacing（一個長度的 dimension token，`em` 照目前的字級，換字型時重新換算）、kerning、stretch、variant-caps（字型沒有時合成）、text-rendering（hinting）、`lang`（locale） | P1：parley 做得到的都接上，`textRendering` 只記下 | P2（只有方向和 spacing） |
| 排版的介面（第 33 條） | context 直接用 `Font`，`PlainTextPainter::DrawWithBidiReorder` 錄成 `DrawTextBlobOp` | C1：`LayoutClient` 的新方法，呼叫時排，不改名 | C2（flush 時排） |
| 度量（第 34 條） | `GetFontBaseline`：top / bottom / middle 用正規化的 typo ascent / descent，hanging、ideographic、alphabetic 先看 BASE（沒有時 80% ascent、−descent、0）；`fontBoundingBox` 用 hhea | M1：照 Chromium | M2（都用 parley 的） |
| `TextMetrics`（第 35 條） | stable：寬度、actual 和 font 的 bounding box、三個 baseline（`TextMetricsBaselines`）；`emHeight*`、`getSelectionRects`、`getActualBoundingBox`、`getIndexFromOffset`、`getTextClusters` 是 experimental（`ExtendedTextMetrics`） | T1：照 stable 的 | T2（照規範全部） |

不另外決定、照 Chromium 做的：文字的前處理（ASCII 的空白 U+0009、U+000A、U+000C、U+000D 換成空格，長度不變、不合併；U+000B 不換，WPT 的 `2d.text.measure.width.space` 檢查它，2b 時更正）；`maxWidth` 比寬度小時水平縮放（`scale(maxWidth / width, 1)`），不是有限的數或 ≤ 0 時不畫；`textAlign` 的 `start` / `end` 照方向是左或右；空字串照樣走畫的流程；x 或 y 不是有限的數、transform 不可逆時不畫；`font` 和文字的屬性都是狀態的一部分，`reset()` 和改 `width` 回到 `10px sans-serif`；`font` 的 getter 照 Chromium 的形式（`italic`、`bold` 或數字的 weight、`small-caps`、px 的大小、family，沒有 line-height 和 stretch）；`direction` 的 getter 回傳字面值（`inherit` 也是）；`measureText` 的結果照 `textAlign`、`textBaseline` 和方向，不捨入。

## 11. 2a 的選項（2026-10-11 確認）

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| shorthand 裡的 small-caps 和 stretch（第 36 條） | `font` shorthand 收 `normal \| small-caps` 和 CSS3 的 stretch keyword，`SetFont` 把 variant-caps 和 stretch 從字型讀回狀態 | D1-a：canvas 的多收這兩種，頁面的不變 | D1-b（頁面也加 longhand） |
| system font（第 37 條） | `Font::ParseShorthand` 先看 `IsValidSystemFont`，`ConsumeSystemFont` 對到平台的字型 | D2-a：不收，記在限制 | D2-b（都是 `system-ui`） |
| `direction: inherit`（第 38 條） | 元素有 style 時用 computed `direction`，沒有時 `CachedDirectionality()`（`dir` attribute 和祖先算的） | D3-a：HTML 的 directionality | D3-b（一律 ltr） |

照 Chromium 做的：`letterSpacing` / `wordSpacing` 只收一個帶長度單位的 dimension token（`0`、`%`、`calc()`、前後的空白、好幾個 token 都不收），照原本的單位序列化，`em` 和 `rem` 照目前的字級；字型存在狀態裡（算好的值，和設定的字串，給「同一個字串跳過」用）；canvas 的文件沒有 frame 時 `font` 的 setter 不做事。不做 `lang`（不在這份的第 2 段的 API 裡，是比較新的成員）。

## 12. 2b 的選項（2026-10-11 確認）

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| 排版的方法（第 39 條） | 畫和量用同一個 `Font`：`ComputeInlineSize` 量寬度，`DrawWithBidiReorder` 畫，`TextMetrics::Update` 量 | B1-a：`LayoutClient` 一個方法，畫和量共用 | B1-b（各一個） |
| canvas 的 raster 快取（第 40 條） | Skia 的 typeface 和 glyph 的快取整個 process 共用 | B2-a：renderer 給所有 canvas 一份，跨 flush | B2-b（每次新建） |

照 Chromium 做的：base direction 照 `direction`（`inherit` 照第 38 條），kun 沒有 `unicode-bidi`，沒有 bidi override；每個 glyph run 一個 `DrawGlyphs`，`fillText` 用填色的、`strokeText` 用線的樣式的 flags；對齊和基準線的位移算進位置，`maxWidth` 是 `SetMatrix` 裡的水平縮放；x、y 不是有限的數、`maxWidth` 不是有限的數或 ≤ 0、transform 不可逆、文件沒有 frame 時不畫；粗體和斜體的合成照頁面的（600 的門檻）。沒有 LCD 的字（Chromium 只在不透明的 software canvas 上可能有）。

## 13. 2c 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `core/html/canvas/text_metrics.{cc,idl}`、`canvas2d/base_rendering_context_2d.cc`（`measureText`）、`platform/fonts/skia/skia_text_metrics.cc`、`runtime_enabled_features.json5`。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| ink bounds 的來源（第 41 條） | `MeasureRuns` 聯集每個 run 的 `InkBounds()`（沒有墨跡的不算）；每個 glyph 的 bounds 在 macOS 是路徑的，其他平台是 `SkFont::getBounds`（SkGlyph 的整數 bounds）；4px 以下用路徑（`CanvasTextMetricsPreciseBounds`，experimental） | I1-a：Skia 的路徑的 bounds，點陣字退回 `getBounds` | I1-b（`getBounds`）、I1-c（`layout` 讀 bbox） |
| 什麼時候算（第 42 條） | 只有 `measureText`（`DrawTextInternal` 只量寬度） | I2-a：`shape_canvas_text` 的參數 | I2-b（每次都算） |

規範：`measureText` 跑 text preparation algorithm，用它的 inline box 建 `TextMetrics`；`actualBoundingBox*` 是「the bounding rectangle of the given text」，單位是 CSS px，相對 `textAlign` 的對齊點和 `textBaseline` 的線，沒有定精確度和取法（WPT 都是範圍檢查）。

照 Chromium 做的：`TextMetrics` 是 `dom` 的 GC 型別（和 `DOMRect` 一樣），數值在 `measureText` 時算好；成員照第 35 條（`TextMetricsBaselines` 是 stable，`emHeight*` 和 `getSelectionRects` 這些是 experimental 的 `ExtendedTextMetrics`）；文件沒有 frame 時回傳全部是 0 的（Chromium 的 `MakeGarbageCollected<TextMetrics>()`），client 不排字時也是；`TextMetrics::Update` 的算法：`actualBoundingBoxLeft` 是 −bounds.x + dx、`Right` 是 bounds.right − dx（dx 照 `textAlign` 和方向：靠左 0、置中寬度的一半、靠右寬度），垂直的都減去 `textBaseline` 的位移（`fontBoundingBox` 用 hhea，`actualBoundingBox` 用 ink，三個 baseline 相對目前的 `textBaseline`）；不捨入；空字串的寬度和 ink 是 0（`actualBoundingBoxAscent` 是 −位移）；文字和畫的時候一樣做前處理。

## 14. 2d 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas2d/canvas_rendering_context_2d.cc`（`ResolveFont`）、`canvas_rendering_context_2d_state.cc`（`SetFont`、`FontsNeedUpdate`）、`skia/ext/test_fonts.h`、`third_party/test_fonts/README.chromium`；WPT 的 `../third_party/wpt/docs/writing-tests/ahem.md`。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| 靠 web font 的 WPT（第 43 條） | canvas 的字型綁上頁面的 font selector，`@font-face` 和 `FontFaceSet` 照常用；web test 另有固定的測試字型，只管預設字型 | W1-a：`text/` 整個加進來，48 個寫成預期的失敗、附上原因 | W1-b（這份裡做 web font）、W1-c（WPT 的字型裝成本機字型） |

照 1f 做的：先用 `--all` 跑一次 `text/`，沒過的照原因分類（web font、experimental 的 `TextMetrics` 成員和 `fillTextCluster` 這類 `.tentative`、`lang`、和 Chrome 不同的地方），其他的當 bug 修；kun 自己的 reftest 比對 canvas 的文字和頁面（Ahem 的方塊、`measureText` 和 `<span>` 的寬度）；`about:demo` 和 `canvas` 情境加上文字；Speedometer 的盤點；第 2 段的收尾檢查。

## 15. 第 3 段的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas2d/canvas_2d_recorder_context.{h,cc}`（`Draw`、`DrawInternal`、`CompositedDraw`、`IsFullCanvasCompositeMode`、`drawImage` 的 `GetSourceImageForCanvas` 和 `WouldTaintCanvasOrigin`）、`canvas_rendering_context_2d_state.cc`（陰影的 `DrawLooper` 和 `DropShadowPaintFilter`）、`canvas_style.h`。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| 分段 | — | 3a 漸層、3b 圖片和圖案、3c 合成和陰影、3d `Path2D` 和 hit test、3e `putImageData`、3f WPT 和收尾 | — |
| 狀態怎麼持有漸層和圖案（第 44 條） | `CanvasStyle` 持有 `Member<CanvasGradient>` / `Member<CanvasPattern>`，畫的時候 `ApplyToFlags` 產生 shader | E1：context 上被 trace 的 slot，每層兩個 | E2（存 shader 的快照） |
| 多載（第 45 條） | 從 IDL 產生的 bindings 照 WebIDL 解析多載 | O-a：宣告表支援多載，WIT 每種形式一個函式 | O-b（最長的形式加 `Optional`）、O-c（`[WitOnly]` 加 `glue.js`） |
| `drawImage` 的圖片（第 46 條） | `GetSourceImageForCanvas`：呼叫當下的 frame，錄成 `PaintImage` | I1：經過 `LayoutClient` 在呼叫當下拿 renderer 的圖片 | I2（錄 URL，flush 時才換） |
| origin-clean（第 47 條） | `WouldTaintCanvasOrigin`、`SetOriginTaintedByContent`；圖案和 canvas 來源傳下去；`getImageData` 丟 `SecurityError` | T1：3b 就做，`Opaque` 的圖片 taint | T2（之後再做） |

已知、留到各步驟定的：陰影在 Chromium 有兩條路（不透明的畫用 `DrawLooper`，圖案和不透明以外的圖用 `DropShadowPaintFilter`，都不受 transform 影響），`source-in`、`source-out`、`destination-in`、`destination-atop` 這四種合成模式和不相容的陰影經過 `CompositedDraw` 的 `saveLayer`，`copy` 先清掉整個畫布（3c）；`isPointInPath` / `isPointInStroke` 的幾何（SVG 的 hit test 用 kurbo，Chromium 用 Skia，3d）；`putImageData` 的寫法（3e）。

## 16. 3b 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas2d/canvas_2d_recorder_context.cc`（`drawImage`、`createPattern`、`WouldTaintCanvasOrigin`）、`canvas_rendering_context_2d_state.cc`（`SetImageSmoothingQuality`）。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| 多載的解析（第 48 條） | 從 IDL 產生的 bindings 照 WebIDL 完整的演算法 | A1：個數，再看 interface 對其他型別；WIT 的名字寫在宣告裡 | A2（完整的演算法）、A3（照個數編號） |
| `DOMMatrix2DInit`（第 49 條） | `CanvasPattern.setTransform`、`ctx.setTransform(init)`、`getTransform()` 回傳 `DOMMatrix` | C1：dictionary 和兩個 `setTransform`，沒有 `DOMMatrix` | C2（連 `DOMMatrix`）、C3（都不做） |

照 Chromium 做的：圖片來源只有 `<img>` 和 `<canvas>`；壞掉的 `<img>` 是 `InvalidStateError`，還沒載好、大小是 0 的不畫，寬或高是 0 的 `<canvas>` 是 `InvalidStateError`；canvas 來源先畫完、用它的快照；負的寬高翻正，來源矩形照圖裁、目的矩形照比例跟著裁；`DrawImage` 多了來源矩形和 flags（cc 的 `DrawImageRectOp`）；`imageSmoothingEnabled` 關掉時最近的像素，`imageSmoothingQuality` 的 low 雙線性、medium 加 mipmap、high 放大時 cubic、縮小時 mipmap；`createPattern` 的 repetition（`repeat`、空字串和 `null` 也是 `repeat`、`repeat-x`、`repeat-y`、`no-repeat`，其他是 `SyntaxError`），圖還沒好時回傳 `null`，建立時拿下快照，不重複的軸外面不畫（`kDecal`），用 cc 的 image shader（kun 的 `TiledShader` 加上 local matrix）；renderer 的圖片表記下 response 的 tainting，畫了 `Opaque` 的圖、不 clean 的 canvas，或 style 設成不 clean 的圖案時 canvas 不 clean，重設也不變回來，`getImageData` 丟 `SecurityError`。

## 17. 3c 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas2d/canvas_2d_recorder_context.{h,cc}`（`DrawInternal`、`CompositedDraw`、`BlendModeRequiresCompositedDraw`、`BlendModeSupportsShadowFilter`、`IsFullCanvasCompositeMode`、`ResetAlphaIfNeeded`、`setShadow*`、`setGlobalCompositeOperation`、`ParseCanvasCompositeAndBlendMode`）、`canvas_rendering_context_2d_state.{h,cc}`（`GetFlags`、`ShouldDrawShadows`、`ShadowOnlyDrawLooper`、`ShadowAndForegroundImageFilter`、`SetGlobalComposite`）、`core/style/shadow_data.h`（`BlurRadiusToStdDev`）。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| 合成模式在 display list 裡（第 50 條） | `PaintFlags::setBlendMode`，fill、stroke、image 三份 flags | M1：`PaintFlags` 加 blend mode | M2（不是 `source-over` 的都包一層 layer） |
| 開 layer 的 op（第 51 條） | `SaveLayerOp`（有 flags）和 `SaveLayerAlphaOp` | L1：新增有 flags 的 `SaveLayer`，`SaveLayerAlpha` 留著 | L2（擴充 `SaveLayerAlpha`）、在 `dom_api` 用反向的 clip |
| 陰影（第 52 條） | 不透明的畫用 looper，圖案、不透明以外的圖片和要 composite 的用 `DropShadowPaintFilter` | S2：一律用 drop shadow 的 image filter 的 layer | S1（照 Chromium 兩條路）、S3（錄兩次 op） |

照 Chromium 做的：模式的名稱（Porter-Duff 的 12 種，`lighter` 是 `kPlus`，和 `multiply` 到 `luminosity` 的 16 種），不認得的字串不改，讀回正規的名稱；`copy` 先清掉整個 clip 的範圍（不透明的畫布是黑色）再用 `kSrc` 畫，不畫陰影；`source-in`、`source-out`、`destination-in`、`destination-atop` 用單位矩陣、有 blend mode 的 layer 裡用 `source-over` 畫；`shadowOffsetX` / `Y` 不是有限的數時不改，`shadowBlur` 不是有限的數或負的不改，sigma 是它的一半；`shadowColor` 照 `fillStyle` 的顏色解析和序列化（包括 `currentcolor`），預設是透明黑；只有顏色不是全透明、而且模糊或位移不是 0 時畫陰影；陰影和本體在模式是 `source-over`、`source-atop`、`destination-out` 時一個 layer，其他分兩個；不透明的畫布在會改到目的 alpha 的模式之後，在 clip 裡用 `kDstOver` 補黑色；`clearRect` 不受模式和陰影影響。

## 18. 3d 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas2d/path_2d.{h,idl}`（建構子、`addPath`）、`canvas2d/canvas_2d_recorder_context.cc`（`IsPointInPathInternal`、`IsPointInStrokeInternal`）、`canvas2d/canvas_path.{h,cc}`、`platform/geometry/path.cc`（`Path::Contains`、`StrokeContains`、`StrokePath`）。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| hit test 的幾何（第 53 條） | Skia 的 `SkPath::contains` 和 `getFillPath`，不經過 renderer | H2：`gfx` 的 kurbo 幾何，和 SVG 共用 | H1（經過 `LayoutClient` 用 Skia） |
| `Path2D` 的字串（第 54 條） | `BuildPathFromString`，和 SVG 共用 | B1：組路徑的程式移到 `computed_style`、解析的入口在 `style`，共用 | B2（另寫一份）、B3（經過 `LayoutClient`） |

照 Chromium 做的：`Path2D` 是 GC 型別、存一條路徑，沒有 edge；建構子收另一個 `Path2D`（複製）或字串（出錯時留下之前的部分）；`addPath` 的 `DOMMatrix2DInit` 不一致是 `TypeError`，不是有限的數時不加，接在後面（`SkPath::addPath` 的 append）；`Path2D` 的路徑方法和 context 的一樣，但沒有 transform；context 的 `fill` / `stroke` / `clip` / `isPointInPath` / `isPointInStroke` 多了收 `Path2D` 的形式（WIT 的名稱加 `Path2d` 結尾），畫的是當下的快照；hit test 的點不是有限的數或 transform 不可逆時是 `false`，點用 transform 的反矩陣換算，`isPointInStroke` 用目前的線寬、端點、接合、miter 和虛線。

## 19. 3e 的選項（2026-10-11 確認）

Chromium 的部分 2026-10-11 對照 `main` 的 `canvas2d/base_rendering_context_2d.cc`（`createImageData`、`putImageData`、`PutByteArray`）、`canvas2d/canvas_rendering_context_2d.cc`（`WritePixels`）、`canvas2d/canvas_image_data.idl`。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| `putImageData` 寫像素（第 55 條） | 先 flush（蓋住整個畫布時丟掉錄製），再寫 backing | W1：`dom_api` 直接寫 `CanvasData` 的像素 | W2（錄 `WritePixels` 的 op） |
| `[EnforceRange] long`（第 56 條） | IDL 有，超出範圍是 `TypeError` | E1：宣告表加這個型別，三個方法都用 | E2（照舊用 `long`） |

照 Chromium 做的：`createImageData(sw, sh, settings)` 取絕對值、0 是 `IndexSizeError`、色彩空間預設是 context 的；`createImageData(imageData)` 是同樣大小和色彩空間的透明黑；`putImageData` 的兩種形式，髒矩形的負的寬高照 Blink 正規化、切到 `ImageData` 和畫布裡、空的不寫，`dirtyWidth × dirtyHeight` 超過 `int` 時不做，buffer 被 detach 是 `InvalidStateError`；不換算色彩空間；WIT 的名稱 `putImageDataPart`、`createImageDataOf`；`new ImageData(data, sw, sh)` 不在這一段。

## 20. 3f 的選項（2026-10-11 確認）

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| WPT 的目錄（第 57 條） | — | W1：範圍內的都加 | W2（只加第 3 段的五個） |

照之前的收尾做的：先用 `--all` 跑，沒過的照原因分類，範圍外的功能和已知的不同寫成預期的失敗、附上原因，其他當 bug 修（大的先問）；kun 自己的 reftest 比對 canvas 和頁面上同樣的東西（不透明的線性漸層和 `linear-gradient`、1:1 的 `drawImage` 和 `<img>`、`repeat` 的圖案和 `background-repeat`、沒有模糊的陰影和移過去的方塊）；`about:demo` 多一個第 3 段的 canvas 和按鈕，情境多截圖；Speedometer 只跑 kun 那一側；第 3 段的收尾檢查。

## 21. 第 4 段的選項（2026-10-12 確認）

Chromium 的部分 2026-10-12 對照 `main` 的 `core/html/canvas/html_canvas_element.cc`（`toDataURL`、`toBlob`、`ToDataURLInternal`）、`platform/image-encoders/image_encoder.cc`（`Encode`、`ComputeJpegQuality`、`ComputeWebpOptions`）和 `image_encoder_utils.cc`（`ToEncodingMimeType`）、`platform/graphics/image_data_buffer.cc`（`ToDataURL`），和 Skia 的 `src/encode/SkPngRustEncoderImpl.cpp`（`kLow` 是 `Level1WithUpFilter`）。

| 事項 | Chromium | 選定 | 沒選的和理由 |
| --- | --- | --- | --- |
| 格式（第 58 條） | PNG、JPEG、WebP，其他退回 PNG | F1：PNG 和 JPEG，WebP 退回 PNG | F2（連 WebP）、F3（只有 PNG） |
| 在哪裡編碼（第 59 條） | Blink 的 `ImageEncoder`：PNG 是 `SkPngRustEncoder`（`png` crate）、JPEG 是 `SkJpegEncoder` | E1：`raster` 照同樣的編碼器和設定，經過 `LayoutClient` | E2（`dom_api` 用純 Rust）、E3（`dom_api` 依賴 `raster`） |
| `toBlob` 的非同步（第 60 條） | 呼叫時拿快照，PNG 在 idle task、JPEG / WebP 在 worker thread 編，task 裡呼叫 callback | A1：呼叫時編碼，task 裡呼叫 callback | A2（背景的 thread） |
| `quality`（第 61 條） | `any`，只認數字 | Q1：照 IDL | Q2（`kun:dom` 另一個形式） |
| `toBlob` 給 wasm 嗎（第 62 條） | 沒有 wasm 的 DOM | J1：只給 JS | J2（`kun:dom` 的 guest 多一個 export） |

照 Chromium 做的：不是 origin-clean 是 `SecurityError`（"Tainted canvases may not be exported."）；寬或高是 0：`toDataURL` 是 `"data:,"`，`toBlob` 在 task 裡給 callback `null`；沒有 context 的畫布照樣編（透明）；格式比對前轉成 ASCII 小寫，不認得的是 PNG；JPEG 的品質只認 0 到 1 的數字（`quality * 100 + 0.5` 取整），其他是 92；編碼的是畫完錄下的 op 之後、不是 premultiplied 的像素；`toDataURL` 是 `data:<type>;base64,`，`toBlob` 的 Blob 的 type 是編成的格式。

## 22. 4c 的選項（2026-10-12 確認）

先用 `--all` 跑了 `html/semantics/embedded-content/the-canvas-element/`：70 個測試，`toDataURL` / `toBlob` 的幾乎都通過，11 個不如預期。

| 事項 | 選定 | 沒選的和理由 |
| --- | --- | --- |
| WPT 的範圍（第 63 條） | W1：整個目錄，失敗的附上原因 | W2（只加輸出和 `security.*` 的）：通過的 `context.*`、`fallback.*` 沒人守 |
| `about:demo`（第 64 條） | D1：`toBlob` → blob URL → `<img>` | D2（`toDataURL`）：沒走非同步的路 |
| reftest（第 65 條） | R2：不加 | R1：和 browser 的測試、WPT 重複 |

找到的 bug（照 Chromium 修）：改了 `width` 或 `height` 時 canvas 應該變回 origin-clean。Blink 的 `HTMLCanvasElement::OnWidthOrHeightAssigned` 在 2D 的 context 重設時把 `origin_clean_` 設回 true；context 的 `reset()`（`Canvas2DRecorderContext::ResetInternal`）只清掉 content 的 taint，元素的 `origin_clean_` 不變，所以 Chrome 的 `security.reset.*` 也失敗（Chromium 的 `-expected.txt`）。kun 原本兩種都不變回（3b 時以為 Blink 都不變），`security.resize.*` 失敗。
