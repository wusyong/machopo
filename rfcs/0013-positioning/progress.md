# RFC 0013 的進度：`position`

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-07（第 1 段 relative 和 `z-index` 當天做完，接著整份退回草稿、實作反轉；2026-10-09 重新確認設計、接回第 1 段，再做完第 2 段 absolute 和第 3 段 fixed）

完成日期：2026-10-09（Fedora 44；第 1 段原本在 Windows 11 上做）

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. relative 和 `z-index` | 完成 | 2026-10-07（2026-10-09 重新接上、重新驗收） |
| 2. absolute（連帶 `offsetParent` 和 inset 的 resolved value，第 9 條） | 完成 | 2026-10-09 |
| 3. fixed | 完成 | 2026-10-09 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **style** | |
| `position`（`static`、`relative`、`absolute`、`fixed`；`sticky` 丟掉）、`top` / `right` / `bottom` / `left`、`inset`、CSS-wide keyword；out-of-flow 的盒子 blockify；`z-index` 對有 `position` 的元素生效，有數字時是 stacking context，`auto` 時是 stacked | `cargo test -p style --test cascade position_insets_and_z_index_on_positioned_boxes` |
| 五個 property 的 CSSOM 序列化：`element.style` 讀回關鍵字和保留單位的位移，`getComputedStyle` 讀到關鍵字和絕對化的位移（百分比和 `auto` 留著），`inset` 從 longhand 合回 | `cargo test -p style --test cssom`（`keywords_read_back_in_lowercase`、`lengths_keep_their_units`）、`--test computed position_and_insets` |
| **layout** | |
| block 照 `top` / `left` 等位移，之後的照沒移動排；`left`、`top` 優先；百分比對包含塊，高度不確定時是 `auto`；`static` 的不移動；flex item 和 atomic inline 的位移 | `cargo test -p layout --test position`（`blocks_move_and_what_follows_stays`、`items_and_atomic_inlines_move`） |
| inline 元素在每一行的 inline box 和文字、巢狀的累加，之後的文字不動 | `cargo test -p layout --test position`（`inline_elements_move_their_boxes_and_content`、`an_inline_element_moves_on_every_line_it_is_on`） |
| 絕對定位的盒子由包含塊排：隔幾層也照包含塊的 padding box 定位、百分比和兩邊都給時的拉伸、一邊 `auto` 時從另一邊量、兩邊 `auto` 時用 static position（從 padding box 量）、不在流裡、只有一個 fragment | `cargo test -p layout --test position absolute` |
| fixed 的盒子對 viewport 排版（百分比、`bottom` / `right` 都是 viewport 的），不管祖先有沒有定位；不算進文件的捲動範圍 | `cargo test -p layout --test position fixed` |
| 局部更新：改位移只重排那個 box、旁邊的保留 fragment；改 `z-index` 只重畫；絕對定位的改位移重排它的包含塊；`position` 變了重建盒子樹 | `cargo test -p layout --test update`（`a_relative_offset_moves_only_that_box`、`an_absolute_box_lays_its_containing_block_out_again`） |
| **paint 和 compositor** | |
| 附錄 E 第 8 步：有 `position` 的畫在之後的一般內容之上；`z-index: auto` 的裡面的有 `position` 的和負的 `z-index` 歸給外面；hit test 照同樣的順序 | `cargo test -p paint`（`artifact` 的 `positioned_boxes_paint_after_the_flow_as_if_stacking_contexts`、`hit_test` 的 `positioned_boxes_are_hit_as_painted`、`a_positioned_container_is_over_the_flow_after_it`） |
| 絕對定位的盒子畫在它被拿出來的流之上、照放好的位置被點到 | `cargo test -p paint --test hit_test absolute_boxes`、`cargo test -p shell` 加上 `--test reftest -- position/p2`（`4ff8d6e` 時的指令） |
| fixed 的內容自成一個 layer（裡面有 transform 的也歸那一層），compositor 捲動時留在原地、不用提交也不用重 raster，hit test 照它顯示的位置 | `cargo test -p paint --test layerize fixed`、`--test hit_test fixed`、`cargo test -p compositor a_fixed_layer` |
| **DOM** | |
| `offsetParent` 是最近的有定位的祖先、`offset*` 從它的 padding box 算，`position: fixed` 的元素沒有 `offsetParent`（`offset*` 是 viewport 相對的）；`getComputedStyle` 的 `top` / `right` / `bottom` / `left`（給了值的保留 computed value、`auto` 的是 layout 算出來的、static 的是 `auto`）；fixed 的 client rect 不扣 viewport 的捲動 | `cargo test -p shell --test geometry`（`absolute_boxes_and_their_insets`、`fixed_boxes_have_no_offset_parent`）、`python3 scripts/test.py scenarios position-fixed` |
| **WPT 和 reftest** | |
| `css/CSS2/positioning`、`css/css-position` 等的 208 個 reftest（第 7 條）、`css/CSS2/zindex` 的候選，和自己的兩個 reftest（疊在一起的頭像和通知徽章、下拉選單） | `python3 scripts/test.py reftest`（`position/p1`、`position/p2`、`wpt/css/CSS2/positioning` 等） |
| `css/cssom/getComputedStyle-insets-*` 的 8 個、既有 `css/cssom-view` 和 `check-layout-th.js` 那批照 baseline | `python3 scripts/test.py wpt` |
| **視窗裡** | |
| `about:demo` 的「Bring 3 forward」：頭像 3 蓋到頭像 2 上 | `python3 scripts/test.py scenarios position-relative`、`python3 scripts/test.py automation position-relative` |
| 「Open menu」：`position: absolute` 的下拉選單放進它的容器、蓋在後面的內容上，並報出 `offsetParent` 和 resolved 的位移 | `python3 scripts/test.py scenarios position-absolute`、`automation position-absolute` |
| 「Pin a bar」：bar 釘在右上角，捲動時留在原地，`getBoundingClientRect` 讀到一樣的位置 | `python3 scripts/test.py scenarios position-fixed`、`automation position-fixed` |

