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
| 2e-6：三個平台共用的啟動入口（`ipc::platform` 的 `spawn`、`inherited`、`child_gone`）；跨 process 的測試拆成不分平台的和 macOS 自己的；`Platform` 夠不夠用對過一次（第 41 條） | 完成 | 2026-11-21 |
| 2e-7：一個 process 一條 IO thread：`IoLoop` 管多個端點，平台的介面拆成 `Platform`（通道）和 `Waiter`（等待）；macOS 和同一個 process 的平台跟著改（第 42 條） | 完成 | 2026-11-21 |
| 2f：Linux 的平台層；量 socket 的緩衝區和大的資料的門檻。在 Linux 的機器上做（第 40 條）；先決定的是第 43 到 49 條 | 完成 | 2026-11-22 |
| 2f-1：Linux 的共用記憶體（memfd 和 seal、map、只能讀的 handle、不能執行；第 46、47 條） | 完成 | 2026-11-22 |
| 2f-2：Linux 的通道（socket 的 `Platform`、epoll 的 `Waiter`、`spawn`、`inherited`、`child_gone`；跨 process 的測試；第 43、48、49 條） | 完成 | 2026-11-22 |
| 2f-3：用真的平台層重量（第 44、45、49 條的數字）；2f 的檢查和文件 | 完成 | 2026-11-22 |
| 2g 之前：Windows 的探測（`prototype-windows/`）：送的 thread 結束時的寫入、pipe 的緩衝區、大的資料、和 `interprocess` 比 | 完成 | 2026-11-21 |
| 2g-1：Windows 的共用記憶體（section 的建立、map、只能讀的 handle；第 51 條） | 完成 | 2026-11-22 |
| 2g-2：Windows 的通道和它的 `Waiter`（completion port；同一個 process 裡的測試；第 52 條） | 完成 | 2026-11-22 |
| 2g-3：Windows 的 `spawn`、`inherited`、`child_gone`（繼承加環境變數，第 53 條）；不分平台的跨 process 測試原樣通過；Windows 自己的跨 process 測試 | 完成 | 2026-11-22 |
| 2g-4：在真的通道上量；當場完成的讀寫改成不經過 completion port、讀的緩衝區維持 64 KiB、CI 先不加 runner（第 54 條） | 完成 | 2026-11-22 |
| 2h：第 2 階段的檢查、`unsafe` 的論證、`test.py` 的指令、文件；`test.py ipc` 的每一項不排進 CI，留在本機跑（第 55 條；原本規劃排進每週的 CI） | 完成 | 2026-11-22 |
| 3：啟動 child process，分成七段（第 56 條） | 進行中 | |
| 3a：child 起得來：`--type=renderer`、child 的 IO thread、Browser 用 `ipc::platform::spawn` 啟動、四種訊息走控制通道 | 完成 | 2026-11-22 |
| 3b：導覽的 body 的管道跨 process，分成兩小段（第 58 條） | 完成 | 2026-11-22 |
| 3b-1：`ipc` 的 codec 認得管道（`Consumer` 能序列化） | 完成 | 2026-11-22 |
| 3b-2：`protocol` 拿掉 `#[serde(skip)]`，`browser` 和 `renderer` 接上 | 完成 | 2026-11-22 |
| 3c：守住現狀：renderer process 的 tile 已經上得了 GPU（多複製兩次）；測試、3a 留下的 mutant、文件（2026-11-22 調整，第 56 條） | 完成 | 2026-11-22 |
| 3d：process 不在了（當掉、被 kill、壞訊息、`about:crash`） | 完成 | 2026-11-22 |
| 3e：切換的旗標、整套測試用多個 process 跑、fuzz target，分兩小段（第 59 條） | 進行中 | |
| 3e-1：`--single-process` 和 `--process-per-tab`；`test.py` 和 CI 用多 process 跑的選項 | 完成 | 2026-11-22 |
| 3e-2：整套測試在多 process 底下跑一遍、修壞的；處理訊息的物件和 body 的管道的 fuzz target | 未開始 | |
| 3f：tile 只複製一次（第 24 條的驗收條件；原本的 3c） | 完成（Linux 等 push 之後的 CI，Windows 還沒編過） | 2026-11-22 |
| 3g：trace；量每個 frame 的延遲和 IO thread 每一步的時間（3f 之前的現況是事後補量的） | 完成（只量不改；要不要接著改等使用者決定） | 2026-11-22 |
| 3h：macOS 上 sandbox 的探測；第 3 階段的檢查和文件 | 未開始 | |

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
| Windows 的共用記憶體（`ipc::platform::windows::memory`）：建的一端寫的，對方的 handle 只能 map 成唯讀，也換不成權限更多的 handle（連改 section 的清單的權限都要不到）；對方寫的，建的一端只 map 成唯讀；唯讀的 mapping 改不成可寫；只建整頁的長度，map 不超過 region 的長度、長度 0 不 map；不是 region 的 handle map 不起來；kernel 拒絕時沒有 handle；交出去的 handle 不被 child 繼承；放掉之後不留 handle、mapping drop 就解除 | `cargo test -p ipc --lib platform`（Windows） |
| Windows 的通道（`ipc::platform::windows::channel`），兩端在同一個 process 裡：交給迴圈之前不寫、加入時被拒絕過的被告知有空間；一次一筆寫入，沒被拒絕過的不被告知；長的訊息一段一段照順序到；host 的 handle 跟著訊息、離開送的一端、到的只有原本的權限、不被繼承，送出去和送不出去都不留 handle；child 不能給 handle，host 把 child 寫的像表頭的位元組當成位元組；host 的每一次寫入各借出去一次、照順序，一個位元組一個位元組到的接得起來；不合理的表頭當成對方不在了；打斷只報一次；一個 waiter 分得出它的通道，拿掉的、自己關掉的、已經報過不在了的都不再報；值丟掉就是關掉；送的 thread 結束之後它的寫入照樣到；進行中的操作讓通道的內部狀態活到通知被取走；通道放掉之後不留 handle；pipe 的兩端不被繼承、第二個 client 連不上 | `cargo test -p ipc --lib platform::windows::channel`（Windows） |
| Windows 上 child 拿到它那一端：不分平台的跨 process 測試原樣通過；一個 process 只拿得到一次，拿到之後環境裡沒有那個變數、那一端不可繼承；變數指到的不是 pipe 就拿不到；child 自己的 child、host 之後另外啟動的 process、同時啟動的其他 child 都沒有它的那一端（child 結束時 host 馬上收到） | `cargo test -p ipc --test it process_channel`、`cargo test -p ipc --test it windows_channel`、`cargo test -p ipc --lib platform::windows`（Windows） |

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

### 2e-6：共用的啟動入口，和 `Platform` 夠不夠用（2026-11-21，macOS 26、Apple M4）

- **為什麼有這一段**：2f 和 2g 要分頭在兩台機器上同時做。分頭之前先看介面夠不夠用，把兩邊都會用到的東西先做好、推上去，免得做到一半才撞。
- **決定**：「已決定的事項」第 41 條；「待決定」多一條（門檻要不要每個平台各一個）。
- **`Platform` 逐個對過**（結論：六個函式都夠用，簽名不用改）：

  | 函式 | Linux | Windows |
  | --- | --- | --- |
  | `write` | 不等的 `sendmsg`，fd 用 `SCM_RIGHTS`；可以只收一部分 | overlapped 的 `WriteFile`，一次一筆：沒有進行中的就整則收下，有的話回 `WouldBlock` |
  | `wake` | eventfd | `SetEvent` |
  | `wait` | `poll` socket 和 eventfd，讀進自己的緩衝區再借出去 | 等「讀完成、寫完成、叫醒」三個 event |
  | `map` | `mmap` | `MapViewOfFile` |
  | `create` | `memfd_create` 加 seal，給對方唯讀的用 `F_SEAL_FUTURE_WRITE` | `CreateFileMappingW`，給對方唯讀的是另一個只有讀權限的 handle |
  | `shut` | `shutdown(SHUT_RDWR)` | `CancelIoEx` 再關 |

- **實作時要注意的**（不是介面的事）：
  - Linux：`recvmsg` 的控制緩衝區要放得下 16 個 fd，被截斷（`MSG_CTRUNC`）當成對方不在了，收到的 fd 設 close-on-exec；`map` 前用 `fstat` 確認大小（map 到不夠大的檔案是當掉，不是錯誤）；寫入被擋之後才等「可寫」，不然 `poll` 一直醒。
  - Windows：overlapped 的寫入沒有當場完成的那一則要自己留一份到完成；handle 的數值走在位元組裡，平台層要有自己的一層包裝，收的一端要先把包裝收齊才借得出位元組；第 41 條的契約。
- **做了什麼**：`ipc::platform` 的 `spawn`、`inherited`、`child_gone` 和 `Channel`（macOS 的實作在 `platform::macos::channel`）；`rendezvous::Server::global()`（一個 process 一個）；`Platform` 的說明多一條契約；`tests/it/process_channel.rs`（不分平台，8 個測試）和瘦下來的 `tests/it/mach_channel.rs`（3 個）。
- **寫的時候發現的**：child 還沒拿它那一端就結束時，macOS 的 host 永遠不會知道（那一端還替它留著，兩種通知都不來）。這是第 3 階段 `browser` 一定會碰到的（child 啟動就當掉），所以有了 `child_gone` 和它的測試。
- **mutation test**（`mutants-2e6.toml`）4 個，全部抓到（child 那一端沒有留給它、host 把 child 當成別的 process、child 把 host 當成自己、`child_gone` 什麼都不做）。跑完確認檔案都還原了。
- **跑過的檢查**：`cargo test -p ipc`（單元 90 個、整合 72 個）；三組跨 process 的測試重複跑 30 次、整個整合測試 15 次都過；`cargo clippy -p ipc --all-targets -- -D warnings`（三種 cfg）、`cargo fmt --check`；`sanity.py` 的 7 項。
- **給在 Linux、Windows 上接手的人**（取代 2e-5 那一節的同一段）：
  - 各開一個從 `0004` 分出去的 branch，做完 rebase 到最新的 `0004` 再 fast-forward。
  - 要寫的：`platform/<系統>/` 底下實作 `Platform` 的通道，和那個系統的 `spawn`、`inherited`、`child_gone`；在 `platform/mod.rs` 照 macOS 的那幾行 re-export；`tests/it/main.rs` 裡 `process_channel` 的 `cfg` 加上自己的系統。`process_channel.rs` 不用改就要過。
  - 那個系統自己才有的行為，另外寫一個測試檔（像 `mach_channel.rs`）。
  - 要動到共用的東西（`endpoint.rs` 的 `Platform`、`codec` 的門檻）時，單獨做成一個小 commit 先推上 `0004`，另一邊 rebase，不要兩邊各改各的。
  - 決定的編號：Linux 從第 43 條、Windows 從第 53 條起（2e-7 用掉了第 42 條）。Windows 的之後順成第 50 到 54 條。
  - 這一段寫的「實作 `Platform`」在 2e-7 之後是兩個介面，見 2e-7 那一節。

### 2e-7：一個 process 一條 IO thread（2026-11-21，macOS 26、Apple M4）

- **為什麼有這一段**：一個 process 只會有一條 IO thread。2e-1 的 `Endpoint::run` 是一條 thread 佔住一條通道，`Platform::wait` 只等自己那一條；2f、2g 照那個介面寫的話，三個平台之後都要重寫等待的那一半。分頭之前先改。
- **決定**：「已決定的事項」第 42 條。
- **做了什麼**：
  - `ipc::endpoint`：`IoLoop`（一條 thread 的迴圈，`run`）和 `LoopHandle`（別的 thread 用：`serve(endpoint, handler)`、`stop`）；`Handler` 多 `gone`；`Endpoint::run` 拿掉，`turn` 留著（迴圈和一步一步推的測試都用它）。
  - 平台的介面拆成 `Platform`（每條通道：`write`、`map`、`create`、`shut`）和 `Waiter`（每條 thread：`new`、`interrupter`、`add`、`remove`、`wait`）。
  - 叫醒的 callback 由核心的通道拿著（`core::channel` 的 `Sender::on_wake`、`Sender::wake`）：`Transport::wake` 拿掉，平台不經手。迴圈在端點加入時裝上；手動推端點的測試用 `Link::on_wake`。
  - `Platform` 的條件寫進 trait（`Transport<Handle: Send>`），用到它的地方不用各自重複。
  - `ipc::local` 的 `LocalWaiter`（輪流看每一端）；macOS 的 `PortWaiter`（一個 port set、一個接收緩衝區、一個打斷用的 port；通道的 receive right 加進 set，收到的訊息照 port 找回是哪一條）。
