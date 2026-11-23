# RFC 0110：renderer 和 engine 的狀態只有一個擁有者

- 開始日期：2026-11-23
- 狀態：🚧 已排程：2026-11-23（第 0 階段的評估做完、待審查）
- 細節：`details.md`；進度：`progress.md`

## 摘要

renderer 和 script engine 在同一個 thread 上，彼此卻像隔著 process：engine 改了 renderer 要知道的事，記成便條（`FrameRequest`），renderer 等 event loop 的外圈才補做；renderer 自己也留著副本（DevTools 的 frame 清單），等外圈才更新。RFC 0109 盤點到的不穩定測試 A1 到 A6 都出在這裡，網頁也看得到（設了 `location.hash` 之後，同一個 task 讀到的捲動位置是舊的）。延後的原因有三種（「動機」）：處理時要用 renderer 自己留的副本，而 engine 執行中借不到 `RenderThread`；處理的過程會執行 script 或用 layout，不能在 engine 的呼叫中回頭做；收尾點排在整批 task 之後。目標照 Blink 和 content 層：同一個 renderer 裡每份狀態只有一個擁有者，另一層要知道時同步呼叫；跨 process 的才用訊息。第一階段只評估、不改產品：盤點每一份副本和每一種延後的機制，用三種擁有權的做法在同一條路上各做一個原型，結果交給使用者決定做法和範圍。

## 動機

**現況**（2026-11-23 照程式看的）：

- renderer 把 `Box<dyn FrameLayout>`（renderer 的 `Frame`）和 `Box<dyn LoadSink>` 交給 engine 擁有；呼叫 engine 時，engine 從 `RenderThread` 借出 `&mut`，這段期間借不到 `RenderThread` 的其他欄位。engine 要碰 renderer 的東西時只剩兩條路：
  - **便條**：`FrameRequest`（`crates/dom_api/src/frame.rs`，九種）照它的註解是「after the call that made it so」才交給 renderer；renderer 在 `crates/renderer/src/render_thread.rs` 和 `render_thread/documents.rs` 共 19 處取出來處理（包括 `RenderThread::run` 的外圈）。外圈另外補做 `tell_frames`、`tell_dom_changes`、`run_intersection_task` 等。
  - **共享**：`RendererLoadSink` 的 `RequestIds`、`DocumentLoaders`、`BlobUrls` 是 `Rc<RefCell<…>>`（`crates/renderer/src/loader.rs`）；DevTools 的 agent 是 `Rc`，因為 engine 的 inspector 在 `RenderThread` 借不到的時候也要用它們（`render_thread/devtools.rs` 的 `DevToolsQueueHandle`）。
- 結果是同一件事在兩邊各有一份，什麼時候對得上取決於外圈什麼時候跑到。
- `dom_wasm` 沒有 `fetch()` 也沒有 load sink（`set_load_sink` 是空的），導覽只留便條。

engine 也有同步呼叫 renderer 的路（建 iframe 時的 `FrameLayout::child_frame_created`、`LoadSink` 的 `start_fetch`、`frame_gone`、`register_blob_url`），哪些延後是逐項選的。延後的有三種原因（2026-11-23 的審查，`details.md` 的「審查」）：

1. **處理時要用 renderer 留的副本**（A1、A3、A5）：`ChildFrames`、`FrameDocument.url`、DevTools 的 frame 清單；engine 執行中借不到 `RenderThread`。拿掉副本（讀 `dom`，或由 engine 當 hook 的參數帶過去）就不用借。
2. **處理的過程會執行 script 或用 layout**：`javascript:` 網址的執行、iframe 移除之後的 `finish_load`（layout、圖片、`load`）。不管擁有權怎麼改，都不能在 engine 的呼叫中回頭做。
3. **收尾點的位置**：外圈的收尾排在整批 task 之後；A2 只是 `did_receive_redirect` 沒有像 `did_receive_resource` 那樣先收尾。

A4（捲動）可以用現有的 `FrameLayout` 加一個 hook 當場做（0109 的原型就是），A6（intersection）、`javascript:` 網址綁在文件上都不需要改擁有權。

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
3. 選一種擁有權的做法，照規則改，不加新的 `Rc`、`RefCell`（例外是 hook 拿 DevTools agent 的 `Rc`，已決定的事項第 6 條），現有的能拿掉就拿掉。

