# RFC 0004：multi-process

- 開始日期：2026-10-04
- 狀態：⏸️ 延後：2026-10-06（第 8 條：預設單一 process，renderer 是 Browser process 裡的 thread；只做了第 0 步「整理邊界」，進度見 `progress.md`。IPC 的設計 2026-10-06 確定，2026-11-17 重新比過一次、改了 macOS 和 Windows 的平台層，整份改寫成現在的樣子；2026-11-20 把 sandbox 移到之後另一份 RFC（第 18 條）。排程之前先做「待決定」的前兩項：重新盤點和原型）
- 細節：`details.md`

原本是 MVP 選做的 P6（`rfcs/pre-rfcs/MVP.md` 的「里程碑」），2026-10-04 決定 MVP 到 P4 為止，改成這份 RFC。

## 摘要

把 renderer 從 Browser process 裡的 thread 拆成獨立的 process，三個平台都做。Renderer 和 Browser 是同一個執行檔，用 `--type=renderer` 啟動（照 Chromium）。把 renderer 關進作業系統的 sandbox 是之後另一份 RFC；這份的設計照「之後會有 sandbox」來做，但做完時還沒有。

IPC 自己做，照 Chromium 的 Mojo 的方向，但只取需要的部分：

- 每個 renderer 一條控制通道，用 Chromium 在各平台用的 primitive：Linux 的 Unix socket、macOS 的 Mach port、Windows 的 named pipe。
- 訊息照舊是 `protocol` 的 serde enum；大的資料放在共用記憶體。
- 加上 Mojo 不能假設的限制：只有 Browser 建立共用記憶體；renderer 不能送 handle，用編號稱呼它拿到的記憶體。
- 量過延遲之後，才決定要不要把小訊息也搬進共用記憶體裡的 ring。

Viz 留在 Browser process，不做 GPU process。單一 process 的模式留著，給 headless 和大部分的測試用，不付 IPC 的成本。

## 動機

**現況**（2026-11-17，對照 `docs/architecture.md` 的「Process 的邊界」和 `docs/limitations.md` 的「隔離」）：

- renderer 是 Browser process 裡的 thread，每個分頁一組。沒有任何隔離：V8 沒開 sandbox，V8 或 DOM 的 heap 的記憶體錯誤能碰到整個 browser 的記憶體。
- renderer 的 thread 結束時分頁顯示 sad tab、可以重新載入（`about:crash` 也在）；但記憶體耗盡、abort、記憶體錯誤這一類結束的是整個程式。
- 邊界大致已經在了：Browser 和 renderer 之間只有 `protocol` 的 message，都能序列化；renderer 不碰檔案系統和網路；compositor 直接把 frame 送給 Viz。規則和檢查（`sanity.py boundary`）是這份的第 0 步做的，例外只剩一個。

**還沒有的**：

- **tile 的交接**（剩下的那個例外）：compositor 同步呼叫 Viz 的 `SharedImageInterface` 建 texture，backing 是 `Arc<dyn Any>`，生命週期靠參考計數。
- **IPC 本身**：跨 process 的通道和共用記憶體。
- **啟動**：child process、命令列上的 `RendererFlags`、child 的 `main` 選 script engine。
- **process 當掉**：現在只處理 thread 結束。

**這份做完拿到什麼、拿不到什麼**：

- 拿到：一個分頁當掉、吃光記憶體、記憶體出錯，不影響 Browser 和其他分頁；別的分頁的資料不在這個 renderer 的記憶體裡。
- 拿不到：renderer 被攻破之後不能動使用者的電腦。獨立的 process 還是用使用者的身分在跑，照樣能開檔案、連網路；擋住它的是作業系統的 sandbox，那是之後的 RFC。拆 process 是 sandbox 的前提（sandbox 套在整個 process 上，Browser 自己需要檔案、網路和 GPU，不能被關起來），不是它的替代。

