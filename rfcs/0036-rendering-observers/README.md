# RFC 0036：`ResizeObserver` 和 `IntersectionObserver`

- 開始日期：2026-10-12
- 狀態：✅ 已完成：2026-10-12（第 1 到 18 條；分三段：「`ResizeObserver`」第 9 到 11 條、「`IntersectionObserver`」第 12 到 14 條、「wasm」第 15 到 18 條；驗收和做過的檢查在 `progress.md`）
- 細節：`details.md`；進度：`progress.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 M，CSS 線：rendering 的 observer，在 rendering update 裡通知。它依賴的 D（RFC 0018 的 `LayoutClient`、同步 layout、pre-paint 變成 lifecycle 的階段）已經完成。

## 摘要

加上 `ResizeObserver`（`content-box`、`border-box`、`device-pixel-content-box` 三種盒子，深度的迴圈和 loop error）和 `IntersectionObserver`（`root`、`rootMargin`、`scrollMargin`、`thresholds`、`takeRecords()`；不含 v2 的 `trackVisibility`、`delay`）。兩者的資料照 `MutationObserver` 放在 `dom`：observation 掛在元素的 rare data 上、強參照 observer，observer 用 `WeakSet` 記它的 observation，每個 `Window` 用弱參照記它的 observer，給 renderer 列舉。`ResizeObserver` 的迴圈在 `begin_main_frame` 裡、animation frame callback 之後、focus fixup 之前（HTML 的順序）：renderer 更新 style 和 layout，`dom_api` 經過 `LayoutClient` 拿盒子的大小、比對、同步呼叫 callback，直到沒有比上次更深的 observation。`IntersectionObserver` 在 lifecycle 跑完之後（Chrome 的 post-lifecycle），在 `paint` 新的 `intersection` 模組用 pre-paint 的 property tree 算交集（Chrome 的 `IntersectionGeometry` 用 `GeometryMapper` 的做法），commit 之後把 entry 排給 observer，每個 document 用一個 task 遞送。分三段：`ResizeObserver`、`IntersectionObserver`、wasm（JS 先做，wasm 的 guest 多兩個 export `resizes`、`intersections`，`kun:dom` 升到 0.14）。

## 動機

**現況（2026-10-12，`main` 的 `6361a8f6`）**：
- 沒有 `ResizeObserver`、`IntersectionObserver`。唯一像它們的是 renderer 的 lazy image：`FrameView::lazy_images_due`（`crates/renderer/src/frame_view.rs`）在 lifecycle 之後檢查圖片離 top-level viewport 多遠，代替 Blink 用 `IntersectionObserver` 做的 `LazyLoadImageObserver`，也記下 `sizes="auto"` 的寬度（Blink 用 `ResizeObserver`）。它不經過捲動容器的 clip，WPT 的 `image-loading-lazy-negative-margin.html` 因此失敗。
- **Speedometer**（RFC 0012 的盤點，2026-10-12；`details.md` 的「Speedometer 怎麼用」）：
  - **Charts-chartjs** 跑完但有例外：Chart.js 的 responsive 模式（預設）建 chart 時 `new ResizeObserver` 觀察 canvas 的 parent，沒有 fallback，`ReferenceError` 丟在量測的 test 裡。M4 剩下的就是這個（Perf-Dashboard 在 RFC 0035 之後跑完了）。
  - **NewsSite-Nuxt** 有時無效：`NuxtLink` 的 prefetch 在 hydration 之後、兩層 `requestIdleCallback` 之後才 `new IntersectionObserver`，有沒有在量測期間發生看時間，2026-10-11 的量測第 3 輪丟出 `ReferenceError`，照 RFC 0012 的第 29 條整個 suite 不給數字。
  - **Editor-CodeMirror** 兩個 observer 都用（兩個 `ResizeObserver`、三個 `IntersectionObserver`），有 feature detection，沒有時改聽 window 的 `resize` 和捲動；M6 另外要 P（編輯）和 I（`Range`）。
  - **TodoMVC-Angular-Complex-DOM**：zone.js 載入時檢查 `IntersectionObserver`，有的話包一層、建一個不觀察任何東西的 instance、讀它的每個屬性；沒有就跳過。
- WPT：`resize-observer/`（39 個測試）、`intersection-observer/`（最上層 115 個，`v2/` 38 個）都還沒匯入。已經在跑的 `html/interaction/focus/processing-model/focus-fixup-rule-one-no-dialogs.html` 有 5 個 subtest 預期失敗，它要的是「rAF → `ResizeObserver` → focus fixup」的順序；`the-img-element/` 有幾個測試等 `IntersectionObserver`（`tests/wpt/README.md`）。

目標：
- `ResizeObserver`：`observe(target, { box })`、`unobserve`、`disconnect`；`ResizeObserverEntry`（`target`、`contentRect`、`borderBoxSize`、`contentBoxSize`、`devicePixelContentBoxSize`）、`ResizeObserverSize`。照 HTML 在 rendering update 裡的迴圈：每一輪只送比上一輪更深的 observation，送不完時發 loop error（`ResizeObserver loop completed with undelivered notifications.`，經過 `window.onerror`）。第一次 observe 一定通知一次（`display: none`、0×0、inline 的也是，WPT 的 `notify.html`）。
- `IntersectionObserver`：`root`（`Element`、`Document` 或 implicit root：top-level 的 viewport，經過 iframe 的內容框）、`rootMargin`、`scrollMargin`、`thresholds`、`observe`、`unobserve`、`disconnect`、`takeRecords()`；`IntersectionObserverEntry`（`time`、`rootBounds`、`boundingClientRect`、`intersectionRect`、`isIntersecting`、`intersectionRatio`、`target`，和它的建構子）。第一次 observe 一定給一個 entry；之後 threshold 的位置或 `isIntersecting` 變了才給。跨源的 frame 照規範：`rootBounds` 是 `null`，不套 `rootMargin`。
- 順序和 Chrome 一樣：`ResizeObserver` 的 callback 在 rendering update 裡同步跑，比 `IntersectionObserver` 早（WPT 的 `ordering.html`）；`IntersectionObserver` 的 callback 在之後的 task。
- Speedometer 的 Charts-chartjs 有效（M4 達成），NewsSite-Nuxt 每一輪都有效；Editor-CodeMirror 在 observer 這一側沒有缺的。
- wasm 的 guest 也能用（第 7 條）。

不在範圍內：
- **v2**：`trackVisibility`、`delay`、`isVisible`、遮擋的計算。只有 Chromium 做；Firefox、Safari 都沒有，Speedometer 不用（zone.js 讀到的 `delay`、`trackVisibility` 只是列舉屬性，沒有它們時不讀）。成員記在 `crates/dom_api/tests/spec-missing.txt`。
- lazy image 改用 `IntersectionObserver` 的交集（第 6 條）。
- `content-visibility`、`contain-intrinsic-size`、anchor positioning 等其他用到 observer 的 CSS 功能（kun 沒有）。
- `PerformanceObserver`：不是 rendering 的 observer，Speedometer 裡只有 NewsSite-Next 的 web-vitals 用到，包在 `try` 裡。

## 設計概要

對寫網頁的人：

```js
const ro = new ResizeObserver(entries => {
  for (const e of entries) chart.resize(e.contentRect.width, e.contentRect.height);
});
ro.observe(container);                     // 下一個 frame 先通知一次目前的大小
// container 的 content box 變了 → 下一個 frame、layout 之後、paint 之前通知

