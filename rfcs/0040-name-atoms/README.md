# RFC 0040：元素和屬性的名稱改成 atom

- 開始日期：2026-10-13
- 狀態：✅ 已完成：2026-10-13（第 1 到 15 條；兩段，都在 Fedora 44 上做；原本的第 3 段做了、量了、拿掉（第 4 條）；驗收和檢查在 `progress.md`）
- 細節：`details.md`

這份接在 RFC 0039 之後，是它的「之後的可能」的「屬性的名稱和 class、id 改成 atom」（RFC 0039 的第 1 條沒選的 Q2）。RFC 0012 的優化：量測在 `rfcs/0012-speedometer/progress.md` 的「表單的簿記讀屬性時不配置、名稱先比 local name（2026-10-13，Fedora 44）」和它前面兩則。

## 摘要

讓 `dom` 的元素和屬性的名稱比較變成整數比較，寫在程式裡的名稱是常數。用 html5ever 已經在用的 `string_cache`（`markup5ever` 的 `LocalName`、`Namespace`）：7 bytes 以內的名稱是 inline atom（值直接存在 atom 裡），更長的已知名稱（HTML、SVG、MathML 的，`web_atoms` 的 1108 個）是靜態 atom，兩種都不經過任何表；會變成動態 atom 的（長的、不認得的名稱，例如 `data-todo-id`）照舊存字串，DOM 裡不放動態 atom（第 1 條）。對應 Blink 從 `.json5` 產生的靜態 `QualifiedName`（`html_names::kClassAttr`）那一半；Blink 的另一半（每個 thread 一張 `AtomicStringTable`，任何字串都 intern）不照做。寫在程式裡的名稱全部是 `local_name!` 常數，比較名稱的 API 收 atom，parser 直接搬 html5ever 的 atom。結果：TodoMVC 的 ES5、jQuery、ES6-Webpack-Complex-DOM、React-Complex-DOM 的 kun 執行的指令少了 1% 到 13%、cycles 少了 5% 到 13%（`progress.md`）；剩下的成本大多不在名稱。class 和 id 的值改成 atom 加 hash 做過、量過，沒有收益，拿掉（第 4 條）。

## 動機

**現況（2026-10-13，`main` 的 `52c8d9af`；Fedora 44，AMD Ryzen AI 9 HX 370）**：
- **`dom` 的 `QualName`**（`crates/dom/src/node.rs`）是三個字串：`namespace: Namespace(Box<str>)`、`prefix: Option<Box<str>>`、`local: Box<str>`。比較名稱就是比字串；`QualName::attr("class")`、`QualName::html("div")` 這種寫法每次都配置。用到 `QualName` 的地方有 1238 處，其中 844 處是寫在程式裡的名稱。
- **`perf`**（frame pointer 重編的 `perf` profile）：`memcmp` 在 TodoMVC 的 ES5、jQuery、ES6-Webpack-Complex-DOM、React-Complex-DOM 佔 renderer main thread 的 6% 到 18%，配置和釋放還有一些。RFC 0039 和之後的兩個修正處理了最熱的幾處（style 的比對、表單的簿記），剩下的散在 parser（`html` 的 sink 把 html5ever 的每個名稱轉成 `Box<str>`，`ParserHooks::attribute_changed`）、`dom_api` 的各種 `is_html` 和 `attribute(&QualName::attr(…))`、`dom` 自己（`create_element_of` 每次比十幾個表單元素的名稱）。
- **Speedometer**（同一天只跑 kun 的量測，比值用上一次的 Chrome 算）：ES5 約 4.6 倍、jQuery 約 16.7 倍、ES6-Webpack-Complex-DOM 約 11.1 倍、React-Complex-DOM 約 10.4 倍。

目標：
- 比較兩個已知的名稱是比整數；寫在程式裡的名稱是常數（`local_name!("class")`），不配置。
- parser 建元素和屬性時，已知的名稱直接用 html5ever 給的 atom，不轉換、不配置。
- 行為不變：大小寫、namespace、prefix 照舊；不認得的名稱照舊能用。
- 不改安全論證：名稱照舊是純資料，GC 的 finalizer 釋放它時只釋放記憶體（`crates/dom/soundness.md` 的 I6），`css` 照舊不 intern。

不在範圍內：
- **不認得的長名稱 intern**（動態 atom）：見「理由和其他做法」的 C、D。
- **`css` 的 `CssIdent`**：照舊是字串；selector 的 tag 名稱改用 atom 在「之後的可能」。

