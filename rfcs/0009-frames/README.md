# RFC 0009：iframe 和多個 browsing context

- 開始日期：2026-10-06
- 狀態：✅ 已完成：2026-10-08（第 1 階段「打底」；第 2 階段同源、同 process 的 iframe：2a 載入和 `contentWindow` 等，2b 子 frame 的 layout、paint、輸入和捲動；第 3 階段（子 frame 的導覽經過 browser、`WindowProxy`）在第 59 條移到「之後的可能」）
- 細節：`details.md`；進度：`progress.md`

這份 RFC 起於 RFC 0008 做完時的討論（2026-10-06）：DOM API 的下一批（`MutationObserver`、`Range`、`innerHTML` 等，RFC 0008 的「已決定的事項」第 50 條）開始之前，先評估 iframe 和 shadow DOM 會動到哪些很難事後改的地方。調查的結果在 `details.md` 的第 1 到 10 節。

## 摘要

同源、同一個 process 的 iframe（多個 browsing context 在一個 renderer 裡，像 site isolation 之前的 Chromium）。調查找到約 15 處「一個 renderer 只有一個 window」的假設（`details.md`）；第 1 階段先把 kun 改成「一個 renderer 可以有多個 window」也正確的模型：每個 window 的 realm、wrapper 建在物件的 realm、同源的 context 設同一個 security token、timer 記得自己的 window、需要 frame 時從元素的 document 找、`Window` 有 fully active 的狀態、document 有自己的 id。第 2 階段在這上面做 iframe：2a 的 `<iframe>` 有自己的 browsing context、window 和 document，由 renderer 載入 `src`、`srcdoc`，有 `contentWindow`、`contentDocument`、`frames`、`parent` 等；2b 的子 frame 照 iframe 的內容框做 style 和 layout、畫進父 frame、接收點擊和滾輪。

子 frame 的導覽和歷史經過 browser、`WindowProxy` 跨導覽的身分（第 3 階段）沒有做，Speedometer 也用不到，移到「之後的可能」；跨網站的 iframe 是 RFC 0004 的範圍。

## 動機

**現況（2026-10-06）**：
- **只有最上層的 frame**：renderer 的 `RenderThread` 只有一個 `FrameView`；`dom` 的 timer 和 `dom_tree_version` 是每個 heap 一份；`bindings` 每份文件建一個 V8 context；`browser` 的 `WebContents` 只有一個 renderer 端的 frame。`dom` 已經能有多份 document（RFC 0008 第 5 步），但只有一份有 window。
- **WPT 的阻礙**：RFC 0008 匯入的測試裡，約 490 個 subtest 讀不到 iframe 的 `contentDocument` / `contentWindow`，skip 清單裡 26 個檔案整個在等 iframe；shadow DOM 擋住 17 個測試檔（`details.md` 的第 8、9 節）。
- **錯的模式會被複製**：wrapper 建在「目前的」context、`<a>` 的換頁用執行中的 realm 的 frame、timer 不記得 window（`details.md` 的第 1 節）。只有一個 window 時看不出錯，但 DOM API 的下一批會照著寫，之後做 iframe 時要回頭改。

目標：
- 一個 DOM（heap）上可以有多個 window 和 realm，每個都照規範運作：物件的 wrapper 在它的 realm、同源的 realm 互相存取得到、timer 和 rAF 各自屬於自己的 window；需要「節點所在的 frame」的地方從節點的 document 找；`Window` 記得自己是否還在 browsing context 裡。
- `<iframe>` 照 HTML 建立、載入、移除子 frame，script 看得到它的 window 和 document；子 frame 照 iframe 的大小排版、畫在 iframe 裡、接收點擊和滾輪。
- WPT 裡等 iframe 的測試能跑；測試能在同一個 DOM 上建第二個 window 和 realm。

不在範圍內：
- **子 frame 的導覽和歷史經過 browser、`WindowProxy` 跨導覽的身分**（第 3 階段，第 59 條）：見「之後的可能」。
- **跨網站的 iframe 和 site isolation、跨語言的 frame**：RFC 0004（第 53 條）。
- **shadow DOM**：另開 RFC（`details.md` 第 9 節是評估）。

## 設計概要

對寫網頁的人：同源的 `<iframe>` 照 HTML 運作：插入時就有 `contentWindow` 和初始的 `about:blank`，照 `src`、`srcdoc` 載入（延後父 document 的 `load`），畫在 iframe 的內容框裡，點擊和滾輪進到子 frame；跨源的 window 碰了就丟 `TypeError`。子 frame 裡的連結只有連到自己的 `#fragment` 會捲動；換一份 document 之後，舊的 `contentWindow` 不再是那個 frame 的（「和 Chromium 不同的地方」）。

對改 kun 的人：
- 物件交給 JS 時，它的 wrapper 在物件所屬的 realm 建立：節點屬於它的 document 的 realm。
- `setTimeout` 這類記在呼叫者的 window；丟棄一份 document 時只清掉它的 timer。
- 需要 frame 時從元素的 document 找，不用 `ScriptHost::frame()`。
- renderer 的每個 frame 是一個 `FrameView`，組成一棵樹；只有最上層的 frame 做 pre-paint、paint 和 hit test，一次走過整棵樹。

## 詳細設計

各項的理由在「已決定的事項」；調查的背景見 `details.md`（第 1 節的現況、第 3 節的資料模型、第 4 節的 agent、第 5、6 節的 render、輸入和導覽、第 10 節給 DOM API 下一批的約束）。

第 1 階段「打底」：

- **每個 window 的 realm**（第 2、4 到 7 條）：
  - `dom` 的 document 記得它的 realm 的 window（`realm` edge）：有 window 的是自己的；`createHTMLDocument`、`createDocument` 用 `implementation` 的 document 的，`new Document()` 用執行中的 realm 的，template contents owner document 和複製出來的 document 用原本那份的。
  - `dom_js` 第一次替物件建 wrapper 時，沿著「物件 → document → realm window → global → creation context」找到 context，進入它再建。節點和它擁有的物件照這個規則；static `NodeList`、建構子建的物件、事件用目前的 context。
  - 每個 realm 的 V8 security token 是 document 的 origin（internalized 的字串，V8 比對 token 看的是同一個物件）；opaque 的 origin 每個 realm 一個獨一無二的 token。
- **timer 和 rAF**（第 8 到 10 條）：照舊集中在每個 heap 一份 `Timers`，每個記擁有者（window 的 document 的 `NodeId`）；`clearTimeout`、`cancelAnimationFrame` 只清得掉自己 window 的；renderer 執行時擁有者不是畫面上的 document 就丟掉。
- **從元素的 document 找 frame**（第 11 到 13 條）：`DomIsolate` 的一般狀態有一張 document 到 frame 的表，engine 建 realm 時登記；`<a>` 的換頁查元素的 document 的 frame。event handler 在元素的 realm 裡編譯，scope 是元素的 document。時鐘照舊用執行中的 realm 的 frame。
- **fully active**（第 14 到 16 條）：`Window` 記得是否還在 browsing context 裡；`discard_document` 把它設成 false，並清掉那份 document 的 timer、animation frame、frame，renderer 導覽時只呼叫它。event handler 的編譯和 `<a>` 的換頁看 fully active。
- **id**（第 17 到 19 條）：renderer 給每份 commit 的 document 一個 `DocumentId`（`compositor` 的型別），compositor 的 document、捲動和輸入用它；「哪次導覽」照舊用 `navigation_id`。協定不帶 frame 的 id（第 18 條；browser 沒有 frame tree 時用不到）。

第 2 階段 2a「看不見的 iframe」：

