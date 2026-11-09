# RFC 0029：用 wptrunner 跑 WPT

- 開始日期：2026-10-09
- 狀態：✅ 已完成：2026-10-10（「已決定的事項」第 1 到 8 條 2026-10-09 接受；第 1 段開始前第 9 到 12 條，第 2 段開始前第 13 到 16 條，第 3 段開始前第 17 到 20 條，merge 前第 21 條；第 0 步和三段都做完，只在 Windows 11 上做；原本編號 0027，merge 前 rebase 到 `main` 時改成 0029（0027 已經給了 `fetch`），三段照新的 `main` 重新轉換、重新比對，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`

## 摘要

WPT 的 testharness.js 測試、reftest 和 crashtest，以及 kun 自己的 reftest，改由上游的 wptrunner 跑：wptrunner 經過 machopodriver（WebDriver）控制 kun，和其他瀏覽器跑 WPT 的方式一樣。Rust 寫的 runner（`4ff8d6e:crates/shell/tests/wpt_testharness.rs`、`4ff8d6e:crates/shell/tests/reftest.rs` 和它們用的 `shell::testharness`、`shell::reftest`、`shell::wptserve`）整個拿掉；只剩 9 個 wasm 文件的 reftest，wptrunner 跑不了，留成一個經過 `headless` 比對像素的 `cargo test`。kun 的 wptrunner product 和呼叫 wptrunner 的腳本是 wdspec 原本的那一份（`tests/wpt/wptrunner_machopo/`），擴大到 testharness、reftest、crashtest 和另一個 test root。這是 testdriver 的前置：testdriver 的動作由 wptrunner 執行。

預期結果改用 wptrunner 的 `.ini`（skip 是 `disabled` 加理由），要跑的測試照舊是清單（兩份合成 `tests/wpt/include.txt`），kun 帶 `--test-fonts`，更新預期結果用 wptrunner 自己的 metadata 更新；kun 自己的 reftest 在 WPT 旁邊的 `/_kun/`（像 Servo、Firefox 的 `/_mozilla/`），`hidpi/` 在 wptrunner 的 subsuite 裡用 `kun --scale` 跑；process 數預設 `min(32, 4 × CPU 數)`。換之前和換之後的結果逐一比過，不一樣的都找到了原因（`progress.md`）；過程中修了 WebDriver 截圖拿到更早的 frame 的 bug。

## 動機

**做之前（2026-10-09）**：WPT 有兩條跑法。testharness.js 測試和 reftest 走 Rust 的 runner：headless 的 kun 在同一個 process 裡開分頁，kun 自己的 `testharnessreport.js`（`4ff8d6e:crates/shell/src/testharnessreport.js`）經過 `testRunner` 交出結果，和 Chromium 格式的 `-expected.txt` 比對。wdspec 和冒煙測試（`test.py smoke`）走上游的 wptrunner 加 machopodriver。前者是 kun 照 wptrunner 的行為重寫的，後者是上游的。代價：

- **testdriver 沒有路**：wptrunner 收到頁面的 testdriver 請求時，自己用 WebDriver 執行（`actions.py` 有 52 個動作，`asyncactions.py` 還有 20 個 BiDi 的）。Rust 的 runner 要支援 testdriver，就得把這些一個個用 Rust 重寫，而且上游一直在加。`4ff8d6e:tests/wpt/testharness-skip.txt` 裡整組 testdriver 的測試在等它。
- **兩份行為要一直對齊**：`.any.js` 的包裝和 global、`timeout=long`、`variant`、reftest 的 chain 和 `fuzzy`、每個測試前的環境（新分頁、焦點、視窗大小）、逾時的計時方式，Rust 的 runner 都是自己判斷的。2026-10-09 實測兩條路的結果：幾乎都一樣，不一樣的都是環境（字型、分頁的焦點、`postMessage`），見 `details.md` 的「結果一致嗎」。
- **結果不能和其他瀏覽器比**：沒有 wptreport，不能上傳 wpt.fyi。
- **`variant`、crashtest、`.any.js` 的 worker 版本沒有跑**：Rust 的 runner 只跑沒有 query 的網址、只跑 window 的版本，不跑 crashtest。

**解決的測試空缺**（`docs/test-gaps.md` 的「測試守不住的地方」，跟著 runner 拿掉了）：WPT 的 testharness.js runner 本身和它的排程、`MACHOPO_WPT_TIMEOUT_MULTIPLIER` 的接線；reftest 的 runner 的平行執行；wptserve 的 process 的管理（`shell::wptserve`）；WPT 的 reftest 從 wptserve 載入的那一項。`docs/limitations.md` 沒有這份要解決的限制。

**不在範圍內**：

- **testdriver、Actions、Element Send Keys**：下一份 RFC。換上的 wptrunner 會真的執行 testdriver 的請求，但回覆要 `window.postMessage`，kun 還沒有，那些測試照樣失敗或 skip。
- **跑整個 manifest**（不列清單、全部都跑）、**CI**、**上傳 wpt.fyi**：見「之後的可能」。
- **wasm 文件執行 WebDriver 的 script**：wasm 的 reftest 因此留成 `cargo test`（「已決定的事項」第 7 條）。
- **html5lib 的 tree construction**（`cargo test -p html --test tree_construction`）：照舊。
- **端到端測試的 `testRunner`**：`shell` 的 `geometry`、`history` 等測試經過 `testRunner.reportHarnessResults` 交回結果，這個 hook 留著，只拿掉 WPT 的 `testharnessreport.js`。
- **`wpt run`**：照舊不用（它要系統的 hosts 檔有 WPT 的網域），和 wdspec 一樣由 kun 的腳本直接呼叫 wptrunner。

## 設計概要

**給跑測試的人**：

```sh
python3 scripts/test.py wpt                 # WPT 的 testharness.js 測試、reftest、crashtest，一次跑完
python3 scripts/test.py wpt dom/nodes/      # 只跑某些路徑
python3 scripts/test.py reftest             # kun 自己的 reftest（wasm 文件的在 cargo test 裡）
python3 scripts/test.py wdspec              # WPT 的 WebDriver 測試（預設不跑，要時才跑，第 21 條）
MACHOPO_UPDATE_EXPECTATIONS=1 python3 scripts/test.py wpt
```

- 三個指令都經過 wptrunner、machopodriver 和 kun，共用 `target/wptrunner/` 的 venv 和 manifest；WPT 的 manifest 只在 checkout 換 commit 時重新產生（第一次約一分鐘）。wptserve 的 port 固定，一次只能跑一份。
- WPT 的預期結果是 wptrunner 的 `.ini`，在 `tests/wpt/metadata/`（每個沒有全部通過的測試檔一份，和 wdspec 的放在一起）；不跑的測試在裡面寫 `disabled` 和一行理由，較長的說明是上方的 `#` 註解。kun 自己的 reftest 的放在 `tests/reftests/` 底下自己的 metadata 目錄（目前全部通過，一個也沒有）。
- 失敗時 wptrunner 印出和預期不同的結果；失敗的 reftest 的截圖寫在 `target/wpt-results/screenshots/`（kun 自己的在 `target/reftest-results/screenshots/`）：`-actual.png`、`-expected.png`、`-diff.png`。wptreport 和原始的 log 在同一個目錄。
- process 數預設每個 CPU 四個、最多 32 個，`--processes N` 可以改。

