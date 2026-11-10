# RFC 0092 的進度：taffy 和 parley 的 fork：跟著上游、修改留成 patch、減少 workaround 的成本

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-10，Windows 11。在 `0092` 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 taffy fork 的基礎設施：fork、path 依賴、patch 的匯出和檢查、parley 現在的 patch、文件 | 完成 | 2026-11-10 |
| 2 計數 | 完成 | 2026-11-10 |
| 3a 重寫 parley 的 fork：合成分組、rebase 到上游的 main、machopo 配合 | 完成（fork 的頂端 `11771f1c`，tag `machopo-2026-11-10-3a`；舊的頂端的 tag `machopo-2026-11-10`） | 2026-11-10 |
| 3b parley 的 fork 自己的測試、上游的草稿、拿掉 `text-indent` 的 workaround、optical sizing 由 machopo 做 | 做完，等審核：fork 的頂端 `b4941e39`（`machopo` 分支）和兩個 tag 已經 push | |
| 4a taffy 的 fork rebase 到上游的 main、machopo 配合 API、預期結果和限制 | 做完：fork 的頂端 `05e64bd4` 和舊的頂端的 tag `machopo-2026-11-10` 都還只在本地，`third_party.toml` 釘本地的 commit；push fork 之後才能 push machopo | 2026-11-10 |
| 4b 用 slot 的高度改寫 `float_bands` | 取消：slot 的高度取代不了它（下面的「第 4b 段」） | 2026-11-10 |
| 5a parley 的 fork：`StyleProperty::FontFamilyIds`、`TreeBuilder::push_default` | 做完，等審核：fork 的頂端 `227d7191`（11 個 commit 加 README）只在本地，`third_party.toml` 釘本地的 commit；push fork 之後才能 push machopo。舊的頂端 `b4941e39` 的 tag 是 `machopo-2026-11-10-3b`（本地） | |
| 5b machopo 的 `FontFallbackMap`、頁面和 canvas 的文字改給 family 的 id、計數改義 | 做完，等審核 | |
| 6a float 旁邊的盒子進快取 | 不做：量過會命中的上限和時間（下面的「第 6a 段：float 的快取」） | 2026-11-10 |
| 6a column flexbox 的 intrinsic 寬度搬進 taffy 的 fork | 做完（fork 的 commit 在本地，還沒 push） | 2026-11-10 |
| 6b 做 fragment 時的斷行 | 做完，等審核 | |
| 7 上游的草稿 | 不寫剩下的草稿：`upstream/parley/README.md` 列了每個 commit 的狀態和送之前的下一步，等審核 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| taffy 的 fork 的 `machopo` 分支是上游的 v0.15.0 加上 `README.machopo.md` 的 commit，source 和 crates.io 的 0.15.0 一樣 | `diff -r` 比 fork 的 `src/` 和 cargo 的 registry 裡的 `taffy-0.15.0/src/`、`Cargo.toml` 和 `Cargo.toml.orig`：一樣；`Cargo.lock` 只少了 taffy 的 `source` 和 `checksum`，依賴的版本不變 |
| machopo 用 path 依賴編 fork，排版不變 | `cargo test -p layout`、`cargo test --workspace` |
| `ci.py patches` 匯出：一個 commit 一個 patch、不編號、沒有簽名、完整的 blob id、內容每次一樣、換掉舊的、CRLF 的檔案和二進位檔照原樣、shallow 的 checkout 先加深、不是從 `upstream_rev` 長出來的不匯出、checkout 不在 pin 上時不匯出 | `python3 scripts/ci_patches_test.py`（`Patches`、`Commands`） |
| `ci.py third-party` 檢查 patch：沒匯出的 commit、被改過的、套不上的（指出是哪一個）、有 patch 卻沒有 `upstream_rev` 都報出來；checkout 的 index 和檔案不動；shallow 的 checkout 只拿 `upstream_rev` 這一個 commit | `python3 scripts/ci_patches_test.py`；在這台機器上 `python3 scripts/ci.py third-party` 對 taffy 和 parley 通過 |
| shallow 和完整的 checkout 匯出的 patch 一樣 | parley 的 checkout 原本是 shallow 的，第一次匯出時加深；之後在完整的 checkout 再匯出一次，每個檔案的 md5 不變 |
| `LayoutUpdate::laid_out_beside_floats`：因為旁邊有 float（或把自己的 float 放進共用的 block formatting context）沒用 taffy 的快取的盒子 | `layout` 的 `float::boxes_beside_floats_are_laid_out_without_the_cache`：沒有 float 時 0，兩個段落都在 float 旁邊時 2，只有一個時 1 |
| `LayoutUpdate::lines_broken_again_beside_floats`：float 旁邊的行從頭重斷的次數，四種原因 | `float::lines_beside_floats_are_broken_again`：沒有 float、float 旁邊放得下時 0；往下找更寬的地方、高了之後在更窄的地方、拿掉移到下一行的 float、放好自己的 float 之後，各自的次數 |
| `LayoutUpdate::lines_broken_for_fragments`：做 fragment 時在最後的寬度真的又斷了一次 | `inline::lines_are_broken_again_in_a_box_wider_than_they_were_broken_at`：`min-width` 撐寬的 float 是 1（行在盒子的寬度對齊）；`inline::lines_are_broken_once_for_their_fragments`：下一列的情況是 0 |
| fragment 用最後排版時斷的行；寬度由內容決定的盒子的行在 fit-content 的寬度斷，被給了寬度的在那個寬度斷 | `inline::lines_are_broken_once_for_their_fragments`（確定的寬度、shrink-to-fit 的 float（放得下的、沒有空間的、自己有寬度的）、從 border box 算回來有浮點誤差的寬度：都斷 1 次、做 fragment 時 0 次）、`inline::a_float_aligns_its_lines_in_its_own_width`、`inline::a_shrunk_box_is_as_high_as_its_lines_at_its_width`、`inline::lines_with_floats_are_broken_once_in_the_width_their_box_is_given`（float 的 button、被給了寬度的盒子裡帶 float 的行） |
| `BoxTreeUpdate::font_family_names`：照名稱在字型集合裡查的次數。一個 `font-family` 的值第一次用來 shape 時每個名稱查一次，之後不查 | `font_selection::a_font_family_value_is_looked_up_once`：block 的清單、inline 元素的、`display: contents` 的元素裡的 run 的，第一次各多幾個名稱就多幾次；看過的值（在別的 block、別種 span、別的字級）是 0；順序不同是另一個值；generic 不算名稱；沒有 shape 時 0。`update::` 的測試比較 `BoxTreeUpdate` 時不比這個欄位 |
| 註冊字型之後，`font-family` 的值重新查 | `font_selection::adding_fonts_looks_the_family_names_up_again`：註冊前名稱找不到、用 Ahem；註冊之後換字級重新 shape，查了名稱、用到新的字型 |
| 一個值查到的是集合裡有的名稱的 family 和 generic 代表的 family，照清單的順序 | `layout` 的 unit test `font_families::tests::a_value_is_the_families_the_collection_has`；`font_selection::a_generic_family_is_its_families` |
| 空的行的高度照 block 的字型 | `font_selection::a_line_with_nothing_on_it_is_in_the_blocks_font`：`x\n\nx` 的中間那行和有字的行一樣高，Lato 的和 Ahem 的不一樣；`shell` 的 `focus::focusing_scrolls_into_view_if_needed`（編輯區結尾的空行、空的 input） |
| canvas 的文字照它的字型家族 | `font_selection::a_canvass_text_is_in_its_families`：Ahem 的寬度是 40、Lato 的不是，第二次（快取裡的）一樣；reftest 的 `canvas/c5-text.html` |
| 計數都在 release build 裡 | `RUST_LOG=layout=debug` 的 release build 印出 `layout`、`build box tree`、`update box tree` 三行的欄位（量測用的就是這些） |
| parley 的 fork 是上游的 main（`1b67b753`，2026-11-08）加上 11 個各做一件事的 commit 和 `README.machopo.md` 的 commit，每個 commit 帶 fork 自己的測試和完整的訊息 | fork 的 `cargo test -p parley -p parley_engine -p parley_tests -p fontique`、clippy、`cargo fmt --all -- --check`；`python3 scripts/ci.py patches parley` 匯出 12 個 patch，`ci.py third-party` 確認它們套在 `upstream_rev` 上和 `rev` 一樣 |
| `font-optical-sizing: auto` 的文字用字級的 `opsz` shape 和畫，`font-variation-settings` 自己設的贏，關掉時沒有 | `layout` 的 `font_features::a_variable_fonts_axes`：軸的值，和 advance（自動的和把 `opsz` 設成字級的一樣，和關掉的、設成 40 的都不一樣） |
| 只被問寬度的盒子有 `text-indent` 時不斷行 | `inline::a_width_alone_breaks_no_lines`：`max-content`、`fit-content` 加縮排的寬度是 110、斷行 1 次 |
| machopo 配合上游的 API，除了 atomic inline 的 bidi 和斷行，排版不變 | `cargo test -p layout`、`-p shell`；`test.py reftest` 的 `text/`、`inline/`、`floats/`、`generated/`、`canvas/`、`svg/`、`block/`、`position/`、`flex/`、`grid/`、`tables/`、`hidpi/`；`test.py wpt` 的 `css/css-text`、`css/css-text-decor`、`css/CSS2/linebox`、`css/CSS2/normal-flow`、`css/css-overflow/line-clamp`、`css/css-sizing`、`css/css-display`、`css/css-pseudo`；差異逐項記在「做過的檢查」 |
| taffy 的 fork 是上游的 main（`4aca4f49`，2026-11-06）加上 `README.machopo.md` 的 commit | fork 的 `cargo test`；`python3 scripts/ci.py patches taffy` 匯出 1 個 patch，`ci.py third-party` 確認它套在 `upstream_rev` 上和 `rev` 一樣 |
| machopo 配合 taffy 的 API，沒有退步；上游修好的 18 個 WPT 測試通過 | `cargo test -p layout`、`-p shell`；`test.py reftest` 的 12 個目錄；`test.py wpt` 的 27 個目錄（「第 4a 段」） |
| 第 2 段的計數不變 | `tests/sites/` 的 7 頁再數一次，每頁每個計數和第 2 段的一樣 |

