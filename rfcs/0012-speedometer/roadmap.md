# RFC 0012 的路線圖：讓 Speedometer 3.1 跑得完

這份放「Speedometer 跑得完還缺的功能要切成哪些 RFC、誰先誰後、怎麼平行」。缺哪些功能、每個 suite 用到什麼，以 `README.md` 的「盤點的結果」和 `details.md` 的「對照表」為準，這裡不重複，只引用。

**狀態**：RFC 的切法、三條平行的線、里程碑 M1 和關鍵路徑先開哪一份在 2026-10-07 定下來（`README.md` 的「已決定的事項」第 4 到 8 條）；每一份 RFC 開始前，照 `rfcs/README.md` 的流程各自決定它的設計。里程碑 M1 到 M6 在 2026-10-10 到 10-12 都達成，2026-10-16 的盤點裡 20 個 default suite 在 kun 上都跑得完。RFC 0012 在 2026-10-16 完成時，下面的表是那時的樣子，之後不再更新（完成的 RFC 不再改，`rfcs/README.md` 的「流程」第 6 步）：除了 U，A 到 T 都有了 RFC，還沒做完的是 G 的第二段（RFC 0050，已接受）、P 的第一份以外的部分（打字、`execCommand`、游標和選取的繪製）和 U（還沒有 RFC）；之後的狀態看 `rfcs/README.md` 的索引。

這份原本的更新方式：一份 RFC 建立時，把表裡它的代號（A、B……）換成編號和連結，做完時寫上日期；一個里程碑的 suite 在 kun 上跑得完時，在「里程碑」寫上日期，量測結果記進 RFC 0012 的 `progress.md`；盤點重跑之後，有新的缺口就加進對應的 RFC 或新的代號。

## 開始時已有的 RFC（2026-10-07，狀態到 RFC 0012 完成時）

| 相關的 RFC | 狀態 | 和 Speedometer 的關係 |
| --- | --- | --- |
| RFC 0003 flex 和 grid | 已完成 | `display: grid`、`inline-block`、`inline-flex`、`inline-grid` 已經有了 |
| RFC 0008 DOM API 擴充 | 已完成 | 現有的宣告表、`createEvent`、`MouseEvent` 的基本成員 |
| RFC 0009 iframe | 已完成（2026-10-08）：2a 看不見的 iframe、2b 子 frame 的 layout、paint 和輸入（E）；子 frame 的導覽經過 browser、`WindowProxy` 在它的「之後的可能」（Speedometer 用不到，它的第 59 條） | Speedometer 每個 suite 都在 iframe 裡跑；子 frame 要有 layout 和 paint |
| RFC 0010 DOM API 第二批 | 已完成（2026-10-07）；`Range` 移到之後的 RFC（它的第 19 條） | `innerHTML`、`DOMParser`、`MutationObserver`、`queueMicrotask`、`AbortController`、`TreeWalker` 已經有了 |
| RFC 0013 `position` | 完成（2026-10-09：`relative`、`absolute`、`fixed`；`sticky` 另一份） | `position` 是 17 個 suite 用到的 CSS |
| RFC 0014 宣告表的拆檔 | 已完成（2026-10-07），已併進 main | DOM 線的前置（「開始平行之前」） |

## 要開的 RFC

A 到 T 各自一份 RFC（U 是之後加的，和 Speedometer 無關），H 拆成 H1 和 H2（`README.md` 的「已決定的事項」第 5 條）。代號只在這份裡用；新 RFC 的編號建立時再取（同時有好幾個人在加 RFC），建立後代號後面加上編號和連結。大小是相對的：S 小於一個階段，M 兩三個階段，L 要分好幾個階段，XL 本身可能要拆成好幾份 RFC。「線」見「三條平行的線」，「里程碑」見「里程碑」。

