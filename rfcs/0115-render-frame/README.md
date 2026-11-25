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
- `FrameView` 和 layout、paint、compositor 怎麼接：`FrameView` 留在 renderer（畫面那一側），這份只讓它問文件「擋著沒」、在 main frame 時捲到 fragment（待決定的第 1 條）。

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
2. **文件的載入搬到 engine 那一側的 `LocalFrame`**（分小段，已決定的事項第 11 條：先只排 2a）：
   - 2a：body 的解碼和 parse、parser 擋住的和 deferred 的 script（第 12 到 16 條）；renderer 只交 pipe 讀到的 bytes。
   - 2b：樣式表的請求、擋 script 和 render-blocking 的判斷（拿掉 2a 的 `set_scripts_blocked`），畫面那一側問它擋著沒。
   - 2c：`load` 和 readiness、「跑完 script 之後這個 frame 還在不在」。
   畫面那一半（`rendered_*`、`load_finished` 和 reftest 的 swap promise、標題的時機）留在 renderer，跟著 `FrameView`。`documents.rs` 的大半、兩個 engine（`dom_wasm` 只有最上層）都要動。
3. **`RenderFrame` 由 engine 的 frame 擁有**：導覽在 client 裡送出並記帳，Browser 的訊息經過 engine 往下走；`FrameRequest` 的導覽、`handle_frame_requests`、十八處和第 1 段的檢查拿掉。
4. **fragment 的捲動排到下一次 main frame**：待決定的第 1 條；可能併進第 3 段。

### 信心和未知數

2026-11-25 的估計：

| 部分 | 信心 | 理由 |
| --- | --- | --- |
| 第 1 段 | 高 | 機械的搬移，行為和時機不變，測試守得住 |
| 第 3 段：導覽的送出和記帳在 client 裡，十八處裡導覽的那幾處拿掉 | 中 | 要第 2 段先把文件的載入搬開 |
| 第 2 段：文件的載入搬到 engine 那一側 | 中（盤點之前是中偏低） | 要搬的比只搬 parser 多（發子資源的請求、擋畫面的判斷），但機制大多已經有：hook（`LoadSink`、`TaskWaker`）和「畫面問 engine」；見下面的未知數 |
| 全部做完、每份狀態一個擁有者、哪裡都不用對帳 | 中偏低 | 文件和畫面之間會剩一道「畫面問文件擋著沒」的界線，但那是問的，不是對帳；風險在第 2 段的規模和 parse 的迴圈的效能 |

第 1 段做完之後的評估（2026-11-25）：照這個方向做完，各個目標會怎樣：

| 目標 | 做完會怎樣 | 信心 | 可能卡住的地方 |
| --- | --- | --- | --- |
| 1. 每份 per-frame 狀態一個擁有者 | 大致成立，但一個 frame 有三個物件：engine 那一側的 `LocalFrame`（文件和它的載入）、`RenderFrame`（對 Browser 的那一面）、畫面那一側（`FrameView`，和 `rendered_*`、commit 的 swap promise、標題的時機） | 中 | 畫面那一份的主人還沒定；navigation timing 由 `RenderFrame` 給、`LocalFrame` 用，誰擁有要說清楚 |
| 2. 沒有呼叫之後的對帳 | `handle_frame_requests` 做的每件事都在呼叫裡有地方時成立：導覽在 client 送出時記；frame 由 engine 當場建和丟；blob URL 走 hook；fragment 的捲動排到下一次 main frame；告訴 DevTools、要 main frame 由 client 經過它的服務做；八處過期的讀法隨文件的載入搬走 | 中 | client 要能要 main frame、排 task（已決定的事項第 5 條說第 3 段再看），服務怎麼給還沒定 |
| 3. frame 的生命週期一處管 | 第 3 段成立：engine 建 frame 時建 `RenderFrame`，拆時一起丟；`FrameView` 的子 frame 已經在 hook 裡當場建 | 中偏高 | renderer 讀 `RenderFrame` 要經過 engine 借，Browser 的每種訊息變成 engine 的方法，兩個 engine 都要實作 |
| 4. `RenderThread` 只剩整個 renderer 的事 | 部分成立：文件的載入搬走、`documents.rs` 大幅變小，但畫面那一步（`update_document_rendering`）照樣在 `RenderThread` 照 frame 走 | 中偏低 | 要連畫面那一側的 per-frame 狀態整理，超出這份 |
| 5. 行為不變、Speedometer 不變慢 | 可能，要花力氣守：parser 讓出變成 DOM 的 task 之後和別的 task 的先後可能變（Chromium 本來是文件的 `kNetworking` 的 task runner），每個子資源的請求多經過一層 hook | 中 | 熱的路搬家，Editor、NewsSite 的 suite 要在 2a、2b 之後量 |

