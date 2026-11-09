# RFC 0042：表單的簿記改成元素層級

- 開始日期：2026-10-13
- 狀態：✅ 已完成：2026-10-13（第 1 到 12 條；兩段，原本的第 3 段量過之後不做，第 4 條）
- 細節：`details.md`；進度：`progress.md`

## 摘要

`dom` 在插入、移除節點的當下，走那時的子樹（light tree）一次，只記下和表單有關的**元素**，不再記子樹的根：form owner 和 select 的（`FormChange`：`<form>`、listed element、`<option>`，出現過 `form` attribute 之後加上有 id 的元素），form 的 pseudo-class 的（`FormStateChange`：會被比對的元素，加上 `<legend>` 進出的 `<fieldset>`）。`dom_api` 在呼叫結束時每筆紀錄處理一個元素，option 清單改了的 select 在 batch 的最後各跑一次 selectedness setting。這是 Blink 對 `ListedElement::InsertedInto`、`RemovedFrom`、`HTMLOptionElement::InsertedInto` 呼叫的那個集合。表單的 pseudo-class 照舊算好存成 `ElementState`；改成比對時才算（Blink 的做法）比較過、不做，寫在「之後的可能」。TodoMVC-jQuery 的指令兩段合計少約 11%（`progress.md`）。

## 動機

RFC 0012 的架構盤點（`rfcs/0012-speedometer/architecture-audit.md` 的第 3 項）：每次宣告表的呼叫結束時，`update_elements` 處理 `dom` 記下的插入和移除。原本記的是子樹的根，處理時走的是改完之後的子樹，而 fragment parser 每插入一個節點記一筆：處理時每一筆的子樹都已經長完整，同一個元素被每一筆蓋到它的紀錄各處理一次。TodoMVC-jQuery 每個改動都用 `innerHTML` 重畫整份清單，有表單紀錄的 batch 平均約 800 筆。

RFC 0012 先做了不改結構的部分（2026-10-13：form pseudo-class 的子樹一個 batch 只走一次、只有 form 的 submit button 找 default button、`descendants` 不配置；jQuery 的指令少 12%、ES5 的時間少約 9%）。剩下的：

- `listed_element` 的 `listed_in`、`inserted_at` 各走 1.38 M 個節點，每個 batch 不重複的只有 0.31 M（jQuery 的 1 個 iteration，含載入）；移除的子樹 0.92 M、不重複的 0.62 M。`listed_element::update_all` 佔 jQuery 的 renderer main thread 15.3%、ES5 7.8%。
- 每筆紀錄都要 `handle_by_id`、建 handle，紀錄多半是和表單無關的 `<li>`、`<div>`、文字。

**目標**：

- 表單的簿記的成本和相關的元素數量成正比，每次插入或移除每個元素處理一次（Blink 的結構）。
- form owner、`<select>` 的 selectedness、radio 的組、form 的 pseudo-class 的結果照舊（WPT 的 `html/semantics/forms/` 和現有的測試）。

**不在範圍內**：

- form 的 pseudo-class 改成比對時才算（「理由和其他做法」的 C）。
- form 被移除時，外面屬於它的元素：照舊走整棵樹（Blink 走 form 自己的 listed elements 的快取；少見）。
- 改成在改變的當下處理（Blink 的時機）：照舊在呼叫結束時處理，`docs/limitations.md` 的「Bindings 和 DOM API」裡 form owner 在每次呼叫結束時才更新的那一條不變。
- handle 的模型、每次呼叫的固定成本（盤點的第 2、7 項）。

## 設計概要

網頁看不出差別：form owner、`form.elements`、radio 的組、`<select>` 選了哪個、`:checked` 這些和原本一樣，只是快。唯一的例外在同一個呼叫裡反覆插入時的中間過程，照 Blink（「怎麼處理」）。

改 `dom`、`dom_api` 的人看到的：

- `FormChange`、`FormStateChange` 的插入和移除是元素層級：`dom` 的 `insert_before` 和移除在當下走子樹，對每個相關的元素各記一筆（`Inserted(element)`；`FormChange::Removed { element, root, parent }`，`root` 是被拿走的子樹的根、`parent` 是它原本的 parent；`FormStateChange::Removed { element, parent }`），和表單無關的節點不記。
- `dom_api` 的 `listed_element`、`select`、`form_states` 每筆處理一個元素，不再走子樹。
- 表單的兩種紀錄共用一個不配置的走訪（`for_each_inclusive_descendant`），和 iframe、script 的（`record_frame_owners`，只在 connected 時、shadow-including）分開走。

