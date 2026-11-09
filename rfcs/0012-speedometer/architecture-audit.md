# RFC 0012 的架構盤點：和 Blink 做法不同、多花時間的地方

這份放「kun 和 Blink 在架構上的哪些差異讓 Speedometer 變慢」：每一項 kun 怎麼做、Blink 怎麼做、量到多少、可能的做法和狀態。用 `perf` 找熱點只看得到 kun 自己的程式花在哪裡；這裡對照 Chrome 的做法，找 kun 做了 Chrome 不做、或延後才做的事。

**狀態**：第一次盤點 2026-10-13，`main` 的 `9e9e6913`，Fedora 44（AMD Ryzen AI 9 HX 370），Chrome for Testing 152.0.7977.82。「差異」表的「狀態」是 RFC 0012 完成時（2026-10-16）的樣子；這份之後不再更新（完成的 RFC 不再改，`rfcs/README.md` 的「流程」第 6 步），還沒做的各項由優化的 RFC 接手，那時照「怎麼盤點」重新量。完成時最後一次的 trace（`progress.md` 的「量測的紀錄」）的差距由大到小是 JS 呼叫的 DOM API（第 2、7 項）、style 的重算（第 5 項）、fragment 的 parse（第 4 項，jQuery 每次約 Chrome 的 3.7 倍，所有 suite 合計 3.5 倍）、paint 之後每個 frame 走整棵樹的兩個 region（scroll 的 hit test region 和 wheel 的 handler region，這裡沒有列）。

這份原本的更新方式：一項開始處理時，在「狀態」寫上日期和做法，做完時寫上量到的結果（細節在 `progress.md` 的「做過的檢查」）；重新盤點時照「怎麼盤點」重跑，更新「次數的對照」和各項的數字；新找到的差異加在表的最後，編號不重用。

## 怎麼盤點

1. **比次數**：`python3 scripts/ci.py speedometer --trace` 錄每個 suite 兩邊各一份 trace（10 個 iteration，含每個 iteration 重新載入頁面），比較 renderer main thread 上各段的次數和時間。kun 的 span 用 Chrome 的名稱（`README.md` 的「已決定的事項」第 33 條），對得起來：kun 的 `Document::UpdateStyle`、`Layout` 對 Chrome 的 `Document::updateStyle`、`LocalFrameView::performLayout`。kun 的那幾段再照外面有沒有 `FunctionCall` 分成「script 強制的」和「frame 的」。次數差一個數量級的地方，通常是 kun 多做的事；只比時間會被每次的常數成本蓋住。
2. **分類 self 時間**：2026-10-13 是手動做的：用 frame pointer 重編 `perf` profile（`RUSTFLAGS="-C force-frame-pointers=yes" cargo build --profile perf -p shell --bin kun`），`perf record --call-graph fp -p <kun 的 pid>` 錄一個 suite（`ci.py speedometer --browser kun --iterations 5 --suites …`），`perf script -F comm,ip,sym --no-inline` 之後，把 renderer main thread（`CrRendererMain`）每個樣本的葉子（libc 的配置和複製算給呼叫它的）按符號的前綴分類。RFC 0012 完成時這一步的前半是 `ci.py speedometer --sample --suites …`（Linux）：`perf` profile 本身就有 frame pointer，JS 的函式由 V8 命名，只留計時區間裡的樣本，寫成 folded stacks 和 `sample.md`；分類照樣在 folded stacks 上做。分類：parser（`html5ever`、`tendril`、`html::sink`）、GC 的 handle（`cppgc__`）、表單（`dom_api::listed_element`、`form_states`……）、bindings（`dom_js`、`dom_api::declare`）、V8 等；也看某個函式底下（inclusive）的分類。
3. **讀 Blink 對應的程式**（`chromium.googlesource.com` 的 `main`），確認 Chrome 怎麼做，寫進各項的「Blink」。

## 結論

