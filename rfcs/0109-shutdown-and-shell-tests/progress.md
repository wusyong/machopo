# RFC 0109 的進度：結束的流程（graceful shutdown）

開始日期：2026-11-22。完成：2026-11-25，Linux（Fedora 45、24 個邏輯 CPU、AMD 的 Mesa；debug build）；macOS 和 Windows 沒有跑，Windows 的 `ipc` 只對 `x86_64-pc-windows-msvc` 編過。目標第 6 條（重複跑不壞）照已決定的事項第 6 條看 merge 之後的 CI。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 盤點結束的流程 | 做完（只在 Linux 上量） | 2026-11-23 |
| 1. 審查 `shell` 的測試；2026-11-24 重新評估，範圍改成以結束為主（第 3 條） | 做完 | 2026-11-24 |
| 2. 收掉 `shell` 測試的尾巴：B3 到 B8、C1 和 C2、S6（第 4 到 7 條） | 做完 | 2026-11-24 |
| 3. 重量、定結束的規則（第 8 到 17 條） | 做完 | 2026-11-24 |
| 4. 照規則改：4a 收尾的路和記一行、4b renderer 共用 2 秒、4c 打斷 script、4d 訊號、4e pipe 的 writer、4f Viz、4g renderer 的 stdio（第 18、19 條） | 做完 | 2026-11-25 |
| 5. 驗收 | 做完（CI 看 merge 之後） | 2026-11-25 |

## 驗收

`shell` 的測試是 `cargo test -p shell --test it <名稱>`。只能靠審查的在 `docs/test-gaps.md`。

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 一定結束 | automation pipe：卡住的 renderer process 不管幾個，2 秒的期限只等一次，到了 kill、記一行（原本一個 5 秒） | `automation_pipe::hung_renderers_take_the_limit_once` |
| 一定結束 | `--single-process`：卡住的 renderer 的 script 被打斷，`setInterval` 一直重新進入的也是，自己結束、不用放棄（原本不結束） | `automation_pipe::a_single_processs_hung_renderers_have_their_script_stopped` |
| 一定結束 | headless：頁面進入無窮迴圈，測試照樣結束 | `script::a_page_that_never_returns_ends_its_test` |
| 一定結束 | machopodriver：頁面卡住時刪 session，在它的 5 秒之前回來 | `webdriver::a_session_with_a_hung_page_ends_in_time` |
| 一定結束 | `Browser.close` 遇上卡住的 renderer，回答照樣送到、writer 不被放掉 | `automation_pipe::a_hung_renderer_leaves_the_writer_its_time` |
| 一定結束 | client 不讀 stdout：writer 被放掉、記一行（原本不結束） | `automation_pipe::a_client_not_reading_doesnt_hold_machopo_up` |
| 一定結束 | SIGTERM、SIGINT、SIGHUP 照關掉結束；第二次立刻結束（143）、記一行；`nohup` 的 SIGHUP 照舊忽略 | `automation_pipe::a_signal_ends_machopo_as_closing_it_does`、`a_second_signal_ends_machopo_at_once`、`a_signal_started_ignored_stays_so` |
| 一定結束 | 視窗：和 automation pipe 同一段收尾；`test.py scenarios --window` 跑過正常的關閉 | 視窗自己的那幾行（`App::exit`、`ShellEvent::Signal`、`ShellEvent::GpuFailed`）靠審查 |
| 一定結束 | Viz 的 device 重建不了：只說一次、不再畫、不拷貝、不改大小 | `viz` 的 `a_renderer_that_cant_draw_says_so_once_and_draws_no_more`；到結束碼 1 的那一半靠審查 |
| 一定結束 | 每一種情況量過，2.07 秒內 | `details.md` 的「驗收時的重量」 |
| 不留東西 | 卡住的 renderer 被 kill 之後都不在了；renderer 不拿著 pipe（stdin 是 `/dev/null`，stdout、stderr 是 Browser 的 stderr） | `automation_pipe::hung_renderers_take_the_limit_once`、`a_renderer_process_hasnt_the_pipe` |
| 被 kill 也乾淨 | Browser 被 kill 時 renderer 跟著結束（等它們的 pid 不在） | `automation_pipe::a_killed_browsers_*`（四個） |
| 順序、不 crash | 啟動到一半失敗時照樣 shutdown，以結束碼 1 結束、說明原因（30 次） | `remote_debugging::a_port_taken_ends_it_with_an_error`；上面每個測試都檢查結束碼 |
| 寫完 | 正常結束時 cookie、`localStorage`、HTTP cache 寫完，沒有放棄的那一行；SIGTERM 之後也是 | `automation_pipe::a_profile_is_written_as_machopo_ends`、`a_signal_ends_machopo_as_closing_it_does` |
| 寫完 | Browser 和 renderer process 的 trace 寫完 | `trace::`（`mutants-4a.toml` 的「trace 的結尾沒寫」） |
| 寫完 | profile 的資料庫關過、等不到的 thread，丟掉時不再等 | `net` 的 `database_thread::a_closed_thread_given_up_on_is_let_go`、`browser` 的 `dom_storage` 的同名測試 |
| `shell` 的測試 | B3 到 B8 守它們本來要守的事 | `mutants-2.toml`（每個一條） |
| `shell` 的測試 | 「拒絕連線」的 port 整個測試期間占著 | `net` 的測試（mutant「guard 改成 listen」） |
| 盤點 | 每一種跑法、情況、thread、全域變數、child process，和 Chromium 的對照 | `details.md` 的「結束的流程的盤點」 |

