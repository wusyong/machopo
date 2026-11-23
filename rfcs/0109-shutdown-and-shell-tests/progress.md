# RFC 0109 的進度：結束的流程，和 `shell` 的測試

開始日期：2026-11-22

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 盤點結束的流程 | 做完，待審查（只在 Linux 上量） | 2026-11-23 |
| 1. 審查 `shell` 的測試 | 做完，待審查 | 2026-11-23 |
| 2. 修 `shell` 的測試 | 等「待決定」的第 2 階段那幾項 | |
| 3. 定結束的規則 | 沒開始 | |
| 4. 照規則改 | 沒開始 | |
| 5. 驗收 | 沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 結束的流程：每一種跑法、情況、thread、全域變數、child process，和 Chromium 的對照 | `details.md` 的「結束的流程的盤點」；不改程式，所以沒有測試 |
| `shell` 的測試：每個模組的分類、壞過的每一個的原因和把握 | `details.md` 的「`shell` 的測試的審查」；不改程式，所以沒有測試 |

## 做過的檢查

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
