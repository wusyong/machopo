# RFC 0054：Speedometer 的 Chrome 那一側經過 CDP，Windows 上也能量

- 開始日期：2026-10-18
- 狀態：🚧 已排程：2026-10-18（CDP 走 pipe、從 `about:blank` 開始用 `Tracing.start` 錄、一律用 `Browser.close` 關、Windows 上用 `icacls` 給 Chrome 的資料夾加權限；Linux、macOS 的 fd 3、4 和 trace 的緩衝區在實作時定）
- 細節：`details.md`（Windows 上的重現和實驗、pipe 的試驗、`Tracing` 的流程）

## 摘要

`bench.py speedometer` 的 Chrome 那一側現在在 Windows 上不能用：Chrome 的 sandbox 讀不到放在 `target/` 底下的 `chrome.exe`，頁面一個請求都發不出去；就算跑得起來，`--trace` 也拿不到 Chrome 的 trace，因為工具是用訊號請 Chrome 結束的，Windows 上沒有這種訊號，只能直接殺掉。這份 RFC 讓工具經過 CDP 控制 Chrome：走 Chrome 的 pipe（和 machopo 的 `--automation-pipe` 同一種格式，收發訊息的程式兩邊共用），開頁面、錄 trace（`Tracing.start` / `Tracing.end`）、關掉 Chrome（`Browser.close`）都經過它，不再靠訊號；Windows 上下載 Chrome 之後給它的資料夾加上 AppContainer 的讀取權限，和裝在 `Program Files` 底下的 Chrome 一樣在 sandbox 裡跑。

## 動機

**Windows 上的 Chrome 量不到**（`docs/limitations.md` 的「平台和建置」第一條，「`bench.py speedometer` 的 Chrome 那一側……Windows 上不跑」）：

- **頁面一個請求都沒有**：Chrome 的 sandbox 以 AppContainer 跑 network service 這類子程序，它們要讀得到 `chrome.exe` 和旁邊的 DLL。`target/` 在使用者目錄底下，預設的 ACL 沒有 `ALL APPLICATION PACKAGES`、`ALL RESTRICTED APPLICATION PACKAGES`，log 的第一行是 `Sandbox cannot access executable`，network service 起不來。2026-10-18 重現，查到這就是原因（`details.md` 的「Windows 上的重現」）。
- **`--trace` 拿不到 Chrome 的 trace**：現在用 `--trace-startup-file` 錄，Chrome 要正常結束才寫得出 trace（收到結束的要求後 5 到 15 秒）。工具在 Linux、macOS 上送 SIGINT 請它結束（Linux 上的 headless Chrome 收到 SIGTERM 不寫 trace），Windows 上沒有能送給一般 process 的「請你結束」的訊號，Python 的 `terminate()` 是 `TerminateProcess()`，等於直接殺掉。

結果是 Windows 的開發機上只能量 machopo 那一側，要和 Chrome 比就得換到 macOS 或 Linux 的機器。

**目標**：
- Windows 上 `bench.py speedometer` 的盤點、量測和 `--trace` 的 Chrome 那一側都能跑，和 macOS、Linux 一樣。
- 三個平台用同一種方式控制 Chrome（CDP），工具裡不再有「SIGINT 才寫 trace」這類各平台的特例。
- Chrome 照常在 sandbox 裡跑，和一般使用者的 Chrome 一樣。

**不在範圍內**：
- `--sample` 在 Windows、macOS 上的取樣（`perf` 只在 Linux；Windows 要 ETW）：另一件事，現在的限制照舊。
- 量測本身：照 `docs/benchmarks.md` 的「效能的量測」，平常不量；這份 RFC 的驗收只跑工具自己的測試和一個 suite 的 `--trace`（「測試」）。
- 換 Chrome for Testing 的版本。

## 設計概要

對使用 `bench.py speedometer` 的人：

