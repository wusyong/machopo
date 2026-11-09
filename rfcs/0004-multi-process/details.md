# RFC 0004 的細節：multi-process 和 sandbox

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 現在的邊界

2026-10-04 的程式，2026-10-06 對照過（第 0 步之後的現況和例外改看 `docs/architecture.md` 的「Process 的邊界」；這一節保留當時的盤點）：這段期間 `protocol` 新增的訊息（RFC 0006 和 RFC 0008 的 `QueryElement`、`RequestVisualState`、`TestharnessResults` 等）都是 serde，不改變下面的結論。

### 會跨 process 的 message

| Channel | 方向 | 型別 | 現在能不能跨 process |
| --- | --- | --- | --- |
| renderer main thread 的 task queue | Browser → renderer | `RendererMsg`（`CommitNavigation` 帶 body、`ResourceResponse`、`UpdateVisualProperties`、`Shutdown`） | 能：都是 serde |
| compositor thread 的 queue | Browser、Viz → renderer | `CompositorMsg`（`BeginFrame`、`HandleInputEvent`） | 能；但 `BeginFrame` 是 Viz 的 `BeginFrameSource` 呼叫 `RenderProcessHost::begin_frame_sender` 給的 closure 送進來的 |
| 回 Browser 的 channel | renderer → Browser | `RendererHostMsg` | 能；`RendererGone` 是 IO thread 在 channel 關閉時自己產生的，不是 renderer 送的 |
| `VizMsg::FrameSink` | compositor → Viz | `CompositorFrameSinkMsg`（`SubmitCompositorFrame`、`SetNeedsBeginFrame`、`DidNotProduceFrame`） | 部分：見下面的 `SharedImage` 和 `TransferableResource` |

不跨 process、但現在是直接呼叫的：
- **`SharedImageInterface`**（`crates/protocol/src/viz.rs`）：compositor 同步呼叫 `create_shared_image`，在 Viz 的 wgpu device 上建 texture、寫入像素（Viz 的 `GpuSharedImages`）。`SharedImage` 的 `backing` 是 `Arc<dyn Any>`，標了 `#[serde(skip)]`，序列化之後就不見了；Viz 沒有 mailbox 到 texture 的表，`sync_resources` 直接 downcast backing，沒有就跳過。
- **context lost**：`add_context_lost_observer` 收 `Weak` 的 observer，compositor 的 `ContextLostForwarder` 再 post `DidLoseLayerTreeFrameSink`。
- **`TransferableResource::pixels`**：`Arc<[u8]>`，靠 serde 的 `rc` feature 序列化，跨 process 時每個 frame 複製一次，沒有共用記憶體。software 的模式（headless、reftest）靠它在 compositor 和 Viz 之間共用像素。
- **display list**（`compositor_paint`）帶著 `gfx::FontData`、`gfx::ImageData`，沒有 serde；raster 在 renderer 裡，所以不跨 process。只有 OOP-R 才需要它們能序列化。
- **`CopyOutputRequest`**（RFC 0006）：`DisplayMsg::RequestCopyOfOutput` 帶一個 `#[serde(skip)]` 的 callback。它在 Browser 和 Viz 之間，這份 RFC 不把 Viz 搬出 Browser，所以不用改；做 GPU process 時才要換成回應的訊息。

### 啟動和參數

- `RenderProcessHost::launch` 建 crossbeam 的 channel，呼叫 `spawn_renderer_main_thread` 開 `Compositor` 和 `CrRendererMain` 兩條 thread；`Chrome_IOThread` 收 renderer 的訊息，`FetchResource` 自己交給 `net`，其他的交給 `WebContents` 給的 closure。
- `RendererFlags` 沒有 serde；`script_engine` 是函式指標，`shell` 和 `browser` 的測試傳 `V8Engine` 的建構函式。
- `RendererEndpoints` 帶 `Sender<VizMsg>` 和 `Option<Arc<dyn SharedImageInterface>>`。renderer 拿到的 `Sender<VizMsg>` 可以送給任何 `FrameSinkId`，沒有檢查。

### 檔案、網路、字型、圖片

