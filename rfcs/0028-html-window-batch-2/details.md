# RFC 0028 的細節：HTML 元素和 Window 的雜項（H2）

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-10-10 的 `python3 scripts/ci.py speedometer --inventory`（kun `90179773faa4`，Chrome for Testing 152.0.7977.82）：Chrome 跑完 32 個 suite 時用到、kun 沒有的成員，數 default 的 20 個 suite 裡有幾個用到（`target/speedometer-results/inventory.json` 的 `missing.api` 和各 suite 的 `api`）。只列這份的範圍；其他代號的（Canvas、SVG、shadow DOM、`Selection`、`MessageChannel`、observer）在 RFC 0012 的 `progress.md`。

| 成員 | suite 數 | 這份的哪一段 |
| --- | --- | --- |
| `HTMLElement.contentEditable` | 7 | 2（reflection；編輯是 P） |
| `Node.isConnected` | 6 | 2 |
| `HTMLAnchorElement.href=` | 6 | 3 |
| `HTMLTemplateElement.content` | 5 | 2 |
| `HTMLImageElement.src=`、`Image.src=` | 3 | 4 |
| `HTMLElement.title=` | 3 | 2 |
| `WebKitMutationObserver.observe` / `disconnect` / `takeRecords` | 3 | 2 |
| `Document.dir` | 2 | 2 |
| `Document.getElementsByName` | 2 | 2 |
| `Crypto.randomUUID`、`window.crypto` | 2 | 5 |
| `window.requestIdleCallback` | 2 | 5 |
| `HTMLImageElement.complete` / `alt=` / `width=` / `height=`（和 `Image` 的） | 2 | 4 |
| `HTMLImageElement.decode`、`Image.decode` | 1 | 4 |
| `HTMLAnchorElement.rel` / `rel=` | 1 | 3 |
| `Document.visibilityState`、`Document.prerendering` | 1 | 2（`prerendering` 不做） |
| `HTMLLIElement.type`、`HTMLUListElement.type` | 1 | 不在範圍（list 的元素和 S3 一起） |
| `HTMLBodyElement.onfocus` | 1 | 不在範圍（`<body>` 的 window 事件 handler，另外的工作） |
| `console.createTask` | 2 | 不做（Chrome 的非標準 API） |
| `console.assert` | 1 | 1 |

`console.error`、`warn` 不在表裡：Chrome 跑的時候沒有 suite 呼叫它們（沒有錯誤可印）。kun 上 NewsSite-Next、NewsSite-Nuxt、Charts-chartjs 停在 `console.error is not a function`，是因為 kun 先有了別的錯誤（`progress.md` 的那張表）。

## `console` 的三種做法

V8 自己就有 `console`（V8 的 bootstrapper 在每個 context 的 global 上裝好，方法是 `src/builtins/builtins-console.cc` 的 builtin），方法把引數交給 embedder 設的 `v8::debug::ConsoleDelegate`；沒有 delegate 時什麼都不做。Chrome 的 delegate 是 V8 inspector 的 `V8Console`，它格式化之後經過 `V8InspectorClient::consoleAPIMessage` 交給 Blink（`MainThreadDebugger`），變成 `ConsoleMessage`。kun 現在在 `bindings` 手寫一個只有 `log` 的 `console`，蓋掉 V8 的。

| | B1 V8 的 `console` 加 delegate | B2 擴充手寫的 | B3 宣告表的 namespace |
| --- | --- | --- | --- |
| 和 Chromium 的對應 | 一樣（方法、引數的處理、`console.createTask` 都是 V8 的） | 不同：方法自己寫 | 不同：Blink 的 `console` 不在 IDL 的 binding 裡 |
| 要動的 | rusty_v8 的 fork 加 `ConsoleDelegate` 的 binding（C++ 的 trampoline 轉給 Rust），要重編 V8（約 40 分鐘，共用的 artifact 跟著換）；`bindings` 實作 delegate 和 formatter | `bindings` 的 `console` 加方法、formatter、count / timer / group 的狀態 | 宣告表多 namespace 的形式（`dom_js`、`kun:dom` 的產生器都要），formatter 在 `dom_api` |
| wasm 的 guest | 拿不到（V8 的） | 拿不到（只有 JS） | 引數是 `any`，照 RFC 0008 只給 JS，一樣拿不到 |
| 之後接 DevTools | delegate 就是入口 | 要改成 B1 | 要改成 B1 |
| 工作量 | 最大（fork 加 V8 重編） | 最小 | 中（新的宣告表形式；`CSS` namespace 之後也用得到） |

