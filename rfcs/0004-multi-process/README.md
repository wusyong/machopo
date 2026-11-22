# RFC 0004：multi-process

- 開始日期：2026-10-04
- 狀態：🚧 已排程：2026-11-20（2026-10-06 延後、只做了第 0 步「整理邊界」；IPC 的設計 2026-11-17 重新比過、整份改寫；2026-11-20 重新盤點（`details.md` 的「現在的邊界」）、做了 macOS 的原型（`prototype/`）、照它的結果定了第 19 到 23 條，從第 1 階段開始做。進度見 `progress.md`）
- 進度：`progress.md`
- 細節：`details.md`

原本是 MVP 選做的 P6（`rfcs/pre-rfcs/MVP.md` 的「里程碑」），2026-10-04 決定 MVP 到 P4 為止，改成這份 RFC。

## 摘要

把 renderer 從 Browser process 裡的 thread 拆成獨立的 process，三個平台都做。Renderer 和 Browser 是同一個執行檔，用 `--type=renderer` 啟動（照 Chromium）。把 renderer 關進作業系統的 sandbox 是之後另一份 RFC；這份的設計照「之後會有 sandbox」來做，但做完時還沒有。

IPC 自己做，照 Chromium 的 Mojo 的方向，但只取需要的部分：

- 每個 renderer 一條控制通道，用 Chromium 在各平台用的 primitive：Linux 的 Unix socket、macOS 的 Mach port、Windows 的 named pipe。
- 訊息照舊是 `protocol` 的 serde enum；大的資料放在共用記憶體。
- 加上 Mojo 不能假設的限制：只有 Browser 建立共用記憶體；renderer 不能送 handle，用編號稱呼它拿到的記憶體。
- 訊息都走控制通道，不另外做共用記憶體裡的 ring（原型量過，第 19 條）；之後要加時不用改訊息的格式和上層。

Viz 留在 Browser process，不做 GPU process。單一 process 的模式留著，給 headless 和大部分的測試用，不付 IPC 的成本。

## 動機

**現況**（2026-11-20，對照 `docs/architecture.md` 的「Process 的邊界」和 `docs/limitations.md` 的「隔離」；每一條路的盤點在 `details.md` 的「現在的邊界」）：

- renderer 是 Browser process 裡的 thread，每個分頁一組。沒有任何隔離：V8 沒開 sandbox，V8 或 DOM 的 heap 的記憶體錯誤能碰到整個 browser 的記憶體。
- renderer 的 thread 結束時分頁顯示 sad tab、可以重新載入（`about:crash` 也在）；但記憶體耗盡、abort、記憶體錯誤這一類結束的是整個程式。
- 邊界大致已經在了：Browser 和 renderer 之間只有 `protocol` 的 message，都能序列化；renderer 不碰檔案系統和網路；compositor 直接把 frame 送給 Viz。規則和檢查（`sanity.py boundary`）是這份的第 0 步做的，例外剩一個（例外 7）。main thread 同步等 Browser 的呼叫（`localStorage`、`document.cookie`）是帶編號的一般訊息，回答放進 scheduler 裡等回答的位置，本來就照 process 的邊界做。

**第 1 階段做完的**（2026-11-21，還是單一 process）：tile 和 canvas 的快照經過訊息交給 Viz（`CreateSharedImage`、`DestroySharedImage`、「像素用完了」，Viz 照 frame sink 和編號記，renderer 不再拿到 Viz 的物件，例外 1 消掉）；訊息裡的位元組都是 `protocol::BigBuffer`；Browser 處理一個 renderer 的訊息的邏輯是一個物件（`IoThreadHost`），IO thread 只讀通道、交給它。

**還沒有的**：

- **導覽的 body 的管道**（例外 7）：`CommitNavigation` 帶著 body 其餘部分的 data pipe 的讀的一端，管道是同一個 process 裡的佇列。
- **IPC 本身**：跨 process 的通道和共用記憶體。
- **啟動**：child process、命令列上的 `RendererFlags`、child 的 `main` 選 script engine。
- **process 當掉**：現在只處理 thread 結束；結束一個 renderer（壞訊息、`kill`）是請它的 thread 自己結束。
- **trace**：`MACHOPO_TRACE` 是整個 process 一份，child 的 span 要另外收。

**這份做完拿到什麼、拿不到什麼**：

- 拿到：一個分頁當掉、吃光記憶體、記憶體出錯，不影響 Browser 和其他分頁；別的分頁的資料不在這個 renderer 的記憶體裡。
- 拿不到：renderer 被攻破之後不能動使用者的電腦。獨立的 process 還是用使用者的身分在跑，照樣能開檔案、連網路；擋住它的是作業系統的 sandbox，那是之後的 RFC。拆 process 是 sandbox 的前提（sandbox 套在整個 process 上，Browser 自己需要檔案、網路和 GPU，不能被關起來），不是它的替代。

**要解決的 `docs/limitations.md` 裡的限制**：「隔離」的第二、三條（renderer 出事時整個程式結束、`gfx::FontData` 和 `gfx::ImageData` 不能序列化）；`docs/architecture.md` 的「Process 的邊界」的例外 1 和 7。「隔離」的第一條（沒有任何隔離）做完時只能改寫，不能刪：renderer 是獨立的 process 了，但 V8 的 sandbox 和作業系統的 sandbox 都還沒有。`rfcs/` 以外的文件不寫這份的編號，做完時照這份清單刪掉或改寫。

目標：

- 每個分頁一個 renderer process，三個平台都是。renderer 當掉或被 kill 時 Browser 不受影響，分頁顯示 sad tab，重新載入時啟動新的 renderer。
- 設計不擋住 sandbox：renderer 向作業系統要的東西越少越好，而且列得出來（「為 sandbox 留的條件」），之後加 sandbox 時 renderer 這一側不用再改。
- IPC 把 renderer 當成不可信的：它送來的訊息和共用記憶體裡的內容，Browser 都驗證過才用；它只能用 Browser 交給它的 handle，碰不到別的 renderer 的通道。
- 單一 process 的模式照樣能用，行為相同，不比現在慢；現有的測試不用改寫成跨 process。
- `protocol` 的 message 型別盡量不變。

不在範圍內：

- GPU process（Viz 搬出 Browser）、Network Service process、utility process。
- Site isolation（依網站分 process、跨網站導航換 process）、多個分頁共用 renderer、跨網站的 iframe。
- OOP-R（raster 搬到 GPU 那一側）。
- 作業系統的 sandbox（macOS、Linux、Windows）和 V8 的 sandbox：之後另一份 RFC（「之後的可能」的第一項）。只做 macOS 的話三個平台不一致，每個平台的機制也值得各自比過選項。
- Mojo 的介面和自動產生的 binding（`.mojom` 的 `Remote` / `Receiver`）：message 照舊是 `protocol` 的 enum。

## 設計概要

給 machopo 的使用者：視窗模式預設每個分頁一個 renderer process，活動監視器裡看得到 `machopo --type=renderer`。renderer 當掉時（不管是 panic、記憶體耗盡還是記憶體錯誤），視窗和其他分頁都還在，那個分頁顯示 sad tab，按重新載入就好。`--single-process` 回到現在的樣子。

給之後改 machopo 的人：

- message 和現在一樣定義在 `protocol`。`RenderProcessHost` 決定它們走 crossbeam（單一 process）還是 `ipc` crate（獨立 process），renderer 這一側的程式相同。
- renderer 不能假設和 Browser 共用記憶體：要給 Browser 或 Viz 的東西只能是 message，或放在 Browser 配給它的共用記憶體裡。
- 從 renderer 來的東西都是不可信的輸入：長度、index、編號先檢查範圍；共用記憶體裡的內容先複製出來再用。
- renderer 要共用記憶體就向 Browser 要，拿到編號和 handle；之後在訊息裡只寫編號。renderer 送的訊息不能帶 handle。

## 詳細設計

### 架構

每個 renderer 和 Browser 之間有一條控制通道（Chromium `mojo::core::Channel` 的角色）。每個 process 有一條 IO thread 讀它的通道：renderer 的讀那一條，Browser 的那一條讀所有 renderer 的（第 42 條）：

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

- **一條通道多工**：四種訊息（`RendererMsg`、`CompositorMsg`、`RendererHostMsg`、`CompositorFrameSinkMsg`）都在同一條控制通道上，各帶一個目的地的編號。每種訊息照送出的順序到，不同種之間的相對順序也保留（相當於 Mojo 的 associated interface 共用一條 pipe）。目的地現在是固定的四個；之後同一條通道上要有更多對象（每個 frame、每個 worker 各一個）時加編號就好，不用為每個對象多開一條系統的通道。
- **兩端的 IO thread**：renderer 多一條，讀控制通道、依目的地的編號放進 main thread 或 compositor thread 的 task queue；main thread 照舊只等它的 scheduler。Browser 整個 process 一條 `Chrome_IOThread`，等所有 renderer 的通道（第 42 條；單一 process 的模式現在是每個 `RenderProcessHost` 一條、等的是 crossbeam，第 3 階段接上 `ipc` 時一起改），`CompositorFrameSinkMsg` 由它轉給 Viz thread，用自己記著的 `FrameSinkId`、不信任訊息裡的。Viz 在 Browser 裡，所以比 Chromium（renderer 直接連到 GPU process）多一次轉手。
- **Browser 對一個 renderer 的訊息做的事是一個物件，不長在 IO thread 的迴圈裡**：它現在做的不只是轉送：子資源的請求直接交給網路、登記 worker 和它們的 blob URL、判斷壞訊息、其餘的轉給 UI thread，而且記著這個 renderer 還在進行的載入。把這些抽成一個只有「收到這則訊息」一個入口的物件，IO thread 只負責從通道讀出訊息、交給它。換通道（crossbeam 換成 `ipc`）時只動外面那一層，裡面的檢查兩種模式跑的是同一份。
- **送的一端**：任何 thread 都能送，不等。送不出去的（對方的 buffer 或佇列滿了、上一筆還沒寫完）放進自己的佇列，一把鎖守著一則訊息的完整和佇列的順序。Browser 不能被 renderer 卡住。
- **crossbeam 留著**：IPC 收到的訊息放進和現在一樣的 crossbeam channel，renderer 和 Browser 的程式不知道訊息從哪裡來。

### `ipc` crate

現在有時鐘（`TimeTicks`）和只在同一個 process 裡的 data pipe；這份把 channel 和共用記憶體放進來。對應 Chromium 的 `base/memory/*shared_memory*` 和 `mojo/core` 的一部分。

裡面的東西（通道、region、編號的表、對收到的訊息的檢查）不分 Browser 和 child，任何一端都用同一套。這份只有 Browser 和 renderer 兩種 process，下面就用它們來講；之後多了別種 process 時（「之後的可能」），它們也要把 renderer 當成不可信的，用的是同樣的元件。

**核心和平台層分開**（第 20 條；驗證靠這個分法，「要保證的性質」和「測試」）：

- **核心**：訊息的框架和序號、目的地的分派、送出的佇列、region 的表和範圍的檢查、對收到的訊息的檢查、data pipe。不碰系統呼叫：它拿到的是「一塊記憶體」（位址和長度）和「位元組」，不知道那塊記憶體是哪個平台怎麼來的。三個平台用同一份；另有一個同一個 process 裡的假通道（兩端在兩條 thread 上，記憶體是一般配置的），讓核心整個在 Miri、loom 和 fuzz 底下跑得起來。核心只依賴 `base`，不知道 `protocol` 的型別（訊息對它是位元組）。
- **平台層**：只做四件事：建通道的兩端和把一端交給 child、送和收帶 handle 的一則位元組、建和 map 共用記憶體、告訴核心對方不在了。裡面沒有判斷：要不要收、合不合法都是核心的事。每個平台一份，薄到可以逐行審。
- `unsafe` 也照這樣分成兩種：核心的是對共用記憶體的指標存取，工具驗得到；平台層的是 FFI，工具驗不到，靠真的跨 process 的測試和審查。

**平台層**（細節見 `details.md` 的「平台層」）：

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| 建立共用記憶體 | rustix 的 `memfd_create`，加上 seal | mach2 的 `mach_make_memory_entry_64`（`MAP_MEM_NAMED_CREATE`） | windows-sys 的 `CreateFileMappingW`（pagefile） |
| map | memmap2 的 `MmapRaw` | mach2 的 `mach_vm_map` | windows-sys 的 `MapViewOfFile` |
| 控制通道 | std 的 `UnixStream::pair`，non-blocking | Mach port：兩端各有自己的 receive port 和通往對方的 send right | windows-sys 的 named pipe（overlapped） |
| 傳 handle | rustix 的 `sendmsg` 和 `ScmRights` | Mach 訊息裡的 port descriptor（memory entry 本身就是 port） | Browser 用 `DuplicateHandle` 複製進 renderer |
| 發現對方不在了 | 讀到 EOF、讀寫錯誤 | kernel 的 dead-name 和 no-senders 通知 | 讀寫失敗、讀到 0 個位元組 |

- **Linux 上 socket 的緩衝區不設**（第 44 條）：Linux 的預設（208 KiB）放得下門檻以下的訊息，設大量不出差別。原本以為要設，是原型在 macOS 上量到的：macOS 的預設只有 8 KiB，16 KiB 來回慢 2.8 倍（`prototype/` 的 README 的「和 ipc-channel、interprocess 比」）；macOS 的控制通道是 Mach 訊息，不受影響。

**共用記憶體**：

- **兩種 region**（第 37 條），語意照 Chromium 的 `ReadOnlySharedMemoryRegion`（建的一端可寫，交出去的只能讀：Browser 送給 renderer 的大資料）和 `UnsafeSharedMemoryRegion`（對方可寫：renderer 的大資料、tile 的像素；讀的一端先複製才用）。Chromium 的第三種 `WritableSharedMemoryRegion`（可寫、能轉成唯讀一次）不做。唯讀的 handle 在 OS 那一層就不能拿來寫（Linux 的 `F_SEAL_FUTURE_WRITE`、macOS 另建一個只有 `VM_PROT_READ` 的 memory entry、Windows 複製 handle 時只給 `FILE_MAP_READ`）。大小建立時決定，不能改。
- **只有 Browser 建立**：renderer 要共用記憶體時向 Browser 要，拿到之後留著重複用。renderer 不需要建立共用記憶體的權限，它也沒辦法交給 Browser 一塊會被縮小的記憶體。
- **一塊 region 只屬於一個 renderer**：renderer 還回來、或它不在了，Browser 就放掉那一塊，不轉給別的 renderer。裡面還留著前一個 renderer 寫的東西（它畫過的像素、它送過的資料），轉手就是跨分頁洩漏。每個 renderer 能要的總量有上限。
- **能不能附 handle 是通道每一端各自的權限，renderer 那一端沒有**：
  - Browser 建一條通道時決定每一端能不能在訊息上附 handle。這份只有 Browser 的那一端能附，所以 handle 只從 Browser 往 renderer 走。
  - 沒有權限的一端送來的訊息附了 handle 就是壞訊息。唯一的例外不在控制通道上：macOS 的 rendezvous 請求照 Mach 的慣例帶一個讓 server 回覆用的 port。
  - 不寫死成「Browser 可以、child 不行」：之後 GPU process 要交出只有它建得出來的緩衝區。不變的是 renderer 永遠不能。
- **用編號稱呼 region**：
  - Browser 建立一塊 region 時給它一個編號（每個 renderer 各自一組），把編號和 handle 一起交給 renderer。作業系統只認 handle，光有編號 map 不了，所以這一次省不掉。
  - renderer map 好、留著，之後直接寫那塊記憶體。它在訊息裡提到這一塊時只寫編號和長度：一塊一次只放一筆資料（第 29 條）。
  - Browser 為每個 renderer 記一張「編號 → region」的表，用自己建立時就有的 handle 和 mapping。要檢查的只有：編號在不在這個 renderer 的表裡、長度有沒有超出那一塊。三個平台都一樣。
- **map 出來的記憶體不轉成 `&[u8]`**：對方隨時可能在寫，拿著 Rust 的參照就違反了 aliasing 的規則。存取一律經過指標：計數用 atomic，內容用指標複製到自己的記憶體之後才用。Viz 上傳 tile 的像素也一樣，不能把共用記憶體直接當成 slice 交給 wgpu。

**控制通道**：

- 一則訊息是長度、目的地的編號、序號、序列化的內容；有權限的一端還可以附帶 handle。序號現在只用來檢查（每一端照送出的順序加一，收的一端看到不連續就是壞訊息）；留著它，是為了之後訊息分兩條路走時（「之後的可能」的 ring）排得回順序，不用改格式。
- 上層只經過一個介面送和收訊息（目的地、內容、handle），不知道底下是哪一種傳輸。
- 三個平台同一個形狀：一個 process 一條 IO thread，等它所有的通道和「喚醒」（第 42 條）；送的時候不等，送不出去的自己排隊。
  - **Linux**：讀寫都不等的 socket（每次呼叫帶 `MSG_DONTWAIT`；原本寫的是把 socket 設成 non-blocking，兩個都做是多餘的），IO thread 用一個 epoll（level-triggered）等所有通道的 socket 和一個喚醒用的 eventfd，每次可讀做一次 `recvmsg`、讀進那條 thread 的 256 KiB 的緩衝區（第 49 條）。寫入被擋時，碰到的那條 thread 自己對 epoll 加上「可寫」，有空間了 IO thread 拿掉（第 48 條）。handle 跟著同一次 `sendmsg` 送出；收的一端處理控制資料被截斷的情況，並且自己數一次讀到幾個 fd（控制資料的緩衝區會進位，放得下的比一則的上限多），收到的 fd 用 `MSG_CMSG_CLOEXEC` 設成 close-on-exec。
  - **macOS**：一則訊息是一則 Mach 訊息（64 KiB 以上的內容走 out-of-line，第 39 條）；每一則都照 audit token 確認是那個 child 送的（第 22 條）。IO thread 阻塞在 `mach_msg`，等一個 port set（每條通道的 receive port 加一個自己的喚醒用的 port）；送的 timeout 是 0，佇列滿了請 kernel 在有空間時通知。在通往對方的 send right 上登記 dead-name、在自己的 receive port 上登記 no-senders，任一個到了就當成斷線；通知只認 kernel 送的（訊息的 audit token），對方可以送一則 id 相同的訊息來假冒。
  - **Windows**：overlapped 的 `ReadFile` / `WriteFile`，寫入一次只有一筆在進行。IO thread 用一個 I/O completion port 等所有通道的完成和喚醒（不用 `WaitForMultipleObjects`：它一次最多等 64 個，一條 thread 等所有 renderer 不夠）；結束時用 `CancelIoEx` 取消進行中的 I/O 再關 handle。Browser 送 handle 時自己用 `DuplicateHandle` 複製進 renderer 的 process，訊息只帶數值。
- 序列化：serde 加 postcard（第 22 條）。postcard 自己沒有巢狀深度的上限：會一層包一層的型別（下一條）的解碼自己數深度。內容裡 region 只是編號和長度，和一般的欄位一樣；Browser 新發出的 region 的 handle 不在內容裡，跟著 `ipc` 自己的一則訊息（「這個編號的 region 給你」）走在用到它的訊息前面（第 30 條）。包在 postcard 外面的一層決定一段位元組放訊息裡還是 region，並數巢狀的深度（第 31 條）。
- 收的一端先檢查長度上限、目的地的編號（不認得的就是壞訊息）和 handle 的數量（對方沒有權限時只能是 0），再反序列化。反序列化時巢狀的深度有上限：訊息裡有可以一層包一層的型別（例如 `protocol::webdriver` 的 `WebDriverValue`，renderer 送的），不設限的話對方送一個巢狀很深的值就能讓收的一端 stack overflow，那不是 panic、接不住，整個 process 結束。不合的就當成對方壞掉，關掉通道（Chromium 的 `ReceivedBadMessage`）。

### 大的資料

