# RFC 0007 的細節：用 vello_cpu 取代 Skia

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。各節寫了查的日期：2026-10-05 的是第一次寫這份 RFC 時讀原始碼和文件查的，2026-10-31 的是重新打開時對照 machopo 當時的程式和 vello_cpu 0.3.0 的原始碼查的，2026-11-10 的是重寫 `README.md` 時對照上游的 `main`（commit `112736d`）、上游的 issue 和 PR，以及 Zed 的 GPUI 的原始碼查的；都還沒有原型，沒有實際跑過。

## 現在的 raster

2026-10-31 的程式，2026-11-10 對過還是這樣。

- **`raster` crate 是唯一用到 Skia 的地方**：那時的 `skia_backend.rs`（第 2 步拿掉了）（約 990 行程式，之後是測試）、`crates/raster/src/encode.rs`（PNG 用 `png` crate，JPEG 用 Skia 的 encoder）、`crates/raster/src/lib.rs`（對外的 API 和 `Bitmap`）。依賴 `skia-safe`（prebuilt 的 `jpegd-jpege-pdf`，沒有 GPU）、只為 Skia 拆字型集合用的 `read-fonts`、`png`。
- **對外的 API 和使用者**：
  - `ops_for`、`playback_into`：compositor 準備 tile 的迴圈（`crates/compositor/src/layer_tree_host_impl.rs`）。每個 tile 先用 `ops_for` 從 display list 的 R-tree 找出要畫的 op，再 `playback_into` 畫進那條 thread 重用的 buffer；`--num-raster-threads` 大於 1 時在 rayon 的 raster worker 上。畫完用 `TileDrawInfo::solid_color` 看像素是不是單色（cc 的 `SolidColorAnalyzer` 的位置，但看的是結果），不是的話上傳成 texture。
  - `draw_into`：canvas raster thread 把 2D context 的錄製畫在 canvas 既有的 bitmap 上（`crates/renderer/src/canvas_raster.rs`、`crates/renderer/src/canvas.rs`）。canvas 每邊最多 65535（`crates/dom/src/canvas.rs` 的 `MAX_SIDE`）。
  - `image_size`：`crates/renderer/src/render_thread/documents.rs` 讀圖片的尺寸（Skia 的 `Codec`，只讀檔頭）。
  - `ink_bounds`：`crates/renderer/src/frame_view.rs` 量文字的 ink 範圍（Skia 的 `SkFont::getPath` 的範圍，點陣的 glyph 用 `getBounds`）。
  - `encode_png`、`encode_jpeg`：`toDataURL`、`toBlob`（`crates/renderer/src/canvas_encoder.rs`）。
  - `RasterCache`：typeface（照字型和軸的值）和解碼過的圖片（`Image::from_encoded`，第一次畫時才解碼），`Mutex` 保護，因為 raster 在哪條 thread 上跑不固定。
  - `BACKEND`：log 用的名字（`crates/renderer/src/render_thread.rs`）。
- **display list**（`crates/compositor_paint/src/lib.rs`）：
  - `PaintOp` 24 種：`Save`、`Restore`、`Translate`、`ClipRect`、`ClipRRect`、`ClipPath`、`ClipPaths`、`Concat`、`SetMatrix`、`SaveLayerAlpha`、`SaveLayer`、`DrawColor`、`DrawRect`、`ClearRect`、`DrawRRect`、`DrawDRRect`、`DrawLine`、`DrawClipped`、`DrawShadow`、`DrawPath`、`DrawGlyphs`、`DrawImage`、`WritePixels`、`DrawRecord`。
  - `PaintFlags`：顏色、fill 或 stroke、抗鋸齒、shader、mask blur、dash、cap、join、miter limit、blend mode。
  - `LayerFlags`：alpha、blend mode、`ImageFilter`（`DropShadow`、`Blur`、`ColorMatrix`、`Chain`）。CSS 的 `filter` 的 brightness、contrast、grayscale、sepia、saturate、hue-rotate、invert 都是 `ColorMatrix`（`crates/paint/src/filter.rs`），好幾個 filter function 是 `Chain`。
  - `PaintShader`：`Tiled`（圖片、漸層或另一個 display list 當圖塊，`spacing`、`repeat_x`、`repeat_y`、local matrix、取樣、`decal`）、`Gradient`（linear、two-point conical、sweep；Clamp、Repeat、Mirror；premultiplied 或 unpremultiplied 的插值）、`Empty`。
  - `BlendMode` 28 種（Porter-Duff 的 13 種、`Plus`、12 種可分離和 4 種不可分離的）；`FilterQuality` 4 種（`None`、`Low`、`Medium` 有 mipmap、`High` 放大時 cubic）。
- **Viz**（`crates/viz/src/wgpu_renderer.rs`）：一律經過 wgpu 合成，沒有 GPU 時用 wgpu 的 CPU adapter（WARP、lavapipe）。tile 是 premultiplied 的 RGBA8。這份 RFC 不改它。

## Vello 系列的現況

- **Linebender 2026-09 重整了 Vello 的 repo**（issue #1871，PR #1872、#1883、#1895）：
  - sparse strips 的兩個 renderer 成為預設：**vello_cpu** 和 **vello_gpu**（原本叫 vello_hybrid，2026-09-08 改名），2026-10-31 都在 repo 的最上層（`vello_cpu/`、`vello_gpu/`、`vello_common/`、`glifo/`）。
  - 原本的 compute 版 `vello` 搬到 `research/`，README 寫它是「experimental」。
  - README 的說法：Vello GPU 打算在成熟之後成為 GPU 的主要 renderer；**Vello CPU 目前整體比較成熟**。
- **版本**：vello_cpu 0.3.0、vello_gpu 0.3.0、vello 0.11.0 都在 2026-10-02 發布，2026-11-10 還是最新（上游的 `main` 一直在改，但 `Cargo.toml` 的版本還是 0.3.0；`main` 的 skrifa 已經是 0.49）。vello_cpu 0.3 依賴 vello_common 0.3、glifo 0.4（skrifa 0.44）；vello_gpu 依賴 wgpu 30，和 machopo 一樣。
- **沒有穩定的承諾**：每一版都是 breaking。vello_cpu 的發布：0.0.8（2026-05-15）、0.0.9（2026-05-30）、0.1.0（2026-07-29）、0.2.0（2026-08-07）、0.3.0（2026-10-02）。MSRV 是 1.89，可能在 patch 版本裡提高（machopo 釘在 1.99）。
- **沒有共用的抽象**：`vello_api`（0.0.7）原本想統一兩個 renderer 的 API，2026-04-25 的 PR #1602 把它從兩邊拿掉了。Linebender 的 2026 Q1 回顧改推第三方的抽象層（Blitz 的 AnyRender、forest-rs 的 imaging）。vello_cpu 的 `RenderContext` 和 vello_gpu 的 `Scene` 方法幾乎一樣，用的也是 vello_common 的同一組型別；這份 RFC 只用 vello_cpu，不先包抽象，等 GPU raster 的 RFC 再說。
- **誰在用**：
  - **Servo**：canvas 2D 預設、而且唯一必要的 backend 是 vello_cpu（開 `multithreading`）；經典的 vello 是 pref 後面的選項。2026-01 把預設改成 vello_cpu，理由是 GPU 版每個 frame 都要讀回 CPU、再上傳，比 CPU 版慢很多。Servo 畫網頁用的是 WebRender，不是 Vello，也沒有用 vello_gpu。
  - **Blitz**（Dioxus 的 HTML/CSS 引擎）用 Vello 系列畫整個網頁（經過 AnyRender）。

