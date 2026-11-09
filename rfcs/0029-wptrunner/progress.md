# RFC 0029 的進度：用 wptrunner 跑 WPT

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-09。完成：2026-10-10，Windows 11（i9-13900K，Rust 1.99.0）。第 0 步和三段都在 `0029`（做的時候叫 `0027`）上做完，每一段做完時 commit、審核；merge 前 rebase 到 `main`，每一段照新的 `main` 重新轉換、重新比對（「做過的檢查」的「rebase 到 `main`」）；M4 上的量測不做（「已決定的事項」第 12 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 實測：wptrunner 的 metadata 更新、轉換腳本的試跑 | 完成 | 2026-10-09 |
| 1. testharness.js 測試 | 完成 | 2026-10-09 |
| 2. WPT 的 reftest 和 crashtest | 完成 | 2026-10-10 |
| 3. kun 自己的 reftest，拿掉 Rust 的 runner | 完成 | 2026-10-10 |

## 驗收

每一段做完時都跑過 fmt（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`、`sanity.py deps`、`boundary`、`unsafe`、`docs`，以及 `test.py wpt`、`wdspec`、`reftest`；第 3 段也跑 `scenarios`，完成時跑 `automation`。`check_wpt_runs.py` 的檢查要真的跑一次 wptrunner，只給 mutation test 用（`README.md` 的「測試」）。

**WPT（`test.py wpt`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| testharness.js 測試、reftest 和 crashtest 經過 wptrunner 跑，結果和 `.ini` 相符 | `python3 scripts/test.py wpt`（0 個預期之外） |
| 換之前和換之後的結果一致（`-expected.txt`、`reftest-expectations.txt` 對 wptrunner） | 一次性的比對，結果在「做過的檢查」 |
| kun 帶 `--test-fonts` | 每一次 `test.py wpt`（不帶時 3 個文字排版的測試預期之外） |
| `MACHOPO_WPT_TIMEOUT_MULTIPLIER` 傳到 wptrunner | `python3 rfcs/0029-wptrunner/check_wpt_runs.py timeout` |
| 失敗的 reftest 的截圖：預期的失敗也寫，`-diff.png` 把不同的像素塗成紅色 | `python3 rfcs/0029-wptrunner/check_wpt_runs.py screenshots` |

**清單和預期結果**

| 項目 | 怎麼驗證 |
| --- | --- |
| 預期結果的更新：錯的拿掉、缺的補上，別的平台的條件、註解和 `bug` 留著 | `python3 rfcs/0029-wptrunner/check_wpt_runs.py update` |
| include 清單或 `.ini` 對不上 checkout 時，跑之前就失敗 | `python3 rfcs/0029-wptrunner/check_wpt_runs.py stale` |
| include 清單的每一行都至少跑了一個測試（`.any.js` 照絕對路徑交出去） | `test.py wpt`、`reftest`、`wdspec` 跑完時自己檢查 |
| include 範圍裡有 `.ini` 的測試都跑過（少跑一種測試時失敗） | `python3 rfcs/0029-wptrunner/check_wpt_runs.py unrun` |

**kun 自己的 reftest（`test.py reftest`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 經過 wptrunner 跑（`/_kun/`），`hidpi/` 在 2 倍和 1.5 倍的 subsuite，`wasm/` 不交給 wptrunner | `python3 scripts/test.py reftest`（0 個預期之外；我們的 root 至少要有一個測試跑過） |
| 換之前和換之後的結果一致 | 一次性的比對，結果在「做過的檢查」 |
| 新加的測試不用手動更新 manifest 就會跑 | `python3 rfcs/0029-wptrunner/check_wpt_runs.py kunnew` |
| `MACHOPO_UPDATE_EXPECTATIONS=1` 更新 kun 自己的 `.ini` | `python3 rfcs/0029-wptrunner/check_wpt_runs.py kunupdate` |
| wasm 文件的 9 個；比對本身（和參考頁不同的頁面失敗、參考頁是指名的那一個） | `cargo test -p shell --test wasm_reftests`（跟著 `cargo test --workspace`） |

**kun 和 wdspec**

| 項目 | 怎麼驗證 |
| --- | --- |
| `kun --automation-pipe --scale F`：頁面的 `devicePixelRatio` 和截圖的大小 | `cargo test -p shell --test automation_pipe the_scale` |
| 截圖等到請求之後畫的 frame 送進 Viz 才複製（沒有新的東西要畫時也一樣） | `cargo test -p browser --test navigation the_visual_state` |
| wdspec 改用 wptrunner 的 metadata 更新和同樣的速度設定，結果照舊 | `python3 scripts/test.py wdspec`（0 個預期之外） |

## 做過的檢查

2026-10-09 到 2026-10-10，Windows 11。每一段的設計和更正寫進了 `README.md`；這裡是濃縮的紀錄。接受之前的速度和結果一致性的實測在 `details.md` 的「2026-10-09 的實測」。

- **第 0 步**（腳本都在 repo 外面，沒有 commit）：
  - wptrunner 的 metadata 更新可以從 kun 的腳本呼叫。用四個 `dom/nodes/` 的測試試：只對 `mac` 的條件在 Windows 上更新之後保留；`disabled` 保留；錯的預期結果拿掉、真的失敗的補上；沒有 `.ini` 的失敗測試新增一份；`#` 註解和 `bug:` 都還在。要在另一個 process 做（wptrunner 跑完時關掉的 logger，同名的不能再用，`LoggerShutdownError`），要讀 log 的複本（mozlog 解析 `--log-raw` 的當下就把檔案清空）。
  - 轉換的試跑：skip 清單的 185 項轉成 `disabled`，對 testharness.js 的清單跑 wptrunner，和 `-expected.txt` 比：1601 個相同，4 個不同（2 個 testdriver 等 `postMessage`，2 個是比對腳本的問題）；之前因為字型不同的 4 個，帶了 `--test-fonts` 之後相同。
  - skip 擋不住 `variant`：wptrunner 裡每個 variant 是各自的 id，`.ini` 裡只寫檔名的話不算，一個 variant 等到 60 秒的逾時。轉換時照 manifest 展開每個 variant。
  - Windows 的路徑長度：`.ini` 的路徑有超過 260 字元的，放在很深的目錄（例如 Windows 的暫存目錄）時寫不進去；在 repo 的 `tests/wpt/metadata/` 和 `target/` 底下沒問題。
