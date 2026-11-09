# RFC 0007：用 vello_cpu 取代 Skia

- 開始日期：2026-10-05
- 狀態：📝 草稿：2026-10-31 重新打開（已決定的事項第 3 條：這份 RFC 把 Skia 整個換成 vello_cpu；GPU raster 照第 1 條的方向另寫一份）。「詳細設計」和「分階段」是 2026-10-31 的看法，還沒確認
- 細節：`details.md`

## 摘要

拿掉 Skia（skia-safe），machopo 用到 Skia 的每個地方都換成純 Rust 的 crate：tile 和 canvas 的 raster 用 Linebender 的 **vello_cpu**，圖片的解碼和讀尺寸用 image-rs 系列的 crate，JPEG 的編碼用 `jpeg-encoder`，glyph 的 ink bounds 用 skrifa。`raster` crate 對外的 API 不變，compositor、renderer 和 Viz 都不用改。Vello 沒有、machopo 的 display list 又用到的功能（不支援的 filter、path 的 union、任意形狀的模糊、圖案 shader、mipmap），先在 machopo 的 playback 裡用 vello_cpu 現有的功能組出來，量過太慢或畫不好的再改 Vello 的 fork。raster 的結果不再以和 Chromium 逐像素一致為目標。GPU raster（vello_gpu，不能用時退回 vello_cpu）是之後的方向（已決定的事項第 1 條），這份 RFC 只讓 playback 不擋住它。

## 動機

**現況**（細節見 `details.md` 的「現在的 raster」）：

- Skia 只在 `raster` crate 裡（`crates/raster/src/skia_backend.rs` 約 990 行程式加上測試，`crates/raster/src/encode.rs` 的 JPEG），一律是 CPU 的 backend。用到它的地方：
  - **tile 的 raster**：compositor thread 上（`--num-raster-threads` 時在 raster worker 上）呼叫 `raster::playback_into`，畫成 premultiplied 的 RGBA8，再上傳到 Viz 的 wgpu device（`crates/compositor/src/layer_tree_host_impl.rs`）；
  - **canvas 2D**：canvas raster thread 用 `raster::draw_into` 把錄製畫進 canvas 的 bitmap（`crates/renderer/src/canvas_raster.rs`、`crates/renderer/src/canvas.rs`）；
  - **圖片**：`raster::image_size` 讀尺寸（`crates/renderer/src/render_thread/documents.rs`），解碼在 raster 時由 Skia 做；
  - **編碼**：`raster::encode_jpeg`（`toDataURL`、`toBlob`；PNG 已經用 `png` crate）；
  - **文字**：`raster::ink_bounds`（`crates/renderer/src/frame_view.rs`、`crates/layout/src/canvas_text.rs`），和照字型、軸的值建的 typeface。
- display list（`crates/compositor_paint/src/lib.rs` 的 `PaintOp`）有 24 種 op，包括 image filter（drop shadow、blur、color matrix、串接）、28 種 blend mode、三種漸層、圖案 shader、dash、mask blur、`DrawShadow`、`DrawRecord`、`WritePixels`。2026-10-05 這份 RFC 第一次寫的時候只有 13 種，那時的 PaintOp 對照和「machopo 碰不到 Vello 會 panic 的功能」都已經不成立。

**問題**：

- **Skia 是 C++ 的依賴，綁著 prebuilt**：只能用 rust-skia 有發布的 feature 組合，多開一個 feature 就要從原始碼編好幾十分鐘；WebP 就是因此沒做（`rfcs/pre-rfcs/P2.md` 的「已決定的事項」第 7 條）。沒有 prebuilt 的平台第一次 build 很久（`docs/architecture.md` 的「要注意的地方」、根目錄 `README.md`）。
- **之後的 GPU raster 用 Skia 的每條路都很貴**：要嘛拿掉 wgpu、自己寫各平台的 swapchain，要嘛 fork skia-safe、自己編 Skia 和 Dawn，要嘛讓 Skia 和 wgpu 共用一個 device、自己處理同步（`details.md` 的「保留 Skia，做 GPU raster」）。Vello 的 CPU 和 GPU 兩個 renderer 共用同一份算覆蓋率的程式，vello_gpu 直接畫在 machopo 已經用的 wgpu 上。
- **Vello 系列是純 Rust，和 machopo 已經用的 parley、skrifa 同一個生態系**：沒有自己的建置、不新增 `unsafe`。在 Linebender 的 benchmark 裡，vello_cpu 單執行緒大約是 Skia CPU 的 2.5 倍（`details.md` 的「效能」；網頁的 tile 文字多，要在 machopo 裡實測）。

