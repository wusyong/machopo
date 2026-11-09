# RFC 0084：Speedometer 用到、kun 還沒有的 API

- 開始日期：2026-10-24
- 狀態：✅ 已完成：2026-10-25（驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`（Chrome 152 和 kun 的探測、每個 suite 怎麼用這些 API、Chromium 的對照、相關的 WPT）；探測的頁面在 `probes/`

## 摘要

2026-10-24 的盤點（`bench.py speedometer --inventory`）裡，Speedometer 3.1 的 20 個 default suite 在 kun 上都跑得完，CSS、選擇器、元素都不缺，只剩一些 API。這份 RFC 照 Chrome 152 和 Blink 補上其中除了 `XMLHttpRequest`、Trusted Types 以外的全部，分六段：

1. **小的 API**：
   - `window.opener`；
   - `MouseEvent` 的 `fromElement`、`toElement`；
   - `navigator.connection`、`navigator.geolocation`、`navigator.wakeLock`；
   - `document.prerendering`。
2. **`navigator.scheduling.isInputPending()`**：照 Chrome 給每個輸入事件標上它的對象（滑鼠下面的 frame、有 focus 的 frame），compositor 標、renderer 數，只有看得到那個 frame 的頁面回 `true`。
3. **`IntersectionObserver` v2**：`delay`、`trackVisibility` 和 `isVisible`，照 Blink 真的算可見：沒有扭曲的效果、沒有被蓋住（paint 的 hit test 加上範圍的查詢）。
4. **`File` 和 `FileReader`**。
5. **`document.all`**（`HTMLAllCollection`）：要 V8 的 undetectable object，rusty_v8 的 fork 要接出一個 binding。
6. **`PerformanceObserver` 和 Navigation Timing**：`navigation` 的 entry、`performance.timing`、`performance.navigation`。

做第 4 段時找到的兩個問題也一起修了：task 分派的事件的每個 listener 回來之後沒有 microtask checkpoint，`setTimeout` 從設它的 task 開始時算（「詳細設計」的「callback 之後的 microtask checkpoint 和 timer 的時間」）。做完之後的盤點，kun 缺的只剩 `XMLHttpRequest` 和 Trusted Types（`progress.md`）。盤點在 debug build 跑時有三個 suite 停在 layout 的檢查，查出 flex 和 grid container 在 shrink-to-fit 時的寬度不對，也在這裡修了（「詳細設計」的「盤點找到的：flex 和 grid container 的 shrink-to-fit 寬度」）。

## 動機

**現況**：2026-10-24 在 `80cf9597` 上的盤點，沿用 2026-10-22 Chrome for Testing 152 的記錄（`--reuse-chrome`）。kun 缺的 API，依 suite 列：

| suite | kun 沒有的 |
| --- | --- |
| runner | `MouseEvent.fromElement`、`navigator.wakeLock`、`WakeLock.request`、`WakeLockSentinel.release` |
| TodoMVC-React-Redux、React-Stockcharts-SVG | `MouseEvent.fromElement` |
| TodoMVC-Backbone、TodoMVC-jQuery | `new XMLHttpRequest`、`window.opener` |
| TodoMVC-Angular-Complex-DOM | `FileReader`（建構子和全部成員）、`IntersectionObserver.delay`、`trackVisibility`、`navigator.geolocation` |
| TodoMVC-Vue | `document.all` |
| TodoMVC-Lit-Complex-DOM | `window.trustedTypes`、`createPolicy`、`createHTML`、`emptyScript` |
| NewsSite-Next | `document.prerendering`、`navigator.scheduling`、`PerformanceNavigationTiming.activationStart`、`type`、`PerformanceObserver`（建構子、`observe`、`disconnect`、`takeRecords`、`supportedEntryTypes`）、`PerformanceObserverEntryList.getEntries`、`PerformanceResourceTiming.responseStart` |
| NewsSite-Nuxt | `navigator.connection`、`NetworkInformation.effectiveType`、`saveData` |
| Editor-CodeMirror | `navigator.scheduling` |

**問題**：它們都不會讓 suite 跑不完。大多只是用來偵測功能，或在載入時被 patch（`details.md` 的「每個 suite 怎麼用」）：

- jQuery 每次讀 style 都讀 `window.opener`，載入時建一個 `XMLHttpRequest` 看支不支援 ajax。
- React 的 `relatedTarget` 是空的時候，退回去讀 `fromElement`。
- Angular 的 zone.js 在載入時 patch `FileReader`、`IntersectionObserver`、`navigator.geolocation`。
- Vue（core-js）偵測 `document.all` 是不是 `[[IsHTMLDDA]]`。
- Nuxt 看 `navigator.connection` 決定要不要 prefetch。
- React 的 scheduler 和 CodeMirror 在 `navigator.scheduling.isInputPending` 存在時用它。
- runner 跑的時候用 Wake Lock 讓螢幕不暗。

所以這份 RFC 不期待 Speedometer 的分數改變，要的是三件事：

- kun 走和 Chrome 一樣的路。例如 CodeMirror 的背景 parse 會問 `isInputPending()`，Chrome 回 `false`，kun 那時根本沒有這個函式，走的是另一條路。
- 盤點表清空，只剩刻意不做的項目，之後新的缺口一眼看得出來。
- 網站和 WPT：
  - `FileReader` 是上傳圖片預覽、讀檔案的標準做法；
  - `PerformanceObserver` 是 web-vitals 這類 RUM 函式庫的入口；
  - `document.all` 是 wdspec 失敗的原因之一（`docs/limitations.md`）。

  相關的 WPT：
  - `FileAPI/` 的 `File` 和讀取 36 份；
  - `performance-timeline/` 51 份、`navigation-timing/` 59 份；
  - `intersection-observer/v2/` 38 份；
  - `screen-wake-lock/` 16 份、`is-input-pending/` 12 份。

  `FileAPI/`、`performance-timeline/`、`navigation-timing/` 那時都不在 `tests/wpt/include.txt` 裡。

**這份做完之後，NewsSite-Next 和 Chrome 做的事還是不一樣**：Next 的 web-vitals 先看 `PerformanceObserver.supportedEntryTypes`，觀察的是 `paint`、`layout-shift`、`largest-contentful-paint`、`first-input` 和 `event`。這些 entry kun 不產生，加了 `PerformanceObserver` 之後 web-vitals 照樣跳過它們。要一樣，得做 Paint Timing、LCP、Layout Instability 和 Event Timing，見「不在範圍內」。

**目標**：

1. 上面列的 API 照 Chrome 152 的行為，`details.md` 有探測結果。
2. 盤點只剩 `XMLHttpRequest`（jQuery、Backbone）和 Trusted Types（Lit）。
3. 相關的 WPT 目錄加進清單，更新預期結果。`dom_api` 的 IDL 檢查（`crates/dom_api/tests/it/spec.rs`）加上 navigation-timing、resource-timing、netinfo、screen-wake-lock、is-input-pending、geolocation 的 IDL。

**要解決的 `docs/limitations.md` 的限制**：

- 「Bindings 和 DOM API」：
  - `navigator` 那一條的「沒有……其他規範加在 `Navigator` 上的成員（`wakeLock`……）」：`wakeLock` 從例子裡拿掉，其他的（`clipboard` 等）留著。
  - `Blob` 那一條的「沒有……`File`、`FileReader`」：`FileList` 留著。
  - `Performance` 那一條的「沒有……`PerformanceObserver`、Navigation Timing（`performance.timing`、`navigation` 的 entry）」，連同「其他 PerformanceTiming 的名稱（`loadEventEnd` 等）是 `InvalidAccessError`」：`measure()` 收這些名稱。
  - `IntersectionObserver` 沒有 v2 的那一條。kun 的 hit test 和 Blink 的差別若影響到 `isVisible`，寫成新的一條。
- 「WebDriver」：wdspec 的失敗原因裡的 `document.all`。

**不在範圍內**：

- **`XMLHttpRequest`**：完整的 XHR（同步和非同步、`readyState`、progress 事件、`responseType`、上傳、CORS），是一整套網路 API。Speedometer 只建一個來偵測支不支援 ajax。另外寫 RFC（「之後的可能」）。
- **Trusted Types**：要接進 `innerHTML`、`eval` 這些 sink，加上 CSP 的 `require-trusted-types-for`。Lit 有 `trustedTypes` 時每個 template 經過 policy 的 `createHTML`，所以在這一點上 Chrome 會多一次呼叫。另外寫 RFC。
- **Paint Timing、LCP、Layout Instability、Event Timing、Long Tasks、`visibility-state` 的 entry**：這些是 layout 和 paint 要產生的資料，NewsSite-Next 的 web-vitals 用的就是它們（上面）。另外寫 RFC。RFC 0082 也在考慮 `firstPaint`，見它的「待決定」第 4 項。
- **子資源的 Resource Timing**（`resource` 的 entry）：`net` 每個請求的時間 RFC 0082 已經量了，renderer 的 network agent 也拿得到。它自己的工作是每個請求的 `initiatorType`、跨 origin 時照 `Timing-Allow-Origin` 歸零、250 筆的 buffer 和 `resourcetimingbufferfull`，約 300 到 400 行，Speedometer 用不到。這份只做 `PerformanceResourceTiming` 這個 interface，因為 `PerformanceNavigationTiming` 繼承它。
- **`FileList`、`<input type=file>` 選檔案、`FileReaderSync`、`Blob.stream()`**。
- **Wake Lock 真的讓螢幕不暗**（「已決定的事項」第 2 條）、**真的定位**（第 3 條）、**真的量網路**（第 4 條）。
- **`window.open()`**：kun 的 DOM 沒有它，所以 `opener` 一律是 `null`（第 1 段）。
- **Permissions API**（`navigator.permissions`）：Wake Lock 和 Geolocation 的權限照固定的答案（「已決定的事項」第 2、3 條）。

## 設計概要

```js
window.opener                 // null；設成別的值時變成 window 自己的屬性（照 Chrome）
// mouseout 時：fromElement 是 target、toElement 是 relatedTarget；其他事件反過來
el.addEventListener("mouseover", e => e.fromElement === e.relatedTarget);   // true
new IntersectionObserver(cb, { trackVisibility: true });   // NotSupportedError：delay 至少要 100
new IntersectionObserver(cb, { trackVisibility: true, delay: 100 }).delay;   // 100
// entry.isVisible：沒有被蓋住、祖先沒有 opacity < 1、filter、旋轉時是 true
navigator.connection.effectiveType          // "4g"；rtt 0、downlink 10（Chrome 估不出網路品質時的值）
navigator.scheduling.isInputPending()       // 有還沒處理、對象是這個 frame（或同 origin 的 frame）的輸入時是 true
const lock = await navigator.wakeLock.request("screen");   // 頁面看得見時成功；切到別的分頁時自動 release
document.prerendering                       // false
```

```js
const reader = new FileReader();
reader.onload = () => img.src = reader.result;   // data:image/png;base64,…
reader.readAsDataURL(new File([bytes], "a.png", { type: "image/png" }));
// loadstart → progress → load → loadend，各自是一個 task；readyState 0 → 1 → 2
```

```js
typeof document.all          // "undefined"，可是 document.all.length 是元素的個數
document.all("main").id      // "main"：可以當函式呼叫
document.all.item(0)         // <html>
```

```js
new PerformanceObserver(list => {
  for (const e of list.getEntries()) console.log(e.name, e.startTime);
}).observe({ type: "mark", buffered: true });   // 之前的 mark 也收到；一個 task 裡收到一批
PerformanceObserver.supportedEntryTypes        // ["mark", "measure", "navigation"]（kun 產生的）
performance.getEntriesByType("navigation")[0].domContentLoadedEventEnd
performance.timing.loadEventEnd                 // 舊的 Navigation Timing，epoch 的毫秒
performance.measure("x", "navigationStart", "loadEventEnd");   // 不再丟 InvalidAccessError
```

## 詳細設計

新的 API 照 `crates/dom_api/README.md` 的「怎麼新增一個 API」寫：

- Rust 的函式放在對應的模組；
- 宣告表放在 `src/interfaces/` 照 Blink 目錄分的檔案；
- 有狀態的 interface 要一個 `dom` 的 GC 型別和 `ObjectKind`；
- 重新產生 `crates/dom_api/tests/machopo-dom.wit`，跑 `ci.py guests`。

### 第 1 段：小的 API

- **`window.opener`**（Blink 的 `DOMWindow::opener`）：
  - getter 一律是 `null`：kun 沒有 `window.open()`，連結開新分頁照 Chrome（`target=_blank` 預設 `noopener`）沒有 opener。
  - 賦值時在 window 上定義同名的自己的 data property，之後讀到那個值。HTML 說設成 `null` 時只斷開 opener、不定義屬性，但 Chrome 152 連 `null` 也定義（`details.md` 的探測），和 WebIDL 的 `[Replaceable]` 一樣，所以宣告成 `[Replaceable]` 的唯讀 attribute，`spec-differences.txt` 寫明原因。
- **`MouseEvent.fromElement`、`toElement`**（Blink 的 `MouseEvent::fromElement`、`toElement`）：
  - `mouseout`、`mouseleave`：`fromElement` 是 target、`toElement` 是 `relatedTarget`；
  - 其他事件反過來；
  - 不是 `Node` 時是 `null`。

  讀 `dom` 的 `event_related_target` 和 target，不存新的東西。
- **`navigator.connection`**（`NetworkInformation : EventTarget`，Blink 的 `NetworkInformation`）：
  - 成員照 Chrome 152 的桌面版：`effectiveType`、`rtt`、`downlink`、`saveData`、`onchange`。沒有 `type`、`downlinkMax`，那兩個只在 Android 和 ChromeOS 上有。
  - 每次讀是同一個物件。
  - 值照 Chrome 估不出網路品質時的：`4g`、`rtt` 0、`downlink` 10、`saveData` `false`，不發 `change`（「已決定的事項」第 4 條）。
- **`navigator.geolocation`**（`Geolocation`，Blink 的 `core/geolocation`）：
  - `getCurrentPosition`、`watchPosition`、`clearWatch`，加上 `GeolocationPositionError`、`GeolocationPosition`、`GeolocationCoordinates`。後兩個沒有物件，只有 interface。
  - 權限一律照「使用者拒絕」回答（「已決定的事項」第 3 條），時序照 Blink：
    - 每個請求是一個 `GeoNotifier`；
    - 第一個請求才要權限，回答比請求晚一個 task（Chrome 到 browser 的一來一回），所以比之後的 `setTimeout(0)` 晚；
    - 回答是 fatal 的錯誤：先給一次性的、再給 watch 的，同一個錯誤物件，清掉 watch；
    - `timeout` 是 0 的請求先 TIMEOUT；
    - 頁面看不見時請求等到看得見才開始。
    - `timeout` 1 ms 以上的 timer 不排：權限的回答一定比它早，fatal 的錯誤會停掉它。
  - `dom` 的 `Geolocation` 用 `Edge` 把請求串成一條鏈、用 `CallbackSlot` 存 callback（照 custom element 的 definition），task 只拿 `Geolocation` 的 handle。
- **`navigator.wakeLock`**（`WakeLock`、`WakeLockSentinel : EventTarget`，Blink 的 `modules/wake_lock`）：
  - `request(type = "screen")` 回傳 `Promise<WakeLockSentinel>`。不是 `"screen"` 時照 IDL 的 enum 丟 `TypeError`（Chrome 的 `system` 要 flag）。
  - 文件不是 fully active、頁面看不見時當場 reject `NotAllowedError`，訊息照 Blink。kun 沒有 Permissions Policy，跨 origin 的 iframe 沒有 `allow="screen-wake-lock"` 也照樣給（Chrome 拒絕），寫進 `docs/limitations.md`。
  - 權限照 Chrome 的桌面版給，回答和 geolocation 一樣晚一個 task；回答時頁面已經看不見就 reject。
  - sentinel 有 `released`、`type`、`release()`（每次回一個新的、已經 resolve 的 promise）和 `onrelease`。`release()` 在回傳前同步發 `release`。
  - 頁面變成看不見時，在 `visibilitychange` 之前 release 全部的 lock、發 `release`，照取得的順序（Chrome 照它的 hash set，不固定）。文件丟棄時不發事件，和 Blink 一樣。
  - `dom` 的 `WakeLock` 撐住 active 的 lock；lock 是 `EventTargetKind::WakeLockSentinel`，`released` 是 `AtomicBool`。
  - 不阻止螢幕變暗（「已決定的事項」第 2 條）。
- **`document.prerendering`**：一律 `false`。kun 沒有 prerender，`onprerenderingchange` 只是 event handler，那個事件不會發生。`PerformanceNavigationTiming.activationStart` 一律是 0，見第 6 段。document 專屬的 event handler（`onprerenderingchange`、`onvisibilitychange`）有自己的名稱清單：原本 `onvisibilitychange` 用元素的那一組，設定時丟 `NotSupportedError`。

### 第 2 段：`isInputPending()` 和輸入的對象

照 Chrome 152 的行為（`details.md` 的「`isInputPending()` 的對象」，「已決定的事項」第 5 條）：

- **`navigator.scheduling`**（`Scheduling`，Blink 的 `Scheduling`）：每次讀是同一個物件，只有 `isInputPending(options)`。
- **輸入的對象**（Blink 的 `WebInputEventAttribution`）：
  - 鍵盤：「有 focus 的 frame」；
  - 滑鼠、滾輪：滑鼠下面的 frame，或「不知道」；
  - 其他的：「不知道」。
- **compositor 怎麼找滑鼠下面的 frame**（cc 的 `LayerTreeImpl::FindFrameElementIdAtPoint`）：main thread 可能正在忙，所以由 compositor 在把輸入交給 main thread 時判斷。Chrome 是對 layer 做 hit test；kun 的 compositor 用區域判斷滾輪（`main_thread_scroll_hit_test_region`、`wheel_event_handler_region`），所以對象也用區域：
  - **paint** 替每個子 frame 記下它看得到的內容框，被外面的 clip 裁過，標成那個 frame 的 document。放的空間和現在的滾輪區域一樣（property trees 的根、每個捲動容器的 scroll translation、sticky 的 translation），compositor 捲動時跟著移動，不用等 main thread。沒有子 frame 的頁面不記任何東西。
  - 祖先 frame 在子 frame 之後畫的、打得到的目標（`HitTestData` 的 rect）蓋在它上面的地方，標成「不知道」（「已決定的事項」第 8 條）。cc 的規則是：打到的東西裡有不屬於最前面那個 frame、也不是它的祖先的，就放棄。
  - 子 frame 在任何圓角的 clip 或 `clip-path` 底下（包括 iframe 自己的 `border-radius`），整個標成「不知道」（cc 的 `HitTestMayBeAffectedByMask`）。最上層的 document 自己的內容在圓角或 `clip-path` 裡照樣算給它：Chrome 152 實測如此，那些內容沒有成為有 mask 的 layer。
  - 對象用 document 的 `NodeId` 識別：paint 的 `ChildFrame` 和 `dom_api` 都用它，compositor 只知道主 frame 的 `DocumentId`。
  - compositor 查那一點最上面的區域：子 frame 的、「不知道」，或都沒有，是最上層的 document。結果放進 `ToMainThread::Input`。
- **renderer 記數**（Blink 的 `PendingUserInput::Monitor`）：
  - compositor 把輸入放進 main thread 的 queue 時（`render_thread.rs` 給 compositor 的 closure，Blink 的 `WillPostInputEventToMainThread`），照對象加一；
  - main thread 取出來、開始處理時扣掉（`run_task`，Blink 的 `WillHandleInputEventOnMainThread`）。
  - 連續的事件（`mousemove`、`wheel`）和其他的分開數，`includeContinuous: true` 時才算。
  - 「不知道」的不記，和 Chrome 一樣。
  - 被合併的 `mousemove`（renderer 的 `pending_mouse_move`）一直算到它真的分派出去；被新的一筆取代時扣掉。
  - 計數在兩個 thread 之間共用，用一個 `Mutex`，Blink 是 `any_thread_lock_`。
- **`isInputPending()` 的判斷**（Blink 的 `LocalFrame::CanAccessEvent`）：`dom_api` 經過 host 的介面向 renderer 要計數，有一筆符合就回 `true`：
  - 對象是某個 frame 的 document：問的 frame 能存取它（同 origin）；
  - 對象是有 focus 的 frame：問的就是現在有 focus 的那個 frame；
  - 沒有 window 的 document：`false`。
- **和 Chrome 還可能不同的地方**：Chrome 照 layer 的範圍判斷，kun 照畫出來的範圍。好幾個蓋在 iframe 上的東西被 Chrome 合成一個 layer、範圍比實際的東西大時，點在空隙裡 Chrome 是「不知道」、kun 算給 iframe。寫進 `docs/limitations.md`。

### 第 3 段：`IntersectionObserver` v2

照 Blink（`details.md` 的「`IntersectionObserver` v2 的可見」，「已決定的事項」第 6 條）：

- **屬性**：
  - init 的 `delay`（預設 0）、`trackVisibility`（預設 `false`），observer 上讀得回來。
  - `trackVisibility` 是 `true`、`delay` 小於 100 時，照 Chrome 丟 `NotSupportedError`，訊息照抄。
  - entry 加上 `isVisible`。
- **`delay` 的節流**（Blink 的 `IntersectionObservation::MaybeDelayAndReschedule`）：同一個 observer 兩次計算之間至少隔 `delay` ms；還沒到時跳過這一次，排到時間到了的那一個 frame 再算。
- **`isVisible`**（Blink 的 `ComputeVisibilityInfo`）：`trackVisibility` 的 observer、target 和 root 相交時才算，三個條件都成立是 `true`：
  1. **沒有扭曲的效果**（`LayoutObject::HasDistortingVisualEffects`），從 pre-paint 的 property trees 判斷：
     - target 所在的 effect 和它的祖先都沒有真的效果：`opacity` < 1、filter、blend；
     - target 的 transform 和 main frame 的在同一個平面上（沒有 3D）；
     - 換算到 main frame 的矩陣只有 2D 平移和等比放大。
  2. **沒有被蓋住**（`LayoutObject::HitTestForOcclusion`）：用 target 的框做一次範圍的 hit test，最上面打到的是 target 自己才算。target 是這次 hit test 的 stop node：它的子孫都算成它；inline 的 target 打到子孫也算。這次 hit test 的規則和點擊不同，照 Blink 的那組旗標：
     - `pointer-events: none` 的也算（`kIgnorePointerEventsNone`）；
     - effective opacity 是 0 的不算（`kIgnoreZeroOpacityObjects`）；
     - 畫到盒子外面的範圍，像 `box-shadow`、`text-shadow`，也算（`kHitTestVisualOverflow`）；
     - 不限在 frame 看得到的範圍裡（`kIgnoreClipping`：Blink 的 `LayoutView::HitTest` 把範圍擴大到整份 document）；`overflow`、`clip-path` 裁掉的照樣不算（Chrome 152 實測，見下面的「paint 的 hit test」）。
  3. **frame 沒有被外面蓋住**：kun 的 iframe 都在同一個 renderer 裡，Blink 的 local frame root 就是 main frame，一定成立。第 1、2 條換算到 main frame 時，已經包含了外面的 frame 的效果和蓋在 iframe 上的東西：Chrome 152 對同 process 的 iframe 也是這樣（`details.md`）。跨 site 的 iframe 照「已決定的事項」第 11 條。
- **paint 的 hit test**（「已決定的事項」第 9、10 條）：
  - 點擊的 hit test（`Walker`）加一種矩形的位置，照上面那組規則，從上往下第一個和矩形相交的目標，target 的子孫算成 target。
  - 被旋轉的元素照矩形換過去的 quad 判斷，不是外框（Chrome 152 實測只有外框碰到的不算蓋住）。
  - 畫到盒子外面的範圍是那個元素自己的 display item 的 `visual_rect` 的聯集，加上 filter 外擴的範圍。
  - 被 `overflow`、`clip-path`、捲動容器裁掉的部分不算（Chrome 152 實測；`kIgnoreClipping` 只擴大 frame 的範圍，Blink 的 box 照樣裁掉子孫）。
  - 只有 `trackVisibility` 的 observer 才會用到，平常的點擊 hit test 不變。
- **計算的時機**：照原本的 `IntersectionObserver`，在 paint 之後算，因為要 paint 的 hit test 資料。Blink 也是在 lifecycle 走完之後算（`UpdateAllLifecyclePhases` 的最後）。entry 的 `time` 是算的那一刻（Blink 的 `GetTimeStamp`），renderer 把時間交給 `update_intersections`：原本是 commit 之後建 entry 時的 `performance.now()`，兩次的差可以比 `delay` 小（WPT 的 `delay-test.html` 偶爾失敗）。
- **paint 的 hit test 實作時改的**：text 的 item 的 `visual_rect` 是 raster invalidation 用的保守範圍（為 glyph 左右留一個字級、上下半個），照 text box 扣掉這個空間（`glyph_reach`、`svg_glyph_reach`，paint 和這裡共用），不然字附近的 target 都看不見；SVG 圖形只照它畫的 item 算，不照 hit test 多留了 stroke 的框；target 是 iframe 時，子 frame 畫的算 target 自己（Blink 的 hit test 停在 target）。

### 第 4 段：`File` 和 `FileReader`

- **`File : Blob`**（Blink 的 `File`，「已決定的事項」第 13、14 條）：
  - `new File(bits, name, { type, lastModified })`：名稱照原樣（`/` 不換），`lastModified` 預設是現在的 epoch 毫秒，往 0 取整，太大時照 Chrome 飽和；
  - `name`、`lastModified`、`webkitRelativePath`（`""`）；沒有 `lastModifiedDate`。
  - 和 `Blob` 是 `dom` 的同一個 GC 型別，多存名稱和時間；`slice()` 回 `Blob`。
- **`FileReader : EventTarget`**（Blink 的 `FileReader`、`FileReaderLoader`）：
  - `readAsArrayBuffer`、`readAsBinaryString`、`readAsText(blob, encoding)`、`readAsDataURL`、`abort()`；
  - `readyState`（`EMPTY`、`LOADING`、`DONE`）、`result`、`error`；
  - 六個 event handler。
- **讀的過程**照 File API 的「read operation」：
  - 呼叫時 `readyState` 變成 `LOADING`；
  - 之後的事件各自在一個 task 裡，放在新的 `TaskSource::FileReading`（Blink 的 `TaskType::kFileReading`）：`loadstart`，`progress`（bytes 已經在 renderer 的記憶體裡，一次就是全部；Chrome 152 讀 20MB 的 blob 也只有一個），然後 `load` 和 `loadend` 在同一個 task。`ProgressEvent` 的 `loaded`、`total` 照 bytes 數。
  - Chrome 的事件和 `Blob.text()` 的先後隨跨 process 的延遲改變（`details.md`），不模仿。
- **讀到一半**：
  - 再呼叫 `readAs*` 丟 `InvalidStateError`，訊息照 Chrome 的「The object is already busy reading Blobs.」。
  - `abort()` 同步改成 `DONE`、`result` 是 `null`，再發 `abort`、`loadend`，之後排好的 task 都不做。
  - 在 `load` 或 `abort` 的 handler 裡開始新的讀時，舊的那次不發 `loadend`（規範，「已決定的事項」第 12 條）。
- **`readAsText` 的解碼**（「已決定的事項」第 12 條）：
  - 照 File API 和 Encoding 的「decode」：BOM 優先，再看參數的 label，再看 blob 的 `type` 的 `charset`，最後是 UTF-8。
  - 用 `encoding_rs`：renderer 解碼 HTML 用的就是它，`dom_api` 要加這個依賴。kun 沒有 `TextDecoder`，不在這份裡做。
- **`readAsDataURL`**：`data:<type>;base64,…`，沒有 type 時是 `application/octet-stream`（Chrome 152）。
- **照 Chrome、和規範不同的小地方**：前後有空白的 label 不認（Encoding 會先去掉空白），讀成 UTF-8；讀完之後的 `abort()` 留著 `result`；`abort` 的 `ProgressEvent` 是 0/0、`lengthComputable` 是 `false`。
- **`dom`**：`FileReader` 是 `EventTargetKind::FileReader`：`readyState`、正在讀的那次的編號（每個 reader 自己數）、`result` 都 `#[no_trace]`；新的 `EventKind::Progress`。
- **`result` 的 `ArrayBuffer` 在同一次讀是同一個物件**（Chrome 152 實測，script 改了它也看得到）：binding 新的 `Kept<T>`（`Value::Kept`，Blink 的 `[CachedAttribute]`），JS 照讀的編號記在 wrapper 的 private property，wasm 拿值。
- **WIT 和 SDK**：回傳的 union 可以有 `ArrayBuffer`；Lua 的 generator 多了不是最後一個參數的 list（`new File()` 的 bits）和字串或 bytes 的回傳的 union（`result`）。

