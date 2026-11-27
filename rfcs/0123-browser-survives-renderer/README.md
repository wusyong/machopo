# RFC 0123：renderer 送什麼都不能讓 Browser 結束

- 開始日期：2026-11-27
- 狀態：✅ 已完成：2026-11-27（Linux：Fedora 45，AMD Radeon 890M 和 llvmpipe；`ipc` 的實體記憶體在 macOS 和 Windows 只編過）
- 進度和驗收：`progress.md`
- 細節：`details.md`

## 摘要

renderer process 關在 sandbox 裡，但它送給 Browser 的東西原本有兩條路能讓整個 Browser 結束：它的 frame 是在 Browser 的 process 裡（Viz thread）交給 wgpu 的，wgpu 的錯誤（驗證錯誤、記憶體不夠）預設是 panic，一個 quad 太多的 frame 就能讓 Browser panic（第 0 步的探測）；Browser 處理它的訊息的程式如果 panic，結束的也是 Browser。這份把它們關上：Browser 的 IO thread 解開 renderer 送給 Viz 的訊息就檢查（`CompositorFrameSinkMsg::check`），說不通的是那個 renderer 的壞訊息、Viz 收不到；wgpu 的錯誤用 error scope 和 uncaptured error 的 handler 接住，照種類處理（debug build 的驗證錯誤 panic，讓測試看到漏掉的檢查）；GPU 的記憶體由 Browser 分：照 Blink 的公式算每個 renderer 的 tile 的預算，compositor 照距離留 tile，Viz 再對每個 client 和全部設上限；`browser`、`viz`、`net` 裡會 panic 的地方都寫明不變式（clippy 的 lint）。這是 site isolation 路線圖（RFC 0122）的 A，里程碑 M1 的第一份。

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
4. 上面三條有測試守著。

**不在範圍內**：

- GPU process（驅動程式自己 crash、device 遺失重建不了）：另外的路線。
- renderer 的記憶體上限、CPU 的用量：失控的 renderer 拖慢整台機器是另一件事。
- Browser 的其他不可信的輸入的檢查：網路的回應（`net`：這份只替它打開 panic 的 lint，第 15 條）、DevTools 的 client、WebDriver。
- ORB、process lock：路線圖的 N、B。

要解決的 `docs/limitations.md` 裡的限制：「隔離」的「renderer 還有兩條路能讓 Browser 結束」；「Viz、GPU 和記憶體」的「Viz 收到 renderer 送來的不合法的 frame……Chromium 會結束那個 client」。

## 設計概要

給 machopo 的使用者：看不到改變；一個壞掉（或被攻破）的分頁只會讓自己變成 sad tab。一個分頁用的 GPU 記憶體有上限（最少 512 MB，照螢幕放大），超過時捲得快的地方可能先看到背景色的 tile。

給改 machopo 的人：

- 在 `protocol::viz` 加新的 quad、新的欄位時，要在 `CompositorFrameSinkMsg::check` 說它合格的範圍。
- Viz 呼叫 wgpu 的新地方包在 `GpuContext::scoped` 裡；不經過錯誤回報、直接 panic 的 wgpu 呼叫（`DeviceExt::create_buffer_init`）不用。驗證錯誤表示檢查漏了：debug build 是 panic。
- `browser`、`viz`、`net` 和 `protocol` 的 `viz.rs` 裡新的 `unwrap`、`expect`、索引，clippy 會擋：改寫成不 panic 的，或寫帶理由的 `#[expect]`（`crates/base/README.md`）。
- compositor 的 raster 改成非同步時，commit 時放掉 active tree 的 tile 那一步要拿掉，預算改成兩棵樹一起分（第 12 條）。

## 詳細設計

下面每一節是選項和比較，最後是選了哪個（理由在「已決定的事項」）。探測、量測和 Chromium 的原始碼在 `details.md`。

### 說不通的訊息

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| V1 | **算壞訊息**，結束那個 renderer（`BadMessageReason` 多一種） | 和 Browser 其他的檢查一樣；被攻破的 renderer 不能一直試；和 Chromium 一樣 | machopo 自己的 compositor 的 bug 也變成 sad tab（看得到） |
| V2 | 照舊丟掉、記一筆 | 不用改 | 被攻破的 renderer 可以無限次地試；錯誤藏在 log 裡 |

定了 **V1**，只對「這一則訊息自己說不通」的（第 1 條的表）；時序（device 換了、sink 不在了、大小變了）、資源不夠、NaN 這類的不算。檢查放在 IO thread 解碼之後（B1，第 6 條，Chromium 在 mojo 解碼時驗），不過的 Viz 收不到；Viz 不再檢查一次（第 11 條：thread renderer 和 Browser 的 UI 是 machopo 自己的 compositor，debug build 送出前檢查，Viz 收到說不通的也畫得下去）。檢查的項目在第 6 條的表：沒有 pass、root 的輸出是空的；quad（262,144）、shared quad state（quad 的上限加 1）、render pass（1,024）的數量；quad 的 state 和 resource 是 frame 的、resource 的 id 不重複；resource 和 `CreateSharedImage` 的像素和大小；renderer 的 frame 不嵌別的 surface（實作時找到 Viz 以前照收，第 6 條）。

