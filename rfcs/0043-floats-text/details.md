# RFC 0043 的細節：float、`vertical-align` 和文字的 property

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

`rfcs/0012-speedometer/progress.md` 的「盤點：kun 丟掉的 CSS（2026-10-13，Fedora 44）」（2026-10-13，`main` 的 `297bda44`，20 個 default 的 suite）裡，屬於這份的宣告（值；條數）：

| Property | 值和條數 |
| --- | --- |
| `vertical-align` | `baseline` 14、`middle` 11、`bottom` 7、`top` 7、`text-top` 1 |
| `text-wrap-mode` | `nowrap` 13、`wrap` 10 |
| `white-space-collapse` | `collapse` 13、`preserve` 5、`break-spaces` 2 |
| `float` | `left` 11、`right` 11 |
| `word-break` | `break-all` 11、`break-word` 1 |
| `letter-spacing` | `var(--spectrum-…)` 18（三個自訂 property 各 6） |
| `text-transform` | `uppercase` 8、`none` 6、`capitalize` 2 |
| `overflow-wrap` | `break-word` 8、`normal` 2、`anywhere` 1 |
| `text-overflow` | `ellipsis` 10 |
| `text-indent` | `0px` 6 |

`clear` 沒有出現（沒被丟掉的宣告裡也沒有）。用到的 suite：`float` 和 `vertical-align` 在每個 TodoMVC（`.todo-count { float: left }`、`.filters`、footer）和 Spectrum 的 Complex-DOM；`text-overflow`、`word-break`、`text-wrap-mode` 主要是 Spectrum（Complex-DOM 的 6 個）和 NewsSite；`text-transform` 在 Spectrum 和 CodeMirror。

## 現況

- `css` 沒有 `float`、`clear`、`vertical-align`、`text-overflow`、`text-transform`、`letter-spacing`、`word-spacing`、`text-indent`、`word-break`、`overflow-wrap`；有 `visibility`（layout 和 paint 照它，hit test 不看；`collapse` 當 `hidden`）、`white-space`（一個 enum，`collapse_white_space` 自己處理空白，`TextWrapMode` 交給 parley）。
- `layout`：block、flex、grid 用 taffy 0.14（feature：`std`、`block_layout`、`flexbox`、`grid`、`content_size`、`calc`；沒開 `float_layout`）；inline formatting context 用 parley，`InlineContent::break_lines(width)` 一次排完所有的行（`layout.break_lines()` 的 breaker 逐行 `break_next`，每行只設 `prior_line_width`），`broken: Option<(width, size)>` 記住同樣寬度的結果。taffy 的 block layout 把 `BlockContext` 傳進 `compute_block_child_layout`，kun 的 `compute_box` 收到它（`block_ctx`）但沒用。
- offset mapping：`text.rs` 的 `TextSource { chars: Vec<(byte offset, utf16 start, utf16 end)>, length }`，每個保留的字元一筆，collapse 掉的空白沒有（Blink 的 `OffsetMapping`，一個節點的）。

## taffy 0.14 的 `float_layout`

`src/compute/float.rs`、`src/compute/block.rs`（2026-10-13 讀的）：

| 東西 | 用途 |
| --- | --- |
| `Style::float`（`Float::None | Left | Right`）、`Style::clear` | `BlockItemStyle` trait 的 `float()`、`clear()`；kun 用 `taffy::Style` 直接設 |
| `FloatContext::place_floated_box(size, min_y, insets, direction, clear)` | 照 CSS 2 visuren 9.5.1 的九條規則放 float，回傳位置 |
| `BlockContext::place_floated_box(size, min_y, direction, clear, adjoins_unresolved_strut)` | 上面的，換成這個 block 的座標（`y_offset`、`insets`）；也記 margin collapsing 的 strut 旁的 float（`clear` 的 clearance 要用） |
| `BlockContext::find_content_slot(min_y, clear, after) -> ContentSlot { segment_id, x, y, width, height }` | 非 float 的內容（行）在 `min_y` 以下能用的空間；`height` 是這個 slot 到下一個 float 邊界的高度，行比它高就要換 slot |
| `BlockContext::find_bfc_slot(…) -> BfcSlot` | 自己是 BFC 的 block（`overflow` 不是 visible、flex、grid 等）不能和 float 重疊，給它找位置 |
| `BlockContext::cleared_threshold(clear)` | `clear` 的元素至少要在的 y |
| `FloatIntrinsicWidthCalculator` | min-content、max-content 算進 float 的寬度 |
| block layout 的 `compute_block_child_layout(node, inputs, Some(&mut child_block_ctx))` | 子元素拿到 context，行內內容向它要 slot |

