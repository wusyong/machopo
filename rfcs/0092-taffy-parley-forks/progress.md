# RFC 0092 的進度：taffy 和 parley 的 fork：跟著上游、修改留成 patch、減少 workaround 的成本

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-10，Windows 11。在 `0092` 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 taffy fork 的基礎設施：fork、path 依賴、patch 的匯出和檢查、parley 現在的 patch、文件 | 完成 | 2026-11-10 |
| 2 計數 | 完成 | 2026-11-10 |
| 3a 重寫 parley 的 fork：合成分組、rebase 到上游的 main、machopo 配合 | 做完，等審核：fork 的改寫只在本地（`11771f1c`，舊的頂端有本地的 tag `machopo-2026-11-10`），`third_party.toml` 釘本地的 commit；審核後才 push tag 和 `machopo` 分支。階段最後的 `cargo test --workspace`、fmt、clippy 等 3b 做完一起跑 | |
| 3b parley 的 fork 自己的測試、上游的草稿、拿掉 `text-indent` 的 workaround | 還沒開始 | |
| 4 taffy 跟上上游的 main | 還沒開始 | |
| 5 字型家族的清單和名稱解析 | 還沒開始 | |
| 6 float 和 taffy 的快取（6a）、做 fragment 時的斷行（6b） | 還沒開始 | |
| 7 上游的草稿 | 還沒開始 | |

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
| `LayoutUpdate::lines_broken_for_fragments`：做 fragment 時在最後的寬度真的又斷了一次 | `inline::lines_are_broken_again_for_their_fragments`：確定的寬度、放得下的 shrink-to-fit 的 float 是 0；在沒有空間的地方 shrink-to-fit 的 float（行在 0 斷、盒子和內容一樣寬）是 1 |
| `BoxTreeUpdate::font_family_names`：shape 的 inline formatting context 給 parley 的字型家族名稱 | `font_selection::each_span_gives_parley_its_family_names`：block 的清單、inline 元素的、`display: contents` 的元素裡的 run 的，各加幾個名稱就多幾個；在 span 自己的 style 裡的文字不加；匿名 block 加兩次；沒有 shape 時 0。`update::` 的測試比較 `BoxTreeUpdate` 時不比這個欄位 |
| 計數都在 release build 裡 | `RUST_LOG=layout=debug` 的 release build 印出 `layout`、`build box tree`、`update box tree` 三行的欄位（量測用的就是這些） |
| parley 的 fork 是上游的 main（`1b67b753`，2026-11-08）加上 10 個各做一件事的 commit 和 `README.machopo.md` 的 commit，每個 commit 帶 fork 自己的測試 | fork 的 `cargo test -p parley -p parley_engine -p parley_tests -p fontique`、clippy、`cargo fmt --all -- --check`；`python3 scripts/ci.py patches parley` 匯出 11 個 patch，`ci.py third-party` 確認它們套在 `upstream_rev` 上和 `rev` 一樣 |
| machopo 配合上游的 API，除了 atomic inline 的 bidi 和斷行，排版不變 | `cargo test -p layout`、`-p shell`；`test.py reftest` 的 `text/`、`inline/`、`floats/`、`generated/`、`canvas/`、`svg/`、`block/`、`position/`、`flex/`、`grid/`、`tables/`、`hidpi/`；`test.py wpt` 的 `css/css-text`、`css/css-text-decor`、`css/CSS2/linebox`、`css/CSS2/normal-flow`、`css/css-overflow/line-clamp`、`css/css-sizing`、`css/css-display`、`css/css-pseudo`；差異逐項記在「做過的檢查」 |
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
