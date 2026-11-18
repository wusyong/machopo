# RFC 0103：worker 的執行環境：Window 以外的 global，和 dedicated worker

- 開始日期：2026-11-17
- 狀態：🚧 已排程：2026-11-17
- 進度：`progress.md`
- 細節：`details.md`

## 摘要

machopo 的每個 realm 的 global 都是 Window，`dom_api` 的成員也都假設「現在的 realm 有一個 window」。這份 RFC 先把「執行環境」從 Window 分出來（Blink 的 `ExecutionContext`：timer、task、訊息、`fetch()` 這些不需要文件的狀態），讓宣告表寫明每個 interface 在哪些 global 有，再做 dedicated worker：自己的 thread、自己的 V8 isolate、自己的 DOM heap，global 是 `DedicatedWorkerGlobalScope`。

2026-11-17 決定了 1a 開始前要決定的三件事（「已決定的事項」）；1b 的還是當天的看法，沒有確認。

## 動機

**現況**。沒有任何一種 worker：`new Worker(...)` 是 `ReferenceError`。程式裡「global 是 Window」的假設有三層：

- `dom` 的 `Window`（`crates/dom/src/window.rs`）同時放著文件才有的狀態（`document`、`history`、animation frame、observer）和任何執行環境都要的狀態（timer、listener、排著的訊息、message port、進行中的 `fetch()`、blob 的讀取、`crypto`、`performance`、time origin）。
- `dom_api` 的成員向 front 要「現在的 realm 的 window」（`OpFront::current_window`，`crates/dom_api/src/typed.rs`）來建物件、排 task：28 個檔案、50 處（`details.md` 的「現況的調查」）。其中 `AbortController`、`Request`、`Response`、`URL`、`Event`、`MessageChannel`、`FileReader` 在規範裡 worker 也有。
- `dom_js` 建 realm 時 global 一律用宣告表的 `Window` 的 template，再把表上每個 interface 裝上去（`crates/dom_js/src/window.rs` 的 `create_window_realm`、`crates/dom_js/src/templates.rs` 的 `install`）。宣告表沒有「這個 interface 在哪些 global 有」的寫法。

**風險**。之後要補很多 DOM API，每補一個就多一處這樣的假設；worker 越晚做，要回頭改的成員越多。先把執行環境分出來、表上有地方寫 exposed，之後每個新的 API 一開始就寫對。

**使用量**（chromestatus.com，2026-11-15，Chrome 的 page load 比例）：啟動過 dedicated worker 的 22%，被 service worker 控制的 19%，啟動過 shared worker 的 3%。web-features 沒有 dedicated worker 的計數，所以 `python3 scripts/features.py` 的清單把它列在沒有使用量的那一節。

**目標**：

1. 執行環境和 Window 分開：不需要文件的狀態和它們的操作，Window 和 worker 的 global 共用一份實作。
2. 宣告表寫明 exposed；每種 global 只裝它有的 interface。
3. dedicated worker：`new Worker(url)`（classic script）、兩個方向的 `postMessage`、`onmessage`、`onerror`、`terminate()`、`close()`、`self`、timer、`console`、`importScripts()`。worker 裡忙等不擋住頁面。
4. WPT 的 `.any.js` 測試的 worker variant 跑得起來：現在這些 variant 在 `tests/wpt/metadata/` 裡標成預期失敗，理由是「machopo has no workers」。

**不在範圍內**：

- shared worker、service worker、worklet（「之後的可能」）。
- `OffscreenCanvas`、`SharedArrayBuffer` 和 `Atomics.wait`（要 cross-origin isolation）、`FileReaderSync`。
- wasm 文件的 worker：worker 的 script 是 JS。
- worker 的 DevTools（`Target` 的 worker session）。

**要解決的限制**（`docs/limitations.md`）：

- 「事件」的 `MessageChannel` 和 `MessagePort` 那一條：兩個 port 都在同一個 renderer 的同一條 thread 上。port 傳給 worker 時要跨 thread（第 2 階段）。
- `docs/limitations.md` 沒有一條專門寫「沒有 worker」，只在「Bindings 和 DOM API」的 `Blob` 那一條順帶提到（`FileReaderSync`）。做完時在那一節加一條，寫 worker 有什麼、還沒有什麼。

## 設計概要

網頁的作者看到的：

```js
// 頁面
const worker = new Worker("work.js");
worker.onmessage = (e) => console.log(e.data);
worker.postMessage({ n: 40 });

// work.js
importScripts("fib.js");
self.onmessage = (e) => self.postMessage(fib(e.data.n));
```

worker 的 script 在另一條 thread 上跑：它算很久的時候，頁面照樣捲動、照樣跑自己的 script。worker 裡沒有 `document`、`window`，`self` 是 `DedicatedWorkerGlobalScope`；`setTimeout`、`console`、`URL`、`AbortController`、`Event`、`EventTarget` 這些照樣有。

之後改這部分程式的人看到的：

- 表上的 interface 多一個 exposed 的寫法，沒寫的只在 Window 有。成員的函式拿到的是執行環境，不是 window；要 window 的成員在表上標明，而且只能在只有 Window 有的 interface 上，放錯是編譯錯誤（「已決定的事項」第 2 條）。
- timer、訊息、`fetch()` 的狀態不再是 `Window` 的欄位，是執行環境的；`Window` 和 worker 的 global 各有一個。

## 詳細設計

以下是方向，細節在每個階段開始前決定（「待決定」）。

### 執行環境（`dom`）

`dom` 多一個 GC 型別 `ExecutionContext`（Blink 的同名類別；那邊是 `LocalDOMWindow` 和 worker 的 global 繼承它，這裡是兩者各拿著一個），放不需要文件的狀態；`Window` 拿著一個，worker 的 global（暫名 `WorkerGlobalScope`）也拿著一個。`Window` 現在的欄位怎麼分，見 `details.md` 的「`Window` 的狀態分到哪裡」。task 的挑選（`Cx::next_task`）、timer 的到期、訊息的佇列改成對執行環境做。

### 宣告表的 exposed（`dom_api`）

`declare!` 的 interface 多一個屬性，寫它在哪些 global 有（WebIDL 的 `[Exposed]`：`Window`、`Worker`、`DedicatedWorker`），沒寫的只在 Window 有；個別的成員需要時也可以寫。產生的 `InterfaceDesc` 帶著它。`DedicatedWorkerGlobalScope`、`WorkerGlobalScope`、`Worker`、`WorkerNavigator`、`WorkerLocation` 是表上新的 interface。