- **`protocol` 加一個 bytes 的型別**（`protocol::BigBuffer`，Chromium 的 `mojo_base::BigBuffer`，第 25 條）：單一 process 時是 `Bytes`；跨 process 時 64 KiB 以下放在訊息裡，以上放進共用記憶體（第 22 條）。兩個方向都走留著重複用的 region，不是每筆資料新建一塊（新建、map、第一次碰每一頁的成本比直接放在訊息裡還高）：Browser 送的寫進它給這個 renderer 的唯讀的 region（handle 在那一塊第一次用之前交出去），訊息只帶編號和長度，renderer 複製出來之後回報、Browser 才重複用那一塊；renderer 送的寫進 Browser 之前配給它的 region，訊息一樣只帶編號和長度，Browser 複製出來之後回報。一筆資料用一整塊（第 29 條）；沒有空的那一塊時照樣放在訊息裡。macOS 的平台層對 64 KiB 以上的訊息用 out-of-line 的 descriptor（第 39 條；`BigBuffer` 的大資料還是走 region，會這麼大的訊息是很長的字串，或池子沒有空塊時退回放在訊息裡的）。跨邊界的訊息裡的位元組欄位都用它：子資源的回應、worker 的主 script、導覽的 body 放在訊息裡的那一段、blob 的內容和 request 的 body（renderer 送的）、history 的 state、沒有 GPU 時 frame 的像素。導覽的 body 不用：它只有開頭在訊息裡，其餘走 data pipe（下一節）。
- **從 renderer 來的大資料**：renderer 還能寫那塊記憶體，所以 Browser 一律先複製出來再用（Chromium 的 `BigBuffer` 的註解也警告這個 TOCTOU 的風險）。

### 導覽的 body 的管道

`CommitNavigation` 的 body 其餘的部分是一條 data pipe（`ipc::data_pipe`）。跨 process 時一段一段走控制通道，背壓用訊息（第 21 條）：

| 步驟 | 單一 process | 跨 process |
| --- | --- | --- |
| 網路收到一段 | 寫進管道（Browser 的 network thread） | 一樣，寫進 Browser 這一側的管道 |
| 交給 renderer | renderer 的 main thread 直接讀同一條管道 | Browser 的 IO thread 從那條管道讀出來，包成 `ipc` 自己的訊息（管道的編號、位元組，一則最多 64 KiB；第 32 條）送進控制通道；renderer 的 IO thread 寫進 renderer 自己 process 裡的另一條管道，main thread 讀它。`CommitNavigation` 裡帶的是管道的編號；「開了這條管道」的訊息走在它前面 |
| 背壓 | 管道滿了，network thread 的寫就等 | renderer 讀走的累積到容量的一半時回一則「這條管道我讀了多少」（第 33 條）；Browser 送出去、對方還沒說讀了的量不超過管道的容量（512 KiB），到了就不再從自己的管道讀，那條管道滿了，network thread 的寫照樣等 |
| 結尾和取消 | 寫的一端 drop 是結尾；讀的一端 drop 讓寫的一端失敗 | 各一則訊息：結尾、renderer 不要了 |

- renderer 這一側看到的還是一條 `Consumer`：它由 renderer 的 IO thread 在收到 `CommitNavigation` 時建好、放進訊息，parser 的程式不用改。單一 process 的模式不變。
- renderer 說它讀了的比 Browser 送的多，是壞訊息（checked 的運算）。renderer 的 IO thread 寫進自己的管道時一定有空間，因為 Browser 不會送超過。
- body 和 frame、輸入排同一條控制通道：一段最多 64 KiB，原型量到一則約 10 µs。

### tile 交接

共用記憶體，Viz 上傳（T1，第 23 條；Chromium 在 OOP-R 之前的 one-copy raster）。tile 和 canvas 的快照都走這條路：

- **三則訊息**（第 24 條，Chromium 的 `SharedImageInterface` 的形狀）：
  - `CreateSharedImage`（編號、大小、像素的緩衝區）：raster 畫好一塊就送，不等 frame。編號（mailbox）由 renderer 自己取，Viz 照送的 frame sink 和編號記著 texture。
  - 「緩衝區還你了」：Viz 把像素交給 GPU 之後送回，那一塊回到 renderer 的池子。
  - `DestroySharedImage`（編號）：tile 不要了。Viz 等沒有 frame 在用、而且收到它，才回收 texture。
- frame 的 resource 只帶編號。訊息在同一條通道上照順序到，「建立」一定在用它的 frame 之前，不需要 Chromium 的 sync token。
- **像素只複製一次**（和改之前一樣）：compositor 有一個緩衝區的池子，raster 直接畫進池子的一塊，那一塊的擁有權跟著「建立」交給 Viz，Viz 交給 GPU 的那一次是唯一的複製。單一 process 時一塊是一段共用的記憶體（`Arc`），跨 process 時是通道的一整塊 region（第 61 條；Browser 先給一組、不夠再要，第 62 條），步驟相同；跨 process 時 Viz 從共用記憶體直接複製進 GPU 給的緩衝區，不先複製到自己的記憶體（共用記憶體不能當成 slice 交給 wgpu；第 63 條）。沒有空的 region 時那一塊 tile 畫在 compositor 自己的緩衝區、像素複製進訊息（三次），不等。畫出來是單一顏色的 tile 不上傳，那一塊當場回池子。
- **怎麼守著**（3f 做的；原本寫的是「每複製一次就計一次」的計數，沒有做成那樣：複製發生在三個 crate 裡，沒有一個地方數得到全部）：每一段各有測試確認它那一段沒有複製。compositor 畫的那一塊就是訊息帶的那一塊（`compositor` 的 `a_tile_is_drawn_in_a_region_of_the_channels_when_one_is_free`：池子一塊都沒建）；訊息裡沒有像素、也沒有為它新建 region（`ipc` 的 `a_region_written_in_place_is_what_the_peer_reads_out_of`：送的一端寫出去的不到 1 KiB）；Browser 解出來的是那一塊 region、不是自己的記憶體，renderer process 的每一塊 tile 都是（`browser` 的 `a_renderer_processs_tiles_go_to_the_gpu_in_shared_memory`：放在訊息裡的是 0 塊）；Viz 從 region 讀的那一次寫進 GPU 的緩衝區（`viz` 的 `pixels_in_shared_memory_are_what_is_drawn`）。Viz 那一段「沒有先複製到自己的記憶體」測試看不出來，靠型別（`SharedBuffer` 拿著 region 時不給 slice）和審查，記在 `docs/test-gaps.md`。
- GPU 遺失時 Viz 手上的 texture 都沒了：Browser 照舊送 `ContextLost`，compositor 忘掉所有編號、重新畫、重新送「建立」。
- renderer 碰不到 GPU，之後的 sandbox 可以鎖得最緊。

### 啟動

Browser 建好控制通道的兩端，以 `--type=renderer` 啟動同一個執行檔，把一端交給 child。child 的 `main` 看到 `--type=renderer` 就不開視窗、選好 script engine，從控制通道拿到第一則訊息（renderer 的旗標 `RendererFlags` 和「Viz 有沒有 GPU」，第 57 條），然後跑現在的 `spawn_renderer_main_thread`。之後的 sandbox 套在這一段的最前面，任何其他程式碼之前。

- **Linux 和 Windows**：child 繼承一端。兩端都由 Browser 建立，沒有用名字連線的步驟，別的 process 搶不到通道。
  - Linux：fork 之後、exec 之前，只在 child 裡拿掉那一端的 close-on-exec，號碼放在環境變數裡告訴 child（第 43 條）。
  - Windows：只在啟動 child 的那一小段把那一端設成可繼承（一次啟動一個），handle 的數值放在環境變數裡告訴 child（第 53 條）。
- **macOS**：Mach port 不能繼承，照 Chromium 的 `MachPortRendezvous`：
  - Browser 開一個 rendezvous server，用固定的前綴加上 Browser 的 pid 當名字，登記在 launchd 的 bootstrap namespace；所有 child 共用這一個 server。
  - 啟動 child 時，Browser 把要給它的 port（它的 receive port、通往 Browser 的 send right）記在 child 的 pid 底下。pid 啟動之後才知道，所以 child 的請求比登記早到時，server 等登記完成再回。
  - child 用 `getppid()` 拼出名字、查到 server、送一則請求；server 從訊息的 audit token 讀出 pid，只把記在那個 pid 底下的 port 回給它，回過一次就拿掉。
  - 名字猜得到，同一個使用者的其他 process 也查得到 server，但拿不到 port：server 只認登記過的 pid。

### 為 sandbox 留的條件

sandbox 不在這份裡，但這份的設計要讓它之後鎖得緊：sandbox 得開放 renderer 會用到的每一樣東西，所以 renderer 向作業系統要的越少越好。做完時 renderer process 只需要：

- 讀執行檔和系統的字型。
- 啟動時交到它手上的控制通道，和 Browser 之後經過通道交給它的 handle。
- macOS 上查一次 Mach 的 bootstrap，只查 Browser 的 rendezvous server 那一個名字（Chromium 的 renderer 也是）。

它不需要：寫檔案、連網路、開 GPU、啟動其他 process、建立共用記憶體或任何有名字的 IPC 物件。這些三個平台都一樣。

假設之後的 sandbox 照 Chromium 現在的做法，在 child 的 `main` 一開始、任何其他程式碼之前就套用，不先「暖機」再鎖（`details.md` 的「Seatbelt 的兩種套用時機」）。所以上面每一樣都要在 sandbox 裡面拿得到，不能靠「鎖之前先拿好」。

這些條件驗證到哪裡：

- **IPC 的那一半驗證過了**：原型的 child 一開始就進只開這三樣的 sandbox，rendezvous、控制通道、共用記憶體都可以（`prototype/README.md`）。
- **renderer 自己的那一半還沒有**：原型的 child 很小，讀字型是直接打開一個檔案。machopo 真的 renderer 在 macOS 上經過 CoreText 列出系統的字型（fontique 的 `coretext` backend），CoreText 可能要連系統的字型服務，那就不是開放字型目錄能解決的；V8 的 JIT、wasmtime、elise 的 worker pool 在 `deny default` 底下各自要什麼，也還不知道。
- **第 3 階段加一個只給測試用的探測**（2026-11-20，「測試」和「分階段」）：macOS 上讓真的 renderer process 在原型那份最緊的 profile 底下啟動、載入一個頁面，記下它被擋了什麼。不是做 sandbox：不出貨、不比較做法、只有 macOS。產出是一張「真的 renderer 在 sandbox 裡還需要這幾樣」的清單，是之後那份 RFC 的起點；清單裡有會改這份的設計的東西（例如字型要改由 Browser 交給 renderer，那是新的訊息和一塊共用記憶體）時，在這份裡就處理或記下，不等到下一份才回頭改。

### process 當掉

sad tab、重新載入和 `about:crash` 已經有了，現在由 renderer 的 thread 結束觸發。拆成 process 之後：

- 控制通道斷了（各平台的訊號見「`ipc` crate」的表）時，IO thread 報告 `RendererGone`，Browser 放掉這個 renderer 的 region 和表。之後照現在的路：分頁顯示 sad tab，重新載入時啟動新的 renderer。
- renderer 送來壞掉的訊息也走同一條路，Browser 另外結束那個 process。
- `about:crash` 改成讓 child process 自己結束。

### 單一 process 的模式

`--single-process`，以及 headless 和測試的預設。renderer 的程式不知道自己在哪種模式。訊息直接走 crossbeam，大的資料是 `Arc`：不序列化、不經過 `ipc`，不付 IPC 的成本。IPC 那條路因此不在這個模式的測試裡，由用多個 process 跑的整套測試守著（「測試」）。

renderer 這一側沒有 IO thread：每條 thread 直接等自己的 crossbeam 佇列。Browser 這一側的 `Chrome_IOThread` 在這個模式也在（現在就是這樣），它等的是 crossbeam，推的是同一個處理訊息的物件（「架構」）；要不要連它也拿掉見「待決定」。

### 要保證的性質

`ipc` 要守住的事，一條一條列出來；每一條在「測試」的表裡對到驗證它的方法。完整的論證（假設、不變式、每一處 `unsafe`）寫在 `ipc` 自己的 `soundness.md`（做的時候新增，像 `bindings` 的）。

1. **記憶體**：不管對方在共用記憶體裡寫什麼、在訊息裡給什麼編號和長度，這一端讀寫的都在它自己 map 的那一塊裡面。
2. **壞的輸入**：從通道讀到的任何位元組，結果只有兩種：照規則處理，或當成壞訊息關掉通道。不會 panic、卡住或 stack overflow，用掉的記憶體有上限。
3. **handle**：沒有權限的一端送的訊息，不會讓收的一端留下任何 handle。
4. **region 的歸屬**：一塊 region 只在建它時指定的那個 renderer 的表裡；收回之後不在任何表裡，不會交給別的 renderer。
5. **不被卡住**：送訊息不等對方；對方不讀時，這一端的佇列到上限就關掉通道，不是一直長。
6. **順序**：一條通道上，收的一端交出訊息的順序就是送的一端送的順序，不重複、不漏。
7. **斷線**：對方不在了（結束、當掉、只關掉一半、送了壞訊息）時，這一端剛好報告一次，之後不再交出它的訊息。
8. **同步的呼叫**：回答只到等它的那個呼叫；通道斷了時等的一方醒來、當成失敗。這一條由 renderer 的 scheduler 守，不在 `ipc` 裡（第 27 條）。
9. **data pipe**：讀到的位元組就是寫的、照順序、不重複；Browser 送出去、renderer 還沒說讀了的量不超過管道的容量；renderer 說讀了的比送的多是壞訊息。

平台層守不住的寫成**假設**：kernel 照文件做的那幾件事（把唯讀的 handle map 成可寫會失敗、收到的 handle 是對方真的給的、訊息的來源認得出來、對方不在了一定通知）。每一條假設有一個真的跨 process 的測試，在那個平台上不成立時測試會失敗；原型在 macOS 上試過一遍（`prototype/README.md`）。

### 測試

**每條性質用什麼驗證**（工具照 elise 的用法；`scripts/test.py` 加上跑它們的指令，`sanity.py` 檢查核心沒有碰系統呼叫）：

| 方法 | 守哪幾條 | 怎麼做 | 什麼時候跑 |
| --- | --- | --- | --- |
| 單元測試和惡意的對端 | 全部 | 核心經過假通道測；測試扮演壞掉的 renderer：超過上限的長度、不認得的目的地、不連續的序號、附了 handle 的訊息、不在它的表裡的 region 編號（包括別的 renderer 的）、超出那一塊的範圍、反序列化失敗的內容、巢狀超過上限的值、超過上限的共用記憶體、寫到一半就斷線。Browser 都要關掉通道、報告 `RendererGone`，不能 panic、卡住或 stack overflow | `cargo test`，每次 |
| 平台層的跨 process 測試 | 假設，和 1、3、7 | 測試自己啟動小的 child process：建立、map、傳 handle、唯讀和大小的保證（拿唯讀的 handle 試著寫入或 map 成可寫、試著縮小 region，都要失敗）、對方整個結束、只關掉收的一半、只丟掉送的一半。macOS 另外測 rendezvous（沒登記的 pid 拿不到、同一個 pid 拿不到第二次、請求比登記早到時等得到）和假冒 kernel 的通知（忽略） | `cargo test`；Linux 和 Windows 在每天的 CI，macOS 在本機 |
| fuzz（cargo-fuzz） | 2，和 1、6、9 | 兩個 target：從「通道讀到的位元組」到「Browser 處理完這則訊息」的整條路（反序列化、分派、處理訊息的物件；惡意的 renderer 唯一碰得到 Browser 的路，那個物件不需要真的通道就能跑）；region 的參照 | 改核心時在本機跑一段（CI 不跑，第 55 條） |
| loom、shuttle | 5、6、7、9 | 送出的佇列和它的鎖（好幾條 thread 同時送、對方同時斷線）、body 的訊息和「讀了多少」的往返（兩端各一條 thread）。同步呼叫等回答的位置不在這裡（第 27 條）。atomic 和鎖的型別在核心裡是抽象的，測試時換成 loom 的 | 改到這幾處時在本機（CI 不跑，第 55 條） |
| TLA+ | 4、5、6、7、9 | 三個模型（第 34 條）：通道的一端（有上限的佇列、結束只報告一次）、region 的發出和收回（每個 peer 各自的、peer 不在了之後不轉手）、body 的管道的額度（結尾和取消在路上交錯、不會卡住）；任一端隨時斷線。TLC 檢查模型成立，再拿掉每一部分確認找得到反例 | 改協定時；模型和協定一起改 |
| Verus | 1、4 | 池子的運算寫成純函式，證明：一個長度對到的 region 放得下它、是最小的一種；選出來的那一塊是空的、夠長、最短的，說沒有時真的沒有；總量的檢查不溢位。和 data pipe 的額度的運算（送出的不超過容量、讀了的不超過送的） | 改到這幾個函式時在本機（CI 不跑，第 55 條） |
| Miri | 1 | 核心的測試經過假通道在 Miri 底下跑：碰共用記憶體的 `unsafe` 沒有未定義的行為 | 改核心時在本機（CI 不跑，第 55 條） |
| mutation test | 每一個檢查 | 照 `docs/testing.md`：弄壞每個檢查，確認有測試失敗 | 每個階段 |

**整個 machopo 的**：

- **message 都能序列化**：`protocol` 的 `boundary` 測試在編譯時確認（已經有）；`SharedImage` 的 `#[serde(skip)]` 拿掉之後才算完整。
- **單一 process 的測試照舊**：`DrivenViz`、reftest、`shell` 的端到端測試都不改，證明新的訊息（`ReturnResources`、`DestroySharedImage`）在同一個 process 裡行為一樣。
- **整套的測試用多個 process 跑**：單一 process 的模式不經過 `ipc`，所以 CI 另外用多個 process 跑整套的測試。WPT 經過 machopodriver 啟動真的 `machopo`，讓它開 renderer process 就行；其他幾套見「待決定」。連啟動都算在內。
- **跨 process 的端到端測試**（`cargo test` 裡的少數幾個）：啟動真的 child process，載入一個頁面、點一下、捲一下；`about:crash` 之後 Browser 還在、分頁是 sad tab、重新載入可以恢復。另外確認 Browser 收回的 region 不會出現在別的 renderer 手上。
- **sandbox 的探測**（只有 macOS，只給測試用；「為 sandbox 留的條件」）：一個只有測試會給的旗標讓 renderer process 一開始就進最緊的 profile（查 rendezvous 的名字、讀系統字型的目錄、讀執行檔），載入一個有文字、圖片和 script 的頁面。測試不要求它通過：它把被擋的東西（sandbox 的拒絕紀錄、頁面有沒有畫出來）寫成清單，清單記進 `progress.md`。之後清單變長時（renderer 多要了一樣東西）測試會失敗，要人看過才更新。
- **只能手動確認的**：活動監視器裡的 process，記進 `docs/test-gaps.md` 的「只做過手動確認」。
- **工具碰不到的**：平台層的 FFI 只有跨 process 的測試和審查；kernel 的行為是假設。記進 `docs/test-gaps.md`。

### 分階段

排進某個階段時再確認。

0. **整理邊界**（2026-10-06 做完，`progress.md`）：邊界的規則和檢查；修掉 context lost 的 observer、`RendererFlags` 的函式指標、能送給任何 `FrameSinkId` 的 sender。
1. **message 化，還是單一 process**（2026-11-21 做完，分成 1a 到 1d，`progress.md`）：tile 的交接改成訊息（拿掉 `SharedImage` 的 backing，加上 `ReturnResources` / `DestroySharedImage`、Viz 的 mailbox 表）、`protocol` 的 bytes 型別；Browser 處理一個 renderer 的訊息的邏輯從 `Chrome_IOThread` 的迴圈抽成物件。現有的測試全部照舊通過。
2. **`ipc` crate**：核心（先做，經過假通道就能測）、平台層（Linux 的 socket 的緩衝區和大的資料的門檻，第 44、45 條）、控制通道、帶 handle 的序列化、Browser 配共用記憶體和編號的表；用測試自己的 child process 測，包括惡意的對端。這個階段把驗證的工具架好（fuzz、loom 和 shuttle、Miri、Verus、TLA+ 的模型、`soundness.md`），核心的每一塊做完時它的那幾條性質就驗過。還沒接上 renderer。
3. **啟動 child process**（每個階段的驗收包含它那一塊的驗證；這個階段加的是 body 的管道和處理訊息的物件的 fuzz target；最後做 macOS 上 sandbox 的探測）：`--type=renderer`、renderer 的 IO thread、所有訊息走控制通道、tile 走共用記憶體、導覽的 body 的管道跨 process、process 當掉。最後量捲動時每個 frame 的延遲。

## 缺點

