# RFC 0004 的細節：multi-process

這份是 `README.md` 的補充，放支撐它的細節：其他實作怎麼做、比過的做法的完整理由、平台層、實測。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

其他實作的部分分兩次查：2026-10-06 和 2026-11-17。各節寫明對照的是原始碼、文件，還是只看了搜尋到的資料；沒有查證的寫「就我們的了解」。

## 現在的邊界

現況以 `docs/architecture.md` 的「Process 的邊界」為準：規則、`sanity.py boundary` 檢查什麼、剩下的例外和不算例外的東西都寫在那裡，這裡不重複。

**2026-11-20 的盤點**（`main` 的 `dcd1b61`；2026-10-04 那一次之後，多了分頁、worker、Browser 擁有的 frame 樹、串流的載入、profile 和同步等 Browser 的呼叫）。renderer 和 Browser、Viz 之間的每一條路：

| 路 | 現在 | 能不能直接過 process |
| --- | --- | --- |
| Browser → renderer 的訊息（`RendererMsg`，29 種） | Browser 的 thread 呼叫 `RendererSender::send`，它照訊息分到 main thread 的 queue、worker 的 thread、DevTools 的 queue，或同步呼叫等回答的位置 | 可以：分派已經在 renderer 這一側的 `RendererSender` 裡，renderer 的 IO thread 讀出訊息之後呼叫它 |
| renderer → Browser 的訊息（`RendererHostMsg`，38 種） | crossbeam 到每個 renderer 一條的 `Chrome_IOThread` | 可以。但 Browser 對它們做的事（核對請求和 cookie 的 loader、子資源交給 `net`、登記 worker 和 blob URL、壞訊息、其餘轉給 UI thread）都寫在 `RenderProcessHost::launch` 裡那條 thread 的迴圈（約 360 行），要先抽成物件 |
| 同步的呼叫（`OpenStorageArea`、`GetCookies`、`CookiesEnabled`） | 帶編號的一般訊息，回答放進 scheduler 裡等回答的位置，main thread 只等它 | 可以：設計時就照 process 的邊界做，回答由讀通道的 thread 放進同一個位置 |
| compositor ↔ Viz 的訊息（`CompositorMsg` 3 種、`CompositorFrameSinkMsg` 3 種） | 綁著 `FrameSinkId` 的 sender；`BeginFrame` 是 Browser 建的 closure | 可以（經過 Browser 的 IO thread 轉手） |
| tile 和 canvas 的快照上 GPU（例外 1） | compositor 同步呼叫 Viz 的 `SharedImageInterface::create_shared_image`（兩處：tile、canvas 的快照），`SharedImage` 的 backing 是 `Arc<dyn Any>`，生命週期靠參考計數；像素是 renderer 用 vello_cpu 畫的 CPU 記憶體，renderer 這一側沒有 wgpu | 不行 |
| 導覽的 body（例外 7） | `CommitNavigation` 帶開頭的 1024 bytes 和其餘部分的 `ipc::data_pipe::Consumer`（`#[serde(skip)]`）；管道是同一個 process 裡一把鎖守著的佇列 | 不行 |
| 子資源的回應、worker 的主 script | 收齊之後整份放在訊息裡（`LoadResult` 的 `Bytes`） | 可以，但大的每次整份複製進通道 |
| blob 的內容 | `RegisterBlobUrl` 帶 `Arc<[u8]>`，renderer 送給 Browser | 可以，同上 |
| 沒有 GPU 時的像素 | `TransferableResource::pixels` 的 `Arc<[u8]>` 跟著 frame | 可以，同上 |
| 啟動 | `RendererFlags`（能序列化）、`RendererEndpoints`（channel 的一端和上面的 `SharedImageInterface`）、`ScriptEngineFactory`（函式） | flags 可以；其他兩樣由 child 的 `main` 和控制通道取代 |
| 字型 | renderer 自己經過 fontique 讀系統的字型檔（`--test-fonts` 時是內建的） | 可以：child 自己讀。之後的 sandbox 要開放讀字型目錄，或改由 Browser 給 |
| trace（`MACHOPO_TRACE`） | 整個 process 一份，每條 thread 的 span 寫進同一個檔案 | 不行：child 的 span 要另外收（每個 process 各寫一個檔案，或送回 Browser） |

**第 1 階段之後**（2026-11-21）：上表的「tile 和 canvas 的快照上 GPU」改成訊息（`CompositorFrameSinkMsg` 多了 `CreateSharedImage`、`DestroySharedImage`，`CompositorMsg` 多了 `SharedImagePixelsReturned`；像素還是同一個 process 裡共用的一塊，第 3 階段換成共用記憶體），例外 1 消掉；子資源的回應、blob 的內容、沒有 GPU 時的像素、request 的 body 和 history 的 state 都是 `protocol::BigBuffer`；`RendererEndpoints` 只剩 channel 的一端和「有沒有 GPU」；Browser 處理 renderer 的訊息的迴圈抽成 `IoThreadHost`。

**不跨邊界、但每個 process 會各有一份的**：V8 的 platform 和它的 thread、elise 的 worker pool、renderer 裡的 thread（compositor、raster worker、dedicated worker、canvas 的 raster 和 encoder）、名稱表和屬性值的表、字型的 collection 和 raster 的快取。Browser 那一側不變的：`net`（cookie、blob URL 的表）、`DomStorageContext`、profile 的資料夾（child 不開它）、DevTools 和自動化的 server。

**依賴單一 process 的測試**：`browser` 的 `DrivenViz`（測試自己扮演 Viz）、headless、`FakeGpu`，和 `browser`、`shell` 在 process 裡建分頁的測試。它們直接傳 closure、`Arc` 和 process 內的 channel，所以保留單一 process 的模式，不把它們改成跨 process。

## 要改的地方

| 東西 | 現在 | 改成 |
| --- | --- | --- |
| tile 和 canvas 快照的 `SharedImage` | compositor 同步呼叫 Viz 建 texture，backing 是 `Arc<dyn Any>` | mailbox 加上 region 的編號；Viz 收到時建 texture、記在 mailbox 表（T1） |
| tile 的生命週期 | 參考計數 | `ReturnResources`（Viz → compositor）和 `DestroySharedImage`（compositor → Viz） |
| 導覽的 body 的管道 | 同一個 process 裡的佇列，`Consumer` 放在 `CommitNavigation` 裡 | 一段一段走控制通道，renderer 回報讀了多少（`README.md` 的「導覽的 body 的管道」）；renderer 的 IO thread 寫進它自己 process 裡的管道 |
| 大的位元組資料（子資源的回應、worker 的 script、blob 的內容、沒有 GPU 時的像素） | `Bytes`、`Arc<[u8]>` 放在訊息裡 | `protocol` 的 bytes 型別：單一 process 時照舊，跨 process 時大的放進共用記憶體 |
| Browser 處理一個 renderer 的訊息 | `RenderProcessHost::launch` 裡 IO thread 的迴圈 | 一個只有「收到這則訊息」一個入口的物件，兩種模式共用 |
| `BeginFrame` | Viz 呼叫 Browser 給的 closure，送進 compositor 的 channel | `RenderProcessHost` 讓 closure 寫進控制通道；renderer 不必改 |
| compositor 送給 Viz | 綁著 `FrameSinkId` 的 `CompositorFrameSinkSender` | 經過控制通道到 Browser 的 IO thread，由它補上 `FrameSinkId` 轉給 Viz |
| `RendererFlags` | 啟動 thread 時的參數 | 命令列 |
| script engine | 啟動的一方給的 `ScriptEngineFactory` | child 的 `main` 選 |
| `RendererGone` | renderer 的 thread 結束、channel 關閉 | 控制通道斷了，或 renderer 送了壞訊息 |
| 結束一個 renderer（`kill`、壞訊息） | 送 `Shutdown`、不再轉它的訊息；thread 自己結束 | 結束那個 process |
| renderer 這一側 | 沒有 IO thread | 新的 IO thread 讀控制通道，呼叫 `RendererSender::send`（main thread、worker、DevTools、等回答的位置）和 compositor 的 queue |
| trace | 一個 process、一個檔案 | 還沒決定（「待決定」） |

## 其他實作

