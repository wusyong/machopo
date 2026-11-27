# RFC 0123：renderer 送什麼都不能讓 Browser 結束

- 開始日期：2026-11-27
- 狀態：👍 已接受：2026-11-27（已決定的事項第 1 到 4 條；數量的上限等第 0 步的探測）
- 細節：`details.md`

## 摘要

renderer process 關在 sandbox 裡，但它送給 Browser 的東西有兩條路能讓整個 Browser 結束：它的 frame 是在 Browser 的 process 裡（Viz thread）交給 wgpu 的，而 wgpu 的錯誤（驗證錯誤、記憶體不夠）預設是 panic；Browser 處理它的訊息的程式如果 panic，結束的也是 Browser。這份把這兩條路關上：Viz 在把值交給 wgpu 之前驗完、不合格的 frame 算那個 renderer 的壞訊息（結束那個 renderer，像 Browser 其他的檢查一樣）；wgpu 的錯誤接住、不結束程式；fuzzer 加上 frame 的、進每週的 CI。這是 site isolation 路線圖（RFC 0122）的 A，里程碑 M1 的第一份。

## 動機

現況（2026-11-27 的 `main`，`146db413`；`details.md` 的「盤點」）：

- renderer 的 `CompositorFrameSinkMsg`（frame、shared image）由 Browser 的 IO thread 解開、轉給 Viz thread，Viz 在 Browser 的 process 裡用 wgpu 畫。
- Viz 對單一的值檢查得不少：resource 的像素和大小對得上、每邊不超過 `MAX_TEXTURE_SIZE`、shared quad state 的索引越界時用預設值、scissor 夾在輸出的範圍裡、不認得的 resource 不畫、surface 嵌自己會停。
- 但是：
  1. **wgpu 的錯誤沒有人接**：`viz` 只裝了 device 遺失的 callback，沒有 error scope、沒有 uncaptured error 的 handler；wgpu 預設的 handler 是 panic。檢查漏掉的值、或檢查擋不住的（GPU 的記憶體不夠），結果都是 Viz thread panic。
  2. **數量沒有上限**：一個 frame 幾個 quad（頂點的 buffer 跟著長，超過 device 的 `max_buffer_size` 是驗證錯誤）、一個 sink 幾個 shared image、合計多少像素。讀程式找到的候選，還沒用測試確認。
  3. **不合格的 frame 只是丟掉**：`Display::submit_compositor_frame` 記一筆錯誤、照舊顯示上一個；renderer 沒有被當成送了壞訊息（那段註解還寫著「a client is a thread of the same process, so such a frame is machopo's own bug」，renderer 拆成 process 之前的）。
- Browser 處理 `RendererHostMsg` 的那一側比 `docs/limitations.md` 寫的好：測試以外的 22 處 `expect` 都是內部的不變式（鎖、剛放進去的項目），不是直接解 renderer 的輸入，另有 21 處索引；整數運算有 lint 守著，`unwrap`、`expect`、索引沒有（workspace 沒有這方面的規則）；有一個 fuzzer（`python3 scripts/test.py browser fuzz`：假的 renderer 送任意訊息，找 panic），但 CI 不跑，也不涵蓋 Viz。
- Viz thread 或 UI thread panic 之後 Browser 怎麼樣，沒有試過（沒有 `catch_unwind`；panic 的策略是預設的 unwind，只有那條 thread 結束）。

**目標**：

1. renderer 送任何 `CompositorFrameSinkMsg`、任何 `RendererHostMsg`，Browser 的 process 都不結束、不卡住；其他分頁照常。
2. 送了不合格的東西的 renderer 被結束（壞訊息），分頁顯示 sad tab：和 Browser 其他的檢查同一條路。
3. wgpu 的錯誤（驗證、記憶體不夠）不是 panic。
4. 上面三條有 fuzzer 和測試守著，fuzzer 定期跑。

**不在範圍內**：

- GPU process（驅動程式自己 crash、device 遺失重建不了）：另外的路線。
- renderer 的記憶體上限、CPU 的用量：失控的 renderer 拖慢整台機器是另一件事。
- Browser 的其他不可信的輸入：網路的回應（`net`）、DevTools 的 client、WebDriver。
- ORB、process lock：路線圖的 N、B。

要解決的 `docs/limitations.md` 裡的限制：「隔離」的「renderer 還有兩條路能讓 Browser 結束」；「Viz、GPU 和記憶體」的「Viz 收到 renderer 送來的不合法的 frame……Chromium 會結束那個 client」。

