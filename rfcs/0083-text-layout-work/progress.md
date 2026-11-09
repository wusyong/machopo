# RFC 0083 的進度：文字 layout 的工作量

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-23，Windows 11。在 `0083` 上做。

完成日期：2026-10-24，Windows 11（Intel Core i9-13900K）。第 1 到 3 段在 2026-10-24 一起 merge 回 `main`（`3171f245`）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 column flexbox 的 min/max-content 寬度不排子項（「詳細設計」第 1 節） | 完成 | 2026-10-23 |
| 2 文字的 fragment 共用 shaping 的結果（第 2 節） | 完成 | 2026-10-24 |
| 　2a fork 共用 shaping 的結果、tab 用 `Arc::make_mut`、開放 `Glyphs` | 完成 | 2026-10-24 |
| 　2b `TextFragment` 改形狀，排版不走 glyph 和 cluster；paint 從共用的結果算 glyph | 完成 | 2026-10-24 |
| 　2c 計時、文件 | 完成 | 2026-10-24 |
| 3 shaping：先取樣再決定（第 3 節） | 完成 | 2026-10-24 |
| 　3a 字詞的 shaping 快取 | 完成 | 2026-10-24 |
| 　3b U+0000–U+017F 和 U+2000–U+203F 的字元查表找斷行機會、不需要時不算字的界線 | 完成 | 2026-10-24 |
| 　3c paint 放 glyph 改成直接的迴圈 | 完成 | 2026-10-24 |

## 驗收

**column flexbox 的寬度（`layout`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 非換行的 column flexbox 的寬度只取子項的貢獻：margin、padding、border、`min-width`、`max-width`、`box-sizing`、out-of-flow 和 `display: none` 不算、`column-reverse` | `layout` 的 `flex::a_column_is_as_wide_as_its_widest_item`；debug build 每次比對 taffy 的寬度 |
| min-content、max-content，確定的寬度夾在兩者之間；可以壓縮的 replaced 元素 | `flex::a_columns_content_widths`、`flex::a_column_fits_a_compressible_item_in` |
| 子項自己的寬度、`min-content`、`max-content`、`min-width`、`max-width` | `flex::a_column_items_own_width` |
| 換行、row、自己有寬度的交還 taffy；絕對定位的兩軸都量 | `flex::what_a_columns_width_needs_heights_for` |
| 類似 TipTap 的頁面每段只斷一次行（換行的 column 經過 taffy 斷兩次） | `flex::a_column_in_a_row_breaks_its_paragraphs_once`（`LayoutUpdate::lines_broken`） |
| 行內內容只被問寬度時不斷行；有 float、長度的 `text-indent` 時照舊；百分比寬度的 inline-block 在 min-content 裡照內容 | `inline::a_width_alone_breaks_no_lines` |
| 斷行的計數 | 上面兩個，和 `update` 的測試（`Page::laid_out`） |

**共用 shaping 的結果（parley fork、`layout`、`paint`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| fork 的 shaping 結果共用：保留的 shaping 和 layout 共用，樣式不同時才複製；重新 build、重新斷行（tab 的寬度變了）、`clear` 都不改到別人拿著的那一份 | `parley_shaped_text::a_kept_shaping_is_shared`、`breaking_lines_again_leaves_a_kept_shaped_text_as_it_was`、`a_layout_built_again_leaves_a_kept_shaped_text_as_it_was` |
| `GlyphSpan` 從共用的結果走出和 `GlyphRun::glyphs` 一樣的 glyph（letter spacing、justify、RTL、shaped run 的一部分） | `parley_shaped_text::a_spans_glyphs_are_its_runs`；kun 的文字 fragment 改從它取 glyph，現有的文字、paint 測試守著位置 |
| `GlyphSpan` 從共用的結果走出和 `Run::visual_clusters` 一樣的 cluster（那一行、那個樣式的，沒有 glyph 的換行也算）和文字的範圍 | `parley_shaped_text::a_spans_clusters_are_its_runs` |
| 排版不排 cluster 和 glyph：一般的斷行、保留的換行、`<br>`、沒有 `text-overflow` 的溢出都不排；`text-overflow` 截斷的那個 run 才排，計數守著 | `text_overflow::only_a_cut_run_has_its_clusters_and_glyphs_placed`（`LayoutUpdate::clusters_placed`、`glyphs_placed`） |
| paint 只替有選取的 text node 算 cluster，其他從 glyph 畫 | `paint` 的 `paint_controller::only_selected_text_has_its_clusters_worked_out` |
| 重新 build 的樹和原本的比值相等：有 tab 的段落斷行時各自複製一份 shaping 的結果 | `update::text_with_a_tab_lays_out_as_built_from_scratch`（和其他 `update` 的測試：`Page` 每一步比對從頭 build 的樹） |
| 位置從 run 的起點算：不在行首的 run 截斷、換行、保留的換行 | `text_overflow::a_run_further_on_is_cut_where_its_clusters_are`、`inline::how_lines_end` |

