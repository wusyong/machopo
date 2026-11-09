# RFC 0079 的進度：Window 的具名屬性

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-23，Fedora 44。在 `0079` 上做。

完成日期：2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `dom` 的 id 和 name 的表，`getElementById` 用它（第 1 到 4 條） | 完成 | 2026-10-23 |
| 2 named properties object 和 getter；WPT 的預期結果（第 5 到 8 條） | 完成 | 2026-10-23 |

## 驗收

**名稱的表（`dom`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 每個 tree scope（document、shadow root）的 id 的表跟著插入、移除、搬移和 `id` 的改變；同名的第一個照 tree order；shadow tree 各自一張（host 不在 document 裡也是，插入有 window 的 document 時也不混進去）；fragment 走樹，template 的內容不算 | `cargo test -p dom --test it -- tree_scopes`；debug build 每次查都和走樹的結果比對（`dom`、`dom_api`、`dom_js` 的測試都經過它） |
| document 的 name 的表（HTML 的 `embed`、`form`、`img`、`object`）和 iframe 的 `name` 的表：名稱不空，不在 shadow tree，跟著改名、移除和搬移 | `cargo test -p dom --test it -- tree_scopes::named_elements`；`cargo test -p dom_js --test it -- iframes::a_child_browsing_context_by_its_name` |
| 查一個名稱不走樹（同名的有好幾個時第一次查走一次、之後記住到再有元素進出）；被回收的 scope 的表在表數加倍時拿掉 | `cargo test -p dom --test it -- tree_scopes`（`dom::testing::tree_scope_walks`、`tree_scope_prunes`） |
| `getElementById` 用表：10,000 個元素時找不到從 323 µs 變成 0.25 µs | `details.md` 的量測 |

**window 的具名屬性**

| 項目 | 怎麼驗證 |
| --- | --- |
| 照 Chrome 152：任何元素（包括 SVG）的 `id`、named element 的 `name`；一個是它，多個是 live 的 `HTMLCollection`（script 拿著時同一個，tree order，SVG 的也在）；子 frame 的名稱是它的 window、優先於元素；沒有子 frame 的 iframe 不算；不在 shadow tree、名稱不空；跟著 DOM 改變；`var`、window 自己的屬性、`Window.prototype` 和更上面（`EventTarget.prototype`、`Object.prototype`）的名稱優先 | `cargo test -p dom_js --test it -- window_named iframes::a_child_browsing_context_by_its_name`（預期值是 Chrome 152 的，`details.md`） |
| named properties object：`[object WindowProperties]`，在 `Window.prototype` 和 `EventTarget.prototype` 之間；屬性 writable、configurable、不可列舉、不在 own keys；`delete` 失敗，在它上面設定、`defineProperty` 丟 `TypeError` | `cargo test -p dom_js --test it -- window_named::the_named_properties_object bindings::interface_objects_and_prototype_chains` |
| iframe 的 `name` reflect 屬性 | `cargo test -p dom_js --test it -- iframes::a_child_browsing_context_by_its_name` |
| 查不到的全域名稱 110–330 ns（Chrome 95–200 ns），和元素的數量無關 | `details.md` 的量測 |
| WPT 的 `html/browsers/the-window-object/named-access-on-the-window-object/`（加進清單）：97 個 subtest 通過 88 個，沒過的是 `window.name`、`<object>` 的子 frame、跨源的 `postMessage`（`.ini` 的註解） | `python3 scripts/test.py wpt html/browsers/the-window-object/named-access-on-the-window-object/` |
| 之前因為它失敗的 WPT（`.ini` 的註解寫到的 47 個目錄、`dom/nodes/`、`shadow-dom/`）：266 份 `.ini` 拿掉（整份通過），原本停用的 13 份重新打開 12 份 | `python3 scripts/test.py wpt <那些目錄>`（目錄列在「做過的檢查」的 WPT 那一條） |

## 做過的檢查

