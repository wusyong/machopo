# RFC 0033：Canvas 2D

- 開始日期：2026-10-10
- 狀態：✅ 已完成：2026-10-12（S1、P2、D1、T1、W1；分四段：「畫布和路徑」（1a 到 1f）、「文字」（2a 到 2d），2026-10-11 做完；「圖片和樣式」（3a 到 3f）、「輸出」（4a 到 4c），2026-10-12 做完；驗收在 `progress.md`）
- 細節：`details.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 N，雜項線，和 O（RFC 0031，SVG）互不依賴、並行。

## 摘要

讓 kun 有 `<canvas>` 和它的 2D context：`HTMLCanvasElement` 是 replaced element，`getContext("2d")` 給出 `CanvasRenderingContext2D`，畫路徑、矩形、文字，讀回像素。畫的指令照 Chromium 錄成 `compositor_paint` 的 paint op（Blink 錄進 `cc::PaintRecord`），存在 `dom` 裡的 context 上；renderer 用現有的 `raster`（Skia）把錄好的指令畫進同樣存在 `dom` 的像素裡，rendering update 時畫（Chromium 的 composited canvas 延到 compositor 要它的 resource 時才畫），script 讀像素時當場畫；canvas 照 Chrome 平常的做法是自己的 compositor layer（`cc::TextureLayer`）：canvas 重畫時只換那一層的 texture，不重新 paint、不重新 raster 頁面的 tile；compositor 和 Viz 只會平移 layer，所以 canvas 或它的祖先有 rotate、scale 這類 transform 或 opacity 時，退回把快照當成一張圖畫進頁面的 display list（Chromium 的 canvas 沒有 shared image 的 backing 時也是這樣畫）；文字經過 renderer 的 `TextContext`（parley）排版，和頁面的文字用同一套字型。分四段做：畫布和路徑（連同 `getImageData`，WPT 的 canvas 測試大多靠它讀像素）、文字、圖片和樣式（漸層、圖案、`drawImage`、合成、陰影、`Path2D`、`putImageData`、origin-clean）、輸出成檔案（`toDataURL` 和 `toBlob`，用和 Chrome 一樣的編碼器和設定的 PNG 和 JPEG）。Speedometer 的三個 suite 用到的是前兩段，做完之後都不再停在 canvas。

## 動機

**現況（2026-10-10，`main` 的 `376eefff`；細節在 `details.md` 第 1 節）**：
- `<canvas>` 只是 `HTMLElement`（`dom_api` 的 `html_element` 的表裡它是「HTML 定義、沒有自己的 interface」的名稱）：沒有 `HTMLCanvasElement`、`getContext`、`width` / `height`。`docs/limitations.md` 沒有記這一條。
- layout 把 `<canvas>` 當成一般的元素：不是 replaced element，沒有 300×150 的預設大小，它的子元素（fallback content）照樣排版、畫出來；有 script 的頁面裡 Chrome 不畫它們。
- display list 和 raster 只認編碼好的圖（`gfx::ImageData` 是 PNG、JPEG、GIF 的 bytes，raster 時解碼），`DrawPath` 只有多邊形，沒有曲線、線的端點和接合的樣式。
- **Speedometer 的盤點**（2026-10-10，`rfcs/0012-speedometer/progress.md`）：Charts-chartjs 跑完但丟例外（`Failed to create chart: can't acquire context from the given item`）、React-Stockcharts-SVG 停在 `this.testCanvas.getContext is not a function`、Perf-Dashboard 停在 `alert`、之後也要 canvas。三個 suite 用到的 API 是同一組（`details.md` 第 1 節）：路徑、矩形、`clip`、transform、`globalAlpha`、虛線、`fillText`、`measureText`，沒有 `getImageData`、`drawImage`、漸層和 `Path2D`。canvas 是這三個 suite 都要、但不是唯一缺的：Charts-chartjs 還要 `MessageChannel`（J）、`ResizeObserver`（M）；React-Stockcharts-SVG 還要 SVG（O）、custom element 和 shadow DOM（K-1）；Perf-Dashboard 還要 shadow DOM（K-1）和 `alert`。

目標：
- `HTMLCanvasElement`：`width`、`height`（改了就重設畫布和 context 的狀態）、`getContext("2d")`；replaced element（大小照 attribute 和 CSS，預設 300×150），fallback content 不畫。
- `CanvasRenderingContext2D` 的 HTML 規範定義的成員，除了下面「不在範圍內」的：狀態（`save`、`restore`、`reset`）、transform、填色和線的樣式、路徑、`fill` / `stroke` / `clip`、矩形、文字和 `TextMetrics`、`CanvasGradient`、`CanvasPattern`、`drawImage`、合成和陰影、`Path2D`、`isPointInPath`、`ImageData` 和像素的讀寫、`toDataURL` / `toBlob`。
- JS 和 wasm 都有（RFC 0012 的第 7 條；第 5 條）。
- WPT 的 `html/canvas/element/` 裡用得到的部分。

不在範圍內（「之後的可能」）：
- `OffscreenCanvas`、worker 裡的 canvas、`transferControlToOffscreen()`。
- 其他 context：WebGL、WebGPU、`bitmaprenderer`；`getContext` 給它們 `null`。
- GPU 加速的 canvas（GPU raster）：canvas 由 CPU 的 Skia 畫，結果經過 compositor 的 layer 交給 Viz。
- canvas 的 layer 在 rotate、scale、opacity 底下：等 compositor 和 Viz 會處理平移以外的 transform 和 layer 的 opacity；在那之前退回畫進頁面。
- `filter`、`ImageBitmap` 和 `createImageBitmap()`、`captureStream()`、`drawFocusIfNeeded()`、`colorSpace` 和 `colorType` 的 float16、`willReadFrequently` 以外的效能提示。
- `<video>` 當 `drawImage` 的來源（kun 沒有 `<video>`）、SVG 檔的 `<img>` 當來源（`<img>` 載得到 SVG 檔，canvas 還不畫它）。
- WebP 的編碼（`image/webp` 給 PNG，第 58 條）、wasm 的 `toBlob`（第 62 條）。
- `alert`：Perf-Dashboard 也要，但它是會擋住 script 的 user prompt（WebDriver 也有規定），和 canvas 無關，另外處理。

## 設計概要

對寫網頁的人：
- `<canvas width="200" height="100">` 佔 200×100，裡面的 fallback content 不畫；`canvas.getContext("2d")` 給出 context，`ctx.fillStyle = "red"; ctx.beginPath(); ctx.arc(50, 50, 40, 0, Math.PI * 2); ctx.fill()` 在下一個 frame 畫出紅色的圓；`ctx.getImageData(50, 50, 1, 1).data` 當場讀到 `[255, 0, 0, 255]`。改 `canvas.width` 清掉畫布和狀態。CSS 的大小和 attribute 不同時，畫布照 CSS 的大小縮放（Chart.js 這類函式庫自己把 `width` 設成 CSS 寬度乘上 `devicePixelRatio`）。
- `ctx.font = "12px sans-serif"; ctx.fillText("Hi", 10, 20)` 畫出文字，`ctx.measureText("Hi").width` 和頁面上同樣字型的寬度一致。
- 漸層、圖案、`drawImage(img, …)`、陰影、`globalCompositeOperation`、`Path2D`、`isPointInPath`、`putImageData` 照 Chrome 畫；畫過別的 origin 的圖片之後，像素讀不回來、也輸出不了。
- `canvas.toDataURL()` 和 `toBlob()` 給出 PNG（`image/jpeg` 是 JPEG），用和 Chrome 一樣的編碼器和設定；`toBlob` 的 callback 在之後的 task 裡拿到 `Blob`。

對改 kun 的人（第 2 到 4 條）：
- 宣告放在新的 `dom_api/src/interfaces/modules_canvas.rs`（Blink 的 `modules/canvas`），`HTMLCanvasElement` 在 `core_html.rs`；演算法在 `dom_api` 的新模組 `canvas`。
- context 是 `dom` 的 GC 型別，帶著狀態的堆疊、目前的路徑、還沒畫的 paint op 和畫布的像素。畫的指令只是錄下來，`dom_api` 不碰 Skia。
- renderer 在 rendering update 的 pre-paint 之前，把有新指令的 canvas 交給 `raster` 畫進它的像素；`getImageData` 經過借給 script 的 `LayoutClient` 當場畫（同步的 readback）；錄的指令超過上限時也先畫（第 7 條）。
- layout 多一種 replaced element（Blink 的 `LayoutHTMLCanvas`）。paint 給 canvas 記一個自己一個 chunk 的 display item，它的 op 是把快照當成一張已經解碼的圖畫進頁面（`gfx::ImageData` 多一種「解碼好的像素」）；layerize 判斷它能不能成為 layer：能的話那個 chunk 變成自己的 `TextureLayer`（新的 layer 種類，cc 的 `TextureLayer`），快照是那一層的 resource，畫成 Viz 的 texture quad；不能的話（平移以外的 transform、opacity）照一般的 chunk 畫進頁面（第 12 條）。
- `compositor_paint` 的 `DrawPath` 改成一般的路徑（`SkPath` 的對應：直線、二次曲線、conic、三次曲線、閉合，f32）和 fill rule，加上 `ClipPath`；`arc` 和 `ellipse` 照 Skia 的 `arcTo` 做成 conic；`PaintFlags` 加上 `lineCap`、`lineJoin`、`miterLimit` 和任意長度的虛線。路徑的型別和這些擴充由 SVG 那邊先做、先 merge（第 6 條），canvas 照用。

## 詳細設計

（整體的選項在 `details.md` 第 2 節，各段和小段的選項在第 5 到 22 節。）

