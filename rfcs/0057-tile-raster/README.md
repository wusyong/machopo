# RFC 0057：tile 的 raster：空間索引、單色 tile、raster worker

- 開始日期：2026-10-18
- 狀態：✅ 已完成：2026-10-18（layer 的 display list 記每個 op 的範圍、`finalize` 時建靜態的 R-tree，每個 tile 只播碰到它的 op；raster 之前從 op 判斷單色的 tile，不 raster；raster worker 是 rayon 的 pool，維持同步，量過 Speedometer 之後預設不開，`--num-raster-threads` 打開；`ci.py raster` 量；在 macOS 上做）
- 細節：`details.md`（一開始的量測、NewsSite 每一步 raster 的 tile、和 Chromium 的逐項對照）
- 進度和驗收：`progress.md`

## 摘要

改視窗大小時，kun 每次內容更新要在 Compositor thread 上 raster 75 到 260 ms（Apple M4，Gutenberg 和 Speedometer 的 NewsSite-Next），NewsSite 捲動時九成以上 raster 的 tile 是單色的空白。慢的原因在 kun 的 raster 流程，不在 Skia：每個 tile 都把整份 display list 從頭播一次，單色的 tile 先畫完再掃像素判斷，所有 tile 在一條 thread 上排隊。這份 RFC 照 Chromium 的 cc 做了三件事：
- layer 的 display list 記下每個 op 的範圍、建 R-tree，每個 tile 只播碰到它的 op；
- raster 之前從 op 判斷單色的 tile（沒有 op 畫到它，或一個不透明的填色蓋滿它），不 raster；
- tile 可以交給一組 raster worker 平行畫。

和開始時比（同一個時段，headless，device scale factor 2）：Gutenberg 捲動時的 raster 少了 81%、改大小時每次更新少了 89%；NewsSite 捲動少了 40%、改大小每次更新少了 24%（開 9 條 raster worker 時少了 82%）。raster worker 讓 tile 多的頁面改大小快好幾倍，但頁面跑的時候它們和 main thread 搶 CPU、多用記憶體（Speedometer 時間約多 1%、記憶體依頁面多到兩成），所以預設不開。

## 動機

**一開始量到的**（2026-10-18，`details.md` 的「捲動和改視窗大小時的量測」）：真的視窗、`perf` profile，Compositor thread 上的 `TileManager::PrepareTiles`（裡面是 raster）和 renderer main thread 的工作：

| 頁面 | 動作 | 內容更新 | `PrepareTiles`（平均、最長） | main thread | raster 的 tile |
| --- | --- | --- | --- | --- | --- |
| Gutenberg | 改大小 | 22 次 | 1648 ms（75、115 ms） | 260 ms | 3322 |
| NewsSite-Next | 改大小 | 11 次 | 1796 ms（163、260 ms） | 75 ms | 12527 |
| NewsSite-Next | 捲動 | 130 次 | 1403 ms（11、109 ms） | 49 ms | 14654 |
| Gutenberg | 捲動 | 240 次 | 932 ms（3.9、15 ms） | 7 ms | 940 |

- **改視窗大小**：raster 是明確的瓶頸，重的頁面縮放時內容一秒只更新 5 到 10 次。
- **NewsSite 捲動**：每往下捲一步 raster 180 個 tile，其中約 170 個是單色的（`details.md` 的「NewsSite 捲動時每一步 raster 的 tile」）。它們來自 9 個和整頁一樣大的透明 layer，大部分的 tile 在 layer 真正有內容的範圍外面（`details.md` 的「每個 layer 的 tile」）。

**原因**（`details.md` 的「程式的現況」）：
- **每個 tile 播整份 display list**：`DisplayItemList` 只存 op 和全部 op 的聯集範圍，成本是「tile 數 × op 數」，空白的 tile 一樣貴。Chromium 的 `cc::DisplayItemList` 有 R-tree，一個 tile 只播碰到它的 op。
- **單色的 tile 先畫再判斷**：raster 完才用 `TileDrawInfo::solid_color` 掃像素。cc 在 raster 之前從 display list 判斷（`SolidColorAnalyzer`）。
- **一條 raster thread**：cc 有 raster worker（`CompositorTileWorker`）。

