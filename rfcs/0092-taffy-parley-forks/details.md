# RFC 0092 的細節：taffy 和 parley 的 fork

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## workaround 的清單

2026-11-10 讀 `crates/layout`、`crates/computed_style`、`crates/shell` 的程式整理的。函式名稱是 `crates/layout/src/` 底下的。成本：**有**（每次 layout 會多做事）、**少**（只在少數情況）、**無**（換算或取代原本的工作）。

### 有成本的 workaround

| workaround | 位置 | 成本 | 原因 | Chromium | 建議 |
| --- | --- | --- | --- | --- | --- |
| float 旁邊的盒子不進 taffy 的快取 | `compute_box`（`lib.rs`） | 有：旁邊有 float、或把 float 放進共用的 BFC 的盒子每次都重排 | taffy 的快取只用 `LayoutInput` 當 key | 快取的 key 包含 exclusion space | 留著：量過，重排的盒子幾乎都是同樣的輸入、同樣的結果，但每次只省盒子自己的一次計算、不省斷行，float 最多的幾頁也沒有比 Chrome 慢 |
| 行內的 float 等行確定才放，排不下就用 `revert_to` 重試 | `break_beside_floats`（`text.rs`） | 有：每行可能斷兩三次 | taffy 的 `BlockContext` 不能複製，放進去的 float 拿不出來 | line breaker 用 exclusion space 的副本試排 | 搬進 taffy 的 fork：`BlockContext` 可以撤回（上游 #1303 已經能讀回放好的 float）；寫上游的草稿 |
| 一段一段走 `find_content_slot`，自己找一段空間的底 | `float_bands`（`lib.rs`） | 少 | `ContentSlot.height` 不是 segment 的底：v0.15.0 永遠是無限大，上游的 main（#1394）只算到被更窄的 segment 縮小的地方，不告訴你哪裡變寬 | | 留著（成本少）：不在 fork 加露出 segment 範圍的 API，float 的其他工作也沒有做 |
| 做 fragment 時在最後的寬度再斷一次行 | `inline_fragments`（`lib.rs`） | 少：盒子最後不是斷行時的寬度（`min-width`、`max-width` 夾過）、或斷行的結果被清掉時多斷一次 | parley 只存最後一次的斷行；taffy 的 leaf 排完才知道盒子的寬度 | 行是 inline layout 的產出（fragment items）；`ComputeMinMaxSizes` 先定 inline size 再排一次 | 做了，在 machopo：fragment 用最後排版時斷的行（記下那時的寬度，不從 border box 算回來），寬度由內容決定的盒子在 fit-content 的寬度斷行。不多存一份斷行的結果 |
| 字型家族的清單每個 span 重建，每個名稱重新解析 | `text_style`（`text.rs`）、`font_stack`（`font_families.rs`） | 有：Linux 上清單加上別名和 metric-compatible 的字型，每個名稱一次 fontique 的 `NameKey::from_str` | machopo 照 Chrome 展開別名；parley 每次照名稱找 family | 照 `FontDescription` 快取 `FontFallbackList` | 做了：machopo 照 `font-family` 的值快取查到的 family（`FontFallbackMap`），fork 的 `StyleProperty::FontFamilyIds` 和 `TreeBuilder::push_default` 讓 parley 直接拿 id；清單的展開（別名）每個值只做一次。上游 #943（fontconfig 的 metric-compatible 別名）、#796（字型查詢的快取）合併時再看 |
| flex 和 grid 的 shrink-to-fit 多量 min/max-content | `fit_content_width`（`lib.rs`） | 少：通常在 taffy 的快取裡 | taffy 的演算法不會照可用的空間縮小 | 每種盒子都照 min/max size 取 shrink-to-fit | 留著；看上游 #1277、#1297 |
| column flexbox 的寬度捷徑，debug build 再跑一次 taffy 對照 | `column_flex_width`（`lib.rs`） | release 無（比 taffy 快）；debug 有 | taffy 為了寬度跑整個 flex 演算法 | `FlexLayoutAlgorithm::ComputeMinMaxSizes` | 做了：在 taffy 的 fork 裡（`compute_preliminary`），上游的草稿在 `upstream/taffy/`；machopo 的捷徑和對照拿掉了，寬度有限時的 shrink-to-fit 由 `fit_content_width` 做 |
| line clamp 把 `ComputeSize` 升級成 `PerformLayout`，fragment 每次重做 | `clamp_lines`（`lib.rs`） | 少：只有 line clamp 的盒子 | taffy 的 block layout 不知道 line clamp | 在 block 的演算法裡做 | 留著 |
| size containment 的 grid 不放 item 再排一次 | `contained_size`（`lib.rs`） | 少 | taffy 沒有 size containment | | 留著；之後可以搬進 fork |
| 列表框照 `size` 排兩次 | `lib.rs` 的表單控制項 | 少 | | | 留著（照 Chrome 的行為） |