- **過程中改掉的**：
  - 第一版叫醒經過平台（`Transport::wake` 加 `Platform::on_wake`）。回頭看介面時發現那是核心自己要用的東西繞了平台一圈，三個平台會各寫一份一樣的；改成通道自己拿著。
  - 「通道被叫醒」和「等待被打斷」原本都叫 woken，讀起來分不清：後者改成 `Waited::Interrupted`、`Waiter::interrupter`。
- **測試**：
  - `crates/ipc/tests/it/endpoint.rs`：兩個迴圈各在一條 thread 上跑一條通道到一端關掉；一個迴圈管五條通道；端點交給迴圈之前做的事（管道開了、餵完、結束；通道關掉）交給迴圈時才做。各有 30 秒的時限。
  - `crates/ipc/tests/it/process_channel.rs`：一條 IO thread 管四個真的 child（各自的訊息照順序、大資料各走各的 region；kill 其中一個只有那一條結束；迴圈停下來其餘的 child 都收到、結束）。原本的測試改成經過迴圈。
  - macOS 的通道的單元測試改成經過 `PortWaiter`：一個 waiter 分得出它的幾條通道、移掉的不再等、打斷不管叫幾次只說一次。
  - 核心的通道：叫醒的次數（佇列從空變成有東西一次、關掉一次）改成數裝上去的 callback。
- **mutation test**（`mutants-2e7.toml`）27 個，全部抓到。過程：
  - 介面整理之前跑到一半（20 個裡的 8 個）漏了 1 個：端點交給迴圈時不先推一步。補了測試（交給迴圈之前管道開了、餵完、結束，另一條通道關掉）。
  - 整理之後 28 個，漏了 2 個。補測試的 1 個：`LocalWaiter` 登記一端時不看已經到的（原本的測試裡後面總有別的訊息到，順便帶出來；現在有一則先到、之後什麼都不送的）。拿掉的 1 處多餘的程式：`Link::open_pipe` 最後再叫醒一次（掛上 `watch` 時來源已經有資料就會當場叫醒，沒有資料時也沒東西可送）；拿掉之後「來源有資料時不叫醒」那個 mutant 重跑還是抓到。
  - 「通道結束不告訴 `handler`」那個 mutant 讓整個 run 卡了 7 分鐘：測試的 child 也是同一份程式，它永遠不結束，host 被時限結束之後留下的 child 握著測試輸出的 pipe。手動停掉 child 之後繼續；`process_channel.rs` 的 child 加了 60 秒自己結束的保險，等迴圈停下來的那一處也加了時限。
  - 跑完確認檔案都還原了。
- **跑過的檢查**：`cargo test -p ipc`（單元 91 個、整合 75 個，重複 5 次）；`test.py ipc models`（loom 3 個、shuttle 2 個）；`test.py ipc fuzz`（4 個 target 各 5 分鐘：`endpoint` 119 萬次、`frame` 3732 萬次、`pipe` 1149 萬次、`region` 265 萬次，沒有發現）；`test.py ipc miri`（核心 49 個、端點 4 個，沒有洩漏）；`cargo clippy -p ipc --all-targets -- -D warnings`（三種 cfg）、`cargo fmt --check`；`sanity.py` 的 7 項。
  - fuzz、Miri、loom 和 shuttle 是在拿掉 `open_pipe` 那一行之前跑的；之後重跑的是 `cargo test -p ipc`、clippy、fmt 和那一個 mutant。
- **沒有跑的**：`test.py ipc verus`、`ipc tla`（`fit`、`credit` 和三個模型對應的程式沒有動）。
- **給在 Linux、Windows 上接手的人**（取代 2e-6 那一節裡講介面的部分；branch、檔案放哪裡、編號照 2e-6）：
  - 要實作的是兩個東西，`ipc::platform::macos::channel` 是完整的例子，`ipc::local` 是最小的：

    | | Linux | Windows |
    | --- | --- | --- |
    | `Platform::write` | 不等的 `sendmsg`，fd 用 `SCM_RIGHTS` | overlapped 的 `WriteFile`，一次一筆 |
    | `Platform::map`、`create` | `mmap`；`memfd_create` 加 seal | `MapViewOfFile`；`CreateFileMappingW` |
    | `Platform::shut` | `shutdown(SHUT_RDWR)` | `CancelIoEx` 再關 |
    | `Waiter` | 一個 epoll：每條通道的 socket 用它的 key 登記 | 一個 I/O completion port：每條通道的 handle 用它的 key 關聯 |
    | `Waiter::interrupter` | 一個 eventfd，也登記在 epoll 裡 | `PostQueuedCompletionStatus` |
    | `Waiter::wait` | `epoll_wait`，可讀的那一條 `recvmsg` 進 waiter 自己的緩衝區再借出去 | `GetQueuedCompletionStatus`，完成的讀是哪一條看 completion key |

  - **叫醒不用做**：通道什麼時候要 IO thread 做事是核心和迴圈之間的事。平台只要讓 `interrupter` 給的 callback 能從任何 thread 打斷 `wait`（回 `Waited::Interrupted`；叫很多次、打斷一次就夠）。
  - `Waiter::wait` 回報「又有空間」（`Event::Writable`）只在那條通道的 `write` 回過 `WouldBlock` 之後：Linux 那時才對 socket 登記 `EPOLLOUT`、有了就拿掉，不然 epoll 一直醒；Windows 是進行中的那一筆寫完成。這表示 `Platform`（`write`）要能告訴 `Waiter` 它在等空間：macOS 是通道和 waiter 共用一個 `Arc` 的內部狀態，可以照做。
  - `remove` 之後那條通道的事不能再報出來（key 不重複使用，迴圈拿到不認得的 key 會略過，但緩衝區裡它的東西要丟掉）。
  - Windows 不能用「每條通道三個 event 加 `WaitForMultipleObjects`」：一次最多等 64 個。
  - `process_channel.rs` 裡 `one_io_thread_serves_several_children` 是驗這一段的：一個迴圈、四個 child。
### 2f：Linux 的平台層（2026-11-22 起，Fedora、kernel 7.3.3、AMD Ryzen AI 9 HX 370）

branch `0004-linux`，從 `0004` 的 2e-7（`82a4fc7`）分出去。

- **先決定的**：「已決定的事項」第 43 到 49 條（child 怎麼拿到它那一端、socket 的緩衝區不設、門檻共用 64 KiB、`MFD_NOEXEC_SEAL`、依賴、誰登記「可寫」、接收緩衝區）。「待決定」裡「門檻要不要每個平台各一個」「Linux 上大的資料的門檻和 socket 的緩衝區」「Linux 的 memfd 要不要加 `MFD_NOEXEC_SEAL`」結掉，「怎麼把控制通道的一端交給 child」只剩 Windows。
- **先量的**：socket 內嵌對 region、`SO_SNDBUF` 的大小、接收緩衝區的大小、兩條通道在同一條 IO thread 上時小訊息的延遲；照核心的複製次數和照原型的量法各量一次。量法和數字在 `details.md` 的「Linux 上 socket 的緩衝區和門檻的實測」。量的是暫時的 C 程式，不是 `ipc`：平台層寫完之後用真的那一層重量一次（同樣兩種量法），結論變了就回頭改第 44、45、49 條。
- **先查的**：
  - unix(7)、socket(7)：`SO_RCVBUF` 對 Unix socket 沒有作用；`SO_SNDBUF` 被加倍、被 `wmem_max` 限住，不報錯。
  - kernel：`wmem_max` 的預設在 6.18 從 212992 改成 4 MiB（commit `a6d4f25888b8`，用 GitHub 的 compare 確認不在 6.17、在 6.18）；`Documentation/userspace-api/mfd_noexec.rst`；`fs/eventpoll.c` 的 `ep_modify`（改登記時已經就緒就排進就緒的清單）。
  - Chromium（2026-11-22 的 main）：`mojo/core/channel_posix.cc`、`channel.cc`、`channel_linux.cc`，`mojo/public/cpp/platform/socket_utils_posix.cc`、`platform_channel.cc`，`base/posix/unix_domain_socket.cc`，`base/memory/platform_shared_memory_region_posix.cc`。沒有設 socket 的緩衝區；`MFD_NOEXEC_SEAL` 先帶、`EINVAL` 就記住不帶；給對方唯讀的 memfd 是經過 `/proc` 重新開的 `O_RDONLY` fd（`details.md` 的「平台層」的 Linux）；`EAGAIN` 時在 IO thread 上登記可寫；送出帶 `MSG_NOSIGNAL`；一次最多附 128 個 fd；每次可讀最多連續讀 256 KiB。
  - std（`rust-toolchain.toml` 的 1.99.0）：`std::os::unix::process::CommandExt` 沒有穩定的「指定繼承哪個 fd」的 API。
  - memmap2 0.9.11：`map_raw`、`map_raw_read_only` 是安全的；給了長度時不讀檔案的大小。
- **還沒做的**：通道和 `Waiter` 和 `spawn`、`inherited`、`child_gone`（2f-2）、Linux 自己的跨 process 測試、用真的平台層重量（2f-3）。依賴（第 47 條）2026-11-22 單獨一個 commit 進了 `0004`。

### 2f-1：Linux 的共用記憶體（2026-11-22，Fedora、kernel 7.3.3、AMD Ryzen AI 9 HX 370）

- **做了什麼**：`ipc::platform::linux::memory` 的 `create`、`map`、`Mapping`（`crates/ipc/README.md` 的「`platform::linux::memory`」）。自己的 `unsafe` 只有 `Mapping` 的 `Memory`；`mmap` 經過 memmap2，其餘是 rustix 的安全呼叫。
- **比決定多做的**：`map` 只接受封了「不能縮短」的檔案，而且先看 seal 再量長度。只用 `fstat` 確認長度的話，檢查之後檔案還能被縮短，mapping 就落在結尾之外；有了 seal，`map` 不管 fd 是誰給的都不會留下那種 mapping。
- **這個 kernel 上確認的**（第 46 條和 `details.md` 寫「要靠測試確認」的）：封了 `F_SEAL_FUTURE_WRITE` 的 region map 成可寫、`write`、經過 `/proc/self/fd` 重新開成可寫再寫或再 map、把唯讀的 mapping `mprotect` 成可寫，都被拒絕；帶 `MFD_NOEXEC_SEAL` 的 region `fchmod` 不成可執行。
- **測試**：`src/platform/linux/memory.rs` 的單元測試 14 個；整個單元測試的 binary 重複跑 1000 次都過。
- **不穩定過的測試**：「mapping drop 就是解除」原本量整個 process 的位址空間（`/proc/self/statm`）有沒有長大。第一版用 64 MiB 的 region，馬上就失敗；改成 1 GiB 之後單獨那一組 100 次都過，但整個 binary 一起跑時 600 次裡失敗 6 次：同時跑的其他測試的 thread 各自保留幾百 MiB（malloc 的 arena、stack），量到多出 600 到 770 MiB。原因是量的東西不對，不是門檻：改成數 `/proc/self/maps` 裡這個 memfd（照 inode）的 mapping 有幾個，別的 thread 影響不到。
- **mutation test**（`mutants-2f1.toml`）18 個，全部抓到。寫清單時拿掉的 1 處多餘的程式：`create` 自己對長度 0 的檢查（`map` 的那一個本來就擋）。跑完確認檔案都還原了。
  - 第一次 `test.py mutate` 沒有跑：`../third_party/rusty_v8` 不在釘住的 commit、有兩個沒 commit 的 `gen/*.sum`（本機編 V8 留下的），使用者確認之後還原、切過去。
- **跑過的檢查**：`cargo test -p ipc`（單元 64 個、整合 53 個）；`cargo clippy -p ipc --all-targets -- -D warnings`；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
- **只靠審查的**：kernel 不認得 `MFD_NOEXEC_SEAL` 時真的回 `EINVAL`、5.1 之前的 kernel 上的行為（`docs/test-gaps.md`）。

### 2f-2：Linux 的通道（2026-11-22，Fedora、kernel 7.3.3、AMD Ryzen AI 9 HX 370）

