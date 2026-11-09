# RFC 0006 的進度：自動化的視窗驗證

開始日期：2026-10-05。完成：2026-10-05，Windows 11（Rust 1.99.0；真視窗是 Vulkan、AMD Radeon RX 7900 XTX、螢幕 1.5x）。macOS、Linux 上還沒跑過真視窗的情境。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| A1 基礎：`BrowserWindow`、`RequestCopyOfOutput`、`QueryElement`、情境的解析器和執行器 | 完成 | 2026-10-05 |
| A2 情境：`primary`、`platform`、`window`、`wait-busy`；五份情境 | 完成 | 2026-10-05 |
| B1 `kun --automation`：wgpu 的讀回、合成的 winit 事件、`RequestVisualState`、`test.py automation` | 完成 | 2026-10-05 |
| B2 `resize`、`gpu-crash` 的情境；工作流程的文件 | 完成 | 2026-10-05 |

## 驗收

2026-10-05 在 Windows 11 上全部通過：fmt、clippy（沒有警告）、`cargo test --workspace` 693 個、reftest 193 個（`harness/chain-broken*.html` 2 個是預期中的失敗）、沒有平台視窗的情境 7 份各用兩個平台的快捷鍵跑（14 次）、`sanity.py deps`、`sanity.py unsafe`、`sanity.py docs`、`test.py automation` 7 份。

**讀回畫面**

| 項目 | 怎麼驗證 |
| --- | --- |
| `Display` 的 copy request：要求時要重畫、畫面沒變也回一個 frame；還沒有東西可畫時等第一個 frame；同一次畫的要求都拿到同一份，之後的不再回答；讀不回來的 renderer 回 `None`；沒畫出東西（`NotDrawn`）時要求留著；經過 Viz thread 也一樣 | `cargo test -p viz` |
| wgpu 的讀回：去掉每列的對齊填充、BGRA 換成 RGBA；讀回本身（`COPY_SRC`、device 重建之後也能讀回） | `cargo test -p viz`；`python3 scripts/test.py automation`（`gpu-crash`） |
| `RequestVisualState`：沒有要畫的東西時立刻回答；改了 DOM 之後，Viz 不送 `BeginFrame` 時不回答，含有那次 main frame 的 frame 交給 Viz 之後才回答；沒有 automation 時不回答 | `cargo test -p browser --test navigation query_element` |

**查元素**

| 項目 | 怎麼驗證 |
| --- | --- |
| `paint::element_bounds`：block 的 border box、inline 元素在每一行的盒子的聯集，不含溢出的文字；`display: none` 沒有；經過 transform、捲動容器的位移；直接在捲動容器裡的 inline 內容跟著捲，容器自己的盒子不跟 | `cargo test -p paint --test hit_test` |
| `QueryElement`：第一個符合的元素、`textContent`（不含註解）、viewport 座標的範圍（捲到 fragment 之後）；沒有符合、selector 不合法時回 `None`；沒有 `RendererFlags::automation` 時不回答 | `cargo test -p browser --test navigation query_element` |

**情境的格式和執行器**