const io = new IntersectionObserver(entries => {
  for (const e of entries) if (e.isIntersecting) prefetch(e.target);
}, { rootMargin: "200px", threshold: [0, 1] });
io.observe(link);                          // 下一個 frame 之後的 task 給第一個 entry
```

和 Chrome 一樣的地方：`ResizeObserver` 的 callback 在同一個 frame 裡（畫出來之前）；callback 改了大小，同一個 frame 裡再 layout、再通知，只送更深的元素；transform 不影響 `ResizeObserver`；`IntersectionObserver` 的交集經過捲動容器的 clip、iframe 的內容框和 transform，callback 在 task 裡；iframe 裡沒有給 `root` 的 observer 看的是最上層的 viewport。

和 Chrome 不同：沒有 v2；子 frame 不 throttle（`docs/limitations.md` 的「iframe」），看不見的 iframe 裡的 observer 照樣每個 frame 計算；`ResizeObserver` 的 device pixel 的大小用這個 frame 的 paint offset，不是上一個 frame 的（第 10 條）。

對改 kun 的人：
- `dom`：`resize_observer`、`intersection_observer` 模組，GC 型別 `ResizeObserver`、`ResizeObserverEntry`、`ResizeObserverSize`、`IntersectionObserver`、`IntersectionObserverEntry`，observation 掛在元素的 rare data，每個 `Window` 一份 observer 的弱集合（第 4 項）。
- `dom_api`：宣告表的 interface 和兩個 callback 型別；`resize_observer::gather_and_broadcast`、`report_loop_error`（gather、broadcast、loop error）和 `intersection_observer::update_intersections`（照 renderer 算好的交集更新 observation、排 entry 和 task）。
- `paint`：`intersection` 模組，從 pre-paint 的 property tree 算 target 在 root 裡的交集（第 6 項）。
- renderer：`begin_main_frame` 在 rAF 之後跑 `ResizeObserver` 的迴圈（第 2 項）；`update_rendering` 在 lifecycle 之後算交集、commit 之後交給 `dom_api`（第 7 項），和現在的 lazy image 同一個位置。
- `dom_wasm` 和 WIT：兩個 callback 各一個 guest 的 export，`resizes`、`intersections`；Lua 的 runtime 和它的 generator 跟著支援（第 10 項）。

## 詳細設計

（比較在 `details.md` 的「選項」，括號裡是那裡的代號；理由在「已決定的事項」。）

1. **範圍**（R）：`ResizeObserver` 的三種盒子，`IntersectionObserver` 的第一版加上 `scrollMargin`，不含 v2（見「動機」）。第 1 條（R1）。`scrollMargin` 是後來加進規範的，Chrome 有、20 個 WPT 測它；捲動容器的 clip 本來就要一個一個走，多套一個 margin 的成本小。
2. **`ResizeObserver` 的迴圈放在哪**（P）：HTML 的 rendering update 在 animation frame callback 之後、focus fixup 之前跑「recalc style、layout、gather、broadcast」的迴圈；kun 的 lifecycle 在 DOM 的 read scope 裡跑，中間不能執行 script（`crates/dom/soundness.md`），所以迴圈不能放進 `update_all_lifecycle_phases` 裡。第 2 條（P1）：`begin_main_frame` 在 `run_animation_frame_callbacks` 之後、`run_focus_fixup` 之前，有 `ResizeObserver` 的時候：
   - 把整棵 frame tree 更新到 pre-paint（`update_lifecycle_to_pre_paint`，第 10 條；和 rendering update 做的一樣，之後的 `update_rendering` 只跑剩下的 paint）；
   - 照 frame tree 的順序，每份有 observer 的 document 經過 `with_host_in_frame` 呼叫 `dom_api::resize_observer`：在它自己的深度 gather（拿每個 observation 的盒子（第 3 項）、和上次送出的比、照深度分成要送的和略過的），有要送的就 broadcast（每個 observer 呼叫一次 callback，callback 丟的例外照樣回報），記下送出的最淺深度當它下一輪的深度；
   - 有任何一份 document 送出了東西，回到第一步；都沒有就結束。還有略過的 document 發 loop error（`ErrorEvent`，訊息是 `ResizeObserver loop completed with undelivered notifications.`），略過的下一個 frame 送：改了它大小的是 callback 對 DOM 的修改，已經要了 main frame（Chrome 在這裡另外要一個，kun 不用，1b 實作時拿掉）。
   每份 document 的深度各自算，幾份 document 共用一個迴圈（第 11 條）。Chrome 的 `LocalFrameView::UpdateLifecyclePhases` 在 layout 之後、pre-paint 之前，用 `ForAllNonThrottledLocalFrameViews` 讓每個 frame 跑 `RunResizeObserverSteps`（`NotifyResizeObservers` 用自己 window 的 `ResizeObserverController` gather 和 deliver），有一個 frame 要重跑就整個 lifecycle 再跑一輪（2026-10-12 對照原始碼）。kun 放在 lifecycle 外面，順序和 HTML、Chrome 一樣，focus fixup 也跟著排在 `ResizeObserver` 之後（WPT 的 focus fixup 測試要的順序）。
3. **`ResizeObserver` 的大小從哪來**（B、D）：`content-box` 和 `border-box` 從 `LayoutClient::element_box`（border box、border、padding；content box 是 border box 減掉 border 和 padding）；inline 的非 replaced 元素、沒有 box 的元素（`display: none`、shadow tree 裡的）是 0×0；SVG 元素用 `svg_geometry` 的 bbox（規範：SVG 的 content box 和 border box 都是 bounding box）；`contentRect` 的大小是 content box，`x`、`y` 是 padding-left、padding-top（規範）。transform 不算（規範和 Chrome 都是 layout 的大小）。`device-pixel-content-box` 要對齊 device pixel，用 pre-paint 的 paint offset（Chrome 的 `ComputeSnappedDevicePixelContentBox` 用 `FirstFragment().PaintOffset()`，2026-10-12 對照原始碼）。每個 entry 都帶三種大小（規範的「create and populate a ResizeObserverEntry」、Chrome 的 `ResizeObserverEntry` 的建構子都三種一起算），所以有 `ResizeObserver` 時迴圈每一輪都更新到 pre-paint，用這個 frame 的 paint offset（第 10 條，修正第 3 條原本的「只在有這種 observation 時」）。第 3 條（B1、D2）。
4. **資料放在哪**（W）：第 4 條（W1）：照 `MutationObserver`（`crates/dom/src/mutation_observer.rs`）：
   - observer 是 `dom` 的 GC 型別，用 `CallbackSlot` 拿著 callback；observation（`ResizeObservation`、`IntersectionObservation`，不給 script）是 GC 物件，掛在元素的 rare data 的串列上、強參照 observer，observer 用 `WeakSet` 記它的 observation：元素活著、還在被觀察時 observer 活著，observer 不讓元素活著（Blink 的 `ElementRareData` 的 `ResizeObserverData`、`ElementIntersectionObserverData` 也是這樣拿）。
   - `ResizeObservation` 記 `observedBox` 和上次送出的大小，`IntersectionObservation` 記上次的 threshold 位置和 `isIntersecting`（`#[no_trace]` 的 `Cell`）。`IntersectionObserver` 的 `root` 是 `WeakEdge`（Blink 的 `root_` 是 `WeakMember`）。
   - 每個 `Window` 有兩個 `WeakSet`，記它的 realm 建的、有 observation 的 observer（規範的 document 的 `[[resizeObservers]]`；Blink 的 `ResizeObserverController`、`IntersectionObserverController` 也用弱參照拿 observer），renderer 列舉時從這裡拿。
   - 等著 task 的 `IntersectionObserverEntry` 照 `MutationRecord` 的做法排在 observer 上；task 拿著 observer 的 handle。
   - `observe` 照 Chrome 的 `ResizeObserver::observe`：同一個 box 再 observe 一次不做事，換了 box 才取代（排到最後、重新回報）。規範的字面是每次都取代；1d 匯入 WPT 時，`notify.html` 的 test8（callback 在同一個 frame 被呼叫兩次、每次都 observe 同一個元素）照規範會在第三輪重新回報、不夠深而發 loop error，Chrome 不會，所以照 Chrome（2026-10-12 對照原始碼）。
   - `WeakSet` 會重用空格、不保留順序，規範的串列和 Blink 的 `HeapLinkedHashSet` 都有順序，所以 observer 和 observation 建立時各拿 isolate 的一個遞增的序號，讀回時照序號排（1a 實作時加的）。
