# RFC 0007：用 vello_cpu 取代 Skia

- 開始日期：2026-10-05
- 狀態：🚧 已排程：2026-11-10（接受：2026-11-10；已決定的事項第 7 到 13 條：先嘗試採用 vello_cpu，做完、可行的話下一份 RFC 是 vello_gpu；「待決定」剩下的在各步開始前或看過那一步的結果再定）
- 細節：`details.md`
- 進度和驗收：`progress.md`

## 摘要

拿掉 Skia（skia-safe），machopo 用到 Skia 的每個地方都換成純 Rust 的 crate：tile 和 canvas 的 raster 用 Linebender 的 **vello_cpu**，圖片的解碼和讀尺寸用 image-rs 系列的 crate，JPEG 的編碼用 `jpeg-encoder`，glyph 的 ink bounds 用 skrifa。`raster` crate 對外的 API 不變，compositor、renderer 和 Viz 都不用改。

這是一次嘗試：第 1 步先做原型，跑完測試、看過截圖、量過速度，再決定要不要繼續。Vello 沒有、machopo 的 display list 又用到的功能，在 machopo 的 playback 裡用 vello_cpu 現有的功能組出來，不先 fork Vello；缺的每一項都列在 `docs/` 底下新的一份 `render-gaps.md`（下面只寫檔名）。raster 的結果不再以和 Chromium 逐像素一致為目標。

GPU raster（vello_gpu 為主，不能用時退回 vello_cpu）是下一份 RFC，這份只讓 playback 不擋住它。

## 動機

**現況**（細節見 `details.md` 的「現在的 raster」）：

- Skia 只在 `raster` crate 裡（那時的 `skia_backend.rs`（第 2 步拿掉了） 約 990 行程式加上測試，`crates/raster/src/encode.rs` 的 JPEG），一律是 CPU 的 backend。用到它的地方：
  - **tile 的 raster**：compositor thread 上（`--num-raster-threads` 時在 raster worker 上）呼叫 `raster::ops_for` 和 `raster::playback_into`，畫成 premultiplied 的 RGBA8，再上傳到 Viz 的 wgpu device（`crates/compositor/src/layer_tree_host_impl.rs`）；
  - **canvas 2D**：canvas raster thread 用 `raster::draw_into` 把錄製畫進 canvas 的 bitmap（`crates/renderer/src/canvas_raster.rs`、`crates/renderer/src/canvas.rs`）；
  - **圖片**：`raster::image_size` 讀尺寸（`crates/renderer/src/render_thread/documents.rs`），解碼在 raster 時由 Skia 做；
  - **編碼**：`raster::encode_jpeg`（`toDataURL`、`toBlob`，`crates/renderer/src/canvas_encoder.rs`；PNG 已經用 `png` crate）；
  - **文字**：`raster::ink_bounds`（`crates/renderer/src/frame_view.rs`、`crates/layout/src/canvas_text.rs`），和照字型、軸的值建的 typeface。
- display list（`crates/compositor_paint/src/lib.rs` 的 `PaintOp`）有 24 種 op，包括 image filter（drop shadow、blur、color matrix、串接）、28 種 blend mode、三種漸層、圖案 shader、dash、mask blur、`DrawShadow`、`DrawRecord`、`WritePixels`。

**問題**：

- **Skia 是 C++ 的依賴，綁著 prebuilt**：只能用 rust-skia 有發布的 feature 組合，多開一個 feature 就要從原始碼編好幾十分鐘；WebP 就是因此沒做（`rfcs/pre-rfcs/P2.md` 的「已決定的事項」第 7 條）。沒有 prebuilt 的平台第一次 build 很久（`docs/architecture.md` 的「要注意的地方」、根目錄 `README.md`）。
- **之後的 GPU raster 用 Skia 的每條路都很貴**：要嘛拿掉 wgpu、自己寫各平台的 swapchain，要嘛 fork skia-safe、自己編 Skia 和 Dawn，要嘛讓 Skia 和 wgpu 共用一個 device、自己處理同步（`details.md` 的「保留 Skia，做 GPU raster」）。Vello 的 CPU 和 GPU 兩個 renderer 共用同一份算覆蓋率的程式，vello_gpu 直接畫在 machopo 已經用的 wgpu 上，而且可以一個 tile 一個 tile 選用哪一個（`details.md` 的「CPU 和 GPU 的結果有多接近」）。
- **Vello 系列是純 Rust，和 machopo 已經用的 parley、skrifa 同一個生態系**：沒有自己的建置、不新增 `unsafe`。在 Linebender 的 benchmark 裡，vello_cpu 單執行緒大約是 Skia CPU 的 2.5 倍（`details.md` 的「效能」；網頁的 tile 文字多，要在 machopo 裡實測）。

要解決的 `docs/limitations.md` 裡的限制：

- 「Viz、GPU 和記憶體」：raster 為可變字型建的 Skia typeface 跟著 compositor、不會變小。Vello 直接吃字型的資料和軸的值，不建 typeface。

做完時跟著改寫、但不是解決的：

- 「Paint 和 raster」的合成粗體（改成 Vello 的 embolden，粗細不同）。
- 「圖片」的 raster 的圖片快取沒有大小上限（多了預先縮小的版本）。

**目標**：

- workspace 裡沒有 skia-safe；`raster` 的 tile 和 canvas 的 raster 都用 vello_cpu。
- `raster` 對外的 API 不變（`playback`、`ops_for`、`playback_ops`、`playback_into`、`draw_into`、`image_size`、`ink_bounds`、`encode_png`、`encode_jpeg`、`RasterCache`、`Bitmap`、`PlaybackParams`、`BACKEND`），所以 compositor、renderer 和 Viz 都不用跟著改。
- 現有的 reftest、WPT 的 reftest、情境和端到端測試照樣通過。因為反鋸齒、文字的粗細、JPEG 的解碼而改了預期結果或加了 fuzzy 的，每一個都寫明原因。
- **playback 不會 panic**：Vello 不支援的功能都在 playback 裡改寫或繞過，不交給 Vello；有測試守著每種 op、filter 和 blend mode 的組合。
- **缺的功能看得到、追得到**：每一項都在 `render-gaps.md`，上游補上時有測試會失敗（見「追蹤缺的功能」）。
- 圖片支援的格式不少於現在（PNG、JPEG、GIF 的第一格）。
- 不新增 `unsafe`。

**不在範圍內**：

- **GPU raster**（vello_gpu、退回 vello_cpu 的條件、raster 的介面、raster 前的單色分析）：下一份 RFC，等這份做完、量過 vello_cpu 再寫（見「之後的可能」）。
- **不經過 tile、直接畫成 Viz 的 quad 的快速路徑**（單色和圓角的矩形、陰影用專用的 shader，GPUI 的做法，`details.md` 的「GPUI 怎麼畫」）：GPU raster 量過不夠快才考慮。
- WebP 等新的圖片格式：拿掉 Skia 之後只要加 `image-webp`，但另外做。
- 新的 CSS 效果（`mask-image`、`backdrop-filter`、SVG 的 filter primitive）：等那些功能時再看 Vello 有沒有、要不要自己做（見「缺點」）。
- 色彩管理（ICC）：今天的 Skia 路徑也沒有（畫進沒有色彩空間的 surface），換掉不會變差。EXIF 的方向在範圍內（已決定的事項第 16 條；原本以為今天不讀 orientation，量過之後不是）。
- CSS 的 `image-orientation: none`。
- OOP-R 的跨 process 部分：屬於 RFC 0004 或之後。

## 設計概要

給 machopo 的使用者：畫面幾乎一樣。看得出來的差異：

- **文字比較細、淡**，深色背景上的淺色字最明顯：Vello 畫文字不像 Skia 那樣調整 gamma 和對比。這是唯一明顯的外觀差異，第 1 步看過截圖再決定怎麼處理（見「待決定」）。
- 合成粗體的粗細略有不同。
- 大面積、顏色相近的漸層可能有色帶（沒有 dithering）。
- 反鋸齒的邊和 JPEG 的解碼有放大才看得出來的差異。

捲動、改視窗大小時的 raster 應該快一些（要量）；用到 Vello 沒有的功能的頁面（好幾個 CSS `filter` 串接、四個角半徑不同的 `box-shadow`）那一部分會慢一些。

給之後改 machopo 的人：

- `raster` crate 對外的樣子不變。裡面的 `skia_backend.rs` 換成一個 Vello 的 playback：把 `PaintOp` 一個一個畫進 vello_cpu 的 `RenderContext`。Vello 沒有的功能在 playback 裡組出來（見「詳細設計」），呼叫的人不需要知道。
- compositor、canvas raster thread、Viz 不變：tile 照樣是 CPU 的像素，照樣上傳；單色的 tile 照樣 raster 完看像素。
- 圖片的解碼、JPEG 的編碼、glyph 的 ink bounds 換成各自的純 Rust crate，函式的簽名不變。
- 加新的繪圖功能前先看 `render-gaps.md`：Vello 有沒有、沒有的話現在怎麼補。

## 詳細設計

各個選擇的比較見「理由和其他做法」和 `details.md`。Vello 的部分對照的是 2026-11-10 的 `main`（crates.io 上還是 2026-10-02 的 vello_cpu 0.3.0）。

### playback（`crates/raster/src/`，取代 `skia_backend.rs`）

