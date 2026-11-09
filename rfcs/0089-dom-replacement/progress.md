# RFC 0089 的進度：換成新的 DOM：照 kun 自己的 GC 重做 `dom`、`dom_api`、`dom_js`、`dom_wasm`

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-27，Linux 7.2、Fedora 44（AMD Ryzen AI 9 HX 370）。在 `0089` 上做，第 5 段做完時 merge 回 `main`；收尾的量測、之後的優化和收尾的整理在 `main` 上做。

完成日期：2026-11-06，同一台。

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。「做過的檢查」照做的時候的名字寫：`dom_next`、`dom_api_next`、`dom_js_next`、`dom_wasm_next`、`style_next`、`NextFrameView`、`--next-dom` 這些在最後-a 改名成現在的（決定 183.2），「今天的 DOM」是當時的舊的 DOM，5b 刪掉。寫著「記在待辦」的，收尾時分掉了（決定 184）：仍然成立的限制在 `docs/limitations.md`，測試的空缺在 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」，效能在 `../0085-optimization-roadmap/README.md`（之後改寫成分階段的優化，條目併進它的各段）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 實驗：V8 呼叫 API callback 的地板，官方的設定編的 V8 和今天的比 | 完成（沒有重編：路上沒有 V8 的 C++，決定 7） | 2026-10-27 |
| 1b `gc` 和 V8 一起標的那一側搬進 `gc` | 完成 | 2026-10-27 |
| 1c 新的 DOM 的節點樹 | 完成 | 2026-10-28 |
| 1d 和 engine 無關的 API 的實作 | 完成 | 2026-10-28 |
| 1e binding 的產生方式、`bindings` 的通用部分、rusty_v8 的 fork 的改動 | 完成 | 2026-10-28 |
| 1g-1 兩個 heap 一起標：V8 的 fork 和 patch | 完成（V8 的 fork 的 `485f5e12` 已經 push；rusty_v8 的 fork 的 `3f538ab` 和 `kun-v152.2.0-22` 的 tag 已經 push；那次 release 只有 release profile 的三個平台，debug 的兩個 job（Linux、macOS）在 Build 失敗，原因沒查（2026-10-28 使用者決定暫時不管，0089 一律用本機的 `RUSTY_V8_ARCHIVE`，`third_party.toml` 照舊釘 `rev`）） | 2026-10-28 |
| 1g-2 新的 DOM 從 JS 呼叫得到 | 完成 | 2026-10-28 |
| 1f-1 `declare!` 改成 proc macro，多產生給 engine 的成員清單 | 完成 | 2026-10-28 |
| 1f-2 新的 `dom_wasm_next` | 完成 | 2026-10-29 |
| 1f-3 量：wasm 的版本 | 完成 | 2026-10-29 |
| 第 1 段收尾：`cargo fmt --check`、clippy、`cargo test --workspace` | 完成 | 2026-10-29 |
| 2a 新的 DOM 補上 parser 要的節點種類，`html` 的 sink 接新的 DOM | 完成 | 2026-10-29 |
| 2b-1 binding 的基礎：不是節點的物件、建構子、dictionary、`double`、`unsigned short` | 完成 | 2026-10-29 |
| 2b-2 事件：union、callback interface、listener、分派 | 完成 | 2026-10-29 |
| 2b-3 追回第 2 段多出的呼叫成本（`nodeType`、`appendChild`、`createElement`） | 完成 | 2026-10-29 |
| 2c-1 window 當 global、`Location`、`#[unforgeable]`、目前的 realm 和 `timeStamp` | 完成 | 2026-10-29 |
| 2c-2 timer、例外的回報和 `ErrorEvent` | 完成 | 2026-10-29 |
| 2c-3 `HTMLCollection`、照 tag 的 element interface、testharness 的煙霧測試 | 完成 | 2026-10-29 |
| 2d-1 開關、`NextEngine`、realm、會暫停的 parser、classic script | 完成 | 2026-10-29 |
| 2d-2 載入的生命週期、timer、testharness 要的成員、headless 的驗收 | 完成 | 2026-10-29 |
| 2e `test.py wpt-next` 和 `dom/nodes` 的第一份對照 | 完成 | 2026-10-29 |
| 第 2 段收尾：`cargo fmt --check`、clippy、`cargo test --workspace` | 完成 | 2026-10-29 |
| 2h-1a 狀態只經過 isolate 借（沒有 `RefCell`）、`Front` 拆成操作外和操作內、getter 用 `read` | 完成 | 2026-10-29 |
| 2h-1b `Front` 那一層：`V8Op` 借一個指標、留下的結果放在 box 裡 | 完成 | 2026-10-29 |
| 3a-1 每個 thread 一張名稱和值的表，parser、新的 DOM、`style` 共用 | 完成 | 2026-10-29 |
| 3a-2 `DomString` 非 atomic（查完先給使用者審） | 完成 | 2026-10-30 |
| 3a-3 `Node` 的 flag word、元素的 style 資料、dirty bit、要的時候才給 id | 完成 | 2026-10-30 |
| 3a-4 `style_next`：並排的 `style` 讀新的 DOM（決定 58） | 完成 | 2026-10-30 |
| 3b-1 `NodeId` 搬到 `gfx`、`layout` 的 `next` 模組建 box tree：block、inline 和文字、差分比對（決定 59） | 完成 | 2026-10-30 |
| 3b-2 `next` 的增量更新、`display: contents`、generated content、list、counter | 完成 | 2026-10-30 |
| 3b-3 `next` 的 table、flex 和 grid 的 anonymous item、`style_next` 的 presentation 屬性 | 完成 | 2026-10-30 |
| 3c-1 `NextFrameView`：新的 DOM 的 lifecycle 畫出 frame，headless 的截圖接受 `--next-dom`（決定 61） | 完成 | 2026-10-30 |
| 3c-2 `<link rel=stylesheet>`、`<style>` 和 `<link>` 的 `media` | 完成 | 2026-10-30 |
| 3c-3 量：新舊 DOM 的 style、box tree、layout、paint | 完成 | 2026-10-30 |
| 3d `test.py reftest-next`：reftest 在新舊 DOM 上 headless 對照（決定 62） | 完成 | 2026-10-30 |
| 4a-1 `getElementById`、`querySelector(All)`、`matches`、`closest`、`classList`、`dataset`（決定 63、64） | 完成 | 2026-10-30 |
| 4a-2 `innerHTML`、`outerHTML`、`insertAdjacentHTML`（決定 65） | 完成 | 2026-10-30 |
| 4a-3 `element.style`（決定 64.4、66） | 完成 | 2026-10-30 |
| 4a-4 `MutationObserver`（決定 64.5、67） | 完成 | 2026-10-30 |
| 4b-1 script 讀 geometry（決定 69） | 完成 | 2026-10-30 |
| 4b-2a 事件的 interface、滑鼠、`:hover`、`:active`（決定 70） | 完成 | 2026-10-30 |
| 4b-2b 鍵盤、視窗模式開放 `--next-dom`（決定 70） | 完成 | 2026-10-30 |
| 4b-3 `requestAnimationFrame`（決定 71） | 完成 | 2026-10-30 |
| 4b-4a focus 的狀態、成員、`FocusEvent`、pseudo-class、fixup（決定 72） | 完成 | 2026-10-30 |
| 4b-4b 滑鼠 focus、Tab 的循序導覽、按鍵的目標（決定 72） | 完成 | 2026-10-30 |
| 4c-1 WebDriver 在新的 DOM 上（決定 73） | 完成 | 2026-10-30 |
| 4c-2 testharness 的 executor 要的 `window.open`、`postMessage`（決定 73） | 不做（決定 73.4：executor 不用它們） | |
| 4c-3a `test.py wpt --next-dom`、`reftest --next-dom`，拿掉 `wpt-next`、`reftest-next`（決定 74.1） | 完成 | 2026-10-30 |
| 4c-3b 情境在新的 DOM 上（決定 74.2） | 完成 | 2026-10-30 |
| 4d-1 分派跳過沒有 listener 的節點、root 照 `PersistentRegion`（決定 75.2） | 完成 | 2026-10-30 |
| 4d-2 event 和 listener 的 type 存名稱的編號（決定 75.1、76） | 完成 | 2026-10-31 |
| 4d-3 `on*` 的 handler 屬性（決定 75.1、77） | 完成 | 2026-10-31 |
| 4d-4a `CustomEvent`、`createEvent`、`InputEvent`、`WheelEvent` 的 interface（決定 75.1、78） | 完成 | 2026-10-31 |
| 4d-4b 滾輪的輸入（決定 78.2） | 完成 | 2026-10-31 |
| 4e-1 shadow root、tree scope、retarget、`composedPath`、`getRootNode`、`isConnected`（決定 79、80） | 完成 | 2026-10-31 |
| 4e-2a slot 的分配、`HTMLSlotElement`、`slotchange`、分派經過 slot（決定 79、81） | 完成 | 2026-10-31 |
| 4e-2b-1 樹的 API：`childNodes`、`children`、`getElementsByClassName`、`ParentNode`、`ChildNode` 的方法、`replaceChild`、`normalize`、`compareDocumentPosition`、`isEqualNode`、namespace 的查詢（決定 82） | 完成 | 2026-10-31 |
| 4e-2b-2 建立節點和文件：`DOMImplementation`、`create*`、`cloneNode`、`importNode`、`adoptNode`（決定 82） | 完成 | 2026-10-31 |
| 4e-2b-3 `document.body`、`head`、`doctype`、`template.content`（決定 83） | 完成 | 2026-10-31 |
| 4e-2c-1 flat tree 的走法、dirty bit 穿過 host 和 slot、`style_next` 照 flat tree（決定 81、84） | 完成 | 2026-10-31 |
| 4e-2c-2 `layout` 照 flat tree、`:hover` 鏈穿過 host，shadow tree 畫出來（決定 81、84） | 完成 | 2026-10-31 |
| 4e-3a `style_next` 的 invalidation set（document 的；決定 85.3、86、87）：4e-3a-1 descendant 和 sibling 的 set、4e-3a-2 stylesheet 的 invalidation、4e-3a-3 DOM 只記 style 讀的改變 | 完成 | 2026-10-31 |
| 4e-3b shadow root 的 sheet、`:host`、`::slotted()`、cascade 順序和它們的 invalidation（決定 79、85、88）：4e-3b-1 sheet、比對和 cascade 順序、4e-3b-2 精細的 invalidation | 完成 | 2026-10-31 |
| 4e-3c CSSOM（`CSSStyleSheet`）和 `adoptedStyleSheets`（決定 85.1、89）：4e-3c-1 binding 和 DOM 那一側、4e-3c-2 style 套用 adopted 的 sheet | 完成 | 2026-10-31 |
| 4e-4 custom element（決定 79、90、91、92、93）：4e-4a-1 registry 和定義、4e-4a-2 建構和 upgrade、4e-4b-1 reaction 和 `[CEReactions]`、4e-4b-2 生命週期 callback、4e-4c-1 `:defined`、4e-4c-2 parser 當場建構 | 完成 | 2026-11-01 |
| 4e-5 declarative shadow DOM（決定 79.4、94）：4e-5a parser、`attachShadow`、template 的 IDL attribute；4e-5b `setHTMLUnsafe`、`parseHTMLUnsafe`、`getHTML` | 完成 | 2026-11-01 |
| 4f 表單（決定 95 到 108）：4f-1 interface、反映、form owner、`<label>`；4f-2 值、勾選、disabled、activation、pseudo-class；4f-3 文字控制項；4f-4 `<select>` 這一族；4f-5 追上今天的 DOM 剩下的（決定 108：驗證和送出不在這份） | 完成 | 2026-11-01 |
| 追退步（決定 109）：`bench.py calls` 的量法、「待辦」裡沒解釋的量測退步；每次呼叫都付的三個固定成本（決定 110） | 完成 | 2026-11-01 |
| 4g-1a 圖片：`HTMLImageElement` 的模型和載入、pending request、跨 document 的快取、replaced 的 layout 和 paint、`background-image`（決定 111、112） | 完成 | 2026-11-01 |
| 4g-1b-1 圖片：`srcset`、`sizes`、`<picture>` 和 `<source>`、環境改變時重新選來源（決定 112、113） | 完成 | 2026-11-01 |
| 4g-1b-2a IntersectionObserver：interface、observer 和 observation、rendering update 算交集、task 送 entry，v1 加 v2（決定 113.2、114） | 完成 | 2026-11-01 |
| 4g-1b-2b-1 ResizeObserver（決定 115.2：從 4h 提前，追上今天的 DOM；決定 116） | 完成 | 2026-11-01 |
| 4g-1b-2b-2 `loading=lazy`（內部的 IntersectionObserver）和 `sizes=auto`（內部的 ResizeObserver），捲動容器裡的距離（決定 113.2、114、115） | 完成 | 2026-11-01 |
| 4g-2a canvas：共用的 crate、`HTMLCanvasElement`、context 的狀態、transform、合成、顏色、矩形、路徑、線的樣式，renderer 畫出來（決定 111、117） | 完成 | 2026-11-02 |
| 4g-2b-1 canvas：`ImageData`（含給 `data` 的建構子）、`getImageData`、`putImageData`、`createImageData`、漸層、陰影、`DOMMatrix2DInit`（`setTransform` 的 dictionary，4g-2a 留下的）、`getContextAttributes`（決定 117、118） | 完成 | 2026-11-02 |
| 4g-2b-2 canvas：`drawImage`、`createPattern` 和圖案（`setTransform`）、origin-clean（決定 117、118） | 完成 | 2026-11-02 |
| 4g-2b-3 canvas：`toDataURL`、`toBlob` 在一條編碼 thread 上編、新的 DOM 的 `Blob`（建構子、`slice`、讀出 bytes；決定 117、118、119） | 完成 | 2026-11-02 |
| 4g-2c-1 canvas：`Path2D`（建構子、`addPath`）、`isPointInPath`、`isPointInStroke`、`roundRect`（決定 117、120） | 完成 | 2026-11-02 |
| 4g-2c-2 canvas：文字的狀態、`font`（和它的快取）、`fillText`、`strokeText`、`measureText`、`TextMetrics`（決定 117、120） | 完成 | 2026-11-02 |
| 4g-3a SVG：元素和 interface、presentation 屬性、`next` builder 的 SVG（外層 `<svg>`、形狀、`<g>`、巢狀的 `<svg>`、`<text>`）（決定 111、121） | 完成 | 2026-11-02 |
| 4g-3b SVG：`<use>`（clone 進 shadow tree）、`<symbol>`、資源（漸層、`clipPath`）（決定 121） | 完成 | 2026-11-02 |
| 4g-3c SVG：幾何的 API、`SVGMatrix` / `SVGPoint` / `SVGRect`、文字的方法、client rect 和 focus、hit test（決定 121、122） | 完成 | 2026-11-02 |
| 4g-3d SVG：SVG 圖片（`<img>`、背景）、XML parser 進新的 DOM（決定 121、123） | 完成 | 2026-11-02 |
| 4g-4a iframe：frame tree（一個 engine 裡多個 realm、`HTMLIFrameElement`、child document 的建立和載入、iframe 的 `load`、window 的 frame 成員、layout 和 paint 的 child frame）（決定 111、124、125） | 完成 | 2026-11-02 |
| 4g-4b-1 iframe：每個 window 的 timer、rAF、rendering update 的步驟照 tree order、跨 frame 的 IntersectionObserver（決定 124、126） | 完成 | 2026-11-02 |
| 4g-4b-2a iframe：滑鼠和滾輪進子 frame（座標換算、hover 鏈跨 frame、capture、滾輪往外層傳）（決定 124、127） | 完成 | 2026-11-02 |
| 4g-4b-2b iframe：鍵盤和 focus 跨 frame（focused frame、Tab、window 的 `focus` / `blur`、`hasFocus()`）（決定 124、127） | 完成 | 2026-11-02 |
| 4g-4c-1 iframe：`WindowProxy`（換文件時同一個 global proxy、初始的 `about:blank` 沿用 `Window`）（決定 124、128） | 完成 | 2026-11-02 |
| 4g-4c-2 iframe：`postMessage`、`MessageEvent`、`MessageChannel` / `MessagePort`（決定 124、128、129） | 完成 | 2026-11-02 |
| 4g-4c-3a iframe：跨 origin 的物件（V8 的 access check）、子 frame 的 `location` 導覽（決定 124、128、130） | 完成 | 2026-11-02 |
| 4g-4c-3b iframe：WebDriver 的 Switch To Frame、Switch To Parent Frame（決定 124、130、131） | 完成 | 2026-11-02 |
| 4h-1a DOM 核心的小成員：`CharacterData` 和 `Text` 的方法、`Element` 和 `Document` 剩下的、`Attr`、`NamedNodeMap`、`CDATASection`、`XMLDocument`、`DOMParser`（決定 132、133） | 完成 | 2026-11-03 |
| 4h-1b-1 `NodeIterator`、`TreeWalker`、`NodeFilter`、`StaticRange`、`Range` 的 boundary point 和 live 的調整（決定 133、134） | 完成 | 2026-11-03 |
| 4h-1b-2 `Range` 的內容操作、`toString`、`createContextualFragment`、`getClientRects`、`getBoundingClientRect`（決定 134、135） | 完成 | 2026-11-03 |
| 4h-1c-1 `Selection`、`getSelection`、`selectionchange`（文字控制項的也是）、畫 document 的選取（決定 133、136） | 完成 | 2026-11-03 |
| 4h-1c-2 `contentEditable`、`isContentEditable`、editing host 的 focus 和初始的選取、editing host 裡的游標（決定 136、137） | 完成 | 2026-11-03 |
| 4h-1d-1 「prepare the script element」、`HTMLScriptElement`、script 插入的 inline script 在插入之後跑（決定 135、138） | 完成 | 2026-11-03 |
| 4h-1d-2 外部的 script：`async`、`defer`、in-order 的清單、`load` / `error`、延遲 `load`、解碼和 CORS、muted errors（決定 138、139） | 完成 | 2026-11-03 |
| 4h-1e-1 module script：靜態的 graph、inline 和外部的、defer、async、script 插入的、`import.meta.url`（決定 138、140） | 完成 | 2026-11-03 |
| 4h-1e-2 `import()`、`modulepreload`、`import.meta.resolve`（決定 140、141） | 完成 | 2026-11-03 |
| 4h-2a CSSOM：sheet 和 rule、`MediaList`、`StyleSheetList`、`HTMLStyleElement`、`HTMLLinkElement`（決定 132、142） | 完成 | 2026-11-03 |
| 4h-2b-1 `getComputedStyle`（含 pseudo-element）、`::selection`（決定 142、143） | 完成 | 2026-11-03 |
| 4h-2b-2 `matchMedia`、`MediaQueryList`、`MediaQueryListEvent`、`CSS.supports` / `escape`（決定 142、143、144） | 完成 | 2026-11-03 |
| 4h-3a 捲動：`scrollTop` / `scrollLeft` 的 setter、`scroll`、`scrollTo`、`scrollBy`、`scrollIntoView`、`scroll` 事件（決定 132、145） | 完成 | 2026-11-03 |
| 4h-3b CSS transition 和 animation（決定 132、145、146） | 完成 | 2026-11-03 |
| 4h-3c-1 Web Animations：`Animation`、`animate()`、`document.timeline`、`getAnimations()`、CSS 動畫的物件、promise 和事件（決定 132、145、147） | 完成 | 2026-11-03 |
| 4h-3c-2 `KeyframeEffect` 物件、被取代的動畫的移除（`persist`、`onremove`、`commitStyles`）（決定 147、148） | 完成 | 2026-11-03 |
| 4h-4a 導覽：`Location`、最上層的導覽、fragment 和 `hashchange`、`History` 和 `popstate`（決定 132、149） | 完成 | 2026-11-03 |
| 4h-4b 連結和 base URL：`<a>`、`<area>`、`<base>`、跟著連結導覽、`document.title`（決定 132、149、150） | 完成 | 2026-11-04 |
| 4h-4c-1 HTML 元素的 interface（表格、`<map>`、`<body>`）、`HTMLElement` 的屬性、`document.images` 這些 collection（決定 132、149、151） | 完成 | 2026-11-04 |
| 4h-4c-2 window 的具名屬性、`Navigator`、`UserActivation`、autofocus（決定 132、149、151、152） | 完成 | 2026-11-04 |
| 4h-5a 宣告表的上限、`URL`、`URLSearchParams`、`crypto`、`structuredClone`、`File`、`FileReader`（決定 132、153、154） | 完成 | 2026-11-04 |
| 4h-5b-1 `performance` 那一族：`Performance`、mark 和 measure、`PerformanceObserver`、navigation timing（決定 132、153、155） | 完成 | 2026-11-04 |
| 4h-5b-2 idle callback、scheduling、geolocation、wake lock、network information（決定 132、153、155、156） | 完成 | 2026-11-04 |
| 4h-5c-1 `AbortController`、`AbortSignal`、`addEventListener` 的 `signal`、`Headers`（決定 132、133.4、153、157） | 完成 | 2026-11-04 |
| 4h-5c-2 `Request`、`Response`、body、`fetch()` 和 renderer 的路（決定 132、153、157、158） | 完成 | 2026-11-04 |
| 第 4 段收尾：`cargo test --workspace`、`cargo fmt --check`、clippy，和使用者一起審「待辦」（4f 之後提早跑過一次，4h 做完再跑） | 完成（決定 159） | 2026-11-04 |
| 4i-1 binding 的形狀（決定 159.1、160）：`Uncaught `、prototype 上的順序（binding 和宣告表都照今天的 DOM）、`Window` 的成員在 global 上、unforgeable 成員也在子類別的物件上（`isTrusted`）、indexed 的 enumerator 和 query、小成員（`MouseEvent` 的 `fromElement`、`toElement`、`layerX`、`layerY`，`initMessageEvent`、`createEvent('MessageEvent')`，`DOMRect`、`DOMRectReadOnly` 的建構子、`fromRect`、`toJSON`，observer 的矩形是 `DOMRectReadOnly`）；對照兩邊的 `getOwnPropertyNames` 時再找到的：`[Replaceable]`、`window.event`、`WebKitMutationObserver`、`document` 的 `onvisibilitychange` 和 `onprerenderingchange`、`SVGElement.autofocus`、`CSSStyleProperties`、`PerformanceServerTiming` 和 `NotRestoredReasons`、`document.all`（`HTMLAllCollection`） | 完成 | 2026-11-04 |
| 4i-2 binding 的物件和時機：wrapper 的 realm、init dictionary 的 `view` 和 `relatedTarget`、listener 之間的 microtask checkpoint、`DOMException` 的 structured clone、`QuotaExceededError`（決定 160.2、161） | 完成 | 2026-11-04 |
| binding 的單型化照成員的種類分派（決定 162）：`TypedVisitor` 每種成員一個方法，宣告表展開時就決定；跨 origin 的成員是宣告表的 `#[cross_origin]` | 完成 | 2026-11-05 |
| 4i-3 frame 和自動化：user activation 傳過 frame tree、`focus()` 的捲動、WebDriver 先捲進視野、DevTools 的 `Page` 的子 frame 和 network idle（`<body>` 的 WindowEventHandlers 4h-4c-1 已經有了；決定 160.2、163） | 完成 | 2026-11-05 |
| 4i-4 畫面：系統顏色、子 frame 的 sticky、`marker-content-017`（`::selection` 4h-2b-1 已經有了；決定 160.2、164）和 4i 的收尾 | 完成 | 2026-11-05 |
| 5a 換成預設：開關拿掉，每份文件都是新的 DOM；預期結果、`shell`、情境改成新的 DOM 的，壞的標成已知失敗（決定 165、166） | 完成 | 2026-11-05 |
| 5b-1 刪掉舊的 wasm：`sdk/`、W 系列的 reftest、wasm 的情境和測試、`about:wasm-demo`、`--script`、blink-perf 的 `--gc`（`dom_wasm` 和 `kun:dom` 留到 5b-2；決定 165、167） | 完成 | 2026-11-05 |
| 5b-2 刪掉 `dom_wasm`、`dom_js` 和 renderer、`shell` 裡舊的路，量測工具搬到新的 DOM（`dom_api` 和 `kun:dom` 留到 5b-3；決定 165、167、168） | 完成 | 2026-11-05 |
| 5b-3a `layout`、`paint`、`style`、`html`、renderer 的測試搬到新的 DOM（決定 169） | 完成 | 2026-11-05 |
| 5b-3b 刪掉 `dom_api`、`kun:dom`、`dom`、`dom_macros` 和共用的 crate 裡舊的路，型別和名稱換成新的 DOM 的，`gc_bench` 只留 `gc` 的部分（決定 165、167、169） | 完成 | 2026-11-05 |
| 5b-3c `bindings` 的 cppgc 只留 `CppHeap` 當掛點（決定 167、169、170） | 完成 | 2026-11-05 |
| 5c-1 DevTools 接新的 engine 的 inspector：Runtime、Debugger、Profiler、console 和例外、暫停和打斷、`Log`（決定 165.2、171） | 完成 | 2026-11-05 |
| 5c-2 DevTools 的 renderer 這一側：`Network` 和 initiator、`getResourceTree` 的 resource、script 做的 frame 的 `frameAttached`（決定 165.2、171） | 完成 | 2026-11-05 |
| 5c-3 heap snapshot 裡的 DOM：fork 綁 `EmbedderGraph`，本機編 V8，做完再發 release（決定 165.2、171、172） | 完成（release 還沒發） | 2026-11-05 |
| 5d-1 輸入和游標（決定 173、174） | 完成 | 2026-11-05 |
| 5d-2a script 和載入（決定 173、175） | 完成 | 2026-11-05 |
| 5d-2b task 和時序，window 的 `load` 排成 task（決定 173、175、176） | 完成 | 2026-11-05 |
| 5d-2c 導覽和 frame（決定 173、175、177） | 完成 | 2026-11-05 |
| 5d-3a 幾何和捲動，含 scroll anchoring、resize observer、子 frame 的 scroll into view（決定 173、178） | 完成 | 2026-11-05 |
| 5d-3b `layout` 的 7 個和 `style_next` 的正確性：動畫、shadow、invalidation（決定 173、178、179） | 完成 | 2026-11-05 |
| 5d-3c `style_next` 的成本：rule index、新的 viewport、重算的集合（決定 173、178） | 完成 | 2026-11-05 |
| 5d-4a 已知失敗、時好時壞的 WPT、5d-3 的退步（決定 173、180） | 完成 | 2026-11-05 |
| 5d-4b WPT gap 裡 DOM 和腳本的：realm、custom element reactions、idlharness、`moving-between-documents`、選取、navigation timing（決定 173、180、181） | 完成 | 2026-11-06 |
| 5d-4c WPT gap 裡 layout 和渲染的：`<img>`、控制項的 baseline、CSS 和 shadow、`scroll()` 的參數、wdspec（決定 173、180、182） | 完成 | 2026-11-06 |
| 最後-a 改名：crate、型別、模組、測試、腳本和文件（決定 165、183） | 完成 | 2026-11-06 |
| 最後-b 收抽象：layout 的 trait 和 `next` 模組、決定 60 的參數、renderer 直接依賴 `css`（決定 183） | 完成 | 2026-11-06 |
| 最後-c 文件和註解：`docs/limitations.md`、出處說明、`docs/test-gaps.md` 重量（決定 183） | 完成 | 2026-11-06 |
| 最後-d fork：拿掉不再用的修改、重編 V8、release（決定 183） | 完成 | 2026-11-06 |
| 第 5 段收尾：`cargo test --workspace`、`cargo fmt --check`、clippy | 完成 | 2026-11-06 |
| 收尾的量測：Speedometer、blink-perf、`calls` 和舊的 DOM、Chrome 比（`measurements.md`）；量的時候修的工具：trace 的 script slice、取樣、`Gc.*` 的階段 | 完成 | 2026-11-06 |
| 收尾之後的優化（在 `main` 上，照使用者排的順序）：`HTMLCollection` 的 index 快取、querySelector 的 compound 比對、子節點的環、子樹的 bloom filter、caret 的第一個位置只算第一個 cluster | 完成 | 2026-11-06 |
| 收尾（決定 184）：2a「待辦」分完、2b 這份濃縮、2c RFC 改寫成完成時的樣子、2d 現在的文件 | 完成 | 2026-11-06 |
| 2h-1c 讀取的 barrier：用型別保證是 isolate 的 heap，拿掉 cage 的檢查 | 不做（決定 53：之後的可能） | |
| 2h-1d 分派跳過沒有 listener 的節點、root 不經過 `Rc` 和 `RefCell` | 不做（決定 53：分派到第 4 段，root 到第 3 段開頭） | |
| 2h-2 字串的進出 | 不做（決定 52、53） | |
| 2h-3 建 wrapper 和 V8 的 scavenge | 不做（決定 52、53） | |
| 1h 量：`bench.py blink-perf` | 完成 | 2026-10-28 |
| 1h-2 往 Chrome 靠：getter 的固定成本、`className` 和 `id`、名稱的字串 | 完成 | 2026-10-28 |
| 1h-3 往 Chrome 靠：字串參數在操作裡讀 V8 的字元 | 完成 | 2026-10-28 |
| 1h-4 往 Chrome 靠：getter 從 handle 直接寫回傳值 | 完成 | 2026-10-28 |
| 1h-5 GC 的量測 | 完成 | 2026-10-28 |
| 1h-6 major GC 的時機，之後重設 traced handle | 完成 | 2026-10-28 |

