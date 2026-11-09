# RFC 0079：Window 的具名屬性（named access on the Window object）

- 開始日期：2026-10-23
- 狀態：✅ 已完成：2026-10-23（驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`（量測：表的成本、`getElementById`、查全域名稱；Chrome 152 的探測）

## 摘要

照 HTML 的「named access on the Window object」和 Blink 的 `WindowProperties`：有 `id` 的 HTML 元素、有 `name` 的 `<embed>`、`<form>`、`<img>`、`<object>`，和有名稱的子 frame，在它們的 document 的 window 上成為具名屬性（`<div id=box>` 讓 `box` 和 `window.box` 是那個元素）。屬性放在 WebIDL 的 named properties object（`Window.prototype` 和 `EventTarget.prototype` 之間）上，用 kun 現有的 named getter interceptor；查名稱用 `dom` 為每份 document 維護的 id 和 name 的表，不走過整份 document。

## 動機

**開始前**（當時 `docs/limitations.md` 的「Bindings 和 DOM API」和「iframe」各一條）：window 沒有具名屬性，`<iframe id="x">` 不會讓 `x` 成為全域名稱。

**問題**：

- **WPT**：很多測試直接用元素的 `id` 當全域變數（`focused.focus()`、`table1.innerHTML = …`），在第一步就丟 `ReferenceError`，測不到它要測的東西。`.ini` 的註解寫明是它的至少 32 份（`css/css-scroll-anchoring/` 16 份、`css/` 的其他目錄、`html/semantics` 等），`shadow-dom/` 的 slot 測試多數也是；`dom/abort` 的兩個測試換成 `getElementById` 之後就通過。
- **網站**：舊的、手寫的頁面和一些範例程式靠它（`<form name=f>` 之後的 `f.submit()`、`<iframe name=frame>` 之後的 `frame.location`）。
- **`getElementById` 每次走過整份 document**（`dom_api` 的 `document::get_element_by_id`）：這份 RFC 要的 id 的表也讓它變成查表。

**目標**：

- 照 HTML 的「supported property names」和 getter：子 frame 的名稱優先（回傳它的 window）；否則有那個名稱的 named object 只有一個時是它，多個時是一個 live 的 `HTMLCollection`（document 順序）。
- 照 WebIDL 的 `[Global]` 和 `[LegacyUnenumerableNamedProperties]`：屬性在 named properties object 上（`Object.getPrototypeOf(Window.prototype)`，`[object WindowProperties]`），不可列舉；window 自己的屬性、`Window.prototype` 的成員和 `var` 宣告的全域變數蓋過它；不能定義、刪除（WebIDL 的 named properties object 的 `[[DefineOwnProperty]]`、`[[Delete]]`）。
- 查一個名稱不走過整份 document：每份 document 一張 id 的表和一張 name 的表，跟著插入、移除和 `id`、`name` 屬性的改變更新。
- `getElementById` 用同一張 id 的表。
- 把相關的 WPT 的預期結果更新。

**要解決的 `docs/limitations.md` 的限制**：「Bindings 和 DOM API」的「沒有 Window 的具名屬性」、「iframe」的「window 沒有 named property」。

**不在範圍內**：

- `Document` 的具名屬性（HTML 的「`Document` 的 named properties」：`document.f` 是名稱是 `f` 的 `<form>`、`<img>` 等，`[LegacyOverrideBuiltIns]`）：同樣的 name 的表可以用，之後另外做。
- 跨源的子 frame 的名稱規則（HTML 的 `WindowProxy` 的 `[[GetOwnProperty]]` 對跨源的具名存取）：kun 的跨源 window 什麼都碰不了（`docs/limitations.md` 的「iframe」）。
- `contentWindow` 不是 `WindowProxy` 的問題（換了 document 之後拿著的是舊的 window）：照現況，回傳子 frame 現在的 window。

## 設計概要

```html
<div id="box"></div>
<form name="login"></form>
<img name="logo"><img name="logo">
<iframe name="side" src="side.html"></iframe>
<script>
  box                     // 那個 <div>（window.box 也是）
  login                   // 那個 <form>
  logo                    // HTMLCollection [<img>, <img>]，live
  side                    // 子 frame 的 window（不是 <iframe> 元素）
  var box = 1;            // 宣告的全域變數蓋過它：box 是 1
  window.alert            // 還是 Window.prototype 的 alert，即使有 id="alert" 的元素
  Object.keys(window).includes("box")   // false：不可列舉
  "box" in window         // true