成員拿不到 window 由型別保證（「已決定的事項」第 2 條）。2026-11-17 用原型確認了下面的寫法（`details.md` 的「2A 的原型」）：

- 現在成員的函式經過 front 的操作那一側拿 window：`OpFront::current_window`（`crates/dom_api/src/typed.rs`），表上標了 `#[front]` 的成員都拿得到（`crates/dom_api/src/interfaces.rs` 有 470 個），標了 `#[realm]` 的拿到的是 `this` 的 relevant realm 的（3 個，`Blob` 的讀取）。
- 改成：`OpFront` 給成員的只有執行環境（現在的 realm 的，或 `#[realm]` 時 `this` 的 relevant realm 的）。要 window 的成員在表上標 `#[window]`，由 `declare!` 產生的程式拿 window（`Front::op_window`）、當成參數交給它的函式（`Option<Gc<Window>>`，接在 `ApiCx` 後面）。`op_window` 是 `Front` 的 associated function，參數是 `&mut Self::Op`：產生的程式對 front 的型別是泛型的，叫得到它；成員的函式只拿到 `O: OpFront`，寫不出 front 的型別，叫它是編譯錯誤（`E0283`）。
- `#[window]` 寫在 exposed 不只 Window 的 interface 的成員上時，`declare!` 報錯，指到表上那一行。「放錯」因此是表的編譯錯誤，不是執行時的例外。
- 沒有選的寫法是把 `OpFront` 拆成兩個 trait、讓 `declare!` 照 exposed 給不同的型別：要替每個 front 多一個型別或一層轉交（`OpFront` 的實作有四個：`dom_js` 的、`dom_wasm` 的和兩個測試用的）。上面的寫法只動巨集和 `Front`，原型確認擋得住，就不用它。

### global 照表建（`dom_js`）

建 realm 時照 global 的種類選 template、只裝 exposed 包含它的 interface。interface 的 template 還是每個 isolate 一份；worker 的 isolate 只建它用得到的。

### worker 的 thread（`renderer`）

每個 dedicated worker 一條 thread（Chromium 的名字是 `DedicatedWorker thread`），由擁有它的文件所在的 renderer 的 main thread 建。那條 thread 上有自己的 V8 isolate、自己的 `Dom`（elise 的 heap；`Dom` 不是 `Send`，名稱表是 thread-local）和 `dom_js` 的 engine 狀態，跑一個比 main thread 簡單的 event loop：等訊息和 timer，一次跑一個 task、之後 microtask checkpoint。

renderer 這一側的規則（`docs/architecture.md` 的「Process 的邊界」）不變：worker 的 thread 在 renderer 這一側，它和 main thread 之間是 renderer 內部的 channel，不是 `protocol` 的 message；不加整個 process 共用的可變狀態。

`dom_api` 的 `ScriptEngine` 是照 frame 和文件設計的（建 window、當 parser 的 host、收輸入、拿 layout）。worker 用另一個小的介面：建 global、跑 classic script、跑到期的 task、收訊息、結束。

### 訊息

`postMessage` 的訊息已經是可以跨 thread 的資料：`dom` 的 `SerializedMessage` 是 engine 序列化的 bytes 加上 `Box<dyn Any + Send>`（`crates/dom/src/message.rs`；V8 的 `ValueSerializer`，`crates/dom_js/src/serialize.rs`）。`Worker` 和 worker 的 global 之間是一對隱含的 port：一邊序列化、經過 channel、另一邊在自己的 realm 反序列化，發 `MessageEvent`。

`MessagePort` 傳進 worker（transfer）要讓 port 的兩端在不同的 thread 上，現在的 port 直接把訊息排進對方的佇列。這是第 2 階段。

### 載入 script

worker 的主 script 和 `importScripts()` 的 script 經過 Browser 的 `net` 載入，和頁面的子資源一樣。`importScripts()` 是同步的：worker 的 thread 要能自己等回應，不能靠 main thread 轉交（main thread 忙的時候 worker 會卡住）。現在子資源的回應（`RendererMsg::ResourceResponse`）只送到 renderer 的 main thread 的 scheduler；worker 怎麼拿到自己的請求通道是「待決定」第 5 條。

### 結束

`worker.terminate()`、worker 自己的 `close()`、擁有它的文件被丟棄時 worker 結束（「已決定的事項」第 12 條）。`terminate()` 馬上打斷正在跑的 script（V8 的 `IsolateHandle::terminate_execution` 可以從別的 thread 呼叫）；之後那條 thread 照順序放掉 engine、`Dom` 和 isolate，再結束。原型確認了打斷之後不會 crash，但 `dom_js` 回頭呼叫 script 的地方（listener、timer、observer）會把打斷當成一般的例外接住、回報掉，script 繼續跑（`details.md` 的「1b 的原型：打斷 script」）：那些地方要讓打斷繼續往外傳，呼叫的那一側不再呼叫下一個 script。

### 測試

- **重構的階段（1a）**：行為不變，現有的 `cargo test -p dom`、`-p dom_api`、`-p dom_js` 和相關的 WPT 目錄（timer、訊息、`fetch`、abort）是回歸檢查；exposed 的表另外寫 `dom_api` 的測試（每個 interface 的 exposed 和規範一致的抽查、Window 才有的 interface 不裝在 worker 的 global 上）。
- **worker（1b 起）**：WPT 的 `workers/` 裡 dedicated worker 的目錄，和 `.any.js` 測試的 worker variant：加進 `tests/wpt/include.txt`、拿掉對應的預期失敗。WPT 沒有涵蓋的（thread 的結束順序、`terminate()` 打斷忙等的 script、文件丟棄時 worker 跟著結束）寫 `dom_js` 和 `renderer` 的整合測試。
- **情境**：一份「worker 忙等的時候頁面照樣捲動、按鈕照樣有反應」的情境（`tests/scenarios/`）。
- **mutation test**：每個階段照 `docs/testing.md` 的「Mutation test」跑。
- **只能靠審查的**：isolate、heap 和 thread 的放掉順序裡 `unsafe` 的論證（`crates/bindings/soundness.md`）；記進 `docs/test-gaps.md`。

### 分階段

| 階段 | 做什麼 | 做完時看得到什麼 |
| --- | --- | --- |
| 1a | 執行環境從 `Window` 分出來；宣告表的 exposed；global 照表建。都在 main thread 上 | 行為不變；表上每個 interface 有 exposed |
| 1b | dedicated worker：thread、isolate、`Dom`、`DedicatedWorkerGlobalScope`，classic script、`postMessage`、timer、`console`、`importScripts()`、`terminate()`、`close()`、`onerror` | 上面的例子跑得起來；WPT 的 worker variant 開始跑 |
| 2 | worker 裡的 `fetch()`、`MessagePort` 跨 thread、module worker（`type: "module"`）、巢狀的 worker、`WorkerNavigator`、`WorkerLocation`、`performance` | 常見的函式庫的 worker 用法 |