- **做了什麼**：`ipc::platform::linux::channel` 的 `Channel`（`Transport`、`Platform`）、`EpollWaiter`、`spawn`、`inherited`、`child_gone`（`crates/ipc/README.md` 的「`platform::linux::channel`」）；`platform/mod.rs` 的 re-export；`tests/it/main.rs` 裡 `process_channel` 在 Linux 上也跑。`unsafe` 三處：`pre_exec`、它裡面借用 fd 的那一行、`inherited` 收下 fd。
- **先查的**：Chromium 的 `base/message_loop/message_pump_epoll.cc`（2026-11-22 的 main）：level-triggered，`epoll_wait` 一次 16 個事件，只等一次的登記（寫的那一種）在使用者這一側做。和第 48、49 條一致。
- **和決定不同、或決定沒說到的**：
  - **「一則最多 16 個 fd」不能靠控制資料的緩衝區大小擋**：`cmsg_space!(ScmRights(16))` 的緩衝區收得進 17 個（會進位）。送和收都另外數個數。這是測試抓到的（送 17 個、預期對方不在了，結果讀到 17 個 handle）。
  - **socket 不設成 non-blocking**（第 43 條前後的敘述寫的是 non-blocking 的 socket）：每次 `sendmsg`、`recvmsg` 自己帶 `MSG_DONTWAIT`，再設一次是多餘的（mutation test 拿掉它沒有測試失敗）。
  - **第 49 條說 `remove` 時丟掉 waiter 裡留著的那條通道的事件**：沒有做。key 不重用，迴圈拿到找不到的 key 本來就略過。
  - 「可寫」和「可讀」同時來時只報「可寫」，「可讀」等 epoll 下一次再說（level-triggered）。
  - 一條通道只能有一個 waiter，`add` 第二次回 `false`；被移掉之後可以換一個。
- **改了三個平台共用的測試**（`tests/it/process_channel.rs`；2026-11-22 使用者決定跟著這一段留在 `0004-linux`，不另外先進 `0004`）。兩個測試的時序假設在串流的 socket 上不成立，放寬之後在 macOS 上應該照樣成立，但沒有在 macOS 上跑過：
  - `a_child_cant_map_to_write_what_the_host_sends_it`：child 一拿到 region 就結束（region 的 handle 走在提到它的訊息前面），host 接著寫的那一則在 Linux 上被拒絕（對方不在了），`send(...).unwrap()` 就失敗，整組一起跑 40 次裡 39 次。改成不看這次送出的結果，照舊只看 child 的 exit code。
  - `a_host_and_a_child_talk`：child 先回答、那一批讀完才說「還你了」；測試在收到回答的當下就檢查 region 空了，40 次裡 4 次還沒空（回答是 300 KiB 的內嵌，在 Linux 上分兩次讀，兩者之間的空檔比 macOS 大）。改成等到空為止，有時限。
- **不穩定過的測試**（都是這一段新寫的，原因都找到了）：
  - 「handle 送出去之後寫的一端的關掉了」原本問那個 fd 的號碼還開不開著：別的測試的 thread 會馬上重用那個號碼，500 次 1 次。改成數這個 process 裡指向那塊 memfd 的 fd；又因為只比 inode，1500 次 2 次（別的檔案系統的檔案撞到同一個 inode，例如 socket），改成比裝置加 inode。
- **測試**：`src/platform/linux/channel.rs` 的單元測試 16 個，整個單元測試的 binary 重複跑 3000 次都過；`tests/it/process_channel.rs` 9 個（不分平台）和 `tests/it/linux_channel.rs` 4 個，整個整合測試重複跑 40 次都過（改掉上面兩個測試之後）。
- **mutation test**（`mutants-2f2.toml`）35 個，31 個抓到。沒抓到的 4 個（`docs/test-gaps.md`）：
  - 等價的 3 個：`sendmsg` 不帶 `MSG_NOSIGNAL`（Rust 的程式本來就忽略 SIGPIPE）；`recvmsg` 不帶 `MSG_DONTWAIT`（只讀 epoll 說有東西的，而且只有一條 thread 讀）；讀出錯時再讀一次（socket 的錯誤只讀得到一次，再來就是結尾）。
  - 只靠審查的 1 個：`MSG_CTRUNC` 的檢查。fd 放不下時剩下的一定超過 16 個，由數個數的檢查抓到；剩下不到 16 個只在這個 process 的 fd 用完時發生，沒有測試做得出來。
  - 寫清單時拿掉的 3 處多餘的程式：把 socket 設成 non-blocking、「可寫」之外的事件排回去、`remove` 時丟掉留著的事件（上面）。
  - 跑完確認檔案都還原了。
- **只靠審查的**：上面的 `MSG_CTRUNC`；`spawn` 在 child 那一端的號碼是 0 到 2 時換號碼；`spawn` 用過的 `Command` 不能再用（`docs/test-gaps.md`）。
- **跑過的檢查**：`cargo test -p ipc`（單元 80 個、整合 66 個）；`cargo clippy -p ipc --all-targets -- -D warnings`、`cargo fmt --check -p ipc`；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。

### 2f-3：用真的平台層重量（2026-11-22，Fedora、kernel 7.3.3、AMD Ryzen AI 9 HX 370）

- **為什麼**：第 44、45、49 條是照一個模擬核心複製次數的 C 程式定的，寫的時候就說平台層做完要用真的重量。
- **怎麼量的**：一個暫時的 `ipc` 的 example（release build，沒有留下），經過 `ipc::platform::spawn`、`IoLoop`、`Link::send` 和 `codec`。`SO_SNDBUF` 和接收緩衝區的大小暫時用環境變數開口，量完還原（`git status` 確認工作目錄乾淨）。數字在 `details.md` 的「Linux 上 socket 的緩衝區和門檻的實測」的最後一段。
- **結論都沒有變**：
  - 第 44 條（不設緩衝區）：`SO_SNDBUF` 要 212992、1 MiB、4 MiB 都沒有趨勢。
  - 第 45 條（門檻 64 KiB）：64 KiB 以上 region 都比較快（64 KiB 是 17 對 22 µs，8 MiB 約 3 倍）。探測程式量到「正好在 64 KiB 交叉」不準：真的程式在內嵌的路上複製得比模擬的多。第 45 條的敘述照真的數字改了。
  - 第 49 條（接收緩衝區 256 KiB）：64 KiB 慢 5% 到 15%，探測程式量到的是一樣快。
- **多看到的**：host 送大的內嵌訊息時，socket 收不下的部分由 IO thread 在「可寫」時寫出去，同一條 IO thread 上別的通道的 p99 到 1 ms（8 MiB）；走 region 沒有。探測程式的送是在送的 thread 上等，看不到這一段。記在第 45 條。
- **量的限制**：一台機器（筆電，`powersave`）；數字有雙峰，只看兩次一致的；門檻以下的 region 量不到（門檻是 `codec` 的常數）。
- **2f 整個做完時跑的**：`cargo test -p ipc`（單元 80 個、整合 66 個）、`cargo clippy -p ipc --all-targets -- -D warnings`、`cargo fmt --check -p ipc`、`sanity.py` 的 7 項。整個 workspace 的測試、fmt、clippy 照規矩在第 2 階段結束（2h）跑。
- **給接著做的人**：`tests/it/process_channel.rs` 的兩個測試在 2f-2 放寬了時序（那一節），macOS 上還沒跑過；`0004-linux` 進 `0004` 之後在 macOS 上跑一次 `cargo test -p ipc`。

### 2g 之前：Windows 的探測（2026-11-21，Windows 11 10.0.26200、Intel Core i9-13900K）

- **為什麼有這一段**：Windows 的平台層沒有原型。寫之前先確認幾件會決定怎麼寫的事；每一項的數字在 `prototype-windows/` 的 README。
- **做了什麼**：`prototype-windows/`（不在 workspace 裡，直接用 windows-sys）：host 和它自己啟動的 child 之間一條 overlapped 的 named pipe，child 那一端是繼承的。
- **查到的**：
  - handle 沒綁 completion port 時，thread 結束會取消它發出的還沒完成的寫入（1 MiB 寫了 0 bytes、對方什麼都沒收到）；綁了就不會。
  - pipe 的緩衝區 4 KiB 到 1 MiB 相差在 15% 以內；一次寫 64 KiB 比整則寫快 13% 到 17%。
  - 64 KiB 時放在 pipe 裡和走 region 一樣，1 MiB 以上 region 快 1.4 到 2 倍。
  - 第二個 client、同名的第二個 instance 都連不上；child 沒碰它那一端就結束或被 kill，host 的讀以 `ERROR_BROKEN_PIPE` 完成。
  - 和 `interprocess` 2.4.9 比：小訊息它慢 40%，64 KiB 以上一樣。
- **這台機器原本編不起來**：`../third_party/elise` 不在 machopo 釘的 commit，`python scripts/ci.py third-party` 同步之後恢復。

### 2g-1：Windows 的共用記憶體（2026-11-22，Windows 11 10.0.26200、Intel Core i9-13900K）

- **決定**：「已決定的事項」第 50 條（分段，和 2026-11-21 那一版為什麼沒有留）、第 51 條（section 的清單、handle 的權限和檢查、handle 的型別）。
- **做了什麼**：`ipc::platform::windows::memory`：`create(len, peer_writes)`、`map(handle, len, writable)`、`Mapping`（實作核心的 `Memory`）。workspace 的 windows-sys 多開五個 feature（`Win32_Foundation`、`Win32_Security`、`Win32_System_Memory`、`Win32_System_SystemInformation`、`Win32_System_Threading`）：這是共用的 `Cargo.toml`，另一邊 rebase 時會碰到。
- **和 2026-11-21 那一版的關係**：第 51 條的三項選的和那一版做的一樣，所以程式和測試就是那一版的，只改了檔頭的說明（清單不是空的、收到的 handle 不檢查）；不是從空白寫的。
- **測試**：單元測試 14 個，只在 Windows，在同一個 process 裡把 handle 再 map 一次當成對方（和 macOS 的那一組同樣的情況，多的是「換不成權限更多的 handle」「不被 child 繼承」「放掉之後不留 handle」）。跨 process 的要等 2g-3 能送 handle。
  - 「mapping drop 就是解除」和 macOS 一樣量整個 process 的位址空間（map 又 drop 4 GiB 之後剩下的沒有少，留著的時候少了）；「不留 handle」數整個 process 的 handle（4000 塊建了又放，多不到 1000 個；留著時是一塊一個）。