## 設計概要

對網頁的作者：沒有變化。對改 `dom` 和以上的人：

- `QualName` 的 `local` 是 `LocalName`：`markup5ever` 的 atom（靜態或 inline），或是字串（只有會變成動態 atom 的才是）。同一個名稱永遠是同一種表示，比較是先比種類、再比 atom（64 位元的整數）或字串。`namespace` 同樣是 `markup5ever` 的 `Namespace` atom（`ns!(html)`、`ns!(svg)` 都是靜態的），任意的 XML namespace 才是字串。
- 寫在程式裡的名稱一律用 html5ever 的巨集：`QualName::attr(local_name!("class"))`、`QualName::html(local_name!("div"))`，編譯期就是常數，不配置（第 3 條）。用字串建名稱的 API 照樣有（`setAttribute` 這種從 script 來的名稱要用），但程式裡寫死的名稱不用它。
- 讀名稱用 `name.local()`、`namespace()`、`prefix()`，可以當字串讀（`.as_str()`）；比較名稱的方法（`is_html`、`is_svg`、`is_attr`、`attr_ref`、`has_attr`）收 atom：`is_html(&local_name!("div"))`，比一個整數（第 8 條）。
- 元素存的 `id` 和切好的 `class`（RFC 0039 加的）照舊是字串：改成 atom 加 hash 做過、量過，沒有收益，拿掉了（第 4 條）。
- parser 從 html5ever 拿到的名稱，靜態和 inline 的直接放進 DOM。

## 詳細設計

### 名稱的型別（`dom`）

- `LocalName`：`Atom(markup5ever::LocalName)`（只放 `is_static()` 或 `is_inline()` 的）或 `Text(Box<str>)`（會變成動態 atom 的）。從 `&str` 建立：7 bytes 以內用 inline atom（`Atom::from`，不經過表），更長的 `try_static` 查得到就用靜態的，查不到存字串（`try_static` 不看長度，短的名稱不能用它，否則同一個名稱有兩種表示，見 `details.md`）；從 html5ever 的 atom 建立：靜態或 inline 的直接用，動態的轉成字串（動態 atom 由 html5ever 自己釋放，不在 GC 的 finalizer 裡）。`Deref<Target = str>`、`Display`、`PartialEq<str>`、`Hash`（照字串，所以和 `&str` 查表一致）。
- `Namespace`：同樣的兩種表示（`markup5ever::Namespace`）；`url()` 照舊回傳網址字串，`Namespace::HTML` 這些網址字串照舊（從網址建 namespace 時用）；比較用 `is_html()`、`is_svg()`、`is_none()`（比 atom）。
- `prefix`：同樣（`markup5ever::Prefix`），大多是 `None`。
- `QualName` 的欄位私有（第 6 條），經過 `new`、`html`、`attr`、`with_prefix` 建立（收 `local_name!` 的 atom 或 `&str`），`is_html`、`is_svg`、`is_attr` 收 atom（第 8 條）；`Name` 和 atom 比較（`PartialEq<Atom>`）；`PartialEq` 照新的表示（第 7 條）。
- `NoTrace`：靜態和 inline 的 atom 的 `Drop` 什麼都不做（只有動態 atom 碰表），字串只釋放記憶體；I6 的論證照舊成立，`soundness.md` 寫明 DOM 裡不放動態 atom、由 `Name` 的建構函式保證（`QualName` 的欄位私有），`dom` 的 `names::` 測試檢查每種名稱的表示。

### parser（`html`）

- `crates/html/src/sink.rs` 的 `qual_name` 把 html5ever 的 `QualName` 轉成 `dom` 的：靜態和 inline 的 atom 直接搬，不配置。
- 反方向（給 tree builder 的 `elem_name`、序列化）：`to_atom()`，atom 直接給，字串的才建 atom（在 html5ever 那一側，不進 DOM）。tree builder 很常呼叫 `elem_name`，原本每次從網址和名稱的字串轉 atom（第 10 條）。

### 用到名稱的地方

