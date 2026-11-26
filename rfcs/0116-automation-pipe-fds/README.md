# RFC 0116：automation pipe 改用 fd 3、4 和 Chrome 的開關

- 開始日期：2026-11-26
- 狀態：✅ 已完成：2026-11-26
- 進度：`progress.md`
- 細節：`details.md`

## 摘要

`machopo --automation-pipe` 原本從 stdin 讀命令、往 stdout 寫回答和事件。stdout 被訊息佔住，renderer 子 process 的 stdout 只好轉到 Browser 的 stderr；Playwright 的 `launch()`、Puppeteer 的 `pipe: true` 照 Chrome 的 `--remote-debugging-pipe` 給 fd 3、4，啟動不了 machopo。這份 RFC 把 pipe 換到 Chrome 的位置，開關也照 Chrome 分成兩個：`--remote-debugging-pipe` 是經過 pipe 驅動（讀 fd 3、寫 fd 4，訊息照舊是 JSON 加 NUL），`--headless` 是沒有平台視窗；`--automation-pipe` 和 `--window` 拿掉（原本的 `--automation-pipe` 是 `--remote-debugging-pipe --headless`，`--automation-pipe --window` 是 `--remote-debugging-pipe`），用了的人會被告知換成什麼。Linux、macOS 只有 fd 3、4 這一種接法；Windows 收 Chrome 的 `--remote-debugging-io-pipes=<讀>,<寫>` 給的 handle（Node 給的 C runtime 的 fd 3、4 不收）。stdin、stdout 不再是 pipe 的一部分，renderer 照 Chromium 繼承 Browser 的 stdout。拿繼承到的 fd 和 handle、在子 process 裡把 pipe 放到 3、4 在 `ipc`（`inherited_pipe`、`spawn_with_pipe`）；machopodriver、`shell` 的測試和情境、`scripts/bench/` 的工具都改過去，工具和驅動 Chrome 用同一段程式。做完之後 Playwright、Puppeteer 在 Linux、macOS 上接得上 pipe，還差 Chrome 的其他開關和 CDP 的一部分（`docs/limitations.md`）。

## 動機

**原本**（2026-11-26，`details.md` 的「盤點」）：

- `shell` 的 `automation_pipe` 模組（現在的 `crates/shell/src/devtools_pipe_handler.rs`）的 `spawn_reader` 讀 `std::io::stdin()`、`spawn_writer` 寫 `std::io::stdout()`；`--window` 的 `crates/shell/src/window.rs` 用同兩個函式。
- `crates/shell/src/lib.rs` 的 `renderer_launcher`：renderer 的 stdin 接 null、stdout 轉到 Browser 的 stderr，讓 pipe 只在 Browser 手上（client 在 Browser 結束時就讀到 EOF，renderer 寫的東西不混進訊息）。
- `crates/shell/src/main.rs` 的 log 和 `crates/renderer/src/render_thread.rs` 的 `print_console` 一律寫 stderr，理由都是「stdout 是 pipe 的」。
- `crates/shell/src/main.rs` 的 `parse_args`：`--automation-pipe` 同時代表「經過 pipe 驅動」（`Machopo` domain、`--host-resolver-rules` 和 `--additional-trust-anchors`、不理平台的輸入、從 `about:blank` 開始、淺色和橫式的 UI、profile 預設在記憶體裡）和「沒有平台視窗」，加 `--window` 才有真的視窗。
- 啟動 `--automation-pipe` 的地方都接 stdin、stdout：machopodriver、`shell` 的測試、`scripts/bench/common.py` 的 `MachopoPipe` 和 `scripts/bench/raster/raster.py`；同一個檔案的 `ChromePipe` 已經照 Chrome 的方式接 fd 3、4。

**問題**：

