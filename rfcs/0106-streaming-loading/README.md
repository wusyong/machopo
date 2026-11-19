# RFC 0106：串流的載入

- 開始日期：2026-11-20
- 狀態：🚧 已排程：2026-11-20
- 細節：`details.md`

## 摘要

現在每一層都等整份 body 收完才用：`net` 把回應接成一個 `Vec`、Browser 在回應結束才 commit、renderer 一次解碼、一次餵給 parser。這份做一條從網路到 renderer、有背壓的資料管道（Chromium 的 data pipe），讓文件和 iframe 收到 header 和 body 的開頭就 commit、邊收邊 parse、parse 到一半就能畫；子資源和 `fetch()` 照舊整份收完，之後的 RFC 建在同一條管道上。設計的決定在「已決定的事項」，分成四個階段做（「分階段」）。

## 動機

RFC 0103 的「架構上可能要先做的事」第 5 個：incremental parsing、`fetch()` 的 streams、影音、server-sent events 都要一條從網路到 renderer 的資料管道，現在沒有；拆 process（RFC 0004）時要跨 process 的也是這條管道（Chromium 的 Mojo data pipe）。先在單一 process 裡把管道的介面定下來，之後拆 process 只換它的實作。

### 現況

2026-11-20 讀程式得到的（逐項的位置在 `details.md` 的「machopo 現在的載入」）：

- **網路**：`net` 在一個 tokio 的 current-thread runtime 上，一塊一塊讀 HTTP 的 body，但全部接到同一個 `Vec`，收完才解 `Content-Encoding`（flate2、brotli 的 `read_to_end`）、沒有 `Content-Type` 時照整份 body sniff MIME。API 只有「做完時」的 callback（`LoadResult`），沒有「收到 header」、「收到一段」。沒有 HTTP cache。`file:` 一次讀完。
- **導覽**：Browser 在回應結束時才 commit（`on_navigation_completed`），`CommitNavigation` 帶著整份 body（`Bytes`）。iframe 經過 Browser 的 frame 樹，走同一條路。
- **renderer**：照開頭的 1024 bytes 決定 JS 或 wasm 的 engine；`decode_document` 一次解碼成一個 `String`（編碼：BOM、HTTP 的 `charset`、`<meta>` 的 prescan，都沒有時看整份 body 是不是合法的 UTF-8）；`parser.push` 一次推進去。parser 本身已經是 html5ever 的 `Tokenizer` 加 `BufferQueue`，能一段一段餵（`crates/html` 的測試 `input_may_come_in_pieces`），但 `advance_document` 第一次收到 `NeedsInput` 就當成結尾、呼叫 `finish()`。parse 在 commit 的那個 task 裡同步跑完（遇到要等的外部 script 才停），中間沒有畫面。
- **子資源**：圖片、樣式、script、worker 的 script、`fetch()`、`importScripts()` 都是一則 `ResourceResponse` 帶整份 body；圖片收完才解碼，`fetch()` 的 body 是 `Arc<[u8]>`，沒有 `ReadableStream`（RFC 0027 的第 4 條選了整份收完，把串流留給這裡）。
- **跨 process**：`ipc` 只有時鐘。RFC 0004 定了「大的資料放在 Browser 配的共用記憶體」，data pipe 在它的「之後的可能」。

### 目標

1. 一條從網路到 renderer 的資料管道：回應的 header 先到，body 一段一段到，有背壓（renderer 讀不快時網路停下），在單一 process 裡不多複製；介面照之後拆 process 時能換成共用記憶體的樣子。
2. 文件和 iframe 邊收邊 parse：收到 header 和 body 的開頭就 commit，body 的每一段經過串流的解碼器餵給 parser，parser 讓出時能畫，`DOMContentLoaded`、`load` 照收完的時機。
3. 管道設計成子資源和 `fetch()` 之後也能用，但這份不換它們（第 1 條）。

### 不在範圍內