- `dom`、`dom_api`、`dom_js`、`style`、`layout`、`paint`、`html`、`renderer` 裡寫在程式裡的名稱全部改成 `local_name!`、`ns!`（844 處的 `QualName::attr("…")`、`QualName::html("…")`、`is_html("…")` 這一類，第 3 條）。`local_name!` 只收靜態清單裡的名稱（或 7 bytes 以內的），寫錯名稱編譯不過；清單外、又超過 7 bytes 的名稱（目前只有 HTML 的 `autocapitalize`）放在 `dom::name::unlisted`，從字串建（第 9 條）；測試裡自己取的名稱（`x-a`、`my-element`）照舊是字串。名稱清單的陣列（表單元素、SVG 的繪圖元素、`<img>` 的屬性）是 atom 的陣列，收名稱的小函式收 atom（第 11 條）。parser 每設一個屬性就經過的 event handler 檢查先看 `on` 開頭和 null namespace，再比字串（量測時找到的，見 `progress.md`）。
- `StyleElement::has_local_name`、`attr_matches` 照舊和 selector 的 `CssIdent` 比字串。

### class 和 id 的值（做了、拿掉了）

原本的第 3 段：`id` 和每個 class 存成 `ClassName`（第 1 段的 `Name` 加上 ASCII 小寫的 FxHash，`css::rule_set::class_hash`），`RuleSet` 的 id 和 class 桶改用 `hashbrown::HashTable`、拿存好的 hash 直接查（第 12 到 14 條）。做完、量了之後拿掉（第 4 條）：和第 2 段比，Svelte-Complex-DOM 和 React-Complex-DOM 的指令數多了 0.4% 到 1.4%、cycles 多了 1% 到 4%（`progress.md`）。RFC 0039 把 document 的 sheet 合成一個 `RuleSet` 之後，每個元素只查兩三份桶，省下的雜湊很少；每個元素設 class 時多算的 hash（Complex-DOM 每個 iteration 重新載入約 6600 個元素）比省下的多。

### 測試

- `dom`：`cargo test -p dom --test it names::`：每種名稱的表示（短的 inline、長的已知名稱靜態、長的不認得的字串，不是動態 atom）；從字串、`local_name!`、html5ever 的 atom（動態的也是）建的同一個名稱相等、`Hash` 和字串一樣、和 atom 比較；大小寫、namespace 不同就不同；namespace 從網址和 atom 建的一樣；`QualName` 的 prefix。`tree::names_are_of_their_namespace`：`is_html`、`is_svg`、`is_attr` 只認自己的 namespace。
- 行為不變：`cargo test --workspace`、`test.py wpt dom/ html/semantics/ css/selectors/`（預期結果不變）、`test.py reftest tests/reftests/svg tests/reftests/cascade tests/reftests/script`；`dom_js` 的 `dispatch::`（`setAttributeNS` 的 `onclick` 不是 event handler）、`dom_api` 的 `api::an_images_attribute_changes_are_recorded`。
- 效能：`perf stat -e instructions:u,cycles:u` 數 kun 執行的指令（時間的起伏太大），和 `main` 的 worktree 輪流量，記在 `progress.md`。
- mutation test：`mutants-1.toml`、`mutants-2.toml`。

### 分階段

1. `LocalName`、`Namespace`、`prefix` 換成 atom 或字串（包一層，第 2 條），`QualName` 的 API 照舊接受字串：行為不變，已知的名稱的比較已經變成比整數。
2. parser 不轉換；寫在程式裡的名稱全部改成 `local_name!`、`ns!`（第 3 條）。量一次。
3. class 和 id 的值（第 4 條），`RuleSet` 用存好的 hash：做了、量了，沒有收益，拿掉（第 4 條）。

## 缺點

- `dom` 的 API 換了名稱的型別、`QualName` 的欄位私有，所有 crate 都改了（約 190 個檔案）；`match name.local().as_str() { "div" => … }` 這種寫法（5 處）照舊比字串，atom 不能當 `match` 的 pattern。
- 收 `&str` 名稱的小函式（reflection、`image.rs`、`script.rs`、`layout` 的 SVG 的幾個）每次呼叫從字串建一個 `QualName`：7 bytes 以內 inline、更長的跑 `try_static` 的雜湊，比原本的配置不一定快。
- 依賴 `markup5ever` 和 `string_cache` 的型別（已經是 `html` 的依賴，現在 `dom` 也直接用）；html5ever 升級時清單跟著變（新的 HTML 名稱變成靜態的），行為不變。
- 不認得的長名稱（`data-todo-id`、`aria-*` 不在清單裡的、custom element 的長名稱）照舊比字串、配置。

## 理由和其他做法

第 1 條（2026-10-13）。四個做法都讓已知的名稱變成 atom，差別在不認得的名稱；完整的比較在 `details.md` 的「做法的比較」。