- **建立和移除**（第 21、25、26 條）：`dom` 在 iframe 變成 connected 或 disconnected 時記一筆，`dom_api` 在宣告表的每個呼叫結束時、renderer 在 parser 每一步之後取出，同步建或丟棄子 frame：初始的 `about:blank`（`<html>`、`<head>`、`<body>`）、它的 window 和 realm 在 script 執行中建好。wasm 文件裡的 iframe 沒有子 frame。
- **origin**（第 27 條）：document 自己記 origin；初始的 `about:blank` 和 `srcdoc` 繼承父 document 的；security token 和同源檢查都用它。
- **frame tree**（第 28、33 條）：renderer 的每個 frame 是一個 `FrameView`；parser、script、子資源、`DOMContentLoaded`、`load` 對每個 frame 都做，照 `DocumentId` 和 request id 找到 frame；timer、animation frame 照 document 找到它的 frame 和 realm。
- **成員**（第 29、40 條）：`contentWindow`、`contentDocument`（跨源時 `null`）；`Window` 的 `frames`、`length`、`window[i]`、`parent`、`top`、`frameElement`；`document.defaultView`。
- **載入**（第 22、30 到 32、34 到 36 條）：插入時、和之後每次設定 `src` 或 `srcdoc` 時導覽；`about:blank` 在插入時同步分派 `load`；`src` 由 renderer 當成子資源抓（`RequestDestination::Iframe`），回應照主 frame 的方式 commit 成新的 document 和 realm（錯誤時是一份空的 document）；載入中的子 frame 延後父 document 的 `load`，子 frame 的 `load` 之後在 iframe 上分派 `load`；新的導覽取代舊的，移除 iframe 時一起拿掉；script 語言和父 document 不同的子 document 照樣 commit、兩種 script 都不執行。
- **事件**（第 38、39 條）：target 的 global 不是 fully active 時不呼叫 listener（`new EventTarget()` 記著建立它的 window）；callback 的例外在 callback 的 global 回報。

第 2 階段 2b「畫面和輸入」：

- **iframe 是 replaced element**（第 43 條）：沒有 natural size 時 300×150、沒有 natural ratio；`width`、`height`、`frameborder` 是 presentational hint；UA stylesheet 的 `border: 2px inset`（畫成 `solid`）。
- **每份 document 一個 `StyleEngine`**（第 45 條）：放在 `FrameView`，UA stylesheet 各 frame 共用；frame 看自己 document 的 style flag 決定要不要重算。
- **子 frame 的 lifecycle**（第 44 條）：父 frame 的 style、layout 之後，每個子 frame 以 iframe 的內容框（沒有 box 時 0×0）為 viewport 跑 style 和 layout，一路往下；stylesheet 還在載入的子 frame 跳過。
- **paint**（第 46、48 到 50 條）：只有最上層的 frame pre-paint 和 paint，一次走過整棵 frame tree，走到 iframe 時在它的內容框底下（clip 和平移，減掉子 frame viewport 的捲動位移）走進子 frame 的 fragment；子 frame 畫在 iframe 的 line phase，一棵 layer tree。子 frame 的 layout、捲動或子 frame 本身變了，整棵一起重新 paint（沒變的 box 照舊從快取拿）。被跳過的子 frame 照上一次的 layout 畫。
- **自動化**（第 47 條）：`RendererMsg::QueryElement` 的 `frames` 和情境的 `frame` 指令，查子 frame 裡的元素，位置是視窗的座標。
- **輸入**（第 55 到 58 條，2b-3）：hit test 用最上層的 pre-paint 一次走過整棵 frame tree，事件在目標所在 frame 的 realm 裡分派，`clientX` / `clientY` 是那個 frame 的 viewport 的座標；子 frame 的 `#fragment` 連結捲動子 frame。子 frame 的 viewport 是 iframe 裡的 scroll node（element 是它的 document），子 frame 裡的捲動容器也是，每個 frame 管自己的捲動位置，滾輪和 compositor 交回的捲動照 scroll node 的 frame 和 element 分給各 frame，捲到底往外傳；iframe 的 `scrolling` 和子 frame 的 `overflow` 決定使用者能不能捲。自動化的 `ScrollIntoViewIfNeeded` 從元素的 frame 往外一層層捲，情境在子 frame 裡也能 `click`、`wheel`、`scroll-to`。

### 測試

- 第 1 階段的行為在 `dom`、`dom_api`、`dom_js` 的測試裡測：在同一個 DOM 上建第二個 window 和 realm（`ScriptEngine::create_realm` 本來就是每份 document 一個）。
- 2a：`dom_api` 的 `frames`、`dom_js` 的 `iframes`，情境 `iframes` 和 `iframe-loads`；WPT 裡等 iframe 的測試放回來（第 37 條）。
- 2b：`style`、`layout`、`paint`（`child_frames.rs`）的單元測試，`browser` 的 `query_element` 測試（子 frame 的位置、點擊、滾輪、捲進畫面），reftest `frames/`，WPT 的 iframe reftest（第 51、52 條），情境 `frames`（`frame` 指令，第 47 條）。
- 每一段跑 mutation test；驗收和檢查的紀錄在 `progress.md`。

### 分階段

每段開始前只決定那一段要決定的事；各段的日期、驗收和檢查在 `progress.md`。

- **第 1 階段「打底」**（2026-10-06 完成）：1a 每個 window 的 realm；1b timer 和 rAF 記得自己的 window；1c 從元素的 document 找 frame；1d `Window` 的 fully active 狀態；1e document 的 id；1f mutation test 的補齊和文件。
- **第 2 階段：同源、同 process 的 iframe**：
  - **2a「看不見的 iframe」**（第 20 到 24 條，2026-10-06 完成）：2a-1 同步建子 frame、frame tree、`contentWindow` 等成員；2a-2 `src`、`srcdoc` 的載入、`load` 的順序、移除、跨源；2a-3 放回等 iframe 的 WPT 測試、mutation test、文件。
  - **2b「畫面和輸入」**（照 RFC 0012 路線圖的 E，第 42、53、54 條）：2b-1 子 frame 的 style 和 layout（2026-10-07 完成：a 每份 document 一個 `StyleEngine`、b iframe 是 replaced element、c 子 frame 的 lifecycle 和情境的 `frame` 指令）；2b-2 子 frame paint 進父 frame（2026-10-07 完成：a 一次走過整棵 frame tree、b reftest 和 WPT）；2b-3 輸入（2026-10-08 完成：a hit test 和點擊、b 捲動、c 情境裡的點擊和捲動）。
- **第 3 階段**（子 frame 的導覽和歷史經過 browser、`WindowProxy` 的身分）：第 53 條排進這份，第 59 條移到「之後的可能」。

## 和 Chromium 不同的地方（之後要改成一樣）

為了分段做，kun 有幾處刻意和 Chromium 不同。每一項寫明 Chromium 的做法、之後在哪裡改；仍然成立的都寫在 `docs/limitations.md`。