5. **`IntersectionObserver` 的 root 和座標**：implicit root 是 top-level 的 document 的 viewport：iframe 裡的 target 先被 iframe 的內容框 clip、再到父 frame，一路到主 frame（Speedometer 的 suite 都在 iframe 裡，CodeMirror、Nuxt 的 observer 都沒給 `root`）。給了 `root` 的，target 要是它的子孫（fragment tree 裡在它之下、同一份 document；`Document` 的話是同一份 document 裡），不是時 entry 的框全是 0（規範跳到全是 0 的 entry；`rootBounds` 也是 0，照 Chrome，WPT 的 `cross-document-root.html`）；沒有 box 的 target 也是（`display-none.html`）。implicit root 的 target 在沒有 frame 的 document 裡時不給 entry，進了 frame 之後才給（照 Chrome，WPT 的 `target-in-detached-document.html`）。`rootBounds` 是 root 的 padding box（有 overflow clip 時）或 border box，或 viewport，加上 `rootMargin`（百分比對 root 的大小）；`scrollMargin` 套在 target 和 root 之間每個捲動容器的 clip 上（子 frame 的 viewport 也是，WPT 的 `scroll-margin-iframe.html`）。`boundingClientRect` 和 `intersectionRect` 是 target 的 document 的 viewport 座標，`rootBounds` 是 root 的 document 的；交集算邊界相接（規範的 edge-inclusive）。`isIntersecting` 照 Chrome 看 threshold 的位置：相交、而且到了第一個 threshold（沒有面積、相交的 target 的 ratio 是 1，所以預設的 `[0]` 邊界相接也算相交；在 `[0.5]` 之下的不算，WPT 的 `isIntersecting-threshold.html`）。
6. **交集怎麼算**（G）：第 5 條（G1）：在 `paint` 加 `intersection` 模組，用 pre-paint 的 property tree（`PropertyTrees` 的 transform 和 clip 的串、`ScrollNode`）把 target 的 border box 換到 root 的空間，一路套上 clip（捲動容器的 overflow clip、iframe 的 viewport），和 hit test 用同一份資料（`paint::element_bounds` 已經會跨進子 frame）。照 Chrome：`IntersectionGeometry` 在 post-lifecycle 用 `GeometryMapper` 和 paint 的 property tree 算。
7. **`IntersectionObserver` 的時機**：`update_rendering` 在 `update_all_lifecycle_phases` 之後（read scope 裡）列舉 observation、用第 6 項算出每個的幾何，和現在的 `lazy_images_due` 同一個位置；commit 之後交給 `dom_api::intersection_observer::update_intersections`：比對上次的 threshold 位置和 `isIntersecting`，變了就建 entry（`time` 是 observer 的 window 的 `performance.now()`，同一個 frame 的都用同一個時間點）、排在 observer 上，observer 的 window 的 document 還沒有排過的話，用新的 `TaskSource::IntersectionObserver` 排一個 task 通知它的 observer（規範的「queue an intersection observer task」；用哪份 document 見第 14 條）。只在上次算過之後 pre-paint 重跑過、或 observation 變了（`observe`）時才算（第 13 條）：交集的輸入（layout、transform、每個捲動位置、子 frame、device scale factor）都是 pre-paint 的輸入。`observe()` 要一個 main frame（kun 只在有變化時才有 main frame，Chrome 的 `observe` 也 `ScheduleAnimation`）；之後交集只會因為 layout、捲動、transform 改變，都會有 main frame。render-blocked 的 document 不算（rendering update 本來就跳過它）。
8. **深度**：規範的 depth 是 flat tree 裡的深度；kun 的 shadow tree 還不排版（RFC 0035 的第 9 條），先用 shadow-including 的深度（往上走、在 shadow root 跳到 host）。K-2 有了 flat tree 之後換成 flat tree 的深度，slot 裡的節點才會不同。
9. **lazy image**（L）：Blink 的 lazy image 是一個 `IntersectionObserver`（`LazyLoadImageObserver`，root margin 依網路速度）。這份不換，`lazy_images_due` 照舊；第 6 項的交集有了之後，換過去是「之後的可能」。第 6 條（L1）。
10. **wasm**（X、N、K、D）：照 RFC 0012 第 7 條，新的 API 都給 JS 和 wasm（第 7 條，X1）。前兩段先是 `[JsOnly]`，第 3 段拿掉，五個 interface 進 WIT：
    - **guest 的 export**：兩個 callback 型別各一個 guest 的 export，形式照 `mutations`：`resizes(id, entries, observer)`、`intersections(id, entries, observer)`，entry 的 list 是 guest 的、observer 借給這次呼叫（第 16 條，N1）。export 是必要的（少了就載入失敗），`kun:dom` 升到 0.14，C、Rust、Lua 的 guest 和測試用的 guest 都要實作。`dom_wasm` 的 `Export` 多 `Resizes`、`Intersections`，三種 observer 的 callback 共用一段：拿著 observer、把 record 或 entry 轉成 guest 的 handle。
    - **rendering update 從哪裡找 observer**：renderer 經過 `with_host` 呼叫 `dom_api` 的 observer 的步驟，步驟從 host 的 `realm_window` 找 observer，所以 wasm 的 `EntryHost` 帶著那個 realm 的 window（microtask checkpoint 的沒有 realm，是 `None`）。3b 寫測試時發現原本一律是 `None`，wasm 文件的 observer 的 callback 都不會被呼叫。
    - **`root` 的 union**：`(Element or Document)` 在 `IntersectionObserverInit` 的 `root`（傳入）和 `root` 的 getter（傳回）是同一個 WebIDL 型別。有物件的 union 在 WIT 裡傳入時 case 是 borrow、傳回時是 own，一個 variant 只能是一種，所以 getter 用另一個同樣成員的 union `IntersectionRoot`；`wit.rs` 的「不能又傳入又傳回」原本只看最上層的參數，改成連 record 的欄位也看，才擋得下這種。
    - **必填又可以是 null 的成員**（`IntersectionObserverEntryInit` 的 `rootBounds`）：WIT 是 `option`，guest 的 `none` 是 null；其他可以是 null 的成員的 `none` 照舊是沒有這個成員。
    - **Lua**：callback 收到的 entry 是 Lua 的（可以留著），observer 是借的、只在 callback 裡（第 17 條，K1，照 mutation observer）。generator 補上 init 用到的欄位形式：只有物件的 union、有預設值的字串、數字或數字的 array（`threshold`）、record 裡的 record；record 裡必填的物件（`target`）用 RFC 0037 同時加的那一個（`StaticRangeInit` 的 container）。
    - **示範**：Rust 的 demo（`about:wasm-demo`）觀察自己的框，C 和 Lua 只有測試（第 18 條，D1）。

