# RFC 0054 的進度：Speedometer 的 Chrome 那一側經過 CDP，Windows 上也能量

開始日期：2026-10-18，在 `0054` 上做（Windows 11）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 CDP 和權限：共用收發訊息的類別、Chrome 走 pipe、`Browser.close`、Windows 上補權限 | 完成（和第 2 段一起做） | 2026-10-18 |
| 2 trace 改用 `Tracing.start` / `Tracing.end` | 完成 | 2026-10-18 |
| 3 Linux 和 macOS 上確認 | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| machopo 和 Chrome 共用的 CDP pipe：回應給等它的 `call`，給更早的 `call` 的回應不算；事件排著等 `event`，前面的丟掉；錯誤變成 `RuntimeError`；`Browser.close` 之後等 process 自己結束 | `python3 scripts/bench/speedometer/speedometer_test.py CdpPipeTests`（假的瀏覽器，照 NUL 的格式回應） |
| Chrome 經過 pipe 控制（Windows 的 `--remote-debugging-io-pipes`）：開頁面之後沒有 session 留在頁面上，`Browser.close` 之後自己結束（exit 0） | `python3 scripts/bench/speedometer/speedometer_test.py ChromeTests.test_the_page_is_left_alone_and_chrome_ends_on_its_own` |
| Windows 上 Chrome 的資料夾讓 app container 讀：沒有的補上、往下繼承，已經有的不再跑 `icacls` | `python3 scripts/bench/speedometer/speedometer_test.py ChromeTests.test_chromes_sandbox_may_read_its_folder` |
| Chrome 那一側的量測、盤點的紀錄在 Windows 上跑得起來 | `python3 scripts/bench/speedometer/speedometer_test.py ChromeTests MeasureTests.test_both_browsers_in_turn MachopoTests.test_machopo_and_chrome_summarize_alike` |
| 頁面和使用者的分頁一樣有 focus（Chrome 導覽前 `Page.bringToFront`） | `python3 scripts/bench/speedometer/speedometer_test.py ChromeTests.test_the_page_has_focus MachopoTests.test_the_page_has_focus` |
| Chrome 的 trace 用 `Tracing` 錄，Perfetto 的 protobuf，寫完才換上正式的檔名 | `python3 scripts/bench/speedometer/speedometer_test.py MeasureTests.test_traces_of_both` |

## 做過的檢查

- **第 1、2 段**（2026-10-18，Windows 11）：
  - 改之前（`main` 的 `c13b45c5`，Chrome 的資料夾照原本的 ACL）`speedometer_test.py` 56 個測試裡 14 個失敗、1 個 skip，1428 秒：Chrome 的 12 個都等不到報告（`ChromeTests` 10 個、`MachopoTests.test_machopo_and_chrome_summarize_alike`、`MeasureTests.test_both_browsers_in_turn`），`MeasureTests.test_traces_of_both` 拿不到 Chrome 的 trace，`SampleReadingTests.test_machopo_is_built_from_a_tree_into_a_target` 是另一個問題（下面）。
  - 改之後 60 個測試裡 58 個通過、1 個 skip（`--sample` 只在 Linux），只剩上面那個和 Chrome 無關的失敗；67 秒。
  - 新的測試：`CdpPipeTests` 的 2 個，`ChromeTests` 的 2 個（pipe 和結束、資料夾的權限），`MeasureTests.test_traces_of_both` 多檢查 `.part` 沒有留下。原本打算也用 `perfetto_slices` 和 `timed_windows` 讀 Chrome 的 trace，但那是照 machopo 的 trace 寫的（照 thread 的名字分組），Chrome 每個頁面有自己的 renderer、`CrRendererMain` 不只一個，開和關對不上；Chrome 的 trace 照舊用 trace processor 分析，這個檢查拿掉。
  - `ci.py speedometer --trace --browser chrome --suites React-Stockcharts-SVG,TodoMVC-jQuery`（各 10 個 iteration）：兩份 trace 都寫出來（7.7 MB、16 MB），沒有丟資料（「已決定的事項」第 6 條）。
  - mutation test（`python3 scripts/test.py mutate rfcs/0054-chrome-cdp/mutants-1.toml`，15 條，用 `--keep-going` 跑完全部：每條各自弄壞一處，互不相依）：第一次 14 條抓到，沒抓到「沒有往下繼承的授權也算數」（`inherited_read` 不看 `OI`、`CI`）：測試只用了完全沒有授權的資料夾。補上一個只有資料夾本身 `(RX)`、不往下繼承的資料夾，`inherited_read` 要說沒有、補過才有，之後抓到。抓到的：事件被丟掉、拿到給更早的 `call` 的回應、拿到等的事件之前的事件（`CdpPipeTests`）；不送 `Browser.close`、不等 process 自己結束、session 留在頁面上、pipe 的 handle 順序反了（`ChromeTests` 的 pipe 和結束）；資料夾一律當作有權限、授權不往下繼承、不認得 `AC` 這個別名、不往下繼承的授權也算數（`ChromeTests` 的權限）；不送 `Tracing.start`、不讀回 trace、只讀第一塊、`.part` 沒換成正式檔名（`MeasureTests.test_traces_of_both`）。跑完檔案都還原（`target/mutate/originals/` 不在）。
  - **和這份 RFC 無關的失敗**：`SampleReadingTests.test_machopo_is_built_from_a_tree_into_a_target` 在 Windows 上拿 `"/t"` 和 `str(Path("/t"))`（`\t`）比，測試本身的問題；2026-10-18 在 `main` 上修好（`3ac7d2f0`）。
