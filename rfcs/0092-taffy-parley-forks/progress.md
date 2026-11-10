# RFC 0092 的進度：taffy 和 parley 的 fork：跟著上游、修改留成 patch、減少 workaround 的成本

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-10，Windows 11。在 `0092` 上做。

完成日期：2026-11-10，Windows 11（AMD Radeon RX 7900 XTX）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 taffy fork 的基礎設施：fork、path 依賴、patch 的匯出和檢查、parley 當時的 patch、文件 | 完成 | 2026-11-10 |
| 2 計數和量測 | 完成 | 2026-11-10 |
| 3a 重寫 parley 的 fork：合成分組、rebase 到上游的 main、machopo 配合 | 完成 | 2026-11-10 |
| 3b parley 的 fork 自己的測試、上游的草稿、拿掉 `text-indent` 的 workaround、optical sizing 由 machopo 做 | 完成 | 2026-11-10 |
| 4a taffy 的 fork rebase 到上游的 main、machopo 配合 API、預期結果和限制 | 完成 | 2026-11-10 |
| 4b 用 slot 的高度改寫 `float_bands` | 取消：slot 的高度取代不了它 | 2026-11-10 |
| 5a parley 的 fork：`StyleProperty::FontFamilyIds`、`TreeBuilder::push_default` | 完成 | 2026-11-10 |
| 5b machopo 的 `FontFallbackMap`、頁面和 canvas 的文字改給 family 的 id、計數改義 | 完成 | 2026-11-10 |
| 6a float 旁邊的盒子進快取 | 不做：量過會命中的上限和時間 | 2026-11-10 |
| 6a column flexbox 的 intrinsic 寬度搬進 taffy 的 fork | 完成 | 2026-11-10 |
| 6b 做 fragment 時的斷行 | 完成 | 2026-11-10 |
| 7 上游的草稿 | 完成：不寫剩下的草稿，`upstream/parley/README.md` 列每個 commit 的狀態和送之前的下一步 | 2026-11-10 |

fork 最後的狀態（都 push 了，和 `third_party.toml` 一致）：

| fork | 上游的起點 | 頂端 | commit | 舊的頂端的 tag |
| --- | --- | --- | --- | --- |
| taffy（`wusyong/taffy` 的 `machopo`） | main 的 `4aca4f49`（2026-11-06） | `0de7a64f` | 1 個（column 的寬度）加 `README.machopo.md` | `machopo-2026-11-10`（v0.15.0 加 README）、`machopo-2026-11-10-4a`（rebase 之後、column 的寬度之前） |
| parley（`wusyong/parley` 的 `machopo`） | main 的 `1b67b753`（2026-11-08） | `227d7191` | 11 個加 `README.machopo.md` | `machopo-2026-11-10`（重寫前的 16 個 commit）、`machopo-2026-11-10-3a`、`machopo-2026-11-10-3b` |

## 驗收

**fork 和 patch**

| 項目 | 怎麼驗證 |
| --- | --- |
| taffy 的 fork 是上游的 main（`4aca4f49`）加上 column 寬度的 commit 和 `README.machopo.md` 的 commit | fork 的 `cargo test`；`python3 scripts/ci.py patches taffy` 匯出 2 個 patch，`ci.py third-party` 確認它們套在 `upstream_rev` 上和 `rev` 一樣 |
| parley 的 fork 是上游的 main（`1b67b753`）加上 11 個各做一件事的 commit 和 `README.machopo.md` 的 commit，每個 commit 帶 fork 自己的測試和完整的訊息 | fork 的 `cargo test -p parley -p parley_engine -p parley_tests -p fontique`、clippy、`cargo fmt --all -- --check`；`ci.py patches parley` 匯出 12 個 patch，`ci.py third-party` 確認 |
| `ci.py patches` 匯出：一個 commit 一個 patch、不編號、沒有簽名、完整的 blob id、內容每次一樣、換掉舊的、CRLF 的檔案和二進位檔照原樣、shallow 的 checkout 先加深、不是從 `upstream_rev` 長出來的不匯出、checkout 不在 pin 上時不匯出 | `python3 scripts/ci_patches_test.py`（`Patches`、`Commands`） |
| `ci.py third-party` 檢查 patch：沒匯出的 commit、被改過的、套不上的（指出是哪一個）、有 patch 卻沒有 `upstream_rev` 都報出來；checkout 的 index 和檔案不動；shallow 的 checkout 只拿 `upstream_rev` 這一個 commit | `python3 scripts/ci_patches_test.py`；在這台機器上 `python3 scripts/ci.py third-party` 對 taffy 和 parley 通過 |
| shallow 和完整的 checkout 匯出的 patch 一樣 | parley 的 checkout 原本是 shallow 的，第一次匯出時加深；之後在完整的 checkout 再匯出一次，每個檔案的 md5 不變 |
| machopo 用 path 依賴編兩個 fork | `cargo test --workspace` |

