# RFC 0003：flex 和 grid layout

- 開始日期：2026-10-04
- 狀態：✅ 已完成：2026-10-07（`display: flex`、`grid` 和它們的屬性，`inline-block`、`inline-flex`、`inline-grid`，`width` / `height` 的 `min-content`、`max-content`、`fit-content`；item 和 atomic inline 整個畫完、`z-index`。taffy 的問題是已知限制。驗收見 `progress.md`）
- 細節：`details.md`

原本是 MVP 的 P5（`rfcs/pre-rfcs/MVP.md` 的「里程碑」），2026-10-04 決定 MVP 到 P4 為止，改成這份 RFC。

## 摘要

用 taffy 的 flexbox 和 grid 演算法排 `display: flex` 和 `display: grid`，接法和 block 一樣：在 box tree 上實作 taffy 的低階 trait，不用 `TaffyTree`。style 加上 flex 和 grid 的屬性、`min-content` / `max-content` / `fit-content`，item 的 blockification 在 style 做（照 Chromium 的 `StyleAdjuster`）；box tree 維持 DOM 順序，container 排版時照 `order` 排出 item 的順序（照 Chromium 的 `FlexChildIterator`）；paint 把 item 當成 inline-block 一樣整個畫完。`inline-block`、`inline-flex`、`inline-grid` 是 atomic inline：box tree 裡的 box，IFC 斷行時讓 box tree 用 taffy 排它，line 決定它的位置；inline 的 `<img>` 也走同一條路（「已決定的事項」第 1 到 3、21 條）。

## 動機

**當時的情況**（2026-10-04）：`computed_style::Display` 只有 `Block`、`Inline`、`None`。`display: flex` 和 `grid` 在 parse 時就被丟掉，所以照 block 排。taffy 只開了 `block_layout`，`layout::taffy_style` 把 `display` 寫死成 `Block`（`details.md` 的「開始時的程式」）。

**問題**：現在的網站大多靠 flex 和 grid 排版。P2 挑測試網站時就因此跳過了幾個（`rfcs/pre-rfcs/P2.md`）：example.com 用 `display: grid` 置中、danluu.com 的每個 `li` 都是 flex、lite.cnn.com 大量用 flex。這些網站在 kun 上會排成一直往下的 block，看得到內容，但版面完全不對。

目標：
- `display: flex` 和 `display: grid` 照規範排版，包括 item 的 blockification、匿名 item、`order`、auto margin、`min-width: auto`。
- `width`、`height` 接受 `min-content`、`max-content`、`fit-content`（`min-*` / `max-*` 上的不做，第 12 條）。
- flex 和 grid item 的 paint 順序和 hit test 照規範（當成 inline-block 整個畫完，`order` 改過的順序，`z-index`）。
- `inline-block`、`inline-flex`、`inline-grid`。
- 通過 `css/css-flexbox`、`css/css-grid` 的 WPT reftest 子集；照 P2 跳過的網站的結構寫的本地頁面排得對（第 10、19、24 條）。
- P4 的局部重排照樣成立：改了 flex、grid 或 atomic inline 裡的東西，結果和整份重排一樣。

不在範圍內：
- `position`（包括 flex / grid 裡的絕對定位）、float、table、`calc()`：各自是獨立的題目。
- subgrid、masonry：taffy 0.14 不支援。
- `vertical-align`：atomic inline 一律在 baseline 上。
- `testharness.js` 的 layout 測試（`check-layout-th.js`）：要的 DOM API（`offsetWidth`、`getBoundingClientRect`…）遠超過這份的範圍，見「之後的可能」。
- 寫自己的 flex / grid 演算法。

## 設計概要

給網頁的作者：`display: flex`、`grid` 和它們 inline-level 的版本排得和 Chrome 一樣（在支援的屬性範圍內，taffy 的問題除外）。`justify-content`、`align-items`、`gap`、`flex: 1`、`grid-template-columns: repeat(3, 1fr)`、`grid-template-areas` 這些常見的寫法都能用；`order` 會改變排列和畫的順序；flex 和 grid item 上的 `z-index` 會建立 stacking context。`inline-block` 的導覽列、`inline-flex` 的按鈕照內容定寬，排在文字的 baseline 上。

給之後改 `layout` 的人：
- flex 和 grid container 在 box tree 裡和 block container 一樣是一個 `LayoutBox`，只是 `taffy_style` 的 `display` 不同，`compute_box` 依它分派到 `compute_flexbox_layout` 或 `compute_grid_layout`。它的 child 一律是 block-level 的 box（item）：連續的文字包成匿名的 item，只有空白的丟掉。box tree 裡的 child 是 DOM 順序；container 排版時照 `order` 排出另一份清單，taffy 和 fragment 都照這份走，所以 fragment tree 是「`order` 改過的文件順序」，paint 和 hit test 走 fragment tree，不必各自處理。
- atomic inline 是它所在的 IFC（block 或匿名 block）的 child，IFC 的內容裡有指向它的 `InlineItem::Atomic`。IFC 斷行時透過 `AtomicInlines` 讓 box tree 排它（`InlineContent` 在排版期間從 box 拿出來）；它的 fragment 是 IFC 的 fragment 的 child，line 上的 `AtomicInline` 說它畫在哪裡。

