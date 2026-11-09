# RFC 0018 的細節：CSSOM View 和 script 觸發的 layout

`README.md` 放決定和設計；這裡放現況的盤點、Blink 的做法、選項的比較和 WPT 的收益。

## 1. 現況的盤點

2026-10-08，`main` 的 `c0d4ded`。

- **script 怎麼碰到 renderer**：`dom_api` 的每個函式拿 `&mut dyn ScriptHost`（`crates/dom_api/src/host.rs`），碰 renderer 只經過 `FrameClient`（`frame.rs`：`follow_link`、`now`、`create_child_frame`、`navigate_child_frame`、`import_module` 等），由 `ScriptHost::frame()`（執行中的 realm 的 frame）或 `dom_api::frame::document_frame`（節點的 document 的 frame）拿到。沒有幾何、viewport、lifecycle 的方法。
- **renderer 唯一的實作是 `FrameRequests`**（`crates/renderer/src/frame_view.rs`）：只記錄要求的佇列（連結、子 frame、導覽、import），沒有指向 `FrameView` 的指標。它在 commit 時建成 `Rc`，一份給 engine 的 realm（V8 context 的 slot；wasm 是 thread-local），一份放在 `FrameView`。
- **`RenderThread` 怎麼執行 script**：`frame_view: Option<FrameView>`、`text: TextContext`、`visual_properties`、`engine`、`dom: Rc<RefCell<DomIsolate>>` 都是它的欄位。每個 script 的入口（`run_script`、`run_module_script`、`dispatch_from_renderer`、timer、`call_callback`、`update_frames`）在呼叫 script 時借著 `FrameView`（至少借著 `view.realm()` 的 `&Realm`），`DomIsolate` 也 `borrow_mut` 著。`Realm` 是 `Box<dyn Any>`，不能複製。
- **所以 script 那一側碰不到** `FrameView`（`StyleEngine`、box tree、fragment、pre-paint、`ScrollState`、viewport、子 frame）、`TextContext`、`visual_properties`。只把 `FrameView` 包成 `Rc<RefCell<…>>` 也不行：呼叫 script 時外面借著它（為了 `&Realm`），hook 再 `borrow_mut` 會 panic。
- **lifecycle 需要的**：`update_style_and_layout` 和 `update_child_frames_lifecycle` 只要 `DomReadScope`、`&mut FrameView`、`&mut TextContext`，不執行 script；computed style 和 style flag 經過節點的 interior mutability 寫。`DomMutationScope::read()` 和 `ScriptHost::read()` 給得出 read scope，所以 DOM 那一側沒有問題。
- **子 frame**：`FrameView` 的 `children` 由主 frame 擁有，子 frame 的 viewport 是父 frame 的 fragment 裡 iframe 的內容框，所以同步更新要從主 frame 往下。這次 script 剛插入的 iframe 還在 `FrameRequests` 的佇列裡，task 結束時（`update_frame_tree`）才變成 `FrameView`。render-blocked 的子 frame 在 rendering update 裡跳過，主 frame render-blocked 時整個 main frame abort。
- **幾何**：`paint::element_bounds`（border box 和 inline 的 line box 的聯集，經過 transform 和捲動，不被 clip 切，document 的座標；它的說明就寫了「這是 `getBoundingClientRect` 減掉 viewport 的捲動之前」）、`paint::hit_test`、`PrePaint::point_in_child_frame`；它們都要 `PrePaint`，而 pre-paint 只在主 frame 的 paint 那條路做（RFC 0009 的第 48 條：一次走過整棵 frame tree）。`layout::document_size`、`scrollable_overflow` 給 `scrollWidth` / `scrollHeight`。`ScrollState`（每個 frame 一份，RFC 0009 的第 56 條）有 viewport 和容器的捲動位置、`clamp`、`scroll_into_view`。
- **視窗大小**：`protocol::VisualProperties`（physical px 的 viewport、device scale factor）由 `RendererMsg::UpdateVisualProperties` 送來，存在 `RenderThread`，rendering update 時換成 CSS px。
- **新的物件型別**：照 RFC 0016（`InlineCssStyleDeclaration`）的做法，每種是 `dom` 的一個 GC 型別、`script_objects!` 的一行、`dom_api` 的 `Object` / `ObjectKind` 的一個變體和宣告表的 interface；成員預設進 `kun:dom`（RFC 0012 的第 7 條），每種新物件一個 guest 測試。
- **Process 的邊界**（`docs/architecture.md`）：renderer 這一側不能有 process 共用的可變 static；共用的狀態用 `Rc` 或 thread-local。