| 項目 | 怎麼驗證 |
| --- | --- |
| 格式：每個指令、行號、引號和跳脫、按鍵和修飾鍵、`primary`、`type`、`platform`、`window`（只能在最前面）、`wait-busy`、`now`、錯誤 | `cargo test -p shell --lib scenario` |
| 畫面比對：整個視窗和元素範圍（1x 和 2x），大小不同的 frame 在範圍裡只比都有的那一塊 | `cargo test -p shell --lib automation`、`cargo test -p shell --test windowless` |
| 執行器（沒有平台視窗的瀏覽器視窗）：點擊打到元素中心（工具列下面）、等元素出現才點、改了版面之後立刻點也打中、按鍵經過網址列和網頁、修飾鍵只在它的按鍵期間有效、`primary` 在兩個平台各是 Cmd 和 Ctrl、`platform` 區塊、滾輪、截圖和 `resize`、`window` 的大小和 scale、`wait-loaded` 等 `DidFinishLoad`、`wait-busy`（從不忙的頁面失敗）、`capture` 等畫面而 `now` 不等、相對路徑、失敗時的行號和看到的值 | `cargo test -p shell --test windowless` |
| 情境等輸入被處理（2026-10-07 補）：問到元素的指令和不加 `now` 的截圖，先等瀏覽器送出的輸入都 ack（`WebContents::input_in_flight`；改了頁面的輸入，等畫出那個改動的 frame）；頁面忙時逾時，訊息說是輸入還沒處理；`wait-busy`、`click-at`、`wheel-at` 和 `now` 不等 | `cargo test -p shell --test windowless`（`a_query_waits_for_the_input_given`、`waiting_until_the_page_is_busy`、`a_click_follows_an_element_a_change_just_moved`）；`cargo test -p browser --test navigation`（`a_click_needs_a_drawn_document`、`a_click_after_a_compositor_scroll_lands_where_the_page_scrolled_to` 的計數） |
| 隱藏的分頁的 input ack（2026-10-07 補）：隱藏時，renderer 的 main thread 和 compositor 都把等著 frame 的 input ack 立刻送出、不帶 frame；隱藏期間處理的輸入立刻 ack；其他的 swap promise 照舊等再顯示時的 frame；點連結開了新分頁之後，情境等得到舊分頁的 ack | `cargo test -p compositor --lib`（`hiding_answers_only_the_input_acks_waiting_for_a_frame`、`input_acks_committed_while_hidden_are_answered_at_once`、`a_wheel_scrolled_here_while_hidden_is_acked_at_once`、`hiding_acks_a_wheel_scrolled_here`）；`cargo test -p browser --test navigation`（`hiding_a_tab_acks_the_input_waiting_for_a_frame`）；`python3 scripts/test.py scenarios link-tabs` |
| 捲進畫面（2026-10-07 補）：`ScrollIntoViewIfNeeded` 在元素的中心不在 viewport 裡時照 fragment 的方式捲（垂直、水平、元素外面的捲動容器），在畫面裡的、找不到的不捲，之後的 `RequestVisualState` 等畫出捲動；沒有 automation 時不捲；`click`、`wheel` 指到元素時先捲，捲了還在畫面外（比視窗高）時失敗；`scroll-to` 等元素有盒子才捲、等畫出來；RFC 0003 的 7 份情境在 800x600 的視窗裡先 `scroll-to` 再截圖 | `cargo test -p browser --test navigation query_element`（`an_element_out_of_view_is_scrolled_into_it`、`an_element_out_of_view_sideways_or_in_its_container_is_scrolled_to`、`nothing_scrolls_without_automation`）；`cargo test -p shell --test windowless`（`a_click_scrolls_its_element_into_view`、`scroll_to_brings_an_element_into_view`、`scroll_to_waits_for_its_element`、`failures_name_the_line_and_what_was_seen`）；`cargo test -p shell --lib scenario`；`python3 scripts/test.py scenarios flex grid inline fit-content` |
| `kun --automation`：選項、和其他選項的衝突、情境在開視窗之前讀好（錯的結束碼 2）、相對路徑照情境檔 | `cargo test -p shell --bin kun` |

**情境**（`python3 scripts/test.py scenarios`；真視窗是 `python3 scripts/test.py automation`）

| 情境 | 檢查什麼 |
| --- | --- |
| `demo-script` | `about:demo` 的 script 區塊：`load`、`setInterval` 的時鐘、「Click me」兩次、被 `preventDefault` 擋住的連結、`performance.now()`、「Hide or show」、rAF 的藍色方塊、兩秒後變綠，在真的 Viz thread 的 `BeginFrame` 底下 |
| `demo-busy` | script 忙碌時，compositor 照樣捲成為 layer 的框和整頁，1 秒之內就畫出來；被蓋住的框等 main thread，忙完才捲（`pages/busy.html`：和 `about:demo` 相同的框，放在固定的位置） |
| `wasm-demo` | C、Rust、Lua 三個 guest 都跑了，兩個時鐘不碰頁面也會走，一次點擊到三個 listener 和 C 自己分派的 `counted`；連回 `about:demo` 換回 JS；再進來時重新開始 |
| `script-wasm` | `window script=wasm`：視窗的設定交給 renderer，`about:demo` 也是 wasm 文件，它的 JS 不跑 |
| `navigation` | 連結；工具列的上一頁、下一頁、重新整理按鈕；各平台的上一頁、下一頁快捷鍵（Cmd+[ / ]、Cmd+←；Alt+← / →）、`primary`+R、F5；`#` 連結不重新載入；F6、`primary`+L、Alt+D（macOS 以外）之後打字和 Enter |
| `resize` | 改窄、再改寬之後頁面重新排版，`.hero` 的文字在新的寬度換行；之後點擊照樣有反應。真視窗由作業系統改大小 |
| `gpu-crash` | 在網址列輸入 `about:gpucrash` 之後，畫面在新的 device 上繼續更新、點擊有反應、還讀得回來（真視窗的 log 有 `made a new GPU device generation=1`） |

