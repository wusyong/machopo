# RFC 0037：`Range` 和 `StaticRange`

- 開始日期：2026-10-12
- 狀態：✅ 已完成：2026-10-12（第 1 到 4 條開始前接受，第 5 條在第 1 段、第 6 條在第 2 段開始前定下；兩段都在 Fedora 44（Linux x86_64）上做，第 1 段做完時先 merge；動到 GC 而要跑的 `test.py matrix`、`fuzz` 在 merge 之後看 CI；驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 I，DOM 線接在 K-1（RFC 0035）之後、P（編輯）之前。原本是 RFC 0010 的第五組，那份的第 19 條把它移出來，留下「移除之前」的 hook 給它沿用。

## 摘要

加上 DOM 的 `AbstractRange`、`Range`、`StaticRange` 和 `document.createRange()`：boundary point、比較、`selectNode` 等設定、`deleteContents`、`extractContents`、`cloneContents`、`insertNode`、`surroundContents`、`toString`，和 HTML 的 `createContextualFragment`。`Range` 是 live 的：每個修改樹和字元資料的 `dom` 原語照 DOM 規範調整 document 裡的每個 live range（插入、移除、replace data、`splitText`、`normalize`），所以字元資料的「replace data」和 `splitText` 搬進 `dom`。JS 和 wasm 都給。`Selection` 是 P，`Range` 的幾何（`getClientRects`）也不在這份（第 1 條）。

## 動機

**現況（2026-10-12，`main` 的 `5c8298aa`）**：
- 沒有 `Range`、`StaticRange`、`document.createRange()`、`createContextualFragment`（`docs/limitations.md` 的「Bindings 和 DOM API」裡「沒有 `Range`、`StaticRange`」那一條）；`spec-missing.txt` 有 `Document.createRange()`。
- **Speedometer**（`ci.py speedometer --inventory --suites Editor-CodeMirror,Editor-TipTap`，2026-10-12，Chrome for Testing 152）：兩個 Editor 跑完但有例外（`getSelection`），是 M6 的兩個 suite。它們用到的 `Range` 只有 `createRange`、`setStart`、`setEnd`、`getClientRects`；`Selection` 的 `anchorNode`、`anchorOffset`、`focusNode`、`focusOffset`、`collapse`（TipTap），由 `Document.getSelection` 拿到。`Range` 本身小，但它是 `Selection`（P）的資料模型，`Selection` 的 anchor 和 focus 就是一個 live range 的兩端。
- **`dom` 的現況**（`details.md` 的「`dom` 的現況」）：live 的物件只有 `NodeIterator`，document 的 rare data 有它們的弱集合（`WeakSet`），`detach_with` 在拆開節點之前呼叫它們的 pre-removing steps（RFC 0010 的 4a）；插入、字元資料沒有 hook。DOM 的「replace data」在 `dom_api`（整串重建再 `set_character_data`，offset 不經過 `dom`），`splitText`（先插入新節點、再截短，和規範的順序一樣，只是中間沒有調整 range）、`normalize` 也在 `dom_api`，兩處都註明「還沒有 live range」。子節點是雙向的 linked list，節點在 parent 裡的 index 要數兄弟。
- WPT：`dom/ranges/` 沒有匯入；`MutationObserver` 的 11 個 subtest、`Node-normalize-2`、`custom-elements/reactions/Range`、`shadow-dom/Range-prototype-insertNode` 等在等它（RFC 0010 的 `details.md`）。

目標：
- `AbstractRange`（`startContainer`、`startOffset`、`endContainer`、`endOffset`、`collapsed`）、`StaticRange`（建構子和它的檢查，不跟著樹改）、`Range`（建構子、`commonAncestorContainer`、`setStart` / `setEnd` / `setStartBefore` 等、`collapse`、`selectNode`、`selectNodeContents`、`compareBoundaryPoints`、`deleteContents`、`extractContents`、`cloneContents`、`insertNode`、`surroundContents`、`cloneRange`、`detach`、`isPointInRange`、`comparePoint`、`intersectsNode`、`toString`）、`document.createRange()`、`createContextualFragment`。
- live range：DOM 規範在插入、移除、replace data、`splitText`、`normalize` 裡的每一步調整；跨 document 的 adopt 讓 boundary point 回到根（規範的移除步驟）。
- JS 和 wasm 的 guest 都有（RFC 0012 的第 7 條）。
- WPT 的 `dom/ranges/` 和等它的 subtest。

