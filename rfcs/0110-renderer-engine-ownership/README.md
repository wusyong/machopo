# RFC 0110：renderer 和 engine 的狀態只有一個擁有者

- 開始日期：2026-11-23
- 狀態：📝 草稿
- 細節：`details.md`

## 摘要

renderer 和 script engine 在同一個 thread 上，彼此卻像隔著 process：engine 改了 renderer 要知道的事，記成便條（`FrameRequest`），renderer 等 event loop 的外圈才補做；renderer 自己也留著副本（DevTools 的 frame 清單），等外圈才更新。RFC 0109 盤點到的不穩定測試 A1 到 A6 都出在這裡，網頁也看得到（設了 `location.hash` 之後，同一個 task 讀到的捲動位置是舊的）。根本是擁有權：engine 執行中借不到 `RenderThread`，所以「事後補做」成了通過借用檢查最省事的寫法。目標照 Blink 和 content 層：同一個 renderer 裡每份狀態只有一個擁有者，另一層要知道時同步呼叫；跨 process 的才用訊息。第一階段只評估、不改產品：盤點每一份副本和每一種延後的機制，用兩種擁有權的做法在同一條路上各做一個原型，結果交給使用者決定做法和範圍。

## 動機

**現況**（2026-11-23 照程式看的）：

- renderer 把 `Box<dyn FrameLayout>`（renderer 的 `Frame`）和 `Box<dyn LoadSink>` 交給 engine 擁有；呼叫 engine 時，engine 從 `RenderThread` 借出 `&mut`，這段期間借不到 `RenderThread` 的其他欄位。engine 要碰 renderer 的東西時只剩兩條路：
  - **便條**：`FrameRequest`（`crates/dom_api/src/frame.rs`，九種）照它的註解是「after the call that made it so」才交給 renderer；renderer 在 `crates/renderer/src/render_thread.rs` 和 `render_thread/documents.rs` 共 19 處、以及 `RenderThread::run` 的外圈取出來處理。外圈另外補做 `tell_frames`、`tell_dom_changes`、`run_intersection_task` 等。
  - **共享**：`RendererLoadSink` 的 `RequestIds`、`DocumentLoaders`、`BlobUrls` 是 `Rc<RefCell<…>>`（`crates/renderer/src/loader.rs`）；DevTools 的 agent 是 `Rc`，因為 engine 的 inspector 在 `RenderThread` 借不到的時候也要用它們（`render_thread/devtools.rs` 的 `DevToolsQueueHandle`）。
- 結果是同一件事在兩邊各有一份，什麼時候對得上取決於外圈什麼時候跑到。
- `dom_wasm` 沒有 load sink（`set_load_sink` 是空的），導覽只留便條。

**問題**：

- **不穩定的測試**：RFC 0109 的 A1 到 A6（`0109` branch 上的 `details.md`，`cb5930d`）。DevTools 收不到新 iframe 的 `CSS.styleSheetAdded`、`fetch()` 的 redirect 漏報和時間倒過來、`location.hash` 的捲動晚了兩個 frame、拿掉的 iframe 在 WebDriver 回錯的錯誤、intersection 的通知排錯順序。CI 上 2026-11-19 到 22 壞的多半是這些。
- **網頁看得到**：同一份文件內的導覽之後，同一個 task 裡讀 `scrollY` 是舊的（Chrome 是新的；0109 的探針，Chrome 156 和 machopo 各量過）。
- **順序靠收尾點維持**：19 處呼叫 `handle_frame_requests`，有的只是為了排在某件事之前（`SetHistoryOffsetAndLength` 的註解寫著不先處理會掉）。少一處就是下一個同類的 bug。
- **該檢查的事落在兩邊之間**：別的 origin 的 `javascript:` 網址會在子 frame 裡執行，engine 只轉交、renderer 只執行，兩邊都沒比對 origin（2026-11-23 找到，`97262a6` 修了）。
- **便條認的是 frame，不是文件**：等外圈的期間 frame 換了文件，舊文件的要求可能套到新文件上。2026-11-23 試了子 frame 和頂層兩條路沒有重現，擋住它的是剛好存在的收尾點，不是規則；其他 commit 的路沒有試。

要解決的 `docs/limitations.md` 裡的限制：

- 「導覽到同一份文件的 `#fragment` 之後，捲動等到 event loop 的最上層才排定、下一個 frame 才捲」。
- 0109 的 branch 加的「renderer 忙的時候，有些事晚一輪才做」（還沒 merge 到 `main`）。

**目標**：

