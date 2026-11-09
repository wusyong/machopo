# RFC 0056：所有模式都用 wgpu 的 renderer，拿掉 `SoftwareRenderer`

- 開始日期：2026-10-18
- 狀態：✅ 已完成：2026-10-18（四段都在 `0056` 上做，Windows 11；驗收在 `progress.md`）
- 細節：`details.md`（1a 的 trace 的分類表、計時區間裡沒有等 Viz、Perf-Dashboard 的截圖和修正、1b 的原型和 `--window` 的確認、backend 的啟動時間、換 renderer 前後的 trace）；`wgpu-dx12-surface-report.md`（給 wgpu 的回報草稿，還沒送出）

## 摘要

做之前 kun 有兩個 Viz 的 renderer：
- 視窗用 `WgpuRenderer`（GPU）。
- headless、`--automation-pipe`（WebDriver、WPT、reftest、Speedometer 的量測）、沒有平台視窗的情境和 `shell` 的測試，用 `SoftwareRenderer`（CPU 逐像素合成）。

現在**每一種模式都用 `WgpuRenderer`**，`SoftwareRenderer` 拿掉了：
- 一個 process 共用一個 GPU 的 device（`viz` 的 `gpu_context`）。adapter 一律先用 GPU，沒有時才退回 wgpu 的 CPU adapter（Windows 的 WARP、Linux 的 lavapipe 或 llvmpipe）；`MACHOPO_GPU_ADAPTER=cpu` 強制用 CPU 的。
- backend 是各平台自己的：Windows 的 DX12、macOS 的 Metal、Linux 的 Vulkan 加 GL。
- 視窗畫進 swap chain，沒有視窗時畫進自己的 texture、從那裡讀回；tile 一律以 shared image 直接上傳到 device。
- kun 經過 CDP 的 `SystemInfo.getInfo` 回報 adapter；Speedometer 的報告記下它，`test.py` 在 CPU adapter 上降低 WPT 的平行度。

第一段另外查了 Perf-Dashboard：kun 根本沒畫 shadow tree 裡的 canvas，修掉了。

## 動機

**兩條路，只有一條有測試**：
- 使用者看到的是 `WgpuRenderer`，但它畫出來的東西沒有任何自動測試，畫圖只「在視窗裡手動確認」。`docs/test-gaps.md` 的「測試守不住的地方」有兩條就是這個：圓角的 fragment shader、照 `nearest_neighbor` 選 sampler。
- WPT、reftest、情境和 `shell` 的測試全部經過 `SoftwareRenderer`。同一件事（圓角的 coverage、混色、取樣）寫兩次，GPU 那一份壞了，測試看不出來。

**量測走的不是使用者的路**：
- `ci.py speedometer` 用 `--headless=new` 開 Chrome。它在 Windows 的開發機上用 GPU 合成：Skia Graphite、Dawn 的 D3D11（`rfcs/0012-speedometer/progress.md` 的「tile 的 raster 算不算進 Speedometer 的時間（2026-10-18，Windows 11）」）。kun 在 `--automation-pipe` 卻用 `SoftwareRenderer`，每個 frame 整個畫面重畫，也不拿掉被蓋住的 quad（1200×900 的 NewsSite 約畫 4 倍的面積）。
- 對分數的影響只有幾 %：
  - 草稿原本引用 RFC 0051 第 6 段之後的表：main thread 在計時區間裡等 Viz 比 Chrome 多 182 ms，以為換成 GPU 合成能拿回來。1a 的 trace 照 runner 的 mark 找計時區間，kun 的 main thread 在區間裡幾乎沒有閒置（20 個 suite 合計 3 ms），Viz 的 draw 都和 main thread 平行（`details.md` 的「計時區間裡沒有等 Viz」）。那 182 ms 是當時的計時區間算寬了，包進了 frame 之間的閒置。
  - 1b 暫時用真的視窗（wgpu）量了 Viz 的 draw 最多的 5 個 suite：十組比較都變快，合計 −3.3%。原因是 `SoftwareRenderer` 在 Viz thread 上和 main thread 搶 CPU 和記憶體頻寬，不是等待。第 4 段在 `main` 和 0056 上前後接著錄 trace：Viz 的 draw 少了 95%，kun 的計時區間扣掉 Perf-Dashboard 少 1.3%，在兩次之間的雜訊裡，只有 draw 最重的小 suite 明顯變快（`details.md` 的「第 4 段：換 renderer 前後的 trace」）。
- 所以換成 wgpu 的主要理由是量測和使用者走同一條路、兩邊用同一種合成；分數只是小幅變好。

