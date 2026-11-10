# RFC 0093：wasm 文件接回 renderer：用 `dom_wasm` 做 TodoMVC

- 開始日期：2026-11-10
- 狀態：🚧 已排程：2026-11-10（第 1 段的決定確認了；第 2 段以後的在各段開始前定；進度在 `progress.md`）
- 細節：`details.md`

## 摘要

讓網頁再一次能只用 wasm 寫 script，這次接在換過的 DOM 和 `dom_wasm` 上，驗收是一份用 wasm guest 寫的 TodoMVC：在視窗裡能新增、勾選、編輯、刪除、篩選 todo，行為和同一份的 JS 版一樣。

RFC 0089 換 DOM 時把舊的 wasm 實作整個刪掉（它的決定 36），`dom_wasm` 只留下約 70 個只碰節點的成員和它自己的測試，renderer 沒有 wasm 的文件。這份把缺的四塊補起來，分四段：最小的 wasm 文件（頁面的 document 交給 guest）、節點以外的物件的 handle 和 TodoMVC 用到的成員、事件和 callback、TodoMVC 本身。guest 用 C 寫，不做 SDK。

第 1 段要決定的事 2026-11-10 都確認了（「已決定的事項」）；第 2 段以後的做法還是當天的看法，每一段開始前只決定那一段要決定的事（「待決定」）。

## 動機

現況（2026-11-10）：

- **renderer 沒有 wasm 的文件**。`--script wasm` 和 `<meta name="script" content="wasm">` 在 RFC 0089 的 5b-1 刪掉了，每份文件都跑 JS。`dom_wasm` 只有它自己的測試在用（`crates/dom_wasm/README.md`）。
- **guest 拿不到頁面的 document**。`machopo:dom` 只有 `create-document`（一份 detached 的文件）；`WasmDom` 自己建一份沒有 engine 驅動的 DOM，不是 renderer 的那一份。renderer 只透過 `dom_api::ScriptEngine` 碰 script，實作它的只有 `dom_js` 的 `V8Engine`。
- **guest 收不到事件**。WIT 裡沒有 event，也沒有 guest 給 host 呼叫的 export。Store 的 handle 表每一格是 `Member<Node>`：`Event`、`Window`、`Location`、`DOMTokenList` 這些不是節點的物件 guest 拿不了。
- **成員凍結在 2026-10-30**（`wit::FROZEN`）：之後表上加的 plain 成員只給 JS。TodoMVC 用到的 `value`、`checked`、`classList`、`style`、`focus()`、`location.hash` 都不在 WIT 裡（清單在 `details.md` 的「TodoMVC 用到的成員」）。
- **guest 這一側只剩 C 的測試 guest**。RFC 0001 的 Rust SDK（`kun_guest`）和 Lua 的 runtime 都刪了；節點在 WIT 裡只是 `u32`，有型別的 API、`release` 的計數、callback 的表都要各語言的 binding 自己包。

為什麼用 TodoMVC 當驗收：它是 Speedometer 的基本單位，同一份的 JS 版 machopo 已經跑得起來，DOM 那一側要的成員都做好了；wasm 版只差 binding 這一層，做完可以直接和 JS 版比行為，之後也能比每次呼叫的成本。它用到的東西（事件委派、表單控制項的值、class、焦點、hash 的路由）也是任何互動的頁面的最小集合。

目標：

1. 宣告成 wasm 的文件在 renderer 裡載入、執行它的 component，guest 操作的是頁面的 document，改動照樣經過 style、layout、paint 畫出來。
2. guest 能註冊 listener、收到使用者的輸入和 DOM 自己分派的事件。
3. guest 拿得到節點以外的物件，TodoMVC 用到的成員都在 WIT 裡。
4. 一份用 C 寫的 TodoMVC 的 guest，放在 repo 裡、由 `ci.py guests` 編，有情境守著。

不在範圍內：

- **JS 和 wasm 在同一份文件裡混用**：一份文件只用一種 script，照 RFC 0001。
- **追上 JS 那一側全部的成員**：只做 TodoMVC 用到的，加上同一種形狀、不用多做事就能帶進來的（例如放開凍結後所有的 plain 成員）。`fetch`、observer、custom element、shadow DOM、canvas、`postMessage` 都不做。
- **`localStorage`**：machopo 的 JS 那一側也沒有 Web Storage。TodoMVC 的 todo 只放在記憶體裡（Speedometer 的版本也是）。
- **wasm 文件的子 frame、worker、DevTools、WebDriver 執行 script**。
- **guest 的 SDK**（Rust 或其他語言的 binding）：guest 維持 C，直接用 `wit-bindgen` 產生的 C binding（「已決定的事項」第 1 條）。
- **Lua 的 runtime**：需要時另開。
- **效能的量測**：Speedometer 平常不量；wasm 版 TodoMVC 的量測要做時另外排。

