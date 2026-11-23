# RFC 0109：結束的流程，和 `shell` 的測試

- 開始日期：2026-11-22
- 狀態：🚧 已排程：2026-11-22（第 0、1 階段的盤點做完、待審查）
- 細節：`details.md`；進度：`progress.md`

## 摘要

兩件事，一起做是因為它們互相擋路：

- **結束的流程**：machopo 的每一種跑法（視窗、automation pipe、headless、renderer process、machopodriver 啟動的）怎麼結束，從來沒有從頭到尾盤過；出事時都是一次修一個。這份先把每條 thread、每個 process、每個有狀態的全域變數怎麼結束盤一遍，定成一套規則，再照規則改。
- **`shell` 的測試**：CI 最近壞掉的幾乎都是 `shell` 的測試。這份審查它們為什麼不穩定（等的方式和時間、好幾個測試搶同一個裝置或同一份全域狀態、產品自己的競爭），修掉或隔開，讓 CI 的紅燈重新代表「真的壞了」。

兩個不改程式的盤點（「分階段」的第 0、1 階段）做完了，做法還沒有選。盤點的結論：

- **CI 的主犯是產品的競爭，不是結束的問題**：renderer 把 Blink 當下做的事延後、讀延後更新的副本、或沒有把該排隊的排成 task（`details.md` 的 A 類，六個測試，其中一個是推論）。捲到 fragment 是網頁看得到的錯，機器不忙也錯。其餘壞過的是測試等錯了東西（八個）和搶 port（兩個）。
- **結束的六條目標在 Linux 上只有第 5 條成立**：`--single-process` 的頁面卡住時永遠不結束、卡住的分頁各等 5 秒、SIGTERM 時 profile 不寫、client 不讀 stdout 時不結束、啟動到一半失敗時約一半的機率 crash。

## 動機

**CI 的紅燈現在不代表什麼**（2026-11-18 到 22 的 60 次 CI：24 次成功、23 次失敗、12 次取消；每一次壞的是什麼在 `details.md` 的「CI 上壞過的」）：

- 23 次失敗裡 22 次是測試（1 次只有 lint），其中 18 次壞在 `shell`，3 次在 `ipc`，1 次在 `browser`。
- 15 次是同一個測試，`devtools_css::sheets_coming_and_going_are_told`，每次都死在同一行：頁面加了一個 `srcdoc` 的 iframe 之後，等 30 秒收不到它的 `CSS.styleSheetAdded`，machopo 還活著。它在 `main` 和 branch 上都壞，最早的一次是 2026-11-19。查到了，是產品自己的競爭（iframe 的文件的 sheet 什麼時候告訴 DevTools），不是結束的問題（`details.md` 的 A1）。
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
- 出過的事：兩個 process 共用 stderr 時一行被插隊（已修）；`documents::history_through_the_browser` 兩個事件的順序相反（一次）；`automation_pipe::the_system_info_tells_the_gpu_adapter`（一次）。兩個都查到了（`details.md` 的 A4、D1）。

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

要解決的 `docs/limitations.md` 和 `docs/test-gaps.md` 裡的項目（2026-11-23 盤點時列的）：

- `limitations.md` 的「平台和建置」：盤點時加的「結束」那一項（第 4 階段），和 renderer process 那一項裡寫到結束、共用輸出的部分。
- `limitations.md` 的「WebDriver 和自動化」：盤點時加的「忙的時候」那一項（第 2 階段）。
- `limitations.md` 的「隔離」的「renderer 還有兩條路能讓 Browser 結束」：只有 Viz 遇到錯誤時怎麼結束那一半（第 3 階段）。
- `test-gaps.md`：通道因為壞訊息、佇列滿了而結束時沒有測試；macOS 上 renderer 隨 Browser 結束只在 Linux 上跑過；`--remote-debugging-port` 的 server 停下時等 websocket 沒有測試。

## 設計概要

給用 machopo 的人：關掉視窗、結束 `machopo --automation-pipe`、machopodriver 刪掉 session，都在固定的時間內結束，不留下 process，不管頁面在做什麼。

給改 machopo 的人：

- 一張表寫著每條 thread 和每個 process 怎麼結束（放在 `docs/architecture.md`）。新加一條 thread 時要填：誰擁有、誰 join、結束時它卡在哪裡會怎樣。
- `shell` 的測試有一套寫法：怎麼等（等什麼事件、時限怎麼定）、哪些資源是整個 process 一份、哪些測試不能同時跑。
- CI 紅了就是壞了。

## 詳細設計

這一節寫兩個盤點產出什麼，結果在 `details.md`。規則和改法在第 2、3 階段開始時定。

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
1. **審查 `shell` 的測試**：不改產品。產出分類表、壞過的每個測試的原因。第 0、1 階段互不相依。
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

對照了 152.0.7977.82 的原始碼，逐項的比較在 `details.md` 的「對照 Chromium」：

- **Chromium 的結束**：`BrowserMainLoop` 照固定的順序停各條 thread；renderer 沒有 `beforeunload`、`unload` 的處理時 Browser 直接結束它，不等它收尾（fast shutdown）；POSIX 上 child process 在通道出錯時從 IO thread 直接結束自己；renderer 正常結束時也不收尾、直接 `_exit`；等不到的 child 2 秒後在背景 kill。Windows 上靠 job object（這次沒有對照）。
- **Chromium 的測試**：測試的 launcher 把測試分批放進各自的 process 跑，壞掉的自動重試、另外統計不穩定的。machopo 的規定是不重試。

## 已決定的事項

