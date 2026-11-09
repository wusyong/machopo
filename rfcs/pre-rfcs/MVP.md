# Rust Chromium-like Browser MVP 規劃

2026-09-29 · @Wusyong

2026-10-04 完成：P0 到 P4 都在 macOS 上驗收（見 `PROGRESS.md`）。原本也排在 MVP 裡的 P5（flex 和 grid）和選做的 P6（multi-process），同一天改成 RFC 0003（`rfcs/0003-flex-grid/`）和 RFC 0004（`rfcs/0004-multi-process/`），還沒排進任何階段（見「里程碑」）。

## 目標與範圍

MVP 是一個照 Chromium 架構切分的 Rust 瀏覽器：能正確顯示 HTML 和 CSS 子集，並能跑基本的 JS。規劃時預估 P0 到 P5 約 26–29 週（一個人、每週固定投入；原本估 P0 到 P4 13–17 週，2026-09-30 規劃 P2 時調成 17–20 週；2026-10-02 規劃 P4 時把 flex / grid 拆成 P5，照 P2、P3 實際的規模再調整）；實際上 P0 到 P4 在 2026-09-29 到 2026-10-04 做完，P5 改成 RFC。

**要做**

- 顯示 block、inline layout，以及文字、圖片、背景、邊框。
- 能捲動、點連結換頁，有網址列。
- 最小的 JS 整合：用 V8 執行 script，加上 event loop、事件分派、timer 和 `requestAnimationFrame`。
- Rendering pipeline 的階段和 thread 配置與 Chromium 一致，包括 property trees、Layerize、cc、Viz。

**先不做**

- `document.write`、ES modules、`fetch`、Worker、Shadow DOM、完整的 CSSOM 和 WebIDL。
- `<video>`、完整的 form 行為、各種進階 Web API。
- Flex 和 grid layout：原本是 P5，2026-10-04 改成 RFC 0003（`rfcs/0003-flex-grid/`）。
- Sandbox 和真正的 multi-process：原本是選做的 P6，2026-10-04 改成 RFC 0004（`rfcs/0004-multi-process/`）。
- 用 HTML / CSS 自訂瀏覽器的 UI（像 WebUI）：MVP 之後的目標。P2 的工具列是原生的過渡版本，見 `P2.md` 的「已決定的事項」。網頁在視窗裡的位置（書籤列、側邊欄、同時顯示多個網頁）的題目見 RFC 0002（`rfcs/0002-web-contents-layout/`）。
- 不經過 JS、直接用 wasm 寫的腳本：MVP 之後的目標，設計見 RFC 0001（`rfcs/0001-wasm-scripts/`）；P3 先做好它需要的分層（`dom_api`）。
- Stylo、WebRender、Vello：這三個會帶入非 Chromium 的架構。

## 架構總覽

2026-10-04 搬到 `docs/architecture.md` 的「Process 和 thread」，改寫成目前的樣子。

## Rendering pipeline

2026-10-04 搬到 `docs/architecture.md` 的「Rendering pipeline」。

## Crate 選型

2026-10-04 搬到 `docs/architecture.md` 的「Crate 選型」。

## Cargo workspace 結構

規劃時的 crate 切法；目前的 crate 和對應的 Chromium 目錄見 `docs/architecture.md` 的「Crate」。

## Script（JS）精簡規劃

JS 引擎用 V8（Deno 維護的 `v8` crate），和 Chromium 相同。Script 端只做「瀏覽器要能執行 script」所需的整合層，DOM API 只放驗證整合用的最小集合。

**整合層（必做）**