這幾件事換成 Vello 或 GPU raster 都還在：那些只讓「畫像素」變快（RFC 0007、`details.md` 的「和換 raster 函式庫的關係」）。tile 的 raster 不在 Speedometer 的計時區間裡（`rfcs/0012-speedometer/progress.md` 的「tile 的 raster 算不算進 Speedometer 的時間（2026-10-18，Windows 11）」）：這份 RFC 改善的是捲動、改視窗大小時的反應。

**不在範圍內**：
- 非同步的 raster（先畫已經好的 tile，沒好的畫 checkerboard，cc 的做法）：kun 刻意選的同步 raster 不變（「理由和其他做法」）。
- 換 raster 的函式庫、GPU raster（RFC 0007）。
- layer 怎麼切（`docs/limitations.md` 的「Compositor、layer 和 input」）：這份 RFC 讓那些大片透明 layer 的 tile 變便宜，不改切法。
- canvas 2D 的 raster（main thread 上的 `raster::draw_into`）。
- tile 的大小、預先 raster 的範圍。

沒有要解決的 `docs/limitations.md` 裡的限制。

## 設計概要

網頁的行為和畫面不變。kun 的使用者感覺得到的：改視窗大小時內容跟得比較快，捲動時比較順；`kun --num-raster-threads N` 讓 tile 多的頁面再快好幾倍。

給之後改這部分程式的人：
- **`visual_rect` 要保守**：`DisplayItemList::push` 收到的範圍會留下來，tile 照它找要播的 op；畫的 op 給的範圍不能比它畫的小（`push_op` 從幾何推，在 transform 底下要自己給 layer 空間的範圍）。沒給範圍的畫圖 op 當成到處都畫。
- **狀態 op 的範圍由 `finalize` 算**：`Save`、clip、transform 的範圍是它管到的畫圖 op 的聯集。
- **只有 layer 的 list 建 R-tree**：`DisplayItemList::new()` 是 layer 的，`record()` 是整份播的（canvas、SVG 圖片、畫進另一份 list 的）。
- **raster 的流程**：`raster::ops_for` 查一個 tile 要播的 op；`DisplayItemList::color_if_solid_in_rect` 判斷它是不是單色；`raster::playback_into` 畫進可以重複用的 buffer。
- **`prepare_tiles` 是同步的**：draw 之前 tile 都好了。開了 raster worker 時，它把 tile 交給 pool，等全部做完，再照順序編 resource id，所以幾條 thread 畫出來都一樣。

## 詳細設計

### 1. display list 的空間索引

**op 的範圍**（第 6 條）：`DisplayItemList` 多存一份和 `ops` 一樣長的 `visual_rects`（layer 的空間，CSS px）。
- **畫的 op**：是給它的範圍；沒給的是「到處」。
- **狀態 op**（`Save`、`Restore`、`Translate`、`Concat`、`SetMatrix`、clip、`SaveLayer*`）：在 `finalize` 時設成它管到的畫圖 op 的聯集。`Save` 和它的 `Restore` 是中間所有東西的聯集，巢狀的算進外層（cc 的 paired begin、paired end）；transform 和 clip 是從它到所在範圍結束；沒配對的 `Save` 到 list 的結尾。這樣查到任何一個畫圖 op 時，它所在的狀態一定也被查到，播放時的狀態就對得上。前面沒有 `Save` 的 `Restore` 不播（Skia 對它什麼都不做）。
- **帶 filter 或 blend mode 的 `SaveLayer`**：裡面的每個 op 都是「到處」。drop shadow 會畫到內容外面，`Src`、`Clear` 會清掉透明的地方。
- **`push_list_at`**：把子 list 的範圍移過來。
- **計算方式**：`finalize` 走兩次。往前一次標出這種 group，往後一次算每個範圍畫了什麼。

layer 的 list 的範圍來自 paint 的 `visual_rect`（換到 layer 的空間）：raster invalidation 和 layer 的大小早就靠它，所以本來就要保守（`progress.md` 的「做過的檢查」逐一確認過）。

**索引**（第 1、2、8 條）：

| | 做法 | 優缺 | 對應 Chromium |
| --- | --- | --- | --- |
| **A1（採用，第 1 條）** | 自己寫靜態的 R-tree（`compositor_paint` 的 `rtree`）：`finalize` 時用 STR 一次建好，每個節點最多 11 個子節點、自己記子節點的範圍；查一個矩形回傳 op 的索引，照順序 | 約兩百行，不加依賴；display list 建好之後不變 | `cc::RTree`，`DisplayItemList::Finalize` 建、`Raster` 查 |
| A2 | 用 `rstar` crate | 動態的 R*-tree，比需要的重，多一個依賴 | 無 |
| A3 | 只照 paint chunk 的範圍跳過整段 | 大的 chunk 照樣整段播，`DisplayItemList` 也沒留 chunk 的邊界 | 無 |

