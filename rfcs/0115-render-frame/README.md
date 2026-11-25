# RFC 0115：每個 frame 的狀態照 content 和 Blink 的界線各有一個擁有者

- 開始日期：2026-11-25
- 狀態：🚧 已排程：2026-11-25（第 1 段；第 2 到 4 段是方向）
- 細節：`details.md`

## 摘要

一次導覽現在橫跨三處：engine 擁有的 hook（`RendererFrameClient`）在 setter 裡送 `BeginNavigation`；renderer 的 `RenderThread` 在 engine 的呼叫回來之後照 engine 記下的 `FrameRequest` 記帳（`handle_frame_requests`，十八個呼叫的地方）、收到 Browser 的 commit 時建文件、餵 parser、捲到 fragment；「frame 在載入中」、網址、session history 在 DOM 裡。照 frame 記的狀態在 `RenderThread` 散成八張表。根源是界線：`renderer` 同時做 Chromium 的 content 層（對 Browser 的導覽、commit、IPC）和一部分 Blink（文件的載入、parser、script 的排程、`FrameView`），engine 只有 DOM 和 script；擁有權又是單向的（renderer 擁有 engine，engine 擁有 hook），engine 呼叫 hook 時碰不到 renderer，只好呼叫之後再補。這份照 Chromium 的界線重新分：engine 那一側每個 frame 一個 `LocalFrame`（DOM 的 frame、文件和它的載入、parser、`load`），renderer 那一側每個 frame 一個 `RenderFrame`（對 Browser 的那一面），每一份狀態只有一個擁有者；`RenderFrame` 是它那個 frame 的 client，由 engine 的 frame 擁有，兩邊用同步的呼叫互通，事情發生的那次呼叫裡各自更新自己的狀態，沒有呼叫之後的對帳。分幾段做：第一段先把 renderer 那一側收成 `RenderFrame`，行為不變；之後的幾段是方向，信心不高（machopo 的 Blink 那一半分在兩邊：DOM 在 engine，layout、`FrameView` 在 renderer，界線不像 Chromium 那樣跟著 crate 走），等依賴的盤點和第一段做完再重新評估。

## 動機

現況（`details.md` 的「現況」，2026-11-25 的 `main`）：

- **送和帳分在兩處**（RFC 0112 的第 10 條）：hook 送出導覽，什麼都不記；engine 推一筆 `FrameRequest`（帶著 `begin_id`）、自己告訴 DOM「frame 在載入中」；呼叫回來之後 `handle_frame_requests` 才照那筆記 renderer 的帳（子 frame 的導覽、`navigations_asked`、origin 的來源、session history 的計數），並做幾件當場要做的事（子 frame 的 fragment 捲動、排 `javascript:` 的 commit、告訴 DevTools、要一個 main frame、跟著 frame 的增減更新表）。改導覽時要一起改 hook、engine 的 `FrameRequest` 和 `handle_frame_requests`。
- **對帳要記得叫**：每個 task 結束時 `after_task` 一定叫一次，所以 Browser 之後的 commit 不會對不上；但同一個 task 裡呼叫 engine 之後馬上讀帳或 frame 清單的地方要先叫，現在有十八處。漏了不會當掉，是靜靜地做錯：資源的回應找不到新 frame 的文件被丟掉、session history 的位置算錯、輸入的 ack 比它觸發的捲動早。
- **照 frame 記的狀態散在八張表**（最上層的 `document`、`ChildFrames` 的 `documents`、`navigations`、`commits`、`owner_loads`、最上層才有的 `main_origin_source`、`devtools_main_frame_token`，整個 renderer 的 `navigations_asked`），最上層和子 frame 分開；frame 不見時要每張各清一次（`ChildFrames::remove` 漏了 `owner_loads`）。
- **只在 renderer 這一側補做不乾淨**（`details.md` 的「做法的比較」）：把帳搬進 hook、拆成共用的 `Rc<RefCell>`，都拿不掉要用 engine 的那幾件（fragment 的捲動要 layout，`FrameDocument` 的 parser 和 engine 同時借），一個 frame 的狀態照樣分兩半；只把八張表收成一個物件，對帳的十八處照舊。
- RFC 0112 的「之後的可能」記過這件事，建議在 site isolation 之前做；renderer 已經是自己的 process（RFC 0108），site isolation 會讓一個 renderer 只有一部分 frame 是 local 的，到時候本來就要 per-frame 的物件。