- **A（選的）：`string_cache` 的靜態和 inline atom，DOM 裡不放動態 atom**：parser 不轉換，不維護清單，名稱照舊是純資料，不改安全論證。
- **B：自己從同一份清單產生靜態編號**：和 A 一樣不碰表，但要自己維護清單和查表，parser 每個名稱轉換一次。
- **C：`string_cache` 全部用，包括動態 atom**：不認得的長名稱也比整數。但動態 atom 在整個 process 一張表，參考計數到 0 時拿全域的鎖移除：DOM 的節點由 GC 的 finalizer 釋放，要改 `soundness.md` 的 I6（finalizer 只釋放記憶體）和 `css` 的「不 intern」，重新審查；每次複製名稱都是 atomic 的參考計數，多個 renderer thread 共用那張表。
- **D：Blink 的做法，每個 thread 一張表**：沒有鎖，但 atom 只在建立它的 thread 有意義，型別系統擋不住它被帶到別的 thread（`css` 的型別要是 `Send + Sync`）；一樣要改 I6。
- A 和 C 用的是同一個型別：之後量到不認得的名稱多的頁面確實慢在這裡，再從 A 改成 C。

## 先例

- **Blink**：`QualifiedName`（prefix、local name、namespace 三個 `AtomicString`，`QualifiedNameImpl` 本身也 intern，比較是比指標）；`html_names`、`svg_names`、`xlink_names` 等由 `.json5` 產生的靜態名稱（`html_names::kClassAttr`、`kDivTag`）；`AtomicString` 每個 thread 一張 `AtomicStringTable`。照做的：靜態的名稱和常數；不照做的：每個 thread 的表、任意字串的 intern。
- **Servo / html5ever**：`string_cache` 的 `Atom`（`LocalName`、`Namespace`、`Prefix`），靜態的編在程式裡（`phf`），7 bytes 以內 inline，其他的在全域的表、有參考計數。照做的：用它的靜態和 inline atom；不照做的：動態 atom 進 DOM。
- 細節在 `details.md` 的「先例」。

## 已決定的事項