| 代號 | RFC | 範圍 | 大小 | 依賴 | 主要動到 | 線 | 最早要的里程碑 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A：[RFC 0015](../0015-es-modules/README.md) | ES module 和 script 的載入 | module graph（靜態 `import`）、動態 `import()`、`modulepreload`、script 插入的 `<script>` 要執行，2026-10-08 完成 | L | — | `bindings`、`dom_js`、renderer 的 script、`html` | 雜項 | M1（靜態 `import`）、M4、M5 |
| B：[RFC 0019](../0019-location-history/README.md) | `location`、`history` 和同一份文件內的導覽 | `Location`、`History`（`pushState`、`replaceState`、`state`）、設定 `hash`、`hashchange`、`popstate`、`URL`、`URLSearchParams`，2026-10-08 完成 | M | — | `dom_api`、renderer、`browser` 的 session history | 雜項 | M1 |
| C：[RFC 0016](../0016-cssom/README.md) | CSSOM | 第一段：inline style 的 `CSSStyleDeclaration`（`element.style`、`setProperty`、`cssText`、camelCase 的屬性），2026-10-07 完成；第二段：`getComputedStyle`（resolved value、唯讀、live），2026-10-09 完成；第三段：constructable stylesheet、`adoptedStyleSheets`（RFC 0016 的第 9 條拆成三段），2026-10-12 完成 | M–L | 第二段要 D 的 hook，第三段要 K-1 | `style`、`computed_style`、`dom_api` | CSS | M1（`element.style`）、M2、M3 |
| D：[RFC 0018](../0018-cssom-view/README.md) | CSSOM View 和 script 觸發的 layout | `getBoundingClientRect`、`DOMRect`、`getClientRects`、`offset*`、`client*`、`scroll*`、`scrollTo`、`elementFromPoint`、`innerWidth`、`devicePixelRatio`；script 讀幾何時同步更新 style 和 layout（Blink 的 `Document::UpdateStyleAndLayout`），2026-10-08 完成 | L | 子 frame 要 E-1 | `dom_api` 往 renderer 的 hook、`frame_view`、`layout` | CSS | M1（`getBoundingClientRect`）、M2 |
| E | RFC 0009 的 2b（既有） | 2b-1 子 frame 的 lifecycle（style、layout）；2b-2 子 frame paint 進父 frame、合成；2b-3 hit test、輸入、捲動。2b-1、2b-2 2026-10-07 完成，2b-3 2026-10-08 完成 | L | — | renderer、`paint`、`compositor` | CSS | M1（2b-1）；2b-2 影響量得對 |
| F：[RFC 0022](../0022-input-events-focus/README.md) | 輸入事件和 focus | `KeyboardEvent`、`FocusEvent`、`WheelEvent`、`InputEvent`、補完 `MouseEvent` 和 `UIEvent`；`click()`（activation behavior）、focus 模型（`focus()`、`blur()`、`activeElement`、`hasFocus`、`tabIndex`、`autofocus`）；使用者的輸入也分派這些事件，2026-10-09 完成 | M | — | `dom_api`、`dom`、`shell` 的輸入 | DOM | M1 |
| G：[RFC 0023](../0023-forms/README.md)，第二段是 [RFC 0050](../0050-form-controls-rendering/README.md)（2026-10-16 接受） | 表單 | 第一段 DOM 和行為：`HTMLInputElement`（`value`、`checked`、`type`…）、checkbox 的 activation behavior 和 `input` / `change`、`label`、`button`、`select`、`option`、`textarea`、`fieldset`（RFC 0023，2026-10-09 完成）；第二段控制項的繪製（RFC 0050，已接受） | L | F | `dom_api`；第二段 `layout`、`paint` | 第一段 DOM、第二段 CSS | M1（`input` 的 `value`、`checked`）、M2 |
| H1：[RFC 0017](../0017-html-window-basics/README.md) | HTML 元素和 Window 的雜項的最小部分（照 RFC 0008 的做法） | M1 要的：`navigator`、`dataset`、`document.title`、`Performance` 和 User Timing（`mark`、`measure`、`timeOrigin`），2026-10-08 完成 | S–M | — | `dom_api`、`dom_js`、WIT | DOM | M1 |
| H2：[RFC 0028](../0028-html-window-batch-2/README.md) | HTML 元素和 Window 的雜項的其他部分（照 RFC 0008 的做法） | `Blob`（2026-10-07 從 H1 移過來）、`a.href`、`<template>.content`、`<img>` 和 `Image`、`title`、GlobalEventHandlers、`isConnected`、`readyState` / `getElementsByName`、`crypto.randomUUID`、`requestIdleCallback`、`URL.createObjectURL`，2026-10-10 完成（`console` 補齊、`<img>` 的 `srcset` 和 lazy loading 也在裡面；`FormData` 另開） | M，多半是照著做 | — | `dom_api`、`dom_js`、WIT | DOM | M2、M5 |
| I：[RFC 0037](../0037-range/README.md) | `Range` 和 `StaticRange`（從 RFC 0010 移出，它的第 19 條） | 每個修改原語調整 live range、`StaticRange`；RFC 0010 的 `MutationObserver`、`queueMicrotask`、`AbortController`、traversal 已經完成。2026-10-12 完成 | M | — | `dom`、`dom_api` | DOM | M6 |
| J：[RFC 0034](../0034-message-channel/README.md) | `MessageChannel` 和 `postMessage` | `MessageChannel`、`MessagePort`、transfer、`structuredClone()`；React 的 scheduler 沒有 `MessageChannel` 會改用 `setTimeout`，時間就不能比；跨源 window 能用的成員（RFC 0009 的第 53 條）。window 的 `postMessage` 和 `MessageEvent` 在 [RFC 0030](../0030-testdriver/README.md)（它的第 3 條，2026-10-10），J 接在它的序列化和 `MessageEvent` 上。2026-10-12 完成（跨源 window 的成員不在 RFC 0034，另外排） | M | — | `dom_api`、`dom_js`、event loop | DOM | M2 |
| K：K-1 是 [RFC 0035](../0035-shadow-dom-custom-elements/README.md)，K-2 是 [RFC 0046](../0046-shadow-tree-rendering/README.md)（2026-10-15 完成） | shadow DOM 和 custom element | K-1：attach（`open`、`closed`）、slot、事件的 retarget、custom element 的 registry、reaction、upgrade（2026-10-12 完成）；K-2：`:host`、`::slotted`、style 的範圍、flat tree 的 layout | XL | C 第一段 | K-1 `dom`、`dom_api`；K-2 `style`、`layout` | K-1 DOM、K-2 CSS | M3 |
| L：[RFC 0027](../0027-fetch/README.md) | `fetch` | `fetch`、`Request`、`Response`、`Headers`、任何 method 和字串的 body、`AbortSignal`、CORS（連同 module script），宣告表的 `Promise<T>`；bytes 的形式在 H2，沒有 Streams 和 XHR（2026-10-10 完成） | M–L | — | `net`、`protocol`、`dom_api`、`dom_js`、`bindings`、`dom_wasm` | 雜項 | M4 |
| M：[RFC 0036](../0036-rendering-observers/README.md) | rendering 的 observer | `ResizeObserver`、`IntersectionObserver`，在 rendering update 裡通知，JS 和 wasm（2026-10-12 完成，不含 v2） | M | D | renderer 的 update rendering | CSS | M4、M6 |
| N：[RFC 0033](../0033-canvas-2d/README.md) | Canvas 2D | `HTMLCanvasElement`、`CanvasRenderingContext2D`（路徑、fill、stroke、文字和 `measureText`、transform、clip、line dash），用 Skia 畫，2026-10-12 完成 | L | — | 新的模組、`paint`、`raster` | 雜項 | M4 |
| O：[RFC 0031](../0031-svg/README.md) | SVG | 先做 inline `<svg>` 基本圖形的繪製（12 個 suite 的頁面有圖示），再做 SVG 的 DOM（`getScreenCTM`、`createSVGPoint`）、`text`、`clipPath`、SVG 圖片。2026-10-11 完成：SVG 的 DOM（先做，解開 TodoMVC-Vue 和 NewsSite-Nuxt）、inline 的繪製（`<use>`、漸層、`text`）、幾何的 API、`<img>` 和背景的 SVG 圖片；`clipPath`、`mask`、filter、marker 在它的「之後的可能」 | XL | — | `layout`、`paint`、`dom_api` | 雜項 | M4；繪製影響量得對 |
| P：第一份是 [RFC 0038](../0038-selection/README.md) | 編輯 | `Selection`、`contenteditable`、`selectionchange`、`beforeinput`、游標。RFC 0038（2026-10-12 完成）做 script 看得到的選取、editing host 的 focus 和 `Range` 的幾何；打字和 `execCommand`、游標和選取的繪製之後各一份 | XL | I、F、D | 很多處 | DOM | M6 |
| S1 | RFC 0013 `position`（既有，2026-10-09 完成） | `relative`、`absolute`、`fixed`、有 `position` 的元素的 `z-index`；`sticky` 另外寫 | L | — | `style`、`layout`、`paint`、`compositor` | CSS | 量得對 |
| S2：[RFC 0026](../0026-backgrounds-borders/README.md) | 背景和邊框 | `background-image`（`url()`、漸層）、`background-position` / `size` / `repeat`、`border-radius`、`box-shadow`、`outline`、`dashed` 等邊框樣式，2026-10-10 完成 | L | O 的 SVG 圖片 | `style`、`paint` | CSS | 量得對 |
| S3：[RFC 0045](../0045-generated-content-lists/README.md) | 生成內容和 list | `::before`、`::after` 的 `content`、`display: list-item`、`list-style`，2026-10-14 完成 | M | — | `style`、`layout` | CSS | 量得對 |
| S4：[RFC 0041](../0041-cascade-features/README.md) | cascade 的功能 | 自訂 property 和 `var()`、`calc()`、`@media`（和 `matchMedia`）、pseudo-class（`:hover`、`:focus`、`:checked`、`:not()`、`:nth-child()`、`:root`、`:empty`、`::placeholder`…）、logical property。2026-10-13 完成：`var()`、`calc()` 和 viewport 單位、`@media`、`@supports` 和 `matchMedia`、`:hover`、`:active` 和滑鼠的邊界事件、`:lang()`、`:read-only` 這些、`:target`、邏輯屬性（照 `horizontal-tb`、`ltr`）；pseudo-element 在 S3 | L | pseudo-class 的狀態要 F、G | `style` | CSS | 量得對 |
| S5：[RFC 0043](../0043-floats-text/README.md) | `float`、`vertical-align` 和文字 | `float`、`clear`、`vertical-align`、`text-overflow`、`text-transform`、`letter-spacing`、`text-indent`、`visibility`，2026-10-14 完成 | L | — | `layout`、`paint` | CSS | 量得對 |
| S6：[RFC 0048](../0048-css-animations/README.md) | transition 和動畫 | `transition`、`@keyframes`、`animation`、Web Animations 的 JS API。2026-10-16 完成，動畫在 main thread 上跑；compositor 的動畫移到 [RFC 0049](../0049-compositor-animations/README.md)（`paint`、`compositor`、`viz`，Speedometer 用不到）；前兩段和 S7 在兩台裝置上同時做（同一條線，主要改的 crate 不同：見 RFC 0048 的 `details.md`「和 RFC 0047 同時做」） | L | — | `animation`（新）、`style`、`css`、`dom`、`dom_api`、renderer 的 rendering update | CSS | 量得對 |
| S7：[RFC 0047](../0047-tables/README.md) | table | table layout。Perf-Dashboard（default suite）的 charts 頁面每次 render 重建一個 `<table>`，在元件的 shadow tree 裡（2026-10-14 查到；RFC 0045 第 9 條說 perf.webkit.org 不屬於 default suite 是錯的）；6 個 Complex-DOM 的靜態 DOM 有一個日曆的 `<table>`。2026-10-15 完成 | L | K-2（Perf-Dashboard 的 table 在 shadow tree 裡） | `layout`、`style`、`paint` | CSS | 量得對 |
| T | RFC 0012 的工具（這份 RFC） | `README.md`「分階段」的 1b（盤點的工具放進 repo，2026-10-10 完成）、第 2 階段（量測的工具：kun 和 Chrome 接著跑、有不有效的檢查、比較表、`--trace`，2026-10-11 完成；2d 的 Perfetto 的 trace 和 `--sample`，2026-10-16 完成）；量測和比較在 RFC 0012 的 `progress.md` | M | 第 2 階段等 M1 | `scripts/ci.py`、`shell` | 雜項 | M1 之後 |
| U | wasm 的 guest 讀寫 `any` | 和 JS 引擎無關的值的模型（在 `dom`，資料不用 GC 追蹤、DOM 物件是 traced 的 edge）；WIT 的 `any-value`：一張節點的清單（WIT 不能定義遞迴的型別），節點是 `undefined`、`null`、`boolean`、`number`、`bigint`、`string`、`array`、`object`、`bytes`、`date` 和 DOM 物件的 handle，能表示循環和共用的參照；產生 WIT 的程式不再拿掉 `any` 的成員（`CustomEvent.detail`、`MessageEvent.data`、`json()`、`AbortSignal.reason`…）；wasm 文件的 `history.state` 等序列化的值也改用它；Rust、C、Lua 的 SDK 各加轉換的工具。需要身分的少數成員之後再加不透明的 handle。2026-10-10 在 [RFC 0030](../0030-testdriver/README.md) 的第 1 段討論時加的，和 Speedometer 無關 | M | — | `dom`、`dom_api` 的 WIT、`dom_wasm`、`sdk/` | 雜項 | —（和 Speedometer 無關） |

