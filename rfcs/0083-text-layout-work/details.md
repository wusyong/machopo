# RFC 0083 的細節

## TipTap 的計數

2026-10-23，Windows 11（Intel Core i9-13900K），`main` 的 `d5b29cda`。

**量法**：暫時在 `crates/layout/src/text.rs` 加 thread-local 的計數和計時（沒有 commit，量完用 `git checkout` 還原），`BoxTree::update` 和 `BoxTree::layout` 結束時印一行到 stderr；用 `python3 scripts/bench.py speedometer --suites Editor-TipTap --browser kun --iterations 3` 跑，從 `target/bench/speedometer/kun.log` 讀。計的東西：

- shaping：`InlineContent::new` 的次數、文字的 byte 數、時間（其中 parley 的 `build`）、fork 的 shaping 快取命中幾次（`shapings_reused_for_testing` 的增加）、文字以前看過幾次（文字的雜湊）。
- 斷行：`break_lines` 的呼叫次數、寬度和 float 沒變而沿用的次數、真的斷的次數、byte 數、行數、時間，和沒沿用的原因（新的、被重設、寬度變了、float 變了、要放 float）。
- min/max-content：`content_widths` 真的算的次數和時間。
- fragment：`fragments` 的次數、行數、時間。

計時本身讓每項慢一點，數字看比例。

**每一步**（第 2、3 個 iteration，兩次幾乎一樣；第 1 個 iteration 只多了冷啟動）：

| | Long | Highlight |
| --- | --- | --- |
| 建 box tree | 25.2–25.6 ms：shaping 261 段、243,946 B，22.4–22.8 ms（幾乎都在 parley 的 `build`）；快取命中 3 段（12 B）；261 段的文字都看過 | 25.2–25.9 ms：shaping 256 段、243,929 B，22.5–23.2 ms；命中 0 |
| layout 合計 | 22.8–23.0 ms | 22.0 ms |
| 斷行 | 1418 次呼叫、604 次真的斷、488,389 B、20,143 行，10.1–10.6 ms；新的 261、寬度變了 321、被重設 22 | 1402 次、588 次、488,304 B、20,276 行，10.0–10.2 ms；新的 256、寬度變了 320、被重設 12 |
| min/max-content | 279 段（815 次呼叫），1.6 ms | 268 段（808 次），1.6 ms |
| fragment | 262 段、1,885 行，10.1–10.6 ms | 258 段、2,018 行，9.5–9.6 ms |

另外每步有 2、3 次小的 layout（控制列的按鈕、游標），每次 0.1–0.5 ms，斷行的都是十幾個字的內容。

**兩次斷行的寬度**：再跑一個 iteration，記下 3,000 B 以上的段落每次斷行的寬度。46 段各兩次：第一次在 83–119 px（各段不同，約是它最長的字），原因是「新的」；第二次在 780 px，原因是「寬度變了」。這 46 段（200,646 B）窄的那次 13,923 行、5.1 ms，780 px 的 1,507 行、2.9 ms。

**快取為什麼沒中**：fork 的 shaping 快取（`shape/cache.rs`）收到 2^18 字就清空重來。Long 和 Highlight 各約 24 萬字、字型不同，一個 iteration 就超過上限，下一個 iteration 再來時早就清掉了。

## taffy 的路

taffy 0.15 的 `compute/flexbox.rs`：

- `compute_flexbox_layout`：`ComputeSize` 而寬度和高度都已知、或只要寬度而寬度已知時提早結束。
- `compute_preliminary`：第 3 步（`determine_flex_base_size`）之後，只有「要的那一軸是主軸」時提早結束（row 只要寬度、column 只要高度）。column 只要寬度時繼續到第 7 步以後，前面的第 3 步已經替每個 item 量了 flex base size：`flex: 1` 的 `flex-basis: 0%` 在高度不確定時當成 `content`，量 item 在可用寬度（這裡是 min-content）下的高度。
- `#editor` 是 block container，在 min-content 的可用寬度下排它的段落：kun 的量法（`lib.rs` 裡 inline content 的 `AvailableSpace::MinContent` 分支）先取 `content_widths` 的 min，再在那個寬度 `break_lines`。

頁面（Speedometer 的 editors 底下的 style.css）：`body { display: flex }`、`#app { flex: 1; display: flex; flex-direction: column }`、`#editor { flex: 1; overflow: auto }`。`body` 決定 `#app` 的 flex base size（`0%`，container 寬度確定，所以是 0）之後，`#app` 的自動最小尺寸是它的 min-content 寬度，於是問 `#app`。

## column flexbox 被問寬度的次數

2026-10-23，同一台。暫時在 `compute_box` 的 flex 分支記下 taffy 每次（沒有從快取拿的）只要寬度、寬度未知的 column container：換不換行、可用寬度的種類、高度是否已知、直接的子項有沒有比例。`bench.py speedometer --suites Editor-TipTap,NewsSite-Next,NewsSite-Nuxt --browser kun --iterations 1`，連同載入頁面，共 21,558 次：