## vello_cpu 0.3

2026-10-31 讀 `vello_cpu/src/render.rs`、`vello_cpu/src/filter/`、`vello_common/src/filter/` 和 README。

- **API**：`RenderContext::new(w, h)`，或用 `RenderSettings { level, num_threads }` 建。是有狀態的 context：`set_transform`、`set_paint`（顏色、漸層、圖片）、`set_paint_transform`、`set_stroke`（kurbo 的 `Stroke`：寬度、cap、join、miter、dash）、`set_fill_rule`、`set_blend_mode`（不隔離的）、`set_aliasing_threshold`；畫的是 `fill_path`、`stroke_path`、`fill_rect`、`stroke_rect`、`fill_blurred_rounded_rect`（只有一個圓形的半徑、只支援單色；`fill_blurred_rounded_rect_in` 多一條決定畫到哪裡的 path 和 `invert`，畫反過來的，給 inset 的陰影）、`glyph_run`；layer 是 `push_layer(clip, blend, opacity, mask, filter)`（各自可選）和 `pop_layer`，另有不隔離的 clip（`push_clip_rect`、`push_clip_path`、`pop_clip`）。圖片用 `register_image` 登記成 `ImageId`。`reset`、`reset_and_resize` 重用 context。
- **render**：`render_with(target, resources, RasterizerSettings)` 畫進 `PixmapMut`。`TargetInit::Clear(color)` 先清成某個顏色，`TargetInit::SrcOver` 畫在既有的像素上（canvas 的 `draw_into` 要的）。context 的大小就是場景的範圍：**context 外面（左上角之前、寬高之後）的內容一律丟掉**；`offset` 決定場景放在 pixmap 的哪裡，負的位移把左上角裁掉。
- **輸出**：premultiplied RGBA8，和 machopo 的 `Bitmap` 一樣。寬高是 `u16`，最大 65535，和 canvas 的 `MAX_SIDE` 一樣。
- **兩種 pipeline**：`u8_pipeline`（預設）和 `f32_pipeline`（比較慢、精度高）。
- **SIMD 和多執行緒**：NEON、SSE2 到 AVX-512（fearless_simd），沒有的話用純量。`multithreading` feature 打開時，一個 context 內部用多條 thread 畫，預設 `min(核心數 - 1, 8)`；這時連單一的 filter 都會 panic（`push_layer` 的文件）。
- **filter 實際做了的只有四種**：vello_common 的 `FilterPrimitive` 列了 17 種（Flood、GaussianBlur、DropShadow、DropShadowOnly、ColorMatrix、Offset、Composite、Blend、Morphology、ConvolveMatrix、Turbulence、DisplacementMap、ComponentTransfer、Image、Tile、DiffuseLighting、SpecularLighting），但 `PreparedFilter::new` 只認得 Flood、GaussianBlur、Offset、DropShadow（含 shadow only），其他是 `unimplemented!("Other filter primitives not yet implemented")`；超過一個 primitive 的 graph 是 `unimplemented!("Multi-primitive filter graphs are not yet supported")`。GaussianBlur 只有兩個方向一樣的 σ。filter 照 layer 當時的 transform 縮放（`FilterData::new(filter, layer_transform)`），和 machopo 的 filter「在目前的空間裡」一致。
- **filter layer 的範圍**：filter 的 pixmap 是 layer 裡 strip 的範圍加上 `filter_expansion`，對齊 tile；左上角之前的內容靠事先位移保住（`source_shift`）。但 strip 只在 context 的範圍裡產生，所以 context 右邊、下面之外的內容不會進 filter：machopo 一個 tile 一個 context，模糊和 drop shadow 跨過 tile 邊界時，鄰近的 tile 看不到對方的內容，會有接縫（讀原始碼的推論，原型時確認）。做法：有這種 filter 的 tile 用四周各加 `filter_expansion` 的 context，transform 多位移一個 expansion，render 時 `offset` 用負的 expansion 裁回 tile。
- **mask**：`push_mask_layer(Mask)`，mask 要和 context 一樣大、不受 transform 影響；有 alpha 和 luminance 兩種。
- **不隔離的 blend**：`render_with` 的註解寫著背景永遠是隔離的，不用擔心破壞性的 blend mode；不隔離的 Clear、Copy、SrcIn 這些在 vello_cpu 上的結果和 Skia 的逐個 op 合成是否一致，原型時確認（canvas 的 `globalCompositeOperation`）。
- **結果在不同機器上不完全一樣**：上游的測試以 f32 的純量版本為準，f32 加 SIMD 容許每個 channel 差 1，u8 容許差 2；多執行緒（f32、純量）要求完全一樣。machopo 的 reftest 是同一台機器、同一個 process 裡比兩張圖，不受影響；要跨機器比對固定的圖檔時，要釘住 SIMD 的等級和 pipeline。
- **README 的限制**：複雜的 filter graph 完全不支援、會 panic；多執行緒時連簡單的 filter 也不支援；有些功能（包括 glyph 快取）還是實驗性的，不建議用；x86 上的效能還有改進的空間。

## PaintOp 怎麼對應

2026-10-31 的看法，原型時確認。