## 驗收

都在 Linux 7.2、Fedora 44（AMD Ryzen AI 9 HX 370）上驗。第 5 段收尾時（2026-11-06）`cargo test --workspace` 58 個 binary、3900 個過、1 個 ignored，`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 沒有錯；收尾之後的優化每一項跑改到的 crate 的測試（子節點的環那項跑了整個 workspace，3907 個過）。整套的 WPT、reftest、情境、`matrix`、`fuzz` 由每天的 CI 跑。效能和舊的 DOM、Chrome 的比較在 `measurements.md`。下表的 `mutants-*.toml` 都在這個資料夾，200 份、4582 條。

**GC 和兩個 heap**

| 項目 | 怎麼驗證 |
| --- | --- |
| `gc` 的 driven heap 由 engine 推進 old 的 marking（`start_marking`、`advance_marking`、`finish_marking`），nursery 滿了自己 scavenge，帶 `JsEdge` 的型別配在 old；marking 結束時回報存活量，V8 照它算下一次的門檻 | `gc` 的 `js`、`young`、`concurrent`；`dom_js` 的 `wrappers`（160 MB 裡 V8 至少 GC 4 次）；`mutants-1b.toml`、`mutants-1h-6.toml` |
| fuzz 有 driven 的 heap 和假的 engine（wrapper 的 root 和邊、Dijkstra 的 barrier、engine 的停頓） | `gc` 的 `ops`、`fuzz`；`test.py gc-fuzz`；`mutants-1f-2.toml` |
| 兩個 heap 一起標：script 拿著的 wrapper 和 root 拿著的物件活著，跨兩個 heap 的循環收掉，scavenge 只丟沒改過的 wrapper；增量 marking 和 linear 的閉包下不遺失；V8 在平行的 thread 上重設 traced handle | `bindings` 的 `unified_heap`、`gc_platform`，`unified_heap_marking`（`--stress-incremental-marking`）、`unified_heap_linear`、`cppgc_traced_marking`；`dom_js` 的 `marking`；`mutants-1g-1.toml` |
| root 照 `PersistentRegion`（表放在 `OwnedShared`，不計數、沒有借用的旗標）；root 活得比 heap 久時 debug build panic | `gc` 的 `heap::a_root_outliving_its_heap_is_caught`、`a_heap_that_moves_keeps_its_roots`；`dom_js` 的 `a_layout_holding_the_document_goes_before_the_heap`；`mutants-4d-1.toml` |
| safepoint 先看 inline 的旗標，write barrier 的 young 頁表是固定長度的陣列，`Member::set` inline；`Gc<Erased>::downcast` 只轉得回物件的型別，`HeapVector::clear` 不留住清掉的物件 | `cargo test -p gc -p bindings -p dom`；`mutants-110.toml` |
| `LocalToken` 一個 thread 同時一個，`TokenCell` 讀拿 `&`、寫拿 `&mut`，離不開 thread | `gc` 的 `token`、`crates/gc/tests/ui/` 的 compile-fail |
| heap snapshot 有 DOM：`Heap::walk` 走過留下的物件、照欄位名的邊和 root，`bindings` 把物件和它的 wrapper 併成一個，detachedness 問 `Cx::is_detached` | `gc` 的 `walk`；`bindings` 的 `heap_snapshot`；`shell` 的 `devtools::a_heap_snapshot_has_the_dom`；`mutants-5c-3.toml` |
| rusty_v8 的 fork 只留 `TracedSlot` 和 `make_garbage_collected`（release `kun-v152.2.0-27`），`CppHeap` 只當掛點、標記固定 incremental | `bindings` 的 `cppgc_traced`、`cppgc_traced_marking`；`nm` 確認 archive 沒有拿掉的符號；`mutants-last-d.toml` |

**節點樹、名稱和字串**

| 項目 | 怎麼驗證 |
| --- | --- |
| 節點有 DOM 的 7 種，`Node` 56 bytes、加 header 一格 64 bytes；插入和移除的連結兩個方向一致，pre-insertion validity（host-including）、adopt、fragment 的插入、clone；子節點是 previous 的環（第一個的 previous 是最後一個，沒有 `last_child`） | `dom` 的 `tree`（`children_link_both_ways_after_every_removal`）、`kinds`（`a_node_fits_its_slot`）、`text`、`gc`（開著 verifier）；`mutants-1c.toml`、`mutants-2a.toml`、`mutants-wrap-up-ring.toml` |
| 屬性照 qualified name 和命名空間；名稱的表每個 thread 一張，固定的名稱編譯時產生（perfect hash）、其他的接在後面，`QualifiedName` 一個 `u32`，parser、DOM、`style` 共用；空字串直接是 `NONE` | `dom` 的 `names`、`attributes`；`mutants-3a-1.toml`、`mutants-1h-2.toml` |
| `DomString` 是 `Rc<str>`、不是 `Send`（編譯時的斷言）；值表的清理和門檻 | `dom` 和 `dom_js` 的 `strings`；`mutants-3a-2.toml` |
| `Node` 的 flag word、`ElementData`、只在 connected 的樹裡標的 dirty bit、樹的版本、要的時候才給的 id 和 class 清單；document 的 `getElementById` 查 id 表（`HAS_ID` 的元素才找） | `dom` 的 `style`、`ids`（計數）；`mutants-3a-3.toml`、`mutants-4a-1.toml` |
| 子樹的 bloom filter：屬性名稱和 class 的位元加到自己和祖先（停在已經有的祖先），插入的子樹帶著自己的，clone 帶原本的 | `dom` 的 `bloom`；`mutants-wrap-up-bloom.toml` |
| `HTMLCollection` 是 live 的、比名稱的編號；記著上一次找到的元素和 index、數過的長度和樹的版本，順著讀每個元素一步，樹或 quirks mode 一變就重找 | `dom` 的 `collection`（`a_collection_walked_in_order_takes_a_step_an_element`、`a_collections_cache_follows_every_change`）；`dom_js` 的 `collections`；`mutants-2c-3.toml`、`mutants-wrap-up-collections.toml` |
| `MutationObserver`：每種改變記 record、microtask 送出、registration 留住 observer、parser 插入的在下一個 script 之前送到 | `dom` 的 `mutation`；`dom_api` 和 `dom_js` 的 `mutation_observer`；`dom_js` 的 `engine`；`mutants-4a-4.toml` |
| shadow root、tree scope、retarget、`composedPath`、`getRootNode`；slot 的分配、`HTMLSlotElement`、`slotchange`；flat tree 的走法、dirty bit 穿過 host 和 slot | `dom` 的 `shadow`、`slot`；`dom_api` 的 `shadow`；`dom_js` 的 `shadow_dom`；WPT `shadow-dom/`；`mutants-4e-1.toml`、`mutants-4e-2a.toml`、`mutants-4e-2c-1.toml` |
| 樹的 API、建立節點和文件、`document.body` 這些、`template.content`；`Range`、`StaticRange`、live range 的調整、`NodeIterator`、`TreeWalker`、`Selection`；DOM 核心的小成員、`Attr`、`NamedNodeMap`、`XMLDocument`、`DOMParser` | `dom_js` 的 `node_tree`、`documents`、`dom_core`、`ranges`、`selections`；`dom_api` 的 `ranges`；情境 `selection`；WPT `dom/nodes/`、`dom/ranges/`、`dom/traversal/`、`selection/`、`domparsing/`；`mutants-4e-2b-*.toml`、`mutants-4h-1a.toml`、`mutants-4h-1b-*.toml`、`mutants-4h-1c-1.toml` |
| custom element：registry、`define`、`whenDefined`、建構和 upgrade、reaction 和 `[CEReactions]`、生命週期 callback、`:defined`，parser 插入時當場建構（template 的內容裡的不建構）；declarative shadow root、`setHTMLUnsafe`、`parseHTMLUnsafe`、`getHTML` | `dom` 和 `dom_js` 的 `custom_elements`；`dom_js` 的 `engine`、`shadow_dom`；`html` 的 `parser`；情境 `custom-elements`；WPT `custom-elements/`、`shadow-dom/declarative/`；`mutants-4e-4*.toml`、`mutants-4e-5*.toml`、`mutants-5d-4b.toml` |
| timer、元素的 task、message 共用一個編號，照 `(時間, 編號)` 跑；window 的 `load` 是元素的 task；idle period 在開始畫之後截到下一個 frame | `dom` 和 `dom_js` 的 `tasks`；`dom_js` 的 `idle::the_time_remaining_is_never_past_the_deadline`；`shell` 的 `modules::the_windows_load_fires_once`、`script::an_idle_period_ends_at_the_next_frame_once_frames_are_drawn`；`mutants-5d-2b.toml`、`mutants-5d-4a.toml` |

**binding**

| 項目 | 怎麼驗證 |
| --- | --- |
| 宣告表每個成員一條專用的路，getter 沒有安全點、method 有；`declare!` 是 proc macro（`dom_api_macros`），表的錯指到那一行；編譯時的 `ACCEPTED`、`NODE_INTERFACES`、`HTML_TAGS`；單型化照成員的種類分派（`TypedVisitor`） | `dom_api` 的 `table`、`document`、`element`、`typed_paths_are_visited_by_their_kinds`；`dom_api_macros` 的單元測試；`mutants-1d.toml`、`mutants-1f-1.toml`、`mutants-2b-3.toml`、`mutants-kind-dispatch.toml` |
| isolate 的狀態放在 V8 的 isolate data slot、只經過 isolate 借（沒有 `RefCell`），狀態就是 embedder heap；ASCII 的字串做成 external one-byte string，`LentString` 借出字元、借出期間 GC abort | `bindings` 的 `isolate_state`、`unified_heap`、`v8_text`；`mutants-1e.toml`、`mutants-1h-3.toml`、`mutants-2h-1a.toml` |
| wrapper：interface 和 prototype 鏈、一個節點一個 wrapper、expando 跟著、receiver 不對是 `TypeError`、scavenge 丟掉的重建、做在 relevant realm；`createElement` 照名稱預測 wrapper；字串的快取 | `dom_js` 的 `wrappers`（`wrappers_are_made_in_their_relevant_realm`、`accessors_are_named`）、`exceptions`、`strings`；`mutants-1g-2.toml`、`mutants-1h-4.toml`、`mutants-4h-4c-1.toml` |
| binding 的形狀：prototype 上的順序、`[Global]`、unforgeable 成員、`[Replaceable]`、indexed 的 query 和 enumerator、`window.event`、`document.all`；回傳 promise 的成員把錯誤變成 rejected promise；`DOMException` 是 Error、能 structured clone | `dom_js` 的 `events::unforgeable_members_of_ancestors`、`document_all::other_collections_are_not_callable`、`exceptions::a_dom_exception_is_an_error`、`blobs::promise_members_reject_what_they_throw`；WPT 的 `idlharness`；`mutants-4i-1*.toml`、`mutants-4i-2.toml` |
| window 是 global，`WindowProxy` 換文件時留著（`detach_global`），同 origin 的第一份文件沿用初始的 window；`Location`、unforgeable 成員在自己身上；event 的時間從目前的 realm 的 window 起 | `dom`、`dom_api`、`dom_js` 的 `window`；`dom_js` 的 `frames::a_frames_window_proxy_stays`；`mutants-2c-1.toml`、`mutants-4g-4c-1.toml` |
| `dom_wasm`：每個 Store 自己的整數 handle（generation、放掉的格子），handle 無效 trap、`this` 不對是 `TypeError`；guest 只綁 `plain` 的成員，WIT 只列它們 | `dom_wasm` 的 `guest`、`wit`（`a_guest_gets_the_plain_members`）；`mutants-1f-2.toml` |

**事件、輸入和 focus**

| 項目 | 怎麼驗證 |
| --- | --- |
| `EventTarget`、`Event` 的建構子和 init dictionary；listener 的加、移除、`once`、passive、capture 和 bubble 的順序；分派在一個操作裡跳過沒有 listener 的節點，path 建的時候算好 shadow-adjusted target；event 和 listener 的 type 存名稱的編號、沒有 finalizer | `dom` 的 `events`（`events_and_listeners_have_no_finalizer`、`an_event_fits_its_slot`）；`dom_api` 的 `event`、`dispatch`（`targets_with_no_listeners_are_passed_over`）；`dom_js` 的 `events`、`shadow_dom::a_dispatch_sees_its_path_as_it_was_made`；`mutants-2b-1.toml`、`mutants-2b-2.toml`、`mutants-4d-2.toml` |
| `on*` 的 handler 屬性：content attribute 第一次要時才編譯、在 listener 裡的位置、`<body>` 轉給 window、window 的 `onerror` 五個參數 | `dom` 的 `a_handlers_content_attribute_sets_it`、`a_handler_keeps_its_place_among_the_listeners`；`dom_js` 的 `handlers`；`dom_api_macros` 的 `an_attribute_may_call_script`；`mutants-4d-3.toml` |
| `CustomEvent`、`createEvent`、`init*Event`、`WheelEvent`、`InputEvent` | `dom_js` 的 `event_interfaces`；WPT `dom/events/`、`uievents/`；`mutants-4d-4a.toml` |
| 滑鼠和鍵盤的事件、`:hover`、`:active`，每個事件之後一次 microtask checkpoint；游標照 hit test、畫面變了時重算指標下的元素；鍵盤捲動從 focus 的元素往外經過 frame | `dom_api` 的 `input`；`dom_js` 的 `ui_events`；`style` 的 `invalidation`；情境 `cursor`；`mutants-4b-2a.toml`、`mutants-4b-2b.toml`、`mutants-5d-1.toml` |
| focus：狀態、`FocusEvent`、`:focus`、`:focus-within`、`:focus-visible`、fixup、滑鼠給 focus、Tab 的循序導覽（展開 iframe）、跨 frame 的 focused frame 和 `hasFocus()`；`focus()` 捲進視野 | `dom_api` 的 `focus`；`shell` 的 `focus`、`focusing_scrolls_into_view_if_needed`；`mutants-4b-4a.toml`、`mutants-4b-4b.toml`、`mutants-4g-4b-2b.toml` |
| 滾輪：wheel listener 的計數、default passive、legacy event type、wheel event handler region，main thread 沒取消才捲；進子 frame、往外層傳 | `dom` 的 `wheel_listeners_are_counted_and_block`；`style` 的 `a_blocking_wheel_listener_styles_its_subtree`；`dom_api` 的 `wheel`；renderer 的 `blocking_wheel_listeners_make_the_region`、`a_wheel_scrolls_on_the_main_thread`、`wheels_and_points_go_into_child_frames`；`shell` 的 `wheel`；`mutants-4d-4b.toml`、`mutants-4g-4b-2a.toml` |
| `requestAnimationFrame` 的 handle、取消、順序，時間從 window 的 time origin 起；每個 window 的 timer、rAF、rendering update 照 tree order | `dom_api` 的 `animation_frame`；`dom_js` 的 `frames`；`mutants-4b-3.toml`、`mutants-4g-4b-1.toml` |

**parser 和 script**

| 項目 | 怎麼驗證 |
| --- | --- |
| html5ever 的 `Sink` 在 DOM 上建樹，parser 在每個 `</script>` 停下；`innerHTML`、`outerHTML`、`insertAdjacentHTML` 的 fragment parse 有 Blink 的 fast path，debug build 和 tree builder 的樹比對；XML parser 建 DOM | `html` 的 `tree_construction`（一般和 `HeapOptions { stress: true }` 各一次）、`parser`、`fast_path`、`xml`（`builds_the_same_tree_as_parse_document`）；`dom_api` 和 `dom_js` 的 `markup`；`mutants-2a.toml`、`mutants-4a-2.toml`、`mutants-4g-3d.toml` |
| classic script：prepare the script element，parser 插入的和 script 插入的，外部的 `async`、`defer`、in-order 的清單、`load` / `error`、擋住 script 的樣式表、`charset`、muted errors；module script、`import()`、`import.meta.resolve`、`modulepreload`，失敗的 fetch 不留在 module map | `dom` 和 `dom_js` 的 `scripts`；`shell` 的 `documents`、`modules`、`inserted_scripts`；WPT `html/semantics/scripting-1/`；`mutants-4h-1d-*.toml`、`mutants-4h-1e-*.toml`、`mutants-5d-2a.toml` |
| 載入的生命週期（`readyState`、`DOMContentLoaded`、window 的 `load`）、`unhandledrejection`、`console`、`queueMicrotask`；timer 照到期和建立的順序、巢狀超過 5 層至少 4 ms；例外在 window 觸發 `ErrorEvent` | `dom` 的 `window`；`dom_api` 的 `timer`；`dom_js` 的 `timers`、`engine`；`shell` 的 `documents`；`mutants-2c-2.toml`、`mutants-2d-1.toml`、`mutants-2d-2.toml` |

**style**

| 項目 | 怎麼驗證 |
| --- | --- |
| `style` 讀 DOM：rule index（document 的分成 `SheetGroup`）、cascade（不讀 DOM 的部分在 `style_resolver`）、祖先的 bloom filter（含 namespace）、presentation 屬性、matched properties cache；root 自己的 `rem` 是初始字級 | `style` 的 `cascade`（`the_roots_own_rem_is_the_initial_font_size`）、`recalc`、`hints`、`matched_properties`、`style_groups`；`mutants-3a-4.toml`、`mutants-5b-3a.toml`、`mutants-5d-3c.toml` |
| invalidation：descendant 和 sibling 的 set、stylesheet 的 invalidation，DOM 只記 style 讀的改變；重算的集合照 selector flags、`sibling_reach` 和查過的狀態挑；新的 viewport 只重算用 viewport 單位的元素 | `style` 的 `invalidation`、`shadow`；`dom` 的 `only_what_style_reads_is_noted`；WPT `css/selectors/invalidation/`；`mutants-4e-3a-*.toml`、`mutants-5d-3b.toml`、`mutants-5d-3c.toml` |
| shadow tree 的 sheet、`:host`、`::slotted()`、cascade 的順序、`adoptedStyleSheets`；沒分到 slot 的子節點從 host 繼承；變了的 sheet 每份 document 一份清單 | `style` 的 `shadow`（`a_child_assigned_to_no_slot_follows_its_changes`）、`adopted`；`dom` 的 `shadow::each_document_lists_its_own_changed_sheets`；`shell` 的 `style_sheets::a_child_frames_changed_sheets_style_it`；情境 `shadow-sheets`；`mutants-4e-3b-*.toml`、`mutants-4e-3c-*.toml`、`mutants-5d-4c.toml` |
| `<link rel=stylesheet>`、`<style>` 和 `media`，擋住 render、alternate 不擋；CSSOM 的 sheet 和 rule、`<link>` 照 CORS 和 origin-clean；`element.style` 是元素唯一一份宣告，`style` 屬性讀時才寫回；`getComputedStyle`、`::selection`、`matchMedia`、`CSS` | `dom` 和 `style` 的 `sheets`；`dom_js` 的 `style_sheets`、`css_style`、`media_queries`；`dom_api` 的 `computed_style`、`css_style`；`shell` 的 `style_sheets`、`link_sheets_from_script`、`computed_styles_and_selection`；`mutants-3c-2.toml`、`mutants-4a-3.toml`、`mutants-4h-2*.toml` |
| 表單的 pseudo-class、`:open` 和內部的 pseudo-class：狀態改了重算自己、`+` 的兄弟和子孫 | `style` 的 `form_states`；情境 `form-pseudo-classes`；`mutants-4f-2c.toml` |
| CSS transition 和 animation、Web Animations（`animate()`、`KeyframeEffect`、被取代的動畫、`commitStyles()`），promise 在事件之前 settle | `style` 的 `animations`、`transitions`；`dom_js` 的 `web_animations`；`shell` 的 `transitions`、`web_animations`、`css_animations`；情境 `transitions`、`css-animations`；`mutants-4h-3b*.toml`、`mutants-4h-3c-*.toml` |
| selector 查詢：id、tag、class 和屬性的 compound 一個一個比，比不了的在找到的元素上完整比對；子樹的 bloom filter 缺 compound 和之後的要的位元時跳過（不看 `style` 屬性，quirks mode 不看 class） | `style` 的 `query`（`compounds_match_as_the_full_match`、`a_bloom_filter_skips_the_subtrees_without_what_it_takes`、`a_lazily_written_style_attribute_is_found`）；`dom_js` 的 `queries`；`mutants-4a-1.toml`、`mutants-wrap-up-compounds.toml`、`mutants-wrap-up-bloom.toml` |

**layout 和畫面**

| 項目 | 怎麼驗證 |
| --- | --- |
| `layout::tree_builder` 從 DOM 建 box tree（block、inline、文字、table、flex 和 grid 的 anonymous item、list、counter、`display: contents`、generated content），增量更新的 `built`、`shaped`、`restyled` 次數守著，結果和從頭建的一樣 | `layout` 的 `update`、`lists`、`generated`、`tables`（`columns_are_sized_from_their_cells`）；`mutants-3b-*.toml`、`mutants-last-b.toml` |
| `FrameView` 跑完整的 lifecycle，有變的時候才 layout、重畫；照 flat tree layout、畫出 shadow tree；`innerWidth` 只 layout 需要的 frame，script 插入的 iframe 立刻有 view | renderer 的 `frame_view` 的單元測試；`shell` 的 `documents`、`geometry`、`webdriver::inserted_frames_documents_are_their_own`；情境 `shadow-slots`；`mutants-3c-1.toml`、`mutants-4e-2c-2.toml`、`mutants-5d-3a.toml` |
| script 讀 geometry 經過 `FrameLayout`；script 的捲動、`scrollIntoView` 經過 frame、`scroll` 事件；scroll anchoring | `dom_api` 的 `geometry`、`scroll`；`dom_js` 的 `scrolling`；`shell` 的 `script_scrolls_and_its_scroll_events`、`scroll_into_view_through_frames`、`scroll_anchoring`；情境 `scroll-anchoring`；`mutants-4b-1.toml`、`mutants-4h-3a*.toml`、`mutants-5d-3a.toml` |
| 子 frame 的 sticky 用整棵 frame tree 的 pre-paint；系統顏色；replaced 元素裡不算 counter | renderer 的 `a_child_frames_sticky_box_is_painted_again_where_it_sticks`、`system_colors_are_the_engines`；`layout` 的 `a_replaced_element_has_no_marker_nor_counters_in_it`；reftest `frames/f11-child-sticky.html`；`mutants-4i-4.toml` |
| caret 在 editing host 裡第一個能放的位置，每個 run 只算第一個 cluster | `layout` 的 `caret`（`a_runs_first_cluster_start_is_its_clusters_firsts`、`the_carets_first_place_works_out_no_clusters`）；`mutants-wrap-up-caret.toml` |

**表單和控制項**

| 項目 | 怎麼驗證 |
| --- | --- |
| interface 的分類和反映、form owner、`form[name]` 和 past names map、`<label>`；activation、checkbox 和 radio group、disabled 的快取；input 的 value mode、`valueAsNumber`、`stepUp()` | `dom_js` 的 `forms`、`activation`、`form_values`；WPT `html/semantics/forms/`；`mutants-4f-1*.toml`、`mutants-4f-2a.toml`、`mutants-4f-2b.toml`、`mutants-4f-5.toml` |
| 文字控制項：值、選取 API、打字、刪和移動、`maxlength`、`change`，caret 和選取的繪製，WebDriver 的 Send Keys 和 Clear | `dom_js` 的 `text_controls`；`dom_api` 的 `editing`、`controls`；情境 `caret`、`caret-inactive`、`demo-input`；`mutants-4f-3a.toml`、`mutants-4f-3c-*.toml` |
| UA shadow tree、part 的規則、控制項的 layout 和 native look（`CONTROL_STATE_CHANGED` 只重做 fragment）；`<select>` 的 selectedness、`HTMLOptionsCollection`、popup、list box | `dom` 的 `ua_shadow`；`style` 的 `parts`；`dom_js` 的 `select`；`dom_api` 的 `controls`；`shell` 的 `browser_view`；情境 `demo-forms`、`list-box`；`mutants-4f-3b.toml`、`mutants-4f-4*.toml` |
| `contentEditable`、editing host 的 focus 和初始的選取、裡面的游標 | `dom` 和 `dom_js` 的 `editing`；`shell` 的 `selections`、`keys`；情境 `caret-editing-host`；`mutants-4h-1c-2*.toml` |

**圖片、canvas、SVG**

| 項目 | 怎麼驗證 |
| --- | --- |
| `<img>` 的 current 和 pending request，list of available images 每筆記著用它的文件，`decode()`、`srcset`、`sizes`、`<picture>`、`loading=lazy`、`sizes="auto"`、背景圖 | `dom` 的 `images`（`an_image_leaves_the_list_with_the_documents_that_used_it`）；`dom_api` 和 `dom_js` 的 `images`；`shell` 的 `images`、`lazy_images`；reftest `backgrounds/b6-slow-image.html`；`mutants-4g-1*.toml`、`mutants-5d-4c.toml` |
| IntersectionObserver 和 ResizeObserver：target 撐著 observation、observer 的清單是 weak 的，跨 frame 的交集在最上層的 viewport 裡算 | `dom` 和 `dom_api` 的 `intersection`、`resize`；`dom_js` 的 `intersection_observers`、`resize_observers`；`shell` 的 `intersection_observers`、`resize_observers`；`mutants-4g-1b-2*.toml` |
| canvas 2D（狀態、路徑、`ImageData`、漸層、陰影、`drawImage`、圖案、origin-clean、`Path2D`、文字），`toDataURL`、`toBlob` 在編碼 thread 上，`Blob` | `canvas` 的單元測試；`dom_js` 的 `canvas`、`blobs`；`shell` 的 `canvas`（`exported_as_files`）；reftest `canvas/`；`mutants-4g-2*.toml` |
| SVG：外層的 `<svg>` 是 replaced 的 box、presentation 屬性、`<use>` 的 copy 和循環、資源、幾何的 API、SVG 圖片 | `layout` 的 `svg`；`dom` 的 `svg_use`；`dom_js` 的 `svg`、`svg_geometry`；renderer 的 `an_svg_images_uses_are_drawn`；reftest `svg/`；`mutants-4g-3*.toml` |

**frame、導覽和其他 Web API**

| 項目 | 怎麼驗證 |
| --- | --- |
| frame tree：子 browsing context 在 reactions 之前建、每個一個 realm、security token 照 origin，子 frame 的 view 照內容框、父的 `load` 等子 frame；初始的 about:blank 在 `Created` 時做成，srcdoc 和 about: 的導覽在 task 之間 commit | `dom` 和 `dom_js` 的 `frames`；renderer 的 `a_child_frame_is_laid_out_at_its_iframes_content_box`、`a_child_frame_whose_rendering_is_blocked_waits`；`browser` 的 `navigation::a_srcdoc_document_commits_with_nothing_else_to_wake_the_renderer`；reftest `frames/`；情境 `iframe-loads`；`mutants-4g-4a*.toml`、`mutants-5d-2a.toml`、`mutants-5d-2c.toml` |
| `postMessage`、`MessageChannel`、`MessagePort`；跨 origin 的 `Window`、`Location` 照 CrossOriginProperties；子 frame 的 `location` 導覽 | `dom_js` 的 `messages`、`cross_origin`；reftest `frames/f9-child-location-fragment.html`；`mutants-4g-4c-2.toml`、`mutants-4g-4c-3a*.toml` |
| 導覽：`Location`、`History`、fragment、`:target`、`popstate`、`hashchange`、`javascript:` URL；base URL（about: 的文件照 container 的）、連結照 `target` 選 frame、`document.title` | `dom_js` 的 `history`、`links`；`dom` 的 `frames::about_documents_base_urls`；`shell` 的 `history`；`mutants-4h-4a*.toml`、`mutants-4h-4b*.toml` |
| HTML 元素的 interface、window 的具名屬性、`Navigator`、`UserActivation`（經過 frame tree）、autofocus；Content-Language 和 `:lang()` | `dom_js` 的 `html_elements`、`window_named`、`navigator`；`dom` 的 `language`；`shell` 的 `navigator`、`the_user_activation_goes_through_the_frame_tree`、`network::content_language_comes_from_the_response`；`mutants-4h-4c-*.toml`、`mutants-4i-3.toml` |
| `URL`、blob URL、`crypto`、`structuredClone`、`File`、`FileReader`、`performance` 和 navigation timing、idle callback、geolocation、wake lock、`AbortSignal`、`Headers`、`Request`、`Response`、`fetch()`（呼叫當下就送，拿掉的 frame 的取消） | `dom_js` 的 `urls`、`files`、`performance`、`idle`、`navigator_apis`、`abort`、`headers`、`fetch`；`shell` 的 `blob_urls_load_until_revoked`、`navigation_timing`、`fetch`、`file_reader`；`mutants-4h-5*.toml` |

**DevTools、WebDriver 和自動化**

| 項目 | 怎麼驗證 |
| --- | --- |
| machopodriver 驅動 DOM（script、reference、Find、click、action、截圖、stale），子 frame 裡（`FrameId`、Switch To Frame、座標換到最上層的 viewport），先捲進視野 | `dom_api` 和 `dom_js` 的 `webdriver`；`shell` 的 `webdriver::the_document_is_driven`、`frames_are_switched_to_and_from`、`elements_out_of_view_are_scrolled_to`；情境 `frames`；wdspec `switch_to_frame`、`switch_to_parent_frame`；`mutants-4c-1.toml`、`mutants-4g-4c-3b*.toml` |
| DevTools：`Page` 的 lifecycle（title 在 `load` 之前送）、子 frame 和 network idle；Runtime、Debugger、console 和例外、暫停和打斷；`Network` 和 initiator（parser 的行號、script 的 stack） | `browser` 的 `navigation::the_title_goes_before_devtools_hears_the_load`；`dom_js` 的 `devtools`；`dom` 的 `initiators`；renderer 的 `task_queue`、`initiator`；`shell` 的 `devtools_lifecycle`、`devtools_network`、`devtools_initiators`、`the_child_frames_are_told`；`mutants-5c-1.toml`、`mutants-5c-2.toml` |
| `kun --run-web-tests` 一行一筆 JSON；情境的已知失敗照清單判斷（清單外的失敗、清單上全過的、清單上沒有的情境都算失敗）；`WebDriverQuery::Probe` | `shell` 的 `web_tests::a_screenshot_is_written`、`windowless`（`a_scenario`）；`dom_api` 的 `a_probe_finds_the_first_match`；`test.py scenarios`；`mutants-2e.toml`、`mutants-4c-3b.toml`、`mutants-5a.toml` |

**工具和量測**

| 項目 | 怎麼驗證 |
| --- | --- |
| `bench.py calls` 數 store forwarding 的衝突，空的迴圈跑 `EMPTY_RUNS = 3` 次取中位數；blink-perf、`bench.py gc` 讀 trace 的 GC | `calls_test.py`、`blink_perf_test.py`、`gc_bench_test.py`；`mutants-1e.toml`、`mutants-1h-5.toml` |
| trace 的 span 都經過 `trace_event` 的 macro（沒開 trace 時不建），`sanity.py spans` 擋直接用的；script 的 slice、main frame 的步驟、`Gc.*` 算進 GC | `trace_event` 的 `spans_are_made_only_at_the_levels_the_trace_takes`；`speedometer_test.py`（`MeasureTests.test_traces_of_both`）；`mutants-wrap-up-trace.toml` |

## 做過的檢查

2026-10-27 到 11-06 都在同一台上做和量：Linux 7.2、Fedora 44，AMD Ryzen AI 9 HX 370（Framework 筆電，4 顆 Zen 5、16 顆 Zen 5c，62 GB），插著電源、`systemd-inhibit` 擋住休眠，量測綁在 Zen 5 上（CPU 0–3、12–15）；kun 的 `perf` profile，Chrome for Testing 152.0.7977.82。動到 fork 的 C++ 之後用 `RUSTY_V8_ARCHIVE` 連結本機編的 V8，最後-d 起連結 release `kun-v152.2.0-27`。mutation test 共 200 份 spec、4582 條，`test.py mutate --restore` 每次都沒有要還原的。

### 第 1 段：新的 DOM 的地基（1a 到 1h）

**量到的**
- 1a：kun 和 Chrome 最佳化後的迴圈指令一模一樣（`CallApiCallbackOptimizedNoProfiling`），路上沒有 V8 的 C++，照決定 7 不重編；`c1c62f0` 的地板裡約 0.8 ns 是 rusty_v8 的 C++ shim，約 2.3 ns 是 store forwarding 失敗（fork 的 `set_internal` 寫 4 bytes、builtin 讀 8 bytes），整格寫入之後空的 getter 5.43 → 3.16 ns。今天的 DOM 的讀取是 Chrome 的 2.7 到 9 倍，差在 kun 自己的 binding 和 DOM。
- 1e：fork 整格寫入之後今天的 DOM 的 `nodeType` 173 條指令、每次呼叫 2.26 次衝突（1a：170 條、2.8 次）。
- 1g-2：新的 DOM 的 method 少 2.6 到 3.4 倍（`appendChild` 4013 → 1176、`createElement` 8652 → 2567 條），getter 的指令比今天的多（`nodeType` 225 對 172，cycles 47 對 42）；`tagName` 名稱表記住之後 4152 → 467 條、22.6 → 0.12 次衝突。
- 1h：照 Speedometer 的呼叫分布加權（15 個成員）新的 DOM 56.2 ns、Chrome 36.9 ns，**1.52 倍**、今天的 4.14 倍（0088 最後 1.09 倍）；getter 2.7 到 3.2 倍、建節點 2.5 到 3.2 倍，method 0.71 到 1.19。
- 1h-2 → 1h-3 → 1h-4：加權 1.23 倍（45.4 ns）→ 1.10 倍（40.5 ns，`setAttribute` 0.91）→ **1.09 倍**（40.1 ns），決定 28 的目標達成；getter 2.0 到 2.6 倍、建節點的 store forwarding 衝突 6 到 9 次沒有達到，上限在 V8 那一側，移到「之後的可能」。
- 1h-2：`nodeType` 的地板，手寫的 callback 186 條指令、36 cycles，typed 的路徑 210、40（Chrome 約 18 cycles），IPC 已經約 5，卡的是指令數。
- 1h-3：一個字串參數約 540 條指令、100 到 150 cycles，大頭是 V8 的 `ValueView`。
- 1h-5：young GC 全部是 V8 的 scavenge，76% 在 `WEAK_GLOBAL_HANDLES_PROCESS`（每個死掉的 wrapper 一次 `ResetRoot`、main thread 上一個一個做）；`create-element` 的 65 ms 停頓是 major GC 的清除（`SWEEP_EXTERNAL_POINTER_TABLE` 108 ms），major GC kun 4 次、Chrome 509 次。
- 1h-6：門檻和平行重設之後 `create-element` 的 young GC 2102 → 0 ms、最長的停頓 65 → 5.9 ms（Chrome 7.9）；`gc-tree` 的停頓 41 對 Chrome 42 ms；扣掉 GC 之後 Chrome 還快 1.5 到 1.9 倍，剩下的在建節點和 wrapper。
- 1b：`gc_bench` 對 `main` 每個工作的總時間 −5.9% 到 +1.2%，沒有量得出來的 regression。1f-1：proc macro 前後每次呼叫的指令差 0 或 1 個。
- 1f-3：wasm 文件裡新的 DOM 5 頁快 2.1 到 3.5 倍；`nodeType` 1656 條指令幾乎都是 wasmtime 的 host 呼叫本身（JS 211），wasm 每次呼叫 36 到 97 次衝突，記進「之後的可能」（第 2 段以後只做 JS）。

**找到、修好的錯**
- 1d：`[LegacyNullToEmptyString]` 把 undefined 也當成空字串，`Front` 加上 `legacy_null_string_arg`。1g-1：`post_job` 裡巢狀的 `fn task` 蓋過了參數，`Box::new(task)` 裝的是那個函式（warning 抓到）。
- 1g-2：`set_isolate_state_with_embedder_heap` 只檢查同一個型別，改成註冊之前檢查那一格有沒有任何狀態；interface 物件的 `[[Prototype]]` 和 prototype 上的 `Symbol.toStringTag` 照 Blink 設；`dom_js` 用 `static` 記 `MACHOPO_NEXT_DOM`，`sanity.py boundary` 擋下，改成每建一個 realm 讀一次。
- 1h、1h-2：`id`、`className` 沒有那個屬性時每次配置空字串（改用 `names::NONE`）；慢的真正原因是空字串的 `memcmp`：glibc 2.43 的 `__memcmp_evex_movbe` 對空 slice 的指標 `0x1` 做遮罩 load，Zen 5 走 microcode assist（`n.id` 94% 的時間在那一條指令）；`Names::find` 對空字串直接是 `NONE`，glibc 的回報草稿 `glibc-memcmp-report.md` 還沒送出。
- 1h-6：V8 照 GC 之前的 embedder heap 大小算下一次的門檻（61 MB 之後是 242 MB），`EmbedderMarking::End` 改成回傳存活量（決定 31、32）。1f-2：wasm 的 `()` 的結果把 setter「沒交」也當成 `undefined`。

**不穩定的測試**
- 1b：`concurrent.rs` 的 `markings_run_alongside_on_a_pool` CPU 忙時偶爾失敗，改成等到一次 marking 做完。
- 1g-1：增量 marking 的鏈測試，`WeakPersistent::get` 的讀取 barrier 讓鏈頭撐過那一輪（約八次一次），改成等兩輪才問；並行 1000 次失敗 4 次，是 `CppHeap::EnterFinalPause` 保守地掃 native stack，舊的 `TracedReference` 值讓 wrapper 多活一輪，改成斷言前先做一次 `NoHeapPointers` 的完整 GC（之後 2000、500、400 次都過）；20 次 churn 裡 marking 沒結束，改成最多 200 次。
- 1g-2：沒有碰到（`it`、`marking` 各 1000 次，所有核心都忙著時各 300 次）。

**mutation test**（1b 39 條、1c 33、1d 32、1e 15、1g-1 37、1g-2 36、1h-2 17、1h-3 11、1h-4 4、1h-5 7、1h-6 8、1f-1 19、1f-2 28、1f-3 10）
- 補了測試的：1b 的 6 條（5 個測試和 1 個斷言）；1c 表的門檻；1e 死掉的 key（單元測試、比參照的數量）；1g-1 `drained`、「local 一律是空的」（`unified_heap_linear`）；1h-2 命名空間裡先出現的屬性、inline unwrap 不看 instance type（table 前 2^20 格）、`on_thread`；1h-4 `set_weak` 快取命中；1h-5 最長停頓只看第一次；1f-2 放兩次 handle、共用的表清空；1f-3 `next_calls`、`--gc` 預設的清單。
- 等價、拿掉或記進 `docs/test-gaps.md` 的：1c 空的命名空間過濾、1e 空字串 external（多餘的程式拿掉）；1g-1 drop 的 handle（`Drop` 和 `detach` 拿掉）、patch 的 `IsMarkingDone`；1g-2 driven 的一步一律說做完、名稱表的快取；1h-4 `return_wrapper`、`return_cached` 一律回 `false`；1h-6 平行重設一律回 false；1f-1 的 2 條、1f-2 的 1 條。只靠審查：1b `retire` 不等進行中的交接。1h-6 的 V8 patch 和 glue 的 C++ 沒做 mutation（每條要重編 V8），由 JS 的測試守著。

**試過、改掉的做法**
- 1b：一個型別加 `driven` 的旗標，不照 cppgc 分成兩個型別。1e：`StringCache` 的 finalizer 只記下死了、下一次用時才拿掉（決定 17 的「callback 裡拿掉」改掉）。
- 1g-2：front 把整個結果記下、`finish` 一次讀出，`DomString` 的 `bool`，clone 字串結果，都造成分段寫、整格讀，改成操作裡就寫、一整格的 `flags`、借出（決定 26）；參數直接讀進自己的 buffer（丙）更糟、拿掉。
- 1h-2：`finish`、`unwrap_object` 加 `#[inline]`，拿掉。1h-3：在操作裡開 `ValueView` 借（較慢，「禁止 GC」scope 的順序讓 `createElement` abort）、複製進留著的 buffer（持平），都拿掉；`ArgString` 的 `OnceCell<enum>` 改成 `LentString` 的 `Cell<bool>`。1h-4：`gc` 的配置游標拆成三個 `Cell<usize>`，沒變、拿掉。