**要解決的 `docs/limitations.md` 裡的限制**：「隔離」的第二、三條（renderer 出事時整個程式結束、`gfx::FontData` 和 `gfx::ImageData` 不能序列化）；`docs/architecture.md` 的「Process 的邊界」的例外 1。「隔離」的第一條（沒有任何隔離）做完時只能改寫，不能刪：renderer 是獨立的 process 了，但 V8 的 sandbox 和作業系統的 sandbox 都還沒有。`rfcs/` 以外的文件不寫這份的編號，做完時照這份清單刪掉或改寫。

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

現在只有時鐘（`TimeTicks`）；這份把 channel 和共用記憶體放進來。對應 Chromium 的 `base/memory/*shared_memory*` 和 `mojo/core` 的一部分。

裡面的東西（通道、region、編號的表、對收到的訊息的檢查）不分 Browser 和 child，任何一端都用同一套。這份只有 Browser 和 renderer 兩種 process，下面就用它們來講；之後多了別種 process 時（「之後的可能」），它們也要把 renderer 當成不可信的，用的是同樣的元件。

**平台層**（細節見 `details.md` 的「平台層」）：

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| 建立共用記憶體 | rustix 的 `memfd_create`，加上 seal | mach2 的 `mach_make_memory_entry_64`（`MAP_MEM_NAMED_CREATE`） | windows-sys 的 `CreateFileMappingW`（pagefile） |
| map | memmap2 的 `MmapRaw` | mach2 的 `mach_vm_map` | windows-sys 的 `MapViewOfFile` |
| 控制通道 | std 的 `UnixStream::pair`，non-blocking | Mach port：兩端各有自己的 receive port 和通往對方的 send right | windows-sys 的 named pipe（overlapped） |
| 傳 handle | rustix 的 `sendmsg` 和 `ScmRights` | Mach 訊息裡的 port descriptor（memory entry 本身就是 port） | Browser 用 `DuplicateHandle` 複製進 renderer |
| 發現對方不在了 | 讀到 EOF、讀寫錯誤 | kernel 的 dead-name 和 no-senders 通知 | 讀寫失敗、讀到 0 個位元組 |

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

- 一則訊息是長度、目的地的編號、序列化的內容；有權限的一端還可以附帶 handle。
- 三個平台同一個形狀：IO thread 等「通道」和「喚醒」；送的時候不等，送不出去的自己排隊。這份每個 process 只有一條通道，但 IO thread 的迴圈不寫死成只等一條。
  - **Linux**：non-blocking 的 socket。handle 跟著同一次 `sendmsg` 送出；收的一端處理控制資料被截斷的情況，收到的 fd 用 `MSG_CMSG_CLOEXEC` 設成 close-on-exec。
  - **macOS**：一則訊息是一則 Mach 訊息。IO thread 阻塞在 `mach_msg`，等一個 port set（通道的 receive port 加一個自己的喚醒用的 port）；送的 timeout 是 0。在通往對方的 send right 上登記 dead-name、在自己的 receive port 上登記 no-senders，任一個到了就當成斷線；通知只認 kernel 送的（訊息的 audit token），對方可以送一則 id 相同的訊息來假冒。
  - **Windows**：overlapped 的 `ReadFile` / `WriteFile`，寫入一次只有一筆在進行。IO thread 等「讀完成」和「喚醒」兩個 event；結束時用 `CancelIoEx` 取消進行中的 I/O 再關 handle。Browser 送 handle 時自己用 `DuplicateHandle` 複製進 renderer 的 process，訊息只帶數值。
- 序列化：serde，格式見「待決定」。Browser 新發出的 region 序列化成 handle 表的 index，handle 本身走帶外；其他時候 region 只是編號，和一般的欄位一樣。
- 收的一端先檢查長度上限、目的地的編號（不認得的就是壞訊息）和 handle 的數量（對方沒有權限時只能是 0），再反序列化。反序列化時巢狀的深度有上限：訊息裡有可以一層包一層的型別（例如 `protocol::webdriver` 的 `WebDriverValue`，renderer 送的），不設限的話對方送一個巢狀很深的值就能讓收的一端 stack overflow，那不是 panic、接不住，整個 process 結束。不合的就當成對方壞掉，關掉通道（Chromium 的 `ReceivedBadMessage`）。