**跟上上游：parley**

| 項目 | 怎麼驗證 |
| --- | --- |
| machopo 配合上游的 API，除了 atomic inline 的 bidi 和斷行（文字裡的 U+FFFC），排版不變 | `cargo test -p layout`、`-p shell`；`test.py reftest` 的 `text/`、`inline/`、`floats/`、`generated/`、`canvas/`、`svg/`、`block/`、`position/`、`flex/`、`grid/`、`tables/`、`hidpi/`；`test.py wpt` 的 `css/css-text`、`css/css-text-decor`、`css/CSS2/linebox`、`css/CSS2/normal-flow`、`css/css-fonts`、`css/css-overflow/line-clamp`、`css/css-sizing`、`css/css-display`、`css/css-pseudo`；差異記在「做過的檢查」 |
| inline 元素的邊緣、最後一個強制換行之後的行、沒有 em 的字型、第一個字型、tab stop、軟連字號、共用 shaped text、shaping 的快取、斷行表 | fork 的 `parley_tests`（`test_edges`、`test_final_break`、`test_font_without_em`、`test_shape_cache`、`test_shape_words`、`test_shape_words_space` 等）；經過 machopo 的 layout 才看得到的、要用 fork 沒有的字型的留在 `layout` 的 `parley_line_breaks::`、`parley_words::` |
| `font-optical-sizing: auto` 的文字用字級的 `opsz` shape 和畫，`font-variation-settings` 自己設的贏，關掉時沒有（machopo 做，fork 沒有這個 commit） | `layout` 的 `font_features::a_variable_fonts_axes`：軸的值，和 advance（自動的和把 `opsz` 設成字級的一樣，和關掉的、設成 40 的都不一樣） |
| 只被問寬度的盒子有 `text-indent` 時不斷行；shrink-to-fit 的寬度含縮排 | `inline::a_width_alone_breaks_no_lines`：`max-content`、`fit-content` 加縮排的寬度是 110、斷行 1 次 |

**跟上上游：taffy**

| 項目 | 怎麼驗證 |
| --- | --- |
| machopo 配合 taffy 的 API，沒有退步；上游修好的 18 個 WPT 測試通過，`.ini` 刪掉 | `cargo test -p layout`、`-p shell`；`test.py reftest` 的 12 個目錄；`test.py wpt` 的 27 個目錄（「做過的檢查」的「reftest 和 WPT」） |

**計數**

| 項目 | 怎麼驗證 |
| --- | --- |
| `LayoutUpdate::laid_out_beside_floats`：因為旁邊有 float（或把自己的 float 放進共用的 block formatting context）沒用 taffy 的快取的盒子 | `layout` 的 `float::boxes_beside_floats_are_laid_out_without_the_cache`：沒有 float 時 0，兩個段落都在 float 旁邊時 2，只有一個時 1 |
| `LayoutUpdate::lines_broken_again_beside_floats`：float 旁邊的行從頭重斷的次數，四種原因 | `float::lines_beside_floats_are_broken_again`：沒有 float、float 旁邊放得下時 0；往下找更寬的地方、高了之後在更窄的地方、拿掉移到下一行的 float、放好自己的 float 之後，各自的次數 |
| `LayoutUpdate::lines_broken_for_fragments`：做 fragment 時在最後的寬度真的又斷了一次 | `inline::lines_are_broken_again_in_a_box_wider_than_they_were_broken_at`：`min-width` 撐寬的 float 是 1（行在盒子的寬度對齊）；`inline::lines_are_broken_once_for_their_fragments` 的情況是 0 |
| `BoxTreeUpdate::font_family_names`：照名稱在字型集合裡查的次數 | 「字型家族」的第一列 |
| 計數都在 release build 裡 | `RUST_LOG=layout=debug` 的 release build 印出 `layout`、`build box tree`、`update box tree` 三行的欄位（量測用的就是這些） |

**字型家族**

