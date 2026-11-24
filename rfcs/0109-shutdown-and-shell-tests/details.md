# RFC 0109 的細節：結束的流程（graceful shutdown）

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

兩個盤點（結束的流程、`shell` 的測試）是 2026-11-22 到 23 做的，基準是 `647ab5a`，在 Linux 上（Fedora 45、24 個邏輯 CPU、`MACHOPO_GPU_ADAPTER=cpu` 的 lavapipe、debug build）。重複跑了幾次、各壞幾次在 `progress.md`。macOS 和 Windows 沒有量。

## 2026-11-24 的重新評估

RFC 0110、0111、0112 merge 之後（`main` 的 `4e5e0ef`）對照程式和 CI 重看兩個盤點。

**每一類現在的狀態**：

| 類 | 盤點時（`647ab5a`） | 2026-11-24 |
| --- | --- | --- |
| A1 到 A6（產品的競爭） | 根因是擁有權，等 RFC 0110 | 0110 修掉，每一個都有必壞的測試（0110 的 `progress.md`） |
| B1、B2（算畫了幾張） | 測試等錯了東西 | RFC 0112 照本意改寫：第一張 frame 的像素和時間、load 之後畫了幾張 |
| B3 到 B8 | 測試等錯了東西 | 程式沒動，照下面的審核改寫 |
| C1、C2（`unused_port`） | 搶資源 | 程式沒動（`net::test_server::unused_port` 照樣放掉 port 才回傳） |
| D1 | 修掉了 | |
| S1 到 S9（結束） | 盤點 | 對過程式都還在：`CHILD_EXIT_WAIT`、machopodriver 的 `CLOSE_TIMEOUT` 都是 5 秒，`shell` 沒有處理訊號，`BrowserWindow` 沒有 `Drop`，pipe 的 writer 照樣 `join`。量到的時間在第 3 段重量 |

**CI**（`gh run list --workflow CI --limit 40`，2026-11-22 到 24）：0110 merge（2026-11-23 13:52）之後 9 次，壞過 1 次（45197631781，B1，之後 0112 改寫了），之後 7 次綠燈。之前壞的是 A1、A2、A3。

### B3 到 B8 的審核

逐一讀過測試和它要守的行為，六個都是測試假設了機器不忙、或時間不會剛好相等，產品照規範是對的：

- **B3**（`documents::idle_callbacks`）：要求第一個 idle callback 的 `timeRemaining()` 大於 0。HTML 的 `timeRemaining()` 在 idle period 的期限過了之後是 0，CPU 被搶走時 callback 開始的那一刻期限已經過了。
- **B4**（`automation_pipe::the_profiler_samples_the_pages_script`）：200 ms 的 script 要求 20 ms 間隔的樣本不超過 25 個、0.5 ms 的至少 100 個。樣本的個數取決於 V8 的 sampler thread 拿到多少 CPU；`Profile` 的 `timeDeltas` 記著每個樣本之間的時間，不受影響。
- **B5**（`automation_pipe::the_profile_tells_waiting_from_rendering`）：200 ms 的 sleep 要求 `(idle)` 至少 200 個樣本，同 B4。
- **B6**（`documents::about_blank_has_its_initiators_origin`）：`<iframe id=same src=/child.html>` 在 `<script>` 之前，parser 照時間讓出時，iframe 在 script 設 `same.onload` 之前就載完，`load` 等不到。parser 讓出合規範（Chrome 也讓出）。
- **B7**（`web_animations::a_replaced_animation_is_removed`）：兩個動畫的 `duration` 都是 50 ms，剛好是 3 個 headless 的 frame（frame 的時間是 1000/60 的倍數），有時 `finish` 和 `remove` 排定的時間完全相等；Web Animations 的事件先照排定的時間、同時的照 composite order 排，a 的兩個事件就排在一起（`finish a remove a finish b`）。Chrome for Testing 152 跑 5 次都是測試要的 `finish a finish b remove a …`：它的 frame 照真的時間，碰不到相等。
- **B8**（`transitions::a_frames_events_go_by_when`）：transition 的 pending 在第一次 tick 之前解決時，`transitionrun` 和 `transitionstart` 的時間相同，照 composite order 排成 `run b / start b / run a / start a`（合規範）。Chrome for Testing 152 跑 5 次都是 `run b / start b / end b / run a / start a / end a`：照 transition 分組，和規範的先照時間排不同，也和測試要的 `run b / run a / start b / start a / end b / end a` 不同。

## CI 上壞過的

2026-11-22 用 `gh run list --workflow CI --limit 60` 和每一次的 `--log-failed` 查的，涵蓋 2026-11-18 到 22：60 次裡 24 次成功、23 次失敗、12 次取消、1 次還在跑。CI 只有 Linux（Ubuntu，4 顆 CPU，沒有 GPU）。下面是 23 次失敗各壞了什麼；`0004` 是多 process 的那份 RFC 的 branch。