## 設計概要

給 machopo 的使用者：看不到改變；一個壞掉（或被攻破）的分頁只會讓自己變成 sad tab。

給改 machopo 的人：

- 在 `protocol::viz` 加新的 quad、新的欄位時，要在 Viz 的檢查（`viz` 的 `display.rs`）說它合格的範圍；fuzzer 會餵任意的值。
- Viz 裡呼叫 wgpu、參數來自 client 的地方，錯誤不會 panic，但那表示檢查漏了：debug build 和 fuzzer 把它當成失敗。

## 詳細設計

下面每一節是選項和比較，最後是選了哪個（理由在「已決定的事項」）；「數量的上限」還沒定。

### 不合格的 frame 怎麼處理

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| V1 | **算壞訊息**：Viz 告訴 Browser 是哪個 sink，Browser 照 `received_bad_message` 結束那個 renderer（`BadMessageReason` 多一種） | 和 Browser 其他的檢查一樣；被攻破的 renderer 不能一直試；和 Chromium 一樣（Viz 對 client 的 frame 做驗證，不過的 `ReportBadMessage`） | Viz 要有一條回 Browser 的路（現在只有 begin frame 和 context lost 的 observer）；machopo 自己的 bug 也會變成 sad tab（好處：看得到） |
| V2 | 照舊丟掉、記一筆 | 不用改 | 被攻破的 renderer 可以無限次地試；錯誤藏在 log 裡 |

Browser 自己的 UI 的 frame（root sink）不合格是 machopo 的 bug：debug build 是 assert，release 丟掉。

定了 **V1**（已決定的事項第 1 條），只對「這一則訊息自己說不通」的；因為時序（device 換了、sink 不在了、大小變了）、資源不夠、或 machopo 自己的 bug（NaN 的座標）發生的，不算壞訊息（同一條的表）。

### wgpu 的錯誤

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| G1 | **先驗完，再加一層接住的**：(a) 交給 wgpu 之前把會造成錯誤的值和數量都擋掉（V1 的檢查）；(b) 每次畫一個 frame、每次上傳 shared image 包在 error scope 裡（驗證和記憶體不夠兩種），有錯誤就丟掉那一次：上傳的知道是哪個 sink，算它的壞訊息；畫的（幾個 sink 的 quad 合在一起）不知道是誰的，那個 frame 不畫、下一個照常；(c) 再裝 uncaptured error 的 handler，記錯誤、不 panic | 檢查漏了也不會結束 Browser；上傳的錯誤找得到人；debug build 和 fuzzer 把 (b)、(c) 接到的當成失敗，所以漏掉的檢查會被發現 | error scope 每個 frame 多兩次 push 和 pop（要量；wgpu 的 native 後端是同步的）；接住之後 device 的狀態要確認還能用 |
| G2 | 只裝 uncaptured error 的 handler（不 panic） | 最少 | 不知道是哪一次、哪個 client；錯誤之後照樣 submit 半個 frame |
| G3 | 只做先驗完，錯誤照舊 panic（當成 assert） | 沒有執行時的成本 | 檢查只要漏一個，renderer 就能結束 Browser；記憶體不夠擋不住 |

定了 **G1**（已決定的事項第 2 條；(b)、(c) 接到的只丟掉、不結束 renderer）。對照 Chromium：client 的命令在 GPU process 裡驗證，不過的那個 client 失去 context；GPU process 自己 crash 也不會帶走 Browser。machopo 沒有 GPU process，所以要靠「驗完加接住」。

### 數量的上限

要擋的是「合格的值、不合理的量」：

| 什麼 | 現在 | 看法 |
| --- | --- | --- |
| 一個 frame 的 quad 數 | 沒有上限（一則訊息最多 128 MiB，放得下幾百萬個） | 上限照 device 的 `max_buffer_size` 換算，超過算壞訊息，而且 renderer 的 compositor 自己守同一個上限（誠實的、很重的頁面不會超過，第 1 條）；或頂點分批畫（不設上限）。第 0 步先確認超過時 wgpu 怎麼反應 |
| 一個 sink 的 shared image 數和合計的像素 | 沒有上限 | 給一個預算（待決定：多少；Chromium 的 cc 照記憶體的預算管 tile，照記憶、沒對原始碼），超過的不做、算壞訊息還是只丟掉也待決定 |
| render pass 數、resource 數 | 只用最後一個 pass；resource 照 frame 提到的 | 跟著 quad 數的上限 |