## 做過的檢查

### 第 4、5 段（2026-11-25，`9109d9f` 到 `d54121b`）

**mutation test**（每一步一份 spec，都在這個資料夾）：4a 11 個抓到 8 個，4b 4 個、4c 5 個、4d 6 個、4e 3 個、4f 5 個、4g 2 個、第 5 段 1 個都抓到。沒抓到和第一輪沒抓到的：

- 4a「視窗收尾時不關 profile」：等價。回到 `main` 之前每份 `StoragePartition` 的 handle 都已經被丟掉，store 的 `Drop` 照樣寫完。查的時候發現 `close` 等不到放棄之後，`Drop` 的 `join` 照樣永遠等、時限沒有作用；補上「放掉」和它的三個 mutant，都抓到。
- 4a「結束前寫 census」「結束前 flush stdout」：等價（計數的 thread 在 process 結束之前就被 join、自己寫了；pipe 的 writer 每則都 flush），兩段程式拿掉，`details.md` 的盤點更正。
- 4e「沒有寬限」靠時機（第一次檢查時 writer 還沒做完），單獨再跑三次，四次都抓到。
- 4f「畫不下去之後照樣畫」第一輪沒抓到：測試只經過要拷貝的那條路畫，那條路有自己的檢查；補上一次不拷貝的畫（畫了 texture 會被上傳），抓到。

**第一版就錯的測試**：4c 的 headless 測試的頁面在 `load` 傳到 Browser 之前就進入迴圈，headless 等不到載入、等滿自己的 30 秒：改成載入之後 200 ms 才進入迴圈。

**改寫的測試**：`renderers_go_with_a_killed_browser` 原本等 pipe 讀到結尾判斷 renderer 有沒有跟著結束，4g 之後 renderer 不拿著 stdout，那個判斷沒有作用了：改成 kill 之前用 `SystemInfo.getProcessInfo` 拿 pid，等它們都不在（Unix 的 `kill -0`、Windows 的 `tasklist`）。

**順帶修的**：`scripts/bench/speedometer/speedometer_test.py` 的 `test_traces_of_both` 還在找 RFC 0112 第 5 段拿掉的 `RenderThread::between_tasks`（工具的測試不在 CI 裡），改成檢查 `RenderThread::run_platform_tasks` 在一個 task 裡。`docs/limitations.md` 裡 RFC 0110 已經修掉的「renderer 忙的時候，有些事晚一輪才做」（A1 到 A5，各有必壞的測試）和第 2 段修掉的「啟動到一半失敗時 crash」刪掉。

**階段結束的檢查**：`cargo test --workspace` 通過（60 個 binary，4855 個通過、1 個 ignored）；`cargo fmt` 改了第 4 段的 10 個檔案的排版；clippy 要求的三類（測試的計數器、`MainThreadTerminator` 的型別、`Instant` 加上時限的五處）改好之後通過；`sanity.py` 的七項通過。量測工具的四組測試（`census`、`raster`、`blink_perf`、`speedometer`）在 4a、4c、4g 之後各跑一次，通過。`test.py scenarios`（沒有平台視窗）98 個通過。

**WPT**（automation pipe 改了）：`test.py wpt html/webappapis/timers` 25 個測試，1 個非預期的通過（`negative-settimeout.any.worker.html`，期望值是初始 commit 的 `ERROR`），三次都一樣；`main` 2026-11-24 的 Nightly 上也是 `Test OK, expected ERROR`，不是這份造成的，期望值沒有改。那次 Nightly 的 `wpt (2)`、`wpt (3)`、`wpt (4)`、`wdspec` 都有非預期的結果（`wpt (3)` 的 67 個都是通過），不在這份處理。

