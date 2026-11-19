# RFC 0106 的細節：串流的載入

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## machopo 現在的載入

2026-11-20 讀程式（`main` 的 `b4acdb8`）。

- **`net`**：一個叫 `NetworkService` 的 thread 上的 tokio current-thread runtime（`crates/net/src/lib.rs`），每個工作 `tokio::spawn` 一個 `url_loader::load`；API 只有 `fetch(request, done)`、`fetch_with_redirects(request, on_redirect, done)` 的 callback。`http.rs` 用 reqwest 的 `response.chunk()` 一塊一塊讀，接進同一個 `Vec`（`RawResponse.body`），收完才 `content_encoding::decode`（`read_to_end`）。沒有 `Content-Type` 時 `mime::sniff(&raw.body)` 看整份 body。redirect 在 `url_loader` 自己跟（最多 20 次），`fetch_with_redirects` 在每一次停下。`file:` 是 `std::fs::read`，`blob:` 在呼叫的 thread 上同步回答。沒有大小的上限，沒有 HTTP cache。
- **`protocol`**：`ResourceResponse` 的說明是「`URLResponseHead` 加上整份 body，Chromium 經過 data pipe 串流」，`body: Bytes`；`LoadResult = Result<ResourceResponse, NetError>`。`RendererMsg::CommitNavigation { .., body: Bytes, .. }`。子資源：`RendererHostMsg::FetchResource`、`CancelResource`、`FollowRedirect`，`RendererMsg::ReceivedRedirect`、`ResourceResponse`（只有「做完」，沒有收到 response、收到資料的訊息）、`DedicatedWorkerScript`。`network.rs` 的 `Iframe` destination 的說明還寫「renderer 自己載入」（RFC 0104 之後已經不對）。
- **Browser**：`WebContents::start_navigation` 用 `fetch_with_redirects`，`on_navigation_completed` 在回應結束時建 `CommittedDocument { body, .. }` 交給 `Committer::commit`（`crates/browser/src/render_process_host.rs`）。說明寫的是 `NavigationRequest::OnResponseStarted`，實際是回應結束。renderer 的 `FetchResource` 在 IO thread 上處理（`fetch_for_renderer`），回答一則訊息。
- **renderer**：`commit_document_navigation`（`crates/renderer/src/render_thread/documents.rs`）：`encoding::declares_wasm_scripts(body)` 看開頭的 1024 bytes 選 engine；`decode_document` 一次解碼（`encoding.rs`：BOM、`charset`、`<meta>` 的 prescan、都沒有時看整份 body 是不是合法的 UTF-8，否則 windows-1252）；`document::markup_for` 把非 HTML 的類型包成 `<pre>`；整份原文留給 DevTools；`Parser::new` 之後 `parser.push(&markup)` 一次，`advance_document`。`crates/html` 的 `Parser` 是 html5ever 的 `Tokenizer<TreeBuilder<..>>` 加 `BufferQueue`，`resume` 回 `NeedsInput` 或 `Script`，忽略 `EncodingIndicator`；`advance_document` 第一次收到 `NeedsInput` 就 `finish_taking_load_lines`、`document_parsed`。script：parser-blocking 的照 `prepare_script` 停下、`execute_parser_script` 之後接著 parse；樣式擋住 script（`PendingSheet.blocks_scripts`）；`defer`、`async`、module 各有自己的路；`DOMContentLoaded` 在 `run_deferred_scripts` 之後，`load` 在 `finish_load`（parse 完、沒有待載的樣式、子 frame、圖片）。iframe（`commit_child`）重複同樣的步驟。
- **子資源**：圖片收完之後讀大小、raster 時整份解碼（`image` crate）；樣式整份 parse；script 整份解碼；wasm 整份；`fetch()` 的 body 變成 `Arc<[u8]>`（`loader::fetched_response`），`dom_api` 的 `body.rs`：「body 是整份 bytes、只能讀一次（沒有 streams）」；worker 的 script 整份，worker thread 同步等。沒有 `XMLHttpRequest`、`ReadableStream`、`Blob.stream()`、`document.write`、preload scanner、web font。
- **跨 process**：`docs/architecture.md` 的「Process 的邊界」說導覽的 body 能序列化、跨 process 只是要複製（還寫 `Vec<u8>`，現在是 `Bytes`）。`crates/ipc` 只有 `TimeTicks`。RFC 0004（延後）定了大的資料放在 Browser 配的共用記憶體（`protocol` 的 bytes 型別，像 `mojo_base::BigBuffer`，`CommitNavigation` 的 body 和子資源的回應用它），handle 只從 Browser 給 renderer；data pipe 在「之後的可能」。
- **測試**：`crates/net/tests/it/`（`load.rs`、`redirects.rs`、`timing.rs`），測試伺服器的 `Route` 有 `delay`、`body_delay`、`close_delimited`；`crates/html/tests/it/parser.rs`；`crates/browser/tests/it/navigation.rs`（端到端）；WPT 的 `moving-between-documents/`（外部的 move-back 十二份標成 `[PASS, FAIL]`，空 `src` 的四份失敗）、`fetch/api/response/response-stream-*`（目錄在清單裡，預期失敗）；`streams/`、`xhr/`、`encoding/`、`document-write*` 不在清單裡。

