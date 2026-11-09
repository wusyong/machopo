# RFC 0042 的細節：表單的簿記改成元素層級

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 原本的資料流

- `dom` 的 `insert_before`：isolate 建過表單的元素之後（`form_elements_seen`、`form_states_seen`），每插入一個節點記 `ElementChange::Form(FormChange::Inserted(node))` 和 `FormStateChange::Inserted(node)`；移除記 `FormChange::Removed { node, parent }` 和 `FormStateChange::Removed { node, parent }`。`form` 和 `id` attribute 的改變另外記（`FormAttributeChanged`、`IdChanged`，後者只在出現過 `form` attribute 之後）。
- iframe、script、autofocus、title 另外在 `record_frame_owners` 走一次插入、移除的子樹（只在 parent 連到有 browsing context 的 document 時）。
- 宣告表的每次呼叫結束時（和 parser 的每一步之後），`dom_api::update_elements` 取走紀錄：
  - `listed_element::update_all`：先走每一筆 `Inserted` 的子樹，記下每個 radio 和 option 最後被哪一筆蓋到（`inserted_at`），再依序處理每一筆：`Inserted` 走子樹找 listed element（`listed_in`）重設 form owner、checked 的 radio 取消同組的其他（`not_yet`：後面的紀錄才插入的不算）；有 id 的元素和 `form` attribute；`select::inserted`（子樹裡最後一個 selected 的 option 取消其他、所在的和子樹裡的 select 跑 selectedness setting）。`Removed` 走被拿走的子樹，重設 owner 不在裡面的；有 `<form>` 被拿走時走 parent 的整棵樹；`select::removed`。
  - `form_states::update_form_states`：走每一筆的子樹，收集會被比對的元素；radio 和有 form owner 的 submit button 記下它們的樹，最後各走一次（radio 的組有沒有 checked、form 的 default button），再算出每個元素的 `ElementState`。
- 處理時看的都是改完之後的樹；同一個元素被每一筆蓋到它的紀錄各處理一次。

## 量到的

RFC 0012 的架構盤點（`rfcs/0012-speedometer/architecture-audit.md` 的第 3 項）。暫時的計數，TodoMVC-jQuery 的 1 個 iteration（含載入），RFC 0012 改之前：

| 走訪 | 走過的節點 | 每個 batch 不重複的 |
| --- | --- | --- |
| `form_states` 的子樹 | 2.30 M | 0.62 M |
| `form_states` 的整棵樹（default button） | 0.86 M | 0.86 M |
| `listed_in` | 1.38 M | 0.31 M |
| `inserted_at` | 1.38 M | 0.31 M |
| 移除的子樹 | 0.92 M | 0.62 M |

有表單紀錄的 batch 644 個、紀錄 52 萬筆（平均約 800 筆）。宣告表的呼叫約 13 萬次。RFC 0012 改了前兩列（子樹一個 batch 只走一次；沒有 form 的 submit button 不走整棵樹），之後 `listed_element::update_all` 佔 jQuery 的 renderer main thread 15.3%、ES5 7.8%（frame pointer 的 `perf`；ES5 是改了之後量的，jQuery 是改之前的）。

## Blink

