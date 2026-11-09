# RFC 0038 的細節：`Selection` 和 `Range` 的幾何

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## Speedometer 用到的

2026-10-12，`python3 scripts/ci.py speedometer --inventory --suites Editor-CodeMirror,Editor-TipTap`（Chrome for Testing 152.0.7977.82 記錄、kun `86dc35bc` 回答）：

| suite | kun 的例外 | kun 缺的 API |
| --- | --- | --- |
| Editor-CodeMirror | `TypeError: target.getSelection is not a function` | `Document.getSelection`、`Range.getClientRects`、`Selection.anchorNode`、`anchorOffset`、`focusNode`、`focusOffset`、`Navigator.scheduling` |
| Editor-TipTap | `TypeError: Cannot read properties of null (reading 'getSelection')` | 同上（沒有 `Navigator.scheduling`），加上 `Selection.collapse` |

兩個 suite 的步驟（`../third_party/speedometer/resources/tests.mjs`）都是點 `#create`、`#layout`、`#long`、`#layout`，再點 `#highlight`、`#layout`；`#layout` 只呼叫 `document.body.getBoundingClientRect()` 和 `elementFromPoint`。頁面把 `requestAnimationFrame` 換成 `setTimeout(cb, 0)`。版本：`@codemirror/view` 6.9.1、`prosemirror-view` 1.30.1、`@tiptap/core` 2.0.0-beta.217（從 `dist/assets/` 的 source map 讀原始碼）。kun 的 user agent 有 `Chrome/`，兩個編輯器都走 Chrome 的路徑。

**為什麼 DOM 和 Chrome 不同**：
- **CodeMirror**：`EditorView` 的建構子裡 `new DOMObserver(this)` 最後呼叫 `readSelectionRange()`，經過 `getSelection(view.root)`（root 是 `document`）丟例外，這時還沒有 `parent.appendChild(this.dom)`，編輯器整個沒有建起來，之後的步驟對 `null` 丟例外。Chrome 的 `<br>` 是 `LineView.sync` 在 viewport 裡的每個空行加的；viewport 由非同步的 `EditorView.measure()` 量出來（`ViewState.measure` → `DocView.measureTextSize` → `LineView.measureTextSize` → `clientRectsFor` → `textRange().getClientRects()`，沒有 catch），只有 `getSelection` 沒有幾何的話，measure 丟例外，viewport 停在估計的行高，畫的行和 Chrome 不同。
- **TipTap**：`Editor.createView()` 的 `view.updateState` 在 Chrome 的路徑上讀 `this.domSelectionRange().focusNode`（`chromeKludge`）；`root` 的 getter 看到 `getSelection` 不存在，自己裝上 `Document.prototype.getSelection = () => search.ownerDocument.getSelection()`，在 `document` 上 `ownerDocument` 是 `null`，所以是這個錯誤訊息。`new Editor` 丟例外，`#long` 沒有 `setContent`，`<blockquote>` 是 `longtext` 的內容，沒有放進去。

