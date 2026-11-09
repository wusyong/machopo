# RFC 0075：Chrome 的 DevTools 連上 kun：CDP 的骨架和 V8 inspector

- 開始日期：2026-10-22
- 狀態：✅ 已完成：2026-10-23（macOS（Apple Silicon））
- 進度和驗收：`progress.md`
- 細節：`details.md`（kun 現在的自動化怎麼走、rusty_v8 的 inspector binding 和缺的部分、Chromium 的對照、websocket 的做法、其他實作）

## 摘要

kun 加上 Chrome 的 `--remote-debugging-port`：HTTP 的 `/json/version`、`/json/list` 和 websocket，Chrome 的 `chrome://inspect` 找得到 kun 的分頁，用 Chrome 的 DevTools 檢查它。這份做 CDP 的骨架：傳輸、同時好幾個 client 的 session、browser 和 renderer 之間的轉送（browser 先處理，處理不了的原樣交給分頁的 renderer，照 Chromium 的 `DevToolsSession`）；以及 CDP 屬於 V8 的那一半（`js_protocol`：`Runtime`、`Debugger`、`Profiler`、`HeapProfiler`、`Console`、`Schema`），由 V8 的 inspector 回答。DevTools 的 Console、Sources（檔案樹、斷點、逐步執行）、Memory（heap snapshot）和 JavaScript Profiler 能用：除了 V8 的那一半，renderer 自己回答 frame 樹和文件的原始碼（`Page` 的一小部分），DevTools 才選得對 context、列得出檔案。automation pipe 經過同一套轉送，`bench.py` 對 kun 和 Chrome 都用 `Runtime.evaluate`。頁面的其他 domain（`DOM`、`CSS`、`Overlay`、`Network`）、screencast、kun 內建的 DevTools 和 Playwright 不在這份。

## 動機

**現況**：kun 沒有除錯工具。script 的錯只有 stderr 的一行 `[uncaught]`；`console` 只印成字串，沒有來源和行號，物件不能展開；沒有斷點。查 Speedometer 和 Chrome 的差異時，只能在頁面裡加 `console.log`，或經過 automation pipe 的 `Kun.executeScript` 一次問一個值。`--automation-pipe` 雖然照 CDP 的格式，但只有 WebDriver、情境和 `bench.py --profile` 用到的方法，沒有 websocket，CDP 的 client 連不上。

**路線**（2026-10-22 和使用者討論，「已決定的事項」第 1 條）：短期的目標是 Chrome 的 DevTools 能遠端連上 kun，之後是 Playwright。分成幾份 RFC，每份做完都有自己的用處：

1. **這份**：CDP 的骨架和 V8 inspector。DevTools 的 Console、Sources、Memory、Profiler。
2. 頁面和網路：`Page`、`Target` 補齊（frame 的事件、`getResourceTree`）、`Log`、`Network` 的事件和 `getResponseBody`。DevTools 的 Network 面板，Playwright 要的 frame tree 和 lifecycle。
3. Elements：`DOM`（節點 id、變動事件）、`CSS`（computed style、matched rules，先唯讀）、`Overlay`（highlight）。
4. Playwright：isolated world、`Target.setAutoAttach` 的 `waitForDebuggerOnStart`、`Emulation`、browser context、`Fetch`。

kun 內建的 DevTools（F12、`devtools://`、停靠在視窗裡）暫時不考慮。

**目標**：

- `kun --remote-debugging-port=9222 <網址>` 照常開視窗、人照常操作；Chrome 的 `chrome://inspect` 列出 kun 的分頁（Chrome 預設就找 `localhost:9222`），按 inspect 開 DevTools。
- **Console**：看到 `console` 的訊息（引數是可以展開的物件、帶來源和行號、`%s` 等照 DevTools 套用）和沒接住的例外；在頁面或 iframe 的 context 裡執行。
- **Sources**：列出頁面的 script（外部的、inline 的、`eval` 的、module），下斷點、`debugger;`、例外時停住、逐步執行，停住時看 scope 和 call stack、在那個 frame 裡執行。
- **Memory**：heap snapshot（V8 的 heap 和 cppgc 的 DOM 物件）。**JavaScript Profiler**：V8 的 CPU profile。
- 同一個分頁可以同時被好幾個 client 接上：兩個 DevTools，或 DevTools 加上 automation pipe 的工具。
- automation pipe 上也能用 `js_protocol`；`scripts/bench/common.py` 不再分兩條路（現在 Chrome 用 `Runtime.evaluate`，kun 用 `Kun.executeScript`，註解寫著 "kun has no `Runtime.evaluate`"）。

**解決的 `docs/limitations.md` 的限制**（「WebDriver 和自動化」）：

- `Profiler.stop` 的 profile 的 `startTime`、`endTime` 只有 V8 時鐘的低 32 位元：改用 inspector 的 `Profiler`，由 inspector 自己組 profile，和 Chrome 一樣（「已決定的事項」第 29 條）；這一條刪掉了。
- `console` 照 Chrome 印到終端機那一條裡的「DevTools 才有這些；kun 沒有 DevTools」和「沒有 Chrome 的 source 和行號」：DevTools 裡是 V8 的完整格式；終端機印來源和行號（第 21 條）。
- 「自動化的訊息只有 WebDriver 用到的那幾個 CDP 方法」：改寫成現在的範圍（`js_protocol` 全部、`Page` 的 frame 樹，其他頁面的 domain 沒有）。

**不在範圍內**：

- `DOM`、`CSS`、`Overlay`、`Network`、`Log`、`Tracing`，以及 `Page`、`Target` 裡 DevTools 前端要的大部分：後面的 RFC。DevTools 的 Elements、Network、Performance 這幾個面板是空的（第 4 段只補了缺了會讓看得到的功能壞掉的 frame 樹，第 8 節）。
- kun 內建的 DevTools、F12；Playwright、Puppeteer、WebDriver BiDi。
- 從其他機器連進來：只聽 `127.0.0.1`。
- wasm 的文件（`--script wasm`，或 `<meta name="script" content="wasm">`）：沒有 V8 的 context，DevTools 看不到它的 script。
- `navigator.webdriver`：開了 `--remote-debugging-port` 照舊是 `false`（Chrome 也是；chromedriver、Puppeteer 另外加 `--enable-automation`）。

## 設計概要

**用 DevTools 的人**：

```
$ kun --remote-debugging-port=9222 https://example.com

DevTools listening on ws://127.0.0.1:9222/devtools/browser/<id>
```

在 Chrome 打開 `chrome://inspect`，「Remote Target」下面列著 kun 的分頁（標題和網址），按 inspect 開 DevTools。也可以直接在 Chrome 打開 `/json/list` 給的 `devtoolsFrontendUrl`。

- **停在斷點時**：頁面停住，script、style、layout、`requestAnimationFrame`、計時器都不跑；已經畫好的畫面照常捲動（compositor thread）；點擊和按鍵排著，繼續之後才處理。和 Chrome 一樣。DevTools 關掉或斷線時，頁面自動繼續。
- **console**：終端機照舊印（stderr），DevTools 裡看到的是 V8 inspector 的格式。
- **wasm 的文件**：DevTools 連得上，但 Console 沒有 context、Sources 沒有 script。

**用 automation pipe 的工具**（machopodriver、`bench.py`）：pipe 照舊；session 上多了 `js_protocol` 的方法。`bench.py` 對 kun 也送 `Runtime.evaluate`。

**改這部分程式的人**：CDP 的方法分兩種。browser 自己回答的（`Browser`、`Target`、`Page` 的一部分、`Input`、`Kun`、`SystemInfo`）在 `protocol::automation` 有型別，處理它們的是 UI thread 的 session；其他的原樣（JSON 字串）交給分頁的 renderer，renderer 先問自己的 agent（現在只有 `Page` 的 frame 樹，Blink 的 `InspectorPageAgent`；之後的 RFC 加 `DOM`、`CSS`……），其他的交給 V8 的 inspector。renderer 回答的也原樣交回 client。

## 詳細設計

改到的地方：`crates/shell`（新的 HTTP 和 websocket 的 thread、`automation_session.rs` 變成好幾個 client 的 session、`window.rs` 和 `windowless.rs` 收新的連線的訊息、`main.rs` 的開關和 `kun --help`）、`crates/protocol`（`automation.rs` 的轉送、`renderer.rs` 的新訊息）、`crates/browser`（`web_contents.rs` 送 DevTools 的訊息）、`crates/renderer`（`render_thread/devtools.rs` 的 DevTools 的 session、`render_thread/page_agent.rs` 的 `Page` agent、`task_queue.rs` 的 DevTools 佇列、`frame_view.rs` 的 frame 的 id 和文件的原始碼）、`crates/dom_api`（`engine.rs` 的 engine 介面；`console` 的宣告和實作刪掉）、`crates/dom_js` 和 `crates/bindings`（V8 inspector、`console`、例外、停住的巢狀迴圈；kun 自己的 `CpuProfiler` 刪掉）、`../third_party/rusty_v8`（inspector 的 binding 缺的部分）、`scripts/bench/common.py`、`scripts/cdp_compare.py`（`test.py cdp`）。

### 1. 傳輸：HTTP 和 websocket