limitations 要記的：taffy 的 float 是 0.14 新加的功能，上游怎麼處理的邊界情況（負的 margin、float 比 containing block 寬、`clear` 加 margin collapsing）照它；kun 的 reftest 和 WPT 的 `css/CSS2/floats/` 會記下差異。

## parley（fork 的 `kun` 分支）已有的

| 東西 | 用途 |
| --- | --- |
| `StyleProperty::VerticalAlign(VerticalAlign { alignment: AlignmentBaseline, shift: BaselineShift })`，`InlineBox::vertical_align` | `BASELINE`、`SUB`、`SUPER`、`TEXT_TOP`、`TEXT_BOTTOM`、`MIDDLE`、`TOP`、`BOTTOM` 和 `length(f32)`；`TOP`、`BOTTOM` 是 line-relative 的 aligned subtree |
| `StyleProperty::LetterSpacing(f32)`、`WordSpacing(f32)` | 每個 span |
| `LayoutData::indent_amount`、`IndentOptions { each_line, hanging }` | `text-indent`，`resolve_indent` 每行決定要不要縮 |
| `StyleProperty::WordBreak`、`OverflowWrap`、`TextWrapMode`、`WhiteSpaceCollapse`（來自 `parlance`） | 斷行的規則 |
| `InlineBoxKind::CustomOutOfFlow` → `YieldData::InlineBoxBreak(BoxBreakData { 目前的 advance… })` | 斷到這個 box 時交回控制權，給 float 用（parley 的文件就這樣寫） |
| `BreakLines::set_line_x`、`set_line_max_advance`、`set_line_y` | 每一行的起點和寬度；`line.metrics.inline_min_coord`、`inline_max_coord` 跟著設 |

fork 的兩個本地修改（`InlineBox::line_break`、inline box 的邊不撐高行）和這份無關。預期不改 fork；「待決定」1 的原型可能發現行中途改 `line_max_advance` 要補東西。

## Blink

- **float**：`core/layout/exclusions/exclusion_space.h`（`NGExclusionSpace`：每個 BFC 的 exclusion，`FindLayoutOpportunity`、`ClearanceOffset`）、`core/layout/floats_utils.cc`（`PositionFloat`、`ComputeMarginsFor`）、`block_layout_algorithm.cc` 的 `HandleFloat`（block-level 的 float，放在目前的 BFC offset）、`inline/line_breaker.cc` 的 `HandleFloat`（行內的 float：這一行剩下的空間放得下就放在這一行的 top、縮這一行的 available width；放不下就留到行結束後放在行的下面，`NGUnpositionedFloat`）。行排出來之後高度超過 opportunity 時 `NGInlineLayoutAlgorithm` 用新的 opportunity 重排（`line_opportunity` 的迴圈）。
- **`vertical-align`**：`inline/inline_layout_algorithm.cc`、`inline/inline_box_state.cc` 的 `ComputeBaselineShift`：`sub`、`super` 用字型的 subscript / superscript offset（沒有就 1/5、1/3 的 em 這類近似），`middle` 是父元素 x-height 的一半，百分比對 `line-height`，`top`、`bottom` 把整個 aligned subtree 貼齊 line box（在 `NGInlineLayoutStateStack::ComputeInlinePositions` 最後處理）。
- **`text-transform`**：`inline/inline_items_builder.cc`（`ApplyTextTransform`，用 `platform/text/case_map.h` 的 `CaseMap`，照 `lang` 選 locale：土耳其文、立陶宛文、希臘文的特例）、`inline/offset_mapping_builder.cc`（長度變了記成 `kExpanded` / `kCollapsed` 的 unit）。`capitalize` 用 ICU 的 word break iterator 找字的開頭（`Character::IsSeparatorSpace` 等）。
- **`text-overflow`**：`inline/line_truncator.cc`（`LineTruncator`，2026-10-13 讀的）：block 的 `overflow` 不是 visible 且 `text-overflow: ellipsis` 時（匿名 block 看 parent，`ShouldTruncateOverflowingText`），`CreateLine` 在斷行之後對超出 available width 的行跑 `TruncateLine`：省略號用 line style（`EllipsisStyle`，也就是 block 的；`::first-line` 時用它的），primary font 有 U+2026 就是它，否則是「...」；從最後一個 child 往前（`EllipsizeChild`）：整個在 box 外的隱藏（`HideChild`），扣掉省略號放得下整個 child 就放在它後面，放不下的文字用 `OffsetToFit` 截（留一份隱藏的原 item、插一份截短的），inline box 不截，atomic inline 整個隱藏（`SetIsTruncated`；有定位的後代時不隱藏）；第一個 child 至少留一個字元或整個 atomic inline（css-ui）。隱藏的 item 留著幾何，`offsetWidth` 等 layout 的查詢不變。省略號是行的最後一個 child，掛在被截的 child 的 layout object 上。
- **`letter-spacing`、`word-spacing`**：`FontDescription::SetLetterSpacing`、`SetWordSpacing`，`HarfBuzzShaper` 在每個 cluster 之後、每個 space 之後加；行尾最後一個字元後面的 letter-spacing 也加（CSS Text 4 允許 UA 省略，Chrome 沒省）。
- **`text-indent`**：`inline/line_breaker.cc`、`inline/inline_layout_algorithm.cc`（`ComputeTextIndent`，百分比對 containing block 的 inline size；`hanging`、`each-line`）。
- **`word-break`、`overflow-wrap`**：`LineBreaker` 的 `break_iterator_` 設定（`LineBreakType::kBreakAll` 等），`overflow-wrap: break-word` 在整行放不下時才允許在任何地方斷（`break_anywhere_if_overflow_`）。
- **`visibility` 和 hit test**：`LayoutObject::VisibleToHitTestRequest` 要 `visibility: visible`（和 `pointer-events`）。

