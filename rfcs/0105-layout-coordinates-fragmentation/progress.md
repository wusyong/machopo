# RFC 0105 的進度：layout 的座標模型和分段

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-19

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 型別和 converter | 完成 | 2026-11-19 |
| 1b inline 的行在 box 裡的位置 | 完成 | 2026-11-19 |
| 1c table，連 collapsed border | 完成 | 2026-11-19 |
| 1d float、list marker、line clamp、置中、slider 的 thumb | 完成 | 2026-11-19 |
| 1e 第 1 階段的檢查和文件 | 完成 | 2026-11-19 |
| 2a style：`direction`、`unicode-bidi`、`dir`、`:dir()` | 完成 | 2026-11-19 |
| 2b 邏輯屬性照 `direction`、layout 用 `direction` | 完成 | 2026-11-19 |
| 2c `unicode-bidi` 在 inline 內容裡 | 完成 | 2026-11-19 |
| 2d 捲動的原點、sticky、`text-overflow`、`<select>` | | |
| 2e 第 2 階段的檢查和文件 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 邏輯和物理的換算在每種 writing mode 和 direction 都對（位移、大小、rect、四邊、距離、內容區的起點） | `cargo test -p layout --lib logical::` |
| 第 1 階段之後，橫排、`ltr` 的排版和改之前一樣 | `cargo test -p layout`、`cargo test -p paint`；`python3 scripts/test.py reftest inline/ text/ block/ position/ tables/ floats/ generated/`；`python3 scripts/test.py wpt css/css-tables/ css/CSS2/tables/` |
| collapsed border 給 paint 的格線和每條邊 | `cargo test -p layout --test it tables::collapsed_borders_lines_and_edges` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a：型別和 converter

macOS、Apple M4，2026-11-19。

- 新的測試：`layout` 的 `logical::` 的單元測試 4 個：5 種 writing mode 乘 2 種 direction 的位移、大小、rect 轉成物理的再轉回來，和四邊的對應（CSS Writing Modes 4 § 6.4 的表）；位移的預期值照 Blink 的 `WritingModeConverter`。
- mutation test（`mutants-1a.toml`）9 個，全部抓到：`rtl` 的橫排沒鏡射、從遠的那一邊量時沒扣掉內層的大小、`sideways-lr` 的 inline-start 放在上面、四邊從錯的那一邊讀、轉回物理時 block-start 和 block-end 對調、直排的大小沒對調、轉回邏輯時沒換回軸、rect 用邏輯的大小轉位移、inline 的和加到 block 的那一邊。
- `ComputedStyle::writing_direction()` 這一段還沒有程式用（一律回答橫排、`ltr`），弄壞它沒有東西會變：1b 開始用時一起測。
- 跑過：`cargo test -p layout --lib logical::`、`cargo test -p computed_style`；`sanity.py numerics`、`tests`、`unsafe`、`deps`、`docs`。

### 1b：inline 的行在 box 裡的位置

macOS、Apple M4，2026-11-19。

- inline 的內容區的起點（排行的 `inline_fragments`、往上交的 out-of-flow box 的靜態位置 `passed_up`、第一行的 baseline `first_line`）改由 `BoxStrut` 和 converter 算（`content_box_origin`）。
- 新的測試：`logical::` 的 `content_box_origin` 在 10 種組合的位置；`position::absolute_boxes_go_up_from_inside_an_atomic_inlines_padding`（上下 padding 不同的 inline-block 往上交的靜態位置）、`lists::an_outside_marker_is_below_the_items_padding_and_border`（有上 padding 和 border 的 list item 的 marker）。後兩個是 mutation test 沒抓到之後補的。
- mutation test（`mutants-1b.toml`）9 個：抓到 8 個（補了上面兩個測試之後）；1 個等價：`passed_up` 用別的 writing mode，內容區的左上角在每種 writing mode 都一樣，記在 `docs/test-gaps.md`。`ComputedStyle::writing_direction()` 回答直排時有測試失敗（1a 沒測的那一條）。
- 跑過：`cargo test -p layout`（472 個）、`cargo test -p computed_style`；`test.py reftest` 的 `inline/`、`text/`、`block/`、`position/`、`tables/`、`floats/`、`generated/`。