- **什麼時候建**：索引在 main thread 的 `finalize` 建（第 2 條）。一份 display list 給很多 tile 查，建一次就夠。
- **哪些 list 建**：只有 layer 的 list 建（`UsageHint::TopLevel`，第 8 條，和 cc 的 `kTopLevelDisplayItemList` 一樣）。canvas 每次 flush 的、SVG 圖片的、分頁列和工具列畫進 Browser 的 layer 的 list 是 `record()`，不建；`ops_in` 對它們回傳全部的 op。

**播放**：`raster::ops_for` 用 tile 的範圍查索引。查詢的範圍外擴一個裝置像素，給反鋸齒，再換回 CSS px。`playback_ops`、`playback_into` 只播查到的 op；播放的狀態只靠 op 本身，所以逐像素和播整份的相同。

### 2. raster 之前判斷單色的 tile

| | 做法 | 優缺 | 對應 Chromium |
| --- | --- | --- | --- |
| B0（第 2 段，第 3 條；第 3 段由索引取代，第 7 條） | tile 和整份 list 的範圍不相交，就是清除色的單色 tile | 不需要索引；但兩塊內容之間、聯集裡的空白 tile 照樣 raster | 無 |
| **B1（採用，第 3、7、9 到 11 條）** | `DisplayItemList::color_if_solid_in_rect`：照順序走 tile 查到的 op，記著 transform、clip 和到目前為止的顏色（從清除色開始）。查不到 op 就是清除色（B2）；幾何碰不到 tile 的 op 當沒畫；蓋滿 tile 的不透明填色讓顏色變成它的；認不得的 op 就放棄，照舊 raster、照舊掃像素 | 判斷保守，錯的話只是多 raster 一次 | `RasterSource::PerformSolidColorAnalysis` → `DisplayItemList::GetColorIfSolidInRect` → `SolidColorAnalyzer` |
| B3 | 照舊：raster 完再掃像素 | 空白的 tile 照樣付全部的成本 | 無 |

**B1 認得的**（第 9 條，照量到的 NewsSite 的單色 tile 選的：63% 是一個蓋滿的不透明矩形，另外約 10% 是被 clip 切掉、或只有保守範圍碰到 tile 的；`progress.md` 的「做過的檢查」）：
- **transform**：`SetMatrix`、`Translate`、`Concat`，只認平移和縮放。
- **clip**：`ClipRect`，和是矩形的 `ClipRRect`。
- **蓋滿 tile 的填色**：填色的 `DrawRect`（不透明，沒有 shader、模糊、虛線，blend mode 是 `SrcOver` 或 `Src`）和 `DrawColor`（`SrcOver` 或 `Src`）。蓋滿是用裝置像素精確比，邊剛好落在 tile 的邊上也算，頁面最底層的背景就是從 0 開始的。
- **碰不到 tile 的判斷**：用 op 自己的幾何（`op_bounds`），換到裝置像素、切掉 clip。影子多算一個像素，Skia 的模糊可能略超過 3σ。

認不得、碰到 tile 就放棄的：文字、圖片、路徑、圓角矩形、group、圓角或路徑的 clip、旋轉。半透明的疊色不做（P2），因為捨入要和 Skia 逐像素一樣，而 macOS 和其他平台不同；文字墨水的精確範圍也不做（P3），因為要查字型。不設要走幾個 op 的上限（第 10 條）：有文字的 tile 在第一個字形就放棄。

清除色是半透明時，判斷不出顏色，照舊 raster：和 raster 之後掃像素一樣，半透明的不算單色。

**檢查**（第 11 條）：debug build 裡判斷是單色的 tile 照樣 raster 一次，`debug_assert` 比對顏色。reftest、WPT 和情境都用 debug build 跑，等於拿真的網頁再檢查一次。

### 3. raster worker

