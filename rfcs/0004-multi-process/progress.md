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
| 1b-2：compositor 的緩衝區的池子、raster 直接畫進去，tile 和 canvas 快照改走訊息，複製次數的計數 | 完成 | 2026-11-21 |
| 1b-3：拿掉舊的介面和交給 renderer 的 Viz 的物件，改 `FakeGpu`、`DrivenViz`，消掉例外 1 | 完成 | 2026-11-21 |
| 1c：`protocol::BigBuffer`，跨邊界的訊息裡的位元組欄位都換成它，`sanity.py boundary` 擋裸的位元組 | 完成 | 2026-11-21 |
| 1d：第 1 階段的檢查和文件 | 完成 | 2026-11-21 |
| 2a：訊息的框架和對收到的位元組的檢查；同一個 process 裡的假通道；`sanity.py` 檢查核心沒有系統呼叫（單元測試、惡意的對端、fuzz、Miri） | 完成 | 2026-11-21 |
| 2b：送出的佇列、目的地的分派、斷線只報告一次（loom、shuttle；同步呼叫等回答的位置留在 renderer，第 27 條） | 完成 | 2026-11-21 |
| 2c：region 的表和範圍的檢查、`BigBuffer` 帶 region 的序列化、留著重複用的池子（Verus、fuzz、Miri） | 完成 | 2026-11-21 |
| 2d：data pipe 的跨 process 版；通道協定的 TLA+ 模型（TLA+、loom、Verus） | 未開始 | |
| 2e：macOS 的平台層（Mach 訊息、rendezvous、memory entry；跨 process 的測試）；把 `Control`（`Released`、`Granted`）接進通道收的一側 | 未開始 | |
| 2f：Linux 的平台層；量 socket 的緩衝區和大的資料的門檻 | 未開始 | |
| 2g：Windows 的平台層 | 未開始 | |
| 2h：第 2 階段的檢查、`soundness.md`、`test.py` 的指令、文件；把 `test.py ipc` 的每一項排進每週的 CI（2a 起在本機跑） | 未開始 | |
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
| tile 的像素到 Viz 之前不複製：直接畫進池子的緩衝區（不經過 thread 自己的緩衝區），訊息帶的就是那一塊；frame 只帶編號、沒有像素；Viz 還回來的緩衝區再拿來畫，Viz 還拿著的不畫；畫出來是單一顏色的 tile 不送、緩衝區當場回池子；tile 放掉時送銷毀；canvas 的快照共用它自己的像素、不進池子 | `cargo test -p compositor --lib -- shared_image:: a_tiles_pixels_go a_released_tile_destroys a_one_color_tiles_buffer` |
| 真的頁面走訊息這條路畫得出來；GPU 遺失時 compositor 銷毀原本的 shared image、重新建立 | `python3 scripts/test.py reftest`；`python3 scripts/test.py scenarios`；`cargo test -p browser --test it losing_the_gpu`；`cargo test -p shell` |
| renderer 和瀏覽器介面的 compositor 都只經過訊息建立 shared image（有 GPU 才建）；frame 指到沒有人建立的編號時不畫，就算帶著像素 | `cargo test -p viz --lib a_shared_image`；`cargo test -p shell --test it the_toolbar_is_drawn_again_when_the_gpu_is_lost`；`cargo test -p browser --test it losing_the_gpu` |
| `protocol` 的訊息裡的位元組都是 `BigBuffer`（兩個例外在允許清單）；它從 `Vec<u8>`、`Arc<[u8]>`、`Bytes` 建立都不複製，序列化成一整段位元組 | `python3 scripts/sanity.py boundary`；`cargo test -p protocol --lib big_buffer` |
| 一則訊息的位元組（`ipc::core::frame`）：怎麼切交出來的都一樣、照順序各一次；長度超過上限、序號不連續、沒有權限的一端的 handle、太多或不夠的 handle 都是壞訊息；謊報的長度不佔記憶體；壞訊息之後什麼都不留；任何輸入都不 panic | `cargo test -p ipc`；`python3 scripts/test.py ipc fuzz`；`python3 scripts/test.py ipc miri` |
| `ipc` 的核心不做系統呼叫、不分平台 | `python3 scripts/sanity.py deps` |
| 通道的兩側（`ipc::core::channel`）：送不等、照順序，好幾條 thread 同時送時線上的序號連續、訊息不互相插入；佇列超過上限就關掉通道；不認得的目的地和被拒絕的內容是壞訊息；結束只報告一次、理由是第一個，之後不交出訊息也送不出去 | `cargo test -p ipc`；`python3 scripts/test.py ipc models`；`python3 scripts/test.py ipc miri` |

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

