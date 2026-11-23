# RFC 0110：renderer 和 engine 的狀態只有一個擁有者

- 開始日期：2026-11-23
- 狀態：✅ 已完成：2026-11-23
- 進度：`progress.md`
- 細節：`details.md`

## 摘要

renderer 和 script engine 在同一個 thread 上，彼此卻像隔著 process：engine 改了 renderer 要知道的事，記成便條（`FrameRequest`），renderer 等 event loop 的外圈才補做；renderer 自己也留著副本（frame 的樹、文件的網址和載入狀態、圖片和 script 的請求），等外圈才更新。RFC 0109 盤點到的不穩定測試 A1 到 A6 都出在這裡，網頁也看得到（設了 `location.hash` 之後，同一個 task 讀到的捲動位置是舊的）。照 Blink 和 content 層改成每份狀態一個擁有者：兩邊都要的資料由 `dom` 或 engine 擁有，engine 呼叫 host 的 hook 時把 hook 要的當參數帶過去，只有 renderer 自己用的留在 renderer；Blink 當下做的當下做（`fetch()`、圖片、script 的請求送出，frame 的建立和移除，捲到 fragment），Blink 排成 task 的排成真的 task（`javascript:` 網址、intersection 的通知）。A1 到 A6 都有在原本的程式上必壞的測試，改完都通過。renderer 的共享的 `Rc<RefCell>`（`DocumentLoaders`、`RequestIds`）拿掉了；新加的只有 DOM 照它原本的做法的 `RefCell` 欄位（送出的請求的記錄）、兩個 thread-local 的計數器，和第 6 條允許的 Network agent 的 `Rc`；沒有新的 `unsafe`。

## 動機

**原本**（2026-11-23 照程式看的）：

- renderer 把 `Box<dyn FrameLayout>`（renderer 的 `Frame`）和 `Box<dyn LoadSink>` 交給 engine 擁有；呼叫 engine 時，engine 從 `RenderThread` 借出 `&mut`，這段期間借不到 `RenderThread` 的其他欄位。engine 要碰 renderer 的東西時只剩兩條路：
  - **便條**：`FrameRequest`（九種）在 engine 的呼叫返回之後才交給 renderer；renderer 在 19 處取出來處理（包括 `RenderThread::run` 的外圈），外圈另外補做 intersection 的通知、圖片和 script 的請求。
  - **共享**：`RendererLoadSink` 的 `RequestIds`、`DocumentLoaders` 是 `Rc<RefCell<…>>`。
- 結果是同一件事在兩邊各有一份，什麼時候對得上取決於外圈什麼時候跑到。

延後的有三種原因（`details.md` 的「審查」）：處理時要用 renderer 留的副本（A1、A3、A5），engine 執行中借不到 `RenderThread`；處理的過程會執行 script 或用 layout（`javascript:` 網址、iframe 移除之後的 `load`）；收尾點排在整批 task 之後（A2）。

**問題**：

- **不穩定的測試**：RFC 0109 的 A1 到 A6（`0109` branch 上的 `details.md`，`cb5930d`）。DevTools 收不到新 iframe 的 `CSS.styleSheetAdded`、`fetch()` 的 redirect 漏報和時間倒過來、`location.hash` 的捲動晚了兩個 frame、拿掉的 iframe 在 WebDriver 回錯的錯誤、intersection 的通知排錯順序。
- **網頁看得到**：同一份文件內的導覽之後，同一個 task 裡讀 `scrollY` 是舊的；拿掉 iframe 時不發 `pagehide`、`unload`；parent 的 `load` 晚到拿掉 iframe 的 task 之後。
- **順序靠收尾點維持**：少一處收尾就是下一個同類的 bug。
- **該檢查的事落在兩邊之間**：別的 origin 的 `javascript:` 網址曾在子 frame 裡執行（`97262a6` 修了）；便條認的是 frame 不是文件，等外圈的期間 frame 換了文件時，舊文件的要求可能套到新文件上。

**不在範圍內**（已決定的事項第 7、8 條）：外圈的其他工作（timer、parser 的續跑、`srcdoc` 的 commit）搬進 scheduler；跨 process 的狀態（Browser 的 session history）；導覽交給 Browser 的時機；console、DOM 的變動和 frame 的事件交給 DevTools 的時機；DevTools agent 之間的 `Rc`、`BlobUrls`、worker thread 的 `Rc<RefCell>`；0109 的其他不穩定原因。

## 設計概要

給之後改這部分程式的人：engine 裡的 DOM API 要讓 renderer 知道一件事時，直接呼叫 host 的 hook，呼叫返回時 renderer 的狀態已經更新；renderer、WebDriver 和 DevTools 要讀 frame 的樹、文件的網址、載入狀態、送出的請求時，讀 `dom` 或 engine 那唯一的一份。例如：