## 做過的檢查

### 第 1 段 relative 和 `z-index`（2026-10-07 Windows 11，2026-10-09 在 Fedora 44 重新接上）

- **新的測試**：`crates/layout/tests/position.rs`（4 個）、`update` 1 個、`style` 的 `cascade` 1 個、`paint` 的 `artifact` 1 個和 `hit_test` 2 個；reftest `position/p1-avatars-and-badge.html`；情境 `position-relative.txt`；WPT 的 reftest 208 個（第 7 條）。
- **WPT 過不了的 11 個**（記在 `reftest-expectations.txt`）：3 個是放在有 `position: relative` 的 inline 元素裡的 block 沒跟著位移（已知限制，「之後的可能」）；5 個是 kun 其他既有的限制（空的 inline 元素不產生行、`overflow: clip` 當成 `hidden`、全形空白不掛在行尾）；1 個依賴字型的換行；1 個 taffy 的。
- **mutation test**（23 條）：22 條抓到；6 條是補了測試之後才抓到（`absolute` 當成 `relative`、`static` 的 inline 元素移動、`right` 的方向、glyph、inline 元素裡的 atomic inline、hit test 的背景階段）；1 條等價（hit test 的 line 階段跳過有 `position` 的元素，記進 `docs/testing.md`）。
- **2026-10-09 重新接上**：整份 RFC 2026-10-07 退回草稿、實作反轉（決定重新變成看法）。重新確認 A2 / Z1 / F1 / S1 之後，照第 10 條把那個 commit 接回來：`crates/` 的部分套用乾淨，一個衝突都沒有；編不過的只有三個 exhaustive match，正是 RFC 0016 的 1a 和 2a 刻意加的「加了 property 沒補序列化就失敗」，所以補上五個 property 的 specified 和 computed 序列化（5 條 mutation 全部抓到）。RFC 0016 的測試原本拿 `position` 當「kun 不支援的 property」的例子，改用 `float`。文件和 WPT 清單的 7 個檔案衝突照現在的內容重解。
- **不穩定的測試**：`cargo test --workspace` 剛大量重編後的第一次執行裡，`browser` 的 `a_click_on_the_old_page_doesnt_reach_the_new_one` 等載入等到 30 秒上限而失敗，之後單獨三次、整個 workspace 兩次都通過。原因是幾十個靜態連結 V8 的測試 binary 同時在 link，而那個測試的頁面自己還有 300 ms 的忙等；和這份 RFC 無關（它的測試頁沒有 `position`）。