| machopo 的 op 或功能 | vello_cpu | 要注意的 |
| --- | --- | --- |
| `Save`、`Restore` | playback 自己維護堆疊：transform，以及這一層推了幾個 clip 或 layer，`Restore` 時一一 pop | Vello 的 layer 不保存 transform |
| `Translate`、`Concat`、`SetMatrix` | 改堆疊頂端的 `Affine`，再 `set_transform`；`SetMatrix` 相對於 tile 的起始矩陣 | |
| `ClipRect`、`ClipRRect`、`ClipPath` | `push_clip_rect`（對齊 pixel 的走快路）、`push_clip_path`（kurbo 的 `RoundedRect`、path 照它的 fill rule） | `anti_alias: false` 用 `set_aliasing_threshold`：覆蓋率過門檻就整個 pixel，和 Skia 不抗鋸齒時的掃描規則不同 |
| `ClipPaths` | 每條 path 照自己的 fill rule 畫進一個和 tile 一樣大的 `RenderContext`，做成 alpha 的 `Mask`，`push_mask_layer` | 覆蓋率的聯集是 1 − (1 − a)(1 − b)，Skia 先用 `PathOps` 算出一條 path；只差在反鋸齒的邊。沒有 path 時什麼都不畫 |
| `SaveLayerAlpha`、只有 alpha 和 blend mode 的 `SaveLayer` | `push_layer`，`bounds` 當成 clip | |
| `SaveLayer` 帶一個 `Blur` 或 `DropShadow` | `push_layer` 帶 Vello 的 filter；tile 的 context 照「vello_cpu 0.3」的做法加大 | |
| `SaveLayer` 帶 `ColorMatrix` 或 `Chain` | 離屏：layer 的內容（到對應的 `Restore`）畫進另一個加大的 `RenderContext`，render 成 pixmap，照順序套每個 filter（`ColorMatrix` 由 machopo 照 Skia 先 unpremultiply、乘矩陣、clamp、再 premultiply；`Blur`、`DropShadow` 把 pixmap 當圖片畫進另一個帶 Vello filter 的 context），最後當成圖片、帶 layer 的 alpha 和 blend mode 畫回去 | 外層的 clip 留在主 context，layer 裡面的 clip 在離屏的 context 裡重播；可以巢狀。成本是多一次 render 和 pixmap |
| `DrawColor` | 對整個 tile `fill_rect`，blend mode 照 op（`Src` 是 clear） | 受目前的 clip 限制 |
| `DrawRect`、`DrawRRect`、`DrawLine`、`DrawPath` | `fill_rect`、`fill_path`、`stroke_rect`、`stroke_path`；`DrawPath` 用它的 fill rule 填成一條 path | 一條 path 內共用的邊沒有縫，和現在一樣 |
| `ClearRect` | `fill_rect`，blend mode 是 Compose 的 `Clear` | |
| `DrawDRRect` | 外面的圓角矩形加上反方向的裡面的，even-odd；`clip` 的多邊形用 `push_clip_path` | |
| `DrawClipped` | `push_clip_path`（多邊形）、畫 `op`、`pop_clip` | |
| `DrawShadow` | 沒有 `hole`、`shape` 的四個角相同而且是圓的：`fill_blurred_rounded_rect`（inset 用 `_in` 的 `invert`），外面套 `clip`；其他的（四個角不同或橢圓的角、有 `hole` 的）包一層 GaussianBlur 的 filter layer | `fill_blurred_rounded_rect` 只有一個圓形的半徑（`RRectF` 是四個角各自的橢圓半徑），只支援單色（陰影本來就是單色）；2026-10-31 的看法漏了半徑這一點 |
| `PaintFlags.blur`（mask blur） | 單一圓形半徑的圓角矩形同上；其他形狀包一層 GaussianBlur 的 filter layer | σ 在目前的空間裡，和 filter 的縮放一致 |
| dash、cap、join、miter limit | kurbo 的 `Stroke`（`dash_pattern`、`dash_offset`） | 長度是 0 的 dash 加圓頭這類邊界情況要對 Skia |
| `PaintFlags.blend_mode` | `set_blend_mode`（不隔離） | 見「vello_cpu 0.3」的不隔離的 blend |
| 漸層 | peniko 的 linear、two-point radial、sweep；`TileMode` 的 Clamp、Repeat、Mirror 對到 `Extend` 的 Pad、Repeat、Reflect；`interpolation_alpha_space` 對 premultiplied 和 unpremultiplied；local matrix 用 `set_paint_transform` | sweep 的角度單位和起點、unpremultiplied 的結果原型時對 Skia；沒有 dithering |
| `DrawGlyphs` | 見「文字」 | `rotations` 不是空的時一個 glyph 一個 run |
| `DrawImage` | 解碼成 `Pixmap`、`register_image`，`ImageBrush` 填 `dst` 的矩形，`ImageSampler` 的 `alpha` 帶 flags 的 alpha | `src` 是一部分時，裁成另一張 pixmap（依 `id` 和 `src` 快取），才不會取到 `src` 外的像素（Skia 的 strict constraint）；取樣見「圖片」 |
| `PaintShader::Tiled` | `ImageBrush`，`Extend` 照 `repeat_x`、`repeat_y`（Repeat 或 Pad），local matrix 和圖塊的位置用 `set_paint_transform` | 圖塊是漸層或 display list（`ShaderImage::Record`）時，照當時的 scale 先畫成 pixmap；`spacing` 畫進四周留空白的 pixmap；`decal` 改成 clip 到圖塊 |
| `DrawRecord` | 照現在的做法，在 transform 和 clip 裡把那個 display list 重播成向量 | |
| `WritePixels` | 前面的 op 先 render 進 bitmap（`TargetInit::SrcOver`），寫入像素，`reset` 之後接著畫 | 只出現在 canvas 的錄製裡 |

## 上游缺什麼

2026-11-10 對照上游的 `main`（`112736d`）、issue 和 PR。第 2 步 merge 時這張表整理成 `docs/` 底下新的 `render-gaps.md`，之後以那份為準。

**結論：沒有非 fork 不可的缺口。** 缺的都能在 playback 裡組出來，或當成外觀的差異接受。

在 playback 裡補、使用者看不出來的（代價是速度，只在用到的頁面上）：

| 缺的 | 上游的狀態 | machopo 哪裡用到 | 做法 | 什麼時候可以拿掉 |
| --- | --- | --- | --- | --- |
| `ColorMatrix` filter | `PreparedFilter::new` 碰到是 `unimplemented!`。PR #1765（2026-07-24 開，2026-11-02 還在更新）CPU 和 GPU 都做，它的前一版 #1623 已經 approve；作者要先合另一個讓 filter 的資料長度可變的重構。追蹤的 issue 是 #1296 | CSS `filter` 的 brightness、contrast、grayscale、sepia、saturate、hue-rotate、invert（`crates/paint/src/filter.rs`） | 離屏，machopo 自己套矩陣 | 上游合了、發布之後，單一的 color matrix 直接交給 Vello |
| 多個 primitive 的 filter graph | `unimplemented!`；issue #1296 只有清單，沒有人在做。#1696（碰到不支援的 filter 就略過、不 panic）沒合就關了 | `ImageFilter::Chain`（好幾個 filter function） | 離屏，照順序一個一個套 | 上游做了 graph 的執行 |
| 模糊的圓角矩形只有一個圓形的半徑 | issue #1245（橢圓和 superellipse 的角）開著 | `DrawShadow`、mask blur：`RRectF` 是四個角各自的橢圓半徑 | 四個角相同而且是圓的走快路，其他的包一層 blur 的 filter layer | 上游支援每個角的半徑 |
| path 的布林運算 | kurbo 和 Vello 都沒有 | `ClipPaths` | 每條 path 畫進 alpha mask，`push_mask_layer` | 不會拿掉（除非 kurbo 加上布林運算） |
| mipmap | 沒有；issue #1289（縮小超過 2 倍品質明顯變差）開著。vello_gpu 的 atlas 也只有 level 0 | `FilterQuality::Medium` 縮小（`<img>` 和背景） | 圖片快取預先縮成 2 的次方 | 上游的圖片取樣支援 mipmap |
| 圖塊是 display list、有 `spacing`、`decal` 的圖案 | 圖片的 paint 只有 `Extend` | `PaintShader::Tiled` | 先畫成 pixmap；`decal` 改用 clip | 不會拿掉 |