要解決的 `docs/limitations.md` 裡的限制：

- 「JavaScript 和 DOM API」的「沒有 wasm 的文件：wasm 的 script 不跑……」。
- 同一份裡還有幾條描述 wasm guest 的行為（guest 的 `customElements`、observer 的 callback 拿到的 handle、wasm 文件裡的 iframe、`requestIdleCallback`、`--script wasm` 的 WebDriver），是舊的實作的：第 1 段開始時逐條對過，不成立的拿掉、之後才成立的改寫成現在的樣子。

## 設計概要

網頁的作者（2026-11-10 的看法，宣告的寫法照 RFC 0001）：

```html
<!doctype html>
<meta name="script" content="wasm">
<link rel="stylesheet" href="todomvc.css">
<section class="todoapp">…</section>
<script type="application/wasm" src="todomvc.wasm"></script>
```

guest 是一個 component，import `machopo:dom`、export 入口和 callback 的入口，用 C 寫（wasi-sdk 的 `wasm32-wasip3` 和 `wit-bindgen` 的 C binding，和 `dom_wasm` 的測試 guest 一樣）。寫起來像這樣（export 和 `add-event-listener` 的名稱只是示意）：

```c
#include "todomvc.h"   // wit-bindgen 從 world 產生的

typedef machopo_dom_dom_node_t node_t;
enum { ON_NEW_TODO_KEYDOWN = 1 };   // callback 的 id 是 guest 自己編的

// `start` is an `async func`: the callback ABI. One that never waits
// returns at once and exits.
todomvc_callback_code_t exports_todomvc_start(node_t document) {
    machopo_dom_dom_dom_exception_t err;
    machopo_dom_dom_option_element_t input;
    todomvc_string_t selector, keydown;
    todomvc_string_set(&selector, ".new-todo");
    todomvc_string_set(&keydown, "keydown");
    machopo_dom_dom_document_query_selector(document, &selector, &input, &err);
    machopo_dom_dom_event_target_add_event_listener(input.val, &keydown, ON_NEW_TODO_KEYDOWN, &err);
    exports_todomvc_start_return();
    return TODOMVC_CALLBACK_CODE_EXIT;
}

bool exports_todomvc_handle_event(uint32_t id, node_t event) {
    switch (id) {
    case ON_NEW_TODO_KEYDOWN: /* … */ break;
    }
    return true;
}
```

會看到的行為：

- wasm 文件裡的 JS（`<script>`、`onclick=""`）不執行，JS 文件裡的 `application/wasm` 的 script 也不執行。
- guest 改了 DOM，下一個 frame 就畫出來，和 JS 改的一樣。
- 使用者按鍵、點擊時，guest 註冊的 listener 照 DOM 的分派順序被呼叫，`preventDefault()` 有作用。
- guest trap 時像沒接住的例外一樣印出來，那個 component 之後不再被呼叫（Component Model 的規定），頁面留著。
- DOM 的函式回傳 `result<_, dom-exception>`：C 的 binding 回傳 `bool`，錯誤的 `code` 寫進最後一個參數。

## 詳細設計

下面每一塊先寫現在有什麼、要補什麼，選項和 2026-11-10 的看法；確認之後才搬進「已決定的事項」。

### 1. wasm 文件和 `ScriptEngine`

- **文件怎麼成為 wasm 文件**：parse 之前決定，因為 engine 要在建 document 之前選好：前 1024 byte 的 `<meta name="script" content="wasm">`（「已決定的事項」第 2 條）。換到另一種文件時換掉整頁的 engine。
- **一個實作 `dom_api::ScriptEngine` 的 wasm engine，在 `dom_wasm`**（「已決定的事項」第 4 條）：`ScriptEngine` 現在約 70 個方法。輸入、timer、rendering update 的邏輯已經在 `dom_api` 裡、對 `ScriptHost` 是泛型的；`V8Engine`（`crates/dom_js/src/engine.rs`）的方法大多只是進 V8 的 scope、呼叫 `dom_api`、跑 microtask checkpoint。wasm 這邊寫自己的 `ScriptHost` 和那一層薄的；兩邊完全一樣的片段搬成 `dom_api` 的函式。第 1 段實作建 window 和 document、執行 script、`document_ready`、rendering update、focus 和可見性；輸入到第 3 段；DevTools、module、observer、WebDriver 的 script 留空。`ScriptEngineFactory` 多一個建 wasm engine 的函式，shell 兩個都裝，renderer 照 `<meta>` 選。
- **頁面的 DOM 是誰的**：wasm 的 engine 的，跑 guest 的一輪時借進 `dom_wasm` 的 thread-local（「已決定的事項」第 10 條）；`dom_wasm` 自己的測試照舊可以讓 DOM 一直留在 thread-local。這份 DOM 就是頁面的那一份（wasm 的 GC 物件和節點在同一個 heap，`StoreSet` 和借 heap 的規則照舊），不建 isolate、沒有 V8 驅動，elise 自己決定什麼時候 GC，有 nursery（「已決定的事項」第 3 條）。
- **guest 怎麼拿到 document**：入口 `start(document)` 的參數，`start` 是 `async func`（「已決定的事項」第 8 條）。`create-document` 留著（建 detached 的文件）。
- **載入**：`<script type="application/wasm" src>` 照外部 script 下載、MIME 要是 `application/wasm`、在 compile thread 上編、編好回到 main thread 照文件順序執行（RFC 0001 的「載入和執行」）。每個 script 一個 Store。
- **Store 是 async 的，guest 用 WASI 0.3**（「已決定的事項」第 5 條）：`dom_wasm` 的 host 函式是同步的 `func_wrap`，async 的 Store 裡照樣能用；現在測試的 Store 是同步的、WASI 0.2，和頁面的一起換成 async 的、WASI 0.3（「已決定的事項」第 6 條）。入口是 `async func`（第 8 條），WASI 的時鐘自己實作（第 7 條），event loop 需要時才跑（第 9 條）。