第 2 階段分成六段（2026-11-18 決定）：2a worker 裡的 `location`、`navigator`、`performance`；2b worker 裡的 `fetch()`，和 worker 裡建的 blob URL 讓 Browser 知道；2c `MessagePort` 跨 thread；2d worker 裡建的 worker；2e module worker；2f WPT、文件、整個階段的檢查。先做最便宜、擋住最多 WPT 的（很多 worker 的測試一開始就讀 `location`），風險最高的 port 自己一段，module graph 的量最大、排在最後。

1a 自己就有價值：它定下之後每個 API 怎麼寫。它做完先 merge（「已決定的事項」第 3 條）。

## 缺點

- 每個 worker 一個 V8 isolate 和一個 heap：記憶體和啟動時間都比「同一個 isolate 裡多一個 realm」多。Chromium 也是這個成本。
- `dom` 多一層：讀 timer、訊息的地方從 `Window` 多走一步到執行環境。
- 1a 是動到很多檔案的重構，期間和其他正在做的 DOM API 的工作容易衝突。
- thread 之間的結束順序是新的一類錯誤（worker 還在跑時文件被丟棄、`terminate()` 和訊息同時到）。

## 理由和其他做法

**執行環境怎麼表示**：

| 做法 | 優點 | 缺點 |
| --- | --- | --- |
| A：分出共用的部分（`ExecutionContext`），`Window` 和 worker 的 global 各拿一個（Blink 的 `ExecutionContext`，那邊是兩者繼承它） | 一份實作；成員的函式從型別就看得出要不要文件 | 重構的範圍最大 |
| B：worker 的 global 是沒有文件的 `Window` | 改得最少 | `Window` 的每個成員都要想「沒有文件怎麼辦」；型別擋不住在 worker 裡碰 `document` |
| C：另寫一個 `WorkerGlobalScope`，timer、訊息各自一份 | 不動現有的 `Window` | 兩份實作會走偏；`current_window` 的每一處都要分兩種情況 |

2026-11-17 決定：A（「已決定的事項」第 1 條）。

**worker 也有的 interface 的成員碰了 window，怎麼發現**：

| 做法 | 怎麼做 | 優點 | 缺點 |
| --- | --- | --- | --- |
| 2A：型別保證 | front 給成員的介面分成兩層：都有的（拿執行環境）和只有 Window 的 interface 的成員才有的（拿 window）；`declare!` 照 exposed 決定每個成員的函式拿到哪一層 | 放錯是編譯錯誤，不用靠測試 | 要改 `declare!` 和 `OpFront` |
| 2B：執行時檢查（Blink） | `current_window` 照舊回傳「可能沒有」，worker 裡是沒有；每一處自己處理 | 不用新的機制 | 放錯要到 worker 裡跑到那一行才知道 |
| 2C：2B 加上一個掃過宣告表的測試 | 對每個 worker 也有的 interface 的每個成員，在 worker 的 realm 裡呼叫一次，不能丟「沒有 window」 | 不改型別 | 測試要替每個成員造參數；只測得到造得出來的呼叫 |

2026-11-17 決定：2A，2C 是退路（「已決定的事項」第 2 條）。

**worker 在哪裡跑**：

| 做法 | 優點 | 缺點 |
| --- | --- | --- |
| A：自己的 thread、isolate 和 heap（Chromium） | 真的平行；worker 當掉或卡住不影響頁面的 script | 記憶體、啟動時間 |
| B：和頁面同一條 thread、另一個 realm | 最省、最簡單 | worker 忙的時候頁面卡住，違反網頁對 worker 的期待 |

2026-11-17 的看法：A。

不做會怎樣：用到 worker 的網站在 `new Worker` 就丟例外；之後補的每個 API 都繼續假設 global 是 Window，worker 越晚做越貴。

## 先例

Chromium（Blink 和 content），2026-11-17 對照 `main` 的 `0889dbff0b08`，檔案和原文的出處在 `details.md` 的「Chromium 的對應」：

- **執行環境**：`ExecutionContext` 是「script 可以執行的環境」，放 window 和 worker 共同的東西（解析相對網址的 base URL、security context、暫停和結束、console）。`LocalDOMWindow` 和 `WorkerOrWorkletGlobalScope` 都**繼承**它；API 的函式收 `ExecutionContext&`（`DOMTimer::setTimeout(ScriptState*, ExecutionContext&, ...)`）。timer 的表不是它的欄位，是掛在它上面的 `Supplement<ExecutionContext>`（`DOMTimerCoordinator`）。**照做**；Rust 和 elise 的 GC 型別沒有繼承，「繼承」換成「各拿著一個共用的部分」。
- **exposed**：IDL 的 `[Exposed]` 寫 global 的名字：`Window` 是 `Exposed=Window`，`WorkerGlobalScope` 是 `Exposed=Worker`，`DedicatedWorkerGlobalScope` 是 `Exposed=DedicatedWorker`，`AbortController` 是 `Exposed=(Window,Worker,ShadowRealm)`；`setTimeout` 這些在 mixin `WindowOrWorkerGlobalScope`（`Exposed=(Window,Worker)`）上，C++ 也有同名的類別讓兩種 global 繼承。**照做**，寫在宣告表上。
- **thread**：dedicated worker 是「in-process worker」，一定和啟動它的文件在同一個 renderer process。`WorkerThread` 在 `WorkerBackingThread` 上跑，後者是「帶著 Oilpan 和 V8 的 thread」：isolate 用那條 thread 自己的 `CppHeap` 建。**照做**（elise 的 heap）。
- **兩條 thread 之間**：文件那一側是 `DedicatedWorkerMessagingProxy`，worker 那一側是 `DedicatedWorkerObjectProxy`，互相 `PostCrossThreadTask`；worker 還沒準備好時送來的訊息先排著（`queued_early_tasks_`）。**照做**。
- **Browser 知道每個 dedicated worker**：`DedicatedWorker::Start` 向 Browser 要一個 `DedicatedWorkerHost`（`CreateWorkerHost`，帶著 script 的網址和 credentials mode），Browser 開始載入主 script（`WorkerScriptFetcher`），再回到 renderer 的 `OnScriptLoadStarted` 繼續；host 也替 worker 建子資源用的 network factory、記著它的 storage key 和擁有它的文件。照不照做是「待決定」第 4 條。
- **worker 自己載入子資源**：「All worker subresources … are fetched off-the-main-thread」。`WebWorkerFetchContext` 在 main thread 建、交給 worker、在 worker 的 thread 上初始化，用來在那條 thread 上建 `URLLoader`；`importScripts()` 是 `WorkerClassicScriptLoader::LoadSynchronously`。**照做**（「待決定」第 5 條的看法）。
- **只有 window 能用的地方**：執行時轉型，`DynamicTo<LocalDOMWindow>(context)` 在不是 window 時得到 null，型別不擋。照不照做是「待決定」第 2 條。
- **結束**：`WorkerThread::Terminate` 排一個結束的 task，排著的其他 task 不再跑；正在跑的 script 過了 2 秒（`kForcibleTerminationDelay`）還沒停，就 `Isolate::TerminateExecution()`。**照做**；等多久是「待決定」第 8 條的一部分。

