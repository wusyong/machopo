# RFC 0056 的細節

## 1a 的 trace：計時區間裡的時間花在哪裡

2026-10-18，Windows 11，Intel Core i9-13900K（32 核）。`main` 的 `24d240d4`（V8 是 `kun-v152.2.0-11`）。`python3 scripts/ci.py speedometer --trace --iterations 3`：20 個 default suite 各錄一份，兩邊都是，Chrome for Testing 152.0.7977.82。

**方法**：暫時的腳本（不 commit）。
- 讀 trace 用 `speedometer.py` 的 Perfetto reader（`perfetto_track_events`、`perfetto_marks`）。
- 計時區間照 runner 的 mark（`mark_windows`：每個 test 從 `-start` 到 `-async-end`）。
- 算 renderer main thread（`CrRendererMain`）上每個 span 在區間裡的 self time，照 RFC 0051 第 6 段之後的分類加總：
  - script：`v8.*`、`FunctionCall`、`RunMicrotasks`、`EventDispatch`、`CustomElementReactionQueue`，含 JS 呼叫、沒有自己的 span 的 DOM API。
  - parse：`ParseHTML`、`HTMLDocumentParser::*`。
  - GC：`V8.GC_*`、`MinorGC`。
  - style：kun 的 `Document::UpdateStyle`、`StyleEngine::*`，Chrome 的 `Document::recalcStyle`、`Document::rebuildLayoutTree`、`UpdateLayoutTree`。
  - layout：kun 的 `Layout`，Chrome 的 `LocalFrameView::performLayout`、`InlineNode::*`。
  - paint：pre-paint、paint、`PaintArtifactCompositor`、`LayerTreeHost::*`、hit test。
  - canvas：`raster::draw_into`、`canvas::*`。
- 2026-10-18 的 trace 是在 Perf-Dashboard 的修正（「Perf-Dashboard 的 `SelectingRange`」）之前錄的：Perf-Dashboard 那一列的 canvas 偏低。

**每個 iteration 的 ms**（20 個 suite 合計，3 個 iteration 除以 3；trace 本身的負擔讓兩邊都比平常量的慢）：

| | script | parse | GC | style | layout | paint | canvas | 其他 | 合計 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| kun | 1015 | 184 | 15 | 389 | 157 | 77 | 94 | 59 | 1990 |
| Chrome | 417 | 114 | 22 | 73 | 139 | 56 | 0 | 49 | 871 |
| kun 多的 | 598 | 70 | −7 | 315 | 17 | 21 | 94 | 10 | 1119 |
| 佔差距 | 53% | 6% | −1% | 28% | 2% | 2% | 8% | 1% | |

**每個 suite 差最多的三類**（每個 iteration 的 ms，kun 多的）：

| suite | kun | Chrome | 差最多的 |
| --- | ---: | ---: | --- |
| TodoMVC-jQuery | 405.1 | 149.3 | script 156.3、parse 54.9、style 28.8 |
| Charts-chartjs | 170.4 | 43.8 | script 69.8、canvas 59.7 |
| Perf-Dashboard | 142.9 | 46.5 | script 61.8、canvas 34.4、style 10.5 |
| Editor-TipTap | 148.9 | 55.4 | style 62.0、layout 24.5、script 7.6 |
| ES6-Webpack-Complex-DOM | 133.4 | 44.6 | script 59.8、style 11.0、parse 10.3 |
| NewsSite-Next | 160.0 | 81.8 | style 36.4、script 32.7、layout 15.0 |
| React-Stockcharts-SVG | 126.3 | 63.8 | style 34.2、script 29.1 |
| ES5 | 84.7 | 53.9 | script 34.6、style 6.3、parse 4.7 |
| NewsSite-Nuxt | 122.1 | 61.4 | style 28.9、script 18.4、layout 18.3 |
| 其他 11 個 | 各 28–64 | 各 13–38 | script 和 style 各幾 ms 到十幾 ms |

### 計時區間裡沒有等 Viz