| | Chromium | Servo | Ladybird | machopo |
| --- | --- | --- | --- | --- |
| 啟動 | 同一個執行檔，`--type=renderer`；Linux 從 zygote fork | 同一個執行檔，`--content-process` | — | 同一個執行檔，`--type=renderer` |
| IPC | Mojo；core 是 ipcz | ipc-channel；`generic_channel` 依模式選它或 crossbeam | 自己的 LibIPC，訊息用 IDL 產生 | 自己做：控制通道加共用記憶體，第二步加 ring |
| Linux 的通道 | non-blocking 的 Unix socketpair，`SCM_RIGHTS` | Unix socket，`SCM_RIGHTS` | Unix socket，`SCM_RIGHTS` | 同 Chromium |
| macOS 的通道 | Mach port | Mach port（ipc-channel 的第一個 backend） | Mach port（2026-03 從 socket 換過來） | Mach port |
| Windows 的通道 | overlapped 的 named pipe | named pipe | socket | overlapped 的 named pipe |
| macOS 上 child 怎麼連上 | rendezvous：一個 server，名字由 parent 的 pid 決定，靠 audit token 的 pid 發 port | 每個 child 一個帶亂數的名字，放在命令列，不驗證對方 | 查 bootstrap server 上的 port，server 照 pid 發 port | 照 Chromium |
| 共用記憶體 | 唯讀、可寫、雙方可寫三種 region（這份只做唯讀和雙方可寫兩種）；`BigBuffer` 64 KB 以上自動改走；data pipe | `IpcSharedMemory`，對方拿到的是可寫的；`GenericSharedMemory` 在單一 process 時是 `Arc<Vec<u8>>` | — | 照 Chromium 的三種 region 和 `BigBuffer` |
| 誰建立共用記憶體 | sandbox 不允許的平台由 Browser 代配，其他的 renderer 自己建（後半是就我們的了解） | 送的一端 | — | 只有 Browser |
| 誰能附 handle | 任何一端，收的一端驗證 | 任何一端 | 任何一端 | 每一端各自的權限；renderer 沒有，所以只從 Browser 往 renderer |
| process 的分法 | site-per-process | 每個 pipeline（大致是每個網站） | — | 每個分頁 |
| raster | OOP-R（GPU process）；早期 one-copy / zero-copy | WebRender 在主 process | — | renderer raster、共用記憶體、Viz 上傳（T1，看法） |
| GPU | GPU process | 主 process | — | Browser process 的 Viz thread |
| macOS sandbox | Seatbelt，child 一開始就套用 | gaol，應該是先連線再套用 | — | 之後另一份 RFC；這份的設計假設一開始就套用 |
| Linux sandbox | namespace + seccomp-bpf | gaol | — | 之後另一份 RFC |
| Windows sandbox | restricted token、job、integrity level | 無 | — | 之後另一份 RFC |
| 字型 | macOS 直接讀；Linux、Windows 有 font proxy | — | — | 現在直接讀；有 sandbox 之後怎麼給，那份 RFC 決定 |
| renderer 當掉 | sad tab | — | — | sad tab |
| 單一 process 的模式 | `--single-process`（只給除錯），訊息照樣經過 Mojo | 預設，不序列化 | — | `--single-process`、headless 和測試的預設，不序列化 |

「—」表示沒查。Servo 的 sandbox 和啟動以外的非 IPC 欄位是 2026-10-04 時的了解，沒有逐一查證。

## Chromium 的 process

2026-11-18 和 2026-11-20 對照 Chromium 的 `process_type.h`、`sandbox.mojom` 和 Rule of 2 的文件（連結在「參考資料」）。

- **五種型別**：Browser（一個，不關進 sandbox）、Renderer（很多，照網站分）、GPU（一個，和顯示卡的驅動程式打交道，Viz 和 raster 也在裡面）、Utility（每個服務一個）、Zygote（只有 Linux 和 ChromeOS，renderer 從它 fork）。列舉裡的 plugin、worker、PPAPI 都標了 deprecated；worker 跑在 renderer 裡。
- **Utility 裝的服務**：sandbox 的定義列了 32 種。主要的：Network、Audio、列印（compositor、backend）、CDM、硬體影片的解碼和編碼、語音辨識、裝置上的模型、Proxy 的解析，和只做運算、幾乎不給任何權限的一般服務。有些只在特定平台有。
- **Rule of 2**：一段程式最多具備三項裡的兩項（原文「Pick no more than 2 of」）：不可信的輸入、不安全的實作語言、高的權限。
  - 不安全的語言至少包括 C、C++ 和組合語言。Rust 列在安全的語言裡，但 `unsafe` 算是另一個不安全的子集，要小、包在安全的 API 後面、寫明理由。
  - 高的權限包括以使用者的身分跑的 process，文件點名 Browser 和 GPU process；Network process 的權限略低但還是高。低的是關在 sandbox 裡的 utility 和 renderer。這一項和下一項是從頁面的摘要讀到的，沒有逐字對照。
  - 三項都有時的做法：換成安全的語言；把解析搬進 sandbox 裡的 utility process，只把整理好的結果送回來；或確認輸入可信。
- **renderer 怎麼連到 Network process**（就我們的了解，沒有對照原始碼）：Network process 只有一個，由 Browser 啟動。Browser 為每個 renderer（每個 frame、每個 worker）建一條 Mojo 的 pipe，一端連同「它代表哪個網站」的限制交給 Network process，另一端交給 renderer；之後 renderer 直接對 Network process 送子資源的請求，不經過 Browser。文件本身的載入一律由 Browser 發起。把網路搬出去之前，資源的載入是在 Browser 的 IO thread 上處理的，machopo 現在的 `Chrome_IOThread` 像那時的樣子。
- **IO thread**：每個 process 一條，處理那個 process 所有的通道（就我們的了解）。machopo 是 Browser 裡每個 renderer 一條。

## Chromium 的 IPC

對照 Chromium 的原始碼和文件（2026-10-06 和 2026-11-17）。

### 分層

- **bindings**：`.mojom` 產生 `Remote` / `Receiver`；同一條 pipe 上的訊息有順序，要和別的介面保持相對順序時用 associated interface 共用一條 pipe。
- **core**：message pipe、data pipe、shared buffer、platform handle 四種 primitive。core 只剩 ipcz 一種實作；`mojo/core` 底下各平台的 channel 是 ipcz 的 driver 用的 transport。
- **ipcz**：每對相連的 process 共用一塊由 ipcz 管的共用記憶體（`NodeLinkMemory`），訊息和 router 之間的狀態盡量放在裡面；README 說它優先用「純 userspace 的記憶體交換」，系統 I/O 留給通知和例外情況。README 也寫明它不打算當成 Chromium 以外能嵌入的函式庫：介面是 C++ 的類別、不是穩定的 C ABI，依賴 `//base`，早期的 standalone 模式已經不支援。
- **Rust 的 binding**（`mojo/public/rust`）：2026-04-16 在 chromium-dev 宣布可用（Rust、C++、Java、JavaScript 之間能互送訊息；`Remote`、`Receiver`、associated interface）。用 GN 的 `mojom()` target 加 `generate_rust = true` 產生，文件沒有提 cargo；associated 的 endpoint 靠 C++ 的元件。所以它包的是 C++ 的 Mojo core，不是另一份實作。

### Linux 的 channel

- non-blocking 的 Unix socketpair（socket 的型別沒有在原始碼裡確認到，就我們的了解是 stream）。
- `mojo/public/cpp/platform/socket_utils_posix.cc`：`sendmsg` 帶 `MSG_NOSIGNAL`，fd 放在一個 `SCM_RIGHTS` 的控制訊息裡；`recvmsg` 沒帶 `MSG_CMSG_CLOEXEC`；看到 `MSG_CTRUNC`（註解說 fd 用完時會發生）就放掉已經收到的 fd、回 0。
- **`ChannelLinux`**（`mojo/core/channel_linux.cc`）：在 socket 之上可以「升級」成共用記憶體。一端建一塊 sealed 的 memfd 當 ring（開頭是 `write_pos`、`read_pos` 和兩個旗標，預設 4 頁，對方只接受 1 到 128 頁）和一個 eventfd，經過 socket 交給對方；之後沒帶 handle、放得進去的訊息寫進 ring、用 eventfd 喚醒對方，帶 handle 的和放不下的照舊走 socket。兩端都驗證收到的 fd 是 sealed 的 memfd。這個檔案裡預設是關的。

### macOS 的 channel

`mojo/core/channel_mac.cc`：