## 比較過的做法

### float 的 exclusion space

| 做法 | 好處 | 代價 |
| --- | --- | --- |
| **taffy 的 `float_layout`（選這個）** | 九條規則、`clear`、margin collapsing 的互動、intrinsic size 都有；和 kun 其他的 block layout 在同一個引擎、同一組座標；kun 只做行內那一側 | 0.14 才加的功能，它的 bug 照 flex、grid 記成限制；逐行斷行要照它的 `ContentSlot` 寫 |
| kun 自己的 exclusion space（Blink 的 `NGExclusionSpace`） | 完全照 Blink，之後 `shape-outside` 等接得上 | 要和 taffy 的 block layout 同步 y（float 影響旁邊 block 的位置和 margin collapsing）：等於重做 block layout，和 RFC 0003 用 taffy 的方向相反 |

### `text-transform` 在哪裡做

| 做法 | 好處 | 代價 |
| --- | --- | --- |
| **kun 的 `collapse_white_space`（選這個）** | 已經在字元層級處理、建 offset mapping；mapping 的擴充（一對多）在同一個地方 | `text.rs` 的 mapping 從一對一變一對多 |
| parley 的 fork 加 `TextTransform` | 對呼叫者透明 | parley 不知道 DOM 的位置，mapping 還是要 kun 做；fork 變大 |

### `text-overflow` 在哪裡截

| 做法 | 好處 | 代價 |
| --- | --- | --- |
| **layout 截行（選這個；Blink）** | fragment、hit test、選取、`getClientRects` 一致 | fragment 多一個「從這裡起隱藏」的記號，line truncation 的程式 |
| paint 蓋省略號 | 最小 | 幾何對到被遮住的字；和 Chrome 不同 |

### `white-space` 的 longhand

| 做法 | 好處 | 代價 |
| --- | --- | --- |
| 放進這份（第 1 段） | Spectrum 的 `text-wrap-mode: nowrap`（13 條）生效，`text-overflow` 有用的前提 | `white-space` 變 shorthand：property 表、cascade 的 longhand 展開、CSSOM 序列化；和 RFC 0041 第 5 段的 shorthand 在同一個檔案改 |
| 留給之後 | 這份小一點 | Spectrum 的 `nowrap` 丟掉，標題照樣換行，`text-overflow: ellipsis` 在那些地方沒作用 |
