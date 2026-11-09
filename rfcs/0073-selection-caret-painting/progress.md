# RFC 0073 的進度：畫出選取和游標

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-22，Fedora 44。在 `0073` 上做。

完成日期：2026-10-22，Fedora 44（AMD Ryzen AI 9 HX 370）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `::selection` 的 style（parse、比對、highlight inheritance、`:window-inactive`、`getComputedStyle`）和 `caret-color` 的 property（第 7 到 11 條） | 完成 | 2026-10-22 |
| 2 選取的繪製：每個 text fragment 的範圍、底色和字、預設和 inactive 的顏色、只讓改變的失效、文字欄位（第 12 到 20 條） | 完成 | 2026-10-22 |
| 3 游標：位置、繪製、`caret-color`、閃爍的計時器、只重畫游標（第 21 到 23 條） | 完成 | 2026-10-22 |

## 驗收

**`::selection` 和 `caret-color` 的 style**

| 項目 | 怎麼驗證 |
| --- | --- |
| 哪些有 `::selection`、`:window-inactive` 的選擇器有效，和 Chrome 152 一樣（樣式規則、`querySelector`、`@supports selector()` 一致；`::-moz-selection` 讓整條無效） | `cargo test -p style --test it -- stylesheet::selection_selectors_are_chromes`；WPT 的 `css/css-pseudo/parsing/highlight-pseudos.html`（讀 `<style>` 的 `sheet`，kun 沒有，失敗） |
| `::selection` 的 style：自己的規則，沒有的照 parent 的 highlight 繼承（背景也是）；只有那幾個 property 有作用，其他的是元素的、其中 `currentcolor` 的照 highlight 的 `color`；`currentcolor`、`inherit` 傳下去的是關鍵字；custom property 是元素和自己的；`:window-inactive` 照頁面的 focus；跟著 class 和 focus 的改變重算 | `cargo test -p dom_api --test it -- computed_style::selection_styles_are_chromes computed_style::selection_styles_follow_changes`（預期值都在 Chrome 152 上量過）；WPT 的 `css/css-pseudo/highlight-cascade/` 的 testharness 測試 |
| 頁面的 focus 改變時，有規則用到 `:window-inactive`（document 或 shadow tree 的）才重算整份 | `cargo test -p style --test it -- invalidation::the_pages_focus_styles_everything_again_if_a_rule_asks`；`cargo test -p dom_api --test it -- focus::the_pages_system_focus` |
| `caret-color`：`auto`、`currentcolor`、`transparent`、顏色；CSSOM 讀回寫的形式，`getComputedStyle` 讀回顏色（`auto` 和 `currentcolor` 是元素自己的 `color`）；插值時 `auto` 離散 | `cargo test -p style --test it -- cssom::caret_color interpolation::caret_color`；`cargo test -p dom_api --test it -- computed_style::caret_colors_are_chromes`；WPT 的 `css/css-ui/parsing/caret-color-*`、`css/css-ui/animation/caret-color-*` |
| `caret-color` 和 `::selection` 的改變不重新排版 | `cargo test -p layout --test it -- update::the_caret_and_selection_colors_lay_nothing_out` |

**選取的繪製**

| 項目 | 怎麼驗證 |
| --- | --- |
| 選取照 Chrome 152 畫：預設的 active 和 inactive 的顏色、`::selection` 的顏色（paired cascade：沒有規則宣告顏色時用預設的兩個）、底色的高度、行尾的空白框（強制換行、自動換行、內容的結尾，延續到有 layout 的內容才畫；atomic inline 之後不畫）、裝飾線和陰影、`user-select: none`、文字欄位只在有 focus 時、沒有 focus 的 frame 是灰色、shadow tree 裡的文字（flat tree） | `cargo test -p shell --test it -- selection_painting::`（預期值都是 Chrome 152 量的）；WPT 的 `css/css-pseudo/` 的選取的 reftest、`selection/shadow-dom/` |
| 行尾的種類和框的位置；哪些節點有 layout，跟著 DOM 的改變 | `cargo test -p layout --test it -- inline::how_lines_end update::what_a_selection_goes_over`（`update` 的每個測試都和從頭建的比對） |
| 選取改變只重畫離開和進入的元素，不重新 layout；改了選取的 DOM 版本號、輸入之後重畫 | `cargo test -p paint --test it -- paint_controller::a_selection_paints_again_what_it_leaves_and_takes`；`selection_painting::a_selection_changed_later`、`a_key_changes_the_selection_of_the_document`、`a_key_changes_the_selection_of_the_text_control` |
| 頁面沒有 focus 時畫灰色，回來時恢復 | 情境 `selection`（`python3 scripts/test.py scenarios selection`） |

