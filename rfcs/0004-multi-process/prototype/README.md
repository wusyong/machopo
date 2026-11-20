# multi-process 的原型

2026-11-20 做的原型，不合併，不在 machopo 的 workspace 裡。回答 `../README.md` 的「待決定」的原型那一項：這份的設計在 macOS 上、在 sandbox 裡跑不跑得起來，三種 process 時的形狀，和幾個要先量的數字（控制通道、大的資料、frame 的編解碼、ring）。

只有 macOS（macOS 26.6.2，Apple M4）。Linux 和 Windows 的平台層沒有原型。

```sh
cd rfcs/0004-multi-process/prototype
CARGO_TARGET_DIR=../../../target/mp-prototype cargo run --release
```

同一個執行檔：沒有參數是 host，`--type=renderer`、`--type=gpu` 是 child（host 自己啟動它們），`--sandbox` 讓 child 一開始就進 sandbox。跑一次約 30 秒，印出下面每一節的結果。

## 做了什麼

| 檔案 | 內容 |
| --- | --- |
| `src/msg.rs` | Mach 訊息的組裝和拆解，照欄位的位置逐一讀寫（kernel 的結構是 4 byte 對齊的）；port descriptor、out-of-line 的記憶體、audit trailer 裡的 pid |
| `src/channel.rs` | 控制通道的一端：自己的 receive port 和通往對方的 send right；dead-name 和 no-senders 兩種通知，只認 kernel 送的；「能不能附 right」是每一端各自的權限 |
| `src/rendezvous.rs` | 照 Chromium 的 `MachPortRendezvous`：host 用自己的 pid 當名字登記在 bootstrap，child 用 `getppid()` 查到、送請求，server 照 audit token 的 pid 回它登記的 right，一次 |
| `src/shm.rs` | Mach 的 memory entry：建立、map、另建唯讀的 entry；`RegionTable`（編號 → mapping，檢查位置和長度） |
| `src/ring.rs` | 共用記憶體裡的 SPSC ring：一個 atomic 的計數和「我在睡」的旗標，位置各自留在自己的 process |
| `src/main.rs` | host 和 child 的流程、每一節的情境和量測（最後兩節是同一個 process 裡的 crossbeam，和對方睡著時的來回） |
| `src/sys.rs` | mach2 0.6.0 沒有宣告的：`mach_port_request_notification`、`mach_port_set_attributes`、`audit_token_to_pid`、`MAP_MEM_NAMED_CREATE`，和 `sandbox_init` |
| `frames/` | machopo 的 compositor 對真的頁面送出的 4 個 frame（JSON），2026-11-20 用暫時加在 `protocol` 的鉤子抓的：1280×800 的視窗，`tests/sites/` 的 gutenberg、sqlite、todomvc 和 `about:demo`，載入和捲動 |

## 結果

### 設計在 sandbox 裡跑得起來

child 的 `main` 第一件事就是 `sandbox_init`，profile 只有 `(deny default)` 加三條：查 rendezvous 那一個名字、讀 `/System/Library/Fonts`、讀自己的執行檔。之後才做 rendezvous。

| | 沒有 sandbox | sandbox 裡 |
| --- | --- | --- |
| rendezvous 拿到 port、控制通道來回 | 可以 | 可以 |
| 讀系統的字型 | 可以 | 可以 |
| 讀 `/etc/passwd`、列出家目錄、在 `/tmp` 建檔案 | 可以 | 拒絕 |
| 連 `127.0.0.1` | 連得出去（對方拒絕） | 拒絕 |
| 啟動別的程式 | 可以 | 拒絕 |
| 查別的 bootstrap 服務 | 查得到 | 查不到（1102，`BOOTSTRAP_UNKNOWN_SERVICE`） |
| 自己建一塊 memory entry | 可以 | **可以** |

「為 sandbox 留的條件」成立。一個和設計的假設不同的地方：sandbox 擋不住 renderer 自己建共用記憶體（memory entry 沒有名字，Seatbelt 沒有對應的規則）。「只有 Browser 建立共用記憶體」在 macOS 上靠的是協定（renderer 的那一端不能附 right，建了也交不出去），不是 sandbox。

