# RFC 0038：`Selection` 和 `Range` 的幾何

- 開始日期：2026-10-12
- 狀態：✅ 已完成：2026-10-12（第 1 到 14 條；兩段，各三步，都在 Fedora 44 上做；第 1 段在 1b 之後先 merge 回 `main`；驗收和檢查在 `progress.md`）
- 細節：`details.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 P（編輯）的第一份，DOM 線接在 I（RFC 0037）之後。P 太大（XL），照第 1 條拆開：這份做 script 看得到的選取和量文字的幾何，打字、游標和選取的繪製另外寫。RFC 0037 的第 1 條把 `Range` 的幾何留給「和 `Selection` 一起、和 CSS 線協調的一份」，就是這份。做完時路線圖的 M6 達成：兩個 Editor 的 Speedometer suite 跑完、不丟例外，TipTap 量得到（第 14 條修了 animation frame callback 的順序）。

## 摘要

加上 Selection API（`Selection`、`document.getSelection()`、`window.getSelection()`、`selectionchange`）和 CSSOM View 的 `Range.getClientRects()`、`getBoundingClientRect()`。每份 document 有一個 `Selection`，照規範是「零或一個 live `Range` 加上方向」，range 就是 RFC 0037 的 live range，樹改變時跟著調整，不另外寫一套；方向、`containsNode`、`selectionchange` 照 Chrome 152 的實測。幾何照 Blink 的 `OffsetMapping` 和 `FragmentItem`：`layout` 收集文字時記下每段文字來自哪個 text 節點、原文的哪個 UTF-16 offset，text fragment 記下它是原文的哪一段和每個 cluster 的位置，盒子從 fragment 算。`contenteditable` 的 editing host 照 Chrome 能 focus，focus 時選取移進去。JS 和 wasm 都給。打字、`execCommand`、游標和選取的繪製、滑鼠選取不在這份。

## 動機

**現況（2026-10-12，`main` 的 `86dc35bc`）**：
- 沒有 `Selection`、`getSelection()`；`Range` 沒有幾何（`docs/limitations.md` 的「Bindings 和 DOM API」裡「`Range` 沒有幾何……沒有 `Selection`」那一條，和「事件」的文字編輯那一條的「`contenteditable`、`Selection`、`execCommand` 都沒有」）。`spec-missing.txt` 有 `Document.getSelection()`、`Window.getSelection()`、`Range.getClientRects()`。
- `contenteditable` 只有 IDL attribute（`contentEditable`、`isContentEditable`），editing host 不能 focus（`docs/limitations.md` 的「事件」的 focus 那一條）。
- **Speedometer**（`ci.py speedometer --inventory --suites Editor-CodeMirror,Editor-TipTap`，2026-10-12，`86dc35bc`）：兩個 Editor（M6）跑完但有例外，都停在 `getSelection`，所以編輯器沒有建起來（`details.md` 的「Speedometer 用到的」）：CodeMirror 的 `EditorView` 建構時讀選取，丟例外時還沒把編輯器放進頁面；TipTap 的 `new Editor` 在 Chrome 的路徑上讀 `focusNode` 時丟例外。兩個 suite 缺的 API 只有 `Document.getSelection`、`Selection` 的 `anchorNode`、`anchorOffset`、`focusNode`、`focusOffset`、`collapse`（TipTap）和 `Range.getClientRects`；有了 `getSelection` 之後，CodeMirror 量文字（`measureTextSize`）和 TipTap 捲到游標（`scrollToSelection`）用 `Range.getClientRects` / `getBoundingClientRect`，沒有它們還是丟例外。
- WPT 的 wdspec 有些失敗是 `document.getSelection`（`docs/limitations.md` 的「WebDriver 和自動化」）；`dom/ranges/` 的 `Range-mutations-*` 有一半的 subtest 在等 `Selection`。

目標：
- Selection API（W3C 的 Selection API）：`Selection` 的 `anchorNode`、`anchorOffset`、`focusNode`、`focusOffset`、`isCollapsed`、`rangeCount`、`type`、`direction`、`getRangeAt`、`addRange`、`removeRange`、`removeAllRanges`、`empty`、`collapse`、`setPosition`、`collapseToStart`、`collapseToEnd`、`extend`、`setBaseAndExtent`、`selectAllChildren`、`deleteFromDocument`、`containsNode`、`toString`；`Document.getSelection()`、`Window.getSelection()`；`selectionchange`（document 的）。
- `Range` 和 `Selection` 的關係照規範：`getRangeAt(0)` 回傳同一個物件，script 改它（`setStart` 等）就是改選取；range 在樹改變時照 RFC 0037 的調整移動，選取跟著動。
- CSSOM View 的 `Range.getClientRects()`、`getBoundingClientRect()`：text 的部分是那一段文字在每一行的盒子，整個包含的元素是它的 border box。
- editing host（`contenteditable` 是 true 或 plaintext-only 的元素）能 focus；focus 進去時，選取不在它裡面就照 Chrome 放到開頭。
- JS 和 wasm 的 guest 都有（RFC 0012 的第 7 條）。
- 兩個 Editor 跑完、不丟例外，DOM 和 Chrome 一樣（M6 還要 RFC 0036 的 `IntersectionObserver`）。結果：兩個都跑完、不丟例外；TipTap 有效，CodeMirror 的 DOM 因為排版的差異和 Chrome 差 2 行（`progress.md`）。

不在範圍內（第 1 條）：
- **編輯**：在 editing host 裡打字和刪除、`beforeinput` / `input` 改 DOM、`execCommand`、undo、剪貼簿、輸入法、`designMode`；文字控制項的編輯（RFC 0030 的最小編輯）和一般的編輯合不合在一起（RFC 0030 的第 4 條）在那時決定。
- **畫出來的部分和滑鼠**：游標、選取的反白、`::selection`、`caret-color`、滑鼠點擊和拖曳選取、`caretPositionFromPoint`、雙擊選字，和 CSS 線協調。
- **要排版移動的**：`Selection.modify()`（以字、行移動）、以 grapheme 以外的單位移動。
- `getComposedRanges()`、Chrome 自己的 `ShadowRoot.getSelection()`、文字控制項的 `selectionchange`（分派在 `<input>`、`<textarea>`）、`SVGTextContentElement.selectSubString()` 真的選取。

## 設計概要

對寫網頁的人：

```js
const selection = document.getSelection();   // 每份 document 一個，window.getSelection() 是同一個
selection.collapse(textNode, 3);              // 游標：anchor 和 focus 都在 (textNode, 3)
selection.extend(textNode, 7);                // focus 移到 7，direction 是 "forward"
const range = selection.getRangeAt(0);        // 同一個 Range，range.setEnd(...) 也改了選取
textNode.insertData(0, "ab");                 // live：選取跟著往後移兩格
document.addEventListener("selectionchange", ...);  // 選取改了之後的 task 裡分派一次