## 做過的檢查

2026-10-05，Windows 11。

- **截圖**：每個階段都看過。沒有平台視窗的：`demo-script`（計數、時鐘、藍色方塊）、`demo-busy`（整頁捲了 130px，第二個框忙完之後才捲，綠色方塊一直在它上面）。真視窗的：1536×1152（1024×768 在 1.5x）、系統字型、顏色對（沒有紅藍對調）；`window size=640x480` 開出 960×720，`resize 500 300` 之後 750×450；`resize` 的窄版重新換行；`gpu-crash` 之後工具列和頁面都畫在新的 device 上、網址列還是 `about:demo`。
- **確認情境真的在檢查**：`script-wasm` 拿掉 `window script=wasm` 時失敗在 `#status`；B2 的 mutation 讓 `resize` 不傳到頁面、讓新的 device 讀不回來，`resize` 和 `gpu-crash` 都失敗。
- **找到的 bug**：
  - **截圖比 DOM 舊**（B1）：`wasm-demo` 在真視窗裡的截圖每次都停在第一個 frame（點了兩次之後還是「Not clicked yet.」、時鐘 0 s），查詢看到的 DOM 卻已經變了：Viz 畫的是 renderer 已經交出的 frame，比 main thread 的 DOM 晚 commit、raster、submit 這一段。A 也一樣，只是剛好夠快。加了 `RequestVisualState`（「已決定的事項」第 13 條）。
  - **Windows 上改成視窗現在的大小**（B1）：`request_inner_size` 回 `None`，之後也不送 `Resized`，等待到時間上限；改成先比對現在的大小。
- **寫情境時找到的陷阱**（寫進 `tests/scenarios/README.md`）：導航之後立刻點，點擊被丟掉（renderer 只把 input 交給畫出來的 document），要先 `wait-loaded`；捲動量剛好是條紋的週期（40px 兩色交替捲 80px），畫面完全一樣；`wait-change` 在空的段落上被上面一行推下來騙過（截圖上看得出 Rust 的時鐘其實還沒走），改成等確切的文字；只出現一秒的時鐘文字要在載入之後就查。
- **不穩定**：
  - 沒有平台視窗的情境和 `windowless` 的測試，每個階段連跑 5 次都通過；B 加了 `RequestVisualState` 之後一次約 5 秒（`waiting_until_the_page_is_busy` 等一段忙碌）。
  - 真視窗：加 `RequestVisualState` 之前，`test.py automation` 20 輪（100 次情境）失敗 1 次：`wasm-demo` 點「Back to about:demo」沒有導航（沒有 navigate 的 log）。推測是改了 DOM 之後立刻點，查到的位置和點擊時的版面不同，寫了 `a_click_follows_an_element_a_change_just_moved`（頁面一直有 rAF，改版面之後立刻點，每次都要打中），120 次都打中，沒有重現，推測沒有證實。之後 `wasm-demo` 單獨 40 次、`test.py automation` 20 輪加 6 輪（B2 的 7 份情境 3 輪）都通過。原因沒有找到，再出現時先看那一次的截圖。