- **Playwright、Puppeteer 啟動不了 machopo**：兩者啟動 Chromium 時都開五個 stdio（Playwright `['ignore', 'pipe', 'pipe', 'pipe', 'pipe']`，Puppeteer 五個都是 `pipe`，Windows 上也一樣），給 `--remote-debugging-pipe`，headless 時加 `--headless`，在 `stdio[3]` 寫命令、從 `stdio[4]` 讀回答。這是 Playwright 那條路（RFC 0075 的「動機」的路線第 4 項）在傳輸和開關上的前提，CDP 的部分是另一件事。
- **stdout 不能用**：renderer 的 stdout 要特地轉走；手動測試、除錯時印到 stdout 的東西會弄壞 pipe。
- **stdin 的意思和 Chrome 不同**：machopo 在 stdin 關掉時結束；Puppeteer 給的 stdin 是一個開著、從來不寫的 pipe，Chrome 不理它。
- **開關和 Chrome 不同**：Chrome 的 `--remote-debugging-pipe` 只管傳輸，有沒有視窗由 `--headless` 決定、預設有視窗；machopo 的 `--automation-pipe` 把兩者綁在一起、預設沒有視窗。

**不在範圍內**（Playwright 的 RFC）：Chrome 的其他開關（Playwright 預設給約 40 個）、`--remote-debugging-pipe` 時命令列上的網址、沒有 `--headless` 時視窗收不收平台的輸入、`--headless` 不配 pipe、`Target.setAutoAttach` 的 `waitForDebuggerOnStart`、browser context 這類 CDP、Windows 上 Node 給的 C runtime 的 fd 3、4、CBOR 的格式（`--remote-debugging-pipe=cbor`）。`--remote-debugging-port` 和 websocket 不動。

**解決的 `docs/limitations.md` 裡的限制**（「WebDriver 和自動化」）：「pipe 是 stdin 和 stdout，不是 Chrome 的 fd 3 和 4」那一條改寫成 Playwright、Puppeteer 還差什麼；「網頁的輸出和 log 混在 stderr」那一條行為不變（「已決定的事項」第 3 條），理由改成照 Chrome；真的視窗不理會輸入那一條換成新的開關，補上和 Chrome 的不同。

## 設計概要

**開關**：

| 原本 | 現在 | 意思 |
| --- | --- | --- |
| `--automation-pipe` | `--remote-debugging-pipe --headless` | 經過 pipe 驅動，沒有平台視窗（machopodriver、測試、bench 用的） |
| `--automation-pipe --window` | `--remote-debugging-pipe` | 經過 pipe 驅動，真的視窗（`test.py scenarios --window`、`bench.py raster --window`） |
| `--window` 只在 pipe 時收 | 拿掉；`--headless` 只在 pipe 時收 | |
| `--scale`（pipe、沒有 `--window` 時預設 1） | 同樣的規則，條件是 `--headless` | |
| `--host-resolver-rules`、`--additional-trust-anchors`、`Machopo` domain | 跟著 `--remote-debugging-pipe` | |
| | `--remote-debugging-io-pipes=<讀>,<寫>`：Windows 上 pipe 的兩個 handle；macOS、Linux 上照 Chrome 不看 | |

`--automation-pipe`、`--window` 是不認得的開關，錯誤訊息說換成什麼；`--remote-debugging-pipe=cbor` 是錯誤（不收值）。`test.py scenarios --window`、`bench.py raster --window` 是工具自己的選項，名字不變。

**啟動 machopo 的程式**：

```rust
// Rust：ipc 在子 process 裡把兩端放到 3、4（Windows 是可繼承的 handle 加上開關）
let mut command = Command::new(machopo);
command.args(["--remote-debugging-pipe", "--headless"]);
let (child, mut writer, reader) = ipc::platform::spawn_with_pipe(&mut command)?;
writer.write_all(b"{\"id\":1,\"method\":\"Browser.getVersion\"}\0")?;
```

```python
# Python：scripts/bench/common.py，和驅動 Chrome 的同一段
process, to_browser, from_browser = common.start_with_pipe(
    ["machopo", "--remote-debugging-pipe", "--headless"], stdin=subprocess.DEVNULL)
```

手動在 shell 裡試：`machopo --remote-debugging-pipe --headless 3<&0 4>&1`（把 stdin、stdout 接到 3、4，等於原本的 `--automation-pipe`）。