## 2. Blink 的做法

- `Element::getBoundingClientRect` → `GetDocument().UpdateStyleAndLayoutForNode(this, DocumentUpdateReason::kJavaScript)` → layout object 的 `AbsoluteQuads`（`LocalToAbsolute` 經過 transform 和捲動）→ 減掉 viewport 的捲動 → `DOMRect::FromRectF`。`getClientRects` 回傳 `DOMRectList`。
- `Document::UpdateStyleAndLayout`：先更新 owner document（`LocalOwner()->GetDocument().UpdateStyleAndLayout(reason)`），再更新自己的 style 和 layout；不執行 script（`ScriptForbiddenScope`）。只到 style 的是 `UpdateStyleAndLayoutTreeForElement`（`getComputedStyle` 用）。
- `DOMRectReadOnly`、`DOMRect`、`DOMRectList` 在 `core/geometry`；`DOMRect` 可以建構、可以改。
- `LocalDOMWindow::innerWidth` / `innerHeight`：先更新 layout，回傳 viewport 的大小（含捲軸）；`scrollX` / `scrollY` 是 layout viewport 的捲動位置；`devicePixelRatio` 是 zoom 和 device scale factor。
- 待確認（對照原始碼）：`elementFromPoint` 的 hit test 前更新到哪個 lifecycle 階段（kun 的 hit test 要 pre-paint）。

## 3. 選項的比較

### A. 所有權：hook 怎麼碰到 lifecycle 的狀態

| | A1 lifecycle 狀態拆成共用的一半 | A2 整棵 frame tree 共用 | A3 呼叫 script 前借出去 |
| --- | --- | --- | --- |
| 做法 | `FrameView` 拆成兩半：lifecycle 的狀態（`StyleEngine`、`linked_sheets`、`images`、box tree、fragment、pre-paint、捲動、viewport、dirty flag）放進 `Rc<RefCell<…>>`，`FrameView` 和 `FrameRequests` 都拿著；`TextContext` 另外共用 | `RenderThread` 的 `frame_view`、`text`、`visual_properties` 放進一個 `Rc<RefCell<LocalFrameTree>>`；script 的入口先複製出 realm（`Realm` 改成能共用），呼叫 script 時不借它；`FrameRequests` 拿 `Weak` | 每次呼叫 script 前，`RenderThread` 把 lifecycle 要的東西移進 hook 拿得到的地方，回來再放回去 |
| 子 frame | 共用的那一半要自己再組成一棵樹（父 frame 的 fragment 決定子 frame 的 viewport） | 本來就是一棵樹 | 要把整棵樹借出去 |
| 載入的程式 | 子資源的回應改 `linked_sheets`、`images`，也要經過 `RefCell` | 照舊經過 frame tree | 照舊 |
| 錯了會怎樣 | `RefCell` 在執行時 panic | 同左 | 忘了借出去的入口，hook 拿不到狀態（安靜地錯） |
| 和 Blink | `LocalFrameView` 和 `LocalFrame` 分開，有點像 | document 經過 frame 碰到 `LocalFrameView`，最像 | 沒有對應 |
| 改動量 | `frame_view.rs` 的欄位大搬家 | `render_thread.rs` 的每個 script 入口和 `self.frame_view` 的用法（機械的） | 每個 script 入口包一層 |

另外兩種（2026-10-08 討論時加上的，不用 `Rc<RefCell>`）：