**量測和工具的問題**
- 1a：用電池、休眠時量的數字不能用，之後每次跑 kun 前檢查 `ACAD/online`；`perf_event_paranoid` 是 2，AMD 的取樣 skid 很大，結論靠版本之間的差、指令數和反組譯。1b：Zen 5 和 Zen 5c 差很多，綁在 Zen 5 上（`common.fastest_cpus`）。
- 1c：mutation 讓 `next_sibling` 指向自己，測試長到 53 GB、機器 OOM 兩次；測試的走訪超過上限就 panic，測試指令加上 `systemd-run --user --scope -q -p MemoryMax=24G -p MemorySwapMax=0`。
- 1g-1：rust-analyzer 沒有 `RUSTY_V8_ARCHIVE`、共用 `target/debug`，下載的 release 蓋掉 `librusty_v8.a`，要 `cargo clean -p v8`；帶著 `RUSTY_V8_ARCHIVE` 或用自己的 target 目錄就不會再蓋。
- 第 1 段收尾：clippy 修了 6 處（`TracedSink::local` 的 `// SAFETY:` 位置、兩個 `const fn` 和測試的 `fetch_add` 的 `#[expect]` 等）。

### 第 2 段：parser、事件、window 和 testharness（2a 到 2h）

**量到的**（`bench.py calls` 每次呼叫的指令數；blink-perf 的新/Chrome 是時間的比）：
- 2b-2：`addEventListener` 2516 → 1071、`dispatchEvent` 11715 → 3445（今天的 DOM 對新的），少 57% 和 71%；其他成員比 1f-3 多（`nodeType` 224、`appendChild` 1269、`createElement` 2377 對 211、1111、2285）。
- 2b-3：`typed::accepts` 和節點的 wrapper 的 interface 改成編譯時的表、`names::TEMPLATE`、callback 存在一格、`#[inline]`：`nodeType` 220、`appendChild` 1074、`createElement` 2313、`addEventListener` 885；剩下的 `nodeType` 多 7、`createElement` 多 27 是 codegen 層級的差。
- getter 拆開量：V8 的地板 13.6 cycles，Chrome 整個 `nodeType` 約 18（3.62 ns）；kun 多約 30 cycles 都在自己的步驟，上限不在 V8（決定 28 的第 6 條更正）：`Front` 和 typed 那一層 12.8、讀取的 barrier 6.1（31 條指令）、借 `Dom` 和開操作 5.4、拿 isolate 的狀態 3.8；手寫的完整版 31 cycles 是 Chrome 的 1.7 倍，現在的 43.8 是 2.4 倍。
- 事件頁：`events-dispatching` 2.55、`-listeners` 0.92、`new-event` 1.92；走 path 慢是每個節點各開一次操作（放進 2h-1，決定 44）；`new Event` 是 `dict_bool` 每個成員新建 internalized 的 key（2h-2），另一半在建 wrapper（2h-3）。
- 2c-2：2c-1 每個成員多的 13 個指令是 `Kept` 的 drop（`ObjectRoot` 多了 variant 後 drop glue 0x57 → 0x12f bytes），sequence 改存 `Rc<[…]>` 追回；`finish` 拆成 inline 的檢查和 `finish_kept` 再少約 20（`nodeType` 221→198）；`dispatchEvent` 還比 2b-3 多約 90，約 50 沒找到、留給 2h-1；`invoke_listeners` 拆出後兩頁回到 2b-3 的 97% 和 98%。
- 2c-3：`node-type` 快了 19%（2c-2 拆 `finish`）；`get-elements-by-tag-name` 是 Chrome 的 13.4 倍：沒有快取，每次 `length` 和 `[j]` 都從 root 走（Chrome 的 `CollectionIndexCache`）。
- 2e：`dom/nodes` 390 個測試：兩邊都過 541、只有新的過 6、缺口 14022、兩邊都不過 1036；缺口多半是還沒有的成員（selector、`compareDocumentPosition`、`classList`、`CharacterData` 的方法）。
- 2h-1a：`nodeType` 198 → 184 條指令（37.6 → 34.7 cycles），狀態的讀取只剩兩條指令；getter 頁快了 10% 到 16%（Chrome 的 2.4 到 2.6 倍 → 1.8 到 2.2 倍）；記下 Chrome 的基準（Chrome for Testing 152.0.7977.82）。
- 2h-1b：`V8Op` 是四個 word、`Option<Box<Kept>>`、`throw` cold：`nodeType` 151 條、29 cycles（決定 44.3 的第一個目標 31 以下到了），頁是 Chrome 的 1.59 倍，其他 getter 1.8 到 1.9 倍；`new Event` 慢約 4.5%、沒有單一熱點，留到第 4 條；`appendChild` 0.58 是還沒有改樹的通知。