- **IPC 要自己維護**：平台層三套（控制通道、傳 handle、共用記憶體、啟動、發現對方不在了，三個平台各寫一套，不共用）、帶 handle 的序列化，加上對惡意對端的驗證，都是 machopo 的程式。
- **多了 `unsafe`**：`ipc` 的平台層。Linux 大多有 rustix 的安全 API；macOS 上 Mach 沒有安全的包裝，port 的擁有權、訊息的組裝、memory entry 的建立和 map 全是 FFI；Windows 上建立兩端、overlapped 的讀寫和取消、共用記憶體、複製 handle 都是 FFI。Windows 不是平常驗收的平台，這些只有每天的 CI 和審查守著。
- **安全的保證靠設計和驗證**：對惡意 renderer 的防護沒有現成被大量使用的實作可以依靠，靠「要保證的性質」的每一條自己驗證。
- **驗證的工具和工夫**：Miri 和 fuzz 要 nightly，Verus 有自己的 toolchain，TLA+ 要 TLC；machopo 多這幾樣要裝、要在 CI 上跑的東西。Verus 的證明和 TLA+ 的模型要跟著程式改。平台層這些工具都碰不到。
- **兩種模式要一起維護**：平常開發跑的是單一 process，IPC 那條路要等 CI 的多 process 測試才看得到壞掉。
- **renderer 要共用記憶體得問 Browser**：要新的一塊時要等回覆（「待決定」）。
- 每個 renderer 多一份記憶體（V8 isolate、字型、raster 的快取）、一條 IO thread 和啟動時間。
- tile 經過共用記憶體再由 Viz 上傳，compositor 到 Viz 也多一次 IO thread 的轉手；像素的複製還是一次（「tile 交接」）。
- **做完還沒有 sandbox**：這份做完到 sandbox 的 RFC 做完之間，renderer 是獨立的 process 但沒有被關起來。穩定性拿到了，安全上最大的那一塊還沒有（「動機」）。

## 理由和其他做法

每一項比過的選項和完整的理由在 `details.md` 的「比過的做法」；這裡只列結論。

- **IPC 整體**：自己做，照 Mojo 的方向。不選 ipc-channel（每則訊息一次系統呼叫、沒有唯讀的共用記憶體、連線時不驗證對方）、iceoryx2（用全域的名字找服務、假設對端互相合作）、直接用 Mojo（它的 Rust binding 只在 Chromium 的 GN 裡 build，底下是 C++ 的 core；ipcz 不打算在 Chromium 以外使用）、Cap'n Proto RPC（要 schema 和 async executor，不管 handle 和共用記憶體）、照 Mojo 做一整套介面層（現在用不到，之後可以疊上去）。
- **為什麼可以比 Mojo 小**：Mojo 是通用的，任何 process 都能建 handle 和通道、傳給任何人，所以每一端都要驗證所有可能。這份的形狀固定（每個 renderer 一條通道接到 Browser）、只有 Browser 建共用記憶體、renderer 不能送 handle、訊息是 enum，要驗證的少很多。代價是沒有動態的介面和直接的 renderer 對 renderer 的通道（「之後的可能」）。
- **要拆幾種 process**：這份做完是兩種（Browser、renderer），Chromium 是五種型別、其中 utility 再裝十幾個服務。不照它拆那麼多，理由是 Chromium 的 Rule of 2（`details.md` 的「Chromium 的 process」）：一段程式不能同時處理不可信的輸入、用不安全的語言寫、又有高的權限。Chromium 幾乎全是 C++，所以碰不可信的資料的程式都得各自關進 sandbox，而每類工作要的權限不同，就拆成很多個。machopo 的 Browser 這一側是 Rust，在裡面解析網路回應不違反這條規則；違反的是 renderer（V8、DOM 的 GC、wasmtime 的 JIT），這份處理它，和顯示卡的驅動程式（別人的 C，收到的內容受 renderer 影響，現在在 Browser 裡），那是之後的 GPU process。
- **sandbox 為什麼不在這份**：原本這份包含 macOS 的 sandbox（Seatbelt），Linux 和 Windows 的留到之後。改成整個移出去：只做一個平台的話三個平台拿到的東西不一樣；三個平台是三套不同的機制，各自有做法和現成的 crate 要比，值得一份自己的 RFC；V8 的 sandbox 也一起，它不需要 multi-process，可以先做。留在這份裡的是 sandbox 對設計的要求（「為 sandbox 留的條件」）和原型的驗證。
- **單一 process 時訊息怎麼走**：crossbeam、不序列化。不選「只序列化」和「同一個 process 裡走真的控制通道」：都讓單一 process 變慢，後者還多一種模式。
- **macOS 的控制通道**：Mach port。不選 Unix socket 加 `SCM_RIGHTS`（2026-10-06 原本選的）：實測送出後馬上關掉的 socket fd 幾乎全壞（`details.md` 的「macOS 上傳 fd 的實測」）；只帶共用記憶體雖然避得開，之後要傳通道的一端或 `IOSurface` 時還是得換。
- **macOS 上 child 怎麼拿到 port**：照 Chromium 的 rendezvous。不選 Servo 的做法（每個 child 一個帶亂數的名字、放在命令列、不驗證對方）。
- **macOS 的共用記憶體**：Mach 的 memory entry。不選 POSIX shm（2026-10-06 原本選的）：控制通道換成 Mach port 之後，fd 要靠私有的 `fileport_makeport` 才帶得過去。
- **Mach 的 FFI**：mach2（已經在依賴樹裡）。不選自己宣告：Mach 訊息的結構是 4 byte 對齊的，layout 寫錯編譯器不會抓。
- **發現對方不在了（macOS）**：兩種通知都登記。只登記一種會漏掉對方只關一半的一種情況。
- **Windows 的控制通道**：直接用 windows-sys。不選 interprocess（2026-10-06 原本選的）：是新的依賴，這份用的是它比較少人走的路，寫入要另一條 thread。不選 std 的匿名 pipe：讀的時候叫不醒，寫入會卡住。
- **Linux 的 socket**：stream、自己寫長度。不選 `SOCK_SEQPACKET`：訊息的大小受 socket 的 buffer 限制。
- **誰能附 handle**：通道每一端各自的權限，renderer 那一端沒有。不選讓 renderer 把 handle 附在訊息上送回來（2026-10-06 原本的寫法）：Browser 本來就有每一塊，送回來只是多一類要驗證的輸入，三個平台各要一套。也不選寫死成「只有 Browser 能附」：之後別種 process 要交出自己才建得出來的東西時得拆掉重來。
- **誰建立共用記憶體**：只有 Browser。不選讓 renderer 自己建（就我們的了解，Chromium 在 sandbox 允許的平台上這樣做，省掉向 Browser 要的那一次來回）：Browser 得驗證收到的每一塊，renderer 的 sandbox 也要多開權限。
- **tile 怎麼交給 Viz**：共用記憶體、Viz 上傳（T1）。不選 renderer 自己上傳、交出 GPU 的 texture（T2，macOS 的 `IOSurface`）：renderer 要能開 GPU，sandbox 得開 IOKit，wgpu 沒有跨 process 共用 texture 的 API，三個平台三種做法。不選 renderer 只送 display list、raster 在 Viz 那一側（T3，OOP-R，Chromium 現在的做法）：display list、字型和圖片都要能序列化，raster 要搬家；machopo 是 CPU raster，現在換不到東西。
- **process 的分法**：每個分頁一個 process。site isolation 要跨網站導航時換 process，先不做。
- **單一 process 的模式要不要留**：留。Chromium 也有 `--single-process`；machopo 的測試大多靠它（`DrivenViz` 直接扮演 Viz），全部改成跨 process 會又慢又難控制時機。

## 先例

細節見 `details.md` 的「其他實作」和它之後的幾節。

- **Chromium**：Browser、Renderer、GPU、Network 和 utility process；同一個執行檔以 `--type=` 區分；renderer 在各平台的 sandbox 裡；renderer 當掉時顯示 sad tab。IPC 是 Mojo，core 現在是 ipcz。machopo 照它的啟動方式、sandbox 的形狀、各平台的 channel 和共用記憶體的做法、三種 region 和 `BigBuffer`；不做 GPU process 和介面層。
- **Servo**：`--multiprocess` 時每個 pipeline 一個 content process，預設單一 process。IPC 是 ipc-channel，上面的 `generic_channel` 依模式選它或 crossbeam。machopo 照它「單一 process 時不序列化」的做法，不用 ipc-channel。
- **Ladybird、Firefox、WebKit**：都是自己做薄的一層，沒有直接用 Mojo。Firefox 只拿 Mojo 的路由演算法；Ladybird 在 macOS 上 2026-03 從 socket 換成 Mach port；WebKit 給 GPU process 用的是共用記憶體裡的 ring 加 semaphore。
- **shmem-ipc、Chromium 的 `ChannelLinux`**：給不可信的對端用的共用記憶體 ring；這份沒有採用（第 19 條），之後要加時的先例。
- **iceoryx2**：資料留在共用記憶體、只傳位置，是「renderer 用編號稱呼 region」同一個想法；它假設對端合作，這份加上了檢查。

## 已決定的事項

2026-10-06 確認；標了 2026-11-17 的是那天重新比過之後改的或新加的。第 8 到 14 條是延後和第 0 步的決定。

1. **IPC 自己做，照 Chromium 現在的方向**，不用 ipc-channel、iceoryx2，也不直接用 Mojo。表面上是 channel，共用記憶體是一級公民。2026-11-17 重新查過這些選項的現況，結論不變。
2. **平台層的組合**（「`ipc` crate」的表；macOS 的部分 2026-11-17 改）：Linux 用 rustix 的 memfd 和 seal、memmap2；macOS 用 Mach 的 memory entry（原本是 rustix 的 POSIX shm）；Windows 用 windows-sys 的 pagefile section。Mach 的 FFI 用 mach2，版本和 wasmtime 的 fork 用的一樣。不用檔案撐共用記憶體。
3. **控制通道**（macOS 和 Windows 的部分 2026-11-17 改）：Linux 用 std 的 `UnixStream::pair`；macOS 用 Mach port（原本和 Linux 一樣）；Windows 用 windows-sys 的 overlapped named pipe（原本用 interprocess）。兩端都由 Browser 建立。三個平台送的時候都不等、IO thread 都等「通道」和「喚醒」；macOS 上對方不在了靠 dead-name 和 no-senders 兩種通知，只認 kernel 送的。
4. ~~**IPC 分兩步**~~（第二步由第 19 條取消）：第一步所有訊息走控制通道，大的資料放進共用記憶體；第二步把小訊息搬進共用記憶體裡的 ring。第二步要等第一步量過延遲才決定做不做，兩步用同一個平台層。
5. ~~**ring 照 shmem-ipc 的 `ringbuf.rs` 的設計自己寫**~~，不依賴那個 crate（第 19 條：訊息的 ring 不做；之後要做時照這一條）。
6. **process 內的 channel 繼續用 crossbeam；renderer 加一條 IO thread**：IPC 收到的訊息由兩端的 IO thread 放進同一種 crossbeam channel，renderer 的 main thread 照舊只等它的 scheduler。
7. **`ipc` 可以寫 `unsafe`**：只在平台層和共用記憶體的存取，每一處都有 `// SAFETY:`，論證寫在 `ipc` 的 `soundness.md`（第 20 條；原本在它的 README 的「`unsafe` 的論證」，做的時候搬過去，`CLAUDE.md` 和 `docs/README.md` 寫到它的地方跟著改）。2026-11-17 起 macOS 和 Windows 的平台層幾乎全是 FFI，範圍比原本估的大。
8. **延後，預設單一 process**（2026-10-06；2026-11-20 排程，預設哪些模式用多個 process 在第 3 階段定）：拆 process、sandbox、IPC 都延後，只做第 0 步，把邊界守住。不選「繼續做」：先做 DOM API 和 CSSOM 等功能。
9. **邊界的規則和檢查**：規則和例外寫在 `docs/architecture.md` 的「Process 的邊界」，`python3 scripts/sanity.py boundary` 和 `protocol` 的 `boundary` 測試檢查。和 Chromium 用 `DEPS`、`checkdeps` 守住邊界同一類。
10. **第 0 步修掉小的例外，tile 的交接留給第 1 步**：小的修了同一個 process 的行為不變；tile 的交接牽涉共用記憶體的設計。
11. **context lost 由 Browser 轉成訊息**：Browser 向 Viz 登記 observer，收到時送 `CompositorMsg::ContextLost`；renderer 不登記。
12. **`BeginFrame` 的 closure 不算例外**：它由 Browser 建立、只送一則 `CompositorMsg`；拆 process 時由 `RenderProcessHost` 改成寫進控制通道，renderer 不必改。
13. **`RendererFlags` 只放能序列化的值，script engine 另外給**：拆 process 時旗標走命令列，engine 由 child 的 `main` 選。
14. **送給 Viz 的 sender 綁著自己的 `FrameSinkId`**：拆 process 時換成 Browser 的 IO thread 補上 id。
15. **單一 process 的模式不付 IPC 的成本，IPC 由多個 process 的整套測試守著**（2026-11-17）：renderer 是 thread 時訊息直接走 crossbeam，不序列化、不經過 `ipc`。IPC 那條路由 CI 用多個 process 跑整套的測試補上，至少是 WPT。
16. **macOS 上 child 經過 rendezvous 拿到 port，照 Chromium 的 `MachPortRendezvous`**（2026-11-17，「啟動」）。之後 sandbox 的 profile 要開放查這一個名字；原本打算完全不開 bootstrap 的查詢。
17. **renderer 不能送 handle，用編號稱呼 region**（2026-11-17，「`ipc` crate」的共用記憶體）。能不能附 handle 是通道每一端各自的權限，由 Browser 建通道時決定，renderer 那一端永遠沒有；這份只有 Browser 的那一端能附，所以 handle 只從 Browser 往 renderer 走。沒有權限的一端送來的訊息附了 handle 就是壞訊息。
18. **sandbox 移出這份，之後另一份 RFC**（2026-11-20；同一天加上：第 3 階段在 macOS 上用最緊的 profile 探測真的 renderer 還需要什麼，只給測試用，「為 sandbox 留的條件」）：三個平台的作業系統的 sandbox 和 V8 的 sandbox 都不在這份裡；原本包含 macOS 的。不選「這份只做 macOS」：三個平台不一致，每個平台的機制也沒有比過選項。這份保留 sandbox 對設計的要求（「為 sandbox 留的條件」）和原型在 macOS 的 sandbox 裡驗證一次；做完時拿到的是穩定和分頁之間記憶體分開，不是安全上最大的那一塊（「動機」）。

19. **訊息不走共用記憶體裡的 ring，都走控制通道**（2026-11-20，取代第 4 條的第二步和第 5 條）：原本的第二步（每個方向一個 SPSC ring，小訊息不經過系統呼叫）不做。留三個地方讓之後加得上去、不用回頭改：訊息的框架有序號；IO thread 的迴圈不寫死成只有一個來源；上層只經過一個送和收訊息的介面。理由（原型量的，`prototype/README.md`）：ring 只在讀的一端醒著時才快（來回 0.21 µs，Mach 訊息是 4.2 µs），它一睡，喚醒就是一則 Mach 訊息、成本一樣；machopo 照人的節奏操作時，閒置 0 則、60 Hz 捲動每秒 82 到 185 則，讀通道的 thread 醒著的時間不到千分之一，緊接著到的訊息全部改走 ring 每秒省不到 0.4 ms；對方睡著時，同一個 process 裡的 crossbeam 來回也要約 24 µs，時間花在叫醒 thread。對照 Chromium：macOS 上的訊息走 Mach 訊息，沒有 ring；`ChannelLinux` 的 ring 只有 Linux、預設關。不選照原本的寫法接上 machopo 之後再量、再決定：到時有真的捲動的數字，但這一項一直掛著，而原型的數字已經差了三個數量級。不選現在就做：多一整個階段，和 `ipc` 裡最需要驗證的一塊 `unsafe`，換不到看得出來的差別。之後訊息變密時（DevTools 開著、大量 `fetch()` 的頁面、Browser 的 IO thread 合成一條）再量。

20. **`ipc` 留在 machopo 的 `crates/ipc`，照 elise 的做法驗證**（2026-11-20）：`ipc` 分成不碰系統呼叫的核心和只做四件事的平台層（「`ipc` crate」）；要守住的事列成「要保證的性質」，每一條對到方法（「測試」的表）：fuzz、loom 和 shuttle、TLA+、Verus、Miri、真的跨 process 的測試、mutation test；論證寫在 `ipc` 的 `soundness.md`，kernel 的行為寫成假設、每條有測試。工具由 machopo 的 `scripts/test.py` 跑、排進每週的 CI。對照 Chromium：Mojo 在它自己的 tree 裡（`//mojo`、`//third_party/ipcz`）；就我們的了解，`mojo/core` 有 channel 和訊息的 fuzzer，沒有形式化的證明。對照 elise：它是 machopo 旁邊的獨立專案，用的是同一組工具。不選像 elise 那樣放在旁邊的獨立專案：驗證的工具和 CI 不壓在 machopo 上、別的專案也能用，但接上 machopo 的階段 `ipc`、`protocol` 和 `browser` 會一起改，每次都是兩個 repo（先 push 它、再改 machopo 釘的 commit），在還沒定型時很頻繁。核心只依賴 `base`、不知道 `protocol`，之後要抽出去時抽得動。不選只做 fuzz 和惡意對端的測試（原本的寫法）：工夫最少，但漏喚醒、順序和斷線這類問題要靠窮舉才找得到，範圍的檢查值得一個證明。

21. **導覽的 body 的管道一段一段走控制通道，背壓用訊息**（2026-11-20，「導覽的 body 的管道」）：Browser 的 IO thread 把 body 包成訊息送給 renderer，renderer 的 IO thread 寫進它自己 process 裡的管道；renderer 回報讀了多少，Browser 送出而還沒被讀的量不超過管道的容量。`ipc` 裡因此沒有任何共用記憶體裡的 ring。理由和第 19 條同一把尺（原型量的）：一則 64 KiB 的訊息約 10 µs，網路每秒 100 MB 時也只佔控制通道約 1.6% 的時間，輸入事件最多被擋十幾 µs；背壓只是幾則訊息；而 ring 是 `ipc` 裡最需要驗證的一種東西。對照 Chromium：Mojo 的 data pipe 是共用記憶體裡的 ring，`URLLoaderClient` 的 body 都走它。不選每條管道一塊共用記憶體裡的 ring（2026-11-20 起先的建議）：body 不佔控制通道、背壓是 ring 滿了，送 16 MiB 是 0.31 ms（走控制通道是 1.47 ms，每秒 11 GiB，比網路快得多）；但那兩個好處照量到的數字都很小，換來的是兩個 process 同時讀寫一塊記憶體的驗證（loom、fuzz、Verus）。之後量到需要時再換，介面不變（「之後的可能」）。

22. **postcard、每則訊息比對 audit token、大的資料走留著重複用的 region**（2026-11-20，原型量過的三項，`prototype/README.md`）：
    - **序列化用 postcard**：machopo 真的 frame 最多 46 個 quad，編 1.4 µs、解 2.3 µs；編出來的 1150 個 quad 的 frame 也只有編 26 µs、解 49 µs。起草時擔心它把整數一個一個編成不定長度、frame 大時太慢，量出來不成立。postcard 沒有巢狀深度的上限，會一層包一層的型別的解碼自己數深度，fuzz 守著。不選 bincode（就我們的了解 2025 年停止維護；ipc-channel 0.21 也換成 postcard）、Cap'n Proto 和 FlatBuffers（要 schema 和產生的程式，`protocol` 的 enum 要整個改寫）、固定長度的自訂格式（量不到需要）。
    - **macOS 上每則訊息都比對 audit token**：原型每則都比對，成本已經在一則來回 4.2 µs 裡。對照 Chromium：也比對。不選只靠「send right 只交給那個 child」：比對幾乎不花時間，多一層。
    - **大的資料走留著重複用的 region，門檻 64 KiB**：送 1 MiB 給對方、直到它複製出來，留著重複用的 region 是 36 µs，放在訊息裡 105 µs，每次新建一塊 region 120 µs（8 MiB 是 317、1050、956 µs）；64 KiB 時是 6 µs 對 12 µs。門檻照 Chromium 的 `BigBuffer`。不選每筆資料新建一塊 region（起草時「Browser 送的放進唯讀的 region、連 handle 一起交給 renderer」的寫法）：比放在訊息裡還慢。`BigBuffer` 不選 Mach 的 out-of-line 記憶體：8 MiB 時和 region 一樣快，收的一方拿到自己的一份，但只有 macOS 有，三個平台要同一個設計（這一句講的是 `BigBuffer` 怎麼搬；平台層怎麼送一則很大的訊息是另一回事，第 39 條）。

