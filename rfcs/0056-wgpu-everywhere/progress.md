# RFC 0056 的進度：所有模式都用 wgpu 的 renderer，拿掉 `SoftwareRenderer`

開始日期：2026-10-18。完成：2026-10-18，Windows 11（Intel Core i9-13900K，AMD Radeon RX 7900 XTX；CPU adapter 是 WARP）。在 `0056` 上做，第 3 段之前 rebase 到 RFC 0053 做完的 `main`（`85f27c57`）；四段一起審核。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a Windows 上的 `--trace` 分類表、Perf-Dashboard 的 `SelectingRange` 的調查（shadow tree 裡的 canvas 不顯示，修掉） | 完成 | 2026-10-18 |
| 1b 原型的量測：GPU、WARP、`SoftwareRenderer` 畫一個 frame 和建 device 的時間、32 個 process 同時跑、`--window` 的確認 | 完成 | 2026-10-18 |
| 2 `WgpuRenderer` 不一定要有視窗：offscreen 的輸出、adapter 的選擇、process 共用的 device、各平台的 backend、DX12 的視窗換 surface | 完成 | 2026-10-18 |
| 3 換掉使用者：`--automation-pipe`、情境、headless、`shell` 的測試，`SystemInfo.getInfo`、量測的報告、`test.py` 的平行度（CI 不改） | 完成 | 2026-10-18 |
| 4 拿掉 `SoftwareRenderer`、換 renderer 前後的 trace、RFC 的收尾 | 完成 | 2026-10-18 |

## 驗收

### GPU 的 device 和 adapter（`viz` 的 `gpu_context`）

| 項目 | 怎麼驗證 |
| --- | --- |
| adapter 有 GPU 就用（獨立的在內建的前面、同類取第一個），沒有才退回 CPU 的；`MACHOPO_GPU_ADAPTER=cpu` 只取 CPU 的，其他的值照預設 | `cargo test -p viz --lib gpu_context::` |
| 選到的 adapter 是要的那一種，在平台的 backend 上（Windows 的 DX12、macOS 的 Metal、Linux 的 Vulkan 或 GL） | `cargo test -p viz --lib drawing_tests::the_adapter_is_the_one_asked_for`；再用 `MACHOPO_GPU_ADAPTER=cpu` 跑一次 |
| 一個 process 的 renderer 共用一個 device | `cargo test -p viz --lib drawing_tests::renderers_share_the_process_device` |
| device 遺失時第一個畫的 renderer 換到下一代，所有 renderer 跟著換，client 都被通知，shared image 上新的 device | `cargo test -p viz --lib drawing_tests::a_lost_device_is_made_again_for_every_renderer` |
| DX12 的視窗在 device 遺失之後換一個新的 surface（main thread 建）、畫面照樣更新 | `python3 scripts/test.py automation` 的 `gpu-crash` 情境（Windows） |

### 畫圖和讀回（`WgpuRenderer`）

| 項目 | 怎麼驗證 |
| --- | --- |
| 畫進自己的 texture、讀回：純色、tile、混色、最近的 texel 和雙線性取樣、圓角、clip、opacity、嵌入的 surface、改變大小；混色和在 texel 之間取樣的像素差 1 以內，其他完全相同 | `cargo test -p viz --lib drawing_tests::`（GPU 和 `MACHOPO_GPU_ADAPTER=cpu` 各一次） |
| quad 的 shader（連同圓角）是合法的 WGSL | `cargo test -p viz --lib shader_tests::` |
| `Display` 和 Viz thread 處理 copy request：等第一個 frame、重畫、只回答一次、畫進「沒有東西」時等下一個；嵌入的 surface 有新的 frame 時重畫；別的 sink 和過時的 surface 不畫 | `cargo test -p viz --lib display:: thread::` |

