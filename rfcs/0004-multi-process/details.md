# RFC 0004 的細節：multi-process 和 sandbox

這份是 `README.md` 的補充，放支撐它的細節：其他實作怎麼做、比過的做法的完整理由、平台層、實測。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

其他實作的部分分兩次查：2026-10-06 和 2026-11-17。各節寫明對照的是原始碼、文件，還是只看了搜尋到的資料；沒有查證的寫「就我們的了解」。

## 現在的邊界

現況以 `docs/architecture.md` 的「Process 的邊界」為準：規則、`sanity.py boundary` 檢查什麼、剩下的例外（tile 的交接）和不算例外的東西都寫在那裡，這裡不重複。

**2026-10-04 盤點之後變了的**（排程時要照現在的程式重新看，`README.md` 的「待決定」）：

- 多了分頁：每個分頁一組 renderer 的 thread，一個 `RenderProcessHost`；renderer 的 thread 結束時分頁顯示 sad tab。
- renderer 裡多了 thread：dedicated worker、canvas 的 raster thread、canvas 的 encoder、raster worker。它們都在 renderer 這一側，不跨邊界；但 canvas 的快照和 tile 一樣要放上 GPU。
- raster 從 Skia 換成 vello_cpu，圖片用 `image` 解碼；所有模式都經過 wgpu，tile 一律走 shared image，上傳改成一輪一次。
- `protocol` 多了 DevTools 和自動化的訊息，都能序列化。

**依賴單一 process 的測試**：`browser` 的 `DrivenViz`（測試自己扮演 Viz）、headless、`FakeGpu`。它們直接傳 closure、`Arc` 和 process 內的 channel，所以保留單一 process 的模式，不把它們改成跨 process。

## 要改的地方

| 東西 | 現在 | 改成 |
| --- | --- | --- |
| tile 的 `SharedImage` | compositor 同步呼叫 Viz 建 texture，backing 是 `Arc<dyn Any>` | mailbox 加上 region 的編號；Viz 收到時建 texture、記在 mailbox 表（T1 的話） |
| tile 的生命週期 | 參考計數 | `ReturnResources`（Viz → compositor）和 `DestroySharedImage`（compositor → Viz） |
| 大的位元組資料（`TransferableResource::pixels`、`CommitNavigation` 的 body、子資源的回應） | `Arc<[u8]>`、`Vec<u8>` 放在訊息裡 | `protocol` 的 bytes 型別：單一 process 時照舊，跨 process 時大的放進共用記憶體 |
| `BeginFrame` | Viz 呼叫 Browser 給的 closure，送進 compositor 的 channel | `RenderProcessHost` 讓 closure 寫進控制通道；renderer 不必改 |
| compositor 送給 Viz | 綁著 `FrameSinkId` 的 `CompositorFrameSinkSender` | 經過控制通道到 Browser 的 IO thread，由它補上 `FrameSinkId` 轉給 Viz |
| `RendererFlags` | 啟動 thread 時的參數 | 命令列 |
| script engine | 啟動的一方給的 `ScriptEngineFactory` | child 的 `main` 選 |
| `RendererGone` | renderer 的 thread 結束、channel 關閉 | 控制通道斷了，或 renderer 送了壞訊息 |
| renderer 這一側 | 沒有 IO thread | 新的 IO thread 讀控制通道，放進 main thread 和 compositor 的 task queue |

## 其他實作