**目標**：

1. 每一份 per-frame 的狀態只有一個擁有者：文件和它的載入（parser、script 的排程、`load`、readiness、「在載入中」）在 engine 那一側的 `LocalFrame`；對 Browser 的那一面（進行中的導覽、`begin_id`、`navigation_id`、commit 的參數、navigation timing、cancellation window、這個 frame 的 fetch）在 renderer 那一側的 `RenderFrame`。
2. 沒有呼叫之後的對帳：送出導覽的那串呼叫裡兩邊各記自己的帳（engine 那一側記 frame 在導覽，`RenderFrame` 記對 Browser 的請求，照 Chromium），Browser 的訊息經過 engine 交給那個 frame、當場建文件和更新帳；`handle_frame_requests` 和它的十八個呼叫拿掉。
3. frame 的生命週期一處管：engine 建 frame 時建它的 `RenderFrame`，frame 不見時一起走，沒有要各清一次的表。
4. `RenderThread` 只剩整個 renderer 的事：scheduler、IPC 的分派、compositor 和 widget、DevTools 的分派。
5. 看得到的行為不變（`shell` 的導覽、iframe、session history、DevTools 的測試，WPT 的 `html/browsers/`、`html/semantics/embedded-content/the-iframe-element/` 照舊通過），Speedometer 不變慢。

**不在範圍內**：

- site isolation、remote frame（`RenderFrameProxy`）。
- 導覽本身的行為（`javascript:` 網址在呼叫之後才算載入中、`BlobURLToken`，`docs/limitations.md` 的那幾條）：界線搬好之後各自比較好修，但不在這份。
- `FrameView` 和 layout、paint、compositor 怎麼接：`FrameView` 留在 renderer（畫面那一側），這份只讓它問文件「擋著沒」、在 main frame 時捲到 fragment（待決定的第 2 條）。

## 設計概要

給改 machopo 的人，做完之後：

- 要知道一個文件的載入到哪（parser、script、`load`），看 engine 那一側的 `LocalFrame`；要知道一個 frame 和 Browser 之間的事（它在不在導覽、commit 的編號、timing），看它的 `RenderFrame`。
- 加一種導覽：engine 在 setter 裡呼叫那個 frame 的 client，client 送出、同時記帳。Browser 回的訊息到 `RenderThread`，交給 engine 的那個 frame，frame 做完自己的部分再呼叫 client。不用在任何地方對帳。
- 加一個 engine 的入口：不用記得任何事。

給寫網頁的人：沒有看得到的改變。

## 詳細設計

目標的樣子（每一段開始前對 Chromium 的原始碼、定那一段的細節，寫進「已決定的事項」）：

- **engine 那一側：`LocalFrame`**（Blink 的 `LocalFrame` 加上 `DocumentLoader`、`ScriptRunner`）：每個 DOM 的 frame 一個，在 `dom_api`（兩個 engine 共用）。擁有它的文件、文件的載入：body 的 bytes 由 renderer 交進來（Blink 的 `DocumentLoader::BodyDataReceived`），parser、編碼的判斷、parser 擋住的和 deferred 的 script、等著的樣式表、`load` 和 readiness、fragment 的導覽、「在載入中」。現在這些在 renderer 的 `FrameDocument`（`details.md` 的「狀態的歸屬」）。
- **renderer 那一側：`RenderFrame`**（content 的 `RenderFrameImpl`）：每個 frame 一個，是那個 frame 的 `FrameClient`。擁有：進行中的導覽（`begin_id`、Browser 的 `navigation_id`、origin 的來源、`referrerpolicy`）、commit 的參數、navigation timing、cancellation window 的編號、這個 frame 的 fetch 和 loader 的帳、DevTools 的 frame token。送 Browser 的訊息都經過它。
- **擁有權和呼叫的方向**：renderer 給 engine 一個工廠，engine 建 frame 時建它的 `RenderFrame`、frame 不見時丟掉（Blink 的 frame 和 `RenderFrameImpl` 一起建、一起走）。engine 往上的呼叫（要送 Browser 的事）在 engine 的呼叫裡當場經過那個 frame 的 client；renderer 往下的呼叫（Browser 的 commit、丟掉、body、載入完成）是 engine 的方法，帶著 frame，engine 交給那個 `LocalFrame`，它做完再呼叫 client。renderer 要讀一個 frame 的 `RenderFrame`（DevTools、測試）時經過 engine 借。沒有 `Rc<RefCell>`：每一份狀態只在一個物件裡，借用跟著呼叫的方向。
- **`RenderThread`**：scheduler、IPC 的分派（訊息照 frame 交給 engine）、compositor 和 widget、DevTools 的分派、整個 renderer 的東西（blob URL 的表、名稱表）。
- **Process 的邊界**：`RenderFrame` 是 renderer 這一側、送 Browser 的那一面，照 `docs/architecture.md` 的「Process 的邊界」，所有往 Browser 的訊息都經過它。

