# RFC 0070 的進度：HTML 元素的專屬 interface

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-21。完成：2026-10-22，macOS（Apple M4；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0070` 上做，第 1、2 段和第 3 段的 3a、3b、3c 各一個 commit，最後一起 merge；rusty_v8 fork 多了一個 commit（`Template::SetLazyDataProperty`，`9e83767`），發了 `kun-v152.2.0-15`，kun 收尾時換成它。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 interface object 用到才建（第 3 到 7、15 條）：fork 的 `Template::SetLazyDataProperty`，每個 context 記哪些接上了 parent，global 上的都用到才建 | 完成 | 2026-10-21 |
| 2 一般的 24 個、`WindowEventHandlers`、`<style>` 的 `disabled`、`<map>` 的 `areas`（第 1、2、8 到 10 條） | 完成 | 2026-10-21 |
| 3 table 的 6 個和畫出來的舊 attribute：3a `dom` 的 collection 存在以種類為 key 的表裡（第 11、13、14 條）；3b table 的 6 個（第 16 條）；3c presentational hint（第 12、17 到 19 條） | 完成 | 2026-10-22 |
| 收尾：kun 換成 `kun-v152.2.0-15` 的 V8 | 完成 | 2026-10-22 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| interface object | 建 realm 時只建它自己要的 interface object（`Window`、`EventTarget` 和 `glue.js` 讀的 `Node`、`Element`、`Performance`）；讀一個 interface 只建它和它的祖先；先建 wrapper（或讀 `window.constructor`）再讀 global 時 `[[Prototype]]` 照樣接好；global 上的 property 照 WebIDL（writable、configurable、不 enumerable），讀之前改寫或刪除不建任何東西 | `cargo test -p dom_js --test it -- interface_objects::` |
| | 另一個 realm 先讀的 interface object、legacy factory function、callback interface 的介面物件和 namespace 建在 global 自己的 realm | `cargo test -p dom_js --test it -- realms::` |
| | realm 建立的時間變短 | `realms::realm_creation_time`（`#[ignore]`，手動跑；下面「做過的檢查」） |
| 元素的 interface | 每個標籤是 HTML 的元素索引的 interface（kun 有行為的 30 個），parser 建的也是；kun 沒有行為的維持 `HTMLElement`；不能建構 | `cargo test -p dom_js --test it -- element_interfaces::`（預期值在 Chrome 152 上跑過同樣的 script）；`cargo test -p dom_api --test it -- api::`；WPT 的 `html/semantics/interfaces.html` |
| | 反映的成員照 HTML 和 Chrome（廢棄的也有）：字串、`[LegacyNullToEmptyString]`、boolean、`long` 和它的預設值、URL、clamped 的 `colSpan`、`rowSpan`、`span`、`scope` 的已知值 | `element_interfaces::`；WPT 的 `html/dom/reflection-*.html`（`grouping`、`text`、`tabular` 整份通過） |
| | `spec` 測試和 `html.idl` 比對；`kun:dom` 的 WIT 多 30 個 resource | `cargo test -p dom_api --test it -- spec:: wit::`（`spec-missing.txt`、`machopo-dom.wit`） |
| handler | window 和 `<body>` 有 `html.idl` 的 18 個 `WindowEventHandlers`，`<body>` 的是 window 的；kun 發的 `unhandledrejection` 呼叫 `onunhandledrejection` | `element_interfaces::`、`errors::onunhandledrejection_is_called`；WPT 的 `html/webappapis/scripting/events/` 的五份 |
| `<style>`、`<map>` | `disabled` 是 sheet 的旗標：disabled 的不套用，文字、`type` 改了或插入、移除時是新的 sheet、不是 disabled；沒有 sheet 時讀到 `false`、設了沒有作用 | `cargo test -p style --test it -- invalidation::a_disabled_style`；`element_interfaces::` |
| | `areas` 是後代的 `<area>`，live、每次同一個 | `element_interfaces::` |
| collection | 節點的 collection 照種類找，每次同一個的超過一個 chunk 也是；`getElementsBy*` 這類照名稱的在 script 拿著時同一個（照給的名稱分辨），放掉之後被收 | `cargo test -p dom --test it -- collections::`；`cargo test -p dom_js --test it -- bindings::named_collections` |
| table | caption 和 section 的 getter、setter、`create*`、`delete*`；`rows` 的順序、`tBodies`、`cells`；`insertRow`、`deleteRow`、`insertCell`、`deleteCell`；`rowIndex`、`sectionRowIndex`、`cellIndex` | `element_interfaces::`（Chrome 152 上跑過）；WPT 的 `html/semantics/tabular-data/` |
| presentational hint | 舊式顏色的解析；table 和它的各部分的 `bgcolor`、`background`、`<table bordercolor>`；`<body>` 的 `bgcolor`、`text`、`background`、margin；`<font>` 的 `color`、`face`、`size`；`<hr>` 照 Chrome | `cargo test -p style --test it -- legacy_hints:: tables:: invalidation::`（值在 Chrome 152 上量過）；`test.py reftest tables/t5-legacy-attributes.html backgrounds/b5-legacy-body.html text/t10-legacy-font.html block/hr-legacy.html`；WPT 的 `html/rendering/` |
| | `<body>` 的 `link`、`alink` 是文件的連結顏色（`-webkit-link`、`-webkit-activelink`），改了整份重算 | `legacy_hints::link_colors`、`invalidation::the_bodys_link_styles_everything_again` |
| 盤點和效能 | 盤點的「Elements without their interface」和這幾個成員消失；Speedometer 和開始前比 | `bench.py speedometer --inventory`、`bench.py speedometer`：照規定只在要求時跑，還沒跑 |

