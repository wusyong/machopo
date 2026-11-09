# RFC 0063：選擇器查詢

- 開始日期：2026-10-20
- 狀態：✅ 已完成：2026-10-20（照 Blink 在每個元素上存 class 和 attribute 名稱的 subtree Bloom filter，查詢照 `SelectorQuery` 每個 compound 一個 filter 跳過找不到的子樹；快速比對比名稱、只走元素、parse 好的選擇器照字串留著。基準頁的 `[data-id]` 0.16 → 0.045 ms、`.filters .selected` 0.12 → 0.002 ms，小範圍的 `querySelector`、`matches`、`closest` 約一半；Speedometer 的 ES6-Webpack −20%，和 Chrome 的倍數約 4 → 2.5；innerHTML 每個節點的指令 +0.7%。剩下每個元素的成本在讀元素的資料，留給元素的 attribute 照 `ElementData` 存；在 Linux 上做）
- 進度和驗收：`progress.md`
- 細節：`details.md`（怎麼選到這裡、Speedometer 裡的查詢、基準頁、kun 每個元素的成本、第 1 段之後的取樣、完成時的 Speedometer、Blink 的 subtree Bloom filter）

## 摘要

`querySelector`、`querySelectorAll` 在 kun 一律從頭走到尾，每個元素都比對一次；Chrome 在每個元素上存一個 32 bit 的 Bloom filter，記著它和它的子樹裡出現過的 class 和 attribute 名稱，查詢時整棵不可能有符合的子樹直接跳過（Blink 的 `attribute_or_class_bloom_`，`SelectorQuery` 的 `CouldMatchFilter`）。同一頁 7137 個元素、要找的在最後面，`document.querySelector('[data-id="99"]')` kun 0.157 ms、Chrome 0.001 ms。做了兩段，行為都不變：

- **第 1 段，subtree 的 Bloom filter**：照 Blink 加上這個 filter，查詢照 Blink 的 `SelectorQuery` 把選擇器拆成 compound，每個 compound 一個 filter，從根往下找的時候剪掉不可能找到還沒找到的那些 compound 的子樹，找到的再完整比對。
- **第 2 段，每次呼叫和每個元素的成本**：快速比對比名稱（不逐字比）、只走元素、parse 好的選擇器照字串留著（Blink 的 `SelectorQueryCache`）：在一個元素上查、`matches`、`closest` 的時間大多在 parse。

`[data-id]` 0.16 → 0.045 ms，`.filters .selected` 0.12 → 0.002 ms；Speedometer 的 ES6-Webpack −20%（`progress.md`）。大範圍的查詢每個元素還是 Chrome 的幾倍：時間在讀元素的資料（`Node`、它的 state、attribute 的 `Vec`），要改元素存 attribute 的方式（「之後的可能」）。

## 動機

- **Speedometer 裡差最多的 suite 卡在這裡**：2026-10-20 的量測（`main` 的 `4a09a2c0`，`details.md` 的「Speedometer 裡的查詢」），TodoMVC-JavaScript-ES6-Webpack-Complex-DOM 是 kun 和 Chrome 差最多的 suite（116 對 29 ms，3.96 倍）。它的 `querySelector` 一個 iteration 約 34 ms，比 Chrome 跑完整個 suite 還多。這個 suite 的 complex DOM 在 todo app 前面放了約 6600 個元素，app 每勾選、刪除一個項目都 `document.querySelector('[data-id="…"]')`，每次都從頭走過那 6600 個元素。查詢的 API 在 19 個 suite 合計約 70 ms：`querySelector` 45（ES6-Webpack 34、NewsSite-Next 4）、`querySelectorAll` 16、`getElementsBy*` 7、`getElementById` 2。
- **基準頁直接比**（`details.md` 的「基準頁」，那個 suite 的頁面，7137 個元素，ms 一次）：

  | 查詢 | kun | Chrome | 倍數 |
  | --- | ---: | ---: | ---: |
  | `querySelector('[data-id="99"]')`（最後一個項目） | 0.157 | 0.001 | 157 |
  | `querySelector('[data-id="nope"]')`（沒有） | 0.159 | 0.002 | 95 |
  | `querySelector('.filters .selected')` | 0.119 | 0.013 | 9 |
  | `querySelectorAll('div').length` | 0.156 | 0.030 | 5 |

  前兩項 Chrome 平均一個元素不到 0.3 ns：它沒有走那些元素。後兩項它照樣走（`.selected` 在 footer 裡，`div` 到處都有），kun 每個元素還是貴 5–9 倍。