2026-10-10 選了 B3（`README.md` 的「已決定的事項」第 2 條）。

## Chrome 怎麼把 `console` 印到終端機

2026-10-10，Chrome for Testing 152.0.7977.82，`--headless=new --enable-logging=stderr`（`--v=0`）；每一行是 `[pid:tid:時間:INFO:CONSOLE:行號] "訊息", source: 網址 (行號)`。測的頁面是 `console` 的每個方法各呼叫一次：

| 呼叫 | 印出的訊息 |
| --- | --- |
| `console.log("plain", 1, true, null, undefined, {a:1, b:[1,2]}, [1,"x"], function f(){})` | `plain 1 true null undefined [object Object] 1,x function f(){}` |
| `console.log("%s is %d …", "x", 42.9, …)` | `%s is %d and %i, %f; %o %O %c styled x 42 42 1.5 [object Object] [object Object] color: red rest 7`：**不套用** `%s` 等（DevTools 才套用），引數照 `toString` 接在後面 |
| `console.info`、`warn`、`error`、`debug` | 和 `log` 一樣，沒有等級的標記（都是 `INFO:CONSOLE`）；`Error` 是它的 `toString`（`Error: boom`） |
| `console.assert(true, …)` | 不印 |
| `console.assert(false, "yes %s", "fmt")` | `yes %s fmt` |
| `console.assert(false)` | `console.assert` |
| `console.count()` 兩次、`count("k")`、`countReset()` 再 `count()` | `default: 1`、`default: 2`、`k: 1`、`default: 1` |
| `console.group("g1")`、`groupCollapsed("g2")` | `g1`、`g2`，之後的訊息**沒有縮排** |
| `console.groupEnd()` | `console.groupEnd` |
| `console.time("t")`、`timeLog("t", "x")`、`timeEnd("t")` | `t: 0.005859375 ms x`、`t: 0.02294921875 ms` |
| `console.timeEnd("nope")` | `Timer 'nope' does not exist` |
| `console.dir({a:1})`、`console.table([{a:1}])` | `[object Object]` |
| `console.trace("tr")` | `tr`（沒有 stack） |
| `console.log(Symbol("s"), 10n, -0, NaN)` | `Symbol(s) 10n 0 NaN` |

再測一次沒有引數、警告和物件的情況（同一天、同一個版本）：

| 呼叫 | 印出的訊息 |
| --- | --- |
| `console.log()`、`info()`、`clear()`、`table()`、`dir()`（沒有引數） | 不印 |
| `console.countReset("nope")` | `Count for 'nope' does not exist` |
| `console.time("x")` 兩次 | 第二次 `Timer 'x' already exists` |
| `console.timeLog("nope")` | `Timer 'nope' does not exist` |
| `console.group()`、`groupCollapsed()`、`trace()`（沒有引數） | `console.group`、`console.groupCollapsed`、`console.trace` |
| `console.dirxml("d", 1)` | `d 1` |
| `console.log({toString(){return "custom"}}, [1,[2,3]], …, Object.create(null))` | `[object Object] 1,2,3 true [object Object]`：物件**不**呼叫它的 `toString`，寫成 `[object` 加 constructor 的名字 `]`；陣列攤平、逗號分開 |
| `console.log(document.createElement("div"))` | `[object HTMLDivElement]` |
| `console.count(undefined)`、`count(null)` | `default: 1`、`null: 1` |
| `console.time()`、`timeEnd()` | `default: 0.001953125 ms` |
| `console.log(1.5e21, 1e-7, -1.25)` | `1.5e+21 1e-7 -1.25`（JS 的 `ToString`） |