- `location.hash = 'x'` 返回時已經捲過去，同一個 task 讀 `scrollY` 是新的。
- 插入 iframe 返回時 frame 已經建好，Browser 已經聽到；`iframe.remove()` 返回之前，它和子孫的 `pagehide`、`unload` 已經發了，frame 已經不在，parent 的 `load` 已經排了，下一個 WebDriver 指令是 `no such window`。
- `fetch()`、`new Image().src = …`、插入的 `<script>` 在 Blink 送請求的時機送出，Network agent 在那時記下請求和時間。

被同步呼叫的 renderer 不能回頭執行 script，也拿不到 `RenderThread` 的其他欄位。所以會執行 script 的步驟（unload、iframe 的 `load`、`javascript:` 網址）都在 `dom_api` 裡、由 engine 做；renderer 只留自己用的帳（parser、body、Browser 那裡的導覽），在每次呼叫 engine 之後對照 engine 的 frame（「frame」一節）。

## 詳細設計

### 擁有權：兩邊都要的在 `dom` 和 engine，hook 拿參數

第 0 階段在 `fetch()` 的登記上比了三種做法的原型（`details.md` 的「原型」，patch 在 `prototypes/`）：engine 擁有 host 的物件（V1）、每次呼叫時把 renderer 的狀態借給 engine（V2）、兩邊都要的放進 `dom`、engine 把 hook 要的當參數帶過去（V3）。選了做法三（已決定的事項第 5 條）：它是唯一沒有借出、拿回的執行期失敗的（換 engine 時 unload 裡的 `fetch()`，V2 panic、V1 的請求編號從 1 重來），也符合 Chromium 的分層（請求的記錄、frame 的樹、inspector 的 agent 都在 Blink core）。

- **請求**：每個 renderer thread 一個請求編號的計數器（`loader::next_request_id`，Blink 的 `CreateUniqueIdentifier`）；`fetch()`、圖片、DOM 排著的 script 送出的請求照編號記在 DOM（`SentFetch`、`SentImage`、`SentScript`），回應來時 renderer 查 DOM 的記錄；文件的 loader 和帶來它的導覽記在 DOM（`Cx::document_loader`）。
- **hook 的參數**：engine 送請求時經過 `dom_api::load_origin` 組出 `FetchOrigin`（loader、frame 的 token、文件的網址、script 的 stack、parser 的行號、引用它的 sheet、import 它的 module），sink 用它告訴 Browser 和 DevTools，不用回頭查 renderer 的表。
- **DevTools**：hook 可以拿 agent 的 `Rc`（已決定的事項第 6 條：agent 是同一個 thread 上的觀察者，沒開 DevTools 時先問 `enabled()`）。
- **renderer 自己的**：只有 renderer 用的留在 renderer：`<link>` 的 sheet 細節（擋 render、擋 script 的旗標）、Browser 那裡的導覽和排著的 `srcdoc`、`about:` commit、parser 和 body、解碼。DOM 只記它判斷 `load` 要的：sheet 請求的編號、子 frame 的導覽進行中的旗標（第 13、15 條）。

### frame

- **frame 的表只有 engine 一份**（第 11 條）：dom_js 的 `Frames`（id、token、parent、owner、文件、V8 context）和 `dom` 的 `content_document`、`frame_owner` 在同一個操作裡一起改。renderer、WebDriver 和 DevTools 經過 `ScriptEngine` 的 `frame_token`、`frame_of_token`、`frame_parent`、`child_frames` 查，renderer 的 `ChildFrames` 的樹和 token 拿掉了。
- **建立和移除當場告訴外面**：Browser 經過 `dom_api::FrameClient`（Blink 的 `LocalFrameClient`，第 12 條）當場聽到 `DidCreateChildFrame`、`DidDetachFrame`；child view 經過 `FrameLayout::child_frame_created`、`child_frame_removed` 當場建立和移除，它的 compositor `DocumentId` 來自 thread-local 的計數器。
- **renderer 的帳對照 engine**（第 22 條）：`FrameRequest` 不再有 `Created`、`Removed`。engine 有 frame 建了或不在了時記一個旗標（`take_frames_changed`），renderer 每次呼叫 engine 之後對帳（`follow_child_frames`）：engine 有、renderer 沒有 `FrameDocument` 的子 frame 補一份初始 `about:blank` 的，engine 已經沒有的 frame 的帳拿掉、還在路上的 `fetch()` 取消。對帳在任何東西用到之前，外面看不出差別；不在 hook 裡當場做，因為 hook 改 `RenderThread` 的狀態要 `Rc<RefCell>`。