## 詳細設計

### style

- `Display` 加上 `Flex`、`Grid`、`InlineBlock`、`InlineFlex`、`InlineGrid`。`is_flexible_or_grid`（children 是 item）包括 inline-level 的兩個，`is_atomic_inline` 是 inline-level 的三個。
- **blockification 在 style 做**（第 2 條，Chromium 的 `StyleAdjuster::AdjustStyleForDisplay`）：root 和父元素是 flex 或 grid container 的元素，computed `display` 變成 block-level（`inline` 和 `inline-block` → `block`，`inline-flex` → `flex`，`inline-grid` → `grid`，Blink 的 `EquivalentBlockDisplay`）。`getComputedStyle` 看得到，box tree 也不必知道父元素是什麼。
- 屬性和值見 `details.md` 的「加的屬性」：flex 的 `flex-*` 和 `order`、grid 的 `grid-*`（`grid` shorthand 除外，第 16 條）、共用的 `justify-*` / `align-*` / `place-*` / `gap`，`z-index`，以及 `width`、`height` 和 `flex-basis` 的 `min-content` / `max-content` / `fit-content`（`Size`、`FlexBasis` 型別，第 11 到 13 條）。
- **grid 的值**：parse 在 `style` 的 `grid` 模組。`computed_style` 的 grid 型別對長度型別是泛型的：`style` 存宣告的長度（`em` 還沒換算），套用時 `map` 成 px，不用兩套一樣的 enum（第 17 條）。
  - `auto-fill` / `auto-fit` 照規範驗證：最多一個，它和其他 track 都要是固定大小（`<fixed-size>`），否則整個宣告丟掉；`minmax()` 的最小值不能是 `fr`；line 0 和 `span 0` 不合法。`grid-row-gap`、`grid-column-gap`、`grid-gap` 是舊的別名。
  - `TrackList` 存 track 和每條線的名稱（每個 entry 前面一組、最後再一組；沒有任何名稱時是空的），`repeat()` 存它裡面的。`grid-template-areas` 在 parse 時整理成矩形（`GridTemplateAreas`：名稱和從 1 起算的線，照 Blink 的 `NamedGridAreaMap`）；不是矩形、row 的格數不同、有名稱和 `.` 以外的字元，整個宣告丟掉。
  - `<grid-line>` 的名稱存成 `Named` / `NamedSpan`；`grid-row`、`grid-column`、`grid-area` 省略的線，在它對應的線只有一個名稱時沿用那個名稱，否則是 `auto`（css-grid 2 的「grid-row」「grid-area」）。
  - `grid-template` 設 rows、columns、areas 三個 longhand：字串的寫法裡，一個 row 後面和下一個 row 前面的名稱是同一條線，沒寫大小的 row 是 `auto`，`/` 之後的欄不能有 `repeat()`（`<explicit-track-list>`；一開始照 `parse_tracks` 接受了整數的 `repeat()`，3b 時自己找到）。
- **`z-index`**（第 9 條）：style 存算出來的 `z_index` 和生效的 `effective_z_index`，只有 flex 和 grid item 的生效；生效的 item 建立 stacking context（不需要 `position`）。
- `StyleDifference`：flex 和 grid 的屬性都是 Layout，`order` 也是（item 重新排版時，祖先的 taffy cache 跟著清掉，container 重新排出順序）；`z-index` 只影響 paint；`display` 的變化是 Reattach；父元素換了 `display` 時，child 的 blockification 跟著變，style 以子樹為單位重算，所以涵蓋得到。

### box tree

- **flex 和 grid container 的內容一律是 block-level 的 child**：每段連續的 inline 內容包成匿名的 item（CSS Flexbox 的「Flex Items」、CSS Grid 的「Grid Items」）。只有空白的那段不產生 item，不管 `white-space`：原本照 block container 的規則，只丟掉「會被合併掉」的空白，`white-space: pre` 時多出一個 item（WPT 的 `whitespace-in-flexitem-001` 抓到）。
- **`order`**（第 3 條，Chromium 的 `FlexChildIterator`）：container 的 `children` 維持 DOM 順序；排版時（`compute_box` 跑到 container 時）只要有一個 child 的 `order` 不是 0，就以 `order` 穩定排序出另一份清單存在 container 上，taffy 的 `child_ids` 和建 fragment 都照這份走。taffy 0.14 沒有 `order`，照 child 的順序排；grid 的 auto-placement 也照這個順序，和規範一致。
- **atomic inline**（第 21 條）：`inline-block`、`inline-flex`、`inline-grid` 和 inline 的 `<img>` 是 box tree 裡的 `LayoutBox`（照 Blink：`LayoutBlockFlow` 和 `LayoutImage` 都是 atomic inline），也是它所在的 IFC 的 children；IFC 的內容裡是指向它的 `InlineItem::Atomic`（Blink 的 `kAtomicInline` item）。內容重新收集時，沒變的 atomic inline 的 box 照樣保留（像 block 的 child），可能換到新的匿名 block 底下：`release` 只放掉還是自己 child 的 box。圖片載入時重建的是圖片自己的 box，它那一行重新收集。