- 開始時一端只有 send right、另一端只有 receive right；有 send right 的一端建自己的 receive port，送一則 handshake 把通往它的 send right 帶過去。
- receive port 的佇列上限調大（`MACH_PORT_QLIMIT_LARGE`）。
- 送的時候 timeout 是 0；`MACH_SEND_TIMED_OUT` 時訊息留在緩衝區，之後的訊息排進 `pending_messages_`，在 IO thread 上重試。
- 一頁以內的訊息內嵌，超過的改用 out-of-line 的 descriptor（`MACH_MSG_VIRTUAL_COPY`）。handle 是 port descriptor；fd 用 `fileport_makeport` 轉成 port。
- 對方不在了：handshake 之前在 receive port 上登記 no-senders，之後在 send port 上登記 dead-name，通知送到自己的 receive port。`MACH_SEND_INVALID_DEST` 時不直接報告斷線，等 dead-name 的通知到了再處理，免得漏掉那個 dead name（crbug 1041682）。
- 驗證：通知要是 kernel 送的（audit trailer 是 `KERNEL_AUDIT_TOKEN_VALUE`）；記下 handshake 時對方的 audit token，之後不符的訊息丟掉；descriptor 只收 port descriptor。
- 等訊息：receive port 掛在 IO thread 的 message pump 上（`WatchMachReceivePort`），可讀時用 timeout 0 的 `mach_msg` 收。

**`MachPortRendezvous`**（`base/apple/mach_port_rendezvous.h`、`mach_port_rendezvous_mac.cc`）：macOS 不能讓 child 繼承 Mach port 的 right。parent 的 server 用 `bootstrap_check_in` 登記成 `<bundle id>.MachPortRendezvousServer.<pid>`；child 用 `getppid()` 拼出同一個名字、`bootstrap_look_up`、送請求；server 用 `audit_token_to_pid` 找出記在那個 pid 底下的 port 回給它。只給直接的 child，每個 pid 只能拿一次，一次最多 8 個 port。iOS 上改成經過 XPC 的連線找到 server。

**socket 上傳 fd 的 bug**（`mojo/core/channel_posix.cc`）：註解寫 XNU 有個 bug，fd 還在傳送途中就關掉是危險的（crbug 298276），所以送出之後把 fd 留著、等對方 ack 才關（`HANDLES_SENT`、`HANDLES_SENT_ACK`）。這段現在只在 iOS 編譯；macOS 走上面的 Mach port。實測見「macOS 上傳 fd 的實測」。

### Windows 的 channel

`mojo/public/cpp/platform/platform_channel.cc`、`mojo/core/channel_win.cc`：

- 建立：server 端 `CreateNamedPipeW`，`PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE`、`PIPE_TYPE_BYTE | PIPE_READMODE_BYTE`、最多一個 instance、進出的 buffer 各 4096；名字是亂數產生的。client 端 `CreateFileW`，`SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS | FILE_FLAG_OVERLAPPED`（註解：server 端因此不能假冒 client），handle 設成可以繼承；child 從命令列的參數讀到它的數值。
- 讀寫：都是 overlapped。寫入排在佇列裡，一次只有第一筆在進行，寫完再送下一筆；`Write()` 不等完成。完成的通知經過 IO thread 的 message pump（IOCP，或每個操作一個 event）。
- 斷線：讀失敗（`ERROR_IO_PENDING` 以外）、讀完成時失敗或讀到 0 個位元組，都當成斷線；寫失敗之後不再寫，等讀到結尾才報告。
- 結束：`CancelIoEx` 取消這個 handle 上所有進行中的 I/O（不管是哪條 thread 發的），再關 handle；沒有 `FlushFileBuffers`，也不等還沒寫完的。
- handle：送出時由有對方 process handle 的一方複製進對方；收到時拒絕 pseudo handle。`kMojoHandleTypeProtections` 預設開啟，檢查傳過來的 handle 的型別。

### 共用記憶體

- **三種型別**（`base`）：`ReadOnlySharedMemoryRegion`（平台那一層保證接收端只能唯讀 map）、`WritableSharedMemoryRegion`（可以寫，能轉成唯讀一次）、`UnsafeSharedMemoryRegion`（雙方都能寫，自己同步）。
- **Linux**（`base/memory/platform_shared_memory_region_posix.cc`）：先試 `memfd_create`（先帶 `MFD_NOEXEC_SEAL`，回 `EINVAL` 就拿掉），加 `F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_SEAL`，註解說這樣拿著它的人就不能縮小檔案、讓別的 process 存取時出錯；`memfd_create` 不能用、或要可執行的 region 時，退回 `/dev/shm` 裡建立後 unlink 的檔案（沒有 seal）。唯讀的 fd 是經過 `/proc/self/fd/<fd>` 用 `O_RDONLY` 再開一次。收到 region 時用 `fcntl(F_GETFL)` 檢查 fd 的存取模式和預期的相符。
- **macOS**（`base/memory/platform_shared_memory_region_apple.cc`）：`mach_make_memory_entry_64` 加 `MAP_MEM_NAMED_CREATE | VM_PROT_READ | VM_PROT_WRITE` 建立；轉成唯讀時先用唯讀 map、再對那塊記憶體建一個只有 `VM_PROT_READ` 的 entry，原本的 right 放掉；收到 region 時試著 map 成可寫，唯讀的應該回 `KERN_INVALID_RIGHT`。不用 fd，沒有改大小的操作。
- **broker**：sandbox 不讓 renderer 配共用記憶體、或不能把 handle 複製進別的 process 的平台，由 Browser 代做。

### 大的資料

- `mojo_base::BigBuffer`：`kMaxInlineBytes` 是 64 KB，以下放在訊息裡，以上複製進共用記憶體只傳 handle。註解警告共用記憶體在接收端讀的時候，送的一端可能還在寫，從不可信的來源收到時有 TOCTOU 的風險。
- data pipe：共用記憶體裡的 ring，用在串流，例如 `URLLoaderClient` 收網路回應的 body。
- GPU 的 command buffer：命令寫進共用記憶體的 ring，大的資料放在 transfer buffer，Mojo 訊息只送 flush 和位置；OOP-R 時 renderer 把 `PaintOp` 序列化進 transfer buffer。
- `CompositorFrame` 是序列化的 Mojo 訊息，資源只帶 Mailbox、SharedImage 和 sync token。

## Servo 的 IPC

對照 servo/servo 和 servo/ipc-channel 的原始碼和 git 歷史。

- **ipc-channel**：serde 的訊息，走各平台原生的 primitive。
  - Linux 和其他 Unix：Unix socket，`SCM_RIGHTS` 傳 fd；超過一個 fragment 的訊息另開一條專用的 channel 分段送。
  - macOS：Mach port，從一開始就是。repo 的第一個 commit 是 2015-06-19，2015-06-23 時整個 repo 只有 `platform/macos/`；Linux 的 Unix socket 是之後加的。大的訊息改成 out-of-line 的記憶體描述子；共用記憶體也是放在 out-of-line 的描述子裡送。
  - Windows：named pipe，handle 用 `DuplicateHandle` 複製進對方。2021-03-10 才合併（PR 272，程式最早寫在 2016-09），0.15.0 起包含。
- **`IpcSharedMemory`**：Linux 用 `memfd_create` 加 `mmap(MAP_SHARED, PROT_READ | PROT_WRITE)`，接收端拿到的也是可寫的 mapping；沒有唯讀的型別。
- **macOS 上怎麼連上**：`IpcOneShotServer::new()` 建一個 receive port，用 `bootstrap_register2` 登記成 `org.rust-lang.ipc-channel.<亂數>`；Servo 把名字放在命令列（`--content-process <名字>`）；child 用 `bootstrap_look_up` 查到、送第一則訊息；`accept()` 收到第一則就當成連上，之後註銷名字。macOS 的 backend 裡沒有 audit token 或 pid 的檢查。
- **Servo 的 macOS sandbox profile**（`components/constellation/sandboxing.rs`）只開放查 `com.apple.FontServer`，沒有開放查 ipc-channel 的名字，所以應該是先連線、再套用 sandbox（從 profile 推斷的，沒有讀 child 那一側的順序）。
- **`generic_channel`**：`GenericSender` / `GenericReceiver` 依 `multiprocess` 或 `force_ipc` 的設定，在執行時選 ipc-channel 或 crossbeam；`GenericSharedMemory` 同樣選 `IpcSharedMemory` 或 `Arc<Vec<u8>>`，單一 process 時不複製。
- **用共用記憶體的地方**：WebRender 的圖片、字型、canvas、`ImageData`、WebGL 和 WebGPU 的 buffer、`fetch` 的 body。display list 不用：序列化成 `Vec<u8>` 走一般的訊息。

## Ladybird、Firefox、WebKit 的 IPC

2026-11-17 查的。Ladybird 對照它在 GitHub 上的原始碼；Firefox 和 WebKit 只看了搜尋到的 bug、changeset 和原始碼的鏡像。

