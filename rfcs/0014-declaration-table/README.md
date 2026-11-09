# RFC 0014：宣告表的拆檔

- 開始日期：2026-10-07
- 狀態：✅ 已完成：2026-10-07（macOS arm64；設計 2026-10-07 確認，「已決定的事項」第 1 到 6 條）
- 細節：`details.md`；進度：`progress.md`

## 摘要

把 `dom_api` 的宣告表（原本的 `interfaces.rs`，一個 `declare!` 宣告全部的 interface）照 Chromium 的目錄拆成好幾個檔，每個檔產生自己的清單，在編譯期接成同一個 `INTERFACES`；同時讓 interface 的方括號只寫最末端的物件種類（祖先的 `accepts` 沿著 parent 鏈推出），HTML 元素的 tag name 寫在宣告上（`element_kind` 從宣告推出）。script 看得到的行為不變。這是 RFC 0012 路線圖的 DOM 線「前置」：三條線開始平行加 API 之前，先讓它們不必改同一批地方。

## 動機

**現況**：RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）把 Speedometer 缺的功能分成 DOM、CSS、其他雜項三條線平行做，三條線都會加 interface。現在加一個 interface 要碰的共用地方：

| 共用的地方 | 為什麼會衝突 |
| --- | --- |
| `interfaces.rs` 只有一個 `declare!`（加上一個 `unions!`、`enums!`、`dictionaries!`） | 全部 interface 在同一個巨集呼叫裡，三條線都在同一個檔案的尾端加 |
| 祖先 interface 的方括號 | `EventTarget`、`Node`、`Element`、`HTMLElement` 的 `[...]` 列出所有實作它的物件種類；加一個 HTML 元素的 interface 要改這四處 |
| `html_element::element_kind` | 每種新元素在 tag name 的 `match` 加一行 |
| `crates/dom_api/src/lib.rs` 的 `pub use interfaces::{...}` | 逐項列出 |

`INTERFACES` 本身必須是一份 parent 在前的清單：`dom_js` 用它的索引當 template 的編號、照順序安裝（child 的 template 繼承 parent 的），`dom_wasm` 用它的索引當 host resource 的編號。

目標：
- 一條線加 interface 時，多半只改自己那個領域的檔案。
- 加一個 HTML 元素的 interface：宣告（含 tag name）、`ObjectKind` 一項、實作的函式，不再改祖先。
- `dom_js`、`dom_wasm` 不用改；script 看得到的行為不變。

不在範圍內：
- **新增 GC 型別的成本**：RFC 0010 第 8 條已經降低（第 4 條）。
- **WebIDL 的 `partial interface`**：多條線都會加成員的 interface（`Window`、`Document`、`Element`、`HTMLElement`）先在同一個宣告裡分區塊加（第 5 條）。
- 新的 API：這份只搬東西和改宣告的寫法。

## 設計概要

給之後加 API 的人：

```text
crates/dom_api/src/interfaces/
  mod.rs          把各檔的清單照順序接成 INTERFACES；pub use 各檔
  core_dom.rs     core/dom：EventTarget、Node、Document、Element、Attr、CharacterData……
  core_events.rs  core/dom/events 和 core/events：Event、UIEvent、MouseEvent、CustomEvent……
  core_html.rs    core/html：HTMLElement 和各種元素、HTMLCollection
  core_frame.rs   core/frame：Window
  core_xml.rs     core/xml：DOMParser
```

新的 HTML 元素：

```text
// core_html.rs
interface HTMLInputElement : HTMLElement [HtmlInputElement] tags("input") as NodeHandle {
    attribute value: DomString, get = ..., set = ...;
}
```

再在 `ObjectKind` 的 HTML 區塊加 `HtmlInputElement`。`EventTarget`、`Node`、`Element`、`HTMLElement` 的宣告、`element_kind` 都不用改。新的領域（CSSOM、geometry、forms……）各開一個檔，在 `mod.rs` 的清單加一行。

## 詳細設計

### 1. 檔案和順序

照 Blink 的目錄分（第 2 條），interface 和只有它用到的 dictionary、union、enum 放在同一個檔：