| | 做法 | 優缺 | 對應 Chromium |
| --- | --- | --- | --- |
| **C1（採用，第 4、12 到 15 條）** | `prepare_tiles` 把要 raster 的 tile 交給 rayon 的專用 pool 平行畫，等全部做完：還是同步的，draw 之前 tile 都好了 | 改動集中在 `prepare_tiles`；tile 多（改大小）時加速接近 thread 的數目 | cc 的 raster 也交給 thread pool，但 cc 是非同步的 |
| C2 | 非同步的 raster：先畫已經好的 tile，沒好的畫 checkerboard | 最像 cc；但要 tile 的優先順序、activation 的條件、記憶體的預算，等於重寫 `prepare_tiles` 和它的呼叫的人 | `TileManager`、`TaskGraphRunner` |

**`prepare_tiles` 分三步**：
1. 在 compositor thread 上走過每個 layer，查 op、判斷單色，把要 raster 的 tile 照畫的順序收成 `RasterJob`。
2. 交給 pool，或在 compositor thread 上一個一個做。每個 tile 在畫它的 thread 上做完：raster 進那條 thread 重複用的 buffer（`TILE_PIXELS`），掃單色，有 GPU 就從 buffer 上傳，沒有就把像素複製出來留著。
3. 在 compositor thread 上照順序編 resource id。

幾條 thread 結果都一樣（測試守這一點）。第 2 步原本把整次更新的 bitmap 收齊再上傳，第 6 段量出記憶體多了才改成這樣（第 16 條）。

**pool**：
- **誰有**（第 4、12 條）：每個 renderer 的 `LayerTreeHostImpl` 一個 rayon 的 `ThreadPool`，thread 叫 `CompositorTileWorker1` 起，第一次有兩個以上的 tile 要 raster 時才建。和 Chromium 一個 renderer process 一組 raster worker 一樣，守住 process 的邊界，拆 process 時不用改。Browser 的 UI 的 compositor 不建。
- **不用的**：不用 rayon 的全域 pool（會和 wasmtime 編譯 wasm 搶）；也不自己寫一組（等於重做 rayon）。
- **條數**（第 13、15 條）：條數由 `kun --num-raster-threads N` 決定，經過 `RendererFlags` 到 `LayerTreeSettings`，和 Chromium 的開關同名。**預設 1，不建 pool**（第 15 條）：

  | | 預設 1 條 | 9 條 |
  | --- | --- | --- |
  | NewsSite 改大小，每次更新 | 103 ms | 25 ms |
  | Speedometer 的時間（對第 4 段） | 沒有退步 | 約 +1% |
  | Speedometer 的記憶體 | 沒有退步 | 依頁面多到兩成 |

  9 條時每條 thread 畫一個 tile 的 CPU 時間是一條時的 1.8 到 8 倍：這台的 10 個核心有 6 個是節能核心，還要和 main、compositor、Viz 搶核心。Chromium 的 renderer 預設也只有 1 條，但它只錄製，像素在 GPU 上畫。

**快取**（第 14 條）：所有 raster thread 共用 compositor 的一份 `RasterCache`。字型和圖片的表放在 `Mutex` 裡，只在查找和加入時拿鎖，畫的時候不拿，所以每張圖片只解碼一次。

### 4. 量測的工具

`ci.py raster`（`scripts/raster.py`，第 5 條）在 kun 裡捲動和改視窗大小，用的頁面是 `tests/sites/` 的 Gutenberg、SQLite 和 Speedometer 的 NewsSite-Next。
- **怎麼跑**：kun 用 `perf` profile，預設 headless、device scale factor 2；`--window` 用真的視窗，`--raster-threads` 交給 kun 的 `--num-raster-threads`。
- **讀什麼**：
  - 從 `MACHOPO_TRACE`：每一段的 `PrepareTiles`、`raster::playback`（含 raster thread 的：它們的名字在 trace 裡截成 15 個位元組，照 track 分）和 main frame；
  - 從 compositor 的 debug log：raster 了幾個 tile、幾個是單色、幾個沒 raster。
- **報告**：在 `target/raster-results/raster.md`。
- **不報 frame 的間隔**：滾輪事件要等 renderer 處理完才回覆，frame 跟著送事件的節奏，量不準。

### 測試