### 2. 節點以外的物件的 handle

現在的 handle 表每一格是 `Member<Node>`，種類（節點的 interface）記在 heap 外，呼叫前不碰 DOM 就能檢查 `this`。`dom` 那一側，任何物件是 `ObjectRef`（一個 enum，節點是其中一種）。

做法在「已決定的事項」第 12 條：一張表，每格指任何一種物件、記它的 interface；元素自己的 interface 的成員（`value`、`checked`）也靠它（`details.md` 的「TodoMVC 用到的成員」）。

WIT 的成員是明列的清單（「已決定的事項」第 13 條）：第 2 段加 TodoMVC 用到的；不定個數的字串是 `list<string>`，可省略的 dictionary 先不放。WIT 的版本升到 `0.2.0`。

### 3. 事件和 callback

- **callback 的表示**：guest 自己的 id（`u32`），`add-event-listener(this, type, callback-id, options)`；DOM 存的 listener 記著 Store 和 id。分派到它時，host 呼叫 guest 的 export `handle-event(id, event)`。`dom_api` 的分派已經經過 `ScriptHost::call_listener`，wasm 的 engine 實作它。
- **重入**：guest 在自己的呼叫裡（例如 `focus()`、`click()`、`dispatch-event`）觸發的事件，listener 要在那個呼叫回來之前跑。這些成員宣告成 `async func`，host 用 `call_concurrent` 呼叫 listener（「已決定的事項」第 5 條）。`dom_api` 的分派是同步的、在迴圈裡呼叫 `ScriptHost::call_listener`，而呼叫 guest 要 `await`、DOM 的操作不能跨過它：兩邊怎麼接是第 3 段最大的題目。
- **使用者的輸入**：renderer 的 `handle_mouse`、`handle_key` 經過 engine 分派，這時沒有 guest 在執行，直接呼叫 export。
- **生命週期**：listener 拿著 Store 的 holder 的 edge，Store 拿著節點的 handle，環在 DOM 的 heap 裡，整個收得掉（RFC 0089 的決定 34 第 2 條留下的形狀）。guest 記憶體裡的 closure 不在圖上：listener 被移除或它的節點被收掉時，host 經過另一個 export 告訴 guest 那個 id 不用了（舊的 `dom_wasm` 沒有，callback 留到 Store 死掉）。
- **Store 什麼時候死**：`start` 回來之後，只要還有 listener、timer 指著它就活著；頁面換掉時連同 DOM 一起丟掉。
- **timer 和 `requestAnimationFrame`**：TodoMVC 用不到，形狀和 listener 一樣（id 加另一個 export）。做不做在第 3 段開始前定。
- **trap**：照 RFC 0001：印出來，那個 Store 不再進入，它的 listener 之後都跳過。

### 4. TodoMVC 的 guest

- **語言是 C，不做 SDK**（「已決定的事項」第 1 條）：guest 直接呼叫 `wit-bindgen` 從 `machopo-dom.wit` 產生的 C 函式，工具鏈照 `ci.py guests` 現在的（wasi-sdk、`wit-bindgen`）。handle 的 `release`、callback 的 id 對到哪個函式、字串的借用和釋放都由 guest 自己寫；TodoMVC 裡重複的部分（借一個字串、檢查錯誤）寫成它自己檔案裡的幾個 helper，不另外做成函式庫。
- **放哪**（「已決定的事項」第 11 條）：guest 的原始碼和編好的 `.wasm` 在 `crates/dom_wasm/tests/guests/`，`ci.py guests` 重編；JS 版（Speedometer 的 vanilla ES5）在 `tests/sites/todomvc/`；wasm 版的頁面在 `tests/scenarios/pages/`。
- **TodoMVC**：HTML 和 CSS 用和 JS 版同一份，guest 只換掉 script。行為照 TodoMVC 的規格，除了不保存（沒有 `localStorage`）。
- **不同平台編出的 `.wasm` 不逐 byte 相同**（`docs/limitations.md` 的「平台和建置」已經有這條），TodoMVC 的 guest 也是。