「跑得完」和「量得對」的分別見 `README.md` 的「盤點的結果」。S1 到 S7 不擋「跑得完」，但 suite 跑得完之後，async 的時間要畫面對了才有意義。

## 三條平行的線

2026-10-07 決定分成 DOM、CSS、其他雜項三條線（`README.md` 的「已決定的事項」第 4 條），三條線在三台裝置上各自進行，透過 `origin` 協調。分法是看一份工作主要改哪些 crate，不看 API 的名稱，讓同一條線裡的工作改同一批檔案，不同線之間的衝突少。

| 線 | 主要動到 | 放什麼 |
| --- | --- | --- |
| **DOM** | `dom`、`dom_api`、`dom_js`、`html` | script 看得到、和 style、layout 無關的 API：事件、表單的行為、HTML 元素的 interface、RFC 0010、shadow DOM 的樹、編輯 |
| **CSS** | `style`、`computed_style`、`layout`、`paint`、renderer 的 lifecycle | CSS 的功能、CSSOM、CSSOM View 和同步的 layout、子 frame 的 lifecycle、rendering 的 observer、控制項的繪製、shadow DOM 的 style 範圍 |
| **其他雜項** | `bindings`、renderer 的 script 和載入、`browser`、`net`、新的子系統 | ES module、導覽、`fetch`、Canvas 2D、SVG、RFC 0012 的工具 |

