# RFC 0029 的細節：用 wptrunner 跑 WPT

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 2026-10-09 的實測

Windows 11，i9-13900K（8 個 P-core 加 16 個 E-core，32 個執行緒），64 GB；`kun`、`machopodriver` 和 Rust 的 runner 都是 optimized 的 dev build；WPT 在 `tests/wpt/README.md` 寫的 commit。

**做法**：

- 先用 Rust 的 runner 跑（`test.py wpt`、`test.py reftest wpt/css/css-flexbox`），從它印出的結果取出實際跑了的測試，寫成 wptrunner 的 `--include-file`（`.any.js` 換成 `.any.html`、`.window.js` 換成 `.window.html`），兩邊跑的是同一份清單：testharness 全部 1653 個、其中 `dom/nodes/` 的 321 個、css-flexbox 的 171 個 reftest。
- wptrunner 照 `python -m wptrunner_machopo.run`（wdspec 的那一份）跑，用一個包裝改它的參數（process 數、`--no-restart-on-unexpected` 等）。沒有給 wptrunner 預期結果，所以加了 `--no-restart-on-unexpected`，不然每個失敗的子測試都會重開瀏覽器。
- 結果一致性：從 wptrunner 的原始 log 取出每個測試的狀態和每個子測試的狀態，和 `4ff8d6e:tests/wpt/baselines/` 的 `-expected.txt` 比（沒有 `-expected.txt` 的就是全部通過）。
- 量測的腳本和 log 沒有放進 repo。

### 結果一致嗎

testharness 全部：1653 個裡 1645 個的狀態和子測試的狀態都和 `-expected.txt` 一樣。不一樣的 8 個：

| 測試 | 差別 | 原因 |
| --- | --- | --- |
| `css/css-flexbox/align-items-baseline-row-horz.html`、`text-as-flexitem-size-001.html`，`css/css-grid/subgrid/subgrid-baseline-016.html`、`-018.html` | 只在 wptrunner 有失敗的子測試 | 字型：product 沒有帶 `--test-fonts`，kun 用系統的字型；Rust 的 runner 用 Ahem 和 Lato（「待決定」的 F） |
| `html/interaction/focus/processing-model/preventScroll.html` | `Sanity test` 只在 Rust 的 runner 失敗 | 焦點：wptrunner 每個測試前點一下新分頁的 `documentElement`，分頁有焦點；Rust 的 runner 不點 |
| `css/cssom-view/mouseEvent-offsetXY-svg.html` | 兩邊都是 harness 的錯誤，子測試的狀態不同 | testdriver：wptrunner 真的執行 testdriver 的請求，回覆時呼叫 `window.postMessage`，kun 沒有，丟出 `javascript error`（另外兩個測試的 log 裡也有同樣的錯誤，但狀態和預期結果相同） |
| `dom/nodes/processing-instruction-attributes.html`、`html/webappapis/timers/evil-spec-example.any.html` | 比對腳本的問題 | 前者的子測試名稱有換行，後者的 harness 訊息裡有 `setTimeout`，被腳本當成逾時；實際相同 |

reftest：css-flexbox 的 171 個，兩邊失敗的是同樣的 12 個（就是 `reftest-expectations.txt` 裡那 12 個）。

### 速度

**wptrunner 的預設設定**（每個 process 是一個 Python 的 worker、一個 machopodriver 和一個 kun）：

| | Rust 的 runner（最多 8 個 worker） | wptrunner，1 個 process | 8 個 | 16 個 |
| --- | --- | --- | --- | --- |
| testharness 全部 | 28 秒 | — | 91 秒 | 69 秒 |
| `dom/nodes/` | 14 秒 | 121 秒 | 45 秒 | — |
| css-flexbox 的 reftest | 5 秒 | 40 秒 | 20 秒 | — |

時間花在哪裡（testharness 全部，8 個 process）：

- **每個測試本身不慢**：從開始到結束的中位數 0.064 秒，九成在 0.095 秒以內。WebDriver 和 Python 的來回不是瓶頸。
- **等逾時的測試**：26 個測試各等了 10 秒，合計 243 秒，是所有測試時間（436 秒）的一半以上。Rust 的 runner 讓預期會逾時的測試各自一個 thread、最先開始，wptrunner 沒有這種排法。
- **分配不平均**：預設的 `SingleTestSource` 照測試 id 的 hash 把測試靜態地分給各 process（「wptrunner 的分組和排程」），16 個 process 的忙碌時間從 10 秒到 51 秒不等。
- **開始前約 12 秒**：wptrunner 每次檢查整個 checkout 的 manifest（`--manifest-update` 是預設）。