- 頁面和子資源都由 Browser 的 `NetworkService` thread 載入；`file://` 在 `net` 裡讀，權限由 `RenderProcessHost` 的 `file_access`（commit 了 `file:` 的網址才打開）檢查。renderer 不碰檔案系統和網路。
- **字型在 renderer 裡讀**：`TextContext` 在測試字型模式只用內嵌的 Ahem 和 Lato，否則 fontique 讀系統字型；skia 的 `FontMgr` 和 typeface 快取也在 renderer。
- **圖片在 renderer 裡解碼**：`ImageData` 是編碼過的 bytes，raster 時才由 skia 解碼，快取在 process 全域的表裡。解碼不安全的話是在 sandbox 裡出事，這正是要的。

### crash

- renderer panic：沒有 `catch_unwind`、panic hook 或 `panic = "abort"`；unwind 時 sender 被 drop，IO thread 報告 `RendererGone`，視窗模式關掉整個程式，headless 回錯誤。`WebContents::on_renderer_message` 忽略 `RendererGone`。
- GPU device 遺失：Viz 原地重建 device（P4.4d），全部失敗時 `std::process::exit(1)`；`about:gpucrash` 模擬它。

### 依賴單一 process 的測試

- `browser` 的 `DrivenViz`：測試自己扮演 Viz，直接驅動 `BeginFrameSource`、檢查 frame token 和 resource；P4 的「JS 卡住」驗收靠它。
- headless：同一個 process 裡的 `Display<SoftwareRenderer>`，`gpu: None`，像素經過 `Arc` 共用；reftest、`shell` 的 script、sites、network 測試都用它。
- `FakeGpu`（`crates/compositor/src/fake_gpu.rs`）和 `shell` 的 `browser_view` 測試用假的 `SharedImageInterface`。

這些都直接傳 closure、`Arc` 和 process 內的 channel，所以 `README.md` 選擇保留單一 process 的模式，而不是把它們改成跨 process。

## 要改的地方

| 東西 | 現在 | 改成 |
| --- | --- | --- |
| `BeginFrame` | Viz 呼叫 closure，送進 compositor 的 channel | Viz 送 `CompositorMsg::BeginFrame`，經過控制通道到 renderer |
| context lost | `Weak` observer | Viz 送一則 message 給每個 client |
| `SharedImage` | compositor 在 Viz 的 device 上建 texture，`Arc<dyn Any>` 的 backing | mailbox 加上 `WritableSharedMemoryRegion`；Viz 收到時建 texture、記在 mailbox 表 |
| tile 的生命週期 | 參考計數 | `ReturnResources`（Viz → compositor）和 `DestroySharedImage`（compositor → Viz） |
| `TransferableResource::pixels` | `Arc<[u8]>` | `protocol` 的 bytes 型別：單一 process 時照舊是 `Arc`，跨 process 時是 region |
| `CommitNavigation` 的 body、`ResourceResponse` | `Vec<u8>` 放在訊息裡 | `protocol` 的 bytes 型別，大的走唯讀的 region |
| `RendererFlags` | 函式指標 | 命令列參數；engine 在 child 的 `main` 選 |
| IO thread 交給 `WebContents` 的訊息 | closure | 可以不變（都在 Browser 裡） |
| compositor 送給 Viz | renderer 拿著 `Sender<VizMsg>` 直接送 | 經過控制通道到 Browser 的 IO thread，用 Browser 記著的 `FrameSinkId` 轉給 Viz |
| `RendererGone` | 關掉視窗 | 錯誤頁，重新載入時啟動新的 renderer；壞掉的訊息也走這條 |
| renderer 這一側 | 沒有 IO thread | 新的 IO thread 讀控制通道，放進 main thread 和 compositor 的 task queue |

## 其他實作