- **merge 之後找到的 bug：Chrome 的頁面沒有 focus**（2026-10-18，Windows 11）：merge 之後第一次量 20 個 suite，Editor-TipTap 無效（machopo 有一個元素帶 `ProseMirror-focused`，Chrome 沒有）。原因是 headless 的第一個分頁要叫到前面才有 focus，用命令列開網址時不會碰到（`details.md` 的「頁面的 focus」）。`ChromePipe.open` 導覽前 `Page.bringToFront`；新的測試 `test_the_page_has_focus`（`BrowserTests`，machopo 和 Chrome 都跑，假的 `focus.html` 回報 `document.hasFocus()`）修之前 Chrome 失敗、machopo 通過。`test_the_page_is_left_alone_and_chrome_ends_on_its_own` 原本 `Page.navigate` 一回來就讀分頁的網址，多了 `bringToFront` 之後讀到空字串（網址還沒更新），改成等 `focus.html` 回報之後再讀。mutation：不 `bringToFront` 由 `test_the_page_has_focus` 抓到；改過的測試守的 4 條（不送 `Browser.close`、不等結束、session 留著、handle 順序反了）重跑都抓到。`speedometer_test.py` 62 個通過、1 個 skip，72 秒。那一次量的數字不用（Chrome 那一側的頁面沒有 focus）。
  - `sanity.py docs`。
- **Editor-TipTap 又無效：在之後的 frame 才設的 class**（2026-10-20，Fedora 44）：量了幾次 Speedometer，TipTap 都因為 Chrome 某個 iteration 的 Long 少了 `ProseMirror-focused` 無效（每次不同的 iteration：5、4、7、1）。只跑 Chrome 的 TipTap 13 次，在 `measure.mjs` 暫時記下每個 test 結束時的 focus（之後還原）：
  - 失敗的 iteration，Long 結束時頂層和 iframe 的 `hasFocus()` 都是 false、`activeElement` 是 `body`，Highlight 結束時又有 focus；頁面一直是 visible。每個 iteration 開始時 runner 換掉 iframe，頂層的 `hasFocus()` 本來就是 false。
  - 攔截 iframe 的 `requestAnimationFrame`：TipTap 的 `focus()` 在 Long 的 steps 裡排一個 rAF，正常的 iteration 它約 35 ms 後跑、拿到 focus，runner 之後才結束這個 test（約 50 ms）；失敗的 iteration runner 在 22–29 ms 就結束了，rAF 還沒跑，focus 到下一個 test 的 `focus()` 才拿到。和視窗的 focus 無關。
  - 13 次裡 1 次整次 run 都沒有 focus（每個 test 的 `hasFocus()` 都是 false，第一個 iteration 也是）：那次 Chrome 自己前後一致，換 machopo 無效。
  - `Emulation.setFocusEmulationEnabled`：導覽後設、照樣斷開 session，4 次裡照樣失敗（`1000111111`、`0000000000`…，斷開就失效）；留著 session，6 次裡 2 次失敗。
  - 改成不比 `TIMING_CLASSES`、一邊整次都沒有時註記（README 第 7 條）。新的測試：`SummaryTests.test_classes_set_in_a_later_frame_are_not_compared`、`test_a_run_that_never_had_a_timing_class_is_noted`，`ComparisonTests` 的報告多一列註記。`speedometer_test.py` 通過。
