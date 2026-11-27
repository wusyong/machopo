# RFC 0123 的細節：renderer 送什麼都不能讓 Browser 結束

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-27 讀的（`main`，`146db413`）。

### renderer 的 frame 怎麼到 wgpu

1. renderer 的 compositor thread 把 `CompositorFrameSinkMsg`（`protocol::viz`：`SubmitCompositorFrame`、`CreateSharedImage`、`DestroySharedImage`、`SetNeedsBeginFrame`、`DidNotProduceFrame`）寫進控制通道。
2. Browser 的 IO thread（`browser` 的 `render_process_host.rs`）解開，包成 `VizMsg::FrameSink(frame_sink_id, msg)` 送給 Viz thread；sink 的編號是 Browser 綁的，renderer 說不了別人的。解不開的是 `BadMessageReason::Channel`。
3. Viz thread（`viz` 的 `thread.rs`）交給 `Display`（`display.rs`）：`submit_compositor_frame`、`create_shared_image`。
4. `SurfaceAggregator` 把嵌的 surface 的 quad 合進 root 的 frame（resource 的編號在這裡換成合起來之後的，各 client 的不會撞），`WgpuRenderer`（`wgpu_renderer.rs`）畫。

一則訊息最多 128 MiB（`ipc` 的 `MAX_PAYLOAD_LEN`），一條通道最多排 256 MiB（`MAX_QUEUED_BYTES`）；shared image 的像素在共用記憶體的 region 裡，不算在訊息裡。

### Viz 現在檢查的

| 值 | 檢查 | 在哪 |
| --- | --- | --- |
| sink | 不是 root、也沒登記過的：丟掉 | `Display::submit_compositor_frame`、`create_shared_image` |
| 帶像素的 resource | 每邊不超過 `MAX_TEXTURE_SIZE`、像素的長度等於 checked 的寬 × 高 × 4（`pixels_fit`）；不過的整個 frame 丟掉 | `bad_resource` |
| shared image | 大小不是空的、`pixels_fit`、同一個 sink 的同一個 mailbox 沒建過 | `create_shared_image` |
| 空的 resource（寬或高是 0） | 不上傳 | `WgpuRenderer::sync_resources` |
| quad 的 `shared_quad_state` 索引 | 越界用預設值 | `SurfaceAggregator`、`WgpuRenderer` 的 `get(..).unwrap_or_default()` |
| quad 提到的 resource | frame 裡沒有的不畫 | `WgpuRenderer` |
| clip | 和輸出的範圍取交集、轉成整數時飽和；寬或高是 0 的不畫 | `WgpuRenderer::scissor` |
| surface 嵌自己 | 停 | `SurfaceAggregator` 的 `path` |

不過的時候都是丟掉和記錯誤，不回報給 Browser。

### 讀程式找到、還沒確認的

- **quad 的數量**：`WgpuRenderer` 把一個 frame 的 quad 排成一個頂點的 buffer（一個 quad 六個頂點、每個頂點 80 bytes，程式裡的註解），`create_buffer_init` 一次建。wgpu 預設的 `max_buffer_size` 是 256 MiB，約 56 萬個 quad；一則 128 MiB 的訊息放得下更多（一個 `DrawQuad` 幾十 bytes）。超過時預期是驗證錯誤。
- **GPU 的記憶體**：一個 shared image 最多 `MAX_TEXTURE_SIZE` 見方，數量沒有上限；用完時 wgpu 回報 OutOfMemory（uncaptured 的話是 panic）。
- **浮點數**：quad 的 `rect`、`tex_coord_rect`、clip、圓角可以是 NaN 或無限大：進頂點的資料，預期不會是驗證錯誤（畫不出東西）；scissor 那條路轉整數時飽和。要用測試確認。
- **wgpu 的錯誤**：`gpu_context.rs` 只有 `set_device_lost_callback`；沒有 `push_error_scope`、`on_uncaptured_error`。wgpu（30）預設的 uncaptured handler 是 panic。
- **panic 之後**：`viz`、`browser`、`shell` 沒有 `catch_unwind`（只有 `raster` 的圖片解碼和 playback 有）；workspace 沒有設 `panic = "abort"`。Viz thread panic 時 Browser 的其他 thread 還在，之後有沒有 frame、會不會結束，沒有試。

### Browser 處理 `RendererHostMsg` 的那一側