### 1c：table，連 collapsed border

macOS、Apple M4，2026-11-19。

- table 的 cell 放進 row、row 放進 row group 時用邏輯的位移馬上換成物理的；caption 和 row group 先記下邏輯的位置（`Pending`），table 的大小定了再一起換。out-of-flow box 的靜態位置、`vertical-align` 的位移（`shift_content`，也是按鈕的置中用的）、欄的背景經過 converter。`TableBorders` 的邊改成 inline-start、block-start，每條邊取哪一個邊框照 table 的 writing direction；`collapsed_borders` 把邏輯的格子翻轉、轉置成給 paint 的物理格線，paint 不用改。
- 新的測試：`logical::` 的 `to_physical_delta` 在 10 種組合；`tables::` 的 collapsed border 的格線和每條邊、欄的背景跨過 row group、caption 的上下 margin、caption 裡和 table 自己的 out-of-flow box 往上交、有 padding 的 inline-table 的 baseline。後五個是 mutation test 沒抓到之後補的。
- mutation test（`mutants-1c.toml`）20 個，全部抓到（第一輪 9 個沒抓到，都是 layout 的測試沒有直接檢查的資料：collapsed border 的格線、欄的背景、caption 的 margin 和 out-of-flow box、table 自己的 out-of-flow box、baseline；補了上面的測試之後抓到）。
- 找到的遺漏：第 4 條要記進 `docs/test-gaps.md` 的 `rtl` 和直排的路，1a、1b 時漏記了，這一段補上（「只靠審查」）。
- 跑過：`cargo test -p layout`（480 個）、`cargo test -p paint`；`test.py reftest tables/`（11 個）、`test.py wpt css/css-tables/ css/CSS2/tables/`（1665 個，和預期一樣）。

### 1d：float、list marker、line clamp、置中、slider 的 thumb

macOS、Apple M4，2026-11-19。

- list marker 照第一行的起點和 baseline（沒有行時照內容區的 inline-start、block-start）用邏輯的位移，經過 list item 的 converter；float 從內容區的起點（`content_box_origin`）放；按鈕和文字欄位的內容沿 block 方向置中；range 的 thumb 沿 inline 方向移；line clamp 的位置、每個 block 的 block size 和容器的 block size 沿 block 方向（容器自己的大小用它的 output，它的 layout 還沒設）。atomic inline 和 float 在行間的位置是 parley 給的，屬於行裡面那一層（第 5 條），沒有換。
- 新的測試：`lists::an_outside_marker_without_a_line_is_at_the_content_box`、`form_controls::a_buttons_block_children_are_centered_with_their_margins`、`line_clamp::nested_blocks_end_with_the_clamped_line_from_their_own_tops`、`paint` 的 `theme::a_ranges_thumb_keeps_to_its_tracks_content_box_and_its_margins`（thumb 的位置只有 paint 看得到：值的那一段畫到 thumb 的中間）。都是 mutation test 沒抓到之後補的。
- mutation test（`mutants-1d.toml`）18 個，全部抓到（第一輪 6 個沒抓到，補了上面的測試之後抓到；range 的三個用 `paint` 的測試）。
- 跑過：`cargo test -p layout`、`cargo test -p paint`；`test.py reftest floats/ generated/ block/ inline/`（24 個，和預期一樣）；`test.py wpt css/css-overflow/ css/css-lists/ css/CSS2/floats/ html/rendering/widgets/`：66 個結果和預期不同：整份的 5 個（`text-overflow-012`、`text-overflow-022` 失敗，`CSS2/floats/new-fc-beside-adjoining-float`、`CSS2/floats/new-fc-separates-from-float-2`、`text-overflow-020` 意外通過），subtest 61 個（`button-layout/display-other` 和 `baseline-alignment-and-overflow.tentative` 意外通過）；這 7 份在 `main`（`2e74f30`，0105 之前）上跑結果完全一樣：這台 macOS 和預期結果對不上，不是這份 RFC 造成的，沒有在這裡改預期結果。

### 1e：第 1 階段的檢查和文件