**找到、修好的錯**：
- 2b-2、2b-3：worktree 共用 target 目錄時 cargo 拿了別的 worktree 的 `gc`、`dom_next`，`target/perf` 也被汙染：每個 worktree 用自己的 target 目錄。
- 2c-3：`interface_of` 讀名稱、查表後 inline 進 getter（`firstChild` 240）：拆成 cold 的 `keep_node`。
- 2h-1a：建元素的兩頁慢了 14% 到 16%：`prepared` 整塊搬進 `V8Op`，不能從分段的 store forwarding；改成借 `&mut self.prepared`，在 1% 以內。rusty_v8 的 `ReturnValue` 有不要 scope 卻會建 heap number 的 setter：操作裡只經過 `OpReturn`。
- 第 2 段收尾：clippy 修 4 處（落空的 `#[expect]`、測試沒看第一次分派的結果、renderer 的 time origin 改 checked（以前不是數字會 panic）、`is_none_or`）；`cargo fmt` 只有排版。

**不穩定的測試**：沒有。2b-2、2c-2：GC 時機的測試重複跑（閒置 1000 次、24 核佔滿 300 次）都沒有失敗，2b-2 那幾輪補上 2b-1 漏跑的 `events_and_targets_are_collected_with_their_wrappers`；2c-3 起照決定 46 在 2h 做完時才重複跑。

