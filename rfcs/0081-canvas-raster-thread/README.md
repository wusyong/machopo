# RFC 0081：canvas 的 raster 移出 main thread

- 開始日期：2026-10-23
- 狀態：✅ 已完成：2026-10-23（驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`（kun 現在的做法、Chromium 的做法、Speedometer 的量測和用到的 API、沒選的做法）

## 摘要

2D context 錄下的 paint op 不再由 main thread 當場用 Skia 畫：交給每個 renderer 一條專用的 canvas raster thread，bitmap 由它持有。main thread 送出錄製就繼續跑 script，frame 和 compositor 拿到的是「畫到這裡的快照」，由 compositor 那一側等它畫好。script 讀像素（`getImageData`、`toDataURL`、`toBlob`）時 main thread 同步等 raster thread 畫完，和 Chrome 加速的 canvas 一樣；canvas 當 `drawImage`、`createPattern` 的來源和 `putImageData` 都錄進錄製，不等。`willReadFrequently: true` 的 canvas 照舊在 main thread 上畫，`getImageData` 讀回 2 次的 canvas 也搬回 main thread（Chrome 的 `kFallbackToCPUAfterReadbacks`）。做法對應 Chromium 加速的 canvas：`CanvasRenderingContext2D` 錄進 `PaintRecord`，交給另一側（GPU process）重播；kun 的另一側是 CPU 的 Skia，在 renderer 裡的另一條 thread。

## 動機

**現況**：

- `dom_api` 把每個繪圖呼叫錄成 `PaintOp`，存在 `dom` 的 `CanvasData` 上。
- 三種時候 renderer 的 main thread 當場用 Skia 的 CPU raster 把錄製畫進 `CanvasData` 的 `Vec<u8>`：
  - 錄滿 `MAX_RECORDED_OPS`（10,000 個 op）：在 script 的呼叫裡畫（Blink 的 `FlushIfRecordingLimitExceeded`）；
  - 每個 frame 之前（`renderer::canvas::flush_shown`）：畫在 frame 的 rendering 裡；
  - script 讀像素、把它當成 `drawImage` 或 `createPattern` 的來源、`putImageData` 只蓋一部分的時候。
- frame 拿 bitmap 時整份複製一次，當成 texture layer 的快照（`CanvasData::snapshot`）。

**問題**：

- Speedometer 3.1 在 Windows 上（2026-10-23，2026-10-23 那一版的 `docs/performance-gaps.md`（2026-11-06 整份改寫之前）裡「Windows 上」那一節）：
  - Charts-chartjs 的 `drawPointLegend` kun 71.8 ms、Chrome 2.6 ms，`draw` 14.0 對 1.3 ms：錄滿 10,000 個 op 時在 script 裡畫；
  - Perf-Dashboard 每個 frame 之前畫 canvas，算在 `(program)`；
  - 取樣到的 canvas raster 兩個 suite 合計 113 ms，兩個 suite 和 Chrome 的差合計 +186.7 ms，是 20 個 suite 裡最大的一類。
- Chrome 在這兩個 suite 上幾乎不花時間在 raster：它的 canvas 是加速的，op 交給 GPU process 重播，main thread 只序列化。快的原因是 raster 不在 main thread 上，不是 GPU 畫得比較快（`details.md` 的「Chromium 的做法」）。
- 真的視窗在螢幕縮放 150% 時 canvas 有 2.25 倍的像素，main thread 上的 raster 更重。
- 兩個 suite 的計時路徑上都沒有讀像素的呼叫（`details.md` 的「Speedometer 的量測和用到的 API」），移出 main thread 之後這段時間整個不在 main thread 上。

**目標**：

1. 錄滿 10,000 個 op 和 frame 之前的 raster 不在 main thread 上：main thread 只把錄製交出去。
2. frame 照舊顯示每個 frame 之前錄的全部內容：compositor 在 draw 之前等那個快照畫好（不在 main thread 上等），畫面不會晚一個 frame、不會露出畫到一半的 bitmap。
3. 讀像素的 API 照規範是同步的：main thread 等 raster thread 畫完再讀，結果和現在一樣。
4. `willReadFrequently: true` 的 canvas 照舊在 main thread 上畫，讀像素不用跨 thread（Chrome 的 `willReadFrequently` 也是 CPU 的 canvas）。
5. 畫的順序和現在一樣：同一個 canvas 的錄製、`putImageData`、拿快照，照 script 呼叫的順序；把 canvas 畫到另一個 canvas（或自己）時，用的是呼叫那時的內容。
6. script 錄得比 raster thread 畫得快時，main thread 等它（上限），錄製和待畫的 bitmap 不會無限增加。
7. canvas 被回收時 raster thread 釋放它的 bitmap。

**要解決的 `docs/limitations.md` 的限制**：

- 「Bindings 和 DOM API」的 `<canvas>` 那條：`willReadFrequently` 只記下、不影響怎麼畫。做完之後它決定 canvas 在哪條 thread 上畫，那句要改寫。

**不在範圍內**：

- GPU 的 raster（Skia 的 Ganesh / Graphite、vello_gpu）：raster thread 照舊用 Skia 的 CPU raster。之後要換的話，換的是 raster thread 重播的那一端，錄製和 thread 之間的協定不用改。
- 換掉 Skia（vello_cpu），或讓 raster 本身變快（小的路徑不經過抗鋸齒的 clip、SIMD）：搬走之後這段時間已經不在 main thread 上。
- `toBlob` 的編碼移出 main thread（現在呼叫時就在 main thread 上編碼）：見「之後的可能」。
- `OffscreenCanvas`、`ImageBitmap`：kun 沒有。
- 把 raster 放進 Viz（跨 process 的邊界）：見「理由和其他做法」。

## 設計概要

網頁的行為不變，變的是時間花在哪條 thread 上：

```js
const ctx = canvas.getContext("2d");
for (let i = 0; i < 50000; i++) {   // chart.js 畫每個點的圖例
  ctx.beginPath(); ctx.arc(x(i), y(i), 3, 0, 2 * Math.PI); ctx.fill();
}
// 每錄滿 10,000 個 op：交給 raster thread，script 馬上繼續（現在是當場畫完才繼續）
requestAnimationFrame(() => {});
// frame 之前：剩下的錄製交出去，frame 帶著「畫到這裡的快照」；
// compositor 在 draw 之前等它畫好，main thread 不等