- **元素**（第 1 段）：`html_element` 的表加上 `canvas` → `HTMLCanvasElement`。`width`、`height` 是 `unsigned long` 的 reflection，預設 300、150；設定時（連同直接改 attribute）照規範重設畫布：像素清成透明黑（`alpha: false` 時是不透明的黑）、context 的狀態和路徑回到預設。`getContext(contextId, options)`：`"2d"` 第一次建 context、之後給同一個；其他 id 給 `null`；`options` 照第 10 條：`alpha: false` 的畫布是不透明的，`willReadFrequently`、`desynchronized`、`colorSpace` 只記下、由 `getContextAttributes()` 回報。
- **畫布的大小**（第 1 段）：照 Chromium 的 `IsValidImageSize`（第 9 條）：面積最多 32768×8192、每邊最多 65535、寬高不是 0；不合格時沒有像素，畫的呼叫什麼都不做，`getImageData` 給全 0，元素照樣照它的大小排版。像素在第一次畫或讀的時候才配置，用可以失敗的配置（Rust 的配置失敗會 abort），失敗時和不合格的一樣。
- **layout**（第 1 段）：`layout` 的 `is_replaced` 加上 `<canvas>`，自然大小是畫布的大小（CSS px），沒有子 box（fallback content 不建 box）。
- **paint 和 layer**（第 1 段）：canvas 照 `object-fit`（目前只有預設的 `fill`）畫進內容框，取樣照 `image-rendering`（目前只有 smooth）。
  - **display item**（第 12 條）：paint 一律給 canvas 記一個 canvas 種類的 display item，自己一個 chunk（Blink 的 `RecordForeignLayer` 也是自己的 chunk），帶著 canvas 的 id 和內容框；它的 op 是把快照當成解碼好的圖畫進內容框。是不是 layer 由 layerize 決定，paint 不用知道。
  - **composited**（照 Chrome 平常的路徑）：layerize 在 paint 順序的那個位置切開，canvas 的 chunk 自己一層 `TextureLayer`，之前和之後的 chunk 照舊是 picture layer（現有的切法：之後畫的、蓋到它的內容在它上面的 layer）。條件和捲動容器成為 layer 的一樣，兩者共用一個函式：到 root 的 transform 只有平移、沒有 opacity、最多一個圓角的 clip。canvas 被畫過時，renderer 只更新那一層的快照、要求 commit，不重新 paint，頁面的 display list 和 tile 都不動（Blink 的 `TextureLayer::SetNeedsDisplayRect`，第 26 條）；raster invalidation 不看成為 layer 的 canvas 的 item。
  - **畫進頁面**（fallback）：不能成為 layer 時，那個 chunk 照一般的 chunk 進 picture layer，item 的 op 把快照畫進頁面；快照換了，item 不沿用、重新 raster 它碰到的 tile。Blink 在 paint 時就決定（`IsComposited()`），因為它的條件只看有沒有 shared image 的 backing，和 transform 無關；kun 的條件看 property tree，所以放在 layerize。
- **compositor 和 Viz**（第 1 段，第 13 條）：`compositor` 多一種 `TextureLayer`（main 端帶著快照：大小和像素的 `Arc<[u8]>`，加上遞增的版本號；`alpha: false` 的是 `contents_opaque`，照 Blink 的 `SetContentsOpaque(is_opaque_)`），commit 時交給 impl 端；impl 端在版本換了時，經過 `SharedImageInterface` 建新的 shared image（和 tile 的 texture 一樣；沒有 GPU 時留著像素），draw 時出一個 texture quad（`Material::Texture`，cc 的 `TextureDrawQuad`）。舊的 shared image 在 frame 不再用它之後釋放（和 tile 一樣靠參考計數，`docs/architecture.md` 的「Process 的邊界」的例外 1；Chromium 是 Viz 用完之後經過 release callback 還回來，拆 process 時和 tile 一起改成那樣）。Viz 照現有的 texture quad 畫，`tex_coord_rect` 是整張畫布，縮放到內容框。
- **錄製**（第 1 段起）：`CanvasRenderingContext2D` 的每個畫的呼叫照當下的狀態（transform、clip、`globalAlpha`、樣式）錄成 paint op，接在 context 的待畫清單後面。路徑照 Blink 存在當下的 user space（第 8 條）：加點時不乘上 transform，換 transform 時把已有的路徑換算到新的 user space；transform 不可逆時改存成 canvas 的座標，之後加的點才乘上 transform（Blink 的 `CanvasPath`）。`fill`、`stroke`、`clip` 都錄成「當下的 CTM 加上 user space 的路徑」，和第 3 段的 `Path2D` 同一條路。原本打算加點時就乘上 transform（規範的字面），但 `stroke` 要再用反矩陣換回 user space，`Path2D` 又是另一套表示，所以改照 Blink。clip 是 context 狀態的一部分，`restore` 時回復，錄成 save / clip / restore。`clearRect` 錄成清除的 op（Skia 的 `kClear` 混合）。
- **raster**（第 1 段起）：`raster` 加一個「把 paint op 接著畫進一份既有的像素」的入口（Blink 的 `CanvasResourceProvider` 用 Skia 的 surface 包住像素）；renderer 在 rendering update 的 pre-paint 之前，對有待畫指令的 canvas 呼叫它、清空待畫清單（第 7 條）；錄的指令超過上限時，下一個畫的呼叫前先畫（Blink 的 `FlushIfRecordingLimitExceeded`）。Blink 平常在畫過的 task 結束時 `FinalizeFrame`，composited 的 canvas 在 `Canvas2dDeferredFlushEnabled` 時延到 compositor 要它的 resource（`PrepareTransferableResource`）才畫；kun 的 commit 接在 paint 之後，rendering update 時畫對應的是後者，同一個 frame 裡好幾個 task 畫過也只 raster 一次（kun 在 main thread 上同步用 CPU 畫）。像素是 context 自己的一份（`Vec`），快照是另一份：像素變了之後第一次要快照時複製一份（可以失敗的配置），沒變就沿用同一個（第 20 條）。原本打算照 Chromium 讓像素和快照共用、畫的時候 copy-on-write，但 kun 把快照畫進頁面時，display list 一直拿著上一張快照，畫下一張之前 copy-on-write 也一定要複製，成本一樣，畫的路徑卻要多處理共用的情況。script 讀像素（`getImageData`、`toDataURL`、`toBlob`）時，`dom_api` 經過借給 engine 的 `LayoutClient`（第 11 條；文字的排版、`drawImage` 的圖片和編碼器也加在它上面，名稱沒改）請 renderer 當場畫完。
- **文字**（第 2 段）：`font` 用 `style` 解析 CSS 的 `font` shorthand，相對的單位（`em`、`%`、`larger`）照 canvas 元素的 computed style（元素不在文件裡時照預設的 `10px sans-serif`），script 設定時經過 client 讓 style 更新到那個元素（Blink 的 `UpdateStyleAndLayoutTreeForElement`）。排版經過 client 交給 renderer 的 `TextContext`（parley），回傳 glyph 和位置，錄成 `DrawGlyphs`；`measureText` 回傳同一次排版的 `TextMetrics`。
- **樣式和圖片**（第 3 段，第 44 到 57 條）：`CanvasGradient`、`CanvasPattern`、`Path2D` 是 `dom` 的 GC 型別，context 每一層狀態的 slot 撐住它的漸層或圖案（`save` 之後下一層也撐住，換掉、pop、重設時放掉）。`drawImage` 和 `createPattern` 在呼叫時經過 `LayoutClient` 拿 renderer 解碼好的 `<img>`，canvas 的來源先畫完、用它的快照。origin-clean 記在 context 上：別的 origin 的圖片（`<img>` 一律 `no-cors`）、有這種圖片的 canvas 或圖案（設成樣式時）讓讀像素和輸出是 `SecurityError`；改了 `width` / `height` 才變回 origin-clean，`reset()` 不會（Blink 的 `OnWidthOrHeightAssigned`；規範是兩者都會）。`globalCompositeOperation` 是 `PaintFlags` 的 blend mode，要 layer 的模式（`source-in` 這類）和陰影用有 flags 的 `SaveLayer`，陰影一律是 drop shadow 的 image filter 的 layer。`Path2D` 和 SVG 共用組路徑的程式（`computed_style`），`isPointInPath` / `isPointInStroke` 用 `gfx` 的 kurbo 幾何，也和 SVG 共用。`putImageData` 在 `dom_api` 直接寫畫布的像素（先畫完錄下的 op）。宣告表因此多了多載（先比個數、再比 interface，`kun:dom` 的名稱用 `as` 給）和 `[EnforceRange] long`。
- **輸出**（第 4 段，第 58 到 62 條）：編碼器在 `raster`：PNG 是 `png` crate，照 Chrome 的 `SkPngRustEncoder` 的 `kLow`（deflate level 1、Up filter，不透明時沒有 alpha）；JPEG 是 Skia 的 `SkJpegEncoder`，照 Blink 的 `ImageEncoder` 的選項（預設品質 92、alpha 合成到黑色、品質 100 時 4:4:4），都先轉回不是 premultiplied 的像素。`dom_api` 經過 `LayoutClient::encode_image` 呼叫，照 Blink 的順序：不是 origin-clean 是 `SecurityError`，沒有像素是 `data:,` 或 `null`，再畫完錄下的 op、取像素（沒有 context 是透明的）。格式字串照 ASCII 小寫比對，不認得的（包括 `image/webp`）是 PNG；`quality` 是 `any`，只認 0 到 1 的數字。`toBlob` 在呼叫時就編碼（Chrome 也在呼叫時拿快照，但 PNG 在 idle task 裡分段編、JPEG 在 worker thread 上編），callback 在 canvas blob serialization 的 task 裡拿到那時建的 `Blob`；文件沒有 window 時什麼都不做。`toBlob` 只給 JS：它的 `BlobCallback` 是新的 callback 種類，`kun:dom` 每一種 callback 要 guest 的一個 export。
- **wasm**：每個 interface 照宣告表進 `kun:dom`；`fillStyle` 這類 `(DOMString or CanvasGradient or CanvasPattern)` 的 union 照現有的 `unions!`。`getContextAttributes()` 是第一個回傳字典的成員，宣告表因此多了這個形式（第 1 段做的）：JS 是每次一個新的一般物件，WIT 是參數用的同一個 record，Lua 的 table 欄位照參數用 snake_case（Lua 的產生器原本把回傳的 record 的欄位寫成 kebab-case，之前回傳的 record 只有一個字的欄位，沒有人碰到）。WIT 的名稱裡，數字後面的大寫字母留在同一個字（`CanvasRenderingContext2D` 是 `canvas-rendering-context2d`，不是 `context2-d`；`Uint8Array` 照舊是 `uint8-array`）。
- **Process 的邊界**：沒有新的訊息。canvas 的 layer 走現有的 commit 和 `CompositorFrame`：texture quad 的 resource 是 `TransferableResource`（shared image，或沒有 GPU 時的 `pixels`），和 tile 一樣；建 shared image 照樣是例外 1 的同步呼叫，拆 process 時和 tile 一起改。fallback 畫進頁面的快照和 `ImageData` 一樣在 display list 裡（`Arc<[u8]>`），只有 raster 搬出 renderer（OOP-R）時才跨邊界。