再測一次很長、很深的陣列（同一天、同一個版本；wptrunner 跑 `console-log-large-array.any.js` 時，kun 把一千萬個元素都印出來，整套平行跑時被當成 crash 之後量的）：

| 呼叫 | 印出的訊息 |
| --- | --- |
| `console.log(new Array(10000000).fill("x"))`、`new Array(10001)` 的 | 不印（這個引數寫不出來，整個訊息是空的） |
| `console.log("a", new Array(10001).fill("y"), "b")` | `a  b`（寫不出來的引數是空字串） |
| `console.log([new Array(5000).fill(1), new Array(5001).fill(2)])` | 不印：一個引數裡所有陣列的元素加起來超過 10000 |
| `console.log(new Uint8Array(10000000))` | `[object Uint8Array]` |
| 巢狀 33 層的陣列；40 層的 | 全部印出；寫不出來（空的） |
| `console.log("")` | 不印 |

這和 V8 inspector 把引數寫成字串的規則一致（`V8ValueStringBuilder`）：字串、數字、boolean、`null`、`undefined` 照 `ToString`；symbol 是它的描述，BigInt 加 `n`；陣列逐個元素、逗號分開（巢狀的也攤平）；函式、`Date`、`RegExp`、`Error` 照 `ToString`（`Error: boom`）；其他物件是 `[object 名字]`。group 沒有縮排，所以不必記 group 的深度；要記的只有 count 和 timer。

## Chrome 的 global attribute

2026-10-10，Chrome for Testing 152.0.7977.82（`--headless=new`，`file://` 的頁面），每一行是 script 和 `JSON.stringify` 的結果；第 2 段的 `dom_js` 測試（`crates/dom_js/tests/html_element.rs`）用同樣的 script 和這些值比對。和規範不同、或規範沒寫清楚的：

| 成員 | Chrome 152 | 規範 |
| --- | --- | --- |
| `autocapitalize` 不認得的值（`"bogus"`） | `"sentences"`（WPT 的 `autocapitalize.html` 也這樣要求） | invalid value default 是 default 狀態，回傳 `""` |
| `autocapitalize`：`<form autocapitalize=words>` 裡沒有 attribute 的 `<input>`；`<img>` | `"words"`；`""` | 同（inheriting element 才用 form owner 的） |
| `isContentEditable`：`contenteditable` 的元素不在文件裡、`display: none` 的和它的子元素 | 都是 `true` | 沒寫（Blink 從 style 算：`EnsureComputedStyle`） |
| `isContentEditable`：SVG 的 `<g contenteditable=true>` 裡的 `<text>` | `undefined`（不是 `HTMLElement`） | 同 |
| `contentEditable = ""` | `SyntaxError`：`The value provided ('') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.` | 同（空字串不是關鍵字） |
| `hidden = "false"`、`{}`、`1` | attribute 是 `""`，讀回 `true` | 同 |
| `hidden = 0`、`-0`、`NaN`、`""`、`null`、`undefined` | 拿掉 attribute | 同 |
| `spellcheck` 沒有 attribute | `true` | UA 決定 |
| `document.dir`：`<html dir=RTL>` | `"rtl"`；`document.dir = "RTL"` 寫進 `<html>` 的是 `"RTL"` | 同 |
| `document.getElementsByName("q")` | `NodeList`，每次同一個物件；SVG 的 `<g name=q>` 不算 | 同（同一個物件規範沒要求） |
| `HTMLTemplateElement.prototype` | `content`、`shadowRootMode`、`shadowRootDelegatesFocus`、`shadowRootClonable`、`shadowRootSerializable`、`shadowRootCustomElementRegistry`、`shadowRootReferenceTarget`、`shadowRootSlotAssignment` | kun 只做 `content`（`README.md` 的「詳細設計」） |
| `'onvisibilitychange' in document`、`in window`、`in HTMLElement.prototype` | `true`、`false`、`false` | 同（只有 `Document` 有） |

## Chrome 的 `<a>`

