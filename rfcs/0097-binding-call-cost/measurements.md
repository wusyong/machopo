# RFC 0097 的量測結果：對 Chrome 的收尾（2026-11-13）

這份是 `README.md` 的補充，放 4m-3 做完時對 Chrome 量的整份結果。各段的前後對照和當時的說明在 `progress.md`；量法的由來在 `details.md`。

## 量法

- machopo：branch `0097`，字串參數的 store forwarding 修好之後（下面「建節點的頁的退步」；逐條指令那一節是修之前的 `747d0ae`），`perf` profile 編的 `dom_js` 測試 binary（`cargo test -p dom_js --test it --profile perf --no-run`），`MACHOPO_BENCH_WINDOW` 在 window 的 realm 裡跑 `bench::bench_calls`。
- Chrome：本機裝的 Chromium（`chromium-browser --headless=new`）。
- 機器：開發機（Linux、AMD Ryzen AI 9 HX 370），`taskset` 釘一顆核心，各跑三次取最小。
- 時間：`prototypes/chrome-compare.py`（腳本是 `prototypes/chrome-compare.js`），每項是每次呼叫的 ns。
- 指令數：`prototypes/chrome-instructions.py`，`perf stat -e instructions:u` 量 2000 萬次和 6000 萬次的迴圈、相減再除，再減掉空迴圈。這個量法在幾條指令的尺度上會晃：Chrome 的 `nodeType` 這次是 66.9，4k-G 那次是 72.9；修好之後重跑的那次 Chrome 的空迴圈量出負的（−2.7），那一次 Chrome 的數字不能用，下表 Chrome 的欄是前一次的。
- 倍數是 machopo ÷ Chrome，小於 1 表示 machopo 比較快。

## 每次呼叫的指令數

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `nodeType` | 66.9 | 102.5 | 1.53 |
| `parentNode` | 91.7 | 132.5 | 1.45 |
| `firstChild` | 88.5 | 132.5 | 1.50 |
| `id` | 212.5 | 248.5 | 1.17 |
| `tagName` | 205.5 | 237.5 | 1.16 |
| `contains` | 338.5 | 334.5 | 0.99 |
| `hasAttribute` | 512.4 | 471.4 | 0.92 |
| `getAttribute` | 617.7 | 540.5 | 0.88 |
| `setAttribute` | 1757.5 | 1034.5 | 0.59 |

## 每次呼叫的時間（ns）

### 熱的 getter 和 setter

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `nodeType` | 3.4 | 4.6 | 1.35 |
| `parentNode` | 4.6 | 6.0 | 1.30 |
| `n.tagName` | 9.1 | 10.2 | 1.12 |
| `n.children` | 18.9 | 10.8 | 0.57 |
| `getAttribute`（平的字串） | 20.1 | 15.6 | 0.78 |
| `getAttribute`（cons 字串） | 116.0 | 53.6 | 0.46 |
| `classList.contains` | 28.0 | 23.8 | 0.85 |
| `n.matches` | 28.7 | 39.7 | 1.38 |
| `opt.value` | 14.7 | 18.3 | 1.24 |
| `opt.text` | 110.7 | 47.5 | 0.43 |
| `link.href` | 151.3 | 105.4 | 0.70 |
| `document.readyState` | 18.6 | 17.1 | 0.92 |
| `document.compatMode` | 60.0 | 17.1 | 0.29 |
| `ctx.lineWidth = 1` | 5.6 | 7.5 | 1.34 |
| `ctx.lineWidth = 1.5` | 12.2 | 15.4 | 1.26 |
| `ctx.lineCap` | 55.0 | 19.3 | 0.35 |
| `ctx.getLineDash` | 62.7 | 24.7 | 0.39 |

### 字串和序列化

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `setAttribute`（平的） | 56.6 | 35.5 | 0.63 |
| `setAttribute`（cons） | 170.9 | 68.4 | 0.40 |
| `setAttribute`（CJK） | 56.5 | 57.7 | 1.02 |
| `setAttribute`（非 ASCII） | 56.5 | 46.2 | 0.82 |
| `getAttributeNames` | 91.3 | 80.0 | 0.88 |
| `c.textContent` | 22.5 | 36.0 | 1.60 |
| `c.innerHTML` | 41.2 | 92.5 | 2.25 |
| `style.color = 'red'` | 46.8 | 73.4 | 1.57 |
| `st.cssText` | 146.5 | 466.4 | 3.18 |