要解決的 `docs/limitations.md` 裡的限制：

- 「Viz、GPU 和記憶體」：raster 為可變字型建的 Skia typeface 跟著 compositor、不會變小。Vello 直接吃字型的資料和軸的值，不建 typeface。

做完時跟著改寫、但不是解決的：「Paint 和 raster」的合成粗體（改成 Vello 的 embolden，粗細不同）。

**目標**：

- workspace 裡沒有 skia-safe；`raster` 的 tile 和 canvas 的 raster 都用 vello_cpu。
- `raster` 對外的 API 不變（`playback`、`ops_for`、`playback_ops`、`playback_into`、`draw_into`、`image_size`、`ink_bounds`、`encode_png`、`encode_jpeg`、`RasterCache`、`Bitmap`、`BACKEND`），所以 compositor、renderer、Viz 和並行的 RFC 0089 都不用跟著改（見「和 RFC 0089 並行」）。
- 現有的 reftest、WPT 的 reftest、情境和端到端測試照樣通過。因為反鋸齒、文字的粗細、JPEG 的解碼而改了預期結果或加了 fuzzy 的，每一個都寫明原因。
- **playback 不會 panic**：Vello 不支援的功能都在 playback 裡改寫或繞過，不交給 Vello；有測試守著每種 op、filter 和 blend mode 的組合。
- 圖片支援的格式不少於現在（PNG、JPEG、GIF 的第一格）。
- 不新增 `unsafe`。

**不在範圍內**：

- **GPU raster**（vello_gpu、退回 vello_cpu、raster 的介面、raster 前的單色分析）：方向照已決定的事項第 1 條，另寫一份 RFC，等這份做完、量過 vello_cpu 再開（見「之後的可能」）。
- WebP 等新的圖片格式：拿掉 Skia 之後只要加 `image-webp`，但另外做。
- 新的 CSS 效果（`mask-image`、`backdrop-filter`、SVG 的 filter primitive）：等那些功能時再看 Vello 有沒有、要不要自己做（見「缺點」）。
- 色彩管理（ICC）和 EXIF 的轉向：今天的 Skia 路徑也沒有（畫進沒有色彩空間的 surface、不讀 orientation），換掉不會變差。
- OOP-R 的跨 process 部分：屬於 RFC 0004 或之後。

## 設計概要

給 machopo 的使用者：畫面幾乎一樣。文字可能細一點、淡一點（Vello 畫文字不像 Skia 那樣調整 gamma 和對比），合成粗體的粗細略有不同，漸層沒有 dithering。捲動、改視窗大小時的 raster 應該快一些（要量）。

給之後改 machopo 的人：

- `raster` crate 對外的樣子不變。裡面的 `skia_backend.rs` 換成一個 Vello 的 playback：把 `PaintOp` 一個一個畫進 vello_cpu 的 `RenderContext`。Vello 沒有的功能在 playback 裡組出來（見「詳細設計」），呼叫的人不需要知道。
- compositor、canvas raster thread、Viz 不變：tile 照樣是 CPU 的像素，照樣上傳；單色的 tile 照樣 raster 完看像素。
- 圖片的解碼、JPEG 的編碼、glyph 的 ink bounds 換成各自的純 Rust crate，函式的簽名不變。

## 詳細設計

這一節是 2026-10-31 的看法；各個選擇的比較見「理由和其他做法」和 `details.md`。