**字詞的 shaping（parley fork、fontique）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 逐字 shaping 的結果和整段 shaping 逐位相同 | debug build 每個用到字的 layout 再整段 shaping 一次比對（`layout` 的測試用 Ahem 的都會走到）；`parley_words::system_fonts_shape_words_as_whole_text`（每個系統字型、多種 script 的文字，release 也比對） |
| 字只 shaping 一次：在空白前後切、組合字元跟著空白、32 字以內才存 | `parley_words::a_word_is_shaped_once`、`text_is_split_before_and_after_each_space`、`a_long_word_is_not_kept`（`words_reused_and_shaped` 的計數） |
| 選字型的樣式（粗細）、大小、feature、variation、locale、script 不同時重新 shaping，顏色不同時沿用 | `parley_words::a_word_asking_for_other_fonts_or_options_is_shaped_again`、`a_word_in_another_script_is_shaped_again` |
| 用到空白超過旁邊一個 glyph 的字型（Lato 的 `calt`）、有預設關掉的 feature、RTL 時整段 shaping；不用到的（Ahem、Lato Bold、Rochester）逐字 | `parley_words::text_is_shaped_whole_where_words_would_shape_otherwise`、`words_beside_a_space_a_font_shapes_with_them_are_shaped_again_as_one` |
| 沒有 em 的字型（`head` 的 units-per-em 是 0、沒有 `head`）不收，要它的文字照找不到時 fallback；有 em 的照收 | `parley_space::a_font_without_an_em_is_not_taken` |
| 只用到空白旁邊的 glyph 的字型逐字，碰到那些 glyph 的邊界把字併起來重新 shaping：positioning 併一側、substitution 併兩側；kern pair（GPOS 的 glyph、class 的，`kern` 表的）、ligature、六種 context 和 reverse chain、跳過 mark、substitution 變出的 glyph、空白和組合字元的 ligature、一側超過 256 個；比對得更遠的（三格的規則、class 0、跳過 base、cursive、空白當成 mark、AAT）整段；預設關掉的 feature 不看 | `parley_space::`（write-fonts 造的字型，每段文字都和整段 shaping 逐位比對、數併的次數）；`parley_words::words_beside_a_space_a_font_shapes_with_them_are_shaped_again_as_one`（Exo 2、Noto Sans Devanagari）、`system_fonts_shape_words_as_whole_text`（Arial 的 `AV To Wa`） |
| 上限：2^15 個字、選項和字型樣式的號碼各 2^12 個，用完連同字一起清；註冊字型時清 | `parley_words::the_words_kept_start_again_*`、`words_are_shaped_again_once_cleared`、`shaping::adding_fonts_forgets_what_shaping_gave` |
| fontique 清掉的字型還有人拿著時，重新載入是同一個 blob（同一個字型 id，存的字和整段 shaping 的 run 才合得起來）；沒人拿著的放掉 | fork 的 fontique 的 `source_cache::tests`（在 fork 裡跑） |

**斷行表和字的界線（parley fork）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 查表找到的斷行機會和 ICU4X 的相同 | fork 的 `line_table::tests`（表是最新的、短字串窮舉、長字串隨機）；debug build 每段再和 ICU4X 整段比對（`layout` 的每個有文字的測試） |
| 表內的字不交給 ICU4X，有表外字元的字、空白旁邊的引號那一側的字才交；其他斷行規則整段給 ICU4X | `parley_line_breaks::text_in_the_table_is_looked_up`、`text_with_other_rules_is_given_to_icu4x_whole`（`line_breaks_looked_up` 的計數） |
| 不算字的界線：沒有字的界線、斷行不變，沒有字典的文字也不標 | `parley_line_breaks::word_boundaries_are_left_out_when_turned_off` |