### 1b-2：compositor 經過訊息建立 shared image，像素不複製（2026-11-21，macOS、Apple M4）

- **做了什麼**：`compositor` 的 `shared_image.rs`：`SharedImages`（Chromium 的 `SharedImageInterfaceProxy`：自己取編號、送三則訊息、緩衝區的池子）、`PixelBuffer`、`ClientSharedImage`（最後一個拿著的放掉時送 `DestroySharedImage`）。tile 直接畫進池子的緩衝區（`raster::playback_into_slice`、`playback_size`），那一塊跟著 `CreateSharedImage` 交給 Viz、不複製；tile 和 canvas 的 layer 拿著自己的 `ClientSharedImage`。canvas 的快照共用它自己的像素（`Bytes::from_owner`），也不複製。`protocol`：`CreateSharedImage` 的像素改成 `Bytes`（共用，不是複製進訊息），`CompositorMsg::SharedImagePixelsReturned`。`renderer` 有 GPU 時給 compositor 一個 `SharedImages`，不再把 Viz 的物件交給它；`browser` 的 observer 把「像素用完了」轉成那則訊息；headless 的迴圈也還緩衝區。
- **複製幾次**：tile 的像素從畫出來到 GPU 是一次（Viz 交給 wgpu 的那一次），和改之前一樣。守著它的：`a_tiles_pixels_go_to_viz_uncopied`（thread 自己的緩衝區沒被用過、訊息的位元組就是池子的那一塊、還回來的再用），mutation test 把「畫進 thread 的緩衝區再複製」和「訊息帶一份複製」各放回去一次都抓到。
- 舊的路（`LayerTreeSettings::shared_image_interface`、frame 帶 backing）還在，renderer 不再用；`compositor` 的 `FakeGpu` 的測試還走它，1b-3 一起換掉。沒有 GPU 時（`gpu` 是 `None`）照舊：像素留在 frame 裡，那條路本來就複製一次。
- **寫的時候碰到的**：`browser` 的 `losing_the_gpu_drops_the_renderers_shared_images` 原本看測試的假 GPU 上還有幾個 shared image；renderer 改走訊息之後假 GPU 收不到，改成測試扮演的 Viz（`DrivenViz`）記下建立和銷毀的編號，看遺失前的那些都被銷毀。`Arc::get_mut` 在有 `Weak` 時也失敗，池子用 `strong_count` 判斷 Viz 還拿不拿著。
- **新的測試**：`compositor` 5 個（池子 2 個、tile 3 個）；`browser` 的 GPU 遺失的測試改寫。
- **mutation test**（`mutants-1b2.toml`）10 個：第一次 9 個抓到，「Viz 還拿著的緩衝區拿來畫」沒抓到，是測試裡剛好有另一塊空的可以拿；改成沒有別的可拿，重跑全部抓到。Browser 轉告「像素用完了」的那一段拿掉時沒有測試看得到（只是多配置），記進 `docs/test-gaps.md` 的「沒有計數守著的優化」。
- **跑過的檢查**：`cargo test` 的 `compositor`（135 個）、`viz`、`raster`、`renderer`、`browser`（175 個）、`shell`（628 個）；`test.py reftest`（152 個）、`test.py scenarios`（96 次）都沒有意外的結果；這幾個 crate 的 clippy、`cargo fmt --check`；`sanity.py` 的 7 項。沒有動到視窗那一層（`shell` 只改了 headless 的迴圈）。

### 1b-3：拿掉舊的 shared image 介面，例外 1 消掉（2026-11-21，macOS、Apple M4）

