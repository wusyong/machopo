# RFC 0095：tile 的 GPU raster（vello_gpu）

- 開始日期：2026-11-11
- 狀態：🚧 已排程：2026-11-11（第 0 步的量測做了；先改上傳（第 1 步，已決定的事項第 4 條），做完再量、再決定要不要做 GPU raster 的原型）
- 細節：`details.md`
- 進度和驗收：`progress.md`

## 摘要

compositor 的 tile 用 vello_gpu 直接畫進 Viz 的 wgpu device 上的 texture，省掉「CPU 上填色再上傳」；vello_gpu 畫不了的 tile、沒有真的 GPU 的機器、測試，照舊用 vello_cpu，那是正式的另一條路，不只是備援。

和 RFC 0007 一樣是一次嘗試：先補上量測、做原型，看過「有多少 tile 走得了 GPU、快多少、畫得對不對」再決定要不要繼續。確認了的只有「已決定的事項」的那幾條；其餘的選項還沒有確認，下面寫的「看法」不是決定。

## 動機

**現況**（細節見 `details.md` 的「現在的 tile 怎麼畫、怎麼到 Viz」）：

- tile 在 compositor thread（或 raster worker）上由 vello_cpu 畫進 CPU 的 buffer，同一條 thread 再呼叫 Viz 的介面：建一個新的 texture、`write_texture`、submit 一次。每個 tile 一次。
- raster 是同步的：缺的 tile 全部畫完才畫 frame。捲動和改視窗大小時 frame 的長度就是 `PrepareTiles` 的長度。
- RFC 0007 量到每個 tile 的 `PrepareTiles` 是 0.11 到 0.73 ms（scale 2、256 的 tile）；其中上傳佔多少**沒有量過**（沒有自己的 span）。

**GPU raster 省得到和省不到的**：

- vello_gpu 是混合的做法：攤平、strip 和覆蓋率還是在 CPU 上算（和 vello_cpu 同一份程式），GPU 只做填色、合成和 blend。
- **省得到**：填色和合成（大片的色塊、漸層、圖片、半透明的 layer、blur）；上傳（像素本來就在 GPU 上）；一個 tile 一次的 submit（很多 tile 錄進同一個 encoder、一次 submit）。
- **省不到**：文字。RFC 0007 量到文字多的頁面每個 tile 127 µs 裡有 104 µs 在 `DrawGlyphs`，那是攤平和產生 strip，留在 CPU 上。
- 所以文字多的頁面可能幾乎不變，圖片和色塊多、scale 高的頁面才看得到差別。**這是這份 RFC 最大的不確定，要先量**。

**為什麼還是值得試**：

- RFC 0007 選 Vello 的理由之一就是 CPU 和 GPU 共用算覆蓋率的程式，兩條路的結果幾乎一樣（差 1），可以混用。
- tile 越大、scale 越高，CPU 填色和上傳的成本越高（scale 2 的全螢幕是幾百萬個 pixel）；GPU 的成本幾乎不跟著 pixel 數長。
- 這是之後其他事情的前提：不經過 tile 的快速路徑、canvas 的 GPU 路徑、async raster 時不卡 compositor thread 的上傳。

要解決的 `docs/limitations.md` 裡的限制：

- 「Viz、GPU 和記憶體」的「不做 cc 的 `ResourcePool`：每個重新 raster 的 tile 都建立新的 texture」：第 1 步做了重用（已決定的事項第 5 條），那一條改寫成和 cc 不同的地方。

做完時跟著改寫、但不一定解決的：
- 「Paint 和 raster」寫 tile 的 raster 是同步的、上傳一個 tile 一次 submit 的那一條（上傳的部分）。

**目標**：

- 有真的 GPU 時，vello_gpu 畫得了的 tile 直接畫進 texture；其他的走 vello_cpu，同一個 frame 裡可以混用。
- 圖片和色塊多的頁面，`bench.py raster` 的 `PrepareTiles` 明顯變短；文字多的頁面不變慢。數字的門檻在第 0 步量過現況之後定。
- 畫出來的和 vello_cpu 每個 channel 最多差 1（filter 的範圍裡差多少照量到的定），有測試守著。
- **GPU 的路不會 panic**：vello_gpu 會 panic 的功能在畫之前從 op 判斷出來、不交給它；`catch_unwind` 只是最後一道。
- reftest、WPT、情境的結果不變（它們預設照舊走 vello_cpu），另外有測試在 CPU 的 adapter 上強制走 GPU 的路。
- `unsafe` 不增加；renderer 這一側照 process 的邊界寫（見「GPU raster 的程式放在哪裡」）。