### 每一種模式（`shell`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `--automation-pipe`、沒有平台視窗的情境、`kun --headless` 都用 offscreen 的 `WgpuRenderer`，tile 走 shared image，像素經過讀回 | `cargo test -p shell`；`python3 scripts/test.py scenarios`；`python3 scripts/test.py reftest`（GPU 和 `MACHOPO_GPU_ADAPTER=cpu`） |
| 工具列的 tile 走 shared image、分頁的交 CPU 像素，兩條路畫出來都對 | `cargo test -p shell --test it browser_view::` |
| `SystemInfo.getInfo` 回報畫圖的 adapter（裝置、類型、backend） | `cargo test -p shell --test it automation_pipe::the_system_info_tells_the_gpu_adapter` |
| 視窗模式照舊（DX12） | `python3 scripts/test.py automation` |

### 量測和測試的 harness（`scripts/`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `speedometer.kun_gpu` 問到 kun 的 adapter（GPU 和 `MACHOPO_GPU_ADAPTER=cpu`）；起不來的 kun 是 `None` | `python3 scripts/speedometer_test.py KunTests.test_kun_tells_its_gpu ComparisonTests.test_kun_on_the_cpu_adapter_is_a_problem` |
| 報告的環境有「kun's GPU」，CPU adapter 或問不到時警告 | `python3 scripts/speedometer_test.py ComparisonTests` |
| `test.py` 在 CPU adapter 上把 WPT 的 process 數降到核數的四分之一（至少 1） | `python3 scripts/wpt_expectations_test.py WptProcesses` |

### shadow tree 裡的 canvas（`renderer`）

| 項目 | 怎麼驗證 |
| --- | --- |
| shadow tree 裡（open、closed、custom element 的）的 canvas，frame 之前照樣畫、顯示出來 | `cargo test -p renderer --lib canvas::tests::a_frame_draws_the_canvases_in_shadow_trees_it_shows`；`python3 scripts/test.py reftest canvas/`（`c7-shadow-trees.html`） |

## 做過的檢查

2026-10-18，Windows 11，i9-13900K，AMD Radeon RX 7900 XTX。數字和比較在 `details.md`。

- **量測**：
  - 1a 的 `--trace`（20 個 suite）：kun 比 Chrome 多 1119 ms（每個 iteration），script 53%、style 28%、canvas 8%、parse 6%；main thread 在計時區間裡合計只閒置 3 ms，沒有等 Viz（RFC 0051 表裡的「等 Viz 182 ms」是計時區間算寬了）。
  - 1b 的原型：GPU 一個 frame 0.12 ms（加讀回 1.3 ms）、WARP 22.6 ms、`SoftwareRenderer` 10.7 ms；建 device 約 180 ms；32 個 process 同時跑時 GPU 10.7 ms、WARP 466 ms。`--window` 和 headless 交替量 5 個 suite：十組都是 `--window` 比較快，合計 −3.3%。
  - backend 的啟動：全部約 190 ms、DX12 約 117 ms、Vulkan 約 73 ms（沒有 WARP）、GL 約 136 ms。
  - 第 4 段在 `main`（`SoftwareRenderer`）和 0056（wgpu）上前後接著錄 trace：Viz 的 draw 和計時區間重疊的時間 190 → 9.6 ms；kun 的計時區間扣掉 Perf-Dashboard 1654 → 1632 ms（−1.3%，同一段時間 Chrome 變了 −2.2%）。
