# RFC 0006：自動化的視窗驗證

- 開始日期：2026-10-05
- 狀態：✅ 已完成：2026-10-05（Windows 11；真視窗的情境還沒在 macOS、Linux 上跑過，見「之後的可能」和 `docs/limitations.md`）
- 細節：`details.md`；驗收和做過的檢查：`progress.md`

## 摘要

每一步做完時「在視窗裡手動確認」的工作，改成可以自動執行的情境。情境是一種很小的文字格式（`tests/scenarios/`），寫下人在視窗裡會做什麼、看什麼：元素用 selector 指定，檢查元素的文字、位置和局部的畫面有沒有變，不比對整頁的像素。同一份情境有兩種跑法：

- **A：沒有平台視窗的瀏覽器視窗**：真的 Viz thread 和它的 `BeginFrame`、不等 main thread 的 compositor，畫在 software 上。由 `python3 scripts/test.py scenarios` 跑（2026-10-07 起 `cargo test --workspace` 跳過它，第 18 條），macOS 和其他平台的快捷鍵各跑一次。
- **B：`kun --automation <情境>`**：`kun` 真的 winit 視窗和 wgpu，存下截圖。`python3 scripts/test.py automation` 跑全部，只在動到視窗那一層和驗收時跑。

畫面由 Viz 讀回（對應 Chromium 的 `CopyOutputRequest`），截圖前先等「目前為止的變化」畫出來（`InsertVisualStateCallback`）。AI 助手能自己跑視窗、看截圖；人只在輸入法、剪貼簿、游標、外觀這些情境做不到的地方開視窗。

## 動機

**原本的狀況**：自動測試都不經過 winit、wgpu 和 Viz thread。所以 `docs/README.md` 的「完成一步時」第 5 步要求每一步都在 `about:demo` 加上示範，再由人開視窗、照步驟看一遍；P2、P3、P4 驗收和 RFC 0001 的每一步都這樣做。問題有三個：

- **每一步都要人**：AI 助手做完一步，要等人開視窗確認才算完成。
- **只在 macOS 上看過**：Linux 和 Windows 的視窗模式從來沒有確認過。
- **抓到的錯誤沒有變成測試**：例如 RFC 0001 第 4a 步，`about:wasm-demo` 的時鐘要點一下才畫出來，是人在視窗裡看到的；headless 和測試不用真的 Viz thread 送 `BeginFrame`，所以補了 reftest 也守不住「真的 Viz thread 底下會不會又壞」。

過去的手動確認分成三類（逐項的對照見 `details.md` 的「過去的手動確認」）：

| 類別 | 例子 | 為什麼原本測不到 | 現在 |
| --- | --- | --- | --- |
| ① 頁面的行為 | `about:demo` 的按鈕、時鐘、rAF；`about:wasm-demo` 三個 guest 的計數和時鐘；`--script wasm`；script 忙碌時照樣能捲；連結和歷史 | 視窗用真的 Viz thread，並且自己組 `RendererFlags`，測試都不經過這兩處 | A（也在 B） |
| ② 平台那一層 | winit 事件的轉換、`WgpuRenderer`、改視窗大小、device 遺失 | headless 用 `SoftwareRenderer`，不經過 winit 和 wgpu | B |
| ③ 真的要人看 | 輸入法候選字視窗的位置、和其他 App 互相複製貼上、游標、外觀、捲動順不順 | 要真的 OS 或人的感覺 | 不在範圍內，維持手動 |

不在範圍內：第 ③ 類；在 OS 層級模擬輸入（Windows 的 `SendInput`、macOS 的 `CGEventPost`）；WebDriver、CDP 這類從外部 process 控制瀏覽器的協定；B 進 CI。這些見「之後的可能」。

## 設計概要

### 給寫情境的人

一份情境是一個文字檔，一行一個指令。格式（每個指令、引號、名稱、`primary` 和 `platform`）只寫在 `tests/scenarios/README.md`。例如 `demo-script.txt` 的一段：

```text
navigate about:demo
wait-loaded
capture loaded
wait-text #status "load fired: every subresource is in."
click #clicker
wait-text #count "Clicked 1 time (onclick)."
expect-differs loaded #light
screenshot demo
```