2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370），兩段都在這台上做。

- **做了什麼**（設計見 `README.md` 的「詳細設計」）：
  - 第 1 段：`dom` 的 `tree_scope.rs`（`TreeScopes`，`DomState::tree_scopes`）：每個 tree scope 一張 `id` 的表、document 一張 `name` 的表，以 `NodeId` 存，一個名稱一個元素時存它、多個時存數量和找到的第一個；插入、移除共用的 `walk_moved_subtree` 多一個回呼（light tree 的每個節點），`attributes_changed` 在 `id`、named element 的 `name` 改變時先拿出、再放回；`DomReadScope::element_by_id`、`first_named_element`（debug build 和走樹比對）；`dom_api` 的 `getElementById` 用 `element_by_id`。`dom::testing` 多兩個計數：`tree_scope_walks`、`tree_scope_prunes`。
  - 第 2 段：`dom` 多一張 iframe 的 `name` 的表和 `DomReadScope::window_named`（debug build 和走樹比對）；`dom_api` 的 `window::named_property`、`CollectionFilter::WindowNamed`、iframe 的 `name`；`dom_js` 的 `window_properties.rs`（每個 realm 一個，`set_up_realm` 時接在 `Window.prototype` 之下）。
- **量測之後改的**（`details.md`）：第 1 段第一版改 `id` 多約 150 ns：表變空時連同 scope 的表一起拿掉、下一次重建（量的頁面只有一個有 `id` 的元素，每次都這樣），改成表變空時留著（被回收的 scope 才拿掉）。名稱的雜湊換成 FxHash、拿出時不複製名稱，量不出差別，留著。最後改 `id` 多約 70 ns、插入或移除一個有 `id` 的元素各多約 80 ns，比決定時估的 30–60 ns 多，和 Chrome（多約 50 ns）同一個數量級。
- **測試時找到的**：
  - 清表判斷 scope 還在不在用了 `NodeIdMap::get`（在 id 表 prune 之前一直是 `Some`），回收的 scope 一個都沒拿掉；改成看節點本身（`NodeIdMap::node`；插入、改屬性的那一刻不會 GC）。
  - 第一次寫 iframe 的測試時 `f.name = 'side'` 沒有作用：kun 的 iframe 沒有 `name` 成員（只是 JS 的屬性），加了 reflection（第 6 條；WPT 的 `window-null-names` 也是因此沒過）。
- **mutation test**：
  - `mutants-1.toml`（第 1 段，26 個）：第一次 3 個 mutant 寫錯編不過（改寫）；「shadow tree 裡的元素也進 host 的表」沒抓到：插入時只在有 browsing context 的 document 才走進 shadow tree，測試的 document 沒有 window，補了有 window 的情況。改了之後那 4 個都抓到。另外 3 個等價（空的 `id`、空的 `name`、shadow root 也收 `name`：多收的查不到），記在 `docs/test-gaps.md`。其他 19 個第一次就抓到。
  - `mutants-2.toml`（第 2 段，最後 16 個）：第一次 19 個沒抓到 8 個。V8 的 interceptor 沒有 setter 時設定走 definer、沒有 definer 時 `defineProperty` 走 setter，沒有 query 時屬性是 `DONT_ENUM`：setter 和 query 拿掉（程式和 mutant），剩下的「沒有 definer」就抓到。masking 的 interceptor、集合裡只算 HTML 元素的 `id`、沒有子 frame 的 iframe 也算：補了 prototype chain 上的名稱、同名的 HTML 和 SVG 元素、沒有子 frame 的 iframe（Chrome 152 探測過），都抓到。另外 2 個等價（「一個」的捷徑拿掉時改走集合、結果一樣），記在 `docs/test-gaps.md`。
  - 每次跑完都確認檔案還原（`target/mutate/originals/` 不存在）。
