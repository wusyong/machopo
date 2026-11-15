# RFC 0101 的進度：`createElement` 的成本往 Chrome 靠

開始日期：2026-11-15

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 第 1 段：接上預先建的 wrapper 不檢查、預先建 wrapper 不開 scope、realm 只問一次 | 做完 | 2026-11-15 |
| 第 2 段：名稱用 V8 字串的身分查（連同拿樣板物件時剩下的檢查） | 未開始 | |
| 第 3 段：建節點 | 未開始 | |

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