- **kun 每個元素的成本**：ES6-Webpack 的 `querySelector` 的樣本 70% 在走訪的迴圈自己（`descendants` 和 `with_matcher` 的 closure 內聯在一起），比對選擇器本身只佔 27%；迴圈的樣本有六成集中在一條指令上，像是剛寫入一個 byte 就讀回八個 byte 的 store forwarding 失敗（`details.md` 的「kun 每個元素的成本」）。

目標：

- `querySelector`、`querySelectorAll`（`Document`、`Element`、`DocumentFragment` 的）不走不可能有符合的子樹。
- 行為不變：結果和順序和現在一樣；debug build 交叉檢查。
- 第 2 段量過再決定。

不在範圍內：

- `getElementById` 的 id 對照表（Blink 的 `TreeScope` 的 `DocumentOrderedMap`）：kun 也是從頭走，但 Speedometer 裡只有約 2 ms。放進「之後的可能」。
- `getElementsByClassName` 這類 live collection：Blink 的 collection 也不用 Bloom filter。
- `matches`、`closest` 的走訪：只比對一條祖先鏈，沒有子樹可剪（第 2 段的 parse 快取照樣用在它們上）。
- innerHTML、`DOMParser`、建節點、GC 的架構：RFC 0062 剩下的候選，另外評估。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

對網頁和使用者：行為不變，只是變快。

對之後改 `dom` 的人：

- 每個元素多一個 32 bit 的 filter：它自己和它的子樹（只算 light tree，不跨 shadow root）的每個 class 名稱和 attribute 名稱各設兩個 bit。只會多，不會少（移除時只在便宜的情況下重算），所以「沒有那幾個 bit」一定表示子樹裡沒有，「有」不一定有。
- 新增一條會讓元素多一個 class 或 attribute 的路時，要經過設定 filter 的地方（debug build 檢查每個元素的 filter 包含它的子樹的）。

## 詳細設計

### 第 1 段：subtree 的 Bloom filter

1. **filter 本身**（Blink 的 `Element::attribute_or_class_bloom_`、`FilterForString`）：`dom` 的 `Node` 多一個 `#[no_trace]` 的 `Cell<u32>`（`subtree_filter`，放在原本的 padding 裡，`Node` 還是 328 bytes；放在 `Node` 上而不是 `NodeState` 裡，查詢讀它時不經過 `RefCell` 的借用），元素以外的節點是 0。一個名稱設兩個 bit，取自 hash 的兩段五個 bit（`name_filter`）。class 用元素本來就存著的 `name_hash`（ASCII 小寫之後的 hash）。attribute 的名稱用自己的 hash（`attribute_name_filter`）：一次八個 byte，每個 byte OR `0x20`，字母的大小寫算成一樣（Blink 的 `QualifiedName::BloomFilter` 用 `LocalNameUpper`），HTML 文件裡選擇器的 `[viewbox]` 照樣找得到 SVG 的 `viewBox`；一開始用 `name_hash`，每個字元一次 hash，建元素時每個 attribute 都要付，innerHTML 每個節點多約 70 個指令，換成這個之後約 50（Blink 的名稱自己存著 filter，只算一次）。quirks mode 的 class 和 id 不分大小寫，`name_hash` 本來就摺疊（第 2 條）。kun 沒有延後才寫進元素的 attribute：CSSOM 改 `style` 時當場寫回 attribute（`set_inline_style`），所以不必像 Blink 排除 lazily synchronized 的。
2. **維護**：
   - 元素建立時帶著的 attribute（parser）、之後加的 attribute、`class` 的改變：設自己的 bit，往上 OR 到 parent element，直到沒有新的 bit（Blink 的 `UpdateSubtreeBloomFilterAfterInsert`）。
   - 插入：被插入的元素的 filter 已經包含它的子樹，往上 OR 一次。parser 只接指標的 fragment 也一樣（`fragment_append_child`）。
   - 移除和刪掉 attribute：留著舊的 bit；parent 只剩零個或一個子元素（中間最多 32 個非元素的子節點）時才重算（Blink 的 `UpdateSubtreeBloomFilterAfterChildRemoval`）。
   - parent 是 document、fragment 或 shadow root 時停下：查詢不跨 shadow root，document 和 fragment 自己沒有 class 或 attribute。
