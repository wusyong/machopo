# RFC 0036 的細節：`ResizeObserver` 和 `IntersectionObserver`

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## Speedometer 怎麼用

2026-10-12 讀 Speedometer `release/3.1`（`1386415`）的 build 出來的檔案（含 minified 的 bundle）。只有五個檔案用到兩個 observer：Charts-chartjs、Editor-CodeMirror、NewsSite-Nuxt，和 TodoMVC-Angular、Angular-Complex-DOM 共用的 zone.js polyfill。Editor-TipTap、NewsSite-Next（連結是 react-router 的 `<Link>`，不是 `next/link`）、Perf-Dashboard、其他 TodoMVC、Charts-observable-plot、React-Stockcharts-SVG 和 runner 都沒有用。

共通的：沒有人給 `ResizeObserver` 的 `box`（都是 `content-box`）；沒有人給 `IntersectionObserver` 的 `root`、`rootMargin`、`scrollMargin`，`threshold` 只有 CodeMirror 的 tooltip 給 `[1]`；沒有人讀 `borderBoxSize`、`contentBoxSize`、`devicePixelContentBoxSize`、`boundingClientRect`、`rootBounds`、`intersectionRect`、`time`。

**時間算在哪裡**：runner 的 `RAFTestInvoker`（`../third_party/speedometer/resources/benchmark-runner.mjs`）第一個 rAF 跑 sync 的步驟，第二個 rAF 排一個 `setTimeout`，它結束 async 的量測。suite 在同源的 iframe 裡，`ResizeObserver` 的 broadcast 在兩個 rAF 之後的同一次 rendering update 裡，所以 callback（和它強制的 layout）算進 async；callback 排的 `setTimeout`、`IntersectionObserver` 的 task 在那個 `setTimeout` 之後，不算。Chart.js、CodeMirror、Nuxt 的頁面都把 `requestAnimationFrame` 換成 `setTimeout(cb, 0)`。

| Suite | 用法 | 沒有時 |
| --- | --- | --- |
| Charts-chartjs | `createResizeObserver` 觀察 canvas 的 parent（這裡是 `<body>`），callback 讀 `entries[0].contentRect` 的寬高，都是 0 就不做事，否則 throttle 之後 `chart.resize`。「Draw scatter」「Draw opaque scatter」各建一個 chart（`reset()` 先 destroy 舊的、`disconnect`），每個新的 observer 的第一次通知在那個 frame 的 rendering update 裡 | 沒有檢查：`new Chart` → `bindResponsiveEvents` 同步丟 `ReferenceError`，丟在量測的步驟裡 |
| Editor-CodeMirror | `DOMObserver`：`ResizeObserver` 觀察 `scrollDOM`（不讀 entry，75 ms 內有更新就略過）和 `contentDOM`（`requestMeasure`）；`IntersectionObserver` 觀察 `contentDOM`（讀最後一個 entry 的 `intersectionRatio > 0`）和長文件裡沒畫的「gap」元素（每次 DOM 更新重新 observe）；tooltip 的 `IntersectionObserver` 用 `threshold: [1]`，沒有 tooltip 時不觀察任何東西。「Long」步驟建 view，observer 都在量測的 sync 步驟裡建 | `typeof X == "function"` 檢查，改聽 window 的 `resize` 和捲動容器的 `scroll` |
| NewsSite-Nuxt | `NuxtLink` 的 prefetch：每個 app 一個共用的 `IntersectionObserver`（沒有 option），讀 `target`、`isIntersecting`、`intersectionRatio`，進到畫面就 prefetch，`unobserve`，空了就 `disconnect`。observe 要等 `app:suspense:resolve` → `requestIdleCallback` → 再一個 `requestIdleCallback`（Nuxt 在 module 執行時就拿走原生的 `requestIdleCallback`，`app.vue` 之後才把 `window.requestIdleCallback` 設成 `undefined`，所以沒擋到）；runner 的 frame 一個接一個，idle 的時間不一定有，`prepare` 也不等它。所以有時在第一步之前、有時在步驟中間、有時整個 run 都沒發生，Chrome 上也是（RFC 0012 的 1b 看過） | 有時 `ReferenceError`（RFC 0012 的 2026-10-11 量測第 3 輪） |
| TodoMVC-Angular、Angular-Complex-DOM | zone.js 的 `Zone.__load_patch("IntersectionObserver")`：沒有就返回；有的話換成包一層的 class，載入時建一個 `new IntersectionObserver(function(){})`（不 observe），`for (o in i)` 列舉、讀每個屬性的 `typeof`（盤點因此記到 `delay`、`root`、`rootMargin`、`scrollMargin`、`thresholds`、`trackVisibility`）。Angular 本身不用 | 跳過，不丟例外 |