### 測試

- `dom`：GC 型別和 observation 的參照（`script_objects::`、`script::every_type_unwraps_only_as_itself`、`gc_stress::`、工作負載）：被觀察的元素讓 observer 活著，observer 不讓元素活著，`root` 被回收時 observer 不再計算，等著 task 的 entry 讓 observer 活著。
- `dom_api`：建構子和 option 的檢查（`rootMargin` 的語法、`threshold` 的範圍和排序）、`observe` 同一個 target 兩次、`unobserve`、`disconnect`、`takeRecords()`、gather 的深度、broadcast 的 entry、loop error、交集到 entry 的比對（threshold 的位置、第一次一定給）。
- renderer：迴圈的順序（rAF → `ResizeObserver` → focus fixup）、callback 改了大小之後同一個 frame 再 layout、`IntersectionObserver` 的 task 在 commit 之後；`paint` 的 `intersection`：transform、捲動容器、巢狀的 iframe、`rootMargin`、`scrollMargin`、邊界相接，和 `element_bounds` 比對同一批頁面。
- `shell` 的 testharness 小頁面：iframe 裡 implicit root 的交集、捲動之後的 entry。
- WPT：`resize-observer/`（第 1 段）、`intersection-observer/` 最上層（第 2 段），`v2/` 不收（不在範圍）；跨源的 `.sub` 測試要 wptserve 的第二個 host，試跑決定。focus fixup 的測試和 `the-img-element/` 裡等 observer 的測試重新看預期結果。
- Speedometer：盤點和量測（Charts-chartjs、NewsSite-Nuxt、Editor-CodeMirror、TodoMVC-Angular-Complex-DOM）。
- wasm：C 的測試 guest（`dom_wasm` 的 `engine::a_guests_resize_and_intersection_observers`，測試扮演 rendering update）、reftest 的 W6（Rust）和 W9（Lua，包括 init 的各種欄位形式），`wit::the_guest_exports`。
- 情境：`about:demo` 有隨 content box 改變文字的 `ResizeObserver`（`resize-observer`：改視窗大小、加 padding）和捲動清單時變色的 `IntersectionObserver`（`intersection-observer`：滾輪捲動）；`about:wasm-demo` 的 Rust script 的兩個 observer（`wasm-demo`：改視窗大小、捲到它的框）。
- mutation test：照 `docs/testing.md`。

