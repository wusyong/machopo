# RFC 0079 的細節

## 量測：表的成本和 `getElementById`

2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370），Chrome for Testing 152.0.7977.82（`--headless=new`）和 kun（release，`main` 的 `1a230598`），經過 `scripts/bench/common.py` 的 `ChromePipe`、`KunPipe`，在 `about:blank` 裡用 `performance.now()` 量一個迴圈，除以次數（先跑一小段暖身）。Chrome 的 `performance.now()` 精度比較粗，只看數量級。

| 操作（每次，ns） | Chrome 152 | kun |
| --- | ---: | ---: |
| 建一個有 `id` 的 div、插入、移除 | 2676 | 2044 |
| 建一個沒有 `id` 的 div、插入、移除 | 755 | 1502 |
| 改連上 document 的 div 的 `id` | 141.5 | 255 |
| 改沒連上的 div 的 `id` | 88 | 252 |
| 改連上 document 的 `<img>` 的 `name` | 505 | 271 |
| `getElementById`，100 個有 id 的元素，找最後一個 | 75 | 4010 |
| 同上，找不到 | 60 | 3500 |
| `getElementById`，1,000 個，找最後一個 | 60 | 31525 |
| 同上，找不到 | 45 | 31130 |
| `getElementById`，10,000 個，找最後一個 | 50 | 325050 |
| 同上，找不到 | 50 | 323250 |

Chrome 的表也有成本：改連上的元素的 `id` 比沒連上的多約 50 ns，插入有 `id` 的元素也多很多。kun 的 `getElementById` 每次走整棵樹，和元素的數量成正比。

### 第 1 段做完之後

同樣的量測（kun 是 `0079` 上第 1 段的 release build）：

| 操作（每次，ns） | 之前 | 之後 |
| --- | ---: | ---: |
| 建一個有 `id` 的 div、插入、移除（每次換一個 `id`） | 2044 | 2207 |
| 建一個沒有 `id` 的 div、插入、移除 | 1502 | 1535 |
| 改連上 document 的 div 的 `id` | 255 | 325.5 |
| 改沒連上的 div 的 `id` | 252 | 255 |
| 改連上 document 的 `<img>` 的 `name` | 271 | 374.5 |
| `getElementById`，100 個，找不到 | 3500 | 235 |
| `getElementById`，10,000 個，找不到 | 323250 | 250 |

表的維護比決定時估的（一次查和改，約 30–60 ns）多：改 `id` 多約 70 ns（Chrome 多約 50 ns），插入或移除一個有 `id` 的元素各多約 80 ns（拿出、放回名稱要配置和釋放字串），改 named element 的 `name` 多約 100 ns（另外記放進去時的名稱）。第一版多約 150 ns：表變空時連同 scope 的表一起拿掉，下一次又建一份，量的頁面只有一個有 `id` 的元素，每次都這樣；改成表變空時留著之後是上表的數字。名稱的雜湊從 SipHash 換成 FxHash、拿出時不複製名稱，量不出差別（都留著：頁面自己製造碰撞只拖慢自己，和 Blink 的 `AtomicString` 一樣）。


## 第 2 段：Chrome 152 的 named access

2026-10-23，同一台機器、同樣的 Chrome。一頁有這些元素：`<div id=box>`、SVG 的 `<rect id=svgRect>`、`<img name=pic>`、`<form name=f>`、`<embed name=emb>`、`<object name=obj>`、`<a name=anchor>`、`<div name=divName>`、`<input name=inp>`、`<img name=both id=imgId>`、兩個 `id=dup`、`<img name=mixed>` 和 `<div id=mixed>`、`<iframe name=frameName id=frameId>` 和 `<div id=frameName>`、`<div id=alert>`、`<div id=declared>`（加上 `var declared = 1`）、shadow tree 裡的 `<p id=shadowed>` 和 `<form name=shadowForm>`、`<img name="">`；`onload` 之後讀 `window[name]`，和 named properties object 的形狀、改 DOM 之後的結果。kun 是 `0079` 上第 2 段的 dev build（`window.alert` kun 沒有，`<embed>`、`<object>` 在 kun 是 `HTMLElement`）。

