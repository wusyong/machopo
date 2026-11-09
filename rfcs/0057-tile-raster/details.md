# RFC 0057 的細節

## 捲動和改視窗大小時的量測

### 方法

2026-10-18，`main` 的 `85f27c57`（RFC 0053 之後：Rust 那一側用 mimalloc），macOS 26.6.2，Apple M4（10 核），接電源。`perf` profile 的 `kun --automation-pipe --window`（真的視窗，Viz 用 wgpu），錄 `MACHOPO_TRACE`。scratchpad 裡的一個 Python 腳本借 `scripts/speedometer.py` 的 `CdpPipe`、`Server` 和 trace 的 reader：

1. 視窗設成 1280 × 800（`Browser.setWindowBounds`），開頁面，`Kun.waitForLoad`，等 2 秒。
2. **捲動**：`Input.dispatchMouseEvent` 的 `mouseWheel`，`deltaY` 100 px，在 (600, 400)，送 120 次，每次之間睡 16 ms；之後等 0.5 秒。
3. 等 1 秒。
4. **改大小**：視窗從 1280 × 800 一步一步縮到 680 × 500（41 個大小），再放回來，每步之間睡 16 ms；之後等 0.5 秒。
5. 兩段的起訖用 Python 讀 `CLOCK_UPTIME_RAW`（macOS 上 Perfetto 的時鐘），從 trace 算那一段裡：
   - Compositor thread 上 `TileManager::PrepareTiles` 的次數、合計、最長；它裡面的 `raster::playback`；`LayerTreeHostImpl::DrawLayers`。
   - renderer main thread（`CrRendererMain`）上 `ProxyMain::BeginMainFrame` 的次數、合計、最長。
   - Viz thread 上 `Display::DrawAndSwap` 的間隔。

頁面：`tests/sites/` 的 Gutenberg（`pg28885-images.html`，長篇文字加 42 張圖）和 SQLite（`about.html`），以及 Speedometer 的 NewsSite-Next（`../third_party/speedometer/resources/newssite/news-next/dist/index.html`，由 `Server` 提供）。

**frame 的間隔不能用**：`Input.dispatchMouseEvent` 等 renderer 處理完才回覆，加上睡的 16 ms，一個滾輪事件一來一回約 37–47 ms，frame 只在有新的捲動時產生，間隔被送事件的節奏綁住（Gutenberg 捲動時 p50 18.2 ms、p95 31.5 ms，NewsSite p50 41.3 ms，都跟著事件）。所以判斷用 Compositor thread 上每次更新的時間；frame 的間隔要等工具能用固定的節奏送 input（例如 compositor 自己的捲動動畫）再量。

### 結果

| 頁面 | 動作 | 時間 | `PrepareTiles`：次數，合計（最長） | `raster::playback`：次數，合計 | `DrawLayers` 合計 | main frame：次數，合計（最長） |
| --- | --- | --- | --- | --- | --- | --- |
| Gutenberg | 捲動 | 4.5 s | 240，932 ms（15.2 ms） | 940，836 ms | 2.0 ms | 120，6.7 ms（0.2 ms） |
| Gutenberg | 改大小 | 2.3 s | 22，1648 ms（115 ms） | 3322，1428 ms | 0.1 ms | 22，260 ms（20.2 ms） |
| SQLite | 捲動 | 2.8 s | 6，0.1 ms | 0 | 0.2 ms | 3，0.5 ms |
| SQLite | 改大小 | 2.3 s | 57，520 ms（22.9 ms） | 4083，225 ms | 0.5 ms | 57，36 ms（1.9 ms） |
| NewsSite-Next | 捲動 | 5.7 s | 130，1403 ms（109 ms） | 14654，999 ms | 12.7 ms | 130，49 ms（2.5 ms） |
| NewsSite-Next | 改大小 | 2.3 s | 11，1796 ms（260 ms） | 12527，1432 ms | 0.5 ms | 11，75 ms（14.2 ms） |