| 項目 | 怎麼驗證 |
| --- | --- |
| 一個 `font-family` 的值第一次用來 shape 時每個名稱查一次，之後不查 | `font_selection::a_font_family_value_is_looked_up_once`：block 的清單、inline 元素的、`display: contents` 的元素裡的 run 的，第一次各多幾個名稱就多幾次；看過的值（在別的 block、別種 span、別的字級）是 0；順序不同是另一個值；generic 不算名稱；沒有 shape 時 0。`update::` 的測試比較 `BoxTreeUpdate` 時不比這個欄位（數目跟平台的字型清單有關） |
| 註冊字型之後，`font-family` 的值重新查 | `font_selection::adding_fonts_looks_the_family_names_up_again`：註冊前名稱找不到、用 Ahem；註冊之後換字級重新 shape，查了名稱、用到新的字型 |
| 一個值查到的是集合裡有的名稱的 family 和 generic 代表的 family，照清單的順序 | `layout` 的 unit test `font_families::tests::a_value_is_the_families_the_collection_has`；`font_selection::a_generic_family_is_its_families` |
| 空的行的高度照 block 的字型（root style 用 `push_default` 給） | `font_selection::a_line_with_nothing_on_it_is_in_the_blocks_font`：`x\n\nx` 的中間那行和有字的行一樣高，Lato 的和 Ahem 的不一樣；`shell` 的 `focus::focusing_scrolls_into_view_if_needed`（編輯區結尾的空行、空的 input） |
| canvas 的文字照它的字型家族 | `font_selection::a_canvass_text_is_in_its_families`：Ahem 的寬度是 40、Lato 的不是，第二次（快取裡的）一樣；reftest 的 `canvas/c5-text.html` |
| fork 的 `StyleProperty::FontFamilyIds`、`TreeBuilder::push_default` | fork 的 `test_family_ids`、`test_tree_default` |

**column flexbox 的寬度**

| 項目 | 怎麼驗證 |
| --- | --- |
| 只被問寬度的、不換行、沒有子項帶比例的 column 不量子項的高度；換行的、有比例的照完整的演算法 | fork 的 `tests/hand_written/column_width.rs`（5 個）、`caching.rs` 的 `measure_count_flex_in_grid`（column 從 10 次 measure 變 6 次）；6689 個 xml fixture 的預期不變 |
| machopo 的 `column_flex_width` 拿掉之後 column 的寬度、shrink-to-fit、斷行次數不變 | `layout` 的 `flex::` 原有的 column 測試沒有改預期；把 fork 的捷徑關掉時 `flex::a_column_in_a_row_breaks_its_paragraphs_once` 失敗；`test.py wpt css/css-flexbox`、`test.py reftest flex/` |

**做 fragment 時的斷行**

| 項目 | 怎麼驗證 |
| --- | --- |
| fragment 用最後排版時斷的行；寬度由內容決定的盒子的行在 fit-content 的寬度斷，被給了寬度的在那個寬度斷 | `inline::lines_are_broken_once_for_their_fragments`（確定的寬度、shrink-to-fit 的 float（放得下的、沒有空間的、自己有寬度的）、從 border box 算回來有浮點誤差的寬度：都斷 1 次、做 fragment 時 0 次）、`inline::a_float_aligns_its_lines_in_its_own_width`、`inline::lines_with_floats_are_broken_once_in_the_width_their_box_is_given`（float 的 button、被給了寬度的盒子裡帶 float 的行） |
| 被給了寬度的盒子裡，行之間的 float 放在那個寬度裡 | `inline::a_float_among_lines_is_placed_in_the_width_its_box_is_given`（`display: flow-root` 的盒子裡的右 float 在盒子的右邊） |
| shrink-to-fit 的盒子的高度是它的行在它的寬度的高度 | `inline::a_shrunk_box_is_as_high_as_its_lines_at_its_width`（裡面有百分比寬度的 inline-block 的 float：高 44、2 行） |
| 量的頁面做 fragment 時不再多斷 | 「做過的檢查」的「量測」：`tests/sites/` 12 → 0，Speedometer 331 → 8 |

## 做過的檢查

都在 2026-11-10、Windows 11 上做。

### 最後的驗證

- rebase 到 `main`（`565d8e4`）之後：`cargo test --workspace`（55 組結果、3735 個測試通過、1 個 ignored）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`；`python3 scripts/sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`python3 scripts/ci.py third-party`；`python3 scripts/ci_patches_test.py`（15 個，在 CI 的 `lint` 裡）。
- 每個階段（1、2、3、4、5、6）結束時各跑過一次 `cargo test --workspace`、fmt、clippy；第 6 段的 fmt 第一次有 3 處（新的測試），`cargo fmt` 修掉。
- fork 的測試：parley 的 `cargo test -p parley -p parley_engine -p parley_tests -p fontique`（11、201、37、252 個通過、1 個 ignored）、clippy（只有上游 manifest 的警告）、`cargo fmt --all -- --check`；taffy 的 `cargo test`（6689 個 xml fixture 沒有改預期）、`cargo fmt --check`、clippy 沒有新的警告。
- **沒做的**：taffy 的 fork 沒有用它的 `rust-version`（1.71）編過（這台機器沒裝），送上游前要做。`test.py automation` 沒有跑（沒有動到視窗那一層）。Speedometer 的時間沒有量（只數了計數）。

### reftest 和 WPT

每段只跑和改動相關的目錄；除了下面列的，結果都符合預期、預期結果沒有改。