**路徑上用到的**：
- `Selection`：兩個都讀 `anchorNode`、`anchorOffset`、`focusNode`、`focusOffset`（CodeMirror 在建構、每次 `update()`、每次 `measure()`；TipTap 在 `updateStateInner`、`setCurSelection`）。CodeMirror 沒有 focus，不寫選取（`mayControlSelection()` 是 false）。TipTap 在 `focus()` 之後的 `selectionToDOM` → `ViewDesc.setSelection` 先比較，不同才呼叫 `collapse(node, offset)`（在 `try` 外面，不能丟例外）；選取一直是 collapse 的，不呼叫 `extend`、`removeAllRanges`、`addRange`。讀回來的值只決定要不要省掉多餘的 `collapse`，DOM 不依賴它。
- focus：TipTap 的 `focus()` 命令（`delayedFocus` → `setTimeout(0)` → `view.focus()` → `dom.focus({preventScroll})`），編輯器是 `<div contenteditable="true" tabindex="0">`；ProseMirror 的 `hasFocus()` 是 `root.activeElement == view.dom`，`focus` 事件讓它加上 `ProseMirror-focused` 的 class。CodeMirror 讀 `document.hasFocus()`、`activeElement`。
- 幾何（都在步驟之後的 `setTimeout` 裡，不在 `#layout` 裡）：CodeMirror 的 `measureTextSize`（沒有 catch）、游標那一層的 `coordsAtPos(0)` → `textCoords` → `textRange().getClientRects()`（`logException` 接住，失敗時不畫 `.cm-cursor-primary`）；Chrome 的路徑上 CodeMirror 預期 collapse 的 text range 回傳一個寬度 0 的盒子。TipTap 在 `delayedFocus` 之後的 `scrollIntoView()` → `scrollToSelection` → `coordsAtPos` → `singleRect(textRange(node, 0, 1))`（`getClientRects`，沒有時 `getBoundingClientRect`），再用 `getBoundingClientRect`、`offset*`、`client*`、`scrollTop` / `scrollLeft` 捲。
- 其他：`getComputedStyle`（`whiteSpace` 要是 `pre-wrap`、`direction`、padding）、`MutationObserver`（兩個都在自己寫 DOM 的前後 `disconnect`，路徑上不觸發）、`ResizeObserver`、`IntersectionObserver`（CodeMirror）、`createEvent("Event")`、`DOMParser`、`matches`、`createRange`、`<style>` 的 `insertRule`。ProseMirror 的 `storeScrollPos` 在 `style.overflowAnchor` 不存在時才跑：kun 沒有這個 property，每次更新都用 `elementFromPoint` 每 5 px 探一次，Chrome 不探。

**登記了但路徑上沒有觸發的**：`selectionchange`（兩個都在 document 上聽；Chrome 在 TipTap 的 `collapse` 之後分派，但選取一樣，什麼都不做）、`keydown`、`beforeinput`、composition、剪貼簿、拖放、滑鼠的 listener。沒有用到的：`rangeCount`、`getRangeAt`（CodeMirror 只在拖曳時）、`modify`、`extend`、`removeAllRanges`、`addRange`、`execCommand`、`caretRangeFromPoint`、`caretPositionFromPoint`、`isCollapsed`、`type`、`direction`、`containsNode`、`setBaseAndExtent`、`getComposedRanges`、`isContentEditable`、`EditContext`。

**結論**：兩個 suite 要跑完、不丟例外，最少要 `Document.getSelection` 的 anchor 和 focus、不丟例外的 `collapse`，和 `Range.getClientRects` / `getBoundingClientRect`；要和 Chrome 的 DOM 一樣，另外要 CodeMirror 量得到（viewport 一樣、畫出游標）、editing host 的 `focus()` 和 `focus` 事件。`selectionchange`、選取的其他方法對這兩個 suite 不是必要的，但這份照規範做完整的 Selection API（`README.md` 的目標）：只做一部分的話，其他編輯器和 WPT 碰到的是「有 `getSelection` 但方法不存在」，比沒有更難判斷。

## kun 的現況

2026-10-12，`main` 的 `86dc35bc`。