**machopo 這一側**：開始時先拿 fd 3、4（Windows 是開關給的兩個 handle）：確認是 pipe 或 socket，設成 close-on-exec（Windows 是不可繼承），之後的 reader、writer 讀寫它們；沒有或不是 pipe 時說出原因（附上 `3<&0 4>&1` 的寫法）、以 2 結束。stdin 不讀，stdout 不寫訊息；log 和 `console` 照舊在 stderr（Chrome 也是）。renderer 的 stdout 照 Chromium 繼承 Browser 的。

**使用的人看到的變化**：machopodriver、WPT、情境、bench 的工具用法不變；直接用 `--automation-pipe` 的指令和程式要換成新的開關、改用 fd 3、4，和驅動 Chrome 的 `--remote-debugging-pipe` 一樣。

## 詳細設計

### 1. `ipc` 的兩個入口（`crates/ipc/src/platform/pipe.rs`）

`unsafe` 只在 `ipc` 和 `bindings`，這裡兩件事都要；論證在 `crates/ipc/soundness.md` 的「被 pipe 驅動的 process 的兩端（`platform/pipe.rs`）」。

- **被啟動的一側**：`inherited_pipe(io_pipes: Option<&str>) -> Result<(PipeReader, PipeWriter), InheritError>`（第 4、5 條）。一個 process 只問一次，不管結果（`TAKEN`，在看之前就設：之後 3、4 上的是這個 process 自己開的東西）。
  - Unix：`fstat` 確認 fd 3、4 是 FIFO 或 socket（Node 的 `'pipe'` stdio 在 Unix 上是 socketpair，第 6 條），`OwnedFd::from_raw_fd`，`fcntl_setfd(CLOEXEC)`；不看 `io_pipes`（第 10 條）。
  - Windows：解析 `io_pipes` 的兩個十進位 handle（讀的在前，Chrome 的順序），`GetFileType == FILE_TYPE_PIPE`、兩者不是同一個，`OwnedHandle::from_raw_handle`，`SetHandleInformation` 拿掉 `HANDLE_FLAG_INHERIT`：std 的 `Command` 在 Windows 上繼承所有可繼承的 handle，不拿掉 renderer 就會拿到（libuv 的程式啟動時也這樣處理繼承到的 C runtime fd）。
  - 錯誤（`InheritError`）：沒開著（`Missing`）、不是 pipe（`NotAPipe`，兩者都說是哪一端）、Windows 上沒有開關（`NoHandles`）或值不對（`BadHandles`）、問過了（`Taken`）、系統呼叫失敗（`Io`）。
  - 要在任何東西開檔案之前呼叫：fd 3、4 沒開時，之後開的檔案會拿到 3、4。
- **啟動的一側**：`spawn_with_pipe(&mut Command) -> io::Result<(Child, PipeWriter, PipeReader)>`（第 7 條），對應 Chromium 的 `LaunchOptions::fds_to_remap` 和 `handles_to_inherit`。兩對 pipe 由 `std::io::pipe()` 建（close-on-exec、不可繼承，不需要 `unsafe`）。
  - Unix：child 的兩端號碼小於 5 的先複製到 5 以上（一個 3、4 是空的 process 建的 pipe 就在 3、4），`pre_exec` 裡 `dup2` 到 3、4（只有 async-signal-safe 的呼叫、不配置記憶體）；啟動完關掉自己手上 child 的那兩端。
  - Windows：兩端設成可繼承、在 `Command` 的最後加 `--remote-debugging-io-pipes=<讀>,<寫>`、啟動、關掉自己的那一份，這一段在 `starting_a_child` 的鎖底下：通道的 `spawn` 也用這把鎖（原本是它自己的），兩種 child 同時啟動時互相拿不到。
  - machopodriver、`shell` 的測試和情境用它；`driver` 多依賴 `ipc`（`ipc` 只依賴 `rustix`、`mach2`、`windows-sys` 這一層，沒有 V8）。

### 2. `shell`

