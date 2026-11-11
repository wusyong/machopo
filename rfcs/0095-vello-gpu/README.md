# RFC 0095：tile 的 raster 花在哪裡、上傳，和 GPU raster（vello_gpu）的評估

- 開始日期：2026-11-11
- 狀態：✅ 已完成：2026-11-11（量了 tile 的 raster 各部分的時間、改了上傳；GPU raster 的原型延後，已決定的事項第 6 條）
- 細節：`details.md`
- 進度和驗收：`progress.md`

## 摘要

這份 RFC 原本要讓 compositor 的 tile 用 vello_gpu 直接畫進 Viz 的 wgpu device 上的 texture。做法是先量、再決定，結果是：

- **量測**（做了）：`bench.py raster` 報出一次 `PrepareTiles` 裡建場景、render、讀像素和上傳各佔多少，以及有幾成的 tile 是只在 GPU 上填色的 renderer 畫得了的。
- **上傳**（做了）：量到把 tile 的像素放上 GPU 佔 `PrepareTiles` 的 26% 到 35%，先改這一段：一輪的 tile 一次 submit、tile 只建它用得到的 bind group、Viz 重用放掉的 texture。每次上傳從約 47 µs 降到 17 到 20 µs。
- **GPU raster**（沒有做，延後）：設計和調查留在這份 RFC 裡，給之後重新打開的人。

## 動機

**開始時的現況**（細節見 `details.md` 的「現在的 tile 怎麼畫、怎麼到 Viz」，那是改之前的樣子）：

- tile 在 compositor thread（或 raster worker）上由 vello_cpu 畫進 CPU 的 buffer，同一條 thread 再呼叫 Viz 的介面：建一個新的 texture、`write_texture`、submit 一次。每個 tile 一次。
- raster 是同步的：缺的 tile 全部畫完才畫 frame。捲動和改視窗大小時 frame 的長度就是 `PrepareTiles` 的長度。
- 每個 tile 的 `PrepareTiles` 是 0.11 到 0.73 ms（scale 2、256 的 tile）；其中上傳佔多少沒有量過（沒有自己的 span）。

**GPU raster 省得到和省不到的**：

- vello_gpu 是混合的做法：攤平、strip 和覆蓋率還是在 CPU 上算（和 vello_cpu 同一份程式），GPU 只做填色、合成和 blend。
- **省得到**：填色和合成、上傳、一個 tile 一次的 submit。
- **省不到**：建場景，尤其是文字（攤平和產生 strip）。
- 所以要先量各部分佔多少，才知道值不值得。

要解決的 `docs/limitations.md` 裡的限制：

- 「Viz、GPU 和記憶體」的「不做 cc 的 `ResourcePool`：每個重新 raster 的 tile 都建立新的 texture」：做了重用（已決定的事項第 5 條），那一條改寫成和 cc 不同的地方。

跟著改寫的：「Paint 和 raster」裡上傳怎麼做的那一句（一個 tile 一次 submit 改成一輪一次；原本寫只有真的視窗才上傳，headless 其實一樣）。

**做到的**：

- `bench.py raster` 看得到 raster 的每一段，和每個 tile「GPU 畫得了嗎」。
- 上傳少了約六成；畫出來的東西沒有變（reftest、情境照舊）。
- `unsafe` 沒有增加；renderer 這一側照舊只經過 `SharedImageInterface` 碰 Viz。

**沒有做的**：GPU raster 本身（下面「GPU raster 的設計」）；直接畫進 wgpu 的 staging、少一次複製；canvas 2D 的 GPU 路徑；async raster；不經過 tile 的快速路徑。

## 設計概要

給 machopo 的使用者：畫面沒有變。捲動和改視窗大小時每個 tile 準備得快一些（上傳那一段少了約六成，整體約少兩成，圖和色塊多的頁面比較明顯）。

給之後改 machopo 的人：

- `python3 scripts/bench.py raster` 的第二張表是 raster 的時間花在哪裡的起點：建場景、render、讀像素、上傳（建 texture、複製、bind group、submit）。
- `raster::playback_into` 回傳的 `CpuOnly` 說這次 playback 有哪些東西只有 CPU 能畫；compositor 的 `prepared tiles` log 照它數 tile。要做 GPU raster 時這是現成的可行性數字。
- 建 shared image 的人在建完一批之後要呼叫 `SharedImageInterface::flush`；不呼叫的話寫入要等 Viz 下一次畫 frame 才送出去。
- Viz 會重用 texture：shared image 的內容在最後一個引用放掉之後就可能被別的 shared image 蓋掉，所以不能在放掉引用之後還畫它（現在沒有這樣的路）。