- kun 的 main thread 在 20 個 suite 的計時區間裡（合計 5972 ms，3 個 iteration）沒有被任何 span 蓋到的時間只有 3 ms；照 runner 的 frame 猜的區間（`frame_windows`）也是 2 ms。
- 同一段時間 Viz thread 的 `Display::DrawAndSwap` 和計時區間重疊的有 16–52 ms（每個 suite），但都和 main thread 的工作平行。
- runner 的 test 在 frame 的第一個 rAF 裡開始，async 結束在同一個 frame 之後的 timer。main thread 的 commit 只是排給 compositor、不等它（`rfcs/0012-speedometer/progress.md` 的「tile 的 raster 算不算進 Speedometer 的時間（2026-10-18，Windows 11）」）。等下一個 `BeginFrame` 的時間落在 test 和 test 之間，不計時。
- RFC 0051 第 6 段之後的表裡「等 Viz」的 182 ms（kun 338、Chrome 155），是當時的計時區間從任何一個 `FireAnimationFrame` 算起、包進了 frame 和 frame 之間的閒置。同一份紀錄指出那種區間多算了 39%。計時區間改照 runner 的 mark 之後，這一欄是 0。
- 所以 headless 換成 wgpu 拿不回那 182 ms。1b 用真的視窗量到的是另一個小一點的效果：`SoftwareRenderer` 和 main thread 搶 CPU（「1b 的確認：量測用真的視窗（wgpu）」）。

## Perf-Dashboard 的 `SelectingRange`

**截圖**（暫時的 hook，「已決定的事項」第 4 條）：kun 在三個 test 之後，所有的 canvas 都沒有出現在畫面上（主圖、右邊的縮圖、選取的範圍），文字照樣更新；Chrome 都有。

**原因**：
- `renderer` 的 `canvas::flush_shown` 在 frame 之前畫有新紀錄的 canvas，只畫在「顯示的文件裡」的。它判斷 canvas 在不在文件裡時，只沿 `parent()` 往上找 `Document`。
- shadow tree 裡的節點往上只走到 shadow root（shadow root 沒有 parent，host 不是），所以被當成不在文件裡。紀錄留著，等 script 讀像素或錄得太多才畫，畫了也不會換上新的內容。
- Perf-Dashboard 的圖表都在 custom element 的 closed shadow root 裡。
- 改成照 shadow-including root 判斷（`NodeRef::shadow_including_root`，DOM 的「connected」），open、closed、custom element 裡的都畫了。

**修正前後**（每個 iteration 的 ms，`--trace --iterations 3 --suites Perf-Dashboard`）：

| test | 修正前 kun | 修正後 kun | Chrome |
| --- | ---: | ---: | ---: |
| `Render` | 19.6 | 22.5 | 10.5 |
| `SelectingPoints` | 48.5 | 79.8 | 21.5 |
| `SelectingRange` | 74.9 | 64.6 | 14.8 |
| 合計 | 143.0 | 166.9 | 46.8 |

- 修正前前兩個 test 根本沒畫 canvas，紀錄到 `SelectingRange` 才因為錄得太多一次畫掉。修正後每個 frame 都畫，合計多了約 24 ms：之前的 Perf-Dashboard 低估了 kun 該做的工作。
- 修正後 `SelectingRange` 的大項：
  - `FunctionCall` 28.8 對 5.4 ms：JS 和 canvas 的 2D API。
  - main thread 的 canvas raster（`raster::draw_into`）25.5 對 0 ms：Chrome 的 canvas 錄成 paint op、交給 GPU process 畫。
  - style 和 layout 各幾 ms。
- 這個 test 每次 mouse move 之後都叫 `serviceRAF`，同步跑 app 的 rAF，所以一個 test 裡重畫圖表 22 次。
- 剩下的兩項是之後的效能 RFC 的範圍：canvas 的 raster 移出 main thread（RFC 0051 的 F1），和 canvas API 本身的成本（要在 Linux 或 macOS 上取樣）。

## 1b 的原型：GPU、WARP 和 `SoftwareRenderer`

2026-10-18，Windows 11，i9-13900K，AMD Radeon RX 7900 XTX（wgpu 選了 Vulkan），WARP 是 DX12 的 Microsoft Basic Render Driver。`main` 的 `90e8b4ac`。