3. **查詢**（Blink 的 `SelectorQuery::BuildCompounds`、`ExecuteSearch`、`ExecuteSearchSingleCompound`）：
   - **拆成 compound**：選擇器清單只有一條選擇器時，照 combinator 拆成 compound，從最左邊（最上面）的到主體。每個 compound 記下它能快速比對的 simple selector：tag、一個 class、完全相等的一個 attribute（`[a="v"]`）；其他的（第二個 class、`:not()`、`:is()`、`:has()`、其他 attribute 的比對方式、pseudo-class）快速比對時當成符合，記下「要完整比對」。
   - **每個 compound 的 filter**：它自己要求的 class 和 attribute 名稱（只算 compound 裡直接寫的，函式裡面的不算），加上它下面（`>`、後代）還要找的 compound 的；`+`、`~` 的下一個 compound 是同一層的兄弟，不併進去（Blink 的 `selector_filter`）。
   - **往下找**：從根開始找第一個 compound，一個元素快速比對符合時，它的子樹改找下一個 compound（`+`、`~` 的，它後面的兄弟改找下一個）；`>` 放寬成後代、`+` 放寬成 `~`，所以不用回頭重試（greedy）。一個元素的 filter 沒有目前要找的 compound 的全部 bit，就跳過它和它的子樹。找到主體的 compound 符合的元素時，它是候選。
   - **候選照樣完整比對**：候選一律再用 `selectors` 的比對（`matches_selector`）確認一次，再放進結果。Blink 只在放寬過、有快速比對不支援的 simple selector 時才這樣做；kun 一律做，少一條要證明「快速比對等於完整比對」的路，多的成本只在候選上。
   - **根上面已經符合的**：從元素開始查時，選擇器左邊的 compound 可能由根的祖先符合（`div .a` 從 `div` 裡面的元素查）；照 Blink 的 `FirstCompoundNotSeenBeforeRoot`，往上看祖先，從還沒符合的那個 compound 開始找。
   - **走訪不遞迴**：Blink 同時能往下和往右時遞迴下去；kun 把還沒走的兄弟和要找的 compound 放在一個 `Vec` 裡，很深的樹不會用掉 stack。
   - **不用這條路的**：選擇器清單有兩條以上（Blink 一樣照舊從頭走）；有 pseudo-element（什麼都不符合）、`:host`、`::slotted()`、`::part()`（它們的 compound 由走訪的樹以外的元素符合：Blink 讓 shadow root 本身充當沒有快速比對的 compound，`skip_for_shadow_root`）、`&` 的。`:scope` 這類其他的 pseudo-class 和函式快速比對時當成符合，候選再完整比對。
   - Blink 用 id 對照表直接跳到 `#id` 的元素（`last_compound_with_id_selector_`）：kun 沒有 id 對照表，不做（「之後的可能」）。

**正確性**：快速比對比完整的寬（不支援的當成符合、combinator 放寬），filter 只多不少，所以不會漏掉真的符合的元素：一個真的符合的元素和它在選擇器左邊的那些元素，沿著放寬過的 combinator 一定找得到，而且路上每個元素的子樹都有還要找的那些 compound 的名稱，filter 有那些 bit。多找到的候選由完整比對拿掉。debug build 每次查詢都和從頭走的結果比對。