- 入口：IO thread 的 `IoThreadHost`（`render_process_host.rs`：子資源、cookie、字型、blob URL、worker）和 UI thread 的 `WebContents`（其餘）。不合格的是 `BadMessageReason`，`WebContents::received_bad_message` 結束那個 renderer、重設 frame 樹。
- 測試以外的 `unwrap`、`expect`、`panic!`、`unreachable!`（2026-11-27 逐一讀過字面，還沒有逐一論證）：

| 檔案 | 幾處 | 是什麼 |
| --- | --- | --- |
| `render_process_host.rs` | 6 | 都是 `self.loads.lock().expect("the loads")`：鎖被 poison 才會（別的 thread 拿著它 panic 過） |
| `navigation_controller.rs` | 8 | session history 的項目「剛找到」「剛建立」「entry 的」：內部的不變式 |
| `frame_tree.rs` | 3 | `about:blank` 解得開（常數）、`at` 那個位置有東西 |
| `dom_storage.rs` | 2 | 記憶體裡的資料庫打得開、收得下 schema |
| `web_contents.rs` | 1 | 作業系統給得出亂數 |
| `loader.rs` | 0 | |

  沒有一處是直接解 renderer 給的值；要論證的是「renderer 的訊息順序破不破得了那些不變式」（`navigation_controller.rs` 的那 8 處最需要看）。索引和整數運算有 workspace 的 lint（`arithmetic_side_effects`、`cast_*`），切片的索引沒有 lint。
- **fuzzer**：`crates/browser/fuzz/fuzz_targets/renderer_message.rs` 呼叫 `crates/browser/tests/it/renderer_message_fuzz.rs`：`RendererLauncher::Detached`（沒有 renderer），輸入是一串「renderer 送的訊息」，每一則照通道上來的走（先 IO thread 的物件、再分頁），中間穿插 Browser 的動作（導覽、renderer 不在了）；找的是 panic。找到的輸入在 `crates/browser/fuzz/regressions/`，`cargo test -p browser` 重放。CI 不跑 fuzzer 本身（`docs/test-gaps.md` 的「缺少的工具和檢查」）。它不涵蓋 `CompositorFrameSinkMsg`（那些不經過 `WebContents`）。

### 會 panic 的寫法有多少（量的）

2026-11-27，`main` 的 `146db413`。workspace 對 `unwrap`、`expect`、索引沒有 lint，文件也沒有規則（有的是整數運算的四個 lint、`unsafe` 的範圍、`clippy.toml` 禁用的幾個方法）。用 clippy 數，只編 lib 和執行檔（不含 `#[cfg(test)]` 的模組和 `tests/`），命令列暫時打開 lint，沒有改檔案：

```sh
cargo clippy --workspace --lib --bins --message-format=json -- \
  -W clippy::unwrap_used -W clippy::expect_used -W clippy::indexing_slicing \
  -W clippy::panic -W clippy::unreachable -W clippy::todo -W clippy::unimplemented
```

合計 2,922 處：索引 1,663、`expect` 896、`unreachable!` 278、`unwrap` 62、`panic!` 23、`todo!` 和 `unimplemented!` 0。

| 在哪一側 | crate（處） |
| --- | --- |
| renderer 這一側（panic 是那個分頁的 sad tab） | `layout` 520、`dom_api` 464、`dom` 352、`paint` 292、`css` 278、`dom_js` 259、`renderer` 164、`dom_wasm` 53、`html` 41、`bindings` 39、`style` 38、`animation` 34、`raster` 21、`compositor` 18、`compositor_paint` 17、`canvas` 16、`style_resolver` 14、`computed_style` 13、`gfx` 13 |
| Browser 這一側，renderer 的輸入走得到的 | `browser` 43（`expect` 22、索引 21；`navigation_controller.rs` 28、`render_process_host.rs` 6、`frame_tree.rs` 4、`dom_storage.rs` 2、`unique_name.rs` 2、`web_contents.rs` 1）、`viz` 9（`wgpu_renderer.rs` 8、`gpu_context.rs` 1）、`protocol` 12（DevTools 和 automation 的訊息，不是 renderer 的）、`ipc` 4 |
| Browser 這一側的其他 | `driver` 53、`shell` 52、`ui` 43、`net` 31 |

上面「Browser 處理 `RendererHostMsg` 的那一側」的表只數了 `expect` 的字面（20 處），這裡的 22 處是 clippy 數的，另外有 21 處索引。

### wgpu 的錯誤處理

2026-11-27 讀的（cargo registry 裡的 wgpu 30.0.1）：

