# RFC 0105 的細節：layout 的座標模型和分段

2026-11-19 讀程式、原始碼和上游的 issue 得到的。標「推測」的沒有直接驗證。

## machopo 現在的 layout

### 架構

- 全部在 `crates/layout`（box tree、taffy 的 block layout、parley 的 inline layout、不可變的 fragment tree）；沒有 README。
- `BoxTree`（`lib.rs:146`）是 `LayoutBox` 的清單，以 taffy 的 `NodeId` 為索引；每個 box 存 `taffy::Style`、`Cache`、`Layout`（`lib.rs:200-316`）。inline 元素不是 box，是 `InlineContent` 裡的開頭和結尾（`lib.rs:11-18`）。
- 分派（`lib.rs:2073-2079`、`2503-2600`）：table 是 machopo 自己的（`table.rs`，約 3000 行，對 taffy 是 leaf）；inline formatting context 是 `compute_inline`（`lib.rs:1620`，用 parley，`text.rs`）；flex、grid、block 是 taffy 的；out-of-flow 是 taffy 的 `compute_oof_layout`；float 用 taffy 的 float context 加 machopo 的 `place_float`、`FloatBands`；sticky 在 layout 之後（`sticky.rs`）；`-webkit-box` 對到 taffy 的 flex。
- fragment tree：`BoxFragment`（`fragment.rs:15`，照 Blink 的 `PhysicalBoxFragment`）存物理的 `size`、上右下左的 `padding`、`border`、`margin`、子 fragment 的 `FragmentLink.offset`（相對父的 border box 原點，`fragment.rs:651-654`）、行的 `LineFragment.rect`；沒有任何邏輯座標。taffy 的 `Layout` 在 `BoxTree::fragment` 換成 fragment（`lib.rs:1297-1366`）。
- 讀 fragment 的：paint（`paint/src/lib.rs`）、hit test（`paint/src/hit_test.rs`）、捲動（`renderer/src/scroll.rs`、`compositor/src/scroll_node.rs` 把位移限在 `0..=max`）、CSSOM（`dom_api/src/geometry.rs` → `LayoutQuery` → `layout::client_rects`）、IntersectionObserver、scroll anchoring。

### 假設物理方向的地方

`.left/.right/.top/.bottom` 和 `.width/.height` 的次數：`layout` 501、466（`lib.rs` 168、195，`geometry.rs` 76、30，`table.rs` 72、36，`text.rs` 70、25，`sticky.rs` 44、21）；`paint` 189、279；`renderer` 23、93；`dom_api` 13、70；`compositor` 6、40；`.x/.y` 另外約 1190 處。

分類：

- style 只存物理的：邏輯的 longhand 在 cascade 時換成物理的（`css/src/properties.rs:1297-1340`）。
- margin、padding、border、inset 照邊交給 taffy（`lib.rs:3489-3535`）；taffy 的 `Style.direction` 從來沒設（預設 `Ltr`）。
- inline：行從 `origin.x + inline_min_coord` 開始（`text.rs:1662`）；`Edge::start()` 是「left：只有橫排由左到右」（`text.rs:2206-2213`）；cluster 當成由左到右（`text.rs:2396`）。
- 對齊交給 parley；caret 把 `Start` 當成 `Left`（`paint/lib.rs:4849-4852`）。
- float、clear 只有 left、right（`computed_style` 的 `lib.rs:662-680`）；`FloatBands` 記左邊的 x 和寬度。
- 定位：`relative_offset` 先看 `left`（`text.rs:2135-2157`）；inline 的 out-of-flow 的靜態位置寫死 `taffy::Direction::Ltr`（`lib.rs:1831-1839`）；sticky 用上右下左。
- list marker 放在 `line.x - size.width`（`lib.rs:1515-1535`）；`text-overflow` 從右邊截（`text.rs:362`）；table 的欄從左排（`table.rs:2488`）。
- 可捲動的範圍只往右下長（`lib.rs:3148-3233`）；捲動的位移是「往上、往左移」。

### 現在有的

- `direction`、`unicode-bidi`、`writing-mode`、`text-orientation`、`column-*`（`column-gap` 以外）、`break-*`、`orphans`、`widows`、`box-decoration-break` 都不在 property 的白名單裡；`@page` 不解析；`print` 的 media query 一律不符合。
- `computed_style` 有 `Direction`（只有 canvas 的文字用，`canvas_text.rs:46-48`）。
- bidi：文件的 inline layout 沒有呼叫 `set_base_direction`，parley 照第一個強方向的字推段落的方向、照 UAX #9 重排；machopo 只在連字號的位置看 `is_rtl()`。
- 沒有 multi-column、沒有分段；唯一像分段的是 inline box 被斷成好幾行（`InlineBoxFragment.first/last`）。
- taffy 只有 `crates/layout` 和 `crates/ui` 用；`layout` 裡 `taffy::` 出現約 340 次，但 fragment 以後（paint、renderer、dom_api）都看不到 taffy 的型別。

## taffy 和 parley

### taffy（fork 是上游 `main` 的 `4aca4f4933`，2026-11-06，加一個沒送上游的修改）