| | Chromium | Servo | machopo |
| --- | --- | --- | --- |
| 啟動 | 同一個執行檔，`--type=renderer`；Linux 從 zygote fork | 同一個執行檔，`--content-process` | 同一個執行檔，`--type=renderer` |
| IPC | Mojo；core 是 ipcz，訊息盡量放在每對 process 共用的記憶體裡 | ipc-channel；`generic_channel` 依模式選它或 crossbeam | 自己做：控制通道加共用記憶體，第二步加 ring（「已決定的事項」第 1 到 5 條） |
| 共用記憶體 | 唯讀、可寫、雙方可寫三種 region；`BigBuffer` 64 KB 以上自動改走；data pipe | `IpcSharedMemory`，對方拿到的是可寫的 mapping；`GenericSharedMemory` 在單一 process 時是 `Arc<Vec<u8>>` | 照 Chromium 的三種 region 和 `BigBuffer`；只有 Browser 建立 |
| 誰建立共用記憶體 | sandbox 不允許的平台由 Browser 代配 | 送的一端 | 只有 Browser |
| process 的分法 | site-per-process | 每個 pipeline（大致是每個網站） | 每個 `WebContents` |
| raster | OOP-R（GPU process）；早期 one-copy / zero-copy | WebRender 在主 process | renderer raster、共用記憶體、Viz 上傳（T1，看法） |
| GPU | GPU process | 主 process | Browser process 的 Viz thread |
| macOS sandbox | Seatbelt，child 一開始就套用 | gaol | Seatbelt，child 一開始就套用 |
| Linux sandbox | namespace + seccomp-bpf | gaol | 之後 |
| Windows sandbox | restricted token、job、integrity level | 無 | 之後 |
| 字型 | macOS 直接讀；Linux、Windows 有 font proxy | — | 直接讀（F1，看法） |
| renderer crash | sad tab | — | 錯誤頁 |
| 單一 process 的模式 | `--single-process`（只給除錯） | 預設 | `--single-process`、headless 和測試的預設 |

Servo 的 IPC 和共用記憶體兩列 2026-10-06 對照過原始碼；其他 Servo 的欄位是 2026-10-04 時的了解，沒有逐一查證；「—」表示沒查。

## Chromium 的 IPC

2026-10-06 對照 Chromium 的原始碼和文件。

- **三層**：
  - bindings：`.mojom` 產生 `Remote` / `Receiver`；同一條 pipe 上的訊息有順序，要和別的介面保持相對順序時用 associated interface 共用一條 pipe。
  - core：message pipe、data pipe、shared buffer、platform handle 四種 primitive。`mojo/core/embedder/features.cc` 裡已經沒有切換 ipcz 的 feature flag，core 只剩 ipcz 一種實作；`mojo/core` 底下的 `channel_posix.cc`、`channel_mac.cc`、`channel_win.cc` 還在，是 ipcz 的 driver 用的 transport。
  - ipcz：node（一個 process）、portal、router。每對相連的 node 共用一塊由 ipcz 管的共用記憶體（`NodeLinkMemory`，裡面有 buffer pool 和 fragment），parcel 和 router 之間的狀態盡量放在裡面；README 說它優先用「純 userspace 的記憶體交換」，系統 I/O 留給通知和例外情況。ipcz 依賴 Chromium 的 `//base`，沒有穩定的 C ABI，不能單獨拿來用。
- **OS 的 transport**：Linux 是 Unix socket，macOS 是 Mach port，Windows 是 named pipe。舊的 Chrome IPC（`IPC::Channel`）現在也疊在 Mojo 上。
- **共用記憶體的型別**（`base`）：
  - `ReadOnlySharedMemoryRegion`：平台那一層保證接收端只能唯讀 map。
  - `WritableSharedMemoryRegion`：可以寫，能轉成唯讀一次。
  - `UnsafeSharedMemoryRegion`：雙方都能寫，自己同步。
- **大的資料**：
  - `mojo_base::BigBuffer`：`kMaxInlineBytes` 是 64 KB，以下放在訊息裡，以上複製進共用記憶體只傳 handle。註解警告共用記憶體在接收端讀的時候，送的一端可能還在寫，從不可信的來源收到時有 TOCTOU 的風險。
  - data pipe：共用記憶體裡的 ring，用在串流，例如 `URLLoaderClient` 收網路回應的 body。
  - GPU 的 command buffer：命令寫進共用記憶體的 ring，大的資料放在 transfer buffer，Mojo 訊息只送 flush 和位置；OOP-R 時 renderer 把 `PaintOp` 序列化進 transfer buffer。
  - `CompositorFrame` 是序列化的 Mojo 訊息，資源只帶 Mailbox、SharedImage 和 sync token；software compositing 時 bitmap 是 renderer 的共用記憶體。
  - 解碼後的圖片放在 discardable 的共用記憶體，由 Browser 統一管理。