### layout

- **taffy 的接法**：開 `flexbox` 和 `grid` feature，在 `BoxTree` 上實作 `LayoutFlexboxContainer`、`LayoutGridContainer`；`compute_box` 依 `taffy_style.display` 分派。`taffy_style` 依 `Display` 設 `display`（inline-level 的也是 `Flex` / `Grid` / `Block`），對應新的屬性。
- **grid 的對應**：track 轉成 taffy 的 `minmax`：一個值同時是最小和最大，但 `fr` 的最小值是 `auto`（css-grid 2 的 `grid-template-columns`）；`fit-content()` 是 `auto` 到它的上限。名稱和 areas 原樣交給 taffy（`grid_template_column_names`、`grid_template_areas`），area 隱含的 `-start` / `-end` 線和照名稱擺放由 taffy 處理。taffy 只在走外層的名稱清單時才讀 `repeat()` 裡的名稱，所以只有 `repeat()` 裡有名稱時，外層每條線給一組空的（WPT 的 `grid-placement-using-named-grid-lines-004` 抓到）。
- **替換元素當 item**：由 leaf 的 measure 定大小：style 照原本的 size（`auto` 和百分比照樣留著），measure 回答自然大小，知道一軸時另一軸照自然比例；不用 taffy 的 `aspect_ratio`。block layout 裡的（`display: block` 的圖片）照舊：`replaced_taffy_style` 先把 size 算成固定的 px（taffy 的 block layout 會把 `auto` 的寬度撐滿）。用哪一種由父 box 決定，在 `set_content` 和 restyle 時設定。
  - 原本 flex item 也用 block 的那種，measure 回答 0：item 縮成 0，stretch 的高度也不會照比例改寬度（WPT 有 9 個 `<img>` 的 flex 測試過不了）。
  - 試過 block 和 flex 都交給 taffy（0.14 的 block layout 對 `item_is_replaced` 的 box 不撐滿寬度、加上 `aspect_ratio`），WPT 照樣全過，但 taffy 的 leaf 把 `aspect_ratio` 套在 border box 上（`height = max(height, border box 的寬 / ratio)`），有 padding 或 border 的圖片變高。flex item 不用 `aspect_ratio` 就沒有這個問題；block 的那種原本在高度是 `auto` 時都設 `aspect_ratio`，同樣的問題（`<img style="display: block; width: 40px; padding: 5px">` 高 100px，應該是 90px），改成只在寬度是百分比時設。
  - **grid item 的圖片不拉伸**：`normal` 對有自然大小的替換元素是 `start`（css-grid 2 的「Grid Item Sizing」），Chrome 也是；taffy 只在 item 有確定大小時才不拉伸，而 item 的圖片大小交給 measure，所以 `replaced_taffy_style` 拿到 container 的 style，在 item 和 container 的 `justify-*` / `align-*` 都是預設的時候，把 `justify_self`、`align_self` 設成 `start`。flex item 照舊拉伸（flex 的 `normal` 是 `stretch`）。原本照 flex 的做法，grid 裡的圖片被拉成整格寬（`update` 測試抓到）。
  - **只在一軸拉伸的圖片**：taffy 的 leaf 只在量的時候（`ComputeSize`）把已知的大小傳給 measure，排版時（`PerformLayout`）傳空的；grid 排版只在一軸拉伸的 item 時只給那一軸，measure 就照自然大小回答另一軸。改成 measure 從 `LayoutInput` 拿已知的大小（WPT 的 `replaced-alignment-with-aspect-ratio-006` 到 `-009`）。
- **measure 量的是 content box**：taffy 的 leaf 量的時候給 measure 的已知大小是 border box 的，可用空間卻是 content box 的；IFC 原本拿 border box 的寬度斷行，有 padding 或 border 的 flex item 量出來的行數太少。IFC 和圖片的 measure 都先扣掉 padding 和 border。
- **intrinsic size**：`width`、`height` 的 `min-content` / `max-content` / `fit-content` 和 `flex-basis: content` 直接交給 taffy 的 `Dimension`；taffy 在 block 方向（`height`）、或它不處理的地方當成 `auto`。圖片把關鍵字當成自然大小，和 `auto` 一樣。
  - `content_widths` 記住結果，flex 和關鍵字會量很多次；只有 atomic inline 變了會清掉（原地換 style 只會是只影響 paint 的變化）。
  - **IFC 的 measure 在只給可用寬度時回答 fit-content 寬度**：`min(max-content, max(min-content, 可用寬度))`，不是照那個寬度斷行後最長的一行。原本回答最長的一行，`width: fit-content` 裡會換行的文字因此比可用寬度窄。
  - **百分比在 intrinsic size 裡是 cyclic**（css-sizing 3）：min-content 時圖片的百分比寬度當成 0（可壓縮的替換元素），max-content 時當成 `auto`（自然大小）。原本兩者都以 0 為基準，`<img width=50%>` 的 IFC 被夾成 0 寬（WPT 的 `inline-replaced-width-006` 抓到）；中間試過「至少和最長的一行一樣寬」，mutation test 證明沒有測試靠它、也不合規範，改成照規範處理百分比。
  - **斷行的容許誤差**：shrink-to-fit 和 `fit-content` 的寬度剛好是內容的寬度，經過 border box 和 padding 的加減之後有浮點誤差，Lato 這種寬度不是整數的字型會多斷一行，box 卻只有一行高（`about:demo` 的導覽列看到；測試用的 Ahem 寬度是整數，看不出來）。斷行時容許超出 1/64 px（Blink 的 layout unit），對齊照原本的寬度（parley 的 `set_prior_line_width`；一開始直接加在斷行寬度上，置中和靠右偏了 1/128 px，`inline` 測試抓到）。