- **jQuery、ES5 的差距不在 rendering pipeline**：兩邊的 style 和 layout 次數差不多（jQuery 的 layout：kun 79 次、Chrome 77 次），各幾十 ms。差距在 JS 呼叫的 DOM API 本身（trace 裡 `FunctionCall` 的 self；那時 kun 的 `innerHTML`、表單的簿記、GC 沒有 span，都算在這裡；之後 RFC 0012 的 2d 補上了 `ParseHTML` 和 V8 的 GC 事件）：jQuery 10 個 iteration，kun 7451 ms，Chrome 整段約 1 s。這一層裡，handle 的模型（第 2 項）散在各處，表單的簿記（第 3 項）和 `innerHTML`（第 4 項）是最大的兩塊。
- **React 的兩個 suite 被強制的 style 和 layout 拖慢**（第 1 項）：次數是 Chrome 的十幾倍。
- **Complex-DOM 的整頁 style 慢約 6 倍**（第 5 項），盤點時在不計時的載入裡，那時 kun 還丟掉 `var()`，Complex-DOM 少算了大量 style；`var()` 補上之後的比較表和 trace（`progress.md` 的「量測的紀錄」）裡，計時的步驟的 style 仍是 Chrome 的好幾倍。

## 差異

| # | 差異 | 影響（量到的） | 改動 | 狀態 |
| --- | --- | --- | --- | --- |
| 1 | `MouseEvent` 的 `pageX`、`pageY` 每次讀都強制整頁的 style 和 layout | React-Redux 強制的 style 1991 次、2926 ms（Chrome 整個 suite 的 style 155 次、86 ms）；React-Complex-DOM 991 次、3592 ms | 小 | 2026-10-13 做完：讀存著的捲動位置、不更新 layout；React-Redux 392.5 → 75.2 ms、React-Complex-DOM 416.6 → 144.3 ms |
| 2 | handle 的模型：每個 handle 一個配置的 `Persistent`，讀 `Member` 是 FFI 呼叫，node id 的表用 SipHash | jQuery、ES5 的 self 時間約 25%，散在 parser、表單、`dom` | 分段：2a 中、2b 中、2c 中（fork） | 還沒做 |
| 3 | 表單的簿記：每次呼叫結束走子樹、重算存起來，同一個節點走好幾次 | jQuery 31.8%、ES5 23.0%（inclusive） | 中到大 | 2026-10-13 做完：先在這裡做了 A（`form_states` 的子樹只走一次、只有 form 的 submit button 找 default button、`descendants` 不配置；jQuery 指令 −12%、ES5 時間約 −9%），再由 RFC 0042 改成元素層級（B；jQuery 指令再 −11%） |
| 4 | `innerHTML` 的解析每次約 Chrome 的 7 倍 | jQuery 38.6%（inclusive，含第 2 項的 9.7%） | 大 | 還沒做 |
| 5 | style 沒有 bloom filter、`MatchedPropertiesCache`、style sharing | Complex-DOM 載入的整頁 style 170 到 236 ms，Chrome 28 到 47 ms（不計時） | 各中 | 部分：RFC 0039（2026-10-13）把規則照最右邊的 compound 分桶，祖先的 bloom filter 量過之後不做；`MatchedPropertiesCache`、style sharing 還沒做（RFC 0039 的「之後的可能」） |
| 6 | 節點有 Rust 的 finalizer，sweep 在 main thread | jQuery 7.5% | 大 | 還沒做 |
| 7 | 每次宣告表的呼叫的固定成本 | bindings 加 V8 約 12%（jQuery），和 Chrome 的 `v8.callFunction` 同一個量級 | 中 | 還沒做 |

建議的順序（2026-10-13）：1（小、照 Blink、React 的兩個 suite）→ 3（jQuery、ES5 共同的熱點）→ 2a → 2b、2c → 4 → 5 → 6、7。

## 每一項

### 1. `MouseEvent` 的 `pageX`、`pageY`

- **kun**：`dom_api::ui_event::page_x`、`page_y` 經過 `view_scroll` 呼叫 `geometry::scroll_x`、`scroll_y`，它們經過 `viewport` 的 `with_layout`：先更新整份 document 的 style 和 layout，再讀捲動位置。
- **Blink**：`MouseEvent::InitCoordinates`（`core/events/mouse_event.cc`）在建構時讀 `frame->View()->LayoutViewport()->PixelSnappedScrollOffset()` 加到 `page_x_`、`page_y_`，存起來，不更新 layout。（`LocalDOMWindow::scrollX`、`scrollY` 會更新 layout，但 `MouseEvent` 不經過它們。）
- **證據**：React 的 SyntheticEvent 在建立時讀 `MouseEvent` 介面的每個屬性。React-Complex-DOM 的 `perf`（0040 之前）：強制 style 的 28.2% 來自 `geometry::viewport` ← `scroll_x` ← `ui_event::view_scroll`。React-Redux 約每個 click 一次強制的 style（10 個 iteration 1991 次），來源照次數推，還沒用 `perf` 確認。
- **做法**（2026-10-13）：讀的時候直接問 layout client 的 `viewport`（frame 存著的捲動位置），不經過 `with_layout`。照 CSSOM View 讀「現在」的位置，不照 Blink 在建構時算好：WPT 的 `css/cssom-view/mouseEvent.html` 要捲動之後讀到新的位置。

