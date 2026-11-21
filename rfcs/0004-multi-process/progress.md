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
| 2d：data pipe 的跨 process 版；通道協定的 TLA+ 模型（TLA+、Verus、fuzz） | 完成 | 2026-11-21 |
| 2e-1：端點（`ipc::endpoint`）：通道、`Control`、池子和表、管道的兩半、`codec` 組成上層用的介面，IO thread 的迴圈；同一個 process 裡的平台（整合測試、Miri、fuzz） | 完成 | 2026-11-21 |
| 2e-2：macOS 的共用記憶體（memory entry 的建立、map、只能讀的 handle） | 完成 | 2026-11-21 |
| 2e-3：macOS 的 Mach 訊息和 right（最底下一層）、rendezvous（Browser 的 server、child 的查詢；測試自己啟動 child process）。順序和 2e-4 對調過（第 38 條） | 完成 | 2026-11-21 |
| 2e-4：macOS 的通道（Mach 訊息的 `Platform`、等 port set、dead-name 和 no-senders、audit token；跨 process 的測試）；大訊息走 out-of-line（第 39 條） | 完成 | 2026-11-21 |
| 2e-5：2e 的檢查和文件；Linux、Windows 在這台機器測不到的部分怎麼處理（第 40 條） | 完成 | 2026-11-21 |
| 2f：Linux 的平台層；量 socket 的緩衝區和大的資料的門檻。在 Linux 的機器上做（第 40 條） | 未開始 | |
| 2g：Windows 的平台層；量 pipe 的緩衝區。在 Windows 的機器上做（第 40 條）；做的時候決定 CI 要不要加 macOS、Windows 的 runner | 未開始 | |
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

### 2d：過通道的 data pipe 和 TLA+ 的模型（2026-11-21，macOS、Apple M4）

- **決定**：「已決定的事項」第 32 條（管道的訊息是 `ipc` 自己的；編號只變大）、第 33 條（讀的一端累積到容量的一半才回報）、第 34 條（三個小模型）。「導覽的 body 的管道」的表和「測試」的 TLA+ 那一列跟著改。
- **做了什麼**：
  - `ipc::core::pipe`：`Outgoing`（寫的一端：`open`、`room`、`sent`、`end`、`peer_read`、`peer_cancelled`）、`Incoming`（讀的一端：`opened`、`get`、`ended`、`cancel`）、`ReadReport`、`MAX_PIPE_CHUNK`（64 KiB）。
  - `ipc::core::control`：`Control` 從 `region.rs` 搬出來，多五則管道的訊息；`PipeData` 的位元組解出來不複製。`BadMessage` 多 `UnknownPipe`、`PipeReopened`、`PipeOverRead`、`PipeOverrun`。
  - `ipc::core::credit`：`room`、`sent`、`read`、`threshold`、`report`，寫在 `verus!` 裡。
  - `ipc::data_pipe`：`Producer::try_write`（整段寫或不寫、不等）。
  - `crates/ipc/tla/`：`Channel.tla`、`Regions.tla`、`Pipe.tla` 和 `check.py`；`test.py ipc tla`（新的）。
- **表上寫的 loom 沒有用到**：管道的兩半各自只在一條 thread 上（寫端的額度在 IO thread；讀端「還沒回報多少」在讀的那條 thread），兩條 thread 共用的只有 `data_pipe` 自己的那把鎖，它已經有測試。表的那一列改成實際用的（TLA+、Verus、fuzz）。
- **Verus**：19 個驗證通過（多的 5 個是額度的）。證明的是：送了之後的量不超過容量、超過的被拒絕；回報的比送的多被拒絕；門檻至少 1、不超過容量；每個讀到的位元組剛好被回報一次。
- **TLA+**（TLC 1.7.4，`check.py` 第一次跑時下載、核對 SHA-256）：67 項檢查，約 48 秒，全部符合預期。
  - 成立：`Channel`（120 個狀態）、`Regions`（2 塊 region 1402 個、3 塊 46518 個）、`Pipe`（容量 1、2、3、4、6：照規矩的讀端 122 到 7244 個，亂回報的讀端 1535 到 415 萬個）。
  - 拿掉一部分要找到反例：`Channel` 6 種、`Regions` 6 種、`Pipe` 7 種，都找到列出來的那一個。
  - 三個模型第一次跑就全部成立，沒有修過；證明它們不是空轉的是那 19 種拿掉的部分。
  - `Pipe` 的容量 8 試過（亂回報的讀端 1.4 億個狀態，6 分 37 秒，成立），不放進每次都跑的清單。
  - 模型和程式的對應是手寫的；`pipe_fuzz.rs` 用同樣的步驟跑真的程式。`Regions` 裡的 peer 都照規矩，亂送的 peer 是 fuzz 和單元測試的事。
