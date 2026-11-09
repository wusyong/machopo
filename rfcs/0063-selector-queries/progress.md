# RFC 0063 的進度：選擇器查詢

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-20。完成：2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0063` 上做；兩段做完之後一起 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 subtree 的 Bloom filter：元素的 filter 和它的維護、查詢每個 compound 一個 filter、debug build 的交叉檢查 | 完成 | 2026-10-20 |
| 2 每次呼叫和每個元素的成本：快速比對比名稱、只走元素、parse 的快取（第 4、5、7 條；第 6 條量了不做） | 完成 | 2026-10-20 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| filter | 元素的 subtree filter：建立時的屬性和 class、加屬性、改 class、一般的和只接指標的插入加到祖先元素（不跨 shadow root、不到 document）；移除只在剩零或一個子元素時重算（越過文字節點，有自己的名稱），往上到有兄弟元素的為止；刪屬性不重算；名稱的大小寫 | `cargo test -p dom --test it -- tree::subtree_filters_take_names_and_shrink_where_cheap` |
| 走訪 | 只走元素的走訪越過文字和註解 | `cargo test -p dom --test it -- tree::element_traversal_skips_other_nodes` |
| 查詢的結果 | `querySelector`、`querySelectorAll` 找到的和每個元素完整比對的一樣：每種 compound 和 combinator、放寬之後靠完整比對拿掉的、從元素開始查時祖先和前面的兄弟已經有的 compound、不走根的兄弟、不進 template 的內容和 shadow tree、選擇器清單、pseudo-element、quirks mode；快速比對的名稱（小寫的或照寫的）和值的大小寫（`i`、`s`、HTML 不分大小寫的 attribute） | `cargo test -p dom_api --test it -- queries::`；debug build 每次查詢都和從頭走比對（所有經過 `querySelector` 的 debug 測試和 WPT） |
| 工作量 | 名稱少見時只看少數元素：`[data-id]`、`.filters .selected` 只看根到它的路，`[data-x=a] .c` 不走進值不對的元素底下 | `cargo test -p dom_api --test it -- queries::queries_skip_subtrees_without_the_names` |
| parse 的快取 | 每個選擇器只 parse 一次，`querySelector`、`querySelectorAll`、`matches`、`closest` 共用，最多留 256 個，無效的每次都丟例外 | `cargo test -p dom_api --test it -- queries::queries_parse_each_selector_once_while_kept` |
| 行為不變 | parse、插入和查詢的結果不變 | `cargo test -p dom -p style -p html -p dom_api -p dom_js`；`test.py wpt dom/ shadow-dom/ custom-elements/ html/semantics/selectors/ domparsing/ html/dom/` |
| 效能 | 基準頁的 `[data-id]` 0.16 → 0.045 ms、小範圍的呼叫約一半；Speedometer 的 ES6-Webpack −20%（`main` 和這份輪流比） | 基準頁（`details.md`）、`ci.py speedometer --against main`（使用者要求時量，數字在下面和 `details.md` 的「完成時」） |

## 做過的檢查

**完成時（2026-10-20，Fedora 44）**：每段做完時各跑過 `cargo test --workspace`（151、117 秒）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check` 和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`；第 2 段（`e0791acc`）之後只改了文件：`sanity.py docs` 通過。新的 `unsafe` 只在 `dom` 讀 edge 的地方（`add_to_ancestor_filters`、`shrink_filter_after_removal`、`has_element_sibling`、`NodeRef::element_along`），論證寫在 `soundness.md`；filter 是 `#[no_trace]`，GC 的 trace 沒變，`matrix`、`fuzz` 照常由 CI 跑。clippy 抓到一個 `bool::then(…).unwrap_or(…)`（改成 `if`）；`sanity.py boundary` 抓到三個新的 static（`style` 的 `NODES_LOOKED_AT`、`QUERY_PARSES`，測試用的計數器；`dom_api` 的 `PARSED`，parse 的快取，每個 renderer main thread 一份），加進 `STATICS_ALLOWED`，寫明理由。

**實作時的發現**：
- filter 一開始在 `Node::new` 裡算：要先讀 state 再把它搬進去，編譯器多複製了一次，innerHTML 每個節點的 `memmove` 多約 87 個指令；attribute 名稱一開始用 `name_hash`（每個字元一次）。改成在 `create_element_of` 建好節點之後設、名稱用自己的 hash（八個 byte 一次）之後，innerHTML 每個節點的指令 +2.9% → +0.7%。`Node` 的大小沒變（328 bytes，新欄位放進原本的 padding）。
- 快取的測試一開始假設滿了會丟最舊的；`FxHashMap` 丟的是迭代到的第一個（Blink 的 `begin()` 也是），改成檢查留著的數量有上限。
- 第 2 段原本要延後建比對用的 context（第 6 條）：實作前再看取樣，那 11–19% 是內聯在 `with_matcher` 裡的完整比對，不是建 context，不做。
- 基準頁上每項只跑 300 次時，GC 的工作會落在不同的項目裡：第 1 段一開始記成 `[data-id]` 0.012、`.filters .selected` 0.037 ms，重量幾次是 0.045、0.002（兩項加起來差不多；3000 次的取樣也是 0.045）。commit message 和這份都改成重量的數字。

