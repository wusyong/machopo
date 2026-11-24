# RFC 0109：結束的流程（graceful shutdown）

- 開始日期：2026-11-22
- 狀態：🚧 已排程：2026-11-22（第 0、1 階段的盤點做完；2026-11-24 重新評估、改寫成以結束為主，待審查）
- 細節：`details.md`；進度：`progress.md`

## 摘要

machopo 的每一種跑法（視窗、automation pipe、headless、renderer process、machopodriver 啟動的）怎麼結束，從來沒有從頭到尾盤過，出事時都是一次修一個。這份照盤點（第 0 階段）定一套結束的規則，再照規則改，做完時有基本的 graceful shutdown：要它結束就在固定的時間內結束、不留 process、該寫的寫完。

這份原本也要讓 `shell` 的測試重新可信：2026-11-22 時 CI 的紅燈幾乎都是它。審查（第 1 階段）的主犯是產品的競爭（`details.md` 的 A 類），之後由 RFC 0110 修掉，兩個算畫了幾張的測試在 RFC 0112 照本意改寫。CI 從那之後連續綠燈，所以這份改成以結束為主；`shell` 的測試剩下的（在忙的機器上會壞的六個測試和搶 port 的一個 helper）在第 2 段先收掉，結束流程的測試才站得穩。

## 動機

**結束的流程**（第 0 階段的盤點，`647ab5a`，Linux；`details.md` 的「結束的流程的盤點」）。結束的六條目標（下面）在 Linux 上只有第 5 條成立：

- **S1**：`--single-process` 時頁面卡住就永遠不結束：`join` 沒有時限，main thread 的 script 也打斷不了（worker 的有 `TerminateExecution`）。headless 一樣。
- **S2、S3**：卡住的 renderer 一個一個等，各 5 秒；machopodriver 也只等 Browser 5 秒，所以一個卡住的分頁就一定被 kill，什麼都來不及寫。
- **S4**：沒有處理 SIGTERM、SIGINT、SIGHUP：照預設直接死，profile（cookie、`localStorage`）和 trace 都沒寫完。
- **S5**：client 不讀 stdout 時，automation pipe 的 writer 卡在 `write`，main 永遠 `join` 它。
- **S6**：啟動到一半失敗時（`--remote-debugging-port` 被占用）約一半的機率 crash：沒有呼叫 `BrowserWindow::shutdown`，Viz 還在建 device 時 `exit` 跑了 C++ 的解構子。
- **S7**：`exit` 時還有 thread 在跑（Browser 的 IO thread、GPU 驅動程式的、V8 的 worker），正常結束時它們閒著，但沒有東西保證。
- **S8**：Viz 有三處遇到 wgpu 的錯誤就 `process::exit(1)`，其他 thread 照跑、profile 不寫。
- **S9**：renderer process 繼承 Browser 的 stdin 和 stdout，也就是 automation 的 pipe：它活著時讀的人等不到 EOF，它寫到 stdout 的東西會混進訊息。

2026-11-24 對過程式，S1 到 S9 都還在（之後 RFC 0110、0111、0112 改了 renderer 的 event loop 和量測，沒有碰結束）。量到的時間要在第 3 段重量。另外 2026-11-10 有兩次「machopo 沒有結束」原因不明（`details.md`）。

**`shell` 的測試，剩下的**（`details.md` 的「2026-11-24 的重新評估」）：

- A1 到 A6（產品的競爭，CI 的主犯：A1 一個就壞了 15 次）由 RFC 0110 修掉；B1、B2（算畫了幾張）由 RFC 0112 照本意改寫。0110 merge 之後 CI 跑了 9 次，只壞過一次，就是 B1，之後連續 7 次綠燈。
- B3 到 B8 六個測試在忙的機器上會壞：盤點時照 CI 的條件（4 顆 CPU、加負載）整組跑 453 次，B5 壞 64 次、B4 24 次、B3 17 次、B6 3 次、B8 2 次、B7 1 次。重新審過，六個都是測試假設了機器不忙或時間不會剛好相等，產品照規範是對的；B7、B8 對過 Chrome for Testing。
- C1、C2：`net::test_server::unused_port` 放掉 port 才回傳，同時跑的另一個測試的 server 拿到同一個（453 次裡各 1、2 次）。

**目標**：

做完時要有基本的 graceful shutdown。「基本」定成下面六條，每一種跑法（視窗、automation pipe、headless、machopodriver 啟動的）都要滿足；它們就是這份的驗收條件：