macOS、Apple M4，2026-11-19。

- `docs/architecture.md`：「要注意的地方」加上 layout 的座標（新的 layout 功能用 inline 和 block 的方向寫、子元素在 box 的大小定了才換成物理的、writing direction 只從 `writing_direction()` 拿），「和 Chromium 刻意不同的地方」加上換算的位置。
- `cargo test --workspace --no-fail-fast`：55 個 test binary 通過，`shell` 的 `web_animations::ready_then_the_end` 在整套一起跑時失敗一次（單獨跑 5 次都通過）：`finish` 先、`transitionend` 後。原因（和這份 RFC 無關，`animation` 原本的）：動畫在離結束 1µs 以內就算結束（`reached`，照 Blink 的 `IsWithinAnimationTimeTolerance`），這時 `finish` 的事件時間夾到 `now`（`Animation::notify`），比結束時間早一點，`transitionend` 的是結束時間本身、沒有夾，排序（時間先、transition 在 playback 之前）就把 `finish` 排到前面；frame 落在結束前 1µs 以內時才會，負載下 frame 的時間不規則。暫時的測試確認：結束前 0.0004ms 的 frame，`finish` 是 1299.9996、`transitionend` 是 1300.0。修法另外做（在 `main` 上，不在這份 RFC）。
- `cargo fmt --check` 通過；`cargo clippy --workspace --all-targets -- -D warnings`：`WritingModeConverter` 的 `to_*` 改成取 `self`（它是 `Copy`），之後通過。

### 2a：style（第一部分：`direction`、`unicode-bidi` 的解析和 computed value）

macOS、Apple M4，2026-11-19。2a 分三部分做：兩個 property；`dir` 的 hint、`<bdi>`、`<bdo>`、`dir=auto` 的方向；DOM 的 directionality 快取和 `:dir()`。

- `direction`（繼承，在 `RareInherited`）、`unicode-bidi`（不繼承，在 `ComputedStyle` 本身：UA 樣式給多數 block 元素 `isolate`，放在 `RareNonInherited` 的話每個都要複製那一組，第二部分時 `style_groups` 的測試抓到之後搬的；6 個值和 Chrome 還收的 `-webkit-isolate`、`-webkit-isolate-override`、`-webkit-plaintext`）：解析、computed value、序列化、不能動畫、`::marker` 收它們。邏輯屬性的對應和 layout 還不看它們（2b）。
- WPT：`css/css-writing-modes/parsing/` 的 `direction-*`、`unicode-bidi-*` 6 份加進 `include.txt`，全部通過；`marker-supported-properties`、`marker-supported-properties-in-animation`、`serialize-values` 的 14 個 subtest 和 `serialize-all-longhands` 變成通過，更新了預期結果。
- 新的測試：`style` 的 `computed::direction_inherits_and_unicode_bidi_doesnt`（繼承、`unset`、`inherit`、舊的名字）。
- mutation test（`mutants-2a1.toml`）9 個，全部抓到（第一輪「`direction` 不算繼承的」沒抓到：繼承是整組 `RareInherited` 一起複製的，`is_inherited` 只影響 `unset`，補了 `direction: unset` 之後抓到）。
- 跑過：`cargo test -p css -p computed_style -p style -p style_resolver -p dom_api`；`test.py wpt css/css-pseudo/ css/cssom/`。

### 2a：style（第二部分：`dir` 的 hint 和 UA 樣式的 bidi 規則）

macOS、Apple M4，2026-11-19。