## 其他實作

2026-11-20 讀原始碼（Chromium、html5ever、Servo、Firefox 的 `main`）。

### Chromium

- **導覽**：`NavigationURLLoaderImpl::OnReceiveResponse` 收到 `response_body`（`mojo::ScopedDataPipeConsumerHandle`），連同 `URLLoaderClientEndpoints` 交給 `OnResponseStarted`；`NavigationClient.CommitNavigation` 帶著它們，body 不經過 Browser（只轉交 handle）。iframe 每次導覽一個自己的 pipe。
- **renderer 讀 pipe**：`NavigationBodyLoader` 等 `MOJO_HANDLE_SIGNAL_READABLE`，`BeginReadData` → `BodyDataReceived` → `EndReadData`；一個 task 最多讀 `kMaxNumConsumedBytesInTask`（1 MiB），其餘排到下一個 task；`SetDefersLoading` 停止讀。`kThreadedBodyLoader`（在另一個 thread 讀和解碼）預設關掉。
- **DocumentLoader**：`BodyDataReceived` → `CommitData` → `parser_->AppendBytes`；`FinishedLoading` → `parser_->Finish()`。
- **data pipe**：ipcz 的實作是共用記憶體裡的 ring buffer，每次讀寫送一個「多了幾個 byte」的 parcel 給對方；consumer 有 READABLE、NEW_DATA_READABLE，producer 在有空間時 WRITABLE；兩階段的 `Begin/EndRead/WriteData`。容量預設 512 KiB，可以到 2 MiB（`services/network/public/cpp/loading_params.cc`）。
- **背壓**：`URLLoader::ReadMore` 在 pipe 滿的時候（`SHOULD_WAIT`）等可寫的通知，有空間才從 socket 讀，所以 pipe 滿了 socket 就停。
- **解碼**：`TextResourceDecoder::Decode` 只在 BOM、XML、CSS 的 charset 還沒決定時暫存；`<meta>` 的 prescan 在建 codec 之前做（`HTMLMetaCharsetParser`，至少看 1024 bytes，遇到 head 以外的標籤停下），之後 `kDoNotFlush` 一段一段解。HTTP 或 BOM 決定時不看 `<meta>`。之後才看到的 `<meta>` 讓之後的 bytes 換編碼（`SetEncoding`），已經解好的不重來（Gecko 會重新 parse）。
- **讓出**：時間預算，前 6 次 10 ms（Android 2 ms），之後 500 ms（Android 50 ms），照 Core Web Vitals 的實驗定的；`ShouldYieldForHighPriorityWork` 也讓出；`should_yield &= HaveExitedHeader()`：`<body>` 出現之前不讓出（除了送出 preload）。parser-blocking 的 script 跑完之後預算回到預設；`kForceSynchronousParsing`（fragment）、`file:` 一次跑完。
- **preload scanner**：`HTMLPreloadScanner` 在 main thread 的 `Append()` 裡，只掃第一段，除非 parser 停住。另一個 thread 的 `BackgroundHTMLScanner` 只找 inline script 預先編譯（預設關掉）。舊的 background HTML parser 已經沒有了。
- **第一次畫**：`Document::ShouldScheduleLayout` 要 render-blocking 的資源都到了、而且有 `body()` 或 parse 完；render-blocking 的資源只能在 body 之前加。paint holding：最外層的 main frame 的 compositor commit 延到 FCP 或 500 ms。所以 parse 到一半、進了 body、讓出時就能畫。
- **子資源**：圖片的 `ImageResource::AppendData` 在大小還不知道或可能是動畫時馬上更新，其他照 `kFlushDelay`（1 s）節流，partial 的 data 交給 `SetData(data, all_data_received)`；樣式收完才 parse；script 經過 `ResourceScriptStreamer` 一邊下載一邊交給 V8 編譯，執行等整份。
- **`fetch()`**：`ResourceLoader` 把 body 的 pipe 包成 `DataPipeBytesConsumer`（thread 綁定、兩階段讀、狀態有等待、可讀、關閉、錯誤）；`BodyStreamBuffer` 建 `ReadableStream::CreateByteStream`，JS 的 `pull` 從 consumer 讀一段 `enqueue`；`text()`、`arrayBuffer()` 經過 `FetchDataLoader`（字串一段一段解碼、ArrayBuffer 累積）。

