# RFC 0080 的細節

## Chrome 152 的探測

2026-10-23，Fedora 44，headless 的 Chrome for Testing 152（`scripts/bench/common.py` 的 `ChromePipe`）和 kun（`KunPipe`，dev build），同一段 script。

### `styleSheets` 和 `sheet`

頁面的 `<head>`：

- `s1`：`<style media=screen title=t1>`，2 條規則；
- `s2`：`<style type=text/plain>`；
- `l1`：`<link rel=stylesheet>`（data URL，2 條規則）；
- `l2`：`<link rel="alternate stylesheet" title=alt>`；
- `l3`：`<link rel=stylesheet title=pref>`；
- `l4`：`<link rel=stylesheet disabled>`；
- `s3`：`<style>@import url(…); p {}</style>`。

| 項目 | Chrome 152 | kun |
| --- | --- | --- |
| `document.styleSheets` | `[object StyleSheetList]`，每次讀同一個物件；依 document 順序 `s1`、`l1`、`l2`、`l3`、`s3`（`s2`、`l4` 沒有 sheet，alternate 的 `l2` 有） | `undefined` |
| `sheet` 的欄位 | `ownerNode` 是元素；`href` 是 `<style>` 的 `null`、`<link>` 的網址；`title` 是屬性；`media.mediaText` 是屬性（`screen`）；`disabled` 都是 `false`（alternate 的也是）；`parentStyleSheet`、`ownerRule` 是 `null`；`type` 是 `text/css` | 沒有 `sheet` |
| `@import` | `cssRules[0]` 是 `CSSImportRule`，它的 `styleSheet` 有 2 條規則，`parentStyleSheet` 是外面的 sheet，`ownerNode` 是 `null` | parser 丟掉 `@import` |
| `sheet.insertRule('p { color: rgb(1, 2, 3) }', 2)` | `<p>` 的 color 變了，`<style>` 的文字不變 | — |
| 規則的 `style.color = …` | `<p>` 的 color 變了 | — |
| `sheet.disabled = true` | 不套用；`<style>.disabled` 也是 `true`（同一個旗標） | — |
| `sheet.media.mediaText = 'print'` | 不套用；`media` 屬性還是 `screen` | — |
| `<style>.media = 'print'` | 同一個 sheet 物件，`media` 變 `print`，加的規則還在；改回 `screen` 之後套用加過的規則 | — |
| `<style>.title = 'x'` | 同一個 sheet 物件，`title` 變 `x` | — |
| 改 `<style>` 的文字（append 一個文字節點） | 新的 sheet 物件，規則回到文字的 2 條；舊的 `ownerNode` 變 `null` | — |
| 移除 `<style>` | `sheet` 是 `null`，舊的 `ownerNode` 變 `null`、規則還在；清單少一個 | — |
| 再插入 | 新的 sheet 物件 | — |
| `createElement('style')`，還沒插入 | `sheet` 是 `null`；插入之後有 | `undefined` |
| `<link>` 的成員 | `sheet`、`media`、`disabled`、`type`、`title` 都有 | 只有 `title` |
| 舊的成員 | `rules === cssRules`；`addRule('p', 'color: red')` 回傳 `-1`，規則多一條；`removeRule` | — |
| `cssRules` | 每次讀同一個物件，live | — |
| 規則物件 | 前面插入規則之後還是同一個物件，`parentStyleSheet` 是 sheet | — |
| shadow tree | `shadowRoot.styleSheets` 是 `StyleSheetList`，有它的 `<style>`；不在 `document.styleSheets` 裡；兩個 shadow tree 有同樣文字的 `<style>`，改其中一個的規則，另一個不變；兩個 sheet 是不同的物件 | — |
| adopted 的 | 不在 `document.styleSheets` 裡 | — |
| 文件的 sheet 的 `replaceSync` | `NotAllowedError` | — |
| SVG 的 `<style>` | 有 `sheet`，在清單裡 | — |

title 和 alternate（另一個頁面）：

- `<link rel="alternate stylesheet" title=alt>`：在清單裡，`disabled` 是 `false`，但不套用；把 `sheet.disabled` 或 `<link>.disabled` 設成 `false` 也不套用。
- 沒有 title 的 alternate：`sheet` 是物件（照樣載入）。
- 兩個有不同 `title` 的 `<style>`：第一個（`other`）套用，第二個（`first`）不套用，`disabled` 都是 `false`。

### 第 4 段：`sheet` 的動態行為、跨 origin、preferred set

2026-10-24，同樣的工具，頁面由本機的 HTTP server 給（`127.0.0.1` 和 `localhost` 是兩個 origin；`/cors/` 底下的回應有 `Access-Control-Allow-Origin: *`，`/redirect/` 轉到別的路徑）。kun 的結果在實作之後跑同一組頁面，除了下面寫的，和 Chrome 一樣（例外的訊息沒有 Chrome 的 binding 加的「Failed to execute … on 'CSSStyleSheet':」前綴）。