1. 盤點同一個 renderer 裡每一份重複的狀態和每一種延後的機制，對照 Blink 和 content 層，分成「跨 process，要留」「同一個 thread 的副本，拿掉」「該同步卻延後了」三類。
2. 定規則：同一個 renderer 裡每份狀態只有一個擁有者；Blink 當下做的當下做；Blink 排成 task 的排成真的 task；DevTools 在改變的當下被告知；跨 process 的用訊息。
3. 選一種擁有權的做法，照規則改，不加新的 `Rc`、`RefCell`，現有的能拿掉就拿掉。

**不在範圍內**：

- 外圈的其他工作（timer、parser 的續跑、child 的 commit）搬進 scheduler：event loop 的改動，見「之後的可能」。
- 跨 process 的狀態（Browser 的 session history 和 renderer 記的 index、length）：Chromium 也是兩份、靠訊息。
- 導覽交給 Browser 的時機（RFC 0019 第 24、35 條：`location` 和 `history` 的導覽在 event loop 的最上層送）：盤點沒有找到它造成的問題就不重新打開（「待決定」第 3 條）。
- 0109 的其他不穩定原因（B 類、port、啟動失敗時的 crash）和結束的流程。

## 設計概要

給之後改這部分程式的人：engine 裡的 DOM API 要讓 renderer 知道一件事時，直接呼叫 host 的 hook，呼叫返回時 renderer 的狀態已經更新，沒有「等一下才會發生」的便條；renderer 和 DevTools 要讀 frame 的樹、一份文件的 sheet 時，讀唯一的那一份。例如：

- `location.hash = 'x'` 返回時已經捲過去，同一個 task 讀 `scrollY` 是新的。
- 插入 iframe 返回時 frame 已經建好，DevTools 已經收到 `frameAttached`；拿掉 iframe 返回時 frame 已經不在，下一個 WebDriver 指令是 `no such window`。
- `fetch()` 返回時 Network agent 已經記下請求和送出的時間。

被同步呼叫的 renderer 不能回頭執行 script：會引起 script 的後續（例如 iframe 移除後的 `load`）排成 task，照 Blink 的時機。

## 詳細設計

草稿：第 0 階段的評估之後才寫得定。兩種擁有權的做法：

**做法一：engine 擁有 host 的物件**（現在的 `Frame` 就是這樣）。frame 的樹搬進兩個 engine 共用的 `dom`，renderer 照讀 DOM 的方式讀它（`engine.dom_mut().read(…)`）；renderer 自己的表放進 engine 擁有的 host 物件，renderer 要用時經過 engine 取回（`FrameLayout` 是 `Any`，renderer 已經這樣 downcast 它的 `Frame`）。不需要 `unsafe`。要回答的：renderer 同時要用 engine 和自己的表時，需要一個同時借出兩者的介面；哪些表放進哪個物件會牽動分層（例如 DevTools 的 agent）。

**做法二：每次呼叫時把 renderer 的狀態借給 engine**。`RenderThread` 拆成 engine 和其他欄位（host），呼叫寫成 `engine.run_…(&mut host, …)`；V8 的 callback 經過 isolate 拿到 host，借用的期限綁在 scope 上，和 `bindings` 現在借 engine 的狀態一樣（`crates/bindings/src/isolate_state.rs`：狀態在 isolate 的 data slot，只經過 isolate 借出，借著的時候不能呼叫 V8）。host 裡沒有 engine，所以被呼叫時不可能回頭執行 script：Blink 在執行期用 `ScriptForbiddenScope` 守的事，在這裡是編譯錯誤。要回答的：在 `bindings` 加有期限的借用要 `unsafe`，論證寫進 `crates/bindings/soundness.md`；`ScriptEngine` 的每個入口都要改，兩個 engine 和它們的測試都是；`RenderThread` 要拆欄位。小的先例是 `dom_api` 的 `LentLayout`（host 為 DevTools 的一次操作借出 layout）。

也可能混用：frame 的樹用做法一，renderer 自己的狀態用做法二。

**不管哪一種都要處理的**（照程式看的）：

- **跑過 script 之後 frame 可能已經不在**：同步移除之後，每個會執行 script 的地方回來時都要容忍 frame 不見了；renderer 的 `self.children` 有 59 處要逐一看。Blink 為此到處檢查 detached，漏掉的歷史上常是記憶體錯誤；Rust 裡會是 `None` 或編譯錯誤。
- **layout 已經借出時同步的 hook 會默默不做**：`LayoutCell` 的借用是 move，借出時拿到 `None`（`crates/dom_api/src/layout.rs`）。每個新的 hook 要證明或 assert 那時借得到，否則「晚一點做」變成「永遠沒做」。
- **兩個 engine 都要做**：`dom_wasm` 現在沒有 load sink、導覽只留便條。
- **DevTools 開著時的成本**：frame 一個個同步告知時不能每次重建整份清單（現在的清單是 frame 數的平方），要改成增量。一次插入很多 iframe 時不再批次處理。
- **過渡期的順序**：一部分同步、一部分還在佇列時會交錯。例如 `fetch()` 的登記要查 frame 的表，frame 的建立和移除要先同步。