### playback（`crates/raster/src/`，取代 `skia_backend.rs`）

- **每條 raster 的 thread 一個 `RenderContext`**，tile 和 tile 之間 `reset` 重用（今天每條 thread 重用 pixel buffer 的做法）。vello_cpu 的多執行緒不開：machopo 已經是一條 thread 畫一個 tile（`--num-raster-threads`），而且 vello_cpu 開多執行緒時連單一的 filter 都會 panic。
- **起始的 transform**：位移到 tile 的原點，再乘上 `contents_scale`；`SetMatrix` 相對於它（和今天一樣）。
- **`Save` / `Restore`**：Vello 的 layer 不保存 transform，playback 自己維護堆疊：目前的 `Affine`，以及這一層推了幾個 clip 或 layer，`Restore` 時一一 pop。
- **`clear_color`** 用 render 時的 `TargetInit::Clear`；canvas 的 `draw_into` 畫在既有的像素上，用 `TargetInit::SrcOver`。
- **直接對應的 op**（`details.md` 的「PaintOp 怎麼對應」）：transform、`ClipRect`、`ClipRRect`、`ClipPath`（不隔離的 `push_clip_path`）、`SaveLayerAlpha`、只有 alpha 和 blend mode 的 `SaveLayer`、`DrawColor`、`DrawRect`、`DrawRRect`、`DrawDRRect`、`DrawLine`、`DrawPath`、`ClearRect`、`DrawClipped`、dash（kurbo 的 stroke）、cap 和 join、28 種 blend mode（peniko 的 Mix 加 Compose）、三種漸層（linear、two-point radial、sweep，Pad、Repeat、Reflect，premultiplied 和 unpremultiplied 的插值）、只有一個 Blur 或 DropShadow 的 filter、不抗鋸齒（`set_aliasing_threshold`）。
- **Vello 沒有、在 playback 裡組出來的**（`details.md` 的「PaintOp 怎麼對應」逐項寫了做法）：
  - **不支援的 filter**（`ColorMatrix`、`Chain`，vello_common 碰到會 `unimplemented!`）：layer 的內容畫進另一個 `RenderContext` 的 pixmap，machopo 自己套 color matrix（照 Skia：先 unpremultiply），Blur 和 DropShadow 照樣交給 Vello，串接的照順序一個一個做，最後當成圖片、帶著 layer 的 alpha 和 blend mode 畫回去。量過太慢時改在 Vello 的 fork 裡做（見「Vello 的 fork」）。
  - **filter 在 tile 邊緣**：`RenderContext` 只畫自己的範圍，tile 外面的內容不會被模糊進來，相鄰的 tile 之間會有接縫。有 filter 的 layer 用比 tile 大、四周加上 filter 的範圍（`filter_expansion`）的 context 畫，render 時用負的位移裁回 tile。原型時先確認接縫真的存在。
  - **path 的 union**（`ClipPaths`）：Vello 沒有 path 的布林運算。把每條 path 照它的 fill rule 畫進一張和 tile 一樣大的 alpha mask，`push_mask_layer`。反鋸齒的邊和 Skia 的 `PathOps` 有一點差異。
  - **任意形狀的模糊**（`PaintFlags.blur`、`DrawShadow`）：圓角矩形用 `fill_blurred_rounded_rect`（`invert` 給 inset 的陰影），其他形狀包一層 GaussianBlur 的 filter layer。
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
- **ink bounds**（`raster::ink_bounds`）改用 skrifa：有輪廓的 glyph 把輪廓畫進 kurbo 的 `BezPath` 取範圍（和 Vello 實際畫出來的一致），點陣的讀 bitmap 的 metrics，再照 embolden、skew 和 stroke 擴大。函式的簽名照舊（`RasterCache` 這個參數留著不用），搬出 `raster` 等 RFC 0089 做完再說（見「和 RFC 0089 並行」）。

### 圖片