## 詳細設計

### 量測

- **span**（都在畫 tile 的那條 thread 上）：`raster::scene`（把 op 畫進 Vello 的 context：攤平、strip、覆蓋率）和 `raster::render`（填色、合成成像素），都在 `raster::playback` 裡；`TileManager::ScanTile`（畫完讀像素看是不是單色）；`TileManager::UploadTile`，裡面是 Viz 的 `SharedImage::Write`（再分成 `CreateTexture`、`WriteTexture`、`BindGroups`）；`SharedImage::Submit` 在一輪結束的 `flush` 裡。
- **`raster::CpuOnly`**（已決定的事項第 3 條）：playback 實際走到只有 CPU 能畫的路時記一個旗標，`playback_into` 連同大小一起回傳（`Played`）。六類：圖片、mask（`ClipPaths`）、Vello 沒有的 filter（`ColorMatrix`、串接的）、blur（layer 的 blur 和 drop shadow、模糊的形狀、畫到旁邊的陰影）、圖案（record 或有間隔的）、有點陣 glyph 的字型。畫到旁邊的 layer 裡面的東西也算進來；不是圖片的圖、藏在 tile 外面的 clip 裡的 filter 不算。
- **compositor**：`TilesPrepared` 多了 `gpu`（一類都沒有的 tile）、`gpu_but_images`（只有圖片那一類的）和每一類的個數，寫進 `prepared tiles` 的 log；一個 tile 可以算進好幾類。只計數，不改怎麼畫。
- **`bench.py raster`**：報告的第二張表（各部分的時間和佔 `PrepareTiles` 的比例、tile 的計數），`raster.json` 多 `parts`。改視窗大小的階段在固定的時間裡能更新幾次就幾次，比較兩次的結果要用每個 tile 的時間。

### 上傳

- **一輪一次 submit**（第 4 條）：`SharedImageInterface` 多 `flush`（Chromium 的介面的 `Flush`）。`create_shared_image` 不再自己 submit；`prepare_tiles` 把這一輪的 tile 和 canvas 的 texture 都建完之後呼叫一次，一張都沒建時不呼叫。`GpuSharedImages::flush` 拿 `submit_lock`、送一次空的 submit（視窗被遮住、renderer 不畫時，寫入才不會在 staging 裡堆積）。
- **bilinear 的 bind group 用到才建**（第 4 條）：`UploadedResource::linear_bind_group` 是 `Arc<OnceLock>`，第一次那樣畫時在 Viz thread 上建（`GpuContext::linear`），renderer 複製的那一份共用。tile 永遠用 nearest，不再為它建第二個。
- **重用 texture**（第 5 條）：`viz` 的 `TexturePool`，在 `GpuContext` 上（一個 device 一個）。
  - `SharedImageBacking` 被丟掉時（compositor 的 tile 和畫過它的 frame 都放了）把它的 texture 連同 bind group 還回池子。renderer 手上可能還有上一個 frame 的那一份，但它只畫 frame 拿著的資源；還在往螢幕上走的繪圖是在之後的寫入之前 submit 的，queue 照順序執行。
  - `create_shared_image` 先找同樣大小的（最後放進去的那一張），有就只寫像素；沒有才建新的。
  - 上限 32 MB、256 張，超過時放掉最舊的；單獨一張就超過 32 MB 的不留。device 遺失時池子跟著舊的 context 一起丟。
  - Viz 自己上傳的資源（沒有 shared image、像素跟著 frame 來的）不經過池子。
- **沒有做的**：直接畫進 wgpu 的 staging buffer、少一次複製（Chromium 的 one-copy raster）。現在的上傳幾乎都是那一次複製，佔 `PrepareTiles` 的 11% 到 20%。

### 測試