- **做了什麼**：`protocol` 的 `SharedImage` 只剩編號（`Copy`，不帶 backing），trait `SharedImageInterface` 拿掉，Browser 看 GPU 遺失用的改叫 `ContextProvider`（只有 `add_context_lost_observer`，Chromium 的 `viz::ContextProvider`）。`viz` 的 `GpuSharedImages` 的建立和 flush 變成自己的方法，只有 Viz 這一側呼叫；renderer 畫的那條「frame 帶 backing」的路拿掉，frame 的 shared image 只從 Viz 的表找。`compositor` 的 `LayerTreeSettings::shared_image_interface` 和舊的路拿掉；`renderer` 的 `RendererEndpoints` 只剩一個 `gpu: bool`（有 GPU 才建 `SharedImages`）。`shell` 的瀏覽器介面的 compositor 也改走訊息：`BROWSER_FRAME_SINK` 登記時帶「像素用完了」的 observer。
- **例外 1 消掉**：`sanity.py boundary` 的允許清單拿掉它的幾項，`docs/architecture.md` 的例外的表和規則跟著改（編號 1 到 6 都已經修掉或不再算例外）；renderer 這一側不再拿到 Viz 的任何物件。
- **測試**：`compositor` 的 `FakeGpu` 改寫成收 `SharedImages` 送出的訊息（建立、銷毀、還活著幾個），只有舊的路才有意義的 4 個測試和 `the_compositor_doesnt_watch_the_gpu` 刪掉（compositor 已經拿不到可以看 GPU 的東西）；`shell` 的 `browser_view` 測試裡扮演視窗的那一側處理建立和銷毀。新的：`viz` 的 `a_shared_image_nobody_made_isnt_drawn`（frame 指到沒有人建立的編號時，就算帶著像素也不畫）。
- **mutation test**（`mutants-1b3.toml`）4 個：第一次 3 個抓到，「Viz 的表裡沒有時改畫 frame 帶的像素」沒抓到，補了上面那個測試，重跑全部抓到。跑完確認檔案都還原了。
- **跑過的檢查**：`cargo check --workspace --all-targets`；`cargo test` 的 `protocol`、`compositor`（130 個）、`viz`（48 個）、`raster`、`renderer`、`browser`（175 個）、`shell`（628 個）；`test.py reftest`、`test.py scenarios`（96 次）、`test.py scenarios --window`（72 次，動到 `browser_view.rs`、`browser_window.rs`、`window.rs`、`windowless.rs`）都沒有意外的結果，看過 `demo-busy` 的截圖（工具列和頁面都畫出來）；這幾個 crate 的 clippy、`cargo fmt --check`；`sanity.py` 的 7 項。

### 1c：`protocol::BigBuffer`（2026-11-21，macOS、Apple M4）

- **決定**：「已決定的事項」第 25 條（明確的型別、裡面是 `Bytes`、所有的位元組欄位都換、兩個不換）。
- **做了什麼**：`protocol` 的 `big_buffer.rs`：`BigBuffer` 包一個 `Bytes`，序列化成一整段位元組（`serialize_bytes`），從 `Vec<u8>`、`Arc<[u8]>`、`Bytes` 建立都不複製，`Debug` 只印長度。換成它的欄位：`ResourceResponse.body`、`CommitNavigation` 的 `body`、`RegisterBlobUrl.bytes`、`TransferableResource.pixels`、`ResourceRequest.body`、history 的 `state`（`HistoryEntryParams` 和 `DidCommitSameDocumentNavigation`）。行為不變，還是單一 process。
- **跟著改的**：`net` 的 blob 的表存 `Bytes`（原本是 `Arc<[u8]>`，載入時才包成 `Bytes`），收到 `RegisterBlobUrl` 時不必複製；`browser` 的 `NavigationController` 的 state 存 `BigBuffer`，每次導覽帶給 renderer 時 clone 不再複製。`renderer` 和 `net` 裡面照舊用自己的型別，在訊息的邊界上轉換；沒有 GPU 時 tile 的像素原本就複製一次（`Arc::from(&[u8])`），現在一樣一次。
- **檢查**：`sanity.py boundary` 多一項：`protocol`（`big_buffer.rs` 除外）裡出現 `Vec<u8>`、`[u8]>`、`Bytes` 就失敗，允許清單（`PROTOCOL_BYTES_ALLOWED`）兩項，拿掉了沒清也失敗。故意把 `ResourceResponse.body` 改回 `Vec<u8>`，它擋下來；改回去之後通過。`docs/architecture.md` 的「Process 的邊界」第 3 條寫上這條規則。
- **新的測試**：`protocol` 4 個（不複製、序列化成一整段位元組、經過 JSON 來回、`Debug`）。
- **mutation test**（`mutants-1c.toml`）8 個，全部抓到：`BigBuffer` 自己的 6 個；history 的 state 沒交回 engine、fetch 的 body 沒送出去各 1 個（`shell` 現有的測試抓到）。跑完確認檔案都還原了。
- **跑過的檢查**：`cargo check --workspace --all-targets`；`cargo test` 的 `protocol`、`net`、`compositor`、`viz`、`renderer`、`browser`、`shell`（628 個）；`test.py wpt` 的 `html/browsers/history/the-history-interface/`、`FileAPI/url/`、`fetch/api/request/` 沒有意外的結果；這幾個 crate 的 clippy、`cargo fmt --check`；`sanity.py` 的 7 項。沒有動到視窗那一層和畫面。