| 檔 | Blink 的目錄 | interface | dictionary、union、enum |
| --- | --- | --- | --- |
| `core_dom.rs` | `core/dom`（含 `core/dom/events/event_target.idl`） | `EventTarget`、`Node`、`Document`、`XMLDocument`、`MutationObserver`、`MutationRecord`、`AbortController`、`AbortSignal`、`NodeIterator`、`TreeWalker`、`DOMImplementation`、`DocumentFragment`、`DocumentType`、`Element`、`NodeList`、`DOMTokenList`、`NamedNodeMap`、`Attr`、`CharacterData`、`Text`、`CDATASection`、`ProcessingInstruction`、`Comment` | `NodeOrString`、`EventListenerOptions`、`AddEventListenerOptions`、`MutationObserverInit`、`GetRootNodeOptions`、`GetHTMLOptions` |
| `core_events.rs` | `core/dom/events`、`core/events` | `Event`、`UIEvent`、`MouseEvent`、`CustomEvent`、`ErrorEvent`、`PromiseRejectionEvent` | `EventInit` 和各個 init 字典 |
| `core_html.rs` | `core/html` | `HTMLElement`、`HTMLUnknownElement`、`HTMLMetaElement`、`HTMLTitleElement`、`HTMLIFrameElement`、`HTMLCollection` | — |
| `core_frame.rs` | `core/frame` | `Window` | — |
| `core_xml.rs` | `core/xml` | `DOMParser` | `DOMParserSupportedType` |

- `EventTarget` 是所有 event target 的根，放在 `core_dom.rs`（Blink 的 `core/dom/events/` 也在 `core/dom` 底下）；`Event` 和其他事件放在 `core_events.rs`，Blink 分在 `core/dom/events/` 和 `core/events/` 兩處，kun 的事件少，先併成一個檔。
- `HTMLCollection` 是 DOM 規範的 interface，Blink 放在 `core/html/html_collection.idl`，照 Blink 放 `core_html.rs`。`DOMParser` 是 DOM Parsing 規範的，Blink 放在 `core/xml/`，照 Blink 放 `core_xml.rs`（之後的 `XMLSerializer` 也在那裡）。
- **檔名**：Blink 的目錄名加上 `core_`（`core/dom` 是 `core_dom.rs`）。原本打算直接叫 `dom.rs`、`html.rs`，但 `dom` 和 `html` 也是 `dom_api` 依賴的 crate，模組同名時 `use dom::...` 會變成有歧義的路徑；之後 `modules/` 底下的目錄照樣寫成 `modules_*.rs`。
- **順序**：`mod.rs` 照 `core_dom → core_events → core_html → core_frame → core_xml` 接；檔內照現在的相對順序。每個 interface 的 parent 都在它之前的檔或同一個檔的前面。新的檔照依賴接在後面。加一個測試：`INTERFACES` 裡每個 interface 的 parent 都排在它前面。
- **接法**：`declare!` 產生的清單改名為檔內的 `DECLARED`（`pub(super)`），`mod.rs` 用一個 `const fn` 在編譯期把各檔的清單複製進一個陣列：`INTERFACES` 的型別（`&[&InterfaceDesc]`）不變，`dom_js`、`dom_wasm` 和測試都不用改。
- **跨檔的型別**：宣告裡寫的型別是其他 interface 的標記型別（`Option<Document>`），各檔 `use super::*` 拿到 `mod.rs` re-export 的全部名字。
- `lib.rs` 改成 `pub use interfaces::*`。
- **順序變了的影響**：`machopo-dom.wit` 照 `INTERFACES` 的順序寫 resource，會有一次只是搬位置的 diff；guest 照名稱匯入 resource，不用升 `kun:dom` 的版本：拆檔後用舊 WIT 編的 guest 照樣通過 `dom_wasm` 的測試（2026-10-07）。Lua 的 runtime 記著 WIT 文字的 hash（`dom_api` 的 `the_lua_runtime_is_built_from_this_wit`），所以 guest 照慣例用 `ci.py guests` 重編一次，二進位檔只有內嵌的 WIT 順序不同。`dom_api` 的 `the_table_describes_each_interface` 原本照順序列出全部 interface，改成照名稱排序比對，順序由新的 `parents_come_first` 守住，各線加 interface 時不必在同一份清單的同一處插入。`spec-missing.txt` 照名稱排序，不受影響；`spec-differences.txt` 是手寫的、照理由分組，不動。

### 2. `accepts` 從 parent 鏈推出