## 詳細設計

### 記什麼

`dom` 在 `insert_before` 插入一棵子樹（fragment 的每個子節點各自一棵）之後、在移除一棵子樹之前，沿 `first_child`、`next_sibling` 走那時的子樹（light tree，照樹的順序；shadow tree 裡的控制項不屬於 host 的 form），對每個元素看它的名稱：

| 元素 | `FormChange`（form owner、radio 的組、select） | `FormStateChange`（form 的 pseudo-class） |
| --- | --- | --- |
| listed element（`button`、`fieldset`、`input`、`object`、`output`、`select`、`textarea`） | 插入、移除 | 插入、移除 |
| `<form>` | 插入、移除（移除時外面屬於它的元素要重設） | — |
| `<option>` | 插入、移除（所在的 select 要重算） | 插入、移除 |
| `<optgroup>`、`<progress>` | — | 插入、移除 |
| 有 `id` 的元素（只在出現過 `form` attribute 之後） | 插入、移除（`form` attribute 可能指到它） | — |

select 的 option 清單只在 option 自己插入、移除時改變，它們自己就有紀錄（select 連同它的 option 插入時，各個 option 找到它）。另外記的「parent 的改變」（只給 form 的 pseudo-class）：插入、移除的是 `<fieldset>` 的 `<legend>` 時記那個 fieldset（`Legends`：第一個 legend 可能換了，各個 legend 裡的 disabled 跟著變；第 10 條），插入、移除的 parent 是 `<textarea>` 時記它（文字，`:placeholder-shown`）。名稱表是 `dom` 的兩張靜態表（`FORM_ELEMENTS`、`FORM_STATE_ELEMENTS`，第 8 條）。

`form_elements_seen`、`form_states_seen`、`form_attributes_seen` 照舊：isolate 沒建過這些元素就不走。

### 怎麼處理

- **form owner**（`listed_element`）：`Inserted(element)`：listed element 沒有 parser inserted flag 就 reset the form owner；在 document 裡的 checked radio 取消同組的其他。`Removed { element, root, parent }`：listed element 的 owner 不在被拿走的子樹（`root`）裡就重設（Blink 的 `ListedElement::RemovedFrom`）。`<form>` 被拿走：照舊走 parent 的整棵樹，重設 owner 在子樹裡、自己不在的元素，每個被拿走的子樹一次。有 id 的元素：重設那棵樹裡 `form` attribute 指的元素，一個 batch 每棵樹一次（再做一次，每個元素拿到一樣的 owner）。
- **「後插入的勝出」**：每個元素在它的每一筆紀錄處理一次，照紀錄的順序；`not_yet` 是「這個元素在後面還有 `Inserted`」。這是 Blink 的 `InsertedInto` 的順序；和原本差在同一個呼叫裡反覆插入時中間的過程（原本較早、但最後的子樹蓋到它的紀錄也處理它），以 Blink 為準（第 2 條）。插到 selected 的 option 前面的 option 是最後插入的，它留著（不是樹的順序上最後的）。
- **`<select>`**：`<option>` 插入時是 selected 的，取消同一個 select 的其他（`select::option_inserted`，後面的勝出）；option 清單改變的 select 記下來，**一個 batch 結束時每個 select 跑一次** selectedness setting（第 9 條）。
- **form 的 pseudo-class**（`form_states`）：每筆紀錄算一個元素；依賴整棵樹的（radio、有 form owner 的 submit button）照舊記下它的樹，batch 結束時走一次；`Legends` 走 fieldset 的各個 legend 的子樹。RFC 0012 加的「子樹走過就不再走」留給剩下的子樹走訪（fieldset、select、optgroup 的 `disabled` 改變，fieldset 的 legend；第 11 條）。

### 測試

- **`dom` 的整合測試**：`tree::form_elements_are_recorded_one_by_one`、`tree::form_state_elements_are_recorded_one_by_one`：記下的元素和順序（巢狀、一個一個插入、shadow tree 不記、id 只在 `form` attribute 之後、移除的根和 parent、legend 和其他 child 進出 fieldset、textarea 的 child）。
- **`dom_api` 的整合測試**：`forms::` 照舊通過（parser 插入的 radio 和 option、form owner 的各種情況、legend、`a_subtree_changed_twice_in_one_call`），加上 `inserted_again_later_in_one_call`（option 和 radio 在同一個呼叫裡又插入一次、插到前面的）。
- **WPT**：`html/semantics/forms/`、`dom/nodes/`、`css/selectors/`、`html/semantics/selectors/`、`shadow-dom/` 的預期結果不變。
- **mutation test**：`mutants-1.toml`、`mutants-2.toml`。
- **量測**：和 `main` 輪流量 jQuery、ES5、React-Complex-DOM（`perf stat` 的指令數和時間）。