### 測試

- 每一段照舊通過：`shell` 的 `documents::`（文件、iframe、導覽）、`history::`、`navigation_timing::`、`script::`、`inserted_scripts::`、`modules::`、`style_sheets::`、`devtools_lifecycle::`、`devtools_network::`；`renderer`、`dom_api`、`dom_js`、`dom_wasm` 的測試；WPT 的 `html/browsers/`、`html/semantics/embedded-content/the-iframe-element/`、`html/browsers/history/`、`html/syntax/parsing/`、`html/semantics/scripting-1/the-script-element/`。
- 新的：frame 不見時它的帳一起走（排著的 owner `load` 不再觸發）；送出導覽之後、同一個呼叫裡讀到的帳已經是新的（現在做不到的順序）。
- mutation test：每一段的轉換（送出、commit、丟掉、`javascript:` 的 commit、frame 不見）、body 交給 `LocalFrame` 的路。
- Speedometer：段落做完、使用者要求時量（parser 和 script 的排程搬家，Editor、NewsSite 的 suite 會受影響）。

### 分階段

第 1 段是要做的；第 2 到 4 段是方向，各段的信心和未知數在下面的「信心和未知數」，第 1 段做完之後重新評估（已決定的事項第 1 條），結論也可能是只搬一部分、接受剩下的接縫。

1. **renderer 那一側收成 `RenderFrame`**（已決定的事項第 2 到 8 條）：八張表收進一個物件、每種轉換一個方法，最上層和子 frame 同一種；`handle_frame_requests` 的十八處變成一行分派；debug build 在讀表的地方檢查 engine 裡沒有沒分派的 `FrameRequest`。擁有者照舊是 `RenderThread`，行為和時機不變（除了第 8 條的修正）。之後的每一段都在它上面搬。
2. **文件的載入搬到 engine 那一側的 `LocalFrame`**：body 的 data pipe 交給它（照 Chromium，它自己收 bytes），parser 和解碼、parser 的 script、樣式表的請求和 render-blocking 的判斷、`load` 和 readiness；發請求經過 `LoadSink` 這類 hook，parser 讓出是 DOM 的一種 task（照 Chromium 的 `kNetworking`），畫面那一側問它擋著沒。畫面那一半（`rendered_*`、`load_finished` 和 reftest 的 swap promise、標題的時機）留在 renderer，跟著 `FrameView`。`documents.rs` 的大半、兩個 engine（`dom_wasm` 只有最上層）都要動，要再分小段。
3. **`RenderFrame` 由 engine 的 frame 擁有**：導覽在 client 裡送出並記帳，Browser 的訊息經過 engine 往下走；`FrameRequest` 的導覽、`handle_frame_requests`、十八處和第 1 段的檢查拿掉。
4. **fragment 的捲動排到下一次 main frame**：待決定的第 2 條；可能併進第 3 段。

### 信心和未知數

2026-11-25 的估計：

| 部分 | 信心 | 理由 |
| --- | --- | --- |
| 第 1 段 | 高 | 機械的搬移，行為和時機不變，測試守得住 |
| 第 3 段：導覽的送出和記帳在 client 裡，十八處裡導覽的那幾處拿掉 | 中 | 要第 2 段先把文件的載入搬開 |
| 第 2 段：文件的載入搬到 engine 那一側 | 中（盤點之前是中偏低） | 要搬的比只搬 parser 多（發子資源的請求、擋畫面的判斷），但機制大多已經有：hook（`LoadSink`、`TaskWaker`）和「畫面問 engine」；見下面的未知數 |
| 全部做完、每份狀態一個擁有者、哪裡都不用對帳 | 中偏低 | 文件和畫面之間會剩一道「畫面問文件擋著沒」的界線，但那是問的，不是對帳；風險在第 2 段的規模和 parse 的迴圈的效能 |