### 測試

- 0109 寫的放大版測試（A1 到 A5 的必壞測試）照規則變成必過；0109 的五個探針和 Chrome 一樣。A6 的原因是推論，先寫出必壞的測試。
- 每個同步的 hook 有測試，跑 mutation test。
- 計數的測試：一次插入很多 iframe 時，DevTools 收到的通知和做的工作是線性的。
- 相關的 WPT 目錄：導覽、iframe、history、fetch。
- 動到 `bindings` 的 `unsafe` 時，`matrix` 和 `fuzz` 由 CI 跑（`gc.yml` 手動開始一次）。

### 分階段

0. **評估，不改產品**：
   - 盤點：每一份副本、每一種便條和外圈的收尾、每一個為了共享而用的 `Rc`、`RefCell`，各自對照 Blink 和 content 層，分成目標第 1 條的三類，寫成 `details.md` 的表。
   - 原型：兩種做法在同一條路上各做一個，建議 `fetch()` 的登記（A2、A3：牽涉 Network agent、`RequestIds` 的 `Rc<RefCell>` 和 frame 的表）。記下借用切不切得開、要不要 `unsafe`、改了多少、兩個 engine 各要做什麼。
   - 交給使用者決定做法和範圍。
1. 之後的階段照第 0 階段的結果定。暫定的順序：frame 的樹 → `fetch()` 的登記 → DevTools 的副本 → 同一份文件內的捲動 → intersection 的通知排成 task。每一步各自測試、mutation test、審核。

## 缺點

- 大重構：`dom_api`、`dom_js`、`dom_wasm`、renderer，做法二還有 `bindings`。
- 回歸的風險集中在 frame 的生命週期：frame 當場不見之後的每一條路；導覽和 iframe 的 WPT 最可能變動，整套只有 CI 跑。
- 做法二多一處 `unsafe` 和它的論證。
- 每次操作變重，不再批次；DevTools 開著時要增量的通知。
- RFC 0109 的 2-1 要等這份。

## 理由和其他做法

在 RFC 0109 的 2-1 比較過（`0109` branch 上的 `README.md`，`cb5930d`）：

- **每個 task 之後收尾**：放大的測試修得掉，但同一個 task、同一次 rendering、連著的 timer 照樣看到舊狀態，五個探針只有一個和 Chromium 一樣。
- **讀之前先補**：漏一處就是下一個同類的 bug。
- **逐個改成同步、不動擁有權**：方向和這份一樣，但搬不動的狀態只能再加 `Rc<RefCell>`，正是這份要避免的；成本也主要在這裡，所以拆成這份 RFC 先評估擁有權。
- **不做**：A 類繼續不穩定（2026-11-23 起維持現狀，不做過渡），同類的 bug 繼續長出來。

## 先例

- **Chromium**：renderer 裡也有兩層，Blink（`LocalFrame`、DOM、inspector 的 agent）和 content 層（`RenderFrameImpl`），在同一個 thread 上。Blink 要讓 content 層知道一件事時當場呼叫 `WebLocalFrameClient`，不留便條；DevTools 的 agent 由 `probe::` 在改變的當下呼叫，CSS agent 列 sheet 時直接讀 document（`inspector_css_agent.cc`），請求送出的當下就告訴 Network agent（`resource_load_observer_for_frame.cc`）；同一份文件內的導覽同步 commit（`FrameLoader::StartNavigation`）；跨 process 的用 mojo 的訊息。以上對過 152.0.7977.82 的原始碼。Blink 靠 raw pointer 和 Oilpan 互相指，同步呼叫沒有借用的問題，代價是到處檢查 frame 還在不在，以及執行期的 `ScriptForbiddenScope`。
- 其他瀏覽器沒有對照。

## 已決定的事項

無。

## 待決定

1. **做法一、做法二或混用**：第 0 階段的原型回答。
2. **範圍**：哪些副本和便條拿掉、哪些留著（只拿來通知、沒有人觀察得到延後的可以留）：盤點回答。
3. **導覽交給 Browser 的時機要不要一起改**：預設不改（「不在範圍內」）；盤點找到它造成的問題時再提。
4. **第 0 階段的原型用哪一條路**：建議 `fetch()` 的登記。
5. **和 RFC 0109 的先後**：0109 的第 3、4 階段會動 `RenderThread` 的結束路徑，做法二要拆 `RenderThread` 的欄位，兩者可能衝突；第 0 階段結束時一起排。

## 之後的可能

- 把外圈的其他工作（timer、parser 的續跑、child 的 commit）搬進 scheduler，讓它們都是真的 task：通道上不斷有 task 時，外圈的工作現在都會晚。
- 導覽在呼叫的當下交給 Browser（照 Chromium）。
