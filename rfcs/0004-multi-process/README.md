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

- **一條通道多工**：四種訊息（`RendererMsg`、`CompositorMsg`、`RendererHostMsg`、`CompositorFrameSinkMsg`）都在同一條控制通道上，各帶一個目的地的編號。每種訊息照送出的順序到，不同種之間的相對順序也保留（相當於 Mojo 的 associated interface 共用一條 pipe）。目的地現在是固定的四個；之後同一條通道上要有更多對象（每個 frame、每個 worker 各一個）時加編號就好，不用為每個對象多開一條系統的通道。
- **兩端的 IO thread**：renderer 多一條，讀控制通道、依目的地的編號放進 main thread 或 compositor thread 的 task queue；main thread 照舊只等它的 scheduler。Browser 沿用每個 `RenderProcessHost` 一條的 `Chrome_IOThread`，`CompositorFrameSinkMsg` 由它轉給 Viz thread，用自己記著的 `FrameSinkId`、不信任訊息裡的。Viz 在 Browser 裡，所以比 Chromium（renderer 直接連到 GPU process）多一次轉手。
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

- **socket 的緩衝區要設**（Linux 的控制通道；2026-11-21 原型在 macOS 上量的，`prototype/` 的 README 的「和 ipc-channel、interprocess 比」）：Unix socket 預設的緩衝區小（macOS 是 8 KiB），超過的訊息要來回切換好幾次才送得完，16 KiB 來回慢 2.8 倍；建通道時用 `SO_SNDBUF`、`SO_RCVBUF` 設大（原型設 1 MiB 之後和 Mach 訊息差不多）。設多大在第 2 階段做 Linux 的平台層時量了再定（「待決定」的那一次量測）：至少要放得下門檻以下的訊息。

**共用記憶體**：

- **三種 region**：`ReadOnlySharedMemoryRegion`、`WritableSharedMemoryRegion`、`UnsafeSharedMemoryRegion`，名字和語意照 Chromium。可寫的能轉成唯讀的一次；唯讀的 handle 在 OS 那一層就不能拿來寫（Linux 的 `F_SEAL_FUTURE_WRITE`、macOS 另建一個只有 `VM_PROT_READ` 的 memory entry、Windows 複製 handle 時只給 `FILE_MAP_READ`）。大小建立時決定，不能改。
- **只有 Browser 建立**：renderer 要共用記憶體時向 Browser 要，拿到之後留著重複用。renderer 不需要建立共用記憶體的權限，它也沒辦法交給 Browser 一塊會被縮小的記憶體。
- **一塊 region 只屬於一個 renderer**：renderer 還回來、或它不在了，Browser 就放掉那一塊，不轉給別的 renderer。裡面還留著前一個 renderer 寫的東西（它畫過的像素、它送過的資料），轉手就是跨分頁洩漏。每個 renderer 能要的總量有上限。
- **能不能附 handle 是通道每一端各自的權限，renderer 那一端沒有**：
  - Browser 建一條通道時決定每一端能不能在訊息上附 handle。這份只有 Browser 的那一端能附，所以 handle 只從 Browser 往 renderer 走。
  - 沒有權限的一端送來的訊息附了 handle 就是壞訊息。唯一的例外不在控制通道上：macOS 的 rendezvous 請求照 Mach 的慣例帶一個讓 server 回覆用的 port。
  - 不寫死成「Browser 可以、child 不行」：之後 GPU process 要交出只有它建得出來的緩衝區。不變的是 renderer 永遠不能。
- **用編號稱呼 region**：
  - Browser 建立一塊 region 時給它一個編號（每個 renderer 各自一組），把編號和 handle 一起交給 renderer。作業系統只認 handle，光有編號 map 不了，所以這一次省不掉。
  - renderer map 好、留著，之後直接寫那塊記憶體。它在訊息裡提到這一塊時只寫編號，一塊裡放好幾樣東西時加上位置和長度。
  - Browser 為每個 renderer 記一張「編號 → region」的表，用自己建立時就有的 handle 和 mapping。要檢查的只有：編號在不在這個 renderer 的表裡、位置加長度有沒有超出那一塊（checked 的運算）。三個平台都一樣。
- **map 出來的記憶體不轉成 `&[u8]`**：對方隨時可能在寫，拿著 Rust 的參照就違反了 aliasing 的規則。存取一律經過指標：計數用 atomic，內容用指標複製到自己的記憶體之後才用。Viz 上傳 tile 的像素也一樣，不能把共用記憶體直接當成 slice 交給 wgpu。