- **開關**：`--remote-debugging-port=N`，視窗和 `--automation-pipe` 都收（`--headless` 不收：它只畫一張圖就結束）。`0` 是讓系統挑一個空的 port。開始聽之後在 stderr 印 `DevTools listening on ws://127.0.0.1:N/devtools/browser/<id>`，和 Chrome 一字不差（Puppeteer、Playwright 啟動 Chrome 時讀這一行）。
- **thread**：新的 `DevToolsHandlerThread`（`crates/shell/src/devtools_http_handler.rs`；Chromium 的 `DevToolsHttpHandler` 有自己的 thread）。它接受連線，HTTP 用 hyper（和 machopodriver 一樣，單 thread 的 tokio），websocket 用 `tokio-tungstenite`（接在 hyper 的 upgrade 後面；「已決定的事項」第 3 條），HTTP 的請求、連線的建立和斷開、每則訊息都變成 `automation_session::ClientEvent` 交給 UI thread：視窗經過 winit 的 `ShellEvent::DevTools`，沒有平台視窗時經過 `windowless::drive` 的 `select!` 多一個來源；送出的照每條連線排隊（tokio 的 channel）。
- **停下**：瀏覽器結束時先丟掉 session（每個 client 的佇列跟著關掉），server 再等 websocket 送完手上的訊息（最多 5 秒；例如 `Browser.close` 的回答），才停下。實作時加的：原本先停 server，回答可能還沒送出連線就被取消。
- **HTTP**（照 Chrome）：`GET /json/version`（`Browser`、`Protocol-Version`、`User-Agent`、`webSocketDebuggerUrl`；沒有 Chrome 的 `V8-Version`：`shell` 不直接碰 V8，`docs/limitations.md`）；`GET /json/list`（和 `/json`）：每個分頁一筆 `id`、`type: "page"`、`title`、`url`、`webSocketDebuggerUrl`、`devtoolsFrontendUrl`；`PUT /json/new?<網址>`（網址解開 `%` 的跳脫，沒有或不是網址時開 `about:blank`，照 Chrome）、`GET /json/activate/<id>`、`GET /json/close/<id>`（沒有那個分頁是 404）。`/json/protocol` 不做（DevTools 不用它）。
- **websocket**：`/devtools/browser/<id>` 和 pipe 一樣（flat session，要自己 `Target.attachToTarget`）；`<id>` 是 UUID 的形式，和 Chrome 一樣。`/devtools/page/<targetId>` 一連上就接到那個分頁（session 在連上時建立），沒有 `sessionId` 的命令屬於那個分頁，送給它的回答和事件也不帶 `sessionId`（DevTools 的前端這樣用，Chromium 的每個 target 的 client）；分頁關掉時送 `Inspector.detached`（`reason: "target_closed"`）再關掉連線，和 Chrome 一樣。
- **安全**（「已決定的事項」第 4 條）：只聽 `127.0.0.1`；HTTP 和 websocket 的 `Host` 必須是 IP 或 `localhost`，不是的回答 500（Chrome 的 `DevToolsHttpHandler` 的做法）；websocket 的握手帶著 `Origin` 時，不是 `devtools://devtools`、也不符合 `--remote-allow-origins`（逗號分開的 origin，或 `*`）就回答 403；`/json/new` 只收 `PUT`，其他是 405。
- **id 猜不到**：browser 的 websocket 網址的 `<id>` 和每個分頁的 target id 都是隨機的 128 位元（`getrandom`；target id 是 32 個十六進位字）。target id 是分頁建立時 `WebContents` 抽的，每次啟動 renderer 都交給它（「已決定的事項」第 17 條；第 1 段是 session 第一次看到分頁時抽）；原本的 `target_id` 是 `TabId` 的十六進位，第一個分頁是 `000…001`。分頁關掉之後 id 還留著：晚到的 renderer 訊息可能還提到它。pipe 上也一樣，不分兩種 id；main frame 的 `frameId` 照舊和 target id 相同。session 處理每個命令前先看一遍分頁（人在視窗裡開的分頁也有了 id），所以命令和訊息提到的分頁都有 id。
- **訊息的大小**（client 送進來的是信任邊界，RFC 0074 的原則，「已決定的事項」第 7 條）：`tokio-tungstenite` 設單則訊息和單個 frame 的上限（`max_message_size`、`max_frame_size`），超過時關掉那條連線；pipe 的一則訊息（讀到 NUL 之前）用同一個上限。上限要裝得下合法的大命令（`Runtime.evaluate`、`Debugger.setScriptSource` 帶著整份 script）：單則 64 MiB、單個 frame 16 MiB（「已決定的事項」第 8 條）。

### 2. session：好幾個 client

原本的 `AutomationSession`（`crates/shell/src/automation_session.rs`）只有一個 client（pipe），session 的 id、每個 session 的狀態（lifecycle 事件、取樣間隔）和送出去的佇列都在它身上。改成：

- **client**：pipe 是一個，每條 websocket 連線是一個。client 有自己的送出佇列（`Outbox`：pipe 的 writer、websocket 的連線、情境的執行器的 channel）、自己的 session、自己打開的事件（`Target.setDiscoverTargets`、`Page.enable`……）。斷線時它的 session 全部 detach。
- **driver**：自動化模式（`--automation-pipe`、`--automation`）的 session 有一個 driver，pipe 或情境的執行器，命令照舊經過 `AutomationSession::command`；websocket 的 client 經過 `client_event`（`ClientEvent::Connect`、`Command`、`Disconnect`、`Json`）。一般的視窗開了 port 時，session 沒有 driver（`AutomationSession::for_remote_debugging`）。實作時這樣分：現有的呼叫的地方和測試都只有 driver，不用改。client 和 session 的 id 用 `base::SequenceNumber`（RFC 0074，用完時 panic，不悄悄 wrap）；session 只屬於開它的 client，別的 client 拿同一個 id 是 `SESSION_NOT_FOUND`，所以 session 的 id 不必猜不到。
- **session** 屬於一個 client 和一個分頁（Chromium 的 `DevToolsSession` 屬於一個 `DevToolsAgentHost`）。同一個分頁可以有好幾個 session，每個 session 在 renderer 那一側也有一個（V8 的 inspector 一個 context group 可以接好幾個 session）。
- **`Kun` domain 只在自動化模式（`--automation-pipe`）下有**，那時 pipe 和 websocket 的 client 都能用；一般的視窗（人在操作，DevTools 經過 port 連上）整個 domain 是 `METHOD_NOT_FOUND`。它的方法是情境和 machopodriver 用的：視窗的那一半（`Kun.dispatchWindowInput`、`Kun.navigateFromAddressBar`……）假設沒有人在操作視窗，那是自動化模式的性質（它不理會平台的滑鼠和鍵盤），不是 pipe 這條連線的性質。對應 Chrome 的 `HeadlessExperimental`：domain 有沒有跟著瀏覽器的模式走（Chrome 不依連線的方式分，pipe 和 port 能用的一樣多）。
- **`Browser.close`**：任何 client 都能關掉 kun（Chrome 也是）。

### 3. 轉送：browser 先處理，其他的交給 renderer

照 Chromium：`DevToolsSession` 先交給 browser 的 handler，handler 說 `FallThrough` 的才送到 renderer 的 `DevToolsAgent`。

- **browser 這一側**：`protocol::automation` 的 `Method` 對 browser 有的方法解析成型別（參數照舊嚴格，「已決定的事項」第 6 條）；沒有的是 `Method::Other`（原本是 `METHOD_NOT_FOUND`），帶 `sessionId`（或在 `/devtools/page/` 的連線上）時整則命令的 JSON 原樣轉給 renderer，沒有 session 的（browser 的命令）是 `METHOD_NOT_FOUND`。`Page.enable`、`disable` browser 記下來之後也轉（fall through，第 8 節）。browser 記著轉出去還沒回答的命令，renderer 不在時回答錯誤（第 6 節）。
- **新的訊息**（`protocol::renderer`）：`RendererMsg::DevToolsAttach { session }`、`DevToolsDetach { session }`、`DevToolsMessage { session, message: String, interrupt: bool }`（`interrupt`：要打斷跑著的 script 的命令，第 5 節）、`DevToolsMainFrameToken`（main frame 的 id）；`RendererHostMsg::DevToolsMessage { session, message: String }`（回答和事件都是它）。`session` 是 browser 發的 id，renderer 不認識 client。都是字串和數字，能跨 process。
- **renderer 回來的訊息不被信任**：browser 只確認它是一個 JSON object、屬於這個 renderer 的 session，再加上 `sessionId` 交給 client；不看內容（Chromium 的 browser 也不解讀 renderer 的回答）。renderer 回答了不是它的 session 的訊息時丟掉。單則訊息有大小的上限（拆成 process 之後，被攻破的 renderer 能送巨大的字串；和 RFC 0074 要 viz 驗證 frame 是同一個理由），要裝得下合法的最大回答（`Debugger.getScriptSource` 的幾 MB 的 script、CPU profile、heap snapshot 的一段）：256 MiB（第 23 條，是 client 的訊息上限 64 MiB 的 4 倍：回答可能比命令大得多）；超過時丟掉、記 log，拆成 process 之後照 Chromium 的 `ReportBadMessage` 處理。
- **方法到底有沒有**：renderer 先問自己的 `Page` agent（第 8 節），其他的整則交給 V8 的 session，V8 自己對不是它的方法（例如 `DOM.enable`）回答 `METHOD_NOT_FOUND`（`'DOM.enable' wasn't found`，和 Chrome 對沒有的方法回答的一樣）；engine 沒有 inspector 時（wasm 的文件）renderer 照同樣的格式回答。
- **命令的 `id`**（「已決定的事項」第 19 條）：V8 inspector 的 call id 是 C++ 的 `int`（rusty_v8 的 `Channel::send_response` 收 `i32`），browser 照 Chrome 在解析信封時就只收 `i32` 範圍內的整數，整則 JSON 原樣交給 V8 時不用換。
- **順序**：同一個 session 的命令照收到的順序交出去；browser 當場回答的可能比前面交給 renderer 的先回來（CDP 照 `id` 對應，不保證順序；Chrome 也是）。