### wgpu 的錯誤

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| G1 | **先驗完，再加一層接住的**：(a) 交給 wgpu 之前把會造成錯誤的值和數量擋掉；(b) 畫一個 frame、上傳和送出 shared image 包在 error scope 裡；(c) uncaptured error 的 handler | 檢查漏了也不結束 Browser；debug build 把接到的驗證錯誤當成失敗，漏掉的檢查找得到 | 每個 frame 三層 error scope 的進出（量過：約 0.2 µs） |
| G2 | 只裝 uncaptured error 的 handler | 最少 | 不知道是哪一次；壞的 frame 畫一半 |
| G3 | 只先驗完，錯誤照舊 panic | 沒有執行時的成本 | 漏一個檢查就能結束 Browser；記憶體不夠擋不住 |

定了 **G1**（第 2 條），接到的照種類處理（第 7 條）：驗證錯誤丟掉那一次（frame 不 present、不給 copy，shared image 當成沒建出來），debug build panic；記憶體不夠也丟掉，再放掉 texture 的池子；內部錯誤當成 device 遺失。第 0 步的探測找到 `create_buffer_init` 在 buffer 建不出來時、錯誤進了 scope 之後照樣 panic：每個 frame 的頂點改成 Viz 自己建的 buffer，照 `MAX_QUADS` 分成幾個。GPU 的記憶體用完時，llvmpipe 在 wgpu 裡 segfault、什麼都接不到：只有不讓它用完擋得住（下兩節）。

### tile 的記憶體

| 什麼 | 做法 |
| --- | --- |
| 預算 | Browser 照 Blink 的 `GetGpuMemoryPolicy` 算（1152 MB 對 2056 x 1329、scale 2，照螢幕的像素放大；最少 512 MB；最多實體記憶體的四分之一），放進 `RendererFlags::tile_memory_limit`（第 8 條：renderer 在 sandbox 裡不知道螢幕和機器） |
| 分給誰 | 照 cc 的 `AssignGpuMemoryToTiles` 的核心（第 9 條）：和可見範圍重疊的 tile 最先，再照距離；一張放不下之後的都放不下；放不下的放掉，可見的畫成 layer 的背景色（`OOM_MODE`），之後放得下時 raster |
| 單色的 | 不佔：還沒 raster 的先照 paint 的指令預測（第 13 條） |
| 兩棵樹 | commit 建好 pending tree 就放掉 active tree 沒被接手的 tile（第 12 條：raster 是同步的，active tree 不會再被畫） |

量測（`details.md` 的「第 3 段的量測」）之後每一頁都在預算以內：1920 x 1080、scale 2 時 Gutenberg 123 MB、SQLite 22 MB、NewsSite-Next 207 MB；刻意做的 2,000 個捲動容器剛好到 512 MiB。

### Viz 的上限

| | 做法 | 說明 |
| --- | --- | --- |
| V1 | **每個 client 的位元組和張數，所有 client 合計的位元組** | Chromium 的 GPU process 只記帳，兩個上限在 renderer 的 cc；machopo 的 GPU 在 Browser 裡，所以 Viz 再擋一次，合計代替「GPU process 當掉也沒關係」 |
| V2 | 每個 client 只限位元組 | 一大堆 1×1 的圖只佔幾 MB |
| V3 | 只限合計 | 一個分頁用光合計，其他分頁建不出 tile |

定了 **V1**（第 10 條）：每個 client 是預算的兩倍、65,536 張，合計是實體記憶體的一半；超過的那個 `CreateSharedImage` 不做、不結束 renderer（第 5 條）。量到的最多是一個分頁 512 MiB、2,310 張。

### Browser 處理訊息時的 panic

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| P1 | **不接 panic：不 panic 是程式的性質，靠檢查、審查和 lint** | 狀態不會壞一半；和 Chromium 一樣（`CHECK` 失敗是 crash） | 不變式錯了照樣結束 Browser |
| P2 | P1，再在訊息的入口 `catch_unwind` | 漏掉的也只結束一個分頁 | panic 的當下狀態可能改了一半（session history、被 poison 的鎖），之後可能安靜地錯 |

定了 **P1**，加上 lint（第 3、15 條）：`browser`、`viz`、`net` 的根和 `protocol` 的 `viz.rs` 打開 `unwrap_used`、`expect_used`、`indexing_slicing`、`panic`、`unreachable`。原本的 80 處多半改寫成不 panic 的（`net` 的解壓縮、鎖、Viz 的 resource、commit 的導覽），其他寫帶理由的 `#[expect]`（session history 的 `current` 和 `items` 的不變式，逐一讀過所有改它們的地方）。不打開整個 workspace（2,922 處，九成以上在 renderer 這一側，那裡的 panic 是那個分頁的 sad tab）。

