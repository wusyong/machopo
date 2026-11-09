# RFC 0044：升級到 taffy 0.15

- 開始日期：2026-10-13
- 狀態：✅ 已完成：2026-10-13（第 1 到 5 條；兩段：升級和絕對定位改用 taffy 的 hoisting、行內的絕對定位盒子。驗收見 `progress.md`）
- 細節：`details.md`；進度：`progress.md`

## 摘要

把 taffy 從 0.14 升到 0.15。直接的原因是 RFC 0043 的第 4 段（float）：0.14 給同一個 BFC 裡的子 block 的位置是 margin 還沒摺疊的位置，float 在 BFC 裡記的位置和畫出來的差一個摺疊掉的 margin，行內內容繞 float 時就會錯；0.15 修好了。0.15 也把絕對定位改成 Blink 的模型：out-of-flow 的盒子由排它的父盒子算出 static position，當成候選往上傳，到它的 containing block 才排（`compute_oof_layout`）。kun 拿掉自己的那一套（0×0 的佔位盒子、兩趟排版、`hoist_out_of_flow`），改用 taffy 的；inline formatting context 裡的絕對定位盒子照 Blink 當成行內的項目，kun 從斷好的行算出它的 static-position rectangle，atomic inline 傳上來的也經過它所在的行往上傳。0.15 修好的 block、flex、grid 和絕對定位的問題也一起拿到：`css/` 底下的 WPT 有 20 個變通過、2 個變失敗（0.15 的 grid 不重排 `fr` 的列，記成限制）。

## 動機

**當時的情況**（2026-10-13）：kun 用 crates.io 的 taffy 0.14（feature：`std`、`block_layout`、`flexbox`、`grid`、`content_size`、`calc`），照 RFC 0003 第 14 條不 fork，它的問題記成限制、等上游修好再升級。0.15 已經發佈。

**問題**：

- **float 的位置**（RFC 0043 第 4 段要用的 `float_layout`）：0.14 的 block layout 給同一個 BFC 裡的子 block 的 `BlockContext` 用 `y_offset_for_absolute + margin.top` 當位置，也就是前一個的下 margin 加上自己的上 margin，沒有摺疊。原型（`details.md` 的「float 的位置」）：A 的下 margin 20、B 的上 margin 10，B 裡的 float 畫在 y 70 到 170，taffy 卻記成 80 到 180，y 172 的 BFC 盒子被推到 x 100（Chrome 是 0）。最常見的 `<img style="float: left"><p>…</p>`（`<body>` 和 `<p>` 的 margin 摺疊）也會中：`<p>` 的行以為自己比 float 低 1em，提早不繞，最後一行壓到 float 上。0.15 用摺疊後的 margin 估計位置，同一個原型正確。0.14 上只能 fork（違反 RFC 0003 第 14 條）或記成限制。
- **0.15 的 API 不相容**：絕對定位改成 hoisting 的模型（`LayoutContainingBlock` trait、`LayoutOutput::oof_candidates`、`compute_oof_layout`），`Position` 多了 `Static`、`Fixed`，`align_items` 等四個對齊不再是 `Option`，leaf 和 flex layout 不再自己扣掉 margin（呼叫的一方扣）。kun 實作的是 taffy 的低階 trait，這些都要跟著改。

解決的 `docs/limitations.md` 的限制（「Layout」）：

- 絕對定位：出現在 inline formatting context 裡的絕對定位盒子，static position 是父盒子的邊框盒原點，不是它在行裡的位置；`display: inline-block` 的絕對定位盒子排在幾個 inline-block 之間時，`top: auto` 讓它落在那一行的下面；絕對定位的盒子不套用 `justify-self` / `align-self`；grid container 的絕對定位子盒子不照 grid 的區域（排版照了，`getComputedStyle` 的 inset 還沒，限制改寫成那一半）。
- block layout 的 padding 和 border 的百分比，上下兩邊對著 containing block 的高度算。
- flex 演算法：百分比的 `flex-basis` 碰到不確定的大小時用 `height` 而不是 `content`；grid 演算法：`1fr` 那一列的 item 的 `max-height: 100%` 沒有作用。
- `fit-content()` 的參數是有百分比的 `calc()` 時只用它的長度那一部分。
- `position: fixed` 的根元素：整份文件什麼都不畫。