這樣分的考量：
- **E 的 2b-1（子 frame 的 lifecycle）放在 CSS 線**：它和 D 的同步 layout 是同一塊程式（`frame_view`、lifecycle）；放在雜項線的話，D 會卡在別條線上。
- **K 拆給兩條線**：樹、slot、事件、custom element 屬於 DOM；`:host`、style 的範圍、`adoptedStyleSheets`（C 第三段）屬於 CSS。
- **N、O 放在雜項**：它們是自成一塊的新子系統；放進 CSS 線的話，CSS 線太長。O 的 presentation attribute 會碰到 `style`，那時和 CSS 線協調。

### 每條線的順序

**DOM 線**：

| 順序 | 工作 | 里程碑 |
| --- | --- | --- |
| 0 | 前置：[RFC 0014](../0014-declaration-table/README.md) 宣告表的拆檔（「開始平行之前」），2026-10-07 完成 | M1 之前 |
| 1 | H1（RFC 0017）：`navigator`、`dataset`、`document.title`、`performance.mark` / `measure`（2026-10-08 完成） | M1 |
| 2 | F（RFC 0022）：`KeyboardEvent`、`MouseEvent`、`click()`、`focus()`、`activeElement`（2026-10-09 完成） | M1 |
| 3 | G 第一段（RFC 0023）：`<input>` 的 `value`、`checked`，checkbox 的 activation behavior；之後 `select`、`option`、`label`、`textarea`（2026-10-09 完成） | M1、M2 |
| 4 | J（RFC 0034，2026-10-12 完成；RFC 0010 的 `MutationObserver`、`queueMicrotask` 已經有了） | M2 |
| 5 | H2（RFC 0028：`Blob`、`<template>.content`、`<img>`、`crypto.randomUUID`、`readyState`…；2026-10-10 完成） | M2、M5 |
| 6 | K-1（RFC 0035，2026-10-12 完成） | M3 |
| 7 | I（`Range`）→ P（I 是 RFC 0037，P 的第一份是 RFC 0038，都在 2026-10-12 完成；M6 達成） | M6 |