### fuzzer

| | 做法 | 說明 |
| --- | --- | --- |
| Z1 | **不加，CI 也不跑** | 檢查有單元測試和 mutation test；debug build 的驗證錯誤 panic，每天的測試都經過 Viz |
| C1 | Viz 的 frame 的 fuzzer，兩個 fuzzer 進每週的 CI | 原本選的 |
| Z2 | `cargo test` 裡固定種子的隨機輸入 | 不照涵蓋率找，淺 |

定了 **Z1**（第 14 條，取代第 4 條）。現有的 `renderer_message` fuzzer 照舊只在本機跑。

### 測試

- **說不通的訊息**：`protocol` 的單元測試（每一種、每個上限的邊）；`browser` 的整合測試：假的 renderer process 送說不通的 frame、shared image 和嵌別人 surface 的 frame，分頁收到壞訊息、process 被結束、Viz 沒收到，說得通的到 Viz（原本打算是 `shell` 的整合測試，但 `machopo` 的執行檔裡的 renderer 送不出說不通的 frame）；Viz 收到說不通的畫得下去、沒有 wgpu 的錯誤。
- **wgpu 的錯誤**：scope 接到它包著的、只接那些；沒有 scope 的由 handler 收；三種各自的處理；debug build 的驗證錯誤 panic；有錯誤的 frame 不給 copy、下一個照畫；比一個 buffer 多的頂點從幾個 buffer 畫。GPU 和 CPU 的 adapter 都過。
- **記憶體**：實體記憶體和 `/proc/meminfo` 一致；預算和上限的公式；compositor 照距離給、一張放不下之後的都放不下、單色的不佔、commit 時舊的 tile 先放掉；renderer process 收到預算；Viz 的三個上限各自擋得住、讓出空間。
- **lint**：四個範圍在 clippy 下沒有警告。
- mutation test：第 1 段 32 個、第 2 段 13 個、第 3 段 28 個，全部抓到（`progress.md`）；第 4 段沒有（lint 是靜態的，改寫照原本的行為）。

### 分階段

0. 草稿、盤點和探測（做完；`details.md` 的「第 0 步的探測」）。
1. **說不通的訊息是壞訊息**（做完，2026-11-27；第 1、6 條）：`CompositorFrameSinkMsg::check`、IO thread、quad 的上限、compositor 守上限。
2. **wgpu 的錯誤接住**（做完，2026-11-27；第 2、7 條）：error scope、handler、頂點的 buffer。
3. **GPU 的記憶體**（做完，2026-11-27；第 8 到 13 條）：預算、compositor 的分配、Viz 的上限、量測，量測之後 Viz 不再檢查 frame、commit 時放掉 active tree 的 tile、單色的不佔。
4. **panic 的 lint**（做完，2026-11-27；第 14、15 條）。

## 缺點

- 每個 frame 多一點成本：IO thread 的檢查（2,000 個 quad 約 30 µs，開發的 build）、三層 error scope（約 0.2 µs）、compositor 照距離排 tile。
- 上限是誠實的網頁可能撞到的地方：quad 超過 262,144 個時 compositor 丟掉最底下的；tile 的預算讓極重的頁面少了預先畫好的 tile，捲得快時先看到背景色。
- machopo 自己的 compositor 的 bug 以前是畫錯或沒畫，現在是 sad tab：比較吵，但看得到。
- 沒有 fuzzer 找沒想到的輸入（第 14 條）；GPU 驅動自己的 bug、記憶體用完時驅動不回報的，照樣結束 Browser（`docs/limitations.md`）。

## 理由和其他做法

- **不做的話**：被攻破的 renderer 不用逃出 sandbox，送一個 quad 太多的 frame 就結束整個 Browser；site isolation 後面幾份的前提（壞訊息只結束那個 renderer）不成立。
- **直接做 GPU process**：Chromium 的做法，wgpu 的錯誤和驅動的 crash 都只影響 GPU process，記憶體用完時是它結束、Browser 重開它；但那是另一個量級的工作（renderer 的 frame、Browser 的 UI 都要跨 process，各平台的 GPU sandbox），frame 的驗證照樣要做。做 WebGL、WebGPU 或把 raster 搬到 Viz 那一側之前該做。
- 各節的選項比較在上面。

## 先例

- **Chromium**（2026-11-27 對了原始碼，`details.md` 的「Chromium 怎麼做」）：Viz 在 GPU process；mojo 的 traits 讀 `CompositorFrame` 時驗形狀（不數 quad），`MaybeSubmitCompositorFrame` 驗 surface 的狀態，不過的關掉那個 `CompositorFrameSink` 的 pipe；`SharedImageStub` 建不出 shared image（記憶體不夠也是）時停掉那個 client 的 GPU channel；GPU process 對 client 只記帳（`MemoryTracker`），tile 的預算在 renderer 的 cc（`GetGpuMemoryPolicy`、`TileManager`）；Browser 的 `CHECK` 失敗是 crash，DoS 不算安全漏洞（security FAQ）。
- **Firefox**：沒查。