### 4. renderer：V8 inspector

- **一個 renderer 一個 `V8Inspector`**（Blink 的 `MainThreadDebugger`：main thread 的 isolate 一個），和 isolate 一起建（「已決定的事項」第 13 條；`bindings` 的 `V8PerIsolateData`），一起消失。分頁是一個 context group（kun 一個分頁一個 renderer）。
- **context**：每個 realm 建好時 `context_created`，`auxData` 照 Chrome：`{"isDefault": true, "type": "default", "frameId": <frame 的 id>}`，`origin` 是文件的 origin（Blink 的 `SecurityOrigin::ToRawString`，Chrome 給的：opaque 的 scheme 和 host 是空的，`://`）。建立都經過 `dom_js` 的 `create_realm`、`create_child_realm`（`dom_js/src/devtools.rs` 的 `realm_created`）。消失：renderer 在可能丟棄文件的地方（導覽的 commit、子 frame 的 commit、script 執行中移除 iframe）之後呼叫 `ScriptEngine::documents_discarded`，engine 對不再 fully active 的文件的 realm `context_destroyed`；main frame 的文件丟棄、它的下一份還沒建 realm 時改成 `resetContextGroup`（`Runtime.executionContextsCleared`，Blink 的 `DidClearContextsForFrame`；舊頁面的 console 訊息一起清掉，晚連上的 DevTools 不會看到上一頁的）。沒有指定 context 的命令在 main frame 的 realm 裡跑（`V8InspectorClient::ensureDefaultContextInGroup`，Blink 的 `MainThreadDebugger` 也是；`FrameClient::is_main_frame`）。
- **frame 的 id**（「已決定的事項」第 17 條）：main frame 的 id 是分頁的 target id，分頁建立時 browser 抽好（`WebContents`），啟動 renderer 時連同參數交給它；子 frame 的是 renderer 給 `Kun.switchToFrame` 的同一個 id（`<prefix>.<token>`），在 `create_child_frame` 時就編號（子 frame 的 realm 比它的第一次 commit 先建，`context_created` 那時就要 `frameId`）。之後的 `Page.getFrameTree` 用同一套。
- **engine 的介面**：`dom_api` 的 `ScriptEngine` 加上 DevTools 的 session（attach、交訊息、detach）和 context 的通知，預設什麼都不做；V8 的實作在 `dom_js` 和 `bindings`。renderer 和 `dom_api` 照舊和 engine 無關（`python3 scripts/sanity.py deps`）。wasm 的文件沒有 context，renderer 的 session 照樣存在。
- **console**（「已決定的事項」第 12 條）：JS 改用 V8 自己的 `console`。V8 的 inspector 只看得到經過 V8 內建的 `console`（builtin 把呼叫交給 inspector 的 `V8Console`）的呼叫，kun 原本的 `console` 是宣告表的 namespace（`dom_api` 的 `console` 模組，第 2 段刪掉了），蓋掉了 V8 的、inspector 看不到。改了之後 inspector 建立時就是 V8 內建 `console` 的 delegate（不用另外接 `ConsoleDelegate`），終端機的輸出從 `V8InspectorClient::consoleAPIMessage` 經過 `ScriptEngine::set_console_sink` 給的 sink 印（Chrome 的 `--enable-logging=stderr` 就是這條路；sink 是 isolate 的，不是 frame 的：`consoleAPIMessage` 只給 context group，不給 frame）；宣告表的 `console` namespace 和 `dom_api` 替它格式化值的部分（`console_string`）拿掉。
- **例外**：沒接住的例外現在在 `crates/dom_js/src/exception.rs` 印到 stderr，同一個地方也交給 `V8Inspector::exception_thrown`（Blink 的 `MainThreadDebugger::ExceptionThrown`）；promise 的 rejection 之後被處理時 `exceptionRevoked`。V8 要 script 的 id（`Message` 的 `GetScriptOrigin().ScriptId()`）和 script 是 shared cross-origin（`ScriptOrigin` 的 `is_shared_cross_origin`），才把例外本身放進 `exceptionDetails`（不然只有文字）：kun 不 mute 任何 script 的錯誤，三個建立 `ScriptOrigin` 的地方都給 `true`（Blink 不 mute 的 script 也是）。位置照 Blink 的 `CaptureSourceLocation`：stack 的頂端在丟出它的 script 裡時用頂端（`new Error` 的地方），不然用丟出的地方。錯誤的完整 stack 照 Chrome 只在有 session 打開 `Runtime` 時記（inspector 自己開關 `SetCaptureStackTraceForUncaughtExceptions`；kun 原本一直開著，每建一個錯誤都要付）。
- **script 的網址**：Sources 列的名字和斷點的 `setBreakpointByUrl` 靠 V8 的 `ScriptOrigin`。外部的 script 是它的網址；inline 的是文件的網址，加上它在文件裡的行和列（DevTools 用它對到 HTML 裡的位置）。第 2 段確認了三個建立 `ScriptOrigin` 的地方（`crates/bindings/src/script_state.rs`、`module_record.rs`、`crates/dom_js/src/host.rs`）：網址和行都給了，列沒有（parser 只記 script 開始的行），inline script 在 `<script>` 那一行上的位置差 `<script>` 前面的字數（`docs/limitations.md`）。

### 5. 停在斷點

V8 停住時呼叫 `run_message_loop_on_pause`，在 script 的 stack 裡面等，直到 inspector 說繼續（`quit_message_loop_on_pause`）。

- **renderer 的巢狀迴圈**：只處理 DevTools 的訊息，其他的 task（瀏覽器的訊息、compositor 的、計時器）先排著，繼續之後照原本的順序處理（Blink 的 `ClientMessageLoopAdapter` 加上 `ScopedPagePauser`）。DevTools 的訊息另外一條佇列（「已決定的事項」第 30 條）：`RendererSender::send` 把 attach、detach、命令放進 `task_queue::DevToolsQueue`，主佇列放一個叫醒 main loop 的 task；巢狀迴圈在 `bindings` 裡（V8 呼叫 `run_message_loop_on_pause`），經過 `dom_api` 的 `DevToolsQueue` trait 從它拿（renderer 和 engine 無關，`bindings` 看不到 renderer 的型別）。停住時才 attach 的 session 和它的命令放回佇列的最前面，等繼續之後由 main loop 處理（迴圈裡建 session 要 renderer 給的 `send`，那在外層）。
- **script 一直在跑時的 `Debugger.pause`**：main thread 在跑 script，不會回到事件迴圈。Blink 的 `InspectorTaskRunner` 把 DevTools 的訊息放進自己的佇列、再用 `Isolate::RequestInterrupt` 讓 V8 在下一個安全的點停下來處理；Node 也是。kun 照做：browser 在要打斷的命令上標著（第 31 條），`RendererSender::send` 放進 DevTools 的佇列之後呼叫 engine 給的打斷（經過 thread 安全的 `IsolateHandle` 請 V8 中斷），V8 的 callback 只拿佇列裡標著的命令（在第一個 attach、detach 之前的：session 都已知），其他的留著。
- **browser 這一側**：renderer 停住時，等它回答的東西都在等（machopodriver 的命令、`Kun.waitForInputHandled`、情境），和 Chrome 一樣；UI thread 不同步等 renderer，視窗照常。
- **停住時斷線、關掉分頁**（第 32 條）：client 斷線 → session detach，巢狀迴圈在那個 session 上送 `Debugger.disable`（V8 沒有 session 開著 `Debugger` 時繼續），session 先留著，迴圈回來之後再 drop（照舊用 `connect`，第 18 條；不綁 `connectShared`）。關掉分頁時 `RendererSender::send` 收到 `Shutdown`，也在 DevTools 的佇列放一個 `Quit`：巢狀迴圈在每個 session 上送 `Debugger.resume`（`terminateOnResume`），讓頁面繼續並終止 script，回到 main loop 處理結束。這兩個命令是 CDP 本身的，不用綁 V8 的 `V8InspectorSession::resume`、`stop`（rusty_v8 沒有）；它們的回答丟掉：session 一 detach 就標成不再送出。

### 6. profiler 和 bench

- **`Profiler`**：原本 kun 自己接 V8 的 `CpuProfiler`（`bindings` 的 `V8PerIsolateData`，經過 fork 的 `cpu_profiler` 模組），browser 有 `Profiler.*` 的型別，轉成 `RendererMsg::Profiler`。第 3 段換成 inspector 的（第 29 條）：`Profiler.*` 照其他方法轉給 renderer，閒置照 Blink 告訴 inspector（`idle_started`、`idle_finished`）。
- **`HeapProfiler`**：交給 inspector。cppgc 的 DOM 物件經過 `CppHeap` 出現在 heap snapshot 裡，名字是它們的 Rust 型別（`Node`；`dom_macros` 產生的 `get_name`），第 3 段確認了。
- **還沒回答就不在的 renderer**：browser 記著交給 renderer、還沒回答的命令（Chromium 的 `DevToolsSession` 也記著），renderer 不在時回答錯誤；原本 `Profiler` 的請求由 browser 自己記著，換成轉送之後也要。
- **bench**：`scripts/bench/common.py` 的 kun 那一側改用 `Runtime.evaluate`（`returnByValue`），拿掉 `Kun.executeScript` 那條路（第 2 段）。

