# RFC 0037 的細節：`Range` 和 `StaticRange`

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## Speedometer 用到的（2026-10-12）

`python3 scripts/ci.py speedometer --inventory --suites Editor-CodeMirror,Editor-TipTap`（Chrome for Testing 152.0.7977.82 記錄、kun `5c8298aa` 回答），兩個 suite 在 kun 上跑完、有例外：

| suite | kun 的例外 | Chrome 裡用到的 `Range` 和 `Selection` |
| --- | --- | --- |
| Editor-CodeMirror | `TypeError: target.getSelection is not a function` | `Document.createRange`、`Range.setStart`、`Range.setEnd`、`Range.getClientRects`；`Document.getSelection`、`Selection.anchorNode`、`anchorOffset`、`focusNode`、`focusOffset` |
| Editor-TipTap | `TypeError: Cannot read properties of null (reading 'getSelection')` | 同上，加上 `Selection.collapse` |

CodeMirror 另外用 `IntersectionObserver`（RFC 0036）。兩個都沒有用到 `Range` 的內容演算法、`StaticRange`；它們用 range 量文字的位置（`getClientRects`），用 `Selection` 讀和設游標。

## `dom` 的現況

2026-10-12，`main` 的 `5c8298aa`。

- **live 的物件**：`NodeRareData::node_iterators: WeakSet<NodeIterator>`（`crates/dom/src/rare_data.rs`），在 document 的 rare data；`create_node_iterator` 加入。`detach_with`（`crates/dom/src/scope.rs`）在拆開節點之前呼叫 `node_iterators_pre_removing`，這時舊的 parent 和兄弟還連著，但沒有算 index；`adopt_node` 和 `insert_before` 換 document 時呼叫 `move_node_iterators`。插入和字元資料沒有 hook。
- **`WeakSet`**（`crates/dom/src/weak_set.rs`）：`EdgeChunks<WeakEdge<T>>`，加入時線性地找空格（註解寫「Small sets only」），`live()` 給 root 著的 `Vec`，移除也是線性的。
- **插入、移除**：`insert_before`（`append_child` 經過它）接上兄弟時知道前後的兄弟，不算 index；`remove`、`remove_without_record`、`detach`、`detach_with`。`replaceChildren`、`innerHTML`、fragment 的插入、`replaceChild` 都拆成這兩個原語，掛在它們上面就涵蓋全部。
- **字元資料**：`dom` 只有 `append_text`（parser）和 `set_character_data`，都經過 `text_changed`。DOM 的「replace data」在 `dom_api` 的 `character_data.rs`：整串 UTF-16 重建、再 `set_character_data`，offset 和 count 不經過 `dom`；`data`、`appendData`、`insertData`、`deleteData`、`replaceData`、`nodeValue`、`textContent` 都經過它。`splitText`：`create_text`、`insert_before`、`set_character_data`；`normalize`（`dom_api` 的 `node.rs`）：`set_character_data` 加 `remove`。兩處的註解寫「(No live ranges to update yet.)」。
- **子節點**：`first_child`、`last_child`、`previous_sibling`、`next_sibling` 的雙向 linked list，沒有 index 的快取；tree order 的比較只有 `dom_api` 的 `compare_document_position`（祖先鏈，再在共同的 parent 掃子節點）。
- **幾何**：`LayoutClient::client_rects`、`bounding_client_rect` 收元素的 `NodeId`；`layout` 的 `TextFragment` 記的是所在的元素，不是 text 節點，也沒有原文的 offset（只有 SVG 的 text 把 UTF-16 對應到幾何）。

## 選項

### L：live range 放在哪

| | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **L1** | document 的 rare data 的 `WeakSet<Range>`，照 `NodeIterator`；`WeakSet` 加上「下一個空格」的提示 | 沿用 4a 的做法和它的論證；不撐住 range | 移除線性；集合在 GC 之前會留著死掉的 range，每次修改都走過它們（Blink 一樣） |
| L2 | 新的雜湊弱集合（`HeapHashSet<WeakMember>` 的對應） | 加入、移除 O(1) | 新的 GC 容器和它的 tracing、論證；走訪的成本一樣 |
| L3 | heap 外的表（`DomState`，以 id 記 boundary point） | 修改時不碰 GC | range 的 offset 和 container 在兩個地方，script 讀的時候要對照；被回收的 range 要另外清 |

選了 L1（README 的第 2 條）；CodeMirror 量到集合大到成為瓶頸時換 L2。

### B：boundary point 的表示

| | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **B1** | container 和 offset（規範的定義） | 簡單；讀的時候不用算 | 插入、移除時要節點的 index：只在有 range 的 container 是那個 parent 時數兄弟 |
| B2 | Blink 的 `RangeBoundaryPoint`：container、offset 之前的子節點、offset 用到時才算 | 插入和移除只比對子節點，不數 | 多一條 traced 的 edge；`setStart(container, offset)` 要找 offset 的子節點；讀 offset 要數 |

選了 B1（README 的第 3 條），量到再改。

### G：`Range` 的幾何

| | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **G1** | 這份不做；和 `Selection` 一起排進 P 之前、CSS 線協調的一份 | 這份只碰 DOM 線的檔案；Editor 反正要等 `getSelection` | Editor 在這份之後照舊有例外 |
| G2 | 這份最後一段：`layout` 的 text fragment 記 text 節點和 UTF-16 的範圍，`LayoutClient` 多一個問 range 的方法（Blink 的 `LayoutText::AbsoluteQuadsForRange`） | 一次做對 | 改 `layout`、`renderer`（CSS 線的檔案），這份變大 |
| G3 | 只做元素的盒子，text 的部分回傳整個 text 所在的 fragment | 快 | 量文字位置的結果是錯的，Editor 不會出錯但行為不對，比沒有更難發現 |

選了 G1（README 的第 1 條）。

### T：分階段

| | 做法 |
| --- | --- |
| **T1** | 兩段：資料和 live range（含 replace data、split、normalize 的原語和比較、設定）；內容的演算法和 WPT |
| T2 | 一段做完 |
| T3 | 三段：把 `dom` 的原語（replace data、split、normalize）先拆成一段，`dom_api` 照舊呼叫，沒有 range 也能先 merge |

選了 T1（README 的第 4 條）。T3 的第一段單獨看不出行為改變，只有重構。

## Chromium 的做法

依記憶整理，實作前對原始碼確認：
- `Range`（`third_party/blink/renderer/core/dom/range.h`、`.cc`）繼承 `AbstractRange`，兩個 `RangeBoundaryPoint`；建立時 `Document::AttachRange`，`Document::ranges_` 是 `HeapHashSet<WeakMember<Range>>`，GC 時死掉的自己離開。
- 調整：`Document::NodeChildrenWillBeRemoved`、`NodeWillBeRemoved`（移除前）、`Range::NodeChildrenChanged`（插入後）、`DidInsertText`、`DidRemoveText`（`CharacterData::SetDataAndUpdate` 呼叫）、`DidMergeTextNodes`（`normalize`）、`DidSplitTextNode`（`Text::splitText`）。
- `StaticRange`（`core/dom/static_range.cc`）不登記；`Selection` 的 `toRange` 等也用它。
- 幾何：`Range::getClientRects`、`getBoundingClientRect` 經過 `Range::GetBorderAndTextQuads`，text 經過 `LayoutText::AbsoluteQuadsForRange`。
- `createContextualFragment`：`Range::createContextualFragment`，context 是 start container（是 text 時用它的 parent）。

## 參考資料

- DOM Standard：§ Ranges（boundary point、live range 的各步）、「replace data」、「split a Text node」、`normalize()`。
- DOM Parsing and Serialization：`createContextualFragment`。
- CSSOM View：`Range` 的 `getClientRects`、`getBoundingClientRect`。