**ring**（第二步，量過才決定做不做，見「分階段」）：每個方向一個放在共用記憶體裡的 SPSC ring，平常的訊息直接序列化進去、不經過系統呼叫，控制通道只用來喚醒睡著的一端和傳 handle。照 shmem-ipc 的 `ringbuf.rs` 的設計自己寫：共用記憶體裡只放一個 atomic 的計數，讀寫的位置各自留在自己的 process 裡，對方能弄壞的只有計數，每次讀都檢查它不超過容量。細節見 `details.md` 的「ring 的設計」。

### 大的資料

- **`protocol` 加一個 bytes 的型別**（Chromium 的 `mojo_base::BigBuffer`）：單一 process 時是 `Arc<[u8]>`；跨 process 時小於門檻放在訊息裡，大於門檻放進共用記憶體。Browser 送的放進唯讀的 region、連 handle 一起交給 renderer；renderer 送的寫進 Browser 之前配給它的 region，訊息只帶編號、位置和長度。`CommitNavigation` 的 body 和子資源的回應用它。
- **從 renderer 來的大資料**：renderer 還能寫那塊記憶體，所以 Browser 一律先複製出來再用（Chromium 的 `BigBuffer` 的註解也警告這個 TOCTOU 的風險）。

### tile 交接

還沒決定（「待決定」的 T1 到 T3）；2026-10-04 的看法是 T1，one-copy raster：

- compositor 照舊 raster 成 CPU 像素，寫進 Browser 配給它的可寫的 region（renderer 留著一個池子重複用）。frame 的 `TransferableResource` 帶 mailbox 和 region 的編號、位置和長度；Viz 經過 Browser 的表找到那一塊，複製出像素、上傳，以 mailbox 為 key 記著 texture。
- 生命週期改成明確的訊息：Viz 用完時送 `ReturnResources`，compositor 丟掉 tile 時送 `DestroySharedImage`。現在的參考計數是它們在同一個 process 裡的替代。單一 process 的模式照舊用 `Arc`，但走同一組訊息。
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

假設之後的 sandbox 照 Chromium 現在的做法，在 child 的 `main` 一開始、任何其他程式碼之前就套用，不先「暖機」再鎖（`details.md` 的「Seatbelt 的兩種套用時機」）。所以上面每一樣都要在 sandbox 裡面拿得到，不能靠「鎖之前先拿好」。macOS 上這一點由原型先驗證（「待決定」）。

### process 當掉

sad tab、重新載入和 `about:crash` 已經有了，現在由 renderer 的 thread 結束觸發。拆成 process 之後：

- 控制通道斷了（各平台的訊號見「`ipc` crate」的表）時，IO thread 報告 `RendererGone`，Browser 放掉這個 renderer 的 region 和表。之後照現在的路：分頁顯示 sad tab，重新載入時啟動新的 renderer。
- renderer 送來壞掉的訊息也走同一條路，Browser 另外結束那個 process。
- `about:crash` 改成讓 child process 自己結束。

### 單一 process 的模式

`--single-process`，以及 headless 和測試的預設。renderer 的程式不知道自己在哪種模式。訊息直接走 crossbeam，大的資料是 `Arc`：不序列化、不經過 `ipc`，不付 IPC 的成本。IPC 那條路因此不在這個模式的測試裡，由用多個 process 跑的整套測試守著（「測試」）。

renderer 這一側沒有 IO thread：每條 thread 直接等自己的 crossbeam 佇列。Browser 這一側的 `Chrome_IOThread` 在這個模式也在（現在就是這樣），它等的是 crossbeam，推的是同一個處理訊息的物件（「架構」）；要不要連它也拿掉見「待決定」。

### 測試