讀法：
- **改大小**時內容更新的次數（`PrepareTiles` 的次數：11–57 次）遠少於改大小的步數（82 步）：每次更新的 raster 太久，中間的大小被跳過。每次更新 Gutenberg 平均 75 ms、NewsSite 163 ms；raster 的時間是 main frame 的 6 倍（Gutenberg）到 24 倍（NewsSite），SQLite 是 14 倍。
- `PrepareTiles` 比它裡面的 `raster::playback` 多出來的（例如 NewsSite 改大小時 1796 對 1432 ms）是 tile 的上傳、建 `TileDrawInfo` 和掃像素判斷單色。
- **捲動**：Gutenberg 每次平均 3.9 ms，不是問題；SQLite 的頁面很短，幾乎不用 raster；NewsSite 每次平均 10.8 ms、最長 109 ms，吃掉大部分的 frame 預算，而且每次 raster 約 113 個 tile（下一節）。

### NewsSite 捲動時每一步 raster 的 tile

同一個頁面再跑一次，只捲 20 步（每步之間睡 50 ms），`RUST_LOG` 開 `compositor=debug`、`paint=debug`、`layout=debug`，看 `prepare_tiles` 的 debug log（`prepared tiles rastered=… solid=… kept=…`）：

```
prepared tiles rastered=180 solid=171 kept=2985
prepared tiles rastered=180 solid=160 kept=3165
prepared tiles rastered=180 solid=178 kept=3345
...
prepared tiles rastered=180 solid=171 kept=4785
prepared tiles rastered=280 solid=256 kept=4505   （這一步前面有一次 main frame 的 paint：painted=588）
```

- 每一步 raster 180 個 tile，其中 158–178 個（約 90–99%）raster 完是單色的。
- `kept` 每一步多約 180，到四千多個 tile：預先 raster 的範圍（可見範圍往四周各擴一個畫面的寬或高，`visible.inset(-max(w, h))`）裡的 tile 很多。
- tile 是 256 × 256（`LayerTreeSettings::default_tile_size`）。一個 layer 往下捲 100 px，新進預先 raster 範圍的 tile 頂多一兩排（範圍寬 3 × 1280 px，一排約 15 個）；一步 180 個，表示有很多個 layer 各自進了 tile。

### 每個 layer 的 tile

在 `prepare_tiles` 暫時加一行 debug log（每個 layer 的大小、是否不透明、op 數、display list 的範圍、raster 了幾個 tile、幾個是單色；查完就還原，沒有留在程式裡），同樣捲 NewsSite：

| layer | 大小（CSS px） | 不透明 | op 數 | display list 的範圍（x、y、寬、高） | 每一步 raster | 其中單色 |
| --- | --- | --- | --- | --- | --- | --- |
| 3 | 3764 × 7709.5 | 否 | 133 | 0、118、1280、7591.5 | 20 | 20 |
| 5 | 3764 × 7709.5 | 否 | 19 | 8、186、932、611.7 | 20 | 20 |
| 7 | 3764 × 7709.5 | 否 | 20 | 324、186、932、617.2 | 20 | 20 |
| 9 | 3764 × 7709.5 | 否 | 56 | 4、222、1266.8、1215.2 | 20 | 20 |
| 11 | 3764 × 7709.5 | 否 | 16 | 426.7、867.5、829.3、576.7 | 20 | 20 |
| 13 | 3764 × 7709.5 | 否 | 227 | 8、903.5、3756、3903.5 | 20 | 0–11 |
| 15 | 3764 × 7709.5 | 否 | 139 | 4、4042.5、1269.7、1165.7 | 20 | 20 |
| 17 | 3764 × 7709.5 | 否 | 20 | 432.6、4820.5、823.4、411.7 | 20 | 20 |
| 19 | 3764 × 7709.5 | 否 | 379 | 0、0、2897.6、6935.1 | 20 | 20 |

