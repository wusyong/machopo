# RFC 0070：HTML 元素的專屬 interface

- 開始日期：2026-10-21
- 狀態：✅ 已完成：2026-10-22
- 進度和驗收：`progress.md`
- 細節：`details.md`（每個 interface 的成員、標籤和 Speedometer 用到的；kun 現有的 30 個）

## 摘要

kun 原本有 30 個 HTML 元素的 interface（表單、`<a>`、`<img>`、`<iframe>`、`<canvas>`……），其他元素一律是 `HTMLElement`：`document.createElement("div").constructor.name` 是 `HTMLElement`，Chrome 是 `HTMLDivElement`。Speedometer 3.1 的 13 個 default suite 用到其中 11 個（`div`、`span`、`p`、`h1`–`h4`、`li`、`ul`、`br`、`blockquote`、`style`、`td`、`tr`）。這份 RFC 照 HTML 的元素表補上 kun 有對應行為的元素的 interface 和它們的成員：一般的 24 個（多數是反映 content attribute 的 IDL attribute，廢棄的也有）和 table 的 6 個（HTML 的演算法），`<body>` 和 window 的 `WindowEventHandlers`，`<style>` 的 `disabled`、`<map>` 的 `areas`；反映的舊 attribute 照 Chrome 畫出來（`bgcolor`、`<body>` 的顏色和連結顏色、`<font>`、`<hr>`……）。先把 global 上的 interface object 改成用到才建（建一個 realm 從約 660 µs 到約 270 µs），`dom` 的 collection 照 Blink 存在一張照種類找的表裡（`getElementsBy*` 在 script 拿著時回傳同一個物件）。kun 還沒有行為的（影音、`<dialog>`、`<details>`、`<object>`、`<embed>`、frame、`<marquee>`）留給做那些功能的 RFC。

## 動機

2026-10-21 的盤點（`bench.py speedometer --inventory`，`b80ebbc3`）在 RFC 0068 補完 CSS 之後，剩下三類：元素的 interface（這份）、Web API、選取和游標的 CSS。元素的 interface 影響最廣：

| 元素 | Chrome 的 interface | default suite |
| --- | --- | --- |
| `div` | `HTMLDivElement` | 13（TodoMVC 的大多數、NewsSite、Editor、Stockcharts、Perf-Dashboard） |
| `li`、`ul` | `HTMLLIElement`、`HTMLUListElement` | 11 |
| `span`、`h1` | `HTMLSpanElement`、`HTMLHeadingElement` | 9 |
| `h2`–`h4`、`p` | `HTMLHeadingElement`、`HTMLParagraphElement` | 3（NewsSite、TipTap） |
| `br`、`style` | `HTMLBRElement`、`HTMLStyleElement` | 2–3（Editor、Perf-Dashboard） |
| `blockquote` | `HTMLQuoteElement` | 1（TipTap） |
| `td`、`tr` | `HTMLTableCellElement`、`HTMLTableRowElement` | 1（Perf-Dashboard） |

盤點另外列出這些 interface 的成員：`HTMLLIElement.type`、`HTMLUListElement.type`（Backbone、jQuery）、`HTMLBodyElement.onfocus`（jQuery）。

缺 interface 的影響：

- **行為不同**：`el instanceof HTMLDivElement`、`el.constructor === HTMLLIElement`、`Object.getPrototypeOf(el)` 這類判斷和 Chrome 不同；框架讀 `li.value`、`ol.start`、`td.colSpan`、`tr.rowIndex` 時拿到 `undefined`。
- **量測失真**：wrapper 的 prototype 鏈、V8 的 inline cache 看到的形狀和 Chrome 不同。

目標：

- HTML 的元素表裡、kun 有對應行為的元素，`createElement` 和 parser 建出的元素是它的 interface，interface object 在 window 上（`window.HTMLDivElement`），`instanceof`、`constructor.name`、`Object.prototype.toString` 照 Chrome。
- 它們的成員照 HTML 的 WebIDL（WPT 的 `interfaces/html.idl`），包括 Chrome 有的廢棄成員（`align`、`compact` 這類）：反映 content attribute 的照 HTML 的反映規則；table 的照 HTML 的演算法。
- `HTMLBodyElement` 有 `WindowEventHandlers`（`onhashchange`、`onpopstate`……），和 window 的同一組 handler。
- 反映的舊 attribute 照 Chrome 畫出來（presentational hint）：kun 還沒有的 `bgcolor`、`<body>` 的 `text`、`link`、`vlink`、`alink`、`background`、`<font>`、`<hr>` 的（已決定的事項第 12 條）。
- `dom` 的 live collection 照 Blink 存在一張以種類為 key 的表裡（第 11 條）。