- `raster`：`a_playback_tells_what_only_a_cpu_draws`（每一類各自被記到、形狀和有外框線的文字不算、藏起來的和不是圖片的不算）。
- `compositor`：`tiles_are_counted_by_what_only_a_cpu_draws`；`shared_images_are_flushed_once_all_of_a_frames_are_made`。
- `viz`：`a_texture_has_what_draws_it_bilinearly_once_it_is_drawn_so`；`a_shared_image_let_go_gives_its_texture_to_the_next_of_its_size`（重用之後畫出來是新的像素、還有人拿著時不還）；`the_texture_pool_keeps_the_newest_within_its_limits`。
- `bench.py raster` 自己的單元測試（log 的計數、報告的第二張表）。
- mutation test 三份（`mutants-0.toml`、`mutants-1a.toml`、`mutants-1b.toml`），結果在 `progress.md`。
- 測試守不住的：`flush` 裡有沒有真的 submit（`docs/test-gaps.md` 的「沒有計數守著的優化」）；span 本身。

### 分階段

0. **量測**：做了。
1. **改上傳**：1a 分開量 write、一輪一次 submit、tile 只建一個 bind group；1b 重用 texture。做了。
2. **GPU raster 的原型、正式做和圖片的 GPU 快取**：延後（第 6 條）。

## GPU raster 的設計（沒有做）

2026-11-11 寫的，沒有確認、沒有實作；裡面的「看法」不是決定。vello_gpu 的事實見 `details.md` 的「vello_gpu」（讀程式得到的，沒有跑過；標「推論」的要先確認）。重新打開時從這裡開始。

### GPU raster 的程式放在哪裡

vello_gpu 要 wgpu 的 device；`compositor` 和 `raster` 是 renderer 這一側的 crate，現在都不依賴 wgpu，texture 是經過 `SharedImageInterface`（Viz 實作）拿到的。

- **A. 放在 Viz 那一側，compositor 經過介面要求「把這個 tile 畫進 texture」**（2026-11-11 的看法：A）。`protocol` 的介面多一個入口（對照 Chromium 的 `RasterInterface`）：給 display list、要畫的範圍、scale，拿回 `SharedImage`。`viz` 依賴 `raster`（共用 `Player`）和 vello_gpu。
  - 好處：renderer 這一側照舊碰不到 device；拆 process 之後這個入口就是 OOP-R 的位置（display list 能序列化時），不用搬。
  - 代價：介面現在收的是 process 裡的 `Arc<DisplayItemList>`，不能序列化，是邊界規則的例外一的延伸；Viz 多了 raster 的責任（在 Chromium 也是 GPU process 做）。
  - 呼叫照舊是同步的、在畫的那條 thread 上跑 Viz 的程式（和現在的 `create_shared_image` 一樣），不是丟給 Viz thread。
- **B. `raster` 直接依賴 wgpu 和 vello_gpu，從介面拿 device**。對照 Chromium 在 OOP-R 之前的做法（renderer 自己有 GL context）。
  - 好處：改動最少，playback 都在一個 crate。
  - 代價：renderer 這一側拿到 device；拆 process 之後這條路整個不能用（sandbox 擋 GPU），要重做成 A。

### 介面和資料的流向（照 A）

- `SharedImageInterface` 加 `raster_tiles(jobs) -> Vec<結果>`（名字待定）：一次給一批 tile（同一次 `prepare_tiles` 裡 GPU 畫得了的），每個 job 是 display list、要畫的 op 的範圍（現在的 `ops_for`）、tile 的矩形、scale、底色。
- Viz 這一側：每個 job 建一個 `Scene`（CPU 的工作：攤平和覆蓋率），全部建完之後一個 encoder、每個 tile 一次 `Renderer::render`、一次 submit。每個 tile 拿到自己的 texture。
- 結果是 `SharedImage`（和現在一樣進 `TransferableResource`），或「這個 tile 畫不了」（compositor 再走 CPU）。
- **批次的大小和順序**：vello_gpu 的 encoder 要照錄的順序 submit；同一個 encoder 錄很多次 render 上游沒有測過，原型第一件事就是確認。

### 哪些 tile 走 GPU

整個 compositor 的條件（任何一項不成立就全部走 CPU）：

- 有介面、device 沒有遺失；
- adapter 不是 CPU 的（`device_type != Cpu`；WARP、lavapipe 上 GPU raster 只會更慢）；
- 旗標沒有關掉。測試預設關（見「測試」）。

每個 tile 的條件：畫之前掃它的 op（`ops_for` 已經找出這個 tile 碰到的 op），碰到下面任何一項就走 CPU：