**debug build 的交叉檢查**：每次剪掉子樹時，確認那棵子樹裡每個元素的 filter 都包含在根的 filter 裡（Blink 的 `VerifyBloomFilterTreeConsistencyIncludingChildren`）；`querySelector`、`querySelectorAll` 的結果和從頭走、每個元素完整比對的一樣。

估計：ES6-Webpack 的 `querySelector` 約 34 ms 大多省掉（`.filters .selected` 也只走有 `.filters` 的子樹），`querySelectorAll` 省一部分；那個 suite 116 → 約 80 ms（3.96 → 約 2.75 倍）。Speedometer 的幾何平均和 Chrome 的比值約 −2%（`details.md` 的「估計」）。插入時多的是每個元素一次 OR，通常第一層就停。做完量的：基準頁的 `[data-id]` 0.16 → 0.045 ms、`.filters .selected` 0.12 → 0.002 ms；ES6-Webpack 102 → 82 ms（`main` 和這份輪流比，−20%）；innerHTML 每個節點的指令 +0.7%（`progress.md`）。

### 第 2 段：每次呼叫和每個元素的成本

第 1 段之後再量（`details.md` 的「第 1 段之後」）：大範圍的查詢，時間在走訪（42–59%）和快速比對（37%）；`querySelectorAll` 每次建比對用的 context 11%；parse 約 1%。小範圍的查詢（在一個元素上查、`matches`、`closest`）每次呼叫是 Chrome 的 8–11 倍，parse 佔 59–72%，context 11–19%。

1. **快速比對比名稱，不比字串**（第 4 條）：compound 存 tag 和 attribute 名稱的 kun 名稱（`dom::name::LocalName`，短的和已知的是 atom，比整數），小寫的和照寫的各一個：kun 的完整比對拿 `selectors` 給的那一個（HTML 文件裡的 HTML 元素是小寫的，其他照寫的）精確比對，所以兩個都比過就是超集合。值照選擇器的大小寫：寫了 `i` 或 HTML 規定不分大小寫的 attribute 不分，其他逐 byte 比。Blink 的 `MatchCompound` 比 `QualifiedName` 和 `AtomicString`。
2. **只走元素**（第 5 條）：`dom` 的 `NodeRef::first_element_child`、`next_element_sibling` 直接沿著 edge 跳過文字和註解，不替它們建 `NodeRef`（Blink 的 `ElementTraversal`）。
3. **比對用的 context**（第 6 條）：量了才知道不必做，見第 6 條。
做完量的（`progress.md`）：小範圍的呼叫快了一倍以上（`li.querySelector('.toggle')`、`matches`、`closest` −51–64%），大範圍的查詢幾乎沒變（比名稱只讓快速比對少約 10%）：剩下每個元素約 150 ns（Chrome 約 20 ns）分散在讀 `Node`、它的 state、attribute 的 `Vec`，像是每一步的 cache miss，要改元素存 attribute 的方式才省得到（「之後的可能」）。

4. **parse 的快取**（第 7 條，Blink 的 `SelectorQueryCache`）：`dom_api` 每個 thread 一份，用選擇器字串當 key，存 parse 好的 `SelectorQuery`（連同 compound，`Rc`），最多 256 筆，滿了丟掉一筆；`querySelector`、`querySelectorAll`、`matches`、`closest` 都經過它。parse 不看 quirks mode（比對時才看），所以只看字串。

### 測試

