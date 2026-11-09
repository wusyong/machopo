# RFC 0008：DOM API 擴充和 WPT 的 testharness 測試

- 開始日期：2026-10-05
- 狀態：✅ 已完成：2026-10-06（接受和排程：2026-10-05；Windows 11，GC 的檢查在 macOS 上；`innerHTML`、`MutationObserver` 等另開 RFC，「已決定的事項」第 50 條）
- 細節：`details.md`；進度：`progress.md`

## 摘要

擴充 `dom_api` 的宣告表，直到 WPT `dom/` 底下的 `testharness.js` 測試能在 kun 裡跑，並且把它們變成自動測試。主要有四塊：

- **runner**：headless 的 kun 經過本機的 HTTP server 載入 WPT 的檔案。kun 自己的 `testharnessreport.js` 收集結果，經過只在 `RendererFlags::web_tests` 時才有的 hook 交給 shell，再和每個測試的預期結果比對。
- **API**：照 WPT 的使用頻率分批補齊，每一批都同時給 JS（`dom_js`）和 wasm（`dom_wasm`、`kun:dom` 的 WIT）。
- **宣告表**：多支援 WPT 需要的 WebIDL 形式，例如常數、可變參數、union、集合的 index getter、iterable。
- **`kun:dom` 退回 0.x**：介面還不穩定，package 從 `kun:dom@1.0.0` 改回 `0.1.0`，既有的函式和字典可以直接改（「已決定的事項」第 1、2 條）。

## 動機

**開始前（2026-10-05）**：
- **WPT 只跑 reftest**：`testharness.js` 的測試沒有跑（`rfcs/pre-rfcs/P3.md` 的「已決定的事項」第 6 條；`docs/test-gaps.md` 的「測試守不住的地方」）。DOM API 的正確性只靠 `dom_api`、`dom_js` 自己寫的測試，和規範的差距看不出來。
- **DOM API 是「基本頁面用得到的最小集合」**：
  - 沒有 `childNodes`、`nodeType`、`querySelectorAll`、`getElementsByTagName`，也沒有任何集合型別（`NodeList`、`HTMLCollection`）；
  - 事件只有 `Event`、`UIEvent`、`MouseEvent` 的一部分（`docs/limitations.md` 的「Bindings 和 DOM API」、「事件」）；
  - `dom` 的節點不記得自己的 document，所以沒有 `ownerDocument`，也沒有 `createHTMLDocument`。
- **新增 API 要同時顧到兩個前端**：
  - 宣告表已經讓一般的成員只要寫一次（`crates/dom_api/README.md` 的「怎麼新增一個 API」）。
  - 但 `kun:dom@1.0.0` 定下來之後只能新增函式（RFC 0001 的「已決定的事項」第 20 條）。
  - 照 WPT 一路補 API，會碰到要改既有成員（降階的 `classList*` 換成真的 `DOMTokenList`）、字典要長出新成員的情況；WIT 的 record 多一個欄位就不相容，而且 WIT 的 `@since` 不能標在欄位上。

目標：
- `shell` 的測試跑匯入的 WPT `testharness.js` 測試（2026-10-07 起由 `python3 scripts/test.py wpt` 跑，見第 52 條），結果以 subtest 為單位和預期比對。預期以外的通過或失敗都讓測試失敗，和 `4ff8d6e:tests/wpt/reftest-expectations.txt` 一樣。
- `testharness.js` 本身在 kun 裡能正常運作：載入、計時、`async_test`、`promise_test`、未捕捉的例外變成 harness 的錯誤。
- 照 WPT 的使用頻率分批補 API（見「分階段」），每一批的成員同時出現在 JS 和 `kun:dom` 的 WIT。
- 宣告表和規範的差距看得見：用 WPT 的 `interfaces/*.idl` 檢查宣告表，刻意降階、還沒做的成員都列出來。
- `kun:dom` 回到 0.x，這份 RFC 期間可以做不相容的改動；不相容的改動要有標示，讓舊的 guest 清楚地失敗，而不是呼叫到一半出錯。

不在範圍內：
- **iframe 和多個 browsing context**（`contentDocument`、`window.open`、`postMessage`）：`dom/` 裡約 85 個檔案用到，多數只是要一份空白的 document。之後另開 RFC；這份只做不屬於任何 browsing context 的 document（`createHTMLDocument`）。
- **XML**：XML parser、`.xhtml` / `.svg` 的測試檔、`XMLDocument` 的完整行為。`createDocument` 這類 API 的範圍見「已決定的事項」第 46 條。
- **shadow DOM、`Range`、`moveBefore`、CSSOM（`.style`、`getBoundingClientRect`）、捲動**。
- **`testdriver.js`**：要 WebDriver 的後端，`dom/events` 裡將近一半的測試用它。
- **worker、`fetch`、`XMLHttpRequest`**：idlharness 要 `fetch`，見「之後的可能」。
- **wptserve 的功能**：`.sub.` 的替換、`.headers` 檔、跨來源。
- **wasm 跑 WPT**：WPT 的測試是 JS。wasm 的前端怎麼驗證見「測試」。

## 設計概要

**給寫網頁的人**：
- 常用的 DOM API 都有了，例如 `childNodes`、`children`、`querySelectorAll`、`getElementsByClassName`、`append`、`remove()`、`cloneNode`、`CustomEvent`、`createEvent`、`ownerDocument`、`document.implementation.createHTMLDocument`。
- 行為照規範，由 WPT 驗證。

**給 wasm 的 guest**：
- 同樣的 API 也在 `kun:dom` 裡，guest 用 wit-bindgen 重新產生 binding 就有。
- `kun:dom` 是 0.x，還不穩定：
  - 既有的函式和字典（record）可能改，例如 `event-init` 多了 `composed`（0.2.0）、`classList*` 換成 `dom-token-list` 的 resource（0.3.0）；
  - 不相容的改動會升 minor（`0.1` → `0.2`，只新增函式不升，「已決定的事項」第 4 條）。照舊版本編的 guest 在新的 kun 上載入時就失敗（wasmtime 的 linker 對 0.x 比對到 minor，`details.md` 的「wasmtime 和 wit-parser 的版本機制」），要重新產生 binding、重新編。
- 第 0 步的改版（`1.0.0` → `0.1.0`）本身也是這樣：之前編的 guest 都要重新編。

**給改 kun 的人**：
- 加 API 的流程不變：模組裡寫函式、宣告表加一行、`dom_api` 的測試。多出來的是：
  - 跑 WPT，把預期結果更新進 repo；
  - WIT 有不相容的改動時升 minor，重新編 repo 裡的 guest（`python3 scripts/ci.py guests`）；
  - 用了新的宣告表形式（常數、集合、union…）時，兩個前端和 Lua 的產生器各要支援一次。
- 跑 WPT：

  ```sh
  python3 scripts/test.py wpt                              # 全部
  python3 scripts/test.py wpt nodes/                       # 名稱有 nodes/ 的
  MACHOPO_UPDATE_EXPECTATIONS=1 python3 scripts/test.py wpt    # 重新產生預期結果，審查 diff
  ```

## 詳細設計

這一節的設計在 2026-10-05 確認（「已決定的事項」第 5 條），實作時改過的地方寫在各段；各選擇的比較見「理由和其他做法」和 `details.md`。驗收的結果見 `progress.md`。

### 1. runner

- **檔案從本機的 HTTP server 載入**：
  - WPT 的測試都用 `/resources/testharness.js` 這種從根目錄開始的路徑，從 `file:` 載入時會指到檔案系統的根（`docs/limitations.md` 的「Parser 和載入」也記著這一點）。
  - 把 `net` 的 `test_server`（只用標準庫的小 HTTP server）擴充成能以一個目錄為根提供靜態檔案，runner 以 WPT 的 checkout（`../third_party/wpt`，第 28 條）為根，用 `http://127.0.0.1:PORT/dom/nodes/...` 載入。
  - 它也照 wptserve 做兩件事：
    - `X.any.js` 包成 `X.any.html`、`X.window.js` 包成 `X.window.html`（處理 `// META: script=`、`timeout=long`、`title=`；`variant=` 只跑沒有 query 的那一個，第 55 條）；
    - `/resources/testharnessreport.js` 換成 kun 的版本。
  - 原本也要把 `/resources/WebIDLParser.js` 改寫到 `webidl2`，那是 idlharness 用的；idlharness 還跑不了（「之後的可能」），沒有做。
- **kun 的 `testharnessreport.js`**（`4ff8d6e:crates/shell/src/testharnessreport.js`）：
  - 呼叫 `setup({output: false, message_events: []})`：不畫結果表，所以不需要 `Output` 用到的那些 DOM；也不需要 `postMessage`。
  - 再用 `add_completion_callback` 把結果組成 Chromium 的 baseline 格式（第 11 條），交給 `testRunner.reportHarnessResults`。
- **`testRunner`**：
  - 只在 `RendererFlags::web_tests` 時（`FrameClient::web_tests`）裝在 realm 的 global 上，對應 Chromium content_shell 的 `testRunner`；不在宣告表裡，所以不進 `kun:dom`。
  - 結果交給 `FrameClient::report_testharness_results`，render thread 在 task 之後經過 `RendererHostMsg::TestharnessResults`（像 `ReftestWaitEnded` 那樣）送回去，headless 的 `run_testharness` 等它。
