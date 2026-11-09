# RFC 0075 的細節

這份是 `README.md` 的補充：調查的紀錄、和其他實作的比較。結論和決定以 `README.md` 為準。

## kun 現在的自動化怎麼走

2026-10-22，`main` 的 `c92058f1`。

**pipe 的一則命令**：

1. `AutomationPipeReader`（`crates/shell/src/automation_pipe.rs`）從 stdin 讀到 NUL，`Command::from_json`（`crates/protocol/src/automation.rs`）解析成型別：`Method` 的 enum，`from_name` 對名字，參數的 struct 都是 `deny_unknown_fields`（多一個參數是 `INVALID_PARAMS`），不認識的方法是 `METHOD_NOT_FOUND`。
2. 沒有平台視窗時，`windowless::drive`（`crates/shell/src/windowless.rs`）的 `select!` 同時等 renderer 的訊息和命令；有視窗時命令經過 winit 的 `ShellEvent::Automation`（`crates/shell/src/window.rs`）。
3. `AutomationSession::command` → `dispatch`（`crates/shell/src/automation_session.rs`）：當場回答、之後回答（`Reply` 帶著 session 和送出佇列），或交給輸入。
4. 要 renderer 做的：`Kun` 的 WebDriver 方法經過 `WebContents::webdriver`（`crates/browser/src/web_contents.rs`）送 `RendererMsg::WebDriver`；`Profiler.*` 送 `RendererMsg::Profiler`（`crates/protocol/src/renderer.rs`）。回答是 `RendererHostMsg::WebDriverReply`、`ProfilerReply`，經過 browser 的 IO thread 回到 `on_renderer_message`，照 `request_id` 回答。
5. `AutomationPipeWriter` 把回答和事件寫到 stdout，每則之後一個 NUL。

**session 和 target**：`AutomationSession` 只有一個 client。`Target.attachToTarget` 只收 `flatten: true`，session 的 id 是遞增的數字（32 位的十六進位）；target 只有分頁（`type: "page"`），id 是 `TabId` 的十六進位，main frame 的 `frameId` 和它一樣。子 frame 在 automation 裡沒有 id。

**renderer 的事件迴圈**（`RenderThread::run`，`crates/renderer/src/render_thread.rs`）：main thread 只有一個 `crossbeam` 的 `Receiver<MainThreadTask>`（`crates/renderer/src/task_queue.rs`：`Browser(RendererMsg)`、`Compositor`、`WasmCompiled`、`HeapTaskPosted`），`recv_timeout` 等到下一個計時器。每個 task 之後跑 microtask，task 之間處理計時器、WebDriver 的結果、idle callback 和 frame 的排程。

- `RendererSender::send` 在送的那一個 thread 直接把 task 放進 main thread 的佇列；它的註解說拆成 process 之後由 IPC 的 thread 放。
- 沒有巢狀迴圈或同步的等待：沒有 `alert`、同步的 XHR；async 的 WebDriver script 在迴圈的開頭檢查，不等。
- 沒有用到 V8 的 `request_interrupt`、`terminate_execution`。
- renderer 這一側沒有能在 main thread 忙著時收 browser 訊息的 thread：compositor thread 有自己的佇列，收的是 `CompositorMsg`。

**realm 的建立和消失**：

- main frame：`ScriptEngine::create_realm`（`crates/dom_api/src/engine.rs`），在 `commit_navigation` 和 `commit_child` 裡呼叫。
- script 執行中插入的 iframe：`dom_api` 的 `create_child_realm`，再由 `FrameRequests::did_create_child_frame`（`crates/renderer/src/frame_view.rs`）收下，`absorb_own_child_frames` 變成 `FrameView`。
- 消失：realm 在 `FrameView` 的 `Rc<Realm>` 裡，跟著 view 消失：main frame 被取代、`children.retain(is_fully_active)`、`drop_child_frames`、`Drop`。沒有「realm 消失」的回呼。
- frame 的 id：`FrameView.frame_token`（`DocumentId`）跨導覽不變；renderer 給 `Kun.switchToFrame` 的是 `<每個 renderer 隨機的前綴>.<token>`，和 browser 的 main frame id 格式不同。