和 Chromium 對照：送出導覽時 Blink 和 content 各記一份（`FrameLoader::client_navigation_` 和 `navigation_client_impl_`），都在送出的那串呼叫裡。所以做完的樣子是「沒有呼叫之後的對帳」，不是「帳在一個地方」。最可能的結果：導覽送出時就記好、十八處和八處過期的讀法消失、frame 的增減一處管；剩下的接縫是畫面那一側的 per-frame 狀態（另一個題目）。

**第 2 段的停損點**（2026-11-25 的看法：第 2 段先只排 2a，當成有停損點的探測）：2a（body 和 parser 搬到 `LocalFrame`、renderer 只交 data pipe）做完時看三件事：

1. 為了它新增的 hook 和「問 engine」的方法有幾個：超過五個左右，表示界線切得不順（第 16 條列的是預期的）。
2. Speedometer 的 Editor、NewsSite 的 suite 有沒有變慢（使用者要求時量）。
3. parse 的迴圈裡的過期讀法（`advance_document` 的開頭、`note_body_parsed`）是不是真的不見了。

三件都順就接著做 2b（樣式表）；不順就停下來，回頭考慮停在第 1 段，或重新切界線。

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
3. **第 1 段：`RenderFrame` 的欄位，最上層和子 frame 的導覽同一種**（2026-11-25，使用者決定；A）：`document: Option<FrameDocument>`（子 frame 一建立就有 initial 的，最上層第一次 commit 之前沒有）、`navigation: Option<NavigationRequest { begin_id, no_referrer, origin_source }>`（最上層的 `main_origin_source` 併進來，`no_referrer` 是 false；commit 時照舊：子 frame 比 `begin_id`，最上層只拿 `begin_id` 對得上的 `origin_source`）、`owner_load_at: Option<f64>`。最上層的 DevTools token（Browser 給的）實作時改放在表上（`RenderFrames::top_token`），不放進最上層的 `RenderFrame`：Browser 可能在第一份文件 commit 之前就給，那時還沒有最上層的 `RenderFrame`，而換 engine 時表清空、token 要留著。最上層 commit 了別的文件（不是它在等的那個導覽）時，那個導覽留著，照原本的 `main_origin_source`。對照 Chromium：`RenderFrameImpl` 的 `navigation_client_impl_` 不分最上層和子 frame。不選兩種各記各的：特例還在，第 3 段搬到 client 時要搬兩種。不選只搬文件：frame 不見要清每張表的問題還在。
4. **第 1 段：整個 renderer 的帳和 `javascript:` 的佇列留在外面**（2026-11-25，照建議，使用者同意）：`own_history_entries`（整個分頁的 session history 的計數）和 `navigations_asked`（照送出的順序在 task 結束時一次送 `NavigationCancellationWindowEnded`）留在 `RenderThread`；`javascript:` 的 commit 佇列跨 frame、照排進來的順序，在 `RenderFrames` 上，`RenderFrame` 的方法排和清。不選把 `navigations_asked` 放進每個 frame（Chromium 是每個 `NavigationClient` 一個 window）：送出的順序變成照 frame 的順序，換不到第 1 段要的東西。新的檔案 `render_thread/render_frame.rs` 放 `RenderFrames`、`RenderFrame`、`NavigationRequest`；`FrameDocument` 的定義實作時留在 `documents.rs`（它的三十個欄位是那裡私有的，搬過去要全部改成 `pub(super)`）；`documents.rs` 裡用它們的程式先不搬。
5. **第 1 段：`RenderFrame` 的方法只改自己的資料**（2026-11-25，使用者決定；A，**第 3 段要再看一次**）：方法回傳接下來要做什麼（例如沒送出的導覽是 fragment 還是 `javascript:` 的 commit），要用 renderer 狀態的步驟（子 frame 捲到 fragment、告訴 DevTools、要一個 main frame、排 `javascript:` commit 的 task）由 `RenderThread` 照回傳做。資料和動作分開、`RenderFrame` 可以單獨測。第 3 段 engine 擁有 `RenderFrame` 時它要自己做這些，那時再決定服務怎麼給（對照 Chromium：`RenderFrameImpl` 的欄位拿著 `FrameHost` 的 remote、task runner）。不選服務是 `RenderFrame` 的欄位：捲動和要 main frame 照樣要 `RenderThread`，第 3 段的形狀還沒定就先定了服務。不選服務當參數傳：每個呼叫要先組參數，第 3 段又改一次。
6. **第 1 段：debug build 在每次讀表時檢查 engine 沒有沒分派的 `FrameRequest`**（2026-11-25，使用者決定；A；做法改成第 9 條、範圍縮成第 10 條）：`ScriptEngine` 多一個 `has_frame_requests()`；`RenderFrames` 的讀取（`frame`、`frame_mut`、走訪）在 debug build 斷言它是 false，`handle_frame_requests` 分派的期間例外。忘了在呼叫 engine 之後分派的地方，第一次讀表就 panic。實作時逐一確認有沒有「先讀表、再分派」本來就對的地方。對照 RFC 0114 的 `assert_no_dom_task_lost`。不選只在處理 Browser 的訊息前和 task 結束時檢查：task 結束時本來就分派，抓不到十八處要防的「同一個 task 裡呼叫完馬上讀」。不選不加：十八處照舊只靠審查。
7. **第 1 段：子 frame 的 `RenderFrame` 照舊在呼叫之後建和拿掉**（2026-11-25，使用者決定；A）：`follow_child_frames` 照 engine 的 frame 清單建和拿掉 `RenderFrame`，時機不變；當場建（Chromium 的 `CreateChildFrame`）是第 3 段 engine 擁有 `RenderFrame` 時的事。不選在 hook 的 `child_frame_created` 當場建：hook 碰不到 `RenderThread` 的表，等於把第 3 段提前做一部分。
8. **第 1 段：frame 不見時它排著的 owner `load` 一起拿掉，照修**（2026-11-25，使用者決定；A）：現在 `ChildFrames::remove` 沒清 `owner_loads`，`known()` 把不見的 frame 當成還在；收成 `RenderFrame` 之後拿掉 frame 就清掉。加一個測試，記進 `progress.md` 的找到的 bug。這是第 1 段唯一的行為改變。不選第 1 段保留舊行為、之後另外修：要在新的結構裡刻意重現漏清。
9. **1c：debug build 的檢查由 hook 設旗標**（2026-11-25，使用者決定；A）：renderer 的 hook 每個 `FrameClient` 方法先設一個 `Rc<Cell<bool>>`（只在 debug build），`RenderFrames` 拿著同一個，它的讀法斷言沒設，`handle_frame_requests` 對帳的期間例外、拿走 `FrameRequest` 之前清掉。取代第 6 條的 `ScriptEngine::has_frame_requests()`：`dom_js` 的佇列在 isolate 的狀態裡要 `&mut self`，讀表的地方多半只有 `&self`。`Rc<Cell>` 的理由：hook 是 engine 擁有的、表是 render thread 的，engine 在它的呼叫裡叫 hook，共用一個旗標只有這樣；只在這條 thread、只在 debug build。不選只在有 `&mut` 的讀法檢查：直接讀 `render_frames` 的幾十處守不到。不選所有讀法經過 `RenderThread` 的一個入口：子模組看得到欄位，繞過入口只靠約定。
10. **1c：檢查只守導覽的帳**（2026-11-25，使用者決定；B）：檢查的是 `navigation`、`take_navigation`、`take_any_navigation`、`is_navigating`，不守子 frame 的清單和文件。第一版也守清單，找到八處 renderer 在 engine 的呼叫之後、對帳之前就讀了清單（`docs/test-gaps.md`）；逐一先對帳修得掉，但對帳的動作跟著提早、和第 1 段「時機不變」不合，照這樣是每個會跑 script 的 engine 呼叫之後都要對帳。這八處在第 2、3 段搬到 engine 那一側（讀 engine 自己的 frame，不會過期）時處理，剩下的是「跑完 script 之後這個 frame 還在不在」（Blink 的 `IsDetached`）。不選逐一修：十八處對帳長成三十處上下，第 2、3 段又要拿掉。不選只在讀「表上沒有的 frame」時檢查：不見了還留著的 frame 抓不到，規則難說清楚。
11. **第 2 段先只排 2a，有停損點**（2026-11-25，使用者決定；A）：2a（body 和 parser 搬到 engine 那一側的 `LocalFrame`）做完時照「信心和未知數」的三個停損點看，都順才做 2b。使用者在意的是「送和帳分開」和「很多地方要做檢查」（目標 2）；「一個 frame 的狀態全在一處」不必完全成立，界線乾淨就好（目標 1 照這個讀：每份狀態一個擁有者、擁有者之間的界線清楚，一個 frame 可以有幾個物件）。不選一次接受第 2、3 段：最難的函式還沒碰，2a 會讓真實的難度清楚。不選停在第 1 段：十八處和八處過期的讀法照舊。
12. **2a：`LocalFrame` 在 `dom_api`，engine 擁有**（2026-11-25，使用者決定；A）：和 engine 無關的 Rust 型別，兩個 engine 的每個 frame 各拿一個（`dom_js` 的 frame、`dom_wasm` 的最上層），要跑 script 時經過 `ScriptHost`、`ParserHost` 這類既有的介面；每個 engine 只做「找到 frame 的 realm、進 scope」的薄包裝。對照 Chromium：`DocumentLoader` 在 Blink 的 core，和 V8 無關。不選各 engine 一份：同一套規則寫兩次。不選型別在 `dom_api`、實例在 renderer：engine 的呼叫裡碰不到，過期的讀法照舊。
13. **2a：renderer 讀 data pipe，`LocalFrame` 解碼和 parse**（2026-11-25，使用者決定；A）：renderer 只管傳輸（從 pipe 讀 bytes、排下一次讀），bytes 交給 engine；判斷編碼（`encoding.rs` 的 `DocumentDecoder` 搬到 engine 那一側）、文件的編碼、parse 是 `LocalFrame` 的。DevTools 要的原始碼改成問 engine。界線是「傳輸」對「文件」，`dom_api` 不依賴 `ipc`。對照 Chromium：Blink 的 `NavigationBodyLoader` 自己讀 mojo 的 pipe、解碼器是 parser 的；machopo 的 pipe 是 `ipc` 的、process 之間的東西，留在 renderer。不選 `LocalFrame` 自己讀 pipe：`dom_api` 要依賴 `ipc`，讀的訊號要經過 hook 叫醒 thread。不選 renderer 也解碼：文件的編碼是文件的事（它的 script 和樣式表用它解碼），留在 renderer 要再給 engine 一份。
14. **2a：「接著 parse」是 DOM 的一種 task**（2026-11-25，使用者決定；A）：parser 讓出時 `dom` 編號、經過 `TaskWaker` 送記號，和 timer、element task 一樣（Chromium 的文件的 `kNetworking` task runner）；讓出照時間的預算和「有輸入在等」（既有的 `InputMonitor`，`isInputPending()` 用的那個 hook）。從 renderer 的 `InternalTask::ParseOn` 移到 DOM 的 queue，和別的 task 的先後可能變（兩個 queue 都照 post 的順序，差在 DevTools 暫停時取哪些 queue）：WPT 和 `shell` 的測試守著。不選經過 hook 排 renderer 的 task：多一個 hook，文件的事繞回 renderer。
15. **2a：parser 擋住的和 deferred 的 script 跟著 parser 一起搬**（2026-11-25，使用者決定；A）：第 14 條讓 parse 在 engine 的呼叫裡跑，script 留在 renderer 的話，parser 碰到 script 只能記一筆、呼叫之後處理，又是一種對帳。所以 2a 是 body 的解碼加上 parser 加上 parser 的 script：請求走 DOM 自己排的 script 那條路（`LoadSink::start_script_fetch`、回應交給 engine），`advance_document` 整個搬到 `LocalFrame`（對照 Chromium 的 `HTMLParserScriptRunner`，parser 的）。原本的 2b 併進來，2a 變大，但中間沒有對帳的過渡形狀。不選 script 留到 2b、2a 的 parse 經過 renderer 的 task：過渡形狀，2b 又要改一次 parse 怎麼排。不選碰到 script 時記一筆對帳：和這份的目標相反。
16. **2a：renderer 和 engine 之間的方法**（2026-11-25，照建議，使用者同意）：renderer 往下：commit 時建 `LocalFrame`（`create_window` 多帶 MIME type、transport 的 charset、要不要 parse）、`body_data(frame, bytes)`、`body_ended(frame, status)`、script 的回應（DOM 自己排的 script 那條路）、`set_scripts_blocked(frame, bool)`（樣式表擋不擋 script：樣式表的帳在搬之前還在 renderer，變了時推給 `LocalFrame`，它不用在呼叫裡回頭問；這是過渡的，樣式表搬過去時拿掉）。engine 往上：script 的請求（`LoadSink::start_script_fetch`）、有沒有輸入在等（`InputMonitor`）。renderer 在呼叫之外問 engine：DevTools 要的原始碼、編碼、body 做出來了沒（render-blocking）。名稱和參數實作時可能調整，改了寫回這一條。對照 Chromium：往下的是 `WebNavigationBodyLoader::Client` 和 `WebLocalFrame` 的那幾個方法，往上的是既有的 hook。

## 待決定

1. **`FrameView` 和 fragment 的捲動放哪**：Chromium 的捲動不在導覽的呼叫裡：設好 `FragmentAnchor`、要一個 frame，lifecycle 的 layout 之後才捲。2026-11-25 的看法：照做，fragment 的導覽在 engine 那一側 commit、記下要捲到哪，下一次 main frame 時 `FrameView` 捲；這樣不用在 hook 裡碰 layout，`FrameView` 也不用搬（它留在 renderer，是畫面那一側的）。要確認的：子 frame 的 fragment 捲動和輸入的 ack 的順序（現在 ack 等捲動）。第 3 段之前定。

## 之後的可能

- site isolation：不是 local 的 frame 是 `RenderFrameProxy`，engine 那一側是 remote frame。
- 導覽的限制（`javascript:` 在呼叫之後才算載入中、`BlobURLToken`）：界線搬好之後，在 `LocalFrame` 和 `RenderFrame` 裡各自修。
- DevTools 的 agent 照 frame 的清單跟著 `RenderFrame` 的生命週期。