- 幾何是物理的：`Rect` 是 left、right、top、bottom，`Size` 是 width、height（`geometry.rs:110-130`）；`AbstractAxis::as_abs_naive` 把 inline 一律對到水平，註解寫「taffy 有了 `writing_mode` 時會改」。
- `direction: rtl`：上游 PR #906（2026-03-17 合併）加上，只管橫排；block、flex、grid、abspos、leaf 都照它鏡射，輸出照舊是物理座標；`ScrollableOverflow`（PR #1372）記 RTL 的捲動原點。fork 裡有，machopo 沒用。
- `writing-mode`：issue #752（2024-12，維護者 nicoburns 開的）還開著；PR #1265（2026-10-07）只加了 `Size`、`Rect`、`LayoutInput`、`LayoutOutput` 的 `transpose()`，讓樹那一側在直排的節點轉置後照樣跑 taffy 的演算法（taffy 本身照舊是 horizontal-tb）。Blitz 的 PR #1090（2026-10-08）照這樣在 taffy 外面做了直排（約 900 行）。2026-11-10 有非成員提案把它搬進 taffy，還沒有維護者回覆。
- 演算法假設 horizontal-tb：flex 用 `is_row()` 把 main、cross 對到 width、height；block 把 height 當 block size（margin collapsing、baseline 從上量）；grid 的 column 是水平的。
- 分段：沒有。沒有 break token、fragmentainer、可以接著排的入口；`RunMode` 只有三種；`LayoutOutput` 一個節點一個大小；上游的 issue、PR、路線圖都沒有提過分段、multi-column、列印。推測：要分段就要新的入口，或在 machopo 自己的 block layout 外面包；taffy 的 cache 沒有放「從某個 token 接著排」的位置。

### parley（fork 是上游的 `1b67b753`，2026-11-08，加 11 個 commit）

- bidi：自己的 UAX #9（`parley_engine/src/bidi.rs`，用 `icu_properties`）；段落方向用 `set_base_direction`；行用 L2 重排；對齊和 justify 照 RTL。沒有 L1（行尾的空白回到段落的 level），PR #907 還開著；行內的方向覆寫（`dir`）要嵌入的程式自己插控制字元（推測）。
- 直排：沒有。shaper 只給 harfrust `LeftToRight` 或 `RightToLeft`；上游唯一的是草稿 PR #634（2026-06，只到分析和 shaping，沒有行的 layout），2026-09 維護者問「還需要嗎」。
- 斷行一次一行（`BreakLines` 的 `break_next`、`state`、`revert_to`、`set_line_max_height`，`line_break.rs:895-1134`）：推測夠在 fragmentainer 的邊界停下、記下狀態或文字的位置接著排；orphans、widows 要用退回的那條路。
- fork 的修改裡碰到 bidi 的：shape cache 的 key 有 bidi level；整字 shaping 只給由左到右的文字（阿拉伯文等走整段的慢路）；inline box 的邊界和 tab stop 沒有 RTL 的測試。

## 三家的做法

2026-11-19 讀 Chromium `main`、Gecko（`mozilla-firefox/firefox` 的 `main`）、Servo（`b59cc398`，2026-11-18）。

### Blink LayoutNG

- 邏輯的型別：`WritingMode`（五種，`platform/text/writing_mode.h:46-54`）、`WritingDirectionMode`（writing mode 加 `TextDirection`）、`LogicalOffset`、`LogicalSize`、`LogicalRect`（`core/layout/geometry/`）、`WritingModeConverter`（`writing_mode_converter.h:18-93`，要外面的大小：有的組合從右或下量）。
- 輸入輸出：每個 `LayoutAlgorithm` 只讀 `BlockNode`、`ConstraintSpace`、break token（`layout_ng.md`），結果照這三個快取；`BlockNode::Layout(space, break_token)` 回 `LayoutResult`（有 `kNeedsEarlierBreak`、`kOutOfFragmentainerSpace`）；`ConstraintSpace` 的大小都是 `LogicalSize`，帶 writing direction、是不是正交的根。
- 換成物理的地方：builder 存 `LogicalFragmentLinkVector`（邏輯的子位移），`PhysicalBoxFragment` 的建構子用 `WritingModeConverter({wm, direction}, Size())` 一次換成物理的（`physical_box_fragment.cc:391-399`）；那時父 fragment 的大小已經定了，vertical-rl 和 RTL 不用事後修正。fragment tree「沒有邏輯的位移或大小」（`layout_ng.md`），父的演算法讀子 fragment 時用 `LogicalFragment` 的 view。
- paint、hit test 走物理的 fragment tree；捲動有物理的「scroll origin」處理 RTL 和 vertical-rl；CSSOM 從物理的資料算。
- 分段：`BreakToken` 是「layout 的接續」，`fragment2 = node->Layout(space, fragment->GetBreakToken())`；`BlockBreakToken` 有已用掉的 block size、序號、子 token（一棵 token 的樹）。`ConstraintSpace` 有 `FragmentationType`（none、page、column、region）、fragmentainer 的 block size（column balancing 的第一輪可以未定）和位移。各演算法共用 `fragmentation_utils.h`（挑斷點的「break appeal」、不能斷的內容）。multicol 是 `ColumnLayoutAlgorithm`，column 是沒有 `LayoutObject` 的 fragment；列印是 `PaginatedRootLayoutAlgorithm`（page container、page border box、page area，page area 是 fragmentainer）；context 可以巢狀。一個 box 可以有任意個 fragment（`LayoutBox` 的 `layout_results_`）。
- 歷史：「RenderingNG deep-dive: LayoutNG block fragmentation」（2023-01-03）：舊的 layout「沒有真的分段的概念」，列印是加上去的、multicol 是補上去的，把內容排成一長條、在 pre-paint 和 paint 時切開，分段之後的變形、flex 和 grid 的分段「基本上做不到」；NG 的分段從 Chrome 102（block、行、float、out-of-flow）到 108（列印）。「LayoutNG」（2021-10-08）的動機是不可變的 fragment tree、以 constraint space 當快取的 key。