</script>
```

之後插入、移除元素，或改它的 `id`、`name`，下一次讀名稱就是新的結果（沒有快取的舊值）。

## 詳細設計

### 1. 名稱的表（`dom` 的 `tree_scope.rs`）

- 每個 tree scope（document 和每個 shadow root，只算它的 light tree）一張 `id` → 元素的表（任何元素），document 再一張 `name` → 元素的表（HTML 的 `<embed>`、`<form>`、`<img>`、`<object>`）和一張 iframe 的 `name` 的表（第 2、6 條），對應 Blink 的 `TreeScope` 的 `elements_by_id_`（`DocumentOrderedMap`）和 `HTMLDocument` 的 named item 的計數；在 heap 外（`DomState::tree_scopes`），以 `NodeId` 存（第 4 條）。名稱不空才收。
- 照 `DocumentOrderedMap`：一個名稱只有一個元素時存它的 `NodeId`；多個時存數量，第一個（tree order）在要的時候走樹找，記到那個名稱再有元素進出（兩個元素的先後只在其中一個移動時變，移動一定先拿出再放回；第 3 條）。
- 更新的時機：插入、移除共用的子樹走訪（`walk_moved_subtree`，light tree 的每個節點，不另外走樹），和 `attributes_changed`（`id`、named element 和 iframe 的 `name`：先照舊的拿出、再照新的放回；name 和 iframe 的表另外記每個元素放進去時的名稱，改的時候屬性已經是新的）。
- 表變空不拿掉（同一個 scope 常有元素再進來）；被回收的 scope 的表在加新的 scope、表數到上次留下的兩倍時拿掉（和 slot assignment 一樣）。
- 查詢：`DomReadScope::element_by_id`（`getElementById`）、`first_named_element`、`window_named`（下一節）；debug build 每次都和走樹的結果比對。

### 2. Named properties object（`dom_js` 的 `window_properties.rs`）

- 每個 realm 一個普通的物件，`@@toStringTag` 是 `WindowProperties`，prototype 是 `EventTarget.prototype`，`Window.prototype` 的 prototype 改成它（WebIDL 的「named properties object」，Blink 的 `WindowProperties`）；建 realm 時（`set_up_realm`）接上，在任何 script 之前。
- V8 的 non-masking named interceptor（只攔字串）：只在物件和它的 prototype chain 都沒有那個名稱時被問到（WebIDL 的 named property visibility：`EventTarget.prototype`、`Object.prototype` 的名稱也優先），所以 window 自己的屬性、`Window.prototype` 的成員、`var` 宣告和 `window.x = …` 建的屬性都蓋過它。
  - getter：`dom_api::window::named_property`（下面）。沒有 query：V8 對只有 getter 的 interceptor 的屬性是 `DONT_ENUM`，正好是 writable、configurable、不可列舉；沒有 enumerator，不在 own keys（`[LegacyUnenumerableNamedProperties]`）。
  - definer 一律失敗：在它上面 `defineProperty` 丟 `TypeError`；V8 對沒有 setter 的 interceptor，設定也走 definer，所以設定（strict mode）也丟（WebIDL 的 [[DefineOwnProperty]]）。
  - deleter：named property 刪除失敗（`false`，strict mode 丟 `TypeError`），其他名稱照常。
- `dom_api::window::named_property`（Blink 的 `WindowProperties::AnonymousNamedGetter`），照 `dom` 的 `window_named`：
  1. iframe 的表裡第一個（tree order）有 content document 的 iframe：它的 window（子 frame 優先，第 5 條）。
  2. 否則 id 和 name 的表：都沒有就不攔（照常找下去，最後是 `ReferenceError` 或 `undefined`）；只有一個元素時是它。
  3. 多個時是 live 的 `HTMLCollection`（`CollectionFilter::WindowNamed`：document 的 light tree 裡 `id` 是它的元素和 `name` 是它的 named element，tree order），在 `named_collections` 的弱快取裡，script 拿著時同一個（第 7 條）；只剩一個時是它。

### 3. `getElementById`

- `Document` 和 `ShadowRoot` 的 `getElementById` 查它自己的 id 的表；沒有表的（沒連到 document 也不在 shadow root 裡的 `DocumentFragment`）照舊走樹。

### 4. iframe 的 `name`

- `HTMLIFrameElement.name` reflect 屬性。kun 沒有 `window.name`，子 frame 的名稱一直是 iframe 的 `name` 屬性。

### 測試

- `dom`：`tests/it/tree_scopes.rs`，表在插入、移除、搬移、`id` 和 `name` 改變、shadow tree（有 window 的 document 也是）、同名的元素時的內容；走樹的次數、清表的次數（`dom::testing::tree_scope_walks`、`tree_scope_prunes`）；debug build 每次查都和走樹的結果比對。
- `dom_js`：`tests/it/window_named.rs`（名稱、跟著 DOM 改變、named properties object 的形狀）和 `iframes.rs` 的子 frame 的名稱，預期值是 Chrome 152 的。
- WPT：`html/browsers/the-window-object/named-access-on-the-window-object/` 加進清單；之前因為它失敗的重跑、更新預期結果和註解。
- mutation test：`mutants-1.toml`、`mutants-2.toml`。

### 分階段

1. `dom` 的 id 和 name 的表，`getElementById` 改用它。
2. named properties object 和 getter；WPT 的預期結果。

## 缺點

- 每次插入、移除元素和改 `id`、`name` 都多更新一次表（Blink 一樣）：插入或移除一個有 `id` 的元素多約 80 ns，改連上的元素的 `id` 多約 70 ns（`details.md`）；記憶體多三張表。
- 全域名稱查不到時（`typeof Foo`、`window.Foo` 這類功能偵測）多一次 interceptor 的呼叫和查表（Blink 一樣）：110–330 ns，Chrome 95–200 ns。debug build 每次查都走樹比對，大的頁面在 debug build 裡會慢。
- 頁面上的 `id` 會蓋過還沒宣告的全域名稱，和現在不同：依賴「沒有這個全域名稱」的程式在 kun 上的行為會改變（和 Chrome 一致）。

## 理由和其他做法

- **每次查名稱就走過整份 document**（不維護表）：程式最少，但查不到的名稱（最常見的情況）每次都走完整棵樹，大頁面上每次功能偵測都是 O(n)。不選。
- **在 DOM 版本變了時重建整張表**（`dom_tree_version` 之類的快取）：比維護表簡單，但框架常在改 DOM 和讀全域名稱之間交替，每次都整張重建。2026-10-23 的看法：照 Blink 維護表（第 1 節），見「待決定」第 1 條。
- **屬性放在 global 物件自己身上**（interceptor 在 global 的 template 上）：比較好接，但 window 自己的屬性和 `Window.prototype` 的成員的優先順序會錯（WebIDL 是 named properties object 在 prototype chain 裡），`Object.getPrototypeOf(Window.prototype)` 也不對。不選。

## 先例

- **Chromium**：`WindowProperties`（`core/frame/window_properties.cc`：`AnonymousNamedGetter` 先找子 frame 的名稱，再用 `HTMLDocument::HasNamedItem`、`TreeScope::HasElementWithId` 查表，一個時回傳元素、多個時 `WindowNameCollection`），`V8Window` 的 named properties object；`TreeScope::elements_by_id_`（`DocumentOrderedMap`）、`HTMLDocument` 的 named item 的計數，在 `Element::UpdateId`、`UpdateName`、插入和移除時更新。
- **規範**：HTML 的「Named access on the Window object」；WebIDL 的「Named properties objects」、`[LegacyUnenumerableNamedProperties]`。

## 已決定的事項

2026-10-23 確認第 1 段的事（使用者：「都照建議」；量測見 `details.md`）：

1. **名稱的表跟著每次改變更新**（Blink 的做法）：元素連上、離開它的樹，或 `id`、`name` 改變時，改那個名稱的那一筆。不選的做法：DOM 改變時整張重建、查的時候才建（框架常在改 DOM 和讀全域名稱之間交替，每次都整棵走，10,000 個元素約 0.3 ms）；不建表、每次查都走樹（現在的 `getElementById`：10,000 個元素時一次 0.32 ms，Chrome 50 ns）。理由：多的成本是每次改動一次 hash map 的查和改（約 30–60 ns：插入或移除約多 2–4%，改連上的元素的 `id` 約多 15–25%，Chrome 在這裡也多約 50 ns），查是 O(1)。
2. **每個 tree scope 一張表**（document 和每個 shadow root，Blink 的 `TreeScope`）：元素進它的樹根的表，`ShadowRoot.getElementById` 也查表。不選的做法：只有 document 的（shadow root 照舊走樹）。
3. **同名的第一個照 Blink 的 `DocumentOrderedMap`**：只有一個時直接存它；多個時存數量，要的時候走樹找第一個（tree order），快取到樹再改變。不選的做法：每個名稱存所有元素、要的時候比樹的位置（每次比較 O(深度)，程式多）。理由：重複的名稱少見，走樹只落在那裡。
4. **表裡存 `NodeId`**：在表裡的元素連在樹上、本來就活著，離開樹時拿掉那一筆，GC 不多追蹤東西，不動 `crates/dom/soundness.md`。不選的做法：存 GC 的 handle（要讓 GC 追蹤表）。debug build 每次查都和走樹的結果比對（照 subtree filter 的做法）。

2026-10-23 確認第 2 段的事（使用者：「都照建議」；Chrome 152 的探測見 `details.md`）：

5. **名稱照 Chrome 152**：任何元素的 `id`（SVG 的也是；規範只算 HTML 元素的），HTML 的 `embed`、`form`、`img`、`object` 的 `name`，有 `name` 的 iframe 指向它的子 frame 的 window、優先於同名的元素；不在 shadow tree、名稱不空。`<img>` 和 `<object>` 沒有另外的 `id` 規則（任何元素的 `id` 本來就算）。kun 沒有 `window.name`，子 frame 的名稱是 iframe 的 `name` 屬性。不選的做法：照規範只算 HTML 元素的 `id`。
6. **子 frame 的名稱也查表**：document 多一張 iframe 的 `name` 的表（和第 1 段的表一樣維護），查到的第一個有 content document 的 iframe 的 window。不選的做法：每次走樹找 iframe（`child_documents` 的做法：每次查不到的全域名稱都 O(n)）。Blink 列舉子 frame（`Tree().ScopedChild`），kun 沒有每份 document 的子 frame 清單。
7. **多個時是 live 的 `HTMLCollection`，script 拿著時同一個**：一種新的 `CollectionFilter`（document 的 light tree 裡 `id` 或 named element 的 `name` 是那個名稱的元素，tree order），在 `named_collections` 的弱快取裡（Blink 的 `WindowNameCollection`；Chrome 的 `window.dup === window.dup` 是 true）。不選的做法：每次新建。
8. **named properties object 照 Chrome 和 WebIDL**：屬性描述 writable、不可列舉、configurable，`getOwnPropertyNames` 看不到；`delete` 回傳 false（strict mode 丟 `TypeError`）；在它上面 `defineProperty` 或設定屬性一律丟 `TypeError`。用 V8 interceptor 的 descriptor、deleter、definer、setter。不選的做法：只做 getter（設定和 `defineProperty` 會建一般的屬性，和 Chrome 不同）。

## 待決定

無。

## 之後的可能

- `Document` 的具名屬性（`document.f`），用同一張 name 的表。
- `HTMLFormElement` 的具名屬性（`form.username`）。
