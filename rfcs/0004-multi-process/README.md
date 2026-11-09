# RFC 0004：multi-process 和 sandbox

- 開始日期：2026-10-04
- 狀態：⏸️ 延後：2026-10-06（第 8 條：預設單一 process；只做第 0 步「整理邊界」，進度見 `progress.md`。IPC 的做法和 `ipc` 的 `unsafe` 2026-10-06 確定，見「已決定的事項」；tile、字型、sandbox 的細節和排程還沒決定）
- 細節：`details.md`

原本是 MVP 選做的 P6（`rfcs/pre-rfcs/MVP.md` 的「里程碑」），2026-10-04 決定 MVP 到 P4 為止，改成這份 RFC。

## 摘要

把 renderer 從 Browser process 裡的 thread 拆成獨立的 process，再用作業系統的 sandbox 限制它（先做 macOS 的 Seatbelt）。Renderer 和 Browser 是同一個執行檔，用 `--type=renderer` 啟動（照 Chromium）。

IPC 照 Chromium 現在的方向（Mojo 的 core 換成 ipcz 之後）自己做：表面上是 channel，共用記憶體是一級公民，最後連訊息本身也盡量放在共用記憶體裡傳。分兩步：第一步所有訊息走每個 renderer 一條的控制通道（Unix socket、Windows named pipe），大的資料放進共用記憶體、只傳 handle；量過延遲之後，第二步再把小訊息搬進共用記憶體裡的 ring，控制通道只用來喚醒和傳 handle。平台層用 rustix、memmap2、windows-sys 和（只在 Windows 上）interprocess，ring 照 shmem-ipc 的 `ringbuf.rs` 的設計自己寫；不用 Servo 的 ipc-channel。

Viz 留在 Browser process，不做 GPU process：renderer 照舊自己 raster，tile 的像素經過共用記憶體交給 Viz 上傳（Chromium 的 one-copy raster），renderer 碰不到 GPU。renderer 掛掉時頁面換成錯誤頁，Browser 照常運作。單一 process 的模式留著，給 headless、reftest 和大部分的測試用。

## 動機

**現況**：renderer 是 Browser process 裡的一條 thread（`RenderProcessHost` 啟動 `CrRendererMain` 和 `Compositor` 兩條 thread），沒有任何隔離：V8 沒開 sandbox，V8 或 `dom` 的任何記憶體錯誤都能碰到整個 browser 的記憶體（`docs/limitations.md` 的「隔離」）。renderer panic 時，IO thread 收到 channel 關閉、報告 `RendererGone`，視窗就關掉整個程式。

MVP 一路照 process 的邊界設計，所以邊界大致已經在了：
- Browser 和 renderer 之間只有 `protocol` 的 message（`RendererMsg`、`CompositorMsg`、`RendererHostMsg`），都是 serde；
- renderer 不碰檔案系統和網路：頁面和子資源都由 Browser 的 `net` 載入，`file://` 的權限由 `RenderProcessHost` 檢查；
- compositor 直接把 `CompositorFrame` 送給 Viz，不經過 Browser 的 UI thread，和 Chromium 相同；
- process 內的 channel 是 crossbeam（`rfcs/pre-rfcs/P4.md` 的「已決定的事項」第 14 條），renderer 的 main thread 只等一條 task queue（同一份的第 6 條）。

還沒準備好的地方（細節見 `details.md` 的「現在的邊界」）：
- **tile 的交接**：P4.4c 起 compositor 直接在 Viz 的 wgpu device 上建 texture、上傳像素，`SharedImage` 帶著一個不能序列化的 `Arc<dyn Any>`，生命週期靠參考計數，沒有歸還的訊息。
- **幾個 callback**：Viz 的 `BeginFrame`、GPU 的 context lost，都是 closure 或 `Weak`，不是 message。
- **啟動參數**：`RendererFlags::script_engine` 是函式指標，不能傳給另一個 process。
- **字型**：renderer 自己用 fontique 讀系統字型，sandbox 之後要確認讀得到。
- **crash**：沒有錯誤頁，也沒有重新啟動 renderer。
- **IPC 本身**：還沒有任何跨 process 的通道和共用記憶體。

**要解決的 `docs/limitations.md` 裡的限制**：「隔離」的三條（沒有任何隔離、renderer panic 時整個程式結束、`gfx::FontData` 和 `gfx::ImageData` 不能序列化）；`docs/architecture.md` 的「Process 的邊界」的例外 1。`rfcs/` 以外的文件不寫這份的編號，做完時照這份清單刪掉或改寫。

目標：
- 每個 `WebContents` 一個 renderer process（現在一個視窗只有一個 `WebContents`），renderer crash 或被 kill 時 Browser 不受影響，頁面顯示錯誤頁，重新載入時啟動新的 renderer。
- macOS 上 renderer 在 Seatbelt 的 sandbox 裡：不能讀寫檔案（字型除外）、不能連網路、不能開 GPU、不能自己建立有名字的 IPC 物件。
- IPC 把 renderer 當成不可信的：renderer 送來的訊息和共用記憶體裡的內容，Browser 都要驗證過才用；renderer 只能用 Browser 交給它的 handle，碰不到別的 renderer 的通道。
- 單一 process 的模式照樣能用，行為相同；現有的測試不用改寫成跨 process。
- `protocol` 的 message 型別盡量不變。