2026-10-10，Chrome for Testing 152，`<base href="http://example.com/dir/page.html">` 的頁面。第 3 段的 `dom_js` 測試（`links_url_members`、`links_own_members_and_the_documents_links`、`a_setter_that_cant_change_the_url_leaves_the_href`）用同樣的 script 和這些值比對，下面三處除外，照 URL Standard 和 HTML。

| script | Chrome 152 | 規範（kun） |
| --- | --- | --- |
| `a.href = "mailto:x@y"; a.protocol = "ftp"` | `ftp://x@y/` | `mailto:x@y`（special 和其他 scheme 不能互換；WPT 的 `setters_tests.json`） |
| `a.href = "file:///tmp/x"`；`a.origin` | `file://` | `null`（URL Standard：`file:` 的 origin 是 opaque；WPT 的 URL 測試、kun 的 `URL` 也是） |
| `a.href = " file:///x "; a.username = "u"`（`password`、`port` 也是） | `href` 變成 `file:///x` | `href` 不變（HTML：不能有帳號密碼和 port 時直接返回，不 update href） |
| `a.href = " mailto:x@y "; a.protocol = "https"` | `https://x@y/` | `mailto:x@y`（不能換 scheme，但照樣 update href） |
| 沒有 `href`：`href`、`origin`、`protocol`、其他 | `""`、`""`、`":"`、`""` | 同 |
| `href="http://["`（解析不了） | `href` 是 `http://[`，`protocol` 是 `:` | 同 |
| `HTMLAreaElement.prototype` | 沒有 `hreflang`、`type`（HTML 有） | 照 Chrome 不做 |
| `document.links` | 有 `href` 的 HTML `<a>` 和 `<area>`（`<map>` 裡的也是），不含 SVG 的 `<a>`；每次同一個 | 同 |

## Chrome 的 `<img>`

2026-10-10，Chrome for Testing 152，`http://` 的頁面（Python 的 `http.server`），`a.png` 是 40×30、`b.png` 是 20×40。每一行是 `[complete, naturalWidth, naturalHeight, width, height, currentSrc, src]`（網址去掉 origin）。4a 的 browser 測試（`crates/browser/tests/navigation.rs` 的 `img_elements`，頁面是 `crates/browser/tests/resources/img-elements.html`）用同樣的 script 和這些值逐行比對。

| 情況 | Chrome 152 |
| --- | --- |
| `new Image()`；`new Image(5, 7)` | `[true,0,0,0,0,"",""]`；`width="5" height="7"`，`[true,0,0,5,7,"",""]` |
| 設了 `src` 的當下、`setTimeout` 之後 | `[false,0,0,0,0,"","/a.png"]`（`currentSrc` 在圖來了之前是空的） |
| `load` 之後 | `[true,40,30,40,30,"/a.png","/a.png"]`，只有 `load`（沒有 `loadstart`、`progress`、`loadend`） |
| 另一個元素設成同一個 `src`（快取） | 當場 `[true,40,30,…]`，`load` 在之後的 task |
| 不是圖、404 | `[true,0,0,0,0,"/bad.png",…]`，`error` |
| `src=""` | 當場 `complete` 是 true，`src` 是文件的網址，`error` |
| 拿掉 `src` | `[true,0,0,0,0,"",""]`，沒有事件 |
| `src="http://["` | `currentSrc` 是 `http://[`，`error` |
| `width="33"`，沒有 box；有 box；`style.width=50px`；`display: none` | `33,40`；`33,66`（長寬比）；`50,100`；`33,40` |
| `width="x" height="-3"` | natural size |
| `decode()`：可用的、壞圖、沒有 `src`、呼叫之後換了 `src` | resolve（`complete` 是 true）；`EncodingError: The source image cannot be decoded.`（後三種） |
| 載入中換 `src` | 只有新的 `load` |
| 同一個 `src` 再設一次 | 再一次 `load` |
| 載入中呼叫 `decode()`，再把 `src` 設成同一個網址（4b 加的） | `decode()` resolve（`complete` 是 true），只有一次 `load` |
| `HTMLImageElement.prototype` | 規範的成員，加上 `x`、`y`、`browsingTopics`（kun 不做） |
| `new Image()` 不在文件裡，慢的圖 | window 的 `load` 等它的 `load` 之後才發（在文件裡的也是） |