驗收的項目和指令在 `progress.md`。
- **索引不改變結果**：`raster` 的隨機測試產生 1000 份 display list（巢狀的 save、group、drop shadow、clip、平移縮放和旋轉、各種畫的 op、沒帶範圍的 `DrawColor`、沒關的 save），配上隨機的 tile、三種縮放，比對只播查到的 op 和播整份的像素；判斷是單色的 tile，每個像素都要是那個顏色。
- **單元測試**：狀態 op 的範圍、R-tree、單色的判斷、raster thread 和 compositor thread 畫出來一樣、共用的快取、重複用的 buffer。
- **工具的測試**：`ci.py raster` 用 dev profile 的 kun 跑過一次，2 條 raster thread 和預設各一次。
- **mutation test**：六份清單，`mutants-1.toml` 到 `mutants-6.toml`。

### 分階段

1. **量測工具和基準**：`ci.py raster`。
2. **內容範圍外的 tile 不 raster**（B0）。
3. **空間索引**：`visual_rects`、狀態 op 的範圍、R-tree、只播查到的 op；查不到 op 的 tile 不 raster，取代 B0。
4. **單色的 tile**：B1。
5. **raster worker**：C1。
6. **量測和決定**：同一個時段重量基準、真的視窗、Speedometer；查到第 5 段讓 Speedometer 退步，改成每個 tile 在它的 thread 上做完、重複用 buffer，預設改成 1 條。

## 缺點

- **display list 變大**：每個 op 多一個 `RectF`（16 bytes），layer 的 list 加上索引；main thread 的 `finalize` 多建一棵樹，第 6 段的 Speedometer 沒有量到差別。
- **op 的範圍要正確**：漏算陰影、filter 擴出去的範圍，那個 tile 就少畫東西。隨機測試只涵蓋產生器會產生的 op；`push_op` 在 transform 底下會給錯範圍，要自己給。
- **單色的判斷要和 raster 一致**：判斷錯的話畫面錯，只在保守的條件下判斷，debug build 照樣比對。
- **raster worker 的效率不高**：見上面的「條數」，所以預設不開；開了每個分頁一組 thread。

## 理由和其他做法

**為什麼先做這三件，不先換函式庫或做 GPU raster**：時間主要花在每個 tile 播整份 display list 和 raster 空白的 tile，這些不管用什麼畫都還在（`details.md` 的「和換 raster 函式庫的關係」）。這三件都在 kun 自己的程式裡，也是 Chromium 本來就有的結構；之後做 GPU raster 時，換掉的只是 `raster` 把 op 畫成像素的那一步，索引、單色的判斷和 worker 都留著。

**為什麼維持同步的 raster（C1，不是 C2）**：kun 選了同步（`prepare_tiles_if_scrolled` 的註解：raster 在 draw 之前做完，捲進來的內容一定在），換來沒有 checkerboard，測試和情境也不用處理「tile 還沒好」。量過之後，NewsSite 這類頁面改大小時每次更新在預設下仍然約 100 ms，主要是同步 raster 了整個 interest rect：可見範圍往四周各擴出一個畫面，大部分 tile 在畫面外。先畫可見的 tile 是之後的方向（「之後的可能」）。

**為什麼 raster worker 預設不開**（第 15 條）：見「條數」。Speedometer 跑的是頁面的 script，raster 不在計時區間裡，但 9 條 raster thread 照樣和 main thread 搶核心，每條 thread 也有自己的 allocator heap。第 6 段試了三件事：
- 查字型改成每條 thread 自己記著，不拿鎖：沒有效果，拿掉了；
- 每個 tile 在它的 thread 上做完，不收齊：記憶體少了一些；
- 重複用 buffer：時間的退步從 +2.0% 到 +1.4%。

剩下的代價沒有消掉。4 條和 9 條在 Speedometer 上分不出差別。

## 先例

- **Chromium**（`details.md` 的「和 Chromium 的對照」）：
  - **R-tree**：`cc::DisplayItemList` 記每個 op 的 visual rect，paired begin（save、clip、transform）的範圍是到對應的 paired end 之間的聯集；`Finalize` 建 `cc::RTree`，只有 `kTopLevelDisplayItemList` 建；`Raster` 用播放的範圍查，只播查到的 op。
  - **單色的判斷**：`RasterSource::PerformSolidColorAnalysis` 用 `DisplayItemList::GetColorIfSolidInRect`（`SolidColorAnalyzer`）判斷，單色的 tile 不 raster，畫成單色的 quad。
  - **raster worker**：raster 非同步做，在 thread pool 上跑。renderer 的前景 raster 預設同時 1 個（`--num-raster-threads` 可以改），因為 GPU raster 時 worker 只錄製。