- 行為不變：`dom`、`dom_api`、`style`、`dom_js` 的測試，`test.py wpt dom/nodes/`（`ParentNode-querySelector-All*`、`Element-matches*` 等）和 `css/selectors/`。
- filter 的維護：建立、加 attribute、改 `class`、插入（一般的和只接指標的）、移除之後，每個元素的 filter 包含它的子樹的（`dom` 的測試直接檢查，debug build 每次剪掉子樹時也檢查）。
- 剪枝：符合的元素在子樹深處、在 shadow tree 裡（不該找到）、在 template 的內容裡（不該找到）；名稱的大小寫（SVG 的 `viewBox`、quirks mode 的 class）；`:not(.a)` 這類不能當成要求的；每種 combinator（後代、`>`、`+`、`~`）和它們的組合，放寬之後要靠完整比對拿掉的（`.a > .b` 的 `.b` 是孫子）；從元素開始查、左邊的 compound 由根的祖先符合的；選擇器清單有兩條的。
- 工作量的測試：`querySelector('[data-id=x]')` 在大部分子樹沒有 `data-id` 的樹裡只看少數元素（計數，不量時間，像 `style` 的 `matching::few_selectors_are_matched`）。
- mutation test：filter 少設一個 bit、插入時不往上 OR、移除時清掉還在的 bit、把函式裡的名稱當成要求、`+`、`~` 的下一個 compound 併進 filter、候選不完整比對、根上面已經符合的不往上看，各有測試失敗。
- 效能的量測不排進測試計畫（`docs/benchmarks.md` 的「效能的量測」）；`details.md` 的基準頁在使用者要求時重量。

### 分階段

1. **subtree 的 Bloom filter**（上面的 1–3），做完量一次（基準頁）。
2. **每次呼叫和每個元素的成本**（第 4–7 條）。

兩段做完一起 merge 回 `main`。

## 缺點

- `Node` 多一個 4 bytes 的欄位（現在放在原本的 padding 裡，之後 `Node` 再多欄位時可能讓它變大）。
- 每次建元素都要算它的 attribute 名稱的 hash，每次加 attribute、改 `class`、插入都多一次往上的 OR（通常第一層就停：parent 已經有那些 bit）：innerHTML 每個節點的指令約 +0.7%（`progress.md`）。
- filter 只多不少：一棵子樹的 attribute 都被刪掉之後，它照樣不會被剪掉，直到 parent 只剩一個子元素。和 Blink 一樣，只是慢，不會錯。
- 新增一條會讓元素多一個 class 或 attribute 的路，要記得經過設 filter 的地方；漏了是看得到的錯（找不到元素），debug build 的檢查和測試守著。
- 查詢多一套快速比對和往下找的狀態機，和 `selectors` 的比對並存；`selectors` 支援新的語法時，快速比對要把它當成「要完整比對」（不認得的一律這樣，所以漏了只是剪得少）。

## 理由和其他做法

- **照 Blink 用 subtree 的 Bloom filter**：Speedometer 以外的網頁也受益（在大文件裡 `document.querySelector('[data-x]')`、`.some-class` 很常見）。
- **照 Blink 每個 compound 一個 filter**（第 1 條），不只用主體的 compound：能剪得更多（`.filters .selected` 在沒有 `.filters` 的子樹），也和 Chrome 剪的一樣多。代價是一套快速比對和往下找的狀態機，和 `selectors` 的比對並存；只支援 tag、class、完全相等的 attribute，其他交給完整比對。
- **不選 class 和 attribute 的索引**（每個名稱一張元素的表）：維護成本高，每次改 attribute 都要更新表；Bloom filter 只多一個整數。
- **不選快取查詢的結果**：DOM 一改就失效，ES6-Webpack 每次查詢之間都改了 DOM。

## 先例

- **Blink**：`Element::attribute_or_class_bloom_`（`element.h`、`element.cc`）：class 和 attribute 名稱，k=2 的 32 bit filter；`AppendAttributeInternal`、`ClassAttributeChanged`、`InsertedInto` 之後 `UpdateSubtreeBloomFilterAfterInsert`；`RemovedFrom` 時 parent 的 `UpdateSubtreeBloomFilterAfterChildRemoval`（只剩零或一個子元素時重算）；刪 attribute 時不重算。`SelectorQuery`（`selector_query.cc`）在每個 compound 上存 filter，`ExecuteSearchSingleCompound` 對 `CouldMatchFilter` 不過的元素跳過整棵子樹。
- **Stylo**（Servo 的 `query_selector`）：id、class、tag 的 fast path，沒有 subtree 的 filter。

