# RFC 0101 的進度：`createElement` 的成本往 Chrome 靠

開始日期：2026-11-15

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 第 1 段：接上預先建的 wrapper 不檢查、預先建 wrapper 不開 scope、realm 只問一次 | 做完 | 2026-11-15 |
| 第 2 段：名稱用 V8 字串的身分查（連同拿樣板物件時剩下的檢查） | 做完 | 2026-11-15 |
| 第 3 段：建節點（不動 elise 的兩項，決定 15） | 做完 | 2026-11-15 |
| 第 3 段的第三項：elise 的 `__init` inline（決定 16、17） | 做完 | 2026-11-15 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `createElement` 的指令、cycles、store forwarding 的衝突往下，其他成員不增加 | `bench.py calls --baseline rfcs/0098-direct-v8-bindings/calls-baseline.json` |
| callback 改到的幾段的條數 | gdb 單步（RFC 0097 的 `prototypes/step-callback.py`） |
| 四頁對 Chrome for Testing | `bench.py blink-perf --pages`，各三次 |

開始前：callback 1658 條；`createElement` 每次 2048 條、448 cycles（另一次量 412）、衝突 2.87。

## 做過的檢查

### 第 1 段

2026-11-15，Linux x86-64。

- **`NewWrapper`**：`bindings` 的 `Boilerplate`（樣板物件，只有 `instantiate` 做得出來、建的時候檢查一次它是 API wrapper 且沒接東西，物件不外流）、`BoilerplateLocal`（只能拿來複製）、`NewWrapper`（複製出來的，不能複製，`InfoNoGc::wrap_new` 收下、不檢查，debug build 仍檢查）。`dom_js` 預先建的 wrapper（`FrontTypes::PreparedNode`）和「替還沒有 wrapper 的節點當場建」（`wrap_node_here`）用它；其他建 wrapper 的地方照舊走有檢查的 `wrap`。
- gdb 單步：callback 1658 → 1573 條（−85：兩次 `IsApiWrapper`、一次 `IsWrapping` 和 `wrap` 的呼叫）。
- `cargo test -p dom_js -p bindings` 通過。

- **決定 7 和 8 對不上**：不開 scope 問 `this` 的 creation context，要 fork 多一個不用 scope 的入口，而決定 8 說這一段不動 fork。使用者選這一段就加（README 的決定 9）：fork 的 `Object::get_creation_context_in_callback`（`118f97d`，只改 Rust），pin 跟著更新。
- **不開 scope、realm 只問一次**：`prepare_new_node` 在狀態借著時問 `this` 的 creation context（`InfoNoGc::creation_context`）、和快取的樣板的 context 比，對得上就複製（`InfoHandles::new_wrapper`）；對不上才走原本開 scope 的路（`prepare_new_node_slowly`，第一次建樣板，或 `this` 是另一個 realm 的）。
- **結果**：

  | | 開始前 | 第 1 段之後 |
  | --- | ---: | ---: |
  | callback 本身（gdb 單步） | 1658 條 | 1496 條 |
  | `createElement` 每次（`bench.py calls`） | 2048 條、448 cycles（另一次量 412）、衝突 2.87 | 1843 條、377 cycles、衝突 3.16 |
  | `bindings/create-element`（對 Chrome for Testing，慢幾倍；三次） | 1.42（一次） | 1.24、1.26、1.28 |
  | `bindings/gc-forest` | 1.37 | 1.34、1.33、1.51 |
  | `bindings/gc-mini-tree` | 1.18 | 1.01、1.01、1.04 |
  | `bindings/gc-tree` | 1.05 | 1.01、0.99、1.02 |

  其他成員的指令數不變（`nodeType` 112、`getAttribute` 560、`appendChild` 1328、`list[i] (first)` 2290、`classList (first)` 2470；`nextSibling (first)` 1900 → 1868 是之前就有的差）。`bench.py calls` 的 −205 比單步的 −162 多，多的部分沒有解釋。`gc-forest` 三次裡一次 1.51，起伏大，看不出有沒有變。