**paint 放 glyph（`paint`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| paint 在一個迴圈裡放 glyph：從 run 的起點和 box 的位置算、移到對齊的 baseline、不留上一個 run 的；`text-overflow` 截掉的不畫，整個被藏起的 run 不出 item | 現有的 paint 測試（`artifact::text_after_all_backgrounds_with_decorations_around_it`、`text_shadows_are_under_the_text_and_its_lines`……）、`artifact::what_text_overflow_hides_is_not_painted` |

**效果**

| 項目 | 怎麼驗證 |
| --- | --- |
| Editor-TipTap 120.2 → 79.8 ms（−33.6%）；第 2 段做完時 102.2 ms | `python3 scripts/bench.py speedometer --browser kun --suites Editor-TipTap`，三個版本輪流（「做過的檢查」的量測） |

## 做過的檢查

2026-10-23 到 2026-10-24，Windows 11（Intel Core i9-13900K），全部在這台上做。

- **做了什麼**（設計見 `README.md` 的「詳細設計」）：
  - 第 1 段：`layout` 的 `compute_box` 攔下非換行的 column flexbox 只被問寬度的情況（`column_flex_width`、`widest_column_item`），debug build 每次比對 taffy 的寬度；`compute_inline` 只被問寬度、沒有 float 和長度的 `text-indent` 時不斷行；`LayoutUpdate::lines_broken`。
  - 第 2 段：fork 的 `Layout::shaped_text`（`Arc`）、`GlyphRun::span`（`GlyphSpan` 的 `glyphs`、`clusters`、`text_range`），fork 的 commit `09abcf9`、`00e4b65`；`layout` 的 `TextGlyphs`、`LineEndAt`，`LayoutUpdate` 的 `clusters_placed`、`glyphs_placed`。
  - 第 3 段：fork 的 `shape/words.rs`、`shape/space.rs` 和 `shape_segment` 的逐字路徑，fontique 的 `SourceCache` 的弱參照（`b7231f4`）；`parley_engine/src/line_table.rs` 和 `AnalysisOptions::skip_word_boundaries`（`2d54a10`）；`layout` 的 `TextFragment::for_each_glyph`，`paint` 的 `Painter` 重用的 buffer。
- **實作時改的設計**（都記在「已決定的事項」）：
  - 確定的寬度原本打算直接用那個寬度量子項，`svg::a_percentage_width_svg_shrinks_with_grid_columns_and_flex_boxes` 的 debug 比對 panic（1280 px 對 taffy 的 195 px）：百分比寬度的 SVG 在 min-content 下可以壓到 0。改成量 min-content、max-content 再夾住（第 2 條）。
  - column 的快速路徑接上之後，類似 TipTap 的頁面每段還是斷兩次：block 問段落的寬度時，行內內容的 measure 照樣在 min-content 寬度斷行。加了行內內容只被問寬度時不斷行（第 7 條）。比對 28 種行內內容在三種可用寬度下改動前後的寬度：只有長度的 `text-indent` 變了，改成有縮排時照舊斷行；另外三處變得比較接近 Chrome（百分比寬度的 inline-block 在 min-content 裡照內容，`text-indent: 10%` 在 min-content 裡當 0，`pre-wrap` 結尾的空白少了浮點的雜訊）。
  - 行尾也改成用到時才算、`hidden_from` 仍是 glyph 的個數（第 11 條）；fragment 的 `==` 改成比值：`update` 的測試拿從頭 build 的樹比對，兩棵樹的 `TextSource` 各有一份。
  - 第 3a 段：Arabic 和 Syriac、預設關掉的 feature 也整段 shaping，`FontSelector` 建立時重設 query 的屬性（第 16 條）；Arial 這類字型改成在空白旁邊的 glyph 的邊界併字（第 15、17 條）。
  - 第 3b 段：先做到 U+00FF，量了之後擴到 U+017F 和 U+2000–U+203F（第 18 條）；原本預期要排除的 LB20a（字首的連字號）ICU4X 沒有實作，那一格不需要上下文。
  - 第 3c 段：fork 的 `GlyphSpan` 的迴圈量了反而慢，照第 19 條的條件拿掉。