| op 或內容 | 為什麼 | 之後能不能改走 GPU |
| --- | --- | --- |
| `ClipPaths` | 用 mask layer 做 path 的 union，vello_gpu 沒有 | 上游做了 mask，或換成別的做法 |
| 離屏的每一項：`ColorMatrix` 和串接的 filter、mask blur、不是單一圓形半徑的陰影、圖案 shader（record、spacing、重複的漸層） | 畫到旁邊的 pixmap 再當圖片貼回來，vello_gpu 不收 pixmap | 把離屏的結果上傳成 texture，或離屏也在 GPU 上畫 |
| 圖片（`DrawImage`、圖片的 shader） | 同上：要是 texture | GPU 的圖片快取 |
| 點陣的 glyph（CBDT、sbix） | glifo 把它畫成 pixmap | 上游改，或 machopo 自己把那個 glyph 當圖片畫 |
| `WritePixels`、不隔離的破壞性 blend mode | 只在 canvas 的錄製裡出現 | canvas 不在範圍內 |

- **圖片是最大的一塊**：有圖片的 tile 在原型裡全部走 CPU，圖片多的頁面就看不到好處。所以原型的量測要分開報「走 GPU 的 tile 佔幾成」。
- **同一個 layer 裡 CPU 和 GPU 的 tile 混用**（2026-11-11 的看法：允許）：兩邊最多差 1（RFC 0007 已經接受 tile 之間差 1）。另一個做法是照 layer 或整頁決定（Chromium 是整個 compositor 一起決定），簡單、沒有混用的疑慮，但一張圖片就讓整個 layer 回到 CPU。看原型量到的差異再定。

### 共用的 playback

- `Player` 對一個 trait 泛型（暫名 `Painter`）：fill、stroke、clip、layer、glyph run、blurred rounded rect、transform 和 paint 的設定。vello_cpu 的 `RenderContext` 和 vello_gpu 的 `Scene` 各實作一次；兩邊的方法幾乎一一對應。
- CPU 才有的（mask、pixmap 的圖片、離屏、`render`）不進 trait：GPU 的路在判斷那一步就不會走到。走到的話是 bug，`debug_assert`。
- 為上游的 bug 加的繞道（`shows`、漸層拉長、glyph 的對齊和 culling）在 `Player` 裡，兩條路共用。vello_gpu 有沒有同樣的 bug、有沒有自己的，原型時各用一個測試確認。

### 文字和 thread

- `glyph_run` 要 `&mut Resources`，而 `Resources` 只能由 `Renderer::new` 產生、只能配那個 renderer。所以建有文字的場景時要拿著某個 renderer 的 `Resources`。
- **a. 每條畫 tile 的 thread 一個 `Renderer`**（2026-11-11 的看法：先這樣）：建場景可以平行（`--num-raster-threads`），各自 render 進自己的 encoder；每個 renderer 有自己的一套 pipeline 和資料 texture（記憶體和啟動的成本要量）。submit 經過現有的 `submit_lock`。
- **b. 一個 `Renderer`，建場景時輪流拿它的 `Resources`**：記憶體最少，但文字（最花時間的部分）變成一次只能一條 thread。
- **c. 在 fork 裡讓 `Resources` 可以獨立建立**（一個 renderer 配好幾份）：最好的形狀，但要改 Vello，先不做。
- 預設只有 compositor thread 一條在畫，三種在預設的設定下一樣；差別在開 raster worker 時。

### 圖片

- vello_gpu 的圖片只能是呼叫的人自己的 texture。Viz 這一側多一個**圖片的 GPU 快取**：照 `ImageData::id` 和層（RFC 0007 的預先縮小的層）把解好的像素上傳成 texture，render 時綁上去。對照 Chromium 的 `GpuImageDecodeCache`。
- 要定的：預算和淘汰（GPU 的記憶體；CPU 的圖片快取本來就沒有上限，這裡不能再沒有）；解碼照舊在 CPU 上、在畫的那條 thread；裁過的來源範圍和 `High` 的品質走 shader 的路（要 `full_image_support`，預設有）。
- 這一步不做時，有圖片的 tile 走 CPU。

### 單色的 tile、texture 和記憶體