### callback 之後的 microtask checkpoint 和 timer 的時間

做第 4 段時對照 Chrome 152 找到、在第 5 段之前修的兩個問題（`probes/micro.html`、`fr7.html`、`cb*.html`、`tm.html`），不限於這份 RFC 的 API：

- **callback 之後的 microtask checkpoint**（HTML 的「clean up after running a callback」）：task 分派的事件的每個 listener、event handler，rAF、`ResizeObserver`、`IntersectionObserver` 的 callback 回來之後，沒有 script 在跑時做一次 microtask checkpoint。
  - `dom_api` 的 `ScriptHost::clean_up_after_callback`，在回報例外之後呼叫。V8 的 host 在 `bindings::trace::in_script` 是假時 checkpoint，像 Blink 的 `V8ScriptRunner::CallFunction` 的 `MicrotasksScope`；wasm 的預設什麼都不做。script 裡的 `dispatchEvent` 叫的 listener 不做。
  - 回報例外時分派的 `error` 進一個 script 的 entry：Blink 從 V8 的 message listener 回報，在呼叫裡面，所以兩個 `error` listener 之間沒有 checkpoint，回報完才跑 microtask。
  - checkpoint 前先結束 timer 的 nesting（「已決定的事項」第 15 條）。
  - 「notify about rejected promises」照樣在 task 的 checkpoint 之後：Chrome 152 裡一個 listener reject、下一個 listener 才接住的 promise 不回報，結果一樣。