**CSS 線**：

| 順序 | 工作 | 里程碑 |
| --- | --- | --- |
| 1 | C（RFC 0016）第一段：`element.style`、`setProperty`（2026-10-07 完成） | M1 |
| 2 | E 的 2b-1：子 frame 的 style 和 layout（2026-10-07 完成） | M1 |
| 3 | D：`getBoundingClientRect`、`innerWidth`；之後 `offset*`、`client*`、`scroll*`（2026-10-08 完成）；C 的第二段 `getComputedStyle` 用它的 hook，接在它之後（2026-10-09 完成） | M1、M2 |
| 4 | S1（RFC 0013，2026-10-09 完成） | 量得對 |
| 5 | S2（RFC 0026，SVG 的背景圖等雜項線的 O；2026-10-10 完成）、S3（RFC 0045，2026-10-14 完成） | 量得對 |
| 6 | S4（RFC 0041，pseudo-class 的狀態等 DOM 線的 F、G；2026-10-13 完成） | 量得對 |
| 7 | M（RFC 0036，2026-10-12 完成） | M4、M6 |
| 8 | K-2、C 第三段（等 DOM 線的 K-1；C 第三段 2026-10-12 完成，K-2 是 RFC 0046，2026-10-15 完成） | M3 |
| 9 | G 第二段（RFC 0050，等 DOM 線的 G 第一段；已接受）、S5（RFC 0043，2026-10-14 完成）、S6（RFC 0048，2026-10-16 完成）、S7（RFC 0047，2026-10-15 完成）、E 的 2b-2 和 2b-3（2026-10-07、2026-10-08 完成，提早做了） | 量得對 |