接受的差異（使用者看得到，或只在比對時看得到）：

| 差異 | 上游的狀態 | 看得出來嗎 | 對測試 |
| --- | --- | --- | --- |
| 文字沒有 gamma 和對比的調整 | issue #1694（深色背景上的文字模糊）開著；Blitz 的追蹤 issue #1491 列了「讓加粗的文字不那麼糊（gamma 校正？）」，還沒做 | **看得出來**：字比較細、淡，深色背景上的淺色字最明顯 | 文字和色塊、圖片比的 reftest 可能要 fuzzy |
| 漸層沒有 dithering | issue #856 開很久了 | 大面積、顏色相近的漸層可能有色帶 | Skia 兩邊的雜訊剛好一樣；拿掉之後漸層對漸層的 reftest 反而容易一致 |
| 反鋸齒的邊、不抗鋸齒時的規則 | 設計不同，不是缺 | 放大才看得出來 | `raster` 的單元測試裡檢查邊緣像素數值的要重新定預期值 |
| JPEG 的解碼 | 和 Vello 無關，是 decoder 不同 | 每個 channel 差 1、2，看不出來 | JPEG 和 PNG 比的 reftest 可能要 fuzzy（machopo 自己的 reftest 只有 1 個檔案用到 JPEG） |
| 合成粗體 | `font_embolden` 的參數和 Skia 不同 | 粗細略有不同 | 沒有測試比這個 |

只在速度上看得出來的：

- **glyph 的快取**：`atlas_cache` 的文件寫著「highly experimental and not recommended for external use」，README 也把它列在不建議用的實驗性功能；Blitz 的追蹤 issue 把「Glyph caching」列在還沒做的效能項目。不開，第 1 步量文字多的 tile。
- **x86 的效能**：README 說 x86 上還有改進的空間（Windows 的開發機是 x86）。

machopo 用不到、不用處理的：

- 多執行緒時連單一的 filter 都 panic：machopo 不開 vello_cpu 的多執行緒。
- LCD 的 subpixel 文字：machopo 現在也只用灰階。
- `backdrop-filter`、alpha 的 mask（CSS 的 `mask`）、相對於場景的 paint transform（`background-attachment: fixed`、`background-clip: text`）：Blitz 的追蹤 issue 列著還沒做，machopo 現在也沒有這些功能。

對測試的影響：machopo 沒有存起來的參考圖（`tests/` 底下的 PNG 只有 reftest 用的圖片素材）。reftest 和 WPT 的 reftest 是同一次執行裡畫兩個頁面互相比，情境的 `capture` 和 `expect-differs` 比的是同一次執行裡前後的 frame，兩邊用同一個 renderer，gamma 和 dithering 是兩邊一起變。會壞的是兩邊用不同方式畫出同一個結果的 reftest、`raster` 裡對著 Skia 寫的檢查確切像素的單元測試、檢查 JPEG 編碼輸出的 bytes 的測試。實際有幾個，第 1 步跑了才知道。

## 文字

- glyph 由 glifo（skrifa）取輪廓、當成 path 畫。也支援 COLR、VARC、點陣字型（CBDT、sbix，要開 `png` feature）；glyph 的快取還是實驗性的。
- **hinting**：只有垂直方向，而且只在等比例縮放、沒有旋轉時才套用。machopo 現在對 Skia 就是關掉 hinting、用 subpixel 的位置（位置由 layout 決定），照樣關掉。
- **只有灰階的反鋸齒**，沒有 LCD subpixel；machopo 現在用的也是灰階（`Edging::AntiAlias`），這點不變。
- **沒有 gamma 和對比的調整**：Skia 畫文字時會調整 gamma 和對比，Vello 沒有（經典 vello 的 issue #1694「深色背景上的文字模糊」就歸因於此）。預期文字會比現在細、淡一些，和 Chromium 的差異變大。
- **合成粗體**：`font_embolden` 用 kurbo 把輪廓外擴，參數和 Skia 的 embolden 不同，粗細要重新對（`docs/limitations.md` 的「Paint 和 raster」記著測試字型的 bold 是合成的）。
- **字型集合**：Vello 直接吃 font 的資料加上 index，不需要現在為了 CoreText 把 face 拆出來的程式。
- **ink bounds**：skrifa 的 `OutlineGlyph` 照 normalized 的座標畫進 kurbo 的 `BezPath`，取 `bounding_box`（和 glifo 畫出來的輪廓一致）；沒有輪廓的點陣 glyph 讀 CBDT、sbix 的 metrics，COLR 的用它的 ClipBox（沒有的話用各層輪廓的聯集）。embolden 外擴的量、skew、stroke 的寬度再加上去。Blink 的 ink bounds 一樣在 CPU 上算（經過 Skia 的 `SkFont::getBounds`）。

## 圖片

- **解碼改用 image-rs 系列的 crate**：PNG（`png`，或 `zune-png`）、JPEG（`zune-jpeg`）、GIF（`gif`，第一格）。Skia 的 `Image::from_encoded` 和讀尺寸的 `Codec` 一起換掉；讀尺寸只讀檔頭，不解碼。解碼的結果轉成 premultiplied 的 `Pixmap`，照現在的方式依 `ImageData::id` 快取。
- **解碼一律在 CPU 上**：JPEG、PNG 的時間大多花在 entropy 解碼（Huffman、deflate），只能一個 bit 一個 bit 依序做。Chromium 的「GPU image decode」也是 CPU 解碼、上傳成 texture 快取；硬體解 JPEG 只在 ChromeOS 的 VA-API。
- **JPEG 的解碼結果和 libjpeg-turbo 不逐像素相同**：IDCT 的精度和色度的 upsampling（fancy upsampling）做法不同。比對 JPEG 和另一張圖的 reftest 可能要 fuzzy。
- **Chromium 的 PNG 已經是 Rust 的 `png` crate**：Skia 的 `SkPngRustCodec` 和 `SkPngRustEncoder`；machopo 的 `encode_png` 照它的編碼設定。
- **JPEG 的編碼**：`jpeg-encoder`（純 Rust）。2026-11-10 讀 `image` 的 `src/codecs/jpeg/encoder.rs`：發布的 0.25.10 的 encoder 是它自己的實作（Annex K 的量化表照 libjpeg 的算式縮放，取樣寫死 1×1，只有 `new_with_quality` 和 `set_pixel_density`）；還沒發布的 `main` 改成包著 `jpeg-encoder`（`ChromaSubsampling` 的 S444、S422、S420，`set_optimize_huffman_tables`、`set_progressive`，預設 quality 90 以上 4:4:4、以下 4:2:0）。2026-10-31 寫的「`image` crate 用它」對 `main` 成立，對發布的版本不成立。`jpeg-encoder` 的 `SamplingFactor` 有 `R_4_4_4`、`R_4_2_0` 等，`QuantizationTableType::Default` 是 Annex K 的表。Blink 用 libjpeg-turbo，quality 照它的設定對到量化表；輸出的 bytes 和今天不同。`toBlob` 的編碼和今天一樣在 renderer 裡做。
- **縮小時沒有 mipmap**：現在照 Chromium 用 bilinear 加 mipmap（`image-rendering: auto`，`FilterQuality::Medium`）。Vello 沒有 mipmap，大幅縮小的圖片會有鋸齒和閃爍。做法：圖片快取的項目照需要的 scale 準備 2 的次方的縮小版本（第一次用到那一層時才做），playback 依 `dst` 和 transform 算出縮小的比例，選比它大、最接近的那一層，交給 Vello 用 bilinear 取樣。cc 的 `SoftwareImageDecodeCache` 也是先把圖縮好再交給 Skia。`None` 是 Vello 的 `Low`（nearest），`Low` 是 `Medium`（bilinear、沒有 mipmap），`High` 放大時是 `High`（bicubic）、縮小時和 `Medium` 一樣。
- **WebP 變得很便宜**：P2 不做 WebP，是因為只多開 `webp-decode` 的 skia-safe 組合沒有 prebuilt（`rfcs/pre-rfcs/P2.md` 的「已決定的事項」第 7 條）。拿掉 Skia 之後，加上 `image-webp` 就有了。
- **色彩管理和 EXIF**：今天的 Skia 路徑畫進沒有色彩空間的 surface（不照 ICC 轉換），也不讀 EXIF 的 orientation；換掉之後一樣。之後要做時，色彩管理用 `moxcms` 或 `qcms`（Firefox 的），orientation 由 decoder 讀 EXIF。