- **方括號只寫「最末端是這個 interface」的物件種類**：`EventTarget [EventTarget]`（`new EventTarget()` 的）、`Node []`、`Element [Element]`（不是 HTML 的元素）、`HTMLElement [HtmlElement]`、`CharacterData []`、`Text [Text]`、`Event [Event]`、`UIEvent [UiEvent]`……每個 `ObjectKind` 只出現在一個 interface 的方括號裡。
- **最末端的 interface**：`InterfaceDesc` 多一個欄位 `own_kinds`（方括號列的種類）；`interface_of(kind)` 查一張以 `ObjectKind` 為索引的表，表是 `const fn` 在編譯期從 `INTERFACES` 建的 `static`。一種 kind 被兩個 interface 列出、或沒有 interface 列出時，編譯失敗。原本打算第一次用時才建（`OnceLock`），但 renderer 這一側不能有整個 process 共用的可變 `static`（`sanity.py boundary`，`docs/architecture.md` 的「Process 的邊界」），編譯期建好的表不可變，也把檢查提前到編譯時。`ObjectKind` 由一個小巨集宣告，多一個 `ALL`（照宣告順序，`kind as usize` 是它的索引），表的大小和測試用它。測試：每個 `ObjectKind` 剛好出現在一個 interface 的方括號裡、parent 鏈上的 interface 接受它。
- **`accepts(kind)`**：`interface_of(kind)` 沿著 parent 鏈往上，碰到這個 interface 就是 true。最多走 depth 步（現在最深是 5：`EventTarget → Node → CharacterData → Text → CDATASection`），比的是 interface 的名稱。`check_this` 每次 API 呼叫都會走到；查表加幾次比較和現在的 `matches!` 同一個數量級，不另外建每個 interface 的位元表（之後量得出差別再改）。
- `InterfaceDesc::accepts` 的欄位型別（`fn(ObjectKind) -> bool`）不變，`dom_js` 的 `convert.rs`、`dom_wasm` 的 `host.rs` 照舊呼叫它。
- Chromium 的 `WrapperTypeInfo` 也是沿著 `parent_class` 判斷一個物件是不是某個 interface 的（`details.md` 的「Chromium 怎麼做」）。

### 3. tag name 寫在宣告上

- **語法**：方括號和 `as` 之間可以寫 `tags("meta")`、`tags("h1", "h2", ...)`。只能寫在方括號剛好一個 `ObjectKind`、而且是 HTML 元素的 interface 上（`HTMLHeadingElement` 一種 kind 對應六個 tag）。
- **`InterfaceDesc` 多一個欄位** `html_tags: &'static [&'static str]`；tag 對應的種類是 `own_kinds` 唯一的那一個。
- **`element_kind`**：HTML namespace 的元素先查一張 tag name 對應 `ObjectKind` 的表（`HashMap`，第一次用時從 `INTERFACES` 建的 `OnceLock`，登記在 `sanity.py boundary` 的允許清單，第 6 條），找不到再照現在的規則：HTML 定義的元素名稱（`HTML_ELEMENTS`）或合法的 custom element 名稱是 `HtmlElement`，其他是 `HtmlUnknownElement`。不是 HTML namespace 的照舊是 `Element`。
- 測試：同一個 tag 不出現在兩個 interface；寫了 `tags` 的 tag 都在 `HTML_ELEMENTS` 裡。
- 對應 Chromium 的 `core/html/html_tag_names.json5`（每個 tag 寫 `interfaceName`）和從它產生的 `HTMLElementFactory`。

### 4. 多條線都會加成員的 interface

`Window`、`Document`、`Element`、`HTMLElement` 的成員照規範分區塊，區塊前是註解（現在 `Element` 已經有 `// ParentNode.`、`// ChildNode.`）。一條線第一次在某個 interface 加某個規範的成員時，開一個新的區塊（`// CSSOM View`、`// HTML: history`），放在 interface 的尾端；不同線在不同區塊加，git 多半合得起來。不預先放空的區塊註解：沒有成員的註解描述的是還不存在的東西，範圍也還會變。

`ObjectKind` 照檔分區塊（DOM、events、HTML、frame、XML），各區塊前加註解，新的種類加在自己區塊的尾端。

### 測試