### 測試

- `dom_api` 的單元測試：`width` / `height` 的 reflection 和重設、`getContext` 給同一個 context 和其他 id 的 `null`、狀態的堆疊、每個呼叫錄成什麼 op（transform 乘進路徑、`restore` 回復 clip）。
- `raster` 的單元測試：接著畫進既有的像素、新的路徑和線的樣式。
- `layout`、`paint`：`<canvas>` 的自然大小、CSS 縮放、fallback content 不排版；layerize 把 canvas 切成自己的 layer、之後畫的內容在它上面、平移以外的 transform 和 opacity 底下退回畫進頁面；fallback 的快照換了之後重新 raster。
- `compositor`：`TextureLayer` 的 commit、版本換了才建新的 shared image、texture quad 的位置和縮放、canvas 重畫時 picture layer 的 tile 不重新 raster；`viz` 的 headless 畫出 texture quad。
- WPT 的 `html/canvas/element/`：testharness 的測試大多用 `getImageData` 讀像素（`_assertPixel`），所以 `getImageData` 放在第 1 段；第 1 段加了 `path-objects/`、`line-styles/`、`transformations/`、`the-canvas-state/`、`drawing-rectangles-to-the-canvas/`、`canvas-host/`，第 2 段 `text/`，第 3 段 `fill-and-stroke-styles/`、`drawing-images-to-the-canvas/`、`compositing/`、`shadows/`、`pixel-manipulation/`、`reset/`、`canvas-context/`、`conformance-requirements/` 和直接在它底下的六個檔，第 4 段 `html/semantics/embedded-content/the-canvas-element/`（`toDataURL`、`toBlob`、origin-clean）；沒通過的寫在 `.ini`，附上原因。
- kun 自己的 reftest（`canvas/c1` 到 `c6`、`hidpi/c3-canvas-layer.html`）：canvas 和頁面上同樣的東西逐像素比較：矩形、內容框、layer、路徑和 `border-radius`、文字和 `<span>`、漸層、圖片、圖案、陰影、`putImageData`。
- 情境和 `about:demo`：四個 canvas（方塊和 idle callback、路徑和「New size」、文字和「Bigger text」、樣式和圖片，點它做 hit test 和 `putImageData`，「Copy as image」經過 `toBlob` 和 blob URL 顯示在 `<img>`）。
- `browser` 的測試在真的 renderer 裡畫、讀回像素，輸出的 PNG 經過 `<img>` 畫回來比較。
- Speedometer 的盤點（`ci.py speedometer --inventory`）：第 2 段之後三個 suite 不再停在 canvas。
- mutation test：每一段照 `docs/testing.md` 的「Mutation test」。

### 分階段

1. **畫布和路徑**：`HTMLCanvasElement`、replaced element、canvas 的 `TextureLayer` 和 fallback、context 的狀態和 transform、純色的 `fillStyle` / `strokeStyle`、`globalAlpha`、線的樣式和虛線、路徑（`moveTo` 到 `roundRect`）、`fill` / `stroke` / `clip`、`fillRect` / `strokeRect` / `clearRect`、`getImageData` 和 `ImageData`。
2. **文字**：`font`、`textAlign`、`textBaseline`、`direction`、`letterSpacing` 這一類、`fillText`、`strokeText`、`measureText` 和 `TextMetrics`。做完時 Speedometer 的三個 suite 用到的 canvas API 都有了。
3. **圖片和樣式**：`CanvasGradient`（線性、放射、圓錐）、`CanvasPattern`、`drawImage`（`<img>` 和 `<canvas>`）、`globalCompositeOperation`、陰影、`imageSmoothingEnabled`、`Path2D`、`isPointInPath` / `isPointInStroke`、`putImageData`、`createImageData`。
4. **輸出**：`toDataURL()`、`toBlob()`（PNG 和 JPEG）。

每一段又分成幾小段（`progress.md` 的「步驟」），各段開始前決定那一段的事；第 1 到 3 段做完、經過審核時各自 merge 回 `main`（`rfcs/README.md` 的「流程」第 5 步）。

## 缺點

- **第 1 段比較大**：除了 DOM、layout 和 raster，還要新的 layer 種類、layerize 的 foreign layer、resource 的版本和釋放、Viz 的 texture quad，加上 fallback 的畫進頁面；兩條路都要測。
- **layer 變多**：每個 composited 的 canvas 把頁面的 picture layer 切成前後兩層（之後的內容在新的一層），頁面上有很多小 canvas 時 layer 數跟著增加（Blink 有 layer 的合併，kun 的 layerize 沒有）。
- **fallback 的路徑還是會重新 raster 頁面的 tile**：rotate、scale、opacity 底下的 canvas 每畫一次，頁面上它碰到的 tile 整塊重畫。
- **CPU 畫**：大的 canvas、每個 frame 重畫時，Skia 的 CPU raster 比 Chrome 的 GPU 慢，每次換版本還要上傳一次 texture，Speedometer 量到的時間會偏高。Chrome 在小的 canvas（小於 128×129）和 `willReadFrequently` 時也用 CPU raster。
- **`getImageData` 同步地畫**：讀像素前要先把錄下的指令畫完（Chrome 也是，`willReadFrequently` 就是為了這個）。
- **`toBlob` 在 main thread 上編碼**：大的 canvas 會卡住 script 和畫面（Chrome 在 idle task 和 worker thread 上編，第 60 條）。
- **宣告表多十幾個 interface、上百個成員**，JS 和 wasm 都有，WIT 和 golden file 變大。
- **和 SVG 同時改 `compositor_paint` 的路徑**：兩條線都要一般的路徑和線的樣式；由 SVG 那邊先做這一塊、單獨先 merge，canvas 在那之前不改它們，用到路徑的部分排在第 1 段的後面（第 6 條）。SVG 那邊定的型別缺 canvas 要的東西（conic、`ClipPath`、虛線的 phase）時，canvas rebase 之後再補。

## 理由和其他做法

（完整的比較在 `details.md` 第 2 節。）

- **錄成 paint op、由 renderer 用 `raster` 畫（D1）**：和 Chromium 的分層一樣（Blink 錄進 `cc::PaintRecord`，`CanvasResourceProvider` 用 Skia 畫），raster 的結果、反鋸齒和頁面一致，Skia 照舊只在 `raster`。不選 D2（`dom_api` 直接依賴 `raster` 和 Skia）：`dom_api`、`dom_js`、`dom_wasm` 的測試都要 link Skia；也不選 D3（另一個 2D 函式庫，例如 tiny-skia、vello_cpu）：`docs/architecture.md` 的選型是只用 Skia，結果才和 Chromium 一致。
- **canvas 自己的 compositor layer（P2）**：Chrome 平常走的就是這條（GPU raster，或 CPU raster 寫進 shared image，`IsComposited()` 都是真的），canvas 重畫時不重新 paint、不重新 raster 頁面的 tile，每個 frame 重畫的圖表（Speedometer 的三個 suite）只換一層的 texture。不選 P1（只把快照當圖畫進頁面）：compositor 和 Viz 不用改、第 1 段比較小，但那是 Chrome 的 fallback，每次重畫都要重新 raster 頁面上它碰到的 tile，之後要改成 P2 還要再做一次。fallback 照樣要有：kun 的 compositor 和 Viz 只平移 layer，和捲動容器一樣，transform 和 opacity 底下的 canvas 不能自己一層。
- **文字經過 renderer 的 `TextContext`（T1）**：字型的選擇、fallback 和頁面一樣，`measureText` 和頁面上的寬度一致。不選 T2（`dom_api` 自己一份 parley 的 context）：兩份字型的快取，結果可能和頁面不同。
- **`getImageData` 放在第 1 段**：WPT 的 canvas 測試大約一半用它讀像素，沒有它第 1 段沒有規範的測試。
- **一份涵蓋 2D context 全部、分四段（S1）**：一份文件說明錄製、raster 和畫面的整體設計，各段共用。不選只做 Speedometer 用到的（S2）：剩下的部分（漸層、`drawImage`、`Path2D`）是常見的用法，之後還要再開一份描述同一套設計。

## 先例

- **Chromium**（2026-10-10 對照 `main` 的原始碼，`details.md` 第 3 節）：`HTMLCanvasElement`（`core/html/canvas`）、`LayoutHTMLCanvas`（replaced）、`HTMLCanvasPainter`；context 在 `modules/canvas/canvas2d`（`CanvasRenderingContext2D`、`BaseRenderingContext2D`、狀態的 `CanvasRenderingContext2DState`、路徑的 `CanvasPath`），畫的指令錄進 `MemoryManagedPaintRecorder`（`cc::PaintRecord`），畫過的 task 結束時的 `FinalizeFrame`（composited 的 canvas 在 `Canvas2dDeferredFlushEnabled` 時延到 `PrepareTransferableResource`）、讀像素時、錄的超過上限時 flush，交給 resource provider 畫：有 shared image 的 backing（GPU raster，或 CPU raster 寫進可以 map 的 shared image）時 canvas 是 composited，`HTMLCanvasPainter` 記一個 foreign layer（`cc::TextureLayer`）；沒有時（`CreateSoftwareSurface`）`HTMLCanvasPainter` 把快照當成圖畫進頁面。字型用 `CanvasFontCache` 快取解析好的 `font`，相對的單位先 `UpdateStyleAndLayoutTreeForElement`，字型由頁面的 `StyleEngine::ComputeFont` 算。kun 照這個分層：平常是 composited 的路徑，不能成為 layer 時退回畫進頁面。
- **Firefox**（沒有對照原始碼）：`HTMLCanvasElement`、`CanvasRenderingContext2D` 直接畫進 Moz2D 的 `DrawTarget`（Skia 或 Direct2D），canvas 是自己的 layer（WebRender 的 image）。
- **Servo**（沒有對照原始碼）：2D context 的指令送到另一個 thread（canvas paint thread）用 Raqote / Vello 畫，結果當 WebRender 的 image；和 kun 的差別是指令即時畫、不錄下來。