| | Chromium | Servo | Ladybird | machopo |
| --- | --- | --- | --- | --- |
| 啟動 | 同一個執行檔，`--type=renderer`；Linux 從 zygote fork | 同一個執行檔，`--content-process` | — | 同一個執行檔，`--type=renderer` |
| IPC | Mojo；core 是 ipcz | ipc-channel；`generic_channel` 依模式選它或 crossbeam | 自己的 LibIPC，訊息用 IDL 產生 | 自己做：控制通道加共用記憶體，第二步加 ring |
| Linux 的通道 | non-blocking 的 Unix socketpair，`SCM_RIGHTS` | Unix socket，`SCM_RIGHTS` | Unix socket，`SCM_RIGHTS` | 同 Chromium |
| macOS 的通道 | Mach port | Mach port（ipc-channel 的第一個 backend） | Mach port（2026-03 從 socket 換過來） | Mach port |
| Windows 的通道 | overlapped 的 named pipe | named pipe | socket | overlapped 的 named pipe |
| macOS 上 child 怎麼連上 | rendezvous：一個 server，名字由 parent 的 pid 決定，靠 audit token 的 pid 發 port | 每個 child 一個帶亂數的名字，放在命令列，不驗證對方 | 查 bootstrap server 上的 port，server 照 pid 發 port | 照 Chromium |
| 共用記憶體 | 唯讀、可寫、雙方可寫三種 region；`BigBuffer` 64 KB 以上自動改走；data pipe | `IpcSharedMemory`，對方拿到的是可寫的；`GenericSharedMemory` 在單一 process 時是 `Arc<Vec<u8>>` | — | 照 Chromium 的三種 region 和 `BigBuffer` |
| 誰建立共用記憶體 | sandbox 不允許的平台由 Browser 代配，其他的 renderer 自己建（後半是就我們的了解） | 送的一端 | — | 只有 Browser |
| handle 的方向 | 雙向，收的一端驗證 | 雙向 | 雙向 | 只從 Browser 往 renderer |
| process 的分法 | site-per-process | 每個 pipeline（大致是每個網站） | — | 每個分頁 |
| raster | OOP-R（GPU process）；早期 one-copy / zero-copy | WebRender 在主 process | — | renderer raster、共用記憶體、Viz 上傳（T1，看法） |
| GPU | GPU process | 主 process | — | Browser process 的 Viz thread |
| macOS sandbox | Seatbelt，child 一開始就套用 | gaol，應該是先連線再套用 | — | Seatbelt，child 一開始就套用 |
| Linux sandbox | namespace + seccomp-bpf | gaol | — | 之後 |
| Windows sandbox | restricted token、job、integrity level | 無 | — | 之後 |
| 字型 | macOS 直接讀；Linux、Windows 有 font proxy | — | — | 直接讀（F1，看法） |
| renderer 當掉 | sad tab | — | — | sad tab |
| 單一 process 的模式 | `--single-process`（只給除錯），訊息照樣經過 Mojo | 預設，不序列化 | — | `--single-process`、headless 和測試的預設，不序列化 |

「—」表示沒查。Servo 的 sandbox 和啟動以外的非 IPC 欄位是 2026-10-04 時的了解，沒有逐一查證。

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

### handle 的方向和誰建立共用記憶體

- **只有 Browser 建立、handle 只往 renderer 走**：選這個。renderer 在訊息裡只寫編號，Browser 查自己的表。三個平台的檢查都一樣，只有「編號在不在表裡、範圍有沒有超出」。
- **renderer 把 handle 附在訊息上送回來**（原本）：Browser 要驗證收到的 handle：Linux 確認是 sealed 的 memfd，Windows 從 renderer 的 process 把 handle 拉過來、檢查型別，macOS 檢查 descriptor。每個 frame 還多一次傳 handle 的系統呼叫。
- **renderer 自己建立**（就我們的了解，Chromium 在 sandbox 允許的平台上這樣做）：省掉向 Browser 要的那一次來回。不選：Browser 得驗證每一塊，renderer 的 sandbox 也要多開權限。

向 Browser 要一塊時要等回覆，是「只有 Browser 建立」的成本，和 handle 的方向無關。

### ring

ringbuf、rtrb、crossbeam 的 `ArrayQueue` 是給同一個 process 的 thread 用的，信任 index；iceoryx2 的 lock-free queue 能放進共用記憶體，但威脅模型是互相合作的 process。shmem-ipc 的 `ringbuf.rs` 是為不可信的對端設計的，只有一個檔案、不依賴 Linux，所以照它的設計自己寫，不依賴那個 crate（它整體只支援 Linux、2022 年之後沒更新）。Chromium 的 `ChannelLinux` 和 WebKit 的 stream connection 是同一個想法的另外兩個實作。

### tile 怎麼交給 Viz（還沒決定）

- **T1. 共用記憶體，Viz 上傳**（one-copy raster；Chromium 在 OOP-R 之前的做法）：renderer 不碰 GPU，sandbox 最緊；raster 的位置不變。
- **T2. 跨 process 的 GPU texture**（macOS 的 `IOSurface`）：renderer 自己上傳，要能存取 GPU，sandbox 得開 IOKit；wgpu 沒有跨 process 共用 texture 的 API，要動到 wgpu 的 Metal 層。
- **T3. OOP-R**：renderer 送 display list，raster 在 Viz（或之後的 GPU process）那一側。Chromium 現在的做法，但 display list、字型和圖片都要能序列化，raster 也要搬家。

### 字型（還沒決定）

- **F1. sandbox 開放系統字型目錄的讀取**：Chromium 在 macOS 上的 renderer 也直接讀系統字型。
- **F2. Browser 提供字型的服務**（Chromium 在 Linux 和 Windows 的 font proxy）：sandbox 更緊，但要做一個 IPC 服務，文字的函式庫要改成從它拿字型。

## 平台層

### Linux

