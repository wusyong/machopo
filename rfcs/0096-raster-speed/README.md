# RFC 0096：tile 的 raster 的速度：建場景那一段量到的，和可以做的

- 開始日期：2026-11-11
- 狀態：📝 草稿
- 細節：`details.md`

## 摘要

RFC 0095 改完上傳之後，一次 `PrepareTiles` 最大的一塊是建場景（文字多的頁面五到七成）。這份 RFC 先量它花在哪裡（做了，結果在下面和 `details.md`），再從量到的結果選要做什麼。量到的重點：

- **建場景幾乎全部是文字**，時間在 Vello 把 glyph 的外框線變成 strip 的那幾步，machopo 這一側省不到多少。
- **量測本身不可靠**：這台機器是大小核的 CPU，compositor thread 落在節能核心上時每個 tile 慢 1.7 倍，同一個執行檔前後兩次可以差一倍。
- **多條 raster thread 是現成、最大的一項**：固定在效能核心上量，2 條每個 tile 少 43%，4 條少 57%，8 條沒有更多。

要做什麼還沒有決定（「待決定」）。2026-11-11 的看法：先修量測（把 benchmark 固定在效能核心上），再把 raster thread 的預設改成 4，並讓 compositor 和 raster 的 thread 在大小核的機器上不被排到節能核心。

## 動機

**現況**（2026-11-11，RFC 0095 之後；`bench.py raster` 的三頁，佔 `PrepareTiles`）：建場景 25% 到 71%，render 13% 到 47%，上傳 11% 到 20%，讀像素 2% 到 5%。raster 是同步的，`PrepareTiles` 多長、frame 就多長。

**問題**：

- 建場景是最大的一塊，但不知道它裡面是什麼、machopo 動得到哪些。
- `bench.py raster` 的數字在這台機器上忽快忽慢（RFC 0007 和 0095 都記過「這一格不穩」「不同的 build 之間差幾個百分點到一成」），沒有查過原因，所以小於一成的改進都看不出來。
- `--num-raster-threads` 預設是 1，`docs/limitations.md` 寫的理由（每條 thread 比較慢、多用 CPU 和記憶體）沒有最近的數字。

要解決的 `docs/limitations.md` 裡的限制：看選了什麼。候選是「Paint 和 raster」的「tile 的 raster 是同步的，預設在 compositor thread 上一個一個畫」那一條裡「預設不開」的部分。

**目標**（草稿）：

- `bench.py raster` 的數字在大小核的機器上重跑時穩定（同一個執行檔兩次差在幾個百分點以內）。
- 文字多的頁面每個 tile 的 `PrepareTiles` 明顯變短（數字看選了什麼）。

**不在範圍內**：改 Vello 產生 strip 的方式；glyph 的 atlas（RFC 0007 第 46 條不採用）；GPU raster（RFC 0095 第 6 條延後）；async raster 和 checkerboard（另一份 RFC：那是讓 raster 不擋 frame，這份是讓它快一點）。

## 設計概要

還沒有選定做法，這一節列的是量到的結果指向的幾件事；選項在「理由和其他做法」。

給 machopo 的使用者：做了「預設多條 raster thread」的話，捲動和改視窗大小時每一輪 tile 準備得快一倍左右，代價是那段時間多用幾個核心。

給之後改 machopo 的人：`bench.py raster` 在 Windows 的大小核機器上會把 machopo 固定在效能核心上，數字才比得起來。

## 詳細設計

### 量到的

細節和原始的數字在 `details.md`。

- **建場景裡是什麼**（samply 取樣 compositor thread，佔 `PrepareTiles`）：

  | | SQLite | Gutenberg | NewsSite-Next |
  | --- | --- | --- | --- |
  | 畫文字（`Player::draw_glyphs`） | 67% | 52% | 23% |
  | 其中 Vello 產生 strip（`generate_with_clip`） | 46% | 34% | 17% |
  | render（`render_with`） | 14% | 31% | 46% |
  | 上傳（`create_shared_image`） | 15% | 11% | 19% |

  文字佔了建場景的九成以上。它的時間在 Vello 裡：攤平外框線 9% 到 11%、切 tile 和排序 12% 到 17%、算 strip 的覆蓋率 17% 到 19%（文字多的兩頁）；讀字型的外框線只有 2% 到 4%。
- **大小核**（i9-13900K，8 個效能核心、16 個節能核心；SQLite 改視窗大小，同一個執行檔，各 3 次）：不固定和固定在效能核心上都是每個 tile 125 µs 上下；固定在節能核心上 214 µs（1.7 倍）。不固定時落在哪一種由 Windows 決定：同一天裡量到過 127 和 267。
- **多條 raster thread**（固定在效能核心上，SQLite 改視窗大小，各 2 次）：

  | raster thread | 每個 tile 的 `PrepareTiles`（µs） | 每個 tile 的 playback（µs） |
  | --- | --- | --- |
  | 1 | 124、127 | 120、122 |
  | 2 | 70、72 | 127、130 |
  | 4 | 54、53 | 169、163 |
  | 8 | 55、51 | 207、194 |

  2 條少 43%，4 條少 57%，8 條沒有更多（每個 tile 自己的 playback 變慢：搶 cache 和記憶體頻寬，上傳還要輪流拿鎖）。只量了這一頁、這一個階段。