**調整之後**（`--fully-parallel`、`--no-restart-on-new-group`、`--no-manifest-update`）：

| | Rust 的 runner | wptrunner，16 個 process | 32 個 |
| --- | --- | --- | --- |
| testharness 全部 | 28 秒 | 43 秒 | 34 秒 |
| `dom/nodes/` | 14 秒 | — | 19.6 秒 |
| css-flexbox 的 reftest | 5 秒 | — | 9.7 秒 |
| 只跑 `dom/nodes/Element-hasAttribute.html` | 3.9 秒 | 6.2 秒（1 個 process） | — |

- 開始前降到約 5 秒，32 個 process 的忙碌時間 10 到 25 秒。結果和調整之前相同。
- 只跑一個測試時的 6.2 秒：載入 manifest 約 1.7 秒、啟動 wptserve 約 1.5 秒、啟動 machopodriver 和 kun 約 1.3 秒、測試 0.1 秒、結束約 1.5 秒。Rust 的 runner 的 3.9 秒包括 cargo 檢查要不要重編。
- **預期之外的結果重開瀏覽器**：`dom/nodes/` 在沒有預期結果、打開 `restart_on_unexpected`（預設）時重開了 90 次，從 19.6 秒變成 29.8 秒，一次約 3.5 秒。wdspec 的文件寫的「每次多等 10 秒」是當時（第一次跑 wdspec）的觀察，這次沒有再量。

**限制核心數**：用 `start /affinity` 把整個 process 樹限制在 8 個 P-core（mask `0x5555`，每個 P-core 一個執行緒，模擬 8 核的機器）或 4 個（`0x55`），兩個 runner 都在同樣的限制下跑。wptrunner 用調整之後的設定。

| | 不限制 | 8 核 | 4 核 |
| --- | --- | --- | --- |
| Rust 的 runner，testharness 全部 | 28 秒 | 25.6 秒 | 28.6 秒 |
| wptrunner，8 個 process | — | 68.1 秒 | 69.7 秒 |
| wptrunner，16 個 process | 43 秒 | 42.7 秒 | 46.5 秒 |
| wptrunner，32 個 process | 34 秒 | 33.4 秒 | 47.6 秒 |
| Rust 的 runner，css-flexbox 的 reftest | 5 秒 | 4.6 秒 | 5.5 秒 |
| wptrunner，8 個 process | — | 10.1 秒 | 11.0 秒 |
| wptrunner，32 個 process | 9.7 秒 | 10.2 秒 | 15.0 秒 |

- process 不必等於核心數：它們大多在等，8 核開 32 個和不限制時一樣快；4 核在 16 個左右就到頂。

**模擬 M4**（4 個效能核心加 6 個節能核心，mask `0x3F0055`：4 個 P-core 各一個執行緒，加上 6 個 E-core。Intel 的 E-core 和 P-core 的差距比 M4 的大，所以這是偏保守的估計，數字要在 M4 上再量）：

| | 時間 |
| --- | --- |
| Rust 的 runner，testharness 全部 | 27.3 秒 |
| wptrunner，16 個 process | 47.1 秒 |
| wptrunner，24 個 process | 38.7 秒 |
| wptrunner，32 個 process | 34.1 秒 |
| Rust 的 runner，css-flexbox 的 reftest | 5.3 秒 |
| wptrunner，16 個 process | 10.3 秒 |
| wptrunner，32 個 process | 11.4 秒 |

16 個 process 時和 4 核一樣（47 秒左右），表示那時受限的是 process 數，不是 CPU；加上 6 個 E-core 之後，32 個 process 和 8 核時差不多。
- 32 個 kun 同時跑，記憶體（working set）合計最多約 2.5 GB，每個約 77 MB。
- 每次的結果一致：Rust 的 runner 每次都符合預期；wptrunner 每次和沒有預期結果比，預期之外的數目相同，只有 32 個 process 的兩次多了一個（可能是負載下對時間敏感的測試，沒有查是哪一個）。

**Rust 的 runner 卡住一次**：限制在 8 核的第一次跑了 107.8 秒，24 個測試到了 runner 的 25 秒期限而失敗（平常不到 0.1 秒）。這 24 個照名稱是連續的（`css/cssom-view/` 的 `getBoundingClientRect-*` 到 `image-x-y-zoom.html`），像是同一段時間裡有東西卡住，不是那幾個測試本身。同樣的設定再跑兩次都正常（25.6、25.7 秒），4 核的那次也正常。沒有追原因：runner 這份要拿掉；如果原因在 kun 或 wptserve，換成 wptrunner 之後還會再看到。