## 已決定的事項

2026-10-20，第 1 段開始前：

1. **照 Blink 每個 compound 一個 filter**（`SelectorQuery` 的 compound 狀態機，「詳細設計」的第 3 條）。不選只用主體的 compound：剪得少（`.filters .selected` 照樣走過沒有 `.filters` 的子樹），和 Chrome 剪的不一樣多。選擇器清單有兩條以上時照舊從頭走，和 Blink 一樣。和 Blink 不同的一點：候選一律再完整比對一次。
2. **大小寫**：attribute 名稱照 ASCII 摺疊大小寫之後取 hash（Blink 的 `LocalNameUpper`），SVG 的 `viewBox` 這類照樣剪得掉；quirks mode 的 class 名稱也摺疊。不選 quirks mode 不用 class 剪：quirks mode 的頁面一樣常查 class。
3. **移除時**：照 Blink，parent 只剩零或一個子元素（中間最多 32 個非元素的子節點）時才重算，其他時候留著舊的 bit；刪 attribute 不重算。不選從不重算：一個清單清空再填滿之後，留著的 bit 越來越多。不選每次重算整條祖先鏈：每次移除都要看每個祖先的所有子元素。

2026-10-20，第 1 段做完、再量過之後（`details.md` 的「第 1 段之後」）：

4. **快速比對比名稱**：tag 和 attribute 名稱存成 kun 的名稱（小寫的和照寫的），值照選擇器的大小寫。不選維持不分大小寫的字串比對：每個元素的每個 attribute 都逐字比，`[data-id]` 在 100 個都有 `data-id` 的 `li` 上剪不掉，那裡的時間有 37% 在比字串。
5. **只走元素的走訪由 `dom` 提供**（`NodeRef::first_element_child`、`next_element_sibling`）。不選留在 `style` 裡一個一個跳：每個文字節點都建一次 `NodeRef`；之後 collection 的走訪也能用。
6. **比對用的 context 到第一個候選才建**。不選跨查詢共用：`selectors` 的 cache 只對一次查詢時的 DOM 有效。後來不做：實作前再看取樣，`with_matcher` 的那 11–19% 是內聯在它裡面的完整比對（候選的 `matches_selector`），不是建 context；`SelectorCaches` 是幾個空的 `FxHashMap`，建起來不配置。原本的數字分類錯了（`details.md` 的「第 1 段之後」已改）。
7. **做 parse 的快取，每個 thread 一份，放在 `dom_api`**（甲）。一開始建議不做（大範圍的查詢 parse 只佔 1%），量了小範圍的才改：那裡 parse 佔 59–72%，Speedometer 裡 jQuery、Perf-Dashboard 的查詢時間有 21%、36% 在 parse。不選照 Blink 每個 document 一份（乙）：`dom` 不能依賴 `style`，document 要多一個不認得型別的欄位；同一個 thread 上的 document 共用一份，結果也一樣，因為 parse 只看字串。renderer 之後拆成 process 時，每個 process 各自一份，不受影響。

## 待決定

無。

## 之後的可能

- `getElementById` 的 id 對照表（Blink 的 `TreeScope::getElementById`，`DocumentOrderedMap`），`#id` 開頭的選擇器也用它（Blink 的 id fast path）。
- `getElementsByClassName` 的 collection 用同一個 filter 跳過子樹。
- 元素的 attribute 照 Blink 的 `ElementData` 存（parser 建的元素共用不變的一份、放在元素旁邊）：查詢每個元素少讀幾次不連續的記憶體，第 2 段之後剩下的每個元素的成本多在這裡。
- RFC 0062 剩下的候選（tokenizer、配置時的 GC、`Member` 寫入的 FFI）、`DOMParser` 的文件也只接指標（TodoMVC-ES5 每次重畫用 `DOMParser`，基準頁上 kun 是 Chrome 的 2 倍）。
