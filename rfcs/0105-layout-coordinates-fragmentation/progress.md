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
| 2a style：`direction`、`unicode-bidi`、`dir`、`:dir()` | 進行中 | |
| 2b layout 用 `direction` | | |
| 2c `unicode-bidi` 在 inline 內容裡 | | |
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