## 已決定的事項

2026-10-11 確認第 1 到 5 條（比較在 `details.md` 第 2 節）。

1. **S1：這份涵蓋 `HTMLCanvasElement` 上的 2D context 全部，分四段**（2026-10-11）：畫布和路徑（連同 `getImageData`）、文字、圖片和樣式、輸出；各段的細節在開始前定。不選 S2（只做 Speedometer 用到的）：漸層、`drawImage`、`Path2D` 很常見，之後還要一份描述同一套設計；沒有 `getImageData` 就沒有 WPT。
2. **P2：canvas 是自己的 compositor layer，照 Chrome 平常的做法；不能成為 layer 時退回畫進頁面**（2026-10-11）：paint 記 foreign layer 的 display item，layerize 給它一層 `TextureLayer`，快照經過 shared image 交給 Viz，畫成 texture quad；canvas 重畫不動頁面的 display list 和 tile。kun 的 compositor 和 Viz 只平移 layer，所以條件和捲動容器成為 layer 的一樣（只有平移、沒有 opacity），其他的把快照當成解碼好的圖畫進頁面（Chromium 沒有 shared image 的 backing 時的做法）。不選 P1（只畫進頁面）：compositor 和 Viz 不用改，但那是 Chrome 的 fallback，每次重畫都要重新 raster 頁面的 tile，之後還要再改一次。
3. **D1：`dom_api` 錄成 `compositor_paint` 的 op，renderer 用 `raster` 畫，像素存在 `dom`**（2026-10-11）：op 和像素都在 `dom` 的 context 上，跟著元素的 GC 釋放；rendering update 時畫，script 讀像素時經過借給 engine 的 client 當場畫。和 Chromium 的分層一樣（`MemoryManagedPaintRecorder`、`FinalizeFrame`；Chromium 平常在畫過的 task 結束時畫，2026-10-11 對照原始碼更正，畫的時機見第 7 條）。不選 D2（`dom_api` 直接依賴 `raster`）：`dom_api`、`dom_js`、`dom_wasm` 的測試都要 link Skia；不選 D3（另一個 2D 函式庫）：違反只用 Skia 的選型；不選 D4（像素存在 renderer 的表）：元素被 GC 之後 renderer 不知道。
4. **T1：文字經過 client 交給 renderer 的 `TextContext`**（2026-10-11）：字型、fallback、度量和頁面一致，照 Chromium 用頁面的 `StyleEngine::ComputeFont`。不選 T2（`dom_api` 自己一份 parley）：兩份字型的快取，結果可能和頁面不同。
5. **W1：JS 和 wasm 都有**（2026-10-11）：照 RFC 0012 的第 7 條。canvas 和導覽、CSSOM 不同，在 wasm 文件裡一樣有用；成員都是數字、字串和 union，WIT 照現有的規則產生。不選 W2（先只給 JS）。

2026-10-11 確認第 6 到 13 條，第 1 段「畫布和路徑」要的（比較和 Chromium 的對照在 `details.md` 第 5 節）。

6. **A1：路徑的型別是 `SkPath` 的對應，由 SVG 那邊先定、先 merge 回 `main`，canvas 照用**（2026-10-11）：canvas 要的是 f32，move、line、quad、conic、cubic、close；`DrawPath` 帶 fill rule，加上 `ClipPath`（`cc::ClipPathOp`）；`PaintFlags` 的端點、接合、miter 和任意長度、帶 phase 的虛線；現有的多邊形（邊框、sad tab、工具列）改用它。`arc`、`ellipse` 照 Skia 的 `arcTo`（Blink 的 `PathBuilder::AddEllipse`）做成 conic，移植 `SkConic::BuildUnitArc`，曲線的像素和 Chrome 最接近；SVG 用 kurbo 算出的路徑轉進來。誰來做：同一天決定由 SVG（RFC 0031）的第 2 段的第一步做這一塊，審查後單獨先 merge（例外記在 RFC 0031）；canvas 在那之前不改 `compositor_paint` 的 `DrawPath`、`PaintFlags` 和 `raster` 畫路徑的地方，第 1 段先做和路徑無關的部分（`progress.md` 的步驟），rebase 到那一塊之後才做路徑和線的樣式，缺的（例如 conic、`ClipPath`、`arcTo` 的移植）那時再補。canvas 這邊先寫好、沒有用到的草稿在 branch 外（只存成 patch，交給 SVG 那邊參考）。不選 A2（`PaintOp` 放 `kurbo::BezPath`）：沒有 conic、f64、paint op 綁在外部 crate 上；不選 A3（攤平成多邊形）：放大時失真、接合錯。第 3 段的 `isPointInPath` 在 `dom_api` 裡沒有 Skia，那時再定自己算 winding 或經過 client。
7. **F1：rendering update 的 pre-paint 之前畫，讀像素時、錄的超過上限時也畫**（2026-10-11）：同一個 frame 裡好幾個 task 畫過只 raster 一次；對應 Chromium 的 composited canvas 延到 `PrepareTransferableResource` 才畫（`Canvas2dDeferredFlushEnabled`）。不選 F2（照 Chromium 平常的做法，畫過的 task 結束時畫）：要 event loop 在 task 結束時的 hook，kun 的 raster 在 main thread 上同步跑，每個 task 都畫比 Chromium 把指令交給 GPU 貴。
8. **R2：路徑照 Blink 存在當下的 user space，換 transform 時換算**（2026-10-11）：`fill`、`stroke`、`clip` 和第 3 段的 `Path2D`、`isPointInPath` 都是「user space 的路徑加上 CTM」，錄下的 op 和 Chromium 一樣；transform 不可逆時照 Blink 改存成 canvas 的座標。不選 R1（加點時就乘上 transform，規範的字面）：`stroke` 要用反矩陣換回 user space，和 `Path2D` 是兩套表示。
9. **Z1：畫布大小的上限照 Chromium 的 `IsValidImageSize`，像素用到時才配置**（2026-10-11）：面積最多 32768×8192、每邊最多 65535；不合格或配置失敗時沒有像素，畫的呼叫不做事、`getImageData` 給全 0、`getContext` 照樣給 context。配置用可以失敗的方式（Rust 的配置失敗會 abort）。不選 Z2（比 Chromium 小的上限）：介於兩者之間的大小在 kun 上畫不出來；不選 Z3（不設上限）：網頁能讓 renderer abort。kun 沒有 Chromium 的 hibernation，背景分頁的 canvas 照樣佔著像素。
10. **C1：`alpha: false` 完整做，`willReadFrequently`、`desynchronized`、`colorSpace` 只記下、回報**（2026-10-11）：`alpha: false` 的畫布從不透明的黑開始、`clearRect` 清成黑、raster 用不透明的 alpha type、layer 是 `contents_opaque`（Blink 的 `SetContentsOpaque(is_opaque_)`、`SetBlendBackgroundColor(!is_opaque_)`）。`willReadFrequently` 在 Chromium 讓 canvas 留在 CPU 上畫，kun 本來就是，不加 Chromium 的 console 警告；`desynchronized` 的 low-latency 和其他色彩空間在範圍外。不選只記下 `alpha`、照透明的畫：畫面和 Chrome 不同。
11. **K1：`LayoutClient` 加上畫 canvas 的方法**（2026-10-11）：讀像素前請 renderer 畫完（第 2 段的文字也加在它上面，那時決定要不要改名）；出借和它的 `unsafe` 論證不變，文字要的 style 更新本來就在它上面，對應 Blink 經過 document 走到 frame 的 lifecycle。Chromium 的 context 直接擁有 `CanvasResourceProvider`，不需要 client；kun 要它，是因為 Skia 只在 renderer（第 3 條）。不選 K2（另一個 `CanvasClient`）：`dom` 多一處 `NonNull` 的出借和論證，每個執行 script 的入口借兩份，文字還是要兩個 client。
12. **L1：paint 一律記 canvas 的 display item、自己一個 chunk，layerize 決定它是 `TextureLayer` 或畫進頁面**（2026-10-11）：item 的 op 是 fallback 的「快照當圖」；成為 layer 的條件和捲動容器共用一個函式（只有平移、沒有 opacity、最多一個圓角 clip）。Blink 在 paint 時用 `IsComposited()` 決定，它的條件只看 backing、和 transform 無關（cc 什麼 transform 都能合成）；kun 的條件看 property tree，本來就是 layerize 的事。不選 L2（paint 時決定、記兩種 item）：paint 要重算 layerize 的條件，兩邊可能不一致。
13. **V1：`TextureLayer` 的快照帶遞增的版本號，靠參考計數釋放，和 tile 一樣**（2026-10-11）：版本換了 impl 端才建新的 shared image，舊的在沒有 frame 用到之後釋放；沒有 GPU 時像素直接放進 `TransferableResource`。Chromium 是 Viz 用完之後經過 release callback 還回來（`PrepareTransferableResource` 綁的 `EndDisplayCompositorAccess`）；kun 的 tile 沒有這條路，拆 process 時兩者一起改。不選 V2（現在就做 release callback）：只為 canvas 多一套 resource 的回收。

2026-10-11 確認第 14 到 18 條，第 1 段的 1b（context 的狀態、矩形和 `getImageData`）要的（比較和 Chromium 的對照在 `details.md` 第 6 節）。