## 效能

- **CPU**：Linebender 的 benchmark（Blend2D 測試工具的 fork，https://laurenzv.github.io/vello_chart/）在 Apple M1 Pro（2026-06-08）上，以 Blend2D 單執行緒為 1，各測試的幾何平均：vello_cpu 單執行緒 0.61、2 條 1.10、4 條 1.66、8 條 1.85；Skia 0.24；tiny-skia 0.13；Cairo 0.14。這是 2026-10-05 研究時自己從原始資料算的，不是官方數字。也就是說，在這個測試工具裡，vello_cpu 單執行緒大約是 Skia CPU 的 2.5 倍。網頁的 tile 和這些測試的內容不同（文字多），README 也說 x86 上還有改進的空間，要在 machopo 裡實測（Windows 的開發機是 x86）。
- **glyph 快取還是實驗性的**：文字多的 tile 每次都要重新攤平輪廓，可能是 machopo 的主要成本，要實測。
- **離屏的 filter**：多一次 render 和一張 tile 加 expansion 大小的 pixmap；CSS `filter` 的元素多的頁面（Speedometer 裡少見）才明顯。
- **量法**：`bench.py raster`（捲動、改視窗大小時 compositor 的 PrepareTiles，`docs/benchmarks.md`）、`bench.py blink-perf paint/ image_decoder/`。

## Vello 的 fork

2026-10-31 查的。

- **上游改得多快**：2026-07-31 到 2026-10-31 的三個月，整個 repo 至少 100 個 commit（GitHub 的 API 一頁最多 100 個）；碰到 machopo 可能改的地方的 commit 數：`vello_common/src/filter` 3 個、`vello_cpu/src/filter` 1 個、`vello_cpu/src/fine`（上色和合成）12 個、`glifo` 15 個、`vello_gpu/src` 52 個。
- **所以**：
  - filter 的 patch（原生的 `ColorMatrix`、多個 primitive 的 graph、tile 邊緣）衝突的風險低；上游已經在 enum 裡列了這些 primitive，自己做了之後 machopo 的 patch 就丟掉。
  - 文字的 gamma、漸層的 dithering 改在 `fine`，風險中等，而且上游不一定接受。
  - vello_gpu 改得最兇，GPU raster 的 RFC 盡量不在那裡放 patch，在 playback 裡繞過。
- **rebase 的成本大多不是 fork 造成的**：每一版都是 breaking，machopo 的 playback 本來就要跟著改；fork 多的只是 patch 的衝突。
- **和 machopo 其他的 fork 比**：parley 是小 patch、跟上游的 fork；rusty_v8 是 machopo 自己接手的 crate，因為它只是 V8 的 binding 層，V8 本身照樣跟上游升級。Vello 沒有這樣一層可以切開，自己接手就要自己維護 renderer 本身。
- **Chromium 對照**：Chromium 不 fork Skia（Google 自己的專案，功能直接做在上游，autoroller 每天往前推）；真正的第三方函式庫（ffmpeg、libjpeg-turbo）是 fork 或帶 patch、定期 merge 上游。machopo 和 Vello 的關係像後者，通用的改動送回上游讓 fork 變小。
- **授權**：Vello 是 MIT 或 Apache-2.0，fork 的改動維持同樣的授權，才能送回上游。
- **`unsafe`**：改到 SIMD 的程式時用 fearless_simd 的 safe API，不在 fork 裡新增要自己論證的 `unsafe`。

## vello_gpu 0.3

2026-10-05 讀的，2026-10-31 對過 README，2026-11-10 對過 `main` 的 `vello_gpu/src/scene.rs` 和 README。給之後的 GPU raster 的 RFC。

- **名字**：它原本叫 vello_hybrid（2026-09-08 改名），本身就是混合的做法：覆蓋率在 CPU 上算，上色和合成在 GPU 上。Vello 系列的三個 renderer：vello_cpu（都在 CPU）、vello_gpu（混合）、經典的 vello（都在 GPU，compute shader，上游當成研究用）。
- **vello_cpu 有、vello_gpu 會 panic 的**（2026-11-10）：mask layer（`unimplemented!`）；不隔離的破壞性 blend mode（`set_blend_mode` 的 assert，文件說要的話改用 clip layer）；pixmap 當圖片來源（要先 `upload_image` 進 atlas）；`blurred_rounded_rect` 的 feature 沒開時的模糊圓角矩形。filter 和 vello_cpu 一樣只有四種。

