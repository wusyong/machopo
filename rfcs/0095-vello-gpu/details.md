# RFC 0095 的細節：tile 的 GPU raster（vello_gpu）

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

下面兩節都是 2026-11-11 讀程式得到的，沒有編、沒有跑：vello_gpu 讀的是 fork 的 `b52311f2`（上游 `main` 的 `112736df`），machopo 讀的是 `70ef614`。原型（「分階段」第 1 步）要先確認標了「推論」的每一項。

## 現在的 tile 怎麼畫、怎麼到 Viz

- **誰決定**：`LayerTreeHostImpl::prepare_tiles`（cc 的 `TileManager::PrepareTiles`）。只有 interest rect（可見範圍每邊加上 viewport 的長邊）裡的 tile 存在；沒有 `draw_info` 的就畫。commit 時 display list 是同一個 `Arc` 就全部留著，否則只留 invalidation 沒碰到的。
- **同步**：commit 之後、畫 frame 之前把缺的 tile 全部畫完，沒有 checkerboard、沒有優先順序、沒有每個 frame 的上限。
- **哪條 thread**：預設在 compositor thread 上一個一個畫；`--num-raster-threads` 大於 1、而且不只一個 tile 時用 rayon 的 pool（`CompositorTileWorker`）。瀏覽器介面的 compositor 是 UI thread 上的單一 thread 版本，走同一個介面。
- **畫進哪裡**：每條 thread 重用的一塊 buffer（`TILE_PIXELS`），`raster::playback_into` 讓 vello_cpu 直接 render 進去，premultiplied 的 RGBA8。
- **怎麼到 Viz**：畫的那條 thread 自己呼叫 `SharedImageInterface::create_shared_image`（`viz::GpuSharedImages`）：拿 `submit_lock`、`create_texture`、`queue.write_texture`、再 `queue.submit` 一次空的（一個 tile 一次，讓 staging 的複製不堆積）。回來的是不帶像素、帶著 `SharedImage` 的 `TransferableResource`。沒有介面、device 還沒好或遺失時，像素複製成 `Arc` 跟著 frame 送，Viz 在自己的 thread 上傳。
- **texture**：`Rgba8Unorm`，usage 是 `TEXTURE_BINDING | COPY_DST`，一層；內容是 premultiplied、sRGB 編碼。tile 用 nearest 的 sampler。沒有 `ResourcePool`：每個重畫的 tile 都是新的 texture，最後一個 `Arc` 放掉時釋放。
- **單色的 tile** 有兩處：畫之前 `DisplayItemList::color_if_solid_in_rect`（只認縮放和位移的 transform、矩形的 clip；release 直接不畫，debug 照畫再比對）；畫之後掃像素（全不透明或全透明才算）。
- **wgpu**：30.0.1，只有 `viz` 和 `shell` 依賴。device 和 queue 在 `viz` 的 `GpuContext`（整個 process 一份），Viz thread、compositor thread、raster worker 和 UI thread 都會用到，`submit_lock` 把 submit 和 `Surface::configure` 排開。adapter 照 Discrete、Integrated、Other、Virtual、Cpu 的順序挑；`device_type == Cpu` 認得出 WARP 和 lavapipe；`MACHOPO_GPU_ADAPTER=cpu` 強制用 CPU 的。device 用 `downlevel_defaults` 的 limits、不要任何 feature。
- **每種模式都有真的 device**：headless、windowless（情境、`--automation-pipe`）、reftest 和 WPT 都用 offscreen 的 `WgpuRenderer`，截圖是 `copy_texture_to_buffer` 讀回。CI 是 4 個 CPU、沒有 GPU 的 runner，用 Mesa 的 lavapipe。
- **device 遺失**：下一次 draw 重建 context（重試 3 次），通知 compositor，所有 tile 的資源丟掉、下一次 `prepare_tiles` 重畫。
- **canvas**：bitmap 在每個分頁的 canvas raster thread 上（CPU），每個新的 snapshot 整張上傳一次，自己一個 `TextureLayer`；`getImageData` 讀 CPU 的像素。
- **量得到的**：trace 的 `TileManager::PrepareTiles`、`raster::playback`（每個 tile）；log 的 `prepared tiles`（rastered、solid、skipped、kept）。**上傳沒有自己的 span**：它在 `PrepareTiles` 裡面、`raster::playback` 外面。
- **process 的邊界**：renderer 這一側的 crate（包括 `compositor` 和 `raster`）不依賴 `viz`，只經過 `protocol` 的訊息和介面碰 Viz；`compositor` 同步呼叫 `SharedImageInterface`、`SharedImage` 裡是 `Arc<dyn Any>`，是邊界規則列出的例外，拆 process 時要換成 mailbox 和 `ReturnResources`。`sanity.py boundary` 沒有禁止 renderer 這一側的 crate 直接依賴 wgpu（推論：它只擋 browser、net、viz、shell）。
- **順便看到、和程式對不上的文件**（還沒改）：`docs/benchmarks.md` 和 `docs/limitations.md` 寫「真的視窗才多了上傳的時間」，但 headless 和 windowless 也經過同一個介面上傳；`crates/shell/src/headless.rs` 開頭的說明還寫著 software 的 renderer。