**Perf-Dashboard**（第一段的 P4）：`SelectingRange` 一個 test 的 sync 是 71.4 對 9.6 ms，原因還沒查；2026-10-16 也記下了它的 rendering 時間比 Chrome 少，可能有東西沒畫。

**目標**（都做到了）：
- 每一種模式都經過 `WgpuRenderer`，WPT、reftest、情境和 `shell` 的測試都測到它。
- `SoftwareRenderer` 和它在 CPU 上的那一份圓角、取樣、混色拿掉，只留一份實作。
- Speedometer 量 kun 時和 Chrome 一樣用 GPU 合成；退回 CPU 的 adapter 時，量測警告。
- 測試的結果在不同的 GPU 上一致到測試需要的程度：reftest 兩邊經過同一個 renderer，逐點比顏色的測試給容許的差；報告記下 adapter，結果不同時找得到原因。
- Perf-Dashboard 的 `SelectingRange` 花在哪裡、有沒有少畫：少畫的修掉，剩下的排進之後的效能 RFC。

**不在範圍內**：
- partial swap（只重畫 damage）、拿掉被蓋住的 quad（「之後的可能」）。
- `BeginFrame` 跟著 vsync、不等 draw（Chromium 的 `DisplayScheduler`）。
- 把 tile 和 canvas 的 raster 搬到 GPU（OOP-R）：raster 照舊在 CPU 上用 Skia，這份只換合成。
- GPU process：Viz 和 GPU 的 device 照舊在 Browser process 裡。
- Rust 的 allocator（RFC 0053），和 RFC 0051 之後其他的效能項目（文字、canvas、style 的成本）。
- CI 的設定（已決定的事項第 8 條）。

解決的 `docs/limitations.md` 裡的限制（都已刪掉）：
- 「Viz、GPU 和記憶體」的 `SoftwareRenderer` 每個 frame 整個畫面重畫、不拿掉被蓋住的 quad。
- 「Paint 和 raster」的 Perf-Dashboard 的 rendering 時間比 Chrome 少。

`docs/test-gaps.md` 的「測試守不住的地方」裡解決的：`WgpuRenderer` 的 fragment shader 照圓角切 quad、照 quad 的 `nearest_neighbor` 選 sampler，兩者都有了自動測試。

新的限制（寫進「平台和建置」）：畫面一律經過 GPU 合成，沒有 GPU 也沒有 CPU adapter 的機器上 kun 畫不出東西（macOS 的虛擬機、沒有 Mesa 的 Linux）；CPU adapter 比 GPU 慢很多。

## 設計概要

網頁的行為不變。對使用 kun 和改程式的人：

- **每一種模式都經過同一個 renderer**，輸出照模式：

  | 模式 | 輸出 | 用 CPU adapter 時 |
  | --- | --- | --- |
  | 視窗（`kun`、`kun --automation`、`--automation-pipe --window`） | swap chain | 照樣畫，log 寫明（WARP 也能畫進視窗） |
  | `kun --headless`、`--automation-pipe`、沒有平台視窗的情境 | 自己的 texture，截圖、reftest、`capture` 從那裡讀回 | 照樣畫，log 寫明 |
  | `ci.py speedometer` 的 kun | 自己的 texture | 報告寫明並警告，和沒接電源時一樣 |
  | 測試的 harness（`test.py wpt`、`reftest`、`wdspec`） | 自己的 texture | 印出 adapter，WPT 的 process 數降到核數的四分之一並警告 |

  這和 Chrome 一樣：有 GPU 就用，`--headless=new` 也是；沒有時 Chrome 用 SwiftShader。

- **強制用 CPU 的 adapter**：環境變數 `MACHOPO_GPU_ADAPTER=cpu`（`kun --help`）。kun 的執行檔、process 內的測試和測試的 harness 都吃得到，用來重現沒有 GPU 的機器、測退回 CPU 的那條路（開發機上；CI 不跑）。
- **一個 process 一個 GPU device**：同一個 process 裡的所有 `Display` 共用一個 device（`GpuContext`），第一次要用時建立，留到 process 結束。對應 Chromium 一個 GPU process 服務所有的 compositor。
- **沒有 adapter 時**：建 renderer 失敗並說明原因，例如「沒有 GPU，也沒有 CPU 的 adapter：在 Linux 上裝 Mesa 的 lavapipe 或 llvmpipe」。沒有 CPU 的合成可以退回。
- **kun 回報 adapter**：`SystemInfo.getInfo`（CDP），照 Chrome 的格式。

## 詳細設計

### A. `WgpuRenderer` 不一定要有視窗（第 2 段）