**mutation test**（2a 62、2b-1 28、2b-2 42、2b-3 6、2c-1 19、2c-2 38、2c-3 25、2d-1 19、2d-2 21、2e 10、2h-1a 14、2h-1b 3 條）：
- 2a：沒抓到的 9 條，7 條補了測試（XML 的 template 文件、comment、doctype 之前的位置、adopt、clone 的 quirks mode、`annotation-xml`），2 條等價（`same_node` 只比 `Rc`、fragment parse 不理會 `set_quirks_mode`）。
- 2b-1、2b-2：wrapper 參照每次新建、path 重設不先清空、dictionary 的 `capture`、trusted、callback 存成 droppable（補了用元素的 wrapper 當 callback）都補了測試；`for_each_member!` 的遞迴改成「generic 的參數不看」；undefined 的成員、`double` 的 -0 等價。
- 2c-1 到 2d-1：document 的空 URL、strict mode 的 timer callback、`id` 是空字串的 `namedItem`、HTTP 404 的 script 補了測試；`window.filter(|_| false)` 取代編不過的 match guard。
- 2h-1a：狀態比 registration 先放掉、回報的例外沒存進去補了測試；「heap 一律回報標完」等價（V8 不看回傳值）；`NoGc`、`OpReturn` 不配置只靠審查。
- 只影響效能的（表、`Rc`、`finish`、`invoke` 的拆法、cold、`#[inline]`、box、借指標）記進 `docs/test-gaps.md` 的「沒有計數守著的優化」。