- **`Range`**（`crates/dom/src/range.rs`）：`Range { start, end: Edge<Node>, start_offset, end_offset, is_static }`；live range 在它的 start 的 document 的 rare data 的弱集合（`NodeRareData::live_ranges`），樹和字元資料的原語調整它們（`live_ranges_inserted`、`live_ranges_pre_removing`、`live_ranges_replaced`、`live_ranges_split`、`merge_text_ranges`、`move_live_ranges`）。弱集合不撐住 range，選取要自己撐住。`dom_api` 的 `range` 模組有 boundary point 的位置、設定、比較、內容的演算法。
- **文字控制項的選取**（`crates/dom/src/form_control.rs` 的 `Selection { start, end, direction }`，`crates/dom_api/src/text_control.rs`）：每個控制項自己的 UTF-16 範圍，`setSelectionRange`、`select()`、`setRangeText`；`crates/dom_api/src/editing.rs` 是最小的編輯（WebDriver 的 Send Keys、使用者的按鍵）。和 document 的選取無關；Chrome 的文字控制項有 focus 時，`FrameSelection` 在它的 shadow tree 裡，`getSelection()` 回報的是控制項在 parent 裡的位置。這份不動它們。
- **focus**（`crates/dom_api/src/focus.rs`）：`supports_focus` 看合法的 `tabindex`、`focusable_by_default`（`<a href>`、表單控制項等）；沒有 `contenteditable`。`HTMLElement` 的 `contentEditable`、`isContentEditable` 只是 IDL attribute（`html_element.rs`，從 DOM 的 attribute 算，不看 `-webkit-user-modify`）。
- **幾何**：`LayoutClient`（`crates/dom/src/layout_client.rs`）的 `client_rects`、`bounding_client_rect` 收元素的 `NodeId`；renderer 的 `FrameTreeLayout` 經過 `FrameView` 到 `layout/src/geometry.rs`（元素的 border box，或每一行的 inline box）。
- **inline 的文字**（`crates/layout/src/text.rs`）：`InlineItem::Text(String, style, Option<NodeId>)` 記的是所在的**元素**，同一個元素的好幾個 text 節點分不開；`collapse_white_space` 逐字元記下來自哪個 item（`chars: Vec<(char, usize)>`），之後組成排版用的字串；`InlineContent` 撐住 parley 的 `Layout<u32>`（brush 是 style 的編號）。`TextFragment`（`fragment.rs`）有元素、glyph 和位置，沒有 text 節點和原文的 offset。hit test 只回答元素。唯一把 UTF-16 對到幾何的是 SVG 的文字（`svg_text_chars`）。
- **parley**（`../third_party/parley`）：`editing::Selection::geometry(&layout)` 給一段 byte 範圍在每一行的盒子，`Cursor::from_byte_index`、`Cursor::geometry` 給游標的盒子，`Cluster::from_point` 和 `Cursor::from_point` 是之後滑鼠和 `caretPositionFromPoint` 要的。shell 的網址列（`omnibox.rs`）用 parley 的 `PlainEditor` 編輯，是 kun 裡唯一畫游標和選取的地方。

## Chrome 的實測

2026-10-12，Chrome for Testing 152.0.7977.82（`ci.py speedometer` 下載的，`--headless --dump-dom --virtual-time-budget`），一頁探測的 script 在每個動作前後各等一個 `setTimeout(0)` 數 `selectionchange`：