- **broker**：sandbox 不讓 renderer 配共用記憶體、或不能把 handle 複製進別的 process 的平台，由 Browser 代做（`InstallMojoIpczBaseSharedMemoryHooks` 的註解）。Windows 上 `kMojoHandleTypeProtections` 預設開啟，檢查傳過來的 handle 的型別。

## Servo 的 IPC

2026-10-06 對照 servo/servo 和 servo/ipc-channel 的原始碼。

- **ipc-channel**：serde 的訊息，走各平台原生的 primitive。
  - Linux 和其他 Unix：Unix socket，`SCM_RIGHTS` 傳 fd；超過一個 fragment 的訊息另開一條專用的 channel 分段送。
  - macOS：Mach port；超過 `MAX_INLINE_SIZE`（送出時遇到 `MACH_SEND_TOO_LARGE` 就調降）改成 out-of-line 的記憶體描述子，用 `vm_allocate` 的頁面和 `MACH_MSG_VIRTUAL_COPY`。
  - Windows：named pipe；超過 64 KB 的訊息另開一條 pipe 送大的資料，handle 用 `DuplicateHandle` 複製進對方。
  - Android、iOS、wasi 和 `force-inprocess` 的 feature 用 process 內的 backend。
- **`IpcSharedMemory`**：可以放進訊息的共用記憶體。Linux 用 `memfd_create` 加 `mmap(MAP_SHARED, PROT_READ | PROT_WRITE)`，接收端拿到的也是可寫的 mapping；Windows 用 `CreateFileMapping` 加 `DuplicateHandle`。`deref_mut` 是 `unsafe`，要求只有一個讀寫者。
- **`generic_channel`**（`components/shared/base/generic_channel/`）：`GenericSender` / `GenericReceiver` 依 `multiprocess` 或 `force_ipc` 的設定，在執行時選 ipc-channel 或 crossbeam；`GenericSharedMemory` 同樣選 `IpcSharedMemory` 或 `Arc<Vec<u8>>`，單一 process 時把 `Arc` 的指標當成「序列化的結果」傳，不複製。
- **用共用記憶體的地方**：WebRender 的圖片（`SerializableImageData::Raw`）、字型（`AddFont` 帶 `Arc<GenericSharedMemory>`）、canvas、`ImageData`、WebGL 和 WebGPU 的 buffer、`fetch` 的 body。display list 不用：序列化成 `items_data` 和 `spatial_tree` 兩個 `Vec<u8>`，走一般的訊息。

## 調查過的 crate

2026-10-06 的版本和狀態。

| crate | 版本（發布日） | 做什麼 | 用不用 |
| --- | --- | --- | --- |
| ipc-channel | — | Servo 的 IPC（見「Servo 的 IPC」） | 不用（「已決定的事項」第 1 條） |
| interprocess | 2.4.4（2026-09-03） | Unix 的 local socket、Windows 的 named pipe、unnamed pipe；沒有共用記憶體，沒有訊息的框架，公開的 API 裡沒有傳 fd 或 handle | 只在 Windows 上用 |
| iceoryx2 | 0.10.0（2026-09-18） | 共用記憶體的 pub/sub、request/response、event、blackboard | 不用（`README.md` 的 I4） |
| shmem-ipc | 0.3.0（2022-11-22） | 給不可信的 process 用的共用記憶體 IPC，只支援 Linux | 不依賴，照它的 `ringbuf.rs` 的設計（見「ring 的設計」） |
| shared_memory | 0.12.4（2022-03-01） | Linux 和 Windows 的共用記憶體，用名字打開 | 不用 |
| ipmpsc | 0.5.1（2021-05-24） | 共用記憶體的 MPSC | 不用 |
| memfd | 0.6.6（2026-09-01） | `memfd_create` 和 seal 的安全包裝，只支援 Linux 和 Android | 不用，rustix 有同樣的 API |
| memmap2 | 0.9.11（2026-06-22） | map 檔案或 fd | Linux 和 macOS 的 map |
| rustix | — | `memfd_create`、`fcntl_add_seals`、`shm::open`、`shm::unlink`、`sendmsg` 和 `SendAncillaryMessage::ScmRights` 都是安全的函式 | Linux 和 macOS 的建立和傳 fd |
| ringbuf | 0.5.2（2026-09-13） | process 內的 SPSC ring | 不用 |