- **做法**：CPU 用 vello_common 的程式把路徑攤平、切 tile、產生 strip，連每個 pixel 的覆蓋率（alpha）都在 CPU 上算好；GPU 收到 alpha 的 texture 和 strip，用 vertex 和 fragment shader 上色、合成、blend。**不需要 compute shader**，另有 WebGL2 的 backend。
- **API**：`Scene::new(w, h)`，畫的方法和 vello_cpu 的 `RenderContext` 對應。`Renderer::new(&Device, &RenderTargetConfig { format, width, height })` 回傳 `(Renderer, Resources)`；`Renderer::render(&Scene, &mut Resources, &Device, &Queue, &mut CommandEncoder, &RenderSize, &TextureView, depth, &TextureBindings, TargetInit) -> Result<(), RenderError>`：畫進呼叫的人自己的 texture，**錄進呼叫的人給的 `CommandEncoder`**，所以很多個 tile 可以錄在同一個 encoder 裡、一次 submit。
- **圖片**：`upload_image` 放進 atlas（`full_image_support` feature 加上上傳的圖片、glyph atlas 的快取和 bicubic）；也可以把別的 wgpu texture 當成 paint（`ImageSource::ExternalTexture`）。
- **filter**：和 vello_cpu 一樣只有 Offset、Flood、GaussianBlur、DropShadow 四種 pass（`vello_gpu/src/filter.rs`，其他是 `unreachable!`）。
- **還不成熟的地方**（README 的「Current state」）：比 vello_cpu「稍微不成熟」；mask layer、複雜的 filter graph、不隔離的某些 blend mode 還不支援，會 panic；有些失敗是 panic 而不是回傳錯誤；wgpu 的 backend 還沒完全最佳化；沒有公開的 benchmark。README 說它在向量多、每個 frame 都變的場景比不上 compute 版。Linebender 2026-04 的回顧說它「大約 beta 的品質」。
- **還不知道的**：寫進 target 的是不是 premultiplied（Viz 的 shader 假設 tile 是 premultiplied）；`Renderer` 能不能在 compositor thread 上用、和 Viz thread 共用一個 device 時有什麼限制；atlas 放不下的圖片怎麼處理。

## CPU 和 GPU 的結果有多接近

2026-10-05 讀的。

- 兩邊的攤平、筆畫、切 tile、strip 和覆蓋率是**同一份 vello_common 的程式**，差別只在上色和合成的精度。
- 上游的共用測試（約 505 個）同時跑 vello_cpu 的各種組合、vello_gpu 的 wgpu 和 WebGL，都和同一張參考圖比：GPU 預設容許每個 channel 差 1；約 38 個測試放寬到 1、21 個到 2、13 個到 3、8 個到 4 至 7（bicubic 的圖片、color-dodge）；19 個測試在 GPU 上跳過，包括所有點陣 emoji 的測試（原因沒寫）。
- 所以兩邊**幾乎一樣，但不是逐像素相同**。比起 Skia 的 CPU 和 GPU backend（覆蓋率的算法不同），這是很大的優勢：之後加 GPU raster 時，視窗裡畫的和 reftest 畫的差異小很多，同一個 frame 裡有些 tile 走 GPU、有些走 CPU 也看不出接縫。

## GPUI 怎麼畫

2026-11-10 讀 Zed 的 `main`（commit `f0d896f`）：`gpui` crate 的 `scene.rs` 和 `window.rs`、`gpui_wgpu` 的 `wgpu_renderer.rs` 和 `shaders.wgsl`，Windows 的 `directx_renderer.rs` 只看了欄位，Apple 的 Metal renderer（`gpui_apple`）沒有讀。查它是為了回答「能不能把圖形直接丟到 GPU、全程在 GPU 上畫」。

- **Scene 只有 8 種 primitive**：`Shadow`、`Quad`、`Path`、`Underline`、`MonochromeSprite`、`SubpixelSprite`、`PolychromeSprite`、`Surface`。每種是一個 `#[repr(C)]` 的 struct，直接當成 instance 的資料寫進 GPU。
- **每個 frame 一個 encoder、一次 submit**：primitive 照繪製順序排好，同種類、同一張 atlas 的併成一批（`Scene::batches`），每批一次 instanced draw，直接畫進視窗的 texture。
- **clip 只有矩形**：`ContentMask` 就是一個 `Bounds`，在 shader 裡算離 clip 邊的距離。沒有圓角或任意 path 的 clip，沒有 blend mode、filter 和離屏的 layer；透明度乘進顏色。
- **圓角、邊框、虛線、陰影在 fragment shader 裡算**：`quad_sdf` 算圓角；陰影用 `erf` 算解析式的高斯，沒有模糊的 pass。背景是單色或兩個色標的線性漸層。
- **文字、SVG、圖片在 CPU 上畫好再放進 atlas**：glyph 由平台的文字系統畫（Windows 是 DirectWrite，Linux 是 cosmic-text），水平方向量化成 4 個 subpixel 位置，照這個 key 快取；SVG 用 resvg（tiny-skia）；之後畫的只是貼圖的 sprite。
- **path 是唯一在 GPU 上算覆蓋率的**：CPU 把 path 拆成三角形扇，二次曲線用 Loop-Blinn 的做法在 fragment shader 裡判斷內外；先畫進一張和視窗一樣大的中間 texture（可以開 MSAA），再貼回主畫面。每一批 path 都要中斷主要的 render pass 再重開。
- **快取是重播上一個 frame 的 primitive**：沒有 dirty 的 view 用 `Scene::replay` 把上一個 frame 的那一段 primitive 複製過來。省的是 layout 和 paint，GPU 還是每個 frame 重畫整個視窗；沒有 tile。
- **後端**：Linux 和 web 用 wgpu，Windows 用 Direct3D 11，Apple 用 Metal，各寫一份 shader。

和 machopo 比：

| | GPUI | machopo |
| --- | --- | --- |
| 覆蓋率在哪算 | 矩形類在 shader；文字、SVG、圖片在 CPU；只有 path 在 GPU | 全部在 CPU（raster） |
| clip | 矩形 | 矩形、圓角、path、path 的 union |
| 效果 | 透明度乘進顏色 | 28 種 blend mode、filter、layer |
| 沒變的內容 | 重播 primitive，GPU 重畫 | tile 留著，GPU 只重新合成 |
| 捲動 | 重畫整個視窗 | compositor thread 移動 tile |

結論：GPUI 不是「全程 GPU」，它是把問題縮到 8 種 primitive、每種一個 shader；UI 只有幾千個簡單的 primitive，所以可以每個 frame 全部重畫。網頁要的任意 clip 和 blend mode 一進來，就得有離屏的 layer 和通用的 path raster，那是 Vello 在做的事。它的做法比較像 WebRender（`docs/architecture.md` 已經否決）。值得借的兩點：glyph 的 atlas（vello_gpu 的 `full_image_support` 已經有，GPU raster 時拿得到），和矩形類的快速路徑（單色和圓角的矩形、邊框、`box-shadow` 用 SDF 的 shader 直接畫成 Viz 的 quad；`README.md` 的「之後的可能」）。

## 不選的做法

### 經典的 vello（compute）