### 測試

- `crates/dom_wasm/tests/it`（harness 改成 async 的 Store，guest 重編成 WASI 0.3，「已決定的事項」第 6 條）：每一段新的 host 行為（非節點的 handle、listener 的呼叫順序、重入、callback 的釋放、trap）用 C 的 guest 的 case；WIT 的 golden。
- renderer：wasm 文件載入、執行順序、JS 不執行，照現有測 script 載入的地方加。
- 情境（`tests/scenarios/`）：TodoMVC 的新增、勾選、編輯（雙擊、Enter、Escape、blur）、刪除、清除已完成、三種篩選；同一份情境也對 JS 版跑，兩邊的結果要一樣。
- 靜態畫出來的 TodoMVC（第 1 段）：一份情境（`wait-text` 確認內容、`screenshot` 留圖），加一個 renderer 的測試比 wasm 版和 JS 版的同一頁畫出來一樣。不用 `test.py reftest`：wasm 文件不執行 WebDriver 的 script（「已決定的事項」第 3 條）。
- 每一段跑 mutation test，結果寫進 `progress.md`。
- 改到 wasmtime 的 fork 或 elise 的 `raw` 時跑 fork 的測試。
- 視窗那一層不動，`test.py automation` 只在驗收時跑。

### 分階段

| 段 | 做什麼 | 做完看得到什麼 |
| --- | --- | --- |
| 1 | guest 和測試換成 WASI 0.3 和 async 的 Store；wasm 文件：選 engine、載入 component、`start(document)`、頁面的 DOM 借給 Store | 用現在的 70 個成員畫出靜態的 TodoMVC（情境和 renderer 的測試） |
| 2 | 節點以外的物件的 handle、放開凍結、補 TodoMVC 用到的成員 | guest 讀寫 `value`、`classList`、`style`、`location.hash` |
| 3 | 事件和 callback：`add-event-listener`、guest 的 export、重入、callback 的釋放、使用者的輸入 | 按鍵和點擊叫得到 guest 的 listener |
| 4 | C 的 TodoMVC 的 guest、情境 | 驗收 |

第 1 段的靜態 TodoMVC 就是第 4 段的 guest 的開頭（只有 `start`），每一段往上加。

## 缺點

- **又多一個要跟著 DOM 走的 front end**：每加一個成員、一種 callback，JS 和 wasm 兩邊都要接。RFC 0089 的決定 36 就是為了不拖住換 DOM 才停掉它。這份只把範圍放到 TodoMVC，之後每個新的 API 還是先只給 JS。
- **`ScriptEngine` 那一層薄的有兩份**：`V8Engine` 和 wasm 的 engine 各寫一次，`ScriptEngine` 加方法時兩邊都要補（wasm 的可以留空）。
- **async 的 Store 要多做很多**：Store 的 event loop、WASI 0.3、guest 的工具鏈，和 `dom_api` 裡會呼叫 script 的路怎麼接上 `await`（可能動到 JS 那條路）。
- **guest 要承受重入**：guest 呼叫會分派事件的成員時，自己的 listener 可能在那個呼叫裡被呼叫，等待期間也會收到事件；C 的 guest 的全域狀態要自己顧好。一個 listener trap，整個 Store 結束（`details.md` 的「重入的原型」）。
- **用 C 寫應用程式很囉嗦**：每個字串、每個 handle 的釋放都要自己寫，TodoMVC 的 guest 會比 JS 版長很多，也不是網頁作者實際會用的寫法；它證明的是 host 這一側夠用，不是好不好寫。

## 理由和其他做法

- **不做**：`dom_wasm` 一直只有測試在用，共用 heap 的 wasm GC（RFC 0005）也沒有真的頁面走過。
- **把舊的實作從 git 拿回來**：它接的是 cppgc 上的 DOM、`func_new` 加 `Val` 的共用 host 函式、WIT 的 resource，三樣都換掉了。拿回來的只有設計（宣告的寫法、載入的順序、「需要時才跑」的 Store），程式重寫。
- **wasm 經過 JS 碰 DOM（Chrome 的做法）**：machopo 做 wasm 文件的理由就是不經過 JS，這份不重新討論。
- **替 guest 做一個 SDK（Rust，或 C 的一層有型別的 API）**：寫應用程式比較自然，但要多維護一份在 machopo 的 workspace 外面的程式，還要決定 handle 的計數、closure 的表怎麼包；這份的目標是 host 這一側，C 的 binding 最薄、最接近 host 本身（RFC 0089 的決定 33 的理由），不選。
- **先做一個比 TodoMVC 小的目標（只有點擊的計數器）**：第 3 段做完就是它；TodoMVC 多出來的是成員的數量，放在同一份裡驗收才完整。