**不在範圍內**：

- 外圈的其他工作（timer、parser 的續跑、child 的 commit）搬進 scheduler：event loop 的改動，見「之後的可能」。
- 跨 process 的狀態（Browser 的 session history 和 renderer 記的 index、length）：Chromium 也是兩份、靠訊息。
- 導覽交給 Browser 的時機（RFC 0019 第 24、35 條：`location` 和 `history` 的導覽在 event loop 的最上層送）：盤點沒有找到它造成的問題，不改（已決定的事項第 8 條）。
- 0109 的其他不穩定原因（B 類、port、啟動失敗時的 crash）和結束的流程。

## 設計概要

給之後改這部分程式的人：engine 裡的 DOM API 要讓 renderer 知道一件事時，直接呼叫 host 的 hook，呼叫返回時 renderer 的狀態已經更新，沒有「等一下才會發生」的便條；renderer 和 DevTools 要讀 frame 的樹、一份文件的 sheet 時，讀唯一的那一份。例如：

- `location.hash = 'x'` 返回時已經捲過去，同一個 task 讀 `scrollY` 是新的。
- 插入 iframe 返回時 frame 已經建好，DevTools 已經收到 `frameAttached`；拿掉 iframe 返回時 frame 已經不在，下一個 WebDriver 指令是 `no such window`。
- `fetch()` 返回時 Network agent 已經記下請求和送出的時間。

被同步呼叫的 renderer 不能回頭執行 script。Blink 在幾處是同步執行 script 的（iframe 移除時子 frame 的 unload、之後 parent 的 `CheckCompleted` 和 `load`；審查照 `local_frame.cc`、`frame_loader.cc`、`document.cc` 推的，沒有追完），machopo 的 hook 結構上做不到，這幾處的時機會和 Blink 不同，WPT 可能看得出來，要一處處決定。

## 詳細設計

草稿：第 0 階段的評估之後才寫得定。三種擁有權的做法：

**做法一：engine 擁有 host 的物件**（現在的 `Frame` 就是這樣）。frame 的樹搬進兩個 engine 共用的 `dom`，renderer 照讀 DOM 的方式讀它（`engine.dom_mut().read(…)`）；renderer 自己的表放進 engine 擁有的 host 物件，renderer 要用時經過 engine 取回（`FrameLayout` 是 `Any`，renderer 已經這樣 downcast 它的 `Frame`）。不需要 `unsafe`。要回答的：renderer 同時要用 engine 和自己的表時，需要一個同時借出兩者的介面；哪些表放進哪個物件會牽動分層（例如 DevTools 的 agent）。

**做法二：每次呼叫時把 renderer 的狀態借給 engine**。`RenderThread` 拆成 engine 和其他欄位（host），呼叫寫成 `engine.run_…(&mut host, …)`；V8 的 callback 經過 isolate 拿到 host，借用的期限綁在 scope 上，和 `bindings` 現在借 engine 的狀態一樣（`crates/bindings/src/isolate_state.rs`：狀態在 isolate 的 data slot，只經過 isolate 借出，借著的時候不能呼叫 V8）。host 裡沒有 engine，所以被呼叫時不可能回頭執行 script：Blink 在執行期用 `ScriptForbiddenScope` 守的事，在這裡是編譯錯誤。要回答的：在 `bindings` 加有期限的借用要 `unsafe`，論證寫進 `crates/bindings/soundness.md`；`ScriptEngine` 的每個入口都要改，兩個 engine 和它們的測試都是；`RenderThread` 要拆欄位。小的先例是 `dom_api` 的 `LentLayout`（host 為 DevTools 的一次操作借出 layout）。

**做法三：兩邊都要的放進 `dom`，engine 把 hook 要的當參數帶過去**（2026-11-23 的審查提的）：frame 的樹、文件的 loader、`fetch()` 送出的請求這類兩邊都要讀的資料由 `dom` 擁有（Chromium 裡它們都在 Blink core：`ResourceFetcher`、inspector 的 agent、frame 的樹）；engine 呼叫 hook 時把 hook 要的帶過去（`FetchOrigin` 的推廣）；只有 renderer 自己用的資料由 renderer 擁有，不借給誰。請求的編號照 Blink 的 `CreateUniqueIdentifier`，每個 renderer 自己的計數器，誰要就取，不共享。