- **元素用 selector 指定**：`click #clicker` 點元素的中心，`demo.html` 加了東西、版面移動之後情境不用改。
- **等待都有時間上限**（預設 10 秒）：`wait-text`、`wait-change` 一直查到條件成立，不用固定的 sleep。
- **截圖只存檔，不判斷對錯**：pass 或 fail 只看 `wait-*` 和 `expect-*`。截圖給人或 AI 助手看「長得對不對」。

### 工作流程

「完成一步時」第 5 步（`docs/README.md`）：在 `about:demo` 加示範，寫或補一份情境，`test.py scenarios` 跑它；動到視窗那一層時再跑 `test.py automation`、看截圖（`CLAUDE.md` 列出那一層的檔案）；人只在情境做不到的地方開視窗。

## 詳細設計

### 1. 共用的 pipeline：`BrowserWindow`

原本 `window.rs` 的 `App::start` 自己建 `WebContents`、`RendererFlags`、`BeginFrameSource`、Viz thread 和 `BrowserView`。這段抽成 `shell::browser_window::BrowserWindow`，對應 Chromium 的 `content::Shell`（不含 `ShellPlatformDelegate`）：

- 參數：renderer 的工廠（`WgpuRenderer` 或 `SoftwareRenderer`）、shared image（有或沒有）、clipboard、`browser_window::Options`（`test_fonts`、`script_language`、`automation`）、renderer 訊息的 callback。
- `RendererFlags` 只在這裡組一次，`wait_for_all_pipeline_stages_before_draw` 固定是 `false`，和視窗一樣。「視窗沒有把 `--script wasm` 交給 renderer」這種錯誤，A 就測得到。
- `window.rs` 只剩 winit 事件的轉換，和套用 cursor、title、IME。

`crates/shell/tests/browser_view.rs` 是決定性的測試（手動推進 frame），留著不動。

### 2. 讀回畫面

**`DisplayMsg::RequestCopyOfOutput(CopyOutputRequest)`**：回傳 Viz 下一個畫出來的 frame。`Display` 收到要求時標成要重畫，畫面沒變也一定有一個 frame 回來；還沒有東西可畫時等第一個 frame。

- **結果經 callback 回來**：`VizMsg` 要能 `Serialize`、`Clone`（為了之後分 process），放不進 channel 的 `Sender`。照 `SharedImage` 的前例，`CopyOutputRequest` 帶一個 `#[serde(skip)]` 的 callback，在同一個 process 裡代替回傳結果的 pipe；分 process 時改成一則回應訊息（RFC 0004）。
- **`DirectRenderer::draw_frame_and_copy`**：畫完交出複本，交不出來時說明原因（`CopyFailure`）：`Unsupported`（讀不回來，要求得到 `None`，預設）或 `NotDrawn`（這次沒有東西可畫，例如視窗被遮住或縮小，要求留著等下一個真的畫出來的 frame）。`SoftwareRenderer` 複製它的像素。
- **`WgpuRenderer`** 只在 automation 模式下替 surface 加上 `COPY_SRC`（「已決定的事項」第 9 條），present 之前把 surface texture 複製到 buffer，等 GPU 做完、map 之後去掉每列的對齊填充，BGRA 換成 RGBA。surface 不支援時記一筆 error，讀回一律 `Unsupported`。device 遺失之後重建時照樣宣告。
- 讀回走 Viz thread，不經過 renderer 的 main thread，所以 script 忙碌時照樣能截圖；「忙碌時照樣能捲」就靠這一點。

**先等畫面跟上 DOM**（第 13 條）：Viz 畫的是 renderer 已經交出的 frame，可能比查詢看到的 DOM 舊幾個 frame（commit、raster、submit 是非同步的，compositor 也不等 main thread）。B1 在真視窗裡發現 `wasm-demo` 的截圖每次都停在第一個 frame；A 也一樣，只是剛好夠快。所以情境的 `capture`、`screenshot` 由 `AutomationHost` 先送 automation 專用的 `RendererMsg::RequestVisualState`：

