# RFC 0006 的細節：自動化的視窗驗證

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 過去的手動確認

2026-10-05 整理 `rfcs/pre-rfcs/PROGRESS.md` 和 `rfcs/0001-wasm-scripts/progress.md` 的視窗確認，逐項對照之後由誰接手。類別同 `README.md` 的「動機」。

| 什麼時候 | 確認的項目 | 類別 | 之後由誰 |
| --- | --- | --- | --- |
| P2（2026-10-01） | `about:demo` 和網站快照的載入、inline box、圖片 | ① | A（截圖給 B） |
| P2 | 滾輪、觸控板、鍵盤捲動；巢狀捲動容器；改視窗大小時的捲動位置 | ①、② | 捲動的結果：A；觸控板的 `PixelDelta` 和改大小：B |
| P2 | 連結、上一頁、下一頁、重新整理的快捷鍵、連結上的游標、`#` 連結 | ① | A（游標由 `BrowserView::cursor` 查） |
| P2 | 工具列的外觀、網址列的 focus 和選取、輸入網址、Esc 還原 | ①、② | A；外觀看 B 的截圖 |
| P2 | 中文輸入法（組字的底線、候選字視窗的位置）、和其他 App 之間的複製和貼上 | ③ | 人 |
| P2 | 真實網站（`https://text.npr.org/`） | — | 要連網，維持手動（`docs/test-gaps.md` 的「只做過手動確認」） |
| P3（2026-10-02） | 按鈕計數、被 `preventDefault` 擋住的連結、兩秒後變綠的方塊、`setInterval`、rAF、`load` 的訊息 | ① | A |
| P3 | Viz thread 的 `BeginFrame`：動畫不停、`setTimeout` 準時、捲動時動畫照樣在跑 | ① | A（真的 Viz thread）；「順不順」是 ③ |
| P4（2026-10-04） | 整頁和容器的捲動、Busy 3 秒期間照樣能捲、時鐘停住後恢復 | ① | A（`capture` 和 `expect-differs`） |
| P4 | 2x 時 0.5px 的細框和細紋 | ② | B 的截圖，在 Retina 上 |
| P4 | invalidation 的 log 數字（每個 frame 重畫多少） | ① | 已有單元測試；log 的數字之後可以讓情境檢查（不在初版） |
| P4 | 被遮住再切回來畫面恢復、記憶體平穩；連續改視窗大小不 panic、沒有黑色的 tile | ② | 改大小之後重新排版：情境 `resize`（A、B）；被遮住要真的切 App、記憶體要長時間看，維持手動 |
| P4 | `about:gpucrash` 之後 device 重建、畫面恢復 | ② | 情境 `gpu-crash`（真正的檢查在 B） |
| RFC 0001（2026-10-05） | `performance.now()` 按鈕、`about:wasm-demo` 三個 guest 的計數和時鐘、換回 `about:demo` 時換回 V8 | ① | A |
| RFC 0001 | `kun --script wasm`：engine 是 wasmtime、直接開 `about:wasm-demo` | ① | A（共用的 pipeline 讓視窗的旗標也被測到） |

## Chromium 怎麼做

- **`browser_tests`**（`chrome/test/base/in_process_browser_test.h`）：瀏覽器的 process 就是測試的 process，有真的 UI thread 和 message loop，也有真的 renderer process。輸入多半由 `content::SimulateMouseClick` 這類函式從 `RenderWidgetHost` 那一層送，不經過 OS。可以平行跑。
- **`interactive_ui_tests`**：需要真的 focus 和 OS 輸入的測試（拖放、快捷鍵、focus 的移動）。用 `ui_controls::SendMouseMove`、`SendKeyPress` 在 OS 層級注入，因為要搶 focus，所以同一台機器上一次只跑一個。
- **web tests**：`content_shell --run-web-tests`，頁面裡多了 `testRunner`、`eventSender`、`internals`。`eventSender` 從 renderer 那一層送輸入，`internals` 讓測試讀內部的狀態。畫面用 viz 的 `CopyOutputRequest` 讀回來，和預期的 PNG 比對。
- **`CopyOutputRequest`**（`components/viz/common/frame_sinks/copy_output_request.h`）：掛在一個 render pass 上，下一次畫的時候把結果複製出來，非同步地交給要求的人。可以要求整個 pass 或其中一塊。
- **CDP**：`DOM.getBoxModel` 回傳元素的 content、padding、border、margin 四個框；`Input.dispatchMouseEvent` 和 `Input.dispatchKeyEvent` 從 `RenderWidgetHostImpl` 那一層注入；`Page.captureScreenshot` 透過 `CopyOutputRequest` 讀回畫面。ChromeDriver 和 Puppeteer 都建在 CDP 上。

