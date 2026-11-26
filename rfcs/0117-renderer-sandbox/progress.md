# RFC 0117 的進度：renderer process 關進作業系統的 sandbox

開始日期：2026-11-26

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、Linux 的第一次探測（`details.md` 的「Linux 的探測」）、第 1 段之前的九條決定 | 做完 | 2026-11-26 |
| 1a. 補探測：wasm 的文件（wasmtime 的 JIT）、`MACHOPO_TRACE`、`about:crash` 和 panic 的路；`strace -k` 找出零星的讀取是誰讀的；fontconfig 的 cache 過期、寫不了時怎麼辦；發行版和 CI 的 kernel 有沒有 Landlock。之後定第 1 段的細節（README 的「待決定」） | 沒開始 | |
| 1b. 新的 crate `sunaba`：Linux 的 Landlock 規則和 seccomp 的清單、`SIGSYS` 的 handler、最低要求的檢查；輔助的執行檔和「做不到」的測試；`soundness.md`；`unsafe` 的規則和 `sanity.py unsafe` 加上它 | 沒開始 | |
| 1c. 接上 machopo：child 的 `main` 一開始進 sandbox；Browser 問 fontconfig 的路徑、啟動時交給 renderer；`--no-sandbox` 和警告；套不上時分頁說明原因；`shell` 的測試從 `/proc` 看真的 renderer | 沒開始 | |
| 1d. trace 和 census 的輸出檔由 Browser 開好、啟動時交給 renderer；工具拿掉暫時的 `--no-sandbox` | 沒開始 | |
| 1e. 第 1 段的檢查和文件：整個 workspace 的測試、WPT、情境、Speedometer（使用者要求時）、`docs/limitations.md`、`docs/architecture.md`、CI | 沒開始 | |
| 2. macOS | 沒開始 | |
| 3. Windows | 沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

### 0：Linux 的第一次探測（2026-11-26，Linux：Fedora 45，kernel 7.3.3，debug build，`52dc5adc`）

`probe.py`（`strace -f` 底下的 `machopo --automation-pipe` 載入一頁），系統字型和 `--test-fonts` 各一次；結果在 `details.md` 的「Linux 的探測」。第一版的統計只算到 renderer 的 1 個 thread：`strace -f` 把被打斷的呼叫拆成 `<unfinished ...>` 和 `<... resumed>` 兩行，沒接起來就認不出 `clone3` 建的 thread；接起來之後是 22 個。