- **找到的 bug**：Speedometer 的長文用 Verdana 時，逐字 shaping 的 debug 比對 panic（逐字 9 個 run、整段 1 個）：`FontContext` 每次 layout 之後 `prune(128)`，fontique 清掉之後重新載入同一個檔案時給新的 blob（新的 id），存著的字和新的 run 合不起來。改成清掉時保留弱參照。段落快取原本的 debug 比對也會被 `FontSelector` 留下的 query 屬性騙到（粗體選到一般的字型），只是測試沒踩到。
- **測試寫法踩到的**：
  - 第 1 段一開始的寬度測試把 column 直接設成 float，18 條 mutation 沒抓到：float 自己的寬度在 `PerformLayout` 裡算，不會用 `ComputeSize` 問。改成放在會量子項寬度的容器裡。
  - 可以帶著的測試字型裡沒有只用到空白判斷一個分支的，9 條 mutation 沒抓到：用 `write-fonts` 在 `layout` 的測試裡造字型（`parley_space`，第 14 條）。
  - paint 原本的測試沒有 `text-overflow`，有文字的 box 都在 x 是 0 的地方，3c 的 4 條沒抓到：加了 `artifact::what_text_overflow_hides_is_not_painted`。
  - fork 的 `core.autocrlf=true`：在 fork 裡 `git stash` 和切換 commit 之後，231 個檔案變成 CRLF，`test.py mutate` 多行的 `from` 靜靜地沒改到、報成沒抓到；轉回 LF 之後重跑抓到。fork 的 `git status` 平常也列出幾乎每個檔案，`git diff --ignore-cr-at-eol` 才是真的改動。
- **mutation test**（每段一份 `mutants-*.toml`，多半是 `cargo test -p layout --test it`）：
  - `mutants-1.toml`：36 條，28 條抓到；其餘 8 條是現在的呼叫方做不出差異的條件（只在 `ComputeSize` 時走、寬度已知、自己有 `width`、`max-width` 不大於 `min-width`、有比例的交還、捲軸、扣掉上下的 margin），記進 `docs/test-gaps.md`。
  - `mutants-2a.toml`：12 條全部抓到；別人拿著時不重新算 tab 的寬度那條讓斷行一直重來，到 600 秒的上限才停（算抓到）；`clear` 不放掉的那條補了測試才抓到。
  - `mutants-2b.toml`：23 條全部抓到（paint 的三條用 `test.py reftest text/t16-text-overflow.html`、`paint_controller::`、`shell` 的 `selection_painting::`）；不在行首的 run 的位置、比值的 `==` 補了測試才抓到。
  - `mutants-3a.toml`：86 條，83 條抓到（fontique 的兩條在 fork 裡跑 `source_cache::tests`）；等價的 3 條（沒有樣式號碼的也逐字、整段重來時不重設 selector、chained 的 backtrack 不倒過來）記進 `docs/test-gaps.md`。沒有做成 mutation 的分支：不知道 reach 的字型（存著的字的字型一定判斷過，走不到）、併前兩個字時前一個字不知道（從 run 的開頭併，結果一樣）、`kern` 表的空白後面接 mark（mark 不會開始一個字）、跳過 mark 的 lookup 把空白和 mark 一起比對（比對不到，保守）。
  - `mutants-3b.toml`：29 條，27 條抓到；兩個 byte、三個 byte 的字元對到錯的類別、字最後的標點當成表外，補了 `čaj`、`note†1`、`€…` 才抓到；等價的 2 條（交給 ICU4X 的字從文字開頭看起、kun 不關掉字的界線）記進 `docs/test-gaps.md`。
  - `mutants-3c.toml`（`cargo test -p layout -p paint`）：12 條，11 條抓到；等價的 1 條（省略號的 glyph 不截）記進 `docs/test-gaps.md`。
  - `mutants-em.toml`（merge 之後的修正，下面的「在 macOS 上」）：4 條全部抓到。