### 7. rusty_v8 的 fork

rusty_v8 的 inspector binding 是上游的（Deno 用的）；V8 的 build 本來就有 inspector。補的部分（細節在 `details.md` 的「rusty_v8 的 inspector binding」；「已決定的事項」第 18、24 條），連同拿掉的，發在 `kun-v152.2.0-18`（第 28 條：第 3 段做完時一次發）：

- `V8InspectorClient::currentTimeMS`：上游沒有轉給 Rust，回答 0，`console` 的時間、`Runtime.consoleAPICalled` 的 `timestamp` 都是 0。
- `V8Inspector::exceptionRevoked`（第 4 節）。
- `Message::get_script_id`、`V8Inspector::context_created_with_origin`、`V8Inspector::reset_context_group`（第 4 節）。
- 拿掉 kun 自己加的 `cpu_profiler` 模組（第 29 條）。停住時不需要新的 binding（第 5 節）。
- `connectShared` 不綁：kun 自己在巢狀迴圈回來之後才刪 session（第 5 節，第 18 條）。`connect` 的 `SessionPauseState`（`waitForDebuggerOnStart`）、`session.state()`：Playwright 才要，之後再補。

補的部分照 fork 的規則：程式裡標 `kun:`，記進 `README.machopo.md`，改了 `src/binding.cc` 要重編 V8（`python3 scripts/ci.py v8`）。

### 8. DevTools 前端的相容（第 4 段）

第 4 段錄下 Chrome for Testing 152 的 DevTools（`inspector.html`）一連上、點過每個面板送的命令（87 個、62 種）：15 種成功，47 種 -32601，沒有卡住、沒有其他錯誤。只有 frame 樹的缺少讓看得到的功能壞掉：Console 一開始停在 iframe 的 context、context 選單只寫 origin、Sources 的檔案樹和 `Cmd + P` 是空的（第 2、3 段的手動確認找到的）。

- **renderer 的 `Page` agent**（第 33 條；`crates/renderer/src/render_thread/page_agent.rs`）：命令交給 engine 之前先問它，`Page.getResourceTree`、`getFrameTree`、`getResourceContent`、`enable`、`disable` 由它回答。它記著上次告訴 DevTools 的 frame（每個 task 結束時和現在的比，有 session 時才比），所以停在斷點時也回答得出來：巢狀迴圈經過 `DevToolsQueue::answer_here` 先問它。
- **frame 的事件**：`frameAttached`、`frameNavigated`、`frameDetached`。新的 iframe 的初始 `about:blank` 照 Chrome 不送 `frameNavigated`；main frame 的新文件在建它的 realm 之前送（Chrome 的順序：`frameNavigated` 再 `executionContextCreated`），它舊的子 frame 先 `frameDetached`。
- **`Page.Frame` 的欄位**：照 Chrome，包括實驗性的 `adFrameStatus`（一律不是廣告）、`securityOriginDetails`；`loaderId` 是 main frame 的 navigation id（和 browser 的 `Page.lifecycleEvent`、`frameStartedNavigating` 一致），子 frame 的是文件的號碼；`secureContextType` 照 Secure Contexts（`about:`、`data:` 跟著父 frame）；`domainAndRegistry` 空的（沒有 public suffix list）。
- **文件的原始碼**（第 34 條）：FrameView 留著解碼後的 markup，`getResourceContent` 要先 `Page.enable`（Chrome 也是）。
- **`Page.enable`、`disable`**：browser 記下來（它自己的事件），再轉給 renderer，由 renderer 回答（Chromium 的 fall through）。
- **和 Chrome 比**（`test.py cdp` 的 `page`）：main frame 的欄位、事件一樣；Chrome 的 `frameAttached` 有建立它的 script 的 stack，kun 沒有；Chrome 沒有快取的文件給不出原始碼，kun 給得出（第 34 條）。Chrome 的 headless 在載入之後列出的子 frame 沒有網址（看起來在別的 process），不比。

### 測試

驗收表和做過的檢查在 `progress.md`。測試的種類：

- **傳輸和 session**（`crates/shell/tests/it/remote_debugging.rs`、`windowless.rs`、`automation_pipe.rs`；`devtools_http_handler` 的單元測試）：HTTP 的 endpoint、兩種 websocket、擋住網頁的檢查、client 之間、訊息的大小上限、`id`、renderer 的訊息的檢查。
- **inspector 和 `Page` agent**（`crates/shell/tests/it/devtools.rs`）：context 和 frame 的 id、console、例外和 rejection、renderer 重開和換 engine、停住、斷點和單步、打斷、停住時斷線和關分頁、`Profiler`、heap snapshot、frame 樹、文件的原始碼、frame 的事件；`bindings` 的 `cpu_profiles`、`dom_js` 的 `console`。
- **和 Chrome 比**（第 20 條）：`python3 scripts/test.py cdp`，本機跑，不在 CI。
- **手動**：Chrome 的 `chrome://inspect` 和 `devtools://` 連上 kun，走過 Console、Sources、Memory：情境做不到（DevTools 在另一個瀏覽器裡），記在 `docs/test-gaps.md` 的「只做過手動確認」。
- **mutation test**：每段一份（`mutants-1.toml` 到 `mutants-4.toml`）。

### 分階段

1. **傳輸和 session**：`--remote-debugging-port`、`DevToolsHandlerThread`、HTTP 的 endpoint、websocket、好幾個 client；現有的 browser 的方法經過 websocket 也能用；安全檢查和隨機的 id。這段不動 renderer。驗收：上面「傳輸」的測試；手動確認 `chrome://inspect` 列得出 kun 的分頁。
2. **V8 inspector 和 `Runtime`**：轉送、renderer 的 session、context 和 frame 的 id、console、例外、`currentTimeMS` 的 fork 修改；`bench.py` 改用 `Runtime.evaluate`。驗收：`Runtime` 和 console 的測試；手動確認 DevTools 的 Console。
3. **`Debugger`、`HeapProfiler`、`Profiler`**：巢狀迴圈、`RequestInterrupt`、停住時斷線和關掉分頁（巢狀迴圈回來之後才刪 session）；heap snapshot；`Profiler` 照第 9 條的決定。驗收：`Debugger` 的測試；手動確認 Sources 的斷點、Memory 的 heap snapshot。
4. **DevTools 前端的相容**：用 Chrome 的 DevTools（`inspector.html`）實際連上，錄下它一連上送的命令，補上「缺了會整個壞掉」的最小回答，第一項是 frame 樹（`Page.getResourceTree`、`Page.getFrameTree`：DevTools 靠它選 main frame 的 context，「已決定的事項」第 22 條；Sources 的檔案樹和 `Cmd + P` 也靠它，第 3 段手動確認時兩者都是空的），也看 screencast（`Page.startScreencast`，遠端分頁的 DevTools 預設打開，現在是黑的）；決定 `/json/list` 的 `devtoolsFrontendUrl` 和 `type`（第 11 條）。驗收：手動在 DevTools 裡走一遍目標的每一項。

每段開始前只決定那一段的事（「待決定」標著第幾段）。第 1 段可以先 merge 回 `main`：pipe 照舊，多了 websocket。

**和其他 RFC 的關係**：RFC 0074 已經做完（2026-10-22），每個 crate 都受 overflow 的 lint 管，這份的新程式一開始就照 `crates/base/README.md` 的規則寫，id 用 `base::SequenceNumber`。2026-10-23 開始時，RFC 0078（scroll anchoring）在另一台機器上平行做：第 1 段只和它各在 `crates/shell/tests/it/main.rs` 加一行、各改 `docs/limitations.md` 和 `docs/architecture.md` 的不同段落；第 2、3 段和它都改 `crates/renderer/src/frame_view.rs`（它在 layout 的前後，這份在 realm 的建立和消失），後 merge 的負責 rebase。

## 缺點

- **開著的 port 等於整個瀏覽器的控制權**：本機的任何程式（包括其他網頁，如果沒擋好）都能經過它執行 script、讀頁面的內容。只聽 `127.0.0.1`、要明確加開關、檢查 `Host` 和 `Origin`、id 猜不到（「已決定的事項」第 4 條）。
- **rusty_v8 的 fork 多幾個修改，要重編 V8**，共用的 artifact 跟著換。
- **多了 websocket 的 crate**（`tokio-tungstenite`、`tungstenite` 和它們的依賴）：CDP 只用到其中很小的一部分；換來的是之後網頁的 `WebSocket` 和這裡共用一套。
- **`console` 換成 V8 的**（「已決定的事項」第 12 條）：等於回到 RFC 0028 當初的 B1，推翻它第 2 條選的宣告表的 namespace（B3）；RFC 0028 比較時已經寫了「之後接 DevTools：要改成 B1」。JS 和之後 wasm 的 `console` 不再是同一份實作（「之後的可能」）。
- **停住的 renderer 讓等它的東西都等著**：machopodriver 的命令沒有期限的部分會一直等（`docs/limitations.md` 已經寫了 machopodriver 對 kun 的呼叫大多沒有期限）。和 Chrome 一樣，但多了一種「kun 看起來卡住」的原因。
- **renderer 的事件迴圈變複雜**：多了 DevTools 的佇列、巢狀迴圈和中斷。這是現在的 renderer 第一個巢狀迴圈（`alert` 這類也會需要，之後可以共用）。
- **inspector 一直存在**（「已決定的事項」第 13 條）：Chrome 也是（`MainThreadDebugger`）。沒有 session 開著 `Debugger` 時 V8 不多做事；`console` 多走一段（沒有 DevTools 時比原本慢 30% 到 40%，和 Chrome 差不多），保留的最近 1000 則訊息會留著它們的引數（大的陣列可以留到幾百 MB，Chrome 也一樣），量測在 `progress.md`。