- **Ladybird**（`Libraries/LibIPC`）：
  - Linux：Unix socket 加 `SCM_RIGHTS`。Windows：socket，handle 用 `DuplicateHandle` 複製進對方。
  - macOS：2026-03 從 Unix socket 換成 Mach port（`TransportMachPort`）。附件是 port 的 right；fd 用私有的 `fileport_makeport` / `fileport_makefd` 轉成 port 再轉回來（原始碼的註解說它們穩定、XPC 也在用）。
  - macOS 上怎麼連上（`TransportBootstrapMach.cpp`）：child 查到 bootstrap server 上的 port，送一則帶著 reply port 和自己的 task name port 的請求；server 照 pid 找出登記的 port，把 receive right 和 send right 放在回覆裡搬過去。
  - 等訊息：一條專用的 IO thread 阻塞在 `mach_msg`，等一個 port set（receive port 加一個自己的喚醒用的 port）；佇列上限調到 `MACH_PORT_QLIMIT_MAX`；在 receive port 上登記 no-senders 當成 EOF；送不出去的訊息留在 backlog，IO thread 重試時照樣繼續收（兩邊都卡在送的話會互相等）。
- **Firefox**：2021 年把 Chromium 的 `mojo/core/ports`（路由的演算法）搬進來，上面的 `NodeController` 和 `NodeChannel` 自己寫，接在原本的 IPC channel 上（Bug 1706374）。只拿演算法，不拿 Mojo 的 core 和 binding。
- **WebKit**：給 GPU process 的 stream connection（2021 年的 changeset 273204）是共用記憶體裡的 ring：小的訊息寫進 ring，放不下的走一般的 IPC；讀寫的位置是 atomic，收的一端靠 semaphore 喚醒，之後的版本把喚醒批次化。

## 調查過的 crate

2026-11-17 的版本和狀態。

| crate | 版本（發布日） | 做什麼 | 用不用 |
| --- | --- | --- | --- |
| rustix | 已經在依賴樹裡 | `memfd_create`、`fcntl_add_seals`、`sendmsg` 和 `SendAncillaryMessage::ScmRights` 都是安全的函式 | Linux 的建立和傳 fd |
| memmap2 | 0.9.11（2026-06-22） | map 檔案或 fd | Linux 的 map |
| mach2 | 0.7.0（2026-08-30）；依賴樹裡是 0.6.0 | Mach 的 user-space API 的宣告 | macOS 的平台層 |
| windows-sys | 已經在依賴樹裡 | Windows 的 API 的宣告 | Windows 的平台層 |
| ipc-channel | 0.23.1（2026-10-24） | Servo 的 IPC（「Servo 的 IPC」） | 不用 |
| interprocess | 2.4.8（2026-11-08） | 用名字連線的 local socket、Windows 的 named pipe、unnamed pipe；沒有共用記憶體，公開的 API 裡沒有傳 fd 或 handle | 不用 |
| iceoryx2 | 0.10.0（2026-09-18） | 共用記憶體的 pub/sub、request/response、event、blackboard | 不用 |
| capnp-rpc | 0.28.0（2026-11-15） | Cap'n Proto 的 RPC | 不用 |
| mojo-rs | 0.3.0（2026-08-05） | 自稱是 Mojo 的純 Rust 實作 | 不用 |
| shmem-ipc | 0.3.0（2022-11-22） | 給不可信的 process 用的共用記憶體 IPC，只支援 Linux | 不依賴，照它的 `ringbuf.rs` 的設計（「ring 的設計」） |
| shared_memory | 0.12.4（2022-03-01） | 用名字打開的共用記憶體 | 不用 |
| ipmpsc | 0.5.1（2021-05-24） | 共用記憶體的 MPSC | 不用 |
| memfd | 0.6.6（2026-09-01） | `memfd_create` 和 seal 的安全包裝，只支援 Linux 和 Android | 不用，rustix 有同樣的 API |
| ringbuf | 0.5.2（2026-09-13） | process 內的 SPSC ring | 不用 |
| crossmist | 1.1.2（2024-09-06） | 跨 process 的 channel，之後沒有更新 | 不用 |

- **mach2**：對照過要用的函式：`mach_msg` 和訊息的結構、port 的建立和 right、`mach_make_memory_entry_64`、`mach_vm_map`、`bootstrap_check_in`、`bootstrap_look_up`、通知、audit trailer 都有。沒有 `audit_token_to_pid`（它在 libbsm）；`MAP_MEM_NAMED_CREATE` 這個常數有沒有還沒確認。依賴樹裡的 0.6.0 是 wasmtime 的 fork 帶進來的。
- **interprocess**：Unix 上的 stream 只是包著 std 的 `UnixStream`。Windows 上 named pipe 用 overlapped 開，能拆成收和送兩半。它不在 machopo 的依賴樹裡。2.4.7 起 named pipe 的 `TryFrom<OwnedHandle>` 不再試著用 overlapped 模式重開（issue 100），同一版修了 `ConnectNamedPipe` 沒帶 `OVERLAPPED` 而提早完成的 bug（issue 101）；兩個都在「Browser 建好兩端、child 從 handle 接手」這條路上。
- **ipc-channel**：0.21.0（2026-02-03）把序列化從 bincode 換成 postcard。
- **iceoryx2**：
  - 做法（就我們的了解，README 和 FAQ 沒有講內部）：publisher 建立時先配好一大塊共用記憶體，送的時候從裡面借一格、直接寫進去，再把那一格的位移放進每個 subscriber 的 lock-free 佇列；subscriber 拿到位移直接讀。不序列化、不複製、不經過系統呼叫。
  - 限制（FAQ）：資料型別要 `#[repr(C)]`、不能有指標，不能放 `Vec`、`String`；記憶體先配好最壞的情況；還沒有服務的權限管理，只有同一個使用者能存取。
  - 服務的狀態放在 `/dev/shm` 和 `/tmp/iceoryx2`，用名字找；macOS 和 Windows 是 Tier 2。
  - 2026-10-06 還開著的 issue：#1951（macOS、Windows 上狀態目錄是編譯期常數，不能依使用者隔離）、#1880（dead node 清理時 `fatal_panic` 會殺掉不相關的 process）、#2034（macOS 上 `shm_open` 回報 ENFILE）。
- **memmap2**：Unix 上 `map_anon` 用 `MAP_PRIVATE | MAP_ANON`，不能分享，所以只負責 map。Windows 上它只吃檔案的 handle，不能 map 我們建的 section。檔案的 `map`、`map_mut` 是 `unsafe`，`map_raw` 是安全的，回傳的 `MmapRaw` 只給指標。
- **mojo-rs**：同一天發了三版，只列 `x86_64-unknown-linux-gnu`。

## 比過的做法

`README.md` 的「理由和其他做法」只寫結論，這裡是每一項的選項。標「原本」的是 2026-10-06 選的、2026-11-17 改掉的。

### IPC 整體

- **I1. ipc-channel**：serde、能傳 channel 和共用記憶體，Servo 用它拆 process。不選：每則訊息都經過 socket 或 pipe 的系統呼叫；`IpcSharedMemory` 給對方的是可寫的 mapping，沒有唯讀的型別；macOS 上連線時不驗證對方；要跟著 Servo 的需求走。
- **I2. 自己做，照 Chromium 現在的方向**：選這個。對 renderer 的不信任可以照 Chromium 做到底，訊息也能逐步搬進共用記憶體。
- **I3. 照 Mojo 做一整套**（message pipe、介面、自動產生的 binding）：最接近 Chromium，但是一個大專案；`protocol` 的 enum 現在沒有需要介面層的地方，之後要加也能疊在 I2 上面。
- **I4. iceoryx2**：最快，`unsafe` 都在它裡面。不選：用全域的名字找服務，同一個使用者的任何 process 都能打開，Seatbelt 也得開放那些路徑；它假設參與的 process 互相合作；`protocol` 的訊息有 `Vec` 和 `String`，放不進去。
- **I5. 直接用 Mojo**：Rust 的 binding 只在 Chromium 的 GN 裡 build，底下是 C++ 的 core；ipcz 不打算在 Chromium 以外使用。要用就得把 `//base` 和 `mojo/core` 拉進 build。
- **I6. Cap'n Proto RPC**：能在訊息裡傳 capability，最接近 Mojo 的介面層。不選：要 schema 和 codegen、要 async executor，訊息不再是 serde 的 enum，也不管 handle 和共用記憶體。

### 單一 process 時訊息怎麼走