- **拿 pipe**（第 11 條）：`main` 解析完開關之後、開 log 和 trace 之前呼叫 `inherited_pipe`（`take_pipe`），把 `--remote-debugging-io-pipes` 的值交給它；拿不到時把原因印到 stderr（Unix 上沒開著、不是 pipe 時附上 `3<&0 4>&1`），以 2 結束，和開關寫錯一樣。
- **開關**（`parse_args`、help、usage）：照「設計概要」的表；`--headless`、`--host-resolver-rules`、`--additional-trust-anchors`、`--remote-debugging-io-pipes` 只在 `--remote-debugging-pipe` 時收。
- **模組**：`automation_pipe.rs` 改名 `devtools_pipe_handler.rs`（Chromium 的 `DevToolsPipeHandler`），和 `devtools_http_handler.rs` 成對；`Pipe` 是兩端，`spawn_reader`、`spawn_writer` 收它們，`run` 和 `window.rs` 從 `main` 拿到（真的視窗等視窗起來才開始讀寫）。測試模組 `crates/shell/tests/it/automation_pipe.rs` 和 `automation_session.rs` 這些不是傳輸的模組名字不動。
- 結束的條件照舊：pipe 的輸入讀到結尾（Chrome 是 `CloseBrowserSoon`）、`Browser.close`、訊號；writer 寫不出去時照 `wait_for_writer` 的規則放掉。
- `lib.rs` 的 `renderer_launcher`：stdin 照 Chromium 的 `LaunchProcess` 接 null，stdout 和 stderr 繼承 Browser 的。

### 3. 量測工具（`scripts/bench/`）

- **`start_with_pipe`**（`common.py`）：照 Chrome 的方式連同 pipe 啟動一個 browser：Linux、macOS 經過 `sh -c 'exec "$0" "$@" 3<&N 4>&M'`（`pass_fds` 保留號碼，取樣器包著時一路傳下去），Windows 給 `--remote-debugging-io-pipes`（放在命令的最後）、兩個 handle 設成可繼承、`STARTUPINFO` 的 `handle_list` 只讓 browser 繼承它們。`ChromePipe`、`MachopoPipe` 和 `raster.py` 都經過它；`MachopoPipe` 給 `--remote-debugging-pipe --headless`，machopo 的 stdout 和 stderr 寫 log。
- **`--against` 的舊版本**：還是 `--automation-pipe`、經過 stdin、stdout。`against_binaries` 問執行檔的 `--help`（`has_stdio_pipe`：舊版本的有 `--automation-pipe`），是的話記進 `WITH_STDIO_PIPE`，`MachopoPipe` 照舊的方式啟動它。不照 commit 判斷（`WITHOUT_SINGLE_PROCESS` 那樣）：這份 RFC 的 commit 在 merge 前 rebase 時 hash 會變。

### 4. 輸出

- log：stderr（照舊，Chrome 也是）。
- `console`、沒接住的例外、wasm guest 的 stdout 和 stderr：照舊都當成 console 的訊息寫 stderr（「已決定的事項」第 3 條）。網頁寫得出來的東西都不進 stdout。

### 測試

驗收的項目和結果在 `progress.md`。