| 項目 | kun | Chromium | 之後在哪裡改 |
| --- | --- | --- | --- |
| `contentWindow` 的身分（第 23 條） | 子 frame 每換一份 document 就是新的 window 和 realm，之前拿著的 `contentWindow` 是舊的 | script 看到的是 `WindowProxy`（外層的 global proxy），跨導覽保留；從初始的 `about:blank` 換到同源的 document 時，連內層的 `Window` 都沿用 | 「之後的可能」的 3b：每個 frame 一個 global proxy，導覽時舊的 context `DetachGlobal`、新的 context 帶同一個 global（要改 rusty_v8 的 C++、重編 V8） |
| iframe 的導覽由誰負責（第 22 條） | renderer 自己載入 `src`（一種子資源），browser 不知道子 frame；子 frame 沒有歷史 | 每次導覽都經過 browser 的 `NavigationRequest`（PlzNavigate），browser 有 frame tree（`FrameTreeNode`），歷史整棵樹一起記（`FrameNavigationEntry`） | 「之後的可能」的 3a（和路線圖的 B 一起設計 session history）：browser 端的 frame tree、每個 frame 的導覽、joint session history；site isolation 在 RFC 0004 |
| 子 frame 自己的導覽（第 20 條） | 子 frame 裡點連結、改 `location` 不支援；只有父 frame 改 `iframe.src` | 和主 frame 一樣，經過 browser | 「之後的可能」的 3a（`location` 要路線圖的 B） |
| 鍵盤捲動（第 56 條） | 鍵盤照舊捲最上層的 viewport | 捲有焦點的 frame（點過的子 frame） | 路線圖的 F（focus） |
| 子 frame 的 throttling（第 44 條） | 每個 main frame 都看每個子 frame 要不要更新 | 看不見的跨源 frame 不跑 lifecycle（同 process 裡 `display: none` 的照樣跑，和 kun 一樣） | 之後的最佳化 |
| 跨語言的 frame（`details.md` 第 3d 節、第 36 條） | wasm 文件裡的 iframe 沒有子 frame；JS 頁面的 iframe 載入的 wasm 文件照樣 commit，但兩種 script 都不執行 | 沒有對應（Chromium 只有 JS） | RFC 0004（第 53 條）：當成不能同步存取的遠端 frame，和跨 process 的 iframe 一起 |
| 子 frame 的錯誤頁（第 32 條） | 網路錯誤、被擋下時 commit 一份空的 document（opaque 的 origin） | commit 一份錯誤頁（`chrome-error://`，有錯誤的說明） | 「之後的可能」的 3a：子 frame 的導覽經過 browser 時，用主 frame 的錯誤頁（browser 的 `loader::error_page`） |
| frame 的 id（第 18 條） | 協定不帶 frame 的 id | 每個 frame 的訊息帶 `LocalFrameToken` / `FrameToken` | 「之後的可能」的 3a（browser 有 frame tree 時） |
| 跨網站的 iframe | 和同源的一樣在同一個 renderer，只靠 security token 擋住互相存取 | site isolation：跨網站的子 frame 在別的 process（`RemoteFrame`、`RemoteWindowProxy`），輸入由 browser 照 surface 分派 | RFC 0004（2026-10-06 延後） |
| 跨源的 window 能用的成員（第 29 條） | 跨源的 window 什麼都碰不了：security token 擋下所有存取（`TypeError`） | cross-origin 的 interceptor：規範列出的少數成員仍然可以用（`postMessage`、設定 `location`、`closed`、`frames`、`length`、`parent`、`top` 等），`frameElement` 是 `null` | `postMessage` 的 RFC（路線圖的 J，第 53 條） |
| 子 frame 的 V8 context 何時建（第 26 條） | 建子 frame 時就建 | 第一次用到時才建（`WindowProxy` 的 lazy 初始化） | 看不出差別，只影響記憶體和時間；需要時再改 |
| fully active 看父 frame（第 16 條） | 只看自己的 window；丟棄 document 時連它的 iframe 的 document 一起丟棄，所以結果一樣（2a-1） | 沿著 frame tree 往上看 | 結果相同，不改 |
| iframe 的 insertion steps 的時機（第 25 條） | 插入或移除的操作（宣告表的一次呼叫、parser 的一步）結束時才建、丟子 frame | 插入、移除的當下（`HTMLFrameOwnerElement`） | 現在看不出差別（中間不執行 script）；有 custom element 的 `connectedCallback` 或 mutation event 時，script 會在插入途中執行，到時要改成當下做 |
| 事件的 wrapper 的 realm（第 6 條） | 建在第一個碰到它的 realm | 呼叫 listener 時用 listener 的 context 建（和 kun 一樣不是規範的 relevant realm） | 已和 Blink 一致，不改 |
| 每個 global 的 time origin（第 13 條） | 所有 frame 共用 renderer 的時鐘，從 renderer 啟動算起 | 每個 global 從自己的導覽開始算 | RFC 0017 的 `Performance`（第 53 條） |

## 缺點

- 子 frame 由 renderer 自己載入，browser 不知道它們：子 frame 沒有歷史，子 frame 裡的換頁不支援，之後做 site isolation 時要改成經過 browser（「和 Chromium 不同的地方」）。
- 多了「找物件的 realm」這一步，每次建 wrapper 都要走（只在第一次建時，之後用 `WrapperSlot` 裡的）。
- 子 frame 不 throttle：每個 main frame 都看每個子 frame 要不要更新，任何一個子 frame 變了，整棵 frame tree 一起重新 paint（沒變的 box 從快取拿）。

## 理由和其他做法

- **先打底，不先做 iframe**（「已決定的事項」第 1 條）：打底的每一項不管 iframe 什麼時候做都要做，而且越早做，DOM API 的下一批越少地方要改；iframe 本身的工作量和 RFC 0008 差不多，牽涉 renderer、layout、paint、compositor，比較大的決定（`WindowProxy`、合成、導覽）可以等 DOM API 的下一批做完再看。
- **不打底、直接做 DOM API 的下一批**：最快看到 WPT 的進展，但錯的模式會被新的 API 照著用。不選。
- **整個做完 iframe 再做 DOM API**：WPT 的收益最大，但 DOM API 的下一批要等很久。不選。

## 先例

- **Chromium**：site isolation 之前，整棵 frame tree 在同一個 renderer；同站的 frame 共用一個 agent（事件迴圈、microtask queue）；script 看到的是 `WindowProxy`；建 wrapper 時目前的 context 必須是物件的 realm（`details.md` 的第 2 節，對照過原始碼）。kun 照這些做，`WindowProxy` 跨導覽的身分除外；子 frame 的 layout、paint 和 hit test 走過整棵 local frame tree，和 Blink 的 `LocalFrameView::UpdateAllLifecyclePhases`、`PrePaintTreeWalk` 一樣。

## 已決定的事項

1. **只排第 1 階段「打底」，iframe 本身之後再決定**（2026-10-06）：做 security token、wrapper 的 realm、timer 和 rAF 記得 window、從元素的 document 找 frame、fully active 的狀態、frame 和 document 的 id；接著做 DOM API 的下一批；iframe 的第 2、3 階段等那之後再評估。理由見「理由和其他做法」。不選「先做 DOM API 的下一批」和「先把 iframe 做完」。
2. **wrapper 建在物件的 realm**（2026-10-06；`details.md` 第 3c 節）：和 Blink 一樣，建 wrapper 時進入物件所屬的 realm 的 context。每條 binding 的路徑都經過它，越早做越少地方要改；沒有替代方案。
3. **分開 frame 的 id 和 document 的 id**（2026-10-06；`details.md` 第 1、6、7 節）：`navigation_id` 不再兼當 document 的 id；協定的訊息之後要能帶 frame 的 id，不假設 frame 一定在同一個 renderer（site isolation）。