**`<link>` 的 sheet**

| 項目 | Chrome 152 |
| --- | --- |
| 同 origin | `cssRules`、`rules`、`insertRule`、`deleteRule`、`addRule`（回傳 `-1`）、`removeRule` 都可以 |
| 別的 origin，沒有 `crossorigin` | 套用；上面那些都丟 `SecurityError`（`cssRules`、`rules`：「Cannot access rules」；`insertRule`、`addRule`：「Cannot access StyleSheet to insertRule」；`deleteRule`、`removeRule`：「… to deleteRule」） |
| 別的 origin，有 `crossorigin`，回應允許 CORS | 讀得到規則 |
| 別的 origin，有 `crossorigin`，回應不允許 | 不套用；有 sheet、在清單裡，讀規則丟 `SecurityError` |
| 轉址 | `href` 是要求的網址（轉址之前的） |
| 404 | 有 sheet、在清單裡，0 條規則，讀得到 |
| `data:` | 讀得到 |
| 每個 sheet | `replaceSync`、`replace` 丟（或拒絕）`NotAllowedError`「Can't call replaceSync on non-constructed CSSStyleSheets.」；`ownerRule` 是 `null`；`media.mediaText` 是 `""` |
| `adoptedStyleSheets = [link.sheet]` | `NotAllowedError`「Can't adopt non-constructed stylesheets.」 |
| `createElement('style').sheet` | `null`；插入之後有 |

**改屬性、改文字、移除**

| 項目 | Chrome 152 | kun |
| --- | --- | --- |
| 載入之後 `link.disabled = true` | `sheet` 是 `null`，不在清單裡，不套用 | 一樣 |
| 再 `disabled = false` | 重新載入：載入之前 `sheet` 是 `null`，之後是新的物件 | 用之前載入的文字，馬上是新的物件、套用（kun 不重新載入 `<link>`） |
| `rel = "icon"` | `sheet` 是 `null`，舊的 `ownerNode` 是 `null` | 一樣 |
| 再 `rel = "stylesheet"` | 重新載入，之後是新的物件 | 馬上是新的物件（同上） |
| `link.media = "print"` | 同一個物件，`media` 變 `print`，不套用 | 一樣 |
| `sheet.disabled = true`（`<link>` 的） | 不套用；`link.disabled` 和屬性不變 | 一樣 |
| `<style>.disabled = true`，再改文字 | 新的 sheet 的 `disabled` 是 `false`，`<style>.disabled` 也是；舊的 sheet 還是 `true` | 一樣 |
| `sheet.media.mediaText = "print"`（`<style>` 的） | 不套用；`media` 屬性不變 | 一樣 |
| 再 `<style>.media = "screen"` | 同一個 sheet，`media` 變 `screen`，套用 | 一樣 |
| 拿掉 `media` | `media` 是 `""` | 一樣 |
| `<style>.title = "tt"`，拿掉 | 同一個 sheet，`title` 變 `tt`，再變 `null` | 一樣 |
| 移除 `<style>` | 舊的 `ownerNode` 是 `null`，規則、`disabled`、`media` 照舊；對它 `insertRule` 有效，但不套用 | 一樣 |
| 再插入 | 新的物件，回到文字的規則 | 一樣 |
| `styleSheets` | 每次讀同一個物件；`item(99)` 是 `null`、`[99]` 是 `undefined`；own key 是 `0`、`1`；有 `@@iterator` | 一樣 |

**title 和 alternate**

| 項目 | Chrome 152 |
| --- | --- |
| preferred set | 第一個有 `title`、不是 alternate 的 sheet 的 title；之後不會變：那個 sheet 移除了、改了 title、前面插入另一個有 title 的，都照舊 |
| 有 title 的 sheet | title 是 preferred set 的才套用，alternate 的 `<link>` 也是（`rel="alternate stylesheet" title=first` 在 preferred set 是 `first` 時套用） |
| 沒有 title 的 alternate | 載入、在清單裡，不套用 |
| 改 `title` 屬性 | 不會馬上重新判斷：要等那個 tree scope 的 sheet 因為別的原因重新收集（例如加了一個 `<style>`）；kun 照做（改 `title` 不標記 tree scope） |
| shadow tree 裡有 title 的 `<style>` | 套用（shadow tree 不看 title） |
| alternate 的 `sheet.disabled` | 一直是 `false`，設成 `false` 也不會套用 |

### `toJSON` 和 `layerX`