**控制通道**：

- 一則訊息是長度、目的地的編號、序號、序列化的內容；有權限的一端還可以附帶 handle。序號現在只用來檢查（每一端照送出的順序加一，收的一端看到不連續就是壞訊息）；留著它，是為了之後訊息分兩條路走時（「之後的可能」的 ring）排得回順序，不用改格式。
- 上層只經過一個介面送和收訊息（目的地、內容、handle），不知道底下是哪一種傳輸。
- 三個平台同一個形狀：IO thread 等「通道」和「喚醒」；送的時候不等，送不出去的自己排隊。這份每個 process 只有一條通道，但 IO thread 的迴圈不寫死成只等一條。
  - **Linux**：non-blocking 的 socket。handle 跟著同一次 `sendmsg` 送出；收的一端處理控制資料被截斷的情況，收到的 fd 用 `MSG_CMSG_CLOEXEC` 設成 close-on-exec。
  - **macOS**：一則訊息是一則 Mach 訊息；每一則都照 audit token 確認是那個 child 送的（第 22 條）。IO thread 阻塞在 `mach_msg`，等一個 port set（通道的 receive port 加一個自己的喚醒用的 port）；送的 timeout 是 0。在通往對方的 send right 上登記 dead-name、在自己的 receive port 上登記 no-senders，任一個到了就當成斷線；通知只認 kernel 送的（訊息的 audit token），對方可以送一則 id 相同的訊息來假冒。
  - **Windows**：overlapped 的 `ReadFile` / `WriteFile`，寫入一次只有一筆在進行。IO thread 等「讀完成」和「喚醒」兩個 event；結束時用 `CancelIoEx` 取消進行中的 I/O 再關 handle。Browser 送 handle 時自己用 `DuplicateHandle` 複製進 renderer 的 process，訊息只帶數值。
- 序列化：serde 加 postcard（第 22 條）。postcard 自己沒有巢狀深度的上限：會一層包一層的型別（下一條）的解碼自己數深度。Browser 新發出的 region 序列化成 handle 表的 index，handle 本身走帶外；其他時候 region 只是編號，和一般的欄位一樣。
- 收的一端先檢查長度上限、目的地的編號（不認得的就是壞訊息）和 handle 的數量（對方沒有權限時只能是 0），再反序列化。反序列化時巢狀的深度有上限：訊息裡有可以一層包一層的型別（例如 `protocol::webdriver` 的 `WebDriverValue`，renderer 送的），不設限的話對方送一個巢狀很深的值就能讓收的一端 stack overflow，那不是 panic、接不住，整個 process 結束。不合的就當成對方壞掉，關掉通道（Chromium 的 `ReceivedBadMessage`）。

### 大的資料

- **`protocol` 加一個 bytes 的型別**（`protocol::BigBuffer`，Chromium 的 `mojo_base::BigBuffer`，第 25 條）：單一 process 時是 `Bytes`；跨 process 時 64 KiB 以下放在訊息裡，以上放進共用記憶體（第 22 條）。兩個方向都走留著重複用的 region，不是每筆資料新建一塊（新建、map、第一次碰每一頁的成本比直接放在訊息裡還高）：Browser 送的寫進它給這個 renderer 的唯讀的 region，第一次用那一塊時連 handle 一起交出去，之後只帶編號、位置和長度，renderer 複製出來之後回報、Browser 才重複用那一段；renderer 送的寫進 Browser 之前配給它的 region，訊息只帶編號、位置和長度。macOS 上訊息一律內嵌在 Mach 訊息裡，不用 out-of-line 的 descriptor（64 KiB 以下兩者一樣快，以上已經走 region）。跨邊界的訊息裡的位元組欄位都用它：子資源的回應、worker 的主 script、導覽的 body 放在訊息裡的那一段、blob 的內容和 request 的 body（renderer 送的）、history 的 state、沒有 GPU 時 frame 的像素。導覽的 body 不用：它只有開頭在訊息裡，其餘走 data pipe（下一節）。
- **從 renderer 來的大資料**：renderer 還能寫那塊記憶體，所以 Browser 一律先複製出來再用（Chromium 的 `BigBuffer` 的註解也警告這個 TOCTOU 的風險）。

### 導覽的 body 的管道

`CommitNavigation` 的 body 其餘的部分是一條 data pipe（`ipc::data_pipe`）。跨 process 時一段一段走控制通道，背壓用訊息（第 21 條）：