**給改 kun 的人**：kun 的 wptrunner product 帶 `--test-fonts`（「已決定的事項」第 5 條），每個測試的環境和其他瀏覽器在 wptrunner 裡一樣（新分頁、點一下 `documentElement` 取得焦點、800×600 的 viewport）。`kun --automation-pipe --scale F` 是 device scale factor，給 `hidpi/` 的 subsuite 用。

## 詳細設計

### 呼叫 wptrunner

`scripts/test.py` 建 venv、build `kun` 和 `machopodriver`，再執行 `python -m wptrunner_machopo.run`（`tests/wpt/wptrunner_machopo/`，裝進 venv 的 product）：

- **三種測試一次跑**（`--test-types testharness reftest crashtest`，第 13 條），product 用 wptrunner 的三個 WebDriver executor。
- **速度的設定**（實測在 `details.md` 的「速度」）：
  - `--fully-parallel`：一個測試一組，process 做完一個從共用的佇列拿下一個。預設的 `SingleTestSource` 照測試 id 的 hash 靜態地分給各 process，實測 16 個 process 的忙碌時間從 10 秒到 51 秒不等。
  - `--no-restart-on-new-group`：不然 `--fully-parallel` 每個測試都重開瀏覽器。
  - WPT 的 manifest 只在 checkout 的 commit 變了時更新（manifest 旁邊的 stamp）：每次檢查整個 checkout 要 10 秒左右。
  - process 數預設 `min(32, 4 × CPU 數)`（第 8 條）：process 大多在等（等逾時、等畫面），可以比核心多。
  - 預期之外的結果照上游的預設重開瀏覽器（一次約 3.5 秒），預期結果齊全時幾乎不會發生。