| 項目 | Chrome 152 | kun |
| --- | --- | --- |
| `JSON.stringify(new DOMRect(1, 2, 3, 4))` | `{"x":1,"y":2,"width":3,"height":4,"top":2,"right":4,"bottom":6,"left":1}` | `{}` |
| `DOMRectReadOnly`、`getBoundingClientRect()` | 同上的格式 | `{}` |
| `DOMPoint`、`DOMQuad`、`DOMMatrix` 的 `toJSON` | 有 | 沒有這些 interface |
| `'layerX' in MouseEvent.prototype` | `true`（`UIEvent` 沒有） | `false` |
| 沒有分派的 `new MouseEvent('click', {clientX: 10, clientY: 20})` | `layerX`、`layerY` 是 10、20（client 座標） | `undefined` |
| 分派到有定位的元素（`position: relative`，在 (38, 48)）裡沒有定位的子元素上，client (60, 70) | `layerX`、`layerY` 是 22、22（相對於有定位的元素）；`offsetX`、`offsetY` 是 17、22 | `undefined`；`offset` 相同 |
| 分派到有定位的元素自己 | 22、22 | `undefined` |
| 分派到沒有定位、祖先也沒有的元素，client (60, 160) | 60、160（page 座標） | `undefined` |

### `layerX` 的 layer

一般的頁面，8px 的 body margin，在每個目標的盒子裡 10px 的地方分派 `click`（`layerX,layerY,pageX,pageY,offsetX,offsetY`）。實作之後 kun 的結果除了下面註明的，都和 Chrome 一樣。

| 目標在 | Chrome 152 | 說明 |
| --- | --- | --- |
| `position: relative`（`left: 20px`）的盒子裡 | `25,25,83,73,7,7` | 從有定位的盒子的 border box（含 relative 的位移） |
| 那個有定位的盒子自己 | `10,10,68,58,6,6` | |
| 絕對定位裡的 relative 盒子裡 | `16,16,435,95,7,7` | 最裡面那個 |
| `transform: translate()` 的盒子裡 | `16,16,89,149,7,7` | 從變形之後的位置 |
| `opacity: 0.5` 的盒子裡 | `16,16,84,176,7,7` | |
| `overflow: hidden` 的盒子裡 | `98,232,98,232,7,7` | 不算 layer：page 座標 |
| 捲過的 `overflow: scroll` 的盒子裡 | `111,325,111,325,7,7` | 不算 layer |
| 沒有定位的祖先 | `114,388,114,388,7,7` | page 座標 |
| inline 的 `<span>` | `109,433,109,433,10,10` | kun 的 `pageY` 是 432.74（文字的位置），`layerY` 432 |
| `position: fixed` 的盒子裡 | `17,17,517,417,7,7` | |
| `position: static` 加 `z-index` 的盒子裡 | `33,457,33,457,7,7` | `z-index` 沒有作用：不算 |
| `contain: paint` 的盒子裡 | `15,15,33,507,7,7` | |
| 頁面捲 15px 之後，relative 的盒子裡 | `25,25,83,73,7,7` | |
| 頁面捲 15px 之後，fixed 的盒子裡 | `17,17,517,432,7,7` | kun `17,2,517,417`：kun 的 `getBoundingClientRect` 對 fixed 盒子裡的元素多扣了捲動（和這份無關的 bug，記在 `docs/limitations.md`） |
| 分派之後把 layer 移走 100px 再讀 | `-75`（`offsetX` `-93`） | 讀的時候才算 |
| 沒有分派的 `new MouseEvent(…, {clientX: 10, clientY: 20})` | `10,20` | `pageX`；捲動之後 Chrome 還是 `10,20`（建立時算好），kun 是 `10,35`（`pageY` 原本就是這樣，記在 `docs/limitations.md`） |
| text node、不在 document 裡的元素 | `10,35` | page 座標 |
| relative 盒子裡 `display: none` 的元素 | `-148,-13` | 往上第一個有盒子的祖先的 layer |
| `display: contents` 加 `position: relative` 的元素裡 | 從外面那個 relative 盒子 | 沒有盒子的不算 |
| sticky、`filter`、`isolation: isolate`、flex 項目的 `z-index` 的盒子裡 | 從那個盒子 | stacking context |
| `clientX` 10.7、-3.2 | `layerX` 10、`layerY` -4 | Chrome 建立時就把 `clientX` 捨去；kun 的 `clientX` 留著小數，`layerX` 捨去 |
| `clientX` ±1e12 | 2147483647、-2147483648 | `long`，夾住 |
| `left: 0.6px` 的 relative 盒子 | 從 0.59375 起算（LayoutUnit），`layerX` 11 | kun 的 layout 是 `f32`：差捨去之前對齊 1/64 px |
| shadow tree 裡的事件，host 上的 listener 讀 | 從 host 的 layer | retarget 之後的目標 |
| `<html style="position: relative; left: 40px; top: 30px">` 裡沒有定位的祖先的目標 | 從 `<html>` 的盒子（`18,18`；`opacity: 0.5` 的 `<html>` 也是） | root element 有定位或是 stacking context 時也算；kun 起初把它排除了，mutation test 找到 |