### 分階段

第 8 條（T1）：

1. **`ResizeObserver`**（JS）：GC 型別、宣告表、`begin_main_frame` 的迴圈、三種盒子、loop error、WPT 的 `resize-observer/`。做完時 Charts-chartjs 跑得完、M4 達成，focus fixup 的測試多過幾個。
2. **`IntersectionObserver`**（JS）：GC 型別、宣告表、`paint` 的 `intersection`、`update_rendering` 的計算和 task、implicit root 經過 iframe、`rootMargin`、`scrollMargin`、WPT 的 `intersection-observer/`。做完時 NewsSite-Nuxt 穩定有效，Editor-CodeMirror 在 observer 這一側沒有缺的。
3. **wasm**（第 10 項）：兩個 callback 的 export、`kun:dom` 的版本、C、Rust、Lua 的 guest 和 SDK。排在 JS 的兩段之後，不擋 Speedometer。

## 缺點

- **五個新的 GC 型別、兩個新的 callback 型別**：每個 GC 型別要改 `dom`、`dom_api` 的 `Object` 和測試的好幾處（`crates/dom/README.md` 的「怎麼修改」）；callback 型別在 wasm 那一側要新的 export，guest 都要跟著改。
- **每個 frame 多一次列舉**：有 `ResizeObserver` 時每個 main frame 至少多一次「更新 style 和 layout、比對大小」；有 `IntersectionObserver` 時每個 main frame 算一次每個 observation 的交集。Chrome 也是每個 frame 算，只是有 throttle（看不見的跨源 frame）。Nuxt 的 footer 連結一直被觀察，每個 frame 都算。
- **`ResizeObserver` 的迴圈在 lifecycle 外面**：`begin_main_frame` 多一段可能來回好幾次的 style 和 layout；callback 改了 DOM 時，`update_rendering` 照常只跑 dirty 的階段，不會多做。
- **`ResizeObserver` 的迴圈要 pre-paint**：每個 entry 都帶 device pixel 的大小，有 `ResizeObserver` 時迴圈的每一輪都把整棵 frame tree 更新到 pre-paint（第 10 條；kun 的 pre-paint 不是增量的）。callback 沒改 layout 時，之後的 `update_rendering` 不再做 pre-paint，只是提早做。
- **wasm 的 guest 多兩個必要的 export**：不用 observer 的 guest 也要實作（C 的範例丟掉收到的；C binding 的 `*_free` 不放掉 list 裡的 handle，要逐個 drop）。

