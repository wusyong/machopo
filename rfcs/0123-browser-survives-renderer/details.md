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

## 沒有查到的

- Chromium 的 Viz 對 client 的 frame 驗證哪些（`components/viz/service/frame_sinks/compositor_frame_sink_support.cc`、mojo 的 `StructTraits`）；cc 和 GPU 的記憶體預算。
- wgpu 的 error scope 在 native 後端的成本、錯誤之後 device 和 encoder 的狀態。
- lavapipe 上記憶體不夠時的行為和真的 GPU 一不一樣。
