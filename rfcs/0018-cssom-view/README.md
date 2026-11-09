# RFC 0018：CSSOM View 和 script 觸發的 layout

- 開始日期：2026-10-08
- 狀態：✅ 已完成：2026-10-08（第 1 到 4 條；第 1 段第 5 到 14 條、第 2 段第 15 到 32 條、第 3 段第 33 到 40 條，每段做完時 merge 回 `main`。1a 到 3a 在 macOS 上做，3b、3c 在 Windows 上，3d 和完成時的驗收在 Linux 上；`progress.md`）
- 細節：`details.md`；進度：`progress.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 D，CSS 線的第三項（第一項 C 的第一段是 RFC 0016，第二項 E 是 RFC 0009 的 2b）。M1 的關鍵路徑（雜項線的 A，和 CSS 線的 E-1 加這一份）到這裡都做完了。

## 摘要

讓 script 讀得到 layout 的結果：`getBoundingClientRect`、`getClientRects` 和 `DOMRect`，`offset*`、`client*`、`scroll*`，`scrollTo` 這一類和 `scrollIntoView`，`scroll` 事件，`elementFromPoint` / `elementsFromPoint`，`innerWidth`、`devicePixelRatio`、`scrollX`。script 讀幾何之前，renderer 同步更新 style 和 layout（Blink 的 `Document::UpdateStyleAndLayout`），子 frame 的也是，先更新它的祖先；幾何照 Blink 從 layout 的結果換算。`elementFromPoint` 和輸入的 hit test 之前更新到 pre-paint（pre-paint 因此變成 lifecycle 自己的一個階段）。

最難的部分是所有權：script 執行時，renderer 的 lifecycle 狀態（`FrameView`、`TextContext`、視窗大小）在 `RenderThread` 裡、被借著，script 那一側碰不到（`details.md` 第 1 節）。做法（第 4 條）：renderer 呼叫 script 時，把 lifecycle 要的東西（frame tree、`TextContext`、視窗大小）和 DOM 一起借給 engine，engine 在 script 執行期間讓 `dom_api` 碰得到它，和現在借 DOM 的方式一樣；借用在編譯期檢查。

## 動機

**做之前（2026-10-08）**：
- script 讀不到任何幾何：沒有 `getBoundingClientRect`、`offsetWidth`、`innerWidth`；layout 只在 rendering update（`BeginMainFrame`）裡做。
- **Speedometer 的 runner 本身就要**：它在 iframe 的 `body.getBoundingClientRect()` 強制同步 layout，把「強制 layout 的時間」算進 async 的量測（RFC 0012 的 `details.md`）；suite 用到 `elementFromPoint`、`scrollTop`、`offsetHeight`。M1 要 `getBoundingClientRect` 和同步 layout，子 frame 裡的也要。
- **擋住別的工作**：RFC 0016 的第 2 段 `getComputedStyle` 用這份的 hook（它的第 1 條：X2）；WPT 的 `check-layout-th.js`（約 1100 個 `css/` 的檔案用它，flex、grid 的大宗）要 `offset*` 和 `getBoundingClientRect`；RFC 0009 有一個測試空缺等它（沒有 box 的 iframe 的子 frame 照樣 layout，`docs/testing.md`）。

目標：
- script 讀幾何時拿到的是最新的：先同步更新 style 和 layout，子 frame 先更新它的祖先。
- 照 CSSOM View 的規範和 Chromium 的行為提供成員，照 Chromium 的類別和 Blink 的目錄放進宣告表，JS 和 wasm 都有（RFC 0012 的第 7 條）。
- 同步的更新不執行 script、不改變 rendering update 的結果；只在 script 讀幾何時才做。

不在範圍內：
- `matchMedia`、`@media`：路線圖的 S4。
- `Range.getClientRects` 等：路線圖的 I。
- smooth scroll（`behavior: 'smooth'`）、`scrollend` 事件、`zoom`、writing mode、`position: fixed` / `sticky` 相關的行為：kun 還沒有這些功能（`docs/limitations.md`）。
- `screenX`、`screenY`、`outerWidth`、`moveTo` 這類視窗本身的位置：之後看需要。

## 設計概要

對寫網頁的人：`el.getBoundingClientRect()`、`el.offsetWidth`、`window.innerWidth` 等照規範和 Chrome 的行為回傳；前面剛改的 DOM、style 會先反映（強制同步 layout），子 frame 裡也一樣。`el.scrollTop = 100`、`window.scrollTo(0, 100)` 會捲動，下一個 frame 畫出來。

對改 kun 的人：
- `dom_api` 經過 `ScriptHost` 拿到 renderer 借出來的 lifecycle 介面，呼叫「把 lifecycle 更新到 layout」（和只到 style 的）和幾何查詢（Blink 的 `Document::UpdateStyleAndLayout` 一族、layout object 的幾何）。
- renderer 的每個 script 入口都要交出這個介面（`ScriptEngine` 的介面要求它，漏了就編譯不過）；呼叫 script 前先把 realm（`Rc<Realm>`）複製出來，不借著 frame tree。
- 新的 GC 型別：`DOMRectReadOnly`、`DOMRect`、`DOMRectList`（Blink 的 `core/geometry`）。

## 詳細設計

（各項的選項和比較在 `details.md` 第 3 節，理由在「已決定的事項」。）

- **hook**：`LayoutClient`（`dom` 定義、`dom_api` re-export，第 9 條）：把一份 document 的 style 和 layout 更新好、一個元素的 bounding rect、client rects 和盒子（`element_box`）、一份 document 的 viewport、寫捲動位置（`scroll_to`、`scroll_into_view`）、一點的 hit test（`element_at`、`elements_at`），都帶 `DomReadScope`（`ScriptHost::read()` 拿得到）。renderer 的實作是 `FrameTreeLayout`（`layout_client.rs`，借著 `RenderThread` 的 `frame_view`、`text`、`visual_properties`）：從主 frame 沿著 owner 鏈往下，每個 frame 只更新自己（第 6 條），更新前先把這次 script 剛建的子 frame 收進 frame tree（第 7 條），render-blocked 的 document 照樣更新（第 8 條）；只跑 dirty 的階段，和 rendering update 共用 `update_own_style_and_layout`。Blink 的 `Document::UpdateStyleAndLayout` 也是先更新 owner document（`LocalOwner()`）再更新自己。只到 style 的那個方法留給 RFC 0016 的第 2 段 `getComputedStyle`。
- **所有權**（第 4 條）：frame tree、`TextContext`、視窗大小照舊由 `RenderThread` 直接擁有；realm 是 `Rc<Realm>`，呼叫 script 前先複製出來。每個 script 入口（`with_host`、執行 script、microtask 的 checkpoint 等）除了 `&mut DomIsolate`，也交出一個 `&mut dyn` 的 lifecycle 介面；engine 在 `ScriptExecution` 期間把它和 `DomState` 一起放進 slot（`dom` 的 `DomStateSlot` 的做法），離開時拿掉；wasm 的 host 用同樣的方式拿到。lifecycle 進行中拿著 `&mut`，不可能同時借給 engine，等於 Blink 的 `ScriptForbiddenScope`。
- **幾何**（第 3 條）：從 layout 的 fragment 換算（Blink 的 `LocalToAbsolute`），經過 transform、捲動和子 frame 的內容框；強制 layout 只到 layout。
- **`DOMRect`**（第 1 條）：照 RFC 0016 的做法，每個 Blink 類別一個 GC 型別（`DOMRectReadOnly`、`DOMRect`、`DOMRectList`），可以 `new DOMRect(...)`、`DOMRect.fromRect`。
- **視窗**：`innerWidth`、`innerHeight` 是 viewport 的 CSS px（kun 沒有捲軸，等於 Chrome 的 overlay scrollbar）；子 frame 的是 iframe 的內容框。`devicePixelRatio` 是 `visual_properties` 的 device scale factor。
- **捲動**：讀 `scrollX` / `scrollTop` 先更新 layout（clamp 之後的值）；寫的話改那個 frame 的捲動位置（RFC 0009 的第 56 條：每個 frame 管自己的），下一個 commit 交給 compositor，compositor 還沒交回的捲動疊在新的位置上（第 16 條）。`dom_api` 只算要捲到哪裡，clamp 和「要重畫什麼」交給 renderer，和滾輪同一套範圍（第 24 條）；`behavior: 'smooth'` 一律當 instant（第 17 條）。
- **`scroll` 事件**（第 25 條）：捲動位置都經過 renderer 的 `ScrollState` 的設定函式，位置真的變了才記下要通知的目標（去重、照順序），`BeginMainFrame` 在 animation frame callback 之前分派（HTML 的「run the scroll steps」）：document 的 bubble 到 window，元素的不 bubble。
- **hit test**（第 27、30 條）：pre-paint 變成 lifecycle 自己的一個階段（Blink 的 `UpdateAllLifecyclePhasesExceptPaint`），結果留在 frame 上，paint 用它；`elementFromPoint` / `elementsFromPoint` 和輸入的 hit test 都先把 lifecycle 更新到 pre-paint，再用 `paint` 的同一個 hit test（`elements_at` 用不停下來的版本），所以 script 看到的和點擊到的一致。
- **`scrollIntoView`**（第 34、35 條）：`block` / `inline` 的四種對齊照 Blink 的 `ScrollAlignment` 和 `GetScrollOffsetToExpose`；從元素往外，每個捲動框和 viewport 都照同樣的對齊捲，再在父 frame 裡把 iframe 內容框裡的那個位置捲進來，一路到主 frame。`#fragment` 的捲動改用同一套（`block: start`、`inline: nearest`，Blink 的 `FragmentAnchor`）。