23. **tile 經過共用記憶體交給 Viz，Viz 上傳**（2026-11-20，T1，「tile 交接」）：compositor 照舊用 CPU 畫，像素寫進 Browser 配給它的 region（第 22 條的池子），frame 的 resource 帶 mailbox 和 region 的編號、位置和長度；Viz 複製出來、上傳，生命週期用 `ReturnResources` 和 `DestroySharedImage` 兩則訊息。canvas 的快照一樣。2026-11-20 的盤點確認 renderer 這一側沒有 wgpu，raster 是 vello_cpu 畫的 CPU 像素，所以 raster 的位置不用動。對照 Chromium：OOP-R 之前是這樣（one-copy raster），現在是 OOP-R（T3）。不選 T2（renderer 自己上傳、交出 texture）和 T3（OOP-R），理由在「理由和其他做法」。之後做 GPU raster 時換成 T3：那時過邊界的是 display list 不是像素，這一段本來就要重來（「之後的可能」）。

24. **tile 畫好就送「建立」的訊息，像素只複製一次、有計數守著**（2026-11-20，第 1 階段的 1b，「tile 交接」）：三則訊息（`CreateSharedImage`、緩衝區還回來、`DestroySharedImage`），編號由 renderer 取；raster 直接畫進要交出去的緩衝區，Viz 交給 GPU 是唯一的一次複製，測試用計數確認。對照 Chromium：`SharedImageInterface::CreateSharedImage` 是 client 取 mailbox、送一則訊息給 GPU process，frame 帶 mailbox 和 sync token（不同通道之間排順序用；這裡是同一條通道，不需要），用完 `DestroySharedImage`。不選像素跟著 frame 送（起草時的寫法）：少一種訊息，但上傳全部擠在收到 frame 的那一刻，捲動時新的 tile 多，那個 frame 要等它們都上傳完才能畫。不選先多複製一次、量過再省（2026-11-20 起先的建議）：使用者的意見是這種事之後容易忘記；raster 本來就畫進呼叫的一方給的緩衝區，直接做成一次不比多複製難多少，計數讓它之後不會悄悄變回去。
    - **2026-11-22 的現況**（3c）：跨 process 時像素複製三次，還沒達到這一條。`CreateSharedImage` 的像素是訊息裡的位元組，renderer process 的 compositor 送的時候複製進訊息，Browser 的 IO thread 解碼時複製出來，Viz 交給 GPU 是第三次。功能是通的（測試守著），「只複製一次」和跨 process 的計數由 3f 做（第 56 條的調整）。

25. **bytes 的型別是明確的 `protocol::BigBuffer`，跨邊界的訊息裡的位元組欄位都用它**（2026-11-21，第 1 階段的 1c，「大的資料」）：`BigBuffer` 包一個 `Bytes`，自己實作序列化；單一 process 時就是 `Bytes`，跨 process 時 `ipc` 在編碼期間決定放訊息裡還是放進 region（第 22 條）。
    - **不選「不加型別，`ipc` 的序列化器攔 `serialize_bytes`」**：哪些欄位會走共用記憶體變成隱含的；`Vec<u8>` 在 serde 裡是一個一個 `u8` 的序列，攔不到，之後加的欄位會默默內嵌在訊息裡，沒有東西提醒。Chromium 也是明確的型別（mojom 裡宣告成 `BigBuffer` 的欄位才走共用記憶體）。
    - **裡面是 `Bytes`，不是 `Arc<[u8]>`**：`Bytes` 能不複製地切片，`ResourceResponse` 和 DevTools 的紀錄已經靠它共用。
    - **所有的位元組欄位都換，不只原本列的四種**：request 的 body（fetch 上傳的，大小沒有上限）和 history 的 state（通常小，頁面可以放很大的）也換。規則只有一句、`sanity.py boundary` 機械地擋；不選「小的留 `Vec<u8>`」，那要靠人判斷每個欄位夠不夠小。Chromium 這兩個是內嵌，大的 request body 另外走 blob 或 data pipe；machopo 沒有那幾套機制，走同一個型別。
    - **兩個不換**（`sanity.py` 的允許清單，附理由）：`CreateSharedImage` 的像素走 tile 自己的共用記憶體（第 23、24 條）；`CopyOutputResult` 的像素是 Viz 給 Browser 的，兩者在同一個 process。

26. **一則訊息的開頭固定 20 bytes，長度的上限 128 MiB，照實際收到的量配置**（2026-11-21，第 2 階段的 2a，「控制通道」）：開頭是長度（`u32`，開頭之後的位元組數）、目的地（`u32`）、序號（`u64`，每一端從 0 開始加一，不會繞回來）、handle 的數量（`u32`），little-endian。核心拿到的是「讀到的位元組」，自己切出訊息：socket 是串流，Mach 訊息一則就是一則，走同一段。
    - **上限 128 MiB**：和 Chromium 同一級（舊的 IPC 128 MiB、Mojo 的 channel 256 MiB）。大的位元組資料是 `BigBuffer`、走 region，但字串不是（DevTools 和 WebDriver 的訊息裡 script 的原始碼、執行的結果可以到幾 MiB）。不選 16 MiB：沒有清單保證所有訊息都在它以下，正常的大訊息會被當成壞訊息、分頁掛掉，之後每加一種帶大字串的訊息都要記得這個上限。
    - **不照開頭寫的長度先配置**：位元組到了多少緩衝區才長多少，謊報長度不花收的一端的記憶體；真的送滿時每個 renderer 最多讓 Browser 暫時拿著 128 MiB。上限是一個常數，之後量到訊息都很小，收緊只改它。

27. **同步呼叫等回答的位置留在 renderer 的 scheduler，`ipc` 不做這一塊**（2026-11-21，第 2 階段的 2b）：它已經在 `renderer` 的 `task_queue.rs`（`sync_call`、`post_reply`），和 task queue 共用一把鎖和一個 condvar（回答到的時候 task 照順序留在佇列裡），也已經處理「通道關了，等的一方醒來、當成失敗」。第 3 階段接上時，renderer 的 IO thread 看到通道斷了就呼叫 scheduler 現有的關閉。
    - **不選搬進 `ipc` 的核心**：scheduler 要拆成兩把鎖或把鎖借出去，動到 main thread 的 event loop；renderer process 在 Browser 不在時本來就整個結束，「斷線時醒來」在那一側幾乎用不到。Mojo 的 `[Sync]` 也是在 binding 那一層等，不在 `mojo/core` 的 channel 裡。
    - 性質 8 因此不在 `ipc` 的 loom 模型裡：由 `renderer` 現有的測試和第 3 階段一個跨 process 的測試守（「測試」的表跟著改）。
28. **送出的佇列的上限是 256 MiB，照位元組算**（2026-11-21，第 2 階段的 2b，性質 5）：等著寫出去的位元組（連開頭）超過它，就當成對方不讀了、關掉通道。兩則最大的訊息（第 26 條）的大小：一則合法的大訊息一定排得進去。佇列平常是空的：對方的 IO thread 是獨立的 thread，main thread 忙的時候照樣在讀；body 有自己的背壓（第 21 條）。會堆起來的只有對方整個 process 停住的時候。
    - **不選照「多久沒讀」算**（或兩個都要）：被 debugger 停住的 renderer 會被殺掉；核心要有時鐘，loom 和 fuzz 都變難；多久算壞沒有好的答案。Mojo 的 channel 送出的佇列預設沒有上限，renderer 沒回應是另一層的事（hang monitor）；這裡也留給之後的 hang monitor。
    - 上限是一個常數（`MAX_QUEUED_BYTES`），之後量到平常的佇列有多長，收緊只改它。
29. **池子裡一筆資料用一整塊 region，照大小分級**（2026-11-21，第 2 階段的 2c，「共用記憶體」）：region 的長度是 64 KiB 起每次加倍的那幾種（到 1 GiB），資料放進最短的放得下的空塊，用完整塊回池子。訊息裡提到 region 時只有編號和長度，沒有位置；檢查只有「編號在不在表裡、長度有沒有超過那一塊」。tile 都一樣大，剛好一級；`BigBuffer` 用幾級。
    - **不選少數幾塊大的 region 裡切段**（以頁為單位的 free list，訊息帶編號、位置、長度）：region 少、浪費的位址空間少，但要寫配置器（切段、合併、碎片），「兩段不重疊」要證，Browser 送的一側還要比對 renderer 還回來的是不是發出去的那一段，閒置的記憶體要一頁一頁還給 OS。它省的是 handle 和 map 的次數，那只在每一塊第一次用時付一次。
    - 對照 Chromium：`BigBuffer` 每筆新建一塊、沒有池子（第 22 條量過，比放在訊息裡慢）；軟體合成的 tile 是一個 resource 一塊 region、compositor 留著重複用，是這個做法；大的 region 裡照頁切的只有 discardable memory。
    - 沒有空的那一塊時資料照樣放在訊息裡，不等。池子什麼時候多要一塊、閒置的什麼時候放掉，在 Browser 接上它的時候定（第 3 階段）。
30. **「這一塊可以再用了」是控制通道上的一則訊息，共用記憶體裡沒有雙方都寫的狀態**（2026-11-21，2c）：`ipc` 自己保留一個目的地（`CONTROL_DESTINATION`）。收的一端處理完一批讀到的位元組，把用過的編號合成一則 `Released` 送出；還的不是「發出去、還沒還」的那一塊就是壞訊息。新的 region 也經過這個目的地給（`Granted`：編號、長度、給對方寫的或讀的，handle 跟著這一則），走在用到它的訊息前面，所以訊息的內容裡不會有 handle 的 index，兩個方向的 region 用同一種方式交出去。tile 的那一塊不經過 `Released`：它的生命週期是上層的訊息（第 23 條），池子另有「由拿的人還」的入口。
    - **不選共用記憶體裡的 atomic 旗標**：Browser 送的 region 對 renderer 是唯讀的，旗標要另開一塊雙方可寫的狀態區；renderer 可以隨時亂改它，每一種改法都要論證只傷到它自己，還要 loom 的模型；池子空了時也沒有喚醒。省下的是每批大資料一則小訊息（單程約 2 µs，1 MiB 的資料本身是 36 µs）。
    - 對照 Chromium：`BigBuffer` 沒有這一步（每筆一塊，雙方放掉 handle 時 OS 收回），這一步是「留著重複用」（第 22 條）和「renderer 不能送 handle」（第 17 條）帶來的；tile 的 resource 用訊息還（`ReturnResources`）。共用記憶體裡放 atomic 狀態的只有 data pipe 的環狀緩衝區。
31. **`BigBuffer` 走 region 和巢狀深度的上限，都做在包住 postcard 的一層 serde 的 Serializer / Deserializer 裡**（2026-11-21，2c，`ipc::codec`）：其他的值原樣轉給 postcard；`serialize_bytes` 的一段位元組到 64 KiB 時向池子要一塊，寫成一個兩種 variant 的 enum（在訊息裡的、在 region 裡的編號和長度）。池子和表是明確傳進去的參數，`protocol` 不用改、不用知道 region。解碼的那一層數深度，上限 128 層。攔的是 `serialize_bytes`，不是 `BigBuffer` 這個型別：現在只有它這樣寫（`sanity.py boundary` 守著），之後別的型別這樣寫也走同一條路。
    - **不選 thread-local 的範圍**（`protocol` 開掛勾，`ipc` 編碼期間把池子掛上去）：藏起來的全域狀態，忘了掛就靜靜地全部放在訊息裡；`protocol` 要知道 region；深度的上限還是得另外包一層。
    - **不選每種訊息自己列出它的 `BigBuffer` 欄位**：加欄位時漏列就不走 region。
    - 對照 Chromium：Mojo 產生的序列化程式把訊息的 context 明確傳給每個型別的 traits，`BigBuffer` 的 traits 從它把 handle 放進訊息。
32. **body 的管道的訊息是 `ipc` 自己的**（2026-11-21，第 2 階段的 2d，「導覽的 body 的管道」）：和 region 的兩則訊息同一個保留的目的地，五則：開了一條管道（編號、容量）、一段資料、讀了多少、結尾、不要了。核心裡有管道的兩半（`core::pipe`）：寫的一端記「送出去、對方還沒說讀了的量」，不超過容量；讀的一端記該回報多少。上層看到的還是 `data_pipe` 的 `Producer` 和 `Consumer`，`CommitNavigation` 帶管道的編號。
    - 性質 9 的檢查因此整個在核心裡，Verus、fuzz、TLA+ 對的是同一份程式；之後換成共用記憶體裡的 ring 時上層和 `protocol` 不用動。
    - **不選 `protocol` 的訊息、由 `browser` 和 `renderer` 處理**：規則分散在三個 crate，「renderer 回報超過」的檢查在 `browser` 裡，核心的 fuzz 和模型碰不到；每多一種走管道的資料就要再寫一次。
    - 對照 Chromium：data pipe 是 `mojo/core` 的東西，上層只拿到兩端的 handle。
    - **管道的編號由寫的一端給，只會變大、不重複用**（實作時定的）：給已經結束的管道的訊息（取消時資料還在路上、結束時回報還在路上）丟掉，給從來沒開過的管道的訊息是壞訊息，兩者靠「編號有沒有超過開過的最大的」分。
    - 一則資料最多 64 KiB（第 21 條量的那個大小）。讀的一端手上超過容量是寫的一端的錯（壞訊息）。
33. **讀的一端累積讀了容量的一半才回報**（2026-11-21，2d）：還沒回報的量到 256 KiB（容量 512 KiB）時送一則，帶累積的數字。大約每四則資料一則回報；規則只看數字、不看時間。門檻不超過容量，所以寫的一端額度用完時，讀的一端把手上的讀完一定會回報：這一條由 Verus（門檻不超過容量、每個位元組剛好回報一次）和 TLA+ 的模型（不會卡住）守著。
    - **不選每次讀都回報**：每則資料配一則回報，訊息數加倍。
    - **不選等一段時間或 IO thread 下一次醒來時合併**：要時鐘或多一次喚醒，核心變得不確定。
    - 對照：Mojo 的 data pipe 讀的位置直接寫在共用的狀態裡，沒有這一步；HTTP/2 的流量控制多數實作在視窗用掉一半時送 `WINDOW_UPDATE`。
34. **TLA+ 是三個小模型，不是一個**（2026-11-21，2d，`crates/ipc/tla/`）：`Channel.tla`（性質 5、6、7）、`Regions.tla`（性質 4，和「先告知再提到」「對方讀到才還」）、`Pipe.tla`（性質 9 和不會卡住）。每個模型自己帶著「照順序、隨時可以斷」的通道。
    - **不選一個模型**：狀態數是各部分相乘，只能用很小的參數、每一部分都檢查得淺；region 和管道在核心裡沒有共用的狀態，交集只有同一條通道和同一次斷線。
    - 每個模型照 elise 的做法：先成立，再把每一部分拿掉、確認 TLC 找得到列出來的反例。Mojo 和 ipcz 沒有公開的形式化模型可以對照。
35. **IO thread 的迴圈由 `ipc` 提供**（2026-11-21，第 2 階段的 2e-1，`ipc::endpoint`）：上層建好 thread、呼叫 `Endpoint::run(handler)`，它阻塞到通道結束、回傳理由；`handler` 只有「收到一則訊息」一個入口。迴圈寫在一個平台的介面上（`Platform`：寫、叫醒、等、把 handle 變成記憶體、建 region、關掉），三個平台和兩種 process 共用，用同一個 process 裡的平台（`ipc::local`）在測試、Miri 和 fuzz 底下跑。「讀到就餵、這一批讀過的 region 還回去、建好要給對方的 region、管道有多少空間送多少、寫出等著的」這個順序因此只有一份。
    - **不選 `ipc` 只給零件、`browser` 和 `renderer` 各自寫迴圈**：同一段順序寫兩次，差一步就是訊息卡住或 region 不還，而且只有跨 process 的測試看得到；平台的等待方式漏到上層。
    - 對照 Chromium：`mojo::core::Channel` 把自己登記到 IO thread 的 `MessagePumpForIO` 上，因為它的 IO thread 同時服務所有 renderer 的通道和別的 I/O；machopo 是一條 IO thread 對一條通道。「迴圈不寫死成只等一條來源」（第 19 條）由平台的 `wait` 滿足。
    - 實作時加的：通道結束時端點呼叫平台的 `shut`，把傳輸關掉，對方的平台才知道（只把這一端標成結束的話，還拿著 `Link` 的 thread 會讓傳輸一直開著；兩條 thread 的測試卡住才看出來）。
    - 2e 分成五段（`progress.md`）：端點、macOS 的共用記憶體、macOS 的通道、rendezvous、檢查和文件。端點放最前面：它和平台無關，做完之後每個平台的那一層只剩搬位元組和 map 記憶體。
36. **訊息的型別在 IO thread 上解，由上層指定**（2026-11-21，2e-1）：`handler` 收到的物件有 `destination()` 和 `decode::<T>()`，上層看目的地決定型別，解出來的值再放進目標 thread 的佇列；`decode` 的錯誤傳回去就是壞訊息。訊息裡提到的 region 在這時候複製出來，一批讀完送一則 `Released`，所以表只在 IO thread 上。送的一側對稱：`Link::send(destination, &value)`。
    - **不選端點綁一個訊息型別**：一條通道的訊息要先合成一個大的 enum，目的地的編號就多餘了（它是留給訊息之後分兩條路走的，第 19 條）。
    - **不選 IO thread 只轉送位元組、目標 thread 自己解**（Mojo 的做法）：region 要在解的時候讀，表就得跨 thread 共用，「讀完才還」散到各個 thread。Mojo 做得到是因為 `BigBuffer` 的共用記憶體每筆一塊、跟著訊息走、不用還。
    - **代價**：大資料複製出來的那一次在 IO thread 上（1 MiB 約 18 µs、8 MiB 約 150 µs，第 22 條的數字的一半），排在後面的訊息要等。總時間還是三種做法裡最短的（第 22 條量的就是這個做法）。對輸入延遲的影響沒有量過，記在「待決定」。
    - 管道的讀端怎麼回報不另外定：RFC 的「導覽的 body 的管道」已經寫明 renderer 看到的是同一個 `Consumer`，所以 `Consumer` 多一個「讀了多少」的掛勾，跨 process 時由端點掛上。
37. **共用記憶體只做用得到的兩種**（2026-11-21，第 2 階段的 2e-2，「共用記憶體」；改掉起草時的「三種 region」）：「建的一端可寫、交出去的只能讀」和「對方可寫」，就是端點向平台要的 `create(len, peer_writes)` 的兩個分支。Browser 自己對「對方可寫」的那一塊只 map 成唯讀。
    - **不做 Chromium 的 `WritableSharedMemoryRegion`**（只有一個擁有者、能轉成唯讀一次）：它是給「A 建好交給 B 寫，B 寫完凍結再交給 C」用的；這裡只有 Browser 建 region、renderer 不能送 handle（第 17 條），沒有這種轉手。平台層是工具驗不到、要逐行審的部分（第 20 條），沒有人呼叫的 FFI 是壞了最不容易發現的；三個平台各少一段轉換（Linux 重新開 fd、Windows 複製 handle）。之後需要時是加一個函式，不動現有的。
    - macOS 的實作（`ipc::platform::macos::memory`）：`mach_make_memory_entry_64` 加 `MAP_MEM_NAMED_CREATE` 建；交出去只能讀的那一種，是對 Browser 自己的 mapping 再建一個只有 `VM_PROT_READ` 的 entry（Chromium 的 `ConvertToReadOnly`），能寫的那個 right 不離開建立的函式。map 時最大的保護和現在的一樣，唯讀的 mapping 之後改不成可寫。長度不是整頁的不建（kernel 會進位，那就不是要的那一塊）。