記憶體不夠（wgpu 的 OutOfMemory）由 G1 的 (b) 接住，不算誰的壞訊息。

### Browser 處理訊息時的 panic

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| P1 | **照現在：不 panic 是程式的性質，靠審查、lint 和 fuzzer**；這份把現有的 `expect` 逐一看過（哪些不變式是 renderer 的訊息順序破得了的）、fuzzer 進 CI | 和 Chromium 一樣（它的 `CHECK` 失敗就是 Browser crash，靠驗證和 fuzz）；狀態不會壞一半 | 漏一個就是 Browser 結束 |
| P2 | P1，再在訊息的入口 `catch_unwind`：panic 當成那個 renderer 的壞訊息 | 漏掉的也只結束一個分頁 | panic 的當下 Browser 的狀態可能改了一半（frame 樹、session history、拿著的鎖會 poison）；之後的行為沒有保證，可能比結束更糟（安靜地錯）；Rust 的慣例是 panic 不當成錯誤處理 |

定了 **P1**，加上只在處理 renderer 輸入的模組打開 panic 的 lint（已決定的事項第 3 條）；Viz thread 也一樣（它的 panic 由 G1 拿掉來源，不用 `catch_unwind`）。第 0 步試一次「Viz thread panic 之後 Browser 怎麼樣」，結果是卡住（沒有 frame、沒有結束）的話，至少讓它變成明確的結束。

### fuzzer 和 CI

- 新的 fuzz target：任意的 `CompositorFrameSinkMsg` 序列餵給 `Display`，用真的 wgpu（CPU adapter：lavapipe）畫；找 panic、wgpu 的錯誤（G1 的 (b)、(c) 接到的算失敗）、卡住。和現有的 `renderer_message` 一樣，檢查寫成整合測試、fuzz target 只是呼叫它，找到的輸入留在 `regressions/` 由 `cargo test` 重放。
- 現有的 `renderer_message` fuzzer 不改。

| | 做法 | 說明 |
| --- | --- | --- |
| C1 | **兩個 fuzzer 進每週的 CI**（`gc.yml` 那一類排程的 job，各跑固定的分鐘數） | 定期有人跑；找到的要有人看（失敗的輸入存成 artifact） |
| C2 | 照舊只在本機跑（`docs/test-gaps.md` 現在的樣子） | 不動 CI；改到這幾個檔案的人記得跑 |

定了 **C1**（已決定的事項第 4 條）。

### 測試

- **不合格的 frame 結束那個 renderer、Browser 和其他分頁照常**：`shell` 的整合測試，一個能送任意 `CompositorFrameSinkMsg` 的假 renderer process（`renderer_process.rs` 的 `RunningRenderer` 那一類）送各種不合格的 frame，看 Browser 還在、那個分頁是 sad tab、另一個分頁照樣畫。
- **wgpu 的錯誤不 panic**：`viz` 的測試直接對 `WgpuRenderer` 做會造成驗證錯誤的事（繞過檢查），確認接住、下一個 frame 照畫。
- **上限**：剛好在上限和超過一個的 frame。
- **Browser 的 `expect` 和索引**：打開 lint 的模組 clippy 沒有警告（每一處有帶理由的 `#[expect]`，或改寫掉了）；逐一看的結果寫進 `progress.md`，破得了的補檢查和 fuzzer 的 regression。
- mutation test：每一條檢查拿掉、error scope 拿掉、壞訊息不回報。

### 分階段

0. 草稿和盤點（這一版），加上探測：Viz 用到的每個 wgpu 呼叫失敗時是走錯誤回報、回 `Result`，還是直接 panic（`create_buffer_init` 是直接 panic 的候選）；超過 `max_buffer_size` 和 GPU 記憶體不夠時實際怎麼反應；NaN 的座標有沒有驗證錯誤；Viz thread panic 之後 Browser 怎麼樣；error scope 的成本。
1. Viz 的檢查補齊、不合格的 frame 算壞訊息（V1）、上限。
2. wgpu 的錯誤接住（G1 的 (b)、(c)）。
3. `browser` 和 `viz` 處理 renderer 輸入的模組打開 panic 的 lint、逐一寫理由或改寫（第 3 條）；規則寫進 `crates/base/README.md` 和 `CLAUDE.md`；Viz 的 fuzzer；CI。

## 缺點

- 多一層檢查和 error scope：每個 frame 多一點成本（第 0 步量）。
- 上限是新的「合法的網頁可能撞到」的地方：renderer 的 compositor 要自己守同一個上限（超過時合併或不送），不然 quad 很多的頁面會變成 sad tab。
- machopo 自己的 bug（compositor 送出不合格的 frame）以前是畫錯或沒畫，之後是 sad tab：比較吵，但看得到。
- fuzzer 進 CI 之後要有人看它的結果。