- **message 都能序列化**：`protocol` 的 `boundary` 測試在編譯時確認（已經有）；`SharedImage` 的 `#[serde(skip)]` 拿掉之後才算完整。
- **單一 process 的測試照舊**：`DrivenViz`、reftest、`shell` 的端到端測試都不改，證明新的訊息（`ReturnResources`、`DestroySharedImage`）在同一個 process 裡行為一樣。
- **`ipc` 的平台層**：三個平台各自測建立、map、傳 handle、唯讀和大小的保證（拿唯讀的 handle 試著寫入或 map 成可寫、試著縮小 region，都要失敗）；跨 process 的部分用測試自己啟動的小 child process。CI 每天在 Linux 和 Windows 上跑，macOS 在本機。macOS 另外測 rendezvous（沒登記的 pid 拿不到 port、同一個 pid 拿不到第二次、請求比登記早到時等得到）和兩種通知（對方整個結束、只關掉收的一半、只丟掉送的一半，都報告斷線）。
- **惡意的對端**：測試扮演壞掉的 renderer，送超過上限的長度、不認得的目的地編號、附了 handle 的訊息、不在它的表裡的 region 編號（包括別的 renderer 的）、超出那一塊的位置和長度、反序列化失敗的內容、巢狀超過上限的值、超過上限的共用記憶體、寫到一半就斷線；macOS 上加上假冒 kernel 的通知。Browser 都要關掉通道、報告 `RendererGone`（假冒的通知是忽略），不能 panic、卡住或 stack overflow。另外確認 Browser 收回的 region 不會出現在別的 renderer 手上。
- **fuzz renderer 送來的位元組**：從「控制通道讀到的位元組」到「Browser 處理完這則訊息」是惡意的 renderer 唯一碰得到 Browser 的路，整條放進一個 fuzz target（反序列化、目的地的分派、處理訊息的物件），不只測手寫的那幾種壞訊息。處理訊息的邏輯抽成物件（「架構」）之後，這個 target 不需要真的通道就能跑。
- **整套的測試用多個 process 跑**：單一 process 的模式不經過 `ipc`，所以 CI 另外用多個 process 跑整套的測試。WPT 經過 machopodriver 啟動真的 `machopo`，讓它開 renderer process 就行；其他幾套見「待決定」。連啟動都算在內。
- **跨 process 的端到端測試**（`cargo test` 裡的少數幾個）：啟動真的 child process，載入一個頁面、點一下、捲一下；`about:crash` 之後 Browser 還在、分頁是 sad tab、重新載入可以恢復。
- **ring**（第二步）：協定用 loom 跑；讀取端用 fuzz，另一條 thread 同時亂寫共用記憶體裡的計數和內容。
- **soundness**：`ipc` 的 `unsafe` 的論證寫在它的 README 的「`unsafe` 的論證」，mutation test 弄壞驗證的地方，確認測試抓得到。
- **只能手動確認的**：活動監視器裡的 process，記進 `docs/test-gaps.md` 的「只做過手動確認」。

### 分階段

排進某個階段時再確認。排程之前先做「待決定」的前兩項（重新盤點、原型）。

0. **整理邊界**（2026-10-06 做完，`progress.md`）：邊界的規則和檢查；修掉 context lost 的 observer、`RendererFlags` 的函式指標、能送給任何 `FrameSinkId` 的 sender。
1. **message 化，還是單一 process**：tile 的交接改成訊息（拿掉 `SharedImage` 的 backing，加上 `ReturnResources` / `DestroySharedImage`、Viz 的 mailbox 表）、`protocol` 的 bytes 型別；Browser 處理一個 renderer 的訊息的邏輯從 `Chrome_IOThread` 的迴圈抽成物件。現有的測試全部照舊通過。
2. **`ipc` crate（IPC 的第一步）**：平台層、控制通道、帶 handle 的序列化、Browser 配共用記憶體和編號的表；用測試自己的 child process 測，包括惡意的對端。還沒接上 renderer。
3. **啟動 child process**：`--type=renderer`、renderer 的 IO thread、所有訊息走控制通道、tile 走共用記憶體、process 當掉。最後量捲動時每個 frame 的延遲，和一個高頻小訊息的 benchmark，給第 4 步用。
4. **ring（IPC 的第二步）**：第 3 步量到控制通道的系統呼叫值得省時才做。平台層和上層都不變，只換傳輸的核心。

## 缺點