### 分階段

1. `FormChange` 改成元素層級：`dom` 記元素，`listed_element`、`select` 照著處理，select 每個 batch 一次。
2. `FormStateChange` 改成元素層級：`form_states` 每筆一個元素，fieldset 只在 legend 進出時記。

原本的第 3 段（插入時的走訪合成一趟）量過之後不做（第 4 條）。

## 缺點

- `dom` 多知道兩張「和表單有關的元素」的名稱表（規則還在 `dom_api`）；兩邊要一致，由測試守住。
- 「後插入的勝出」的中間過程照 Blink，和原本不完全一樣（只在同一個呼叫裡反覆插入時）。
- `dom` 的插入和移除多走子樹（表單的兩種各一次，不在 browsing context 的 document 裡也走）：parser 插入的節點沒有子節點，成本在插入整棵子樹時，和原本處理時走的一樣多、只走一次。

## 理由和其他做法

完整的比較在 `details.md`。

- **元素層級（選這個）對子樹的根但只記相關的**：後者改動小（`dom` 記錄時看子樹裡有沒有相關的元素，沒有就不記），但處理時走的還是改完之後的子樹，巢狀的紀錄照樣重複，語意也變成原本和 Blink 的混合。
- **倒著處理紀錄、剪掉走過的子樹**（RFC 0012 列的 A3）：不改 `dom`，但「後插入的勝出」的正確性要靠處理順序的論證，之後改的人難守住。
- **C：form 的 pseudo-class 比對時才算**（Blink 的 `SelectorChecker` 問元素，`PseudoStateChanged` 失效）：不用記插入、移除，但 `style` 要能在比對時呼叫表單的規則（在 `dom_api`；要搬進 `dom`，或照 `LayoutClient` 加一個 client，style 在 rendering update 時也要拿得到），每個改變狀態的地方要自己標失效（checkedness、radio 的組、default button），`querySelector(':checked')` 也要一起。這份做完之後 `form_states` 剩下的是每個相關元素算一次狀態，C 能再省的很少；RFC 0041 的第 4 段也在現有的架構上加 `:read-only`、`:required` 這些。寫在「之後的可能」，附上重新考慮的條件。
- **插入時的走訪合成一趟**（原本的第 3 段）：量過（第 4 條），三趟走訪合起來不到 jQuery 的 renderer main thread 的 2%，省一趟約 1%，React-Complex-DOM 量不到；而合成之後，最常呼叫的修改原語裡要有一個兩種範圍（connected 才記的、任何樹都記的）、兩種走法（shadow-including、light tree）混在一起的走訪。Blink 也是各個 hook 在 `InsertedInto` 裡各做各的。

## 先例

- **Blink**：`ContainerNode` 插入、移除時對子樹的每個節點呼叫 `InsertedInto`、`RemovedFrom`（`NodeTraversal` 走過去），`ListedElement::InsertedInto` 必要時 `ResetFormOwner`，`RemovedFrom` 在 owner 和自己不在同一棵樹時重設；`HTMLOptionElement::InsertedInto` 通知所在的 select（`OptionInserted`）；`HTMLFormElement::RemovedFrom` 處理屬於它的元素（`ListedElements()` 的快取）；`HTMLFieldSetElement::ChildrenChanged` 重新判斷各個 legend 的後代。form 的 pseudo-class 在比對時問元素（`IsChecked()`、`IsDisabledFormControl()`、`MatchesDefaultPseudoClass()`），改變時 `PseudoStateChanged`。照做的：元素層級、每次插入每個元素一次、legend 的子樹。不照做的：在改變的當下處理（kun 在呼叫結束時）、pseudo-class 比對時才算、form 自己的 listed elements 的快取。
- **Stylo / Gecko**：form 的狀態是元素的 `ElementState` 位元，改變時設定、發 state change 的失效（和 kun 存 `ElementState` 的做法相近）。

## 已決定的事項

2026-10-13 確認（RFC 0012 的架構盤點之後討論）：