不在範圍內：
- GPU process（Viz 搬出 Browser）、Network Service process、utility process。
- Site isolation（依網站分 process、跨網站導航換 process）、多個分頁共用 renderer；跨網站的 iframe 也是（2026-10-07 從 RFC 0009 移到這份的「之後的可能」）。
- OOP-R（raster 搬到 GPU 那一側）。
- Linux 和 Windows 的 sandbox：照目前的平台政策，只在 macOS 上驗收（`CLAUDE.md` 的「平台」）；架構不排除它們。IPC 本身三個平台都要能跑，CI 每天在 Linux 和 Windows 上測。
- Mojo 的介面和自動產生的 binding（`.mojom` 的 `Remote` / `Receiver`）：message 照舊是 `protocol` 的 enum。

## 設計概要

給 machopo 的使用者：視窗模式預設每個頁面一個 renderer process，活動監視器裡看得到 `machopo --type=renderer`。頁面的 script 讓 renderer 當掉（或打開 `about:crash`）時，視窗還在，頁面換成「這個頁面當掉了」的錯誤頁，按重新載入就好。`--single-process` 回到現在的樣子。

給之後改 machopo 的人：
- Browser 和 renderer 之間的 message 和現在一樣定義在 `protocol`，`RenderProcessHost` 決定它們走 crossbeam（單一 process）還是新的 `ipc` crate（獨立 process），兩邊的 renderer 程式碼相同。
- renderer 不能再假設和 Browser 共用記憶體：要給 Browser 或 Viz 的東西只能是 message，或放在 Browser 配給它的共用記憶體裡。大的位元組資料用 `protocol` 裡的一個型別包起來（照 Chromium 的 `BigBuffer`），小的時候放在訊息裡，大的時候自動放進共用記憶體。
- 從 renderer 來的東西都是不可信的輸入：共用記憶體裡的內容先複製出來再解析，長度和 index 先檢查範圍。
- 共用記憶體分成唯讀、可寫和雙方都能寫三種型別（照 Chromium `base` 的 `ReadOnlySharedMemoryRegion` 等），由型別決定對方拿到的 handle 有沒有寫入的權限；只有 Browser 能建立。

## 詳細設計

### 架構

每個 renderer 和 Browser 之間有一條控制通道（Chromium `mojo::core::Channel` 的角色），兩端各有一條 IO thread 讀它：

```
Browser process                                Renderer process
  UI thread、Viz thread ──寫──┐            ┌──寫── main thread、compositor thread
                              v            v
                        ═══════ 控制通道 ═══════
                              │            │
  Chrome_IOThread <───讀──────┘            └──讀───> IO thread（新的）
    ├─ RendererHostMsg → WebContents         ├─ RendererMsg → main thread 的 task queue
    └─ CompositorFrameSinkMsg → Viz thread   └─ CompositorMsg → compositor 的 task queue
```

- **一條通道多工**：現在的四種訊息（Browser → main thread 的 `RendererMsg`、Browser 和 Viz → compositor 的 `CompositorMsg`、renderer → Browser 的 `RendererHostMsg`、compositor → Viz 的 `CompositorFrameSinkMsg`）都在同一條控制通道上，各帶一個表示目的地的標記。一條通道本身有順序，所以每種訊息照送出的順序到，不同種之間的相對順序也保留（相當於 Mojo 的 associated interface 共用一條 pipe）。
- **renderer 多一條 IO thread**：讀控制通道，依標記放進 main thread 或 compositor thread 的 crossbeam task queue。main thread 照舊只等一條 queue（`rfcs/pre-rfcs/P4.md` 的「已決定的事項」第 6 條）。P4 把「renderer 怎麼讀 IPC」留到有 IPC 時再決定；IPC 自己做之後沒有 ipc-channel 的 router 可用，所以照 Chromium 加這條 thread（「已決定的事項」第 6 條）。
- **Browser 的 IO thread**：沿用現在每個 `RenderProcessHost` 一條的 `Chrome_IOThread`，改成讀控制通道：`RendererHostMsg` 照現在的方式處理，`CompositorFrameSinkMsg` 轉給 Viz thread。Chromium 的 renderer 有直接連到 GPU process 的 pipe；machopo 的 Viz 在 Browser 裡，所以多一次 IO thread 到 Viz thread 的轉手。
- **送的一端**：任何 thread 都能送，寫進控制通道的送出端（Unix 上是 `UnixStream` 的另一個 clone，Windows 上是 interprocess 拆出來的送出端），各 thread 之間用一把鎖保證一則訊息整個寫完。
- **crossbeam 留著**：IPC 收到的訊息放進和現在一樣的 crossbeam channel，renderer 和 Browser 的程式不知道訊息從哪裡來。單一 process 的模式直接用 crossbeam，不序列化。

### `ipc` crate

新的 crate，對應 Chromium 的 `base/memory/*shared_memory*`、`mojo/core`（和 ipcz）的一部分。分三層：

**平台層**（細節見 `details.md` 的「平台層」）：

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| 建立共用記憶體 | rustix 的 `memfd_create`，加上 seal | rustix 的 `shm::open`，馬上 `shm::unlink` | windows-sys 的 `CreateFileMappingW`（pagefile） |
| map | memmap2 的 `MmapRaw` | memmap2 的 `MmapRaw` | windows-sys 的 `MapViewOfFile` |
| 控制通道 | std 的 `UnixStream::pair` | std 的 `UnixStream::pair` | interprocess 的 named pipe |
| 傳 handle | rustix 的 `sendmsg` 和 `ScmRights` | 同 Linux | Browser 用 `DuplicateHandle` 複製進對方（broker） |