**kun 照做的**：注入的那一層（瀏覽器內部，不是 OS）、讀回畫面的語意（下一個畫出來的 frame）、測試專用的介面用旗標關著。
**不照做的**：沒有協定和外部的 client；情境是一份檔案，在瀏覽器的 process 裡執行。

## 沒選的做法

### OS 層級的注入

例如 Windows 的 `SendInput`、macOS 的 `CGEventPost`、Linux 的 XTest，再加上截整個螢幕。

- 好處：連 OS 到 winit 的那一段也測到，包括按鍵的轉換。
- 壞處：
  - 要搶 focus，跑的期間不能用電腦，也不能平行跑。
  - 各平台各寫一套。macOS 要另外給終端機輔助使用和螢幕錄製的權限。
  - 截螢幕會截到其他視窗；座標還要扣掉標題列和 Retina 的縮放。
- 多測到的只有 winit 本身的行為，那是 winit 的責任。

### 外部的控制協定（WebDriver、CDP）

- 好處：可以用現成的工具（Puppeteer、Selenium、WPT 的 runner）。
- 壞處：要一個 server、一套協定的實作，還要處理多個 client 和 session。這份 RFC 只需要 AI 助手和測試能跑情境，不需要這些。
- 情境的執行器做好之後，協定的 server 可以建在它上面（`README.md` 的「之後的可能」）。

### 情境寫成 Rust 的測試

- 好處：不用新的格式，可以用 Rust 的所有東西。
- 壞處：B 要另外寫一套；每改一份情境都要重編。AI 助手想臨時確認一件事時，寫一份文字檔就能跑，比較方便。
- 需要複雜邏輯的情境，還是可以直接寫成 Rust 的測試，呼叫同一個執行器。

### 寫死座標

`demo.html` 每一步都會加示範，P4 就加了好幾次，每次版面都會移動。座標寫死的話，每加一次示範，所有情境都要跟著改。

### 整頁比對像素

`about:demo` 用的是系統字型，各平台長得不一樣，只能在一個平台上存預期的結果。需要像素精確的地方，已經由 reftest 和 WPT 涵蓋（測試字型、headless）。情境只檢查文字、範圍和局部的「有沒有變」。

## 實作上要注意的地方

- **winit 0.30.13**：`KeyEvent` 有 `pub(crate)` 的 `platform_specific` 欄位，外面建不出來。`DeviceId::dummy()` 是安全的 `const fn`，可以用來建滑鼠和滾輪的事件。
- **wgpu 30 的 surface usage**：`SurfaceCapabilities::usages` 列出這個 surface 支援的 usage，`RENDER_ATTACHMENT` 一定有，`COPY_SRC` 不保證。Metal 要關掉 `framebufferOnly` 才能複製，wgpu 會照 usage 處理。
- **讀回 surface texture 要在 present 之前**：present 之後 texture 就交還給 OS 了。複製到 buffer 之後要 `map_async` 加上 poll，在 Viz thread 上做，不擋住 UI thread。
- **`textContent` 和範圍在 main thread 上算**：fragment tree 已經有每個 box 對應的元素（hit test 用的就是這份）；從元素反查它的 fragment 要走一遍 tree（`paint::element_bounds`）。它用 hit test 的那份 pre-paint，所以 transform 和捲動容器的位移都算進去，最後再扣掉 viewport 的捲動位置，全部用 main 那邊的值。compositor 剛捲過、還沒交給 main 的那一小段不算。

## 參考資料

- Chromium：`chrome/test/base/in_process_browser_test.h`、`ui/base/test/ui_controls.h`、`content/web_test/`、`components/viz/common/frame_sinks/copy_output_request.h`。
- Chrome DevTools Protocol：`DOM.getBoxModel`、`Input.dispatchMouseEvent`、`Page.captureScreenshot`。
- winit 0.30：`winit::event::KeyEvent`、`winit::event::DeviceId::dummy`。
- wgpu：`SurfaceCapabilities::usages`、`TextureUsages::COPY_SRC`。