**試過、改掉的做法**：
- 2b-3：`Kept` 回到三個 variant、`forget` 空的 front：`nodeType` 多 5 到 10，還原。
- 2c-2：`call_listener` `#[inline(always)]`（+46）、`#[inline(never)]`（+23）、`dispatch` `#[inline(always)]`（+12）等都沒用或更糟，還原。
- 2c-3：`Element` 一律先做 `HTMLElement` 的 wrapper 讓 `create-element-ns` 慢 16%，改成照成員寫（`NewObject<I, L>`）。
- 2h-1b：字串參數先問會不會借字元：`setAttribute` 269 → 252 cycles，但字串參數的成員多約 20 條指令，還原、留到 2h-2；`RetType` 的關聯常數沒有做（`prepare` 本來就消失）。

### 第 3 段：名稱、字串、style、layout 和 frame（3a 到 3d）

**量到的**
- 3a-1：`bench.py blink-perf --next-dom`：`calls/new-event`、`node-name`、`tag-name` +14%（2h-1b 的 −4.5% 追回來了），`calls/get-attribute` −7%（先查表成編號再比整數，量的元素只有一個屬性；Blink 也先轉成 `AtomicString`）。
- 3a-2：快取沒中的 `getAttribute` +25%、+22%，`set-class-name` +15%；`new-event` −5%（每個 event 約 12 ns：event 的 type 是 `LocalExternal`，finalizer 回到主 thread）：使用者同意先接受，type 改成名稱的編號放進「之後的可能」。
- 3a-3：`appendChild` 多 165 條指令、cycles 多 14%（dirty bit 本身的工作，Blink 的 `InsertedInto`、`RemovedFrom`、`ChildrenChanged` 也做）；`setAttribute` 多 72 條；名稱一個編號沒有變慢，不用退回 B。
- 3a 整段：`new-event` +12%、`node-name`、`tag-name` +10%、`set-class-name` +8%；`get-attribute` −5%，`append-child`、`insert-before`、`set-attribute` −3%（dirty bit 的版本計數和 connected 的檢查）。
- 3c-3：大頁 style 新的快 31%（8.81 → 6.09 ms），box tree 快 6%（18.59 → 17.41）；layout、pre-paint、paint 是同一份程式，差在誤差裡；sqlite 的截圖不一樣（沒有圖片、`getElementById`），數字只能參考。3b 照決定 59.5 不量。
- 3d：`reftest-next` 的第一份對照：6016 個 reftest，兩邊都過 3963、只有新的過 62、缺口 1262、兩邊都不過 729；缺口是圖片、SVG、shadow DOM、`::selection`、動畫和 script 的 API，都在第 4 段；沒有找到新的 DOM 畫錯的。

**找到、修好的錯**
- 3b-2：`style_next` 的祖先 bloom filter 沒有放元素的 namespace，UA 裡有祖先的規則在新的 DOM 上一條都沒套到（差分比對的巢狀 list 高了 16px）：`bloom.rs` 加上 namespace 的 hash。
- 3b-3：`<body>` 的 `link`、`alink` 沒有成為 document 的 link 的顏色（差分比對的 `b5-legacy-body` 找到）：`text_link_colors`。
- 3b-1：差分比對第一次 73 頁不一樣，是 `map_nodes_for_testing` 漏了 `TextSource` 的節點編號。
- 3d：`wpt_next.py` 只結束 wptserve 的父程序，留下的 server 佔著 port（3c 的 `wpt-next` 結果照樣有效）：server 自成 process group、`port_taken`；排除的目錄寫成 `wasm/` 沒有作用，改了。`text/lato-smoke.html` 的 mismatch ref 不存在，沒有改，交給使用者決定。

**mutation test**（`mutants-3a-1` 15、`3a-2` 3、`3a-3` 27、`3a-4` 21、`3b-1` 24、`3b-2` 54、`3b-3` 15、`3c-1` 13、`3c-2` 24、`3d` 14 條，補了之後都抓到）
- 等價的，記進 `docs/test-gaps.md`、從 spec 拿掉：3a-1 固定的名稱轉大寫經過表（連同 hash、整塊讀取、`const` 初始化）；3a-4 author 的 rule 排在 UA 的之後（不同的 origin 先比 origin）；3b-3 hint 的 digest 都一樣（cache 照值比 declaration）；3c-1 box tree 每個 frame 重建（fragment 照樣共用）。
- 只靠審查：3c-1 compositor 捲的量不拿回來、commit 時 viewport 的 offset 給 0，等第 4 段有輸入時用 click 之後的 frame 守。
- 補了測試的：3a-3 已經 style 過的節點搬動之後要重算；3a-4 大寫的 tag（`EM`）、`+` 和 structural 分頁測（`:nth-child(2)`）、root 的 font size 中間有一層；3b-1 盒子的數目、用 DOM 的操作做的 root 不是 `html`；3b-2 每個節點的 `block_of`、移到另一份 document、`display: contents` 裡的 reset、containment；3b-3 不在 row 裡的 cell；3c-1 沒有 `vw` 的頁、慢的什麼都不做的 script；3c-2 `text/css` 的 404。
- 3a-2：`DomString` 是 `Send` 不是 mutant，換成 `Arc` 就 build 不過。

**試過、改掉的做法**
- 3a-1：`phf`（SipHash-1-3，`createElement` 多約 250 條指令、cycles 多 15%）改成 `build.rs` 自己產生的 CHD 加 FxHash 和 murmur3 的收尾；名稱的尾巴抄進 `[u8; 8]`（store forwarding 失敗，`find_fixed` 佔 27%）改成照長度重疊讀 4 或 8 bytes；轉大寫的 `nodeName` 經過 thread-local 的表（慢 2%）改成編譯時產生，快 14%。
- 3a-3：插入和移除各自標五次（多 215 條、cycles 多 20%）改成旗標一次設好、祖先一起走一次。
- 3a-4：`class` 一變就重建 class 清單（`set-class-name` 252 → 120 runs/s）改成只設一個 bit、style 讀時才建（273）。

**其他**
- 3a-4：`rustc` 1.99 的 incremental 快取兩次讓 compiler panic，`cargo clean -p` 那個 crate 之後就好。clippy、`cargo test --workspace`、`cargo fmt --check` 照決定 53 延到 RFC 收尾（3a-2 的 `unified_heap.rs` 的 `mut_from_ref` 也是）。

### 第 4 段：4a 到 4e

**量到的**
- 4a-1：第一版 `appendChild` 多 143 條指令、cycles 多 15%（插入、移除時每個元素都找 `id`）；元素有 `HAS_ID` 才找之後多 9 條、cycles 多 2%。第一版 `set-class-name` 慢 11%（270 → 242）；`HAS_ID` 只在 `attribute_added` 設、id 表只在連上 document 的分支更新之後 275。
- 4a-2：fast path 讓 `parser/inner-html-setter` 998 → 6211 runs/s（Chrome 10127）、`tiny-inner-html` 9.45 → 14.8（Chrome 26.8），照決定 65.2 搬了；`inner-html-setter` GC 佔 25%。
- 4a-3：`css/css-property-setter-getter` 7252 runs/s，Chrome 22983（0.32 倍），記在「待辦」。
- 4a-4：`calls/set-attribute` 181 → 170（6%），試過三種改法都沒變回來，還沒有解釋，記在「待辦」；`child-list-mutation-with-unrelated-observer` 697 runs/s，Chrome 357（快 1.95 倍）。
- 4b-1：第一版 `setAttribute` 的 cycles 多 30%（332），layout 的 cell 放進自己的 box 之後 245；推測是 CPU 的記憶體相依判斷，沒有用 perf 證實。
- 4b-2b：新加的 `dict_string` 讓 `appendChild` 的 cycles 205 → 272、`dispatchEvent` 698 → 792（指令數一樣），標 `#[cold]` 之後 206、700；推測是程式的配置。
- 4c-1：`calls` 每個比記錄少 12 到 15；4M 次的量法看出每個 process 固定多約 500 萬個指令，原因沒找到，記成未解釋的變化。
- 4d-1：`events/events-dispatching` 1867 → 2597（+39%）、`-listeners` 1731 → 2393（+38%）；`dispatchEvent` 3475 → 3400。
- 4d-2：`addEventListener` 876 → 938（每次 intern `'x'`），記在「待辦」；`Event` 72 → 48 bytes。
- 4d-3：屬性的 hook 讓 `setAttribute` 1239 → 1258，試過的改法最少多 19，記在「待辦」。
- 4e-1：第一版 `appendChild` 1214 → 1449、`dispatchEvent` 3405 → 3508；cold 的走進 shadow tree、`set_shadow_scope` 的快路、`invoke_pass` 兩個單態化的版本之後 1255、3436。`insert-before` 慢 6%、`owner-document` 慢 2%（`document_of` 的分支），記在「待辦」。量法超過約定，之後照：每段一輪 `calls`，blink-perf 只量 3 到 4 頁、各一輪。
- 4e-2b-1：`get-elements-by-tag-name` 176 → 138（`collection_matches` 不 inline），class 的比對拆成 `#[inline(never)]` 的 `class_matches` 之後 175；`appendChild` 1271 → 1312，host-including 那一步改成走到 shadow root 才跳到冷的函式之後 1266。
- 4e-2c-1：`appendChild` 1255 → 1311、`set-class-name` 3%，記在「待辦」。
- 4e-3a-3：一次性的計時，1000 個元素：沒有 rule 用的 `data-x` 25 對 748 µs/frame、沒有 rule 用的 class 61 對 1171、有 rule 用的 class 1167 對 1137、`title` 759 對 766（開對關），留著（決定 86.3）。
- 4e-3b-2：shadow tree 1001 個元素、改 `<style>` 的文字 1168 → 199 µs/frame。
- 4e-4a-1：`Call` 多三個欄位時每個成員多約 23 條；併成 `Option<Box<RareCall>>` 之後回到 1350、1269、2326。
- 4e-4a-2：`createElement` 2418（HEAD 2326，多的在 `names::find_fixed`）；小的 `#[inline]` 的 `create_element` 2378，看狀態 byte 2352；多 26 記進「待辦」。
- 4e-4b-1：第一版 closure 讓 `setAttribute` +42、`appendChild` +41、`getAttribute` +15；改成守衛、兩個 microtask 的旗標併成一個 byte 之後 +26、+22。

**找到、修好的錯**
- 4a-1：`contains` 在 class 表還沒建時找不到沒 intern 的 class；`replace` 先查空的再查空白；沒有 query callback 時 named property 的 descriptor 不對。
- 4b-4a：決定 72.3 的「fixup 不發」是把「拿掉」和「不再能 focus」弄混了，已更正。
- 4d-1：新的檢查抓出四處 root 比 heap 晚放，改成擁有 heap 的欄位排在最後；heap 的 drop 先留下表再 panic（第一版反過來，segfault）。
- 4e-1：shadow tree 裡之後才設的 `id` 進了 document 的表。
- 4e-2b-1：pre-insert 的檢查不是 host-including（host 能插進自己的 shadow tree）；host 上設 `textContent` 不經過 slot 的步驟。
- 4e-2c-1：`layout` 的測試的 `Dom` 排在 `NodeRoot` 前面，debug build 的 `gc` 檢查讓 36 個測試失敗（HEAD 上就是）。
- 4e-2c-2：`NextFrameView` 只拿掉內容時不重新 layout（3c-1 留下），改成有任何 layout 的 flag 就重新 layout。
- 4e-3a-2：分到不同 slot 的 DOM 兄弟不重算（4e-3a-1 起），照決定 87.1 當場沿 DOM 的兄弟套用 sibling set。
- 4e-3a-3：debug 的檢查從來沒跑（recalc 提早結束）；release build 的三個 warning；`form-pseudo-classes` 在新的 DOM 上不可能真的通過，從 `next-include.txt` 拿掉。
- 4e-4a-1：`dom_wasm_next` 的 `a_guest_gets_the_plain_members` 在 HEAD 就失敗（4e-3c-1 寫的「都通過」不對）。
- 4e-4a-2：WPT 找出少了規範的第 2 步（`new.target` 是 `HTMLElement` 自己）。

**不穩定的測試**
- 4d-1、4d-4b：今天的 DOM 的幾個 DevTools 的測試等事件等滿 30 秒的 timeout；工作目錄 15 次失敗 4 次、HEAD 15 次失敗 6 次，不是這份 RFC 造成的。原因：DevTools 的命令經過 engine 的路沒有 `start_loads`，新的 frame 和請求要等下一個 task；修正放在 `start_dynamic_imports` 之後、`tell_frames` 之前，整套 `shell` 20 次都過。補不出沒有修正就一定失敗的測試，記在 `docs/test-gaps.md` 的「只靠審查」。

**mutation test**
- 40 份 spec、1036 條；沒抓到的多半是測試沒走到，補了測試重跑都抓到。`test.py mutate --restore` 每次都沒有要還原的。
- 等價或抓不到、記在 `docs/test-gaps.md` 的：4a-3 改屬性時 `HAS_INLINE_STYLE` 的判斷；4b-3「callback 改的不在同一個 frame 畫」；4c-1 沒有 frame 的 Switch To Frame；4c-3b「捲了不要 frame」；4d-1「marking 中刪的 root 沒記下」、renderer 的 engine 排在文件之前；4e-2a 兩條；4e-2b-1 child list 的長度和 cache 的四條、`normalize`、class 去重；4e-2c-1 過期的清單五條；4e-2c-2 layout 往上走的 6 處和滑鼠的鏈（「能補、還沒補的測試空缺」）；4e-3a-1、4e-3a-2、4e-3c-2 各一到兩條；4e-4a-1 建構子存成 droppable 的 edge（V8 的 scavenge 只丟沒改過的 API 物件）；4e-4a-2 `upgradable_under`；4e-4b-2 排定義沒有的 callback；4e-4c-1 `Features` 的 `:defined`；4e-4c-2 建構的 element queue 不 invoke；4e-5b `setHTMLUnsafe` 走 fast path。
- 4c-3a：「kun 帶 `--next-dom`」和 `--kun-arg` 沒有自動的測試，記在手動確認。4e-3c-1：`reject` 那條路還沒有成員走得到。
- 先記成看不出、之後的段落換成 mutation 的：4d-4a 的 wheel event 的 wrapper（4d-4b 抓到）、4e-1 的 `composedPath()` 降上限（4e-2a）、4e-4a 的 observed attributes 和 callback（4e-4b-2）。

**試過、改掉的做法**
- 4d-1：決定 75.2 的「`Cell` 的格子、分塊長大」沒照做，整張表放進 `UnsafeCell`。
- 4c-3b：「載入中空的標題不送」mutation test 看不出差別，拿掉了。
- 4d-4b：建快照和 legacy type 共用 `snapshot_of` 慢 3% 到 4.5%，另寫一個迴圈。
- 4e-2a：拿存著的分配清單當「之前」會 signal 沒變的 slot，改成重新算之前的分配；一般的 path 走到 slot 時改用 shadow 的版本是等價的，拿掉。
- 4e-2c-1：`attachShadow` 時重算 host 的 style 多餘，只留 layout 的重建。
- 4e-3a-1：「位置變了時往後幾個兄弟」的 reach 計數等價，拿掉。
- 4e-3a-2：照決定 87.2 把 `Walk::element` 拆成 `invalidate` 和重算、往下走兩半，行為不變。
- 4e-3b-1：比新舊時用 `media` 各算一次永遠一樣，改成比 index 的清單（`Rc` 的位址）。
- 4e-4a-2：以為 `gc` 沒有 weak 的集合，其實有 `WeakMember`，改成每個候選一個小物件（決定 91.3）。
- 4e-4c-1、4e-4c-2：決定 90.3 的「沿用 parser 為 script 停下來」做不到，使用者選照 Blink 讓 sink 同步呼叫 engine（決定 93.1）；第一版在 sink 的 `create_element` 就建構，template 裡的也被建構，改成插入時才建構。