- **計時**：harness 自己計時，10 秒，有 `<meta name=timeout content=long>` 時 60 秒，時間到時交出已完成的結果和 `Harness: timeout`；`MACHOPO_WPT_TIMEOUT_MULTIPLIER` 把它乘上幾倍（第 54 條）。runner 另外有一個 deadline（harness 的時間再加 15 秒），頁面卡住、從沒交出結果時，結果是 `Runner: no testharness.js results before the deadline`。預期結果說會等到逾時的測試先開始、各自一個 thread，不佔 worker（第 52 條）。原本打算像 wptrunner 那樣 `explicit_timeout: true`、由 runner 在頁面裡呼叫 `timeout()`；實作時改掉：shell 沒有在頁面裡執行 script 的方法，而 harness 自己的計時效果一樣。
- **匯入和預期結果**：
  - runner 自己的測試頁在 `4ff8d6e:tests/testharness/`（從 `/kun/` 提供，同樣的比對），WPT 的測試直接從上游的 checkout `../third_party/wpt` 讀（第 28 條）；`.xhtml` 和 `.svg` 的測試要 XML parser，不找。
  - 以整個子目錄為單位列進 `4ff8d6e:tests/wpt/testharness-include.txt`：`dom/collections`、`dom/lists`、`dom/nodes`、`dom/events`、`html/webappapis/timers`。原本也列了 `dom/traversal`、`dom/abort`、`html/webappapis/animation-frames`，它們要的 API（`TreeWalker`、`AbortController`）留給第 50 條的新 RFC。WPT 的 commit 固定在 `third_party.toml`，換 commit 照 `tests/wpt/README.md` 的「Updating」。
  - skip 清單（`4ff8d6e:tests/wpt/testharness-skip.txt`）只放跑了也得不到結果的（第 21 條）：testdriver、整個檔案都在等 iframe 或 shadow DOM（沒有任何結果、等到逾時）、CSS 動畫、`.sub`、太慢的；只有部分 subtest 用到 iframe 或 shadow DOM 的照跑，失敗記在 `-expected.txt`，其他 subtest 的結果不會少掉。
  - 每個沒有全部通過的測試檔有一份預期結果，照 Chromium 的 `-expected.txt`（「已決定的事項」第 3、11 條）：
    - 放在 `4ff8d6e:tests/wpt/baselines/` 裡和測試相同的路徑（`dom/nodes/Node-cloneNode.html` 的是 `4ff8d6e:tests/wpt/baselines/dom/nodes/Node-cloneNode-expected.txt`）；沒有測試讀的 `-expected.txt` 讓 runner 失敗；
    - 內容是只有沒過的 subtest 和 harness 的狀態；全部通過的測試沒有這個檔案；
    - runner 比對整份文字，`MACHOPO_UPDATE_EXPECTATIONS=1` 時重寫它們，審查看 diff；失敗的原因寫在 `progress.md` 或 skip 清單的分組裡。
- **在哪裡跑**：`shell` 的一個測試 target，由 `python3 scripts/test.py wpt` 跑；`cargo test --workspace` 跳過它，CI 照樣跑（第 52 條）。每個測試檔開一次 headless 的頁面，平行跑。總時間的評估見「已決定的事項」第 16、52 條。

### 2. harness 本身需要的 API

`details.md` 的「testharness.js 需要什麼」有逐項的出處。第 0 步補的：
- `window.parent`（最上層時是自己）。原本也列了 `window.opener`，harness 實際上用不到，沒有做。
- `document.getElementsByTagName`：回傳 `HTMLCollection`，所以集合型別在第 0 步就要有，見下面第 4 點。
- `CharacterData.data`：`<title>` 的文字是測試的名稱。
- window 的 `error` 事件（`ErrorEvent`）和 `unhandledrejection`（`PromiseRejectionEvent`）：
  - 未捕捉的例外現在只印到 stdout；
  - harness 要靠這兩個事件把它們變成 harness 的錯誤，否則只會逾時。
  - 這是 HTML 的「report an exception」，兩個前端都會用到（wasm 的 trap 不分派，「已決定的事項」第 9 條）。
- `HTMLMetaElement` 的 `name` 和 `content`（讀 timeout 的設定）。`HTMLScriptElement` 的 `src`（失敗時裁 stack 用，找不到有退路）先不做（第 7 條）。

### 3. `dom` 的資料模型

- **節點記得自己的 document**（Blink 的 `Node::GetDocument`、`TreeScope`；第 44、45 條）：
  - 每個節點有一條指向它的 document 的 `Edge`（document 的指向自己），建立節點的函式都帶 document；`ownerDocument`、事件 handler 的 window（`<body onload>`）、quirks mode 都讀它，不再從樹的根往上找，所以不在樹裡的節點也有。
  - 插進另一份 document 的樹、或 `adoptNode` 時 adopt：子孫、元素的 `Attr` 改成新的 document，`<template>` 的內容改成新 document 的 template contents owner document（第 47 條）；對 style 和 layout 來說是新的節點，computed style 清掉、flag 重新開始。adopt 只寫 edge，論證在 `crates/dom/soundness.md`。
- **同一個 heap 上的多份 document**：
  - `createHTMLDocument`、`createDocument`、`new Document()` 的 document 不屬於任何 browsing context：沒有 window，不 render；event handler content attribute 不編譯（讀到 `null`，元素移回有 window 的 document 之後照常編譯），`<a>` 不換頁（規範是 document 不是 fully active 就不做，否則會讓執行 script 的 realm 的 frame 換頁）。用 `addEventListener` 加的 listener 照常執行，和規範一樣。
  - document 記得自己是 HTML 還是 XML（大小寫的規則，第 46 條）、是不是 `XMLDocument`，以及 content type、網址、編碼（renderer 在導覽時照回應設定，script 建立的是 `about:blank`、UTF-8）。
  - renderer 的 hit testing 用最後一次 paint 的結果；那之後被移到別的 document 的節點不收點擊（改給根元素），否則它的 listener 會在別的 document 裡執行。
  - `dom_tree_version` 和 timer 照舊每個 heap 一份：改了別的 document，畫面上那份下一個 frame 也跑一次 style（結果不變，`docs/limitations.md`）；timer 只有一個 window。
  - 導覽時 renderer 本來就在同一個 heap 上建新的 document，所以「一個 heap 多份 document」本身不是新的情況（2026-10-06 開始第 5 步前的調查）。
- **集合**（`NodeList`、`HTMLCollection`）：見第 4 點。
- **HTML 元素的 interface**（`HTMLElement`、`HTMLMetaElement`、`HTMLScriptElement`…）：
  - 元素照 namespace 和 local name 對應到 interface（HTML 規範的表），`dom_api` 的 `interface_of` 決定 wrapper 的 prototype 和 WIT 的 resource。
  - `dom` 的 `NodeKind` 照舊只有 `Element`（不新增 GC 的種類）；`dom_api` 的 `ObjectKind` 依 namespace 和 local name 細分（`HtmlElement`、`HtmlMetaElement`…），interface 的方括號照舊列出實作它的種類。

### 4. 宣告表要多支援的形式

每一種都要決定在宣告表、JS、WIT 裡各是什麼樣子，見下表。

| 形式 | 例子 | JS（`dom_js`） | WIT |
| --- | --- | --- | --- |
| 常數 | `Node.ELEMENT_NODE`、`Event.AT_TARGET` | interface object 和 prototype 上的唯讀屬性 | 沒有常數：在 WIT 的註解列出值（binding 產生器讀不到，guest 自己寫值，第 26 條） |
| 可變參數 | `append(...nodes)`、`classList.add(...tokens)` | 收集成陣列 | `list<T>` |
| union | `(Node or DOMString)`、`(AddEventListenerOptions or boolean)` | 照 WebIDL 的 union 轉換 | `variant` |
| index 和名稱的 getter | `collection[i]`、`collection.foo` | V8 的 indexed / named property interceptor | `item(index)`、`named-item(name)` |
| iterable | `for (const n of childNodes)` | `Symbol.iterator`、`keys`、`values`、`entries`、`forEach` | `length` 加 `item`，不另外加 |
| `[SameObject]` 的集合 | `childNodes`、`children` | 同一個 wrapper | 同一個物件，handle 照舊每次都是新的 |
| callback interface | `handleEvent` 的物件 | 物件或 function | 照舊是 `callback`（guest 自己分派） |
| `sequence<T>`（第 2a 步） | `composedPath()` | 陣列 | `list<T>` |
| `[PutForwards=x]`（第 42 條） | `el.classList = "a b"` | setter 設回傳的物件的 `x` | 沒有（guest 呼叫 `x` 的 setter） |
| `stringifier`（第 43 條） | `String(classList)` | prototype 的 `toString` | 沒有（guest 讀那個 attribute） |
| 沒有預設值的 `optional T`（`Optional<T>`，第 4c 步） | `toggle(token, force)` | 只有 `undefined` 是沒給，`null` 照 `T` 轉換 | `option<T>` |
| `[LegacyNullToEmptyString]`（第 0b、3c 步） | `data = null` | 只有 `null` 是空字串，`undefined` 是 `"undefined"` | `option<string>` |
| `any`（第 8 條） | `CustomEvent.detail` | 存在 `dom` 的 slot 的 script 值 | 不進 WIT（只給 JS） |

- **集合是 GC 上的物件，live**（Blink 的 `LiveNodeList`、`HTMLCollection`）：
  - 記著根和過濾條件；
  - 用 `dom` 現有的 `dom_tree_version` 決定快取的長度和位置還能不能用；
  - `childNodes`、`children` 存在節點上（`[SameObject]`）。
  - `querySelectorAll` 回傳的是 static 的 `NodeList`。
- **降階的成員**：真的 interface 做好之後直接取代降階的成員。`classList` 在第 4 步這樣換掉了：JS 拿掉 `glue.js` 裡的那一段，WIT 的 `class-list-add` 等四個函式換成 `dom-token-list` 的 resource（不相容的改動，`kun:dom` 0.3.0）。`performance` 還是降階的（`Window.performanceNow`，`glue.js` 組出 `performance.now()`），等 `Performance`。
- **一個參數是不是必要的**（JS 的 `length`、參數數目的檢查）：沒有預設值、不是可變參數；dictionary 參數只有沒有必要成員時才能省略（第 6a 步比對規範時發現 `PromiseRejectionEvent` 的錯）。

### 5. `kun:dom` 怎麼演進