- **轉換**：skip 清單轉成 184 個 `disabled` 的 `.ini`（manual 的一個 wptrunner 本來就不跑，不轉）；testharness.js 的預期失敗用 `MACHOPO_UPDATE_EXPECTATIONS=1 test.py wpt` 寫出，共 1036 個 `.ini`（另外 84 個是 wdspec 的）。`reftest-expectations.txt` 的 75 行用一次性的腳本轉：`[ Failure ]` 是 `expected: FAIL`，`[ Mac ]` 是 `if os == "mac": FAIL`，`[ Skip ]` 是 `disabled`；每組的註解原樣放在 `.ini` 的上方。兩份 include 清單合成 1274 行（重複的只有 `css/cssom-view/`）。
- **和換之前比**：
  - testharness.js 測試：Rust 的 runner 跑過的 1653 個都跑到；1650 個的 harness 和子測試的狀態一樣，3 個不同：2 個 testdriver（wptrunner 回覆時呼叫 `window.postMessage`，kun 沒有），1 個是比對腳本分不出 Chromium 格式裡名稱的換行（實際相同）。
  - WPT 的 reftest：Rust 的 runner 跑的 704 個（630 個通過、73 個預期的失敗、1 個 skip）都跑到，703 個相同；`css/cssom-view/add-background-attachment-fixed-during-smooth-scroll.html` 在 Rust 的 runner 是失敗、在 wptrunner 是 TIMEOUT（script 丟出錯誤之後不拿掉 `reftest-wait`，Rust 的 runner 等到自己的 10 秒期限才算失敗），改成 `disabled`。
  - kun 自己的 reftest：Rust 的 runner 的 117 個（115 個通過，2 個預期的失敗都在 `harness/`）；扣掉 `harness/` 和 `wasm/` 各 9 個，wptrunner 跑的 99 個都和換之前一樣通過，`hidpi/` 的 6 個都在對的 subsuite；wasm 的 9 個由 `wasm_reftests` 跑，都通過。
