# RFC 0054 的細節

## Windows 上的重現

2026-10-18，Windows 11（10.0.26200），Chrome for Testing 152.0.7977.82，`main` 的 `faf2d039`。一個不放進 repo 的腳本照 `run_in_chrome` 的參數啟動 Chrome（`--headless=new`、新的 `--user-data-dir`、`--window-size`），另外加 `--enable-logging=stderr --v=0` 留下 log，經過 `speedometer.py` 的 `Server` 量 TodoMVC-JavaScript-ES5 一個 iteration，數 server 收到的請求。

| 情況 | 結果 |
| --- | --- |
| `target/chrome-for-testing/152.0.7977.82` 照原本的 ACL（從使用者目錄繼承，沒有 AppContainer 的 ACE），等 60 秒 | 3 次都沒有報告，server 收到 0 個請求 |
| 同上，等 240 秒 | 沒有報告，0 個請求：network service 重啟一次之後就沒有動靜，不會自己恢復 |
| 加上 `*S-1-15-2-1:(OI)(CI)(RX)`、`*S-1-15-2-2:(OI)(CI)(RX)` | 5 次都在 1 秒內收到報告，每次 26 個請求 |

沒有權限時，log 的前兩行：

```
ERROR:sandbox\policy\win\sandbox_win.cc:804] Sandbox cannot access executable ...\chrome-win64\chrome.exe. Check filesystem permissions are valid. See https://bit.ly/31yqMJR.
ERROR:content\browser\network_service_instance_impl.cc:650] Network service crashed or was terminated, restarting service.
```

加了權限之後，`chrome.exe` 的 ACL 多了兩條繼承來的 `(I)(RX)`。`C:\Program Files` 預設的 ACL 就有這兩個群組（`(RX)`，和給子項目的 `(OI)(CI)(IO)(GR,GE)`），所以裝在那裡的 Chrome 不會碰到這個問題。試完之後權限撤回原狀。

RFC 0012 的 `progress.md` 記著 2026-10-12「同一天稍早的盤點不用改就跑完」、2026-10-16「剛下載完 Chrome 的第一次盤點跑完了，之後的又等不到報告」。上面等了 240 秒也不會恢復，所以那幾次成功不是因為盤點等得比較久（600 秒）；當時資料夾的 ACL 是什麼樣子已經查不到，這次沒有重現。加了權限之後沒有失敗過。

## pipe 的試驗

2026-10-18，同一台機器。`chrome.dll` 裡有 `remote-debugging-pipe`、`remote-debugging-io-pipes`、`remote-debugging-port` 這幾個開關。一個不放進 repo 的腳本：

- `os.pipe()` 開兩對 pipe，`msvcrt.get_osfhandle` 拿到 Chrome 要讀的一端和要寫的一端的 handle，`os.set_handle_inheritable` 設成可繼承，`STARTUPINFO.lpAttributeList = {"handle_list": [讀, 寫]}`、`close_fds=True` 只讓 Chrome 繼承這兩個。
- 送 `{"id": 1, "method": "Browser.getVersion"}` 加 NUL，等回應；再送 `Browser.close`，等 Chrome 結束。

| 開關 | 結果 |
| --- | --- |
| 只有 `--remote-debugging-io-pipes=<讀>,<寫>` | 沒有回應，`Browser.close` 也沒有作用，最後殺掉 |
| 只有 `--remote-debugging-io-pipes=<寫>,<讀>`（順序反過來） | 同上 |
| `--remote-debugging-pipe --remote-debugging-io-pipes=<讀>,<寫>` | `Browser.getVersion` 回了 `Chrome/152.0.7977.82`；`Browser.close` 之後 0.1 秒結束，exit 0 |

所以兩個開關要一起給，順序是 Chrome 讀的在前、寫的在後。這幾次 Chrome 的資料夾沒有權限（network service 照樣 crash），CDP 的連線不受影響：它在 browser process 裡。

## `Tracing` 的流程

照 Chrome DevTools Protocol 的 `Tracing` 和 `IO` domain（tip-of-tree 的文件；實作時對照 Chrome 152 的行為）：

1. `Tracing.start`，參數：
   - `traceConfig`：`{"includedCategories": [...], "excludedCategories": ["*"]}`，分類照 `CHROME_TRACE_CATEGORIES`（`toplevel`、`blink`、`v8`、`cc`、`viz`、`benchmark`、`loading`、`devtools.timeline`）；`recordMode` 預設 `recordUntilFull`，需要時改 `recordContinuously`；`traceBufferSizeInKb` 見 README 的「待決定」第 2 條。
   - `transferMode: "ReturnAsStream"`、`streamFormat: "proto"`：結束時拿到一個 stream，內容是 Perfetto 的 protobuf。
2. `Tracing.end`：回應是空的，之後送來 `Tracing.tracingComplete` 事件，帶著 `stream`（handle）、`dataLossOccurred`、`traceFormat`。
3. `IO.read`（`handle`，可以給 `size`）一直讀到 `eof: true`；`base64Encoded` 是 true 時先解碼。最後 `IO.close`。

在 browser 的連線上（沒有 `sessionId`）送，錄的是整個瀏覽器的所有 process，和 `--trace-startup` 一樣。

## 頁面的 focus

2026-10-18，同一台機器。照新的做法量 20 個 suite 時 Editor-TipTap 無效：`Editor-TipTap/Long` 的摘要裡 machopo 有 1 個元素帶 `ProseMirror-focused`，Chrome 0 個；2026-10-17 用命令列開網址量的時候兩邊一致。一個不放進 repo 的腳本用 `measure.mjs` 跑 TipTap 一個 iteration，數每個 test 的摘要裡帶 `ProseMirror-focused` 的元素：

| 開法 | `Long`、`Highlight` |
| --- | --- |
| 網址放在命令列（沒有 pipe） | 1、1 |
| 網址放在命令列，接著 pipe 但什麼都不送 | 1、1 |
| `about:blank`，attach、`Page.navigate`（session 留著） | 0、0 |
| `about:blank`，`Target.activateTarget`，再 attach、`Page.navigate`、detach | 0、0 |
| `about:blank`，attach、`Page.bringToFront`、`Page.navigate`、detach | 1、1 |
| `about:blank`，`Target.createTarget` 開新的分頁 | 1、1 |

所以是 headless 的第一個分頁要被叫到前面才有 focus，和 pipe、session 留不留著無關。工具改成導覽前 `Page.bringToFront`；`speedometer_test.py` 的 `test_the_page_has_focus`（machopo 和 Chrome 都跑）載入一個回報 `document.hasFocus()` 的頁面。