- **退回 0.1.0**（「已決定的事項」第 1 條）：還不想穩定介面。之後升了兩次：0.2.0（第 2a 步，`event-init` 多了 `composed`）、0.3.0（第 4c 步，`class-list-*` 換成 `dom-token-list`）。RFC 做完時留在 0.x（第 53 條）。第 0 步改了這些地方：
  - `crates/dom_api/src/wit.rs` 的 `PACKAGE` 和模組的說明；
  - golden file `crates/dom_api/tests/machopo-dom.wit`；
  - `crates/dom_wasm/src/store.rs` 寫死的 `kun:dom/guest@1.0.0`；
  - `sdk/rust/kun_guest/wit/world.wit` 的 `include`；
  - repo 裡所有編好的 guest（`crates/dom_wasm/tests/guests/`、`sdk/`、wasm 的 reftest、Lua 的 runtime）：它們 import 的是 `@1.0.0`，對不上 `@0.1.0`，要用 `ci.py guests` 重新編；
  - 寫到「1.0.0、只能新增」的文件：`crates/dom_api/README.md`，以及 RFC 0001 的第 20 條和「介面有版本」那一段（改成指向這份 RFC）。
- **不相容的改動直接做**（第 2 條）：既有的函式、字典的成員、降階的成員都可以直接改，不另外保留舊的版本，也不用 `@since` / `@unstable`。
- **怎麼讓舊的 guest 清楚地失敗**：
  - wasmtime 的動態 host 函式在連結時幾乎不檢查型別：函式的參數改了、record 多了欄位，照舊版本編的 guest 照樣載入，要到呼叫時才出錯，或在回傳時 trap（`details.md` 的「wasmtime 和 wit-parser 的版本機制」）。
  - 不相容的改動升 minor（`0.1.0` → `0.2.0`），只新增函式不升（「已決定的事項」第 4 條）。wasmtime 對 0.x 比對到 minor，舊的 guest 在 `instantiate_pre` 就失敗，錯誤訊息寫出它要的版本；只新增時，舊的 guest 照樣能跑。
  - golden file 照舊守住「沒有意外的改動」；審查 diff 時判斷是不是不相容、該不該升版。

### 6. 宣告表和規範的比對

- 讀 WPT checkout 裡的 `interfaces/dom.idl`、`html.idl`、`uievents.idl`、`pointerevents.idl`、`hr-time.idl`（第 59 條），用測試自己的小 parser（第 56 條）。
- `dom_api` 的測試 `spec` 把宣告表的每個 interface 和 dictionary 對上 IDL（partial 和 `includes` 的 mixin 合起來），比形狀、不比型別（第 57 條），產生兩份清單：
  - 宣告表和規範不同的（宣告表多出來的成員或 interface、形狀不同、parent 不同）：每一項都要列在 `crates/dom_api/tests/spec-differences.txt`，寫明原因（第 58 條）；沒列的、或列了但已經沒有差異的，都讓測試失敗。
  - 規範有、宣告表沒有的：只列 kun 有宣告的 interface 和 dictionary，存成 golden file `crates/dom_api/tests/spec-missing.txt`（`MACHOPO_UPDATE_GOLDEN=1` 重新產生），看得到每一步補了多少。

### 測試

- **語意**：每個新的成員照舊在 `dom_api` 的 `tests/` 測（不經過 engine），WPT 是第二層。WPT 驗證得到 `dom_api` 的演算法、`dom_js` 的 WebIDL 轉換和 interface 的形狀。
- **WPT**：
  - 匯入的測試以 subtest 為單位和預期比對；
  - 每一步做完時更新預期，`progress.md` 記下通過的數字；
  - 預期是「失敗」的 subtest 記在 `-expected.txt` 裡；原因寫在 `progress.md`（Chromium 的 baseline 也沒有註解）。
- **wasm**：WPT 驗證不到 `dom_wasm` 的 `Val` 轉換和 handle。每種新的宣告表形式（常數以外）、新的物件種類（集合）、新的 `async method`（會呼叫回 guest 的成員），在 `crates/dom_wasm/tests/` 用 guest 測一次橋接，不必每個成員都測。
- **宣告表和規範的比對**（第 6 節）：`cargo test -p dom_api --test spec`。
- **mutation test**：
  - runner 的部分照 `docs/testing.md` 的做法；
  - API 的部分要有 `dom_api`、`dom_js`、`dom` 的測試失敗；WPT 是第二層，沒有要求每個 mutation 都讓 WPT 失敗（原本的計畫；跑一次 WPT 的時間是相關單元測試的好幾倍，而 baseline 裡的失敗訊息不是每次都精確）；
  - 結果寫進 `progress.md`（共 421 個）。
- **runner 本身**：
  - 用幾個自己寫的小 testharness 頁面（全部通過、有失敗、逾時、未捕捉的例外、`promise_test` 的 rejection）確認結果回得來；
  - `.any.js` 的包裝和 META 的處理有單元測試。

### 分階段

順序照 WPT `dom/` 的調查：每一步能讓多少測試檔用到的成員都存在（`details.md` 的「WPT `dom/` 的調查」）。每一步開始前，只決定那一步要決定的事。

0. **runner 和 harness 的前置**：
   - 第 1 節的 runner、第 2 節的 API、集合型別的基本形式（`HTMLCollection` 的 `length`、`item`、index getter）；
   - 第 5 節的退回 0.1.0 和重新編 guest；
   - 先匯入 `dom/collections`、`dom/lists` 和 `html/webappapis/timers` 確認流程。
1. **集合、`ParentNode`、`ChildNode`、`Element`**：
   - `querySelectorAll`、`getElementsByTagName`、`getElementsByClassName`、`NodeList`、`children`、`*ElementChild`、`*ElementSibling`、`childElementCount`；
   - `id`、`className`、`removeAttribute`、`hasAttribute`、`matches`、`closest`、`append`、`prepend`、`remove()`。
   - 調查裡一步解開最多的一組。
2. **事件補齊**：
   - `createEvent`（只對應 kun 有的 interface，第 24 條）、`initEvent`、`initCustomEvent`（第 25 條）、`cancelBubble`、`returnValue`、`srcElement`、`composed`、`composedPath`（沒有 shadow DOM，只回傳路徑）、事件的常數（第 26 條）；
   - `CustomEvent`（`detail` 只給 JS，第 8 條）、`new EventTarget()`（第 23 條）、callback interface（`handleEvent`，第 22 條）；
   - `window.event`（原本不在範圍裡，第 27 條）；
   - 匯入整個 `dom/events`，skip 清單照第 21 條。
   - 動到 GC（listener 的 slot、新的 GC 型別、`Window` 的 edge）。
3. **`Node` 和 `CharacterData`**：
   - `nodeType`、`nodeName`、`nodeValue`、`childNodes`（第 31 條）、`lastChild`、`previousSibling`、`parentElement`；
   - `hasChildNodes`、`cloneNode`（文件先不行，第 32 條）、`isEqualNode`、`contains`、`compareDocumentPosition`（第 35 條）、`Node` 的常數；`replaceChild` 在第 1 步做了；
   - `CharacterData` 的方法（UTF-16 的位置，第 33 條）、`splitText`、`wholeText`、`createComment`、`createDocumentFragment`、`new Text()` 這類建構子；
   - `ProcessingInstruction`（第 34 條）；
   - 清單以外的 `Element.tagName`、`normalize()`、`isSameNode()`、`getRootNode()`（第 36 條）。
   - 動到 GC（新的集合篩選條件、新的節點種類）。
4. **namespace 和 `Attr`**：`createElementNS`、`setAttributeNS` 等、`attributes`（`NamedNodeMap`）、`Attr`、真的 `DOMTokenList`（範圍見第 40 條）。
   - 4a namespace：`createElementNS`、`prefix`、`namespaceURI`、名稱的檢查（第 39 條）、attribute 照規範比對（只有 HTML 元素轉小寫；帶 NS 的方法比 namespace 和 local name，其他的比完整名稱）、帶 NS 的 attribute 方法、`lookupPrefix` 等、`getElementsByTagNameNS`；
   - 4b `Attr`（第 37 條）和 `NamedNodeMap`（第 38 條）；
   - 4c `DOMTokenList`（第 38 條），`classList` 換掉降階的成員，`kun:dom` 升到 0.3.0、重新編 guest；
   - 4d WPT 的 baseline、mutation test、文件。
   - 動到 GC（新的節點種類、兩個新的 GC 型別、rare data 和 `Attr` 的 edge）。
5. **document 的模型**：第 3 節的「節點記得自己的 document」和多份 document、`DOMImplementation`、`createHTMLDocument`、`doctype`、`head`、`importNode`、`adoptNode`（範圍見第 49 條）。
   - `Node-appendChild`、`insertBefore`、`ParentNode-append` 這些最核心的測試，和 `dom/common.js`（traversal 的測試用它），都被它擋住。
   - 5a 節點的 document、adopt、建立節點的 API（第 44、45 條）；
   - 5b `DOMImplementation`、`createHTMLDocument`、`XMLDocument`、`createDocumentType`、`createCDATASection`（第 46、48 條）；
   - 5c `importNode`、`adoptNode`、複製 document、template 的 owner document（第 47 條）；
   - 5d 沒有 browsing context 的 document，和 renderer 那邊的修正（第 3 節的調查）；
   - 5e WPT 的 baseline、mutation test、文件。
   - 動到 GC（每個節點多一條 edge、新的節點種類、新的 GC 型別）。
6. **RFC 做完**：
   - 6a 第 6 節，宣告表和規範的比對（第 51 條）；
   - 6b runner：預期逾時的測試先開始（第 52 條）、`timeout_multiplier`（第 54 條）；
   - 6c 收尾：驗收表補上第 3 到 5 步、剩下的失敗原因、`docs/limitations.md` 和 `docs/testing.md`、「之後的可能」。
   - `kun:dom` 留在 0.x（第 53 條）。`innerHTML`、`TreeWalker` 和 `NodeIterator`、`MutationObserver`、`AbortController` 另開 RFC（第 50 條）。

## 缺點

- **宣告表和兩個前端變複雜**：
  - 每一種新的形式（union、可變參數、interceptor、iterable）都要在宣告表、`dom_js`、`dom_wasm`、`wit.rs` 和 Lua 的產生器各做一次；
  - JS 的 interceptor 是 `dom_js` 第一次用到 V8 的 interceptor API（rusty_v8 的 `IndexedPropertyHandlerConfiguration` / `NamedPropertyHandlerConfiguration` 是 safe API，2026-10-05 確認，`dom_js` 不需要寫 `unsafe`）。