### 換成上游就能拿掉的

| workaround | 位置 | 上游 |
| --- | --- | --- |
| 有 `text-indent` 時量 intrinsic 寬度改成真的斷行 | `indented`、`content_widths`（`text.rs`） | parley #866、#889（已合併） |
| `LineBreakData::hanging_advance` | parley 的 fork | parley #892（已合併） |
| atomic inline 的 last baseline 自己算 | `last_baseline`（`lib.rs`） | taffy #1110、#1111（進行中） |
| replaced 元素不用 taffy 的 `aspect_ratio`，先換算成 content box 的尺寸 | `replaced_taffy_style`（`lib.rs`） | taffy #1347（進行中）、#1157（進行中）、#1401（已合併） |

### 沒有成本、留在 machopo 的

- taffy：overflow 的 `auto` / `scroll` 對應成 `Hidden`、用 `Contain::PAINT` 建 BFC、覆寫 `is_containing_block`、`calc()` 的對照表（上游 #1400 改了 handle 的表示，跟上時配合）、grid 只在 `repeat()` 裡的名稱、16-bit 的飽和、`order` 的排序、`-webkit-box` 對應到 flex、table 的 style（`item_is_table`、collapsed border 寫進 cell 的 style）、長度的 clamp。
- taffy 沒有、machopo 自己做的：table formatting context（`table.rs`）、inline formatting context、`position: sticky`、scrollable overflow、outside 的 list marker、表單控制項、out-of-flow 盒子的 fragment。它們取代原本的工作，Chrome 也要做。
- parley：white space 的 phase I（`collapse_white_space`；parley 的 span 設成 `Preserve`，它自己的處理不跑）、inline 元素的盒子（邊緣是 0px 高的 `InlineBox`）、`text-overflow` 和省略號、連字號的字形、SVG text 每個字元的位置、canvas text、UTF-16 的 offset 對應、行尾的框、`line-height` 和 `word-break: break-word` 的對應、`vertical-align` 的百分比、用 read-fonts 讀 metrics。
- 照 Chrome 的：`needs_synthetic_bold`（fontique 只在差 200 以上才加粗）、字型家族的別名（`font_families.rs`）、variation 軸給 raster 時換回 user-space 的值。

## parley fork 現在的 commit

2026-11-10 的 `machopo` 分支（`cefcbc5d`），從上游的 `97beea38` 分出來，16 個 commit（GitHub 的 compare）：

| commit | 名稱 | 大小 | 改到的 |
| --- | --- | --- | --- |
| `8f8f6a4b` | Add InlineBox line break handling and update related tests | +244 −29 | parley |
| `4cc636ce` | Update README | +1 −1 | README |
| `2d056a98` | Wrap an atomic inline as the span it is in does | +20 −1 | parley |
| `43db46de` | Fix line break | +8 −1 | parley |
| `c6df7041` | Keep the shaping of layouts alike | +281 −8 | parley、parley_engine |
| `4a3f6213` | Make the line after a final break only where asked | +33 −1 | parley |
| `2f917f3e` | Take the first font when it covers a cluster | +95 −49 | parley |
| `d21a1186` | Lay out tabs at tab stops | +149 −8 | parley、parley_engine |
| `1e8a04a1` | Size the font optically | +49 −4 | parley、parley_engine |
| `7926e436` | Break lines after soft hyphens with a hyphen | +196 −18 | parley、parley_engine |
| `09abcf9b` | Share a layout's shaped text | +105 −22 | parley |
| `00e4b657` | Walk a glyph run's clusters from its span | +134 −34 | parley |
| `b7231f47` | Shape text a word at a time | +2272 −111 | fontique、parley、parley_engine |
| `2d54a105` | Look line breaks up in a table | +749 −6 | parley、parley_engine |
| `38493cc5` | Leave out fonts without an em | +8 −0 | fontique |
| `cefcbc5d` | Rename kun to machopo | +149 −149（28 個檔案） | 全部 |