## 做過的檢查

**第 1 段**（2026-11-10，Windows 11）：

- **fork**：`gh repo fork DioxusLabs/taffy` 建 `wusyong/taffy`；`machopo` 分支從 v0.15.0（`eb645069`）開出來，一個 commit（`bfa9eda2`，`README.machopo.md`），push 到 fork。
- **source 一樣**：fork 的 `src/` 和 crates.io 的 0.15.0 逐檔一樣，`Cargo.toml` 和 crates.io 的 `Cargo.toml.orig` 一樣。第一次比的時候每個檔案都不同：這台機器的 `core.autocrlf=true` 讓 clone 的檔案變成 CRLF；`core.autocrlf false` 之後重新 checkout 就一樣了。所以這一段沒有跑 reftest 和 WPT：編出來的程式和之前同一份 source、同樣的依賴。
- **patch**：taffy 1 個、parley 16 個（parley 現在的 commit，原樣匯出；第 3 段重寫）。`patches/` 標成 `-text`：上游有 CRLF 的檔案（taffy 的 `CONTRIBUTING.md`、parley 的幾個字型的 LICENSE），改到時 patch 帶著 CR，換成 LF 就套不上。
- **新的測試**：`scripts/ci_patches_test.py`，15 個，加進 CI 的 `lint`。
- **mutation test**（`mutants-1.toml`，`python3 scripts/test.py mutate rfcs/0092-taffy-parley-forks/mutants-1.toml`）：16 條全部抓到。第一次跑 17 條、3 條沒抓到：
  - 拿掉 `--binary`：等價，`format-patch` 本來就給二進位的 diff。改成不傳這個選項，註解寫明，這條 mutation 拿掉。
  - 檢查用 checkout 自己的 index：測試的 index 本來就等於 HEAD，用自己的 index 讀樹、套 patch 之後還是一樣。改成先 stage 一個改動，確認檢查之後 staged 的內容還在。
  - 抓 `upstream_rev` 時連同它的歷史：結果一樣，只是多下載。補上 shallow 的 checkout 沒有 `upstream_rev` 的 parent 的斷言。