**不在範圍內**：

- kun 還沒有行為的元素的 interface：`<video>`、`<audio>`（`HTMLMediaElement`）、`<track>`、`<dialog>`、`<details>`、`<object>`、`<embed>`、`<frame>`、`<frameset>`、`<marquee>`。它們維持 `HTMLElement`，等做那些功能的 RFC：interface 一出現，頁面就以為有它的行為（`'play' in video`）。
- `HTMLStyleElement.sheet`、`document.styleSheets`：kun 沒有 document 的 stylesheet 的 CSSOM（只有 constructed stylesheet），另一份 RFC。
- customized built-in element（`is`、`define` 的 `extends`）：kun 沒有（`docs/limitations.md`），新的 interface 和現有的一樣是 illegal constructor。
- 其他 Web API（`XMLHttpRequest`、`MouseEvent.fromElement`、`PerformanceObserver`……）。

要解決的 `docs/limitations.md` 裡的限制：

- 「Bindings 和 DOM API」的「HTML 元素的 interface 只有……」那一條的「其他元素是 `HTMLElement`，例如 `<div>` 不是 `HTMLDivElement`」：改寫成剩下沒有 interface 的那幾個（「不在範圍內」的第一項）。

## 設計概要

網頁的作者看到的是：

```js
document.createElement("div") instanceof HTMLDivElement;   // true（現在沒有 HTMLDivElement）
document.querySelector("li").value = 3;                    // 設 value="3"
list.type;                                                  // ul 的 type attribute
table.rows[0].cells[1].cellIndex;                           // 1
table.insertRow().insertCell().textContent = "x";           // 照 HTML 的演算法
document.body.onhashchange = f;                             // 是 window 的 onhashchange
```

給之後改這部分程式的人：新的元素 interface 照 `crates/dom_api/README.md` 的「怎麼新增一個 API」加：宣告表一行 `interface HTMLXElement : HTMLElement [HtmlXElement] tags("x") as NodeHandle { ... }`，反映的 attribute 用 `reflection.rs` 的 `reflected!`。

## 詳細設計

### A. 哪些元素（已決定的事項第 1 條）

`details.md` 的「元素表」照 HTML 的元素索引（WPT 的 `html/semantics/interfaces.js`）列出每個標籤的 interface。分三組：

- **一般的**（成員都是反映 content attribute，沒有演算法）：`HTMLHtmlElement`、`HTMLHeadElement`、`HTMLBodyElement`、`HTMLHeadingElement`（`h1`–`h6`）、`HTMLParagraphElement`、`HTMLHRElement`、`HTMLPreElement`（`pre`、`listing`、`xmp`）、`HTMLQuoteElement`（`blockquote`、`q`）、`HTMLOListElement`、`HTMLUListElement`、`HTMLMenuElement`、`HTMLLIElement`、`HTMLDListElement`、`HTMLDivElement`、`HTMLDataElement`、`HTMLTimeElement`、`HTMLSpanElement`、`HTMLBRElement`、`HTMLModElement`（`ins`、`del`）、`HTMLStyleElement`（`sheet` 除外）、`HTMLMapElement`、`HTMLParamElement`、`HTMLDirectoryElement`（`dir`）、`HTMLFontElement`：24 個。
- **table**（有 HTML 的演算法：`rows`、`cells`、`tBodies` 這類 collection，`insertRow`、`deleteCell`、`createCaption` 這類方法，`rowIndex`、`cellIndex`）：`HTMLTableElement`、`HTMLTableCaptionElement`、`HTMLTableColElement`（`col`、`colgroup`）、`HTMLTableSectionElement`（`thead`、`tbody`、`tfoot`）、`HTMLTableRowElement`、`HTMLTableCellElement`（`td`、`th`）：6 個。
- **kun 沒有行為的**：上面「不在範圍內」的 10 個。

選項：

- **甲**：只做 Speedometer 用到的 11 個（和 `HTMLBodyElement`），各自的成員全做。
- **乙**：一般的和 table（30 個），各自的成員全做；沒有行為的維持 `HTMLElement`。
- **丙**：全部 40 個，沒有行為的只有 interface 和反映的 attribute（`play()` 這類方法不做或丟 `NotSupportedError`）。