- **`ipc`**（`crates/ipc/tests/it/inherited_pipe.rs`，測試把自己再執行一次當成 child，第 8 條；參數以 `--` 結尾，Windows 上 `spawn_with_pipe` 加在最後的開關是多的一個測試名稱）：讀寫兩端；child 的 child 拿不到（孫 process 活 4 秒，host 兩秒內讀到結尾）；同時啟動的 child 互相拿不到（Windows 上連同通道的 child）；只問一次；shell 交的 pipe、socketpair、3、4 是空的 process；拿不到時的每一種原因。
- **`shell`**：原本的 pipe 的測試全部經過新的開關和接法；新增沒有 pipe 時說出原因、stdin 開著不寫時照常、renderer 的 stdout 是 Browser 的（Linux 比對 `/proc/<pid>/fd/1`，macOS 比對 `lsof` 對 pipe 說的另一端，不需要 `unsafe`）；`main.rs` 的開關。
- **量測工具**：`speedometer_test.py` 的 `PipeTests`（讀 3、寫 4 的假 browser、舊版本的判斷和啟動）；Windows 上 `blink_perf_test.py` 的 `RunTests`、`GcRunTests` 經過 handle 驅動真的 Chrome 和 machopo。
- **mutation test**：`mutants-1.toml`、`mutants-2a.toml`、`mutants-2b.toml`（macOS、Linux）、`mutants-3.toml`（Windows），每一個都抓到。Python 的 `start_with_pipe` 的 Windows 分支沒有 mutant，記在 `docs/test-gaps.md`。
- **沒做的**：計畫裡用 Playwright 的 `chromium.launch({ executablePath, ignoreAllDefaultArgs: true, args: ['--remote-debugging-pipe', '--headless'] })` 手動確認連得上：要多裝 Node 和 Playwright，留給 Playwright 的 RFC。

### 分階段

1. **`ipc`**：兩個入口和它們的測試（第 4 到 8 條）。
2. **換過去**（第 9 到 11 條）：2a `shell` 的開關、reader、writer 和 `main`、renderer 的 stdout，連同 Rust 的啟動程式；2b Python 的工具；2c 文件和限制。
3. **Windows**：在使用者的裝置上編譯 Windows 的那一份、跑測試、mutation test（`mutants-3.toml`，為它補了兩個測試）和一次 Speedometer。

都在 2026-11-26 做完（`progress.md`）。

## 缺點

- `ipc` 多兩處 `unsafe` 的論證（拿 fd 和 handle、`pre_exec` 裡的 `dup2`）。
- 手動驅動 machopo 從 `echo … | machopo --automation-pipe` 變成要寫 `--remote-debugging-pipe --headless 3<&0 4>&1`。
- 預設變成有視窗：忘了加 `--headless` 的指令會開出一個在最上層、不理輸入的視窗。
- 取樣器、其他包著 machopo 的程式要把 fd 3、4（Windows 是可繼承的 handle）傳過去，以後加新的包裝工具時也要注意（Chrome 有同樣的問題）。
- 量測工具要分辨 `--against` 的版本是哪一種 pipe，多一次執行 `--help`。

## 理由和其他做法

**開關和模式**（「已決定的事項」第 1 條）：

- **N1**：`--automation-pipe` 留著名字和意思，只換到 fd 3、4。改動最少，但開關和 Chrome 不同，Playwright 的 RFC 還要再改一次開關。
- **N2**：N1 再加 `--automation-pipe=stdio`，讓手動測試照舊。兩種接法都要測，stdio 模式下 renderer 的 stdout 又要轉走；`3<&0 4>&1` 已經能做到同樣的事。
- **N3**：用 Chrome 的名字 `--remote-debugging-pipe`。對應有三種：
  - **N3a**：只改名，`--remote-debugging-pipe` 照 `--automation-pipe` 的意思，預設沒有視窗、`--window` 照舊：和 Chrome 的意思相反，Playwright 不開 headless 時期待有視窗卻沒有，開 headless 時給的 `--headless` 又不收，Playwright 的 RFC 一定要再改。
  - **N3b（選這個）**：照 Chrome 拆成 `--remote-debugging-pipe`（經過 pipe 驅動）和 `--headless`（沒有平台視窗），`--window` 拿掉。兩種現在的用法各自一對一對應，啟動的程式這次本來就要改，多加一個 `--headless` 的成本很小。
  - **N3c**：兩個名字都留：同一件事有兩個名字，兩個都要測、都要寫進文件。

**Windows**（「已決定的事項」第 2 條）：