- **WebRender**（Firefox）：picture cache 的 tile 也只重畫碰到它的 primitive，在 GPU 上畫。不照它：kun 的結構是 cc 的。

## 已決定的事項

2026-10-18 接受。

1. **空間索引是自己寫的靜態 R-tree（A1）**（2026-10-18，使用者）：`finalize` 時一次建好（STR 的 bulk load），和 Chromium 的 `cc::RTree` 一樣。不選 `rstar`（動態的 R*-tree，比需要的重，多一個依賴）；不選照 paint chunk 跳過（A3：大的 chunk 照樣整段播，`DisplayItemList` 也沒留 chunk 的邊界）。
2. **索引在 main thread 的 `finalize` 建**（2026-10-18，使用者）：cc 的做法，一份 display list 給很多 tile 查，建一次就夠。量測顯示 main thread 的成本明顯時，再改成 Compositor thread 第一次 raster 時建。
3. **單色的 tile：先做 B0，有了索引再做 B1**（2026-10-18，使用者）：B0 是 tile 和 display list 的範圍不相交就不 raster（NewsSite 的空白 tile 大多在 layer 的內容範圍外，`details.md` 的「每個 layer 的 tile」）；B1 是索引查到的很少幾個 op 裡有蓋滿 tile 的不透明矩形時也不 raster（cc 的 `SolidColorAnalyzer`）。判斷不出來照舊 raster、照舊掃像素。
4. **raster worker 是 rayon 的專用 `ThreadPool`，維持同步（C1）**（2026-10-18，使用者提議 rayon）：`prepare_tiles` 交給 pool、等全部畫完；thread 取名 `CompositorTileWorker`，條數第 5 段量測後定（後來由第 15 條改成預設 1）。不選非同步的 raster（C2，工作量大很多，量過再考慮）；不選 rayon 的全域 pool（和 wasmtime 編譯 wasm 搶）；不選自己用 `crossbeam-channel` 寫一組（等於重做 rayon）。
5. **量測工具是 `ci.py raster`，預設 headless**（2026-10-18，使用者）：沿用 `scripts/speedometer.py` 的 CDP 和 trace 的 reader；Compositor thread 上的 raster 時間和用哪個 Viz 的 renderer 無關，headless 不會跳出視窗；`--window` 用真的視窗。不選情境加上量測（情境是正確性的測試，不量時間）。
6. **狀態 op 的範圍是它管到的畫圖 op 的聯集；不知道會畫到哪裡的 op 每個 tile 都播（S1）**（2026-10-18，使用者）：沒帶範圍的畫圖 op（`DrawColor` 填滿整個 clip），和帶 filter 或 blend mode 的 `SaveLayer` 裡的每一個 op，範圍是「到處」。目前只有 canvas 的 list 有這兩種 op，而它不經過 tile。不選照 filter 算出確切的擴出（S2：程式多，目前沒有 layer 的 list 用得到）。
7. **B0 改成用索引查（Y）**（2026-10-18，使用者）：tile 查不到任何 op 就是清除色的單色 tile（B2），取代和整份 list 的範圍比；兩塊內容之間的空白 tile 也抓得到。不選留到第 4 段（N）。
8. **只有給 tile 用的 list 建索引（U2）**（2026-10-18，使用者）：和 cc 的 `DisplayItemList::UsageHint`（只有 `kTopLevelDisplayItemList` 建 `RTree`）一樣。不選每一份都建（U1：canvas 和 SVG 白付建樹的成本）；不選再加 op 數的門檻（U3：多一條路徑要測）。
9. **B1 認得平移縮放、`ClipRect`、不透明的 `DrawRect` 和 `DrawColor`，幾何碰不到 tile 的 op 當沒畫（P1）**（2026-10-18，使用者）：照量到的 NewsSite 的單色 tile 選的（63% 是一個蓋滿的不透明矩形，另外約 10% 是被 clip 切掉或只有保守範圍碰到的）。不選只認一個蓋滿的矩形（P1′：漏掉被 clip 切掉的）；不選半透明的疊色（P2：捨入要和 Skia 逐像素一樣，而 macOS 和其他平台不同）；不選文字墨水的精確範圍（P3：要查字型，Gutenberg 的單色 tile 只佔它 raster 的 9%）。
10. **不設 op 數的上限，第一個認不得的 op 就放棄（N1）**（2026-10-18，使用者）：不選設上限（N2：上限之內本來就會在第一個認不得的 op 放棄）。cc 有上限（`max_ops_to_analyze`）。
11. **單元測試，加上 debug build 照樣 raster 比對（V2）**（2026-10-18，使用者）：reftest、WPT 和情境用 debug build 跑，等於拿真的網頁再檢查一次；release 和 `perf` 不受影響。不選只靠單元測試（V1）。
12. **每個 renderer 的 compositor 各有一個 pool，Browser 的 UI 不建（O1）**（2026-10-18，使用者）：和 Chromium 一個 renderer process 一組 raster worker 一樣，守住 process 的邊界。不選 Browser 建一個交給所有 renderer 共用（O2：renderer 之間共用執行中的狀態，拆 process 時要改回來）。
13. **條數由 `--num-raster-threads N` 決定，經過 `RendererFlags`（F1）**（2026-10-18，使用者）：和 Chromium 的開關同名，列在 `kun --help`；`ci.py raster` 有對應的選項。不選環境變數（F2：不容易發現）；不選不開放（F3：量測和調整都要重編）。
14. **worker 共用一份 `RasterCache`，查表時用 `Mutex`（K1）**（2026-10-18，使用者）：每張圖片只解碼一次。不選每個 worker 一份（K2：同一張圖片解碼好幾次、記憶體好幾份）；不選開始之前先在 compositor thread 上準備好（K3：要走遍巢狀的 record 和 shader，程式多）。
15. **raster worker 預設不開（1 條），`--num-raster-threads` 打開**（2026-10-18，使用者）：第 6 段的 Speedometer 量到第 5 段（預設 9 條）比第 4 段時間 +1.0%、記憶體 +9.9%（20 個 suite 都變多）；修正之後 5 個 suite 仍然時間 +1.4%、記憶體 −2% 到 +20%，4 條也分不出差別。預設 1 條時對 `main` 是時間 +0.5%、記憶體 −0.1%（A/A 是 +0.3%、−1.2%），沒有退步。不選預設開著接受代價（跑 Speedometer 這類頁面時多用 CPU 和記憶體）；不選 revert 第 5 段（tile 多的頁面要快的時候開得起來，以後做 GPU raster 時再決定拿掉或沿用）。
16. **每個 tile 在畫它的 thread 上做完，重複用 buffer**（2026-10-18，使用者，第 6 段的 N0）：raster thread 畫進自己重複用的 buffer、掃單色、上傳到 GPU 或複製出要留的像素；compositor thread 只照順序編 resource id。原本把整次更新的 bitmap 收齊才上傳。試過的「查字型每條 thread 自己記著、不拿鎖」沒有效果（每個 tile 的時間沒變），拿掉了。

