# RFC 0048：transition 和動畫

- 開始日期：2026-10-15
- 狀態：✅ 已完成：2026-10-16（第 1 到 18 條 2026-10-15 接受，第 19 到 23 條 2026-10-16；三段都做完，第 4 段移到 RFC 0049，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`；進度：`progress.md`

## 摘要

RFC 0012 路線圖的 S6（CSS 線）：CSS Transitions（`transition-*`）和 CSS Animations（`@keyframes`、`animation-*`），連同它們的事件。照 Blink 把兩者建在 Web Animations 的模型上（document timeline、`Animation`、`KeyframeEffect`），動畫的值在 cascade 裡當成自己的 origin 套上；全部在 main thread 上跑（每個 frame 重算動畫的元素的 style，照 `StyleDifference` 走 layout 或只 paint）。Web Animations 的 JS API（`getAnimations()`、`element.animate()`、`document.timeline`、`Animation` 的控制）是第 3 段。transform 和 opacity 交給 compositor 的動畫原本是第 4 段，移到 RFC 0049（第 23 條）。

## 動機

- **Speedometer**：盤點裡 14 個 default suite 有 `transition-*`、8 個有 `@keyframes`（`rfcs/0012-speedometer/README.md` 的「盤點的結果」、`rfcs/0012-speedometer/details.md` 的「對照表」）。kun 現在丟掉這些宣告，hover、focus、勾選造成的變化一步跳到終點，Chrome 會在之後的幾個 frame 裡重算 style 和重畫：async 的時間要「畫面對了」才有意義（路線圖的「量得對」）。實際在跑的動畫是兩個 Editor 的游標閃爍和 Perf-Dashboard 載入時的 spinner（都是 `infinite`），見 `details.md` 的「Speedometer」。
- **一般網頁**：transition 和 `@keyframes` 是最常見的 CSS 之一（選單、按鈕的 hover、載入的 spinner）；`transitionend`、`animationend` 從來不發，等它們才繼續的頁面會卡住（只靠 `setTimeout` 保底的才不會）。
- **WPT**：`css/css-transitions/`（258 個檔案）、`css/css-animations/`（306 個）、`css/css-easing/`（17 個）都沒有加進清單；各個 property 的 `*-interpolation.html`（340 個檔案用 `interpolation-testcommon.js`）要 Web Animations 的 JS API。

要解決的 `docs/limitations.md` 的限制：

- 「有些事件 kun 從來不分派（`scrollend`、CSS 動畫和 transition 的）」的 CSS 動畫和 transition 的部分（「事件」的 Event handler 那一條）。

原本也列了「transform 和 opacity 本身也不成為 layer」的動畫的部分（「Compositor、layer 和 input」），第 4 段移走之後由 RFC 0049 解決。

**目標**：`transition-property` / `duration` / `timing-function` / `delay` / `behavior` 和 `transition` shorthand；`@keyframes`（`from`、`to`、百分比，keyframe 裡的 `animation-timing-function`，`!important` 的宣告不算）和 `animation-name` / `duration` / `timing-function` / `delay` / `iteration-count` / `direction` / `fill-mode` / `play-state` 和 `animation` shorthand；easing 的 `linear`、`ease*`、`cubic-bezier()`、`steps()`、`linear()`；kun 有的 property 都有 animation type（能插值的照 computed value 插值，其他 discrete）；`TransitionEvent`、`AnimationEvent` 和它們的事件；`@keyframes` 照 tree scope 找（shadow tree）；Web Animations 的 JS API（web-animations 1 的主體，第 13 條）和 `@keyframes` 的 CSSOM。

**不在範圍內**：scroll-driven animation（`animation-timeline`、`animation-range`）；`animation-composition` 和 keyframe 的 `composite`（只有 `replace`）；`@starting-style` 和 `display`、`content-visibility` 的 transition（`allow-discrete` 只影響 kun 有的 discrete property）；kun 還沒有的 transform 函式（`rotate()`、`skew()`、3D）和 property（`transform-origin`、`translate` / `rotate` / `scale`）；`will-change`；compositor 上跑的動畫（RFC 0049）；SMIL（SVG 的 `<animate>`）。

## 設計概要

網頁的作者看到的：

- `.button { transition: background-color .2s ease-out } .button:hover { background-color: red }`：滑鼠移上去，背景在 0.2 秒裡變成紅色，結束時發 `transitionrun`、`transitionstart`、`transitionend`；中途移開，照規範從目前的值反向（reversing shortening factor）。
- `@keyframes blink { 50% { opacity: 0 } } .cursor { animation: blink 1.1s steps(2, start) infinite }`：游標一直閃，每次重複發 `animationiteration`。
- `getComputedStyle()` 在動畫中途讀到插值的結果，和 Chrome 一樣。
- `prefers-reduced-motion` 照舊永遠是 `no-preference`（kun 沒有這個設定）。
- `el.getAnimations()`、`document.timeline.currentTime`、`el.animate(keyframes, options)`、`animation.pause()` / `finished`、`commitStyles()`，和 Chrome 一樣。

改 kun 的人看到的：`style` 多一層「動畫」：每個 document 有一條 timeline 和一份元素的動畫表（Blink 的 `DocumentAnimations`、`ElementAnimations`）；style recalc 比較舊的和新的 style 開始 transition（Blink 的 `CSSAnimations::CalculateTransitionUpdate`），照 `animation-name` 建動畫，cascade 把動畫在這個時間的值當成一個 origin 套上；renderer 的 rendering update 多一步「update animations and send events」，動畫在跑時一直要 frame。

## 詳細設計

### 模型

- **timeline**：每個 document 一條（Blink 的 `DocumentTimeline`），時間是這個 frame 的 animation time：和 `requestAnimationFrame` 拿到的時間同一個值（Blink 的 `AnimationClock`，一個 frame 裡不變）。新的 transition 先是 pending（停在開始的值、只發 `transitionrun`：css-transitions 2 的 idle 到 pending），第一個畫出它的 frame 的 lifecycle 跑完時，start time 才定成那個 frame 的時間（Blink 的 `PendingAnimations::Update`）。原本 start time 直接用開始它的 style change 時 timeline 的時間，但頁面閒置時沒有 frame，timeline 一直停在上一個 frame（還沒有動畫的頁面更是停在 0），第一次點 `about:demo` 的長條時 transition 從那個舊的時間算起、一開始就結束（2026-10-15 使用者在視窗裡看到，`shell` 的 `transitions::a_first_transition_later_starts_then` 守住）；timeline 也改成每個 frame 都更新，不管有沒有動畫。
- **animation 和 effect**：一個 `Animation` 有 start time、hold time、playback rate 和播放狀態，帶一個 `KeyframeEffect`（target、timing：delay、duration、iteration、direction、fill、easing，和 keyframe 的清單）；CSS transition 和 CSS animation 是 owner 是 CSS 的 `Animation`（Blink 的 `CSSTransition`、`CSSAnimation`），規則照 css-transitions 1 和 css-animations 1。計算照 web-animations 1 的「timing model」（active time、iteration progress、transformed progress），是純函式，單元測試直接給時間。
- **狀態放在哪裡**（第 2 條）：`dom` 裡每份 document 一張動畫表（Blink 的 `DocumentAnimations` 和 `ElementAnimations`），key 是（元素，pseudo-element），資料是一般的 Rust、不用 GC 追蹤；`style` 在 recalc 時經過它拿著的 DOM 讀寫，renderer 的 rendering update 也從這裡拿；元素離開文件、adopt 到別的 document 時一起清掉。第 3 段 JS 的物件只放動畫的編號（第 15 條，見「Web Animations 的 JS API」）；沒有 target 的動畫放在 document 的一份清單，沒有動畫的 effect 另外一張表（第 20 條）。

### 和 style 怎麼接

- **新的 property**（`css` 的 `properties.rs`）：上面「目標」列的 longhand 和兩個 shorthand，值是逗號分隔的清單；computed value 放在 `ComputedStyle`（清單是 `Arc<[T]>`，和 `background-*` 一樣）。
- **`@keyframes`**：`stylesheet.rs` 的 `CssRule` 多一種規則，keyframe 的 block 有自己的 parser（每個 keyframe 是 selector 的清單加一組宣告，`!important` 的宣告丟掉，`animation-*` 本身丟掉、`animation-timing-function` 留下）；`RuleSet` 收集名稱到規則的表（`@media`、`@supports` 照樣把關，同名的後面的勝出）。找名稱照 css-scoping：從元素的 tree scope 往外找（`ScopeCascade`），shadow tree 的 `@keyframes` 不給外面用。
- **開始 CSS animation**（第 9 條）：style change 時先照 `animation-name` 對應已有的動畫（同名的照 css-animations 1 從尾端配對，Blink 的 `CalculateAnimationUpdate`；第一版從頭配對，3b 的 WPT 抓到），對到的留著開始時間、換上新的 timing 和 play state，新的名稱有 `@keyframes` 才開始（和 transition 一樣等顯示它的 frame 才定開始時間），其他的取消。每個 keyframe 在元素上 cascade 一次（animation origin），一個 property 的 keyframe 照 offset 排、同一個 offset 後面的勝出，缺 0% 或 100% 的是 base style；兩個 keyframe 之間照前一個的 `animation-timing-function`（沒寫的是動畫的）插值，動畫本身是 linear。沿用上次的 keyframe 的條件除了 base style 一樣，還有規則（同一個 `Arc`）、parent 的 style、root 的 font size 和 viewport 一樣：keyframe 的 `inherit`、`rem`、`vw` 讀它們，只比 base style 會漏掉；keyframe 用到 viewport 單位時元素的 style 也標成用到，viewport 改了才會重新 style 它。transition 的 before-change 和 after-change style 都有 CSS animation 在這個時間的值，所以動畫中的 property 不因別的改變開始 transition。
- **開始 transition**（`style` 的 recalc，`StyleEngine::recalc_style`）：在有舊的 style 的地方（`node.computed_style()` 和新的 `cascade` 之間），照 css-transitions 1 的「Starting of transitions」比較 before-change style 和 after-change style 的每個 transition 的 property（`all` 展開成有 animation type 的 longhand），開始、取代或取消 transition；不能插值的一對（discrete 的 property，或 `auto` 對長度）只在 `allow-discrete` 時開始（一步）。before-change style 是 transition 在這個時間的值重新套到 base style：存在元素上的是上一個 frame 的，或還帶著一個剛結束的（原本直接用存著的那份，WPT 的 `transition-zero-duration-with-delay.html` 抓到從上一個 frame 的值開始）；沒有 transition 的元素是存著的那份。沒有舊的 style 的元素（剛建立、從 `display: none` 出來）不開始 transition；移除過的元素也是：`dom` 在移除時記下子樹的根（`Animations::removed`，用過 transition 之後才記），style 不用它們移除前的 style，搬到別處的元素的 transition 取消，再次 style 之後忘掉（css-transitions 1：新進文件的元素沒有 before-change style）。
- **動畫的值怎麼套上**（第 3 條）：cascade 的一個 origin（Blink 的 `StyleCascade`）：`cascade_blocks` 找到每個 longhand 的勝出者之後，有動畫的 longhand 照 css-cascade 的順序換成動畫的值（已經是 computed value）：animation 蓋過 author normal、擋不住 `!important`，transition 蓋過所有 `!important`；`font-size` 照現在的順序先套，`em` 等跟著動畫的 `font-size`。`CascadePriority` 多兩級。
- **每個 frame 重算**（第 4 條）：照 Blink 的 base computed style：動畫表記住元素沒有動畫時的 style（`ElementAnimations::base_computed_style_`），元素本身沒有被標記、只有動畫在跑時，每個 frame 只在 base style 上套動畫的值，不重新比對 selector；只有動畫的 property 是繼承的（`color`、`visibility`）時子樹才重算，否則子樹不動。元素本身、祖先、stylesheet、viewport 改了時 base style 作廢，走一般的路徑。動畫的值只改 paint 相關的 property 時，layout 照 `StyleDifference::Paint` 只換盒子的 style、不重排（box tree 的 `restyle` 已經在做）。
- **插值**（第 5 條）：`LonghandId` 多一個 `animation_type()`（by computed value、discrete、not animatable），kun 有的每個 longhand 都照規範給；第 1 段做完每種值：數字、整數、長度和百分比（不同單位時混成 `calc()`）、顏色（舊式的顏色照 Chrome 在 premultiplied sRGB 裡插值）、陰影的清單、transform 的清單（函式對得上時逐項，否則照 css-transforms 1 的 2D 矩陣分解）、`visibility`、背景的 position 和 size、`border-radius`，其他 discrete（50% 時換）。`0%` 和長度照樣混成 `calc()`，只有 `translate()` 把零的長度也當成零的百分比（第一版套到所有 `<length-percentage>`，`*-interpolation.html` 抓到）；`cubic-bezier()` 的控制點和 `linear()` 的 stop 照寫的文字讀成 `f64`（`cssparser` 的數字是 `f32`，色彩 channel 剛好 .5 時捨去）。`var()` 在 keyframe 裡照元素代換（keyframe 的值在元素的 context 裡算成 computed value 再插值），自訂 property 本身是 discrete（沒有 `@property`）。

### renderer

- `begin_main_frame`（`render_thread.rs`）在 `report_media_query_changes` 之後、`run_animation_frame_callbacks` 之前多一步「update animations and send events」（HTML 的 update the rendering）：更新每個 document 的 timeline 的時間、transition tick 出事件，把前進的和剛結束的元素標成要重算（剛結束的 style 要回到 base style），再發事件（照排定的時間、transition 的順序排序，document 照 tree order，`dispatch_from_renderer` 的寫法）。離開文件的元素的 transition 取消、發 `transitioncancel`（在不在文件裡的元素上，WPT 的 `disconnected-element-001.html`）；不在任何 frame 裡的 document 的停掉、不發事件。
- **要 frame**：有在跑的動畫時，`set_needs_begin_main_frame` 的條件多一項（和 `wants_animation_frame` 並列）；都結束、或只剩 `paused` 和沒有在跑的 fill 時不再要。隱藏的分頁本來就不給 `BeginFrame`；時間照舊往前，顯示時動畫跳到該在的位置（Chrome 也是）。
- **事件**：`TransitionEvent`（`propertyName`、`elapsedTime`、`pseudoElement`）、`AnimationEvent`（`animationName`、`elapsedTime`、`pseudoElement`），`EventKindInit` 多兩種；`ontransition*`、`onanimation*` 的 handler 原本就有（`onanimationcancel` 是新加的），`webkit` 的舊名字的對應也原本就有。JS 和 wasm 都給（宣告表的規則）。
- **時間的來源**（第 6 條）：animation time 就是 rAF 拿到的時間（main frame 開始時 renderer 的時鐘），動畫、rAF、timer 用同一個時鐘。`BeginFrameArgs.frame_time` 照舊不用（rAF 的時間是另一個題目）；測試用負的 delay、長的 duration 和 `steps()` 讓結果固定，不夠時再讓 renderer 的 `Clock` 能注入。

### pseudo-element

`::before`、`::after`、`::marker` 的 style 存在元素上（`set_pseudo_styles`），它們也會有 transition 和動畫（Speedometer 的 Complex-DOM 的 `::before` 有 `transition`）：動畫表的 key 是（元素，pseudo-element），事件的 `pseudoElement` 照它。第 1 段就連 pseudo-element 一起做（第 1 條）。

### Web Animations 的 JS API

- **一種 `Animation`**（第 14 條）：`animation` crate 的 `Animation` 帶 `Playback`（start time、hold time、playback rate 和 pending 的 rate、pending 的 play 和 pause，web-animations 1 的 play、pause、finish、cancel、reverse、設 current time 和 start time、`updatePlaybackRate`、「update the finished state」）、`Timing` 和一個 `KeyframeEffect`；`Owner` 是 transition、CSS animation 或 script，CSS 的規則照舊在 `style`、各自的事件照 css-transitions 2 和 css-animations 2 的表發，promise 和 playback 的事件只有一套。transition 的 effect 是兩個 keyframe 的 `KeyframeEffect`。
- **JS 的物件只放編號**（第 15 條）：`Animation`、`CSSAnimation`、`CSSTransition`、`KeyframeEffect`、`DocumentTimeline` 是 `dom` 的 GC 型別，狀態在動畫表；同一個動畫一直是同一個物件（document 的 `WeakSet`，動畫還拿得到時由 document 留著），只有 script 拿著、已經 idle 的動畫在物件被回收之後由 `prune` 清掉。`ready` 和 `finished` 是 engine 留著的 promise（第 18 條，`ScriptHost::create_kept_promise`），JS 和 wasm 拿到的是同一個、settle 過的也是。
- **動畫在哪裡**：元素的 `ElementAnimations` 有 style 的清單（transition、CSS animation）和 `others`：script 的動畫，和離開 CSS 規則、effect 的 target 還是這個元素的動畫（第 19 條；style 取消、取代的斷開 owning element，不再發 CSS 的事件）。原本把離開規則的搬到 `detached`，值不套、不發 CSS 的事件，WPT 的 `event-dispatch.tentative.html` 抓到和規範不同；`detached` 只剩元素離開文件、`display: none`、pseudo-element 不在了時整份拿掉的。沒有動畫的 `KeyframeEffect` 在另一張表（第 20 條），換 effect 時從一邊搬到另一邊，改 `target` 時動畫搬到另一個元素。元素離開文件、不再 render 時只取消 markup 的動畫，script 的留著（原本一起取消，`interpolation-testcommon.js` 的 Web Animations 從第二組起全部失敗）。
- **script 的 keyframe**（第 21 條）：存宣告（`DeclaredKeyframes`），在 target 的 style recalc 時和 `@keyframes` 走同一條路算，`em`、`var()` 跟著元素；`getKeyframes()` 讀回 serialize 過的值，`computedOffset` 照「compute missing keyframe offsets」。script 改過的 CSS animation 的 keyframe 和 timing 的成員（`TimingOverrides`，逐項）不再被 style 改。
- **composite order**：transition、CSS animation 之後是 `others`，照 global animation list（`global_order`；style 不再管的 CSS animation 第一次再離開 idle 時排到最後，照 Gecko）；`getAnimations()` 和事件照它，元素、`::marker`、`::before`、`::after`、子孫。script 的動畫算進 transition 的 before-change 和 after-change style（它蓋住的 property 不 transition）。
- **rendering update**：timeline 前進、tick 之後，先 settle promise（一次 microtask checkpoint），再移除被取代的動畫（`remove_replaced`，照 Blink 的 `RemoveReplacedAnimations`：結束、填著、property 全被後面的蓋住的，`persist()` 的不移除；移除不改值，所以不重新 style），再照排定的時間發事件；frame 的 lifecycle 之後定開始的動畫的 start time、resolve `ready`，最後交還停下來的動畫的 promise、放掉 script 拿不到的。`finish` 排在效果結束的時間（不超過現在），同時間的事件照 target 的 tree order。
- **時間**：current time 取到整數的微秒，到 end 差一微秒以內算到了（Blink 的時間是微秒，`IsWithinAnimationTimeTolerance`）：`finish()` 把 start time 設成現在減掉 end，`(now - end) + end` 的浮點數有時比 end 小一點，動畫沒有 finish、`finish` 事件排到了同時的 `cancel` 之後（WPT 的 `update-and-send-events.html` 並行跑時偶爾失敗）。
- **`commitStyles()`**：照 composite order 到它為止的 effect stack（`apply_up_to`，endpoint-inclusive 的 timing）算 computed value，寫進 inline style。`Animation.timeline` 可以設成 `null` 或 document 的 timeline（第 22 條）。
- **`@keyframes` 的 CSSOM**：第 2 段做了 Editor-CodeMirror 要的部分（第 11 條），第 3 段補齊 `name` 和 `keyText` 的 setter、`appendRule()`、`deleteRule()`、`findRule()`、可以改的 keyframe `style`；改了規則的動畫跟著重算。

### compositor 的動畫：不做（第 23 條）

動畫全部在 main thread 上：`transform` 和 `opacity` 的動畫和其他的一樣，每個 frame 重算 style、重新 paint，畫進它所在的 layer、改變的範圍 re-raster；JS 忙的時候不動。照 Chromium 交給 compositor thread（paint 的 compositing reason、compositor 的矩陣和 effect tree、Viz 的 affine quad 和 render surface、impl thread 的動畫）改的是 paint、compositor 和 Viz 的架構，和這份的動畫模型分得開，移到 RFC 0049。

### 測試

- **`css` 的整合測試**：新 property 的解析、序列化、computed value；`@keyframes` 的解析（無效的 keyframe selector、`!important`、重複的百分比）。
- **`style` 的整合測試**（`animations`、`transitions`）：timing model（每種 fill、direction、iteration、負的 delay、`steps()` 的邊界）；每種值的插值；開始、取代、反向、取消 transition 的條件；`@keyframes` 在 tree scope 裡的查找；動畫的 origin 和 `!important` 的順序。
- **`shell` 的整合測試**：`getComputedStyle` 在中途讀到的值（用很長的 duration 和負的 delay，讓結果不受真實時間影響，WPT 也這樣做）、事件的種類和順序、動畫結束後不再要 frame、隱藏的分頁。
- **kun 的 reftest**：`animations/`（新，用 `steps()` 和負的 delay 固定在某一格）。
- **`dom_js`、`shell`、`dom_wasm` 的整合測試**（第 3 段）：`dom_js` 的 `animations` 扮演 renderer 的 frame，測 playback 的程序、promise、事件、keyframe 參數、被取代的動畫、`commitStyles()`；`shell` 的 `web_animations` 用真的 frame；`dom_wasm` 的 guest 用 C 寫。
- **WPT**：`css/css-transitions/`、`css/css-animations/`、`css/css-easing/`、`web-animations/` 整個，和各 property 的 `*-interpolation.html`（用 `interpolation-testcommon.js` 的 336 個檔案）；失敗的 `.ini` 寫了原因。
- **情境**：`about:demo` 加一個 hover 有 transition 的按鈕和一個 spinner；情境驗「結束後的樣子」和事件，順不順由人看（`docs/test-gaps.md` 的「只做過手動確認」）。
- **mutation test**：每段一份。
- **量測**：不排（Speedometer 只在使用者要求時量）。

### 分階段

第 7 條：

1. **timing model 和 transition**：新的 property（`transition-*`）和 easing、timeline 和 `Animation` / `KeyframeEffect` 的內部模型、動畫的 origin、插值、開始和取消 transition、rendering update 的一步和要 frame、`TransitionEvent`（1a 到 1e）。
2. **CSS animation**：`animation-*`、`@keyframes` 和 tree scope 的查找、iteration、direction、fill、`play-state`、`AnimationEvent`（2a、2b、2c，第 12 條）。
3. **Web Animations 的 JS API**：`document.timeline`、`Animation`、`KeyframeEffect`、`Element.animate()`、`getAnimations()`、`CSSTransition`、`CSSAnimation`、被取代的動畫自動移除和 `commitStyles()`，`@keyframes` 的 CSSOM 補齊；加 WPT 的 `web-animations/` 和 `*-interpolation.html`（3a、3b、3c，第 17 條）。
4. **compositor 的動畫**：不在這份做，移到 RFC 0049（第 23 條）。

## 缺點

- 動畫都在 main thread 上：每個 frame 重算動畫的元素的 style、重畫和重新 raster 它所在的 layer；JS 忙的時候動畫會停（Chrome 的 transform、opacity 動畫不會），等 RFC 0049。
- 每個 property 都要有 animation type 和插值，之後每加一個 property 都要決定它怎麼插值。
- 時間照真實的時鐘時，自動測試只能用負的 delay、很長的 duration 和 `steps()` 讓結果固定（第 6 條）。

## 理由和其他做法

完整的比較在 `details.md`。

- **不做**：hover 等狀態的變化一步跳到終點，`transitionend`、`animationend` 等不到；Speedometer 的 async 時間少算了 Chrome 在 transition 期間的 style 和 paint。

## 先例

- **Blink**：CSS transition 和 animation 建在 Web Animations 上：`CSSAnimations::CalculateTransitionUpdate`、`CalculateAnimationUpdate` 在 style resolve 時算出要開始、取消的動畫，`MaybeApplyPendingUpdate` 套用；`ElementAnimations`（在 `ElementRareData`）存元素的動畫和 base computed style；`DocumentTimeline`、`AnimationClock`、`PendingAnimations`；`InterpolationType` 和 `CSSInterpolationType` 的每種值的插值，`StyleCascade` 的 animation origin；`DocumentAnimations::UpdateAnimationTimingForAnimationFrame` 在 `PageAnimator::ServiceScriptedAnimations` 裡、rAF 之前；事件經過 `CSSAnimations` 的 event delegate 排進 document 的 animation 事件。能在 compositor 跑的由 `CompositorAnimations::CheckCanStartAnimationOnCompositor` 判斷，cc 的 `AnimationHost`、`KeyframeModel` 在 impl thread 上跑。
- **Gecko**：`nsTransitionManager`、`nsAnimationManager` 也建在它的 Web Animations（`dom/animation/`）上；Stylo 算出 transition 要不要開始，動畫的值在 cascade 的 animation 和 transition 的 origin。
- **Servo**：`style::animation` 的 `ElementAnimationSet` 是自己的模型，沒有完整的 Web Animations，`getAnimations()` 等之後再補（`details.md`）。

## 已決定的事項

2026-10-15 確認（第 1 段開始前要決定的；選項的比較在 `details.md` 的「選項的比較」）：

1. **第 1 段連 pseudo-element 一起做，事件 JS 和 wasm 都給**（P1）：`::before`、`::after`、`::marker` 的 transition 和元素的一起做，動畫表的 key 是（元素，pseudo-element）；Complex-DOM 的 `::before` 有 `transition`，之後的 `getAnimations()` 也要列出 pseudo-element 的。不選先只做元素本身（P2）：pseudo-element 的 transition 一步跳過去，之後要回頭改 key 和事件。`TransitionEvent`、`AnimationEvent` 照宣告表的規則 JS 和 wasm 都給。
2. **動畫的狀態在 `dom` 裡每份 document 一張表，資料不用 GC 追蹤**（M1）：和 Blink 的 `ElementAnimations` 掛在元素上同一個位置，`style`、renderer、之後的 `dom_api` 都拿得到，第 1、2 段不碰 `unsafe`；第 3 段 JS 的 `Animation` 是用 id 找到它的 GC wrapper。不選放在 `style` 的 `StyleEngine`（M2）：renderer 和 `dom_api` 要經過 `style` 拿，元素移除時 `style` 不一定知道。不選一開始就做成 GC 物件（M3）：第 1、2 段沒有 script 看得到它，卻要付 GC 型別的成本。
3. **動畫的值是 cascade 的一個 origin**（V1，Blink 的 `StyleCascade`）：animation 在 author normal 之上、`!important` 之下，transition 在所有 `!important` 之上，動畫的 `font-size` 先套。不選 cascade 完之後直接改 `ComputedStyle`（V2）：`font-size` 的動畫不影響 `em`，`!important` 的順序和繼承都要另外補。
4. **每個 frame 照 Blink 的 base computed style 重算**（R2）：只在記住的 base style 上套動畫的值，動畫的 property 是繼承的才重算子樹。不選照一般的 recalc（R1）：重算一個元素會連整個子樹重算，`body` 上的 `color` transition、Complex-DOM 大量的 `all` transition 讓每個 frame 都是一次大的 recalc。
5. **kun 有的 property 都照規範的 animation type，第 1 段做完每種值的插值**（I1）：`transition: all` 的結果和 Chrome 一樣，第 3 段的 `*-interpolation.html` 直接能跑，之後加 property 時有地方宣告它的 animation type。不選只做 Speedometer 用到的（I2）：其他 property 在一半時跳過去，和 Chrome 不同、不易察覺，之後還要找出哪些是暫時的。
6. **animation time 是 rAF 的時間**（C1）：一個 frame 一個值，動畫、rAF、timer 用同一個時鐘，`transitionend` 和 `setTimeout` 的先後和 Chrome 一樣；測試用負的 delay、長的 duration 和 `steps()` 固定結果。不選改用 `BeginFrameArgs.frame_time`（C2）：rAF 的時間是另一個題目（已經記在 `docs/limitations.md`），headless 的 frame time 和真實時間脫鉤，timer 和動畫的先後在測試和視窗裡會不同。測試能換的時鐘（C3）等 C1 的測試不夠時再做。
7. **分四段**：transition、`@keyframes`、Web Animations 的 JS API、compositor 的動畫；第 1 段做完共用的基礎（timing model、插值、origin、要 frame），第 2 段只加 `@keyframes` 那一層，每段做完可以先 merge（和同時進行的 RFC 0047 衝突的範圍小）。不選 transition 和 `@keyframes` 合成一段：一段很大，Speedometer 用得多的是 transition。

2026-10-15 確認（第 1 段的 1c 開始前）：

8. **timing model、`Animation`、`KeyframeEffect`、timeline 和 transition 開始、取消的規則放在新的 `animation` crate**（A，Blink 的 `core/animation`）：依賴 `css`、`computed_style`，不連 V8；`dom` 只存它的型別（第 2 條的表在 `DomState`，以 `NodeId` 為 key，不在 GC 的 heap 上，style recalc 經過 `DomReadScope` 寫），`style`、renderer 和之後的 `dom_api` 直接用它。不選放在 `dom` 的模組（B）：`dom` 對應 `core/dom`，測試要連結 V8。不選型別放在 `computed_style`、邏輯放在 `style`（C）：renderer 和 JS API 要經過 `style` 拿。三個做法的執行成本一樣。

2026-10-15 確認（第 2 段開始前）：

9. **keyframe 的值在每次 style change 時，在元素上 cascade 每個 keyframe 得到**（K1）：每個 keyframe 的宣告當成 animation origin 的區塊疊在元素的 cascade 上，各得一份 `ComputedStyle`（`var()`、`em`、`currentcolor`、`inherit` 照元素算），缺的 0%、100% 是 base style；新的 base style 和上次的一樣時沿用上次的 keyframe；frame 時只在相鄰的兩份之間插值（第 5 條）。不選每條規則只算一次、元素之間共用（K2）：和元素有關的值都錯（Complex-DOM 的 `@keyframes` 用 `var()`）。不選每個 frame 重新 cascade（K3）：`infinite` 的動畫一直付。不選照 Blink 存宣告值、照依賴快取（K4）：kun 的插值在 `ComputedStyle` 的欄位上做、cascade 不記依賴，要重做一套每種值的換算和依賴的記錄；它省下的主要是 base style 沒變的重新 style，已經由沿用上次的 keyframe 省下，之後量到瓶頸再改。
10. **`@keyframes` 放在每個 tree scope 的 `RuleSet`，從元素的 tree scope 往外找**（R1，Blink 的 `ScopedStyleResolver::KeyframeStylesForAnimation`）：`@media`、`@supports` 照樣把關，同名的後面的勝出。不選整份 document 一張表（R2）：Perf-Dashboard 的元件在 shadow tree 裡的 `@keyframes` 會找不到，外面也找得到裡面的。
11. **`@keyframes` 的基本 CSSOM 在第 2 段**（C1）：`insertRule()`、`replace()` 收 `@keyframes`，`CSSKeyframesRule` 的 `name`、`cssRules`、`cssText`，`CSSKeyframeRule` 的 `keyText`、唯讀的 `style`，JS 和 wasm 都給；`appendRule()`、`deleteRule()`、`findRule()` 和改 `name`、`keyText` 留在第 3 段。原本整個 CSSOM 放在第 3 段，但 Editor-CodeMirror 用 `insertRule()` 插入它的 `@keyframes`（沒有 try/catch），kun 丟 `SyntaxError`。不選整個 CSSOM 都在第 2 段（C2）：改了規則要讓動畫重算，第 2 段變大，Speedometer 用不到。不選照原本（C3）：`cssRules` 和 `insertRule` 的 index 和 Chrome 不同。
12. **第 2 段分 2a、2b、2c，2c 做完再 merge**：2a `animation-*` 和 shorthand、`@keyframes` 的解析、`RuleSet` 和查找、CSSOM；2b `animation` crate 的 CSS animation、keyframe 的值、cascade 的 animation origin；2c `AnimationEvent`、renderer、`about:demo`、情境、WPT。

2026-10-15 確認（第 3 段開始前）：

13. **第 3 段做 web-animations 1 的主體**（W1）：`document.timeline`、`Animation`（建構子、`play`、`pause`、`finish`、`cancel`、`reverse`、`updatePlaybackRate`，可寫的 `currentTime`、`startTime`、`playbackRate`，`playState`、`pending`、`ready` 和 `finished`、`onfinish`、`oncancel`、`onremove`）、`AnimationEffect` 和 `KeyframeEffect`、`Element.animate()`、`getAnimations()`、`CSSAnimation`、`CSSTransition`、`AnimationPlaybackEvent`、被取代的動畫自動移除（`persist()`、`commitStyles()`）；`composite` 只收 `replace`，scroll-driven 的不做。不選常用的子集（W2）：`web-animations/` 大多跑不起來，之後補時還要再改模型的邊界。不選只讀（W3）：`*-interpolation.html` 的 Web Animations 的那一組和 `getAnimations()[0].pause()` 都不行。Speedometer 都用不到。
14. **內部合成一種 `Animation` 帶一個 `KeyframeEffect`，CSS transition 和 CSS animation 是它的 owner**（M1，Blink 的 `Animation`、`KeyframeEffect`、`CSSTransition`、`CSSAnimation`）：CSS 的規則照舊在 `style`，JS 的控制、pending、promise、事件、composite order 只有一套。不選三種各自的結構（M2）：pause、seek、playback rate、promise、事件要寫三份。第 1、2 段的 mutation spec 跟著改、重跑。
15. **JS 的 `Animation`、`KeyframeEffect`、`DocumentTimeline` 只存動畫的 id，狀態在 `dom` 的動畫表**（G1）：style 和 renderer 照舊不碰 GC 的 heap；同一個動畫回同一個物件；只有 script 拿著、已經 idle 的動畫留在表裡，物件被回收之後清掉（和外部 engine 的 store 的 `prune` 一樣）。不選照 Blink 把動畫放在 GC 的 heap 上（G2）：style recalc 和 renderer 要經過 GC 的 heap 讀寫，`soundness.md` 的論證要重寫。
16. **keyframe 是 `object` 的四個成員（`animate()`、`KeyframeEffect` 的建構子、`getKeyframes()`、`setKeyframes()`）只給 JS，其他 JS 和 wasm 都給**（K1，宣告表的規則）。不選另外給 wasm 一個降階的 keyframe（K2）：多一個只給 WIT 的形式和轉換，guest 可以用 CSS 的動畫。不選第 3 段只給 JS（K3）：和規則不一致。
17. **第 3 段分 3a、3b、3c，3c 做完再 merge**：3a 模型換成 M1、`document.timeline`、`getAnimations()`、`Animation` 的控制、promise 和事件、`CSSAnimation`、`CSSTransition`；3b `Element.animate()`、`KeyframeEffect`、被取代的動畫自動移除、`commitStyles()`；3c `@keyframes` 的 CSSOM 補齊（第 11 條留下的）、WPT 的 `web-animations/` 和 `*-interpolation.html`。

2026-10-15 確認（3a 做到一半時）：

18. **`ready` 和 `finished` 用 engine 留著的 promise**（P1，Blink 的 `ScriptPromiseProperty`）：`ScriptHost` 多 `create_kept_promise()` 和 `release_promise()`，JS 的表在 settle 之後照樣留著它、每次交出同一個物件，wasm 等一個已經 settle 的就直接拿到結果；`dom` 的動畫表存編號，換新的或動畫清掉時 release。不選只在 JS 的前端快取（P2）：wasm 拿到的是另一個 promise，兩邊的行為不同，`dom_api` 還要分辨是哪個 engine。

2026-10-16 確認（3a 做到一半時）：

19. **離開 CSS 規則的動畫留在元素上**（R1，Blink 的 `ElementAnimations` 放元素的所有動畫、`CSSTransition` 另外記 owning element）：元素的 `ElementAnimations` 除了 style 的清單，多一份 style 不再管、effect 的 target 還是它的動畫。style 取消、取代的斷開 owning element（css-transitions 2、css-animations 2：不再發 CSS 的事件，playback 的事件照發）；結束的 transition 留著 owning element（seek 回去照樣發 `transitionstart`、`transitionend`）；script `cancel()` 的照樣在 style 的清單裡、留著 owning element（再 `play()` 照樣發 `transitionrun`）。它們在 effect 裡時照樣套值（在 CSS animation 之後、transition 之前，composite order 是 script 的動畫）。只有元素離開文件、或它的那一份整個拿掉時（`display: none`、pseudo-element 不在了）才搬到 `dom` 的 `detached`。不選維持原本的 `detached`（R2）：值不套到元素、不再發 CSS 的事件，和規範不同，3b 的 `element.animate()` 本來就要放在元素上，到時候一樣要改。

2026-10-16 確認（3b 開始時）：

20. **`KeyframeEffect` 在它的動畫裡，沒有動畫的另外一張表**（E1，Blink 的 `KeyframeEffect` 是 `Animation` 指著的物件）：JS 的物件照第 15 條只放 effect 的編號；換 effect 時從一邊搬到另一邊，改 `target`、`pseudoElement` 時動畫搬到另一個元素的 `ElementAnimations`（沒有 target 的放在 document 的一份清單）；effect 的 target 和動畫的 owning element 分開記。代價是找 effect 要先掃動畫、搬家的路要寫對（mutation test 守），沒有動畫的 effect 物件被回收後要清。不選全部放在一張表（E2）：style 每次套值多查一次表，元素的清單和 effect 的 target 兩邊都要同步。
21. **script 給的 keyframe 存宣告，在 target 的 style recalc 時算**（V1，Blink 的 `StringKeyframe`）：和 CSS animation 的 keyframe 同一套（`KeyframeSource` 的快取、`Restyle::with_keyframe`），`em`、`var()`、`inherit` 跟著元素變，`getKeyframes()` 回傳寫的值。不選設的時候算一次（V2）：之後元素的 style 變了值不跟著，和規範不同。
22. **`Animation.timeline` 可以設成 `null` 或 document 的 timeline**（T1）：沒有 timeline 的動畫不前進，current time 只有 hold time；`new Animation(effect, null)` 也行。不選維持唯讀（T2）：用到它的 WPT 照樣失敗。

2026-10-16 確認（第 4 段開始前）：

23. **compositor 的動畫不在這份 RFC 做，移到 RFC 0049**（C3）：RFC 0048 做到第 3 段為止，動畫照舊全部在 main thread 上。第 4 段要改的是 paint 的 layerize、compositor 的 property tree（矩陣、effect tree）、Viz 的 affine quad 和 render surface、impl thread 的動畫，和這份的動畫模型分得開，自成一份 RFC 比較好審、好排。不選照 Chromium 全做（C1）：連 render surface 和兩個 renderer 的 affine quad，大約四段。不選先做單一 layer（C2）：不做 render surface，成本約 C1 的六成。兩個都留在 RFC 0049 裡比較。

## 待決定

無：第 1 到 23 條都已決定；compositor 的動畫移到 RFC 0049（第 23 條）。

## 之後的可能

- scroll-driven animation（`animation-timeline`、`scroll()`、`view()`）。
- `@starting-style` 和 `display` 的 transition、`transition-behavior: allow-discrete` 對 `display` 的作用。
- `@property`（註冊的自訂 property 能插值）。
- `animation-composition`、keyframe 的 `composite`、`additive`。
- `prefers-reduced-motion` 的使用者設定。
- compositor 上跑的 transform 和 opacity 的動畫、`will-change` 當成 compositing reason：RFC 0049。
