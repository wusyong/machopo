# RFC 0116：automation pipe 改用 fd 3、4 和 Chrome 的開關

- 開始日期：2026-11-26
- 狀態：👍 已接受：2026-11-26
- 細節：`details.md`

## 摘要

`machopo --automation-pipe` 現在從 stdin 讀命令、往 stdout 寫回答和事件。stdout 被訊息佔住，所以 renderer 子 process 的 stdout 被轉到 Browser 的 stderr，網頁的輸出和 log 也只能寫 stderr；Playwright 的 `launch()`、Puppeteer 的 `pipe: true` 照 Chrome 的 `--remote-debugging-pipe` 給 fd 3、4，啟動不了 machopo。這份 RFC 把 pipe 換到 Chrome 的位置，開關也照 Chrome 分成兩個：`--remote-debugging-pipe` 是經過 pipe 驅動（讀 fd 3、寫 fd 4，訊息照舊是 JSON 加 NUL），`--headless` 是沒有平台視窗；`--automation-pipe` 和 `--window` 拿掉（原本的 `--automation-pipe` 是 `--remote-debugging-pipe --headless`，`--automation-pipe --window` 是 `--remote-debugging-pipe`）。Linux、macOS 只有 fd 3、4 這一種接法，不留 stdin、stdout 的模式；Windows 收 Chrome 的 `--remote-debugging-io-pipes=<讀>,<寫>` 給的 handle（Node 給的 C runtime 的 fd 3、4 之後再做）。stdin、stdout 還給一般的用途，renderer 照 Chromium 繼承 Browser 的 stdout。把繼承到的 fd 和 handle 變成檔案、在子 process 裡把 pipe 放到 3、4 都要 `unsafe`，放在 `ipc`。啟動 machopo 的程式（machopodriver、`shell` 的測試、`scripts/bench/` 的工具）一次全部改過去。Playwright 要的其他東西（Chrome 的其他開關照收、`waitForDebuggerOnStart`、browser context……）不在這份。

## 動機

**現況**（2026-11-26，`details.md` 的「盤點」）：

- `crates/shell/src/automation_pipe.rs` 的 `spawn_reader` 讀 `std::io::stdin()`、`spawn_writer` 寫 `std::io::stdout()`；`--window` 的 `crates/shell/src/window.rs` 用同兩個函式。
- `crates/shell/src/lib.rs` 的 `renderer_launcher`：renderer 的 stdin 接 null、stdout 轉到 Browser 的 stderr，讓 pipe 只在 Browser 手上（client 在 Browser 結束時就讀到 EOF，renderer 寫的東西不混進訊息）。註解自己寫著 Chromium 的子 process 是保留 stdout 的，因為它的 pipe 是另外兩個 fd。
- `crates/shell/src/main.rs` 的 log 和 `crates/renderer/src/render_thread.rs` 的 `print_console`（`console`、沒接住的例外、wasm guest 的輸出）一律寫 stderr，理由都是「stdout 是 pipe 的」。
- `crates/shell/src/main.rs` 的 `parse_args`：`--automation-pipe` 同時代表「經過 pipe 驅動」（`Machopo` domain、`--host-resolver-rules` 和 `--additional-trust-anchors`、不理平台的輸入、從 `about:blank` 開始、淺色和橫式的 UI、profile 預設在記憶體裡）和「沒有平台視窗」，加 `--window` 才有真的視窗。`--headless` 是不認得的開關（machopo 以前有一個畫一張圖就結束的 `--headless`，拿掉了）。
- 啟動 `--automation-pipe` 的地方都接 stdin、stdout：`crates/driver/src/machopo.rs`；`shell` 的測試（`crates/shell/tests/it/automation_pipe.rs`、`renderer_process.rs`、`remote_debugging.rs`、`crates/shell/tests/scenarios.rs`）；`scripts/bench/common.py` 的 `MachopoPipe` 和 `scripts/bench/raster/raster.py`。同一個檔案的 `ChromePipe` 已經照 Chrome 的方式接 fd 3、4（Windows 用 `--remote-debugging-io-pipes`）。