- **mutation test**（`mutants-2g1.toml`）18 個，全部抓到。跑完確認檔案還原了。不列的一個是等價的：`create` 不自己檢查長度 0 時，kernel 本來就不建長度 0 的 section（記在 `docs/test-gaps.md`）。
- **Miri 跑不到**：全部是系統呼叫。
- **跑過的檢查**：`cargo test -p ipc`（Windows：單元 64 個、整合 53 個）；`cargo clippy -p ipc --all-targets -- -D warnings`；`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`boundary`、`spans`、`docs`。核心和端點沒有動，fuzz、Miri、模型、Verus、TLA+ 沒有重跑。
- **留給 2g-2 的**：`Platform` 的 `map` 和 `create` 直接接這兩個函式；handle 送給對方是 `DuplicateHandle` 進對方的 process（這裡的 handle 已經只有該有的權限，照原樣的權限複製）。
### 2g-2：Windows 的通道和它的 `Waiter`（2026-11-22，Windows 11 10.0.26200、Intel Core i9-13900K）

- **決定**：「已決定的事項」第 52 條（十項，逐項問過）。
- **做了什麼**：
  - `ipc::platform::windows::channel`：`pipe()`（兩端）、`Channel::host(pipe, child 的 process)`、`Channel::child(pipe)`（實作 `Transport` 和 `Platform`）、`IocpWaiter`（實作 `Waiter`）。從空白寫的，沒有用 2026-11-21 那一版的結構；系統呼叫的參數對過那一版和 Chromium。
  - `ipc::endpoint` 的 `Platform` 的說明照第 52 條改了一句，`crates/ipc/README.md` 同一句也改（共用的，要單獨一個 commit 先進 `0004`）。
  - workspace 的 windows-sys 再多三個 feature（`Win32_Storage_FileSystem`、`Win32_System_IO`、`Win32_System_Pipes`）。
- **還沒有的**：`spawn`、`inherited`、`child_gone`（2g-3）。兩端現在只在同一個 process 裡測過：host 的「child 的 process」是自己，所以「handle 是放進對方的 process 而不是自己的」這裡分不出來，等 2g-3 的跨 process 測試；兩個端點接在這個通道上也是那時第一次跑。
- **測試**：`channel.rs` 的單元測試 24 個（各有 10 秒的時限）。`cargo test -p ipc --lib platform::windows` 重複跑 20 次都過。
- **寫的時候碰到的**：
  - **自己關掉、而且一次都還沒等過的那一端被報成對方不在了**：加入時排著的第一次讀在 pipe 已經關掉時發不出去，被當成 pipe 的盡頭。是把測試改成「真的讓等待把 port 裡的東西取走」之後才看到的（原本的測試先打斷等待，等待在看 port 之前就回來了）。改成自己關掉的那一端什麼都不報。
  - **數 handle 的測試卡住時拖住別的測試**：三個數 handle 的通道測試原本在時限裡面拿那把共用的鎖，卡住的 thread 一直拿著，共用記憶體那邊沒有時限的同類測試就等到永遠（第一次 mutation test 因此每個會卡住的 mutant 都等到工具的上限）。改成由測試自己的 thread 在時限外面拿。
  - **測試連寫兩筆沒有等上一筆完成**：第一版有三個測試這樣寫，第二筆被拒絕。這是設計的行為（寫入要等 IO thread 取到通知才算完成），測試改成被拒絕時等「又有空間」。
- **mutation test**（`mutants-2g2.toml`）：33 個，全部抓到（12 個是測試卡住、到時限才失敗的：叫醒、又有空間、對方不在了這一類）。過程：第一次漏了 1 個，「自己關掉的那一端被告知它的操作怎麼結束的」：測試裡那一端關掉時沒有讀在進行。補了兩種情況（關掉時有讀在進行、一次都還沒等過就關掉），重跑相關的 4 個都抓到。第一次跑到一半停掉過（上面那一條的鎖），用 `--restore` 還原後重跑。跑完確認檔案還原了（還原之後 38 個測試通過）。
- **等價、沒有列的**（記在 `docs/test-gaps.md`）：`shut` 不取消（關掉 pipe 就會取消）；讀失敗或讀到 0 不當成盡頭（下一次讀會失敗）；寫入少寫了當成整段（pipe 不會少寫）；打斷每次都放一筆（只是迴圈多轉幾圈）。
- **只靠審查的**（`docs/test-gaps.md`）：pipe 建立時的旗標（要另一個知道名字、卡在那一小段時間的 process 才看得到）；進行中的操作拿著的那一份 `Arc` 成對（發出時拿、取到通知時放、沒發出去時還回去；弄壞它是釋放後被用，沒有列成 mutant）。有一個測試守著方向：操作還在進行時通道的內部狀態活著，通知被取走之後才釋放。
- **Miri 跑不到**：全部是系統呼叫。
- **跑過的檢查**：`cargo test -p ipc`；`cargo clippy -p ipc --all-targets -- -D warnings`、`cargo fmt -p ipc --check`；`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`boundary`、`spans`、`docs`。核心沒有動；端點只改了 `Platform` 的說明文字。fuzz、Miri、模型、Verus、TLA+ 沒有重跑。
- **留給 2g-3 的**：`spawn`（怎麼把 client 那一端交給 child：自己呼叫 `CreateProcessW` 給清單，還是用標準庫加一把鎖）、`inherited`、`child_gone`；`process_channel.rs` 原樣通過；跨 process 才看得到的（handle 真的到了 child 的 process、child 亂附 handle 時 host 以壞訊息關掉、送的 thread 結束之後寫入照樣到）。
### 2g-3：Windows 上 child 怎麼拿到它那一端（2026-11-22，Windows 11 10.0.26200、Intel Core i9-13900K）

- **決定**：「已決定的事項」第 53 條（四項，逐項問過）。
- **做了什麼**：
  - `ipc::platform::windows::channel` 的 `spawn`、`inherited`、`child_gone`，`ipc::platform` 在 Windows 上 re-export 它們和 `Channel`。
  - `tests/it/main.rs`：`process_channel` 在 Windows 上也編；新的 `windows_channel`（只在 Windows）。
  - `tests/it/process_channel.rs`（共用的，單獨一個 commit）：亂送的 child 先等它那一端、再寫；拿掉沒有人用的 `wait_for_the_host_to_go`。
- **不分平台的跨 process 測試**（`process_channel.rs`）9 個在 Windows 上都過。第一次跑是 8 個過：沒過的那一個就是上面改的（child 在被服務之前寫，被拒絕）。這是兩個真的端點第一次接在 Windows 的通道上：訊息來回、host 的大資料走 region（handle 真的到了 child 的 process、child map 得到、map 不成可寫）、3 MiB 的字串、700 KiB 的管道、child 先睡著時連送 3000 則、child 亂附 handle 時 host 以壞訊息關掉、一條 IO thread 服務四個 child。
- **Windows 自己的測試**：`tests/it/windows_channel.rs` 6 個（child 拿到一次、變數清掉、第二次拿不到；`spawn` 之後呼叫的人的 `Command` 不再帶那個變數；變數是空的、0、不是數字、-1、太大、沒有這個 handle 的數值、沒設，都拿不到；child 自己的 child 活著時 child 結束，host 兩秒內收到；之後由這個 process 另外啟動的 process 活著時 child 被 kill，host 兩秒內收到；八條 thread 同時啟動八個 child，逐一 kill 都馬上收到）。`channel.rs` 多兩個單元測試（變數指到的不是 pipe 就拿不到，包括一個開著的檔案、檔案之後還開著；拿到的那一端被設回不可繼承、變數清掉、再設一次也拿不到第二次）。
- **mutation test**（`mutants-2g3.toml`）11 個，全部抓到。跑完確認檔案還原了。
  - 「拿掉啟動的鎖」是靠八個同時啟動的 child 抓到的，看的是時機；這一次抓到了，但它是機率的。
  - 第一版的測試抓不到兩個，跑之前就看出來、先改了：「`Command` 還帶著變數」（第二個 child 讀到的數值在它那裡多半不是 handle，照樣拿不到；改成直接看 `Command` 的環境）和「只能拿一次的旗標」（變數清掉之後第二次本來就拿不到；改成清掉之後再設一次）。
  - 沒有列的：`spawn` 在建不出 host 那一端時把 child 結束掉（測試裡 kernel 不會拒絕那個複製）。記在 `docs/test-gaps.md`。
- **過程中改掉的**：`inherited` 原本另外檢查數值是不是 0；`GetFileType` 對 0 本來就說不是 pipe，拿掉了。
- **跑過的檢查**：`cargo test -p ipc`（Windows：單元 90 個、整合 69 個，重複 5 次）；`cargo clippy -p ipc --all-targets -- -D warnings`、`cargo fmt -p ipc --check`；`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`boundary`、`spans`、`docs`。核心和端點沒有動；fuzz、Miri、模型、Verus、TLA+ 沒有重跑。
- **macOS 上沒有驗的**：`process_channel.rs` 改的那一處（先等再寫）在 macOS 上應該沒有差別，但這台機器跑不到，要在 macOS 上跑一次 `cargo test -p ipc`。
- **留給 2g-4 的**：在真的通道上量（當場完成的讀寫等不等 port 的通知、讀的大小、一條 thread 服務好幾條通道）；決定 CI 要不要加 macOS、Windows 的 runner。
### 2g-4：在真的通道上量（2026-11-22，Windows 11 10.0.26200、Intel Core i9-13900K）

- **決定**：「已決定的事項」第 54 條（三項，看過數字之後逐項選的）。
- **怎麼量的**：`tests/it/windows_channel.rs` 的 `measure`（平常不跑：`cargo test -p ipc --test it -- --ignored --exact windows_channel::measure --nocapture`）。host 是真的端點，四個會回送的 child 接在同一條 IO thread 上；每個情境重複 5 到 7 次取中位數，整組跑 3 次。測試用的 profile（有最佳化）。
- **數字**（µs；「等 port」是 2g-2 的寫法，「不經過 port」是現在的）：

  | 情境 | 等 port | 不經過 port（現在） |
  | --- | --- | --- |
  | 小訊息，一來一回 | 10.6 到 11.5 | 13.4 到 14.2 |
  | 小訊息，一次送 20000 則（每則來回） | 2.42 | 1.12 到 1.19 |
  | 小訊息，4 個 child 各 5000 則同時送（每則） | 3.50 | 0.38 到 0.46 |
  | 48 KiB 放在訊息裡，來回 | 64 | 46 到 47 |
  | 1 MiB 放在訊息裡，來回 | 1402 | 1338 到 1356 |
  | 8 MiB 放在訊息裡，來回 | 9565 | 9147 到 9508 |
  | 64 KiB 走 region，來回 | 35 | 28 到 33 |
  | 1 MiB 走 region，來回 | 684 | 640 到 686 |
  | 8 MiB 走 region，來回 | 4590 | 3759 到 4699 |

  - 讀 128 KiB（暫時的程式量的，沒有留）：等 port 時每一列都沒有差別（一來一回量到 11.8 到 14.1，走的是和讀 64 KiB 同一段程式）；不經過 port 時只有 48 KiB 放在訊息裡從 47 到 36。
- **說錯過一次**：第一輪我說一來一回「沒有差別、是雜訊」。換成正式的寫法重量之後，不經過 port 的六次都在 13.4 到 14.2，比等 port 的那一組穩定地高約 3 µs；不能確定是真的（另一組走同一段程式的也量到 14.1），但不能再說是雜訊。把這一點告訴使用者之後才定的。
- **做了什麼**：
  - `ipc::platform::windows::channel`：讀和寫發出之後分當場完成和進行中；pipe 加入迴圈時設 `FILE_SKIP_COMPLETION_PORT_ON_SUCCESS`；IO thread 借出一筆之後換別條通道、連續 8 輪之後看一次 port。量測用的環境變數切換沒有留。
  - 單元測試：6 個原本測「寫入要等通知才算完成」的改成新的行為（pipe 有空間時連寫 20 筆都不用等；滿了才有一筆在進行、第三筆被拒絕）。新增：當場完成的讀寫不拿著通道（值丟掉就釋放）；進行中的讀、寫各自拿著通道到通知被取走；一條通道一次讀到 1000 筆要借出去時，另一條通道的訊息最多等 9 筆就輪到，那 1000 筆順序不亂；關掉之後 pipe 和 child 的 process handle 都不在了。
- **mutation test**（`mutants-2g2.toml`，改成現在的寫法，41 個）：全部抓到。第一次漏 1 個，「關掉時留著 child 的 process handle」：它原本是靠操作還掛著、內部狀態沒釋放間接抓到的，當場完成的操作不掛著之後那條路沒了；補了直接的檢查。跑完確認檔案還原了。
  - 沒有列的一個（記在 `docs/test-gaps.md`）：不設那個模式。弄壞它是計數多還一次，不是測試失敗。
  - 多一個等價的：port 只看不等（迴圈空轉，行為一樣）。
- **跑過的檢查**：`cargo test -p ipc`（Windows：單元 93 個、整合 69 個、量測 1 個不跑；重複 20 次）；`cargo clippy -p ipc --all-targets -- -D warnings`、`cargo fmt -p ipc --check`；`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`boundary`、`spans`、`docs`。核心和端點沒有動。
- **2g 到這裡做完**。還沒有做的、不在 2g 裡的：`cf3c85b`（共用的測試先等再寫）在 macOS 上跑一次；整個階段的 `cargo test --workspace`、`cargo fmt --check`、clippy 留到第 2 階段做完時（2h）。

### 2f、2g 的審查（2026-11-22，macOS 26、Apple M4）

- **做了什麼**：在 macOS 上把 2e-7 之後的 12 個 commit 讀過一次：Linux 和 Windows 的共用記憶體和通道（測試以外的部分逐行讀）、共用的介面和測試的改動、文件、第 43 到 54 條。那兩層在這台機器上編不起來，是用讀的。
- **特別對過的**：Linux 寫入被擋時向 epoll 登記「可寫」在幾種 thread 交錯之下不漏通知（寫入和排隊在核心的同一把鎖底下）；一次讀到前一則的尾巴加下一則帶 fd 的開頭時，核心照順序把 handle 配給訊息；Windows 進行中的操作拿著的那一份 `Arc`（`hold`、`let_go`、取走通知時的 `Arc::from_raw`）每條路徑成對；Windows 的鎖的順序一致；child 的位元組沒有表頭，host 不會把它給的數值當成 handle。
- **結果**：沒有發現會出錯的地方。改了四處小的：
  - `crates/ipc/README.md` 講 `process_channel.rs` 的那一段漏了 Linux。
  - Windows 的 `inherited` 的 `SAFETY` 理由：原本是「呼叫的人保證沒有別的 thread」；標準庫寫明 `remove_var` 在 Windows 上多條 thread 也安全，改成這個，使用規則拿掉（第 53 條裡記了）。只改了註解。
  - Linux 的 child 不拿掉環境變數：記進第 43 條、README 和 `inherited` 的說明。
  - 決定的編號順過：Windows 的 53 到 57 改成 50 到 54。