### global 上的 getter

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `document` | 0.2 | 0.2 | 1.00 |
| `navigator` | 64.7 | 58.2 | 0.90 |
| `location` | 87.1 | 66.2 | 0.76 |
| `performance` | 62.6 | 66.4 | 1.06 |
| `innerWidth` | 69.8 | 60.3 | 0.86 |
| `performance.now()` | 106.1 | 120.7 | 1.14 |
| `navigator.languages` | 157.3 | 127.3 | 0.81 |

### 會跑 script 的成員

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `dispatchEvent`（沒有 listener） | 79.4 | 76.1 | 0.96 |
| `dispatchEvent`（一個 listener） | 324.5 | 162.6 | 0.50 |
| `dispatchEvent`（沿樹 bubble） | 545.0 | 297.5 | 0.55 |
| `c.click()` | 677.0 | 326.5 | 0.48 |
| `form.reset()` | 550.0 | 363.0 | 0.66 |
| `inp.select()` | 163.5 | 208.5 | 1.28 |
| `new AbortController().abort()` | 1750.0 | 2121.0 | 1.21 |
| `inp.focus()` + `blur()` | 48834.0 | 58.0 | 0.00 |

### 走訪

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `tw.parentNode()` | 9.9 | 9.2 | 0.93 |
| TreeWalker 設 `currentNode` | 12.0 | 14.6 | 1.22 |
| TreeWalker 設 + `nextNode` | 26.8 | 26.1 | 0.97 |
| TreeWalker 設 + `firstChild` | 30.8 | 25.9 | 0.84 |
| NodeIterator next + previous | 33.8 | 25.0 | 0.74 |
| TreeWalker 設 + `nextNode`（有 filter） | 178.5 | 117.0 | 0.66 |
| 走訪沒有 wrapper 的節點 | 37.0 | 56.0 | 1.51 |
| 再走訪一次（已有 wrapper） | 9.0 | 9.5 | 1.06 |

### 建物件、dictionary、例外和其他冷的路

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `new Event` | 97.8 | 125.5 | 1.28 |
| `new Event`（帶 init） | 245.7 | 181.0 | 0.74 |
| `new CustomEvent`（帶 init） | 376.0 | 220.3 | 0.59 |
| `new MouseEvent`（帶 init） | 805.5 | 951.5 | 1.18 |
| `new DOMRect` | 274.3 | 62.7 | 0.23 |
| add + remove listener（帶 options） | 564.7 | 212.0 | 0.38 |
| `getBoundingClientRect` | 914.7 | 72.3 | 0.08 |
| `querySelectorAll` | 315.7 | 153.3 | 0.49 |
| `scrollTo(options)` | 1623.0 | 250.0 | 0.15 |
| 丟例外：`appendChild(self)` | 2696.0 | 1938.3 | 0.72 |
| 丟例外：參數型別錯 | 2605.7 | 990.3 | 0.38 |
| 丟例外：selector 語法錯 | 2479.3 | 2042.3 | 0.82 |
| `new Response().text()` | 5663.0 | 418.0 | 0.07 |
| `new Blob().text()` | 430752.0 | 777.0 | 0.00 |

### 對照組

| 項目 | Chrome | machopo | 倍數 |
|---|---|---|---|
| 空迴圈 | 0.2 | 0.3 | 1.50 |
| 字串串接（`setAttribute` cons 的對照） | 4.8 | 4.2 | 0.88 |
| 字串串接（`getAttribute` cons 的對照） | 4.7 | 4.2 | 0.89 |

## 逐條指令：`nodeType` 和 `getAttribute` 的 callback

