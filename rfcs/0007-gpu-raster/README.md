# RFC 0007：用 vello_cpu 取代 Skia

- 開始日期：2026-10-05
- 狀態：👍 已接受：2026-11-10（已決定的事項第 7 到 13 條：先嘗試採用 vello_cpu，做完、可行的話下一份 RFC 是 vello_gpu；「待決定」剩下的在各步開始前或看過那一步的結果再定）
- 細節：`details.md`

## 摘要

拿掉 Skia（skia-safe），machopo 用到 Skia 的每個地方都換成純 Rust 的 crate：tile 和 canvas 的 raster 用 Linebender 的 **vello_cpu**，圖片的解碼和讀尺寸用 image-rs 系列的 crate，JPEG 的編碼用 `jpeg-encoder`，glyph 的 ink bounds 用 skrifa。`raster` crate 對外的 API 不變，compositor、renderer 和 Viz 都不用改。

這是一次嘗試：第 1 步先做原型，跑完測試、看過截圖、量過速度，再決定要不要繼續。Vello 沒有、machopo 的 display list 又用到的功能，在 machopo 的 playback 裡用 vello_cpu 現有的功能組出來，不先 fork Vello；缺的每一項都列在 `docs/` 底下新的一份 `render-gaps.md`（下面只寫檔名）。raster 的結果不再以和 Chromium 逐像素一致為目標。

GPU raster（vello_gpu 為主，不能用時退回 vello_cpu）是下一份 RFC，這份只讓 playback 不擋住它。

## 動機

**現況**（細節見 `details.md` 的「現在的 raster」）：

- Skia 只在 `raster` crate 裡（`crates/raster/src/skia_backend.rs` 約 990 行程式加上測試，`crates/raster/src/encode.rs` 的 JPEG），一律是 CPU 的 backend。用到它的地方：
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
- 色彩管理（ICC）和 EXIF 的轉向：今天的 Skia 路徑也沒有（畫進沒有色彩空間的 surface、不讀 orientation），換掉不會變差。
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
- 可變字型：`variations` 轉成 normalized 的座標交給 Vello。
- 合成粗體用 `font_embolden`，合成斜體用 `glyph_transform` 的 skew；粗細要重新對（`docs/limitations.md` 的「Paint 和 raster」的合成粗體那一條跟著改寫）。
- COLR 和點陣的 emoji（CBDT、sbix）由 glifo 畫，要開 `png` feature。
- **glyph 的快取不開**：上游標成高度實驗性、不建議用。文字多的 tile 每次都重新攤平輪廓，第 1 步量這個成本。
- **ink bounds**（`raster::ink_bounds`）改用 skrifa：有輪廓的 glyph 把輪廓畫進 kurbo 的 `BezPath` 取範圍（和 Vello 實際畫出來的一致），點陣的讀 bitmap 的 metrics，再照 embolden、skew 和 stroke 擴大。函式的簽名照舊（`RasterCache` 這個參數留著不用）。

### 圖片

- **解碼**：PNG 用 `png`（machopo 的編碼已經用它，Chromium 的 Skia 也已經改用它），JPEG 用 `zune-jpeg`，GIF 用 `gif`（只有第一格，和今天一樣）。解碼的結果轉成 premultiplied 的 `Pixmap`，照現在的方式依 `ImageData::id` 快取在 process 全域的表裡；縮小的版本跟著同一個項目。
- **讀尺寸**：各 crate 只讀檔頭，不解碼。
- **JPEG 的解碼和 Skia（libjpeg-turbo）不逐像素相同**（IDCT 和色度 upsampling 的做法）：比對 JPEG 和 PNG 的 reftest 可能要 fuzzy，每個寫明原因。
- **JPEG 的編碼**：`jpeg-encoder`（純 Rust）。quality 照 Blink 的設定對到量化表；輸出的 bytes 和今天不同，只要解得回來、品質相當。

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
- 用 crates.io 上發布的版本還是釘上游 `main` 的某個 commit，見「待決定」。

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
3. **改進**（看第 1、2 步的結果，可能不需要）：文字的 gamma 和對比、漸層的 dithering、Vello 的 fork 裡的 `ColorMatrix`；不需要的寫明理由、不做。

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

## 待決定

- **要不要繼續**：第 1 步的兩個檢查點各定一次（1a 看文字的外觀和速度，1b 看整套 reftest 和組出來的功能的成本）。
- **文字的外觀怎麼處理**：接受比較細、淡的文字；在 playback 這一側調整（例如照 Skia 的做法對文字的覆蓋率套對比的曲線，做不做得到、能多接近還沒查）；或做進 Vello 的 fork。第 1 步看過截圖再定。漸層的 dithering 同樣。
- **不支援的 filter 一直用離屏，還是改在 fork 裡做**：第 1 步量離屏的 color matrix 的成本（第 4 條：慢的話才 fork）；那時再看上游的 PR 合了沒有。
- **Vello 的版本怎麼跟**：用 crates.io 上發布的版本（每一版都是 breaking，但有版本號）還是釘上游 `main` 的 commit（拿得到還沒發布的修正，例如合了之後的 `ColorMatrix`）；每次發布都升，還是釘住、需要時再升。升級時要重新對 reftest 和 `render-gaps.md`。
- **mipmap 的品質和記憶體**：預先縮小用 box filter 還是更好的 filter（對照 Chromium 的 `SoftwareImageDecodeCache` 怎麼縮，原型時查），縮小的版本佔多少記憶體、什麼時候清（`docs/limitations.md` 的「圖片」記著 raster 的圖片快取沒有大小上限）。
- **圖片解碼的 crate**：PNG 用 `png` 還是 `zune-png`；要不要直接用 `image` 一整包。第 0 步決定。

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