- **W1**：和 Chrome 一樣，同時收 C runtime 的 fd 3、4 和 `--remote-debugging-io-pipes` 的 handle，兩者在 `ipc` 裡都變成同一種 `File`。Playwright、Puppeteer 在 Windows 上經過 Node（libuv 把 handle 放進 `STARTUPINFO.lpReserved2`）只用 C runtime 的 fd；我們自己的程式（std 的 `Command`、Python 的 `subprocess`）填不了 `lpReserved2`，用 handle 的開關，經過 samply 也傳得下去。成本：`ipc` 多一個入口和一處 `unsafe`，測試自己呼叫 `CreateProcessW`（不需要 Node）；要在 Windows 上確認 Rust 的程式在 MSVC 上也有 C runtime 的 fd 表。
- **W2（選這個）**：只收 handle 的開關。範圍最小，我們自己的程式和 samply 都用這個；Windows 上的 Playwright 在傳輸這一層就連不上，要等之後加上 C runtime 的入口（那時變成 W1）。
- **W3**：只收 C runtime 的 fd 3、4：每個啟動的程式都要自己呼叫 `CreateProcessW`（Rust 在 `ipc` 裡、Python 用 ctypes），samply 如果用 Rust 的 `Command` 啟動 machopo，`lpReserved2` 在那一層就丟掉，Windows 上的取樣連不上。和 Chrome 不同、成本最高，排除。

**不做**：Playwright 的那份 RFC 一開始就要做這件事；先獨立做，傳輸和開關的改動（碰到所有啟動 machopo 的程式）和 CDP 的改動分開審核，stdout 也早一點空出來。

## 先例

- **Chromium**：`--remote-debugging-pipe` 讀 fd 3、寫 fd 4（`DevToolsAgentHostImpl::StartRemoteDebuggingPipeHandler` 的 `kReadFD`、`kWriteFD`）；Windows 上預設把它們當成 C runtime 的 fd，有 `--remote-debugging-io-pipes=<讀>,<寫>` 時改用那兩個 handle（`AdoptPipes` 確認是 pipe、`_open_osfhandle` 包成 fd），之後 `DevToolsPipeHandler` 一律用 `_get_osfhandle` 拿回 handle 讀寫。訊息是 NUL 結尾的 JSON（ASCIIZ，接收的 buffer 上限 100 MB）或 CBOR。讀到結尾時 Chrome 呼叫 `ChromeDevToolsManagerDelegate::CloseBrowserSoon`。有沒有視窗由 `--headless` 決定，和傳輸無關。子 process 由 `LaunchProcess` 啟動，只拿到 `fds_to_remap` 列出的 fd，pipe 不會漏給 renderer。**照做**：開關的名字和分工、fd 的號碼、Windows 的開關、NUL、結尾時結束、子 process 拿不到、macOS 和 Linux 上不看 `--remote-debugging-io-pipes`。**不照做**：fd 3、4 不是 pipe 時 machopo 說出原因、結束（Chrome 照樣讀、讀不到時斷線）；`--headless` 只在 pipe 時收；Windows 上的 C runtime 的 fd 和 CBOR 不做。
- **Playwright 的其他瀏覽器**：WebKit 的 `--inspector-pipe`、Firefox 的 `-juggler-pipe` 也是 fd 3、4，Playwright 對三個瀏覽器用同一個 `PipeTransport(stdio[3], stdio[4])`。
- **chromedriver**：用 `--remote-debugging-port`（或 pipe）啟動 Chrome，Chrome 的 stdout、stderr 照常是 log。

## 已決定的事項

2026-11-26 和使用者逐條確認，同一天接受（第 1 到 3 條）；第 1 段的細節在排程時確認（第 4 到 8 條），第 2 段的在它開始前（第 9 到 11 條）。