- `style` 的 `hints.rs` 的 `dir_declarations`：每個 HTML 元素的 `dir`，`ltr`、`rtl` 給 `direction`，不合法的值在 `<body>` 給 `ltr`，`auto` 給 `unicode-bidi`（`isolate`；search、tel、url、email 的 `<input>` 給 `plaintext`；`<bdo>`、`<textarea>`、`<pre>` 的由 UA 樣式給）。`dir=auto` 照 directionality 給的 `direction` 在第三部分（要 directionality 的快取和失效）。
- UA 樣式（`style_resolver` 的 `html.css`）照 Blink 的 `html.css` 加上 bidi 的規則：一串 block 元素、`<bdi>`、`<output>`、`[dir=ltr i]`、`[dir=rtl i]`、`[dir=auto i]` 是 `isolate`，`<bdo>` 是 `isolate-override`，`dir=auto` 的 `<textarea>`、`<pre>` 是 `plaintext`，`input[type=tel i]` 是 `direction: ltr`；`::marker` 照 HTML 的 UA 樣式是 `isolate`（拿掉了「`::marker` 沒有 `unicode-bidi`」的偏差）。
- `unicode-bidi` 從 `RareNonInherited` 搬到 `ComputedStyle` 本身：UA 樣式給多數 block 元素 `isolate` 之後，`style_groups::a_group_is_shared_until_one_of_its_properties_is_set` 失敗（每個 `div`、`p` 都複製了 rare 那一組）。
- WPT：`html/rendering/bidi-rendering/unicode-bidi-ua-rules.html`（130 個 subtest）、`slot-direction.window.js`、`html/rendering/non-replaced-elements/flow-content-0/search-styles.html` 加進 `include.txt`，全部通過；`css/css-pseudo/marker-default-styles.html` 的 8 個 subtest 變成通過，整份通過了，`.ini` 拿掉。`bidi-rendering/slot-no-isolate-001.html`（reftest）要 layout 做 bidi，還沒加。
- 新的測試：`style` 的 `hints::dir_maps_to_direction_and_unicode_bidi`。
- mutation test（`mutants-2a2.toml`）10 個，全部抓到（先把測試裡幾個元素搬到 `rtl` 的元素裡：在 `ltr` 的 parent 底下，`dir=ltr`、`input[type=tel]` 的 `ltr` 拿掉了也一樣）。
- 跑過：`cargo test -p style -p css -p style_resolver`；`test.py wpt html/rendering/ css/css-pseudo/`（另外 61 個意外通過的 subtest 是 macOS 上已知的、和這裡無關的那兩份）。

### 2a：style（第三部分：directionality 的快取、`:dir()`、`dir=auto` 的方向）

macOS、Apple M4，2026-11-19。

- `dom` 的 `directionality.rs`：directionality 快取在 node 的 padding 裡的那個 byte（原本只放 layout 的一個旗標，改名 `more`），連同「自己或祖先是 `auto`」的旗標（Blink 的 `CachedDirectionality`、`kSelfOrAncestorHasDirAutoAttribute`）。插入、移除、`dir` 改變時從那個元素往下重算，方向沒變的地方停下；`auto` 的元素底下的文字改了（插入、移除、`data_changed`）往上找最近的 `auto` 元素重算；表單控制項的值（`state_changed` 的 `VALUE`、`value` attribute）和 `<input>` 的 `type` 也是。沒有 `dir`、沒做過 `<bdi>` 的頁面（`form::DIRS_SEEN`）只多看一個 word。`dir=auto` 的 `<input>`（text、search、tel、url、email、password、hidden 和按鈕類）、`<textarea>` 照值；沒有合法 `dir` 的 telephone 欄位是 `ltr`；shadow tree 的最上層和裡面的 slot 照 host。方向變了的元素：`:dir()` 的狀態改變（`AttributeChanges::DIRECTIONALITY`，Blink 的 `PseudoStateChanged(kPseudoDir)`），`auto` 的再整個重算（hint 的 `direction`）；`dir` 自己改了的走 attribute 的路（子樹和兄弟本來就重算）。
- `:dir()`：照 Blink 收任何一個 identifier、照寫的序列化，`ltr`、`rtl` 才比對得到；失效照 `:lang()`（`dir` 改了重算子樹），其他改變方向的路由 DOM 標記。
- `dir=auto`（和沒有合法 `dir` 的 `<bdi>`）的 hint 照 directionality 給 `direction`（Blink 的 `StyleResolver::MatchPresentationalHints`）。
- 測試找到的錯誤：host 的方向改了，往下走到方向沒變的元素就停，裡面的 slot（照 host、不照 parent）沒有跟著改；改成 host 的方向變了時把它的 slot 也排進去（`slots_of`）。改 `dir` 的路原本先把元素標成整個重算，attribute 的路就以為不用再標兄弟，`:not(:dir(ltr)) + div` 沒更新（WPT 的 `dir-style-03b`）；改成方向的改變當成 `:dir()` 的狀態改變、`dir` 的路不另外標。
- WPT：HTML 的 directionality 和 `:dir()` 的 127 份（`dir-tests.txt`）加進 `include.txt`，282 項照預期：沒過的寫在 `.ini`：`innerText` 的 setter（machopo 沒有）、巢狀 `<span>` 那一行和參考頁差幾個像素（`dir-style-01a`、`01b`）、slot 的 `dir=auto` 不看 assigned node（`docs/limitations.md`）、layout 還沒有 bidi（`dir_auto-*` 的 reftest，2b、2c）。
- 新的測試：`dom` 的 `directionality::`（7 個：繼承、移動、`auto` 的文字、`<bdi>`、表單控制項、shadow tree 和 slot、標記 style）；`style` 的 `hints::dir_auto_is_its_texts_direction`（兩個只差文字的 `dir=auto` 不共用 style、文字改了之後的兄弟規則、telephone 欄位改 `type` 之後的兄弟規則）。
- mutation test（`mutants-2a3.toml`）16 個，全部抓到（第一輪 3 個沒抓到：slot 的那個補了測試，也因此找到上面的錯誤；另外兩個是多餘的程式，拿掉了：matched properties cache 的 hash 加上方向（比對時 hint 本來就逐一比內容），`:dir()` 在組合子左邊也收狀態（每條改變方向的路本來就讓後面的兄弟重算：`auto` 的元素整個重算、祖先是 `auto` 的那棵子樹整個重算、`dir` 和 telephone 欄位的 `type` 走 attribute 的路））。
- 跑過：`cargo test -p dom -p style -p dom_api`；`test.py wpt` 的 `dir-tests.txt`、`html/canvas/element/text/`、`html/canvas/offscreen/text/`。