14. **B1-b：`clearRect` 錄成新的 `ClearRect` op，raster 用 `kClear` 的 paint 畫；`alpha: false` 的畫布錄成黑色的 `DrawRect`**（2026-10-11）：Chromium 錄成 blend mode 是 `kClear` 的 `drawRect`（不透明的畫布用黑色），結果一樣。不碰 `PaintFlags`，和 SVG 那邊的改動（第 6 條）不衝突；是暫時的形式：第 3 段的 `globalCompositeOperation` 在 `PaintFlags` 加 blend mode 時，改回 Chromium 的寫法、拿掉這個 op。不選 B1-a（現在就在 `PaintFlags` 加 blend mode）：改到 SVG 那邊正在改的 struct；不選 B1-c（`clearRect` 等到 1e）：Chart.js 每個 frame 都先 `clearRect`。
15. **B2-d：`ImageData` 是 `dom` 的 GC 型別，照 Chromium，JS 文件裡的像素是 engine 的 `Uint8ClampedArray`**（2026-10-11）：寬、高和色彩空間在 `dom`；JS 的 `data` 由 `ScriptHost` 建立，存在 `ValueSlot`（`CustomEvent.detail` 的做法），每次是同一個物件、script 寫進去的留著（Chromium 的 `ImageData` 是 `ScriptWrappable`，`data_u8_` 是 V8 heap 上的陣列）；wasm 文件裡存在不被 trace 的 bytes，guest 拿到一份複本（和 `getRandomValues` 一樣）。不選 B2-c（JS 的 glue.js 裡的 class）：不能序列化、brand check 和形狀要自己做、JS 和 wasm 兩套；不選 B2-a（`data` 每次一份複本）：違反 `[SameObject]`，寫進去的會不見。
16. **B3-a：`fillStyle` 和 `strokeStyle` 在設定時解析，`currentcolor` 照 Chromium 讀 canvas 元素的 inline style 的 `color`，沒有時是黑色**（2026-10-11）：Chromium 的 `CanvasRenderingContext2D::GetCurrentColor` 只看 inline style（元素不在文件裡、沒有 inline style 時是黑色），不更新 style。和規範（元素的 computed `color`）不同，記在 `docs/limitations.md`。不選 B3-b（照規範用 computed `color`、設定時經過 `LayoutClient::update_style`）：結果和 Chrome 不同，設定一次更新一次 style。
17. **B4：錄到 10,000 個 op 就先畫；rendering update 只畫這次更新的 frame 的文件裡 connected 的 canvas**（2026-10-11）：Chromium 也有上限（`FlushIfRecordingLimitExceeded`，它的常數沒有對照到），kun 的常數寫在一處、之後好改。`dom` 記下哪些 canvas 有還沒畫的指令（和 `<img>` 的變更一樣），不在文件裡的 canvas 不出現在畫面上，等讀像素或超過上限才畫。
18. **B5：`dom` 依賴 `compositor_paint`**（2026-10-11）：op 存在 `dom` 的 context 上（第 3 條）；`compositor_paint` 只依賴 `gfx`、不碰 V8。不選在 `dom` 存不透明的型別、由 `dom_api` 解讀：多一層轉換，沒有好處。

2026-10-11 確認第 19 到 23 條，第 1 段的 1c（畫進頁面）要的（比較和 Chromium 的對照在 `details.md` 第 7 節）。

19. **C1-a：快照是 `gfx::ImageData` 的一種「解碼好的像素」，沿用 `DrawImage`**（2026-10-11）：大小和 premultiplied 的 RGBA8；raster 直接包成 `SkImage`，不解碼，照 id 快取。和 cc 的 `PaintImage` 一樣（它可以包解碼好的 `SkImage`，和編碼的圖用同一個 `DrawImageRectOp`）。不選 C1-b（新的 `DrawBitmap` op）：cc 沒有，縮放和取樣要另外一份。
20. **C2-a：畫布的像素和快照各一份，像素變了之後第一次要快照時複製**（2026-10-11）：配置都可以失敗；像素沒變就沿用同一個快照（同一個 id，display item 也沿用）。Chromium 的 `makeImageSnapshot` 是 copy-on-write；kun 把快照畫進頁面時，display list 一直拿著上一張，畫下一張前 copy-on-write 也一定要複製，成本一樣。不選 C2-b（照原本寫的共用像素、copy-on-write）：畫的路徑多處理共用、`Arc<[u8]>` 的配置失敗會 abort。
21. **C3-a：layout 標出 canvas 的 box，paint 向圖片來源要快照，renderer 畫完 canvas 之後讓它的 paint 失效**（2026-10-11）：`BackgroundImages` 多一個方法（renderer 從 DOM 讀），沿用背景圖載入後的 invalidation（`PaintController::invalidate`）。和 Chromium 一樣只重新 paint、不重新 layout（`LayoutHTMLCanvas::CanvasChanged`）。什麼都還沒畫的 canvas：透明的不畫，不透明的要快照時先建出黑色的像素。不選 C3-b（快照放進 fragment）：每畫一次都要重建 box、重新 layout。
22. **C4：有 canvas 等著畫時要求 main frame**（2026-10-11）：renderer 判斷要不要 main frame 時，除了 DOM 的版本也看有沒有 canvas 等著畫；Chromium 的 context 畫過之後（`DidDraw`）canvas 要求下一個 frame。不這樣做的話，只畫 canvas、沒改 DOM 的動畫不會更新畫面。
23. **C5-a：canvas 照 Chromium 用雙線性、沒有 mipmap 的取樣**（2026-10-11）：Chromium 的 `CanvasDefaultInterpolationQuality` 是 `kInterpolationLow`（`filter_quality_` 是 `kLow`）；`DrawImage` 帶取樣方式，`<img>` 照舊有 mipmap。kun 還沒有 `image-rendering`，只有預設值。不選 C5-b（和 `<img>` 一樣有 mipmap）：縮小顯示時和 Chrome 不同。

2026-10-11 確認第 24 到 26 條，第 1 段的 1d（canvas 自己的 layer）要的（比較和 Chromium 的對照在 `details.md` 第 8 節）。

24. **E1-a：compositor 的 layer list 是 `Layer` 的列舉（`Picture`、`Texture`），照 paint 的順序**（2026-10-11）：和 cc 一樣（`PictureLayer`、`TextureLayer` 都是 `cc::Layer`，在同一個 list），canvas 的 layer 插在 picture layer 之間，之後畫的內容蓋在它上面；commit、damage、`prepare_tiles`、畫 quad 照種類分開。surface layer 照舊另外一個 list。不選 E1-b（`PictureLayer` 的內容是 display list 或快照）：一個型別兩種意思，tile 和 invalidation 的欄位在快照的那一種沒有意義。
25. **E2-a：texture quad 帶取樣方式，canvas 的 layer 用雙線性**（2026-10-11）：`Material::Texture` 加上 cc 的 `TextureDrawQuad::nearest_neighbor`；tile 照舊是 nearest（1:1），canvas 的 layer 被 CSS 的大小或 device scale factor 縮放時用雙線性，software 和 wgpu 的 renderer 都做。Chromium 的 canvas layer 是 `SetFilterQuality(kLow)`（雙線性），`pixelated`、`crisp-edges` 才是 `kNone`。不選 E2-b（只有和 device pixel 1:1 的 canvas 成為 layer）：沒乘 `devicePixelRatio` 的 canvas 在高解析度螢幕上成不了 layer，每畫一次都重新 raster 頁面。
26. **E3-b：canvas 是 layer 時，畫過不重新 paint，renderer 直接換它的 `TextureLayer` 的快照、commit**（2026-10-11）：照 Chromium（`DidDraw` 只標記要檢查 paint invalidation，composited 的 canvas 由 `DoDeferredPaintInvalidation` 呼叫 `cc_layer_->SetNeedsDisplayRect`，foreign layer 的 display item 不變）。layerize 給 canvas 的 `TextureLayer` 時，快照照當下的 DOM 拿，不拿 display item 裡的（item 可能是快取的、舊的）；畫過、還沒重新 paint 的 canvas 記下來，下一次因為別的原因重新 paint 時一起讓它們的 item 失效，這樣它不再是 layer、畫進頁面時用的是新的快照。不是 layer 的 canvas 照 1c 重新 paint。不選 E3-a（一律重新 paint、layerize）：每個 frame 多跑一次 paint 和 layerize。

2026-10-11 確認第 27 到 30 條，第 1 段的 1e（路徑和線的樣式）要的（比較和 Chromium 的對照在 `details.md` 第 9 節）。

27. **G-a：`arc`、`ellipse`、`arcTo`、`roundRect` 的幾何是 `gfx::Path` 的方法，移植 Skia 的 `SkPathBuilder`**（2026-10-11）：`arc_to`（橢圓弧，`SkPathBuilder::arcTo(oval, start, sweep, forceMoveTo)`，裡面的 `SkConic::BuildUnitArc`）、切線的 `arc_to`（`arcTo(p1, p2, r)`）、`add_rrect`（`addRRect`，radii 照 `SkRRect::setRectRadii` 縮）、`transform`；canvas 只做參數的檢查、角度的正規化（Blink 的 `CanonicalizeAngle`、`AdjustEndAngle`）和旋轉的橢圓（Blink 的 `PathBuilder::AddEllipse`）。和 Chromium 的分層一樣（Blink 的 `PathBuilder` 呼叫 Skia），幾何在 `gfx` 單獨測，第 3 段的 `Path2D` 直接用。改到 SVG 那邊定的型別：只加方法，不改既有的。不選 G-b（放在 `dom_api` 的 canvas 模組）：Skia 層級的幾何在 DOM 那一層，SVG 要用時得搬。
28. **K-a：clip 照 Chromium 記在每一層狀態，每次開始新的錄製時重播**（2026-10-11）：`save`、`restore`、`clip` 照呼叫錄成 `Save`、`Restore`、`ClipPath`；每層狀態記下自己加的 clip（換算成 bitmap 座標的路徑），畫完一次、清空錄製之後，下一次錄製開頭替每一層狀態錄一個 `Save` 和它的 clip（Blink 的 `ClipList`、`RestoreMatrixClipStack`）。每個畫的呼叫還是固定幾個 op。不選 K-b（不錄 `save` / `restore`，每次畫都包上全部生效的 clip）：clip 多時 op 數放大，和 Chromium 不同。
29. **F-a：`fill` 和 `stroke` 一律錄成 `DrawPath`**（2026-10-11）：Chromium 把只有一條線的路徑的 `stroke` 錄成 `DrawLineOp`、空路徑上的一個 `arc` 錄成 `DrawArcOp`（`CanvasUsesArcPaintOp`），是為了少配置記憶體；Skia 的 CPU raster 畫 arc 時也是用同樣的 `arcTo` 做出路徑，像素應該一樣，WPT 看得出差別時再補。不選 F-b（照 Chromium 做兩條快速路徑）：`compositor_paint` 和 `raster` 多一種 `DrawArc`，`closePath` 對 arc 要特別處理。
30. **Q-a：路徑和規範不同的地方照 Chromium**（2026-10-11）：空路徑上的 `lineTo(p)` 是 `M p L p`（長度 0 的線段，圓的、方的端點畫出一個點；規範只開一個 subpath）；寬或高是負的 `roundRect` 最後 `moveTo` 正規化之後的左上角（規範是原本的 (x, y)）；沒有目前的點、transform 不可逆時，`quadraticCurveTo` / `bezierCurveTo` 先 `moveTo` 沒有換算的控制點；`roundRect` 先檢查 radii 的個數（`RangeError`），再看數字是不是有限的。和第 16 條一樣：網頁在 Chrome 上畫什麼，kun 就畫什麼；記在 `docs/limitations.md`。不選 Q-b（照規範）：WPT 可能多通過幾個，畫面和 Chrome 不同。