- **atomic inline 的排版**：IFC 斷行前透過 `AtomicInlines` 讓 box tree 排它，parley 的 inline box 是它的 margin box。
  - 圖片照 `Replaced::content_size`。其他的用 taffy 排：先只量寬度，再照那個寬度排，和 block-level 的 box 一樣，對它的寬度解析的東西（`inline-grid` 的百分比 `gap`，WPT 的 `grid-gutters-015`）才有東西可以對。taffy 不給寬度時，block、flex 和 grid 都照內容算寬度，夾在可用寬度裡，就是 shrink-to-fit（CSS 2 § 10.3.9）。
  - **margin**：taffy 的 leaf 和 flex layout 自己會從可用寬度扣掉 margin，它的 block 和 grid layout 不會，所以只對後兩者先扣。原本一律先扣，只有文字的 `inline-block`（leaf）shrink-to-fit 的寬度太窄。
  - **百分比高度**：IFC 的內容高度確定時（taffy 給的，或 style 的高度夾在 min / max 之間），是 atomic inline 的百分比高度的基準（CSS 2 § 10.5；WPT 的 `max-height-applies-to-017`）；高度變了就重新排它們。
  - **baseline**（第 22 條）：`inline-flex`、`inline-grid` 是 taffy 給的第一個；`inline-block` 是 `last_baseline` 從它裡面最後一行、或最後一個有 baseline 的 child 往上算，`overflow` 不是 `visible` 的 child 用它的下 margin 邊（Blink 替捲動容器合成的 baseline；WPT 的 `baseline-block-with-overflow-001`）；它自己 `overflow` 不是 `visible`、或沒有行時是下 margin 邊；圖片是下 margin 邊。
  - **量 min-content / max-content 寬度時只量**（`ComputeSize`）：taffy 一個 box 只記一個排版結果（量的有九個），量的時候也排會把最後寬度的那個擠掉，atomic inline 每次都重排、fragment 也保不住（`update` 測試抓到）。只量寬度之後、或在別的寬度排過之後，IFC 照最後的寬度斷行時都會重新排它（`sized_for` 記著上次排的空間），所以建 fragment 時它的排版就是最後寬度的；line 決定它的位置。
  - **局部更新**：atomic inline 裡的東西改了，`mark_needs_layout` 往上走到 IFC 時讓 IFC 重新量它，只重新斷行、不重新 shape。
- **item 的 baseline**：`align-items: baseline` 要 item 回報第一條 baseline：IFC 的 leaf 從 parley 的第一行算出來，放進 `LayoutOutput` 的 `baselines.first`；block 由 taffy 從第一個 child 傳上來。
- **P4 的局部重排**：flex 和 grid 會單獨量 item（`ComputeSize`），`compute_box` 只在 `PerformLayout` 時標 `needs_fragment`；只被量過、排版命中快取的 box 照樣沿用 fragment（`update` 測試的 `a_box_only_measured_keeps_its_fragment`）。

### paint 和 hit test

- **flex 和 grid item 整個畫完**（第 7、8 條）：照 inline-block 的規則畫（CSS Flexbox 的「Flex Item Z-Ordering」，grid 的規則相同）：在父 stacking context 的文字階段、照 `order` 改過的樹的順序，畫到 item 時先畫它整個子樹的背景、再畫它的文字，背景階段跳過 item。item 裡真正的 stacking context 不跟著 item 畫，歸到父 stacking context 的清單。對應 Blink 的 `BoxFragmentPainter::PaintAllPhasesAtomically`。哪些 box 整個畫完看父 fragment（`paints_children_whole`），不在 fragment 上加標記。
- **atomic inline 整個畫在 line 上它的位置**（CSS 2 附錄 E 的 7.2.1.4）：IFC root 的 children 是它的 atomic inline，背景階段和平常走 children 的地方跳過它們（`children_on_lines`）；line 上的 `AtomicInline` 記它在第幾段文字之前、在第幾個 inline box 之後（`after_boxes`），paint 和 hit test 都用 `line_parts` 排出 line 上的順序。原本圖片和 inline box 只比「在哪段文字之前」，同一段文字前、在圖片之後才開始的 inline 元素被畫在圖片之前（`artifact` 測試抓到）。
- 一個 stacking context 裡照 CSS 2 附錄 E：自己的背景、負的 `z-index` 的子 stacking context、其他背景、文字（item 和 atomic inline 整個畫完）、`0` 和 `auto` 的照樹的順序、正的由小到大（`stacking_children` 以 `z-index` 穩定排序）。原本子 stacking context 一律照樹的順序畫在最後。
- hit test 是 paint 的反向，兩邊一起改；主執行緒捲動區域的 `covered_scrollers` 走同一個 walker，背景階段要跳過 item 和 atomic inline，不然它們多出一個比較早的位置，排在它和它之後之間的東西會被當成蓋住它（`hit_test` 測試的 `a_flex_item_container_is_over_the_items_before_it`、`an_inline_block_container_is_over_the_inline_box_around_it`）。
- P4.3d 的 display item 快取不用改：整個畫完時，每個 box 照樣各有背景和文字兩段（`details.md` 的「實作時注意到的地方」）。有 atomic inline 的 IFC 每次都重畫它的 line（不拿上次的 item）：atomic inline 夾在文字中間、各有自己的 property，它們的 item 由自己去拿上次的。

