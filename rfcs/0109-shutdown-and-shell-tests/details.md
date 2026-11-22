# RFC 0109 的細節：結束的流程，和 `shell` 的測試

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

兩個盤點（結束的流程、`shell` 的測試）做的時候把結果放在這裡。

## CI 上壞過的

2026-11-22 用 `gh run list --workflow CI --limit 60` 和每一次的 `--log-failed` 查的，涵蓋 2026-11-18 到 22：60 次裡 24 次成功、23 次失敗、12 次取消、1 次還在跑。CI 只有 Linux（Ubuntu，4 顆 CPU，沒有 GPU）。下面是 23 次失敗各壞了什麼；`0004` 是多 process 的那份 RFC 的 branch。

| 壞的測試 | crate | 次數 | 在哪裡 | 知道的原因 |
| --- | --- | --- | --- | --- |
| `devtools_css::sheets_coming_and_going_are_told` | `shell` | 15 | `main` 8 次（2026-11-19 到 21）、`0004` 7 次 | 沒有查。每次都在 `devtools_css.rs` 的同一行：加了 `srcdoc` 的 iframe 之後等 30 秒收不到 `CSS.styleSheetAdded`，留著沒讀的只有 `DOM.childNodeCountUpdated`，machopo 還在跑 |
| `process_channel::a_host_and_a_child_talk` | `ipc` | 3 | `0004` 2 次、`main` 1 次 | 測試自己的競爭：child 先回答、再說讀完了，下一筆送出時 host 的那塊共用記憶體還沒還回來，host 就多建一塊。`5136d5c` 改成每一筆都等。當時我以為是 tile 走共用記憶體那一步帶出來的，但它在那一步之前的 `main` 上就壞過 |
| `renderer_process` 的兩個 `about:crash` 測試 | `browser` | 2（其中 1 次同時壞了上面的 `devtools_css`） | `0004` | Linux 上 `abort()` 要等系統收完 core dump，通道才斷。改成先設成不留 core dump |
| `devtools_log::commands_machopo_cant_carry_out_are_warned_of`、`devtools_overlay::other_overlays_are_answered` | `shell` | 1（同一次） | `0004` | Browser 分段寫 stderr 的一行，renderer process 的一行插在中間。改成一次寫出 |
| `documents::history_through_the_browser` | `shell` | 1 | `0004` | 沒有查。`popstate` 和 `hashchange` 之後的兩個布林值順序相反；在測試的 process 裡跑 |
| `automation_pipe::the_system_info_tells_the_gpu_adapter` | `shell` | 1 | `main`（2026-11-18） | 沒有查 |
| （lint：測試編不過） | `browser` | 1 | `0004` | 不是不穩定：commit 之前沒有重跑 clippy |

更早的（不在這 60 次裡，從記錄來的）：2026-11-18 兩次 Linux 的不穩定（Viz 的 device 在全域變數裡、`exit` 時 lavapipe 還在忙；被放掉的 renderer 晚到的訊息落在重複使用的 frame sink 上），都修了；2026-11-10 兩次「machopo 沒有結束」，原因不明。

## 參考資料

- Chromium 的 `content/browser/browser_main_loop.cc`（結束的順序）、`content/child/child_thread_impl.cc`（child 在通道出錯時）：盤點時對照。