2026-10-11 確認第 31 到 35 條，第 2 段「文字」要的（比較和 Chromium 的對照在 `details.md` 第 10 節）。第 2 段分 2a（`font` 和文字的狀態）、2b（`fillText`、`strokeText`）、2c（`measureText` 和 `TextMetrics`）、2d（WPT、reftest、情境和 `about:demo`、Speedometer 的盤點、文件）。

31. **N1：`font` 在設定時解析和計算**（2026-10-11）：`dom_api` 經過 client 讓 canvas 元素的 style 更新，`style` 用 `font` shorthand 的文法解析，相對的單位（`em`、`%`、`larger`、`bolder`）照元素的 computed style 算，元素不在文件裡時照 `10px sans-serif`；算好的字型存進狀態，和上次同一個字串時跳過。和 Chromium 一樣（`UpdateStyleAndLayoutTreeForElement`、`CSSParser::ParseFont`、`StyleEngine::ComputeFont`）；Chromium 另有每份文件 50 個字串的 LRU（`CanvasFontCache`），kun 先不做，量得出差別再加。不選 N2（存字串，畫或量時才解析）：每次畫都要解析，結果也和 Chromium 照設定當下的值不同。
32. **P1：parley 做得到的文字屬性都影響排版**（2026-10-11）：`letterSpacing`、`wordSpacing`（照 Chromium 只收一個長度，`em` 換字型時重新換算）、`fontStretch`（parley 的 font width）、`fontKerning`（`kern` feature）、`fontVariantCaps`（OpenType 的 small caps 這類 feature；Chromium 在字型沒有時合成，kun 不合成）、`direction`（`inherit` 照元素的 computed direction；kun 的 style 沒有 `direction`，照 `dir` attribute 算出的方向，是 Chromium 在元素不在文件裡時的做法）；`textRendering` 只記下、回報（Chromium 用它選 hinting，kun 的 raster 不做 hinting）。不選 P2（只有 `direction` 和 spacing 影響排版）：工作少，但畫出來和 Chrome 不同。
33. **C1：`LayoutClient` 加一個排版的方法，在呼叫當下排**（2026-10-11）：給它文字、算好的字型和屬性，回傳 glyph run（字型資料、字級、glyph、位置、合成的粗體和斜體）、寬度、ink bounds 和字型的度量；`layout` 開一個公開的單獨排版的入口（SVG 的 `<text>` 在 crate 裡已經這樣用）。和 Chromium 一樣在呼叫時排（Blink 的 `Font`，錄成 `DrawTextBlobOp`）。介面不改名（第 11 條留下的問題）：排版本來就是 layout 的事，改名會動到每個現有的呼叫者。不選 C2（錄下文字，flush 時才排）：`measureText` 要當場量，等於兩條路。
34. **M1：字型的度量照 Chromium，用 `read_fonts` 讀 OS/2、hhea 和 BASE**（2026-10-11）：`top`、`bottom`、`middle` 用 OS/2 的 typo ascent / descent 正規化成合起來 1em（Blink 的 `NormalizedTypoAscent`），`fontBoundingBox` 用 hhea 的 ascent / descent，`hanging`、`ideographic`、`alphabetic` 先看 OpenType 的 BASE，沒有時 hanging 是 ascent 的 80%、ideographic 是 −descent、alphabetic 是 0。圖表常用 `textBaseline = "top"` 或 `"middle"`，這樣位置和 Chrome 一樣。不選 M2（都用 parley 的 ascent / descent）：`top`、`middle` 會差幾個 px。
35. **T1：`TextMetrics` 照 Chromium stable 的成員**（2026-10-11）：`width`、`actualBoundingBoxLeft` / `Right` / `Ascent` / `Descent`、`fontBoundingBoxAscent` / `Descent`、`hangingBaseline`、`alphabeticBaseline`、`ideographicBaseline`。`emHeightAscent` / `Descent`、`getSelectionRects`、`getActualBoundingBox`、`getIndexFromOffset` 在 Chromium 還是 experimental（`ExtendedTextMetrics`），不做。不選 T2（照規範全部做）。

2026-10-11 確認第 36 到 38 條，2a（`font` 和文字的狀態）要的（`details.md` 第 11 節）。

36. **D1-a：canvas 的 `font` 多收 `small-caps` 和 stretch 的 keyword，頁面的 `font` shorthand 不變**（2026-10-11）：照 Chromium 的 `font` shorthand（`normal | small-caps` 和 CSS3 的 stretch keyword），存進 `fontVariantCaps` 和 `fontStretch`；kun 的頁面沒有 `font-variant-caps` 和 `font-stretch`，那兩個值在頁面的 `font` 裡照舊不收。`style` 裡兩者共用一個文法，canvas 的多兩種值。不選 D1-b（頁面也加這兩個 longhand）：頁面的排版和 WPT 的 css 跟著變，超出這份。
37. **D2-a：system font（`caption`、`menu` 這類）不收**（2026-10-11）：當成不合法的值，`font` 不變，記在 `docs/limitations.md`。Chromium 收，對到平台的系統字型和字級，每個平台不同；canvas 很少用。不選 D2-b（都對到 `system-ui` 和固定的字級）：字級和 Chrome 不同。
38. **D3-a：`direction` 的 `inherit` 照 HTML 的「directionality」**（2026-10-11）：`dom_api` 照元素和祖先的 `dir` attribute 算，`auto` 取第一個有方向的字元；和 Chromium 在元素沒有 style 時的 `CachedDirectionality` 一樣，頁面沒有寫 CSS 的 `direction` 時（kun 的 style 沒有這個屬性）也和它的 computed `direction` 一樣。在 2b 畫的時候用。不選 D3-b（一律 ltr）：`dir="rtl"` 的頁面和 Chrome 不同。

2026-10-11 確認第 39、40 條，2b（`fillText`、`strokeText`）要的（`details.md` 第 12 節）。

39. **B1-a：`LayoutClient` 只加一個排版的方法，畫和量共用**（2026-10-11）：給它準備好的文字、算好的字型、base direction、換成 px 的 spacing、kerning 和 canvas 的元素，回傳每個 glyph run（字型資料、字級、glyph、位置、合成的粗體和斜體）、寬度和第 34 條的字型度量（2c 再加 ink bounds）；`fillText`、`strokeText` 和 2c 的 `measureText` 都用它，結果一致。和 Chromium 一樣同一次排版給畫和量（`ComputeInlineSize`、`DrawWithBidiReorder` 用同一個 `Font`）。base direction 交給 parley 的 `set_base_direction`，不用改 fork。不選 B1-b（畫和量各一個方法）：兩邊要保持一致，介面多一個。
40. **B2-a：renderer 給所有 canvas 共用一份 `RasterCache`，跨 flush 留著**（2026-10-11）：Skia 的 typeface 不用每次畫都重建（Skia 的 typeface 和 glyph 的快取在 Chromium 是整個 process 共用的）；用到的字型有限，記憶體跟著字型的數量。不選 B2-b（每次新建一份）：文字多的 canvas 每個 frame 都重新載入字型。

2026-10-11 確認第 41、42 條，2c（`measureText`）要的（`details.md` 第 13 節）。

41. **I1-a：ink bounds 用 Skia 的 glyph 路徑的 bounds**（2026-10-11）：renderer 經過 `raster` 取每個 glyph 的路徑（共用的 `RasterCache` 給 typeface），用路徑的 bounds，沒有路徑的點陣字（彩色 emoji）退回 `SkFont::getBounds`；合成的粗體和斜體一起算。和 macOS 的 Chrome 一樣（`SkFontGetBoundsForGlyph` 在 macOS 用路徑），浮點數精確，各平台相同；規範只說是「the bounding rectangle of the given text」，沒有定精確度。Windows / Linux 的 Chrome 用 `getBounds`，那是 Skia 的 glyph 快取裡往外取整的整數 bounds（Chromium 自己在 4px 以下改用路徑，`CanvasTextMetricsPreciseBounds`，還是 experimental），和 kun 差不到 1px。不選 I1-b（`getBounds` 的整數）：小字時誤差大，WPT 的 `actualBoundingBox.small-font` 不過；不選 I1-c（`layout` 用 skrifa 讀 `glyf` / CFF 的 bbox）：和畫的時候不是同一套字型處理，合成的粗體和斜體要自己補。
42. **I2-a：只在 `measureText` 時算 ink bounds**（2026-10-11）：`shape_canvas_text` 多一個參數說要不要 ink，`ShapedText` 多一個 `ink`。和 Chromium 一樣（`DrawTextInternal` 只量寬度，`TextMetrics::MeasureRuns` 才聯集 glyph 的 bounds）。不選 I2-b（每次都算）：`fillText` 多付取每個 glyph 的路徑的成本。

2026-10-11 確認第 43 條，2d（WPT、reftest、情境）要的（`details.md` 第 14 節）。