- **看到、沒有動的**：Windows 的 `named_pipe` 對 overlapped 的 handle 呼叫 `ConnectNamedPipe` 時沒有給 `OVERLAPPED`，微軟的文件說要給；這裡 client 已經連上、呼叫當場回來，Windows 上的測試也過。
- **跑過的檢查**（macOS）：`cargo test -p ipc`（單元 91 個、整合 75 個）、`cargo clippy -p ipc --all-targets -- -D warnings`、`cargo fmt --check`、`sanity.py` 的 7 項。改了註解和文件之後重跑的是 `sanity.py docs`、`unsafe` 和 `cargo fmt --check`。
- **沒有驗到的**：共用的測試在 Windows 那一段改過一處（亂送的 child 先等它那一端），Linux 上有沒有在那之後重跑，紀錄看不出來；CI（Ubuntu）每天跑 `cargo test --workspace`，由它確認。這次改的 Windows 的註解沒有在 Windows 上編過。

### 2h：第 2 階段的檢查和文件（2026-11-22，macOS 26、Apple M4）

- **決定**：「已決定的事項」第 55 條（`test.py ipc` 的五項不排進 CI，使用者定的：`ipc` 之後分成獨立的專案，CI 那時做）。論證另外寫成 `crates/ipc/soundness.md`（第 7、20 條原本就這樣寫；做到這裡只有 README 的一節，使用者選了補上）。
- **階段結束的檢查**（照規矩整個階段跑一次）：`cargo fmt --check`、`cargo test --workspace`（60 組測試的結果都是通過）、`cargo clippy --workspace --all-targets -- -D warnings`，都沒有要修的。在 macOS 上跑的：Linux 和 Windows 的平台層不在裡面，它們各自那一段跑過 `cargo test -p ipc` 和 clippy（2f、2g 各節）。
- **`crates/ipc/soundness.md`**（新的）：兩層的目標；信任誰；九條性質各由哪段程式守、怎麼驗證；對 kernel 的假設 K1 到 K9，每個平台各靠什麼、哪個測試守著，標出實測來的；`ipc` 自己的不變式 I1 到 I8；每一處 `unsafe` 的表（從 README 搬過來，照核心和三個平台分成四張，34 列）；怎麼驗證的；還守不住的；修改時要審查的。`crates/ipc/README.md` 的那一節改成指過去，`CLAUDE.md`、`docs/README.md`、`docs/test-gaps.md` 的指向跟著改。
- **寫的時候對過的**：文件裡提到的每個測試名字都在程式裡找得到；共用記憶體的存取點只有 `Pool::put` 和 `Table::read` 兩處（I3）。Linux 和 Windows 的假設是照 2f、2g 的程式、測試和決定寫的，沒有在那兩台機器上重跑。
- **文件**：「測試」的表、`docs/testing.md`、`docs/test-gaps.md` 改成 CI 不跑那五項；`test.py ipc -h` 本來就寫了每一項怎麼裝、怎麼跑，沒有改。
- **沒有做的**：`test.py ipc` 的五項這一段沒有重跑（核心從 2e-7 之後沒有動；那時跑過，結果在 2e-7 那一節）。
- **第 2 階段到這裡做完**。還沒有 merge 回 `main`：等使用者要求。

### 3a：child 起得來（2026-11-22，macOS 26、Apple M4）

- **決定**：「已決定的事項」第 56 條（第 3 階段分成七段）、第 57 條（3a 的五件事：程式放進現有的 crate、旗標是通道的第一則訊息、送的一端換成自己的型別、目的地的編號在 `protocol`、`Handler` 不收窄）。
- **做了什麼**：
  - `protocol::control_channel`（新的）：兩個方向的目的地（往 renderer：main thread、compositor；往 Browser：`RendererHostMsg`、給 Viz 的），兩端共用的通道設定（只有 Browser 附 handle、開管道；region 的上限暫定每個方向 64 MiB），`BrowserSender`，`send_over`（通道拒絕的訊息記下來、丟掉，通道繼續）。`protocol` 多依賴 `tracing`（為了那一則 log）。
  - 送的一端都有兩種：`BrowserSender`（取代 `renderer`、`compositor` 裡 30 處 crossbeam 的 `Sender<RendererHostMsg>`）、`CompositorFrameSinkSender`、`RendererSender`、`CompositorSender`。走通道的 `CompositorFrameSinkSender` 不帶 `FrameSinkId`，它的 `frame_sink_id()` 拿掉了（沒有人用）。
  - `renderer::child_process`（新的）：`run_child_process`（child 的入口：建端點、`Chrome_ChildIOThread`、等第一則訊息 `ChildInit`、開 renderer 的 thread、等它結束）、`ChildExit`。IO thread 的 `Handler` 只解碼、放進兩個佇列；通道結束時送 `Shutdown` 給 main thread、停迴圈。
  - `browser`：`RendererLauncher`（`InProcess(ScriptEngineFactory)` 或 `Process(建 Command 的函式)`），取代 `WebContents::new` 和 `RenderProcessHost::launch` 的 `script_engine` 參數；`RenderProcessHost` 的 process 那一支：`ipc::platform::spawn`、端點、送 `ChildInit`、把端點交給整個 Browser 一條的 IO thread（第一次用到時才建，不停），`ChannelHost`（`Handler`：`RendererHostMsg` 交給原本的 `IoThreadHost`、給 Viz 的補上這個 renderer 的 `FrameSinkId`）；結束時等 child 5 秒、不走就 kill；`WebContents::renderer_pid()`。
  - `shell`：`machopo --type=renderer`（只認第一個參數）、`shell::renderer_main`。
- **這一段刻意沒做的**：視窗和 headless 還不會啟動 renderer process（怎麼打開、預設是哪一種，等 3b、3c 之後定）；body 的管道（3b）；tile 的共用記憶體（3c）；結束不回應的 process、`about:crash`、壞訊息時結束 process（3d，現在 `kill` 只關掉通道、process 在 host 放掉時才結束）；child 的 trace（3f）。記在 `docs/limitations.md`。
- **測試**：
  - `crates/browser/tests/it/renderer_process.rs`（測試把自己再執行一次當 renderer）：`about:blank` 和一個帶標題的 `data:` 頁面在另一個 process 載入、同一個 process 服務兩次導覽；兩個分頁各一個 process、各自的訊息不混；renderer 的 frame 到 Viz 時是分頁的那個 sink；renderer 被 `kill -9` 之後分頁收到 `RendererGone`、變成當掉的狀態（只在 Unix）；同一個 process 的 renderer 沒有 pid。
  - `crates/shell/tests/it/renderer_process.rs`（真的 `machopo --type=renderer`）：手動啟動時說明、以 2 結束；拿到通道之後 Browser 那一端不見就結束；renderer 的 thread 都在跑時 Browser 不見也結束；Browser 那一端在它拿之前就不見時不會留著；switch 不在第一個參數時不算。
- **過程中碰到的**：
  - `shell` 的「Browser 不見」的測試一開始失敗，原因在測試：剛 link 好的 `machopo` 第一次啟動要兩秒多，測試放掉 Browser 那一端時 child 還沒拿它那一端（macOS 上拿到的是已經死掉的 right，當成沒有通道、以 2 結束）。改成 renderer 開始讀通道、thread 開好時各在 log 說一聲，測試等那一行。
  - 同一份測試裡「手動啟動」的那一個原本要 30 秒：測試的 process 因為別的測試已經是 rendezvous 的 host，child 向它要自己的那一端、等到逾時。改成經過 shell 啟動（它的 parent 不是 host）。真的手動啟動沒有這個問題。
  - 在 macOS 上 `child_gone` 只能對已經結束的 child 呼叫：對還活著、還沒拿它那一端的 child 呼叫，它會等 30 秒才知道沒有通道。`RenderProcessHost` 是先 kill、等它結束才呼叫。
- **mutation test**（`mutants-3a.toml`）跑了 18 個，17 個抓到，1 個漏掉、留到後面：
  - 漏掉的是「Browser 那一端照 renderer 的設定建」。兩端的目的地都是兩個，而這一段沒有任何東西需要 Browser 附 handle（大的資料建不出 region 時退回放在訊息裡，照樣到）。補不出有意義的測試：要看得出來，得有非走共用記憶體不可的東西（3c 的 tile），或一個自己附 handle 的 renderer 被拒絕（3d、3e 的壞訊息）。從清單拿掉、在清單的開頭寫明，3c 和 3d 做的時候放回去。
  - 沒有列成 mutant 的兩處（清單開頭有寫）：`ChildInit::gpu`（還沒有帶 GPU 的 renderer process 的測試，3c）、`end_child` 在 child 不走時 kill（測試的 renderer 都會自己結束，3d）。
  - 跑完確認檔案都還原了（重新 build 過）。
- **跑過的檢查**：`cargo test -p ipc -p protocol -p compositor -p renderer -p browser -p shell`（都過）；`cargo clippy` 同樣六個 crate `--all-targets -- -D warnings`；`cargo fmt --check`；`sanity.py` 的 7 項（新檔案先 `git add -N` 再跑：它只看追蹤的檔案）。之後加的測試和文件各自重跑了那兩組測試和 `sanity.py docs`。
- **沒有驗到的**：Linux 和 Windows（這一段只在 macOS 上編和跑；`child` 的啟動、`kill -9` 的測試在 Linux 上應該一樣，Windows 上那個測試不編）。

### 3b-1：管道放在訊息裡就能過通道（2026-11-22，macOS 26、Apple M4）

- **決定**：「已決定的事項」第 58 條（codec 認得 `Consumer`，不是線上多包一層；使用者選的，考慮了 `ipc` 之後分成獨立的專案）。
- **做了什麼**（核心 `core/` 沒有動）：
  - `ipc::data_pipe`：`Consumer` 裡面是可以被取走的（`Mutex<Option<..>>`；只有序列化經過那把鎖，其他用法都是 `&mut`）。`Serialize`：把管道取走、放進一個 thread-local、用保留的名字 `$ipc::data_pipe::Consumer` 呼叫 serializer；codec 沒拿走就還給那個值、回錯誤。`Deserialize`：只從 codec 交過來的那一個拿。`capacity()`。關掉管道的動作從 `Consumer` 的 `Drop` 搬到裡面那一層。
  - `ipc::codec`：`Place` 多 `pipe`、`unpipe`（預設不開），`Resolve` 多 `pipe`（預設沒有這條）；serializer 和 deserializer 在那個名字上攔下來；`encode` 失敗時結束為這個值開的管道。
  - `ipc::endpoint`：`send` 的 `Placing` 開管道（原本 `open_pipe` 的內容抽成共用的函式，容量用那條管道自己的）、訊息沒送出去時結束它、放掉來源、送 `PipeEnded`；`decode` 用表加上「到了還沒被拿的管道」；收到 `PipeEnded` 時，沒被任何訊息提到的那一端丟掉。
- **寫的時候想過的**：
  - serde 把值借給 `serialize`，拿不到所有權，也沒辦法從泛型的 `&T` 認出 `Consumer`。比過三種交接的辦法：thread-local（選的）、整個 process 一張表加編號（一樣是藏起來的狀態，還跨 thread）、把位址當數字傳（要 `unsafe`，而且別的型別可以冒用名字傳假的位址）。thread-local 的範圍是同一條 thread 的同一次呼叫，兩邊都保證拿走或還回去（`soundness.md` 的 I9）。
  - `encode` 結束時原本多清一次那個 thread-local：`Consumer` 的 `serialize` 自己一定會拿回去，所以是多餘的，拿掉了（列成 mutant 會是等價的）。
- **測試**：`crates/ipc/tests/it/pipe_in_message.rs`（新的，7 個）、`crates/ipc/tests/it/endpoint.rs` 多 5 個（兩條管道跟著一則訊息、容量不變、通道結束時、佇列滿時、child 不能送）、endpoint 的 fuzz harness 多一種操作和一種訊息。`cargo test -p ipc`：單元 91 個、整合 87 個。
- **mutation test**（`mutants-3b1.toml`）9 個，全部抓到；跑完確認檔案還原（還原後重跑測試）。沒有列成 mutant 的兩處寫在清單開頭：結束管道時通知對方、對方丟掉替它留的那一端。漏做的後果是多留一個 `Consumer` 到通道結束，測試看不到，靠審查。
- **跑過的檢查**：`cargo clippy -p ipc --all-targets -- -D warnings`（三種 cfg）、`cargo fmt --check`、`sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`docs`；`test.py ipc fuzz --minutes 3`（`endpoint` 129 萬次、`frame` 2207 萬次、`pipe` 664 萬次、`region` 163 萬次，沒有發現）；`test.py ipc miri`（核心 49 個；端點和新的測試 13 個：Miri 的清單加了 `endpoint::pipes_in_a_message`、`endpoint::a_message_the_channel_cant_take` 和整個 `pipe_in_message`）。
  - fuzz 和 Miri 我一開始沒有先問就要跑，被權限擋下；使用者同意之後才跑的。
- **沒有跑的**：`test.py ipc models`、`verus`、`tla`（核心沒有動）。Linux 和 Windows 沒有驗：改的是不分平台的部分，那兩台機器 pull 之後要跑一次 `cargo test -p ipc`。
- **還沒接上的**：`protocol` 的 `CommitNavigation` 還標著 `#[serde(skip)]`，renderer process 還收不到 body 的管道（3b-2）。