### 數學函式

| 寫法 | Chrome 152 | kun |
| --- | --- | --- |
| `width: round(117px, 10px)` 的 computed value | `120px` | 宣告無效 |
| `round(up, 111px, 10px)` | `120px` | 無效 |
| `round(down, 119px, 10px)` | `110px` | 無效 |
| `round(to-zero, -119px, 10px)` 的 specified value | `calc(-110px)` | 無效 |
| `calc(round(117.5px, 1px) * 2)` | `236px` | 無效 |
| `round(117px)` | 無效（一個參數只對 number：`z-index: round(7.5)` 是 `calc(8)`） | 無效 |
| `opacity: round(0.46, 0.1)` 的 specified value | `calc(0.5)` | 無效 |
| `round(33.3%, 10px)`（混合百分比和長度）的 specified value | `round(33.3%, 10px)`（留到 layout 才算） | 無效 |
| `round(nearest, 15px, 10px)` 的 specified value | `calc(20px)` | 無效 |
| `mod(17px, 5px)`、`rem(17px, 5px)` | `calc(2px)`、`calc(2px)` | 無效 |
| `abs()`、`sqrt()`、`pow()`、`hypot()`、`sin()`、`exp()`、`log()`、`clamp()`、`min()` | 支援（`sign(-3px)` 當寬度無效：它是 number） | 只有 `clamp()`、`min()` |

混合百分比和長度的 `round()` 和 kun 現在的 `min()`、`max()` 一樣不能解析（computed value 只能是長度加百分比），照舊是限制。

### 其他的數學函式（第 2 段）

`width` 或 `margin-left`，10px 的字（`specified` 讀回的，`computed` 是 `getComputedStyle`）。kun 實作之後除了註明的都一樣。

| 寫法 | Chrome 152 讀回 | 說明 |
| --- | --- | --- |
| `round(1em, 3px)`、`round(nearest, 1em, 3px)` | `round(1em, 3px)`，9px | `nearest` 省略 |
| `round(up, 1em, 3px)` | `round(up, 1em, 3px)`，12px | |
| `round(7em, 2em)`、`round(10%, 3%)` | 原樣，80px、9% | 同一個單位，但不是標準單位：不算出來 |
| `round(down, 7px, 2px)`、`round(to-zero, -7px, 2px)` | `calc(6px)`、`calc(-6px)` | 標準單位：算出來 |
| `mod(-7px, 3px)`、`rem(-7px, 3px)` | `calc(2px)`、`calc(-1px)` | `mod()` 帶除數的正負號，`rem()` 帶被除數的 |
| `abs(1em - 3px)`、`hypot(3em, 4px)` | 原樣，7px、30.25px（kun 30.2655px：Chrome 的 layout 是 1/64 px） | |
| `calc(1px * sin(30deg))`、`calc(100px * cos(0.5))`、`calc(1px * exp(1))` | `calc(0.5px)`、`calc(87.7583px)`、`calc(2.71828px)` | |
| `calc(1px * tan(90deg))` | `calc(infinity * 1px)` | 角度剛好在漸近線上是無限大 |
| `round(10px, 0px)`、`mod(10px, 0px)` | `calc(NaN * 1px)` | |
| `calc(sign(1em) * 10px)`、`calc(sign(1em) * 2 * 10px)` | `calc(10px * sign(1em))`、`calc(20px * sign(1em))` | 乘積的葉節點合成一個，放前面 |
| `calc(10px / sign(-1em))`、`calc(2 * (10px / sign(-1em)))` | 原樣、`calc(20px / sign(-1em))` | |
| `calc(1px + 2px * sign(1em))`、`calc(1px - 2px * sign(1em))` | `calc(1px + (2px * sign(1em)))`、`calc(1px - (2px * sign(1em)))` | 加總裡的乘積加括號，減掉的寫在 ` - ` 之後 |
| `calc(10px - 2 * min(1px, 1em))`、`calc(-2 * min(1px, 1em) + 1em)` | `calc(10px - (2 * min(1px, 1em)))`、`calc(1em + (-2 * min(1px, 1em)))` | kun 原本沒有括號，也不分減掉和負的係數 |
| `calc(1px * (1 + sign(1em)))` | 原樣 | 乘積裡的加總加括號 |
| `calc(1px / sign(mod(1, -1)))`、`calc(1px / sign(round(-0.4, 1)))`、`calc(1px / sign(min(0px, -0px)))` | `calc(-infinity * 1px)` | 正負零 |
| `opacity: sign(1em)`、`z-index: sign(1em)` | `sign(1em)`，1 | kun 無效（number 在解析時算出來，見 `docs/limitations.md`） |
| `opacity: round(2.5)`、`z-index: round(-2.5)` | `calc(3)`、`calc(-2)` | kun 讀回 `3`、`-2`（原本就有的限制） |
| `rotate: atan2(1em, 10px)` | `calc(atan2(1em, 10px))`，45deg | 最外層的 `atan2()` 包在 `calc()` 裡；kun 沒有 `rotate`，角度裡的 `atan2(1em, 10px)` 無效 |
| `round(1em, 3%)`、`abs(10% - 1px)` | 原樣 | kun 無效：長度和百分比混用（原本就有的限制） |