4. **從物件找 realm，沿著現有的關係走**（2026-10-06，1a）：物件 → 它的 document → document 的 realm 的 window → window 的 wrapper（context 的 global）→ global 的 creation context，只在第一次建 wrapper 時走。和 Blink 從 document 的 execution context 找 `ScriptState` 一樣。不選「`dom_js` 存一張 document 到 `ScriptState` 的表」：多一份要和 window、document 的存活同步的狀態。
5. **document 記得它的 realm 的 window**（2026-10-06，1a）：`dom` 的 document 多一條指向 window 的 edge。有 window 的 document 在建 window 時設成那個 window；`createHTMLDocument`、`createDocument` 用 `this` 的 realm，`new Document()` 用執行中的 realm，template contents owner document 和複製出來的 document 用原本那份的。和 Blink 一樣（沒有 frame 的 document 用建立它的 document 的 execution context）；這條 edge 讓那個 window 活得和 document 一樣久，Blink 也是。不選「沒有 window 的 document 照舊用目前的 context」：規則會不一致。
6. **不是節點的物件的 realm**（2026-10-06，1a）：節點擁有的物件（集合、`NodeList`、`NamedNodeMap`、`DOMTokenList`、`DOMImplementation`）跟著擁有它的節點；建構子建的物件本來就 wrap 進 `new` 建的物件，不用改；事件照舊用目前的 context（Blink 在呼叫 listener 時用 listener 的 context 建事件的 wrapper）。不選「事件也記自己的 realm」：和 Blink 的行為不同，也很少被測到。
7. **security token 是 document 的 origin**（2026-10-06，1a）：建 realm 時用 document 網址的 origin 序列化後的字串，同源的 context 是同一個 token；opaque 的 origin 每個 realm 一個獨一無二的 token。跨源的互相存取被 V8 擋下（`TypeError`），和之後拆 process 的行為一致。不選「一個 renderer 的 context 都同一個 token」：跨源的 frame 也能互相存取。

8. **timer 和 rAF 照舊集中在每個 heap 一份 `Timers`，各自記擁有者**（2026-10-06，1b）：排程（誰先到期、event loop 的時間、巢狀的層數）是事件迴圈的工作，kun 一個 renderer 一條 task queue，集中在一處；每個 timer 和 rAF 多記它屬於哪個 window，決定在哪個 realm 執行、導覽時清掉誰的。Blink 的 timer 是每個 execution context 一份（`DOMTimerCoordinator`），排程交給 frame 的 task queue。不選「每個 `Window` 自己存一份」：callback 要進 GC 物件、找下一個到期的要走過所有 window，是比較大的 GC 改動，現在看不出好處。
9. **擁有者是 window 的 document 的 `NodeId`**（2026-10-06，1b）：window 和 document 一對一，`NodeId` 只增不減、不會被重用，不是 root，不會讓 window 多活。呼叫時的 `this`（window）決定擁有者。不選「記 `WindowHandle`」：handle 是 root，`Timers` 拿著就要保證 isolate 拆掉前清乾淨。
10. **`clearTimeout`、`cancelAnimationFrame` 只清得掉自己的**（2026-10-06，1b）：和規範一樣，每個 global 一份 map；handle 照舊整個 heap 一起編號、不重複（規範只要求同一個 global 裡不重複）。擁有者的 document 不在畫面上時，到期的 timer 和 rAF 丟掉不執行（規範是留著等 document 回到 fully active，那是給 back-forward cache 的，kun 沒有）；導覽換掉 document 時只清掉那份的。

11. **document 到 frame 的對照放在 `DomIsolate` 裡**（2026-10-06，1c）：`DomIsolate` 的一般狀態（不是 GC 物件）有一張 document 的 `NodeId` 到 frame 的表，`dom` 存成 `Rc<dyn Any>`（它不認得 `dom_api` 的 `FrameClient`），`dom_api` 轉回來；建 realm 時登記，導覽換掉 document 時拿掉。需要「節點所在的 frame」時從節點的 document 查這張表。對照只有一份，engine 無關，第 2 階段 renderer 的 frame tree 直接維護它；像 Blink 從 document 找 `LocalFrame`。不選「`ScriptHost` 從 window 找 frame」：對照分散在 JS 的 context slot 和 wasm 的 map，wasm 只答得出自己的 window，第 2 階段還會和 renderer 的 frame tree 成了兩份；也不選「`Window` 直接存 frame」：`Window` 被回收時 finalizer 會 drop renderer 的物件，違反「finalizer 只釋放記憶體」。
12. **event handler 在元素的 document 的 realm 裡編譯，scope 是元素的 document**（2026-10-06，1c）：照規範（handler 屬於元素的 node document 的 realm）和 Blink（用 event target 的 execution context 的 `ScriptState`）；沿用 1a 找物件的 realm 的方法。不選「只換 scope 的 document」：handler 函式會屬於呼叫者的 realm。
13. **時鐘照舊用執行中的 realm 的 frame**（2026-10-06，1c）：script 建立的事件的時間、`performance.now()` 要的只是時鐘，所有 frame 共用 renderer 的時鐘。每個 global 自己的 time origin 是已知限制。

14. **`Window` 記得它還在不在 browsing context 裡**（2026-10-06，1d）：`Window` 多一個不被 trace 的旗標，建 window 時是 true，document 被丟棄時改成 false；fully active 是「document 有 window，而且 window 還在 browsing context 裡」。對應 Blink 的 `Document::IsActive()`（`DocumentLifecycle`，`Shutdown` 之後是 `kStopping` / `kStopped`）；kun 的 window 和 document 一對一，放在 `Window` 等價。不選「frame 表裡有這份 document 就算」：測試建 realm 時多半沒有 frame，那些 document 會變成不是 fully active，而「沒有 frame 的 realm 不能跑 handler」也不自然。
15. **document 離開時，一個 `dom` 操作收尾**（2026-10-06，1d）：`discard_document` 把 window 標成不在 browsing context、清掉它的 timer 和 animation frame、從 frame 表拿掉；renderer 導覽時（第 2 階段移除 iframe 時也是）只呼叫它。對應 Blink 的 `Document::Shutdown()`；Blink 再發出 context 銷毀的通知讓各自停掉（`DOMTimer` 是 `ExecutionContextLifecycleObserver`），kun 東西少，直接在這個操作裡清。不選「renderer 分開呼叫」：每多一件收尾的事就多改一處，容易漏。
16. **只有現在用到 fully active 的兩處改看它**（2026-10-06，1d）：event handler 的編譯、`<a>` 的換頁；分派事件本身不檢查（規範和 Blink 都不擋）。timer 在丟棄時就清掉了，執行時不再檢查（Blink 的 timer 也是 context 銷毀時停掉）。有 iframe 時 fully active 還要看父 frame 的 document，第 2 階段再加。

17. **renderer 給每份 document 一個 `DocumentId`**（2026-10-06，1e）：renderer 的計數器，每 commit 一份 document 給一個新的（第 2 階段子 frame 的 document 也是），一個獨立的型別。對應 Blink 的 `DocumentToken`（Chromium 用不可猜的 token，跨 process 唯一；kun 目前只在一個 renderer 裡用，計數器就夠）。不選 document 的 `NodeId`：換 script 語言時 renderer 建新的 DOM，`NodeId` 從頭編，新舊 document 會撞號；也不選照舊借用 `navigation_id`：子 frame 的 document 不是 browser 導覽來的。
18. **frame 的 id 等第 2 階段再加**（2026-10-06，1e）：協定的訊息現在不帶 frame 的 id；有 frame tree 時再加，和 Chromium 的 `LocalFrameToken` / `FrameToken` 一樣跟著 frame tree 出現。不選「現在就加、只有主 frame 一個值」：沒人用、測不到的欄位。第 3 條的「協定之後要能帶 frame 的 id」照舊。
19. **「哪份 document」用 `DocumentId`，「哪次導覽」照舊用 `navigation_id`**（2026-10-06，1e）：compositor 記著畫的是哪份 document、compositor 交回的捲動、轉給 main thread 的輸入改用 `DocumentId`；丟掉過時的 commit，以及回報給 browser 的 `DidCommitNavigation`、`DidFinishLoad`、`ReftestWaitEnded`、`TestharnessResults` 照舊用 `navigation_id`（browser 比對的是自己發出的導覽，和 Chromium 用那次導覽的 mojo 介面或 navigation id 對上一樣）。Chromium 的 cc 不認得 document（Blink 換 document 時重設 view 和 layer、換 surface id），kun 的 compositor 記 document 是自己的設計，這裡只換 id 的來源。

