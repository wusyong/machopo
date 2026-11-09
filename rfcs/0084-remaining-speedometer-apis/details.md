# RFC 0084 的細節：Speedometer 用到、kun 還沒有的 API

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點（2026-10-24）

`python3 scripts/bench.py speedometer --inventory --reuse-chrome`：

- kun `80cf9597`（dev）；
- Chrome for Testing 152.0.7977.82 的記錄是 2026-10-22 那次；
- Fedora 44，Ryzen AI 9 HX 370。

20 個 default suite 都是 `done`，沒有例外。

和 2026-10-22（kun `9357e4d9`）比只少了 `console.createTask`。CSS 的 at-rule、選擇器、宣告、元素都沒有缺。

suite 用到、kun 只解析沒有效果的值：

- `hyphens: auto`：6 個 Complex-DOM suite，Spectrum 的元件；
- `user-select: none`；
- CodeMirror 的 `-webkit-user-modify: read-write-plaintext-only`。

它們在 benchmark 跑的時候不改變畫面，不在這份裡。

## 每個 suite 怎麼用

從 `../third_party/speedometer/resources/` 的 build 找出來（2026-10-24）：

- **runner**（`benchmark-runner.mjs`）：`class WakeLock` 的 `request()` 先 `if (!navigator.wakeLock) return;`，再 `navigator.wakeLock.request("screen")`，跑完 `release()`。
- **jQuery**（TodoMVC-jQuery、TodoMVC-Backbone 的 `jquery.min.js`）：
  - 讀 style 的 `getStyles`：`t = e.ownerDocument.defaultView; return t && t.opener || (t = window), t.getComputedStyle(e)`。每次 `css()` 都讀 `opener`。kun 是 `undefined`、Chrome 是 `null`，走同一條路。
  - 載入時：`S.ajaxSettings.xhr = function(){ try { return new XMLHttpRequest } catch(e){} }; zt = S.ajaxSettings.xhr(); v.cors = !!zt && "withCredentials" in zt; v.ajax = !!zt`。之後沒有發請求。
- **React**（React-Redux、React-Stockcharts-SVG 的 bundle）：合成的 mouse event 的 `relatedTarget: function(e){ return e.relatedTarget || (e.fromElement === e.srcElement ? e.toElement : e.fromElement) }`。只在 `relatedTarget` 是空的時候讀。
- **Angular**（zone.js，`polyfills.*.js`）：
  - `Zone.__load_patch("IntersectionObserver", …)`、`Zone.__load_patch("FileReader", …)`：patch prototype 上的方法。
  - `Zone.__load_patch("geolocation", e => { e.navigator && e.navigator.geolocation && … })`。
  - `delay`、`trackVisibility` 是 patch 時列舉 prototype 讀到的。
- **Vue**（core-js，`chunk-vendors.*.js`）：`var t = "object" == typeof document && document.all, n = void 0 === t && void 0 !== t; e.exports = { all: t, IS_HTMLDDA: n }`。
- **Lit**（`lit/dist/index.js`）：
  - `h = globalThis.trustedTypes, c = h ? h.emptyScript : ""`：boolean attribute 用的空字串。
  - `y = globalThis.trustedTypes, _ = y ? y.createPolicy("lit-html", { createHTML: t => t }) : void 0`：每個 template 準備時經過 `_.createHTML`。
- **NewsSite-Next**：
  - `main-*.js` 是 web-vitals v3：`p = function(e, t, r){ try { if (PerformanceObserver.supportedEntryTypes.includes(e)) { var n = new PerformanceObserver(…); return n.observe(Object.assign({ type: e, buffered: !0 }, r || {})), n } } catch(e){} }`，觀察 `paint`、`layout-shift`、`first-input`、`largest-contentful-paint`、`event`。
  - TTFB 讀 `performance.getEntriesByType("navigation")[0]` 的 `responseStart`、`activationStart`。
  - `document.prerendering` 決定 `navigationType`，和等不等 `prerenderingchange`。
  - `framework-*.js`（React 的 scheduler）：`"undefined" != typeof navigator && void 0 !== navigator.scheduling && void 0 !== navigator.scheduling.isInputPending && navigator.scheduling.isInputPending.bind(navigator.scheduling)`。這個 build 只綁定、沒有用到。
- **NewsSite-Nuxt**（`entry.*.js`）：`function tf(){ const e = navigator.connection; return !!(e && (e.saveData || /2g/.test(e.effectiveType))) }`。是 `true` 時不 prefetch 連結。
- **Editor-CodeMirror**（`codemirror-*.js`）：`isInputPending = navigator.scheduling?.isInputPending ? () => navigator.scheduling.isInputPending() : null`。背景 parse（`ParseWorker`）在有輸入等著時停下來。

## Chrome 152 和 kun 的探測（2026-10-24）

探測頁面是 `probes/apis.html`（`run()` 回傳結果的 JSON），經過本機的 HTTP server 開：Chrome for Testing 152 `--headless=new`、新的 profile，用 CDP 的 `Runtime.evaluate` 呼叫；kun 是 `80cf9597`（dev），經過 automation pipe。