## 已決定的事項

1. **不合格的 frame 算那個 renderer 的壞訊息（V1）**（2026-11-27，使用者決定）：Viz 告訴 Browser 是哪個 sink，Browser 照 `received_bad_message` 結束那個 renderer、分頁顯示 sad tab（`BadMessageReason` 多一種）。Browser 自己的 UI 的 frame（root sink）不合格是 machopo 的 bug：debug build 是 assert，release 丟掉。理由：和 Browser 其他的檢查同一條路；被攻破的 renderer 不能一直試；Chromium 也是不過的結束那個 client。代價：Viz 要多一條回 Browser 的路；machopo 自己的 compositor 的 bug 也變成 sad tab（比較吵，但看得到）。不選照舊丟掉、記一筆（V2）。（誰檢查、怎麼回報，第 6 條改成 IO thread 解碼時、走現有的壞訊息的路，Viz 不必回 Browser。）
   **只有「這一則訊息自己說不通」的才算**（2026-11-27，使用者問「不合格的 frame 會不會是不穩定造成的」之後補的）：誠實的 renderer 在任何時序下都不會送的（像素的長度和大小對不上、超過兩邊都知道的 texture 上限、超過 renderer 自己也守的數量上限）。其他的照下表，實作時每一條檢查標明是哪一類：

   | 情況 | 處理 |
   | --- | --- |
   | 這一則訊息自己說不通 | 壞訊息，結束 renderer（硬體不穩造成的也是：重新載入就好） |
   | 取決於 Viz 或 Browser 的狀態：frame 提到的 shared image 是舊的 device 上的（不認得的 mailbox）、sink 已經註銷（分頁在關）、frame 還是舊的大小、沒有 GPU 時送來的 shared image | 忽略或不畫，和現在一樣：renderer 送出的當下不可能知道 |
   | 資源不夠（GPU 的記憶體） | 丟掉那一次，不算誰的 |
   | 值很怪但無害（NaN、無限大的座標：machopo 自己的 bug 的樣子） | 那個 quad 不畫；renderer 的 debug build 在送出前自己 assert，bug 在源頭、帶著 backtrace 被看到，不是等 Viz 變成 sad tab |

   Chromium 的原則也是這樣：只有 renderer 本來就該知道不能這樣的才結束它，可能因為時序發生的一律忽略。實作時要釐清的：device 遺失之後 renderer 用同一個 mailbox 的編號重建 shared image，和現在「同一個編號建兩次」的檢查會不會撞。