## 理由和其他做法

各項的完整比較在 `details.md` 的「選項」。重點：
- 迴圈放在 lifecycle 外面（第 2 項）：kun 的 lifecycle 拿著 read scope，不能在裡面跑 script，這是型別保證（RFC 0018 第 4 條）；放在 `begin_main_frame` 的 rAF 之後，順序和 HTML 一樣，也不用拆開 `update_all_lifecycle_phases`。
- 資料照 `MutationObserver`（第 4 項）：參照的方向、弱參照、測試和 soundness 的論證都有先例；renderer 只在列舉時讀。
- 交集用 pre-paint（第 6 項）：Chrome 的做法；transform、clip、跨 frame 已經在 property tree 裡，hit test 用的是同一份。

## 先例

- **Chromium**（`details.md` 的「Chromium 的做法」；`ResizeObserver` 的部分對照過原始碼，`IntersectionObserver` 的依記憶）：`core/resize_observer/` 的 `ResizeObserver`、`ResizeObservation`、`ResizeObserverController`（每個 window 一個）、`ResizeObserverEntry`、`ResizeObserverSize`；`LocalFrameView::RunResizeObserverSteps` 在 layout 之後跑迴圈、callback 改了 DOM 就再 layout，最後報 loop error。`core/intersection_observer/` 的 `IntersectionObserver`、`IntersectionObservation`、`IntersectionObserverController`（每個 document 一個）、`IntersectionGeometry`；`LocalFrameView::RunPostLifecycleSteps` 裡算交集，entry 排好後 post 一個 task 遞送。
- **RFC 0010**：`MutationObserver` 的資料和參照的方向。
- **RFC 0018**：`LayoutClient` 的 `element_box`、`scroll` 事件在 `begin_main_frame` 裡分派的做法、pre-paint 留在 frame 上。
- **RFC 0028**：lazy image 的檢查在 lifecycle 之後、commit 之後才排 task（它的第 14 條），`IntersectionObserver` 放在同一個位置。

## 已決定的事項

2026-10-12 確認第 1 到 8 條（比較在 `details.md` 的「選項」）。第 7 條的升版由使用者定，其他照草稿的看法。

1. **R1：`ResizeObserver` 的三種盒子，`IntersectionObserver` 的第一版加 `scrollMargin`，不含 v2**（2026-10-12）：Firefox、Safari 的範圍加上 Chrome 有的 `scrollMargin`；交集本來就要一個一個走捲動容器的 clip，多套一個 margin 的成本小。不選 R2（只做 Speedometer 用到的：省下的只是 option 的解析，交集的計算一樣要做，WPT 大多會失敗）、R3（加上 v2：只有 Chromium 做，遮擋要 hit test、`delay` 要 throttle，Speedometer 不用）。
2. **P1：`ResizeObserver` 的迴圈在 `begin_main_frame` 裡，rAF 之後、focus fixup 之前**（2026-10-12）：renderer 更新整棵 frame tree 的 style 和 layout，每份有 observer 的 document 經過 `with_host_in_frame` gather 和 broadcast，重複到沒有更深的 observation（第 2 項）。順序和 HTML 一樣，focus fixup 跟著排在後面；`update_all_lifecycle_phases` 不用拆。不選 P2（照 Chrome 放進 lifecycle：kun 的 lifecycle 拿著 read scope，要拆成兩半，focus fixup 也要搬）、P3（`dom_api` 經過 `LayoutClient` 自己跑：每份 document 各自走一次 owner 鏈，和 HTML「每輪先整個更新」不同）。
3. **B1、D2：大小從 `LayoutClient::element_box` 算，`device-pixel-content-box` 用 pre-paint 的 paint offset 對齊**（2026-10-12）：content box 是 border box 減掉 border 和 padding，不另加方法；用 pre-paint 的 paint offset 對齊，照 Chrome（什麼時候更新到 pre-paint 由第 10 條修正：每個 entry 都要這個大小）。不選 D1（乘 device scale factor 再四捨五入：位置不在整數的 device pixel 上時和 Chrome 差一個 pixel）、D3（當成 `content-box`：安靜地錯）。
4. **W1：資料照 `MutationObserver` 放在 `dom`**（2026-10-12）：observation 在元素的 rare data、強參照 observer，observer 用 `WeakSet` 記 observation，每個 `Window` 用弱集合記它的 observer 給 renderer 列舉，`root` 是 `WeakEdge`（第 4 項）。參照的方向、GC 的論證和測試都有先例；改到 GC，照 `crates/dom/soundness.md` 寫論證。不選 W2（觀察的清單放在 renderer：`unobserve`、`disconnect`、元素被回收時都要同步兩邊，GC 看不到 renderer 那邊的參照）、W3（列舉用 off-heap 的清單：它是 root，observer 不 `disconnect` 就永遠不會被回收）。
5. **G1：交集在 `paint` 的 `intersection` 模組，用 pre-paint 的 property tree 算，在 lifecycle 之後**（2026-10-12）：照 Chrome 的 `IntersectionGeometry`（`GeometryMapper`）；transform、clip、跨 frame 已經在 property tree 裡，和 hit test、繪製用同一份（第 6、7 項）。不選 G2（從 layout 的 fragment 往上走、自己套 overflow clip：clip 要再寫一份，transform 只有固定在中間的 transform-origin，和 hit test、繪製會分歧；RFC 0018 第 3 條為了效能才在 script 的路上用 layout 的幾何，這裡每個 frame 一次、本來就有 pre-paint）。
6. **L1：lazy image 這份不換**（2026-10-12）：`lazy_images_due` 照舊，改用 `paint` 的 `intersection` 列進「之後的可能」。不選 L2（這份順便換：要重看 RFC 0028 第 14、15 條的距離和子 frame 的規則、`the-img-element/` 的 baseline，和 observer 本身無關）。
7. **X1：兩個 observer 都給 wasm，排成最後一段；需要時升 `kun:dom` 的 minor 版**（2026-10-12）：照 RFC 0012 第 7 條；前兩段先是 `[JsOnly]`。guest 收 callback 的兩個新 export 照現在的做法是必要的（少了就載入失敗），所以第 3 段升 `kun:dom` 的 minor 版、C、Rust、Lua 的 guest 和測試用的 guest 一起改，不等 RFC 0035 第 4 段決定它的 export 要不要選用。不選 X2（只給 JS：wasm 的文件少一整塊 API，理由只有工作量）。
8. **T1：分三段**（2026-10-12）：1 `ResizeObserver`（做完 M4 達成）、2 `IntersectionObserver`（NewsSite-Nuxt 穩定有效）、3 wasm。不選 T2（先 `IntersectionObserver`：M4 晚一段，而它的交集比較大）、T3（兩個 observer 一段：一段太大，兩個 WPT 目錄的 baseline 一次出來）。
第 1 段開始前（2026-10-12）確認第 9 到 11 條：