- 9 個 layer 都和整頁一樣大、都透明：這就是「每個成為 layer 的容器把頁面切成之前、容器、之後」（`docs/limitations.md` 的「Compositor、layer 和 input」）。每個 layer 每一步 20 個 tile，加起來 180 個。
- 大部分 layer 真正有內容的範圍（display list 的 `bounds`）很小，捲到的 tile 都在範圍外面，raster 出來全透明。只有內容寬到 3756 px 的 layer 13 有不是單色的 tile。
- 所以 raster 之前先看 tile 和 display list 的範圍相不相交（`DisplayItemList` 已經存著這個範圍），就能省掉 NewsSite 捲動時大部分的 raster（README 的 B0）。

## 整個 Speedometer 的 trace 裡的 raster

2026-10-18 同一台機器，`ci.py speedometer --trace --iterations 3`（20 個 default suite），整份 trace（包括載入頁面）裡 `raster::playback` 和 `raster::draw_into` 的 self time：

| thread | 什麼 | 合計 |
| --- | --- | --- |
| Compositor | 網頁的 tile（`raster::playback`） | 5131 ms |
| main（shell 的 UI thread） | browser 自己的 tile（分頁列、工具列……，`raster::playback`） | 756 ms |
| CrRendererMain | canvas 2D（`raster::draw_into`） | 168 ms |

tile 的 raster 是 CPU 上很大的一塊，但不在 Speedometer 計時的 main thread 上（`rfcs/0012-speedometer/progress.md` 的「tile 的 raster 算不算進 Speedometer 的時間（2026-10-18，Windows 11）」）。

## 程式的現況

（`main` 的 `85f27c57`）

- **`compositor_paint::DisplayItemList`**：`ops: Vec<PaintOp>`、`bounds: RectF`（全部 op 的聯集）、`finalized`。`push(op, visual_rect)` 的 `visual_rect` 只拿來擴 `bounds`；狀態 op 傳 `None`。`push_list_at` 把子 list 的 op 包在 `Save`、`Translate`、`Restore` 裡接上，子 list 的範圍只用來擴 `bounds`。
- **`raster::playback`**（`crates/raster/src/skia_backend.rs` 的 `playback`）：每個 tile 建一個 `sk::Canvas::from_raster_direct`，`clear`、`translate`、`scale`，然後 `for op in list.ops() { draw_op(...) }`：整份 display list 播一次。Skia 會對每個畫的 op 做 quick reject（不在 clip 裡就不畫），但每個 op 照樣要轉成 Skia 的呼叫（建 path、paint……）。
- **`LayerTreeHostImpl::prepare_tiles`**（`crates/compositor/src/layer_tree_host_impl.rs`）：在 Compositor thread 上，對每個 layer 算預先 raster 的範圍，丟掉範圍外的 tile、補上缺的，對每個沒有 `draw_info` 的 tile 同步呼叫 `raster::playback`，再用 `TileDrawInfo::solid_color` 掃一遍像素：全部一樣而且不透明或全透明就是 `SolidColor`，否則上傳到 Viz（`shared_image_interface`）。
- **`prepare_tiles_if_scrolled`** 的註解寫明了同步的選擇：cc 先 draw、沒 raster 好的地方畫 checkerboard；kun 在 draw 之前 raster，捲進來的內容一定在。
- **invalidation**：commit 時 `PictureLayerTiling::keeping` 照 layer 的 `invalidation` 留下沒被碰到的 tile（cc 的 `PictureLayer` invalidation），所以內容沒變的部分不會重 raster。
- **`RasterCache`**：`typefaces: HashMap<(u64, u32), sk::Typeface>`、`images: HashMap<u64, (ImageBytes, sk::Image)>`。skia-safe 0.153.3 對 `Typeface`、`Image` 都標了 `unsafe_send_sync!`（`src/core/typeface.rs`、`src/core/image.rs`）。`Image::from_encoded` 是延遲解碼，第一次畫時解。
- **依賴**：`crossbeam-channel` 在 workspace 裡；`rayon` 1.12 只因為 wasmtime 而在依賴樹裡；沒有 R-tree 的 crate。