限制裡其他寫明是 taffy 的那幾條（flex base size 被 `max-width` 限制等）0.15 照舊，留著；新加的：高度不確定的 grid 裡 flex factor 加起來小於 1 的 `fr` 列不重排、`position: relative` 的 inline 元素不是包含塊（以前就是，這次確認後寫下）。

**不在範圍內**：float 本身（RFC 0043 第 4 段，開 `float_layout` 也在那裡）；`position: fixed` 的 containing block 照 `transform`（0.15 有 `CoreStyle::is_containing_block`，但 kun 直接給 `taffy::Style`，要換成自己的 style 型別，見「之後的可能」）；`min-*` / `max-*` 的關鍵字（0.15 照舊是 `LengthPercentageAuto`）；`position: sticky`；直書（0.15 的 `transpose()`）。

## 設計概要

網頁的作者看到的：

- 排版大致不變；0.15 修好的 flex、grid、block 的邊角情況和 Chrome 一樣了（例如百分比的 `flex-basis`、上下的百分比 padding）。
- 絕對定位的盒子照 `justify-self` / `align-self` 在它的 static-position rectangle 裡對齊；grid container 的絕對定位子盒子照它的 grid 區域。
- 段落裡的 `<span style="position: absolute">`（兩邊的位移都是 `auto`）出現在它在行裡的位置，不再跑到段落的左上角，也不再把行切開；原本是 block-level 的，前面同一行有內容時在下一行的開頭。

改 kun 的人看到的：`layout` 的 box tree 沒有 0×0 的佔位盒子和兩趟排版；out-of-flow 的盒子照舊是它父盒子的 child（box tree 照 DOM），taffy 排完它的 containing block 時告訴 kun 它掛在哪裡（`add_hoisted_children`），fragment tree 把它放在 containing block 底下。行內的 out-of-flow 盒子是 `InlineItem::OutOfFlow`，kun 的 inline formatting context 自己組候選往上傳。

## 詳細設計

### 升級和 taffy 的 hoisting

- **依賴**：`Cargo.toml` 的 `taffy = "0.15"`，feature 同上（`detailed_layout_info` 在 0.15 是 no-op，grid 的詳細資訊一律有）。`rust-version` 1.71，不影響 kun 的 toolchain。
- **style 的對應**（`layout` 的 `taffy_style`）：`Position` 直接對到 0.15 的 `Static`、`Relative`、`Absolute`、`Fixed`，inset 一律給（0.15 的 static 忽略 inset）；四個對齊的 `normal` 是 `NORMAL`；`justify-self` / `align-self` 的 `normal` 是 `Some(NORMAL)`，不再當成 `auto`；`fit-content()` 有百分比的 `calc()` 時用 `fit_content_calc`；grid 裡有自然大小的 replaced item 設 `item_is_replaced`（0.15 的 grid 在 `normal` 下自己不拉伸它），拿掉 kun 原本改成 `start` 對齊的繞法；`grid-row` 等只寫一個 ident 時是 `NamedLine(name, 0)`（0.15 用 0 表示「先找區域的邊」，1 是「第一條叫這個名字的線」；做的時候 `grid-area: head` 因此找不到 `head-start`，`grid::items_placed_in_areas` 抓到）。
- **margin**：0.15 的 leaf 和 flex 不再扣掉自己的 margin，`AtomicInlines::lay_out` 原本「leaf 和 flex 不扣、block 和 grid 扣」的分支改成一律由 kun 扣。
- **絕對定位**（第 2 條）：`BoxTree` 實作 `LayoutContainingBlock`（`get_oof_item_style`、`clear_hoisted_children`、`add_hoisted_children`、`get_detailed_layout_info`）；`compute_box` 在 `compute_cached_layout` 裡、box 的演算法跑完之後，`PerformLayout` 時呼叫 `compute_oof_layout`（taffy 的規定：cache 命中時不重跑；量大小時 taffy 不設 `oof_positioning_area`，呼叫了也是空的，所以照 `TaffyTree` 只在 `PerformLayout` 呼叫）。拿掉 `placeholder`、`is_placeholder`、`in_flow`、`in_flow_pass`、`with_absolute`、`set_in_flow`、`hoist_out_of_flow`、`hoist_below`、`resolve_static_positions`、`static_position`：taffy 的 block、flex、grid 自己算 static position（block 是假想的位置，flex、grid 照對齊和 grid 區域），候選照樹的順序往上傳。`LayoutBox::absolute` 是 `add_hoisted_children` 給的清單，`fragment()` 把它們排在 in-flow 的 child 後面（paint 和 hit test 的順序和以前一樣），盒子自己的 out-of-flow child 不放（它們在 `children` 裡、算 static position 用，fragment 是包含塊的）；`last_baseline` 也跳過它們（包含塊是 inline-block 自己時，那個 child 已經排好、有行，不跳過會拿它當 baseline）。`position: fixed` 的盒子由 `compute_root_layout` 最後的那一趟排（containing block 是 viewport，和以前一樣）。
- **grid 的詳細資訊**：`LayoutGridContainer::set_detailed_grid_info` 存下每個 grid container 的 track（`LayoutBox::grid_info`），`get_detailed_layout_info` 交回去，絕對定位的 child 照它的 grid 區域。
- **局部重排**：kun 的 `needs_layout` 清掉一路到根的 taffy cache；out-of-flow 的盒子變了時，它的 containing block 在那條路上。0.15 的 cache 把 `oof_candidates` 存在 final layout 的 entry 裡，命中時候選照樣往上傳。`update.rs` 的測試守住「局部重排和整份重排一樣」。