Firefox 和 Servo 也是每個 worker 一條 thread、自己的 JS runtime（這一句是起草時記得的，沒有對照原始碼）。

## 已決定的事項

2026-11-17：

1. **執行環境從 `Window` 分出來，兩種 global 各拿著一個共用的部分**（「理由和其他做法」的 A）。`dom` 多一個 GC 型別，放不需要文件的狀態和它們的操作；`Window` 和 worker 的 global 各拿著一個。不選 B（worker 的 global 是沒有文件的 `Window`）：`Window` 的每個成員都要處理沒有文件的情形，型別擋不住在 worker 裡碰文件。不選 C（另寫一個 worker 的 global，timer 和訊息各自一份）：兩份實作要一起維護，`current_window` 的每一處都要分兩種情況。理由：這是唯一讓「一個 API 在 worker 能不能用」由型別決定的做法，也和 Blink 的分法一樣（`ExecutionContext`，那邊是兩者繼承它）；代價是 1a 的重構最大。
2. **worker 也有的 interface 的成員拿不到 window，由型別保證**（「待決定」原本的第 2 條的 2A）。exposed 寫在宣告表的 interface 上，個別的成員需要時也可以寫；front 給成員的介面分成兩層，都有的拿執行環境，只有 Window 的 interface 的成員拿得到 window，`declare!` 照 exposed 決定每個成員拿到哪一層，放錯是編譯錯誤。不選 2B（只在執行時檢查，Blink 的 `DynamicTo<LocalDOMWindow>`）：放錯要在 worker 裡跑到那一行才知道。2C（2B 加上掃過宣告表的測試）是退路：1a 的原型做不出合理的寫法時用它，那時回來改這一條。
3. **1a 做完先自己 merge 回 `main`**，不等 1b（`rfcs/README.md` 的「流程」的階段的 merge）。1a 是重構、行為不變；merge 之後新的 API 馬上照新的規則寫，和其他 DOM 的工作衝突的時間最短。不選「等 1b 一起」：branch 活得久，期間每個新加的 API 都要再改一次。代價是 `main` 上有一段時間執行環境分出來了、還沒有第二種 global 在用。
4. **執行環境的型別叫 `ExecutionContext`**（2026-11-17），Blink 的名字（`CLAUDE.md`：型別和模組盡量對應 Chromium 的名字）。不叫草稿裡的暫名 `GlobalScope`：Blink 的 `WorkerGlobalScope` 是 JS 看得到的 global 物件，名字相近容易混。和 Blink 不同的是那邊 `LocalDOMWindow` 和 `WorkerOrWorkletGlobalScope` 繼承它，這裡兩者各拿著一個，註解寫明。
5. **listener 和 wrapper 留在各自的 event target，不放進執行環境**（2026-11-17）。`Window` 照舊拿著它的 listener 和 JS 的 global 物件（wrapper）；1b 的 worker 的 global 是另一種 event target，自己拿著。和 Blink 一樣：listener 在 `EventTarget` 的 `EventTargetData` 裡，wrapper 是 `EventTarget` 的（它是 `ScriptWrappable`），`ExecutionContext` 兩者都沒有；machopo 的節點、`Window`、`EventTargetImpl` 已經各自拿著自己的 listener。不選放進執行環境：listener 是 `window` 這個物件的，放到環境上讓 global 變成 target 裡的特例；執行環境不是 JS 看得到的物件，沒有 wrapper 可放（Blink 的網頁和擴充功能的 script 共用一個 `ExecutionContext`、各有自己的 global）。非節點的 target 共用一個放 listener 的型別（Blink 的 `EventTargetData`）留到 1b 加 worker 的 global 時再看。
6. **執行環境指回它的 global，由 `dom` 回答從 global 來的值**（2026-11-17）。base URL、origin 這類從 window 的文件算出來的值，由 `dom` 照執行環境的 global 回答（`context_base_url`、`context_origin`；1b 加上 worker 的 global）。指回 global 的欄位只在 `dom` 裡面用，沒有「給一個執行環境、拿回 window 或它的文件」的公開函式；算 origin 的邏輯（`about:` 的文件照它的 iframe 的文件）跟著從 `dom_api` 搬進 `dom`。和 Blink 一樣：`ExecutionContext` 的 `BaseURL()`、`GetSecurityOrigin()` 是 virtual，`LocalDOMWindow` 從它的文件回答。不選「執行環境自己存一份 URL 和 origin」：base URL 隨 `<base>` 改，每個改的地方都要記得同步。
7. **從物件找 window 的 `dom` 函式是執行時檢查**（2026-11-17）。`dom` 本來就有從物件找到 window 的公開函式（`target_window`、`window_of(document_of(node))`），engine 建 wrapper 時也靠它們找 realm（`dom_api::realm_document`）；物件改記執行環境之後，`target_window` 在 `dom` 裡面經過執行環境找回 window，在 worker 的物件上回傳沒有。所以第 2 條由型別擋住的是「成員向 front 要現在的 realm 的 window」這條路，從物件找 window 是執行時檢查，和 Blink 的 `DynamicTo<LocalDOMWindow>(context)` 一樣。不選「改得更嚴」（engine 改用執行環境找 realm、這類函式拆成 window 才有和 worker 也有的兩組）：從節點出發的路（`window_of(document_of(node))`）照樣擋不住，要完全擋住得把 `dom` 的 API 照 realm 的種類拆開（「之後的可能」）。而且 worker 也有的成員在 window 的 realm 裡本來就該碰到 window（`dispatchEvent` 的路徑經過 window），兩邊都跑的程式只能在執行時分辨。
8. **dedicated worker 向 Browser 登記**（2026-11-17，「待決定」原本的第 4 條）。`new Worker()` 時 renderer 告訴 Browser；Browser 記下它屬於哪個 renderer、哪個文件，載入它的主 script，worker 結束時清掉。1b 的 Browser 那一側只做這三件事。和 Chromium 一樣（`DedicatedWorker::Start` 的 `CreateWorkerHost`、Browser 的 `DedicatedWorkerHost` 和 `WorkerScriptFetcher`）。不選「renderer 自己建、Browser 不知道」：shared worker 要 Browser 找到它、service worker 要知道每個 client（dedicated worker 也是），儲存的 key 也掛在這裡，到時候 dedicated worker 的啟動流程得重寫。
9. **worker 的子資源請求走 renderer 現有的通道，訊息帶它是哪個 client 的**（2026-11-17，「待決定」原本的第 5 條）。worker 的 thread 自己送 `FetchResource`，訊息多帶 client（main thread 或第幾個 worker）；回應也帶著它，renderer 收訊息的那一端照它排到那個 worker 的佇列，不經過 main thread（`importScripts()` 是同步的，頁面的 script 忙時 worker 照樣等得到回應）。Browser 照第 8 條的登記檢查 client 是不是這個 renderer 的 worker。Chromium 的效果一樣、做法不同：每個 worker 一條 `URLLoaderFactory` 的通道，Browser 登記時建好交給 renderer（`DedicatedWorkerHost` 替 worker 建 network factory，`WebWorkerFetchContext` 在 worker 的 thread 上用它）。不選「每個 worker 一條通道」：`protocol` 的訊息裡不能放通道（`docs/architecture.md` 的「Process 的邊界」規則 3、5），`ipc` 也還不能傳通道；拆成 process 之後收訊息的是 renderer 的 IO thread，照 client 分到各 thread 正好是它的工作，之後能傳通道時只換這一層。不選「經過 main thread 轉交」：頁面忙時 worker 卡住。
10. **worker 的 engine 是 `dom_api` 裡另一個小的 trait**（2026-11-17，「待決定」原本的第 6 條；暫名 `WorkerEngine`）：建 worker 的 global、跑 classic script、跑到期的 task、收訊息、結束時打斷正在跑的 script；啟動 renderer 時和 `ScriptEngineFactory` 一樣多給一個建它的函式。`dom_js` 多一個實作它的型別，和 `V8Engine` 共用 isolate 和 template 的程式；`dom_wasm` 不實作（worker 的 script 是 JS）。和 Blink 一樣是兩個類別：frame 的 `ScriptController`（`bindings/core/v8/script_controller.h`）和 worker 的 `WorkerOrWorkletScriptController`（同一個目錄，由 worker 的 global 拿著）。不選擴充 `ScriptEngine`：它是照 frame 和文件設計的（建 window、parser 的 host、輸入、layout），worker 的那份大部分方法用不到。
11. **worker 的 thread 和 event loop 是 `renderer` 的新模組**（2026-11-17，「待決定」原本的第 7 條；暫名 `worker_thread.rs`），和 main thread 的 scheduler（`task_queue.rs`）放在一起：共用佇列的型別、`RendererSender` 照 client 的分流（第 9 條）和啟動 renderer 時給的 engine 工廠（第 10 條）；`renderer` 已經在 `sanity.py boundary` 的 renderer 這一側。和 Chromium 一樣和 frame 的程式在同一個元件（Blink 的 `core/workers/`）。不選新的 crate：那些型別要搬到兩邊都用得到的地方，邊界的檢查也要多列一個；之後 `renderer` 太大時再拆。
12. **worker 的結束**（2026-11-17，「待決定」原本的第 8 條）。由擁有它的那一側（頁面的 thread）發起，worker 的 thread 自己收尾：它的 engine（V8 的 context 和 persistent）、DOM 的 heap、isolate 照這個順序在它自己的 thread 上放掉（`Dom` 不是 `Send`），論證寫進 `crates/bindings/soundness.md`。`terminate()` 時**馬上**打斷正在跑的 script（V8 的 `TerminateExecution()`，從頁面的 thread 呼叫），同時排收尾的 task；之後排著的 task 不跑。文件被丟掉時走同一條路；renderer 結束時 main thread 等每個 worker 的 thread 結束才離開；結束之後才到的回應和訊息丟掉，Browser 照第 8 條的登記清掉它。和規範一樣（terminate a worker 要「Abort the script currently running in the worker」）。不選 Chromium 的「先排收尾的 task，2 秒後還在跑才打斷」：那是它的實作選擇，為了少走打斷之後 V8 呼叫回傳空值的路（`details.md` 的「`terminate()` 之後多久打斷正在跑的 script」）；這條路兩種做法都得處理好，馬上打斷讓測試每次都走到它。WPT 的 `workers/` 只測頁面看得到的效果，兩種做法都會通過。1b 的第一步用原型確認打斷之後從 bindings 一路退出來不會 crash。
13. **被打斷的 callback 回到還有 script 在下面的地方時，再要求打斷一次**（2026-11-17，1b-1 的 mutation test 找到的漏洞之後選的，選項 C）。V8 會把正在進行的打斷清掉：會跑 script 的 API（呼叫函式、建 wrapper、reject promise、`JSON.parse`）一進來就先清掉目前的例外，打斷也是一種例外；debug 的 V8 先清、後 DCHECK，也抓不到。所以 listener 被打斷之後，同一個成員裡接著用了這種 API（例如 `fetch` 在 abort 時 reject），外層的 script 就繼續跑。`dom_js` 接住打斷的地方（回報例外的 `report_caught`、node filter、解析 JSON）在 stack 上還有 script 時，在自己的 thread 上再呼叫一次 `TerminateExecution()`：之後不管哪個 API 清掉，script 在下一個檢查點（函式的入口、迴圈的回跳）就停。底下沒有 script（task 直接呼叫的 callback）時不再要求：V8 在那裡自己結束打斷，下一個 task 照常跑（window 被 DevTools 的 `Runtime.terminateExecution` 打斷之後；worker 的 loop 不再跑 task）。另外照 Blink 的 `V8ThrowDOMException`，被打斷時連 `DOMException` 都不建（建它會清掉打斷），丟例外前也檢查（`bindings::throw`）。從 Rust 的 task 直接呼叫 callback 的入口的檢查拿掉：打斷在那裡已經被 V8 結束，從別的 thread 要求的打斷要等 script 在跑才生效，那些檢查走不到。Blink 只在 listener 的入口、建和丟例外前、回報錯誤前檢查，不再要求打斷：它先讓 worker 在兩個 task 之間收尾，2 秒後還在跑才打斷，成員做到一半被打斷的情形少；我們馬上打斷（第 12 條），那是常態。不選只逐處檢查（A，Blink 的做法）：每個「回頭呼叫 script 之後、同一個成員裡又用 V8 的 API」的地方都要一道，多半沒有測試走得到，`fetch` 這條就漏了。不選只再要求打斷、不留任何檢查（B）：打斷之後 embedder 不該再進 V8（V8 的 `--strict-termination-checks` 的約定），回頭呼叫 script 的入口照樣先檢查。平常的路上沒有成本：再要求打斷只在被打斷時走；檢查是讀一次 isolate 的狀態，只在呼叫 script 和丟例外的旁邊，一般的成員呼叫不經過。
14. **worker 的 global 在 `dom` 裡是自己的 GC 型別**（2026-11-17，1b-2 開始前；暫名 `WorkerGlobalScope`）：`TargetRef` 多一種，和 `Window` 並列；執行環境指回的 global 是 window 或它。和 Blink 一樣（`core/workers/worker_global_scope.h` 的 `WorkerGlobalScope` 經過 `WorkerOrWorkletGlobalScope` 繼承 `EventTarget` 和 `ExecutionContext`，和 `LocalDOMWindow` 並列）。對 `TargetRef` 分情況的地方（`TargetRef::Window` 在 `dom`、`dom_api`、`dom_js` 的 13 個檔案裡出現 34 次）各自決定 worker 的 global 怎麼做；要 window 的函式由型別擋住 worker 的 global。不選「`EventTargetImpl` 的一種」（像 `MessagePort`、`AbortSignal` 那樣多一個資料的欄位）：每個 `EventTargetImpl` 多 8 bytes，「這是 global」只能在執行時判斷（`dom_js` 的 global 的 wrapper 是 realm 的 global 物件，不是一般的 wrapper），同樣的問題藏到執行時的判斷裡。
15. **`dom` 的 timer、task、訊息的公開函式改成收執行環境**（2026-11-17，1b-2 開始前）：`add_timer`、`remove_timer`、`next_task`、`take_posted_message` 這類原本收 window 的函式改收 `ExecutionContext`，window 那一側的呼叫先拿 window 的執行環境；只有 window 才有的（`requestIdleCallback` 的 timeout）在 `next_task` 裡照執行環境的 global 判斷。和 Blink 一樣（`DOMTimer` 的 `setTimeout`、`clearTimeout`、`RemoveByID` 收 `ExecutionContext&`，timer 的清單 `DOMTimerCoordinator` 和 task runner 從執行環境拿）。不選「另外給 worker 一套」：每個函式兩份，之後改 timer 或 task 的規則要記得兩邊一起改。
16. **頁面那一側的 `Worker` 物件是 `EventTargetImpl` 的一種**（2026-11-17，1b-3 開始前）：`EventTargetImpl` 多一個資料的欄位（它對應的 worker 的編號、是不是結束了），像 `MessagePort`、`AbortSignal`、`FileReader`；`interface_of` 照它對應到 `Worker`。代價是每個 `EventTargetImpl` 多 8 bytes。不選自己的型別（第 14 條那樣多一種 `TargetRef`）：`Worker` 物件是一般的 event target，不是 global，34 處分情況的地方不用多一種；Blink 的 `DedicatedWorker` 是自己的類別，但 Blink 每種 event target 都是，machopo 對非 global 的 target 一直用 `EventTargetImpl`。
17. **`dom_api` 請 host 啟動 worker、送訊息、`terminate()` 時直接呼叫**（2026-11-17，1b-3 開始前）：`dom_api` 多一個小的 trait（worker 的 host），renderer 建 engine 時給它（和第 10 條的 engine 工廠一起）；成員經過它當場建 worker、把訊息送進 worker 的 channel、打斷 worker；`new Worker()` 的同一個 task 裡送的訊息，在 worker 準備好之前先排著（HTML 的 outside port 的佇列）。trait 的細節和訊息排在哪裡在 1b-5 實作 renderer 那一側時定。和 Chromium 一樣（`DedicatedWorker::postMessage` 當場把 task 跨 thread 送給 worker；`terminate()` 當場呼叫 `WorkerThread::Terminate`；`new Worker()` 當場向 Browser 建 `DedicatedWorkerHost`）。不選照 `fetch()` 排進要求的佇列、task 結束後 renderer 拿走：`terminate()` 要等頁面的 task 跑完才打斷（第 12 條的「馬上」變成「等頁面的 task 跑完」），頁面在同一個 task 裡等 worker 的回應時會一直等不到。
18. **`dom_api` 跑 task 的函式收 realm 的 global 的 enum**（2026-11-17，1b-4 開始前）：`dom` 多一個 `GlobalRef`、`GlobalRoot`（window 或 worker 的 global），跑 timer、送 posted message、回報例外的函式（`timer::run_due`、`run_task`、`run_posted_messages`、`report_exception` 等）和 `ScriptHost` 的 `call_timer`、`deserialize_message` 改收它；只有 window 才有的（idle callback 的 timeout、`hashchange`）用 `match` 分。和 Blink 的效果一樣（`DOMTimer` 收 `ExecutionContext`，要 window 時 `DynamicTo<LocalDOMWindow>`），寫法不同：第 6 條不給「執行環境 → window」的公開函式。不選收執行環境、開放 `context_window`：任何拿得到執行環境的程式（worker 也有的 interface 的成員）都能拿回 window，第 6 條擋的路就開了。
19. **dedicated worker 跑到被結束為止**（2026-11-17，1b-5 開始前）：只在 `terminate()`、worker 自己 `close()`、擁有它的文件被丟掉、renderer 結束時結束；`Worker` 物件沒有人拿著也不收掉 worker。和 Chrome 一樣（Chromium `main` 的 `a5229e6c`：`DedicatedWorker` 是 `ActiveScriptWrappable`，`HasPendingActivity()` 問 `DedicatedWorkerMessagingProxy::HasPendingActivity()`，只回傳 `!AskedToTerminate()`；文件不在時 `ContextDestroyed()` 呼叫 `terminate()`；`ThreadedObjectProxyBase::ReportPendingActivity` 只剩宣告，沒有定義也沒有呼叫）。不選 HTML 允許的「不是 protected worker（沒有 timer、網路連線、port）、`Worker` 物件也沒人拿著時收掉」：worker 那一側要隨時回報還有沒有在忙，頁面那一側的 `Worker` 物件要在 GC 時通知 host，Chrome 也不這樣做（「之後的可能」）。
20. **worker 的 script 的 MIME type 照 Chrome 寬鬆地檢查**（2026-11-17，1b-6 開始前）：主 script 和 `importScripts()` 的回應只擋 `image/*`、`audio/*`、`video/*`、`text/csv`（Fetch 的「因為 MIME type 擋下回應」），其他的照樣執行；狀態不是 2xx 的是網路錯誤。和 Chrome 預設一樣（Chromium `main` 的 `a5229e6c`：`WorkerClassicScriptLoader` 用 `AllowedByNosniff::MimeTypeAsScript` 的 `kLaxForWorker`，嚴格要開 `StrictMimeTypesForWorkers`，那是 `experimental`、預設關著）。不選照 HTML 嚴格（http(s) 的回應不是 JavaScript 的 MIME type 就是網路錯誤）：伺服器把 script 設成 `text/plain` 之類的網站的 worker 會失敗，Chrome 也還沒這樣做；和規範不同的記在 `docs/limitations.md`。
21. **沒有 window 的執行環境的 task 和 UA 的 timer 排進它自己的執行環境**（2026-11-17，1b-7 開始前）：`FileReader` 的讀、`Blob` 的讀（`text()` 等）、`AbortSignal.timeout()` 這類不屬於元素的 task 和 timer，在 worker 裡不再找 window 的文件，不帶元素，排進 worker 的執行環境，由 worker 的 engine 跑 task 的那條路跑（和 window 的同一條）。和 Blink 一樣（`ExecutionContext::GetTaskRunner`：worker 的 task 在它自己的 thread 上跑，不需要文件）。不選留成限制：這幾個 API 在 worker 裡會停住或永遠不完成，WPT 的 FileAPI 的 worker variant 也一直失敗。
22. **WPT 加 `workers/` 裡 dedicated worker 的部分，已經在清單裡的目錄的 worker variant 都跑**（2026-11-17，1b-7 開始前）：`workers/` 排除 shared worker、module worker、巢狀的 worker、service worker；清單裡的目錄（FileAPI、performance-timeline、netinfo、screen-wake-lock……）的 worker variant 不再標成「沒有 worker」，預期結果寫真正的原因（例如第 2 階段才有的 `fetch()`、`performance`、`location`）。和 Chromium 一樣（整個 `workers/` 在 CI 跑，失敗的寫在 `TestExpectations`）。不選只更新清單裡的目錄、`workers/` 留到第 2 階段：dedicated worker 自己的測試在 1b 做完時就該跑。
23. **worker 的 `navigator` 的值在啟動時交給它的 engine**（2026-11-18，2a 開始前）：renderer 的 UA 和 `webdriver` 跟著啟動 worker 的參數一起給，worker 的 `Dom` 自己記著；物件沿用 `dom::Navigator` 的型別，表上另一個 `WorkerNavigator` interface，只有 worker 有的成員（NavigatorID 的一部分、language、`onLine`、`hardwareConcurrency`、`connection`）。和 Chrome 一樣（`GlobalScopeCreationParams` 帶著 user agent；Blink 的 `Navigator` 和 `WorkerNavigator` 共用 `NavigatorBase`）。不選每次讀的時候問 main thread：要跨 thread 同步等，值又不會變。
24. **worker 的 time origin 是頁面呼叫 `new Worker()` 的時刻**（2026-11-18，2a 開始前）：main thread 啟動 worker 時記下時刻，交給 worker，它的 `performance.timeOrigin` 和 `now()` 從那裡算。和 Chrome 一樣（`WorkerThread` 的建構子在頁面的 thread 上記下 `TimeTicks::Now()`，Chromium `main` 的 `a5229e6c` 的 `worker_thread.cc`）。不選 worker 的 global 建好的時刻：machopo 的 global 在主 script 載入之後才建，比 Chrome 晚一段載入的時間。
25. **`WorkerLocation` 照 HTML**（2026-11-18，2a 開始前）：worker 的 global 的網址（主 script redirect 之後的），成員都唯讀，`location` 每次是同一個物件（`[SameObject]`）。
26. **宣告表的成員也可以寫 exposed，realm 不裝它的 global 沒有的成員**（2026-11-18，2a 做中）：WPT 抓到 worker 裡讀 `performance.timing` 會讓 machopo 結束（`Performance` 兩邊都有，`timing`、`navigation` 在規範裡是 `[Exposed=Window]`，表上原本只能寫在整個 interface 上，worker 也裝了它們，造出沒有 window 的 `PerformanceTiming`）。改成屬性和方法可以寫 `#[exposed(..)]`，只能比它的 interface 少，`InterfaceDesc` 的每個成員帶著自己的 exposed，`dom_js` 做 template 時照 isolate 的 global 的種類跳過沒有的成員；同名的 overload 要寫一樣的。和 Chrome 一樣（Blink 的 IDL 的成員可以寫 `[Exposed]`，產生的 binding 依 global 決定裝不裝：worker 裡 `'timing' in performance` 是 false）。是第 2 條的「個別的成員需要時也可以寫」。不選在 getter 裡擋（worker 裡丟錯誤或給 `null`）：成員還在，`in` 和 `getOwnPropertyNames` 和規範、Chrome 不同，之後每個同樣的成員都要再擋一次。
27. **worker 的 `fetch()` 和 window 的共用一份，worker 的 thread 自己送**（2026-11-18，2b 開始前）：`fetch()` 收 global（window 或 worker 的），請求帶它是哪個 client 的，不再帶文件；worker 的 engine 在操作之後把請求交給 worker 的 host（`WorkerGlobalHost` 多送出和取消兩個方法），worker 的 thread 送 `FetchResource`（帶 worker），回應照第 9 條排進 worker 的佇列，在 worker 的 task 裡 resolve。initiator 是 worker 的 origin（請求的其他部分和 window 的一樣）；worker 結束時 Browser 照第 8 條的登記取消它的載入。和 Chrome 一樣（每個 `WorkerGlobalScope` 有自己的 `GlobalFetch::ScopedFetcher` 和 `FetchManager`，在 worker 的 thread 上載入；Chromium `main` 的 `9784e3bb` 的 `global_fetch.cc`）。不選另寫一份 worker 的 `fetch()`：兩份要一起維護，第 1 條要避開的就是這個。
28. **worker 建的 blob URL 由 worker 的 thread 自己向 Browser 登記，worker 結束時 Browser 拿掉**（2026-11-18，2b 開始前）：`RegisterBlobUrl` 帶 client（和 `FetchResource` 一樣），Browser 收到 `DedicatedWorkerGone` 時拿掉那個 worker 登記的；撤銷照 window 的做法（worker 自己馬上算撤銷，之前要求的載入送出之後才告訴 Browser）。登記和之後的載入走同一條通道，先後有保證；被強制結束的 worker 也不留下網址；同一個 renderer 的頁面也用得到 worker 建的。和 Chrome 一樣（worker 經過自己的 `BrowserInterfaceBroker` 拿一個 `BlobURLStore`，它跟著 worker 的 host 消失時解構子拿掉登記的網址；Chromium `main` 的 `9784e3bb` 的 `public_url_manager.cc`、`blob_url_store_impl.cc`）。不選交給 main thread 的 `BlobUrls` 代登記：頁面忙時登記晚於 worker 自己的 `fetch()` 到 Browser。不選由 renderer 在 worker 結束時撤銷：`terminate()` 在 script 跑到一半時結束 worker，main thread 要另外記著每個 worker 的網址。和 Chrome 不同、先不做的：頁面 `revokeObjectURL()` 撤銷不了 worker 建的網址（Chrome 只看是不是同 origin），記在 `docs/limitations.md`。

