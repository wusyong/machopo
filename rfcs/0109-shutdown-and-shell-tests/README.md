# RFC 0109：結束的流程，和 `shell` 的測試

- 開始日期：2026-11-22
- 狀態：📝 草稿
- 細節：`details.md`

## 摘要

兩件事，一起做是因為它們互相擋路：

- **結束的流程**：machopo 的每一種跑法（視窗、automation pipe、headless、renderer process、machopodriver 啟動的）怎麼結束，從來沒有從頭到尾盤過；出事時都是一次修一個。這份先把每條 thread、每個 process、每個有狀態的全域變數怎麼結束盤一遍，定成一套規則，再照規則改。
- **`shell` 的測試**：CI 最近壞掉的幾乎都是 `shell` 的測試。這份審查它們為什麼不穩定（等的方式和時間、好幾個測試搶同一個裝置或同一份全域狀態、產品自己的競爭），修掉或隔開，讓 CI 的紅燈重新代表「真的壞了」。

做法還沒有選：這是草稿，先做兩個不改程式的盤點（「分階段」的第 0、1 階段），照結果再定。

## 動機

**CI 的紅燈現在不代表什麼**（2026-11-18 到 22 的 60 次 CI：24 次成功、23 次失敗、12 次取消；每一次壞的是什麼在 `details.md` 的「CI 上壞過的」）：

- 23 次失敗裡 22 次是測試（1 次只有 lint），其中 18 次壞在 `shell`，3 次在 `ipc`，1 次在 `browser`。
- 15 次是同一個測試，`devtools_css::sheets_coming_and_going_are_told`，每次都死在同一行：頁面加了一個 `srcdoc` 的 iframe 之後，等 30 秒收不到它的 `CSS.styleSheetAdded`，machopo 還活著。它在 `main` 和 branch 上都壞，最早的一次是 2026-11-19。看起來是產品自己的競爭（iframe 的文件的 sheet 什麼時候告訴 DevTools），不是結束的問題；還沒有查。
- 其餘的每一種都只出現一兩次，各有各的原因（下面的「已經知道的」）。
- 這段期間做多 process 的那份 RFC 時，每次 push 都要先分辨「這次紅的是不是又是那一個」，有三次真的壞了是混在裡面才看到的。

**結束的流程，已經知道的問題**：

- 2026-11-18 Linux 的 CI 兩次不穩定，都是結束和生命週期的順序：Viz 的 device 放在全域變數裡，`exit` 時 lavapipe 還在忙（`29a94c7` 修的）；被放掉的 renderer 晚到的訊息落在重複使用的 frame sink 上（`5bd5273` 修的）。
- 2026-11-10 兩次「machopo 沒有結束」，原因不明。
- renderer 是自己的 process 之後多出來的：
  - renderer 有三種結束法：收到 `Shutdown` 自己收尾；通道從 Browser 那一側斷了，IO thread 直接 `_exit`（macOS、Linux）；Browser 沒了由 kernel 結束（Windows 的 job）。前一種寫得完 trace，後兩種寫不完。
  - Browser 正常結束時，對每個 renderer 一個一個等，卡住的各等 5 秒才 kill；machopodriver 只等 Browser 5 秒就 kill 它。兩個卡住的分頁就一定超過（量過：一個卡住的分頁，Browser 5.2 秒才結束）。
- 有狀態的全域變數：wgpu 的 instance 和共用的 device、V8 的 platform、Browser 的 IO thread 的迴圈（建了就不停）、macOS 的 rendezvous server。哪些有解構子、誰先誰後、`exit` 時還有哪些 thread 在碰它們，沒有列過。
- Viz 有三處遇到 wgpu 的錯誤就 `std::process::exit(1)`，其他 thread 當時在做什麼不管。
- 結束時還在跑 script 的 renderer（單一 process 時是同一個 process 裡的 thread）。

**`shell` 的測試，已經知道的**：

- 整合測試是一個執行檔，640 個測試、3 萬多行，libtest 用和 CPU 一樣多的 thread 同時跑。
- 兩種跑法混在一起：
  - **在測試的 process 裡建瀏覽器**（`documents.rs` 等）：所有測試共用同一個 process 的全域狀態（一個 GPU device、一個 V8 platform）。
  - **啟動真的 `machopo`**（pipe、DevTools、遠端除錯、WebDriver）：每個測試一個 Browser，現在預設再加每個分頁一個 renderer process。CI 的機器是 4 顆 CPU、沒有 GPU（wgpu 用 lavapipe 在 CPU 上畫），同時有好幾組在搶。
- 等的方式不一致：有的等事件，有的等固定的時間，時限寫死 30 秒。
- 出過的事：兩個 process 共用 stderr 時一行被插隊（已修）；`documents::history_through_the_browser` 兩個事件的順序相反（一次）；`automation_pipe::the_system_info_tells_the_gpu_adapter`（一次）；都沒有查到原因。

**目標**：

做完時要有基本的 graceful shutdown。「基本」定成下面六條，每一種跑法（視窗、automation pipe、headless、machopodriver 啟動的）都要滿足；它們就是這份的驗收條件：