1. **一定結束**：要它結束（關視窗、關 pipe、刪 session、收到 SIGTERM）之後，在固定的時間內結束，不管頁面在做什麼、開了幾個分頁。時限暫定 2 秒（「待決定」）。
2. **不留東西**：結束之後沒有殘留的 process，沒有人還握著它的 stdout、stderr。
3. **順序固定**：每條 thread 有人擁有、有人等它結束，照寫下來的順序停；整個 process 一份的東西（GPU device、V8 的 platform）在還有 thread 用它的時候不會被收掉。結束時不 crash、不卡住。
4. **該寫完的寫完**：正常結束時，profile（cookie、`localStorage`）和 trace 都寫完。
5. **被 kill 也乾淨**：Browser 被強制結束時 renderer 跟著結束（多 process 的那份做完時已經有，這份把它列進表裡、和其他幾條一起守著）。
6. **每一條有測試守著**，而且這些測試照 CI 的跑法重複跑不會壞。

`shell` 的測試：B3 到 B8 改寫成守它們真正要守的事，C1、C2 的 port 整個測試期間占著；照 CI 的條件重複跑不會壞（次數見「待決定」）。

**不在範圍內**：

- 頁面的 `beforeunload`、`unload`、`pagehide` 在關分頁和結束時的行為（要不要跑、給多久）。
- 結束時存 session、下次還原分頁。
- 偵測「不回應」的 renderer 並主動處理（hang monitor）。這份只管「要它結束時它一定結束」。
- 當掉時的收尾（crash reporter 這一類）。
- GPU 驅動程式自己的問題：這份能做的是結束時不讓它還在忙，不能保證驅動程式不出別的事。
- 作業系統的 sandbox：另一份。它會多一種結束法（被 sandbox 擋下而死的 renderer），排在這份之後。
- `shell` 的測試在 B3 到 B8、C1、C2 以外的整理（搬到別的 crate、重寫）；WPT 的 known intermittent 清單（另一套機制）。

**事先保證不了的**：

- 2026-11-10 那兩次「machopo 沒有結束」原因不明。這份能給的是「結束的順序寫下來了、有測試，那個現象沒有再出現」，不是「確定修掉了那一個」。
- 改規則時可能發現要動比較大的地方（例如 thread 的擁有關係要重排）。那時列選項決定，範圍可能比現在想的大。

要解決的 `docs/limitations.md` 和 `docs/test-gaps.md` 裡的項目（2026-11-23 盤點時列的）：

- `limitations.md` 的「平台和建置」：盤點時加的「結束」那一項，和 renderer process 那一項裡寫到結束、共用輸出的部分。
- `limitations.md` 的「WebDriver 和自動化」：盤點時加的「忙的時候」那一項。
- `limitations.md` 的「隔離」的「renderer 還有兩條路能讓 Browser 結束」：只有 Viz 遇到錯誤時怎麼結束那一半。
- `test-gaps.md`：通道因為壞訊息、佇列滿了而結束時沒有測試；macOS 上 renderer 隨 Browser 結束只在 Linux 上跑過；`--remote-debugging-port` 的 server 停下時等 websocket 沒有測試。

## 設計概要

給用 machopo 的人：關掉視窗、結束 `machopo --automation-pipe`、machopodriver 刪掉 session、送 SIGTERM，都在固定的時間內結束，不留下 process，不管頁面在做什麼；正常結束時 profile 和 trace 都寫完。

給改 machopo 的人：

- 一張表寫著每條 thread 和每個 process 怎麼結束（放在 `docs/architecture.md`）。新加一條 thread 時要填：誰擁有、誰 join、結束時它卡在哪裡會怎樣。
- 結束的規則（第 3 段定）：Browser 等不等 renderer、等多久、全域變數怎麼收、遇到錯誤時怎麼結束。
- `shell` 的測試不靠機器的快慢：等條件成立，不數樣本、不假設時間不會剛好相等。

## 詳細設計

規則和改法在第 3 段開始時照重量的結果定。盤點的產出（每一種跑法的結束順序和時間、每條 thread、每個有狀態的全域變數、每個 child process、和 Chromium 的對照）在 `details.md`。

**`shell` 的測試的改寫**（第 2 段，`details.md` 的「B3 到 B8 的審核」）：