- **解碼**：PNG 用 `png`（machopo 的編碼已經用它，Chromium 的 Skia 也已經改用它），JPEG 用 `zune-jpeg`，GIF 用 `gif`（只有第一格，和今天一樣）。解碼的結果轉成 premultiplied 的 `Pixmap`，照現在的方式依 `ImageData::id` 快取在 process 全域的表裡；縮小的版本跟著同一個項目。
- **讀尺寸**：各 crate 只讀檔頭，不解碼。
- **JPEG 的解碼和 Skia（libjpeg-turbo）不逐像素相同**（IDCT 和色度 upsampling 的做法）：比對 JPEG 和 PNG 的 reftest 可能要 fuzzy，每個寫明原因。
- **JPEG 的編碼**：`jpeg-encoder`（純 Rust）。quality 照 Blink 的設定對到量化表；輸出的 bytes 和今天不同，只要解得回來、品質相當。

### Vello 的 fork

- **先不 fork**（已決定的事項第 4 條），第 0 步的原型或之後的量測看出非改 Vello 不可的地方再 fork。fork 照 parley 的模式：`../third_party` 底下的 vello（`machopo` 分支），釘在上游的發布版、只放非改不可的小 patch，每次上游發布時 rebase；`README.machopo.md`、`third_party.toml` 照現有的 fork 的規則。不完全自己接手（像 rusty_v8 那樣）：machopo 還需要上游的 x86 SIMD、glyph 快取和 vello_gpu。
- 可能進 fork 的：原生的 `ColorMatrix` 和多個 primitive 的 filter graph（離屏的做法太慢時），filter 的 tile 邊緣（如果加大 context 不夠）。上游這幾個檔案很少改（`details.md` 的「Vello 的 fork」），vello_common 的 enum 已經列了這些 primitive，上游做了之後 machopo 的 patch 就丟掉。
- 文字的 gamma 和對比、漸層的 dithering：改在上色的核心，上游不一定接受（會改變所有使用者的結果），第 0 步看過截圖再決定要不要對齊 Skia。
- 要送上游的 patch，照 `CLAUDE.md`，先在本地寫好草稿給人審。

### 和 RFC 0089 並行

RFC 0089（換成新的 DOM）同時在做，兩份並行（已決定的事項第 6 條；0089 的「不在範圍內」跟著改成只限 DOM 的功能，見它的已決定的事項第 104 條）：

- **碰到的程式碼幾乎不重疊**：這份在 `raster`；0089 在 `dom*_next`、`gc`、`bindings`、`style_next`、`layout` 的 `next` 模組、renderer 的 `render_thread/next*.rs`。0089 的 4g（圖片、canvas 搬到新的 DOM）會碰 renderer 裡呼叫 `raster` 的那一側，所以 `raster` 的 API 在這份 RFC 裡不變。
- **共用的 `../third_party`**：worktree 共用它，`ci.py third-party` 會把 checkout 切到目前這份 checkout 釘的 commit。0089 換了 rusty_v8 或 parley 的 pin，`0007` 的 branch 就 rebase 到最新的 `main`，不在舊的 pin 上跑 `test.py`。
- **預期結果**：0089 以「今天通過的都還通過」驗收，這份會刻意改變像素。raster 和 DOM 無關，新舊 DOM 同時用同一個 raster，0089 的比對照樣成立；但兩份都會改 reftest 和 WPT 的預期結果。所以換掉 playback 的那一步（第 2 步）放在 0089 兩個階段的交界，預期結果在同一個 commit 裡一次更新，0089 再 rebase 上去。
- 第 0 步的原型不 merge，對 0089 沒有影響。

### 測試