- **fuzz**：新的 target `pipe`（`tests/it/pipe_fuzz.rs`：寫端、讀端、真的 `data_pipe`、中間編成位元組的訊息）。三個 target 各 2 分鐘：`frame` 1594 萬次、`pipe` 511 萬次、`region` 91 萬次，沒有發現。
- **Miri**：核心的單元測試 49 個通過、1 個跳過，約 9 分鐘。
- **mutation test**（`mutants-2d.toml`）40 個，全部抓到。第一次跑有 1 個沒抓到：「結束一條管道連帶結束編號比它小的」，測試只結束過編號最小的那一條；補了結束中間那一條的檢查。跑完確認檔案都還原了。
- **跑過的檢查**：`cargo test -p ipc`（單元 50 個、整合 37 個）；`test.py ipc verus`、`ipc tla`、`ipc fuzz --minutes 2`、`ipc miri`、`ipc models`；`cargo clippy -p ipc --all-targets -- -D warnings`（一般的、`ipc_loom` 和 `ipc_shuttle` 的各一次）、`cargo fmt --check`；`sanity.py` 的 7 項。
- **留給後面的**：
  - 管道的兩半接到通道和 `data_pipe` 的那一段還沒有寫（Browser 的 IO thread 從自己那一側讀出來送、renderer 的 IO thread `try_write` 進它那一側、讀的那條 thread 回報）：要等 `Control` 接進通道（2e，記在上面的表）。`pipe_fuzz.rs` 裡有完整的一份。
  - `test.py ipc` 現在有五項（`fuzz`、`miri`、`models`、`tla`、`verus`），都還沒有排進 CI（2h）。

### 2e-1：端點（2026-11-21，macOS、Apple M4）

- **決定**：2e 分成五段（上面的表）。「已決定的事項」第 35 條（IO thread 的迴圈由 `ipc` 提供）和第 36 條（訊息的型別在 IO thread 上解、由上層指定）；「待決定」多一條「IO thread 上複製大資料的時間」。管道的讀端怎麼回報沒有另外定：RFC 已經寫明 renderer 看到的是同一個 `Consumer`。
- **做了什麼**：
  - `ipc::endpoint`：`Platform`（平台要實作的：`Transport` 的 `write` 和 `wake`，加上 `wait`、`map`、`create`、`shut`）、`Config`、`endpoint()` 給出 `Link`（任何 thread：`send`、`open_pipe`、`grant_to_peer`、`close`）和 `Endpoint`（IO thread：`run`，和一步一步推的 `turn`）、`Handler` 和 `Received`（`destination`、`decode`、`take_pipe`）。
  - `ipc::local`：同一個 process 裡的 `Platform`（`pair`、`poll`，和測試看的 `written`、`created`）。
  - `ipc::data_pipe`：`Consumer::observe` 和 `ReadObserver`。
  - `ipc::core::channel`：核心自己的目的地一律放行。
- **寫的時候碰到的**：
  - **通道結束時要把傳輸關掉**：只把這一端標成結束，別的 thread 還拿著 `Link`，傳輸就一直開著，對方永遠不知道。兩條 thread 的測試卡住才看出來；`Platform` 多了 `shut`。
  - **送不出去的訊息放進 region 的那幾塊要還**：第一版留了一段什麼都沒做的程式，重寫成 `Placing` 記下這一則放了哪幾塊。
  - `local` 第一版兩端各一把鎖、互相看對方的，寫到一半就看得出會互鎖；改成一把鎖。
  - Miri 報了 3 筆沒釋放的記憶體：是測試為了讓管道保持開著寫的 `mem::forget`，不是端點。改成把來源留到檢查結束。