2. **wgpu 的錯誤：先驗完，再用 error scope 和 uncaptured error 的 handler 接住（G1）**（2026-11-27，使用者決定）：(a) 交給 wgpu 之前把會造成錯誤的值和數量擋掉（第 1 條的檢查）；(b) 每次畫一個 frame、每次上傳 shared image 包在 error scope 裡（`Validation`、`OutOfMemory`、`Internal` 三種各一層），有錯誤就丟掉那一次；(c) 裝 uncaptured error 的 handler，記錯誤、不 panic。(b)、(c) 接到的一律只丟掉、記錯誤，不結束 renderer（記憶體不夠不是誰的錯；驗證錯誤表示 (a) 漏了，是 machopo 要修的；畫的時候幾個 sink 的 quad 合在一起，也分不出是誰的）：結束 renderer 只靠 (a)。debug build 和 fuzzer 把 (b)、(c) 接到的驗證錯誤當成失敗，漏掉的檢查才找得出來。理由：(b)、(c) 就是 wgpu 自己說的兩種處理錯誤的方式（`Error` 的文件：「By default, errors translate into panics……use `Device::push_error_scope()` or `Device::on_uncaptured_error()`」）；但它們接不住全部：wgpu 自己的 Rust 程式裡有直接 panic 的路（`create_buffer_init` 建不出 buffer 時，錯誤進了 error scope，下一行對 mapped range 的 `expect` 照樣 panic；照程式讀的，`details.md` 的「wgpu 的錯誤處理」），那些只有 (a) 擋得住，或換成不會 panic 的寫法。error scope 是每條 thread 一份，Viz 的 wgpu 呼叫都在 Viz thread 上。代價：每個 frame 多幾次 error scope 的進出（第 0 步量）。不選只裝 handler（G2：不知道是哪一次，壞的 frame 畫一半）。不選只先驗完、錯誤照舊 panic（G3：漏一個檢查就能結束 Browser，記憶體不夠擋不住）。Chromium 在 GPU process 驗證命令，machopo 沒有 GPU process。
3. **Browser 的 panic 不接；處理 renderer 輸入的模組用 lint 守（P1 加 E1）**（2026-11-27，使用者決定）：不在訊息的入口 `catch_unwind`。`browser` 處理 renderer 訊息的模組和 `viz` 收 frame 的模組打開 clippy 的 `unwrap_used`、`expect_used`、`indexing_slicing`、`panic`、`unreachable`；留下的每一處用 `#[expect(lint, reason = "...")]`，理由寫 renderer 為什麼破不了那個不變式（和整數運算的規則同一個寫法，`crates/base/README.md`）；測試不套用。理由：2026-11-27 用 clippy 數過（只編 lib 和執行檔，不含測試；`details.md` 的「會 panic 的寫法有多少（量的）」）：整個 workspace 2,922 處，renderer 的輸入在 Browser 這一側走得到的只有約 50 處（`browser` 43：22 個 `expect`、21 個索引，28 處在 `navigation_controller.rs`；`viz` 9），逐一寫理由做得到；這份本來就要逐一論證它們，lint 讓論證留在程式裡，之後新加的被擋下來、不靠人記得，補上「不接 panic」漏一處就結束 Browser 的弱點。不接的理由：panic 的當下狀態可能改了一半（session history、被 poison 的鎖），接住之後的行為沒有保證，可能安靜地錯；Chromium 的 `CHECK` 失敗也是 Browser crash，靠驗證和 fuzz。不選整個 workspace 都打開（E2：2,922 處，九成以上在 renderer 這一側，其中 1,663 處是 layout、paint 這類的索引；renderer 的 panic 只是那個分頁的 sad tab）。不選不加 lint（E3：這次論證過的，之後再加一處就沒人看）。不選 `catch_unwind` 當最後一道（P2）。（範圍第 15 條改成整個 `browser`、`viz` crate，加上 `net` 和 `protocol` 的 `viz.rs`。）
4. **兩個 fuzzer 進每週的 CI（C1）**（2026-11-27，使用者決定）：現有的 `renderer_message`（Browser 對 renderer 的訊息）和這份新的 Viz frame 的 fuzzer，放進每週排程的 job（照 `gc.yml` 的 GC fuzz 的寫法：有時限、失敗的輸入存成 artifact），runner 上裝 cargo-fuzz。理由：CI 現在跑的 fuzz 只有 GC 的（隨機的 GC 設定，不是 cargo-fuzz），cargo-fuzz 的兩種（`browser`、`ipc`）都只在本機跑，每天的 `cargo test` 只重放找到過的輸入；這份的保證靠 fuzzer，lint（第 3 條）擋不住的「訊息的順序破了不變式」正是要長時間跑的。代價：改 workflow；Viz 的 fuzzer 在 CI 上只有 CPU 的 adapter（lavapipe）；失敗要有人看。不選照舊只在本機跑（C2）。`ipc` 的 fuzzer 進不進 CI 不在這份。
5. **數量的上限：quad 數固定、超過算壞訊息；tile 的預算在 renderer，Viz 另設上限、超過只丟掉（Q1 加 S1，renderer 的預算放在這份）**（2026-11-27，使用者決定）：
   - 一、**一個 frame 的 quad 數**：protocol 的常數（暫定 2^18 = 262,144 個，頂點約 120 MiB），超過算壞訊息：從訊息本身就看得出（第 1 條）；renderer 的 compositor 送出前守同一個常數（超過時少畫後面的，debug build 會 assert）。理由：Viz 把一個 frame 的頂點放在一個 wgpu 的 buffer 裡，上限 256 MiB（machopo 要的 limits，和 adapter 無關；`details.md` 的「第 0 步的探測」）；Viz 只有一條 thread，幾百萬個 quad 的 frame 卡住所有的分頁。Chromium 的 Viz 不數 quad（`details.md` 的「Chromium 怎麼做」），它畫 quad 不受一個 buffer 的限制。不選頂點分批畫（Q2：記憶體能用固定的 buffer 分幾次 submit 擋住，Viz thread 的時間擋不住）。不選超過只丟掉（Q3：誠實的 bug 看不到）。
   - 二、**shared image**：renderer 的 compositor 有 tile 的記憶體預算，照 Chromium 的 `GetGpuMemoryPolicy`（最少 512 MB、照螢幕的像素放大、最多實體記憶體的四分之一），超過時照 cc 的 TileManager：先放掉 prepaint 的 tile，現在要畫的放不下時畫成單色。Viz 另有每個 sink 的上限（暫定 renderer 預算的兩倍）和所有 sink 合計的上限，超過的那個 `CreateSharedImage` 不做、記錯誤，不結束 renderer（S1；第 1 條的「資源不夠」）。數字在第 3 段量過誠實的頁面之後定：`scripts/bench/raster` 的頁面（Gutenberg、SQLite、NewsSite-Next；捲動、改視窗大小、scale 2），加上刻意做的三種（一萬個 `will-change` 的元素、4K 的視窗裡一個很大的 layer、很多分頁）（量的時候改成 2,000 個捲動容器和一個長頁面：machopo 的 `will-change` 不成為 layer，捲動容器才會；很多分頁是每個分頁的量相加，沒有另外量；`details.md` 的「第 3 段的量測」）。理由：Chromium 的預算是 renderer 自己守的，GPU process 那一側沒有上限，建不出 shared image 時停掉那個 client 的 GPU channel，GPU process 當掉時 Browser 重開它；machopo 的 GPU 在 Browser 的 process 裡，記憶體用完時（至少 llvmpipe）是 segfault、error scope 接不到，所以 Browser 這一側要有上限。誠實的 renderer 照預算就不會撞到 Viz 的上限，所以超過的只丟掉、不結束 renderer。不選超過算壞訊息（S2：預算估錯時誠實的重頁面是 sad tab）。不選像 Chromium 一樣丟掉那個 sink 所有的 shared image、告訴它 context 不見了（S4：renderer 重建、又撞到，一直重來）。不選只靠 OutOfMemory 的 error scope（S3：接不到 segfault）。renderer 的預算放在這份而不是另一份：沒有它，Viz 的上限沒有依據，誠實的重頁面也會缺 tile。
