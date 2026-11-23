# RFC 0108 的進度：幾何查詢從元素往上找，不走整棵樹

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-22。完成日期：2026-11-23（Windows 11）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 座標 | 完成 | 2026-11-22 |
| 2. 元素（連同行內元素在第幾行） | 完成 | 2026-11-23 |
| 3. 文字和其他的走訪 | 完成 | 2026-11-23 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 座標 | 頁面捲過之後，fixed 盒子和它裡面的不動、其他的照捲：每個回 viewport 座標的查詢；根元素 | `cargo test -p layout --test it contain::`（`the_pages_scroll_moves_what_isnt_in_a_fixed_box`、`the_pages_scroll_and_the_root_element`） |
| 座標 | SVG 的 rect 和 CTM 一樣 | `cargo test -p layout --test it svg::`（`the_pages_scroll_and_svg_geometry`） |
| 座標 | 從 script 看到的：rect、文字的 `Range`、SVG、`layerY`；`offsetX`、`offsetY` 不變 | `cargo test -p shell --test it geometry::` |
| 座標 | fixed 盒子裡的 DevTools 盒子模型和 `<select>` 的選單在畫出來的地方 | `cargo test -p shell --test it devtools_overlay::`；`cargo test -p shell --test it browser_view::` |
| 座標 | 捲過的頁面上點得到 fixed 列裡的按鈕，選單開在它下面；`about:demo` 示範 | `python3 scripts/test.py scenarios fixed-geometry`；`python3 scripts/test.py scenarios position-fixed` |
| 座標 | fixed 到 viewport 的 fragment 只在根的下一層；一個節點（或它的一個 pseudo-element）一個盒子 | `cargo test -p layout --test it`（`Page` 的 `assert_for_geometry`，每次檢查都驗） |
| 元素 | 元素的盒子從連結和行內元素的索引找到：經過 transform、sticky、捲動容器，out-of-flow 盒子、外側 marker、flex 的 `order`、float、被 block 切開和換行的行內元素、table；DOM 改過、盒子的位置被重用之後；查詢只走到那個盒子，和兄弟的數量無關 | `cargo test -p layout --test it geometry::` |
| 元素 | 每個節點從盒子找到的 `element_box`、client rect、border quad、沒有 pseudo 的 box quad、`scroll_size` 和從根走的每個 bit 相同（捲過的捲動容器、頁面和黏住的盒子）；每個 fragment 剛好被一個盒子的連結找到；行內元素的索引和重建的一樣 | `cargo test -p layout --test it`（`Page` 的 `assert_found_from_boxes`，每次檢查都驗） |
| 元素 | `scrollWidth`、`scrollHeight` 問的時候才算，盒子不帶它 | `cargo test -p paint --test it geometry::`；`cargo test -p dom_api --test it geometry::` |
| 行內 | 一段裡的行內元素的查詢只看它在的那幾行：它前後的元素變十倍時看的行數不變，跨好幾行的元素每一行看一次；文字的 rect 也是 | `cargo test -p layout --test it geometry::`（`a_query_looks_only_at_the_elements_lines`） |
| 文字和 SVG | 文字的 rect、SVG 元素的四種查詢只走到盒子（外層的 `<svg>` 的），和兄弟的數量無關 | `cargo test -p layout --test it geometry::`（`a_query_goes_down_only_to_the_box`） |
| 文字和 SVG | 每個 text node 從盒子和它的行找到的 rect（整段、開頭和結尾折疊的、第一個 run 的）和當錨點的位移、每個 SVG 元素的 `svg_geometry`、`svg_node`、`svg_shape_path`、`svg_text_chars`，和從根走的每個 bit 相同（捲過的捲動容器和頁面）；DOM 改過之後的文字 | `cargo test -p layout --test it`（`Page` 的檢查；文字在從頭建的樹上比，改過的樹上的在 `geometry::` 的 `boxes_are_found_after_changes`） |
| 其他的走訪 | 每個元素的 `fragment_of` 和當錨點的位移、捲動容器的順序和從根走的相同；游標、child frame 的大小從盒子找；巢狀的捲動容器由外往內錨定 | `cargo test -p layout --test it`（`Page` 的檢查）；`cargo test -p shell --test it`（`scroll_anchoring::nested_scrollers_adjust_outside_in`；游標和 iframe 由現有的測試守著，見「測試和 mutation test」） |
| 全部 | 守著這些的測試擋得住弄壞的實作 | `python3 scripts/test.py mutate rfcs/0108-geometry-queries/mutants-1.toml`；`python3 scripts/test.py mutate rfcs/0108-geometry-queries/mutants-2.toml`；`python3 scripts/test.py mutate rfcs/0108-geometry-queries/mutants-3.toml` |

## 做過的檢查