- **單色**：GPU 的 tile 讀不回像素，只靠畫之前的 `color_if_solid_in_rect`（已經有）。它認不出來、畫完才知道是單色的 tile，在 GPU 的路上就是一張一般的 texture（多用記憶體，不影響正確性）。
- **texture**：usage 多 `RENDER_ATTACHMENT`。每個 tile 建新的 texture 的成本原型時量；太高的話在 Viz 這一側重用（cc 的 `ResourcePool`），那時連 CPU 的路一起用。
- **vello_gpu 自己佔的**：資料 texture 和中間的 texture 只會長大、不會變小（一個 renderer 最多幾十 MB，照場景）。記進 `docs/limitations.md`。

### 失敗的時候

- **`RenderError`**（資料 texture 放不下、中間的 texture 超過上限）：那個 tile 改走 CPU，計數。
- **panic**：GPU 的路同樣包在 RFC 0007 的 `guarded` 裡（release 接住、記 log、Console 的警告），接住之後那個 tile 改走 CPU 一次，那個 renderer 丟掉重建。
- **device 遺失**：現有的流程會丟掉所有 tile、重畫；renderer 跟著 device 的 generation 重建。重建好之前走 CPU（像素跟著 frame 送）。

### 測試（GPU raster 的）

- **預設走 CPU 的**：`cargo test`、reftest、WPT、情境（2026-11-11 的看法）。理由：結果要在每台機器上一樣；CI 是 lavapipe；開發機上有真的 GPU 時不能讓 reftest 的結果跟著顯示卡變。對照 Chromium：layout test 用 SwiftShader 跑 GPU 的路徑，結果固定是因為每台機器都是同一個軟體的 GPU。
- **強制走 GPU 的測試**（旗標，CPU 的 adapter 上也走）：
  - **CPU 和 GPU 的比對**：`raster` 的 `one_of_everything_is_drawn` 裡 GPU 畫得了的每一種 op，兩條路各畫一次，每個 channel 最多差 1（filter 的範圍照量到的）。在 lavapipe 和 WARP 上跑。
  - **判斷的測試**：每一種「GPU 畫不了」的 op 都被判斷擋下來；繞過判斷直接交給 vello_gpu 時 panic 的各配一個「上游還缺」的測試（`#[should_panic]`），上游補上時失敗，照 `docs/render-gaps.md` 拿掉那一條。
  - **混用的接縫**：同一個 layer 相鄰的 tile 一個 CPU、一個 GPU，接起來最多差 1。
  - **一批 tile 一個 encoder**：很多個 tile 一次 submit，每個都和單獨畫的一樣。
  - **失敗的路**：`RenderError`、注入的 panic、device 遺失之後，tile 都畫得出來（走 CPU）。
  - reftest 和情境各挑一部分用強制 GPU 再跑一次（哪些、CI 上跑不跑：待定；lavapipe 上會比較慢）。
- **mutation test**：照 `docs/testing.md`。
- **量測**：`bench.py raster` 加 `--window` 才有真的 GPU 的意義（headless 也有 device，但要確認用的是哪個 adapter）；報告多出「走 GPU 的 tile 數、走 CPU 的理由」。

## 缺點

做了的部分：

- **Viz 多一塊狀態**：texture 的池子平常多佔最多 32 MB，沒有閒置逾時。
- **重用讓「放掉引用之後內容就不可靠」成為一條要記得的規則**（設計概要的最後一條）。
- **`flush` 是呼叫的人的責任**：新的建 shared image 的地方漏掉時不會壞，只是寫入晚一點才送出去。
- **量測的程式留在熱的路上**：每個 tile 幾個 span（沒開 trace 時不建）和幾個旗標；字型多記一個「有沒有點陣 glyph」。

GPU raster 如果做，要付的（當時列的）：兩條 raster 的路都要維護和測試；vello_gpu 比 vello_cpu 不成熟；Viz 多了 raster 的責任和圖片的 GPU 快取；邊界的例外變大（介面收不能序列化的 display list）；開發機和 CI 走的路不同。

## 理由和其他做法