- **三種 region**：`ReadOnlySharedMemoryRegion`、`WritableSharedMemoryRegion`、`UnsafeSharedMemoryRegion`，名字和語意照 Chromium。可寫的 region 能轉成唯讀的一次；唯讀的 handle 在 OS 那一層就不能拿來寫（Linux 的 `F_SEAL_FUTURE_WRITE`、macOS 在 unlink 之前用唯讀再開一次、Windows 複製 handle 時只給 `FILE_MAP_READ`）。大小建立時決定，不能改（Linux 的 `F_SEAL_SHRINK` 和 `F_SEAL_GROW`；Windows 的 section 本來就固定；macOS 見「待決定」）。
- **只有 Browser 建立**：renderer 要共用記憶體時（例如 tile）向 Browser 要，拿到之後自己留著重複用。renderer 不需要 `memfd_create` 或 `shm_open` 的權限，惡意的 renderer 也沒辦法交給 Browser 一塊會被縮小的記憶體（Browser 讀它時會收到 SIGBUS）。Chromium 在 sandbox 不讓 renderer 配共用記憶體的平台上也是由 Browser 代配。
- **map 出來的記憶體不轉成 `&[u8]`**：對方隨時可能在寫，拿著 Rust 的參照就違反了 aliasing 的規則。存取一律經過指標：index 和計數用 atomic（`AtomicUsize::from_ptr`），內容用指標複製到自己的記憶體。

**控制通道**（Chromium 的 `mojo::core::Channel`）：
- 一則訊息是長度、目的地的標記、序列化的內容，加上附帶的 handle。Unix 上 handle 跟著同一次 `sendmsg` 送出；收的一端處理控制資料被截斷的情況，收到的 fd 馬上設 close-on-exec（macOS 沒有 `MSG_CMSG_CLOEXEC`）。
- Windows 上 named pipe 不能帶 handle：Browser 送 handle 給 renderer 時，自己用 `DuplicateHandle` 複製進 renderer 的 process，訊息只帶數值；renderer 送 handle 給 Browser 時，由 Browser 從 renderer 的 process 拉過來，並檢查 handle 的型別（照 Chromium 的 `MojoHandleTypeProtections`），免得 renderer 塞一個 process 或 thread 的 handle 進來。
- 序列化：serde，格式見「待決定」。訊息裡的 region 序列化成 handle 表的 index，handle 本身走帶外；序列化時經過一個 context 對應兩者（ipc-channel 和 Mojo 都這樣做）。
- 收的一端先檢查長度上限、標記和 handle 的數量，再反序列化；反序列化失敗或 handle 不對，就當成 renderer 壞掉，關掉通道（Chromium 的 `ReceivedBadMessage`）。

**ring**（第二步，見「分階段」）：控制通道之外，每個方向一個放在共用記憶體裡的 SPSC ring，平常的訊息直接序列化進去，不經過系統呼叫；控制通道只用來喚醒睡著的一端和傳 handle。照 shmem-ipc 的 `ringbuf.rs` 的設計自己寫（「已決定的事項」第 5 條）：共用記憶體裡只放一個 atomic 的計數，讀寫的位置各自留在自己的 process 裡，所以對方能弄壞的只有計數，每次讀都檢查它不超過容量。machopo 自己加上訊息的框架（長度先檢查再複製出來）、喚醒（「我在睡」的旗標加 SeqCst fence，對方睡著才在控制通道上送一個位元組）、序號（帶 handle 的訊息還是走控制通道，接收端照序號交出去）和滿了時的處理（Browser 送不出去時先放在自己的 queue 裡，不能被 renderer 卡住；renderer 送不出去時等）。細節見 `details.md` 的「ring 的設計」。

### 大的資料

- **`protocol` 加一個 bytes 的型別**（Chromium 的 `mojo_base::BigBuffer`）：單一 process 時是 `Arc<[u8]>`；跨 process 時小於門檻放在訊息裡，大於門檻放進唯讀的 region、只傳 handle。`CommitNavigation` 的 body 和子資源的回應改用它。Servo 的 `GenericSharedMemory` 也是同樣的做法（單一 process 時是 `Arc<Vec<u8>>`）。
- **從 renderer 來的大資料**：Browser 要解析的，從共用記憶體複製出來之後才解析（`BigBuffer` 的註解寫明了這個 TOCTOU 的風險）；只拿來上傳的 tile 像素不用複製，renderer 中途改內容只會讓它自己的畫面錯。

### tile 交接（one-copy raster）

compositor 照舊 raster 成 CPU 像素，寫進 Browser 配給它的 `WritableSharedMemoryRegion`（renderer 留著一個 tile 的 pool 重複用），frame 的 `TransferableResource` 帶 mailbox 和 region 的 handle；Viz 收到時建 texture、上傳，以 mailbox 為 key 記著。生命週期改成明確的訊息：Viz 用完時送 `ReturnResources`，compositor 丟掉 tile 時送 `DestroySharedImage`（P4.4c 時參考計數就是它們在同一個 process 裡的替代，`rfcs/pre-rfcs/P4.md` 第 80 條）。單一 process 的模式照舊用 `Arc`，但走同一組訊息。