- Windows 上不再需要 `--browser machopo` 或 `--reuse-chrome` 才跑得起來；`bench.py speedometer`、`--inventory`、`--trace` 在三個平台上一樣用。
- 第一次下載 Chrome for Testing 時（Windows），工具在它的資料夾上加 AppContainer 的讀取權限；已經下載過的，下一次跑的時候補上。只動 `target/chrome-for-testing/<版本>/` 這個資料夾，不動系統的設定，不需要系統管理員。
- `--trace` 的 Chrome 那一份從頁面開始載入前錄到 suite 結束（不再包括 Chrome 自己啟動的那一段，那一段本來就不計時），一樣是 Perfetto 的 protobuf，用法不變。Chrome 的 trace 的緩衝區滿了、丟了事件時，工具會說出來。
- Chrome 每次都正常結束（exit 0），不是被殺掉的。

對改這支工具的人：machopo 和 Chrome 都經過同一個 CDP 的連線類別控制，差別只在怎麼啟動 process；`end()` 只剩「結束不了才殺掉」。

## 詳細設計

改到的檔案：`scripts/bench/speedometer/speedometer.py`、`scripts/bench/speedometer/speedometer_test.py`、`docs/testing.md`（「效能的量測」）、`docs/limitations.md`（「平台和建置」）。

### CDP 的連線：pipe，和 machopo 共用

`MachopoPipe` 現在就是用「JSON 加一個 NUL」收發 CDP 的訊息，這正是 Chrome 的 `--remote-debugging-pipe` 的格式（machopodriver 照它做）。把收發訊息的部分（讀的 thread、`call`、等回應、事件）抽成一個類別，例如 `CdpPipe`，machopo 和 Chrome 各自只負責啟動 process 和接上 pipe：

- **machopo**：照舊用 `--automation-pipe` 的 stdin 和 stdout，送的指令也和現在一樣。共用的只是 Python 這一側收發訊息的程式：machopo 不用多支援任何 CDP 的指令，Rust 的程式不改。`Tracing`、`IO` 只送給 Chrome；machopo 的 trace 照舊用 `MACHOPO_TRACE`。
- **Chrome，Windows**：`--remote-debugging-pipe --remote-debugging-io-pipes=<讀>,<寫>`，兩個值是 Chrome 繼承到的 pipe 的 handle（Python 用 `os.pipe()`、`msvcrt.get_osfhandle`、`os.set_handle_inheritable`，`STARTUPINFO.lpAttributeList` 的 `handle_list` 只讓 Chrome 繼承這兩個）。兩個開關要一起給，只給 `--remote-debugging-io-pipes` 時 Chrome 不理會（2026-10-18 實測，`details.md` 的「pipe 的試驗」）。
- **Chrome，Linux、macOS**：`--remote-debugging-pipe`，Chrome 固定從 fd 3 讀、往 fd 4 寫。Python 的 `pass_fds` 不換 fd 的號碼，要在子 process 裡把 pipe 換到 3 和 4（「待決定」第 1 條）。

除了收發訊息，CDP 的事件（`Tracing.tracingComplete`）也要能等：現在的 `call` 只挑出自己的回應、把其他訊息丟掉，改成事件另外排在一個佇列裡。

### 開頁面和關掉