- renderer 已經要了 main frame 時，把回答（`VisualStateReady`）當成 swap promise 跟著下一次 commit，和 input 的 ack 一樣，在含有它的 frame 交給 Viz 之後才送；沒有要時立刻回答。
- 收到回答才送 `RequestCopyOfOutput`，這時那個 frame 已經在 Viz 的佇列裡，排在讀回的要求前面。
- 不強制跑 main frame，所以「改了 DOM 卻沒要求重畫」的 bug（RFC 0001 第 4a 步那種）在截圖上照樣看得出來。
- `capture <name> now`、`screenshot <name> now` 跳過這一步：script 忙碌時 main thread 不回答，只能拿下一個畫出來的 frame。`expect-differs` 一律不等，它本來就一直讀回直到不同。

### 3. 查元素：`QueryElement`

`RendererMsg::QueryElement { query_id, selector }`，回應 `RendererHostMsg::QueryElementResult { query_id, element }`，`element` 是 `Option<QueriedElement>`：

- `rect`：元素的 border box 的聯集（block 的盒子、inline 元素在每一行的盒子、replaced 元素），經過 transform 和捲動位置，不被 clip 切，扣掉 viewport 的捲動位置，用 viewport 的 CSS px；和 `getBoundingClientRect` 一樣不含溢出的內容，`display: none` 時是 `None`。由 `paint::element_bounds` 算，它和 hit test 用同一份 pre-paint。
- `text`：元素現在的 `textContent`。
- 沒有元素符合、沒有 document，或 selector 不合法時，`element` 是 `None`。

細節：

- **只在 `RendererFlags::automation` 打開時處理**，一般瀏覽時忽略（記一筆 warning），網頁無從得知；對應 `content_shell --run-web-tests` 才有的 `internals`。`RequestVisualState` 也一樣。
- **在 renderer 的 main thread 上回答**，直接用 `style::SelectorQuery` 和 DOM 的讀取 scope，不經過 JS，所以 JS 文件和 wasm 文件都能查。
- **換成視窗座標由 `BrowserView::web_view_to_window` 負責**，和滑鼠事件換成網頁座標的方向相反、用同一份網頁 view 的 bounds（RFC 0002）。
- **位置是 main thread 最後一次 pre-paint 的**：compositor 自己捲過、main 還沒聽到的那一段不算；DOM 一解析完就回答，比第一個 frame 早。所以導航之後要先 `wait-loaded` 才點（renderer 只把 input 交給畫出來的 document），被 compositor 捲過之後不要立刻用位置。
- script 忙碌時 main thread 不回答，查詢等到時間上限；這時用 `wait-busy`、`click-at`、`wheel-at` 和 `capture <name> now`。

**捲進畫面：`ScrollIntoViewIfNeeded`**（第 20 條）。`RendererMsg::ScrollIntoViewIfNeeded { selector }`，同樣只在 automation 模式下處理：第一個符合的元素的中心不在 viewport 裡時，下一次 main frame 照 `#fragment` 的方式捲（`ScrollState::scroll_into_view`，`block: start`）。不另外回答，之後的 `RequestVisualState` 等到畫出捲動之後的 frame 才回答。情境的 `click` 和 `wheel` 指到元素時先做，`scroll-to` 只做這件事。

### 4. 情境的格式和執行

格式只寫在 `tests/scenarios/README.md`，解析在 `shell::scenario`。執行分成兩半，在兩條 thread 上，A 和 B 共用（`shell::automation`）：

- **執行器**（`automation::run`）在自己的 thread 上，一步一步跑，每 16 ms 問一次，直到條件成立或超過時間上限；它把要做的事包成 `Request` 送給視窗。input 的 `Request` 帶一個 `done`，embedder 交給視窗之後回答；`resize` 等到視窗真的換了大小才回答。
- **`AutomationHost`** 在視窗的 UI thread 上，對 `BrowserWindow` 執行每個 `Request`，用 renderer 和 Viz 的回應回答。input 交回給 embedder，照平台的方式交給視窗：A（`shell::windowless`）直接呼叫 `BrowserView`，B 合成 winit 的 `WindowEvent`。

原本打算讓執行器透過一個 trait 直接碰視窗；改成兩條 thread 傳訊息，是因為 B 的 UI thread 是 winit 的 event loop，不能停下來等 renderer 回答。

格式的設計：