### 第 2 段 absolute（2026-10-09，Fedora 44）

- **決定**：第 9 到 15 條。核心是第 11 條：taffy 0.14 的絕對定位算法是私有的、一律對父盒子解，所以 box tree 照舊是 DOM 結構，但交給 taffy 的 child 清單把 out-of-flow 盒子移到它的包含塊底下，原本的位置留一個 0×0 的佔位盒子當 static position；包含塊排版分兩趟（先排自己的內容讓佔位盒子就位，再把兩邊都 `auto` 的位移換成它、連 out-of-flow 的一起排）。每次 layout 從 `LayoutView` 走一遍重建這些關係，所以盒子被拿掉、搬家、`position` 變了都不用各留一套帳。
- **連帶補上**（第 9 條）：`BlockBox` 多 `inset`（used 的位移），`getComputedStyle` 的四個位移成為 resolved value；`offsetParent` 改成最近的有定位的祖先。
- **WPT 找到的 bug**（修了再定 baseline）：`position` 改變時父盒子的 in-flow 清單沒更新（改成每次 layout 重算，並把它算進 `StyleDifference::Reattach`）；包含塊自己的邊框在它排版途中還不在 `layout` 裡，static position 因此多算一次邊框；佔位盒子自己也被當成 out-of-flow 掛了一次；flex 和 grid container 自己的 out-of-flow 子盒子要照 css-flexbox 1 § 4.1 對齊（taffy 本來就會做，那種情況不注入 static position）。
- **第 1 段漏跑的檢查**：第 1 段沒有跑 `test.py wpt`（它動到 `style` 和 `layout`，照規則要跑），這一段跑的時候才發現 `css/css-overflow/overflow-empty-child-box.html` 從那時起就壞了：完全沒有面積的盒子照舊撐大捲動範圍。`scrollable_overflow` 改成「沒有面積的盒子自己不算，裡面的東西照算；只有一軸是 0 的照舊算另一軸」。
- **WPT**：匯入 `getComputedStyle-insets-*` 的 7 個（RFC 0016 的第 13 條留給這一份），2 個全部通過；候選腳本放行 `absolute` 後新增 2 個 reftest（1 個通過，1 個因為 window 的具名屬性會等到逾時，記成 `[ Skip ]`）。既有 baseline 31 個變動，14 個整份通過後刪掉。`grid-content-alignment-with-abspos-001.html` 從通過變失敗：它以前是「`position` 被丟掉、盒子留在流裡剛好被置中」才過的，Chrome 靠 css-align 的 self-alignment，kun 還沒有（記進限制和「之後的可能」）。
- **mutation test**（12 條）：10 條抓到；「不重排包含塊」是等價的（包含塊一定是它的祖先，`mark_needs_layout` 本來就會走到，那段程式順手拿掉了）；「給了值的位移改取 layout 的值」只在 `top` 和 `left` 那兩行等價（kun 只有 horizontal-tb、ltr），兩條都記進 `docs/testing.md`。

### 第 3 段 fixed（2026-10-09，Fedora 44）