決定：乙（已決定的事項第 1 條）。甲留下一半的元素表，之後每用到一個再補一次，`instanceof` 的結果依元素而不同；丙讓頁面以為有影音、`<dialog>` 的行為（功能偵測走錯路），比沒有 interface 還糟。乙的 30 個裡 24 個只是反映 attribute，`reflection.rs` 已經有每種規則的 getter 和 setter，成本主要在 table。

### B. 成員（已決定的事項第 2 條）

每個 interface 的成員照 `html.idl`（`details.md` 的「元素表」）。其中有 HTML 列為廢棄、但 Chrome 照樣有的（`HTMLDivElement.align`、`HTMLUListElement.compact`、`HTMLTableCellElement.bgColor`……）。

- **甲**：照 `html.idl` 全做，包括廢棄的（Chrome 有，WPT 的 `html/dom/reflection-*.html` 測它們）。
- **乙**：只做非廢棄的，廢棄的列進 `crates/dom_api/tests/spec-missing.txt`。

決定：甲（第 2 條）。廢棄的成員幾乎都是反映字串或數字，一行宣告；少了它們，WPT 的 reflection 測試整份失敗，`'align' in div` 也和 Chrome 不同。

### C. `<body>` 的 handler 和 `<style>` 的 `disabled`

`HTMLBodyElement` 有 `WindowEventHandlers`（`onafterprint`……`onunload`），HTML 的「Window-reflecting body element event handler set」（`onblur`、`onerror`、`onfocus`、`onload`、`onresize`、`onscroll`）在 `<body>` 上讀寫的是 window 的。kun 的元素的 handler（content attribute 和 `HTMLElement` 的 `GlobalEventHandlers` 的 IDL attribute）原本就經過 `handlers.rs` 的 `handler_target`，`<body>` 的那 6 個和 `WindowEventHandlers` 轉到 window；缺的是 `HTMLBodyElement` 這個 interface 和它的 `WindowEventHandlers` 的 IDL attribute（`document.body.onhashchange` 是 `undefined`）。宣告表的 `[WindowEventHandlers] attributes` 宣告它們（和 `[GlobalEventHandlers]` 一樣是一份清單，`window_event_handlers!`），`<body>` 的走同一條路（Blink 的 `HTMLBodyElement` 的 `DEFINE_WINDOW_ATTRIBUTE_EVENT_LISTENER`）。盤點的「`HTMLBodyElement.onfocus`」是因為沒有這個 interface。

window 原本只有 `WindowEventHandlers` 的 4 個（`onhashchange`、`onmessage`、`onmessageerror`、`onpopstate`）；kun 在 window 上發 `unhandledrejection`，`window.onunhandledrejection` 卻不存在。決定（第 8 條）：window 和 `<body>` 都照 `html.idl` 有 18 個全部（Chrome 152 的 `<body>` 沒有 `onpagereveal`、`onpageswap`，照 `html.idl` 留著）；kun 不發的事件（`pageshow`、`beforeunload`……）的 handler 設了不會被呼叫，記進限制。`onbeforeunload` 的型別是 `OnBeforeUnloadEventHandler`，它的回傳值只在分派 `BeforeUnloadEvent` 時有意義，kun 不發這個事件，照一般的 `EventHandler` 做。

**`HTMLStyleElement.disabled`** 不是反映 content attribute：它讀寫元素的 stylesheet 的 disabled flag，`true` 時那份 sheet 不套用；sheet 重建（文字改了）時 flag 回到 `false`，沒有 sheet 時讀到 `false`、設定不做事（Blink 的 `HTMLStyleElement::disabled()` 回傳 `sheet_->disabled()`，setter 呼叫 `sheet_->setDisabled()`）。kun 的 `<style>` 的 sheet 只在 style engine 裡，沒有 CSSOM 的物件。決定（第 9 條）：flag 記在元素的 rare data（`dom` 的 `set_style_sheet_disabled`），style engine 收 `<style>` 的 sheet 時跳過 disabled 的，改了讓 tree scope 的 sheet 重收；sheet 重建（文字或 `type` 改了、插入、移除）時清掉，`media` 改了不清（Chrome 152 實測）。`sheet` 本身照舊不做。`blocking`（render-blocking 的 `DOMTokenList`）照 kun 的 `<script>`、`<link>` 的先例不做，列進 `spec-missing.txt`。

### D. table 的演算法