**問題**：

- **Playwright、Puppeteer 啟動不了 machopo**：兩者啟動 Chromium 時都開五個 stdio（Playwright `['ignore', 'pipe', 'pipe', 'pipe', 'pipe']`，Puppeteer 五個都是 `pipe`，Windows 上也一樣），給 `--remote-debugging-pipe`，headless 時加 `--headless`，在 `stdio[3]` 寫命令、從 `stdio[4]` 讀回答；stdout、stderr 一行一行讀成 log。machopo 現在只能讓它們 `connectOverCDP` 連 `--remote-debugging-port`。這是 Playwright 那條路（RFC 0075 的「動機」的路線第 4 項）在傳輸和開關上的前提，CDP 的部分是另一件事。
- **stdout 不能用**：renderer 的 stdout 要特地轉走；網頁和 wasm guest 的輸出只能和 log 混在 stderr；手動測試、除錯時印到 stdout 的東西會弄壞 pipe。
- **stdin 的意思和 Chrome 不同**：machopo 在 stdin 關掉時結束。Puppeteer 給的 stdin 是一個開著、從來不寫的 pipe，Chrome 不理它；換到 fd 3、4 之後 machopo 也不理它，結束只看 fd 3。
- **開關和 Chrome 不同**：Chrome 的 `--remote-debugging-pipe` 只管傳輸，有沒有視窗由 `--headless` 決定、預設有視窗；machopo 的 `--automation-pipe` 把兩者綁在一起、預設沒有視窗。

**目標**：

1. 開關照 Chrome：`--remote-debugging-pipe` 和 `--headless`（「已決定的事項」第 1 條）。
2. Browser 照 Chrome 讀 fd 3、寫 fd 4，格式照舊（JSON 加 NUL，上限照舊）；fd 3 讀到結尾時照舊結束。Linux、macOS 只有這一種接法。
3. Windows 上收 `--remote-debugging-io-pipes=<讀>,<寫>` 的 handle（「已決定的事項」第 2 條）。
4. pipe 只在 Browser 手上：renderer 和 Browser 之後啟動的任何 process 都拿不到 fd 3、4 和那兩個 handle。
5. renderer 照 Chromium 繼承 Browser 的 stdout，不再轉到 stderr；stdin 照舊是 null（Chromium 的 `LaunchProcess` 也是）。
6. 啟動 machopo 的 Rust 和 Python 程式全部改用新的開關和 fd 3、4，測試和量測工具照常。

**不在範圍內**（Playwright 的 RFC）：

- Chrome 的其他開關照收或忽略（Playwright 預設給約 40 個，machopo 現在報 `unknown option`）。
- `--remote-debugging-pipe` 時命令列上的網址（Playwright 給 `about:blank`，machopo 現在報錯）。
- `--remote-debugging-pipe` 沒有 `--headless` 時，視窗照舊不理平台的滑鼠和鍵盤；Playwright 不開 headless 時使用者通常想在視窗裡自己操作。
- `--headless` 不和 `--remote-debugging-pipe` 一起時（Chrome 可以只配 `--remote-debugging-port`）：這份照現在 `--window` 的規則，只在 pipe 時收。
- `Target.setAutoAttach` 的 `waitForDebuggerOnStart`、browser context、`Browser.setDownloadBehavior`……
- Windows 上 Node 給的 C runtime 的 fd 3、4（`STARTUPINFO.lpReserved2`，Playwright、Puppeteer 在 Windows 上只用這個）。
- CBOR 的格式（Chrome 的 `--remote-debugging-pipe=cbor`）。
- `--remote-debugging-port`、websocket：不動。

要解決的 `docs/limitations.md` 裡的限制（「WebDriver 和自動化」）：