- **網路**：product 經過 driver 傳 `--host-resolver-rules` 和 WPT 的根憑證，wptserve bind 在 `127.0.0.1`。wptrunner 也啟動 wptserve 的 HTTP/2 server，換之前的 runner 用 `--no-h2`：`.h2` 的測試以前其實走 HTTP/1.1，`fetch/api/basic/status.h2.any.js` 的失敗被當成 kun 不會 HTTP/2，換了之後通過（kun 經過 reqwest 談 HTTP/2，`docs/limitations.md` 那一條跟著改正）。
- **checkout 不變**：Python 不寫 bytecode，跑完檢查 checkout 沒被寫入。
- **清單交給 wptrunner 的方式**：wptrunner 的 include 先把每一行當成「從目前的目錄算起的檔案路徑」去找，找到了照 manifest 展開成那個檔案的所有測試（`variant`、`.any.js` 的每個 global），找不到的當成測試網址的前綴。我們的清單寫的是原始檔（`x.any.js`），不是網址（`x.any.html`、`x.any.worker.html`），所以 `test.py` 把每一行換成 checkout 裡的絕對路徑再交出去。原本照字面交出去，wptrunner 又是從 `target/wptrunner/` 執行的，`.any.js` 和 `.window.js` 的 48 個測試默默地沒跑，結果照樣是 0 個預期之外（第 1 段找到）。
- **wptrunner 默默略過的，`test.py` 自己檢查**（照 test root 分開做）：
  - 開始之前：include 的每一行和每個 `.ini` 在 root 裡有對應的檔案或目錄（換之前的 runner 也會在這時失敗，wptrunner 不會）。
  - 跑完之後：include 的每一行都至少跑了一個測試（`disabled` 的也算，它們在 log 裡是 SKIP）——上一條的問題就是在這裡抓到；include 範圍裡每個有 `.ini` 的測試檔都有測試跑過——wptrunner 只跑給它的種類，少給一種（例如 crashtest）時那種測試默默消失，上一項抓不到（第 2 段加的）；我們的 root 至少有一個測試跑過——root 沒交給 wptrunner 時一個測試都不跑也算成功（第 3 段加的）。include 清單裡的空行在 WPT 的清單裡就是空行，只有我們的 root 用 `""` 表示整個 root。
- **log**：wptrunner 把瀏覽器寫的東西都印出來，`test.py` 讓 kun 和 machopodriver 只印 warning 以上（`RUST_LOG=warn`，有設的話照設的）；網頁的 `console.log` 和沒接住的例外照樣印。
- **wdspec**：同一個 `run.py` 和同樣的設定，從一次一個 process 的約十分鐘變成約一分鐘。

### 預期結果、清單和更新

第 2、3、6、9、10、13、14 條：

- **轉換**（一次性的腳本，第 11 條）：skip 清單轉成 `disabled`（每個測試檔的每個網址一節，照 manifest 展開；目錄寫成 `__dir__.ini`；manual 的測試 wptrunner 本來就不跑，不轉）。testharness.js 的預期失敗不從 `-expected.txt` 解析（Chromium 的格式分不出 subtest 名稱裡的換行），而是用 wptrunner 跑一次、用它自己的 metadata 更新寫出 `.ini`，再逐一和 `-expected.txt` 比對。`reftest-expectations.txt` 直接轉：`[ Failure ]` 是 `expected: FAIL`，`[ Skip ]` 是 `disabled`。
- **平台的差別**：Chromium 的平台標籤（`[ Win ]` 等）寫成 `.ini` 的條件（`if os == "win"`）。只存狀態也消掉了訊息裡的平台差別：rebase 時 `main` 上在 macOS 存的兩個 `-expected.txt` 在 Windows 上不符（訊息裡有 Mac 的 UA、英文的時區名稱），狀態是一樣的。
- **手動測試**：WPT 的 manifest 把有 `interact`、`paged` 等 CSS flag 的測試當成手動測試，wptrunner 不跑。舊的 runner 只看 `rel=match`，跑了其中兩個（`css/CSS2/backgrounds/background-root-012a.xht`、`-013a.xht`，rebase 時找到）：從清單拿掉，`scripts/wpt_reftest_candidates.py` 也略過這種測試。
- **`sanity.py docs`**：`.ini` 的段落標題是 WPT 的測試和子測試的名稱，可能剛好像 MVP 的階段代號（`Display P3 colors`），不檢查它們有沒有引用 RFC；註解照樣檢查。
- **Rust 的 runner 照規則不跑的**（`.any.js` 的 worker 版本、`.xhtml`、`.svg`、`.xml`、`variant`）和合併清單之後多帶進的 reftest、crashtest：照實際的結果記下，會等到逾時的才 `disabled`（第 10 條）。
- **更新**（`MACHOPO_UPDATE_EXPECTATIONS=1`）：`run.py --update` 在跑完之後，另開一個 process 執行 `wptrunner_machopo/update.py`，用 wptrunner 的 `metadata.update_expected`（`os` 是唯一的 property，不是 full update：full update 照這一次的 log 重建所有條件，會洗掉別的平台的）。另開 process 是因為 wptrunner 跑完時會關掉它的 logger，同名的 logger（metadata 的更新用的）就不能再用；`update.py` 也不經過 wptrunner 的參數解析，mozlog 解析 `--log-raw` 的當下就會把 log 清空。註解、`bug:`、`disabled` 和別的平台的條件，經過更新都留著（第 0 步確認）。
- `MACHOPO_WPT_TIMEOUT_MULTIPLIER` 交給 wptrunner 的 `--timeout-multiplier`（wptrunner 在測試的逾時之外另給 5 秒）。

### reftest 的截圖