- **Mutation test**：共 71 條，最後 70 條抓到、1 條沒抓到。
  - **Viz 和讀回**：copy request 不要求重畫、回答了沒清掉、從來不複製、`SoftwareRenderer` 讀不回來、Viz thread 丟掉要求、沒畫出來當成讀不回來、BGRA 沒換、每列的填充沒去掉，由 `viz` 的測試抓到；surface 沒有 `COPY_SRC`、沒有記錄複製、視窗沒要讀回由 `test.py automation`（`demo-busy`、`demo-script`、`wasm-demo`）抓到；新的 device 讀不回來由 `test.py automation`（`gpu-crash`）抓到。
  - **`element_bounds`**：漏掉 inline 盒子、inline 盒子用盒子自己的空間、不換到 root 的座標、漏掉 block 盒子、只取最後一個盒子。「用盒子自己的空間」第一輪沒抓到：兩種空間只在捲動容器自己有行內內容時不同，補了 `inline_bounds_in_a_scroll_container_scroll`。
  - **`QueryElement`、`RequestVisualState`**：沒有 automation 也回答（兩者各一條）、不扣 viewport 的捲動位置、註解的文字也算、取最後一個符合的、要了 main frame 還立刻回答、改了 DOM 之後永遠不回答，由 `browser` 的 `navigation` 測試抓到。
  - **執行器和沒有平台視窗的視窗**：不移到工具列下面、`wait-loaded` 立刻回答或永遠不回答、`wait-text` 和 `wait-change` 的判斷反過來、比對畫面不看範圍、範圍不乘 scale、大小不同的 frame 在範圍裡一律算不同、範圍的列用了另一個 frame 的寬度、不等元素出現、路徑不照情境檔、點擊不放開、修飾鍵沒按著、`resize` 沒作用、截圖的檔名多一個 `.png`、`platform` 區塊兩種錯法、`primary` 對調和忽略、`window` 的 scale 和大小沒作用、`wait-busy` 兩種錯法、`capture` 不等畫面、等到了卻不讀回，由 `windowless` 和 `automation` 的測試抓到；`window` 的 script 沒作用、快捷鍵一律用執行的作業系統的、`expect-differs` 也等畫面、`resize` 不傳到頁面，由情境（`script-wasm`、`navigation`、`demo-busy`、`resize`）抓到。**修飾鍵按完沒放開**沒抓到：每個按鍵送出前都設好自己的修飾鍵，留著的只影響之後的滑鼠（例如工具列的 Shift+點擊），情境查不到工具列；記在 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」。「修飾鍵沒按著」第一輪沒抓到：Shift+Space 和 Space 都讓畫面改變，改成 Ctrl+End（不送給網頁）和之後的 End。
  - **從 `window.rs` 搬到 `BrowserWindow` 的**：compositor 等 main thread 由 `demo-busy` 抓到，`script_language` 沒交給 renderer 由 `script-wasm` 抓到。
  - **真視窗**：點擊只按不放（當時的五份都失敗）、滾輪方向反了（`demo-busy`）、按鍵沒有修飾鍵（`navigation`）、情境的 `window script=` 沒作用（`script-wasm`），由 `test.py automation` 抓到；失敗的情境當成通過、情境檔有錯時結束碼 1 而不是 2，由實際跑一份會失敗的、一份有錯的情境抓到；`kun` 的選項三條由它的單元測試抓到。
  - **格式**：引號從字的中間開始也算、不跳過註解、Shift 不轉大寫、`primary` 的按鍵會打字、`window` 可以放在其他指令之後、`now` 被忽略，由格式的單元測試抓到。
  - **腳本本身的錯**：第一輪檔案是 CRLF，跨行的 mutation 沒有套上，看起來沒抓到；改成照檔案的換行並確認真的改了，重跑之後都抓到。有幾條第一次寫成加 `if false` 的 match guard，match 不完整、編不過，改寫之後重跑。
- **沒有自動測試的**（記在 `docs/testing.md`）：automation 期間忽略平台的輸入、視窗在最上層（要真的去動滑鼠、切 App），只靠審查；真視窗的 `window size=` 只看過截圖的大小；被蓋住的框「忙碌期間不捲」只確認了忙完之後會捲，那部分由 `paint` 的 hit test 測試和 `compositor` 的 `a_wheel_over_a_container_goes_to_the_main_thread_with_the_scrolling` 守住；網址列的 Esc 還原、游標的樣子由 `omnibox` 和 `browser_view` 的測試守住。

### 情境等輸入被處理（2026-10-07，macOS arm64）