### 三種 process

host、一個 sandbox 裡的 renderer、另一種 child（`gpu`）。host 建好兩個 child 之間直接的通道（兩個 port 都在 host 建，各把一端交出去），和一塊 1 MiB 的 region：renderer 拿到可寫的、編號 1；另一個 child 拿到唯讀的 entry，記在「這個對端可以稱呼的 region」的表裡，也是編號 1。renderer 寫進去，在直接的通道上只說「編號、位置、長度」，另一個 child 照表複製出來。

| 情況 | 另一個 child 看到的 |
| --- | --- |
| 正常的參照 | 內容相符 |
| 表裡沒有的編號 | 壞訊息 |
| 超出 region 的範圍 | 壞訊息 |
| renderer 在訊息上附了 right | 壞訊息（那一端沒有權限） |
| 假冒 kernel 的 dead-name 通知，再送正常的 | 假的被忽略（audit token 的 pid 不是 0），正常的照收 |
| 把唯讀的 entry map 成可寫 | kernel 拒絕（17，`KERN_INVALID_RIGHT`） |
| child 再問一次 rendezvous、沒登記的 process 問 | 都拿不到 |
| child 被 kill | 另一端在 12 到 40 µs 內從 kernel 的通知知道 |

「附 right 是每一端的權限」「目的地是編號」「表和檢查任何一端都能用」三件事，在程式裡都只是 `Channel` 的兩個布林、訊息開頭的一個 `u32`，和兩邊共用的 `RegionTable`。

### 控制通道（Mach 訊息）

host 和 sandbox 裡的 renderer 之間，一去一回，各 20000 次：

| 大小（每個方向） | 中位數 | p99 |
| --- | --- | --- |
| 16、64 bytes | 4.2 µs | 7.7 µs |
| 1 KiB | 4.8 µs | 8.5 µs |
| 16 KiB | 8.2 µs | 12.3 µs |

### 大的資料

送給 renderer，直到它複製進自己的記憶體、回答為止（中位數）：

| 大小 | 放在訊息裡 | out-of-line | 留著重複用的 region | 每次新建一塊 region |
| --- | --- | --- | --- | --- |
| 64 KiB | 11.8 µs | 10.2 µs | 6.2 µs | 15.0 µs |
| 1 MiB | 105 µs | 104 µs | 36 µs | 120 µs |
| 8 MiB | 1050 µs | 320 µs | 317 µs | 956 µs |

- 留著重複用的 region 最快：只有兩次複製（寫進去、讀出來），不經過 kernel。
- 每次新建一塊 region 比放在訊息裡還慢：建 entry、map、第一次碰到每一頁、unmap 的成本蓋過了省下的複製。所以大的資料要走池子，不是每筆一塊。
- Mach 的 out-of-line 在 8 MiB 時和 region 一樣快，而且收的一端拿到的是自己的 copy-on-write 的一份（送的一方之後改不到）；但只有 macOS 有，三個平台要同一個設計，沒有用它。

### frame 的編解碼（postcard）

| frame | quad | resource | postcard | 編 | 解 | 編、Mach 訊息、解、回答 |
| --- | --- | --- | --- | --- | --- | --- |
| 真的，最小 | 15 | 7 | 543 bytes | 0.46 µs | 0.50 µs | 4.8 µs |
| 真的 | 21 | 5 | 633 bytes | 0.46 µs | 0.50 µs | 4.6 µs |
| 真的 | 30 | 27 | 2128 bytes | 1.1 µs | 1.6 µs | 8.5 µs |
| 真的，最大 | 46 | 38 | 2777 bytes | 1.4 µs | 2.3 µs | 8.0 µs |
| 編出來的（最大的 × 25） | 1150 | 950 | 68 KiB | 26 µs | 49 µs | 92 µs |
| 編出來的（最大的 × 100） | 4600 | 3800 | 273 KiB | 104 µs | 194 µs | 319 µs |