照 HTML 的「Tabular data」：`HTMLTableElement` 的 `caption`、`tHead`、`tFoot`（取第一個、設定時換掉）、`tBodies`、`rows`（thead 的列、tbody 和直接的列、tfoot 的列的順序）、`createCaption`／`deleteCaption`、`createTHead`……、`insertRow`、`deleteRow`；`HTMLTableSectionElement` 的 `rows`、`insertRow`、`deleteRow`；`HTMLTableRowElement` 的 `rowIndex`、`sectionRowIndex`、`cells`、`insertCell`、`deleteCell`；`HTMLTableCellElement` 的 `cellIndex`、`colSpan`、`rowSpan`（反映的規則有上下限）、`headers`、`scope`、`abbr`。collection 是 live 的 `HTMLCollection`（`dom` 現有的 collection 的做法，見 `crates/dom/README.md`）。

table 的方法直接走 DOM 樹（第 16 條，Blink 的 `HTMLTableRowsCollection::RowAfter`、`LastRow`）：`insertRow(-1)` 從尾端找最後一列、`rowIndex` 走一遍，不經過 `rows` collection（DOM 一改它就整份重算，迴圈裡建表會是 O(n²)）；列的順序寫在 `dom_api` 的 `table` 一處，collection 也照它算。`colSpan`、`rowSpan`、`span` 是「clamped」的 `unsigned long`：解析非負整數沒有上限，太大的夾到最大值（Blink 的 `ParseHTMLClampedNonNegativeInteger`；`reflection-tabular.html` 抓到原本給預設值）。

**collection 存在哪裡**：table 多了 4 種每次同一個物件的 collection（`<table>` 的 `rows`、`tBodies`，section 的 `rows`，`<tr>` 的 `cells`）。kun 的 rare data 原本一種用途一個欄位（`elements`、`options`、`labels`……）。決定（第 11、13、14 條）：照 Blink 的 `NodeListsNodeData`，rare data 的 collection 欄位都搬進一張照 collection 的 filter 找的表（`dom` 的 `collection`）：每次同一個的放在強參照的 `EdgeChunks`（expando 經過 GC 還在），`getElementsByTagName` 這類照名稱的放在 `WeakSet`，script 拿著時同一個（照給的名稱分辨：`'a b'` 和 `'a  b'` 是兩個，Chrome 152 實測），沒人拿著時回收。`EdgeChunks` 和 `WeakSet` 原本一個 chunk 固定 64 格，改成格數是參數，collection 用 4 格的。

**畫出來**：kun 的 presentational hint 原本有 table 的 `width`、`height`、`border`、`cellspacing`、`cellpadding`、`align`、`valign`、`nowrap` 和清單的，沒有 `bgcolor`（`<table>`、`<tr>`、section、cell、`<body>`）、`<body>` 的 `text`、`link`、`vlink`、`alink`、`background`、`<font>` 的 `color`、`face`、`size`、`<hr>` 的 `noshade`、`size`、`color`、`width`、`align`：反映的成員讀寫得到，畫面卻不變。決定（第 12、17 到 19 條）：照 Chrome（Blink 的 `CollectStyleForPresentationAttribute`）做，值都用 `getComputedStyle` 在 Chrome 152 上量過：

- **顏色**：HTML 的 rules for parsing a legacy colour value（`css::values::parse_legacy_color`；`chucknorris` 是 `#c00000`）。
- **table**：table 和它的各部分（column 也是）的 `bgcolor`、`background`（`url()` 照 base URL 解析），`<table bordercolor>`：table 的邊框顏色，有它時 table 和 cell 的邊框是實線（HTML 照樣 outset、inset；照 Chrome），cell 的邊框是它的顏色；section 和 row 照 Chrome 的 UA stylesheet 繼承 table 的 `border-color`。cell 不繼承作者給 table 的 CSS `border-color`：Chrome 152 會繼承，那是 Chromium 的 bug（crbug.com/40745409），照 WPT 的 `table-border-color-not-inherited`。
- **`<body>`**：`bgcolor`、`text`、`background`，`marginwidth`、`leftmargin`（左右）、`marginheight`、`topmargin`（上下），兩個一起出現時後寫的贏（照 Chrome；HTML 是 `marginwidth`、`marginheight` 贏）；`rightmargin`、`bottommargin` Chrome 沒有效果，不做。
- **`<font>`**：`color`、`face`（`font-family`）、`size`（HTML 的 legacy font size，1 到 7 是 `x-small` 到 `xxx-large`）。
- **`<hr>`**（第 17 條，照 Chrome、不照 HTML）：`color` 是邊框和背景的顏色（`color` property 不變）、邊框實線；`noshade` 是灰色的邊框和背景；`size` 取前面的整數，一以下拿掉下邊框，否則 `height: size − 2px`；`align`、`width`。
- **連結顏色**（第 18 條，Blink 的 `TextLinkColors`）：`<body>` 的 `link`、`alink` 是文件的連結顏色，`-webkit-link`、`-webkit-activelink` 照它算（UA stylesheet 的 `:link`、`:active` 用它們，網頁的 CSS 也能用）。style engine 每次 recalc 從 `<body>` 讀，變了整份重算（Blink 只重算連結）；matched properties cache 以環境為 key，換了不共用。`vlink` 是去過的連結的，kun 沒有 `:visited`。