- **A. crossbeam，不序列化**：選這個（Servo 的 `generic_channel` 也是）。單一 process 不該付 IPC 的成本。缺點是 IPC 那條路平常沒有測試在跑，由 CI 用多個 process 跑整套的測試補上。
- **B. 序列化、不經過 OS 的通道**（Chromium 的 `--single-process`，就我們的了解）：只多守住「實際送出的值經得起序列化」。
- **C. 同一個 process 裡走真的控制通道和共用記憶體**：現有的測試都能守到 IPC，但每則訊息多編碼、解碼和系統呼叫，還多一種模式要維護。真的多 process 的測試守得比它完整。

### 單一 process 時 Browser 的 IO thread

renderer 這一側在單一 process 時本來就沒有 IO thread：crossbeam 是 process 裡的東西，每條 thread 直接等自己的佇列。Browser 這一側有，因為 UI thread 等的是視窗系統的 event loop，沒辦法同時等 crossbeam；而且子資源的請求不用吵醒 UI thread 就能交給網路。它轉給 UI thread 靠的是 Browser 給的 callback，那個 callback 把訊息塞進 event loop 並叫醒它，任何 thread 都能呼叫。

- **留著 IO thread，邏輯抽成物件**：`README.md` 現在寫的。兩種模式都是「IO thread 讀出訊息、交給物件」，差別只在讀的是 crossbeam 還是 `ipc` 的通道。
- **單一 process 時直接呼叫那個物件**（`README.md` 的「待決定」）：renderer 送訊息的那條 thread 自己呼叫，沒有 IO thread。邏輯還是只有一份。和 `BeginFrame` 的做法同一招：Browser 建好一個東西交給 renderer，裡面做什麼由 Browser 決定。
- **每條 thread 各開一條通道、各自讀**（多 process 時也不要 IO thread）：不選。每條 thread 都得改成能等 kernel 的物件；不同通道之間訊息的先後沒有保證；main thread 忙的時候沒有人讀它那一條，對方死了也不知道。

就我們的了解，Chromium 在把網路搬到獨立的 process 之前，也是用掛在 IO thread 上的 message filter 處理 renderer 的資源請求。

### macOS 的控制通道

- **X. Unix socket 加 `SCM_RIGHTS`**（原本）：和 Linux 共用、都是安全的 API、child 繼承一個 fd 就好。不選：實測送出後馬上關掉的 socket fd 幾乎全壞（「macOS 上傳 fd 的實測」），之後只要有人在訊息裡帶 socket 就會默默出錯。
- **Y. 同 X，但訊息只能帶共用記憶體的 region**：實測共用記憶體的 fd 沒事，由型別擋住會壞的用法，成本最小。不選：之後要傳通道的一端或 `IOSurface` 時還是得換。
- **Z. Mach port**：選這個。Chromium、ipc-channel 和 Ladybird 都這樣做；port 的轉移在 kernel 裡一次完成。代價是 `unsafe`、另一套平台程式和 rendezvous。

### macOS 上 child 怎麼拿到 port

- **照 Chromium**：選這個。一個長期的 server、名字由 pid 決定、靠 audit token 的 pid 發 port。名字是固定的前綴加 pid，可以直接寫進 Seatbelt 的 profile。
- **照 Servo**：每個 child 登記一個帶亂數的名字、放在命令列、用完註銷。不選：不驗證對方，名字在命令列上看得到，誰先連上就是誰；它是先連線再鎖 sandbox，和這份「一開始就鎖」不合。
- **換掉 child 的 bootstrap port**：不適用，系統的服務（例如字型）也靠 bootstrap port（就我們的了解）。
- **XPC service**：不適用，要 app bundle 裡放 service 的 bundle，machopo 是單一執行檔（就我們的了解）。

### macOS 的共用記憶體

- **M1. Mach 的 memory entry**：選這個，照 Chromium。本身就是 port、大小固定、唯讀由 kernel 擋、不佔 fd、都是公開的 API。
- **M2. POSIX shm**（原本；rustix 的 `shm::open`，開完馬上 unlink，memmap2 map）：建立和 map 都是安全的 API。不選：控制通道換成 Mach port 之後訊息不能直接帶 fd，要用私有的 `fileport_makeport`；拿到可寫 fd 的一方能不能縮小它也沒有驗證過。

### Mach 的 FFI

- **mach2**：選這個。已經在依賴樹裡；結構的 layout 是最容易錯、錯了最難發現的地方。
- **在 `ipc` 裡自己宣告**（ipc-channel 這樣做）：沒有依賴。不選：Mach 訊息的結構是 4 byte 對齊的，寫錯就是記憶體錯誤。

兩者都只是原始的宣告；port 的擁有權、訊息的組裝這些安全的包裝都要在 `ipc` 裡自己寫。

### macOS 上發現對方不在了

| | dead-name | no-senders | 兩種都登記 |
| --- | --- | --- | --- |
| 誰這樣做 | Chromium（handshake 之後） | Ladybird；Chromium 在 handshake 之前 | 選這個 |
| 登記在哪 | 通往對方的 send right | 自己的 receive port | 兩邊 |
| process 結束 | 發現得到 | 發現得到 | 發現得到 |
| 對方只關掉收的一半 | 發現得到 | 等下一次送失敗才知道 | 發現得到 |
| 對方只丟掉送的一半 | 發現不到 | 發現得到 | 發現得到 |

### Windows 的控制通道

- **W1. interprocess 的 named pipe**（原本）：通道本身沒有 `unsafe`。不選：是新的依賴；這份的用法正是它 2026-10 修了兩個 bug 的那條路；它的寫入是等到寫完的（就我們的了解），要另外加一條寫入的 thread。
- **W2. 直接用 windows-sys**：選這個，照 Chromium。windows-sys 已經在依賴樹裡；寫入不等；IO thread 的形狀和另外兩個平台相同。代價是 `unsafe`。
- **W3. std 的匿名 pipe 兩條**（`std::io::pipe`）：沒有 `unsafe`、沒有依賴。不選：同步的 pipe 讀的時候叫不醒，對方不讀時寫入會卡住。

不管選哪個，把 handle 交給 child、`DuplicateHandle`、`MapViewOfFile` 都本來就要 `unsafe`。

### Linux

和 Chromium 幾乎一樣（「Chromium 的 IPC」）。不同的地方：唯讀用 `F_SEAL_FUTURE_WRITE`，Chromium 是用 `O_RDONLY` 再開一次（就我們的了解，唯讀的 fd 可以經過 `/proc` 重新開成可寫，這個 seal 就是為了擋它才加進 kernel 的；Chromium 靠的是 renderer 碰不到 `/proc`）；收 fd 時帶 `MSG_CMSG_CLOEXEC`，Chromium 沒帶；`memfd_create` 失敗時不退回 `/dev/shm` 的檔案（檔案加不上 seal）。

考慮過把 socket 換成 `SOCK_SEQPACKET`（kernel 保留訊息的邊界，每則訊息和它的 fd 一起到）：一則訊息的大小受 socket 的 buffer 限制，renderer 送大的訊息給 Browser 時得自己分段，不如用 stream、自己寫長度。

### 誰能附 handle 和誰建立共用記憶體

- **只有 Browser 建立；能不能附 handle 是通道每一端的權限，renderer 那一端沒有**：選這個。renderer 在訊息裡只寫編號，Browser 查自己的表。三個平台的檢查都一樣，只有「編號在不在表裡、範圍有沒有超出」。
- **寫死成「只有 Browser 能附 handle」**：這份只有兩種 process 時結果一樣。不選：之後 GPU process 要交出只有它建得出來的緩衝區；Windows 上 child 之間不能直接複製 handle，要經過 Browser 轉手，也就是 Browser 要從那個 child 收 handle。做成每一端的權限，到時只是多開一個權限。
- **renderer 把 handle 附在訊息上送回來**（原本）：Browser 要驗證收到的 handle：Linux 確認是 sealed 的 memfd，Windows 從 renderer 的 process 把 handle 拉過來、檢查型別，macOS 檢查 descriptor。每個 frame 還多一次傳 handle 的系統呼叫。
- **renderer 自己建立**（就我們的了解，Chromium 在 sandbox 允許的平台上這樣做）：省掉向 Browser 要的那一次來回。不選：Browser 得驗證每一塊，renderer 的 sandbox 也要多開權限。

向 Browser 要一塊時要等回覆，是「只有 Browser 建立」的成本，和誰能附 handle 無關。

### ring

ringbuf、rtrb、crossbeam 的 `ArrayQueue` 是給同一個 process 的 thread 用的，信任 index；iceoryx2 的 lock-free queue 能放進共用記憶體，但威脅模型是互相合作的 process。shmem-ipc 的 `ringbuf.rs` 是為不可信的對端設計的，只有一個檔案、不依賴 Linux，所以照它的設計自己寫，不依賴那個 crate（它整體只支援 Linux、2022 年之後沒更新）。Chromium 的 `ChannelLinux` 和 WebKit 的 stream connection 是同一個想法的另外兩個實作。

