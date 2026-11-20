# RFC 0004 的進度：multi-process

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-06（第 0 步「整理邊界」，之後延後）；2026-11-20 排程，從第 1 階段開始

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0a 邊界的規則和檢查（`docs/architecture.md` 的「Process 的邊界」、`sanity.py boundary`、`protocol` 的 `boundary` 測試） | 完成 | 2026-10-06 |
| 0b 修掉例外 2、4、5（context lost 的 `Weak` observer、`RendererFlags` 的函式指標、送給任何 `FrameSinkId` 的 sender；例外 3 改成不算例外） | 完成 | 2026-10-06 |
| 排程之前：重新盤點（`details.md` 的「現在的邊界」）、macOS 的原型（`prototype/`） | 完成 | 2026-11-20 |
| 1a：Browser 處理一個 renderer 的訊息的邏輯從 IO thread 的迴圈抽成物件（行為不變） | 完成 | 2026-11-20 |
| 1b-1：`protocol` 的三則訊息；Viz 照 frame sink 和編號記 shared image、收到就上傳、還緩衝區，frame 經過表找 texture（舊的直接呼叫還在） | 完成 | 2026-11-21 |
| 1b-2：compositor 的緩衝區的池子、raster 直接畫進去，tile 和 canvas 快照改走訊息，複製次數的計數 | 未開始 | |
| 1b-3：拿掉舊的介面和交給 renderer 的 Viz 的物件，改 `FakeGpu`、`DrivenViz`，消掉例外 1 | 未開始 | |
| 1c：`protocol` 的 bytes 型別（子資源的回應、worker 的 script、blob 的內容） | 未開始 | |
| 1d：第 1 階段的檢查和文件 | 未開始 | |
| 2：`ipc` crate（核心、平台層、驗證的工具） | 未開始 | |
| 3：啟動 child process | 未開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| renderer 這一側的 crate 不依賴 Browser 這一側的；`protocol` 裡不能跨 process 的東西、renderer 這一側整個 process 共用的可變狀態，都只有允許清單上的例外 | `python3 scripts/sanity.py boundary` |
| 跨邊界的 message 都能序列化 | `cargo test -p protocol --test it boundary::` |
| renderer 這一側不提 `VizMsg`、不向 Viz 登記 context lost 的 observer；`sanity.py boundary` 擋得住每一種違規 | `python3 scripts/sanity.py boundary`；故意放進每一種違規（見下面的 0b） |
| GPU 遺失時只有 Browser 在看，它用 `CompositorMsg::ContextLost` 轉告，renderer 的 compositor 放掉所有 shared image；compositor thread 收到訊息就重畫 | `cargo test -p browser --test it navigation::`（`losing_the_gpu_drops_the_renderers_shared_images`）；`cargo test -p compositor --lib`（`a_main_frame_round_trip_through_the_compositor_thread`）；`python3 scripts/test.py automation`（`gpu-crash`） |
| 送給 Viz 的 sender 綁著自己的 `FrameSinkId` | `cargo test -p protocol`（`a_frame_sink_sender_speaks_for_its_sink`）；`cargo test -p browser --test it navigation::` |
| `RendererFlags` 能序列化，engine 另外給 | `cargo test -p renderer --lib`（`the_flags_can_cross_a_process`） |
| Browser 對一個 renderer 的訊息做的事是一個只有一個入口的物件（`IoThreadHost`），不用 renderer 和 thread 就能測：不是它的事的訊息原樣轉給 UI thread；沒給的 loader 的請求、文件的請求、沒給的 loader 的 cookie 讀取照原因回報；worker 的 blob URL 跟著 worker 走、通知 UI thread，不是它的 worker 的不登記；通道關掉時 blob URL 收回、最後回報 `RendererGone`。renderer 自己結束時分頁收到 `RendererGone` | `cargo test -p browser --lib io_thread_host`；`cargo test -p browser --test it a_renderer_that_ends_on_its_own` |
| Viz 的 shared image：client 用自己的編號建立，frame 只畫自己的 client 建的（別的 client 同號的、沒建的都沒有）；銷毀之後，畫它的 frame 還在時 texture 留著、frame 換掉才放掉；client 不在了它的跟著走；不建的情況（不能送 frame 的 sink、不是那個大小的像素、同號建兩次、沒有 GPU）；一批只 flush 一次；像素的緩衝區在 Viz 放開之後才通知 client；真的 GPU 上畫得出來 | `cargo test -p viz --lib shared_image` |

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

### 排程之前：重新盤點和原型（2026-11-20，macOS、Apple M4）

- **重新盤點**：照 `main` 的 `dcd1b61` 把 renderer 和 Browser、Viz 之間的每一條路列了一遍（`details.md` 的「現在的邊界」和「要改的地方」）。過不了 process 的：tile 和 canvas 快照上 GPU（例外 1）、導覽的 body 的管道（例外 7）、trace；Browser 處理 renderer 的訊息的邏輯要先從 IO thread 的迴圈抽出來。已經照邊界做好的：Browser 送給 renderer 的訊息的分派、同步的呼叫、worker 的訊息、loader 的核對。
- **原型**（`prototype/`，它的 README 有每一項的數字）：設計在 macOS 的 sandbox 裡跑得起來；三種 process 不用改結構；量了控制通道、大的資料、frame 的編解碼、ring、同一個 process 裡的 crossbeam。抓 frame 和量訊息的間隔時暫時在 `protocol`、`renderer`、`compositor`、`browser` 加了記錄用的鉤子和兩份情境，量完都拿掉了（`git status` 確認只剩 RFC 和文件的改動）。
- **量 machopo 實際的訊息有多密**：用 machopodriver 照人的節奏操作（每 16 ms 一個滾輪事件）；先用情境的執行器量到的是 80 個滾輪事件在 20 ms 內送完，不是人的節奏，只當成爆量的情況。
- **照結果定的**：第 19 條（訊息不走 ring）、第 20 條（`ipc` 留在 machopo、照 elise 驗證）、第 21 條（body 的管道走控制通道）、第 22 條（postcard、audit token、大的資料走留著重複用的 region）、第 23 條（tile 經過共用記憶體）；第 18 條加上第 3 階段的 sandbox 探測。
- **沒有原型的**：Linux 和 Windows 的平台層，到第 2 階段才第一次寫。
- **跑過的檢查**：`sanity.py` 的 `docs`、`boundary`（改了 `sanity.py` 和 `protocol` 的註解：body 的管道之後怎麼跨 process）。