## 待決定

1a 開始前決定的三條都決定了：

1. （已決定，見「已決定的事項」第 1 條。）
2. （已決定，見「已決定的事項」第 2 條；比較過的三個做法在「理由和其他做法」。）
3. （已決定，見「已決定的事項」第 3 條。）

1b 開始前決定的五條都決定了：

4. （已決定，見「已決定的事項」第 8 條。）
5. （已決定，見「已決定的事項」第 9 條。）
6. （已決定，見「已決定的事項」第 10 條。）
7. （已決定，見「已決定的事項」第 11 條。）
8. （已決定，見「已決定的事項」第 12 條。）

1b 的第一步是一個原型：worker 的 thread 上跑 script，從另一條 thread 打斷正在呼叫 DOM 的 script，確認不會 crash、engine 和 heap 照第 12 條的順序放得掉。

## 之後的可能

接著這份做的：

- **shared worker**：好幾個文件共用一個 worker，要由 Browser 找到或建立它，port 要跨 renderer。
- **service worker**：要攔截網路請求，還要先有 cookie、HTTP cache 和持久的儲存；另一份 RFC，排在那些之後。
- **worklet**（`AudioWorklet`、CSS 的 paint worklet）、`OffscreenCanvas`、worker 的 DevTools session、`FileReaderSync`。
- **wasm 文件的 worker**。
- **沒人拿著、也沒在忙的 worker 收掉**（HTML 的 protected worker）：「已決定的事項」第 19 條沒有選的。worker 回報有沒有 timer、網路連線、port，頁面那一側的 `Worker` 物件被 GC 收掉時請 host 結束 worker。Chrome 現在也不做；沒存下 `Worker` 物件、又一直有 timer 的頁面多佔一條 thread 和一個 isolate，到文件被丟掉為止。