**其他雜項線**：

| 順序 | 工作 | 里程碑 |
| --- | --- | --- |
| 1 | A（RFC 0015）：先做靜態 `import`；之後動態 `import()`、`modulepreload`、script 插入的 `<script>`（2026-10-08 完成） | M1；M4、M5 |
| 2 | B（RFC 0019）：第 1、2 段 `URL`、`URLSearchParams`、`location`、`hashchange`；第 3 段 `history`（2026-10-08 完成） | M1；M5 |
| 3 | T 的 1b：盤點的工具放進 repo；kun 能跑 runner 之後，記下實際的錯誤（2026-10-10 完成） | M1 之後 |
| 4 | L（RFC 0027，2026-10-10 完成） | M4 |
| 5 | N、O（互不依賴，可以分給兩個人；O 2026-10-11、N 2026-10-12 完成） | M4 |
| 6 | T 的第 2 階段：量測的工具（2026-10-11 完成，2d 2026-10-16 完成） | 第一批 suite 跑得完之後 |

### 跨線的依賴

| 等的一方 | 被等的一方 |
| --- | --- |
| CSS 線的 S4 pseudo-class（`:focus`、`:checked`） | DOM 線的 F、G 的狀態 |
| CSS 線的 G 第二段（控制項的繪製） | DOM 線的 G 第一段 |
| CSS 線的 K-2、C 第三段 | DOM 線的 K-1 |
| DOM 線的 P（游標和選取要幾何） | CSS 線的 D |
| CSS 線的 S2 背景圖 | 雜項線的 O（SVG 圖片；2026-10-11 完成，背景的 SVG 在 O 裡做了） |
| 三條線的 M1 | 雜項線的 A：沒有 module，runner 不能執行，kun 上看不到實際的錯誤 |

## 里程碑

一個里程碑是「這些 suite 在 kun 上跑得完」（不丟例外、不卡住）。量得對（畫面對、async 有意義）由 S1 到 S7 另外推進（E 的 2b-2 在 2026-10-07 完成）。