- 拆 process 本身（RFC 0004）：這份只讓管道的介面能換成共用記憶體。
- HTTP cache、cookie。
- `document.write`（limitations 裡另一個原因：parser 和 script 用 `RefCell` 共用 DOM）：串流之後才有意義，另外做。
- `fetch()` 的 `response.body`（`ReadableStream`，Streams API）、圖片的漸進解碼、script 邊下載邊編譯：子資源照舊整份收完（第 1 條），各自之後的 RFC 建在這條管道上。
- 影音、server-sent events、WebSocket。

### 要解決的限制

`docs/limitations.md` 的：「Parser 和載入」的「Body 一次收完，沒有串流，也還沒有 incremental parsing」、iframe 的 HTML 一次解析整份 body（`moving-between-documents/` 的四份失敗）、「網路」的 `net` 收完整份 body 才解壓、「Script」的一次 parse 整個回應。「Bindings」的 `fetch()` 等整份 body、沒有 `ReadableStream` 不在這份（第 1 條）。

## 設計概要

對網頁的作者：大的文件、慢的伺服器邊收邊畫；伺服器分段送的頁面在兩段之間看得到前一段；`<head>` 先送的頁面先開始載入樣式和 script。

一次導覽的資料這樣走（每一步的理由在「已決定的事項」）：

1. `net` 讀 socket、解 `Content-Encoding`，寫進一條 data pipe（`Producer`，滿了就等：背壓）；收到 header（沒有 `Content-Type` 時 sniff 完）就 `on_response(head, consumer)`，結束時 `on_complete(status)`（第 2、4 條）。
2. Browser 從管道讀出開頭的 1024 bytes（或到結尾），和 head、管道的 `Consumer` 一起放進 `CommitNavigation`（第 3 條）。
3. renderer 照那一段選 engine、決定編碼、建文件（第 3、5 條），之後管道有資料時排一個 task：讀一段、用 encoding_rs 的 `Decoder` 解、切成小片推給 parser，超過時間預算或有輸入時讓出（第 6 條）；管道結束、`on_complete` 來了才 `finish()`，`DOMContentLoaded`、`load` 照舊在那之後。
4. 有了 `<body>`、擋住 render 的 sheet 都到了才畫第一次；在那之前顯示舊的畫面，最多 4 秒（第 6、7 條）。

子資源和 `fetch()` 用的 `fetch(done)` 和它共用同一個 loader，body 收完才回答，對它們和現在一樣。

## 詳細設計

每一條的決定和理由在「已決定的事項」，每個階段的細節到那個階段開始時再定。Chromium、Servo、Gecko 怎麼做在 `details.md`。

### 要決定的事（依序）

1. ~~範圍~~：文件和 iframe（第 1 條）。
2. ~~資料管道的模型~~：照 Chromium 的 data pipe（第 2 條）。
3. ~~commit 的時機~~：收到 header 和 body 開頭的 1024 bytes（或結尾）就 commit（第 3 條）。
4. ~~網路那一側~~：照 Chromium 的 `URLLoaderClient`（第 4 條）。
5. ~~編碼~~：commit 時照開頭的 1024 bytes 決定，之後用 encoding_rs 的 `Decoder` 一段一段解（第 5 條）。
6. ~~parser 什麼時候讓出、畫面什麼時候畫~~：照 Blink 的時間預算，進了 `<body>` 才讓出（第 6 條）。
7. ~~子資源和 `fetch()`~~：照舊整份收完（第 1 條），走包在新的 API 上的 `fetch(done)`（第 4 條）。
8. ~~換頁時新文件第一次畫之前顯示什麼~~：舊的畫面，最多 4 秒（第 7 條）。

### 測試

`net` 的測試伺服器已經能延遲 head 和 body（`Route::with_body_delay`）；`browser` 的 `navigation.rs` 端到端的測試；`html/semantics/scripting-1/the-script-element/moving-between-documents/` 裡因為一次 parse 而不穩定、失敗的那幾份。

### 分階段

每個階段做完，載入都照常能用；每個階段裡的段落和那一段要決定的事（管道的容量、一次讀多少、切片的大小……），到那個階段開始時再列。