| 壞的測試 | crate | 次數 | 在哪裡 | 知道的原因 |
| --- | --- | --- | --- | --- |
| `devtools_css::sheets_coming_and_going_are_told` | `shell` | 15 | `main` 8 次（2026-11-19 到 21）、`0004` 7 次 | 查到了（下面的 A1）。每次都在 `devtools_css.rs` 的同一行：加了 `srcdoc` 的 iframe 之後等 30 秒收不到 `CSS.styleSheetAdded`，留著沒讀的只有 `DOM.childNodeCountUpdated`，machopo 還在跑 |
| `process_channel::a_host_and_a_child_talk` | `ipc` | 3 | `0004` 2 次、`main` 1 次 | 測試自己的競爭：child 先回答、再說讀完了，下一筆送出時 host 的那塊共用記憶體還沒還回來，host 就多建一塊。`5136d5c` 改成每一筆都等。當時我以為是 tile 走共用記憶體那一步帶出來的，但它在那一步之前的 `main` 上就壞過 |
| `renderer_process` 的兩個 `about:crash` 測試 | `browser` | 2（其中 1 次同時壞了上面的 `devtools_css`） | `0004` | Linux 上 `abort()` 要等系統收完 core dump，通道才斷。改成先設成不留 core dump |
| `devtools_log::commands_machopo_cant_carry_out_are_warned_of`、`devtools_overlay::other_overlays_are_answered` | `shell` | 1（同一次） | `0004` | Browser 分段寫 stderr 的一行，renderer process 的一行插在中間。改成一次寫出 |
| `documents::history_through_the_browser` | `shell` | 1 | `0004` | 查到了（下面的 A4）。`location.hash` 的捲動晚了：兩個 frame 之後還沒捲、返回之後卻捲了（不是原本寫的「兩個值順序相反」）；在測試的 process 裡跑 |
| `automation_pipe::the_system_info_tells_the_gpu_adapter` | `shell` | 1 | `main`（2026-11-18） | `29a94c7` 修掉的結束時 crash：還原它就重現（下面的 D1） |
| （lint：測試編不過） | `browser` | 1 | `0004` | 不是不穩定：commit 之前沒有重跑 clippy |

更早的（不在這 60 次裡，從記錄來的）：2026-11-18 兩次 Linux 的不穩定（Viz 的 device 在全域變數裡、`exit` 時 lavapipe 還在忙；被放掉的 renderer 晚到的訊息落在重複使用的 frame sink 上），都修了；2026-11-10 兩次「machopo 沒有結束」（見下面的「2026-11-10 的兩次沒有結束」）。再往前到最近 100 次 CI 的範圍，`automation_pipe` 的幾個測試和 `remote_debugging::a_message_over_the_limit_ends_the_pipe` 也在結束時失敗過，後者印出 SIGSEGV，都早於 `29a94c7`。

盤點期間的下一次 CI（2026-11-22，`647ab5a`，45028139535）壞了兩個：`devtools_css::sheets_coming_and_going_are_told`（A1）和 `devtools_network::requests_are_told_with_their_responses_and_bodies`（A3），後者和本機重現的一樣是查到的同一類。

## 結束的流程的盤點

### 重量（第 3 段，2026-11-24，`59492771`）

量法同下面的「量法」，只量 automation pipe（視窗和 machopodriver 走的程式沒變，照程式對）：每一格三次，debug build、預設的 GPU，頁面用 `localStorage` 寫一筆，結束之後另一個 machopo 用同一個 profile 讀回來（「寫完」），並看結束時 renderer process 留下沒有。卡住的頁面是 timer 裡的無窮迴圈。

| 要它怎麼結束 | 分頁 | 時間（秒） | renderer 留下 | profile |
| --- | --- | --- | --- | --- |
| EOF | 閒著 1 個 | 0.02–0.03 | 沒有 | 寫完 |
| EOF | 閒著 8 個 | 0.06 | 沒有 | 寫完 |
| EOF | 卡住 1、2、3 個 | **5.02、10.03–10.06、15.04–15.07** | 沒有 | 寫完 |
| EOF，`--single-process` | 卡住 1 個 | **20 秒內不結束**（之後 kill） | | 寫完（`localStorage` 在那之前照它的時間表寫了，和結束無關） |
| EOF，`--single-process` | 閒著 1 個 | 0.02 | | 寫完 |
| `Browser.close` | 卡住 1 個 | **5.02–5.05** | 沒有 | 寫完 |
| `Browser.close` | 閒著 1 個 | 0.02–0.03 | 沒有 | 寫完 |
| SIGTERM、SIGHUP | 閒著、卡住 | 0.01–0.02，被訊號結束 | 沒有 | **沒寫** |
| SIGINT | 閒著 | 0.01，被訊號結束（第一輪）；**20 秒內不結束**（第二、三輪） | 沒有 | 第一輪沒寫 |