- 純重構：現有的測試全部通過（`cargo test --workspace`，包括 `dom_api` 的 `api`、`spec`、`wit`，`dom_js`、`dom_wasm` 和 WPT）就是回歸檢查；`machopo-dom.wit` 除了順序沒有變化（`MACHOPO_UPDATE_GOLDEN=1` 重新產生之後，排序過的 diff 是空的）。
- 新的測試：parent 在 child 之前；每個 `ObjectKind` 剛好有一個最末端的 interface；`accepts` 和舊的結果一樣（每個 interface 對每個 kind，和現在方括號列的完全相同，第 2 段先用舊的方括號產生預期的表再刪掉舊的寫法）；tag 不重複、都在 `HTML_ELEMENTS` 裡；`element_kind` 對現有的 tag 結果不變。
- mutation test：弄壞 `accepts` 的走法、`interface_of` 的表、tag 的表、`INTERFACES` 的接法，確認有測試失敗。
- 不動 GC、`unsafe`、視窗那一層，不跑 `matrix`、`fuzz`、`automation`。

### 分階段

1. **拆檔**：搬到 `interfaces/` 底下的五個檔、`mod.rs` 的接法、parent 在前的測試、`lib.rs` 的 `pub use`、重新產生 `machopo-dom.wit` 並確認 guest 不用升版；`ObjectKind` 照區塊加註解。
2. **`accepts` 從 parent 鏈推出**：方括號只留最末端的種類、`interface_of` 的表、`accepts` 的走法和測試。
3. **tag name 寫在宣告上**：`tags(...)`、`element_kind` 從宣告推出和測試。
4. **文件**：`crates/dom_api/README.md` 的「怎麼新增一個 API」、`declare!` 的說明、RFC 0012 路線圖的「開始平行之前」。

## 缺點

- 宣告分在五個檔，找一個 interface 要先知道它在哪個領域；`INTERFACES` 的順序由 `mod.rs` 和各檔一起決定，不再是一眼看得出的一份清單（由測試守住 parent 在前）。
- 祖先的 `accepts` 從「讀方括號就知道」變成要沿 parent 鏈推，`check_this` 多了查表和幾次比較。
- `declare!` 多一種語法（`tags`），`InterfaceDesc` 多兩個欄位。

## 理由和其他做法

- **照 Chromium 的目錄分，不照三條線分**（第 2 條）：線會換人、換範圍，規範和 Blink 的分界不會；照線分的話，線一調整檔案就要重分。
- **`accepts` 從 parent 鏈推**（第 3 條）：不推的話，表單一批（約 8 種元素）就要改祖先的方括號 8 次，三條線同時加時一定衝突。沒選「每個 interface 建一張位元表」：多一份要維護的結構，現在看不出 `check_this` 的時間有差。
- **tag name 寫在宣告上**（第 3 條）：不寫的話，每種新元素都要改 `element_kind` 的 `match`，和宣告分在兩個地方、容易漏。
- **新增 GC 型別的成本不再降低**（第 4 條）：見「已決定的事項」第 4 條和 `details.md` 的「新增 GC 型別的三個做法」。
- **不做 `partial interface`**（第 5 條）：要改 `declare!` 讓好幾個檔的成員併進同一個 `InterfaceDesc`，`members` 也要在編譯期接；現在三條線還沒開始，看不出區塊的做法不夠。
- **記在一份新的 RFC**（第 1 條）：原本路線圖建議記在 RFC 0010 第 2 階段的前置，RFC 0010 已經做完；記在 RFC 0012 的話，要在 `0012` 的 branch 上做，它很長，不容易很快併回 `main`。

## 先例

- **Blink**：IDL 檔照目錄放（`core/dom`、`core/html`、`core/frame`……），列在 `core_idl_files.gni` 等 build 檔裡；binding 由 IDL 編譯器產生，每個 interface 一個 `WrapperTypeInfo`，`parent_class` 指向 parent 的；HTML 元素的 tag 和 interface 的對應在 `html_tag_names.json5`。照做：目錄的分法、parent 鏈、tag 的對應寫在宣告的這一側。不照做：kun 沒有 IDL 編譯器，宣告表是 Rust 的巨集，所以接清單在 `mod.rs`，不在 build 檔。細節見 `details.md` 的「Chromium 怎麼做」。

## 已決定的事項