抓到的 28 個 frame 最多 46 個 quad：machopo 的 frame 是一個 render pass、每個 tile 一個 quad，比 RFC 起草時假設的「成百上千個矩形」小很多。一個 frame 跨 process 多花不到 10 µs，是 16.7 ms 的 0.05%。postcard 夠用；就算是編出來的 1150 個 quad 也只有 0.1 ms。

### ring

和同一對 process 之間的 Mach 訊息比（中位數）：

| | Mach 訊息 | ring，讀的一端醒著 | ring，讀的一端每次都睡 |
| --- | --- | --- | --- |
| 64 bytes 來回 | 4.2 µs | 0.21 µs | 4.3 µs |
| 16 KiB 來回 | 8.2 µs | 1.0 µs | 4.4 µs |
| 連續 10000 則 64 bytes，單向 | 2.9 ms | 0.27 到 0.47 ms | 0.27 ms |
| 真的 frame（46 quad）編、送、解、回答 | 8.0 µs | 3.5 µs | 7.8 µs |
| 16 MiB 當管道送（64 KiB 一段 / 1 MiB 的 ring） | 1.47 ms | 0.31 ms | 0.32 ms |

- 讀的一端醒著時 ring 快 20 倍；但它一睡著，喚醒就是一則 Mach 訊息，成本和直接送 Mach 訊息一樣。「看一陣子再睡」（量到約 150 µs）在來回不停的測試裡等於一直醒著，代價是那 150 µs 佔著一個核心。
- machopo 的訊息不是這種頻率：一個 frame 一兩則，每則之間隔幾 ms，讀的一端一定睡了。所以 ring 省不到，除非讓 IO thread 空轉。
- 連續送一大批時 ring 快 6 到 11 倍（對方還在讀，不用每則都進 kernel）。
- 當管道送 body 時 ring 快 4.8 倍；但 Mach 訊息一段一段送也有每秒 11 GiB，比網路快得多。
- Mach port 的佇列上限（預設 5 則，可以調到 1024）在送的一方肯等的時候沒有差別。

### 和單一 process 的 crossbeam 比

同一個 process 裡兩條 thread、兩個 crossbeam 的 channel，也就是 machopo 現在單一 process 模式的走法：訊息本身搬過去，不序列化、不複製，大的資料是共用的 `Arc`。

兩邊都醒著（來回不停）時，中位數：

| | crossbeam（兩條 thread） | Mach 訊息（兩個 process） | ring，醒著（兩個 process） |
| --- | --- | --- | --- |
| 64 bytes 來回 | 0.21 µs | 4.2 µs | 0.21 µs |
| 16 KiB 來回 | 0.21 µs（搬的是指標） | 8.2 µs | 1.0 µs |
| 連續 10000 則 64 bytes | 0.15 ms | 2.9 ms | 0.27 到 0.47 ms |
| 真的 frame（46 quad）來回 | 0.21 µs（不編不解） | 8.0 µs | 3.5 µs |
| 1 MiB 交給對方 | 0.33 µs（共用，不複製） | 36 µs（重複用的 region） | — |
| 16 MiB 當管道，64 KiB 一段 | 0.011 ms（共用，不複製） | 1.47 ms | 0.31 ms |

對方已經睡著時（每次隔 1 ms 才送，machopo 的訊息平常是這樣到的），64 bytes 來回：

| | 中位數 | p99 |
| --- | --- | --- |
| crossbeam（兩條 thread） | 23.8 µs | 39.0 µs |
| Mach 訊息（兩個 process） | 24.8 µs | 38.8 µs |

- 兩邊都醒著時，crossbeam 比 Mach 訊息快 20 倍，和醒著的 ring 一樣；大的資料差得更多，因為同一個 process 裡完全不用複製。這是單一 process 模式不走 IPC 的理由。
- 對方睡著時兩者一樣：時間幾乎都花在把一條睡著的 thread 叫醒（約 24 µs），跨不跨 process 沒有差別。crossbeam 的接收端也是先空轉一下才睡，所以上一張表量到的是它還沒睡的情況。
- 所以拆成 process 之後，一則訊息多出來的成本是編和解（真的 frame 約 4 µs）加上 kernel 裡的複製，叫醒 thread 的那一段本來就在付。

### machopo 實際的訊息有多密