## vello_gpu

上游 `main` 的 `112736df`。`rfcs/0007-gpu-raster/details.md` 的「vello_gpu 0.3」是 2026-10-05 讀的；之後上游改了不少，以這一節為準。

**API**

- `Scene::new(w, h)`：畫的方法和 vello_cpu 的 `RenderContext` 幾乎一一對應（fill、stroke、clip、layer、`glyph_run`、`fill_blurred_rounded_rect` 和它的 `_in` 版本、`set_aliasing_threshold`）。沒有 `set_mask`、沒有 `render`（render 在 `Renderer` 上）。建場景不需要 device：攤平、切 tile、strip 和覆蓋率在呼叫 fill、stroke 時就在 CPU 上算完。
- `Renderer::new(&Device, &Queue, &RenderTargetConfig) -> (Renderer, Resources)`。`render(&mut self, &Scene, &mut Resources, &mut CommandEncoder, RenderTarget, &TextureBindings, RenderOptions) -> Result<(), RenderError>`。
- **一個 `Renderer` 一種 target 的格式**；大小可以每次不同（讀 target 的 texture 的大小；超過 65,535 會 panic）。`RenderOptions` 有 `target_init`（`Clear` 或 `SrcOver`）和 `offset`。
- **`render` 只錄進呼叫的人給的 encoder**，不 submit、不寫 queue（開 glyph atlas 時例外）。上傳都經過 staging belt 錄進同一個 encoder。文件說那個 encoder 一定要送到這個 renderer 的 queue，否則之後的 render 可能畫錯；推論：錄的順序就是要 submit 的順序。**同一個 encoder 裡錄好幾次 render，API 沒有禁止，但上游沒有測試或例子這樣用**。
- **`Resources` 只能由 `Renderer::new` 產生**（`new` 不是公開的），而且只能配那一個 renderer；`glyph_run` 要 `&mut Resources`（裡面是 glyph 的準備快取和實驗性的 atlas）。所以**有文字的場景要拿得到 `Resources` 才建得起來**。
- thread（照型別讀的）：`Scene` 和 `Resources` 是 `Send`、不是 `Sync`；`Renderer` 是 `Send + Sync`。沒有 thread-local。

**需要的 GPU 功能**

- wgpu 30.0.0（和 machopo 同一個大版本）；MSRV 1.89。
- 不用 compute shader、不要求任何 wgpu 的 feature；上游的例子用 WebGL2 的 downlevel limits 跑。
- 持續佔用的：四張資料用的 texture（覆蓋率、paint、漸層、filter 的資料，各自最寬 4096，只會長大）、strip 的 vertex buffer、staging belt；場景有 layer、blend 或 filter 時另有中間的 texture（至少 512×512，照最大的 layer 長大，最多 4096，不會變小）。

**vello_cpu 可以、vello_gpu 會 panic 的**

| 功能 | 在哪裡 panic | machopo 哪裡用到 |
| --- | --- | --- |
| mask layer | `push_layer` 帶 mask 時 `unimplemented!` | `ClipPaths`（path 的 union） |
| 不隔離的破壞性 blend mode（Clear、Copy、SrcIn、DestIn、SrcOut、DestAtop） | `set_blend_mode` 的 assert；用在 `push_blend_layer` 上不會 | canvas 的 `globalCompositeOperation`、`ClearRect` |
| pixmap 當圖片來源 | render 時 panic（不是回傳錯誤）；上游寫明不在範圍內 | 每一張圖片、圖案 shader、每一種「畫到旁邊再貼回來」的離屏 |
| 點陣的 glyph（CBDT、sbix 的 emoji） | glifo 把它們畫成 pixmap 的圖片 | 彩色 emoji |
| 多個 primitive 的 filter graph、四種以外的 filter | `vello_common` 的 `unimplemented!`（vello_cpu 也一樣） | 已經在 playback 裡離屏做 |
| 邊長超過 65,535 的 target 或外部 texture | `unwrap` | 不會碰到（texture 上限是 8192） |