### 2b：邏輯屬性照 `direction`（第一部分：style）

macOS、Apple M4，2026-11-19。

- `css` 的 `LonghandId::physical_for`：照 writing direction（四種 writing mode、兩種 direction）把邏輯屬性對應到物理的（Blink 的 `CSSDirectionAwareResolver`）；`physical()` 是 `ltr` 橫排的那個，只用在分類（型別、logical group）。
- cascade（`style_resolver` 的 `cascade_declarations`）：先照 parent 的方向對應，元素自己的 `direction` 不同、又有邏輯屬性的宣告時，照自己的方向再 cascade 一次（Blink 先 cascade `direction`；只有方向和 parent 不同、又有邏輯屬性的元素多做一次）。`apply` 改成收物理的 id。`getComputedStyle` 的邏輯屬性（`physical_property`、computed value）、keyframe（`KeyframeEffect::new`，照元素的 base style；base 換了 effect 就重建）和 `transition-property` 也照元素的方向。
- UA 樣式：inline 方向的照 Blink 改回邏輯屬性（`dd`、清單、`blockquote`、`figure`、`fieldset`、`legend`、search 欄位的清除鈕、`optgroup` 裡的 option）；block 方向的照舊寫成上下（只有橫排）。
- WPT：`css/css-logical/` 的 27 個 `direction: rtl` 的 subtest 變成通過，更新了 7 份 `.ini` 和 8 份的說明。`css/css-transitions/`、`css/css-animations/`、`web-animations/`（8723 項）、`test.py reftest generated/ block/` 照預期。
- 新的測試：`style` 的 `logical::inline_start_is_the_right_in_rtl`（元素自己的和繼承的 `rtl`、圓角、CSSOM、UA 樣式的清單和 `<dd>`）、`css_animations::a_flow_relative_keyframe_animates_by_the_elements_direction`、`css_animations::a_flow_relative_transition_is_by_the_elements_direction`。keyframe 的那個測試先找到了錯誤：keyframe 在 `KeyframeEffect::new` 照 `ltr` 換成物理的。
- mutation test（`mutants-2b1.toml`）10 個，全部抓到（第一輪動畫的 `visit` 那個沒抓到：它收到的已經是物理的 id，改回原本的 `physical()`、拿掉那一條；highlight 的 cascade 也只看非邏輯的屬性，照舊用 `physical()`）。
- 跑過：`cargo test -p css -p style -p style_resolver -p animation -p dom_api -p layout`。