- `run.py` 跑完之後由 `screenshots.py` 從 log 寫出（第 15 條）：log 裡每個失敗的 reftest 有一串截圖，依序是測試、關係（`==` 或 `!=`）、參考頁，chain 每多一步就多一個關係和一張參考頁，最後兩張是判定失敗的那一次比較；`-diff.png` 是實際的截圖，和預期不同的像素塗成紅色。
- **截圖等到 frame 真的送進 Viz**（第 3 段找到的 bug，修在 renderer）：WebDriver 的 Take Screenshot 先向 renderer 要「目前狀態的 frame」（`request_visual_state`）。renderer 原本在沒有待做的 main frame 時立刻回答，以為上一次 commit 的 frame 已經在 Viz 的佇列裡；但 compositor 可能還沒畫它，Viz 先處理了複製的請求，截到更早的 frame：一個在最後一個 task 才拿掉 `reftest-wait` 的 reftest（`script/j4-timers.html`），32 個 process 一起跑時大約三次失敗一次（`progress.md` 的第 3 段）。改成一律要一個 main frame、等它的 frame swap 之後才回答，像 Chromium 的 `VisualStateRequest`。換之前的 Rust runner 的截圖綁在 frame token 上，碰不到這個問題；wdspec 和情境的截圖、等畫面也經過同一條路。

### kun 自己的 reftest

第 7、17 到 20 條（`details.md` 的「kun 自己的 reftest 能不能搬到 wptrunner」）：

- **另一個 test root**：`run.py --root URL_BASE TESTS METADATA` 寫出 wptrunner 的設定檔（`--config`，`[manifest:_kun]`），`tests/reftests/` 在 `/_kun/`，wptserve 照設定檔掛上它（Servo 的 `[manifest:mozilla]`）。測試改從 HTTP 載入，不再是 `file:`。這個 root 的 manifest 每次跑之前更新：WPT 的只在換 commit 時更新，kun 的測試卻跟著 repo 改；目錄小，很快（manifest 的工具走檔案系統，還沒 `git add` 的新測試也找得到）。WPT 的 manifest 把 root 本身的檔案、`resources/` 和 `support/` 底下的檔案當成支援檔，所以測試放在子目錄裡。
- **`hidpi/`**（6 個）：wptrunner 的 subsuite，一組測試配一組瀏覽器的參數（`subsuite.config` 的 `binary_args`，Chromium 的 virtual suite 也是這樣），列在 `tests/reftests/subsuites.json`：4 個是 2 倍、2 個是 1.5 倍，各一組。`kun --scale` 擴大到 `--automation-pipe`（第 17 條），product 把 subsuite 的參數經過 driver 交給 kun。`kun-device-scale-factor` 這個 meta 拿掉。預設的 subsuite 用 `--exclude` 排除 `hidpi/` 和 `wasm/`，交的是檔案路徑、不是網址的前綴：wptrunner 把檔案路徑展開成一個個測試，測試自己的規則蓋過目錄的，網址的前綴擋不住 `test.py reftest hidpi/` 指定的測試（第 3 段做的時候發現）。
- **`kun-fragment`**（5 個）：改成在 `load` 時設 `location.hash`、兩個 frame 之後拿掉 `reftest-wait`（WPT 的捲動測試都用 script），meta 拿掉。
- **`harness/`**（9 個）和它的 `expectations.txt`：拿掉，它們測的是 Rust 的 runner 的 chain 判斷。
- **wasm 的文件**（9 個）：`shell` 的 `wasm_reftests`，跟著每次的 `cargo test`。經過 `headless` render 測試頁和它的 `<link rel="match">` 指的參考頁、逐像素比對；沒有清單、預期結果、chain、`fuzzy` 和平行，`reftest-wait` 照 `headless` 的處理（animation frame 用虛擬的時鐘）。另外兩個測試守住比對本身：和參考頁不同的頁面要失敗、參考頁是測試指名的那一個。
- `requestAnimationFrame` 的虛擬時鐘不需要：用到它的測試只檢查順序、間隔和次數，wptrunner 裡用真的時間。

### 拿掉和留下的

| 部分 | 現在 |
| --- | --- |
| `4ff8d6e:crates/shell/tests/wpt_testharness.rs`、`shell::testharness`、`4ff8d6e:crates/shell/src/testharnessreport.js`、`4ff8d6e:tests/testharness/` | 拿掉（第 1 段） |
| `4ff8d6e:crates/shell/tests/reftest.rs`、`shell::reftest`、`shell::expectation_tags` | 拿掉（WPT 的那一組在第 2 段，其餘在第 3 段） |
| `shell::wptserve`（連同 `win32job`、`rustix` 的依賴）、`headless::render_web_test`、`test.py smoke` | 拿掉（第 2 段） |
| `4ff8d6e:tests/wpt/baselines/`、`4ff8d6e:tests/wpt/testharness-skip.txt`、`4ff8d6e:tests/wpt/reftest-expectations.txt`、`4ff8d6e:tests/reftests/expectations.txt` | 轉成 `.ini` 之後拿掉 |
| `4ff8d6e:tests/wpt/testharness-include.txt`、`4ff8d6e:tests/wpt/reftest-include.txt` | 合成 `tests/wpt/include.txt` |
| `testRunner.reportHarnessResults`、`RendererHostMsg::TestharnessResults`、`headless::run_testharness` | 留下：端到端的測試在用 |
| `headless` 的 `reftest-wait` 和截圖 | 留下：`wasm_reftests` 和端到端的測試在用 |
| `scripts/wpt_reftest_candidates.py` | 留下：挑測試的方式和 runner 無關 |