### 3b-2：renderer process 收到 body 的管道（2026-11-22，macOS 26、Apple M4）

- **做了什麼**：`protocol` 的 `CommitNavigation` 的 `body_pipe` 拿掉 `#[serde(skip)]`，就這一處：`Consumer` 能序列化之後（3b-1），commit 帶著的管道跟著訊息過通道。`browser` 和 `renderer` 的程式沒有改：Browser 照舊把 `Consumer` 放進訊息，renderer 的 main thread 照舊讀它，只是 renderer process 裡讀到的是它自己那一側的管道。
- **邊界上最後一個例外消掉**：`sanity.py` 的 `PROTOCOL_ALLOWED` 拿掉 `renderer.rs` 那一項，`docs/architecture.md` 的例外那一段改成「現在沒有」；`protocol` 的 `boundary` 測試不用改（它在編譯時要求每種訊息都能序列化，`Consumer` 現在滿足）。
- **測試**：`crates/browser/tests/it/renderer_process.rs` 多一個：測試的 server 送一份 1.5 MB 的文件（超過管道的容量 512 KiB，所以背壓一定用到），標題放在最後；renderer process 說出那個標題，表示每個位元組照順序到了。重複 3 次都過。
  - 第一次失敗是測試自己的問題：它拿 commit 之後的第一個標題（空的），文件最後的標題是之後才更新的。改成等到非空的標題。
- **mutation test**（`mutants-3b2.toml`）2 個，都抓到：把 `#[serde(skip)]` 放回去；renderer 那一端不准 Browser 開管道。後者是 3a 那時看不出來的同一類（兩端的設定），現在有管道就看得出來了；「Browser 那一端照 renderer 的設定建」還是看不出來，等 3c。跑完確認檔案還原。
- **跑過的檢查**：`cargo test -p ipc -p protocol -p renderer -p browser -p shell`（都過）；`cargo clippy` 六個 crate `--all-targets -- -D warnings`；`cargo fmt --check`；`sanity.py` 的 7 項。
- **這次才發現的**：clippy 在 `renderer_process.rs` 找到兩處（多餘的借用、只有一個分支的 `match`），是 3a 最後補的三個測試裡的。3a 那時 clippy 是在補那幾個測試之前跑的，之後沒有再跑，就 commit 了。這一段修掉。之後的做法：最後一次改程式之後再跑一次 clippy，不只是「跑過」。
- **沒有驗到的**：Linux 和 Windows。

### 3c：守住 renderer process 現在對 tile 和大資料做的事（2026-11-22，macOS 26、Apple M4）

- **決定**：第 56 條的調整（「只複製一次」移到 3e 之後的 3f；使用者確認的，這份 RFC 結束前要做到）。
- **開始時試出來的**：我原本以為有 GPU 時 renderer process 的 tile 過不去。寫了一個暫時的測試（試完還原）：過得去。`CreateSharedImage` 的像素是訊息裡的位元組，codec 照一般的大位元組處理。所以 3c 沒有功能要做，要做的是把現況用測試守住、把和第 24 條的差距寫清楚。
- **現在實際的路**（讀程式確認的）：
  - renderer → Browser 的大資料（tile 的像素、blob）：Browser 沒有給 renderer 可以寫的 region（沒有人呼叫 `grant_to_peer`），所以都放在訊息裡。像素複製三次：compositor 序列化時、Browser 的 IO thread 解碼時、Viz 交給 GPU 時。
  - Browser → renderer 的大資料（子資源的回應）：Browser 需要時自己建 region，renderer 讀出來之後還回去重複用。
- **做了什麼**：`WebContents::renderer_shared_memory()`（Browser 替那個 renderer process 留的 region 的總量和空的塊數；測試要看，之後列出每個 process 用多少記憶體時也用得到）；測試的輔助型別（`DrivenViz`、`RecordingGpu`）開放給同一個測試 binary 的其他模組。
- **測試**（`crates/browser/tests/it/renderer_process.rs` 多 3 個，共 10 個）：renderer process 加有 GPU 的 Viz，tile 的 shared image 到、frame 也到；GPU 遺失時 Browser 經過通道告訴 compositor，舊的 shared image 都銷毀、新的再來；Browser 送一份 300 KiB 的 script 給 renderer process，script 跑了，而且 Browser 為它建了一塊 512 KiB 的 region（之前是 0）。
- **mutation test**（`mutants-3c.toml`）3 個，都抓到：Browser 那一端照 renderer 的設定建（3a 那時漏掉的那個，現在由「大資料走 region」的測試抓到；因為它 9 秒就被抓到，另外手動套一次確認是那個測試失敗、不是編不過）；告訴 renderer process 沒有 GPU（3a 那時沒有列的那個）；`renderer_shared_memory` 回空的。3a 的清單開頭改成指向這裡。跑完確認檔案還原。
- **跑過的檢查**（都在最後一次改程式之後跑）：`cargo test -p browser -p shell`；`cargo clippy` 六個 crate（`ipc`、`protocol`、`compositor`、`renderer`、`browser`、`shell`）`--all-targets -- -D warnings`；`cargo fmt --check`；`sanity.py` 的 7 項。
- **沒有驗到的**：Linux 和 Windows。

### 3d：renderer process 不在了（2026-11-22，macOS 26、Apple M4）

- **決定**：沒有新的大決定，照「process 當掉」寫的做。做的時候定的幾件小事記在這裡（下面「選的」）。
- **開始時試出來的**（暫時的測試，試完還原）：`about:crash` 之後 renderer process 50 ms 內自己結束、重新載入會啟動新的、連跑著無窮迴圈的頁面也會結束（關閉的訊息會打斷 script）。但都靠 renderer 配合：Browser 沒有去結束 process，結束的 process 沒有被回收（一直是 zombie，到重新載入或關分頁），通道拒絕的東西只被當成「對方不見了」。
- **做了什麼**：
  - `browser` 的 `RenderProcessHost`：`kill` 對 renderer process 是直接 kill 並等它結束（不請它，它可能不聽），回來時 process 已經沒有了；`ended`（分頁收到 `RendererGone` 時呼叫）把 process 送終：給 200 ms 讓它自己結束（當掉的 process 通道比 process 早一點點消失），沒有就 kill，都回收，結束的狀態記進 log；`crash`（送 `RendererMsg::Crash`）。
  - 通道自己判定的壞訊息（`Gone::BadMessage`、`Gone::QueueFull`）在 IO thread 上先當成壞訊息交給分頁（新的 `BadMessageReason::Channel`），再照通道結束處理：和 `IoThreadHost` 的檢查判定的走同一條路，分頁結束那個 renderer。
  - `protocol` 的 `RendererMsg::Crash`；`renderer` 的 child 的 IO thread 一解出它就 `std::process::abort()`（在 IO thread 上，所以 main thread 卡住也有效）；同一個 process 裡的 renderer 收到的話不理（Browser 不會送）。
  - `about:crash`：renderer process 自己異常終止，分頁經過 `RendererGone` 得知（不是自己先標成當掉）；同一個 process 的照舊由 Browser 結束。
  - `ipc` 的端點：通道結束時把這一端留著送大資料的 region 放掉（原本要等最後一個 `Link` 被丟掉，而分頁會留著當掉的 renderer 的 host 到重新載入）。
- **選的**：
  - `about:crash` 用訊息讓 child 自己 `abort`，不是 Browser 送 signal：三個平台一樣，而且測得到「真的當掉」那條路（通道先斷、分頁才知道）。
  - 被 kill 之後 `renderer_pid()` 和 `renderer_shared_memory()` 回空的：那個 process 沒有了。
  - 沒有做「不回應」的偵測（Chromium 的 hang monitor）：RFC 的「process 當掉」沒有列，記在 `docs/limitations.md`。
- **測試**（`crates/browser/tests/it/renderer_process.rs` 多 4 個，共 13 個，重複 5 次都過；看 process 還在不在的那幾個只在 Unix）：`about:crash` 之後分頁先不是當掉、收到 `RendererGone` 才是，process 連 zombie 都沒留下，重新載入啟動另一個 process、頁面載入；main thread 卡在無窮迴圈的 renderer 也當掉；送了不是訊息的東西的 process（通道拒絕，`Channel`）和送了別人的 cookie 的 process（Browser 的檢查，`UnknownLoader`）都被 kill，而它們自己是不會結束的。`crates/ipc/tests/it/endpoint.rs`：通道結束後這一端的 region 是 0。
- **mutation test**（`mutants-3d.toml`）6 個，全部抓到；跑完確認檔案還原。
  - 「被 kill 的 process 其實沒死」那個 mutant 讓整個 run 卡了 20 分鐘：它讓測試裡故意不結束的 process 真的留下來（60 個），握著測試輸出的 pipe。手動停掉之後繼續。測試用的 process 現在都有壽命上限（90 秒自己結束），和 `ipc` 的測試 child 同樣的做法；這是第二次碰到同一種卡法，之後寫會啟動 process 的測試時一開始就加。
  - 沒有列成 mutant 的兩處寫在清單開頭：佇列超過上限也算通道的壞訊息（要 256 MiB 沒人讀，測試送不出來）、等 200 ms 的那個數字（只影響 log 寫的結束方式）。
- **跑過的檢查**（都在最後一次改程式之後跑）：`cargo test -p ipc -p protocol -p renderer -p browser -p shell`；`cargo clippy` 六個 crate `--all-targets -- -D warnings`，`ipc` 另外兩種 cfg；`cargo fmt --check`；`sanity.py` 的 7 項。跑完確認沒有留下測試啟動的 process。
- **沒有跑的**：`test.py ipc` 的 fuzz 和 Miri（`ipc` 這一段只多一行，在端點結束時換掉池子；沒有先問使用者就不跑）。
- **沒有驗到的**：Linux 和 Windows。Windows 上看 process 還在不在的測試不編（`ps`），kill 那條路在那裡只有「分頁變成當掉」被測到。

### 3e-1：切換的旗標，和用多 process 跑測試的選項（2026-11-22，macOS 26、Apple M4）

- **決定**：「已決定的事項」第 59 條：先加旗標、預設不變，3h 驗收時才換；收尾時視窗和 pipe 都預設多 process（看過下面的數字之後定的）。
- **做了什麼**：
  - `machopo` 的 `--single-process`（預設）和 `--process-per-tab`，視窗和 `--automation-pipe` 都認；兩個一起給是錯誤。`shell::renderer_launcher`：多 process 時啟動的是正在跑的這個執行檔加 `--type=renderer`。
  - `test.py` 的 `wpt`、`reftest`、`wdspec`、`scenarios --window` 加 `--process-per-tab`：設環境變數 `MACHOPO_PROCESS_PER_TAB`，WPT 的 product（`tests/wpt/wptrunner_machopo`）和 `shell` 的情境、pipe 測試看到就替 `machopo` 加上旗標。`scenarios` 不加 `--window` 時不能用（那是在測試的 process 裡跑的）。
  - 每天的 CI workflow（`nightly.yml`）加一個手動開始時的輸入 `process_per_tab`，選了就讓 reftest、wpt、wdspec 用它跑。每天自動跑的內容不變。
- **量的**（決定收尾預設的依據）：

  | | 單一 process | `--process-per-tab` |
  | --- | --- | --- |
  | `shell` 的 pipe 測試（36 個，`cargo test -p shell --test it -- automation_pipe`） | 36 過，4.4 秒 | 36 過，4.6 秒 |
  | WPT `dom/events`（217 個測試、1069 項檢查，`test.py wpt dom/events`） | 83 秒 | 71 秒 |

  WPT 的結果兩邊逐項相同（各有同樣的 4 個意外通過，和這次無關：兩邊都被同一個逾時的測試佔掉 64 秒，所以時間的差距是雜訊）。
