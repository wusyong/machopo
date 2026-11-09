# RFC 0068 的進度：Speedometer 用到、kun 丟掉的其他 CSS

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-21。完成：2026-10-21，macOS（Apple M4；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0068` 上做，三段和收尾時的 cascade 修正各一個 commit，最後一起 merge；parley fork 多了兩個 commit（optical sizing、軟連字號）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 只解析的 property 和 shorthand（第 7 到 12 條）：1a 少見的繼承 property 放進共用的組；1b 介面的 property；1c `border-image`；1d 邏輯圓角、`border-spacing` 的 longhand；1e `animation-timeline` 和 `animation-range-*` | 完成 | 2026-10-21 |
| 2 字型（第 13 到 18 條）：2a 字型組和全部的字型 longhand、`font`、`font-variant`、`font-synthesis` shorthand；2b feature、variation、字寬、`opsz`、合成，可變字型照軸的值畫；2c 軟連字號（parley fork） | 完成 | 2026-10-21 |
| 3 `cursor`（第 19 到 24 條）：解析，renderer 照 Blink 選游標，視窗換游標，情境檢查游標 | 完成 | 2026-10-21 |
| 收尾：cascade 只走有宣告的 longhand（第 25 條） | 完成 | 2026-10-21 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 共用的組 | 少見的繼承 property（`RareInherited`）和字型（`FontDescription`）共用一份：子孫沿用 parent 的組，設了一個（字型是值不同）才有自己的；設了 `border` 的元素和別人共用 `rare` 組；改了照樣讓子元素重算、該重新排版的照樣排 | `cargo test -p style --test it -- style_groups:: invalidation::a_changed_rare_inherited_property`；`cargo test -p layout --test it -- update::a_text_indent_lays_out_again lists::an_outside_marker_isnt_indented` |
| 只解析的 property | `user-select` 和 `-webkit-` 的別名、`-webkit-user-modify`、`forced-color-adjust`、`-webkit-tap-highlight-color`、`text-size-adjust`、`text-rendering`、`-webkit-font-smoothing`、`resize`、`scroll-behavior`：Chrome 的值、初始值、繼承、computed value、插值；改了不重新排版 | `cargo test -p style --test it -- the_properties_parsed_only`；`cargo test -p layout --test it -- update::a_property_parsed_only_lays_nothing_out`；WPT 的 `css/css-ui/parsing/{resize,user-select}-*`、`css/css-size-adjust/`、`css/css-forced-color-adjust/parsing/` |
| | `animation-timeline` 只收 `auto`、`animation-range-*` 只收 `normal`；`animation` 重設它們，照 Chrome 寫回；keyframe 丟掉它們 | `cargo test -p style --test it -- animation_properties:: keyframes::a_keyframe_drops`；WPT 的 `css/css-animations/parsing/` |
| `border-image` | 它和它的 longhand：解析、計算（`url()` 照 base URL）、寫回和 `getComputedStyle` 照 Chrome，逐邊插值；`border` 重設它，寫回時照 Chrome | `cargo test -p style --test it -- border_image css_text_groups shorthands_come_from`；WPT 的 `css/css-backgrounds/parsing/border-image-*`、`css/css-backgrounds/animations/border-image-*` |
| | Web Animations 的 keyframe 裡的 shorthand 照宣告的寫法讀回（不寫它省略的 longhand） | `cargo test -p dom_js --test it -- a_shorthands_keyframe_reads_back_as_written` |
| 邏輯圓角、`border-spacing` | 邏輯圓角對到物理的角，和它們同一個邏輯群組；`border-spacing` 是 Chrome 的兩個 longhand 的 shorthand | `cargo test -p style --test it -- logical:: tables::`；`test.py scenarios rounded`（點邏輯圓角的弧外打到頁面）；WPT 的 `css/css-tables/parsing/border-spacing-*` |
| 字型 | 字型的 longhand 和 `font`、`font-variant`、`font-synthesis`：解析、computed value、寫回、`getComputedStyle` 和插值照 Chrome；`font` 重設其他 longhand | `cargo test -p style --test it -- fonts::`；WPT 的 `css/css-fonts/parsing/` |
| | 字型的 property 生效：feature 照 Blink 的順序合併、variation、字寬、`opsz`、`font-synthesis`、`text-rendering: optimizeSpeed` | `cargo test -p layout --test it -- font_features::`；`cargo test -p layout --lib -- font_features` |
| | 可變字型照 shaping 用的軸的值畫，instance 照值快取 | `cargo test -p raster -- a_variable_font_is_drawn_at_its_axes` |
| 軟連字號 | 斷在 U+00AD 的行尾有連字號（軟連字號的 style，U+2010 或 `-`，照段落的方向放），連字號放不下時用這一行前面的斷點；`hyphens: none` 不在它斷；min-content 寬度算進連字號；`hyphens` 改了重新排版 | `cargo test -p layout --test it -- inline::a_line_breaks_after_a_soft_hyphen_with_a_hyphen inline::a_soft_hyphen_before_an_inline_element inline::the_measured_min_content_width_counts_the_hyphen update::a_new_color_reaches_the_hyphen update::a_text_indent_lays_out_again`；WPT 的 `css/css-text/parsing/hyphens-*`、`css/css-text/hyphens/`（清單裡的） |
| `cursor` | 解析、computed value、寫回照 Chrome（`url()` 的清單和座標、`-webkit-` 別名），繼承；改了不重新排版 | `cargo test -p style --test it -- cursors`；`cargo test -p layout --test it -- update::a_property_parsed_only_lays_nothing_out`；WPT 的 `css/css-ui/parsing/cursor-*` |
| | 視窗的游標照它的關鍵字，`auto` 在連結上是手指、選得到的文字上是 I 形；UA stylesheet 照 Chrome 給連結、文字欄位、按鈕這些游標；滑鼠不動、底下變了也更新 | `cargo test -p browser --test it -- the_cursor`；`cargo test -p shell --test it -- browser_view::the_cursor`；`test.py scenarios cursor`；游標的樣子請人看 |
| 盤點和效能 | 盤點裡這類的 property 都讀得回來 | `bench.py speedometer --inventory` 的「CSS declarations kun drops」只剩 `caret-color` |
| | 沒有明顯變慢：cascade 的成本不跟著 longhand 的總數長 | `bench.py speedometer --against` 和 `bench.py blink-perf --dirs css --against` 開始前的程式碼（下面「做過的檢查」） |

## 做過的檢查

**完成時（2026-10-21，macOS，Apple M4）**：每一段做完時和收尾的修正之後各跑過 `cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（第 1 段 3443 個，第 2 段 3465，第 3 段和收尾 3471，都通過）和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`；parley fork 在它的目錄裡用它自己的 toolchain 跑 parley 和 parley_engine 的測試、`cargo fmt --check` 通過。改到的範圍的 `test.py reftest`（185 個）、`scenarios`（`rounded`、`cursor`）和 WPT 的目錄沒有非預期的結果。第 3 段動了視窗那一層，跑了 `test.py automation`：第一次螢幕是鎖定的（`CGSSessionScreenIsLocked`），macOS 不讓視窗畫東西，要畫面的情境都逾時；解鎖之後重跑，50 個情境和 `pipe --window` 全部通過。游標的樣子由使用者在 `kun tests/scenarios/pages/cursor.html` 上看過：每個區塊的游標、`none` 藏起再移開時出現、點了之後滑鼠不動也換成 `grabbing`、網址列的 I 形，都正確。

**每一段做的時候對照 Chrome 152、Blink 和 WPT 定下或改掉的**（細節在「已決定的事項」）：
- **共用的組**：14 個少見的繼承欄位搬進 `RareInherited`（主體從 88 個欄位到 75 個），字型的 4 個欄位和 `text-rendering`、`-webkit-font-smoothing` 搬進 `FontDescription`（到 72 個）。每個元素都經過的地方（`finish` 的套用的裝飾線、`StyleAdjuster` 的 float、marker 的 `text-indent`）和 `font` 重設的 longhand 只在值不同時才寫，否則每個元素都複製一份。
- **只解析的 property 和 `border-image`**：值、初始值和讀回的值照 Chrome 152 查過（`details.md` 的「Chrome 上確認的事」）。`border` 照 Chrome 重設 `border-image` 之後，它不再是「Chrome 的 shorthand 還有 kun 沒有的 longhand」：`cssText` 和 Chrome 一樣多寫一個 `border-image: none`，`getComputedStyle` 的 `border` 只看四邊；Web Animations 的 `getKeyframes()` 因此把 `border: pink` 寫成 `medium none pink`（第 3 段的 WPT 才看到），`CssPropertyValueSet::parse_value` 改成和宣告區塊一樣留著「shorthand 省略的 longhand」的標記。
- **字型**：`font` 不重設 `font-synthesis-*`（Chrome 152），`getComputedStyle` 的 `font` 省略 400 的粗細，`font-variant-alternates` 照 css-fonts 4 的順序（WPT 照它）。2b 開始時發現 kun 畫字一律用可變字型的預設 instance（macOS 的 `system-ui` 的 `bold` 畫成一般的粗細、字距卻是粗體的），照第 18 條修好：`system-ui` 的 `bold`、`900` 畫成粗體，`font-variation-settings: "wght" 300` 畫成細體（`kun --headless`，系統字型）。
- **軟連字號**：WPT 的 `css/css-text/hyphens/` 多半用 `ch` 定寬度（kun 沒有 `ch`）、靠字典的 `auto` 斷字或 `hyphenate-*`，通過的 13 個加進清單（`hyphens-shaping-002` 檢查有畫連字號、`hyphens-vertical-001` 比對 U+2010）。ICU4X 的 UAX #14 在希伯來字母之間的軟連字號不斷，記進限制。
- **`cursor`**：Chrome 的 `html.css` 給連結 `pointer`、文字欄位 `text`、按鈕和其他控制項 `default`，照搬進 UA stylesheet（hand 原本是 renderer 對連結硬給的）。情境的 `wait-cursor` 第一次在 mutation test 裡沒跑到（`cargo test` 預設跳過情境），改用 `test.py scenarios`。

**WPT**：每一段用 `--all` 試過對應的目錄（`css/css-ui/parsing/`、`css/css-text/parsing/`、`css/css-size-adjust/`、`css/css-forced-color-adjust/parsing/`、`css/css-backgrounds/parsing/border-image-*`、`css/css-tables/`、`css/css-fonts/parsing/`、`css/css-text/hyphens/`），通過的和失敗原因是 kun 沒有的功能（`calc()` 的序列化、`sign()`、`cqw`、`image-set()`、`light-dark()`、`ch`）的加進清單，`.ini` 寫原因。原本在清單裡、因為這份從失敗變成通過的（`*-interpolation`、`border-shorthand`、`cssom/shorthand-values`、`serialize-values` 的 `cursor`、`kerning` 關掉比對的 reftest……）改了預期結果；變成失敗的只有因為 property 生效之後照實失敗的：`letter-spacing-ligatures-001`（參考頁的 `font-variant-ligatures: none` 生效了，kun 沒有 `text-justify: inter-character`）、web-animations 的 `font-stretch`、`font-variation-settings` 的 `composite: add` / `accumulate`（kun 沒有 composite），寫明原因。改了自動化協定，`test.py wpt css/css-ui/` 經過 machopodriver 通過。

**mutation test**（`mutants-1a.toml` 到 `mutants-1e.toml`、`mutants-2a.toml` 到 `mutants-2c.toml`、`mutants-3.toml`、`mutants-perf.toml`，連同 parley fork 的）：每一份最後都全部抓到（8、14、18、8、5、22、23、32、23、6 個）。第一輪沒抓到的多半是測試走的路和實作不同：繼承靠共用的組、繼承清單只管 `unset`（`user-select`、`cursor`）；`width: min-content` 走換行、不是 parley 的量測；沒有 viewport 的分頁裡 `div` 寬 0；情境沒有跑到；shaping 只檢查了畫的時候的值。都補了測試。等價的記進 `docs/test-gaps.md`、從清單拿掉（fork 在 optical sizing 改變處切開 item、軟連字號的五個、游標座標的第二個數字可省略）。`cargo fmt` 和後面的段落改到的行跟著改寫 `from`、重跑都抓到；最後所有的 `from` 都在它的檔案裡剛好出現一次。

**效能**：開始前（`b63cd58f`）kun / Chrome 比值的幾何平均 2.121，第 1 段之後 2.141，第 2 段之後 2.179，做完 2.136；這些是不同時段的單獨量測（同一份程式在早上和下午就差 5% 左右），判斷用兩個版本在同一時段輪流跑的 `--against`。`--against` 要兩個版本釘同一個 `../third_party`，所以開始前的版本用一個不在任何分支上的 commit 物件 `13a33095`：`b63cd58f` 的內容，只把 `third_party.toml` 換成 fork 的 `7926e43`（fork 新加的都預設關掉，行為不變）。
- 三段做完時（`4491fe35`，3 次）：Speedometer 20 個 suite 的幾何平均 +1.0%，一致變慢的只有 TodoMVC-Lit-Complex-DOM +3.6%；取樣（macOS 每秒 1000 個樣本）指向 style 重算。blink-perf 的 `css/` 有效的 22 頁 +3.6%：`CustomProperties*Inheritance` +7% 到 +8%（每個元素的 cascade 跑三趟長度是全部 longhand 數的迴圈，longhand 從 206 個到 247 個）、CSSOM 的 `CSSPropertySetterGetter*`、`CSSPropertyUpdateValue` −18% 到 −19%（`border`、`font`、`font-variant` 展開成更多 longhand）。
- 照第 25 條改了之後（`69d37c4b`）：`CustomProperties*Inheritance` 比開始前快 4% 到 7%，22 頁 +2.8% 全來自 CSSOM 那三頁（沒改，記在 `docs/performance-gaps.md`）；Speedometer 的 Lit +2.2%，WebComponents、Vue、Stockcharts ±1% 以內。