WPT 的 `css/css-values/` 的數學函式測試：invalid 的六份和三份 crashtest、`signed-zero.html` 整份通過；computed 和 serialize 的多半用 `scale`、`rotate`（kun 沒有這兩個 property）、number 的 `calc(1)`（kun 讀回 `1`）、`ex`、`ch`、`sibling-index()` 和 number 裡的相對長度，還跑不過。

### 第 3 段的量測

2026-10-23 到 24，Fedora 44（AMD Ryzen AI 9 HX 370），`bench.py blink-perf --dirs css --against bf09c473`（第 2 段的 commit；81 頁，各 3 輪，每頁中位數）。工具在寫報告時除以 0 結束（一頁在舊版是 0 ms：`blink_perf.py` 的 `write_compare_report`），數字從 `compare.json` 算。

- 第一次（用電池）：能比的 49 頁的幾何平均是 0.994（新版稍快）。變慢最多的：`NestingIdent*`（0.3 對 0.4 ms，太小量不準）、`HasDescendantInvalidation*` 四頁 5–7%、`ChangeStyleCSSVariableRecalc` 4%。
- 插電、那幾頁各 5 輪：`ChangeStyleCSSVariableRecalc` 穩定慢 4%（478 對 498 ms），`HasDescendantInvalidationMultipleSubjects`、`NestingIdentInvalidValue` 一樣，`HasDescendantInvalidation` 舊版本身在 42.8–45.5 runs/s 之間跳。
- `ChangeStyleCSSVariableRecalc` 有 20000 個有 `classList` 的元素，每個都有 rare data：rare data 多了兩個欄位（`Option<(Arc<str>, Option<Url>)>` 一百多 bytes），改成一個 box 之後是 480.3 對 481.4 ms（0.2%）。
- `HasDescendantInvalidation` 之後還是穩定慢 2.7%（42.8 對 41.7 runs/s）。`perf record` 兩邊的 renderer main thread：第 3 段的函式一個也沒有出現在熱點上（`collect_document_sheets` 沒有取樣到），差別在 `recalc_style` 和它 inline 的比對之間的分配（15.4% 對 17.9%、`matches_complex_selector_internal` 5.6% 對 3.4%），是編譯出來的 inline 和程式的排列不同，不是多做了事。

### 第 4 段：`RuleSet` 怎麼分

2026-10-24，Fedora 44（AMD Ryzen AI 9 HX 370），插電，`perf` 的 kun（commit `dce731f4`）。決定前量的（「已決定的事項」第 11 條），頁面和程式不在 repo 裡。

**比對**：2000 個元素（每個 3 個 class，400 種），總共 2000 條規則（`.a`、`.a .b`、`div.a > .b` 各三分之一），分在 N 個 sheet；`<html>` 的 class 一換（`.t div` 讓每個元素都重算），量 `getComputedStyle` 的時間，25 次的中位數、3 次啟動的中位數。`<style>` 是現在合成一份的做法，adopted sheet 是每個 sheet 一份。

| sheet 數 | 合成一份 | 每個 sheet 一份 |
| --- | --- | --- |
| 1 | 59.8 ms | 59.7 ms |
| 20 | 59.8 ms | 59.6 ms |
| 200 | 59.9 ms | 72.8 ms（+22%） |
| 1000 | 60.4 ms | 101.5 ms（+68%） |

**建 `RuleSet`**：`css` 的 `RuleSet::add_rules`，release，21 次的中位數。「接起來」是把各 sheet 已經建好的 `RuleSet` 的 bucket 接成一份（暫時的原型，rule 的編號加位移）。框架的規則是 class、子孫、`:hover`、`:not()`、id、`:nth-child` 和 `::before` 混合，CSS-in-JS 的是一個 class 四個宣告。

| 情況（變了的是後者或其中一個） | 全部重建 | 只建變了的 | 建變了的再接起來 |
| --- | --- | --- | --- |
| 框架 5000 條＋CSS-in-JS 500 條 | 1.011 ms | 0.055 ms | 0.424 ms |
| 框架 5000＋CSS-in-JS 5000 | 1.308 ms | 0.587 ms | 1.661 ms |
| 框架 20000＋CSS-in-JS 2000 | 2.698 ms | 0.233 ms | 1.004 ms |
| CSS-in-JS 20000 | 3.050 ms | 3.048 ms | 5.512 ms |
| 20 個 sheet × 100 條 | 0.258 ms | 0.014 ms | 0.189 ms |
| 200 × 10 | 0.259 ms | 0.001 ms | 0.180 ms |
| 1000 × 2 | 0.263 ms | 0.000 ms | 0.190 ms |
| 200 × 50 | 1.133 ms | 0.007 ms | 0.681 ms |