43. **W1-a：WPT 的 `text/` 整個加進來，靠 web font 的寫成預期的失敗**（2026-10-11）：148 個裡 48 個用 `@font-face` 載入 `CanvasTest.ttf` 或 Ahem、`await document.fonts.ready`，kun 的 style 丟掉 at-rule、沒有 `document.fonts`，它們的 `.ini` 各加一行註解說明原因；其他的照常守著 canvas 的文字。Chromium 的 canvas 沒有自己的字型載入：`ResolveFont` 照 canvas 的 style 算出 `FontDescription`，`SetFont` 綁上頁面的 font selector（認得 `@font-face`），字型載完時 `FontsNeedUpdate`；`FontFaceSet` 在 `core/css`。WPT 規定 Ahem 也要當 web font 載入（`../third_party/wpt/docs/writing-tests/ahem.md`）。Chromium 的 web test 另有 `//third_party/test_fonts`（`InitializeSkFontMgrForTest`），和 kun 的測試字型一樣只是讓預設的字型各平台一致。不選 W1-b（這份裡做最小的 `@font-face` 和 `document.fonts.ready`）：web font 是頁面層級的功能（載入、重新排版、`FontFaceSet`），canvas 只是使用者之一，該另開一份 RFC；不選 W1-c（跑 WPT 時把 `CanvasTest` 和 Ahem 裝成本機字型）：`document.fonts` 還是沒有，測試一樣失敗，只讓測試的環境和 kun 不同。

2026-10-11 確認第 44 到 47 條，第 3 段「圖片和樣式」要的（比較和 Chromium 的對照在 `details.md` 第 15 節）。第 3 段分 3a（`CanvasGradient`，`fillStyle` / `strokeStyle` 可以是物件）、3b（`drawImage`、`CanvasPattern`、`imageSmoothingEnabled` / `Quality`、origin-clean）、3c（`globalCompositeOperation`、陰影）、3d（`Path2D`、`isPointInPath` / `isPointInStroke`）、3e（`putImageData`、`createImageData`）、3f（WPT、reftest、情境和 `about:demo`、文件）：漸層先做，用 SVG 已有的型別，先定下狀態持有 GC 物件的方式；圖案要圖片；合成和陰影包住每一個畫的呼叫，放在畫的呼叫都有了之後；`Path2D` 和 `putImageData` 各自獨立。

44. **E1：狀態經過 context 上被 trace 的 slot 持有漸層和圖案**（2026-10-11）：context 多一個 `EdgeChunks`，每一層狀態兩個 slot（fill、stroke），狀態記顏色或「在我的 slot 裡的物件」；`save` 用新的一層的 slot，`restore` 清掉，畫的時候才從物件產生 shader。和 Chromium 一樣（`CanvasStyle` 的 `Member<CanvasGradient>` / `Member<CanvasPattern>`，`ApplyToFlags` 時產生 shader）：設定之後的 `addColorStop` 照樣影響之後的畫，`ctx.fillStyle === g`。沿用 `EdgeChunks`（只加長、`Edge` 的 barrier）在 concurrent marking 下的保證，不用新的 GC primitive。不選 E2（狀態存設定當下的 shader 快照，另外一條 edge 給 getter）：一樣要 GC edge，設定之後的改動又不生效，和 Chrome 不同。
45. **O-a：宣告表支援多載**（2026-10-11）：一個成員可以宣告好幾種形式，JS 的 bindings 照 WebIDL 的多載解析（先看參數個數，再看區分位置上是 interface 還是字串），`kun:dom` 每種形式一個函式（名字寫在宣告裡）。和 Chromium 從 IDL 產生的 bindings 一樣（`drawImage(img, 1, 2, 3)` 是 `TypeError`）；這一段的 `drawImage`、`fill` / `stroke` / `clip` / `isPointInPath` / `isPointInStroke`、`createImageData`、`putImageData`、`Path2D` 的建構子都用到，之後的 API（`setTransform(DOMMatrix2DInit)`）也能用。改到 `declare` 的 macro、`dom_js` 的 bindings、WIT 和 Lua 的產生器、`spec` 測試。不選 O-b（只宣告最長的形式，照給了幾個分派）：`undefined` 會被當成沒給，和 WebIDL 不同，`Path2D or CanvasFillRule` 這種 union 在 WIT 裡彆扭；不選 O-c（每種形式各自是 `[WitOnly]` 成員，JS 用 `glue.js` 對應）：大約 8 個成員要手寫 glue。
46. **I1：`drawImage` 經過 `LayoutClient` 在呼叫當下拿 renderer 的圖片**（2026-10-11）：給 `<img>`，回傳 renderer 快取的圖片（`gfx::ImageData`，bytes 共用，raster 用到時才解碼、用共用的 `RasterCache`）和它的 tainting，錄進 `DrawImage`；來源是 `<canvas>` 時先經過 client 把它畫完，用它的快照。和 Chromium 一樣（`GetSourceImageForCanvas` 拿呼叫當下的 frame，錄成 `PaintImage`）。不選 I2（錄 URL，flush 時才換成圖片）：`src` 在那之前改了、或快取被清掉時畫成別張圖。
47. **T1：3b 就做 origin-clean**（2026-10-11）：tainting 是 `Opaque` 的圖片 taint canvas，taint 經過圖案和 canvas 來源傳下去，被 taint 的 canvas 上 `getImageData`（和第 4 段的 `toDataURL`、`toBlob`）丟 `SecurityError`。和 Chromium 一樣（`WouldTaintCanvasOrigin`、`SetOriginTaintedByContent`）。不選 T2（之後再做）：一有 `drawImage` 頁面就能讀到別的來源的圖片的像素。

2026-10-11 確認第 48、49 條，3b（`drawImage`、`CanvasPattern`、origin-clean、宣告表的多載）要的（`details.md` 第 16 節）。

48. **A1：宣告表的多載照參數個數和「interface 對其他型別」解析，WIT 的名字寫在宣告裡**（2026-10-11）：同名的 `method` 可以寫好幾個，多出來的形式用 `as` 給 `kun:dom` 一個名字（`drawImage(...) as drawImageScaled`），JS 照 WebIDL 的多載解析，只支援先看個數、再看區分位置上是 interface 還是別的型別這兩層；宣告表的測試檢查每個多載都能這樣分開。這一段的 `drawImage`、`fill` / `stroke` / `clip` / `isPointInPath` / `isPointInStroke`、`createImageData`、`putImageData`、`setTransform` 都夠用。Chromium 從 IDL 產生的 bindings 照完整的演算法。不選 A2（完整的 WebIDL 多載解析）：現在用不到的大部分，實作和測試大很多，碰到時再擴充；不選 A3（WIT 的名字照個數自動編號）：guest 看不出每個形式的意思。
49. **C1：做 `DOMMatrix2DInit`，不做 `DOMMatrix`**（2026-10-11）：dictionary 照 geometry 規範的「validate and fixup」（`a`–`f` 和 `m11`–`m42` 兩套名字要一致，不一致是 `TypeError`），`CanvasPattern.setTransform(init)` 和 `ctx.setTransform(init)`（多載）都收；`getTransform()` 要回傳 `DOMMatrix`，不做。Chromium 都有。不選 C2（連 `DOMMatrix` / `DOMMatrixReadOnly` 一起做）：geometry 規範的一整組 interface，超出 canvas，該另開一份；不選 C3（都不做）：圖案只能沿著畫布的座標鋪。

2026-10-11 確認第 50 到 52 條，3c（`globalCompositeOperation`、陰影）要的（`details.md` 第 17 節）。

50. **M1：合成模式是 `PaintFlags` 的 blend mode**（2026-10-11）：`compositor_paint` 的 `PaintFlags` 多一個 `blend_mode`（canvas 用到的 27 種 Skia 的模式：Porter-Duff 的 12 種，`lighter` 是 `kPlus`；`multiply` 那類的 15 種，`normal` 就是 `source-over`），每個畫的 op 帶著它，raster 設到 `SkPaint`；`clearRect` 的 `ClearRect` 留著。和 cc 一樣（`SetGlobalComposite` 設 fill、stroke、image 三份 flags 的 blend mode）。不選 M2（`PaintFlags` 不動，不是 `source-over` 的畫一律包一層有 blend mode 的 layer）：每次畫一個 layer，和 cc 不同。
51. **L1：新增有 flags 的 `SaveLayer`**（2026-10-11）：`SaveLayer { bounds, flags }`，flags 有 alpha、blend mode 和 image filter（drop shadow），給 canvas 的「整個畫布」的模式（`source-in`、`source-out`、`destination-in`、`destination-atop`：單位矩陣、有 blend mode 的 layer、`source-over` 畫、`Restore`）和陰影用；CSS 的 effect 照舊用 `SaveLayerAlpha`。和 cc 一樣（`SaveLayerOp` 和 `SaveLayerAlphaOp` 都有；Blink 的 `CompositedDraw`）。不選 L2（把 `SaveLayerAlpha` 擴充成通用的）：要動 CSS 的 effect 和它的測試，和 cc 不同；也不在 `dom_api` 用反向的 clip 清掉形狀以外：文字和圖片的 alpha 做不到。
52. **S2：陰影一律用 drop shadow 的 image filter**（2026-10-11）：有陰影的畫錄成單位矩陣、有陰影和本體的 filter 的 `SaveLayer`、目前的 matrix、畫、`Restore`；和陰影不相容的模式（`source-over`、`source-atop`、`destination-out` 以外）照 Blink 的 `CompositedDraw` 分兩次：只有陰影的 layer、本體的 layer，各自用那個模式合成。就是規範的模型，圖片、圖案、文字的 alpha 都跟著，位移和模糊在 bitmap 的座標裡。Chromium 分兩條路：不透明的畫用 looper（同一個形狀換成陰影的顏色、mask filter 的模糊，在 device 空間位移後先畫一次），圖案、不透明以外的圖片和要 composite 的用 `DropShadowPaintFilter`。不選 S1（照 Chromium 兩條路）：looper 是不開 layer 的效能最佳化，結果只差在模糊的近似，Skia 已經沒有 `SkDrawLooper`，raster 要替每種 op 模擬，之後有效能的問題再加；不選 S3（錄兩次 op，用現有的 `blur`）：圖案透明的地方和圖片的 alpha 做不到。