- **測試**：`tests/it/endpoint.rs` 12 個（兩端一步一步推的、亂送的 child 和亂送的 host、兩條 thread 真的跑的）；`tests/it/endpoint_fuzz.rs`。兩條 thread 的那一個重複跑 300 次都過；它自己設 20 秒的時限，卡住時說是卡住。
- **fuzz**：新的 target `endpoint`（host 的端點對一個照輸入送任何 frame 的 child）。四個 target 各 2 分鐘：`endpoint` 71 萬次、`frame` 1467 萬次、`pipe` 433 萬次、`region` 110 萬次，沒有發現。
- **Miri**：核心的單元測試 49 個通過、1 個跳過；端點資料量小的 4 個測試通過（`test.py ipc miri` 現在兩個都跑）。
- **mutation test**（`mutants-2e1.toml`）45 個，全部抓到。過程：
  - 第一次 45 個裡 6 個沒抓到。5 個補了測試：通道結束後管道的來源要放掉；帶 handle 的一般訊息是壞訊息（原本的測試裡框架那一層先擋掉了，走不到端點的檢查）；child 不會建 region（原本只看 host 的池子）；給兩塊 region 是兩個編號（原本只給過一塊）；讀到結尾的讀端離開時不送取消。第 6 個是程式多餘：收到取消時「把來源拿掉」那一行，緊接著的送管道那一步本來就會做；拿掉那一行，清單換成「取消沒有交給管道」。
  - 「region 的上限不檢查」第一次也沒抓到：池子自己也會擋，差別只在白建一塊再丟掉。`local` 記下建了幾塊，測試檢查沒有白建。
  - 抓到的裡面有 13 個是兩條 thread 的測試卡住才失敗的（叫醒、對方不在了、管道不送這一類）。原本要等外面的上限，現在測試自己在 20 秒內失敗。它們指得出「卡住」，指不出卡在哪一步。
  - 跑完確認檔案都還原了。
- **跑過的檢查**：`cargo test -p ipc`（單元 50 個、整合 51 個）；`test.py ipc fuzz --minutes 2`、`ipc miri`、`ipc models`、`ipc verus`、`ipc tla`；`cargo clippy -p ipc --all-targets -- -D warnings`（一般的、`ipc_loom` 和 `ipc_shuttle` 的各一次）、`cargo fmt --check`；`sanity.py` 的 7 項。
- **沒有做的**：
  - 端點自己沒有 loom 的模型：送的 thread 和 IO thread 共用的是一把鎖底下的狀態（池子、管道的額度、排著要給的 region），靠兩條 thread 的測試和它重複跑的結果。之後量到要拆鎖時再加。
  - 池子什麼時候放掉閒置的 region、每個 renderer 的上限是多少：第 3 階段 Browser 接上時定。
  - 單一 process 的模式要不要也走 `ipc::local`：沒有討論過，現在它只給測試用。

### 2e-2：macOS 的共用記憶體（2026-11-21，macOS 26、Apple M4）

- **決定**：「已決定的事項」第 37 條（只做用得到的兩種 region，不做 `WritableSharedMemoryRegion`）；「共用記憶體」的「三種 region」改成兩種。
- **做了什麼**：`ipc::platform::macos::memory`：`create(len, peer_writes)`、`map(handle, len, writable)`、`SendRight`、`Mapping`（實作核心的 `Memory`）。workspace 多一個依賴 `mach2`（0.6，本來就在 `Cargo.lock` 裡），只在 macOS。mach2 沒有宣告 `MAP_MEM_NAMED_CREATE`，自己寫了那一個常數。
- **測試**：單元測試 12 個，只在 macOS，在同一個 process 裡把 handle 再 map 一次當成對方。跨 process 的要等 2e-3 能傳 handle。
- **寫的時候碰到的**：
  - 「mapping drop 就是解除」第一版是在剛放掉的位址上再 map 一次；別的測試的 thread 可能剛好佔走那個位址，會不穩。改成量整個 task 的位址空間：map 又 drop 4 GiB 之後沒有長大，留著的時候有長大。
  - 「handle drop 就是放掉」是再放一次、看它失敗。別的 thread 剛好拿到同一個名字的話會誤判；Mach 的名字帶世代，短時間內不會重複。這一組重複跑 300 次、整組單元測試平行跑 50 次都過。
  - 長度 0 的 entry kernel 建得出來（我原本以為會失敗）；`create(0)` 回 `None` 是後面的 map 擋的。