### 測試

- `dom_api` 的單元測試用測試的 `LayoutClient` 回答幾何和記下捲動的要求；`dom_js`、`dom_wasm` 測 binding 和新的型別（wasm 每種新物件一個 guest 測試，RFC 0008 的規則）。
- `layout` 和 `paint` 的測試：`layout` 算的 rect 和 pre-paint 的 `element_bounds` 在同一批頁面上一致（transform、捲動容器、跨行的 inline、flex），`box_transform` 和 scrollable overflow 對照手算的值。
- `shell` 的端對端測試（第 12 條）：小頁面經過 `testRunner` 把 script 讀到的值交回 Rust 比對——元素和 viewport、子 frame（自己的 viewport、剛插入的、沒有 box 的）、render-blocked 的 document、每一種執行 script 的入口、捲動和 `scroll` 事件、hit test、`scrollIntoView`。只有兄弟 frame 有沒有多跑一次 layout 看不出來，用 renderer 的單元測試守住（第 13 條）。
- 情境（視窗裡看得到的）：`geometry` 的滾輪、改視窗大小、`devicePixelRatio`、script 捲動和 `scroll` 事件、`elementFromPoint`；`about:demo` 的 getBoundingClientRect、scrollBy、elementsFromPoint、scrollIntoView 按鈕由 `demo-script` 檢查（第 32、39 條）。
- WPT：`css/geometry/` 的幾個檔案和整個 `css/cssom-view/`（含 reftest，第 14、31、36 條）；`check-layout-th.js` 解開的 `css/` 測試評估過，全過的 106 個收進清單（第 37 條，分類見 `details.md` 第 4 節）。
- mutation test 照舊；`unsafe` 的部分照 `crates/dom/soundness.md` 的做法驗證（Miri 不能跑 V8 的部分，靠論證和 debug 的檢查），完成時跑 `test.py matrix` 和 `fuzz`。

### 分階段

第 2 條；照這個順序做完（每一步的狀態和日期在 `progress.md`）：