做完之後補的修正，在 RFC 0010 的第 2 階段期間查到（那份 RFC 的 `progress.md` 的 2d-2）。

- **現象**：`windowless` 的 `a_click_follows_an_element_a_change_just_moved` 越來越常失敗（40 次點擊打中 39 次）：整個 `windowless` 跑 5 次都失敗，單獨跑都通過；1d（HEAD）的程式用同一個 V8 另外編，5 次失敗 4 次，所以不是 RFC 0010 造成的。
- **原因**：情境的 `click` 先問元素在哪（`QueryElement`），再送點擊；兩者都對 renderer 最後畫出來的那一幀回答和 hit test。但執行器送完一個點擊就繼續，沒等 renderer 的 `InputEventAck`（`windowless` 和真視窗都是收下輸入就回答），而輸入經過 compositor thread、`QueryElement` 直接到 main thread。所以點了 `#grow` 之後，下一次查 `#target` 拿到的是還沒畫出改動的舊位置；查詢和點擊的 hit test 之間剛好畫了新的一幀時，點擊就照新的版面打在舊座標上。在頁面裡記錄打錯的那次，是第 7 次點擊打到第 1 行（還沒長高時 `#target` 的位置），和這個推論相符；暫時讓點擊等到三個 ack，`windowless` 8 次都通過。B1 在真視窗看到的那次「Back to about:demo 沒有導航」，推測也是同一個原因。
- **修正**：`WebContents` 記著送出、還沒 ack 的輸入事件數（`input_in_flight`，Chromium 的 `RenderWidgetHostImpl::in_flight_event_count_`），`AutomationHost` 多一個 `Request::InputHandled`，數到 0 時才回答；執行器在 `QueryElement` 和不加 `now` 的截圖之前送它。renderer 本來就讓改了頁面的輸入等畫出改動的 frame 才 ack，所以之後問到的位置和截到的畫面都包含那次輸入。`wait-busy`、`click-at`、`wheel-at`、`key` 和 `now` 不等（頁面忙時要用它們，`demo-busy`）。和 Chromium 比：DevTools 的 `Input.dispatchMouseEvent`（ChromeDriver 用它點擊）也要等 renderer 的 ack 才算完成。
- **`wait-change` 的起點**：原本是「等待開始時」的元素，這個起點的查詢以前通常比前一個輸入早到 renderer，拿到改之前的樣子；有了上面的等待，它拿到的已經是改之後的，緊接在輸入後面的 `wait-change` 永遠等不到（`demo-script`、`navigation`、`wasm-demo` 失敗）。它以前能用，靠的正是這個競爭。改成：緊接在輸入（`click`、`wheel`、`key`、`type`、`resize`，中間的 `platform` 行不算）後面時，執行器在送輸入之前先記下那個元素，`wait-change` 和它比；其他的照舊和等待開始時比（例如等 rAF 的動畫）。情境的寫法不用改。Chrome 那邊沒有這種指令：WebDriver、Puppeteer 要「和之前不同」時，由測試在動作之前自己取舊的值，Playwright 建議等「變成某個值」；之後再看要不要照那樣做出自動化的工具。
- **測試**：`a_change_the_input_made_is_waited_for`（輸入之後的 `wait-change` 看得到那次的改變，中間有 `platform` 行也一樣；不是緊接在輸入後面的，和開始時比；起點只用一次）；`a_query_waits_for_the_input_given`（點擊讓頁面忙時，`wait-text` 的逾時是「輸入還沒處理」，不是查詢沒人回答）；`waiting_until_the_page_is_busy` 裡不加 `now` 的截圖改成同樣的原因；`browser` 的兩個測試檢查計數（送出時加、ack 時減，改了頁面的等到 frame）。`a_click_follows_an_element_a_change_just_moved` 留著當端對端的檢查，從 3 輪改成 1 輪（等 frame 之後一組點擊至少兩個 frame）；它單獨跑時本來就很少失敗，所以等待本身由上面的測試守住。`windowless` 連跑 10 次都通過，一次約 5.7 秒。
- **mutation test**（11 條；跑的期間使用者沒有 commit，跑完確認都還原了，之後跑了 `ci.py prune`）：查詢不等、截圖不等、有輸入在途中也回答、ack 不通知等待的、送出不計數、ack 不減；`wait-change` 不在輸入之前記起點、`platform` 行算成下一步、起點用第二次、點擊不算輸入，都抓到。沒抓到 1 條：renderer 不在時不清掉等待中的 `InputHandled`。只差在多快失敗（清掉時立刻、不清時等到逾時，訊息一樣），`windowless` 收到 renderer 不在時本來就結束這一輪；查詢、載入、截圖的等待也是同樣處理，同樣沒有測試，算等價。
- **檢查**：fmt、clippy（沒有警告）、`cargo test --workspace`（945 個都通過，`windowless` 不再失敗；沒有平台視窗的情境 9 份各兩個平台都通過；`wpt_testharness` 617 個都相符；reftest 193 個通過）、`sanity.py deps`、`unsafe`、`docs`。`test.py automation`（macOS，2x 的螢幕）9 份都通過；前兩次在螢幕鎖定時跑，要讀回畫面的情境都沒有 frame（連不受這次影響的 `capture … now` 也是），解鎖之後重跑。
- **另外記下的差別**：滑鼠事件對最後畫出來的那一幀 hit test，Blink 是 hit test 之前先把版面更新到最新；script 剛改了版面、新的一幀還沒畫出來時點擊，兩者打到的地方不同。這是產品行為的差別，不是這次的原因，記在 `docs/limitations.md`。

