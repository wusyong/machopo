# RFC 0043：float、`vertical-align` 和文字的 property

- 開始日期：2026-10-13
- 狀態：✅ 已完成：2026-10-14（第 1 到 23 條；驗收結果在 `progress.md`）
- 細節：`details.md`；進度：`progress.md`

## 摘要

RFC 0012 路線圖的 S5（CSS 線）：`float` 和 `clear`、`vertical-align`、`text-overflow`、`text-transform`、`letter-spacing`、`word-spacing`、`text-indent`，加上 `visibility` 剩下的部分（hit test）；連同 Speedometer 用到、parley 已經支援的 `word-break`、`overflow-wrap` 和 `white-space` 的兩個 longhand（`white-space-collapse`、`text-wrap-mode`）。block-level 的 float 用 taffy 的 `float_layout`（第 4 段之前 kun 沒開；做之前先升級到 0.15，RFC 0044），行內內容繞過 float 和 inline-level 的 float 由 kun 在 parley 的逐行斷行裡處理；`vertical-align`、spacing、indent、`word-break` 直接對到 parley 已有的功能；`text-transform` 在 kun 處理空白的那一步做、連同 offset mapping；`text-overflow: ellipsis` 照 Blink 在 layout 截行。分四段，每段做完可以先 merge。

## 動機

盤點（`rfcs/0012-speedometer/progress.md` 的「盤點：kun 丟掉的 CSS（2026-10-13，Fedora 44）」，2026-10-13，20 個 default 的 suite 丟掉的宣告）：`vertical-align` 39 條（`middle` 11、`top`、`bottom` 各 7，Spectrum 和 TodoMVC 的 footer）、`float` 22 條（`left`、`right` 各 11：每個 TodoMVC 的 `.todo-count`、`.filters`，Spectrum）、`text-wrap-mode` 23、`white-space-collapse` 20、`letter-spacing` 18（都是 `var()`，RFC 0041 之後解析得了）、`text-transform` 16（`uppercase` 8）、`word-break` 12、`overflow-wrap` 11、`text-overflow: ellipsis` 10、`text-indent` 6。這些都是「量得對」的一部分：footer 的 float 沒排、`nowrap` 沒生效時，layout 的工作量和畫面都和 Chrome 不同。

要解決的 `docs/limitations.md` 的限制（「Layout」）：

- 「只有 block、inline、flex 和 grid layout……沒有 float」（`position: sticky`、table 不在這份）。
- 「沒有 `tab-size`、`text-indent`、`letter-spacing`、`word-spacing`」（`tab-size` 不在這份）。
- 「Inline box：只有 `vertical-align: baseline`」。
- 「Hit testing……沒有 `pointer-events` 和 `visibility`」的 `visibility` 那一半。

**目標**：這些 property 解析、cascade、layout 和 paint 都照 CSS 和 Chrome；WPT 的對應目錄能跑、預期結果記下來；Speedometer 的頁面不再丟掉它們。

**不在範圍內**：`position: sticky`、table（S7）、`::before` / `::after`（S3）、`tab-size`、`text-transform: full-width` / `full-size-kana`、`text-overflow` 的 `<string>` 值（Chrome 也沒有）、`vertical-align` 對 table cell、bidi（kun 只有 ltr）、`shape-outside`、`float: inline-start` / `inline-end`、`clear: inline-*`、`text-wrap-style`（`balance` 等）、`text-indent` 以外的 `hanging-punctuation`。

## 設計概要

網頁的作者看到的：

- `float: left` 的元素靠左，後面的文字和行內內容繞著它排，`clear` 的元素排到它下面；TodoMVC 的 footer 左右兩邊各一塊。
- `vertical-align: middle` 的圖示和文字對齊中線，`top`、`bottom` 貼齊行框，`sub`、`super` 和長度、百分比都有。
- `text-transform: uppercase` 的標題變大寫（`capitalize` 每個字的第一個字母），`letter-spacing`、`word-spacing`、`text-indent` 生效；`white-space: nowrap`（連同 `text-wrap-mode: nowrap`）加 `overflow: hidden; text-overflow: ellipsis` 的標題尾端是「…」；`word-break: break-all` 的長字在任何字元之間斷。
- `visibility: hidden` 的元素點不到（點到它下面的東西）。

改 kun 的人看到的：`css` 多這幾個 property；`layout` 開 taffy 的 `float_layout`，`BoxTree` 的 inline formatting context 在逐行斷行時向 taffy 的 `BlockContext` 問每一行可用的範圍；`text.rs` 的空白處理多一步大小寫轉換和截行；paint 多畫 ellipsis、hit test 看 `visibility`。

## 詳細設計

### float 和 clear