- **每條 raster 的 thread 一個 `RenderContext`**，tile 和 tile 之間 `reset` 重用（今天每條 thread 重用 pixel buffer 的做法）。vello_cpu 的多執行緒不開：machopo 已經是一條 thread 畫一個 tile（`--num-raster-threads`），而且 vello_cpu 開多執行緒時連單一的 filter 都會 panic。
- **起始的 transform**：位移到 tile 的原點，再乘上 `contents_scale`；`SetMatrix` 相對於它（和今天一樣）。
- **`Save` / `Restore`**：Vello 的 layer 不保存 transform，playback 自己維護堆疊：目前的 `Affine`，以及這一層推了幾個 clip 或 layer，`Restore` 時一一 pop。
- **`clear_color`** 用 render 時的 `TargetInit::Clear`；canvas 的 `draw_into` 畫在既有的像素上，用 `TargetInit::SrcOver`。
- **直接對應的 op**（`details.md` 的「PaintOp 怎麼對應」）：transform、`ClipRect`、`ClipRRect`、`ClipPath`（不隔離的 `push_clip_path`）、`SaveLayerAlpha`、只有 alpha 和 blend mode 的 `SaveLayer`、`DrawColor`、`DrawRect`、`DrawRRect`、`DrawDRRect`、`DrawLine`、`DrawPath`、`ClearRect`、`DrawClipped`、dash（kurbo 的 stroke）、cap 和 join、28 種 blend mode（peniko 的 Mix 加 Compose）、三種漸層（linear、two-point radial、sweep，Pad、Repeat、Reflect，premultiplied 和 unpremultiplied 的插值）、只有一個 Blur 或 DropShadow 的 filter、不抗鋸齒（`set_aliasing_threshold`）。
- **Vello 沒有、在 playback 裡組出來的**（`details.md` 的「上游缺什麼」列了每一項的上游狀態，「PaintOp 怎麼對應」逐項寫了做法）：
  - **不支援的 filter**（`ColorMatrix`、`Chain`，vello_common 碰到會 `unimplemented!`）：layer 的內容畫進另一個 `RenderContext` 的 pixmap，machopo 自己套 color matrix（照 Skia：先 unpremultiply），Blur 和 DropShadow 照樣交給 Vello，串接的照順序一個一個做，最後當成圖片、帶著 layer 的 alpha 和 blend mode 畫回去。`ColorMatrix` 上游有 PR 在審（CPU 和 GPU 都做），合了之後單一的 color matrix 直接交給 Vello；串接的沒有人在做，離屏的做法會留著。
  - **filter 在 tile 邊緣**：`RenderContext` 只畫自己的範圍，tile 外面的內容不會被模糊進來，相鄰的 tile 之間會有接縫。有 filter 的 layer 用比 tile 大、四周加上 filter 的範圍（`filter_expansion`）的 context 畫，render 時用負的位移裁回 tile。原型時先確認接縫真的存在。
  - **path 的 union**（`ClipPaths`）：Vello 沒有 path 的布林運算。把每條 path 照它的 fill rule 畫進一張和 tile 一樣大的 alpha mask，`push_mask_layer`。反鋸齒的邊和 Skia 的 `PathOps` 有一點差異。
  - **陰影和任意形狀的模糊**（`DrawShadow`、`PaintFlags.blur`）：Vello 的 `fill_blurred_rounded_rect` 只有一個圓形的半徑（`_in` 的版本加上 `invert` 給 inset 的陰影），machopo 的 `RRectF` 是四個角各自的橢圓半徑。四個角相同、而且是圓的走 Vello 的快路；其他的圓角矩形和任意形狀包一層 GaussianBlur 的 filter layer（結果一樣，多一次離屏）。
  - **圖案 shader**：`ShaderImage::Record`（SVG 當背景）照當時的 scale 先畫成 pixmap；`spacing`（`background-repeat: space`）畫進四周留了空白的 pixmap；`decal` 改用 clip。
  - **每個 glyph 自己的旋轉**（SVG 的 `rotate`）：拆成一個 glyph 一個 run。
  - **`WritePixels`**（只在 canvas 的錄製裡）：先把前面的 op render 進 bitmap，寫入像素，再接著畫。
- **mipmap**（`FilterQuality::Medium` 縮小時）：Vello 沒有。圖片快取照取樣的 scale 準備縮小的版本（2 的次方，box filter），playback 選最接近的一張交給 Vello 用 bilinear 取樣。這是 cc 的 `SoftwareImageDecodeCache` 的做法：交給 Skia 的就是預先縮好的圖。`High` 放大時用 Vello 的 bicubic。

### 文字

- `gfx::FontData` 的資料和 index 直接交給 Vello 的 `glyph_run`；Skia 的 typeface 快取、為 CoreText 拆字型集合的程式（和只為它用的 `read-fonts`）拿掉。
- hinting 照舊關掉，glyph 的位置由 layout 決定（subpixel）。只有灰階的反鋸齒，和今天一樣。
- **位置對齊到四分之一個 pixel**（已決定的事項第 29 條）：matrix 只有縮放和位移時，每個 glyph 的原點在交給 Vello 之前對齊到 device pixel 的四分之一（沿著行）和整數（垂直方向），照 Skia 的 glyph 快取的量化。旋轉或歪斜的文字照原位置畫。
- **離 tile 遠的 glyph 不交給 Vello**（已決定的事項第 24 條）：一段文字是一個 op，它碰到的每個 tile 都收到整段；Vello 先把輪廓攤平才知道 glyph 在 tile 外面，所以 playback 先濾掉原點離 tile 超過字級兩倍（加上 stroke 的寬度）的。
- 可變字型：`variations` 轉成 normalized 的座標交給 Vello。
- 合成粗體用 `font_embolden`，合成斜體用 `glyph_transform` 的 skew；粗細要重新對（`docs/limitations.md` 的「Paint 和 raster」的合成粗體那一條跟著改寫）。
- COLR 和點陣的 emoji（CBDT、sbix）由 glifo 畫，要開 `png` feature。
- **glyph 的快取不開**：上游標成高度實驗性、不建議用。文字多的 tile 每次都重新攤平輪廓，第 1 步量這個成本。
- **ink bounds**（`raster::ink_bounds`）改用 skrifa：有輪廓的 glyph 把輪廓畫進 kurbo 的 `BezPath` 取範圍（和 Vello 實際畫出來的一致），點陣的讀 bitmap 的 metrics，再照 embolden、skew 和 stroke 擴大。函式的簽名照舊（`RasterCache` 這個參數留著不用）。

### 圖片

- **解碼**：用 `image` crate，關掉預設的 feature、只開要的格式（已決定的事項第 14 條）。它底下是 `png`（machopo 的編碼已經用它，Chromium 的 Skia 也已經改用它）、`zune-jpeg` 和 `gif`；猜格式、各種顏色格式的轉換（調色盤、灰階、16 bit、`tRNS`、interlace、CMYK 的 JPEG）由它做。GIF 只有第一格，和今天一樣。解碼的結果轉成 premultiplied 的 RGBA8，照現在的方式依 `ImageData::id` 快取在 `RasterCache`（每個 compositor 一個）；第 2 步起縮小的版本跟著同一個項目。
- **什麼時候解碼**（已決定的事項第 18 條）：和今天一樣，第一次畫到那張圖時，在 raster 的 thread 上（compositor thread 或 raster worker）。今天靠 Skia 的懶惰解碼（在 `RasterCache` 的鎖裡只建一個 Skia 的圖片，真的解碼在畫的時候）；`image` 沒有懶惰解碼，所以快取的每個項目是一個只填一次的格子：鎖只用來找到或建立格子，解碼在鎖外面做，兩條 worker 同時要同一張圖時一條解、另一條等它。沒畫到的圖不解碼，main thread 只讀檔頭。Skia 還在畫的期間（第 0 步到第 2 步之間），解好的像素交給 Skia 建成圖片（一次複製），交完就放掉自己那一份。
- **壞掉和惡意的檔案**（已決定的事項第 20 條）：解碼和讀尺寸都包在 `catch_unwind` 裡，panic 當成解不開；`image` 的 `Limits` 擋住宣稱超大尺寸的檔頭。
- **讀尺寸**：`image` 只讀檔頭，不解碼。
- **Skia 今天的行為**（2026-11-10 對著 Skia 寫測試量的）：BMP 和 ICO 也解得開（Skia 的 codec 內建，照內容認，不看 MIME type）；JPEG 的 EXIF 方向，`image_size` 回傳沒轉的尺寸、畫的卻是轉過的圖（40×30、方向是 6 的 JPEG，尺寸是 40×30、Skia 的圖片是 30×40），所以今天這種圖片是變形的。
- **JPEG 的解碼和 Skia（libjpeg-turbo）不逐像素相同**（IDCT 和色度 upsampling 的做法）：比對 JPEG 和 PNG 的 reftest 可能要 fuzzy，每個寫明原因。
- **JPEG 的編碼**：`jpeg-encoder`（純 Rust，已決定的事項第 17 條）。照 Blink 的設定：疊在黑色上，quality 低於 100 時色度是 4:2:0、等於 100 時是 4:4:4；輸出的 bytes 和今天不同，只要解得回來、品質相當。
- **編碼前轉成不是 premultiplied 的那一步**（PNG 和 JPEG 都用到，今天是 Skia 的 `readPixels`）：換成 machopo 自己的算式。趁 Skia 還在，用一個測試把所有的顏色和 alpha 的組合拿來和 Skia 的結果比，逐位相同之後再拿掉 Skia 的那一段，`toDataURL('image/png')` 的輸出不變。

### 追蹤缺的功能

Vello 缺的有十來項，分三種：在 playback 裡補上、使用者看不出來的；接受的外觀差異；只在速度上看得出來的。做法：