| 步驟 | 單一 process | 跨 process |
| --- | --- | --- |
| 網路收到一段 | 寫進管道（Browser 的 network thread） | 一樣，寫進 Browser 這一側的管道 |
| 交給 renderer | renderer 的 main thread 直接讀同一條管道 | Browser 的 IO thread 從那條管道讀出來，包成 body 的訊息（管道的編號、位元組）送進控制通道；renderer 的 IO thread 寫進 renderer 自己 process 裡的另一條管道，main thread 讀它 |
| 背壓 | 管道滿了，network thread 的寫就等 | renderer 每讀走一些就回一則「這條管道我讀了多少」；Browser 送出去、對方還沒說讀了的量不超過管道的容量（512 KiB），到了就不再從自己的管道讀，那條管道滿了，network thread 的寫照樣等 |
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
- **像素只複製一次**（和改之前一樣）：compositor 有一個緩衝區的池子，raster 直接畫進池子的一塊，那一塊的擁有權跟著「建立」交給 Viz，Viz 交給 GPU 的那一次是唯一的複製。單一 process 時一塊是一段共用的記憶體（`Arc`），跨 process 時是 Browser 配給它的 region 的一段（第 22 條的池子），步驟相同；跨 process 時 Viz 從共用記憶體用指標直接複製進 GPU 給的緩衝區，不先複製到自己的記憶體（共用記憶體不能當成 slice 交給 wgpu）。畫出來是單一顏色的 tile 不上傳，那一塊當場回池子。
- **有計數守著**：tile 的像素每複製一次就計一次，測試確認次數等於上傳的 tile 的數量，單一 process 和跨 process 都是；多一次複製時測試失敗。第 3 階段的驗收包含跨 process 的這一項。
- GPU 遺失時 Viz 手上的 texture 都沒了：Browser 照舊送 `ContextLost`，compositor 忘掉所有編號、重新畫、重新送「建立」。
- renderer 碰不到 GPU，之後的 sandbox 可以鎖得最緊。

### 啟動

Browser 建好控制通道的兩端，以 `--type=renderer` 和 renderer 的旗標（`RendererFlags`，已經能序列化）啟動同一個執行檔，把一端交給 child。child 的 `main` 看到 `--type=renderer` 就不開視窗、選好 script engine，從控制通道拿到初始的訊息，然後跑現在的 `spawn_renderer_main_thread`。之後的 sandbox 套在這一段的最前面，任何其他程式碼之前。

- **Linux 和 Windows**：child 繼承一端（怎麼繼承見「待決定」）。兩端都由 Browser 建立，沒有用名字連線的步驟，別的 process 搶不到通道。
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

1. **記憶體**：不管對方在共用記憶體裡寫什麼、在訊息裡給什麼編號、位置和長度，這一端讀寫的都在它自己 map 的那一塊裡面。
2. **壞的輸入**：從通道讀到的任何位元組，結果只有兩種：照規則處理，或當成壞訊息關掉通道。不會 panic、卡住或 stack overflow，用掉的記憶體有上限。
3. **handle**：沒有權限的一端送的訊息，不會讓收的一端留下任何 handle。
4. **region 的歸屬**：一塊 region 只在建它時指定的那個 renderer 的表裡；收回之後不在任何表裡，不會交給別的 renderer。
5. **不被卡住**：送訊息不等對方；對方不讀時，這一端的佇列到上限就關掉通道，不是一直長。
6. **順序**：一條通道上，收的一端交出訊息的順序就是送的一端送的順序，不重複、不漏。
7. **斷線**：對方不在了（結束、當掉、只關掉一半、送了壞訊息）時，這一端剛好報告一次，之後不再交出它的訊息。
8. **同步的呼叫**：回答只到等它的那個呼叫；通道斷了時等的一方醒來、當成失敗。
9. **data pipe**：讀到的位元組就是寫的、照順序、不重複；Browser 送出去、renderer 還沒說讀了的量不超過管道的容量；renderer 說讀了的比送的多是壞訊息。

平台層守不住的寫成**假設**：kernel 照文件做的那幾件事（把唯讀的 handle map 成可寫會失敗、收到的 handle 是對方真的給的、訊息的來源認得出來、對方不在了一定通知）。每一條假設有一個真的跨 process 的測試，在那個平台上不成立時測試會失敗；原型在 macOS 上試過一遍（`prototype/README.md`）。

### 測試