未知數（2026-11-25 的依賴盤點之後，`details.md` 的「依賴的盤點」和「盤點之後的看法」）：

- **沒有一個欄位只屬於文件的載入**：`FrameDocument` 的 64 處使用，33 處同時碰畫面、30 處碰載入、17 處碰 scheduler、20 處碰 DevTools；最纏的是兩個 commit、parse 的迴圈（`advance_document`）、`update_document_rendering`、`start_loads_of`、`sheet_loaded`。這些函式要先拆開才搬得動。
- **界線不跟著 crate 走**：Blink 的工作在 machopo 分在兩邊：DOM 和 binding 在 `dom`、`dom_api`、`dom_js`，layout、paint、`FrameView`（`frame_view.rs` 4861 行）和驅動 parser 在 `renderer`。文件的載入搬到 engine 那一側之後，和畫面有關的（`FrameView`、fragment 的捲動、擋住第一次畫面的樣式表、`FrameDocument` 的 `rendered_*`、reftest 的等待）還在 renderer，接縫可能只是從「導覽」移到「文件和畫面」之間。
- **parser 讓出 event loop**：`yields`、`parse_due` 現在是 renderer 的 scheduler 的 task；搬過去之後要經過像 `TaskWaker` 的東西排 task。
- **樣式表和 script 的抓取**：經過 loader 和 load sink，`css_refetches` 這類帳要找到它的主人。
- **兩個 engine**：`dom_js` 和 `dom_wasm` 都要有 `LocalFrame`；放在 `dom_api` 共用要先確認不碰 engine 專屬的東西。
- **效能**：parse 的迴圈和 parser 的 script 是熱的路；搬過去之後每次讓出、每個子資源的請求都經過 hook。

## 缺點

- 規模：`render_thread/documents.rs`（5789 行）的大半、`render_thread.rs`、`dom_js` 和 `dom_wasm` 的 frame、`dom_api` 的 `FrameClient` 和 `ParserHost`；比 RFC 0114 大好幾倍，要很多段。
- 第 2 段搬的是熱的路（parser、script 的排程），要留意 Speedometer。
- renderer 讀一個 frame 的 `RenderFrame` 要經過 engine 借（不能同時借 engine 的別的部分）。
- 做到一半時（第 1、2 段之後）界線是新舊混著的，文件要跟著每一段寫清楚。

## 理由和其他做法

完整的比較在 `details.md` 的「做法的比較」。

- **不選只在 renderer 這一側補**（2026-11-25 討論過）：
  - 只收成 `RenderFrame`（C）：表乾淨了，十八處照舊；加 debug build 的檢查可以讓「忘了叫」在測試裡 panic，但送和帳照樣分開。它是這份的第 1 段。
  - 導覽的帳搬進 hook（A）：送出時就記帳，但文件、parser 留在 `RenderThread`（要和 engine 同時借），fragment 的捲動要 layout，一個 frame 照樣兩半，讀帳要經過 engine 的 downcast。
  - 共用的 `Rc<RefCell>`（B）：能搬進去的是帳、scheduler、DevTools，搬不進去的是要 engine 的那幾件；171 處碰 engine 的地方任何一處借著格子呼叫 engine 就在執行時 panic，型別擋不住。要歸零還得把剩下的改成排 task，改了時機。
- **不考慮 engine 的入口多收一個 `&mut` 的 host**：hook 在 V8 的 callback 裡被呼叫，從 isolate 的 slot 拿狀態，借用存不進去（要 `'static`，或在 `bindings` 加有 `unsafe` 的機制）。
- **不做的話**：導覽的每個改動照舊改三處、新的 engine 入口要記得對帳，site isolation 時再一次拆表。

## 先例