**游標**

| 項目 | 怎麼驗證 |
| --- | --- |
| 游標照 Chrome 152 畫：文字欄位和 editing host 裡收合的選取上，1 px 寬、字型的高度、`caret-color`，downstream，空的元素照 `text-align`、`text-indent`；範圍、沒有 focus、唯讀、`transparent`、不能編輯的地方不畫 | `cargo test -p shell --test it -- selection_painting::carets`（33 個案例的預期值都是 Chrome 的）；WPT 的 `css/css-ui/caret-*` |
| 游標閃爍：自己消失再出現，打字、按方向鍵時跟著走，page 沒有 focus 時不畫 | 情境 `caret`（`window caret-blink=300`）、`caret-inactive`（`python3 scripts/test.py scenarios caret`） |
| 閃爍只重畫游標：block 的 item 照舊沿用，stacking context 只被走進去 | `cargo test -p paint --test it -- paint_controller::a_blinking_caret_paints_only_itself_again` |
| 閃爍的間隔：Windows 的 `CursorBlinkRate`（`-1` 不閃），自動化不閃 | `cargo test -p shell --lib -- system_colors::tests::a_blink_rate_is_one_phase_and_minus_one_none` |

**focus**

| 項目 | 怎麼驗證 |
| --- | --- |
| `select()` 讓控制項取得 focus（不捲動）；Selection API 把選取放進 editing host 時讓它取得 focus | `cargo test -p shell --test it -- selections::select_gives_the_focus selections::the_selection_api_focuses_an_editing_host selections::what_the_selection_doesnt_focus` |

## 做過的檢查

2026-10-22，Fedora 44（AMD Ryzen AI 9 HX 370），三段都在這台上做。

- **做了什麼**（設計和理由見 `README.md` 的第 7 到 23 條）：
  - style：`::selection` 的 parse 和比對（`MatchedAs::Selection`）、highlight inheritance（`HighlightStyle` 放在 `RareInherited`，只替比對到的元素算；`author_colors` 是 paired cascade）、`:window-inactive`（DOM 記頁面的 focus，有規則用到時整份重算）、`getComputedStyle(el, '::selection')`；`caret-color`。
  - 選取：DOM 的 `selection_version`；renderer 的 `selection` 模組照 flat tree 找出選到的 text node 的哪一段和 `<br>`，從終點往回找第一個有 layout 的節點；layout 的 `LineFragment::end` 和 `BoxTree::texts`（`is_rendered`、`block_of`）；paint 照 `HighlightPainter` 畫（字和原本的裝飾線裁掉選取的範圍，再畫底色、highlight 的陰影、裝飾線和字）、行尾的框；`SystemColors` 的 active 和 inactive 的顏色；只讓改變的元素失效。
  - 游標：renderer 找出位置（`selection::caret`），paint 畫在 block 的行之後、不在 box 的快取範圍裡，`PaintController::invalidate_caret` 只走進 stacking context；閃爍的計時器（`RendererFlags::caret_blink_interval`，Windows 讀 `CursorBlinkRate`）；情境的 `window caret-blink=`。
  - focus：`select()` 讓控制項取得 focus（不捲動）；Selection API 改變選取、放進 editing host 時讓它取得 focus（`SelectionOnFocus::None`，選取不動）。這 9 個方法成了 `async method`，`kun:dom` 的 WIT 升到 0.16.0，guest 和 Lua 的 runtime 重新編，Lua 的產生器學會 async 函式的可省略的物件參數。