1. **宣告表的拆檔記在這份新的 RFC，在 branch `0014` 上做**（2026-10-07）：RFC 0012 路線圖的「開始平行之前」原本有三件事。第 1 件（RFC 0010 第 1 階段併進 `main`）已經完成，第 3 件見第 4 條，剩下的就是這份。不選「寫進 RFC 0012 的路線圖、在 `0012` 上做」：RFC 0012 很長，它的 branch 不容易短時間內併回去；也不選「RFC 0010 第 2 階段的前置」：RFC 0010 已經做完。
2. **照 Chromium 的目錄分檔**（2026-10-07）：`core_dom.rs`、`core_events.rs`、`core_html.rs`、`core_frame.rs`、`core_xml.rs`，之後新的領域各開一個檔（「詳細設計」第 1 節）。不選「照三條線分」：線一調整，檔案就要重分。
3. **拆檔時一起做：`accepts` 從 parent 鏈推出、tag name 寫在宣告上**（2026-10-07）：兩件都是為了讓加一個 HTML 元素的 interface 不用改祖先和 `element_kind`，表單一批就有約 8 種元素。不選「只拆檔」：拆完之後祖先的方括號和 `element_kind` 還是三條線共用的衝突點。
4. **新增 GC 型別的成本當成 RFC 0010 第 8 條已經處理，不再降低**（2026-10-07）：第 8 條之後，加一個 GC 型別是 `dom` 的 struct 和 `script_objects!` 一行，`dom_api` 的 `Object`、`ObjectKind` 各一項和編譯器會要求補上的幾個分支、`object_handle!` 一行，加上兩行測試；`dom_js`、`dom_wasm` 不用改。路線圖原本寫的成本（要改 `WrapTag`、wasm 的 slot）是第 8 條之前的。不選「`Object`、`fetch`、`visit`、`object_handle!` 從一份清單產生」：只省三、四行，多一層巨集；也不選「一般的 platform object 共用一個 `Object` 的變體和一個 wrap tag」（接近 Blink 的 `ScriptWrappable` 加 `WrapperTypeInfo`）：多一處「比對 tag 再轉型」的 `unsafe`，要寫 soundness、跑 `matrix` 和 `fuzz`；`Object` 的 `match` 不再列出每個型別，編譯器不再提醒補分支；共用一個 V8 的 wrap tag，V8 的 sandbox 現在在 unwrap 時做的型別檢查就要自己做。一族 interface 共用一個 GC 型別（所有節點是 `Node`、所有事件是 `Event`）的做法下，新的 GC 型別不多；真的要一次加一批時再評估（比較見 `details.md` 的「新增 GC 型別的三個做法」）。
5. **先不做 `partial interface`**（2026-10-07）：多條線都會加成員的 interface 在同一個宣告裡分區塊加（「詳細設計」第 4 節）；衝突真的變多時再支援（「之後的可能」）。

6. **tag 的表是 `OnceLock`，登記在 `sanity.py boundary` 的允許清單；`interface_of` 的表在編譯期建**（2026-10-07，第 3 步之後）：renderer 這一側不能有整個 process 共用的可變 `static`（`docs/architecture.md` 的「Process 的邊界」第 4 條），兩張表原本都打算用 `OnceLock`，被 `sanity.py boundary` 擋下。tag 的表從靜態的宣告表推出、建好之後不再改，每個 process 各算一份結果相同，和 `crates/dom_api/src/wit.rs` 的 `FUNCTIONS`、`TYPES` 一樣（允許清單的理由是同一句：從靜態的宣告表推出），所以照這個先例登記。`interface_of` 的表用 `const fn` 在編譯期建成不可變的 `static`，不用登記，而且一種 kind 被兩個 interface 列出、或沒有 interface 列出時編譯失敗。不選「tag 的表用 `thread_local!`」（第 3 步先這樣做過）：不用擴大允許清單，但每個 renderer thread 各建一份，寫法也和 `wit.rs` 不同；也不選「兩張表都用 `OnceLock` 加例外」：`interface_of` 的表在編譯期建得出來，少一項例外，還多了編譯期的檢查。

## 待決定

無。

## 之後的可能

- **`partial interface`**：衝突真的變多時，照 Blink 的做法，`partial interface Window` 寫在各規範的檔，產生時併進 `Window`。
- **`accepts` 的位元表**：量得出 `check_this` 的時間時，每個 interface 預先算好一個以 `ObjectKind` 為索引的位元表。
- **新增 GC 型別的成本**：一次要加一批 GC 型別時，重新評估第 4 條沒選的兩個做法。