- **輸出**有兩種：
  - 視窗的 surface：取 surface 的 texture、present。
  - 自己的 texture（`Rgba8Unorm`，`RENDER_ATTACHMENT | COPY_SRC`）：大小跟著 `resize`，不 present，copy request 一律從它讀回（`copy_texture_to_buffer`、`map_async`、等 `poll`）。
  - Chromium 的對應是 `SkiaOutputDevice`（視窗）和 `SkiaOutputDeviceOffscreen`（headless）。
- **process 共用的 `GpuContext`**（`viz` 的 `gpu_context`）：
  - 一個 wgpu instance（第一個使用者建立；視窗的帶著 display handle）和一個 context：adapter、device、queue、每種輸出格式一條 quad 的 pipeline、送 command 的 lock、1×1 的白色 texture。建了就留到 process 結束（1b 量到建一個要約 180 ms，測試的 binary 會開很多次瀏覽器）。
  - 視窗模式建它時帶著 surface 找 adapter；之後的 surface 不被它的 adapter 支援時記錯誤、另外建一個（保底）。
- **找 adapter**：列出平台 backend 上的 adapter，照類型挑（獨立的 GPU、內建的、其他、虛擬的，最後才是 CPU 的），同類取第一個；`MACHOPO_GPU_ADAPTER=cpu` 時只取 `Cpu` 的。這是純函式（`pick_adapter`），選到的寫進 log（`viz: using adapter`），用 CPU adapter 時另外警告。wgpu 預設的 `request_adapter` 在 `power_preference` 是 `None` 時不排序，照 backend 列出的順序取，所以自己挑。
- **backend**（已決定的事項第 7 條）：Windows 只開 DX12（WARP 也在裡面），macOS 開 Metal，Linux 開 Vulkan 加 GL。Windows 上啟動從約 190 ms 降到約 117 ms。
- **device 遺失**（`about:gpucrash`、驅動重置）：
  - 第一個要畫的 renderer 建下一代的共用 context，其他 renderer 畫的時候換過去，各自通知自己的 client 重新上傳 tile。對應 Chromium 的 GPU process 重啟時，所有 compositor 一起重建。
  - **DX12 的視窗要新的 surface**（第 2 段做的時候發現）：DX12 的 swap chain 屬於建它的 device，wgpu 換 device 重新設定 surface 時拿舊的 swap chain 做 `ResizeBuffers`，舊的 device 已經被移除，所以失敗；wgpu-core 只在 `Surface` 被丟掉時才放掉 swap chain（`wgpu-dx12-surface-report.md`）。所以 DX12 上 renderer 先換上新的 context、放掉舊的（舊的 device 上還有東西時，舊的 swap chain 不會真的被釋放，DXGI 不讓同一個視窗再建一個，`E_ACCESSDENIED`），再丟掉舊的 surface、換上新的。新的 surface 在建視窗的 main thread 上建（winit 只讓那條 thread 拿 window handle）：Viz thread 經過視窗的 `MakeSurface` 事件請它建好再傳回來，對應 Chromium 的 GPU process 從 browser 拿 surface。Vulkan 和 Metal 照舊沿用同一個 surface。
  - **讀回時 device 遺失**（第 3 段做的時候發現）：沒有平台視窗的情境在同一個 process 裡平行跑、共用 device，`gpu-crash` 弄丟它時，旁邊的情境可能正在讀回。`draw_frame_and_copy` 這時換到新的 device、重畫再讀，不讓 copy request 失敗（Chromium 的 copy request 在 GPU process 重啟之後也是重新要求）。沒有這一步，`test.py scenarios` 大約每三次有一次有別的情境讀回失敗。
- **tile 的資源**：compositor 有 shared image 的介面時（`GpuSharedImages`），tile 在 raster 之後直接上傳到共用的 device；沒有介面的 client（測試的分頁、還沒有 device 時）交 CPU 的像素，Viz 每個新的 resource 上傳一次。

### B. 每一種模式改用它（第 3 段）

