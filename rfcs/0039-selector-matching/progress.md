# RFC 0039 的進度：style 的規則比對

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-12。完成：2026-10-13，Fedora 44（Linux x86_64，AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0039` 上分兩段做，第 1 段做完先 merge 回 `main`；第 2 段等 RFC 0016 的第 3 段 merge 回 `main` 才開始（第 10 條）；原本的第 3 段（祖先的 bloom filter）照第 3 條不做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 比對時不配置、不複製：`dom` 存元素的 id 和切好的 class，`StyleElement` 借用 | 完成，merge 回 `main` | 2026-10-12 |
| 2 `RuleSet` 分桶；document 自己的 sheet 合成一個 | 完成 | 2026-10-13 |
| 3 祖先的 bloom filter、`CssIdent` 的 hash | 不做（第 3、9 條） | |

## 驗收

### 元素的 id 和 class

| 項目 | 怎麼驗證 |
| --- | --- |
| 元素的 `id` 和切好的 `class`：建立時就有、ASCII 空白切開（no-break space 不算）、去掉重複、照第一次出現的順序；設定、替換、移除、`set_attr_node` 之後跟著變，其他屬性不影響；只算 null namespace 的；其他節點沒有；`attributes_ref` 照順序借用 | `cargo test -p dom --test it style_data::` |
| 比對讀存好的值：quirks mode 的 class 和 id 不分大小寫、standards mode 分；`[\|name]` 只認 null namespace 的屬性、`[*\|name]` 任何 namespace；`:link` 要 `href` | `cargo test -p style --test it cascade::quirks_mode_matches_classes_and_ids_case_insensitively cascade::attribute_selectors_with_a_namespace`、`cargo test -p style` |

### 分桶

| 項目 | 怎麼驗證 |
| --- | --- |
| 分桶和逐條比對給每個元素一樣的 style（standards、quirks mode，有 adopted sheet；id、class、tag（大寫、SVG 的大小寫）、屬性、`*`、`:is()`、`:where()`、`:not()`、`:has()`、combinator、`!important`、多個 selector 的規則、兩份 `<style>`）；值本身也對：後面的 sheet 贏、adopted 的排在 document 的之後、規則取最大的 specificity；逐條比對真的比對得多 | `cargo test -p style --test it matching::the_buckets_give_the_same_styles` |
| 一千條規則裡只比對自己桶裡的：class、id、最右邊的 compound（`.x .cN`）、tag、class 的大小寫不影響、只差大小寫的 class 和兩個桶給的同一個 selector 只比對一次；「其他」對每個元素都比對 | `cargo test -p style --test it matching::few_selectors_are_matched` |

### 行為不變和效能

| 項目 | 怎麼驗證 |
| --- | --- |
| WPT 的預期結果不變 | `python3 scripts/test.py wpt css/selectors/ css/css-cascade/ css/cssom/ dom/nodes/`（0 個預期外） |
| Complex-DOM 等的 `Document::UpdateStyle` 變少（沒有自動的效能測試，下面的量測） | `python3 scripts/ci.py speedometer --trace --browser kun --iterations 3 --suites TodoMVC-React-Complex-DOM,TodoMVC-Svelte-Complex-DOM,TodoMVC-jQuery,TodoMVC-React-Redux` |

## 做過的檢查

- **量測**（`ci.py speedometer --trace --browser kun --iterations 3`，`perf` profile，`Document::UpdateStyle` 的合計；和 `rfcs/0012-speedometer/progress.md` 的「優化前的重新量測：比較表、trace 和 `perf`（2026-10-12，Fedora 44）」同一台、同樣的 suite；每次錄約 200 到 230 秒，含重編 `perf` profile）：

  | Suite | 之前 | 第 1 段後 | 每份 sheet 一個 `RuleSet` | 合成一個（完成時） | Chrome |
  | --- | --- | --- | --- | --- | --- |
  | TodoMVC-React-Complex-DOM | 31.0 秒 | 8.4 秒 | 4.0 秒 | 1.9 秒（16 倍） | 59 ms |
  | TodoMVC-Svelte-Complex-DOM | 25.1 秒（一次 387 ms） | 5.7 秒（89 ms） | 4.2 秒（65 ms） | 2.0 秒（30 ms，13 倍） | 62 ms |
  | TodoMVC-jQuery | 6.2 秒 | 5.0 秒 | 1.2 秒 | 1.4 到 1.7 秒 | 15 ms |
  | TodoMVC-React-Redux | 4.5 秒 | 3.0 秒 | 0.76 秒 | 0.80 到 1.0 秒 | 14 ms |

  jQuery 和 React-Redux 完成時跑了三次（1.43、1.37、1.73 秒和 1.00、0.80、0.97 秒），每份 sheet 一個時的數字在這個起伏裡。完成時剩下的（`perf`，Svelte-Complex-DOM 的 main thread）分散在 `cascade` 本身、套用和複製 property 的值（`Value::clone`）、`selectors` 的比對、候選的排序，所以不做 bloom filter（第 3 條）。
- **改過的做法**：第 11 條原本 document 的每份 sheet 各一個 `RuleSet`。量到 Svelte-Complex-DOM 只從 5.7 秒降到 4.2 秒，`perf` 的 main thread 有 37% 在查桶（SipHash、雜湊表）。換成 FxHash 試過（實驗，已還原）只到 4.0 秒；`lookup` 的時間在載入雜湊表（cache miss）：Complex-DOM 的頁面有 420 個 `<style>`（10042 個 class，8546 個有大寫字母），每個元素的每個 class 在每份 sheet 裡各查一次。document 的 sheet 合成一個之後降到 2.0 秒。
- **rebase**：第 1 段做完時 RFC 0016 剛做完，rebase 到它的 `main`（`73cc8b9e`）沒有衝突（`StyleElement` 留在 `style`，stylesheet 搬到了 `css`），重跑 `cargo test --workspace`（43 個 binary、2470 個通過，207 秒）、fmt、clippy 之後 merge。
- **mutation test**：`mutants-1.toml`（17 條，127 秒）、`mutants-2.toml`（20 條，94 秒），37 條都抓到。沒抓到、補了測試的：id 不管 quirks mode 的大小寫、屬性 selector 不管 namespace（現有的 `style` 測試沒涵蓋）；規則的順序只在自己的 set 裡、adopted 的排序、規則取第一個符合的 selector 的 specificity（同時改了逐條比對的路，比較兩條路看不出來，補了值本身的斷言）；候選沒有排序（兩個桶給的同一個 selector 中間隔著別的時沒去重，補了計數的斷言）。編不過的 3 條（拿掉 id、class、tag 的桶）改寫成讓那個桶不被選；clippy 改了 `attributes_changed` 之後，指向它的 3 條改寫、重跑。
- **跑過的測試**（第 2 段做完時）：`cargo test --workspace` 43 個 binary、2472 個通過（151 秒，含 clippy）；`cargo fmt --check`（連同 `sdk/lua/generator`、`sdk/rust`）、`cargo clippy --workspace --all-targets -- -D warnings`（修了巢狀的 `if`、新測試的 `drop(scope)`、排版）；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py wpt css/selectors/ css/css-cascade/ css/cssom/ dom/nodes/`：18111 個預期的結果、0 個預期外（100 秒；第 1 段時不含 `css/cssom/`，15630 個）。
- **畫面**：沒有看得到的改變，情境不需要。