### 測試

- **WPT**：`css/css-flexbox`、`css/css-grid`、`css/css-sizing` 的 reftest 候選全部匯入，加上已經在用的目錄裡因為新的屬性和值而成為候選的（第 5、23 條）；過不了的記進 `4ff8d6e:tests/wpt/reftest-expectations.txt` 附上原因。每一階段放寬的候選腳本和數字在 `tests/wpt/README.md`。
- **自己的 reftest**：`tests/reftests/flex/`、`grid/`、`block/b7`、`inline/i7`、`i8`：blockification、匿名 item、`order`、item 的 paint 順序和 `z-index`、intrinsic size、track 和擺放、areas 版面、atomic inline 和它的 baseline。
- **`layout` 的測試**：`flex.rs`、`grid.rs`、`inline_block.rs`；`update` 守住局部重排和整份重排的 fragment 一樣（延續 P4）。
- **style**：屬性的 parse、shorthand、CSS-wide keyword、blockification 和 `z-index` 的生效。
- **paint**：`artifact`、`hit_test`、`paint_controller`：畫的順序、hit test 的順序、快取。
- **情境**：`about:demo` 的 flex 列、疊在一起的卡片、fit-content 標籤、grid tiles、areas 版面、導覽列、inline-flex 標籤，各一份情境（`tests/scenarios/`）。
- **真實網站**：不存進 `tests/sites/`，用照它們的結構寫的本地頁面（第 10、19、24 條）。
- **mutation test**：照 `CLAUDE.md`。flex 和 grid 的演算法本身在 taffy 裡，kun 測的是接法（分派、style 的對應、item 的產生和順序、paint 順序、atomic inline 的大小和位置）。

### 分階段

1. **flex**（2026-10-06，第 4 條）：
   - **1a. layout**：`Display::Flex`、blockification、匿名 item、`order`、flex 的屬性和 `taffy_style` 的對應、`LayoutFlexboxContainer`、IFC 的 baseline（`align-items: baseline`）；`css/css-flexbox` 的 reftest 候選全部匯入。
   - **1b. paint**：item 整個畫完、`z-index` 和附錄 E 的 stacking context 順序、hit test；照 danluu.com 結構縮減的頁面（第 10 條）。
2. **intrinsic size**（2026-10-06，第 11 到 13 條）：`width`、`height` 的 `min-content` / `max-content` / `fit-content`、`flex-basis: content`、`content_widths` 的快取、只被量過的 box 的測試。
3. **grid**（2026-10-07，第 15 到 19 條）：
   - **3a**：`Display::Grid`、track list（不含具名的線）、照數字和 `span` 擺放、`grid-auto-*`、`justify-items` / `justify-self`、`place-*`、`LayoutGridContainer`；grid item 的 paint、`z-index`、`order`；`css/css-grid` 的 reftest 候選。
   - **3b**：具名的線、`grid-template-areas`、`grid-template` shorthand；常見的 areas 版面（第 19 條）。
4. **atomic inline**（2026-10-07，第 20 到 23 條）：
   - **4a**：`inline-block` 和共用的機制（box tree、在 IFC 裡排版、baseline、paint、hit test、局部更新），inline 的 `<img>` 一起改走這條路；導覽列。
   - **4b**：`inline-flex`、`inline-grid`；WPT 的候選。

原本打算先做原型（只開 `flexbox` feature、對應 `flex-direction` 和 `justify-content`），2026-10-06 決定不另外做：1a 的前半就是原型要驗證的接法。

## 缺點

- 三種 layout 都靠 taffy：它的 bug 和沒實作的東西（subgrid、`calc()`、`min-*` / `max-*` 的關鍵字）直接變成 kun 的限制；kun 不 fork taffy（第 14 條），只能等上游修好再升級。
- 屬性的數量大約加倍，`style` 的 parse 和 `StyleDifference` 的分類都跟著長；grid 的 track list 和 `grid-template-areas` 的 parse 是整個 style 系統裡最複雜的值。
- paint 多了「整個畫完的 box」兩條路（item 和 atomic inline），hit test 和 `covered_scrollers` 要跟著維持同樣的順序。
- atomic inline 在它那一行每次重新排版時都要再量一次（taffy 的快取讓沒變的很便宜），IFC 的排版不再只是 parley。

## 理由和其他做法