- Chrome 以 `about:blank` 啟動，視窗大小照舊用 `--window-size`。
- 開頁面照 `MachopoPipe.open` 的步驟：`Target.getTargets` 找到那個分頁，`Target.attachToTarget`（`flatten`），`Page.navigate`；Chrome 再多兩步：導覽前 `Page.bringToFront`，之後 `Target.detachFromTarget`：頁面有 focus、量測期間頁面上沒有接著的 CDP session，和用命令列開網址時一樣。machopo 沒有這兩個指令，也不需要：它的頁面本來就有 focus，session 不在頁面上做任何事。
- **`Page.bringToFront` 是後來加的**（2026-10-18）：第一次照新的做法量 20 個 suite 時，Editor-TipTap 變成無效：machopo 有一個元素帶 `ProseMirror-focused`，Chrome 沒有。用命令列開網址時 Chrome 的頁面有 focus，headless 的第一個分頁（`about:blank`）卻要叫它到前面才有；只導覽、先 `Target.activateTarget` 都沒有 focus，`Page.bringToFront` 或 `Target.createTarget` 開新的分頁才有（`details.md` 的「頁面的 focus」）。選 `Page.bringToFront`：用的還是同一個分頁，Puppeteer 的 `page.bringToFront()` 也是它。
- 結束時一律 `Browser.close`，等 Chrome 自己結束；等不到才殺掉（`end()`）。machopo 那一側本來就這樣關。`end()` 的 `interrupt`（SIGINT）和 `CHROME_TRACE_GRACE` 拿掉。

### trace：`Tracing.start` / `Tracing.end`

`--trace` 時：

1. 連上之後、開頁面之前，對 browser 送 `Tracing.start`：`traceConfig` 的分類和現在的 `CHROME_TRACE_CATEGORIES` 一樣（`includedCategories`，其他的排除），`transferMode: "ReturnAsStream"`、`streamFormat: "proto"`。browser 層級的 tracing 包括所有的 process（renderer、GPU），和現在的 startup trace 一樣。
2. 頁面 `POST` 回報告之後送 `Tracing.end`，等 `Tracing.tracingComplete`：它帶著一個 stream 的 handle 和 `dataLossOccurred`。
3. `IO.read` 讀到 `eof`（`base64Encoded` 時先解碼）寫進 `<suite>-chrome.pftrace`，再 `IO.close`。`dataLossOccurred` 是 true 時印出來，那份 trace 照樣留著。
4. 最後照常 `Browser.close`。

輸出照舊是 Perfetto 的 protobuf，`perfetto_slices`、`timed_windows` 和 trace processor 的分析都不用改。拿掉 `--trace-startup`、`--trace-startup-file`、`--trace-startup-duration`。

### Windows：Chrome 的資料夾的權限

`chrome_binary()` 在 Windows 上確認 `CHROME_DIR`（`target/chrome-for-testing/<版本>/`）有兩條繼承的 ACE，沒有就補上：

```
icacls <CHROME_DIR> /grant *S-1-15-2-1:(OI)(CI)(RX) *S-1-15-2-2:(OI)(CI)(RX)
```

- `S-1-15-2-1` 是 `ALL APPLICATION PACKAGES`、`S-1-15-2-2` 是 `ALL RESTRICTED APPLICATION PACKAGES`。用 SID 寫，不受系統語言影響（中文的 Windows 上 `icacls` 印出的名稱是中文）。
- 裝在 `C:\Program Files` 底下的 Chrome 本來就有這兩條：那個資料夾預設的 ACL 就給它們讀取和執行（2026-10-18 看過）。
- 每次啟動 Chrome 前都確認、不只在下載時：已經下載過的 Chrome 也會被補上。確認是用 `ctypes` 讀資料夾的 DACL（`GetNamedSecurityInfoW`，轉成 SDDL），找這兩個 SID 有沒有往下繼承（`OI`、`CI`）的讀取和執行（`0x1200a9`）；`S-1-15-2-1` 在 SDDL 裡寫成別名 `AC`。已經有就不跑 `icacls`：每次都 `/grant` 會重寫資料夾裡每個檔案的 ACL。

### 測試

`scripts/bench/speedometer/speedometer_test.py`（Chrome 的測試在還沒下載 Chrome 時 skip，和現在一樣）：