## 和 Chromium 的對照

| | Chromium（cc） | kun 現在 | 這份 RFC |
| --- | --- | --- | --- |
| op 的範圍 | `DisplayItemList` 記每個 op 的 visual rect；`StartPaint` / `EndPaintOfPairedBegin` / `EndPaintOfPairedEnd` 讓 save、clip、transform 的範圍是到對應的結尾之間的聯集 | 只記全部的聯集 | 每個 op 都記；狀態 op 在 `finalize` 時算聯集 |
| 空間索引 | `Finalize` 建 `cc::RTree`（`cc/base/rtree.h`，靜態、一次建好） | 無 | 靜態的 R-tree（A1） |
| 播放 | `DisplayItemList::Raster` 用播放的範圍查 R-tree，`PaintOpBuffer::Playback` 只播查到的 op（offsets） | 整份 | 只播查到的 |
| 單色的 tile | raster 之前 `RasterSource::PerformSolidColorAnalysis` → `DisplayItemList::GetColorIfSolidInRect` → `SolidColorAnalyzer`（只分析很少幾個 op）；單色的 tile 不 raster，畫成單色的 quad | raster 完掃像素 | raster 之前判斷（B1），判斷不出來照舊 |
| raster 的 thread | `cc::CategorizedWorkerPoolJob`：process 共用的 `base::ThreadPool` 上的 job，前景（`USER_BLOCKING`）同時最多 `num_raster_threads` 個，預設 1（`--num-raster-threads`），背景 1 個；`TileManager` 排優先順序 | Compositor thread 一條 | rayon 的專用 pool（C1） |
| 同步或非同步 | 非同步：draw 時沒好的 tile 畫 checkerboard 或低解析度；activation 只等「需要的」tile | 同步 | 同步（C1） |
| 預先 raster 的範圍 | 依捲動方向和速度（skewport）、tile 的優先順序和記憶體的預算 | 可見範圍往四周各擴一個畫面 | 不變 |
| GPU raster | 預設開（macOS 的 Graphite、Windows 和 Linux 的 Ganesh 經過 ANGLE；`rfcs/0007-gpu-raster/details.md` 的「Chromium 各平台的 GPU 設定」） | CPU（Skia 的 raster 後端） | 不變（RFC 0007） |

Chromium 的 raster thread 數目（2026-10-18 讀 Chromium `main` 的 `cc/raster/categorized_worker_pool.cc`）：`CategorizedWorkerPool::GetOrCreate` 的 `num_raster_threads` 預設 1，只有命令列的 `--num-raster-threads` 能改；`CategorizedWorkerPoolJob::Start` 用它當前景 job 的最大並行數，背景 job 最多 1。草稿起先寫的「核心數的一半、1 到 4 條」是舊的記憶，不對。Chrome 預設 GPU raster（OOP-R），raster worker 只錄製、序列化 paint op，像素在 GPU process 畫，所以 1 個就夠；kun 是 CPU raster，不能照搬。

## 和換 raster 函式庫的關係

- 換成 Vello（RFC 0007 的 vello_cpu、vello_gpu）只改 `raster` 這個 crate 裡畫的部分（Skia 只在 `raster` 裡：`skia_backend.rs` 約 830 行程式、`encode.rs` 約 95 行）。tile 怎麼選、每個 tile 播哪些 op、單色的判斷、在哪條 thread 上，都在 `compositor` 和 `compositor_paint`，換函式庫不會改到。
- 所以這份 RFC 和 RFC 0007 不衝突：先做這份，畫像素的時間在 raster 的時間裡佔多少就清楚了，再決定值不值得換。