| 元件 | 做什麼 | 對應 Blink |
| --- | --- | --- |
| V8 初始化 | 每個 Renderer main thread 一個 `Isolate`，每個 frame 一個 `Context` | `V8PerIsolateData`、`ScriptState` |
| Script 載入與執行 | Inline 和外部的 classic `<script>`，parser-blocking | `ScriptLoader`、`ClassicScript` |
| Event loop | Task queue、microtask checkpoint、到 frame 時間時執行 rendering update | `scheduler::EventLoop` |
| Bindings 基礎 | `Object::wrap` 把 JS 物件綁到 cppgc 上的節點；同一個 Node 永遠對應同一個 wrapper | `ScriptWrappable`、`DOMDataStore` |
| 事件分派 | Capture → target → bubble，呼叫 JS listener | `EventDispatcher` |
| Timer | `setTimeout`、`setInterval`、`requestAnimationFrame` | `DOMTimer`、`ScriptedAnimationController` |
| 錯誤回報 | Uncaught exception 和 `console.log` 印到 stdout | `V8Initializer` 的 message listener |

**DOM API（最小集合）**：`getElementById`、`querySelector`、`createElement`、`appendChild`、`textContent`、`setAttribute`、`addEventListener`。這些夠證明「JS 改 DOM → 畫面更新」整條路是通的，其他 API 等需要時再加。

**三個設計決定**

1. **DOM 放在 V8 的 cppgc heap。** 和 Blink 的 Oilpan 相同，JS 和 DOM 由同一次 GC 處理，細節見下方「DOM 與 GC」。
2. **Microtask 由我們自己觸發。** 把 V8 的 microtask policy 設成 explicit，每個 task 跑完後呼叫 `perform_microtask_checkpoint`，這樣順序才符合 HTML 規範。
3. **DOM mutation 只設定 dirty flag。** 修改 DOM 時只在節點上設 dirty flag（P4 起；P1 到 P3 是 `FrameView` 的 `style_dirty`、`layout_dirty`），下一個 frame 再只重算受影響的部分。

**DOM 與 GC：直接用 cppgc（P1 就做）**

DOM 節點直接配置在 V8 的 cppgc heap 上，cppgc 就是併入 V8 的 Oilpan。JS wrapper 和 DOM 節點互相 trace，由 V8 一次 GC 處理兩邊，循環引用會自動回收。這和現在的 Blink 相同。`v8` crate（v152）已經提供 `make_garbage_collected`、`Member` / `WeakMember` / `Persistent`、`TracedReference` 和 `Object::wrap`。

1. **節點是 GC 物件。** 只有一個 GC 型別 `Node`，種類用 enum 區分。樹狀關係（parent、child、sibling）用 `Member<Node>`，event listener 用 `TracedReference`，`trace()` 由 derive macro 產生。細節見 `P1.md` 第 1 節。
2. **Wrapper 用 `Object::wrap` 綁定。** 對應 Blink 的 `ScriptWrappable`：JS 物件和節點互相連結，同一個節點永遠對應同一個 wrapper。
3. **Root 只有少數幾個，而且都在 GC heap 之外。** `FrameView` 用 `DocumentHandle`（包住 `Persistent<Node>`）持有 document；其他地方需要長期指向節點時，用 `NodeHandle`（`Persistent`）、`WeakPersistent` 或 node id。
4. **unsafe 只留在 `dom` 和 `bindings`。** 由 workspace 的 `unsafe_code = "deny"` lint 保證，只有這兩個 crate 在 crate root 用 `#![allow(unsafe_code)]` 放行；其他 crate 需要的 unsafe 實作（例如替 `ComputedStyle` 實作 `NoTrace`）也寫在 `dom` 裡。`dom` 對外提供 safe API，例如只在 main thread scope 內有效的 `NodeRef<'a>`。`style`、`layout`、`paint` 只透過這層讀取 DOM。
5. **Pipeline 的輸出不進 GC heap。** `ComputedStyle`、fragment tree、display list 都是一般的 Rust 資料（`ComputedStyle` 以 `Arc` 的形式由節點不被 trace 的欄位持有，見 `P1.md` 第 4 節）。Hit testing 要從 fragment 找回節點時，用 node id 在 isolate 的 `NodeIdMap` 裡查（`DomReadScope::node_by_id`，見 `P1.md`）。
6. **可變狀態。** GC 物件只拿得到 `&T`。GC 指標只能透過 `dom` 內部的 `Edge::set` 修改；其他可變資料用 `Cell` / `RefCell`，DOM 只在 main thread 上操作，這樣就夠用。