- **`--automation-pipe`、沒有平台視窗的情境**：`windowless::start` 建 `BrowserWindow`，Viz 用 offscreen 的 `WgpuRenderer`，compositor 拿到 `GpuSharedImages`。它也登記一個 context lost 的 observer，`windowless::drive` 收到時讓工具列重新 raster，對應視窗的 `ContextLost` 事件。`capture`、`expect-differs`、`screenshot` 和 WebDriver 的截圖都經過讀回。
- **`kun --headless`**：保留自己的迴圈（自己帶著 `Display`、虛擬的時間），renderer 換成 offscreen 的 `WgpuRenderer`，`WebContents` 拿到 `GpuSharedImages`。最後的 `Frame` 是把最後一個 frame 再畫一次、經過 copy request 讀回；一個 frame 都沒有時是透明的。`frames_drawn` 不算讀回的那一次。
- **`SystemInfo.getInfo`**：`gpu.devices` 列裝置的名稱和驅動；kun 自己的 adapter 類型（`Cpu` 是 CPU adapter）和 backend 放在 `gpu.auxAttributes`。Viz 建好 renderer 之前 `devices` 是空的，問的一方等它出現。
- **`shell` 的測試**：`browser_view.rs` 用共用 device 的 offscreen `WgpuRenderer`，讀某一點的顏色經過讀回；工具列的 tile 走 `GpuSharedImages`，分頁的交 CPU 的像素（測試的 `WebContents` 不給介面），兩條路都測得到。`windowless.rs` 用 `windowless::start`。`wasm_reftests` 兩邊都用同一個 renderer 畫，照舊逐像素比。

### C. 量測和測試的 harness

- `ci.py speedometer` 先開一個 kun 問 `SystemInfo.getInfo`（`speedometer.kun_gpu`），報告的環境多一項「kun's GPU」，CPU adapter 或問不到時警告；`--against` 的報告也一樣。
- `test.py` 跑 wptrunner（`wpt`、`reftest`、`wdspec`）前也問一次，印出 kun 用的 adapter；是 CPU adapter 時 process 數降到核數的四分之一（至少 1）並警告（1b 量到 32 個 process 同時用 WARP 畫時，一個 frame 從 23 ms 變成 466 ms）。給了 `--processes` 時照給的。

### D. 拿掉 `SoftwareRenderer`（第 4 段）

- 刪掉 `software_renderer.rs` 和它的 export，`rounded_corner.rs` 只留 WGSL（CPU 上算 coverage 的那一半和它的測試刪掉）。
- `viz` 的 `Display` 和 Viz thread 的測試用假的 renderer：讀回時給 frame 的背景色，測 copy request 和 frame 的處理，不測畫圖。`SoftwareRenderer` 測 `Display` 行為的兩個（嵌入的 surface 有新的 frame 時重畫、忽略別的 sink 和過時的 surface）搬到 `display.rs`；其他的在 `WgpuRenderer` 的 `drawing_tests` 都有對應，或只測 `SoftwareRenderer` 自己的捷徑。

### E. Perf-Dashboard 的 `SelectingRange`（第一段的 P4）

數字在 `details.md` 的「Perf-Dashboard 的 `SelectingRange`」。
- 用暫時的 hook 在 Perf-Dashboard 的三個 test 之後經過 CDP 截圖（已決定的事項第 4 條）：kun 的 canvas 全部沒顯示。
- 原因：`renderer` 的 `canvas::flush_shown` 判斷 canvas 在不在文件裡時只沿 parent 往上找，停在 shadow root；Perf-Dashboard 的圖表都在 custom element 的 closed shadow root 裡。改成照 shadow-including root（DOM 的「connected」），連同 `renderer` 的單元測試和 reftest `canvas/c7-shadow-trees.html`。
- 修正之後 kun 每個 frame 都畫圖表，Perf-Dashboard 每個 iteration 多了約 24 ms：之前低估了 kun 該做的工作。
- `SelectingRange` 剩下的差距是 JS 和 canvas 的 2D API（28.8 對 5.4 ms），和 main thread 的 canvas raster（25.5 對 0 ms），都留給之後的效能 RFC。

### 測試

驗收的清單和指令在 `progress.md`。
- **`viz`**：`WgpuRenderer` 畫純色、tile、縮放的 texture（bilinear 和 `nearest_neighbor`）、圓角的 clip、clip 和 opacity、嵌入的 surface、改變大小，和期望的像素比；共用的 device 只建一次；device 遺失之後所有 renderer 換到下一代、client 都被通知；選到的 adapter 是要的那一種、在平台的 backend 上。GPU 上跑一次，開發機上再用 `MACHOPO_GPU_ADAPTER=cpu` 跑一次。
- **逐點比顏色的容許**：混色和在 texel 之間取樣的像素比到差 1 以內，其他完全相同。GPU 上和期望值完全相同；WARP 上 50% 的黑色疊在白色上是 127（期望 128）。
- **adapter 的選擇**：純函式的測試，給它一份假的 adapter 清單。
- **端到端**：WPT、reftest、情境和 `shell` 的測試全部在 `WgpuRenderer` 上跑；換 renderer 之後完整的 WPT 沒有新的意外結果（意外的 21 個在 `main` 上本來就有）。`SystemInfo.getInfo` 有 `shell` 的測試和 `speedometer_test.py` 的測試。
- **mutation test**：`mutants-1a.toml` 到 `mutants-4.toml`。沒抓到的記成等價的，或記進 `docs/test-gaps.md` 的「測試守不住的地方」：DX12 的視窗換 surface、讀回時 device 遺失就重讀、沒有平台視窗的工具列在 device 遺失之後重新 raster。