- 「pipe 是 stdin 和 stdout，不是 Chrome 的 fd 3 和 4」那一條：傳輸和開關的部分解決；Playwright、Puppeteer 還是啟動不了（其他開關和 CDP；Windows 上還有 C runtime 的 fd），改寫成只說那一半，指向 Playwright 要的東西。
- 「網頁的輸出……和 machopo 的 log 一樣都在 stderr（stdout 是 pipe 的）」那一條：行為不變（「已決定的事項」第 3 條），理由改成照 Chrome：網頁寫得出來的東西都留在 log 裡，不寫 stdout。
- 「`machopo --automation-pipe --window`（真的視窗）期間不理會平台的滑鼠和鍵盤……」那一條：行為不變，開關改成 `--remote-debugging-pipe`（沒有 `--headless`），照新的名字改寫。

## 設計概要

**開關**：

| 現在 | 之後 | 意思 |
| --- | --- | --- |
| `--automation-pipe` | `--remote-debugging-pipe --headless` | 經過 pipe 驅動，沒有平台視窗（driver、測試、bench 用的） |
| `--automation-pipe --window` | `--remote-debugging-pipe` | 經過 pipe 驅動，真的視窗（`test.py scenarios --window`、`bench.py raster --window`） |
| `--window` 只在 pipe 時收 | 拿掉；`--headless` 只在 pipe 時收 | |
| `--scale`（pipe、沒有 `--window` 時預設 1） | 同樣的規則，條件改成 `--headless` | |
| `--host-resolver-rules`、`--additional-trust-anchors`、`Machopo` domain | 跟著 `--remote-debugging-pipe` | |

`--automation-pipe`、`--window` 之後是不認得的開關，錯誤訊息照 `parse_args` 的寫法說明換成什麼。`test.py scenarios --window`、`bench.py raster --window` 是工具自己的選項，名字不變，內部改給 machopo 新的開關。

**啟動 machopo 的程式**：

```python
# Python（Linux、macOS）：和 ChromePipe 一樣
to_machopo, to_browser = os.pipe()
from_browser, from_machopo = os.pipe()
subprocess.Popen(["sh", "-c", 'exec "$0" "$@" 3<&{} 4>&{}'.format(to_machopo, from_machopo),
                  "machopo", "--remote-debugging-pipe", "--headless"], pass_fds=(to_machopo, from_machopo))
```

```rust
// Rust：ipc 替它在子 process 裡把兩端放到 3、4（Windows 是可繼承的 handle 加上開關）
let mut command = Command::new(machopo);
command.args(["--remote-debugging-pipe", "--headless"]);
let (child, pipe) = ipc::platform::spawn_with_pipe(&mut command)?;
pipe.write_all(b"{\"id\":1,\"method\":\"Browser.getVersion\"}\0")?;
```

手動在 shell 裡試：`machopo --remote-debugging-pipe --headless 3<&0 4>&1`（把 stdin、stdout 接到 3、4，等於現在的 `--automation-pipe`）。

**machopo 這一側**：

- 開始時先拿 fd 3、4（Windows 是開關給的兩個 handle）：確認是 pipe 或 socket，設成 close-on-exec（Windows 是不可繼承），之後的 reader、writer 讀寫它們。沒有、或不是 pipe 時印一行錯誤（附上 `3<&0 4>&1` 的寫法）結束，不像 Chrome 照樣讀。
- stdin 不讀，stdout 不寫訊息；log 和 `console` 照舊在 stderr（Chrome 也是）。
- renderer 的 stdout 不再轉到 stderr，照 Chromium 繼承 Browser 的。

**使用的人看到的變化**：machopodriver、WPT、情境、bench 的工具用法不變（它們內部換了開關和接法）；直接用 `--automation-pipe` 的指令和程式要換成新的開關、改用 fd 3、4，和驅動 Chrome 的 `--remote-debugging-pipe` 一樣。

## 詳細設計

### 1. `ipc` 的兩個入口

`unsafe` 只在 `ipc` 和 `bindings`，這裡兩件事都要：