- **parley 跟上上游（3a、3b）**：reftest 12 個目錄；WPT 9 個目錄 14,151 個檢查。變化只有一個：上游不在零寬的格式字元之後加 `letter-spacing`，`css/css-text/letter-spacing/letter-spacing-control-chars-001.html` 從 FAIL 變 PASS，`.ini` 拿掉。
- **taffy 跟上上游（4a）**：reftest 12 個目錄 84 個；WPT 27 個目錄 27,129 個檢查，沒有退步，18 個原本預期失敗的通過、`.ini` 刪掉（括號裡的上游 PR 是照它的說明對上的，沒有逐一 bisect）：
  - `css/CSS2/floats/floats-wrap-top-below-bfc-00{1,2,3}{l,r}.xht`（6 個）：自己是 BFC 的盒子照整個高度避開 float（#1385、#1386）。
  - `css/CSS2/floats-clear/float-replaced-height-001.xht`、`floats-101.xht`：BFC 的根在 float 下面加上自己的 padding 和 border（#1384）。
  - `css/CSS2/floats/floats-wrap-bfc-with-margin-010.html`、`floats-zero-height-wrap-002.xht`、`zero-width-floats-positioning.tentative.html`：高度是 0 的 float 記成一段（#1387）、slot 的寬度分開扣兩邊的 float（#1392）。
  - `css/CSS2/floats-clear/adjoining-float-nested-forced-clearance-002.html`、`margin-collapse-157.xht`：摺穿的第一個子項的 margin 不再加到 block 的底（#1397）。
  - `css/css-flexbox/align-content-007.htm`：`wrap-reverse` 溢出的 line 靠 flex-start（#1363）。
  - `css/css-grid/alignment/grid-self-alignment-baseline-gfc-001.html`、`css/css-lists/list-and-grid-001.html`：grid container 的 baseline 取第一個 item 最後 layout 的（#1322、#1323）。
  - `css/CSS2/floats-clear/floats-031.xht`、`css/css-contain/contain-paint-clip-006.html`：哪一個 commit 修好的沒有查（後者猜是 BFC 的根的高度變了之後沒有東西可以捲）。這兩個和 `list-and-grid-001`、`margin-collapse-157` 另外各跑了 3 次，都通過。
  - 還在的三個（`zero-width-floats.html`、`zero-space-between-floats-003.html`、`floats-bfc-003.html`）的 `.ini` 註解改成現在的原因，限制在 `docs/limitations.md`。
- **字型家族（5b）**：reftest 整套 195 個檢查；WPT 的 `css/css-text`、`css/css-fonts`、`css/CSS2/linebox`、`css/css-text-decor`、`html/canvas/element/text` 共 9,689 個檢查。
- **column 的寬度（6a）**：`test.py wpt css/css-flexbox` 1646 個檢查、`test.py reftest flex/` 16 個。
- **做 fragment 時的斷行（6b）**：reftest 的 `inline/`、`text/`、`floats/`、`flex/`、`position/`、`tables/`、`forms/` 共 103 個；WPT 的 `css/css-sizing`、`css/CSS2/floats`、`css/CSS2/linebox`、`css/css-text`、`css/css-flexbox` 共 9,048 個檢查。`css/CSS2/floats` 的統計是「Ran 244、Expected 243、Unexpected 0」，差的 1 個沒有查是哪一個。
- WPT 一次給太多目錄時 Windows 的指令列太長（`WinError 206`），分成幾次跑。`cssom-view/scroll-behavior-smooth-navigation.html` 出現一次已經標過的 known intermittent。

### mutation test

machopo 的（`python3 scripts/test.py mutate rfcs/0092-taffy-parley-forks/<spec>`），最後全部抓到：

| spec | 範圍 | 條數 | 過程中沒抓到的、怎麼處理 |
| --- | --- | --- | --- |
| `mutants-1.toml` | `ci.py` 的 patch 匯出和檢查 | 16 | 拿掉 `--binary`：等價（`format-patch` 本來就給二進位的 diff），改成不傳這個選項、這條拿掉。檢查用 checkout 自己的 index：測試改成先 stage 一個改動，確認檢查之後還在。抓 `upstream_rev` 時連同歷史：補上 shallow 的 checkout 沒有它的 parent 的斷言 |
| `mutants-2.toml` | 四個計數 | 18 | 「匿名 block 不把它的名稱加進去」：測試裡後面的空 `<div>` 也拿走累計的名稱，總數不變；改成空的 flex container 之後抓到。沒有 mutation 的：`tracing::debug!` 的欄位（只有量測的 script 讀） |
| `mutants-3b.toml` | `opsz` | 5 | 「加的是另一個軸」：補一個關掉 optical sizing、把 `opsz` 設成字級的例子，advance 要和自動的一樣。`font-variation-settings` 設了 `opsz` 時照樣加：等價（後面的贏），記在 `docs/test-gaps.md` |
| `mutants-5.toml` | `FontFallbackMap`、計數 | 11 | 「generic 也算成查了名稱」：補帶 `serif, cursive` 的值。「generic 代表的 family 不在 id 裡」：測試的集合每個 generic 都是 Lato、parley 退回的也是 Lato，分不出來；補 `font_families` 的 unit test，用自己的集合（`serif` 對到 Ahem）直接比 id |
| `mutants-6a.toml` | `fit_content_width` 對 column | 2 | |
| `mutants-6b.toml` | 記下斷行的寬度、寬度由內容決定的條件 | 8 | 「永遠用記下的寬度」：補 `min-width` 的測試。「不看 style 的寬度」：補 float 自己有寬度的例子。「不看 `aspect-ratio`」抓不到、沒有放進清單：有比例的盒子的寬度都由父盒子傳進來，走不到；記在 `docs/test-gaps.md` |