SIGINT 的兩種結果是量法造成的：第二、三輪的量測從 shell 的 `&` 啟動，非互動的 shell 讓背景的 job 生下來就忽略 SIGINT，machopo 照 Unix 的慣例留著繼承來的「忽略」。

和盤點時（`647ab5a`）一樣：S1（`--single-process` 卡住）、S2（一個一個各等 5 秒）、S4（訊號什麼都不寫）。S3、S5、S7、S8、S9 照程式對過都還在。S6 第 2 段修掉了。

多的 thread：`HttpCacheStore`（RFC 0113）。它和 `CookieStore` 一樣：擁有它的 `SqlStoreThread` 被丟掉時送 `Close`、`join`，沒有時限；在最後一份 handle 被丟掉的那條 thread 上等。

### 量法

真的 `machopo` 和 `machopodriver`，每一格三次：載入頁面、等它卡住、要它結束，記 Browser 結束和最後一個子 process 結束的時間，15 秒沒有結束就用 gdb 抓所有 thread 的堆疊（超過 15 秒的另外用 25 秒再量）。視窗在沒有螢幕的 mutter 裡量。關視窗的按鈕沒有量：它和 pipe 的 EOF 走同一個 `App::exit`。

### 每一種跑法實際發生什麼

時間是秒。「不結束」是量的時限內沒有結束；實際讀過堆疊的有三份，其餘的原因是照程式對的。登出那一格各只量 1 次。trace「缺最後一段」是照檔案的大小判斷的，沒有 parse。

| 要它怎麼結束 | 頁面 | pipe | pipe `--single-process` | 視窗 | 視窗 `--single-process` |
| --- | --- | --- | --- | --- | --- |
| EOF、`Browser.close` | 閒著 | 0.02 | 0.02 | 0.02–0.04 | 0.02–0.03 |
| EOF | 閒著，8 個分頁 | 0.05–0.08 | 0.04 | 沒有量 | 沒有量 |
| EOF | 1、2、3 個分頁卡住 | **5、10、15** | **不結束** | **5、10**（3 個沒有量） | **不結束** |
| `Browser.close`、關最後一個分頁 | 1 個卡住 | **5** | **不結束** | 沒有量 | 沒有量 |
| EOF | worker 卡住、動畫、沒有回應的 fetch 和圖片 | 0.03 以內 | 0.02 以內 | 沒有量 | 沒有量 |
| EOF | client 不讀 stdout、回答塞滿了 pipe | **不結束** | **不結束** | 同一段程式，沒有量 | 同一段程式，沒有量 |
| 只關 stdout、不關 stdin | 閒著 | 不結束（照設計） | 不結束（照設計） | 沒有量 | 沒有量 |
| SIGTERM、SIGINT、SIGHUP | 任何 | 0.01，**什麼都沒寫完** | 同左 | 同左（只量了 SIGTERM） | 同左 |
| SIGKILL | 任何 | 0.02 以內，沒有殘留 | 同左 | 同左 | 同左 |
| 和桌面的連線斷了（登出） | 閒著、卡住 | | | 0.03、**5**，代碼 1 | 沒有量 |

「卡住」試了六種，結果都一樣：timer 裡和頂層的無窮迴圈、`requestAnimationFrame` 裡的、會回溯很久的正規表示式、停不下來的 microtask、一直配置。

| machopodriver | 頁面 | 每個分頁一個 process | `--single-process` |
| --- | --- | --- | --- |
| 刪 session | 閒著 | 0.02–0.03 | 0.02–0.03 |
| 刪 session | 1、2 個卡住 | **5，driver kill 掉 machopo** | **5，driver kill 掉 machopo** |
| driver 被 SIGTERM、SIGKILL | 閒著 | 0.02 | 0.01 |
| driver 被 SIGTERM、SIGKILL | 卡住、正在執行 script | **machopo 多留 5 秒** | **machopo 永遠留著** |

headless 不是 `machopo` 的一種命令列，是測試的 process 裡的 `shell::headless`：結束時呼叫同一個 `WebContents::shutdown`，頁面卡住時和 `--single-process` 一樣永遠等（照程式，沒有另外量）。

profile 和 trace：

| 怎麼結束 | cookie 和 `localStorage` | Browser 的 trace | renderer 的 trace |
| --- | --- | --- | --- |
| EOF、`Browser.close`，閒著 | 寫完 | 寫完 | 寫完 |
| EOF，卡住 | 寫完（5 秒之後） | 寫完（5 秒之後） | **缺最後一段**（被 kill） |
| EOF，卡住，`--single-process` | **沒有寫** | **沒有寫完** | |
| SIGTERM、SIGKILL | **沒有寫** | **缺最後一段** | **缺最後一段** |

### 找到的問題