**真的視窗**（`test.py scenarios --window`，4d、4e、4f 改了視窗那一層）：74 個裡 6 個失敗。`intersection-observer`、`resize-observer`、`resize`、`scroll-anchoring` 是 `docs/limitations.md` 記著的 Linux 真視窗的大小問題；`fixed-into-view`（捲到 1500、要 1600）和 `frame-input`（點在 `49.75 20.5`、要 `50 20`）是新的，也是大小或縮放的那一類：`fixed-into-view` 是 2026-11-22 才加的、之前沒在 Linux 的真視窗上跑過；merge 之後（使用者問起）才在這份之前的 `ec069536` 上確認：兩邊各跑三次，兩個都每次失敗、數字一樣，所以不是這份造成的。這個確認應該在驗收之前做，不是只寫「推論是同一類」。之後查出六份的原因、修掉（另一個 branch `force-device-scale-factor`，`mutants-window-scale.toml`）：螢幕的縮放是 1.333，情境照整數的縮放寫，所以真視窗的情境改用情境自己的縮放（`machopo --scale` 用在視窗上，預設 1）；`geometry` 的 `scale=2` 讓第一個視窗比螢幕大而被放到最大，所以第一個視窗照螢幕的邏輯 px 要；Wayland 的 compositor 蓋掉一開始要的大小（`position-fixed`、`resize-observer` 時過時不過），所以被蓋掉時再要一次；`fixed-into-view` 量的是一行字的 box，系統的字型的 ascent 讓它高出容器 1 px、捲到 1599，改成量容器本身。之後 74 份兩輪都通過。

**重量**：第 3 段的量法、同一份腳本，每一格三次（`details.md` 的「驗收時的重量」）。

### 第 2 段（2026-11-24，Linux：Fedora 45，debug build，`ec06953` 之上）

**B3 到 B8 改寫**：六個各跑一次都通過。mutation test（`mutants-2.toml`）每個配一條，弄壞它守的行為：idle period 沒有時間、長的 idle period 超過 50 ms、inspector 沒聽到 thread 閒著、`about:blank` 沒有 initiator 的 origin、動畫的事件不照時間排、同時的不照 transition 的順序排。第一輪「長的 idle period 超過 50 ms」沒抓到：原本的測試也抓不到，它的第一個 idle callback 落在載入期間，期限被下一個 frame 截短；`idle_callbacks` 加一段在畫面靜下來之後要一個 idle callback（重試到剩下的時間超過一個 frame），重跑抓到。B4 的 sampling 的間隔是 V8 的 inspector 自己的，machopo 這一側沒有可以弄壞的，它守的轉送由 B5 的那條抓到。

**C1、C2**：`refused_port()` 的 guard 只 bind、不 listen；Linux 上連到它馬上被拒絕（`net` 279 個、`browser` 229 個、`shell` 用到它的 62 個測試都通過）。mutant「guard 改成 listen」被 `net` 的測試抓到；同時跑的別的測試拿不到同一個 port 這一點，一個測試裡看不出來（`docs/test-gaps.md`）。

**S6**：先對了 Chromium（`BrowserMainRunnerImpl` 的解構子在還沒 shutdown 時呼叫 `Shutdown()`），`BrowserWindow` 照做加 `Drop`。crash 的機率比盤點時低很多，直接啟動 `machopo --automation-pipe --remote-debugging-port=<占著的>` 數結束的方式（CPU 的 adapter）：

| | 次數 | crash（SIGSEGV、SIGABRT） |
| --- | --- | --- |
| 沒有修正，預設的 GPU | 200 | 0 |
| 沒有修正，CPU 的 adapter | 200 + 1000 | 3 + 1 |
| 沒有修正，CPU 的 adapter，綁 4 顆 CPU 加兩個忙碌的 process | 200 | 1 |
| 有修正，CPU 的 adapter | 1000 | 0 |
| 有修正，CPU 的 adapter，綁 4 顆 CPU 加負載 | 200 | 0 |