- **第一次用真的 `machopo` 跑多 process 找到的**：pipe 測試 36 個裡壞 1 個（`the_outer_size_is_the_windows`）：它導覽到 `about:crash` 之後馬上再導覽。3d 把 renderer process 的 `about:crash` 做成送出去就回來、等通道斷了才算當掉，下一個導覽撞上正在死的 renderer。單一 process 時是當場生效的。
  - 改成：`RenderProcessHost::crash` 送出之後等 process 結束（最多 5 秒，不然 kill），回來時已經沒有了；分頁照 kill 的路當場標成當掉。兩種模式行為一致。3d 那一節寫的「分頁經過 `RendererGone` 得知」因此不再成立；`architecture.md` 改了，3d 的 mutant 清單裡對應的那一個也改了。
  - 多了 `WebContents::renderer_exit()`（renderer process 是怎麼結束的）。測試用它確認 `about:crash` 是 process 自己 abort（signal 6）、壞訊息是被 kill（signal 9），不會把「沒當掉、時限到了被 kill」當成通過。
- **測試**：`machopo` 的參數解析多一個（兩種模式的預設、兩個旗標、一起給）；`crates/shell/tests/it/renderer_process.rs` 多一個（只在 Unix）：pipe 的 `machopo` 加 `--process-per-tab` 時底下有一個 renderer process，不加或加 `--single-process` 時沒有，pipe 關掉之後 Browser 和 renderer 都結束。
  - 那個測試第一次失敗是測試自己的問題：`pgrep -f --type=renderer` 把樣式當成它自己的選項。
- **mutation test**（`mutants-3e1.toml`）6 個，全部抓到（旗標收下但不生效、永遠多 process、兩個旗標一起給也收、沒送「當掉」、改成直接 kill、不記得怎麼結束的）；跑完確認檔案還原。
- **跑過的檢查**（都在最後一次改程式之後）：`cargo test -p ipc -p protocol -p renderer -p browser -p shell`；`MACHOPO_PROCESS_PER_TAB=1 cargo test -p shell --test it -- automation_pipe`（36 過）；`cargo clippy` 六個 crate `--all-targets -- -D warnings`；`cargo fmt --check`；`sanity.py` 的 7 項。
- **沒有驗到的**：
  - `nightly.yml` 的改動在本機跑不了，也沒有工具檢查它的語法：要在 GitHub 上手動開始一次才知道。
  - 整套 WPT、reftest、wdspec 在多 process 底下（3e-2，用上面那個手動的輸入）；真的視窗（`scenarios --window --process-per-tab`，還沒跑）；Linux 和 Windows。

### 3e-2：整套測試在多 process 底下跑（進行中）

- **真的視窗接 renderer process**（2026-11-22，macOS 26、Apple M4，Metal）：`python3 scripts/test.py scenarios --window --process-per-tab`，72 個情境（每個一個真的視窗，一次一個）全部通過，加上 pipe 開視窗的那個測試，共 85 秒。這是 renderer process 第一次接上 winit、wgpu 和真的 GPU：tile 上 GPU、輸入、捲動、動畫、canvas、分頁、TodoMVC（JS 和 wasm 的）都在裡面。
  - 確認真的走了多 process：72 份 `machopo.log` 每一份都有「a renderer process ended」那一行（Browser 回收 renderer process 時記的）。看了 `todomvc-app-js` 的截圖，畫面正確。
  - renderer process 的 log 也在同一份 `machopo.log` 裡（它繼承了 stderr）。
- **Browser 這一側的 fuzz target**（第 60 條；2026-11-22）：
  - `browser`：`RendererLauncher::Detached`（另一端沒有東西的 renderer）、`WebContents::deliver_from_renderer` 和 `deliver_renderer_gone`（把 renderer 的訊息、「它不見了」交進來，當場處理完）。
  - `net`：host 規則把 host 對到「不存在」時，寫成 IP 的網址也在送出前拒絕（`HttpTransport::send`）。原本 IP 的網址不經過解析，規則管不到；fuzz 用 `MAP * ^NOTFOUND` 保證什麼都連不出去要靠它。測試（`crates/net/tests/it/load.rs`）：`localhost`、`127.0.0.1` 和一個名字都被拒絕，server 一個請求都沒收到。
  - harness：`crates/browser/tests/it/renderer_message_fuzz.rs`（`run`、它自己的測試、重放 `crates/browser/fuzz/regressions/`）；cargo-fuzz 的 target 在 `crates/browser/fuzz/`（自己的 workspace，和 `ipc` 的釘同一個 nightly）；`test.py browser fuzz`，`test.py` 裡跑 fuzzer 的那一段和 `ipc` 共用。
  - 所有輸入共用一份記憶體裡的 profile（啟動網路那一側是一次 run 大部分的成本）：cookie 和 blob URL 會留給下一個輸入，和它們有關的發現重放時可能要連同前面的輸入。
  - **跑了一次**（使用者同意之後）：`test.py browser fuzz --minutes 10`，394,876 次，每秒約 657 次，沒有發現。最後一分鐘還在長新的涵蓋（6028 條邊、語料 1438 份），所以 10 分鐘還沒跑到頭；之後改到這幾個檔案時再跑。
  - mutation test（`mutants-3e2.toml`）2 個，都抓到（IP 的網址不擋、交進來的訊息丟掉）；沒有列的寫在清單開頭（harness 本身是測試的程式；`deliver_gone` 只要求不 panic）。跑完確認檔案還原。
  - 跑過的檢查（最後一次改程式之後）：`cargo test -p net -p browser`、`cargo clippy` 七個 crate（加上 `net`）、`cargo fmt --check`、`sanity.py` 的 7 項；fuzz 的 crate 在它的 nightly 上 `cargo check` 過。
- **還沒做的**：整套 WPT、reftest、wdspec 用 `--process-per-tab`（用 `nightly.yml` 手動開始時的輸入，在 CI 上跑；那個輸入本身也還沒驗過）。這一項要使用者在 GitHub 上開始。
- **CI 的結果到這時才第一次看**（2026-11-22，使用者問了才看的）：`0004` 上每次 push 都有 CI（Ubuntu），從 3a 起我一次都沒看。結果：
  - 3a（`84d9fd0`）：lint 失敗。就是 3b-2 那時本機才發現、已經修掉的兩處 clippy。
  - 3b-2（`365507c`）：通過。
  - 3c（`adb1098`）：`shell` 的 `documents::history_through_the_browser` 失敗一次（`popstate` 和 `hashchange` 之後的兩個布林值順序相反）。那個測試是在測試的 process 裡跑的，3c 沒有改到它走的路；`main` 在這之前的 CI 也有別的 `shell` 測試偶爾失敗（`devtools_css::sheets_coming_and_going_are_told`）。看起來是原本就有的不穩定，沒有查原因，告訴使用者了。
  - 3d（`1674686`）和 3e-1（`d03a129`）：`about:crash` 的兩個測試在 Linux 上失敗。3d 的是等 30 秒收不到 `RendererGone`；3e-1 的是 process 被 kill（signal 9）而不是自己 abort（signal 6）。兩者是同一件事：在那台機器上 `abort()` 之後 process 沒有馬上結束。原因我判斷是 core dump：Ubuntu 把 core dump 用 pipe 交給收集程式，process 要等它收完才算結束，通道也到那時才斷。macOS 上沒有這件事，所以本機一直是過的。
  - **修正**：renderer process 被要求當掉時，在 Linux 上先把自己設成不留 core dump（`prctl(PR_SET_DUMPABLE, 0)`，經過 rustix 的 `set_dumpable_behavior`，`renderer` 在 Linux 上多依賴 rustix 的 `process`），再 `abort`。這段只在 Linux 上編，本機編不到：確認了 rustix 的原始碼裡有這個函式、`cargo metadata` 以 Linux 為目標時 `renderer` 拿得到它，剩下的要靠 push 之後的 CI。
  - 真的當掉的 renderer 在 Linux 上還是要等 core dump 收完 Browser 才知道，記進 `docs/limitations.md`。
  - 之後的做法：push 之後看 CI，不是只看本機。
  - 修正之後（`9784771`，2026-11-22）：`about:crash` 的兩個測試在 Linux 上過了。那一次 CI 還是紅的，壞的是 `shell` 的 `devtools_css::sheets_coming_and_going_are_told`（等 30 秒收不到 `CSS.styleSheetAdded`）：就是上面說 `main` 上也偶爾失敗的那一個，在測試的 process 裡跑、和這幾步無關，還沒有查原因。

### 3f：tile 只複製一次（2026-11-22，macOS 26、Apple M4）

- **決定**：「已決定的事項」第 61 條（用 `ipc` 現有的 region，加上「借出去畫」）、第 62 條（先給一組、不夠再要、compositor 不等）、第 63 條（Viz 直接複製進 GPU 的緩衝區）。三條都是使用者選的；62 和 63 各有一段「實作時定的」。
- **做了什麼**：
  - `ipc`：新的 `shared_buffer` 模組。`Link::take_region(len)` 拿一塊空的 region（`Writer`，裡面是 `&mut [u8]`）；`SharedBuffer` 是訊息裡的欄位型別（自己的位元組、這一端寫好的 region、對方寫的 region）；codec 認得它（和 `Consumer` 一樣經過 thread-local 交接），線上的格式和原本「大的位元組在 region 裡」完全一樣；收的一端解出來的是 region 本身，只能複製出來（`copy_to`、`chunks`），最後一個 clone 被丟掉時才送 `Released`。
  - `ipc` 的池子多一個「被拿著」的狀態（`Pool::take` 到 `Pool::lend` 之間）：這段期間對方說「讀完了」是壞訊息。少了它，惡意的對方可以讓同一塊記憶體同時有兩個可寫的 slice。
  - `ipc` 多一則控制訊息 `Wanted`（沒有空的 region、大的位元組只好放進訊息時送），和 `Config::peer_region_limit`（Browser 替對方建的 region 的上限；原本 `grant_to_peer` 沒有上限）。
  - `Platform::Memory` 多要求 `Sync`（mapping 現在會被 IO thread 以外的 thread 拿著）。三個平台的 `Mapping` 本來就是，平台層沒有改。
  - `protocol`：`CreateSharedImage` 的像素從 `Bytes` 改成 `SharedBuffer`（`sanity.py boundary` 的例外少一個）；`CompositorFrameSinkSender::take_region`。
  - `compositor`：`SharedImages::with_regions`；`PixelBuffer` 可以是池子的一塊或通道的一塊 region。
  - `renderer`：有 GPU 時把 `take_region` 交給 `SharedImages`。`browser`：啟動有 GPU 的 renderer process 時先給 16 塊 256 KiB。
  - `viz`：像素來自共用記憶體時（`Pixels::Shared`），`GpuContext::stage` 建一個 mapped 的緩衝區、把像素複製進去、排一個「緩衝區到 texture」的複製；一批的複製指令在 flush 或畫 frame 的那次 submit 一起送。
- **和原本想的不一樣的**：
  - wgpu 30 的 mapped 緩衝區只能寫（`WriteOnly`），拿不到 `&mut [u8]`。改成以值一段一段交（第 63 條的「實作時發現、改掉的」）。量的（`wgpu-types` 的 `WriteOnly` 包一般的記憶體，256 KiB，64 組來源和目的地輪流）：`copy_from_slice` 5.8 到 8.1 µs；一段 64 bytes 5.2 到 6.2 µs、1024 bytes 4.6 到 4.8 µs、4096 bytes 5.2 µs。
  - 「要一塊」原本想由 compositor 送，做成 `ipc` 在大的位元組放進訊息時自己送（第 62 條）。副作用是 renderer 送給 Browser 的其他大資料也開始走共用記憶體（3c 記的「都放在訊息裡」不再成立，`architecture.md` 和 `limitations.md` 改了）。
  - RFC 的「tile 交接」寫的是用一個複製次數的計數守著。沒有做成那樣（複製發生在三個 crate 裡），改成每一段各有測試；那一節改了。接線的那一段守不住，記在 `docs/test-gaps.md` 的「沒有計數守著的優化」。
- **測試**：
  - `ipc`：`core/region.rs` 多 3 個單元測試（拿著的狀態、別的 region 離開時狀態跟對、slice 和複製出來）；`core/control.rs` 的三個測試加上 `Wanted`；`tests/it/shared_buffer.rs`（新的，10 個，內容在 `crates/ipc/README.md` 的「測試」）；`region_fuzz` 的模型多「交給訊息」的操作和「拿著」的狀態；`endpoint.rs` 的一個測試改了預期（沒有 region 的 child 送完大資料之後自己得到一塊）。
  - `compositor`：`a_tile_is_drawn_in_a_region_of_the_channels_when_one_is_free`。
  - `viz`：`pixels_in_shared_memory_are_what_is_drawn`（真的 GPU：四種大小涵蓋「一列剛好 1024 bytes」「不到 256」「超過 256 又不是倍數」「一列 4 bytes」，寫在新的和池子裡的 texture 上，flush 過的和靠畫 frame 那次 submit 的）。
  - `browser`（真的 renderer process）：`a_renderer_processs_tiles_go_to_the_gpu_in_shared_memory`（每一塊 tile 都是共用記憶體裡的，放在訊息裡的是 0 塊，讀出來不是單色）；`a_renderer_process_out_of_shared_memory_copies_its_tiles_and_is_given_more`（Viz 不放手時，renderer 不停下來、退回複製，之後要到超過 16 塊）。
  - 寫測試時自己弄錯的兩次：`viz` 的測試第二輪用了同一個 resource 編號（Viz 照編號留著 resource），我一開始以為是實作沒有把暫存的複製送出去；`region.rs` 的一個測試算錯哪一塊會被選到。