- `wgpu::Error` 的文件：「By default, errors translate into panics. Depending on the backend and circumstances, errors may occur synchronously or asynchronously. When errors need to be handled, use `Device::push_error_scope()` or `Device::on_uncaptured_error()`」。三種：`OutOfMemory`、`Validation`（「signifying a bug in code or data provided to `wgpu`」）、`Internal`（「any failures not explicitly expected by WebGPU ... internal implementation or system limits being reached」）。
- `Device::push_error_scope(filter)` 回一個 guard，`pop()` 回一個 future（那段期間的錯誤，有的話）；一個 scope 接一種（`ErrorFilter`），可以疊；沒 pop 就 drop 的 guard 會 pop、錯誤丟掉。堆疊在有 `std` 時是每條 thread 一份（原始碼的錯誤訊息：「Error scopes are thread-local」）。
- `Device::on_uncaptured_error(handler)`：沒有被 scope 接到的錯誤交給它；沒裝時是預設的 handler（panic）。
- **不經過錯誤回報的 panic**：`util::DeviceExt::create_buffer_init`（`src/util/device.rs`）先 `create_buffer`（`mapped_at_creation`），再對 buffer `get_mapped_range_mut(..).expect("Failed to get mapped range for buffer created with mapped_at_creation")`：buffer 建不出來（例如超過 `max_buffer_size`）時，錯誤進了 scope 或 handler，這個 `expect` 照樣 panic。照程式讀的，沒有跑過。`viz` 的 `WgpuRenderer` 用它建每個 frame 的頂點 buffer。其他呼叫有沒有同樣的情形，第 0 步逐一看。

## 第 0 步的探測

2026-11-27，`0123` 的 `1c2c8825`，Linux（Fedora 45）。程式在 `probe.rs`（`viz` 的 `wgpu_renderer.rs` 最後一個測試模組 `include!` 它）和 `probe_shell.rs`（`crates/shell/tests/it/automation_pipe.rs` 最後 `include!` 它；另外暫時讓 `viz` 的 `thread.rs` 在 `MACHOPO_PROBE_VIZ_PANIC` 時收到 renderer 的 frame 就 panic）；跑完拿掉了，不是 build 的一部分。兩個 adapter：AMD Radeon 890M（RADV），和 CI 用的 CPU adapter（`MACHOPO_GPU_ADAPTER=cpu`：llvmpipe）。

### 上限

兩個 adapter 都是 `max_buffer_size` 256 MiB、`max_texture_dimension_2d` 16384：machopo 要的是 `Limits::downlevel_defaults().using_resolution(..)`（`gpu_context.rs`），buffer 的上限是 WebGPU 預設的 256 MiB，和 adapter 無關。一個頂點 80 bytes，一個 quad 六個：一個 frame 最多 559,240 個 quad。

### wgpu 的錯誤

| 做了什麼 | 結果（兩個 adapter 一樣） |
| --- | --- |
| 現在的 Viz 畫 559,241 個 quad 的 frame | panic：預設的 uncaptured handler，「In Device::create_buffer, label = 'viz quads' Buffer size 268435680 is greater than the maximum buffer size (268435456)」 |
| 同上，包著三層 error scope、裝了 uncaptured handler | scope 接到 `MaxBufferSize` 的驗證錯誤；接著 `create_buffer_init` 照樣 panic（`util/device.rs:72`，「Failed to get mapped range for buffer created with mapped_at_creation: MapRangeError(...Buffer with 'viz quads' label is invalid)」）：照程式讀的那條確認了 |
| `create_buffer` 超過上限（沒用 `create_buffer_init`）、texture 超過 16384、texture 寬是 0 | 都是 scope 接到的驗證錯誤；`get_mapped_range_mut` 回 `Err`；handler 沒收到東西；之後同一個 device 畫的 frame 照樣對 |
| quad、clip、圓角、`tex_coord_rect`、`opacity`、`device_scale_factor`、`output_rect` 是 NaN、±無限大、±`f32::MAX`、-0 的 36 種組合 | 36 個都畫了，沒有任何錯誤 |
| 每個 frame 三層 error scope（200 個 quad） | push 和 pop 三層：RADV 0.25 µs、llvmpipe 0.18 µs；一個 frame 90–100 µs（RADV）、385–490 µs（llvmpipe），有沒有 scope 的差在雜訊裡 |

`pop()` 回的 future 在 native 後端當場就好（`pollster::block_on` 不等）。

### 記憶體不夠