沒有修正 1600 次裡 5 次（約 0.3%），有修正 1200 次 0 次；修正沒有作用的話 1200 次都不 crash 的機率約 2%。盤點時（`647ab5a`）是約一半，差這麼多的原因沒有查。必壞的測試 `remote_debugging::a_port_taken_ends_it_with_an_error`（CPU 的 adapter，啟動 30 次，每次要以結束碼 1 結束、說明原因）在原本的程式上第一次跑就壞了（被訊號結束），之後的 5 遍沒有再壞；mutant「沒有 shutdown 就丟掉時不 shutdown」沒抓到，記進 `docs/test-gaps.md`。

**穩定性**：照第 6 條原本的做法開始跑，第一個（`documents::idle_callbacks`）在 4 顆 CPU、lavapipe、加負載下 1000 次 0 失敗（298 s），之後使用者改成看 CI，停掉了。

**階段結束的檢查**：`cargo test --workspace` 通過（60 個 binary，4841 個通過、1 個 ignored）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。

### 重新評估（2026-11-24，Linux：Fedora 45，`main` 的 `4e5e0ef`）

沒有改程式。對照程式確認 S1 到 S9、B3 到 B8、C1 和 C2 都還在（`details.md` 的「2026-11-24 的重新評估」）；`gh run list` 看 0110 merge 之後的 9 次 CI（1 次失敗，是 B1）。B7、B8 的頁面在 Chrome for Testing 152 各跑 5 次（經過 `scripts/bench` 的 server 和 `run_in_chrome`，`testRunner.reportHarnessResults` 換成 `POST`）：B7 5 次都是 `finish a finish b remove a removed 50 frame 0.4 0.2 0.2`，B8 5 次都是 `run b / start b / end b / run a / start a / end a`。

### 第 0、1 階段（2026-11-22 到 23，Linux：Fedora 45、24 個邏輯 CPU、lavapipe、debug build，`647ab5a`）

沒有改產品和測試的程式，所以沒有 mutation test，也沒有跑 workspace 的測試、fmt 和 clippy。下面「修正」都是在另一份複本上做的實驗，沒有進這個 branch。

**整組重複跑**（4 顆 CPU、五份同時、跳過已知的 `devtools_css` 那一個）：453 次，138 次有失敗，沒有一次卡住。

| 條件 | 次數 | 有失敗的 |
| --- | --- | --- |
| 平常（三份） | 91、91、83 | 5、7、11 |
| 加兩個忙碌的 process | 65 | 28 |
| 10 條測試 thread | 123 | 87 |

各測試壞的次數（453 次裡）：`the_profile_tells_waiting_from_rendering` 64、`requests_are_told_with_their_responses_and_bodies` 44、`the_profiler_samples_the_pages_script` 24、`idle_callbacks` 17、`redirects_and_failures_are_told` 10、`a_document_is_drawn_once_it_has_a_body` 4、`a_draw_asks_for_a_frame` 3、`about_blank_has_its_initiators_origin` 3、`frames_are_switched_to_and_kept_through_their_navigations` 2、`subresources_are_listed_with_their_content` 2、`a_frames_events_go_by_when` 2、`what_a_change_brings` 1、`an_image_set_shows_and_loads_the_image_for_the_scale` 1、`a_replaced_animation_is_removed` 1。

**`devtools_css::sheets_coming_and_going_are_told`**：634 次壞 58 次（單獨、釘一顆 CPU：367 次 10 次；單獨、兩個 thread：56 次 18 次；單獨加負載：128 次 14 次；整個模組：60 次 10 次；整組：23 次 6 次）。

**修正的實驗**：

| 原因（`details.md` 的編號） | 修之前 | 修之後 |
| --- | --- | --- |
| A1，原本的測試 | 634 次壞 58 次 | 2480 次壞 0 次 |
| A1，放大的測試（srcdoc 裡忙等 150 ms） | 三輪：4 次壞 4 次、9 次壞 5 次、5 次壞 5 次 | 933 次壞 0 次 |
| A2、A3，原本的兩個測試 | 453 次壞 10、44 次 | 1323 次壞 0 次 |
| A2、A3，必壞的測試（`fetch()` 之後忙等 200 ms） | A2、A3 各 120 次壞 120 次 | 700 次壞 0 次 |
| A4，原本的測試，加負載 | CI 上 1 次 | 951 次壞 0 次 |
| A4，放大（每個 task 之後睡 5 ms） | 6 次壞 6 次 | 300 次壞 0 次 |
| S6，port 被占用時啟動 | 300 次：SIGSEGV 99、SIGABRT 48、正常的代碼 1 有 153 | 300 次都是代碼 1 |

