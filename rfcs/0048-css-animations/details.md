# RFC 0048 的細節：transition 和動畫

## 現況（2026-10-15）

- **style**：`transition-*`、`animation-*` 都不在白名單（`css` 的 `properties.rs`），宣告丟掉，`CSS.supports()` 是 `false`；`@keyframes` 是不認得的 at-rule，整條丟掉（`stylesheet.rs` 只認 `@media`、`@supports`）。`ComputedStyle` 是一個平的 struct，存成 `Arc` 放在元素上，每次 cascade 從頭建一份；cascade 的 priority 只有 UA、author 的 normal 和 `!important` 四級（`style` 的 `CascadePriority`），沒有 animation、transition 的 origin。recalc 在 `StyleEngine::recalc_style`：重算的元素一律連整個子樹重算，沒有 style sharing、matched properties 的快取。舊的 style 在新的 cascade 之前讀得到（`node.computed_style()`），新舊不同時 `set_style_changed`，layout 照 `StyleDifference`（`Reattach`、`Layout`、`Paint`）決定重建、重排或只換 style。
- **transform 和 opacity**：`transform` 只有 translate、scale、matrix（computed value 是 `TransformOperation` 的清單），沒有 `rotate()`、`skew()` 和 3D，`transform-origin` 不是 property（layout 固定 50% 50%）；`opacity` 是 `f32`。沒有矩陣分解。
- **renderer**：`begin_main_frame` 的順序是滑鼠、autofocus、捲動、media query 的變化、rAF、`ResizeObserver`、focus fixup、標題，再 `update_rendering`（lifecycle、`IntersectionObserver`、commit）；沒有「update animations and send events」。`BeginFrameArgs.frame_time` 沒有用（`let _ = args.frame_time`），rAF 的時間是 renderer 的時鐘（`Instant`）在 main frame 開始時的值。沒有要 frame 的事時，`BeginFrame` 停掉；隱藏的分頁也停。
- **測試的時間**：renderer 沒有假的時鐘；headless 和情境的 harness 連續送 `BeginFrame`（`frame_time` 每次加 1000/60），但 renderer 照真實的時間。
- **compositor 和 Viz**：只有捲動容器、canvas 和 main frame 的 `position: fixed` 會成為 layer，而且只能在只有平移的 transform 底下、沒有 opacity（`paint` 的 `layerize.rs`）；compositor 的 transform node 是一個 offset，沒有 effect tree；Viz 的 quad 沒有 transform、`SharedQuadState.opacity` 一直是 1、只畫 root render pass。impl thread 只會改捲動的位置，沒有動畫。
- **DOM**：沒有 `TransitionEvent`、`AnimationEvent`；`ontransitionrun` / `start` / `end` / `cancel`、`onanimationstart` / `iteration` / `end` 和 `onwebkit*` 的 handler 已經宣告，`onanimationcancel` 沒有；`animationend` 對 `webkitAnimationEnd` 這類舊名字的對應已經有。`CSSRule` 有 `KEYFRAMES_RULE`、`KEYFRAME_RULE` 的常數，沒有那兩個 interface；script 只看得到 constructed stylesheet（沒有 `document.styleSheets`）。`prefers-reduced-motion` 解析得了，永遠是 `no-preference`。

## Speedometer

Speedometer 3.1 的 resources 裡的用法（2026-10-15 grep `../third_party/speedometer/resources`）：

- **`transition`**：最多的是 `transition: var(--transition-default)`（`--transition-default: all .3s ease`，Complex-DOM 共用的靜態 DOM），和 Spectrum 元件的 `transition: background .13s ease-out, border-color …, color …, box-shadow …`、`opacity`、`margin`、`transform`、`border-color` 等，duration 多半寫在 `var()` 裡；TodoMVC 的 `transition: color .2s ease-out` / `.4s`、`width 1s`；NewsSite 的 `transition-property: left`、`color, background-color`；charts 的 `fill .3s`。transition 的 property 是顏色、長度、`opacity`、`transform`、`box-shadow`、`fill`、`all`。
- **`@keyframes`**：Complex-DOM 的 `indeterminate-loop-ltr` / `rtl`（progress bar 的 `transform: translate(calc(var(…) * -1))`）沒有規則用到，只要解析得了；兩個 Editor 的游標閃爍（CodeMirror 的 `cm-blink`、`cm-blink2`，TipTap 的 `ProseMirror-cursor-blink 1.1s steps(2, start) infinite`）和 Perf-Dashboard 的 `spinner-animation 1.6s linear infinite` 是真的在跑的動畫，都是 `infinite`：Chrome 在測試期間一直有 frame。
- **影響**：transition 由 `:hover`、`:checked`、class 的變化觸發；Speedometer 的步驟大多改 DOM 和 class，transition 讓之後的 frame 多了 style、paint。`all` 的 transition 在大量的元素上，每個 recalc 要比較每個有 animation type 的 property（第 4 條的成本）。