接起來每條規則約 0.12 µs（hash map 的插入），從頭建約 0.15 到 0.2 µs。

**每個 frame `insertRule`**：框架的 `<style>` 加上 adopted 的 CSS-in-JS sheet、1000 個元素；每個 frame `insertRule(rule, sheet.cssRules.length)` 5 條（emotion 的寫法）、加一個用到它的元素、`getComputedStyle`，31 個 frame 的中位數。

| 情況 | 每個 frame |
| --- | --- |
| CSS-in-JS 500 | 0.5 ms |
| 框架 5000＋CSS-in-JS 500 | 1.9 ms |
| 框架 20000＋CSS-in-JS 2000 | 7.1 ms |
| CSS-in-JS 20000 | 16.6 ms |

最後一種的 `perf record`（包括一條一條加進 20000 條的準備）：`CssPropertyValueSet::has_unresolved_urls` 36%、`css_rules_of` 21%、`CssRule` 的 drop 27%、`insert_rule` 7%、`resolve_rule_urls` 6%；`RuleSet` 的函式加起來不到 1%。每次 `insertRule` 複製、丟掉整份規則的清單，每條規則都重新檢查 `url()`；讀 `cssRules.length` 也複製一次（`css_rules_of` 的 `to_vec()`）。

### 第 4 段的量測

2026-10-24，Fedora 44（AMD Ryzen AI 9 HX 370），插電，`perf` 的 kun。

**實作之後再跑一次「每個 frame `insertRule`」**（同上一節的頁面，`insertRule` 只解析新的那條規則的 `url()`、讀 `cssRules.length` 不複製、規則多的 sheet 各自一份 `RuleSet`、每個 set 的特徵算一次之後）：

| 情況 | 之前 | 之後 |
| --- | --- | --- |
| CSS-in-JS 500 | 0.5 ms | 0.4 ms |
| 框架 5000＋CSS-in-JS 500 | 1.9 ms | 0.5 ms |
| 框架 20000＋CSS-in-JS 2000 | 7.1 ms | 1.0 ms |
| CSS-in-JS 20000 | 16.6 ms | 8.2 ms |

只分 `RuleSet` 不夠：框架 20000＋2000 那一種的 profile 裡，建 `RuleSet` 約 16%，把全部規則的 invalidation 特徵和 dependency 重新算一次（`FeatureSets::of`、`InvalidationFeatures::of`、`scope_dependencies`）約 35%，所以特徵也是每個 set 算一次、存著，tree scope 合併各 set 的（Blink 的每個 `RuleSet` 帶著它的 `RuleFeatureSet`）。比對那一組（2000 個元素）實作之後是 61.1 / 61.2 / 61.2 / 61.5 ms（1、20、200、1000 個 `<style>`，都合成一份），和實作之前差不多（兩邊一起高了約 2%，adopted 那一欄也是，看來是機器的狀態）。

**`bench.py blink-perf --dirs css --against dce731f4`**（第 3 段的 commit，81 頁各 3 輪；工具照樣在寫報告時除以 0 結束，數字從 `compare.json` 算）：能比的 51 頁的幾何平均是 0.999。

- `SelectorCountScaling` 慢了一倍（694 對 337 runs/s）：每一輪插入一個 14000 條規則的 `<style>` 再移除，下一輪的文字一樣，內容共用同一份，但 media 是每個元素一份 `Arc`，engine 比的是指標，就當成新的 sheet、算一次規則的增減。改成指標不同時比 media 的內容（之前比的是 `media` 的文字）之後，5 輪是 1.8% 以內（另一個版本 930–945、這個 912–933 runs/s）。
- `ChangeStyleCSSVariableRecalc` 慢 4.1%、`ClassInvalidation` 慢 5.3%（各 5 輪，穩定）。
  - rare data 多了兩個 edge（sheet 的物件、`styleSheets`）：把它們和元素的 sheet 放進一個 GC 物件（`StyleSheetData`），rare data 回到第 3 段的大小，兩頁照舊（471 對 490 ms、931 對 883 runs/s）。
  - `ClassInvalidation` 的元素沒有 rare data。`perf stat` 那一頁的 renderer main thread 10 秒：照 runs/s 換算，每一輪的指令數這個版本少 3.6%（1173 萬對 1217 萬），IPC 從 2.86 降到 2.70；`perf record` 兩邊都是 `recalc_style`（走訪的那段 inline 在裡面）70–76%，沒有新的函式出現。
  - `ChangeStyleCSSVariableRecalc` 的頁面有一個 2000 條規則的 `<style>` 和一個只有 `:root` 的：之後是兩個 set。同樣的事另外寫一頁量（15 次的中位數、3 次啟動的中位數）：分兩個 `<style>` 時兩個版本是 501.9 和 502.0 ms，合成一個時 500.1 和 488.0 ms，多一個 set 沒有多花；兩邊的 profile 一樣是 `cascade` 約 50%、候選的 `RuleData` 排序約 30%。
  - 和第 3 段的 `HasDescendantInvalidation` 一樣，看作編譯出來的 inline 和程式的排列不同，不是多做了事。