- **timer 的時間**：`dom` 的 `Timers` 收 renderer 的時鐘的起點（`set_clock`），每次要時間時讀。timer、DOM 的 task 和 idle callback 的 timeout 都從設定、排入的那一刻算，像 Blink 的 `DOMTimer` 讀 `TimeTicks::Now()`。原本是每個 task 開始時記一次的時間，忙了 300 ms 的 task 最後設的 `setTimeout(f, 100)` 在 task 結束後馬上執行。DOM 的 task 和 timer 用同一個時鐘：只改 timer 的話，`setTimeout(0)` 之後的 `postMessage` 會跑到它前面。

### 第 5 段：`document.all`

照 Chrome 152 的探測（`details.md` 的「`document.all`」）：

- **`HTMLAllCollection`**（Blink 的 `HTMLAllCollection`）：
  - `length`、indexed getter、`item(nameOrIndex)`、`namedItem(name)`，prototype 上照 Chrome 是 `item` 在 `namedItem` 前面。不是 `HTMLCollection` 的子 interface。
  - `item` 的參數是 array index 的字串時照 index 取（`item("5")` 是第 6 個元素）。
  - 名稱照 HTML：任何元素的 `id`，加上 `a`、`button`、`embed`、`form`、`frame`、`frameset`、`iframe`、`img`、`input`、`map`、`meta`、`object`、`select`、`textarea` 的 `name`。空字串也是名稱（HTML 規定回 `null`，Chrome 照樣找）。
  - 一個時回那個元素，好幾個時回一個 `HTMLCollection`（`HTMLCollectionOrElement`），同一個名稱是同一個 live 的集合（「已決定的事項」第 17 條）。
  - 可以當函式呼叫，和 `item` 一樣。