### 行內的絕對定位盒子

- **box tree**（第 3、5 條）：`collect` 把 out-of-flow 元素放成 `InlineItem::OutOfFlow(id, style)`，`set_content` 照第 5 條決定它留在行內內容裡（不切開 inline formatting context；Blink 的 `InlineItem::kOutOfFlowPositioned`）還是 block 子盒子（flex、grid container 裡一律是）；只有 out-of-flow 盒子和空白的容器也是 inline formatting context。行內的 out-of-flow 盒子和 atomic inline 一樣是 inline formatting context 的盒子的 child（局部重排從它往上走得到包含塊），空白照它不在那裡處理（Blink 的 `InlineItemsBuilder` 也是）。
- **style**：`ComputedStyle::original_display` 記下 blockify 之前的 `display`（Blink 的 `ComputedStyle::OriginalDisplay`）：out-of-flow 的盒子都被 blockify 了，static position 要看它原本是 inline-level 還是 block-level。
- **static-position rectangle**：`InlineContent` 為它放一個 parley 的 `InlineBoxKind::OutOfFlow` 的 inline box（parley 給它「當成 0×0 的 inline box」的位置，不佔空間、不影響行高和斷行）；`compute_inline` 在 `PerformLayout` 時用斷好的行算出每個的 static-position rectangle（css-position 3；Blink 的 `InlineLayoutAlgorithm::PlaceOutOfFlowObjects`）：原本是 inline-level 的（`display: inline`、`inline-block` 等），inline 軸是它在行裡的那一點、block 軸是那一行的上下緣；原本是 block-level 的（CSS 2 § 10.3.7 的假想盒子是一個 block），inline 軸是 content box 的左右緣、block 軸是一條線：前面同一行有內容（文字或 atomic inline）時在那一行的底端，否則在頂端。它照自己的 `justify-self`（`auto` 時用容器的 `justify-items`）和 `align-self`（`auto` 時是 `normal`）在 rectangle 裡對齊（`AxisStaticPosition::from_alignment`），和 taffy 的 block layout 對 block 的 out-of-flow child 一樣。原本打算給一個點（`from_edge`），WPT 的 `css-align/abspos/align-self-static-position-001` 因此回歸（只有一個絕對定位的 child 的 block 變成 inline formatting context，`justify-self: end` 不見了），改成照規範的 rectangle；第一版也在 taffy style 還沒放回 box 時讀容器的 `justify-items`，讀到預設值（`align-items-static-position-001.tentative` 抓到）。
- **往上傳**：候選照樹的順序放進 `LayoutOutput::oof_candidates`；inline formatting context 自己是 containing block 時（`position: relative` 的段落），它的 `oof_positioning_area` 是 padding box，`compute_box` 照樣呼叫 `compute_oof_layout`。`inline_fragments` 不放它（它不在行裡畫）。
- **atomic inline 傳上來的**：atomic inline 由 kun 排（`AtomicInlines::lay_out`），不經過 taffy 的父盒子，它的 `LayoutOutput` 裡沒被認領的候選由它所在的行接著往上傳：`lay_out` 把它們存在 box 上（`LayoutBox::oof_candidates`），`compute_inline` 照 atomic inline 的 border box 在行裡的位置（連同它自己的 `position: relative`）平移、和自己的候選照樹的順序合在一起。第 1 段漏了這一點：inline-block 裡、包含塊在 inline-block 外面的絕對定位盒子沒有被排（沒有 fragment），WPT 沒有測到，第 2 段開始時發現、一起修好。
- 這一段也是 RFC 0043 的 inline-level float 會用到的形狀（行內的項目、parley 的 out-of-flow box）。