### tile 怎麼交給 Viz（選了 T1，`README.md` 的第 23 條）

- **T1. 共用記憶體，Viz 上傳**（one-copy raster；Chromium 在 OOP-R 之前的做法）：renderer 不碰 GPU，sandbox 最緊；raster 的位置不變。
- **T2. 跨 process 的 GPU texture**（macOS 的 `IOSurface`）：renderer 自己上傳，要能存取 GPU，sandbox 得開 IOKit；wgpu 沒有跨 process 共用 texture 的 API，要動到 wgpu 的 Metal 層。
- **T3. OOP-R**：renderer 送 display list，raster 在 Viz（或之後的 GPU process）那一側。Chromium 現在的做法，但 display list、字型和圖片都要能序列化，raster 也要搬家。

### 字型（留給 sandbox 的 RFC）

- **F1. sandbox 開放系統字型目錄的讀取**：Chromium 在 macOS 上的 renderer 也直接讀系統字型。
- **F2. Browser 提供字型的服務**（Chromium 在 Linux 和 Windows 的 font proxy）：sandbox 更緊，但要做一個 IPC 服務，文字的函式庫要改成從它拿字型。

## 平台層

### Linux

- 建立：rustix 的 `memfd_create`，帶 `MFD_ALLOW_SEALING`、`MFD_CLOEXEC` 和 `MFD_NOEXEC_SEAL`（kernel 回 `EINVAL` 就不帶它再建一次，`README.md` 的第 46 條）；設好大小之後加 `F_SEAL_SHRINK`、`F_SEAL_GROW`，最後加 `F_SEAL_SEAL`。
- 唯讀：`F_SEAL_FUTURE_WRITE`（Linux 5.1 起）讓已經存在的可寫 mapping 照舊能寫，但之後誰都不能再 map 成可寫，所以 Browser 先 map 好自己的，再交出 fd。seal 是檔案本身的，拿到 fd 的人經過 `/proc/self/fd/N` 重新開也寫不了。
  - Chromium 不是這樣做（`base/memory/platform_shared_memory_region_posix.cc`，2026-11-22 讀的 main）：它經過 `/proc/self/fd/N` 重新開一個 `O_RDONLY` 的 fd 交出去。拿到這種 fd 的 process 可以經過 `/proc` 再開一次、開成可寫，Chromium 靠 sandbox 不讓 renderer 讀 `/proc` 擋住。`F_SEAL_FUTURE_WRITE` 不靠 sandbox。
- 只有 memfd 建的檔案能加 seal：一般 tmpfs 的檔案一開始就帶著 `F_SEAL_SEAL`，所以不能改用 `/dev/shm` 底下的暫存檔。
- map：memmap2 的 `MmapRaw`。給了長度時它不確認檔案夠不夠大，先用 `fstat` 確認（第 47 條）。
- 控制通道：std 的 `UnixStream::pair`，每次讀寫帶 `MSG_DONTWAIT`，緩衝區不設（第 44 條）；一端給 child 繼承（第 43 條）。IO thread 用一個 level-triggered 的 epoll 等所有通道的 socket 和一個 eventfd；「可寫」只在寫入被擋之後才登記（第 48 條）；接收緩衝區 256 KiB（第 49 條）。
- 傳 fd：rustix 的 `sendmsg` 和 `SendAncillaryMessage::ScmRights`，帶 `MSG_NOSIGNAL`（對方關掉時不要 SIGPIPE）；收的一端用 `MSG_CMSG_CLOEXEC`，一次讀到超過 16 個 fd（核心的 `MAX_HANDLES`）或控制資料被截斷（`MSG_CTRUNC`）就當成對方不在了；個數要自己數，給 16 個 fd 的控制資料緩衝區實測收得進 17 個（大小會進位）。fd 只由 Browser 送給 renderer；Browser 這一端收到的 fd 照樣交給核心，由它判成壞訊息（`HandlesNotAllowed`）。
- 對方不在了：讀到 0 個位元組或讀寫錯誤。不看 `EPOLLHUP`：對方關掉之前送的訊息要先讀完、交出去。

### macOS

都經過 mach2 的 FFI。

- 建立：`mach_make_memory_entry_64`，帶 `MAP_MEM_NAMED_CREATE | VM_PROT_READ | VM_PROT_WRITE`，拿到一個代表這塊記憶體的 port。沒有名字、不佔 fd；大小建立時固定（kernel 回報的可能比要的大，以它為準）。
- map：`mach_vm_map`；unmap 用 `mach_vm_deallocate`。
- 唯讀：另建一個只有 `VM_PROT_READ` 的 entry 交給對方（Chromium 是先用唯讀 map 一次、再對那塊記憶體建 entry）。拿它 map 成可寫，kernel 回 `KERN_INVALID_RIGHT`。
- 控制通道：兩個 Mach port，都由 Browser 建立。Browser 留著自己的 receive port 和通往 child 的 send right；child 的 receive port 和通往 Browser 的 send right 經過 rendezvous 交給它。因為兩個 port 一開始就都建好，不需要 Chromium 的 handshake。
- 傳 handle：memory entry 本身就是 port，放在 Mach 訊息的 port descriptor 裡；port 的轉移在 kernel 裡一次完成。Browser 這一端收到任何 descriptor 都是壞訊息。
- sandbox：region 都由 Browser 建立；memory entry 沒有名字，profile 不用為它開任何東西。
- 沒有 tmpfs：用檔案撐共用記憶體的話在 APFS 的磁碟上，髒頁會被寫回，所以不考慮檔案。

### Windows

都經過 windows-sys 的 FFI。

- 建立：`CreateFileMappingW(INVALID_HANDLE_VALUE, …, PAGE_READWRITE, …)`，用 pagefile 撐，大小固定；map 用 `MapViewOfFile`，unmap 用 `UnmapViewOfFile`。
- 唯讀：`DuplicateHandle` 時只給 `FILE_MAP_READ`。
- 控制通道：Browser 用亂數的名字建 server 端（`CreateNamedPipeW`，`PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE`，位元組模式，最多一個 instance），自己再開 client 端（`CreateFileW`，`FILE_FLAG_OVERLAPPED`），把 client 端交給 child；不靠名字連線。旗標照 Chromium。
- 傳 handle：named pipe 不能帶 handle。Browser 用 `DuplicateHandle` 把 handle 複製進 renderer 的 process，訊息只帶數值。renderer 在 sandbox 裡沒有 Browser 的 process handle，自己複製不了，也不需要：它只送編號。
- 考慮過用暫存檔撐共用記憶體以減少 `unsafe`：記憶體吃緊時髒頁可能寫回磁碟，防毒軟體會掃描新建的檔案，`DuplicateHandle` 也還是要 `unsafe`，所以維持 pagefile。

## macOS 上傳 fd 的實測

2026-11-17，macOS 26.6.2（Darwin 25.6.0）。起因：Chromium 的 `channel_posix.cc` 說 XNU 有 bug，fd 在傳送途中關掉是危險的，而當時選的是 macOS 也用 Unix socket 加 `SCM_RIGHTS`。

**測法**：parent 和 fork 出來的 child 之間一條 `socketpair`。parent 建一個 fd、把一個記號寫進它指到的東西、用 `SCM_RIGHTS` 連同記號送出、`sendmsg` 一回來就關掉自己的 fd；child 之後才收，檢查收到的 fd 讀得到同一個記號。每種 fd 送 20,000 個，分兩種時機：一批 32 個全部送完、關完，child 才開始收；或 child 一邊收、parent 一邊送和關。對照組是 parent 等 child 說收到了才關。

| 送的 fd | 送出後馬上關：收到的不能用的 | 等對方收到才關 |
| --- | --- | --- |
| POSIX shm（`shm_open` 之後 unlink） | 0 | 沒測 |
| 檔案（建立後 unlink） | 0 | 沒測 |
| pipe 的讀端 | 0 | 沒測 |
| Unix socket，另一端等 child 收完那一批才關 | 一批一批時 17,028 和 17,546（跑了兩次） | 0 |
| Unix socket，另一端在送出之前就關掉 | 一批一批時 19,997 和 20,000（跑了兩次）；邊收邊送時 2,041 | 0 |