| 可用寬度 | 不換行，高度未知 | 不換行，高度已知 | 換行 |
| --- | ---: | ---: | ---: |
| `MinContent` | 7,536 | 3,551 | 162 |
| `MaxContent` | 6,188 | 2,373 | 54 |
| `Definite` | 0 | 1,694 | 0 |

直接的子項都沒有比例。每次都整個跑一次 taffy 的 flex 演算法（巢狀的 container 時間重複算，所以不加總時間）。

## 文字的 fragment 花在哪

2026-10-23，Windows 11，release build。`layout` 的暫時測試：Speedometer 的 editors 底下的 `assets/longtext.html` 放進同一個資料夾的 `style.css` 的 `#app`、`#editor` 裡（`TextContext::new(false)`，系統字型），在 800 和 820 px 寬之間交替排版 12 次，暫時在 `InlineContent::fragments` 和 `text_fragment` 裡計時（量完還原）。後 10 次，每次：

| 部分 | 時間 |
| --- | ---: |
| `fragments`（256 個行內內容、1,829–1,866 行） | 12.2–13.2 ms |
| 其中 `text_fragment` | 10.4–11.2 ms |
| 　`TextSource::clusters` | 7.0–7.8 ms |
| 　　只走一遍 parley 的 `visual_clusters()`（另外量） | 5.2–5.4 ms |
| 　`positioned_glyphs` 轉成絕對位置（231,745 個 glyph） | 2.9–3.3 ms |
| 　字型的 `Arc` | 0.1 ms |
| 　`run_variations` | 0.1 ms |
| 行內元素的盒子和 `translate` | 0.04 ms |
| `line_end` | 0.2 ms |

整個排版每次 15–16 ms（每段斷一次行）。大半是 parley 的兩個迭代器：每個 cluster 約 23 ns（`Cluster` 每次從 atom 和 grapheme 組出來），每個 glyph 約 13 ns；cluster 對到 UTF-16 的查表和存起來約 2 ms。配置和複製本身（字型的 `Arc`、`Vec`）幾乎不花時間。

paint 沒有 cull rect：每個 fragment 都轉成 `PaintOp::DrawGlyphs`，每個 run 再配置一次 glyph id 和位置（`paint` 的 `text_run`）。TipTap 每一步的 fragment 都是新的，paint 每一步也把 23 萬個 glyph 轉一次；這裡沒有量。

## 共用 shaping 的結果之後

2026-10-24，Windows 11，release build。同一頁、同樣在 800 和 820 px 之間交替 12 次，這次量整個排版（`BoxTree::layout`）和整個 paint（`paint::paint`，不含 pre-paint），用 `paint` 的暫時測試（量完刪掉）。第 2a 段的 commit 另開一個 worktree 編，兩個版本輪流跑 3 次，取每次的後 10 步（各 30 個）：

| | 第 2a 段（glyph 和 cluster 在排版時放） | 第 2b 段（用到時才放） |
| --- | ---: | ---: |
| 排版 | 16.8 ms（15.9–18.5） | 4.8 ms（4.4–5.6） |
| paint | 1.5 ms（1.3–2.0） | 5.2 ms（4.7–5.9） |
| 加起來 | 18.3 ms | 10.0 ms |

排版少了約 12 ms：cluster（約 7 ms）、glyph 的位置（約 3 ms）和複製。paint 多了約 3.7 ms：每一步放 23 萬個 glyph（parley 的 `Glyphs`，每個約 13 ns），`text_run` 先收進一個 `Vec` 再轉成 glyph id 和位置，比原本多走一遍。這一頁的 paint 沒有 cull rect，每一步把每個 run 都畫一次；之後 paint 只畫看得到的 run 時，這部分跟著變小，排版省下的不受影響。

## TipTap 的取樣

2026-10-24，Windows 11，kun `b331feaa`（第 2 段做完），`python3 scripts/bench.py speedometer --sample --suites Editor-TipTap`：`perf` profile、samply 經過 ETW、每 1 ms 一次、以 CPU 時間計，只取 renderer 的主執行緒、runner 計時的區間（10 個 iteration、20 個區間），共 949.2 ms。輸出在 `target/bench/speedometer/sample/`（`Editor-TipTap-kun.folded`），沒有 commit。

每個 stack 照最外層的 kun 的 frame 分階段：

| 階段 | 時間 | 比例 |
| --- | ---: | ---: |
| 建 box tree（`BoxTree::build`、`update`） | 498.9 ms | 52.6% |
| script、DOM 和其他 | 174.9 ms | 18.4% |
| 排版（`BoxTree::layout`） | 158.6 ms | 16.7% |
| paint | 97.3 ms | 10.3% |
| style | 19.5 ms | 2.1% |