- **集合**：`dom` 的 `CollectionFilter` 加兩種：`DocumentAll` 是 document 底下所有的元素，照 tree order，由節點撐住、每次同一個（和 `document.forms` 一樣）；`DocumentAllNamed` 是一個名稱的元素，照名稱的 weak cache，和 `getElementsByTagName` 一樣。
- **`[[IsHTMLDDA]]`**：
  - `typeof` 是 `"undefined"`，`== null` 是 `true`，轉成 boolean 是 `false`。
  - V8 的做法是 `ObjectTemplate::MarkAsUndetectable()`，rusty_v8 上游沒有接出來（`details.md` 的「rusty_v8」）。fork 加一個 `ObjectTemplate::mark_as_undetectable`（`src/template.rs` 和 `src/binding.cc` 各幾行，標 `kun:`，記進 `README.machopo.md`），重編 V8，發 `kun-v152.2.0-20`（「已決定的事項」第 16 條）。
- **宣告表**：
  - `[IsHTMLDDA] interface`：`dom_js` 在 instance template 上 `mark_as_undetectable`。
  - `[LegacyCaller] method`（WebIDL 的 `legacycaller`）：`dom_js` 在 instance template 上設 call handler，用共用的 `member_callback`，data 是那個成員的編號（V8 把被呼叫的物件當 receiver）。只給 JS：WIT 裡匿名的 indexed getter 也叫 `item`，會撞名。