- **`float`、`clear` 的 style**：`float: none | left | right`、`clear: none | left | right | both`（`inline-start` 等不做）。float 的元素 blockify（CSS 2 § 9.7，`display: inline` 的 float 也是 block），`position: absolute | fixed` 時 `float` 算成 `none`；根元素和 flex、grid item 的 `float` 沒有作用（taffy 只有 block layout 讀它）。`float` 改了要重建 box（它在行內內容裡還是 block 之間會變）。CSSOM 的 `cssFloat`。
- **block-level 的 float**（第 20 條：前面沒有行內內容的 float）：開 taffy 的 `float_layout` feature，`taffy::Style` 的 `float`、`clear` 從 computed style 設；taffy 的 block layout 照 CSS 2 的九條規則放它（`FloatContext::place_floated_box`），`clear` 的元素照 `cleared_threshold` 往下推，margin collapsing 和 float 的互動也在 taffy 裡；自己是 BFC 的盒子（`overflow` 不是 `visible`、flex、grid）避開 float（`find_bfc_slot`）。float 的 box 是 block formatting context。
- **快取**（第 18 條）：所在的位置有還沒結束的 float，或子樹會把 float 放進同一個 BFC 的盒子，不用 taffy 的快取；斷行的快取以寬度加上 float 的區段當 key。
- **行內內容繞過 float**（第 19 條）：IFC 在 `compute_box` 收到 taffy 的 `BlockContext`；`has_active_floats` 時從內容的頂端用 `find_content_slot`（`after`）一段段查出 float 的區段（`FloatBands`；最後一段從 `cleared_threshold(Both)`，也就是最低的 float 的底，開始恢復全寬），斷行改成逐行（`break_beside_floats`）：每一行設 parley 的 `set_line_x`、`set_line_y`、`set_line_max_advance`（取它高度範圍內的區段的交集，夾在 content box 裡：slot 是照盒子的 border box 算的，夾住之後和先套 content box 的 inset 一樣，所以查區段前不用 `apply_content_box_inset`；放自己的 float 時才套，float 的 containing block 是 content box）再 `break_next`；行比所在的區段高就退回這一行的開頭（`revert_to` 這一行開始時存下的狀態）再用更窄的寬度重排。第一個不能斷的內容在 float 旁邊放不下，要移到下一段：這要斷了才知道，parley 交回的 `advance` 含行尾掛出去的空白，所以 fork 在 `LineBreakData` 多給 `hanging_advance`（第 23 條），每一行斷完扣掉它比較，放不下、下面還有更寬的段就退回、從那一段重排這一行。4b 時沒改 fork，是整段斷完看每一行的 `metrics`、再整段重斷；4c 的 float 放進 BFC 就收不回，改成逐行。不換行的長行也一樣移下去（Blink 對溢出的行試下一個 layout opportunity）。行的 y 有跳過的地方，所以內容的高度用最後一行的底端（照 parley，最後一行空的不算），不是 parley 的 `height()`；第一版沒略過空的最後一行，`css-text/text-indent/text-indent-overflow` 因此變高，抓到。沒有 float 時照原本的一次全排。`inline_fragments` 重新斷行時用 `PerformLayout` 時記下的區段（`laid_out_beside`）。行的 fragment 的 x 加上 parley 的 `inline_min_coord`（行的起點），`text-overflow` 截在行自己的終點（`inline_max_coord`）。
- **inline-level 的 float**（第 20、21 條）：跟著行內內容的 float 是 `InlineItem::Float`，parley 的 `InlineBoxKind::CustomOutOfFlow` 的 box；它的盒子在 `resize_boxes` 和 atomic inline 一起排（只要大小）。parley 斷到它時交回控制權（`InlineBoxBreak`），kun 看這一行剩下的寬度：放得下、這一行前面也沒有放到下面的 float，就暫時縮窄這一行（`set_line_max_advance`）、記成這一行旁邊的；否則記成這一行下面的（Blink 的 `ShouldPushFloatAfterLine`）。這一行斷完時：旁邊的 float 不在這一行（斷在它前面，box 的位置比這一行的 `advance` 後面），不算它、重排這一行（Blink 的 `RewindFloats`）；都沒有時旁邊的 float 用 `place_floated_box` 放在這一行的頂端，重新查區段，再照區段重排這一行（不用 `set_line_x` 自己移：taffy 放的位置可能不是預期的，例如 `clear`，照區段排一定對）；最後這一行定了，下面的 float 放在這一行的底端。縮窄之後內容溢出（float 後面是不能斷的內容）時，旁邊的 float 照樣放在這一行的頂端，這一行照區段往下移到放得下的地方（Blink 先放放得下的 float，再在下一個 layout opportunity 試這一行；WPT 的 `float-no-content-beside-001`、`floats-132`：第一版把 float 改放下面，抓到）。float 的 margin box 因為負 margin 比 0 窄時不占這一行的寬度（不然這一行比 layout 的上限寬，parley 的斷言失敗）。放進 BFC 的 float 不再收回，重排時 parley 再交回它就直接略過。float 用的 BFC：IFC 在別人的 BFC 裡（收到 `BlockContext`）時放進那個（先設 content box 的 inset，float 的 containing block 是 content box），它的高度只到行的底端、float 由 BFC 的根包住；自己是 BFC 的 IFC（inline-block、`overflow` 不是 `visible`、float、flex item 等，沒有收到 `BlockContext`）自己建 `BlockFormattingContext`，高度包住它的 float（CSS 2 § 10.6.7）。斷行的快取在放 float 時不用（每次都要把 float 放進 BFC）；`PerformLayout` 之後記下包含自己的 float 的區段，`inline_fragments` 重斷時照它排、float 不再放（位置是 `PerformLayout` 時放的，`place_floats` 設給它的盒子，連同 `position: relative`）。shrink-to-fit 的 IFC 有 float 時在它的 fit-content 寬度斷行（沒有 float 時照舊在可用寬度：結果一樣，atomic inline 少排一次）。只有 float（和 out-of-flow 盒子、會摺掉的空白）的內容照舊交給 taffy 的 block layout：Blink 在沒有行的 IFC 裡放它們，位置一樣。fragment 的 children 裡，IFC 的 float 排在 atomic inline 後面（行用 index 指 atomic inline）；paint 的 float 階段和 hit test 也走進 IFC 的 float。float 裡的 out-of-flow 盒子從它被放的位置傳上去。
- **intrinsic size**：min-content 是文字的和最寬的 float 取大；max-content 是文字的加上 float 並排的寬度（taffy 的 `FloatIntrinsicWidthCalculator`，`clear` 的不並排）：Blink 的 `InlineNode::ComputeMinMaxSizes` 把 float 加在它所在的那一行，kun 加在最寬的那一行，float 不在最寬的那一行時比 Chrome 寬。
- **paint、hit test、幾何**：float 的 box 是一般的 block box，有自己的 fragment；paint 照 CSS 2 附錄 E 第 5 步：在 block 的背景之後、行之前，每個 float 整個畫完（它裡面的 positioned 盒子和 stacking context 照舊是它所在的 stacking context 的；Blink 的 `PaintPhase::kFloat`），hit test 照 paint 的反序。`getBoundingClientRect` 等照 fragment。
- **對照 Blink**：`ExclusionSpace`（每個 BFC 的 exclusion）、`LayoutOpportunity`、`floats_utils.cc` 的 `PositionFloat`、`BlockLayoutAlgorithm::HandleFloat`、`LineBreaker::HandleFloat`。kun 把 exclusion space 交給 taffy（第 1 條），自己只做行內那一側。