- **Chromium**（2026-11-25 對過原始碼，`details.md` 的「對照 Chromium（原始碼）」）：每個 local frame 兩個物件：Blink 的 `LocalFrame`（`DocumentLoader` 擁有 body 的 loader、parser 和解碼器；`HTMLParserScriptRunner`、`ScriptRunner`、`RenderBlockingResourceManager`、`Document::CheckCompleted`、`LocalFrameView`）和 content 的 `RenderFrameImpl`（`blink::WebLocalFrameClient`，對 Browser 的 `FrameHost`、`NavigationClient`）。送出導覽時兩邊各記一份：Blink 記這個 frame 在導覽（`FrameLoader::client_navigation_`），content 記對 Browser 的這次請求（`navigation_client_impl_`、`is_requesting_navigation_`），都在送出的那串呼叫裡；commit 時 content 把 Browser 的 data pipe 交給 Blink，Blink 建 `DocumentLoader`、自己收 body。parser 自己 post 到文件的 task runner；fragment 的導覽同步 commit，捲動排到下一次 lifecycle。兩個物件一起建、一起走，但靠 raw 指標和約定（content 先建、交給 Blink 的工廠，Blink 拆 frame 時告訴它）。照做：兩個 per-frame 物件、各自的狀態、同步的呼叫互通、一起建一起走、body 交給文件那一側、fragment 的捲動排到下一次 main frame。不照做：擁有權（Rust 裡不互相拿著指標；machopo 由 engine 的 frame 擁有 client，renderer 經過 engine 借）。

## 已決定的事項

1. **接受的範圍：第 1 段，第 2 到 4 段是方向**（2026-11-25，使用者決定）：第 1 段把 renderer 那一側收成 `RenderFrame`（八張表、每種轉換一個方法、最上層和子 frame 同一種、十八處變成一行分派、debug build 在讀帳的地方檢查 engine 裡沒有沒分派的 `FrameRequest`），擁有者照舊是 `RenderThread`，行為和時機不變；它不管之後選哪個方向都要做。第 2 到 4 段照「分階段」和「信心和未知數」是方向，第 1 段做完之後重新評估，再決定接不接受。理由：依賴的盤點之後，第 1 段的信心高、自己看得出對不對；之後的幾段規模大、信心中等，第 1 段做完依賴會清楚得多。不選一次接受整個方向：第 2 段要拆的函式（兩個 commit、parse 的迴圈）還沒拆過，現在定細節會猜。不選只做第 1 段、不寫方向：第 1 段的形狀（`RenderFrame` 只放對 Browser 的那一面，還是連文件一起）要照之後的方向定。
2. **第 1 段：`RenderThread` 照 frame 的狀態是一張 `BTreeMap<FrameId, RenderFrame>`**（2026-11-25，使用者決定；A）：`RenderFrames` 取代 `document`、`ChildFrames`（`documents`、`navigations`、`owner_loads`；`commits` 的佇列也在 `RenderFrames` 上，第 4 條）、`main_origin_source`、`devtools_main_frame_token`；最上層和子 frame 同一種，最上層是 `frames[&TOP]`。`FrameId` 是 engine 照建立的順序給的（最上層是 0），走訪的順序和原本的 `Vec` 一樣。對照 Chromium：content 的 `g_frame_map`，每個 frame 一個 `RenderFrameImpl`。不選最上層一個欄位、子 frame 一張表：「照 frame 找」要分兩種情況，`doc`、`is_top` 的特例留著。不選 `Vec` 照 id 找：每次查表掃一遍。
3. **第 1 段：`RenderFrame` 的欄位，最上層和子 frame 的導覽同一種**（2026-11-25，使用者決定；A）：`document: Option<FrameDocument>`（子 frame 一建立就有 initial 的，最上層第一次 commit 之前沒有）、`navigation: Option<NavigationRequest { begin_id, no_referrer, origin_source }>`（最上層的 `main_origin_source` 併進來，`no_referrer` 是 false；commit 時照舊：子 frame 比 `begin_id`，最上層只拿 `begin_id` 對得上的 `origin_source`）、`devtools_token: Option<FrameToken>`（只有最上層，Browser 給的）、`owner_load_at: Option<f64>`。對照 Chromium：`RenderFrameImpl` 的 `navigation_client_impl_` 不分最上層和子 frame。不選兩種各記各的：特例還在，第 3 段搬到 client 時要搬兩種。不選只搬文件：frame 不見要清每張表的問題還在。
4. **第 1 段：整個 renderer 的帳和 `javascript:` 的佇列留在外面**（2026-11-25，照建議，使用者同意）：`own_history_entries`（整個分頁的 session history 的計數）和 `navigations_asked`（照送出的順序在 task 結束時一次送 `NavigationCancellationWindowEnded`）留在 `RenderThread`；`javascript:` 的 commit 佇列跨 frame、照排進來的順序，在 `RenderFrames` 上，`RenderFrame` 的方法排和清。不選把 `navigations_asked` 放進每個 frame（Chromium 是每個 `NavigationClient` 一個 window）：送出的順序變成照 frame 的順序，換不到第 1 段要的東西。新的檔案 `render_thread/render_frame.rs` 放 `RenderFrames`、`RenderFrame`、`NavigationRequest` 和 `FrameDocument` 的定義；`documents.rs` 裡用它們的程式先不搬。
5. **第 1 段：`RenderFrame` 的方法只改自己的資料**（2026-11-25，使用者決定；A，**第 3 段要再看一次**）：方法回傳接下來要做什麼（例如沒送出的導覽是 fragment 還是 `javascript:` 的 commit），要用 renderer 狀態的步驟（子 frame 捲到 fragment、告訴 DevTools、要一個 main frame、排 `javascript:` commit 的 task）由 `RenderThread` 照回傳做。資料和動作分開、`RenderFrame` 可以單獨測。第 3 段 engine 擁有 `RenderFrame` 時它要自己做這些，那時再決定服務怎麼給（對照 Chromium：`RenderFrameImpl` 的欄位拿著 `FrameHost` 的 remote、task runner）。不選服務是 `RenderFrame` 的欄位：捲動和要 main frame 照樣要 `RenderThread`，第 3 段的形狀還沒定就先定了服務。不選服務當參數傳：每個呼叫要先組參數，第 3 段又改一次。
6. **第 1 段：debug build 在每次讀表時檢查 engine 沒有沒分派的 `FrameRequest`**（2026-11-25，使用者決定；A）：`ScriptEngine` 多一個 `has_frame_requests()`；`RenderFrames` 的讀取（`frame`、`frame_mut`、走訪）在 debug build 斷言它是 false，`handle_frame_requests` 分派的期間例外。忘了在呼叫 engine 之後分派的地方，第一次讀表就 panic。實作時逐一確認有沒有「先讀表、再分派」本來就對的地方。對照 RFC 0114 的 `assert_no_dom_task_lost`。不選只在處理 Browser 的訊息前和 task 結束時檢查：task 結束時本來就分派，抓不到十八處要防的「同一個 task 裡呼叫完馬上讀」。不選不加：十八處照舊只靠審查。
7. **第 1 段：子 frame 的 `RenderFrame` 照舊在呼叫之後建和拿掉**（2026-11-25，使用者決定；A）：`follow_child_frames` 照 engine 的 frame 清單建和拿掉 `RenderFrame`，時機不變；當場建（Chromium 的 `CreateChildFrame`）是第 3 段 engine 擁有 `RenderFrame` 時的事。不選在 hook 的 `child_frame_created` 當場建：hook 碰不到 `RenderThread` 的表，等於把第 3 段提前做一部分。
8. **第 1 段：frame 不見時它排著的 owner `load` 一起拿掉，照修**（2026-11-25，使用者決定；A）：現在 `ChildFrames::remove` 沒清 `owner_loads`，`known()` 把不見的 frame 當成還在；收成 `RenderFrame` 之後拿掉 frame 就清掉。加一個測試，記進 `progress.md` 的找到的 bug。這是第 1 段唯一的行為改變。不選第 1 段保留舊行為、之後另外修：要在新的結構裡刻意重現漏清。