- **`dom_wasm`**：guest 那一側沒有 `typeof` 的語意，`document.all` 是一般的 resource，用 indexed getter 和 `namedItem`。

### 第 6 段：`PerformanceObserver` 和 Navigation Timing

照 Chrome 152 的探測（`details.md` 的「`PerformanceObserver` 和 Navigation Timing」）：

- **`PerformanceObserver`**（Blink 的 `PerformanceObserver`、`Performance::NotifyObserversOfEntry`），只給 JS（「已決定的事項」第 19 條）：
  - `observe({ entryTypes })` 或 `observe({ type, buffered })`、`disconnect()`、`takeRecords()`。參數錯誤的 `TypeError`、換方式的 `InvalidModificationError`（新的 `ExceptionName`，WIT 的錯誤碼 25），和 console 的警告照 Chrome 的訊息。`disconnect()` 留著觀察的方式。
  - `static supportedEntryTypes`：kun 產生的種類（`mark`、`measure`、`navigation`），照字母排、凍結，每次同一個陣列。宣告表多了 `static attribute`（`MemberKind::StaticGetter`）：`dom_js` 裝成 interface object 上的 accessor，WIT 當成沒有 `this` 的函式。
  - console 的警告經過 `FrameClient::console_warning` 到 renderer 的 Log agent（`LogEntry::javascript_warning`）。
- **`dom`**：`PerformanceObserver`、`PerformanceObserverEntryList` 兩個 GC 型別。window 的 `performance` 撐住兩個列表（`EdgeChunks` 加原子的計數）：註冊的 observer，照第一次觀察的順序；有 entry 要送的 observer，照上一次送出之後第一次拿到 entry 的順序（Blink 的 `active_observers_`）。
- **加 entry 時**：
  - `dom_api` 的 `queue_entry` 先把 entry 交給觀察那個種類的 observer，再放進 `performance` 的 buffer。
  - 照 Blink 排一個 task（`TaskSource::PerformanceTimeline`，Blink 的 `TaskType::kPerformanceTimeline`），照有 entry 要送的順序把每個 observer 的 buffer 交給 callback，參數是 `PerformanceObserverEntryList`（`getEntries`、`getEntriesByType`、`getEntriesByName`，照 `startTime` 排）。
  - `buffered: true` 時把現有的同種 entry 照 `startTime` 先放進 buffer。
- **`PerformanceNavigationTiming : PerformanceResourceTiming : PerformanceEntry`**（Blink 的 `PerformanceNavigationTiming`、`DocumentLoadTiming`）：
  - 每個 document 一個 `navigation` 的 entry，`name` 是文件的網址，`startTime` 0，`duration` 是 `loadEventEnd`。window 的 `performance` 第一次被用到時（`performance::of_window`）放進 buffer，之後每次同一個物件。`load` 的 listener 都跑完之後 `dom_api::navigation_timing::load_ended` 把它交給 observer。
  - 網路那一側的時間由 browser 在 commit 時交給 renderer（「已決定的事項」第 7 條，Chromium 的 `CommitNavigationParams` 裡的 `NavigationTiming`）：
    - `NavigationLoad` 帶著最後一次請求的 `LoadTiming`、每一跳 redirect（`RedirectHop`：網址、時間、`Timing-Allow-Origin`）、`protocol`、`Content-Encoding`；`CommitNavigation` 帶著導覽的種類（`NavigationTimingType`：reload、history traversal、其他）。子 frame 導覽時的 redirect 由 render thread 記著（`frame_redirects`）。
    - renderer 的 `navigation_timing::network_timing` 是純函式，換算成相對於 time origin 的毫秒。沒有解析網址、重用的連線，時間都在 `fetchStart`；`requestStart` 是連線結束（kun 沒有量送出請求的那一刻）；HTTPS 的 `secureConnectionStart` 是 `connectStart`（TLS 包在連線裡分不開），HTTP 時是 0。
    - redirect 照 Chrome 的規則（`details.md`）：每一跳都和文件同 origin，或每一跳的 `Timing-Allow-Origin` 都是 `*` 或文件的 origin，才有 redirect 的時間和次數。
  - 文件那一側的時間由 render thread 在 `readystatechange`、`DOMContentLoaded`、`load` 的前後記下（`Milestone`，只記第一次），和 Chrome 一樣 `loadEventEnd` 在 `load` 的 listener 和它的 microtask 裡都還是 0。
  - 其他的照 Chrome：`type`、`activationStart` 0、`nextHopProtocol`、`transferSize`（`encodedBodySize` 加 300）、`encodedBodySize`、`decodedBodySize`、`responseStatus`、`renderBlockingStatus` `non-blocking`、`contentType`；service worker 的欄位是 0 或空字串；`notRestoredReasons` 是 `null`。
  - `serverTiming` 是凍結的空陣列：不解析 `Server-Timing`，`PerformanceServerTiming`、`NotRestoredReasons` 和回傳它們的兩個 attribute 只給 JS。
  - `confidence`：照 Chromium 的 randomized response（「已決定的事項」第 20 條）。
- **`performance.timing`（`PerformanceTiming`）和 `performance.navigation`（`PerformanceNavigation`）**：舊的 Navigation Timing，從同一份時間讀，值是 epoch 的毫秒（整數）。`dom` 的 `PerformanceTiming` 這個 GC 型別照 `PerformanceTimingKind` 給三種 interface：`timing` 和 `navigation` 由 `performance` 撐住，`confidence` 每次是新的。
  - `measure()` 收 PerformanceTiming 的名稱，照 Chrome 用 `performance.timing` 的整數毫秒減去 `navigationStart`；還沒發生的是 `InvalidAccessError`，`mark()` 用這些名稱是 `SyntaxError`，訊息照 Chrome。
- **trace**（「已決定的事項」第 18 條）：每個時間點照 Blink 的 `DocumentLoadTiming`、`DocumentTiming` 在 `blink.user_timing` 寫一個 mark（`navigationStart`、`fetchStart`、`responseEnd`、`commitNavigationEnd`、`domLoading`、`domInteractive`、`domContentLoadedEventStart` / `End`、`domComplete`、`loadEventStart` / `End`），走 `performance.mark()` 的那條路。

### 盤點找到的：flex 和 grid container 的 shrink-to-fit 寬度

做完第 6 段的盤點用 debug build 跑，NewsSite-Next、NewsSite-Nuxt、Editor-CodeMirror 停在 `layout` 的檢查：非換行的 column flexbox 只量寬度時取子項的貢獻（`column_flex_width`），debug build 每次也跑 taffy 比對，兩邊不同（例如 364 和 1640 px）。0084 之前的 `main` 一樣失敗，release build 不做這個比對。

- **原因**：column 的子項是 row flexbox 或有 `fr` track 的 grid 時，taffy 在確定的空間（364 px）裡量它，回傳的寬度不受空間限制。row flexbox 取 item 的 flex base size 的總和，grid 的 `fr` track 照 item 的 max-content 貢獻。kun 的快速路徑照 Blink 取 fit-content，所以兩邊不同。
- **不只這三頁**：對照 Chrome 152（`probes/fit.html`），float、絕對定位的盒子、inline-block 和 `width: fit-content` 裡的 row flexbox 和 `fr` 的 grid，或它們本身就是 float、絕對定位的盒子時，kun 都照內容不斷行的寬度排。換行的 column flexbox 也一樣：taffy 給它全部的空間。例如 250 px 的空間裡，kun 是 400、800、824 px，Chrome 都是 250 px。
- **修法**：`layout` 的 `BoxTree::fit_content_width`。flex 和 grid container 寬度未知、空間確定時，寬度取 fit-content：在空間的寬度和 min-content 寬度之間取大的，不超過 max-content 寬度。Blink 的 shrink-to-fit 照任何盒子的 min/max size 算，是同一個公式。
  - min-content、max-content 寬度用 taffy 自己量的，經過它的快取。
  - 只要寬度時直接回傳；要排版或高度時，用那個寬度當已知的寬度交給 taffy。Blink 也是先定 inline size，再在那個寬度裡排。
  - 非換行的 column 照樣先走 `column_flex_width`，它的結果本來就是 fit-content。