- **Chrome 跑得起來**：現有的 `ChromeTests`（量一個 suite、盤點的紀錄、shadow root 的摘要……）改走 CDP 之後照樣通過；Windows 上它們現在都等不到報告，所以拿掉權限的處理時這些測試會失敗。
- **權限**（Windows）：在一個暫時的資料夾上跑補權限的函式，讀回它的 ACL，兩個 SID 都在、而且是繼承的；跑兩次結果一樣。
- **Chrome 自己結束**：`Browser.close` 之後 Chrome 的 exit code 是 0（被殺掉時不是 0）。
- **trace**：`MeasureTests.test_traces_of_both` 現在在 Windows 上因為拿不到 Chrome 的 trace 而失敗；改了之後兩份都在，Chrome 的那份有 `CrRendererMain`、runner 計時的區間（和 machopo 那份同樣的檢查），沒有丟資料。
- **收發訊息的類別**：machopo 那一側的現有測試（`MachopoTests`、`MeasureTests`）守著抽出來之後的行為；另外用一個假的 process（照 NUL 的格式回應）測事件和回應交錯時，`call` 拿到自己的回應、事件留在佇列裡。
- **mutation test**：拿掉補權限、拿掉 `Browser.close`（改成直接殺掉）、不讀 `Tracing.end` 的 stream、`call` 把事件丟掉，各自要有測試失敗。

在其他平台上：Linux 和 macOS 的 pipe（fd 3、4）由同樣的 `ChromeTests` 守著，要在那兩個平台上各跑一次（「分階段」第 3 段）。

### 分階段

1. **CDP 和權限**（Windows）：抽出收發訊息的類別；Chrome 走 pipe、從 `about:blank` 開頁面、`Browser.close` 關；Windows 上補權限。
2. **trace 改用 `Tracing`**（Windows）：`Tracing.start` / `Tracing.end` / `IO.read`，拿掉 startup trace 和 SIGINT 的特例。做完時更新 `docs/testing.md`、把 `docs/limitations.md` 那一條換成「pipe 只在 Windows 上跑過」。
3. **Linux 和 macOS 上確認**：在那兩台機器上跑 `speedometer_test.py` 的 Chrome 和 trace 的測試、一個 suite 的 `--trace`；fd 3、4 的做法（「待決定」第 1 條）在這段確認。

第 1、2 段一起做（2026-10-18）：原本打算第 1 段先保留 startup trace，但 Chrome 經過 `Browser.close` 正常結束時，`--trace-startup-file` 可能寫成 JSON（原本的工具註明「只有 Chrome 自己結束時才寫 JSON」，它一直是被訊號結束的），中間狀態的 `--trace` 會壞掉，所以兩段一起換。第 3 段要另外的機器。CI 不跑 `speedometer_test.py`，Linux、macOS 上壞了只有在那兩台機器上跑才看得出來，所以第 3 段做完才算完成這份 RFC。

## 缺點

- **工具和 CDP 綁得更緊**：開頁面、關掉、錄 trace 都經過 CDP。Chrome 的 CDP 很穩定（Puppeteer、Playwright、Telemetry 都靠它），而工具本來就經過 CDP 控制 machopo。
- **Linux、macOS 的啟動要換 fd 的號碼**：多一段平台的程式（「待決定」第 1 條），而且這台 Windows 的開發機試不到。
- **trace 少了 Chrome 啟動的那一段**：本來就不在計時的區間裡，分析不受影響；但要看 Chrome 自己啟動的成本時，startup trace 才有。
- **改了 Chrome 在 Windows 上的資料夾的 ACL**：只在 `target/` 底下、只多兩條讀取的 ACE，和 `Program Files` 一樣；`cargo clean` 刪掉整個 `target/` 時跟著消失，下一次下載時再補。
- **和 RFC 0053 改同一個檔案**：0053 的第 1 段在 `scripts/bench/speedometer/speedometer.py` 加 `--against` 的計時和記憶體的峰值，這份改 Chrome 的啟動和 trace，地方不同；先做完的先 merge，另一份 rebase。

## 理由和其他做法

**CDP 的連線**（「已決定的事項」第 2 條）：