- **被啟動的一側**：`ipc::platform::inherited_pipe() -> Option<(File, File)>`（名字待定）：
  - Unix：`fstat` 確認 fd 3、4 是 FIFO 或 socket，`OwnedFd::from_raw_fd`（`unsafe`：這兩個號碼由啟動它的 process 交給它、只拿一次，和 `inherited()` 一樣用一個 `AtomicBool`），`fcntl_setfd(CLOEXEC)`。
  - Windows：解析 `--remote-debugging-io-pipes` 的兩個十進位 handle（讀的在前，Chrome 的順序），`GetFileType == FILE_TYPE_PIPE`，`OwnedHandle::from_raw_handle`（`unsafe`），`SetHandleInformation` 拿掉 `HANDLE_FLAG_INHERIT`：std 的 `Command` 在 Windows 上繼承所有可繼承的 handle，不拿掉 renderer 就會拿到（libuv 的程式啟動時也這樣處理繼承到的 C runtime fd）。開關怎麼傳到 `ipc` 見「待決定」第 1 條。Windows 上沒有這個開關時報錯結束，和 Unix 上沒有 fd 3、4 一樣。
  - 要在任何東西開檔案之前拿：fd 3、4 沒開時，之後開的檔案可能拿到 3、4，就分不出來。`main` 解析完開關、建 log 之前（log 寫 stderr、trace 才開檔案）。
- **啟動的一側**：`ipc::platform::spawn_with_pipe(&mut Command) -> io::Result<(Child, PipeEnds)>`，對應 Chromium 的 `LaunchOptions::fds_to_remap` 和 `handles_to_inherit`：
  - Unix：建兩對 pipe（close-on-exec），`pre_exec` 裡 `dup2` 到 3、4（`dup2` 的結果沒有 close-on-exec；源頭是 3 或 4 時先搬開，和 `linux/channel.rs` 處理 `FIRST_FREE_FD` 的方式一樣）。`pre_exec` 裡只做 async-signal-safe 的系統呼叫、不配置記憶體。
  - Windows：兩端設成可繼承、加上 `--remote-debugging-io-pipes=<讀>,<寫>`，照 `windows/channel.rs` 的 `spawn` 已經有的做法（一次啟動一個、啟動完立刻拿掉可繼承，兩者用同一把鎖），避免同時啟動的其他 child 也拿到。
  - machopodriver 和 `shell` 的測試用它；`driver` 多依賴 `ipc`（`ipc` 只依賴 `rustix`、`mach2`、`windows-sys` 這一層，沒有 V8）。

### 2. `shell`

- **開關**（`main.rs` 的 `parse_args`、help、usage）：`--remote-debugging-pipe` 取代 `--automation-pipe`，不收值（`=cbor` 這類報錯）；`--headless` 只在 `--remote-debugging-pipe` 時收，取代「沒有 `--window`」；`--automation-pipe`、`--window` 變成不認得的開關，加進 `main.rs` 測試裡「machopo 沒有的模式」那一份清單。`--scale`、`--ui-layout`、`--ui-theme`、`--host-resolver-rules`、`--additional-trust-anchors`、網址的規則照舊，條件換成新的開關。
- **模組**：`automation_pipe.rs` 改名 `devtools_pipe_handler.rs`（Chromium 的 `DevToolsPipeHandler`），和已經有的 `devtools_http_handler.rs` 成對；`spawn_reader`、`spawn_writer` 改收 `File`，`run` 和 `window.rs` 從 `main` 拿到 `inherited_pipe()` 的結果傳進去。`automation_session.rs`、`automation.rs` 這些不是傳輸的模組名字不動。
- 結束的條件照舊：fd 3 讀到結尾（Chrome 是 `CloseBrowserSoon`）、`Browser.close`、訊號。writer 寫不出去時照 `wait_for_writer` 的規則放掉。
- `lib.rs` 的 `renderer_launcher`：stdout 不再轉到 stderr。stdin 照 Chromium 的 `LaunchProcess` 接 null（照舊）。註解改寫。
- log 的註解拿掉「stdout 是 pipe 的」；`print_console` 的註解改成照 Chrome 寫 stderr。

### 3. 啟動 machopo 的程式