拿掉 `InlineContent::indented`、taffy 的 API 配合（4a）沒有可以弄壞的新程式：前者是刪掉的（斷行次數的斷言分得出拿掉前後），後者照新的簽名傳一樣的東西。

fork 裡的（scratchpad 的一次性 script：改 fork 的檔案、跑 fork 自己的測試、還原）：

| commit | 條數 | 結果 |
| --- | --- | --- |
| 邊緣 | 24 | 全部抓到 |
| 最後的強制換行 | 6 | 4 條抓到、2 條等價 |
| 沒有 em 的字型 | 3 | 全部抓到 |
| 第一個字型 | 2 | 全部抓到 |
| tab stop | 16 | 15 條抓到、1 條走不到 |
| 軟連字號 | 30 | 27 條抓到、3 條等價 |
| 共用 shaped text | 11 | 10 條抓到、1 條等價 |
| shaping 的快取 | 16 | 12 條抓到、3 條等價、1 條是缺口（帶 safe-to-concat 旗標時照樣整段：parley 一律不要這些旗標，走不到） |
| default ignorable 開頭的字 | 5 | 4 條抓到（一條補了 Noto Sans CJK SC 的例子之後 fork 自己抓到）；「不看前一個字是不是空白」等價 |
| `space.rs` 的規則 | 22 | 19 條抓到；補了空白後面 glyph 太多的測試之後抓到 1 條；「`kern` 的 pair 把空白後面的 mark 也算進去」等價；「讀不懂的表當成不碰空白」是缺口（沒有壞掉的表的測試字型） |
| 斷行表 | 10 | 9 條抓到、1 條等價 |
| `FontFamilyIds` | 4 | 全部抓到 |
| `push_default` | 6 | 全部抓到（「不檢查等著合併的空白」第一次沒抓到：測試的 root 改成明確用 `Collapse`） |
| taffy 的 column 寬度 | 12 | 全部抓到；過程中補了換行、比例、容器高度、margin、可用高度（`column_width.rs`）和 padding 的下限的測試。第一版的 `item_with_aspect_ratio` 用 leaf，完整演算法也不經由比例換算，改成 block 的子項才分得出來 |

缺口和等價的都記在 `docs/test-gaps.md`（連同沒有測試的 24-bit GSUB、GPOS 格式）。optical sizing 的 commit 在 3a 跑過 7 條（6 條抓到、1 條等價），3b 拿掉了。

### 量測

**第 2 段：各個 workaround 實際的量**（release build，`RUST_LOG=layout=debug`，scratchpad 裡一次性的 script 加總每頁的 log）：`tests/sites/` 用 `--headless`（1280×800）載入一次；blink_perf 的 30 頁照 `bench.py blink-perf` 的方法（`--automation-pipe`，頁面跑完整個 runner）；Speedometer 3.1 的 20 個 suite 各一次 iteration（`iterationCount=1`，只數計數，沒量時間）。「字型名稱」是當時的意義（每個 span 給 parley 的名稱數），第 5 段之後這個計數數的是真的照名稱查的次數，沒有重量。

| 頁面 | 排的盒子 | 其中因為 float 沒用快取 | 斷行 | 其中做 fragment 時再斷 | float 旁邊重斷 | shape 的 IFC | 字型名稱 | 每個 IFC 的名稱 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `tests/sites/` 7 頁 | 4,467 | 2,899（65%） | 1,633 | 12（0.7%） | 0 | 1,423 | 2,806 | 2.0 |
| 　其中 gutenberg | 4,231 | 2,887（68%） | 1,448 | 12 | 0 | 1,238 | 2,282 | 1.8 |
| 　其中 sqlite | 76 | 12（16%） | 48 | 0 | 0 | 48 | 285 | 5.9 |
| blink_perf 30 頁 | 59,166,325 | 9,181,787（16%） | 20,692,932 | 0 | 0 | 7,450,128 | 19,939,869 | 2.7 |
| Speedometer 20 個 suite | 41,057 | 194（0.5%） | 18,611 | 331（1.8%） | 0 | 11,723 | 93,493 | 8.0 |