- **playback 的單元測試**：`raster` 現在的測試照搬（每種 `PaintOp`、`Save` / `Restore` 和 clip、layer 的配對、tile 的位移和 scale、`SetMatrix` 相對於 tile、canvas 的 `draw_into`）。像素的預期值因為反鋸齒不同而改的，寫明原因。
- **不會 panic**：一個測試列舉每種 op、每種 `ImageFilter`（包括巢狀的 `Chain`）、每種 blend mode、`ClipPaths`、圖案 shader 的組合畫一次。vello_common 的 `unimplemented!` 一碰到就會讓它失敗。
- **組出來的功能各自的測試**：color matrix 和 Skia 的公式對照（固定的輸入和輸出）；串接的順序；path union 和逐條 clip 的差別；mask blur 的範圍；圖案 shader 的 spacing 和 decal；mipmap 選到哪一張（計數）。
- **tile 的接縫**：同一個 display list 用一個大的 tile 畫一次，再切成 256 的 tile 各畫一次拼起來，兩者逐像素相同；特別放模糊、drop shadow 和陰影跨過 tile 邊界的內容。
- **圖片和編碼**：每種格式的解碼和讀尺寸、壞掉的檔案回傳 `None`；JPEG 編碼後再解碼，和原圖的差在容許範圍內；ink bounds 和輪廓的範圍一致，embolden、skew、旋轉、點陣 glyph。
- **reftest、WPT 的 reftest、情境**：照 `CLAUDE.md` 跑改到的部分，第 2 步的那個 commit 跑 `test.py reftest`、相關目錄的 `test.py wpt`、`test.py scenarios`。每個改了預期結果或加了 fuzzy 的測試，在測試或 `tests/reftests/README.md` 寫明原因。
- **mutation test**：照 `docs/testing.md` 的「Mutation test」，結果寫進 `progress.md`。
- **只能人看的**：文字和圖片的外觀（變細、縮小的品質），在 `about:demo` 上和改之前的截圖比，記進 `progress.md` 的「做過的檢查」；`docs/test-gaps.md` 的「測試守不住的地方」記下文字的外觀只靠人看。
- **量測**：`bench.py raster`（捲動、改視窗大小時 compositor 的 PrepareTiles）、`bench.py blink-perf paint/ image_decoder/`，Skia 和 vello_cpu 各一次（Windows，有 Mac 時加 macOS）。Speedometer 不排，使用者要求時才跑。

### 分階段

2026-10-31 的看法，排程時再確認：

0. **原型和量測**（不 merge，在 `0007` 的 worktree 裡做）：
   - vello_cpu 的 playback 接上全部 24 種 op，照上面的做法組出 Vello 沒有的功能；跑 `raster` 的測試、reftest、WPT 的 reftest 和情境，列出哪些不通過、差在哪裡。
   - 確認 filter 在 tile 邊緣的接縫、unpremultiplied 的漸層插值、不隔離的破壞性 blend mode 在 vello_cpu 上的結果。
   - 量 raster 的時間（`bench.py raster`、`bench.py blink-perf paint/`），特別是文字多的 tile（glyph 快取還是實驗性的）和離屏的 color matrix。
   - 結論寫進 `details.md`：CPU 的差異能不能接受、要不要 fork。差異大到不能接受的話，這份 RFC 回到草稿重新評估。
1. **不是 raster 的部分先換**（Skia 照舊畫）：圖片的解碼和讀尺寸、JPEG 的編碼、ink bounds。解碼的結果當成 raster 的圖片交給 Skia 畫，像素的差異只來自 JPEG 的解碼。可以先 merge。
2. **playback 換成 vello_cpu、拿掉 skia-safe**：tile 和 canvas 一起換，連同 mipmap 和所有組出來的功能（少了任何一項就會 panic 或退步，所以不再拆）；預期結果在同一個 commit 裡更新；放在 RFC 0089 兩個階段的交界 merge（已決定的事項第 5 條）。`docs/architecture.md` 的「Crate 選型」和 pipeline 的表、根目錄 `README.md` 的 skia-safe 說明、`Cargo.toml` 裡寫到 Skia 的註解跟著改。
3. **改進**（看第 0、2 步的結果，可能不需要）：Vello 的 fork 裡的原生 `ColorMatrix` 和 filter graph、文字的 gamma、漸層的 dithering；不需要的寫明理由、不做。