- **先量再決定**（第 1、2 條）：量到上傳比預期的大（26% 到 35%），而它每一頁、每個 tile 都受惠、不用第二條 raster 的路，所以先改它（第 4 條）。
- **GPU raster 延後**（第 6 條）：它還能拿走的上限是 26% 到 71%，但最高的那一頭要先有圖片的 GPU 快取，vello_gpu 自己在 CPU 上的成本又沒有量過；文字多的頁面瓶頸在建場景，它幫不上。
- **重用 texture 放在 Viz 這一側、有上限**（第 5 條）：現在沒有 `ReturnResources`，資源的生命是引用計數，最後一個引用在哪裡放掉只有 Viz 這一側看得到。
- **不做少一次複製**：介面要能借出可寫的記憶體，改動比前三項大，上限是現在的上傳（11% 到 20%）。
- **經典的 vello（compute）、不經過 tile 的快速路徑**：RFC 0007 已經比較過。

## 先例

- **Chromium**：OOP-R，paint op 序列化到 GPU process、用 Skia 畫進 `SharedImage`；圖片有 `GpuImageDecodeCache`；CPU 或 GPU 是整個 compositor 一起決定；tile 的資源用 `ResourcePool` 重用（`details.md` 的「Chromium 怎麼做」）。照做的：raster 在 Viz 那一側、經過介面、圖片的 GPU 快取、畫之前判斷單色。不照做的：一個 tile 一個 tile 選（因為 vello_gpu 缺的功能比 Ganesh 多，而兩條路的結果比 Skia 的 CPU 和 GPU 接近）。
- **Servo**：canvas 試過 GPU 的 vello（compute 版），每個 frame 要讀回再上傳、比 CPU 慢，改回 vello_cpu。提醒：GPU 的路只有在不讀回時才划算，所以 canvas 不在範圍內。
- **Blitz、Linebender 的例子**：vello_gpu 畫整個視窗、一個 frame 一次 render；沒有找到「很多小的 target、一個 encoder」的用法。machopo 會是少見的用法，要自己確認。

## 已決定的事項

沿用 RFC 0007 的：raster 用 Vello 系列（第 1 條）、GPU raster 以 vello_gpu 為主、不能用時走 vello_cpu（第 3、8 條）、glyph 的 atlas 不採用（第 46 條）。

1. **2026-11-11：先做第 0 步的量測，量完再決定**（使用者）。第 0 步只加量測（上傳和 raster 各部分的 span、每個 tile「GPU 畫得了嗎」的計數、`bench.py raster` 的報告），不改 tile 怎麼畫，做完可以先 merge。
2. **2026-11-11：「只改上傳」和 GPU raster 是並列的兩個候選**（使用者），第 0 步的數字出來之後一起比；也可能兩個都不做。
3. **2026-11-11：「GPU 畫得了嗎」照實際畫的記，不照 op 掃**（實作第 0 步時定的，只影響量測）。playback 走到只有 CPU 能畫的路（圖片、mask、Vello 沒有的 filter、畫到旁邊的 blur、圖案的 pixmap、有點陣 glyph 的字型）時記一個旗標，回傳給 compositor 計數。比事先掃 op 準（不是圖片的圖、藏在 tile 外面的 clip 裡的 filter 不算），也不用另外維護一份和 playback 對不上的判斷。真的做 GPU raster 時要在畫之前知道，那時才需要事先掃的版本。blur 另外算一類：vello_gpu 有 blur 的 filter layer，現在畫到旁邊是為了 tile 邊緣以外的內容，GPU 上可以換做法。
4. **2026-11-11：先改上傳，GPU raster 等它做完再量、再決定**（使用者照建議）。第 0 步量到（`progress.md` 的「量測」）上傳佔 `PrepareTiles` 的 26% 到 35%、render 佔 11% 到 38%、建場景（兩個做法都省不到）佔 21% 到 60%。改上傳每一頁、每個 tile 都受惠（包括軟體的 adapter 和之後拆 process 的路），不用第二條 raster 的路；GPU raster 之後也需要其中的一部分（一批一次 submit、texture 的重用）。不選直接做 GPU raster 的原型（它自己在 CPU 上的成本沒有量過，圖片的 GPU 快取沒做之前只有 67% 到 100% 的 tile 走得了）。順序，每做一項重跑 `bench.py raster`：
    - 先把 Viz 的 write 那一段分開量（建 texture、`write_texture`、建 bind group）。
    - 一次 `PrepareTiles` 只 submit 一次；tile 只建它用得到的 bind group。
    - 重用 texture（cc 的 `ResourcePool`）：看分開量的結果，開始前再確認。
    - 直接畫進 wgpu 的 staging buffer、少一次複製（Chromium 的 one-copy raster）：多半不做，除非複製佔了大半。