- **realm 的種類寫在型別上**：`ApiCx`（和它底下的 `dom::Cx`）多一個型別參數，說它是 window 的 realm、worker 的 realm，或兩者都可能；window 才有的 `dom` 函式（`window_of`、`target_window`、文件的）只在 window 的那一型上，`declare!` 照 exposed 給成員哪一型。只在 worker 裡跑的程式因此在編譯時碰不到 window；兩邊都跑的程式照樣經過回傳 `Option` 的函式，在執行時分辨（「已決定的事項」第 7 條）。`dom_api` 幾乎每個成員的簽名都會動到，要等 1b 之後有只在 worker 裡跑的程式時才有意義；另一份 RFC。Blink 和 Servo 都是執行時檢查（`DynamicTo<LocalDOMWindow>`、`GlobalScope::downcast::<Window>()`）。

### 架構上可能要先做的事（2026-11-17 的評估）

起這份 RFC 時，一起評估了「哪些缺口拖越久、要回頭改的程式越多」。結論記在這裡，讓之後排 RFC 的順序時有依據；寫在這裡不代表已經決定要做，也不是決定好的順序。依據是 `docs/architecture.md`、`docs/limitations.md` 和幾次搜尋，不是逐行審過程式碼。

| # | 缺口 | 為什麼拖了會變貴 | 2026-11-17 的現況 |
| --- | --- | --- | --- |
| 1 | 執行環境不只有 Window | 每加一個 API 就多一處「global 是 Window」的假設 | 這份 RFC |
| 2 | frame 樹和導覽由 Browser 擁有 | 子 frame 的歷史、`sandbox`、cookie 算哪個網站、權限、跨源的 `postMessage`、site isolation 都建在它上面。Chromium 當年 frame 也是 renderer 內部的，後來為了跨 process 的 iframe 才把 frame 樹和導覽搬到 Browser | Browser 不知道子 frame，子 frame 的導覽由 renderer 自己載入（`docs/limitations.md` 的「iframe」） |
| 3 | layout 的座標模型和分段 | 每個只照物理方向寫的 layout 功能，之後都要補邏輯方向（`direction`、`writing-mode`）和斷開的情形（multi-column、列印）。Blink 重寫成 LayoutNG 的原因之一是舊的 layout 補不上分段 | 沒有 `direction`、`writing-mode`，multi-column 只有一部分；layout 建在 taffy 上。先要一份評估：taffy 的 fork 撐不撐得起 |
| 4 | renderer 真的拆成 process 跑一次 | 邊界只有靜態檢查（`sanity.py boundary`），沒有實際跑過；每加一則訊息就多一個沒驗證過的假設 | `ipc` 只有時鐘；`gfx::FontData`、`ImageData` 不能序列化；tile 的 texture 還是同步呼叫 Viz（`docs/architecture.md` 的「Process 的邊界」的例外 1） |
| 5 | 載入是串流的 | incremental parsing、`fetch()` 的 streams、影音、server-sent events 都要網路到 renderer 的資料管道 | 導覽、iframe、`fetch()` 的 body 都是一次收完（`docs/limitations.md` 的「Parser 和載入」） |
| 6 | 持久資料的 key | cookie、HTTP cache、`localStorage`、IndexedDB 要用同一把 key（origin 加最上層的網站） | 都還沒有，所以還沒有債；第一份儲存的 RFC 定下來 |

當時的看法：1、2、3 先做（3 先做評估），4 的價值高、成本也最高，現有的靜態檢查已經擋住大部分的退化，排在 2 之後；5、6 在用到它們的功能的 RFC 裡定。往上加的功能（CSS 的 property、顏色、小的 API、`XMLHttpRequest`、Web Storage）晚做不會變貴，可以排在 1、2 之後。