38. **macOS 上先做 rendezvous、再做通道**（2026-11-21，第 2 階段的 2e-3；改掉 2e 原本的順序）：child 繼承不了 Mach port，通道的跨 process 測試要先有 rendezvous 才啟動得了拿著 port 的 child。兩者共用的最底下一層（送和收一則帶 right 的 Mach 訊息、audit token、會自己放掉的 right）放在先做的這一段。
    - **不選照原順序、通道先只在同一個 process 裡測**：通道 commit 的時候，對方整個 process 死掉、audit token 是另一個 process 的、right 真的搬到別的 task，這幾項都還沒驗證過。
    - **不選啟動時換掉 child 的 bootstrap port 把 port 塞進去**：只給測試用的另一條路，而且 child 會查不到系統服務。
    - 對照 Chromium：`MachPortRendezvous` 在 `base/`，`ChannelMac` 建在它上面。
    - rendezvous 實作時定的（第 16 條的細節）：名字是 `org.machopo.MachPortRendezvousServer.<pid>`；比登記早到的問題留著、`register` 時當場回答，server 的 thread 不等任何人（原型是等 500 ms，那樣一個沒登記的 process 就能卡住所有 child）；等著的問題最多 64 個、同一個 process 的新問題擠掉舊的；回答過的 pid 再問直接拒絕；只回答 id 對、沒有內容、沒有 right、有回答用的 port 的訊息。
    - Mach 訊息這一層定的：不送也不收 out-of-line 的記憶體（第 22 條）；排法不對的訊息整則用 `mach_msg_destroy` 毀掉，不先拿一半；一則最多 16 個 right（和框架的 `MAX_HANDLES` 一樣）。
39. **macOS 的平台層對大訊息用 out-of-line，不切**（2026-11-21，第 2 階段的 2e-4，`ipc::platform::macos::channel`；改掉第 22 條的「macOS 上訊息一律內嵌」）：核心的一則訊息是一則 Mach 訊息，64 KiB 以下內嵌、以上 out-of-line（kernel 把送的一方的頁用 copy-on-write map 給收的一方）。這是 Chromium 的 `ChannelMac` 的做法（它的門檻是一個 page）。
    - **量到的**（macOS 26、M4）：一則 Mach 訊息內嵌最多 48 MiB，而核心的訊息最大 128 MiB，所以不用 out-of-line 就得切。切成 16 KiB 到 16 MiB 搬同樣多的資料，每 MiB 都是 80 到 115 µs，切多大幾乎沒差。同一個量法下 out-of-line 對內嵌一則：64 KiB 一樣（23 µs），256 KiB 是 30 對 90 µs，8 MiB 是 584 對 1371 µs。經過通道、不複製（下面）：8 MiB 是 408 µs。
    - **不選切成好幾則內嵌的**（256 KiB 或 1 MiB 一則）：慢 2 到 3 倍，收的一端沒到齊的訊息要先存起來（多一次複製），每條通道的緩衝區也要那麼大。
    - **第 22 條的理由哪裡不夠**：它回答的是「`BigBuffer` 的大資料用什麼搬」（region，三個平台都有），不是「平台層怎麼送一則很大的訊息」。大訊息不走 region 的情況有兩種：很長的字串，和池子沒有空塊時退回放在訊息裡的 `BigBuffer`。平台層怎麼搬位元組核心看不到，Linux 的 socket 和 Windows 的 pipe 本來就是另一種搬法（串流：寫多少算多少），不受這一條影響。
    - **out-of-line 的風險和擋法**：送的一方決定收的一方 map 多少。接收的緩衝區帶一個上限（核心最長的訊息），超過的、兩塊的、內嵌和 out-of-line 同時有的，整則毀掉、當成對方不在了。這些檢查 fuzz 和 Miri 碰不到，靠跨 process 的測試（惡意的 child）。
    - **端點向平台要位元組的介面改成「借」**（`Platform::wait` 收一個 callback）：原本是「放進這個 `Vec`」，out-of-line 的內容就要先複製一次，省掉的那次複製等於沒省。改完 8 MiB 從 622 µs 到 408 µs。
    - **佇列滿了**：不等的送失敗時 kernel 把訊息退回成收到的樣子（right 多一個計數、out-of-line 的記憶體重新 map 一份），實測的；所以 right 用複製的方式送、失敗就 `mach_msg_destroy`，寫的人手上的不動。之後請 kernel 在有空間時通知（`MACH_NOTIFY_SEND_POSSIBLE`，登記時已經有空間就立刻通知，也是實測的）。Chromium 不是這樣做：它把訊息留在送的緩衝區、排一個重試的 task。選通知是因為核心的佇列已經留著那則訊息，平台層不用再留一份，也不用輪詢。
    - **比對整個 audit token**：一開始照 pid（host 知道 child 的 pid，child 知道 parent 的），第一則到了之後記下整個 token，之後要一模一樣（Chromium 記的是握手那一則的）。不合的丟掉、通道不關，和 Chromium 一樣。
    - 對方不在了：dead-name 和 no-senders 都登記，任一個到了就算（第 3 條）。叫醒用另一個 port（放進同一個 port set），不拿著通往自己的 port 的 send right：拿著的話 no-senders 永遠不會來。
40. **Linux 和 Windows 的平台層在那個平台的機器上做和驗**（2026-11-21，第 2 階段的 2e-5）：2f 在真的 Linux 機器上、2g 在真的 Windows 機器上寫和跑，不在 macOS 上盲寫、也不用容器；branch 推上去，在那台機器上接著做。量測（socket 的緩衝區、大資料的門檻）因此是在真的機器上量的。
    - **不選本機用 Docker 跑 Linux**：寫的當下能跑，但那是虛擬機裡的 aarch64，量到的時間不能當成真的機器的。
    - **不選只靠 CI**：每改一次要 push 一次、等一輪，mutation test 和量測做不了。
    - **CI 要不要加 macOS 和 Windows 的 runner，等 Windows 那一層做的時候一起決定**：CI 現在只有 Ubuntu，macOS 那一層的測試一次都不跑，Windows 的之後也一樣。兩個平台是同一個問題。在那之前記在 `docs/test-gaps.md` 的「缺少的工具和檢查」。
41. **host 和 child 各拿到一端，三個平台是同樣的三個函式；跨 process 的測試拆成不分平台的和平台自己的**（2026-11-21，第 2 階段的 2e-6，分頭做 2f、2g 之前）：`ipc::platform` 的 `spawn(&mut Command) -> (Child, Channel)`、`inherited() -> Option<Channel>`、`child_gone(pid)`。`browser` 不用知道 macOS 是先啟動 child 才建通道（要 pid：rendezvous 登記用、比對 audit token 用），而 Linux 和 Windows 是先建好、啟動時繼承。
    - `child_gone` 是 macOS 需要的：child 還沒拿它那一端就結束時，那一端還在 rendezvous 的 server 手上，host 的 no-senders 和 dead-name 都不會來。別的平台 host 在啟動之後就關掉自己手上 child 的那一端，child 結束 kernel 自己會說，那裡它什麼都不做。
    - **`Platform` 的六個函式三個平台都夠用，簽名不用改**（逐個對過，`progress.md` 的 2e-6）。
    - **平台要自己守的一條契約**（寫在 `Platform` 的說明裡）：交給端點的 handle 一定是 kernel 搬進來的，不是對方寫的數值。Windows 的 handle 是送的一方用 `DuplicateHandle` 複製進對方、訊息裡帶數值，所以 child（不能附 handle）可以在訊息裡寫任意數值；host 的平台層照著包成 handle 的話，child 就指定了 host 自己的某個 handle。所以 Windows 的通道建立時要知道對方能不能附，不能的話訊息只要說有 handle 就當成對方不在了，數值不看；要有跨 process 的測試（child 送一個假的數值）。macOS 的 right 和 Linux 的 fd 是 kernel 搬的，對方編不出來。
    - `crates/ipc/tests/it/process_channel.rs` 只用上面三個函式和 `Platform`，Linux 和 Windows 的那一層原樣跑同一份；`crates/ipc/tests/it/mach_channel.rs` 留下只有 macOS 有的（假冒 kernel 的通知、超過上限的 out-of-line、audit token）。
    - 2f 和 2g 各自記決定時的編號：Linux 從第 43 條起，Windows 從第 53 條起（第 42 條是分頭之前最後一條共用的），兩邊都進了 `0004` 之後再順一次。2026-11-22 順過了：Linux 用到第 49 條，Windows 的改成第 50 到 54 條（做的時候是 53 到 57；更早的 commit message 裡寫的是舊的編號）。
42. **一個 process 一條 IO thread，`ipc` 的迴圈管所有通道；「等」是每條 thread 一個的介面**（2026-11-21，第 2 階段的 2e-7，分頭做 2f、2g 之前；改掉第 35 條的一部分）：Browser 的 IO thread 等所有 renderer 的通道，child 的等它那一條。`ipc::endpoint` 的 `IoLoop` 是那條 thread 的迴圈：別的 thread 用 `LoopHandle::serve` 把端點交給它、`stop` 停它；某一條通道結束時告訴那個端點的 `Handler`（`gone`），其他的照常。
    - **平台的介面拆成兩個角色**：通道的（`Platform`，每條一個：寫、map 記憶體、建 region、關掉）和等待的（`Waiter`，每條 thread 一個：把通道登記進來、等所有通道、被叫醒）。等待的對應各平台原生的東西：macOS 是一個 port set，Linux 是 epoll，Windows 是 I/O completion port。接收的緩衝區跟著等待的那一半，整條 thread 一個（Windows 例外，每條通道一塊，第 52 條）。
    - **第 35 條哪些還成立**：「迴圈由 `ipc` 提供、順序只有一份」不變（一條通道做一步還是 `Endpoint::turn`）。「一條 thread 佔住一條通道」（`Endpoint::run`）拿掉了：它的前提是 machopo 一條 IO thread 對一條通道，那不是要的。
    - **不選保留一個介面、`wait` 改成收一組通道**：每次等都要把整組交進去，Linux 和 Windows 上等於每次重建等待的集合，或者在裡面留狀態，那就是拆成兩個只是藏起來。
    - **叫醒怎麼知道是哪一條通道**：callback 由核心的通道自己拿著（`Sender::on_wake`），迴圈在端點加入時裝上：記下那條通道的編號、打斷那條 thread 的等待。平台完全不經手（`Transport` 沒有 `wake`、`Platform` 沒有 `on_wake`），所以每個平台少寫兩個函式，也沒有「平台忘了轉呼叫」這種錯。還沒加入迴圈的通道叫醒沒有作用，加入時迴圈先推它一步，所以加入之前排著的東西不會掉。
    - **不選讓平台拿著 callback**（第一版這樣做）：`Transport::wake` 和 `Platform::on_wake` 在三個平台會是一模一樣的幾行（存一個 callback、呼叫它），而且是核心自己要用的東西繞了平台一圈。
    - **兩種「被叫醒」分開命名**：一條通道被叫醒是 `Event::Woken`（那個端點要推一步）；等待被別的 thread 打斷是 `Waited::Interrupted`、由 `Waiter::interrupter` 給的 callback 觸發（迴圈自己有事：有通道被叫醒、有端點交進來、要停）。平台要實作的只有後者。
    - **對 Windows 的影響**：原本想的「等三個 event」（`WaitForMultipleObjects`）一次最多等 64 個，每條通道三個的話約 21 個 renderer 就滿了；一條 thread 等所有通道要用 completion port。
    - **型別不擋一個 process 開兩個迴圈**：child 開一個、裡面只有一條通道；測試為了扮演兩端會開兩個。「Browser 只開一個」是用法上的規定，寫在 `IoLoop` 的說明裡。
    - **macOS 的一個限制**：比接收緩衝區長的內嵌訊息被 kernel 毀掉時，不知道是哪一條通道的（訊息已經沒了）。只有一條通道時當成對方不在了；現在是略過，那條通道接下來的訊息序號不連續，那時才關掉。會送這種訊息的只有惡意的對端（我們自己超過 64 KiB 的都走 out-of-line）。
    - 對照 Chromium：每個 process 一條 IO thread，`mojo::core::Channel` 把自己登記到它的 `MessagePumpForIO` 上。分法相同；不同的是迴圈在 `ipc` 裡（第 35 條的理由）。
    - 這一段只改 `ipc`。`browser` 現在單一 process 的模式裡每個 `RenderProcessHost` 一條 IO thread，第 3 階段接上時才改。
43. **Linux 的 child 怎麼拿到它那一端：只在 child 裡拿掉那個 fd 的 close-on-exec，號碼放在環境變數**（2026-11-22，第 2 階段的 2f，「啟動」）：`spawn` 用 `UnixStream::pair` 建兩端，在 `Command` 的 `pre_exec` 裡（fork 之後、exec 之前，只在 child 的 process 裡）用 `fcntl` 拿掉 child 那一端的 close-on-exec，號碼寫進一個環境變數；child 起來之後 host 關掉自己手上的那一端。`inherited` 讀環境變數，確認那個 fd 是 `AF_UNIX` 的 stream socket，收下、重新設 close-on-exec；只拿一次，第二次回 `None`（macOS 的 rendezvous 也只給一次）。
    - **號碼不是秘密**：fd 只在擁有它的 process 裡有意義，知道號碼的別的 process 拿不到它。只有這一個 child 拿得到 fd，靠的是 close-on-exec 只在它的 process 裡拿掉；改了環境變數、指向別的 fd 的，頂多是通道上收到不合格式的東西、通道關掉。孫 process 會繼承環境變數，但 fd 已經重新設了 close-on-exec，它拿不到。
    - **號碼不放命令列**：測試的 child 是測試的執行檔本身（libtest），多一個它不認得的參數就壞了。
    - **child 不把變數從自己的環境拿掉**（2026-11-22 審查時定的，使用者選的；Windows 會拿掉，第 53 條）：`std::env::remove_var` 在 Linux 上只有 process 沒有別的 thread 時才安全，`inherited` 保證不了，也不該把這件事變成呼叫的人的責任。孫 process 看得到變數，但那個號碼在它那裡是關著的（上一點），`inherited` 回 `None`。不選由啟動孫 process 的人在 `Command` 上清掉：要靠每一處記得做，而留著沒有害處。
    - **不選當成 child 的 stdin 傳**（起草時的建議；host 和 child 都不用 `unsafe`，std 也照舊走 `posix_spawn`）：在 `inherited` 之前讀 stdin 的程式會吃掉通道的位元組；用 gdb、rr、strace 這類工具包著 renderer 時它們要用 stdin；呼叫的人自己設的 stdin 會被悄悄蓋掉；查問題的人想不到 fd 0 是一條雙向的 socket。
    - **不選 dup2 到另一個號碼**（Chromium 的做法）：dup2 會悄悄關掉目標號碼上原本的 fd（std 在 fork 的路上開一條回報 exec 失敗的 pipe，號碼可能就是那個），exec 失敗時 `spawn` 會回傳成功；來源和目標剛好同一個號碼時 dup2 什麼都不做，close-on-exec 還在，child 拿不到。兩個都補得起來，但補的是用不到的東西：號碼反正要告訴 child，沿用原本的號碼就好。
    - **不選在 Browser 裡、spawn 之前拿掉 close-on-exec**：同時有別的 thread 在啟動 child 的話，fd 會漏給那個 child，它就能讀這個 renderer 和 Browser 之間的通道。
    - **不選 command-fds 這類 crate**：它裡面就是 `pre_exec` 加 dup2，多一個依賴。
    - **代價**：兩個 `unsafe`，都在 `ipc`：`pre_exec`（closure 裡只呼叫 async-signal-safe 的 `fcntl`）和 `OwnedFd::from_raw_fd`（號碼是我們的 `spawn` 給的、確認過是 socket、只收一次）。有 `pre_exec` 時 std 從 `posix_spawn` 改成 fork，啟動時要複製 Browser 的 page table，沒有量。std（`rust-toolchain.toml` 的 1.99.0）沒有穩定的「指定繼承哪個 fd」的 API。
    - **closure 留在 `Command` 裡**：同一個 `Command` 再啟動一次的話，會對那個號碼上當時的 fd 拿掉 close-on-exec。`spawn` 的說明寫明之後不要再用那個 `Command`。
    - 對照 Chromium（`mojo/public/cpp/platform/platform_channel_endpoint.cc`、`base/process/launch_posix.cc`，2026-11-22 讀的 main）：fork 之後照 `fds_to_remap` 把 fd dup2 到從 `base::GlobalDescriptors::kBaseDescriptor` 起第一個沒用到的號碼，號碼放在命令列（`PlatformChannel::kHandleSwitch`）。號碼另外告訴 child 這一點相同；不同的是不 dup2、用環境變數。Chromium 在 Linux 上還有從 zygote fork renderer 的路，machopo 沒有，要不要做是之後 sandbox 的事（那條路怎麼交 fd 沒有讀）。
44. **Linux 上 socket 的緩衝區不設**（2026-11-22，2f，「`ipc` crate」；改掉「socket 的緩衝區要設」）：`SO_SNDBUF` 和 `SO_RCVBUF` 都不設，用 kernel 的預設。量法和數字在 `details.md` 的「Linux 上 socket 的緩衝區和門檻的實測」。
    - **量到的**：`SO_SNDBUF` 從預設（208 KiB）、6.18 之前的 kernel 的上限（約 416 KiB）到 2 MiB、8 MiB，64 KiB、1 MiB、8 MiB 內嵌的時間都落在同一個範圍；照原型的量法（不算核心的複製）設成 1 MiB 反而比不設慢（1 MiB 約 90 對 60 µs）。原型在 macOS 上要調，是因為 macOS 的預設只有 8 KiB。
    - **`SO_RCVBUF` 對 Unix socket 沒有作用**（unix(7)）：送出去的資料算在送的一端的帳上。原本的「`SO_SNDBUF`、`SO_RCVBUF` 設大」這一半本來就不對。
    - **設大也拿不到**：kernel 把 `SO_SNDBUF` 限在 `net.core.wmem_max`，超過不報錯。kernel 6.18 之前預設是 212992（實際最多約 416 KiB），6.18 起改成 4 MiB（commit `a6d4f25888b8`，2025-08-21）；Ubuntu 24.04 的 GA kernel（6.8）在 6.18 之前，CI（`ubuntu-24.04`）應該也是，沒有確認。突破上限要 `SO_SNDBUFFORCE`，需要 `CAP_NET_ADMIN`。
    - **不選設成新舊 kernel 都拿得到的值（約 416 KiB）**：一樣量不出差別，多一個常數。
    - 對照 Chromium：`mojo/public/cpp/platform/platform_channel.cc` 用 `socketpair(AF_UNIX, SOCK_STREAM, 0, …)` 建通道，讀過的 `channel_posix.cc`、`socket_utils_posix.cc` 等五個檔案裡都沒有設緩衝區。
45. **「多大的資料改走 region」三個平台共用 64 KiB**（2026-11-22，2f，「大的資料」；結掉「待決定」裡「要不要每個平台各一個數字」）：用真的平台層量，Linux 上 64 KiB 以上 region 都比較快：64 KiB 是 17 對 22 µs，1 MiB 約 2 倍，8 MiB 約 3 倍。`Platform` 不用加常數。（寫平台層之前用探測程式量到的是「正好在 64 KiB 交叉」；兩種量法的數字都在 `details.md`。）
    - **不選 Linux 提高門檻**（「待決定」原本的猜測）：依據是原型「調過緩衝區的 socket 內嵌 1 MiB 和 region 差不多」，但那量的是純傳輸。核心在內嵌的路上另外複製好幾次（`codec` 編進 payload、`Sender::send` 加上框架再複製一次、收的一端一次沒讀完時存進還沒讀完的那一則、解碼成 `BigBuffer`），region 的路上是寫進 region 和解碼時複製出來各一次；算進去之後內嵌在 128 KiB 以上都比較慢。量測的程式是近似：送的一端少算一次、收的一端對一次讀得完的訊息多算一次，平台層寫完之後重量過，結論相同。
    - **不選 Linux 降低門檻**：64 KiB 時 region 已經快 5 µs，交叉點可能更低，但門檻是 `codec` 裡三個平台共用的常數，真的平台層量不到 64 KiB 以下的 region；探測程式在 32 KiB 量到的差距（約 2 µs）在雜訊範圍裡。小的資料改走 region 的話池子用得多，還多一則「還你了」的訊息。沒有看得出來的好處就不讓平台之間不一樣。
    - **一條 IO thread 管所有通道之後**（第 42 條）：一條通道傳 8 MiB 時，同一條 IO thread 上另一條通道的小訊息 p99 多約 0.2 到 0.35 ms，內嵌和 region 差不多（region 的複製是一整塊、不中斷的，尾巴稍長，但總量快 2 倍多）；1 MiB 的看不出影響。這是「待決定」裡「IO thread 上複製大資料的時間」的第一批數字。用真的平台層重量時多看到一件事：host 送 8 MiB 的內嵌訊息時，socket 收不下的部分是 IO thread 在「可寫」時一段一段寫出去的，別的通道的 p99 到 1 ms；走 region 就沒有這一段。這是大的資料走 region 的另一個理由，也是池子沒有空塊、退回內嵌時的代價。