Windows 11、Chrome 158，2026-11-22 到 2026-11-23。branch `0108` 從 35c2e9f 開出來，第 2 階段之前 rebase 到 `main` 的 d2105e5（含 cc6479f 的盒子樹修正：原型上答案不同的 10 組都出在它），做完時 rebase 到 87fc238。

### 排程時的檢查

- 座標的量測重做：排程前的頁面派發的事件都沒有 `view`，Chrome 的 `layerY` 因此差了一個捲動的距離，RFC 據此寫的「和 Chrome 不同」和成本都錯了。五頁用 `view: window` 重跑：只有那三個 `layerY` 的值不同，`offsetX`、`offsetY` 和 rect 都沒變；`layerY` 再用真的輸入確認過。
- 已決定的事項重查（machopo a2f1722 的 `perf` binary；Blink 讀 main 的原始碼，WebKit 和 Gecko 只讀原始碼、沒有實跑）：滑鼠事件那幾條有不完整和寫錯的，改過之後記成「已決定的事項」7 到 10，後來由 11 移出範圍。重查時一度算成座標錯的兩項其實是沒實作：`will-change` 的 layer（沒有這個 property）和 `<video>`。
- 用隨機頁面反證規則的模型（一次性的腳本，沒有進 repo；驗的是模型，不是 machopo）：座標 8000 頁、layer 37773 個目標（不含匿名 table）都和 Chrome 相符，故意弄壞模型三次都抓到；真的滑鼠輸入 30 個案例。第一版的 7、8 和 SVG 的規則都被反證推翻過，數字在 `details.md` 的「區塊的四種情況」和「SVG 的邊界」。
- 縮小範圍（「已決定的事項」11）：兩個核心問題重量過都還在，根那一步沒有需要特例的情況（`details.md` 的「縮小範圍時的重量」）；讀過被改到的查詢所有的呼叫者，都要 viewport 座標，沒有靠原本的錯補償的。
- 捲動錨定在 box tree 更新和 layout 之間讀舊的 fragment：讀程式確認它不經過 `geometry` 的查詢（README 的「一致性」）。

### 測試和 mutation test

- 先紅：第 1 階段三個新的 shell 測試和情境在改之前的程式上失敗，都差一個捲動的距離（選單在 −376，應該是 124）。
- 答案不變的兩個階段靠 `Page` 的檢查拿從盒子找的和從根走的逐 bit 比（README 的「測試」），現有的 layout 和 update 的測試頁都成為案例；只差在快慢的用計數守著（`Geometry::entered_for_testing`、`lines_for_testing`）。文字的 rect 會算出 run 的 cluster，一開始在受測的樹上比，`caret::` 的兩個測試看到它算過而失敗，所以文字改在同一次檢查從頭建的樹上比，改過的樹上的在 `geometry::` 改 DOM 的測試最後比一次。
- mutation test（spec 是這個資料夾的 `mutants-*.toml`）最後 74 條全部抓到，每次跑完檔案都還原：
  - `mutants-1.toml`：第一次 38 條，沒抓到 3 條。1 條補了測試（`box_of` 忘了盒子的位置）；2 條等價（找到盒子就停），記進 `docs/test-gaps.md`、從 spec 拿掉。最後 36 條。
  - `mutants-2.toml`：第一次 16 條，沒抓到 3 條，都改了程式或記下來：text 的盒子也放進要走的路徑（那時的查詢從它拿不到東西，拿掉）；路徑去掉重複（一個元素不會兩種索引都有它，拿掉；因此「行內元素的索引不收 pseudo-element 的」變成要守的，加了一條）；連結比對 fragment 的指標、往上爬的步數有上限（盒子一致時都不成立的防呆，記進 `docs/test-gaps.md`、從 spec 拿掉）。另外加 `is_user_scrollable` 的 `>=`。行內元素在第幾行加 5 條（查詢看每一行、不建索引、沒排序、最後一行當成第一行、少看最後一行）。最後 21 條。
  - `mutants-3.toml`：第一次 18 條，沒抓到 2 條：`own_path` 濾掉空的路徑（空的只有 `LayoutView` 的盒子，沒有元素的盒子是它，拿掉那一行和那一條）；有錨點的容器的順序反過來（只有一個容器時反不反都一樣，viewport 一律最先：加了巢狀兩層容器的 shell 測試 `nested_scrollers_adjust_outside_in`，三個捲動的都移 30，和 Chrome 158 一樣，反過來時外層的不動）。最後 17 條。
- debug build 的「盒子樹的最後一次 layout」的 assertion 原本只在 `BoxTree::geometry`，也放進 `own_path`：`fragment_of`、找捲動容器和容器的順序不經過 `Geometry`，拿別的 layout 來問時也先被抓到。

### 根那一步和 Chrome 比