**第 0 階段的原型**（`details.md` 的「原型」）：三種做法都在 `fetch()` 的登記上做了，都修掉 A2、A3、都不需要 `unsafe`。做法二不必在 `bindings` 加有期限的借用：把狀態**移進** engine 擁有的 sink（取 engine 時借出、取狀態時拿回），不是借參照給 V8 的 callback。但借用檢查器只保證 slot 不會同時交出 engine 和狀態，不保證每條會執行 script 的路都借出了：本質上和 `RefCell` 一樣是執行期的檢查。審查找到、之後重現的：換 engine 時 unload 裡的 `fetch()`，做法二在 sink 的 `expect` panic（狀態在 unload 之前就拿回來了），做法一的請求編號從 1 重來（讀程式確認，沒有重現）。做法三沒有借出、拿回，這條路照樣正常。

**不管哪一種都要處理的**（照程式看的）：

- **跑過 script 之後 frame 可能已經不在**：同步移除之後，每個會執行 script 的地方回來時都要容忍 frame 不見了；renderer 的 `self.children` 有 52 處要逐一看（`documents.rs` 50、`webdriver_commands.rs` 2；之前寫 59 多算了 DOM agent 自己的欄位）。Blink 為此到處檢查 detached，漏掉的歷史上常是記憶體錯誤；Rust 裡會是 `None` 或編譯錯誤。
- **layout 已經借出時同步的 hook 會默默不做**：`LayoutCell` 的借用是 move，借出時拿到 `None`（`crates/dom_api/src/layout.rs`）。每個新的 hook 要證明或 assert 那時借得到，否則「晚一點做」變成「永遠沒做」。
- **兩個 engine 都要做**：`dom_wasm` 的導覽只留便條（它沒有 `fetch()`）。
- **DevTools 開著時的成本**：frame 一個個同步告知時不能每次重建整份清單（現在的清單是 frame 數的平方），要改成增量。一次插入很多 iframe 時不再批次處理。
- **過渡期的順序**：一部分同步、一部分還在佇列時會交錯。文件的 loader 要先交給 `dom`，frame 才能同步建立（子 frame 初始的 `about:blank` 的 loader 現在是在 `Created` 的便條裡照 parent 設的）。
- **frame 的樹放進 `dom`，實際是**（2026-11-23 第二次審查，`details.md` 的「審查」）：`dom` 擁有樹和 id（`FrameId` 要從 `dom_api` 往下搬，token 的型別要定），dom_js 留一張 FrameId 到 V8 context 的表（像 Blink 的 `LocalFrame` 和 `WindowProxy`）；`FrameView` 的 child view 保留巢狀、用 `FrameLayout` 的 hook 同步更新（建立的已經有，加一個移除的），不改成讀 `dom`；`Created` 的便條現在是 hook 沒做到時的補救（parent 的 view 不在、layout 被借走），拿掉之前要改成 assert 或證明做得到。
- **移除當場完成，仍然有兩件延後**：parent 的 `finish_load`（要 renderer 的 readiness、sheet 和 layout），和 renderer 私有資料（`FrameDocument` 的 parser、body）的清理。外面看得到的（DevTools、Browser、WebDriver）改讀 `dom`；WebDriver 現在照 `self.children` 找 frame，不改讀 `dom` 的話 A5 還在。
- **DevTools 的順序**：Blink 先送 `childNodeInserted`，才建 frame、送 `frameAttached`，`frameAttached` 又在 `executionContextCreated` 之前。machopo 的 DOM 變動在外圈才送；iframe 建立時 script 的 stack 要等 `note_request_stacks`，排在 `update_elements` 之後（`dom_js/src/state.rs`）。同步的 `frameAttached` 要先送出 DOM 的變動，並把這兩步的順序對調（沒有驗證能不能調）。
- **hook 要拿 DevTools agent 的 `Rc`**：在改變的當下告訴 DevTools，hook 就要拿得到 agent（V3 的 sink 已經拿了 `Rc<NetworkAgent>`，frame 的要 Page 和 DOM agent），和目標第 3 條「不加新的 `Rc`」衝突，要明文決定。

### 測試