1. **表單的簿記改成元素層級**：`dom` 在插入、移除的當下走那時的子樹，只記和表單有關的元素，`dom_api` 每筆處理一個元素（Blink 的 `InsertedInto`、`RemovedFrom` 的集合）。不選「子樹的根、只記相關的」：處理時照樣重複，語意是原本和 Blink 的混合。
2. **「後插入的勝出」以 Blink 為準**：每個元素在它的每一筆紀錄處理一次，照紀錄的順序；不證明和原本的中間過程等價。不選倒著處理、剪掉走過的子樹（A3）：正確性靠處理順序的論證。
3. **`FormStateChange` 一起改成元素層級**，和 `FormChange` 共用走訪的函式（第 4 條之後各走一次）。
4. **插入時的走訪不合成一趟**（2026-10-13 改；原本決定把 `record_frame_owners` 的 iframe、script、autofocus、title 和表單的合成一趟，當成第 3 段）：第 2 段之後用 frame pointer 的 `perf` 量，jQuery 的 `insert_before` 佔 renderer main thread 12.7%，其中 `record_frame_owners` 0.7%、含 inline 的兩趟表單走訪的 self 1.4%，省一趟約 1%；React-Complex-DOM 的 `insert_before` 只有 0.8%。合成要在最常呼叫的修改原語裡混兩種範圍、兩種走法，Blink 也沒有合。同一個函式裡大的是 `adoption_targets`（4.6%：為了比較 document 建 handle），屬於架構盤點的第 2 項。
5. **form 被移除時照舊走整棵樹**找外面屬於它的元素：少見；form 自己的 listed elements 的快取（Blink）寫在「之後的可能」。
6. **form 的 pseudo-class 照舊算好存起來**，比對時才算（C）這份不做：這份之後剩下的成本小，C 要改 crate 的分層、自己做失效，RFC 0041 的第 4 段也在現有的架構上加 pseudo-class。
7. **獨立一份 RFC、分段做**（原本三段，第 3 段不做：第 4 條）。
8. **和表單有關的元素的名稱表放在 `dom`**（靜態表）：那是 HTML 定義的分類，不是規則；`dom` 已經自己認 `iframe`、`script`、`title`。不選 `dom_api` 在建 isolate 時給 `dom` 一個判斷的函式：知識留在 `dom_api`，但 `dom` 的每次插入多一次間接呼叫。兩邊的一致由測試守住。
9. **`<select>` 的 selectedness setting 一個 batch 每個 select 跑一次**：照元素處理時，每個 option 都跑一次會讓 parser 插入有 n 個 option 的 select 變成 O(n²)。結果和 Blink 逐個 option 處理（`OptionInserted`、`ResetToDefaultSelection`）一樣（`details.md` 的「`<select>`」）。
10. **fieldset 只在 `<legend>` 進出時記，只重算它的各個 legend 的子樹**（2026-10-13，第 2 段開始前）：HTML 的 fieldset 的 disabled 不套用到第一個 legend 裡，只有 legend 進出會改變哪個是第一個；新進來的控制項自己有紀錄。Blink 的 `HTMLFieldSetElement::ChildrenChanged` 在每次 children 改變時重新判斷各個 legend 的後代，結果一樣。不選「任何 child 進出都走整個 fieldset 的子樹」（原本的做法：往 fieldset 裡一個一個加控制項是平方）。
11. **「子樹走過就不再走」留著**（2026-10-13，第 2 段開始前）：插入、移除不再走子樹，但 `disabled` 改變和 legend 進出還要走，一個 batch 可能有好幾個。
12. **和 RFC 0041 第 4 段的協調**（2026-10-13，第 2 段開始前）：它在 `form_states` 加 `:read-only`、`:required` 這些（狀態怎麼算），這份改紀錄和走訪（哪些元素要重算）；第 2 段開始時 RFC 0041 做完第 2 段、第 4 段還沒開始，這份先改，它之後 rebase。

## 待決定

無。

## 之後的可能

- **form 的 pseudo-class 比對時才算**（C）：重新考慮的條件：`form_states` 在某個 suite 仍佔 renderer main thread 的幾個百分點以上，或有頁面大量比對這些 pseudo-class 而很少改。
- **form 自己的 listed elements 的快取**（Blink 的 `HTMLFormElement::ListedElements()`）：form 被移除、`form.elements`、default button 都不必走整棵樹。
- **在改變的當下處理**（Blink 的時機）：要等 `dom` 的修改原語能呼叫 `dom_api` 的規則。
- **插入時的走訪合成一趟**：第 4 條量的收益約 1%；修改原語的其他成本（handle）做完之後，走訪變成主要的成本時再看。