### 1d：第 1 階段的檢查和文件（2026-11-21，macOS、Apple M4）

- **跑過的檢查**：`cargo test --workspace`（60 個測試 binary，沒有失敗）；`cargo fmt --check`；`cargo clippy --workspace --all-targets -- -D warnings`；`sanity.py` 的 7 項。都沒有要修的。reftest、情境（含視窗的）在 1b-3 跑過，1c 之後沒有再動畫面和視窗那一層；1c 跑了相關的 WPT 目錄。
- **文件**：grep 過這個階段改名和拿掉的東西（`SharedImageInterface`、`shared_image_interface`、`Arc<[u8]>` 的欄位）：`docs/` 和各 crate 的 README 裡沒有留下舊的說法（`docs/architecture.md`、`docs/limitations.md`、`docs/test-gaps.md` 在 1b 和 1c 各步當下改過）。`README.md` 的「還沒有的」拿掉做完的兩項、加上「第 1 階段做完的」；`details.md` 的盤點是 2026-11-20 的，在它下面加一段第 1 階段之後的現況。
- **留給後面的**：Browser 轉告「像素用完了」那一段沒有測試守著（`docs/test-gaps.md` 的「沒有計數守著的優化」）；`matrix`、`fuzz`、`wdspec` 和整套的 WPT 由 CI 跑。

### 原型和 `ipc-channel`、`interprocess` 比（2026-11-21，macOS、Apple M4）

- 第 1 階段做完之後、第 2 階段開始之前量的：原型多一個 `compare` 模式（`prototype/src/others.rs`），同一組訊息走三種傳輸。結果在 `prototype/` 的 README 的「和 ipc-channel、interprocess 比」。
- 小的訊息三者一樣；大的資料原型的 region 比 `ipc-channel` 快 2 到 3.5 倍；Unix socket 預設的緩衝區（8 KiB）讓超過的訊息慢 2.8 到 4 倍，設成 1 MiB 之後內嵌送 1 MiB 和 region 一樣快。
- 寫進 RFC 的：「`ipc` crate」的平台層多一條「socket 的緩衝區要設」；「待決定」多一條「Linux 上大的資料的門檻和 socket 的緩衝區」，第 2 階段做 Linux 的平台層時量。沒有改任何已經定的事。

### 2a：訊息的框架（2026-11-21，macOS、Apple M4）

- **決定**：「已決定的事項」第 26 條（開頭 20 bytes、上限 128 MiB、照實際收到的量配置）。第 2 階段分成 2a 到 2h 八段，每段帶著自己的驗證工具。
- **做了什麼**：`ipc` 的 `core`（`src/core/`）：`frame.rs` 的 `Encoder`（編號、寫開頭和內容，擋掉對方會拒絕的）和 `Decoder<H>`（`feed` 讀到的位元組和 handle，交出到齊的訊息；handle 的型別是泛型的）。讀進來的那一塊裡完整的訊息直接交出去、不複製，只有沒到齊的開頭留著。`fake.rs`（只在測試裡）：同一個 process 裡單向的假傳輸，照種子把位元組切成任意長度的段。
- **`sanity.py deps`** 多一項：`src/core/` 裡不能提到 `rustix`、`libc`、`mach2`、`windows_sys`、`tokio`、`std::fs`／`net`／`os`／`process`／`env`、`extern "`、分平台的 `cfg`。故意在 `fake.rs` 放 `std::fs::read` 和 `#[cfg(unix)]`，兩行都被擋下來；拿掉之後通過。
- **`test.py ipc`**（新的指令）：`fuzz` 跑 `crates/ipc/fuzz` 的每個 target，`miri` 在 Miri 底下跑核心的單元測試；用 elise 釘的同一個 nightly（`nightly-2026-10-01`，這台機器上已經有 Miri，不用另外裝）。
- **測試**：`frame.rs` 的單元測試 14 個；`tests/it/frame_fuzz.rs` 是 fuzzer 和測試共用的檢查（種子和 `fuzz/regressions/` 的重播）。
- **fuzz**：`frame` 跑 2 分鐘、1470 萬次，沒有發現問題。寫檢查時自己寫錯一條（「沒有權限的一端只要輸入裡有 handle 就一定失敗」：那一段沒被餵到時不成立），種子的測試當場抓到，改成每一段餵完就檢查。
- **Miri**：13 個測試通過（`-Zmiri-strict-provenance`），約 5 分鐘；128 MiB 的那一個測試在 Miri 底下跳過。核心現在還沒有 `unsafe`，這一次是把工具架好。
- **mutation test**（`mutants-2a.toml`）22 個，全部抓到：每個檢查拿掉和邊界差一、序號不前進和繞回來、handle 從後面拿、壞訊息之後留下位元組或 handle 或忘記、留整塊而不是沒到齊的、照開頭的長度先配置、交出去的不從緩衝區拿掉、`Encoder` 的四個、開頭的欄位換位置和位元組順序。跑完確認檔案都還原了。
- **跑過的檢查**：`cargo test -p ipc`（單元 14 個、整合 20 個）；`cargo clippy -p ipc --all-targets -- -D warnings`、`cargo fmt --check`；`sanity.py` 的 7 項。別的 crate 沒有動。
- **留給後面的**：目的地認不認得、送出的佇列、斷線（2b）；`test.py ipc` 還沒有排進 CI（記在 2h 的那一列和 `docs/testing.md`）。