layout 演算法用 taffy 是 MVP 的 crate 選型時就決定的（`docs/architecture.md` 的「Crate 選型」），這份 RFC 不重新比較，只比較接法上的選擇。完整的比較見 `details.md`。

- **display 的範圍**：
  - **D1. 先做 block-level 的 `flex`、`grid`**：box tree 和 IFC 不用改，可以先驗證 taffy 的接法。
  - **D2. 一次連 `inline-flex`、`inline-grid`、`inline-block` 一起做**：要先有 atomic inline（IFC 裡的 inline box 由 taffy 排版），牽動 parley 的 inline box、斷行和 baseline。
  - 2026-10-04 的看法：D1，atomic inline 當成第 4 步。它和 flex / grid 是兩種風險，P4 時把 flex / grid 拆出來也是同樣的理由。
- **blockification 在哪裡做**：
  - **B1. style（Chromium 的 `StyleAdjuster`）**：computed value 就是規範說的值，box tree 只看自己的 style。代價是父元素換 `display` 時要重算 child 的 style。
  - **B2. box tree**：style 不變，box tree 建 child 時看父元素是不是 flex / grid。`getComputedStyle` 會回傳錯的值，box tree 的局部更新也要多看一層。
  - 2026-10-04 的看法：B1。
- **`order` 在哪裡處理**：
  - **O1. box tree 的 child 順序**：建 box tree 時排一次，`children` 直接存 `order` 改過的順序。
  - **O2. 排版時排序**（Chromium 的 `FlexChildIterator`）：box tree 保持 DOM 順序，container 每次排版時排出一份清單。
  - **O3. fork taffy、加上 `order`**：只解決 layout，paint 還是要自己排。
  - 選 O2（第 3 條）。2026-10-04 原本傾向 O1，理由是「O2 的話每個走 child 的地方都要記得用 iterator」；2026-10-06 重新看過，這在 kun 不成立：paint 和 hit test 走的是 fragment tree，fragment 照排版的順序建，所以只有 taffy 的 `child_ids` 和建 fragment 兩處用排過的清單。O2 還有兩個好處：`order` 變了只是重新排版（O1 要重建 container 的內容，box tree 的局部更新插入 child 時也要重新排序）；box tree 維持 DOM 順序，之後照 DOM 順序走 layout tree 的功能（選取、`innerText`、無障礙）不用改。
- **`z-index`**：只做 flex / grid item 上的（加上依 `z-index` 排序 stacking context），`position` 的部分等 `position` 一起做。不做的話，flex item 疊在一起時的順序會錯，WPT 也有一批測試靠它。

## 先例

- **Chromium（LayoutNG）**：`FlexLayoutAlgorithm`、`GridLayoutAlgorithm`，container 是 `LayoutFlexibleBox`、`LayoutGrid`；blockification 在 `StyleAdjuster`；`order` 用 `FlexChildIterator`（每次排版時以 `order` 穩定排序 child，`std::stable_sort`）；flex 和 grid item 由 `BoxFragmentPainter` 整個畫完。kun 照它的 style、`order` 和 paint 的做法。
- **Servo**：grid 用 taffy，flex 是自己寫的；接法是把自己的 box tree 轉給 taffy 的低階 trait，和 kun 相同。
- **Blitz（Dioxus）**：block、flex、grid 都用 taffy，也是低階 trait。

逐項比較見 `details.md` 的「其他實作」。

## 已決定的事項

