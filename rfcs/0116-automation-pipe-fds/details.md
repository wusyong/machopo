# RFC 0116 的細節

## 盤點

2026-11-26，`main` 的 `52dc5adc`。

**machopo 這一側**：

- `crates/shell/src/automation_pipe.rs`：`spawn_reader` 用 `std::io::stdin().lock()`，一則訊息讀到 NUL，上限 `MAX_MESSAGE_SIZE`（64 MiB）；`spawn_writer` 用 `std::io::stdout().lock()`，每則加 NUL、`flush`。讀到結尾時 `ended`，`run` 結束。
- `crates/shell/src/window.rs`（`--automation-pipe --window`）：同樣的 `spawn_reader`、`spawn_writer`。
- `crates/shell/src/lib.rs` 的 `renderer_launcher`：`.stdin(Stdio::null()).stdout(std::io::stderr())`。註解寫著理由（pipe 只在 Browser 手上，client 在 Browser 結束時讀到 EOF）和 Chromium 的不同（它的子 process 保留 stdout）。這是 RFC 0109 的第 16 條。
- `crates/shell/src/main.rs` 的 `log_layer`：`with_writer(std::io::stderr)`，註解「Standard output is `--automation-pipe`'s」。
- `crates/renderer/src/render_thread.rs` 的 `print_console`：`console`、沒接住的例外，一行一次寫 stderr，註解「Never stdout」。
- `crates/dom_wasm/src/wasi.rs`、`engine.rs`：guest 的 stdout、stderr 經過 `OutputSink` 一行一行收進緩衝區，`flush_output` 把每一行當成 console 的訊息交出去（不分 stdout、stderr），和 `console.log` 一樣最後寫 stderr。
- `crates/ipc/src/platform/linux/channel.rs` 的 `spawn`：renderer 的 channel 用環境變數 `MACHOPO_IPC_FD` 傳號碼，`pre_exec` 裡拿掉 close-on-exec；號碼小於 3 時先搬到 3 以上（`FIRST_FREE_FD`）。`inherited()` 拿了之後設回 close-on-exec，用 `AtomicBool` 確保只拿一次。macOS 用 Mach 的 rendezvous，Windows 用可繼承的 handle 和一把鎖（`windows/channel.rs` 的 `spawn`）。fd 3、4 的拿法照同一套模式寫。

**啟動 `--automation-pipe` 的地方**（都接 stdin、stdout）：

| 地方 | 用途 |
| --- | --- |
| `crates/driver/src/machopo.rs` 的 `Machopo::start` | machopodriver（WPT、wdspec 經過它） |
| `crates/shell/tests/it/automation_pipe.rs` 的 `Browser::start_in`，和同一個檔案後面直接啟動 machopo 的幾個測試 | `shell` 的整合測試，其他模組（`trace`、`devtools_*`）也用 `Browser` |
| `crates/shell/tests/it/renderer_process.rs` | renderer process 的測試 |
| `crates/shell/tests/it/remote_debugging.rs` | pipe 加上 `--remote-debugging-port` |
| `crates/shell/tests/scenarios.rs` | `--window` 的情境 |
| `scripts/bench/common.py` 的 `MachopoPipe` | Speedometer、blink-perf、census 的 machopo 那一側 |
| `scripts/bench/raster/raster.py` | raster 的量測 |

`tests/wpt/wptrunner_machopo/` 經過 machopodriver，不直接啟動 machopo。

`scripts/bench/common.py` 的 `ChromePipe` 已經照 Chrome 的方式接：Linux、macOS 用 `sh -c 'exec "$0" "$@" 3<&{讀} 4>&{寫}'` 加上 `pass_fds`（`subprocess` 保留 fd 的號碼，`sh` 把它們搬到 3、4）；Windows 用 `--remote-debugging-io-pipes=<讀>,<寫>`、`os.set_handle_inheritable`、`STARTUPINFO.lpAttributeList = {"handle_list": [...]}`，只讓 Chrome 繼承那兩個 handle。`docs/limitations.md` 記著 Unix 的那條路還沒在 Linux、macOS 上跑過。

## Chromium

2026-11-26 讀 `main` 的原始碼：