- **結果**：探測的 39 種情況，除了兩個本來就不同的以外都和 Chrome 一樣。那兩個是 taffy 的 intrinsic 寬度不同（換行的 flexbox 的 item 有 `flex-basis`）和 scrollbar 的寬度。WPT 的 `css/css-flexbox/negative-margins-001.html` 因此通過，`docs/limitations.md` 拿掉了「在確定的空間裡照內容算寬度時沒算進 item 的負 margin」。20 個 default suite 在 debug build 都跑完。
- **代價**：這種 container 被量寬度時，多量 min-content 和 max-content 兩次（有快取的話不用重量）。量的時候只算寬度、不斷行（`flex::what_shrinks_to_fit_breaks_its_lines_at_its_width` 數斷行的次數）。

### 測試

清單在 `progress.md` 的「驗收」。

- **每個成員的行為照 `details.md` 的 Chrome 探測**：`dom_js` 的整合測試跑同一段 script（`bindings::`、`navigator::`、`geolocation::`、`wake_lock::`、`scheduling::`、`intersection_observers::`、`file_reader::`、`document_all::`、`performance_observers::`、`navigation_timings::`）。GC 型別在 `dom` 的整合測試和 GC stress 的 workload。
- **要經過真的 compositor、paint 和網路的**在 `shell` 的整合測試（headless 的整個瀏覽器）：`isInputPending()` 讓 main thread 忙著問它、期間從測試送輸入；`IntersectionObserver` v2 的 `delay` 和蓋住；`FileReader` 經過 event loop；navigation entry 的 redirect、reload 和 trace。原本打算放在 `renderer`，但 `renderer` 自己沒有經過 compositor thread 和 hit test 的測試的架子。
- **`paint`、`compositor`、`renderer` 的測試**：對象區域、扭曲的效果、範圍的 hit test 的每一條規則；compositor 照點和捲動找對象；計數和網路的時間的純函式。
- **IDL**：`crates/dom_api/tests/it/spec.rs` 加上新的 IDL 檔，缺的成員記進 `spec-missing.txt`，刻意不同的記進 `spec-differences.txt`。
- **WPT**：加進清單，照 `MACHOPO_UPDATE_EXPECTATIONS` 寫預期結果，失敗的原因寫進 `.ini`：
  - `netinfo/`、`geolocation/`、`screen-wake-lock/`、`is-input-pending/`；
  - `intersection-observer/v2/`（拿掉 `__dir__.ini` 的 `disabled`）；
  - `FileAPI/` 的 `file/`、`FileReader/`、`reading-data-section/`；
  - `html/infrastructure/common-dom-interfaces/collections/` 的 `htmlallcollection.html` 和 `document-all.html`；
  - `performance-timeline/`、`navigation-timing/`。

  要 popup、Permissions API 和 testdriver 權限的失敗。
- **layout**：shrink-to-fit 的寬度在 `layout` 的整合測試（`flex::`、`grid::`），期望值是 Chrome 152 跑同樣的 HTML（Ahem）的結果。
- **mutation test**：每段各一份 spec（`mutants-*.toml`）。
- **驗收**：重跑 `bench.py speedometer --inventory`，缺的只剩 `XMLHttpRequest` 和 Trusted Types。這只是盤點，不計時；Speedometer 的時間照 `docs/benchmarks.md` 只在使用者要求時量。

### 分階段

1. **小的 API**（第 1 段）：每個 API 各自一個 commit，不互相依賴。
2. **`isInputPending()`**（第 2 段）：
   - 先做 renderer 的計數、鍵盤和最上層的 document，加上 `navigator.scheduling`；
   - 再做 paint 的對象區域和 compositor 的判斷。

   兩步都做完才 merge：只有前一步時，滑鼠在跨 origin 的 iframe 上會算錯給最上層。它跨 paint、protocol、compositor、renderer 和 `dom_api`，和其他段的 DOM API 分開審。
3. **`IntersectionObserver` v2**（第 3 段）：
   - 先做屬性、檢查和 `delay` 的節流；
   - 再做 `isVisible` 的扭曲效果；
   - 最後做 hit test 的範圍查詢和蓋住的判斷。

   三步都做完才 merge，不在 `main` 上先放一個 `isVisible` 一律 `false` 的版本。
4. **`File`、`FileReader`**（第 4 段）。
   第 4 段找到的 microtask checkpoint 和 timer 的時間在第 5 段之前修（「callback 之後的 microtask checkpoint 和 timer 的時間」）。
5. **`document.all`**（第 5 段）：先改 fork、在本機編 V8、做 kun 這一側，再發 release、換 tag（「已決定的事項」第 16 條）。
6. **`PerformanceObserver`、Navigation Timing**（第 6 段）：observer 先做，它只要 `mark`、`measure`；再做 navigation 的 entry 和舊的 `performance.timing`。

做完之後的盤點找到的 flex 和 grid 的寬度，在 merge 之前修（「盤點找到的：flex 和 grid container 的 shrink-to-fit 寬度」、「已決定的事項」第 21 條）。

## 缺點

- **API 有了，效果沒有**：
  - Wake Lock 不讓螢幕不暗；
  - Geolocation 永遠拒絕；
  - `navigator.connection` 是固定值。

  網站看到 API 存在，會以為功能也有。這幾點照 Chrome 的「使用者拒絕」或「估不出來」的樣子回答，寫進 `docs/limitations.md`。
- **fork 多一個修改**：`MarkAsUndetectable` 的 binding。每次跟上游都要重新套用，改的是 C++，要重編 V8。
- **`document.all` 這個怪東西**：`[[IsHTMLDDA]]` 讓 JS 的語意出現例外，`dom_js` 多一條特別的路。
- **paint 的 hit test 多一種查詢**：`IntersectionObserver` v2 的範圍查詢和點擊的規則不同，`HitTestData` 要多記 `pointer-events: none` 的盒子和畫到外面的範圍。只有 `trackVisibility` 的 observer 用到。
- **`isInputPending()` 的對象要 paint 多記一份區域**：每次 paint 有子 frame 時，要檢查子 frame 之後畫的東西有沒有蓋到它；protocol 和 compositor 多一種區域要維護。只為了一個 API，但 Chrome 也為它在 cc 裡做 hit test。
- **navigation 的時間要跨 process**：browser 量的時間要在 commit 時交給 renderer，`CommitNavigation` 多了導覽的種類，`NavigationLoad` 多了時間、redirect 和 header 的資料（大到 `loaded` 要 `Box`，讓 `MainThreadTask` 的 variant 不差太多）。

## 理由和其他做法

- **一份 RFC 分六段，不是每個 API 各一份**：大多是幾十行的小 API，各自寫 RFC 的成本比實作還大。`document.all` 和 Navigation Timing 有自己的設計問題（fork、跨 process），所以各自一段，可以分開 merge。
- **不做 `XMLHttpRequest`、Trusted Types**：見「不在範圍內」。它們各自是一個子系統，Speedometer 只拿來偵測或多一次呼叫。
- **只補 API、不補效果的那幾個**：
  - **Wake Lock**：kun 的 `unsafe` 只能在 `dom`、`bindings`、`ipc`，系統呼叫要 FFI（「已決定的事項」第 2 條）。
  - **Geolocation**：要平台的定位服務和權限的 UI。
  - **`connection`**：要網路品質的估計（Chromium 的 `NetworkQualityEstimator`）。

  有 API 照「拒絕」或「固定值」回答，比沒有 API 更接近 Chrome：網站用 Chrome 的那條路，只是得到使用者拒絕時的結果。
- **不做**：盤點表上一直留著這些項目，新的缺口不容易看出來；CodeMirror、React 的 scheduler 照樣走沒有 `isInputPending` 的路。

## 先例