文字的工作（stack 裡有 parley、harfrust、icu、fontique、`layout::text` 或 `ShapedSpan` 的）共 697.1 ms，照最內層認得的 frame 分：

| 部分 | 時間 | 在哪個階段 |
| --- | ---: | --- |
| harfrust | 128.5 ms | 建 box tree |
| parley 的 shaping 簿記（`shape_text` 本身 65 ms、`push_run` 46 ms 等） | 122.7 ms | 建 box tree |
| 選字型（`Mapper::coverage` 45 ms、fontique 的 `charmap` 28 ms 等） | 94.4 ms | 建 box tree |
| icu 的分析（`Analyzer::analyze` 本身 40 ms，ICU 的 segmenter 約 49 ms） | 90.8 ms | 建 box tree |
| kun 的 `layout::text` | 106.0 ms | 建 box tree 53 ms、排版 53 ms |
| paint 放 glyph（`ShapedSpan` 的 glyph 迭代器） | 58.8 ms | paint |
| parley 的斷行 | 34.8 ms | 排版 |
| `LineState` 的 clone（`line_break` 的 `impl$12::clone`，每個緊急斷點一次） | 33.7 ms | 排版 |
| parley 的其他 | 27.6 ms | 排版 24.6 ms |

呼叫 parley 的 kun 函式：`InlineContent::new` 434.2 ms（其中 `shape_text` 330.5 ms、`Analyzer::analyze` 90.8 ms），`break_lines` 68.5 ms，`fragments` 20.5 ms。`shape_text` 底下查 fork 的段落快取幾乎不花時間，只看得到 `ShapeCache::insert`：快取最多放 2^18 個字元，滿了整個清掉，一個 iteration 的一般字和粗體約 49 萬字，所以每個 iteration 都重新 shaping。

paint 的 59 ms 裡，43 ms 是 `Take<Chain<FlatMap<…ShapedSpan…>>>` 的 `next` 本身，16 ms 是裡面 parley 的 `Glyphs` 和位置的 `map`；`Painter::text_run` 本身 9 ms。

Speedometer 的長文（editors 底下的 `assets/longtext.html`）：255,426 個字元、196 段（沒有兩段一樣）、以空白分開的 39,003 個字只有 9,917 種；最常見的 100 種佔 47%，1,000 種佔 70%。

## Blink 的對照

2026-10-23 讀的 Chromium `main`：

- `third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc` 的 `ComputeMinMaxSizes`：先試 `CalculateMinMaxSizesIgnoringChildren`；row 走 `ComputeMinMaxSizeOfRowContainer`，換行的 column 走 `ComputeMinMaxSizeOfMultilineColumnContainer`（在 column-wrap 的 intrinsic 階段跑 `PlaceFlexItems`）；非換行的 column 用 `FlexChildIterator` 走 in-flow 的子項，每個用 `ComputeMinAndMaxContentContribution`（`MinMaxSizesInput::UnconstrainedUntriaged()`）加上 inline 方向的 margin，取最大，`max` 不小於 `min`，加上 border、捲軸和 padding。
- `third_party/blink/renderer/core/layout/inline/inline_node.cc`：`PrepareLayoutIfNeeded` 拿舊的 `InlineNodeData`，`ShapeText` 用 `ReusingTextShaper` 沿用舊的 `ShapeResult`（不安全的不沿用：`IsUnsafeToReuseShapeResult`、套過間距的）；`IsNGShapeCacheAllowed` 只在簡單的情況用 `NGShapeCache`（字型沒有複雜的 feature、沒有間距、`auto_space` 不適用）。`ComputeMinMaxSizes` 用 `LineBreaker` 的 `kMinContent`、`kMaxContent` 模式斷行，能從 min 的結果推出 max 時不跑第二次。
- `third_party/blink/renderer/platform/fonts/shaping/ng_shape_cache.h`：`kMaxTextLengthOfEntries = 30`、`kMaxSize = 2048`，滿了只查不加；記憶體壓力時清空。

## kun 的程式

- 斷行沿用的條件：`InlineContent::break_lines` 的 `broken`（寬度、float 的範圍、大小）。`content_widths` 呼叫 `resize_for_widths`，會把 `broken` 清掉。
- fragment：`InlineContent::fragments` 和 `text_fragment`（每個 run：`TextSource::clusters`、`Arc::new(font.font.data.clone())`、`run_variations`、`positioned_glyphs` 轉成絕對位置）；`TextFragment::translate` 逐個 glyph 移動。
- 用到 `TextFragment.glyphs` 的地方：`paint` 的 `text_run`、`paint/src/svg.rs`；自己做 glyph run 的：`layout` 的 `svg_text.rs`、`canvas_text.rs`，`dom_api` 和 `renderer` 的 canvas 文字。