- **`render-gaps.md`**（已決定的事項第 9 條）：raster 的函式庫缺的每一項一列，包括補在 playback 裡、使用者看不出來的：缺什麼、上游的 issue 或 PR、machopo 補在哪個函式、什麼條件下可以拿掉。只放目前仍然成立的，上游補上、machopo 拿掉補的程式時刪掉那一列。
  - 第 2 步 merge 時加這份文件；在那之前，清單在 `details.md` 的「上游缺什麼」。
  - `docs/limitations.md` 只留使用者看得到的結果（文字比較細、漸層的色帶），一句話，細節指到 `render-gaps.md`，同一件事不寫兩份。
  - 跟著改的：`docs/README.md` 的「每份文件放什麼」加一列、「其他會牽動文件的變更」加一條（升級 Vello 時逐列確認）；`CLAUDE.md` 的「先讀」加上它。
- **「上游還缺」的測試**（已決定的事項第 10 條）：Vello 碰到會 panic 的缺口各配一個測試，直接把那個功能交給 Vello，用 `#[should_panic]` 預期它 panic：`ColorMatrix` 和多個 primitive 的 filter graph 這 2 項。升級 Vello 時上游補上了，測試就失敗，訊息寫明可以拿掉 playback 的哪一段（WPT 的預期失敗通過時 wptrunner 報「unexpected pass」是同一個想法）。這樣不用有人定期去看上游的 issue。API 根本不存在、沒有東西可以呼叫的 4 項（單一半徑的模糊圓角矩形、path 的布林運算、mipmap、圖案 shader）測不了，只靠 `render-gaps.md` 和升級時逐列確認。上游如果改成碰到不支援的 filter 就略過、不 panic（有人提過這樣的 PR，沒合就關了），這些測試也會失敗，那時改成檢查畫出來的像素。
- **程式裡的標記**（已決定的事項第 11 條）：補的地方註解以 `vello:` 開頭，寫缺什麼和上游的 issue（和 fork 裡的 `machopo:` 同一個做法，Chromium 用 `TODO(crbug.com/編號)` 標繞道的地方），例如 `// vello: no ColorMatrix filter (linebender/vello#1296); applied offscreen here.`。`grep` 得到全部、可以和 `render-gaps.md` 對數量，函式改名或搬家也不會失聯。註解不引用 `docs/` 的文件，只寫缺什麼和上游的 issue；方向是 `render-gaps.md` 指向程式碼。這個規則寫在 `render-gaps.md` 的開頭（`raster` 沒有自己的 README）。沒有腳本檢查，補的地方只有幾處，人工對得過來。
- **測不出來的**：glyph 快取是效能，記在 `docs/test-gaps.md`，寫明升級 Vello 時重新量 `bench.py raster`。

### Vello 的版本和 fork

- **先不 fork**（已決定的事項第 4 條）。2026-11-10 對照上游的 `main`，沒有非改 Vello 不可的缺口（`details.md` 的「上游缺什麼」）。
- **可能 fork 的只有 `ColorMatrix`**：第 2 步要 merge 時上游的 PR 還沒合、離屏的做法又量到太慢，才 fork 帶那一個 patch；它本來就是上游的程式，合了之後 fork 就消失。fork 照 parley 的模式：`../third_party` 底下的 vello（`machopo` 分支），只放非改不可的小 patch；`README.machopo.md`、`third_party.toml` 照現有的 fork 的規則。不完全自己接手（像 rusty_v8 那樣）：machopo 還需要上游的 x86 SIMD、glyph 快取和 vello_gpu。
- 文字的 gamma 和對比、漸層的 dithering：改在上色的核心，上游不一定接受（會改變所有使用者的結果），第 1 步看過截圖再決定（見「待決定」）。
- 要送上游的 patch，照 `CLAUDE.md`，先在本地寫好草稿給人審。
- **原型（第 1 步，不 merge）釘上游 `main` 的 commit**（已決定的事項第 21 條）；第 2 步用哪個版本見「待決定」。

### 測試

- **playback 的單元測試**：`raster` 現在對著 Skia 寫的測試照搬（每種 `PaintOp`、`Save` / `Restore` 和 clip、layer 的配對、tile 的位移和 scale、`SetMatrix` 相對於 tile、canvas 的 `draw_into`）。像素的預期值因為反鋸齒不同而改的，寫明原因。
- **不會 panic**：一個測試列舉每種 op、每種 `ImageFilter`（包括巢狀的 `Chain`）、每種 blend mode、`ClipPaths`、圖案 shader 的組合畫一次。vello_common 的 `unimplemented!` 一碰到就會讓它失敗。
- **「上游還缺」的測試**：見「追蹤缺的功能」。
- **組出來的功能各自的測試**：color matrix 和 Skia 的公式對照（固定的輸入和輸出）；串接的順序；path union 和逐條 clip 的差別；四個角半徑不同的陰影和單一半徑的走不同的路、結果接近；mask blur 的範圍；圖案 shader 的 spacing 和 decal；mipmap 選到哪一張（計數）。
- **tile 的接縫**：同一個 display list 用一個大的 tile 畫一次，再切成 256 的 tile 各畫一次拼起來，兩者逐像素相同；特別放模糊、drop shadow 和陰影跨過 tile 邊界的內容。
- **圖片和編碼**：每種格式的解碼和讀尺寸、壞掉的檔案回傳 `None`；JPEG 編碼後再解碼，和原圖的差在容許範圍內；ink bounds 和輪廓的範圍一致，embolden、skew、旋轉、點陣 glyph。
- **reftest、WPT 的 reftest、情境**：machopo 沒有存起來的參考圖，reftest 是同一次執行裡畫兩個頁面互相比，兩邊用同一個 renderer，大部分不受影響。會壞的是兩邊用不同方式畫出同一個結果的（文字和色塊比、JPEG 和 PNG 比）。照 `CLAUDE.md` 跑改到的部分，第 2 步的那個 commit 跑 `test.py reftest`、相關目錄的 `test.py wpt`、`test.py scenarios`。每個改了預期結果或加了 fuzzy 的測試，在測試或 `tests/reftests/README.md` 寫明原因。
- **mutation test**：照 `docs/testing.md` 的「Mutation test」，結果寫進 `progress.md`。
- **只能人看的**：文字和圖片的外觀（變細、縮小的品質），在 `about:demo` 上和改之前的截圖比，記進 `progress.md` 的「做過的檢查」；`docs/test-gaps.md` 的「測試守不住的地方」記下文字的外觀只靠人看。
- **量測**：`bench.py raster`（捲動、改視窗大小時 compositor 的 PrepareTiles）、`bench.py blink-perf paint/ image_decoder/`，Skia 和 vello_cpu 各一次（Windows，有 Mac 時加 macOS）。Speedometer 不排，使用者要求時才跑。

### 分階段

順序照已決定的事項第 12、13 條；每一步開始前只決定那一步要決定的事。實作在 `0007` 的 branch 上。

0. **圖片的解碼和 JPEG 的編碼先換**（Skia 照舊畫，做完先 merge）：圖片的解碼和讀尺寸、JPEG 的編碼換成純 Rust 的 crate。解碼的結果當成 raster 的圖片交給 Skia 畫，像素的差異只來自 JPEG 的解碼，預期結果因此要改的在這一步改。排在原型之前（已決定的事項第 13 條）：原型直接拿到純 Rust 解碼的像素，不用寫之後會丟掉的轉接；之後第 2 步改預期結果時，分得出哪些是解碼造成的、哪些是 Vello 造成的；而且不管最後用不用 Vello，這一步都有用（之後加 WebP 只要多一個 crate；Chromium 也是 Skia 照舊畫、PNG 先換成 Rust 的 `png`）。第 1 步的結論是不繼續的話，`main` 上是「Skia 畫、解碼和編碼不是 Skia」，能正常運作。
1. **原型和量測**（不 merge）：這一步決定要不要繼續，分兩段、各有一個檢查點（已決定的事項第 12 條）。兩段是同一份程式，決定繼續之後整理成第 2 步，不是做完就丟。任何一個檢查點的結論是不能接受，這份 RFC 就回到草稿重新評估，Skia 留著。
   - **1a. 常用的 op**：vello_cpu 的 playback 先接矩形、圓角、文字、圖片、漸層、基本的 clip 和 layer，沒接的 op 先跳過。
     - 量 raster 的時間（`bench.py raster`、`bench.py blink-perf paint/`），特別是文字多的 tile。
     - 存下 `about:demo` 等頁面換之前和換之後的截圖，給人比文字的外觀。
     - **第一個檢查點**：文字看起來能不能接受、文字多的 tile 快不快。這兩項是最可能讓這份 RFC 停下來的，接完常用的 op 就看得到。這時的數字不含 filter 和陰影，跳過 op 的頁面畫出來是錯的，reftest 的結果還不能看。
   - **1b. 剩下的 op 和組出來的功能**：接上全部 24 種 op，照上面的做法組出 Vello 沒有的功能。
     - 跑 `raster` 的測試、reftest、相關的 WPT 的 reftest 和情境，列出哪些不通過、差在哪裡。
     - 確認 filter 在 tile 邊緣的接縫、unpremultiplied 的漸層插值、不隔離的破壞性 blend mode 在 vello_cpu 上的結果。
     - 量離屏的 color matrix 和四個角半徑不同的陰影。
     - **第二個檢查點**：整體的差異能不能接受、要不要 fork。結論寫進 `details.md`。