## 理由和其他做法

- **V8 的 inspector，不自己做 `Runtime`、`Debugger`**：它就是 Chrome 用的那一個，回答的格式、RemoteObject、斷點的語意和 Chrome 一字不差；`Debugger` 自己做不出來（要 V8 的 debug API，inspector 就是它的上層）。代價只有 fork 補幾個 binding。
- **遠端的 DevTools，不內建**：使用者的決定（「已決定的事項」第 1 條）。好處是後端對不對，不受「kun 畫不畫得好 DevTools 前端」影響；前端用的是 Chrome 自己的，永遠是最新的。
- **websocket 現在就做**：DevTools 連上 kun 只有這條路（`chrome://inspect` 只認 HTTP 和 websocket）；pipe 照舊給 machopodriver 和 `bench.py`。
- **browser 先處理、其他原樣交給 renderer**（Chromium 的 `FallThrough`），不在 browser 把每個方法都寫成型別：`js_protocol` 有上百個方法和事件，V8 已經會解析；browser 只看信封（`id`、`method`、`sessionId`），renderer 的回答也不解讀。這也是之後拆成 process 時的樣子。
- **不經過 machopodriver**：machopodriver 是 WebDriver 的 server，CDP 的 client 直接連 kun，和 Chrome 一樣。

各項的比較在 `details.md`。

## 先例

- **Chromium**：`DevToolsHttpHandler`（自己的 thread、`/json/*`、`Host` 和 `Origin` 的檢查）、`DevToolsAgentHost` 和 `DevToolsSession`（browser 的 handler、`FallThrough`）、Blink 的 `DevToolsAgent`、`DevToolsSession`、`InspectorSession`（renderer 的 agent 和 `V8InspectorSession`）、`MainThreadDebugger`（`V8InspectorClient`，停住時的 `ClientMessageLoopAdapter` 和 `ScopedPagePauser`）、`InspectorTaskRunner`（`RequestInterrupt`）、`V8Console`（`console` 經過 inspector）。這份照它的分層；不照的地方：沒有 mojo，訊息是 JSON 字串（Chromium 在 browser 和 renderer 之間用 CBOR）；session 的狀態不跨 renderer 的換手保存：renderer 當掉重開、換 script engine 之後 session 照樣接上，但打開過的 domain 要 client 再打開（Chrome 把 session 的狀態交給新的 renderer）。
- **Node.js（`--inspect`）**：只有 V8 inspector 的後端加上 websocket 和 `/json/list`（自己寫的 websocket），`type` 是 `node`，前端是 Chrome 的 JS-only DevTools（`js_app.html`）；DevTools 的訊息經過另一個 thread 和 `RequestInterrupt` 送進 main thread。這份做完時的 kun 在能力上和它差不多。
- **Deno**：用的是同一個 rusty_v8 的 inspector binding，websocket 用 `fastwebsockets` 加 hyper。
- **Servo、Ladybird**：各自的 DevTools 後端都講 Firefox 的 Remote Debugging Protocol，前端用 Firefox 的。kun 照 Chromium 的分層、目標是 Playwright（Chromium 的那一路走 CDP），所以用 CDP。

## 已決定的事項

1. **路線：短期是 Chrome 的 DevTools 遠端連上 kun，之後是 Playwright；kun 內建的 DevTools 暫時不考慮**（2026-10-22，使用者）。不以「完全支援 CDP」為目標：CDP 沒有規範，約 50 個 domain，很多綁著 Chromium 的內部，沒有終點也沒有測試能說做完了；改成每份 RFC 對著一個看得到的用處（DevTools 的面板、Playwright）。
2. **這份的範圍：傳輸、session 的轉送、`js_protocol`（由 V8 的 inspector 回答），加上 bench 改用 `Runtime.evaluate`**（2026-10-22，使用者）。頁面的 domain 分到後面兩份：先 `Page`、`Target`、`Log`、`Network`（frame 的模型是 `DOM` 的前提，對 Playwright 的用處也比較大），再 `DOM`、`CSS`、`Overlay`。

第 1 段，逐條確認：

3. **websocket 用 `tokio-tungstenite`，HTTP 用 hyper（W2）**（2026-10-22，使用者）：DevTools 的 server 和之後網頁的 `WebSocket` API（network service 那一側的 client）用同一套 websocket 的實作，只維護一套。不選 W1（hyper 加自己寫的 frame，不加 crate；Chromium 和 Node 的 DevTools server 都是這樣的精簡版，Chromium 網頁的 `WebSocket` 是 `net/websockets/` 另一份完整的實作）：兩邊分開時要維護兩套。不選 W3（`fastwebsockets`）：用的人比 `tungstenite` 少，統一的理由也一樣適用於 W2。這一條只定 DevTools 這一側；網頁的 `WebSocket` 寫 RFC 時要確認 `tungstenite` 做得到它要的部分（`permessage-deflate`、背壓、接上 `net` 的 TLS 和 proxy），做不到時那份 RFC 再決定。
4. **安全：照 Chrome 的四項，加上猜不到的 id**（2026-10-22，使用者）：
   - 只聽 `127.0.0.1`，不收 `--remote-debugging-address`：擋其他機器。
   - HTTP 和 websocket 的 `Host` 必須是 IP 或 `localhost`：擋 DNS rebinding（`evil.com` 把自己解析到 127.0.0.1，它的網頁讀 `/json/list` 在瀏覽器看來是同源的）。Chrome 的 `DevToolsHttpHandler` 的做法。
   - websocket 的握手帶著 `Origin` 時，不符合 `--remote-allow-origins` 就拒絕；沒有 `Origin` 的（Python、Node 的工具）照收：websocket 不受同源政策限制，任何網頁都能連 `ws://127.0.0.1:<port>/…`。Chrome 111 起的做法。
   - `/json/new` 只收 `PUT`：擋網頁用 `<img>` 這類 GET 開分頁。Chrome 111 起的做法。
   - browser 的 websocket 網址和分頁的 target id 是隨機的 128 位元：kun 現在的 target id 是 `TabId` 的十六進位，猜得到；`/json/close/<id>`、`/json/activate/<id>` 照 Chrome 是 GET，網頁放一個 `<img src="http://127.0.0.1:9222/json/close/000…001">` 就能關掉分頁，`Host` 和 `Origin` 的檢查都擋不住。Chrome 的 id 本來就是隨機的 token。pipe 上一起改：沒有測試寫死現在的 id，machopodriver 和 `bench.py` 都把它當成不透明的字串。
   - 不需要 Chrome 136 起「預設的 profile 不能開 port」那一條：它擋的是偷磁碟上的 cookie，kun 沒有存在磁碟上的 profile。
5. **好幾個 client 同時連（M1）**（2026-10-22，使用者）：照「詳細設計」第 2 節，每個 client 有自己的送出佇列、session 和打開的事件，斷線時它的 session 全部 detach；同一個分頁可以有好幾個 session，renderer 那一側各有一個 `V8InspectorSession`；`Kun` domain 只在自動化模式下有，那時每個 client 都能用。和 Chromium 一樣（Chrome 63 起一個 target 可以有好幾個 `DevToolsAgentHostClient`）。原本寫「`Kun` domain 只在 pipe 上」，同一天改成依模式：Chrome 不依連線的方式限制方法（只依 client 的信任程度，例如擴充功能；或依模式，例如 `HeadlessExperimental` 只在 headless），而 `Kun` 視窗的那一半要的是「沒有人在操作」，那是自動化模式的性質；照連線分時，`--automation-pipe --remote-debugging-port` 下 websocket 的工具反而不能用。不選 M2（websocket 一次一條）、M3（整個 kun 一次一個 client）：省下的只是 `AutomationSession` 的一部分重構，但「DevTools 看 machopodriver 或 `bench.py` 正在跑的頁面」這個最常見的除錯情況就不能用，之後的 Playwright 也要 M1。
6. **browser 的方法照舊嚴格：kun 不認識的參數是 `INVALID_PARAMS`（P1）**（2026-10-22，使用者）：第 4 段實際連上 DevTools 時，遇到一個參數決定一個：做出來，或明確列成「收下但不理會」，在程式和 `crates/shell/README.md` 寫明理由；pipe 和 websocket 一樣。不選 P2（只有 websocket 寬鬆）：同一個方法依連線的方式行為不同，和第 5 條不依連線分的原則矛盾，Chrome 也不這樣做。不選 P3（一律忽略不認識的參數，照 Chrome 的向前相容）：Chrome 做了 CDP 的全部，忽略的只會是它真的不知道的新參數；kun 只做了一小部分，忽略的大多是「CDP 有、kun 沒做的」（例如 `Page.captureScreenshot` 的 `clip`），呼叫的人會以為成功了。等 kun 做的範圍夠大，可以再考慮只對 CDP 沒有的參數寬鬆。
7. **照 RFC 0074 的規則寫**（2026-10-22，使用者）：遞增的 id（client、session）用 `base::SequenceNumber`；兩個新的信任邊界設大小的上限：client 送進來的訊息（websocket 和 pipe，超過時關掉連線）、renderer 交回來的 `RendererHostMsg::DevToolsMessage`（超過時丟掉）；命令的 `id` 從 `u64` 到 V8 的 `int` 的範圍另外確認（「待決定」第 12 條）。數字在第 1、2 段定。