不在範圍內：
- `Selection`、`getSelection()`、`selectionchange`、`contenteditable`、游標（P）。
- `getComposedRanges`、shadow root 裡的 range 的特別規則以外的 shadow DOM 部分（`Selection` 的事，P）。
- `moveBefore`（kun 沒有）。
- `Range` 的幾何（`getClientRects`、`getBoundingClientRect`，CSSOM View）：要 `layout` 記下 text 節點和原文的 offset，和 `Selection` 一起排進 P 之前、和 CSS 線協調的一份（第 1 條，G1）。

## 設計概要

對寫網頁的人：

```js
const range = document.createRange();
range.setStart(textNode, 2);
range.setEnd(textNode, 5);
textNode.insertData(0, "ab");        // live：start 和 end 跟著往後移兩格
range.toString();                    // 照樣是原本那三個字
range.extractContents();             // 移到一個 DocumentFragment，觸發 MutationObserver 和 custom element 的 reaction
new StaticRange({ startContainer: p, startOffset: 0, endContainer: p, endOffset: 1 }); // 不跟著改
```

和 Chrome 一樣的地方：offset 是 UTF-16 的單位；range 在 document 改變之後照規範移動，被移除的節點裡的 boundary point 移到被移除的節點原本的位置；`StaticRange` 的 offset 可以超出範圍（建構時只檢查 doctype 和 attr）。

對改 kun 的人：
- `dom`：`Range` 和 `StaticRange` 是新的 GC 型別（兩個 boundary point：traced 的 container edge 和 offset），live range 登記在 document 的 rare data 的弱集合（照 `NodeIterator`）；插入、移除（`detach_with`）、replace data、split、normalize 的合併是 `dom` 的原語，各自在規範說的那一步調整 live range。
- `dom_api`：新的 `range` 模組：boundary point 的比較（tree order）、設定和檢查、內容的演算法（`deleteContents` 等照規範用既有的插入、移除、replace data 組成，所以 `MutationObserver` 的紀錄和 custom element 的 reaction 照舊）；`CharacterData` 的成員和 `splitText`、`normalize` 改呼叫 `dom` 的新原語。
- `dom_js`、`dom_wasm`：宣告表的新 interface，WIT 只加不改。

## 詳細設計

（比較在 `details.md` 的「選項」，項目的編號對應它；決定在「已決定的事項」。）