| 名稱或檢查 | Chrome 152 | kun |
| --- | --- | --- |
| `box` | `[object HTMLDivElement]#box` | `[object HTMLDivElement]#box` |
| `svgRect` | `[object SVGRectElement]#svgRect` | `[object SVGRectElement]#svgRect` |
| `pic` | `[object HTMLImageElement]<img>` | `[object HTMLImageElement]<img>` |
| `f` | `[object HTMLFormElement](0)<form>` | `[object HTMLFormElement](0)<form>` |
| `emb` | `[object HTMLEmbedElement]<embed>` | `[object HTMLElement]<embed>` ← |
| `obj` | `[object HTMLObjectElement]<object>` | `[object HTMLElement]<object>` ← |
| `anchor` | `undefined` | `undefined` |
| `divName` | `undefined` | `undefined` |
| `inp` | `undefined` | `undefined` |
| `both` | `[object HTMLImageElement]#imgId` | `[object HTMLImageElement]#imgId` |
| `imgId` | `[object HTMLImageElement]#imgId` | `[object HTMLImageElement]#imgId` |
| `dup` | `[object HTMLCollection](2)` | `[object HTMLCollection](2)` |
| `mixed` | `[object HTMLCollection](2)` | `[object HTMLCollection](2)` |
| `frameName` | `frameWindow` | `frameWindow` |
| `frameId` | `[object HTMLIFrameElement]#frameId` | `[object HTMLIFrameElement]#frameId` |
| `alert` | `[object Function](0)` | `[object HTMLDivElement]#alert` ← |
| `declared` | `1` | `1` |
| `shadowed` | `undefined` | `undefined` |
| `shadowForm` | `undefined` | `undefined` |
| `nothere` | `undefined` | `undefined` |
| `(空)` | `undefined` | `undefined` |
| `WP` | `[object WindowProperties]` | `[object WindowProperties]` |
| `WPproto` | `true` | `true` |
| `own` | `"no"` | `"no"` |
| `desc` | `true,false,true,[object HTMLDivElement]#box` | `true,false,true,[object HTMLDivElement]#box` |
| `keys` | `false` | `false` |
| `in` | `true` | `true` |
| `hasOwnWP` | `true` | `true` |
| `names` | `false` | `false` |
| `del` | `false` | `false` |
| `delStrict` | `throw TypeError` | `throw TypeError` |
| `define` | `throw TypeError` | `throw TypeError` |
| `defineNew` | `throw TypeError` | `throw TypeError` |
| `setWP` | `throw TypeError` | `throw TypeError` |
| `dupLive` | `2->3 same=true item0=[object HTMLDivElement]#dup named=[object HTMLDivElement]#dup` | `2->3 same=true item0=[object HTMLDivElement]#dup named=[object HTMLDivElement]#dup` |
| `afterRename` | `undefined,[object HTMLDivElement]#renamed` | `undefined,[object HTMLDivElement]#renamed` |
| `assignedFirst` | `7` | `7` |
| `later` | `[object HTMLDivElement]#later typeof=object` | `[object HTMLDivElement]#later typeof=object` |
| `ifr` | `frameWindow len=1` | `frameWindow len=1` |
| `emptyName` | `undefined` | `undefined` |

查全域名稱的成本（release build，`performance.now()` 量一個迴圈，每次 ns；頁面有 10 個或 10,000 個有 `id` 的元素）：

| | Chrome 152 | kun |
| --- | ---: | ---: |
| `typeof nothere`，10 個 | 183 | 125 |
| `window.nothere`，10 個 | 202 | 330 |
| `window.e5`（有），10 個 | 398.5 | 353 |
| `typeof nothere`，10,000 個 | 104 | 110.5 |
| `window.nothere`，10,000 個 | 95 | 295 |
| `window.e5`（有），10,000 個 | 255.5 | 313 |

和元素的數量無關：查表，不走樹。