量法：`prototypes/step-callback.py`。gdb 在 callback 的入口等迴圈跑過二十萬次（JIT 已經最佳化），再一條一條單步到它返回，記下每一條指令和它在哪個函式；所以條數是精確的，不是取樣（取樣在這麼短的函式之間會偏：`perf record` 把 `nodeType` 的 90% 算在 V8 的 `CallApiCallbackOptimizedNoProfiling` 上）。Chromium 的執行檔沒有符號，Blink 那一側沒辦法照樣單步：下面 Chrome 的 callback 的條數是「整次呼叫的條數減掉 V8 呼叫 callback 的那一段」，並假設那一段兩邊一樣長（兩邊的 V8 版本不同，這一點沒有驗證）。

### `nodeType`

整次呼叫 102.5 條，callback 自己 52 條，所以 V8 從 JIT 過的程式呼叫 callback 的那一段約 50 條。Chrome 整次 66.9 條，照同樣的 50 條算，Blink 的 callback 約 17 條（4k-G 那次量到整次 72.9，算出來約 23）。

| 做什麼 | machopo 幾條 | Blink 有沒有 | 要什麼才拿得掉 |
| --- | ---: | --- | --- |
| 進出函式：存和還原 5 個 callee-saved 暫存器、把 info 放進其中一個、`ret` | 12 | 幾乎沒有（熱的路上沒有要跨呼叫留著的值） | 冷的路（標記中的 barrier、panic、建 scope）搬成收 info、自己重新讀的函式，熱的路就不用 callee-saved 的暫存器。估計省 8 到 10 條，沒做過 |
| 讀 isolate、讀 isolate 的狀態 | 2 | 讀 isolate 1 條 | 狀態是 machopo 的設計（DOM 的 heap 在 isolate 的狀態裡） |
| 讀型別的 tag（一個全域的值）、檢查它初始化了沒有 | 4 | 沒有（tag 是編譯期的常數） | elise 的型別編號變成編譯期的常數 |
| 解出 `this`：讀 holder、handle、`CppHeapPointerTable` 的那一格，檢查不是空的、比 tag | 13 | 約 7（`ToWrappableUnsafe`：讀同樣的三處，不比 tag） | 不比 tag 就是信任 V8 的 signature 檢查；要補 soundness 的論證（使用者問過，現在不拿掉） |
| 檢查位址在這個 heap 的 cage 裡 | 5 | 沒有 | 決定留著（4k-G） |
| 檢查是不是正在標記（讀取的 barrier 的旗標） | 2 | 沒有（Oilpan 讀取不用 barrier） | 要改 GC 的設計 |
| 算 `nodeType`（照節點的種類查跳躍表） | 9 | 一個 virtual call，差不多 | 查表試過，多 2 條（4k-G） |
| 寫回傳值 | 5 | 一樣（V8 的 `SetInternal` 編出來是同樣的指令） | 拿不掉 |
| 合計 | 52 | 約 17 到 23 | |

差的約 30 到 35 條裡，進出函式和 tag 的那兩項（合計約 18 到 20 條）是寫法和 elise 的 API 可以拿掉的；tag 的比對、cage 的檢查、標記的旗標（合計 13 條）是 machopo 和 Blink 設計不同的地方。

### `getAttribute('id')`

（修字串參數之前量的；修好之後整次是 540.5 條，少的 20 條在「看參數能不能原地借」那一列。）

整次呼叫 560.5 條，callback 和它呼叫的函式 508 條（V8 那一段約 52 條）。Chrome 整次 617.7 條，Blink 那一側約 565 條：machopo 已經比較少。

| 做什麼 | 在哪個函式 | 幾條 |
| --- | --- | ---: |
| 進函式，看參數是不是能原地借的字串 | callback 34、`LentString::lends` 43 | 77 |
| 解出 `this`、開操作、借出字元 | callback 72、`LentString::ascii` 33 | 105 |
| 名稱要不要轉小寫 | callback 8、`element::qualified` 35 | 43 |
| 找屬性：名稱在固定名稱表裡的編號，再在元素的屬性裡找 | callback 7、`Cx::attribute_by_name` 85、`names::find_fixed` 74 | 166 |
| 值的 V8 字串從快取寫進回傳值 | callback 60、rusty_v8 的 `HandleHost::assert_match_isolate` 9 | 69 |
| 收尾、還原暫存器、返回 | callback 48 | 48 |
| 合計 | | 508 |