### 分階段

1. **基準和原型**（1a、1b）：Windows 上的 `--trace` 分類表和 Perf-Dashboard 的調查（shadow tree 裡的 canvas 修掉）；暫時的原型量 GPU、WARP 和 `SoftwareRenderer` 畫一個 frame 的時間、建 device 的時間、32 個 process 同時跑的負擔，`--window` 的確認。
2. **`WgpuRenderer` 不一定要有視窗**（A）。
3. **換掉使用者**（B、C），先 rebase 到 RFC 0053 做完的 `main`。
4. **拿掉 `SoftwareRenderer`**（D），換 renderer 前後的 trace，RFC 的收尾。

## 缺點

- **沒有 GPU 時沒有退路**：Windows 一定有 WARP，Linux 要有 Mesa（lavapipe 或 llvmpipe），macOS 的實體機都有 GPU，但虛擬機和某些 CI 的 macOS runner 沒有 Metal 的 device。Chromium 在這些地方退回 software compositing，kun 沒有。這是刻意和 Chromium 不同的地方（`docs/architecture.md`）。
- **測試變慢**：`shell` 的整合測試從 8.6 秒變成 15.2 秒（GPU 的啟動和讀回）。CPU adapter 比 `SoftwareRenderer` 慢一倍以上（仿 NewsSite 的一個 frame 23 對 11 ms），多個 process 同時跑時更慢，所以 harness 在那時降低平行度。
- **像素隨機器上的 GPU 不同**：不同廠商的驅動、CPU adapter 的混色捨入和取樣精度可能差 1。reftest 兩邊經過同一個 renderer、逐點的測試給容許的差、報告記下 adapter。以前 `SoftwareRenderer` 在每台機器都畫出一樣的像素，這份 RFC 放棄了這點，換來測試涵蓋使用者看到的路。
- **驅動的問題**：GPU 驅動和 WARP、lavapipe 的版本與 bug 會進到測試的結果；DX12 換 device 的那個 wgpu 的問題就是一例。
- **共用的 device**：一個 renderer 弄丟它，同一個 process 的其他 renderer 都跟著重建；讀回時的重試讓 copy request 不失敗，但在平行的測試裡弄丟 device 的測試只能放在情境裡。

## 理由和其他做法

| | 做法 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **R1** | 全部用 `WgpuRenderer`，拿掉 `SoftwareRenderer`（這份，使用者決定） | 一條路、一份實作；測試涵蓋使用者看到的；量測和 Chrome 一樣 | 沒有 GPU 時沒有退路；測試的速度和像素看 adapter |
| R2 | 量測和視窗用 wgpu，測試留 `SoftwareRenderer` | 測試的像素在每台機器都一樣，沒有 GPU 的機器照樣能跑 | 兩份實作照舊；GPU 的那一份照舊沒有自動測試 |
| R3 | 只改進 `SoftwareRenderer`（partial swap、拿掉被蓋住的 quad） | 改動最小 | 量到的照樣是使用者不走的路；兩份實作照舊 |

選 R1（已決定的事項第 2 條）。R2 留著兩份實作，GPU 那一份最容易壞、又最沒有測試。R3 只讓數字變好，不讓量測變得公平。

**adapter 一律先用 GPU、沒有才退回 CPU，測試也一樣**（第 3 條）：

| | 測試用的 adapter | 優點 | 缺點 |
| --- | --- | --- | --- |
| **G1** | 預設先用 GPU，沒有才退回 CPU（這份） | 測試走的就是使用者走的路；GPU 上快；每一種模式同一個規則 | 像素看機器上的 GPU |
| G2 | 測試強制用 CPU 的 adapter | 同一個平台上像素一樣（Chromium 在沒有 GPU 的 bot 上用 SwiftShader） | 測的是使用者幾乎不走的路；比較慢；macOS 沒有 CPU 的 adapter |

選 G1：測試的意義是測使用者看到的東西。G2 留成一個選項（`MACHOPO_GPU_ADAPTER=cpu`）。

**一個 process 一個 device**：測試會開很多次瀏覽器；Chromium 的 GPU process 本來就是一個 process 服務所有的 compositor，之後拆出 GPU process 時，共用的 context 就是它的雛形。

## 先例

