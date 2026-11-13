# RFC 0097 的量測結果：對 Chrome 的收尾（2026-11-13）

這份是 `README.md` 的補充，放 4m-3 做完時對 Chrome 量的整份結果。各段的前後對照和當時的說明在 `progress.md`；量法的由來在 `details.md`。

## 量法

- machopo：branch `0097` 的 commit `747d0ae`，`perf` profile 編的 `dom_js` 測試 binary（`cargo test -p dom_js --test it --profile perf --no-run`），`MACHOPO_BENCH_WINDOW` 在 window 的 realm 裡跑 `bench::bench_calls`。
- Chrome：本機裝的 Chromium（`chromium-browser --headless=new`）。
- 機器：開發機（Linux、AMD Ryzen AI 9 HX 370），`taskset` 釘一顆核心，各跑三次取最小。
- 時間：`prototypes/chrome-compare.py`（腳本是 `prototypes/chrome-compare.js`），每項是每次呼叫的 ns。
- 指令數：`prototypes/chrome-instructions.py`，`perf stat -e instructions:u` 量 2000 萬次和 6000 萬次的迴圈、相減再除，再減掉空迴圈。這個量法在幾條指令的尺度上會晃：Chrome 的 `nodeType` 這次是 66.9，4k-G 那次是 72.9。
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
| `hasAttribute` | 512.4 | 492.5 | 0.96 |
| `getAttribute` | 617.7 | 560.5 | 0.91 |
| `setAttribute` | 1757.5 | 1081.5 | 0.62 |

## 每次呼叫的時間（ns）

### 熱的 getter 和 setter

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `nodeType` | 3.6 | 4.5 | 1.25 |
| `parentNode` | 4.4 | 6.1 | 1.39 |
| `n.tagName` | 9.2 | 10.2 | 1.11 |
| `n.children` | 18.2 | 10.8 | 0.59 |
| `getAttribute`（平的字串） | 20.1 | 25.3 | 1.26 |
| `getAttribute`（cons 字串） | 116.5 | 56.7 | 0.49 |
| `classList.contains` | 26.8 | 33.5 | 1.25 |
| `n.matches` | 27.8 | 48.1 | 1.73 |
| `opt.value` | 14.5 | 18.3 | 1.26 |
| `opt.text` | 110.7 | 47.2 | 0.43 |
| `link.href` | 151.5 | 105.5 | 0.70 |
| `document.readyState` | 18.6 | 17.1 | 0.92 |
| `document.compatMode` | 60.0 | 17.1 | 0.29 |
| `ctx.lineWidth = 1` | 5.7 | 7.5 | 1.32 |
| `ctx.lineWidth = 1.5` | 12.1 | 15.2 | 1.26 |
| `ctx.lineCap` | 55.1 | 19.2 | 0.35 |
| `ctx.getLineDash` | 58.3 | 24.7 | 0.42 |

### 字串和序列化

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `setAttribute`（平的） | 57.0 | 44.3 | 0.78 |
| `setAttribute`（cons） | 172.1 | 73.5 | 0.43 |
| `setAttribute`（CJK） | 56.7 | 64.3 | 1.13 |
| `setAttribute`（非 ASCII） | 56.6 | 51.7 | 0.91 |
| `getAttributeNames` | 93.0 | 80.7 | 0.87 |
| `c.textContent` | 21.4 | 35.9 | 1.68 |
| `c.innerHTML` | 41.6 | 90.0 | 2.16 |
| `style.color = 'red'` | 47.1 | 81.8 | 1.74 |
| `st.cssText` | 148.8 | 473.1 | 3.18 |

### global 上的 getter

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `document` | 0.2 | 0.2 | 1.00 |
| `navigator` | 65.0 | 58.1 | 0.89 |
| `location` | 87.0 | 65.9 | 0.76 |
| `performance` | 62.7 | 66.4 | 1.06 |
| `innerWidth` | 70.3 | 60.4 | 0.86 |
| `performance.now()` | 107.3 | 122.5 | 1.14 |
| `navigator.languages` | 159.0 | 128.7 | 0.81 |