1. **資料**（L）：`Range` 是 script 物件（`ObjectTag::Range`，有 wrapper），兩個 boundary point 各是 traced 的 `Edge<Node>` 加上 `#[no_trace]` 的 offset；`StaticRange` 一樣但不登記。建立的 `Range` 加進它的 start container 的 node document 的 rare data 的弱集合（`live_ranges: WeakSet<Range>`），document 不撐住它（Blink 的 `Document::ranges_`，`HeapHashSet<WeakMember<Range>>`）。boundary point 換到另一份 document 的節點時（`setStart` 給別的 document 的節點、adopt），從舊的集合移到新的。現在的 `WeakSet` 每次加入線性地找空格、只適合小的集合；CodeMirror 每次量位置都建一個 range，GC 之前集合會變大，所以 range 用 `WeakSet::insert_unordered`：從上次拿走的位置往後找空格，找過一遍都沒有時先往後加，等集合長到兩倍再找（原本的 `insert` 照第一個空格放，slot 的 manual assignment 靠這個順序，不改）；量到瓶頸再換雜湊集合。第 2 條（L1）。
2. **boundary point 的表示**（B）：container 加 offset（規範的定義）；調整時要「節點在 parent 裡的 index」，只在有 live range 的 container 是那個 parent 時才數兄弟（先比對，沒有就不數）。Blink 的 `RangeBoundaryPoint` 另外記 offset 之前的子節點、offset 用到時才算，插入和移除不用數；第 3 條（B1：只記 offset），量到再改。
3. **live range 的調整**（DOM 規範在各個演算法裡寫的步驟，全部在 `dom`）：
   - **插入**（`insert_before` 把節點接上之後，對每個插進來的節點）：container 是 parent、offset 大於 index 的加 1（照規範的「insert」，加上插入的節點數）。
   - **移除**（`detach_with` 拆開之前，和 `NodeIterator` 的 pre-removing steps 同一處）：boundary point 在被移除的節點裡（inclusive descendant）的移到 (parent, index)；container 是 parent、offset 大於 index 的減 1。
   - **replace data**（新的 `dom` 原語 `replace_data(node, offset, count, data)`，UTF-16）：照規範的「replace data」調整 container 是那個節點的 boundary point，再改資料、記 `MutationObserver` 的紀錄（現在的 `set_character_data` 變成它的特例）。
   - **split a Text node**（新的原語 `split_text`）：規範的順序：建新節點、插入、調整（原節點裡超過 offset 的移到新節點、parent 裡在它後面一格的往後一格），再 replace data；原本 `dom_api` 的順序已經是這樣，加上中間的調整。
   - **normalize**（`dom` 的 `merge_text_ranges`，一次一個被併的 text）：被併的 text 裡的 boundary point 移到 run 的第一個（加上之前的長度），parent 裡在它的位置上的移到第一個的那個長度；`dom_api` 的 `normalize` 改成在第一個的尾端 replace data（原本設整串，會把第一個自己的 boundary point 移到 0），移完才移除被併的。
   - **adopt**：節點在 document 裡時先經過移除，boundary point 已經移出去；detached 的子樹整個被 adopt 時，range 的 start 跟著到了新的 document，`adopt` 之後的 `move_live_ranges`（和 `move_node_iterators` 同樣兩處）把它搬到新 document 的集合，之後那份 document 的修改才調整得到它。
4. **比較和設定**（`dom_api` 的 `range` 模組）：boundary point 的位置（規範的「position of a boundary point」）用祖先鏈和兄弟的順序，和 `compareDocumentPosition` 共用一個 `dom_api` 的輔助函式（`tree_order`）；宣告表沒有 16 位元的整數，`compareBoundaryPoints` 的 `how`（`unsigned short`）宣告成 `unsigned long`、取 2^16 的餘數（WebIDL 的 ToUint16 是 ToUint32 再取餘數，JS 看到的一樣），回傳的 `short` 宣告成 `long`，WIT 的型別寬一點（1d）；`setStart` / `setEnd` 的檢查（doctype、offset 超出長度、根不同時 collapse）、`collapsed`、`commonAncestorContainer`。
5. **內容的演算法**（`deleteContents`、`extractContents`、`cloneContents`、`insertNode`、`surroundContents`）：照規範用既有的插入、移除、replace data、clone 組成，在 `dom_api`；它們本來就產生 `MutationObserver` 的紀錄，宣告表標 `[CEReactions]`（`spec::ce_reactions_as_the_specs` 比對）。「extract」和「clone the contents」是一個函式：開始時讀下原本的 boundary point 和部分、完全選到的子節點；遞迴處理部分選到的節點時直接傳 boundary point，不另建 live range（規範的 subrange 只用到它一開始的 boundary point）。錯誤（doctype、部分選到的非 text 節點、新的 parent 的種類）都在動任何東西之前檢查。`createContextualFragment` 用 `markup` 的 fragment parsing，context 照規範從 start container 找；fragment 的 script 清掉 already started 和 parser（插進文件時執行，和 `innerHTML` 的不同）；fragment 裡已經定義的 custom element 在這次 `[CEReactions]` 的範圍裡 upgrade（規範的 fragment parsing 經過 context element 的 registry 找 definition，`html` 的 parser 不找，Blink 一樣當場建構）。
6. **wasm**（W）：`range`、`static-range`、`abstract-range` 的 resource 和 `create-range`，只加不改，`kun:dom` 不升版；`StaticRange` 的建構子收 record；新的例外 `InvalidNodeTypeError`、`WrongDocumentError` 是 22、23。Lua 的 generator 多了 record 裡必填的物件欄位（`StaticRangeInit` 的 container）：整個 table 沒給時是錯誤，不把 handle 0 傳給 host（會 trap、停掉整個 Lua runtime）。
7. **幾何**（G）：`Range.getClientRects`、`getBoundingClientRect` 要 text 從某個 offset 到某個 offset 的盒子；layout 的 `TextFragment` 現在只記元素、不記 text 節點和原文的 offset（`details.md`）。這份不做（第 1 條，G1），宣告表也不加這兩個成員。