5. **2026-11-11：重用 texture：Viz 這一側一個有上限的池子**（使用者照建議）。一批一次 submit 和 tile 只建一個 bind group 之後（`progress.md` 的「量測」），上傳還佔 `PrepareTiles` 的 16% 到 24%，其中建 texture 和 bind group 約一半。
    - **什麼時候算不用了**：shared image 的最後一個引用放掉時（compositor 的 tile 和畫過它的 frame 都放了），它的 texture 連同 bind group 還回它那個 device 的池子。renderer 手上可能還有上一個 frame 的那一份，但它只畫 frame 拿著的資源。還在往螢幕上走的繪圖是在之後的寫入之前 submit 的，queue 照順序執行。
    - **取用**：`create_shared_image` 先找同樣大小的（最後放進去的那一張），有就只寫像素；沒有才建新的。
    - **上限**：32 MB、256 張，超過時放掉最舊的；單獨一張就超過 32 MB 的不留。device 遺失時池子跟著舊的 context 一起丟。數字是保守的起點，沒有量過別的值。
    - 對照 Chromium：cc 的 `ResourcePool` 在 renderer 那一側（資源經過 `ReturnResources` 還回來），照大小和格式重用、有記憶體上限、閒置一段時間的釋放。machopo 放在 Viz 這一側是因為現在沒有 `ReturnResources`（資源的生命是引用計數）；拆 process 時跟著那一條例外一起改。沒有做閒置逾時：池子最多多佔 32 MB。
    - 不選不做（上傳已經少了四成，但建 texture 是剩下的一半）；不選沒有上限（改視窗大小會留下很多再也用不到的大小）。
6. **2026-11-11：這份 RFC 停在量測和改上傳，GPU raster 的原型延後**（使用者照建議）。改完上傳之後（`progress.md` 的「量測」），GPU raster 還能拿走的是 render、讀像素和剩下的上傳：SQLite 26%、Gutenberg 34% 到 48%、NewsSite-Next 53% 到 71%，要扣掉 vello_gpu 自己在 CPU 上的工作（沒有量過）；建場景（25% 到 71%，文字多的頁面最高）它省不到。效益最高的是圖片多的頁面，而那正好要先有圖片的 GPU 快取。在不知道 vello_gpu 自己的成本時投入這些，順序不對。不選現在做原型；不選把剩下的上傳也拿掉（直接畫進 wgpu 的 staging：上限是現在的 11% 到 20%，介面的改動比前三項大）。下一步是量建場景那一段（另一份 RFC）。重新打開的條件和那時要先確認的事在「之後的可能」。

## 待決定

沒有。原本列的（GPU raster 的程式放在哪裡、同一個 layer 裡混不混用、thread 的模型、旗標、圖片快取的預算、強制 GPU 的測試跑哪些）都跟著原型一起延後，留在「GPU raster 的設計」裡。

## 之後的可能

- **建場景那一段的量測**（下一份 RFC）：改完上傳之後它是最大的一塊（文字多的頁面五到七成）。把它再拆開，看有沒有 machopo 這一側能省的；連同 `--num-raster-threads` 預設開不開的最新數字。
- **GPU raster 的原型**：重新打開前先確認的：vello_gpu 自己在 CPU 上的成本（編 paint、上傳 strip 和覆蓋率）；一個 encoder 錄很多個小 target（上游沒有這樣用過）；圖片的 GPU 快取要不要一起做（沒有它時這三頁只有 67% 到 100% 的 tile 走得了，圖片多的頁面最低）；有 `filter`、`box-shadow`、`clip-path` 的頁面有幾成的 tile 走得了（這次量的三頁一個都沒有）。那時 texture 要能當 render target，池子要跟著改。
- **直接畫進 wgpu 的 staging**（one-copy raster）。
- **async raster**：現在 raster 慢就是 frame 變長；先畫已經好的 tile、沒好的畫 checkerboard 才是結構性的解法，要 tile 的優先順序和 activation 的條件，另一份 RFC。
- canvas 2D 的 GPU 路徑；不經過 tile 的快速路徑；拆 process 之後的 OOP-R（「GPU raster 的設計」的 A 的介面變成跨 process 的；texture 的池子跟著 `ReturnResources` 搬到 renderer 那一側或留在 Viz）。