9. **第 1 段分成 1a 到 1d**（2026-10-12）：1a `dom` 和 `dom_api`：GC 型別（`ResizeObserver`、`ResizeObserverEntry`、`ResizeObserverSize`、不給 script 的 `ResizeObservation`）、`Window` 的弱集合、建構子、`observe`、`unobserve`、`disconnect`、`[JsOnly]`、規範的 IDL；1b renderer 的迴圈：`content-box`、`border-box`、SVG、沒有 box 的元素、深度、loop error、`observe` 要一個 main frame、focus fixup 的順序；1c `device-pixel-content-box`；1d WPT 的 `resize-observer/`、文件、限制、`about:demo` 和情境、Speedometer（Charts-chartjs、M4）。不選三步（`device-pixel-content-box` 併進 1b：1b 太大）。
10. **E2：有 `ResizeObserver` 時，迴圈每一輪更新到 pre-paint**（2026-10-12）：每個 entry 都帶 `devicePixelContentBoxSize`，所以只要有 entry 就要 paint offset；迴圈用 `update_lifecycle_to_pre_paint` 取代只到 layout，用這個 frame 的 paint offset 對齊。callback 沒改 layout 時，之後的 `update_rendering` 不再做 pre-paint，只是提早做；callback 改了 layout 時多一次 pre-paint（kun 的 pre-paint 不是增量的）。和 Chrome 的差別：Chrome 這時 pre-paint 還沒跑，用的是上一個 frame 的 paint offset，元素剛移到小數位置的那個 frame 可能差 1 個 device pixel。修正第 3 條的「只在有這種 observation 時更新到 pre-paint」。不選 E4（用 layout 的位置的小數部分：不用 pre-paint，但祖先有 transform 或捲動位置是小數時和 paint offset 不同）、E1（照 Chrome 用上一次 pre-paint 的值：kun 的 pre-paint 對著上一次的 fragment，layout 換了 fragment 就查不到，新元素只能當 0）。
11. **H3：幾份 document 共用一個迴圈，深度各自算，照 Chrome**（2026-10-12）：每一輪更新整棵 frame tree，每份有 observer 的 document 照 frame tree 的順序在自己的深度 gather 和 broadcast，有任何一份送出就再跑一輪；loop error 每份 document 各自發（Chrome 的 `ResizeObserverController` 每個 window 記一次）。子 frame 的 callback 改了父 frame 的大小時，父 frame 同一個 frame 裡還收得到。不選 H1（照 HTML 每份 document 各自跑完一個迴圈：和 Chrome 不同，子 frame 的 callback 影響父 frame 時要等下一個 frame）。只有兩份以上的 document 都有 observer、callback 又影響別的 document 時才看得出差別。原本草稿寫「依記憶，Chrome 共用一個最淺的深度」，對照原始碼後更正。

第 2 段開始前（2026-10-12）確認第 12 到 14 條（比較在 `details.md` 的「選項」）：