- **跑過的檢查**：`cargo test -p layout`；`cargo fmt --check`；`cargo clippy --workspace --all-targets -- -D warnings`；`python3 scripts/sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`python3 scripts/ci.py third-party`；`cargo test --workspace`（55 個 test binary，3739 個通過、1 個 ignored；`bindings` 的 `gc_platform` 印出的 panic 是測試本來就要的）。

**第 2 段**（2026-11-10，Windows 11）：

- **計數**：`LayoutUpdate` 的 `laid_out_beside_floats`、`lines_broken_for_fragments`、`lines_broken_again_beside_floats`，`BoxTreeUpdate` 的 `font_family_names`。字型名稱在 `InlineContent::new` 裡數（block 的 `text_style`、每個元素的 span、和外面的 span 的 style 不同的 run 的 span），累計在 `TextContext`，由建 box 的兩個地方拿走、加進 `BoxTreeUpdate`。`BoxTree::build` 也印一行 `build box tree`（原本只有 update 印）：第一次載入的 shaping 才數得到。
- **新的測試**：`layout` 的 `float::boxes_beside_floats_are_laid_out_without_the_cache`、`float::lines_beside_floats_are_broken_again`、`inline::lines_are_broken_again_for_their_fragments`、`font_selection::each_span_gives_parley_its_family_names`。`update.rs` 的 `update()` 改成回傳只比 `full`、`built`、`shaped`、`restyled`、`svg_captured` 的值：字型名稱的數目跟平台的字型清單有關（Linux 多一個標準字型），在字型的測試裡用相對的數目比。
- **mutation test**（`mutants-2.toml`，`python3 scripts/test.py mutate rfcs/0092-taffy-parley-forks/mutants-2.toml`）：18 條全部抓到。第一次 17 條、1 條沒抓到：「匿名 block 不把它的名稱加進去」。測試裡匿名 block 之後還有一個空的 `<div>`，它的內容走另一個地方，沒有 shape 也拿走累計的名稱，總數不變；改成不走那裡的子 block（空的 flex container）之後抓到。沒有 mutation 的：`family_names` 的 `Single`、`Source` 兩支（`text_style` 一律給 `List`，走不到）；`tracing::debug!` 的欄位（只有量測的 script 讀）。
- **跑過的檢查**：`cargo test -p layout`；`cargo fmt --check`；`cargo clippy --workspace --all-targets -- -D warnings`；`python3 scripts/sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`cargo test --workspace`（55 個 test binary，3743 個通過、1 個 ignored）。這一段只加計數、不改排版，所以沒有跑 reftest 和 WPT。
- **找到的 bug**（還沒修，記在 `docs/limitations.md` 的「Layout」）：`display: flow-root` 裡有 float 和比盒子窄的行內內容時，行和 float 是在 max-content 的寬度斷和放的，最後的寬度沒有再斷一次：右 float 放在 max-content 寬度的右邊（`<div style='display: flow-root'><div style='float: right; width: 10px; height: 5px'></div>X</div>` 在 800px 寬的 body 裡，float 在 x=20，Chrome 在 x=790）。做 fragment 時行在最後的寬度又斷了一次（所以文字的位置是對的），但不放 float。`lines_broken_for_fragments` 的測試因此不用 flow-root 的例子。

**第 3a 段**（2026-11-10，Windows 11）：

- **fork 的改寫**：舊的 16 個 commit 先合成分組（tree 和 `cefcbc5` 一樣），再一組一組搬到上游的 `1b67b753`（66 個上游的 commit，parley 0.12.0、harfrust 0.15.0）。改寫後 10 個 commit 加 README：元素的邊緣（`InlineBoxKind::StartEdge`、`EndEdge`）、最後一個強制換行之後的行、沒有 em 的字型、第一個字型、tab stop、optical sizing、軟連字號、共用 shaped text、shaping 的快取（段落的和字的）、斷行表。舊的「atomic inline 照它所在的 span 斷行」和 `InlineBoxLineBreak` 沒有搬：照上游的做法，atomic inline 是文字裡的 U+FFFC。
- **machopo 配合的地方**：`layout` 的 `text.rs`（摺空白時在 atomic inline 的位置放 U+FFFC、邊緣用 `StartEdge`/`EndEdge`、`edit_inline_box`、斷行前 `set_alignment`、`line.metrics`）、`canvas_text.rs`、`svg_text.rs`、`shell` 的 `ui_text.rs`（`run.font()`）；`read-fonts` 0.46、`write-fonts` 0.55。
- **和舊的 fork 不同的行為**：
  - atomic inline 照 U+FFFC 參與 bidi 和斷行（決定的事項）。
  - 邊緣放不下時和文字一樣處理（舊的 fork 把它留在行上）。fork 的 `test_edges` 守著。
  - 上游的 `calculate_content_widths` 現在把 `text-indent` 算進去：有長度的 `text-indent` 的盒子在確定的寬度裡 shrink-to-fit 時寬度含縮排（原本少了縮排、可能多折一行，`docs/limitations.md` 的那一條拿掉）。`inline::a_width_alone_breaks_no_lines` 加了 `fit-content` 和 float 的兩個例子（把縮排改回不算時是 100，不是 110）；`min-content` 加縮排的例子斷行次數從 2 變成 1（min-content 的寬度直接含縮排，最後的 layout 命中上一次斷的行）。只被問寬度時遇到縮排仍照舊斷行（`InlineContent::indented`），這個繞路在 3b 拿掉。
  - 上游不在零寬的格式字元之後加 `letter-spacing`：`css/css-text/letter-spacing/letter-spacing-control-chars-001.html` 從 FAIL 變成 PASS，預期結果的 `.ini` 拿掉。
  - `text.rs` 統計的 cluster 數每個 atomic inline 多 1（U+FFFC）。