- 單步裡這一段還剩的：拿樣板物件時的 `assert_access_allowed` 26、`Local::New` 18、`v8__Data__EQ` 14（決定 8：留到第 2 段和 fork 的其他改動一起）。
- **新的測試**：`info_front::a_new_nodes_wrapper_is_made_with_no_scope`（`createElement`、`createTextNode`、`createComment`、`createDocumentFragment`、`cloneNode` 在樣板已經有時 0 個 scope、各建一個 wrapper；realm 第一次建某個 interface 才開）、`frames::a_new_nodes_wrapper_is_of_its_documents_realm`（兩個 realm 的函式和 document 的十種組合，wrapper 都是 document 那個 realm 的）、`bindings` 的 `unified_heap::a_boilerplate_is_an_api_wrapper`。
- **mutation test**（`mutants-1.toml`，5 個）：4 個抓到；「樣板不必是 API wrapper」漏掉，補了 `bindings` 的測試後抓到。
- **WPT**：`dom/nodes/`、`custom-elements/`、`html/browsers/origin/`、`html/dom/`：661 個測試、79,064 個檢查，非預期的結果 0。
- **階段的檢查**：`cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、sanity 的各項通過。

### 第 2 段

2026-11-15，Linux x86-64。

- **做法**（README 的決定 10 到 13）：`dom_js` 的 `Caches::name_slots`（1024 格，V8 的 hash 選一格，記名稱的編號）；`front::name_by_identity` 讀參數的 hash、看那一格的名稱在名稱快取裡的字串和參數是不是同一個物件；`InfoFront::known_element_name` 認得出來時操作只拿到編號（`dom_api` 的 `ElementName::Known`），HTML 文件裡 `document::create_element` 直接照編號建，不借文字、不檢查合法、不找 `-`。認不出來的照文字查，查到的是 `is_plain_element_name` 的名稱（固定名稱表裡合法、沒有大寫、沒有 `-`）就登記（`front::remember_name`）：參數那個字串留作名稱快取裡那個名稱的字串。fork 的 `v8::machopo::internalized_hash`、`Local::is_same_object`、`Local::new_in_callback_unchecked`。
- **結果**：

  | | 第 1 段之後 | 第 2 段之後 |
  | --- | ---: | ---: |
  | callback 本身（gdb 單步） | 1496 條 | 1076 條 |
  | `createElement` 每次（`bench.py calls`） | 1843 條、377 cycles、衝突 3.16 | 1423 條、298 cycles、衝突 4.32 |
  | `bindings/create-element`（對 Chrome for Testing，慢幾倍；三次） | 1.24、1.26、1.28 | 0.99、1.00、0.99 |
  | `bindings/gc-forest` | 1.34、1.33、1.51 | 1.04、1.22、1.03 |
  | `bindings/gc-mini-tree` | 1.01、1.01、1.04 | 0.85、0.90、0.89 |
  | `bindings/gc-tree` | 1.01、0.99、1.02 | 0.81、0.87、0.67 |

  `create-element` 的分數：machopo 3562 runs/s，Chrome for Testing 3541。其他成員的指令數不變（`firstChild` 139、`nodeType` 112、`getAttribute` 560、`setAttribute` 1071、`appendChild` 1329、`classList (first)` 2473、`nextSibling (first)` 1865、`list[i] (first)` 2289）。`gc-forest` 和 `gc-tree` 各有一次離其他兩次遠（1.22、0.67），這兩頁的起伏沒有查。
- **store forwarding 的衝突變多（3.16 → 4.32）**：查了，不在改到的程式裡。單步一次 callback、找「讀的範圍和前面 150 條內的寫部分重疊」的讀取，第 1 段和第 2 段的 binary 是同一組（`Node::__init` 讀 `Cx::node` 分兩半寫的那一格，第 3 段的範圍；其餘三處是腳本把 1、2 byte 的寫算成 8 byte 的誤判）。用 `perf record -e ls_bad_status2.stli_other` 取樣：多出來的落在 V8 的 `ExternalEntityTable::TryAllocateEntry`（每次 0.86 → 1.7）和 `Factory::CopyJSObjectWithAllocationSite`（0.04 → 0.83），兩個函式都沒改。這個計數算的是「讀的時候前面的寫還沒完成」；推測是它們前面的路短了 420 條，寫還在路上的機會變多。這個推測沒有另外驗證。cycles 照指令的比例下降（−21% 對 −23%）。
- **兩個原有的測試的預期改了**（`strings::a_names_v8_string_is_made_once`、`a_new_string_is_made_with_no_scope`）：`createElement` 的參數字串現在留作名稱的字串，`localName` 第一次回傳時不另建（少一個 handle；`names_made` 在 `createElement` 時就加一）。
- **兩個落在同一格的名稱**輪流建時互相蓋掉，兩個都走照文字的路、每次多一次登記（決定 11 的代價）。固定名稱表裡 `is_plain_element_name` 的有 1259 個，全部各建兩輪，第二輪 364 個靠身分認到（獨佔一格的；照 1259 個丟進 1024 格算是約 369 個）。
- **新的測試**：`info_front::a_literal_element_name_is_told_by_its_strings_identity`（字面值第二次起靠身分；執行時接出來的字串、大寫、有 `-`、沒見過的名稱、不合法的、不是字串的都照文字，建出來的元素一樣；GC 之後仍然認得、字串還是同一個）、`info_front::names_that_share_a_slot_make_their_own_elements`、`frames::a_name_told_by_identity_makes_its_documents_element`（另一個 realm 的字面值是同一個字串、wrapper 是 document 那個 realm 的；XML 和 XHTML 的文件照文字建的結果）。靠身分認到的次數用 debug build 才有的計數看（`dom_js::names_by_identity_for_testing`）。fork 的 `internalized_hash` 在 debug build 和 `Name::GetIdentityHash` 比對。
- **mutation test**（`mutants-2.toml`，11 個）：11 個都抓到。
- **WPT**：`dom/nodes/`、`custom-elements/`、`html/dom/`：661 個測試、79,064 個檢查，非預期的結果 0。
- **階段的檢查**：`cargo test --workspace`（3900 個通過）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、sanity 的各項通過（`boundary` 的清單加了兩個 static：測試的計數、照固定名稱表算一次的 bitset）。fork 的 commit `ba468a0`，pin 跟著更新。

### 第 3 段

2026-11-15，Linux x86-64。只做不動 elise 的兩項（README 的決定 15）。

- **`make_element`**：`dom` 的 build script 多一張表 `MADE_WITH_MORE`（每個固定名稱一格：HTML 元素建立時除了節點還要做事的，template、iframe、base 和表單相關的名稱），`make_element` 在 HTML 的命名空間裡查一次，要做事的才進 `html_element_made`（原本那幾段，搬到一個不 inline 的函式）。
- **`QualifiedName::new`**：放得進 bits 的名稱 inline 直接組，要進表的搬到不 inline 的 `new_tabled`。
- **結果**：

  | | 第 2 段之後 | 第 3 段之後 |
  | --- | ---: | ---: |
  | `createElement` 每次（`bench.py calls`） | 1423 條、298 cycles、衝突 4.32 | 1353 條、286 cycles、衝突 4.15 |
  | `bindings/create-element`（對 Chrome for Testing，慢幾倍；三次） | 0.99、1.00、0.99 | 0.94、0.93、0.93 |
  | `bindings/gc-forest` | 1.04、1.22、1.03 | 1.04、1.03、1.15 |
  | `bindings/gc-mini-tree` | 0.85、0.90、0.89 | 0.81、0.82、0.80 |
  | `bindings/gc-tree` | 0.81、0.87、0.67 | 0.83、0.81、0.77 |

  `create-element` 的分數：machopo 3880 runs/s，Chrome for Testing 3624。其他成員的指令數不變。callback 本身這一段沒有再單步。
- **整份前後**（`bench.py calls` 對開始前的基準）：`createElement` 每次 2048 → 1353 條、448 → 286 cycles；`create-element` 對 Chrome for Testing 1.42 → 0.93。
- **測試**：沒有加新的；這兩項不改行為，原有的測試（template 的內容、表單的 owner 和狀態、iframe 和 base 的計數）守著。
- **mutation test**（`mutants-3.toml`，6 個）：6 個都抓到。「`made_with_more` 對每個名稱都是真」是等價的（它擋著的那一段自己再看一次名稱），沒有列。
- **WPT**：`dom/nodes/`、`custom-elements/`、`html/dom/`、`html/semantics/forms/`、`html/semantics/scripting-1/the-template-element/`：974 個測試、83,804 個檢查，非預期的結果 0。
- **階段的檢查**：`cargo test --workspace`（3900 個通過）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、sanity 的各項通過。

### 第 3 段的第三項

2026-11-15，Linux x86-64。

- **當時的選項**：（一）併進這份、另開一份、不做；（二）所有型別都 inline、加屬性只標指定的型別。使用者都選第一個（README 的決定 16、17）。
- **做法**：elise 的 `elise-derive` 產生的 `GcObject::__init` 標 `#[inline(always)]`，一行。不動 soundness 的論證（每個欄位照樣經過 `HeapField::__write`）。
- **結果**：

  | | 第 3 段的兩項之後 | 加上這一項 |
  | --- | ---: | ---: |
  | callback 本身（gdb 單步） | 沒有量（第 2 段之後 1076 條） | 945 條 |
  | 其中 `Cx::node` 加 `Node::__init` | 166 條 | 105 條 |
  | `createElement` 每次（`bench.py calls`） | 1353 條、286 cycles、衝突 4.15 | 1293 條、268 cycles、衝突 2.58 |
  | `bindings/create-element`（對 Chrome for Testing，慢幾倍；三次） | 0.94、0.93、0.93 | 0.88、0.88、0.88 |
  | `bindings/gc-forest` | 1.04、1.03、1.15 | 1.02、1.02、1.09 |
  | `bindings/gc-mini-tree` | 0.81、0.82、0.80 | 0.82、0.76、0.75 |
  | `bindings/gc-tree` | 0.83、0.81、0.77 | 0.83、0.77、0.81 |

  `create-element` 的分數：machopo 4071 runs/s，Chrome for Testing 3563。`Cx::node` 分兩半寫、`Node::__init` 整格讀的那一次轉發不了的讀取不見了。其他成員的指令數不變（`classList (first)` 2459 → 2450）。測試的 binary 的 text 64,862,326 → 64,843,246 bytes（前一個是第 2 段的 binary，差也含第 3 段的兩項）。
- **整份前後**：`createElement` 每次 2048 → 1293 條、448 → 268 cycles；`create-element` 對 Chrome for Testing 1.42 → 0.88。
- **沒量的**：`createTextNode`、`cloneNode`、parser 建樹（走同一個 `Cx::node`，`bench.py calls` 沒有這幾個成員）；欄位是動態值的型別 inline 之後各自的大小（只看了整個 binary）。
- **測試**：沒有新的（不改行為）；elise 把它記在「沒有計數守著的優化」，要守的話看 `createElement` 的指令數。
- **檢查**：elise 的 `cargo test --workspace`（315 個通過）、`cargo fmt --check`、clippy、`sanity.py` 通過；Miri 開始跑了、沒跑完就停掉（使用者決定：這個改動不碰 `unsafe` 的論證，而且 Miri 解譯 MIR、看不到 inline 的差別）；models、fuzz 沒跑。elise 的 commit `df8d8bb`，pin 跟著更新。machopo 的 `cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、sanity 的各項通過。