1. **開關照 Chrome 拆成 `--remote-debugging-pipe` 和 `--headless`（N3b）**（2026-11-26，使用者）：`--remote-debugging-pipe` 是經過 pipe 驅動（fd 3、4，連同現在只在 pipe 時有的 `Machopo` domain、測試的網路設定、不理平台的輸入、`about:blank`、淺色和橫式的 UI、記憶體裡的 profile），`--headless` 是沒有平台視窗、只在 pipe 時收；`--automation-pipe` 和 `--window` 拿掉。Linux、macOS 只有 fd 3、4 這一種接法，不留 stdin、stdout 的模式。不選 N1（留 `--automation-pipe`：開關和 Chrome 不同，Playwright 的 RFC 要再改一次）、N2（另留 stdio 模式：兩種接法要測，renderer 的 stdout 的例外留著，`3<&0 4>&1` 就做得到）、N3a（只改名：和 Chrome 的意思相反）、N3c（兩個名字都留：同一件事兩個名字）。
2. **Windows 只收 `--remote-debugging-io-pipes` 的 handle（W2）**（2026-11-26，使用者）：我們自己啟動 machopo 的程式（std 的 `Command`、Python 的 `subprocess`）填不了 `lpReserved2`，只能給 handle，經過 samply 也傳得下去；Node 給的 C runtime 的 fd 3、4 這份不做，Windows 上的 Playwright、Puppeteer 在傳輸這一層連不上，留到之後（「之後的可能」）。不選 W1（兩種都收：多一個入口、一處 `unsafe`，還要在 Windows 上確認 Rust 的程式拿得到 C runtime 的 fd，現在沒有使用者）、W3（只收 C runtime 的 fd：每個啟動的程式都要自己呼叫 `CreateProcessW`，samply 那一層可能把 `lpReserved2` 丟掉）。
3. **wasm guest 的 stdout 和 stderr 照舊當成 console 的訊息、寫 stderr（O1）**（2026-11-26，使用者）：網頁寫得出來的東西只有一條路，和 `console.log` 一致，照 Chrome 留在 log 裡（Chrome 的 `console` 開了 log 時印在 stderr，從不寫 stdout）；stdout 空出來是給 renderer 和除錯的。不選 O2（guest 的 stdout 寫 machopo 的 stdout：WASI 的本意，但網頁就能寫滿終端機、讓不讀 stdout 的 client 塞住 renderer，wasm 文件的 stdout 也進不了 console）。
4. **第 1 段：兩端是 std 的 `PipeReader`、`PipeWriter`**（2026-11-26，照建議，使用者同意）：Rust 1.87 起有的 `std::io::pipe()` 建出來的就是這兩種，本來就是 close-on-exec、不可繼承，啟動的一側建 pipe 不需要 `unsafe`；拿到的 fd 和 handle 也轉成它們（`From<OwnedFd>`、`From<OwnedHandle>`）。不選 `File`：讀寫得了，但名字看不出是 pipe。
5. **第 1 段：`inherited_pipe(io_pipes: Option<&str>) -> Result<(PipeReader, PipeWriter), InheritError>`**（2026-11-26，照建議，使用者同意；原本「待決定」的第 1 條）：Windows 的 `--remote-debugging-io-pipes` 的值由 `shell` 的 `parse_args` 解析後傳進來，`ipc` 不讀命令列；Unix 上不看這個值，照 Chrome 忽略（原本是錯誤，第 10 條改掉：正常使用時沒有人在 Unix 上給它）。回傳原因（沒有、不是 pipe、開關的值不對、已經拿過），`shell` 照它印出錯誤。不選回 `Option`（`inherited()` 的寫法）：印不出為什麼。不選 `ipc` 自己讀命令列：命令列由 `shell` 負責。
6. **第 1 段：Unix 上 fd 3、4 收 FIFO 和 socket，Windows 上收 `FILE_TYPE_PIPE`**（2026-11-26，照建議，使用者同意）：Node 的 `'pipe'` stdio 在 Unix 上是 socketpair（libuv），Playwright、Puppeteer 給的就是 socket；Python 的 `os.pipe()` 和 `std::io::pipe()` 是 FIFO。Windows 照 Chrome 的 `AdoptPipes`。對照 Chromium：POSIX 上不檢查、照樣讀，讀不到時斷線；machopo 兩邊都檢查。不選只收 FIFO：Playwright 連不上。
7. **第 1 段：`spawn_with_pipe(&mut Command) -> io::Result<(Child, PipeWriter, PipeReader)>`**（2026-11-26，照建議，使用者同意）：寫的一端是 child 的 fd 3，讀的一端是 child 的 fd 4；對應 Chromium 的 `LaunchOptions::fds_to_remap`（Windows 是 `handles_to_inherit`）。
8. **第 1 段：測試照 `process_channel.rs` 讓測試自己再啟動一次當 child**（2026-11-26，照建議，使用者同意）：`crates/ipc/tests/it/` 一個新的模組，環境變數說 child 要做什麼；測 child 在 3、4 讀寫得到、child 再啟動的 grandchild 拿不到 3、4、沒有 3、4 和 3、4 是一般檔案和拿第二次各自回對的原因。
9. **第 2 段分成三小段**（2026-11-26，照建議，使用者同意）：2a 是 `shell` 的開關、拿 pipe、reader 和 writer、renderer 的 stdout，連同 Rust 的啟動程式（machopodriver、`shell` 的測試、情境），它們要一起改，不然測試連不上；2b 是 Python 的工具（`MachopoPipe` 和 `ChromePipe` 共用接 fd 的那一段、`raster.py`）和 bench 工具的四份測試；2c 是文件和限制。不選一次做完：一個 commit 碰到四、五十個檔案，難審。
10. **第 2 段：macOS、Linux 上照 Chrome 忽略 `--remote-debugging-io-pipes`（b3）**（2026-11-26，使用者決定）：`shell` 在每個平台都收這個開關（只在 `--remote-debugging-pipe` 時），值交給 `inherited_pipe`；Unix 上它不看這個值，照樣讀 fd 3、4。改掉第 5 條的「Unix 上給了值是錯誤」：`InheritError::HandlesOnlyOnWindows` 和它的測試、mutant 在 2a 拿掉，換成「Unix 上給了也照樣讀寫 fd 3、4」的測試。對照 Chromium：只在 Windows 的程式裡讀這個開關。不選 b1（每個平台都收、Unix 上報錯）：正常使用時沒有人在 Unix 上給它，和 Chrome 一樣忽略就夠了。不選 b2（只在 Windows 上收、其他平台是不認得的開關）：`parse_args` 要分平台，錯誤訊息也沒說為什麼。
11. **第 2 段：沒有 pipe 時說出原因再結束（c1）**（2026-11-26，照建議，使用者同意）：`main` 解析完開關之後、開 log 和 trace 之前拿 pipe（開任何檔案之前拿，本來就是第 1 段的要求），拿不到就把 `InheritError` 的原因印到 stderr、附上 `3<&0 4>&1` 的寫法，照開關寫錯的方式結束。對照 Chromium：POSIX 上不檢查，讀失敗就 `CloseBrowserSoon`。不選照 Chrome（c2）：使用者只看到 machopo 馬上結束，fd 3 剛好開著別的東西時還會把它當成命令讀。