### 1a：處理訊息的邏輯抽成物件（2026-11-20，macOS、Apple M4）

- 第 1 階段分成四段（1a 到 1d，上面的表）。1a 沒有要決定的事；「單一 process 時要不要連 Browser 的 IO thread 也拿掉」留著那條 thread、行為不變，之後量過再定。「Browser 的 IO thread 要不要合成一條」排到第 3 階段決定。
- **做了什麼**：`RenderProcessHost::launch` 裡 `Chrome_IOThread` 的迴圈（約 250 行，自己處理 12 種訊息、其餘轉給 UI thread、結束時收尾）搬進同一個檔案的 `IoThreadHost`（Chromium 的 `RenderProcessHostImpl::IOThreadHostImpl`）：`on_message` 是唯一的入口，`on_channel_closed` 是收尾。IO thread 只剩「從 channel 讀出來、交給它」四行。程式是照原樣搬的（區域變數改成欄位、`continue` 改成 `return`），沒有改判斷。
- **新的測試**：`browser` 的單元測試 2 個，直接呼叫 `on_message`，沒有 renderer 也沒有 thread；整合測試 1 個（`a_renderer_that_ends_on_its_own_is_reported_gone`：engine 一建就 panic 的 renderer，分頁收到 `RendererGone`）。
- **找到的測試缺口**（原本就有，不是這次造成的）：mutation test 把 IO thread 結束時的收尾拿掉，沒有測試失敗。`about:crash` 和 sad tab 的測試走的是分頁自己知道 renderer 被結束的路（`kill` 之後不再轉它的訊息），renderer 自己結束、通道關掉才回報 `RendererGone` 的那條路沒有人守。補了上面那個整合測試。
- **mutation test**（`mutants-1a.toml`）10 個：第一次跑在第 2 個停下（上面的缺口），補了測試之後重跑，全部抓到。跑完確認檔案都還原了。
- **文件**：`docs/architecture.md` 的 `browser` 那一列（`IoThreadHost`）。
- **跑過的檢查**：`cargo test -p browser`（36 個單元測試、175 個整合測試通過）、`cargo clippy -p browser --all-targets -- -D warnings`、`cargo fmt --check`；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。沒有動到視窗那一層。

### 1b-1：Viz 的 shared image 的表和三則訊息（2026-11-21，macOS、Apple M4）

- 決定了第 24 條（tile 畫好就送「建立」的訊息、像素只複製一次、有計數守著）；使用者不要「先多複製、之後再省」。1b 分成三小段（上面的表），每段都通過現有的測試。
- **做了什麼**：`protocol` 的 `CompositorFrameSinkMsg::CreateSharedImage`（編號、大小、像素的 `Arc`）、`DestroySharedImage`，`BeginFrameObserver` 多一個「像素還回來了」的 callback（Browser 和 Viz 之間的，`sanity.py boundary` 的允許清單加了兩行），`SharedImage::from_mailbox`。`viz`：`Display` 照（frame sink、編號）記 shared image（Chromium 的 `SharedImageManager`），`DirectRenderer` 多 `create_shared_image`、`flush_shared_images`；收到 frame 時把它用到的 shared image 跟著 frame 存進 `SurfaceManager`（Chromium 的 `DisplayResourceProvider` 鎖住 frame 用的），aggregator 連同換過的編號交給 renderer；Viz thread 處理兩則訊息、一批之後 flush 一次、放開緩衝區之後通知 client。`WgpuRenderer` 上傳的那一段從 `GpuSharedImages::create_shared_image` 抽成 `upload`，兩條路共用。
- 舊的路（compositor 直接呼叫 `SharedImageInterface`、frame 帶 backing）還在，還沒有人走新的；1b-2 換過去，1b-3 拿掉舊的。
- `shell` 的 headless 有自己的一個 Viz 迴圈（不經過 Viz thread）：`cargo check --workspace` 抓到它沒處理新的兩則訊息，補上建立、銷毀和 flush；它還沒把緩衝區還給 client，1b-2 接上 client 那一側時一起做。
- **新的測試**：`viz` 的 `display` 3 個、`thread` 1 個、`wgpu_renderer` 1 個（真的 GPU）。
- **寫測試時碰到的**：「緩衝區還回來時 client 可以再寫」原本用 `Arc::get_mut` 檢查，但測試自己留著的 `Weak` 也讓它失敗；改成 callback 當場數有幾個人拿著（只剩 client 自己的 1 個）。
- **mutation test**（`mutants-1b1.toml`）12 個，全部抓到。跑完確認檔案都還原了。
- **跑過的檢查**：`cargo test -p viz`（47 個通過）、`-p protocol`、`-p compositor`；`cargo check --workspace --all-targets`；`cargo clippy -p viz -p protocol --all-targets -- -D warnings`、`cargo fmt --check`；`sanity.py` 的 7 項。
