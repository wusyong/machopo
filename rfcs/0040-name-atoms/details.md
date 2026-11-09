# RFC 0040 的細節

## 量測

2026-10-13，`main` 的 `1bcc5329` 之前（RFC 0039、live collection 和表單簿記的修正做完時），Fedora 44，AMD Ryzen AI 9 HX 370。`perf` profile 用 `RUSTFLAGS="-C force-frame-pointers=yes"` 重編，`perf record -F 999 --call-graph fp -p <kun>`，每個 suite 2 個 iteration，只看 `CrRendererMain`：

| Suite | `memcmp` | `dom_api::frame::update_elements` | `html::sink` | `dom_api::markup` |
| --- | --- | --- | --- | --- |
| TodoMVC-JavaScript-ES5 | 16.9% | 43.7% | 22.6% | 0.6% |
| TodoMVC-jQuery | 17.7% | 33.4% | 21.4% | 23.5% |
| TodoMVC-ES6-Webpack-Complex-DOM | 8.7% | 27.1% | 14.9% | 14.1% |
| TodoMVC-React-Complex-DOM | 6.1% | 13.9% | 2.3% | 0.3% |

（`update_elements` 的那一大塊在表單簿記的修正之後少了很多；`memcmp` 的其他來源見下。）

`memcmp` 的直接呼叫者（jQuery，修正之前）：`input_type` 10.8%、`form_named` 1.6%、`has_disabled` 0.8%、`input::state` 0.7%、`get_element_by_id` 走過子孫 0.7%、`style::cascade` 讀 `style` attribute 0.7%、`button_type` 0.6%，其餘分散。都是 `NodeRef::attribute` 用 `QualName` 找屬性（`a.name == name` 逐一比字串）或 `is_html`。

## 做法的比較

| | A：`string_cache` 的靜態和 inline（選的） | B：自己產生的靜態編號 | C：`string_cache` 全部 | D：每個 thread 一張表（Blink） |
| --- | --- | --- | --- | --- |
| 已知的名稱 | 靜態 atom，比整數 | 自己的編號，比整數 | 靜態 atom | 靜態 atom |
| 7 bytes 以內的不認得的 | inline atom（值在 atom 裡），比整數 | 字串 | inline atom | intern |
| 更長的不認得的 | 字串 | 字串 | 動態 atom，比整數 | intern，比指標 |
| 寫在程式裡的名稱 | `local_name!("class")` | 產生的常數 | `local_name!` | 產生的常數 |
| parser | 直接用 html5ever 的 atom | 每個名稱轉換（查表） | 直接用 | 轉換 |
| 清單 | html5ever 的（`web_atoms`） | 自己維護 | html5ever 的 | 自己的 |
| 複製 | 整數或字串 | 整數或字串 | 動態的是 atomic 的參考計數 | 參考計數 |
| GC 的 finalizer 裡 drop | 只釋放記憶體：I6 不變 | 不變 | 動態的到 0 時拿全域的鎖：I6 要改 | 碰 thread-local 的表：I6 要改 |
| `css` 的純資料規則 | 不變 | 不變 | 違反（intern） | 違反（thread-local） |
| 跨 thread | 安全 | 安全 | 安全 | 型別擋不住 |

## `string_cache` 的 atom

- `string_cache` 0.11：atom 是 64 位元，低兩位是種類：動態（`DYNAMIC_TAG`，指向全域的表裡的 entry，有參考計數）、inline（`INLINE_TAG`，7 bytes 以內的字串直接存在裡面）、靜態（`STATIC_TAG`，靜態清單的編號）。`Atom::from`：7 bytes 以內一律是 inline（即使在靜態清單裡，例如 `div`），更長的先查靜態清單、查不到才建動態的，所以同一個字串永遠是同一種表示；`local_name!` 也照這個規則（短的是 inline）。`try_static` 只查靜態清單，不看長度：短的名稱也會給靜態的那一種，和 `Atom::from` 不一致，所以 kun 從字串建名稱時，短的照 `Atom::from`（inline）、長的才用 `try_static`。`is_static`、`is_inline`、`is_dynamic` 看種類（2026-10-13 讀 `string_cache` 0.11 的 `atom.rs` 確認）。
- 靜態清單：`web_atoms` 0.3.0 的 `local_names.txt`，1108 個，HTML、SVG、MathML 的元素和屬性名稱，SVG 的大小寫照寫（`clipPath`、`viewBox`）。html5ever 的 `local_name!` 就是它。
- 寫在程式裡、用字串建的名稱（`QualName::attr("class")`）在 debug build 斷言是靜態的（寫錯名稱的話測試時就發現）：實作時確認。

## 先例

### Blink

- `third_party/blink/renderer/core/dom/qualified_name.h`：`QualifiedName` 指向 intern 過的 `QualifiedNameImpl`（prefix、local name、namespace），比較是比指標；`QualifiedName::InitAndReserveCapacityForSize` 在啟動時建好靜態的名稱。
- `third_party/blink/renderer/core/html/html_tag_names.json5`、`html_attribute_names.json5`、`svg/svg_tag_names.json5` 等：產生 `html_names::kDivTag`、`kClassAttr`。
- `third_party/blink/renderer/platform/wtf/text/atomic_string_table.h`：每個 thread 一張表。

### Servo / html5ever

- `string_cache`：`Atom<Static>`，靜態的編在 `phf` 的表裡（atom 的資料是 tag 加編號），動態的在全域的 `DYNAMIC_SET`（分桶的 `Mutex`），參考計數到 0 時移除。
- `markup5ever` 的 `LocalName`、`Namespace`、`Prefix` 是三種 `Atom`；`local_name!`、`ns!` 是編譯期的靜態 atom。