### 其他要改成 message 的

- Viz 的 `BeginFrame` 從 closure 改成送進 compositor 的 `CompositorMsg`；context lost 也改成 message。
- compositor 只拿得到自己的 `FrameSinkId` 的 sink：Browser 的 IO thread 轉給 Viz 時用自己記著的 `FrameSinkId`，不信任訊息裡的；renderer 不能送到別人的 surface。
- `RendererFlags` 改成命令列參數，script engine 在 child 的 `main` 裡選（取代函式指標）。

### 啟動

Browser 建好控制通道的兩端，以 `--type=renderer` 和 renderer 的旗標啟動同一個執行檔，把一端交給 child（怎麼交見「待決定」）。child 的 `main` 看到 `--type=renderer` 就先套用 sandbox、不開視窗，從控制通道拿到初始的訊息，然後跑現在的 `spawn_renderer_main_thread`。兩端都由 Browser 建立，沒有用名字連線的步驟：別的 process 搶不到通道，Seatbelt 的 profile 也不用開放連線到某個路徑。

### sandbox（macOS）

照 Chromium 現在的做法，child 的 `main` 在執行任何其他程式碼之前就套用 Seatbelt 的 profile（Browser 把 profile 和參數交給 child，Chromium 的 `SeatbeltExecClient` / `SeatbeltExecServer`），不先「暖機」再鎖（Chromium 舊的做法，見 `details.md` 的「Seatbelt 的兩種套用時機」）。profile 預設拒絕，只開：執行檔和系統字型目錄的讀取、必要的系統呼叫。不開檔案寫入、網路、GPU（IOKit）、啟動其他 process、`shm_open` 和 Mach 的 bootstrap 查詢；IPC 只用繼承來的 socket 和 Browser 交過來的 fd。

### crash 和錯誤頁

控制通道斷了（EOF 或讀寫錯誤）時，IO thread 報告 `RendererGone`，unmap 這個 renderer 的 region。`WebContents` 不再關視窗，改成 commit 一個 Browser 產生的錯誤頁（P2 的 `loader::error_page` 那一套）、丟掉這個 renderer 的 surface；重新載入時啟動新的 renderer。加上 `about:crash`（Chromium 的 `chrome://crash`，`debug_urls.rs`）讓 renderer 自己 panic，給測試和手動確認用。renderer 送來壞掉的訊息時（見「`ipc` crate」的控制通道）也走同一條路。

### 單一 process 的模式

`--single-process`，以及 headless 和測試的預設。renderer 的程式碼不知道自己在哪種模式。訊息直接走 crossbeam，大的資料是 `Arc`。

### 測試

- **message 都能序列化**：每種 message 做一次 serialize / deserialize 的 round trip 測試；`SharedImage` 這類 `#[serde(skip)]` 的欄位拿掉之後才算數。
- **單一 process 的測試照舊**：`DrivenViz`、reftest、`shell` 的端到端測試都不改，證明新的訊息（`ReturnResources`、`DestroySharedImage`、`BeginFrame` 的 message）在同一個 process 裡行為一樣。
- **`ipc` 的平台層**：三個平台各自測建立、map、傳 handle、唯讀和大小的保證（拿唯讀的 handle 試著寫入或 map 成可寫、試著縮小 region，都要失敗）；跨 process 的部分用測試自己啟動的小 child process。CI 每天在 Linux 和 Windows 上跑，macOS 在本機。
- **惡意的對端**：測試扮演壞掉的 renderer，送超過上限的長度、不存在的標記、不對的 handle 數量和型別、反序列化失敗的內容、寫到一半就斷線，Browser 都要關掉通道、報告 `RendererGone`，不能 panic 或卡住。
- **ring**（第二步）：協定（計數、喚醒的旗標和 fence）用 loom 跑；讀取端用 fuzz，另一條 thread 同時亂寫共用記憶體裡的計數和內容，模擬惡意的 renderer；ring 本身也能在同一個 process 裡用兩條 thread 測，加一個測試用的模式讓單一 process 的訊息也走 ring，大部分的 IPC 程式就能跟著 `cargo test` 跑。
- **跨 process 的端到端測試**：啟動真的 child process，載入一個頁面、點一下、捲一下；`about:crash` 之後 Browser 還在、頁面是錯誤頁、重新載入可以恢復。
- **sandbox**：一個只在測試建置裡有的 renderer 訊息，要 renderer 嘗試讀檔、連網路、開 GPU、`shm_open`，檢查都失敗。
- **soundness**：`ipc` 的 `unsafe`（共用記憶體的存取、Windows 的 FFI）照 `crates/dom/soundness.md` 的格式寫論證（「已決定的事項」第 7 條），mutation test 弄壞驗證和 fence 的地方，確認測試抓得到。
- **只能手動確認的**：活動監視器裡的 process、視窗裡的 crash 和恢復，記進 `docs/test-gaps.md` 的「只做過手動確認」。

### 分階段