| | 問題 | 在哪裡 | 違反目標 |
| --- | --- | --- | --- |
| S1 | `--single-process` 時頁面卡住就永遠不結束：`join` 沒有時限，也沒有打斷 script（worker 有 `TerminateExecution`，main thread 沒有） | `browser` 的 `RenderProcessHost::shutdown` | 1、2、4 |
| S2 | 卡住的 renderer 一個一個等，各 5 秒 | `BrowserView::shutdown`、`end_child`、`CHILD_EXIT_WAIT` | 1 |
| S3 | machopodriver 的時限也是 5 秒，所以一個卡住的分頁就一定 kill Browser，它什麼都來不及寫 | `driver` 的 `Machopo::close` | 1、4 |
| S4 | 沒有處理 SIGTERM、SIGINT、SIGHUP：照預設直接死，profile 和 trace 都沒寫完 | `shell` 的 `main` | 1 的「收到 SIGTERM」、4 |
| S5 | client 不讀 stdout 時 writer 卡在 `write`，main 永遠 `join` 它 | `automation_pipe::run` 最後、`window.rs` 的 `App::exit` | 1 |
| S6 | 啟動到一半失敗時 crash：`--remote-debugging-port` 被占用時 `?` 直接 return，沒有呼叫 `BrowserWindow::shutdown`，Viz 還在建 device 時 `exit` 跑了 C++ 的解構子，以 SIGSEGV 或 SIGABRT 結束，約一半的機率。pipe 的 reader、writer 建不起來的兩個 `?` 是同一條路，`BrowserWindow` 沒有 `Drop` | `automation_pipe::run`、`BrowserWindow` | 3 |
| S7 | `exit` 時還有 thread 在跑（下面的表）：Browser 的 IO thread、GPU 驅動程式的幾十條、V8 的 worker。正常結束時它們閒著、沒有看到 crash，但沒有東西保證 | 下面兩張表 | 3 |
| S8 | Viz 的三處 `process::exit(1)`：其他 thread 照跑、profile 不寫、`exit` 的解構子和它們同時跑（照程式，沒有引發來量） | `WgpuRenderer::recreate_context` | 3、4 |
| S9 | renderer process 繼承 Browser 的 stdin 和 stdout，也就是 automation 的 pipe：它活著時讀的人等不到 EOF，它寫到 stdout 的東西會混進訊息 | `ipc::platform::spawn` 的呼叫者沒有設 stdio | 2 |

只關 stdout 時不結束不算問題：pipe 照設計在輸入結束時結束，Chromium 的 `DevToolsPipeHandler` 寫不進去時也只記一行 log。

### Browser process 的 thread

「`exit` 時」是正常結束、`main` return 的當下實際列出來的。

| thread | 誰建、誰擁有 handle | 誰 join | 阻塞在 | 它不結束時 | `exit` 時 |
| --- | --- | --- | --- | --- | --- |
| `main` | | | event loop 或 `windowless::drive` 的 `select!` | | |
| `VizCompositorThread` | `BrowserWindow::start`，`viz_thread` | `BrowserWindow::shutdown`，沒有時限；只有明確呼叫時（沒有 `Drop`） | 訊息；結束時 `device.poll` 無限等 GPU | main 卡在 join；沒有 join 就是 S6 | 沒了 |
| `Chrome_IOThread`（renderer 是 process） | `io_thread()` 的 static，handle 丟掉 | 沒有人 | `IoLoop` 的 wait | 沒有影響，跟著 process 死 | **還在** |
| `Chrome_IOThread`（同一個 process，每個 renderer 一條） | `RenderProcessHost` | `shutdown`，沒有時限 | renderer 的佇列 | renderer 不結束它就不結束（S1） | 沒了 |
| `CrRendererMain`（同一個 process） | `RenderProcessHost` | `shutdown`，沒有時限 | scheduler，或 script | S1 | 沒了 |
| `Compositor`（同一個 process） | renderer 的 main thread | main thread 結束時，沒有時限 | task queue | main thread 卡在 join | 沒了 |
| `DedicatedWorker thread` | 建它的 thread 的 `Workers` | `terminate_all`，先打斷 script | channel | main thread 卡在 join | 沒了 |
| `CanvasRaster`、`CanvasEncoder` | renderer 的 main thread | 各自 drop 時，沒有時限 | 工作的佇列 | main thread 卡在 join | 沒建 |
| `CompositorTileWorker*` | compositor 的 rayon pool | pool 的 drop 不等 | pool | 沒有影響 | 沒建 |
| `NetworkService` | `NetworkContext::start`，handle 丟掉 | 沒有人；最後一個 sender 放掉就自己結束 | tokio 的 recv | `exit` 時還在跑 | 沒了（沒有人保證） |
| `CookieStore` | `NetworkContext` 共用的 `CookieStoreThread` | 最後一個 handle 的 drop，沒有時限；那可能在 IO thread 上（`IoThreadHost` 也拿著一份） | channel、SQLite | 拿最後一份的 thread 卡住 | 沒了 |
| `LocalStorage` | `DomStorage` 共用的 `Shared` | 同上 | channel、SQLite | 同上 | 沒了 |
| `AutomationPipeReader` | `automation_pipe::spawn_reader`，handle 丟掉 | 沒有人 | `read(stdin)` | 沒有影響，跟著 process 死 | EOF 時沒了；`Browser.close` 時還在（照程式） |
| `AutomationPipeWriter` | `automation_pipe::run`、`App` | 結束的最後，沒有時限 | `write(stdout)` | S5 | 沒了 |
| `DevToolsHandlerThread` | `DevToolsHttpHandler` | 它的 `Drop`；自己等連線最多 5 秒 | tokio | main 卡在 join | 沒了 |
| `HeadlessViz` | `shell::headless` | 載入結束時，沒有時限 | Viz 的佇列 | 測試卡住 | |
| V8 的 worker | V8 的 platform（static） | 沒有人 | V8 的佇列 | 沒有影響 | **還在**（同一個 process 時） |
| GPU 驅動程式的（lavapipe 的 `llvmpipe-*`、Mesa 的 shader 和磁碟快取的；有些繼承了 `VizCompositorThread` 這個名字） | wgpu 的 instance（static） | 沒有人 | 驅動程式的佇列 | 忙著時 `exit` 會 crash（S6、D1） | **還在** |
| macOS 的 rendezvous server | `ipc` 的 static，handle 丟掉 | 沒有人 | Mach 的 receive | 沒有影響 | 沒有量 |