**原型**：暫時的 `viz` 的 example（不 commit），release build。
- 畫一個仿 NewsSite-Nuxt 的 frame：1200×900 的輸出、77 個 256×256 的 tile，分四層，約 4 倍的面積，後面一層不透明、其他部分半透明，1:1 放在整數位置。
- GPU 和 WARP 用 wgpu 畫進 offscreen 的 texture（一個 pipeline、每個 tile 一次 draw、premultiplied 的混色），讀回是複製到 buffer、map、等 `poll`。
- `SoftwareRenderer` 經過 `Display` 畫同一個 frame，讀回是複製它的像素。

**一個 process**（60 個 frame 的中位數；建 device 前後的時間是第二次以後跑的，第一次冷啟動時 instance 300 ms、device 158 ms）：

| | instance | adapter | device | 上傳 77 個 tile | 一個 frame | frame 加讀回 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| GPU | 107 ms | 56 ms | 20 ms | 18 ms | 0.12 ms | 1.3 ms |
| WARP | 105 ms | 56 ms | 16 ms | 403 ms | 22.6 ms | 23.4 ms |
| `SoftwareRenderer` | — | — | — | — | 10.7 ms | 11.3 ms |

- 建一個 GPU 的 device 前後約 180 ms，主要是 instance（列舉 Vulkan、DX12、GL 三個 backend）和找 adapter。共用 device 省得了一個 process 裡的重複，跨 process 的省不了；只開需要的 backend 可能省下一部分（第 2 段再量）。
- WARP 畫一個 frame 是 `SoftwareRenderer` 的兩倍，上傳慢 20 倍：退回 CPU 的 adapter 比現在的 `SoftwareRenderer` 慢。

**32 個 process 同時跑**（WPT 的預設平行度；每個都連續畫 30 個 frame、每個都讀回，比 WPT 實際的負擔重；每個 process 的中位數再取中位數）：

| | instance + adapter + device | 一個 frame 加讀回 | p95 | 牆上時間 |
| --- | ---: | ---: | ---: | ---: |
| GPU | 約 1.2 s（最多約 2 s） | 10.7 ms | 18 ms | 2.1 s |
| WARP | 約 0.96 s | 466 ms | 528 ms | 15.8 s |
| `SoftwareRenderer` | — | 約 31 ms（畫 28.6、讀回 2.7） | 約 43 ms | 1.3 s |

- GPU 上 32 個 process 搶同一個 GPU 的 queue，每個 frame 慢到約 11 ms，照樣比 `SoftwareRenderer` 快。
- WARP 在 32 個 process 搶 CPU 時，一個 frame 慢了 20 倍。沒有 GPU 而退回 CPU 的機器上，WPT 的平行度要降低（「待決定」）。
- lavapipe 這台沒有，留到 Fedora 的機器或 CI 上量。

## 1b 的確認：量測用真的視窗（wgpu）

暫時讓 `ci.py speedometer` 用 `kun --automation-pipe --window` 開 kun（不 commit），`--browser kun`，5 個 Viz 的 draw 和計時區間重疊最多的 suite，每輪 10 個 iteration，headless 和 `--window` 交替各兩輪。`--window` 那兩輪的 log 確認用的是 RX 7900 XTX。

| suite | headless（兩輪） | `--window`（兩輪） | 變化 |
| --- | --- | --- | ---: |
| TodoMVC-WebComponents | 54.2、55.3 | 52.5、50.7 | −5.8% |
| TodoMVC-Vue | 36.7、37.4 | 36.3、35.3 | −3.4% |
| TodoMVC-Lit-Complex-DOM | 39.4、39.2 | 39.0、37.7 | −2.4% |
| NewsSite-Next | 158.7、156.6 | 156.7、155.1 | −1.1% |
| NewsSite-Nuxt | 123.3、120.2 | 119.5、117.8 | −2.5% |

- 十組比較都是 `--window` 比較快，5 個 suite 合計 −3.3%。各自的 95% 區間有重疊，但方向一致。
- 1a 量到 main thread 在計時區間裡沒有等 Viz，所以這不是等待：`SoftwareRenderer` 每個 frame 在 Viz thread 上用 CPU 畫約 4 倍畫面的像素，和 main thread 搶記憶體頻寬和快取。換成 GPU 之後 Viz thread 只錄指令，這份競爭沒了。
- 其他 suite 的 Viz 的 draw 比較少，效果應該更小。所以換成 wgpu 讓分數小幅變好，幅度是幾 %，不是 RFC 0051 的表以為的 182 ms。