每個 commit 都改了 README（當時叫 `README.kun.md`）。重寫前 fork 的 README 列修改的那張表（當時的標題是 Local changes），名稱和實際的 commit 不同：「Don't let inline box edges size the line」和「Give a line break's hanging whitespace」不是單獨的 commit（在第一個和 `43db46de` 裡）。fork 的測試都在 machopo 裡（`layout` 的 `parley_*::` 等），fork 自己幾乎沒有測試。

## parley fork 的重寫

建議的分組（rebase 到最新的 main 之後再確認）。每一組是一個 commit，帶 fork 自己的測試，README 的更新放在最後一個 commit：

| 組 | 包含 | 上游 | 建議 |
| --- | --- | --- | --- |
| inline 元素的邊緣 | `InlineBox::line_break`、邊緣不撐高行、atomic inline 照 span 的 `text-wrap-mode` | inline box 改用 U+FFFC（#902）；out-of-flow 的 inline box 換成 float 的 box（#940，進行中） | 照 #902 改寫；寫上游的草稿（上游的 Blitz 追蹤 issue #208 列著 inline span 的 margin、border、padding） |
| `hanging_advance` | `43db46de` | #892 已合併 | 拿掉，改用上游的 |
| 強制換行之後不做最後一行 | `set_line_after_final_break` | | 小，寫上游的草稿 |
| tab stop | `TabStops` | #934（進行中） | 和上游的 PR 比較；它合併時決定改用哪一個 |
| 軟連字號的連字號 | `SoftHyphens` | #827（進行中） | 同上 |
| optical sizing | `OpticalSizing` | | 先查能不能由 machopo 把 `opsz` 放在 `font_variations` 的最前面做到（shaping 快取的 key 也要分得開）；能的話拿掉 |
| 沒有 em 的字型 | fontique 的 `FontInfo::from_font_ref` | | 小，寫上游的草稿 |
| 先取第一個字型 | `FontSelector::select_font` 的 `first_font` | #796（字型查詢的快取，進行中）有關 | 寫上游的草稿 |
| 共用 shaped text | `Layout::shaped_text`、`GlyphSpan`、`GlyphSpan::clusters` | reshaping（#876、#879 已合併；#908、#930 進行中）會碰到 `ShapedText` | 兩個 commit 合成一個；照上游的 reshaping 改寫 |
| shaping 的快取 | `ShapeCache`、`Words`、`space_reach`、fontique 的 `SourceCache` | | 兩個 commit 合成一個（字詞快取用得到 layout 的快取的基礎）；最大的一組，送上游前先開 issue 討論設計 |
| 斷行的查表 | `line_table.rs`、`skip_word_boundaries` | | 寫上游的草稿；上游的 #755（把 segmentation 移出 `parley_engine`，進行中）會碰到 |
| 改名 | `Rename kun to machopo` | | 不再是單獨的 commit：每個 commit 從一開始就用 `machopo:` |

16 個 commit 預計變成 10 個左右，其中幾個在上游合併 #934、#827 之後還能再拿掉。

## 上游的變化

### parley 的上游

2026-09-30（fork 的起點）到 2026-11-08，上游的 main 多了 66 個 commit，v0.12.0 在 2026-10-09 發佈。和 machopo 有關的：

- **和 fork 重複**：Expose `hanging_advance` in `LineBreakData`（#892）。
- **會和 fork 衝突**：Represent inline boxes with U+FFFC（#902）、Store information needed for reshaping in `ShapedText`（#879）、Wire up HarfRust reshaping-related flags（#876）、Remove `ShapedRun::advance`（#916）、Reuse allocations when rebuilding（#906）、Allow graphemes to span `ShapedSlice` boundaries（#927）、Always align when line breaking（#941）、read-fonts 0.46 / skrifa 0.49 / harfrust 0.15（#898）。
- **讓 machopo 能拿掉 workaround**：Account for text indent in `calculate_content_widths`（#866、#889）。
- **可能讓 machopo 變快**：fontique: Cache system fallback misses（#900）、compute run's box metrics once（#863）、Avoid reading an atom's characters where possible（#937）。
- **新的功能**：`text-align-last`（#872）、`CollapsedSpace`（#887）、`Layout::primary_caret`（#939）。

進行中的 PR（2026-11-10）：Tab stops（#934）、Render a hyphen when a line breaks at a soft hyphen（#827）、fontique: resolve metric-compatible aliases in fontconfig backend（#943）、Fontique: Cache font queries and metrics in `FontContext`（#796）、Replace out-of-flow inline boxes with floated boxes（#940）、Reshape after line breaking（#930）、Implement reshaping in `parley_engine`（#908）、Move segmentation out of Parley Engine（#755）。