## 選項的比較

### 第 1 段的範圍（第 1 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **P1 連 pseudo-element 一起（建議）** | Complex-DOM 的 `::before` 有 `transition`；動畫表的 key 是（元素，pseudo-element），之後的 `getAnimations()` 本來就要列出 pseudo-element 的（`CSSAnimation.effect.pseudoElement`） | pseudo-element 的 style 另外存（`set_pseudo_styles`），舊的值要在 recalc 時多讀一次 |
| P2 先只做元素本身 | 第 1 段小一點 | pseudo-element 的 transition 一步跳過去；之後要回頭改 key 和事件 |

事件的 interface 照宣告表的規則 JS 和 wasm 都給：`TransitionEvent`、`AnimationEvent` 只有三個唯讀的成員和建構子，沒有例外的理由。

### 動畫的狀態放在哪裡（第 2 條）

`ComputedStyle` 照規定只放純資料（`Send + Sync`、沒有 `Drop`，GC 的 finalizer 可能放掉它），動畫的狀態不能放在它裡面。

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **M1 `dom` 裡每份 document 一張動畫表（建議）**：key 是（元素，pseudo-element），資料是一般的 Rust（不用 GC 追蹤）；`style` 在 recalc 時經過它已經拿著的 DOM 讀寫；元素離開文件時一起清掉；第 3 段 JS 的 `Animation` 是 GC 的 wrapper，用動畫的 id 找到它（同一個動畫回同一個 wrapper） | 和 Blink 的 `ElementAnimations` 掛在元素上同一個位置；`style`、renderer 的 rendering update、之後的 `dom_api` 都拿得到；資料不進 GC，第 1、2 段不碰 `unsafe` 和 soundness 的論證 | 元素移除、adopt 到別的 document 時要記得清；第 3 段 `element.animate()` 建的動畫在 target 不在時也要活著（表的 key 改成動畫的 id，元素只是欄位） | 查表是 hash map；沒有動畫的元素零成本 |
| M2 放在 `style` 的 `StyleEngine` | 只有 style 會改它 | renderer 的 rendering update 和之後的 `dom_api` 要經過 `style` 拿；元素移除時 `style` 不一定知道（沒有樣式的子樹不一定會被走到） | 一樣 |
| M3 第 1 段就做成 `dom` 的 GC 物件（Blink 的 `Animation` 是 GC 的） | 第 3 段不用改形狀 | 第 1、2 段沒有 script 看得到它，卻要付 GC 型別的成本（trace、`ObjectKind`、soundness 的論證）；recalc 時建 GC 物件要在 GC 的 scope 裡 | 每個 transition 多一個 GC 物件 |

### 動畫的值怎麼進 style（第 3 條）

css-cascade 4：transition 的 origin 在所有 `!important` 之上，animation 的 origin 在 author 的 normal 之上、`!important` 之下。

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **V1 cascade 的一個 origin（建議）**：`cascade_blocks` 找到每個 longhand 的勝出者之後，有動畫的 longhand 照 priority 換成動畫的值（已經是 computed value，套用時不再換算）；`font-size` 照現在的順序先套，`em`、`ex` 這類跟著動畫的 `font-size`；`!important` 的宣告擋得住 animation、擋不住 transition | 照規範和 Blink 的 `StyleCascade`；`currentcolor`、`em` 等依賴關係自然對 | cascade 多一個輸入；動畫的值要能「套用」成 computed value（每種值一個 apply 的路徑） | 每個 frame 對有動畫的元素多做一次 cascade 的套用（第 4 條決定不重新比對 selector） |
| V2 cascade 完之後直接改 `ComputedStyle` 的欄位 | 簡單 | `font-size` 的動畫不影響 `em`（WPT 的 `animate-font-size-with-margin-override.html` 這類）；`!important` 的優先順序要另外處理；繼承的 property 改了之後子孫拿到的是舊的值，要另外補 | 最小 |