### 2b：layout 用 `direction`（第二部分）

macOS、Apple M4，2026-11-19。

- `ComputedStyle::writing_direction()` 讀 `direction`（橫排）：第 1 階段寫好的 `rtl` 那一半開始用到（table 的欄從右排、collapsed border 翻轉、range 的 thumb 從右邊開始、float 和 inline 內容區的起點）。
- parley 的段落方向照 inline 內容所在的 block 的 `direction`：原本沒設，是 parley 的 `Auto`（第一個強方向的字），`ltr` 的頁面裡希伯來文的段落被排成 `rtl`，和 CSS、Chrome 不同。`text-align: start`、`end` 跟著段落的方向。
- taffy 的 `Style::direction` 照 `direction`；從 inline 內容往上交的、table 自己的 out-of-flow box 的靜態位置照 containing block 的方向對齊（原本寫死 `ltr`），`self-start` 照它自己的。
- outside marker 放在第一行的 inline-start（`rtl` 是行的右邊，`InlineContent::first_line`）。
- 改的測試：`inline::a_line_breaks_after_a_soft_hyphen_with_a_hyphen` 的 `rtl` 段落原本靠開頭的 RLM 讓 parley 自動判斷，改成寫 `direction: rtl`（照 CSS，那一段本來是 `ltr` 的段落）。
- 新的測試：`layout` 的 `direction::`（段落的方向是 block 的、flex 和 table 從右排、靜態位置在右邊）、`lists::an_outside_marker_is_on_the_right_in_rtl`。
- WPT（`css/CSS2/`、`css-text`、`css-tables`、`css-lists`、`css-position`、`css-flexbox`、`css-grid`、`css-align`、`css-logical`、`html/rendering/` 和 `dir-tests.txt`，23989 項）：變成通過的 `css-tables/paint/col-paint-htb-rtl`（第 1 階段 table 的 `rtl` 第一次有真的測試）、`CSS2/floats/floats-wrap-bfc-with-margin-005`、`css-lists/list-style-type-string-002`、`003`、`007`、`input-text-overflow-ellipsis-rtl-scrolled`，拿掉 `.ini`；變成失敗的 `html/rendering/widgets/input-text-overflow-ellipsis-rtl`：值從左邊溢出，`text-overflow` 只截右邊溢出的，和參考頁一樣（mismatch），標成預期失敗，`text-overflow` 照方向截斷在 2d。8 份 `.ini` 的說明原本寫 machopo 沒有 `rtl`，看過之後改成實際的原因（被 float 切開的字分成兩段排、multi-column、`unicode-bidi`、直排、marker 的位置還沒查）。另外兩份意外通過（`floats-clear/clear-on-parent-and-child`、`margin-padding-clear/margin-collapse-009`）在上一個 commit 就是，是 macOS 上已知的不一致。
- `docs/test-gaps.md`「只靠審查」的那一條只剩直排；`docs/limitations.md` 的 table 那一條拿掉「欄一律從左排」那一句。
- mutation test（`mutants-2b2.toml`）7 個，全部抓到。
- 跑過：`cargo test -p layout -p paint`；`test.py reftest text/ inline/ tables/ floats/ position/`（34 個）。

### 2c：`unicode-bidi` 在 inline 內容裡

macOS、Apple M4，2026-11-19。