2026-10-05 讀 vello 0.11.0 的原始碼：
- **上游已經把它當成研究用的 renderer**（見「Vello 系列的現況」）。
- **固定大小的中間 buffer**：大小是為某個測試場景挑的（`vello_encoding` 的 `BufferSizes::new`），每個 renderer 大約 165 MiB 的 GPU buffer，和畫面大小無關。場景太複雜而溢出時，shader 設一個失敗的旗標、後面的步驟不寫，target 保留舊的內容，`render_to_texture` 照樣回傳 `Ok`：**畫錯了也不知道**。動態配置的 issue #366 從 2023 年開到現在。
- **每次 render 的固定成本高**：每次 `render_to_texture` 都在 CPU 上整理整個場景、一個 encoder、一次 submit、大約 14 到 20 個 compute dispatch。一個 frame 一兩百個 tile 就是一兩百次 submit、約三千個 dispatch。
- **輸出是 straight alpha**（`fine.wgsl` 在寫出前除以 alpha），Viz 要另外轉成 premultiplied。
- **shader 關掉了邊界檢查**（0.6 起），對不受控制的網頁內容是安全上的考量。
- Servo 的經驗：比 CPU 版慢很多，clip layer 有 bug（#1061）。
- 好處是 CPU 端的工作最少、文字和向量多的場景在 GPU 上最快；但上面這些對瀏覽器的 tile raster 都是硬傷。

### tiny-skia

純 Rust、和 Skia 的模型一樣，但只有 CPU，速度大約是 vello_cpu 單執行緒的五分之一（「效能」），也沒有 GPU 的路。`docs/architecture.md` 的「Crate 選型」原本就因為「不維護第二個 backend」不用它。

### 完全自己接手 Vello

像 rusty_v8 那樣從某個版本起自己維護、不再跟上游。想改什麼都可以，但 machopo 需要的東西大多還在上游開發中（x86 的 SIMD 優化、glyph 快取、vello_gpu 走向成熟），自己接手就要自己做這些，等於養一個 2D renderer（見「Vello 的 fork」）。

### 保留 Skia，只做 CPU（加 raster worker）

成本最低，raster 結果和 Chromium 最接近。raster worker 已經有了（`--num-raster-threads`）。但 Skia 留著，它帶來的限制也留著：C++ 的依賴、prebuilt 的組合限制了 feature（WebP）、沒有 prebuilt 時要從原始碼編好幾十分鐘；之後要 GPU raster 還是得面對下一節的選擇。

### 保留 Skia，做 GPU raster

這份 RFC 最早（2026-10-05 早先的版本）比較的就是這些，結論是傾向 G4。2026-10-05 決定改用 Vello 之後都不做，留下比較的結果：

| | G1 Skia Ganesh 取代 wgpu | G2 Graphite（Metal、Vulkan）取代 wgpu | G3 Ganesh 畫進 wgpu 的 texture | G4 Graphite 加 Dawn 取代 wgpu |
| --- | --- | --- | --- | --- |
| 和 Chromium 的對應 | `SkiaRenderer` 加 Ganesh（Apple 以外現在的預設） | 沒有（Chrome 只在測試用的參數下用原生 Vulkan） | 沒有 | `SkiaRenderer` 加 Graphite 加 Dawn（Apple 現在的預設） |
| 平台的 swapchain | 自己寫 | 自己寫 | wgpu 處理 | Dawn 處理 |
| Windows 的 API | D3D 或 Vulkan | 只有 Vulkan | wgpu 選 | D3D11 或 D3D12 |
| Skia 的建置 | prebuilt | prebuilt | prebuilt | fork skia-safe，自己編 Skia 和 Dawn |
| 新的 `unsafe` | 平台層和交給 Skia 的 handle | 同 G1 | `as_hal` 和交接的同步 | Dawn 的 C API 和 surface |
| 主要的問題 | raster 要搬到 context 的 thread；平台層自己維護 | 原生 Metal 的 Graphite 只為測試維護；binding 很薄；`Recorder` 沒有 `Send` | 兩套 GPU 層共用 device 和 queue 的同步，錯誤只在某些驅動程式上出現 | 第三個 fork 和自己的建置；Windows、Linux 的 Graphite 加 Dawn 在 Chrome 還沒出貨 |

細節：
- **G2**：skia-safe 0.153.3 的 Graphite binding 全部大約 1150 行，Vulkan 專屬的只有建 context 的 52 行，範例都是離屏的。Skia 的 Greg Daniel 2025-12-24 在 skia-discuss 說原生的 Vulkan backend 會用在 production，原生的 Metal backend 只「為了測試」維持能動。`Recorder` 沒有 `Send`（`Recording` 有），要在其他 thread 錄得自己 `unsafe impl Send`。
- **G3**：wgpu 30 的 `as_hal` 系列可以把 device 交給 Skia，但 Vulkan 的 queue 要外部同步（wgpu 自己的 submit 不會拿 machopo 的鎖，`Surface::configure` 也會 submit；P4.4c 已經碰過類似的問題，`rfcs/pre-rfcs/P4.md` 的「已決定的事項」第 82 條），image layout 兩邊各自追蹤，要手動轉回 wgpu 預期的狀態（wgpu 的內部細節，沒有保證），wgpu 每季一個大版本也會改 hal。
- **G4**：Dawn 是 Chromium 的 WebGPU 實作（C++），在 machopo 的位置和 wgpu 一樣；Skia Graphite 用它的原生 API 加 Dawn 專屬的 feature。Chrome 建 Dawn 的 device 時一定開 `DawnInternalUsages` 和 `ImplicitDeviceSynchronization`，D3D11 時再開 `D3D11MultithreadProtected`（`gpu/command_buffer/service/dawn_context_provider.cc`）。Dawn 自己的支援文件（Dawn 的 repo 裡的 docs/support.md）把 D3D11 寫成實驗性、D3D12 寫成 Windows 的首選，但 Chrome 的 Graphite 在 Windows 預設用 D3D11，Skia 的 Michael Ludwig 2025-04 也說 Dawn 的 D3D12 比較少測。skia-safe 沒有 Dawn（沒有 feature、binding 和 gn 參數），`dawn-sys` 只有 Windows x64、D3D12 的 prebuilt。
- **skia-safe 0.153.3 的 prebuilt**：現在用 `jpegd-jpege-pdf`（沒有 GPU）。有 GPU 的組合依平台不同：macOS 有 Ganesh 加 Metal 或 Vulkan、Graphite 加 Metal；Windows 有 Ganesh 加 D3D 或 Vulkan、Graphite 加 Vulkan；Linux 有 Ganesh 或 Graphite 加 Vulkan。沒有任何組合有 Dawn。

### Chromium 各平台的 GPU 設定

2026-10-05 查 Chromium `main` 的原始碼。這些是程式裡的預設值；Chrome 實際出貨時還可能由伺服器端的 Finch 實驗對部分使用者打開或關掉，某一台機器實際用什麼以 `chrome://gpu` 為準。