### 每個 frame 怎麼重算動畫的元素（第 4 條）

現在重算一個元素一律連整個子樹重算。

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| R1 照一般的 recalc：標記元素，它和子樹重新比對 selector、cascade | 不用新的路徑 | `body` 上的 `color` transition、Complex-DOM 的大量 `all` transition 讓每個 frame 重算大片的子樹 | 每個 frame 是一次大的 recalc；Complex-DOM 的 `UpdateStyle` 本來就是瓶頸（RFC 0039） |
| **R2 照 Blink 的 base computed style（建議）**：動畫表記住元素沒有動畫時的 style（Blink 的 `ElementAnimations::base_computed_style_`），只有動畫在跑而元素本身沒有被標記時，每個 frame 只在 base style 上套動畫的值（V1 的套用那一步），不重新比對 selector；只有動畫的 property 是繼承的（`color`、`visibility`）時子樹才重算，否則子樹不動 | 每個 frame 的工作只和有動畫的元素數量有關 | base style 什麼時候作廢要對：元素本身、祖先（繼承）、stylesheet、viewport 改了都要走一般的路徑；多一份 `Arc<ComputedStyle>` | 和 Chrome 同一個量級 |

### 插值的範圍（第 5 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **I1 每個 property 都照規範的 animation type（建議）**：`LonghandId` 多一個 `animation_type()`（by computed value、discrete、not animatable，`properties.rs` 的一張 `match`），第 1 段做完 kun 有的每種值：數字、整數、長度和百分比（不同單位時混成 `calc()`）、顏色（Chrome 對舊式的顏色在 premultiplied sRGB 裡插值）、陰影的清單（長度不同時補 transparent 的陰影）、transform 的清單（函式對得上時逐項插值，否則照 css-transforms 1 的 2D 矩陣分解）、`visibility`、背景的 position 和 size、border-radius、其他 discrete（50% 時換） | `transition: all` 的結果和 Chrome 一樣；第 3 段的 `*-interpolation.html` 直接能跑；之後加 property 時有地方宣告它的 animation type | 第 1 段比較大（矩陣分解、`calc()` 的混合） |
| I2 只做 Speedometer 用到的（顏色、長度、`opacity`、`transform`、`box-shadow`），其他暫時 discrete | 第 1 段小 | 其他 property 在一半時跳過去，和 Chrome 不同、不易察覺；之後還是要補，到時要再找出哪些是暫時的 |

### 時間的來源和測試（第 6 條）

Blink 的 document timeline 的時間和 rAF 的時間是同一個（`AnimationClock`，來自 `BeginFrameArgs.frame_time`）。

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **C1 照 rAF 現在的時間（建議）**：一個 frame 一個 animation time，就是 rAF 拿到的那個值（main frame 開始時 renderer 的時鐘） | 動畫、rAF、timer 用同一個時鐘，`transitionend` 和 `setTimeout` 的先後和 Chrome 一樣；不改現有的行為 | 自動測試的結果受真實時間影響，要用負的 delay、長的 duration、`steps()` 讓結果固定（WPT 的做法）；測「剛好結束」要等真實的時間 |
| C2 改用 `BeginFrameArgs.frame_time`（Chrome），rAF 跟著改 | 和 Chrome 一樣；headless 的 frame time 是假的，測試可以決定時間 | headless 的 frame time 每次加 1000/60、和真實時間脫鉤，timer（真實時間）和動畫的先後在測試裡和視窗裡不同；rAF 的時間是另一個題目（已經記在 `docs/limitations.md`），混在這裡改會牽動 RFC 0012 的量測 |
| C3 C1 加上測試能換的時鐘（renderer 的 `Clock` 可以注入，測試一步步推時間） | 測試決定時間，結果固定 | renderer 的 `Clock` 現在是 `Instant`，注入要經過啟動 renderer 的參數（process 的邊界）；timer 也要跟著換才一致 |

### 怎麼分段（第 7 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **四段（建議）**：transition、`@keyframes`、JS API、compositor | 第 1 段做完共用的基礎（timing model、插值、origin、要 frame），第 2 段只加 `@keyframes` 的解析、查找和 iteration；每段都能先 merge，和同時進行的 RFC 0047（也動 `properties.rs`、`ComputedStyle`）衝突的範圍小 | 第 1 段結束時 `@keyframes` 照舊整條丟掉 |
| transition 和 `@keyframes` 合成一段 | 少一次 merge | 一段很大；Speedometer 用得多的是 transition，`@keyframes` 只有三個在跑 |