1. **資料管道和 `net`**：單一 process 的 data pipe 放在 `ipc`（`ipc::data_pipe`：Chromium 的 data pipe 在 Mojo，拆 process 時在這裡換實作）；`net` 改成 `on_response(head, consumer)`、`on_complete(status)`（第 4 條），`Content-Encoding` 一段一段解、沒有 `Content-Type` 時先存開頭的 1024 bytes sniff、`file:` 一段一段讀；`fetch(done)` 包在上面。Browser 和 renderer 的行為不變。
2. **Browser 提早 commit**：收到 head 和開頭的 1024 bytes（或結尾）就 commit，`CommitNavigation` 帶著那一段和管道的 `Consumer`（第 3 條），iframe 一樣；導覽的取消、commit 之後收到一半的錯誤、DevTools 的 `Network` 事件（`dataReceived`、`loadingFinished`）的時機。renderer 這時先把管道讀到結尾，照舊一次 parse。
3. **renderer 邊收邊 parse**：管道有資料時排 task 讀、串流的 `Decoder`（第 5 條）；`advance_document` 收到 `NeedsInput` 時等下一段、管道結束才 `finish()`；時間預算、進了 `<body>` 才讓出、第一次畫要有 `<body>`（第 6 條）；`DOMContentLoaded`、`load` 的時機；DevTools 的文件原文邊收邊留；`moving-between-documents/` 的四份。
4. **舊畫面的時限、檢查和文件**：commit 之後 4 秒還畫不了就送空的 frame（第 7 條）；`<head>` 先到、body 晚到的頁面先畫出前一段的情境（`net` 的測試伺服器延遲 body）；`docs/limitations.md`、`docs/test-gaps.md`、`docs/architecture.md`；整套的檢查。

## 缺點

串流之後，載入的每一步都多了「還沒收完」的狀態：parser 停在一半、script 在文件還沒收完時執行、`DOMContentLoaded` 和 `load` 的時機、取消和錯誤在中途發生，要測的情況變多；管道有背壓之後，renderer 不讀時網路也停，忘了讀會卡住載入。

## 理由和其他做法

不做的話：大的文件要整份收完才看得到、記憶體裡同時有好幾份；`fetch()` 永遠沒有 streams；拆 process 時大的 body 每次整份複製。

## 先例

`details.md` 的「其他實作」：Chromium（Mojo data pipe、`NavigationBodyLoader`、`HTMLDocumentParser` 的時間預算、`TextResourceDecoder`、`BodyStreamBuffer`）、Servo（html5ever 的 `Tokenizer` 一段一段餵、`NetworkDecoderState`）、Gecko（另一個 thread 上的 stream parser、推測的 parse）。

## 已決定的事項