### 測試

- **自動**：`cargo test -p layout` 的 `position.rs`（hoisting、對齊和 grid 區域、行內的 out-of-flow 盒子、經過 atomic inline 往上傳）、`update.rs`（局部重排）、`grid.rs`、`block.rs`、`inline_block.rs`；`style` 的 `cascade.rs`（`original_display`）；`cargo test --workspace`（`geometry`、`paint` 的 hit test、`dom_api` 的 CSSOM View 讀到的幾何）。
- **reftest 和 WPT**：`test.py reftest`（`position/`、`flex/`、`grid/`、`block/`、`inline/`、`text/`）；`test.py wpt` 逐個目錄跑 `include.txt` 裡 `css/` 底下的 38 個目錄（一次跑整個 `css/` 在 Windows 上命令列太長），預期結果用 `MACHOPO_UPDATE_EXPECTATIONS` 更新，每個改變在 `progress.md` 寫原因。
- **情境**：畫面看不出差別，現有的情境當回歸檢查（`test.py scenarios`）。
- **mutation test**：每段一份（`mutants-1.toml`、`mutants-2.toml`）。
- **效能**：不量（`docs/benchmarks.md` 的「效能的量測」：只在使用者要求時量）。0.15 說 grid 快約 60%、flex 快約 15%，下一次量 Speedometer 時看得出來。原本草稿寫要量 Complex-DOM 和 NewsSite 的 suite 和 0.14 比，第 1 段做完時拿掉（量了 `main` 的一輪就停了，數字沒有用）。

### 分階段

1. **升級和絕對定位**：依賴、style 的對應、margin、`LayoutContainingBlock`、拿掉 kun 的佔位盒子和兩趟排版、grid 的詳細資訊；預期結果更新。在這一段，行內內容裡的絕對定位盒子照舊把它切成匿名 block，static position 是兩個匿名 block 之間。
2. **行內的絕對定位盒子**：`InlineItem::OutOfFlow`、parley 的 `OutOfFlow` box、從行算 static-position rectangle、經過 atomic inline 往上傳。

原型：「float 的位置」的原型做過（`details.md`）；API 的差異在試編時看到（`details.md` 的「API 的差異」）。

## 缺點

- 0.15 是新版，它的新問題直接變成 kun 的（和 0.14 一樣記成限制，不 fork）：這次的是 grid 的 `fr` 列不重排。
- 絕對定位改用 taffy 的之後，kun 對 static position 和 hoisting 的控制變少（例如 `position: fixed` 的 containing block 要等 kun 換成自己的 style 型別才能照 `transform`）；kun 自己排的 atomic inline 要記得把候選往上傳（第 1 段漏了一次）。
- `InlineContent` 多一種項目，`collect` 和 `set_content` 要分辨「跟著行內內容」和「在 block 之間」的 out-of-flow 盒子。

## 理由和其他做法

完整的比較在 `details.md`。

- **升級（第 1 條）對 fork 0.14、只 backport float 的修正**：fork 只改幾行，但違反 RFC 0003 第 14 條，多一個 fork 要跟著每次升級 rebase，0.15 修好的其他問題也拿不到；升級是那一條本來就預期的路。
- **絕對定位用 taffy 的（第 2 條）對保留 kun 自己的**：taffy 0.15 的模型就是 Blink 的（候選往上傳到 containing block、在那裡排），static position 由排它的演算法算，flex 和 grid 的對齊、grid 區域都對；保留 kun 的佔位盒子要自己組候選、自己呼叫 `compute_oof_layout_for_area`，兩套重疊，而且 flex、grid 的 static position 照舊不對。
- **行內的絕對定位盒子當成行內的項目（第 3 條）對照舊切開行內內容**：照舊的話 kun 一直有「段落裡的絕對定位盒子把行切開、static position 不對」的限制；Blink 也是行內的項目。代價是 `collect` 和 `InlineContent` 多一種項目。

## 先例