- **mutation test**（`mutants-2e2.toml`）13 個，全部抓到。第一次有 1 個沒抓到：「系統呼叫失敗時仍把回來的名字當成 right」，外面看不出來是因為後面的 map 也失敗；補了直接對那個函式的測試（對沒有記憶體的位址建 entry）。跑完確認檔案都還原了。
- **Miri 跑不到**：全部是系統呼叫。`test.py ipc miri` 只跑核心和端點。
- **只靠審查的**：mapping 不被 child 繼承（`VM_INHERIT_NONE`），要 `fork` 才看得到，記在 `docs/test-gaps.md`。
- **跑過的檢查**：`cargo test -p ipc`（單元 62 個、整合 51 個）；`cargo clippy -p ipc --all-targets -- -D warnings`、`cargo fmt --check`；`sanity.py` 的 7 項。核心和端點沒有動，fuzz、Miri、模型、Verus、TLA+ 沒有重跑。

### 2e-3：macOS 的 Mach 訊息和 rendezvous（2026-11-21，macOS 26、Apple M4）

- **決定**：「已決定的事項」第 38 條：2e-3 和 2e-4 對調（先 rendezvous、再通道），和 rendezvous、Mach 訊息這一層實作時定的細節。
- **做了什麼**：
  - `ipc::platform::macos::port`：`SendRight`（從 `memory` 搬過來）、`SendOnceRight`、`ReceiveRight`、`Right`。
  - `ipc::platform::macos::message`：`Buffer`、`send`、`receive`、`Received`、`Sender`（audit token 的 pid）。
  - `ipc::platform::macos::rendezvous`：`Server`（`start`、`register`、`forget`）、`fetch`、`name_for`。
  - mach2 沒有宣告的自己宣告：`mach_port_request_notification`、libbsm 的 `audit_token_to_pid`。
- **和原型不同的地方**：
  - 原型的 server 收到沒登記的 pid 的問題時等 500 ms；一個沒登記的 process 一直問就能讓所有 child 每次多等半秒。現在問題留著、`register` 時回答，server 不等。
  - 原型收到的 right 是裸的 port 編號、一律用 `mach_port_deallocate` 放；receive right 那樣放不掉。現在照 descriptor 寫的種類包成對應的型別。
  - 原型收訊息時用 `MACH_RCV_LARGE` 把緩衝區長大；現在緩衝區固定，太長的由 kernel 毀掉（不讓對方決定這一端配置多少）。
- **測試**：`message.rs` 的單元測試 11 個、`rendezvous.rs` 的 1 個（都在同一個 process 裡）；`tests/it/rendezvous.rs` 9 個，其中 6 個真的啟動 child（測試把自己再執行一次）。rendezvous 那一組重複跑 30 次、整個整合測試 15 次都過。
- **寫的時候碰到的**：
  - `Buffer::for_body(n)` 第一版沒有記 `n`：為 16 個 right 預留的空間被沒有 right 的訊息拿去放多的位元組，上限就不是說的那個數字。測試（17 個位元組送進 16 的緩衝區）抓到，改成明確記著。
  - 同一個問題在 server 又出現一次：註解寫「問題沒有內容，長的會被接收拒絕」，實際上短的內容收得進來、照樣被回答。不是安全問題（回答的對象還是照 kernel 給的 pid），但說的和做的不一樣；補測試時看到，server 改成明確檢查內容是空的、沒有帶 right。
  - 「登記之後收回」的測試第一版兩種結果都接受（child 可能在兩個呼叫之間問到），等於沒測；改成 child 先等 700 ms 才問。