| 動作 | 結果 |
| --- | --- |
| `collapse(div, 2)` 之後在它前面插入、移除子節點 | offset 跟著變（3、2），**不**分派 `selectionchange` |
| `collapse(text, 3)` 之後 `deleteData(0, 1)`、`splitText(1)`、`normalize()`、設 `data` | 位置跟著變，**分派** |
| 改選取位置之後的資料（`appendData`） | 位置不變，不分派 |
| 同樣的參數再 `collapse` 一次 | 不分派 |
| `getRangeAt(0).setEnd(...)` | 選取跟著變、分派，`getRangeAt(0)` 仍是同一個物件；之後 `collapse` 換成新的 range |
| 選取的 range `setStart` 到 fragment 裡的節點 | `rangeCount` 變 0；range 移回 document 也不恢復 |
| 移除選取所在的元素 | 選取移到 parent，不分派 |
| `<div contenteditable>`（沒有 `tabindex`）`focus()` | `activeElement` 是它，`tabIndex` 是 -1；選取在外面時移到它的開頭（text, 0），分派；選取已經在裡面時不動 |
| `selectAllChildren(div)` 的 `toString()` | 和 range 的 `toString()` 一樣（一般的文字） |
| collapse 在文字裡的 range 的 `getClientRects()` | 1 個，寬 0、高 17（行高） |
| `createHTMLDocument('')` 的 `getSelection()` | `null` |
| `collapse` 到不在 document 裡的節點 | 不變（`rangeCount` 仍是 1） |
| 已經有 range 時 `addRange`；`addRange` 一個不在 document 裡的 range | 都不做事 |
| `collapse` 到 `display: none` 的文字裡 | 照存 |
| `collapse(div, 0)` 再 `extend(text, 1)`（text 在 div 前面） | `direction` 是 `backward`，`type` 是 `Range` |
| `window.getSelection() === document.getSelection()` | true |
| `direction`：`setBaseAndExtent` 給 focus 在前的；之後 `collapse`、`collapseToStart` / `End`、`setBaseAndExtent` 給同一點 | `backward`；都是 `none`（collapse 的選取一律 `none`） |
| `removeAllRanges` 之後 `addRange`；backward 的選取的 range 被 `setStart` | 都是 `forward`（規範是保留方向） |
| `containsNode`：`selectAllChildren(div)` 之後的子元素、`div` 本身、`div` 加 `allowPartialContainment`；游標在 text 裡時那個 text | true、false、true；true（部分在裡面的 text 節點也算） |
| 錯誤：`extend` 和 `collapseToStart` 的空選取、`getRangeAt` 超出、`removeRange` 別的 range、`containsNode(null)` | `InvalidStateError`、`IndexSizeError`、`NotFoundError`、`TypeError` |

## 選項

### P：範圍

- **P1：拆成三份，這份做選取和幾何**（選的）。
  - 優點：兩個 Editor（M6）只要這份；這份主要是 DOM 線的檔案，只有幾何碰到 `layout`；後兩份（編輯、畫面）的設計可以等這份的資料模型定下來。
  - 缺點：路線圖的 P 變成三份 RFC；這份做完時，使用者看到的 kun 沒有改變。
- **P2：一份 RFC，四段**（選取、幾何、編輯、畫面）。
  - 優點：P 的設計（正規化、編輯指令、繪製）一次想清楚。
  - 缺點：XL，M6 要等前兩段之後的 merge 才看得到；後兩段大半在 CSS 線，一份 RFC 跨兩條線太久。
- **P3：只做 Speedometer 要的最小部分**（`getSelection`、anchor 和 focus、`collapse`、`getClientRects`）。
  - 優點：最快解開 M6。
  - 缺點：只有一部分的 `Selection` 讓其他頁面「有 `getSelection` 但方法不存在」；WPT 的 `selection/` 大半不能收；之後補的時候資料模型可能要重來。

### S：`Selection` 的資料

- **S1：規範的模型，撐住一個 live `Range` 和方向**（選的）。
  - 優點：RFC 0037 已經照規範調整 live range，選取不用自己的 hook；`getRangeAt(0)` 就是它，script 改 range 就是改選取，不用同步；GC 的關係簡單（document → selection → range）。
  - 缺點：之後編輯和繪製要正規化的位置時另外算；選取 range 被 script 改到另一棵樹要在 `Range` 的設定裡檢查。
- **S2：照 Blink，選取存 position、`getRangeAt` 建快取的 range 再同步**（`FrameSelection`、`SelectionEditor`、`Range::UpdateSelectionIfAddedToSelection`）。
  - 優點：和 Chromium 的分層一樣，之後的編輯直接接上。
  - 缺點：position 在樹改變時要另一套調整（Blink 的 `SelectionEditor` 是 `SynchronousMutationObserver`），和 live range 的調整重複；同步的兩個方向都要測。Blink 這樣做有歷史的原因（選取比 Selection API 早，`Range` 後來才接上）。