### 測試

- **換之前和換之後的比對**是這份的驗收：同一份清單，Rust 的 runner 和 wptrunner 各跑一次，每個測試的狀態和每個子測試的狀態相同；不同的逐一找出原因（`progress.md` 每一段的「和換之前比」）。只在 Windows 11 上做（第 12 條）。
- **kun 這一側的接線**：mutation test 確認有測試會失敗（三段的 spec 在這個資料夾，`mutations-stage*.toml`）；要真的跑一次 wptrunner 才看得出來的（更新預期結果、過時的 `.ini`、沒跑到的 `.ini`、逾時的倍數、截圖、kun 的 root 的新測試和更新），由這個資料夾的 `check_wpt_runs.py` 檢查，只給 mutation test 用。速度的設定弄錯只會變慢，記在 `docs/test-gaps.md` 的「只做過手動確認」。
- **`--automation-pipe --scale`**：`shell` 的 `automation_pipe` 測試（`devicePixelRatio` 和截圖的大小）。
- **visual state**：`browser` 的 `navigation` 測試（Viz 暫停 `BeginFrame` 時不回答，沒有新的東西要畫時也一樣）。
- wptrunner 本身是上游的，不另外測；wdspec 照舊是 machopodriver 和規範一致的驗收，只是預設不跑（第 21 條）。

### 分階段

0. **實測**（2026-10-09）：速度、process 數、結果一致性（`details.md` 的「2026-10-09 的實測」），wptrunner 的 metadata 更新和轉換的試跑（`progress.md`）。M4 上的量測不做（第 12 條）。
1. **testharness.js 測試**（2026-10-09）：轉換預期結果和 skip 清單，`test.py wpt` 改走 wptrunner，拿掉 Rust 的 testharness runner 和它的測試頁；wdspec 也改用 wptrunner 的 metadata 更新和同樣的速度設定。
2. **WPT 的 reftest 和 crashtest**（2026-10-10）：三種測試一次跑、清單合成一份、截圖、轉換 `reftest-expectations.txt`；拿掉 Rust runner 的 WPT 那一組、`shell::wptserve`、`test.py smoke`。
3. **kun 自己的 reftest**（2026-10-10）：`/_kun/` 的 test root、`--scale` 和 `hidpi/` 的 subsuite、改寫 `kun-fragment` 的 5 個、拿掉 `harness/`、`wasm_reftests`；拿掉整個 Rust 的 runner；修 visual state。

三段都在 `0029` 上（做的時候叫 `0027`），每段做完時 commit、審核。merge 前 rebase 到 `main`（RFC 0026、0027、0028 等 26 個 commit，清單和 `-expected.txt` 在這期間改了很多；做到一半 `main` 又往前走，做了兩次）：每一段照新的 `main` 重新轉換、重新寫出預期結果、重新和 Rust 的 runner 比對，結果在 `progress.md`。

## 缺點

- **變慢**（2026-10-09 到 10，Windows 11，i9-13900K，`details.md` 的「速度」和 `progress.md`）：testharness.js 測試全部，Rust 的 runner 約 26–29 秒，wptrunner：8 核以上和模擬的 M4 約 33–34 秒（約慢 20–30%），4 核約 47 秒（約慢 60%）；多跑了 worker 版本、XML、`variant`、reftest 和 crashtest 之後，`test.py wpt` 一次約 50 秒（rebase 到 `main` 之後清單大了，約 58 秒）。只跑一個測試，固定多 2–3 秒（載入 manifest、啟動 wptserve、machopodriver 和 kun）。kun 自己的 reftest 從約 2 秒變成約 10 到 13 秒（第 20 條）。
- **多很多 process 和記憶體**：每個 wptrunner 的 process 是一個 Python 的 worker、一個 machopodriver 和一個完整的 kun；32 個 kun 合計約 2.5 GB。
- **每次跑 WPT 和 reftest 都要 Python 的 venv**：WPT 原本就要（wptserve）；kun 自己的 reftest 原本不用。
- **預期結果只存狀態**（第 2 條）：失敗的訊息不再存在預期結果裡，在 log 和 wptreport 裡。
- **wasm 的 reftest 另外一條路**：9 個 wasm 文件的 reftest 不經過 wptrunner，比對的規則（只比像素，沒有 `fuzzy`）和其他 reftest 不同。
- **kun 自己的 reftest 失去兩個擴充**：`kun-fragment` 改用 script，`kun-device-scale-factor` 改成 subsuite（要在 `subsuites.json` 列出哪些測試）。
- **只在 Windows 上驗證過**：macOS、Linux 上的速度、process 數和平台條件都還沒確認（第 12 條）。