- **float 旁邊不用快取**：集中在有 float 的頁。blink_perf 的 `floats_2_100_nested` 96%、`floats_show_hide` 92%（909 萬個裡的 834 萬）、`floats_20_100_nested` 82%；沒有巢狀的 `floats_*` 0–36%。gutenberg（圖片是 float）68%。Speedometer 只有 TodoMVC 的幾個 suite 有，每個 13–40 個。
- **做 fragment 時再斷行**：Perf-Dashboard 986 次斷行裡 222 次（23%）；其他 suite 每個 0–15 次；gutenberg 12 次；blink_perf 0。
- **float 旁邊的行重斷**：量的頁面全部是 0。blink_perf 的 float 頁是 block 的 float，旁邊沒有文字。
- **字型名稱**：每頁都有。Speedometer 每個 IFC 平均 8 個名稱，Complex-DOM 的 TodoMVC 除了 Lit 都是 11.2 個（每個 suite 1.2 萬個名稱，Lit 的 5.2 個）、Perf-Dashboard 9.4、TipTap 9.1。blink_perf 的 `flexbox-row-nowrap`、`flexbox-row-wrap` 各 650 萬個（每個 IFC 5.4 個），`chapter-reflow*` 每個 IFC 4.8 個。
- parley 重寫之後（3a）`tests/sites/` 的 7 頁再數一次，每頁每個計數和上面一樣。

**float 的快取會命中的上限**（6a，量了之後不做；release build，暫時的計數，量完還原）：`compute_box` 裡每個盒子多留一份 taffy 的 `Cache`，只給 float 旁邊的計算用，數同一個盒子、同樣的 `LayoutInput` 又算一次的次數，和結果（大小、上下的 margin）跟上次一樣的次數；另外數這些計算底下一共做了幾次盒子的計算、斷了幾次行（巢狀的只算一次）。

| 頁面 | 全部的計算 | float 旁邊沒用快取 | 其中同樣的輸入又算 | 省掉的斷行 |
| --- | --- | --- | --- | --- |
| gutenberg（38 次 layout） | 4,848 | 3,077 | 2,960（96%） | 0 |
| sqlite 的 `about.html` | 130 | 12 | 5 | 0 |
| `floats_show_hide` | 9,094,695 | 8,335,800 | 8,293,602（99.5%） | 0 |
| `floats_2_100_nested` | 556,032 | 535,487 | 523,700（98%） | 0 |
| `floats_20_100_nested` | 258,614 | 214,182 | 208,200（97%） | 0 |
| `floats_50_100_nested` | 125,830 | 42,824 | 40,400（94%） | 0 |
| `floats_100_100_nested` | 137,957 | 6,413 | 5,000（78%） | 0 |
| 沒有巢狀的 `floats_*` 5 頁 | 48,614 到 315,071 | 111 到 21,000 | 0 | 0 |

- 又算的那些結果全部和上次一樣；省掉的盒子的計算數等於命中數（每次命中只省那個盒子自己的）。這是上限：真的 key 還要 float 的狀態一樣。
- 時間（`bench.py blink-perf --pages`，machopo `60bf10a` 的 `perf` profile 對 Chrome for Testing 152，各一次）：`floats_show_hide` 21.1 對 19.3 runs/s（machopo 的時間是 Chrome 的 0.91 倍）、`floats_2_100_nested` 23.2 對 22.2 ms（1.05 倍）、`floats_20_100_nested` 43.5 對 53 ms（0.82 倍）。單次的量測，只能讀成大致打平。
- 沒有取樣 `compute_box` 佔這幾頁多少時間（`blink-perf` 沒有取樣的選項，上面的結果已經夠決定）。

**做 fragment 時的斷行**（6b；release build，暫時的輸出，量完還原；`tests/sites/` 7 頁和 Speedometer 20 個 suite 各一次 iteration）。做之前再斷的 343 次（`tests/sites/` 12、Speedometer 331）照原因分：

| 原因 | 次數 |
| --- | --- |
| content box 的寬度從 border box 算回來的浮點誤差（行已經在同一個寬度斷過） | 234 |
| shrink-to-fit：行在給它的空間斷，盒子最後是 fit-content 的寬度 | 101 |
| 斷行的結果被清掉（atomic inline 的大小變了、量過 min 和 max content） | 8 |
| taffy 在最後的 layout 之後又量了別的寬度 | 0 |

做之後（同樣的頁面，各一次）：