### E. interface object 的成本（已決定的事項第 3 條）

kun 每個 realm 建立時把宣告表裡每個 interface 的 interface object 都裝到 global 上（`dom_js` 的 `lib.rs` 的 `install`），template 每個 isolate 建一次。多 30 個 interface，每個 realm 多 30 個 function 和 prototype：Speedometer 每個 suite 在 iframe 裡跑（一個 realm），iframe 和 `about:blank` 也是。

- **甲**：照現在的做法，先量 realm 建立的時間（`blink-perf` 沒有現成的頁，用 `dom_js` 的計時或 Speedometer）；看得出差別才改。
- **乙**：改成用到才建（V8 的 lazy data property：global 上的 interface object 第一次讀時才建），同時處理現有的 212 個 interface。

決定：乙（第 3 條），在加新的 interface 之前做（「分階段」的第 1 段），改之前和之後量 realm 建立的時間。做法（第 4 到 7 條）：

- **global 的 template 上的 lazy data property**（Blink 的 `IDLMemberInstaller::InstallExposedConstructs`：Window 的 instance template 的 `SetLazyDataProperty`）：只設一次，每個 realm 不必再定義。rusty_v8 只有物件的 `Object::SetLazyDataProperty`，fork 加上 `Template::SetLazyDataProperty` 的 binding；改了 `binding.cc` 要重編 V8。fork 的 commit 是 `9e83767`，發了 `kun-v152.2.0-15`（第 15 條，第 3a 段之後就發），kun 收尾時換成它；在那之前開發時連結本機編的 V8（`RUSTY_V8_ARCHIVE`）。
- **每個 context 一張 interface object 的快取**（Blink 的 `V8PerContextData::ConstructorForType`）：第一次要一個 interface 時先確保它的 parent，再建它、把它的 `[[Prototype]]` 接到 parent 的；global 的 lazy getter 和建 wrapper 都經過它（wrapper 用 instance template `new_instance`，V8 順便建出的 function 沒有接上 parent）。function 本身 V8 已經每個 context 留一個（`FunctionTemplate::GetFunction`），所以快取只記哪些已經接上 parent：context 的 slot 裡一個 interface 一個 bit，不必在 context 裡另外留 V8 的值（`v8::Global` 是 root，會讓 context 一直活著）。lazy getter 在 global 自己的 realm 裡建（Blink 用 holder 的 creation context）：另一個 realm 先讀 `frame.contentWindow.Node` 時，建出的是 frame 的。V8 建 context 時已經建好 global 自己的 `Window`（`window.constructor` 拿得到），建 realm 時把它接上。
- **global 上的都用到才建**：interface object、`[LegacyWindowAlias]`、legacy factory（`Option`、`Image`）、有常數的 callback interface（`NodeFilter`）、namespace（`console`、`CSS`……）。namespace 是 global 物件自己的 lazy data property，不在 template 上：它們要蓋掉 context 原有的 `console`（V8 自己的）。建 realm 時建的只有 `Window`、`EventTarget`（`glue.js` 要它的 prototype），和 `glue.js` 讀 `Element.prototype` 和 `performance` 時建的 `Node`、`Element`、`Performance`。
- **計數的測試和計時**：`dom_js` 的 `testing::interface_objects` 列出一個 context 建過的 interface object，測試確認建一個 realm 只建上面那幾個，讀一個 interface 只建它和它的祖先，先建 wrapper 再讀 global 時 `[[Prototype]]` 照樣接好；realm 建立的時間在 `dom_js` 一次性地量（`realms::realm_creation_time`，建 500 個 realm 取平均），從約 660 µs 到約 270 µs（`progress.md`）。

### 測試