### taffy 的上游

machopo 用的是 v0.15.0（2026-10-17）；v0.15.1 在 2026-10-25 發佈。v0.15.0 之後 main 多了 40 個 commit，和 machopo 有關的：

- **修好 machopo 記成限制的**：Include a BFC root's bottom padding and border below its floats（#1384）、Check a BFC root against floats over its styled height（#1385）、Make `find_content_slot` avoid floats beside the content's full height（#1386）、Resolve percentage widths against zero in the min-content contribution of replaced block children（#1401）。`rfcs/0043-floats-text/taffy-float-root-inset-report.md` 的草稿要回報的就是 #1384 修好的問題，不用再送。
- **讓 machopo 能拿掉 workaround**：Compute the height of the BFC slot（#1394）、Store a caller-defined id with each placed float, and read placements back from `BlockContext`（#1303）、Make container baselines consistent across layout modes（#1323）、Grid: take the container baseline from the final layout of its first item（#1322）。
- **flex 的修正和效能**：#1327、#1328、#1332、#1333、#1335、#1336、#1343、#1363。
- **API 的變化**：Account for reversed scroll origins via a packed `ScrollableOverflow` type（#1372）、Add a public `FloatInsets` type（#1389）。

進行中的 PR（2026-11-10）：Implement CSS table layout（#1380）、Ensure BFCs avoid floats for their entire height（#1396）、Store calc() values as an opaque `CalcHandle` with 61 bits（#1400）、Request and cache baselines in ComputeSize passes（#1361）、Output last baselines from all built-in layout algorithms（#1110）、last-baseline alignment（#1111）、`aspect-ratio` applied to the content box（#1347）、Grid: limit fr tracks by definite available space（#1297）、Flexbox/Grid: limit the space available to a container's content by its max size（#1277）、Skip the layout cache for context-free leaves（#1377）。

## patch 檔的放法

| 做法 | 改的地方 | 升級 | 好處 | 壞處 |
| --- | --- | --- | --- | --- |
| A. 只有 fork（現在） | fork 的分支 | `git rebase` | 一份；git 的工具都能用 | 修改的歷史只在 fork 的 reflog；舊的頂端要打 tag 才保留；machopo 的審核看不到修改本身 |
| B. fork 加上 machopo 裡的 patch（建議） | fork 的分支，匯出 patch | `git rebase`（或 `git am -3` 套到新的上游），再匯出 | 修改和它的歷史在 machopo 裡、跟著 machopo 的 commit 審核；舊的 machopo commit 帶著當時的 patch；送上游的 PR 從 fork 開 | 兩份，要腳本檢查一致；改 fork 多一步匯出 |
| C. 只有 patch | machopo 裡的 patch，`ci.py third-party` clone 上游、套上 | `git am -3` 套到新的上游，再匯出 | 一份；不需要 fork 的分支 | 送上游的 PR 還是要 fork；套出來的 commit 的 hash 要固定 commit 的日期才穩定；不能直接在 checkout 上 commit、push |

B 的細節（第 1 段決定）：

- 放在 machopo 的 repo 的一個資料夾，每個專案一個子資料夾，裡面是 `git format-patch` 的輸出（`0001-….patch` 這樣編號），加上記上游起點的檔案，或直接用 `third_party.toml` 加一個上游的 rev 欄位。
- `ci.py third-party` 檢查：fork 的 rev 的上游起點和 patch 套上去的結果，和 fork 的分支的樹一樣；不一樣時說出是哪個 patch。
- `ci.py` 加一個子指令匯出 patch（從上游的起點到 fork 的 rev），避免手動匯出時漏掉。
- patch 檔照 `.gitattributes` 是 LF；上游的檔案都是 LF，套得上。
- 以前 parley 是放在 machopo 裡的副本加上 `patches/`（2026-10-01 改成 fork）。B 和那時不同：程式本身不在 machopo 裡，只有 patch。

## 參考資料

- RFC 0003 第 14 條（不 fork taffy）、RFC 0044（升到 taffy 0.15 時照它不 fork）、RFC 0083（column flexbox 的寬度、parley fork 的字詞快取和斷行表）。
- `docs/performance-gaps.md`（2026-11-08 的取樣）。
- `../third_party/parley/README.machopo.md`。
- Chromium 的 `README.chromium` 的格式；Firefox 的 `moz.yaml`。