CPU adapter，4096 見方的 texture 一個 64 MiB，一個接一個建、寫像素（`write_texture`，Viz 的 `upload` 也是這條；`create_shared_image` 的像素在一般記憶體時也走它）：

- 不限制：20 個沒有錯誤。
- `ulimit -v` 12 GB：100 個沒有錯誤；再多就是 **segfault**（SIGSEGV，不是 panic），在 wgpu-core 的 `Queue::write_texture`（`device/queue.rs:999`）把像素複製進 staging buffer 的 `memmove`，目的地是 `0xffffffffffffffff`（mmap 失敗時的 `MAP_FAILED`）。沒有 OutOfMemory 的錯誤，scope 和 handler 都沒收到東西。像是驅動（llvmpipe）沒檢查 mmap 的失敗就把位址交出來，沒有對 Mesa 的原始碼確認。
- 真的 GPU 沒試：890M 是內顯，它的記憶體就是這台機器的，用完時整台機器都受影響。

所以 GPU 記憶體用完時，至少有一個 adapter 是 process 直接當掉：error scope 擋不住，只有不讓它用完（shared image 的預算）擋得住。

### wgpu 的呼叫失敗時

讀 wgpu 30.0.1 的 native 後端（`src/backend/wgpu_core.rs`）：

| 呼叫（Viz 用到的） | 失敗時 |
| --- | --- |
| `create_buffer`、`create_texture`、`create_view`、`create_bind_group`、`create_command_encoder`、render pass 的 `set_*` 和 `draw`、`finish`、`Queue::submit`、`write_texture`、`write_buffer`、`copy_*`、`present`、`Surface::configure`、`map_async`、`unmap` | 進 device 的錯誤（`handle_error`、`handle_error_nolabel`）：error scope 或 uncaptured handler。render pass 裡的錯誤在 pass 結束或 `finish` 時才回報 |
| `get_mapped_range`、`get_mapped_range_mut` | 回 `Result`（Viz 的 `stage` 和讀回的路已經處理 `Err`） |
| `util::DeviceExt::create_buffer_init` | **直接 panic**（上面的探測）：每個 frame 的頂點 buffer 用它 |
| `Device::poll` | 不是 `PollError` 的錯誤 **直接 panic**（`handle_error_fatal`）；Viz 的 `finish` 和讀回呼叫它，`Err` 已經處理 |
| `Surface::get_current_texture` | 沒有 error sink 時 **直接 panic**（`handle_error_fatal`）；Viz 先 `configure` 才拿，有 sink |
| `Buffer::slice`、`map_async` 的範圍和對齊 | API 層的 `# Panics`：範圍由 Viz 自己算（`slice(..)`），renderer 管不到 |

### Viz thread panic 之後

`viz` 的 `thread.rs` 沒有接 panic；thread 結束之後，送給它的 `viz.send(..)` 都回錯誤，Browser 用 `let _ =` 丟掉。

- **headless（automation pipe）**：探測裡，Viz 在 renderer 的第一個 frame panic，Browser 約 1 秒內結束，exit status 1，記錄「Viz can't draw: Viz has ended without a renderer」（`windowless.rs` 的 `context_lost.failed`：它的 sender 跟著 Viz 的 renderer 走，renderer 跟著 unwind drop 了）。renderer 的 process 跟著正常結束。也就是說，Viz 的 panic 現在等於 Browser 結束：G1 要拿掉的正是這個。
- **視窗**：照程式讀（沒有在視窗裡試）：失敗的通知是 renderer 的 `on_failure` 的 closure 送的 `ShellEvent::GpuFailed`，drop 的時候不送；視窗留著、不再畫，輸入照常到分頁。

### renderer 的 compositor 送多少

照程式讀（`compositor` 的 `layer_tree_host_impl.rs`）：

- quad：每個 picture layer 的每個可見的 tile 一個（tile 256 px 見方），每個 surface layer 一個，加上顏色單一的 tile。誠實的 frame 是幾百到幾千個；每個元素都是自己的 layer（`will-change` 一萬個）時是幾萬個。
- shared image：每個 tile 一個，保留 interest rect 裡的（可見的範圍往四邊各擴大 `max(寬, 高)`）；沒有記憶體的預算。4K 的 viewport（3840 × 2160 個像素）滿版的一個 layer：interest rect 11,520 × 9,840，約 1,755 個 tile、450 MiB。

## Chromium 怎麼做

2026-11-27 讀的（`chromium/src` 的 `main`）。

### tile 的記憶體預算：renderer 自己守