### `vertical-align`

- `css` 的值：`baseline`、`sub`、`super`、`text-top`、`text-bottom`、`middle`、`top`、`bottom`、`<length>`、`<percentage>`（相對自己的 `line-height`）。
- `layout`：`InlineContent` 用 parley 的 `TreeBuilder` 建 layout（第 14 條）：根的 style 是 block container 的（轉成 `TextStyle`；空行的 strut 用它），每個 inline 元素一個 span，在它父元素的 span 裡（開在這段內容之前的元素、也就是被 block 切開的，先由外到內進去），帶它的文字 property 和 `StyleProperty::VerticalAlign`；每個 run 一個 modification span，只帶 brush 和 `VerticalAlign::BASELINE`（位移在元素的 span 上，run 不再移一次）；元素的邊和 atomic inline（`inline-block`、`inline-flex`、`inline-grid`、圖片、`<svg>`、canvas）是 inline box，照 byte offset 和 runs 合併、依序 push，atomic inline 帶自己的 `InlineBox::vertical_align`（`InlineItem::Atomic` 帶著它的 style）。元素的 span 從父 span 複製再套屬性，所以 `letter-spacing`、`word-spacing` 是 0 也要設（`normal` 在有間距的父元素裡）。parley 的 `VerticalAlign` 是 `AlignmentBaseline` 加 `BaselineShift`，CSS 2 的八個關鍵字各有常數，長度和百分比算成 `BaselineShift::Length`（百分比對自己的 `line-height`，`normal` 時用同一個 style 排一個字取字型的 ascent、descent、line gap）；`sub`、`super`、`middle`、`text-top` 相對於父 span 的字型，位移逐層疊加。`top`、`bottom` 是 line-relative 的 aligned subtree（parley 有）。block 自己的文字不套：絕對定位的 span 已經是 block，它的 `vertical-align` 對它自己的行沒有意義。inline 元素的背景框照它自己的 run 的 baseline 放（`LineBox::baseline`），沒有文字時照行的。
- 不做：table cell（沒有 table）；`vertical-align` 對 block 無效（照 CSS）。
- **對照 Blink**：`NGInlineLayoutStateStack`、`NGInlineBoxState::ComputeBaselineShift`（`middle` 用父元素字型的 x-height 的一半，百分比用 `line-height`）。要確認 parley 的 `middle` 和 top/bottom 的 aligned subtree 和 Chrome 一致（WPT 的 `css/CSS2/linebox/`）。

### 文字的 property

- **`letter-spacing`、`word-spacing`**：`css` 的 `normal | <length>`（`<percentage>` 照 Chrome 不收，第 9 條）；每個 span 設 parley 的 `StyleProperty::LetterSpacing`、`WordSpacing`。computed value 照 Chrome：`letter-spacing` 的 0 是 `normal`，`word-spacing` 的 `normal` 是 `0px`。
- **`text-indent`**：`<length-percentage>`（`hanging`、`each-line` 照 Chrome 不收，第 13 條）；百分比對 containing block 的寬度，斷行時才知道（`break_lines(width)`），設在 parley layout 的 `set_text_indent`；只有元素的第一個 formatted line，匿名 block 的行不縮（css-text 3；Chrome 也是），所以匿名 block 的 style 的 `text-indent` 是 0。
- **`text-transform`**：`none | capitalize | uppercase | lowercase`（`full-width`、`full-size-kana` 不做）。在 kun 處理空白的那一步（`text.rs` 的 `collapse_white_space`，Blink 的 `NGInlineItemsBuilder`）轉換，offset mapping（`TextSource`）記下每個產生的字元對到來源的哪一段：一個字元變成幾個時（ß → SS）幾個產生的字元對同一段（Blink 的 offset mapping 的 expand unit），所以選取、`Range` 的幾何、hit test 回到 DOM 的位置照舊對。`capitalize` 的「字」照 CSS Text 3（typographic letter unit 的第一個），用 `icu_segmenter` 的 `WordSegmenter`（workspace 已經依賴它：`dom_api` 的 editing、`driver`）。大小寫用 Rust 的 Unicode 轉換（Blink 的 `CaseMap`，有 locale 的特例：土耳其文的 i；kun 先不看 `lang`，記成限制）。
- **`word-break`**（`normal | break-all | keep-all`）、**`overflow-wrap`**（`normal | break-word | anywhere`）：parley 的 `WordBreak`、`OverflowWrap`，每個 span 設。`word-break: break-word`（Chrome 的非標準值）當成 `normal` 加 `overflow-wrap: anywhere`（Chrome 的作法）。
- **`white-space` 的 longhand**：`white-space-collapse`（`collapse | preserve | preserve-breaks | preserve-spaces | break-spaces`）和 `text-wrap-mode`（`wrap | nowrap`），`white-space` 改成它們的 shorthand（Chrome 152 也是：一個關鍵字單獨出現，或 `<'white-space-collapse'> || <'text-wrap-mode'>` 任意順序），`text-wrap` 也是 shorthand、只收 `wrap | nowrap`（第 8 條；`getComputedStyle` 的 `text-wrap` 是 mode 的值，Chrome 把 `auto` 的 style 省略，`cssText` 裡只有經過 shorthand 設的才寫成它）；kun 的 `WhiteSpace` enum 拆成兩個欄位（第 7 條），`collapse_white_space` 照 `white-space-collapse` 做、`TextWrapMode` 照 `text-wrap-mode` 設；CSSOM 的 `white-space` 照 shorthand 的規則序列化（對不回關鍵字的組合序列化成兩個 longhand 的值）。放進這份的第 1 段（第 4 條）。
- **對照 Blink**：`FontDescription` 的 `LetterSpacing`、`WordSpacing`（在 `HarfBuzzShaper` 加）；`NGLineBreaker` 的 `text-indent`；`NGInlineItemsBuilder` 加 `NGOffsetMappingBuilder` 的 `text-transform`；`white-space` 自 M114 起是 `white-space-collapse` 和 `text-wrap-mode` 的 shorthand。