| 探測 | Chrome 152 | kun |
| --- | --- | --- |
| `window.opener`、`"opener" in window` | `null`、`true`；accessor（get、set、enumerable、configurable） | `undefined`、`false` |
| `opener = 5` 再讀；再設 `null` | `5`；`null`，`opener` 成為 window 自己的屬性 | `5`；`null`（一般的屬性） |
| 新的 iframe 的 window：設 `null`、`undefined`、`0`（`probes/opener.html`） | 每一次都成為自己的 data property：`null`、`undefined`、`0`。HTML 說 `null` 不定義屬性，Chrome 照樣定義 | 同上一列 |
| `opener` 的 setter：名稱、`length`、`this` 不是 window | `set opener`、1、`TypeError: Illegal invocation`；`Window.prototype` 上沒有 `opener` | 同上一列 |
| 沒有分派的 `new MouseEvent("mouseover", { relatedTarget: a })` 的 `fromElement`、`toElement`、`srcElement` | `a`、`null`、`null` | `undefined`、`undefined`、`null` |
| 在 `b` 上分派的 `mouseout`（`relatedTarget: a`） | `fromElement` `b`、`toElement` `a` | `undefined` |
| 在 `b` 上分派的 `mouseover`、`click` | `a`、`b` | `undefined` |
| `IntersectionObserver` 預設；`{ delay: 100, trackVisibility: true }`；`{ delay: 50 }` | `0, false`；`100, true`；`50, false` | 都是 `undefined` |
| `{ trackVisibility: true, delay: 99 }` | `NotSupportedError`：「Failed to construct 'IntersectionObserver': To enable the 'trackVisibility' option, you must also use a 'delay' option with a value of at least 100. Visibility is more expensive to compute than the basic intersection; enabling this option may negatively affect your page's performance. Please make sure you *really* need visibility tracking before enabling the 'trackVisibility' option.」 | 不丟 |
| `navigator.connection` | `NetworkInformation`，`4g`、`saveData` `false`、`rtt` 100、`downlink` 1.75；沒有 `type`、`downlinkMax`；prototype 是 `onchange`、`effectiveType`、`rtt`、`downlink`、`saveData` | `undefined` |
| `navigator.scheduling` | `Scheduling`，`isInputPending()` 是 `false` | `undefined` |
| `navigator.wakeLock` | `WakeLock`（`request`）；`WakeLockSentinel` 的 prototype：`onrelease`、`released`、`type`、`release` | `undefined` |
| `request("screen")` | 成功：`WakeLockSentinel`，`type` `screen`，`released` `false`；`release()` 回 `Promise`，之後 `released` 是 `true` | 沒有 |
| `request("system")` | `TypeError`：「The provided value 'system' is not a valid enum value of type WakeLockType.」 | 沒有 |
| `document.prerendering`、`onprerenderingchange` | `false`、`null` | `undefined` |
| `PerformanceObserver.supportedEntryTypes` | `element`、`event`、`first-input`、`interaction-contentful-paint`、`largest-contentful-paint`、`layout-shift`、`long-animation-frame`、`longtask`、`mark`、`measure`、`navigation`、`paint`、`resource`、`soft-navigation`、`visibility-state` | `ReferenceError` |
| `getEntries()` 的種類 | `navigation`、`visibility-state`、`resource` | 空的 |
| navigation entry | `PerformanceNavigationTiming`，`name` 是網址，`type` `navigate`，`activationStart` 0，`redirectCount` 0，`startTime` 0，`nextHopProtocol` `http/1.0`，`deliveryType` `""`，`workerStart` 0 | 沒有 |
| navigation entry 的 `toJSON()` 的鍵 | `name`、`entryType`、`startTime`、`duration`、`navigationId`、`initiatorType`、`deliveryType`、`nextHopProtocol`、`renderBlockingStatus`、`contentType`、`contentEncoding`、`workerStart`、`workerRouterEvaluationStart`、`workerCacheLookupStart`、`workerMatchedSourceType`、`workerFinalSourceType`、`redirectStart`、`redirectEnd`、`fetchStart`、`domainLookupStart`、`domainLookupEnd`、`connectStart`、`secureConnectionStart`、`connectEnd`、`requestStart`、`responseStart`、`firstInterimResponseStart`、`finalResponseHeadersStart`、`responseEnd`、`transferSize`、`encodedBodySize`、`decodedBodySize`、`responseStatus`、`serverTiming`、`unloadEventStart`、`unloadEventEnd`、`domInteractive`、`domContentLoadedEventStart`、`domContentLoadedEventEnd`、`domComplete`、`loadEventStart`、`loadEventEnd`、`type`、`redirectCount`、`activationStart`、`criticalCHRestart`、`notRestoredReasons`、`confidence` | 沒有 |
| `performance.navigation`、`performance.timing` | `type` 0、`redirectCount` 0；`navigationStart`、`loadEventEnd` 都大於 0 | 沒有 |
| `document.all` | `typeof` `undefined`，`== null` `true`，`=== undefined` `false`，`!!` `false`；`length` 6；`[0]` 是 `<html>`；`.a` 和 `("b")` 照 id 找到；`item(1)` 是 `<head>`；`namedItem("a")`；`[object HTMLAllCollection]` | `undefined` |
| `FileReader` | 有；`EMPTY` 0、`LOADING` 1、`DONE` 2；prototype：`readyState`、`result`、`error`、六個 handler、`abort`、`readAsArrayBuffer`、`readAsBinaryString`、`readAsDataURL`、`readAsText` | `ReferenceError` |
| `readAsText` 的事件 | 呼叫後同步 `readyState` 1；`loadstart`（1，0/8）、`progress`（1，8/8）、`load`（2，8/8）、`loadend`（2，8/8），`lengthComputable` `true` | 沒有 |
| `readAsDataURL(new Blob(["hi"], { type: "text/plain" }))` | `data:text/plain;base64,aGk=` | 沒有 |
| `readAsArrayBuffer`、`readAsBinaryString` | `ArrayBuffer` 2 bytes；UTF-8 的 `é` 是兩個 Latin-1 字元 | 沒有 |
| 讀到一半再 `readAsText` | `InvalidStateError`：「Failed to execute 'readAsText' on 'FileReader': The object is already busy reading Blobs.」 | 沒有 |
| 讀到一半 `abort()` | 同步發 `abort`（`readyState` 2）、`loadend`（2），`result` `null`，之後沒有 `loadstart` | 沒有 |
| `new File(["ab"], "x.txt", { type: "text/plain", lastModified: 5 })` | `x.txt`、2、`text/plain`、5、是 `Blob`、`webkitRelativePath` `""`、`[object File]` | `ReferenceError` |