- **各平台不同的快捷鍵**（第 5 條）：`primary` 修飾鍵是 macOS 的 Cmd、其他平台的 Ctrl；修飾鍵不是 Cmd 對 Ctrl 的、只有一個平台有的，寫在 `platform mac`、`platform other` 區塊裡。A 裡「用哪個平台的快捷鍵」只是 `BrowserView::set_mac` 的旗標，所以每份情境兩套都跑；B 只跑本機的。
- **`window` 指令**（第 6 條）：情境開頭指定 `script=`、`scale=`、`size=`。
- **`wait-busy`**（第 7 條）：一次查詢 300 ms 沒有回答，就當成 main thread 在忙；「忙碌時照樣能捲」要確定捲動時已經在忙，才能用短的時間上限檢查 compositor 沒有等它。
- **`expect-differs` 的元素範圍是元素現在的位置**；兩個大小不同的 frame 只比兩者都有的那一塊（第 15 條），不帶範圍時大小不同就算不同。
- 時間上限預設 10 秒（原本規劃 5 秒）：等待在條件成立時就結束，上限只決定失敗多快報出來；第一次載入要啟動 V8，給寬一點。
- A 一律存截圖，在 `target/scenario-results/<情境>/mac/`、`other/`，失敗時可以直接看。

**B 的限制**：winit 0.30 的 `KeyEvent` 有私有欄位，外面建不出來，所以按鍵直接呼叫 `BrowserView::key`，略過 winit 的按鍵轉換（那段在 `window.rs` 只有一行）。滑鼠和滾輪的事件用 `DeviceId::dummy()` 建，經過完整的轉換。

### 5. B 的執行環境

- **`kun --automation <情境> [--out <目錄>]`**：情境在開視窗之前就讀好、解析，有錯時結束碼 2、不開視窗。視窗打開 `RendererFlags::automation` 和 wgpu 的讀回，不先導航。`Request` 經 `EventLoopProxy` 交給 UI thread 的 `AutomationHost`。
- **一個 process 一份情境**（第 10 條）：winit 的 event loop 一個 process 只能建一次。`python3 scripts/test.py automation [FILTER…]` 逐一用新的 process 跑，截圖在 `target/automation-results/<情境>/`。
- **本機的快捷鍵、字型和 scale**：只跑本機作業系統的 `platform` 區塊；`window size=` 照設定開視窗，`window scale=` 只能是螢幕的，記一行說明後忽略。
- **不受使用者的輸入干擾**（第 11、12 條）：平台送來的滑鼠、鍵盤、輸入法事件都忽略，只收合成的；resize、重畫、關閉照常。視窗在最上層（`WindowLevel::AlwaysOnTop`）。
- **resize**：`request_inner_size` 讓作業系統真的改大小，收到 `Resized` 才回答；已經是那個大小時立刻回答（Windows 這時不送 `Resized`，B1 實測：沒有這一條時，改成同樣的大小會等到時間上限）。
- **拿不到畫面時**（縮小等）：讀回的要求留著等下一個畫出來的 frame，等不到就照時間上限失敗，訊息提示「視窗被藏起來了嗎」。
- **結束碼**：0 全部通過；1 有檢查失敗（印出那一行、看到什麼）或視窗有問題；2 情境檔或選項有錯。
- **什麼時候跑**（第 14 條）：動到視窗那一層（`CLAUDE.md` 列出的檔案）、RFC 和階段驗收時，其他時候等使用者要求。要顯示器和 GPU，不在 CI 上（第 19 條）。

### 測試

- 解析器、執行器、`QueryElement`、`RequestVisualState`、`RequestCopyOfOutput`、wgpu 讀回的格式轉換、`kun --automation` 的選項，各有單元或端到端的測試；每一步都做了 mutation test（`progress.md`）。
- 情境檔本身就是 A 的測試，也是 B 的內容：`demo-script`、`demo-busy`、`wasm-demo`、`script-wasm`、`navigation`、`resize`、`gpu-crash`。
- 情境守不住、只靠審查或手動的（忽略平台的輸入、最上層的視窗、真視窗的 `window size=`、輸入法和剪貼簿），記在 `docs/test-gaps.md` 的「測試守不住的地方」。

### 分階段