### 文件的載入狀態和 `load`

- **載入狀態在 DOM**（第 14、16 條）：`LoadProgress`（`Loading`、`Deferring`、`Parsed`、`LoadQueued`、`Loaded`）由 DOM 從 `readyState`、`DOMContentLoaded` 發過了、`load` 排了、「載入結束了」推出來。「載完了沒」是 `dom_api::check_completed`（Blink 的 `Document::CheckCompleted`、`ShouldComplete`）：文件是 `Parsed`、沒有在等的 sheet 請求、子 frame 都不在載入、沒有延後 `load` 的圖，就排 window 的 `load`。parent 的 `load` 照 HTML 排成 task，不照 Blink 在 `CheckCompleted` 裡同步發（第 20 條：同步發要讓每個呼叫的地方都能執行 script）。
- **拿掉 iframe**（第 17、18 條，照 Chrome 的 `LocalFrame::DetachImpl`）：`dom_api` 先 unload（它和子孫的 `pagehide`、`unload`，各在自己的 realm，可以執行 script），再 discard，再當場 `check_completed(parent)`。unload 的期間，文件的 unload counter（HTML 的名字）大於 0，擋住在它裡面建子 frame 和導覽它（Chrome 的 `SubframeLoadingDisabler`、`FrameNavigationDisabler`），一直到子孫都 unload 完；導覽離開文件時的 unload 也一樣。
- **iframe 的 `load`**（第 19 條，照 Chrome 的 `LocalDOMWindow::DispatchLoadEvent`）：子 frame 的 window `load` 的 task 發完 window 的 `load`，iframe 還連著、文件沒有被 discard 時接著在 iframe 上發 `load`，再設「載入結束了」、檢查 parent（`dom_api::lifecycle::end_load`）。在目前的 realm 直接發：dom_js 發事件時依賴 realm 的地方都照物件自己的。

### 導覽

- **捲到 fragment 在 engine 的呼叫裡做**：`dom_api` 的 `navigate` 找到目標（`fragment_target`）就經過 `FrameLayout::scroll_to_fragment` 捲（A4）；renderer 的文件網址的副本拿掉了，讀 DOM 的。
- **`javascript:` 網址排在文件上**（第 23 條，照 Blink 的 `ProcessJavaScriptUrl`）：網址放進文件自己的清單，排一個綁在文件上的 element task 一次跑完（`dom_api::history::run_javascript_urls`）；discard 了的文件的 task 不跑；script 讓那份文件導覽到別處時清掉清單。`load` 不等清單：machopo 的 `load` 是 task，排著的網址本來就在之後才排的 `load` 之前跑，等只會把 `load` 推到那段 script 排的 task 後面（Blink 在 `ExecuteJavaScriptUrls` 裡同步發 `load`，在那些 task 之前，所以不等才和 Chrome 的順序一樣）。

### intersection 的通知

第一個 observer 有了 entry 時，DOM 排一個 element task 送出通知（Blink 的 `PostTaskToDeliverNotifications`），不再由 renderer 在外圈補做（A6）。A6 的原因在寫必壞的測試時確定：一個 frame 算出 entry 之後，通道上排著的 task 讓外圈晚到下一個 frame 的 `requestAnimationFrame` 之後。

### 請求在 Blink 的時機送出

- **`fetch()`** 在呼叫的當下經過 `LoadSink::start_fetch` 送，同時告訴 Network agent（A2、A3；第 10 條）；取消也在 sink 當場做；文件換掉時取消它和它的 frame 還在路上的 `fetch()`（Chrome 在 execution context 不在時取消）。
- **圖片**在 DOM 的圖片 microtask 之後、**DOM 排著的 script**（不是 parser 自己的）在每次 binding 呼叫的結尾和 module graph 要它時，經過 `LoadSink::start_image_fetch`、`start_script_fetch` 送（第 24 條；Blink 的 `ImageLoader` 在 microtask、`ScriptLoader::PrepareScript` 在 prepare 的當下送）。兩個 engine 共用 `dom_api::send_image_fetches`、`send_script_fetches`。背景圖照樣由 renderer 在它的 style 更新之後送（Blink 也在 style recalc 裡找到它們）。
- **撤銷了的 blob URL**：請求在內圈就送，比外圈的撤銷先到 Browser，所以 renderer 的 sink 先查，排一個 Browser 會給的網路錯誤（`ERR_FILE_NOT_FOUND`）。導覽還沒有這一步（`docs/limitations.md`，「之後的可能」）。
- **DevTools 的 initiator** 要的 parser 行號記在 DOM 的元素上，送出時取一次。