| | 做法 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **P1** | pipe（Windows 的 `--remote-debugging-io-pipes`，其他平台 fd 3、4） | 和 machopo 的 `--automation-pipe` 同一種格式，收發訊息的程式共用；不開 port；不用 websocket | Linux、macOS 要換 fd 的號碼 |
| P2 | `--remote-debugging-port=0`，讀 profile 裡的 `DevToolsActivePort`，連 websocket | 三個平台一樣 | Python 的標準庫沒有 websocket，要自己寫（約 80 行）或加依賴；要等檔案出現；本機開一個 port |
| P3 | chromedriver（Chrome for Testing 有同版本的），WebDriver 的 HTTP | 只用 urllib | 多下載、多管一個 process；錄 trace 還是要 CDP（`goog:chromeOptions` 的 `perfLoggingPrefs` 是 JSON 的事件，不是 Perfetto） |
| P4 | 不用 CDP，Windows 上對 Chrome 送 `WM_CLOSE` 或 console 的 Ctrl 事件 | 改動最小 | headless 的 Chrome 沒有頂層視窗、也沒有 console，請它結束的路走不通；`taskkill` 不加 `/F` 會說只能強制結束 |

原本選 P2（2026-10-18），因為以為 Windows 上的 Python 用不了 Chrome 的 pipe（要 fd 3、4，Windows 上沒有 `pass_fds`）；查 Chrome 152 有 Windows 專用的 `--remote-debugging-io-pipes`，試過能用，改選 P1。

**trace 的錄法**（第 3 條）：

- **T1：`Tracing.start` / `Tracing.end`（採用）**：Chromium 自己的工具（Telemetry、Lighthouse、Puppeteer 的 `page.tracing`）都這樣錄；不靠 Chrome 怎麼結束；`dataLossOccurred` 告訴我們有沒有丟資料。
- T2：保留 `--trace-startup-file`，經過 CDP 送 `Browser.close` 讓 Chrome 正常結束、寫出 trace：改動最小，trace 的範圍不變。不選：照樣要等 Chrome 結束才寫（5 到 15 秒），丟沒丟資料看不出來；而 CDP 都接上了，T1 多的只有幾個指令。
- T3：只錄 runner 計時的區間：頁面要另外通知工具開始和結束，machopo 那一份是整個 process 都錄，兩邊會不對稱。不選；計時的區間由分析的時候找（`timed_windows`）。

**sandbox**（第 4 條）：

- **S1：給 Chrome 的資料夾加 AppContainer 的讀取權限（採用）**：Chrome 照常在 sandbox 裡跑，和 `Program Files` 底下的 Chrome 一樣。
- S2：`--no-sandbox`：一行就好。不選：renderer、network service 都不在 sandbox 裡，和使用者的 Chrome 不同，量到的不是真的 Chrome。
- S3：把 Chrome 放到 `target/` 以外、ACL 本來就對的地方（例如 `%ProgramFiles%`）：要系統管理員，而且不在 repo 裡，`cargo clean` 管不到。不選。

## 先例

- **Playwright**：預設用 `--remote-debugging-pipe` 控制 Chromium，pipe 經過 Node 的 stdio 放在 fd 3、4。
- **Puppeteer**：預設用 websocket（`--remote-debugging-port`），`pipe: true` 時用 fd 3、4；錄 trace 用 `Tracing.start`、`ReturnAsStream`。
- **Chromium 的 Telemetry**：經過 DevTools 的 websocket 控制瀏覽器，trace 用 `Tracing.start` 的 `ReturnAsStream` 拿回來。
- **machopodriver**：machopo 的 `--automation-pipe` 照 Chrome 的 `--remote-debugging-pipe` 的格式，所以這份 RFC 才能讓兩邊共用收發訊息的程式。

## 已決定的事項

2026-10-18 接受。