第 2 步做完，Skia 就完全拿掉了。

## 缺點

- **raster 的結果和 Chromium 不同**：`docs/architecture.md` 的「Crate 選型」原本只用 Skia，理由就是 raster 結果和 Chromium 一致。換成 Vello 之後：文字沒有 gamma 和對比的調整，看起來比較細、淡；合成粗體的粗細不同；path 的 union 和 mask blur 的反鋸齒邊不同；漸層沒有 dithering；JPEG 的解碼有小的差異。拿 Chromium 的截圖或 WPT 上別的瀏覽器的結果來比時，差異會變多。
- **Vello 還不穩定**：每一版都是 breaking（2026-05 到 2026-10 出了五版），每次升級都要改 playback；MSRV 可能在 patch 版本裡提高。fork 之後每次升級還要 rebase。
- **machopo 要自己維護組出來的功能**：離屏的 filter、mask 的 union、圖案 shader 的 pixmap、mipmap 的快取。這些在 Skia 裡都是現成的，而且離屏比 Skia 的 filter 慢、多用記憶體。
- **少了 Skia 現成的、之後的 CSS 會用到的功能**：多步的 filter graph、SVG 的 filter primitive（vello_common 的 enum 列了，但只實作了 Flood、GaussianBlur、Offset、DropShadow）、`mask-image`、`backdrop-filter`、LCD 的文字。到時候要等上游或自己做。
- **glyph 的快取還是實驗性的**：文字多的 tile 每次都要重新攤平輪廓，可能比 Skia 慢，要在第 0 步量。
- **和 RFC 0089 並行**：兩份都會改預期結果，`0007` 的 branch 要跟著 0089 rebase，人要審兩條線。

## 理由和其他做法

完整的比較見 `details.md` 的「不選的做法」。

- **A. 保留 Skia，只做 CPU raster**：成本最低，結果最接近 Chromium；但 C++ 的依賴和 prebuilt 的限制都留著，之後要 GPU raster 還是得面對 B。
- **B. 保留 Skia，做 GPU raster**（Ganesh、原生的 Graphite、Skia 畫進 wgpu、Graphite 加 Dawn）：和 Chromium 一致、功能最齊，但每條路都要拿掉 wgpu 自己寫平台層、fork 和自己編 Skia 加 Dawn，或兩套 GPU 層互相同步。
- **C. 先用 vello_cpu 取代 Skia，之後再加 vello_gpu**（選這個，第 1、3 條）：純 Rust、wgpu 留著、沒有新的 `unsafe`、沒有自己的建置；CPU 和 GPU 共用同一份算覆蓋率的程式，之後加 GPU raster 時兩條路的結果幾乎一樣。代價是「缺點」那些。
- **D. 經典的 vello（compute）**：上游已經把它當成研究用的；中間 buffer 是固定大小，場景太複雜時畫錯了也回傳成功；每次 render 都要一次 submit 和十幾個 dispatch。**不選。**
- **E. tiny-skia**：只有 CPU，速度約是 vello_cpu 的五分之一，沒有 GPU 的路。**不選。**

Vello 的 fork（「詳細設計」的「Vello 的 fork」）：
- **甲. 不 fork，只在 machopo 的 playback 裡補**：沒有同步的成本，但原生的 `ColorMatrix`、文字的 gamma、dithering 做不到。
- **乙. 小 patch 的 fork，跟發布版 rebase**（選這個，需要時才 fork，第 4 條）：拿得到上游的改進，fork 只是幾個獨立的 patch。
- **丙. 完全自己接手**：失去上游最有價值的部分（x86 的 SIMD、glyph 快取、vello_gpu 走向成熟），等於 machopo 自己養一個 2D renderer。**不選。**