**「每個 task 之後就做收尾」的第一次實驗**（做了四件：`handle_frame_requests`、`run_child_commits`、`run_intersection_task`、`tell_frames`，比後來寫的選項 A 多兩件）：A1 的必壞測試 120 次壞 0 次（不開：9 次壞 5 次）、A2 120 次壞 0 次（不開：120 次壞 120 次）、A4 120 次壞 0 次（不開：6 次壞 6 次）、A5 120 次壞 0 次（不開：120 次壞 120 次）、A6 開和不開都是 5 次壞 5 次（放大器造成的逾時，驗不出來）。開著時整組測試 5 次都通過（12 顆 CPU，不是 CI 的 4 顆）。成本沒有量。

**選項 A 只做兩件的實驗**（2026-11-23，同一個 commit 的另一份複本）：

| 測試（都放大過） | 不改 | 兩件（選項 A） | 四件 |
| --- | --- | --- | --- |
| A1 | 5 次壞 5 次 | 120 次壞 0 次 | 120 次壞 0 次 |
| A2 | 120 次壞 120 次 | 120 次壞 0 次 | 120 次壞 0 次 |
| A4 | 5 次壞 5 次 | 120 次壞 0 次 | 120 次壞 0 次 |
| A5 | 120 次壞 120 次 | 120 次壞 0 次 | 120 次壞 0 次 |
| A6 | 5 次壞 5 次 | 5 次壞 5 次（驗不出來） | 5 次壞 5 次（驗不出來） |
| 整組，4 顆 CPU、六份同時、沒有另外加負載 | 34 次壞 7 次 | 34 次壞 0 次 | 32 次壞 2 次（C1、B3） |

整組花的時間（中位數）：這台機器的核心有快有慢，同一組核心比，不改和兩件是快的 42.7、43.4 秒，慢的 48.0、48.1 秒。兩輪不是同時量的，只夠說沒有明顯變慢，不是每個 task 的成本。

**選項 A 和 C 對照 Chromium**（2026-11-23；C 只做了「當下捲動」的原型；Chromium 是本機的 156.0.8078.79，headless，各 5 次結果都相同；machopo 每格 60 次結果都相同）：

| 設 `location.hash` 之後 | Chromium | 不改 | A | C 的原型 |
| --- | --- | --- | --- | --- |
| 同一個 task 讀到捲過了 | 是 | 否 | 否 | 是 |
| 下一個 timer 讀到捲過了 | 是 | 否 | 否 | 是 |
| 同一次 rendering 的下一個 `requestAnimationFrame` 讀到捲過了 | 是 | 否 | 否 | 是 |
| idle callback 之後的 frame 讀到捲過了 | 是 | 否 | 否 | 是 |
| 馬上 `scrollTo(0, 0)`，三個 frame 之後還在最上面 | 否 | 否 | 否 | 否 |

- 每個 task 之後只清空 `FrameRequest`、不更新 DevTools 的清單：A1 放大的測試 2 次壞 2 次，A2 的 60 次壞 0 次。
- 整組：C 的原型 5 次壞 1 次（A1，原型沒有修它），C 的原型加 A 5 次壞 0 次。WPT 沒有跑。
- **更正 A4 的放大實驗**：不改時壞的多半是逾時（第一次 6 次裡 5 次，後來 5 次都是），不是捲動的斷言。推論是佇列一直不空、`history.back()` 的要求沒有被處理；C 的原型沒有搬那一個，所以照樣逾時（2 次壞 2 次）。導覽改成呼叫的當下送出也只救得了它：內圈只取通道上的 task，網頁自己的 timer 和 `postMessage` 在外圈，放大器一直往通道塞 task 時，外圈的所有工作（timer、parser 的續跑）都一樣晚（2026-11-23 讀 `RenderThread::run` 看到的，推論）。捲動本身以上表為準。

**其他原因的實驗**：

- B1：單獨加負載 388 次壞 16 次；追蹤看到兩次都畫在 body 之後。
- B4：CPU 被搶時 40 次壞 40 次，平常 40 次壞 0 次。
- B6：放大時 13 次壞 8 次；parser 的預算設成 20 秒時 751 次壞 0 次。預算設成 0 時 1452 次也壞 0 次，和「照時間讓出」的簡單解釋不完全相符，所以只算「對得上」。
- B7：單獨跑、十二種條件合計 5695 次壞 1 次。B8：十二種條件合計 4418 次沒有重現。
- D1：還原 `29a94c7` 之後 `the_system_info_tells_the_gpu_adapter` 401 次壞 195 次；啟動後 0.2 秒內結束 300 次 crash 9 次。同一個還原下 `the_window_has_bounds_to_set` 76 次沒有一次不結束（2026-11-10 的現象沒有重現）。