**依賴**：workspace 已經有 hyper 1（`http1`、`server`）、hyper-util、tokio（`rt`、`sync`）、httparse、base64、sha2、ring。沒有 websocket 的 crate，也沒有 `sha1`。machopodriver 的 HTTP（`crates/driver/src/http.rs`）是 hyper 的 `http1::Builder::serve_connection`，在自己的 thread 上跑單 thread 的 tokio。`python3 scripts/sanity.py deps` 只管 V8 和 wasmtime 在哪些 crate，不管網路的 crate。

**console 和例外**：`console` 是宣告表的 namespace（`dom_api` 的 `console` 模組，第 2 段刪掉了），每次呼叫組成一行字，經過 `FrameClient::console_message` 由 renderer 印到 stderr；值的格式照 inspector 的寫法自己做（`dom_js` 的 `console` 模組，第 2 段刪掉了）。沒接住的例外在 `crates/dom_js/src/exception.rs` 印成 `[uncaught] …`。程式裡沒有用到 `v8::inspector`。

## rusty_v8 的 inspector binding

`../third_party/rusty_v8/src/inspector.rs` 是上游的 binding（Deno 用的），kun 的 fork 沒有改過（`README.machopo.md` 的「Local modifications」沒有 inspector）。V8 的 build 一直包含 inspector（`v8/BUILD.gn` 的 `v8_base` 依賴 `src/inspector`，沒有開關），所以現在的 artifact 就有。

**有的**：

- `V8Inspector`：`create`、`connect`（context group、`Channel`、state、`V8InspectorClientTrustLevel`）、`context_created`（group、名稱、`auxData`；不能給 `origin`）、`context_destroyed`、`idle_started`、`idle_finished`、`async_task_*`、`exception_thrown`、`create_stack_trace`。
- `V8InspectorSession`：`dispatch_protocol_message`、`can_dispatch_method`、`release_object_group`、`wrap_object`、`unwrap_object`、`schedule_pause_on_next_statement`、`cancel_pause_on_next_statement`、`add_inspected_object`。
- `V8InspectorClientImpl`：`run_message_loop_on_pause`、`quit_message_loop_on_pause`、`run_if_waiting_for_debugger`、`generate_unique_id`、`console_api_message`（level、訊息、網址、行、列、stack）、`value_subtype`、`description_for_value_subtype`、`ensure_default_context_in_group`、`resource_name_to_url`。
- `Channel`：`send_response`、`send_notification`、`flush_protocol_notifications`。

**缺的、和這份有關的**：

| 缺的 | 影響 | 什麼時候補 |
| --- | --- | --- |
| `V8InspectorClient::currentTimeMS` | C++ 的預設回答 0：`console.time`、`Runtime.consoleAPICalled` 的 `timestamp`、`exceptionThrown` 的時間都是 0 | 第 2 段 |
| `connectShared` | `connect` 的 session 在 `run_message_loop_on_pause` 裡被刪掉不安全（V8 的 header 把 `connect` 標成 deprecated）；它的 `Channel*` 版本也只「部分」確保（session 可能活得比 kun 的 `Channel` 久），完全安全要 `ManagedChannel` | 不綁：kun 在巢狀迴圈回來之後才刪 session（`README.md` 的「已決定的事項」第 18 條） |
| `exceptionRevoked` | promise 的 rejection 後來被處理時，DevTools 的 Console 不會把那則錯誤劃掉 | 第 2 段 |
| `connect` 的 `SessionPauseState` | 不能讓新的 session 停在第一行之前（`waitForDebuggerOnStart`） | Playwright |
| `V8InspectorSession::state` | 斷線再接時保留 session 的狀態（Chromium 在換 renderer 時用） | 不需要 |
| `consoleClear`、`consoleTime*`、`memoryInfo`、`installAdditionalCommandLineAPI` 等 | Blink 用它們做 DevTools 的附加功能（例如 `performance.memory`、`$0`） | 之後的 RFC 需要時 |