對 kun 的要求：`new IntersectionObserver(cb)` 不給 option 要能建；屬性是 prototype 上 enumerable 的 getter（宣告表本來就這樣）；沒有 v2 的成員時 zone.js 不讀，沒有影響。

## 選項

### R：範圍

| | 內容 | 評估 |
| --- | --- | --- |
| **R1** | `ResizeObserver` 三種盒子；`IntersectionObserver` 第一版加 `scrollMargin`；不含 v2 | Firefox、Safari 的範圍加上 `scrollMargin`（Chrome 有）；`resize-observer/` 和 `intersection-observer/` 最上層的大部分能跑 |
| R2 | 只做 Speedometer 用到的：`content-box`、implicit root、`threshold` | 少做 `box`、`root`、`rootMargin`、`scrollMargin`：省下的是 option 的解析和幾個分支，交集的計算（clip、跨 frame）一樣要做；WPT 大多會失敗 |
| R3 | R1 加 v2（`trackVisibility`、`delay`、`isVisible`） | 只有 Chromium 做；遮擋要對 paint 的結果做 hit test、`delay` 要 throttle，Speedometer 不用 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：R1。

### P：`ResizeObserver` 的迴圈放在哪

HTML 的 rendering update（每份 document 照順序）：autofocus、resize、scroll、media query、animation、fullscreen、animation frame callback，然後是 `ResizeObserver` 的迴圈（depth = 0；重算 style 和 layout；gather 比 depth 深的 active observation、其他的記成 skipped；沒有 active 就停；broadcast，depth = 送出的最淺的深度；回到重算），之後 skipped 還有的話發 loop error，再來是 focus fixup、view transition、`IntersectionObserver` 的「run the update intersection observations steps」、paint。

kun 現在的 `begin_main_frame`（`crates/renderer/src/render_thread.rs`）：mouse move、autofocus、scroll steps、子 frame 的 environment change、`run_animation_frame_callbacks`、`run_focus_fixup`、`update_title`，然後 `update_rendering`（`update_all_lifecycle_phases` 在 read scope 裡跑 style、layout、pre-paint、paint，接著 commit）。

| | 做法 | 評估 |
| --- | --- | --- |
| **P1** | `begin_main_frame` 的 rAF 之後、focus fixup 之前，renderer 跑迴圈：`update_style_and_layout`（整棵 frame tree）→ 每份有 observer 的 document 經過 `with_host_in_frame` 呼叫 `dom_api` 的 gather 和 broadcast → 回到第一步 | 順序和 HTML 一樣，focus fixup 也變成在 `ResizeObserver` 之後（WPT 的 `focus-fixup-rule-one-no-dialogs.html` 要的）；`update_all_lifecycle_phases` 不用拆；之後的 `update_rendering` 只做剩下的 dirty 階段 |
| P2 | 照 Chrome 放進 lifecycle：`update_rendering` 把 lifecycle 拆成「到 layout」和「pre-paint、paint」，中間放迴圈 | 結果和 P1 一樣，但 focus fixup 和 rAF 之間多夾了 lifecycle 的上半段，要搬 focus fixup 才和 HTML 一致；`update_rendering` 開頭的 form state、canvas 的 flush 和 abort 的條件要重新排 |
| P3 | `dom_api` 自己跑整個迴圈：經過 `LayoutClient::update_style_and_layout` 更新每個 target 的 document | 每份 document 各自走一次 owner 鏈；renderer 不知道迴圈跑了幾輪，`update_rendering` 的 dirty 判斷照舊能用，但每份 document 的 gather 之間要重新借 layout，和 HTML「每輪先整個更新」不同 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：P1。gather 用的 `element_box` 在 layout 乾淨時不會再做事（`FrameTreeLayout::update_style_and_layout` 只跑 dirty 的階段）。