### html5ever 和 Servo

- html5ever 0.40.1（2026-09-14）；0.28 起 tokenizer 和 `TreeSink` 都是 `&self`（為了重入的 `document.write`），`BufferQueue` 裡面是 `RefCell<VecDeque<StrTendril>>`。`parse_document` 的 `TendrilSink`（`process`、`finish`）忽略 `Script` 和 `EncodingIndicator`（「FIXME」），不能用在要執行 script 的文件；低階的 `Tokenizer::feed(&BufferQueue)` 回 `Done`、`Script`、`EncodingIndicator`，沒用完的留在 queue 裡。html5ever 只收 UTF-8，不依賴 encoding_rs。
- Servo 的 `ServoParser`：先暫存 1445 bytes 給 MIME sniff，之後每一段網路資料 `push_bytes_input_chunk` → `parse_sync`；`NetworkDecoderState` 暫存到 1024 bytes 或 1 秒，照規範的「決定編碼」（BOM、Content-Type、`<meta>` 的 prescan、parent 文件、chardetng），之後用 encoding_rs 一段一段解；`Script` 時提高 nesting level、`prepare`，有 parser-blocking 的 script 就暫停；`document.write` 有自己的 `BufferQueue`。沒有時間預算，在 feed 之間看要不要 reflow。可選的 `async_html` tokenizer 在另一個 thread 建樹、經過 channel 送操作。

### Gecko

- `nsHtml5StreamParser` 在 parser 的 thread 上 tokenize，把 tree op 送到 main thread 的 `nsHtml5TreeOpExecutor`；會越過 script 推測地 parse，`document.write` 打破推測時丟掉（失敗 100 次後停用推測）。無條件看的 `<meta>` 範圍也是 1024 bytes，用 chardetng，有重新 parse 的路。

## 參考資料

- Chromium：`content/browser/loader/navigation_url_loader_impl.cc`、`content/common/navigation_client.mojom`、`third_party/blink/renderer/platform/loader/fetch/url_loader/navigation_body_loader.cc`、`third_party/blink/renderer/core/loader/document_loader.cc`、`mojo/core/ipcz_driver/data_pipe.{h,cc}`、`services/network/url_loader.cc`、`services/network/public/cpp/loading_params.cc`、`third_party/blink/renderer/core/html/parser/{text_resource_decoder,html_meta_charset_parser,html_document_parser,html_preload_scanner,background_html_scanner}.*`、`core/loader/resource/image_resource.cc`、`modules/fetch/{body_stream_buffer,fetch_manager,body,fetch_data_loader}.cc`、`platform/loader/fetch/bytes_consumer.h`。
- html5ever：`html5ever/src/driver.rs`、`markup5ever/interface/{mod,tree_builder}.rs`、`markup5ever/util/buffer_queue.rs`；PR #548。
- Servo：`components/script/dom/servoparser/{mod,encoding,async_html,prefetch}.rs`。
- Firefox：`parser/html/nsHtml5StreamParser.{h,cpp}`。