- **A4 和 DOM 一起借給 engine**：frame tree、`TextContext`、視窗大小照舊由 `RenderThread` 擁有；script 入口交出 `&mut DomIsolate` 時也交出 `&mut dyn` 的 lifecycle 介面，engine 在 `ScriptExecution` 期間把它和 `DomState` 一起放進 isolate 的 slot（`crates/dom/src/v8_engine.rs` 的 `DomStateSlot`），離開時拿掉；wasm 的 host 一樣。realm 搬出 `FrameView`，`RenderThread` 照 `DocumentId` 存，呼叫 script 時拿 realm 和借出 frame tree 是不同的欄位。借用在編譯期檢查，漏交的入口編譯不過；多一處 `unsafe`，論證和 `DomStateSlot` 一樣。
- **A5 frame tree 放進 DOM 這一側**：放在 `DomIsolate` 的 embedder 狀態，script 從 `cx.mutate()` 拿到，DOM 的讀和 frame tree 的改在 `dom` 裡拆成兩個借用。最像 Blink 的所有權（`LocalFrame`、`LocalFrameView` 和 DOM 在同一個 GC heap），但 `RenderThread` 碰 frame 都要經過 DOM，rendering update、載入、輸入都要改。

2026-10-08 決定 **A4**（`README.md` 的第 4 條）：使用者不想用 `Rc<RefCell>`；A4 的改動集中在 script 的入口和 engine 的介面，`unsafe` 的論證沿用現有的。

### B. 幾何的來源

- **B1 從 layout 的 fragment 換算**：照 Blink 的 `LocalToAbsolute`，沿著 fragment tree 加 offset、套 transform、減捲動。不用 pre-paint，但等於再寫一份 pre-paint 已經做的座標換算（transform、捲動、子 frame 的內容框），兩份要一致。
- **B2 跑 pre-paint，用它的 property tree**：同步的路跑到 pre-paint（只做 pre-paint，不 paint），幾何用 `element_bounds` 等現成的函式；和畫面、hit test 同一套座標，`elementFromPoint` 也直接能用。代價是每次強制 layout 都從頭做一次 pre-paint。

2026-10-08 決定 **B1**（`README.md` 的第 3 條）：Chrome 強制 layout 只到 layout，座標從 layout tree 換算；B2 讓每次強制 layout 多做整棵樹的 pre-paint，Speedometer 量的時間會偏高。

### C. `DOMRect` 的表示

- **C1 每個 Blink 類別一個 GC 型別**（RFC 0016 的 Z1）：`DOMRectReadOnly`、`DOMRect`、`DOMRectList`，JS 和 wasm 都是同一套物件；`new DOMRect()`、`fromRect`、setter、`instanceof` 都照規範。
- **C2 中性的 record**（`rfcs/pre-rfcs/P3.md` 提過）：宣告表回傳一個沒有身分的值，各 front end 自己建物件。宣告表還沒有這種機制，`DOMRect` 又是可以建構、可以改的 interface，要另外處理。

2026-10-08 決定 **C1**（`README.md` 的第 1 條）。

### D. 分階段

照 M1 要的先做（`README.md` 的「分階段」）：第 1 段是 hook 和 `getBoundingClientRect`（Speedometer 的 runner 要的），第 2 段是元素的盒子大小和捲動（M2 的 suite 要的），第 3 段是 `scrollIntoView` 和 WPT 的匯入。

## 4. WPT 的收益

2026-10-08，WPT `0f195499`（`tests/wpt/README.md` 釘住的 commit）。

- `css/cssom-view/`：228 個 testharness.js 的測試、11 個 reftest，都還沒匯入。用到各成員的測試檔數（粗略地用字串找）：`scrollTop` 59、`scrollIntoView` 49、`getBoundingClientRect` 42、`scrollLeft` 39、`scrollTo(` 38、`clientWidth` 36、`scrollY` 29、`clientHeight` 26、`offsetWidth` 24、`offsetTop` 24、`offsetHeight` 22、`getClientRects` 20、`scrollX` 19、`scrollHeight` 19、`scrollWidth` 17、`elementsFromPoint` 17、`elementFromPoint` 16、`offsetLeft` 14、`innerHeight` 9、`innerWidth` 8、`scrollBy(` 8、`DOMRect` 8、`clientTop` 8、`offsetParent` 6、`devicePixelRatio` 1。
- 會卡住的功能：smooth scroll 29、writing mode 29、transform 27、`<iframe>` 17、`position: fixed` / `sticky` 13、scroll 事件 8、testdriver 5。
- `check-layout-th.js`：`css/` 裡約 1134 個檔案用它（`offset*`、`getBoundingClientRect`），RFC 0003 的 flex、grid 測試的大宗。