### B、D：盒子的大小

- `content-box`、`border-box`：`LayoutClient::element_box` 已經有 border box（`border_box`）、`block` 裡的 border 和 padding（RFC 0018 第 20 條），content box 照 `frame_view.rs` 的 `content_box_size` 的算法；inline 的非 replaced 元素 `block` 是 `None`，照規範給 0×0（WPT 的 `notify.html` test6）。SVG 元素在 `svg_geometry` 有 bbox。不用另加 `LayoutClient` 的方法（B1）；另加一個「三種大小一次給」的方法（B2）只省掉 `dom_api` 的減法。
- writing mode：kun 沒有（`docs/limitations.md`），`inlineSize` 是寬、`blockSize` 是高。
- `device-pixel-content-box`：

| | 做法 | 評估 |
| --- | --- | --- |
| D1 | content box 乘 device scale factor 再四捨五入 | 不用 pre-paint；元素的位置不在整數的 device pixel 上時，Chrome 照 paint offset 對齊的結果可能差一個 pixel，`devicepixel*.html` 的 reftest 用它畫 canvas |
| **D2** | 只在有這種 observation 時，迴圈的那一輪把 lifecycle 更新到 pre-paint，用 paint offset 對齊（`LayoutClient` 多一個方法，或 `element_box` 多一個欄位） | 照 Chrome；只有用到的頁面付 pre-paint 的成本 |
| D3 | 先不做，`box: "device-pixel-content-box"` 當成 `content-box` | 安靜地錯，不選 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：B1、D2。

### W：資料放在哪

| | 做法 | 評估 |
| --- | --- | --- |
| **W1** | `dom` 的 GC 資料，照 `MutationObserver`：observation 在元素的 rare data、強參照 observer；observer 的 `WeakSet`；每個 `Window` 的弱集合給 renderer 列舉 | 參照的方向、GC 的論證、測試都有先例（`crates/dom/README.md` 的 `mutation_observers` 測試、`soundness.md` 的 `WeakSet`）；元素被回收時 observation 一起走 |
| W2 | script 的物件在 `dom`，觀察的清單在 renderer（像 lazy image 的 `FrameRequests`，`FrameClient` 登記） | renderer 拿 `NodeId` 不讓元素活著，元素被回收時要另外清；`unobserve`、`disconnect`、observer 被回收都要通知 renderer；GC 看不到兩邊的關係 |
| W3 | W1，但列舉用 agent 的 off-heap 清單（像 `MutationState` 的 `pending`） | off-heap 的清單是 root，observer 永遠不會被回收，除非 `disconnect` |

2026-10-12 確認（`README.md` 的「已決定的事項」）：W1。`Window` 是每個 realm 的 GC 物件（`crates/dom/src/event.rs`），已經拿著 `location`、`performance` 這類每個 window 一份的東西；規範把 `[[resizeObservers]]` 放在 document 上，kun 的 window 和 document 一對一，放哪邊結果一樣，Blink 的 `ResizeObserverController` 也是每個 window 一個。

### G：交集怎麼算