2. **playback 換成 vello_cpu、拿掉 skia-safe**：tile 和 canvas 一起換，連同 mipmap 和所有組出來的功能（少了任何一項就會 panic 或退步，所以不再拆）；ink bounds 也在這一步改用 skrifa（Vello 用 skrifa 畫 glyph，和 playback 一起換，量的和畫的始終是同一套）；預期結果在同一個 commit 裡更新；直接替換，不留切換的旗標（已決定的事項第 5、7 條）。同一次改動加上 `render-gaps.md`，並改掉其他文件裡的 Skia：`docs/architecture.md` 的 pipeline 的表、「Crate」的表、「Crate 選型」、「要注意的地方」，`docs/limitations.md`、`docs/testing.md`、根目錄的 `README.md`、`Cargo.toml` 和 `.cargo/config.toml` 裡寫到 Skia 的註解、CI 的 setup action。
3. **改進**：文字的速度（Vello 的 glyph atlas：頁面大小、保留時間、斜的 glyph 的 key，`vello-atlas-experiment.patch`；已決定的事項第 28 條）；canvas 上破壞性的 blend mode 配 clip（第 31 條，有頁面碰到才做）；看第 1、2 步的結果可能不需要的：文字的 gamma 和對比、漸層的 dithering、Vello 的 fork 裡的 `ColorMatrix`，不需要的寫明理由、不做。

第 2 步做完，Skia 就完全拿掉了。merge 回 `main` 之後跑 `cargo clean`。

## 缺點

- **raster 的結果和 Chromium 不同**：`docs/architecture.md` 的「Crate 選型」原本只用 Skia，理由就是 raster 結果和 Chromium 一致。換成 Vello 之後：文字沒有 gamma 和對比的調整，看起來比較細、淡；合成粗體的粗細不同；path 的 union 和 mask blur 的反鋸齒邊不同；漸層沒有 dithering；JPEG 的解碼有小的差異。拿 Chromium 的截圖或 WPT 上別的瀏覽器的結果來比時，差異會變多。
- **Vello 還不穩定**：每一版都是 breaking（2026-05 到 2026-10 出了五版），每次升級都要改 playback；MSRV 可能在 patch 版本裡提高。
- **machopo 要自己維護組出來的功能**：離屏的 filter、mask 的 union、非單一半徑的陰影、圖案 shader 的 pixmap、mipmap 的快取。這些在 Skia 裡都是現成的，而且離屏比 Skia 的 filter 慢、多用記憶體。
- **多一份要維護的清單**：`render-gaps.md` 和「上游還缺」的測試，每次升級 Vello 都要對一次。
- **少了 Skia 現成的、之後的 CSS 會用到的功能**：多步的 filter graph、SVG 的 filter primitive（vello_common 的 enum 列了，但只實作了 Flood、GaussianBlur、Offset、DropShadow）、`mask-image`、`backdrop-filter`、LCD 的文字。到時候要等上游或自己做。
- **glyph 的快取還是實驗性的**：文字多的 tile 每次都要重新攤平輪廓，可能比 Skia 慢，要在第 1 步量。

## 理由和其他做法

完整的比較見 `details.md` 的「不選的做法」。

- **A. 保留 Skia，只做 CPU raster**：成本最低，結果最接近 Chromium；但 C++ 的依賴和 prebuilt 的限制都留著，之後要 GPU raster 還是得面對 B。
- **B. 保留 Skia，做 GPU raster**（Ganesh、原生的 Graphite、Skia 畫進 wgpu、Graphite 加 Dawn）：和 Chromium 一致、功能最齊，但每條路都要拿掉 wgpu 自己寫平台層、fork 和自己編 Skia 加 Dawn，或兩套 GPU 層互相同步。
- **C. 先用 vello_cpu 取代 Skia，之後再加 vello_gpu**（選這個，第 1、3、8 條）：純 Rust、wgpu 留著、沒有新的 `unsafe`、沒有自己的建置；CPU 和 GPU 共用同一份算覆蓋率的程式，之後加 GPU raster 時兩條路的結果幾乎一樣，可以一個 tile 一個 tile 選。vello_cpu 不是過渡：它之後是 reftest、headless、沒有 GPU 和 vello_gpu 不支援的 tile 走的路。代價是「缺點」那些。
- **D. 經典的 vello（compute）**：連攤平和覆蓋率都在 GPU 上，是唯一「全程 GPU」的；但上游已經把它當成研究用的，中間 buffer 是固定大小，場景太複雜時畫錯了也回傳成功，每次 render 都要一次 submit 和十幾個 dispatch。**不選。**
- **E. tiny-skia**：只有 CPU，速度約是 vello_cpu 的五分之一，沒有 GPU 的路。**不選。**
- **F. 拿掉 tile，每個 frame 整頁直接畫**（GPUI、WebRender、Blitz 的做法，`details.md` 的「GPUI 怎麼畫」）：GPUI 能這樣做是因為它只有 8 種 primitive、clip 只有矩形、沒有 blend mode 和 filter，每個 frame 重畫整個視窗。網頁的內容是任意的，而且捲動和合成動畫要能不重新 raster；這等於放棄 cc 這一層。**不選。**

缺的功能怎麼處理（「詳細設計」的「Vello 的版本和 fork」）：
- **甲. 不 fork，只在 machopo 的 playback 裡補**（開始時選這個，第 4 條）：沒有同步的成本；2026-11-10 對照上游，缺的都補得起來。
- **乙. 小 patch 的 fork**（需要時才用）：只在離屏的做法量到太慢、上游又還沒合時，帶上游自己的 patch。
- **丙. 完全自己接手**：失去上游最有價值的部分（x86 的 SIMD、glyph 快取、vello_gpu 走向成熟），等於 machopo 自己養一個 2D renderer。**不選。**

缺的功能記在哪裡（「詳細設計」的「追蹤缺的功能」）：
- **甲. 獨立的 `render-gaps.md`**（選這個，第 9 條）：項目多，而且大部分使用者看不到（補在 playback 裡），不屬於 `docs/limitations.md` 的「已知限制」；集中在一份，升級 Vello 時逐列確認。
- **乙. 全部寫進 `docs/limitations.md`**：不用多一份文件，但那裡是給使用者看得到的限制，十來項補起來的東西會把它淹沒。**不選。**
- **丙. 只留在這份 RFC 的 `details.md`**：RFC 做完就不再更新，之後升級 Vello 沒有地方改。**不選。**

換掉 playback 的那一步怎麼進 `main`（「分階段」第 2 步）：
- **甲. 直接替換**（選這個，第 5、7 條）：一次更新預期結果，之後只有一套 raster。
- **乙. 過渡期間用旗標切換 Skia 和 Vello**（Chromium 推 Graphite 時用旗標逐步開啟）：能直接比兩邊的像素；但有一段時間要維護兩個 backend、兩個都要編，`docs/architecture.md` 寫的是不維護第二個 backend。比對用第 1 步的原型（在 branch 上，`main` 還是 Skia）就做得到。**不選。**

`docs/architecture.md` 原本不用 Vello 的理由是「整個場景一次 render，沒有 cc 那種以 tile 為單位的 raster 概念」：那是針對經典的 vello，而且是假設由 Vello 取代 cc 和 Viz。這份 RFC 保留 cc 的 tile，每個 tile 是一個小場景，vello_cpu 適合這種用法。

不做會怎樣：Skia 留著，WebP 等功能繼續受 prebuilt 限制；要 GPU raster 時只剩 B 那幾條昂貴的路。

## 先例

- **Chromium**：Skia，tile 在 GPU process 裡 raster（OOP-R）；圖片的解碼在 CPU 上（`ImageDecoder`；PNG 的解碼和編碼已經換成 Rust 的 `png` crate），縮小的圖片由 cc 的 `SoftwareImageDecodeCache` 先縮好再交給 Skia。machopo 照 cc 的分層和預先縮小的圖片，raster 的函式庫不同。
- **Servo**：網頁用 WebRender 畫，沒有 tile 的 raster；canvas 2D 預設用 vello_cpu。它試過經典的 vello，GPU 版每個 frame 都要讀回再上傳，比 CPU 版慢，2026-01 改回 vello_cpu 當預設。
- **Blitz**：用 Vello 系列畫整個網頁，是目前最接近 machopo 這種用法的；上游有一份它需要的功能的追蹤 issue，和 machopo 缺的大部分重疊（`details.md` 的「上游缺什麼」）。
- **Firefox**：WebRender 畫整個場景，canvas 和部分內容用 Skia（`docs/architecture.md` 已經否決 WebRender）。
- **GPUI**（Zed 的 UI 框架）：不經過通用的 2D renderer，8 種 primitive 各一個 shader 直接畫進視窗；文字、SVG 和圖片在 CPU 上畫好放進 atlas。不照做（「理由和其他做法」的 F），它的 glyph atlas 之後由 vello_gpu 提供。

逐項比較見 `details.md` 的「和其他實作比較」。

## 已決定的事項