換掉 playback 的那一步怎麼進 `main`（「分階段」第 2 步）：
- **甲. 直接替換，放在 RFC 0089 兩個階段的交界**（選這個，第 5 條）：一次更新預期結果，之後只有一套 raster。
- **乙. 過渡期間用旗標切換 Skia 和 Vello**（Chromium 推 Graphite 時用旗標逐步開啟）：時間點和 0089 完全解耦，也能直接比兩邊的像素；但有一段時間要維護兩個 backend、兩個都要編，`docs/architecture.md` 寫的是不維護第二個 backend。**不選。**

`docs/architecture.md` 原本不用 Vello 的理由是「整個場景一次 render，沒有 cc 那種以 tile 為單位的 raster 概念」：那是針對經典的 vello，而且是假設由 Vello 取代 cc 和 Viz。這份 RFC 保留 cc 的 tile，每個 tile 是一個小場景，vello_cpu 適合這種用法。

不做會怎樣：Skia 留著，WebP 等功能繼續受 prebuilt 限制；要 GPU raster 時只剩 B 那幾條昂貴的路。

## 先例

- **Chromium**：Skia，tile 在 GPU process 裡 raster（OOP-R）；圖片的解碼在 CPU 上（`ImageDecoder`；PNG 的解碼和編碼已經換成 Rust 的 `png` crate），縮小的圖片由 cc 的 `SoftwareImageDecodeCache` 先縮好再交給 Skia。machopo 照 cc 的分層和預先縮小的圖片，raster 的函式庫不同。
- **Servo**：網頁用 WebRender 畫，沒有 tile 的 raster；canvas 2D 預設用 vello_cpu。它試過經典的 vello，GPU 版每個 frame 都要讀回再上傳，比 CPU 版慢，2026-01 改回 vello_cpu 當預設。
- **Blitz**：用 Vello 系列畫整個網頁，是目前最接近 machopo 這種用法的。
- **Firefox**：WebRender 畫整個場景，canvas 和部分內容用 Skia（`docs/architecture.md` 已經否決 WebRender）。

逐項比較見 `details.md` 的「和其他實作比較」。

## 已決定的事項

1. **2026-10-05：用 Vello 系列取代 Skia，CPU 和 GPU raster 都是**。raster 的結果不再以和 Chromium 一致為目標，`docs/architecture.md` 的「Crate 選型」裡「只用 Skia」的原則改掉。不做保留 Skia 的 GPU raster（Ganesh、原生的 Graphite、Skia 畫進 wgpu、Graphite 加 Dawn），理由見「理由和其他做法」的 B：每條路都要拿掉 wgpu、自己編 Skia 和 Dawn，或兩套 GPU 層互相同步，而 Vello 是純 Rust、直接畫在 machopo 已經用的 wgpu 上。
2. **2026-10-06：延後**。當時不打算考慮，raster 照舊用 Skia 的 CPU backend。2026-10-31 由第 3 條重新打開。
3. **2026-10-31：重新打開，這份 RFC 把 Skia 換成 vello_cpu**（使用者）。範圍是 machopo 用到 Skia 的每個地方：tile 和 canvas 的 raster、圖片的解碼和讀尺寸、JPEG 的編碼、glyph 的 ink bounds。GPU raster 照第 1 條的方向（vello_gpu 為主，沒有 GPU 時退回 vello_cpu），另寫一份 RFC，等這份做完、量過 vello_cpu 再開；這份 RFC 原本的 raster 介面、單色分析和 GPU raster 的設計移到那裡（見「之後的可能」）。
4. **2026-10-31：Vello 先不 fork，需要時做小 patch 的 fork**（使用者照建議）。Vello 沒有的功能先在 machopo 的 playback 裡用 vello_cpu 現有的功能組出來；第 0 步的原型或之後的量測看出非改 Vello 不可（例如離屏的 `ColorMatrix` 太慢）時，照 parley 的模式 fork：`machopo` 分支、釘在上游的發布版、只放小的 patch、每次上游發布時 rebase，通用的 patch 草稿給人審過再送上游。不選完全自己接手（「理由和其他做法」的丙）：machopo 還需要上游的 x86 SIMD、glyph 快取和 vello_gpu。
5. **2026-10-31：換掉 playback 的那一步直接替換，放在 RFC 0089 兩個階段的交界 merge**（使用者照建議，「理由和其他做法」的甲）。預期結果在同一個 commit 裡一次更新，0089 再 rebase 上去。不選過渡期間用旗標切換 Skia 和 Vello（乙）：一段時間要維護、編兩個 backend。
6. **2026-10-31：和 RFC 0089 並行**（使用者照建議）。0089 的「不在範圍內」原本寫著 `main` 只做那一條線，改成只限 DOM 的功能（0089 的已決定的事項第 104 條）。做法見「和 RFC 0089 並行」：`raster` 的 API 不變；0089 換了 `third_party.toml` 的 pin 時 `0007` 的 branch 跟著 rebase；第 0 步的原型不 merge。