### `text-overflow`

- `css` 的 `clip | ellipsis`。只在 block container 的 `overflow` 不是 `visible` 時有作用（CSS Overflow 3），對它直接包含的每一行各自截（Chrome 對每一行都截，不只最後一行）。
- `layout`（第 16 條）：`BoxTree` 建 `InlineContent` 時傳入這個 block 要不要截（`text-overflow: ellipsis` 而且 `overflow` 不是 `visible`；匿名 block 看它的 parent，Blink 的 `ShouldTruncateOverflowingText`），要截的先用 block 的 style 排好省略號（U+2026，block 的第一個字型沒有它時是「...」；Blink 的 `LineTruncator::SetupEllipsis` 用 line style 的 primary font）。`InlineContent::fragments` 把每一行轉成 fragment 時，行的內容超出 content box 就截（Blink 在 `CreateLine` 裡斷行之後跑 `LineTruncator::TruncateLine`）：從最後一個 run 或 atomic inline 往前，整個在 box 外面的隱藏；扣掉省略號的寬度放得下整個 child 的，省略號放在它後面；放不下的文字 run 截在放得下的最後一個 cluster，一個都放不下就隱藏、再看前一個；行的第一個 child 至少留一個 cluster 或整個 atomic inline（css-overflow 4：第一個字元或 atomic inline 要 clip、不 ellipse）。被截的 `TextFragment` 留著完整的幾何，多記 `hidden_from`（從第幾個 glyph 起不畫）和 `visible_advance`；被隱藏的 atomic inline 是 `AtomicInline::hidden`；省略號是行的最後一個 `TextFragment`，元素是它後面那個 run 的（點它和點那段文字一樣，Blink 把省略號掛在被截的 child 的 layout object 上）。inline 元素的框不變（Blink 的 box fragment 也照沒截的大小，`offsetWidth` 不變）。
- `paint`：只畫可見的 glyph，裝飾畫到 `visible_advance`；隱藏的 atomic inline 不畫、不打到；hit test 用 run 可見的寬度。
- 不做：`text-overflow: <string>`、雙值（Chrome 也沒有）。`overflow-x`、`overflow-y` 任一個不是 `visible` 就截（Blink 的 `HasNonVisibleOverflow`；kun 的 `clip` 算成 `hidden`）。

### `visibility`

- Hit test 早就照 Blink 的 `VisibleToHitTestRequest` 不打到 `visibility: hidden` 的 box 和文字（`paint` 的 `hittable`，`geometry.rs` 有測試）；`docs/limitations.md` 說沒有是過時的，第 1 段修正文件。`collapse` 照舊等於 `hidden`（沒有 table）。

### 測試

- **`css` 的整合測試**：每個 property 的解析和序列化（含 shorthand 的 `white-space`）。
- **`layout` 的整合測試**：float 的位置（左、右、好幾個、`clear`、和 margin collapsing）、行內內容繞過 float（每一行的 x 和寬度、撞到更低的 float 時重排）、inline-level 的 float 放在這一行還是下一行；`vertical-align` 每個關鍵字的 baseline 位移；`text-indent` 的百分比和 `hanging`；`text-transform` 的 offset mapping（ß → SS 的 `Range` 幾何）；ellipsis 截在哪個 cluster。
- **kun 的 reftest**（`tests/reftests/`）：`floats/`（新）、`inline/`（`vertical-align`）、`text/`（spacing、indent、transform、ellipsis、`word-break`）。
- **WPT**（`tests/wpt/include.txt` 加目錄，照 `tests/wpt/README.md` 記預期結果）：`css/CSS2/floats/`、`css/CSS2/floats-clear/`、`css/CSS2/linebox/`（`vertical-align`）、`css/css-text/text-transform/`、`letter-spacing/`、`word-spacing/`、`text-indent/`、`word-break/`、`overflow-wrap/`（第 1 段整個目錄加進去了）、`white-space/`（整個目錄 455 個只有 108 個整個通過，大多是 `ch` 單位、`tab-size`、字型和 `::before` 這些別的限制：只加通過、和這份有關的 27 個檔案）、`css/css-overflow/` 的 `text-overflow-*`、`css/css-text/parsing/` 對應的檔案。
- **情境**：畫面的改變由 reftest 守住，不另外寫情境；`about:demo` 加一段 float 和 ellipsis 的示範（手動看）。
- **mutation test**：每段一份。
- **量測**：不排（Speedometer 只在使用者要求時量，`docs/benchmarks.md` 的「效能的量測」）；第 2b 段量過一次，見 `progress.md`。