- 建 inline 內容時照元素的 `unicode-bidi` 和 `direction` 在它的開頭、結尾插入 bidi 控制字元（block 的 `bidi-override`、`isolate-override` 套在整段上），強制換行前後關掉、重開，一段裡沒有字元的元素不插（第 12、16 條）；控制字元是自己的 run，沒有 DOM 的來源，不畫、不算內容，text 節點的對應跳過它們（插在一個 run 中間時切開那個 run）。`unicode-bidi: plaintext` 的 block 的段落方向是 parley 的 `Auto`（第 14 條）。
- 行內元素的邊界照元素的方向給寬度（第 13 條），parley 排好一行之後照視覺順序重新擺每個 glyph run、atomic inline 和行內的 out-of-flow 盒子，盒子在這一行最左邊的項目前面加它左邊的邊界、最右邊的後面加右邊的（第 15 條）。先試過 A（改 parley 的 fork，開頭的邊界拿後面那段內容的 level，照 level 的奇偶決定邊界在哪一邊）：bidi 的 WPT 結果一樣，但內容和元素方向相反的盒子跨行時邊界在相反的一邊、巢狀而且方向混合的 embedding 拿錯 level，改成 B，fork 的 commit 沒有 push、丟掉了。
- `::placeholder` 的 UA 樣式加上 `direction: inherit !important`（Blink 的 `html.css`）：2b 讓 layout 照 `direction` 之後，`css-pseudo/placeholder-excluded-properties` 的 placeholder 跟著頁面的 `direction: rtl` 靠右。
- 補測試時找到、修好的：強制換行沒有關掉、重開 embedding（`<br>` 之後的內容掉出元素的 isolate）；只有控制字元的行有一行的高度（只放 out-of-flow 盒子的 `<span dir=rtl>`、`<br>` 結尾的 `<span dir=rtl>` 之後多一行）；保留的換行在 run 中間時，插入的控制字元落在 run 裡面，切 run 的字串時 panic（WPT 的 `content-175`、`marker-content-018`、`firefox-bug-1904419` crash）；行內的 out-of-flow 盒子的靜態位置照 parley 放的位置，不照重擺之後的。
- 新的測試：`layout` 的 `direction::`（`unicode-bidi` 的每個值、block 的 override、`plaintext`、邊界在它那一邊、跨行的 `rtl` 盒子、控制字元不算內容、強制換行之後的 embedding）、`text_rects::past_bidi_controls_around_a_newline`、`update::a_brs_style_in_place_past_bidi_controls`。
- WPT：`css/CSS2/bidi-text/`、`html/rendering/bidi-rendering/` 和 css-writing-modes 的 `bidi-*`、`unicode-bidi-*`（85 份）加進 `include.txt`；沒過的 21 份寫在 `.ini`：被 bidi 拆成不相連的幾段的盒子畫成一個（`bidi-001` 到 `bidi-011`）、`plaintext` 的 `<br>` 不分段、`ch` 單位、`innerText`、`bidi-glyph-mirroring-002` 的希伯來字之後差一個像素（還沒查）。變成通過的 `css-text/word-break/word-break-break-all-004`、`word-break-normal-ar-000`、`html/dom/elements/global-attributes/dir_auto-isolate`，拿掉 `.ini`；另外 6 份 `dir_auto-*` 的 `.ini` 原本寫 layout 沒有 bidi，改成實際的原因（`plaintext` 整段當成一段）。跑過的目錄（`css/CSS2/`、`css-text`、`css-inline`、`css-writing-modes`、`css-pseudo`、`html/rendering/`、`html/dom/elements/global-attributes/`，12547 項）沒有意外的失敗；意外通過的 6 份是 macOS 上已知的不一致（之前的 commit 就是）。
- mutation test（`mutants-2c.toml`）37 個：33 個抓到；沒抓到的 4 個：3 個是等價的（close 控制字元在它的 tag 之前或之後：邊界照方向重擺，和它的 level 無關；強制換行前關掉 embedding 的順序：UBA 的 X8 在段落分隔字元結束所有 embedding；只有控制字元的行：控制字元只插在有內容的地方，和那些內容在同一行，碰不到）、1 個補了測試（切開的 run 的字元的對應：`text_rects::past_bidi_controls_around_a_newline`）。第一輪的 8 個沒抓到的另外補了測試（override 和 isolate-override 要有左到右的文字才和 embed 不同、`<br>` 在 block 的 override 裡、右邊的 margin 和只有一邊的邊界要後面還有東西）。
- 跑過：`cargo test -p layout -p paint -p style -p style_resolver`；`test.py reftest text/ inline/ floats/ position/`（31 個）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs。