`../third_party/rusty_v8/src/crdtp.rs` 也有 CBOR 的 `UberDispatcher`（Deno 用 `deno_inspector` 的 protocol 產生的程式），kun 不需要：kun 在 browser 和 renderer 之間傳 JSON 字串。

## Chromium 的對照

| kun（這份之後） | Chromium | 差別 |
| --- | --- | --- |
| `DevToolsHandlerThread`：HTTP、websocket、`/json/*` | `DevToolsHttpHandler`（自己的 thread）、`DevToolsSocketFactory` | — |
| pipe 和 websocket 的 client | `DevToolsPipeHandler`、websocket 的 connection，都是 `DevToolsAgentHostClient` | — |
| UI thread 的 session：browser 有型別的方法，其他的轉給 renderer | `DevToolsSession` 加上 `content/browser/devtools/protocol/*_handler.cc`，handler 回答 `FallThrough` 的送到 renderer | Chromium 的 handler 從 `browser_protocol.pdl` 產生；kun 手寫 |
| `RendererMsg::DevToolsMessage`、`RendererHostMsg::DevToolsMessage`（JSON 字串） | mojo 的 `blink.mojom.DevToolsAgent`、`DevToolsSession`（CBOR） | — |
| renderer 的 DevTools session、`V8InspectorSession` | Blink 的 `DevToolsAgent`、`DevToolsSession`、`InspectorSession`（agent 和 `V8InspectorSession`） | kun 這份還沒有 Blink 的 agent |
| `V8PerIsolateData` 裡的 `V8Inspector` | `MainThreadDebugger`（`ThreadDebugger` 的子類別，`V8InspectorClient`） | — |
| 停住時的巢狀迴圈 | `ClientMessageLoopAdapter`（`web_dev_tools_agent_impl.cc`）、`ScopedPagePauser` | — |
| DevTools 的佇列加 `RequestInterrupt` | `InspectorTaskRunner`、`DevToolsSession` 的 IO session | — |
| `console` 經過 V8 的 builtin 和 inspector（C1 時） | `V8Console`，`MainThreadDebugger::consoleAPIMessage` 變成 `ConsoleMessage`，`--enable-logging` 時印出 | — |

## websocket 的做法

2026-10-22 選了 W2（`README.md` 的「已決定的事項」第 3 條），理由是之後網頁的 `WebSocket` API 和這裡共用一套。下面是比較時的紀錄。

**兩種 websocket**：DevTools 要的是 server（DevTools 連進 kun），網頁的 `WebSocket` 是 client（頁面連到外面），放在 network service，要規範的全部：二進位、`permessage-deflate`（Chrome 預設會談）、subprotocol、`wss`、proxy、cookie、CSP 和 mixed content、close code、`bufferedAmount` 的背壓。Chromium 把兩者分開：DevTools 用 `net/server/web_socket.cc` 的精簡 server，網頁用 `net/websockets/`（`WebSocketChannel`、`WebSocketBasicStream`、`WebSocketDeflateStream`）；兩者共用的只有 frame 的編碼和解碼。WebDriver BiDi 之後也走 websocket，和 DevTools 一樣是只送文字 JSON 的 server。

CDP 用到的 websocket（RFC 6455）：HTTP 的 upgrade 握手（`Sec-WebSocket-Key` 加上固定的 GUID 做 SHA-1、base64），文字的訊息（可能分成好幾個 frame）、client 送來的 frame 有 mask、ping/pong、close。不用 extension（`permessage-deflate`）、subprotocol、二進位的訊息。