### 分階段

1. **文字的 property**：`letter-spacing`、`word-spacing`、`text-indent`、`text-transform`（含 offset mapping）、`word-break`、`overflow-wrap`，（「待決定」3 同意的話）`white-space` 的 longhand（第 4 條）；`visibility` 的 hit test。只改 `css`、`computed_style`、`layout/text.rs`、`paint` 的 hit test。
2. **`vertical-align`**，兩段：2a 在現在的 `RangedBuilder` 上設 parley 的 `VerticalAlign`（巢狀的先相對於 block，記成限制）；2b 把 `InlineContent` 改用 parley 的 `TreeBuilder`，span 有 parent，巢狀照 CSS（第 14 條）。
3. **`text-overflow`**：`layout` 的截行、fragment、`paint`。
4. **float 和 clear**：最大、最後做（第 5 條），再分三段（第 22 條原本是 4a 和 4b 一起 merge；4c 接著在同一個 branch 做完，三段一起 merge）：
   - 4a `float`、`clear` 的 style，taffy 的 `float_layout`（block-level 的 float、`clear`、BFC 避開 float），paint 和 hit test，快取（第 18 條）。
   - 4b 行內內容逐行繞過 float（第 19 條）。
   - 4c inline-level 的 float（第 20、21、23 條）、自己是 BFC 的 IFC、intrinsic size。

## 缺點

- 開 taffy 的 `float_layout` 之後，block layout 走它新的 float 程式（0.14 才加，0.15 修了 BFC 的位置）；它的 bug 和 flex、grid 一樣記成已知限制，不 fork（RFC 0003 的原則）。
- 行內內容有 float 時斷行變成逐行、可能重排一行，比一次全排慢；沒有 float 的 block 照舊，不受影響。
- `text-transform` 讓 offset mapping 從「每個保留的字元一筆」變成「可能幾個產生的字元對一段」，`text.rs` 的 mapping 更複雜。
- `white-space` 變 shorthand 動到 `css` 的 property 表和 CSSOM 的序列化，和 RFC 0041 的第 5 段（邏輯屬性的 shorthand）會在同一個檔案改，後做的 rebase。

## 理由和其他做法

完整的比較在 `details.md`。

- **float 用 taffy 的 `float_layout`（選這個）對 kun 自己做 exclusion space**：taffy 已經實作 CSS 2 的九條規則、`clear`、和 margin collapsing 的互動、intrinsic size，而且和 kun 其他的 block layout 在同一個引擎裡（float 影響它旁邊 block 的位置，exclusion space 要和 block layout 的 y 一起算，分開做要兩邊同步）。kun 自己做（Blink 的 `NGExclusionSpace`）可以完全照 Blink，但要重做 block layout 和 float 的互動，和「不 fork taffy、用它的 block layout」的方向相反。
- **`text-transform` 在 kun 做（選這個）對在 parley 的 fork 做**：parley 不知道 DOM 的 offset mapping；kun 的 `collapse_white_space` 已經在做字元層級的處理和 mapping，加一步最自然。fork 要盡量小。
- **`text-overflow` 在 layout 截（選這個）對只在 paint 蓋省略號**：paint 只能畫得像，`getClientRects`、選取、hit test 都會對到被遮住的字；Chrome 在 layout 做。
- **`white-space` 的 longhand 放進這份（選這個）對留給之後**：Speedometer 的 Spectrum 直接用 longhand（`text-wrap-mode: nowrap` 13 條），不做的話 `nowrap` 整個丟掉，`text-overflow: ellipsis` 也多半靠它；代價是 shorthand 的序列化（第 4 條）。
- **不做**：TodoMVC 的 footer、Spectrum 的標題排得和 Chrome 不同，Complex-DOM 的比值一直沒意義。

## 先例

- **Blink**：float 是 `NGExclusionSpace` 加 `NGLayoutOpportunity`，block 和 inline 的 layout 都向它問空間；`vertical-align` 在 `NGInlineLayoutStateStack` 算 baseline shift；`text-transform` 在 `NGInlineItemsBuilder` 建 inline item 時轉換、`NGOffsetMappingBuilder` 記 mapping；`text-overflow` 是 `NGLineTruncator`；`letter-spacing`、`word-spacing` 進 `FontDescription` 由 shaper 加；hit test 看 `VisibleToHitTestRequest`。照做的：inline 那一側的 float 處理、offset mapping、layout 截行、hit test 的 `visibility`。不照做的：exclusion space 由 taffy 管。
- **taffy 0.15**（設計時是 0.14）：`float_layout` feature（`compute/float.rs`：`FloatContext`、`ContentSlot`、`BfcSlot`、`FloatIntrinsicWidthCalculator`），block layout 把 `BlockContext` 傳給子元素的 `compute_block_child_layout`，讓行內內容向它要 slot。
- **parley（kun 的 fork）**：`VerticalAlign`（`AlignmentBaseline` 加 `BaselineShift`）、`LetterSpacing`、`WordSpacing`、`IndentOptions`、`WordBreak`、`OverflowWrap`、`TextWrapMode`、`WhiteSpaceCollapse`、`InlineBoxKind::CustomOutOfFlow`（給 float）、每一行的 `set_line_x`、`set_line_max_advance`。fork 改了兩處，照「改 fork 要盡量小」：atomic inline 的 `text-wrap-mode`（第 17 條）和 `LineBreakData::hanging_advance`（第 23 條）。