### 隱藏的分頁的 input ack（2026-10-07，macOS arm64）

把 RFC 0010 的分支 rebase 到 RFC 0011（分頁）之後查到：上面的「情境等輸入被處理」和分頁合在一起才出現的問題。

- **現象**：情境 `link-tabs` 在 `click #open` 之後的 `wait-text` 逾時，訊息是「輸入還沒處理」。
- **原因**：點擊跑了 script，它的 ack 等畫出改動的 frame；同一個點擊開了新分頁，舊分頁被隱藏，不再畫 frame，ack 一直不來，`input_in_flight` 停在 1。等著的 ack 卡在兩個地方：renderer 的 main thread（`pending_input_acks`，等下一次 main frame 交給 commit），和 compositor（已經 commit、還沒畫）。compositor 自己捲的 wheel 的 ack 也一樣。
- **和 Chromium 比**：Chromium 的 input ack 不是 swap promise，main thread 處理完就 ack，和畫不畫、可不可見無關。隱藏時 Chromium 不打斷排著的 swap promise：`LayerTreeHost::SetVisible(false)` 只通知 proxy，scheduler 不可見時不送 `BeginMainFrame`（`CouldSendBeginMainFrame`），只有已經送出的 `BeginMainFrame` 碰到不可見時才放棄並 `BreakSwapPromises(COMMIT_FAILS)`（`ProxyMain::BeginMainFrame`）。
- **修正**：`SwapPromise` 分出 input ack 這一類（`SwapPromise::input_ack`）。隱藏時 compositor 把這一類用 `None` 結算（`LayerTreeHostImpl::break_input_acks`），隱藏期間收到的 commit 和自己捲的 wheel 也一樣；renderer 的 main thread 收到 `WasHidden` 時結算 `pending_input_acks`，隱藏期間處理的輸入立刻 ack（`LayerTreeHost::is_visible`）。其他的 swap promise（`DidFinishLoad`、visual state）照 Chromium 留到再顯示時的 frame。比過的做法：只改 `AutomationHost`、不等不是 active 的分頁（`input_in_flight` 留著不對的數字），或隱藏時打斷所有的 swap promise（和 Chromium 不同）。
- **測試**：`compositor` 的四個（隱藏時只結算 input ack、其他的等再顯示時的 frame；隱藏之後才到的 commit；隱藏時捲的 wheel；捲了之後才隱藏）；`browser` 的 `hiding_a_tab_acks_the_input_waiting_for_a_frame`（Viz 不畫時，點擊在 main thread 等、wheel 在 compositor 等，隱藏之後兩個都 ack；隱藏期間的點擊和 wheel 立刻 ack；都沒畫 frame）；情境 `link-tabs`。
- **mutation test**（11 條；在另一個 worktree 跑，跑完確認都還原了）：compositor 隱藏時不結算、隱藏期間的輸入不結算、隱藏期間的 commit 不結算、結算所有的 promise、什麼都不結算、compositor 的 wheel ack 用一般的 promise、`input_ack` 的標記失效、main thread 隱藏時不結算、隱藏期間的輸入照樣等 frame、`ProxyMain` 不記可見性，都抓到。沒抓到 1 條：main thread 交給 commit 的 ack 用一般的 promise。只在不等 main thread 就畫的 compositor（視窗）裡、commit 和下一次畫之間被隱藏時才有差別，測試做不出這個時序，記在 `docs/testing.md`。腳本第一輪還原檔案時留下較舊的 mtime，跨 crate 的下一條沿用了上一條弄壞的編譯結果（M8 看起來抓到了）；改成還原後更新 mtime，重跑後面 4 條。
- **檢查**：連同後面調整情境視窗的 commit，fmt、clippy（沒有警告）、`cargo test --workspace` 都通過（libtest 1112 個；`wpt_testharness` 641 個都相符、138 個不跑；reftest 504 個通過、43 個是預期中的失敗；沒有平台視窗的情境 20 份各兩個平台）、`sanity.py deps`、`boundary`、`unsafe`、`docs`。沒有跑 `test.py automation`（沒動到視窗那一層）。