- **Chromium**：
  - 合成在 GPU process 上用 `SkiaRenderer`。視窗用 `SkiaOutputDevice`，`--headless=new` 和有視窗的 Chrome 共用同一套 Viz，沒有視窗時用 `SkiaOutputDeviceOffscreen`。
  - 沒有 GPU 時用 SwiftShader 或退回 `viz::SoftwareRenderer`（software compositing）。kun 只照前半，不留 software compositing。
  - GPU 的 device 遺失時，GPU process 重啟，所有的 compositor 重建資源、copy request 重新要求。kun 用共用的 context 照做。
  - Windows 上用 Direct3D（ANGLE、Dawn 的 D3D11），kun 用 DX12。
  - 回報 GPU 的 CDP 指令是 `SystemInfo.getInfo`，kun 照它的格式。
- **Firefox**：WebRender 在沒有 GPU 時有自己的軟體版（SWGL），也是同一條 pipeline 的 CPU 實作，不是另一套合成器。和 kun 沒有 GPU 時退回 wgpu 的 CPU adapter 是同一個方向。

## 已決定的事項

1. **第一段的範圍**（2026-10-18，使用者決定）：
   - headless 不再每個 frame 整個畫面重畫（P3）、查 Perf-Dashboard 的 `SelectingRange`（P4）、Windows 上的 `--trace`。
   - P3 的做法由第 2 條取代；P4 和 trace 照舊在第一段（第 1a 段）。
2. **所有模式都用 `WgpuRenderer`，拿掉 `SoftwareRenderer`**（2026-10-18，使用者決定）：
   - 每一種模式照自己的需要設定 adapter 和輸出（「設計概要」的表）。
   - 不選只改進 `SoftwareRenderer` 的 partial swap 或拿掉被蓋住的 quad（R3）：量到的照樣是使用者不走的路。
   - 不選測試留著 `SoftwareRenderer`（R2）：兩份實作照舊，GPU 那一份照舊沒有自動測試。
   - 原本放在這份 RFC 的其他效能項目（文字、canvas、style 的成本），移到之後的效能 RFC。
3. **adapter 一律先用 GPU，沒有才退回 CPU**（2026-10-18，使用者決定）：
   - 每一種模式都一樣，連同測試和 CI。
   - 強制用 CPU 只是選項，不是預設。
   - 不選測試強制用 CPU 的 adapter（G2，「理由和其他做法」）：測的是使用者幾乎不走的路。
   - 同一天也確認 CI 的 Fedora runner 有 GPU（`docs/testing.md` 裡沒有 GPU 的那句是 GitHub Actions 時期寫的）。
4. **1a 的做法**（2026-10-18，使用者決定，1a 開始前）：
   - **P4 的截圖用暫時的 hook（S1）**，不 commit：
     - 注入 runner 的 module 包住 `benchmarkClient.didRunTest`。runner 會 `await` 它（`benchmark-runner.mjs` 的 `_recordTestResults`），而且它在這個 test 量完之後、iframe 拿掉之前才呼叫。
     - Perf-Dashboard 的三個 test（`Render`、`SelectingPoints`、`SelectingRange`）各自結束時，hook 通知工具，等工具回覆才 resolve。這段期間頁面照樣畫 frame。
     - 工具經過同一條 CDP 的 pipe 對兩個瀏覽器送 `Page.captureScreenshot`（kun 的 `--automation-pipe` 和 Chrome 都支援）。
     - 兩邊在同一步截圖，可以重跑。
   - 不選的做法：
     - 自己在 kun 的視窗和 Chrome 裡開 `InteractiveRunner.html` 逐步跑（S2）：手動、不能重跑，kun 的視窗走的也不是量測的那條路。
     - suite 跑完再截圖：runner 在 suite 結束時拿掉 iframe，截不到。
     - 把截圖做成工具的選項：要改 `speedometer.py`，RFC 0053 正在改它。
   - **trace 的分析用暫時的腳本**，不 commit：讀 trace 用 `speedometer.py` 的 Perfetto reader 和計時區間（`perfetto_slices`、`perfetto_marks`、`mark_windows`），分類照 RFC 0051 第 6 段之後那張表。
   - **量測照常在這台跑**（Windows 11，i9-13900K）。