### `focus()` 捲進畫面

800×600 的 iframe，`body` 5000px 高（第一組 3000px 寬，Chrome 因此有 15px 的捲軸，kun 的捲軸不佔版面），`button` 是 100×40 的絕對定位。

| 情況 | Chrome 152 | kun | 說明 |
| --- | --- | --- | --- |
| 在 2000px，看不到 | 1728 | 1440 | Chrome 置中（2020 − 585/2） |
| 在 580px，部分看得到 | 35 | 20 | 都捲到最近的邊；差的是 Chrome 的捲軸 |
| 在 300px，捲到 320，上緣看不到 | 300 | 300 | 最近的邊 |
| 在 x = 1500，看不到 | x 1158 | x 800 | 水平也置中 |
| 900px 高，在 1200px，看不到 | 1358 | 1200 | 比視窗高的也置中 |
| 在 200px 高的捲動容器裡的 1000px，容器在 3000px | 視窗 2808、容器 920 | 2600、840 | 每一層都置中 |
| 900px 高，在 100px，捲到 500：視窗被它蓋住 | 400 | 400 | 最近的邊（視窗被整個蓋住時 Blink 也算看得到；這裡沒有整個蓋住） |

editing host（第二組，`body` 9000px 高，沒有水平捲軸）：

| 情況 | Chrome 152 | kun | 說明 |
| --- | --- | --- | --- |
| 3000px 高的 editing host 在 0，捲到 8 | 0 | 8 | caret 在最上面，部分看不到：捲到最近的邊 |
| 同一個，捲到 3500（整個看不到） | 0 | 2400 | caret 置中（夾到 0） |
| 40px 高的在 4000px | 3709 | 3440 | caret（約 18px 高）置中 |
| 40px 高的在 6580px，捲到 6000（caret 看得到、元素的下緣看不到） | 6000 | 6020 | Chrome 不捲：看的是 caret |
| 1500px 高的在 7000px | 6709 | 7000 | caret 置中 |
| `tabindex=0` 的 div 在 5000px | 4720 | 4440 | 置中 |

文字控制項（第三組，`body` 9000px 高，16px monospace）：

| 情況 | Chrome 152 | kun | 說明 |
| --- | --- | --- | --- |
| 400px 高的 `<textarea>` 在 4000px，看不到 | 3711 | 3800 | caret 置中（元素置中會是 3900） |
| 40px 高的 `<input>` 在 6000px，看不到 | 5720 | 5440 | 置中 |
| 400px 高的 `<textarea>` 在 7400px，捲到 7000（caret 看得到、元素的下緣看不到） | 7000 | 7200 | Chrome 不捲：看的是 caret |
| editing host 在 8000px，先把 selection 放在它最後一行，再 `focus()` | 7931 | 0 | 探測的分頁沒有 focus 時 Chrome 把 caret 置中；分頁有 focus 時 Chrome 也是 0：Selection API 把 selection 放進 editing host 時它就拿到 focus（Blink 的 `SetFocusedNodeIfNeeded`，kun 照做），之後的 `focus()` 什麼都不做 |

editing host 拿到 focus 時 caret 放的位置（分頁有 focus；host 裡沒有 selection）：

| host 的內容 | Chrome 152 | kun 原本 |
| --- | --- | --- |
| `abc` | `"abc"` 0 | 同 |
| `<p> abc</p>` | `" abc"` 1（第一個看得見的字元） | 0 |
| `\n  <div>first</div>\n` | `"first"` 0 | `"\n  "` 0：那裡畫不出 caret、也捲不到 |
| `<span></span><span> </span><span>abc</span>` | `"abc"` 0 | host 0 |
| `<p style="white-space: pre">\nabc</p>` | `"\nabc"` 0（保留的換行之前） | 同 |
| `<p></p><p><br></p>` | 第二個 `p` 的 0（`<br>` 之前） | host 0 |
| `<span></span><input><span>abc</span>` | host 1（`<input>` 之前） | host 0 |
| `<span contenteditable="false">abc</span>def` | host 0 | 同 |
| `<p><!-- c --></p><p>abc</p>` | `"abc"` 0 | host 0（第一個葉節點是註解） |
| `<span></span>` | host 0 | 同 |
| `<div><span></span><span contenteditable="false">abc</span>def</div>` | `div` 0（不能編輯的元素的 parent 的開頭） | host 0 |