**不在範圍內**：

- **canvas 2D**：留在 CPU（`getImageData` 要讀回；破壞性的 blend mode vello_gpu 不支援）。
- **拆 process、OOP-R**：這份 RFC 的 GPU raster 只在單一 process 時成立；介面照之後能變成 OOP-R 的樣子設計，但不做序列化。
- **async raster、checkerboard、tile 的優先順序**。
- **不經過 tile 的快速路徑**（矩形、圓角、陰影直接變成 Viz 的 quad）。
- **glyph 的 atlas**：vello_gpu 的和 vello_cpu 的是同一套（實驗性、glyph 在 atlas 裡它的位置上畫），RFC 0007 沒有採用的原因同樣成立。
- **改 Vello 讓它支援缺的功能**（mask layer、pixmap 的圖片）：先在 machopo 這一側判斷和繞過；真的需要 patch 時照 fork 的規則另外確認。

## 設計概要

給 machopo 的使用者：畫面看不出差別。有獨立或內建顯示卡的機器上，圖片和色塊多的頁面捲動和改視窗大小比較順；文字多的頁面大致一樣。可以用旗標關掉（`--gpu-raster=off`，名字待定），`about:gpu` 之類的頁面看得到現在用哪一條路（要不要做待定）。

給之後改 machopo 的人：

- `raster` 的 playback 變成對「畫的對象」泛型：同一份 `Player` 把 `PaintOp` 畫進 vello_cpu 的 `RenderContext` 或 vello_gpu 的 `Scene`。加新的 `PaintOp` 時兩條路一起有。
- 每個 tile 在畫之前先問「GPU 畫得了嗎」（從它的 op 判斷）。畫不了的理由有計數，trace 和 log 看得到。
- 加新的繪圖功能時，`docs/render-gaps.md` 多一欄：vello_gpu 有沒有、沒有時那個 tile 走 CPU。
- 測試預設走 CPU；要測 GPU 的路時用旗標強制（CI 的 lavapipe 上也跑得動）。

## 詳細設計

vello_gpu 的事實見 `details.md` 的「vello_gpu」（2026-11-11 讀程式得到的，沒有跑過；標「推論」的原型要先確認）。

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
| 離屏的每一項：`ColorMatrix` 和串接的 filter、mask blur、不是單一圓形半徑的陰影、圖案 shader（record、spacing、重複的漸層） | 畫到旁邊的 pixmap 再當圖片貼回來，vello_gpu 不收 pixmap | 把離屏的結果上傳成 texture，或離屏也在 GPU 上畫（第 4 步的候選） |
| 圖片（`DrawImage`、圖片的 shader） | 同上：要是 texture | GPU 的圖片快取（第 3 步） |
| 點陣的 glyph（CBDT、sbix） | glifo 把它畫成 pixmap | 上游改，或 machopo 自己把那個 glyph 當圖片畫 |
| `WritePixels`、不隔離的破壞性 blend mode | 只在 canvas 的錄製裡出現 | canvas 不在範圍內 |

- **圖片是最大的一塊**：有圖片的 tile 在原型（第 2 步）全部走 CPU，圖片多的頁面就看不到好處。所以原型的量測要分開報「走 GPU 的 tile 佔幾成」。
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

### 圖片（第 3 步）

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

### 測試

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

### 分階段

0. **量測先補上**（做了；可以先 merge）：上傳自己的 span、`bench.py raster` 報出上傳佔 `PrepareTiles` 的幾成；每個 tile「GPU 畫得了嗎」的判斷先做出來（只計數，不改怎麼畫），報出各頁面有幾成的 tile 畫得了、畫不了的理由分布。**這一步的數字決定要不要做第 1 步**：上傳和填色加起來只佔一成的話，這份 RFC 停在這裡。
1. **改上傳**（已決定的事項第 4 條）：不動 raster，只改像素怎麼變成 Viz 的 texture。做完重量，再決定下面幾步做不做。
2. **GPU raster 的原型**（不 merge，旗標切換）：`Player` 改成泛型、Viz 這一側的 `raster_tiles`、沒有圖片的 tile 走 GPU。確認 `details.md` 標「推論」的每一項（一個 encoder 錄很多次 render、thread、premultiplied）。量 `bench.py raster --window`，和 CPU 的截圖比。**檢查點**：快多少、差多少、vello_gpu 穩不穩。
3. **正式做、加上圖片的 GPU 快取**：測試、mutation、文件（`docs/render-gaps.md` 的那一欄、`docs/architecture.md`、`docs/limitations.md`）。
4. **看量測決定的**：離屏的項目也走 GPU、預設打開。