1. **2026-10-05：用 Vello 系列取代 Skia，CPU 和 GPU raster 都是**。raster 的結果不再以和 Chromium 一致為目標，`docs/architecture.md` 的「Crate 選型」裡「只用 Skia」的原則改掉。不做保留 Skia 的 GPU raster（Ganesh、原生的 Graphite、Skia 畫進 wgpu、Graphite 加 Dawn），理由見「理由和其他做法」的 B：每條路都要拿掉 wgpu、自己編 Skia 和 Dawn，或兩套 GPU 層互相同步，而 Vello 是純 Rust、直接畫在 machopo 已經用的 wgpu 上。
2. **2026-10-06：延後**。當時不打算考慮，raster 照舊用 Skia 的 CPU backend。2026-10-31 由第 3 條重新打開。
3. **2026-10-31：重新打開，這份 RFC 把 Skia 換成 vello_cpu**（使用者）。範圍是 machopo 用到 Skia 的每個地方：tile 和 canvas 的 raster、圖片的解碼和讀尺寸、JPEG 的編碼、glyph 的 ink bounds。GPU raster 照第 1 條的方向（vello_gpu 為主，沒有 GPU 時退回 vello_cpu），另寫一份 RFC，等這份做完、量過 vello_cpu 再開。
4. **2026-10-31：Vello 先不 fork，需要時做小 patch 的 fork**（使用者照建議）。Vello 沒有的功能先在 machopo 的 playback 裡用 vello_cpu 現有的功能組出來；第 1 步的原型或之後的量測看出非改 Vello 不可（例如離屏的 `ColorMatrix` 太慢）時，照 parley 的模式 fork。不選完全自己接手（「理由和其他做法」的丙）：machopo 還需要上游的 x86 SIMD、glyph 快取和 vello_gpu。
5. **2026-10-31：換掉 playback 的那一步直接替換**（使用者照建議，「理由和其他做法」的甲）。預期結果在同一個 commit 裡一次更新。不選過渡期間用旗標切換 Skia 和 Vello（乙）：一段時間要維護、編兩個 backend。當時另外定了這一步放在 RFC 0089 兩個階段的交界 merge，由第 7 條拿掉。
6. **2026-10-31：和 RFC 0089 並行**（使用者照建議；0089 的已決定的事項第 104 條）。由第 7 條拿掉：0089 已經做完。
7. **2026-11-10：RFC 0089 在 2026-11-06 做完，第 5、6 條裡和它有關的安排不再適用**。這份 RFC 從最新的 `main` 開 `0007` 的 branch，照 `rfcs/README.md` 的「流程」做；第 2 步直接替換（第 5 條的前半照舊），不用等別的 RFC 的階段。同時在做的 RFC 0093（wasm 文件）不碰 `raster`、`compositor` 和 `paint`。
8. **2026-11-10：先嘗試採用 vello_cpu；做完、可行的話，下一份 RFC 是 vello_gpu**（使用者）。「嘗試」的意思是第 1 步的原型有一個要不要繼續的決定（「分階段」）。之前比較過 GPUI 的做法（直接畫 primitive、不經過 tile）和經典的 vello（全程 GPU），都不選（「理由和其他做法」的 D、F）；vello_gpu 本身是混合的做法（覆蓋率在 CPU、上色和合成在 GPU），之後和 vello_cpu 一個 tile 一個 tile 選。
9. **2026-11-10：缺的功能全部列在 `docs/` 底下新的 `render-gaps.md`**（使用者）。獨立的一份文件，包括補在 playback 裡、使用者看不出來的；第 2 步 merge 時加，在那之前清單在 `details.md` 的「上游缺什麼」。不選全部寫進 `docs/limitations.md`（項目多、大部分使用者看不到）和只留在 RFC 裡（RFC 做完不再更新）。
10. **2026-11-10：Vello 會 panic 的缺口各配一個「上游還缺」的測試**（使用者照建議）。`ColorMatrix` 和多個 primitive 的 filter graph，用 `#[should_panic]`；升級 Vello 時失敗就表示可以拿掉 playback 裡補的那一段。不選只靠 `render-gaps.md` 人工逐列對（升級時要改的東西已經很多，容易跳過），也不先做成用 `catch_unwind` 檢查像素的版本（為了上游改成略過、不 panic 這個還沒發生的情況，多一層複雜度；真的發生時再換）。API 不存在的 4 項測不了，靠 `render-gaps.md`。
11. **2026-11-10：補的地方註解以 `vello:` 開頭**（使用者照建議），寫缺什麼和上游的 issue。不選再加 `sanity.py` 的檢查（有的缺口沒有上游的 issue、有的補在好幾處，對應不是一對一，規則會有例外；真的發生過對不上再加），也不選不加標記（`render-gaps.md` 寫的函式名稱改名之後就指錯地方，`sanity.py docs` 不檢查函式名稱）。
12. **2026-11-10：第 1 步的原型分兩段，各有一個檢查點，程式留著當第 2 步的基礎**（使用者照建議）。1a 先接常用的 op，看文字的外觀和文字多的 tile 的速度；1b 接完全部的 op 和組出來的功能，跑整套 reftest。不選一次接完才看（最可能讓這份 RFC 停下來的兩項，要等離屏的 filter 都做完才知道），也不選只接常用的 op、之後重寫（reftest 的結果沒有意義、組出來的功能的成本看不到、第 2 步要重寫）。
13. **2026-11-10：不屬於 raster 的三件事拆開來排**（使用者照建議）。圖片的解碼和讀尺寸、JPEG 的編碼排在原型之前、先 merge（第 0 步）；ink bounds 留到第 2 步，和 playback 一起換。不選三件事都排在原型之後（原型要寫之後會丟掉的轉接，把 Skia 解碼的圖片轉成 Vello 的 pixmap；JPEG 解碼的像素差異和 Vello 的混在一起），也不選三件事都排在前面（ink bounds 改用 skrifa 之後和 Vello 畫的一致、和 Skia 畫的可能差一點，Skia 還在時會有一段時間量的和畫的不是同一套），也不選和原型同時在兩個 branch 上做（兩邊都改 `raster` 的同一個檔案）。代價：最後決定不繼續的話，第 0 步是多做的，但它本身有用。
14. **2026-11-10：圖片的解碼和讀尺寸用 `image` crate**（使用者照建議），關掉預設的 feature（`rayon` 和十幾種格式），只開要的。它底下和各格式各自的 crate 是同一批 decoder（`png`、`zune-jpeg`、`gif`），省下的是猜格式和顏色格式的轉換，那是自己寫最容易出錯的部分；BMP、ICO、之後的 WebP 都只是開 feature。不選各格式各自的 crate（依賴最少、少一次 RGB 轉 RGBA 的複製、最像 Blink 的每種格式一個 `ImageDecoder`，但轉換和 GIF 第一格的合成要自己寫和測），也不選 zune 系列（`zune-png` 用的人少、沒有 GIF）。加進來時用 `cargo tree` 確認它用的 `png` 和 machopo 的是同一個版本、沒有帶進預期外的依賴，有問題再回來改。Servo 和 Blitz 用 `image`。
15. **2026-11-10：BMP 和 ICO 留著**（使用者照建議）：`image` 多開 `bmp` 和 `ico`（內建的純 Rust 實作，不帶進新的依賴）。Skia 今天解得開這兩種（「圖片」的「Skia 今天的行為」），這一步的目標是格式不少於現在；Blink 也有 `BMPImageDecoder` 和 `ICOImageDecoder`。ICO 有好幾張圖時照 Blink 用最大的那一張，和今天的 Skia 不同的話在測試裡寫明。`docs/limitations.md` 的「圖片」補上這兩種；`<source type>` 照 MIME type 挑來源時認不認得 `image/x-icon`、`image/bmp` 是另一件事，不在這一步改。不選拿掉（沒有測試守著、文件也沒寫，但是 regression），也不選只留 BMP（ICO 比較常見）。
16. **2026-11-10：EXIF 的方向照 Chrome，尺寸和圖都轉**（使用者照建議）。讀尺寸時也讀方向，方向是 5 到 8 時寬高對調；解碼後照方向轉好像素再快取，layout、`naturalWidth`、canvas 的 `drawImage` 拿到的都是轉好的（Blink 是畫的時候才套用，結果一樣）。今天是尺寸不轉、圖轉了，是 bug（「圖片」的「Skia 今天的行為」），這一步一定會改到這個行為，直接改成 Chrome 的 `image-orientation: from-image`（預設值），預期結果只改一次。不選都不轉（最簡單、和 `docs/limitations.md` 原本寫的一致，但直拍的照片是橫的，之後還要再改一次）。`image-orientation: none` 沒有做，記進 `docs/limitations.md`；原本「不在範圍內」寫的「不讀 orientation、換掉不會變差」不成立，改掉。`image` 不能只讀檔頭就拿到方向的話，自己從 JPEG 的 APP1 讀那一個欄位。做完時跑 WPT 的 `image-orientation` 的目錄。
17. **2026-11-10：JPEG 的編碼用 `jpeg-encoder`**（使用者照建議）。它可以設 quality 和色度的取樣，Blink 的兩種設定（4:2:0、quality 100 時 4:4:4）都對得上。不選 `image` 內建的 encoder（不用多的依賴），也不自己寫。2026-11-10 讀了 `image` 的原始碼：發布的 0.25.10 的 `JpegEncoder` 是它自己的實作，只能設 quality 和像素密度，三個 component 的取樣寫死是 1×1（一律 4:4:4，文件註解寫的 4:2:2 和程式不符），Blink 在 quality 低於 100 時的 4:2:0 做不到；它還沒發布的 `main` 已經把 `JpegEncoder` 改成包著 `jpeg-encoder`，加了 `set_chroma_subsampling`。所以兩條路底下會是同一個 encoder，現在直接用 `jpeg-encoder`；`image` 發布那一版之後，它本來就會把 `jpeg-encoder` 帶進來，那時可以改成經過 `image` 的 API、少列一個直接的依賴。它的授權是「(MIT OR Apache-2.0) AND IJG」：今天 Skia 帶的 libjpeg-turbo 也是 IJG，不是新的義務，拿掉 Skia 之後它是唯一帶這個授權的依賴。它預設的量化表是 JPEG 標準 Annex K 的那一組（libjpeg 用的），另有幾組可選、不用；照 quality 縮放的算式是不是和 libjpeg 一樣，實作時確認。
18. **2026-11-10：解碼照今天的時機，第一次畫時在 raster 的 thread 上做，在快取的鎖外面**（使用者照建議）。快取的每個項目是只填一次的格子，同一張圖只解一次。時機和 cc 的 `ImageDecodeCache` 一樣（要畫才解、不在 main thread），差別是解碼和畫在同一個 task 裡、沒有分開排程。不選載入時在 main thread 上解（最簡單、`raster` 裡沒有 decoder，但大圖卡住 script 和 layout，沒畫到的圖也解碼），也不選載入時交給另一條 thread 解、raster 等它（最接近 Chromium，但多一條 thread 或 pool 要管，這一步的範圍變大；之後量到第一個 tile 等解碼太久時再提）。
19. **2026-11-10：`image` 用發布的版本，不用它的 `main`**（使用者照建議）。`main` 的 `JpegEncoder` 已經包著 `jpeg-encoder`、可以選取樣，用它就不用直接依賴 `jpeg-encoder`；但 `main` 比 v0.25.10 多 826 個 commit、正在改 decoder 的介面（「Version Next」列著 breaking changes），照它寫等發布時很可能要再改一次；直接釘上游的 commit 也是 machopo 沒有的一種依賴（現在只有 crates.io 的發布版和 `../third_party` 的 fork），沒有版本號和 release notes；省下的只是 `Cargo.toml` 的一行，執行的是同一份編碼的程式。`image` 發布下一版時，升級那一次再看要不要改成經過它的 API。
20. **2026-11-10：解碼和讀尺寸包在 `catch_unwind` 裡，並設 `image` 的 `Limits`**（使用者照建議）。`image` 的 release notes 開頭的 Known issues 寫著很多 decoder 碰到惡意的輸入會 panic；圖片是網頁給的，不能讓 raster 的 thread 或 main thread 掛掉。panic 就當成解不開、什麼都不畫（今天 Skia 碰到壞檔案的行為），失敗的結果記在快取裡，同一張圖不重複解。沒有 panic 時的成本是多一層不會被 inline 的呼叫（unwind 是查表式的），量不出來；panic 的那一次是微秒到毫秒，加上預設的 panic hook 往 stderr 印一行（先不關，看到洗版再說）。`catch_unwind` 接不住無窮迴圈、吃光記憶體和 stack overflow，所以 `Limits` 設寬高和配置的上限，數值實作時對照 Blink。**這讓 workspace 不能把 `panic` 改成 `abort`**（改了壞圖會讓整個 process 結束），記進 `docs/limitations.md`。不選不包（壞圖讓整個分頁壞掉）；放進另一個 process 解碼要等拆 process，那之後這一層仍然有用（壞圖只是不顯示，不是分頁掛掉）。沒有實測過哪些輸入會 panic，照上游自己的說明處理；測試放壞掉的、截斷的、檔頭宣稱超大尺寸的檔案。
21. **2026-11-10：原型（第 1 步）的 Vello 釘上游 `main` 的 commit，用 git 依賴**（使用者照建議）。發布的 0.3.0 和 `main` 的 API 已經對不上（`main` 拿掉了圖片的登記 `register_image` 和 `ImageId`、改成直接給 pixmap，`fill_blurred_rounded_rect` 的 `invert` 搬到另一個函式），原型的程式要當第 2 步的基礎，照下一版的 API 寫才不用再改一次；`main` 和 machopo 用同一個 skrifa（0.49；0.3.0 用 0.44，會編兩份）；`main` 修了 `Color` 和 `Luminosity` 的 blend 把某些灰色畫成黑色的 bug。不選發布的 0.3.0（有版本號，但圖片那部分一定要重寫），也不選現在就建 fork（還不知道要不要繼續；第 4 條是先不 fork）。和第 19 條不用 `image` 的 `main` 不同的地方：這是不 merge 的原型，而且發布的版本和下一版的 API 對不上。第 2 步用哪個版本到時再定（「待決定」）。
22. **2026-11-10：原型期間 Skia 和 Vello 都編進去，用環境變數 `MACHOPO_RASTER`（`skia` 或 `vello`，預設 `skia`）在執行時切換**（使用者照建議），只在 `raster` 裡讀一次。同一個執行檔並排比截圖和 `bench.py raster` 的時間，不用 build 兩次；預設是 Skia，原型期間 `cargo test`、reftest、情境照常通過，沒接完的 op 只在設了變數時看得到。切換只活在不 merge 的 branch 上，第 2 步拿掉 Skia 時一起拿掉，所以和第 5 條（`main` 上不留切換的旗標）不衝突；Chromium 推 Graphite 時也是兩個 backend 都編進去、執行時切換。不選在 branch 上直接取代 Skia（1a 期間很多測試會失敗，分不出是還沒接還是接錯了；每次比較都要另外 build 改之前的版本），也不選 cargo 的 feature（比較要 build 兩次，Windows 上還會另外 link 一整套執行檔）。
23. **2026-11-10：第一個檢查點用的頁面**（使用者照建議）。量測用現成的 `bench.py raster`（Gutenberg、SQLite 的文件頁、NewsSite-Next 捲動和改視窗大小時的 PrepareTiles），Skia 和 Vello 各跑一次；先確認每一頁跳過了多少還沒接的 op，跳過太多的不採計。截圖用 `about:demo`、Gutenberg 的一章、SQLite 的文件頁，加上一頁專門比文字的（放在這個資料夾，是原型的素材、不是測試）：幾種字級、淺底深字和深底淺字、一般、粗體、斜體和合成粗體，1x 和 2x 各一次，用測試字型。不選只用現成的頁面：它們幾乎都是白底黑字、單一字級，最可能出問題的深色背景和小字看不到。外觀能不能接受由人判斷，不同的 pixel 數只是輔助。
24. **2026-11-10：第一個檢查點通過，做 1b；glyph 的 atlas 先不開**（使用者）。1a 量到的（`progress.md` 裡第 1a 步的那一段）：文字在淺底比 Skia 粗、深底比 Skia 細，正常大小下差別小；交給 Vello 之前先濾掉離 tile 遠的 glyph 之後，文字多的頁面和 Skia 差不多到慢 1.7 倍，圖和色塊多的頁面快 15% 到 40%。Vello 的 glyph atlas 改小頁面、拉長保留時間之後大致追上 Skia（最慢的一頁慢 1.3 倍），但那兩個設定要改 Vello 才有，而且斜的 glyph 開 atlas 會畫錯：先不用，改動留成 `vello-atlas-experiment.patch`，要不要為它 fork 在第二個檢查點一起定。machopo 自己快取輪廓量過沒有用（時間在攤平和產生 strip，不在讀字型），從原型拿掉。濾 glyph 的那一步留著，第 2 步要有測試。
25. **2026-11-10：第二個檢查點用 Vello 把整套測試各跑一次**（使用者照建議）：`MACHOPO_RASTER=vello` 下的 `test.py reftest`、`test.py wpt`、`test.py scenarios` 和 `cargo test --workspace`。`CLAUDE.md` 的規則是本機只跑改到的部分、不跑整套的 `wpt`；換 raster 影響每一個會畫東西的測試，沒有哪個目錄算不相關，所以這個檢查點例外一次。`wdspec`、`matrix`、`fuzz` 照舊不跑。不選只跑 CSS 裡畫東西的幾個目錄（會漏掉其他目錄的 reftest，檢查點的數字不完整）。
26. **2026-11-10：第二個檢查點通過，做第 2 步**（使用者）。1b 量到的（`progress.md` 裡第 1b 步的那一段和它後面補查的）：用 Vello 跑整套，machopo 的 reftest 195 個和情境 78 次都通過；WPT 的 13,917 個測試裡 39 個因為 Vello 不通過（34 個 reftest 差一點，其中 31 個最大差 1；5 個 canvas 的）；`raster` 對著 Skia 寫的 75 個單元測試有 71 個照原樣通過。上游的 bug 到這裡有四個，都在 machopo 這一側避開了或先不用：clip 的交集在 stroke 剛好從右邊緣開始時溢位（不把整個在外面的 clip 交給 Vello）、結尾硬切的漸層超過終點時用錯顏色（把漸層拉長一倍）、glyph atlas 每次 render 重畫整頁和斜的 glyph 用錯項目（atlas 不開，第 24 條）。繞不過的是 canvas 上破壞性的 blend mode 配 clip（4 個 WPT）和 kurbo 的 stroker 在很粗的線的內側接合（1 個）。Vello 的 f32 管線只讓 5 個差 1 的測試通過、每一頁都比 Skia 慢（1.2 到 2.7 倍），不用。第 2 步開始前要定的在「待決定」。
27. **2026-11-10：Vello 建 fork，照 parley 和 taffy 的做法**（使用者照建議）。`wusyong/vello` 的 `machopo` 分支是上游 `main` 的 `112736df24` 加上 `README.machopo.md` 的 commit，checkout 在 `../third_party/vello`，machopo 用 path 依賴，`third_party.toml` 釘 `rev` 和 `upstream_rev`，patch 在 `patches/vello/`。一開始沒有改 Vello 的 patch。第 4 條是「需要時做小 patch 的 fork」，原型做完之後條件出現了：上游的 bug 已經找到四個、glyph atlas 要改才用得上，有 fork 才能在上游修之前先用修好的版本；即使沒有 patch，fork 也是把這個依賴放進現有規則的方式（`ci.py third-party` 管它）。不選直接釘上游的 commit 當 git 依賴進 `main`（machopo 沒有的一種依賴，要帶任何 patch 就得改成 fork），也不選等上游發布（不知道要多久，發布版一樣不能帶 patch）。第 21 條的 git 依賴只在原型期間用過。
28. **2026-11-10：glyph 的 atlas 不在第 2 步做，留到第 3 步**（使用者照建議）。第 2 步用不開 atlas 的 Vello 換掉 Skia，接受文字多的頁面每個 tile 的時間是 Skia 的 1.0 到 1.7 倍（最慢的 SQLite 的那一頁，改視窗大小時每次更新的 raster 約 7 ms 變 12 ms）。第 2 步的範圍留在換掉而且畫得對；atlas 是上游標成實驗性、改得最頻繁的部分，現在放 patch 進去 rebase 最容易衝突；開 atlas 會把 glyph 的位置量化，預期結果要多對一輪。不選第 2 步就帶 atlas 的三個 patch（頁面大小、保留時間、斜的 glyph 的 key），也不選 machopo 自己做一層 glyph 的點陣快取（等於重做 atlas，而且量過從 atlas 貼 glyph 在 Vello 裡比填純色貴）。文字的速度是第 3 步明確的一項。
29. **2026-11-10：glyph 的位置照 Skia 對齊到四分之一個 pixel**（使用者照建議）。交給 Vello 之前，把每個 glyph 的原點對齊到 device pixel 的四分之一（沿著行）和整數（垂直方向），只在 matrix 沒有旋轉和歪斜時做。Skia 的 glyph 快取就是這樣量化位置的，machopo 今天的畫面是對齊過的；Vello 位置在哪就畫在哪，所以測試頁和參考頁的同一段文字位置差了浮點數的誤差（十萬分之一個 pixel）時，邊緣的 pixel 會差 1。用 Vello 跑整套 WPT，不對齊時 Vello 造成 39 個測試不通過（34 個 reftest），對齊之後 13 個（8 個 reftest），沒有變壞的；那些都是 WPT 的測試，machopo 自己的 reftest 兩種都全部通過。代價是每個 glyph 最多偏移八分之一個 pixel（和今天一樣），放棄 Vello 本來能給的位置精度。不選不對齊、34 個各加 `fuzzy`（位置最精確，但測試變鬆，WPT 的清單擴大時文字相關的 reftest 會陸續碰到；WPT 的作者預設同一個瀏覽器裡兩頁畫得一樣，Chrome 和 Firefox 都對齊）；不選對齊到六十四分之一（試過剩 11 個 reftest：格線越多，誤差跨過格線的機會越大）。對齊也是 glyph 快取能命中的前提（第 3 步的 atlas）。剩下的 8 個 reftest 逐個看過再加 `fuzzy`，每個寫原因。
30. **2026-11-10：Vello 的 panic：有檢查的 build 不接，release 才接住、那個 tile 留白，並警告**（使用者照建議；DevTools 的警告使用者要求放進第 2 步）。
    - **開著 debug assertions 的 build（開發的 debug build、CI 跑 WPT 和 reftest 的 `ci` profile）不接**：thread 照今天結束，測試判定 crash。接住的話測試頁和參考頁同一塊都留白，reftest 會假通過。Chromium 的 DCHECK 也是測試和開發時致命、出貨的版本才容忍。
    - **release build 接住**（一個 tile 的 playback 包 `catch_unwind`）：那個 tile 留成底色，其他照常；canvas 的那一次繪圖不生效、bitmap 維持原樣（**第 37 條改成不保證**）；那條 thread 的 Vello context 丟掉、下次重建（panic 之後它的狀態不可信）。
    - **警告**：error 等級的 log，帶頁面的網址、tile 的位置和大小、panic 的訊息；次數累計，同樣的訊息只在第一次和次數翻倍時記（同一個 tile 每次重畫都會再 panic）；送一則警告到 DevTools 的 Console（`Log` domain），網頁的作者看得到有一部分畫不出來。raster 在 compositor thread 上，訊息要傳回 renderer 的 main thread 再交給 Console（怎麼傳、網址記在哪一行：第 38 條）。
    - 接不接是 `raster` 的一個參數，預設跟著 build 走，測試可以指定：release 的那條路的測試本身在開著 debug assertions 的 build 裡跑。用已知會讓 Vello panic 的輸入（clip 的溢位），上游修掉之後換一個觸發的方法。
    - 不選不接（任何網頁湊出會 panic 的內容就能讓分頁不再更新）；不選接住之後把那個 tile 的 op 一個一個重試（複雜，重試時每個 op 都可能再 panic，換掉 Skia 之後沒有另一個 backend 可以退）。留白的 tile 畫成棋盤格、記進 trace 是之後的事。
    - 和圖片的解碼一樣，workspace 不能把 `panic` 改成 `abort`（第 20 條）；接不住無窮迴圈和記憶體用光。release 沒有整數溢位的檢查，那個 clip 的 bug 在 release 下是算錯而不是 panic：要不要只對 Vello 的 crate 打開溢位檢查見「待決定」。