- 0109 寫的放大版測試（A1 到 A5 的必壞測試）照規則變成必過；0109 的五個探針和 Chrome 一樣。A6 的原因是推論，先寫出必壞的測試。
- 每個同步的 hook 有測試，跑 mutation test。
- 計數的測試：一次插入很多 iframe 時，DevTools 收到的通知和做的工作是線性的。
- 相關的 WPT 目錄：導覽、iframe、history、fetch。
- 動到 `bindings` 的 `unsafe` 時，`matrix` 和 `fuzz` 由 CI 跑（`gc.yml` 手動開始一次）。

### 分階段

0. **評估，不改產品**：
   - 盤點：每一份副本、每一種便條和外圈的收尾、每一個為了共享而用的 `Rc`、`RefCell`，各自對照 Blink 和 content 層，分成目標第 1 條的三類，寫成 `details.md` 的表。
   - 原型：三種做法在同一條路上各做一個：`fetch()` 的登記（已決定的事項第 2 條），patch 放在 `prototypes/`（第 3 條）。記下借用切不切得開、要不要 `unsafe`、改了多少、兩個 engine 各要做什麼。
   - 交給使用者決定做法和範圍。
1. 之後的階段照已決定的事項第 7 條的六步（文件的 loader 要在 frame 的樹之前，「不管哪一種都要處理的」）。每一步各自測試、mutation test、審核。

## 缺點

- 大重構：`dom`、`dom_api`、`dom_js`、`dom_wasm`、renderer。
- 回歸的風險集中在 frame 的生命週期：frame 當場不見之後的每一條路；導覽和 iframe 的 WPT 最可能變動，整套只有 CI 跑。
- 做法二是執行期的借出和拿回，用錯時是 panic（原型裡換 engine 時就碰到了）。做法三沒有這一類，但 `dom` 多了 host 讀的記錄（`fetch()` 送出的請求），renderer 要經過 engine 讀。
- 每次操作變重，不再批次；DevTools 開著時要增量的通知。
- RFC 0109 的 2-1 要等這份。

## 理由和其他做法

在 RFC 0109 的 2-1 比較過（`0109` branch 上的 `README.md`，`cb5930d`）：

- **每個 task 之後收尾**：放大的測試修得掉，但同一個 task、同一次 rendering、連著的 timer 照樣看到舊狀態，五個探針只有一個和 Chromium 一樣。
- **讀之前先補**：漏一處就是下一個同類的 bug。
- **逐個改成同步、不動擁有權**：方向和這份一樣，但搬不動的狀態只能再加 `Rc<RefCell>`，正是這份要避免的；成本也主要在這裡，所以拆成這份 RFC 先評估擁有權（原型 V0 就是這樣，多了一張共享的表）。
- **集中收尾**（審查提的）：每次呼叫 engine 之後收尾一次（例如包一層 `with_engine`），19 個收尾點就不再各自脆弱。修得掉收尾點排錯的那一類（A2），修不掉同一個 task 裡看到舊狀態的那一類（A4）；可以和這份的做法並用，處理會執行 script、不能同步的那幾項。
- **不做**：A 類繼續不穩定（2026-11-23 起維持現狀，不做過渡），同類的 bug 繼續長出來。

## 先例

- **Chromium**：renderer 裡也有兩層，Blink（`LocalFrame`、DOM、inspector 的 agent）和 content 層（`RenderFrameImpl`），在同一個 thread 上。Blink 要讓 content 層知道一件事時當場呼叫 `WebLocalFrameClient`，不留便條；DevTools 的 agent 由 `probe::` 在改變的當下呼叫，CSS agent 列 sheet 時直接讀 document（`inspector_css_agent.cc`），請求送出的當下就告訴 Network agent（`resource_load_observer_for_frame.cc`）；同一份文件內的導覽同步 commit（`FrameLoader::StartNavigation`）；跨 process 的用 mojo 的訊息。以上對過 152.0.7977.82 的原始碼。Blink 靠 raw pointer 和 Oilpan 互相指，同步呼叫沒有借用的問題，代價是到處檢查 frame 還在不在，以及執行期的 `ScriptForbiddenScope`。
- **分層**（2026-11-23 的審查更正）：這份要處理的狀態，在 Chromium 都在 Blink core，不在 content 層：請求的編號（`CreateUniqueIdentifier`，一個 process 的 atomic 計數器，`unique_identifier.cc`，對過）、`fetch()` 的記錄（`ResourceFetcher`、`FetchManager`）、Network、Page、CSS 的 inspector agent、frame 的樹。照 Chromium 的分層它們在 machopo 的 `dom` 和 engine 那一側，支持做法三，不支持「renderer 擁有、借給 engine」。另外，Blink 的同步呼叫可以重入：content 層被呼叫時可能執行 script（`CheckCompletedInternal` 的註解），machopo 的 hook 不行（「設計概要」的最後一段）。
- 其他瀏覽器沒有對照。