- 建立：rustix 的 `memfd_create`，帶 `MFD_ALLOW_SEALING` 和 `MFD_CLOEXEC`；設好大小之後加 `F_SEAL_SHRINK`、`F_SEAL_GROW`，最後加 `F_SEAL_SEAL`。`MFD_NOEXEC_SEAL` 見 `README.md` 的「待決定」。
- 唯讀：`F_SEAL_FUTURE_WRITE`（Linux 5.1 起）讓已經存在的可寫 mapping 照舊能寫，但之後誰都不能再 map 成可寫，所以 Browser 先 map 好自己的，再交出 fd。
- 只有 memfd 建的檔案能加 seal：一般 tmpfs 的檔案一開始就帶著 `F_SEAL_SEAL`，所以不能改用 `/dev/shm` 底下的暫存檔。
- map：memmap2 的 `MmapRaw`。
- 控制通道：std 的 `UnixStream::pair`，non-blocking；一端給 child 繼承。IO thread 等 socket 可讀、可寫和喚醒。
- 傳 fd：rustix 的 `sendmsg` 和 `SendAncillaryMessage::ScmRights`；收的一端用 `MSG_CMSG_CLOEXEC`，檢查控制資料有沒有被截斷（`MSG_CTRUNC`）。fd 只由 Browser 送給 renderer；Browser 這一端收到 `SCM_RIGHTS` 就是壞訊息。

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

## ring 的設計

### shmem-ipc 的 `ringbuf.rs`

2026-10-06 讀的版本：348 行，MIT 或 Apache-2.0，只用 `AtomicUsize` 和指標，不依賴 Linux（整個 crate 只支援 Linux，是因為它的其他部分用 memfd 和 eventfd）。

- 共用記憶體的開頭一個 cache line 放一個 atomic 的計數（ring 裡有幾個項目），後面是資料。
- 送和收的位置各自存在 `Sender` 和 `Receiver` 裡，也就是自己 process 的記憶體，對方碰不到。
- 每次讀計數都檢查它不超過容量，超過就回報損毀。所以對方能做的最多是把計數改成範圍內的錯誤值：讓我們讀到亂碼，或覆蓋還沒讀的資料，不會讓我們讀寫到 ring 以外的地方。
- `send` 和 `recv` 給 callback 原始指標和可用的數量，不給 `&[T]`。
- 送完用 `fetch_add`、收完用 `fetch_sub` 更新計數，回傳的狀態帶一個旗標：計數從 0 變成非 0（或從滿變成不滿）時，表示要喚醒對方。

### machopo 加上的部分

- **訊息的框架**：ring 的項目是位元組；一則訊息是長度、目的地的標記、序號和內容。讀的時候先檢查長度不超過上限、也不超過目前可讀的量，複製到自己的記憶體之後才反序列化。
- **喚醒**：共用記憶體裡加一個「我在睡」的旗標。接收端要睡之前設旗標、SeqCst fence、再檢查一次計數，還是空的才在控制通道上等；送的一端寫完、更新計數、fence 之後讀旗標，對方在睡才在控制通道上送一則喚醒。IO thread 只要等一個東西。
- **序號**：Browser 送的帶 handle 的訊息走控制通道，其他走 ring；每則訊息帶序號，接收端照序號交出去。renderer 送的訊息不帶 handle，全部走 ring，不需要這一段。
- **滿了**：Browser 往 renderer 送時 ring 滿了，先放在 Browser 自己的佇列裡；renderer 往 Browser 送時滿了就等。
- **atomic 的抽象**：計數和旗標的 atomic 型別抽象出來，測試時換成 loom 的版本。
- **只有 Browser 建立 ring 的記憶體**，和其他 region 一樣。

## Seatbelt 的兩種套用時機

- **暖機之後再鎖**（Chromium 舊的做法）：child 先正常啟動、載入會用到的系統函式庫和資源，再呼叫 `sandbox_init` 鎖住。profile 可以比較緊，但鎖之前跑過的程式碼不在 sandbox 裡，哪些東西要先暖機也難維護。
- **一開始就鎖**（Chromium 現在的做法）：Browser 把 profile 和參數經過 pipe 交給 child，child 的 `main` 在執行其他程式碼之前就套用。profile 要允許之後會用到的東西（例如字型），但所有 renderer 的程式碼都在 sandbox 裡。

`README.md` 照第二種。child 查 rendezvous server 時已經在 sandbox 裡，所以那個名字一定要寫進 profile；它是固定的前綴加 Browser 的 pid，pid 當成參數交給 profile。字型是另一個要讀的東西；之後加上 ICU data 時，profile 也要開放它。這三件事能不能同時成立還沒驗證（`README.md` 的「待決定」的原型）。

## 參考資料

- Chromium 的 multi-process 架構：https://www.chromium.org/developers/design-documents/multi-process-architecture/
- Chromium 的 macOS sandbox：https://chromium.googlesource.com/chromium/src/+/main/sandbox/mac/README.md
- Chromium 的 sandbox 總覽：https://chromium.googlesource.com/chromium/src/+/main/docs/design/sandbox.md
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