第 1 段開始前（2026-10-23，使用者：四條都照建議）：

8. **訊息的大小上限用 `tokio-tungstenite` 的預設值**：單則 64 MiB、單個 frame 16 MiB；pipe 的一則訊息也是 64 MiB；超過時關掉那條連線。CDP 的命令最大的是帶整份 script 的（`Debugger.setScriptSource`），幾 MB。不選更小的數字（例如 16 MiB）：沒有需要擋的更小的東西，數字越小越可能擋到合法的命令。
9. **帶值的開關都多收 `--opt=value`**（Chrome 的寫法，Puppeteer、Playwright 啟動瀏覽器時這樣給），原本的 `--opt value` 照舊。不選只讓兩個新的開關收：同一個程式的開關有兩種寫法規則，之後 Playwright 給的其他開關也要一個個補。
10. **不改名**：HTTP 和 websocket 放在新的 `crates/shell/src/devtools_http_handler.rs`（Chromium 的 `DevToolsHttpHandler`），好幾個 client 的重構改在 `automation_session.rs` 裡；`automation_*`、`protocol::automation` 照舊。要照 Chromium 改成 `devtools_*` 的話，另外做一次純改名的 commit。不選這份裡改名：只是搬家，會碰到很多檔案和文件，混在功能的改動裡不好審。
11. **`/json/list` 的 `devtoolsFrontendUrl` 先填 `devtools://devtools/bundled/inspector.html?ws=127.0.0.1:<port>/devtools/page/<id>`**：在 Chrome 裡打開就是 Chrome 內建的 DevTools（kun 自己沒有前端可以給）。最後的值第 4 段實測後再定（「待決定」第 11 條）。

第 2 段開始前，逐條確認：