1. **開始做**（2026-11-22，使用者要求）：先做不改程式的第 0、1 階段。兩個互不相依，所以一起做了，原本「待決定」的先後和 `devtools_css` 那一個放在哪一步不再需要決定。
2. **A 類的根因不在這份修**（2026-11-23，使用者決定）：A1 到 A6 的共同原因是 renderer 和 engine 在同一個 thread 上卻各留一份狀態、用便條（`FrameRequest`）和外圈的收尾互相通知，根本是擁有權的問題（engine 執行中借不到 `RenderThread`），不是 `FrameRequest` 本身。要不要、能不能像 Blink 和 content 層那樣每份狀態只有一個擁有者、同步呼叫，另寫 RFC 0110 評估；2-1 等它。這段期間 A 類維持現狀，不做選項 A 的過渡（它會被 0110 取代；放大時修得掉，但同一個 task 裡照樣是舊的）。2-2 到 2-5 和第 3、4 階段不受影響。

## 待決定

### 第 2 階段開始前（2026-11-23 的看法，等確認）

**2-1. A 類的根因怎麼修**：等 RFC 0110（已決定的事項第 2 條）。2026-11-23 比較過的結論留給 0110：

- **A**（每個 task 之後呼叫 `handle_frame_requests` 和 `tell_frames`）：放大的測試修得掉，但不是根治：同一個 task、同一次 rendering、連著的 timer、idle callback 之後的 frame 照樣看到舊狀態（五個探針只有一個和 Chromium 一樣，和不改相同）。
- **B**（每個讀的人讀之前先補）：漏一處就是下一個同類的 bug。
- **C**（Blink 當下做的當下做、排 task 的排成真的 task、DevTools 在改變的當下被告知）：方向對，捲動的原型讓五個探針都和 Chromium 一樣；成本主要在擁有權，不在效能：frame 的表和 `fetch()` 的登記要搬到 engine 執行中借得到的地方，否則要更多 `Rc<RefCell>`。照程式看到的其他成本：跑過 script 之後 frame 可能已經不在（renderer 的 `self.children` 59 處要逐一看）；layout 已經借出時同步的 hook 會默默不做；`dom_wasm` 要各做一次；`fetch()` 的登記要查 frame 的表，所以 frame 的建立和移除要先同步；導覽在呼叫的當下送出要推翻 RFC 0019 第 35 條，好處只在時間上，CI 壞過的測試都不需要它。

**2-2. 「拒絕連線」的 port**（C1、C2）：

| 選項 | 做什麼 | 優點 | 缺點、成本 | 和 Chromium |
| --- | --- | --- | --- | --- |
| A（建議） | 回傳一個 guard：整個測試期間 bind 著、不 listen | 沒有別人拿得到；不拖慢測試 | 連它是不是馬上被拒絕，三個平台都還沒有驗證 | `EmbeddedTestServer` 分 `InitializeAndListen` 和開始接受兩步，port 先占著再用；要失敗的載入也可以由 `URLLoaderInterceptor` 直接給錯誤 |
| B | 用固定、沒有人 listen 的 port（例如 1） | 最簡單 | 靠機器上剛好沒人用；Fetch 的 bad port 會先擋掉一些 | |
| C | 用到的測試和所有建 server 的測試排隊 | 不動 helper | 幾乎整組都要排隊，慢很多 | test launcher 分批、各批一個 process |

**2-3. B 類的測試怎麼改**：

| 選項 | 做什麼 | 優點 | 缺點、成本 | 和 Chromium |
| --- | --- | --- | --- | --- |
| A（建議） | 一個一個改成斷言真正要守的事（怎麼改在 `details.md` 那張表的最後一欄）；B7、B8 先和 Chrome 比過再決定改哪一邊 | 測試不再量機器的速度 | 八個各自要想，各要 mutation test 確認還守得住 | 一樣：`base::test::RunUntil` 等條件成立，註解寫明用別的等法會不穩定 |
| B | 放寬門檻和時限 | 快 | 只是比較少壞，守的也變弱 | |
| C | 對時間敏感的排隊、不和別的同時跑 | 斷言不用改 | 慢；CI 的機器本身忙時照樣壞 | 壞掉的測試另外重試；machopo 不重試 |

**2-4. 怎麼算穩定**：建議修過的每一個在 4 顆 CPU、lavapipe、加負載下重複 1000 次 0 失敗，整組照同樣的條件 300 次 0 失敗（五份同時約 45 分鐘），再加 CI 連續 20 次綠燈。另一個選項是整組也跑 1000 次（約 2.5 小時一輪）。

**2-5. 啟動失敗時的 crash（S6）要不要提前**：建議提前到第 2 階段的第一步。改動是幾行（失敗的路也呼叫 `shutdown`），不牽涉第 3 階段的任何選擇；不提前的話要等到第 4 階段。

### 第 3 階段開始時

- **結束的時限**（目標的第 1 條）：暫定 2 秒。量到的在 `details.md`。
- **Browser 結束時等不等 renderer**：牽涉到 trace 寫不寫得完、`beforeunload` 之後要不要做。
- **全域變數怎麼收**：明確的結束順序，還是結束時不解構。
- 盤點多出來的：同一個 process 的 renderer 卡住時怎麼結束（S1）、訊號（S4）、卡住的 writer（S5）。

## 之後的可能

- 作業系統的 sandbox 的那一份 RFC。
- 偵測「不回應」的 renderer。
- `beforeunload`、`unload`、`pagehide` 在分頁關掉和瀏覽器結束時的行為。