## 已決定的事項

1. **開始做第 0 階段**（2026-11-23，使用者要求）：只評估、不改產品。
2. **原型用 `fetch()` 的登記**（2026-11-23，「待決定」原本的第 4 條）：A2、A3 牽涉 Network agent、`RequestIds` 的 `Rc<RefCell>` 和 frame 的表，renderer 的三種狀態都碰到，比得出兩種做法的差別，改動中等。不選 frame 的建立和移除：最能代表難的部分，但原型本身就接近實作；不選同一份文件內的捲動：已經有原型，只碰到 engine 擁有的 `Frame`，比不出差別。
3. **原型不合併，patch 放在 `prototypes/`**（2026-11-23）：原型在另一份複本上做，結果和 patch（`.diff`）放進這個資料夾的 `prototypes/`，`0110` branch 上不帶原型的程式碼，之後要重做時套 patch（`rfcs/README.md` 的「資料夾」：原型不合併）。不選把原型 commit 在 branch 上再 revert：branch 上會有要拿掉的程式。
4. ~~**擁有權用混用的做法**~~（2026-11-23，使用者同意第 0 階段的看法；原本「待決定」的第 1 條。**同一天重新打開**，回到「待決定」第 1 條：獨立的審查指出下面「也不像 Chromium」的理由是錯的，那幾份狀態在 Chromium 是 Blink core 的；做法二的原型在換 engine 時會 panic；另外提了做法三，原型補做了。以下是當時的內容）：renderer 自己的狀態用做法二，由 render thread 擁有，取 engine 時借進它的 hook（原型 V2：`EngineSlot`，不需要 `unsafe`）；DOM 層兩邊都要的資料（frame 的樹）放進 `dom`。不選只用做法一：狀態跟著 engine 走，沒有 engine、wasm 的 engine、換 engine 時都要另外處理（原型裡 wasm 的 engine 丟掉 sink 時差點連狀態一起丟掉），被好幾個 host 共享的表（`DocumentLoaders`）要把那些 host 併成一個；也不像 Chromium，content 層的 `RenderFrameImpl` 擁有自己的狀態。不選只用做法二：frame 的樹本來就有一份在 `dom`（`content_document`、`frame_owner`），借給 engine 反而多一份。代價：renderer 取 engine 的每一處都要改（一次性、機械式），每次取 engine 時借出、取狀態時拿回各一次 downcast；只讀寫 DOM 和 layout 的地方用不借的 `unlent_mut`，用錯時是執行期的 `expect`。frame 的樹放進 `dom` 的部分沒有做原型。
5. **擁有權用做法三**（2026-11-23，使用者同意；第 4 條重新打開之後）：兩邊都要的資料由 `dom` 擁有（frame 的樹、文件的 loader、`fetch()` 送出的請求這一類），engine 呼叫 hook 時把 hook 要的當參數帶過去（`FetchOrigin` 的推廣），只有 renderer 自己用的資料由 renderer 擁有、不借給誰；請求的編號每個 renderer thread 一個計數器（Blink 的 `CreateUniqueIdentifier`）。理由：原型 V3 是三個裡唯一沒有借出、拿回的執行期失敗的（換 engine 時 unload 裡的 `fetch()`，V2 panic、V1 的編號重來），改動和 V1 差不多（在 V0 之上 +163／−112），`Rc<RefCell<` 一樣少；也符合 Chromium 的分層，這幾份狀態在 Chromium 都在 Blink core。不選做法一：狀態跟著 engine 走，engine 的生命週期每一處都要另外處理，原型在換 engine 時把編號弄得從 1 重來。不選做法二：借出、拿回是執行期的檢查，和 `RefCell` 同一類，原型在換 engine 時 panic；hook 分散在 `State` 的不同欄位，一份借出的狀態放不進幾個地方；也不符合 Chromium 的分層（第 4 條的理由錯了的地方）。代價：`dom` 多了 host 讀的記錄；renderer 處理回應時要經過 engine 讀 DOM；DevTools 的 Page agent 要的每個 frame 的資料（source、resources、mime）在 renderer，要另外決定留成副本還是搬進 `dom`；frame 的樹放進 `dom` 沒有做原型。
6. **hook 可以拿 DevTools agent 的 `Rc`；frame 的 DevTools 事件這次不改成同步**（2026-11-23，使用者決定；第二次審查指出的衝突）：要在改變的當下告訴 DevTools，hook 就要拿得到 agent（`fetch()` 的登記要 `Rc<NetworkAgent>`），所以允許，每一處在註解寫明理由：agent 是 DevTools 的觀察者，在同一個 thread 上，沒開 DevTools 時先問 `enabled()`，不在每次呼叫都走的路上（Blink 的 agent 也是 probe 共用的 GC 物件）。frame 的 `frameAttached`、`frameDetached` 照樣在外圈送，記成已知的限制；agent 要 frame 的樹時直接讀 `dom` 那一份（修 A1）。不選連 frame 的事件也改成同步：要先把 DOM 的變動送出去、把 iframe 建立時取 stack 和建 frame 的順序對調（沒有驗證做不做得到），和 DevTools 收到事件的時機一起之後做。不選把 agent 搬進 `dom` 或 engine：等於重新設計 DevTools，這份做不完。
7. **範圍：乙加圖片和 script 的載入**（2026-11-23，使用者決定；原本「待決定」的第 1 條；第二次審查之後定的）。照這個順序，各自測試、mutation test、審核：
   1. **請求和 loader 交給 `dom`**：請求的編號每個 renderer thread 一個計數器；`fetch()` 送出的請求記在 DOM，取消也在 sink 當場做，拿掉產品裡走不到的「沒有 sink」那條路；文件的 loader 和 DevTools 的 loader id 由 `dom` 擁有，cookie 和 worker 的 hook 由 engine 帶參數，`DocumentLoaders` 不再共享（A2、A3）。
   2. **frame 的樹只留 `dom` 一份**：`FrameId` 搬到 `dom`，dom_js 留一張 FrameId 到 V8 context 的表；拿掉 renderer 的那幾份樹（`ChildFrames` 的樹、`FrameDocument.child_of`）；`FrameView` 的 child view 保留巢狀，用 `FrameLayout` 的 hook 同步建立和移除；`Created` 的補救換成 assert；WebDriver 和 DevTools 改讀 `dom`（A1、A5）。仍然延後、要寫明的：移除之後 parent 的 `finish_load`，和 renderer 私有資料的清理；`frameAttached` 照樣在外圈送（第 6 條）。
   3. **同一份文件內的捲動當場做**（`FrameLayout` 的 hook），拿掉 renderer 那份文件網址的副本（A4）。
   4. **`javascript:` 網址**照 Blink 在 `dom_api` 裡排成綁在 Document 上的 task。
   5. **intersection 的通知排成真的 task**（A6；原因是推論，先寫出必壞的測試）。
   6. **圖片和 script 開始載入**：它們的記錄照第 1 步的做法交給 `dom`，由 hook 在 Blink 的時機送出（圖片在 microtask、script 在 prepare 的當下）。
   不包括：console 和 DOM 變動交給 DevTools 的時機，和第 6 條沒選的 frame 事件同步一起之後做；`srcdoc` 的 commit、iframe 晚到的 `load`、parser 的續跑和外圈的其他工作，屬於把外圈搬進 scheduler，另寫一份 RFC，排在這份之後（狀態都只有一個擁有者之後，排成 task 才不會碰到借不到的問題）；DevTools agent 之間的 `Rc`、`BlobUrls`、worker thread 的 `Rc<RefCell>`；readiness、encoding、mime type 的副本（寫明誰為什麼留）。做完之後外圈還在：它剩下排程的角色，不再拿來補狀態。不選只做乙：圖片和 script 的載入是同一個擁有權的模式，留著就有兩種規則並存；不選連外圈一起拿掉：範圍變成兩份 RFC，event loop 改了，WPT 的時序要全部重看。