- **新跑的測試**（Rust 的 runner 照規則不跑的，和合併清單多帶進的）：一次 2825 個測試 id。worker 版本、`.xhtml`、`.svg`、`.xml`、`variant` 大多很快地失敗或通過，照實記下；新跑的 reftest 有 1 個失敗、crashtest 有 4 個 ERROR，照實記下。等到逾時的寫成 `disabled` 和理由：2 個 `.xhtml`（kun 沒有 XML parser），2 個 reftest（shadow DOM、CSS paint worklet），6 個 crashtest（`scrollend`、`garbageCollect()`、跨源的子 frame、XMLHttpRequest，和 `moveBefore-subframe-count-crash.html`：一開始就拿掉 `test-wait` 卻照樣逾時，原因還沒查）。
- **找到的問題**（設計上的都寫進了 `README.md`）：
  - include 的 `.any.js`、`.window.js` 照字面交給 wptrunner，48 個測試默默地沒跑，結果照樣是 0 個預期之外（第 0 步的試跑也有，那時只比了有跑的測試）。改成交絕對路徑，加上跑完之後的檢查；拿修正前的 log 試，抓到 49 行。這個檢查一開始把 wdspec 清單裡不帶 `/` 的目錄當成檔案，改成照 checkout 判斷。
  - wptrunner 只跑給它的種類，少給一種時那種測試默默消失：加上「include 範圍裡有 `.ini` 的測試都要在 log 裡出現」，拿掉 crashtest 的 log 試，列出那幾個 crashtest。
  - 截圖的檢查一開始用的測試失敗時本來就是紅色，塗紅之後分不出來；改用 `align-baseline.html`，逐點驗證 `-diff.png`。
  - `hidpi/` 有兩個是 1.5 倍，subsuite 分成兩組。
  - 預設組用網址的前綴排除 `hidpi/`，擋不住 `test.py reftest hidpi/`：exclude 改交檔案路徑。
  - kun 的 root 的新測試放在 root 本身時不是測試（WPT 的 manifest 的規則），寫進 `tests/reftests/README.md`；manifest 的 `working_copy=False` 不支援，照預設走檔案系統。
  - 「整個 root」的 include（`""`）一開始在跑完的檢查裡被當成空行跳過，root 沒交給 wptrunner 時也算成功；補上「我們的 root 至少要有一個測試跑過」。之後又發現 WPT 的清單本來就有空行，`""` 被當成「整個 WPT」，改成只對我們的 root 有這個意思。
  - `update.py` 的迴圈變數 `metadata` 蓋掉了同名的模組（`kunupdate` 抓到）。
- **找到的 bug：截圖拿到更早的 frame**（第 3 段，修在 renderer）：`script/j4-timers.html` 在 32 個 process 一起跑時大約三次失敗一次（單獨跑不會）。做了一個把每次 tick 寫進格子的版本，失敗時格子裡沒有字：截圖拿到了最後一個 task 之前的 frame。原因和修法在 `README.md` 的「reftest 的截圖」；修好之後連跑 10 次沒有失敗（之前 10 次有 4 次）。`browser` 的 `the_visual_state_waits_for_the_frame_a_change_asked_for` 原本寫死「沒有東西要畫時立刻回答」，改成也要等一個 frame。
- **mutation test**：三段共 26 條（`mutations-stage1.toml` 9 條、`stage2` 7 條、`stage3` 10 條），都抓到，每次跑完都確認檔案還原了；速度的設定弄錯只會變慢，記在 `docs/test-gaps.md` 的「只做過手動確認」。
  - 第 1 段：不帶 `--test-fonts`；include 照網址交出去；不傳逾時的倍數（`test.py`、`run.py` 各一條）；`--update` 不更新、改成 full update；不檢查過時的項目、`.ini` 帶著 `.ini` 去找；不帶 `/` 的目錄當成檔案。
  - 第 2 段：不跑 reftest、不跑 crashtest；不檢查沒跑的 `.ini`；只留預期之外的失敗的截圖、不寫截圖、`-diff.png` 不塗紅、`-expected.png` 用了測試自己的。「不檢查沒跑的 `.ini`」第一次沒抓到：spec 給它的指令只跑一個測試，這項檢查只在照清單跑完整一次時才做；加上 `check_wpt_runs.py unrun` 之後抓到。
  - 第 3 段：product 丟掉 subsuite 的參數（`hidpi/` 在 1 倍下失敗：它們真的測到倍率）；`hidpi/` 也在預設組跑；`wasm/` 交給 wptrunner；kun 的 manifest 不更新；更新時漏掉我們的 root；設定檔沒有我們的 root；`--scale` 沒交給 pipe 的視窗；`wasm_reftests` 的比對永遠相等、參考頁就是測試頁；沒有東西要畫時立刻回答 visual state。「我們的 root 至少要有一個測試跑過」沒有單獨的 mutation：只有 root 沒交給 wptrunner 時它才有作用，「設定檔沒有我們的 root」就是靠它抓到的。第 3 段把 `test.py` 的檢查照 root 分開，第 1 段的 3 條和第 2 段的 2 條跟著改、重跑，都抓到。