5. **1b 的做法**（2026-10-18，使用者決定，1b 開始時）：
   - **`--window` 的確認**：只量 Viz 的 draw 和計時區間重疊最多的 5 個 suite（NewsSite-Nuxt、NewsSite-Next、WebComponents、Vue、Lit-Complex-DOM），`--browser kun`，headless 和 `--window` 交替各兩輪，每輪 10 個 iteration。暫時改 `speedometer.py` 的一行，量完還原。
   - **原型**：暫時的 `viz` 的 example（不 commit），用 wgpu 畫一個仿 NewsSite-Nuxt 的 frame 進 offscreen 的 texture 再讀回，和 `SoftwareRenderer` 畫同一個 frame 比。不改 `WgpuRenderer`：它現在一定要有 surface，改它是第 2 段的事。
   - **adapter**：這台的 AMD Radeon RX 7900 XTX 和 WARP；lavapipe 留到 Fedora 的機器或 CI 上。
   - **平行度**：同時開 32 個原型的 process（WPT 的預設），GPU、WARP 和 `SoftwareRenderer` 各一次。
   - **結果**：數字寫進 `details.md` 和 `progress.md`。
6. **第 2 段的設計**（2026-10-18，使用者決定，第 2 段開始前）：
   - **強制用 CPU 的 adapter 用環境變數 `MACHOPO_GPU_ADAPTER=cpu`**，寫進 `kun --help` 的環境變數表。kun 的執行檔、`cargo test` 裡在 process 內開瀏覽器的測試、CI 的 job 都吃得到，一個機制全部涵蓋（和 wgpu 自己的 `WGPU_POWER_PREF` 同一類）。不選命令列選項：process 內的測試吃不到命令列，還是要另一個環境變數。
   - **共用的 `GpuContext` 建了就留到 process 結束**：`shell` 的測試 binary 開關很多次瀏覽器，不必重建（1b 量到約 180 ms）。不選最後一個使用者放開時就釋放：省下的只是閒置時的 GPU 資源，代價是測試之間反覆重建。device 遺失時照樣重建它。
   - **一個 process 一個 context**，由第一個使用者建立。視窗模式建它時帶著 surface 找 adapter；之後的 surface 不被它的 adapter 支援時記錯誤、另外建一個（保底，一個 process 只會是有視窗或沒有視窗的一種）。
   - **backend 先量再決定**：第 2 段量全部的 backend 和只開平台主要的（Windows 的 DX12、macOS 的 Metal、Linux 的 Vulkan 加上 GL 做退路）的啟動時間，換 backend 時跑 `test.py automation` 確認。
   - **完全找不到 adapter 時測試失敗**，訊息說明怎麼裝驅動；不跳過，免得把環境的問題藏起來。
7. **backend**（2026-10-18，使用者決定，第 2 段量過之後；數字在 `details.md` 的「第 2 段：backend 的啟動時間」）：
   - **Windows 只開 DX12**（WARP 也在裡面），**Linux 開 Vulkan 加 GL**（lavapipe 在 Vulkan、llvmpipe 在 GL），**macOS 開 Metal**。
   - 理由：Windows 上只有一條路，和 Chromium 在 Windows 上用 Direct3D 一樣，不依賴各家的 Vulkan 驅動，CPU 的 adapter 也在同一個 backend 裡。Windows 的啟動從約 190 ms 降到約 117 ms。
   - 不選全部都開（約 190 ms，視窗在這台用的是 Vulkan）：多列舉兩個用不到的 backend。
   - 不選 Windows 先 Vulkan、沒有 GPU 才列舉 DX12（估計約 90 ms）：快約 27 ms（每個 process 一次），但要多維護一條退路，也和 Chromium 不同。
   - 這台的視窗因此從 Vulkan 換成 DX12，換完跑 `test.py automation` 確認。
8. **第 3 段的做法**（2026-10-18，使用者決定，第 3 段開始前）：
   - **先 rebase 到最新的 `main`**：RFC 0053 已經 merge，它改過 `crates/shell/src/main.rs`、根目錄的 `README.md`、`scripts/speedometer.py`，第 3 段在它之上改。
   - **沒有視窗的模式也給 compositor `GpuSharedImages`**：headless、`--automation-pipe`、沒有平台視窗的情境、`shell` 的測試，tile 在 raster 之後直接上傳到共用的 device，和視窗同一條路（Chromium 一律用 shared image）。不選照舊給 CPU 的像素、由 Viz 上傳：多一條只有沒有視窗時才走的路。
   - **`kun --headless` 保留自己的迴圈**（自己帶著 `Display`、虛擬的時間），只把 renderer 換成 offscreen 的 `WgpuRenderer`，最後的像素經過一次 copy request 讀回。不選改走 Viz thread：改動大，也會失去虛擬時間的確定性。
   - **kun 用 CDP 的 `SystemInfo.getInfo` 回報 adapter**（名稱、類型、backend），和 Chrome 回報 GPU 的同一個指令：`ci.py speedometer` 的報告記下它、退回 CPU 時警告；`test.py` 跑 WPT 和 reftest 前先問一次。不選只印在 log 裡、讓 harness 讀 stderr：脆弱，也和 Chrome 不同。
   - **退回 CPU adapter 時 WPT 的 process 數降到核數的四分之一**（至少 1），並印出醒目的警告（原本的待決定第 2 項）。
   - **逐點比顏色的測試**照已決定的規則：混色和在 texel 之間取樣的像素比到差 1 以內，其他完全相同。
   - **段落做完時跑完整的 `test.py wpt`、`reftest`、`scenarios`**（GPU 上；`reftest` 再用 `MACHOPO_GPU_ADAPTER=cpu` 跑一次），差異逐一確認：這次換的是每一張截圖都經過的合成。這是 `CLAUDE.md`「本機不跑整套 WPT」的例外，使用者同意。
   - **CI 不改**：`.gitlab-ci.yml` 不加強制用 CPU 的那一組；runner 的設定由使用者決定。「詳細設計」的 B 裡 CI 的那一點跟著改。