### keyframe 的值（第 9 條）

css-animations 1：keyframe 的值是在元素上算出來的 computed value（`var()`、`em`、`inherit` 都照元素），缺 0% 或 100% 的 property 用元素自己沒有動畫時的值（neutral keyframe）。

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **K1 style change 時在元素上 cascade 每個 keyframe（建議）**：每個 keyframe 的宣告當成 animation origin 的一個區塊，疊在元素的 cascade 上（`cascade_blocks` 已經能多收區塊），各得一份 `ComputedStyle`；缺的 0%、100% 是 base style 本身；新的 base style 和上次的一樣時（`ComputedStyle` 的 `PartialEq`）沿用上次的 keyframe，不重算；frame 時只找出現在落在哪兩個 keyframe 之間、照第 5 條插值 | `var()`、`em`、`currentcolor`、`inherit` 都對（Complex-DOM 的 `@keyframes` 用 `var()`）；frame 時不 cascade，只插值，和 transition 的路一樣 | 每個有動畫的元素每次 style change 多幾次 cascade；keyframe 多的動畫存幾份 style | 一次 style change 多 N 次 `cascade_blocks`（N 是 keyframe 數，Speedometer 的是 2 到 3）；frame 時和 transition 一樣便宜。Blink 存 specified value，在 style change 時轉成 `InterpolableValue`（`CSSInterpolationType::MaybeConvert*`，同樣在元素上），成本同一個量級 |
| K2 每條 `@keyframes` 規則只算一次，元素之間共用 | 最省 | 和元素有關的值都錯：`var()`、`em`、`inherit`、`currentcolor` | 最小 |
| K3 每個 frame 都重新 cascade 一次 keyframe | 不用存 keyframe 的 style | 每個 frame 每個有動畫的元素多幾次 cascade：Editor 的游標閃爍、spinner 都是 `infinite`，一直付 | 每個 frame 多 N 次 cascade |
| K4 照 Blink：存宣告值，第一次取樣時在元素上換算，連同依賴（`inherit` 的 parent、`em` 的 font-size、`var()` 的變數）快取，每次取樣先檢查依賴，變了才重算 | 只有依賴真的變了才重算；和 Chrome 的作廢時機一樣 | kun 的插值直接在 `ComputedStyle` 的欄位上做，cascade 也不記下一個值依賴什麼：要替每種值補上換算和依賴的記錄（Blink 的每個 `InterpolationType`、conversion checker），差不多重做一次第 5 條的插值 | 每次取樣多跑依賴的檢查（便宜）；style change 時多半不用重算 |

**Blink 的做法**：`CSSAnimations::CalculateAnimationUpdate` 在元素的 style resolve 時照 `animation-name` 找到 `@keyframes`（從元素的 tree scope 往外），建 `StringKeyframeEffectModel`：每個 keyframe（`StringKeyframe`）存宣告值（`var()`、`em` 還沒算）和自己的 easing，缺 0%、100% 的 property 是 neutral keyframe（元素沒有動畫時的值）。每個 property 相鄰的兩個 keyframe 是一個 `InvalidatableInterpolation`，第一次取樣時用 `CSSInterpolationType::MaybeConvertValue` 在元素的環境（`StyleResolverState` 的 base style、parent style）裡換成 `InterpolableValue`，結果連同它依賴什麼（conversion checker：`inherit` 看 parent 的值、`em` 看 font-size、`var()` 看變數）一起快取；之後每次取樣先跑 checker，依賴沒變就沿用，變了才重新換算，再插值、照 animation origin 寫進 style（`StyleCascade`，`font-size` 先套）。只有動畫在動、沒有依賴 font-size 這類的時候，走 base computed style 的捷徑，不重新 cascade（第 4 條的 R2）。

和 K1 比：都在元素上算，結果一樣，frame 時都只插值、不 cascade。差在快取什麼時候作廢：Blink 照每個值的依賴逐一檢查，沒變就一直沿用；K1 在元素的 base style 變了時把它的 keyframe 全部重算（2 到 3 次 `cascade_blocks`），沒變時（祖先改了不相干的、整個子樹重算）沿用，所以只有 base style 真的變了、動畫用到的值卻沒變時多做。形式上 Blink 每個 property 存一個 `InterpolableValue`，K1 每個 keyframe 存一整份 `ComputedStyle`（和第 1 段的 transition 一樣，插值直接沿用第 5 條，不用另外做每種型別的轉換）。之後量到 style change 時的 keyframe 重算是瓶頸，再改成照依賴作廢的快取。