- **實作時照 Chrome 152 實測改的**（`README.md` 的第 8 到 10、19、20 條和第 1 節）：property 照 Blink 的 `valid_for_highlight`；`:window-inactive` 是一般的 pseudo-class；行尾的框也畫在自動換行和內容的結尾、延續到有 layout 的內容才畫、atomic inline 之後不畫；paired cascade（沒有宣告顏色的規則照預設）；原本的裝飾線在選取裡不畫、原本的陰影照畫；選取照 flat tree 走；`select()` 不捲動，設成原樣的選取不給 focus，不能取得 focus 的 host 不動 focus；游標在 `visibility: hidden` 的文字裡也畫。Chrome 的測量都用同一份頁面在 Chrome for Testing 152 上跑（`details.md`，腳本在 scratchpad 的 `sel/`）。
- **找到的問題**：
  - 這份自己的：WPT 的分類（交給一個 subagent）找到 4 個（paired cascade、原本的裝飾線沒裁掉、行尾的框蓋在圖片上、圖片之後的 `<br>` 的框），都修好了；`selection/shadow-dom/` 的 reftest 找到選取要照 flat tree 走。
  - 0073 之前就有的：`@supports selector(::-webkit-scrollbar)` 是 `false`；`<br>` 的字型撐高行高（Chrome 不會）；kun 的波浪線的形狀和 Chrome 不同；空的 contenteditable 沒有一行的高度。除了波浪線都記進 `docs/limitations.md`。
  - Chrome 152 自己在 `css/css-pseudo/active-selection-063.html` 也失敗；`--disable-blink-features=CaretBlinking` 沒有讓 headless 的游標停止閃爍（量的時候截好幾張合起來）。WPT 的 `css/css-overflow/clip-003.html` 在這台 Linux 上通過、預期是失敗（平台的差別，沒改 `.ini`）。
- **測試**：style 和 dom_api 的 `::selection`、`:window-inactive`、`caret-color` 的測試（預期值是 Chrome 的）；`shell` 的 `selection_painting`（反白和游標的畫面，預期值是 Chrome 的）和 `selections`（focus）；`layout` 的 `inline::how_lines_end`、`update::what_a_selection_goes_over`（`Page::check` 每次比對索引）、`update::the_caret_and_selection_colors_lay_nothing_out`；`paint` 的 `paint_controller::a_selection_paints_again_what_it_leaves_and_takes`、`a_blinking_caret_paints_only_itself_again`；`system_colors` 和 `scenario` 的單元測試；情境 `selection`、`caret`、`caret-inactive`。
- **WPT**：加進清單的有 `css/css-pseudo/parsing/highlight-pseudos.html`、`highlight-cascade/` 的 testharness 12 份、選取的 reftest 84 份（43 份通過；41 份失敗的是 kun 沒有的功能，`.ini` 寫了原因；`highlight-cascade/` 裡載入不存在的 `support/selections.js` 的 7 份沒列）、`css/css-ui/` 的 `caret-color` 的 parse 3 份和游標 10 份。`selection/shadow-dom/` 裡要跨樹選取的 3 份標成失敗（`getComposedRanges()`）。`selection/`、`css/selectors/`、`css/css-scoping/`、`css/css-pseudo/`、`css/css-color/`、`css/css-ui/`、`css/css-text-decor/`、`html/semantics/forms/`、`html/interaction/focus/` 和 kun 自己的整套 reftest（189 份）沒有意外的結果。
- **Mutation test**（scratchpad 的 `mutants-0073-*.toml`）：
  - 第 1 段 21 條：漏的 2 條在 Chrome 上量過之後補了測試，重跑都抓到。
  - 第 2 段 40 條和 focus 的 16 條：漏的補了測試（`<br>` 的字型、匿名 block、按 Escape 改選取、半透明的底色、錨點在 host 裡的 `extend`），重跑都抓到；查索引時對一次 box 的內容改成 debug build 的檢查；`set_selection` 自己加的版本號是多餘的，拿掉了；「只有選取改變時要求 main frame」測不到（輸入的分派和跑 script 的 task 本來就要求 frame），記進 `docs/test-gaps.md`。
  - 第 3 段 23 條：漏的 3 條補了測試（行高較大的空行、`<br>` 字型較小、不閃爍時按 ←），重跑都抓到。
  - 沒跑的：`StyleElement::page_active` 沒有 scope 時的值（只有 `<use>` 的複本）。
- **跑過的檢查**：每段結束時 `cargo test --workspace`（最後一次 3553 個通過，跳過 3 個；1 分 46 秒）、`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy、`sanity.py` 的五項（`docs` 只有 `main` 上就有的、根目錄 README 引用的 V8 build 目錄在這台不存在）；`test.py automation` 53 份裡 49 份通過，失敗的 4 份是 `docs/limitations.md` 記過的、這台 Linux 的視窗大小帶小數的，沒有平台視窗的 `test.py scenarios` 跑它們都通過；新的情境在真視窗裡也通過。