- `dom_api` 的 `spec` 測試：新的 interface 照 `html.idl` 比對，沒做的列進 `crates/dom_api/tests/spec-missing.txt`（`HTMLStyleElement` 的 `sheet`、`blocking`）；`kun:dom` 的 WIT 的 golden file（多 30 個 resource，只新增、不升版）。
- `dom_js` 的 `element_interfaces`：每個標籤的 interface、反映的每種規則、`<body>` 的 handler、`<style>` 的 `disabled`、`<map>` 的 `areas`、table 的每個方法和 collection；預期值都先在 Chrome 152 上跑過同樣的 script。`interface_objects`、`realms` 守 interface object 的建立。
- `dom` 的 `collections`、`style` 的 `legacy_hints`、`tables`、`invalidation`；kun 自己的 reftest 四份（table、`<body>`、`<font>`、`<hr>` 的舊 attribute 和等價的 CSS 畫出來一樣）。
- WPT：`html/semantics/interfaces.html`、`html/dom/reflection-*.html`（`grouping`、`text`、`tabular` 整份通過）、`html/semantics/tabular-data/`、`html/webappapis/scripting/events/` 的五份、`html/rendering/` 的 `<hr>`、`<font>`、`<body>`、table 的那些：用 `--all` 試過，加進清單，失敗的寫了原因。
- mutation test 照 `docs/testing.md`（五份 spec，79 個）。
- `bench.py speedometer --inventory`：「Elements without their interface」和這幾個成員應該消失（照規定只在要求時跑）。

### 分階段

1. **interface object 用到才建**（E）：現有的 212 個，改之前和之後量 realm 建立的時間。
2. **一般的 24 個和 `<body>` 的 handler**（A、B、C）。
3. **table 的 6 個和畫出來的舊 attribute**（D），分成三段，各自開始前決定細節：
   - 3a：`dom` 的 collection 改存在以種類為 key 的表裡（第 11 條）。
   - 3b：table 的 6 個 interface 和它們的演算法。
   - 3c：presentational hint（第 12 條）。

每段開始前決定那一段的細節。改動在 `dom_api`（宣告表、反映、table 的演算法）、`dom`（collection、`<style>` 的 disabled flag）、`dom_js`（interface object 的建立）、`css` 和 `style`（舊式顏色、連結顏色、presentational hint）；`dom_wasm` 的 resource 是宣告表生成的，只重新產生了 `typed_list.rs`。

## 缺點

- **interface object 用到才建要改所有 interface 的安裝**（E）：第一次讀一個 interface 時多一點工作，換來 realm 建立不再跟著 interface 的數目長。
- **宣告表和 WIT 變大**：`kun:dom` 多 30 個 resource。
- **table 的演算法要維護**：collection 的順序和 live。

## 理由和其他做法

- **為什麼不只做 Speedometer 的 11 個**：見 A 和已決定的事項第 1 條；元素表的其他元素成本一樣低（多數只是反映 attribute），一次補齊，`instanceof` 的結果不會依元素而異。
- **為什麼沒有行為的不做**：interface 是功能偵測的依據，空殼比沒有更糟。
- **為什麼不順便做 `HTMLStyleElement.sheet`**：要 document 的 stylesheet 的 CSSOM（`CSSStyleSheet` 包著 style engine 的 stylesheet、改了要讓 style engine 重收），是另一件事。

## 先例

- **Blink**：`html_tag_names.json5` 列出每個標籤的 C++ 類別，`HTMLElementFactory` 照它建元素，沒有專屬類別的是 `HTMLElement`；wrapper 的 interface 照類別的 IDL（`HTMLDivElement.idl`……）。反映的 attribute 是 IDL 的 `[Reflect]`，由生成的程式碼讀寫 content attribute。table 的演算法在 `HTMLTableElement::rows()`（`HTMLTableRowsCollection`）、`insertRow()` 這些。
- **kun**：`declare!` 的 `tags("...")`（`HTMLButtonElement` 這些）和 `reflection.rs`（表單的 interface 用過）。

## 已決定的事項

2026-10-21，草稿之後（使用者）：

1. **範圍照 A 的乙**：一般的 24 個和 table 的 6 個，各自的成員全做；kun 沒有行為的 10 個（影音、`<track>`、`<dialog>`、`<details>`、`<object>`、`<embed>`、frame、`<marquee>`）維持 `HTMLElement`，等做那些功能的 RFC。不選甲（只做 Speedometer 的 11 個）：元素表留一半，`instanceof` 依元素而不同。不選丙（全部 40 個）：沒有行為的 interface 讓功能偵測走錯路。
2. **廢棄的成員照 B 的甲**：照 `html.idl` 全做，包括 Chrome 有的廢棄成員（`align`、`compact`、`HTMLFontElement`……）。不選乙：Speedometer 用到的 `HTMLUListElement.type` 就是廢棄的，WPT 的 reflection 測試也整份失敗。
3. **interface object 照 E 的乙，用到才建**：連同現有的 212 個，在加新的 interface 之前做，改之前和之後量 realm 建立的時間。不選甲（照現在、看得出差別才改）：每加一批 interface，每個 realm（Speedometer 每個 suite 的 iframe）的建立就跟著變貴。

