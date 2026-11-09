# RFC 0081 的細節

## kun 現在的做法

2026-10-23，`main` 的 `d7e323cb`。

**錄製**（`crates/dom_api/src/canvas.rs`）：

- 每個繪圖呼叫經過 `record`、`record_under`、`record_plain` 到 `record_ops`，照目前的 transform、`globalCompositeOperation` 和陰影（`composite`）錄成 `PaintOp`，放進 `CanvasData::ops`。
- `record_draw` 告訴 `dom` 這個 canvas 有東西要畫（`take_canvas_draws` 的集合）。
- 錄滿 `MAX_RECORDED_OPS`（10,000）時 `flush`：經過 `LayoutClient::flush_canvas` 到 renderer。

**raster**（`crates/renderer/src/canvas.rs`）：

- `flush`：拿走 `CanvasData::ops`，放進一個 `DisplayItemList`，`raster::draw_into` 用 Skia 畫進 `CanvasData::pixels`（第一次時配置，`pixels_mut`）。`draw_into` 每次用借來的像素包一個 Skia 的 surface。
- `flush_shown`：frame 之前，對每個有東西要畫、在有 frame 的 document 裡的 canvas 呼叫 `flush`；不在 document 裡的留著錄製，等讀像素或錄滿。

**快照**（`crates/dom/src/canvas.rs` 的 `CanvasData::snapshot`）：

- 整份複製 `pixels` 成 `gfx::ImageData`（`bytes` 是 `Arc<dyn AsRef<[u8]> + Send + Sync>`），存著，像素變了（`pixels_mut`）才重新複製。
- `FrameView::take_canvas_textures` 和 `FrameImages::canvas_layer` 在 frame 的時候拿它：canvas 是自己的 layer 時換 texture layer 的內容（`compositor` 的 `TextureLayer`），否則重新 paint，畫進 tile。

**讀像素**（`crates/dom_api/src/canvas.rs`）：

| 呼叫 | 做法 |
| --- | --- |
| `getImageData` | `flush`，再從 `CanvasData::pixels` 複製矩形，un-premultiply |
| `putImageData` | 蓋住整個 bitmap 時丟掉錄製，否則 `flush`；再在 main thread 上寫像素（premultiply），`record_draw` |
| `toDataURL`、`toBlob` | `flush`，複製 `pixels`，在 main thread 上編碼 |
| `drawImage`、`createPattern` 的來源是 canvas | 來源 `flush`，拿它的快照（`SourceImage`） |

**快取**：`raster::RasterCache`（typeface 和解碼過的圖片）有鎖，compositor 的 raster worker 已經共用它。renderer 的 main thread 有一份（`RenderThread::raster_cache`，canvas 的 raster 和 `measureText` 的 ink 用），compositor 有自己的一份。

**型別**：`CanvasData` 有 `const` 斷言它是 `Send + Sync + 'static`；`PaintOp` 裡的字型和圖片是 `Arc`。

## Chromium 的做法

- **錄製**：`BaseRenderingContext2D` 的呼叫錄進 `Canvas2DRecorderContext` 的 `MemoryManagedPaintRecorder`（`cc::PaintRecord`）。
- **flush 的時機**：
  - frame 之前（`CanvasRenderingContext2D::FinalizeFrame`、`PrepareTransferableResource`）；
  - 讀像素（`GetImageData` → `FlushCanvas`）；
  - 錄製超過上限（`FlushIfRecordingLimitExceeded`，以錄製的大小和 pin 住的圖片的大小計）。
- **加速的 canvas**（有 GPU 的預設）：
  - `CanvasResourceProviderSharedImage`：flush 時經過 `gpu::raster::RasterInterface` 把 `PaintOp` 序列化進 command buffer（OOP raster），GPU process 的 `RasterDecoder` 用 Skia（Ganesh 或 Graphite）重播進 shared image。main thread 只序列化，不等。
  - `getImageData`：`CanvasResourceProvider::ReadPixels`，同步等 GPU process 讀回。
  - `putImageData`：`WritePixels`，經過 command buffer，不等。
  - 給 compositor 的是 `TextureLayer` 的 `TransferableResource`（shared image 的 mailbox 和 sync token），compositor 不等 main thread；Viz 用 sync token 等 GPU 畫完。