## 待決定

無。

## 之後的可能

- **Playwright 的 RFC**：Chrome 的其他開關、命令列上的網址、沒有 `--headless` 時視窗收不收平台的輸入、`--headless` 不配 pipe、`waitForDebuggerOnStart`、browser context、`Emulation`、`Fetch`、isolated world（RFC 0075 的路線第 4 項）；這份沒做的、用 Playwright 的 `launch()` 手動連一次，在那裡做，記下連上之後卡在哪個命令。
- **Windows 上的 C runtime 的 fd 3、4**：Node（libuv）經過 `STARTUPINFO.lpReserved2` 給的，Windows 上的 Playwright、Puppeteer 要它；在 `ipc` 加一個入口，和 handle 一樣變成 `File`（`_get_osfhandle`，或自己讀 `GetStartupInfoW` 的 `lpReserved2`；`details.md` 的「Playwright 和 Puppeteer」），測試自己呼叫 `CreateProcessW` 填 `lpReserved2`。和 Playwright 的 RFC 一起做。
- **CBOR**：Chrome 的 `--remote-debugging-pipe=cbor`，Puppeteer 和 Playwright 都沒用，有 client 要時再做。
- **multi-process 之後的 DevTools 的 pipe**：拆出 network、GPU process 時，它們也照這份的規則拿不到 fd 3、4。