## Chrome 的 `srcset` 和 `<picture>`

2026-10-10，Chrome for Testing 152，`--headless=new`，Python 的 `http.server`，每個候選都是一張 40×30 的 PNG。頁面是 `crates/browser/tests/resources/srcset.html`（4b 的 browser 測試 `srcset_and_picture`、`srcset_and_picture_at_2x` 用它逐行比對），`--window-size=800,600` 時 `innerWidth` 是 800（`innerHeight` 457）；`--window-size=400,600` 時 `innerWidth` 是 500（視窗最窄 500）。resize 用 `window.open` 開的視窗的 `resizeTo`（headless 的主視窗不能改大小）。每一格是 `[currentSrc, naturalWidth, naturalHeight, width, height]`。

| 元素 | 800 寬、1x | 800 寬、2x | resize 到 500 寬、1x |
| --- | --- | --- | --- |
| `c1` `x1 1x, x2 2x, x3 3x` | `x1`，40×30 | `x2`，20×15 | `x1` |
| `c2` `src=s` 加 `x2 2x` | `s`（`src` 是 1x） | `x2`，20×15 | `s` |
| `c3` `w200`…`w1600`，沒有 `sizes`（`100vw`） | `w800`，40×30 | `w1600`，20×15 | `w800`，25×18（同一張，密度變了，不重新載入） |
| `c4` 同上，`sizes="(max-width: 500px) 100px, 50vw"` | `w400`，40×30；有時 `w800`，20×15 | `w800`，20×15；有時 `w1600`，10×7 | `w800`，5×3（已經載入的比較密的）；有時 `w200`，20×15 |
| `c5` `w200, w400, w800`，`sizes="150px"` | `w200`，30×22；有時 `w800`，7×5 | `w400`，15×11；有時 `w800`，7×5 | `w800`，7×5；有時 `w200`，30×22 |
| `c6` `<picture>`，`media` 寬的、窄的 | `wide` | `wide` | `narrow`；把兩個 `media` 都改成不符合之後是 `<img>` 的 `src` |
| `c7` `<picture>`，`type="image/avif"`、`image/png` | `avif`（Chrome 解得了 AVIF；kun 跳過它，是 `png`） | `avif` | `avif` |
| `c8` `<picture>`，`p100`、`p300`、`p600`，`sizes="(min-width: 700px) 300px, 100px"` | `p300`，40×30 | `p600`，20×15 | `p300`，13×10；有時 `p100`，40×30 |
| `c9` `srcset="bogus 1q" src=s` | `s`（壞的描述子整個候選不要） | `s` | `s` |
| `c10` `srcset=""`，沒有 `src` | `""`，`complete` 是 true，沒有事件 | 同左 | 同左 |
| `c11` `big 2x, small 0.5x` | `big`，20×15（1x 時取第一個不小於 1 的） | `big` | `big` |
| `c12` `<picture>` 的 `<source>` 不符合、`<img src=fallback srcset="x2 2x">` | `fallback` | `x2`，20×15 | `fallback` |

「有時」的那些看時序：Blink 選候選時，已經載入（記憶體快取裡、`IsLoaded`）而比較密的優先，preload scanner 先抓好的、或別的元素先載入的圖就會被選上。同一頁跑 8 次，2x 的 `c4` 5 次是 `w1600`；1x 跑 4 次，2 次 `c4`、`c5` 是 `w800`；resize 跑 8 次，4 次留著 `w800`、`p300`（三個一起）。kun 沒有 preload scanner，第一次載入時沒有圖已經載入，結果是表的第一個；resize 之後照 Blink 的規則留著比較密的。