### 外圈剩下的

外圈還在，但只剩排程的角色（timer、parser 的續跑、`srcdoc` 和 `about:` 的 commit、DevTools 的 frame 事件和 DOM 的變動），不再拿來補狀態。frame 的 DevTools 事件（`frameAttached`……）照樣在外圈送（第 6 條），記在 `docs/limitations.md`。

### 測試

驗收和做過的檢查在 `progress.md`。A1 到 A6 和這份改的每件網頁看得到的事，各有一個在原本的程式上必壞的測試（A6、parent 的 `load`、unload 的期間擋的事、`javascript:` 網址的取消和順序……）；每一步跑 mutation test，沒抓到的寫進 `docs/test-gaps.md`；相關的 WPT 目錄每一步跑。

### 分階段

0. **評估，不改產品**：盤點、三種做法的原型、兩次審查，交給使用者決定（第 1 到 7 條）。
1. **實作**：照第 7 條的六步，第 2 步分成 2a 到 2f（第 11、21 條），第 6 步分成 6a 到 6c（第 24 條），每一步各自測試、mutation test、commit。

## 缺點

- 大重構：`dom`、`dom_api`、`dom_js`、`dom_wasm`、`html`、renderer。
- `dom` 多了 host 讀的記錄（送出的請求、sheet 請求的編號、導覽進行中的旗標），renderer 要經過 engine 讀。
- renderer 的帳在呼叫 engine 之後才對照 engine 的 frame，不在 hook 裡當場（不加 `Rc<RefCell>` 的代價）；中間沒有東西讀它，外面看不出差別，但要記得每次呼叫 engine 之後對帳。
- 拿掉 iframe 時會執行 script（unload），之後的程式要容忍 frame 和文件不在；unload counter 擋的事要照 Chrome 一條條加。
- 請求在內圈送出之後，比外圈的事（blob URL 的撤銷）先到 Browser，要在 renderer 補檢查。

## 理由和其他做法

- **擁有權的另外兩種做法**：見「擁有權：兩邊都要的在 `dom` 和 engine，hook 拿參數」和第 4、5 條（做法一的狀態跟著 engine 走，engine 的生命週期每一處都要另外處理；做法二的借出、拿回是執行期的檢查，和 `RefCell` 同一類）。
- **每個 task 之後收尾**（RFC 0109 的 2-1 比較過）：放大的測試修得掉，但同一個 task、同一次 rendering 照樣看到舊狀態，五個探針只有一個和 Chromium 一樣。
- **讀之前先補**：漏一處就是下一個同類的 bug。
- **逐個改成同步、不動擁有權**：搬不動的狀態只能再加 `Rc<RefCell>`（原型 V0 就是這樣，多了一張共享的表）。
- **集中收尾**（每次呼叫 engine 之後收尾一次）：修得掉收尾點排錯的那一類（A2），修不掉同一個 task 裡看到舊狀態的那一類（A4）。renderer 的帳對照 engine 用的就是這個位置，但只補 renderer 自己用的帳。
- **便條只留給 renderer 自己的帳**（第 11 條沒選的）：A1、A5 修得掉，但 parent 的 `load` 晚到移除的 task 之後，renderer 還要保留便條。

## 先例