## 待決定

1. **第 2 段怎麼搬文件的載入**：`LocalFrame` 放在 `dom_api` 還是各 engine、parser（`html` crate）和 script 的排程搬過去的順序、要不要再分小段。盤點和第 1 段做完之後定。
2. **`FrameView` 和 fragment 的捲動放哪**：Chromium 的捲動不在導覽的呼叫裡：設好 `FragmentAnchor`、要一個 frame，lifecycle 的 layout 之後才捲。2026-11-25 的看法：照做，fragment 的導覽在 engine 那一側 commit、記下要捲到哪，下一次 main frame 時 `FrameView` 捲；這樣不用在 hook 裡碰 layout，`FrameView` 也不用搬（它留在 renderer，是畫面那一側的）。要確認的：子 frame 的 fragment 捲動和輸入的 ack 的順序（現在 ack 等捲動）。第 3 段之前定。

## 之後的可能

- site isolation：不是 local 的 frame 是 `RenderFrameProxy`，engine 那一側是 remote frame。
- 導覽的限制（`javascript:` 在呼叫之後才算載入中、`BlobURLToken`）：界線搬好之後，在 `LocalFrame` 和 `RenderFrame` 裡各自修。
- DevTools 的 agent 照 frame 的清單跟著 `RenderFrame` 的生命週期。