## 理由和其他做法

- **不做的話**：被攻破的 renderer 不用逃出 sandbox，送一個讓 wgpu 驗證失敗的 frame 就結束整個 Browser、所有分頁；site isolation 後面幾份的前提（壞訊息只結束那個 renderer）不成立。
- **直接做 GPU process**：Chromium 的做法，wgpu 的錯誤和驅動程式的 crash 都只影響 GPU process；但那是另一個量級的工作，而且 frame 的驗證還是要做。
- 各節的選項比較在上面。

## 先例

- **Chromium**（照記憶，各節開始時對原始碼）：Viz 在 GPU process；client 的 frame 經過 mojo 的型別驗證和 `viz` 的檢查，不過的 `ReportBadMessage`、結束那個 client；GPU 的命令在 GPU process 驗證，不過的 client 失去 context；Browser 的 `CHECK` 失敗就是 crash，靠驗證和 fuzz（`content/browser/bad_message.h` 的那一長串理由）。
- **Firefox**：沒查。

## 已決定的事項

1. **不合格的 frame 算那個 renderer 的壞訊息（V1）**（2026-11-27，使用者決定）：Viz 告訴 Browser 是哪個 sink，Browser 照 `received_bad_message` 結束那個 renderer、分頁顯示 sad tab（`BadMessageReason` 多一種）。Browser 自己的 UI 的 frame（root sink）不合格是 machopo 的 bug：debug build 是 assert，release 丟掉。理由：和 Browser 其他的檢查同一條路；被攻破的 renderer 不能一直試；Chromium 也是不過的結束那個 client。代價：Viz 要多一條回 Browser 的路；machopo 自己的 compositor 的 bug 也變成 sad tab（比較吵，但看得到）。不選照舊丟掉、記一筆（V2）。
   **只有「這一則訊息自己說不通」的才算**（2026-11-27，使用者問「不合格的 frame 會不會是不穩定造成的」之後補的）：誠實的 renderer 在任何時序下都不會送的（像素的長度和大小對不上、超過兩邊都知道的 texture 上限、超過 renderer 自己也守的數量上限）。其他的照下表，實作時每一條檢查標明是哪一類：

   | 情況 | 處理 |
   | --- | --- |
   | 這一則訊息自己說不通 | 壞訊息，結束 renderer（硬體不穩造成的也是：重新載入就好） |
   | 取決於 Viz 或 Browser 的狀態：frame 提到的 shared image 是舊的 device 上的（不認得的 mailbox）、sink 已經註銷（分頁在關）、frame 還是舊的大小、沒有 GPU 時送來的 shared image | 忽略或不畫，和現在一樣：renderer 送出的當下不可能知道 |
   | 資源不夠（GPU 的記憶體） | 丟掉那一次，不算誰的 |
   | 值很怪但無害（NaN、無限大的座標：machopo 自己的 bug 的樣子） | 那個 quad 不畫；renderer 的 debug build 在送出前自己 assert，bug 在源頭、帶著 backtrace 被看到，不是等 Viz 變成 sad tab |

   Chromium 的原則也是這樣：只有 renderer 本來就該知道不能這樣的才結束它，可能因為時序發生的一律忽略。實作時要釐清的：device 遺失之後 renderer 用同一個 mailbox 的編號重建 shared image，和現在「同一個編號建兩次」的檢查會不會撞。