## 已決定的事項

2026-10-13 確認：

1. **float 的 exclusion space 交給 taffy 0.14 的 `float_layout`**，kun 只做行內那一側（每行向 `BlockContext` 要 slot、行太高時重排、inline-level 的 float 經過 parley 的 `CustomOutOfFlow`）。不選 kun 自己做 Blink 的 `NGExclusionSpace`：要和 taffy 的 block layout 同步 y，等於重做 block layout，和用 taffy 的方向相反。
2. **`text-transform` 在 kun 的 `collapse_white_space` 做**，offset mapping 擴成一對多（Blink 的 expand unit）。不改 parley 的 fork（它不知道 DOM 的位置，mapping 還是要 kun 做）。
3. **`text-overflow` 在 layout 截行**（Blink 的 `NGLineTruncator`），不只在 paint 蓋省略號：fragment、hit test、選取、`getClientRects` 要一致。
4. **`white-space` 的兩個 longhand（`white-space-collapse`、`text-wrap-mode`）和 `word-break`、`overflow-wrap` 放進這份的第 1 段**，雖然路線圖的 S5 沒列：Speedometer 的 Spectrum 直接用 longhand（`text-wrap-mode: nowrap` 13 條）和 `word-break`（12 條）、`overflow-wrap`（11 條），parley 都支援；`white-space` 變 shorthand 的序列化是代價，和 RFC 0041 第 5 段在同一個檔案改、後做的 rebase。
5. **四段由小到大，float 最後**（文字的 property → `vertical-align` → `text-overflow` → float）；float 的原型（「待決定」1）在第 1 段期間先做，不等到第 4 段。不選 float 先做（對 TodoMVC 的畫面影響最大）：它最大、未知最多，前三段先 merge 讓 Speedometer 的宣告早點生效。
6. **獨立一份 RFC，四段**，每段做完可以先 merge。
7. **`white-space` 在 `ComputedStyle` 拆成 `white_space_collapse` 和 `text_wrap_mode` 兩個欄位，`white-space` 是 `Shorthand`**（2026-10-13，第 1 段開始前）：Chrome 的模型，longhand 展開和 CSSOM、`getComputedStyle` 的序列化走現有的 shorthand 機制。不選留著 enum、從 longhand 推回去：表示不了 `preserve-spaces` 加 `nowrap` 這類組合。
8. **`text-wrap` 也是 shorthand，只收 `wrap | nowrap`**（2026-10-13，第 1 段開始前）：頁面常寫 `text-wrap: nowrap`；kun 沒有 `text-wrap-style`，`balance`、`pretty`、`stable` 照舊無效，記成限制。
9. **`word-spacing` 只收 `<length>`**（2026-10-13，第 1 段開始前）：CSS Text 3 的 `<percentage>` Chrome 沒做；`letter-spacing` 的百分比（css-text 4）也一樣。
13. **`text-indent` 不收 `hanging`、`each-line`**（2026-10-13，第 1 段做的時候）：Chrome 沒做（WPT 的 `text-indent-valid.html` 在 Chrome 也失敗），parley 有但先不接；草稿原本要收。
14. **`vertical-align` 分 2a、2b，2b 把 `InlineContent` 改用 parley 的 `TreeBuilder`**（2026-10-13，第 2 段開始前）：parley 的 `RangedBuilder` 把每個 span 的 parent 都當成根，`vertical-align` 只能相對於 block：巢狀的不疊加、有 `vertical-align` 的 span 裡的 atomic inline 不跟著移、`middle`、`text-top`、`sub` 用的是 block 的字型。`TreeBuilder` 每個 span 有 parent，照 CSS；要重寫建 layout 的那一段（runs 和 tags 照 byte offset 合併、依序 push span、text、inline box，根的 style 轉成 `TextStyle`，每個 run 用 modification span 帶 brush），fork 的 `InlineBoxLineBreak` 在這條路徑上重新驗證，inline 多的 suite 要量效能。先做 2a（diff 好審、Speedometer 的用法都是一層），2b 拿掉限制。不選只做 2a：語意不對的那幾種情況真實頁面會碰到（`<sup>` 裡的 `<sup>`、圖示在有 `vertical-align` 的 span 裡）。
15. **`sub`、`super` 的位移用 parley 的**（2026-10-13，第 2 段開始前）：`font-size / 5` 和 `font-size / 3`，Blink 各多 1px；記成限制，WPT 看影響大再改。
16. **`text-overflow` 照 Blink 的 `LineTruncator`**（2026-10-13，第 3 段開始前）：在 `InlineContent::fragments` 轉 fragment 時截（不改 parley 的 fork：parley 不知道 content box 和 fragment）；被截的 run 留一個 fragment、記從哪裡起不畫（不照 Blink 留一份隱藏的、插一份截短的：那是 Blink 的 item 模型，kun 這樣做 `getClientRects` 會算到兩次，語意一樣、程式多）；atomic inline 整個隱藏；省略號用 block 的 style 和主字型、U+2026 或「...」；截哪些、第一個 child 留什麼照 Blink 的規則；匿名 block 看 parent；`text-overflow` 只收 `clip | ellipsis`、不繼承。
17. **parley 的 fork：atomic inline 照它所在的 span 的 `text-wrap-mode` 斷行**（2026-10-13，第 3 段做的時候）：parley 的斷行器只在處理文字時更新目前的 wrap mode，`nowrap` 的行開頭是 inline box 時用的是預設的 `Wrap`，在它後面換行（WPT 的 `text-overflow-009`：開頭的 inline-block 後面的文字跑到下一行），min/max-content 的量法也一樣。改成 `Auto` 的 inline box（atomic inline）用它的 `parent_style_index` 的 mode；元素的邊（`KeepWithNext`、`KeepWithPrevious`）不改：它們旁邊的換行機會屬於旁邊的內容，第一版連邊也改，`inline-formatting-context-012`（兩個 `nowrap` 的 span 中間的空格）就不換行了。不選在 kun 這邊繞：wrap mode 是斷行器內部的狀態，從外面設不到。
18. **有 float 時的快取：碰到 float 就不用快取**（2026-10-13，第 4 段開始前；原本的「待決定」2）：taffy 的快取只看 `LayoutInput`，不知道 BFC 裡的 float，局部重排時會錯（旁邊的 float 變了，用快取的段落不重新繞；包著 float 的 block 用快取時，它的 float 不再放進 BFC）。兩種盒子不用 taffy 的快取：所在的位置有還沒結束的 float（`has_active_floats`），或子樹會把 float 放進同一個 BFC（建 box tree 時算的旗標）；`InlineContent` 的斷行快取以寬度加上 float 的區段當 key。沒有 float 的頁面只多一個常數時間的判斷；有 float 時，float 垂直範圍裡同一個 BFC 的盒子每次 layout 都重排（它們本來就要逐行繞 float）。不選照 Blink 把 exclusion space 放進快取的 key、命中時重播它的 float（`ExclusionSpace::MergeExclusionSpaces`）：exclusion space 在 taffy 裡、只能從外面查，比較和重播都要另外維護一份，容易出錯；之後量到 float 多的頁面慢再考慮（「之後的可能」）。不選不處理：局部重排的結果和整份重排不同。
19. **每一行的可用寬度：排版時先記下 float 的區段**（2026-10-13，第 4 段開始前）：IFC 排版時，`has_active_floats` 是 true 才從內容的頂端用 `find_content_slot`（`after`）一段段往下查，記下每段的 y、x、寬度（content box 的座標；taffy 的 `ContentSlot.height` 一律是無限大，不告訴段在哪裡結束）。每一行取它高度範圍內的交集：第一個不能斷的內容放不下就移到下一段，行比所在的區段高就用更窄的寬度重排這一行（parley 的 `revert`，只重斷這一行，不重新 shape）。記下的區段是斷行快取的 key（第 18 條），`inline_fragments` 重新斷行時也用它（那時 taffy 的 `BlockContext` 已經不在）。成本：沒有 float 時是 0；有 float 時每次排這個 IFC 查一次 O(段數)，每一行和碰到的段求交集，重排的次數大約是 float 的個數。這是 Blink 的模型：`InlineLayoutAlgorithm` 從 constraint space 拿到 exclusion space 的快照，用 `LayoutOpportunityIterator` 逐一試 layout opportunity。不選每一行即時查（每行 O(段數)，而且結果只對那一次斷行有效，`inline_fragments` 換寬度重斷時查不到）；不選只看行頂端那一段（行比 float 的頂端高時壓到 float 上，真實頁面常見）。
20. **哪些 float 是行內的項目：照 Blink 的 `LayoutBlockFlow::AddChild`**（2026-10-13，第 4 段開始前）：和 RFC 0044 第 5 條的 out-of-flow 盒子同一條規則（Blink 對 float 和 out-of-flow 盒子也是同一條）：容器只有行內內容時 float 是行內的項目（Blink 的 `InlineItem::kFloating`）；有 block 子盒子時跟著緊接在它前面的那段行內內容，前面是 block、容器的開頭或只有會摺掉的空白時是 block-level，交給 taffy。不選一律當 block-level（把行內內容切成匿名 block）：段落中間的 float 會把段落切斷，float 前面的字提早換行。
21. **inline-level 的 float 在這一行確定之後才放進 BFC**（2026-10-13，第 4 段開始前；原本的「待決定」1 的原型確認了 parley 在 `CustomOutOfFlow` 交回控制權、繼續時重讀 `line_max_advance`）：交回控制權時先排 float 的盒子；這一行剩下的寬度放得下，就只在這一行的狀態裡暫時縮窄（left float 時 `set_line_x` 往右移）、記成暫留；這一行斷完時，float 的 box 確實在這一行的項目裡才 `place_floated_box`（這一行的頂端；放不下的在這一行的底端）並加進自己的區段（第 19 條），被退回到下一行的丟掉暫留，parley 下一行會再交回它。taffy 的 BFC 收不回已經放進去的 float，這樣做等於 Blink 的 `LineBreaker::RewindFloats`（Blink 的 exclusion space 是 line breaker 自己的，可以撤回）。成本：每個 float 一次交回控制權、排一次它的盒子、一次 `place_floated_box`，有暫留時多一次「box 在不在這一行」的檢查。不選遇到就放：斷行退回到 float 之前時，float 留在原來那一行、內容到了下一行，和 Chrome 不同。
22. **第 4 段分成 4a、4b、4c，4a 和 4b 一起 merge**（2026-10-13，第 4 段開始前）：4a 是 style、taffy 的 `float_layout`（block-level 的 float、`clear`）、paint 和 hit test、快取；4b 是行內內容繞過 float；4c 是 inline-level 的 float、自己是 BFC 的 IFC、intrinsic size。各段各自 commit、跑 mutation test、審查；只做完 4a 時 float 放到兩邊但行不繞，文字壓在 float 上，比現在（不認得 `float`，當成一般的 block）差，不符合「階段的 merge」的條件，所以和 4b 一起 merge。4b 到 4c 之間，跟著行內內容的 float 照 block-level 處理（切開段落），和現在比沒有更差。不選每段各自 merge（4a 之後 `main` 上文字和 float 重疊）；不選全部做完才 merge（branch 活最久、一次 merge 的 diff 最大）。Chromium 用 runtime flag 切換新舊 layout，可以合併做到一半的程式；kun 沒有舊的 layout 可以退回，靠 merge 的時機。