- **mutation test**（`mutants-3f.toml`）28 個（視窗那一次之後 29 個），全部抓到；沒有列的寫在清單開頭。為了它補的測試：送過的 `SharedBuffer` 在那塊 region 被別人拿走之後不能再送（少了 `sent` 旗標時它會把別人正在寫的 region 送出去）；別的通道同一個編號的 region 被拿著時也不會被借走；flush 之後沒有暫存的複製留著。跑完確認檔案還原（`target/mutate/originals` 是空的）。
- **跑過的檢查**（都在最後一次改程式之後）：`cargo test -p ipc -p protocol -p compositor -p viz -p renderer -p browser -p shell`；`MACHOPO_PROCESS_PER_TAB=1 cargo test -p shell --test it -- automation_pipe renderer_process`（42 過）；`cargo clippy` 七個 crate `--all-targets -- -D warnings`；`cargo fmt --check`；`sanity.py` 的 7 項（在 `git add -N` 新檔案之後）。
- **Miri 和 fuzz**（使用者同意之後；2026-11-22）：
  - `test.py ipc miri`：核心的單元測試 52 個過（564 秒），端點的 23 個過（含 `shared_buffer::` 的 10 個；2633 秒），沒有回報 undefined behavior。端點那一段從幾分鐘變成 44 分鐘：`shared_buffer` 的測試每個都搬 256 KiB，在 Miri 底下很慢。之後把那幾個測試的資料縮小。
  - `test.py ipc fuzz`（4 個 target 各 5 分鐘）：`endpoint` 182 萬次、`frame` 3485 萬次、`pipe` 1081 萬次、`region` 225 萬次（模型加了「拿著」和「交給訊息」之後的），沒有發現。
- **真的視窗**（`scenarios --window --process-per-tab`，使用者同意之後；2026-11-22，Metal）：
  - 第一次的結果不能用：72 份 `machopo.log` 每一份都是「viz: can't draw … occluded」，沒有一張截圖。`pmset -g log` 顯示那段時間顯示器是關的；視窗看不到時 Viz 不畫。之後跑這一項用 `caffeinate -di`（`-d` 不讓顯示器睡），跑之前確認顯示器開著。
  - 顯示器開著再跑：72 個裡 `gpu-crash` 失敗。**是這一步的 bug**：GPU device 遺失之後，新的上傳路徑向遺失的 device 建緩衝區，拿到的是無效的；我處理了「map 不到」，卻照樣呼叫 `unmap`，wgpu 把那當成致命錯誤，Viz thread panic。原本的 `write_texture` 在遺失的 device 上不會這樣。單元測試沒有涵蓋「device 遺失之後上傳共用記憶體裡的像素」，是情境抓到的。
  - 修正：map 不到就直接回去，不 `unmap`（`GpuContext::stage`）。`viz` 的 `pixels_in_shared_memory_are_what_is_drawn` 多一段：device 遺失之前暫存的、遺失之後送來的都不出事，新的 device 照樣畫。mutant 多一個（無效的緩衝區照樣 `unmap`），抓到；清單現在 29 個。
  - 修正之後整套重跑：72 個全部通過；72 份 log 都有「a renderer process ended」（確實是多 process），沒有一份有 `ERROR` 或 panic；看了 `todomvc-app-js` 的截圖，畫面正確。之後 `cargo test -p viz`、`cargo clippy -p viz`、`cargo fmt --check` 再跑過。
- **沒有驗到的**：
  - Linux 和 Windows：平台層沒有改，但 `Platform::Memory: Sync` 和其他的改動在那兩個平台上有沒有編過，Linux 要看 push 之後的 CI，Windows 要在那台機器上編。
  - 時間：這一步沒有量每個 frame 快了多少（3g）。
- **push 之後的 CI**（`0440bf9`，Linux）：lint 過；測試壞一個，`ipc` 的 `process_channel::a_host_and_a_child_talk`（host 自己的 region 是 1 MiB，預期 512 KiB）。Linux 上這一步的程式編得過、其他的都過（包括 `shared_buffer` 的和 `browser` 的 renderer process 測試）。
  - 原因是測試本來就有的競爭，被這一步的時間差帶出來：測試連送四筆大資料，每筆等到回音就送下一筆，但 child 是「先回答、再說讀完了」，下一筆送出時 host 的那塊 region 可能還沒被還回來，host 就再建一塊。macOS 上兩則訊息總是一起到，所以沒發生過。行為本身是對的。
  - 改測試：每一筆都等到 child 說讀完了才送下一筆（原本只有第一筆這樣等）。本機重複 5 次都過；Linux 上要等下一次 CI 才知道。

### 3g：trace 和量測（2026-11-22，macOS 26、Apple M4，接電源）

- **決定**：「已決定的事項」第 64 條（renderer process 寫自己的 trace 檔）、第 65 條（3f 之前的現況用暫時的 worktree 補量）、第 66 條（量三層，只量不改）。都是使用者選的。
- **做了什麼**：
  - `machopo`：renderer process 在 `MACHOPO_TRACE` 設了的時候寫 `<檔>.<pid>`（`main.rs` 的 `start_trace`，Browser 和 renderer 共用）。
  - 量測用的記錄點：compositor 送出 frame 時的 debug log（`frame submitted`）、Viz 收到和畫出來時的（`viz: frame arrived`、`viz: frame drawn`，帶 sink 和 frame 的編號）、Browser 的 IO thread 處理 renderer process 每則訊息的 span（`ChannelHost::OnMessage`；`browser` 多依賴 `trace_event`）。
  - frame 的延遲用 log 對、不用 trace：span 現在帶不了參數（要帶編號得改 rusty_v8 的 C++、重編 V8），而兩個 process 的 log 是同一個檔、時間是微秒。
  - `bench.py raster`：`--process-per-tab`、`--binary`（量別的 commit 編出來的）、`--name`（結果各放一個資料夾）；讀 renderer 自己的 trace 檔；第三張表（Browser 這一側，和 frame 到畫面的時間）。
- **怎麼量的**：`bench.py raster`，不開視窗（device scale factor 2，GPU 照用），`perf` profile，三個頁面各捲動 120 次、改視窗大小 82 次。三種組態交錯著各跑 3 次，下面是 3 次的中位數。「3f 之前」是 `9784771` 加上這一步的 trace 和記錄點（在暫時的 worktree 裡編的）。原始結果在 `target/bench/raster/{single,multi,before}-{1,2,3}/`。
- **每塊 tile 的成本**（µs；從 Gutenberg 捲動那一段算，612 塊，每塊 256 KiB；其他頁面的比例相近）：

  | | 單一 process | 3f 之前的多 process | 現在的多 process |
  | --- | --- | --- | --- |
  | compositor 送出一塊（`TileManager::UploadTile`） | 3.4 | 94 | 5.6 |
  | Browser 的 IO thread（那一段全部的訊息，除以 tile 數） | 沒有 | 59 | 14 |
  | Viz 放上 GPU（`SharedImage::Write`） | 150 | 153 | 163 |

- **compositor 準備 tile 的總時間**（`PrepareTiles`，ms）：

  | 頁面、動作 | 單一 process | 3f 之前 | 現在 |
  | --- | --- | --- | --- |
  | Gutenberg 捲動 | 340.9 | 388.3 | 339.7 |
  | Gutenberg 改大小 | 819.2 | 1021.4 | 818.8 |
  | SQLite 改大小 | 605.9 | 697.2 | 595.5 |
  | NewsSite 捲動 | 246.1 | 302.1 | 219.0 |
  | NewsSite 改大小 | 1267.2 | 1451.7 | 1217.9 |

  改大小的那幾段各組態畫的 frame 數不一樣（慢的組態在同樣的時間裡畫得少：NewsSite 改大小是 67、47、71 個 frame），所以總時間只能大致比；每塊 tile 的成本比較準。
- **Browser 的 IO thread**：一則訊息最長 0.1 ms（3f 之前 0.5 ms）；最忙的一段（NewsSite 改大小，39780 則）合計 16.5 ms（3f 之前 25225 則 266 ms）。
- **每個 frame 從送出到畫出來**（ms，中位數／第 95 百分位）：

  | 頁面、動作 | 單一 process | 3f 之前 | 現在 |
  | --- | --- | --- | --- |
  | Gutenberg 捲動 | 0.63 / 0.89 | 0.66 / 0.94 | 0.69 / 1.03 |
  | Gutenberg 改大小 | 0.20 / 0.35 | 0.18 / 0.34 | 0.19 / 0.33 |
  | SQLite 改大小 | 0.24 / 0.58 | 0.22 / 0.48 | 0.24 / 0.47 |
  | NewsSite 捲動 | 1.36 / 1.90 | 1.25 / 1.92 | 1.47 / 1.93 |
  | NewsSite 改大小 | 0.77 / 1.47 | 0.74 / 1.15 | 1.50 / 3.44 |

  其中「送出到 Viz 收到」（過通道和 IO thread 的那一段）中位數是 0.02 到 0.05 ms，單一 process 是 0.01 ms；NewsSite 改大小例外（現在 0.31 ms，單一 process 0.12 ms）。
- **看得出來的**：
  1. 3f 有效：compositor 送一塊 tile 從 94 µs 降到 5.6 µs，IO thread 從 59 µs 降到 14 µs。多 process 準備 tile 的時間現在和單一 process 一樣（3f 之前多 14% 到 25%）。
  2. IO thread 不是瓶頸：一則訊息最長 0.1 ms，最忙時佔那一段時間的 1% 左右。
  3. frame 的延遲三種組態幾乎一樣，都在 2 ms 以內，離一個 frame 的 16.7 ms 很遠。
  4. 一個例外：NewsSite 改大小（2 萬塊 tile，最重的一段）現在的延遲是單一 process 的兩倍（中位數 1.5 對 0.77 ms，第 95 百分位 3.4 對 1.5 ms）。Viz 從共用記憶體上傳比 `write_texture` 慢約一成（每塊 163 對 150 µs，改大小的那幾段是 43 到 48 對 39 到 44 µs），Viz thread 忙著上傳時 frame 就排在後面。我判斷多出來的是每次新建 GPU 緩衝區的成本，沒有另外驗證。
  5. Browser 這一側最貴的一直是 Viz 把 tile 放上 GPU（每塊 40 到 160 µs），三種組態都是；過 process 的成本（現在每塊約 20 µs）比它小得多。
  6. 改大小時有 650 到 690 塊 tile 走退回複製的路（3% 到 12%）：一次要畫的比手上的 region 多。捲動時沒有。
- **測試**：`shell` 的 `trace::a_renderer_process_writes_a_trace_of_its_own`（多 process 時 renderer 的 thread 和頁面的 mark 只在它自己的檔，兩邊時間對得上；單一 process 時只有一個檔）；原本的兩個 trace 測試改成兩種模式都讀對檔案（`MACHOPO_PROCESS_PER_TAB=1` 也跑過）；`scripts/bench/raster/raster_test.py` 多 3 個、改 2 個（讀 log 裡的 frame、找 renderer 的 trace 檔、報告的第三張表、真的跑一次多 process），12 個過。
- **mutation test**（`mutants-3g.toml`）6 個，全部抓到；跑完確認檔案還原。
- **跑過的檢查**（最後一次改程式之後）：`cargo test -p browser -p viz -p compositor -p shell`、`cargo clippy` 這四個 crate、`cargo fmt --check`、`sanity.py` 的 7 項、`raster_test.py`。
- **沒有驗到的、要小心的**：
  - 只在 macOS、不開視窗的情況下量；真的視窗（有 vsync、要 present）和 Linux、Windows 沒有量。
  - 各 3 次，沒有算變異；差距在一成以內的（第 4 點的「慢一成」）要多跑才說得準。
  - 兩個 process 的 trace 檔接起來能不能在 Perfetto 的 UI 一起看，沒有試。
  - worktree `../machopo-before-3f` 量完、使用者同意之後刪掉了（2026-11-22）：要再量 3f 之前的，照第 65 條重開一份。