9. **第 4 段的做法**（2026-10-18，使用者決定，第 4 段開始前）：
   - **`viz` 的 `Display` 和 Viz thread 的測試用假的 renderer**：它們測的是 copy request 和 frame 的處理，不是畫圖；假的 renderer 讀回時給 frame 的背景色。不選換成 offscreen 的 `WgpuRenderer`：多依賴 GPU，測的東西和 `drawing_tests` 重複（Chromium 的 viz 單元測試也用假的 output surface）。
   - **`SoftwareRenderer` 的測試**：測 `Display` 行為的兩個（嵌入的 surface 有新的 frame 時重畫、忽略別的 sink 和過時的 surface）搬到 `display.rs`；其他的在 `drawing_tests` 都有對應，或只測 `SoftwareRenderer` 自己的捷徑，跟著刪掉。
   - **`rounded_corner.rs` 在 CPU 上算 coverage 的那一半連同它的測試刪掉**，只留 WGSL。
   - **trace 在 `main`（`85f27c57`，mimalloc 加 `SoftwareRenderer`）和 0056（mimalloc 加 wgpu）上前後接著錄**，只比 renderer 造成的差別：1a 的基準在 RFC 0053 之前，直接比會混進 allocator 的效果。不量 Speedometer 的比較表。
   - **RFC 做完時的整理放在這一段**：改寫成完成時的樣子、併 `progress.md`、文件的一致性；審核之後才 fast-forward merge 回 `main`，再 `cargo clean`。
   - **wgpu 在 DX12 上換 device 時沿用舊 swap chain 的問題寫成英文的回報草稿**，放在這個資料夾，給使用者審核；不對上游送出（`CLAUDE.md`）。

## 待決定

無。做的時候的兩項：
- 逐點比顏色的測試的容許：第 2 段量過，混色和在 texel 之間取樣的像素比到差 1 以內，其他完全相同（「詳細設計」的「測試」，已決定的事項第 8 條）。
- 沒有 Metal 的 macOS（虛擬機、某些 CI 的 runner）：不支援，寫進 `docs/limitations.md` 的「平台和建置」。

## 之後的可能

- **partial swap 和拿掉被蓋住的 quad**：`WgpuRenderer` 只重畫 damage（compositor 已經算好 `damage_rect`，aggregator 還沒合併）、Chromium 的 `Display::RemoveOverdrawQuads`。視窗模式也省 GPU 的時間和耗電。
- **`BeginFrame` 跟著 vsync、不等 draw**（Chromium 的 `DisplayScheduler`）。
- **GPU process**：共用的 context 搬到自己的 process，Viz 經過 IPC 用它。
- **tile 和 canvas 在 GPU 上 raster**（OOP-R）：Charts-chartjs、Perf-Dashboard 的 canvas。
- **之後的效能 RFC**：RFC 0051「之後的可能」的文字的 shaping 和斷行、F1、D8、後代的 invalidation set、`querySelector` 的捷徑、jQuery 剩下的 DOM 操作，照 1a 的分類表排（kun 多的時間 script 53%、style 28%、canvas 8%、parse 6%）；Perf-Dashboard 的 canvas 的 2D API 和 main thread 的 raster。
- **強制用 CPU 的測試放進 CI**（已決定的事項第 8 條沒做）；真視窗的情境在 CI 上跑（`test.py automation`，要 xvfb 或 headless 的 Wayland compositor）。
- **DX12 的 surface 問題回報給 wgpu**（`wgpu-dx12-surface-report.md`，使用者審核之後決定）；修好之後可以拿掉換 surface 的繞法。