- `fill_blurred_rounded_rect` 要 `blurred_rounded_rect` feature；sweep gradient 要 `gradient_sweep`；bicubic、裁過的 texture、有顏色的 tint 要 `full_image_support`。預設都開。
- COLR 的 glyph 沒有找到會 panic 的地方。

**圖片**

- `upload_image` 和圖片的 atlas 在 `112736df`（#2061）拿掉了：**圖片只能是呼叫的人自己的 wgpu texture**，render 時用 `TextureBindings` 照 `TextureId` 綁上去（`ImageSource::ExternalTexture`）。內容要是 premultiplied。
- 取樣：整張、不是 High、沒有顏色的 tint 時用硬體的 sampler（Low 是 nearest，Medium 是 linear）；High（bicubic）和裁過的範圍在 shader 裡算。
- 沒綁到的 texture 回傳 `MissingTextureBinding`；把 target 自己當來源回傳 `TextureFeedbackLoop`。

**glyph 的快取**

- 實驗性、預設不開，要 `full_image_support` 和每個 run 的 `atlas_cache(true)`。outline 的 glyph 也是在 atlas 裡它的位置上畫（和 vello_cpu 一樣，RFC 0007 沒有採用的原因在這裡同樣成立）；開了之後 `render` 之前會自己建 encoder、`queue.submit`，淘汰時 `queue.write_texture`（推論：和「好幾個 tile 錄在同一個 encoder」的順序會打架）。

**輸出**

- premultiplied（pipeline 是 premultiplied 的 blend、clear 的顏色先 premultiply），shader 不做色彩空間的轉換；上游的測試和例子用 `Rgba8Unorm` 和 `Bgra8Unorm`。和 Viz 的 tile 現在的格式一致。target 要有 `RENDER_ATTACHMENT`（現在的 tile texture 沒有）。

**和 vello_cpu 的差**

- 上游共用的測試拿 vello_cpu 的 f32 管線當參考圖，GPU 預設每個 channel 容許差 1，filter 的測試 2 到 4。名字帶 `mask` 的、不隔離的六種破壞性 blend、點陣 emoji、裁過的圖片的一個測試在 GPU 上跳過。
- README 的「Current state」：比 vello_cpu 稍微不成熟；mask layer、複雜的 filter graph、某些不隔離的 blend mode 會 panic；有些失敗是 panic 而不是回傳錯誤；wgpu 的 backend 還沒完全最佳化；向量多、每個 frame 都變的場景比不上 compute 版。
- 沒有讀回的 helper、沒有處理 device 遺失的程式：都是呼叫的人的事。

## Chromium 怎麼做

照記得的寫的，2026-11-11 沒有對過 Chromium 的原始碼；接受這份 RFC 之前要對一次。

- **OOP-R**（現在的做法）：renderer 的 raster worker 把 paint op 序列化進 transfer buffer，GPU process 的 `RasterDecoder` 用 Skia（Ganesh 或 Graphite）畫進 `SharedImage`；renderer 只拿到 mailbox。圖片由 `GpuImageDecodeCache` 解碼、縮好、上傳成 GPU 的圖片，有預算、照最近用過的順序淘汰；glyph 經過 `SkStrikeServer`／`SkStrikeClient` 傳過去，GPU 那邊放進 atlas。
- **OOP-R 之前**：renderer 用自己的 GL context（command buffer）在 raster worker 上跑 Ganesh，GL 的指令送到 GPU process。
- **CPU 和 GPU 之間怎麼選**：整個 compositor 一起決定（`LayerTreeSettings::gpu_rasterization`、GPU 的黑名單），不是一個 tile 一個 tile；早期有照內容否決 GPU raster 的規則（例如太多複雜的 path），後來拿掉了。GPU 不能用時整個退回 software raster（`OneCopyRasterBufferProvider`、`ZeroCopyRasterBufferProvider`）。
- **資源**：`ResourcePool` 重用 tile 的資源；單色的 tile 在 raster 之前由 `SolidColorAnalyzer` 判斷，不靠讀回。
- 測試：layout test 的機器用 SwiftShader（軟體的 Vulkan）跑 GPU raster 的路徑。

## 參考資料

- `rfcs/0007-gpu-raster/README.md` 的「之後的可能」，`rfcs/0007-gpu-raster/details.md` 的「vello_gpu 0.3」和「CPU 和 GPU 的結果有多接近」。
- `rfcs/0004-multi-process/README.md`：拆 process 之後 renderer 碰不到 GPU（T1 到 T3 的做法）。
- vello 的 fork：`vello_gpu/README.md`、`vello_gpu/src/scene.rs`、`vello_gpu/src/render/wgpu/mod.rs`、`vello_gpu/src/render/common/scene_preparer.rs`、`CHANGELOG.md`。