- **interprocess**：Unix 上的 stream 是 `pub struct Stream(pub(super) UnixStream)`，能從 `UnixStream` 或 `OwnedFd` 轉過來，沒有多的功能。Windows 上 named pipe 用 `FILE_FLAG_OVERLAPPED` 開，能拆成收和送兩半在不同 thread 上同時用；listener 支援 `FILE_FLAG_FIRST_PIPE_INSTANCE`；能從 `OwnedHandle` 建立 stream；關閉前由一個叫 limbo 的 thread pool 先 flush，對方才不會在收完之前看到斷線。
- **iceoryx2**：
  - 服務的狀態放在 `/dev/shm` 和 `/tmp/iceoryx2`（Windows 是 `c:\Temp\iceoryx2`），用名字找；FAQ 只說資源限於同一個使用者存取，沒有談惡意的參與者。
  - issue #906（2026-04-14 由 PR #1550 合併）讓 subscriber 唯讀地 map，動機寫的是防止「意外」寫入。
  - payload 要是 `#[repr(C)]` 的 `ZeroCopySend`，不能放 `Vec`、`String`；publisher 預先配置最壞情況的記憶體；FAQ 提到動態擴充大小時，送的一端關掉可能遺失訊息。
  - 平台：Linux、macOS、Windows、FreeBSD 都是 Tier 2（功能有限制），Android 是概念驗證。
  - 2026-10-06 還開著的 issue：#1951（macOS、Windows、FreeBSD 上狀態目錄是編譯期常數，不能依使用者隔離）、#1880（dead node 清理時 `fatal_panic` 會殺掉不相關的 process）、#2034（macOS 上 `shm_open` 回報 ENFILE）、#2035（0.10.0 的 `Publisher::loan` 每個 sample 多配置一次 heap，0.9.1 沒有）。
- **memmap2**：Unix 上檔案的 map 用 `MAP_SHARED`，`map_anon` 用 `MAP_PRIVATE | MAP_ANON`，不能分享；Windows 上 `map` 對傳進來的檔案 handle 呼叫 `CreateFileMappingW(handle, …, 0, 0)`，大小取自檔案，不能拿 section 的 handle 來 map；`map_anon` 建了 pagefile 的 section 但馬上關掉 handle。檔案的 `map`、`map_mut` 是 `unsafe`（檔案被別人改時是 UB），`map_raw` 是安全的，回傳的 `MmapRaw` 只給指標。

## 平台層

### Linux

- 建立：rustix 的 `memfd_create`，帶 `MFD_ALLOW_SEALING` 和 `MFD_CLOEXEC`；設好大小之後加 `F_SEAL_SHRINK`、`F_SEAL_GROW`（之後也加 `F_SEAL_SEAL`）。
- 唯讀：`F_SEAL_FUTURE_WRITE`（Linux 5.1 起）讓已經存在的可寫 mapping 照舊能寫，但之後誰都不能再 map 成可寫，所以 Browser 先 map 好自己的，再交出 fd。
- 只有 memfd 建的檔案能加 seal：一般 tmpfs 的檔案一開始就帶著 `F_SEAL_SEAL`，加不上任何 seal，所以不能改用 `/dev/shm` 底下的暫存檔。
- 傳 fd：rustix 的 `sendmsg` 和 `SendAncillaryMessage::ScmRights`；收的一端用 `MSG_CMSG_CLOEXEC`，檢查控制資料有沒有被截斷（`MSG_CTRUNC`）。

### macOS