- fixed 的盒子 3876 個對 3858 個、裡面的 SVG 圖形 364 個對 362 個，和寫程式之前照規則算的一樣。
- 12 組隨機頁面，捲 (70, 500) 前後各移動多少，和 Chrome 比（每軸一個值）：

  | 查詢 | 改之前對的 | 改之後對的 | 全部 |
  | --- | --- | --- | --- |
  | 盒子 | 23950 | 31322 | 31452 |
  | 第一個 client rect | 23950 | 31322 | 31452 |
  | 文字 | 9633 | 12959 | 13054 |
  | CTM | 3280 | 4648 | 4656 |

- 同樣的頁面和改之前比：沒捲時 16318 個元素每個 bit 相同；捲過時 30574 個值相同，9733 個剛好差捲動的距離，沒有別種差別。另外 24 組、inline 元素沒有 `filter` 的頁面：沒有原本對、現在錯的；盒子對的從 47092 變 62090（共 62240）。
- 剩下 363 個和 Chrome 不同的值都追到源頭的盒子，它在改之前就不對（下一節）。

### 找到的 bug 和限制

- **原本和 Chrome 一樣、現在不一樣的 14 個值**：都在同一個 fixed 盒子裡，它在有 `filter` 的 inline `<span>` 底下。Chrome 把這種 fixed 盒子跟著頁面捲，machopo 的 layout 把它釘在 viewport（畫和 hit test 都是）；改之前裡面的元素多扣一次捲動，移動的距離才碰巧和 Chrome 一樣。這是 layout 的錯：記進 `docs/limitations.md`，沒有修，也沒有加特例。連同它，這種盒子共 16 個值。
- 338 個值是非 atomic 的 inline sticky 不會黏（原有的限制）。
- 9 個值來自同一個 sticky 的 flex item：沒捲時排版就差 8px（同一列 flex item 的寬度和 Chrome 不同），排版為什麼不同沒有再查。
- 反證規則時順帶找到 SVG 排版的錯：`<switch>` 選錯 child、巢狀的 `<a>`，不在這份。
- 開 branch 的 35c2e9f 在 `main` 上的 CI 有一個無關的失敗：`ipc` 的 `process_channel::a_host_and_a_child_talk`。沒有重現過，只讀程式推測是測試的迴圈沒有等 child 回答之後才送的 `Released`，有時多開一塊 region。
- 流程上的錯：`test.py mutate` 還原原始碼，但 `target/debug/machopo.exe` 還是最後一條 mutation 編出來的，拿它分類時多出幾千個假的不同。重編之後整套比對重跑，上面是重跑的數字。
- 已完成的 RFC 的 spec 有三條不能照原樣重跑，照規定不改：0091 的 `mutants-2a.toml` 的兩條（拿掉的 `viewport_origin_of`、「a box that just fits scrolls」的原文都不在了，現在由這份的 `mutants-1.toml`、`mutants-2.toml` 守著）；0036 的 `mutants-1b.toml` 的「an inline element has its box」（換上去的程式建一個有 `scroll_size` 的 `BlockBox`，編不過；它守的是 resize observer 分辨行內元素，拿掉那個欄位就能跑）。

### 量測

`target/perf`，V8 連 `target/debug` 解好的同一份 release 的 V8（`RUSTY_V8_ARCHIVE`）。每個元素讀一次，三次的中位數，ms。「改之前」是那一段開始前的 binary：第 2 階段是原型的 binary 切到原本的走訪（基底 a2f1722），行內元素在第幾行是第 2 階段的，第 3 階段是第 2 階段加行內元素在第幾行的。

| 頁面 | 查詢 | 改之前 | 做完 | Chrome |
| --- | --- | --- | --- | --- |
| 1000 列 | `offsetHeight` | | 0.19 | 0.20 |
| 10000 列 | `offsetHeight` | 428 | 2.16 | 2.12 |
| 10000 列 | `getBoundingClientRect` | 1570 | 3.8 | 3.9 |
| 10000 列 | 前 1000 列的 `offsetTop` | 120 | 0.33 | 0.41 |
| 10000 列 | `body.offsetWidth` 1000 次 | 124 | 0.20 | 0.21 |
| 30000 列 | `offsetHeight` | | 7.15 | 6.4 |
| 30000 列 | `getBoundingClientRect` | | 14.1 | 12.9 |
| 一段 1000 個 `<span>` | `offsetHeight` | 0.68 | 0.26 | 0.17 |
| 一段 10000 個 `<span>` | `offsetHeight` | 40 | 2.89 | 2.19 |
| 一段 10000 個 `<span>` | `getBoundingClientRect` | 35.7 | 4.41 | 4.35 |
| 一段 10000 個 `<span>` | 前 1000 個的 `offsetTop` | 9.62 | 0.53 | 0.42 |
| 10000 列 | 每列的文字的 `Range` 的 rect | 1990 | 5.15 | 6.23 |
| 一段 10000 個 `<span>` | 每個的文字的 `Range` 的 rect | 74.9 | 4.58 | 6.27 |
| 10000 列之後的 `<svg>` | 一個 shape 的 `getBBox` 1000 次 | 116 | 0.40 | 0.28 |
| 一個 `<svg>` 裡 10000 個 shape | 每個讀一次 `getBBox` | 98.8 | 19.6 | 2.91 |
| 10000 列之後的文字欄位 | `focus()` 100 次 | 288 | 226 | 2.78 |