- **IPC 要自己維護**：平台層三套（控制通道、傳 handle、共用記憶體、啟動、發現對方不在了，三個平台各寫一套，不共用）、帶 handle 的序列化、之後的 ring，加上對惡意對端的驗證，都是 machopo 的程式。
- **多了 `unsafe`**：`ipc` 的平台層。Linux 大多有 rustix 的安全 API；macOS 上 Mach 沒有安全的包裝，port 的擁有權、訊息的組裝、memory entry 的建立和 map 全是 FFI；Windows 上建立兩端、overlapped 的讀寫和取消、共用記憶體、複製 handle 都是 FFI。Windows 不是平常驗收的平台，這些只有每天的 CI 和審查守著。
- **安全的保證靠設計和測試**：對惡意 renderer 的防護靠測試、fuzz 和審查守住，沒有現成被大量使用的實作可以依靠。
- **兩種模式要一起維護**：平常開發跑的是單一 process，IPC 那條路要等 CI 的多 process 測試才看得到壞掉。
- **renderer 要共用記憶體得問 Browser**：要新的一塊時要等回覆（「待決定」）。
- 每個 renderer 多一份記憶體（V8 isolate、字型、raster 的快取）、一條 IO thread 和啟動時間。
- tile 經過共用記憶體再由 Viz 上傳（T1 的話），compositor 到 Viz 也多一次 IO thread 的轉手；比現在多幾次複製要照現在的上傳方式重算（「待決定」）。
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
- **process 的分法**：每個分頁一個 process。site isolation 要跨網站導航時換 process，先不做。
- **單一 process 的模式要不要留**：留。Chromium 也有 `--single-process`；machopo 的測試大多靠它（`DrivenViz` 直接扮演 Viz），全部改成跨 process 會又慢又難控制時機。

## 先例

細節見 `details.md` 的「其他實作」和它之後的幾節。

- **Chromium**：Browser、Renderer、GPU、Network 和 utility process；同一個執行檔以 `--type=` 區分；renderer 在各平台的 sandbox 裡；renderer 當掉時顯示 sad tab。IPC 是 Mojo，core 現在是 ipcz。machopo 照它的啟動方式、sandbox 的形狀、各平台的 channel 和共用記憶體的做法、三種 region 和 `BigBuffer`；不做 GPU process 和介面層。
- **Servo**：`--multiprocess` 時每個 pipeline 一個 content process，預設單一 process。IPC 是 ipc-channel，上面的 `generic_channel` 依模式選它或 crossbeam。machopo 照它「單一 process 時不序列化」的做法，不用 ipc-channel。
- **Ladybird、Firefox、WebKit**：都是自己做薄的一層，沒有直接用 Mojo。Firefox 只拿 Mojo 的路由演算法；Ladybird 在 macOS 上 2026-03 從 socket 換成 Mach port；WebKit 給 GPU process 用的是共用記憶體裡的 ring 加 semaphore。
- **shmem-ipc、Chromium 的 `ChannelLinux`**：給不可信的對端用的共用記憶體 ring，第二步的 ring 的先例。
- **iceoryx2**：資料留在共用記憶體、只傳位置，是「renderer 用編號稱呼 region」同一個想法；它假設對端合作，這份加上了檢查。

## 已決定的事項

2026-10-06 確認；標了 2026-11-17 的是那天重新比過之後改的或新加的。第 8 到 14 條是延後和第 0 步的決定。