### `@keyframes` 放在哪裡（第 10 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **R1 每個 tree scope 的 `RuleSet` 一張名稱的表，從元素的 tree scope 往外找（建議）** | 照 css-scoping 和 Blink（`ScopedStyleResolver::KeyframeStylesForAnimation` 從元素的 tree scope 往上找）；`@media`、`@supports` 和 sheet 的增減沿用 `RuleSet` 重建的時機 | `RuleSet` 多一張表；shadow tree 的 sheet 變了要讓用到它的動畫重算（和規則一樣，host 和 shadow tree 本來就重算） |
| R2 整份 document 一張表 | 簡單 | shadow tree 裡的 `@keyframes`（Perf-Dashboard 的元件的 spinner）找不到，或外面的找得到裡面的 |

### `@keyframes` 的 CSSOM（第 11 條）

Editor-CodeMirror 的 style-mod 在有 `adoptedStyleSheets` 時建一份 constructed stylesheet，`for (…) sheet.insertRule(mod.rules[k], pos++)`，沒有 try/catch；它的規則有 `@keyframes cm-blink { 0% {} 50% { opacity: 0 } 100% {} }` 和 `cm-blink2`，游標用 `animation: steps(1) cm-blink 1.2s infinite`，選取改變時在 `cm-blink` 和 `cm-blink2` 之間換 `animationName` 讓它重新開始。kun 現在的 `insertRule()` 對 `@keyframes` 丟 `SyntaxError`（`@media`、`@supports` 以外的 at-rule 都是），這個迴圈在那裡停下。

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **C1 第 2 段做基本的：`insertRule()`、`replace()` 收 `@keyframes`，`CSSKeyframesRule` 的 `name`、`cssRules`、`cssText`，`CSSKeyframeRule` 的 `keyText`、唯讀的 `style`（建議）** | CodeMirror 的規則都插得進去、動畫找得到；`cssRules` 裡的規則數和 Chrome 一樣（`insertRule` 的 index 才對得上） | 多兩個 interface（`ObjectKind`、`CssRuleKind`、WIT）；改規則的方法另外再做 |
| C2 整個 CSSOM（加上 `appendRule()`、`deleteRule()`、`findRule()`、改 `name`、`keyText`、`style` 可寫） | 一次做完 | 改了規則要讓用到它的動畫重算，第 2 段變大；Speedometer 用不到 |
| C3 照原本放在第 3 段，第 2 段的 `insertRule()` 收下但 `cssRules` 裡沒有它 | 第 2 段小 | `cssRules.length` 和 `insertRule` 的 index 和 Chrome 不同，style-mod 之後插入的位置會錯 |

### 第 3 段的範圍（第 13 條）

用到的地方：WPT 的 `css/support/interpolation-testcommon.js`（各 property 的 `*-interpolation.html`，約 340 個檔案）的 Web Animations 的那一組用 `target.animate([{offset: 0, composite, [property]: from}, …], {fill: 'forwards', duration, easing})`、`pause()`、`currentTime =`；`css-animations` 和 `css-transitions` 的 `*.tentative.html`（104 個檔案用 `getAnimations()` 或 `animate()`）用 `getAnimations()`、`CSSAnimation`、`CSSTransition`、`currentTime`、`startTime`、`playbackRate`、`finish()`、`ready`、`finished`、`effect.getComputedTiming()`、`effect.getKeyframes()`；`web-animations/`（226 個檔案）是整個 API。Speedometer 的盤點沒有用到（`rfcs/0012-speedometer/`）。

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **W1 web-animations 1 的主體（建議）** | 上面三類測試幾乎都跑得起來；`playbackRate`、`reverse()`、seek 讓 2c 的事件表往回走的那幾格真的走得到（現在只有 timing 改了時）；被取代的動畫自動移除免得 `animate({fill: 'forwards'})` 一直累積（Chrome 也做） | 介面多（`Animation` 的狀態機、兩種 promise、`AnimationPlaybackEvent`、`commitStyles()` 要把值寫回 inline style） | 只有用到的頁面付；每個 frame 多處理 pending 的 play、pause 和 promise |
| W2 常用的子集 | 小一半 | `getComputedTiming()`、`updateTiming()`、`setKeyframes()`、建構子沒有，`web-animations/` 大多跑不起來；之後補時還要再改模型的邊界 | 同上 |
| W3 只讀 | 最小 | interpolation 的 340 個檔案的那一組照樣失敗，`getAnimations()[0].pause()` 這種常見寫法也不行 | 最小 |