- **WPT**：
  - 第 1 段 `python3 scripts/test.py wpt dom/nodes/ shadow-dom/`：`shadow-dom/offsetParent-across-shadow-boundaries.html` 4 個非預期的通過；把 `getElementById` 暫時改回走樹也一樣通過，和這次無關（`.ini` 是 2026-10-14 的，之後別的改動讓它們通過），更新了它的 `.ini`。
  - 第 2 段：`.ini` 的註解寫到 named access 的 278 份在 47 個目錄（`css/CSS2/floats/`、`css/css-animations/`、`css/css-contain/`、`css/css-display/`、`css/css-gaps/animation/`、`css/css-grid/animation/`、`css/css-pseudo/`、`css/css-scroll-anchoring/`、`css/css-shadow/`、`css/cssom-view/`、`css/css-transitions/`、`dom/collections/`、`dom/events/`、`html/rendering/widgets/`、`html/semantics/`、`intersection-observer/`、`resize-observer/`、`shadow-dom/crashtests/`、`web-animations/` 等），連同 `dom/nodes/` 全部重跑：266 份 `.ini` 拿掉（整份通過），160 份更新。非預期的失敗和 timeout 逐一看過，都是原本第一步就丟 `ReferenceError`（子測試沒有跑、或 reftest 的 wait 沒有拿掉），現在跑到 kun 缺的其他功能（smooth 捲動、`round()`、highlight 的 pseudo-element、`document.styleSheets`、`::details-content`、`switch` 等），沒有因為這次改動而壞掉的。
  - 停用的 13 份（理由寫著 named access）重新打開：8 份整份通過，4 份在時間內跑完（smooth 捲動、iframe 的畫面等），`intersection-observer/cross-origin-tall-iframe-rendering.sub.html` 還在等跨源 frame 的 `postMessage`，照舊停用、理由改寫。
  - 留下的 98 份 `.ini` 的註解還寫著 named access：交給兩個 subagent 照測試的原始碼和這次的失敗訊息改寫（只改註解），我抽查過其中的說法（例如 kun 沒有 `document.styleSheets`、`<style>` 的 `sheet`）；`css/css-scroll-anchoring/` 的 6 份自己改。
  - 最後一次：上面的目錄加上 named access 的目錄和 `dom/nodes/`，54654 個檢查只剩 `dom/nodes/` 的 6 個非預期的通過，更新之後全部符合預期。
- **順帶找到的、和這份無關的**：kun 沒有 CSS 的 `round()`（`css-scroll-anchoring/adjustment-followed-by-scrollBy.html` 的 div 因此沒有高度）；`html/rendering/widgets/input-checkbox-switch-indeterminate.tentative.html` 之前是碰巧通過（`input.indeterminate = true` 丟出 `ReferenceError`），現在露出 kun 沒有 `switch` 的控制項。
- **跑過的**：`dom`、`dom_api`、`dom_js` 的測試；上面的 WPT；情境全部（70 個）。每段結束時：`cargo test --workspace --no-fail-fast`（最後一次 3640 個通過、3 個 ignored）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `deps`、`boundary`（第 1 段第一次抓到 `dom::testing` 新的兩個計數是 static：和同一個檔案的其他計數一樣是 thread-local，加進 `sanity.py` 的例外清單）、`unsafe`、`tests`、`numerics`、`docs`（只剩 README 的 `../third_party/rusty_v8_build` 在這台不存在，和這次無關）。第 2 段第一次的 `cargo test --workspace`（4 分 31 秒）3636 個通過、4 個失敗、3 個 ignored：iframe 多了 `name`，`dom_api` 的 `tests/machopo-dom.wit`、`spec-missing.txt` 和 `dom_wasm` 的 `typed_list.rs` 要重新產生（`MACHOPO_UPDATE_GOLDEN=1`），wasm 的 guest 要重編（`python3 scripts/ci.py guests`：`resources/kun-lua.wasm` 和 Rust 的 guest）；之後全部通過。
- **不穩定的**：沒有。