ctx.getImageData(0, 0, 1, 1);
// 讀像素：main thread 等 raster thread 把交出去的都畫完，再讀（同步，和現在一樣的結果）

const fast = other.getContext("2d", { willReadFrequently: true });
// 這個 canvas 照舊在 main thread 上畫，讀像素不用等別的 thread
```

給之後改這部分程式的人：

- canvas 的 bitmap 不在 `dom` 的 `CanvasData` 上，在 raster thread 上；`CanvasData` 只有錄製、狀態和一個指向 raster thread 上那份 bitmap 的 id（`willReadFrequently` 的、讀回多次搬回來的除外）。
- 要讀 bitmap 的程式（新的讀像素的 API、新的「把 canvas 當來源」的地方）一律經過 renderer 的 canvas 模組：它先把錄製交出去，再看要等快照、還是在 raster thread 上引用它。

## 詳細設計

### 1. canvas raster thread

- 每個 renderer 一條（thread 名稱 `CanvasRaster`），第一個 canvas 要交出錄製時才建立，和 compositor 的 raster worker 一樣是 renderer 這一側的 thread，拆 process 時跟著 renderer 走，`protocol` 不變。
- 它持有每個 canvas 的資源（Chromium 的 `CanvasResourceProvider`）：bitmap 的像素和大小、`alpha`。
- 和 main thread 之間是一條 FIFO 的佇列，一次送一批命令（`renderer` 的 `canvas_raster::Item`）：
  - **建立**：canvas 第一次交出錄製時，bitmap 的大小、`alpha`，和它在 main thread 上已經有的像素（例如整個蓋住的 `putImageData`、不透明的 canvas 的快照），沒有的話照背景色配置；
  - **畫**：一個 canvas 的錄製（`Vec<PaintOp>`）。`putImageData` 的矩形也是錄製裡的一個 op（`PaintOp::WritePixels`，Skia 的 `writePixels`：不經過 matrix 和 clip），和畫的 op 照呼叫的順序；
  - **快照**：回一個快照（見第 3 節）；
  - **釋放**：context 不再持有這個 bitmap（被回收、或 `width`、`height` 改了、`reset()`）。重設不是另外的命令：`CanvasData::reset` 丟掉資源，下一次交出錄製時建一個新的。
- raster thread 照佇列的順序做，自己不等 main thread 和 compositor，所以不會和它們互相等待。
- 它用的 `RasterCache`（typeface 和圖片的快取）和 main thread 共用一份：`RasterCache` 已經是 thread-safe 的（compositor 的 raster worker 也共用），main thread 照舊用它算 `measureText` 的 ink。
- `PaintOp` 和 `CanvasData` 已經是 `Send + Sync`（`dom::canvas` 有斷言），錄製不用轉換就能交出去。

### 2. 什麼時候交出錄製、什麼時候等

| 時機 | 現在 | 改成 |
| --- | --- | --- |
| 錄滿 `MAX_RECORDED_OPS` | main thread 當場畫 | 送「畫」，不等 |
| frame 之前（`flush_shown`） | main thread 當場畫，再整份複製成快照 | 送「畫」和「快照」，frame 帶著快照的 handle，不等 |
| `getImageData`、`toDataURL`、`toBlob` | 當場畫完再讀 | 送「畫」和「快照」，等快照畫好再讀 |
| `drawImage`、`createPattern` 的來源是 canvas | 來源當場畫完、整份複製 | 來源送「畫」和「快照」，錄製裡放快照的 `ImageData`，raster thread 畫到那個 op 時才讀它，main thread 不等 |
| `putImageData` 只蓋一部分 | 當場畫完、在 main thread 上寫像素 | 錄一個「寫像素」的 op，不 flush、不等；bitmap 還在 main thread 上時先 flush（這時交出去），再錄 |
| `putImageData` 蓋住整個 bitmap | 丟掉錄製、寫像素 | 丟掉錄製，bitmap 在 raster thread 上時錄「寫像素」，還在 main thread 上時照舊寫 |
| `willReadFrequently: true`，或 `getImageData` 讀回 2 次之後 | — | 全部照現在，在 main thread 上畫（第 4 節） |

- **一個 frame 是一批**：`flush_shown` 把這個 frame 所有要畫的 canvas 的錄製和快照放進同一個命令送出，不是每個 canvas 各一個。
- **兩種上限**（「已決定的事項」第 5 條），超過時 main thread 等到降下來：
  - script 中途（錄滿 `MAX_RECORDED_OPS` 時交出的）：佇列裡沒畫完的 op 合計最多 `MAX_QUEUED_OPS`（100,000，10 批）。這只是守住記憶體的安全網，正常的頁面碰不到；對應 Chromium 的 command buffer 滿了才等 GPU process。不用批次數：chart.js 這種 raster 比錄製慢一個數量級的長 script，批次的上限一小，main thread 很快就要一批一批地等。
  - frame 之前：raster thread 最多落後 `MAX_FRAMES_BEHIND`（2）個 frame，第三個 frame 交出去之前等最舊的那個畫完（Chromium 的 `kMaxCanvasAnimationBacklog`）。
- 上限照 op 的個數算，不照錄製的位元組：kun 的錄製是 `Vec<PaintOp>`，位元組要為每種 op 估算 heap 上的大小（路徑、glyph、色標），而「錄滿就交出去」本來就照個數算，兩邊用同一個單位（「之後的可能」有一起改成位元組的做法）。
- canvas 第一次交出錄製時才在 raster thread 上建 bitmap；只錄、從來沒交出去的 canvas 不佔 raster thread 的記憶體。

### 3. 快照和 compositor

- **快照**照舊是 `gfx::ImageData`（「已決定的事項」第 4 條），大小和 id 在 main thread 上送出時就定了，像素由 raster thread 畫到那個命令時填進去（`Arc` 的像素）。對應 Blink 的 `HTMLCanvasElement::Snapshot` 給的 `StaticBitmapImage`：加速的是 GPU 上的 shared image，附一個 sync token。
- **明確的等待點**（Chromium 的 sync token）：`ImageData` 多一個 `wait_ready()`，要讀像素的地方先呼叫它：compositor 上傳 texture、raster 建 Skia 的 image、`dom_api` 讀像素的 API。還沒填好的快照呼叫 `as_bytes()` 會 panic，不會偷偷等；`ImageData` 的 `Debug` 不再讀 bytes。paint、display item、`PaintOp`、compositor 的 `TextureLayer` 照舊傳 `ImageData`，型別不變。
- 把 canvas 當 `drawImage` 的來源時，快照的 `ImageData` 直接放進 `PaintOp::DrawImage`：raster thread 照 FIFO 畫到那個 op 時，快照的命令在它前面，已經填好了。
- **copy-on-write**：raster thread 的 bitmap 是 `Arc` 的像素。快照拿走一個 `Arc`；下一次畫的時候，如果快照還在別人手上，先複製一份再畫（配置不到時這次的錄製不畫），沒人拿著就直接畫。
- **沒來的像素**：raster thread 配置不到 bitmap、或已經不在（panic、renderer 結束）時，快照的填寫者被丟掉，等的一方得到「不會來了」（`wait_ready` 回傳 `false`）：讀像素的 API 讀到背景色，compositor 不畫那個 layer，tile 的 raster 不畫那張圖。現在每次換快照都在 main thread 上整份複製，改成在 raster thread 上、而且只在快照還被拿著時複製。
- **frame**：`FrameView::take_canvas_textures` 和 paint 拿到的是還沒填好的快照。canvas 是自己的 layer 時，compositor 在 draw 之前（上傳 texture 時）等它；不是 layer 時，快照畫進 tile，tile 的 raster 在 compositor thread（或 raster worker）上等它。等的都不是 main thread。

### 4. `willReadFrequently` 和其他留在 main thread 上的

- `willReadFrequently: true` 的 context 照現在的路：`CanvasData` 上有 `Vec<u8>`，main thread 當場畫。
- 規範的 `willReadFrequently` 只是提示，Chrome 用它選 CPU 的 canvas，這裡照做。
- 讀很多次、但沒設 `willReadFrequently` 的 canvas，Chrome 在讀回 2 次之後改成 CPU（`details.md` 的「Chromium 的做法」）。照做（「已決定的事項」第 9 條）：`CanvasData` 記讀回的次數（`readbacks`），`getImageData` 讀完第 2 次時把剛等到的快照的像素複製成 main thread 的 bitmap，`resource` 放掉（raster thread 上的在下一個 frame 釋放），`drawn_here` 設起來，renderer 之後和 `willReadFrequently` 一樣當場畫。`reset()` 留著兩者（Blink 的 `read_count_` 和 `DisableAcceleration` 也不隨 reset 改回）。快照的像素沒來、或沒有記憶體複製時留在 raster thread 上。

### 5. 回收

- `CanvasData` 持有 renderer 給的 `Arc<CanvasResource>`（只有 bitmap 的 id）；renderer 留著它的 `Weak`。每個 frame 之前，renderer 把 `Weak` 已經失效的（context 被回收、或 `reset` 換掉了）放進那個 frame 的批次，送「釋放」。不在 drop 時送：`CanvasData` 是 `NoTrace`，它的 `Drop` 在 GC 的 finalizer 裡跑，只能釋放記憶體，不能上鎖或送訊息（`crates/dom/soundness.md`）。
- renderer 結束時 raster thread 跟著結束，佇列裡沒畫完的丟掉；raster thread panic 時，main thread 之後不再等它。

### 測試

- 現有的 canvas 測試照舊要過：`dom_api` 和 `renderer` 的 canvas 測試、`browser` 的 `canvas_2d`、`canvas_images`、`canvas_export`、kun 的 canvas reftest、WPT 的 `html/canvas/`。它們讀像素或比畫面，經過的就是「交出去再等」的路。
- `gfx`：還沒填的快照（等到填好、沒等就讀會 panic、填的一方丟掉時「不會來了」）。`raster`、`compositor`：等快照才畫、才上傳。
- `renderer`：`canvas_raster` 的 thread、順序、copy-on-write、兩種上限、釋放、panic 和結束（能停住 thread 的 `Pause` 命令）；`canvas` 的交出、frame 一批、`willReadFrequently` 和搬回來的當場畫（main thread 上 `raster::draws_into_for_testing` 的計數）。
- `dom_api`：讀像素等快照、`putImageData` 照順序錄成 op、canvas 當來源、讀回 2 次搬回來（假的 renderer 晚 20 ms 才填快照，漏了等待會讀到錯的像素或 panic）。
- 情境 `canvas-animation`：不停畫的 canvas，畫面每個 frame 都有新的內容。
- mutation test：`mutants-1.toml`、`mutants-3.toml`。
- 只能靠審查的（`docs/test-gaps.md`）：raster thread 不等 main thread 和 compositor（沒有 deadlock）、喚醒不會漏、新的讀快照的地方要先 `wait_ready()`。

### 分階段

1. **raster thread 和「交出去、要用時等」**：thread、佇列、canvas 的資源和釋放；錄滿和 frame 之前交出去不等；快照（`ImageData` 加等待點）和 copy-on-write，compositor 等它；兩種上限；讀像素的 API 先交出去再等；`willReadFrequently` 留在 main thread。原本的第 2 段（canvas 當來源、`putImageData` 不等）一起做了（「已決定的事項」第 6 條）。
2. （併進第 1 段）
3. **讀回多次的 canvas 搬回 main thread**（「已決定的事項」第 9、10 條）：`getImageData` 讀回 2 次的 canvas 搬回 main thread；小的 canvas 不另外處理。

## 缺點

- **多一條 thread 和一層協定**：canvas 的狀態分在兩條 thread 上，順序和回收要靠佇列的 FIFO 和釋放的命令維持。之前在 main thread 上一個函式做完的事，現在是送命令、等 handle。
- **讀像素多了跨 thread 的往返**：上次讀之後又畫了的話，`getImageData` 多一次喚醒另一條 thread 再等它（數十 µs），加上它還沒畫完的錄製；沒再畫的讀用同一份快照，不等。Chrome 加速的 canvas 也有一樣的代價（同步的 IPC），所以讀回 2 次就搬回 main thread（「已決定的事項」第 9 條），只有第一次和第二次要等。
- **compositor 可能等 raster thread**：大的 canvas 畫得慢時，compositor 的 draw 延後（現在是 main thread 的 frame 延後）。畫面一樣慢，但捲動和輸入的處理不會被 script 擋住，main thread 也能在這段時間繼續跑 script。
- **記憶體**：快照還被拿著時要多一份 bitmap（現在每次換快照本來就多一份）。佇列裡的錄製在上限以內，上限照 op 的個數算，一個 op 帶著很大的路徑時守不住位元組（現在「錄滿就交出去」也一樣）。
- **漏掉等待點是執行時的錯**：讀快照的像素前要先 `wait_ready()`，漏了是 panic，不是編譯錯誤；靠測試和 mutation test 守住。
- **測試的時序**：raster thread 讓 canvas 的測試多了非同步的部分，等待點（讀像素、frame）要決定得清楚，否則測試會偶爾失敗。

## 理由和其他做法

完整的比較在 `details.md` 的「沒選的做法」。

- **專用的 canvas raster thread**（選這個，「已決定的事項」第 1 條）：它只收命令、不等別人，沒有 deadlock 的可能；大的 canvas 不會拖慢 compositor 處理 BeginFrame、捲動和輸入；main thread 和 compositor 只在要用快照時等它。
- **放在 compositor thread（和 tile 的 raster 同一處）**：重用現有的 raster 和 worker pool。但錄滿 10,000 個 op 是在 script 的中途，不在 commit 的時候，要另開一條送錄製的路；讀像素時 main thread 同步等 compositor thread，要證明 compositor thread 不會同時在等 main thread（commit）；大的 canvas 會擋住捲動。不選。
- **放在 Viz（GPU process 那一側）**：和 Chromium 的分層一樣，之後換 GPU raster 時在原地換。但要跨 process 的邊界：錄製要序列化，圖片和字型要有 transfer cache（Chromium 的 `TransferCache`、`ServiceFontManager`），讀像素是跨 process 的同步呼叫。kun 的 tile raster 也不在 Viz（`docs/architecture.md` 的「和 Chromium 刻意不同的地方」的 Raster 的位置），只有 canvas 放過去不一致。等之後要上 GPU 再考慮。
- **留在 main thread、讓 raster 本身變快**：Skia 的 CPU raster 熱點是逐條 edge 的抗鋸齒和配置，SIMD 和小的優化幫得有限；就算快一倍，時間還是在 main thread 上。不選。
- **不做**：chart.js 和 Perf-Dashboard 的差距（+186.7 ms）留著，canvas 頁面在高 DPI 的視窗更慢。

## 先例

- **Chromium**：
  - `CanvasRenderingContext2D`（`BaseRenderingContext2D`）把呼叫錄進 `MemoryManagedPaintRecorder` 的 `PaintRecord`；在 frame 之前（`FinalizeFrame`）、讀像素、錄製超過上限（`FlushIfRecordingLimitExceeded`）時 flush。
  - 加速的 canvas（有 GPU、沒有 `willReadFrequently`）：`CanvasResourceProviderSharedImage` 把 op 經過 `RasterInterface` 序列化進 command buffer，GPU process 的 `RasterDecoder` 用 Skia 重播；main thread 不等。`getImageData` 經過 `ReadPixels` 同步等 GPU process；`putImageData` 是 `WritePixels`。
  - CPU 的 canvas（沒有 GPU、`willReadFrequently: true`、讀回多次之後）：`CanvasResourceProviderBitmap`，在 main thread 上用 Skia 畫，和 kun 現在一樣。
  - `HTMLCanvasElement::Snapshot` 給 `StaticBitmapImage`，畫到別的 canvas 時錄進 `PaintImage`，不在 main thread 上複製像素。
  - 交給 compositor 的是 `TextureLayer` 的 `TransferableResource`（shared image 和 sync token），compositor 不等 main thread；用到像素的一方先等 sync token。
  - 上限：錄製超過 2 MB 的 op（Graphite 是 6 MB）或 32 MB 釘住的圖片時 flush；GPU 落後 2 個 frame 時 main thread 在 frame 那裡等（`kMaxCanvasAnimationBacklog`）；script 中途沒有另外的上限，command buffer 滿了才等。
- **kun 自己的先例**：tile 的 raster 在 compositor thread 和 raster worker 上，不在 main thread（Chromium 的 OOP-R 在 GPU process）；`RasterCache` 已經在 thread 之間共用。

## 已決定的事項

2026-10-23 確認（使用者：「照建議：專用的 canvas raster thread、讀回同步等、willReadFrequently 留在 main thread」）：

1. **raster 放在專用的 canvas raster thread**，每個 renderer 一條。不選的做法：compositor thread（讀像素時要證明它不會同時等 main thread，大的 canvas 擋住捲動）、Viz（要序列化錄製和 transfer cache，等之後上 GPU）、留在 main thread 讓 raster 變快（時間還是在 main thread 上）。
2. **讀像素時 main thread 同步等**：`getImageData`、`toDataURL`、`toBlob` 等 raster thread 畫完再讀（規範的 API 是同步的，Chrome 加速的 canvas 也是同步等 GPU process）；`drawImage`、`createPattern` 的來源是 canvas 時，在 raster thread 上引用呼叫那時的快照，main thread 不等（原本排在第 2 段，第 1 段先一律等；後來併進第 1 段，見第 6 條）。不選的做法：讀像素也改成非同步（不符合規範）。
3. **`willReadFrequently: true` 的 canvas 留在 main thread 上畫**，和 Chrome 用它選 CPU 的 canvas 一樣。不選的做法：全部交給 raster thread（常讀的頁面每次都跨 thread 等）。

2026-10-23 確認第 1 段的事（使用者：「好都照你的建議」）：

4. **快照照舊是 `gfx::ImageData`，加上明確的等待點**（Chromium 的 sync token）：像素由 raster thread 填，讀像素的三處（compositor 上傳、raster 建 image、`dom_api` 讀像素）先 `wait_ready()`；沒等就 `as_bytes()` 會 panic，不偷偷等；`Debug` 不讀 bytes。不選的做法：`bytes` 讀的時候才等（等待藏在型別裡，`Debug` 印一個 layer 就可能在 main thread 上卡住）、另外一個 `CanvasSnapshot` 型別（`compositor_paint`、`paint`、`compositor`、`raster` 的型別和比對都要改，和 Chromium 一律用 `PaintImage` 不同）。草稿時的看法是另外一個型別，查過讀像素只有三處之後改了。
5. **佇列的上限分兩種，照 op 的個數算**：script 中途的佇列合計最多 `MAX_QUEUED_OPS`（100,000 個 op），frame 之前 raster thread 最多落後 `MAX_FRAMES_BEHIND`（2）個 frame，一個 frame 的所有 canvas 算一批。不選的做法：未畫完的批次最多 3 批（每個 canvas 各算一批時，一個 frame 畫好幾個 canvas 就在 frame 中途等；chart.js 這種 raster 比錄製慢一個數量級的長 script，main thread 每交一批就要等一批）、照錄製的位元組算（要為每種 `PaintOp` 估算 heap 上的大小，和「錄滿就交出去」的單位不一致）。數字之後量 chart.js 每個 iteration 交出幾批時再調。

2026-10-23 確認第 1 段實作時和原本的設計不同的三處（使用者：「三處偏離都照目前的做法」）：

6. **原本的第 2 段併進第 1 段**：快照是 `ImageData` 之後，canvas 當來源時把快照放進錄製，比「先等再複製」少寫一條路；`putImageData` 在 bitmap 已經在 raster thread 上時本來就要把像素送過去，錄成 op 比「flush、等、再送命令」簡單，也保住和畫的順序。不這樣做：第 1 段要多寫一條等待和複製的路，第 2 段再拿掉。
7. **「寫像素」是 `PaintOp::WritePixels`，不是另外的命令**：和畫的 op 在同一份錄製裡，順序自然對，也算進錄製的上限；`compositor_paint` 多一種只出現在 canvas 的錄製裡的 op（paint 的 display list 不會有，`color_if_solid_in_rect` 碰到它就放棄）。Blink 是 flush 之後呼叫 `CanvasResourceProvider::WritePixels`，GPU 的 command buffer 也照順序。不選：另外的命令（`putImageData` 每次都要 flush 一次，送一批）。
8. **釋放是 frame 時掃 `Weak`，不是 drop 時送命令**：見第 5 節，`NoTrace` 的 `Drop` 不能送訊息。代價是被回收的 bitmap 晚到下一個 frame 才釋放；沒有 frame 的頁面（分頁隱藏）會留著，到下一個 frame 為止。

2026-10-23 確認第 3 段的事（使用者：「照建議：讀回 2 次就搬回 main thread，不設大小門檻」）：

9. **`getImageData` 讀回 2 次的 canvas 搬回 main thread，之後一直在 main thread 上畫**，和 Chrome 的 `kFallbackToCPUAfterReadbacks`（2）一樣：沒設 `willReadFrequently` 的 context 才算，只算 `getImageData`（`toDataURL`、`toBlob` 不算）。搬的時候等最後一份快照，把它的像素當成 main thread 的 bitmap，raster thread 上的在下一個 frame 釋放。不搬 document 裡之後的 canvas（Chrome 的 `DisabledAccelerationCounterSupplement`：那是為了省建立 GPU context 的成本，這裡建一個 bitmap 很便宜）。不選的做法：不搬（「畫、讀」交錯的頁面每次讀都多一次跨 thread 的往返，一個 frame 交錯上千次就多出數十 ms）、不讀了再搬回 raster thread（「不讀了」要猜，多一種反方向的轉換，Chrome 也沒有）。沒有量：第 1 段之後讀像素只有「上次讀之後又畫了」才等，量不到改變決定的東西。
10. **小的 canvas 不留在 main thread**：和 Chromium 現在一樣（`AcceleratedSmallCanvases` 是 stable，`kMinimumAccelerated2dCanvasSize` 只剩 Webview 用）。不讀的小 canvas 在 main thread 上只多送一批，常讀的照第 9 條搬回來。不選的做法：面積小於門檻的留在 main thread（有第 9 條之後省得很少，又多一個要調的數字）。

## 待決定

無。

## 之後的可能

- `toBlob` 的編碼移出 main thread，在 raster thread 或另一個 worker 上做（Chrome 的 PNG 在 idle task、JPEG 在 worker thread）。
- 「錄滿就交出去」和佇列的上限一起改成照錄製的位元組算（Chromium 的 2 MB）：為每種 `PaintOp` 估算大小，兩處共用；會改變每一批的大小。
- raster thread 重播的那一端換成 GPU（Skia 的 GPU backend、vello_gpu），或換成 vello_cpu：錄製和協定不用改。
- `OffscreenCanvas`：worker 上的 context 也把錄製交給同一條 raster thread。
- renderer 拆成 process 之後，視需要把 raster thread 移到 Viz 那一側（跨 process 的版本，Chromium 的做法）。