### 捲進畫面（2026-10-07，macOS arm64）

做完之後補的，在 macOS 上第一次跑 `test.py automation` 時查到（第 20 條）。

- **現象**：20 份情境裡 7 份失敗，都是 RFC 0003 的：`fit-content`、`flex`、`flex-z-index`、`grid`、`grid-areas`、`inline-block`、`inline-flex`，訊息都是要點的元素在視窗外。它們要求 2600 到 3900 高的視窗；Windows 開得出來（RFC 0003 只在 Windows 11 上跑過 automation），macOS 把視窗限制在螢幕裡，這台開出來是 800×891（2x）。沒有平台視窗的版本沒有這個限制，所以 `cargo test` 照樣通過。
- **修正**：見第 20 條。執行器在 `click`、`wheel` 指到元素時先送 `ScrollIntoViewIfNeeded`，再用 `RequestVisualState` 等畫出來，之後照舊查位置；新指令 `scroll-to` 只做這件事。renderer 的 `FrameView` 記下要捲到的元素，下一次 update 時用 `ScrollState::scroll_into_view`（和 `#fragment` 同一個）捲。7 份情境拿掉 `window size=`（預設的 800x600），`capture` 之前先 `scroll-to`。另外 `windowless` 的 `a_click_follows_an_element_a_change_just_moved` 在 main 上本來就失敗：RFC 0011 的分頁列把網頁往下移了 34px，第 17 次點擊時目標超出 600 高的視窗；它測的是「版面剛改就點」，不該捲，給它 800x1400 的視窗。
- **測試**：`browser` 的三個（垂直超出時捲到頂端、在畫面裡的和找不到的不捲；只在水平方向超出、只在捲動容器裡超出；沒有 automation 時不捲，用捲上來的連結會不會被點到檢查）；`windowless` 的三個（點擊先捲、`scroll-to` 之後的截圖、`scroll-to` 等元素出現）和改過的一個（比視窗高的元素捲了還在外面時失敗）；格式的單元測試。新的測試連跑 5 次都通過。
- **mutation test**（13 條；在使用者的工作目錄裡跑，跑的期間使用者沒有 stage 或 commit，跑完和備份逐 byte 比對確認都還原了）：從不捲、一律捲、不看垂直方向、不看水平方向、update 時不捲、捲了之後不重新 paint、不要求 main frame、沒有 automation 也捲、`AutomationHost` 不送捲動、捲動等不到回答、點擊不先捲、`scroll-to` 什麼都不做、捲動之前不等元素有盒子，都抓到。第一輪沒抓到 4 條（不看水平方向、不重新 paint、沒有 automation 也捲、不等盒子），補了上面的測試之後重跑都抓到；其中「沒有 automation 也捲」的測試第一版用滑鼠移動的 ack 當同步點，輸入經過 compositor thread、和捲動的請求誰先到 main thread 不一定，改成用 `resize`（和請求走同一條 channel，它的 main frame 也會捲）。