## wptrunner 的分組和排程

在 WPT 的 checkout 的 `tools/wptrunner/wptrunner/testloader.py` 和 `testrunner.py`：

- **`SingleTestSource`**（預設）：每種測試照 `hash(test.id) % processes` 分成 `processes` 組，每組固定給一個 process，事先分好。逾時的測試剛好集中在某幾組時，那幾個 process 就拖到最後。
- **`FullyParallelGroupedSource`**（`--fully-parallel`）：每個測試自己一組，各 process 從同一個佇列拿下一組。組照 `sum(test.timeout)` 由大到小排，所以 `timeout=long` 的先開始；它不知道哪些測試會逾時（預期結果裡的 `TIMEOUT` 不算進去）。
- **換組時重開瀏覽器**：`restart_on_new_group` 預設打開，`--fully-parallel` 時等於每個測試都重開，要配 `--no-restart-on-new-group`。
- **`--run-by-dir`**：照目錄分組，同一個目錄在同一個 process；這次沒有量。

## 自己改寫 runner 的成本

要讓 Rust 的 runner 和 wptrunner 的行為一致（W3），要對齊的部分：

| 部分 | 現在的差別 | 對齊要做的 |
| --- | --- | --- |
| testharness 的結果 | 字型、分頁的焦點（「結果一致嗎」） | 用測試字型、每個測試前點一下 `documentElement` |
| reftest 的判斷 | 這次的 171 個一樣 | 已經照 wptrunner 寫了（`4ff8d6e:crates/shell/src/reftest.rs`），上游改了要跟著改 |
| 測試的清單 | 自己的規則：`timeout=long`、`variant`、`.any.js` 的 global、crashtest、print-reftest | 改讀 `MANIFEST.json`（Python 本來就要裝） |
| 逾時和狀態 | harness 自己計時；wptrunner 由 runner 計時（`explicit_timeout`），狀態多了 EXTERNAL-TIMEOUT、PRECONDITION_FAILED 等 | 照 wptrunner 改計時和狀態 |
| testdriver | 沒有 | `actions.py` 的 52 個動作、`asyncactions.py` 的 20 個 BiDi 的，每個是「頁面送來的請求 → 一串 WebDriver 指令」，都要用 Rust 重寫；上游一直在加 |
| wptreport | 沒有 | 格式能自己產生，但不是 wptrunner 跑出來的 |

wptrunner 裡和這些有關的程式約 8900 行（`executorwebdriver.py`、`base.py`、`protocol.py`、`actions.py`、`testrunner.py`、`testloader.py`、`wpttest.py`、`testdriver-extra.js` 等），加上頁面那一側的 WPT 的 `/resources/testdriver.js` 約 2900 行。對齊之後只能靠兩邊都跑來檢查有沒有再分開，等於兩套都要維護。

## kun 自己的 reftest 能不能搬到 wptrunner

`tests/reftests/` 有 115 個 reftest（2026-10-09，有 `match` 或 `mismatch` 的檔案）。搬到 wptrunner 的話，照 Servo、Firefox 的 `/_mozilla/`，在 wptrunner 的設定加一個 test root（例如 `/_kun/`），wptserve 跟著提供它；它們改從 HTTP 載入，不再是 `file:`。大部分直接能跑，要處理的是用了 WPT 沒有的東西的那些：

| 哪些 | 數目 | 為什麼 wptrunner 不能直接跑 | 做法 |
| --- | --- | --- | --- |
| `hidpi/`（`kun-device-scale-factor`） | 6 | wptrunner 的 reftest 一律 device pixel ratio 1 | wptrunner 的 subsuite：一組測試配一組瀏覽器的參數（Chromium 的 virtual suite 走同一條路，`subsuite.config` 的 `binary_args`）；kun 要能在 `--automation-pipe` 時指定 device scale factor（現在 `--scale` 只給 `--headless`） |
| `scroll/` 的 `kun-fragment` | 5 | 測試的網址來自檔名，不能帶 `#fragment` | 改寫：script 在 `reftest-wait` 期間設 `location.hash`（WPT 的捲動測試都用 script），或用 `src` 帶 fragment 的 iframe |
| `wasm/`（`<meta name="script" content="wasm">` 的文件） | 9 | wptrunner 的 reftest 在測試頁裡執行 `test-wait.js`（Execute Async Script），wasm 的文件不執行 WebDriver 的 script（`unsupported operation`） | 見下面 |
| `harness/` | 9 | 測的是 Rust 的 runner 自己（chain、`mismatch`、壞掉的 chain 預期失敗） | 拿掉：wptrunner 的判斷是上游的 |
| `wasm/w9-lua.html` 的 `../../../resources/kun-lua.wasm` | 1 | 在 test root 外面，wptserve 不提供 | 在設定裡加一個路徑對應，或改放的位置 |