- **Perf-Dashboard**：暫時的 hook 在三個 test 之後經過 CDP 截圖，kun 的 canvas 全部沒顯示；最小的頁面重現是 shadow tree 裡的 canvas。修正之後和 Chrome 畫得一樣；每個 iteration 143.0 → 166.9 ms（Chrome 46.8）：之前前兩個 test 根本沒畫 canvas。
- **做的時候找到的問題**：
  - 換成 DX12 之後真視窗的 `gpu-crash` 失敗：wgpu 換 device 時拿舊的 swap chain 做 `ResizeBuffers`（舊的 device 已經被移除）；換新的 surface 時 DXGI 回 `E_ACCESSDENIED`（renderer 手上的舊 context 讓舊的 swap chain 還活著）；新的 surface 在 Viz thread 上又建不了（winit 只在 main thread 給 window handle）。最後：先換上新的 context，丟掉舊的 surface，新的由 main thread 建。暫時拿掉新 surface 那一步，`gpu-crash` 照樣失敗。給 wgpu 的回報草稿在 `wgpu-dx12-surface-report.md`。
  - `test.py scenarios` 有時有情境讀回失敗：情境共用 device，`gpu-crash` 弄丟它時旁邊的情境正在讀回。讀回時 device 遺失就換新的、重畫再讀之後，連跑五次都通過（之前三次裡兩次各有一個失敗）。
  - `speedometer_test.py` 的 `CompareTests` 用假的 kun 路徑，`kun_gpu` 起不來時丟出 `FileNotFoundError`：改成回 `None`。
  - rebase 到 RFC 0053 做完的 `main`：根目錄 `README.md` 的環境變數那一句、`kun --help` 的測試清單兩處衝突，兩邊都留。
- **測試的結果**：
  - `test.py reftest`：168 個，GPU 18.8 秒、WARP（8 個 process）28.2 秒，都沒有意外的結果。`test.py scenarios`：56 次都通過。`test.py automation`：每一段都跑過，最後一次 44 個情境和 `pipe --window` 都通過（看過 `rounded`、`gpu-crash` 的截圖）。
  - 完整的 `test.py wpt`（GPU，4 分 50 秒）：11604 個測試裡 19 個意外的結果（13 個意外通過），加上 3 個 TIMEOUT，全是 testharness.js 的測試，沒有 reftest；在 `main`（`SoftwareRenderer`）上跑同樣的 22 個是同樣的 21 個：`main` 上本來就有。
  - 逐點比顏色：GPU 上和期望值完全相同；WARP 上 50% 的黑色疊在白色上是 127（期望 128），所以混色和在 texel 之間取樣的像素比到差 1 以內。
  - `shell` 的整合測試從 8.6 秒變成 15.2 秒（GPU 的啟動和讀回）。
- **mutation test**：
  - `mutants-1a.toml`（shadow tree 裡的 canvas）：3 個都被抓到。
  - `mutants-2.toml`（`GpuContext`、adapter、offscreen 的輸出、backend）：11 個裡 10 個；instance 不限平台的 backend 沒被抓到：選到的一樣，只多花啟動時間，記成等價的。
  - `mutants-3.toml`（`SystemInfo.getInfo`、headless 的讀回、`windowless`、工具）：7 個裡 5 個；`windowless` 不給 shared image（畫出來一樣，記成等價的）、`windowless` 不把 device 遺失轉給工具列（記進「測試守不住的地方」）沒被抓到。
  - `mutants-4.toml`（搬到 `display.rs` 的兩個測試）：3 個都被抓到。
  - 只有時序或真視窗測得到的，記進 `docs/test-gaps.md` 的「測試守不住的地方」：DX12 的視窗換 surface、讀回時 device 遺失就重讀、沒有平台視窗的工具列在 device 遺失之後重新 raster。
- **每一段做完時**：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（第 2 段 3229 個、第 3 段 3231 個、第 4 段 3226 個，都沒有失敗）、`MACHOPO_GPU_ADAPTER=cpu cargo test -p viz --lib`、`python3 scripts/speedometer_test.py`（78 個，2 個 skip：`--sample` 只在 Linux 和 macOS）、`sanity.py` 的 `docs`、`tests`、`unsafe`、`deps`、`boundary` 都通過。
- **沒有做的**：lavapipe（Linux 的 CPU adapter）和 macOS 的 Metal 都還沒跑過；CI 沒有強制用 CPU 的那一組（已決定的事項第 8 條）。