- **CPU 的 canvas**：`CanvasResourceProviderBitmap`，在 main thread 上用 Skia 畫，和 kun 現在一樣。用在：
  - 沒有 GPU（或 GPU 被封鎖）；
  - `willReadFrequently: true`（`RasterModeHint::kPreferCPU`）；
  - 加速的 canvas 讀回之後：沒設 `willReadFrequently` 時，`BaseRenderingContext2D::getImageData` 每次把 `read_count_` 加一，到 `kFallbackToCPUAfterReadbacks`（2）就 `DisableAcceleration`，之後都是 CPU；一份 document 裡改成 CPU 的 canvas 超過 `kDisableAccelerationThreshold`（100）個、佔 `kDisableAccelerationPercent`（95%）以上時，新的 canvas 一開始就是 CPU（`html_canvas_element.cc` 的 `DisabledAccelerationCounterSupplement`）。
- **小的 canvas**：`HTMLCanvasElement::ShouldAccelerate` 在 `AcceleratedSmallCanvases` 關掉時不加速小於 `kMinimumAccelerated2dCanvasSize`（128×129 個像素）的 canvas；這個 feature 現在是 stable，只有 Webview 還用門檻。
- **上限**：
  - `FlushIfRecordingLimitExceeded`：錄製的 op 超過 `kMaxRecordedOpKB`（2 MB；Graphite 是 `kMaxRecordedOpGraphiteKB`，6 MB），或釘住的圖片超過 `kMaxPinnedImageKB`（32 MB）時 flush。照位元組算，`PaintOpBuffer` 是連續序列化的，大小本來就有。
  - frame：`CanvasRenderingContext2D::FinalizeFrame` 在 canvas 顯示著、連續 `kMaxCanvasAnimationBacklog`（2）個 frame 沒有 commit 時建一個 `SharedContextRateLimiter`，之後每個 frame `Tick()`，GPU 落後超過 2 個 frame 就讓 main thread 等。
  - script 中途的 flush 沒有另外的上限，command buffer 滿了才等 GPU process。
- 以上是 2026-10-23 的 Chromium `main`（`base_rendering_context_2d.cc`、`canvas_rendering_context_2d.cc`、`html_canvas_element.cc`、`blink/common/features.cc`、`runtime_enabled_features.json5`）。
- **canvas 當來源**：`HTMLCanvasElement::Snapshot` 給 `StaticBitmapImage`；加速的是 `AcceleratedStaticBitmapImage`（引用 shared image，copy-on-write），畫到別的 canvas 時錄成 `PaintImage`，不在 main thread 上複製像素。
- kun 對應的位置：kun 沒有 GPU process 那一側的 raster（tile 的 raster 也在 renderer 的 compositor thread 上），所以「另一側」是 renderer 裡的一條 thread，重播用 CPU 的 Skia。

## Speedometer 的量測和用到的 API

**量測**（2026-10-23，Windows 11，i9-13900K；2026-10-23 那一版的 `docs/performance-gaps.md`（2026-11-06 整份改寫之前）裡「Windows 上」那一節）：

| | kun | Chrome | 差 |
| --- | ---: | ---: | ---: |
| Charts-chartjs 的 `drawPointLegend`（`--profile`，每個 iteration 的 ms） | 71.8 | 2.6 | +69.2 |
| Charts-chartjs 的 `draw` | 14.0 | 1.3 | +12.7 |
| canvas 的 raster，`--sample` 取樣到的：Charts-chartjs | 61.2 | — | — |
| 同上：Perf-Dashboard（frame 之前，算在 `(program)`） | 52.1 | — | — |
| 兩個 suite 和 Chrome 的差（比較表） | | | +186.7 |

