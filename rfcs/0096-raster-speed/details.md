# RFC 0096 的細節：tile 的 raster 的速度

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

都是 2026-11-11 在 Windows 11、i9-13900K（8 個效能核心加 16 個節能核心，32 個邏輯 CPU）、Radeon RX 7900 XTX 上量的，machopo 是 `6dda0d5` 的 `perf` profile。量測用的腳本是臨時的，沒有進 repo；要重做時照下面寫的做法。

## 取樣：`PrepareTiles` 裡是什麼

做法：`bench.py raster` 的每一頁（捲動加改視窗大小，headless、scale 2、一條 raster thread）包在 `samply record` 裡跑（`scripts/bench/common.py` 的 `samply_sampler`，1000 Hz、ETW），讀 `Compositor` thread 的樣本（`speedometer.py` 的 `read_samples`），只看堆疊裡有 `prepare_tiles` 的；下面是每個函式（連同它呼叫的）佔那些樣本的比例。一頁一次。

| 函式 | SQLite | Gutenberg | NewsSite-Next |
| --- | --- | --- | --- |
| `raster::vello_backend::Player::draw_glyphs` | 67.0% | 51.5% | 23.3% |
| `vello_cpu::RenderContext::fill_path`（幾乎都在 `draw_glyphs` 底下） | 57.8% | 44.2% | 20.5% |
| `vello_common::strip_generator::StripGenerator::generate_with_clip` | 45.5% | 34.2% | 16.7% |
| `vello_common::strip::render` | 18.7% | 16.7% | 7.6% |
| `vello_common::tile::Tiles::make_tiles_analytic_aa`（含排序） | 17.3% | 11.7% | 5.5% |
| `vello_common::flatten::fill` | 10.7% | 9.1% | 3.5% |
| `core::slice::sort` 的 quicksort | 8.3% | 5.4% | 3.0% |
| `vello_cpu::RenderContext::render_with` | 14.0% | 30.9% | 45.7% |
| `vello_cpu::fine::Fine::paint_fill` | 5.3% | 24.0% | 34.4% |
| `viz` 的 `create_shared_image` | 14.7% | 11.1% | 18.7% |
| `wgpu` 的 `write_texture` | 10.1% | 8.4% | 10.9% |
| glifo 的外框線快取、skrifa 讀外框線 | 約 3% | 約 2% | 不到 1% |

- 樣本的總量：compositor thread 的 CPU 時間 675 ms、1053 ms、1338 ms，其中 `prepare_tiles` 底下 623 ms、952 ms、1213 ms（92%、90%、91%）。
- 自己的時間最多的是 `fearless_simd` 的 `vectorize_avx2`（45% 到 56%）：那是 Vello 的 SIMD 分派的外殼，攤平、strip 和填色都 inline 在裡面，分不出來；上面用連同呼叫的比例。
- `PrepareTiles` 裡 `draw_glyphs` 以外的 `Player::draw` 只有 1% 到 2%：形狀和圖片在建場景時幾乎不花時間（它們的成本在 render）。

## 大小核

做法：`bench.py raster` 的 `run_page` 起了 machopo 之後用 `SetProcessAffinityMask` 設它的 affinity（`0xffff` 是前 16 個邏輯 CPU，這顆 CPU 上是 8 個效能核心的兩條 thread；`0xffff0000` 是 16 個節能核心），SQLite 的那一頁，改視窗大小的階段。

| affinity | 每個 tile 的 `PrepareTiles`（µs，三次） | 每個 tile 的 playback（µs） |
| --- | --- | --- |
| 不設 | 127、125、127 | 123、121、124 |
| 效能核心 | 125、123、125 | 121、120、122 |
| 節能核心 | 213、214、214 | 195、197、197 |

- 這一輪不設時落在效能核心上。同一天稍早，同一個執行檔、不設 affinity 連跑幾次是 245 到 277（比只用節能核心還慢，原因沒查：可能加上了別的程式在搶），再早是 127 到 150。
- 之前記的「Gutenberg 的捲動每一輪差很多」「不同的 build 之間 playback 差 -3% 到 +11%」「跑到一半整體變慢」，都可能是這個原因；沒有回頭一一確認。
- 沒有查的：Windows 什麼時候把它排到節能核心（沒有視窗的 process、前景後景、電源模式）；有視窗時是不是一樣。

## 多條 raster thread

做法同上，固定在效能核心上，`--num-raster-threads` 1、2、4、8 輪流跑兩輪，SQLite 改視窗大小。數字在 `README.md` 的「量到的」。

- 同一天沒有固定核心、機器在慢的那個狀態時也量過三頁（一輪）：每個 tile 的 `PrepareTiles` 對一條 thread 的比，2 條是 0.77 到 0.83，4 條是 0.49 到 0.73（Gutenberg 改大小 0.49、SQLite 0.55、NewsSite-Next 改大小 0.73）。那一輪的絕對值不可靠，只能看方向：圖和色塊多的頁面（render 的比例高）從多條 thread 得到的比文字多的少。
- 沒有量的：固定核心之後的另外兩頁和捲動的階段；記憶體；CPU 的總用量；4 個 CPU 的機器。

## 用字型的 bounding box 過濾 glyph

做法（實驗，量完還原）：`draw_glyphs` 原本把原點在「tile 往外兩倍字級加 stroke 的寬度」以內的 glyph 交給 Vello。實驗多一個條件：字型 `head` 表的 bounding box 放在 glyph 的位置上、經過目前的 matrix 之後要碰到 tile（可變字型、有點陣 glyph 的字型、每個 glyph 自己旋轉的不套用；合成粗體和斜體多留一些）。開關是環境變數，同一個執行檔輪流跑。

- 交給 Vello 的 glyph：Gutenberg 少 17%（400,001 → 330,816）、SQLite 少 20%、NewsSite-Next 少 12%。
- 時間：開和關輪流各兩次，每個 tile 的 `PrepareTiles` 和建場景的時間都在雜訊裡（那幾次機器在慢的狀態，SQLite 是 256、266 對 266、266）。
- `cargo test -p raster` 開著它全部通過（包括 tile 和整塊逐像素比的那個）。
- 字型的 bounding box 很寬（要包住最寬的 glyph），所以少得不多。每個 glyph 自己的範圍會緊很多：照 tile 256、字 32 個 device pixel 估，原本交出去的 glyph 大約一半碰不到 tile。被丟掉的 glyph 在 Vello 裡省下的主要是攤平（佔 `PrepareTiles` 的 9% 到 11% 是全部 glyph 的），所以估計是 5% 上下。沒有做、沒有量。