## 待決定

沒有。

## 之後的可能

- **GPU raster 或換 raster 的函式庫**（RFC 0007）：換掉 `raster` 把 op 畫成像素的那一步；索引、單色的判斷和 worker 都留著。那時 worker 的角色也跟著變（Chromium 的 worker 只錄製）。
- **先畫可見的 tile**：draw 之前只同步 raster 可見範圍的 tile，interest rect 裡其餘的在 draw 之後補（cc 的 tile 優先順序，非同步 raster 的第一步）。NewsSite 改大小時每次更新 raster 約 300 個 tile，可見的約 70 個。
- **非同步的 raster**（C2）：沒畫好的 tile 先畫 checkerboard，捲動時 frame 不等 raster。
- **上傳到 GPU**：真的視窗裡每個 tile 各送一次 queue，在 compositor thread 上排隊；raster 變快之後它佔的比例變大（Gutenberg 捲動 headless 87 ms、真的視窗 369 ms）。
- **raster worker 的效率**：每條 thread 畫一個 tile 的時間是一條時的 1.8 到 8 倍，原因還沒查清楚（核心的快慢、排擠、Skia 的字形快取的鎖）；查清楚之後再考慮預設開。
- **文字的單色判斷**（P3）：字形墨水的精確範圍，Gutenberg 的單色 tile 都是文字的保守範圍碰到的。
- **layer 的切法**（重疊判斷、squashing，像 Blink 的 `PaintArtifactCompositor`）：少一些大片透明的 layer。
- **main thread 改大小的成本**：Gutenberg 改大小時 layout 約 900 ms，比 raster 的 550 ms 多。