- **查到的事：default ignorable 開頭的字**。`parley_shaped_text::a_spans_glyphs_are_its_runs`、`a_spans_clusters_are_its_runs` 在 debug build 的比對（以字為單位的 shaping 和整段的）失敗：glyph 相同，只有 U+202E 的 cluster 的 `SAFE_TO_BREAK_BEFORE` 不同。原因：字型自己不放 mark（沒有 GPOS 的 mark 規則）時，harfrust 的 fallback mark positioning 把 base 之後的 default ignorable 和 base 當成一組（`unsafe_to_break`），整段 shape 時空白和它後面的 default ignorable 之間不能切；以字為單位時它們是兩個字。新的 harfrust 才有這個行為。修法：default ignorable 開頭的字和前面的空白一起 shape（`WordRuns::joins`），fork 的 `test_shape_cache` 的 `a_default_ignorable_after_a_space_is_shaped_with_it` 守著（Roboto 和空白什麼都不碰的 Noto Sans CJK SC 各一次）。parley 和 harfrust 的 default ignorable 的集合可能有小的差異；debug build 的比對會抓到。
- **fork 的測試還是精簡的**：快取、第一個字型和 `space.rs` 的完整測試仍在 machopo（`parley_space::`、`parley_words::` 等），3b 搬進 fork。machopo 的 `parley_inline_box.rs` 拿掉測舊行為的兩個案例（`a_start_edge_goes_with_an_atomic_inline`、`a_box_first_on_a_nowrap_line_doesnt_break_it`），由 fork 的 `test_edges`、`test_final_break` 守著。多數 commit 的訊息只有標題，3b 寫上游的草稿時補。
- **mutation test**（fork 裡的，scratchpad 的一次性 script：改 fork 的檔案、跑 fork 和 machopo 的測試、還原）：
  - 邊緣 24 條全部抓到；最後的強制換行 6 條、4 條抓到、2 條等價；沒有 em 的字型 1 條抓到；第一個字型 2 條由 `parley_tests` 抓到；tab stop 16 條、15 條抓到、1 條走不到；optical sizing 7 條、6 條抓到、1 條等價；軟連字號 30 條、27 條抓到、3 條等價；共用 shaped text 11 條、10 條抓到、1 條等價；shaping 的快取 16 條、11 條抓到、4 條等價、1 條是缺口（帶 safe-to-concat 旗標時照樣整段：parley 一律不要這些旗標，走不到；記在 `docs/test-gaps.md`）；斷行表 10 條、9 條抓到、1 條等價。
  - default ignorable 的規則 5 條：不合併、不看是不是 default ignorable、空白什麼都不碰的字型不合併、碰得到的字型不合併，4 條抓到（第三條第一次只有 machopo 的測試抓到，fork 的測試補了 Noto Sans CJK SC 的例子）；「不看前一個字是不是空白」等價（記在 `docs/test-gaps.md`）。
  - 沒有測試的：24-bit 的 GSUB、GPOS 格式一律當成跨過空白（記在 `docs/test-gaps.md`）。
- **成本**：有非零寬度 atomic inline 的 layout 每次 build 複製 shaped text（改寬度時 shaped text 不能和快取共用）；段落快取的 key 多了 scale，字的快取的 key 多了 `point_size`。
- **跑過的檢查**：fork 的 `cargo test -p parley -p parley_engine -p parley_tests -p fontique`（11、169、37、252 個通過、1 個 ignored）、clippy（只有上游 manifest 的警告）、`cargo fmt --all -- --check`（只有 Windows checkout 的換行警告）；machopo 的 `cargo test -p layout`（25、469）、`cargo test -p shell`（70、11、550）；reftest 12 個目錄 112 個全部符合預期；WPT 的 8 個目錄 9,611 個檢查，只有上面那一個從 FAIL 變 PASS；`tests/sites/` 的計數和第 2 段一樣；`python3 scripts/ci.py third-party`。WPT 一次給太多目錄時 Windows 的指令列太長（`WinError 206`），分成幾次跑。
- **rebase 到 `main` 的 `743514d`**（2026-11-10）：`rfcs/README.md` 的清單衝突兩次（另一份 RFC 的那一列照 `main`，這一份的照這個 branch），其他自動合併；之後 `cargo test -p layout`（25、469）通過，`cargo test -p shell` 的 `devtools_initiators::script_asks_with_its_stack` 不穩定（單獨跑 6 次失敗 2 次：沒有 `href` 的 `<link>` 之後拿到的是前一段 script 的 stack，`bare` 而不是 `giveHref`），其他 562 個通過。這個測試和它測的程式這個 branch 都沒有動過。看程式推測的原因（還沒驗證）：沒有 load 拿走的 stack 在 render thread 一輪的最後才清掉（`clear_request_stacks`），兩個 `Runtime.evaluate` 落在同一輪時第二個拿到第一個留下的。rebase 前跑過一次是通過的，沒有重複跑，不能確定 `main` 上有沒有。

**第 3b 段**（2026-11-10，Windows 11）：