- `content/browser/devtools/devtools_agent_host_impl.cc` 的 `StartRemoteDebuggingPipeHandler`：`read_fd = kReadFD`、`write_fd = kWriteFD`（3、4）。Windows 上有 `--remote-debugging-io-pipes` 時 `AdoptPipes`：值用逗號分成兩個，第一個是讀、第二個是寫；`AdoptHandle` 把十進位的值轉成 `HANDLE`，`GetFileType` 不是 `FILE_TYPE_PIPE` 就失敗，再 `_open_osfhandle`（讀的一端 `_O_RDONLY`）；失敗時呼叫 `on_disconnect`，不開 pipe。
- `content/browser/devtools/devtools_pipe_handler.cc`：Windows 上的 fd 經過 `_get_osfhandle` 變成 `HANDLE`（暫時換掉 CRT 的 invalid parameter handler，讓壞的 fd 回 `INVALID_HANDLE_VALUE` 而不是 crash）。`--remote-debugging-pipe` 的值是 `cbor` 時用 CBOR，其他用 NUL 結尾的 JSON（ASCIIZ），接收的 buffer 上限 100 MB（`kReceiveBufferSizeForDevTools`），寫的時候切成 64 KiB 一塊。讀到結尾或失敗時 post `OnDisconnect` 到 UI thread、關掉兩端。
- `chrome/browser/devtools/remote_debugging_server.cc`：`--remote-debugging-pipe` 時 `StartRemoteDebuggingPipeHandler(base::BindOnce(&ChromeDevToolsManagerDelegate::CloseBrowserSoon))`：pipe 斷了 Chrome 就結束。
- 子 process：`base::LaunchProcess` 在 POSIX 上只留下 `LaunchOptions::fds_to_remap` 列的 fd（其他全關），Windows 上只繼承 `handles_to_inherit` 列的 handle。renderer 拿不到 pipe，也不靠 close-on-exec。machopo 的 renderer 由 std 的 `Command` 啟動，它不關其他 fd、Windows 上繼承所有可繼承的 handle，所以 pipe 的兩端要自己設成 close-on-exec 和不可繼承。

## Playwright 和 Puppeteer

2026-11-26 讀 GitHub 上的 `main`：

- Playwright 的 `packages/utils/processLauncher.ts`：`stdio === 'pipe'` 時 `['ignore', 'pipe', 'pipe', 'pipe', 'pipe']`，Unix 上 `detached: true`（自己一個 process group，結束時 `kill(-pid)`）；stdout、stderr 用 `readline` 一行一行記成 log。
- `packages/playwright-core/src/server/browserType.ts`：`new PipeTransport(stdio[3], stdio[4])`；`ignoreAllDefaultArgs` 時命令列只有使用者給的 `args`。
- `packages/playwright-core/src/server/chromium/chromium.ts`：預設的開關一定加 `--remote-debugging-pipe`（使用者自己給就報錯「Playwright manages remote debugging connection itself」）、`--user-data-dir=…`、`--headless`（headless 時）、`about:blank` 或 `--no-startup-window`，加上 `chromiumSwitches.ts` 的約 40 個 Chrome 的開關（`--disable-field-trial-config`、`--no-first-run`、`--use-mock-keychain`……）。machopo 對不認得的開關報錯（`unknown option`），所以要 `ignoreAllDefaultArgs: true`，或 Playwright 的 RFC 讓 machopo 照 Chrome 忽略它們。
- `crBrowser.ts` 的 `CRBrowser.connect`：先 `Browser.getVersion`，再 `Target.setAutoAttach({ autoAttach: true, waitForDebuggerOnStart: true, flatten: true })`；persistent context 時另外 `Target.getTargetInfo`、`Browser.setDownloadBehavior`。machopo 的 `Target.setAutoAttach` 現在直接回空的結果（`crates/shell/src/automation_session.rs`），不照 `waitForDebuggerOnStart` 讓新的分頁停住：Playwright 的 RFC 要做的第一件事。
- WebKit（`webkit.ts`）給 `--inspector-pipe`、Firefox（`firefox.ts`）給 `-juggler-pipe`，同樣是 fd 3、4。
- Puppeteer 的 `packages/browsers/src/launch.ts`：`pipe: true` 時五個 stdio 都是 `'pipe'`，stdin 也是一個開著的 pipe（Puppeteer 不寫它）。

Windows 上 Node 的 `child_process.spawn` 把 3 以上的 stdio 經過 `STARTUPINFO` 的 `lpReserved2` 交給 child 的 C runtime（libuv 的做法），Chrome 用 `_get_osfhandle(3)` 拿到。Rust 的 `std::process::Command` 給不出 `lpReserved2`；machopo 要收它，得在 `ipc` 讀 `GetStartupInfoW` 的 `lpReserved2`（開頭 4 byte 是個數 n，接著 n byte 的旗標、n 個 `HANDLE`）或呼叫 UCRT 的 `_get_osfhandle`（Rust 在 MSVC 上經過 CRT 的啟動程式，C runtime 的 fd 表照樣初始化；要在 Windows 上確認）。

## 沒選的做法

- **Rust 的啟動程式也用 `sh -c 'exec … 3<&N 4>&M'`**（Python 的做法），不用 `pre_exec`：不用 `unsafe`，但 `sh` 要繼承沒有 close-on-exec 的 N、M，在父 process 裡拿掉 close-on-exec 時，同時啟動的其他 child（`shell` 的測試平行啟動很多 machopo）也會拿到；要在 child 裡拿掉，就回到 `pre_exec`。Python 的 `pass_fds` 是在 child 裡做的，所以 Python 不需要這一步。
- **Browser 啟動 renderer 時關掉 3 以上的所有 fd**（Chromium 的 `LaunchProcess`）：比 close-on-exec 更徹底，但 std 的 `Command` 沒有這個選項，要在 `pre_exec` 裡逐一 `close` 或用 `close_range`（Linux 5.9 以上、macOS 沒有）；只為了 fd 3、4，拿到時設 close-on-exec 就夠了，和 renderer 的 channel 現在的做法一致。