range.getClientRects();                       // 那一段文字在每一行的盒子（viewport 的座標）
range.getBoundingClientRect();                // 它們的聯集
```

和 Chrome 一樣的地方：offset 是 UTF-16 的單位；同一個 task 裡改好幾次選取只分派一次 `selectionchange`；collapse 的選取沒有方向；collapse 在文字裡的 range 的 `getClientRects()` 有一個寬度 0 的盒子（CodeMirror 靠它量游標），在換行處是下一行的開頭；範圍的頭尾在一個字元（surrogate pair）中間時算整個字元；`<div contenteditable>` 沒有 `tabindex` 也能 `focus()`，選取移到它的開頭。和 Chrome 不同的（`docs/limitations.md`）：kun 照規範存 script 給的位置（Chrome 的編輯和繪製用正規化的 `VisibleSelection`，第 2 條）；`toString()` 是 range 的文字，不看排版（第 8 條）；使用者還不能用滑鼠選、選了也看不到。

對改 kun 的人：
- `dom`：`Selection` 是新的 GC 型別（script 物件，有 wrapper）：一條 traced 的 edge 指向它的 `Range`（可以沒有）加上方向；document 的 rare data 撐住它。range 照舊在 document 的 live range 的弱集合，樹改變時照 RFC 0037 的調整移動，`Selection` 不用自己的 hook。
- `dom_api`：新的 `selection` 模組（宣告在 `interfaces/core_editing.rs`）：規範的演算法、`selectionchange` 的排程（選取上「已排程」的旗標，DOM 的 task）、editing host 的 focus。`range` 模組的設定經過一個 `set_range`：設的是選取的 range 時，選取跟著它；`getClientRects()` 照 Blink 的 `Range::GetBorderAndTextQuads` 取元素和文字。
- `layout`：`InlineItem::Text` 記下 text 節點，空白摺疊時記下保留的字元的 UTF-16 位置（`TextSource`），`TextFragment` 多了 `source` 和 `clusters`；`layout::text_rects` 和 `client_rects` 共用走 fragment tree 的部分。
- `LayoutClient::text_rects`（renderer 經過 `FrameView` 實作，和元素的 `client_rects` 一樣先更新到 layout）。
- renderer 的 animation frame callback 照 frame tree 的順序、每份 document 輪到時才拿（第 14 條）。
- `dom_js`、`dom_wasm`：宣告表的新 interface，WIT 只加不改。

## 詳細設計

（比較在 `details.md` 的「選項」，項目的編號對應它；決定在「已決定的事項」。）

1. **範圍**（P）：P 拆成三份：這份（選取和幾何）、編輯（contenteditable 的打字、`beforeinput`、`execCommand`、undo，和文字控制項的編輯合併與否）、畫面（游標、反白、滑鼠選取、`caretPositionFromPoint`）。第 1 條（P1：拆成三份，這份是第一份）。兩個 Editor 只要這份（加上 RFC 0036 的 `IntersectionObserver`），後兩份各自是 L 以上，而且畫面那份主要改 CSS 線的檔案。
2. **`Selection` 的資料**（S）：規範的模型：`Selection` 撐住零或一個 `Range`，加上方向（forward、backward、none）；anchor 和 focus 照方向從 range 的兩端讀。range 是一般的 live range，所以樹改變時已經會動；script 拿到的 `getRangeAt(0)` 就是它。document 的 rare data 有一條 traced 的 edge 撐住 `Selection`（第一次 `getSelection()` 時建立），`Selection` 撐住 range。第 2 條（S1）。Blink 的 `FrameSelection` 存的是 position（`SelectionInDOMTree`），自己在 DOM 改變時調整，`getRangeAt` 另外建一個快取的 `Range` 再同步回去；正規化（`VisibleSelection`）要排版，在 Blink 裡是編輯和繪製用的，script 看到的 anchor 和 focus 現在也是沒有正規化的位置。
3. **規範的演算法**（`dom_api` 的 `selection` 模組）：
   - `addRange`：已經有 range 就不做事（Chrome 和規範一樣）；range 的 root 不是這份 document 也不做事。
   - `getSelection()`：沒有 browsing context 的 document（kun 是沒有 window 的，例如 `createHTMLDocument`、`DOMParser` 的）回傳 `null`，`window.getSelection()` 和它的 document 的是同一個物件。
   - shadow tree：`collapse` 等照規範收 document 的 shadow-including 的 inclusive descendant，anchor 和 focus 照存、不改指到 host（Chrome 現在也是，WPT 的 `selection/shadow-dom/tentative/` 由 Chromium 寫）；`getComposedRanges()` 不在這份。
   - `collapse` / `setPosition`、`extend`、`setBaseAndExtent`、`selectAllChildren`：檢查（doctype、offset 超出長度、`extend` 時沒有 range，順序照 WPT），建一個新的 live range（不改原本的那個，規範的寫法：script 手上的舊 range 不再是選取的），設方向。
   - **方向照 Chrome 152 的實測**：collapse 的選取的 `direction` 一律是 `none`；`addRange` 和 script 改選取的 range 之後是 `forward`（規範是保留之前的方向）；只有 `setBaseAndExtent`、`extend` 照 focus 在 anchor 前後給 `backward`。
   - `removeRange`、`removeAllRanges`、`empty`、`collapseToStart`、`collapseToEnd`、`deleteFromDocument`（range 的 `deleteContents`）、`containsNode`（照 Blink 的 `DOMSelection::containsNode`：節點在 parent 裡的前後，部分在裡面的 text 節點也算；tree order 用 RFC 0037 的輔助函式）。
   - **range 被 script 改到另一棵樹時**：選取的 range 的兩端不再在 document 裡（shadow-including）時選取清空，range 移回來也不恢復；在 `Range` 的設定之後檢查「它是不是某個選取的 range」（document 的 `Selection` 的 edge 比對），是就清掉。第 9 條（C1，照 Chrome 和 WPT 的 `move-selection-range-into-different-root.tentative.html`）。
   - **樹改變讓選取離開 document**：shadow host 被移除時，live range 的調整只把 boundary point 移出被移除的節點自己的樹（DOM 的規定），選取的 range 留在 shadow tree 裡；照規範的 `rangeCount`、`anchorNode`，script 讀的時候看兩端在不在 document 裡，不在就當成空的（WPT 的 `selection-range-in-shadow-after-the-shadow-removed.tentative`，1c 時發現）。
   - `toString`：第 7 項。
4. **`selectionchange`**：選取真的改變時（anchor、focus 或方向和之前不同；同樣的參數再設一次不算），在 document 上「排一次」。改變來自選取的方法、`Range` 的設定、editing host 的 focus，和字元資料的調整（replace data、split、normalize 讓選取的兩端移動時，在 RFC 0037 的 `live_ranges_replaced`、`live_ranges_split`、`merge_text_ranges` 裡）；插入、移除節點的調整讓選取移動時不排。第 10 條（D1，照 Chrome 152 的實測，`details.md` 的「Chrome 的實測」）。排的做法：選取有「已排程」的旗標，沒有才排一個 DOM 的 task（user interaction 的 task source），task 裡清旗標、分派不冒泡、不能取消的 `Event`；`dom` 的原語只記下哪份 document 要排，`dom_api` 在宣告表每次呼叫的收尾排（自己的改變當場排，使用者的 focus 不經過宣告表）。不用 `select` 那種 element task：renderer 在一輪裡先跑完到期的 timer 才跑它們，事件會晚於之後的 `setTimeout(0)`。Blink 的 `Document::ScheduleSelectionchangeEvent` 也是一個旗標加一個 task。
5. **editing host 的 focus**（F）：`contenteditable` 是 true 或 plaintext-only（含繼承：最近的有 `contenteditable` 的 HTML 元素決定）的元素是 editing host 時能 focus（照 Blink 的 `IsRootEditableElement`：可編輯、parent 不可編輯的 HTML 元素；focusable area，Tab 也停在它，`tabIndex` 照 HTML 和 Chrome 仍是 -1；Chrome 的 Tab 沒有實測）；focus 進去時，在 `focus` 事件之前，選取不在它裡面就 collapse 到它的開頭（Blink 的 `Element::UpdateSelectionOnFocus`；開頭簡化成第一個葉節點是 text 時的那個 text 的 0，否則它自己的 0）。第 3 條（F1：照 Chrome）。TipTap 自己給了 `tabindex="0"`，這一項對 Speedometer 不是必要的，但 editing host 不能 focus 時，任何不給 `tabindex` 的編輯器都 focus 不了。
6. **幾何**（G、O）：
   - **offset 的對應**（O）：`layout` 收集 inline 的文字（`InlineItem::Text`）時，每段文字記下它的 text 節點（原本記的是元素）；空白摺疊（`collapse_white_space`）時記下每個原文的 UTF-16 offset 對到排版文字的哪個 byte（被摺掉的對到下一個留下的字元）。對應表跟著 `InlineContent` 存到下一次 layout。第 4 條（O1：layout 時一律建）。Blink 的 `OffsetMapping` 在需要時才建、快取在 inline formatting context 上（O2）；kun 的空白摺疊已經逐字元記下來源，順手建的成本小，量到記憶體或時間是問題時再改成 O2。
   - **盒子**（第 11 條，A1）：照 Blink 的 `FragmentItem` 和 `ShapeResultView::LocalRect`，從 fragment 算。每個 text 節點的文字有自己的 parley brush，所以一個 glyph run 只屬於一個 text 節點；`TextFragment` 記下它代表原文的哪一段（`source`：一個節點的 run 依序蓋滿整段原文，摺疊掉的空白算在前一個 run）和每個 cluster（`TextCluster`：原文的開頭和結尾、x；parley 沒有公開 glyph run 的 cluster，從 run 裡 `style_index` 一樣的取，不改 fork）。`layout::text_rects` 走 fragment tree（和元素的 `client_rects` 共用：transform、捲動容器、inline 的 `position: relative` 都已經在 fragment 裡），每個和範圍重疊的 run 給一個盒子：開頭在一個 cluster 裡時是它的開頭（照 Chrome 擴到整個字元），結尾是下一個 cluster 的開頭（cluster 一個接一個），在摺疊掉的空白裡是下一個保留的字元，掛在行尾的空白不佔位置；高度是字型的 ascent 加 descent（不是 line box，和 Chrome 一樣）。collapse 的範圍給一個寬度 0 的盒子，在它所在的 run（一個 run 的結尾、下一個的開頭時是下一個：換行處是下一行的開頭），只有在原文的結尾才用結尾是它的 run。沒有 box 的 text 節點沒有盒子。不用 parley 的 `Selection::geometry`（A2）：留在 box tree 的 parley `Layout` 在用快取的 fragment 時，換行的寬度可能不是最後的，高度是 line box 的，inline 的 `position: relative` 也要另外補。
   - **查詢**（第 12 條，B1）：`LayoutClient` 多一個 `text_rects(text 節點, UTF-16 的 start, end)`，renderer 經過 `FrameView` 實作，和元素的 `client_rects` 一樣先更新 style 和 layout（只到 layout）；range 裡整個包含的元素用現有的 `client_rects`，`dom_api` 照 CSSOM View 組起來。
   - **`getClientRects()`**：照 Blink 的 `Range::GetBorderAndTextQuads`（CSSOM View 的規則的寫法）：從 range 的第一個節點走到最後一個之後的節點（character data 的 container 本身算），parent 不在其中、也不含頭尾 container（兩者不同時）的元素用它的 `getClientRects()`（collapse 在空元素裡時是那個元素的），每個 text 節點用 `text_rects` 取它在 range 裡的一段，照 tree order 串起來；不在 document 裡的 range 沒有。`getBoundingClientRect()` 是聯集（寬或高是 0 的盒子不加入，只有它們時回傳第一個；規範的寫法）。text 的盒子扣掉 viewport 的捲動，parent 是 `position: fixed` 的元素時不扣（和元素的 `getClientRects` 一樣只看那一個元素）。
   - SVG 的 `<text>`：照舊沒有（SVG 的文字不經過 inline layout），`getClientRects()` 不給它的盒子；記在 `docs/limitations.md`。
7. **`Selection.toString()`**：規範說是「rendered text」，沒有寫演算法（WPT 只有 `.tentative` 的測試），Chrome 用排版之後的純文字（和 `innerText` 同一套，`<br>` 是換行、看不見的不算）；kun 沒有 `innerText`。第 8 條（B1）：用 range 的 `toString()`，`display: none` 的文字和 `<br>` 和 Chrome 不同，記在 `docs/limitations.md`；`innerText` 另外排，做了之後改用它。
8. **wasm**（W）：`selection` 的 resource、`document` 和 `window` 的 `get-selection`、`range` 的 `get-client-rects` 和 `get-bounding-client-rect`，只加不改，照 `crates/dom_api/README.md` 的規則 `kun:dom` 不升版（只新增 resource 和函式不升；草稿寫成升 minor 版，1b 之後 merge 時照規則改正）。第 5 條（W1：都給）。
9. **animation frame callback 的順序**（第 14 條，2c 時發現）：kun 原本在 frame 開始時就拿走每份 document 的 callback；HTML 的 rendering update 照 tree order 一份 document 一份 document 跑，輪到時才拿它的清單（Blink 的 `PageAnimator::ServiceScriptedAnimations`）。Speedometer 的 runner 在最上層 document 的 callback 裡跑測試步驟，TipTap 排在 iframe 的 `requestAnimationFrame` 裡的 `focus()` 在 Chrome 是同一個 frame、runner 的 `setTimeout` 之前跑，kun 晚一個 frame，DOM 的摘要少了 `ProseMirror-focused`。改成照 HTML 的順序輪流（父的先，子 frame 照 iframe 在父 document 裡的 tree order，`FrameView::frames_in_tree_order`；原本的 `frames()` 是建立的先後），輪到才拿（`Timers::animation_frame_handles_of`）；frame 開始時就有、屬於沒有 frame 顯示的 document 的照舊丟掉。rendering update 的其他步驟照舊是建立的先後（`docs/limitations.md`）。

### 測試

驗收的每一項和指令在 `progress.md`。概要：
- `dom`：`Selection` 的 GC（`script_objects::selection`、`script::every_type_unwraps_only_as_itself`、`selections::`、工作負載的選取拿走 range）。
- `dom_js`：`selections::`（每個方法、方向、檢查的順序、`addRange` 和 `removeRange`、選取的 range 是 live 的、`containsNode`、shadow tree），`ranges::a_ranges_boxes`（取哪些盒子）。
- `layout`：`text_rects::`（Ahem：一段、摺疊掉的空白、跨行和掛著的空白、換行處、第二個 text 節點、`position: relative`、UTF-16、transform 和捲動）。
- `shell`：`selections::`（`selectionchange` 的 task、editing host 的 focus）、`keys::tab_reaches_an_editing_host`、`geometry::a_ranges_boxes`（renderer 的 layout）、`script::` 的三個 animation frame 的順序。
- `dom_wasm` 的 `selection.c`、`geometry.c`，Lua 的 `w9-lua.html` 第 46 項。
- WPT：`selection/` 整個收；`dom/ranges/`、`css/cssom-view/` 等的預期結果更新；wdspec 的 `new_window`、`new_tab`。
- Speedometer：兩個 Editor 的盤點和有效性（RFC 0012 的 `progress.md`）。
- mutation test：每一步一份 `mutants-*.toml`，等價的記在 `docs/testing.md`。

### 分階段

第 6 條（T1），分兩段：
1. **`Selection`**：資料、所有的方法、`getSelection()`、`selectionchange`、`Range` 的設定和選取的關係、editing host 的 focus；JS 和 wasm；WPT 的 `selection/`。這段做完時兩個 Editor 不再停在 `getSelection`，但還會停在 `Range.getClientRects`。第 7 條（A1）分三步：
   - **1a**：`dom` 的 `Selection`（document 的 rare data 撐住它，它撐住 range 和方向）、`dom_api` 的 `selection` 模組的所有方法、`Document.getSelection()` / `Window.getSelection()`、range 換到別的樹時清空；JS。
   - **1b**：`selectionchange`；editing host 能 focus、focus 時把選取放進去。
   - **1c**：wasm（C、Rust、Lua 的 guest 的測試；WIT 在 1a 已經加了）、WPT 的 `selection/`、文件。
2. **`Range` 的幾何**：`layout` 的 offset 對應、text 的盒子、`LayoutClient`、`getClientRects()` / `getBoundingClientRect()`；JS 和 wasm；WPT 的 `css/cssom-view/` 的 range；Speedometer 的兩個 Editor。這段改 `layout`、`renderer` 的 `frame_view`、`layout_client`（CSS 線的檔案）；2026-10-12 確認遠端沒有 branch 在改它們，CSS 線接下來的 RFC 0016 第 3 段主要改 `style`，只會和這段在 WIT（rebase 時重新產生、重編 guest）和 `frame_view.rs`（不同的函式）碰到。第 13 條（C1）分三步：
   - **2a**：`layout`：text 節點的 id 和 UTF-16 的對應表（第 4 條）、text fragment 的原文（UTF-16）的一段和 cluster 的位置（第 11 條）、`layout::text_rects`；`layout` 的單元測試。
   - **2b**：`LayoutClient::text_rects` 和 renderer 的實作、`Range.getClientRects()` / `getBoundingClientRect()`（JS 和 wasm）、WPT 的 `css/cssom-view/` 的 range；相關的 `test.py reftest` 和 `wpt` 確認畫面沒有變。
   - **2c**：Speedometer 兩個 Editor 的盤點和有效性、文件、mutation test、第 2 段的檢查。

## 缺點

- **選取不正規化**（第 2 條）：script 設在看不見的位置（例如 `display: none` 的裡面），kun 照存，Chrome 也照存 anchor 和 focus，但之後的編輯和繪製要的是正規化之後的位置，那時要另外算（Blink 的 `VisibleSelection` 就是這樣在 `FrameSelection` 外面算的）。
- **每段 inline 文字多一張對應表**（O1）：所有頁面都付記憶體，沒有用 `getClientRects` 的頁面也是。
- **每個 text fragment 多一張 cluster 的表**（第 11 條）：每個 cluster 12 bytes，和 glyph 的表差不多大。
- **量出來的數字和 Chrome 不同**：字型和排版（kun 的 parley 對 Chrome 的 HarfBuzz 和 LayoutNG）的差異直接影響 CodeMirror 量的行高和字寬，它照量到的值決定畫哪些行。結果 CodeMirror 的 DOM 和 Chrome 差 2 行，Speedometer 的有效性檢查不過；這是排版的問題，不在這份解決。

## 理由和其他做法

各項的完整比較在 `details.md` 的「選項」。重點：
- 拆開 P（第 1 條）：兩個 Editor 要的是 script 看得到的部分，打字和繪製各自都很大，綁在一起的話 M6 要等全部做完。
- 規範的模型（第 2 條）：RFC 0037 的 live range 已經照規範調整，選取只要撐住一個 range；Blink 的 position 加快取的 range 是為了編輯和繪製，那時要的正規化可以在選取外面算。
- 幾何照 Blink 的 `OffsetMapping` 和 `FragmentItem`（第 4、11 條）：DOM 的 offset 對到排好的文字只能由 layout 回答，它在收集文字時記下對應；盒子從畫出來的 text fragment 算，位置和高度都和畫的一樣。

## 先例

- **Chromium**（`details.md` 的「Chromium 的做法」，依記憶整理；行為用 Chrome 152 實測，`details.md` 的「Chrome 的實測」）：`DOMSelection`（script 的 `Selection`，`core/editing/dom_selection.cc`）包著 `FrameSelection`（每個 `LocalFrame` 一個，`SelectionEditor` 存 `SelectionInDOMTree`，是 `SynchronousMutationObserver`）；`getRangeAt` 回傳 `FrameSelection` 快取的 `Range`，`Range::UpdateSelectionIfAddedToSelection` 把 script 對它的修改同步回選取；`Document::ScheduleSelectionchangeEvent`；`Element::UpdateSelectionOnFocus`；`Range::getClientRects` 經過 `LayoutText::AbsoluteQuadsForRange`、`OffsetMapping`（DOM 的 offset 對到 inline formatting context 的文字）和 `FragmentItem` 的 `LocalRect`。
- **Gecko**：`Selection` 存好幾個 range（規範只允許一個，Firefox 為了表格的選取保留多個），`nsRange` 是 live range。
- **RFC 0037** 的 live range、第 1 條（幾何留給這份）；**RFC 0022** 的 focus 模型；**RFC 0023** 的文字控制項的選取（另一個模型，這份不動）。

## 已決定的事項

2026-10-12 確認第 1 到 6 條，第 1 段開始前確認第 7 到 10 條（比較在 `details.md` 的「選項」）。

1. **P1：P（編輯）拆成三份，這份做選取和幾何**（2026-10-12）：Selection API、`selectionchange`、editing host 的 focus、`Range` 的幾何；編輯（contenteditable 的打字、`beforeinput`、`execCommand`、undo、輸入法，和文字控制項的編輯合不合併）和畫面（游標、反白、`::selection`、滑鼠選取、`caretPositionFromPoint`）之後各寫一份。兩個 Editor 只要這份，後兩份各自是 L 以上、畫面那份主要在 CSS 線。不選 P2（一份四段：XL，M6 要等全部、跨兩條線太久）、P3（只做 Speedometer 用到的成員：「有 `getSelection` 但方法不存在」比沒有更難判斷，WPT 的 `selection/` 收不了）。
2. **S1：`Selection` 撐住一個 live `Range` 和方向**（2026-10-12）：照規範；樹改變時由 RFC 0037 的調整移動，不另外寫；`getRangeAt(0)` 是同一個物件，script 改它就是改選取。代價：沒有 affinity（之後畫游標那一份在 `Selection` 上加）、編輯和繪製要的正規化在需要時另外算、照 Blink 寫編輯指令時要從 `Position` 換過來、每次 `collapse` / `extend` 建一個新的 `Range`（規範的寫法）、`Range` 的設定多一次「是不是選取的」的比對。不選 S2（照 Blink 存 position、`getRangeAt` 建快取的 range 再雙向同步：第二套調整和 RFC 0037 的重複、同步兩個方向都要測，好處要到之後的編輯和繪製才用得到；之後真的需要時可以換，script 看到的行為一樣）、S3（存兩個 boundary point、`getRangeAt` 每次建新的：不合規範，調整也要另寫）。
3. **F1：editing host 照 Chrome 能 focus，focus 進去時選取不在裡面就放到開頭**（2026-10-12）：HTML 列 editing host 是 focusable area，Blink 的 `Element::SupportsFocus`、`UpdateSelectionOnFocus`。不選 F2（留給編輯那一份：沒有 `tabindex` 的 `contenteditable` 元素 `focus()` 沒有作用，是 script 看得到、和選取有關的行為）。
4. **O1：layout 收集 inline 文字時一律建 DOM offset 的對應表，跟著 `InlineContent` 存**（2026-10-12）：空白摺疊已經逐字元記下來源，順手記下 text 節點和 UTF-16 offset；盒子見第 11 條（從 fragment 算）。不選 O2（照 Blink 的 `OffsetMapping` 需要時才建、快取：要先把收集和摺疊從 layout 拆出來、保證結果一樣；量到成本是問題時再改）、O3（每次查詢重算：CodeMirror 一次 measure 量好幾次，太慢，也可能和 layout 不一致）。
5. **W1：JS 和 wasm 都給**（2026-10-12）：RFC 0012 的第 7 條的預設；只新增，`kun:dom` 不升版。不選 W2（先只給 JS：沒有特別的成本當例外的理由）。
6. **T1：分兩段**（2026-10-12）：1 `Selection`（DOM 線的檔案，可以先 merge）；2 `Range` 的幾何（`layout`、`frame_view`，CSS 線的檔案，開始前協調），做完時重跑兩個 Editor。不選 T2（一段做完：太大，CSS 線的協調卡住整份）、T3（先做幾何：兩個 Editor 先停在 `getSelection`，看不出效果）。

7. **A1：第 1 段分三步**（2026-10-12）：1a 資料和方法（JS）、1b `selectionchange` 和 editing host 的 focus、1c wasm、WPT 和文件。
8. **B1：`Selection.toString()` 是 range 的 `toString()`**（2026-10-12）：一般的文字和 Chrome 一樣，`display: none` 的文字和 `<br>` 的換行不同（記在 `docs/limitations.md`）。不選 B2（順便做 `innerText` 的 rendered text：要 computed style 和 layout，L 級、另一條線的範圍；之後做了 `innerText` 再改用它）。
9. **C1：選取的 range 被 script 移到 document 外時清空選取**（2026-10-12）：在 `Range` 的設定之後檢查，移回來也不恢復，照 Chrome 152 的實測和 WPT 的 `move-selection-range-into-different-root.tentative.html`。不選 C2（照規範的文字只在讀的時候看兩端在不在 document 裡：range 移回來時選取「復活」，和 Chrome 不同）。
10. **D1：`selectionchange` 照 Chrome**（2026-10-12）：選取真的改變才排；字元資料的調整（replace data、split、normalize）讓選取移動時排，插入、移除節點的調整不排（Chrome 152 的實測）。規範沒寫 DOM 改變時的行為。不選 D2（DOM 改變一律不排：編輯器靠它同步，少一次事件）、D3（任何調整都排：比 Chrome 多）。

11. **A1：文字的盒子從 fragment 算**（2026-10-12）：照 Blink 的 `FragmentItem` 和 `ShapeResultView`，text fragment 記下它是原文（UTF-16）的哪一段和每個 cluster 的位置，位置和畫出來的一樣，高度是字型的 ascent 加 descent（和 Chrome 一樣）。代價：每個 text fragment 多一張 cluster 的表。不選 A2（parley 的 `Selection::geometry`：快取的 fragment 時換行寬度可能不對、高度是 line box、`position: relative` 要另外補；CodeMirror 用這些盒子量行高和字寬）。
12. **B1：`LayoutClient` 多一個 `text_rects`**（2026-10-12）：text 節點和 UTF-16 的範圍，只更新到 layout；`dom_api` 照 CSSOM View 把元素的和文字的組起來。不選 B2（整個 range 交給 layout：layout 要懂 DOM 的 range 和 tree order）。
13. **C1：第 2 段分三步**（2026-10-12）：2a `layout`、2b 接到 script 和 WPT、2c Speedometer、文件和檢查。
14. **A：在這份修 animation frame callback 的順序**（2026-10-12，2c 時）：TipTap 的有效性卡在它（第 9 項）；修正不大（renderer 的 `run_animation_frame_callbacks` 和 `dom` 的 timers），bug 修正不用另寫 RFC，在 `0038` 上做、另一個 commit。不選 B（照當時的結果收尾、修正另外在 `main` 上做：M6 的結果要等下一次）。

## 待決定

無。WPT 收了 `selection/` 整個目錄（第 1 段試跑時決定），其他目錄只更新預期結果。

## 之後的可能

- P 的後兩份：編輯（contenteditable 的打字、`beforeinput`、`execCommand`、undo、輸入法，和文字控制項的編輯）、畫面（游標、反白、`::selection`、滑鼠選取、`caretPositionFromPoint`，和 CSS 線協調）。
- `Selection.modify()`、`getComposedRanges()`、文字控制項的 `selectionchange`、`selectSubString()`。
- offset 的對應改成需要時才建（Blink 的 `OffsetMapping`，O2）。
- `Selection.toString()` 和 `innerText` 的 rendered text（第 8 條）；SVG 的 `<text>` 裡的文字的盒子；在 `position: fixed` 的元素裡、parent 不是它的文字（和元素的 `getClientRects` 一起）。
- CodeMirror 的 DOM 和 Chrome 差 2 行：排版的差異（字型、行高），CSS 線；TipTap 量得到，是 Chrome 的 8.6 倍（Long 和 Highlight 的 sync 時間都慢 7 到 10 倍）。
- 盤點時看到、和這份無關的：兩個 Editor 用到的 `<div>`、`<span>`、`<p>`、`<br>`、`<h1>`、`<ul>`、`<li>`、`<blockquote>` 在 kun 沒有自己的 interface（`HTMLDivElement` 等，現在都是 `HTMLElement`）；ProseMirror 看 `style.overflowAnchor` 不存在時每次更新都用 `elementFromPoint` 每 5 px 探一次（Chrome 有這個 property，不探）。