代價是 GC 的規則很多，漏掉一條就可能 use-after-free。P1 用型別和 derive macro 讓大部分規則由編譯器檢查，剩下的用 GC stress 等測試抓出來（見 `P1.md` 第 1 節）。

**先不做**：`innerHTML`、`classList`、`element.style`、`document.write`、`defer` / `async`、ES modules、`fetch` / XHR、Worker、Shadow DOM、CSSOM，以及 WebIDL code generator（binding 先手寫）。

## 里程碑

先把整條 pipeline 走通，再加 JS，最後做 Chromium 式的 compositor。時程是規劃時的估計，假設一個人、每週固定投入，只能當作大概的參考。

| 階段 | 時間 | 內容 | 驗收 |
| --- | --- | --- | --- |
| P0 骨架 | 1 週 | Cargo workspace、protocol crate、winit + wgpu 視窗、V8 + cpp heap 初始化 | 寫死的 display list 經 skia-safe raster 後顯示在視窗上 |
| P1 靜態渲染 | 4–5 週 | html5ever → DOM（cppgc）→ UA stylesheet + cascade → block layout + parley → property trees → PaintArtifact | 純文字加簡單 CSS 的頁面通過第一批 reftest（2026-09-30 完成，見 `PROGRESS.md`） |
| P2 像個瀏覽器 | 約 5.5 週（原本估 2–3 週，見 `P2.md`） | net 載入、圖片 decode、inline 元素的 box（background、border、padding）、捲動、連結導航、網址列 | 能瀏覽簡單的文件類網站（2026-10-01 在 macOS 上完成，Windows 上的驗收延後，見 `PROGRESS.md`） |
| P3 基本 JS | 約 5.5 週（原本估 3–4 週，見 `P3.md`） | V8（v8 crate）、`Object::wrap` bindings、event loop、事件分派、timer、rAF、最小 DOM API | 點按鈕後由 JS 修改文字，畫面跟著更新；rAF 動畫正常運作（2026-10-02 在 macOS 上完成，見 `PROGRESS.md`） |
| P4 Chromium 式 compositor | 約 4–5.5 週（原本估 3–4 週，含 flex / grid，見 `P4.md`） | cc thread（main / impl 分開、pending / active tree、在 impl thread 上捲動）、Layerize、invalidation（style、layout、paint 局部重算） | JS 卡住時捲動仍然順暢（2026-10-04 在 macOS 上完成，見 `PROGRESS.md`） |

每個階段的驗收項目，要先寫成 reftest 或 demo 頁才算完成。

原本表上還有兩個階段：P5 Flex 和 grid（約 3.5–5 週：taffy 的 flex / grid、intrinsic size、相關的 CSS 屬性和 paint 順序；驗收是通過 `css/css-flexbox`、`css/css-grid` 的 WPT 子集），和選做的 P6 Multi-process（拆出 Renderer process、ipc-channel、sandbox；驗收是一個 tab 一個 process，Renderer crash 不會拖垮 Browser）。2026-10-04 決定 MVP 到 P4 為止：P4 完成時，MVP 要證明的事（Chromium 式的 pipeline 和 thread 配置、JS、compositor）都驗收了；這兩項各自是獨立的大題目，和 MVP 的其他部分沒有相依，要先比較做法再決定什麼時候做，所以改成 RFC 0003、0004。之後排進某個階段時，照 `rfcs/README.md` 的「流程」由那個階段的 `Pn.md` 引用。

## 測試策略

2026-10-04 搬到 `docs/testing.md`。

## 與 Chromium 的刻意差異與風險

2026-10-04 搬到 `docs/architecture.md` 的「和 Chromium 刻意不同的地方」和「要注意的地方」。