1. **A：用 `string_cache`（`markup5ever` 的型別）的靜態和 inline atom，DOM 裡不放動態 atom，會變成動態 atom 的名稱存字串**（2026-10-13）：已知的名稱（熱點幾乎都是）變成比整數、寫在程式裡的是常數；parser 不轉換、不維護清單；名稱照舊是純資料，I6 和 `css` 的規則不變。不選 B（自己產生清單：多維護一份、parser 要轉換）、C（動態 atom 進 DOM：要改 I6 和 `css` 的規則，GC 的 finalizer 裡拿全域的鎖，atomic 的參考計數）、D（每個 thread 一張表：型別擋不住跨 thread，也要改 I6）。不認得的長名稱多的頁面量到慢在這裡時，再考慮 C。
2. **`LocalName` 包一層 enum（atom / 字串）**（2026-10-13）：型別保證 DOM 裡沒有動態 atom。不選 `dom` 直接用 `markup5ever::LocalName`、由建立名稱的地方保證：保證不在型別裡，漏掉一處就把動態 atom 帶進 GC 的 heap。
3. **寫在程式裡的名稱全部改成 `local_name!`、`ns!`**（2026-10-13）：不只熱的地方，844 處全改；寫錯的名稱編譯不過，之後也不會有一半字串、一半常數。不選只改熱的地方（原本的看法）：省下的機械式修改不多，混著兩種寫法以後還要再改。
4. **class 和 id 的值照舊是字串**（2026-10-13 原本決定也用 atom：7 bytes 以內 inline、更長的字串加算好的 hash，`RuleSet` 用存好的 hash 查桶；同一天做完第 3 段、量了之後改成不做）：和第 2 段比，Svelte-Complex-DOM、React-Complex-DOM 的指令數多了 0.4% 到 1.4%、cycles 多了 1% 到 4%。RFC 0039 的 `RuleSet` 合成一個之後，查桶已經不是成本，而每個元素設 class 時多算的 hash 比省下的多。把 hash 算得更快（一次 8 bytes、第一次查時才算）也只能省查桶那一小塊，不做。第 3 段的第 12 到 15 條跟著不用。
5. **`dom` 直接依賴 `markup5ever`（和 `string_cache`）**（2026-10-13，第 1 段開始前）：原本只有 `html` 依賴它們；兩個都是純資料、不碰 V8（`sanity.py deps` 管的是 V8 的位置）。
6. **`QualName` 的欄位改成私有**（同上）：建立一律經過建構函式（`new`、`html`、`attr`、帶 prefix 的那個），讀經過 `local()`、`namespace()`、`prefix()`；「DOM 裡不放動態 atom」的保證不會被直接寫欄位繞過。Blink 的 `QualifiedName` 也只能經過建構函式建立、不能改。原本 22 處直接寫 `QualName { … }`。
7. **`Hash` 照字串，比較先看表示**（同上）：同一個字串只有一種表示（7 bytes 以內 inline，更長的靜態或字串），所以兩邊都是 atom 就比整數、一邊 atom 一邊字串就一定不同、兩邊都是字串就比字串；`Hash` 照字串算，兩種表示一致，用 `&str` 查表也查得到。
8. **比較名稱的 API 收 atom**（2026-10-13，第 2 段開始前）：`is_html`、`is_svg`、`is_attr`、`NodeRef::attr_ref`、`has_attr` 的參數是 html5ever 的 atom（`&dom::name::LocalNameAtom`，寫成 `&local_name!("div")`），兩邊都是 atom 時比一個整數，名稱寫錯編譯不過；執行時才知道的名稱（script 給的）明確轉成 `LocalName::new` 再比。Blink 的 `HasTagName(html_names::kDivTag)`。不選同時收 `&str` 和 atom：寫錯的名稱不會被發現，和第 3 條的目的衝突。
9. **不在靜態清單裡、又超過 7 bytes 的名稱集中在 `dom::name` 的一個小表**（同上）：`local_name!` 寫不出來（編譯時就發現），用字串建一次。
10. **parser 直接搬 atom**（同上）：`html` 的 `qual_name` 用 `Name::from_atom`，不經過字串；`elem_name` 用 `to_atom()` 交回 html5ever（tree builder 很常呼叫它，原本每次從網址和名稱的字串轉 atom）。
11. **一千多處用 regex 包成 `local_name!`，再照編譯錯誤收尾**（同上）：名稱清單的陣列（例如表單元素的那幾個）改成 atom 的陣列（`local_name!` 是 const）。
12. **`RuleSet` 的 id 和 class 桶用 `hashbrown::HashTable`**（2026-10-13，第 3 段開始前）：拿元素存好的 hash 直接查，key 存小寫、用 `eq_ignore_ascii_case` 比，元素不必另存一份小寫的。`hashbrown` 是標準庫 `HashMap` 的底層，`Cargo.lock` 裡已經有；`css` 多一個直接依賴。Stylo 的 `PrecomputedHashMap`。不選標準庫的 `HashMap`：查的時候一定重算 hash。（第 3 段拿掉了，見第 4 條。）
13. **hash 是 ASCII 小寫的 FxHash（`rustc-hash`），函式在 `css`**（同上）：`css::rule_set::class_hash`，`dom` 存 class 時和 `RuleSet` 建桶時用同一個；`css` 不能依賴 `dom`。不選 SipHash：慢，而 key 都是頁面自己的 class，碰撞只拖慢那個頁面。（第 3 段拿掉了，見第 4 條。）
14. **`dom` 存名稱加 hash**（同上）：`id` 和每個 class 是 `ClassName`（第 1 段的 `Name`：7 bytes 以內 inline atom、更長的字串，加上小寫的 hash），第 4 條。`StyleElement::has_class` 照舊和 selector 的 `CssIdent` 比字串。（第 3 段拿掉了，見第 4 條。）
15. **用 `perf stat` 數指令，和第 2 段的狀態比**（同上）：看這一段單獨的效果，量 Complex-DOM（Svelte、React）。（第 3 段拿掉了，見第 4 條。）

## 待決定

無。

## 之後的可能

- 收 `&str` 名稱的小函式改收 atom，`match` 名稱的 5 處改成和常數比（「缺點」）。
- 不認得的長名稱也 intern（「理由和其他做法」的 C），如果量到 custom element 或 `data-*` 多的頁面慢在這裡。
- `css` 的 `CssIdent` 帶著 `LocalName` 的 atom（selector 的 tag 名稱），`has_local_name` 比整數；`RuleSet` 的 tag 桶用 atom 當 key。
- 祖先的 bloom filter（RFC 0039 的第 3 條）：要 class 和 id 的 hash；第 3 段的做法（`dom` 存 hash）量過、單獨做沒有收益，和 bloom filter 一起時再量。