### 2b：通道的兩側（2026-11-21，macOS、Apple M4）

- **決定**：「已決定的事項」第 27 條（同步呼叫等回答的位置留在 renderer 的 scheduler）和第 28 條（送出的佇列上限 256 MiB，照位元組）。「測試」的表和性質 8 跟著改。
- **做了什麼**：`ipc` 的 `core::channel`：`Transport`（平台那一層要實作的：不等的 `write` 和 `wake`）、`channel()` 給出 `Sender`（任何 thread，`send`、`flush`、`close`）和 `Receiver`（讀傳輸的那條 thread，`feed`、`peer_gone`、`woken`）。一把鎖守著訊息的編號和寫出去的順序。`core::frame` 的 `Decoder` 多 `try_feed`（`sink` 可以拒絕一則訊息），`BadMessage` 多 `UnknownDestination` 和 `Rejected`。`core::sync`：鎖的型別，照 cfg 換成 loom 或 shuttle 的。
- **cfg 的名字是 `ipc_loom`、`ipc_shuttle`**，不是 elise 用的 `loom`、`shuttle`：`ipc` 依賴 tokio，tokio 自己認 `cfg(loom)`（1.54.1 的 `src/` 裡有 72 處，不限於測試：拿掉 `net` 等模組、換掉幾處實作）。用 `--cfg loom` 編得過（試過），但測到的是另一個 tokio。
- **`test.py ipc models`**（新的）：loom 3 個模型、shuttle 2 個（各 10000 次隨機的排程），各用自己的 target 目錄。
- **測試**：`channel.rs` 的單元測試 13 個；loom 和 shuttle 的模型共 5 個。Miri 底下 26 個通過。
- **mutation test**（`mutants-2b.toml`）20 個，全部抓到：18 個是單元測試抓的（繞過佇列直接寫、上限不檢查和差一、關掉後留著等著的或它的計數、理由被後來的蓋掉、該叫醒的沒叫和不該叫的叫了、`flush` 不扣和不拿掉、沒有權限的一端送 handle、大的緩衝區留著、目的地差一、結束報告多次、讀到一半的不丟、結束後還送、別的 thread 關掉之後還交出訊息）；「鎖在編號和寫出之間放開」單元測試看不到，loom 和 shuttle 各抓到一次。跑完確認檔案都還原了。
- **跑過的檢查**：`cargo test -p ipc`（單元 27 個、整合 20 個）；`test.py ipc models`、`test.py ipc miri`；`cargo clippy -p ipc --all-targets -- -D warnings`（一般的、`ipc_loom` 和 `ipc_shuttle` 的各一次）、`cargo fmt --check`；`sanity.py` 的 7 項。這一段沒有加 fuzz target：`Receiver` 在 `Decoder` 上面只多目的地的比較，從位元組到「Browser 處理完這則訊息」的那個 target 在第 3 階段。
- **留給後面的**：`Transport` 的讀的一側（平台自己的迴圈）和 `wake` 怎麼實作在 2e 之後；region（2c）、data pipe（2d）。

### 2c：region、池子和 `BigBuffer` 的序列化（2026-11-21，macOS、Apple M4）