- **optical sizing 由 machopo 做**：`layout` 的 `font_features::variations` 在 `font-optical-sizing: auto`、`font-variation-settings` 沒有設 `opsz` 時，在給 parley 的 variation 最前面加字級的 `opsz`（Blink 的 `FontPlatformData` 也是在自己這一側加）；沒有這個軸的字型不理它。fork 的「Size the font optically」和 `StyleProperty::OpticalSizing` 拿掉，fork 剩 9 個 commit 加 README。成本：有 optical sizing 的 style（預設就是）每次 `text_style` 多一個 variation，variation 的清單不再是空的，所以多一次小的配置；量測沒有另外做。reftest 和 WPT 的結果和 3a 一樣（下面）。
- **拿掉 `InlineContent::indented`**：上游的 content widths 已經含縮排，只被問寬度時不用為了縮排斷行。`a_width_alone_breaks_no_lines` 的 `max-content` 和 `fit-content` 加縮排的兩個例子斷行次數從 2 變成 1，寬度不變（110）。float 加縮排的那個例子仍是 2：float 本來就先在給它的空間斷一次、再在自己的寬度斷一次，和縮排無關。
- **測試搬進 fork**：machopo 的 `parley_inline_box.rs`、`parley_shaped_text.rs`、`parley_space.rs` 刪掉，`parley_words.rs`、`parley_line_breaks.rs` 只留下面說的幾個。fork 多了 `test_shape_words`（現做的 `Made`、`Reaching` 兩個字型）、`test_shape_words_space`（`space.rs` 的規則，每個測試現做一個只有一兩條規則的字型），`test_edges` 多兩個案例（空的盒子的邊緣、字中間的邊緣不多一個斷行機會），`test_font_without_em` 多一個沒有 `head` 的案例；fork 的 `parley` 多一個 dev-dependency `write-fonts`，machopo 的 workspace 不再依賴它。
- **留在 machopo 的**（比決定的事項多一類）：經過 machopo 的 layout 才看得到的（`parley_line_breaks` 的兩個：`TextContext` 的文字查表、有其他規則的交給 ICU4X），和**要用 fork 的 repository 沒有的字型的**（`parley_words::text_in_lato_is_shaped_whole` 用真的 Lato-Medium，`words_beside_a_space_a_font_shapes_with_them_are_shaped_again_as_one` 用 WPT 的字型）。
- **查到的事：計數看的是這一次 shape 的字**。`text_in_lato_is_shaped_whole` 加 Ahem 的對照時，同一個 context 先排過別的文字，對照的斷言拿到 `(4, 1)` 而不是 `(2, 3)`：字的快取裡已經有前一段文字的字，命中的不算 shape。對照改用新的 context。
- **commit 的訊息**：9 個 commit 都補了完整的訊息（改了什麼、為什麼、怎麼測），寫成可以直接送上游的樣子。
- **上游的草稿**（`upstream/parley/`，只在本地，沒有送出任何東西）：最後的強制換行、沒有 em 的字型、斷行表。上游的 PR 範本規定說明不能是 LLM 寫的、第一行要揭露 LLM 的參與，所以草稿寫成材料（改了什麼、為什麼、怎麼測、reviewer 可能問什麼），送出的人照它自己寫；斷行表建議先開 issue，送出前要用上游的 `parley_bench` 量測，`skip_word_boundaries` 拆成另一個 PR。
- **mutation test**：
  - machopo（`mutants-3b.toml`，`python3 scripts/test.py mutate rfcs/0092-taffy-parley-forks/mutants-3b.toml`）：`opsz` 的 5 條全部抓到。第一次 4 條抓到、1 條沒抓到：「加的是另一個軸」（`wdth` 設成字級，advance 一樣和關掉的不同）；測試加了一個關掉 optical sizing、把 `opsz` 設成字級的例子，advance 要和自動的一樣。另一條等價，沒有放進去：`font-variation-settings` 設了 `opsz` 時照樣加字級的（設定排在後面，後面的贏；記在 `docs/test-gaps.md`）。
  - 拿掉 `indented` 沒有可以弄壞的程式（是刪掉的）：斷行次數的斷言在拿掉之前是 2、之後是 1，測試分得出兩者。
  - fork（scratchpad 的一次性 script，只跑 fork 自己的測試，確認測試搬過去之後不靠 machopo 也抓得到）：沒有 em 的字型 3 條全部抓到（不檢查、沒有 `head` 的收、em 是 0 的收）；第一個字型 1 條抓到；共用 shaped text 11 條、10 條抓到、1 條等價（和 3a 一樣）；shaping 的快取 16 條、12 條抓到，沒抓到的 4 條和 3a 的一樣（3 條等價、1 條是 safe-to-concat 的缺口）；`space.rs` 的規則新寫 22 條、19 條抓到，第一次 3 條沒抓到：
    - 「空白後面的 glyph 太多時不整段」：測試只有空白前面的；補了後面的（`a_font_with_many_glyphs_beside_a_space_shapes_text_whole`），抓到。
    - 「`kern` 的 pair 把空白後面的 mark 也算進去」：等價，只是多合併一些字、比較慢（HarfBuzz 的 kerning 跳過 mark）。
    - 「讀不懂的表當成不碰空白」：缺口，沒有壞掉的表的測試字型（記在 `docs/test-gaps.md`）。