第一次的探測頁面沒有 `<meta charset>`，所以 `readAsText` 的 `héllo` 讀回 `hÃ©llo`：script 的原始碼照 windows-1252 解碼，blob 存的是那兩個字元的 UTF-8。第二次加上了，表裡是第一次的事件序列，bytes 數是那次的 8。

還要探測的（實作時）：

- `readAsDataURL` 沒有 type 時；
- `confidence` 的值；
- `supportedEntryTypes` 的錯誤參數的訊息。

Geolocation 和 Wake Lock 在第 1 段探測過，結果在 `progress.md` 的 1d、1e。

## `navigator.connection` 的值

**Chrome 的探測**（2026-10-24，Chrome for Testing 152 headless，兩次啟動，每次依序開 `127.0.0.1`、`localhost`、`127.0.0.1` 的同一頁，各讀三次：解析時、1 秒、4 秒）：

| 啟動 | `127.0.0.1` | `localhost` | `127.0.0.1` 再一次 | `about:blank` |
| --- | --- | --- | --- | --- |
| 第一次 | `4g`、50、1.55 | `4g`、50、1.7 | `4g`、50、1.55 | `4g`、50、1.45 |
| 第二次 | `4g`、100、1.75 | `4g`、150、1.75 | `4g`、50、1.75 | `4g`、50、1.7 |

- 同一頁的三次讀數都一樣，`change` 沒有發過。
- `saveData` 都是 `false`。

**Chromium 的程式**（2026-10-24 從 GitHub 上的 Chromium 的 `main` 讀，和 152 可能有小差異）：

- `modules/netinfo/network_information.cc`：
  - `EffectiveConnectionTypeToEnum` 把 `kTypeUnknown`、`kTypeOffline` 都對應到 `4g`。
  - 沒有 listener 時，每次讀都問 `NetworkStateNotifier`；有 listener 時讀上次 `change` 時存下的值。
  - 捨入之後的 `effectiveType`、`rtt`、`downlink` 有一個不同就發 `change`。
- `platform/network/network_state_notifier.cc`：
  - `RoundRtt`：沒有估計時回 0；否則乘上 `GetRandomMultiplier(host)`，最多 3 秒，捨入到 50 ms。
  - `RoundMbps`：沒有估計時用最快的 10 Mbps（註解：「Throughput is unavailable. So, return the fastest value.」）；否則最多 10 Mbps，捨入到 50 kbps。
  - `GetRandomMultiplier`：`0.9 + ((hash(host) + salt) % 21) * 0.01`，salt 是每次啟動 1 到 20 的亂數，所以同一個 host 在一次執行裡固定、換一次啟動就變。

所以探測裡的值是 browser 的 `NetworkQualityEstimator` 量出來的估計，加上每個 host 的雜訊。kun 選的是「沒有估計」那一支：`4g`、0、10。

## `isInputPending()` 的對象

**Chrome 的探測**（2026-10-24，Chrome for Testing 152 headless，`probes/iip_*`）：

- 做法：
  - 最上層的頁面在 `127.0.0.1:A`，裡面有三個 300×200 的 iframe：
    - 同 origin（A）；
    - 跨 origin、同 site（`127.0.0.1:B`，和外面同一個 process）；
    - 跨 site（`localhost:B`，另一個 process）。
  - 讓 main thread 在其中一個 frame 裡跑 1.2 秒的迴圈，一直問 `isInputPending()` 和 `isInputPending({ includeContinuous: true })`。
  - 迴圈開始後 0.4 秒，用 CDP 的 `Input.dispatchMouseEvent` 在某一點送 `mousePressed` 加 `mouseReleased`，或 `mouseMoved`；送了不等回答。
- 結果（「看得到」是迴圈裡的 `isInputPending()` 變成 `true`）：

| 點在哪裡 | 忙著的 frame：最上層 | 同 origin 的 iframe | 跨 origin 的 iframe | 跨 site 的 iframe |
| --- | --- | --- | --- | --- |
| 同 origin 的 iframe 上 | 看得到 | 看得到 | 看不到 | 看不到 |
| 跨 origin 的 iframe 上 | 看不到 | 看不到 | 看得到 | 看不到 |
| 跨 site 的 iframe 上 | 看不到 | 看不到 | 看不到 | 看得到（自己的 process） |
| 外面的頁面上 | 看得到 | 看得到 | 看不到 | 看不到 |

- 換成 `mouseMoved` 時，只有 `includeContinuous: true` 看得到，對象和上表一樣。
- 第二組，iframe 的內容不能捲動（`probes/iip_top2.html`）：
  - 跨 origin 的 iframe 上：照樣只有它自己看得到。
  - 外面頁面的一個沒有 layer 的半透明方塊蓋在跨 origin 的 iframe 上，點在方塊上：**誰都看不到**。
  - 一個 `will-change: transform` 的方塊蓋在同 origin 的 iframe 上：最上層和那個 iframe 看得到。兩個本來就互相存取得到，分不出算給誰。