20. **第 2 階段先做 2a「看不見的 iframe」**（2026-10-06）：範圍是 `<iframe>` 插入時建立子 frame、它的 window 和初始的 `about:blank`；照 `src`、`srcdoc` 載入（script 改 `iframe.src` 時再載入）；子 frame 的 `load` 在 iframe 元素上分派，並延後父 document 的 `load`；移除 iframe 時丟棄子 document；`contentWindow`、`contentDocument`、`frames`、`length`、`parent`、`top`、`frameElement`；跨源的 `src` 照樣載入，但互相存取被擋，`contentDocument` 是 `null`。不含：畫面、輸入、捲動（2b）；子 frame 自己的導覽和歷史、`WindowProxy`（第 3 階段）；wasm 文件裡的 iframe。理由：WPT 裡用 iframe 的測試大多只要「另一份有 window 的 document」；畫面那一塊最貴、收益相對少；Chromium 的 iframe 在 `display: none` 時也一樣有完整的 browsing context，只是不畫。不選「把第 2 階段整個做完」：畫面和輸入要先定子 frame 的合成等決定，DOM API 的下一批會延後很久。
21. **子 frame 在插入的當下同步建**（2026-10-06）：和規範、Blink（`HTMLFrameOwnerElement::InsertedInto` 同步建 `LocalFrame` 和初始的空白 document）一樣，`appendChild(iframe)` 之後馬上有 `contentDocument`。要在 `dom_api` 加第一個 HTML 的 insertion steps 的 hook，engine 要能在 script 執行中建 realm（V8 允許，Blink 也這樣做）。不選「像 `<img>` 那樣在 task 結束時掃 document 再建」：插入後馬上讀 `contentDocument` 會是 `null`，WPT 大部分 iframe 的測試會過不了。
22. **iframe 的 `src` 由 renderer 載入**（2026-10-06）：一種子資源（`RequestDestination` 多一種 iframe），照樣經過 browser 的 `net`，browser 對子資源的檢查也適用；回應在子 frame commit 成新的 document 和 realm。browser 不知道子 frame，子 frame 沒有歷史。使用者傾向和 Chromium 一樣（browser 的 frame tree 和 `NavigationRequest`），但那屬於第 3 階段，記在「和 Chromium 不同的地方」。
23. **子 frame 每換一份 document 就是新的 window**（2026-10-06）：沿用第 1 階段「window 和 document 一對一」的模型，`contentWindow` 回傳新的 window。使用者傾向和 Chromium 一樣（`WindowProxy`），但要改 rusty_v8 的 C++、重編 V8，屬於第 3 階段，記在「和 Chromium 不同的地方」。不選「只做規範的例外：初始的 `about:blank` 換到同源的 document 時沿用 window」：常見的情況不多，卻要打破 window 和 document 一對一、timer 和 frame 表以 document 為鍵、丟棄 document 三處的假設；第 3 階段的 `WindowProxy` 也會涵蓋它。
24. **2a 分三段**（2026-10-06）：2a-1 同步建子 frame 的機制和 `contentWindow` 等成員；2a-2 載入、`load` 的順序、移除、跨源；2a-3 WPT、mutation test、文件。2b（畫面和輸入）之後再排。