1. **IPC 自己做，照 Chromium 現在的方向**，不用 ipc-channel、iceoryx2，也不直接用 Mojo。表面上是 channel，共用記憶體是一級公民。2026-11-17 重新查過這些選項的現況，結論不變。
2. **平台層的組合**（「`ipc` crate」的表；macOS 的部分 2026-11-17 改）：Linux 用 rustix 的 memfd 和 seal、memmap2；macOS 用 Mach 的 memory entry（原本是 rustix 的 POSIX shm）；Windows 用 windows-sys 的 pagefile section。Mach 的 FFI 用 mach2，版本和 wasmtime 的 fork 用的一樣。不用檔案撐共用記憶體。
3. **控制通道**（macOS 和 Windows 的部分 2026-11-17 改）：Linux 用 std 的 `UnixStream::pair`；macOS 用 Mach port（原本和 Linux 一樣）；Windows 用 windows-sys 的 overlapped named pipe（原本用 interprocess）。兩端都由 Browser 建立。三個平台送的時候都不等、IO thread 都等「通道」和「喚醒」；macOS 上對方不在了靠 dead-name 和 no-senders 兩種通知，只認 kernel 送的。
4. **IPC 分兩步**：第一步所有訊息走控制通道，大的資料放進共用記憶體；第二步把小訊息搬進共用記憶體裡的 ring。第二步要等第一步量過延遲才決定做不做，兩步用同一個平台層。
5. **ring 照 shmem-ipc 的 `ringbuf.rs` 的設計自己寫**，不依賴那個 crate。
6. **process 內的 channel 繼續用 crossbeam；renderer 加一條 IO thread**：IPC 收到的訊息由兩端的 IO thread 放進同一種 crossbeam channel，renderer 的 main thread 照舊只等它的 scheduler。
7. **`ipc` 可以寫 `unsafe`**：只在平台層和 ring 的存取，每一處都有 `// SAFETY:`，論證寫在 `ipc` 的 README。2026-11-17 起 macOS 和 Windows 的平台層幾乎全是 FFI，範圍比原本估的大。
8. **延後，預設單一 process**（2026-10-06）：拆 process、sandbox、IPC 都延後，只做第 0 步，把邊界守住。不選「繼續做」：先做 DOM API 和 CSSOM 等功能。
9. **邊界的規則和檢查**：規則和例外寫在 `docs/architecture.md` 的「Process 的邊界」，`python3 scripts/sanity.py boundary` 和 `protocol` 的 `boundary` 測試檢查。和 Chromium 用 `DEPS`、`checkdeps` 守住邊界同一類。
10. **第 0 步修掉小的例外，tile 的交接留給第 1 步**：小的修了同一個 process 的行為不變；tile 的交接牽涉共用記憶體的設計。
11. **context lost 由 Browser 轉成訊息**：Browser 向 Viz 登記 observer，收到時送 `CompositorMsg::ContextLost`；renderer 不登記。
12. **`BeginFrame` 的 closure 不算例外**：它由 Browser 建立、只送一則 `CompositorMsg`；拆 process 時由 `RenderProcessHost` 改成寫進控制通道，renderer 不必改。
13. **`RendererFlags` 只放能序列化的值，script engine 另外給**：拆 process 時旗標走命令列，engine 由 child 的 `main` 選。
14. **送給 Viz 的 sender 綁著自己的 `FrameSinkId`**：拆 process 時換成 Browser 的 IO thread 補上 id。
15. **單一 process 的模式不付 IPC 的成本，IPC 由多個 process 的整套測試守著**（2026-11-17）：renderer 是 thread 時訊息直接走 crossbeam，不序列化、不經過 `ipc`。IPC 那條路由 CI 用多個 process 跑整套的測試補上，至少是 WPT。
16. **macOS 上 child 經過 rendezvous 拿到 port，照 Chromium 的 `MachPortRendezvous`**（2026-11-17，「啟動」）。之後 sandbox 的 profile 要開放查這一個名字；原本打算完全不開 bootstrap 的查詢。
17. **renderer 不能送 handle，用編號稱呼 region**（2026-11-17，「`ipc` crate」的共用記憶體）。能不能附 handle 是通道每一端各自的權限，由 Browser 建通道時決定，renderer 那一端永遠沒有；這份只有 Browser 的那一端能附，所以 handle 只從 Browser 往 renderer 走。沒有權限的一端送來的訊息附了 handle 就是壞訊息。
18. **sandbox 移出這份，之後另一份 RFC**（2026-11-20）：三個平台的作業系統的 sandbox 和 V8 的 sandbox 都不在這份裡；原本包含 macOS 的。不選「這份只做 macOS」：三個平台不一致，每個平台的機制也沒有比過選項。這份保留 sandbox 對設計的要求（「為 sandbox 留的條件」）和原型在 macOS 的 sandbox 裡驗證一次；做完時拿到的是穩定和分頁之間記憶體分開，不是安全上最大的那一塊（「動機」）。