2. **wgpu 的錯誤：先驗完，再用 error scope 和 uncaptured error 的 handler 接住（G1）**（2026-11-27，使用者決定）：(a) 交給 wgpu 之前把會造成錯誤的值和數量擋掉（第 1 條的檢查）；(b) 每次畫一個 frame、每次上傳 shared image 包在 error scope 裡（`Validation`、`OutOfMemory`、`Internal` 三種各一層），有錯誤就丟掉那一次；(c) 裝 uncaptured error 的 handler，記錯誤、不 panic。(b)、(c) 接到的一律只丟掉、記錯誤，不結束 renderer（記憶體不夠不是誰的錯；驗證錯誤表示 (a) 漏了，是 machopo 要修的；畫的時候幾個 sink 的 quad 合在一起，也分不出是誰的）：結束 renderer 只靠 (a)。debug build 和 fuzzer 把 (b)、(c) 接到的驗證錯誤當成失敗，漏掉的檢查才找得出來。理由：(b)、(c) 就是 wgpu 自己說的兩種處理錯誤的方式（`Error` 的文件：「By default, errors translate into panics……use `Device::push_error_scope()` or `Device::on_uncaptured_error()`」）；但它們接不住全部：wgpu 自己的 Rust 程式裡有直接 panic 的路（`create_buffer_init` 建不出 buffer 時，錯誤進了 error scope，下一行對 mapped range 的 `expect` 照樣 panic；照程式讀的，`details.md` 的「wgpu 的錯誤處理」），那些只有 (a) 擋得住，或換成不會 panic 的寫法。error scope 是每條 thread 一份，Viz 的 wgpu 呼叫都在 Viz thread 上。代價：每個 frame 多幾次 error scope 的進出（第 0 步量）。不選只裝 handler（G2：不知道是哪一次，壞的 frame 畫一半）。不選只先驗完、錯誤照舊 panic（G3：漏一個檢查就能結束 Browser，記憶體不夠擋不住）。Chromium 在 GPU process 驗證命令，machopo 沒有 GPU process。
3. **Browser 的 panic 不接；處理 renderer 輸入的模組用 lint 守（P1 加 E1）**（2026-11-27，使用者決定）：不在訊息的入口 `catch_unwind`。`browser` 處理 renderer 訊息的模組和 `viz` 收 frame 的模組打開 clippy 的 `unwrap_used`、`expect_used`、`indexing_slicing`、`panic`、`unreachable`；留下的每一處用 `#[expect(lint, reason = "...")]`，理由寫 renderer 為什麼破不了那個不變式（和整數運算的規則同一個寫法，`crates/base/README.md`）；測試不套用。理由：2026-11-27 用 clippy 數過（只編 lib 和執行檔，不含測試；`details.md` 的「會 panic 的寫法有多少（量的）」）：整個 workspace 2,922 處，renderer 的輸入在 Browser 這一側走得到的只有約 50 處（`browser` 43：22 個 `expect`、21 個索引，28 處在 `navigation_controller.rs`；`viz` 9），逐一寫理由做得到；這份本來就要逐一論證它們，lint 讓論證留在程式裡，之後新加的被擋下來、不靠人記得，補上「不接 panic」漏一處就結束 Browser 的弱點。不接的理由：panic 的當下狀態可能改了一半（session history、被 poison 的鎖），接住之後的行為沒有保證，可能安靜地錯；Chromium 的 `CHECK` 失敗也是 Browser crash，靠驗證和 fuzz。不選整個 workspace 都打開（E2：2,922 處，九成以上在 renderer 這一側，其中 1,663 處是 layout、paint 這類的索引；renderer 的 panic 只是那個分頁的 sad tab）。不選不加 lint（E3：這次論證過的，之後再加一處就沒人看）。不選 `catch_unwind` 當最後一道（P2）。
4. **兩個 fuzzer 進每週的 CI（C1）**（2026-11-27，使用者決定）：現有的 `renderer_message`（Browser 對 renderer 的訊息）和這份新的 Viz frame 的 fuzzer，放進每週排程的 job（照 `gc.yml` 的 GC fuzz 的寫法：有時限、失敗的輸入存成 artifact），runner 上裝 cargo-fuzz。理由：CI 現在跑的 fuzz 只有 GC 的（隨機的 GC 設定，不是 cargo-fuzz），cargo-fuzz 的兩種（`browser`、`ipc`）都只在本機跑，每天的 `cargo test` 只重放找到過的輸入；這份的保證靠 fuzzer，lint（第 3 條）擋不住的「訊息的順序破了不變式」正是要長時間跑的。代價：改 workflow；Viz 的 fuzzer 在 CI 上只有 CPU 的 adapter（lavapipe）；失敗要有人看。不選照舊只在本機跑（C2）。`ipc` 的 fuzzer 進不進 CI 不在這份。

## 待決定

開始第 1 段之前要定的四件定了（已決定的事項第 1 到 4 條）。

第 0 步的探測之後定的：數量的上限（一個 frame 的 quad 數、一個 sink 的 shared image 的數量和像素）要不要設、設多少，還是改成分批畫；renderer 的 compositor 自己怎麼守（第 1 條）。

## 之後的可能

- GPU process。
- renderer 的記憶體和 GPU 記憶體的預算（Chromium 的 cc 的 tile 管理）。
- Browser 其他不可信的輸入的 fuzzer（`net` 的回應、DevTools 的訊息）。