31. **2026-11-10：canvas 的 5 個 WPT 的退步，第 2 步先接受、記下來**（使用者照建議）。4 個是破壞性的 blend mode 配 clip（`2d.composite.clip.destination-atop`、`destination-in`、`source-in`、`source-out`：形狀以外的地方會被清掉的四種模式，Vello 不隔離的 clip 限制不了它們，clip 外面的像素也被清掉），1 個是 kurbo 的 stroker 在線寬遠大於線段長度時內側的接合多畫一塊（`2d.line.cross`）。兩種都少見（常見的橡皮擦是 `destination-out`，不受影響）。它們的預期結果標成失敗、寫原因，列進 `render-gaps.md` 和 `docs/limitations.md`，回報上游的草稿寫好給人審。不選第 2 步就在 machopo 這一側修那 4 個（做得到：碰到這四種模式而且有 clip 時，把目前的畫面 render 出來、形狀和 clip 各畫成一張圖，machopo 逐個 pixel 照公式合成、clip 外面保留，再寫回 bitmap；但每次都要整個 canvas 過一遍，多一段自己維護的合成程式，留作第 3 步的候選），也不選現在就改 Vello 的合成（估不出要改多少，上游在 vello_gpu 的文件寫明了這個限制，可能是設計上的取捨；kurbo 是另一個專案）。（**其中 4 個後來發現是 machopo 自己的錯、修掉了：第 41 條。**）
32. **2026-11-10：第 2 步分成四段，每段做完給人審**（使用者照建議）：2a Vello 成為唯一的 playback、測試搬過來和補齊；2b ink bounds 改用 skrifa、mipmap、拿掉 skia-safe；2c panic 的處理和警告；2d WPT 的預期結果、文件、整個階段的檢查。四段都做完才 merge。不選一次做完（改動太大，審不動；拿掉 Skia 之前先確認 Vello 單獨撐得住測試）。
33. **2026-11-11：tile 之間允許差 1**。同一塊內容切成 tile 畫和整塊畫，反鋸齒的邊緣和模糊的範圍裡每個 channel 最多差 1（Vello 的覆蓋率用浮點數算，從不同的原點算捨入不同；Skia 用定點數，沒有這個差異）。測試在 scale 1 的形狀、文字和陰影要求逐位相同，其餘允許差 1。不選要求全部逐位相同（要改 Vello 算覆蓋率的方式）。實作時發現、照測到的結果定的，使用者確認接受；記進列出缺口的文件。
34. **2026-11-11：縮小的圖取一層預先縮好的版本、用 bilinear 畫**（使用者確認）。filter 是 `Medium` 或 `High`、而且在縮小時，照 cc 的規則取層（`MipMapUtil::GetLevelForSize`：下一層只要有一邊比要畫的大小小，就用目前這一層；每一層的邊長是原圖除以 2 的次方、往上取整），那一層從原圖做面積平均，第一次用到時才做，和圖片一起放在快取、一起丟掉。對照 Chromium：cc 的 `SoftwareImageDecodeCache` 也是先縮到一層（`scalePixels`，`kMedium`）、畫的時候降成 `kLow`。和它不同的：
    - cc 對 `kNone` 以外的品質都取層，包括 `kLow`；machopo 的 `Low` 是 canvas 用的，維持不取（Chromium 的 canvas 走不走這個快取沒有查）。
    - cc 的縮小版是另一個快取項目，有記憶體預算、用 LRU 淘汰；這裡跟著圖片，每張圖最多多三分之一，沒有預算。
    - 有來源範圍時 cc 只縮那個範圍；這裡縮整張圖、把範圍換算過去。
    - 不選在相鄰兩層之間內插（原本 Skia 的 `MipmapMode::Linear`；每次畫兩遍、多一個 layer），也不選每種大小各縮一份（大小在動的動畫每一格都重縮）。