- 壞掉的樣子：child 收到一個 fd，但第一次 `read` 就回 end of file，寫在 socket 裡的記號不見了。
- 邊收邊送時「另一端還開著」那一組不乾淨：腳本在送出之後馬上也關了另一端，沒有等 child 收到；那一組壞了 3,954 個，不列在表裡。
- 所以這個 bug 在 macOS 26 還在，但在這個測試裡只影響 socket 的 fd。0 個出錯不是證明：Chromium 的註解沒有說只限 socket。
- 測試的腳本是 Python（`socket.send_fds`、`socket.recv_fds`），沒有放進 repo；Linux 上沒有跑。之後在 Linux 上要傳 socket 的 fd 時，先照同樣的測法測。

## Linux 上 socket 的緩衝區和門檻的實測

2026-11-22，Fedora、kernel 7.3.3、AMD Ryzen AI 9 HX 370（筆電，`powersave` governor、`balanced` 的電源設定）。`README.md` 的第 44、45、49 條用的數字。

**測法**：一個暫時的 C 程式（沒有放進 repo），兩個 process 之間一條 `socketpair(AF_UNIX, SOCK_STREAM)`；收的一端用 level-triggered 的 epoll，每個事件做一次 `recv`。量兩種：

- **照核心的複製次數**（2f 的平台層會走的路）：內嵌是送的一端編碼時複製一次、寫進 socket，收的一端 `recv`、存進還沒讀完的那一則（複製）、解碼成新的緩衝區（複製，`BigBuffer::copy_from_slice`）；region 是送的一端複製進留著重複用的 memfd、送一則 16 bytes 的通知，收的一端從 region 複製進新的緩衝區。
- **照原型的量法**（`prototype/` 的 `src/others.rs`，「和 ipc-channel、interprocess 比」那一節）：blocking 的 socket、先長度後內容，收的一端讀進自己重複用的緩衝區，不算核心的複製。

每組跑兩次。數字有雙峰（例如 64 KiB 有時 11 µs、有時 17 µs），看起來和兩個 process 排到哪一種核心有關，只看兩次一致的趨勢。`SO_SNDBUF` 寫的是 `getsockopt` 讀回來的（kernel 加倍過的）值。

**一條通道：送一則、回答 8 bytes（中位數）**

照核心的複製次數，`SO_SNDBUF` 是預設（212992）、接收緩衝區 64 KiB：

| 大小 | 內嵌 | region |
| --- | --- | --- |
| 16 KiB | 9–10 µs | 5–8 µs |
| 32 KiB | 12 µs | 10 µs |
| 64 KiB | 11–12 µs | 12 µs |
| 128 KiB | 18–23 µs | 13 µs |
| 256 KiB | 28–37 µs | 15–19 µs |
| 512 KiB | 50–54 µs | 28–31 µs |
| 1 MiB | 106–110 µs | 50 µs |
| 8 MiB | 1.24–1.43 ms | 0.53–0.60 ms |

- `SO_SNDBUF` 是 425984（要 212992，6.18 之前的 kernel 的上限）、2 MiB、8 MiB 時，64 KiB、1 MiB、8 MiB 內嵌的數字都落在同一個範圍（1 MiB 都是約 106 µs，偶爾一次 160 或 250 µs，是上面的雙峰）。
- 接收緩衝區 64 KiB 和 256 KiB 一樣。

照原型的量法：

| 大小 | 內嵌，預設的緩衝區 | 內嵌，`SO_SNDBUF`、`SO_RCVBUF` 都要 1 MiB | region |
| --- | --- | --- | --- |
| 64 KiB | 8–11 µs | 9–12 µs | 7–11 µs |
| 1 MiB | 54–68 µs | 90–92 µs | 50 µs |
| 8 MiB | 675–698 µs | 691–699 µs | 549–561 µs |

- 和原型在 macOS（Apple M4）上的比：macOS 上調大緩衝區讓 1 MiB 從 430 µs 到 41 µs（預設只有 8 KiB）；Linux 上調大沒有變快，1 MiB 兩次都變慢，原因沒有查。純傳輸時兩個平台都是內嵌和 region 接近；絕對值不能跨機器比（region 1 MiB 在 M4 是 36 µs，這台 50 µs）。

**兩條通道在同一條 IO thread 上**：一條連續傳大資料（每則等回答），另一條每 1 ms 送一則 16 bytes、收 8 bytes 的回答，共 2000 則；照核心的複製次數，`SO_SNDBUF` 是預設和 425984 各一組，數字合在一起寫範圍：

| 另一條在傳 | 小訊息 p50 | p90 | p99 | 大資料的速度 |
| --- | --- | --- | --- | --- |
| 沒有 | 26–30 µs | 29–39 µs | 42–65 µs | |
| 1 MiB 內嵌 | 12–21 µs | 28–36 µs | 39–60 µs | 7.5–9.4 GiB/s |
| 1 MiB region | 9–16 µs | 28–30 µs | 40–43 µs | 14–19 GiB/s |
| 8 MiB 內嵌 | 13–112 µs | 126–202 µs | 220–290 µs | 6.0–6.6 GiB/s |
| 8 MiB region | 27–184 µs | 139–313 µs | 330–377 µs | 11.7–15 GiB/s |

- 閒置時比較慢：收的 thread 睡著了，要先叫醒（原型在 macOS 上也是約 24 µs）。
- 接收緩衝區 256 KiB 時，8 MiB 內嵌的 p99 是 242–290 µs，和 64 KiB 的一樣。

**同一台機器上查到的**：

- 預設的 `SO_SNDBUF` 是 212992（kernel 的 `SK_WMEM_DEFAULT`）；不等、每次 4 KiB 寫進去，`EAGAIN` 之前放得下 180224 bytes（每段資料有 kernel 的記帳開銷）。
- 要 212992 拿到 425984、1 MiB 拿到 2 MiB、4 MiB 拿到 8 MiB、16 MiB 也只拿到 8 MiB（這台的 `wmem_max` 是 4 MiB，kernel 6.18 起的預設）。
- `vm.memfd_noexec` 是 0。

**用真的平台層重量**（2026-11-22，同一台機器；`ipc::platform::linux` 寫完之後）：一個暫時的 example（沒有留下），release build。host 是一個 `IoLoop` 加 `ipc::platform::spawn` 啟動的 child，child 也是一個 `IoLoop`；訊息是帶一個 `Bytes` 的 enum，經過 `Link::send`、`codec`、端點。內嵌是把 `region_limit` 設成 0（沒有 region，`codec` 退回放在訊息裡）。`SO_SNDBUF` 和接收緩衝區的大小沒有對外的設定，量的時候暫時用環境變數開口。每組兩次。數字比上面的探測程式高：多了 IO thread 把訊息轉給送的那條 thread 的一次交接，和真的編碼、解碼。

host 送一則給 child、child 回一則小的（中位數；兩次差得多的是上面說的雙峰）：

| 大小 | 內嵌 | region |
| --- | --- | --- |
| 64 KiB | 22 µs | 17 µs |
| 128 KiB | 30–31 µs | 23 µs |
| 256 KiB | 35–52 µs | 23–35 µs |
| 512 KiB | 71–107 µs | 29–48 µs |
| 1 MiB | 139–199 µs | 67–83 µs |
| 8 MiB | 2.34 ms | 0.79–0.83 ms |

- `SO_SNDBUF` 要 212992、1 MiB、4 MiB：內嵌的數字都在預設的那兩次的範圍裡（1 MiB 是 141–215 µs，8 MiB 是 1.9–2.4 ms），沒有趨勢。
- 接收緩衝區 64 KiB：比 256 KiB 慢一點（64 KiB 是 24 對 22 µs，1 MiB 是 229–234 對 139–199 µs）。

兩個 child 在 host 的同一個 `IoLoop` 上，一個連續傳大資料、另一個每 1 ms 一則小的來回（2000 則）：

| 大資料的方向和走法 | 小訊息 p50 | p90 | p99 | 大資料的速度 |
| --- | --- | --- | --- | --- |
| 沒有 | 36–51 µs | 49–65 µs | 74–138 µs | |
| host 送 1 MiB，內嵌 | 34–35 µs | 38–41 µs | 73–83 µs | 4.2–5.2 GiB/s |
| host 送 1 MiB，region | 31–34 µs | 37–40 µs | 44–55 µs | 12 GiB/s |
| host 送 8 MiB，內嵌 | 50–57 µs | 149–270 µs | 0.61–1.15 ms | 3.4 GiB/s |
| host 送 8 MiB，region | 40 µs | 43–47 µs | 58–66 µs | 10 GiB/s |
| child 送 1 MiB，內嵌 | 34–38 µs | 57–70 µs | 88–101 µs | 1.9–2.4 GiB/s |
| child 送 1 MiB，region | 37–39 µs | 58–65 µs | 78–84 µs | 9.9–11 GiB/s |
| child 送 8 MiB，內嵌 | 30–32 µs | 192–250 µs | 389–406 µs | 1.9–2.0 GiB/s |
| child 送 8 MiB，region | 37–39 µs | 366–380 µs | 434–435 µs | 8.5–8.6 GiB/s |