renderer process 裡：`Chrome_ChildIOThread` 由 `run_child_process` 最後停下、join；`CrRendererMain` 由它 join（沒有時限，所以卡住時只能靠 Browser kill，或通道斷了時 IO thread 的 `_exit`）；`exit` 時還在的只有 V8 的 worker。

machopodriver 裡：`WebDriverServer`（main 永遠 join 它）、`WebDriverDispatcher`、每個 session 的 `MachopoPipeReader`（兩個都不 join）。沒有正常結束的路：只有被訊號結束。

### 有狀態的全域變數

Rust 的 `static` 都不解構，所以要注意的是它們背後的 C++ 在 `exit` 時解構、而 thread 還在用。

| 全域 | 在哪裡 | 怎麼收 | 要先停的 |
| --- | --- | --- | --- |
| wgpu 的 instance | `viz` 的 `INSTANCE` | 不收；驅動程式的 thread 跟著留到 `exit` | Mesa、LLVM 的解構子跑的時候它們不能在忙 |
| 共用的 device | `viz` 的 `SHARED` | Viz 結束時 `release_shared`（先等 GPU） | Viz 要被 join；沒有 join 的路是 S6 |
| V8 的 platform | `bindings` 的 `PLATFORM` | 不收，也不 dispose V8 | 用 V8 的 thread；`exit` 時 worker 還在 |
| Browser 的 IO 迴圈 | `browser` 的 `io_thread()` | 不停 | 沒有定：它的 handler 拿著 `NetworkContext`、`DomStorage`、往 Viz 的 sender |
| trace | `bindings` 的 `RECORDING`、V8 的 tracing | `TraceEnd` 的 drop（`main` return 時） | 還在記 span 的 thread；S4、S8 走不到它 |
| macOS 的 rendezvous server | `ipc` 的 `SERVER` | 不停 | 沒有量 |
| Windows 的 job | `ipc` 的 `OF_THIS_PROCESS` | kernel 在 process 結束時關 | 不用 |
| 其他（編號、表、每條 thread 的快取） | | 沒有解構的需要 | 不用 |

### child process

| 誰 → 誰 | 怎麼請它結束 | 等多久 | 等不到 | 誰還拿著它的輸出 |
| --- | --- | --- | --- | --- |
| Browser → renderer | `RendererMsg::Shutdown` | 5 秒，一個一個 | kill、回收 | stdin、stdout、stderr 都是 Browser 的（S9）；trace 是它自己的檔案 |
| Browser 不在了 → renderer | 不用請：通道斷了 `_exit`（macOS、Linux），job（Windows） | | | 同上；trace 寫不完 |
| machopodriver → Browser | `Browser.close`、關 stdin | 5 秒 | kill、回收（它的 renderer 隨通道結束） | stdout 是 driver 的 pipe，stderr 是 driver 的 |
| machopodriver 不在了 → Browser | 不用請：stdin 到 EOF | 沒有時限（S1、S2） | 沒有人 | |

### 2026-11-10 的兩次沒有結束

兩次都是 `automation_pipe::the_window_has_bounds_to_set`（CI 的 42794260454、42779518184）：EOF 之後 30 秒還沒結束。前一天有一個專門抓堆疊的 CI（`debug-shutdown`，42617233565），它抓到的都是結束時 crash，沒有一次不結束，所以沒有堆疊。