12. **JS 的 `console` 改用 V8 的（C1′）**（2026-10-23，使用者）：V8 內建的 `console` 加上 inspector 的 `V8Console`，終端機的輸出從 `consoleAPIMessage` 經過 `FrameClient::console_message` 印；宣告表的 `console` namespace 和 `dom_api` 替它格式化值的部分不再給 JS，第 2 段刪掉（git 的歷史留著，之後 wasm 要時可以拿回來）。和 Chrome 一模一樣（格式、計數、計時、group、訊息的保留和補送、stack、`console.createTask`），V8 升級時自動跟著。推翻 RFC 0028 第 2 條（宣告表的 namespace，B3），它當時的比較已經寫了「之後接 DevTools：要改成 B1」。代價：inspector 一直要在（第 13 條：V8 內建的 `console` 沒有 delegate 時什麼都不做），JS 和之後 wasm 的 `console` 不是同一份實作。不選 C2（留著 kun 的 `console`，JS 的 DevTools 事件用 `V8InspectorSession::wrap_object` 自己組）：一份實作 JS 和 wasm 共用、inspector 可以等到有 client 才建，但要自己對齊 DevTools 期待的事件格式（`timeEnd`、`table`、`count` 的文字和預覽）、自己接 `console.profile`、`time`、`createTask` 和 async stack、為了 stack 改 fork、自己持有保留的訊息的 V8 handle，而且每次升級 V8 都要回頭確認；走這條路的只有 kun。不選 C4（kun 的一份實作負責語意和 stderr，JS 的 realm 另外把同一次呼叫交給 V8 內建的 `console` 讓 inspector 送事件）：語意和計數算兩份，Chromium 沒有這種做法。wasm 之後也要有 `console`（使用者 2026-10-23），怎麼做那時再定（「之後的可能」）。
13. **inspector 和 isolate 一起建，一直在（D1）**（2026-10-23，使用者）：分頁的 renderer 建立 V8 的 isolate 時就建 `V8Inspector`（Blink 的 `MainThreadDebugger` 也一直在），它是 V8 內建的 `console` 的 delegate（第 12 條）。沒有 session 時不讓 script 變慢（`Debugger.enable` 和 async stack 的追蹤要 session 打開才啟動）；成本是每個 realm 的紀錄、`console` 多走一段、保留最近 1000 則訊息（DevTools 晚一點連上也看得到之前的訊息，和 Chrome 一樣），第 2 段一開始量一次有和沒有 inspector 的記憶體（包括 `console` 的引數物件有沒有被留著）和一個 `console` 密集的 microbenchmark，不跑 Speedometer。原本同一天選了 D2（沒有 client 時用 kun 自己的 `ConsoleDelegate` 接 V8 內建的 `console`，第一次有 DevTools 的 session 才建 inspector），接著查到 `v8::debug::ConsoleDelegate` 和 `SetConsoleDelegate` 是 V8 的內部介面（`src/debug/interface-types.h`、`debug-interface.h`，`V8_EXPORT_PRIVATE`，沒有相容的保證，每次升級 V8 都可能要改），delegate 還是 cppgc 的物件（`cppgc::GarbageCollectedMixin`）、有 22 個方法要轉給 Rust，fork 的長期成本比當時估的高，改回 D1。D2 帶出的第 14 到 16 條跟著取消。
14. **（取消）kun 的 `ConsoleDelegate` 用 `dom_api` 的 `console` 做**：2026-10-23 隨 D2 定的，第 13 條改回 D1 之後沒有 kun 的 delegate，不再需要；`dom_api` 的 `console` 照第 12 條處理。
15. **（取消）inspector 接手時 `console` 的狀態接不起來**：2026-10-23 隨 D2 接受的限制，D1 沒有接手這件事，不再有這個限制。
16. **（取消）最後一個 client 離開之後 inspector 留著**：2026-10-23 隨 D2 定的，D1 的 inspector 和 isolate 一起建、一起消失，不再需要。
17. **frame 的 id：main frame 的由 browser 給 renderer，子 frame 沿用 renderer 給 WebDriver 的 id（F1a 加 i）**（2026-10-23，使用者）：分頁一建立，browser 就替它抽 id（放在 `WebContents` 上；Chromium 的 `devtools_frame_token` 也是 browser 建立 frame 時產生、放在建立的參數裡），啟動 renderer 時連同參數交給它；target id、main frame 的 `frameId` 都是它，session 從 `WebContents` 讀，不再自己抽（第 1 段的對照表搬過去，第 2 段開始時改）；renderer 當掉重開，同一個分頁給同一個 id。原本定的是 F1（第一次 attach 時才在 `RendererMsg::DevToolsAttach` 裡給），第 13 條改回 D1 之後對不上：inspector 和 isolate 一起建，realm 一建好就要 `context_created`、`auxData` 帶 `frameId`，那時可能還沒有 client。不選 F1b（attach 時才給，renderer 再把已經登記的 context 先 `context_destroyed` 再 `context_created` 一次）：context 的 id 會換掉，先登記錯的再重來容易漏。子 frame 的 `frameId` 是 renderer 給 `Kun.switchToFrame` 的同一個 id（`<renderer 的隨機前綴>.<frame 的號碼>`，導覽時不變），一個 frame 只有一個 id，和 Chromium 的 `devtools_frame_token` 一樣只有一套；格式和 Chrome 的 32 位十六進位不同，CDP 把它當成不透明的字串。子 frame 的 id 不必猜不到：只在已經接上的 session 裡用得到。不選 F2（renderer 給 main frame 的 id，browser 拿它當 target id）：被攻破的 renderer 能挑一個猜得到的 target id，renderer 一換 target id 也跟著換。不選 F3（browser 轉送時換掉 JSON 裡的 id）：browser 要改寫 renderer 的訊息，違反「不解讀 renderer 的回答」。不選另給子 frame 一個 32 位十六進位的 CDP id（ii）：一個 frame 兩個 id，之後同時用 WebDriver 和 CDP 的工具要對應。
18. **rusty_v8 的修改一次補，第 2 段開始時：`currentTimeMS` 和 `exceptionRevoked`**（2026-10-23，使用者）：`V8InspectorClient::currentTimeMS`（沒接時回答 0：`console.time`、`consoleAPICalled`、`exceptionThrown` 的時間都是 0）、`V8Inspector::exceptionRevoked`，都是 `v8-inspector.h` 的公開 API，一起改 `src/binding.cc`、重編 V8 一次、換一次共用的 artifact；照 fork 的規則標 `kun:`、記進 `README.machopo.md`，同一次改動更新 `third_party.toml`，先 push fork 再 push kun，並告訴其他機器。不選分兩次：多重編一次、多換一次 artifact，其他機器（2026-10-23 有一台在平行做 RFC 0078）要多同步一次。原本也要綁 `connectShared`（停在斷點時 client 斷線，session 要等巢狀迴圈回來才能刪），寫 binding 時發現它的 `Channel*` 版本解決不了：session 可能活得比 kun 擁有的 `Channel` 久，V8 自己的註解也說只能「部分」確保；完全安全要用 `ManagedChannel`（`Channel` 是 cppgc 的物件），binding 複雜得多。改成（甲，同一天，使用者）照舊用 rusty_v8 已經有的 `connect`，由 kun 自己延後刪除：巢狀迴圈跑著時不刪 session，等它回來再刪（V8 的說明本來就要求 embedder 這樣做），session 和 `Channel` 一起放掉。V8 的標頭把 `connect` 標成 deprecated（「改用 `connectShared`」）：哪個 V8 版本拿掉它，那時再綁 `connectShared` 加 `ManagedChannel`（乙）。
19. **命令的 `id` 照 Chrome 是 `i32`（A）**（2026-10-23，使用者）：`id` 是 `i32` 範圍內的整數（負數可以，`1e3` 這種整數值的寫法也可以），否則送回沒有 `id` 的錯誤 `{"error": {"code": -32600, "message": "Message must have integer 'id' property"}}`；`protocol::automation` 的 `Command.id` 從 `u64` 改成 `i32`，交給 V8 時不用換 id（RFC 0074 的 `u64` 轉 `int` 的問題跟著解決）。Chrome for Testing 152 的實測見 `details.md` 的「命令的 `id`」：browser 和頁面的 session 都一樣，`2^31` 以上和 `1.5` 拒絕、`-1`、`1e3` 接受；kun 原本收 `u64`、不收負數和 `1e3`，不合法時只記 log、不送任何東西。不選 B（照舊收 `u64`，交給 renderer 時換成 browser 自己的 `i32` id）：和 Chrome 不同，browser 要記住對應。
20. **和 Chrome 比：一個即時比較的工具（D-a）**（2026-10-23，使用者）：`test.py` 的一個指令（例如 `python3 scripts/test.py cdp`，借用 `scripts/bench/common.py` 的 `ChromePipe`、`KunPipe`）對 kun 和 Chrome for Testing（bench 釘住的版本，和 kun 的 V8 同一個主版本）送同一組命令，把每次都會變的值（`objectId`、`scriptId`、`executionContextId`、時間）換成固定的記號再比，列出不同的地方；不用 CI 跑（使用者），每一段做完、改了轉送、context、`console` 時在本機跑，結果記進 `progress.md`，也能當 mutation test 的測試指令。kun 自己的行為（轉送、session、context 和 frame 的 id、停住和斷線）照舊由 `cargo test` 守著。不選只用 `cargo test`、預期的值手抄（D-b）：容易漏欄位，Chrome 升級時不知道哪些要重抄。不選錄成黃金檔、`cargo test` 和它比（D-c，Blink 的 `inspector-protocol` 測試的做法）：不需要 CI 跑時，多出來的只是沒有 Chrome 的機器也能比，卻多一套黃金檔要維護。
21. **終端機的 `console` 帶來源（S2）**（2026-10-23，使用者）：`[console] hello 1,2 (https://example.com/app.js:12)`，來源是 `consoleAPIMessage` 給的網址和行號，沒有網址時（`eval` 的程式）不加括號；和 kun 沒接住的例外的 `[uncaught] 訊息 (網址:行)` 同一種寫法。等級照舊不印。不選照舊只印訊息（S1）：看不出是哪個 script、哪一行印的。不選照 Chrome 的後半段 `"hello 1,2", source: <網址> (12)`（S3）：前半段是 Chromium 的 log 格式，kun 的不同，只像一半，又和 kun 的 `[uncaught]` 不一致。
22. **DevTools 選錯 context：留到第 4 段（甲）**（2026-10-23，使用者）：第 2 段的手動確認時，DevTools 的 Console 一開始停在 iframe 的 context（在裡面輸入 `app` 是 `ReferenceError`）。DevTools 的前端靠 frame 樹（`Page.getResourceTree`）認出 main frame 的 context、自動選它，找不到時停在第一個收到的；V8 回報 context 的順序 iframe 在前（Chrome for Testing 152 也一樣，2026-10-23 實測），kun 還沒有 frame 樹。第 2 段的驗收手動在 Console 的 context 選單切換（輸入 `location.href` 認），第 4 段第一項補 frame 樹。選單上的每一項也只寫 origin（同源的 iframe 和 main frame 看起來一樣；Chrome 照 frame 樹寫 `top` 和 iframe 的名字）。不選乙（第 2 段先補只有 id、網址、父子關係的 `Page.getResourceTree`、`Page.getFrameTree`，browser 轉給 renderer 回答，Chromium 的 `InspectorPageAgent`）：要先定 frame 樹的格式，那是第 4 段錄下 DevTools 一連上送的命令之後一起決定的事；代價是第 3 段做完之前，每次打開 DevTools 都要自己切一次 context。
23. **renderer 的 DevTools 訊息上限 256 MiB**（2026-10-23，使用者確認第 2 段的實作）：client 的訊息上限 64 MiB 的 4 倍，回答可能比命令大得多（`Debugger.getScriptSource` 的 script、CPU profile、heap snapshot 的一段）；超過時 browser 丟掉、記 log（renderer 不可信）。不選和 client 一樣 64 MiB：合法的大回答可能過不了。不選沒有上限：拆成 process 之後，被攻破的 renderer 能送巨大的字串。
24. **rusty_v8 多補三個 binding**（2026-10-23，使用者確認）：第 18 條的 `currentTimeMS`、`exceptionRevoked` 之外，`Message::get_script_id`（`exception_thrown` 要 script 的 id，沒有時 V8 不把例外本身放進 `exceptionDetails`、`text` 也和 Chrome 不同）、`V8Inspector::context_created_with_origin`（上游的 `context_created` 不給 `V8ContextInfo::origin`，context 的 `origin` 是空的）、`V8Inspector::reset_context_group`（main frame 導覽時的 `executionContextsCleared`）。都是 `v8-inspector.h`、`v8-message.h` 的公開 API，和第 18 條的兩個同一次重編、同一次換 artifact。不選留到之後：每一個都讓 DevTools 看到的和 Chrome 不同（`cdp_compare` 比得出來），之後補要再重編、再換一次 artifact。
25. **錯誤的完整 stack 交給 inspector 開關**（2026-10-23，使用者確認）：拿掉 kun 一直開著的 `SetCaptureStackTraceForUncaughtExceptions(true, 10)`，照 Chrome 只在有 session 打開 `Runtime` 時記（V8 的 inspector 在 `Runtime.enable`、`disable` 時自己開關）。不選照舊一直開著：每建一個錯誤都要付，而且 inspector 在 `Runtime.disable` 時會把它關掉，兩邊互相蓋掉；kun 自己的錯誤回報（`[uncaught]`、`ErrorEvent`）不用這份 stack。
26. **所有 script 標成 shared cross-origin**（2026-10-23，使用者確認）：三個建立 `ScriptOrigin` 的地方（classic script、module、event handler 的 content attribute）都給 `is_shared_cross_origin = true`，V8 才把例外本身放進 `exceptionDetails`。kun 不 mute 任何 script 的錯誤（`docs/limitations.md`），標上去和現在的行為一致；Blink 不 mute 的 script 也是這樣標（`SanitizeScriptErrors::kDoNotSanitize`）。之後做跨源 classic script 的 mute 時，那些 script 改成 `false`。
27. **console 的輸出口在 `ScriptEngine::set_console_sink`**（2026-10-23，使用者確認）：不照原本寫的 `FrameClient::console_message`。V8 的 `consoleAPIMessage` 只給 context group，不給 frame 或 context，輸出口只能是 isolate 的；renderer 建 DOM 時設一次。不選從訊息的網址反查 frame：同一個 script 可能在好幾個 frame 裡。
28. **fork 的 release 等第 3 段做完再發**（2026-10-23，使用者）：第 18、24 條的 fork 修改先只在本機重編（`ci.py v8 --profile release`），fork 不 commit、不發 release，`third_party.toml` 和 `RUSTY_V8_MIRROR_TAG` 不換；第 3 段做完時連同它可能要的修改一起 commit、發一次 release、換一次 artifact。那之前 `0075` 只在這台機器上 build 得起來（要 `RUSTY_V8_ARCHIVE` 指著本機的 V8），第 1、2 段也不 merge 回 `main`。不選第 2 段做完就發：第 3 段要是還要改 fork，就要再發一次、其他機器再同步一次；第 3、4 段需要的 V8 API rusty_v8 大多已經綁了，但要做下去才確定。
29. **`Profiler` 換成 inspector 的（甲）**（2026-10-23，使用者）：`Profiler.*` 不再由 browser 攔下，照其他方法轉給 renderer，由 V8 inspector 的 `V8ProfilerAgentImpl` 回答（Chromium 也是）；拿掉 browser 的 `Profiler.*` 型別、`RendererMsg::Profiler`、renderer 的 `profiler()`、`ScriptEngine` 的 `start_profiling`、`stop_profiling` 和 fork 的 `cpu_profiler` 模組（和第 28 條的 release 一起）；閒置照 Blink 的 `ThreadDebugger::IdleStarted` 改成告訴 inspector（`V8Inspector::idle_started`、`idle_finished`；`ScriptEngine::set_idle` 留著，renderer 照舊在每個 task 前後呼叫）。profile 由 inspector 組，和 Chrome 的一樣，`Profiler.stop` 的時間只有低 32 位元的限制跟著消失，DevTools 的 JavaScript Profiler 面板也能用；`Profiler` 不再只在自動化模式，wasm 的文件是 -32601（原本 -32000）。`bench.py` 的 `--profile` 要重新確認結果不變。不選乙（保留 kun 自己的）：同一組方法兩份實作，DevTools 用的也是 kun 的，32 位元的限制留著。
30. **DevTools 的訊息走另一條佇列（甲）**（2026-10-23，使用者）：browser 送給 renderer 的 DevTools 訊息（attach、detach、命令）進 renderer 的 DevTools 佇列，主佇列另外放一個叫醒 main loop 的 task；平常 main loop 在最上層處理，停住時巢狀迴圈只等這條，其他 task 留在主佇列，繼續之後照原順序。DevTools 的訊息和其他瀏覽器訊息之間不保證順序（Chrome 也是：不同的 mojo pipe）。Chromium：DevTools 的訊息經過 IO thread 進 `InspectorTaskRunner` 的佇列，`ClientMessageLoopAdapter` 的巢狀迴圈只處理它。不選乙（同一條佇列，巢狀迴圈從主佇列挑）：巢狀迴圈在 V8 的 callback 裡，碰不到外層 `RenderThread` 的佇列，挑剩的 task 要放回最前面，順序容易錯。
31. **只有幾個方法打斷跑著的 script（甲）**（2026-10-23，使用者）：照 Chromium 的 `DevToolsSession::ShouldSendOnIO`，`Debugger.pause`、`resume`、設定和移除斷點、`getScriptSource`、`getPossibleBreakpoints`、`getStackTrace`、`Runtime.terminateExecution` 經過 `Isolate::RequestInterrupt` 在 script 跑著時就處理（它們不執行頁面的 script）；其他的（包括 `Runtime.evaluate`）等 script 跑完。由 browser 判斷（它知道方法名稱），在訊息上標著。不選乙（每則都打斷）：`Runtime.evaluate` 會在頁面的 script 中間執行頁面的 JS，和 Chrome 不同。不選都不打斷：DevTools 的暫停鍵對跑著的 script 沒用。
32. **停住時斷線、關分頁（10c）**（2026-10-23，使用者）：斷線的 detach 經過 DevTools 佇列，巢狀迴圈跑著時先讓那個 session 關掉 `Debugger`（沒有別的 session 開著時 V8 自己繼續），回來之後再刪 session（第 18 條）；關分頁、renderer 要結束時，經過 DevTools 佇列送一個「離開」的訊號，巢狀迴圈讓 V8 繼續，回到 main loop 再處理結束（Chromium 拆掉 frame 時 `MainThreadDebugger` 也 `QuitMessageLoopOnPause`）。沒有替代：不處理的話 renderer 卡在巢狀迴圈，browser 等它結束時 UI 跟著卡住。停住期間 compositor 的捲動和瀏覽器的 UI 照常，machopodriver 的命令和等 input 的 ack 的要等（Chrome 也是）。
33. **frame 樹由 renderer 回答（A1）**（2026-10-23，使用者）：第 4 段錄下 Chrome for Testing 152 的 DevTools 一連上送的命令（87 個、62 種：15 種成功，47 種 -32601，沒有卡住的），只有 frame 樹的缺少讓看得到的功能壞掉（Console 選錯 context、Sources 的檔案樹和 `Cmd + P` 是空的）。renderer 加一層 Blink 那樣的 agent（`InspectorPageAgent`）：命令交給 engine 之前，先看是不是它自己回答的（`Page.getResourceTree`、`Page.getFrameTree`、`Page.getResourceContent`），frame 的 id、父子關係、網址、origin、MIME type 都在 renderer 手上。`Page.enable`、`Page.disable` 照 Chromium 的 fall through：browser 記下來之後再轉給 renderer，由 renderer 回答，renderer 打開時送 `Page.frameNavigated`、`frameAttached`、`frameDetached`（沒有這些事件，導覽之後 DevTools 的 frame 樹就舊了）。不選 A2（browser 回答）：browser 只知道 main frame，子 frame 還是要問 renderer。
34. **文件的原始碼由 renderer 留著（B1）**（2026-10-23，使用者）：每份文件留著它的原始 HTML（解碼後的文字），文件丟棄時跟著丟掉，`Page.getResourceContent` 從它回答；其他資源回答沒有快取（Chrome 的 `Content unavailable. Resource was not cached`），script 有 `Debugger.getScriptSource`。成本是每份文件多一份文字。Chromium 的 `InspectorPageAgent` 也從留著的回應（fetcher、memory cache）回答，不序列化 DOM。不選 B2（不給）：Sources 列得出 HTML 卻打不開，inline script 還是看不到。不選 B3（browser 重抓）：可能和頁面拿到的不同（動態的頁面、POST），Chromium 也不這樣做。
35. **screencast 這份 RFC 不做（C1）**（2026-10-23，使用者）：`Page.startScreencast` 照舊 -32601，DevTools 左半邊的 screencast 是黑的，可以關掉；記進 `docs/limitations.md` 和「之後的可能」。不選 C2（browser 每個新的 frame 讀回、編成 JPEG、送 `Page.screencastFrame`，照 ack 控制速度；`Input.dispatchMouseEvent`、`dispatchKeyEvent` 已經有了）：不影響其他面板，每個 frame 要從 GPU 讀回加編碼，值得另外做。
36. **`/json/list` 照舊是 `inspector.html` 和 `type: "page"`（D1）**（2026-10-23，使用者）：第 2、3 段的手動確認裡 DevTools 打得開，Console、Sources、Memory 都能用。不選 D2（只有 JS 的 `js_app.html`）：會藏起 Elements、Network，`chrome://inspect` 對 `type: "page"` 的分頁不一定用它，之後有了 DOM 又要改回來。
37. **其他沒有的方法照舊回答 -32601（E1）**（2026-10-23，使用者）：錄到的其他 46 種（Elements 的 `DOM`、`CSS`、`Overlay`，`Network`、`Emulation`、`Storage`、`Target.setAutoAttach`……）不假裝支援；DevTools 都容忍，每個面板都打得開。Chromium 也只回答它做的方法，也是 shell 的「名字相同，語意就相同」。不選 E2（都回答 `{}`）：DevTools 會以為功能在，卻什麼都不發生，更難看出缺什麼。