Chrome 都有（`core/animation`：`Animation`、`KeyframeEffect`、`DocumentTimeline`、`CSSAnimation`、`CSSTransition`、`ElementAnimation::animate`）。

### 內部的模型（第 14 條）

| 做法 | 好處 | 壞處 | 效能和成本 |
| --- | --- | --- | --- |
| **M1 合成一種 `Animation` 帶 `KeyframeEffect`，CSS 的兩種是它的 owner（建議）** | 和 Blink 一樣（`Animation` + `KeyframeEffect`，`CSSTransition`、`CSSAnimation` 是子類別）：JS 對 CSS 動畫的 `pause()`、`currentTime`、`playbackRate`、`cancel()` 和 script 的動畫是同一套；事件、pending、promise 一個地方做；composite order 照 class（transition、CSS animation、script）排一次；2b 的 `KeyframeEffect` 直接沿用，transition 是兩個 keyframe 的 effect | 要改寫 1c、2b 的 `Transition` 和 `CssAnimation` 的結構（規則不變），它們的 mutation spec 要跟著改、重跑 | 一次 style change 和 frame 的成本不變；表多一個 id 的索引 |
| M2 三種各自的結構 | 不動已有的 | pause、seek、playback rate、promise、事件要寫三份，`getAnimations()` 的排序要把三種湊在一起 | 同上 |

### JS 物件和動畫的關係（第 15 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **G1 GC 物件只存 id，狀態在動畫表（建議）** | style、renderer 照舊不碰 GC 的 heap（第 2 條的 M1：表在 `DomState`、以 id 為 key）；和外部 engine 的 store 一樣的生命週期（holder 只有 id，`prune` 清掉）；`unsafe` 不變 | 沒有 target、已經 idle 的動畫只有 script 拿著時，要靠「物件被回收」才知道可以清掉：id 到物件的弱表（`WeakSet` 的 `WeakEdge`），frame 時清掉物件已經不在的 |
| G2 動畫整個在 GC 的 heap 上（Blink） | 生命週期由 GC 決定，不用另外清 | style recalc 和 renderer 要經過 GC 的 heap 讀寫動畫（現在的設計刻意避開：read scope 裡改 GC 物件的狀態、`NoTrace` 的論證要重寫），`soundness.md` 的範圍變大 |

Blink：`Animation` 是 GC 物件（`ElementAnimations::animations_` 是 `HeapHashCountedSet<WeakMember<Animation>>`，`DocumentTimeline` 也用 weak 的集合），沒有 JS 和 effect 指著時被回收；CSS 的動畫由 `CSSAnimations` 的 `running_animations_` 留著。

### `kun:dom`（第 16 條）

`animate(keyframes, options)` 的 `keyframes` 是 `object?`（陣列或 property-indexed 的物件，key 是 property 的名稱），`getKeyframes()` 回傳 `sequence<object>`：宣告表的規則裡 `object` 只給 JS（`wit::is_js_only`）。

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **K1 keyframe 的四個成員只給 JS，其他 JS 和 wasm 都給（建議）** | 照現在的規則，不用新的型別；guest 照樣能 `getAnimations()`、控制 CSS 的動畫 | guest 不能從 script 建動畫（可以用 CSS 的） |
| K2 另外給 wasm 一個降階的 keyframe（offset、easing、（property，值）的清單） | guest 也能 `animate()` | 多一個只給 WIT 的形式和它的轉換、golden file；Lua 等 runtime 的 binding 跟著長 |
| K3 第 3 段只給 JS | 最小 | 和宣告表的規則（能給 WIT 的都給）不一致 |

### `ready` 和 `finished` 的 promise（第 18 條）