12. **第 2 段分成 2a 到 2d**（2026-10-12）：2a `dom` 和 `dom_api`：GC 型別（`IntersectionObserver`、`IntersectionObserverEntry`、不給 script 的 `IntersectionObservation`）、`Window` 的弱集合、`root` 的 `WeakEdge`、等著遞送的 entry、建構子和 option（`rootMargin`、`scrollMargin` 在 `style` 用 cssparser 解析，像 canvas 的 `font`；Chrome 的 `ParseMargin` 也用 CSS 的 tokenizer）、`observe`、`unobserve`、`disconnect`、`takeRecords()`、entry 的建構子、`[JsOnly]`、規範的 IDL；2b `paint::intersection`：給 pre-paint 和一批 target、root、margin，算 target 的框、交集、root 的框，用 `paint` 的單元測試守住；2c renderer 和 `dom_api::intersection_observer::apply`：列舉、計算、比對 threshold、排 entry 和 task、`observe()` 要 main frame、implicit root 經過 iframe、跨源的 `rootBounds`、`time`；2d WPT 的 `intersection-observer/`、文件、限制、`about:demo` 和情境、Speedometer。不選「2b 併進 2c」：幾何可以單獨用單元測試和 mutation test 守住，renderer 那段每條 mutation 都要重新 link `shell`。
13. **A2：只在 pre-paint 重跑過、或 observation 變了時才算交集**（2026-10-12）：照 Chrome 的 `LocalFrameView` 的 `intersection_observation_state_`（layout、捲動、paint property 改變時標記，`observe` 也標記；依記憶，2c 實作前對照原始碼）。kun 的交集的輸入都是 pre-paint 的輸入，所以「pre-paint 重跑過」就是 Chrome 的標記；renderer 記下上次算的時候 pre-paint 和 observation 的版本。不選 A1（有 observation 時每次 `update_rendering` 都算：沒有要追的狀態，但只畫了 canvas、只換了 wheel listener 的 frame 也算一次）。不管哪個，都只走一次 fragment tree 拿到所有 target 的框，不對每個 target 各呼叫一次 `element_bounds`（每次都走整棵樹）。
14. **Q1：entry 用 observer 的 window 的 document 的 task 遞送，照 Chrome**（2026-10-12）：每個 window 一個 task，送它自己的 observer（Chrome 的 `IntersectionObserverController` 用 observer 的 execution context 的 task runner post，依記憶，2c 實作前對照原始碼）；和第 1 段每個 window 一個 observer 的弱集合一致，`DomTask` 的 `owner` 就是執行它的 realm 的 document。不選 Q2（規範的字面：root 的 document，implicit root 的是 top-level 的 document，在它的 task 裡呼叫所有 root 在它裡面的 observer：子 frame 裡沒給 `root` 的 observer 要由 top-level 的 document 的 task 送）。callback 都在 observer 的 realm 跑，差別只在子 frame 的 observer 在哪個 task。

第 3 段開始前（2026-10-12）確認第 15 到 18 條（比較在 `details.md` 的「選項」）：

15. **第 3 段分成 3a 到 3c，照 RFC 0035 第 4 段的分法**（2026-10-12）：3a `kun:dom` 升 0.14（照 RFC 0035 的第 22 條：guest 的新 export 是必要的，升 minor 版）、拿掉兩個 observer 的 `[JsOnly]`（五個 interface 進 WIT：union 是 variant、`FrozenArray` 是 list）、`dom_wasm` 的 `Export` 多兩個變體、所有 guest（C 的測試 guest、Rust 和 C 的 SDK 和 demo、Lua 的 runtime）先補空的實作並重編，少了 export 的 guest 載入失敗，所以放在第一步；3b Lua runtime 的兩個 export，C、Rust、Lua 的測試；3c 示範、文件、限制和第 3 段做完時的檢查。
16. **N1：guest 的兩個 export 是 `resizes`、`intersections`**（2026-10-12）：形式照 `mutations`，entry 的 list 交給 guest、observer 是借的（`resizes: func(id: u32, entries: list<resize-observer-entry>, observer: borrow<resize-observer>)`，`intersections` 同樣）；和 `mutations` 一樣是「送來的變化」的複數名詞。不選 N2（草稿寫的 `resize-observations`：送的不是 observation，是 entry）、N3（照 WebIDL 的型別 `resize-observer-callback`：和現有的 export 的命名都不一樣）。
17. **K1：Lua 的 callback 收到的 entry 是 Lua 的、observer 是借的，照 mutation observer**（2026-10-12）：entry 可以留到 callback 之後，observer 只在 callback 裡能用（它是另一個物件，要比較時用 closure 裡自己的）。不選 K2（照 RFC 0035 的第 26 條把 observer 換成 Lua 自己的 handle：每次呼叫多兩次 host 呼叫，observer 通常用 closure 裡的那個）。
18. **D1：示範放在 Rust 的 demo，C 和 Lua 只有測試**（2026-10-12）：Rust 的 demo 加一個 `ResizeObserver` 和一個 `IntersectionObserver`，像 `about:demo` 的；三種語言的行為都有測試。不選 D2（C、Rust、Lua 的 demo 各一份，照 RFC 0035 的 custom element：每種語言多維護一份示範）、D3（只有測試：wasm 的 demo 看不出它們能用）。

## 待決定

（沒有。各段收的 WPT 和 `disabled` 的理由在 `tests/wpt/README.md`；第 3 段沒有新的 WPT，WPT 只有 JS。）

## 之後的可能

- lazy image 改用 `paint` 的 `intersection`（第 6 條不選的 L2）：經過捲動容器的 clip，`image-loading-lazy-negative-margin.html` 會過。
- `IntersectionObserver` v2（`trackVisibility`、`delay`、遮擋）。
- K-2 的 flat tree 之後，`ResizeObserver` 的深度改用 flat tree（第 8 項），shadow tree 裡的元素有大小。
- 看不見的子 frame 不計算 observer（和 Chrome 的 throttle 一起）。