## 理由和其他做法

完整的比較在 `details.md`。

- **為什麼不和 Rust 的 runner 並存**（W2，Rust 的 runner 留給日常的回歸、wptrunner 跑 testdriver 和全套）：兩邊的環境會分開漂移，2026-10-09 的比對已經看到字型和焦點不同；預期結果也要維護兩份或一份加轉換。第 3 段找到的截圖的 bug 也是只走 WebDriver 才看得到。
- **為什麼不把 Rust 的 runner 改寫成和 wptrunner 一樣**（W3）：testharness 和 reftest 的部分不難對齊，難的是 testdriver：72 個動作，上游一直在加，而且一致只能靠兩邊都跑來檢查（`details.md` 的「自己改寫 runner 的成本」）。對齊之後，Rust 的 runner 省下的時間也會變少：wptrunner 每個測試前的步驟它也得照做。
- **為什麼是現在**：WebDriver（machopodriver）做完了，testdriver 的下一步就是換 runner；預期結果和清單越晚換，要轉的越多。

## 先例

各家的做法比較在 `details.md` 的「各家的做法」。

- **Chromium**：`run_wpt_tests.py` 用 wptrunner 經過 chromedriver 跑（預設是 `headless_shell`），用的是和 `run_web_tests.py` 相同的 `-expected.txt` 和 TestExpectations；不是 WPT 的 web test 照舊由 `run_web_tests.py`（content_shell）跑。要加 flag 的測試用 virtual suite，wptrunner 那邊是 subsuite。kun 不照它保留 `-expected.txt`（第 2 條），自己的測試也不留在另一個 runner（第 7 條）；subsuite 照做（第 18 條）；`VisualStateRequest` 的等法照做。
- **Firefox、Servo**：wptrunner，預期結果是 wptrunner 的 `.ini`；自己的測試放在另一個 test root（`/_mozilla/`），一樣經過 wptrunner。kun 照這個做。
- **Ladybird**：wptrunner 經過自己的 WebDriver（kun 的 product 照它寫的）。
- **WebKit**：自己的 `run-webkit-tests` 跑匯入的 WPT、自己的 `testdriver-vendor.js`；Safari 在 wpt.fyi 的結果另外用 wptrunner 加 safaridriver 跑，兩套都在維護。

## 已決定的事項

2026-10-09 確認：