| | 做法 | 評估 |
| --- | --- | --- |
| **G1** | `paint::intersection`：從 pre-paint 的 `PropertyTrees` 拿 target 的 transform 和 clip 的串（`to_root`、`clip_chain`、`ScrollNode` 的 overflow clip、子 frame 的 viewport），在 lifecycle 之後算 | 照 Chrome 的 `IntersectionGeometry`（`GeometryMapper` 走 paint 的 property tree）；transform、圓角以外的 clip、跨 frame 都已經在裡面，和 hit test、繪製用同一份；只在 `update_rendering` 裡用，pre-paint 本來就做了 |
| G2 | `layout::geometry`：從 fragment 往上走、套 overflow clip 和 iframe 的內容框，在 `begin_main_frame` 裡、paint 之前算（HTML 的位置） | 不用等 pre-paint，但 clip 要再寫一份，transform 只有 `box_transform`（transform-origin 固定在中間）；lazy image 現在的檢查就是這樣做，不經過 clip |

2026-10-12 確認（`README.md` 的「已決定的事項」）：G1。圓角的 clip 先當成矩形（實作前對照 Chrome 的 `IntersectionGeometry` 怎麼處理）；`clip-path` 規範算、Chrome 算（`clip-path.html`），kun 沒有 `clip-path`。

### L：lazy image

| | 做法 | 評估 |
| --- | --- | --- |
| **L1** | 這份不換，`lazy_images_due` 照舊 | 範圍不擴大；`image-loading-lazy-negative-margin.html` 照舊失敗 |
| L2 | lazy image 改用 `paint::intersection`（top-level 的 viewport 加上距離當 root margin） | 照 Blink（`LazyLoadImageObserver` 是 `IntersectionObserver`）；要重新確認 RFC 0028 第 14、15 條的距離和子 frame 的規則、`the-img-element/` 的 baseline，`sizes="auto"` 也該改用 `ResizeObserver` 的 observation |

2026-10-12 確認（`README.md` 的「已決定的事項」）：L1，L2 列進「之後的可能」。

### X：wasm

這份開始前（2026-10-12），guest 收 callback 的 export 有 `handle-event`、`run`、`frame`、`mutations`、`accept-node`（`crates/dom_wasm/src/store.rs` 的 `Export`；RFC 0035 的第 4 段之後還有 custom element 的五個）；`Export::of` 對其他 callback 型別是 `unreachable!`，guest 少了 export 時載入失敗，`crates/dom_api/tests/it/wit.rs` 的 `the_guest_exports` 釘住清單。

| | 做法 | 評估 |
| --- | --- | --- |
| **X1** | 兩個 observer 都給 wasm，最後一段做；前兩段先 `[JsOnly]` | 照 RFC 0012 第 7 條；guest 多兩個 export（`resize-observations(id, entries, observer)`、`intersections(id, entries, observer)`，名稱第 3 段定），export 是必要的，第 3 段升 `kun:dom` 的 minor 版 |
| X2 | 只給 JS（`[JsOnly]`），列進「之後的可能」 | 省下一段；wasm 的文件不能做響應式的版面，RFC 0012 第 7 條的例外要有理由，這裡只有工作量 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：X1。

### T：分階段

| | 做法 | 評估 |
| --- | --- | --- |
| **T1** | 1 `ResizeObserver`、2 `IntersectionObserver`、3 wasm | 第 1 段做完 M4 就達成（Charts-chartjs）；兩個 observer 的資料結構相似，第 2 段沿用第 1 段的 |
| T2 | 先 `IntersectionObserver` | NewsSite-Nuxt 的有效性先解決，但 M4 晚一段；`IntersectionObserver` 的交集比較大 |
| T3 | 兩個一起一段，wasm 一段 | 一段太大，`resize-observer/` 和 `intersection-observer/` 的 baseline 一次出來 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：T1。

### A：交集多久算一次

| | 做法 | 評估 |
| --- | --- | --- |
| A1 | 有 observation 時，每次 `update_rendering` 都算全部 | kun 只在有變化時才有 main frame；沒有要追的狀態，不會漏算 |
| **A2** | 照 Chrome 的 `intersection_observation_state_`：只在 layout、捲動、pre-paint 變了，或有新的 `observe` 時才算 | 省下「有 main frame、pre-paint 沒變」的那幾種（只畫了 canvas、只換了 wheel listener）；多一個要追的狀態 |