- 建立：rustix 的 `shm::open`（`O_CREAT | O_EXCL | O_RDWR`，名字用亂數），`ftruncate` 設大小，然後馬上 `shm::unlink`；名字只存在建立的那一下，之後只靠 fd。
- 唯讀：unlink 之前用 `O_RDONLY` 再開一次，把唯讀的 fd 交給對方；拿唯讀的 fd 不能 map 成可寫。
- 大小：我記得 macOS 的 POSIX shm 物件只能 `ftruncate` 一次，之後再設會失敗，所以拿到可寫 fd 的 renderer 也縮小不了；還沒驗證（`README.md` 的「待決定」）。不成立的話改用 Mach 的 memory entry（`mach_make_memory_entry_64`，Chromium 在 macOS 上的做法），或只給 renderer 唯讀的 fd。
- 沒有 tmpfs：用檔案撐的話在 APFS 的磁碟上，髒頁會被寫回。
- 傳 fd：和 Linux 一樣用 `SCM_RIGHTS`；沒有 `MSG_CMSG_CLOEXEC`，收到之後馬上用 rustix 設 close-on-exec。
- sandbox：renderer 不需要 `shm_open` 的權限，因為 region 都由 Browser 建立。

### Windows

- 建立：`CreateFileMappingW(INVALID_HANDLE_VALUE, …, PAGE_READWRITE, …)`，用 pagefile 撐，大小固定；map 用 `MapViewOfFile`，unmap 用 `UnmapViewOfFile`。這三個都是 windows-sys 的 FFI。
- 唯讀：`DuplicateHandle` 時只給 `FILE_MAP_READ`。
- 傳 handle：named pipe 不能帶 handle。Browser 是 broker：送給 renderer 時用 `DuplicateHandle` 複製進 renderer 的 process，訊息只帶數值；從 renderer 拿時，由 Browser 從 renderer 的 process 拉過來（`DUPLICATE_CLOSE_SOURCE`），並檢查型別是 section。renderer 在 sandbox 裡沒有 Browser 的 process handle，自己複製不了。
- 曾考慮過改用 memmap2 加暫存檔（`FILE_ATTRIBUTE_TEMPORARY` 和 `FILE_FLAG_DELETE_ON_CLOSE`）以減少 `unsafe`：記憶體吃緊時髒頁還是可能寫回磁碟，防毒軟體會掃描新建的檔案，`DuplicateHandle` 也還是要 `unsafe`，所以維持 pagefile。

### 控制通道

- Unix：std 的 `UnixStream::pair`，一端給 child 繼承；送和收用 `try_clone` 出來的兩個 `UnixStream`。
- Windows：interprocess 的 named pipe。Browser 用亂數的名字和 `FILE_FLAG_FIRST_PIPE_INSTANCE` 建 server 端，自己再開 client 端，把 client 端交給 child；child 從 `OwnedHandle` 建 stream，不靠名字連線。
- 交給 child 的方式還沒決定（`README.md` 的「待決定」）。

## ring 的設計

### shmem-ipc 的 `ringbuf.rs`

2026-10-06 讀的版本：348 行，MIT 或 Apache-2.0，只用 `AtomicUsize` 和指標，不依賴 Linux（整個 crate 只支援 Linux，是因為它的其他部分用 memfd 和 eventfd）。

- 共用記憶體的開頭一個 cache line 放一個 atomic 的計數（ring 裡有幾個項目），後面是資料。
- 送和收的位置（`index`）各自存在 `Sender` 和 `Receiver` 裡，也就是自己 process 的記憶體，對方碰不到。
- 每次讀計數（`load_count`）都檢查它不超過容量，超過就回報 `BufCorrupt`。所以對方能做的最多是把計數改成範圍內的錯誤值：讓我們讀到亂碼，或覆蓋還沒讀的資料，不會讓我們讀寫到 ring 以外的地方。
- `send` 和 `recv` 給 callback 原始指標和可用的數量，不給 `&[T]`；callback 回傳用了幾個，超過可用的數量就回報錯誤。
- 送完用 `fetch_add`、收完用 `fetch_sub` 更新計數，回傳的 `Status` 帶一個 `signal` 旗標：計數從 0 變成非 0（或從滿變成不滿）時，表示要喚醒對方。
- 惡意的對端把計數改小時，`fetch_sub` 可能下溢；下一次 `load_count` 會發現超過容量，回報損毀，不會越界。