| 地方 | 改法 |
| --- | --- |
| `crates/driver/src/machopo.rs` | `--remote-debugging-pipe --headless`，`ipc::platform::spawn_with_pipe`；machopo 的 stdout、stderr 都繼承 driver 的 |
| `crates/shell/tests/it/automation_pipe.rs` 的 `Browser`、`renderer_process.rs`、`remote_debugging.rs` | 同上；直接啟動 machopo 的幾個測試（`automation_pipe.rs` 後面的幾個）共用一個 helper |
| `crates/shell/tests/scenarios.rs` 的 `--window` 那一路 | `--remote-debugging-pipe`（沒有 `--headless`），同一個 helper |
| `scripts/bench/common.py` | `MachopoPipe` 給 `--remote-debugging-pipe --headless`，和 `ChromePipe` 共用接 fd 3、4 的那一段（Windows 的 handle 和開關也共用） |
| `scripts/bench/raster/raster.py` | `--window` 時只給 `--remote-debugging-pipe`，否則加 `--headless`；接法用 `MachopoPipe` 那一段 |
| `scripts/bench/blink_perf/blink_perf_test.py` 這類檢查命令列的測試 | 照新的開關改預期 |
| `tests/wpt/wptrunner_machopo/` | 只改註解：它經過 machopodriver |

取樣器包著 machopo 時（`samply record`、`perf record`），fd 3、4 和 Windows 的可繼承 handle 要經過取樣器傳到 machopo：Linux 的 `perf` 和 macOS、Windows 的 samply 都是直接 exec 或繼承，照理會傳過去，由 bench 工具的測試確認（「測試」）。

文件裡寫 `--automation-pipe` 的地方照新的開關改：`docs/architecture.md`、`docs/limitations.md`、`docs/testing.md`、`docs/test-gaps.md`、`docs/benchmarks.md`、`crates/shell/README.md`、`crates/driver/README.md`、`tests/scenarios/README.md`，和程式碼的註解、`scripts/test.py` 的說明。

### 4. 輸出

- log：stderr（照舊，Chrome 也是）。
- `console` 和沒接住的例外：stderr（照舊；Chrome 開了 log 時也印在 stderr）。
- wasm guest 的 stdout 和 stderr：照舊，都當成 console 的訊息（`crates/dom_wasm/src/engine.rs` 的 `flush_output`），和 `console.log` 走同一條路寫 stderr（「已決定的事項」第 3 條）。網頁寫得出來的東西都不進 stdout。

### 測試

- **`ipc`**（`crates/ipc/tests/it/` 的新模組）：`spawn_with_pipe` 的 child 在 3、4 讀寫得到；child 拿了之後再啟動的 grandchild 沒有 fd 3、4（Unix：grandchild 對 3、4 `fcntl` 得到 `EBADF`；Windows：grandchild 用那兩個 handle 值 `GetFileType` 失敗）；3、4 沒開或不是 pipe 時拿不到。
- **`shell`**：現有的 pipe 的測試全部經過新的開關和接法，等於整套驗證。新增：沒有 fd 3、4 時 machopo 結束並印出原因；stdin 是一個開著、沒人寫的 pipe 時（Puppeteer 的樣子）照常運作，fd 3 關掉時結束；renderer 的 stdout 是 Browser 的 stdout、不是 stderr（網頁寫不進 stdout，所以比對 fd：Linux 上比對 `/proc/<pid>/fd/1`，renderer 的 pid 從 `SystemInfo.getProcessInfo` 拿；macOS 上沒有不用 `unsafe` 的比法，記進 `docs/test-gaps.md`）。`main.rs` 的開關的測試：`--headless` 沒有 `--remote-debugging-pipe` 時報錯，`--automation-pipe`、`--window` 是不認得的開關，`--remote-debugging-pipe=cbor` 報錯，`--scale` 的預設跟著 `--headless`。
- **mutation test**：拿掉 close-on-exec 或不可繼承、拿掉 `dup2`、拿掉 pipe 的檢查、renderer 的 stdout 改回轉走、`--headless` 的判斷反過來，各自有測試失敗。
- **其他**：`CLAUDE.md` 的規則裡改 machopodriver 和 pipe 時跑 `test.py wpt` 的一個小目錄；bench 工具的四份測試（`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py`、`census_test.py`）都跑，連同 `--sample`；`scenarios --window`（`crates/shell/tests/scenarios.rs` 的 `--window` 那一路換了開關和接法）在階段驗收時跑。
- **手動確認**：Playwright 的 `chromium.launch({ executablePath, ignoreAllDefaultArgs: true, args: ['--remote-debugging-pipe', '--headless'] })` 在 macOS 上連得上（`Browser.getVersion` 有回答），記下接下來卡在哪個命令，寫進 `details.md` 給 Playwright 的 RFC。不放進自動測試：多一個 Node 的依賴。
- **Windows**：使用者在自己的 Windows 裝置上跑 `ipc`、`shell`、`driver` 的測試和一次 `bench.py speedometer`。