結論：Blink 的 `CenterIfNeeded` 是：整個看得到時不捲，部分看得到時捲到最近的邊，看不到時置中。`<input>`、`<textarea>` 和 editing host 捲的是 caret，不是元素。`docs/limitations.md` 原本「部分在畫面裡的元素也捲（Chrome 152 不捲）」的說法，來自 WPT 的 `contenteditable-near-cursor`：那裡的元素是 editing host，Chrome 看的是 caret，caret 已經看得到。

## 原本的做法（開始實作前）

- **`<style>` 和 `<link>`**：
  - `dom` 只有元素的文字、屬性，和 rare data 的 `style_sheet_disabled` 旗標（`<style>.disabled`）。
  - `set_tree_scope_style_sheets_changed` 在文字、`media`、`type`、`rel`、`href`、`disabled` 改變和插入、移除時標記 tree scope。
- **`<link>` 的文字**：在 renderer 的 `FrameView::linked_sheets`（`style::LinkedStyleSheets`，`NodeId` → 文字和網址，加上版本號）。每個 `<link>` 只載入一次（`requested_links`）。`stylesheet_link_href` 只收這樣的 `<link>`：
  - `rel` 有 `stylesheet`、沒有 `alternate`；
  - 沒有 `disabled`；
  - CSS 的 `type`；
  - `href` 不空。
- **style engine 的 `collect_document_sheets`**：
  - 文字（`style_sheet_texts`）和上次一樣的沿用解析結果，否則 `Stylesheet::parse`。
  - 所有 sheet 合成一個 `RuleSet`（一個頁面可能有幾百個 `<style>`）。
  - 規則變了的照規則做失效（`changed_rules`、`RuleSetInvalidation`）。
  - `media` 在收集時解析，不符合的 sheet 留著、規則不用。
  - `title` 不看：有 title 的 sheet 全部套用。
- **shadow tree**：
  - `<style>` 解析成 `Arc<StyleSheetContents>`，以（文字，base URL）共用（`style_texts`）。
  - adopted 的排在後面。
  - 比較用 `Arc::ptr_eq` 加 `media`。
- **constructed stylesheet**（`dom` 的 `css_style_sheet.rs`）：
  - `CssStyleSheet` 持有 `RefCell<Arc<StyleSheetContents>>`、`media`、`disabled`、`adopters`。
  - `insert_css_rule`、`delete_css_rule`、`set_css_rule`、`replace_css_style_sheet_contents` 產生新的內容，再 `css_style_sheet_changed` 標記每個 adopter。
  - `dom_api` 的 `ownerNode`、`href`、`title`、`parentStyleSheet`、`ownerRule` 都是 `null`。
- **`css::Stylesheet` 和 `StyleSheetContents` 是兩種型別**：文件的 sheet 用前者，shadow tree 和 constructed 的用後者。第 3 段把文件的 sheet 也換成 `StyleSheetContents`。
- **`focus()`**：`dom_api` 的 `focus` 用 `ScrollAlignment::Nearest`。renderer 的 `scroll.rs` 的 `expose_axis` 在整個看得到、或元素比視窗大且蓋住視窗時不捲。
- **數學函式**：`css` 的 `calc.rs` 只有 `calc()`、`min()`、`max()`、`clamp()`。
- **`DOMRectReadOnly`**：`dom_api` 的 `geometry.rs`，IDL 沒有 `toJSON`。
  - 宣告表其實已經能回傳 dictionary（`Value::Dictionary`），但 `dom_js` 會照名稱排序，`toJSON` 要的是屬性的順序。
  - `Value::Record` 保留順序。
  - `docs/limitations.md` 寫「宣告表還不能回傳 dictionary」已經過時。

## 用到的地方

- **Speedometer 3.1**（`../third_party/speedometer/resources`，grep）：
  - 沒有 `layerX`、rect 的 `toJSON`；
  - CSS 裡沒有 `round()`（JS 的 `Math.round` 不算）；
  - `document.styleSheets` 只在 react-stockcharts 的 build 裡：匯出 SVG 時收集 CSS 的函式。
  - CodeMirror（editors）有 `.sheet`：`style-mod` 在有 `adoptedStyleSheets` 時用它，kun 有，所以不走到。
- **WPT**（`../third_party/wpt`，grep `document.styleSheets` 或 `.sheet`）：367 個 `.html`。最多的目錄：
  - `css/css-counter-styles/cssom/`（19）；
  - `html/semantics/document-metadata/`（11）；
  - `css/css-conditional/container-queries/`（11，kun 沒有 container query，還是跑不了）；
  - `css/css-fonts/parsing/`（9）；
  - `css/css-page/`。