**結束的量測**：每一格 3 次（結果在 `details.md`）；安靜的機器上抽七格各 5 次，一致。正常結束時 profile 在一顆 CPU 上 100 次都寫完。啟動後馬上或 0.2 秒內結束，兩種 process 的模式各 800 次，代碼都是 0。量完確認沒有留下 process。

**作廢的一輪**：A1 的修正第一次驗證時，跑到一半重編了同一份複本，後段測的是沒有修正的程式。沒有採用，換成不再重編的執行檔重跑（上表的數字）。

### 順帶找到的：別的 origin 的 `javascript:` 網址（2026-11-23）

查「延後的導覽會不會套到換上的新文件」時發現：別的 origin 的頁面把子 frame 的 `location` 設成 `javascript:` 網址，script 就在子 frame 的文件裡執行，DOM 和 renderer 都沒有比對 origin。和這份 RFC 的主題無關，在另一個 branch（`javascript-url-origin`，從 `d2105e5`）單獨修了，commit `97262a6`，2026-11-23 merge 回 `main` 並 push。

**驗證**（`647ab5a` 的複本，shell 的整合測試；Chrome 是本機的 156.0.8078.79，headless，兩個 origin 是 127.0.0.1 的兩個 port；兩邊各 5 次，每格的結果都相同）：

| 情況 | machopo | Chrome |
| --- | --- | --- |
| 父頁面把別的 origin 的子 frame 設成 `javascript:` | 在子 frame 裡執行 | `SecurityError` |
| 同上，同 origin 的子 frame（對照） | 執行 | 執行 |
| 子 frame 剛導覽到別的 origin 之後才設 | 在新的子 frame 裡執行 | `SecurityError` |
| 子 frame 導覽到同 origin 的另一頁之後才設 | 在新的那一頁執行 | 在新的那一頁執行 |
| 子 frame 的導覽還沒有回應時設（回應延後 300 ms） | 在舊文件裡執行 | 什麼都沒執行 |
| 頂層頁面導覽中，設自己的 `javascript:` | 什麼都沒執行 | 什麼都沒執行 |

- 「延後的導覽套到新文件」沒有重現：子 frame 的導覽回應先經過 `did_receive_resource`，它先處理 `FrameRequest`，舊的要求在 commit 之前就做了；頂層那一條丟掉了它，和 Chrome 一樣，丟在哪裡沒有查。其他 commit 的路（srcdoc、`about:blank`、同一份文件的上一頁）沒有試。
- 子 frame 導覽中時 machopo 在舊文件裡執行、Chrome 不執行，是還沒記進 `docs/limitations.md` 的差異（2-1 改寫時一起記）。

**修正**：HTML 的 navigate 檢查 `javascript:` 網址的發起者和目標同 origin（Blink 的 `LocalFrame::CanNavigate`，對過 152.0.7977.82 的 `local_frame.cc`、`frame_tree.cc`、`location.cc`）：連結什麼都不做，`location` 的 `href` setter、`assign()`、`replace()` 照 Chrome 丟 `SecurityError`（HTML 是什麼都不做；使用者 2026-11-23 確認照 Chrome）。

**Mutation test**（`mutants-javascript-url.toml`，在那個 branch 上跑；這個 branch 在 `97262a6` 之前，要在 `main` 上重跑）：7 條都抓到。拿掉整個檢查（`dom_js` 的測試，和 `shell` 從頭到尾的那一個各一條）、連結不檢查、`href` 的 setter 不丟、`replace()` 不丟、realm 找不到它的文件、連結的發起者當成目標。

**其他**：`cargo test --workspace`、`cargo fmt --check`、clippy、`sanity.py` 的六項都通過。WPT 跑了 `navigating-across-documents`、`the-location-interface`、`links-created-by-a-and-area-elements`：有修正的 4 次裡 1 次多一個非預期的通過（`replace-before-load/location-setter-user-mouseup.html` 的 `search`），沒有修正的 2 次和單獨跑的 3 次都沒有。它靠 `/common/slow.py` 拖住 load 事件，機器忙時點擊送到之前文件可能已經載入完成、不再 replace；決定這件事的 `is_completely_loaded` 沒有改，所以判斷是原本就和時序有關（推論，沒有逐次追蹤）。`javascript-url-security-check-*` 三份照樣是預期的失敗：`location.href` 的幾項是因為丟了例外，其他的要 `document.domain`。
