# RFC 0040 的進度：元素和屬性的名稱改成 atom

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-13。完成：2026-10-13，Fedora 44（Linux x86_64，AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0040` 上做兩段，一起 merge 回 `main`；原本的第 3 段做了、量了、拿掉（第 4 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `LocalName`、`Namespace`、`prefix` 換成 atom 或字串，`QualName` 的欄位私有 | 完成 | 2026-10-13 |
| 2 parser 不轉換；寫在程式裡的名稱全部改成 `local_name!`、`ns!`；量測 | 完成 | 2026-10-13 |
| 3 class 和 id 的值；`RuleSet` 用存好的 hash；量測 | 做了、量了、拿掉（第 4 條） | 2026-10-13 |

## 驗收

### 名稱

| 項目 | 怎麼驗證 |
| --- | --- |
| 名稱的表示：7 bytes 以內 inline atom、更長的已知名稱靜態 atom、更長的不認得的字串（不是動態 atom）；從字串、`local_name!`、html5ever 的 atom（動態的也是）建的同一個名稱相等、`Hash` 和字串一樣，`to_atom` 回到同一個 atom；和 atom 比較：同一個名稱是它的 atom、別的不是，字串的名稱只等於同一段文字的動態 atom；大小寫、namespace 不同就不同，atom 和字串永遠不同；namespace 從網址和 atom 建的一樣，HTML、SVG、沒有；`QualName` 的 prefix | `cargo test -p dom --test it names::` |
| `is_html`、`is_svg`、`is_attr` 只認自己的 namespace | `cargo test -p dom --test it tree::names_are_of_their_namespace` |

### 用到名稱的地方

| 項目 | 怎麼驗證 |
| --- | --- |
| 只有 null namespace 的 `on…` 屬性是 event handler（`setAttributeNS` 的 `onclick` 不是） | `cargo test -p dom_js --test it dispatch::` |
| `<img>` 的 `src`、`srcset`、`sizes`、`crossorigin`、`referrerpolicy` 改了記下要更新，其他屬性和別的元素的 `src` 不記 | `cargo test -p dom_api --test it api::an_images_attribute_changes_are_recorded` |
| 行為不變 | `cargo test --workspace`；`python3 scripts/test.py wpt dom/ html/semantics/ css/selectors/`（下面那兩個測試以外 0 個預期外）；`python3 scripts/test.py reftest tests/reftests/svg tests/reftests/cascade tests/reftests/script` |

## 做過的檢查

- **做了什麼**：
  - 第 1 段（第 5 到 7 條）：`dom` 的 `name` 模組：`Name<S>`（`string_cache` 的 `Atom<S>` 只放 inline 和靜態的，或 `Box<str>`），`LocalName`、`Prefix`、`NamespaceUrl`；`new` 照 `string_cache` 的規則（7 bytes 以內 inline，更長的 `try_static`，查不到存字串），`from_atom` 把動態的轉成字串，`to_atom` 給 html5ever。`Namespace` 包 `NamespaceUrl`（`svg()`、`from_atom`、`to_atom`、`is_html`、`is_svg`、`is_none`）。`QualName` 的欄位私有，加 `with_prefix`、`local()`、`namespace()`、`prefix()`。`dom` 依賴 `markup5ever`、`string_cache`。其他 crate 機械式的修改：讀欄位改成方法（照 rustc 的 JSON 錯誤自動加 `()`，140 多處）、`&*name.local()` 改成 `.as_str()`、12 處 `QualName { … }` 改成 `with_prefix`、`cargo clippy --fix` 修了多餘的借用。
  - 第 2 段（第 8 到 11 條）：比較名稱的方法收 atom，`Name` 和 atom 比較。寫在程式裡的名稱 1056 處用 regex 包成 `local_name!`（149 個檔案），照 rustc 的錯誤收尾：`local_name!` 沒有的名稱（測試裡的 50 多處）改回字串、名稱清單的陣列改成 atom 的陣列、收名稱的小函式改收 atom；`autocapitalize` 放在 `dom::name::unlisted`。namespace 和網址字串的比較 43 處改成 `is_html()`、`is_svg()`、`is_none()`。parser 的 `qual_name` 用 `from_atom`，`elem_name` 和序列化用 `to_atom()`。連同第 1 段約 190 個檔案。
  - 文件：`crates/dom/README.md`（名稱的型別和 API、`names` 測試）、`crates/dom/soundness.md` 的「`NoTrace` impl」（名稱裡不放動態 atom）、`docs/architecture.md` 的第三方選型（`markup5ever`、`string_cache`）、`docs/testing.md` 的等價的 mutation、`docs/limitations.md`。
- **量測**（`perf stat -e instructions:u,cycles:u`，整個 kun process，5 個 iteration；`main` 和 `0040` 輪流各量兩次，`main` 在 `../kun-main` 的 worktree 編。時間的量測起伏太大：只跑 kun、3 輪時 `main` 自己的 jQuery 兩次是 1642 和 1934 ms）：

  | Suite | `main` 的 instructions | 0040 的 instructions | cycles |
  | --- | --- | --- | --- |
  | TodoMVC-JavaScript-ES5 | 24.1–24.3 G | 20.9–22.5 G（−7% 到 −13%） | −7% 到 −13% |
  | TodoMVC-jQuery | 118.0–118.1 G | 105.5–105.6 G（−10.6%） | −5% 到 −12% |
  | TodoMVC-ES6-Webpack-Complex-DOM | 68.5–68.6 G | 64.9–66.4 G（−3% 到 −5%） | −6% 到 −8% |
  | TodoMVC-React-Complex-DOM | 61.3–61.7 G | 60.1–60.8 G（−1% 到 −2%） | −6% 到 −8% |

  剩下的成本大多不在名稱。
- **改過的做法**：
  - 第 2 段第一次和 `main` 輪流量（時間，各 3 輪），ES5、ES6-Webpack-Complex-DOM、jQuery 慢了 5% 到 15%。frame pointer 的 `perf`（ES5）：`QualName::attr::<&&str>` 4.1%、`phf_shared::hash` 2.65%：parser 每設一個屬性，`handlers::attribute_changed` 就對約 100 個 event handler 的名稱各建一個 `QualName`（長的名稱每次跑 `try_static` 的雜湊），`form_states`、`image` 也有同樣的清單。改成先看 `on` 開頭和 null namespace 再比字串，清單改成 atom 的陣列；之後用指令數量的就是上面的表。
  - 第 3 段（第 12 到 15 條）：`css` 的 `class_hash`（ASCII 小寫的 FxHash，`rustc-hash`）、`RuleSet` 的 id 和 class 桶改用 `hashbrown::HashTable`、`dom` 的 `ClassName`（名稱加 hash）。和第 2 段的狀態（放在 worktree 編）輪流各量兩次：Svelte-Complex-DOM 的指令數 55.9、57.2 G → 56.6、57.4 G（+0.4% 到 +1.1%），cycles +1% 到 +2.5%；React-Complex-DOM 60.0、60.5 G → 60.9、61.4 G（約 +1.4%），cycles +1.4% 到 +3.9%。沒有收益，程式還原成第 2 段的狀態（從 worktree 複製回 9 個檔案），改寫第 4 條（推測的原因在那裡，沒有另外用 `perf` 驗證）；沒有跑它的 mutation test。
- **mutation test**：`mutants-1.toml`（10 條，57 秒）全部抓到（寫 spec 時先補了「atom 和字串永遠不同」的斷言）。`mutants-2.toml`（7 條；第一次 8 條、145 秒）：第一次漏了 2 條：`<img>` 的 `srcset` 不算（補了 `api::an_images_attribute_changes_are_recorded`，重跑抓到）；字串的名稱和同一段文字的靜態 atom 相等（等價，記進 `docs/testing.md`，從 spec 拿掉）。parser 用 `from_atom` 和從字串重建也是等價的（只差在快），沒有放進 spec。0039 的 `mutants-1.toml` 和 0012 的 `mutants-attributes.toml` 指向的程式被這份改寫了，不再對得上；那兩份記的是當時測過的東西。
- **跑過的測試**（第 3 段拿掉之後）：`cargo test --workspace --no-fail-fast` 43 個 binary、2481 個通過（59 秒）；`cargo fmt --check`（連同 `sdk/lua/generator`、`sdk/rust`）；`cargo clippy --workspace --all-targets -- -D warnings`；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`。第 2 段時（程式和現在一樣）：`test.py wpt dom/ html/semantics/ css/selectors/` 71070 個預期的結果，36 個預期外都在兩個測試裡，都不是這份造成的：`canvas-descendants-focusability-004.tentative.html` 的 UNEXPECTED-OK（`main` 上已經是這樣，預期結果還沒更新）、`focus-dynamic-type-change-on-blur.html` 平行跑時 TIMEOUT（單獨跑有沒有這份都通過；第 1 段時 `focus-dynamic-type-change.html` 也一樣）；`test.py reftest tests/reftests/svg tests/reftests/cascade tests/reftests/script` 0 個預期外。
- **畫面**：沒有看得到的改變，情境不需要。