### 會跑 script 的成員

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `dispatchEvent`（沒有 listener） | 78.0 | 76.5 | 0.98 |
| `dispatchEvent`（一個 listener） | 325.6 | 161.5 | 0.50 |
| `dispatchEvent`（沿樹 bubble） | 565.0 | 290.0 | 0.51 |
| `c.click()` | 699.5 | 316.0 | 0.45 |
| `form.reset()` | 522.5 | 346.0 | 0.66 |
| `inp.select()` | 162.0 | 206.5 | 1.27 |
| `new AbortController().abort()` | 1764.5 | 2162.5 | 1.23 |
| `inp.focus()` + `blur()` | 49234.5 | 59.5 | 0.00 |

### 走訪

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `tw.parentNode()` | 9.8 | 9.2 | 0.94 |
| TreeWalker 設 `currentNode` | 11.7 | 14.6 | 1.25 |
| TreeWalker 設 + `nextNode` | 26.8 | 26.2 | 0.98 |
| TreeWalker 設 + `firstChild` | 31.4 | 25.9 | 0.82 |
| NodeIterator next + previous | 34.2 | 25.4 | 0.74 |
| TreeWalker 設 + `nextNode`（有 filter） | 191.5 | 117.5 | 0.61 |
| 走訪沒有 wrapper 的節點 | 36.0 | 55.0 | 1.53 |
| 再走訪一次（已有 wrapper） | 9.5 | 9.5 | 1.00 |

### 建物件、dictionary、例外和其他冷的路

| 呼叫 | Chrome | machopo | 倍數 |
|---|---|---|---|
| `new Event` | 141.0 | 134.6 | 0.95 |
| `new Event`（帶 init） | 250.7 | 184.7 | 0.74 |
| `new CustomEvent`（帶 init） | 372.0 | 229.7 | 0.62 |
| `new MouseEvent`（帶 init） | 801.0 | 924.5 | 1.15 |
| `new DOMRect` | 246.7 | 63.0 | 0.26 |
| add + remove listener（帶 options） | 375.0 | 224.0 | 0.60 |
| `getBoundingClientRect` | 920.7 | 72.3 | 0.08 |
| `querySelectorAll` | 284.3 | 167.3 | 0.59 |
| `scrollTo(options)` | 1632.3 | 260.7 | 0.16 |
| 丟例外：`appendChild(self)` | 2663.0 | 1954.7 | 0.73 |
| 丟例外：參數型別錯 | 2594.7 | 1015.0 | 0.39 |
| 丟例外：selector 語法錯 | 2484.7 | 2051.0 | 0.83 |
| `new Response().text()` | 5229.0 | 429.0 | 0.08 |
| `new Blob().text()` | 422152.0 | 772.0 | 0.00 |

### 對照組

| 項目 | Chrome | machopo | 倍數 |
|---|---|---|---|
| 空迴圈 | 0.3 | 0.3 | 1.00 |
| 字串串接（`setAttribute` cons 的對照） | 4.7 | 4.2 | 0.89 |
| 字串串接（`getAttribute` cons 的對照） | 4.7 | 4.2 | 0.89 |

## 怎麼讀

- 時間的 66 項裡：machopo 比較快或一樣的 42 項，1.0 到 1.3 倍的 16 項，超過 1.3 倍的 8 項。
- 最熱的 getter 仍落後：`nodeType`、`parentNode`、`firstChild` 的指令數是 Chrome 的 1.45 到 1.53 倍，時間上 `nodeType` 1.25 倍、`parentNode` 1.39 倍。
- 超過 1.3 倍的多半不是 binding 的外殼：`cssText`、`innerHTML`、`style.color =`、`matches`、`textContent` 慢在 DOM 那一側的序列化、解析和 selector；沒有 wrapper 的走訪慢在建 wrapper。
- 幾個極端的倍數不要當成 binding 的差距：`inp.focus()` + `blur()`、`new Blob().text()`、`getBoundingClientRect`、`new Response().text()` 在 Chrome 那邊很可能包含 machopo 沒做的工作（headless 下的 focus 處理、真的非同步讀取、layout）。這一點沒有拆開確認過。
- 4m-2 和 4m-3 本身沒有改變這些數字（冷的路前後差在 ±10 條指令內，見 `progress.md`）：這裡是整份 RFC 累積到這個 commit 的結果。
- 腳本裡拿掉的一項：`sel.selectAllChildren(n)` 在整份腳本裡量到 62 µs、單獨量約 0.15 µs，原因還沒查明。