- **Blink**（LayoutNG）：out-of-flow 的後代在 fragment builder 裡當成候選（`LogicalOofPositionedNode`，`BoxFragmentBuilder::AddOutOfFlowChildCandidate`）往上傳，到 containing block 由 `OutOfFlowLayoutPart` 排；static position 由排它的演算法記下（block 是假想的位置，flex、grid 照對齊），行內的是 `InlineItem::kOutOfFlowPositioned`，在 `InlineLayoutAlgorithm::PlaceOutOfFlowObjects` 照行的位置和它原本的 `display`（`OriginalDisplay`）算；哪些 out-of-flow 盒子進行內內容照 `LayoutBlockFlow::AddChild`。照做：全部；taffy 0.15 已經是這個模型，kun 只補行內那一側。不照做：`position: relative` 的 inline 元素當包含塊（kun 的 inline 元素沒有盒子，記成限制）。
- **taffy 0.15**：`compute_oof_layout`、`LayoutContainingBlock`、`OofCandidate`、`AxisStaticPosition`（`details.md` 的「API 的差異」）。

## 已決定的事項

2026-10-13 確認：

1. **升級到 taffy 0.15，不 fork 0.14**：0.14 給同一個 BFC 裡的子 block 的位置沒有摺疊 margin，float 的位置會錯（`details.md` 的「float 的位置」），0.15 修好了。不選 fork 0.14、只 backport 那幾行：違反 RFC 0003 第 14 條，多一個 fork 要維護，0.15 修好的其他問題也拿不到。不選留在 0.14、記成限制：最常見的「float 加段落」就繞錯。
2. **絕對定位用 taffy 0.15 的 hoisting，拿掉 kun 自己的佔位盒子和兩趟排版**：0.15 的模型就是 Blink 的（`OutOfFlowLayoutPart`，候選往上傳到 containing block），flex、grid 的 static position 和 grid 區域都由排它的演算法算。不選保留 kun 的、自己組 `OofCandidate` 呼叫 `compute_oof_layout_for_area`：兩套重疊，flex、grid 的 static position 照舊不對（`details.md` 的「絕對定位」）。
3. **inline formatting context 裡的絕對定位盒子當成行內的項目，做在這份 RFC 的第 2 段**（Blink 的 `InlineItem::kOutOfFlowPositioned`）：用 parley 的 `OutOfFlow` box 算出它在行裡的 static position，`ComputedStyle` 記下 blockify 之前的 `display`。不選照舊切開行內內容、留給之後：「段落裡的絕對定位盒子把行切開、static position 不對」的限制會一直在，而且 RFC 0043 的 inline-level float 要用同樣的形狀。
4. **WPT 的預期結果變壞的**：查清楚是 0.15 的問題就記成限制（不 fork、不在 kun 繞過），是 kun 接錯就修；有大量、明顯的回歸（例如常見的 flex 版面變錯）時停下來，重新討論要不要升。結果只有 2 個變失敗，都是 grid 的 `fr` 列不重排（0.14 時碰巧通過）。
5. **哪些 out-of-flow 盒子是行內的項目：照 Blink 的 `LayoutBlockFlow::AddChild`**（2026-10-13，第 2 段開始前）：容器只有行內內容和 out-of-flow 盒子時，整個是一個 inline formatting context、每個 out-of-flow 盒子都是行內的項目；有 block 子盒子時，out-of-flow 盒子加入緊接在它前面的那段行內內容，前面是 block、容器的開頭、或只有會摺掉的空白（Blink 不為它建 layout object）時是 block 子盒子。不選「前後都有行內內容才算」：在一段文字結尾的 out-of-flow 盒子會是 block 子盒子，static position 和 Chrome 差一行。

## 待決定

無。

## 之後的可能

- `position: fixed` 的 containing block 照 `transform`、`filter` 等：kun 的 taffy style 換成自己的型別，實作 `CoreStyle::is_containing_block`。
- `position: relative` 的 inline 元素當它裡面的絕對定位盒子的包含塊（Blink 的 `LayoutInline` 是 containing block，區域是它在各行的框的聯集）：kun 的 inline 元素沒有盒子，要在 inline formatting context 裡自己認領候選。
- `getComputedStyle` 的 `auto` inset 照 grid 區域算。
- `min-*` / `max-*` 的關鍵字：等 taffy 支援。
- 直書：0.15 的 `LayoutInput::transpose()`、`LayoutOutput::transpose()`。
- `position: sticky`：0.15 的 `Position::Sticky` 只排成 static（當成 containing block），黏住要 kun 自己做。