- kun 的成員表裡，`drawPointLegend` 的時間在共用路徑的 callback 底下（`fill`、`stroke`），錄到第 10,000 個 op 的那次呼叫當場畫。
- chart.js 每個點是一條有 conic 的抗鋸齒路徑，畫在抗鋸齒的 clip 底下；Skia 的熱點是逐條 edge 的 scan conversion（`SkAnalyticQuadraticEdge`、`RunBasedAdditiveBlitter::blitAntiH`、`SkAAClipBlitter`）、path 的 convexity 和 bounds、NT heap 的配置。這些都是純量的分支邏輯，prebuilt 的 Skia 的 raster pipeline 只有 SSE2，SIMD 幫不了多少。
- Linux（Fedora 44）上的 `perf`（`cycles:u`，只算使用者空間）在 `drawPointLegend` 裡只看到約 7 ms，當時推測 main thread 在等；Windows 上以 CPU 時間計、連 kernel，main thread 是在 Skia 裡畫。兩個平台為什麼不同還沒查；移出 main thread 之後兩種情況都不在 main thread 上，所以不先查。第 1 段做完時在兩個平台上各取樣一次，確認 main thread 上沒有 raster。

**用到的 API**（`../third_party/speedometer/resources`，2026-10-23 grep）：

| 檔案 | 讀像素的呼叫 | 在計時的路徑上嗎 |
| --- | --- | --- |
| `charts/dist/assets/chartjs-*.js` | `toDataURL` 1（`toBase64Image`）、`drawImage` 1（點的樣式是圖片或 canvas 時） | 不在：10-23 的 `--profile` 的 kun 成員表裡沒有 `drawImage`、`toDataURL`、`getImageData`、`putImageData`、`createPattern` |
| `perf.webkit.org/public/v3/bundled-scripts.js` | 沒有（只有 `getContext`、`scale`、`clearRect`、填色） | — |
| `react-stockcharts/build/static/js/*.js` | `drawImage` 4、`toDataURL` 3 | 同上，不在成員表裡；這個 suite 主要是 SVG |

所以兩個 canvas 的 suite 在計時的路徑上不讀像素，raster 移出去之後不會被同步等待吃回來。

## 沒選的做法

### compositor thread

- **做法**：錄製交給 compositor thread，和 tile 一起 raster；有 `--num-raster-threads` 時交給 raster worker。
- **好處**：重用 compositor 的 raster 和 worker pool，`RasterCache` 已經在那裡；對應 kun 的 tile raster 的位置。
- **問題**：
  - 錄滿 10,000 個 op 是在 script 的中途，不在 commit 的時候。compositor 和 main thread 之間現在的通道是 commit 和 BeginFrame，要另外開一條送錄製的路。
  - 讀像素時 main thread 同步等 compositor thread。要證明 compositor thread 這時不會在等 main thread（例如等 commit），否則會互相等待。Chromium 的 main thread 也不同步等 compositor thread（commit 除外）。
  - 大的 canvas 畫得慢時，compositor thread 處理不了 BeginFrame、捲動和輸入。
- 專用的 thread 只收命令、不等任何人，沒有這些問題。

### Viz

- **做法**：照 Chromium，錄製經過 `protocol` 送到 Viz，Viz 持有 canvas 的資源並重播。
- **好處**：拆 process 之後和 Chromium 一樣在 GPU process 那一側；之後換 GPU raster 在原地換。
- **問題**：
  - 要序列化 `PaintOp`；字型和圖片要有跨 process 的快取（Chromium 的 `TransferCache`、`ServiceFontManager`、`ServiceImageTransferCacheEntry`），現在 `PaintOp` 只帶 `Arc`。
  - 讀像素是跨 process 的同步呼叫。
  - kun 的 tile raster 在 renderer 裡，只有 canvas 在 Viz 裡，兩套 raster 的位置不一致。
- 等上 GPU、拆 process 時再一起考慮（「之後的可能」）。

### 留在 main thread，讓 raster 變快

- 例如小的路徑不經過抗鋸齒的 clip、換成 vello_cpu、從原始碼編 AVX2 的 Skia。
- 熱點大多是純量的 scan conversion 和配置，就算快一倍，時間還是在 main thread 上、在 script 的同步時間裡。Chrome 快的原因是 raster 不在 main thread 上。

### 每個 canvas 一條 thread、或一個 thread pool

- 一個頁面的 canvas 通常很少；多條 thread 要處理 canvas 之間的順序（把 canvas 畫到另一個 canvas），失去單一 FIFO 佇列的簡單。
- 之後真的有很多大 canvas 同時畫的頁面，再把 raster thread 換成 pool，按 canvas 分配、跨 canvas 的引用加上等待。