46. **Linux 的 memfd 先帶 `MFD_NOEXEC_SEAL`，`EINVAL` 就不帶再建一次，不記住**（2026-11-22，2f，「共用記憶體」；2026-11-17 的看法）：帶了之後那塊記憶體不能被當成程式執行，而且改不回來（kernel 加上 `F_SEAL_EXEC`）。否則被攻破的 renderer 可以在「對方可寫」的 region 裡寫一個執行檔、用 `fexecve` 執行，繞過 noexec 的掛載點；之後的 sandbox 會擋 `execve`，這一層不靠它。
    - 旗標 Linux 6.3 起才有，舊的 kernel 回 `EINVAL`；有的系統把 `vm.memfd_noexec` 設成 2，不帶旗標的 `memfd_create` 會失敗。所以先帶、不認得才不帶，兩種系統都建得了。
    - **不選一律帶**：只支援 6.3 以上，Ubuntu 22.04 的 GA kernel（5.15）上建不了任何 region。**不選不帶**：沒有保護，`vm.memfd_noexec=2` 的系統上也建不了。
    - **不記住「kernel 不認得」**：region 只在池子不夠時才建，舊的 kernel 上多一次系統呼叫不算成本，少一個全域的狀態。
    - 退回去的那條路只有 6.3 之前的 kernel 走得到，開發機和 CI 都測不到，實作時記進 `docs/test-gaps.md`。
    - 對照 Chromium：`base/memory/platform_shared_memory_region_posix.cc` 也是先帶、`EINVAL` 就不帶，但記在一個 static 的 atomic 裡，之後都不帶。這個旗標是 Chromium 的人為 ChromeOS 加進 kernel 的（kernel 的 `Documentation/userspace-api/mfd_noexec.rst`）。
47. **Linux 那一層的依賴：rustix 加 `net`、`fs`、`event`，workspace 加 memmap2 0.9**（2026-11-22，2f；照第 2 條）：都已經在依賴樹裡（rustix 1.1.5；memmap2 0.9.11 由 parley 的 fontique 和視窗那一層用），不多 crate。memmap2 的 `map_raw`、`map_raw_read_only` 是安全的 API，unmap 由它的 `Drop` 做。給了長度時 memmap2 不確認檔案夠不夠大，所以 `map` 前自己用 `fstat` 確認：map 一個太短的檔案，碰到超出的部分是 SIGBUS，不是錯誤。
    - **不選只用 rustix、自己 `mmap`**：`mmap`、`munmap` 和保證 unmap 的 `Drop` 都要自己寫成 `unsafe`。**不選 libc**：每個呼叫都是 `unsafe`。**不選 nix**：不在依賴樹裡，和 rustix 重疊。
    - 依賴的改動單獨一個 commit 先進 `0004`（2e-6 的規則：Windows 那一層也改 workspace 的 `Cargo.toml`）。
48. **寫入被擋時，碰到 `EAGAIN` 的那條 thread 自己向 epoll 登記「可寫」**（2026-11-22，2f，「控制通道」）：通道和 waiter 共用一小塊狀態（epoll 的 fd 和這條通道的 key，`add` 時裝上、`remove` 時拿掉）；`write` 碰到 `EAGAIN` 就把登記改成「可讀、可寫」，IO thread 收到「可寫」時改回只有「可讀」，兩者用同一把鎖。epoll 是 level-triggered：加上時已經有空間，會馬上報出來。
    - **跨 thread 改 epoll 是 kernel 明文支援的**（man epoll_wait：一條 thread 在 `epoll_wait` 時，別的 thread 改的登記照樣叫醒它）。萬一漏了也不會卡住：核心在佇列從空變成有東西時叫醒 IO thread（`core::channel` 的 `send`），IO thread 自己再寫一次，還是被擋就自己登記。
    - 和 macOS 那一層同一個形狀：它的 `write` 在當下那條 thread 上請 kernel 有空間時通知。
    - **不選只有 IO thread 碰 epoll**（`write` 只記旗標，waiter 每次等之前登記）：正確性一樣，但 waiter 每次等都要掃所有通道，或另外維護一份「要登記的」清單。**不選一直開著 edge-triggered 的「可寫」**：對方每讀走一次就來一個事件，IO thread 每送一則多醒一次。
    - 對照 Chromium：`mojo/core/channel_posix.cc` 的 `WaitForWriteOnIOThreadNoLock` 在 IO thread 上就登記，不在的話丟一個 task 給 IO thread。那是因為它登記用的 `IOWatcher` 只能在 IO thread 上用，不是 kernel 的限制。
49. **Linux 的接收緩衝區 256 KiB，`epoll_wait` 一次最多拿 16 個事件**（2026-11-22，2f，「控制通道」）：每次 `Waiter::wait` 做一次 `recvmsg`，讀進那條 thread 的緩衝區借給核心（第 42 條）。
    - **256 KiB，不選 64 KiB**：探測程式量到兩者一樣快，真的平台層量到 64 KiB 慢 5% 到 15%；而且門檻邊緣的最大內嵌訊息（64 KiB 加上框架的標頭和編碼）比 64 KiB 大一點，64 KiB 要讀兩次。一次讀 256 KiB 估計是十幾 µs（256 KiB 走 region 的來回，含兩次複製，量到 15–19 µs），不算佔住 IO thread。緩衝區一條 thread 一個，記憶體不是考量。
    - **不選從小開始、照訊息的大小變大**（Chromium 的 `channel.cc` 從 4 KiB 開始）：緩衝區只有一個、借出去用，沒讀完的部分核心本來就自己存，變大換不到東西。
    - **16 個事件**留在 waiter 裡一個一個交出去。原本打算 `remove` 時丟掉屬於那條通道的，沒有做：key 不重用，找不到的本來就略過。它也是「別的 thread 打斷等待，最多晚多久被處理」的上限：16 次 `recvmsg`，估計最多約 0.3 ms，沒有量。
    - 對照 Chromium：`channel_posix.cc` 每次可讀時連續讀，最多 256 KiB（`kMaxBatchReadCapacity`）。

50. **Windows 的平台層分五段，每一段開始前先定那一段的事**（2026-11-22，第 2 階段的 2g）：探測（`prototype-windows/`，2026-11-21 做的，只量 Windows 自己的行為）；2g-1 共用記憶體；2g-2 通道和它的 `Waiter`（同一個 process 裡的測試）；2g-3 `spawn`、`inherited`、`child_gone` 和跨 process 的測試；2g-4 在真的通道上量、文件、CI 的 runner。
    - **2026-11-21 寫過一版，沒有留**：那一版照第 42 條之前的介面寫（通道自己擁有 completion port、自己等），而且共用記憶體和通道裡有好幾處選擇沒有先問過。第 42 條之後通道的擁有權整個不同，所以重寫；每一段要選的事先列出來、確認了才寫。留下來的只有探測和它量到的數字。
51. **Windows 的共用記憶體**（2026-11-22，2g-1；三項都是使用者確認的）：
    - **section 的存取清單誰都不允許，而且不讓擁有者改清單**。問題：交給 renderer 的 handle 只能讀，但 `DuplicateHandle` 要的權限比來源多時，kernel 照 section 的清單檢查；物件的擁有者不看清單就能改清單（`WRITE_DAC`），而同一個使用者的 process 都是擁有者。實測：清單是空的時候，要一個帶 `WRITE_DAC` 的 handle 要得到。所以清單裡放一項：對 owner rights（S-1-3-4）拒絕 `WRITE_DAC`。不選空的清單（Chromium 的做法，`base/memory/platform_shared_memory_region_win.cc`，2026-11-22 的 `main`）：它靠 sandbox 讓 renderer 不是擁有者，這裡還沒有 sandbox。代價是多一段建 SID 和加一項的 FFI。
    - **交出去的 handle 只有 map 的權限，收到的 handle 不檢查是哪一種 section**。問題：對方給的 handle 可能不是這裡建的那種（例如執行檔的 image section）；要檢查就得留著 `SECTION_QUERY`。現在只有 child 收 host 的 handle，host 不收 child 的（第 17 條），送的一方是可信的，所以不檢查、權限也不留：沒有人能觸發的 FFI 是壞了最不容易發現的。Chromium 兩件都做（建立時留 `FILE_MAP_READ | FILE_MAP_WRITE | SECTION_QUERY | READ_CONTROL`，唯讀的是 `FILE_MAP_READ | SECTION_QUERY`；收到時用 `NtQuerySection` 查不是 `SEC_IMAGE`，並試著要 `FILE_MAP_WRITE` 看和模式一不一致）：它的 renderer 可以送 region 給 Browser。**之後開放 child 送 handle 時，這兩個檢查和 `SECTION_QUERY` 要一起加。**
    - **region 的 handle 是 std 的 `OwnedHandle`，不另外包一個型別**。問題：型別上看不出它是 section，任何 handle 都能傳給 `map`。留著通用的型別：傳錯的由 kernel 在 map 時拒絕；通道收到的數值本來就只能「當成」section，包一層也沒有多保證什麼。macOS 的 `SendRight`、Chromium 的 `win::ScopedHandle` 也是通用的。
    - **照已經定的和 macOS 的做法，沒有另外選的**：只做兩種（第 37 條）；長度要是整頁，不是的不建（核心只要 64 KiB 起每次加倍的大小，第 29 條）；`map` 的長度是 0 時不 map（Windows 把 0 當成整個 section）；map 可以少於整塊、不能多；不比對 section 實際的大小（Chromium 收到和 map 的時候也不比）。
    - **建立時就預訂整塊的 commit**（`PAGE_READWRITE`，不加 `SEC_RESERVE`；和 Chromium 一樣）：訂不到時 `create` 沒有結果，核心把資料放在訊息裡。Chromium 這時會重試、並結束別的 process 來騰記憶體，這裡不做。

52. **Windows 的通道和它的 `Waiter`**（2026-11-22，2g-2；十項都是逐項列出問題和選項之後使用者選的，Chromium 的部分對的是當天的 `main`）：
    - **通道交給迴圈之前不寫**。問題：pipe 要到 `Waiter::add` 才在 completion port 上，在那之前沒當場完成的寫入沒有人收得到它的完成，而且探測量到發出它的 thread 一結束 kernel 就取消它。做法：加入之前 `write` 一律回 `WouldBlock`，訊息留在核心的佇列；加入時被拒絕過的那一端會被告知有空間。不選照樣寫（要依賴「關聯之前發出的操作完成時進不進 port」，文件沒有寫）；不選建通道時就給 port（要改三個平台共用的 `Waiter::add`）。Chromium 一樣：`delay_writes_` 到 `StartOnIOThread` 註冊完才清掉。
    - **讀的緩衝區每條通道一塊**（第 42 條的「整條 thread 一個」在 Windows 做不到）。問題：completion port 是先把緩衝區交給 kernel、資料到了才通知，所以每條在等資料的通道都要有一塊在 kernel 手上。不選用長度 0 的讀當通知、再用共用的緩衝區讀：每批資料多一次系統呼叫，切到一半的那半段還是要每條通道自己存。大小先固定（一次寫入的上限，64 KiB），2g-4 量。Chromium 也是每條通道自己的，從 4096 起、不夠就加倍。
    - **進行中的讀寫自己拿著通道的一份 `Arc`，完成的通知被取走時才放掉；迴圈停下來時不收**。問題：通道關掉（`CancelIoEx`、關 pipe）之後，kernel 還會寫那筆操作的 `OVERLAPPED` 和緩衝區，直到它的通知從 port 取走。通道離開迴圈之後遲到的通知照樣被取走、釋放，不報給迴圈。迴圈停下來（waiter 丟掉）時還在進行的不收：那幾塊記憶體不釋放，洩漏是安全的、釋放才不安全；`shut` 一定關掉所有 handle，所以留下的只有記憶體。不選關的時候等通知收完（2026-11-21 那一版，最多 5 秒）：一條 IO thread 上會卡住所有通道。不選停下來時收完再關 port：多一條測試走不到的路（超過時限）和一個沒有依據的數字；正式的用法裡迴圈只在 process 結束時停。Chromium 一樣：`pending_read_`、`pending_write_` 讓物件活到 `OnIOCompleted`，IO thread 結束時不收（「Pending IO will never be dispatched once the pump is gone」）。
    - **child 說它帶了 handle 時由核心當成壞訊息；改掉第 41 條的契約的寫法**。問題：兩種做法都不看數值，差在通道結束的理由。原本寫的是平台層當成對方不在了；改成平台層不交出 handle、核心看到訊息說有 handle 而這一端不准附，照壞訊息關掉（`HandlesNotAllowed`）。這樣理由和另外兩個平台一樣，不分平台的跨 process 測試原樣通過，判斷只有核心那一份。`Platform` 的說明跟著改一句（共用的檔案，單獨一個 commit）。
    - **一次寫入最多 64 KiB（含表頭），pipe 的緩衝區 64 KiB**。問題：沒當場完成的寫入，平台層要自己留一份副本到完成；一次收多少副本就多大，而一則訊息最大 128 MiB。探測量到：緩衝區 4 KiB 到 1 MiB 相差在 15% 以內，一次 64 KiB 比整則快 13% 到 17%。不選整則一次寫（Chromium：緩衝區 4096、整則寫，它的佇列擁有訊息的記憶體所以不用副本）。
    - **當場完成的讀寫也等 port 的通知才算完成**（先寫這一種，2g-4 兩種都量；**量過之後改掉了，第 54 條**）。問題：`WriteFile`、`ReadFile` 當場完成時要不要也等 IO thread 取到通知。等的話只有一條路、一種狀態，代價是每則送出的訊息 IO thread 都要醒一次。另一種（`FILE_SKIP_COMPLETION_PORT_ON_SUCCESS`）是這一種加一條捷徑。Chromium 是這一種。
    - **只有 host 往 child 的方向有這一層的表頭**。問題：pipe 是位元組串流，handle 的數值要跟著某一則訊息的第一段，所以 host 的每次寫入前面有表頭（後面幾個位元組、幾個 handle，各 `u32`；每個 handle 一個 8 位元組的數值）。child 往 host 沒有 handle 會走，所以那個方向沒有表頭，寫的就是核心的位元組：host 不解析 child 的任何平台層位元組，沒有任何一行會把位元組變成 handle，不被信任的輸入全部由核心（有 fuzzer 和模型）處理。不選兩個方向對稱（Chromium 的做法，它的 child 也送 handle）：host 要解析 child 寫的表頭，那一段工具看不到。之後開放 child 送 handle 時這個方向要加格式；那時 host 也不能照數值收，要去 child 的 process 把 handle 拉過來。
    - **pipe 建立時的保護**：照 Chromium 的 `platform_channel.cc`（名字第一次用、只有一個 instance、位元組模式、client 那一端沒有再建 instance 的權限、server 不能冒用 client、建好後確認 client 連著），另外四點不同：加 `PIPE_REJECT_REMOTE_CLIENTS`（Chromium 沒有；多一層）；名字是 pid、process 裡的序號和標準庫的 `RandomState` 給的 64 位元亂數（名字被猜到只會讓我們建不成，搶不到通道）；兩步的失敗都換名字重試，最多 8 次，再不行回 `None`（Chromium 只重試一次、其餘讓 process 當掉）；兩端建立時都不可繼承（Chromium 建立時就可繼承，因為它啟動 process 時用清單指定繼承哪幾個；用標準庫的 `Command` 沒有清單，怎麼交給 child 是 2g-3 的事）。
    - **host 在寫入當下才把 handle 複製進 child，失敗就收回**。順序：有寫入在進行就回 `WouldBlock`、什麼都不複製；否則逐個複製（照原樣的權限、在 child 裡不可繼承）、寫 pipe；寫入發出去之後才關掉自己的那一份；複製到一半或寫入當場失敗時，把已經放進 child 的從 child 裡關掉。host 留的 child 的 process handle 只有 `PROCESS_DUP_HANDLE`，不借用 `Child` 的那一個。child 收到的數值是 0 或 -1 時當成對方不在了。不選複製的同時關掉來源、失敗不收回（Chromium 的 `DUPLICATE_CLOSE_SOURCE`）：寫入失敗時 child 裡留一個它不知道的 handle，而且「不留東西」沒辦法用測試守。
    - **幾個小的**：寫入完成的位元組數不是整段，當成對方不在了（Chromium 一樣）；child 收到不合理的表頭（長度 0、超過一次寫入、handle 超過 16 個）當成對方不在了；讀失敗或讀到 0 個位元組當成對方不在了，不分哪一種錯誤（Chromium 一樣）；打斷等待被呼叫很多次只往 port 放一筆；核心交來 0 個位元組時什麼都不寫。
    - **寫的時候定的細節**：「又有空間」只告訴被拒絕過的那一端（每筆寫入完成都報的話，每則訊息端點都要多推一步）；對方不在了只報一次（讀和寫可能都失敗）；自己關掉的那一端之後什麼都不報；`Channel` 的值丟掉就是關掉（waiter 可能還拿著它的內部狀態）；child 那一端被交了 handle 時放掉它們（核心的約定是寫入收了位元組就收走 handle，而它沒有地方放）。

53. **Windows 上 child 怎麼拿到它那一端**（2026-11-22，2g-3；四項都是逐項列出問題和選項之後使用者選的）：
    - **用標準庫的 `Command`，client 那一端只在啟動的那一小段可繼承，整段包在一把鎖裡**。問題：Windows 讓 child 繼承 handle 有兩種，給清單（只繼承指定的）和不給（這段時間啟動的每個 process 都繼承所有可繼承的）；標準庫只有第二種（給清單的 `spawn_with_attributes` 在 1.99 還是 unstable，對過這台的原始碼）。別的 process 多拿到 client 那一端時，它能讀寫通道，而且真正的 child 當掉時 pipe 不會斷。做法：`spawn` 拿鎖、把 client 那一端設成可繼承、啟動、關掉自己的那一份、放鎖。**留下的缺口**：鎖只管得到經過 `ipc::platform::spawn` 的，Browser 在別處同時啟動別的 process 時那個 process 也會拿到；現在 machopo 的 Browser 沒有在別處啟動 process。記在 `docs/limitations.md`。不選照 Chromium 自己呼叫 `CreateProcessW` 給清單（`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`）：標準庫的 `Child` 不能從 process handle 建出來，第 41 條的簽名要改、另外兩個平台跟著改，還要自己組命令列、環境變數的區塊、標準輸出入；這些本來就是作業系統的 sandbox 那一份要做的（restricted token、job 都要自己呼叫 `CreateProcessW`），那時整個啟動重寫，**做 sandbox 時換成這個**，缺口跟著消失。不選不繼承、child 用名字連上來、host 照 kernel 說的 client 的 pid 確認：通道多一個「等對方連上來」的狀態，child 沒連就結束時 kernel 不說話。
    - **數值放環境變數 `MACHOPO_IPC_CHANNEL`**（十進位）。問題：child 繼承到的 handle 在它那裡是一個數值，啟動時要有人告訴它；管道只有命令列和環境變數。不選命令列的 switch（Chromium 的 `--mojo-platform-channel-handle`）：不分平台的跨 process 測試的 child 是測試的執行檔自己，它的入口是 test harness，不認得的 `--` 參數會讓它在我們的程式跑到之前就結束；`ipc` 也得往呼叫的人的 `Command` 加參數、child 那邊要有人挑出來，`inherited()` 的簽名就不能三個平台一樣。安全性上沒有差別（數值只是 child 的 handle 表裡的編號）。做 sandbox、child 的入口是我們自己的時候可以重看。`inherited` 讀到之後確認那個數值是一個 pipe（`GetFileType`），不是就沒有通道：擋的是變數被別人亂設。`spawn` 啟動完把變數從呼叫的人的 `Command` 拿掉。
    - **child 拿到之後清掉交接留下的三樣東西**：把 handle 設回不可繼承（不然它自己啟動的 process 會拿到通道的一端）；把變數從自己的環境拿掉（不然傳給孫 process）；一個 process 只能拿到一次（另有一個旗標守著，不靠變數有沒有清）。拿掉變數是 `unsafe` 的 `std::env::remove_var`，原本因此規定 `inherited` 要在 process 還沒有別的 thread 時呼叫；2026-11-22 審查時拿掉這條規定：標準庫寫明它在 Windows 上多條 thread 的程式裡也安全，一個安全的函式也不該靠呼叫的人才安全。host 這一側啟動完就關掉自己的 client 那一端（不關的話 child 當掉時 pipe 不會斷）。
    - **`child_gone` 在 Windows 上什麼都不做**。問題：child 還沒拿它那一端就結束時 host 要知道。child 是用繼承拿到的，process 一啟動那一端就在它的 handle 表裡、結束時 kernel 關掉，host 的讀就失敗（探測量過：kill 之後 0.6 到 1.4 ms）。第一點留下的缺口在這裡的樣子是：別的 process 也拿著 client 那一端時 child 死了 host 收不到；和那個缺口一起在做 sandbox 時消失。不選 `child_gone` 主動通知（一張 pid 對通道的全域表、waiter 多一種通知）、不選通道自己等 child 的 process handle（系統 thread pool 的回呼，或 job 接到 port 上）：補的是現在碰不到、之後會消失的缺口。Chromium 的分工相同：`ChannelWin` 只看 pipe，process 的死活由啟動它的那一層看。
    - **共用的測試改了一處**（單獨一個 commit）：`process_channel.rs` 裡亂送的 child 原本先寫、再開始等它那一端；Windows 的通道在有 thread 服務之前不寫（第 52 條），所以改成先等、再寫。另外兩個平台上順序無所謂。

