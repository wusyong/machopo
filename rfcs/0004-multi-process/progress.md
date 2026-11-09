# RFC 0004 的進度：multi-process 和 sandbox

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-06（這份 RFC 延後，只排第 0 步「整理邊界」，「已決定的事項」第 8 到 10 條）

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0a 邊界的規則和檢查（`docs/architecture.md` 的「Process 的邊界」、`sanity.py boundary`、`protocol` 的 `boundary` 測試） | 完成 | 2026-10-06 |
| 0b 修掉例外 2、4、5（context lost 的 `Weak` observer、`RendererFlags` 的函式指標、送給任何 `FrameSinkId` 的 sender；例外 3 改成不算例外） | 完成 | 2026-10-06 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| renderer 這一側的 crate 不依賴 Browser 這一側的；`protocol` 裡不能跨 process 的東西、renderer 這一側整個 process 共用的可變狀態，都只有允許清單上的例外 | `python3 scripts/sanity.py boundary` |
| 跨邊界的 message 都能序列化 | `cargo test -p protocol --test it boundary::` |
| renderer 這一側不提 `VizMsg`、不向 Viz 登記 context lost 的 observer；`sanity.py boundary` 擋得住每一種違規 | `python3 scripts/sanity.py boundary`；故意放進每一種違規（見下面的 0b） |
| GPU 遺失時只有 Browser 在看，它用 `CompositorMsg::ContextLost` 轉告，renderer 的 compositor 放掉所有 shared image；compositor thread 收到訊息就重畫 | `cargo test -p browser --test it navigation::`（`losing_the_gpu_drops_the_renderers_shared_images`）；`cargo test -p compositor --lib`（`a_main_frame_round_trip_through_the_compositor_thread`）；`python3 scripts/test.py automation`（`gpu-crash`） |
| 送給 Viz 的 sender 綁著自己的 `FrameSinkId` | `cargo test -p protocol`（`a_frame_sink_sender_speaks_for_its_sink`）；`cargo test -p browser --test it navigation::` |
| `RendererFlags` 能序列化，engine 另外給 | `cargo test -p renderer --lib`（`the_flags_can_cross_a_process`） |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 0a（2026-10-06，Windows 11）

- **做了什麼**：照 2026-10-06 重新核對的例外（`details.md` 的「現在的邊界」，加上那份盤點沒寫的 raster 全域快取）寫了 `docs/architecture.md` 的「Process 的邊界」；`sanity.py boundary`（CI 的 lint 也跑）和 `protocol` 的 `boundary` 測試；`CLAUDE.md` 的檢查清單和規則。

### 0b（2026-10-06，Windows 11）

- **做了什麼**（第 11 到 14 條）：
  - context lost：`protocol` 的 `CompositorMsg::ContextLost`；`RenderProcessHost` 向 Viz 登記 observer（`ContextLostForwarder` 從 compositor 搬到 Browser），收到時送給 renderer 的 compositor；compositor 不再登記。
  - `RendererFlags` 加上 serde（`dom_api::ScriptLanguage` 也是），`script_engine` 拿出來變成 `spawn_renderer_main_thread`、`RenderProcessHost::launch`、`WebContents::new` 的參數（`ScriptEngineFactory`）。
  - `protocol` 的 `CompositorFrameSinkSender`（`protocol` 多一個 `crossbeam-channel` 的依賴），`RendererEndpoints::viz` 改用它、拿掉 `frame_sink_id`；Browser 的工具列也用它。
  - `sanity.py boundary` 加上「renderer 這一側不提 `VizMsg`、不呼叫 `add_context_lost_observer`」；`docs/architecture.md` 的例外剩 1 和 6。
- **新的測試**：`browser` 的 `losing_the_gpu_drops_the_renderers_shared_images`（假的 GPU 記著它做的 shared image；載入頁面、等 tile 上了 GPU，讓它遺失：只有一個 observer，compositor 放掉全部）；`protocol` 的 `a_frame_sink_sender_speaks_for_its_sink`；`renderer` 的 `the_flags_can_cross_a_process`；compositor 的 `the_compositor_doesnt_watch_the_gpu` 取代原本測 observer 的測試，`a_main_frame_round_trip_through_the_compositor_thread` 改成送 `ContextLost`。
- **mutation test**（7 個；跑的期間使用者沒有 commit，跑完確認都還原了）：Browser 不登記、observer 沒留著、收到不轉告、compositor 不理 `ContextLost`、sender 用別的 id、Browser 綁別的 id，都抓到；`RendererFlags` 拿掉 serde 時測試編譯不過（編譯期的檢查本來就是這樣抓）。
- **`sanity.py boundary` 的故意違規**：依序放進一種違規再還原（renderer 依賴 `net`、renderer 提到 `VizMsg`、renderer 呼叫 `add_context_lost_observer`、`protocol` 的 `#[serde(skip)]`、`protocol` 的 closure、renderer 這一側的 `static Mutex`、允許清單上的 static 改名），7 種都被擋下。
- **途中的事**：使用者在這個目錄下了 `git restore .`，0a 和 0b 已經做的改動被還原；照留在 scratchpad 的腳本依序重新套用（每段改動都先確認原文還在），之後的檢查全部重跑。同一時間 `../third_party/rusty_v8` 的 checkout 不在 machopo 釘的 tag，`bindings` 的 build script 擋下建置；使用者把 fork 切回之後恢復。
- **真的視窗**：`test.py automation` 的 9 個情境都通過，`gpu-crash` 在真的 wgpu 上遺失 device，走新的 context lost 路徑重畫。
- **跑過的檢查**：`cargo fmt --check`、clippy（沒有警告）、`cargo test --workspace` 871 個全部通過（`wpt_testharness` 另計：476 個都和預期相符、102 個不跑；情境 18 次）、`sanity.py deps`、`unsafe`、`boundary`、`docs`。沒有動到 GC。