2026-10-11 確認第 53、54 條，3d（`Path2D`、`isPointInPath` / `isPointInStroke`）要的（`details.md` 第 18 節）。

53. **H2：`isPointInPath` / `isPointInStroke` 用 `gfx` 的 kurbo 幾何，和 SVG 共用**（2026-10-11）：SVG 的 `isPointInFill` / `isPointInStroke` 和 hit test 在 `paint` 裡用的那段（winding 判斷裡外、邊上也算，照 Skia 的 `SkPath::contains`；線用 `kurbo::stroke` 變成區域）移到 `gfx` 的 `Path`，canvas 和 SVG 都呼叫它；點用目前 transform 的反矩陣換算。Chromium 用 Skia（`Path::Contains`、`StrokeContains` 的 `getFillPath`），也不經過 renderer。不選 H1（經過 `LayoutClient` 讓 raster 用 Skia 算）：沒有 frame 的文件就算不出來，canvas 和 SVG 用兩套幾何；kurbo 的 conic 近似和 stroker 和 Skia 不同，線的邊緣附近可能有差，寫進 limitations。
54. **B1：路徑資料組成 `gfx::Path` 的程式移出 `layout`，SVG 的 `d` 和 `Path2D` 共用**（2026-10-11）：`layout` 的 `path_data`（弧線變成 conic、`S` / `T` 反射控制點）搬到 `computed_style`（`append_path_data`，`PathSegment` 在那裡；實作時發現 `layout` 不依賴 `style`，所以不放 `style`），`style` 提供把字串變成 `gfx::Path` 的函式（`path_from_string`：解析、換成絕對座標、組路徑，出錯時留下之前的部分），`layout` 和 `dom_api` 都用它。和 Chromium 一樣（`BuildPathFromString`，SVG 和 `Path2D` 共用）。不選 B2（`dom_api` 另寫一份）：兩份會走樣；不選 B3（經過 `LayoutClient`）：沒有 frame 就不能建 `Path2D`。

2026-10-11 確認第 55、56 條，3e（`putImageData`、`createImageData`）要的（`details.md` 第 19 節）。

55. **W1：`putImageData` 在 `dom_api` 直接寫進畫布的像素**（2026-10-11）：先畫完錄下的 op（寫的範圍蓋住整個畫布時丟掉它們），再把像素寫進 `CanvasData` 的 bitmap（有 alpha 的換算成 premultiplied，不透明的照搬 RGB、alpha 是 255），不經過 transform、clip、`globalAlpha`、合成和陰影，記下畫布要重畫。和 Chromium 一樣（`PutByteArray`、`WritePixels`：蓋住整個畫布時 `RestartRecording`，否則 `FlushCanvas`，再寫 backing）。不選 W2（錄一個 `WritePixels` 的 op，raster 用 `SkCanvas::writePixels`）：和 Chromium 不同，多一種 op。
56. **E1：宣告表加 `[EnforceRange] long`**（2026-10-11）：JS 收到 NaN、無限大或超出範圍的數是 `TypeError`，WIT 是 `s32`；`getImageData`、`createImageData`、`putImageData` 的整數參數都用它（`getImageData` 原本的限制拿掉）。和 Chromium、規範一樣。不選 E2（照舊用 `long`）：和 Chrome 不同，WPT 檢查它的測試失敗。

2026-10-11 確認第 57 條，3f（WPT、reftest、情境和 `about:demo`、收尾）要的（`details.md` 第 20 節）。

57. **W1：範圍內的 WPT 目錄都加**（2026-10-11）：`html/canvas/element/` 的 `fill-and-stroke-styles/`、`compositing/`、`pixel-manipulation/`、`shadows/`、`drawing-images-to-the-canvas/`、`reset/`、`canvas-context/`、`conformance-requirements/` 和直接放在它底下的檔；不在範圍內的 `layers/`（Chrome experimental 的 `beginLayer`）、`filters/`、`color-type/`、`global-hdr-headroom/`、`video/`、`wide-gamut-canvas/` 不加。預期的失敗寫明原因。不選 W2（只加第 3 段功能的五個目錄）：`reset/` 這類以前缺陰影和合成、現在守得住的沒人守。

2026-10-12 確認第 58 到 61 條，第 4 段「輸出」要的（比較和 Chromium 的對照在 `details.md` 第 21 節）。第 4 段分 4a（`toDataURL` 和編碼器）、4b（`toBlob`）、4c（WPT、情境和 demo、文件、整份 RFC 的收尾）。

58. **F1：PNG 和 JPEG，`image/webp` 退回 PNG**（2026-10-12）：WPT 的 `the-canvas-element/` 有 8 個用 JPEG，沒有用 WebP 的。Chrome 有 PNG、JPEG、WebP，其他的（和不認得的）退回 PNG；規範只要 PNG。不選 F2（連 WebP）：kun 的 Skia 沒開 WebP 的編碼器，要換一套 Skia 的 binary，純 Rust 的 `image-webp` 只有無損；不選 F3（只有 PNG）：JPEG 的 WPT 失敗。
59. **E1：編碼器在 `raster`，`dom_api` 經過 `LayoutClient` 呼叫**（2026-10-12）：PNG 用 `png` crate，照 Chrome 的 `SkPngRustEncoder` 的 `kLow`（deflate level 1、Up filter）；JPEG 用 Skia 的 `SkJpegEncoder`，照 Chrome 的選項（預設品質 92、alpha 合成到黑色、品質 100 時 4:4:4）。和 Chromium 用一樣的編碼器和設定（Blink 的 `ImageEncoder`）；`dom_api` 照舊不碰 Skia。沒有 frame 的文件編不了碼（`"data:,"`、`toBlob` 給 `null`），那裡的 canvas 本來就不畫。不選 E2（`dom_api` 用純 Rust 的 `png` 和 `jpeg-encoder`）：JPEG 和 Chrome 的 libjpeg-turbo 不同，多一個依賴；不選 E3（`dom_api` 依賴 `raster`）：`dom_api` 連到 Skia。
60. **A1：`toBlob` 在呼叫時拿快照、編碼，task 裡呼叫 callback**（2026-10-12）：結果和 Chrome 一樣（呼叫當下的快照），callback 在之後的 task。Chrome 拿快照之後，PNG 在 idle task 裡分段編、JPEG 和 WebP 交給 worker thread（`CanvasAsyncBlobCreator`）。不選 A2（背景的 thread 編碼）：要新的「背景工作完成送回 renderer 的 loop」，大的畫布卡住 main thread 寫進 limitations。
61. **Q1：`quality` 照 IDL 是 `any`**（2026-10-12）：JS 照 Chrome 只認數字（不轉換，字串不算）；`kun:dom` 照現有的規則拿掉這個可選的 `any`，guest 用預設的品質。不選 Q2（另外給 `kun:dom` 一個 `option<f64>` 的形式）：多一個函式，現在沒有人要。

2026-10-12 4b 開始前確認第 62 條。

62. **J1：`toBlob` 只給 JS（`[JsOnly]`）**（2026-10-12）：它的 `BlobCallback` 是新的 callback 種類，`kun:dom` 的 guest 每一種 callback 有一個 export，加一個會改掉 world（minor 版本要升，C 的測試 guest、Rust 的 demo、Lua 的 runtime、wasm 的 reftest 都要重編）；`requestIdleCallback` 也是這樣只給 JS。guest 用同步的 `toDataURL`，寫進 `docs/limitations.md`。Chromium 沒有 wasm 的 DOM，沒有對照。不選 J2（加 `blob(id, blob)` 的 export）：為了一個現在沒有 guest 要的 API 讓所有 guest 重編。

2026-10-12 4c 開始前確認第 63 到 65 條（`details.md` 第 22 節）。

63. **W1：整個 `html/semantics/embedded-content/the-canvas-element/` 加進 WPT 的清單**（2026-10-12）：沒通過的寫成預期失敗、附上原因，跑到 timeout 的停用。和第 57 條一樣，同一個目錄裡現在通過的 origin-clean、fallback、`getContext` 的測試也有人守。不選 W2（只加 `toDataURL.*`、`toBlob.*`、`security.*`）：通過的 `context.*`、`fallback.*` 沒人守。
64. **D1：`about:demo` 的樣式的 canvas 多一個「Copy as image」**（2026-10-12）：`toBlob` 給的 Blob 經過 `URL.createObjectURL` 給旁邊的 `<img>`，說明寫出 PNG 的大小；情境點它、截圖。走過 `toBlob` 的 task、Blob、blob URL 和 `<img>` 的解碼，一般網站也這樣用。不選 D2（`toDataURL` 給 `<img>`）：沒走非同步的那一條。
65. **R2：不加 kun 自己的 reftest**（2026-10-12）：browser 的測試已經逐像素確認 PNG 經過 `<img>` 畫回來（半透明的也是），WPT 的 `toDataURL.png.*`、`toDataURL.jpeg.*` 也載回來比像素。不選 R1（比較 canvas 和它的 `toDataURL` 的 `<img>`）：和那些重複。

## 待決定

（沒有。）

## 之後的可能

- web font（`@font-face` 和 CSS Font Loading API 的 `document.fonts`）：頁面和 canvas 共用，另開一份 RFC；做完時 WPT 的 `text/` 裡靠它的 48 個測試會意外通過（第 43 條）。
- compositor 和 Viz 處理平移以外的 transform 和 layer 的 opacity 之後，canvas 在它們底下也能自己一層（和捲動容器一起）。
- layer 的合併（Blink 的 `PendingLayer` merging）：頁面上很多小 canvas 時。
- GPU 加速的 canvas（要 GPU 的 raster，和 Vello 的方向一起考慮）。
- `OffscreenCanvas` 和 worker 裡的 canvas（要先有 worker）。
- WebGL、WebGPU。
- `filter`、`ImageBitmap`、`createImageBitmap()`、`captureStream()`。
- WebP 的編碼（要換一套開了 WebP 編碼器的 Skia）；`toBlob` 在背景的 thread 上編碼（要「背景的工作做完送回 renderer 的 loop」）；wasm 的 `toBlob`（`kun:dom` 的 guest 多一個 callback 的 export）。
- `alert`、`confirm`、`prompt`（Perf-Dashboard；WebDriver 的 user prompt）。