**每條性質用什麼驗證**（工具照 elise 的用法；`scripts/test.py` 加上跑它們的指令，`sanity.py` 檢查核心沒有碰系統呼叫）：

| 方法 | 守哪幾條 | 怎麼做 | 什麼時候跑 |
| --- | --- | --- | --- |
| 單元測試和惡意的對端 | 全部 | 核心經過假通道測；測試扮演壞掉的 renderer：超過上限的長度、不認得的目的地、不連續的序號、附了 handle 的訊息、不在它的表裡的 region 編號（包括別的 renderer 的）、超出那一塊的範圍、反序列化失敗的內容、巢狀超過上限的值、超過上限的共用記憶體、寫到一半就斷線。Browser 都要關掉通道、報告 `RendererGone`，不能 panic、卡住或 stack overflow | `cargo test`，每次 |
| 平台層的跨 process 測試 | 假設，和 1、3、7 | 測試自己啟動小的 child process：建立、map、傳 handle、唯讀和大小的保證（拿唯讀的 handle 試著寫入或 map 成可寫、試著縮小 region，都要失敗）、對方整個結束、只關掉收的一半、只丟掉送的一半。macOS 另外測 rendezvous（沒登記的 pid 拿不到、同一個 pid 拿不到第二次、請求比登記早到時等得到）和假冒 kernel 的通知（忽略） | `cargo test`；Linux 和 Windows 在每天的 CI，macOS 在本機 |
| fuzz（cargo-fuzz） | 2，和 1、6、9 | 兩個 target：從「通道讀到的位元組」到「Browser 處理完這則訊息」的整條路（反序列化、分派、處理訊息的物件；惡意的 renderer 唯一碰得到 Browser 的路，那個物件不需要真的通道就能跑）；region 的參照 | 每週的 CI；改核心時在本機跑一段 |
| loom、shuttle | 5、6、8、9 | 送出的佇列和它的鎖（好幾條 thread 同時送、對方同時斷線）、同步呼叫等回答的位置、body 的訊息和「讀了多少」的往返（兩端各一條 thread）。atomic 和鎖的型別在核心裡是抽象的，測試時換成 loom 的 | 每週的 CI；改到這幾處時在本機 |
| TLA+ | 4、5、6、7、9 | 通道的協定的模型：兩端、有上限的佇列、任一端隨時斷線、region 的發出和收回、body 的管道的額度。TLC 檢查模型成立，再拿掉每一部分確認找得到反例 | 改協定時；模型和協定一起改 |
| Verus | 1 | 範圍的檢查和位置的運算寫成純函式（輸入數字，回答合不合法），證明「檢查通過的範圍一定在那一塊裡面」和 data pipe 的額度的運算（送出的不超過容量、讀了的不超過送的） | 每週的 CI |
| Miri | 1 | 核心的測試經過假通道在 Miri 底下跑：碰共用記憶體的 `unsafe` 沒有未定義的行為 | 每週的 CI |
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
2. **`ipc` crate**：核心（先做，經過假通道就能測）、平台層（Linux 的那一層做完時量 socket 的緩衝區和大的資料的門檻，「待決定」）、控制通道、帶 handle 的序列化、Browser 配共用記憶體和編號的表；用測試自己的 child process 測，包括惡意的對端。這個階段把驗證的工具架好（fuzz、loom 和 shuttle、Miri、Verus、TLA+ 的模型、`soundness.md`），核心的每一塊做完時它的那幾條性質就驗過。還沒接上 renderer。
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
    - **大的資料走留著重複用的 region，門檻 64 KiB**：送 1 MiB 給對方、直到它複製出來，留著重複用的 region 是 36 µs，放在訊息裡 105 µs，每次新建一塊 region 120 µs（8 MiB 是 317、1050、956 µs）；64 KiB 時是 6 µs 對 12 µs。門檻照 Chromium 的 `BigBuffer`。不選每筆資料新建一塊 region（起草時「Browser 送的放進唯讀的 region、連 handle 一起交給 renderer」的寫法）：比放在訊息裡還慢。不選 Mach 的 out-of-line 記憶體：8 MiB 時和 region 一樣快，收的一方拿到自己的一份，但只有 macOS 有，三個平台要同一個設計。