**WPT**：每段做完各跑一次 `dom/`、`shadow-dom/`、`custom-elements/`、`html/semantics/selectors/`、`domparsing/`、`html/dom/`（1441 個測試、127196 個 subtest），都沒有非預期的結果。

**mutation test**（spec 不放 repo，`cargo test -q -p dom -p style -p html -p dom_api -p dom_js`）34 條：
- 第 1 段 27 條，23 條一開始就抓到（filter 漏了 class、漏了屬性、屬性名稱不摺疊大小寫、建立時不設、插入和只接指標的插入不往上、只往上一層、新的屬性和 class 不加、屬性改變不往上、剩兩個子元素也重算、往上越過有兄弟的元素、不越過文字、移除從不重算；`+`、`~` 當成後代、`+`、`~` 前的 compound 要求整層的名稱、class 不快速比對、不看根的祖先、根可以是主體、走根的兄弟、往下時丟掉兄弟、選擇器清單照第一條走、從不走這條路）。沒抓到的 4 條補了測試，重跑都抓到：
  - 重算時漏了元素自己的名稱：測試裡重算的元素都沒有屬性；`list` 加上 `role`。
  - compound 只要求自己的名稱（不併進下面的）、屬性的值不快速比對、子元素不照 filter 跳過：結果一樣，只是多看元素，工作量的測試的門檻太寬（最多 12 個）、沒有「有 `.filters` 但底下沒有 `.selected`」和「值不對的屬性底下有下一個 compound」的子樹。改成精確的個數，加上那兩種子樹。
- 第 2 段 7 條都抓到：tag、attribute 只比小寫的名稱，值一律分大小寫，只走元素的走訪停在任何節點，從不留 parse 好的、留著沒有上限、拿任何留著的回答別的。

**量測**（基準頁：`details.md` 的「基準頁」那一頁和 RFC 0062 的基準頁的清單；輪流各開五次，中位數，ms）：

| 操作 | `main`（`74fc9861`） | 第 1 段 | 第 2 段 | Chrome |
| --- | ---: | ---: | ---: | ---: |
| `querySelector('[data-id="99"]')` | 0.160 | 0.045 | 0.045 | 0.002 |
| `querySelector('.filters .selected')` | 0.121 | 0.002 | 0.002 | 0.013 |
| `querySelectorAll('div').length` | 0.156 | 0.077 | 0.075 | 0.031 |
| `li.querySelector('.toggle')`，100 次 | 0.045 | 0.062 | 0.029 | 0.006 |
| `li.querySelector('input.toggle[type=checkbox]')`，100 次 | 0.087 | 0.115 | 0.042 | 0.022 |
| `li.matches('.completed')`，100 次 | 0.040 | 0.049 | 0.021 | 0.005 |
| `li.closest('ul')`，100 次 | 0.040 | 0.049 | 0.024 | 0.006 |

`main` 和第 1 段是不同的兩次輪流（`main` 對第 2 段、第 1 段對第 2 段，第 2 段兩次都是同樣的數字）。小範圍的呼叫第 1 段比 `main` 慢約 20–40%：每次 parse 多建一份 compound（`Compounds::new`），`matches`、`closest` 也是；第 2 段的快取把 parse 和它一起省掉。`div` 剪不掉，變快是因為只完整比對快速比對過的元素、只走元素。`[data-id]` 每個 `li` 還要約 150 ns（Chrome 約 20 ns），樣本分散在函式的進出和讀 `Node`、state、attribute 的 `Vec`（README 的「之後的可能」）。指令數（innerHTML 每個節點）：4618 → 4651（+0.7%，`own_filter` 約 74、插入和移除時的往上 OR 和重算約 50）；`textContent = ''` 在雜訊範圍內；`DOMParser` 在 ±4% 以內。

**Speedometer**（`details.md` 的「完成時」）：`ci.py speedometer --against main`（`main` 和 `0063` 輪流各三次）：ES6-Webpack 101.9 → 81.9 ms（−20%，三次都低），20 個 suite 的幾何平均 −6.0%，但其他 suite 的變化在雜訊範圍內：jQuery +16%、WebComponents +43% 再單獨輪流各五次，`main` 自己也在 198–281、38–54 ms 之間跳，`0063` 反而低（−23%、−13%），不是這份的效果。另外 kun 和 Chrome 各一次：ES6-Webpack 116.2 → 82.0 ms，和 Chrome 的倍數 3.97 → 2.48。