23. **parley 的 fork：`LineBreakData` 多給 `hanging_advance`，行放不下時逐行移到下一段**（2026-10-14，4c 做的時候）：inline-level 的 float 照第 21 條在一行定了之後放進 taffy 的 BFC，放了就收不回（`BlockContext` 不能複製、不能移除 float）；但 4b 判斷「這一行的內容在 float 旁邊放不下、要移到下一段」是整段斷完才看，再整段重斷，因為 parley 交回的 `advance` 含行尾掛出去的空白。重斷之前，float 可能已經照一行還沒定的位置放進去了。fork 在 `LineBreakData` 加上那一行的 `hanging_advance`（`commit_line` 本來就算好了，約 5 行，`kun:` 註解），每一行斷完就能判斷，放不下就退回、從下一段重排這一行；整段重斷（最多 `bands × 4 + 1` 趟）拿掉。這是 Blink 的做法：`LineBreaker` 逐個 layout opportunity 試一行，float 在 `HandleFloat` 時放進 exclusion space。不選只改 kun、保留整段重斷，每一趟最多放一行的 float（那一行和它之前的行都沒有放不下的才放），放了再從頭重斷：fork 不用動，但每多一行有 float 就多斷一整段，大約 O(有 float 的行數 × 段落長度)，程式也比較複雜，Blink 沒有對應的做法。代價是 fork 多一個 commit，要更新 `third_party.toml`，fork 先 push。