- **guest 一直要重新編**：
  - `kun:dom` 在 0.x 期間，每次不相容的改動都讓之前編的 guest 不能跑，kun 以外的 guest 作者也一樣；
  - repo 裡的 guest 要 `ci.py guests` 重新編，它要 wasi-sdk 34 和 wit-bindgen（第 0a 步確認 Windows 上也能編）。
- **測試時間**：423 個 WPT 測試檔，每個開一次頁面；預期會等到逾時的先開始之後約 10 秒（第 52 條），由 `test.py wpt` 跑。
- **WPT 的更新成本**：換 commit 時，預期結果要重新產生、逐項審查。
- **GC 的改動**：第 5 步改 `dom` 的節點和 trace，是這份 RFC 風險最高的部分。

## 理由和其他做法

完整的比較見 `details.md` 的「沒選的做法」。

- **runner 從 `file:` 載入，把 `/resources/` 改寫成相對路徑**：
  - 不用 HTTP server，但要改寫測試檔，或在 loader 裡特別處理 `/`；
  - `.any.js` 的包裝還是要有人產生；
  - `location` 的值、同源的判斷和真的瀏覽器不同。
  - 不選。
- **結果的回報**：
  - **Servo 的做法**：`console.log` 印出結果，runner 讀 stdout。kun 的 headless 在同一個 process 裡，stdout 要從別的 thread 撈，而且和頁面自己的 `console.log` 混在一起。
  - **寫進 `document.title`**：要先有 title 的 setter，而且頁面自己改 title 時會被蓋掉。
  - 都不選。hook 和 `ReftestWaitEnded` 是同一種東西。
- **預期結果的格式**：
  - wptrunner 的 `.ini` metadata（Firefox、Servo）：要寫 parser，格式是 wptrunner 自己的；
  - 一份總的 expectations 檔（像 `4ff8d6e:tests/wpt/reftest-expectations.txt`）：subtest 的名稱很長、可能有任何字元，一份檔裡上千行不好審查。
  - 選 Chromium 的 `-expected.txt`：每個測試檔一份，diff 直接看出哪個 subtest 變了。
- **WIT 的演進**（比較見 `details.md` 的「WIT 的其他演進方式」）：
  - **留在 1.x，新成員用 `@unstable`，RFC 做完時升 1.1.0、改標 `@since`**：照 1.0.0 編的 guest 一直能跑，但既有的成員和字典不能改，字典要長出成員時只能新增 record 和 `new2` 這種函式，降階的 `classList*` 也要永遠留著。介面還在大幅變動，這些代價不值得。不選（「已決定的事項」第 1 條）。
  - **升 2.0.0**：同樣能改，但版本號表示的是「穩定的第二版」，和實際狀態不符。不選。
- **宣告表從 WebIDL 檔產生**（Blink 的做法）：
  - 不用手寫宣告表，但要寫一個 WebIDL 的 code generator，還要表達宣告表現在有的東西（Rust 函式的路徑、降階、`async method`）；
  - 第 6 節的比對拿到大部分好處（看得見差距），成本小很多。
  - 不選。
- **不做**：DOM API 照舊只有最小集合，WPT 只有 reftest，DOM 的正確性只靠自己寫的測試；wasm 的 guest 拿到的 API 也一直很少。

## 先例

- **Chromium**：
  - **web test**：content_shell 用 `--run-web-tests`，`testharnessreport.js` 把結果轉成文字，經過 `testRunner` 交出去，和 `-expected.txt` 比對；沒有 `-expected.txt` 就表示全部通過。WPT 放在 `web_tests/external/wpt`，經過 wptserve 載入。kun 照它的 hook 和 baseline 的想法。
  - **binding**：DOM 的 binding 從 WebIDL 產生，集合是 `LiveNodeList` / `HTMLCollection`，靠 DOM tree version 讓快取失效，`childNodes` 存在節點的 `NodeListsNodeData` 上。kun 的集合照這個模型。
- **Servo**：wptrunner 加上自己的 `testharnessreport.js`，用 `console.log` 印出結果；metadata 是 `.ini`。
- **WIT 和 WASI**：WASI 在 0.x 期間每次不相容都升 minor（0.1 → 0.2 → 0.3），guest 要重新編；0.2.x 之內才用 `@since` 和 `@unstable` 演進。`kun:dom` 在 0.x 期間照前者。

細節見 `details.md` 的「其他實作」。

## 已決定的事項

1. **`kun:dom` 退回 0.x**（2026-10-05）：package 從 `1.0.0` 改回 `0.1.0`，取代 RFC 0001 的第 20 條「做完時定為 1.0.0，之後只新增函式」。理由：還不想穩定介面；這份 RFC 會大量新增、也要改既有的成員。不選「留在 1.x、用 `@unstable` 和 `@since` 演進」：既有的成員和字典不能改，代價太大。
2. **不相容的改動直接做**（2026-10-05）：既有的函式、字典（WIT 的 record）的成員、降階的成員（`classList*`、`performanceNow`）都可以直接改，不保留舊的版本。不選「新增 record 和 `new2` 這類函式」：WIT 會留下一堆舊的形式，0.x 期間沒有必要。
3. **預期結果用 Chromium 的 `-expected.txt`**（2026-10-05；位置在第 0c、0e 步改過）：每個沒有全部通過的測試檔一份，放在 `4ff8d6e:tests/wpt/baselines/` 裡和測試相同的路徑（`dom/nodes/X.html` 的是 `4ff8d6e:tests/wpt/baselines/` 底下的 `dom/nodes/X-expected.txt`），和 Chromium 的 `platform/*/external/wpt` 一樣是一棵對應測試的樹。原本放在和 `tests/wpt/` 平行的目錄，讓匯入的目錄和上游一模一樣；0c 開始前改成測試檔旁邊（Chromium 的 `web_tests/external/wpt`）：找起來直接，當時一個個下載 WPT 的檔案，不會蓋掉它們；0e 改讀上游的 checkout 之後（第 28 條），checkout 裡不能放我們的檔案，改回對應的樹。不選一份總的 expectations 檔或 wptrunner 的 `.ini`：一份檔裡上千行 subtest 不好審查，`.ini` 是 wptrunner 自己的格式。
4. **0.x 期間的版本號**（2026-10-05）：不相容的改動（改了既有函式的參數或回傳、record 的欄位、拿掉函式或 resource）升 minor，只新增函式、resource 或型別不升。理由：`dom_wasm` 用動態的 host 函式，wasmtime 在連結時幾乎不檢查型別，版本號不變的話，照舊 WIT 編的 guest 要到呼叫時才出錯；升 minor 之後 wasmtime 在載入時就擋下。不選「0.x 期間版本號不變」：舊的 guest 會在執行到一半時壞掉，原因不明顯。
5. **接受這份 RFC 的設計**（2026-10-05）：
   - runner：本機的 HTTP server 以 WPT 的檔案為根提供檔案（0e 起是 `../third_party/wpt`，第 28 條）、照 wptserve 包裝 `.any.js` 和 `.window.js`；kun 自己的 `testharnessreport.js` 把結果交給只在 `RendererFlags::web_tests` 時才有的 hook，hook 不在宣告表裡（第 1 節）；
   - 第 2 到 4 節的 harness 前置、`dom` 的資料模型和宣告表要多支援的形式，第 6 節和規範的比對；
   - 「分階段」的順序。
   - 「待決定」裡的事項在各自的步驟開始前，或第 0 步實測之後決定。