6. **第 1 段：只看訊息本身的檢查在 IO thread 解碼之後跑（B1），算壞訊息的照下表**（2026-11-27，使用者決定）：檢查是 `protocol::viz` 的一個函式，Browser 的 IO thread 解開 `CompositorFrameSinkMsg` 之後就跑，不過的走現有的壞訊息的路（分頁結束那個 renderer）；renderer 的 compositor 送出前在 debug build 跑同一個並 assert。原本 Viz 也對每個 frame 再跑一次（同一個 process 裡的 thread renderer，`--single-process`，不經過 IO thread），不過的丟掉；第 11 條拿掉了。要看 Viz 的狀態的（mailbox 認不認得、sink 登記了沒）照第 1 條只丟掉。理由：Chromium 在 mojo 解碼時驗（`StructTraits` 的 `Read`，`details.md` 的「Chromium 怎麼做」）；IO thread 已經有回報壞訊息的路，Viz 沒有回 Browser 的路。代價：IO thread 每個 frame 多走一次 quad。不選在 Viz 驗、另開一條回 Browser 的通道（B2：多一條通道，Viz 要從 sink 找到分頁）。

   | 檢查 | 之後 |
   | --- | --- |
   | 沒有 render pass、最後一個 pass 的 `output_rect` 是空的 | 壞訊息 |
   | quad 的 `shared_quad_state` 越界 | 壞訊息（以前用預設的） |
   | quad 提到 `resource_list` 沒有的 resource | 壞訊息（以前不畫那個 quad） |
   | `resource_list` 的 id 重複 | 壞訊息 |
   | resource 的像素和大小不合、超過 `MAX_TEXTURE_SIZE` | 壞訊息（以前整個 frame 丟掉） |
   | quad 數、shared quad state 數（不超過 quad 的上限加 1）、render pass 數（暫定 1,024）超過上限 | 壞訊息 |
   | renderer 的 frame 嵌別的 surface（`Material::Surface`） | 壞訊息（以前照收） |
   | `CreateSharedImage` 的大小是空的、像素不合、超過 `MAX_TEXTURE_SIZE` | 壞訊息（以前不建） |
   | 同一個 mailbox 建兩次、frame 提到不認得的 mailbox | 照舊只丟掉（看 Viz 的狀態；device 換過之後會重建） |
   | NaN、無限大的座標 | 照舊畫（探測沒有錯誤），renderer 的 debug build assert |

   和定的時候不同的兩處（實作時找到的，`progress.md` 的「做過的檢查」）：shared quad state 原本打算是「不超過 quad 數加 1」，但 renderer 的 compositor 每個有 clip 的 layer 給一個 state，那個 layer 的 tile 一個都沒畫時 state 照樣在，照 quad 數會讓誠實的 frame 變成壞訊息，所以改成 quad 的上限加 1；嵌 surface 是表裡原本沒有的：Viz 以前照收任何 sink 的 `Material::Surface`，renderer 能把別的分頁或 Browser 的 UI 的畫面嵌進自己的 frame，renderer 的 compositor 從來不嵌，所以算壞訊息（之後有 OOPIF 時照 Chromium 的 frame sink 的階層放行）。

   quad 的上限是一個 sink 的一個 frame 的：Browser 的 UI 和分頁的 frame 合起來畫時會超過，第 2 段換掉 `create_buffer_init` 時照上限把頂點分成幾個 buffer。