## 先例

- **Chromium**：wasm 不能直接碰 DOM，經過 JS 的 glue（`wasm-bindgen` 產生的），字串在邊界複製或用 JS string builtins；瀏覽器沒有實作 Component Model。machopo 的 guest 直接呼叫 host 的 DOM 函式，不經過 JS。對照 Blink 的地方是 binding 的形狀：每個 world 一個 wrapper（Store 的 handle）、receiver 的檢查、`JSEventListener::Invoke`（`ScriptHost::call_listener`）。
- **RFC 0001**（舊的 wasm 文件）：`<meta>` 的宣告、script 的載入和執行時機、每個 script 一個 Store、callback 是 guest 的 id 加每種 callback 一個 export、trap 的處理，這些照舊；async 的 Store 和 WASI 0.3 也照舊（「已決定的事項」第 5 條）；resource 的 handle、Lua 不照舊。
- **RFC 0089 的決定 34、35**：整數的 handle、每個成員一個有型別的 `func_wrap`、WIT 從宣告表產生，是這份的地基。

## 已決定的事項

1. **guest 維持 C，不做 Rust 的 SDK**（2026-11-10，使用者）：TodoMVC 的 guest 和 `dom_wasm` 的測試 guest 一樣用 C 寫，直接用 `wit-bindgen` 的 C binding，工具鏈不變。不選 Rust 的 SDK（草稿原本的看法：寫應用程式比較自然）：多一份要維護的程式，這份要驗證的是 host 這一側。
2. **文件用 `<meta name="script" content="wasm">` 宣告成 wasm 文件，不加命令列的開關**（2026-11-10，第 1 段開始前）：parse 之前掃前 1024 byte（`crates/renderer/src/encoding.rs` 的 `prescan_metas`，和 `<meta charset>` 同一次掃），engine 照它選；頁面的 engine 是整頁一個、跨換頁沿用，所以換到另一種文件時換 engine。不選只有 `--script wasm`（每份文件都是 wasm）：測試頁不能自己宣告，要另外開一輪帶開關的 machopodriver；而且 reftest 在同一個 renderer 裡交替載入 wasm 的測試頁和沒有 script 的參照頁（算 JS 文件），換 engine 省不掉。不選兩者：舊的開關是為了不建 isolate 的設定和示範頁，都不在範圍內，之後要時再加。代價：DevTools 在第一份文件之前建的 engine 是 V8 的，碰到 wasm 文件要換掉，「換掉時不壞」要有測試。對照 Chromium：沒有對應（文件只有 JS），最接近的是 `<meta charset>` 的 prescan。
3. **wasm 文件不建 V8 的 isolate**（2026-11-10，第 1 段開始前）：wasm 的 engine 自己不帶 isolate，DOM 的 heap 沒有 engine 驅動、elise 自己 GC、有 nursery，和 `dom_wasm` 現在測試的一樣。不選也建一個 isolate 給 DevTools 和 WebDriver 的 script 用：heap 變成 V8 驅動的（沒有 nursery），節點的 engine word 要給 V8 的 wrapper（main Store 的 handle 現在放在那裡），V8 隨時可以開始 mark、要重新設計怎麼交出 wasm stack 上的 root；等於重做 handle 和 GC，只為了範圍外的功能，每頁還多一個幾乎不用的 isolate。代價：wasm 文件裡 DevTools 的 Console、WebDriver 執行 script 不能用；`test.py reftest` 跑不了 wasm 的頁（wptrunner 的 reftest 執行器靠執行 script 等頁面載入），第 1 段的驗收改成一份情境加一個 renderer 的測試（「測試」）。情境照樣能跑：它的查詢和輸入不經過 script。對照 Chromium：Chrome 的 wasm 一定在 isolate 裡、DOM 經過 JS；這是刻意不同的地方。
4. **wasm 的 engine 在 `dom_wasm` 裡另寫，重複的片段搬成 `dom_api` 的函式**（2026-11-10，第 1 段開始前）：它自己實作 `ScriptEngine` 和 `ScriptHost`，用不到的方法留空；和 `V8Engine` 完全一樣的片段（例如找最早到期的 timer）搬進 `dom_api`，兩邊呼叫同一個。不選把 engine 的骨架抽進 `dom_api`、V8 和 wasm 各當 backend：邏輯本來就在 `dom_api` 共用了，剩下重複的只是薄的一層，卻要重寫整份 `crates/dom_js/src/engine.rs`，V8 每個 frame 一個 context、microtask 的時機都在那一層，改壞了是 JS 文件壞；之後重複到難維護時再另開。不選完全不共用：碰到一樣的片段沒有理由寫兩次。執行時的成本三者一樣。對照 Chromium：Blink 只有一個 engine；`core/` 放邏輯、`bindings/core/v8` 放呼叫 script 的那一側，和 `dom_api` 對 `dom_js` 的切法一樣，wasm 是另一個 bindings。
5. **Store 是 async 的，guest 用 WASI 0.3**（2026-11-10，第 1 段開始前，使用者）：guest 可以等待。會呼叫回 guest 的成員在 WIT 裡宣告成 `async func`、host 用 `func_wrap_concurrent` 定義、用 `call_concurrent` 呼叫 guest 的 export；其他成員照舊是同步的 `func_wrap`。這是 async 的入口裡唯一能重入的路（`details.md` 的「重入的原型」），和 RFC 0001 一樣。不選同步的 Store 當場呼叫（原型確認可行、要做的最少，草稿當時的看法）：guest 不能等待。不選事件延後到呼叫回來之後：順序和 JS 不同。代價：要 Store 的 event loop；`dom_api` 裡會呼叫 script 的路是同步的（`ScriptHost::call_listener`），怎麼接上要 `await` 的呼叫在第 3 段開始前定（「待決定」第 7 條）；guest 的工具鏈要換成 WASI 0.3。對照 Chromium：JS 的 listener 同步、可重入地被呼叫（`JSEventListener::Invoke`），行為一樣；`await` 期間照樣收事件也和 JS 一樣。
6. **所有的 guest 都換成 WASI 0.3，`dom_wasm` 的測試也用 async 的 Store**（2026-11-10，第 1 段開始前，使用者）：`ci.py guests` 的 target 從 `wasm32-wasip2` 改成 `wasm32-wasip3`，wasi-sdk 34.0 和 `wit-bindgen` 0.62.0 不換（`details.md` 的「C 的 guest 和 WASI 0.3 的工具鏈」）；現有的測試 guest（`crates/dom_wasm/tests/guests/dom.c`）一起換，測試的 harness 改成 poll async 的 Store。不選只換頁面的 guest、現有的留在 WASI 0.2 和同步的 Store（草稿當時的看法：現有的 case 不用動）：要維護兩種 target 和兩種 Store，現有的 case 也走不到頁面的那條路。代價：第 1 段多一步搬 harness；改了 target 之後要在有 wasi-sdk 的機器上重編、確認。
7. **WASI 0.3 的時鐘自己實作，其他的 interface 用 wasmtime-wasi；main thread 不跑 tokio 的 runtime**（2026-11-10，第 1 段開始前）：`wasi:clocks` 的 monotonic clock 的等待是 renderer 的延遲 task（和 DOM 的 timer、frame 在同一個佇列上排順序），不是 DOM 的 timer（沒有巢狀 timer 的下限，`clearTimeout` 碰不到），`now` 降到 100 µs；stdout、stderr 印到 console，亂數用 wasmtime-wasi 的，檔案和 socket 接上但一律拒絕（C 的 libc 啟動時就 import 它們，linker 沒給 component 建不起來）。wasmtime 本身的 async 不用 tokio（原型只用 `futures` 的 executor poll）；用 tokio 的是 wasmtime-wasi 的實作，其中要 runtime 在跑的只有時鐘的 `tokio::time::sleep` 和檔案系統的 `spawn_blocking`。不選整套用 wasmtime-wasi、main thread 加一個 tokio 的 runtime：兩個 event loop，誰先醒不確定，喚醒還要橋接回 renderer。不選時鐘不能等：和第 5 條衝突。代價：換 wasmtime 的版本時時鐘的 WIT 改了要跟著改；stdout、stderr 的 stream 沒有 runtime 時能不能用還沒實際跑過，不行的話那個 interface 也自己實作。對照 Chromium：沒有 WASI；Blink 的 timer 都經過 main thread 的 scheduler，不另開 reactor。
8. **入口 `start(document)` 是 `async func`；第一次停下來就算這個 script 執行完了**（2026-11-10，第 1 段開始前）：guest 只有在 `start` 裡能等待（listener 的 export 是同步的：Component Model 規定不是 async 的 export 回傳之前不能等待），它可以一直不回傳；後面的 script 和 `DOMContentLoaded` 不等它，host 記著還沒結束的 `start`，它等的東西到了再 poll。執行時機照 module script：不擋住 parser，parse 完之後照文件順序、在 `DOMContentLoaded` 之前；每個 `<script>` 一個 Store；參數是頁面的 document 的 handle（`Window` 等第 2 段有了節點以外的 handle 再看要不要換）。不選同步的 export：入口和 listener 都不能等待，第 5 條選的 async 就沒有地方用。代價：C 的 guest 的 `start` 要照 callback ABI 寫（不等待的馬上回傳結束）。對照 Chromium：和 module script 的 top-level await 一樣，後面的 script 不等它。這些都照 RFC 0001。
9. **Store 的 event loop「需要時才跑」**（2026-11-10，第 1 段開始前）：Store 平常放在 `StoreSet` 裡；有事要做時（執行 script、分派事件給它、它等的東西到了）借出來、建 `run_concurrent`、poll 到不動、丟掉 future、放回去。guest 的 task 和 host 的 future 留在 Store 裡，等待中的 guest 下一輪接著跑（`details.md` 的「等待中的 guest，event loop 丟掉再重建」）。一輪就是 `StoreSet` 現在的一次借出：兩輪之間 Store 沒借出，DOM 的安全點照常 GC、`StoreSet` 交出它的 root；分派事件給它時借出來、呼叫、放回去，listener 在分派回來之前跑完。下一輪由 host 自己排（一個 renderer 的 task），不靠 waker：上一輪的 waker 跟著 future 丟掉了，所以 guest 能等的只有 machopo 自己知道什麼時候到的東西（第 7 條的時鐘）。guest 讓出（callback code 的 `YIELD`）時在同一輪裡繼續 poll，不丟掉 future（RFC 0001 的決定 41 在 wasmtime 49 上踩過：已經取出的工作會不見；50 上用測試確認）。不選每個 Store 一個常駐的 `run_concurrent`：Store 永遠是借出的，DOM 的安全點不能做要讀 root 的 GC，分派事件給它也拿不到 Store、只能排隊。限制：用 wasm GC 的語言寫的 guest 暫停時 stack 上的 root 沒有驗證過（C 的 guest 只拿整數的 handle，不受影響；這份只做 C）。對照 Chromium：沒有對應；task 和 microtask 都由 main thread 的 scheduler 排，script 的 runtime 沒有自己的 loop。照 RFC 0001。
10. **頁面的 DOM 是 wasm 的 engine 擁有的，跑 guest 的一輪時借進 `dom_wasm` 的 thread-local**（2026-11-10，1c 開始前）：renderer 經過 `ScriptEngine` 的 `dom_mut()`、`dom_and_layout()` 拿 `&mut Dom`，所以 DOM 平常是 engine 的；host 函式和 wasmtime 借 heap 的 provider 是 linker 和 engine 的、沒有 wasm engine 的參照，從 thread-local 找 DOM，所以一輪開始前把 `Dom` 搬進 thread-local 的格子、結束時搬回來（heap 本身不動）。guest 只有在一輪裡碰得到 DOM，和第 9 條的模型一致；兩輪之間 thread-local 是空的，這時找 DOM 的呼叫回錯誤。不選 DOM 留在 thread-local、把 `ScriptEngine` 拿 DOM 的方法改成傳 closure 或回傳 guard：`ScriptEngine`、`V8Engine` 和 renderer 每個拿 DOM 的地方都要改，為了 wasm 動 JS 那條路（和第 4 條的方向相反）。不選 thread-local 放指向 engine 的 DOM 的指標：要 `unsafe`，`dom_wasm` 不寫。對照 Chromium：Blink 的 DOM 屬於 isolate，binding 從 isolate 找到它；「跑 script 時才借得到」是 machopo 自己的規則。
11. **TodoMVC 用 Speedometer 3.1 的 vanilla ES5 版，複製進 `tests/sites/todomvc/`**（2026-11-10，1e 開始前，使用者）：它的 `dist/`（HTML、`base.css`、`index.css`、七份 JS，約 38 KB）原樣放進 repo，MIT 的授權聲明一起（`tests/sites/README.md` 記來源和 commit）；wasm 版的頁面用它的 markup 和樣式表、把 script 換成 guest，JS 的對照版就是它。不選測試頁直接指到 `../third_party/speedometer`：`cargo test` 會依賴 repo 外面的目錄，只有 `test.py` 的指令會先同步它。不選照規格自己寫一份：和 JS 版不是同一份 CSS，還要另外寫 JS 的對照版。guest 的原始碼和 `.wasm` 先放在 `crates/dom_wasm/tests/guests/`（`ci.py guests` 編的那個資料夾），頁面在 `tests/scenarios/pages/`。對照 Chromium：Speedometer 整份在 `third_party/`，測試從那裡讀；machopo 的 `../third_party` 在 repo 外面。
12. **Store 的 handle 表一張，每格指任何一種物件、記它的 interface**（2026-11-10，第 2 段開始前）：格子是 `Member<Erased>`（節點和 `Event`、`Window`、`Location` 這些都放得進去），heap 外記的從節點的種類（`NodeKind`）換成那個物件最衍生的 interface 的編號（元素的照它的名稱，`dom_api` 的 `interface_of`），交出時算一次；`this` 的檢查是查「這個 interface 是不是成員的 interface 或它的子孫」的一個 bit，照舊不碰 DOM。main Store 的編號放在物件的 engine word：每個 DOM 物件都有放 wrapper 的那一格（`Cx::object_wrapper_edge`），所以「一個物件在一個 Store 裡一個編號」對所有物件都成立。guest 那一側還是一種 handle、一個 `release`。這同時解決兩件事：元素自己的 interface 的成員（`HTMLInputElement.value`、`checked`，現在因為表只記種類而整個排除）和節點以外的物件。不選非節點的物件每次交出配新的一格（草稿當時的看法，以為只有節點有 engine word）：比不出是不是同一個物件，也沒有比較好做。不選節點和其他物件分兩張表：guest 要分兩種 handle、兩個 `release`，元素的 interface 還是要另外解決。不選其他物件用 WIT 的 resource：兩套機制，RFC 0089 的決定 34 替節點否決過的缺點都在。限制：元素的 interface 交出時記下來之後不變（custom element 升級會改它；wasm 文件沒有 custom element）。對照 Chromium：每個 wrapper 帶著 `WrapperTypeInfo`，receiver 的檢查沿 parent 鏈比；每個物件在每個 world 最多一個 wrapper。
13. **WIT 的成員是明列的清單，照需要加**（2026-11-10，第 2 段開始前，使用者）：`wit::FROZEN` 改成 WIT 的清單，繼續用；進清單的成員要符合規則（`this`、參數、回傳值是任何 interface 的物件、字串、數字、布林），第 2 段加 TodoMVC 用到的和同一個 interface 上成套的：`HTMLInputElement` 的 `value`、`checked`；`HTMLElement` 的 `focus()`、`blur()`、`click()`、`style`；`Element` 的 `remove()`、`nextElementSibling`、`previousElementSibling`、`classList`、`children`、`querySelectorAll`；`Document` 的 `body`、`defaultView`、`querySelectorAll`；`DOMTokenList` 的 `add`、`remove`、`toggle`、`contains`、`length`、`item`；`CSSStyleDeclaration` 的 `getPropertyValue`、`setProperty`、`removeProperty`、`cssText`；`HTMLCollection` 和 `NodeList` 的 `length`、`item`；`Event` 的 `type`、`target`、`currentTarget`、`preventDefault()`、`stopPropagation()`、`defaultPrevented`；`KeyboardEvent.key`；`MouseEvent` 的 `button`、`detail`；`Window` 的 `location`、`document`；`Location` 的 `hash`、`href`（實際的名稱和有沒有照表上的為準，做的時候對過）。兩種不是 plain 的型別：不定個數的字串（`classList.add`）在 WIT 是 `list<string>`；可省略的 dictionary（`focus(options?)`）先不放那個參數、等於用預設值，之後要時加成 record。union、callback、promise、`any` 照舊不進。不選符合規則的全部進 WIT（2026-11-10 算的：1757 個，現在 68 個）：每個成員是一個各自單型化的 host 函式，`dom_wasm` 的編譯時間和執行檔會明顯變大（沒有量），每個頁面的 linker 要登記那麼多函式，一千多個沒有 guest 測過的函式進了對外的介面；之後要全部時拿掉清單就好，反過來難。對照 Chromium：Blink 替 IDL 的每個成員產生 binding，沒有清單（machopo 的 JS 那一側也是）。2b 做的時候改的兩處：事件的 interface 的成員（`Event`、`UIEvent`、`MouseEvent`、`KeyboardEvent`）留到第 3 段和 listener 一起加，因為 guest 還拿不到 event、加了也測不到；`focus()`、`blur()`、`click()` 先當普通的函式放進 WIT（guest 還沒有 listener，分派時沒有東西要呼叫回 guest），第 3 段定了重入怎麼接時再照第 5 條改成 `async func`。