6. **集合是 `dom` 的 GC 物件**（2026-10-05，第 0b 步）：一個 GC 型別（Blink 的 `HTMLCollection` / `LiveNodeList`），一條指向根的 `Edge`、過濾條件、靠 `dom_tree_version` 失效的快取。不選「wrapper 直接拿著節點的 handle」：handle 是 root，會讓集合抓著的節點永遠收不掉，也會和 wrapper 形成 GC 看不到的循環。JS 用 indexed / named interceptor（safe API），WIT 是 `html-collection` resource（`length`、`item`、`named-item`）。
7. **HTML 元素的 interface 用到才加**（2026-10-05，第 0b 步）：0b 只做 `HTMLElement`、`HTMLUnknownElement`、`HTMLMetaElement`、`HTMLTitleElement`，元素照 namespace 和 local name 對應到 interface：`dom` 的 `NodeKind` 照舊只有 `Element`，`dom_api` 的 `ObjectKind` 細分（實作時更正：原本寫成 `ObjectKind` 也不變，但宣告表靠 `ObjectKind` 決定 `this` 和 wrapper 的 interface，不細分就分不出 `HTMLMetaElement`）。不一次建齊 HTML 規範的整張表：每個 interface 在 WIT 裡多一個 resource 和轉型函式，空的 interface 只是雜訊。`HTMLScriptElement.src` 先不做：要 document 的網址，harness 找不到也有退路。
8. **宣告表有 `any`，可以標「只給 JS」**（2026-10-05，第 0b 步）：`any` 在 JS 那邊是中性的 `ScriptValue`（像 `ScriptCallback`，存在 `dom` 的 slot 裡）；有 `any` 的成員標成只給 JS，不進 WIT。`ErrorEvent`（`message`、`filename`、`lineno`、`colno` 兩邊都有，`error` 只給 JS）和 `PromiseRejectionEvent`（`promise`、`reason` 只給 JS）是宣告表的 interface；V8 的 `SetPromiseRejectCallback` 照 HTML 在 microtask checkpoint 之後通知 `unhandledrejection`。不選「`glue.js` 定義這兩個 class」：第 2 步的 `CustomEvent.detail` 也要 `any`，會改兩次。
9. **wasm 的 trap 不分派 `error`**（2026-10-05，第 0b 步）：trap 之後 Store 不能再進入，也沒有能放進 `error` 的值；照舊印出 `[uncaught]`，記進 `docs/limitations.md`。
10. **runner 的 HTTP server 是 `net::test_server`**（2026-10-05，第 0c 步）：多一種「以目錄為根提供檔案」的模式，加上 runner 註冊的路徑（包裝好的 `.any.html`、kun 的 `testharnessreport.js`）。不另寫一個，也不跑 wptserve（要 Python 和它的設定，wptserve 的大部分功能用不到）。
11. **`-expected.txt` 的內容照 Chromium**（2026-10-05，第 0c 步）：「This is a testharness.js-based test.」，接著只有沒過的 subtest（`[FAIL]`、`[TIMEOUT]`、`[NOTRUN]`、`[PRECONDITION_FAILED]` 加上縮排的訊息），最後是 harness 的狀態（「Harness: the test ran to completion.」或錯誤、逾時）；全部通過的測試沒有這個檔。由 kun 的 `testharnessreport.js` 產生文字，runner 只比對字串。不選「列出全部 subtest」：檔案長，diff 裡都是沒變的通過。
12. **不跑的測試是一份 skip 清單**（2026-10-05，第 0c 步）：`4ff8d6e:tests/wpt/testharness-skip.txt`，照原因分組（iframe、testdriver、XML 檔、shadow DOM…），對應 Chromium 的 `TestExpectations` 的 `Skip`。不做完整的 `TestExpectations`：失敗已經由 `-expected.txt` 記錄，kun 沒有平台、旗標的差異，也還不需要 `Slow`。
13. **怎麼跑、怎麼更新**（2026-10-05，第 0c 步）：`cargo test -p shell` 加上 `--test wpt_testharness`（`4ff8d6e` 時的測試），參數是名稱的篩選；`MACHOPO_UPDATE_EXPECTATIONS=1` 重新寫 `-expected.txt`（全部通過時刪掉），和 `MACHOPO_UPDATE_GOLDEN` 一樣。Chromium 用 `run_web_tests.py --reset-results`；kun 的測試在 `cargo test` 裡，傳旗標不方便。2026-10-07 起 `cargo test --workspace` 不跑它（沒設 `MACHOPO_SLOW_TESTS` 時直接結束），改由 `python3 scripts/test.py wpt` 跑，參數同樣是名稱的篩選。
14. **結果經過 `testRunner` 交給 shell**（2026-10-05，第 0c 步）：只在 `RendererFlags::web_tests` 時，realm 的 global 有 `testRunner`（名稱照 Chromium），kun 的 `testharnessreport.js` 用 `add_completion_callback` 組好文字交給它，經過 `FrameClient` 和新的 `RendererHostMsg::TestharnessResults` 回到 shell。不照 Chromium 把文字寫進頁面再 dump：那要能把頁面轉成文字（`dumpAsText`）和 `createElementNS`，kun 都還沒有；直接交出也不受頁面自己的內容影響。
15. **`testharness.js` 是 WPT 的**（2026-10-05，第 0c 步）：WPT 的 `resources/`（0e 起直接從 checkout 讀，第 28 條），和其他測試同一個 WPT commit，`test_server` 從 `/resources/` 提供；`/resources/testharnessreport.js` 換成 kun 的，和 Chromium 一樣。
16. **WPT 跟著 `cargo test --workspace` 跑**（2026-10-05，第 0d 步）：`wpt_testharness` 照舊是 `shell` 的測試 target，完成前要跑的 `cargo test --workspace` 和 CI 都涵蓋它。0d 時約 23 秒，大多是兩個缺 API、等 harness 逾時的測試。每一步匯入之後在 `progress.md` 記下時間；這份 RFC 做完時（第 6 步）再評估一次，太久的話改成只在 `ci.py` 跑（Chromium 的 web test 是另外跑的）。
17. **固定位置的 GC 指標串列是共用的 `EdgeChunks<T>`**（2026-10-05，第 1 步）：固定大小的 chunk、只往後串新的、從不搬動或刪掉中間的；每格是一般的 `Edge`（有 barrier）。static `NodeList`（`querySelectorAll`）和 wasm store 的 slot 表都用它，slot 表那段 `unsafe` 搬進這個型別，只留一份安全論證。不做 HeapVector（Chromium 的 `StaticNodeList` 用它）：concurrent marking 下的擴容要一大塊 `unsafe` 和論證（`P3.md` 的「已決定的事項」第 2 條），而這份 RFC 的範圍裡只有第 4 步的 `Attr` 可能用到，規模小，替代做法就夠；最可能需要它的是將來的 CSSOM（`CSSRuleList` 的 `insertRule` / `deleteRule`），見「之後的可能」。
18. **宣告表有具名的 union 和可變參數**（2026-10-05，第 1 步）：像字典一樣宣告 `union NodeOrString { Node, DomString }`，參數寫 `Variadic<T>`。Rust 是 enum 和 `Vec`，JS 照 WebIDL 的 union 轉換、收集剩下的參數，WIT 是 `variant` 和 `list`，Lua 的產生器多支援這兩種形狀。對應 Chromium 的 IDL compiler 產生的 `V8UnionNodeOrString`。不選「只做寫死的 `NodeOrString`」：之後每個 union 都要再改三個前端。
19. **`[SameObject]` 的集合存在 `NodeRareData`**（2026-10-05，第 1 步）：每個節點多一條 `Edge<NodeRareData>`，第一次用到才配置，裡面放 `children`（第 3 步的 `childNodes` 也是），對應 Blink 的 `NodeRareData` / `NodeListsNodeData`。只快取規範要求 `[SameObject]` 的，`getElementsByTagName` 照舊每次新的。
20. **`id`、`className` 是 `Element` 的 reflection**（2026-10-05，第 1 步）：DOM 定義的，放在 `Element`；手寫兩個小函式。Chromium 用 IDL 的 `[Reflect]` 產生；HTML 元素的 interface 多起來時，宣告表再加 `[Reflect]`。
21. **第 1 步匯入整個 `dom/nodes`**（2026-10-05）：做不到的列進 skip 清單，其他照跑、沒過的記成 `-expected.txt`，和 Chromium 匯入整個 WPT、用 baseline 和 `TestExpectations` 記錄一樣。之後每一步都看得到預期結果變短。skip 清單收（2026-10-05 匯入時細化，先全部跑一次再定）：
    - **testdriver** 的檔案；
    - **iframe 和另一份載入的 document**：只收整個檔案都在等它、沒有任何結果的（跑起來全部是 `Harness: timeout`）。原本打算凡是用到 iframe 的都不跑，但 `Node-appendChild`、`Text-constructor` 這類只有部分 subtest 用 iframe，整個跳過會連同一個檔案裡其他 subtest 的結果都沒有，所以照跑，等 iframe 的那些 subtest 記成失敗；
    - **shadow DOM**（和 custom elements）：和 iframe 一樣，只收沒有它就在任何結果之前出錯的檔案（`Harness: error`）。原本是凡是用到 shadow DOM 的都不跑；2026-10-06 第 2 步匯入 `dom/events` 時改的：`event-global.html` 是測 `window.event` 最主要的檔案，只有幾個 subtest 用 shadow DOM，整個跳過就沒有第 27 條的測試了。改了之後 `dom/nodes` 原本跳過的 25 個也照新規則重看，只剩 4 個；`dom/events` 剩 2 個；`dom/nodes` 和 `dom/events` 的檔案因此多跑 25 個；
    - **XML 檔**（`.xhtml`、`.svg`、`.xml`）runner 本來就不跑，不用列；
    - **基礎設施做不到的其他東西**：等 CSS 動畫的、要 wptserve 的 `.sub` 替換的；
    - **太慢、不適合每次 `cargo test` 都跑的**：例如 `NodeList-static-length-getter-tampered-*`，每個要做上億次 indexed 讀取，一個就要幾十秒到兩分多鐘。這不是能力上做不到，所以要寫清楚原因；之後 indexed 讀取變快、或 WPT 拆出 `cargo test --workspace` 時（第 16 條，RFC 結束時評估）重新考慮。
22. **callback interface 存成物件，呼叫時才判斷**（2026-10-05，第 2 步）：`ScriptCallback` 和 listener 的 slot 存 `v8::Object`（原本是 `v8::Function`），listener 可以是 function 或有 `handleEvent` 的物件。呼叫時可呼叫的就直接呼叫，否則每次呼叫都重新讀 `handleEvent`（規範要求，getter 每次執行）；`removeEventListener` 比對物件本身。只有 event listener 是 callback interface，timer 和 event handler 照舊只收 function。和 Chromium 的 `JSEventListener` 一樣（存 callback object，`InvokeInternal` 判斷 `IsFunction()`，否則 `Get("handleEvent")`）。不選「膠水把物件包成 function」：要用 `WeakMap` 維持身分，`removeEventListener` 才對得回同一個包裝，讀 `handleEvent` 的時機也要包裝自己處理。wasm 不受影響（guest 的 callback 是 id）。
23. **`new EventTarget()` 是 `dom` 的新 GC 型別**（2026-10-05，第 2 步）：只有 listener 串列和 wrapper，對應 Chromium 的 `EventTargetImpl`；`EventTarget` 多一種，事件路徑只有它自己。建構子和 `Event` 的一樣從 `new.target` 取 prototype，JS 的子類別（`class X extends EventTarget`）也是 wrapper；WIT 的 `event-target` 多一個 `new`，wasm 也能用。不選「膠水用 JS 寫一個 EventTarget」：分派的演算法會有兩份，wasm 也用不到。
24. **`createEvent` 只對應 kun 有的 interface**（2026-10-05，第 2 步）：`event`、`events`、`htmlevents`、`svgevents` 是 `Event`，`uievent(s)`、`mouseevent(s)`、`customevent` 各自對應；規範的表裡其他名稱（`keyboardevent`、`touchevent`…）照規範丟 `NotSupportedError`（新的 `DOMException`，WIT 只多一個代碼，不升版）。Chromium 整張表都支援，因為那些 interface 它都有。不替沒有的 interface 加空殼；`Document-createEvent` 裡對應它們的 subtest 記成預期失敗，有了那些 interface 時再對應。
25. **`init*` 只做 `initEvent` 和 `initCustomEvent`**（2026-10-05，第 2 步）：兩個都是 DOM 規範的；`initUIEvent`、`initMouseEvent` 是 UI Events 規範的舊方法，`dom/events` 只有一個測試用到，不做（Chromium 有）。`initCustomEvent` 的 `detail` 是 `any`，照第 8 條只給 JS。
26. **常數在宣告表，WIT 裡寫成註解**（2026-10-05，第 2 步）：宣告表加 `const NAME: u32 = 值;`（實作時改成 `u32`：DOM 的常數都放得進，宣告表不必多一種型別）（`Event` 的 `NONE`、`CAPTURING_PHASE`、`AT_TARGET`、`BUBBLING_PHASE`，第 3 步 `Node` 的常數也用它）。JS 照 WebIDL 裝在 interface object 和 prototype 上（唯讀、不可列舉、不可設定），對應 Chromium 的 IDL `const`。WIT 沒有常數，所以寫成 resource 上的註解（名稱和值），guest 的作者看得到；binding 產生器讀不到，guest 自己寫值（規範的值固定不變）。不選「不進 WIT」：guest 的作者只能去查規範；不選「每個常數一個 `static func`」：多一堆函式，guest 呼叫它也沒比寫死的值好。
27. **`window.event` 在第 2 步一起做**（2026-10-05，第 2 步）：原本不在範圍裡，但它是分派演算法的一部分（呼叫 listener 前設定 global 的「current event」，之後還原），`dom/events` 有 10 個測試用到。`Window` 多一條指向目前事件的 `Edge`；WIT 是 `option<event>`。和 `self` 一樣先不做 `[Replaceable]`（`docs/limitations.md`）；Chromium 是 `[Replaceable]`。設定的是 **listener 自己的 realm 的** window（規範的「callback 的 associated realm 的 global」，Chromium 的 `JSBasedEventListener` 也拿 listener 的 context），不是事件目標的：同一個 DOM 在同語言的導航之間會有好幾個 window。`dom_api` 經過新的 `ScriptHost::callback_window` 問 engine：V8 是 function 的 creation context 的 global；wasm 的 callback 屬於 Store，Store 開始時記下文件的 window（2026-10-06 實作時定的細節）。`window.event` 沒有時是 `undefined`，所以宣告表加了 `OrUndefined<T>`。