2026-10-06 的看法，排進某個階段時再確認：
0. **整理邊界**（2026-10-06 排了，延後期間做，第 10 條）：邊界的規則和檢查（`docs/architecture.md` 的「Process 的邊界」、`sanity.py boundary`，第 9 條）；修掉例外 2、4、5：context lost 的 `Weak` observer、`RendererFlags` 的函式指標、送給任何 `FrameSinkId` 的 `Sender<VizMsg>`（例外 3，`BeginFrame` 的 closure，重新看過不算例外，第 12 條）。單一 process，行為不變。
1. **message 化，還是單一 process**：拿掉 `SharedImage` 的 backing（例外 1），加上 `ReturnResources` / `DestroySharedImage`、Viz 的 mailbox 表和 `protocol` 的 bytes 型別。現有的測試全部照舊通過。
2. **`ipc` crate（IPC 的第一步）**：平台層、控制通道、帶 handle 的序列化、Browser 配共用記憶體的介面；用測試自己的 child process 測，包括惡意的對端。還沒接上 renderer。
3. **啟動 child process**：`--type=renderer`、renderer 的 IO thread、所有訊息走控制通道、tile 走共用記憶體；`RendererGone` 之後的錯誤頁、重新載入、`about:crash`。最後量 `about:demo` 捲動時每個 frame 的延遲，和一個高頻小訊息的 benchmark，給第 6 步用。
4. **macOS 的 sandbox**：Seatbelt 的 profile、字型、sandbox 的測試。
5. **V8 的 sandbox**（可以另外排，也可以在第 1 步之前）：`v8_enable_sandbox` 重編 V8，embedder 照它的規則改；這是 process 內的防線，和 OS 的 sandbox 互補。
6. **ring（IPC 的第二步）**：第 3 步量到控制通道的系統呼叫值得省時才做（門檻見「待決定」）。平台層和上層都不變，只換傳輸的核心。

可以先做的原型：只用控制通道（Unix socket 和 Windows named pipe）把 `RendererMsg` 和 `RendererHostMsg` 跨 process 傳，frame 先整張序列化複製，量一次 `about:demo` 捲動時每個 frame 的延遲，看第一步的成本。

## 缺點

- **IPC 要自己維護**：平台層三套、控制通道、帶 handle 的序列化、之後的 ring，加上惡意對端的驗證，都是 machopo 的程式；ipc-channel 這些 Servo 已經做好了。
- **多了 `unsafe`**：共用記憶體的存取和 Windows 的 FFI 免不了，`unsafe` 從 `dom` 和 `bindings` 擴大到 `ipc`（「已決定的事項」第 7 條），也多一份 soundness 論證要維護。
- **安全的保證靠設計和測試**：ring 和控制通道對惡意 renderer 的防護，靠 fuzz、loom 和審查守住，沒有現成被大量使用的實作可以依靠。
- 兩種模式要一起維護：測試大多跑單一 process，跨 process 那條路只有少數端到端測試守著（`ipc` 自己的測試補一部分）。
- 每個 renderer 多一份記憶體（V8 isolate、字型、skia 的快取）、一條 IO thread 和啟動時間；之後多分頁時會更明顯。
- tile 多一次複製（共用記憶體 → GPU），比現在 compositor 直接上傳慢；`rfcs/pre-rfcs/P4.md` 第 80 條才剛把 tile 收成一份，這裡在 Viz 那一側又多一份暫存。compositor 到 Viz 也多一次 IO thread 的轉手。
- sandbox 的 profile 是平台專屬、難測的東西，macOS 改版時可能要跟著改；Linux 和 Windows 要各寫一套。

## 理由和其他做法

完整的比較見 `details.md` 的「調查過的 crate」和「其他實作」。

- **IPC**：
  - **I1. ipc-channel**（2026-10-04 的看法）：serde、能傳 channel 和共用記憶體，Servo 用它拆 process。不選：每則訊息都經過 socket 或 pipe 的系統呼叫，大的訊息在 transport 裡分段或整個複製；`IpcSharedMemory` 在 Linux 上給對方的是可寫的 mapping，沒有唯讀的型別；平台之間的成熟度不同，而且要跟著 Servo 的需求走。
  - **I2. 自己做，照 Chromium 現在的 ipcz 的方向**：選這個（「已決定的事項」第 1 到 5 條）。對 renderer 的不信任（只有 Browser 建立共用記憶體、唯讀的保證、handle 的型別檢查）可以照 Chromium 做到底，訊息也能逐步搬進共用記憶體。
  - **I3. 照 Mojo 做一整套**（message pipe、介面、自動產生的 binding）：最接近 Chromium，但是一個大專案；machopo 的 message 是 `protocol` 的 enum，現在沒有需要介面層的地方，之後要加也能疊在 I2 上面。
  - **I4. iceoryx2（加上 interprocess 當控制通道）**：zero-copy 的共用記憶體 IPC，`unsafe` 都在它裡面。不選：服務用全域的名字找（`/dev/shm`、`/tmp/iceoryx2`），同一個使用者的任何 process 都能打開，和「renderer 只能用 Browser 交給它的 handle」相反，Seatbelt 也得開放那些路徑；它假設參與的 process 互相合作，沒有談惡意的參與者；1.0 以前，macOS 和 Windows 是 Tier 2；兩條通道之間沒有順序。
