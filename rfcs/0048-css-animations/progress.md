# RFC 0048 的進度：transition 和動畫

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-15。完成：2026-10-16，Windows 11（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0048` 上做，每段做完時先 merge 回 `main`（由人審核）；第 4 段移到 RFC 0049（第 23 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a `transition-*` 的 property 和 easing：解析、序列化、computed value、`getComputedStyle` | 完成 | 2026-10-15 |
| 1b 每個 longhand 的 animation type 和插值 | 完成 | 2026-10-15 |
| 1c timing model、transition 的規則（`animation` crate）、`dom` 的動畫表 | 完成 | 2026-10-15 |
| 1d recalc 開始和取消 transition、cascade 的 transition origin、base computed style | 完成 | 2026-10-15 |
| 1e renderer 的「update animations and send events」、要 frame、`TransitionEvent`；`about:demo`、情境、WPT | 完成 | 2026-10-15 |
| 2a `animation-*` 和 shorthand、`@keyframes` 的解析、`RuleSet` 和查找、CSSOM | 完成 | 2026-10-15 |
| 2b CSS animation、keyframe 的值、cascade 的 animation origin | 完成 | 2026-10-15 |
| 2c `AnimationEvent`、renderer、`about:demo`、情境、WPT | 完成 | 2026-10-15 |
| 3a 模型合成一種 `Animation`、`document.timeline`、`getAnimations()`、`Animation` 的控制、promise 和事件、`CSSAnimation`、`CSSTransition` | 完成 | 2026-10-16 |
| 3b-1 `KeyframeEffect`、`Element.animate()`、`new Animation()`、`effect` 和 `timeline` 的 setter | 完成 | 2026-10-16 |
| 3b-2 被取代的動畫自動移除（`replaceState`、`onremove`、`persist()`）、`commitStyles()` | 完成 | 2026-10-16 |
| 3c `@keyframes` 的 CSSOM 補齊、WPT 的 `web-animations/` 和 `*-interpolation.html` | 完成 | 2026-10-16 |
| 4 compositor 的動畫 | 移到 RFC 0049（第 23 條） | 2026-10-16 |

## 驗收

### CSS 的 property 和插值

| 項目 | 怎麼驗證 |
| --- | --- |
| `transition-*` 和 `transition` 的解析、宣告值和 computed value 的序列化照 Chrome（WPT 的 `css/css-transitions/parsing/`、`css/css-easing/` 的預期）：時間照寫的單位、computed 是秒，`calc()` 的時間；easing 的關鍵字、`cubic-bezier()`、`steps()`（`start` 和 `jump-start` 分開寫）、`linear()`（computed 補上每個 input）；`transition-property` 的名稱照寫的留著；shorthand 省掉初始值、有 delay 時留著 duration，長度不同的清單寫不成 shorthand；不繼承 | `cargo test -p style --test it transitions::` |
| easing 的計算：`cubic-bezier()` 和關鍵字（範圍外照端點的切線、兩個控制點都在端點時斜率是 1）、`steps()` 的每種 position 和 before flag、`linear()`（範圍外延伸頭尾的線段、同一個 input 的兩點、stop 在 `f64` 比較）；數字照寫的 `f64` | `cargo test -p style --test it transitions::cubic_bezier_easing transitions::step_easing transitions::linear_easing` |
| `animation-*` 和 `animation` 的解析、序列化照 Chrome（WPT 的 `css/css-animations/parsing/` 的預期）：名稱是 identifier 或字串（keyword 寫成字串）、`auto` 的 duration、`infinite`；shorthand 照 Blink 的順序分給 longhand、省掉初始值、名稱會被讀成別的值時把那個值也寫出來；computed value；不繼承 | `cargo test -p style --test it animation_properties::` |
| 每個 longhand 的 animation type（`display` 和 `transition-*` 不動畫，discrete 的、照 computed value 插值的）；插值：長度和百分比（混成 `calc()`；`0%` 和長度混成 `calc()`，只有 `translate()` 的零長度也是零的百分比）、`auto` 對長度是 discrete 的一步、數字和整數（半數往上）、超出範圍時夾回 property 的範圍、顏色（premultiplied，`currentcolor` 照各自的 `color`，兩邊都是時留著關鍵字）、`visibility`、陰影（補透明的、`inset` 不同時一步）、清單（重複到最小公倍數）、`background-position` 從尾端算的、`background-size` 一樣的 keyword 留著、`letter-spacing` 的 `normal` 在起點、`border-spacing`、`transform`（函式對得上時逐項，否則各自的矩陣分解後插值，旋轉走短的一邊，百分比時一步）、flow-relative 的照 physical 的 | `cargo test -p style --test it interpolation::` |

### timing model 和動畫的規則（`animation` crate）

| 項目 | 怎麼驗證 |
| --- | --- |
| timing model：phase 和 active time、fill、負的 delay、iteration 和 direction、iteration start、duration 0、easing 的 before flag | `cargo test -p animation --test it timing::` |
| transition 的規則：style change 開始一個（after-change style 的 `transition` 算數，值一樣、時間加起來不到 0、不能插值時不開始，`allow-discrete` 的一步），pending 到畫出它的 frame，delay 之前停在開始的值，完成之後同樣的結束值不再開始；新的結束值取代、從目前的值開始；回到開始的值時照 reversing shortening factor 縮短；不再列出、回到目前的值、duration 變 0 時取消；`all`、shorthand、flow-relative 的名稱和最後一個符合的項目；事件（run、start、end、cancel）的 elapsed time 和順序，沒 tick 過的不發 cancel | `cargo test -p animation --test it transitions::` |
| 事件的順序：時間、transition 在 CSS animation 之前、建立的順序 | `cargo test -p animation --test it events::` |

### style recalc 和 `@keyframes`

| 項目 | 怎麼驗證 |
| --- | --- |
| transition：class 改了開始 transition、frame 時套到 base style（子樹不重算）、別的原因重算時 transition 繼續；`display: none`（自己或祖先）取消、從沒畫出來的 style 不開始；繼承的 property 的值往下傳（子孫和 pseudo-element 每個 frame 重算）；`font-size` 的 transition 讓 `em` 跟著；pseudo-element 自己的 transition，沒有 `content` 或規則不再符合時取消；transition 蓋過 `!important`；只有 delay 也開始 | `cargo test -p style --test it animations::` |
| `@keyframes`：名稱、selector（`from`、`to`、百分比、`calc()`，不合的 keyframe 丟掉）、去掉 `!important` 和 `animation-*`、同名的後面的勝出、`@media` 和 `@supports` 把關、從元素的 tree scope 往外找（元素自己的 shadow tree 先）、`cssText` 和 `keyText`、`insertRule` | `cargo test -p style --test it keyframes::` |
| CSS animation：和元素一起開始、缺的 keyframe 是元素自己的值、`animation: none` 和 `display: none` 取消、沒有 `@keyframes` 不開始；delay 和 fill、iteration 和 direction、每個 keyframe 的 easing、keyframe 的 cascade（同 offset 後面的勝出、logical property）；keyframe 在元素上算（`var()`、`em`、`inherit`），沒變時沿用、base style、parent、root 的 font size、viewport 變了重算；animation origin（擋不住 `!important`、蓋過 style attribute）；動畫中的 property 不 transition；名稱清單改了留著還在的、同名的從尾端配對；暫停和繼續；子孫和 `em`；pseudo-element 和 `::marker`；shadow tree 的 `@keyframes`；事件（每種 phase 的變化、一個 frame 跨好幾圈、負的 delay、暫停的、timing 改了不重發、取消和它的 elapsed time） | `cargo test -p style --test it css_animations::` |

### renderer 和事件

| 項目 | 怎麼驗證 |
| --- | --- |
| transition：script 讀到中途的值（開始時、兩個 frame 之後、200 ms 之後）、頁面閒置之後的第一個 transition 照樣從頭開始、`IntersectionObserver` 看得到中途的 frame；事件（run、start、end）的種類、elapsed time、target、冒泡、`isTrusted`、`TransitionEvent`；一個 frame 裡的事件照排定的時間、再照 transition 的順序；隱藏、移除、搬到別的 parent 時 `transitioncancel`，搬過去之後新的值立刻生效、之後的改變照樣 transition；結束時 style 就是結束的值，接著的 transition 從那裡開始；pseudo-element 的事件發在它的元素上、帶 `pseudoElement` | `cargo test -p shell --test it transitions::` |
| CSS animation：動畫自己要 frame 跑完；`animationstart`、`animationiteration`、`animationend` 的 elapsed time、`AnimationEvent`、冒泡、可取消、`isTrusted`；暫停的照樣開始、名稱拿掉和移除時下一個 frame 發 `animationcancel`；pseudo-element 的發在元素上 | `cargo test -p shell --test it css_animations::` |
| Web Animations 的真的 frame：`ready` 在開始的 frame resolve，`finished` 在發事件之前，同時間的事件照 tree order，取消的事件，`detached` 的動畫要 frame 跑完；`animate()` 跑到結束（`ready`、中途的值、`finished`、`finish`、沒有 fill 時回到 base）、`new Animation(new KeyframeEffect())` 的 `fill: forwards` 和換 target；`finish` 的事件在被取代的動畫的 `remove` 之前、移除之後 `commitStyles()` | `cargo test -p shell --test it web_animations::` |
| `TransitionEvent`、`AnimationEvent` 的建構子、成員和預設值、`ontransitionend`、`onanimationcancel`（JS 和 wasm） | `cargo test -p dom_js --test it bindings::transition_events bindings::animation_events`、`cargo test -p dom_wasm --test it engine::a_guest_makes_animation_events` |

### Web Animations 的 JS API

| 項目 | 怎麼驗證 |
| --- | --- |
| playback 的程序（web-animations 1）：play、pause、finish、cancel、reverse（不給 -0）、`currentTime` 和 `startTime`、`playbackRate`、`updatePlaybackRate`（沒有 current time 時立刻生效），pending 的工作在 frame 開始，finished 的狀態（到 end 差一微秒以內算到了，`finish()` 之後同時的 `cancel` 在 `finish` 之後）、rate 0 的動畫在 before phase 不算 current；錯誤（`InvalidStateError`、`currentTime = null` 的 `TypeError`） | `cargo test -p dom_js --test it animations::` |
| script 的物件：`CSSTransition`、`CSSAnimation`，同一個動畫同一個物件、拿得到時由 document 留著；`document.timeline`（第一個 frame 之前是 frame 的時鐘）；`getAnimations()` 的 composite order、`subtree`（不進 shadow tree）、`ShadowRoot.getAnimations()`；`ready` 和 `finished` 每次同一個、style 換掉、取消時拒絕（不算沒處理的 rejection）、停下來之後交還；`finish`、`cancel`、`remove` 的 `AnimationPlaybackEvent`；`animation-play-state` 被 `play()`、`pause()`、`reverse()`、`startTime` 取代；離開 CSS 規則的動畫留在元素上（值照樣套、有 owning element 的照樣發事件），整份拿掉的照樣能播；CSS 事件的表在取消和重播之後 | `cargo test -p dom_js --test it animations::` |
| `KeyframeEffect`：建構子（`options` 沒給是 `{}`）、`getTiming()`、`getComputedTiming()`（方向、往回走時在開頭是 before、`startTime`）、`updateTiming()` 和它的檢查（一項錯整個不改）、`target` 和 `pseudoElement`（`:before`、`::marker`，換 target 時 script 的動畫跟著搬、舊的元素不再有值）、`getKeyframes()`、`setKeyframes()`；keyframe 參數（清單和 property-indexed、offset 的平均分配、`computedOffset` 照「compute missing keyframe offsets」、合併和排序、`offset`、`easing` 和 `composite` 的清單都檢查、`composite` 重複套用、不能動畫的 property 略過、只讀 IDL 的名稱、值讀回 serialize 過的、錯的 offset、easing、composite 和不是物件的 keyframe）；script 的 keyframe 在元素上算（`em` 跟著）、改了自己重新 style、step easing 照 before flag | `cargo test -p dom_js --test it animations::` |
| `element.animate()`（`id`、`timeline: null`）、`new Animation(effect, timeline)`、`effect` 的 setter（舊的變成沒有動畫的、新的離開原本的動畫）、`timeline` 的 setter（`null` 時 hold time 清掉、CSS 的 cancel 事件、給回 timeline 再播從頭發事件）；`updateTiming()` 讓結束的動畫再跑；CSS animation 被 script 改過的 keyframe、timing 的成員和換掉的 effect 不再被 style 改；effect 的物件拿著它的動畫；script 的和 style 不再管的動畫照 global animation list 排；script 的動畫算進 before-change 和 after-change style（它蓋住的 property 不 transition），script 的 effect 的成員不更新 style；元素不再 render、搬動過時 script 的動畫留著 | `cargo test -p dom_js --test it animations::` |
| 被取代的動畫：結束、填著、後面的動畫蓋住全部 property 時移除（`remove` 和它的時間、不在 `getAnimations()` 裡、不套值，取代它的不在了也一樣，script 放掉之後清掉）；部分蓋住、不在 effect 裡、markup 的 CSS animation 和 transition、`persist()` 的不移除；style 不再管的 CSS animation 重播之後會；`persist()` 讓移除的再套值 | `cargo test -p dom_js --test it animations::` |
| `commitStyles()`：composite order 到它為止的 computed value（`em` 成 px）寫進 inline style、被移除的連同其他被移除的、在 delay 裡時是它下面的值、CSS animation 的、結束之後是最後的值（endpoint-inclusive）；pseudo-element、不在 render 的（`display: contents` 算）、沒有 target | `cargo test -p dom_js --test it animations::` |
| wasm：`get-animations`、`css-transition`、play state、`pending`、current time、pause 和 seek、`finish`、已經 settle 的 `finished`、timeline；transition 的 effect 的 timing、`update-timing`、computed timing、`new Animation(null, null)`、拿走 effect 和 target、設 timeline、`replace-state`、`persist`、`commit-styles` | `cargo test -p dom_wasm --test it engine::a_guest_uses_web_animations engine::a_guest_uses_animation_effects` |
| `KeyframeEffect` 的 GC 型別：slot 留著它、只讀回自己的型別、放掉之後回收、別的 DOM 的 slot 不收；`DocumentTimeline` | `cargo test -p dom --test it script_objects:: script::` |
| 宣告表：interface 清單、和規範的 WebIDL 比對、`kun:dom` 的 WIT | `cargo test -p dom_api --test it api:: spec:: wit::` |

### `@keyframes` 的 CSSOM

| 項目 | 怎麼驗證 |
| --- | --- |
| `CSSKeyframesRule`（`name` 和它的 setter（任何字串）、`cssRules`、`length` 和 index 的 getter、`appendRule()`（剛好一個 keyframe）、`findRule()` 和 `deleteRule()`（全部 selector 一樣的最後一個））、`CSSKeyframeRule`（`keyText` 和它的 setter、`SyntaxError`，可以改的 `style`：物件不變、規則換新的），`type`，JS 和 wasm；動畫跟著改了的 `@keyframes` | `cargo test -p dom_api --test it css_style_sheet::`、`cargo test -p dom_js --test it style_sheets::keyframes_rules animations::keyframes_changed_by_script`、`cargo test -p dom_wasm --test it engine::a_guest_reads_keyframes` |

### WPT 和 `about:demo`

| 項目 | 怎麼驗證 |
| --- | --- |
| WPT：`css/css-transitions/`、`css/css-animations/`、`css/css-easing/`、`web-animations/` 整個，各個 property 的 `*-interpolation.html`（用 `interpolation-testcommon.js` 的 336 個檔案）；失敗的 `.ini` 寫了原因 | `python3 scripts/test.py wpt css/css-transitions/ css/css-animations/ css/css-easing/ web-animations/ <清單裡的 *-interpolation.html>` |
| 在視窗裡：`about:demo` 的「Move the bar」讓長條變寬、變綠（按下之後第二個 frame 在中間），兩個 `transitionend` 到了 note，再按一次回去 | `python3 scripts/test.py scenarios transitions` |
| 在視窗裡：`about:demo` 的 spinner 轉、數到 `animationiteration`，暫停時停住、再按繼續；「Reverse the spinner」讓它反著轉、再按轉回來 | `python3 scripts/test.py scenarios css-animations` |

## 做過的檢查

- **做了什麼**（第 1 到 23 條）：
  - `computed_style`、`css`：`transition-*`、`animation-*` 和兩個 shorthand 的解析、computed value、序列化（`calc()` 認得時間）；easing（`EasingFunction`，`cubic-bezier()` 的控制點和 `linear()` 的 stop 照寫的文字讀成 `f64`）；`interpolate` 模組（每個 longhand 的 `animation_type`、每種 computed value 的插值、2D 矩陣的分解）；`@keyframes`（`CssRule::Keyframes`、`CssRule::Keyframe`，`RuleSet` 照名稱收、`StyleEngine::find_keyframes` 照 tree scope 往外找）和它的 CSSOM 的修改。
  - 新的 `animation` crate（第 8 條）：timing model（`Timing`、`ComputedTiming`、`compute_inclusive`）、`Playback`、`Animation`（`Owner`、owning element、`global_order`、`ReplaceState`）、`KeyframeEffect`（`DeclaredKeyframes`）、transition 和 CSS animation 的規則、事件和它的排序、`ElementAnimations`（`apply`、`apply_up_to`、`remove_replaced`、`cancel_all`、`cancel_markup`）。
  - `style`：`Animator`（style change 時開始、取消、套值；frame 時對 base style 取樣）；`CascadePriority` 的 animation 和 transition origin；keyframe 在元素上 cascade（`Restyle`）。
  - `dom`：每份 document 的動畫表（`Animations`：timeline、`ElementAnimations`、base style、`removed`、`detached`、沒有動畫的 effect、`timing_inputs`、等著發的事件、`prune`），`StyleFlags::animations_changed`；GC 型別 `Animation` 的物件、`KeyframeEffectObject`、`DocumentTimeline`；事件的資料（`TransitionEventData`、`AnimationEventData`、`AnimationPlaybackEventData`）。
  - `dom_api`：`TransitionEvent`、`AnimationEvent`、`AnimationPlaybackEvent`、`Animation`、`CSSAnimation`、`CSSTransition`、`AnimationEffect`、`KeyframeEffect`、`AnimationTimeline`、`DocumentTimeline`、`getAnimations()`、`animate()`、`CSSKeyframesRule`、`CSSKeyframeRule`；`ScriptHost::create_kept_promise`、`read_keyframes`（`bindings`、`dom_js` 的 `keyframes.rs`、`dom_wasm` 的 kept slot）。`kun:dom` 每次都只新增、不升版，`ci.py guests` 重編 guest；Lua 的 generator 多了 `option<f64>` 和 variant 的 `f64` 的回傳值。
  - renderer：`update_animations_and_send_events`（timeline、tick、settle promise、移除被取代的、發事件）、frame 的 lifecycle 之後定 pending 的開始時間、要 frame 的條件、交還 promise 和放掉 script 拿不到的動畫。
  - `about:demo` 的「Move the bar」、spinner、「Pause the spinner」、「Reverse the spinner」；情境 `transitions`、`css-animations`，`demo-script` 的按鈕清單。
  - 文件：`docs/limitations.md`（transition 和 CSS animation、Web Animations 的那幾條，事件、`linear()` 的 `f32`、還沒查的幾個 WPT），`docs/architecture.md`（`animation` crate，`css`、`style`、`dom`、renderer 的列），`docs/testing.md`（不穩定的測試、只靠審查、等價的 mutation、手動確認）。
- **和原本的計畫不同的**：
  - transition 的開始時間：原本用 style change 時 timeline 的時間，頁面閒置之後第一次按「Move the bar」時長條直接到底（使用者 2026-10-15 在視窗裡看到）；照 Blink 的 `PendingAnimations` 改成第一個畫出它的 frame 的時間，timeline 每個 frame 都更新。pending 的照 css-transitions 2 發 `transitionrun`（WPT 的 `transitioncancel-002.html` 抓到）。修好之後使用者在視窗裡確認長條照 transition 變長。
  - 3a 的模型改成一種 `Animation`（第 14 條），第 1、2 段的 mutation spec 跟著改、重跑；3a 做到一半把離開 CSS 規則的動畫留在元素上（第 19 條，WPT 的 `event-dispatch.tentative.html` 抓到）。
  - `@keyframes` 的基本 CSSOM 從第 3 段提前到第 2 段（第 11 條，Editor-CodeMirror 的 `insertRule()`）。
  - 第 4 段（compositor 的動畫）移到 RFC 0049（第 23 條）。
- **找到並修的**（自己的測試和 WPT 抓到的；細節在 README 對應的段落）：
  - 插值：`translate(10px)` 的 y 是 `0%`，混成了 `calc()`、進不了矩陣（只在 `translate()` 把零的長度當成百分比；第一版套到所有 `<length-percentage>`，`background-position`、`text-indent` 的 124 個子測試失敗）；轉 0° 的矩陣的 sine 是 -2e-16；`vertical-align` 的 `baseline` 對長度；`cubic-bezier()` 的控制點和 `linear()` 的 stop 存成 `f32`（色彩 channel 剛好 .5 時捨去）；兩個控制點都在端點時的斜率；`background-size` 一樣的 keyword；`letter-spacing` 的 `normal`。
  - transition：`transitionend` 的 listener 讀到上一個 frame 的值；新的 transition 從上一個 frame 的值開始（before-change style 改成 transition 在這個時間的值重新套到 base style）；移除、搬走的元素沒有 `transitioncancel`、從舊的 style 開始；`allow-discrete` 對不能插值的一對；`content: none` 的 `::before` 的 transition 不取消；只有 transition 在動的 frame 沒有要畫。
  - CSS animation：keyframe 用 `vw` 時 viewport 改了不重新 style；`::marker` 的動畫沒有開始；timing 一改就再發一次 `animationstart`；往回走的事件用錯 interval 的一端、`animationend` 排到 `animationstart` 前面；秒數轉毫秒照 f32 的十進位（`0.1s` 是 100 ms）；同名的 `animation-name` 從頭配對。
  - Web Animations：promise 被當成舊的丟掉、交還得太早（`finished` 不 resolve、`CSSAnimation-ready.tentative.html` 逾時）；wasm 的 kept promise 比 realm 活得久；取消之後的事件狀態沒回到 idle；`options` 沒給時當成 duration 0；prune 把 effect 還在的動畫清掉；CSS animation 換掉 effect、`updateTiming()` 之後 style 又改回來；`timeline = null` 不發 cancel；script 的動畫沒算進 after-change style（`style-change-events.html` 2/24 → 22/24）；元素不再 render、搬動時 script 的動畫被取消；property-indexed keyframe 的 `computedOffset`、`easing` 和 `composite` 的檢查；step easing 的 before flag 沒看方向；rate 0、`reverse()` 的 -0、`updatePlaybackRate()`；`getAnimations({subtree})` 進了 shadow tree；`finish` 事件的時間。
- **不穩定的測試**：
  - `shell` 的 `transitions::a_removed_element_cancels`（30 ms 的 transition）和 `css_animations::its_events`（100 ms 一圈）在一起跑、搶 CPU 時 frame 的間隔太長（使用者在 Linux 上看到）：照規範的行為，是測試假設了 frame 的間隔；改成 `2s linear -1s`、`1s linear 2 -0.5s` 留下餘裕，之後整個 `shell` 一起跑三次都通過，記在 `docs/testing.md`。
  - WPT 的 `update-and-send-events.html` 並行跑時偶爾 `["cancel", "finish"]`：浮點數的 current time 差一點沒到 end、`finish` 的排定時間比現在大一點；改成取到整數的微秒、差一微秒以內算到了、排定時間不超過現在。`dom_js` 測試扮演 renderer 時原本沒有照 renderer 的 `sort_events` 排事件，改了之後抓得到。
- **mutation test**（每一小段一份，`mutants-*.toml`；第 3 段做完時照最後的程式全部重跑過）：
  - 最後一次（2026-10-16）：`1a` 25 條、`1b` 35、`1c` 38、`1d` 14、`1e` 19、`2a` 46、`2b` 45、`2c` 32、`3a` 65、`3b` 69、`3b-2` 25、`3c` 27 條，共 440 條；沒抓到的只有下面三條。
  - 每段第一次沒抓到的，多數是測試沒碰到的情況，都補了測試：shorthand 在有 delay 時拿掉 duration、`currentcolor` 照另一邊的 `color`、duration 0 的 active duration、已完成的 transition 的三條、祖先的 `display: none`、timeline 的時間、事件的順序、剛結束的元素沒標、空字串的 `animation-name`、keyframe 裡的 `animation-timing-function`、動畫本身的 easing、tick 過之後不相干的 style change、`pause()` 讓 `animation-play-state` 不再管、`detached` 的動畫開始和結束、keyframe 的排序、`findRule` 只比第一個 selector、property-indexed 的單獨一個值、換 target 之後重算 keyframe、markup 的 transition 和不在 effect 裡的不被取代、移除的不套值和拿不到、`commitStyles()` 算它下面的動畫。多餘的程式拿掉（和那一條）：`-0` 換成 `0`、`all` 收 flow-relative 的 filter、`@keyframes` 名稱之後的 `expect_exhausted`、移除被取代的動畫之後重新 style。
  - 沒抓到、留著的：renderer 的 `animating`（等價：run loop 的 `wants_frames` 照樣替它要 frame）、拿走的 effect 留著 keyframe（等價：timing 同時換成長度 0、沒有 fill），記在 `docs/test-gaps.md` 的「等價、不用處理的 mutation」；renderer 不呼叫 `let_go`（`shell` 的測試看不到記憶體，步驟本身有 `dom_js` 的測試），記在「只靠審查」。另外「timeline 只在有動畫時才更新」沒抓到，記在「能補、還沒補的測試空缺」。
  - `cargo fmt` 和後面的段落改過的程式，前面的 spec 的 `from` 跟著改過再跑。Windows 上兩次跑到一半停下（一次一起跑八份、一次寫檔的 `io.open` 失敗），留下沒還原的 mutant，`test.py mutate --restore` 還原之後重跑。
- **WPT**（`include.txt` 加上 `css/css-transitions/`、`css/css-animations/`、`css/css-easing/`、`web-animations/` 整個和 336 個 `*-interpolation.html`；預期結果用 `MACHOPO_UPDATE_EXPECTATIONS` 寫下，每個失敗的 `.ini` 寫了原因，3b-1 和 3c 由 subagent 對照測試和失敗訊息寫、我審過，161 個 `.ini` 是 kun 沒有那個 property）：
  - 這些一共 905 個檔案，370 個整個通過；每段都沒有變失敗的。清單裡其他用到動畫的測試（`css/css-tables/animations/`、各目錄的 `*-no-interpolation.html` 等）只有變成通過的。
  - 還失敗的主要原因：kun 沒有的 property（`translate`、`rotate`、`scale`、`transform-origin`、`calc-size()`、`skewX()`）、`composite: add`、`animation-composition`、`@starting-style`、scroll-driven animation、`new DocumentTimeline()`、window 的 named property、`AnimationEvent.animation`；還沒查的寫明了：巢狀 iframe 裡的 `finished`、iframe 裡 `vw` 的 fill、繼承的 `visibility`、`oklch()` 的結束顏色、元素搬進 iframe 的 document 之後的 transition。
- **跑過的**：改到的 crate 的測試；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；每段做完時的 `cargo test --workspace`（第 1 段 2778 個、第 2 段 2876 個、第 3 段 2954 個測試通過；`dom` 的 compile-fail 的預期輸出多列了新的 `NoTrace` 型別，錯誤的種類沒變）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`；上面的 WPT；`test.py scenarios transitions css-animations demo-script tables`；`test.py automation transitions css-animations demo-script`（第 1、2 段，真的視窗：長條變寬變綠、spinner 轉、暫停時停住、第二個 frame 長條在中間）。
- **沒跑的**：`matrix`、`fuzz`（新的 GC 型別在 merge 之後看 CI）；整套的 `wpt`、`wdspec`、`reftest`；第 3 段沒動視窗那一層，沒跑 `automation`；`ci.py speedometer`（只在使用者要求時跑）。`cargo clean` 等 merge 回 `main` 之後跑。