- **Chromium**：renderer 裡也有兩層，Blink（`LocalFrame`、DOM、inspector 的 agent）和 content 層（`RenderFrameImpl`），在同一個 thread 上。Blink 要讓 content 層知道一件事時當場呼叫 `LocalFrameClient`，不留便條；DevTools 的 agent 由 `probe::` 在改變的當下呼叫，請求送出的當下就告訴 Network agent（`resource_load_observer_for_frame.cc`）；同一份文件內的導覽同步 commit；跨 process 的用 mojo 的訊息。這份要處理的狀態在 Chromium 都在 Blink core，不在 content 層（請求的編號、`ResourceFetcher`、inspector 的 agent、frame 的樹），照這個分層它們在 machopo 的 `dom` 和 engine 那一側。Blink 靠 raw pointer 和 Oilpan 互相指，同步呼叫沒有借用的問題，代價是到處檢查 frame 還在不在，以及執行期的 `ScriptForbiddenScope`；Blink 的同步呼叫可以重入（content 層被呼叫時可能執行 script），machopo 的 hook 不行，會執行 script 的步驟因此都在 `dom_api`。以上對過 152.0.7977.82 和 Chromium main 的原始碼（各條已決定的事項寫了對過哪些）。
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
12. **第 2c 步：建立和移除子 frame 時給 Browser 的訊息走新的 `FrameClient`**（2026-11-23，使用者決定）：`dom_api` 的 trait，對應 Blink 的 `LocalFrameClient`（`CreateFrame`、`Detached` 都在它上面，content 的 `RenderFrameImpl` 當場送 `CreateChildFrame`、`Detach`）。不選加在 `LoadSink` 上：改動最少，但 `LoadSink` 的職責會變成載入加上 frame 的生命週期，和 Blink 的分層對不上。
13. **第 2d 步的第 1 項：`<link>` 的 sheet 請求，DOM 照編號記成「這份文件的 `load` 在等的請求」，細節留在 renderer**（2026-11-23，使用者決定；B）。照 Chromium main 的原始碼對過（`document.cc` 的 `ShouldComplete`、`link_style.cc`、`style_engine.cc`、`resource.cc`）：`load` 看的是文件的 `ResourceFetcher` 的載入中請求（`loaders_`，`IsLoadEventBlockingResourceType` 的 sheet、圖片、字型……），擋 script 和 parser 的是 `StyleEngine` 的另一份計數，擋 render 的是 `RenderBlockingResourceManager`；`<link>` 拿掉時 `StyleEngine` 的計數當場減掉，請求不取消，`load` 照樣等它回來（從讀到的程式推的，沒有跑過），和 machopo 現在一樣。renderer 的 `pending_sheets` 留著 `<link>`、URL 和擋 render、擋 script 的旗標（第 5 條：只有 renderer 用的留 renderer），DOM 的集合和它在同樣兩處（開始載入、收到回應）一起改。不選 DOM 現有的 `load_delay` 計數：等於 renderer 那張表的大小的副本，加減不成對時 `load` 永遠不發、查不出是哪個請求；Chrome 的 `load` 也不靠計數。不選整筆搬進 DOM（C）：DOM 要多存會被 GC 追蹤的元素和只有 renderer 用的旗標，改動約兩倍，等 engine 也要判斷 render-blocking 時再搬。
14. **第 2d 步的第 2 項：文件的解析和 `load` 的進度，DOM 在它本來就經手的地方自己記，最後的「載入結束了」由 host 設**（2026-11-23，使用者決定 B，同一天實作前更正成 B'）：現有的 `readyState` 之外，`document_ready(ContentLoaded)` 記下 `DOMContentLoaded` 發過了、`queue_window_load` 記下 `load` 排了（Blink 的 `Document` 的 `parsing_state_`、`load_event_progress_`，在 core），renderer 原本的 `Loading`、`Deferring`、`Parsed`、`LoadQueued` 由 DOM 從這三個推出來；`Loaded` 是另一個「host 結束了它的載入」的 bit，renderer 在原本設 `Loaded` 的地方設（iframe 的 `load` 分派之後、中止時；初始的 `about:blank` 沒有 parser 設它 loading，`readyState` 一建好就是 complete，DOM 推得出來）。renderer 的 `readiness` 拿掉，讀的地方問 DOM。更正的原因：原本說 `Loaded` 就是 `readyState` 是 complete，但 DOM 在 window 的 `load` task 裡就設 complete，renderer 的 `Loaded` 在之後的外圈、iframe 的 `load` 分派之後才設（iframe 的 `load` 不在子 frame 的 task 裡分派）；parent 的 `load` 等子 frame 的 `Loaded`，用 `readyState` 推會讓 parent 的 `load` 可能排在 iframe 的 `load` 之前。2e 把 iframe 的 `load` 搬進 engine 時，這個 bit 跟著改由 engine 設（和子 frame 的 `load` 在同一個 task 時可以拿掉，2e 開始前決定）。不選把五個值原樣搬進 DOM、renderer 照舊設：DOM 的 `readyState` 和它成了兩份，renderer 仍然是擁有者。不選在 2d 就把 iframe 的 `load` 搬進 DOM 的 `load` task：那是 2e 的工作，會讓 2d 的行為改變。
15. **第 2d 步的第 3 項：子 frame 的導覽進行中，DOM 在它的 iframe 元素上記一個旗標，細節留在 renderer**（2026-11-23，使用者決定；A）：Browser 那裡的導覽（`ChildFrames.navigations`）和排著的 `srcdoc`、`about:` commit（`commits`）照舊是 renderer 的，renderer 改它們的 7 處經過同一個入口，同時改 DOM 的旗標；子 frame 的文件還沒載完，DOM 從 frame 的樹和第 14 條看得出來。iframe 元素在 frame 換文件時還在，文件不在（DOM 不認得 `FrameId`）。對照 Blink：`Document::ShouldComplete` 的 `AllDescendantsAreComplete` 問每個子 frame 的 `IsLoading()`，在 core 的 frame 上（它怎麼算出來的沒有對過）。不選整筆搬進 DOM：`origin_source`、URL 和 markup 只有 renderer commit 時用，第 5 條。不選判斷時經過 hook 問 renderer：hook 拿不到 `RenderThread` 的狀態，除非加 `Rc<RefCell>`。
16. **第 2d 步的第 4 項：「載完了沒」是 `dom_api` 的 `check_completed`**（2026-11-23，使用者決定；A；Blink 的 `Document::CheckCompleted`、`ShouldComplete`）：經過 engine 的 `FrameLayout` 更新 style（找出背景圖：machopo 在 `load` 前更新一次，Blink 是在平常的 style recalc 裡找到），看文件是 `Parsed`、沒有在等的 sheet 請求（第 13 條）、子 frame 都不在載入（第 15 條的旗標、它的文件的進度）、`!load_event_delayed`、`!background_images_loading`，都好了就 `queue_window_load`。背景圖和 `<img>` 在 style 找到、請求建立的當下就算在載入，請求還沒送出也一樣，所以 engine 判斷時不用 renderer。renderer 的 `finish_load` 照舊先送 `<link>`、更新 style、送圖片的請求，再呼叫它（撤銷的 blob URL 的圖片在送的時候當場失敗，先送才當場完成）；2e 的 engine 在移除 iframe 時直接呼叫，那種圖片要等下一次 `finish_load`。不選只放查詢、更新 style 和排 `load` 由呼叫的人做：2e 的 engine 要把同樣的步驟再寫一次。不在這裡做的：`ShouldComplete` 的 `javascript:` 網址的 task、`IsInRequestResource`；`background_images_loading` 是整個 DOM 的、不是每份文件的（現有的行為）。
17. **第 2e 步的第 1 項：拿掉 iframe 時當場發 `pagehide`、`unload`，發的期間不建子 frame、不導覽**（2026-11-23，使用者決定；C）：`dom_api` 處理 iframe 拿掉的步驟改成先 unload、再 discard（原本先 `discard_content_document`，什麼都不發），用現有的規則（先自己、再照建立的順序發子孫的，初始的 `about:blank` 不發，各在自己的 realm），可以執行 script。DOM 的文件加一個 unload counter（HTML 的名字），`dom_api` 的 `unload_document` 發 `pagehide`、`unload` 的期間加一，導覽離開文件時的 unload 也一樣：建子 frame 時看 parent 文件的、導覽時看被導覽的文件的，大於 0 就不做。對照 Chrome（`LocalFrame::DetachImpl`，讀過原始碼）：unload 在拆 frame 之前、可以執行 script，先自己再 `DetachChildren` 發子孫的，期間有 `SubframeLoadingDisabler`、`FrameNavigationDisabler`、不准 `document.open`；commit 時的 `FrameNavigationDisabler` 沒有對過。不選只擋建子 frame（A，原本的建議）：理由是被拿掉的 frame 的導覽反正會被丟掉，但同一份文件內的導覽在 machopo 是當場做的（URL、`popstate`、`hashchange`、Browser 的 session history 多一筆），擋不到。`document.open` 現在沒有（沒有 `document.write`），實作時看同一個 counter。不選什麼都不擋（B）：handler 裡插入的 iframe 暫時有 frame，和 Chrome 不同。代價：導覽離開時 unload 的 handler 裡的同一份文件內導覽不再生效（找 WPT 有沒有測）。
18. **第 2e 步的第 2 項：拿掉 iframe 之後當場檢查 parent**（2026-11-23，使用者決定；A）：`dom_api` 拿掉 iframe 的那一步在 unload、discard 之後呼叫 `check_completed`（parent 是 iframe 的 node document，style 經過 `lend_layout`），parent 的 `load` 的 task 在那時就排。對照 Chrome（讀過原始碼）：`Detach` 的 `StopAllLoaders` 和 `FrameLoader::Detach` 呼叫 `DidFinishNavigation`，結尾是 `parent->CheckCompleted()`，都在 `iframe.remove()` 返回之前；Blink 當場發 `load`，machopo 照 HTML 排 task（第 16 條）。renderer 處理 `Removed` 時的 `finish_load(parent)` 留到 2f 和便條一起拿掉（多判斷一次，`load` 已經排了就是 `Waits`）。不選維持 renderer 在 task 之後判斷：parent 的 `load` 排在那個 task 裡之後才排的 task 後面，和 Chrome 不同。
19. **第 2e 步的第 3 項：iframe 的 `load` 在子 frame 的 window `load` 的 task 裡發，在目前的 realm 直接發**（2026-11-23，使用者決定；b）：`dom_api` 的 `WindowLoad` task 發完 window 的 `load`，iframe 還連著、文件沒有被 discard 時接著用 `fire_event` 在 iframe 上發 `load`，再設「host 結束了它的載入」（由 DOM 設，不再是 renderer），再 `check_completed(parent)`；renderer 的 `document_loaded` 只留 DevTools 的 lifecycle、`DidFinishLoad` 這些。對照 Chrome（讀過原始碼）：`LocalDOMWindow::DispatchLoadEvent` 發完 window 的 `load` 緊接著 `owner->DispatchLoad()`，同一個呼叫，不切 realm。dom_js 發事件時依賴 realm 的地方都照物件自己的（wrapper：`new_wrapper_in_realm`；`window.event`：listener 的 creation context；例外：listener 的 realm；`onload` 屬性：元素的 realm），所以不用切；`limitations.md` 說 iframe 的 `load` 晚一輪是「免得 wrapper 做在子 frame 的 realm」，這個理由已經不成立，拿掉。不選加一個進到 target 的 realm 再發的 hook（a，原本的建議）：多一個 hook，換不到什麼。不選 dom_js 的 task 迴圈在那個 task 之後補發（c）：規則離開 `dom_api`（第 16 條）。
20. **parent 的 `load` 照舊排 task，不照 Blink 在 `CheckCompleted` 裡同步發**（2026-11-23，使用者決定）：HTML 的「the end」是 queue 一個 task。同步發要讓每個呼叫 `check_completed` 的地方（renderer 有十來處：sheet 的回應、parse 完、圖片的回應、`finish_loads` 的迴圈……）都能執行 script、照 task 做 microtask checkpoint，listener 改掉 frame 的清單和文件的狀態之後的程式要逐處審查；換到的是和 Chrome 一樣的時序（`iframe.remove()` 返回前 parent 的 `load` 已經發了）。效能上兩者要做的事一樣多，差別只在 `load` 前面排著的 task 先跑還是後跑；同步發會拉長呼叫它的 task。
21. **第 2e 步分成三段，先寫必壞的測試**（2026-11-23，使用者決定）：2e-1 是 `shell` 的 `documents.rs` 的四個測試（拿掉 iframe 時當場發它和子孫的 `pagehide`、`unload`；unload 期間插入的 iframe 沒有 frame、`location.hash` 不生效；拿掉最後一個還在載入的 iframe 之後，parent 的 `load` 比同一個 task 之後排的 task 先到；iframe 的 `load` 比子 frame 的 `onload` 排的 task 先到，`e.target`、`window.event`、例外都在 parent 的 realm），2e-2 做第 17、18 條，2e-3 做第 19 條，各自跑相關的測試、WPT 和 mutation test，各自 commit（必壞的測試跟著讓它通過的那一段）。WPT 沒有直接測這幾件事的；`navigation-unload-same-origin-fragment.html` 守著導覽離開時 unload 的 handler 裡的 `location.hash`（第 17 條讓它不生效，原本的導覽照樣完成），`ignore-opens-during-unload.window.js` 要 `document.open`，跑不了。
22. **第 2f 步：拿掉 `Created`、`Removed`，renderer 自己的帳在原本處理它們的地方對照 engine 的 frame**（2026-11-23，使用者決定；A）：`FrameRequest` 不再有這兩種。renderer 原本處理它們的地方（每次呼叫 engine 之後的 `handle_frame_requests`）改成對帳：engine 有、renderer 沒有 `FrameDocument` 的子 frame 補一份初始 `about:blank` 的（DevTools 的 `frameAttached` 的 stack 照舊在那時拿），engine 已經沒有的 frame 的 `FrameDocument`、導覽、排著的 commit 這些拿掉，discard 了的文件的 `fetch()` 取消；engine 記一個「frame 建了或不在了」的旗標，沒變時跳過。child view 的 compositor `DocumentId` 改成 thread-local 的計數器（第 5 條的請求編號的做法），由 `child_frame_created` 當場設。`Removed` 原本的 blob URL 撤銷（`LoadSink::frame_gone` 已經當場做）和 `finish_load(parent)`（2e-2）是重複的，拿掉。對照 Chrome（憑記憶，沒有對過）：`RenderFrameImpl::CreateChildFrame` 和 `FrameLoader::Init` 當場建好 content 層的物件和初始空白文件的 `DocumentLoader`，`FrameDetached` 當場刪掉自己；machopo 的建和清在那次呼叫 engine 之後、任何東西用到之前，外面看不出差別，這是不加 `Rc<RefCell>` 的代價。不選需要時才建（B，原本進度表的寫法）：Chrome 沒有「沒有這一份就當成初始空白文件」的狀態，讀 `FrameDocument` 的幾十處都要處理沒有的情況，清理照樣要對帳。不選在 hook 裡當場做（C）：hook 改 `RenderThread` 的狀態要 `Rc<RefCell>`。
23. **第 4 步：`javascript:` 網址排在文件上，照 Blink**（2026-11-23，使用者決定；都照建議，第 2 項實作前改成不等）：對照 Chromium main 的原始碼（`frame_loader.cc` 的 `StartNavigation`、`document.cc` 的 `ProcessJavaScriptUrl`、`ExecuteJavaScriptUrls`、`CancelPendingJavaScriptUrls`、`ShouldComplete`，讀過）：網址放進文件自己的清單，排一個綁在文件上的 element task 一次跑完（discard 了的文件的 task 不跑，不會跑到別的文件上；machopo 的佇列照時間和編號排，用 element task 和另開 networking 的佇列順序一樣），`load` 不等清單，跑完也不再 `check_completed`（mutation test 指出它是多的：`load` 不等它們，其他會讓 `load` 不再等的事各自會判斷）（實作前更正：原本建議照 Blink 的 `ShouldComplete` 的 `!javascript_url_task_handle_.IsActive()` 等，但 machopo 的 `load` 是 task（第 20 條），佇列先進先出，排著的 `javascript:` 網址本來就在之後才排的 `load` 之前跑，等只會把 `load` 推到那段 script 排的 task 後面；Blink 在 `ExecuteJavaScriptUrls` 的 `CheckCompleted` 裡同步發 `load`，在那些 task 之前，所以不等才和 Chrome 的順序一樣）；script 讓那份文件導覽到別處（`dom_api` 的 `navigate`）時清掉清單（Blink 的 `Document::Abort` 經過 `CancelParsing` 取消）。不選照舊由 renderer 在 task 之後執行：便條認的是 frame，不是文件，`load` 不等它，新的導覽也不取消它。Browser 那一側開始的導覽（使用者重新整理……）走 renderer，不清清單：文件在 commit 時本來就換掉，記成已知的差異。不在這一步做的：`<iframe src="javascript:…">` 不執行（Blink 這時也把初始空白文件的載入狀態設回「`load` 還沒跑」），結果是字串時不取代文件。
24. **第 6 步：圖片和 script 的請求由 engine 在 Blink 的時機經過 `LoadSink` 送出，記錄在 DOM；分三段**（2026-11-23，使用者決定；都照建議）：圖片在 DOM 的圖片 microtask 排進佇列之後、script 在 `prepare` 之後，engine 當場經過 `LoadSink` 送（第 1 步 `fetch()` 的做法；Blink 的 `ImageLoader` 在 microtask、`ScriptLoader::PrepareScript` 在 prepare 的當下送，第 0 階段對過 Chromium 152 的 `image_loader.cc`、`script_loader.cc`）；送出的請求照編號記在 DOM（`fetch()` 的 `SentFetch` 的做法），renderer 的 `images`、`script_fetches` 拿掉，回應來了查 DOM 的記錄，解碼圖片、照 charset 解 script 還是 renderer 的；DevTools 的 initiator 要的 parser 行號從 renderer 的 `parser_lines` 搬進 DOM（DOM 已經替 `<style>` 記 `parser_line`），engine 送出時帶過去；背景圖照樣由 renderer 在它的 style 更新之後送（Blink 也在 style recalc 裡找到它們），同樣記在 DOM。分成 6a（parser 的行號）、6b（圖片）、6c（script），各自測試和 mutation test。不選維持 renderer 在 task 之後送：請求晚出去，DevTools 看到的順序和 Chrome 不同，被撤銷的 blob URL 要特例。

## 待決定

無。

## 之後的可能

- 把外圈的其他工作（timer、parser 的續跑、`srcdoc` 的 commit）搬進 scheduler，讓它們都是真的 task：通道上不斷有 task 時，外圈的工作現在都會晚。排在這份之後（已決定的事項第 7 條）。
- console、DOM 的變動和 frame 的事件在改變的當下交給 DevTools（已決定的事項第 6 條沒選的那一項）：先驗證 iframe 建立時取 stack 和建 frame 的順序能不能對調。
- 導覽在呼叫的當下交給 Browser（照 Chromium）。
- 導覽照 Chrome 帶著解析好的 blob（`BlobURLToken`）：先撤銷再導覽的失敗、撤銷之後重新載入的成功，兩個都修掉（2026-11-23 評估過，不在這份做：它是 blob URL 的生命週期，和擁有權無關，也不是不穩定的原因；只帶「撤銷了沒」的做法修不了重新載入）。