### 第 4 段：4f 到 4i

**量到的**
- 決定 109：4f-3b、4f-3c-1、4f-4c-1 整組偏低約 10 條的原因是空的迴圈：同一個 build 連跑 12 輪，`empty` 的總指令在 59M 到 70M 之間，大約 6 次有 1 次多約 1000 萬；`calls.py` 改成 `EMPTY_RUNS = 3` 取中位數。
- 追退步：4a-3 到 4f-1b 的 19 個 commit 重量，`appendChild` +203（17%）、`setAttribute` +68、`createElement` +52、`addEventListener` +74，`dispatchEvent` 少 41；`appendChild` 的每一段都是那一段的功能在每次插入和移除時的檢查。4c-1 的「每個 process 固定多 500 萬」重量是 0，拿掉。AMD 的 `instructions:u` 取樣有 skid，改用 IBS（`ibs_op/swfilt=1,cnt_ctl=1/u`），總數和 `calls` 差 5% 以內。
- 追退步：HEAD 的固定成本：`unwrap_object` 每次約 57 個 op，write barrier 約 30 個 op、沒有 inline，每次 `mutate` 結束的 safepoint 約 70 個 op，字串參數換成名稱 `getAttribute`、`setAttribute` 約 250、`addEventListener` 約 450。
- 決定 110：`appendChild` 1404 → 1179（-16%）、`dispatchEvent` 3458 → 3174（-8%）、`getAttribute` 799 → 737；`nodeType` 141 → 149 多的 8 在 V8 的 builtin（`Builtins_CallApiCallbackOptimizedNoProfiling` 41 → 49.5），記成雜訊。
- 4f-2a：activation 在分派裡 `dispatchEvent` +32，移到 cold 的函式（click 時配置一個 `Box`）之後約多 12 條。4g-1b-2a：`listeners::have` 不再 inline，`dispatchEvent` 3197，`#[inline(always)]` 之後 3174。
- 4g-2b-1：收集 overload 的 visitor 對每個 `M` 取 callback 的位址，`Member<M>::call` 不再 inline 進 trampoline，每個成員多約 22 條（`getAttribute` 736 → 759）；改成編譯時判斷（`declare::is_overload`）之後回到 736。
- 4h-2b-2：`addListener` 呼叫 `event_target::add_event_listener`，`addEventListener` 889 → 929（二分：只有 `dom_next` 887、加 `dom_api_next` 928、`#[inline(always)]` 909）；照它的步驟另外寫，回到 888。
- 4h-4c-1：`div` 有自己的 interface 之後 `createElement('div')` 2363 → 2908（先做 `HTMLElement` 的 wrapper、對不上再做一次）；拿名稱參數時就照名稱預測，HTML 文件裡直接用編號建元素（`find_fixed` 原本佔 13%），之後 2231，比這段開始前少 132。
- 留到收尾的單一成員的退步：4g-2a `createElement` +19、4g-3a 和 4g-3b `setAttribute`、`appendChild`（脫離的 document 上）、4g-4a `appendChild` +15、4g-4c-3a 每個成員多 9 到 24、4h-1b-1 `appendChild` +12、4h-1d-1 `createElement` +10、4h-4a `createElement` +17、4h-4c-2 `appendChild` +14、4h-5b-1 `dispatchEvent` +10、4h-5c-2 每個多 12 到 17、4i-1 `dispatchEvent` 3387（`window.event` 的 +179）。
- 決定 162：單型化照成員的種類分派，`dom_js_next` 的函式 269,893 → 69,555、rlib 900 MB → 151 MB；Linux 上 `Member::call` 的條件的重編 133 s → 34 s、touch `templates.rs` 13 s → 4 s；`calls` 比 4i-2 少 8 到 22。
- 4i-1：mutation 的時間：`templates.rs` 只 `touch` 時 `dom_js_next` 重編 11 秒，前面加一行 194 秒（行號表變了），`test.py mutate` 把 `to` 補到和 `from` 一樣的行數；不移動行號的還是約 70 到 190 秒，原因還沒查（使用者說可以接受）。
- 4g-1b-2b-1：Chrome for Testing 152 的 lazy 的圖的距離：上下 1250、左右 625，套在每個捲動容器的 scrollport 上，`overflow: clip` 的不算；headless 的 `--dump-dom`、`--virtual-time-budget` 會全部載入，量不出來。

**找到、修好的錯**
- 4f-1a：wasm 的 `NotAllowedError` 的代碼 22 是今天的 `kun:dom` 的 `InvalidNodeTypeError`，改成 24。4f-1b：parser inserted flag 照 Blink 在插入時清掉，WPT `form_owner_and_table_3.html` 抓到：規範只在 reset 時清。
- 4f-3a：`Node::scope` 的 `HAS_CHILD_LIST`（bit 3）和 custom element 的狀態（bit 3 到 5）重疊，讀過 `childNodes` 的元素被當成 undefined 的 custom element；改成 bit 6。表單控制項原本不能 focus。4f-3c-1：滑鼠按下找 focus 改走 flat tree（原本停在 UA shadow root）、表單控制項進 Tab 的順序。
- 4f-3a：「WPT 連著跑時沒印出統計」是 log 裡有 NUL（`'#ffffff\0'`），`grep` 當成二進位檔，用 `grep -a`；不是 WPT 或 `test.py` 的問題。
- 4g-1a：背景圖的三條 `url()` 都沒解析、`load` 沒等背景圖、圖來了沒重畫（12 個 `object-fit-*i.html`）。4g-1b-2a：4g-1a 的 `finish_next_load` 清掉 `STYLE_SHEETS_CHANGED`，晚到的 `<link>` 不載入（那兩段沒跑 `shell` 的 `next_dom`）；`observe` 在沒有 `ElementData` 時 panic（34 個 CRASH）。
- 4g-1b-2b-1：照規範「先 unobserve 再加」，WPT `notify.html` 多回報一次而發了 loop 的錯誤，改成同一個 box 什麼都不做（Blink 的 `observeInternal`）；跳過的要下一個 frame（`eventloop.html`）。
- 4g-2a：新的 binding 漏了 `read_only_prototype`，每個 interface 的 `prototype` 能改、能刪（`2d.canvas.context.type.prototype.html`）。4g-2b-1：`putImageData` 的 18 個 CRASH，`ImageData`、`CanvasGradient` 沒有 `take_arg`、`arg`。
- 4g-3b：參考自己不算循環的 mutant 查出 kun 往上找停在 copy 的 shadow root，`<g>` 裡的 `<use>` 參考外面的 `<use>` 的循環找不到、整個不畫；照 Blink 的 `ParentOrShadowHostElement` 走。
- 4g-4a：子 frame 的 view 的 `paint_dirty` 從來不清，每個 frame 都重新 paint；iframe 的步驟在 reactions 之後（`connectedCallback` 看到 `contentWindow` 是 null）；realm 之間碰不到（V8 每個 context 預設的 token 不同），照 origin 設。
- 4g-4b-2a：`file://` 的 opaque origin 每次 parse 都是新的，最上層和自己也不同源（`shell` 的 `the_first_entries` 失敗，4g-4b-1 沒跑 `shell`），改成 `same_origin`；event handler 原本在最上層的 context 編譯。
- 4g-4c-3a：新的 DOM 一直沒有 `set_accept_any_receiver(false)`；4g-4c-1 起沿用的初始 `about:blank` 的 window 沒有 page 的 focus（`focus-event-view.html` 逾時）；srcdoc 沒有 doctype 時不是 quirks（`NextParser::new_srcdoc`）。
- 4h-1a：WIT 的參數名稱是關鍵字時 4g-4a 起 `ci.py guests --next` 解析不了，跳脫成 `%string`。4h-1b-1：V8 的 `Uint32Value` 不讓字串的負數 wrap，新的 DOM 的所有 `unsigned long` 參數都是，改 `front::to_uint32`（`compareBoundaryPoints` 93 個 gap）。4h-1c-1：`selectAllChildren` 用了 DOM 的 length，規範是子節點的數目（177 個 gap）。
- 4h-3b、4h-3c：自己變成 `display: none` 時沒取消、一批事件之間沒有 checkpoint、被 reject 的 promise 報了 unhandled rejection、沒有元素的動畫的事件被丟掉、`finished` 在 `finish` 事件之後；4h-3c-2：`pseudoElement` 收下任何 `::` 開頭的字串、`DocumentTimeline` 當參數讓 renderer panic、暫停和 fill 的 script 動畫在 style 重算後不見（今天的 DOM 也是：`letter-spacing-animating-*`、`word-spacing-animating-word-spacing` 的預期結果跟著改）。
- 4h-4a：`pushState` 之後 renderer 的 session history 沒加一，`history.back()` 以為沒有上一筆；`onload` 裡的導覽要取代 entry。4h-4b：`click()` 的 `FrontHost` 沒轉交 `ScriptHost::navigate`，點連結的請求被丟掉。
- 4h-5a：`toBlob` 的 callback 裡登記的 blob URL，renderer 先處理 frame 請求才跑 element task，圖的請求比 Browser 聽到登記早（`currentSrc-blob-cache.html`）；`shell` 的測試用 `file:` 的頁只是因為這個順序才過，改成 HTTP。
- 4h-5b-1：`--next-dom` 的 network 的時間都是 0：commit 在 renderer 的佇列裡等（約 20 ms）讓 time origin 晚了；browser 改送導覽開始的 `ipc::TimeTicks`。4h-5b-2：導覽離開的文件還算 fully active（`wakelock-active-document`）；idle period 只看今天的 DOM 的 timer（`callback-timeout-when-busy.html`）。
- 4h-5c-1：record 的轉換的順序（`headers-record` 的 6 項，`URLSearchParams` 一起改）。4h-5c-2：getter 和 setter 的函式沒有名稱（`get x`），`idlharness` 的每個 attribute 都失敗；移除的 iframe 的初始 `about:blank` 的 base URL 退回 `about:blank`（`keep_about_base_url`）。
- 4i-2：checkpoint 的 debug 檢查找到兩個漏算的入口：engine 的 classic script 在回報例外前放掉計數、parser 建構 custom element 沒經過 `Host`。4i-3：`is_inclusive_ancestor` 沒經過 shadow host，click 打到 UA shadow tree 的 editor 被當成 intercepted。
- 4i-4：子 frame 的 sticky 只用最上層的 `stuck`；子 frame 捲過就整棵樹重畫；`display: list-item` 的 `<img>` 的 `::marker` 有 `content` 時 counter 給了文字、box tree 沒有，debug 的檢查 panic。4i 的收尾：`dom_wasm_next` 的 `wit` 兩個從 4i-1 起失敗（那段沒跑它）。
- 4f 整段：誤以為第 4 段做完、提早跑了階段收尾（commit 訊息寫的「stage 4 wrap-up」是錯的）；4f-3c-1 跑 `shell` 時漏了 `webdriver`。

**不穩定的測試**
- 4g-4c-1：`frames/f8-child-resized` 約一半逾時：子文件在第一次 layout 之前選圖（viewport 是 0×0，`(max-width: 150px)` 成立），iframe 縮小時選到同一張、沒有 `load`；選來源前先 layout 一次（`NextLayout::has_viewport`），連跑六次都過。
- 4h-5b-2：`next_dom::navigation_timing_on_the_new_dom` 和其他瀏覽器一起跑時 8 輪裡 6 輪 `domInteractive` 比 `responseEnd` 早：time origin 被夾了兩次（renderer 夾到時鐘的開始、`set_window_time_origin` 夾到 DOM 建立），renderer 晚開始 7 到 30 ms；改成可以是負的（`Clock::instant_at`），8 輪都過。試過、不是原因的：兩個時鐘之間被搶走。
- `dom_api_next` 的 `window.rs` 的 `events_reach_the_window`、`a_new_event_counts_from_its_realms_window` 靠真的時間：第 4 段收尾改成測試自己給 window 的 time origin、差距放大到 15 ms 以上，連跑 5 次都過。
- 4h-2a：`shell` 的 `automation_pipe::frames_are_switched_to_and_kept_through_their_navigations` 失敗一次、之後 20 次都過，沒拿到訊息，記在待辦。
- WPT 基準上也不穩的，記在待辦：`redirect-tao`、`secure-connection-start-non-zero.https.html`、`after-scrollable-range-shrinkage-001.html`（基準 1/8、工作目錄 4/8、再跑 0/5）、`sizes-auto-rendering-dynamic`、`adoptedstylesheets-cascade-order`（單獨跑失敗、`css/cssom/` 一起跑過）、`srcset-natural-size-in-dynamic-iframe.html`。

**mutation test**
- 96 份 spec、2329 條（每段一份，有的加 `-shell`、`-renderer`、`-scenario`、`-wpt`、`-wdspec`、`-reftest`、`-frames`），補了測試之後除了等價的都抓到；決定 110 的 `mutants-110.toml` 10 條、決定 162 的 `mutants-kind-dispatch.toml` 26 條也是；`test.py mutate --restore` 每次都沒有要還原的。
- 沒抓到的大多是測試的漏洞，補了測試：例如 4f-2c 的 `type`、`value` 是 presentational hint 的屬性，元素自己一定重算，加 `+` 的兄弟；4g-3b 的 markup `height=10/>` 沒加引號的值吃掉 `/`；4g-4c-3a-reftest 的 f9 在今天的 DOM 預期 FAIL，改看「只有新的 DOM 過 1、gap 0」；4g-4c-3b-wdspec 照索引切換差一（wdspec 用 `<frameset>`），`shell` 的 `frames_are_switched_to_and_from` 也在新的 DOM 上跑；4h-1c-1 情境多了只改選取的那一步。
- 等價的，記進 `docs/test-gaps.md`：4f-3c-2 的三條、4f-4a 的 optgroup 的 `:disabled`、4f-4b 的 `:-internal-list-box` 不讀 `size`、4f-5 的 `valueAsNumber` 設 NaN、4g-1b-2a 的 visibility 的 paint 編號、4g-4c-3b 的兩條、4h-1a 的 `removeAttributeNode`、4h-1b-2 的 `insertNode` 的 comment、4h-4c-2 的處理過之後仍收候選、4h-5b-2 的 `due <= now`、4i-1 的 `new DOMRectReadOnly`、4i-2 的 settle 之後的 checkpoint。不記的等價：4f-3b 的 `with()` 不清 `shadow_pseudo`、4g-4a 的 `create_child_frame` 不看 `is_discarded`（留著給沒有 frame 清單的 host）。
- 沒有計數守著的優化：4g-3b 的 `update_svg_clients`、`update_use_trees`，4g-4a 的 `has_seen_iframes`，4h-1c-2 的 host 已經有 focus 時不出操作。只靠審查或測試做不出來的：4f-1b 的 parser 的「intended parent 在 form 的樹裡」、4g-1b-1 的 renderer 在 viewport 改變時重新選、4g-4b-1 的跨 origin 的 target、4g-4c-1 的 `detach_global`、4h-4b 的新分頁前景或背景、4h-4c-2 的跨 origin 的 autofocus 和 `navigator.webdriver`、4h-5c-2 的 renderer 送出的請求、4i-4 的系統顏色交給 view 的三處、4h-5b-2 的不夾 time origin（只在負載下看得到）。
- 寫 spec 時拿掉的多餘的程式：4f-1b 的三處、4g-1a 的 `take_image_event` 走不到的一段、4g-4a-frames 的 response 沒有 fragment 時留著 request 的（`net` 本來就留著）、4h-1b-1 的 `CONTAINED_BY`、4h-2a 的 `ptr_eq` 和 `<style>` 的文字比對、4h-1e-1 的 renderer 幫 module 加 CORS、4h-4b 的 `has_element_changes`、4h-5a 的 `abort()` 加一、4h-5b-2 的 `didTimeout` 的提早回傳、4h-5c-1 的 `timeout()` 的檢查。