7. **第 2 段：接到的錯誤照種類處理，debug build 的驗證錯誤 panic（E1 加 D1）**（2026-11-27，使用者決定）：`Validation`（machopo 的檢查漏了）丟掉那個 frame、畫面留著上一個，上傳的 shared image 當成沒建出來，記錯誤；`OutOfMemory` 一樣，再放掉 Viz 的 texture 的池子，下一個 frame 再試；`Internal` 當成 device 遺失，走 `about:gpucrash` 的路（新的 device，client 重建它的 shared image）。debug build 接到 `Validation` 時 panic（每天的 `cargo test`、reftest、情境、WPT 和 fuzzer 都看得到漏掉的檢查），release 只記錯誤；Viz 自己測「接得住」的測試用只有測試設得到的開關關掉那個 panic；`OutOfMemory`、`Internal` 在 debug build 也不 panic（不是 machopo 的 bug）。理由：Chromium 的 GPU process 在 context 出錯時是 lost context，`Internal` 照它；另外兩種不必每個分頁都重建 tile。不選三種都當成 device 遺失（E2：漏一個檢查、記憶體不夠一次，所有的分頁閃一下、重建 tile）。不選 debug build 也只記錯誤、測試讀計數（D2：每個測試都要記得看）。
8. **第 3 段：tile 的預算由 Browser 算，放進啟動 renderer 的參數（M1）**（2026-11-27，使用者決定）：照 Chromium 的 `GetGpuMemoryPolicy` 的公式（最少 512 MB、照螢幕的像素放大、最多實體記憶體的四分之一），螢幕是視窗所在的那個，headless 用視窗的大小；renderer 的 `RendererFlags` 帶著它。理由：renderer 在 sandbox 裡讀不到記憶體的大小（Landlock 不放行 `/proc/meminfo`），不必放寬；Viz 的上限也是 Browser 定的，兩邊用同一個數字。代價：預算在 renderer 啟動時定了，視窗移到別的螢幕不跟著變（Chromium 也只看啟動時的螢幕，`initial_screen_size`）。不選 renderer 自己算（M2：要放行 `sysinfo` 這類 syscall，或讀 `/proc`）。
9. **第 3 段：超過預算時照 tile 離可見範圍的距離留（T1）**（2026-11-27，使用者決定）：可見的最優先，再來是 interest rect 裡近的，從最遠的放掉；可見的都放不下時，那些 tile 畫成 layer 的背景色。理由：這是 cc 的 TileManager 的核心（照優先順序、先放掉低的，放不下的現在要畫的是 `OOM_MODE`），不用它整套。代價：沒有 cc 照捲動的方向和速度的預測（`skewport`），捲得快時前面的 tile 不比後面的優先。不選照搬 cc 的 priority bin（NOW、SOON、EVENTUALLY）和 eviction queue（T2：大很多，machopo 的 tile 還沒有那些結構）。
10. **第 3 段：Viz 的上限是每個 sink 的位元組和張數、所有 sink 合計的位元組（V1）**（2026-11-27，使用者決定）：超過的那個 `CreateSharedImage` 不做（第 5 條）。數字在量過誠實的頁面之後定（暫定：每個 sink 是 renderer 的預算的兩倍、65,536 張，合計是實體記憶體的一半）。理由：Chromium 的 GPU process 對每個 client 只記帳（`MemoryTracker`）、不設上限，兩個上限（位元組 `bytes_limit_when_visible`、張數 `kDefaultNumResourcesLimit`）都在 renderer 的 cc 裡，GPU process 記憶體用完時是它被結束、Browser 重開它（`details.md` 的「GPU process 的記憶體：記帳，不設上限」）；machopo 沒有 GPU process，所以 Viz 再擋一次 cc 的那兩個，合計的上限代替「GPU process 當掉也沒關係」。張數不照 cc 的一千萬：那是 renderer 給自己的寬鬆上限，Viz 每張有一個 texture 和 bind group。不選每個 sink 只限位元組（V2：一大堆 1×1 的圖只佔幾 MB）。不選只限合計（V3：一個分頁用光合計，其他分頁建不出 tile）。
11. **Viz 不再檢查它收到的 frame（O1）**（2026-11-27，使用者決定；改了第 6 條）：只有 IO thread 檢查。原本 Viz 對每個 frame 再檢查一次，為的是不經過 IO thread 的 thread renderer（`--single-process`）和 Browser 的 UI；但量了之後（一個 frame 2,000 個 quad 約 30 µs，`progress.md`），process 的 renderer 的 frame 每個都多花一次，而那一次對 thread renderer 也沒有作用：它在 Browser 的 process 裡、compositor 是 machopo 自己的，被攻破時 Browser 已經被攻破；抓 machopo 自己的 bug 由 compositor 的 debug build 送出前的檢查做；第 2 段之後 Viz 收到說不通的 frame 也畫得下去（不是 frame 的 state 用預設的、不是 frame 的 resource 不畫、頂點分 buffer、wgpu 的錯誤接住），不會讓 Browser 結束。只有 Browser 的 UI 嵌別的 surface，renderer process 的由 IO thread 守，machopo 的 thread renderer 從來不嵌。shared image 的大小和像素在 Viz 照舊檢查（`Display::create_shared_image`：一張一次，不是每個 frame）。不選只對沒經過 IO thread 的 sink 檢查（O2：註冊 sink 的訊息要多帶一個欄位，省不到的只是 `--single-process`）。
12. **commit 時放掉 active tree 裡 pending tree 沒接手的 tile（A1）**（2026-11-27，使用者決定）：量測時（`progress.md`）改視窗大小的那一下，一個分頁在 Viz 用到預算的將近兩倍（2,000 個捲動容器的頁面，1,010 MiB，上限 1,024 MiB）：commit 建好的 pending tree 重新 raster 一整批，舊的 active tree 拿著它的那批到 activate 才放。machopo 的 raster 是同步的，兩個 proxy 都在同一個呼叫裡 commit、`prepare_tiles`、`activate`，active tree 在這之間不會再被畫，那些 tile 沒有用處：建好 pending tree 就放掉（它接手的是同一張 shared image，不受影響），一個分頁最多用到預算，Viz 的上限（兩倍）有一倍的空間。前提是同步的 raster：Chromium 的 raster 是非同步的（GPU process 裡），active tree 在 pending tree raster 的那幾個 frame 裡照樣畫、捲動，兩邊的 tile 都要留，預算由 cc 的 `TileManager` 分給兩棵樹；machopo 改成非同步的 raster 時要拿掉這一步、改成那樣（`docs/limitations.md`）。不選 Viz 的上限改成三倍（U1：記憶體照樣白白多一倍）。不選照 cc 讓預算涵蓋兩棵樹（U2：同步的 raster 底下不必要，改視窗大小時 pending tree 的可見 tile 會因為舊的還在而放不下）。
13. **單色的 tile 不佔預算：分配之前先看 paint 的指令（S1）**（2026-11-27，使用者決定）：還沒 raster 的 tile 先照它的指令預測是不是單色（原本 raster 之前做的同一件事，搬到分配之前），是的不算記憶體。理由：量測時 NewsSite-Next 在 Viz 只用了 259 MB，compositor 卻以為超過 512 MiB、放掉最多 7,863 張：還沒 raster 的一律算一整張，其中很多是單色的。在預算內的本來就要預測，沒有多做；超過的也預測一次（比 raster 便宜得多）。不選照舊（S2：cc 也是 raster 之前先算一整張，但誠實的頁面少了很多預先畫好的 tile）。
14. **不加 fuzzer，CI 也不跑（Z1）**（2026-11-27，使用者決定；取代第 4 條）：Viz 的 fuzzer 不寫，現有的 `renderer_message` fuzzer 照舊只在本機跑。理由：誠實的 renderer 送的本來就說得通，這份做的檢查、上限是對被攻破的 renderer 和 machopo 自己的 bug；守著它們的是 IO thread 的檢查（每一條有單元測試、mutation test）、debug build 遇到 wgpu 的驗證錯誤 panic（每天的 `cargo test`、reftest、情境、WPT 都經過 Viz），不再多維護一個 cargo-fuzz 的 target 和 CI 的 job。代價：沒有東西找沒想到的輸入（`docs/test-gaps.md`）。原本選的是兩個 fuzzer 進每週的 CI（C1）；不選改成 `cargo test` 裡固定種子的隨機測試（Z2：不照涵蓋率找輸入，找得到的淺）。
15. **第 4 段：lint 打開在整個 `browser`、`viz`、`net` crate 和 `protocol` 的 `viz.rs`（L1 加 `net`）**（2026-11-27，使用者決定；第 3 條的範圍）：在 crate（`viz.rs` 是模組）的根打開，以後新的模組自動在裡面。`net` 原本不在：網路上的回應是比 renderer 更直接的不可信的輸入（不必先攻破 renderer），它在 Browser 的 process 裡。不選只標碰得到 renderer 的輸入的那幾個檔案（L2：新的模組要有人記得標）。不開一份讓整個 workspace 都不能 panic 的 RFC（2,922 處，九成以上在 renderer 這一側，那裡的 panic 是那個分頁的 sad tab；硬改成 `get` 會把「立刻停下」換成「默默畫錯」；Chromium 的方向是多用 `CHECK`）：管的是信任的邊界，不是 panic 本身。

## 待決定

沒有。

## 之後的可能

- GPU process（做 WebGL、WebGPU 或把 raster 搬到 Viz 那一側之前）。
- 非同步的 raster：tile 的預算兩棵樹一起分、activation 的條件（第 12 條）。
- renderer 的 tile 的優先順序照捲動的方向和速度（cc 的 `skewport`）。
- Browser 其他不可信的輸入的 fuzzer（`net` 的回應、DevTools 的訊息）。
- panic 時經過 log 記下來、log 寫進檔案（現在只到 stderr）。