## 缺點

- **兩條 raster 的路都要維護和測試**：每個新的 `PaintOp` 要想兩邊，CPU 和 GPU 的差異是新的一類 bug。
- **vello_gpu 比 vello_cpu 不成熟**：上游自己說有些失敗是 panic；mask、pixmap 的圖片、點陣 emoji 都不支援；API 還在大改（圖片的 atlas 上個月才拿掉）。每次跟上游都可能要改這一層。
- **可能不夠快**：覆蓋率還在 CPU 上；文字多的頁面可能沒有差別。
- **Viz 多了責任和記憶體**：raster、圖片的 GPU 快取、每個 renderer 自己的 texture。
- **邊界的例外變大**：介面收不能序列化的 display list，拆 process 時要補序列化才能留著 GPU raster。
- **開發機和 CI 走的路不同**：真的 GPU 的路主要靠強制的測試和手動的視窗確認。

## 理由和其他做法

- **不做，維持 vello_cpu 加上傳**：最簡單。如果第 0 步量到上傳和填色佔得很少，這就是結論。
- **只改上傳**（一批 tile 一次 submit、重用 texture、staging 的 buffer 直接當 raster 的目標），不做 GPU raster：改動小、每一頁都受惠、沒有第二條 raster 的路。**這可能才是比較划算的第一步**；第 0 步的數字出來之後和 GPU raster 一起比。
- **經典的 vello（compute）**：RFC 0007 已經否決。
- **不經過 tile 的快速路徑**（GPUI 的做法）：和 GPU raster 不衝突，但等 GPU raster 量過再說。
- 放在哪裡（A、B）、同一個 layer 裡混不混用、thread 的模型（a、b、c）的比較在「詳細設計」。

## 先例

- **Chromium**：OOP-R，paint op 序列化到 GPU process、用 Skia 畫進 `SharedImage`；圖片有 `GpuImageDecodeCache`；CPU 或 GPU 是整個 compositor 一起決定；tile 的資源用 `ResourcePool` 重用（`details.md` 的「Chromium 怎麼做」）。照做的：raster 在 Viz 那一側、經過介面、圖片的 GPU 快取、畫之前判斷單色。不照做的：一個 tile 一個 tile 選（因為 vello_gpu 缺的功能比 Ganesh 多，而兩條路的結果比 Skia 的 CPU 和 GPU 接近）。
- **Servo**：canvas 試過 GPU 的 vello（compute 版），每個 frame 要讀回再上傳、比 CPU 慢，改回 vello_cpu。提醒：GPU 的路只有在不讀回時才划算，所以 canvas 不在範圍內。
- **Blitz、Linebender 的例子**：vello_gpu 畫整個視窗、一個 frame 一次 render；沒有找到「很多小的 target、一個 encoder」的用法。machopo 會是少見的用法，要自己確認。

## 已決定的事項

沿用 RFC 0007 的：raster 用 Vello 系列（第 1 條）、GPU raster 以 vello_gpu 為主、不能用時走 vello_cpu（第 3、8 條）、glyph 的 atlas 不採用（第 46 條）。