54. **Windows 的通道量過之後定的三件事**（2026-11-22，2g-4；使用者逐項選的。數字在 `progress.md` 的 2g-4）：
    - **當場完成的讀寫就算完成，不經過 completion port**（`FILE_SKIP_COMPLETION_PORT_ON_SUCCESS`；改掉第 52 條的「也等 port 的通知」）。量到：一次送 20000 則小訊息，每則來回從 2.42 µs 到 1.15 µs；四個 child 共用一條 IO thread 同時送，從 3.50 µs 到 0.40 µs；大的資料一樣。原因是原本每一則都要等 IO thread 取到上一則的通知才能寫下一則，IO thread 是瓶頸，通道越多越明顯，而 Browser 正是一條 IO thread 服務所有 renderer（第 42 條）。**一來一回可能慢約 3 µs**（原本 10.6 到 11.5，現在 13.4 到 14.2；另一組走同一段程式的量到 11.8 到 14.1，所以不確定是不是真的；可能的原因是原本每筆寫入多的那一次通知剛好讓 IO thread 在回覆到的時候還醒著）。使用者在知道這一點之後維持這個選擇：machopo 平常的訊息稀疏，這一點每秒不到 1 ms，而另一種的弱點隨負載變大。
    - **這樣做多出來的三件事**：每筆操作分「當場完成（沒有通知，不拿通道的計數）」和「進行中（有通知，拿著）」；pipe 加入迴圈時先設好這個模式才讀寫，設不成就加不進去（沒設而照常跑的話，當場完成的操作會既當場結算、又收到通知，計數多還一次）；IO thread 自己守公平，因為資料一直當場讀得到時同一條通道可以一直讀：借出一筆之後下一輪換別條有東西的通道，連續 8 輪之後一定看一次 port（只看、不等）。對另外兩個平台沒有影響，它們本來就是「有空間就當場寫完」。Chromium 不設這個模式。
    - **讀的緩衝區維持每條通道固定 64 KiB**。量到：改成 128 KiB 只有 48 KiB 放在訊息裡的那一種快 22%（剛好比一次寫入多一點、被切成兩段的），其餘沒有差別；每條通道多 64 KiB 不值得。不選 Chromium 的從 4096 開始、不夠就加倍：固定 4096 在這裡不能用（child 那一端要整筆到齊才借出去），會長大的那一種只省記憶體（只收小訊息的通道一條省 60 KiB，一百個 renderer 6 MiB），多約四十行；量到每條通道的記憶體是問題時再換，介面不變。
    - **「多大的資料改走 region」在 Windows 上留在 64 KiB**：真的通道上 64 KiB 走 region 來回 28 到 33 µs，48 KiB 放在訊息裡 46 µs，1 MiB 是 650 對 1340 µs。
    - **CI 先不加 macOS、Windows 的 runner**（第 40 條留下來的）：這兩個平台的那一層還是靠改到的人在那台機器上跑 `cargo test -p ipc`。不選加在每天或每週的 CI。留下的缺口記在 `docs/test-gaps.md`：共用的東西改壞另一個平台時不會自動知道，那幾個實測來的行為在別的系統版本上成不成立也不知道。
55. **`test.py ipc` 的五項不排進 CI，留在本機跑**（2026-11-22，第 2 階段的 2h；使用者定的；改掉第 2 階段的規劃和「測試」的表原本寫的「每週的 CI」）：`fuzz`、`miri`、`models`、`tla`、`verus` 在改到 `ipc` 的核心時由改的人在本機跑，結果記在 `progress.md`。
    - **為什麼**：預計之後把 `ipc` 分成獨立的專案（像 elise），CI 那時在它自己的 repo 裡一起做。現在在 machopo 的 CI 裝五種工具（cargo-fuzz、Miri 的 nightly、Verus、TLC），分出去時要再做一次。
    - **不選的**：新開一個每週的 workflow（五個 job，fuzz 每個 target 30 分鐘）；加進每週跑 GC 的那個 workflow；快的三項（`models`、`tla`、`verus`）每天、慢的每週。
    - **留下的缺口**：核心改了而沒有人在本機跑這五項時，沒有東西會發現。記在 `docs/test-gaps.md`。`cargo test -p ipc`（包括 fuzzer 留下的回歸輸入）照樣由每天的 CI 在 Linux 上跑。
    - 對照 Chromium：fuzzer 是持續在跑的服務（ClusterFuzz），不是排程的 CI。
56. **第 3 階段分成七段，先打通最細的一條路**（2026-11-22，第 3 階段開始前；使用者確認的）：
    - 3a：child 起得來（`--type=renderer`、child 的 IO thread、Browser 用 `ipc::platform::spawn` 啟動、四種訊息走控制通道）；做完 `about:blank` 在另一個 process 裡載入。
    - 3b：導覽的 body 的管道跨 process（邊界上最後一個例外）。
    - 3c：tile 走共用記憶體。
    - 3d：process 不在了（當掉、被 kill、壞訊息、`about:crash`），接到現有的 sad tab。
    - 3e：整套現有的測試哪些用多個 process 再跑；`IoThreadHost` 和 body 的管道的 fuzz target。
    - 3f：trace 怎麼收；量捲動時每個 frame 的延遲和 IO thread 每一步的時間。
    - 3g：macOS 上 sandbox 的探測；第 3 階段的檢查和文件。
    - **為什麼**：整合的問題（啟動、旗標、兩邊的 thread 怎麼接）最早碰到；3b 在 3c 前面，因為任何真的頁面都要 body，而沒有 GPU 時像素本來就走訊息，3b 做完 headless 就能跑；3e 要等基本功能都在才定得了；量測照原本的規劃放最後。
    - **不選先把單一 process 的模式改成 Browser 只有一條 IO thread**：動到現在全部測試都在跑的路，而「單一 process 時要不要連 IO thread 都拿掉」還沒定（「待決定」）。3a 只讓多 process 的模式用一條，單一 process 的先不動。
    - 對照 Chromium：平常是多個 process，`--single-process` 是測試和除錯用的；renderer 的入口是 `RendererMain`，child 的 IO thread 由 `ChildProcess` 建。
    - **2026-11-22 調整（3c 開始前；使用者確認的）：tile「只複製一次」從 3c 移到 3e 之後，後面的往後挪**。原因是開始 3c 時試出來的事實：renderer process 的 tile 現在就上得了 GPU（像素是訊息裡的一段位元組，codec 自己處理），只是比第 24 條定的多複製兩次（compositor 送的時候一次、Browser 的 IO thread 解碼時一次）。所以 3c 變成守住現狀，「只複製一次」是新的 3f：
        - 3c：renderer process 加 GPU 的測試、補上 3a 留下的 mutant、文件記明現在複製三次。
        - 3d、3e 不變。
        - 3f（新的）：tile 只複製一次（`ipc` 把 region 借出去畫、compositor 的像素池從 region 切、Viz 從共用記憶體直接上傳、region 不夠時怎麼要）。
        - 3g：trace 和量測（原本的 3f；3f 做之前先量一次現況）。
        - 3h：sandbox 的探測和階段的檢查（原本的 3g）。
    - **為什麼「只複製一次」不現在做**：它動到畫面那條路，3e 之後 reftest、WPT 和情境才能在多 process 底下守著它；把 region 借給 Viz 之後 renderer 當掉怎麼收回，是 3d 的規則，先有再接；排在量測之前，做之前和做之後各量一次，省了多少是量出來的。現在補的測試到時候就是它的回歸測試。沒有任何一段在等它。第 24 條的驗收條件（跨 process 也只複製一次）不改，由 3f 達成；這份 RFC 結束前要做到。
57. **3a 的幾件事**（2026-11-22，第 3 階段的 3a；逐項列出選項、使用者選的）：
    - **程式放進現有的 crate**：child 那一側（入口函式、IO thread）在 `renderer`，Browser 啟動 child、接上通道的那一段在 `browser`（`RenderProcessHost` 旁邊），兩邊共用的在 `protocol`；`shell` 的 `main` 只認 `--type=renderer`、選好 script engine、呼叫 `renderer` 的入口。理由：`renderer` 和 `browser` 本來就依賴 `ipc`；`browser` 的整合測試可以把自己再執行一次當 child，不必先編好整個 `machopo`；script engine 還是 `shell` 選，`renderer` 維持和 engine 無關。不選新開一個 `child` crate（Chromium 的 `content/child`）：現在只有一種 child，哪些算共用是猜的，有第二種（GPU process）時再從 `renderer` 搬出來。不選都放 `shell`：`browser` 的測試啟動不了 child，而且啟動 renderer 是 `browser` 的事。
    - **旗標是控制通道上的第一則訊息，命令列只有 `--type=renderer`**（改掉「啟動」和「要改的地方」原本寫的命令列）：child 拿到它那一端之後等第一則訊息（`RendererFlags` 和「Viz 有沒有 GPU」），再開 renderer 的 thread。理由：型別照 serde 走，加欄位不用改傳遞的程式，沒有長度和跳脫的問題；`browser` 的測試把自己再執行一次當 child，測試的執行檔不認得多的命令列參數，旗標走通道的話測試的 child 和真的 child 走同一條路；之後執行中要改的設定也是通道上的訊息。代價：`ps` 看不到每個 renderer 的設定；child 要等到第一則訊息才開始。`--type=renderer` 留在命令列：之後的 sandbox 要在讀任何東西之前靠它知道自己是哪種 process。不選每個欄位一個 switch（Chromium 對啟動時不變的設定的做法：10 個欄位各寫一次轉換，漏改不會編譯失敗）；不選命令列一個序列化好的參數（看不懂，而且有長度上限）。
    - **送的一端都是自己的型別，裡面是 crossbeam 或通道**：renderer 送給 Browser 的 `Sender<RendererHostMsg>`（crossbeam 的型別直接寫在 `renderer` 和 `compositor` 的 30 處）換成 `BrowserSender`；它、`CompositorFrameSinkSender` 和 Browser 送給 renderer 的 sender 裡面各有兩種：單一 process 時是 crossbeam，多 process 時直接呼叫 `ipc` 的 `Link::send`。理由：送的 thread 直接寫進通道，沒有多的 thread 和佇列；四種訊息經過同一把鎖，不同種之間的順序保得住（「架構」）；對方不讀時只有 `ipc` 的那一個佇列在長，上限只有一份。代價：30 處型別改名（呼叫 `send` 的地方不動）。不選留著 crossbeam、child 多一條轉送的 thread：每則訊息多一次叫醒（睡著時約 20 µs），兩種訊息各一個佇列時不同種之間的順序保不住，兩個沒有上限的佇列疊在一起。不選讓 IO thread 去拿 crossbeam 裡的訊息：`ipc` 的迴圈等的是 kernel 的東西。對照 Chromium：送的一端是 `mojo::Remote`，任何 sequence 呼叫就寫進 pipe。
    - **目的地的編號定義在 `protocol`**（照慣例做的，沒有列選項）：兩邊都要知道哪個編號是哪種訊息，而 `protocol` 是兩邊都依賴的。往 renderer：`RendererMsg`、`CompositorMsg`；往 Browser：`RendererHostMsg`、`CompositorFrameSinkMsg`。第一則訊息的型別帶著 `RendererFlags`，那是 `renderer` 的型別，所以定義在 `renderer`（`browser` 本來就依賴它）。
    - **`Handler` 的介面不收窄；規定是「不等、每則訊息做的事有上限」，3f 量**（「待決定」留下來的那一條）：看程式才知道兩邊不一樣。renderer 那一側的 IO thread 只解碼、放進 main thread 或 compositor 的佇列；Browser 那一側處理訊息的物件（`IoThreadHost`）本來就在 IO thread 上做事（核對請求和 cookie、子資源交給網路、登記 worker 和 blob URL，其餘的才轉給 UI thread），都是交出去、帶一個完成時的 callback，不等，但不是「只放進佇列」。所以收窄成「只能解碼加放佇列」對 Browser 行不通。不選只收窄 renderer 那一側：`ipc` 多一套只有一個使用者的介面，而那一側的 `Handler` 只有十幾行。不選把 Browser 的檢查搬離 IO thread：推翻「架構」和第 1 階段抽好的物件，子資源多一次轉手。對照 Chromium：IO thread 上禁止會等的呼叫（debug build 會 assert），允許做事。
58. **管道放在訊息裡就能過通道：`ipc` 的 codec 認得 `Consumer`**（2026-11-22，第 3 階段的 3b；使用者選的。改掉「導覽的 body 的管道」原本寫的「`CommitNavigation` 裡帶的是管道的編號」的做法，線上的結果一樣）：`ipc::data_pipe::Consumer` 自己能序列化。codec 序列化到它時開一條過通道的管道（`PipeOpened` 走在提到它的訊息前面）、在訊息裡寫編號；解碼到編號時取出這一端為那條管道建好的 `Consumer`。訊息的任何欄位放一個 `Consumer` 都能過，和大的位元組走 region 是同一套（第 31 條）。
    - **理由**：訊息的型別說了算，沒有「哪些訊息要另外處理」要記；`CommitNavigation` 的 `#[serde(skip)]` 拿掉，邊界上最後一個例外消失；`ipc` 之後分成獨立的專案時這是它自己完整的功能，由它自己的測試和 fuzzer 守，不是每個用它的專案各寫一段「開管道、放編號、取回來」；失敗時的收拾照 region 已有的做法（序列化到一半失敗，已經開的管道關掉）。
    - **代價**：serde 序列化時只借得到值，而管道要被拿走，所以 `Consumer` 裡多一層可以取走的包裝，同一個值序列化第二次是錯誤；`Consumer` 只在 `ipc` 的 codec 底下能序列化，別的格式回錯誤，靠一個保留的名字讓 codec 認出它（`serde_bytes` 和 TOML 的日期用的手法；分出去之後是公開的約定）；動到 codec 和端點（核心、TLA+ 的模型、Verus 的證明不動：管道的協定沒變，變的是誰在什麼時候開）。
    - **不選線上多包一層、明寫出來**（往 main thread 的線上型別是「訊息加上它的管道的編號」，送的一端先開管道、收的一端放回去；起草時的建議）：不動 `ipc`，但每多一種帶管道的訊息兩邊各加一段，`#[serde(skip)]` 留著，而且那一段邏輯留在 machopo，不跟著 `ipc` 走。不選線上和記憶體裡兩套訊息型別。
    - **3b 分成兩小段**：3b-1 在 `ipc` 做完並驗證；3b-2 `protocol` 拿掉 `#[serde(skip)]`、`browser` 和 `renderer` 接上、renderer process 載入大的頁面。
    - 對照 Chromium：Mojo 的 handle 可以放在訊息的任何位置，序列化時從物件裡取走。
59. **先加切換的旗標，預設不變；預設在階段驗收時才換**（2026-11-22，第 3 階段的 3e；使用者選的）：`machopo` 加 `--single-process`（Chromium 的名字，最後留下來的那個）和 `--process-per-tab`（要求每個分頁的 renderer 是自己的 process；Chromium 也有這個旗標，意思相近）。視窗和 automation pipe 兩種跑法的預設都維持單一 process。3e 用 `--process-per-tab` 把視窗和整套測試跑過、修掉壞的；3f、3g 做完，3h 階段驗收時把視窗的預設換成多 process。
    - **為什麼不現在就換視窗的預設**（「單一 process 的模式」寫的最後狀態）：視窗接 renderer process 還沒試過，tile 現在複製三次（3f），沒有偵測 renderer 卡住的機制，Linux 和 Windows 沒有驗過；每天用的視窗會先踩到這些。換預設那一刻要有整套測試和量測的數字。
    - **不選兩種跑法現在都預設多 process**（Chromium 的做法）：上面的缺點都有，加上整套測試變慢多少還不知道。
    - **收尾時 pipe 和測試也預設多 process**（2026-11-22 定的，使用者選的；改掉「單一 process 的模式」寫的「headless 和測試的預設」是單一 process，那一節在 3h 換預設時一起改）。依據是 3e 量的：`shell` 的 pipe 測試 36 個，單一 process 4.4 秒、`--process-per-tab` 4.6 秒，都過；WPT 的 `dom/events`（217 個測試、1069 項檢查）83 秒對 71 秒（差距是雜訊），結果逐項相同。看不出變慢。之後每天的 WPT、reftest、情境、WebDriver 的測試走的就是使用者用的那條路，不用另外再跑一遍；單一 process 那條路由 `cargo test` 裡直接在 process 裡建分頁的測試守（`browser`、`shell`，本來就只能那樣跑）。不選 pipe 維持單一 process：多 process 那條路要另外跑，每天跑 CI 的時間加倍，每週跑壞了最多一週後才知道，而且平常跑的不是使用者用的那條路（「缺點」裡自己列的）。
    - **換預設之前**：`test.py` 的 `wpt`、`reftest`、`wdspec`、`scenarios --window` 加 `--process-per-tab`；每天的 CI workflow 加一個手動開始時可以選的輸入，選了就用它跑整套。每天自動跑的內容不變。整套在多 process 底下是綠的才換。
    - **一直是單一 process 的**：情境「沒有平台視窗」的版本和 `cargo test` 裡直接建分頁的測試在測試的 process 裡跑，沒有 `machopo` 執行檔可以當 renderer。多 process 的情境用 `scenarios --window`。
60. **Browser 這一側的 fuzz target 涵蓋到分頁，body 的管道不另外寫**（2026-11-22，第 3 階段的 3e-2；使用者選的；「測試」的表原本寫的是兩個 target）：
    - **一個 target**：輸入切成幾則訊息，每則解成 `RendererHostMsg`，先交給 IO thread 上的物件（`IoThreadHost`），它轉給 UI thread 的再交給分頁（`WebContents::on_renderer_message`），分頁判定壞訊息時照正常的路結束 renderer。惡意的 renderer 碰得到 Browser 的整條路。要找的是讓 Browser panic 的訊息（`unwrap`、索引越界）：Browser 一 panic 所有分頁都沒了。
    - **不選只到 IO thread 上的物件**：frame 樹、導覽、history 的檢查在分頁那一半，判定壞訊息的地方那裡比較多，狀態也比較複雜。
    - **為了它加的**：一種「另一端沒有東西」的 renderer 啟動方式（`RendererLauncher::Detached`：Browser 送給它的訊息留在佇列裡沒有人讀，renderer 的訊息由呼叫的人同步交進來），只給測試和 fuzz 用；`net` 的 host 規則把一切對到「不存在」時，連寫成 IP 的網址也拒絕（原本 IP 不經過解析、規則管不到；Chromium 的規則兩種都管），fuzz 用這個確保什麼都連不出去。
    - **body 的管道**：過通道的部分是 `ipc` 的，它的 `pipe` 和 `endpoint` 兩個 target 已經涵蓋（3b-1 擴充過訊息裡帶管道的情況），不另外寫。
    - **放哪裡、怎麼跑**照 `ipc` 的做法：harness 是 `browser` 整合測試的一個模組，找到過的輸入放進回歸的資料夾、`cargo test` 每次重放；cargo-fuzz 的 target 在 `crates/browser/fuzz/`；`test.py` 跑，只在本機（第 55 條），跑之前先問使用者。