`animation.ready === animation.ready`、`await animation.finished` 之後再讀 `finished`、`ready` 被新的取代：WPT 的 `web-animations/interfaces/Animation/ready.html`、`finished.html` 和多數 `*.tentative.html` 都依賴同一個物件。kun 的 promise 現在由 engine 照編號留著到 settle 並交出為止：`bindings` 的 `hand_out` 在 settle 之後交出時就把它從表裡拿掉，wasm 的 guest 只能等還沒 settle 的。`customElements.whenDefined()` 每次回同一個 promise 沒問題，是因為它 settle 之後就不再被要。

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **P1 engine 多一種留著的 promise（建議）** | 和 Blink 的 `ScriptPromiseProperty` 一樣：同一個物件、settle 前後都一樣；JS 和 wasm 的行為一致；之後 `FontFaceSet.ready`、`navigator.serviceWorker.ready` 這類也用得上 | `ScriptHost` 多兩個函式，`bindings`、`dom_js`、`dom_wasm` 各改一處；留著的 promise 要記得 release（換新的、動畫清掉時），否則 realm 的表一直長 |
| P2 JS 的前端把 promise 快取在 wrapper 上 | engine 不動 | 前端要認得哪些 attribute 要快取（另一種 `[CachedAttribute]`）；wasm 的 guest 拿到的是每次新建、當場 settle 的另一個 promise，兩邊的行為不同；`dom_api` 要知道 settle 過了沒，還要分辨呼叫它的是哪個 engine |

### 離開 CSS 規則的動畫放在哪裡（第 19 條）

3a 原本把離開元素的動畫（取消、結束、被取代的）搬到 `dom` 的 `detached`：script 拿著物件時還能播，但值不套到元素、不再發 CSS 的事件。WPT 的 `css/css-transitions/event-dispatch.tentative.html`（結束之後 seek 回去、取消之後過幾個 frame 再重播）和規範都不是這樣：css-transitions 2 只在 style 取消時斷開 owning element，script 的 `cancel()` 不斷開；斷開之後只是不發 CSS 的事件，effect 的 target 還是那個元素。

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **R1 留在元素上（建議）** | 和規範、Blink 一樣：值照樣套、有 owning element 的照樣發事件；3b 的 `element.animate()` 的動畫直接放進同一份清單 | 改 `ElementAnimations`、style 的取消、`dom` 的 `prune`；style 套值時多走一份清單 |
| R2 維持 `detached` | 現在改得少 | 和規範不同，記成限制；3b 一樣要回頭改同一塊，那之前幾個 WPT 都失敗 |

### `KeyframeEffect` 放在哪裡（第 20 條）

effect 可以不屬於任何動畫（`new KeyframeEffect(...)`），可以換給另一個動畫（`animation.effect = effect`，原本的動畫失去它），`target` 和 `pseudoElement` 可以改（動畫跟著搬到另一個元素）。CSS 動畫的 effect 也是物件（`CSSAnimation.effect`、`getKeyframes()`）。Blink 的 `KeyframeEffect` 是自己的 GC 物件，`Animation` 指著它；`ElementAnimations` 照 effect 的 target 記動畫。

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **E1 effect 在它的動畫裡，沒有動畫的另外一張表（建議）** | 現在的 `Animation` 已經帶著 effect，style 和 renderer 不用改讀法；JS 的 `KeyframeEffect` 物件照第 15 條只放 effect 的編號；換 effect 時從一邊搬到另一邊；改 target 時動畫搬到另一個元素的 `ElementAnimations`（沒有 target 的放在 document 的一份清單）；effect 一次只屬於一個動畫，不會有重複的狀態 | 找一個 effect 要先掃動畫、再查那張表（成員呼叫時多一次線性的找，之後可以加索引）；換 effect、改 target 的搬家要連同排好的事件和 transition 的 completed 紀錄，路要寫對（有 mutation test 守）；沒有動畫的 effect 物件被回收後要從表裡清掉；effect 的 target 和動畫的 owning element 要分開記（改了 CSS transition 的 target，值套到新的元素、事件照樣在原本的元素，E2 也要） |
| E2 effect 都放在一張表，動畫存它的編號 | 和 Blink 的指標關係最像 | style 每次套值都要多查一次表，`ElementAnimations` 的清單和 effect 的 target 兩邊都要同步 |

### script 給的 keyframe 什麼時候算（第 21 條）