- 第三組（`probes/iip_top3.html`）：
  - iframe 外面包著 `clip-path: inset(0 round 20px)`，點在正中間：**誰都看不到**。
  - 兩個 10×10 的方塊蓋在 iframe 的兩角，點在中間：iframe 看得到。
  - iframe 外面包著 `opacity: .5`：iframe 看得到。

**Chromium 的程式**（2026-10-24 從 GitHub 上的 Chromium 的 `main` 讀）：

- `core/frame/scheduling.cc`：`Scheduling::isInputPending` 向 `MainThreadScheduler::GetPendingUserInputInfo(includeContinuous)` 要每個還在排隊的事件的對象，有一個 `LocalFrame::CanAccessEvent` 是 `true` 就回 `true`。
- `platform/scheduler/main_thread/pending_user_input.cc`：
  - `Monitor::OnEnqueue` 和 `OnDequeue` 照對象分組計數；
  - 沒有對象（`kUnknown`）的不記；
  - `IsContinuousEventType`：`kMouseMove`、`kMouseWheel`、`kTouchMove`、`kPointerMove`、`kPointerRawUpdate`。
- `platform/scheduler/main_thread/main_thread_scheduler_impl.cc`：
  - `WillPostInputEventToMainThread`（compositor thread 交給 main thread 時）呼叫 `OnEnqueue`；
  - `WillHandleInputEventOnMainThread`（main thread 開始處理時）呼叫 `OnDequeue`；
  - 兩個 thread 之間用 `any_thread_lock_`。
- `platform/widget/input/input_handler_proxy.cc` 的 `PerformEventAttribution`：
  - 鍵盤：`kFocusedFrame`；
  - 滑鼠、滾輪、gesture、touch（第一個觸點）：`kTargetedFrame`，加上 `FindFrameElementIdAtPoint` 找到的 frame；
  - 其他：`kUnknown`。
- `cc/trees/layer_tree_impl.cc` 的 `FindFrameElementIdAtPoint`：
  - 從最前面往後對每個 hit testable 的 layer 做 hit test，最前面打到的是候選；
  - 從候選的 transform node 往上找第一個有 `visible_frame_element_id` 的 node，就是它的 frame；
  - 打到的 layer 裡有屬於別的 frame、而且不在候選的 frame 往上的那條路上的，放棄；
  - 候選的 effect 有 mask（`HitTestMayBeAffectedByMask`）時也放棄。
- `core/paint/paint_property_tree_builder.cc`：每個 frame 的 `LayoutView` 的 paint offset translation 標上 `visible_frame_element_id`（那個 document 的 DOM node id；frame 對 hit test 看不見時是空的）。
- `core/frame/local_frame.cc` 的 `CanAccessEvent`：
  - `kTargetedFrame`：有 pointer lock 時是被鎖的元素的 document，否則是 id 對應的 document；問的 frame 的 origin 能存取它就是 `true`。
  - `kFocusedFrame`：問的 frame 就是 focus controller 現在的 focused frame。
  - `kUnknown`：`false`。

- 第四組，什麼算「蓋住」（`probes/iip_top4.html`，各種 div 蓋在跨 origin 的 iframe 中間，忙著的是 iframe）：

  | 蓋在上面的 | iframe 看得到 |
  | --- | --- |
  | 紅色、`pointer-events: none` | 看得到 |
  | 沒有背景的空 div | 看不到 |
  | 紅色、`visibility: hidden` | 看得到 |
  | 紅色、`opacity: 0` | 看不到 |
  | 元素在 iframe 外面，只有 `box-shadow` 伸過來，點也不在 shadow 上 | 看不到 |

  前四種是 hit test 的規則（打得到的才算），最後一種是 layer 的範圍：元素和它的 shadow 在同一個 layer，範圍是兩者的聯集。
- 第五組，mask（`probes/iip_top5.html`）：
  - iframe 自己有 `border-radius`：iframe 看不到；
  - iframe 包在圓角的 `overflow: hidden` 裡：iframe 看不到；
  - 最上層的頁面自己的內容在圓角的 `overflow: hidden` 或圓角的 `clip-path` 裡，忙著的是最上層：看得到。

探測裡兩種「誰都看不到」，正是 cc 放棄、對象成為空的 id 的兩種情況：

- 方塊蓋在 iframe 上：最前面的候選是外面頁面的方塊，打到的 iframe 不在方塊的 frame 往上的那條路上；
- 圓角的 `clip-path`：讓候選的 effect 有 mask。

## `IntersectionObserver` v2 的可見

**Chromium 的程式**（2026-10-24 從 GitHub 上的 Chromium 的 `main` 讀，都在 `third_party/blink/renderer/core/`）：

- `intersection_observer/intersection_observer.cc`：
  - `trackVisibility` 而 `delay` 小於 100 ms 時丟 `NotSupportedError`；
  - `GetEffectiveDelay()` 是 `delay`（測試可以關掉）。
- `intersection_observer/intersection_observation.cc`：
  - `MaybeDelayAndReschedule`：離上次計算不到有效的 `delay` 時不算，把剩下的時間交給 `ComputeIntersectionsContext::UpdateNextRunDelay`，之後再排一次；
  - `ShouldCompute`：追蹤可見、上次不是 visible、frame 的 occlusion state 還不知道時先不算；
  - `isVisible` 改變也算一次「改變」，會發通知。