- **量測**（暫時的計時都是 release、量完刪掉）：
  - 第 2 段（`details.md` 的「共用 shaping 的結果之後」）：Speedometer 的 `longtext.html`，800 和 820 px 交替，和 2a 的 commit 輪流各 3 次：排版每一步 16.8 → 4.8 ms，paint 1.5 → 5.2 ms（放 glyph 搬到 paint），加起來 18.3 → 10.0 ms。
  - 第 3a 段：長文第一次 layout，字型清單只有 Verdana 時 67,737 個字用存著的、9,952 個 shaping（87%）。照第 15 條改之前，TipTap 的字型清單在 Windows 上落到 Arial（沒有 Helvetica），Arial 的 `kern` 和 `ccmp` 用到空白，整段 shaping；長文的 54,842 個空白的邊界裡，Arial 的那幾個字母在旁邊的只有 208 個（0.4%），所以改成在那些邊界併字。
  - 第 3b 段（長文合併空白後 225 KB、189 段）：只算斷行機會，ICU4X 1.53 ms；表到 U+00FF 0.88 ms、2774 個字交給 ICU4X（`’` 2589 個）；擴大之後 0.53 ms、沒有字交給 ICU4X。整個分析算字的界線 5.69 ms、不算 4.30 ms。
  - 第 3c 段：長文 paint 一步（231,753 個 glyph）5.77、5.88、5.48 ms → 1.89、1.86、1.88 ms；換成 fork 的迴圈 2.38–2.56 ms，不留。
  - Speedometer（做完之後，使用者要求）：`--browser kun --suites Editor-TipTap`，這份 RFC 之前的 `main`（`1e670a6a`）、第 2 段做完的 `b331feaa`、第 3 段做完的 `6f580a90` 輪流各 5 次，10 個 iteration：120.2 ms（119.7–121.2）→ 102.2 ms（101.6–103.7）→ 79.8 ms（79.0–80.7）；Long 的 sync 59.8 → 38.6 ms、Highlight 55.1 → 34.7 ms，第 2 段多出來的 paint（async 約 +3.5 ms）由第 3 段拿回來。三個版本釘的 parley fork 不同，`--against` 不能用，舊的兩個各自另開 worktree、配上自己的 fork 建。和 2026-10-23 的 Chrome（41.3 ms）比，從 2.87 倍變成約 1.93 倍。數字也記在 Speedometer 量測工具那份 RFC 的 `progress.md`。
- **WPT 和 reftest**：`test.py reftest` 的 `flex/`、`block/`、`inline/`、`text/`、`images/`、`svg/`、`floats/`、`tables/`、`paint/`、`generated/`、`invalidation/`；`test.py wpt` 的 `css/css-flexbox/`、`css/css-sizing/`、`css/CSS2/floats/`、`css/css-overflow/`、`css/css-pseudo/`、`css/css-text/`：沒有預期之外的結果。做的期間 `css/css-text/` 有兩個非預期的（`percentage-value-intrinsic-size.html`、`letter-spacing-trim-start-002.html`），在這份 RFC 開始前的 commit 上也一樣；rebase 到之後的 `main` 時已經修好。一次給 `test.py wpt` 三個目錄時 Windows 回報命令列太長（`WinError 206`），分開跑。
- **其他**：建 Speedometer 用的 `perf` profile 時，rustc 有一次在編 `layout` 時 crash（`STATUS_ILLEGAL_INSTRUCTION`），重建就過了，沒有重現。
- **最後的驗證**（rebase 到 `c872e22f` 之後）：`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`；`cargo test --workspace`（3845 個通過）；`test.py reftest text/ inline/`、`test.py wpt css/css-text/` 沒有預期之外的結果。
- **在 macOS 上**（merge 之後，2026-10-24，macOS，Apple M4）：`parley_words::system_fonts_shape_words_as_whole_text` 在 GB18030 Bitmap（`NISC18030.ttf`）上失敗。逐字和整段 shaping 其實逐位元相同：這個字型是 Apple 舊式的 bitmap 字型，只有 `bhed`、`bdat`、`bloc`，沒有 `head`、`hhea`、`hmtx`，em 讀成 0，glyph 的位置、run 和 cluster 的 advance 是 NaN 和 inf（`font_size / units_per_em`），`==` 把 NaN 當成不同；網頁用到它時 debug build 的檢查也會 panic，layout 拿到 NaN 的寬度（逐字之前就是這樣）。使用者選了讓 fontique 不收沒有 em 的字型（`FontInfo::from_font_ref`，照找不到字型時 fallback；記進 `docs/limitations.md`），不選只把比對改成逐位元（字型照舊排出 NaN）。`parley_space::a_font_without_an_em_is_not_taken`（write-fonts 造的 `head` 是 0 和沒有 `head` 的字型），`cargo test -p layout` 和 fork 的 `fontique`、`parley` 的測試通過（fork 的 `parley_tests` 在這之前就編不過：`InlineBox` 的初始化少了 `line_break`）。