- **少交一些 glyph 給 Vello**（實驗，沒有留下程式）：現在離 tile 兩倍字級以內的 glyph 都交給 Vello。改用字型的 bounding box 判斷，只少交 12% 到 20% 的 glyph，時間量不出差別（那些 glyph 在 Vello 裡只走到攤平就被丟掉）。用每個 glyph 自己的範圍估計能少交四到五成，換算成 `PrepareTiles` 大約 5%：是估計，沒有量。

### 候選的做法

- **量測固定在效能核心上**：`bench.py` 在 Windows 上把 machopo 的 process 的 affinity 設成效能核心（用 `GetSystemCpuSetInformation` 的 `EfficiencyClass` 找；Linux 已經照最高頻率挑核心）。只動腳本。
- **raster thread 的預設**：改成 `min(4, 效能核心數的一半)` 之類的值（對照 Chromium：`RasterThreadsFromCommandLineOrDefault`，桌機通常是 4）。要補量的：其他兩頁、捲動的階段、記憶體（每條 thread 一個 Vello 的 context 和一塊 tile 的 buffer）、少核心的機器（CI 是 4 個 CPU）。
- **thread 不被排到節能核心**：compositor thread 和 raster worker 設成高的 QoS（Windows 的 `SetThreadInformation` 關掉 power throttling 或提高 priority，macOS 的 QoS class）。對照 Chromium：`ThreadType::kDisplayCritical`。要 FFI（`unsafe` 只能在 `bindings` 和 `ipc`：要嘛用現成的 crate，要嘛放進 `ipc`），而且「瀏覽器跑的時候會不會被排到節能核心」還沒有直接量過，只知道 benchmark 有時會。
- **每個 glyph 自己的範圍來過濾**：估計 5%，要快取每個 glyph 的範圍。
- **tile 大一點**：邊界上重複畫的 glyph 少一些、每個 tile 固定的成本攤掉；沒有量。invalidation 重畫的面積會變大。

### 測試

看選了什麼。量測的腳本有自己的單元測試（`scripts/bench/`）；raster thread 的預設值是設定，compositor 已經有多條 thread 的測試；thread 的 QoS 測不了行為，只能測「有設」。

### 分階段

還沒有定。看法：一、量測固定核心（腳本）；二、補量 raster thread（三頁、兩個階段、記憶體），定預設值；三、看要不要做 QoS。

## 缺點

- 多條 raster thread：頁面在動的時候多用幾個核心和一些記憶體；每條 thread 一份 Vello 的 context。
- 固定核心的 benchmark 量到的是「落在效能核心時」的速度，不是使用者一定拿到的速度。
- QoS：多一段平台專屬的程式和 FFI。

## 理由和其他做法

- **A. 只修量測**：之後的每一項效能工作都需要它，最小。
- **B. 修量測，raster thread 預設改成多條**（2026-11-11 的看法：B）：最大、現成的一項，程式已經有（`--num-raster-threads`）。
- **C. B 加上 thread 的 QoS**：在大小核的機器上可能再差 1.7 倍，但還沒有證據說瀏覽器平常就會被排到節能核心；先量再說。
- **D. 過濾 glyph、tile 的大小**：小（估計 5% 上下），而且 B 之後每個 tile 的時間已經不是同一個瓶頸。
- **不做**：文字多的頁面維持每個 tile 一百多 µs、一條 thread。

## 先例

Chromium 的部分照記得的寫的，2026-11-11 沒有對過原始碼；接受之前要對一次。

- **Chromium**：raster 在 worker thread 上（桌機預設 4 條，`cc::TaskGraphRunner`），compositor thread 不畫；這些 thread 是 `kDisplayCritical`／`kCompositing` 的優先權，在 Windows 和 macOS 上對應到不被節能排程的 QoS。量測的機器固定設定（perf bots），本機量的時候 Chromium 的文件也建議固定核心。
- machopo 的 `bench.py` 在 Linux 上已經把瀏覽器固定在最高頻率的核心（`on_browser_cpus`），Windows 沒有做（當時寫的是 Windows 沒有 `taskset`）。

## 已決定的事項

無（草稿）。

## 待決定

- 做 A、B、C 的哪一個（看法：B；C 等量過瀏覽器平常落在哪種核心再決定）。
- raster thread 的預設值怎麼算（固定 4、照核心數、照效能核心數），少核心的機器上是多少。
- QoS 的 FFI 放在哪裡（現成的 crate，或 `ipc`）。

## 之後的可能

- async raster（tile 的優先順序、checkerboard）：raster 不擋 frame。
- GPU raster（RFC 0095 的「GPU raster 的設計」）：拿走 render 和上傳。
- 上游的 glyph 快取能用時（RFC 0007 第 46 條）：文字的 strip 不用每個 tile 重算，這是文字這一塊真正的解法。