其他：`<img srcset="">`、`<picture><img></picture>`（都沒有 `src`）沒有 `error`，`<img srcset="" src="">`、`<picture><img src=""></picture>` 有（WPT 的 `null-image-source.html` 也是）。新建的 iframe 在插入時同步發的 `onload` 裡，`innerWidth` 已經是 300，`<img srcset="… 256w">` 的 natural size 是 300×300；`src` 的 iframe 裡的 `<picture>`，`media="(min-width: 200px)"` 的 `<source>`（iframe 300 寬），只抓了那一張，沒有抓 `<img>` 的 `src`；`<base>` 在那之後改了，那份 `about:blank` 的網址照樣照建立時的 base URL 解析。`matchMedia` 和 kun 的 media query 比過一組（寬度、高度、範圍寫法、`orientation`、`resolution`、`aspect-ratio`、`prefers-*`、`not`、`or`、不認得的 feature），只有 `(hover)` 不同（headless 是 `none`）。

## Chrome 的 lazy loading

2026-10-10，Chrome for Testing 152，`--headless=new`，Python 的伺服器記下每個請求（哪些圖抓了）。

**距離**：頁面放一排 `loading=lazy`、10×10 的圖，往下每隔一段、往右每隔一段，看哪些被抓。`--window-size=800,600`（`innerWidth` 800、`innerHeight` 513；`clientWidth` 785、`clientHeight` 498，扣掉捲軸）和 `1000,900` 兩種視窗，結果相對 viewport 一樣：

| 方向 | 抓了 | 沒抓 |
| --- | --- | --- |
| 往下（圖的上緣到 `clientHeight` 的距離） | 1245px | 1255px |
| 往右（圖的左緣到 `clientWidth` 的距離） | 615px | 665px |

上下是 Chromium 的 `lazyImageLoadingDistanceThresholdPx4G`（1250px，headless 的網路類型是 4G）；左右是一半。kun 用 1250px、625px，從 viewport（kun 沒有捲軸）的邊算。

**行為**（頁面是 `crates/browser/tests/resources/lazy.html`，4c 的 browser 測試 `lazy_images` 用它逐行比對；三次結果都一樣）：

| 情況 | Chrome 152 |
| --- | --- |
| viewport 裡的 lazy 圖（慢的，500ms） | window 的 `load` 先發，圖的 `load` 在後；恢復載入之後也不延後 `load`（另一頁：慢 400ms 的一般圖和慢 1500ms 的 lazy 圖，window 的 `load` 在 402ms） |
| 遠在下面的 | `[complete, currentSrc, naturalWidth]` 是 `[false,"",0]`，沒有請求；捲到附近之後載入 |
| `display: none` | 從來不抓 |
| 不在文件裡的 `new Image()`，`loading=lazy` | 從來不抓、沒有事件 |
| `loading` 改成 `eager` | 立刻載入；之後捲到它也不再載一次（`load` 一次） |
| iframe 在畫面下方 2500px，裡面在 iframe 頂端的 lazy 圖 | 不載（對最上層的 viewport 算）；捲到附近才載 |
| 側邊 500px 的；700px 的 | 載；不載 |
| `loading=lazy sizes="auto" width=10`，`50w`、`51w` | `50w`，natural size 8（寬度 10）；寬度改成 100 之後重新選 `51w`，natural size 78 |
| 同上，加 `padding: 0 40px` | `50w`（content box 的 10，不含 padding） |
| 不是 lazy 的 `sizes="auto, 10px"`；layout 之後才設 `srcset` 的 `sizes="auto"` | 都是 `100vw`（`51w`，natural size 627），不是 `10px`，也不是寬度 |

## Chrome 的 `crypto`

2026-10-10，Chrome for Testing 152，`http://127.0.0.1` 的頁面（`isSecureContext` 是 true）。`dom_js` 的 `crypto` 測試照這些值比對（`TypeError` 的訊息是 kun 自己的，只比類別）。