- **Chromium**：每個 API 照 Blink 的對應類別，見各段，細節和程式的位置在 `details.md` 的「Chromium 的對照」。不照做的：
  - Wake Lock 不經過 `device::mojom::WakeLock` 和 `PowerSaveBlocker`；
  - `NetworkInformation` 不接 `NetworkStateNotifier` 的估計；
  - Geolocation 沒有 `GeolocationService`。
- **Firefox**：
  - 沒有 `navigator.connection`（桌面版）、`navigator.scheduling`、`IntersectionObserver` v2；
  - 有 `document.all`、`FileReader`、`PerformanceObserver`；
  - Wake Lock 在 Firefox 126 起有。

  kun 照 Chrome，所以 Firefox 沒有的也做。Firefox 的 `document.all` 在 SpiderMonkey 上用 `JSCLASS_EMULATES_UNDEFINED`，和 V8 的 undetectable 是同一件事。

## 已決定的事項

2026-10-24（使用者，「待決定」的第 1、2、6 項照建議）：

1. **範圍照「摘要」**：`XMLHttpRequest`、Trusted Types、Paint Timing 這類由 layout 和 paint 產生的 entry、子資源的 Resource Timing 都另外寫 RFC（「不在範圍內」）。不選每個 API 各寫一份：大多是幾十行的小 API，寫 RFC 的成本比實作還大。
2. **Wake Lock 只做 API（W1）**：sentinel、release 的時機和事件照 Chrome，不阻止螢幕變暗，記進 `docs/limitations.md`。不選 W2（照 Chromium 的 `PowerSaveBlocker` 真的阻止）：三個平台各要一套系統呼叫，要 FFI，`unsafe` 不能放在 `shell`；Speedometer 和 WPT 都看不出差別。W2 留在「之後的可能」。
3. **Geolocation 一律照「使用者拒絕」回答（G1）**：error callback 拿到 `PERMISSION_DENIED`，時序和訊息照 headless 的 Chrome 在使用者拒絕時的探測。不選 G2（不做）：盤點表上會一直留著 Angular 的這一項，zone.js 也走和 Chrome 不同的路。

2026-10-24（使用者，「待決定」第 3 項）：

4. **`navigator.connection` 照 Chrome「估不出網路品質」時的值（N1a）**：`effectiveType` `4g`、`rtt` 0、`downlink` 10、`saveData` `false`，永遠不發 `change`，`docs/limitations.md` 寫明是固定值。這是 Chromium 本來就有的分支：`NetworkInformation` 把 unknown 對應到 `4g`，`NetworkStateNotifier::RoundRtt` 沒有估計時回 0，`RoundMbps` 回最快的 10 Mbps（`details.md` 的「`navigator.connection` 的值」）。不選 N1b（固定成某台機器量到的典型值，例如 `rtt` 100、`downlink` 1.75）：數字沒有出處，Chromium 也沒有這種寫法。不選 N2（照 `NetworkQualityEstimator` 從 `net` 的時間真的估，照 Blink 捨入、加每個 host 的雜訊、值變了發 `change`）：跨 `net`、browser 和 renderer 數百行：RFC 0082 量的時間已經有了，但估計本身（加權的中位數、隨時間衰減）要另外做；Speedometer 和 WPT 都看不出差別；留在「之後的可能」。

2026-10-24（使用者，「待決定」第 4 項）：

5. **`isInputPending()` 照 Chrome 給輸入標對象，滑鼠的由 compositor 照 paint 記下的區域判斷（S1c）**：
   - 規則照 Chrome 152 的探測和 cc 的 `FindFrameElementIdAtPoint`：
     - 滑鼠在子 frame 上算給那個 frame；
     - 被祖先 frame 的東西蓋住，或在圓角、`clip-path` 底下時是「不知道」；
     - 鍵盤算給有 focus 的 frame；
     - 照 `CanAccessEvent` 判斷誰看得到。
   - 約 400 到 500 行（含測試），跨 paint、protocol、compositor、renderer 和 `dom_api`，所以單獨成為第 2 段，`navigator.scheduling` 整個在這一段，第 1 段不先放一個只回 `false` 的版本。
   - 和 Chrome 只差在 layer 的範圍比畫出來的大的時候（第 2 段）。
   - 不選 S1a（整個 renderer 一個計數）：跨 origin 的 iframe 也看得到使用者在外面的操作，Chrome 特別擋掉這件事。
   - 不選 S1b（滑鼠一律算給最上層的 document）：滑鼠在跨 origin 的 iframe 上時那個 iframe 看不到、外面的頁面卻看得到，和 Chrome 相反。
   - 不選 S2（一律 `false`）：API 名不副實。

2026-10-24（使用者，「待決定」第 5 項）：

6. **`IntersectionObserver` v2 照 Blink 真的算 `isVisible`（V2）**：
   - 扭曲的效果照 `HasDistortingVisualEffects`；
   - 蓋住照 `HitTestForOcclusion`：paint 的 hit test 加一種範圍的查詢，規則照 Blink 的那組 `HitTestRequest`；
   - `delay` 照 `MaybeDelayAndReschedule` 節流。

   約 400 到 600 行（含測試），跨 paint 的 hit test 和 `IntersectionObserver` 的計算，單獨成為第 3 段，屬性和節流也移到這一段，第 1 段不先放一個 `isVisible` 一律 `false` 的版本。WPT 的 `intersection-observer/v2/` 38 份拿來驗證。不選 V1（只有屬性和節流、`isVisible` 一律 `false`）：規範允許，但 Chrome 會給 `true`，v2 的 WPT 大多照樣失敗。

2026-10-24（使用者，「待決定」第 7 項）：

7. **navigation entry 的網路時間由 browser 在 commit 時交給 renderer（T1）**：照 Chromium 的 `CommitNavigationParams` 和 `NavigationTiming`，交導覽的 `LoadTiming`、每一跳 redirect 的時間、`nextHopProtocol` 和狀態碼（第 6 段）。這些 RFC 0082 第 1 階段已經量好、在 `main` 上，只是還沒交給 renderer。
   - kun 量不到的照這樣給：
     - `secureConnectionStart`：HTTPS 時是 `connectStart`，HTTP 時是 0；
     - `requestStart`：連線結束，重用的連線是請求開始。
   - `serverTiming` 先是空的陣列。
   - 不選 T2（網路的欄位全是 0）：資料已經有了，沒有理由不用。
   - 草稿原本寫成「等 RFC 0082 的時間 merge」，那時沒有先確認它的進度：第 1 階段（1a 到 1c）在 2026-10-23 已經 merge。

2026-10-24（使用者，第 2 段開始前）：

8. **「iframe 被蓋住」照打得到的範圍（A）**：iframe 之後畫的、祖先 frame 的打得到的目標，它們的 hit test 的 rect（kun 的 `HitTestData`）蓋到的地方，對象是「不知道」。
   - Chrome 152 的實測（`details.md` 的「`isInputPending()` 的對象」，`probes/iip_top4.html`）：`pointer-events: none` 的不蓋住，沒有背景的空 div、`opacity: 0` 的蓋住，`visibility: hidden` 的不蓋住。這是 hit test 的規則，不是畫出來的範圍。
   - 唯一不同的是只有 `box-shadow` 伸到 iframe 上的情況：Chrome 照 layer 的範圍（元素和 shadow 的聯集）也放棄，kun 照元素的框。寫進 `docs/limitations.md`。
   - 不選 B（打得到的 paint chunk 的整個範圍，含 display item 畫出來的範圍）：在這 5 種情況都和 Chrome 一樣，但 kun 的 paint chunk 不等於 Chrome 的 layer，一個 chunk 可能橫跨很大一片，換一個頁面就會在 Chrome 不放棄的地方放棄。

2026-10-24（使用者，第 3 段開始前，都照建議）：