1. **寫一份小 RFC**（2026-10-18，使用者）：RFC 0012 已經完成，不回頭改它；這份改的是工具的設計（trace 的範圍、Chrome 怎麼關），選擇要記下來。
2. **CDP 走 pipe（P1），收發訊息的程式和 machopo 共用**（2026-10-18，使用者）：原本選 P2（port 加自己寫的 websocket），實測 Windows 上有 `--remote-debugging-io-pipes` 之後改成 P1（「理由和其他做法」）。
3. **trace 用 `Tracing.start` / `Tracing.end`，從 `about:blank` 開始錄（T1）**（2026-10-18，使用者）。
4. **一律用 `Browser.close` 關 Chrome**，有沒有 `--trace` 都一樣；拿掉 SIGINT 的特例（2026-10-18，使用者）。
5. **Windows 上用 `icacls` 給 Chrome 的資料夾加 AppContainer 的讀取權限（S1）**，不用 `--no-sandbox`（2026-10-18，使用者）。
6. **`Tracing.start` 用預設的緩衝區**（2026-10-18，第 2 段）：startup trace 和 CDP 的 tracing 預設的緩衝區大小可能不同，所以錄了兩個重的 suite，各 10 個 iteration（TodoMVC-jQuery 的 trace 16 MB、React-Stockcharts-SVG 7.7 MB），`dataLossOccurred` 都是 false，不設 `traceBufferSizeInKb`。之後丟了資料，工具會印出來（`save_trace` 的回傳值），到時再調大。

2026-10-20，Editor-TipTap 又無效之後（`progress.md` 的「做過的檢查」）：

7. **在之後的 frame 才設的 class 不比**（使用者，甲）：摘要比對時不比 `TIMING_CLASSES`（現在只有 ProseMirror 的 `ProseMirror-focused`）：TipTap 的 `focus()` 在 `requestAnimationFrame` 裡才 focus，Chrome 有時在 suite 那個 frame 的 callback 跑之前就結束一個 test，class 跟著下一個 test 的摘要出現，整個 iteration 做的事一樣。一邊整次 run 都沒有、另一邊有時，不判無效，在 `measure.md` 註記（那一邊確實少做了：Chrome 偶爾整次都沒有 focus）。不選等 suite 的下一個 frame 再取摘要：`didRunTest` 之後下一個 test 馬上開始，等了就混進下一個 test 的改動，或改變 runner 的時序。不選留著 CDP session 開 `Emulation.setFocusEmulationEnabled`：試過，斷開 session 就失效，留著也照樣六次裡兩次無效（問題不在視窗的 focus），而且違背第 1 段「量測期間頁面上沒有 session」。

## 待決定

1. **Linux、macOS 上怎麼把 pipe 放到 fd 3、4**：Python 的 `subprocess` 不能換 fd 的號碼。選項：
   - `preexec_fn` 裡 `os.dup2`：最直接；但 Python 的文件說有其他 thread 時 `preexec_fn` 不安全（工具的 HTTP server 是另一個 thread）。
   - 經過 `sh` 換：`sh -c 'exec "$0" "$@" 3<&N 4>&M' chrome …`，`pass_fds` 傳 N、M：不用 `preexec_fn`，多一層 `sh`（`exec` 之後就是 Chrome 本身）。
   - `os.posix_spawn` 的 `file_actions`（`POSIX_SPAWN_DUP2`）：thread 安全；但拿不到 `Popen`，要自己處理 stdout、stderr 和等它結束。

   2026-10-18 的看法：`sh` 的 `exec`。第 3 段在那兩個平台上試過再定。

## 之後的可能

- **CDP 打開了更多量法**：例如用 `Performance.getMetrics` 或 `Memory` 拿 Chrome 那一側的記憶體，和 machopo 的記憶體峰值比。
- **Windows 上的取樣**：ETW（WPR 或 samply），讓 `--sample` 不只在 Linux 上。