35. **2026-11-11：ink bounds 用 skrifa 的外框線算，取控制點的範圍**（使用者確認）。和 Blink 給 canvas 的 `TextMetrics` 用的相同（`SkFontGetPreciseBoundsForGlyph`：glyph 的 path 的 `getBounds()`，不取整，所有平台；沒有 path 的退回 `font.getBounds()`）。假粗體和畫的時候一樣呼叫 `kurbo::expand_path`、同一組參數，再取範圍（不選每邊加固定的量：miter 接合的尖端伸得比加粗的量遠）。點陣的 glyph 照 Vello 擺點陣圖的算法算矩形（那段算法在 Vello 裡不是公開的，照抄一份，用測試對著畫出來的像素）。沒有外框線的 COLR glyph 先當成沒有墨水，記成缺口。不選展平之後算真正的墨水範圍（和 Blink 的數字不同，也比較慢）。
36. **2026-11-11：ink bounds 留在 `raster`，不再拿快取**（使用者確認）。它要和畫出來的一致，假粗體加多少、點陣 glyph 怎麼擺都在 `raster`；假粗體的量抽成一個函式，畫和量都呼叫它。換成 skrifa 之後不需要 typeface 的快取，main thread 量字時不再去拿 canvas raster thread 的快取。不選搬到 `layout`（要加 skrifa 的依賴，上面兩樣要抄一份或再抽出去；把字型相關的東西整理成像 Blink 的 `platform/fonts` 那一層是另一份工作）。
37. **2026-11-11：canvas 的 playback panic 時，bitmap 不保證維持原樣**（使用者選 A；改第 30 條）。Vello 不直接畫在 bitmap 上：先把 bitmap 複製進場景當底，op 都畫完才 render 回去，一次 flush 裡有 `putImageData` 時在它之前先 render 一次。所以 panic 時有三種情況：建場景時 panic（已知的那個 bug 是這種），bitmap 沒被動到；在 `putImageData` 之後 panic，它之前的已經寫回去；在最後 render 的途中 panic（沒有看過實例），bitmap 可能只寫了一部分。三種都照原樣留著，`alpha: false` 的 canvas 仍然補成不透明。不選「每次 flush 先備份整張 bitmap、panic 時還原」：panic 本來不該發生，為了它每一次 canvas 繪圖都多複製一次整張（1920×1080 是 8 MB，現在已經複製一次當底）不划算。Chromium 沒有接住 crash 的路，沒得對照。
38. **2026-11-11：失敗怎麼記、怎麼送到 Console**（使用者照建議）。會 panic 的 thread 都不知道頁面的網址，只有 main thread 知道：
    - **次數和節流在 `RasterCache`**（每個 compositor 一份、每個頁面的 canvas 一份，所以 tile 和 canvas 各算各的）：失敗時才碰，平常的路不多東西。
    - **失敗的那條 thread 立刻記一行 error log**：位置和大小、panic 的訊息、次數，沒有網址。shell 的介面（`SingleThreadProxy`，沒有頁面）只有這一行。不等 main thread：它可能正忙。
    - **tile**：`ToMainThread` 加 `RasterFailed { message, count }`，只在第一次和次數翻倍時送。**canvas**：不加新的路，main thread 在每個 task 之後從那份共用的 cache 拿（canvas raster thread 和 main thread 上畫的都在同一份）。
    - **main thread** 加一筆 `Log` 的 entry（source `rendering`、level `warning`、帶最上層文件的網址）。Console 的每一筆本來就印到 stderr（`[console] 訊息 (網址)`），帶網址的那一行就是它，沒有另外再記一行。
    - 不選只記 log（開著 DevTools 的人看不到）、不選建 compositor 時把網址傳進去（換頁要跟著更新，compositor 多知道一件它不需要的事）。對照 Chromium：它不接 raster 的 crash；最接近的是 GPU context lost，cc 經過 `LayerTreeHostClient` 通知 main thread，Blink 用 `AddConsoleMessage`（`kRendering`）寫進 Console，同一個方向。
