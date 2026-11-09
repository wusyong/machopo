# RFC 0014 的細節：宣告表的拆檔

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## Chromium 怎麼做

- **IDL 檔的位置**：每個 interface 一個 `.idl`，放在實作它的 C++ 類別旁邊：`core/dom/node.idl`、`core/dom/events/event_target.idl`、`core/dom/events/event.idl`、`core/events/mouse_event.idl`、`core/html/html_collection.idl`、`core/xml/dom_parser.idl`、`core/frame/window.idl`。build 檔（`core/core_idl_files.gni` 等）列出全部的 `.idl`，IDL 編譯器一次讀進來、產生每個 interface 的 binding。Blink 也有共用的清單，只是在 build 檔裡。
- **型別資訊**：給 script 用的類別繼承 `ScriptWrappable`，類別裡寫 `DEFINE_WRAPPERTYPEINFO()`，由 virtual 的 `GetWrapperTypeInfo()` 回傳產生的 `WrapperTypeInfo`（例如 `v8_node.cc` 裡的）。`WrapperTypeInfo` 有 interface 名稱、`parent_class`、安裝 template 的函式；判斷一個物件是不是某個 interface 的，沿著 `parent_class` 往上找（`IsSubclass`），通過之後再 `static_cast` 成具體類別。kun 的 `InterfaceDesc::parent` 對應 `parent_class`，第 2 段的 `accepts` 走的就是這條鏈。
- **`partial interface`**：寫在別的規範的目錄（例如 CSSOM View 的 `partial interface Element` 在 `core/css/cssom/` 附近），IDL 編譯器把成員併進原本的 interface。kun 先不做（`README.md` 第 5 條）。
- **HTML 元素的 tag**：`core/html/html_tag_names.json5` 每個 tag 一項，寫它的 `interfaceName`（沒寫的是 `HTMLElement`）；build 時從它產生 `HTMLElementFactory`（tag 對應建構函式的表）和 `html_names`。kun 的 `tags(...)` 把同樣的資訊寫在 interface 的宣告上，`element_kind` 從宣告推出。

## 新增 GC 型別的三個做法

2026-10-07 比較的結果（`README.md` 第 4 條選 A）。

**現在加一個型別要改的地方**（RFC 0010 第 8 條之後；例子是 `TreeWalker`）：

| 位置 | 改什麼 | 漏寫的話 |
| --- | --- | --- |
| `dom` | struct、`script_objects!` 一行（產生 `ObjectTag`、handle、V8 的 wrap tag） | — |
| `dom_api` 的 `object.rs` | `Object` 一項、`ObjectKind` 一項、`kind` / `fetch` / `visit` / `realm_window` / `as_event_target` 各一個分支、`object_handle!` 一行 | 編譯失敗 |
| 測試 | `crates/dom/tests/script_objects.rs` 和 `crates/dom/tests/script.rs` 的 `every_type_unwraps_only_as_itself` 各一行 | 測試守不住 |

**Blink 加一個型別要改的地方**：C++ 類別（繼承 `ScriptWrappable`、`DEFINE_WRAPPERTYPEINFO()`、`Trace`）、`.idl`、build 檔的清單。和 Blink 比，kun 多的是 `Object` 的列舉：Rust 沒有繼承和動態轉型，`dom_api` 的值要能說出是哪一種，換到的是 `match` 的完整性由編譯器檢查。

| | A. 當成已經處理（選這個） | B. `Object` 從一份清單產生 | C. 一般的 platform object 共用一個變體 |
| --- | --- | --- | --- |
| 做什麼 | 不改 | `dom_api` 用一個巨集清單產生 `Object` 的列舉、`fetch`、`visit`、`object_handle!`；`kind`、`realm_window`、`as_event_target` 帶語意，照舊手寫 | `Object` 只留 `Node`、`Window`、`Event` 等幾種，其他的是 `Platform(PlatformHandle)`：不帶型別的 root 加 tag，用的時候比對 tag 再轉型（`downcast::<TreeWalker>()`）；V8 這一側共用一個 wrap tag |
| 每個新型別 | 約 11 處，每處一行 | 約 8 處 | 約 5 處（`ObjectKind` 照樣每個型別一項，interface 由它決定） |
| 好處 | 不花時間；編譯器要求補齊每個分支 | 少三、四處；不碰 `unsafe` | 最接近 Blink；`Object` 不再隨型別變長 |
| 壞處 | 幾條線同時加型別時，`object.rs` 的列舉尾端會衝突 | 省得不多，多一層巨集 | 多一處 `unsafe`，要寫 `soundness.md`、跑 `matrix` 和 `fuzz`；`match` 不再列出每個型別，編譯器不提醒；V8 的 sandbox 在 unwrap 時的型別檢查（每個型別一個 tag）要改成自己做，或照 Blink 排 tag 的區間 |
| 大小 | 0 | S | M，而且動到 GC |

C 有用的情況是一次加很多 GC 型別（SVG 的 DOM、CSSOM 的各種 rule）；但 kun 的做法是一族 interface 共用一個 GC 型別、用種類分 interface（`Node`、`Event`、`EventTargetImpl` 的 `AbortSignal`），照這個做法新的 GC 型別不多。

## 參考資料

- Blink 的 `third_party/blink/renderer/core/html/html_tag_names.json5`、`third_party/blink/renderer/platform/bindings/wrapper_type_info.h`、`third_party/blink/renderer/core/core_idl_files.gni`。
