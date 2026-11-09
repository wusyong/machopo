# RFC 0083：文字 layout 的工作量

- 開始日期：2026-10-23
- 狀態：✅ 已完成：2026-10-24（驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`（TipTap 的計數和取樣、taffy 的路、column flexbox 被問寬度的次數、文字的 fragment 花在哪和共用之後的計時、Blink 的對照、kun 的程式）

## 摘要

Speedometer 的 Editor-TipTap 每一步都把約 244 KB 的文字重新 shaping、斷行、做成 fragment，kun 在這三件事上一步約花 24、10、10 ms；Chrome 在 Long 那一步的 `setValue`（含 ProseMirror 的 JS）加上每步之後強迫的 layout 只有約 18 ms。這份 RFC 分三段拿掉其中不必要的工作，TipTap 從 120.2 ms 到 79.8 ms（−33.6%，`progress.md` 的「做過的檢查」）：

1. **column flexbox 的 min/max-content 寬度不排子項的內容**：照 Blink 的 `FlexLayoutAlgorithm::ComputeMinMaxSizes`，非換行的 column container 的寬度取子項的 min/max-content 貢獻；行內內容只被問寬度時也不斷行（Blink 的 `InlineNode::ComputeMinMaxSizes`）。原本 taffy 先在 min-content 的寬度下排一次子項，算出用不到的高度，TipTap 每段因此在約 100 px 寬度多斷一次行，佔斷行時間的六成。
2. **文字的 fragment 共用 shaping 的結果**：照 Blink 的 `ShapeResultView`，行上的每個 run 是段落 shaping 結果（parley fork 裡的 `Arc`）的一段，排版時不走它的 glyph 和 cluster：glyph 在 paint 時放，cluster 和行尾用到時才算。
3. **shaping**：取樣拆出 icu 的分析、選字型、harfrust、parley 的簿記和斷行記緊急斷點各佔多少，從四個候選（字詞的 shaping 快取、比較便宜的分析、溢出時才找緊急斷點、換成 HarfBuzz）做了兩個：字詞的 shaping 快取（照 Gecko 的 `gfxShapedWord`，但放在選字型之前；字型的規則只把空白和旁邊一個 glyph 一起比對時，在那些 glyph 的邊界把字併起來重新 shaping），和 U+0000–U+017F、U+2000–U+203F 的字元查表找斷行機會（照 Blink 的 `ShouldBreakFast`，表從 ICU4X 產生）、kun 不算用不到的字的界線。第 2 段搬到 paint 的放 glyph 改成直接的迴圈。

## 動機

**現況**（2026-10-23，Windows 11，暫時加在 `crates/layout/src/text.rs` 的計數，沒有 commit；量法和完整的數字見 `details.md` 的「TipTap 的計數」）：TipTap 每個 iteration 兩步，Long（`setContent` 換成長文，約 260 段）和 Highlight（全選加粗），每一步：

| 工作 | 量 | 時間 |
| --- | --- | ---: |
| shaping（建 box tree 時，`InlineContent::new`） | 256–261 段、約 244 KB；kun 的 shaping 快取幾乎沒命中 | 22–25 ms |
| 斷行（`InlineContent::break_lines`） | 真的斷的 604 次（另外 814 次寬度和 float 沒變、沿用），488 KB、20,143 行 | 10–10.6 ms |
| min/max-content 寬度 | 279 段 | 1.4–1.6 ms |
| 做 fragment（`InlineContent::fragments`） | 262 段、約 1,900 行 | 9.6–10.6 ms |

合計每個 iteration 約 92 ms，和 2026-10-23 那一版的 `docs/performance-gaps.md`（2026-11-06 整份改寫之前）裡「Windows 上」那一節取樣到的文字 90.2 ms 相符。Chrome 的 `setValue`（Long 那一步，含 ProseMirror 的 JS 和 DOM）是 14.0 ms，runner 每步之後強迫的 layout 合計 4.2 ms（2026-10-23 那一版的 `docs/performance-gaps.md`（2026-11-06 整份改寫之前）裡「Windows 上」那一節）。

看到的事：

- **沒改的文字沒有重排**：時間都在真的新的文字上。Long 換掉全部的節點，Highlight 換掉每個 run 的字型（粗體是另一個字型檔），Chrome 一樣要重新 shaping。步驟之間的小的強迫 layout 每次約 0.4 ms。
- **每段斷了兩次行**：第一次在 90–120 px（那段的 min-content 寬度）斷成一萬八千多行，第二次才在 780 px 斷成真正的 1,900 行；大段落裡窄的那次 5.1 ms、真正的 2.9 ms。來源是頁面的 `body`（row）和 `#app`（column）兩層 flexbox：`body` 要 `#app` 的 min-content 寬度（自動最小尺寸），taffy 算 column container 的寬度時照樣跑 flex 演算法的第 3 步，在 min-content 寬度下量每個子項的高度（`details.md` 的「taffy 的路」）。
- **shaping 的快取用不上**：第 2、3 個 iteration 的文字其實看過，但一個 iteration 要 shaping 約 49 萬字，超過快取的 2^18 字，快取一直重來；Highlight 換了字型，本來就不中。Blink 的 `NGShapeCache` 只收 30 字以內的文字，長段落一樣每次重新 shaping，所以 Chrome 快在每個字的成本，不在快取。
- **fragment 每行每個 run 都複製**：glyph 的 id 和絕對位置、cluster 和它在 DOM 文字裡的 UTF-16 位置、每個 run 一個新的字型資料的 `Arc`；`translate` 移動 fragment 時逐個 glyph 改位置。
- **斷行的 `clone`**：ProseMirror 用 `overflow-wrap: break-word`，parley 在每個字元記一次緊急斷行機會，每次 clone 一份 `LineState`（RFC 0058 第 9 條）。多斷的那一次讓這個成本也多一倍以上。

**之前的 RFC**：RFC 0051 第 27 條估計文字的改善約 40–60 ms 而不做；RFC 0058 做了 cluster 的 UTF-16 位置（1c）和選字型的快速路徑（2c），第 9 條把 `LineState` 留給「以後處理文字的 RFC」。這次的計數多找到兩件它們沒看到的事：多斷的那一次來自 flexbox，fragment 的成本可以照 Blink 不複製。

**目標**：

- 非換行的 column flexbox 被要求 min-content 或 max-content 寬度時，不排子項的內容；TipTap 的每段只在最後的寬度斷一次行。
- 排版做文字的 fragment 時不再逐個走 glyph 和 cluster；做 fragment 的時間降到現在的一小部分。
- shaping 的成本拆清楚，選定、做完第 3 段的做法。
- 排版、畫面、hit test、選取的結果都不變。

**要解決的 `docs/limitations.md` 的限制**：無。這份 RFC 只拿掉多做的工作。

**不在範圍內**：

- 改了一個字時只重新 shaping 那一段（Blink 的 `ReusingTextShaper`、`InlineNodeDataEditor`）和沿用沒變的行（`docs/limitations.md` 裡一個字改了、整個段落重新 shape 的那一條）：TipTap 的兩步都是全新的內容，用不上；放在「之後的可能」。
- 加大 shaping 快取，讓 Speedometer 的下一個 iteration 命中：Chrome 也沒有，網頁不會這樣用。
- NewsSite-Next、Nuxt 的 flexbox 本身（`docs/performance-gaps.md` 的「可以開 RFC 的方向」第 3 項）：第 1 段可能順便讓它們少做一些，但不以它們驗收。
- 量 Speedometer：照 `docs/benchmarks.md`，只在使用者要求時量。第 3 段的取樣是設計需要的輸入，使用者已經同意（第 1 條）。