`animate()` 的 keyframe 是 property 和字串（`{ width: ['0px', '10em'], easing: ... }`），值可以是 `var()`、`em`、`inherit`、`currentcolor`。Blink 的 `StringKeyframe` 存 CSS 的值，每次 style 時照 target 的 style 解出來。

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **V1 存宣告，在 target 的 style recalc 時算（建議）** | 和 Blink、和 CSS animation 的 keyframe 一樣：`em`、`var()`、`inherit` 跟著元素變；直接用 CSS animation 那一套（`KeyframeSource` 的快取、`Restyle::with_keyframe`）；`getKeyframes()` 回傳寫的值 | keyframe 要在 style recalc 裡算，script 剛建的動畫要等 style 更新（`getComputedStyle` 本來就會） |
| V2 設的時候照 target 當下的 style 算一次 | 簡單 | 之後元素的 `font-size`、變數改了，值不跟著；換 target 要重算；和規範不同 |

### `timeline` 能不能設成 `null`（第 22 條）

3a 的 `timeline` 是唯讀的（`spec-differences.txt`）。WPT 的 `web-animations/` 和 `event-dispatch.tentative.html` 有不少用 `timeline = null` 和 `new Animation(effect, null)`：沒有 timeline 的動畫不前進，current time 只有 hold time。`Playback` 的程序本來就收 `Option<f64>` 的 timeline 時間。

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **T1 可以設成 `null` 或 document 的 timeline（建議）** | 程序本來就照 web-animations 1 處理沒有 timeline 的情況；那些 WPT 能跑；`spec-differences.txt` 少一行 | 動畫要記自己有沒有 timeline，tick 和 pending 的工作跳過沒有 timeline 的 |
| T2 維持唯讀 | 不用改 | 那些 WPT 照樣失敗；`new Animation(effect, null)` 要丟錯或忽略 `null`，和規範不同 |

### 第 3 段怎麼分（第 17 條）

3a 先把模型換掉（M1），JS 讀到的是已經在跑的 CSS 動畫，`*.tentative.html` 多半在這一段就能跑；3b 才有 script 建的動畫和 keyframe 的轉換（`interpolation-testcommon.js` 的那一組）；3c 是 CSSOM 的補齊和 WPT。三段的介面在 3a 定下來，之後只加東西。

### 第 4 段的範圍（第 23 條）

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| C1 照 Chromium 全做 | 有動畫的 transform、opacity 是 compositing reason；compositor 的 transform node 是矩陣、多 effect tree；Viz 的 `quad_to_target_transform`、render surface；兩個 renderer 都支援；impl thread 的 `AnimationHost`、`KeyframeModel` | JS 忙時照樣動、不再每個 frame re-raster；和 Chromium 同構 | 最大，大約四段；軟體 renderer 的逆矩陣取樣和邊緣的 AA、render surface 的離屏繪製 |
| C2 先做單一 layer | 同 C1，不做 render surface：opacity 只在動畫的元素整個是一個 layer、裡面沒有別的 layer 時交出去 | 常見的淡入淡出、平移縮放都交出去；約 C1 的六成 | 有巢狀 layer 的 opacity 動畫照舊在 main thread |
| **C3 這份 RFC 不做（選了）** | 第 4 段移到 RFC 0049，這份做到第 3 段 | 不動 compositor 和 Viz 的架構，這份 RFC 可以收尾；Speedometer 用不到 | 動畫照舊在 main thread、每個 frame re-raster，JS 忙時會停 |

## 和 RFC 0047 同時做

RFC 0047（table）在另一台裝置上同時進行（2026-10-15）。兩份都在路線圖的 CSS 線上，但主要改的地方不同：0047 是 `layout`（table formatting context、`collect` 的 fixup）、`paint`（table 的背景層次）、`style` 的 `hints.rs` 和 UA stylesheet；這份是 `style` 的 recalc 和 cascade、`css` 的 `stylesheet.rs` / `rule_set.rs`、renderer 的 rendering update、`dom` 的動畫表和事件；compositor 的動畫才會動到 `paint` 的 `layerize.rs`、`compositor`、`viz`，後來移到 RFC 0049（第 23 條）。

都會改的檔案：`css` 的 `properties.rs`（兩份都加 longhand：`LonghandId`、`ALL`、`from_name`、`name`、`parse_value`、`apply`、`copy`）、`serialize.rs`、`computed.rs`，和 `computed_style` 的 `ComputedStyle`、`initial()`。都是各自往清單裡加，後 merge 的一方 rebase 時解衝突，`LonghandId::ALL` 的順序照 `all_lists_every_longhand_at_its_index` 的檢查重排。這份第 1 段的 `animation_type()` 要涵蓋所有的 longhand，0047 先 merge 的話，它的五個 property 在 rebase 時補上（`border-spacing` 能插值，其他 discrete）。