- **跑過的檢查**：fork 的 `cargo test -p parley -p parley_engine -p parley_tests -p fontique`（11、191、37、252 個通過、1 個 ignored）、clippy（只有上游 manifest 的警告）、`cargo fmt --all -- --check`（只有 Windows checkout 的換行警告）；machopo 的 `cargo test -p layout`（25、432：少的是搬走的測試）、`cargo test -p shell`（70、11、563）；reftest 的 12 個目錄 104 個全部符合預期；WPT 的 `css/css-text`、`css/css-text-decor`、`css/CSS2/linebox`、`css/CSS2/normal-flow`、`css/css-fonts`、`css/css-overflow/line-clamp`、`css/css-sizing`、`css/css-display`、`css/css-pseudo` 共 14,151 個檢查，全部符合預期（沒有預期結果要改）；`python3 scripts/ci.py third-party`；`python3 scripts/sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。階段（3a 和 3b）最後：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（55 個 test binary，3727 個通過、1 個 ignored）。

**第 4b 段**（2026-11-10）：取消，沒有改程式。

- **查到的事**：上游的 `find_content_slot` 回傳的 `ContentSlot::height` 是這個寬度往下到被更窄的 segment 縮小為止的距離。它不是 segment 的底（變寬的地方不算），後面沒有更窄的 segment 時是無限大。`float_bands` 要的是每一段的頂端和寬度，包括變寬的地方（`FloatBands::room` 用下一個 band 的頂端當這一個的底），最後一段還要最低的 float 的底，所以一段一段用 `after` 走、最後問 `cleared_threshold(Clear::Both)` 都拿不掉。
- **階段最後的檢查**（第 4 段到 4a 為止）：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（55 個 binary、3727 個通過）。

**第 4a 段**（2026-11-10，Windows 11）：

- **rebase**：fork 的 `machopo` 分支從 v0.15.0（`eb645069`）搬到上游 main 的頂端 `4aca4f49`（2026-11-06，中間 40 個 commit），只有 `README.machopo.md` 的 commit，沒有衝突；README 的起點和版本跟著改。舊的頂端 `bfa9eda2` 有 tag `machopo-2026-11-10`。fork 沒有 `upstream` 這個 remote，照 README 用 URL fetch。
- **machopo 的配合**（`crates/layout/src/lib.rs`，4 處）：`find_content_slot` 多的 `height` 傳 0（和原本一樣只看頂端那一段）；`apply_content_box_inset` 的 `[f32; 2]` 換成 `FloatInsets`；`place_floated_box` 多的 `id` 傳盒子的 node id（現在沒有人讀，6a 的 `floats_placed_since` 會用）。`ScrollableOverflow`（#1372）不用改：machopo 只把 taffy 給的原樣傳回去，scrollable overflow 是自己從 fragment 算的。`calc()` 的 handle（#1400）還沒合併。
- **WPT**：27 個目錄共 27,129 個檢查，沒有退步；18 個原本預期失敗的測試通過，`.ini` 刪掉（括號裡的上游 PR 是照它的說明對上的，沒有逐一 bisect）：
  - `css/CSS2/floats/floats-wrap-top-below-bfc-00{1,2,3}{l,r}.xht`（6 個）：自己是 BFC 的盒子照整個高度避開 float（#1385、#1386）。
  - `css/CSS2/floats-clear/float-replaced-height-001.xht`、`floats-101.xht`：BFC 的根在 float 下面加上自己的 padding 和 border（#1384）。
  - `css/CSS2/floats/floats-wrap-bfc-with-margin-010.html`、`floats-zero-height-wrap-002.xht`、`zero-width-floats-positioning.tentative.html`：高度是 0 的 float 記成一段（#1387）、slot 的寬度分開扣兩邊的 float（#1392）。
  - `css/CSS2/floats-clear/adjoining-float-nested-forced-clearance-002.html`、`margin-collapse-157.xht`：摺穿的第一個子項的 margin 不再加到 block 的底（#1397）。
  - `css/CSS2/floats-clear/floats-031.xht`：原本記成「沒有逐一查」的 float 的 replaced 元素，哪一個 commit 修好的沒有查。
  - `css/css-flexbox/align-content-007.htm`：`wrap-reverse` 溢出的 line 靠 flex-start（#1363）。
  - `css/css-grid/alignment/grid-self-alignment-baseline-gfc-001.html`、`css/css-lists/list-and-grid-001.html`：grid container 的 baseline 取第一個 item 最後 layout 的（#1322、#1323）。
  - `css/css-contain/contain-paint-clip-006.html`：原本記的原因是 reference 的 `overflow: hidden` 自己成一層、文字分開 raster 的 anti-aliasing；現在通過的原因沒有查（猜是 BFC 的根的高度變了之後沒有東西可以捲）。它和 `list-and-grid-001`、`floats-031`、`margin-collapse-157` 另外各跑了 3 次，都通過。
- **還在的限制**（`docs/limitations.md` 跟著改）：寬是 0 的 float 旁邊的 BFC 盒子（`zero-width-floats.html`、`zero-space-between-floats-003.html`）；BFC 盒子移到比較窄的地方之後沒有照新的寬度重排（`floats-bfc-003.html`；上游的 #1396 還沒合併）。這三個 `.ini` 的註解改成現在的原因。flex、grid 那一條拿掉 grid container 的 baseline，「不 fork taffy」改成現在的做法。
- **沒有新的測試和 mutation test**：machopo 這一段沒有新的行為（4 處都是照新的簽名傳一樣的東西），行為的變化是上游的、由上游的測試和上面的 WPT 守著。
- **跑過的檢查**：fork 的 `cargo test`（188、161、6689、5 個通過、4 個 ignored）；machopo 的 `cargo test -p layout`（25、432）、`cargo test -p shell`（70、11、563）；reftest 的 `block/`、`floats/`、`flex/`、`grid/`、`tables/`、`inline/`、`position/`、`text/`、`images/`、`generated/`、`scroll/`、`hidpi/` 共 84 個，全部符合預期；WPT 的 `css/CSS2/floats`、`floats-clear`、`normal-flow`、`margin-padding-clear`、`positioning`、`linebox`、`visudet`、`ui`、`css/css-flexbox`、`css-grid`、`css-align`、`css-sizing`、`css-overflow`、`css-position`、`css-tables`、`css-contain`、`css-display`、`css-box`、`css-lists`、`css-content`、`css-inline`、`css-images`、`css-text`、`css-logical`，和 `css/cssom-view`、`css-scroll-anchoring`、`css-pseudo`（`cssom-view/scroll-behavior-smooth-navigation.html` 出現一次已經標過的 known intermittent）；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`python3 scripts/ci.py third-party`。WPT 一次給三個大目錄時 Windows 的指令太長（`WinError 206`），`css-flexbox` 到 `css-display` 那九個是一次跑一個。

**第 5a 段**（2026-11-10，Windows 11）：

- **fork 的兩個 commit**：「Take font families as ids already looked up」（`StyleProperty::FontFamilyIds`，`test_family_ids`）和「Set a property of a tree builder's root style」（`TreeBuilder::push_default`，`test_tree_default`）。fork 的頂端 `227d7191`，只在本地。
- **為什麼多一個 `push_default`**：5b 第一次的做法是 root style 不帶家族、所有文字外面包一個帶 id 的 span。`cargo test -p shell` 的 `focus::focusing_scrolls_into_view_if_needed` 穩定失敗（編輯區結尾 `0,3920`，預期 `3910`；input `0,8419`，預期 `8420`）：parley 的 root style 是沒有文字的 layout 和空的行的 style，行高來自它的字型，span 管不到。`cargo test -p layout` 當時全過，也就是 layout 的測試沒守住空的行的高度；5b 補了（`a_line_with_nothing_on_it_is_in_the_blocks_font`）。
- **fork 的 mutation**（scratchpad 的一次性 script，只跑 fork 自己的測試）：`FontFamilyIds` 4 條全部抓到。`push_default` 6 條全部抓到（不更新 style table、不更新 tree、不 assert、不檢查 span、不檢查文字、不檢查等著合併的空白）；最後一條第一次沒抓到：測試的 root 的 `white_space_collapse` 是預設的 `Preserve`，空白直接變成文字、被文字的檢查擋下；測試改成 root 明確用 `Collapse` 並確認文字是空的。
- **fork 的 fmt**：第一個 commit 的 `test_family_ids.rs` 原本沒有照 rustfmt 排（當時的 `--check` 被 Windows checkout 的換行警告蓋過，沒看到 diff）；改進那個 commit 裡。
- **跑過的檢查**：fork 的 `cargo test -p parley -p parley_engine -p parley_tests -p fontique`（11、201、37、252 個通過、1 個 ignored）、clippy（只有上游 manifest 的警告）、`cargo fmt --all -- --check`；`python3 scripts/ci.py patches parley`（12 個 patch）、`ci.py third-party`。

**第 5b 段**（2026-11-10，Windows 11）：

- **做法**：`layout` 的 `FontFallbackMap`（`font_families.rs`）以清單的值為 key 記查到的 `FamilyId`；`InlineContent::new`、`shape_line`、canvas 的 `shape` 在建 parley 的 builder 之前先查用到的每個值（查要借 `FontContext`，builder 也借它），之後 span 的 `FontFamilyIds` 借快取裡的清單。root 的用 `push_default` 給。`register_fonts_for_testing` 清掉快取。`computed_style` 的 `FontFamily`、`GenericFamily` 多了 `Hash`。
- **計數改義**：`font_family_names` 原本數每個 span 給 parley 的名稱，現在數真的照名稱查的次數。沒有重新量測各頁的數目（第 2 段的表是原本的意義）：看過的值是 0，一頁的次數是它用到的不同的值的名稱數的總和。
- **mutation test**（`mutants-5.toml`，`python3 scripts/test.py mutate rfcs/0092-taffy-parley-forks/mutants-5.toml`）：11 條全部抓到。過程中沒抓到、補了測試的兩條：
  - 「generic 也算成查了名稱」：計數的測試沒有 generic；加了一個帶 `serif, cursive` 的值。
  - 「generic 代表的 family 不在 id 裡」：測試的字型集合每個 generic 都是 Lato，沒有 family 時 parley 退回的也是 Lato，整合測試分不出來；加了 `font_families` 的 unit test，用自己的集合（`serif` 對到 Ahem）直接比查到的 id。
- **跑過的檢查**：`cargo test -p layout`（26、436）、`cargo test -p shell`（70、11、563：focus 的測試回到通過）；reftest 整套 195 個檢查全部符合預期；WPT 的 `css/css-text`（2,751 個檢查）、`css/css-fonts`（4,540）、`css/CSS2/linebox`（728）、`css/css-text-decor`（1,355）、`html/canvas/element/text`（315）全部符合預期，沒有預期結果要改。階段最後的 `cargo test --workspace`（3,732 個通過、1 個 ignored）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的七項、`ci.py third-party`。

**第 6a 段：float 的快取**（2026-11-10，Windows 11）：量了之後不做，程式沒有改動。

- **會命中的上限**（release build，暫時的計數，量完還原）：`compute_box` 裡每個盒子多留一份 taffy 的 `Cache`，只給 float 旁邊的計算用，數同一個盒子、同樣的 `LayoutInput` 又算一次的次數，和結果（大小、上下的 margin）跟上次一樣的次數。另外數這些計算底下一共做了幾次盒子的計算、斷了幾次行（巢狀的只算一次）。

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

  又算的那些結果全部和上次一樣；省掉的盒子的計算數等於命中數（每次命中只省那個盒子自己的）。這是上限：真的 key 還要 float 的狀態一樣。
- **時間**（`bench.py blink-perf --pages`，machopo `60bf10a` 的 `perf` profile 對 Chrome for Testing 152，各一次）：`floats_show_hide` 21.1 對 19.3 runs/s（machopo 的時間是 Chrome 的 0.91 倍）、`floats_2_100_nested` 23.2 對 22.2 ms（1.05 倍）、`floats_20_100_nested` 43.5 對 53 ms（0.82 倍）。單次的量測，只能讀成大致打平。
- **沒做的**：沒有取樣 `compute_box` 佔這幾頁多少時間（`blink-perf` 沒有取樣的選項，上面的結果已經夠決定）。

**第 6a 段：column flexbox 的寬度**（2026-11-10，Windows 11）：

- **fork**（taffy 的 `fadc44a6`「Don't measure the items' heights for the width of a single-line column」，頂端 `0de7a64f`；舊的頂端 `05e64bd4` 打了 tag `machopo-2026-11-10-4a`）：`cargo test` 全過，6689 個 xml fixture 沒有一個改預期；`caching.rs` 的 `measure_count_flex_in_grid` 的 column 從 10 次 measure 變 6 次；新的 `tests/hand_written/column_width.rs` 5 個測試。`cargo fmt --check` 過，clippy 沒有新的警告。**沒做的**：沒有用 taffy 的 `rust-version`（1.71）編過（這台機器沒裝），送上游前要做。
- **和完整演算法不同的地方**：fixture 都沒變，但不是嚴格等價。完整的演算法量子項的寬度時給它 flex 之後的高度，捷徑給它自己的 style 的高度（夾在 min、max 裡、不小於 padding 加 border），沒有的話不給。寬度會跟著給的高度變、又不是經由 `aspect-ratio` 的子項（measure function 自己用高度算寬度的 leaf、換行的 column flexbox、有比例的子項以外靠高度決定寬度的 container）結果會不同；`column_width.rs` 的 `item_available_height` 的第一個斷言在完整演算法下是 10、捷徑是 70（刻意做的、寬度等於可用高度的 leaf）。machopo 原本的捷徑也是這樣（它的 debug 對照從沒被踩到），Blink 的 `ComputeMinMaxSizes` 也只用子項自己的貢獻。記在 `docs/test-gaps.md`。
- **fork 的 mutation test**（scratchpad 的一次性 script，每條跑 fork 的 `cargo test --no-fail-fast`）：12 條全部抓到：不限 column（89 個失敗）、不限寬度（152）、換行的也走（`wrapping_column`）、有比例的也走（`item_with_aspect_ratio`）、不設已知的容器高度（`item_available_height`）、不算 line 的 cross size（87）、不扣子項的 margin、不用 style 的高度、不夾 min 和 max、不以 padding 為下限（都是 `item_with_height`、`item_available_height`）、容器高度未知時也用它當可用高度、永遠不走捷徑（`measure_count_flex_in_grid`）。過程中沒抓到、補了測試的：換行、比例、容器高度、margin、可用高度（第一輪 5 條，補了 `column_width.rs`），padding 的下限（補了 `item_with_height` 的一個斷言）。第一版的 `item_with_aspect_ratio` 用 leaf，完整演算法也不經由比例換算（leaf 不把已知的高度經比例轉成寬度），改成 block 的子項才分得出來。
- **machopo**：`column_flex_width`、`widest_column_item` 和 debug 的對照拿掉（`lib.rs` 少 198 行）。`cargo test -p layout` 全過（unit 26、`it` 436），`flex.rs` 原有的 column 測試（寬度、min 和 max content、shrink-to-fit、子項自己的寬度、斷行次數）沒有改預期。把 fork 的捷徑關掉時 `flex::a_column_in_a_row_breaks_its_paragraphs_once` 失敗（斷行次數）。mutation test（`mutants-6a.toml`）：2 條全部抓到（flexbox 不走 `fit_content_width`、shrink-to-fit 不以 min-content 為下限）。`ci.py third-party` 過（`patches/taffy/` 兩個 patch）。 `test.py wpt css/css-flexbox`：1646 個檢查，沒有非預期的結果；`test.py reftest flex/`：16 個，沒有非預期的結果。sanity 的 7 項都過。

**第 6b 段：做 fragment 時的斷行**（2026-11-10，Windows 11）：

- **做之前的分類**（release build，暫時的輸出，量完還原；`tests/sites/` 7 頁和 Speedometer 20 個 suite 各一次 iteration）：做 fragment 時再斷的 343 次（`tests/sites/` 12、Speedometer 331）照原因分：

  | 原因 | 次數 |
  | --- | --- |
  | content box 的寬度從 border box 算回來的浮點誤差（行已經在同一個寬度斷過） | 234 |
  | shrink-to-fit：行在給它的空間斷，盒子最後是 fit-content 的寬度 | 101 |
  | 斷行的結果被清掉（atomic inline 的大小變了、量過 min 和 max content） | 8 |
  | taffy 在最後的 layout 之後又量了別的寬度 | 0 |

- **做法**：`compute_inline` 在 `PerformLayout` 結束時記下斷行的寬度（`InlineContent::keep_laid_out`），`inline_fragments` 用它問 `break_lines`；和 border box 算回來的寬度差超過 1/64 px 時用算回來的。寬度由內容決定的盒子（taffy 沒傳已知的寬度、style 的寬度是 `auto` 或 `fit-content`、沒有 `aspect-ratio`）的行在 fit-content 的寬度斷，其他的在給它的寬度斷。
- **原本確認的「一律在 fit-content 的寬度斷」行不通**：照做時 `cargo test -p layout` 有 8 個測試失敗（行數、list marker 的位置、斷行的次數）。原因是 taffy 的 `compute_leaf_layout` 在 `PerformLayout` 時不把盒子被給的寬度當已知的大小傳給 measure function，而是放進可用的空間；這時空間就是盒子最後的寬度，fit-content 的寬度（夾在 min-content 和 max-content 之間）可能不同。改成只在寬度由內容決定時用 fit-content 的寬度之後，失敗的只剩 2 個、都是斷行次數變少（`a_width_alone_breaks_no_lines` 的 float 加 `text-indent` 從 2 次變 1 次；舊的 `lines_are_broken_again_for_their_fragments` 的例子從再斷 1 次變 0 次），排版沒有變，改了這兩個測試預期的次數。
- **順便修到的**：shrink-to-fit 的盒子裡有百分比寬度的 inline-block 時，原本高度是在空間的寬度量的、行是在盒子的寬度斷的（`a_shrunk_box_is_as_high_as_its_lines_at_its_width` 的 float 原本高 24、行有 2 行，現在高 44）。
- **做完再分類一次發現的兩種**（第一次做完 Speedometer 還剩 21 次）：13 次是 float 的 button（TodoMVC 的「Clear completed」）：button 的 `auto` 寬度在 taffy 的 style 裡是 `fit-content`，不是 `auto`，條件漏了，補上。另外 `layout` 的測試裡看到被給了寬度的盒子裡帶 float 的行先在 fit-content 的寬度排、做 fragment 時再在盒子的寬度斷（原本帶 float 的行不管盒子有沒有被給寬度都用 fit-content 的寬度）：拿掉這個條件，帶 float 的行和其他的一樣。量的頁面裡沒有這種。
- **做之後的計數**（同樣的頁面，各一次）：

  | 頁面 | 斷行（之前 → 之後） | 其中做 fragment 時再斷（之前 → 之後） |
  | --- | --- | --- |
  | `tests/sites/` 7 頁 | 1,633 → 1,591 | 12 → 0 |
  | Speedometer 20 個 suite | 18,611 → 17,738 | 331 → 8 |
  | 　其中 Perf-Dashboard | 986 → 764 | 222 → 0 |

  剩下的 8 次是 Editor-CodeMirror 和 Editor-TipTap 各 4 次，都是記下的寬度和盒子的寬度一樣、斷行的結果被清掉的那一種（不處理）。blink_perf 做之前是 0，沒有重量。「之前」是第 2 段量的；斷行的總數少掉的比再斷的多：這段之外，中間的階段的改動也算在裡面，沒有分開量。
- **新的測試**（`layout` 的 `inline::`）：`lines_are_broken_once_for_their_fragments`（取代 `lines_are_broken_again_for_their_fragments`）、`a_float_aligns_its_lines_in_its_own_width`、`a_shrunk_box_is_as_high_as_its_lines_at_its_width`、`lines_are_broken_again_in_a_box_wider_than_they_were_broken_at`、`lines_with_floats_are_broken_once_in_the_width_their_box_is_given`。
- **mutation test**（`mutants-6b.toml`）：8 條全部抓到：fragment 的寬度從 border box 算、永遠用記下的寬度、不記寬度、只有帶 float 的行用 fit-content 的寬度、被給了寬度的盒子裡帶 float 的行用 fit-content 的寬度、不看 taffy 傳的寬度、不看 style 的寬度、`fit-content` 的寬度不算由內容決定。過程中沒抓到、補了測試的：「永遠用記下的寬度」（補 `min-width` 的測試）、「不看 style 的寬度」（補 float 自己有寬度的例子）。「不看 `aspect-ratio`」抓不到、沒有放進清單：試過 float、inline-block、flex 和 grid 的子項，有比例的盒子的寬度都由父盒子傳進來，走不到這個條件；記在 `docs/test-gaps.md`。
- **跑過的**：`cargo test -p layout`（unit 26、`it` 440）、`-p shell`（70、11、563）全過。`test.py reftest`：`inline/` 17、`text/` 27、`floats/` 11、`flex/` 16、`position/` 11、`tables/` 13、`forms/` 8，沒有非預期的結果。`test.py wpt`：`css/css-sizing` 3679 個檢查、`css/CSS2/floats` 244、`css/CSS2/linebox` 728、`css/css-text` 2751、`css/css-flexbox` 1646，沒有非預期的結果，預期結果都沒有改。`css/CSS2/floats` 的統計是「Ran 244、Expected 243、Unexpected 0」，差的 1 個沒有查是哪一個。
- **階段結束的檢查**（第 6 段）：`cargo test --workspace` 全過；`cargo fmt --check` 第一次有 3 處（新的測試），`cargo fmt` 修掉；clippy 沒有警告；sanity 的 7 項都過。

**第 2 段的量測**（2026-11-10，Windows 11，release build，`RUST_LOG=layout=debug`，scratchpad 裡一次性的 script 加總每頁的 log）：`tests/sites/` 用 `--headless`（1280×800）載入一次；blink_perf 的 30 頁照 `bench.py blink-perf` 的方法（`--automation-pipe`，頁面跑完整個 runner）；Speedometer 3.1 的 20 個 suite 各一次 iteration（`iterationCount=1`，只數計數，沒量時間）。

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