### `check-layout-th.js` 的評估

3c，2026-10-08，Windows 11。第 37 條：`css/` 裡用 `check-layout-th.js` 的 testharness.js 檔案，共 1133 個（`.html` / `.htm`；沒有 `.xht`、`.svg` 的），在 headless 的 kun 裡全部跑過一次，依第一個失敗的原因分類：

| 結果 | 個數 | 說明 |
| --- | --- | --- |
| 全過 | 106 | `css-flexbox` 49、`css-grid` 22、`css-sizing` 13、`CSS2` 8、`css-overflow` 7、`css-multicol` 4，`css-tables`、`css-values`、`css-conditional` 各 1。收進 `testharness-include.txt` 當回歸測試；連跑兩次，也用 CI 的 `MACHOPO_WPT_TIMEOUT_MULTIPLIER=3` 跑一次，都過 |
| 停在 `document.fonts.ready` | 294 | 量之前等字型載入（CSS Font Loading），kun 沒有 `document.fonts`：`css-grid` 267、`css-sizing` 11、`css-tables` 5…。留在「之後的可能」 |
| 量到的值不同 | 634 | 其中 604 個用了 kun 不支援的 CSS（照 `scripts/wpt_reftest_candidates.py` 的清單；一個檔案可能有好幾種）：`position` 423、`writing-mode` 233、`float` 115、`direction` 107、`outline` 105、`top` / `left` 等、`inline-size` / `block-size` 等 logical property、`inset`、`content`、`vertical-align`、`clear`、`align-items: last baseline`、anchor positioning 44、table、`columns`、`border-spacing`…；用連結的 stylesheet 的 88 個沒有再細分。只用支援的 CSS 的 30 個，是 layout 的差異：SVG 10 個（`svg-intrinsic-size-*`、`svg-root-as-flex-item-*`）、table 6 個（`colspan-*`、`percent-width-ignored-*`、`table-as-item-cross-size`）、`<img>` 的比例和最小尺寸 5 個、`<canvas>` 2 個、scroll container 的最小尺寸 2 個（flex 和 grid 的 `*-minimum-size-single-axis-scroll-container`）、scrollable overflow 3 個（`input` 的 padding、self-collapsing、`transform`）、`CSS2/normal-flow` 的 `containing-block-percent-padding-*` 2 個（`padding-top: 10%` 沒有反映在高度上：預期 100、得到 50） |
| harness 的錯誤 | 44 | 大多是 `dataset`（`check-layout-th.js` 寫 `element.dataset.expectedHeight`，kun 還沒有，RFC 0017），`css-grid/abspos/` 的 32 個、`contain-intrinsic-size` 等；anchor positioning 的 3 個（`anchor-position-inline-*`）是 `Maximum call stack size exceeded`：頁面同時有全域的 `function onload()` 和 `<body onload="onload()">`，在 kun 裡互相呼叫：kun 編譯 `<body>` 的 window handler 時，scope 也放了 `<body>`，`onload` 指到 handler 自己；HTML 和 Chrome 的 scope 只有 global。3c 之後修正（`progress.md`），這 3 個接著停在 `document.fonts.ready` |
| 少了 API 或名稱 | 54 | `getComputedStyle` 35（RFC 0016 的第 2 段）、window 的 named property（以 `id` 當全域變數）、`showPopover`、`showModal`，和 `css-values/viewport-units-gutter-*` 的 `measure` 7 個 |
| 逾時 | 1 | `css-flexbox/relayout-image-load.html` |

layout 的差異照第 37 條不在這份 RFC 補，記在「之後的可能」；不支援的 CSS 是各自的 RFC 的範圍（`position` 是 RFC 0013，`float`、table 等是 RFC 0012 路線圖的 S 系列）。