- **時間**（32 個 process）：`test.py wpt` 只有 testharness.js 測試時約 40 到 48 秒，三種一起（2825 個測試 id）約 47 到 51 秒；WPT 的 manifest 第一次產生約 40 秒；`test.py reftest` 約 13 秒（之前約 2 秒），`wasm_reftests` 約 0.3 秒；wdspec 約 51 到 56 秒（一次一個 process 時約十分鐘）。
- **穩定性**：第 1 段不更新、連跑三次 `test.py wpt`，每次都是 0 個預期之外；第 3 段 `test.py reftest` 連跑 5 次都通過。不穩定的只有上面的 `j4-timers.html`（已修）。
- **拿掉的程式**：Rust 的 runner（testharness 和 reftest）、`shell::wptserve`（連同 `win32job`、`rustix` 的依賴）、`shell::expectation_tags`、`headless::render_web_test`、`test.py smoke`；`shell` 對 `dom` 的依賴移到 dev-dependencies。`cargo test --workspace` 從 1604 個（第 1 段）變成 1597 個：跟著 `shell::wptserve` 等拿掉的單元測試，加上新的 `wasm_reftests`、`automation_pipe` 的測試。
- **rebase 到 `main`**（2026-10-10，Windows 11）：做的時候 `main` 多了 26 個 commit（RFC 0026、0027 的 `fetch` 第 1 段和第 2 段的 2a–2c、0028 的草稿、interface 的 prototype 改成唯讀），改了三百多個 `-expected.txt`、兩份清單（testharness.js 多約 340 行、reftest 多約 520 行）、skip 清單、`reftest-expectations.txt` 和 `tests/reftests/` 的 37 個檔案；編號改成 0029（0027 給了 `fetch`）。不用 git 合併轉換過的檔案，每一段照新的 `main` 重做。第一次 rebase 到 `1e48aed6` 做完、檢查都過了之後，`main` 又多了 4 個 commit，再照 `732398e6` 做一次（期間 `main` 暫停 push）；下面是第二次的結果，第一次的結果相同，只是少了這 4 個 commit 的測試：
  - **基準**：在新的 `main` 上用 Rust 的 runner 跑一次。testharness.js 測試 2093 個檔，2 個不符只在訊息（在 macOS 存的 `-expected.txt` 有 Mac 的 UA、英文的時區名稱，Windows 上不同），狀態一樣；reftest 1226 個通過、103 個預期的失敗、2 個 skip（WPT 的 1205 個，kun 自己的 126 個）。
  - **第 1 段**：新的 skip 清單轉成 189 個 `disabled`（多了 6 組：等表單送出、要 `no-cors` 和 wptserve 的第二個 port、等無盡的 body、結果每次不同的 `response-null-body.any.js`，轉換腳本補上它們的一行理由），加上原本手動的 2 個 `.xhtml`；預期失敗用 wptrunner 重新寫出（第一次有大量預期之外的結果，重開 kun，跑了約 2.5 分鐘；之後約 52 秒、0 個預期之外，2808 個測試 id）。和 `-expected.txt` 比：Rust 的 runner 跑的 2093 個扣掉它自己的 9 個測試頁和 2 個 crashtest（新的清單裡有，第 1 段只跑 testharness，移到第 2 段），2082 個裡 4 個不同：2 個 testdriver（同之前）；`processing-instruction-attributes.html` 的子測試名稱有換行，Chromium 的格式寫不出來；`fetch/api/basic/status.h2.any.js` 通過（wptrunner 啟動 wptserve 的 HTTP/2 server，換之前用 `--no-h2`，`.h2` 的測試其實走 HTTP/1.1）。有 `variant` 的 6 個（`url-constructor`、`url-setters`、`url-setters-a-area`、`url-parsing.sub`、`request-keepalive-quota`、`selection-not-application`）Rust 的 runner 沒帶 query 跑：前 5 個各個 variant 的失敗合起來和 `-expected.txt` 一樣；`selection-not-application.html` 沒帶 query 時型別清單是空的，wptrunner 跑它真的兩個 variant，都通過。
  - **第 2 段**：清單照原本的做法合成 `tests/wpt/include.txt`；新的 `reftest-expectations.txt` 轉成 104 個 `.ini`，加上原本手動的 9 個 `disabled`。Rust 的 runner 跑的 WPT reftest 1205 個，1202 個相同；不同的是之前就有的 `add-background-attachment-fixed-during-smooth-scroll.html`（`disabled`），和 `css/CSS2/backgrounds/background-root-012a.xht`、`-013a.xht`：它們的 `interact` flag 讓 WPT 的 manifest 當成手動測試，wptrunner 不跑，從清單拿掉，`wpt_reftest_candidates.py` 也略過這種測試（`css/CSS2/backgrounds` 的候選從 171 個變成 169 個）。新跑的 7 個 reftest、44 個 crashtest 和之前一樣（1 個失敗、4 個 crashtest 等 `postMessage`、8 個 `disabled`）。三種一起 4062 個測試 id，約 57 秒，0 個預期之外。
  - **不穩定的測試**：第 2 段寫預期結果的那一次，`Document-parseHTMLUnsafe-url-pushstate.html` 和 `-url-base-pushstate.html` 是 ERROR（`window.resolveLoadPromise is not a function`），之前三次和之後三次都是 OK。原因在測試本身：iframe 的 `onload` 呼叫的函式由後面一個外部 script 定義，parser 等那個 script 時 iframe 先載入完的話（HTML 允許這個順序），`onload` 丟出錯誤，harness 在任何子測試之前就 ERROR；整次跑的負載下大約六次一次。同樣寫法的 8 個檔（`domparsing/` 和 `html-unsafe-methods/` 各 4 個）記成 `expected: [OK, ERROR]`（wptrunner 的 intermittent），照樣跑；帶更新再跑一次，metadata 的更新保留這個寫法。
  - **第 3 段**：kun 自己的 reftest 多了 `backgrounds/` 的 9 個（沒有用到拿掉的擴充）；Rust 的 runner 的 126 個扣掉 `harness/` 和 `wasm/`，wptrunner 跑的 108 個都和換之前一樣通過，`hidpi/` 的 6 個都在對的 subsuite，約 11 秒。`render_thread.rs` 的 visual state 的修正自動合併，和 `main` 的改動不衝突。
  - **wdspec**：5 個預期之外的通過（`execute_script`、`execute_async_script` 的 form controls 和 options collection、`test_content_attribute`，以及 Get Element Property 的 `test_content_attribute`）：`main` 上做的表單讓它們過了，wdspec 的預期結果當時沒有更新；用 `MACHOPO_UPDATE_EXPECTATIONS=1 test.py wdspec` 拿掉，之後 0 個預期之外。
  - **rebase 之後的檢查**：`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`（1774 個）、`sanity.py` 的四項、`test.py wpt`（4062 個測試 id，0 個預期之外，約 71 秒）、`reftest`（108 個）、`scenarios`（41 次）、`wdspec`、`automation`（33 份和 `pipe --window`，截圖看過）都通過，WPT 的 checkout 不變。mutation test 沒有重跑：rebase 沒有改到 `test.py`、`run.py`、product 和 renderer 的修正，只改了清單和預期結果。
  - **跟著改的**：`sanity.py docs` 不檢查 `.ini` 的段落標題（WPT 的子測試名稱 `Display P3 colors` 被當成引用 MVP 的階段；註解照樣檢查，手動放了一行引用 RFC 的註解確認會失敗）；`docs/limitations.md` 說 kun 沒有 HTTP/2 的那一條改正；`main` 新加的文字裡指向拿掉的檔案的（`speedometer` 測試的註解、`tests/wpt/README.md` 的紀錄）跟著改。