### 分階段

1. **`ipc`**：兩個入口（Unix、Windows）和它們的測試。之後的段落才用到，可以先 merge（`main` 上多一個沒人用的 API，但有測試）。
2. **換過去**：`shell` 的開關、reader、writer 和 `main`、renderer 的 stdout、所有啟動的程式、文件和限制。一次換完，中間不留兩種開關和接法。
3. **Windows**：在使用者的裝置上跑第 1、2 段的測試和 bench，修平台上的問題。

## 缺點

- `ipc` 多兩處 `unsafe`（拿 fd、handle 和 `pre_exec` 裡的 `dup2`），`crates/ipc/soundness.md` 要多兩段論證。
- 手動驅動 machopo 從 `echo … | machopo --automation-pipe` 變成要寫 `--remote-debugging-pipe --headless 3<&0 4>&1`；Python 和 Rust 的程式多十幾行接 fd。
- `--automation-pipe` 寫在很多文件、註解和別人自己的指令裡，這次全部要改；預設變成有視窗，忘了加 `--headless` 的指令會開出一個在最上層、不理輸入的視窗。
- 取樣器、其他包著 machopo 的程式要把 fd 3、4 傳過去，以後加新的包裝工具時也要注意（Chrome 有同樣的問題）。
- 一次改完所有啟動的地方，這個 commit 會碰到 driver、shell 的測試和 Python 的工具。

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

- **Chromium**：`--remote-debugging-pipe` 讀 fd 3、寫 fd 4（`DevToolsAgentHostImpl::StartRemoteDebuggingPipeHandler` 的 `kReadFD`、`kWriteFD`）；Windows 上預設把它們當成 C runtime 的 fd，有 `--remote-debugging-io-pipes=<讀>,<寫>` 時改用那兩個 handle（`AdoptPipes` 確認是 pipe、`_open_osfhandle` 包成 fd），之後 `DevToolsPipeHandler` 一律用 `_get_osfhandle` 拿回 handle 讀寫。訊息是 NUL 結尾的 JSON（ASCIIZ，接收的 buffer 上限 100 MB）或 CBOR。讀到結尾時 Chrome 呼叫 `ChromeDevToolsManagerDelegate::CloseBrowserSoon`。有沒有視窗由 `--headless` 決定，和傳輸無關。子 process 由 `LaunchProcess` 啟動，只拿到 `fds_to_remap` 列出的 fd，pipe 不會漏給 renderer。**照做**：開關的名字和分工、fd 的號碼、Windows 的開關、NUL、結尾時結束、子 process 拿不到。**不照做**：fd 3、4 不是 pipe 時 machopo 報錯結束（Chrome 照樣讀、讀不到時斷線）；`--headless` 只在 pipe 時收；Windows 上的 C runtime 的 fd 和 CBOR 先不做。
- **Playwright 的其他瀏覽器**：WebKit 的 `--inspector-pipe`、Firefox 的 `-juggler-pipe` 也是 fd 3、4，Playwright 對三個瀏覽器用同一個 `PipeTransport(stdio[3], stdio[4])`。
- **chromedriver**：用 `--remote-debugging-port`（或 pipe）啟動 Chrome，Chrome 的 stdout、stderr 照常是 log。