1. **WPT 全部改由 wptrunner 跑，拿掉 Rust 的 runner 裡 WPT 的部分**（W1；第 7 條之後，整個 Rust 的 runner 都拿掉）。不選兩套並存（W2），也不選把 Rust 的 runner 改寫成和 wptrunner 一樣（W3），理由見「理由和其他做法」。速度的代價接受（「缺點」），用實測的設定壓低（「呼叫 wptrunner」）。
2. **預期結果用 wptrunner 的 `.ini`**（A1，Firefox、Servo、Ladybird，kun 的 wdspec 已經在用），取代 Chromium 的 `-expected.txt` 和 `reftest-expectations.txt`。當初選 `-expected.txt` 時不選 `.ini` 的理由是「wptrunner 自己的格式」，換成 wptrunner 之後不成立；不用自己寫轉換和比對（這份要減少的正是自己的程式），平台條件是原生的。不選保留 `-expected.txt`、執行時轉成 wptrunner 的預期結果（A2，Chromium：它有大量既有的 baseline，而且和 `run_web_tests.py` 共用）。代價：預期結果只存狀態，不存失敗的訊息（在 log 和 wptreport 裡）；現有的 `-expected.txt` 和 `reftest-expectations.txt` 轉換一次。
3. **要跑的測試照舊是清單，skip 寫成 `.ini` 的 `disabled` 加理由**（B1）：清單交給 wptrunner 的 `--include-file`（兩份清單在第 13 條合成 `tests/wpt/include.txt`）。不選 Servo 的 `include.ini`（B2，include 和 skip 寫在同一個可以一層層覆蓋的檔案）：B1 的改動最小，skip 的理由跟著測試的預期結果。不跑整個 manifest（B3）：在「之後的可能」。
4. **拿掉 runner 自己的測試頁**（D1，`4ff8d6e:tests/testharness/`）：它們測的是 Rust 的 runner 和 kun 的 `testharnessreport.js`，兩者都拿掉；wptserve 的功能（`.sub`、`.headers`、HTTPS）和 kun 的網路設定（WPT 的網域、根憑證）由清單裡用到它們的 WPT 測試涵蓋。不選搬到 wptrunner 的另一個 test root（D2）。
5. **kun 的 wptrunner product 帶 `--test-fonts`**（F1）：Ahem 和 Lato，和換之前的 runner 一樣，結果不隨平台和機器上裝的字型變。不選系統的字型（F2，和一般使用一樣，但結果隨平台變）。2026-10-09 實測時沒帶它，4 個和文字排版有關的測試結果不同（`details.md` 的「結果一致嗎」）。
6. **更新預期結果用 wptrunner 自己的 metadata 更新**（G1），保留平台條件和 `disabled`。不選 wdspec 原本的寫法（G2，`run.py` 的 `write_metadata`：一律寫成沒有條件的，會蓋掉平台條件）。第 0 步確認它能從 kun 的腳本呼叫；wdspec 也跟著換。
7. **kun 自己的 reftest 搬到 wptrunner 的另一個 test root，wasm 文件的 9 個留成一個小的 `cargo test`**（C3）：`/_kun/`（Servo、Firefox 的 `/_mozilla/`）；`hidpi/` 用 wptrunner 的 subsuite 加上 kun 在 `--automation-pipe` 時的 device scale factor；`kun-fragment` 的 5 個改用 script 設 fragment；`harness/` 拿掉；wasm 的 9 個經過 `headless` 比對像素，沒有清單、預期結果、chain、`fuzzy` 和平行（「kun 自己的 reftest」）。這樣整個 Rust 的 runner 都拿掉。不選留在 Rust 的 runner（C1，Chromium 的非 WPT web test 留在 `run_web_tests.py`）：清單、預期結果、chain、`fuzzy`、平行和 wptserve 的管理都得留著，和第 1 條要減少的是同一批程式。不選連 wasm 的文件也搬（C2）：wptrunner 的 reftest 要在測試頁裡執行 script，wasm 的文件要先有只給 WebDriver 用的 JS 世界，那要自己的 RFC；值得做時再把這 9 個搬過去。2026-10-10 第 3 段開始前討論過直接刪掉這 9 個，不刪：它們是唯一經過整個瀏覽器、用真的 wasmtime 跑 guest 的測試（`shell` 的 `wasm_scripts` 用 V8 假裝 wasm engine，`dom_wasm` 的測試不經過瀏覽器），W6、W9 也是 Rust 和 Lua 的 guest 唯一的端到端測試；留著只是一個小的 `cargo test`。
8. **process 數預設 `min(32, 4 × CPU 數)`**，`test.py` 的選項可以改：process 大多在等，可以比核心多。實測 8 核和模擬的 M4（4 個效能核心加 6 個節能核心）開 32 個最快（約 34 秒），4 核 16 個就到頂（`details.md` 的「速度」）。M4 照這個公式是 32 個；模擬用的 Intel E-core 比 M4 的弱，所以那邊應該不會更慢。沒有在 M4 上量過（第 12 條）：在 macOS 上用時明顯比這裡慢，再量、再改公式。

2026-10-09，第 1 段開始前確認（第 0 步的結果在 `progress.md`）：

9. **skip 的理由：一行寫在 `disabled:` 的值，較長的說明寫成上方的 `#` 註解**：wptrunner 跳過測試時印出 `disabled` 的值（Firefox 在這裡放 bug 的網址），註解和 `bug:` 經過 metadata 的更新都會保留（第 0 步確認）。不選只寫註解、`disabled` 的值放固定的字：跳過時看不到理由。
10. **Rust 的 runner 照規則不跑、wptrunner 照 manifest 會跑的測試，照實際的結果記進 `.ini`**：`.any.js` 的 worker 版本、`.xhtml`、`.svg`、`.xml`、`variant`。它們大多很快就失敗；有了 worker 或 XML parser 時，會以預期之外的通過出現。會等到逾時的才寫 `disabled` 和理由。不選照舊的規則一律 `disabled`：要另外維護一份「哪些種類不跑」的規則，有了功能也不會知道。
11. **轉換腳本不 commit**：它只用一次（skip 清單轉成 `disabled`、照 manifest 展開 `variant`；`reftest-expectations.txt` 轉成 `.ini`），做法和比對的結果記在 `progress.md`。不選放進 `scripts/`：之後沒有人會再跑它。
12. **不在 M4 上量，公式照這台 Windows 的實測和模擬確定**（2026-10-10 改）：原本定為第 1 段做完時在 M4 上用 `test.py wpt --processes N` 量，再確認第 8 條的公式；2026-10-10 決定這份 RFC 只在目前的裝置（Windows 11，i9-13900K）上做，M4 上的量測不做。第 8 條的公式有 8 核、4 核和模擬的 M4 的數字支撐（`details.md` 的「速度」）；macOS 上的速度和平台的差別（例如只在 macOS 上失敗的 reftest）沒有確認過，記在 `docs/test-gaps.md` 的「只做過手動確認」的各平台那一列。

2026-10-10，第 2 段開始前確認：

