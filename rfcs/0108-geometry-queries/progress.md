# RFC 0108 的進度：幾何查詢從元素往上找，不走整棵樹

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-22

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 座標 | 做完，等審核 | 2026-11-22 |
| 2. 元素 | 做完，等審核 | 2026-11-23 |
| 3. 文字 | 還沒開始（先決定「待決定」4） | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 頁面捲過之後，fixed 盒子和它裡面的不動、其他的照捲：每個回 viewport 座標的查詢；根元素 | `cargo test -p layout --test it contain::`（`the_pages_scroll_moves_what_isnt_in_a_fixed_box`、`the_pages_scroll_and_the_root_element`） |
| SVG 的 rect 和 CTM 一樣 | `cargo test -p layout --test it svg::`（`the_pages_scroll_and_svg_geometry`） |
| fixed 到 viewport 的 fragment 只在根的下一層；一個節點（或它的一個 pseudo-element）一個盒子 | `cargo test -p layout --test it`（`Page` 的 `assert_for_geometry`，每次檢查都驗） |
| 從 script 看到的：rect、文字的 `Range`、SVG、`layerY`；`offsetX`、`offsetY` 不變 | `cargo test -p shell --test it geometry::` |
| fixed 盒子裡的 DevTools 盒子模型和 `<select>` 的選單在畫出來的地方 | `cargo test -p shell --test it devtools_overlay::`；`cargo test -p shell --test it browser_view::` |
| 捲過的頁面上點得到 fixed 列裡的按鈕，選單開在它下面；`about:demo` 示範 | `python3 scripts/test.py scenarios fixed-geometry`；`python3 scripts/test.py scenarios position-fixed` |
| 元素的盒子從連結和行內元素的索引找到：經過 transform、sticky、捲動容器，out-of-flow 盒子、外側 marker、flex 的 `order`、float、被 block 切開和換行的行內元素、table；DOM 改過、盒子的位置被重用之後；查詢只走到那個盒子，和兄弟的數量無關 | `cargo test -p layout --test it geometry::` |
| 每個節點從盒子找到的 `element_box`、client rect、border quad、沒有 pseudo 的 box quad、`scroll_size` 和從根走的每個 bit 相同（捲過的捲動容器、頁面和黏住的盒子）；每個 fragment 剛好被一個盒子的連結找到；行內元素的索引和重建的一樣 | `cargo test -p layout --test it`（`Page` 的 `assert_found_from_boxes`，每次檢查都驗） |
| `scrollWidth`、`scrollHeight` 問的時候才算，盒子不帶它 | `cargo test -p paint --test it geometry::`；`cargo test -p dom_api --test it geometry::` |
| 守著這些的測試擋得住弄壞的實作 | `python3 scripts/test.py mutate rfcs/0108-geometry-queries/mutants-1.toml`；`python3 scripts/test.py mutate rfcs/0108-geometry-queries/mutants-2.toml` |

## 做過的檢查

2026-11-22，排程（Windows 11）：branch `0108` 從 35c2e9f 開出來。那個 commit 在 `main` 上的 CI 有一個和這份無關的失敗：`ipc` 的 `process_channel::a_host_and_a_child_talk`。原因還沒查實：沒有重現過，只讀程式推測是測試的迴圈沒有等 child 回答之後才送的 `Released`，有時多開一塊 region。

2026-11-22，座標的量測重做（Windows 11、Chrome 158）：排程前的頁面派發的事件都沒有 `view`，Chrome 的 `layerY` 因此差了一個捲動的距離，RFC 據此寫的「和 Chrome 不同」和成本都錯了。五頁用 `view: window` 重跑：只有那三個 `layerY` 的值不同，`offsetX`、`offsetY` 和 rect 都沒變；`layerY` 再用真的輸入確認過。

2026-11-22，已決定的事項重查（Windows 11、Chrome 158、machopo a2f1722 的 `perf` binary；Blink 讀 main 的原始碼，WebKit 和 Gecko 只讀原始碼、沒有實跑）：