## 已決定的事項

2026-11-26 和使用者逐條確認，同一天接受。

1. **開關照 Chrome 拆成 `--remote-debugging-pipe` 和 `--headless`（N3b）**（2026-11-26，使用者）：`--remote-debugging-pipe` 是經過 pipe 驅動（fd 3、4，連同現在只在 pipe 時有的 `Machopo` domain、測試的網路設定、不理平台的輸入、`about:blank`、淺色和橫式的 UI、記憶體裡的 profile），`--headless` 是沒有平台視窗、只在 pipe 時收；`--automation-pipe` 和 `--window` 拿掉。Linux、macOS 只有 fd 3、4 這一種接法，不留 stdin、stdout 的模式。不選 N1（留 `--automation-pipe`：開關和 Chrome 不同，Playwright 的 RFC 要再改一次）、N2（另留 stdio 模式：兩種接法要測，renderer 的 stdout 的例外留著，`3<&0 4>&1` 就做得到）、N3a（只改名：和 Chrome 的意思相反）、N3c（兩個名字都留：同一件事兩個名字）。
2. **Windows 只收 `--remote-debugging-io-pipes` 的 handle（W2）**（2026-11-26，使用者）：我們自己啟動 machopo 的程式（std 的 `Command`、Python 的 `subprocess`）填不了 `lpReserved2`，只能給 handle，經過 samply 也傳得下去；Node 給的 C runtime 的 fd 3、4 這份不做，Windows 上的 Playwright、Puppeteer 在傳輸這一層連不上，留到之後（「之後的可能」）。不選 W1（兩種都收：多一個入口、一處 `unsafe`，還要在 Windows 上確認 Rust 的程式拿得到 C runtime 的 fd，現在沒有使用者）、W3（只收 C runtime 的 fd：每個啟動的程式都要自己呼叫 `CreateProcessW`，samply 那一層可能把 `lpReserved2` 丟掉）。
3. **wasm guest 的 stdout 和 stderr 照舊當成 console 的訊息、寫 stderr（O1）**（2026-11-26，使用者）：網頁寫得出來的東西只有一條路，和 `console.log` 一致，照 Chrome 留在 log 裡（Chrome 的 `console` 開了 log 時印在 stderr，從不寫 stdout）；stdout 空出來是給 renderer 和除錯的。不選 O2（guest 的 stdout 寫 machopo 的 stdout：WASI 的本意，但網頁就能寫滿終端機、讓不讀 stdout 的 client 塞住 renderer，wasm 文件的 stdout 也進不了 console）。

## 待決定

1. **`inherited_pipe` 怎麼拿到 Windows 的開關**：`shell` 解析後傳值（看法），或 `ipc` 自己讀命令列。實作時決定。

## 之後的可能

- **Playwright 的 RFC**：Chrome 的其他開關、命令列上的網址、沒有 `--headless` 時視窗收不收平台的輸入、`--headless` 不配 pipe、`waitForDebuggerOnStart`、browser context、`Emulation`、`Fetch`、isolated world（RFC 0075 的路線第 4 項）。
- **Windows 上的 C runtime 的 fd 3、4**：Node（libuv）經過 `STARTUPINFO.lpReserved2` 給的，Windows 上的 Playwright、Puppeteer 要它；在 `ipc` 加一個入口，和 handle 一樣變成 `File`（`_get_osfhandle`，或自己讀 `GetStartupInfoW` 的 `lpReserved2`；`details.md` 的「Playwright 和 Puppeteer」），測試自己呼叫 `CreateProcessW` 填 `lpReserved2`。和 Playwright 的 RFC 一起做。
- **CBOR**：Chrome 的 `--remote-debugging-pipe=cbor`，Puppeteer 和 Playwright 都沒用，有 client 要時再做。
- **multi-process 之後的 DevTools 的 pipe**：拆出 network、GPU process 時，它們也照這份的規則拿不到 fd 3、4。