- **host 送大的內嵌訊息時，別的通道等得最久**（p99 到 1 ms）：socket 一次收不下，剩下的排在佇列裡，由 IO thread 在「可寫」時一段一段寫出去，那是 IO thread 的時間。走 region 時 host 只送一則小的通知，別的通道看不出影響。探測程式沒有這一段（它的送是在送的那條 thread 上等）。
- **child 送大資料給 host**（renderer 往 Browser 的方向）：內嵌和 region 讓別的通道多等的差不多（8 MiB 的 p99 約 0.4 ms），region 快約 4 倍。和探測程式量到的一致。

## ring 的設計

訊息的 ring 沒有採用（`README.md` 的第 19 條）；body 的管道也不用 ring（第 21 條）。這一節留給之後要加的時候（`README.md` 的「之後的可能」）。

### shmem-ipc 的 `ringbuf.rs`

2026-10-06 讀的版本：348 行，MIT 或 Apache-2.0，只用 `AtomicUsize` 和指標，不依賴 Linux（整個 crate 只支援 Linux，是因為它的其他部分用 memfd 和 eventfd）。

- 共用記憶體的開頭一個 cache line 放一個 atomic 的計數（ring 裡有幾個項目），後面是資料。
- 送和收的位置各自存在 `Sender` 和 `Receiver` 裡，也就是自己 process 的記憶體，對方碰不到。
- 每次讀計數都檢查它不超過容量，超過就回報損毀。所以對方能做的最多是把計數改成範圍內的錯誤值：讓我們讀到亂碼，或覆蓋還沒讀的資料，不會讓我們讀寫到 ring 以外的地方。
- `send` 和 `recv` 給 callback 原始指標和可用的數量，不給 `&[T]`。
- 送完用 `fetch_add`、收完用 `fetch_sub` 更新計數，回傳的狀態帶一個旗標：計數從 0 變成非 0（或從滿變成不滿）時，表示要喚醒對方。

### machopo 加上的部分

- **訊息的框架**：ring 的項目是位元組；一則訊息是長度、目的地的編號、序號和內容。讀的時候先檢查長度不超過上限、也不超過目前可讀的量，複製到自己的記憶體之後才反序列化。
- **喚醒**：共用記憶體裡加一個「我在睡」的旗標。接收端要睡之前設旗標、SeqCst fence、再檢查一次計數，還是空的才在控制通道上等；送的一端寫完、更新計數、fence 之後讀旗標，對方在睡才在控制通道上送一則喚醒。IO thread 只要等一個東西。
- **序號**：Browser 送的帶 handle 的訊息走控制通道，其他走 ring；每則訊息帶序號，接收端照序號交出去。renderer 送的訊息不帶 handle，全部走 ring，不需要這一段。
- **滿了**：Browser 往 renderer 送時 ring 滿了，先放在 Browser 自己的佇列裡；renderer 往 Browser 送時滿了就等。
- **atomic 的抽象**：計數和旗標的 atomic 型別抽象出來，測試時換成 loom 的版本。
- **只有 Browser 建立 ring 的記憶體**，和其他 region 一樣。

## Seatbelt 的兩種套用時機

sandbox 不在這份 RFC 裡（`README.md` 的「已決定的事項」第 18 條）；這一節是之前查到的，留給之後那一份用，也說明這份的設計假設的是哪一種。

- **暖機之後再鎖**（Chromium 舊的做法）：child 先正常啟動、載入會用到的系統函式庫和資源，再呼叫 `sandbox_init` 鎖住。profile 可以比較緊，但鎖之前跑過的程式碼不在 sandbox 裡，哪些東西要先暖機也難維護。
- **一開始就鎖**（Chromium 現在的做法）：Browser 把 profile 和參數經過 pipe 交給 child，child 的 `main` 在執行其他程式碼之前就套用。profile 要允許之後會用到的東西（例如字型），但所有 renderer 的程式碼都在 sandbox 裡。

`README.md` 的「為 sandbox 留的條件」假設第二種。child 查 rendezvous server 時已經在 sandbox 裡，所以那個名字一定要寫進 profile；它是固定的前綴加 Browser 的 pid，pid 當成參數交給 profile。字型是另一個要讀的東西；之後加上 ICU data 時，profile 也要開放它。這幾件事能不能同時成立還沒驗證（`README.md` 的「待決定」的原型）。

## 參考資料

- Chromium 的 multi-process 架構：https://www.chromium.org/developers/design-documents/multi-process-architecture/
- Chromium 的 macOS sandbox：https://chromium.googlesource.com/chromium/src/+/main/sandbox/mac/README.md
- Chromium 的 sandbox 總覽：https://chromium.googlesource.com/chromium/src/+/main/docs/design/sandbox.md
- Chromium 的 process 的型別：https://chromium.googlesource.com/chromium/src/+/main/content/public/common/process_type.h 、sandbox 的種類：https://chromium.googlesource.com/chromium/src/+/main/sandbox/policy/mojom/sandbox.mojom
- Rule of 2：https://chromium.googlesource.com/chromium/src/+/main/docs/security/rule-of-2.md
- ipcz：https://chromium.googlesource.com/chromium/src/+/main/third_party/ipcz/README.md
- Mojo 的 Rust binding：https://chromium.googlesource.com/chromium/src/+/main/mojo/public/rust/README.md 、https://chromium.googlesource.com/chromium/src/+/main/mojo/public/rust/bindings/README.md （公告：https://groups.google.com/a/chromium.org/g/chromium-dev/c/UR0HRiRxrYI）
- Chromium 的 Linux channel：https://chromium.googlesource.com/chromium/src/+/main/mojo/core/channel_linux.cc 、https://chromium.googlesource.com/chromium/src/+/main/mojo/public/cpp/platform/socket_utils_posix.cc
- Chromium 的 POSIX channel（iOS 上等 ack 才關 fd）：https://chromium.googlesource.com/chromium/src/+/main/mojo/core/channel_posix.cc
- Chromium 的 macOS channel：https://chromium.googlesource.com/chromium/src/+/main/mojo/core/channel_mac.cc
- `MachPortRendezvous`：https://chromium.googlesource.com/chromium/src/+/HEAD/base/apple/mach_port_rendezvous.h 、https://chromium.googlesource.com/chromium/src.git/+/main/base/apple/mach_port_rendezvous_mac.cc
- Chromium 的 Windows channel：https://chromium.googlesource.com/chromium/src/+/main/mojo/core/channel_win.cc 、https://chromium.googlesource.com/chromium/src/+/main/mojo/public/cpp/platform/platform_channel.cc
- Chromium 的共用記憶體：https://chromium.googlesource.com/chromium/src/+/main/base/memory/platform_shared_memory_region_posix.cc 、https://chromium.googlesource.com/chromium/src/+/main/base/memory/platform_shared_memory_region_apple.cc
- `BigBuffer`：https://chromium.googlesource.com/chromium/src/+/main/mojo/public/cpp/base/big_buffer.h
- ipc-channel：https://github.com/servo/ipc-channel （macOS backend：https://github.com/servo/ipc-channel/blob/main/src/platform/macos/mod.rs）
- Servo 的 `generic_channel`：https://github.com/servo/servo/tree/main/components/shared/base/generic_channel
- Servo 啟動 content process 和 sandbox：https://github.com/servo/servo/blob/main/components/constellation/sandboxing.rs
- Ladybird 的 LibIPC：https://github.com/LadybirdBrowser/ladybird/tree/master/Libraries/LibIPC
- Firefox 搬進 `mojo/core/ports`：https://bugzilla.mozilla.org/show_bug.cgi?id=1706374
- WebKit 的 stream connection：https://trac.webkit.org/r273204
- interprocess：https://docs.rs/interprocess/latest/interprocess/
- iceoryx2：https://github.com/eclipse-iceoryx/iceoryx2 （FAQ：https://github.com/eclipse-iceoryx/iceoryx2/blob/main/FAQ.md）
- shmem-ipc：https://github.com/diwic/shmem-ipc
- memmap2：https://github.com/RazrFalcon/memmap2-rs
- rustix：https://docs.rs/rustix/latest/rustix/
- mach2：https://docs.rs/mach2/latest/mach2/
- Apple 的 Technical Q&A QA1541（傳 fd 時互相 ack）：https://developer.apple.com/library/content/qa/qa1541/_index.html