| | 測試 | 為什麼會壞 | 改成 |
| --- | --- | --- | --- |
| B3 | `documents::idle_callbacks` | callback 開始時期限已經過了，`timeRemaining()` 回 0（合規範） | 接受 0；另外再要 idle callback，直到有一次 > 0（有上限） |
| B4 | `automation_pipe::the_profiler_samples_the_pages_script` | 數樣本的個數：sampler 拿到多少 CPU | 比兩種間隔的 `timeDeltas` 的中位數 |
| B5 | `automation_pipe::the_profile_tells_waiting_from_rendering` | 同上 | 照 `timeDeltas` 算 `(idle)` 佔的時間 |
| B6 | `documents::about_blank_has_its_initiators_origin` | parser 讓出時，iframe 在設 `onload` 的 script 之前就載完 | iframe 不在標籤上寫 `src`，script 設好 handler 才設 |
| B7 | `web_animations::a_replaced_animation_is_removed` | 50 ms 剛好是 3 個 headless 的 frame：時間相等時照 composite order 排（合規範），Chrome 的 frame 照真的時間、碰不到 | `duration` 改成不是 frame 間隔的倍數 |
| B8 | `transitions::a_frames_events_go_by_when` | `run` 和 `start` 剛好同時，照 composite order 排（合規範） | 加一點 `transition-delay`，`start` 一定晚於 `run`；Chrome 照 transition 分組（`run b / start b / end b / run a …`），和規範不同，記進 `docs/limitations.md` |

C1、C2：`unused_port` 改成回傳一個 guard，整個測試期間 bind 著、不 listen（「待決定」2-2）。

### 測試

- 結束的流程：每一種跑法、每一種情況一個測試，量結束花的時間和留下的 process（多 process 的那幾個已經有：`shell` 的 `a_killed_browsers_*`）。
- `shell` 的測試本身：改過的每一個照 CI 的跑法重複跑（次數見「待決定」），還守得住原本要守的事（mutation test），寫進 `progress.md`。
- 產品的改動照平常的規定：新的測試、mutation test。

### 分階段

每個階段開始前只決定那個階段的事。

0. **盤點結束的流程**：不改程式。做完（2026-11-23）。
1. **審查 `shell` 的測試**：不改產品。做完（2026-11-23）；2026-11-24 重新評估。
2. **收掉 `shell` 測試的尾巴**：B3 到 B8 照上表改寫，C1、C2 的 port，S6（啟動到一半失敗時呼叫 `shutdown`，幾行，不牽涉第 3 段的選擇）。
3. **重量、定結束的規則**：照第 0 階段的量法在現在的程式上重量，列選項、定下來（時限、Browser 等不等 renderer、全域變數怎麼收、Viz 遇到錯誤怎麼結束、訊號、卡住的 writer）。
4. **照規則改**，一種跑法一步。
5. **驗收**：「目標」的六條逐條對到守它的測試；CI 連續綠燈；`docs/architecture.md` 的表；`docs/limitations.md` 和 `docs/test-gaps.md` 更新。

## 缺點

- 結束的規則定了之後要維護：每加一條 thread、一個 process 都要照著填。
- 有些結束的問題只在 CI 的機器上出現（4 顆 CPU、lavapipe、Ubuntu 的 core dump 收集）；本機在 4 顆 CPU、lavapipe、加負載下重現得了 A 類，結束的不一定。
- macOS 和 Windows 沒有量過；這份只在 Linux 上量，其他平台照程式對。

## 理由和其他做法

- **為什麼還是一份**：`shell` 的測試裡剩下的六個在忙的機器上會壞，不先收掉，結束流程的每一步都分不出是不是自己弄壞的；它們都只是改測試，範圍小，不值得另開一份。
- **為什麼重新評估之後改成以結束為主**（已決定的事項第 3 條）：原本並重是因為 CI 的紅燈分不出原因，現在那個理由不成立了；照原本的寫法，讀的人會以為 `shell` 的測試還是主要的問題。
- **不選繼續一次修一個**：2026-11-18 到 22 這五天修了五個結束或生命週期的問題，每一個都是事後從 CI 的 log 猜的；沒有修過的還在（S1 到 S9、2026-11-10 的那兩次）。
- **不選先重跑蓋過去**：`docs/testing.md` 已經規定不這樣做。
- **不選先做 sandbox**：它做起來會多出很多「卡住」「莫名結束」的失敗，要先有一套結束的規則和乾淨的 CI 才分得出來。

## 先例

對照了 152.0.7977.82 的原始碼，逐項的比較在 `details.md` 的「對照 Chromium」：