## 做過的檢查

**完成時（2026-10-22，macOS，Apple M4）**：每一段做完時各跑過 `cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（第 1 段 3485 個，第 2 段 3492，第 3 段 3515，都通過）和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`，那時連結本機重編的 V8（`RUSTY_V8_ARCHIVE`）；收尾換成 `kun-v152.2.0-15` 的 prebuilt V8 之後再跑一次（見最後）。fork 用它自己的 toolchain（1.91.0）`cargo check` 通過。中途 rebase 到 RFC 0069 之後的 `main`（它發了 `kun-v152.2.0-14`），fork 的改動搬到它上面、重編，相關的 8 個 crate 的測試都通過。

**realm 建立的時間**：`dom_js` 的 `realms::realm_creation_time`（一個 isolate 上先建一個 realm 讓 template 建好，再建 500 個取平均；debug 的測試 binary、release 的 V8）。改之前三次 678、661、655 µs，改之後 269、273、274 µs，約少 59%。

**每一段對照 Chrome 152 和 Blink 定下或改掉的**（細節在 README 的「已決定的事項」）：
- **interface object**：lazy getter 一開始在讀的那個 realm 裡建，另一個 realm 先讀 `other.Node` 時建錯 realm，改成 holder 的 creation context（Blink 的做法）。V8 建 context 時順便建了 global 自己的 `Window`，建 realm 時把它接上 `EventTarget`。
- **元素的 interface**：`element_interfaces` 的運算式都先在 Chrome 152 上跑過，只有刻意的 `<dialog>`（kun 沒有它的行為）不同。Chrome 152 的 `<body>` 沒有 `onpagereveal`、`onpageswap`，照 `html.idl` 留著；`<style>` 的 `type` 改了是新的 sheet，`media` 不是，照 Chrome；`colSpan` 這類超過 2³¹ 的值照 Chrome 夾到上限（`reflection-tabular.html` 抓到）。`spec` 的 `[CEReactions]` 比對抓到 table 的 `create*`、`insertRow`、`insertCell` 不該有它。
- **collection**：`getElementsBy*` 的物件身分照 Chrome 152 實測；為了分辨 `'a b'` 和 `'a  b'`，`ClassName` 多存給的字串。`EdgeChunks` 和 `WeakSet` 的 chunk 原本固定 64 格，改成格數是參數，collection 用 4 格的。
- **presentational hint**：值都用 `getComputedStyle` 在 Chrome 152 上量過。`<hr size="5px">` 是 3px、`<hr width=0>` 是 0px，`rightmargin`、`bottommargin` 沒有效果（第 19 條更正）。Chrome 152 的 cell 會繼承作者給 table 的 CSS `border-color`：那是 Chromium 的 bug（crbug.com/40745409），照 WPT 的 `table-border-color-not-inherited.html` 不繼承（kun 原本就通過它）；`tables::the_cell_hints` 原本的預期值照這樣。照 Chrome、和 WPT 不同的（`bordercolor` 讓邊框是實線、`<body>` 的 margin 後寫的贏）寫進 `.ini`。這個瀏覽器窗格（Electron 的 Chromium 152）的連結本身的預設顏色和一般 Chrome 不同，連結的顏色照 Blink 的原始碼，`-webkit-link` 的值照實測。