1. **2026-11-11：先做第 0 步的量測，量完再決定**（使用者）。第 0 步只加量測（上傳和 raster 各部分的 span、每個 tile「GPU 畫得了嗎」的計數、`bench.py raster` 的報告），不改 tile 怎麼畫，做完可以先 merge。
2. **2026-11-11：「只改上傳」和 GPU raster 是並列的兩個候選**（使用者），第 0 步的數字出來之後一起比；也可能兩個都不做。
3. **2026-11-11：「GPU 畫得了嗎」照實際畫的記，不照 op 掃**（實作第 0 步時定的，只影響量測）。playback 走到只有 CPU 能畫的路（圖片、mask、Vello 沒有的 filter、畫到旁邊的 blur、圖案的 pixmap、有點陣 glyph 的字型）時記一個旗標，回傳給 compositor 計數。比事先掃 op 準（不是圖片的圖、藏在 tile 外面的 clip 裡的 filter 不算），也不用另外維護一份和 playback 對不上的判斷。真的做 GPU raster 時要在畫之前知道，那時才需要事先掃的版本。blur 另外算一類：vello_gpu 有 blur 的 filter layer，現在畫到旁邊是為了 tile 邊緣以外的內容，GPU 上可以換做法。
4. **2026-11-11：先改上傳，GPU raster 等它做完再量、再決定**（使用者照建議）。第 0 步量到（`progress.md` 裡第 0 步的那一段）上傳佔 `PrepareTiles` 的 26% 到 35%、render 佔 11% 到 38%、建場景（兩個做法都省不到）佔 21% 到 60%。改上傳每一頁、每個 tile 都受惠（包括軟體的 adapter 和之後拆 process 的路），不用第二條 raster 的路；GPU raster 之後也需要其中的一部分（一批一次 submit、texture 的重用）。不選直接做 GPU raster 的原型（它自己在 CPU 上的成本沒有量過，圖片的 GPU 快取沒做之前只有 67% 到 100% 的 tile 走得了）。順序，每做一項重跑 `bench.py raster`：
    - 先把 Viz 的 write 那一段分開量（建 texture、`write_texture`、建 bind group）。
    - 一次 `PrepareTiles` 只 submit 一次；tile 只建它用得到的 bind group。
    - 重用 texture（cc 的 `ResourcePool`）：看分開量的結果，開始前再確認。
    - 直接畫進 wgpu 的 staging buffer、少一次複製（Chromium 的 one-copy raster）：多半不做，除非複製佔了大半。
5. **2026-11-11：重用 texture：Viz 這一側一個有上限的池子**（使用者照建議）。一批一次 submit 和 tile 只建一個 bind group 之後（`progress.md` 裡第 1a 步的那一段），上傳還佔 `PrepareTiles` 的 16% 到 24%，其中建 texture 和 bind group 約一半。
    - **什麼時候算不用了**：shared image 的最後一個引用放掉時（compositor 的 tile 和畫過它的 frame 都放了），它的 texture 連同 bind group 還回它那個 device 的池子。renderer 手上可能還有上一個 frame 的那一份，但它只畫 frame 拿著的資源。還在往螢幕上走的繪圖是在之後的寫入之前 submit 的，queue 照順序執行。
    - **取用**：`create_shared_image` 先找同樣大小的（最後放進去的那一張），有就只寫像素；沒有才建新的。
    - **上限**：32 MB、256 張，超過時放掉最舊的；單獨一張就超過 32 MB 的不留。device 遺失時池子跟著舊的 context 一起丟。數字是保守的起點，沒有量過別的值。
    - 對照 Chromium：cc 的 `ResourcePool` 在 renderer 那一側（資源經過 `ReturnResources` 還回來），照大小和格式重用、有記憶體上限、閒置一段時間的釋放。machopo 放在 Viz 這一側是因為現在沒有 `ReturnResources`（資源的生命是引用計數）；拆 process 時跟著那一條例外一起改。沒有做閒置逾時：池子最多多佔 32 MB。
    - 不選不做（上傳已經少了四成，但建 texture 是剩下的一半）；不選沒有上限（改視窗大小會留下很多再也用不到的大小）。

## 待決定

改完上傳、重量之後要定的：

- **要不要做 GPU raster 的原型**（第 2 條）。GPU raster 的 texture 要能當 render target，池子那時要跟著改。
- **GPU raster 的程式放在哪裡**：A（Viz 那一側、經過介面）或 B（`raster` 直接拿 device）。看法：A。
- **測試預設走 CPU**，GPU 的路靠旗標強制。看法：是。

決定做 GPU raster 的話，原型（第 2 步）開始前或看過它的結果再定的：

- 同一個 layer 裡 CPU 和 GPU 的 tile 混不混用。
- thread 的模型（a、b、c）。
- 原型要快多少才算通過。
- 旗標的名字和預設值（做完時預設開或關）。
- 圖片的 GPU 快取的預算。
- 強制 GPU 的 reftest 和情境跑哪些、CI 上跑不跑。

## 之後的可能

- canvas 2D 的 GPU 路徑（不讀回的 canvas）。
- 離屏的項目（filter、mask、圖案）在 GPU 上畫；上游有了 mask layer 之後 `ClipPaths` 也走 GPU。
- 不經過 tile 的快速路徑。
- async raster：GPU raster 之後上傳不再卡 compositor thread，但覆蓋率還是；tile 的優先順序和 checkerboard 是另一份 RFC。
- 拆 process 之後的 OOP-R：display list、字型和圖片能序列化時，A 的介面變成跨 process 的。