- **平台層的 crate**：
  - interprocess 在 Unix 上只是包著 std 的 `UnixStream`，它多的是用名字連線，正是要避開的，所以只在 Windows 上用（overlapped 的 named pipe、收送分開、`FILE_FLAG_FIRST_PIPE_INSTANCE`）。
  - memmap2 不能建立能分享的記憶體（Unix 的 `map_anon` 是 `MAP_PRIVATE`），只負責 map；全部改用檔案撐的話，Linux 的 tmpfs 檔案加不上 seal、macOS 的暫存檔在磁碟上，都比較差。它在 Windows 上只吃檔案的 handle，不能 map 我們建的 section，所以 Windows 的 map 也自己做。
  - memfd crate 只支援 Linux 和 Android，rustix 在 Linux 和 macOS 上都有需要的安全 API，所以 Unix 上只用 rustix。
  - 現成的共用記憶體 crate（shared_memory、ipmpsc、shmem-ipc）不是很久沒維護、就是只支援一個平台或只用名字。
- **ring**：ringbuf、rtrb、crossbeam 的 `ArrayQueue` 是給同一個 process 的 thread 用的，信任 index；iceoryx2 的 lock-free queue 能放進共用記憶體，但威脅模型是互相合作的 process。shmem-ipc 的 `ringbuf.rs` 是為不可信的對端設計的，只有一個檔案、不依賴 Linux，所以照它的設計自己寫，不依賴那個 crate（它整體只支援 Linux、2022 年之後沒更新）。
- **tile 怎麼交給 Viz**：
  - **T1. 共用記憶體，Viz 上傳**（one-copy raster）：renderer 不碰 GPU，sandbox 最緊；raster 的位置不變（`rfcs/pre-rfcs/P4.md` 第 1 條）。多一次複製。
  - **T2. 跨 process 的 GPU texture**（macOS 的 `IOSurface`）：renderer 自己上傳，要能存取 GPU，sandbox 得開 IOKit；wgpu 沒有跨 process 共用 texture 的 API，要動到 wgpu 的 Metal 層。
  - **T3. OOP-R**：renderer 送 display list，raster 在 Viz（或之後的 GPU process）那一側。Chromium 現在的做法，但 display list、字型和圖片都要能序列化，raster 也要搬家。
  - 2026-10-04 的看法：T1，T3 留給之後（見「之後的可能」）。
- **process 的分法**：每個 `WebContents` 一個 process。site isolation 需要跨網站導航時換 process（Chromium 的 `RenderFrameHostManager`），而 machopo 現在一個視窗只有一個網頁，iframe 只做同源、同 process 的（RFC 0009），先不做。
- **字型**：
  - **F1. sandbox 開放系統字型目錄的讀取**：Chromium 在 macOS 上的 renderer 也直接讀系統字型。
  - **F2. Browser 提供字型的服務**（Chromium 在 Linux 和 Windows 的 font proxy）：sandbox 更緊，但要做一個 IPC 服務，parley 和 skia 都要改成從它拿字型。
  - 2026-10-04 的看法：macOS 上 F1；做 Linux、Windows 時再看。
- **單一 process 的模式要不要留**：留。Chromium 也有 `--single-process`；machopo 的測試大多靠它（`DrivenViz` 直接扮演 Viz、headless 在同一個 process 裡畫），全部改成跨 process 會又慢又難控制時機。

## 先例

- **Chromium**：Browser、Renderer、GPU、Network 和 utility process；同一個執行檔以 `--type=` 區分；renderer 在各平台的 sandbox 裡（macOS 的 Seatbelt、Linux 的 seccomp-bpf 和 namespace、Windows 的 restricted token 和 job）；renderer 掛掉時顯示 sad tab。IPC 是 Mojo：bindings 之下的 core 現在是 ipcz，每對 process 共用一塊由 ipcz 管的共用記憶體，訊息盡量放在裡面，OS 的通道主要用來喚醒和傳 handle；共用記憶體有唯讀、可寫等型別，大的位元組資料用 `BigBuffer`（64 KB 以上自動改走共用記憶體），串流用 data pipe。machopo 照它的啟動方式、sandbox 的形狀、crash 的處理和 IPC 的方向，不做 GPU process 和 Mojo 的介面層。細節見 `details.md` 的「Chromium 的 IPC」。
- **Chromium 早期**（OOP-R 之前）：renderer 自己 raster 到共用記憶體，GPU process 上傳（one-copy / zero-copy raster），就是 T1。
- **Servo**：`--multiprocess` 時每個 pipeline（大致是每個網站）一個 content process，預設還是單一 process。IPC 是自己維護的 ipc-channel，上面一層 `generic_channel` 在執行時依模式選 ipc-channel 或 crossbeam，`GenericSharedMemory` 同樣選 `IpcSharedMemory` 或 `Arc<Vec<u8>>`；圖片和字型走共用記憶體，display list 序列化成 `Vec<u8>` 走一般的訊息。machopo 照它「單一 process 時不序列化」的做法，不用 ipc-channel。細節見 `details.md` 的「Servo 的 IPC」。
- **shmem-ipc**：Linux 上給不可信的 process 用的共用記憶體 IPC（memfd 的 seal、eventfd、SPSC ring）；machopo 的 ring 照它的設計。

逐項比較見 `details.md` 的「其他實作」。

## 已決定的事項

2026-10-06 確認：

