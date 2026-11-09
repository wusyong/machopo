# RFC 0070 的細節：HTML 元素的專屬 interface

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 元素表

2026-10-21，照 WPT 的 `html/semantics/interfaces.js`（HTML 的元素索引）和 `interfaces/html.idl`（WPT 釘的 commit）。成員後面標「（廢棄）」的是 `html.idl` 裡 HTML 列為廢棄、寫在 `partial interface` 的（Chrome 有）。`HTMLParamElement`、`HTMLDirectoryElement`、`HTMLFontElement` 整個 interface 是廢棄的（不在 `partial interface` 裡，所以成員沒有標）。Speedometer 那欄是 2026-10-21 的盤點裡用到它的 default suite 數（`README.md` 的「動機」）。

### 一般的

| interface | 標籤 | 成員 | Speedometer |
| --- | --- | --- | --- |
| `HTMLHtmlElement` | html | `version`（廢棄） |  |
| `HTMLHeadElement` | head | （沒有） |  |
| `HTMLBodyElement` | body | `text`（廢棄）、`link`（廢棄）、`vLink`（廢棄）、`aLink`（廢棄）、`bgColor`（廢棄）、`background`（廢棄）、`WindowEventHandlers` | 1（`onfocus`：jQuery） |
| `HTMLHeadingElement` | h1–h6 | `align`（廢棄） | 9 |
| `HTMLParagraphElement` | p | `align`（廢棄） | 3 |
| `HTMLHRElement` | hr | `align`（廢棄）、`color`（廢棄）、`noShade`（廢棄）、`size`（廢棄）、`width`（廢棄） |  |
| `HTMLPreElement` | pre、listing、xmp | `width`（廢棄） |  |
| `HTMLQuoteElement` | blockquote、q | `cite` | 1 |
| `HTMLOListElement` | ol | `reversed`、`start`、`type`、`compact`（廢棄） |  |
| `HTMLUListElement` | ul | `compact`（廢棄）、`type`（廢棄） | 11（`type` 是 2：Backbone、jQuery） |
| `HTMLMenuElement` | menu | `compact`（廢棄） |  |
| `HTMLLIElement` | li | `value`、`type`（廢棄） | 11（`type` 是 2：Backbone、jQuery） |
| `HTMLDListElement` | dl | `compact`（廢棄） |  |
| `HTMLDivElement` | div | `align`（廢棄） | 13 |
| `HTMLDataElement` | data | `value` |  |
| `HTMLTimeElement` | time | `dateTime` |  |
| `HTMLSpanElement` | span | （沒有） | 9 |
| `HTMLBRElement` | br | `clear`（廢棄） | 2 |
| `HTMLModElement` | ins、del | `cite`、`dateTime` |  |
| `HTMLStyleElement` | style | `disabled`、`media`、`blocking`、`type`（廢棄）、`LinkStyle` | 3 |
| `HTMLMapElement` | map | `name`、`areas` |  |
| `HTMLParamElement` | param | `name`、`value`、`type`、`valueType` |  |
| `HTMLDirectoryElement` | dir | `compact` |  |
| `HTMLFontElement` | font | `color`、`face`、`size` |  |

### table

| interface | 標籤 | 成員 | Speedometer |
| --- | --- | --- | --- |
| `HTMLTableElement` | table | `caption`、`createCaption`、`deleteCaption`、`tHead`、`createTHead`、`deleteTHead`、`tFoot`、`createTFoot`、`deleteTFoot`、`tBodies`、`createTBody`、`rows`、`insertRow`、`deleteRow`、`align`（廢棄）、`border`（廢棄）、`frame`（廢棄）、`rules`（廢棄）、`summary`（廢棄）、`width`（廢棄）、`bgColor`（廢棄）、`cellPadding`（廢棄）、`cellSpacing`（廢棄） |  |
| `HTMLTableCaptionElement` | caption | `align`（廢棄） |  |
| `HTMLTableColElement` | col、colgroup | `span`、`align`（廢棄）、`ch`（廢棄）、`chOff`（廢棄）、`vAlign`（廢棄）、`width`（廢棄） |  |
| `HTMLTableSectionElement` | thead、tbody、tfoot | `rows`、`insertRow`、`deleteRow`、`align`（廢棄）、`ch`（廢棄）、`chOff`（廢棄）、`vAlign`（廢棄） |  |
| `HTMLTableRowElement` | tr | `rowIndex`、`sectionRowIndex`、`cells`、`insertCell`、`deleteCell`、`align`（廢棄）、`ch`（廢棄）、`chOff`（廢棄）、`vAlign`（廢棄）、`bgColor`（廢棄） | 1 |
| `HTMLTableCellElement` | td、th | `colSpan`、`rowSpan`、`headers`、`cellIndex`、`scope`、`abbr`、`align`（廢棄）、`axis`（廢棄）、`height`（廢棄）、`width`（廢棄）、`ch`（廢棄）、`chOff`（廢棄）、`noWrap`（廢棄）、`vAlign`（廢棄）、`bgColor`（廢棄） | 1 |

### kun 沒有行為、維持 `HTMLElement` 的

`HTMLMediaElement`（`HTMLVideoElement`、`HTMLAudioElement`）、`HTMLTrackElement`、`HTMLDialogElement`、`HTMLDetailsElement`（kun 的 `<details>` 只有 `display: block`，不會開合）、`HTMLObjectElement`、`HTMLEmbedElement`、`HTMLFrameElement`、`HTMLFrameSetElement`、`HTMLMarqueeElement`。

### kun 現有的 30 個

`HTMLElement`、`HTMLUnknownElement`、`HTMLBaseElement`、`HTMLMetaElement`、`HTMLTitleElement`、`HTMLSlotElement`、`HTMLIFrameElement`、`HTMLLinkElement`、`HTMLScriptElement`、`HTMLAnchorElement`、`HTMLAreaElement`、`HTMLImageElement`、`HTMLPictureElement`、`HTMLSourceElement`、`HTMLTemplateElement`、`HTMLFormElement`、`HTMLLabelElement`、`HTMLInputElement`、`HTMLButtonElement`、`HTMLSelectElement`、`HTMLDataListElement`、`HTMLOptGroupElement`、`HTMLOptionElement`、`HTMLTextAreaElement`、`HTMLOutputElement`、`HTMLProgressElement`、`HTMLMeterElement`、`HTMLFieldSetElement`、`HTMLLegendElement`、`HTMLCanvasElement`（`crates/dom_api/src/interfaces/core_html.rs`）。

## 盤點怎麼判斷

`scripts/bench/speedometer/probe.js`：對每個 suite 在 Chrome 裡建過的元素，在 kun 裡 `createElement` 同一個標籤，比較 `constructor.name` 和 Chrome 的。成員（`HTMLLIElement.type` 這類）是另一份清單：Chrome 裡讀寫過、kun 的 prototype 上沒有的。

## 參考資料

- HTML 的元素索引：<https://html.spec.whatwg.org/multipage/indices.html#element-interfaces>
- HTML 的反映規則：<https://html.spec.whatwg.org/multipage/common-dom-interfaces.html#reflecting-content-attributes-in-idl-attributes>
- HTML 的「Tabular data」：<https://html.spec.whatwg.org/multipage/tables.html>
- Blink：`third_party/blink/renderer/core/html/html_tag_names.json5`、`html/table/html_table_element.cc`、`html/html_body_element.cc`