- Blink 那一側做的事（讀 Chromium 的原始碼知道的，條數沒辦法量）：參數轉成 `AtomicString`（每次查 atom 的表）、`Element::getAttribute`、回傳值從 `StringCache` 拿。
- machopo 還能少的：`find_fixed` 的 74 條是每次呼叫用名稱的文字查固定名稱表（決定 20：要省掉得把編號記在 V8 的字串上，那要 V8 的 sandbox）；`assert_match_isolate` 的 9 條和一次呼叫是 rusty_v8 開 `Weak` 時比 isolate 的檢查，可以在 fork 加一個不比的入口（沒做）。

## blink_perf 的頁（2026-11-13）

`python3 scripts/bench.py blink-perf --pages …`，machopo 和 Chrome for Testing 152 同一次各跑一次（同一頁兩次之間約有一成的起伏）。machopo 慢幾倍；「開始前」是 2026-11-10 量的（`docs/performance-gaps.md` 當時的快照），「修之前」是 4m-3 做完時，「現在」是修好字串參數的 store forwarding 之後。

| 頁 | 開始前 | 修之前 | 現在 |
| --- | ---: | ---: | ---: |
| `bindings/node-type` | 1.83 | 1.32 | 1.33 |
| `bindings/first-child` | 1.77 | 1.45 | 1.44 |
| `bindings/id-getter` | 1.69 | 1.31 | 1.28 |
| `bindings/id-setter` | 1.59 | 1.49 | 1.30 |
| `bindings/get-attribute` | 1.49 | 1.27 | 0.96 |
| `bindings/set-attribute` | 1.40 | 1.26 | 1.02 |
| `bindings/get-element-by-id` | 2.20 | 2.06 | 1.41 |
| `bindings/get-elements-by-tag-name` | 1.93 | 1.69 | 1.26 |
| `bindings/create-element` | 1.43 | 1.61 | 1.50 |
| `bindings/gc-forest` | 1.58 | 1.68 | 1.58 |
| `bindings/gc-mini-tree` | 1.19 | 1.22 | 1.10 |
| `bindings/gc-tree` | 1.01 | 1.13 | 1.07 |
| `bindings/append-child` | 0.27 | 0.27 | 0.27 |
| `bindings/insert-before` | 0.33 | 0.30 | 0.31 |
| `events/EventsDispatching` | 1.24 | 1.20 | 1.22 |
| `events/EventsDispatchingInV1ShadowTrees` | 0.77 | 0.69 | 0.70 |

`append-child`、`insert-before` 工具標成可疑（machopo 快兩倍以上，兩邊做的事可能不同）。

## 建節點的頁的退步：store forwarding（2026-11-13，收尾時找到、修掉）

4m-3 做完重量時 `create-element` 和 `gc-forest` 比開始前差（上表的「修之前」）。查的經過：