### 測試

- `dom`：`Range` 和 `StaticRange` 的 GC（`script_objects::`、`script::every_type_unwraps_only_as_itself`、弱集合不撐住 range、空格重用）；每種修改的調整（插入、移除、adopt、replace data、split）；工作負載的 live range 在隨機的插入、移除和字元資料的修改之後，offset 不超過節點的長度。
- `dom_api`、`dom_js`：設定、比較、`toString`、內容的演算法、`createContextualFragment`、`MutationObserver` 的紀錄和 custom element 的 reaction、`StaticRange` 的檢查、`unsigned long` 的轉換；`shell` 的 `inserted_scripts` 測 `createContextualFragment` 的 script 插進文件時執行。
- `dom_wasm`、W9（Lua）：guest 建 range、設定、錯誤的代碼、`StaticRange`、`extract-contents`。
- WPT：`dom/ranges/` 整個收（`tentative/` 的 `OpaqueRange` `disabled`），`Selection` 的 subtest 照預期失敗；等它的 `dom/nodes/`、`domparsing/`、`custom-elements/`、`shadow-dom/`、`css/cssom-view/` 的預期結果更新。數字在 `progress.md`。
- mutation test：照 `docs/testing.md`，七份。
- 畫面：沒有看得到的改變。

### 分階段

第 4 條（T1）：
1. **資料和 live range**：`Range`、`StaticRange` 的 GC 型別和登記；`dom` 的 replace data、split、normalize 的原語和所有調整；設定、比較、`toString`、`cloneRange`、`detach`、`isPointInRange`、`comparePoint`、`intersectsNode`；`createRange`；JS 和 wasm。
2. **內容的演算法**：`deleteContents`、`extractContents`、`cloneContents`、`insertNode`、`surroundContents`、`createContextualFragment`；WPT 的 `dom/ranges/` 和其他目錄。

## 缺點

- **每次修改都要看 live range**：document 有 live range 時，插入、移除、改字元資料都要走一次集合；沒有 range 的 document（大多數頁面）只多一次檢查集合是不是空的。CodeMirror 這類建很多短命 range 的頁面，在 GC 之前集合裡有很多死掉、還沒回收的 range（Blink 一樣）。
- **index 要數兄弟**：有 range 的 container 很大時（長的清單），每次插入、移除都數一次（B1）。
- **字元資料的原語變多**：`replace_data`、`split_text`、合併 text 搬進 `dom`，`dom_api` 原本的整串重建要改。

## 理由和其他做法

各項的完整比較在 `details.md` 的「選項」。重點：
- 調整在 `dom` 的原語裡（第 3 項）：移除要用舊的 parent 和 index、replace data 要用 offset，只有原語知道；和 `MutationObserver` 的紀錄、`NodeIterator` 的 hook 同一個地方（RFC 0010 的第 12 條）。
- 弱集合放在 document（第 1 項）：照 `NodeIterator` 和 Blink；range 只活在 script 撐住的期間。
- 先不做 `Selection`：它有自己的模型（每個 document 一個、和 focus、編輯、繪製的關係），是 P；`Range` 是它的資料，先做這份。

## 先例

- **Chromium**（`details.md` 的「Chromium 的做法」，依記憶整理，實作前對原始碼確認）：`Range`（`core/dom/range.cc`）和 `RangeBoundaryPoint`（container、offset 之前的子節點、offset 用到時才算）；`Document::ranges_` 弱集合，`Document::NodeChildrenWillBeRemoved`、`NodeWillBeRemoved`、`DidInsertText`、`DidRemoveText`、`DidMergeTextNodes`、`DidSplitTextNode` 調整；`StaticRange`（`core/dom/static_range.cc`）；`Range::getClientRects` 經過 `LayoutText` 的 quads。
- **RFC 0010** 的 4a（`NodeIterator` 的 pre-removing steps、document 的弱集合）和第 19 條。