## 待決定

1. **inline-level 的 float 在行中途縮這一行**：已決定（第 21 條）。原型（2026-10-13）：parley 的 `CustomOutOfFlow` box 在被消耗之前交回控制權，呼叫的一方用 `append_inline_box_to_line(目前的 advance, 0, 0)` 消耗它；繼續時 `break_next` 重讀 `line_max_advance`，`set_line_x` 把整行往右移：200px 寬的一行，40px 的 left float 放得下時這一行變成 x 40 到 200，float 後面的字接在同一行。
2. **逐行斷行的快取**：已決定（第 18 條）。
3. **taffy 0.14 給子 block 的 BFC 位置沒有摺疊 margin**（2026-10-13 發現）：float 在 BFC 裡記的位置和畫出來的差一個摺疊掉的 margin，`<img style="float: left"><p>…</p>` 這種最常見的情況也繞錯；0.15 修好了。2026-10-13 決定先升級 taffy（RFC 0044，2026-10-13 完成），第 4 段在 0.15 上做，剩下的事開始前再決定。

## 之後的可能

- **`ch` 和 `ex` 單位**：`css/css-text/` 的 WPT 有一大半用 `width: 5ch` 這類寬度（word-break 和 overflow-wrap 的 63 個），kun 沒有這兩個單位時整個測試跑不出來；要在 cascade 時拿到字型的 metrics（Blink 的 `FontMetrics::ZeroWidth`、`XHeight`）。
- `position: sticky`（原本和 float 一起列在限制裡）。
- `tab-size`、`text-transform: full-width` / `full-size-kana`、照 `lang` 的大小寫特例。
- `shape-outside`、`float: inline-start`。
- `text-wrap-style: balance`、`hanging-punctuation`。
- 文字分開 shape 的 kerning（限制裡的「不同元素裡的文字分開 shape」）。
- 有 float 時的快取照 Blink 把 exclusion space 放進 key、命中時重播 float（第 18 條沒選的做法）：量到 float 多的頁面慢時再考慮。
- **使用者捲不動的捲動容器不成為 layer**（2026-10-14，查 WPT 的 `css/css-overflow/text-overflow-012`、`022` 時發現）：kun 讓有東西可捲的捲動容器都成為自己的 layer，連 `overflow: hidden`、`clip` 這種使用者捲不動的也是；layer 裡的文字先 raster 在透明的底上再疊上去，抗鋸齒的捨入和直接畫在背景上的差 1，這兩個測試因此失敗（截行本身是對的）。Chrome 只讓使用者捲得動的容器走 composited scrolling（`PaintLayerScrollableArea::ScrollsOverflow` 要捲得動的方向是 `UserInputScrollable`），其他的畫在所在的 layer 裡、script 捲它時重畫。照做的話 `paint` 的 `layerize` 多一個條件，代價是 script 捲這種容器時要重新 paint、raster（現在只移動 layer），compositor 的捲動判斷（`covered_scrollers` 等）也要跟著看；動到 compositor 的設計，另寫一份 RFC。