| 呼叫 | Chrome 152 |
| --- | --- |
| `window.crypto === crypto`；`Crypto.prototype` 的成員 | true；`constructor getRandomValues randomUUID subtle`（kun 沒有 `subtle`） |
| `new Crypto()`；拿下來單獨呼叫 | `TypeError`（Illegal constructor、Illegal invocation） |
| `getRandomValues(new Uint8Array(8))` | 回傳同一個陣列，填好了 |
| `Int8`、`Uint8`、`Uint8Clamped`、`Int16`、`Uint16`、`Int32`、`Uint32`、`BigInt64`、`BigUint64` 的陣列 | 都可以 |
| `Float16`、`Float32`、`Float64`、`DataView` | `DOMException` `TypeMismatchError`（code 17）：「The provided ArrayBufferView is of type 'Float32', which is not an integer array type.」；長度超過的 float 陣列也是這個（先看型別） |
| 65536 byte；65537 byte；`Uint32Array(16385)` | 可以；`QuotaExceededError`（它自己的類別，code 22，`quota` 和 `requested` 是 null）：「The ArrayBufferView's byte length (65537) exceeds the number of bytes of entropy available via this API (65536).」；同上，65540 |
| `ArrayBuffer`、`null`、數字、沒有參數 | `TypeError` |
| detach 的 view | 回傳它，長度 0 |
| buffer 中間的 view（offset 2、長度 4） | 只寫 view 的範圍 |
| `randomUUID()` | 36 個字元，第 4 版，variant 是 `8`–`b` |
| `QuotaExceededError` | 原型鏈接在 `DOMException` 上；`length` 0；`new QuotaExceededError()` 的 `name` 是 `QuotaExceededError`、`code` 22、`quota` / `requested` 是 null；`{quota: 5, requested: 7}` 照給的；負數、`requested` 小於 `quota` 都不丟（WebIDL 說是 `RangeError`）；`quota: NaN` 是 `TypeError`（「The provided double value is non-finite.」）；`new DOMException("x", "QuotaExceededError")` 的 code 是 22，不是它的 instance |

## Chrome 的 `requestIdleCallback`

2026-10-10，Chrome for Testing 152（headless）。`browser` 的 `idle_callbacks` 測試的頁面（`crates/browser/tests/resources/idle.html`）在 Chrome 跑了幾次，結果每次一樣，測試比對那一行；另外量了 deadline 會不會變、跨 realm 的 `timeRemaining`。

| 情況 | Chrome 152 |
| --- | --- |
| 沒有東西要畫，連續排三個（每個在前一個裡排） | 每個都剩 0 到 50 ms，`didTimeout` false |
| `requestAnimationFrame` 一直在跑（300 ms） | 剩不到一個 frame（< 17 ms） |
| `{timeout: 100}`，之後每 20 ms 一個 `setTimeout(0)` 忙 300 ms | 過了 100 ms 才跑、還在忙的時候就跑；`didTimeout` true，`timeRemaining()` 是 0 |
| 排 a、b、c、d，取消 c | 照 a、b、d 的順序 |
| `IdleDeadline.prototype` 的成員；`new IdleDeadline()`；`length` | `constructor didTimeout timeRemaining`；`TypeError`；`requestIdleCallback.length` 1，`IdleDeadline.length` 0 |
| callback 裡先記 `timeRemaining()`，再排 `requestAnimationFrame` 或 `setTimeout(…, 10)`，再記一次 | 兩次一樣（例如 50.0 / 50.0、13.9 / 13.9）：deadline 在 period 開始時就固定。WPT 的 `deadline-max-rAF-dynamic.html`、`deadline-max-timeout-dynamic.html` 要它變短，Chrome 也不通過 |
| 子 frame 的 `IdleDeadline.prototype.timeRemaining.call(deadline)` | 可以，和 `deadline.timeRemaining()` 差 0（kun 是 `TypeError`：`glue.js` 的私有欄位每個 realm 一份） |

## Chrome 的 `Blob` 和 `blob:` 網址

2026-10-10，Chrome for Testing 152，`http://127.0.0.1` 的頁面。同步的部分是 `dom_js` 的 `blob` 測試比對的（`TypeError` 的訊息是 kun 自己的，只比類別）；非同步的部分是 `browser` 的 `blobs` 測試的頁面（`crates/browser/tests/resources/blob.html`），在 Chrome 跑了三次，每次一樣。