1. **第 1 段（M1）**：所有權的改動和 hook；`getBoundingClientRect`、`getClientRects`、`DOMRectReadOnly` / `DOMRect` / `DOMRectList`；`innerWidth`、`innerHeight`、`devicePixelRatio`、`scrollX` / `scrollY`（和 `pageXOffset` / `pageYOffset`）。分成：
   - **1a** 借出去的路：realm 搬出 `FrameView`；`ScriptEngine` 的入口多交一個 `LayoutClient`，engine 在 script 執行期間放進 slot，`ScriptHost` 拿得到；renderer 先交一個什麼都不做的實作。行為不變。
   - **1b** renderer 的同步更新：`LayoutClient` 的實作，只更新自己和祖先（第 6 條）、先收進剛建的子 frame（第 7 條）、render-blocked 照樣更新（第 8 條）。
   - **1c** `layout` 的 `geometry`：從 fragment 換算 bounding rect 和 client rects（第 3、5 條），和 pre-paint 的 `element_bounds` 比對的測試。
   - **1d** DOM API：`DOMRectReadOnly`、`DOMRect`、`DOMRectList`（`dom`、`dom_api`、`dom_js`、`kun:dom`），`getBoundingClientRect`、`getClientRects`，`innerWidth`、`innerHeight`、`devicePixelRatio`、`scrollX`、`scrollY`、`pageXOffset`、`pageYOffset`。
   - **1e** 端對端的測試（renderer、browser、情境；RFC 0009 的測試空缺）、WPT、mutation test 的補齊、文件。
2. **第 2 段（M2）**：`offset*`、`offsetParent`、`client*`、`scroll*` 的讀；`scrollTo` / `scrollBy` / `scroll`、`scrollTop` / `scrollLeft` 的寫（main thread 的捲動和 compositor 的同步）；`scroll` 事件；`elementFromPoint` / `elementsFromPoint`。分成（第 15 條）：
   - **2a** 讀盒子的大小和捲動：`layout` 的 geometry 加上 offset、client、scroll 的量；`offsetParent`（kun 還沒有 `position`：最近的 `td`、`th`、`table` 或 `body`，RFC 0013 再擴充）。
   - **2b** 寫捲動位置：`scrollTop` / `scrollLeft` 的 setter、Element 和 Window 的 `scrollTo` / `scrollBy` / `scroll`；先更新 layout 再 clamp，和 compositor 的同步照第 16 條，`behavior` 照第 17 條。
   - **2c** `scroll` 事件：main thread 和 compositor 的捲動都記下要通知的目標，rendering update 裡分派（第 18 條）。
   - **2d** `elementFromPoint` / `elementsFromPoint`，照第 19、27 條分成兩步：
     - **2d-1** pre-paint 變成 lifecycle 的一個階段（Blink 的 `UpdateAllLifecyclePhasesExceptPaint`）：只有一份、存在 frame 上，paint 用它；輸入的 hit test（點擊、游標、滾輪、連結）也先更新到 pre-paint 再做，不再用上次 paint 的那份。
     - **2d-2** `elementFromPoint` / `elementsFromPoint`：先更新到 pre-paint，用同一個 hit test（`paint` 加上列出全部命中的版本）。
   - **2e** 端對端的測試、WPT、mutation test、文件。
3. **第 3 段**：`scrollIntoView`；匯入 `css/cssom-view/` 的 WPT；評估 `check-layout-th.js` 解開的 `css/` 測試。分成（第 33 條）：
   - **3a** `scrollIntoView`：`block` 和 `inline` 的四種對齊（第 34 條），沿著 frame 往上捲（第 35 條）；`#fragment` 改用同一套對齊。
   - **3b** 匯入整個 `css/cssom-view/`，含 reftest（第 36 條）。
   - **3c** 評估 `check-layout-th.js`：跑過 `css/` 裡用它的檔案，失敗的原因分類記進 `details.md`，全過的收進清單（第 37 條）。
   - **3d** `scrollIntoView` 的視窗示範（`about:demo` 和 `demo-script`，第 39 條）、mutation test、RFC 完成時的文件和 `matrix`、`fuzz`。

## 缺點

- `ScriptEngine` 的介面和 `render_thread.rs` 的每個 script 入口都要改（`dom_js`、`dom_wasm`、測試的 engine 跟著改）；slot 裡多一個指標，多一處 `unsafe`（論證和 `DomStateSlot` 一樣，寫進 `crates/dom/soundness.md`）。
- 座標換算有兩份：script 的幾何從 layout 的 fragment 算（第 3 條），畫面和 hit test 用 pre-paint 的 property tree，要靠比對的測試保持一致。
- 新的 GC 型別和 WIT 的成員，維護量和 RFC 0016 差不多。

## 理由和其他做法

`details.md` 第 3 節有完整的比較。要點：所有權的五種做法裡，A4（和 DOM 一起借給 engine，第 4 條）不用 `Rc<RefCell>`，借用在編譯期檢查，`unsafe` 的論證沿用現有的；幾何照 Chrome 從 layout 算（B1，第 3 條），強制 layout 不多做 pre-paint。

## 先例

- **Chromium**：`Element::getBoundingClientRect` 先 `Document::UpdateStyleAndLayoutForNode`，再用 layout object 的 `AbsoluteQuads`；`Document::UpdateStyleAndLayout` 先更新 owner document；`DOMRect` 等在 `core/geometry`；`LocalDOMWindow::innerWidth`、`scrollX` 也先更新 layout。kun 照這些做。
- **Firefox**：`FlushPendingNotifications(FlushType::Layout)`，同樣是讀幾何時同步 flush。

## 已決定的事項

2026-10-08 確認：