- 找到「已決定的事項」4 不完整、6 的理由和寫法有錯、「第 2、3 階段答案不變」不一定成立、取整沒有問過就列成範圍外；都已改進 RFC，要選的問過之後記成「已決定的事項」7 到 10。
- 重查時一度算成座標錯的兩項其實是沒實作：`will-change` 的 layer（沒有這個 property）和 `<video>`（沒有實作）。
- 捲動錨定在 box tree 更新和 layout 之間讀舊的 fragment：讀程式確認它不經過 `geometry` 的查詢，A 的前提成立（README 的「一致性」）。
- 沒有改程式，沒有跑 cargo test 和 WPT；「已決定的事項」7 各選項對幾頁是推算的。

2026-11-22，用隨機頁面反證決定（Chrome 158；一次性的腳本，沒有進 repo；驗的是規則的模型，不是 machopo）：

- 座標（1、4、5）8000 頁、三種都相符；故意弄壞模型三次都抓到。layer（4、9）37773 個目標都相符（不含匿名 table）。
- 第一版的 7（全部用 B1，根據是手挑的 19 頁）錯了：改成四種情況，數字在 `details.md` 的「區塊的四種情況」。
- 真的滑鼠輸入 30 個案例：空的 inline 元素（`::after` 蓋滿的連結）打得到；測到的 text node 和 `<br>` 都打不到。第一版的 8（五種都修）因此改成只修點得到的和 document。
- 8 的 SVG 規則寫進去之後又反證一次：照元素名稱排除六種是錯的（隨機的樹上把現在對的弄錯 97.5%），改成看有沒有排版節點，數字在 `details.md` 的「SVG 的邊界」。順帶找到 `<switch>` 選錯 child 和巢狀的 `<a>`，是 SVG 排版的錯，不在這份。

2026-11-22，縮小範圍（「已決定的事項」11；Windows 11、Chrome 158、machopo a2f1722 的 `perf` binary）：

- 上面提到的「已決定的事項」4 到 10 和它們的量測移到 `details.md` 的「移出範圍的滑鼠事件座標」，編號不變。
- 兩個核心問題重量過都還在；根那一步在隨機頁面上反證過，沒有需要特例的情況：數字在 `details.md` 的「縮小範圍時的重量」。
- 讀過被改到的查詢所有的呼叫者：都要 viewport 座標，沒有靠現在的錯補償的。`<select>` 的選單和 DevTools 因此一起對（還沒跑）。
- 只改文件，沒有寫程式，沒有跑 cargo test 和 WPT。

2026-11-22，第 1 階段（Windows 11、Chrome 158；和改之前的 `perf` binary 比；隨機頁面的腳本是一次性的，沒有進 repo）：

- 先紅：三個新的 shell 測試和情境在改之前的程式上失敗，都差一個捲動的距離（選單在 −376，應該是 124）。
- `cargo test --workspace` 4606 個通過、0 失敗、2 個原本就忽略；fmt、clippy、七個 sanity 通過。
- WPT（cssom-view、layer-coords-transform、dom/ranges、css-position、css-scroll-anchoring，SVG 的 geometry、types、text）48811 個檢查，沒有非預期的結果，1 個原本標著的 known intermittent；預期結果沒改。
- mutation test：第一次 38 條，沒抓到 3 條。1 條補了測試（`box_of` 忘了盒子的位置）；2 條等價（找到盒子就停），記進 `docs/test-gaps.md`、從 spec 拿掉。最後 36 條全部抓到，檔案都還原。
- 根那一步：fixed 的盒子 3876 個對 3858 個、裡面的 SVG 圖形 364 個對 362 個，和寫程式之前照規則算的一樣。
- 12 組隨機頁面，捲 (70, 500) 前後各移動多少，和 Chrome 比（每軸一個值）：

  | 查詢 | 改之前對的 | 改之後對的 | 全部 |
  | --- | --- | --- | --- |
  | 盒子 | 23950 | 31322 | 31452 |
  | 第一個 client rect | 23950 | 31322 | 31452 |
  | 文字 | 9633 | 12959 | 13054 |
  | CTM | 3280 | 4648 | 4656 |