| | 做法 | 優點 | 缺點 |
| --- | --- | --- | --- |
| W1 | hyper 處理 HTTP 和 upgrade（`hyper::upgrade::on`），frame 自己寫，SHA-1 用 `ring` 的 `SHA1_FOR_LEGACY_USE_ONLY` | 不加 crate；只做要的部分，每個分支都測得到 | 約 200 行要自己維護；frame 的邊界情況（分段、控制 frame 夾在中間、過大的訊息）要自己測 |
| W2 | hyper 加 `tokio-tungstenite` | Rust 最常用的 websocket；完整、經過大量使用 | 多 `tungstenite`、`sha1`、`rand` 等幾個 crate；功能大多用不到 |
| W3 | hyper 加 `fastwebsockets` | Deno 的 inspector 用它；直接接 hyper 的 upgrade；小 | 多一個 crate，使用的人比 W2 少 |

訊息的大小：heap snapshot 經過 `HeapProfiler.addHeapSnapshotChunk` 分段送，單則訊息不大；`Debugger.getScriptSource` 會送整份 script（Speedometer 的 bundle 有幾 MB）。不管哪個做法，都要設一個單則訊息的上限，數字在第 1 段定。

## 其他實作

- **Node.js**：`src/inspector_agent.cc`（`V8InspectorClient`）、`src/inspector_io.cc`（自己的 thread、HTTP 和 websocket，`src/inspector_socket.cc` 自己寫的 websocket）。`/json/list` 的 `type` 是 `node`，`devtoolsFrontendUrl` 是 `devtools://devtools/bundled/js_app.html?experiments=true&v8only=true&ws=…`。訊息從 IO thread 經過 `RequestInterrupt` 和 main thread 的 uv 的 async 送進去；停住時 main thread 在 `runMessageLoopOnPause` 裡只處理 inspector 的訊息。預設聽 `127.0.0.1:9229`，檢查 `Host`。
- **Deno**：`deno_core` 的 inspector 用同一個 rusty_v8 的 binding；websocket 是 `fastwebsockets` 加 hyper；`run_message_loop_on_pause` 在 main thread 輪詢 channel。
- **Servo**：`components/devtools` 講 Firefox 的 Remote Debugging Protocol（TCP 上的 JSON），前端用 Firefox 的 `about:debugging`。
- **Ladybird**：DevTools 的後端也講 Firefox 的協定，前端用 Firefox 的。
- **Firefox**：曾經做過 CDP 的子集給 Puppeteer，後來拿掉，改成 WebDriver BiDi。

## 命令的 `id`

2026-10-23，macOS，Chrome for Testing 152.0.7977.82（headless，經過 `--remote-debugging-pipe`，`scripts/bench/common.py` 的 `ChromePipe`）。同一個 `id` 各送一次給 browser（`Browser.getVersion`）和頁面的 flat session（`Runtime.evaluate`），兩邊的結果一樣：

| `id` | Chrome | kun（第 1 段） |
| --- | --- | --- |
| `2147483647` | 照常回答 | 照常回答 |
| `2147483648`、`4294967296`、`9007199254740993` | `{"error": {"code": -32600, "message": "Message must have integer 'id' property"}}`，沒有 `id` | 接受（`u64`） |
| `-1` | 照常回答，`id` 是 `-1` | 拒絕（`u64` 不收負數） |
| `1e3` | 照常回答，`id` 是 `1000` | 拒絕（serde 讀成浮點數） |
| `1.5` | 和 `2^31` 一樣的錯誤 | 拒絕，只記 log、不送任何東西 |

Chrome 在最外層解析信封時就把 `id` 限制在 `int`（`crdtp` 的 dispatcher），交給 renderer 和 V8 之前已經擋掉。

## DevTools 前端連上時

第 4 段實測後補上：`inspector.html` 一連上依序送了哪些命令、哪些回答 `METHOD_NOT_FOUND` 時它照樣能用、哪些會讓整個前端壞掉；`chrome://inspect` 對 `type` 不同的 target 開哪個前端。