1. **一定結束**：要它結束（關視窗、關 pipe、刪 session、收到 SIGTERM）之後，在固定的時間內結束，不管頁面在做什麼、開了幾個分頁。時限暫定 2 秒（「待決定」）。
2. **不留東西**：結束之後沒有殘留的 process，沒有人還握著它的 stdout、stderr。
3. **順序固定**：每條 thread 有人擁有、有人等它結束，照寫下來的順序停；整個 process 一份的東西（GPU device、V8 的 platform）在還有 thread 用它的時候不會被收掉。結束時不 crash、不卡住。
4. **該寫完的寫完**：正常結束時，profile（cookie、`localStorage`）和 trace 都寫完。
5. **被 kill 也乾淨**：Browser 被強制結束時 renderer 跟著結束（多 process 的那份做完時已經有，這份把它列進表裡、和其他幾條一起守著）。
6. **每一條有測試守著**，而且這些測試照 CI 的跑法重複跑不會壞。

`shell` 的測試：

- 每一個不穩定的測試查到原因，分成：產品的競爭（修產品）、測試等錯了東西（改測試）、搶資源（隔開或排隊）。
- CI 連續綠燈（次數見「待決定」）。

**不在範圍內**：

- 頁面的 `beforeunload`、`unload`、`pagehide` 在關分頁和結束時的行為（要不要跑、給多久）。
- 結束時存 session、下次還原分頁。
- 偵測「不回應」的 renderer 並主動處理（hang monitor）。這份只管「要它結束時它一定結束」。
- 當掉時的收尾（crash reporter 這一類）。
- GPU 驅動程式自己的問題：這份能做的是結束時不讓它還在忙，不能保證驅動程式不出別的事。
- 作業系統的 sandbox：另一份。它會多一種結束法（被 sandbox 擋下而死的 renderer），排在這份之後。
- 把 `shell` 的測試搬到別的 crate 或重寫：只處理不穩定的原因。
- WPT 的 known intermittent 清單：那是另一套機制。

**事先保證不了的**：

- 2026-11-10 那兩次「machopo 沒有結束」原因不明。盤點找不到的話，這份能給的是「結束的順序寫下來了、有測試，那個現象沒有再出現」，不是「確定修掉了那一個」。
- 盤點可能發現要動比較大的地方（例如 thread 的擁有關係要重排）。那時列選項決定，範圍可能比現在想的大。

要解決的 `docs/limitations.md` 和 `docs/test-gaps.md` 裡的項目在第 0 階段盤點時列出來，補在這裡。

## 設計概要

給用 machopo 的人：關掉視窗、結束 `machopo --automation-pipe`、machopodriver 刪掉 session，都在固定的時間內結束，不留下 process，不管頁面在做什麼。

給改 machopo 的人：

- 一張表寫著每條 thread 和每個 process 怎麼結束（放在 `docs/architecture.md`）。新加一條 thread 時要填：誰擁有、誰 join、結束時它卡在哪裡會怎樣。
- `shell` 的測試有一套寫法：怎麼等（等什麼事件、時限怎麼定）、哪些資源是整個 process 一份、哪些測試不能同時跑。
- CI 紅了就是壞了。

## 詳細設計

草稿：這一節只寫兩個盤點要產出什麼。規則和改法等盤點的結果。

**結束的流程的盤點**（第 0 階段，產出放在 `details.md`）：

- **每一種跑法的結束順序**：視窗、`--automation-pipe`（有沒有 `--window`）、headless、renderer process、machopodriver；各自「正常結束」「被要求結束時頁面卡住」「被 kill」三種情況，現在實際發生什麼、花多久（量）。
- **每條 thread**：名字、誰建的、誰擁有它的 handle、誰 join、它阻塞在什麼上面、它不結束時 process 會怎樣。
- **每個有狀態的全域變數**：有沒有解構子、解構時需要哪些 thread 已經停了、`exit` 時實際的順序。
- **每個 child process**：誰結束它、等多久、等不到怎麼辦、它的輸出（stdout、stderr、trace）誰還拿著。
- **對照 Chromium**：`BrowserMainLoop` 的結束順序、renderer 的 fast shutdown、child process 在通道出錯時的行為、GPU 的結束。對照原始碼，不是憑記憶；查不到的寫明。

**`shell` 的測試的審查**（第 1 階段，產出放在 `details.md`）：

- **分類每個測試模組**：在測試的 process 裡跑，還是啟動 `machopo`；用到哪些整個 process 或整台機器一份的東西（GPU device、V8 platform、port、profile 的資料夾、stderr、顯示器）；怎麼等（事件、輪詢、固定的時間）；時限。
- **讓它壞**：在像 CI 的條件下重複跑（CPU 限成 4 顆、`MACHOPO_GPU_ADAPTER=cpu`、同時跑別的負載），收集壞掉的測試和當時的 log。只跑一次看不到的才是這份要找的。
- **每一個壞過的測試查到原因**，寫成一列：現象、原因、屬於哪一類、怎麼修。`devtools_css::sheets_coming_and_going_are_told` 是第一個。
- **查不到的**照 `docs/testing.md` 的規定記下來，不用重跑蓋過去。