- **對 `main` 直接比**（`bench.py blink-perf --against`，同一台機器各三次）：`create-element` 2510 → 2215 runs/s（慢 11.8%），`gc-forest` 91.5 → 100.3 ms（慢 9.6%），同一次 `node-type` 快 37.5%。是這份 RFC 造成的。`gc-tree` 重跑是 137 對 Chrome 的 133 ms，那次的 1.13 是雜訊。
- **不是 GC**（`blink-perf --gc`）：`create-element` 的 GC 是 1099 ms，開始前 1177；`gc-*` 頁的名字是建一批節點再丟掉，GC 只佔區間的一成，慢的是建節點的那九成。
- **照 commit 二分**（舊的 commit 加上現在的 fork pin 編出來比）：`create-element` 在 `main` 2510、第 2 段 2343、第 3 段 2324、4a 2319、4b 2321、4c 2271、4h 2263、4k-D 2281、4l 2264、4m-3 之後約 2200；`gc-forest` 91.5、95.5、96.4、98.1、94.1、4c 100.7，之後持平。不是單一一段。
- **不是多做了事**：和整頁同樣的迴圈（window 的 realm、結果丟掉，五百萬次 `document.createElement('div')`）每次呼叫 `main` 2063 條指令、413 cycle，4m-3 之後 2027 條、459 cycle；分支和快取的 miss 兩邊一樣，多的是 store forwarding 的衝突（AMD 的 `ls_bad_status2.stli_other`）：5.9 → 8.1 次。
- **哪幾處**：`perf record` 的取樣會偏幾條指令、這台機器開不了精確取樣，定不準。改用 gdb 單步一次呼叫，記下每個寫入和讀取的位址和寬度，找「讀的範圍和前面的寫入重疊、但不在那個寫入裡面」的讀取。`main` 有 4 處（都在 V8 和 `dom` 裡），4m-3 之後多了 3 處，都在 callback 裡：
  1. 字串參數的 `LentString`（V8 字串的 handle 加兩個 `bool`）：`lends` 透過參照把「已知 ASCII」那個 byte 寫進去，callback 接著用 16 bytes 把整個結構搬走。
  2. 同一個結構搬到下一個位置時，前 8 bytes 是剛寫的 16 bytes 的後半、後 8 bytes 是另一次寫入。
  3. `foresee_element` 回傳 `(Option<NameId>, usize)`：`Option<u32>` 分成兩個 4 bytes 寫到呼叫者的 stack，呼叫者用 8 bytes 讀回。
- **修法**：`bindings::LentString::lending`（先檢查、通過了才建，不經過參照改）；兩個旗標併成一個字；`foresee_element` 給只拿 info 的 front 的版本回傳兩個字（在暫存器裡回來）。第 1、2 處每個字串參數都會走到，所以帶字串的成員都受惠。
- **結果**（同一個迴圈）：413（`main`）、459（修之前）、415 cycle（修之後）；衝突 5.9、8.1、3.2 次。對 `main` 的整頁：`create-element` −0.2%、`gc-forest` 快 3.8%、`gc-mini-tree` 快 2.9%、`get-attribute` +52.7%、`set-attribute` +33.1%、`get-element-by-id` +56.2%、`node-type` +37.8%（七頁的幾何平均快 19.8%）。
- **教訓**：這份 RFC 從頭到尾用指令數驗收每一段，而這種成本不在指令數裡（指令數還變少）；`bench.py calls` 其實一直有印 store forwarding 的衝突數（`createElement` 4.66 → 8.16），沒有人看。之後 binding 的改動要連同 cycle 和衝突數一起看，建節點的頁要在每個階段結束時對 `main` 量一次。

## 怎麼讀

- 時間的 63 項（另有 3 項對照）裡：machopo 比較快或一樣的 42 項，1.0 到 1.3 倍的 12 項，超過 1.3 倍的 9 項。
- 最熱的 getter 仍落後：`nodeType`、`parentNode`、`firstChild` 的指令數是 Chrome 的 1.45 到 1.53 倍，時間上 `nodeType` 1.35 倍、`parentNode` 1.30 倍。
- 帶字串參數的成員追上了：`getAttribute` 15.6 對 20.1 ns（0.78），`setAttribute` 35.5 對 56.6（0.63），`classList.contains` 0.85；修 store forwarding 之前是 1.26、0.78、1.25。
- 超過 1.3 倍的 9 項：三個最簡單的 getter 和 setter（`nodeType`、`parentNode`、`lineWidth = 1`，1.30 到 1.35），DOM 那一側的序列化、解析和 selector（`cssText` 3.18、`innerHTML` 2.25、`textContent` 1.60、`style.color =` 1.57、`matches` 1.38），沒有 wrapper 的走訪（1.51，建 wrapper）。
- 幾個極端的倍數不要當成 binding 的差距：`inp.focus()` + `blur()`、`new Blob().text()`、`getBoundingClientRect`、`new Response().text()` 在 Chrome 那邊很可能包含 machopo 沒做的工作（headless 下的 focus 處理、真的非同步讀取、layout）。這一點沒有拆開確認過。
- 腳本裡拿掉的一項：`sel.selectAllChildren(n)` 在整份腳本裡量到 62 µs、單獨量約 0.15 µs，原因還沒查明。