## 待決定

排程之前：

- **重新盤點**：這份的盤點是 2026-10-04 做的。之後多了分頁、worker、canvas 的 raster thread、DevTools，raster 從 Skia 換成 vello_cpu，所有模式都走 wgpu 和 shared image、上傳的路重做過。排程時照現在的程式重新看「還沒有的」和下面的 T，包括 `Chrome_IOThread` 現在管的東西（每個 renderer 的載入、worker 的登記和主 script、blob URL、壞訊息）要怎麼抽成物件（「架構」）。
- **原型**：在 macOS 上做，可以是 machopo 以外的獨立 crate。到 2026-11-20 為止，這份的設計都還沒有程式驗證過。要回答的：
  - 「為 sandbox 留的條件」成不成立：child 一開始就套一份最簡單的 Seatbelt profile（只開那一節列的東西），然後查 rendezvous 的名字、讀一個系統字型。正式的 sandbox 是之後的 RFC，這裡只確認這份的設計在 sandbox 裡跑得起來。
  - 放三種 process，不是兩種：一個 host、兩種 child，兩個 child 之間有 host 建的直接通道、共用一塊 host 建的記憶體。「附 handle 是每一端的權限」「目的地是可以增加的編號」「檢查和表任何一端都能用」這三件事在這個最小的例子裡就會被逼出來，比接上 machopo 之後才發現便宜。
  - 來回一則小訊息、送一塊大的資料、編和解一個真的 frame 各要多久；接上 machopo 之後再量捲動時每個 frame 多多少延遲。

設計：

- **tile 怎麼交給 Viz**：
  - **T1. 共用記憶體，Viz 上傳**（one-copy raster）：renderer 不碰 GPU，sandbox 最緊；raster 的位置不變。
  - **T2. 跨 process 的 GPU texture**（macOS 的 `IOSurface`）：renderer 自己上傳，sandbox 得開 IOKit；wgpu 沒有跨 process 共用 texture 的 API。
  - **T3. OOP-R**：renderer 送 display list，raster 在 Viz 那一側。Chromium 現在的做法，但 display list、字型和圖片都要能序列化。
  - 2026-10-04 的看法：T1。要和 GPU raster 的方向一起看：做 GPU raster 的話 raster 得在 GPU 那一側，過邊界的是 display list 不是像素，就是 T3。
- **Viz 讀 tile 的像素要複製幾次**：共用記憶體不能當成 slice 交給 wgpu（「`ipc` crate」），要用指標複製出來。現在上傳時 Viz 本來就留一份複製；raster 能不能直接畫進共用記憶體、整條路比現在多幾次複製，重新盤點時算。
- **renderer 向 Browser 要共用記憶體時怎麼不卡住 compositor**：只發生在池子不夠的時候。可以做的：一次要一大塊、renderer 自己切；池子快用完就先要；要的時候不停下來等。還有 Browser 什麼時候收回一塊。
- **單一 process 時要不要連 Browser 的 IO thread 也拿掉**：處理訊息的邏輯抽成物件之後（「架構」）做得到：renderer 拿到的「送訊息的東西」在這個模式直接呼叫那個物件，由它交給網路或叫醒 UI thread，中間沒有佇列也沒有 thread。每個分頁少一條 thread，renderer 到 UI thread、到網路各少一次轉手。代價：Browser 的邏輯在 renderer 的 thread 上跑（main thread 和 worker 可能同時送，要加鎖；它 panic 時倒的是 renderer 的 thread）；`RendererGone` 改成靠最後一個 sender 被丟掉來報告；「從通道讀出來、交給物件」那一段只剩多 process 的測試跑得到。那條 thread 現在就在，不是這份加的成本，所以先量每則訊息的那一次轉手占多少再決定。比過的做法見 `details.md` 的「比過的做法」。
- **renderer 不能讓 Browser 當掉**：惡意的 renderer 最容易走的三條路，接上 renderer 的那一步要一起處理。
  - 沒有上限的東西：Browser 收 renderer 訊息的佇列、Browser 送給卡住的 renderer 的佇列、一個 renderer 能要的共用記憶體（「`ipc` crate」）、反序列化的巢狀深度（同上）。各要定一個數字，超過就關掉那個 renderer。
  - 處理訊息時 panic：Rust 擋得住記憶體錯誤，擋不住 `unwrap` 和越界。處理 renderer 的訊息的程式遇到不合理的內容要走壞訊息的路（`browser` 的 `BadMessageReason`），不能 panic。
  - 壞的 frame：frame 是在 Browser 裡交給 GPU 的，而 wgpu 的驗證錯誤現在會結束整個程式。Viz 收到 frame 時的檢查要涵蓋所有會走到 wgpu 的值；這一項單一 process 時就可以開始做。