28. **WPT 改讀 `../third_party/wpt` 的上游 checkout，不再複製進 repo**（2026-10-05，第 0e 步）：WPT 和 parley 一樣由 `third_party.toml` 固定 commit、`ci.py third-party --clone` 抓。`tests/wpt/` 只放我們的東西：要跑的清單（`testharness-include.txt`、`reftest-include.txt`）、skip 清單、`baselines/` 裡的 `-expected.txt`，以及 `reftest-expectations.txt` 和 `tree-construction-expectations.txt`。原本是照 `tests/wpt/README.md` 一組一組下載檔案：沒有檔案清單，上游刪掉或改名的檔案這邊不會知道；沒有檢查檔案和上游一樣；CSS 的 reftest 挑了哪些只寫在文字裡。一起決定的（比較過 Chromium 的做法）：
    - **直接用上游、不 fork、不改**：Chromium 把 WPT 複製進自己的 repo（`web_tests/external/wpt`，`wpt_import.py` 定期匯入），是為了在 repo 裡改測試、由 `wpt_export.py` 自動送回上游的雙向同步；kun 不對上游送東西，用不到。測試本身有錯時，在本地寫回報上游的草稿，kun 這邊用 skip 或預期結果處理。不選「繼續複製、加檔案清單和匯入腳本」（Chromium 的做法）：沒有雙向同步，就只剩 repo 變大和要維護的腳本；也不選 git submodule：和 `../third_party` 的其他專案不一致。
    - **抓完整的 WPT，不用 sparse checkout**：測試常引用別的目錄的支援檔（`/common/`、`/html/resources/`），sparse 要自己追依賴，少一個就是難懂的失敗；之後接 wptrunner 也要完整的 checkout。shallow clone，`core.autocrlf=false`（`.dat` 裡有 CR，之前 Windows 的 CI 轉成 CRLF 時結果全亂；`ci.py third-party` 會檢查）、`core.longpaths`。2026-10-05 在 Windows 上約 2 分鐘，`.git` 約 145 MB。
    - **跑哪些用 include 清單**：Chromium 匯入的全部都跑，用 `W3CImportExpectations` 和 `TestExpectations` 排除；kun 大部分的測試都還跑不了，所以反過來列出要跑的。testharness 和 reftest 各一份：同一個目錄可能兩種都有，各自的 runner 只該跑自己那種。reftest 的參考頁和它們的 chain 不用列，runner 自己跟著走；只有也要當成測試跑的參考頁才列。清單裡的路徑不存在、列的檔案不是那種測試、`-expected.txt` 沒有測試讀，runner 都失敗，換 commit 時就看得到上游的刪除和改名。
    - **預期結果的位置**：`-expected.txt` 放在 `baselines/` 這棵對應測試的樹（第 3 條，像 Chromium 的 `platform/*/external/wpt`）；兩份 expectations 檔和清單一起放在 `tests/wpt/` 底下（像 Chromium 放在 `web_tests/` 的 `TestExpectations`）。討論時原本打算全部放進一棵叫 `expectations/` 的樹、清單只有一份，實作時改掉：兩份 expectations 檔裡的 id 不是它們在樹裡的路徑（reftest 是 checkout 裡的路徑，`.dat` 是 `file.dat#N`），放進樹裡只是看起來對應；清單的理由見上一項。
    - **統一在 `0f195499`**：`.dat` 原本在 `12093d1b`，兩個 commit 的 `.dat` 一模一樣。
    - 搬遷前比對過，repo 裡複製的 764 個檔案和 checkout 的 blob 逐 byte 相同；搬遷後每個測試的結果都沒變。
    - **wptserve**：照舊用 `net::test_server`（第 10 條）。有了完整的 checkout，之後要 `.sub`、`.headers` 時可以改跑上游的 wptserve，到時候再決定。
29. **`composedPath()` 的路徑存在 `DomIsolate`，不在 `Event` 裡**（2026-10-06，第 2 步）：分派開始時 `dom_api` 把路徑交給 `DomMutationScope::set_event_path`，結束時清掉；`DomReadScope::event_path` 給 `composedPath()`。存成 GC heap 外的 handle（root），一個分派一份，巢狀的分派各有各的。Chromium 把 `EventPath` 放在 `Event` 裡、由 GC trace；kun 那樣做要在 `Event` 裡放一串會變長的 GC 指標（`EdgeChunks` 或新的 trace），動到 GC。而分派的期間 `dom_api` 本來就拿著路徑上每個目標的 handle，存在 heap 外的生命週期完全一樣，不需要 GC 的改動。路徑是分派開始時算的，listener 改了樹也不變（和規範一樣）。
30. **script 建立的事件的 `timeStamp` 是建立當下的時間**（2026-10-06，第 2 步）：建構子和 `createEvent` 的事件，`timeStamp` 取 `performance.now()` 用的那個時鐘（`FrameClient::now`，script 執行期間照樣前進），和 Chromium 一樣（`Event` 建構時取 `base::TimeTicks::Now()`，讀的時候照 `performance` 的時間原點換算、降低精度）。原本一律是 event loop 這一輪的時間（`Timers::now`），script 執行期間不動：WPT 的 `Event-timestamp-safe-resolution` 一直建立事件、等兩個 `timeStamp` 不同，永遠停不下來；`Event-timestamp-high-resolution` 的事件時間早於之前讀的 `performance.now()`，失敗訊息裡的數字每次不同。不選「這兩個列進 skip 清單、記成限制」：和規範、Chromium 都不同，修正只是建立時多設一次時間。沒有 frame 時（`dom_api` 的測試）照舊用 event loop 的時間；瀏覽器送的 trusted 事件也照舊，Chromium 給 input 事件的是平台記下的輸入時間，kun 的 event loop 時間比較接近它。
31. **`childNodes` 沿用 `Collection`**（2026-10-06，第 3 步）：`Collection` 加一種篩選條件「所有子節點」（元素、文字、註解都算），存在節點的 `NodeRareData`（第 19 條），每次同一個。`NodeList` 這個 interface 同時接受 static 的（`querySelectorAll`，`StaticNodeList`）和這種會更新的；JS 看到的都是 `NodeList`，WIT 照舊是一個 `node-list` resource。Chromium 有專門的 `ChildNodeList`，但會更新的清單的機制（`CollectionIndexCache`）和 `HTMLCollection` 共用。不選「另外新增一種 GC 型別」：`Collection` 已經有根的 edge、wrapper、照 DOM tree version 失效的快取，再寫一份只是重複。
32. **`cloneNode` 先不複製文件**（2026-10-06，第 3 步；第 5 步改成可以，第 49 條）：複製 `Document` 時丟 `NotSupportedError`，其他種類照規範（深或淺、`<template>` 的內容一起複製、listener 不複製）。「一個 DOM 裡有多份文件」是第 5 步開始前要整理的待決定事項，複製文件會提前帶進來。Chromium 支援複製文件；第 5 步再補。
33. **`CharacterData` 的位置照 UTF-16 換算，字串照舊是 UTF-8**（2026-10-06，第 3 步）：`substringData`、`deleteData` 等的 offset 和 count 是 UTF-16 code unit，操作時換算；把 surrogate pair 切開時，落單的那一半變成 U+FFFD，和 `P3.md` 第 4 節一致。`CharacterData-surrogates` 裡相關的 subtest 照舊失敗，記成已知限制。Chromium 的字串是 UTF-16，會保留落單的 surrogate。不選「改用 WTF-8 儲存」：整個 DOM 字串的表示法都要改，要另一份 RFC。
34. **第 3 步加上 `ProcessingInstruction`**（2026-10-06，第 3 步）：`dom` 多一種節點（`target` 和 `data`），是 `CharacterData` 的一員；parser 不會產生，style 和 layout 像註解一樣略過。`createProcessingInstruction` 在 HTML 文件也能用（`CDATASection` 不做：照規範，HTML 文件的 `createCDATASection` 丟 `NotSupportedError`，要等 XML 文件）。和 Chromium 一樣支援。不選「延到第 4、5 步」：它就是 `CharacterData` 家族，WPT 有 9 個測試卡在它，`dom/common.js` 的測試節點也用它。
35. **`compareDocumentPosition` 對不同樹的節點照 `NodeId` 排序**（2026-10-06，第 3 步）：規範只要求「實作自訂、但前後一致」；照節點的編號，每次執行都一樣，測試結果可以重現。Chromium 比較指標。
36. **順便加 `Element.tagName`、`normalize()`、`isSameNode()`、`getRootNode()`**（2026-10-06，第 3 步）：不在原本第 3 步的清單裡，但都很小；`tagName` 很多測試在讀（例如 `getElementsByClassName-19` 到 `-30` 都卡在它）。`getRootNode` 沒有 shadow DOM，就是樹的根。Chromium 都有。