- **S3：選取直接存兩個 boundary point（不經過 `Range`），`getRangeAt` 每次建新的**。
  - 缺點：不合規範（`getRangeAt` 要回傳同一個物件，WPT 測這個），而且調整也要另寫。

### F：editing host 的 focus

- **F1：照 Chrome，editing host 能 focus，focus 進去時選取不在裡面就放到開頭**（選的）。
  - 優點：沒有 `tabindex` 的 `contenteditable` 元素能 `focus()`（HTML 規範也列 editing host 是 focusable area）；focus 之後 `getSelection()` 在編輯器裡，和 Chrome 一樣，之後的編輯直接用。
  - 缺點：focus 改變選取會分派 `selectionchange`，和 Chrome 一樣，但這份裡使用者看不到選取。
- **F2：這份不動 focus，留給編輯那一份**。
  - 優點：這份更小。
  - 缺點：editing host 能不能 focus 是 script 看得到的行為，和 `Selection` 一起才完整；TipTap 有 `tabindex`，其他編輯器（例如沒有 `tabindex` 的 `contenteditable`）`focus()` 沒有作用。

### O：DOM 的 offset 對到排好的文字

- **O1：layout 時一律建對應表，跟著 `InlineContent` 存**（選的）。
  - 優點：空白摺疊已經逐字元記下來源，順手記下 text 節點和 UTF-16 offset；查詢時不重算；layout 和查詢用同一份，不會不一致。
  - 缺點：每段 inline 文字多一張表（每個 text 節點一筆，加上摺掉的空白的範圍），沒用到的頁面也付。
- **O2：照 Blink，需要時才建、快取到下一次 layout**（`OffsetMapping::GetFor`、`InlineNodeData` 的快取）。
  - 優點：沒用到的頁面不付記憶體。
  - 缺點：要能在 layout 之外重跑收集和摺疊、結果和 layout 時一模一樣（Blink 用同一個 `InlineItemsBuilder` 的 template 做到）；kun 的收集和摺疊連在 layout 裡，要先拆出來。
- **O3：每次查詢從 DOM 重算**。
  - 缺點：CodeMirror 每次 measure 都量好幾次，重算整個 inline formatting context 太慢；結果也可能和 layout 不一致。

### T：分段

- **T1：兩段，`Selection`，再幾何**（選的）：第 1 段只改 DOM 線的檔案、可以先 merge；第 2 段改 CSS 線的檔案，開始前協調。
- **T2：一段做完**：兩個 Editor 要兩者都有才跑得完，但一段太大，而且 CSS 線的協調卡住整份。
- **T3：先做幾何**：幾何單獨有用（`Range.getClientRects` 本身是 CSSOM View 的 API），但兩個 Editor 先停在 `getSelection`，看不出效果。

### A：第 1 段的步驟

- **A1：三步**（選的，第 7 條）：1a 資料和方法（JS）、1b `selectionchange` 和 editing host 的 focus、1c wasm、WPT 和文件。每步都能單獨測、單獨審。

### B：`Selection.toString()`

- **B1：range 的 `toString()`**（選的，第 8 條）：一般的文字和 Chrome 一樣；不同在看不見的文字和 `<br>`。規範沒有演算法，WPT 只有 `.tentative`。
- **B2：順便做 `innerText` 的 rendered text，兩者共用**：和 Chrome 一樣，但要 computed style（`display`、`visibility`）和 layout（being rendered），是 L 級的工作，屬於另一條線。

### C：選取的 range 被 script 移到另一棵樹

- **C1：在 `Range` 的設定之後清空選取**（選的，第 9 條）：Chrome 152 的實測，WPT 的 `move-selection-range-into-different-root.tentative.html`。
- **C2：只在讀的時候看兩端在不在 document 裡**（規範的文字）：range 移回來時選取又出現，和 Chrome 不同。

### D：樹改變讓選取移動時的 `selectionchange`