## 待決定

第 1 段開始前的都決定了（「已決定的事項」第 2 到 9 條）。

第 2 段開始前：

5. （已決定，「已決定的事項」第 12 條。）
6. （已決定，「已決定的事項」第 13 條。）

第 3 段開始前：

7. `dom_api` 裡同步的、會呼叫 script 的路怎麼接上要 `await` 的 `call_concurrent`（把那幾條路變成 async、V8 那邊 poll 一次就完成，RFC 0001 的做法；或 wasm 這邊另外一步一步地分派）；callback 的 export 的形狀、host 告訴 guest 釋放 callback 的時機、timer 和 `requestAnimationFrame` 做不做。

第 4 段開始前：

8. （TodoMVC 的來源、guest 和頁面放哪已決定，「已決定的事項」第 11 條。）互動的情境怎麼和 JS 版比（同一份情境對兩個頁面跑，或各一份）。

## 之後的可能

- wasm 版 TodoMVC 和 JS 版的量測（每次呼叫的指令數、整個流程的時間）。
- 用 wasm GC 的語言寫的 guest（物件和節點在同一個 heap，RFC 0005 做好的那一半）。
- guest 的 SDK（Rust 或其他語言）、Lua 的 runtime。
- timer、`fetch`、observer、custom element 這些 callback 比較多的 API。
- wasm 文件的子 frame、DevTools。