25. **`dom` 記下 iframe 的插入和移出，修改操作結束時處理**（2026-10-06，2a-1）：`dom` 的插入、移除在 HTML 的 iframe 變成 connected 或 disconnected 時記一筆（只在有 browsing context 的 document 裡：RFC 0010 的 1c-1 發現，fragment parser 建在一份沒有 window 的 document 裡，`innerHTML` 再把節點搬進頁面，原本那份 document 也記了插入，`dom_api` 處理時照 iframe 當下的位置多建了子 frame；HTML 的 insertion steps 本來就在根沒有 browsing context 時不做事）（不在 GC heap 上的佇列），`dom_api` 在宣告表的每個呼叫結束時取出、同步建或丟棄子 frame，renderer 在 parser 每一步之後取出；script 和 parser 兩條路都涵蓋。時機和 Blink 一樣（整個插入做完後，`DidNotifySubtreeInsertionsToDocument`）；機制不同：Blink 是元素的 virtual 方法。parser 那邊 Blink 在插入的當下建、kun 在 parser 每一步之後建，parser 遇到 script 會暫停、執行前先建好，所以看不出差別。不選「`dom_api` 的插入函式自己找 iframe，parser 那邊 renderer 事後掃」：兩條偵測的路，之後新增的插入方式容易漏。
26. **子 frame 的 document、window、realm 在 script 執行中同步建**（2026-10-06，2a-1）：`dom_api` 建初始的 `about:blank`（有 `<html>`、`<head>`、`<body>`），透過 `ScriptHost` 的新方法請 engine 在目前的 isolate 裡建 window 和 context、裝好 binding；子 frame 的 `FrameClient` 向父 document 的 frame 要（Blink 的 `LocalFrameClient::CreateFrame`），renderer 在 task 之後把它加進 frame tree。Blink 的 context 等第一次用到才建，kun 一開始就建（記在「和 Chromium 不同的地方」）。wasm 的 engine 不建，wasm 文件裡的 iframe 的 `contentDocument` 是 `null`。
27. **document 自己記 origin**（2026-10-06，2a-1）：`dom` 的 document 記一個 origin：renderer 照回應的網址設；初始的 `about:blank`（和 2a-2 的 `srcdoc`）繼承父 document 的；沒有設的照舊從網址算（測試用）。security token、`contentDocument` 的同源檢查都用它。和 Blink 的 `Document::GetSecurityOrigin` 一樣。不選「只有初始的 `about:blank` 直接沿用父 realm 的 token」：`srcdoc` 會遇到同樣的問題，規則散在各處。
28. **renderer 的每個 frame 都是一個 `FrameView`，組成一棵樹**（2026-10-06，2a-1）：子 frame 在 2a 不跑畫面的階段；timer、animation frame、callback 照 document 找到它的 frame 和 realm。和 Blink 每個 `LocalFrame` 一個 `LocalFrameView`（`display: none` 的 iframe 也有）一樣；kun 的 `FrameView` 本來就把 `LocalFrame`、`LocalFrameView`、`DocumentLoader` 合在一起。不選「子 frame 用一個比較小的結構」：載入、script、load event 那一套要再寫一份。
29. **`window` 這一側的成員**（2026-10-06，2a-1）：`HTMLIFrameElement` 的 `contentWindow`、`contentDocument`（跨源時是 `null`）；`Window` 的 `frames`（就是自己）、`length`、`window[i]`（global 上的 indexed interceptor，和集合一樣）、`parent`、`top`、`frameElement`。和 Blink 一樣，只差跨源的 window：kun 的 security token 擋下所有存取，Blink 允許規範列出的少數成員（記在「和 Chromium 不同的地方」）。
30. **`src`、`srcdoc` 照屬性的改變導覽**（2026-10-06，2a-2）：iframe 插入時照當下的屬性導覽一次（HTML 的「process the iframe attributes」）；之後每次設定 `src` 或 `srcdoc`（設成同一個值也算）都重新導覽。`dom_api` 在屬性改變時記一筆（和 event handler 的 `attribute_changed` 同一條路，`setAttribute` 和 parser 都經過），`update_frames` 在那次呼叫結束時請父 frame 導覽子 frame。和 Blink 的 `HTMLFrameElementBase::ParseAttribute`、插入時的 `OpenURL` 一樣。不選「像 `<img>` 那樣每段 script 之後掃 document、比對 `src`」：抓不到設成同一個值的重新導覽，規則也散在兩處。
31. **`about:blank` 的 iframe 在插入時同步分派 `load`**（2026-10-06，2a-2）：沒有 `src`、或 `src` 是 `about:blank`（也沒有 `srcdoc`）時不導覽，`update_frames` 建完子 frame 就在 iframe 上分派 `load`，對 script 來說是在 `appendChild` 之中。和規範、Blink 一樣；WPT 很多測試先設 `onload` 再插入。不選「排到下一個 task」。
32. **回應照主 frame 的方式 commit**（2026-10-06，2a-2）：解碼、照 MIME type 決定怎麼 parse、建新的 document 和 realm（renderer 在 task 裡用 `create_realm`），換掉子 frame 的 `FrameView`、丟棄舊的 document，iframe 指向新的 document（`contentWindow` 換成新的，第 23 條）。origin 照最後的網址（`data:` 是 opaque）；`srcdoc` 的網址是 `about:srcdoc`、繼承父 document 的 origin，不用抓，但照樣排成一個 task 再 commit（非同步，和規範、Blink 一樣）。HTTP 的錯誤狀態照樣 commit 回應的內容；網路錯誤、被擋下時 commit 一份空的錯誤 document（opaque 的 origin），`load` 照樣分派。Blink 經過 browser 的 `NavigationRequest`（第 22 條）、錯誤時 commit 錯誤頁（記在「和 Chromium 不同的地方」）。
33. **renderer 的載入對每個 frame 都能做**（2026-10-06，2a-2）：parser、parser-blocking 的 script、子資源、`DOMContentLoaded`、`load` 這幾條路（`advance_document`、`start_loads`、`did_receive_resource`、`wasm_compiled`、`fire_load_event`）改成照 `DocumentId` 在 frame tree 裡找 frame，子資源的回應照 request id 找到發出的 frame。和 Blink 每個 `LocalFrame` 有自己的 `DocumentLoader`、`ScriptRunner`、`ResourceFetcher`、共用 event loop 一樣。不選「子 frame 走簡化的路（一次 parse 完、不等 script）」：script 的順序和規範不同。
34. **`load` 的順序**（2026-10-06，2a-2）：載入中的子 frame 是父 frame 的一個 pending 項目；子 frame 的 window 的 `load` 分派完之後，在父 document 的 iframe 上分派 `load`（HTML 的 iframe load event steps），再拿掉那一項。父 document 的 `load` 等所有子 frame，`DOMContentLoaded` 不等；載入途中被移除的 iframe 不再拖住父 frame；父 document `load` 之後才插入的不影響它。`DidFinishLoad` 跟著主 frame 的 `load`。和 Blink 的 `CheckCompleted` 一樣。
35. **重新導覽取代舊的，移除時一起拿掉**（2026-10-06，2a-2）：新的導覽取代舊的（舊請求的回應用 request id 認出來丟掉），commit 時丟棄舊的 document；iframe 被移除時，子 frame 和它還沒回來的請求一起拿掉。和 Blink 取消舊的 `NavigationRequest`、detach 時取消載入一樣。
36. **子 document 的 script 語言和父 document 不同時兩種都不執行**（2026-10-06，2a-2）：JS 頁面的 iframe 載入 `<meta name="script" content="wasm">` 的文件時照樣 commit，但 JS 和 wasm 的 script 都不執行（跨語言的 frame 是第 3 階段，記在「和 Chromium 不同的地方」）。不選「照父 document 的 engine 跑」：wasm 文件裡的 JS 會執行，和它當頂層頁面時不同。
37. **skip 清單裡等 iframe 的測試全部放回來**（2026-10-06，2a-3）：它們現在都跑得完，失敗的子測試寫進 baseline，照 RFC 0008 的規則（只有拿不到任何結果的才 skip）。不選「只放回整個通過的」。
38. **target 的 global 不是 fully active 時不呼叫 listener**（2026-10-06，2a-3）：DOM 的 inner invoke（whatwg/dom#1084）：節點看它的 document 的 realm window，window 看自己，`new EventTarget()` 看建立它的 window（`EventTargetImpl` 多一條指向 window 的 edge，動到 GC）。和 Blink 的 `EventTarget::FireEventListeners`（target 的 execution context 被銷毀時不呼叫，`new EventTarget()` 也記著它的 execution context）一樣。不選「只做節點和 window」：`new EventTarget()` 的不一致要另外記成限制。
39. **callback 的例外在 callback 的 global 回報**（2026-10-06，2a-3）：WebIDL 的「invoke a callback function」在 callback 的 realm 回報；`V8Host::call` 用 function 的 creation context 的 window 分派 `error`，不是執行它的 realm 的。和 Blink 一樣。
40. **`document.defaultView`**（2026-10-06，2a-3）：document 的 window；沒有 browsing context 或不是 fully active 時是 `null`。和 Blink 的 `Document::defaultView` 一樣。WPT 的 `Element-matches` 等用 iframe 的 document 的 `defaultView` 拿那個 realm 的 interface。不選「留給 RFC 0010」：它是 document 到 window 的連結，屬於這份 RFC。
41. **iframe 和 window 的 WPT 目錄先不整個匯入**（2026-10-06，2a-3）：`html/semantics/embedded-content/the-iframe-element/` 和 `html/browsers/the-window-object/` 的測試只有一成通過（數字見 `progress.md`），其他大多卡在 `location`、`postMessage`、`window.open`、`history`；只匯入通過、而且測 2a 行為的檔案，整個目錄等那些 API 做了再匯入（「之後的可能」）。不選「整個匯入」：失敗和逾時的大多和 2a 無關，`cargo test` 也會慢不少。