- 同樣的頁面和改之前比：沒捲時 16318 個元素每個 bit 相同；捲過時 30574 個值相同，9733 個剛好差捲動的距離，沒有別種差別。
- **原本和 Chrome 一樣、現在不一樣的有 14 個值**：都在同一個 fixed 盒子裡，它在有 `filter` 的 inline `<span>` 底下。最小的頁面上，Chrome 把這種 fixed 盒子跟著頁面捲，machopo 的 layout 把它釘在 viewport（畫和 hit test 都是）。新的 rect 和畫出來的一致；改之前裡面的元素多扣一次捲動，移動的距離才碰巧和 Chrome 一樣。這是 layout 的錯：記進 `docs/limitations.md`，沒有修，也沒有加特例。
- 另外 24 組、inline 元素沒有 `filter` 的頁面：沒有原本對、現在錯的；盒子對的從 47092 變 62090（共 62240），沒捲時 32323 個元素和改之前相同。
- 剩下 363 個和 Chrome 不同的值都追到源頭的盒子，它在改之前就不對：
  - 338 個是非 atomic 的 inline sticky 不會黏（原有的限制）。
  - 16 個是上面的 inline `filter`，含那 14 個。
  - 9 個來自同一個 sticky 的 flex item：沒捲時排版就差 8px（同一列 flex item 的寬度和 Chrome 不同），捲過之後兩邊都黏在 0.64px 上下，所以只有移動的距離不同。排版為什麼不同沒有再查。
- 流程上的錯：`test.py mutate` 還原原始碼，但 `target/debug/machopo.exe` 還是最後一條 mutation 編出來的，拿它分類時多出幾千個假的不同。重編之後整套比對重跑，數字是上面這些。
- 已完成的 RFC 0091 的 `mutants-2a.toml` 有一條的原文是拿掉的 `viewport_origin_of`，那份 spec 因此不能照原樣重跑；同一件事現在由這份的 `mutants-1.toml` 守著，舊的照規定不改。
- 沒驗的：macOS、Linux、真的視窗（沒動視窗那一層）、真實網站。這一段不讓查詢變快：走訪照舊是整棵樹。

2026-11-23，第 2 階段（Windows 11、Chrome 158）：

- `0108` 先 rebase 到 `main` 的 d2105e5（含 cc6479f 的盒子樹修正：原型上不同的 10 組都出在它）。
- 每次 `Page` 的檢查從盒子找每個節點的 `element_box`、client rect、border quad、沒有 pseudo 的 box quad 和 `scroll_size`，和從根走的每個 bit 比（每個捲動容器、頁面都捲過，黏住的盒子照它們的位移）；每個 fragment 剛好被一個盒子的連結找到；行內元素的索引和重建的一樣。layout 的 `geometry::` 另外四個測試（「驗收」）。
- `cargo test -p layout -p paint -p dom_api -p renderer` 和 shell 的 `geometry::`、`devtools_overlay::`、`browser_view::` 通過（layout 32 + 481、paint 9 + 278、dom_api 26 + 201、renderer 128、shell 38、11、45）。
- WPT（cssom-view、`uievents/mouse/layer-coords-transform.html`、dom/ranges）47007 個檢查，沒有非預期的結果，1 個原本標著的 known intermittent（`scroll-behavior-smooth-navigation.html`）；預期結果沒改。
- mutation test：第一次 16 條，沒抓到 3 條，都改了程式或記下來，沒有補測試：
  - text 的盒子也放進要走的路徑：這一段的查詢從它拿不到東西（text 節點沒有行內盒子，`<br>` 不在行的盒子裡），從 `paths` 拿掉；之後 `text_rects` 才用 `texts`。
  - 路徑去掉重複：一個元素不是有自己的盒子、就是在某個 block 的行內內容裡，兩種索引不會都有它，拿掉。因此「行內元素的索引不收 pseudo-element 的」變成要守的（block 有行內的 `::before` 時它自己的盒子會走兩次），加了一條。
  - 連結比對 fragment 的指標、往上爬的步數有上限：盒子一致時都不會成立，是防呆，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」，從 spec 拿掉。
  - 另外加 `is_user_scrollable` 的 `>=`（剛好填滿的不算能捲）。最後 16 條全部抓到，檔案都還原。