kun 的 pre-paint 只在 `needs_pre_paint`（layout、device scale factor、子 frame）或 `ScrollState::needs_pre_paint`（main frame 的 viewport 和每個捲動容器、子 frame 的）標記時重跑（`FrameView::update_lifecycle_to_pre_paint`），交集的輸入都在裡面，所以 A2 的標記就是「pre-paint 重跑過」。

2026-10-12 確認（`README.md` 的「已決定的事項」）：A2。

### Q：entry 用哪份 document 的 task 遞送

| | 做法 | 評估 |
| --- | --- | --- |
| **Q1** | observer 的 window 的 document：每個 window 一個 task，送它的 observer（Chrome） | 和第 1 段每個 window 的弱集合一致；`DomTask` 的 `owner` 是執行它的 realm 的 document |
| Q2 | root 的 document（implicit root 的是 top-level 的 document），在它的 task 裡呼叫所有 root 在它裡面的 observer（規範的字面） | 子 frame 裡沒給 `root` 的 observer 由 top-level 的 document 的 task 送 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：Q1。

### N：guest 的兩個 export 的名稱

| | 名稱 | 評估 |
| --- | --- | --- |
| **N1** | `resizes`、`intersections` | 和 `mutations` 一樣是「送來的變化」的複數名詞，短 |
| N2 | `resize-observations`、`intersections`（草稿） | 送的是 entry，不是 observation |
| N3 | `resize-observer-callback`、`intersection-observer-callback`（WebIDL 的型別） | 和現有的 export（`handle-event`、`run`、`frame`、`mutations`、`accept-node`、`construct`……）的命名都不一樣 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：N1。

### K：Lua 的 callback 收到的東西

| | 做法 | 評估 |
| --- | --- | --- |
| **K1** | entry 是 Lua 的（可以留著），observer 是借的、只在 callback 裡（照 mutation observer） | 同類的 callback 同一個規則 |
| K2 | observer 也換成 Lua 自己的 handle（照 custom element，RFC 0035 的第 26 條） | 每次多兩次 host 呼叫；observer 通常用 closure 裡的 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：K1。

### D：wasm 的示範

| | 做法 | 評估 |
| --- | --- | --- |
| **D1** | Rust 的 demo 加兩個 observer，C 和 Lua 只有測試 | 示範一份就夠，行為都有測試 |
| D2 | C、Rust、Lua 的 demo 各一份 | 一致，但多維護三份 |
| D3 | 只有測試 | wasm 的 demo 看不出能用 |

2026-10-12 確認（`README.md` 的「已決定的事項」）：D1。

## Chromium 的做法

`ResizeObserver` 的部分 2026-10-12 對照過原始碼；`IntersectionObserver` 和 v2 依記憶整理，沒有對照（`third_party/blink/renderer/core/intersection_observer/`、`core/frame/local_frame_view.cc`），和 WPT 對不上的地方照 WPT 和 Chrome 的實際行為（`progress.md` 的「做過的檢查」）。