### Gecko

- `WritingMode`、`LogicalPoint`、`LogicalSize`、`LogicalMargin`、`LogicalRect`（`layout/generic/WritingModes.h`）；邏輯的型別只在 debug build 帶 writing mode 檢查。
- frame 存物理的 `mRect`（`nsIFrame.h:5437`），邏輯的從它算（要 container 的大小）；`ReflowInput`、`ReflowOutput` 用邏輯的大小。
- 存物理位置的代價：`nsBlockFrame.cpp:2198-2225`：reflow 時還不知道自己的大小，先用假的 container 大小，排完再修正行、marker、float 的位置（vertical-rl 是 block 方向往負的唯一情況）。
- 分段用 continuation：放不下時回 `Incomplete`，父 frame 建一個接續的 frame（next-in-flow）；multicol 的每個 column 是內容 block 的 continuation（`nsColumnSetFrame`），平衡是二分搜尋；頁是 page frame 的 continuation。一棵會被分開的 frame tree。

### Servo

- 用 stylo 的 `WritingMode`，自己的 `LogicalVec2`、`LogicalRect`、`LogicalSides`（`components/layout/geom.rs`）；layout 用邏輯的，每個 `BoxFragment` 建立時換成物理的。
- 那時用的轉換（`as_physical_deprecated`）只看 containing block 的 inline size、不翻 block 方向，註解說「之後要拿到父 container 的大小」；正確的 `as_physical(wm, container_size)` 和它的呼叫各約一半。
- 2026-11-18：`direction: rtl` 有（行用 `unicode_bidi` 重排）；`writing-mode` 在 pref 後面、WPT 約三分之一通過，正交的 flow 還是 TODO；multicol 在 pref 後面、只建立 BFC；沒有分段（一個 box 的 fragment 是清單，但沒有接續的 token）。#22397（2018）討論分段的模型，否定 Gecko 那種一棵樹，選 Blink 那種兩棵樹，沒有做。

### 共同點和差別

- 三家都用 inline、block 寫演算法、輸出存物理座標、paint 和 hit test 用物理的；邏輯換物理都要 container 的大小。
- 差在什麼時候換：Blink 在父的大小定了才換（不用修正）；Gecko 先存物理的再修正；Servo 太早換、block 方向至今沒做完。
- 分段：Blink 是可以接續的 layout（break token）、不可變的 fragment、一個 box 好幾個 fragment，所有演算法共用；Gecko 是 continuation 改 frame tree；Servo 還沒有。
- 推測（給這份 RFC）：從一開始就用邏輯的寫演算法、子位移存邏輯的到父的大小定了、入口帶 break token 和 fragmentainer（先一律沒有）、一個 box 可以有好幾個 fragment：這幾個是之後補最貴的。

## 做法的比較

| | A：物理座標，邊界轉換 | B（建議）：自己的邏輯座標，taffy 經過一層 | C：全部自己寫 | D：在 taffy 上游做 |
| --- | --- | --- | --- | --- |
| 現在要改的 | 幾乎沒有 | inline、table、定位、fragment builder 換成邏輯的寫法（`layout` 約 500 處物理的欄位） | 整個 layout | 上游的 PR，fork 照舊 |
| 之後每個功能 | RTL、直排各寫一次分支 | 只寫一份 | 只寫一份 | 看上游 |
| RTL | taffy 已經有，machopo 的部分各自補 | taffy 已經有，machopo 的部分照邏輯座標 | 自己寫 | taffy 已經有 |
| 直排 | 轉置（Blitz） | 轉置（Blitz），上游有了換掉 | 自己寫 | 等 #752 |
| 分段 | 沒地方接 | 入口有 break token，block 的分段到時候決定 | 從頭就有 | 上游沒有人提過 |
| 和 Chromium 的對照 | 像 Gecko 的問題（事後修正） | 和 LayoutNG 同一個模型 | 最像 | — |
| 風險 | 補到後面要整個改 | 改寫期間和其他 layout 的工作衝突 | 成本最高、WPT 要重追 | 時程不在自己手上 |