| 里程碑 | 新增跑得完的 suite | 要的工作 | 日期 |
| --- | --- | --- | --- |
| **M1** | runner、TodoMVC-JavaScript-ES5、Preact-Complex-DOM、Svelte-Complex-DOM | A 的靜態 `import`、B、C 的 `element.style`、D 的 `getBoundingClientRect` 和同步 layout、E 的 2b-1、F、G 的 `<input>`、H1（不包括 runner 最後一步的下載連結：`README.md` 的「已決定的事項」第 9 條） | 2026-10-10（`ci.py speedometer --inventory` 第一次在 kun 上跑：三個 suite 和 runner 都跑完、不丟例外；結果見 `progress.md`） |
| **M2** | jQuery、Backbone、Vue、React-Complex-DOM、React-Redux、ES6-Webpack-Complex-DOM、Angular-Complex-DOM | M1 加上 `getComputedStyle`、D 的 `offset*` 等、G 的其他元素、J、H2 | 2026-10-11（盤點的重跑：七個都跑完、不丟例外；J 還沒做，React 的 scheduler 改用 `setTimeout`，時間要等 J 才能比；結果見 `progress.md`） |
| **M3** | WebComponents、Lit-Complex-DOM | K、C 第三段 | 2026-10-12 C 第三段之後兩個都跑完（只跑了 kun 這一側，例外沒記到）；畫面要 K-2 才有 shadow tree 的 style（K-2 2026-10-15 完成） |
| **M4** | Charts-observable-plot、Charts-chartjs、React-Stockcharts-SVG、Perf-Dashboard | A 的動態 `import()`、O、N、L、M、K-1（Perf-Dashboard 先檢查 `Element.prototype.attachShadow`，沒有就呼叫 `alert`，2026-10-12 找到） | 2026-10-12（盤點的重跑：四個都跑完、不丟例外；Charts-chartjs 的 `ResizeObserver` 是 RFC 0036 的第 1 段，Perf-Dashboard 的 shadow DOM 和 custom element 是 RFC 0035；結果見 `progress.md`） |
| **M5** | NewsSite-Next、NewsSite-Nuxt | A 的 script 插入的 `<script>`、B 的 `history`、H2 的 `crypto.randomUUID`、`<img>`、`requestIdleCallback` | 2026-10-11（盤點的重跑：兩個都跑完、不丟例外；NewsSite-Nuxt 還要 O 的 `SVGElement`；結果見 `progress.md`） |
| **M6** | Editor-CodeMirror、Editor-TipTap | P、I、M | 2026-10-12（盤點的重跑：兩個都跑完、不丟例外；TipTap 量得到，CodeMirror 的 DOM 因為排版和 Chrome 差 2 行；結果見 `progress.md`） |

M1 的關鍵路徑是雜項線的 A（ES module）和 CSS 線的 E-1 加 D（子 frame 的 lifecycle、同步 layout；E-1 在 2026-10-07 完成）：它們都要先比較設計選項，最可能是瓶頸；M1 的其他工作都比較直接。

## 開始平行之前

三條線都會加 interface、新的 GC 型別。原本建議三條線開始前先做三件事，2026-10-07 的結果：

1. **RFC 0010 第 1 階段併進 main**：已完成（RFC 0010 整份都做完了）。
2. **宣告表的拆檔**：[RFC 0014](../0014-declaration-table/README.md)，照 Chromium 的目錄拆檔，同時讓祖先的 `accepts` 從 parent 鏈推出、tag name 寫在宣告上（它的第 2、3 條）。
3. **新增 GC 型別的成本**：RFC 0010 第 8 條已經降低，不再另外處理（RFC 0014 第 4 條）。

### 三條線都會碰的檔案

拆檔之後還是共用的，改之前互相知會：
- `crates/dom_api/src/object.rs` 的 `ObjectKind`（照區塊加）、新增 GC 型別要改的幾處（`crates/dom/README.md` 的「怎麼修改」）。
- `machopo-dom.wit` 和 `kun:dom` 的版本：每次 merge 各自照 `crates/dom_api/README.md` 的規則判斷要不要升版；後 merge 的一方 rebase 時有衝突，就重新產生 golden file 和 guest（`README.md` 的「已決定的事項」第 10 條）。
- `crates/renderer/src/render_thread.rs`、`crates/renderer/src/frame_view.rs`：CSS 線（lifecycle、同步 layout）和雜項線（script 的載入、導覽）都會改。
- 每個新的 API 都同時給 JS 和 wasm（RFC 0008 的規則），工作量大約多一倍；例外（先只給 JS）在各自的 RFC 決定（`README.md` 的「已決定的事項」第 7 條）。
- 每一份 RFC 開始前都要確認它的選項：真正的瓶頸可能是審查的頻寬，不是實作。

## 待決定

2026-10-07 定下來的：原本的五項（三條線的分法、RFC 的切法、M1 的範圍、新的 API 要不要都給 wasm、關鍵路徑先開哪一份），以及 M1 不包括 runner 的最後一步、`kun:dom` 的升版，見 `README.md` 的「已決定的事項」第 4 到 10 條。之後加進來的一項也已經定了：

1. **匿名的 named getter / setter 的宣告形式由誰定**：RFC 0016 的 camelCase 屬性（CSS 線）和 RFC 0017 的 `dataset`（DOM 線）都要在宣告表加同一種形式；建議由先做到那個階段的一方定下寫法，另一方照用。2026-10-07 由 RFC 0016 的 1c 定下（它的「已決定的事項」第 8 條、`crates/dom_api/README.md` 的「怎麼新增一個 API」），RFC 0017 照用。