### 2. handle 的模型

- **kun**：
  - `NodeHandle` 等是 `cppgc::Persistent` 的 root：`cppgc__Persistent__CONSTRUCT` 是 `new cppgc::Persistent`（rusty_v8 的 `src/binding.cc`），每個都要配置和登記到 persistent region。
  - `Member`、`Persistent`、`WeakPersistent` 的讀取（`cppgc__Member__Get` 等）是 FFI 呼叫，Rust 這邊不能 inline：走 parent、children、sibling 都經過它。
  - `node_ids`（`NodeId` 到節點）是 std 的 `HashMap`（SipHash），`handle_by_id`、`node_by_id` 每次查一次。
  - 熱點上的例子：html5ever 的 Handle 型別是 `Handle<Node>`（open elements 的 stack、每次 append 和 clone 都是 `Persistent` 的操作）；表單的走訪把節點收成 handle；`insert_before` 的 `adoption_targets` 為了比較兩個節點的 document 建 handle。
- **Blink**：stack 上用裸指標（Oilpan 的 conservative stack scanning 保活），`Member<T>` 的讀取是 inline 的載入（有 pointer compression 時加上解壓縮），`Persistent` 只用在 heap 外長期持有的地方；節點沒有 id 的表（`DOMNodeIds` 只給 DevTools 等，用到才編號）。
- **證據**（jQuery，renderer main thread 的 self 時間；ES5 差不多）：cppgc 的 handle 操作 18.6%（建 `Persistent` 6.9%、`Member::Get` 5.1%、`Persistent::Get` 2.5%、`WeakPersistent::Get` 2.2%），hash 5.0%，`handle_by_id`、`to_handle`、`handle_of`、`node_by_id` 3.3%。
- **可能的做法**（各自獨立）：
  - 2a：`node_ids` 和其他以 id 為 key 的表改用 FxHash（`rustc-hash`，`css` 已經用）。
  - 2b：熱點少建 handle：在一個 scope 裡用 `NodeRef`、`NodeId` 比較；parser 的 Handle 型別換成不 root 的（要看 GC 在 parse 期間能不能跑，`crates/dom/soundness.md`）。
  - 2c：rusty_v8 的 fork 讓 `Member::Get` 在 Rust 裡 inline（照 cppgc 的 pointer compression 的設定解壓縮），`Persistent` 不另外配置；改 fork 的 C++ 要重編 V8。

### 3. 表單的簿記