37. **`Attr` 是要用時才建立的節點，元素的 rare data 記住它**（2026-10-06，第 4 步）：attribute 照舊存成名稱和值；`attributes`、`getAttributeNode` 等第一次拿到某個 attribute 時才建立 `Attr`（新的節點種類，`nodeType` 2，不能放進樹裡），元素的 `NodeRareData` 記住它，之後拿到的都是同一個（WPT 檢查 `attributes[0] === getAttributeNode(...)`）。還在元素上時，值從元素讀；被移除後 `Attr` 自己保存當時的值（4b 決定，和 Blink 一樣；不選「`Attr` 一直存一份值、每次改都同步」：parser、`setAttribute`、reflection 每條寫入的路徑都要記得同步）。`ownerElement` 是指回元素的 edge。和 Chromium 一樣：Blink 的 `Element` 把 attribute 存在 `ElementData`，`Attr` 由 `EnsureAttr` 建立、放在 `ElementRareData` 的 `AttrNodeList`，拿掉 attribute 時 `DetachAttrNodeAtIndex`。不選「attribute 一律存成 `Attr` 節點」（Servo 的做法）：style、selector、parser 讀 attribute 的地方都要改；不選「`Attr` 是沒有身分的值」：identity 的測試全部失敗。
    - **清單是 linked list**（4b 決定）：每個 `Attr` 有一條 `next` 的 edge，rare data 存第一個，和 listener 的串列（`Listener` 的 `next`）一樣；拿掉時從頭找。Blink 的 `AttrNodeList` 是 `HeapVector`，kun 沒有。不選第 17 條的 `EdgeChunks`：一個 chunk 固定 64 格，只要有一個 `Attr`，元素就多配置一整個 chunk。
    - **元素強引用它的 `Attr`**（4b 決定，和 Blink 一樣）：建立過的 `Attr` 跟著元素活。不選弱引用：script 看不出差別、比較省記憶體，但串列要處理被清掉的弱指標，GC 的面積變大。
38. **`NamedNodeMap` 和 `DOMTokenList` 各是新的 GC 型別**（2026-10-06，第 4 步）：各自有指向元素的 edge 和 wrapper，由元素的 `NodeRareData` 持有，每次同一個；對應 Blink 的 `NamedNodeMap` 和 `DOMTokenList`（都由 `ElementRareData` 持有）。比較過「沿用 `Collection`，加兩種篩選條件」：要新接的地方最少（`Collection` 已經有指向節點的 edge、wrapper、slot 表的 edge、GC 的測試），但它的快取用不到、名字和用途對不上。使用者選擇照 Chromium 的分層新增型別，接受每個型別要接的地方（`crates/dom/README.md` 的「怎麼修改」：handle、wrap 的 tag、wasm slot 表每格多一條 edge、`Object`、測試和 `matrix` / `fuzz`），以後再優化新增型別的成本（「之後的可能」）。第 31 條的 `childNodes` 照舊沿用 `Collection`：它就是會更新的節點清單，和 Blink 的 `ChildNodeList` 共用 `HTMLCollection` 的機制一樣。
    - **`DOMTokenList` 不快取 token**（4c 決定）：每次從 attribute 解析（`class` 通常很短）。Blink 的 `DOMTokenList` 存一份解析好的 `SpaceSplitString`，attribute 改變時更新；量到慢了再照 `Collection` 的快取做。