- **同步的請求**：現在 renderer 沒有任何地方同步等 Browser 的回覆。之後有（`document.cookie`、同步的 XHR）時，main thread 要能等某一則回覆而不處理別的訊息；Mojo 為此有專門的機制。
- **Linux 和 Windows 怎麼把控制通道的一端交給 child**：std 的 `Command` 沒有安全的 API 指定要繼承哪個 fd 或 handle。選項：spawn 前短暫拿掉那個 fd 的 close-on-exec、用 command-fds 這類 crate、當成 child 的 stdin 傳、Windows 上照 Chromium 讓 handle 可以繼承並把數值放在命令列。
- **macOS 上大的訊息怎麼送**：Chromium 是一頁以內內嵌，超過改用 out-of-line 的 descriptor。和 bytes 型別的門檻一起決定。
- **macOS 上要不要每則訊息都比對 audit token**：Chromium 會；這份的 port 都由 Browser 建立、send right 只交給那個 child。
- **Linux 的 memfd 要不要加 `MFD_NOEXEC_SEAL`**：2026-11-17 的看法：加，kernel 不認得就不加，Chromium 也是。
- **哪幾套測試在 CI 上用多個 process 跑**：WPT 可以。情境和 reftest 怎麼啟動、要全部還是一部分、多花多少時間，還沒確認。
- **序列化的格式**：看法：postcard（ipc-channel 0.21 也從 bincode 換成它）。要先量的是 frame：一個 `CompositorFrame` 有成百上千個矩形和矩陣，每個 frame 編一次、解一次，而 postcard 把整數一個一個編成不定長度；Mojo 的格式對整塊的數字幾乎是直接複製。原型時拿真的 frame 量編和解各要多久，太慢的話換固定長度的格式，或把大塊的數字當成位元組放。格式也要能設巢狀深度的上限（「`ipc` crate」的控制通道）。
- **bytes 型別的門檻**：Chromium 的 `BigBuffer` 是 64 KB；量過再定。門檻也決定大的訊息會不會擋住小的：四種訊息共用一條通道和一把鎖，main thread 送一則很大的訊息時，compositor 的 frame 和輸入的回應排在後面，所以大的內容要盡量走共用記憶體。
- **Browser 的 IO thread 要不要合成一條**：現在每個 renderer 一條，分頁多的時候 thread 跟著多；Chromium 是整個 process 一條處理所有通道。IO thread 的迴圈本來就不寫死成只等一條通道（「`ipc` crate」），合起來不用改設計；量到 thread 的數量是問題時再做。
- **第二步（ring）的門檻**：第 3 步量到控制通道的系統呼叫占多少才值得做。
- **背景 GC**：拆 process、有背景分頁時，V8 的 heap 在背景要不要 concurrent GC。
- **什麼時候排**。

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
- **data pipe**：串流的資料（子資源的回應、`fetch` 的 body）用共用記憶體裡的 ring 傳，Chromium 的 `URLLoaderClient` 就是這樣收 body；第二步的 ring 可以直接延伸。
- **Mojo 式的介面**：訊息多到 enum 難以管理時，在 `ipc` 上面加介面和 `Remote` / `Receiver`。