**WPT**：每一段跑過改到的目錄：interface object 那段跑了清單裡的 idlharness、`console/`、`html/browsers/the-window-object/`、`custom-elements/`、`dom/nodes/`、`dom/events/`；之後 `html/dom/`（reflection 的 2061 個 subtest 從失敗變成通過）、`html/semantics/`、`html/webappapis/scripting/events/`、`dom/nodes/`、`dom/collections/`、`css/css-tables/`、`html/rendering/`，都更新了預期結果。加進清單的：`html/semantics/interfaces.html`、`html/semantics/tabular-data/`、`html/webappapis/scripting/events/` 的五份、`html/rendering/` 的 `<hr>`、`<font>`、`<body>` 的 margin、table 的 `border`、`bordercolor` 那些；失敗的都寫了原因（kun 沒有行為的元素、XML parser、`HTMLCollection` 的 named property、`GlobalEventHandlers` 在 `Element` 上、照 Chrome 和規範不同的地方）。在 macOS 上意外通過的三個 scrollend 的 subtest 和這份無關：測試在 Mac 上直接結束，`.ini` 是照 CI 的 Linux 定的。kun 自己的 reftest 多了 4 份，整套 189 份都符合預期。

**mutation test**（`mutants-1.toml`、`mutants-2.toml`、`mutants-3a.toml`、`mutants-3b.toml`、`mutants-3c.toml`）：最後都全部抓到（10、17、9、19、24 個）。第一輪沒抓到的都是測試沒走到的情況：namespace 在讀的 realm 裡建、沒有 browsing context 的 document 的 shadow tree 中的 `<style>`、`<tfoot>` 還在時新的 `<tbody>` 的位置、section 不在 table 裡時的 `rowIndex`、row 自己有 `border-color` 時 table 的 `bordercolor` 改了；都補了測試，和 Chrome 對過。等價、不列的寫在各份 spec 的開頭；「table 的方法不經過 `rows` collection」是效能的捷徑，記進 `docs/test-gaps.md` 的「沒有計數守著的優化」。動到 GC 的部分（rare data 的 collection、chunk 的格數）merge 之後看 CI 的 `matrix`、`fuzz`。

**換成 prebuilt 的 V8 之後（2026-10-22，macOS，Apple M4）**：kun 的 `RUSTY_V8_MIRROR_TAG` 換成 `kun-v152.2.0-15`（fork 的 release workflow 編好 Windows、macOS、Linux 的 V8），`../third_party/rusty_v8` 由 `ci.py third-party` 切到它的 commit；不設 `RUSTY_V8_ARCHIVE`，`cargo build --workspace`、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（3515 個，都通過）和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs` 都通過。