1. **不用 ipc-channel，IPC 自己做**（「理由和其他做法」的 I2）：照 Chromium 現在的方向，表面上是 channel，共用記憶體是一級公民，最後連訊息也盡量放在共用記憶體裡。不選 ipc-channel（I1）和 iceoryx2（I4）的理由見「理由和其他做法」。
2. **平台層的組合**（「`ipc` crate」的表）：Linux 用 rustix 的 memfd 和 seal、macOS 用 rustix 的 POSIX shm（開完馬上 unlink），兩者都用 memmap2 的 `MmapRaw` map、rustix 的 `ScmRights` 傳 fd；Windows 用 windows-sys 的 `CreateFileMappingW`（pagefile）和 `MapViewOfFile`，用 `DuplicateHandle` 傳 handle。不用 memfd crate（只支援 Linux），不用檔案撐共用記憶體。
3. **控制通道**：Linux 和 macOS 用 std 的 `UnixStream::pair`，由 child 繼承一端，不用 interprocess（它在 Unix 上只是包著 `UnixStream`，多的是用名字連線）；Windows 用 interprocess 的 named pipe，兩端都由 Browser 建立。
4. **IPC 分兩步**：第一步所有訊息走控制通道，大的資料放進共用記憶體、只傳 handle（ipcz 以前的 Mojo）；第二步把小訊息搬進共用記憶體裡的 ring，控制通道只用來喚醒和傳 handle（ipcz）。第二步要等第一步量過延遲才決定做不做，兩步用同一個平台層。
5. **ring 照 shmem-ipc 的 `ringbuf.rs` 的設計自己寫**，不依賴那個 crate：共用記憶體裡只放一個計數，讀寫的位置各自留在自己的 process 裡，每次讀計數都檢查範圍。訊息的框架、喚醒、序號和滿了時的處理由 machopo 加上（「`ipc` crate」的 ring）。
6. **process 內的 channel 繼續用 crossbeam；renderer 加一條 IO thread**：`rfcs/pre-rfcs/P4.md` 第 14 條選 crossbeam 的理由之一（ipc-channel 的 router 能轉成 crossbeam 的 receiver）不成立了，但其他理由（`select!`、timer channel、能 clone 的 receiver）還在，效能也本來就不是理由。IPC 收到的訊息由兩端的 IO thread 放進同一種 crossbeam channel，renderer 的 main thread 照舊只等一條 task queue（同一份的第 6 條）；原本「待決定」裡「renderer 這一側怎麼讀 IPC」的兩個選項中，ipc-channel 的 router 隨著第 1 條消失，所以照 Chromium 加 IO thread。
7. **`ipc` 可以寫 `unsafe`**：加進 `python3 scripts/sanity.py unsafe` 的允許清單（crate root 和整合測試，和 `dom`、`bindings` 相同）和 `CLAUDE.md` 的規則。共用記憶體的指標存取和 Windows 的 FFI 免不了 `unsafe`；範圍限制在平台層和 ring 的存取，Linux 和 macOS 的建立和傳 fd 用 rustix 的安全 API。每一處都有 `// SAFETY:`，`ipc` 的論證照 `crates/dom/soundness.md` 的格式另外寫一份，和 crate 一起建立。不選：放進 `bindings`（它是 V8 的 FFI，`ipc` 和 V8 無關，而且 renderer 以外的 process 也要用）、放進 fork（共用記憶體的程式不是哪個上游的修改）。
8. **延後，預設單一 process**（2026-10-06）：renderer 照舊是 Browser process 裡的 thread，這份 RFC 的拆 process、sandbox、IPC 都延後；只做第 0 步，把邊界守住，之後拆開時 renderer 這一側的程式不必改。site isolation（RFC 0009 的第 3 階段）跟著延後，第 3 階段裡不依賴 process 的部分（browser 的 frame tree、子 frame 的導覽和歷史、`WindowProxy`）可以先做。不選「繼續做」：先做 DOM API 和 CSSOM 等功能。
9. **邊界的規則和檢查**（2026-10-06）：規則和例外寫在 `docs/architecture.md` 的「Process 的邊界」；`python3 scripts/sanity.py boundary` 檢查 renderer 這一側的 crate 不依賴 Browser 這一側的、`protocol` 裡沒有不能跨 process 的東西、renderer 這一側沒有整個 process 共用的可變狀態（後兩項各有允許清單，就是例外）；`protocol` 的 `boundary` 測試在編譯時確認跨邊界的 message 都能序列化。和 Chromium 用 `DEPS`、`checkdeps` 守住 `content/renderer` 和 `content/browser` 的邊界同一類。不選「只寫規則，不檢查」。
10. **第 0 步修例外 2 到 5，例外 6 交給分頁，例外 1 留給第 1 步**（2026-10-06）：2 到 5 都小，修了同一個 process 的行為不變；例外 6（raster 的全域快取）只有一個 renderer 時看不出差別，有分頁時才變成跨分頁共用狀態，寫進分頁的 RFC；例外 1（tile 的交接）牽涉共用記憶體的設計，是第 1 步的主要工作。
11. **context lost 由 Browser 轉成訊息**（2026-10-06，第 0b 步）：Browser 的 `RenderProcessHost` 向 Viz 的 `SharedImageInterface` 登記 observer，收到時送 `CompositorMsg::ContextLost` 給 renderer 的 compositor；renderer 不再登記 observer（`sanity.py boundary` 擋住）。和 Chromium 一樣（renderer 經過 GPU channel 收到 context lost，不在 GPU process 裡登記 callback）。不選「Viz 經過 `BeginFrame` 的 client 通知」：要把 client 改成送任何訊息，Viz 改得較多。
12. **`BeginFrame` 的 closure 不算例外**（2026-10-06，第 0b 步）：它由 Browser 的 `RenderProcessHost` 建立，Viz 也在 Browser process 裡，它只送一則 `CompositorMsg::BeginFrame`；拆 process 時 `RenderProcessHost` 讓它寫進控制通道，renderer 這一側不必改。對應 Chromium 裡 Viz 拿著的 `CompositorFrameSinkClient` remote。不改程式，從例外清單拿掉。
13. **`RendererFlags` 只放能序列化的值，engine 另外給**（2026-10-06，第 0b 步）：`RendererFlags` 加上 serde（`dom_api::ScriptLanguage` 也是），選 engine 的函式改成 `spawn_renderer_main_thread` 的另一個參數，由啟動 renderer 的一方給（拆 process 時是 child 的 `main`；headless 的測試照樣給假的 engine）。和 Chromium 一樣（switches 經過命令列，`ContentRendererClient` 由 child 的 `main` 選）。不選「搬進 `protocol`，另外定義 `ScriptLanguage`」：兩個一樣的 enum。
14. **送給 Viz 的 sender 綁著自己的 `FrameSinkId`**（2026-10-06，第 0b 步）：`protocol` 的 `CompositorFrameSinkSender` 由 Browser 建立，`FrameSinkId` 是私有的欄位，只能送 `CompositorFrameSinkMsg` 給自己的 sink；renderer 拿不到 `Sender<VizMsg>`（`sanity.py boundary` 擋住）。拆 process 時換成 Browser 的 IO thread 補上 id。和 Chromium 一樣（`FrameSinkManager` 把 `CompositorFrameSink` 綁在 renderer 的 `FrameSinkId` 上）。