## 設計概要

網頁看不出差別：排出來的位置、畫面、`getBoundingClientRect`、選取都和現在一樣，只是快。

```html
<body style="display: flex">
  <div id=app style="flex: 1; display: flex; flex-direction: column">
    <div id=editor style="flex: 1; overflow: auto">
      <p>很長的一段……</p>  <!-- 幾百段 -->
    </div>
  </div>
</body>
```

- 現在：`body` 問 `#app` 的 min-content 寬度時，taffy 在每段的 min-content 寬度下排一次 `#editor`，每段斷成一行一個字；接著在真正的寬度再斷一次。
- 第 1 段之後：`#app` 的 min-content 寬度是子項的 min-content 貢獻裡最大的（段落的 min-content 寬度本來就記著），每段只在真正的寬度斷一次行。
- 第 2 段之後：段落的每行的每個 run 指向段落 shaping 的結果的一段，排版時不再逐個走 glyph 和 cluster。
- 第 3 段之後：重複的字（同樣的樣式和選項）只 shaping 一次，Long 裡的和 Highlight 的粗體都是；表內的字元找斷行機會不經過 ICU4X；paint 在一個迴圈裡放每個 run 的 glyph。

## 詳細設計

### 1. column flexbox 的 min/max-content 寬度

在 `crates/layout/src/lib.rs` 的 `compute_box` 裡 flex 的分支（`compute_flexbox_layout` 之前）攔下，接法和 block 量寬度時的 `measuring_width` 一樣：