- **RFC 完成時**（`docs/README.md` 的「一份 RFC 做完時」，2026-10-10，Windows 11）：這份 RFC 沒動到 GC，不跑 `matrix`、`fuzz`。文件的一致性交給一個唯讀的 agent 對過這份改過的檔案（`git diff 2af016d4..HEAD`），找到的都改了：根目錄 README 的指令說明和環境變數表、`docs/testing.md` 的一條已經不存在的等價 mutation、manifest 的時間、舊的 CI 設定和現在的差別、`docs/limitations.md` 的平台（wptrunner 只在 Windows 11 上跑過、沒有每天的 CI）、`tests/wpt/README.md` 紀錄裡的時態，以及程式碼註解裡 visual state、headless 和 `SoftwareRenderer` 的用途。第 3 段做完時的檢查：`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy（`-D warnings`）、`cargo test --workspace`（1597 個，144 個 test binary）、`sanity.py` 的四項、`test.py scenarios`（34 次）、`wpt`、`wdspec`、`reftest` 都通過，WPT 的 checkout 不變；完成時另外跑了 `test.py automation`（renderer 的 visual state 也是真的視窗的截圖和等畫面走的路）：28 份情境和 `pipe --window` 都通過，截圖看過；改了文件和註解之後 `sanity.py docs` 通過。