2026-10-21，第 1 段開始前（使用者）：

4. **global 上的用 template 的 lazy data property**（E 的第一點），fork 加 `Template::SetLazyDataProperty` 的 binding、重編 V8；fork 的 release tag 等 RFC 收尾時再打（2026-10-22 改成先打，見第 15 條）。不選每個 realm 在 global 物件上 `Object::SetLazyDataProperty`（不改 fork，但每個 realm 照樣定義兩百多個 property）。
5. **每個 context 一張 interface object 的快取**（E 的第二點），wrapper 也經過它。不選 fork 加 `FunctionTemplate::SetPrototypeProviderTemplate` 讓 V8 自己接 parent（不是 Blink 的做法，一樣要改 fork）。
6. **global 上的都用到才建**（E 的第三點）。不選只做 interface object：namespace 和 legacy factory 留著，realm 建立還有一塊成本。
7. **計數的測試和 `dom_js` 的計時**（E 的第四點）。不選只用 Speedometer 的 `--against`：一個 suite 只建幾個 realm，差別被雜訊蓋掉。

2026-10-21，第 2 段開始前（使用者）：

8. **`WindowEventHandlers` 照 `html.idl` 全做**（C 的前半）：window 和 `<body>` 都有 18 個（做的時候查到 Chrome 152 的 `<body>` 沒有 `onpagereveal`、`onpageswap`，Blink 只放在 window 上；照 `html.idl` 留著）。不選只加 kun 會發的（window 現有的 4 個和 `onunhandledrejection`）：`'onpageshow' in window` 這類檢查和 Chrome 不同。不選只給 `<body>` window 現有的 4 個：`onunhandledrejection` 照樣不存在。
9. **`HTMLStyleElement.disabled` 這一段就做**（C 的後半）：flag 在元素上，style engine 跳過 disabled 的 `<style>`。不選留給 document stylesheet 的 CSSOM、和 `sheet` 一起：`style.disabled = true` 會變成 expando，頁面以為關掉了實際上沒有。`blocking` 照先例不做。
10. **24 個都進 `kun:dom`**：和現有的 HTML 元素的 interface（包括沒有自己成員的 `HTMLPictureElement`）一樣都是 resource，WIT 約多 4000 行。不選沒有自己成員的 `HTMLHeadElement`、`HTMLSpanElement` 標 `[JsOnly]`（SVG 的規則）：HTML 的 interface 一致比較重要。不選只有廢棄成員的也標 `[JsOnly]`：guest 拿不到廢棄的成員，JS 和 wasm 的形狀差更多。

2026-10-21，第 3 段開始前（使用者）：

11. **collection 照 Blink 存在以種類為 key 的表裡**（D 的「collection 存在哪裡」）：rare data 的 collection 欄位都搬進去，table 的 4 種也放在那裡。不選 rare data 再加 3 個欄位（每個有 rare data 的節點多 3 個 edge，大多用不到）。不選照 `areas` 的先例借用現有的欄位（欄位名稱和內容對不上）。
12. **反映的舊 attribute 照 Chrome 畫出來**（D 的「畫出來」）：table、`<body>`、`<font>`、`<hr>` 的都做。不選留到之後（讀寫得到、畫面卻不變）。不選只做 table 和 `<body>` 的 `bgcolor`、`background`。

2026-10-21，第 3a 段開始前（使用者）：

13. **照名稱的 collection 也放進表裡，弱參照**：`getElementsByTagName`、`getElementsByTagNameNS`、`getElementsByClassName`、`getElementsByName` 對同一個節點、同樣的參數回傳同一個物件，沒人拿著時回收（Chrome 152 實測如此，Blink 的 `NodeListsNodeData`），DOM 沒變時沿用算好的項目。不選只放每次同一個的：`===` 和 expando 和 Chrome 不同，迴圈裡反覆呼叫每次都重算。
14. **表是強參照的 edge 清單加一個 `WeakSet`，線性查找**：每次同一個的放在 `EdgeChunks`（撐住，expando 經過 GC 還在），照名稱的放在 `WeakSet`，照 collection 自己的 `CollectionFilter` 比對。不選 Blink 的 hash map：一個節點通常只有幾個 collection，線性比 hash 快，也不必另存 key，`dom` 也沒有 GC 追蹤的 hash map。