1. **先做 block-level 的 `flex`、`grid`（D1）**（2026-10-06）：`inline-flex`、`inline-grid`、`inline-block` 要先有 atomic inline，排在第 4 步。
2. **blockification 在 style 做（B1）**（2026-10-06）：照 Chromium 的 `StyleAdjuster::AdjustStyleForDisplay`。不選 box tree（B2）：`getComputedStyle` 會回傳錯的值。`recalc_style` 重算一個元素時整棵子樹都重算，父元素換 `display` 時 child 跟著重算。
3. **`order` 在排版時排序（O2，照 Chromium 的 `FlexChildIterator`）**（2026-10-06）：box tree 維持 DOM 順序。不選在 box tree 排一次（O1）、不選 fork taffy（O3），理由見「理由和其他做法」。
4. **第 1 階段分成 1a（layout）和 1b（paint）**（2026-10-06）：和 RFC 0011 的 3a、3b 一樣分開驗收。`align-items: baseline` 放進 1a（原本排在第 2 階段）：導覽列、按鈕列常用，IFC 的第一條 baseline 從 parley 的第一行就算得出來。
5. **`css/css-flexbox` 的 reftest 候選全部匯入**（2026-10-06）：候選腳本加上 1a 的屬性之後有 154 個（2026-10-06 數的），過不了的記進 `4ff8d6e:tests/wpt/reftest-expectations.txt`。不挑子集：taffy 的行為要靠它們守住。
6. **不另外做原型**（2026-10-06）：1a 的前半就是原型要驗證的接法。
7. **flex item 照 Chromium 整個畫完**（2026-10-06，1b）：在父 stacking context 的文字階段、照 `order` 改過的樹的順序，畫到 item 時先畫它整個子樹的背景、再畫文字（Blink 的 `BoxFragmentPainter::PaintAllPhasesAtomically`）；item 裡真正的 stacking context 照樣歸到父 stacking context 的清單（規範的「as if it created a new stacking context, but…」）。不選把 item 當成偽 stacking context、畫在所有文字之後：非 item 的內容和 item 重疊時順序和 Chrome 不同。
8. **paint 和 hit test 看父 fragment 判斷 item**（2026-10-06，1b）：flex container 的 fragment 的每個 child（包括匿名 item）都整個畫完，不在 `BoxFragment` 加標記（Chromium 有 `IsPaintedAtomically`）。走樹時本來就拿著父 fragment；第 3 階段改成「父是 flex 或 grid」。
9. **`z-index` 是否生效在 style 決定**（2026-10-06，1b）：照 Blink 的 `StyleAdjuster` 和 `EffectiveZIndex`，`ComputedStyle` 存算出來的 `z_index` 和生效的 `effective_z_index`；不是 flex item 的，生效的值是 `auto`。`is_stacking_context` 看生效的值。不選在 paint 看父元素：每個用到 `is_stacking_context` 的地方都要多傳父元素。`z-index` 的變化只影響 paint。
10. **danluu.com 用本地縮減的頁面驗**（2026-10-06，1b）：照它的結構（每個 `li` 是一列 flex）寫測試頁，不下載網站。
11. **第 2 階段排程，`width`、`height` 用新的 `Size` 型別**（2026-10-06）：`Size` 是 `auto`、長度或百分比、`min-content`、`max-content`、`fit-content`；`flex-basis` 用 `FlexBasis`（`Size` 或 `content`）。和 Blink 的 `Length` 一樣把關鍵字當成值的一種，但依屬性分型別：`margin` 照舊用 `LengthPercentageOrAuto`，型別本身就擋掉關鍵字。不選擴充 `LengthPercentageOrAuto`：`margin` 共用它，要在 parse 時另外擋。原本打算 `min-*` 也用 `Size`、`max-*` 用 `MaxSize`，因為第 12 條這一階段不支援，先維持原本的型別，支援時再換。
12. **`min-*` / `max-*` 先不支援關鍵字**（2026-10-06）：taffy 0.14 的 `min_size`、`max_size` 是 `LengthPercentageAuto`，放不進關鍵字；寫了關鍵字的宣告整個丟掉，記在 `docs/limitations.md`；不 fork taffy（第 14 條），所以等上游支援。不選 fork taffy：改動大，block、flex、leaf 的演算法都要跟著改；不選 parse 了當成 `auto` / `none`：行為和 Chrome 不同，又比丟掉更難察覺。
13. **這一階段的值：`min-content`、`max-content`、`fit-content` 和 `flex-basis: content`**（2026-10-06）：Chrome 都支援，taffy 也有。不做 `stretch`（和 `-webkit-fill-available`）和 `fit-content(<長度>)`，之後再說。
14. **不 fork taffy，它的問題當成已知限制**（2026-10-07）：照 crates.io 的版本用，碰到的不合規範的地方（`details.md` 的「taffy 的問題」、`min-*` / `max-*` 的關鍵字）記在 `docs/limitations.md` 和 WPT 的 expectation，等上游修好再升級。不 fork：要維護 fork 的成本（照 `CLAUDE.md` 的 fork 規則，每次升級都要 rebase），換來的只是幾個邊角的行為。
15. **第 3 階段排程，分成 3a、3b**（2026-10-07）：3a 是 `display: grid`、track list（不含具名的線）、照數字擺放、`grid-auto-*`、`justify-items` / `justify-self` 和 `place-*`、`grid-gap` 這些舊的別名，以及 grid item 的 paint、`z-index`、`order`；3b 是具名的線、`grid-template-areas`、`grid-template` shorthand 和照 example.com 結構縮減的頁面（照第 10 條）。和 1a、1b 一樣分開驗收。
16. **grid 的語法除了 `grid` shorthand 都做**（2026-10-07）：`grid` shorthand 的語法最複雜（`auto-flow` 的寫法），實際網站很少用，之後再說。subgrid 和 masonry 不做：taffy 0.14 沒有。
17. **grid 的值在 `ComputedStyle` 用自己的型別**（2026-10-07）：照 Blink 的 `ComputedGridTrackList`、`GridPosition`、`NamedGridAreaMap`，名稱用 `Arc<str>`，areas 存整理好的矩形；`layout` 的 `taffy_style()` 再轉成 taffy 的型別，和 flex、`Size` 一樣。不選直接存 taffy 的型別：taffy 會從 `layout` 擴散到 `computed_style` 和依賴它的 `dom`、`style`、`paint`、`renderer`；`NoTrace` 的論證要涵蓋 taffy 的型別（0.14 的 `CompactLength` 可以裝 `calc()` 的裸指標），每次升級 taffy 都要重看；之後 `getComputedStyle` 也要 CSS 形狀的值。代價是多一組型別和轉換。
18. **grid item 沿用 flex item 的機制**（2026-10-07）：blockification、`order`、整個畫完、`z-index` 生效的條件都改成「父是 flex 或 grid」（第 2、3、7 到 9 條）；當 grid item 的圖片也用 flex item 的那種大小（`replaced_taffy_style`）。
19. **3b 用常見的 areas 版面驗收，不用 example.com**（2026-10-07）：example.com 已經不用 grid（2026-10-07 看的時候是 `body { max-width: 26em; margin: auto; padding: 25vh 2em 2em }`），驗收它測不到 grid。改寫一份本地頁面：header、側欄、主要內容、footer，用 `grid-template-areas` 和具名的線排，是網站用 grid 最常見的寫法。不選照 example.com 現在的結構寫（不測 grid）、不選拿掉真實網站的驗收。