- 同一段時間、同一組測試在結束時 crash，是 `29a94c7` 修的那一個；還原它就重現 crash。
- 還原它之後這個測試沒有重現「不結束」。
- **推論，沒有證實**：crash 之後 process 要等系統收完 core dump 才算結束，`child_process.rs` 的 `crash` 記著這可以超過半分鐘，正好超過測試的 30 秒。另一個可能是同一個 bug 在 C++ 的解構子裡卡住，而不是 crash。
- 那時還沒有 renderer process，S1 也會造成一樣的現象，但那個測試的頁面沒有卡住的 script。

### 對照 Chromium

152.0.7977.82 的原始碼。

| | Chromium | machopo |
| --- | --- | --- |
| renderer 正常結束 | `RenderThreadImpl::Shutdown`：不收尾，flush 之後直接 `_exit`。註解寫明以前的 graceful shutdown「浪費效能，而且是很多結束順序的 crash 的來源」 | 照順序拆掉、`exit` |
| Browser 結束 renderer | `FastShutdownIfPossible`：沒有 `beforeunload`、`unload`、`pagehide`、`visibilitychange` 的 handler、worker、keepalive 等理由時直接結束它，不等 | 一律請它結束、各等 5 秒 |
| 結束不了的 child | `base::EnsureProcessTerminated`：背景的 thread 等 2 秒再 SIGKILL，不擋呼叫的人 | UI thread 同步等 |
| single process | `FastShutdownIfPossible` 不做；`ShutDownInProcessRenderer` 在停其他 thread 之前 | 順序一樣，但卡住時沒有出路 |
| child 的通道斷了 | POSIX 上從 IO thread `TerminateCurrentProcessImmediately` | 一樣 |
| 訊號 | `chrome_browser_main_posix.cc`：SIGINT、SIGHUP 走正常結束，SIGTERM 走 `SessionEnding` | 沒有處理 |
| thread 的順序 | `BrowserMainLoop::ShutdownThreadsAndCleanUp`：renderer（同一個 process 時）、network service、child 的 host、compositor 和 GPU 的通道、IPC、IO thread、tracing、thread pool，固定的順序 | renderer、Viz、pipe 和 server；network、storage 看最後一份 handle 在哪裡；IO thread 不停 |
| DevTools pipe 的 thread | `PipeIOBase::Shutdown`：先關 pipe，join 丟給 `CONTINUE_ON_SHUTDOWN` 的背景工作，不擋結束 | main join writer，沒有時限 |
| 啟動到一半失敗（2026-11-24 對的，`main`） | `BrowserMainRunnerImpl::Initialize` 從哪裡提早 return 都一樣：runner 的解構子 `if (initialization_started_ && !is_shutdown_) { Shutdown(); }`，照樣 `PreShutdown`、`ShutdownThreadsAndCleanUp` | 提早 return 時沒有 shutdown（S6）；第 2 段照 Chromium，`BrowserWindow` 的 `Drop` |
| remote debugging 的 port 綁不上（同上） | 不算啟動失敗：`StartServerOnHandlerThread` 記一行 `Cannot start http server for devtools.`，照樣啟動，`DevToolsHttpHandler` 不 listen | 啟動失敗、結束（第 3 段定要不要照 Chrome） |

GPU process 的結束沒有對照：machopo 沒有 GPU process，抓原始碼時那個檔案的路徑也不對，沒有再找。

## `shell` 的測試的審查

### 量法

整組測試的執行檔，`taskset` 成 4 顆 CPU（兩個核心、四個 thread，像 CI）、lavapipe，五份同時跑、各用自己的暫存目錄，其中一份加兩個忙碌的 process、一份用 10 條測試 thread；壞過的再一個一個單獨重複。查原因時在另一份複本裡加追蹤和放大的辦法（每個 task 之後睡一下、parser 的預算可調），不動這個 branch 的程式。**本機照這些條件就重現得了 CI 的主犯，不用容器。**

### 模組的分類