## 已決定的事項

2026-10-12 確認第 1 到 4 條（比較在 `details.md` 的「選項」）。

1. **G1：這份不做 `Range` 的幾何**（2026-10-12）：`getClientRects`、`getBoundingClientRect` 要 `layout` 的 text fragment 記下 text 節點和 UTF-16 的範圍（CSS 線的檔案），和 `Selection` 一起排進 P 之前、和 CSS 線協調的一份；兩個 Editor 不論如何都要等 `getSelection`，先做幾何解不開它們。不選 G2（這份最後一段做，Blink 的 `LayoutText::AbsoluteQuadsForRange`：這份變大、改到另一條線的檔案）、G3（只回傳整個 text 的盒子：結果錯、不丟例外，比沒有更難發現）。
2. **L1：live range 在 document 的 rare data 的弱集合**（2026-10-12）：照 `NodeIterator` 和 Blink 的 `Document::ranges_`；`WeakSet` 加上「下一個空格」的提示，CodeMirror 這類頁面量到瓶頸時再換雜湊集合。不選 L2（新的雜湊弱集合：新的 GC 容器和論證，走訪的成本一樣）、L3（heap 外的表：offset 和 container 在兩處，被回收的 range 要另外清）。
3. **B1：boundary point 是 container 和 offset**（2026-10-12）：照規範的定義，插入、移除時只在有 range 的 container 是那個 parent 時才數兄弟。不選 B2（Blink 的 `RangeBoundaryPoint` 記 offset 之前的子節點：多一條 traced 的 edge，讀 offset 要數），量到數兄弟是瓶頸時再改（「之後的可能」）。
4. **T1：分兩段**（2026-10-12）：1 資料、`dom` 的原語和 live range 的調整、設定和比較；2 內容的演算法和 WPT。不選 T2（一段做完：太大）、T3（先把 `dom` 的原語拆成一段：單獨看不出行為改變）。

第 1 段開始前，2026-10-12 確認第 5 條。

5. **第 1 段分四步**（2026-10-12）：1a `Range`、`StaticRange` 的 GC 型別和 document 的弱集合、`createRange`、兩個建構子、`AbstractRange` 的屬性、`setStart` 等設定、`collapse`、`selectNode`、`selectNodeContents`、`cloneRange`、`detach`，JS 和 wasm；1b 插入、移除（含 adopt）的調整；1c 字元資料：`dom` 的 `replace_data`（UTF-16）、`split_text`、`normalize` 的合併原語和它們的調整，`dom_api` 改呼叫它們；1d 比較（`compareBoundaryPoints`、`isPointInRange`、`comparePoint`、`intersectsNode`、`commonAncestorContainer`、`toString`，tree order 的輔助函式和 `compareDocumentPosition` 共用）、WPT 的試跑和收錄、文件、限制和第 1 段做完時的檢查。`moveBefore` 的規則不做（kun 沒有 `moveBefore`）。

第 2 段開始前，2026-10-12 確認第 6 條。

6. **第 2 段分三步**（2026-10-12）：2a `deleteContents`、`extractContents`、`cloneContents`（後兩者共用規範的「clone/extract the contents」：部分選到的字元資料用 replace data，部分選到的節點 clone 外殼、遞迴處理裡面），照規範用既有的插入、移除、clone 組成，`[CEReactions]` 照規範的 WebIDL；2b `insertNode`、`surroundContents`；2c `createContextualFragment`、WPT、文件、限制和第 2 段（也是這份 RFC）做完時的檢查。wasm 的 guest 一樣有，reaction 照舊走 backup element queue。

## 待決定

無。

## 之後的可能

- `Selection` 和編輯（P）。
- `Range` 的幾何（第 1 條），和 `Selection` 一起。
- `getComposedRanges`、`moveBefore` 的 live range 規則。
- boundary point 記 offset 之前的子節點（Blink 的 `RangeBoundaryPoint`），量到數兄弟是瓶頸時。