8. **導覽交給 Browser 的時機不改**（2026-11-23，使用者同意；原本「待決定」的第 1 條）：`location` 和 `history` 的導覽照 RFC 0019 第 24、35 條在 event loop 的最上層送。盤點沒有找到它造成的問題：導覽本來就是跨 process 的訊息，差別只在送出比 Chrome 晚那個 task 剩下的時間。不選改成呼叫的當下送：要推翻 RFC 0019 的兩條，好處只在時間上。
9. **這份先做完，再做 RFC 0109 剩下的**（2026-11-23，使用者決定；原本「待決定」的第 2 條）：0109 的第 2 階段（2-1 以外）和第 3、4 階段都排在這份之後。理由：最單純，兩份都要改 `RenderThread`（這份改欄位，0109 的第 3、4 階段改結束的路），先後排開就不衝突。代價：這份做的期間，CI 上 0109 的 B 類、port、啟動失敗的 crash 照樣不穩定，判斷這份的回歸時要先排除它們。不選先做 0109 第 2 階段的小修正：CI 的訊號會乾淨一些，但切換的次數多；不選 0109 全部先做：這份要 rebase 到改過的結束路徑上。
10. **第 1 步：拿掉「沒有 sink」的路；順便取消頂層文件換掉時的 `fetch()`**（2026-11-23，使用者同意）。`FrameRequest` 的 `Fetch`、`FetchSent`、`CancelFetch` 和 renderer 對應的處理都拿掉，`dom_js` 的測試改用測試的 sink：只有一條送出的路，記錄只有一個地方寫。不選留給測試：engine 裡會留著第二條送出的路，mutation test 也會有等價的 mutant。頂層的文件換掉時，取消它和它的 frame 還在路上的 `fetch()`（Chrome 在 execution context 不在時取消），`docs/limitations.md` 那一條跟著刪：記錄搬進 DOM 之後，碰的是同一張表。不選維持限制：範圍最小，但回應不來的請求一直留著。
11. **第 2 步：frame 的表在 engine 那一側；建立和移除照 Chrome 當場做完；分成小步**（2026-11-23，使用者決定；更正第 7 條第 2 步的寫法）。
    - **唯一的一份是 engine 那一側的**：dom_js 的 `Frames`（id、token、parent、owner、文件、V8 context）和 `dom` 的 `content_document`、`frame_owner`，在同一個操作裡一起改，不是跨邊界的副本（`dom_wasm` 只有頂層的）。renderer 經過 `ScriptEngine` 查，`FrameId`、`FrameToken` 不搬到 `dom`（第 7 條原本寫搬：`FrameToken` 是 `protocol` 的，`dom` 不依賴它，換來的好處這樣都有；Chromium 的 frame 樹也在 Blink core）。
    - **建立和移除當場做完，照 Chrome**：子 frame 的 unload、parent 的載入檢查和它的 `load`、child view、告訴 Browser，都在移除（和建立）的當下；renderer 不再靠 `Created`、`Removed` 的便條補做。前提是文件的載入狀態（readiness、還在等的載入，Blink 的 `DocumentLoader` 那一層）先搬到 engine 那一側，engine 在 `update_elements` 裡做（那裡本來就能執行 script）；這也修掉「拿掉 iframe 時不發 `pagehide`、`unload`」的限制。不選便條只留給 renderer 自己的帳：A1、A5 修得掉，但 parent 的 `load` 晚到移除的 task 之後（網頁看得到），renderer 還要保留便條。
    - **分成小步**，各自測試、mutation test、審核，太大就再拆；先寫出 A1、A5 的必壞測試。

## 待決定

無。

## 之後的可能

- 把外圈的其他工作（DOM 的 task 和 timer、parser 的續跑、`srcdoc` 的 commit、iframe 晚到的 `load`）搬進 scheduler，讓它們都是真的 task：通道上不斷有 task 時，外圈的工作現在都會晚。排在這份之後（已決定的事項第 7 條）。
- console、DOM 的變動和 frame 的事件在改變的當下交給 DevTools（已決定的事項第 6 條沒選的那一項）：先驗證 iframe 建立時取 stack 和建 frame 的順序能不能對調。
- 導覽在呼叫的當下交給 Browser（照 Chromium）。