| 怎麼跑 | 模組 | 一份的東西 | 怎麼等、時限 |
| --- | --- | --- | --- |
| 啟動 `machopo`，websocket | `devtools`、`devtools_cookies`、`devtools_css`、`devtools_dom`、`devtools_initiators`、`devtools_lifecycle`、`devtools_log`、`devtools_network`、`devtools_overlay`、`remote_debugging` | port（系統挑的；「拒絕連線」的用 `unused_port`）、CPU、GPU adapter、stderr（讀 listening 那一行） | 等事件，每次讀 30 秒；`devtools`、`devtools_css`、`devtools_lifecycle` 另有固定的 sleep |
| 啟動 `machopo`，pipe | `automation_pipe`、`images`（一部分）、`trace` | port、CPU、GPU adapter、`CARGO_TARGET_TMPDIR` 裡固定名稱的資料夾 | 等回答和事件 30 秒；輪詢 10、20 秒；幾處固定的 sleep |
| 測試的 process 裡的 Browser 那一側，啟動 renderer process | `renderer_process` | CPU、process | 等事件，輪詢 |
| 測試的 process 裡的 driver，啟動 `machopo` | `webdriver` | port、CPU、GPU adapter | 指令自己的逾時，最長 60 秒 |
| 測試的 process 裡，有視窗那一層 | `browser_view`、`windowless` | V8 的 platform、GPU device、`temp_dir`；`browser_view` 加 port | 等事件和 frame，30 秒 |
| 測試的 process 裡，headless，`file:` | `css_animations`、`file_reader`、`focus`、`forms`、`history`、`intersection_observers`、`keys`、`limits`、`mouse`、`resize_observers`、`scheduling`、`script`、`scroll_anchoring`、`selection_painting`、`selections`、`sites`、`transitions`、`web_animations`、`wheel` | V8 的 platform、GPU device、`CARGO_TARGET_TMPDIR` 裡固定名稱的資料夾（同一個 checkout 同時跑兩份會互相改） | 頁面回報，30 秒；或畫完、`reftest-wait`（600 個 frame）。頁面裡用 `requestAnimationFrame` 數 frame、用 `setTimeout` 等固定的時間 |
| 測試的 process 裡，headless，有 server | `canvas`、`documents`、`fetch`、`geometry`、`images`（其餘）、`inserted_scripts`、`lazy_images`、`messaging`、`modules`、`navigation_timing`、`navigator`、`network`、`performance`、`style_sheets`、`wasm_documents`、`workers` | 同上，加 port；`network`、`modules` 加 `temp_dir` | 同上；`network`、`modules`、`workers` 靠 server 延遲 200 到 600 ms 排先後 |

沒有模組用顯示器（`automation_pipe` 的 `--window` 那一個只在 `MACHOPO_WINDOW_TESTS` 時）。libtest 沒有一個測試的時限：測試的 process 裡的 renderer 卡住時，整組測試停在那裡（S1）。

### 壞過的測試和原因

「確定」是做成每次都壞、或改掉那個原因就不再壞；「對得上」是重現了、現場和程式一致，沒有反過來驗；「推論」是只從失敗的內容和程式推的。

**A. 產品的競爭。** 三種原因，都在機器忙、好幾個 task 同一輪被取出時露出來：

- **該同步的延後了**（A2、A3、A4、A5）：Blink 在呼叫的當下做的事，machopo 排進 `FrameRequest`。A4 不用忙也錯：設 `location.hash` 之後同一個 task 讀 `scrollY` 就是舊的。
- **讀的是延後更新的副本**（A1）：DOM agent 的 frame 清單等 `tell_frames` 才更新。child 的 commit 不是 `FrameRequest`，每個 task 之後只清空 `FrameRequest` 修不掉它。
- **該是 task 的不在 task queue 裡**（A6）：留在 `RenderThread::run` 的外圈「佇列空了才做」。

| | 測試 | 現象 | 原因 | 把握 | 怎麼修 |
| --- | --- | --- | --- | --- | --- |
| A1 | `devtools_css::sheets_coming_and_going_are_told` | 等不到 iframe 的 `styleSheetAdded` | child 的 commit 和 `BeginMainFrame` 同一輪：`sheets_updated` 照 DOM agent 舊的 frame 清單列 sheet，卻記下了新的版本，之後版本不變就再也不送。使用者的 DevTools 也會漏 | 確定 | 列之前先更新清單 |
| A2 | `devtools_network::redirects_and_failures_are_told` | redirect 的第二個 `requestWillBeSent` 沒有 | `fetch()` 在呼叫的當下送出，`FetchSent` 晚一輪才讓 Network agent 記下；redirect 先到，`did_redirect` 不認得它 | 確定 | 處理 redirect 之前先記下 |
| A3 | `devtools_network::requests_are_told_with_their_responses_and_bodies` | `loadingFinished` 的時間早於 `requestWillBeSent` | 同 A2：時間是記下時取的，不是送出時 | 確定 | 送出時取時間，帶在 `FetchSent` 裡 |
| A4 | `documents::history_through_the_browser`（`history::push_state_does_not_scroll` 放大時一樣） | 兩個 frame 之後還沒捲到 fragment，之後又蓋掉 `scrollTo` | `location.hash` 的捲動是 `FrameRequest`，兩個 `BeginMainFrame` 在它前面。網頁看得到 | 確定 | 呼叫的當下捲（frame 之前先處理只修掉這個測試的樣子） |
| A5 | `automation_pipe::frames_are_switched_to_and_kept_through_their_navigations` | 拿掉的 iframe 是 `javascript error`，不是 `no such window` | 下一個指令在 frame 的移除被處理之前執行 | 確定 | 每個 task 之後處理 |
| A6 | `intersection_observers::what_a_change_brings` | 第一次的通知排在「捲了」之後 | 通知的 task 在外圈（`run_intersection_task`），兩個 frame 先跑了 | 推論（放大時現場一樣；反過來驗時被放大器本身拖到逾時） | 通知排成真的 task |