20. **第 4 階段排程，分成 4a、4b**（2026-10-07）：4a 是 `inline-block` 和共用的機制（box tree、在 IFC 裡排版、baseline、paint、hit test、局部更新），4b 是 `inline-flex`、`inline-grid` 和 WPT 的匯入：機制先在最單純的 container 上驗過，4b 只換 container 的種類。不選一次做完（出問題時分不出是機制還是 flex / grid）；不選像 1a、1b 那樣分 layout 和 paint（line 只畫文字、inline box 和圖片，4a 做完時 `inline-block` 的內容畫不出來，不能用 reftest 驗）。
21. **inline 的 `<img>` 一起改成 atomic inline 的 box**（2026-10-07）：照 Chromium（`LayoutReplaced` 和 `inline-block` 一樣是 atomic inline，在 inline 排版裡由 `BlockNode::LayoutAtomicInline` 排），inline 的圖片也是 box tree 裡的 `LayoutBox`，line 上是它的 `BoxFragment`，拿掉 `ReplacedFragment`，paint 和 hit test 只有一條路。不選保留 `ReplacedFragment`、只為 `inline-block` 加一種：範圍小，但 paint 和 hit test 要維持兩條一樣的路。代價是圖片載入後的重建、inline 圖片的局部更新和它們的測試要一起改。
22. **atomic inline 的 baseline 照 CSS 2 和 Chromium**（2026-10-07）：`inline-block` 是裡面最後一個 line box 的 baseline；沒有 line box，或 `overflow` 不是 `visible` 時，是下 margin 邊（CSS 2 § 10.8.1）；`inline-flex`、`inline-grid` 是第一個 baseline（css-align 3）；替換元素是下 margin 邊。這是 Chrome 的行為（`baseline-source: auto`）。不選一律用第一個 baseline：taffy 只給第一個，比較好做，但多行的 `inline-block` 會對錯位置。
23. **第 4 階段的 WPT 候選全部匯入**（2026-10-07）：候選腳本不再排除 `inline-block`、`inline-flex`、`inline-grid` 之後，在已經用的目錄（加上 `css/CSS2/linebox`、`visudet` 和 `css/css-display`）多了 94 個：CSS2 41 個、`css-flexbox` 25 個、`css-grid` 17 個、其他 11 個。照 1a、3a 的做法全部匯入，過不了的記進 expectations 附上原因。不選 CSS2 照 P1 每個系列挑一兩個：CSS2 的這些大多就是 `inline-block` 本身的測試。（4b 時其中兩個測的是 kun 沒有的 `<fieldset>` 和 `white-space: break-spaces`，照以前的做法改由候選腳本排除，實際匯入 92 個，見 `tests/wpt/README.md`。）

24. **真實網站不存進 `tests/sites/`**（2026-10-07）：那裡只放公眾領域或授權允許的頁面（`tests/sites/README.md` 的「Licenses」），danluu.com 和 lite.cnn.com 的內容有著作權；example.com 已經不用 grid（第 19 條），存了也測不到 flex 和 grid。驗收用照它們的結構寫的本地頁面（`f7-dated-list`、`g2-areas-layout`、`i7`、`i8`）。不選存 example.com：只測得到置中的 block。原本「待決定」裡的 css-grid 的 WPT 子集，3a 時已經照第 5 條的做法全部匯入。
25. **不寫回報 taffy 的草稿**（2026-10-07）：taffy 的問題留在 `details.md` 的「taffy 的問題」、`docs/limitations.md` 和 WPT 的 expectation，之後要回報時再從那裡整理。

## 待決定

沒有。

## 之後的可能

- `position`（包括 flex / grid 裡的絕對定位）和完整的 `z-index`：stacking context 變多時，順便做 `rfcs/pre-rfcs/P4.md` 第 73 條說的 subsequence 快取和 `PaintLayer`。
- 能跑 `check-layout-th.js` 的 WPT：需要 `offsetWidth` / `offsetTop`、`getBoundingClientRect` 和 testharness，能大幅增加 flex 和 grid 可以用的測試。
- relayout boundary：現在祖先一律重跑整個 taffy 演算法（`docs/limitations.md` 的「Invalidation」），flex 和 grid 的頁面上這個成本更明顯。
- `vertical-align`、`baseline-source`，`min-*` / `max-*` 的 intrinsic size 關鍵字（等 taffy）、`stretch` 和 `fit-content(<長度>)`，`grid` shorthand。
- 把 taffy 的問題整理成回報上游的草稿（第 25 條）。