| 平台 | raster 和合成 | GPU raster | Graphite | ANGLE（GL、WebGL） | WebGPU |
| --- | --- | --- | --- | --- | --- |
| macOS | Graphite → Dawn → Metal | 開 | 預設開啟 | Metal | 開 |
| Windows | Ganesh → GL → ANGLE → D3D11 | 開 | 預設關閉；開的話是 Dawn → D3D11；ARM64 不支援；Intel GPU 由另一個 feature 控制 | D3D11 | 開 |
| Linux | Ganesh → GL → ANGLE → 桌面版 OpenGL | 程式預設關，交給 Finch（註解說已經在 Linux 推出） | 不允許（只有命令列參數能強制打開） | OpenGL | 預設關閉 |
| Android | Ganesh → Vulkan（裝置不支援時用 GL ES） | 開 | 預設關閉；只在已經用 Vulkan 的裝置上考慮，開的話是 Dawn → Vulkan | OpenGL ES（有沒有經過 ANGLE 沒查） | 開 |
| ChromeOS | Ganesh → GL（Vulkan 預設關閉） | 開 | 預設關閉；只在已經用 Vulkan 的機器上考慮，開的話是 Dawn → Vulkan | OpenGL | 開 |

依據：
- **Graphite**（`gpu/config/gpu_finch_features.cc`）：`kSkiaGraphite` 只在 `IS_APPLE` 預設開啟（2025-07 在 Apple Silicon 的 Mac 上推出）。`IsSkiaGraphiteSupportedByDevice()`：Apple 要用 passthrough command decoder；Android（桌面版的 Android 除外）和 ChromeOS 要已經在用 Vulkan；Windows ARM64 不支援；其他 Windows 支援；其他平台（Linux）就算 feature 打開也拒絕，寫 log「not-yet-supported platform」。另有 `kSkiaGraphiteWinIntel`，預設關閉。
- **Graphite 一律畫在 Dawn 上**（`gpu/config/gpu_util.cc` 的 `GpuModeToGrContextType`）：只有加上 `--skia-graphite-vulkan-backend` 才用原生的 Vulkan，其他時候是 `kGraphiteDawn`。
- **Dawn 的預設 backend**（`dawn_context_provider.cc` 的 `GetDefaultBackendType()`）：Windows 是 D3D11；Linux、ChromeOS、Android 是 Vulkan；Apple 是 Metal。`--skia-graphite-dawn-backend` 可以改成 `d3d11`、`d3d12`、`metal`、`opengles`、`vulkan`、`swiftshader`。
- **Vulkan**（`ui/gl/gl_switches.cc`）：`kVulkan` 只在 Android 預設開啟。ANGLE 的 Vulkan backend（`kDefaultANGLEVulkan`）只在 Skia 本身用 Vulkan 時才可能打開（Chromium 關掉了 ANGLE 的 share group 鎖），預設關閉。
- **ANGLE 的 backend**（`ui/gl/init/gl_display_initializer.cc` 的 `GetEGLInitDisplays`）：有 Metal 就用 Metal；有 D3D 時用 D3D11；都沒有時 Android 用 OpenGL ES，其他平台用 OpenGL。
- **GPU raster**（`kDefaultEnableGpuRasterization` 和 `gpu_util.cc` 的 `GetGpuRasterizationFeatureStatus`）：Apple、Windows、ChromeOS、Android 預設開啟；Linux 不在清單裡，由 Finch 控制；用 Vulkan 時一律開啟。
- **WebGPU**（`kWebGPUService`）：Apple、Windows、ChromeOS、Android 預設開啟；Linux 只在 `USE_WEBGPU_ON_VULKAN_VIA_GL_INTEROP` 的建置開啟。各平台的 WebGPU 用 Dawn 的哪個 backend 沒有查。
- **DrDC**（`kEnableDrDc`）：只在 Android 預設開啟。

## 和其他實作比較

| | Chromium | Servo | Blitz | machopo 現在 | machopo（這份 RFC） |
| --- | --- | --- | --- | --- | --- |
| 網頁的 raster | Skia，tile；Apple 是 Graphite 加 Dawn，其他是 Ganesh | WebRender（整個場景在 GPU 上畫，沒有 tile 的 raster） | Vello 系列（經過 AnyRender） | Skia CPU，tile | vello_cpu，tile |
| canvas 2D | Skia | vello_cpu（經典 vello 可選） | — | Skia CPU，canvas raster thread | vello_cpu，canvas raster thread |
| raster 在哪裡 | GPU process（OOP-R） | — | — | renderer 的 compositor thread 或 raster worker | 同左 |
| 合成 | `SkiaRenderer` | WebRender | — | `WgpuRenderer` | 同左 |
| 單色的 tile | raster 前用 `SolidColorAnalyzer` 分析 paint op | — | — | raster 完看像素 | 同左（raster 前的分析在 GPU raster 時才需要） |
| 縮小的圖片 | Skia 的 mipmap；software raster 時 cc 的 `SoftwareImageDecodeCache` 先縮好 | — | — | Skia 的 mipmap | machopo 的圖片快取先縮好 |
| 圖片的解碼 | Blink 的 `ImageDecoder`（PNG 是 Rust 的 `png`） | image-rs | image-rs | Skia | image-rs 系列 |

Servo 和 Blitz 的欄位依研究時讀到的資料寫，沒有逐項查證原始碼。

## 參考資料

- linebender/vello：README、ARCHITECTURE.md、issue #1871、PR #1602、#1872、#1883、#1895；vello_tests 的測試和容許誤差；經典 vello 的 issue #366、#476、#1061、#1063、#1545、#1694。2026-10-31：`vello_cpu/README.md`、`vello_cpu/src/render.rs`、`vello_cpu/src/filter/mod.rs`、`vello_common/src/filter/mod.rs`、`vello_gpu/README.md`、`vello_gpu/src/filter.rs`，各目錄的 commit 歷史。
- linebender/vello，2026-11-10：`main` 的 commit `112736d`（`vello_common/src/filter/mod.rs`、`vello_common/src/filter_effects.rs`、`vello_cpu/src/render.rs`、`vello_gpu/src/scene.rs`、`glifo/src/glyph.rs`、兩份 README）；PR #1623、#1765（`ColorMatrix`）、#1696；issue #856（dithering）、#1245（橢圓的角）、#1289（縮小的品質）、#1296（剩下的 filter）、#1491（Blitz 需要的功能）、#1694（文字）。
- zed-industries/zed，2026-11-10：`main` 的 commit `f0d896f`，`gpui`、`gpui_wgpu`、`gpui_windows` 這幾個 crate。
- crates.io：vello_cpu 0.3.0、vello_gpu 0.3.0、vello_common 0.3.0、glifo 0.4.0、vello 0.11.0、vello_hybrid 0.2.0、vello_api 0.0.7 的原始碼；vello_cpu 的版本歷史。
- docs.rs：vello_common 0.3.0 的 `FilterPrimitive`；peniko 0.6.1 的 `Gradient`、`InterpolationAlphaSpace`、`ImageSampler`、`ImageQuality`。
- Linebender 的 2026 Q1 回顧（https://linebender.org/blog/tmil-25/）。
- vello_cpu 的 benchmark：https://laurenzv.github.io/vello_chart/。
- Servo：PR servo#36821（2025-07，經典 vello 的 canvas backend）、servo#41820（2026-01，預設改成 vello_cpu）；`components/canvas`。