- **Chromium 的結束**：`BrowserMainLoop` 照固定的順序停各條 thread；renderer 沒有 `beforeunload`、`unload` 的處理時 Browser 直接結束它，不等它收尾（fast shutdown）；POSIX 上 child process 在通道出錯時從 IO thread 直接結束自己；renderer 正常結束時也不收尾、直接 `_exit`；等不到的 child 2 秒後在背景 kill；SIGINT、SIGHUP 走正常結束，SIGTERM 走 `SessionEnding`。Windows 上靠 job object（這次沒有對照）。
- **Chromium 的測試**：測試的 launcher 把測試分批放進各自的 process 跑，壞掉的自動重試、另外統計不穩定的；等條件用 `base::test::RunUntil`。machopo 的規定是不重試。

## 已決定的事項

1. **開始做**（2026-11-22，使用者要求）：先做不改程式的第 0、1 階段。兩個互不相依，所以一起做了。
2. **A 類的根因不在這份修**（2026-11-23，使用者決定）：A1 到 A6 的共同原因是 renderer 和 engine 在同一個 thread 上卻各留一份狀態、用便條（`FrameRequest`）和外圈的收尾互相通知，根本是擁有權的問題，另寫 RFC 0110 評估。0110 修掉了 A1 到 A6（每一個都有必壞的測試），RFC 0112 拿掉了外圈。
3. **重新評估之後改成以結束為主**（2026-11-24，使用者決定；A）：A 類修掉、CI 連續綠燈之後，`shell` 的測試剩下 B3 到 B8 和 C1、C2，縮成第 2 段；結束的流程是這份的主體。B3 到 B8 審過再決定改測試還是修產品（使用者要求）：六個都是測試的問題，產品照規範是對的（`details.md`）。不選拆成兩份：`shell` 的那份剩很少，兩件事互相擋路的理由仍在。不選照原本並重的寫法：動機已經過時。

## 待決定

### 第 2 段開始前

**2-1. B3 到 B8 照「詳細設計」的表改寫**（建議）：六個都只改測試。B8 和 Chrome 不同的順序（Chrome 照 transition 分組）記進 `docs/limitations.md`，不改產品去照 Chrome：規範要先照時間排，Chrome 的做法和它不同。另一個做法是 B8 照 Chrome 改產品：和 Chrome 一樣，但偏離規範，別的瀏覽器的順序沒有對過。

**2-2. 「拒絕連線」的 port**（C1、C2）：

| 選項 | 做什麼 | 優點 | 缺點、成本 | 和 Chromium |
| --- | --- | --- | --- | --- |
| A（建議） | 回傳一個 guard：整個測試期間 bind 著、不 listen | 沒有別人拿得到；不拖慢測試 | 連它是不是馬上被拒絕，三個平台都還沒有驗證 | `EmbeddedTestServer` 分 `InitializeAndListen` 和開始接受兩步，port 先占著再用；要失敗的載入也可以由 `URLLoaderInterceptor` 直接給錯誤 |
| B | 用固定、沒有人 listen 的 port（例如 1） | 最簡單 | 靠機器上剛好沒人用；Fetch 的 bad port 會先擋掉一些 | |
| C | 用到的測試和所有建 server 的測試排隊 | 不動 helper | 幾乎整組都要排隊，慢很多 | test launcher 分批、各批一個 process |

**2-3. 怎麼算穩定**：建議改過的每一個在 4 顆 CPU、lavapipe、加負載下重複 1000 次 0 失敗，整組照同樣的條件 300 次 0 失敗（五份同時約 45 分鐘），再加 CI 連續 20 次綠燈。另一個選項是整組也跑 1000 次（約 2.5 小時一輪）。

**2-4. S6 放在第 2 段**（建議）：改動是幾行（失敗的路也呼叫 `shutdown`），不牽涉第 3 段的任何選擇；不放的話要等到第 4 段。

### 第 3 段開始時

- **結束的時限**（目標的第 1 條）：暫定 2 秒，照重量的結果定。
- **Browser 結束時等不等 renderer**：照 Chromium 的 fast shutdown 直接結束、在背景等，還是先請它們收尾、有時限。牽涉到 trace 寫不寫得完。
- **同一個 process 的 renderer 卡住時怎麼結束**（S1）：main thread 的 script 怎麼打斷。
- **全域變數怎麼收**（S7）：明確的結束順序，還是結束時不解構。
- **訊號**（S4）、**卡住的 writer**（S5）、**Viz 遇到錯誤時**（S8）、**renderer 的 stdio**（S9）。

## 之後的可能

- 作業系統的 sandbox 的那一份 RFC。
- 偵測「不回應」的 renderer。
- `beforeunload`、`unload`、`pagehide` 在分頁關掉和瀏覽器結束時的行為。