## 待決定

- **CPU 的差異能不能接受**：第 0 步跑完 reftest、看過截圖再定；特別是文字的粗細。要不要在 fork 裡對齊 Skia 的文字 gamma 和漸層的 dithering。
- **不支援的 filter 一直用離屏，還是改在 fork 裡做**：第 0 步量離屏的 color matrix 的成本（第 4 條：慢的話才 fork）。
- **mipmap 的品質和記憶體**：預先縮小用 box filter 還是更好的 filter（對照 Chromium 的 `SoftwareImageDecodeCache` 怎麼縮，原型時查），縮小的版本佔多少記憶體、什麼時候清（`docs/limitations.md` 的「圖片」記著 raster 的圖片快取沒有大小上限）。
- **圖片解碼的 crate**：PNG 用 `png` 還是 `zune-png`；要不要直接用 `image` 一整包。第 1 步決定。
- **Vello 的版本怎麼跟**：每次發布都升，還是釘住、需要時再升；升級時要重新對 reftest。

## 之後的可能

- **GPU raster**（另一份 RFC，方向照已決定的事項第 1 條）：vello_gpu 畫進 Viz 的 wgpu device 上的 texture，GPU 不能用時退回 vello_cpu。2026-10-31 討論過的要點：
  - 退回 vello_cpu 的條件：adapter 是 CPU 的（WARP、lavapipe，`crates/viz/src/gpu_context.rs` 分得出來）、headless 和 reftest、device 遺失到重建好之前、使用者用旗標關掉、拆 process 之後 renderer 碰不到 GPU 時。CPU 這條路是正式的路徑，不只是備援。
  - vello_gpu 碰到不支援的功能會 panic（mask layer、不隔離的破壞性 blend mode、filter graph）：在 raster 前改寫或判斷那個 tile 改走 CPU，不靠 panic 之後退回；`catch_unwind` 只當最後一道防線，接到之後整個 compositor 改用 CPU raster。這份 RFC 用 mask 做的 path union 在 GPU 上要換別的做法。
  - raster 的介面（Chromium 的 `RasterInterface` 加上 `SharedImageInterface`）、raster 前的單色分析（`SolidColorAnalyzer`）、CPU 和 GPU 的比對測試（CI 沒有 GPU，用 lavapipe 或 WARP）。
  - canvas 先留在 CPU（`getImageData` 要讀回）。
  - 先量：vello_gpu 的覆蓋率還是在 CPU 上算，GPU 只省下填色、合成和上傳。
  - vello_gpu 的細節見 `details.md` 的「vello_gpu 0.3」和「CPU 和 GPU 的結果有多接近」。
- **WebP**：加 `image-webp` 就有（`details.md` 的「圖片」）。
- **ink bounds 搬出 `raster`**：改用 skrifa 之後只需要字型資料，可以搬到 `layout` 或 `gfx`，renderer 量文字不再經過 `raster`。等 RFC 0089 做完。
- **CSS 的模糊和陰影以外的效果**：`mask-image`、`backdrop-filter`、SVG 的 filter primitive，看 Vello 那時有什麼。
- **OOP-R**（RFC 0004 的 T3）：display list 和字型、圖片要能序列化。