- **mutation test**（`mutants-2e3.toml`）24 個，全部抓到。第一次 23 個裡 2 個沒抓到：
  - 「任何種類的 descriptor 都當成 port」：測的 out-of-line 記憶體剛好被另一個檢查擋掉。補了 out-of-line 的 port 陣列（它的那個位元組剛好是合法的 disposition）。
  - 「server 對任何訊息都回答」：測試沒有對 server 送過不是問題的訊息。補了單元測試（就是它發現上面那個有內容的問題）。
  - 跑完確認檔案都還原了。
- **Miri 跑不到**：全是系統呼叫。
- **只靠審查的**（`docs/test-gaps.md`）：等著的問題超過 64 個時拒絕最舊的（要 65 個 process）；`fetch` 比對回答的人的 pid（做不出別人送的回答）。
- **跑過的檢查**：`cargo test -p ipc`（單元 74 個、整合 60 個）；`cargo clippy -p ipc --all-targets -- -D warnings`（三種 cfg）、`cargo fmt --check`；`sanity.py` 的 7 項。核心和端點沒有動，fuzz、Miri、模型、Verus、TLA+ 沒有重跑。
- **留給 2e-4 的**：Mach 訊息內嵌的大小 kernel 有上限，一則訊息最大 128 MiB，大訊息怎麼送先量再定；佇列滿了之後怎麼知道又有空間也在那一段。（這一行原本寫「Chromium 用 `MACH_NOTIFY_SEND_POSSIBLE`」，是錯的：2e-4 讀了 `channel_mac.cc`，它是留著訊息、排重試。）

### 2e-4：macOS 的通道（2026-11-21，macOS 26、Apple M4）

- **決定**：「已決定的事項」第 39 條（大訊息用 out-of-line、不切；端點的 `wait` 改成借位元組；佇列滿了用 kernel 的通知；比對整個 audit token）。第 22 條的「macOS 上訊息一律內嵌」和「控制通道」的 macOS 那一條跟著改。
- **先量的、先查的**：
  - Mach 訊息內嵌的上限是 48 MiB；切成 16 KiB 到 16 MiB 每 MiB 都是 80 到 115 µs（數字在第 39 條）。
  - 讀了 Chromium 的 `mojo/core/channel_mac.cc`：緩衝區各一個 page、超過就 out-of-line、不切、佇列滿了留著訊息排重試、比對整個 audit token。我先前說它用 `MACH_NOTIFY_SEND_POSSIBLE`、有 pseudo-receive 的註解，兩個都錯。
  - 探測 kernel（暫時的程式，沒有留下）：佇列滿、不等的送失敗時訊息被退回成收到的樣子（right 多一個計數、16 MiB 的 out-of-line 重新 map 一份、descriptor 改成要解除），`mach_msg_destroy` 之後全部恢復，連做 200 次不累積；`MACH_NOTIFY_SEND_POSSIBLE` 用 `sync=1` 登記會在有空間時通知，登記時已經有空間就立刻通知；dead-name 的 `sync` 是 0 或 1 沒有差別，對已經死掉的名字登記會失敗。
- **做了什麼**：
  - `ipc::platform::macos::channel`：`Channel`（`pair`、`from_rights`），實作 `Transport` 和 `Platform`。
  - `message`：`Buffer::with_out_of_line`、`try_send`、收的一側認一個 out-of-line 的 descriptor、`Received::port`。
  - `port`：`PortSet`、`raise_queue_limit`、`tell_when_dead`、`tell_when_sendable`、`dead_name_told`。
  - `ipc::endpoint` 的 `Platform::wait` 改成收一個 callback、把位元組借給它（`ipc::local` 跟著改）。
- **寫的時候碰到的**：
  - 我說 out-of-line「少一次複製」，第一版沒有做到：`wait` 的介面是「放進這個 `Vec`」。量了才看到（8 MiB 是 584 µs，不是引用原型的 320 µs），改了介面之後是 408 µs。
  - 「通往自己的 port 的 send right」不能拿來叫醒：拿著它，no-senders 永遠不來。改用另一個 port 加 port set（RFC 原本就這樣寫）。