- **決定**：「已決定的事項」第 29 條（一筆資料用一整塊 region、照大小分級）、第 30 條（「可以再用了」是一則訊息；新的 region 也經過 `ipc` 自己的訊息給）、第 31 條（包住 postcard 的一層 serde）。「共用記憶體」「控制通道」「大的資料」三段和「測試」的表的 Verus 那一列跟著改：訊息裡提到 region 時沒有位置，內容裡沒有 handle 的 index。
- **做了什麼**：
  - `ipc::core::region`：`Memory`（一段共用的記憶體，`unsafe trait`）、`Pool`（送的一端：`add`、`put`、`take`、`released`、`give_back`、`remove`，總量的上限）、`Table`（收的一端：`insert`、`read`、`take_read`、`remove`）、`Control`（`Released`、`Granted` 和它們的位元組）、`HeapMemory`（一般配置的記憶體當 region）。`BadMessage` 多 `TooDeep`、`UnknownRegion`、`OutsideRegion`、`RegionNotOut`。
  - `ipc::core::fit`：`class_len`、`best_fit`、`admits`，寫在 `verus!` 裡。
  - `ipc::codec`：`encode`、`decode`，`Place` 和 `Resolve`（`Pool` 和 `Table` 實作它們），`NoRegions`，`MAX_DEPTH`（128）。
  - workspace 多兩個依賴：`postcard`（1，只開 `alloc`）和 `vstd`（釘和 elise 同一個版本）。
- **Verus**（`test.py ipc verus`，新的）：14 個驗證通過。證明的是：`class_len` 回來的是合法的一種長度、放得下、最小；`best_fit` 回來的是空的、夠長、最短的，回 `None` 時沒有任何一塊可以；`admits` 等於「總量加這一塊不超過上限」。把 `class_len` 的規格改成「嚴格大於」確認過 Verus 會失敗。`Pool::put` 的 `unsafe` 靠 `best_fit` 的後置條件。用的是 elise 的那一版 Verus（toolchain 1.99.1），整個 `ipc`（連 tokio）在它底下編得過。
- **fuzz**：新的 target `region`（`tests/it/region_fuzz.rs`）：輸入當成提到 region 的訊息、`Control`、和一串對池子和表的操作（對著模型）。兩個 target 各 2 分鐘：`frame` 1380 萬次、`region` 122 萬次，沒有發現。
- **Miri**：核心的單元測試 39 個通過、1 個跳過（128 MiB 的那一個），約 8 分鐘；這一段起核心有 `unsafe`（對 region 的複製、`HeapMemory`）。`codec` 的測試不在 Miri 底下：它自己沒有 `unsafe`，經過的 `Table::read` 和 `Pool::put` 由核心的單元測試在 Miri 底下跑。
- **mutation test**（`mutants-2c.toml`）52 個，全部抓到。第一次跑有 3 個沒抓到：
  - 「拿第一個放得下的，不是最短的」：測試裡的 region 都是由短到長加的，兩者一樣。補了一個長的先加的測試。
  - 「不認得的 variant 當成放在訊息裡的」：原本偽造的內容剛好也解不出來。改成後面接著一段解得出來的位元組。
  - 「region 的錯回報最後一個而不是第一個」：等價。serde 產生的型別碰到錯就整個停，不會有第二個；留著 `get_or_insert` 是為了手寫的 `Deserialize` 接住錯誤繼續的情況。記在清單的註解裡，不列成 mutant。
  - 跑完確認檔案都還原了。
- **寫的時候碰到的**：深度的測試原本用程式建一個兩萬層的值，測試自己先 stack overflow（建、序列化、drop 都遞迴），改成直接組位元組（一百萬層）。`encode` 原本把 `out` 整個交給 postcard，失敗時拿不回來；改成 postcard 寫進一個包著 `&mut Vec` 的 `Extend`。
- **跑過的檢查**：`cargo test -p ipc`（單元 40 個、整合 33 個）；`test.py ipc verus`、`ipc fuzz --minutes 2`、`ipc miri`、`ipc models`；`cargo clippy -p ipc --all-targets -- -D warnings`（一般的、`ipc_loom` 和 `ipc_shuttle` 的各一次）、`cargo fmt --check`；`sanity.py` 的 7 項。
- **留給後面的**：
  - `Control` 還沒有接進 `core::channel` 的收的一側，`codec` 也還沒有人呼叫：要等平台那一層能把 handle 變成 `Memory`（2e，記在上面的表）。
  - 池子什麼時候多要一塊、閒置的什麼時候放掉、每個 renderer 的上限是多少：Browser 接上時定（第 3 階段）。
  - `test.py ipc` 現在有四項（`fuzz`、`miri`、`models`、`verus`），都還沒有排進 CI（2h）。