2026-10-22，第 3b 段開始前（使用者）：

15. **fork 的 release tag 先打，kun 換 tag 照樣等收尾**（改第 4 條）：rebase 到 RFC 0069（fork 發了 `kun-v152.2.0-14`）之後，fork 的改動 commit 到它的 `kun` 分支（`9e83767`）、發 `kun-v152.2.0-15`，release 的 workflow 編各平台的 V8；kun 的 `RUSTY_V8_MIRROR_TAG` 和之前一樣等 RFC 收尾時再換，在那之前照樣連結本機編的 V8。不選等收尾才打：之後又有別的 RFC 發 tag 時要再搬一次 fork 的改動。

16. **table 的方法直接走 DOM 樹**（照 Blink 的 `HTMLTableRowsCollection::RowAfter`、`LastRow`）：`insertRow`、`deleteRow`、`rowIndex`、`sectionRowIndex`、`insertCell`、`deleteCell`、`cellIndex` 不經過 `rows` collection，列的順序（thead、tbody 和直接的列、tfoot）寫在一處、collection 也照它算。不選都經過 `rows` collection 的快取：DOM 一改就整份重算，迴圈裡建表、刪列是 O(n²)，第一次呼叫還替 table 建一個沒要求的 collection。

2026-10-22，第 3c 段開始前（使用者）：

17. **`<hr>` 照 Chrome**（Blink 的 `HTMLHRElement::CollectStyleForPresentationAttribute`）：`color` 設邊框和背景的顏色、`color` property 不變；`noshade` 的邊框和背景是灰色；`size` 一律是 `height: size − 2px`。不選照 HTML 的「Rendering」（`color` 設 `color`、有 `color` 或 `noshade` 時 `size` 是邊框寬 size ÷ 2）：和 Chrome 畫的不同，和規範不同的地方記進限制。
18. **`<body link>`、`alink` 照 Blink 的 `TextLinkColors`**：文件的連結顏色從 `<body>` 的 attribute 來，UA stylesheet 的 `:link` 用 `-webkit-link`、`:active` 用 `-webkit-activelink`，算 style 時照文件換成顏色；這兩個關鍵字也開放給網頁的 CSS（Chrome 152 的 `CSS.supports` 是 `true`）。不選照規範當成作者層的 hint：`revert` 回到的顏色和 Chrome 不同（Chrome 152 實測 `:link { color: revert }` 還是 `link` 的顏色）。不選只給 UA 用：網頁的 `color: -webkit-link` 會被丟掉。`vlink`：kun 沒有 `:visited`，用不到，記進限制。
19. **這幾個元素上 Chrome 會畫、沒有 IDL attribute 的舊 attribute 也做**：`<table bordercolor>`、table、section、`<tr>`、cell 的 `background`、`<body>` 的 `marginwidth`、`marginheight`、`leftmargin`、`topmargin`（Chrome 152 實測都有效：`marginwidth` 和 `leftmargin` 設左右、`marginheight` 和 `topmargin` 設上下；決定時列進來的 `rightmargin`、`bottommargin` 實測沒有效果，照 Chrome 不做）。不選只做第 12 條列的：舊式網頁常把它們和 `bgcolor` 一起用。

## 待決定

無。

## 之後的可能

- 影音（`HTMLMediaElement`、`<video>`、`<audio>`、`<track>`）、`<dialog>`（top layer）、`<details>` 的開合、`<object>` 和 `<embed>`、frame、`<marquee>`：做它們的行為時加 interface。
- document 的 stylesheet 的 CSSOM（`document.styleSheets`、`HTMLStyleElement.sheet`、`HTMLLinkElement.sheet`）。
- customized built-in element。
- 建 wrapper 時從每個 context 一個的 boilerplate 複製（Blink 的 `V8PerContextData::CreateWrapperFromCache`）。
- table 的 `rules`、`frame`，`<hr>` 的 UA stylesheet 的 `overflow: hidden`，`colspan`、`rowspan`、`span` 太大時排版也夾到上限（`docs/limitations.md`）。
- `HTMLCollection` 的 named property 列在 own property 的名稱裡（`html/semantics/tabular-data/` 的 `table-rows.html` 這類）。
- table 的方法不經過 `rows` collection 是效能的捷徑，沒有計數守著（`docs/test-gaps.md`）。