- 列數變三倍，`offsetHeight` 和 rect 慢 3.3 和 3.7 倍（Chrome 3.0 和 3.3）：不隨列數的平方變慢，「什麼時候停」不成立。一段裡的 `<span>` 變十倍：第 2 階段時慢 59 倍（從根走也一樣），記下每個在第幾行之後慢 11 倍（Chrome 13 倍）。
- 第 3 階段做完時重量第 2 階段的頁面：10000 列的 `offsetHeight` 2.21、rect 3.96，一段 10000 個 `<span>` 的 3.08、4.46，在雜訊裡。
- SVG 裡面 10000 個兄弟仍是平方（README 的「之後的可能」4）。`focus()` 剩下的 226 裡 167 是 `scrollIntoView` 自己的走訪（`preventScroll` 時 59.4，Chrome 2.36）；不捲的 59.4 也隨列數線性，是哪一段沒查（「之後的可能」5、8）。
- 「改一點、讀一次」和 Chrome 的差在 layout 本身（「之後的可能」1 到 3），看不出索引和表的成本：10000 列 15.8（Chrome 4.95），和原型交替各跑 5 次是 9.65–16.2 和 9.55–13.5；一段 10000 個 `<span>` 是 815（從根走 796、Chrome 23.5），加上每個在第幾行之後和改之前交替各跑 5 次是 835–851 和 832–855，第 3 階段做完時 1100，和改之前的 binary 交替是 1117–1196 和 848–1166，是機器的狀態；10000 列後面有 iframe 時 3.6 對 3.34，捲到底、每次都錨定時 4.15 對 3.28（Chrome 1.46、1.52），沒有 iframe、不捲的同一頁也是 4.08 對 2.79。索引和表的記憶體、建表的時間沒有單獨量。

### WPT

每個階段跑 cssom-view、`uievents/mouse/layer-coords-transform.html`、dom/ranges；第 1 階段加 css-position、css-scroll-anchoring 和 SVG 的 geometry、types、text（48811 個檢查），第 2 階段和行內元素在第幾行沒有加（47007 個），第 3 階段加 css-scroll-anchoring 和 SVG 的 geometry、types、text、path（47847 個）。都沒有非預期的結果，1 個原本標著的 known intermittent（`scroll-behavior-smooth-navigation.html`）；預期結果沒改。

### 不穩定的測試

- 第 3 階段單獨跑「順序反過來」那一條 mutation 時，shell 的整套失敗過一次（那時的測試頁只有一個容器，這條是等價的）。`test.py mutate` 不留測試的輸出，不知道是哪一個；沒改的程式上整套連跑 8 次、套著這條跑 2 次，都沒再出現。
- 最後一次跑 `mutants-3.toml` 時，沒改的程式（baseline）上 `automation_pipe::the_window_has_bounds_to_set` 等 30 秒逾時：已知的不穩定（很多 `machopo` 同時啟動時有的拿不到 GPU、退回 WARP，RFC 0004 的 `progress.md`，還沒修），和這份無關；上面那一次多半也是它，沒有證實。

### 跑過的檢查

- 每個階段做完時：`cargo test --workspace`（第 1 階段 4606 個、第 2 階段 4673 個、行內元素在第幾行 4674 個、第 3 階段 4675 個通過，0 失敗，2 個原本就忽略）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`，`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。fmt 排過改到的檔案（第 3 階段 spec 的兩條原文跟著換行），clippy 讓 paint 的測試多一個型別別名。
- 做完、rebase 到 87fc238 之後：`cargo test -p layout -p paint -p dom_api -p renderer`、shell 的整合測試（644 個）、`dom_js` 的 `cross_origin`，`cargo fmt --check`、`sanity.py docs` 通過。
- 沒有動到 GC、`unsafe` 和視窗那一層：`matrix`、`fuzz` 由每週的 CI 跑，沒跑 `scenarios --window`。
- 沒驗的：macOS、Linux、真的視窗、真實網站；`fragment_of`、錨定的位移和容器的順序只比答案，沒有計數（`docs/test-gaps.md`）。