- **kun**：只要文件裡出現過表單元素，`dom` 的每次插入和移除都記一筆（`FormChange`、`FormStateChange`），宣告表的每次呼叫結束時 `update_elements` 處理：`listed_element::update_all` 走每一筆的子樹重設 form owner，`form_states::update_form_states` 走子樹、重算 `:checked`、`:disabled`、`:default` 等存起來。fragment parser 每插入一個節點記一筆，處理時子樹已經長完整，同一個節點被走好幾次；submit button（`<button>` 預設是 submit）一改，就走它整棵樹找 default button，沒有 form 也是。
- **Blink**：`ListedElement::InsertedInto`、`RemovedFrom` 只對 listed element 在它插入、移除時處理一次，必要時才 `ResetFormOwner`；disabled 的祖先狀態設成 unknown，用到才算（`ancestor_disabled_state_`）；`:checked` 等在 selector 比對時問元素（`IsChecked()`）；`:default` 走那個 form 自己的 listed elements（`HTMLFormElement::FindDefaultButton`），改了只對那個 form 的 submit button `PseudoStateChanged`（`InvalidateDefaultButtonStyle`）。
- **證據**：`update_elements` 在 jQuery 佔 31.8%、ES5 23.0%（inclusive）；`Affected::subtree` 8.1%、`update_all` 的 self 3.1%、`descendants` 每層收集 children 的配置 3.2%。
- **量到的重複**（jQuery 的 1 個 iteration，含載入）：有表單紀錄的 batch 平均約 800 筆；`form_states` 的子樹走了 2.30 M 個節點、每個 batch 不重複的 0.62 M；找 default button 每個 batch 走整份文件（0.86 M，頁面沒有 `<form>`）；`listed_element` 的 `listed_in`、`inserted_at` 各 1.38 M、不重複的 0.31 M；移除的子樹 0.92 M、0.62 M。
- **決定**（2026-10-13）：
  - 做了 A1（`form_states` 的子樹在一個 batch 裡只走一次）、A2（只有有 form owner 的 submit button 依賴它的樹；離開 form 的記下那個 form）、A0（`descendants` 沿 child、sibling 的 edge 走，不配置）。
  - `listed_element` 的重複照 B（RFC 0042，2026-10-13 做完；插入時的走訪合成一趟量過之後不做，`adoption_targets` 的 4.6% 歸第 2 項）：`dom` 在插入、移除的當下走一次子樹，只記和表單有關的元素（不記子樹的根），`dom_api` 每個元素處理一次；插入時已經走子樹的 `record_frame_owners` 可以合成一趟。不選 A3（倒著處理紀錄、剪掉走過的子樹）：radio 和 option 的「後插入的勝出」（`not_yet`）要靠處理順序的論證，之後改的人難守住。
  - C（pseudo-class 比對時才算，Blink 的做法）現在不做：`style` 要能回頭呼叫 `dom_api` 的表單邏輯，或把它搬進 `dom`，另外要做失效；A、B 之後還是熱點再考慮。

### 4. `innerHTML` 的解析

- **kun**：html5ever（`tendril`、`BufferQueue` 逐字元）加上 `html::sink`，每個節點經過 `DomMutationScope` 的 `create_node`、`insert_before`。
- **Blink**：`HTMLDocumentParser`（自己的 tokenizer 和 tree builder）；`HTMLDocumentParserFastPath` 處理簡單的 markup，但 jQuery 和 ES5 在 Chrome 的 trace 裡走的是完整的 parser（`ParseHTML`、`PumpTokenizer`），每次約 0.1 ms。
- **證據**：jQuery 的 `set_inner_html` 38.6%（inclusive）：html5ever 本身 14.4%、handle 9.7%、`dom` 的建立和插入 8.0%；照樣本換算每次約 Chrome 的 7 倍。
- **可能的做法**：先做第 2 項（handle 那部分）再量；之後看 tokenizer 的熱點（`pop_front_char`、`push_bytes_without_validating`），或照 Blink 加 fast path。

### 5. style 的吞吐量

- **kun**：`docs/limitations.md` 的 style：沒有祖先的 bloom filter（Blink 的 `SelectorFilter`）、沒有 `MatchedPropertiesCache`、沒有 style sharing；以子樹為單位重算（Blink 用 invalidation set）。
- **證據**：Complex-DOM 每個 iteration 重新載入時的整頁 style：Svelte 每次約 170 ms（Chrome 最多 28 ms），Angular、React、Preact 約 215 到 236 ms（Chrome 36 到 47 ms）。這些在不計時的載入裡；`var()` 補上之後計時的步驟裡也看得到（`progress.md` 的「量測的紀錄」）。
- **可能的做法**：bloom filter、`MatchedPropertiesCache`，各自一份工作。

### 6. GC 的 finalizer

- **kun**：節點的狀態（`NodeState`：屬性的 `Vec`、字串等）有 Rust 的 drop，cppgc 對每個節點跑 finalizer，sweep 的 finalize 在 main thread 的 platform task 裡（`DomIsolate::run_platform_tasks`）。
- **Blink**：`Node` 沒有 finalizer（屬性在 GC 的 `ElementData` 裡），沒有 finalizer 的 page 可以在別的 thread 上 sweep。
- **證據**：jQuery 的 `run_platform_tasks` 7.5%（其中 `free` 和 `NodeState` 的 drop 為主）。
- **可能的做法**：節點的狀態放進 GC 的物件、拿掉 finalizer。大的改動，排在後面。

### 7. 每次呼叫的固定成本