39. **名稱照 DOM 規範新的規則檢查**（2026-10-06，第 4 步）：`createElement`、`createElementNS`、`setAttribute`、`setAttributeNS`、`createAttribute(NS)` 用規範的 valid element local name、valid attribute local name、valid namespace prefix 和新的「validate and extract」（[whatwg/dom#1079](https://github.com/whatwg/dom/pull/1079)：例如元素名稱可以有 emoji 和 Latin-1 的字元，prefix 在第一個冒號切開）。WPT 的 `name-validation.html` 測的是這套（作者是 Chromium 的人），Chromium 已經改成這樣。`createProcessingInstruction` 的 target 照舊是 XML 的 `Name`（規範沒改）。不選舊的 XML QName 規則：和 WPT、Chromium 都對不上。
40. **第 4 步的範圍**（2026-10-06，第 4 步）：
    - `Element`：`namespaceURI`、`prefix`、`getAttributeNames`、`getAttributeNS`、`setAttributeNS`、`removeAttributeNS`、`hasAttributeNS`、`toggleAttribute`、`getAttributeNode(NS)`、`setAttributeNode(NS)`、`removeAttributeNode`、`attributes`、`getElementsByTagNameNS`；
    - `Document`：`createElementNS`、`createAttribute(NS)`、`getElementsByTagNameNS`；`Node`：`lookupPrefix`、`lookupNamespaceURI`、`isDefaultNamespace`；
    - `Attr`：`name`、`localName`、`namespaceURI`、`prefix`、`value`、`ownerElement`、`specified`；第 3 步的 `cloneNode`、`isEqualNode`、`compareDocumentPosition` 照規範處理 `Attr`；
    - `NamedNodeMap` 的全部成員；`DOMTokenList` 的全部成員（`length`、`item`、`contains`、`add`、`remove`、`toggle`、`replace`、`supports`、`value`、stringifier、iterable），只給 `classList`。
    - 不做：`relList` 這類（kun 沒有用到它們的元素）、`Attr.ownerDocument`（第 5 步）、`createElementNS` 的 `is`（custom element）。
41. **SVG 和 MathML namespace 的元素實作 `Element`**（2026-10-06，第 4 步）：`createElementNS` 在這兩個 namespace 建立的元素，interface 是 `Element`，沒有 `SVGElement`、`MathMLElement`；記成已知限制。WPT 的 `dom/nodes` 和 `dom/lists` 沒有檢查 `instanceof SVGElement` 的測試。Chromium 有完整的 SVG 和 MathML interface，那是另一份工作。
42. **宣告表支援 `[PutForwards=x]`**（2026-10-06，第 4 步）：`attribute classList: DOMTokenList, get = ...` 前面標 `[PutForwards=value]`，JS 的前端替它裝 setter：讀出那個物件，再設它的 `value`（WebIDL 的做法）；WIT 不多函式，guest 讀出物件再呼叫它的 setter 就是同一件事。之後的 `style`（`cssText`）、`location`（`href`）也用它。和 Chromium 一樣（binding 從 IDL 產生）。不選「直接給 `classList` 寫 setter」：JS 看起來一樣，但 WIT 會多一個規範沒有的 `set-class-list`。
43. **宣告表支援 `[Stringifier]`**（2026-10-06，第 4 步）：標在一個 attribute 上（`DOMTokenList` 的 `value`），JS 的前端在 prototype 上裝 `toString`，回傳那個 attribute 的 getter 的結果（WebIDL 的 `stringifier attribute`）；WIT 不變，getter 本來就在。之後的 `URL`、`Location`、`HTMLAnchorElement` 也用它。不選「在 `glue.js` 手寫 `DOMTokenList` 的 `toString`」：每個有 stringifier 的 interface 都要再寫一次。
44. **每個節點有一條指向它的 document 的強 edge**（2026-10-06，第 5 步）：建立時設定，插入另一份 document 的樹時改掉（DOM 的「adopt」：子孫、`Attr` 一起改）；`ownerDocument` 讀它，需要 document 的地方不再從根往上找。和 Chromium 一樣（Blink 的 `Node::tree_scope_` 指向它的 `Document`，有 shadow DOM 時是 `ShadowRoot`）。不選「只有不在 document 樹裡的節點才記（放在 rare data）」：省一個指標，但每次都要走到根，搬移時兩種情況都要處理。
45. **建立節點的函式都帶 document**（2026-10-06，第 5 步）：`dom` 的 `create_element`、`create_text` 等多一個 document 參數；parser 傳它自己的，`dom_api` 傳 `this`（建構子用 realm 的 window 的 document）。和 Chromium 一樣（Blink 的節點建構時都帶 `Document&`）。不選「DOM 記一份預設的 document」：改動少，但把「只有一份 document」的假設留在 API 裡。
46. **做沒有 XML parser 的 `XMLDocument`**（2026-10-06，第 5 步）：`createDocument`、`new Document()`、`createDocumentType`、`createCDATASection`（新的節點種類）；document 記「HTML 或 XML」，XML document 的 `createElement` 不轉小寫、namespace 是 null，attribute 名稱也不轉小寫。不做 XML parser、`DOMParser`。被「另一份 document」擋住的 53 個測試只有 11 個只用 `createHTMLDocument`，`dom/common.js` 也要 `new Document()`。Chromium 全部支援。不選「只做 `createHTMLDocument`」：最核心的 `Node-*` 測試多數還是過不了。
47. **`<template>` 的內容屬於另一份 inert 的 document**（2026-10-06，第 5 步）：每份 document 第一次需要時建它的「template contents owner document」（沒有 window，和那份 document 同樣是 HTML 或 XML），template 的內容屬於它，和規範、Chromium（`Document::EnsureTemplateDocument`）一樣。不選「內容屬於 template 所在的 document」：簡單，但 `template.content.ownerDocument` 和規範不同。
48. **`DOMImplementation` 是新的 GC 型別**（2026-10-06，第 5 步）：照第 38 條的原則，有一條指向 document 的 edge，document 的 rare data 撐住它，`document.implementation` 每次同一個（Blink 的 `Document` 用 `Member<DOMImplementation>` 持有它）。
49. **第 5 步的範圍**（2026-10-06，第 5 步）：
    - `ownerDocument`；`document.implementation`（`createHTMLDocument`、`createDocument`、`createDocumentType`、`hasFeature`）；`new Document()`、`XMLDocument`；
    - `Document` 的 `doctype`、`head`、`contentType`、`compatMode`、`characterSet` / `charset` / `inputEncoding`、`URL` / `documentURI`、`importNode`、`adoptNode`、`createCDATASection`；`cloneNode` 可以複製 document（第 32 條改掉）；
    - 第 3 節的調查裡會出錯的地方：沒有 browsing context 的 document 停用 script、`<a>` 不換頁，移到這種 document 的節點清掉 style。
    - 不做：XML parser、`DOMParser`、iframe。
50. **`innerHTML`、`TreeWalker` / `NodeIterator`、`MutationObserver`、`AbortController` 另開 RFC**（2026-10-06，第 6 步）：這份 RFC 到第 6 步為止。它們都是新的機制，不只是加成員：`MutationObserver` 要每個修改操作通知、microtask 和新的 GC 型別；`AbortController` 的 `AbortSignal` 是新的 `EventTarget`，還要接上 listener 的 `signal`；`TreeWalker` 要呼叫 script 的 filter；`innerHTML` 要片段的 parse 和序列化。Chromium 裡它們也是分開的元件（`core/dom` 的 `mutation_observer`、`abort_signal`、`node_iterator` / `tree_walker`，`core/editing/serializers`）。不選「先把 `MutationObserver` 放進來」：可以解開 17 個測試檔，但它碰到每一個修改操作和 GC，等於再做一個第 5 步；也不選只放進比較小的那兩個：機制一樣是新的，這份 RFC 做完的界線會不清楚。
51. **第 6 節的比對在第 6 步做**（2026-10-06，第 6 步）：第 6 節原本沒排進任何一步。baseline 只看得到測試用到的成員，這份清單看得到全部的差距，下一份 RFC 也能拿來排優先順序。Chromium 的 binding 從 IDL 產生，形狀天生和 IDL 一致；kun 的宣告表是手寫的，更需要比對。「規範有、宣告表沒有」只列 kun 有宣告的 interface（`html.idl` 整份太長）。不選「不做，靠 baseline」：看不到測試沒用到的成員；也不選「只檢查宣告表多出來的」：看不到缺少的。
52. **WPT 由 `test.py wpt` 跑，預期逾時的先開始**（2026-10-06，第 6 步；第 16 條的再評估；2026-10-07 改）：2026-10-07 起 `cargo test --workspace` 跳過它（沒設 `MACHOPO_SLOW_TESTS` 時直接結束），改由 `python3 scripts/test.py wpt` 跑：不在完成一段工作前的清單裡，改了 DOM API、WPT 的清單或 baseline 時才跑；CI 設了變數，每天跑。那時 `cargo test --workspace` 執行約 77 秒，情境約 29 秒、WPT 約 10 秒；兩個移出去，平常的 `cargo test` 快一半。代價是 2026-10-06 不選的那一點：沒跑 `test.py wpt` 的改動，API 的退步要到 CI 才發現。2026-10-06 的評估：`cargo test --workspace` 約 57 秒，其中 WPT 約 21 到 29 秒，幾乎都是 12 個等到 10 秒逾時的測試（其他的都不到 1.3 秒）。runner 讓預期結果是 harness 逾時的測試先開始，而且不佔 worker 的名額，WPT 大約變成 10 秒。每次改動都抓得到 API 的退步。Chromium 的 web test 另外跑，是因為要跑好幾個小時；kun 的規模還不需要。不選「改成只在 `ci.py` 跑」：退步要到 CI 才發現。
53. **`kun:dom` 留在 0.x**（2026-10-06，第 6 步）：第 50 條的 RFC 還會新增和修改成員，`mouse-event-init` 的修飾鍵也還沒有；repo 外面還沒有 guest 依賴它。DOM API 的範圍比較穩定時再決定。
54. **`timeout_multiplier`**（2026-10-06，第 6 步）：環境變數 `MACHOPO_WPT_TIMEOUT_MULTIPLIER`（預設 1）讓 runner 在提供 `testharnessreport.js` 時填入 `setup({timeout_multiplier: N})`，deadline 也乘上 N，不需要在頁面裡執行 script。CI 設成 3，免得慢的 runner 把正常的測試誤判成逾時；本機照舊，預期逾時的測試不會變慢。和 wptrunner 的 `--timeout-multiplier`、Chromium 的 `run_web_tests` 在 debug build 放寬成 3 倍一樣。
55. **`// META: variant=` 移到「之後的可能」**（2026-10-06，第 6 步）：整個 `dom/` 只有一個檔案用到，匯入用到它的目錄時再做。
56. **第 6 節用測試自己的小 parser 讀 WebIDL**（2026-10-06，第 6 步）：只讀需要的部分（interface、mixin、partial、`includes`、dictionary 的成員名稱和種類），extended attribute 照括號整段跳過，所以 WebIDL 多了新的 attribute 寫法也不受影響；不多一個依賴。Chromium 也用自己的 parser（`tools/idl_parser`）。不選 `weedle2`（uniffi 維護的 WebIDL parser）：2026-10-06 試過，它不支援 `[Exposed=*]` 和 `ReflectDefault=1.0`，`dom.idl` 的 `Event`、`EventTarget` 都讀不進來；先把這些寫法換掉再讀可以，但換 WPT 的 commit 時可能又碰到新的寫法，而逐一讀、跳過讀不了的會悄悄少比幾個 interface。
57. **第 6 節比形狀、不比型別**（2026-10-06，第 6 步）：interface 比成員的名稱、種類（attribute、method、const、建構子、iterable）、attribute 是否唯讀、const 的值、method 必要參數的數目和有沒有可變參數（overload 取最少的必要參數，和 JS 的 `length` 一樣）；dictionary 比成員名稱和是否 `required`。型別要先做一張 IDL 型別和宣告表型別的對照表，工作量大概是兩倍，記成「之後的可能」。不選「只比名稱」：看不到「應該唯讀卻可寫」、「參數數目不對」。
58. **刻意和規範不同的地方記在 `spec-differences.txt`**（2026-10-06，第 6 步）：一行一項加上原因，和 `testharness-skip.txt`、`reftest-expectations.txt` 一樣；宣告表保持乾淨，參數數目這類形狀的差異也好寫。原本第 6 節寫「在宣告表註明」：要多一種寫法、`Member` 多一個只給測試用的欄位，所以改掉。
59. **第 6 節讀五份 IDL，缺少的清單只列 kun 有宣告的**（2026-10-06，第 6 步）：`dom.idl`、`html.idl`、`uievents.idl`、`pointerevents.idl`（`MouseEvent` 已經搬到 Pointer Events 規範，實作時發現）、`hr-time.idl`，從 WPT 的 checkout 讀（`third_party.toml` 固定 commit，第 28 條）。缺少的清單包括 kun 有宣告的 interface `includes` 的 mixin（例如 `GlobalEventHandlers` 上百個 `on...`）和散在這幾份檔案裡的 partial；`glue.js` 做的成員不在宣告表裡，照樣列出。不選「也列 kun 完全沒有的 interface」：幾百個，多半是 HTML 的，清單的用處不大。

## 待決定

（無）

## 之後的可能

- **idlharness**：要 `fetch`（同源、只要 `ok` 和 `text()`）、`location.search`，以及 `createDocument`、`createRange` 等；它檢查 interface 的形狀最完整。
- **iframe 和多個 browsing context**：解開 `dom/` 裡約 85 個檔案，以及 `Node-*` 裡用 iframe 拿另一份 document 的測試。
- **DOM API 的下一批**（第 50 條，另開 RFC）：`MutationObserver`、`innerHTML` / `outerHTML`（片段的 parse 和序列化）、`TreeWalker` 和 `NodeIterator`、`AbortController`（和 listener 的 `signal`）、`Range`。剩下的 WPT 失敗各卡在哪裡、各擋住幾個測試檔，見 `progress.md` 的「做過的檢查」；宣告表缺少的成員見 `crates/dom_api/tests/spec-missing.txt`。
- **`html/dom` 的 reflection 測試**：要完整的 HTML 元素 interface 和它們反映的 attribute。
- **testdriver**：kun 的 automation（RFC 0006）已經能送輸入，可以實作 `testdriver-vendor.js`，解開 `dom/events` 的那一半。它需要 runner 和頁面之間的通道（頁面請 runner 送輸入、runner 在頁面裡執行 script），到時候也可以重新考慮由 runner 控制計時（wptrunner 的 `explicit_timeout` 加上外部呼叫 `timeout()`，第 1 節改掉的做法）。
- **宣告表和規範比型別**（第 57 條）：第 6 節的比對再加上參數、回傳和 attribute 的型別，要一張 IDL 型別和宣告表型別的對照表。
- **`// META: variant=`**（第 55 條）：wptserve 替每個 variant 產生帶 query 的網址；runner 現在只跑沒有 query 的那一個。整個 `dom/` 只有一個檔案用到，匯入用到它的目錄時再做。
- **wasm 的一致性測試**：用 Lua 的 runtime 跑一部分移植的 WPT。
- **降低新增 GC 型別的成本**（第 38 條）：kun 的「任何物件」逐型別列出，每加一個給 script 用的型別，事件的目標、`dom_api` 的 `Object`、wasm 的 slot 表（每格每種型別一條 edge）、wrap 的 tag 都要跟著改（`crates/dom/README.md` 的「怎麼修改」）。可能的方向：slot 表每格只放一條不帶型別的 edge（加上型別的 tag，讀的時候檢查），或用 macro 從一份型別清單產生這些分支。型別再變多、或下一個要新增型別的 RFC 開始前評估。RFC 0010 的「已決定的事項」第 8 條評估過，選了 trait 加泛型、slot 每格一條不帶型別的 edge 加上 tag。
- **HeapVector**（Oilpan 的可變長 GC 陣列）：一串 GC 指標頻繁在中間插入、刪除，又要用索引讀時才需要。最典型的是 CSSOM 的 `CSSRuleList`（`insertRule` / `deleteRule`，CSS-in-JS 會大量呼叫）和 `adoptedStyleSheets`；不過 kun 的 CSS 規則在 `style` 裡是 Rust 的 `Arc`，到時候要 GC 的可能只有 wrapper 的身分快取。在那份 RFC 裡決定（第 17 條）。