- `intersection_observer/intersection_geometry.cc` 的 `ComputeVisibilityInfo`：
  - local frame root 的 occlusion state 不是 `kGuaranteedNotOccluded` 時是 `false`；
  - `HasDistortingVisualEffects()` 時是 `false`；
  - 否則對 target 的框做 `HitTestForOcclusion`，打到的 inner node 是空的或是 target 自己時是 `true`；inline 的 target，inner node 是它的子孫也是 `true`；其他是 `false`。
  - 只在相交、而且要算可見時才算。
- `layout/layout_object.cc`：
  - `HasDistortingVisualEffects`：
    - 還沒 paint 過，沒有 local border box properties，算成有；
    - 祖先的 effect 有 `HasRealEffects()`（opacity、filter、blend）；
    - transform 和 local frame root 的不在同一個平面上；
    - 換算到 local frame root 的投影不是 `Is2dProportionalUpscaleAndOr2dTranslation`。
  - `HitTestForOcclusion`：旗標是 `kIgnorePointerEventsNone | kReadOnly | kIgnoreClipping | kIgnoreZeroOpacityObjects | kHitTestVisualOverflow`，用範圍的 `HitTestLocation`，stop node 是 target 自己。有 `hit_node_cb` 時才是 list-based，`IntersectionObserver` 的一般路徑不是。

**Chrome 的探測**（2026-10-24，Chrome for Testing 152 headless，`probes/iov_*`，`iov_probe.py`）：每個 target 40×40，一個 `{ trackVisibility: true, delay: 100 }` 的 observer，800 ms 後讀最後的 `isVisible`。

- 什麼算蓋住（`probes/iov_over.html`，蓋住的東西排在 target 之後）：

  | 蓋在上面的 | `isVisible` |
  | --- | --- |
  | 沒有 | `true` |
  | 外側的 `box-shadow`（偏移或 spread）伸到 target 上 | `false` |
  | `outline`、`outline-offset` 的 outline 伸到 target 上 | `false` |
  | `text-shadow` 伸到 target 上 | `false` |
  | `filter: blur()`、`drop-shadow()` 伸到 target 上 | `false` |
  | 旋轉 45° 的方塊，只有外框碰到 target | `true` |
  | 旋轉 45° 的方塊，真的重疊 | `false` |
  | 被 `overflow: hidden` 的盒子裁掉的部分在 target 上 | `true` |
  | 被 `clip-path` 裁掉的部分在 target 上 | `true` |
  | 被捲動容器裁掉的部分在 target 上 | `true` |
  | `visibility: hidden` | `true` |
  | `opacity: 0` | `true` |
  | `pointer-events: none` | `false` |
  | 沒有背景的空 div | `false` |

- iframe（`probes/iov_top.html`、`iov_child.html`，target 在 iframe 裡，外面的頁面有一個半透明的方塊）：

  | iframe | 方塊在 target 上 | 方塊在 iframe 的別處 | 沒有方塊 |
  | --- | --- | --- | --- |
  | 同 origin（同 process） | `false` | `true` | `true` |
  | 跨 site（`localhost` 對 `127.0.0.1`，另一個 process） | `false` | `false` | `true` |

  同 process 的照實際的範圍；跨 site 的整個 frame 一起判斷，是 browser 端算的 occlusion（local frame root 的 occlusion state 不是 `kGuaranteedNotOccluded`）。

- Blink 的 `kIgnoreClipping`（`HitTestRequest`）：`LayoutView::HitTest` 把 hit test 的範圍從 frame 的大小擴大到整份 document；`PaintLayer::HitTestLayer` 不照 layer 的 overflow clip。實測被 `overflow: hidden` 裁掉的照樣不算蓋住。

## `File` 和 `FileReader`

第 4 段開始前和實作時的探測，Chrome for Testing 152 headless，2026-10-24，`probes/fr*.html`：

- `readAsDataURL`：沒有 type 時 `data:application/octet-stream;base64,…`，空的 blob 是 `…;base64,`；type 小寫（`Text/Plain;Charset=UTF-8` 讀成 `text/plain;charset=utf-8`）。
- `new File(["ab"], "a/b:c.txt")` 的名稱照原樣；`lastModified` 預設接近 `Date.now()`、是整數；1.7 讀成 1、-1.5 讀成 -1、`NaN` 是 0、1e20 是 9211727563254776（Blink 存成 `base::Time` 時飽和）。
- 少於兩個參數：`TypeError`「Failed to construct 'File': 2 arguments required, but only 1 present.」；`readAsText("x")`：「Failed to execute 'readAsText' on 'FileReader': parameter 1 is not of type 'Blob'.」。
- `File.prototype`：`name|lastModified|lastModifiedDate|webkitRelativePath|constructor`；`lastModifiedDate` 每次是新的 `Date`。`FileReader.prototype`：`readyState|result|error|onloadstart|onprogress|onload|onabort|onerror|onloadend|EMPTY|LOADING|DONE|abort|readAsArrayBuffer|readAsBinaryString|readAsDataURL|readAsText|constructor`。
- 20MB 的 `readAsArrayBuffer`：`loadstart` 0/20971520，一個 `progress`，`load`、`loadend`。
- 解碼：
  - BOM 優先（UTF-8 和 UTF-16 的 BOM，也勝過 label）；
  - label 有效時照它（`windows-1252`、`latin1`、` UTF-16LE `、`shift_jis`）；
  - 無效的 label 和沒有 label 時是 UTF-8，**blob 的 `type` 的 `charset` 不理**（`shift_jis` 的 type 讀成兩個 U+FFFD），和規範、WPT 不同；
  - label `replacement` 讀成一個 U+FFFD。