1. **A1：基礎**：`BrowserWindow`、`RequestCopyOfOutput`（`SoftwareRenderer`）、`QueryElement`、情境的解析器和執行器、A 的測試入口。
2. **A2：情境**：P3、P4、RFC 0001 的視窗確認改寫成情境；`primary`、`platform`、`window`、`wait-busy`。
3. **B1：`kun --automation`**：真視窗、wgpu 的讀回、合成的 winit 事件、`RequestVisualState`、`test.py automation`。
4. **B2**：`resize`、`about:gpucrash` 的情境；工作流程的文件。

## 缺點

- **正式的程式裡多了只給測試用的東西**：`RendererFlags::automation`、`QueryElement`、`RequestVisualState`、`--automation`。用旗標關著，但要一直維護，分 process 時也要跨 process。
- **A 用真的時間**：比 `browser_view.rs` 慢，也比較容易不穩定，只能靠「等到條件成立」和夠寬的時間上限。B1 修正之前，真視窗跑 100 次失敗過 1 次，原因沒有找到（`progress.md`）。
- **寫情境有幾個要記得的規則**：導航之後先 `wait-loaded`、忙碌時用 `now`、`wait-change` 會被版面推移騙過、捲動量不要剛好是圖案的週期。都寫在 `tests/scenarios/README.md`。
- **B 不在 CI 上**，只在開發機上跑時有保護。
- **鍵盤略過 winit 的轉換**；輸入法、跨 App 剪貼簿、外觀、順不順還是要人。

## 理由和其他做法

完整的比較見 `details.md` 的「沒選的做法」。

| | 選的做法 | 其他做法 |
| --- | --- | --- |
| 驅動視窗 | 在 winit 事件或 `BrowserView` 那一層注入，對應 CDP 的 `Input.dispatchMouseEvent` | OS 層級的注入（Chromium 的 `ui_controls`）：要處理 focus、各平台的權限，不穩定；多測到的只有 OS 到 winit 那一段 |
| 指定元素 | selector，renderer 回答範圍和文字 | 寫死座標：`demo.html` 每一步都會加東西，座標一移動所有情境都要改 |
| 檢查結果 | 元素的文字和範圍，加上局部的「有沒有變」 | 整頁比對像素：系統字型各平台不同；reftest 已經涵蓋需要像素精確的地方 |
| 讀回畫面 | Viz 的 `RequestCopyOfOutput`，先等畫面跟上 DOM（不強制重畫） | 測試裡包一層 `SoftwareRenderer`（B 要再做一套）；只拿下一個畫出來的 frame（比 DOM 舊）；強制重畫（替 bug 補畫） |
| 情境寫成 | 文字檔，A、B 共用 | 寫成 Rust 的測試：B 要另外寫一套，改一份情境就要重編 |
| 外部控制 | 無 | WebDriver 或 CDP 的 server：範圍大很多，現在不需要 |

## 先例

- **Chromium 的 `browser_tests`**：整個瀏覽器在測試的 process 裡跑，有真的 message loop，不經過 OS 的輸入。和 A 相同。
- **`interactive_ui_tests`**：用 `ui_controls` 在 OS 層級注入輸入，要搶 focus，只能一個一個跑。B 不在 OS 層級注入，但和它一樣在跑的時候佔住視窗（忽略使用者的輸入、在最上層）。
- **web tests**：`content_shell --run-web-tests` 才有 `testRunner`、`eventSender`、`internals`，畫面用 `CopyOutputRequest` 讀回。`RendererFlags::automation` 和 `RequestCopyOfOutput` 對應這一套。
- **`RenderFrameHost::InsertVisualStateCallback`**：browser test 用它等到目前為止的變化都畫出來；`RequestVisualState` 對應它。
- **CDP**：`DOM.getBoxModel` 查元素的範圍、`Input.dispatchMouseEvent` 從 `RenderWidgetHost` 那一層注入、`Page.captureScreenshot` 讀回畫面。B 的三個動作分別對應它們，但不是協定，而是一份情境檔。

各項的細節見 `details.md` 的「Chromium 怎麼做」。

## 已決定的事項