42. **2b 先排 2b-1 和 2b-2 一起做**（2026-10-07）：RFC 0012 的路線圖把 2b 分成 2b-1（子 frame 的 style 和 layout，M1 要的）、2b-2（子 frame paint 進父 frame）和 2b-3（hit test、輸入、捲動）。這次排 2b-1 和 2b-2：子 frame 的內容畫出來，reftest 和情境才看得到它的 layout，「量得對」也要它。不選「只排 2b-1」：子 frame 的 layout 除了新的情境指令沒有別的方法看到，要等 D 的 `getBoundingClientRect`。2b-3 之後再排。
43. **iframe 是 replaced element**（2026-10-07，2b-1）：和 Blink 的 `LayoutIFrame`（`LayoutEmbeddedContent`）一樣：沒有 natural size 時用 300×150（CSS 的 default object size），沒有 natural ratio；`width`、`height` attribute 是 presentational hint（HTML 的「maps to the dimension property」，和 `<img>` 同一條路），`frameborder` 照 Chromium 的 `HTMLIFrameElement::CollectStyleForPresentationAttribute`（不是非零的整數時邊框寬度是 0，實作時補上）；iframe 的子節點不畫。UA stylesheet 照 Chrome 加上 `iframe { border: 2px inset }`：`border-style` 的白名單加上 `inset`，paint 先畫成 `solid`（`docs/limitations.md`），讓 iframe 的大小和 Chrome 一樣（外框 304×154、內容框 300×150）。不選「照 Chrome 畫兩色的 inset」：邊框樣式是路線圖 S2 的範圍；也不選「先不要邊框」：大小和 Chrome 不同，量測和版面都會差 4px。
44. **子 frame 的 lifecycle 在父 frame 的 `BeginMainFrame` 裡跑**（2026-10-07，2b-1）：父 frame 的 style、layout 做完，每個子 frame 以 iframe 的內容框（layout 的結果）為 viewport 跑 style 和 layout，再往下；子 frame 的 layout 不影響父 frame（iframe 的大小不看內容）。和 Blink 的 `LocalFrameView::UpdateAllLifecyclePhases` 經過 local frame tree 一樣。iframe 沒有 box（`display: none`，或在不畫的子樹裡）時，子 frame 的 viewport 是 0×0，照樣跑 style 和 layout：2b-1c 開始前對照過，Blink 不 throttle 同一個 process 裡 `display: none` 的 frame（throttle 的是跨 origin、或看不見的跨 origin frame），script 在裡面照樣拿得到 layout 的結果。原本寫的「沒有 box 時不跑 layout」和 Chrome 不同，改掉。子 frame 的 stylesheet 還在載入時（render-blocking），只有那個子 frame 跳過、等下一個 frame，父 frame 照常畫。跨 origin frame 的 throttling 是之後的最佳化。
45. **每份 document 一個 `StyleEngine`，看自己 document 的 style flag**（2026-10-07，2b-1）：和 Blink 一樣（`StyleEngine` 屬於 `Document`；要不要更新看 document 自己的 dirty bit，`Document::NeedsLayoutTreeUpdate`）。`StyleEngine` 放進 `FrameView`；UA stylesheet 由 `RenderThread` 解析一次、各 frame 共用（對應 Blink 的 `CSSDefaultStyleSheets`，但放在 renderer 裡，不是整個 process 的 `static`：`docs/architecture.md` 的「Process 的邊界」）。`FrameView` 改看自己 document 的 style flag（`dom` 已經把 flag 往上標到 document），heap 共用的 `dom_tree_version` 照舊只給集合的快取用。不選「只搬 `StyleEngine`」：任何一份 document 改了，每個 frame 照樣要走過自己整份 document 收集 `<style>`；也不選「一個 `StyleEngine`、快取照 document 分開」：和 Blink 的分層對不上，D 的 hook 之後要「只更新某份 document」。
46. **子 frame paint 進父 frame**（2026-10-07，2b-2；`details.md` 第 5 節的 A）：照 Chromium 同 process 的做法，子 frame 的 paint 結果和 property tree 接在父 frame 的 iframe 底下（transform 是 iframe 內容框的位置，clip 是內容框），整個分頁照舊一次 commit、一棵 layer tree。不選 B（子 frame 自己一棵 layer tree、Viz 用 surface 嵌入）：kun 的 `SurfaceLayer` 沒有 transform、一律畫在所有 picture layer 上面，蓋在 iframe 上的父 frame 內容會畫錯，每個 frame 還要一個 `LayerTreeHost`；B 留給跨 process 或跨語言的 frame。
47. **情境加 `frame` 指令**（2026-10-07，2b-1）：像 WebDriver 的「Switch To Frame」：`frame <selector>` 之後，查詢的 selector 在那個 iframe 的子 frame 的 document 裡找，回傳的位置換成視窗的座標；`frame top` 回到最上層。子 frame 的 layout 用它測，2b-3 點進子 frame 也用它；`RendererMsg::QueryElement` 多一個 `frames`（iframe 的 selector，一層一個）。實作時補上：情境沒有比對位置的指令，位置的精確值寫在 `browser` 的 `query_element` 測試裡（同一條 `QueryElement` 的路），情境測文字和 `wait-change`；在子 frame 裡 `click`、`wheel`、`scroll-to` 先報錯，到 2b-3。動到 automation 那一層，要跑 `test.py automation`。不選「只在 renderer 加 Rust 測試」：renderer 還沒有不開 thread 的測試工具，情境也看不到子 frame；也不選「等 D 的 `getBoundingClientRect`」：這段期間子 frame 的 layout 沒有測試守住。
48. **pre-paint 和 paint 從最上層一次走過整棵 frame tree**（2026-10-07，2b-2）：和 Blink 一樣（`PrePaintTreeWalk` 走進 `LayoutEmbeddedContent` 的子 frame，整棵 local frame tree 一個 paint controller）。`paint` 的 `pre_paint`、`paint` 多一個查詢：iframe 的子 frame 的 fragment 和捲動位置；走到 iframe 時，在它的內容框底下（平移的 transform 和內容框的 clip，第 46 條）走進子 frame。只有最上層的 `FrameView` 有 `PaintController` 和 `PrePaint`，子 frame 有變化時標最上層要 paint；2b-1c 子 frame 自己的 pre-paint 拿掉，查詢元素的位置改用最上層的 `PrePaint`，2b-3 的 hit test 也照這樣走進子 frame。`NodeId` 在同一個 DOM 裡不重複，`PaintController` 照元素的快取跨 frame 也不會撞。iframe 本身不是升成 layer 的理由，子 frame 的 chunk 照 `layerize` 的規則分；子 frame 的 document 沒有背景時透出父 frame（iframe 是透明的，和 Chrome 一樣；最上層的白色底照舊由 compositor 鋪）。不選「每個 frame 各自 paint，合成時接起來」：子 frame 的 property tree 要重新編號、chunk 要插進父 frame 的清單中間，查詢和 hit test 要處理兩層座標；Chromium 只有跨 process 的 frame（用 surface 嵌入）才分開。
49. **2b-2 的子 frame viewport 只有 clip 和捲動位移，ScrollNode 在 2b-3 加**（2026-10-07，2b-2）：子 frame 已經捲到的位置（例如網址的 `#fragment`）照樣畫出來，但使用者還捲不動子 frame，超出 iframe 的內容被裁掉。ScrollNode 只給 compositor 捲動和 hit test 用，2b-3 接上滾輪時和 hit test 一起加（子 frame 的 viewport 變成一般的捲動容器，和 Blink 一樣）。不選「2b-2 就加 ScrollNode」：2b-3 之前沒有輸入會送到它，行為測不到，還可能被 `composited_scrollers` 升成 layer。實作時補上：子 frame 的 document 裡的捲動容器（`overflow: auto` 等）也一樣，只有 clip 和捲動位移、沒有 ScrollNode：有的話 compositor 會在 hit test 找到它、自己捲，再把捲動交給 main thread，但 main thread 只認得最上層 document 的捲動容器，兩邊會不一致；2b-3 和 viewport 一起加。
50. **子 frame 被跳過 lifecycle 時畫上一次的結果**（2026-10-07，2b-2）：stylesheet 還在載入（render-blocking）的子 frame 跳過 lifecycle 時（第 44 條），照它上一次的 layout 畫；從來沒 layout 過就什麼都不畫，iframe 裡透出父 frame。規範在 rendering update 裡跳過 render-blocked 的 document，沒規定畫面；Blink 被跳過的 frame 沿用上次 paint 的結果。導覽換 document 時是新的 `FrameView`，不會畫出舊 document。不選「被跳過就不畫」：子 frame 的 script 加一個 stylesheet，iframe 就閃成空白。
51. **WPT 的 iframe reftest 只加用到的功能都有的**（2026-10-07，2b-2b）：試跑了和 iframe 畫面有關的 WPT reftest（數目見 `progress.md`），加進 `4ff8d6e:tests/wpt/reftest-include.txt` 的是 iframe 本身、現在就通過的（inline 裡的 iframe、transform 裡的 iframe、`scrolling` 屬性、`<base>` 等）；用到 kun 還沒有的 `position`、`aspect-ratio`、`rotate()`、`window.stop()` 的不加，和之前挑 WPT reftest 的規則一致（`tests/wpt/README.md`）。不選「全部加、失敗的寫進 expectations」：失敗的原因都不是 iframe，只會多跑、多維護。
52. **`body-margin` 系列加進清單，失敗的寫成預期的失敗**（2026-10-07，2b-2b）：`html/rendering/non-replaced-elements/the-page/body-margin-*` 裡，用 `style` 指定 margin 的現在就通過，其他的要 `<body>` 的 `marginwidth` 等屬性和 iframe 的 `marginwidth`、`marginheight` 的 presentational hint（HTML 的「The page」；Blink 在 body 插進 document 時把 frame owner 的 margin 抄成 body 的屬性），不在 2b 的範圍，寫進 `reftest-expectations.txt` 和 `docs/limitations.md`；做了之後 runner 會因為它們通過而失敗，不會忘記拿掉。不選「2b-2b 順便做」：要新的設計決定（跨 document 讀 frame owner 的屬性、改了屬性之後的 invalidation、quirks 模式），2b-2b 會變大；也不選「整系列都不加」：現在通過的那幾個也守住 style 的優先順序。
53. **RFC 0009 做到 2b-3、3a、3b 為止**（2026-10-07；3a、3b 在 2026-10-08 由第 59 條移到「之後的可能」）：2b 做完之後盤點剩下的工作，RFC 0009 以「同 process 的 iframe 完整可用」為完成：2b-3（輸入進子 frame）、3a（子 frame 的導覽和歷史經過 browser）、3b（`WindowProxy` 的身分）。依賴別的 RFC 的移出去，免得這份一直做不完：跨源 window 能用的成員到 `postMessage` 的 RFC（路線圖的 J；設定 `location` 要 3a），跨語言的 frame 和 site isolation 到 RFC 0004（都是遠端 frame 的模型），每個 global 的 time origin 到 RFC 0017 的 `Performance`；iframe 和 window 的 WPT 目錄照第 41 條等它們。3a 和路線圖的 B 共用 session history，兩者一起設計。不選「只做 2b-3，第 3 階段另開 RFC」：導覽和 `WindowProxy` 是 iframe 本身的行為，第 3 階段的設計前提都在這份；也不選「全部留在這份」：要等 J 和延後的 RFC 0004，短期內做不完。
54. **先做 2b-3**（2026-10-07）：延續 2b 的 frame tree 和 paint，獨立、風險低；3b 要重編 V8、動到 GC，單獨一段；3a 等和 B 一起設計。2b-3 不在 Speedometer M1 的關鍵路徑上（Speedometer 用 script 派送合成的事件），先做它會讓 CSS 線的 D 往後延，這是為了先把 iframe 做成完整的功能。
55. **hit test 一次走過整棵 frame tree**（2026-10-07，2b-3）：`paint::hit_test` 和 `element_bounds` 一樣，用最上層的 pre-paint（第 48 條）走到 iframe 時進入子 frame，結果直接是子 frame 的節點；renderer 照節點的 document 找到它的 frame，在那個 frame 的 realm 裡分派事件。`click` 的目標照 UI Events 是按下和放開處的共同祖先，分在兩個 frame 時沒有（和 Chrome 一樣不分派）；`clientX` / `clientY` 經過 iframe 的 transform 反算成子 frame 的 viewport 的座標。不選「照 Blink 的分層，停在 iframe 再交給子 frame 自己 hit test」（`MouseEventManager` 的 `PassMousePressEventToSubframe`）：kun 的子 frame 沒有自己的 pre-paint，要另外反算座標，結果相同。
56. **每個 frame 管自己的捲動位置**（2026-10-07，2b-3）：和 Blink 每個 frame 自己的 `ScrollableArea` 一樣。子 frame 的 viewport 和裡面的捲動容器有 ScrollNode（第 49 條；viewport 的 element 是子 frame 的 document），parent 是 iframe 外面那一層，滾輪捲到底往父 frame 傳；main thread 的滾輪沿著整棵 scroll tree 捲，每個 node 改它的 frame 的位置，compositor 交回的捲動也照 element 分給各 frame。`scrolling="no"` 的 iframe 使用者捲不動（Blink 的 `ScrollbarMode::kAlwaysOff`），script 和 `#fragment` 照樣能捲。鍵盤照舊捲最上層的 viewport，等路線圖的 F（Chrome 捲有焦點的 frame）。不選「最上層管整棵樹的捲動位置」：子 frame 的 `#fragment` 捲動和 layout 之後的 clamp 要跨 frame 改最上層的資料，和 Blink 的分層對不上。
57. **子 frame 裡的 `#fragment` 連結現在捲動，其他導覽等 3a**（2026-10-07，2b-3）：同一份文件內的導覽只在 renderer 裡（`FrameView::follow`），子 frame 的連結指到自己的 `#fragment` 時捲動子 frame；換頁和 `javascript:` 照第 53 條經過 browser，3a 做，在那之前照舊記一行 log。不選「全部等 3a」：子 frame 裡的目錄連結點了沒反應；也不選「renderer 先自己載入換頁」：3a 要改成經過 browser，等於寫兩次。
58. **情境在子 frame 裡點之前從裡到外捲**（2026-10-07，2b-3）：`RendererMsg::ScrollIntoViewIfNeeded` 也帶 `frames`：先在子 frame 裡把元素捲進它的 viewport，再在父 frame 把 iframe 捲進來，一直到最上層，和 Blink 的 `ScrollRectToVisible` 跨 frame 往上捲一樣；之後 `click`、`wheel` 和最上層一樣點元素的中心。不選「只點已經在畫面上的」：情境要自己先捲，和最上層的 `click` 不一致。
59. **RFC 0009 做到 2b 為止，第 3 階段移到「之後的可能」**（2026-10-08）：2b-3 做完之後，3a（子 frame 的導覽和歷史經過 browser）和 3b（`WindowProxy`）都不在 Speedometer 需要的範圍：它每個 suite 開一個新的 iframe、由頁面設定 `src`（2a 已經能做），載入之後才拿 `contentWindow`；suite 裡同一份文件內的導覽（`pushState`、`location.hash`）是路線圖 B 在 renderer 裡的部分，不需要 browser 端的 frame tree。所以不排進路線圖，留在這份的「之後的可能」，到時照流程開 RFC；3a 的 session history 和 B 一起設計的前提照舊（第 53 條）。不選「3a 併進 B、3b 當路線圖的新項目」：兩者都和路線圖的目標無關，放進去只會讓路線圖變長。GC 的檢查不再跑：動到 GC 的第 1 階段和 2a 做完時都跑過，2b 沒有動到 GC。