1. **範圍是文件和 iframe**（2026-11-20）：做從網路到 renderer 的資料管道（有背壓，介面照之後拆 process 能換成共用記憶體的樣子）、收完 body 之前就 commit（時機見第 3 條）、文件和 iframe 邊收邊 parse、parse 到一半就能畫。子資源和 `fetch()` 照舊整份收完，管道設計成它們之後也能用。對照 Chromium：文件、iframe、子資源和 `fetch()` 都走 data pipe；這份先做對載入速度影響最大的那一段（incremental parsing、第一次畫）。不選加上 `fetch()` 的 `response.body`：Streams API 本身是一份大的規範（WPT 的整個 `streams/`），另開 RFC 建在這條管道上；不選連圖片的漸進解碼、script 邊下載邊編譯也做：還要改 raster 和接 V8 的那一層，好處比不上 incremental parsing。
2. **資料管道照 Chromium 的 data pipe**（2026-11-20）：一條管道分成 `Producer` 和 `Consumer` 兩半，上限照 byte 算（Mojo 的 `CreateDataPipe` 的容量），寫的一端滿了就等，讀的一端讀走才讓出空間；producer 關掉是結尾，consumer 關掉（導覽取消）讓 producer 的寫回錯誤。async 只在 producer 那一側：`net` 本來就在 tokio 上，`write(chunk).await` 就是背壓（Chromium 的 `URLLoader` 等可寫的通知才再讀 socket）。consumer 那一側不等待：`read(max)` 回一段、還沒有或已經結束，`watch` 註冊「有資料時排一個 task」（Mojo 的 `SimpleWatcher`，`NavigationBodyLoader` 一個 task 讀一段），renderer 的 main thread 不放 async executor、parser 也不寫成 `async fn`：await 什麼時候繼續和 task、microtask、render 的順序要另外定，對不上 Blink。單一 process 的實作是 `Mutex` 裡的 `Bytes` 佇列加 tokio `sync` 的 `Notify`（兩個 thread 真的共用，所以用 `Arc`），片段直接交出、不複製；拆 process 時換成共用記憶體裡的 ring 和控制訊息，`read` 變成兩階段（先拿到 ring 裡的一段，用完再告訴對方用了多少），呼叫的地方照舊。自己寫，不用現成的 crate：`tokio::io::simplex`、`piper` 兩端都是 async、讀的時候複製進呼叫者的 buffer；channel（tokio 的 `mpsc`、`crossbeam-channel`）的上限算訊息、不算 byte；`ringbuf`、`rtrb` 沒有通知，也不能放進跨 process 的共用記憶體；`ipc-channel` 一段一則訊息、每段複製（Servo 的 fetch 這樣送）。不選 `protocol` 的訊息一段一段送、加上額度的流量控制（舊的 Chromium 的 `DataReceived` 加 ACK、Servo）：單一 process 裡用現成的 channel 加 semaphore 就做得到，但拆 process 時每段都要複製進訊息，要換成共用記憶體就得改介面和每個用它的地方；Chromium 也是從這個換成 data pipe 的。
3. **收到 header 和 body 開頭的 1024 bytes（或 body 結束）才 commit**（2026-11-20）：Browser 先從管道讀出這一段，和 header 一起放進 `CommitNavigation`，其餘的留在管道裡交給 renderer。renderer 收到 commit 時，頂層文件的 engine（開頭的 `<meta name=script content=wasm>`）、`<meta>` 的編碼 prescan 都能馬上決定，文件一次建在對的 engine 上；沒有 `Content-Type` 時的 MIME sniff 也用這一段（第 4 條）。對照 Chromium：有 `Content-Type` 時收到 header 就 commit（`OnResponseStarted`），commit 時就建一份空的文件；要 sniff 時 `MimeSniffingThrottle` 先讀一段才讓導覽繼續，這裡等於每次都這樣做。比 Chromium 晚的只有伺服器先送不到 1024 bytes 就停住、而且 HTTP 宣告了 charset 的頁面（沒宣告時 Blink 的 `TextResourceDecoder` 也要等 1024 bytes 才開始解碼）。不選收到 header 就 commit、renderer 等到 1024 bytes 才建文件：文件住在 engine 的 DOM heap 裡，engine 要看 body 的開頭才知道，中間有一段舊文件已經 unload、新文件還沒有的空檔（輸入、DevTools、網址列和 history 都要另外定）。不選收到 header 就 commit、先用現在的 engine 建空文件、之後發現要另一種再換：建文件有兩條路，commit 時做的每件事都要能重做。不選照舊等 body 收完：違反第 1 條。只有頂層文件選 engine，iframe 用頁面的 engine，但時機一樣，不分兩條路。
4. **`net` 的 API 照 Chromium 的 `URLLoaderClient`**（2026-11-20）：一個 load 依序告訴呼叫的人 redirect（現在的 `on_redirect`）、`on_response(head, body)`（`OnReceiveResponse`：不含 body 的 `ResponseHead` 和管道的 `Consumer`），最後 `on_complete(status)`（`OnComplete`：收到一半的錯誤、`encoded_body_length`、`response_end` 這些收完才知道的）。收到一半出錯時管道只是關掉，錯誤從 `on_complete` 來：管道只傳 byte，拆 process 時才換得成共用記憶體裡的 ring。現在的 `fetch(done)` 留著，和 `load` 共用同一個 loader，只是 body 收進一個 buffer、收完才 `done`，子資源和現有的測試不用改（第 1 條）。（實作時改的：原本寫「在 network thread 上把管道讀到結尾」，但那是同一個 thread 自己寫、自己讀同一條管道，多繞一圈、多一次 watch；結果一樣。）`Content-Encoding` 在 network thread 上一段一段解（flate2 的 `Decompress`、brotli 的串流 decoder；Chromium 的 `SourceStream`），寫進管道的是解好的。沒有 `Content-Type` 時，`net` 先存開頭最多 1024 bytes（或到結尾）sniff 完才 `on_response`，存下的照樣寫進管道（Chromium 的 `network::URLLoader` 也是在 network service 裡先 sniff 才給 head）。`file:` 一段一段讀進管道（`FileURLLoader`），`data:`、`blob:` 一次寫進去就關掉。不選只有一個 callback、`ResourceResponse` 的 body 換成 `Consumer`、錯誤和收完的數字也從管道來：管道裡除了 byte 還有別的，拆 process 時還是要另一條控制訊息，介面要再改一次（Mojo 的 data pipe 也只傳 byte，錯誤走 `URLLoaderClient`）。
5. **編碼在 commit 時照開頭的 1024 bytes 決定**（2026-11-20）：BOM、HTTP 的 `charset`、`<meta>` 的 prescan 照舊，都用 commit 帶來的那一段（第 3 條）；都沒有時，這一段是合法的 UTF-8（全是 ASCII 也算，結尾被切斷的多 byte 序列不算錯）就是 UTF-8，否則 windows-1252。之後用 encoding_rs 的 `Decoder` 一段一段解（字元切在兩段之間也對），編碼不再變。和現在（看整份 body 是不是合法的 UTF-8）不同的只有沒宣告、開頭 1024 bytes 全是 ASCII、後面才有 windows-1252 的字元的頁面：之後的字元變成 U+FFFD。對照 Chromium：沒宣告時 Blink 的 `TextResourceDecoder` 在開頭的資料上用 CED 猜一次，時機一樣，這裡用合不合法的 UTF-8 代替偵測器。1024 bytes 之後才出現的 `<meta charset>`（html5ever 的 `EncodingIndicator`）照舊忽略（規範要重新導覽，Blink 換掉之後的 bytes 的解法，Gecko 重新 parse），限制留著。不選等到第一個不是 ASCII 的 byte 才決定（ASCII 在兩種編碼裡一樣，先 parse）：結果和現在更接近，但多一個還沒決定的狀態，那段期間的 `document.characterSet`、子資源網址的 query、樣式和 script 退回的編碼都要另外定。不選加上 chardetng（Gecko、Servo 的偵測器）：猜得到 Shift_JIS、GBK 這些現在猜不到的，但多一個依賴、結果不同的頁面更多，和串流無關，要做另外做。
6. **parser 照 Blink 的時間預算讓出**（2026-11-20）：html5ever 的 tokenizer 一次吃完 `BufferQueue` 裡的、只在 script 停下，所以解好的文字切成小片（例如 16 KiB）推，每片之後看要不要讓出：超過預算（Blink 的 `HTMLParserScheduler`，桌面的前 6 次 10 ms、之後 500 ms）、或有排著的輸入（`PendingUserInput`，Blink 的 `ShouldYieldForHighPriorityWork`）就排一個 task 接著 parse。進了 `<body>` 才讓出（`HaveExitedHeader`）：`<head>` 的 `<link>` 和 script 一次找到、開始載入，不先畫一張空的頁面。第一次畫照 Blink 的 `ShouldScheduleLayout`：擋住 render 的 sheet 都到了（machopo 已經有），而且有 `<body>` 或 parse 完了。parser-blocking 的 script 跑完之後預算回到預設。預算和每片的大小由 `RendererFlags` 給，測試設成每片都讓出，結果不看機器的快慢。不選只在網路的兩段之間讓出（Servo）：簡單，但網路快或 `file:` 時一段可以到一次讀的上限，parse 成一個上百 ms 的 task，中間不畫也不處理輸入。
7. **換頁時舊的畫面最多留 4 秒**（2026-11-20）：commit 到新文件第一次畫之間（等 `<body>`、擋住 render 的 sheet），compositor 照舊顯示舊文件的最後一個 frame（現在就是這樣），對著它的輸入照舊丟掉；commit 之後 4 秒新文件還畫不了，renderer 先送新文件的一個空的 frame（背景色），之後照常，能畫時換上。對照 Chromium：`RenderWidgetHostImpl` 的 `kNewContentRenderingDelay`（4 秒沒有新的 compositor frame 就 `ClearDisplayedGraphics`）；計時器先放在 renderer 的 commit，拆 process 時再看要不要搬到 Browser。不選沒有時限：伺服器在 `<head>` 之後停住時，網址已經換了，畫面停在不能操作的舊頁面上，看起來像當掉。不選連 paint holding（Chromium 把最上層的 compositor commit 延到 FCP 或 500 ms）也做：要先有 FCP（什麼算畫出了內容，Paint Timing），machopo 還沒有；有 `<body>`、還沒有內容時會先畫一個空白，記在 `docs/limitations.md`，和 Paint Timing 一起做。
8. **回應分成 head 和 body 兩層**（2026-11-20，第 1 階段）：`protocol` 的 `ResponseHead` 是不含 body 的部分（Chromium 的 `URLResponseHead`），`on_response` 給它；整份收完的是 `ResourceResponse { head, body, encoded_body_length }`，包起來的 `fetch(done)` 給它。`encoded_body_length` 收完才知道，不在 head 裡（Chromium 的在 `URLLoaderCompletionStatus`）。讀欄位的地方從 `response.x` 改成 `response.head.x`（約 150 處，機械的）。對照 Chromium：`URLResponseHead` 不含 body，Blink 的 `Resource` 也把 response 和資料分開放。不選另外定義一個 `ResponseHead`、`ResourceResponse` 照舊、兩者互換：呼叫的地方不用改，但 12 個欄位和註解寫兩份，加欄位時容易對不上。不選 `ResourceResponse` 實作 `Deref<Target = ResponseHead>`：讀的地方不用改，但不是指標的型別用 `Deref` 藏起了結構，建構的地方照樣要改。
9. **管道的上限照 byte 嚴格守住，`net` 用 512 KiB**（2026-11-20，第 1 階段）：`data_pipe::create(capacity)` 帶容量（測試用很小的測背壓），`net` 的是 512 KiB（Chromium 的 `GetDataPipeDefaultAllocationSize` 的預設）。`write` 一段比剩下的空間大時，先放進放得下的（`Bytes::split_to`，不複製），其餘的等 consumer 讀走再放：還沒讀的永遠不超過容量，換成共用記憶體裡的 ring 時語意一樣（ring 只寫得到滿）。對照 Chromium：`URLLoader` 用兩階段的寫（`BeginWriteData`），從 socket 讀多少由剩下的空間決定。不選還沒讀的不到容量就整段放進去：少幾行，但上限變成容量加上一段 chunk（hyper 給的，可以上百 KiB），換成 ring 時語意不同。

## 待決定

每個階段開始時列那一段要決定的事（「分階段」）。

## 之後的可能

- `document.write` 和 insertion point（需要 parser 能在 script 裡重入）。
- preload scanner（parser 停在 script 時先找到後面的子資源）。
- 拆 process 時把管道換成共用記憶體裡的 ring（RFC 0004）。
- paint holding（最上層的 compositor commit 延到 FCP 或 500 ms），和 Paint Timing 一起（第 7 條）。
- server-sent events、影音的 `Range` 讀取、`Blob.stream()`、`Request.body` 的上傳串流。