- Blink 的 `third_party/blink/renderer/platform/widget/compositing/layer_tree_settings.cc` 的 `GetGpuMemoryPolicy` 給 renderer 的 cc 一個預算 `bytes_limit_when_visible`：桌面最少 512 MB（`kDefaultMemoryMB`），照螢幕的像素放大（2056 × 1329、scale 2 時 1152 MB，`kLargeResolutionMemoryMB`），最多實體記憶體的四分之一；Android 96 MB（低階、或記憶體不到 2 GB）或 256 MB；`--force-gpu-mem-available-mb` 蓋過。`priority_cutoff_when_visible` 是 `CUTOFF_ALLOW_NICE_TO_HAVE`（`cc/trees/managed_memory_policy.cc`：`ALLOW_PREPAINT_ONLY`）。resource 的個數另有 `kDefaultNumResourcesLimit` 一千萬。
- `cc/trees/layer_tree_host_impl.cc`：hard limit 是那個預算，soft limit 是它的 `max_memory_for_prepaint_percentage`（預設 100）%。`cc/tiles/tile_manager.cc` 照 tile 的優先順序排：「現在要畫的」用 hard limit，其他（prepaint）用 soft limit；超過時先 evict 優先順序低的 tile 的 resource，還是放不下的「現在要畫的」tile 是 `OOM_MODE`（不 raster，畫成單色）。
- 這是 renderer 自己的設定：GPU process 那一側沒有對應的檢查（下面）。

### Viz 收 frame：檢查形狀，不數 quad

- 解開的時候（mojo 的 `StructTraits`）：`services/viz/public/cpp/compositing/compositor_frame_mojom_traits.cc`：render pass 不是空的、最後一個 pass 的 `output_rect` 不是空的、view transition 提到的 pass 在 frame 裡、capture 的範圍在 frame 裡；`compositor_render_pass_mojom_traits.cc`：pass 的 id 不是 0、`subtree_size` 不超過 `output_rect`、每個 quad 的種類認得、每個 quad 有 shared quad state（沒帶的用上一個，第一個 quad 一定要帶）。沒有 quad 數的上限。解不開是 mojo 的驗證錯誤；那條 pipe 在 GPU process 裡，之後怎麼處理 renderer 沒有讀。
- 交出去的時候（`components/viz/service/frame_sinks/compositor_frame_sink_support.cc` 的 `MaybeSubmitCompositorFrame`）：`LocalSurfaceId` 合法、frame 不是空的（`INVALID_FRAME`）、不是 root 的不能帶 display transform、`BeginFrameAck` 合法、copy request 是不是允許、surface 的序號沒有變小、surface 是這個 client 的、hit test 的資料合法、大小和 surface 的一樣（`SIZE_MISMATCH`）。
- 不過的時候（`compositor_frame_sink_impl.cc`）：`DLOG(ERROR)`，`ResetWithReason` 關掉那個 `CompositorFrameSink` 的 pipe：不是從這裡結束 renderer；renderer 的那一端看到斷線。

### GPU process 建 shared image：失敗就停掉那個 client 的 channel

- `gpu/ipc/service/shared_image_stub.cc`：參數不合（格式、用途、上傳資料的大小）或 `CreateSharedImage` 失敗（記憶體不夠也是）都是 `OnError()`，也就是 `channel_->Stop()`：那個 client 的整條 GPU channel 停掉（renderer 的 context 不見了，自己重來：照記憶，沒有讀 renderer 那一側）。`memory_tracker_` 記大小，沒有看到每個 client 的上限。
- GPU process 自己當掉時 Browser 重開它：GPU 的記憶體用完傷不到 Browser。machopo 的 Viz 和 GPU 在 Browser 的 process 裡，這一點沒有對應的。

### GPU process 的記憶體：記帳，不設上限

2026-11-27 讀的：
- 每個 client 有一個 `gpu::MemoryTracker`（`gpu/command_buffer/service/memory_tracking.h`：`TrackMemoryAllocatedChange`、`GetSize`），`GpuChannelManager::GpuPeakMemoryMonitor`（`gpu/ipc/service/gpu_channel_manager.cc`）合計所有的，用來記錄和量。`SharedImageStub`、`SharedImageManager`、`SharedImageFactory`、`GpuChannel` 裡沒有看到超過多少就拒絕的檢查。
- 有記憶體壓力時（`OnMemoryPressure`、`OnApplicationBackgrounded`）清 cache（`SharedContextState::PurgeMemory`、Skia 的 `PurgeAllCaches`）。
- context 一直遺失時（第五次，或和上一次在五秒以內：`force_restart`）重開整個 GPU process。
- GPU process 被作業系統因記憶體不夠結束時，Browser 記下來（`content/browser/gpu/gpu_process_host.cc` 的 `PROCESS_WAS_KILLED_BY_OOM`）、再開一個。
- 所以張數的上限在 renderer：cc 的 `ManagedMemoryPolicy::kDefaultNumResourcesLimit` 一千萬。