23. **tile 經過共用記憶體交給 Viz，Viz 上傳**（2026-11-20，T1，「tile 交接」）：compositor 照舊用 CPU 畫，像素寫進 Browser 配給它的 region（第 22 條的池子），frame 的 resource 帶 mailbox 和 region 的編號、位置和長度；Viz 複製出來、上傳，生命週期用 `ReturnResources` 和 `DestroySharedImage` 兩則訊息。canvas 的快照一樣。2026-11-20 的盤點確認 renderer 這一側沒有 wgpu，raster 是 vello_cpu 畫的 CPU 像素，所以 raster 的位置不用動。對照 Chromium：OOP-R 之前是這樣（one-copy raster），現在是 OOP-R（T3）。不選 T2（renderer 自己上傳、交出 texture）和 T3（OOP-R），理由在「理由和其他做法」。之後做 GPU raster 時換成 T3：那時過邊界的是 display list 不是像素，這一段本來就要重來（「之後的可能」）。

24. **tile 畫好就送「建立」的訊息，像素只複製一次、有計數守著**（2026-11-20，第 1 階段的 1b，「tile 交接」）：三則訊息（`CreateSharedImage`、緩衝區還回來、`DestroySharedImage`），編號由 renderer 取；raster 直接畫進要交出去的緩衝區，Viz 交給 GPU 是唯一的一次複製，測試用計數確認。對照 Chromium：`SharedImageInterface::CreateSharedImage` 是 client 取 mailbox、送一則訊息給 GPU process，frame 帶 mailbox 和 sync token（不同通道之間排順序用；這裡是同一條通道，不需要），用完 `DestroySharedImage`。不選像素跟著 frame 送（起草時的寫法）：少一種訊息，但上傳全部擠在收到 frame 的那一刻，捲動時新的 tile 多，那個 frame 要等它們都上傳完才能畫。不選先多複製一次、量過再省（2026-11-20 起先的建議）：使用者的意見是這種事之後容易忘記；raster 本來就畫進呼叫的一方給的緩衝區，直接做成一次不比多複製難多少，計數讓它之後不會悄悄變回去。

25. **bytes 的型別是明確的 `protocol::BigBuffer`，跨邊界的訊息裡的位元組欄位都用它**（2026-11-21，第 1 階段的 1c，「大的資料」）：`BigBuffer` 包一個 `Bytes`，自己實作序列化；單一 process 時就是 `Bytes`，跨 process 時 `ipc` 在編碼期間決定放訊息裡還是放進 region（第 22 條）。
    - **不選「不加型別，`ipc` 的序列化器攔 `serialize_bytes`」**：哪些欄位會走共用記憶體變成隱含的；`Vec<u8>` 在 serde 裡是一個一個 `u8` 的序列，攔不到，之後加的欄位會默默內嵌在訊息裡，沒有東西提醒。Chromium 也是明確的型別（mojom 裡宣告成 `BigBuffer` 的欄位才走共用記憶體）。
    - **裡面是 `Bytes`，不是 `Arc<[u8]>`**：`Bytes` 能不複製地切片，`ResourceResponse` 和 DevTools 的紀錄已經靠它共用。
    - **所有的位元組欄位都換，不只原本列的四種**：request 的 body（fetch 上傳的，大小沒有上限）和 history 的 state（通常小，頁面可以放很大的）也換。規則只有一句、`sanity.py boundary` 機械地擋；不選「小的留 `Vec<u8>`」，那要靠人判斷每個欄位夠不夠小。Chromium 這兩個是內嵌，大的 request body 另外走 blob 或 data pipe；machopo 沒有那幾套機制，走同一個型別。
    - **兩個不換**（`sanity.py` 的允許清單，附理由）：`CreateSharedImage` 的像素走 tile 自己的共用記憶體（第 23、24 條）；`CopyOutputResult` 的像素是 Viz 給 Browser 的，兩者在同一個 process。

26. **一則訊息的開頭固定 20 bytes，長度的上限 128 MiB，照實際收到的量配置**（2026-11-21，第 2 階段的 2a，「控制通道」）：開頭是長度（`u32`，開頭之後的位元組數）、目的地（`u32`）、序號（`u64`，每一端從 0 開始加一，不會繞回來）、handle 的數量（`u32`），little-endian。核心拿到的是「讀到的位元組」，自己切出訊息：socket 是串流，Mach 訊息一則就是一則，走同一段。
    - **上限 128 MiB**：和 Chromium 同一級（舊的 IPC 128 MiB、Mojo 的 channel 256 MiB）。大的位元組資料是 `BigBuffer`、走 region，但字串不是（DevTools 和 WebDriver 的訊息裡 script 的原始碼、執行的結果可以到幾 MiB）。不選 16 MiB：沒有清單保證所有訊息都在它以下，正常的大訊息會被當成壞訊息、分頁掛掉，之後每加一種帶大字串的訊息都要記得這個上限。
    - **不照開頭寫的長度先配置**：位元組到了多少緩衝區才長多少，謊報長度不花收的一端的記憶體；真的送滿時每個 renderer 最多讓 Browser 暫時拿著 128 MiB。上限是一個常數，之後量到訊息都很小，收緊只改它。