9. **蓋住的判斷走 fragment tree 的 hit test，位置換成矩形（H1）**：照 Blink 的 `HitTestForOcclusion`，它走的就是點擊的那條路（`PaintLayer::HitTest`）。`paint` 的 `Walker` 加一種矩形的位置和那組旗標：`pointer-events: none` 也算、opacity 0 不算、算畫到外面的範圍、target 是 stop node。被旋轉的元素照 quad 判斷。只有 `trackVisibility` 的 observer 付成本：每個 target 每次計算走一次樹，`delay` 至少 100 ms。不選 H2（草稿原本的寫法：`HitTestData` 多記節點、`pointer-events: none` 的盒子和畫到外面的範圍）：`HitTestData` 沒有節點的身分，要加上它，每一頁、每次 paint 都要多記、沿用 item 時也要搬，只為了很少用到的 API。
10. **「畫到盒子外面的範圍」從 paint 記下的 display item 取（O2）**：每個元素自己的 display item 的 `visual_rect` 的聯集，加上 filter 外擴的範圍，只在有 `trackVisibility` 時才掃 artifact。畫了什麼就算什麼，不在 hit test 裡重算 shadow 的幾何。不包括子孫的 overflow（Blink 的 ink overflow 包括）：子孫自己會被打到。不選 O1（hit test 裡照 style 算 shadow、outline、filter）：和 paint 的算法重複。不選 O3（只用 border box）：Chrome 152 實測 shadow、outline、text-shadow、filter 都算蓋住（`details.md`）。
11. **跨 site 的 iframe 也照實際的範圍判斷（P2）**：和 Chrome 對同 process 的 frame 一樣。Chrome 152 實測，跨 site 的 iframe（另一個 process）只要外面的東西蓋到它的任何一處，裡面的 target 都是 `false`；kun 的 iframe 都在同一個 renderer 裡，照實際的範圍，寫進 `docs/limitations.md`。不選 P1（把 host 不同當成跨 site、模仿整個 frame 一起判斷）：kun 沒有 public suffix list，host 的近似和 Chrome 的 site 不同，模仿的又是多 process 的結果。

2026-10-24（使用者，第 4 段開始前，都照建議）：

12. **`FileReader` 在 Chrome 和規範不同的兩處照規範（S1）**：`readAsText` 沒有 label 時照 blob 的 `type` 的 `charset` 解碼（Chrome 152 不理它，一律 UTF-8）；在 `load` 或 `abort` 的 handler 裡開始新的讀時，舊的那次不發 `loadend`（Chrome 152 照樣發）。兩處 WPT 都照規範測（`filereader_readAsText_blob_type_charset.any.js`、`FileReader-multiple-reads.any.js`），寫進 `docs/limitations.md`。不選 S2（照 Chrome）：要在 `.ini` 標成預期的失敗，Chrome 修了也要跟著改；依賴這兩個 bug 的頁面很少，Speedometer 沒有。
13. **沒有 `File.prototype.lastModifiedDate`（L1）**：Chrome 152 還有這個非標準的屬性，Firefox 已經拿掉，WPT 的 `historical.https.html` 要它不存在。不選 L2（照 Chrome 做）：binding 還沒有 `Date` 這種值，要在 `dom_js` 和 WIT 都加一種型別，只為一個過時的屬性。
14. **`File` 和 `Blob` 是 `dom` 的同一個 GC 型別（F1）**：`Blob` 多一個欄位記 `File` 的名稱和時間，interface 照它決定，和 `Event`、`EventTarget` 的子類別一樣。`Blob` 的方法直接用在 `File` 上，`slice()` 照樣回 `Blob`。不選 F2（獨立的 GC 型別，Blink 的 C++ 子類別）：binding 拆開 `Blob` 的地方都要多收一種型別。

2026-10-24（使用者，修第 4 段找到的 microtask checkpoint 時，選 1）：

15. **timer 的 callback 回來之後跑的 microtask 裡設的 timer 不算巢狀在那個 timer 裡（照 HTML）**：callback 之後的 checkpoint 前先結束那個 timer 的 nesting。HTML 的 nesting level 看 event loop 正在跑的 task，checkpoint 期間是 microtask；WPT 的 `timer-nesting-not-inherited-in-microtask.html` 照這樣測，kun 本來也是這樣。不選照 Chrome 152：它在第 6 層以上的 timer 的 microtask 裡設的 timer 也夾到 4 ms（實測 4.0 到 4.2 ms，`probes/nest2.html`），因為 Blink 的 `DOMTimer::Fired` 在 `MicrotasksScope` 跑完 microtask 之後才把層數歸零；照它要把 WPT 標成預期的失敗，Blink 修了也要跟著改。缺點是這種 timer 比 Chrome 早最多約 4 ms 跑，記在 `docs/limitations.md`。

2026-10-24（使用者，第 5 段開始前）：

16. **V8 先在本機編，kun 這一側做完再發 release（V1）**：fork 的 `MarkAsUndetectable` binding 先在本機 commit，`ci.py v8 --profile release` 編，用 `RUSTY_V8_ARCHIVE` 連結做完 kun 這一側和測試；binding 確定之後才 push fork、打 `kun-v152.2.0-20` 的 tag（push 前再確認），release 好了換 `RUSTY_V8_MIRROR_TAG`、用下載的 artifact 重跑測試再 merge。不選 V2（先發 release 再做）：要等幾個小時才能開始，binding 錯了要再發一輪。

2026-10-24（使用者，第 5 段實作時）：

17. **`document.all` 的同名元素照 Chrome，同一個名稱是同一個 `HTMLCollection`**：script 拿著時每次查到同一個 live 的集合（`dom` 照名稱的集合的 weak cache，和 `getElementsByTagName` 一樣），Chrome 152 實測 `all.a === all.a`（`probes/all.html`）。HTML 規定每次建新的，WPT 的 `htmlallcollection.html` 的「collections are new live HTMLCollection instances」照規範測，在 `.ini` 標成預期的失敗，寫進 `docs/limitations.md`。不選照規範（每次 `create_collection`）：同一個名稱在迴圈裡查時每次重新走樹。

2026-10-24（使用者，第 6 段開始前）：

18. **navigation timing 的時間點在 6b 一起寫進 trace（R1）**：照 Blink 的 `DocumentLoadTiming`、`DocumentTiming`，每個時間點在 `blink.user_timing` 寫一個 mark（`navigationStart`、`fetchStart`、`responseEnd`、`domInteractive`、`domContentLoadedEventStart` 等），走 `performance.mark()` 已經有的那條路。拿掉 `docs/limitations.md` 說沒有這些 instant 的那一句。不選 R2（留給之後）：6b 本來就在每個時間點記下時間，之後再回來要動同一批程式。

2026-10-24（使用者，6a 開始時）：

19. **`PerformanceObserver` 只給 JS（W1）**：`[JsOnly]`，`kun:dom` 的 world 不動，guest 照樣可以讀 `performance.getEntries*()`。不選 W2（也給 guest）：`kun:dom` 要升到 0.17、多一個每個 guest 都要有的 callback export，SDK、Lua 的 runtime 和 generator、demo 和測試的 guest 都要改，guest 也有自己的排程（和 `requestIdleCallback` 只給 JS 的理由一樣）。

2026-10-25（使用者，6b 開始前）：

20. **navigation entry 的 `confidence` 照 Chrome 的機制（C2）**：`randomizedTriggerRate` 固定 0.4994798（Chrome 152 實測），真正的值一律是 `high`（kun 沒有瀏覽器剛啟動這類的訊號），再以這個機率改成隨機的 `high` 或 `low`；每次導覽抽一次，同一份文件讀幾次都一樣（`probes/conf.html`）。不選 C1（固定 `high`、rate 0）：值確定、好測，但和 Chrome 的 rate 不同。

2026-10-25（使用者，收尾時）：

21. **盤點找到的 layout 的 bug 在 `0084` 上修**：merge 之前修，盤點的 20 個 suite 才都跑得完。不選 merge 之後另外在 `main` 上修：那樣的話，這份的驗收留著三個 debug build 跑不完的 suite。修法照 Blink：flex 和 grid container 寬度未知、空間確定時一律 fit-content（「盤點找到的：flex 和 grid container 的 shrink-to-fit 寬度」）。也不選讓 column 的快速路徑改用 taffy 的寬度：debug build 就不會失敗，但寬度和 Chrome 差得更遠。

## 待決定

沒有。

## 之後的可能

- `XMLHttpRequest`（盤點的 jQuery、Backbone）。
- Trusted Types（盤點的 Lit）。
- Paint Timing、LCP、Layout Instability、Event Timing：NewsSite-Next 的 web-vitals 和 Chrome 做一樣的事。
- 子資源的 Resource Timing（「不在範圍內」）。
- `Server-Timing` header 和 `PerformanceServerTiming`。
- Wake Lock 真的阻止螢幕變暗（W2）。
- `navigator.connection` 從 `net` 的時間估（N2）。
- Permissions API。