### machopo 加上的部分

- **訊息的框架**：ring 的項目是位元組；一則訊息是長度、目的地的標記、序號和內容。讀的時候先檢查長度不超過上限、也不超過目前可讀的量，複製到自己的記憶體之後才反序列化。
- **喚醒**：共用記憶體裡加一個「我在睡」的旗標。接收端要睡之前設旗標、SeqCst fence、再檢查一次計數，還是空的才在控制通道上等；送的一端寫完、更新計數、fence 之後讀旗標，對方在睡才在控制通道上送一個位元組。shmem-ipc 用 eventfd，machopo 用控制通道，IO thread 只要等一個東西。
- **序號**：帶 handle 的訊息走控制通道，其他走 ring；每則訊息帶序號，接收端照序號交出去。
- **滿了**：Browser 往 renderer 送時 ring 滿了，先放在 Browser 自己的 queue 裡，等 renderer 收走一些再送，Browser 不能被 renderer 卡住；renderer 往 Browser 送時滿了就等。
- **atomic 的抽象**：計數和旗標的 atomic 型別抽象出來，測試時換成 loom 的版本。
- **只有 Browser 建立 ring 的記憶體**，和其他 region 一樣。

## Seatbelt 的兩種套用時機

- **暖機之後再鎖**（Chromium 的 V1 sandbox：先 warmup 再鎖）：child 先正常啟動、載入會用到的系統函式庫和資源，再呼叫 `sandbox_init` 鎖住。profile 可以比較緊，但鎖之前跑過的程式碼不在 sandbox 裡，哪些東西要先暖機也難維護。
- **一開始就鎖**（Chromium 現在的 V2 sandbox）：Browser 把 profile 和參數（例如使用者目錄、執行檔的路徑）經過 pipe 交給 child，child 的 `main` 在執行其他程式碼之前就套用。profile 要允許之後會用到的東西（例如字型），但所有 renderer 的程式碼都在 sandbox 裡。

`README.md` 照第二種。machopo 現在沒有 ICU data（`docs/limitations.md` 的「平台和建置」），字型是唯一要讀的檔案；之後加上 ICU data 時，profile 也要開放它。IPC 只用繼承來的 socket 和 Browser 交過來的 fd，profile 不用開放 `shm_open` 和 Mach 的 bootstrap 查詢。

## 參考資料

- Chromium 的 multi-process 架構：https://www.chromium.org/developers/design-documents/multi-process-architecture/
- Chromium 的 macOS sandbox：https://chromium.googlesource.com/chromium/src/+/main/sandbox/mac/README.md
- Chromium 的 sandbox 總覽：https://chromium.googlesource.com/chromium/src/+/main/docs/design/sandbox.md
- ipcz：https://chromium.googlesource.com/chromium/src/+/main/third_party/ipcz/README.md
- ipcz 的 `NodeLinkMemory`：https://chromium.googlesource.com/chromium/src/+/main/third_party/ipcz/src/ipcz/node_link_memory.h
- Mojo 的 feature：https://chromium.googlesource.com/chromium/src/+/main/mojo/core/embedder/features.cc
- `BigBuffer`：https://chromium.googlesource.com/chromium/src/+/main/mojo/public/cpp/base/big_buffer.h
- ipc-channel：https://github.com/servo/ipc-channel
- Servo 的 `generic_channel`：https://github.com/servo/servo/tree/main/components/shared/base/generic_channel
- interprocess：https://docs.rs/interprocess/latest/interprocess/
- iceoryx2：https://github.com/eclipse-iceoryx/iceoryx2 （FAQ：https://github.com/eclipse-iceoryx/iceoryx2/blob/main/FAQ.md）
- shmem-ipc：https://github.com/diwic/shmem-ipc
- memmap2：https://github.com/RazrFalcon/memmap2-rs
- rustix：https://docs.rs/rustix/latest/rustix/