- `readAsBinaryString`：每個 byte 一個 Latin-1 字元。
- 同一個 reader 讀完再讀可以；讀到一半 `abort()` 同步發 `abort`、`loadend`（`readyState` 2），`result` 是 `null`，之後沒有事件。在 `onload` 裡開始新的讀：舊的照樣發 `loadend`（`readyState` 1、`result` 是 `null`），和規範不同。
- 什麼都沒在讀時 `abort()` 什麼都不發。
- 前後有空白的 label（`' UTF-16LE '`）不認，讀成 UTF-8（Encoding 的「get an encoding」會先去掉空白）。
- `ProgressEvent`（`probes/fr5.html`、`fr6.html`）：`loaded`、`total` 是 double（1.5、-2 照收），`NaN` 是 `TypeError`；空的 blob 沒有 `progress`；`abort` 和它之後的 `loadend` 是 0/0、`lengthComputable` `false`；讀完之後的 `abort()` 留著 `result`（File API 是 `null`），什麼都不發。
- `result` 的 `ArrayBuffer` 每次讀回同一個，script 改了它也看得到（`probes/fr4.html`）。
- `load` 的 handler 排的 microtask 在 `loadend` 之前跑（`probes/fr7.html`）；`MessagePort` 的兩個 `message` listener 之間也一樣（`probes/micro.html`）：listener 回來時的 microtask checkpoint。
- 先後（`probes/fr3.html`，用 `MessageChannel` 數 task）：`loadstart` 在第 0 或第 1 個 task，`Blob.text()` 的結果在第 1 個，`progress`、`load`、`loadend` 在第 1 或第 2 個，每次不同：bytes 在 browser 的 process，讀要來回一趟。

## `document.all`

Chrome 152（headless）的探測（`probes/all.html`、`all2.html`、`all_test.html`），第 5 段照這些做：

- **[[IsHTMLDDA]]**：`typeof` 是 `"undefined"`，`== null`、`== undefined` 是 `true`，`=== undefined`、`=== null` 是 `false`，`!document.all`、`Boolean()` 是 `false`。`??` 和 `?.` 照樣拿到物件（不是 null 或 undefined）。`String()` 是 `[object HTMLAllCollection]`，`JSON.stringify` 略過它。
- **物件**：每次讀 `document.all` 是同一個物件。它不是 `HTMLCollection`（`instanceof` 是 `false`），prototype 只有 `length`、`item`、`namedItem`、`constructor`，`@@iterator` 是 `Array.prototype.values`。`all` 在 `Document.prototype` 上，每種 document 都有：XML 的、`new Document()` 的（0 個元素，`typeof` 照樣是 `"undefined"`）。`new HTMLAllCollection()` 是 `TypeError`。
- **index**：
  - `all[0]`、`all(0)`、`all("0")`、`item(0)` 都是 `<html>`。
  - 是 array index 的字串照 index 取：`item("5")` 是第 6 個元素，不是 `id=5` 的。
  - `item()`、`all()`、`all(null)`、`all(undefined)`、`item(-1)`、`item(1e10)`、`item(4294967295)` 都是 `null`：null 轉成字串 `"null"`，照名稱找不到。
- **名稱**：
  - 任何元素的 `id`（SVG 的也算），加上 HTML 的 `a`、`button`、`embed`、`form`、`frame`、`frameset`、`iframe`、`img`、`input`、`map`、`meta`、`object`、`select`、`textarea` 的 `name`。`<div name>` 不算。
  - 好幾個時是一個 live 的 `HTMLCollection`，同一個名稱每次是同一個物件。
  - 找不到的：`namedItem` 和 `item` 是 `null`，`all.x` 是 `undefined`。`namedItem()` 沒有參數是 `TypeError`。
  - 空字串也是名稱（`probes/all3.html`）：有 `name=""` 或 `id=""` 的元素時查得到它們，只有一個時是那個元素。HTML 規定空字串回 `null`，WPT 只在沒有這種元素時測，Chrome 照樣通過；`HTMLCollection.namedItem("")` 和 `[""]` 才照規範是 `null`、`undefined`。kun 照 Chrome。
- **呼叫**：
  - `Function.prototype.call.call(all, null, 0)` 也找得到（`this` 不是它也一樣），`new all(0)` 是 `TypeError`（不是 constructor）。
  - `item.length` 是 0，`namedItem.length` 是 1。
- **列舉**：`Object.keys` 只有 index。`getOwnPropertyNames` 也有名稱（照 tree order，每個元素先 `id` 再 `name`），名稱不可列舉；kun 的 `HTMLCollection` 本來就不把名稱列進 own keys（沒有 `[SupportedNames]`），`HTMLAllCollection` 照同一套。

## `PerformanceObserver` 和 Navigation Timing

Chrome 152（headless）的探測，第 6 段照這些做：