39. **2026-11-11：接不接是 `RasterCache` 上的狀態**（使用者選 A）。預設 `!cfg!(debug_assertions)`；`playback_into` 和 `draw_into` 都拿它，呼叫的地方不用改。不選每個入口多一個參數（幾十個呼叫的地方和 canvas 那條路都要傳）、不選 process 層級的開關（測試平行跑會互相影響，renderer 這一側也不加全域狀態）。`RasterCache` 已經不只是快取，改名留給以後。**實作時的調整**：確認時說的是「多一個建構函式給測試指定」，但 compositor 和 canvas 的 cache 都建在別的型別裡面，建構函式傳不進去；改成測試用的 `fail_next_for_testing`（下一條）順便打開，`raster` 自己的測試直接設欄位。接住之後：丟掉那條 thread 的 context；tile 整塊填成底色（buffer 是重複用的，不填會留著上一個 tile 的像素），它要等內容下次失效才重畫；字型和圖片的快取留著（進了快取的都是完整的）。
40. **2026-11-11：測試用注入的 panic，加一個真的 Vello panic**（使用者照建議）。沒有內容能讓 Vello panic（已知的那個被 `shows` 擋在前面），所以 `RasterCache::fail_next_for_testing(次數)` 讓接下來幾次 playback 一開始就 panic、並打開「接」；它掛在 cache 上，不是全域的，release 裡也在（別的 crate 的測試要用），平常是讀一個 atomic。`raster` 另有測試繞過 `shows`、用已知的那個 clip 讓 Vello 真的 panic 走過同一段；上游修好時它和 `upstream_panics_on_a_clipped_stroke_at_the_right_edge` 一起失敗。不寫情境：沒有頁面內容能觸發，記進測試缺口的文件。
41. **2026-11-11：canvas 那 4 個 `2d.composite.clip.*` 是 machopo 自己的錯，修掉了；第 31 條對它們的說明不對**（2d 寫回報的草稿時發現）。這四個測試的 clip 整個在畫布外面（`rect(-20, -20, 10, 10)`）。`Player::shows`（為了避開 Vello 的那個 panic）不把這種 clip 交給 Vello、它底下的東西也不畫，但 `SaveLayer` 照樣推了一層：那一層沒有被 clip，用 `source-in` 這類模式疊回去就把整張 canvas 清掉。改成 clip 藏起來時不推 layer。四個測試通過，預期結果不用標。**Vello 的缺口還在，但範圍小了**：clip 落在畫布裡時，這四種模式還是會清掉 clip 外面（`upstream_doesnt_keep_a_clearing_layer_to_its_clip` 記著），WPT 沒有測到這種。剩下標成失敗的 canvas 測試只有 `2d.line.cross`。
42. **2026-11-11：8 個 reftest 加 `fuzzy`，數字照量到的**（照第 2 步原本的計畫，逐個看過截圖的差異）。7 個每個 channel 最多差 1（1 到 588 個 pixel）：三個 `css-tables/tentative/paint/background-image-*` 和 `box-shadow-041`、`042` 是 8-bit 混色的捨入（f32 的管線會過，第 1b 步量過、太慢不用），`contain-paint-clip-006`（4 個 pixel）和 `nested-overflow-subtree-layout`（1 個）是文字邊緣。`clip-path-animation-custom-timing-function` 最多差 3、628 個 pixel：測試自己允許差到 15、但只允許 480 個 pixel，clip 的邊落在 pixel 之間、整圈反鋸齒的邊都不一樣。不標成失敗（差異都看不出來，標成失敗就守不住真的退步）。數字是 Windows 上量的；文字那兩個和字型有關，CI（Linux）上不一樣的話照那邊的結果改。

## 待決定

- **文字的外觀怎麼處理**：接受比較細、淡的文字；在 playback 這一側調整（例如照 Skia 的做法對文字的覆蓋率套對比的曲線，做不做得到、能多接近還沒查）；或做進 Vello 的 fork。第 1 步看過截圖再定。漸層的 dithering 同樣。
- **不支援的 filter 一直用離屏，還是改在 fork 裡做**：第 1 步量離屏的 color matrix 的成本（第 4 條：慢的話才 fork）；那時再看上游的 PR 合了沒有。
- **release 要不要只對 Vello 的 crate 打開整數溢位的檢查**（第 30 條）：打開的話 Vello 裡的溢位一律變成 panic、被接住，不會算出錯的值繼續跑；代價是它最熱的迴圈多了檢查，慢多少沒有量。第 2 步先不開，量過再定。
- **Vello 的 fork 多久跟一次上游**：每次上游發布都 rebase，還是釘住、需要時再跟；rebase 之後要重新對 reftest 和 `render-gaps.md`。
- **mipmap 的品質和記憶體**：預先縮小用 box filter 還是更好的 filter（對照 Chromium 的 `SoftwareImageDecodeCache` 怎麼縮，原型時查），縮小的版本佔多少記憶體、什麼時候清（`docs/limitations.md` 的「圖片」記著 raster 的圖片快取沒有大小上限）。

## 之後的可能

- **GPU raster**（下一份 RFC，第 8 條）：vello_gpu 畫進 Viz 的 wgpu device 上的 texture，多個 tile 錄進同一個 encoder、一次 submit，省掉上傳；不能用時那個 tile 走 vello_cpu。已經知道的要點：
  - 走 vello_cpu 的條件：tile 裡有 vello_gpu 不支援、會 panic 的功能（mask layer、不隔離的破壞性 blend mode、filter graph、pixmap 當圖片來源），在 raster 前從 op 判斷，不靠 panic 之後退回（`catch_unwind` 只當最後一道防線）；adapter 是 CPU 的（WARP、lavapipe，`crates/viz/src/gpu_context.rs` 分得出來）、headless 和 reftest；device 遺失到重建好之前；使用者用旗標關掉；拆 process 之後 renderer 碰不到 GPU 時。CPU 這條路是正式的路徑，不只是備援。
  - 這份 RFC 用 mask 做的 path union 在 GPU 上要換別的做法，或那些 tile 留在 CPU。
  - raster 的介面（Chromium 的 `RasterInterface` 加上 `SharedImageInterface`）、raster 前的單色分析（`SolidColorAnalyzer`）、CPU 和 GPU 的比對測試（CI 沒有 GPU，用 lavapipe 或 WARP）。
  - glyph 的 atlas（vello_gpu 的 `full_image_support`）：文字不用每個 tile 重新攤平。
  - canvas 先留在 CPU（`getImageData` 要讀回）。
  - 先量：vello_gpu 的覆蓋率還是在 CPU 上算，GPU 只省下填色、合成和上傳。
  - vello_gpu 的細節見 `details.md` 的「vello_gpu 0.3」和「CPU 和 GPU 的結果有多接近」。
- **矩形類的快速路徑**（GPU raster 之後，量過不夠快才做）：單色和圓角的矩形、邊框、`box-shadow` 不經過 tile，直接變成 Viz 的 quad，用 SDF 的 shader 畫（`details.md` 的「GPUI 怎麼畫」）。Chromium 只對單色的 tile 這樣做（`SolidColorDrawQuad`）。
- **WebP**：加 `image-webp` 就有（`details.md` 的「圖片」）。
- **ink bounds 搬出 `raster`**：改用 skrifa 之後只需要字型資料，可以搬到 `layout` 或 `gfx`，renderer 量文字不再經過 `raster`。
- **CSS 的模糊和陰影以外的效果**：`mask-image`、`backdrop-filter`、SVG 的 filter primitive，看 Vello 那時有什麼。
- **OOP-R**（RFC 0004 的 T3）：display list 和字型、圖片要能序列化。