- 量測：`target/perf` 第一次編，V8 照原型的做法連 `target/debug` 解好的同一份 release 的 V8（`RUSTY_V8_ARCHIVE`）。每個元素讀一次，三次的中位數，ms；「從根走」是原型的 binary 切到原本的走訪（它的基底是 a2f1722，不是第 1 階段的 commit）：

  | 頁面 | 查詢 | 第 2 階段 | 從根走 | Chrome |
  | --- | --- | --- | --- | --- |
  | 1000 列 | `offsetHeight` | 0.19 | | 0.20 |
  | 10000 列 | `offsetHeight` | 2.16 | 428 | 2.12 |
  | 10000 列 | `getBoundingClientRect` | 3.8 | 1570 | 3.9 |
  | 10000 列 | 前 1000 列的 `offsetTop` | 0.33 | 120 | 0.41 |
  | 10000 列 | `body.offsetWidth` 1000 次 | 0.20 | 124 | 0.21 |
  | 30000 列 | `offsetHeight` | 7.15 | | 6.4 |
  | 30000 列 | `getBoundingClientRect` | 14.1 | | 12.9 |
  | 一段 1000 個 `<span>` | `offsetHeight` | 0.68 | 0.62 | 0.17 |
  | 一段 10000 個 `<span>` | `offsetHeight` | 40 | 41.5 | 2.13 |
  | 一段 10000 個 `<span>` | `getBoundingClientRect` | 35.7 | 31.9 | 4.36 |

  - 列數變三倍，`offsetHeight` 和 rect 慢 3.3 和 3.7 倍（Chrome 3.0 和 3.3）：不隨列數的平方變慢，「什麼時候停」不成立。
  - 一段裡的 `<span>` 變十倍慢 59 倍：「待決定」3 的 I1 是 O(M²)，從根走時也一樣；這一段沒有讓它變慢。數字也寫進 README 的「待決定」3。
  - 「改一點、讀一次」10000 列 15.8 ms（Chrome 4.95），比原型的 10.2 慢；和原型交替各跑 5 次是 9.65–16.2 和 9.55–13.5，是雜訊，不是索引的維護。和 Chrome 的差在 layout 本身（「之後的可能」1 到 3）。一段 10000 個 `<span>` 時是 815 ms（從根走 796、Chrome 23.5），同樣是 layout。
- 已完成的 RFC 的 spec 有兩條因為 `BlockBox` 拿掉了 `scroll_size` 不能照原樣重跑：0036 的 `mutants-1b.toml` 的「an inline element has its box」（換上去的程式建一個有 `scroll_size` 的 `BlockBox`，編不過）、0091 的 `mutants-2a.toml` 的「a box that just fits scrolls」（原文不在了）。後者現在由這份的 `mutants-2.toml` 守著；前者守的是 resize observer 分辨行內元素，和 `scroll_size` 無關，換上去的程式拿掉那個欄位就能跑。舊的照規定不改。
- `cargo test --workspace` 4673 個通過、0 失敗、2 個原本就忽略；fmt（排了五個改到的檔案之後）、clippy（paint 的測試多一個型別別名之後）、七個 sanity 通過。
- 沒驗的：行內元素的索引的記憶體和維護的時間沒有單獨量（「缺點」）；macOS、Linux、真的視窗（沒動視窗那一層）、真實網站。