## 待決定

都決定了（「已決定的事項」第 1 到 37 條，第 14 到 16 條取消）。還沒做的，限制寫在 `docs/limitations.md`，之後可以做的列在下一節。

## 之後的可能

- 頁面和網路、Elements、Playwright（「動機」的路線第 2 到 4 項）。
- **screencast**（第 35 條，不在這份做）：`Page.startScreencast`，browser 每個新的 frame 讀回、編成 JPEG、送 `Page.screencastFrame`，照 DevTools 的 ack 控制速度；DevTools 的滑鼠和按鍵經過已經有的 `Input.dispatchMouseEvent`、`dispatchKeyEvent`。成本只在 screencast 開著時付。
- **`frameAttached` 的 stack**：建立 frame 的 script 的 stack（Chrome 給，DevTools 的 frame 詳細資料顯示「Created by」）。
- **`Tracing` 經過 CDP**：kun 的 trace 現在由 `MACHOPO_TRACE` 環境變數決定、從啟動錄到結束；`bench.py` 的 Chrome 那一側已經用 `Tracing.start`、`Tracing.end`。kun 也有 `Tracing` 之後兩邊錄同一個區間。不需要 inspector，哪時候做都可以。
- **WebDriver BiDi**：底層和這份共用（websocket、V8 的 inspector），WPT 的 testdriver 裡只有 BiDi 才有的部分（`console/` 的 log）會跟著解開。
- **kun 內建的 DevTools**：devtools-frontend 包進 kun、`devtools://`、`InspectorFrontendHost`、停靠在視窗裡的第二個網頁 view。
- **`alert`、`confirm`、`prompt`**：同樣要 renderer 的巢狀迴圈，可以共用這份做的。
- **wasm 的文件的 `console`**：之後 wasm 的文件也要有 `console`（使用者 2026-10-23）。可能的做法：
  - 從 git 的歷史拿回 `dom_api` 的 Console 規範的實作（`dom_api` 的 `console` 模組和它的宣告，第 2 段刪掉了），只給 wasm 用：成本最小；JS（V8 的）和 wasm（kun 的）的語意各自維護。
  - 一份 kun 自己的實作給 JS 和 wasm 共用，JS 的 DevTools 事件由 kun 用 `wrap_object` 組（「已決定的事項」第 12 條的 C2）：完全共用，代價見那一條。
  - 一份 kun 的實作負責語意和 stderr，JS 的 realm 另外把呼叫交給 V8 內建的 `console` 送 DevTools 的事件（第 12 條的 C4）。
  
  wasm 的文件要在 DevTools 的 Console 看到訊息，不管哪個做法，都還要 kun 自己替它做 `Runtime` 的 execution context（它沒有 V8 的 inspector）。
- **pipe 改用 fd 3、4**（Chrome 的 `--remote-debugging-pipe`；Windows 是 C runtime 的 fd 3、4 或 `--remote-debugging-io-pipes` 的 handle）：kun 的 pipe 現在用 stdin、stdout。換了之後 stdout 空出來（`docs/limitations.md` 的「WebDriver 和自動化」裡，網頁的輸出和 kun 的 log 混在 stderr 的那一條可以拿掉），Playwright 的 `launch()`、Puppeteer 的 `pipe: true` 能像啟動 Chrome 一樣啟動 kun。代價：kun 把繼承到的 fd 變成 `File` 要 `unsafe`，只能放在還沒建立的 `ipc`；啟動 kun 的 Rust 程式（machopodriver、`shell` 的測試）要在子 process 裡 `dup2`（`pre_exec`，也是 `unsafe`），Windows 要指定繼承的 handle。2026-10-22 和使用者決定不放在這份（DevTools 走 websocket，用不到），留到 Playwright 的 RFC 或一份獨立的小 RFC；第 1 段的 client 不假設 pipe 接在哪兩個 fd 上。