**B. 測試等錯了東西。**

| | 測試 | 現象 | 原因 | 把握 | 怎麼修 |
| --- | --- | --- | --- | --- | --- |
| B1 | `network::a_document_is_drawn_once_it_has_a_body` | 畫了 2 次，要 1 次 | body 有了畫一次、`load` 之後再一次，兩個都在 body 之後。測試拿次數代替「body 之前沒畫」 | 對得上（追蹤看到先後） | 斷言沒有畫過紅的 frame |
| B2 | `canvas::a_draw_asks_for_a_frame` | 4 個 frame，要 3 | 同 B1：`load` 之前多一個 frame | 對得上（追蹤看到先後） | 斷言畫了之後多一個 frame，不數全部 |
| B3 | `documents::idle_callbacks` | `timeRemaining()` 是 0 | callback 開始時期限已經過了（被排程搶走 CPU），0 是合法的值 | 對得上（印出值和時間） | 接受 0，或重試到有剩 |
| B4 | `automation_pipe::the_profiler_samples_the_pages_script` | 200 ms 的樣本不到 100 | 樣本數量的是拿到多少 CPU | 確定（CPU 被搶時每次都壞） | 比兩個間隔的比例，或照實際的時間算 |
| B5 | `automation_pipe::the_profile_tells_waiting_from_rendering` | idle 的樣本不到 200 | 同 B4，加上 200 ms 的固定 sleep | 對得上 | 同 B4；先等真的閒下來 |
| B6 | `documents::about_blank_has_its_initiators_origin` | 等 30 秒沒有回報 | parser 照時間（10 ms）讓出時，iframe 在設 `onload` 的 script 之前就載入完 | 對得上（預算很大時不再壞） | script 先於 iframe，或不靠第一次 `load` |
| B7 | `web_animations::a_replaced_animation_is_removed` | `remove` 在第二個 `finish` 之前 | 事件的排程時間剛好相同時照 composite order 排，a 的兩個事件排在一起 | 對得上 | 先查 Chrome 在同時時的順序，再決定改測試還是 `notify` 的時間 |
| B8 | `transitions::a_frames_events_go_by_when` | `start b` 在 `run a` 之前 | 同 B7：pending 在第一次 tick 之前解決時，`run` 和 `start` 的時間相同 | 推論（單獨跑沒有重現） | 同 B7 |

**C. 搶資源。**

| | 測試 | 現象 | 原因 | 把握 | 怎麼修 |
| --- | --- | --- | --- | --- | --- |
| C1 | `devtools::subresources_are_listed_with_their_content` | 該被拒絕的請求沒有失敗 | `net::test_server::unused_port` 放掉 port 才回傳，同時跑的另一個測試的 server 拿到同一個 | 對得上（C2 是它的另一半） | 整個測試期間占著 port |
| C2 | `network::an_image_set_shows_and_loads_the_image_for_the_scale` | 自己的 server 收到沒要過的 `/gone.png` | 就是 C1 的那個請求 | 對得上 | 同上；`unused_port` 的另外五個呼叫也一樣 |

**D. 已經修掉的。**

| | 測試 | 現象 | 原因 | 把握 |
| --- | --- | --- | --- | --- |
| D1 | `automation_pipe::the_system_info_tells_the_gpu_adapter` 和同時期其他結束時失敗的 | `browser.end().success()` 不成立 | `29a94c7` 修的：device 在全域變數裡，`exit` 時 lavapipe 在忙 | 確定（還原它就重現） |

放大器掃整組測試時另外有一批只在放大時壞的（`inserted_scripts`、`modules`、`scheduling` 等）：多半是數到 600 個 frame 或等的時間被放大器吃掉，沒有在正常條件下壞過，不列進來。

## 參考資料

- Chromium 152.0.7977.82：`content/browser/browser_main_loop.cc`（結束的順序）、`content/child/child_thread_impl.cc`（child 在通道出錯時）、`content/renderer/render_thread_impl.cc`（renderer 的結束）、`content/browser/renderer_host/render_process_host_impl.cc`（fast shutdown）、`base/process/kill_posix.cc`（等多久才 kill）、`chrome/browser/chrome_browser_main_posix.cc`（訊號）、`content/browser/devtools/devtools_pipe_handler.cc`（pipe 的 thread）、`third_party/blink/renderer/core/inspector/inspector_css_agent.cc`（sheet 直接從 document 列）、`third_party/blink/renderer/core/loader/resource_load_observer_for_frame.cc`（送出的當下告訴 DevTools）。