- **條件**：`RunMode::ComputeSize`、只要寬度（`RequestedAxis::Horizontal`）、寬度沒有已知的值、可用寬度是 `MinContent`、`MaxContent` 或 `Definite`（第 2 條）、高度已知或未知都算（第 3 條），`flex-direction` 是 `column` 或 `column-reverse`、`flex-wrap: nowrap`。寬度已知的（`width` 有值、`min-width` 和 `max-width` 夾住）taffy 本來就提早結束。
- **算法**（照 Blink 的 fast path）：每個 in-flow 的子項（跳過 out-of-flow 和 `display: none`）量它的寬度貢獻（`compute_child_layout`，`ComputeSize`、`Horizontal`；子項的 `width` 是 `min-content`、`max-content` 時照它量），加上它的左右 margin，取最大。可用寬度是 `MinContent`、`MaxContent` 時照它量；是確定的寬度時，`MinContent` 和 `MaxContent` 各量一次，把可用寬度（扣掉 container 的 padding、border 和捲軸）夾在兩者之間（Blink 的 shrink-to-fit 用 `ComputeMinMaxSizes` 的結果，第 2 條）。再加上 container 的 padding、border 和捲軸的寬度（照 taffy 怎麼算），照 container 的 `min-width`、`max-width` 夾住，回傳寬度、高度 0（taffy 自己只要寬度時也回傳高度 0）。
- **交還給 taffy 的情況**：`flex-wrap: wrap`（Blink 也排，`ComputeMinMaxSizeOfMultilineColumnContainer`）、row 方向（taffy 已經只算主軸就結束）、直接的子項有一個有比例（有自然比例的 replaced 元素；kun 沒有 CSS 的 `aspect-ratio`）：taffy 用 flex 之後的高度換算它的寬度，Blink 的 fast path 不看，交還讓結果和現在一樣（第 4 條）。
- **行內內容只被問寬度時不斷行**（第 7 條）：`compute_inline` 被問 `ComputeSize`、`Horizontal`，裡面又沒有 float、沒有長度的 `text-indent` 時，不在那個寬度斷行，直接回傳已知的寬度、確定寬度下的 fit-content，或 `content_widths` 的 min/max-content 寬度（Blink 的 `InlineNode::ComputeMinMaxSizes` 也不排出行）。column 的快速路徑量 block 子項的寬度時，block 問它的段落寬度，原本每段還是在 min-content 的寬度斷一次行，只為了用不到的高度。
- **debug build 的交叉檢查**：照「快取和增量」的做法，debug build 每次也跑 taffy 的原路，比對寬度一樣（容許 1/64 px）；不一樣時 panic，指出是哪個盒子（第 5 條）。碰到 taffy 已知不合規範的地方不一樣時，停下來決定照 Blink 還是照 taffy，不放寬比對。這也讓現有的 flex 測試和 WPT 的 `css/css-flexbox` 守著它。
- **文件**：`docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」記著：新的 flex 屬性如果讓 column container 的寬度不只看子項的貢獻，要加進交還的條件；行內內容的新輸入如果讓寬度要斷了行才知道，要加進照舊斷行的條件。

### 2. 文字的 fragment 共用 shaping 的結果

原本（`crates/layout/src/text.rs` 的 `fragments` 和 `text_fragment`，`crates/layout/src/fragment.rs` 的 `TextFragment`）每個 run 有自己的 glyph（絕對位置）、cluster 和字型資料，排版時從 parley 的 `positioned_glyphs` 和 `visual_clusters` 做出來。計時（`details.md` 的「文字的 fragment 花在哪」）顯示時間大半在這兩個迭代器，不在配置和複製。

照 Blink（`ShapeResult` 跟著 inline formatting context 留著，行上的 item 是 `ShapeResultView`，只記範圍；第 8 到 11 條）：

- **共用的結果在 fork**：parley 的 `LayoutData` 裡 shaping 的結果是 `Arc`（`Layout::shaped_text`），段落的 shaping 快取存同一份、命中時不複製。重新 build 時別人還拿著就換一份新的；斷行改 tab 的 advance 前 `Arc::make_mut`，只有有 tab、tab 的位置又變了的段落付複製的成本。
- **span**：`GlyphRun::span` 給 `GlyphSpan`（shaped run、cluster 的範圍、方向、那一行的間距）。`GlyphSpan::glyphs` 和 `GlyphRun::glyphs` 走 fork 開放的同一個 `Glyphs`；`GlyphSpan::clusters` 照視覺順序給 cluster 的文字範圍和含間距的寬度，和 `Run::visual_clusters` 走同一個 `AtomGraphemes`；`text_range` 只看字元，不走 cluster。
- **`TextFragment` 的 glyph 是 `TextGlyphs`**：段落的 run 是 `Shaped`（共用的結果、span 和 text node 的 `TextSource`），省略號和連字號自己 shaping 的一個字是 `Owned`。位置相對於 run 的起點，`translate` 只改 `x` 和 `baseline`；`source` 在排版時從 `text_range` 算。
- **glyph 在 paint 時放**：`TextFragment::for_each_glyph`（第 3 節的 3c），`hidden_from` 仍是 glyph 的個數（第 11 條）；沒變的 fragment 沿用 paint 的快取。
- **cluster 和行尾用到時才算**：`clusters()` 第一次問時算、留在 fragment 裡（`OnceLock`），用在 `x_range`（`Range` 的矩形、選取和游標）和 `text-overflow` 的 `truncate_line`。行尾存 `LineEndAt`（`<br>` 的位置，或文字的 run 的 glyph 和起點、終點），`x()`、`kind()` 問的時候才算，`br()`、`text()` 不算；paint 只替有選取的 text node 問。
- **計數**：`LayoutUpdate` 的 `clusters_placed`、`glyphs_placed`，一般的排版是 0，只有 `text-overflow` 截斷的 run 會算。
- fragment 的 `==` 比的是值（glyph、cluster 和 `TextSource` 的內容）：`update` 的測試拿從頭 build 的樹比對，兩棵樹各有一份 `TextSource`。
- SVG 文字和 canvas 文字有自己的型別，不經過 `TextFragment`。

### 3. shaping

`python3 scripts/bench.py speedometer --sample --suites Editor-TipTap`（Windows 上經過 xperf，要系統管理員，UAC 問一次）的結果在 `details.md` 的「TipTap 的取樣」。10 個 iteration 的 949 ms CPU 時間裡，建 box tree 時的 shaping（`InlineContent::new`）佔 434 ms：harfrust 129 ms、parley 的 shaping 簿記 123 ms、選字型 94 ms、icu 的分析 91 ms（其中 ICU 的 segmenter 約 49 ms）。排版的斷行佔 147 ms，其中 `LineState` 的 clone 34 ms；paint 放 glyph 佔 59 ms。fork 的段落 shaping 快取幾乎不命中（「動機」）。

候選和選擇（第 12 條）：

- **S1 字詞的 shaping 快取（做，3a）**：簡單的文字（沒有跨空白的 kerning 或 ligature 的字型、不需要上下文的文字）以字為單位 shaping 並快取，像 Gecko 的 `gfxShapedWord`。長文 39,003 個字只有 9,917 種，第一次 shaping 就約 75% 命中，Highlight 的粗體也一樣。省的是 harfrust、選字型和 shaping 的簿記（346 ms）裡命中的部分。要改 parley fork 的 shaping。
- **S2 比較便宜的分析（只做斷行表，3b）**：U+0000–U+017F 和 U+2000–U+203F 的字元用表找斷行機會，不經過 ICU 的 segmenter，照 Blink 的 `LazyLineBreakIterator::ShouldBreakFast`；不需要時不算字的界線（第 18 條）。不做分析的結果跟著段落快取：段落快取在這裡不命中。
- **S3 溢出時才找緊急斷點（不做）**：照 Blink 的 `LineBreaker`，不在每個字元記緊急斷行機會，一行放不下時才照 `overflow-wrap` 重做（RFC 0058 第 9 條）。最多省 34 ms，要改 parley 斷行的核心。
- **S4 換成 HarfBuzz（不做）**：parley 的 shaping 改用 C++ 的 HarfBuzz（和 Chrome 同一份）。harfrust 是 parley 帶進來的、HarfBuzz 的 Rust 移植，結果一樣，但沒有另外比過速度；harfrust 一共 129 ms，換的代價是多一個要 build 的原生依賴、改 fork 的 shaping 介面、fork 更難跟上游 rebase。
- **paint 放 glyph 的迴圈（做，3c）**：第 2 段把放 glyph 搬到 paint 之後，paint 每一步走過每個 run 的 glyph，59 ms 裡有 43 ms 是 `Take<Chain<FlatMap…>>` 這層迭代器本身；改成直接的迴圈。paint 的 cull rect 不在這份 RFC（「之後的可能」）。

做出來的（細節在第 13 到 19 條）：

- **字詞的 shaping（3a）**：fork 的 `Shaper` 在空白前後把 segment 切成字（空白後面接組合字元時不切），32 字以內的字照它的文字、每個字元選字型用的樣式（parley 的 `ShapeCache::font_styles` 給號碼）、script 和 item 的選項，存下選到的字型和 harfrust 的輸出（字型單位，`shape/words.rs`）。命中時不選字型、不 shaping，同一個字型的字併成一個 `ShapedRun`，和整段 shaping 一樣轉換，結果逐位相同。只在這樣和整段的結果一樣時做：LTR、不是 Arabic 和 Syriac、沒有預設關掉的 feature，而且字型的規則最多把空白和旁邊一個 glyph 一起比對（`shape/space.rs` 的 `space_reach`，每個字型判斷一次）。只到旁邊的字型（Arial、Times New Roman）記下那些 glyph，空白旁邊實際是它們的邊界把前後的字併成一個單位重新 shaping（positioning 併一側、substitution 併兩側）；比對得更遠的（Calibri、kun 的測試字型 Lato）整段。上限 2^15 個字，選項和樣式的號碼各 2^12 個，用完時連同字一起清；註冊字型時清掉（`LayoutContext::clear_shapings`）。fontique 清掉的字型保留弱參照，還有人拿著時重新載入是同一個 blob。debug build 用到字的 layout 再整段 shaping 一次比對。
- **斷行表和字的界線（3b）**：fork 的 `parley_engine/src/line_table.rs` 把 U+0000–U+017F 和 U+2000–U+203F 的字元分成 22 個 UAX #14 的類別，照空白前面最後一個字元的類別（或文字的開頭）、中間有沒有空白、這個字元的類別，查「不斷、斷、決定不了」；表從 ICU4X 產生，測試確認提交的表沒有過時。`analyze_text` 在整段都是預設的斷行規則時查表；字裡有表外的字元或決定不了的格時，那個字連同前後的字交給 ICU4X，只取那個字的位置；debug build 每段再整段跑 ICU4X 比對。`AnalysisOptions::skip_word_boundaries`（parley 的 `LayoutContext::set_word_boundaries`）不算字的界線，kun 的 `TextContext` 關掉：編輯和 `capitalize` 自己用 `WordSegmenter`，網址列的 `PlainEditor` 照樣算。
- **paint 放 glyph（3c）**：`TextFragment::for_each_glyph` 兩種來源各一個 `for` 迴圈，把每個 glyph 的 id 和在 baseline 上的位置交給呼叫方；`paint` 的 `text_run` 在同一個迴圈裡寫進 `Painter` 重用的 `Vec<u16>`、`Vec<PointF>`，再各轉成一個 `Arc`。`glyphs()` 用它實作，給 `text-overflow` 和測試。fork 的 `GlyphSpan` 自己的迴圈量了反而慢，沒有留（第 19 條）。

### 測試

每一項怎麼驗證在 `progress.md` 的「驗收」。

- **第 1 段**：`layout` 的 `flex::`（column 的寬度、子項自己的寬度、交還 taffy 的情況、類似 TipTap 的頁面每段只斷一次行）和 `inline::a_width_alone_breaks_no_lines`；`LayoutUpdate::lines_broken` 數真的斷行（第 6 條）。debug build 每次比對 taffy 的寬度，讓現有的 flex 測試、`test.py reftest`、WPT 的 `css/css-flexbox/` 都守著它。
- **第 2 段**：fork 的改動由 `layout` 的 `parley_shaped_text::` 測；`text_overflow::`、`update::`、`inline::how_lines_end` 和 `paint` 的 `paint_controller::only_selected_text_has_its_clusters_worked_out` 守著位置和計數；現有的文字、paint、hit test、選取的測試、文字的 reftest 和 WPT 的 `css/css-text/` 守著行為不變。
- **第 3 段**：`layout` 的 `parley_words::`（系統字型也和整段 shaping 逐位比對）、`parley_space::`（`write-fonts` 造的字型，第 14 條）、`parley_line_breaks::`，fork 的 `line_table.rs` 的測試和 fontique 的 `source_cache::tests`；`paint` 的 `artifact::what_text_overflow_hides_is_not_painted`。debug build 的兩種比對（整段 shaping、整段 ICU4X）讓每個有文字的測試都守著。
- 每段都跑 mutation test（`mutants-*.toml`）。效果用計數和暫時的計時確認；做完之後照使用者的要求量了 Speedometer。

### 分階段

1. **column flexbox 的 min/max-content 寬度**（「詳細設計」第 1 節）。只改 `layout`。
2. **fragment 共用 shaping 的結果**（第 2 節）。改 parley fork、`layout`、`paint`，可能還有 `gfx` 的 glyph run。分三段（第 9 條）：
   - 2a：fork 共用 shaping 的結果、tab 用 `Arc::make_mut`、開放 `Glyphs`；kun 改用新的 API，行為不變。
   - 2b：`TextFragment` 改形狀，排版時不走 glyph 和 cluster，cluster 用到時才算（fork 的 `GlyphSpan` 給 cluster 和文字範圍）；paint 從共用的結果算 glyph 的位置（第 10 條）。
   - 2c：計時（layout 和 paint，和 2a 對照）、文件（第 11 條）。
3. **shaping**（第 3 節）：取樣之後分三段（第 12 條）：
   - 3a：字詞的 shaping 快取（S1），改 parley fork 的 shaping。
   - 3b：U+0000–U+017F 和 U+2000–U+203F 的字元查表找斷行機會（S2 的一部分）、不需要時不算字的界線（第 18 條），改 fork 的分析。
   - 3c：paint 放 glyph 改成直接的迴圈，改 `layout` 的 `TextFragment::glyphs`（加 `for_each_glyph`）和 `paint` 的 `text_run`（重用的 buffer）。

三段都做完，2026-10-24 一起 merge 回 `main`。

## 缺點

- 第 1 段多一條和 taffy 並行的路：taffy 之後改了 column 的 intrinsic size，或 kun 加了新的 flex 屬性，兩邊要一起看；debug build 的交叉檢查讓 debug 的 layout 慢一些。
- 第 2 段改了 parley fork，讓 shaping 的結果可以共用；用到 glyph 和 cluster 的地方都跟著 `TextFragment` 改了形狀。glyph 的位置移到 paint 放：paint 沒有 cull rect，變了的 fragment 一樣每個 glyph 走一遍（3c 的迴圈讓長文的 paint 一步從 5.7 ms 降到 1.9 ms）。
- 第 3 段的字詞快取和斷行表都改 parley fork 的核心（shaping 和分析），fork 和上游的差異變多，rebase 更難。字詞快取多一份要守著的結果：判斷字型的規則碰不碰到空白錯了時，畫出來的字就不同（debug build 的比對和 `parley_space` 守著）；斷行表要跟著 ICU4X 的版本重新產生（過時時測試失敗，印出新的表）。

## 理由和其他做法

- **為什麼從這三件做**：計數顯示每一步的 44 ms 裡，多斷的那一次約 6 ms、做 fragment 約 10 ms 是 Blink 不做的工作，拿掉它們不改變結果；shaping 的 24 ms 是 Chrome 也要做的，差在每個字的成本，要先取樣才知道該改哪裡。
- **第 1 段改 taffy 本身**（fork 或送上游）：RFC 0003 第 3 條決定不 fork taffy；在 kun 的 dispatch 攔下和現有的 `measuring_width` 一樣。之後可以把問題寫成上游的 issue 草稿（「之後的可能」）。
- **第 1 段只記住窄寬度的斷行結果**：多斷的那一次還是會做一次（每段 5 ms 左右），只是不重複，不如不做。
- **第 2 段只拿掉配置**（字型的 `Arc` 共用、`Vec` 預留大小）：計時顯示配置和複製幾乎不花時間，省不到什麼。
- **第 2 段照舊在排版時算，只把迭代改快**（直接走 parley 的 atom，cluster 對到 UTF-16 時少查表）：fork 不用動，但每次排版還是每個 glyph、cluster 走一遍（第 8 條）。
- **不做**：TipTap 繼續是文字差距最大的 suite，而第 1 段的問題在每個 column flexbox 裡都有。

## 先例

- **Blink**：
  - `FlexLayoutAlgorithm::ComputeMinMaxSizes`：非換行的 column container 只取子項的 `ComputeMinAndMaxContentContribution`（註解寫的是 "a fast-path for performance reasons"）；換行的 column 才排（`ComputeMinMaxSizeOfMultilineColumnContainer`）。第 1 段照做。
  - `ShapeResultView`：行上的文字 item 指向 `ShapeResult` 的一段。第 2 段照做。
  - `NGShapeCache`：每個字型一份，只收 30 字以內的文字、最多 2048 筆；長段落不快取。`ReusingTextShaper` 在重新收集時沿用沒變的 `ShapeResult`。
  - `LineBreaker`：不在每個字元記緊急斷點，溢出時照 `overflow-wrap` 重做。第 3 段的 S3（不做）。
  - `LazyLineBreakIterator::ShouldBreakFast`（`text_break_iterator.cc`）：兩個字元都在快速的表裡（ASCII 到 U+00AD）時查表，其他才交給 ICU。第 3 段的 3b 照做（表到 U+017F 加上 U+2000–U+203F、從 ICU4X 產生）。
- **Gecko**：`gfxShapedWord`，每個字型的字詞 shaping 快取；字型的 lookup 用到空白時那段不用它。第 3 段的 3a 照做。
- **kun 自己**：RFC 0051 第 27 條、RFC 0058 的 1c、2c 和第 9 條（「動機」）。

## 已決定的事項

1. **範圍和分階段**（2026-10-23，使用者確認）：一份 RFC、三個階段，依序是 column flexbox 的 min/max-content 寬度不排子項、fragment 共用 shaping 的 glyph、shaping 先取樣再決定。不做增量的 reshaping 和行的重用（TipTap 用不上）。
2. **第 1 段：三種可用寬度都走**（2026-10-23）：`MinContent`、`MaxContent` 和 `Definite`。`MinContent`、`MaxContent` 用同樣的可用寬度量子項的貢獻再取最大；確定的寬度照 Blink，先取子項 min-content 和 max-content 貢獻各自的最大，再把可用寬度夾在中間（shrink-to-fit 用 `ComputeMinMaxSizes` 的結果）。原本打算在確定的寬度下也直接用那個寬度量子項（每個子項各自 fit-content 再取最大，對一般的內容和夾住算出來一樣），實作時發現可以壓縮的 replaced 元素不一樣：`width` 是百分比的圖片 min-content 貢獻是 0，在確定的寬度下量卻是它自己的寬度，column 會被撐開（現有的 SVG 測試抓到；taffy 經由 flex 之後的高度和比例算出和 Blink 一樣的結果）。確定的寬度因此多量一次。不選只做 `MinContent`、`MaxContent`：TipTap、NewsSite-Next、Nuxt 各跑一個 iteration，column flexbox 被問寬度的 21,558 次裡有 1,694 次是 `Definite`（`details.md` 的「column flexbox 被問寬度的次數」）。
3. **第 1 段：container 的高度已知時也走**（2026-10-23）：照 Blink 的 fast path，不看高度。高度已知時 taffy 把子項 flex 之後的高度傳下去，只有有比例的子項的寬度因此改變，那種交還（第 4 條）。高度已知的佔 7,618 次，不做的話省不到一半。
4. **第 1 段：有比例的子項交還 taffy**（2026-10-23）：直接的子項有 `aspect-ratio` 或自然比例時，整個 container 走 taffy 的原路，結果和現在一樣；換行的 column 也交還（Blink 也排）。不照 Blink 用貢獻：兩邊的結果會不同，三個 suite 裡也沒有碰到。
5. **第 1 段：debug build 比對 taffy，不一樣就 panic**（2026-10-23）：容許 1/64 px，指出是哪個盒子；現有的 flex 測試、reftest 和 WPT 的 `css/css-flexbox/` 都因此守著它。不一樣的地方是 taffy 已知不合規範的時，停下來決定照 Blink 還是照 taffy，不放寬比對。
6. **第 1 段：`LayoutUpdate` 加真的斷行的次數**（2026-10-23）：給測試和 `RUST_LOG=layout=debug`，和 `laid_out`、`fragments` 同一種計數。不選在 `TextContext` 加只給測試的計數：layout 的計數本來就在 `LayoutUpdate`。
7. **第 1 段：行內內容只被問寬度、沒有 float 時不斷行**（2026-10-23，使用者確認）：直接回傳已知的寬度、確定寬度下的 fit-content，或 min/max-content 寬度，對所有問寬度的地方（block 的 shrink-to-fit、flex、grid）都一樣；有 float 時照舊斷行，放 float 要行；`text-indent` 有長度時也照舊，parley 的 `calculate_content_widths` 不算縮排（`width: max-content` 加 10px 的縮排會少 10px），只有斷出來的行算進去。改了之後 `width` 是百分比的 inline-block 的 min-content 貢獻照它的內容算（css-sizing 3 的 cyclic percentage，和 Chrome 一樣；原本是在 min-content 寬度斷行時的百分比）。實作時發現：column 的快速路徑接上之後，類似 TipTap 的頁面每段還是斷兩次行，多的那次在行內內容的 measure：只被問寬度也在 min-content 寬度斷行，算出用不到的高度；改了之後每段一次。照 Blink 的 `InlineNode::ComputeMinMaxSizes`（不排出行）。不選只在 column 的快速路徑量子項時不斷行（像 `measuring_width` 的旗標）：別處問寬度時一樣多斷一次。這一層沒有 debug build 的比對：要比就得真的斷一次行，會改掉斷行的記憶和行，debug 和 release 的計數會不同；由測試、reftest 和 WPT 守著。
8. **第 2 段：照 Blink 共用 shaping 的結果，glyph 和 cluster 的位置用到時才算**（2026-10-24，使用者確認）：行上的 run 指向段落共用、不再改動的 shaping 結果的一段，只存範圍、起點和那一行的間距；glyph 的位置在 paint 時算，cluster 的位置在選取、游標、`Range` 的矩形、`text-overflow` 和行尾要用時才算（Blink 的 `ShapeResultView`）。開始時的計時（`details.md` 的「文字的 fragment 花在哪」）顯示做 fragment 的 12–13 ms 大半是 parley 的兩個迭代器（cluster 約 7 ms、glyph 約 3 ms），配置和複製幾乎不花時間，原本「只拿掉複製」的方向省不到什麼。不選照舊在排版時算、只把迭代改快（直接走 parley 的 atom，cluster 對到 UTF-16 時少查表）：改動小、fork 不用動，但每次排版還是每個 glyph、cluster 走一遍；共用之後排版完全不碰它們。代價是 fork 要讓 shaping 的結果可以共用、`TextFragment` 和用到它的地方都要改，glyph 的成本移到 paint：paint 沒有 cull rect，變了的 fragment 一樣每個 glyph 算一次（沒變的沿用 paint 的快取），所以這一段省下的主要是 cluster 的部分；paint 的 cull 是另一件事。
9. **第 2 段的細節**（2026-10-24，使用者確認）：
   - **共用的結果放在 fork**：parley 的 `LayoutData` 裡 shaping 的結果改成 `Arc`，有 public 的取用方法，fragment 拿同一份；fork 的 shaping 快取也存同一份，命中時不再複製。不選 kun 在 shaping 之後自己複製一份：每次 shaping 多複製約 23 萬個 glyph，算 glyph 位置的邏輯（間距、justify、RTL）kun 要再寫一份。Blink 的 `ShapeResult` 也是 ref-counted，`InlineItem` 和 `ShapeResultView` 拿同一份。
   - **tab 的寬度用 `Arc::make_mut`**：斷行改 tab 的 advance 時，舊的 fragment 還拿著那份結果就先複製整份再改；只有有 tab、tab 的位置又變了的段落付這個成本，fork 改一行。不選每行另外記 tab 的寬度：parley 的斷行、對齊、cluster、glyph 都讀 cluster 的 advance，每處都要加。Blink 在斷行時替 tab 另外做一份 `ShapeResult`（`CreateForTabulationCharacters`），不改共用的那份，效果一樣。
   - **glyph 的位置由 fork 算**：fork 把 `Glyphs`（`Run::glyphs_in` 用的）開放成 public，給共用的結果、cluster 的範圍、方向和間距（run 的 letter、word spacing 加上那一行 justify 的值），斷行、對齊和 paint 用同一份程式。不選 kun 自己寫：兩份算法會漂開。Blink 的 `ShapeResultView::ForEachGlyph`。
   - **`TextFragment` 的 glyph 是 enum**：段落的 run 指向共用的結果（run、cluster 的範圍、間距），省略號和連字號自己 shaping 的一個字存自己的 glyph；位置相對於 run 的起點，`translate` 只改 `x` 和 `baseline`；`hidden_from` 從 glyph 的索引改成 cluster 的界線。不選省略號和連字號也共用：只有一個字，沒有好處。
   - **cluster 第一次用到時算、留在 fragment 裡**（`OnceLock`）：拖曳選取時反覆問同一個 run。不選每次重算：Blink 每次從 `ShapeResult` 查，但它有 cluster 的索引表，kun 沒有。
   - **計數守著**：`LayoutUpdate` 加排版時算出的 glyph 和 cluster 的個數，測試確認一般的排版是 0；另外照「文字的 fragment 花在哪」的做法暫時計時一次，寫進 `details.md`，不量 Speedometer。
   - **分三段**：2a fork 共用 `Arc`、tab 用 `make_mut`、開放 `Glyphs`，kun 改用新的 API、行為不變；2b `TextFragment` 改形狀，排版不走 glyph 和 cluster，cluster 用到時才算；2c paint 從共用的結果算 glyph 的位置、計時、文件。每段跑 mutation test，第 2 段做完、審核過才 merge。
10. **第 2b 段：cluster 由 fork 算，paint 併進 2b**（2026-10-24，使用者確認）：
    - fork 加 `GlyphSpan` 的 cluster：照視覺順序給每個 cluster 的文字範圍和寬度，間距（letter、word spacing、justify）和 parley 自己的 cluster 用同一份程式；span 也直接給它的文字範圍，fragment 的 `source` 在排版時不走 cluster。不選 kun 自己走 parley_engine 的 atom 和 grapheme：fork 一樣要開放間距的型別，算間距的邏輯又寫兩份。Blink 也從 `ShapeResultView` 查 cluster 的位置。
    - paint 的 `text_run` 在 2b 改成從共用的結果算 glyph：`TextFragment` 不存 glyph 之後，paint 不改就編不過；不選先留一個轉換的方法，那只是之後要拆的一層。2c 只剩計時和文件。
11. **第 2b 段和第 9 條不同的兩處，和第 2c 段**（2026-10-24，使用者確認）：
    - **`hidden_from` 仍然是 glyph 的個數**，不照第 9 條改成 cluster 的界線：截斷的地方本來就取 cluster 的起點，glyph 的個數用同一個界線數出來，結果一樣；改成 cluster 的界線，paint 畫每個截斷的 run 時反而要算 cluster。
    - **行尾（`LineEnd`）也在用到時才算**：每一行都有行尾，原本做行尾時就要那個 run 最後的 cluster，等於每個 run 都算。改成存 run 的 glyph 和位置，`x()`、`kind()` 問的時候才算，`br()`、`text()` 不算；paint 只替有選取的 text node 問。不選只對行尾那個 run 先算 cluster：還是每一行算一次。Blink 的行尾（`LineInfo` 的 `HasForcedBreak`、`InlineCursor` 的選取）也在用到時從 `ShapeResultView` 查。
    - **計時量 layout 和 paint，和 2a 對照**：glyph 的位置從 layout 搬到 paint，paint 每一步都要放 glyph；只量 layout 看不出淨效果。照「文字的 fragment 花在哪」的做法（release、`longtext.html`、800 和 820 px 交替），2a 的版本另開 worktree 編；不量 Speedometer。
    - **文件**：`docs/architecture.md`（文字的 fragment 是共用結果的一段，glyph 和 cluster 用到時才算；「快取和增量」加一條：要讀 cluster 的新程式用 `clusters()`，只在需要時讀；「和 Chromium 刻意不同的地方」加一條：cluster 算出來留在 fragment 裡，Blink 每次從 `ShapeResult` 查）、`details.md`（計時）。`layout` 沒有 README：`TextFragment`、`LineEnd` 的方法說明在程式的註解裡（2b 已寫）。

12. **第 3 段：做字詞的 shaping 快取和 ASCII 的斷行表，加上 paint 放 glyph 的迴圈**（2026-10-24，使用者確認）：取樣（`details.md` 的「TipTap 的取樣」）之後：
    - **S1 字詞的 shaping 快取**：harfrust、選字型和 shaping 的簿記加起來 346 ms，長文 39,003 個字只有 9,917 種，第一次 shaping 就約 75% 命中，之後的 iteration 幾乎全中；預估省 250–300 ms。記憶體跟著字彙量，不跟著文字長度，所以不用為了 Speedometer 加大段落的快取（「動機」的「不在範圍內」）。照 Gecko 的 `gfxShapedWord`。Blink 只有 `NGShapeCache`（30 字以內的整個 item），長段落每次整段 shaping：這裡和 Chromium 不同，3a 做完時寫進 `docs/architecture.md` 的「和 Chromium 刻意不同的地方」。代價是 fork 的 shaping 改動大、更難 rebase，判斷能不能以字為單位錯了時畫出來的字不同。
    - **S2 只做 ASCII 的斷行表**：icu 的分析 91 ms，其中 ICU 的 segmenter 約 49 ms。只有 ASCII 的文字查表，照 Blink 的 `ShouldBreakFast`；改動只在 fork 的分析。不選分析的結果跟著段落快取：段落快取在這裡不命中。
    - **paint 放 glyph 的迴圈**：第 2 段把放 glyph 搬到 paint 的成本（59 ms，其中 43 ms 是迭代器本身），由這份 RFC 收尾。不做 paint 的 cull rect：根本的原因是 paint 每一步走過整份長文，但 cull rect 牽動 paint 的快取和捲動，是一份 RFC 的量；記進 `docs/limitations.md`。
    - **不選 S3**：`LineState` 的 clone 最多 34 ms（3.5%），要改 parley 斷行的核心。
    - **不選 S4**：harfrust 129 ms，就算 HarfBuzz 快三成也只省約 40 ms；代價是多一個原生依賴、改 fork 的 shaping 介面、fork 更難 rebase。做了 S1 之後 harfrust 只剩沒命中的字。
    - **kun 自己的 `layout::text`（106 ms）留到之後**：分散在 `InlineContent::new` 本身（31 ms）、`content_widths`（25 ms）、`fragments`（12 ms）等處，沒有一個明確的原因，要先另外拆；S1、S2 做完之後比例會變，到時候（使用者要求時）重新取樣再決定（「之後的可能」）。
    - **分三段**：3a 字詞快取、3b ASCII 的斷行表、3c paint 的迴圈；每段跑 mutation test，效果用計數確認。

13. **第 3a 段：字詞快取的細節**（2026-10-24，使用者確認）：
    - **放在選字型之前**：key 是字、segment 的 script、item 的 shaping 選項（大小、locale、feature、variation、optical sizing），和每個字元選字型用的樣式（字型堆疊、width、weight、style，parley 給一個號碼）；命中時選字型、harfrust 和轉換都省。Blink 的 `NGShapeCache` 也以整個 `Font`（含 fallback）為 key。不選照 Gecko 放在選字型之後、以字型為 key：每個字元照樣選字型，94 ms 省不到。代價是一個字用到好幾個字型時要存成好幾段，註冊字型時要清掉。
    - **在 U+0020 切**：空白本身也是一個字；空白後面接著組合字元時（同一個 grapheme）不切。只快取 32 字以內的字；只做 LTR 的 segment，RTL 的整段 shaping。
    - **每個字型判斷一次空白會不會參與 shaping**：用 `read-fonts` 走 GSUB、GPOS 的每一個 lookup（不只預設 feature 的）和 `kern` 表，空白的 glyph 在 coverage、class、序列、pair 裡，或字型有 AAT 的表時，那個 segment 整段 shaping。照 Gecko 的 `CheckForFeaturesInvolvingSpace`；harfrust 沒有 `collect_glyphs`，fork 自己寫。
    - **存 harfrust 的輸出**（glyph id、字型單位的位置、cluster 相對於字的開頭）和每段的字型：命中時接上去，相鄰而且同一個字型的字併成一個 `ShapedRun`，再和整段 shaping 一樣轉成 cluster，結構和數值都和整段 shaping 一樣。原本打算存轉換之後的 cluster 和 glyph，但轉換要用到每個字元的分析（斷行機會、空白的種類），那是段落的，不能跟著字存；存字型單位的值，乘上大小的結果也和整段的逐位相同。
    - **上限 2^15 個字**，滿了整個清掉；註冊字型時和段落的快取一起清。Blink 每個字型 2048 筆、滿了只查不加；Gecko 用計時器讓字過期。
    - **段落的快取照舊**：先查段落的，沒命中才以字為單位。
    - **debug build 的比對**：用到字詞快取的 layout 再整段 shaping 一次，和段落快取一樣用 `same_shaping` 比對，不一樣就 panic。
    - **計數**：fork 數命中的和 shaping 的字，kun 的 `TextContext` 給測試讀。

14. **第 3a 段：空白檢查的分支在測試裡造字型來測**（2026-10-24，使用者確認）：用 `write-fonts`（fontations 的，和 `skrifa` 同一組）造只有一兩條 lookup 的小字型，每個分支各測一次。可以帶著的測試字型（WPT、kun 的）裡沒有只用到一個分支的，9 條 mutation 沒抓到，「`kern` 表不算」只靠這台機器的系統字型抓到。不選記進 `docs/test-gaps.md` 只靠審查：這段判斷錯了，畫面上只差零點幾個 px，不容易被別的測試發現。原本寫加在 fork 的 `space.rs` 的測試裡；實作時放在 kun 的 `layout` 整合測試（`parley_space`，`write-fonts` 是 `layout` 的 dev-dependency），照「改 fork 的照 fork 的規則在 kun 裡測」，fork 的 `Cargo.toml` 不動；只影響測試的 build。

15. **第 3a 段：Arial 這類字型每個空白邊界看兩邊實際的 glyph**（2026-10-24，使用者確認）：字型的 lookup 只把空白和它旁邊的一個 glyph 一起比對時，照舊以字為單位 shaping，只在空白旁邊實際是那些 glyph 的邊界，把前後的字併成一個單位重新 shaping（精確，和整段 shaping 逐位相同）；比對得更遠的字型照舊整段。這台 Windows 上 Arial、Times New Roman 因此以字為單位（長文的空白邊界裡要併的約 0.4%），Calibri 和 kun 的測試字型 Lato（`calt` 跨空白）照舊整段。不選照舊整段：Windows 上 TipTap 落到 Arial，字的快取沒有作用。不選照 Gecko 在 `font-kerning: auto` 時忽略和空白的 kerning：有損（16 px 的 Arial 每對差約 0.9 px），Arial 也仍然因為 `ccmp` 過不了。Chrome 的 LayoutNG 沒有字的快取（整個 item shaping，30 字以內的 item 進 `NGShapeCache`，編輯時用 `ReusingTextShaper` 沿用 unsafe-to-break 之間的結果）；舊的 `CachingWordShaper` 以字型為單位判斷，和照舊整段一樣。代價是 fork 的空白判斷從「用不用到」變成記下用到的 glyph，多一層要守著的邏輯；細節在第 17 條。

16. **第 3a 段和第 13 條不同、第 13 條沒寫到的地方**（2026-10-24，使用者確認）：
    - **Arabic 和 Syriac 的 segment 也整段 shaping**，不只 RTL：harfrust 只有 Arabic shaper（只用在這兩種 script）會看 run 前後的文字（pre-context、post-context），字單獨 shaping 時前後的文字不同；其他 shaper 都不看。這兩種 script 平常是 RTL，只有 `bidi-override` 之類讓它們從左到右時會走到。
    - **item 有預設關掉的 feature（`dlig`、`smcp`、`ss01` 這類）時整段 shaping**：空白的判斷每個字型只做一次，只走可能開著的 feature 的 lookup（HarfBuzz 預設就開、或依 script 開的都算），不跟著 item 的 feature 重做。
    - **空白的判斷怎麼算「用到空白」**：lookup 把空白和它前面的 glyph、或後面可能不是組合字元的 glyph 一起比對（ligature、pair、context 的序列）才算；只改空白本身的（single substitution、single positioning）不算，空白後面接的組合字元和它在同一個字裡。跳過 base glyph 的 lookup、cursive attachment、空白當成 mark、`kern` 表的 class 和 state machine 的格式、讀不了的表都當成用到。這台 Windows 的系統字型加上 WPT 和 kun 的測試字型共 156 個檔，94 個不用到（Verdana、Segoe UI、Tahoma、Georgia、Courier New、中日韓的字型）。用到的再照第 15 條分：只到空白旁邊的 glyph 的（Arial、Times New Roman、Mongolian Baiti、WPT 的 Exo2 和 Noto Sans Devanagari）以字為單位，其他（Calibri、kun 的測試字型 Lato）照舊整段。
    - **碰到用到空白的字型時**，那個 segment 已經 shaping 的部分丟掉，整段重來；途中存下的字留著（裡面只有不用到空白的字型）。計數只算成功逐字的 segment。
    - **長於 32 字的字照樣逐字 shaping**，只是不存：字的界線兩邊都是空白，結果和整段一樣。
    - **選項和字型樣式的號碼各最多 2^12 個**，用完時連同存的字一起清掉：號碼重新發，不能有用舊號碼存的字被新的選項或樣式命中。
    - **parley 的 `FontSelector` 建立時重設 query 的屬性**：debug build 用同一個 query 再建一個 selector 整段 shaping 比對時，query 還留著前一個 selector 設的粗細，新的 selector 以為是預設，粗體的字選到一般的字型，比對失敗。段落快取原本的 debug 比對也有同樣的問題，只是測試沒踩到。
    - **註冊字型時清掉**：kun 只有測試的 `register_fonts_for_testing` 加字型，在那裡呼叫新的 `LayoutContext::clear_shapings`，段落的快取和字一起清（段落的快取原本沒清）。
    - **fontique 清掉的字型保留弱參照**：`FontContext` 每次 layout 之後清掉 128 次沒用到的字型，重新載入同一個檔案時原本給新的 blob（新的 id），存著的字拿的是舊的，和新的 run 合不起來（長文用 Verdana 時 debug 比對 panic）。改成清掉時保留弱參照，還有人拿著就用同一個 blob；記憶體裡也只有一份。

17. **第 15 條的細節**（2026-10-24，使用者確認）：
    - **規則只有兩格（空白和旁邊的一個 glyph）才算「只到旁邊」**：三格以上有空白的規則、class 0 又沒有 coverage 限制的（等於幾乎所有 glyph）、上面那些當成用到的情形，都照舊整段。
    - **positioning 和 substitution 分開記**：positioning（GPOS 的 pair、context，和 `kern` 表）只改位置，只把空白和那一側的字併起來；substitution 可能把空白換成別的 glyph，那個 glyph 又可能和另一側的字一起比對，所以兩側的字都併（空白在後面時併前一個字和下一個字；在前面時併前兩個字）。
    - **substitution 會變出的 glyph 也算**：記下的 glyph 加上之後的 substitution（feature 的和 context 套用的）會把它們變成的 glyph，一直到不再變多；空白本身只加 feature 直接套用的 single、multiple、alternate、reverse 變出的 glyph，不跟 ligature、不跟只在 context 裡套用的 lookup（Arial 和 Segoe UI 的 `ccmp` 有空白加 mark 的 ligature 和 context 裡的 single substitution，跟著它們會把整套附加符號都當成空白，判成整段）。空白開頭的 ligature 另外記一個旗標：那種字型裡帶組合字元的空白兩側都併。positioning 看最後的 glyph，不加。
    - **跳過 mark 的 lookup**：比對空白和前一個 glyph 時跳過 mark（`kern` 表一律跳過，harfrust 和 HarfBuzz 一樣）的，字的最後一個 grapheme 有好幾個字元時，兩側都併；GDEF 標成 mark 的 glyph 也加進前面那一側（substitution 從一個字元變出的 mark）。
    - **一側最多 256 個 glyph**（不算 mark），多了照舊整段：幾乎每個邊界都要併時，以字為單位沒有好處。Arial 前面那一側 344 個裡大多是 mark。
    - **併起來重新 shaping 的單位不存**：它跨了兩三個字，下一次以同樣的字出現時照樣從存著的字併；計數另外數併的次數。
    - **不知道的字型照最保守的併**：存著的字的字型一定判斷過；萬一沒有，併前兩個字和下一個字。
    - **測試的字型在 kun 造**（第 14 條）：每個分支一兩條規則，每段文字都和整段 shaping 逐位比對，並數併的次數。

18. **第 3b 段：斷行表和字的界線**（2026-10-24，使用者確認）：開始前量的（release、長文分析一次 5.8 ms）：斷行機會 29%、字的界線 23%、grapheme 17%，其他是查字元的屬性和組 `CharInfo`。長文 196 段只有 11 段全是 ASCII（法文的 é、’ 幾乎每段都有），兩個相鄰字元都在 ASCII 到 U+00AD 的佔 92%。
    - **範圍：斷行表，加上不需要時不算字的界線**：kun 不讀 parley 的字的界線（編輯和 `capitalize` 需要時自己用 `WordSegmenter`），預估省分析的一半左右。grapheme 照舊：shaping 和游標都用它，只佔 17%，要和 ICU 一致得另外寫一套規則。Blink 排版時也只做斷行（`LazyLineBreakIterator`），字和 grapheme 的界線用到時才問 ICU。改了第 12 條的「只做 ASCII 的斷行表」。
    - **表從 ICU4X 產生，結果和現在一樣**：fork 的測試用 ICU4X 產生表、確認提交的表沒有過時，並把短字串窮舉、長字串隨機地和 ICU4X 比對；表記著空白前面最後一個字元，跨空白的規則（LB14–17）照樣查表；只看前後一個字元不夠的（LB25 的數字之類），那個字交給 ICU4X。debug build 每段再整段跑一次 ICU4X 比對，不一樣就 panic。不選照抄 Blink 的表（`ShouldBreakFast`）：Blink 在幾個 ASCII 的情況和 ICU 不同，結果會變。
    - **表到 U+00FF（Latin-1）**：含表外字元的字佔長文字元的 11%（到 U+00AD 時 29%）。字元先對到斷行的類別，再查類別對類別的表。Blink 的表到 U+00AD。（後來擴大，見下一點。）
    - **表擴到 U+0000–U+017F 和 U+2000–U+203F**（2026-10-24，使用者確認）：到 U+00FF 做好後量的（release、長文合併空白後 225 KB）：全部用 ICU4X 1.53 ms，表到 U+00FF 0.88 ms，交給 ICU4X 的字 2774 個（5.3%），其中 2589 個是因為 `’`（U+2019），其餘是 `—` 和 `œ`；把這幾個換成表內的字元後 0.47 ms、沒有字交給 ICU4X。英文網頁的 `’ “ ” —` 也一樣。擴大的兩段：Latin Extended-A（UTF-8 仍是兩個 byte）、General Punctuation 的前 64 個（`‘ ’ “ ” – — … ‹ ›`、窄不換行空白 U+202F、`‰`；UTF-8 是 `E2 80` 開頭的三個 byte）。代價是多 B2、IN、NS 等幾個類別和一種三個 byte 的解碼；這段裡的 ZW、ZWJ、U+2028/2029 照樣交給 ICU4X；產生和驗證的做法不變。不選維持到 U+00FF：一般西文網頁最常見的標點都在表外，大約一成的字仍要交給 ICU4X。Blink 的表只到 U+00AD。
    - **表外的以字為單位交給 ICU4X**：以 U+0020 分開的字裡有表外的字元或要上下文的位置時，那個字連同前後的字交給 ICU4X，只取那個字的結果。ICU4X 不能從文字中間開始（沒有 Blink 用的 `following()`），也不選有表外字元就整段交給 ICU4X：長文只有 18 段走得到。計數數交給 ICU4X 的字，給測試讀。
    - **字的界線用開關**：fork 的 `AnalysisOptions` 加一個欄位，預設照舊算，kun 關掉；parley 自己的游標和選取照樣能用。不選整個拿掉：和上游的差異更大。

19. **第 3c 段：paint 放 glyph 的迴圈**（2026-10-24，使用者確認）：paint 每個 run 走 `TextFragment::glyphs`（`Take<Chain<FlatMap<…>>>`，取樣 59 ms 裡 43 ms 是這層迭代器本身、16 ms 是 parley 的 `Glyphs` 和位置的 `map`），先 collect 成 `Vec<(u32, PointF)>`，再各配置一次 glyph id 和位置的 `Arc`。
    - **迴圈：kun 的內部迭代，加上 fork 的 `GlyphSpan` 的迴圈，後者量過值得才留**：kun 的 `TextFragment` 改成 `for_each_glyph`（兩種來源各一個 `for` 迴圈，`hidden_from` 是上限），paint 在同一個迴圈裡寫 glyph id 和位置；`glyphs()` 用它實作，留給 `text-overflow` 和測試。fork 的 `GlyphSpan` 加自己的迴圈，沒有間距時直接走 cluster 和 glyph 陣列，不經 `Glyphs` 每個 glyph 的狀態判斷；量了省得不多就不留，fork 少一個修改。不選 `DrawGlyphs` 存 span、raster 時才放：牽動 display item 的比較、paint 的快取和 raster。Blink 的 `ShapeResultBloberizer` 用 `ShapeResult::ForEachGlyph` 直接寫進 `SkTextBlobBuilder` 的 buffer。
    - **配置：`Painter` 重用的 buffer**：迴圈寫進重用的 `Vec<u16>`、`Vec<PointF>`，再 `Arc::from(&scratch[..])`，每個 run 兩次配置。不選 collect 成 `Vec` 再轉：多一次複製和成長；不選預先配置 `Arc` 的空間：要 `unsafe`。Blink 的 `SkTextBlobBuilder::allocRunPos` 照 glyph 數配置一次。
    - **效果：暫時的測試量長文 paint 一步**（release，改動前後，照第 2c 段），量完刪掉，記進 `progress.md`；行為由現有的 paint 測試守著，新的迴圈跑 mutation test。不跑 Speedometer。
    - **量了之後：fork 的迴圈不留**（2026-10-24）：kun 的迴圈加上重用的 buffer，長文 paint 一步從 5.7 ms 到 1.9 ms；再換成 fork 的 `GlyphSpan` 的迴圈反而慢到 2.4 ms（`flat_map` 和明確的迴圈兩種寫法都一樣），照上面的條件拿掉，fork 沒有這一段的修改。

## 待決定

## 之後的可能

- 改了一個字時只重新 shaping 那一段、沿用沒變的行（Blink 的 `ReusingTextShaper`、`InlineNodeDataEditor` 和行的重用）：編輯器打字時用得上。
- 第 1 段的問題寫成 taffy 的 issue 草稿（不送出，先給人審核）。
- 第 2 段之後，paint 也照範圍指向 glyph，不在 `PaintOp` 再複製一份；paint 加上 cull rect，畫面外的文字不轉成 glyph run（TipTap 的 paint 每一步放 23 萬個 glyph，大部分在畫面外）。
- kun 自己的 `layout::text`：TipTap 的取樣裡 106 ms（`InlineContent::new` 本身、`content_widths`、`fragments`），第 3 段之後重新取樣再拆。
- 溢出時才找緊急斷點（第 3 節的 S3）：`overflow-wrap: break-word` 讓 parley 在每個字元記一次緊急斷點、複製 `LineState`，取樣裡 34 ms；要改 parley 斷行的核心。
- grapheme 的界線也查表：分析剩下的時間裡佔得最多（3b 開始前量的 17%），要和 ICU4X 一致得另外產生一套表。