`requestAnimationFrame` 的虛擬時鐘：用到 `requestAnimationFrame` 的（`script/j5-animation-frames.html`、`invalidation/` 用的 `after-frames.js` 等）只檢查順序、間隔和 frame 數，不看確切的時間，真的時間也能跑。

wasm 的文件的做法：

- **WebDriver 的 script 在 wasm 的文件裡也能執行**：wasm 的文件沒有 V8 的 context，要另外給它一個只給 WebDriver 用的 JS 世界（像 Chromium 的 isolated world），DOM 要同時接上兩個 engine。工作量大，要自己的 RFC。
- **用 iframe 包起來**：JS 的測試頁裡放一個載入 wasm 文件的 iframe，`test-wait.js` 在外面的 JS 文件裡執行，截圖照樣包括 iframe。外面的文件只知道 iframe 的 `load`，不知道裡面的 `reftest-wait` 什麼時候拿掉（`wait.html`、`w5-animation-frames.html` 用它）；JS 能不能讀 wasm 文件的 DOM 也還沒確認。要原型。
- **留成一個小的 `cargo test`**：這 9 個經過 `headless` render 測試頁和參考頁、比對像素，沒有清單、預期結果、chain、`fuzzy` 和平行，不算 runner。

## 預期結果的格式

| | A1：wptrunner 的 `.ini` | A2：Chromium 的 `-expected.txt` |
| --- | --- | --- |
| 誰用 | Firefox、Servo、Ladybird；kun 的 wdspec（`tests/wpt/metadata/`） | Chromium（`run_web_tests.py` 和 `run_wpt_tests.py` 共用） |
| 內容 | 每個測試檔一份，harness 和每個子測試的狀態，可以加平台條件（`if os == "win": FAIL`）和 `disabled: 理由` | 每個測試檔一份，沒有通過的子測試、它的訊息和 harness 的狀態，像 runner 印出的文字 |
| 比對 | wptrunner 自己比狀態 | 要自己寫：執行前轉成 wptrunner 的預期結果，或執行後從 wptreport 產生 `-actual.txt` 再比文字 |
| 平台的差別 | 原生的條件 | 現在是 skip 清單的平台標籤；Chromium 用 `platform/<名稱>/` 底下的 baseline |
| 失敗的訊息 | 不存，只在 log 和 wptreport 裡 | 存在預期結果裡，審查時看得到為什麼失敗；訊息變了也算不同 |
| 轉換 | 740 個 `-expected.txt` 和 `reftest-expectations.txt` 轉一次 | 不用轉 |

當初選 `-expected.txt` 時不選 `.ini` 的理由是「`.ini` 是 wptrunner 自己的格式」，和「一份檔裡上千行 subtest 不好審查」（後者說的是一份總的 expectations 檔，`.ini` 也是每個測試檔一份）。

## 各家的做法

| | 跑 WPT 的 runner | 預期結果 | 自己的測試 |
| --- | --- | --- | --- |
| Chromium | `run_wpt_tests.py`：wptrunner 加 chromedriver，預設的 suite 是 `headless_shell`；`run_web_tests.py`（content_shell）是舊的那一條，還有一部分 WPT 留在那裡 | 兩條路共用 `-expected.txt` 和 TestExpectations | 非 WPT 的 web test 由 `run_web_tests.py` 跑 |
| Firefox | wptrunner 加 Marionette | `.ini`（`testing/web-platform/meta`） | `testing/web-platform/mozilla/tests`，在 wptrunner 裡是 `/_mozilla/` |
| Servo | wptrunner | `.ini`，要跑哪些用 `include.ini` | `/_mozilla/`，一樣經過 wptrunner |
| Ladybird | wptrunner 加自己的 WebDriver | — | — |
| WebKit | 自己的 `run-webkit-tests` 和 WebKitTestRunner，testdriver 用自己的 `testdriver-vendor.js` | 自己的格式 | 同一個 runner；Safari 在 wpt.fyi 的結果另外用 wptrunner 加 safaridriver 跑 |

## 參考資料

- WPT 的 checkout 的 `tools/wptrunner/wptrunner/`（`testloader.py`、`testrunner.py`、`wptcommandline.py`、`executors/executorwebdriver.py`、`executors/actions.py`、`executors/asyncactions.py`）。
- Chromium，[Running Web Platform Tests with run_wpt_tests.py](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/testing/run_web_platform_tests.md)。