- `ContainerNode::InsertBefore` 等插入之後，`ChildListMutationScope`、`NotifyNodeInserted` 對插入的子樹的每個節點呼叫 `InsertedInto(insertion_point)`（`NodeTraversal` 的 shadow-including 順序），移除時 `RemovedFrom`。
- `ListedElement::InsertedInto`（`core/html/forms/listed_element.cc`）：disabled 的祖先狀態設成 unknown；不是 parser 設的 owner、或 owner 和自己不在同一棵樹時 `ResetFormOwner`；有 `form` attribute 時重設它的 id observer。`RemovedFrom`：有 `form` attribute、或 owner 和自己不在同一棵樹時重設。
- `HTMLOptionElement::InsertedInto`（`html_option_element.cc`）：往上找最近的 select（`WalkAncestorsAndUpdate`），換了就 `select->OptionInserted(option, …, Selected())`。`HTMLSelectElement::OptionInserted`（`html_select_element.cc`）：插入的是 selected 的就選它、取消其他的（`kDeselectOtherOptionsFlag`，沒有 `multiple` 時）；不是 selected、select 也還沒有選的（`!last_on_change_option_`），`size` 不大於 1、option 不是 disabled 時跑 `ResetToDefaultSelection`。所以一般是線性的：有選中的之後就不再跑。
- `HTMLFormElement::RemovedFrom`：屬於它的 listed element（`ListedElements()` 的快取，走 form 所在的樹建）裡不在被拿走的子樹的，`FormRemovedFromTree`。
- form 的 pseudo-class：`SelectorChecker` 在比對時問元素（`IsChecked()`、`IsDisabledFormControl()`、`MatchesDefaultPseudoClass()`、`IsIndeterminate()`），改變時 `PseudoStateChanged(CSSSelector::kPseudo…)`；default button 由 `HTMLFormElement::FindDefaultButton` 走 form 的 listed elements，換了 `InvalidateDefaultButtonStyle`。

## `<select>`

「已決定的事項」第 9 條：一個 batch 結束時，option 清單改變的每個 select 跑一次 selectedness setting（沒有 `multiple`：保留最後一個 selected 的、取消其他的；沒有 selected 的而 display size 是 1：選第一個不是 disabled 的）。

和 Blink 的結果一致的理由：Blink 每插入一個 option，selected 的就取消其他的（所以最後一個 selected 的勝出），不是 selected 的只在還沒有選中的時候跑 `ResetToDefaultSelection`；batch 結束時跑一次，最後的狀態一樣。`ResetToDefaultSelection`（2026-10-13 讀過）：沒有 `multiple` 時，照順序保留最後一個 selected 的、取消前面的；沒有 selected 的而 `size_` 不大於 1 時，選第一個不是 disabled 的（`SelectOption`，插入和移除時的 flag 不設 dirtiness）。和 kun 的 `select::selectedness_setting` 的規則相同（display size 1 對 `size_` 是 0 或 1）。

## 比較過的做法

### form owner 的紀錄

| 做法 | 處理的元素 | 語意 | 改動 |
| --- | --- | --- | --- |
| 原本：子樹的根，處理時走改完之後的子樹 | 每一筆蓋到它的紀錄各一次 | 原本的 `not_yet` | — |
| **元素層級（選這個）** | 每次插入或移除各一次（Blink 的 `InsertedInto`） | Blink 的順序 | `dom` 的修改原語和紀錄的格式、`dom_api` 的處理 |
| 子樹的根、只記當下有相關元素的 | parser 插入的空節點不記；整棵插入時照樣走改完之後的子樹 | 原本和 Blink 的混合 | 小 |
| 倒著處理、剪掉走過的子樹（RFC 0012 的 A3） | 每個元素一次，在最後蓋到它的紀錄 | 要證明和原本一樣 | 只改 `dom_api` |

### form 的 pseudo-class

| 做法 | 成本 | 風險 |
| --- | --- | --- |
| **元素層級的紀錄，算好存成 `ElementState`（選這個）** | 每次插入或移除每個相關元素算一次；依賴整棵樹的每個 batch 走那棵樹一次 | 低：結果和原本一樣，失效照舊（狀態變了就標 style） |
| C：比對時才算（Blink） | 只在比對時算，沒有規則用到就不算 | 高：`style` 要能呼叫表單的規則（分層），每個改變狀態的地方要標失效，`querySelector` 也一起 |

C 的收益估計：RFC 0012 改了之後，`form_states` 在 ES5 的 renderer main thread 佔 7.1%（含載入），大部分是走訪和 handle；元素層級之後剩下的是相關元素各算一次。