- **D1：照 Chrome**（選的，第 10 條）：字元資料的調整讓選取移動時排，插入、移除節點的不排；選取真的改變才排。
- **D2：DOM 改變一律不排**：最簡單；編輯器在字元資料改變時少一次事件。
- **D3：任何調整都排**：比 Chrome 多分派。

### W：wasm

- **W1：都給**（選的，第 5 條；RFC 0012 的第 7 條的預設）：`selection` 的 resource、`get-selection`、range 的幾何。
- **W2：先只給 JS**：選取對 wasm 的 guest 用處小（guest 多半自己管編輯），但規則是例外要有理由，這裡沒有特別的成本。

## Chromium 的做法

依記憶整理，實作前對原始碼確認（`third_party/blink/renderer/core/editing/`、`core/dom/range.cc`、`core/layout/inline/`）：
- **`DOMSelection`**（`dom_selection.cc`）是 script 的 `Selection`，每個 `TreeScope` 一個（`TreeScope::getSelection`，所以 `ShadowRoot.getSelection` 是 Chrome 自己的），轉給 `LocalFrame` 的 `FrameSelection`。anchor 和 focus 是 `SelectionInDOMTree` 的 base 和 extent，不正規化（以前會，後來為了 Selection API 改掉）；shadow tree 裡的位置照 tree scope 往外移（`DOMSelection::ShadowAdjustedNode`）。
- **`FrameSelection`**（`frame_selection.cc`）、**`SelectionEditor`**（`selection_editor.cc`）：存選取，`SelectionEditor` 是 `SynchronousMutationObserver`，在節點移除、字元資料改變、split、merge 時調整 position；`ComputeVisibleSelectionInDOMTree` 要排版時才算正規化。
- **`getRangeAt`**：`FrameSelection::DocumentCachedRange` 快取一個 `Range`，`Range` 有 `owner_document_` 和「是不是選取的」的標記，`Range::UpdateSelectionIfAddedToSelection` 在 `setStart` 等之後把 range 寫回選取。
- **`selectionchange`**：`Document::ScheduleSelectionchangeEvent`（或 `FrameSelection::DidSetSelectionDeprecated` 經過 `EventQueue`），一個旗標加一個 task。
- **focus**：`Element::SupportsFocus` 對 editing host（`IsRootEditableElement`）是 true；`Element::focus` 之後 `UpdateSelectionOnFocus`：選取不在元素裡就放到開頭（`SelectionBehaviorOnFocus::kSet`）。
- **幾何**：`Range::getClientRects` → `GetBorderAndTextQuads`：整個包含的元素用 `AbsoluteQuads`，text 用 `LayoutText::AbsoluteQuadsForRange(start, end)`：`OffsetMapping`（DOM 的 offset 對到 inline formatting context 的 text content 的 offset，摺掉的空白對到同一個位置），再用 `InlineCursor` 走 `FragmentItem`，每個 text 的 fragment 用 `ShapeResultView` 算 `LocalRect(start, end)`。collapse 的範圍在文字裡給一個寬度 0 的 quad。
- **編輯的其他部分**（之後的兩份）：`Editor`、`EditCommand` 的系列（`TypingCommand`、`InsertTextCommand`、`DeleteSelectionCommand`）、`UndoStack`、`InputMethodController`；`FrameCaret`、`CaretDisplayItemClient`、`LayoutSelection`、`HighlightPainter`。

## 參考資料

- W3C Selection API：https://w3c.github.io/selection-api/
- CSSOM View 的 `Range` 的擴充：https://drafts.csswg.org/cssom-view/#extensions-to-the-range-interface
- HTML 的 editing host 和 focusable area：https://html.spec.whatwg.org/multipage/interaction.html#focusable-area
- WPT：`selection/`（110 個檔案和 `anonymous`、`bidi`、`caret`、`contenteditable`、`crashtests`、`shadow-dom`、`textcontrols` 七個子目錄）、`css/cssom-view/range-*`。