1. **`DOMRect` 每個 Blink 類別一個 GC 型別**（C1）：`DOMRectReadOnly`、`DOMRect`、`DOMRectList`（Blink 的 `core/geometry`），照 RFC 0016 的做法（它的 Z1）：`dom` 的 GC 型別、`dom_api` 的 `Object` 變體和宣告表的 interface，JS 和 wasm 都有；`new DOMRect()`、`DOMRect.fromRect`、setter、`instanceof` 照規範。不選「中性的 record，各 front end 自己建物件」（`rfcs/pre-rfcs/P3.md` 提過）：宣告表沒有這種機制，`DOMRect` 又是可以建構、可以改的 interface，要另外處理。
2. **分三段**：第 1 段（M1）所有權的改動和 hook、`getBoundingClientRect`、`getClientRects`、`DOMRect` 系列、`innerWidth` / `innerHeight`、`devicePixelRatio`、`scrollX` / `scrollY`；第 2 段（M2）`offset*`、`client*`、`scroll*` 的讀、`scrollTo` / `scrollTop` 的寫、`elementFromPoint`；第 3 段 `scrollIntoView`、匯入 `css/cssom-view/` 的 WPT、評估 `check-layout-th.js` 解開的 `css/` 測試。每段開始前只決定那一段的事。
3. **幾何從 layout 的 fragment 換算**（B1）：照 Blink 的 `LayoutObject::AbsoluteQuads` / `LocalToAbsolute`，沿著 fragment tree 加 offset、套 transform、減捲動、進子 frame 的內容框；強制 layout 只到 layout，和 Chrome 一樣不做 pre-paint。座標換算因此有兩份（這裡和 pre-paint 的 property tree），transform 的解析等從 `paint` 拿出來共用，再用測試比對同一批頁面（transform、捲動容器、巢狀子 frame）兩者的結果一樣。不選「同步的路跑到 pre-paint，用現成的 `element_bounds`」（B2）：kun 的 pre-paint 每次從頭做整棵 frame tree，每次強制 layout 都多做一次，而 Speedometer 量的正是強制 layout 的時間，會比 Chrome 系統性地多。`elementFromPoint` 的 hit test 要不要 pre-paint，第 2 段決定。
4. **lifecycle 和 DOM 一起借給 engine**（A4）：frame tree、`TextContext`、視窗大小照舊由 `RenderThread` 直接擁有；每個 script 入口交出 `&mut DomIsolate` 時，也交出一個 `&mut dyn` 的 lifecycle 介面，engine 在 script 執行期間放進 slot（和 `DomStateSlot` 一樣），`dom_api` 經過 `ScriptHost` 拿到；呼叫 script 時不借著 frame tree：realm 先複製出來（實作時改成 realm 留在 `FrameView` 裡、包成 `Rc<Realm>` 只讀地共用，不另外維護一張照 `DocumentId` 存、要和 frame tree 同步的表；Blink 的 `ScriptState` 也屬於 window，不屬於 view）。和 Chrome 的差別：Blink 的 document 隨時經過 GC 指標碰得到 `LocalFrameView`，kun 只在 script 執行中碰得到，而 DOM API 本來就只在 script 執行中被呼叫，所以從 `dom_api` 看一樣；lifecycle 進行中不執行 script（`ScriptForbiddenScope`）在這裡是型別保證。不選 A2（`Rc<RefCell<…>>` 共用整棵 frame tree）：借用變成執行期檢查，違反時 panic；不選 A1（只拆出 lifecycle 的一半共用）：要另外組一棵樹、載入的程式也要經過 `RefCell`；不選 A3（呼叫 script 前把狀態搬出去）：漏了的入口安靜地錯；不選 A5（frame tree 放進 DOM 這一側，最像 Blink 的所有權）：`RenderThread` 碰 frame 都要經過 DOM，改動比 A4 大很多。
5. **借出去的是高階的介面，幾何在 `layout` 算**（2026-10-08，第 1 段）：`dom_api` 定義一個 trait（暫名 `LayoutClient`）：把一份 document 的 style 和 layout 更新好、一個元素的 bounding rect 和 client rects、一份 document 的 viewport（大小、device scale factor、捲動位置）；renderer 實作。座標換算放在 `layout` 新的 `geometry` 模組（Blink 的 `LayoutObject::AbsoluteQuads`），transform 的解析從 `paint` 搬到 `layout` 共用；`dom_api` 只把結果包成 `DOMRect`。`getBoundingClientRect` 是相對於元素自己那份 document 的 viewport，換算不跨 frame。不選「交出 fragment tree 和捲動位置，`dom_api` 自己算」：`dom_api` 要依賴 `layout`，違反它和 renderer 無關的分層（`rfcs/pre-rfcs/P3.md`）。
6. **子 frame 只更新自己和祖先**（2026-10-08，第 1 段）：照 Blink 的 `Document::UpdateStyleAndLayout`（先更新 owner document，不碰兄弟 frame）：從主 frame 沿著 owner 鏈往下，每個 frame 只更新自己的 style 和 layout，再到下一個。`update_style_and_layout` 拆出「只更新這一個 frame」的部分，rendering update 照舊遞迴整棵樹。不選「整棵從主 frame 往下」：兄弟 frame 有變化時也做，強制 layout 的時間比 Chrome 多。
7. **hook 先把這次 script 剛建的子 frame 收進 frame tree**（2026-10-08，第 1 段）：子 document 和 realm 在插入時就同步建好了（RFC 0009 的第 26 條），`FrameView` 原本等 task 結束才收進 frame tree；hook 更新之前先收，和 Blink 插入時同步建好 `LocalFrameView` 一致，剛插入的 iframe 的 document 立刻有 layout（WPT 常這樣寫）。借出去的介面因此也要能建 frame（`DocumentId` 的計數器、送出子 frame 的導覽）。不選「當成還沒 layout，task 結束才有值」：和 Chrome 不同。
8. **render-blocked 的 document 照樣更新**（2026-10-08，第 1 段）：stylesheet 還在載入時，用已經載入的算；render-blocking 只擋 rendering update，不擋 script 強制的 layout，照 Chrome（實作前對照原始碼確認，結果寫在這裡）。不選「不更新，回傳上次的結果」：還沒畫過的 document 讀到全 0，和 Chrome 不同。
9. **`LayoutClient` 定義在 `dom`，借出的機制也在 `dom`**（2026-10-08，1a）：把借來的 `&mut` 存起來給 script 用要抹掉生命週期，`unsafe` 只能在 `dom`、`bindings`、`ipc`，存的地方又要叫得出 trait 的名字，所以 trait 在 `dom`（`dom_api` re-export）。`DomIsolate` 在 script 執行期間把指標放進 `DomState`、離開時放回原本的（可以巢狀），V8 和 wasm 都經過 `DomState` 拿到，`dom_js`、`dom_wasm` 沒有 `unsafe`；`dom_api` 從 mutation scope 同時拿到它和 read scope（兩者是不同的記憶體）。`dom` 多依賴 `gfx`（幾何的型別）。和 `DocumentFrames`、`Timers` 一樣是 `dom` 給 embedder 的 hook。不選「另開一個小 crate 放 trait」：`unsafe` 還是在 `dom`，只多一個 crate。
10. **會執行 script 的 `ScriptEngine` 方法都多一個 `&mut dyn LayoutClient` 參數**（2026-10-08，1a）：engine 實作時先借出再執行，少交就編譯不過（第 4 條）。不選「`ScriptEngine` 不改，renderer 在外面包一層」：漏包的入口安靜地拿不到 layout。
11. **`DOMRectReadOnly` 的 `toJSON()` 先不做**（2026-10-08，1d）：它回傳一般的物件，宣告表還不能回傳字典（`dom_js` 沒有那條轉換，WIT 也沒有 record）；`css/cssom-view/` 的 WPT 沒有用到，`css/geometry/` 只有一個檔案。記在 `crates/dom_api/tests/spec-missing.txt` 和 `docs/limitations.md`，匯入 `css/geometry/` 或有人要用時，和「回傳字典」一起做。不選「現在做」：1d 要先多做宣告表的一種回傳型別。
12. **端對端的測試用 testharness 的小頁面和情境**（2026-10-08，1e）：`shell` 的測試寫小頁面，用 `headless::run_testharness` 經過 `testRunner` 把讀到的值交回 Rust 比對（確定、快，iframe 和 render-blocked 都寫得出來）；要輸入或視窗的（滾輪之後的 `scrollY`、改視窗大小之後的 `innerWidth`、縮放的 `devicePixelRatio`）寫成情境。不選 reftest：依結果畫顏色，數字錯了看不出差多少；不選全部寫成情境：比 headless 慢，iframe 的載入只能靠等文字。
13. **第 6、7 條用 renderer 的單元測試守住**（2026-10-08，1e）：網頁看不出兄弟 frame 有沒有多跑一次 layout（結果一樣），所以在 `frame_view.rs` 的測試裡建一棵有兄弟子 frame 的 tree，強制更新其中一個之後，確認鏈上的 frame 不再 dirty、兄弟的還是 dirty（Blink 的 `LocalFrameView` 也有這類單元測試）。不選只靠審查：mutation test 抓不到「整棵更新」。
14. **這一段只匯入第 1 段成員的 WPT 檔案**（2026-10-08，1e）：`css/geometry/` 的 `DOMRect-*`、`DOMRectList`，`css/cssom-view/` 裡測 `getBoundingClientRect`、`getClientRects`、`DOMRectList` 和第 1 段的 `Window` 成員的檔案，一個一個列進 `4ff8d6e:tests/wpt/testharness-include.txt`，失敗的寫 baseline；整個 `css/cssom-view/` 照第 2 條留到第 3 段。不選現在加整個目錄：254 個檔案大多是第 2、3 段的成員，baseline 每段都要大改。
15. **第 2 段分成 2a 到 2e**（2026-10-08）：讀、寫、`scroll` 事件、`elementFromPoint`、測試和文件，每一步前只決定那一步的事（見「分階段」）。
16. **script 寫捲動位置時，compositor 還沒交回的捲動疊在新的位置上**（W1，2026-10-08，第 2 段）：main thread 立刻改那個 frame 的捲動位置（先更新 layout 再 clamp），下一個 commit 交出去當 base；compositor 還沒交回的部分照 `SyncedScrollOffset` 疊在上面、再 clamp。照 Chrome：commit 時 cc 的 `ScrollTree::PushScrollUpdatesFromMainThread` 經過 `SyncedProperty::PushMainToPending` 只換 base，不碰 impl 的 `active_delta_`；只有 main thread 打斷 compositor 上的捲動動畫時才丟掉 impl 的 delta（`clobber_active_value`，`ScrollTree::CollectScrollDeltas` 那時也不交 delta）。kun 的 compositor 已經是這樣，不用改；kun 沒有 compositor 的捲動動畫，clobber 和第 17 條的 smooth 一起留到之後。不選「script 的位置蓋過」（W2）：和 Chrome 不同，使用者正在捲時會跳一下。
17. **`behavior: 'smooth'` 這一段當 instant**（B1，2026-10-08，第 2 段）：`ScrollToOptions` 的 `behavior` 照規範轉換，但一律立刻捲到；和 Chrome 不同，記進 `docs/limitations.md`。Chrome 的 `ProgrammaticScrollAnimator` 用 cc 的 `ScrollOffsetAnimationCurve`，能的話在 compositor 上跑（`KeyframeModel` 的 `SCROLL_OFFSET`），不能時在 main thread 每個 frame 照曲線設定位置；動畫中來一個 instant 的捲動會取消它。smooth 的動畫和 clobber 列進「之後的可能」。不選 main thread 的動畫（B2）：main thread 忙的時候會卡住，Chrome 大多在 compositor 上跑；不選照 Chrome 在 compositor 上跑（B3）：範圍最大，可以是之後獨立的一段。
18. **`scroll` 事件在這一段做**（E2，2026-10-08，第 2 段）：照 HTML 的 rendering update 的「run the scroll steps」：main thread 和 compositor 交回的捲動都記下 pending scroll event target，下一次 rendering update 分派 `scroll`（document 的會 bubble 到 window，元素的不 bubble）。`scrollend` 不在這一段，列進「之後的可能」。不選留到之後：Speedometer 的部分 suite 和 `dom/events/scrolling` 的 WPT 用到，而它和這一段的捲動是同一條路。
19. **`elementFromPoint` 的 hit test 前同步更新到 pre-paint**（H1，2026-10-08，第 2 段）：照 Chrome（`LayoutView::HitTest` 先把 lifecycle 更新到 pre-paint；實作前對照原始碼確認），用輸入事件同一個 `paint::hit_test`，結果和點擊一致；點到 iframe 回傳 iframe 元素。kun 的 pre-paint 每次從頭做整棵 frame tree，但 `elementFromPoint` 不像 `getBoundingClientRect` 那麼頻繁。不選從 layout 的 fragment 另寫一份 hit test（H2）：transform、捲動、clip、z-order 都要再做一次，和點擊的是兩份。
20. **`LayoutClient` 用一個方法交出元素的盒子**（Q1，2026-10-08，2a）：`element_box(scope, element) -> Option<ElementBox>`：layout 的座標（不算 transform）的 border box、邊框、padding box、scrollable overflow、是不是捲動容器和它的捲動位置；幾何在 `layout` 的 geometry 算，`dom_api` 從它算出 `offset*`、`client*`、`scroll*`，和第 5 條一樣是高階的介面。不選每個成員一個方法（Q2）：trait 變得很大，每個實作都要跟著加。
21. **2a 加上 `document.scrollingElement`**（2026-10-08，2a）：根元素和 `body` 的 `client*`、`scroll*` 本來就要照它的規則（standards mode 是根元素，quirks mode 是 `body`），多一個屬性很小。
22. **`scrollTo(x, y)` 這類 overload 在 `glue.js` 轉成 options**（O1，2026-10-08，2b）：宣告表和 WIT 只有規範的 `scrollTo(options)`（`ScrollToOptions` 的 `left`、`top` 可以沒給）；JS 的 glue 把 `(x, y)` 轉成 `{ left: x, top: y }`，和 `addEventListener` 的布林值 options 同一個做法。Blink 在從 IDL 產生的 C++ binding 裡做 WebIDL 的 overload resolution（`scrollTo(x, y)` 建一個 `ScrollToOptions` 再走同一條路），kun 的宣告表是降階過的，沒有 overload，所以放在 JS 的 glue；結果一樣，只差在哪一層轉。讓宣告表認得 overload、由 `dom_js` 分派（較像 Blink）列進「之後的可能」。不選表裡兩個成員（O2）：`(x, y)` 的要另取表內的名字，WIT 會有規範沒有的 `scroll-to-xy`。
23. **`scrollTo` / `scrollBy` / `scroll` 回傳已經 resolve 的 promise**（R2，2026-10-08，2b）：宣告表的成員不回傳，`glue.js` 回傳 `Promise.resolve()`（規範的 `Promise<undefined>`）；第 17 條讓捲動都是 instant，回傳時已經捲完。和 Chrome 的差別：Chrome 的 `ScrollPromiseResolver` 在所有被影響的捲動結束後、在一個 DOM manipulation task 裡 resolve，值是 `ScrollResult`（`interrupted`）；kun 在 microtask 裡 resolve、值是 `undefined`，記進 `docs/limitations.md`。wasm 不回傳。不選照 Chrome 在 task 裡 resolve（R3）：宣告表要能回傳 promise，或 renderer 和 engine 之間多一條路；不選回傳 `undefined`（R1）：`.then()` 不能用。
24. **寫捲動位置時 renderer clamp**（S1，2026-10-08，2b）：`dom_api` 只算要捲到哪裡（沒給的軸保留原值、非有限的數當 0、`scrollBy` 的加法），`LayoutClient::scroll_to` 交給 renderer，renderer 用和滾輪、`#fragment` 同一個 `ScrollNode` 的範圍 clamp、標記要重畫或重做 pre-paint、要下一個 frame。照 Chrome 的分工：`Element::ScrollLayoutBoxTo` / `ScrollFrameTo` 算目標，`ScrollableArea::SetProgrammaticScrollOffset` → `ClampScrollOffset` clamp，和使用者的捲動同一套範圍。不選 `dom_api` 用 `element_box` 自己 clamp（S2）：viewport 的範圍規則要再寫一份，和滾輪的會分歧，寫的方法也省不掉。
25. **要發 `scroll` 的目標在捲動位置改變的地方記下**（C1，2026-10-08，2c）：renderer 的 `ScrollState` 每次改捲動位置（script、滾輪、compositor 交回的、layout 之後的 clamp、`#fragment`、自動化的 `scroll_into_view`）都經過同一個設定的地方，把目標（document 代表 viewport，或捲動容器）加進那個 frame 的待發清單，去重、照改變的順序；rendering update 在 animation frame callback 之前、照 frame tree 的順序分派（HTML 的「run the scroll steps」），document 的 bubble 到 window、元素的不 bubble。照 Blink：`ScrollableArea::ScrollOffsetChanged` 呼叫 `Document::EnqueueScrollEventForNode`，`ScriptedAnimationController` 的 per-frame 佇列同一個 target 只排一次，`ServiceScriptedAnimations` 在 rAF 之前分派。不選 rendering update 時比對位置（C2）：順序變成樹的順序，捲了又回到原位不發，和 Blink 不同。
26. **2c 加上 `onscroll`**（2026-10-08，2c）：`Element`（kun 的 `onclick` 等放在 `Element`）和 `Window` 的 IDL attribute 和 content attribute，`<body onscroll>` 是 window 的（和 `onload` 一樣），照現有的 `handlers` 模組。
27. **pre-paint 變成 lifecycle 的階段，script 和輸入的 hit test 都先更新到它**（P3，2026-10-08，2d）：照 Chrome：pre-paint 的結果（property tree，Blink 存在 `LayoutObject` 的 `FragmentData`）只有一份、留在 frame 上，paint 用它；`elementFromPoint` 經過 `LayoutView::HitTest` → `UpdateAllLifecyclePhasesExceptPaint`，輸入（`EventHandler`）也走同一個 `LayoutView::HitTest`，lifecycle 乾淨時幾乎不做事。kun 原本 paint 和 pre-paint 綁在一起，輸入照上次畫的 hit test（記在 `docs/limitations.md`）；2b 修的 bug（新的 fragment 配上一次的 pre-paint）就是兩份不一致造成的。分成 2d-1（拆開 paint 和 pre-paint、輸入先更新）和 2d-2（`elementFromPoint` / `elementsFromPoint`）。不選每次呼叫重做、不存（P1）：結果和 Chrome 一樣，但 kun 會有兩份 pre-paint，輸入照舊用上次畫的；不選快取（P2）：key 要包含整棵 frame tree 的 fragment 和捲動位置，容易漏。
28. **pre-paint 用 dirty 旗標判斷過期**（K2，2026-10-08，2d-1）：照 Chrome（`DocumentLifecycle` 到 `kPrePaintClean` 就不重做，改變的地方各自 `SetNeedsPaintPropertyUpdate`）：layout 換了 fragment、捲動位置變了（2c 已經都經過 `ScrollState` 的設定函式）、子 frame 來去或換了、device scale factor 變了，各自標記；frame tree 裡有一個標了，就從主 frame 整個重做（kun 的 pre-paint 不是增量的）。不選比對輸入（K1）：不會漏，但每次 hit test 前都要比對，和 Chrome 的結構不同。
29. **拿掉 hit test 之後的「節點還在這份 document」檢查**（2026-10-08，2d-1）：它是為了「上次 paint 之後節點被搬到別的 document」（`FrameView::is_ours`）；hit test 一律先更新之後走不到了，Chrome 也沒有這個檢查。`shell` 的 `a_click_skips_a_node_moved_to_another_document` 照樣保留，確認點擊送到對的地方。不選留著當防禦：走不到的死碼。
30. **`LayoutClient` 用兩個方法交出 hit test 的結果**（E2，2026-10-08，2d-2）：`element_at`（最上面的那個，找到就停）和 `elements_at`（穿透、照 paint 的順序由上到下列出全部，每個一次），對應 Blink 的一般 hit test 和 `kListBased | kPenetratingList`；renderer 先把 lifecycle 更新到 pre-paint（第 27 條），點在那個 frame 的 viewport 之外是沒有。`dom_api` 照 Blink 的 `TreeScope::ElementForHitTest` 把命中的節點換成這份 document 的元素（子 frame 裡的換成 iframe 元素），`elementsFromPoint` 去掉重複、最後接上根元素（`ElementsFromHitTestResult`）。不選一個列出全部的方法（E1）：`elementFromPoint` 每次都要走完整個頁面。
31. **第 2 段的 WPT：`css/cssom-view/` 裡測第 2 段成員的 48 個檔案**（A，2026-10-08，2e）：和 1e 一樣，收測 `offset*`、`client*`、`scroll*`、`scrollTo` 這一類、`scrollingElement`、`elementFromPoint` / `elementsFromPoint` 的檔案，不收另外需要 kun 沒有的東西的（`position`、testdriver、`scrollIntoView`、`scrollend`、`writing-mode`、`zoom`、`visualViewport`……）；全部跑、結果記進 baseline（smooth scroll 的照第 17 條失敗），跑不出結果的才 skip。整個目錄照第 2 條留到第 3 段。Chrome 也是整個 WPT 都跑、失敗的記在 expectations。不選現在收整個目錄（B）：把第 3 段的事提前，skip 和 baseline 大多是別的 RFC 的功能；不選只收大多會過的（C）：看不到失敗在哪，之後補功能時少了回歸的訊號。
32. **`elementFromPoint` 寫進情境和 `about:demo`**（2026-10-08，2e）：`geometry` 情境在滾輪捲動（compositor 捲的）之後點一個呼叫 `elementFromPoint` 的元素，頁面顯示點到的 id，確認 main thread 用的是 compositor 交回的捲動位置；`about:demo` 加一個按鈕。shell 的測試只有 script 自己的捲動。
33. **第 3 段分成 3a 到 3d**（2026-10-08，第 3 段）：`scrollIntoView`、匯入 `css/cssom-view/`、評估 `check-layout-th.js`、測試和文件（見「分階段」）。
34. **`scrollIntoView` 有完整的 `block` / `inline`**（S1，2026-10-08，3a）：`start`、`center`、`end`、`nearest` 照 Blink 的 `ScrollAlignment` 和 `GetScrollOffsetToExpose`（`nearest`：整個看得到時不捲，否則對齊較近的邊，比捲動框大時反過來）；boolean 參數照規範是 `start` 或 `end`、`inline` 是 `nearest`；`behavior` 照第 17 條是 instant。renderer 現有只對齊頂端的 `align` 換成一般的對齊，`#fragment` 照 Blink 的 `FragmentAnchor` 用 `block: start`、`inline: nearest`。不選只做 `start`（S2）：預設的 `inline` 就是 `nearest`，和 Chrome 的結果常常不同，WPT 的 49 個檔案大多測這些組合。
35. **`scrollIntoView` 沿著 frame 往上捲**（X1，2026-10-08，3a）：照 Chrome，先捲元素自己 frame 裡的容器和 viewport，再在父 frame 裡把 iframe 裡的那個位置捲進來，一路到主 frame（情境的 `scroll_into_view_if_needed` 已經這樣走）。不選只捲自己的 frame（X2）：iframe 裡的元素可能還在畫面外。
36. **匯入整個 `css/cssom-view/`，含 reftest**（W1，2026-10-08，3b）：照第 2 條；不再逐個列檔案，跑不出結果的（testdriver、逾時不穩的）放進 skip，其他的記 baseline，reftest 失敗的記在 expectations。不選只加 `scrollIntoView` 的檔案（W2）：和第 2 條不同，整個目錄又要再等一次。
37. **`check-layout-th.js` 評估並收全過的**（C2，2026-10-08，3c）：跑過 `css/` 裡用它的約 1100 個檔案，失敗的原因分類記進 `details.md`，全過的收進清單當回歸測試。`document.fonts.ready`（試跑 flex 和 grid 的 697 個檔案時擋住約三成）和 layout 的差異記在「之後的可能」，不在這份 RFC 補。不選只評估不收（C1）：會過的沒有測試守著；不選全部收、記 baseline（C3）：上千個 baseline，每次改 layout 都要更新很多；不選順便做最小的 `document.fonts`（C4）：CSS Font Loading 的 API，超出這份 RFC。
38. **Dictionary 的 WebIDL 預設值交給 guest 的 SDK**（L1，2026-10-08，3a）：WIT 的 record 沒有預設值，Lua 的 SDK 原本把沒寫的欄位當成 `false`、0、空字串和 enum 的第一個值；`ScrollIntoViewOptions` 的 `inline` 預設是 `nearest`，不是第一個值。宣告表的 `DictionaryDesc` 多一個 `defaults`（巨集從成員的預設值算出），WIT 在預設值不是上面那些值的欄位上加註解（`/// WebIDL's default: "nearest".`），Lua 的 generator 照它先填好（table 整個沒給時也一樣）。C 的 guest 本來就要寫出每個欄位。不選記成 Lua SDK 的限制（L2）：Lua 和 JS 的預設不一樣，之後別的 dictionary 也會踩到。
39. **`scrollIntoView` 的視窗示範是 `about:demo` 的按鈕，檢查「在框裡和在視窗裡都居中」**（2026-10-08，3d）：3a 只有 `shell` 的 headless 測試，視窗裡看得到的行為照 `docs/README.md` 的「完成一步時」第 5 步要有情境。按鈕把第一個捲動框裡的 A4 以 `block: 'center'` 捲到框中間，`demo-script` 檢查「在 120px 的框裡往下 40px、框捲了 80px，而且在視窗裡也居中」：居中是第 35 條「每一層都照同樣的對齊」的結果，和視窗大小、顯示器的縮放無關，所以在 windowless 和真的視窗裡都成立；絕對的捲動位置則會跟著視窗大小變。第一版只檢查「元素在視窗裡」，mutation test 時發現抓不到「不往外捲」（點按鈕本來就把框捲進視窗了），才改成檢查居中。不選改 `geometry` 情境的頁面（它已經在測滾輪和 `elementFromPoint`，`scrollIntoView` 會動到那些檢查看的捲動位置）。
40. **完成時的驗收在 Linux 上跑**（2026-10-08，3d）：1a 到 3a 在 macOS、3b、3c 在 Windows，3d 和 `matrix`、`fuzz` 在 Linux（Fedora 44）；三個平台都跑過一次完整的 WPT 和 reftest。3b 留下的「macOS 上重跑一次 `test.py wpt` 和 `reftest`」還沒做（`progress.md` 的「做過的檢查」），字型和時間的差別由 `main` 上每天的 CI 顧（`docs/testing.md` 的「CI」）。
41. **`SharedArrayBuffer` 的重複 throw 修在 fork 的 C++**（2026-10-08，3d）：完成時的 `matrix` 抓到之後，先用 kun 這一側的繞法（delegate 丟了例外之後回傳一個用不到的 id）解開 `matrix`，使用者接著要求修到根源，所以 fork 的 `src/binding.cc` 改成只在沒有 pending exception 時才轉呼叫 V8 自己的 delegate，kun 的繞法拿掉（`progress.md` 的「找到的 bug」）。動到 C++：V8 要重編（`ci.py v8`），prebuilt 要發新的 release tag，`.cargo/config.toml` 的 `RUSTY_V8_MIRROR_TAG` 和 `third_party.toml` 的 `[rusty_v8]` 跟著換（要人來 commit 和 push，還沒做）。不選只留繞法：delegate「丟了例外再拒絕」是 Blink 的寫法，其他用 `v8` crate 的人也會踩到，而 kun 這一側要一直用註解解釋為什麼回傳一個用不到的 id。

## 待決定

（目前沒有。）

## 之後的可能

- `behavior: 'smooth'` 的捲動動畫（照 Chrome 在 compositor 上跑）和打斷它時丟掉 impl 的 delta（cc 的 `clobber_active_value`），第 17 條。
- `scrollend` 事件（第 18 條）。
- 宣告表認得 WebIDL 的 overload（同名、另一組參數，轉成主要的那個），由 `dom_js` 照參數分派，像 Blink 從 IDL 產生的 binding；`scrollTo(x, y)`、`addEventListener` 的布林值 options 等現在在 `glue.js` 轉的就能搬過來（第 22 條）。
- RFC 0016 的第 2 段 `getComputedStyle`（用這份的 hook）。
- `check-layout-th.js` 解開的大批 `css/` 測試（`details.md` 第 4 節的分類：`document.fonts.ready` 擋住 294 個，量到的值不同的 634 個裡有 30 個只用了支援的 CSS）。
- 導覽到同一份文件的 `#fragment` 時當下就捲動（現在等到 event loop 的最上層，`docs/limitations.md`）：寫捲動位置的路已經有了。
- `ResizeObserver`、`IntersectionObserver`（路線圖的 M，依賴這份）。