**試過、改掉的做法**
- 4f-3b：native look 一開始 DOM 留一份 id 清單，使用者問了 Chrome 的做法之後改成旗標（`CONTROL_STATE_CHANGED`，`layout_more` 在 padding 裡，節點大小不變）。
- 4f-4b：建樹的檢查放在插入的走訪裡 `appendChild` 多 6 條，搬出去（`schedule_control_tree`）；4g-1b-1：picture 的檢查在快路上時 `appendChild` 1185，移進表單的 out-of-line 的 hook。
- 決定 110：`unwrap_object` 標 `#[inline]`，getter 一樣、其他成員多 15 到 40（`appendChild` 1222 對 1179），沒有留。
- 4g-3b：copy 照 Blink 舊的 `IsDisallowedElement` 只留畫的元素，WPT 的 `use-element-non-rendered-has-selector` 要 `<defs>`，改成留全部 SVG 的元素。4g-3c：決定 122.2 寫的 `layout` 改放 `gfx`（兩個 binding 都不依賴 `layout`）。
- 4h-4c-1：把名稱複製出來讀（`write_one_byte`，2715）、就地借出（`ArgString::text`：V8 的 GC 時 abort），都沒留。4i-3：renderer 自己告訴初始 `about:blank` 的 lifecycle，和 page agent 的重複，拿掉。

**其他**
- rusty_v8 的 fork：`Context::detach_global`、`get_incumbent_context`、cross-origin 的 access check（`f9b6cb7`），本機的 glue 重編 8 到 12 秒；之後換到 `kun-v152.2.0-25`（macOS 的兩個 job 排不到 runner、被取消）。4h-1e-1：`librusty_v8.a` 兩次被換成 `-24`（另一組 feature 的 build script 沒帶 `RUSTY_V8_ARCHIVE`，共用 `gn_out`），複製回去。
- `cargo fmt`：4f 時 200 個檔案 1952 處（要跑兩次才穩定），第 4 段收尾 1985 處，4i 收尾 57 處。

### 第 5 段、最後一段和收尾的量測

**量到的**
- 5a～5d-4a：`bench.py calls` 每段一輪，大多和前一段差 4 以內；5b-2 起量 `dom_js_next` 的 `bench::bench_calls`，`createElement` 多 11（量的環境換成自己的 realm）。
- 5c-2：`setAttribute` 1279（+3）、`appendChild` 1315（+6）：插入和改屬性時讀「要不要 stack」的 `Cell`。
- 5d-3c：`appendChild` 第一次量 1354（插入時拆成兩次寫旗標），常見情況照舊一次寫、shadow root 的放到 `#[cold]` 之後回到 1318。
- 5d-4b：`createElement` 2307（+72：先建 wrapper 之前比 receiver 的 creation context，V8 的 `GetCreationContext` 和 handle）、`dispatchEvent` 3424（+52：fully active 的檢查原本 +337，改成先看「有沒有文件 discard 過」的旗標、檢查移到 out of line）；收尾時一起看。
- 收尾：直接用 `trace_span!` 時 `dispatchEvent` 3424 → 3521（store forwarding 的衝突 11.1 → 17.5），先看 `level_enabled!` 之後 3467，換成 `trace_event` 之後 3481、連量三次都一樣；多的 44 沒有再拆，記在「待辦」。數字和比較在 `measurements.md`。
- 第 5 段收尾：`cargo test --workspace` 58 個 binary、3900 個過、1 個 ignored，122 秒（第 4 段收尾時 69 個 binary、約 6 分鐘）；`cargo fmt` 累積 142 個檔案。
- 5d-4a：整套 WPT 293624 個檢查，81 個不照預期（大多是 5d 修好的新通過），和 5a 之前的預期比還有 69 份 gap（5d-4b、5d-4c 查完，剩下的不是退步）。

**找到、修好的錯**
- 5b-3b：新的 DOM 的 title 在 task 結束後才送，`load` 的 lifecycle 事件先到 DevTools：`tell_next_lifecycle` 先 `update_next_title()`（同一條 `to_browser`）。
- 5c-2：`<style>` 的 `url()` 的 referrer 是文件的 URL，不是 base URL。5d-2a：about:srcdoc、about:blank 的文件送出的請求的 origin 是 `null`，改成繼承來的 origin。
- 5d-3c：`:nth-last-child(… of S)` 漏算前面的兄弟（`FeatureSets::earlier_fallback`）；`set_page_focused` 在 page focus 沒變時把不顯示 ring 的 focus 加上 `FOCUS_VISIBLE`。
- 5d-4a：整套 WPT 抓到 5d-3 的四個退步（有動畫的元素換 viewport、`offsetParent` 的 open shadow tree、slot 的 fallback 再顯示時是舊的、`attr()` 的轉小寫只在 `text/html`）。
- 5d-4c：`style_next` 拿上一輪的 root 字級，`2rem` 的 root 每重算一次加倍；shadow root 和 constructed sheet 變了的清單整個 DOM 一份、頂層 frame 先全部拿走，改成每份 document 一份（Blink 的 `StyleEngine::dirty_tree_scopes_`）；SVG 圖片的 `<use>` 沒有副本。
- 第 5 段收尾：`dom_api_macros` 的 `large_enum_variant` 只在 workspace 裡觸發（`syn` 的 `Type` 的大小跟著別的 crate 打開的 feature 變），把 `MemberDef::Attribute` 的 `set` 放進 `Box`。收尾：`speedometer_test.py` 5a 之後就壞了 3 個（不在 CI 裡）。

**不穩定的測試**
- 5a：`devtools::a_command_left_waiting_is_answered_when_the_renderer_goes`：renderer 先走、等不到回答也過，一併標。
- 5d-2a：`subresources_from_another_origin_go_by_cors`：module 的表不留失敗的 fetch，`/closed.mjs` 比 `deep.mjs` 的 import 早失敗時再抓一次，延遲 300 ms；`the_tab_title_follows_document_title` 之前是碰巧過的（srcdoc 同步的 `DOMContentLoaded`）。
- 5d-4a：`adoptedstylesheets-cascade-order`（第一次 layout 之前 style 沒有 viewport）、`deadline-max-rAF`（長 idle period 截到下一個 frame，`timeRemaining()` 往下取到 0.1 ms 的格子）、`scroll-behavior-smooth-navigation`（平滑捲動那項 known intermittent）。
- 5d-4b：`secureConnectionStart`、`redirectStart` 要大於 0 的三份是 0.1 ms 的取整（決定 181.5），標成 known intermittent。
- 5d-4c：`sizes-auto-rendering-dynamic` 約一半選到 `green.png`：list of available images 整個 renderer 一份、載入過的從不拿掉，`srcset::pick` 偏好快取裡更密的；照 Blink 的 weak cache 修（決定 182.3）。
- 最後-b：`messaging::messages_and_timers_run_in_the_order_queued`：負載下 `BeginMainFrame` 排在 message 的 task 前面（HTML 允許），改成在 rAF 的 callback 裡做。
- 5c-3：rust-analyzer 的 `cargo check` 沒帶 `RUSTY_V8_ARCHIVE`，把 -25 的 prebuilt 蓋回共用的 `target/`（原因查到了），mutation test 改用 `target/v8-local`。
- 收尾：`KunTests.test_the_windows_are_the_runners_times` 每次都失敗（第一個 iteration 的第一個 test 長 0.2 到 0.4 ms，容許 0.15），原因還沒找到，沒有放寬、沒有用重跑蓋過去。

**mutation test**
- 每段都跑（5b-1、5b-2 只刪東西沒跑）；最後都抓到或說明了。補了測試才抓到的：5c-2 parser 的行號、沒人拿的 stack；5c-3 標記中交出的不標上、任何 JS 的邊都當 wrapper；5d-1 的 7 個；5d-2a 的 2 個；5d-2b 的 3 個；5d-3a 的 4 個；5d-4a fallback 不重算；5d-4b 的 4 個；5d-4c `KeyframeEffect`、`DocumentTimeline` 的 realm 和 discard 的文件不放掉圖；最後-b `<th>` 的 span；收尾 callback 之後的 `RunMicrotasks`。
- 卡住的：5c-1「`Shutdown` 不叫醒暫停」跑了 1065 秒，改成 channel 加 10 秒的時限之後 11 秒抓到；5d-2b「`load` 排兩次」卡 33 分鐘，補 `modules::the_windows_load_fires_once` 之後 34 秒斷言失敗。
- 等價或記進 `docs/test-gaps.md` 的：5c-2「最後一個 session 離開後還要 stack」；5c-3「young 頁上的 filler 當成物件」；5d-2c「沒連上的 meta 也算」（spec 刪掉）；5d-3a 「shadow root 自己的文字沒有 host」「刪掉的 focus listener 也算」（程式改簡單）、「anchor 移了但沒標成要 commit」；5d-3b「page focus 改變時 DOM 叫醒 style」（多餘，拿掉）；5d-4a `sizes="auto"` 先排版測不到；5d-4b `cssText` 先寫回（多餘，拿掉）、script 派送的 `entry`；最後-a「compositor 捲的量不交給文件」（只靠審查）；最後-b `colspan` 不夾到 1000；最後-d `object_address`；收尾「rAF callback 之後的 checkpoint 移到 span 外」。
- 最後-c：`mutants-last-c.toml` 5 個照舊程式的結論重做，3 個照樣抓不到；「canvas 畫了要 frame」「沒有動畫時照樣更新 timeline」現在有測試抓到，從 test-gaps 拿掉。

**試過、改掉的做法**
- 5d-2a 的「還有元素的 task 時先不發 window 的 `load`」（決定 175.3 的甲）→ 5d-2b 改成元素的 task（乙）。
- 5d-4a：`timeRemaining()` 四捨五入會給 16.7，改成往下取格子，抽成 `on_grid` 用單元測試。
- 收尾：clippy 的 `disallowed-macros` 連 macro 展開出來的 `tracing::span!` 也報、`#[expect]` 蓋不住，改成 `sanity.py spans`。
- 5a：`browser` 的測試用 V8 的今天的 engine 會和新的 DOM 的 isolate 撞在一起（renderer panic 在 isolate 的 drop 順序），改用 wasm 的。

### 收尾之後的優化（在 `main` 上）

**量到的**（Speedometer 每個 suite 10 個 iteration 的中位數；數字和比較在 `measurements.md`）
- 收尾的量測：Speedometer 合計舊的 DOM 1262.9 ms、收尾時和 Chrome 的比值的幾何平均 1.61；比舊的 DOM 慢的是 TodoMVC-jQuery（186.9 → 280.9 ms）、ES6-Webpack-Complex-DOM（63.7 → 70.2）、Editor-TipTap。使用者排的順序：先追回比舊的 DOM 慢的，一項一個 commit。
- `HTMLCollection` 的 index 快取（Blink 的 `CollectionIndexCache`）：jQuery 的 `cleanData` 用 `elems[i]` 讀 `getElementsByTagName("*")`，每讀一個都從頭走樹（`collection_item`、`collection_matches` 佔 43%）；TodoMVC-jQuery 280.9 → **141.5 ms**（Chrome 80.1）。同一個 commit 把排 UA shadow tree 的 `Vec::contains` 去重（5%）改成集合。`bindings/get-elements-by-tag-name` 沒有變（81 runs/s，Chrome 1145：Chrome 對同一個根和名字回傳同一個 collection）。
- querySelector 照 Blink 的 `MatchCompound`：每個元素重建 `MatchingContext`、重查 quirks mode 改成一個 compound 一個 compound 直接比；ES6-Webpack-Complex-DOM 70.2 → **53.6 ms**，`parser/query-selector-all-class` 386 → 531 runs/s（Chrome 1570）。
- 子節點的環（Blink 的 `ContainerNode` 沒有 `last_child_`）：`Node` 的欄位 54 → 50 bytes，空出 bloom filter 的位置；`appendChild` 1320 → 1332（移到最後時移除和插入都更新第一個子節點的環指標）。
- 子樹的 bloom filter（Blink 的 `attribute_or_class_bloom_`）：ES6-Webpack-Complex-DOM 53.6 → **40.9 ms**（Chrome 26.4）；`setAttribute` 1278 → 1300。
- Editor-TipTap：`focus()` 找 caret 第一個能放的位置時把 editing host 裡每段文字都切成 grapheme cluster（使用者猜的，取樣 5.7%）；每個 run 只算第一個之後 102.0 → **96.3 ms**（舊的 DOM 95.2），`Long` 的 async 9.8 → 4.5 ms。
- 重量 Speedometer：合計舊的 DOM 1262.9 ms、現在 1083.3 ms（少 14%）、Chrome 678.2 ms；和 Chrome 的比值的幾何平均 1.78（舊的 DOM）→ 1.61（收尾時）→ **1.50**。20 個 suite 都不比舊的 DOM 慢（TipTap 差 1%，在雜訊裡）。
- 不是退步的：`layout/line-layout-line-height` 的 +45% 是舊的 DOM 兩輪之間沒有做 rendering update（`display: none` 沒套用就換回 `block`），在 `none` 之後多讀一次 layout 時兩邊 298、294 ms。沒找到的：`dispatchEvent` 補 trace 之後多的 44 條，`perf record` 照函式比時 AMD 的 skid 和 ICF 讓樣本落點不可靠，要 callgrind（這台沒有）。

**找到、修好的錯**
- bloom filter：`style` 屬性是從宣告懶惰地寫回的，位元可能還沒加上，查詢不要它的位元（Blink 的 bloom filter 也不收它懶惰同步的屬性）。
- `trace_event` 的兩個測試平行跑時互相改全域的 `LevelFilter`，併成一個。`perf` profile 編 `html` 的警告（`NodeKind` 只在 debug 的 `same_tree` 用）。

**mutation test**（`mutants-wrap-up-*.toml`，6 份、52 條）
- trace 14 條抓到 13 條，等價的 1 條（rAF callback 之後的 checkpoint 移到 span 外：V8 的 engine 在 callback 回來時就跑完了，記進 `docs/test-gaps.md`）。
- collections 7 條：第一次「跑過最後不記長度」「每次重數長度」沒抓到，補了測試之後都抓到。compounds 12 條、bloom 8 條、caret 3 條都抓到。
- ring 8 條：第一次照平常的 24 GB、1800 秒跑，一條卡了 26 分鐘、另一條長到上限，機器進 swap、使用者收到兩次記憶體滿的警告；改成先在 24 GB 裡編譯、再在 2 GB 裡只跑測試、時限 120 秒，4 條抓到、4 條卡住由時限或上限結束（算抓到）。另一條「移除最後一個之後環留著舊的」等價（移除最後一個時 `ring` 本來就是 `previous`），把多餘的判斷拿掉、spec 也拿掉。
- `test.py mutate --restore` 每次都沒有要還原的。