- **`PerformanceObserver`**（`probes/po.html`、`po2.html`、`po_*.html`、`console_probe.py`）：
  - `observe()` 沒有 `entryTypes` 也沒有 `type`、兩個都有：`TypeError`。
  - 換方式：`InvalidModificationError`，兩個方向的訊息不同（「This observer has performed observe({entryTypes:...}…」、「This PerformanceObserver has performed observe({type:...}…」）。`disconnect()` 之後方式照舊。
  - 不支援的種類：console 的 warning「The entry type 'X' does not exist or isn't supported.」。`entryTypes` 帶 `buffered`：「The PerformanceObserver does not support buffered flag with the entryTypes argument.」。
  - 一個 task 送一批，在 microtask 之後；callback 的 `this` 是 observer，三個參數；`observe()` 之後第一次的第三個參數是 `{droppedEntriesCount: 0}`，之後是 `{}`。
  - entry list 照 `startTime` 排，`getEntries()` 每次是新的陣列。`takeRecords()` 照放進的順序；`buffered` 拿到的照 `startTime` 排。
  - callback 的先後照 observer 在上一次送出之後第一次拿到 entry 的順序（Blink 的 `active_observers_`）。
  - `supportedEntryTypes` 凍結、每次同一個（Chrome 列出 15 種）。
- **navigation entry**（`probes/nav.html`、`nav2.html`、`nav3.html`、`nav4.html`）：
  - 從文件一開始就在 buffer 裡，每次讀是同一個物件。還沒發生的時間是 0，`duration` 是 `loadEventEnd`。
  - `domInteractive`、`domComplete` 在 `readystatechange` 之前填上；`domContentLoadedEventStart` 在 `DOMContentLoaded` 之前、`End` 在之後；`loadEventEnd` 在 `load` 的 listener 和它的 microtask 裡都還是 0，下一個 task 才有。
  - observer 在 `load` 之後的 task 拿到 entry；之後 `buffered` 的也拿得到。
  - `transferSize` 是 `encodedBodySize` 加 300；`renderBlockingStatus` 是 `non-blocking`，`initiatorType` 是 `navigation`，`deliveryType` 和 service worker 的欄位是空字串或 0，`notRestoredReasons` 是 `null`。
  - `serverTiming` 凍結，每次是新的陣列；`confidence` 每次是新的物件。
  - `PerformanceNavigation` 的常數在 attribute 之後。
- **`confidence`**（`probes/conf.html`）：`randomizedTriggerRate` 固定是 0.4994798；`value` 每次導覽隨機，同一份文件裡固定。
- **`measure()` 的 PerformanceTiming 名稱**（`probes/nav3.html`）：照 `performance.timing` 的整數毫秒減去 `navigationStart`（`domInteractive` 的 measure 是 114.0，entry 的是 114.2）。還沒發生的是 `InvalidAccessError`「'loadEventEnd' is empty: either the event hasn't happened yet, or it would provide cross-origin timing information.」；`mark('fetchStart')` 是 `SyntaxError`「'fetchStart' is part of the PerformanceTiming interface, and cannot be used as a mark name.」。
- **redirect**（`probes/tao_probe.py`）：
  - 每一跳都和文件同 origin，就算。有一跳跨 origin 時，要每一跳（同 origin 的也算）的 `Timing-Allow-Origin` 都是 `*` 或文件的 origin 才算。
  - A→B（TAO `*`）→A 是 0，B（TAO `*`）→A 是 1，主 frame 和 iframe 都一樣。
  - WPT 的新舊兩份（`redirect-tao.html`、`nav2-test-redirect-chain-xserver-final-original-origin.html`）也都符合這條規則。
- **trace**（`probes/trace_probe.py`）：`blink.user_timing` 的 mark，依序是：
  - `navigationStart`、`fetchStart`、`responseEnd`；
  - `unloadEventStart`、`End`；
  - `commitNavigationEnd`、`domLoading`、`domInteractive`；
  - `domContentLoadedEventStart`、`End`、`domComplete`、`loadEventStart`、`End`。

  每個都帶著 frame。`dom*` 的 category 是 `blink.user_timing,rail`。

## Chromium 的對照

都在 `third_party/blink/renderer/`。路徑和名稱是憑記憶寫的（這台機器沒有 Chromium 的原始碼），實作時對照 Chromium 152 的原始碼確認：

| API | Blink |
| --- | --- |
| `window.opener` | `core/frame/dom_window.cc` 的 `opener()`；setter 是 `bindings/core/v8/custom/v8_window_custom.cc` 的 `OpenerAttributeSetterCustom`：不是 `null` 時在 window 上定義自己的屬性 |
| `fromElement`、`toElement` | `core/events/mouse_event.cc`：`mouseout`、`mouseleave` 時 `toElement` 是 `relatedTarget`，其他是 target；`fromElement` 反過來 |
| `IntersectionObserver` v2 | `core/intersection_observer/`，見「`IntersectionObserver` v2 的可見」 |
| `navigator.connection` | `modules/netinfo/network_information.cc`，值來自 `platform/network/network_state_notifier.cc`，照 Chrome 的規則捨入 `rtt`、`downlink` |
| `isInputPending` | `core/frame/scheduling.cc`（`Scheduling`），問 `platform/scheduler/main_thread/pending_user_input.h` 的 `PendingUserInput::Monitor`；見「`isInputPending()` 的對象」 |
| Geolocation | `modules/geolocation/`（`Geolocation`、`GeoNotifier`），browser 那一側是 `GeolocationService` |
| Wake Lock | `modules/wake_lock/`（`WakeLock`、`WakeLockManager`、`WakeLockSentinel`），browser 那一側是 `device::mojom::WakeLock`，最後是 `services/device/wake_lock/power_save_blocker/` |
| `document.prerendering` | `core/dom/document.cc` 的 `prerendering()` |
| `File`、`FileReader` | `core/fileapi/file.cc`、`file_reader.cc`、`file_reader_loader.cc`；事件的 task 是 `TaskType::kFileReading` |
| `HTMLAllCollection` | `core/html/html_all_collection.cc`；產生的 binding 在 instance template 上 `MarkAsUndetectable()` 和 `SetCallAsFunctionHandler()` |
| `PerformanceObserver` | `core/timing/performance_observer.cc`，`performance.cc` 的 `NotifyObserversOfEntry` 和 `deliver_observations_timer_`（`TaskType::kPerformanceTimeline`） |
| Navigation Timing | `core/timing/performance_navigation_timing.cc`、`performance_timing.cc`、`performance_navigation.cc`，時間在 `core/loader/document_load_timing.cc`；網路的時間由 browser 在 `CommitNavigationParams` 帶過來 |

## rusty_v8

fork（`kun` 分支）沒有 `MarkAsUndetectable`、`IsUndetectable` 的 binding（`src/*.rs`、`src/binding.cc` 都沒有）。V8 有：

- `v8/include/v8-template.h` 的 `ObjectTemplate::MarkAsUndetectable()`；
- `v8/include/v8-object.h` 的 `Object::IsUndetectable()`。

`ObjectTemplate::set_call_as_function_handler` fork 已經接出來了（`src/template.rs`）。

新的 binding 照 `README.machopo.md` 的「Local modifications」的格式記。

## kun 原本的做法（開始實作前）

- **`Navigator`**：`crates/dom_api/src/interfaces/core_frame.rs`。說明裡寫著沒有其他規範的成員（`wakeLock`、`clipboard`）。
- **`opener`**：DOM 沒有。`shell` 的 `browser_view.rs` 只記哪個分頁開了哪個分頁。
- **`MouseEvent`**：`core_events.rs`。`relatedTarget` 是 `dom` 的 `Event` 上的一條 GC 邊（`related_target`）。
- **`IntersectionObserver`**：`core_intersection_observer.rs`，說明寫著沒有 v2。通知是一個 task（`Task::NotifyIntersectionObservers`、`TaskSource::IntersectionObserver`）。
- **`Performance`**：`core_timing.rs`。entry 只有 `Mark`、`Measure`，存在 `dom` 的 `PerformanceData` 裡，是一條從新到舊的鏈。`add_performance_entry` 是加 entry 唯一的入口，observer 要接在這裡。
- **`Blob`**：`core_fileapi.rs`、`blob.rs`。bytes 是 `Arc<[u8]>`，`text()` 這些經過 `body::read_in_task`，放在 `TaskSource::Networking`。
- **集合**：`dom` 的 `CollectionFilter`（`TagName`、`DocumentForms`……）、`dom_api` 的 `collection.rs`。`HTMLCollection` 還沒有 supported property names。
- **`TaskSource`**：`Networking`、`Timer`、`DomManipulation`、`PostedMessage`、`CanvasBlobSerialization`、`IntersectionObserver`、`UserInteraction`。沒有 file reading 和 performance timeline。
- **IDL 檢查**（`crates/dom_api/tests/it/spec.rs`）：
  - 有 hr-time、performance-timeline、user-timing、FileAPI、intersection-observer、uievents 的 IDL。
  - 沒有 navigation-timing、resource-timing、netinfo、screen-wake-lock、is-input-pending、geolocation。
  - `spec-missing.txt` 記著 `Document.all`、`IntersectionObserver.delay` / `trackVisibility`、`Window.opener`。
- **導覽的時間**：
  - RFC 0082 第 1 階段之後，`net` 每個請求有 `protocol::network::LoadTiming`（請求開始、DNS、連線、收到 header、收完，`ipc::TimeTicks`），redirect 每一跳帶著自己的回應。
  - `RendererMsg::CommitNavigation` 只帶 `since_navigation_start`（time origin 從這裡來）和 `NavigationLoad`（header、body 的長度和 `response_end`）。導覽的完整時間在 browser 手上，沒有交給 renderer。
- **`document.visibilityState`**：從 `frame.is_page_visible()` 來，`visibility_state_changed` 發 `visibilitychange`。Wake Lock 的 release 接在這裡。
- **沒有 Permissions API、`TextDecoder`**。`encoding_rs` 在 `renderer`。

## 相關的 WPT

2026-10-24 在 `../third_party/wpt`，`.html`、`.any.js`、`.window.js`，不含 `resources/`、`support/`：

| 目錄 | 份數 | 在清單裡 |
| --- | ---: | --- |
| `FileAPI/reading-data-section/` | 16 | 否 |
| `FileAPI/file/` | 15 | 否 |
| `FileAPI/FileReader/` | 5 | 否 |
| `html/infrastructure/common-dom-interfaces/collections/` | 6 | 否 |
| `performance-timeline/` | 51 | 否 |
| `navigation-timing/` | 59 | 否 |
| `intersection-observer/v2/` | 38 | 整個 `intersection-observer/` 在清單裡 |
| `netinfo/` | 2 | 否 |
| `screen-wake-lock/` | 16 | 否 |
| `is-input-pending/` | 12 | 否 |

`html/browsers/windows/` 有幾份測 `opener`，大多要 `window.open()`。

## 參考資料

- File API：https://w3c.github.io/FileAPI/
- HTML 的 `HTMLAllCollection`：https://html.spec.whatwg.org/multipage/common-dom-interfaces.html#the-htmlallcollection-interface
- HTML 的 `opener`：https://html.spec.whatwg.org/multipage/nav-history-apis.html#dom-opener
- Performance Timeline：https://w3c.github.io/performance-timeline/
- Navigation Timing 2：https://w3c.github.io/navigation-timing/
- Intersection Observer v2：https://w3c.github.io/IntersectionObserver/v2/
- Network Information：https://wicg.github.io/netinfo/
- Screen Wake Lock：https://w3c.github.io/screen-wake-lock/
- `isInputPending`：https://wicg.github.io/is-input-pending/
- Geolocation：https://w3c.github.io/geolocation/
- UI Events 的 `fromElement`、`toElement`：Blink 的 `mouse_event.cc`（沒有寫進規範）