61. **tile 的像素用 `ipc` 現有的 region，加上「借出去畫」**（2026-11-22，第 3 階段的 3f；使用者選的。第 23 條寫的「resource 帶 region 的編號、位置和長度」改成帶一整塊 region，沒有位置）：送的一方先向池子拿一塊空的 region 直接畫（不複製），訊息裡帶那一塊的參照；收的一方解出來的是那塊記憶體的 view（不複製），view 放掉時才回報「可以重複用了」。tile 是 256 KiB，剛好是 region 的一種大小。
    - **理由**：沿用已經驗證過的池子、表和回報的協定（TLA+ 的 `Regions` 模型、fuzzer），不另外發明一套；「Viz 用完了」就是 view 放掉，`ipc` 自己回報；對方給的編號和長度的檢查已經在 `ipc` 裡。
    - **代價**：`ipc` 的一條不變式要改（`soundness.md` 的 I3：共用記憶體只被整段複製、不借出去）：view 借出去的是對方還能寫的記憶體，要定清楚它只能怎麼用；view 活多久由上層決定，一塊 region 被佔住的時間變長。
    - **不選另外給一大塊、compositor 自己切**（第 23 條原本的樣子）：多一套分配、位置和長度的檢查、收回的邏輯要自己驗證；讀共用記憶體要 `unsafe`，還是得在 `ipc` 加介面；大小要先猜。不選只省 IO thread 上那一次（收的一方拿 view、送的一方照舊複製）：`ipc` 要動的差不多，只省一半。
    - 對照 Chromium：改成 GPU raster 之前，tile 的共用記憶體是每塊各自一個、由 renderer 向 Browser 要，生命週期跟著 resource 的歸還。
62. **renderer 可以寫的 region：啟動時先給一組，不夠再要，compositor 不等**（2026-11-22，3f；使用者選的；「待決定」的「renderer 向 Browser 要共用記憶體時怎麼不卡住 compositor」）：
    - Browser 啟動有 GPU 的 renderer 時先給一組小的（暫定 16 塊 256 KiB，4 MiB）。compositor 拿不到空的 region 時，那一塊照現在的路走（像素複製進訊息），同時送一則「我要這個大小的」給 Browser；Browser 在上限內建好交給它（`ipc` 已有的 `Granted`），之後的就有得用。總量的上限沿用每個方向的 region 上限（暫定 64 MiB）。
    - **compositor 永遠不停下來等記憶體**：沒有空的就走複製。所以「沒有共用記憶體時退回哪條路」不用另外決定，就是現在的路。
    - **實作時定的**（2026-11-22）：「我要這個大小的」是 `ipc` 自己的一則控制訊息（`Wanted`），由 `ipc` 在「大的位元組找不到空的 region、只好放進訊息」的那一刻送，不是 compositor 另外送的：tile 退回複製時送的就是這樣的位元組，所以一塊退回的 tile 剛好要一塊；canvas 的快照和 renderer 送給 Browser 的其他大資料也跟著要得到。上限算在 Browser 那一端（`peer_region_limit`，Browser 自己給的和對方要的加起來），超過的不建、不算壞訊息；建 region 的那一端收到 `Wanted` 是壞訊息。tile 的 region 和其他大資料用的是同一個池子。
    - **不選只在啟動時給固定的量**：大螢幕、高 DPI、捲動快時不夠，多的 tile 永遠走複製；canvas 的快照大小不合（region 的大小是 2 的次方），完全用不到。**不選全部用要的**：每個分頁最前面幾個 frame 都走複製，那是載入最忙的時候。
    - **Browser 什麼時候收回給出去的 region 不在這次**：renderer 結束時全部放掉（3d）。有上限守著；量到記憶體是問題時再做（分頁變成背景時收回）。留在「待決定」。
    - 對照 Chromium：renderer 每要一塊共用記憶體都向 Browser 發一次同步的請求、等回答。這裡不等、先走複製，因為 compositor 不能停。
63. **Viz 把共用記憶體裡的像素直接複製進 GPU 的緩衝區**（2026-11-22，3f；使用者選的）：像素來自 view 時，Viz 向 wgpu 要一塊已經 map 好的緩衝區，view 提供「把這一段複製到給的地方」，再由 GPU 把緩衝區搬進 texture。整條路只複製這一次（第 24 條的驗收條件）。像素是一般的記憶體時（單一 process、退回複製進訊息的那幾塊）照舊走 `write_texture`。
    - **view 不變成 `&[u8]`**：那塊記憶體 renderer 還能寫，借成 slice 是不成立的借用（編譯器假設借用期間內容不變）。view 只提供長度和「複製出去」；renderer 在複製途中改內容，拿到的是亂的像素，不是未定義行為。
    - **代價**：Viz 多一條上傳的路；wgpu 要求緩衝區每一列的長度是 256 的倍數，tile（256 px 寬，一列 1024 bytes）剛好，寬度不合的（canvas 的快照）一列一列複製、補齊。
    - **實作時發現、改掉的**（2026-11-22）：wgpu 30 的 mapped 緩衝區是「只能寫」的型別（`WriteOnly`，因為那塊記憶體可能是 write-combining 的），拿不到 `&mut [u8]`，所以「view 把位元組複製到給的地方」寫不成。直接寫它的裸指標要 `unsafe`，而 `unsafe` 只能在 `ipc`：那就得讓 `ipc` 依賴 wgpu 的型別。改成 view 把位元組一段一段以值（`[u8; 256]`）交出去，Viz 用 wgpu 安全的介面寫進去：`unsafe` 還是只在 `ipc`，`ipc` 不認得 wgpu。量過（256 KiB，來源和目的地輪流換、不讓它們留在 cache 裡）：一次 `memcpy` 5.8 到 8.1 µs，一段 64、1024、4096 bytes 以值交是 4.6 到 6.2 µs，沒有差別。不選讓 `ipc` 依賴 `wgpu-types`：`ipc` 之後要獨立成 crate。
    - **緩衝區沒有重複用**：每上傳一塊建一個（mapped 的緩衝區用過之後要等 GPU 才能再 map，重複用要一個非同步的池子）。wgpu 自己的 `write_texture` 也是每次建一個，所以和原本的路一樣；一批 tile 的複製指令合成一次 submit。3g 量到是問題時再做。
    - **不選先複製到 Viz 自己的記憶體再 `write_texture`**：兩次複製，沒達到驗收條件，留一截之後要做。**不選把 view 當成 `&[u8]`**：上面說的借用，要在 `ipc` 的 soundness 開一個說不出理由的例外。
    - 對照 Chromium：one-copy raster 時 GPU process 把共用記憶體直接交給 `glTexSubImage2D`，次數相同。

## 待決定


- **CI 之後要不要加 macOS、Windows 的 runner**（第 40 條；2g 做完時定了先不加，第 54 條）：要跑的只有 `cargo test -p ipc`（不依賴 V8，幾分鐘）。重看的時機：共用的改動又在另一個平台上壞掉而沒有人發現時，或作業系統的 sandbox 開始做的時候。要看的是費用（GitHub 的 macOS runner 是 Linux 的 10 倍、Windows 是 2 倍）和 runner 的系統版本與開發機不同時那幾個實測來的行為還成不成立。
- **IO thread 上複製大資料的時間**（第 36 條；第 3 階段量捲動時每個 frame 的延遲時一起看）：64 KiB 以上的 `BigBuffer` 在 IO thread 上從 region 複製出來，那一段時間後面的訊息等著。量到是問題時，把複製搬到目標 thread（IO thread 只交出「這一塊的借據」，目標 thread 複製完再還）：線上的格式不變，代價是表跨 thread 共用、多一組 loom 的模型。
    - **第 42 條之後影響變大**（2026-11-21）：Browser 的一條 IO thread 管所有 renderer 的通道，一個 renderer 送大資料時，等的是其他每一個 renderer 的訊息，不只是它自己後面的。
    - **現在有的數字**：複製一次約 18 到 26 µs/MiB（第 36 條的 1 MiB 約 18 µs、8 MiB 約 150 µs；macOS 的通道借出來不複製是 8 MiB 408 µs、多複製一次是 622 µs）。照這個推算，64 MiB 約 1.7 ms、256 MiB 約 7 ms、最大的一塊 1 GiB 約 27 ms；這幾個是推算，沒有量過那麼大的，也沒有和解碼合在一起量。常見的大小（幾百 KiB 到幾 MiB）是幾十到幾百 µs。
    - **管道的資料也在 IO thread 上複製**：一步裡每條管道最多送到對方還有的空間（不超過那條管道的容量，64 KiB 一段），所以一步佔多久看開管道的人給的容量；容量幾百 KiB 時是十幾 µs。量的時候一起看。
    - **對照 Chromium**：IO thread 只把訊息轉給綁定 `Receiver` 的那條 thread，型別的檢查和反序列化在那條 thread 上做；`BigBuffer` 走共用記憶體時 IO thread 只經手 handle，收的一方拿到 mapping 的 view，要不要複製由它決定。它的 IO thread 不付這一次複製，付的是 region 佔得比較久、和對方不被信任時要自己先複製再用。上面「搬到目標 thread」就是往這個做法靠。

排程之前：

- ~~重新盤點~~：2026-11-20 做了，結果在 `details.md` 的「現在的邊界」和「要改的地方」。和 2026-10-04 比，新的工作是導覽的 body 的管道（例外 7）和 trace；同步的呼叫、worker 的訊息、Browser 的 frame 樹和 loader 的核對都已經照邊界做，不用改。renderer 這一側沒有 wgpu（raster 是 vello_cpu 畫的 CPU 像素），所以 T1 不用搬 raster。
- ~~原型~~：2026-11-20 做了，在 `prototype/`（macOS，它的 README 有每一項的數字和沒做的部分）。結果：
  - 「為 sandbox 留的條件」成立：child 一開始就進只開三樣東西的 sandbox（rendezvous 那一個名字、系統字型的目錄、自己的執行檔），rendezvous、控制通道、讀字型都可以，其他的（讀寫檔案、網路、啟動程式、查別的服務）都被擋。和假設不同的一點：sandbox 擋不住 renderer 自己建 memory entry，「只有 Browser 建立共用記憶體」在 macOS 上靠協定（renderer 的那一端不能附 right），不靠 sandbox。
  - 三種 process（host、sandbox 裡的 renderer、另一種 child，兩個 child 之間有 host 建的直接通道和一塊 host 建的 region）跑得起來：「附 handle 是每一端的權限」「目的地是編號」「表和檢查任何一端都能用」都不用為第三種 process 改結構。壞的編號、超出的範圍、沒有權限的一端附的 right、假冒的 kernel 通知、把唯讀的 entry map 成可寫，都被擋下。
  - 控制通道：一則小訊息來回 4.2 µs（中位數，p99 7.7 µs）；child 被 kill 之後 12 到 40 µs 內知道。
  - frame：machopo 對真的頁面送的 frame 最多 46 個 quad，postcard 編出來不到 3 KiB，編 1.4 µs、解 2.3 µs，連送帶回答 8 µs，是一個 frame 的時間的 0.05%。
  - 大的資料：留著重複用的 region 最快（1 MiB 是 36 µs，放在訊息裡是 105 µs）；每次新建一塊 region（120 µs）比放在訊息裡還慢。
  - ring：讀的一端醒著時來回 0.21 µs（Mach 訊息的 20 分之一），但它一睡，喚醒就是一則 Mach 訊息、成本一樣；連續一大批時快 6 到 11 倍；當管道送 16 MiB 是 0.31 ms，一段一段的 Mach 訊息是 1.47 ms（每秒 11 GiB）。
  - 和單一 process 的 crossbeam 比：兩邊都醒著時 crossbeam 來回 0.21 µs（訊息是搬的，不編不解不複製），Mach 訊息 4.2 µs；對方已經睡著時（隔 1 ms 才送）兩者都是約 24 µs，時間花在叫醒 thread，跨不跨 process 沒有差別。
  - machopo 實際的訊息有多密（單一 process 上量的，一個分頁照人的節奏操作）：閒置時 0 則；60 Hz 捲動時往 Browser 每秒 185 則、往 renderer 每秒 82 則，一個 frame 的幾則緊接著到、frame 和 frame 之間隔約 16 ms；載入時請求一口氣發出。讀通道的 thread 醒著的時間不到千分之一，緊接著到的訊息全部改走 ring 每秒省不到 0.4 ms。
  - 和別的 crate 比（2026-11-21）：小的訊息（machopo 的都是）原型、`ipc-channel` 和 `interprocess` 一樣，來回約 4.3 µs、一個真的 frame 約 10 µs；大的資料原型的留著重複用的 region 比 `ipc-channel` 的 `IpcSharedMemory`（每次新建一塊）快 2 到 3.5 倍。調過緩衝區的 Unix socket 內嵌送 1 MiB 是 41 µs，和 region（36 µs）差不多，比 Mach 訊息內嵌（100 µs）快：macOS 的結論不變（控制通道是 Mach 訊息），Linux 上要重量（「待決定」的設計那一段）。
  - 沒有原型的：Linux 和 Windows 的平台層；送的時候不等、自己排隊的那一段；接上 machopo 之後捲動時的延遲。

設計：

- **Browser 什麼時候收回給 renderer 的 region**（第 62 條留下的；量到記憶體是問題時）：現在只在 renderer 結束時放掉。可以做的：分頁變成背景時收回空的那幾塊。
- **單一 process 時要不要連 Browser 的 IO thread 也拿掉**：處理訊息的邏輯抽成物件之後（「架構」）做得到：renderer 拿到的「送訊息的東西」在這個模式直接呼叫那個物件，由它交給網路或叫醒 UI thread，中間沒有佇列也沒有 thread。每個分頁少一條 thread，renderer 到 UI thread、到網路各少一次轉手。代價：Browser 的邏輯在 renderer 的 thread 上跑（main thread 和 worker 可能同時送，要加鎖；它 panic 時倒的是 renderer 的 thread）；`RendererGone` 改成靠最後一個 sender 被丟掉來報告；「從通道讀出來、交給物件」那一段只剩多 process 的測試跑得到。那條 thread 現在就在，不是這份加的成本，所以先量每則訊息的那一次轉手占多少再決定。比過的做法見 `details.md` 的「比過的做法」。
- **renderer 不能讓 Browser 當掉**：惡意的 renderer 最容易走的三條路，接上 renderer 的那一步要一起處理。
  - 沒有上限的東西：Browser 收 renderer 訊息的佇列、Browser 送給卡住的 renderer 的佇列、一個 renderer 能要的共用記憶體（「`ipc` crate」）、反序列化的巢狀深度（同上）。各要定一個數字，超過就關掉那個 renderer。
  - 處理訊息時 panic：Rust 擋得住記憶體錯誤，擋不住 `unwrap` 和越界。處理 renderer 的訊息的程式遇到不合理的內容要走壞訊息的路（`browser` 的 `BadMessageReason`），不能 panic。
  - 壞的 frame：frame 是在 Browser 裡交給 GPU 的，而 wgpu 的驗證錯誤現在會結束整個程式。Viz 收到 frame 時的檢查要涵蓋所有會走到 wgpu 的值；這一項單一 process 時就可以開始做。
- **trace 怎麼收**：每個 process 各寫一個檔案（`MACHOPO_TRACE` 加上 pid）、之後合併，或 child 把 span 送回 Browser。Chromium 是每個 process 的 tracing service 把資料送回 Browser 合成一份。
- **哪些模式預設多個 process**：看法是只有人用的視窗；headless、情境、`--automation-pipe` 和測試預設單一 process，CI 另外用多個 process 跑選定的幾套（下面的「哪幾套測試」）。
- **哪幾套測試在 CI 上用多個 process 跑**：WPT 可以。情境和 reftest 怎麼啟動、要全部還是一部分、多花多少時間，還沒確認。
- **Browser 的 IO thread 要不要合成一條**（2026-11-20：第 3 階段決定，那時開得了多個 renderer process、可以量；合成一條之後那條 thread 比較忙，同時重量一次訊息的 ring 值不值得，第 19 條）：現在每個 renderer 一條，分頁多的時候 thread 跟著多；Chromium 是整個 process 一條處理所有通道。IO thread 的迴圈本來就不寫死成只等一條通道（「`ipc` crate」），合起來不用改設計；量到 thread 的數量是問題時再做。
- **背景 GC**：拆 process、有背景分頁時，V8 的 heap 在背景要不要 concurrent GC。

## 之後的可能

這份做完之後，照對安全的幫助排（「理由和其他做法」的「要拆幾種 process」）：

1. **sandbox**（另一份 RFC，第 18 條）：沒有它，拆 process 只換到穩定和記憶體分開。不用多拆 process，是把已經拆出來的關起來。那份要涵蓋：
   - 三個平台的作業系統的 sandbox：macOS 的 Seatbelt、Linux 的 seccomp 和 namespace、Windows 的 restricted token 和 job；各平台的做法和現成的 crate 要比過。
   - V8 的 sandbox（`v8_enable_sandbox` 重編 V8，embedder 照它的規則改）：process 內的防線，和作業系統的互補。它不需要 multi-process，可以排在這份之前。
   - 字型怎麼給 renderer：sandbox 開放系統字型目錄的讀取（Chromium 在 macOS 上），或 Browser 提供字型的服務（Chromium 在 Linux 和 Windows）。
   - 已經查到的在 `details.md` 的「Seatbelt 的兩種套用時機」和「比過的做法」的字型。
2. **GPU process**：Viz 搬出 Browser，顯示卡的驅動程式當掉時只重啟它，renderer 送來的 frame 也不再是在 Browser 裡交給驅動程式。到時 renderer 要有直接連到 GPU process 的通道，`CopyOutputRequest` 的 callback 要改成回應的訊息；GPU process 自己建的緩衝區要交給別人，是「誰能附 handle」第一個用到 Browser 以外的地方。
3. **site isolation**：照網站分 renderer，跨網站的 frame 到別的 process（跨語言的 frame 也當成遠端 frame 一起做），多個分頁共用 renderer。難的是 Browser 這一側的架構，不是 IPC。
4. **Network Service process**：Browser 這一側是 Rust，急迫性最低。

和它們一起、或之後才需要的能力：

- **child 之間直接互通**（renderer 和 GPU process、renderer 和 Network process、跨 renderer 的 `MessagePort` 和 shared worker）：先由 Browser 轉送，量到太慢才加直接的通道。通道的兩端都由 Browser 建好再交出去，每對 process 之間一條；上面的每個對象是一個目的地的編號，由 Browser 向收的一方登記它代表誰、能做什麼。Chromium 是讓「拿到通道」本身代表權限（每個 frame、每個 worker 各一條 Mojo 的 pipe，renderer 直接對 Network process 送請求）；編號加登記是同一件事。不同通道之間的訊息沒有先後的保證，現在靠順序的檢查（先登記 worker、再替它送請求）到時要改寫。
- **OOP-R（T3）**：raster 搬到 GPU 那一側，tile 不再經過 CPU；需要 display list、`gfx::FontData`、`gfx::ImageData` 能跨 process。
- **macOS 上直接交 GPU 的 texture（T2）**：`IOSurface` 能換成 Mach port 放進訊息，傳遞這一段是現成的；wgpu 那一側的問題不變。
- **子資源的 body 也走 data pipe**：現在只有導覽的 body 是串流的，子資源和 `fetch()` 收齊才交給 renderer；Chromium 的 `URLLoaderClient` 收 body 都經過 data pipe。
- **body 的管道換成共用記憶體裡的 ring**（第 21 條）：接上 machopo 之後量到大的頁面載入時控制通道被 body 佔住，才換；每條管道一塊 Browser 建的記憶體，Mojo 的 data pipe 的做法。`Producer` 和 `Consumer` 的介面不變。原型量到當管道送 16 MiB 是 0.31 ms（走控制通道是 1.47 ms）。
- **訊息的 ring**（第 19 條）：訊息變密、量到控制通道的系統呼叫值得省時，每個方向加一個共用記憶體裡的 SPSC ring，照 `details.md` 的「ring 的設計」；帶 handle 的訊息照舊走控制通道，兩條路的先後靠訊息的序號排回來。原型的 `ring.rs` 是它最小的樣子。
- **Mojo 式的介面**：訊息多到 enum 難以管理時，在 `ipc` 上面加介面和 `Remote` / `Receiver`。
