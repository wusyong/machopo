# RFC 0043 的進度：float、`vertical-align` 和文字的 property

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-13。完成：2026-10-14，Windows 11（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0043` 上做：第 1 到 3 段在 Fedora 44（Linux x86_64，AMD Ryzen AI 9 HX 370），第 4 段在 Windows 11；第 1 到 3 段各自 merge 回 `main`，第 4 段的三段做完時一起 merge。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 文字的 property：`letter-spacing`、`word-spacing`、`text-indent`、`text-transform`、`word-break`、`overflow-wrap`、`white-space` 的 longhand 和 `text-wrap`；`visibility` 的限制修正 | 完成 | 2026-10-13 |
| 2a `vertical-align`（`RangedBuilder` 上，巢狀相對於 block） | 完成 | 2026-10-13 |
| 2b `InlineContent` 改用 parley 的 `TreeBuilder`，巢狀照 CSS | 完成 | 2026-10-13 |
| 3 `text-overflow` | 完成 | 2026-10-13 |
| 4a `float`、`clear` 的 style，taffy 的 `float_layout`，paint 和 hit test，快取 | 完成 | 2026-10-13 |
| 4b 行內內容繞過 float | 完成 | 2026-10-13 |
| 4c inline-level 的 float、自己是 BFC 的 IFC、intrinsic size | 完成 | 2026-10-14 |

## 驗收

### 文字的 property（第 1 段）

| 項目 | 怎麼驗證 |
| --- | --- |
| 解析和序列化：`white-space` 是 shorthand（關鍵字單獨出現，或兩個 longhand 任意順序；有關鍵字的組合讀回關鍵字）、`text-wrap` 只收 mode、`word-wrap` 是 `overflow-wrap` 的別名、`letter-spacing` / `word-spacing` 的 `normal` 和長度、`text-indent` 的長度和百分比、各關鍵字；無效的值（`word-spacing` 的百分比、`text-wrap: balance`、`preserve-spaces`、沒單位的 `letter-spacing`）；`cssText` 把 longhand 併成 `white-space`，`text-wrap-mode` 不併成 `text-wrap` | `cargo test -p style --test it cssom::` |
| computed value：`letter-spacing` 的 `normal` 和 0 是 `normal`、`word-spacing` 的 `normal` 是 `0px`、`text-indent` 的百分比留著、`text-wrap` 是 mode 的值 | `cargo test -p style --test it computed::`；WPT 的 `css/css-text/parsing/*-computed.html` |
| `text-transform`：`uppercase`、`lowercase` 的完整對應（ß → SS）、`capitalize` 每個字的第一個字元（標點之後也是）、接著前一個文字節點的字不算、`<br>` 之後算；展開的字元對到同一段來源 | `cargo test -p layout --lib`（`text::tests::text_is_transformed`、`an_expanded_character_keeps_its_source`）；`cargo test -p layout --test it text_rects::an_expanded_character`、`inline::text_transform` |
| layout：`letter-spacing` 每個字元之後、`word-spacing` 每個空格之後、`text-indent` 只在元素的第一行（百分比對 containing block，匿名 block 不縮）、spacing 算進 intrinsic size；`word-break: break-all`、`overflow-wrap: anywhere`、`word-break: break-word`、`break-spaces`、`text-wrap-mode: nowrap`、`white-space-collapse: preserve` | `cargo test -p layout --test it inline::spacing_and_indent`、`inline::breaking_words_and_spaces`；`python3 scripts/test.py reftest text/`（t10 到 t15） |
| 畫面和 Chrome 一樣 | `python3 scripts/test.py wpt css/css-text/letter-spacing/ css/css-text/word-spacing/ css/css-text/text-indent/ css/css-text/text-transform/ css/css-text/word-break/ css/css-text/overflow-wrap/ css/css-text/parsing/`（預期結果在 `tests/wpt/metadata/`；失敗的原因在下面） |

### `vertical-align`（第 2a 段）

| 項目 | 怎麼驗證 |
| --- | --- |
| 解析、序列化、computed value（關鍵字、長度、百分比留著；css-inline 3 的 `first baseline` 無效）；UA stylesheet 的 `sub`、`sup` | `cargo test -p style --test it cssom::`、`computed::` |
| layout：長度和百分比（`normal` 的 line height 用字型的）、`sub`、`top`、`bottom`，行跟著長高；block 自己的文字不動；atomic inline 和 inline 元素的背景跟著移 | `cargo test -p layout --test it inline::vertical_align`；`python3 scripts/test.py reftest inline/`（i9） |
| 巢狀照 CSS（第 2b 段）：位移逐層疊加、`super` 和 `middle` 用父元素的 `font-size` 和 x-height、atomic inline 跟著它所在的 span 移、被 block 切開的元素在後面的 anonymous block 裡照樣巢狀；`letter-spacing: normal`、`word-spacing: normal` 在有間距的父元素裡是 0 | `cargo test -p layout --test it inline::vertical_align_nests`、`inline::vertical_align_goes_on_after_a_block`、`inline::spacing_and_indent` |
| 畫面和 Chrome 一樣 | `python3 scripts/test.py wpt css/CSS2/linebox/ css/css-inline/parsing/`（`vertical-align*` 的 95 個加 3 個 parsing） |

### `text-overflow`（第 3 段）

| 項目 | 怎麼驗證 |
| --- | --- |
| 解析、序列化、computed value（`clip`、`ellipsis`，不繼承） | `cargo test -p style --test it cssom::`、`computed::` |
| layout：截在省略號放得下的最後一個 cluster、每一行都截、沒超出或不 clip 或 `clip` 的不截、整個在外面的和省略號之後的 run 和 atomic inline 隱藏、第一個 child 留一個 cluster 或整個 atomic inline、匿名 block 照 parent、省略號是被截的元素的 | `cargo test -p layout --test it text_overflow::` |
| `nowrap` 的行開頭的 atomic inline 不讓行在它後面斷（parley 的 fork），元素的邊照舊 | `cargo test -p layout --test it parley_inline_box::a_box_first_on_a_nowrap_line_doesnt_break_it`；WPT 的 `css/CSS2/linebox/inline-formatting-context-012.xht` |
| paint 和 hit test：隱藏的 glyph、裝飾、atomic inline 不畫；點省略號打到被截的元素，隱藏的 atomic inline 打不到 | `python3 scripts/test.py reftest text/t16-text-overflow.html`；`cargo test -p paint --test it hit_test::what_text_overflow_hides_is_not_hit` |
| 畫面和 Chrome 一樣 | `python3 scripts/test.py wpt css/css-overflow/`（`text-overflow*` 的 96 個；失敗的原因在下面） |

### float（第 4a 段）

| 項目 | 怎麼驗證 |
| --- | --- |
| `float`、`clear` 的解析、序列化、computed value（不繼承；`position: absolute`、`fixed` 時 `float` 是 `none`；float 的 `<span>` 是 block）；`cssFloat` 和 `float` 兩個 attribute | `cargo test -p style --test it computed::float_and_clear`、`cssom::`；`cargo test -p dom_api --test it computed_style::` |
| block-level 的 float：左右、好幾個、放不下的往下（盡量高）；`clear`；BFC 盒子在 float 之間；float 不佔 flow 的空間、BFC 的根包住它；flex item 的 `float` 沒有作用 | `cargo test -p layout --test it float::` |
| 局部重排：包著 float 的盒子、float 旁邊的盒子不用 taffy 的快取 | `cargo test -p layout --test it update::a_box_holding_floats_puts_them_there_again`、`update::a_box_next_to_floats_follows_them` |
| paint：float 在 block 的背景之後、整個畫完（連同它裡面的 block），只畫一次；flex item、inline-block 裡的 float 也畫；hit test 照反序 | `python3 scripts/test.py reftest floats/fl1-paint-order.html`；`cargo test -p paint --test it artifact::a_float_is_painted_once_after_the_blocks_backgrounds`、`hit_test::floats_are_hit_before_the_blocks_around_them` |
| 行繞過 float（第 4b 段）：float 旁邊的行從它的邊開始、寬度是剩下的空間，下面恢復全寬；第一個字放不下的行（連同不換行的）移到放得下的地方；行碰到更低的 float 時用更窄的寬度重排；padding 比 float 寬時照 content box；內容的高度到最後一行的底端 | `cargo test -p layout --test it float::`；`python3 scripts/test.py reftest floats/fl2-lines-around.html` |
| float 變了，旁邊的行照新的區段重新斷行（斷行快取的 key 包含區段） | `cargo test -p layout --test it update::lines_follow_a_float_that_changes` |
| 行內的 float（第 4c 段）：放在它那一行的頂端，那一行（連同它前面的內容）和旁邊的行從它的邊開始；剩下的寬度放不下的放在那一行下面，後面的 float 也是；斷在它前面的那一行不算它，它放在下一行；那一行的內容因此溢出時 float 照樣在頂端、行往下移（連同前面已經有的 float）；`padding` 的 content box 是它的 containing block；負 margin 比 0 窄的不占寬度 | `cargo test -p layout --test it float::`；`python3 scripts/test.py reftest floats/fl3-floats-in-lines.html` |
| 行內的 float 在 BFC 裡：後面的 BFC 盒子避開它，自己是 BFC 的 IFC 包住它；每次排版都重新放；裡面的 out-of-flow 盒子從它的位置傳上去 | `cargo test -p layout --test it float::floats_in_lines_are_in_their_block_formatting_context`、`float::floats_in_lines_are_placed_at_every_layout`、`position::absolute_boxes_in_a_float_in_a_line_go_up_from_it` |
| intrinsic size：shrink-to-fit 的盒子包住行和旁邊的 float（直接的、inline-block 裡的），最窄是最寬的 float | `cargo test -p layout --test it float::floats_in_lines_widen_a_shrink_to_fit_box` |
| 局部重排：block 變成 float 時重建、進到前面那段行內內容；中間的 block 拿掉時 float 的盒子跟著進新的匿名 block（不被丟掉） | `cargo test -p layout --test it update::a_block_that_floats_goes_into_the_line_before_it`、`update::a_float_goes_into_the_line_a_removed_block_was_between` |
| paint 和 hit test：行內的 float 在 float 的階段畫、在行之前，atomic inline 照舊在行裡（float 排在 children 的 atomic inline 後面） | `cargo test -p paint --test it artifact::a_float_in_a_line_is_painted_before_the_lines`、`hit_test::a_float_in_a_line_is_hit` |
| 畫面和 Chrome 一樣 | `python3 scripts/test.py wpt css/CSS2/floats/ css/CSS2/floats-clear/`（預期結果在 `tests/wpt/metadata/`；失敗的原因在下面） |

## 做過的檢查

- **做了什麼**：
  - 第 1 段（第 2、4、7、8、9、13 條）：`computed_style` 的 `WhiteSpace` 拆成 `white_space_collapse` 和 `text_wrap_mode`（`WhiteSpace` 留著當 shorthand 的關鍵字），加 `text_transform`、`letter_spacing`（`None` 是 `normal`，0 也是）、`word_spacing`、`text_indent`、`word_break`、`overflow_wrap`，都繼承，匿名 block 的 `text_indent` 是 0。`css` 的 8 個 longhand（`word-wrap` 是別名）和 2 個 shorthand（`white-space`、只有 mode 的 `text-wrap`）、序列化、resolved value。`layout`：這些 property 給 parley（Chrome 的 `word-break: break-word` 是 `overflow-wrap: anywhere`；只有 `break-spaces` 不是 `Preserve`，空白已經在 kun 處理過）；`collapse_white_space` 做 `text-transform`（`icu_segmenter` 找字的開頭，接著前一個文字節點的最後一個字元；`CaseChars` 把完整的 case mapping 逐個推進去，offset mapping 的幾個字元對同一段）；斷行和量寬度之前 `set_text_indent`。`svg_text` 改讀 `white_space_collapse`。
  - 第 2a、2b 段（第 14、15 條）：`computed_style` 的 `VerticalAlign`（八個關鍵字和長度、百分比，不繼承）；`css` 的 `vertical-align`、序列化、resolved value（百分比留著）；UA stylesheet 的 `sub`、`sup`。2a 在 `RangedBuilder` 上設 parley 的 `VerticalAlign`；2b 把 `InlineContent::new` 改用 parley 的 `TreeBuilder`：先列出這段內容裡有邊的 inline 元素和之前就開著的（由外到內），根的 `TextStyle` 是 container 的，tags 和 runs 照 byte offset 合併（run 用 modification span 帶 brush，元素一個 span、帶文字 property 和 `VerticalAlign`，邊和 atomic inline 是 inline box），parley 吐回的文字和給它的一樣（`debug_assert`）。`vertical_align` 把百分比和 `normal` 的行高算成 parley 的長度；`LineBox` 照自己的 run 的 baseline 放背景框。
  - 第 3 段（第 16、17 條）：`computed_style` 的 `TextOverflow`；`css` 的 `text-overflow`。`layout`：`truncates`（匿名 block 看 parent）傳進 `InlineContent::new`，先排好省略號（`ellipsis_fragment`：U+2026 和 `X` 在同一個字型時用它，否則「...」）；`fragments` 照 Blink 的 `LineTruncator` 截（`truncate_line`）；`TextFragment` 多 `hidden_from`、`visible_advance`，`AtomicInline` 多 `hidden`。`paint` 只畫可見的 glyph、裝飾畫到 `visible_advance`、跳過隱藏的 atomic inline，hit test 用可見的寬度。parley 的 fork：`Auto` 的 inline box 用所在 span 的 `text-wrap-mode`（`2d056a98`）。
  - 第 4 段（第 18 到 23 條）：`computed_style` 的 `Float`、`Clear`（不繼承）；`css` 的 `float`、`clear`、`cssFloat`；`style`：out-of-flow 的盒子 `float` 算成 `none`，float 的盒子 blockify。`layout`：taffy 開 `float_layout`；`compute_box` 在有還沒結束的 float、或盒子上一次放了 float（`places_floats`）時不用 taffy 的快取（`lay_out_box`）；`FloatBands` 和 `float_bands`（從 `find_content_slot` 一段段查）；`compute_inline` 收 `BlockContext`，`break_beside_floats` 逐行斷（每一行在它高度範圍內的區段的交集裡，太高、放不下就退回這一行的開頭重排）；`InlineItem::Float`（parley 的 `CustomOutOfFlow`），交回控制權時決定放在這一行旁邊或下面，一行定了才 `place_floated_box`；`InlineChildren` 帶著 `BlockContext`，自己是 BFC 的 IFC 自己建一個、高度包住 float；`content_widths` 加上 float；`BoxFragment::floating`，IFC 的 float 在 fragment 的 children 裡排在 atomic inline 後面；`StyleDifference` 在 `float` 變成或不再是 `none` 時重建；`collect_again` 不丟掉進到新的匿名 block 的盒子。`paint` 的 float 階段（CSS 2 附錄 E 第 5 步，每個 float 整個畫完）和 hit test 的反序。parley 的 fork：`LineBreakData::hanging_advance`（`43db46d`）。
  - `about:demo`：被截的句子、繞過綠色方塊的文字和一行 `clear: left`。
  - 文件：`docs/limitations.md`（拿掉：沒有 float、沒有 `text-indent` 等、只有 `vertical-align: baseline`，Hit testing 的 `visibility` 那一條本來就過時；加上：`text-transform`、`white-space-collapse`、`text-wrap`、`sub` / `super` 的位移、`text-overflow`、float 的限制）；`docs/test-gaps.md` 的「等價、不用處理的 mutation」；給 taffy 的 issue 草稿 `taffy-float-root-inset-report.md`（還沒送出）。
- **找到並修的**：
  - 第 1 段：WPT 看到、記進限制的：`overflow-wrap: break-word` 加 `word-break: keep-all` 時 parley 不斷、`letter-spacing` 行尾的那一份和 css-text 4 的 trim 不同（照 Chrome）。
  - 第 2a 段：block 自己的文字也套了 block 的 `vertical-align`（WPT 的 `vertical-align-sub-001`，`position: absolute` 的 span 是 block），拿掉。2b：元素的 span 從父 span 複製屬性，`letter-spacing`、`word-spacing` 只在不是 0 時才設的話 `normal` 會繼承父元素的，改成一律設；fork 的 `InlineBoxLineBreak` 原本只有 parley 層級的測試，補了 kun 層級的。
  - 第 3 段：WPT 的 `text-overflow-009` 查到 parley 在 `nowrap` 的行開頭的 inline box 後面換行（第 17 條）；第一版連元素的邊也改，`inline-formatting-context-012` 回歸，改成只對 atomic inline。text-transform 的四個測試第一次看起來通過，是假象（span 之間都不換行），查清楚了。並行的 WPT log 交錯讓結果認錯測試，改成讀 `target/wpt-results/raw.log`。
  - 第 4 段：
    - 「行放不下就往下移」不能看 parley 交回的 `advance`（含行尾掛出去的空白）：4b 改成整段斷完看 `metrics`、整段重斷；4c 的 float 放進 BFC 就收不回，和整段重斷衝突，改 fork 給 `hanging_advance`、逐行判斷（第 23 條）。
    - 內容的高度用最後一行的底端時，沒照 parley 略過空的最後一行：`css-text/text-indent/text-indent-overflow`（沒有 float）變高；改成只在有區段時這樣算、略過空的最後一行。
    - shrink-to-fit 的 IFC 在可用寬度斷行，float 放在 x 700：有 float 時改在 fit-content 寬度斷（沒有 float 時照舊，否則 atomic inline 多排一次、fragment 留不住）。
    - WPT 抓到：負 margin 讓 float 的 margin box 比 0 窄，行比 layout 的上限寬，parley 的斷言失敗；block 拿掉之後 float 或 out-of-flow 盒子進到新的匿名 block，`collect_again` 把它的盒子丟掉、被重用，當掉；float 後面的內容溢出時第一版把 float 放下面，Chrome 是 float 留在頂端、行往下移。
    - `dom_js` 的四個測試拿 `float` 當不支援的 property（4a 換了 `style`、`dom_api` 的，漏了這四個），整個 workspace 到第 4 段做完才跑、才看到，換成 `tab-size`。
    - `css/CSS2/floats/crashtests/huge-width-after-float.html` 卡住：不是 float，是 compositor 替整個 layer 一次建所有 tile（已知的限制），停用。
    - clippy：兩處 `filter().last()`（第 3 段找 cluster、4b 找最後一個不空的行），改成從尾端找。
  - 每段都有幾次測試的預期值算錯（例如行高要加 strut 的 descent、放不下的 float 盡量高），照實際的排版和規則改正。
- **測試**：`style` 的 `cssom::`、`computed::`（各 property 的解析、序列化、computed value，`text_properties_compute`、`float_and_clear`）；`layout` 的 `inline::`（`spacing_and_indent`、`breaking_words_and_spaces`、`text_transform`、`vertical_align`、`vertical_align_nests`、`vertical_align_goes_on_after_a_block`、`edges_keep_to_their_content`）、`text::tests::`、`text_rects::an_expanded_character`、`text_overflow::`（五個）、`parley_inline_box::a_box_first_on_a_nowrap_line_doesnt_break_it`、`float::`（20 個）、`update::`（float 的五個）、`position::absolute_boxes_in_a_float_in_a_line_go_up_from_it`；`paint` 的 `hit_test::what_text_overflow_hides_is_not_hit`、`artifact::` 和 `hit_test::` 的 float 各兩個；reftest `text/` 的 t10 到 t16、`inline/i9-vertical-align`、`floats/` 的 fl1 到 fl3。
- **mutation test**（每段一份 `mutants-*.toml`，結果都是最後一次的）：
  - `mutants-1.toml` 18 條、`mutants-2a.toml` 12 條（兩條用 reftest）、`mutants-2b.toml` 11 條：全部抓到；第一次沒抓到的「邊不跟著內容」補了 `edges_keep_to_their_content`。
  - `mutants-3.toml` 19 條（含 fork 的 3 條）：全部抓到；paint 的兩條第一次沒抓到（reftest 裡被藏的字都在省略號底下，spec 的 `reftest text/t16` 也沒對到檔案），reftest 加寬、spec 改成檔名之後抓到。hit test 用可見或完整的寬度等價。
  - `mutants-4a.toml` 18 條、`mutants-4b.toml` 9 條：全部抓到；補了數 display item 的 `artifact::` 測試和「float 前面有 15px 的盒子」的案例。hit test 的背景階段不跳過 float、區段不照 content box 的 padding 等價。
  - `mutants-4c.toml` 31 條：30 條抓到，1 條卡住（同一行一直重排，算抓到）；補了 `float::a_line_too_long_beside_its_float_goes_past_the_floats_before_too`。只有 float 的內容也照行內內容排、放 float 時也用斷行快取等價；「float 的大小變了就清掉斷行快取」等價，連同那段程式拿掉。
  - 等價的都記在 `docs/testing.md`。
- **WPT**（`include.txt` 加上 `css/css-text/` 的 `letter-spacing/`、`word-spacing/`、`text-indent/`、`text-transform/`、`word-break/`、`overflow-wrap/` 和 `parsing/` 的 29 個、`white-space/` 的 27 個，`css/CSS2/linebox/` 的 95 個 `vertical-align*`，`css/css-inline/parsing/` 的 3 個，`css/css-overflow/` 的 96 個 `text-overflow*`，整個 `css/CSS2/floats/` 和 `floats-clear/`；預期結果用 `MACHOPO_UPDATE_EXPECTATIONS` 寫下，float 的每個 `.ini` 寫了原因）：
  - 文字的 property：整個通過的 letter-spacing 6/35、word-spacing 1/10、text-indent 17/28、text-transform 79/113、word-break 45/105、overflow-wrap 25/57、parsing 18/34。失敗的大多是別的限制：`ch`、`ex` 單位（word-break 和 overflow-wrap 的 63 個）、`::after`、`@font-face`、`lang` 的 tailoring、`innerText`、window 的 named access、animation、`tab-size`、`text-indent` 的 `hanging` 和 `each-line`。`letter-spacing-200` 到 `212`（容器 `float: left`）在 float 做完之後照舊失敗：`200` 到 `206` 測的是行頭、行尾和元素邊界的 `letter-spacing`（css-text 3 說不加在行的頭尾），kun 照 Chrome 在每個字元之後都加，Chromium 也失敗（wpt.fyi 上 Edge 157 是 FAIL）；`211`、`212` 是 `text-emphasis` 和 ruby，kun 沒有；`text-indent/below-float3` 是 `::first-line`；`text-indent-percentage-001` 的 diff 只有幾個像素，還沒查。`white-space/` 整個目錄 455 個只有 108 個整個通過（`ch`、`tab-size`、字型、`::before`、bidi），只加了通過、和這份有關的 27 個。
  - `vertical-align`：`vertical-align-*` 48/52、`applies-to` 12/13、`baseline` 6/10。失敗的是 `ex` 單位、table、`table-cell`、window 的 named access、css-inline 3 的新值；`sub-001`、`super-001` 只差 1px 的水平位置（絕對定位的 span 的 static position）；`baseline-004a`、`005a` 在 RFC 0044 之後通過，`008` 還沒查。
  - `text-overflow`：27/96 整個通過。失敗的：`<string>` 值（Chrome 在 flag 後面）、bidi 和直排、`::before` / `::after`、`ch` 單位（含 `006`）、`::first-line`、ruby、編輯和游標、window 的 named access（`scroll-*` 因此 timeout）、`document.fonts`、要點擊的（`017`）。`012`、`022`（float 撐出寬度、裡面絕對定位的盒子截行）截得對，畫面只差十幾個像素的綠色 1 級：截行的盒子是 `overflow: hidden`、內容溢出的捲動容器，kun 讓它成為自己的 layer，文字先 raster 在透明的底上再疊上去，捨入和直接畫在背景上的不同（reference 加上 `overflow: hidden` 和讓內容溢出的寬度就畫得和測試一樣，不溢出的就和原本的一樣）；Chrome 不讓使用者捲不動的容器成為 layer。
  - float（`floats/` 148 個、`floats-clear/` 217 個測試）：失敗的照原因分：table（51 個）、直書、`direction: rtl`、`flow-root`、`ch` / `ex`、multicol、`::before`、XHTML（`<span/>`、`svg:svg`）、animation、window 的 named access、分開 shape 的 kerning、taffy 的三個限制（BFC 的根沒加下方的 padding 和 border、BFC 盒子只看頂端那一段的 float、寬或高是 0 的 float 和負 margin）；`float-replaced-height-002`、`005`、`006`、`floats-031`、`039` 只看了測試內容，沒有逐一查到根本原因。4c 讓 16 個變通過，2 個之前碰巧通過的變失敗（`float-nowrap-3-ref` 是 `ch`、`float-table-align-left-quirk` 是 table）。
  - 其他目錄：4b 和 4c 各把 `css/` 底下相關的目錄整批再跑一次（4b 38 個、4c 19 個）。變通過的：css-text 的 `overflow-wrap-min-content-size-002`、`overflow-wrap-shaping-001`、`002`、`word-break-min-content-006`、`text-indent/below-float`、`below-float2`，cssom 的 `float`、`clear` 的序列化和 `cssstyledeclaration-setter-declarations`，cssom-view 的 `scrollIntoView-*` 的幾個子測試；`css-text/letter-spacing/letter-spacing-ligatures-003` 4b 時變失敗，查清楚是 `text-align: justify` 等和 `ch`。
- **效能**：2b 用 Speedometer 的 jQuery 和 ES5 量過一次（`perf` profile，2a 和 2b 輪流各量四次）：suite 的差跟著量的順序走，`perf` 的 profile 和 kun 的 trace 裡 inline layout 的時間兩邊都不到 10 ms，差在 JS 的時間，結論是量不出 2b 的成本。之後照新的規則（只在使用者要求時量），沒有再量。
- **跑過的**：每段做完跑改到的 crate 的測試、`sanity.py`、相關的 reftest 和 WPT；第 1、2a、3 段和第 4 段做完時跑 `cargo test --workspace --no-fail-fast`（最後一次 2026-10-14，Windows 11，43 個 binary 全部通過）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py reftest`（4b 時全部 158 個，0 個預期外；4c 跑 `floats/`）；`test.py scenarios`（第 3 段載入 `about:demo` 的 11 個，4b 全部 50 次，0 個失敗）。
- **沒跑的**：`test.py automation`（沒有動到視窗那一層）；`cargo clean` 在 merge 回 `main` 之後跑。