- **kun**：宣告表的每次呼叫配置一個 boxed future（`on_the_spot`），結束時跑 `update_elements`、mutation observer、custom element 的 backup queue、selectionchange 四個收尾。
- **Blink**：bindings 是產生的直接呼叫；這些收尾掛在真的有變化的地方。
- **證據**：bindings 6.2% 加 V8 6.0%（jQuery 的 self）；和 Chrome 的 `v8.callFunction` 的 self（10 個 iteration 352 ms）同一個量級。
- **可能的做法**：沒有變化時跳過收尾、不配置 future。排在最後。

## 次數的對照

2026-10-13，`9e9e6913`，每個 suite 10 個 iteration（含重新載入）；kun 的「強制」是在 `FunctionCall` 裡的，「frame」是其他的。

| Suite | kun 的 style：強制（次、ms）/ frame | Chrome 的 style | kun 的 layout：強制 / frame | Chrome 的 layout |
| --- | --- | --- | --- | --- |
| TodoMVC-React-Redux | 1991、2926 / 170、69 | 155、86 | 2032、85 / 85、10 | 106、94 |
| TodoMVC-React-Complex-DOM | 991、3592 / 186、4739 | 153、282 | 1022、221 / 110、202 | 113、215 |
| NewsSite-Next | 1556、409 / 200、79 | 1651、296 | 1582、762 / 31、14 | 1616、464 |
| NewsSite-Nuxt | 1501、269 / 210、439 | 1656、318 | 1522、311 / 90、1033 | 1616、438 |
| React-Stockcharts-SVG | 241、246 / 170、113 | 366、132 | 272、88 / 72、13 | 262、101 |
| Editor-TipTap | 91、40 / 90、20 | 150、30 | 112、704 / 1、0 | 95、522 |
| Editor-CodeMirror | 51、50 / 120、34 | 130、32 | 82、54 / 12、3 | 106、95 |
| Charts-chartjs | 51、2 / 99、15 | 119、16 | 82、5 / 18、1 | 116、43 |
| Charts-observable-plot | 1、0 / 130、172 | 115、121 | 2、0 / 78、16 | 112、111 |
| Perf-Dashboard | 11、0 / 140、17 | 1305、77 | 42、3 / 49、1 | 1281、177 |
| TodoMVC-jQuery | 89、11 / 232、69 | 222、52 | 70、1 / 86、13 | 147、71 |
| TodoMVC-JavaScript-ES5 | 1、0 / 240、72 | 149、42 | 22、0 / 93、13 | 110、63 |
| TodoMVC-Backbone | 51、55 / 330、80 | 216、69 | 62、6 / 97、9 | 158、89 |
| TodoMVC-Vue | 1、0 / 180、116 | 142、119 | 22、1 / 90、25 | 104、95 |
| TodoMVC-WebComponents | 11、0 / 183、16 | 150、51 | 12、0 / 70、3 | 138、56 |
| TodoMVC-Angular-Complex-DOM | 1、0 / 210、7893 | 160、370 | 22、7 / 110、206 | 118、253 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 1、0 / 189、3845 | 156、231 | 22、6 / 121、201 | 113、170 |
| TodoMVC-Lit-Complex-DOM | 1、0 / 163、1767 | 132、340 | 2、0 / 90、77 | 126、260 |
| TodoMVC-Preact-Complex-DOM | 1、0 / 179、7560 | 142、343 | 22、7 / 111、202 | 107、244 |
| TodoMVC-Svelte-Complex-DOM | 1、0 / 190、5917 | 147、184 | 22、7 / 121、185 | 98、143 |

Complex-DOM 的 frame 的 style 大多是每個 iteration 重新載入時的整頁 style（第 5 項）。

## 之前找到、處理過的

用 `perf` 找到、後來看是架構差異的（細節在 `progress.md`）：

| 差異 | Blink | 處理 |
| --- | --- | --- |
| `focus()` 對已經有 focus 的元素也 layout、捲動 | `Element::Focus` 第一件事就 return | 2026-10-13，照 Blink；jQuery 1729 → 849 ms |
| live collection 用 index 讀是 O(n) | `CollectionIndexCache` | 2026-10-13，沿用的項目直接讀第幾個 |
| 規則逐條比對 | `RuleSet` 照 id、class、tag 分桶 | 2026-10-13（RFC 0039） |
| 屬性和元素的名稱是字串 | `QualifiedName`（atom） | 2026-10-13（RFC 0040） |
