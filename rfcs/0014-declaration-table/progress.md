# RFC 0014 的進度：宣告表的拆檔

開始日期：2026-10-07。完成：2026-10-07（macOS arm64）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 拆檔 | 完成 | 2026-10-07 |
| 2. `accepts` 從 parent 鏈推出 | 完成 | 2026-10-07 |
| 3. tag name 寫在宣告上 | 完成 | 2026-10-07 |
| 4. 文件 | 完成 | 2026-10-07 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 拆檔 | `INTERFACES` 的 parent 都在 child 之前、名稱不重複 | `cargo test -p dom_api --test api parents_come_first` |
| 拆檔 | 拆檔後的 interface 和原本相同（照名稱比對） | `cargo test -p dom_api --test api the_table_describes_each_interface` |
| 拆檔 | `machopo-dom.wit` 只有順序改變 | 拆檔時用 `MACHOPO_UPDATE_GOLDEN=1` 重新產生，新舊兩份各自 `sort` 再 `diff` 是空的；之後由 `cargo test -p dom_api --test wit` 守住 |
| 拆檔 | 用拆檔前的 WIT 編的 guest 照樣能用（不升 `kun:dom` 的版本） | 重編 guest 之前跑 `cargo test -p dom_wasm` |
| `accepts` | 每個 `ObjectKind` 剛好被一個 interface 的方括號列出 | 編譯期的 `by_kind`（違反時編譯失敗）；`cargo test -p dom_api --test api each_kind_is_listed_once_and_accepted_up_its_chain` |
| `accepts` | parent 鏈上的 interface 接受底下的種類，其他不接受 | 同上一個測試；改寫前後每個 interface 對每種 kind 的 `accepts` 和 `interface_of` 逐項比對，完全相同（一次性的檢查，見「做過的檢查」） |
| tag | `tags(...)` 只在一種 kind 的 interface 上、tag 不重複、都是 HTML 定義的元素 | `cargo test -p dom_api --lib each_tag_has_one_interface` |
| tag | `element_kind` 對 `meta`、`title`、`iframe`、一般的、custom、未知的、其他 namespace 的元素結果不變 | `cargo test -p dom_api --test api elements_get_their_html_interface` |

## 做過的檢查

2026-10-07，macOS arm64。

- **改寫前後一致**：第 2 步改寫方括號前，用一個暫時的測試把每個 interface 對每種 `ObjectKind` 的 `accepts` 和每種 kind 的 `interface_of` 寫進檔案，改寫後再寫一次，`diff` 是空的（37 個 interface × 37 種 kind）；暫時的測試已刪掉。`machopo-dom.wit` 排序後和拆檔前相同。
- **guest**：用舊 WIT 編的 guest 通過 `dom_wasm` 的測試（20 個）。Lua 的 runtime 的 WIT hash 檢查（`the_lua_runtime_is_built_from_this_wit`）要求重編，`python3 scripts/ci.py guests` 重編了全部 guest，二進位檔只有內嵌的 WIT 順序不同。
- **設計的改變**：檔名原本是 `dom.rs` 等，和 crate `dom`、`html` 同名會讓 `use` 的路徑有歧義，改加 `core_`；`interface_of` 的表和 tag 的表原本打算用 `OnceLock`，`sanity.py boundary` 擋下（renderer 這一側不能有整個 process 共用的可變 `static`），`interface_of` 的表改成編譯期建的 `static`；tag 的表先改成 `thread_local!`，之後照 `wit.rs` 的先例改回 `OnceLock`、登記在允許清單（`README.md`「已決定的事項」第 6 條），改完重跑 tag 的兩個 mutation（`element_kind` 不查 tag、tag 對應成 `HtmlElement`），都抓到。
- **mutation test**（全部抓到，檔案已還原）：

  | 弄壞了什麼 | 哪個測試失敗 |
  | --- | --- |
  | `DECLARED` 拿掉 `core_frame` 的清單 | `the_table_describes_each_interface`、`each_kind_of_object_gets_its_most_derived_interface` |
  | `core_html` 排到 `core_dom` 前面 | `parents_come_first` |
  | `join` 不前進 `n`；`join` 跳過第一份清單 | `call_entries_convert_and_check`、`character_data` 等 |
  | `implements` 不往 parent 走；走完回傳 `true`；從 parent 開始走 | `call_entries_convert_and_check` 等；`each_kind_is_listed_once_and_accepted_up_its_chain`、`elements_get_their_html_interface` |
  | `by_kind` 不寫入表；`interface_of` 不看 kind | `call_entries_convert_and_check`、`character_data` 等 |
  | `Comment` 的方括號多列 `Text`；`Window` 的方括號清空 | 編譯失敗（`by_kind` 的 const 斷言） |
  | `element_kind` 不查 tag；tag 對應成 `HtmlElement`；巨集丟掉 `tags` | `elements_get_their_html_interface`、`meta_and_title_elements` |
  | `HTMLMetaElement` 多寫 `tags("title")` | `each_tag_has_one_interface` |

- **跑過的檢查**：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（1121 個通過，0 個失敗）、`sanity.py deps`、`boundary`、`unsafe`、`docs`。沒動到 GC、`unsafe` 和視窗那一層，不跑 `matrix`、`fuzz`、`automation`。