ring 只在收的一端醒著時才快，所以要知道 machopo 的訊息到得多密。2026-11-20 在單一 process 的 machopo 上量：暫時在四個地方記下每則訊息的時間（Browser 送給 renderer 的 main thread 和 compositor、renderer 送給 Browser 的 IO thread 和 Viz），這些就是拆開之後會走控制通道的訊息。用 machopodriver 照人的節奏操作一個分頁：載入頁面、每 16 ms 一個滾輪事件捲幾秒、停 1 秒。

| 階段 | 往 Browser | 往 renderer |
| --- | --- | --- |
| 閒置（1 秒） | 0 則 | 0 則 |
| 捲動長的頁面（60 Hz，6 秒） | 每秒 185 則；間隔的中位數 0.58 ms；48% 在前一則的 150 µs 以內，52% 隔 1 ms 以上 | 每秒 82 則；間隔的中位數 17 ms；82% 隔 1 ms 以上 |
| 捲到底、畫面不動的頁面 | 每秒 61 則，間隔約 19 ms | 每秒 54 則，間隔約 20 ms |
| 載入有 38 個子資源的頁面（本機的檔案） | 1 秒內 61 則；73% 在前一則的 25 µs 以內（請求一口氣發出） | 1 秒內 49 則；72% 在 25 µs 以內 |

- 一個 frame 往 Browser 是三到四則（`SetNeedsBeginFrame` 兩次、`SubmitCompositorFrame`、`InputEventAck`），其中幾則緊接著到；往 renderer 是一到兩則（`BeginFrame`、輸入事件），之間隔一個 frame 的時間。
- 所以讀通道的 thread 大部分時間是睡的：每秒不到 200 則、每則處理幾 µs，醒著的時間不到千分之一。只有緊接著到的那幾則（捲動時約一半，載入時約四分之三）碰得到它還醒著。
- 這些緊接著的訊息全部改走 ring，每則省約 4 µs：捲動時每秒省不到 0.4 ms，載入一頁省約 0.15 ms。
- 這是一個分頁、沒有開 DevTools 的情況。訊息會更密的情況：DevTools 開著（它的訊息很多）、大量 `fetch()` 的頁面、之後 Browser 的 IO thread 合成一條處理所有分頁時。
- 情境的執行器把 80 個滾輪事件在 20 ms 內送完時（不是人的節奏），84% 到 88% 的訊息在前一則的 150 µs 以內：爆量時讀的一端確實一直醒著。

## 寫的時候碰到的

- **Mach 訊息要自己照位置組**：`#[repr(C)]` 的結構在有指標的 descriptor（out-of-line）時和 kernel 的 4 byte 對齊不同，mach2 的結構只有 header 能直接用。
- **mach2 0.6.0 缺五樣**（`src/sys.rs`），正式做時要自己宣告。
- **no-senders 通知的 `sync` 參數要給 1**：給 0 的話，port 還沒做出第一個 send right 時（rendezvous 還沒把它交給 child）kernel 馬上就通知「沒有 sender 了」。
- **同一個 task 裡，一個 port 的 receive right 和 send right 是同一個名字**：把 receive right 移給 child 之後，host 手上那個名字就是通往 child 的 send right，不用另外記。
- **喚醒可能多送一則**：讀的一端設了旗標、發現有資料就不睡了，寫的一端可能已經看到旗標、送了喚醒。之後在控制通道上讀到沒人等的喚醒要略過。

## 沒有做的

- Linux 和 Windows 的平台層。
- port set 和 IO thread 自己的喚醒 port；送的時候不等（timeout 0）、送不出去自己排隊、`MACH_NOTIFY_SEND_POSSIBLE`。原型的送都是等到送出去。
- ring 對惡意對端的檢查只有計數的範圍；loom 的測試、序號、每個目的地的長度上限都沒有。
- 接上 machopo 之後，捲動時每個 frame 實際多多少延遲（thread 之間的轉手、優先權）。
- 大的訊息在 macOS 上怎麼送的門檻、每則訊息比對 audit token 的成本（原型每則都比對了，包含在上面的數字裡）。