1. **2026-10-05：範圍是 A 和 B 都做**。原本也考慮過只做 A，或兩邊都只做最小的一版（A 只有 3 到 4 個情境，B 只有截圖）。只做 A 的話，第 ② 類還是要人，AI 助手也還是不能自己確認視窗。
2. **2026-10-05：畫面由 Viz 讀回（`DisplayMsg::RequestCopyOfOutput`），A、B 共用**（「詳細設計」第 2 節）。不選只在測試裡把 `SoftwareRenderer` 包一層：B 還要另外做一套。
3. **2026-10-05：元素用 selector 指定，由 automation 專用的 `QueryElement` 回答位置和文字**（第 3 節），只在 `RendererFlags::automation` 打開時處理。不選寫死座標：`demo.html` 每一步都會加東西，座標一移動所有情境都要改。
4. **2026-10-05：情境寫成文字檔，A、B 共用**（第 4 節）。不選寫成 Rust 的測試：B 要另外寫一套，每改一份情境都要重編。
5. **2026-10-05（A2）：各平台不同的快捷鍵用 `primary` 修飾鍵和 `platform` 區塊寫在同一份情境裡**（第 4 節）。`primary` 是 macOS 的 Cmd、其他平台的 Ctrl，涵蓋最常見的差異（重新整理、移到網址列、全選和剪貼簿）；修飾鍵不是 Cmd 對 Ctrl 的（以詞移動是 Option 對 Ctrl，上一頁是 Cmd+← 對 Alt+←）和只有一個平台有的（Cmd+[、Alt+D）寫在 `platform mac`、`platform other` 區塊裡。不選每個平台一份檔案：共同的步驟要重複寫；不選只用各平台相同的鍵：平台不同的快捷鍵就不走完整的路徑。A 的快捷鍵平台只是 `BrowserView::set_mac` 的旗標，和執行的作業系統無關，所以**每份情境兩套都跑**；B 只跑本機的。
6. **2026-10-05（A2）：視窗的設定由情境開頭的 `window` 指令指定**（`window script=wasm scale=2`），只能在第一個其他指令之前。不選用檔名區分（組合一多就難讀）、不選在測試入口另外列（設定和情境分開，容易對不上）。
7. **2026-10-05（A2）：新指令 `wait-busy`**：等到一次查詢 300 ms 沒有回答，當成 renderer 的 main thread 在忙。「忙碌時照樣能捲」的情境要確定捲動時 main thread 已經在忙，才能用短的時間上限檢查 compositor 沒有等它。不選只靠順序（點完立刻捲）：捲動可能在忙之前就處理完，抓不到 compositor 在等 main thread 的錯；不選留給 B 或手動。
8. **2026-10-05（A2）：A2 的情境**：`demo-script`（補齊）、`demo-busy`、`wasm-demo`、`script-wasm`、`navigation`，以人手動看的 `about:demo`、`about:wasm-demo` 為主，需要控制時機或版面時才在 `tests/scenarios/` 底下另外放測試頁。2x 的細框、改視窗大小、`about:gpucrash`、被遮住之後恢復留給 B，真實網站維持手動。
9. **2026-10-05（B1）：wgpu 只在 automation 模式下替 surface 加上 `COPY_SRC`，不支援就讓讀回失敗、寫明原因，不做 fallback**（第 2 節）。2026-10-05 在 Windows 11（Vulkan，AMD Radeon RX 7900 XTX）上實測 surface 支援 `COPY_SRC`；macOS（Metal）還沒查。平常的視窗不宣告，Metal 的 `framebufferOnly` 最佳化照樣保留。不選一律先畫到離屏的 texture（每個 frame 多一次複製，平常的視窗也要付）；fallback 等真的有平台不支援再做。
10. **2026-10-05（B1）：`kun --automation <情境> [--out <目錄>]`，一個 process 跑一份情境；`python3 scripts/test.py automation` 逐一跑 `tests/scenarios/` 的每一份**（第 5 節）。winit 的 event loop 一個 process 只能建一次；不選一個視窗跑多份（上一份的歷史紀錄、網址列、捲動位置會留給下一份）。真視窗只跑本機作業系統的 `platform` 區塊；`window size=` 照設定開視窗，`window scale=` 只能是螢幕的，印一行說明後忽略。
11. **2026-10-05（B1）：automation 期間忽略平台送來的輸入，`resize` 用 `request_inner_size` 讓作業系統真的改大小，等到 `Resized` 才繼續**（第 4、5 節）。不然使用者的滑鼠移過視窗，合成的點擊會打到別的地方；不選合成 `Resized`：沒測到作業系統那一段。
12. **2026-10-05（B1）：automation 期間視窗在最上層（`WindowLevel::AlwaysOnTop`），等不到畫面時失敗並說明原因**（第 5 節）。macOS 上視窗被遮住時 surface 不給畫面，縮小時各平台都不畫；不選只在失敗時說明（被別的 App 擋住就失敗）。
13. **2026-10-05（B1）：`capture` 和 `screenshot` 預設等到「目前為止的變化」都畫出來才讀回，不強制重畫；忙碌期間用 `now`**（第 2 節）。B1 在真視窗裡發現：查詢看的是 main thread 的 DOM，讀回的是 Viz 下一個畫出來的 frame，中間隔著 commit、raster、submit，`wasm-demo` 的截圖每次都停在第一個 frame（點了兩次之後還是「Not clicked yet.」）。A 也一樣，只是剛好夠快。新的 automation 專用訊息 `RequestVisualState`：renderer 已經要了 main frame 時，跟著下一次 commit，含有它的 frame 交給 Viz 之後才回答；沒有要時立刻回答（對應 Chromium 的 `RenderFrameHost::InsertVisualStateCallback`）。不強制跑 main frame：強制會替「改了 DOM 卻沒要求重畫」的 bug 補畫（RFC 0001 第 4a 步就是這種），截圖就看不出來；不選維持「下一個畫出來的 frame」（截圖不可靠，`capture` 當基準可能假通過）、不選讀到連續兩個 frame 相同（不保證和 DOM 一致，有動畫的頁面永遠不穩定）。script 忙碌時 main thread 不回答，`capture <name> now`、`screenshot <name> now` 拿下一個畫出來的 frame；`expect-differs` 本來就一直讀回直到不同，維持不等。
14. **2026-10-05：`test.py automation` 和 `matrix`、`fuzz` 一樣只在特殊狀況跑**：動到視窗那一層（`window.rs`、`browser_window.rs`、情境的執行器、`WgpuRenderer`、Viz thread）、RFC 和階段驗收時，其他時候等使用者要求（`CLAUDE.md`）。沒有平台視窗的情境照舊在 `cargo test --workspace` 裡，每一步都跑。原本 B2 打算讓真視窗的情境變成每一步的視窗確認，改成只有上述狀況；它要顯示器和 GPU，跑的時候視窗會蓋在最上層。
15. **2026-10-05（B2）：`resize` 的情境檢查「改大小之後重新排版」，不加新指令**：`about:demo` 改窄、再改寬，`.hero` 的文字在新的寬度換行，用 `expect-differs` 比對它的範圍（P2、P4 手動確認的「改視窗大小之後重新換行」）。為此 `expect-differs` 帶元素範圍時，兩個大小不同的 frame 只比兩者都有的那一塊；不帶範圍時大小不同照樣算不同。不選新增 `expect-size`（大小對了不代表頁面跟著重新排版）、不選只截圖。
16. **2026-10-05（B2）：`about:gpucrash` 的情境放在 `tests/scenarios/`，A、B 都跑**：在網址列輸入它之後，畫面還在更新、點擊還有反應、還讀得回來（device 重建之後 surface 照樣宣告 `COPY_SRC`）。A 畫在 software 上，沒有 device 可失去，只是照樣通過；真正的檢查在 B。不選另開一個只給 B 的目錄（多一種分類）。
17. **2026-10-05（B2）：wgpu 和 `SoftwareRenderer` 的比對延後，移到「之後的可能」**：真視窗用系統字型和螢幕的 scale，沒有平台視窗的用測試字型和 1x；情境有動畫和時鐘；視窗模式的 tile 在 GPU 上，software 拿不到像素，不能在同一個 Viz 畫兩次。wgpu 的 renderer 只畫四邊形、texture 和 scissor，真視窗的截圖已經看得出明顯的錯，比對的成本高、價值低。不選 GPU reftest（`tests/reftests/` 逐頁在真視窗畫，約 5 分鐘，還要處理 scale）、不選同一份情境兩邊截圖比（要另外寫靜態頁面）。
18. **2026-10-05（B2）：每一步的視窗確認改成情境**：在 `about:demo` 加示範，寫或補一份 `tests/scenarios/` 的情境（原本跟著 `cargo test` 跑；2026-10-07 起 `cargo test` 跳過情境，它們約 29 秒，佔了 `cargo test --workspace` 執行時間的三分之一以上，改由 `python3 scripts/test.py scenarios` 跑：寫了或改了情境時跑那幾份，CI 每天跑全部）；動到視窗那一層時再跑 `test.py automation`、看截圖（第 14 條）；人只在輸入法、剪貼簿、游標、外觀，或使用者要求時開視窗（`docs/README.md` 的「完成一步時」第 5 步）。不選情境之外仍然每一步都要人工確認。
19. **2026-10-05（B2）：B 不進 CI，延後**：CI 的 runner 沒有 GPU；Windows 的 WARP、Linux 的 xvfb 加 lavapipe 都要另外試，和第 14 條「真視窗只在特殊狀況跑」一致。
20. **2026-10-07：點元素之前先把它捲進畫面，新指令 `scroll-to`**（第 3、4 節）。RFC 0003 的 7 份情境要求 2600 到 3900 高的視窗，Windows 開得出來，macOS 會把視窗限制在螢幕裡（這台是 800×891），`test.py automation` 在 macOS 上 7 份都失敗。新的 automation 專用訊息 `ScrollIntoViewIfNeeded`（對應 DevTools 的 `DOM.scrollIntoViewIfNeeded`，ChromeDriver 點擊前用它捲）：元素的中心不在 viewport 裡時，renderer 下一次 main frame 照 fragment 的方式捲，之後的 `RequestVisualState` 等畫出來。`click`、`wheel` 指到元素時先做，和 WebDriver 的 Element Click 一樣；要在 `capture` 之前捲時用 `scroll-to`（點擊時才捲，`capture` 的畫面和之後的捲動位置不同，`expect-differs` 一定成立，檢查就沒有意義）。那 7 份情境改成預設的 800x600，先 `scroll-to` 再截圖。不選執行器自己送滾輪捲（游標下的捲動容器會先被捲、捲不到容器裡的元素）；不選只改情境、每份用 `wheel` 捲（之後的情境都要記得，執行器不變）；不選照 WebDriver 的 `block: end`（要另外寫對齊，對情境沒有差別）。

## 待決定

無。

## 之後的可能

- **在 Linux 上跑真視窗的情境**：macOS 2026-10-07 起跑過（Metal 的 surface 讀得回畫面）；Linux 的視窗模式從來沒確認過（`docs/limitations.md` 的「平台和建置」）。不支援時，先畫到一張離屏的 texture 再複製到 surface，只在 `--automation` 時做。
- **B 進 CI**（第 19 條）：Windows 的 runner 用 WARP（DX12 的軟體實作）、Linux 用 xvfb 加 lavapipe，要試過才知道穩不穩、要跑多久。
- **wgpu 和 `SoftwareRenderer` 的比對**（第 17 條）：GPU reftest，用 `--test-fonts` 在真視窗裡逐頁畫 `tests/reftests/`，和 headless 的結果比像素；容許的誤差要實測。
- **量化「順不順」**：接上 `tracing-chrome`，B 跑完之後輸出 frame 的間隔和漏掉的 `BeginFrame`，用門檻判斷 pass 或 fail。
- **檢查視窗大小、工具列和游標的指令**：情境現在只查得到網頁裡的元素。
- **`click <selector>` 先把元素捲進視窗再點**：像 Puppeteer、Playwright 的 click（CDP 的 `DOM.scrollIntoViewIfNeeded`）。現在元素的中心不在網頁上時直接失敗（RFC 0002 的第 13 條），要多一個 renderer 的 automation 訊息、等捲完才點。
- **OS 層級的注入**：要測 winit 的按鍵轉換、輸入法時再考慮，只用在少數情境。
- **WebDriver 或 CDP 的 server**：要讓外部的工具（WPT 的 `testharness.js` runner、Puppeteer）控制 kun 時；情境的執行器可以當成它的第一個 client。