## 第 3 段的量測

2026-11-27，同一台（Fedora 45，AMD Radeon 890M）。一次性的腳本（沒有放進 repo）照 `scripts/bench/raster` 的做法：`machopo --remote-debugging-pipe --headless --scale 2`（開發的 build），頁面載入、等 2 秒，捲動 120 次（滾輪，一次 100 px），改視窗大小 21 次（到 680 x 500），讀 Viz 的 debug log：每個 client 的 shared image 到新高時的位元組和張數（`viz: a client's shared images at their most yet`）、每個 frame 的 quad 數（`viz: frame arrived` 的 `quads`）、compositor 超過預算的 tile（`prepared tiles` 的 `over_budget`）。頁面是 `raster` 的三個（Gutenberg、SQLite、Speedometer 的 NewsSite-Next），和兩個刻意做的：2,000 個捲動容器（machopo 裡捲動容器才成為 layer，`will-change` 不會）、1,500 段的長頁面。預算在每一次都是 512 MiB：headless 的「螢幕」是啟動時的視窗，不是 `Browser.setWindowBounds` 之後的。

分頁（client 1）的峰值，視窗 1280 x 800 / 1920 x 1080：

| 頁面 | 第一次（A1、S1 之前） | 超過預算的 tile | 第二次（A1、S1 之後） | 超過預算的 tile | 一個 frame 最多的 quad |
| --- | --- | --- | --- | --- | --- |
| Gutenberg | 89 / 182 MB，340 / 695 張 | 0 / 0 | 60 / 123 MB，227 / 468 張 | 0 / 0 | 70 / 135 |
| SQLite | 34 / 44 MB，138 / 170 張 | 0 / 0 | 17 / 22 MB，70 / 88 張 | 0 / 0 | 70 / 120 |
| NewsSite-Next | 160 / 259 MB，630 / 1,041 張 | 2,595 / 7,863 | 127 / 207 MB，502 / 827 張 | 0 / 0 | 330 / 517 |
| 捲動容器 | 921 / 1,010 MiB，3,997 / 4,570 張 | 0 / 2,721 | 474 / 512 MiB，2,152 / 2,310 張 | 0 / 2,563 | 626 / 1,273 |
| 長頁面 | 140 / 294 MB，540 / 1,140 張 | 0 / 0 | 71 / 153 MB，270 / 585 張 | 0 / 0 | 70 / 135 |

Browser 的 UI（client 0）：第一次 5.2 / 7.9 MB、20 / 30 張，第二次 2.6 / 3.9 MB、10 / 15 張。

- 第一次的捲動容器在改視窗大小時用到預算的將近兩倍：那一次 `PrepareTiles` raster 了 2,257 張新的（tile 的位置都變了），舊的 active tree 還拿著它的那批到 activate（已決定的事項第 12 條）。NewsSite 的超過預算是還沒 raster 的單色 tile 也算了一整張（第 13 條）。第一次的 binary 還有「貼著可見範圍的 tile 當成可見的」那個 bug（`progress.md`）。
- 第二次（第 12、13 條之後）每一頁都在預算以內；只有刻意做的捲動容器在大的視窗剛好到預算（512 MiB），放掉了 2,563 張預先畫的 tile。
- 所以 Viz 的上限照暫定的：每個分頁是預算的兩倍（512 MiB 時 1 GiB：最多的頁面是它的一半）、65,536 張（最多 2,310 張，28 倍）、所有的合計是實體記憶體的一半（這台 31 GiB）；quad 的上限（262,144）是最多的 200 倍。

## 沒有查到的

- Chromium 的 mojo 驗證錯誤在 GPU process 的 pipe 上之後怎麼處理（renderer 有沒有被結束）；renderer 在 GPU channel 停掉之後怎麼重來。
- 真的 GPU 上記憶體不夠時的行為（內顯的記憶體是整台機器的，沒試）；llvmpipe 的 segfault 是不是 Mesa 的 bug。