| 呼叫 | Chrome 152 |
| --- | --- |
| `Blob.prototype` 的成員；`Blob.length` | `arrayBuffer bytes constructor size slice stream text textStream type`（kun 沒有 `stream`、`textStream`）；0 |
| `new Blob(['\u00e9'])`、`['\ud800']` 的 `size` | 2、3（UTF-8，lone surrogate 是 U+FFFD） |
| typed array、`ArrayBuffer`、`DataView` 的 part | 它看到的 byte 數（`Uint16Array([1,2,3])` 是 6）；建立之後改 buffer 不影響 |
| `[123, null, undefined, {}]` | 31（都轉成字串） |
| `new Blob('abc')`、`({length: 2})`、`null`、`[1], 'x'`；`Blob()` | `TypeError`（不是 iterable、options 不是 dictionary、沒有 `new`）；`undefined` 是空的 |
| `endings: 'native'` 的 `'a\r\nb\rc\n'` | 6（Linux 的 `\n`）；`transparent` 是 7；`'bad'` 是 `TypeError` |
| type `'Text/HTML;Charset=UTF-8'`、`'caf\u00e9'`、`'a\u0019b'` | `text/html;charset=utf-8`；空的；空的 |
| `slice(1, 4)` 的 type | 空的（不繼承）；`slice(0, 3, 'A/B')` 是 `a/b`，`null` 是 `"null"` |
| `slice(-2)`、`(-10, -3)`、`(4, 2)`、`(NaN, Infinity)`、`(1.5, 3.5)`、`(2.5)` | 2、3、0、6、2、4（`[Clamp]`：四捨六入） |
| resizable 的 `ArrayBuffer` 或它的 view：`Blob` 的 part、`Response` 的 body、`getRandomValues` | `TypeError`（「The provided ArrayBuffer value must not be resizable.」） |
| `Response` 的 body 是有 type 的 blob；沒有 type 的、buffer | `Content-Type` 是那個 type；沒有 `Content-Type` |
| `blob.text()` 和之前排的 microtask | microtask 先，`text()` 在之後的 task；開頭的 BOM 拿掉 |
| `Response.blob()` 的 type，`Content-Type: Text/HTML; Charset="UTF-8"` | `text/html; charset="utf-8"`（header 的值轉小寫，不是 Fetch 序列化的 `text/html;charset=UTF-8`） |
| `URL.createObjectURL(blob)` | `blob:http://127.0.0.1:8766/` 加第 4 版的 UUID；不是 `Blob` 是 `TypeError` |
| `fetch(blobURL)` | 200、`OK`、`Content-Type` 是 blob 的 type（沒有 type 是空字串、`invalid` 照樣）、`Content-Length`、`basic`；`#fragment` 一樣；`POST` 是 `TypeError` |
| `Range: bytes=1-3`；`BYTES=0-5`；`bytes= 0-0`；`bytes=0-0,1-1` | 206、`Content-Range: bytes 1-3/5`；206（單位不分大小寫，WPT 要分，Chrome 也不通過）；206；`TypeError` |
| 先 `fetch()` 再撤銷；先撤銷再 `fetch()` | 拿得到；`TypeError` |
| `<img>`、`<script>`、iframe 設好 `src` 之後同一個 task 裡立刻撤銷 | 圖載不到（0x0：`<img>` 在 stable state 之後才解析網址），沒撤銷時是 1x2；script 和 iframe 載得到 |
| iframe 的 realm 建的網址，iframe 移除之後；同源的另一份文件撤銷 | `TypeError`；撤銷得掉 |
| `location.href = blobURL` 之後立刻撤銷 | 導覽成功，文件的 origin 是頁面的 |

## 參考資料

- Console 規範：https://console.spec.whatwg.org/
- HTML：`HTMLElement`（global attribute）、`HTMLHyperlinkElementUtils`、`HTMLImageElement` 的「update the image data」、`requestIdleCallback`（https://w3c.github.io/requestidlecallback/）
- File API（https://w3c.github.io/FileAPI/）：`Blob`、blob URL store；Fetch 的 blob 的 scheme fetch
- WebCrypto 的 `getRandomValues`、`randomUUID`；File API 的 `Blob` 和 blob URL store。