## 待決定

- T1 / T2 / T3、F1 / F2（見「理由和其他做法」）。T 和 RFC 0007 的 GPU raster 的關係見那份的「待決定」。
- **怎麼把控制通道的一端交給 child**：Unix 上 std 的 `Command` 沒有安全的 API 指定要繼承哪個 fd（`pre_exec` 是 `unsafe`）；Windows 上 Chromium 用 handle 繼承的白名單（`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`），std 也沒有穩定的 API。選項：spawn 前短暫拿掉那個 fd 的 close-on-exec（rustix 的安全 API；同時 spawn 的其他 process 也會繼承到）、用 command-fds 這類 crate（`unsafe` 在依賴裡）、Windows 上 spawn 之後用 `DuplicateHandle` 送進去，再經過 child 的 stdin 告訴它數值。第 2 步開始前決定。
- **macOS 的 POSIX shm 能不能被縮小**：我記得 macOS 的 shm 物件只能設定一次大小，之後的 `ftruncate` 會失敗；第 2 步的平台層測試先確認。不成立的話，改用 Mach 的 memory entry（Chromium 在 macOS 上的做法）或只給 renderer 唯讀的 fd。
- **序列化的格式**：看法：postcard（serde、緊湊、持續維護）。第 2 步開始前確認。
- **bytes 型別的門檻**：Chromium 的 `BigBuffer` 是 64 KB；第 3 步量過再定。
- **第二步（ring）的門檻**：第 3 步量 `about:demo` 捲動時每個 frame 的延遲，和高頻小訊息的 benchmark；控制通道的系統呼叫占多少才值得做，量到之後再決定。
- **V8 的 sandbox 和這份 RFC 的關係**：放在第 5 步，或當成獨立的工作先做；它也影響 RFC 0001 的 wasm（`rfcs/0001-wasm-scripts/details.md` 的「安全和資源」）。
- **背景 GC**：`rfcs/pre-rfcs/P1.md` 第 1 節的「防止 V8 自己升級成 concurrent」說過 V8 的 heap 在背景也不再單執行緒 GC 的代價，拆 process、有背景分頁時再權衡。
- **什麼時候排**：和其他還沒排的 RFC 比較；這份不依賴 RFC 0003。

## 之後的可能

- **GPU process**：Viz 搬出 Browser，GPU 驅動程式 crash 時重啟 GPU process，不再整個程式結束（`docs/limitations.md` 的「Viz、GPU 和記憶體」）；`rfcs/pre-rfcs/P4.md` 第 84 條不做 Viz 重啟的理由也指到這裡。到時 renderer 要有直接連到 GPU process 的通道，`CopyOutputRequest` 的 callback 也要改成回應的訊息（RFC 0006）。
- **OOP-R（T3）**：raster 搬到 GPU process，tile 不再經過 CPU；需要 display list、`gfx::FontData`、`gfx::ImageData` 能跨 process（`rfcs/pre-rfcs/P1.md` 的「拆 process 的注意事項」）。GPU raster 本身和它與這份 RFC 的先後見 RFC 0007。
- **data pipe**：串流的資料（子資源的回應、之後的 `fetch` body）用共用記憶體裡的 ring 傳，Chromium 的 `URLLoaderClient` 就是這樣收 body；第二步的 ring 可以直接延伸。
- **Mojo 式的介面**（I3）：訊息多到 enum 難以管理時，在 `ipc` 上面加介面和 `Remote` / `Receiver`。
- **Network Service process**、site isolation（2026-10-07 從 RFC 0009 移過來，它的第 53 條：跨網站的 frame 到別的 process，跨語言的 frame 也當成遠端 frame 一起做）、多個分頁和 renderer 的共用（`rfcs/0002-web-contents-layout/details.md` 的「和其他部分的關係」）。
- **Linux 和 Windows 的 sandbox**。