## 待決定

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

- **renderer 向 Browser 要共用記憶體時怎麼不卡住 compositor**：只發生在池子不夠的時候。可以做的：一次要一大塊、renderer 自己切；池子快用完就先要；要的時候不停下來等。還有 Browser 什麼時候收回一塊。
- **單一 process 時要不要連 Browser 的 IO thread 也拿掉**：處理訊息的邏輯抽成物件之後（「架構」）做得到：renderer 拿到的「送訊息的東西」在這個模式直接呼叫那個物件，由它交給網路或叫醒 UI thread，中間沒有佇列也沒有 thread。每個分頁少一條 thread，renderer 到 UI thread、到網路各少一次轉手。代價：Browser 的邏輯在 renderer 的 thread 上跑（main thread 和 worker 可能同時送，要加鎖；它 panic 時倒的是 renderer 的 thread）；`RendererGone` 改成靠最後一個 sender 被丟掉來報告；「從通道讀出來、交給物件」那一段只剩多 process 的測試跑得到。那條 thread 現在就在，不是這份加的成本，所以先量每則訊息的那一次轉手占多少再決定。比過的做法見 `details.md` 的「比過的做法」。
- **renderer 不能讓 Browser 當掉**：惡意的 renderer 最容易走的三條路，接上 renderer 的那一步要一起處理。
  - 沒有上限的東西：Browser 收 renderer 訊息的佇列、Browser 送給卡住的 renderer 的佇列、一個 renderer 能要的共用記憶體（「`ipc` crate」）、反序列化的巢狀深度（同上）。各要定一個數字，超過就關掉那個 renderer。
  - 處理訊息時 panic：Rust 擋得住記憶體錯誤，擋不住 `unwrap` 和越界。處理 renderer 的訊息的程式遇到不合理的內容要走壞訊息的路（`browser` 的 `BadMessageReason`），不能 panic。
  - 壞的 frame：frame 是在 Browser 裡交給 GPU 的，而 wgpu 的驗證錯誤現在會結束整個程式。Viz 收到 frame 時的檢查要涵蓋所有會走到 wgpu 的值；這一項單一 process 時就可以開始做。
- **trace 怎麼收**：每個 process 各寫一個檔案（`MACHOPO_TRACE` 加上 pid）、之後合併，或 child 把 span 送回 Browser。Chromium 是每個 process 的 tracing service 把資料送回 Browser 合成一份。
- **哪些模式預設多個 process**：看法是只有人用的視窗；headless、情境、`--automation-pipe` 和測試預設單一 process，CI 另外用多個 process 跑選定的幾套（下面的「哪幾套測試」）。
- **Linux 和 Windows 怎麼把控制通道的一端交給 child**：std 的 `Command` 沒有安全的 API 指定要繼承哪個 fd 或 handle。選項：spawn 前短暫拿掉那個 fd 的 close-on-exec、用 command-fds 這類 crate、當成 child 的 stdin 傳、Windows 上照 Chromium 讓 handle 可以繼承並把數值放在命令列。
- **Linux 上大的資料的門檻和 socket 的緩衝區**（2026-11-21，第 2 階段做 Linux 的平台層時量）：原型在 macOS 上量到，緩衝區設成 1 MiB 的 Unix socket 內嵌送 1 MiB 和留著重複用的 region 一樣快。在 Linux 上量同一組（64 KiB、1 MiB、8 MiB：socket 內嵌對 region），定兩件事：緩衝區設多大；64 KiB 的門檻在 Linux 上要不要不一樣。region 不在這個數字裡的理由不變：大的資料不佔住控制通道（frame 和輸入排在它後面）、不用把每個 renderer 在 kernel 裡的緩衝區開大；所以量到一樣快也不是把門檻拿掉的理由，只是門檻可以高一點。
- **Linux 的 memfd 要不要加 `MFD_NOEXEC_SEAL`**：2026-11-17 的看法：加，kernel 不認得就不加，Chromium 也是。
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