## 第 2 段：backend 的啟動時間

2026-10-18，Windows 11，AMD Radeon RX 7900 XTX。暫時的 `viz` 的 example（不 commit）：建 instance、列舉 adapter、在第一個 GPU（沒有就第一個 adapter）上建 device，release build，各跑三次，取後兩次（第一次是冷啟動）。

| backend | instance | 列舉 adapter | device | 合計 | 列出的 adapter |
| --- | ---: | ---: | ---: | ---: | --- |
| 全部（現在） | 約 117 ms | 約 55 ms | 約 20 ms | 約 190 ms | Vulkan、DX12、WARP、GL |
| Vulkan | 約 50 ms | 約 1.5 ms | 約 21 ms | 約 73 ms | Vulkan |
| DX12 | 約 15 ms | 約 69 ms | 約 32 ms | 約 117 ms | DX12、WARP |
| GL | 約 88 ms | 0.1 ms | 約 48 ms | 約 136 ms | GL |

- 現在什麼都開，wgpu 照 backend 的順序列出（Vulkan、Metal、DX12、GL），這台的視窗和 offscreen 都用 Vulkan。
- Windows 上的 CPU adapter（WARP）只在 DX12 裡：只開 Vulkan 就沒有退路。
- 只開 Vulkan 和 DX12、先列舉 Vulkan，只有 Vulkan 沒有 GPU 時才列舉 DX12，預期約 90 ms（兩個 instance 約 65 ms，加上 Vulkan 的列舉和 device）。
- 決定（已決定的事項第 7 條）：Windows 只開 DX12，Linux 開 Vulkan 加 GL，macOS 開 Metal。換成 DX12 之後，這台的視窗也從 Vulkan 換成 DX12；device 遺失時 DX12 的視窗要換新的 surface（README 的「A. `WgpuRenderer` 不一定要有視窗」）。

## 第 4 段：換 renderer 前後的 trace

2026-10-18，Windows 11，i9-13900K，AMD Radeon RX 7900 XTX。`ci.py speedometer --trace --iterations 3`，20 個 default suite，前後接著錄兩份：
- `main` 的 `85f27c57`：mimalloc，headless 和 `--automation-pipe` 用 `SoftwareRenderer`。
- 0056 的 `4432158c`：mimalloc，所有模式用 wgpu（DX12）。

照 1a 的方法算，每個 iteration 的 ms：

| | kun 的計時區間 | Viz 的 draw 和計時區間重疊 | main thread 在區間裡閒置 | Chrome 的計時區間 |
| --- | ---: | ---: | ---: | ---: |
| `main`（`SoftwareRenderer`） | 1779 | 190 | 1.0 | 897 |
| 0056（wgpu） | 1781 | 9.6 | 2.9 | 877 |

- **Viz 的 draw 少了 95%**：GPU 合成時，Viz thread 只錄指令、交給 GPU。
- **kun 的合計幾乎不變**，但 Perf-Dashboard 從 125.4 變成 148.8 ms（+18.7%）。`main` 還沒有 1a 的修正：shadow tree 裡的 canvas 在 `main` 上根本不畫，0056 上每個 frame 都畫（canvas 那一類 97 → 122 ms）。
- **扣掉 Perf-Dashboard，kun 1654 → 1632 ms（−1.3%）**。Viz 的 draw 最重的小 suite 變最多：WebComponents −9.2%、Preact −7.4%、Svelte −5.6%、React-Redux −5.2%；其他在 ±3% 內。
- 同一段時間 Chrome 自己也變了 −2.2%（Lit-Complex-DOM 一個 suite −22%），所以整體的 −1.3% 在兩次之間的雜訊裡，只有那幾個小 suite 的變化比較明顯。這和 1b 用真的視窗量到的一致：分數小幅變好，原因是 `SoftwareRenderer` 不再和 main thread 搶 CPU。