- **測試**：`message.rs` 多 6 個、`channel.rs` 10 個（同一個 process 裡，各自有 10 秒的時限）、`tests/it/mach_channel.rs` 8 個（真的 child）。跨 process 的那一組重複跑 30 次、整個整合測試 15 次、macOS 的單元測試 100 次都過。
- **mutation test**（`mutants-2e4.toml`）27 個，全部抓到。過程：
  - 第一次 30 個裡 9 個沒抓到。補測試的 6 個：只有 dead-name 或只有 no-senders 各自都算對方不在了（原本兩個總是一起來，拿掉一個看不出來）；receive right 不算 handle；兩塊 out-of-line、內嵌加 out-of-line；假冒的訊息帶著像樣的內容（原本是空的，被當真也沒有東西可交出去）；dead-name 通知多給的計數。
  - 拿掉的 2 處多餘的程式：叫醒訊息靠「來自哪個 port」擋（後面的「是不是對方送的」本來就擋）；佇列滿了「問完再試一次」（kernel 在登記時有空間會立刻通知）。
  - 等價的 1 個：dead-name 登記的 `sync` 參數。記在清單的註解裡。
  - 有幾個 mutant 讓測試永遠等：同一個 process 裡的通道測試（`wait` 沒有時限）、餵管道的迴圈、等 child 結束。各加了時限，卡住時測試自己失敗並說明。
  - 跑完確認檔案都還原了。
- **只靠跨 process 的測試守的**：收到的 out-of-line 的各種檢查、audit token、通知只認 kernel 的。fuzz 和 Miri 碰不到（系統呼叫）。
- **跑過的檢查**：`cargo test -p ipc`（單元 90 個、整合 69 個）；`test.py ipc models`、`ipc verus`；`cargo clippy -p ipc --all-targets -- -D warnings`（三種 cfg）、`cargo fmt --check`；`sanity.py` 的 7 項。
- **沒有重跑的**：fuzz 和 Miri。核心沒有動；端點改的是 `Platform::wait` 的介面和 `run` 的迴圈，而 fuzz 和 Miri 的端點測試驅動的是 `turn`（一步一步推），碰不到改的地方。改到的由兩條 thread 的端點測試（走 `run` 和 `ipc::local` 的 `wait`）和跨 process 的測試（走 Mach 的 `wait`）涵蓋。

### 2e-5：其他平台怎麼驗（2026-11-21）

- **決定**：「已決定的事項」第 40 條。2f、2g 在 Linux、Windows 的機器上做；CI 加不加 macOS、Windows 的 runner 等 2g 一起定（「待決定」多一條）。
- **查到的現況**：CI 的三個 workflow 都只跑 `ubuntu-24.04`。macOS 那一層的測試（`platform::macos` 的單元測試 40 個、`tests/it/rendezvous.rs`、`tests/it/mach_channel.rs`）只在 macOS 上編得起來，CI 一次都不跑；記在 `docs/test-gaps.md`。
- **這一段沒有程式**。2e 的每一段各自跑過它的檢查（上面各節）；整個 workspace 的測試、`cargo fmt --check`、workspace 的 clippy 照規矩在第 2 階段結束（2h）跑。
- **branch**：`0004` 在 2026-11-21 推到 origin（到 2e-4 為止），給另外兩台機器 pull。
- **給在 Linux、Windows 上接手的人**：平台層要實作的只有 `ipc::endpoint::Platform`（`write`、`wake`、`wait`、`map`、`create`、`shut`）；`ipc::platform::macos::channel` 是一個完整的例子，`ipc::local` 是最小的。`tests/it/mach_channel.rs` 的測試不分平台的部分（host 是真的端點、child 是測試把自己再執行一次）可以照著寫；child 怎麼拿到它那一端在 Linux 是繼承 fd、在 Windows 是繼承 handle，不需要 rendezvous。