- **決定**：第 16 到 19 條。G 多談了一輪：kun 的 viewport 捲動不在 paint 的 property tree 裡（P4 的設計，compositor 用 `visible_rect` 直接捲），所以不能照 Chromium 把 transform 接在 scroll 之上。結論是主 frame 的 fixed 內容自成一層、帶「釘在 viewport」的標記，compositor 畫的時候把目前的捲動位移加回原點——tile 是每層自己內容空間的，所以不失效、不重 raster，也不用提交；子 frame 裡的 fixed 接在它本來就有的 scroll node 之上。
- **做了什麼**：`layout` 把 fixed 的盒子掛到它那個 frame 的 `LayoutView`，並且不算進 `scrollable_overflow`；`paint` 的 pre-paint 讓 fixed 的子樹掛在 viewport 捲動之上、`layerize` 讓標過的 chunk 自成一層；`compositor` 的 `LayerProperties` 多一個 `fixed`，`layer_origin_in_pixels` 把捲動位移加回去；hit test 和 `getBoundingClientRect` 照它顯示的位置（第 18 條）。
- **自己踩的**：client rect 第一版改在 `paint::element_bounds`，但 renderer 走的是 `layout::client_rects`（RFC 0018），沒生效，改成問 `layout::is_fixed`；情境在真視窗裡失敗過一次，viewport 的寬度是小數（顯示器的縮放），`right: 0` 的 bar 落在 712.25，`about:demo` 的提示改成四捨五入。
- **WPT**：匯入 `getComputedStyle-insets-fixed.html`，整份失敗、記 baseline：它的包含塊是一個有 `transform` 的祖先（CSS Transforms 說會，Chrome 也是），kun 一律用 viewport（記進限制和「之後的可能」）。候選腳本放行 `fixed` 之後沒有新的 reftest 候選。
- **mutation test**（9 條）：9 條全部抓到。過程中發現 `cargo test -p shell --test scenarios` 不設 `MACHOPO_SLOW_TESTS` 根本不跑情境，所以一開始有三條「被情境漏掉」是假的，改用 `scripts/test.py scenarios` 之後正常；補了三個斷言（捲動範圍的寬度、fixed layer 裡的 chunk 數、捲動後的 client rect）。

### 整理時補的（2026-10-09，Fedora 44）

寫 `docs/limitations.md` 的時候發現第 3 段漏了 CSSOM View `offsetParent` 演算法的第 1 步：`position: fixed` 的元素應該回 `null`（它對 viewport 排版，`offset*` 就是文件原點相對的），kun 照舊回最近的有定位的祖先。補在 `offset_parent_of`，新增 `crates/shell/tests/geometry.rs` 的 `fixed_boxes_have_no_offset_parent`（`offsetParent` 是 `null`、`offsetLeft` / `offsetTop` 是 viewport 座標、resolved 的 `left` 是 `right: 0` 算出來的）；mutation 1 條（拿掉那個判斷），抓到。WPT 的 `css/cssom-view/offsetParent_element_test.html` 的第 1 個子測試（演算法第 1 步）因此通過，baseline 只剩第 2 步（空的 `<span>` 不產生盒子，所以它的 `offsetParent` 是 `null` 而不是 `td`）。

### 完成時的驗收（2026-10-09，Fedora 44）

`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（沒有警告）、`cargo test --workspace` 1549 個全部通過、`sanity.py deps`、`boundary`、`unsafe`、`docs`；`test.py wpt`（1675 個跑、212 個 skip、0 個不符；整理時補的那條之後重跑過）、`reftest`（745 個通過、75 個照預期失敗、1 個 skip、沒有意外的）、`scenarios`（56 個）、`automation`（28/28，真視窗，看過 `position-*` 三份的截圖）。沒有動到 GC，`matrix` 和 `fuzz` 照規則不跑。

### Rebase 到最新的 main 之後重跑（2026-10-09，Fedora 44）

`main` 這期間多了 16 個 commit（其中一條新規則：RFC 的引用只放在 `rfcs/`，所以這份 RFC 在 `crates/`、`docs/`、`tests/`、`resources/`、`scripts/` 留下的引用都改寫成只講行為和理由）。重跑：`cargo fmt --check`、clippy 沒有警告、`cargo test --workspace --no-fail-fast` 1586 個通過（唯一失敗的 `shell` 的 `pages_are_captured_as_png` 在 `main` 上用同一行、同樣的值也失敗，是上游既有的問題）、`sanity.py` 四項、`test.py wpt`（1671 個跑、216 個 skip、0 個不符）、`scenarios`（34 runs）、`automation`。

`reftest` 第一次跑出 3 個「unexpected pass」：`css/cssom-view/` 的 `cssom-getBoundingClientRect-vertical-rl`、`getClientRects-inline`、`offsetTopLeft-inline`——它們量的位置牽涉到繞著 `float` 的絕對定位盒子，有了絕對定位就過了，所以把那三行 expectation 刪掉；重跑是 745 個通過、75 個照預期失敗、1 個 skip、0 個 unexpected。