- **`ResizeObserver`**（2026-10-12 對照 `main` 的原始碼）：`ResizeObserverController` 每個 `LocalDOMWindow` 一個，用弱參照拿 observer；`ResizeObservation` 記 target、observed box 和上次的大小，`Element` 的 rare data 拿著 observation。`LocalFrameView::UpdateLifecyclePhasesInternal` 在 style、layout 之後、pre-paint 之前，用 `ForAllNonThrottledLocalFrameViews` 讓每個 frame 跑 `RunResizeObserverSteps`（只在目標是 paint-clean 時），它的 `NotifyResizeObservers` 用自己 window 的 controller `GatherObservations`（回傳那個 frame 的最淺深度）、`DeliverObservations`；各 frame 的結果 OR 起來，有一個要重跑就整個 lifecycle 再跑一輪。略過的 observation 清掉，每次迴圈發一次 loop error（`ResizeObserver loop completed with undelivered notifications.`，不 sanitize），再要一個 animation frame。第一次 observe 的 0×0 也通知（WPT 的 `notify.html`）。`ResizeObserverEntry` 的建構子不管 observed box，三種大小都算：`LayoutBox` 用 `ComputeZoomAdjustedBox`，SVG 用 bounding box（device pixel 那個乘 zoom 再對齊），沒有 layout object 的全是 0；`ComputeSnappedDevicePixelContentBox` 用 `FirstFragment().PaintOffset()` 的小數部分對齊（`SnapSizeToPixel`），這時 pre-paint 還沒跑，是上一個 frame 的值。
- **`IntersectionObserver`**：`IntersectionObserverController` 每個 document 一個，分 explicit root 和 implicit root 的 observation；`ElementIntersectionObserverData` 在 rare data。`LocalFrameView::RunPostLifecycleSteps` 呼叫 `ComputeIntersections`，用 `IntersectionGeometry`（`GeometryMapper` 走 paint 的 property tree）算，有 entry 就 `ScheduleIntersectionObserverForDelivery`，post 一個 task 遞送（`TaskType::kInternalIntersectionObserver`）。`observe` 呼叫 `ScheduleAnimation` 要一個 frame。`root_` 是 `WeakMember`，root 被回收時 observer 不再計算。跨源的 frame 的 target 和 implicit root：`rootBounds` 是 `null`、不套 root margin。看不見的跨源 frame 被 throttle，不計算。
- **v2**：`trackVisibility` 用 `delay`（至少 100 ms）throttle，遮擋用 hit test。

## WPT

2026-10-12 數 `../third_party/wpt`（`0f195499`）：

- **`resize-observer/`**：39 個測試（34 個 testharness、`idlharness.window.js`、3 個 reftest：`devicepixel.html`、`devicepixel2.html`、`iframe-same-origin.html`，1 個 crashtest）。用到 `box` 的 10 個，`zoom.html`（kun 沒有 `zoom`），`fragments.html`（multicol，kun 沒有），跨源的只有 `observer-in-cross-origin-frame.sub.html`。`notify.html` 的 test6、test11、test12：inline、`display: none`、0×0 都通知一次 0×0；`eventloop.html`：深度的迴圈、loop error 經過 `window.onerror`、跨 shadow root 的深度；`ordering.html`：`ResizeObserver` 比 `IntersectionObserver` 早。
- **`intersection-observer/`**：最上層 115 個（111 個 testharness、idlharness、1 個 reftest、2 個 crashtest），`v2/` 38 個。iframe 29 個、跨源 11 個、捲動約 40 個、`scrollMargin` 20 個、SVG 13 個。不用 testdriver。等 entry 的 helper（`../third_party/wpt/intersection-observer/resources/intersection-observer-test-utils.js`）是兩個 rAF 加 `step_timeout`。
- **已經在跑、等 observer 的**：`html/interaction/focus/processing-model/focus-fixup-rule-one-no-dialogs.html`（5 個 subtest 預期失敗，順序「rAF → `ResizeObserver` → focus fixup」）、`the-img-element/sizes/sizes-auto-resize-observer-removes-auto.html`、`the-img-element/image-loading-lazy-negative-margin.html`（第 9 項的 L1 之下照舊失敗）。
- 其他目錄用到 observer 的約 52 個檔案（`css/css-anchor-position`、`long-animation-frame`、`css/css-sizing/contain-intrinsic-size`、`css/css-contain/content-visibility`……），多半另外要 kun 沒有的功能。

## 參考資料

- Resize Observer：https://drafts.csswg.org/resize-observer/
- Intersection Observer：https://w3c.github.io/IntersectionObserver/
- HTML 的「update the rendering」：https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering
- IDL：`../third_party/wpt/interfaces/resize-observer.idl`、`intersection-observer.idl`（`crates/dom_api/tests/it/spec.rs` 的 `IDL_FILES` 有這兩個）