### 測試

- 結束的流程：每一種跑法、每一種情況一個測試，量結束花的時間和留下的 process（多 process 的那幾個已經有：`shell` 的 `a_killed_browsers_*`）。
- `shell` 的測試本身：修過的每一個照 CI 的跑法重複跑（次數見「待決定」），寫進 `progress.md`。
- 產品的競爭修掉時照平常的規定：新的測試、mutation test。

### 分階段

每個階段開始前只決定那個階段的事。

0. **盤點結束的流程**：不改程式。產出上面那幾張表和量到的時間。
1. **審查 `shell` 的測試**：不改產品。產出分類表、壞過的每個測試的原因。第 0、1 階段互不相依，先後見「待決定」。
2. **修 `shell` 的測試**：照第 1 階段的分類，一類一類修。產品的競爭各自是一步。
3. **定結束的規則**：照第 0 階段的結果列選項、定下來（Browser 結束時等不等 renderer、全域變數怎麼收、Viz 遇到錯誤怎麼結束）。
4. **照規則改**，一種跑法一步。
5. **驗收**：「目標」的六條逐條對到守它的測試；CI 連續綠燈；`docs/architecture.md` 的表；`docs/limitations.md` 和 `docs/test-gaps.md` 更新。

## 缺點

- 兩個盤點都很花時間，而且產出是文件，不是功能。
- 讓測試在本機「像 CI 一樣壞」不一定做得到：有些只在 CI 的機器上發生（4 顆 CPU、lavapipe、Ubuntu 的 core dump 收集）。做不到的只能靠 CI 的 log。
- 結束的規則定了之後要維護：每加一條 thread、一個 process 都要照著填。
- 把測試隔開或排隊會讓整套測試變慢。

## 理由和其他做法

- **為什麼兩件事放在一份**：`shell` 的測試不穩定時，改結束流程的每一步都分不出是自己弄壞的還是原本就會壞；而 `shell` 的測試裡有一部分不穩定就是結束造成的（2026-11-18 的那兩次）。分成兩份的話第二份要等第一份。
- **不選繼續一次修一個**：2026-11-18 到 22 這五天修了五個（「動機」列的），每一個都是事後從 CI 的 log 猜的；沒有修過的還在（2026-11-10 的那兩次）。
- **不選先重跑蓋過去**（失敗的測試自動再跑一次）：`docs/testing.md` 已經規定不這樣做，而且壞了 15 次的那一個是真的收不到事件。
- **不選先做 sandbox**：它做起來會多出很多「卡住」「莫名結束」的失敗，要有一個乾淨的 CI 才分得出來。

其他做法的完整比較等盤點之後補。

## 先例

盤點時對照原始碼補上。就我們的了解（還沒有查證）：

- **Chromium 的結束**：`BrowserMainLoop` 照固定的順序停各條 thread；renderer 沒有 `beforeunload`、`unload` 的處理時 Browser 直接結束它，不等它收尾（fast shutdown）；POSIX 上 child process 在通道出錯時從 IO thread 直接結束自己，Windows 上靠 job object。
- **Chromium 的測試**：測試的 launcher 把測試分批放進各自的 process 跑，壞掉的自動重試、另外統計不穩定的。machopo 的規定是不重試。

## 已決定的事項

無。

## 待決定

- **第 0、1 階段的先後**。2026-11-22 的看法：先審查 `shell` 的測試。CI 的紅燈大多來自它，先讓 CI 可信，結束流程的每一步才看得出有沒有弄壞東西。
- **`devtools_css::sheets_coming_and_going_are_told` 放在哪一步**。原本說等結束的流程時一起查；看了 CI 的 log 之後，它不像是結束的問題。2026-11-22 的看法：放在第 1 階段的第一個。
- **結束的時限**（目標的第 1 條）：暫定 2 秒。第 0 階段量到現在每一種跑法實際花多久之後定；要分「正常的」和「有卡住的頁面」兩個數字也可以。
- **怎麼算「穩定了」**：CI 連續幾次綠燈、本機照 CI 的跑法重複幾次。`docs/testing.md` 對新測試的規定是上千次。
- **讓測試壞的條件要做到多像 CI**：只限 CPU 和換成 CPU 的 adapter，還是要在 Linux 的機器上（或容器裡）跑。
- **搶資源的測試怎麼隔開**：排隊（一把鎖）、分成另一個執行檔（和「每個 crate 的整合測試是一個執行檔」的規定衝突）、每個測試自己一份資源。等第 1 階段知道搶的是什麼再定。
- **Browser 結束時等不等 renderer**：直接結束它們（Chromium 的 fast shutdown），還是先請它們收尾、有時限。牽涉到 trace 寫不寫得完、`beforeunload` 之後要不要做。第 3 階段。
- **全域變數怎麼收**：明確的結束順序，還是結束時不解構（直接 `_exit`）。第 3 階段。

## 之後的可能

- 作業系統的 sandbox 的那一份 RFC。
- 偵測「不回應」的 renderer。
- `beforeunload`、`unload`、`pagehide` 在分頁關掉和瀏覽器結束時的行為。