| 頁面 | 斷行（之前 → 之後） | 其中做 fragment 時再斷（之前 → 之後） |
| --- | --- | --- |
| `tests/sites/` 7 頁 | 1,633 → 1,591 | 12 → 0 |
| Speedometer 20 個 suite | 18,611 → 17,738 | 331 → 8 |
| 　其中 Perf-Dashboard | 986 → 764 | 222 → 0 |

- 剩下的 8 次是 Editor-CodeMirror 和 Editor-TipTap 各 4 次，都是斷行的結果被清掉的那一種（不處理）。blink_perf 做之前是 0，沒有重量。
- 「之前」是第 2 段量的；斷行的總數少掉的比再斷的多：中間的階段的改動也算在裡面，沒有分開量。
- 第一次做完 Speedometer 還剩 21 次：13 次是 float 的 button（TodoMVC 的「Clear completed」），button 的 `auto` 寬度在 taffy 的 style 裡是 `fit-content`，條件漏了，補上。

**字型家族**：改完沒有重新量各頁的數目。看過的值是 0，一頁的次數是它用到的不同的值的名稱數的總和（測試守著）。

### 查到的事和踩過的坑

- **`core.autocrlf`**（第 1 段）：比 fork 的 `src/` 和 crates.io 的 0.15.0 時每個檔案都不同：這台機器的 `core.autocrlf=true` 讓 clone 的檔案變成 CRLF；改成 `false` 重新 checkout 之後逐檔一樣（`Cargo.toml` 和 crates.io 的 `Cargo.toml.orig` 一樣），所以第 1 段沒有跑 reftest 和 WPT。`patches/` 標成 `-text`：上游有 CRLF 的檔案（taffy 的 `CONTRIBUTING.md`、parley 的幾個字型的 LICENSE），改到時 patch 帶著 CR，換成 LF 就套不上。
- **parley 的改寫**（3a）：舊的 16 個 commit 先合成分組（tree 和 `cefcbc5` 一樣），再一組一組搬到上游的 `1b67b753`（66 個上游的 commit，parley 0.12.0、harfrust 0.15.0）。舊的「atomic inline 照它所在的 span 斷行」和 `InlineBoxLineBreak` 沒有搬：照上游，atomic inline 是文字裡的 U+FFFC。和舊的 fork 不同的行為：atomic inline 照 U+FFFC 參與 bidi 和斷行；邊緣放不下時和文字一樣處理（舊的把它留在行上）；上游的 `calculate_content_widths` 算進 `text-indent`；上游不在零寬的格式字元之後加 `letter-spacing`；`text.rs` 統計的 cluster 數每個 atomic inline 多 1。成本：有非零寬度 atomic inline 的 layout 每次 build 複製 shaped text（改寬度時不能和快取共用）；段落快取的 key 多了 scale，字的快取的 key 多了 `point_size`。
- **default ignorable 開頭的字**（3a）：`parley_shaped_text::` 的兩個測試在 debug build 的比對（以字為單位的 shaping 和整段的）失敗：glyph 相同，只有 U+202E 的 cluster 的 `SAFE_TO_BREAK_BEFORE` 不同。字型自己不放 mark 時，新的 harfrust 的 fallback mark positioning 把 base 之後的 default ignorable 和 base 當成一組，整段 shape 時空白和它後面的 default ignorable 之間不能切；以字為單位時它們是兩個字。修法：default ignorable 開頭的字和前面的空白一起 shape（`WordRuns::joins`），fork 的 `a_default_ignorable_after_a_space_is_shaped_with_it` 守著。parley 和 harfrust 的 default ignorable 的集合可能有小的差異；debug build 的比對會抓到。
- **計數看的是這一次 shape 的字**（3b）：`text_in_lato_is_shaped_whole` 加 Ahem 的對照時，同一個 context 先排過別的文字，字的快取裡已經有那些字、命中的不算 shape；對照改用新的 context。
- **optical sizing 的成本**（3b）：有 optical sizing 的 style（預設就是）每次 `text_style` 多一個 variation，清單不再是空的，多一次小的配置；沒有另外量。
- **`ContentSlot::height`**（4b，取消）：它是這個寬度往下到被更窄的 segment 縮小為止的距離，不是 segment 的底（變寬的地方不算），後面沒有更窄的 segment 時是無限大。`float_bands` 要每一段的頂端和寬度，包括變寬的地方，最後一段還要最低的 float 的底，所以一段一段走、最後問 `cleared_threshold(Clear::Both)` 都拿不掉。
- **taffy 的 API 配合**（4a，`crates/layout/src/lib.rs` 4 處）：`find_content_slot` 多的 `height` 傳 0；`apply_content_box_inset` 的 `[f32; 2]` 換成 `FloatInsets`；`place_floated_box` 多的 `id` 傳盒子的 node id（現在沒有人讀）。`ScrollableOverflow`（#1372）不用改：machopo 只把 taffy 給的原樣傳回去。`calc()` 的 handle（#1400）還沒合併。
- **root style 也是空的行的 style**（5a）：5b 第一次的做法是 root style 不帶家族、所有文字外面包一個帶 id 的 span，`shell` 的 `focus::focusing_scrolls_into_view_if_needed` 穩定失敗（差 1px 到 10px）：沒有文字的 layout 和空的行的行高來自 root style 的字型，span 管不到。所以 fork 多一個 `TreeBuilder::push_default`；當時 `cargo test -p layout` 全過，補了 `a_line_with_nothing_on_it_is_in_the_blocks_font`。
- **fork 的 fmt 被換行警告蓋過**（5a）：`test_family_ids.rs` 原本沒有照 rustfmt 排，當時的 `--check` 被 Windows checkout 的換行警告蓋過、沒看到 diff；改進那個 commit 裡。
- **column 的捷徑不是嚴格等價**（6a）：完整的演算法量子項的寬度時給它 flex 之後的高度，捷徑給它自己的 style 的高度（夾在 min、max 裡、不小於 padding 加 border），沒有的話不給。寬度跟著給的高度變、又不經由 `aspect-ratio` 的子項結果會不同（`column_width.rs` 的 `item_available_height` 的第一個斷言在完整演算法下是 10、捷徑是 70）。machopo 原本的捷徑也是這樣，Blink 的 `ComputeMinMaxSizes` 也只用子項自己的貢獻。記在 `docs/test-gaps.md`。machopo 的 `column_flex_width`、`widest_column_item` 和 debug 的對照拿掉（`lib.rs` 少 198 行）。
- **「一律在 fit-content 的寬度斷」行不通**（6b）：照做時 `cargo test -p layout` 有 8 個測試失敗（行數、list marker 的位置、斷行的次數）。taffy 的 `compute_leaf_layout` 在 `PerformLayout` 時不把盒子被給的寬度當已知的大小傳給 measure function，而是放進可用的空間；這時空間就是盒子最後的寬度，fit-content 的寬度可能不同。改成只在寬度由內容決定時用 fit-content 的寬度之後，只剩 2 個測試的斷行次數變少（排版沒變），改了預期的次數。
- **順便修到的**（6b）：shrink-to-fit 的盒子裡有百分比寬度的 inline-block 時，原本高度是在空間的寬度量的、行是在盒子的寬度斷的（測試的 float 原本高 24、行有 2 行，現在高 44）。被給了寬度的盒子裡帶 float 的行原本先在 fit-content 的寬度排、做 fragment 時再在盒子的寬度斷，現在直接在盒子的寬度排。
- **找到又修好的 bug**（第 2 段找到，6b 修好）：寫計數的測試時發現 `display: flow-root` 的盒子裡有 float 和比盒子窄的行內內容時，行和 float 只在 max-content 的寬度排過：`<div style="display: flow-root"><div style="float: right; width: 10px; height: 5px"></div>X</div>` 在 800px 寬的 body 裡，float 在 x=20（Chrome 在 790）。當時記成 `docs/limitations.md` 的限制、原因沒查。原因是帶 float 的行內內容不管盒子有沒有被給寬度都在 fit-content 的寬度排；6b 之後被給了寬度的盒子直接在那個寬度排，float 在 790。收尾時確認、補了 `inline::a_float_among_lines_is_placed_in_the_width_its_box_is_given`（把條件改回 `floats || sized_by_content` 時它失敗：float 在 x=20），限制刪掉。
- **不穩定的測試**（3a 之後 rebase 到當時的 `main`）：`shell` 的 `devtools_initiators::script_asks_with_its_stack` 單獨跑 6 次失敗 2 次（沒有 `href` 的 `<link>` 之後拿到的是前一段 script 的 stack）。這個測試和它測的程式這個 branch 都沒有動過。看程式推測的原因（沒有驗證）：沒有 load 拿走的 stack 在 render thread 一輪的最後才清掉（`clear_request_stacks`），兩個 `Runtime.evaluate` 落在同一輪時第二個拿到第一個留下的。之後各段的 `cargo test -p shell` 和 `--workspace` 沒有再遇到。
- **上游的草稿**（只在本地，沒有送出任何東西）：`upstream/parley/` 的最後的強制換行、沒有 em 的字型、斷行表，`upstream/taffy/` 的 column 寬度。上游的 PR 範本規定說明不能是 LLM 寫的、第一行要揭露 LLM 的參與，所以草稿寫成材料（改了什麼、為什麼、怎麼測、reviewer 可能問什麼），送出的人照它自己寫；斷行表建議先開 issue，送出前要用上游的 `parley_bench` 量測，`skip_word_boundaries` 拆成另一個 PR。