## 待決定

沒有：第 3 階段移到「之後的可能」（第 59 條）。

## 之後的可能

- **3a 子 frame 的導覽和歷史經過 browser**（第 53、59 條）：browser 端的 frame tree（`FrameTreeNode`）、每個 frame 的導覽（`NavigationRequest`）、協定帶 frame 的 id、子 frame 裡的連結和 `javascript:` 網址、子 frame 的錯誤頁、整棵樹的歷史（joint session history）；session history 和路線圖的 B（`location`、`history`）一起設計。site isolation（RFC 0004）也要走這條路。
- **3b `WindowProxy` 的身分**（第 23、53、59 條）：子 frame 換 document 時 `contentWindow` 還是同一個物件；要改 rusty_v8 的 C++、重編 V8，動到 GC。
- **匯入 iframe 和 window 的 WPT 目錄**（第 41 條）：`location`、`postMessage`、`window.open`、`history` 做了之後，整個匯入 `html/semantics/embedded-content/the-iframe-element/` 和 `html/browsers/the-window-object/`。
- **移到別的地方的**（第 52、53 條）：跨源 window 能用的成員（`postMessage` 的 RFC，路線圖的 J）、跨語言的 frame 和 site isolation（RFC 0004）、每個 global 的 time origin（RFC 0017 的 `Performance`）、`<body>` 和 iframe 的 margin 屬性（presentational hint 的雜項）、看不見的跨源 frame 的 throttling（之後的最佳化）、鍵盤捲動有焦點的 frame（路線圖的 F）。
- **shadow DOM**：路線圖的 K，`details.md` 第 9 節是評估。