13. **`test.py wpt` 一次跑 WPT 的三種測試（testharness、reftest、crashtest），兩份 include 清單合成一份**（A1）：wptrunner 一次只吃一份 include，一行目錄本來就帶進那裡所有種類的測試，分開的清單反而讓人以為只跑一種；合起來之後 testharness.js 那份的目錄多帶進 7 個 reftest、41 個 crashtest，照第 10 條記下實際的結果。不選照種類分兩次跑、各用自己的清單（A2）：每次多約 5 秒的啟動，清單的分法也和 wptrunner 不同。Chromium 的 `run_wpt_tests.py` 也一次跑所有種類。
14. **reftest 預期失敗的理由照第 9 條**：整組的說明寫成 `.ini` 上方的 `#` 註解，`[ Skip ]` 寫成 `disabled:` 加一行理由。
15. **失敗的 reftest 的截圖由 `run.py` 從 wptrunner 的 log 寫出**（C1）：`-actual.png`、`-expected.png`、`-diff.png`（用 venv 裡 wptrunner 本來就要的 Pillow），每個失敗的 reftest 都寫，預期的也一樣（`--reftest-screenshot fail`），和換之前的 runner、Chromium 的結果頁一樣。不選只留在 log 裡（C2，base64）：查排版的 bug 要看圖，WPT 的 checkout 裡也沒有看圖的工具。
16. **include 範圍裡的 crashtest 開始跑，結果照實記下**（D1）：kun 會當掉的記成 `CRASH`。不選先不跑（D2）：它們很快，也是 renderer 的回歸測試。

2026-10-10，第 3 段開始前確認：

17. **`--automation-pipe` 的 device scale factor 用現有的 `--scale`**（a）：它原本只給 `--headless`，擴大到 `--automation-pipe`（真的視窗照舊用螢幕的）；`kun --help` 寫出對應 Chromium 的 `--force-device-scale-factor`。不選另外加 Chromium 的名字（b）：同一件事會有兩個選項。
18. **`hidpi/` 只在它的倍率的 subsuite 跑**（a）：wptrunner 的預設組照全域的 include 和 exclude 跑，有名字的 subsuite 只跑自己的 include，所以預設組用 `--exclude` 排除 `hidpi/`。原本寫「2 倍」，做的時候發現有兩個是 1.5 倍，分成兩組（2026-10-10）。不選也在 1 倍下跑（b）：它們測的是那個倍率下邊緣對齊裝置像素，1 倍下沒有意義，換之前也只在它們的倍率下跑。
19. **kun 自己的 reftest 的 `.ini` 放在 `tests/reftests/` 底下自己的 metadata 目錄**（a，像 Servo 把 `/_mozilla/` 的預期結果放在自己的測試旁邊）。不選放在 `tests/wpt/metadata/` 底下的 `_kun/`（b）：`tests/wpt/` 放的是我們對上游 WPT 的設定。
20. **`test.py reftest` 照舊只跑 kun 自己的 reftest，另外跑一次 wptrunner**（a）。不選併進 `test.py wpt`、一次跑兩個 test root（b）：只有改了排版才要跑它，`wpt` 的範圍也維持是上游的 WPT。代價：從約 2 秒變成約 13 秒（wptrunner 的啟動）。

2026-10-10，merge 前確認：

21. **`test.py wdspec` 預設不跑，要時才跑**（和 `matrix`、`fuzz` 一樣）：改了 machopodriver、`--automation-pipe`、`protocol::automation` 或 product 時跑 `test.py wpt`（它也經過 machopodriver，約一分鐘，這條路壞了會失敗），`cargo test` 裡的 `webdriver`、`automation_pipe` 測試照樣守著 driver；wdspec 只在使用者要求、或改了它的清單和預期結果時跑。不選照舊和 `wpt` 一起跑：兩者測的對象不同（瀏覽器、driver 合不合規範），driver 不常改。也不選把兩個指令合成一個、用 `--test-types` 篩選（Chromium 的 `run_wpt_tests.py`、Firefox 的 `mach wpt`）：等 WPT 的支援做完（testdriver、跑整個 manifest）時再一起重新看測試的流程。

## 待決定

沒有。

## 之後的可能

- **testdriver 和 Actions**（下一份）：Perform Actions、Element Send Keys、`postMessage`（wptrunner 回覆 testdriver 時用）、user activation。
- **跑整個 manifest**（B3）、CI、上傳 wpt.fyi：每天跑全部、輸出 wptreport（現在每次都有）。
- **在 macOS 和 Linux 上確認**：速度、process 數的公式、預期結果裡的平台條件（第 12 條）。
- **重新看 skip 的理由**：預期結果只存狀態之後，有些 skip 的理由已經不成立，例如「失敗的訊息裡有每次不同的網址」（`concurrent-failed-imports.any.js`），可以改成照實記下。
- **wasm 文件執行 WebDriver 的 script**：有了之後，wasm 的 9 個 reftest 也搬到 wptrunner（C2）。
- **讓預期會逾時的測試先開始**：Rust 的 runner 這樣排，wptrunner 只照 manifest 的逾時長短排；要改它的排程得在我們的腳本裡動 wptrunner 的內部 API，等逾時的測試多到值得時再做。
- **kun 結束得更快**：預期之外的結果重開瀏覽器時少等一點。
- **WebDriver BiDi**：testdriver 只有 BiDi 才有的部分。
