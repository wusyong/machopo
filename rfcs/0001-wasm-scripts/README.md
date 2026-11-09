# RFC 0001：wasm 腳本

- 開始日期：2026-10-01
- 狀態：✅ 已完成：2026-10-05（2026-10-01 接受的是跑在 V8 上的設計；2026-10-04 改走 wasmtime + cppgc、Component Model 和 WASI 0.3，見「已決定的事項」第 8–15 條；2026-10-05 改成 JS 和 wasm 的文件分開、兩套 DOM，第 26 條；原本的第二層「cppgc 當 wasmtime 的 GC」2026-10-05 移到 RFC 0005，第 54 條）
- 細節：`details.md`；實作前用原型釐清的問題和結論：`wasmtime-questions.md`（原型在哪裡見它的開頭）；驗收和做過的檢查：`progress.md`

P3 做了 JS，同時做好這份設計需要的共用分層（`P3.md` 第 4 節）。跑在 V8 上的版本已經在 `wasm` 分支做完（W1：`kun:dom/v1` 和 Rust SDK；W2：WASI preview1、`kun:dom` 的 WIT、C / Go / Lua），2026-10-04 決定從 main 重新實作：wasm 改跑在 wasmtime 上，guest 是 component，DOM 介面只留 WIT，WASI 用 0.3；2026-10-05 做完（`wasmtime` 分支）。`wasm` 分支留著當對照組。

## 摘要

網頁可以完全不用 JS，只用 wasm 寫腳本：guest 是一個 component（Component Model），import kun 自己的 DOM 介面 `kun:dom`，直接操作 DOM。介面寫成 WIT，從 `dom_api` 的 API 宣告表產生，各語言的 binding 由 wit-bindgen 產生；另外提供 WASI 0.3（P3），讓社群工具鏈編出的 component 不經過 kun 特製的工具就能執行。

一份文件只用一種 script：用 `<meta>` 宣告的 wasm 文件只執行 wasm，跑在 wasmtime 上，DOM 放在單獨的 cppgc heap 上，不建 V8 的 isolate；其他文件照舊是 JS 文件，跑在 V8 上。兩套 DOM 不互通。wasm 的路徑不依賴 V8，所以之後可以有不跑 V8 的設定：只有 wasmtime 和 cppgc。每個 wasm script 一個 wasmtime 的 `Store`，由 cppgc 上的一個 GC 物件（holder）代表，所以 guest 拿著的 DOM 物件、DOM 拿著的 guest callback 之間的循環照樣能回收。`kun --script wasm` 是不建 isolate 的設定：每份文件都是 wasm 文件。讓 cppgc 當 wasmtime 的 GC（wasm 自己的 GC 物件也放在 cppgc 上）是 RFC 0005。

## 動機

標準的 Web 上，wasm 不能直接存取 DOM：wasm-bindgen、Emscripten、Kotlin/Wasm 編出來的 wasm，都經過它們產生的 JS 膠水呼叫 DOM API。這條「wasm 加上 JS 膠水」的路，在 P3 有 JS 之後自然就能用（實際能跑多少，取決於 DOM API 的覆蓋率），不需要這份 RFC。這份 RFC 處理的是「完全不用 JS」的那一種：中間沒有 JS 膠水，錯誤不用靠膠水接住例外。它的 DOM 介面是 kun 自己的擴充，不是 Web 標準。

**為什麼是 wasmtime**（2026-10-04）：
- **資源和隔離**：每個 Store 自己的 memory 上限、用 epoch 中斷失控的 wasm（kun 到現在都還沒有 watchdog）。這份 RFC 還沒用上（`docs/limitations.md`），之後再接。
- **等待不需要 JSPI**：WASI 0.3 和 Component Model 的 async，guest 等待時 host 照常執行。
- **不需要 JS realm**：wasm 拿到的 window 就是一個 host 物件，只有 wasm 的頁面不必建 JS 的 global。
- **Component Model 和 WASI 在這一側**：WIT、wit-bindgen、wasmtime-wasi（P3 是預設的 feature）都是現成的，不用自己實作 canonical ABI。
- **原本不選它的理由（GC）有了做法**：見「Store 和 GC」。
- **可以不要 V8**：wasm 的路徑只用 wasmtime 和 cppgc，之後 kun 可以有一種不跑 V8、只當 wasm 的 engine 用的設定（「不跑 V8 的設定」）。

**為什麼只留 WIT**（2026-10-04）：
- **一種介面**：`wasm` 分支要同時維護 `kun:dom/v1`（`externref`）和 WIT 兩種形式；WIT 這一種，各語言的 binding 都由 wit-bindgen 產生，不需要 kun 自己的 SDK 和 `kun-wasm link`。
- **GC 更單純**：guest 只拿著整數的 handle，cppgc 和 wasmtime 之間沒有 GC 參照，第一層不需要 fork wasmtime 的 collector。
- **Winch 可以用**：Winch 不支援 reference-types，跑不了 `externref` 的 module。
- **字串直接**：參數是 UTF-8 的 `(指標, 長度)`，和 DOM 的 `DomString` 一樣是 UTF-8，只複製一次。

目標：
- 網頁可以只用 wasm 寫腳本，不需要任何 JS。
- **一份文件只用一種 script**：宣告成 wasm 的文件只執行 wasm，DOM 不經過 V8；其他文件照舊是 JS。兩種文件各有自己的 DOM，不互通（「已決定的事項」第 26 條；2026-10-04 的目標原本是同一個頁面可以同時有 JS 和 wasm、互傳 DOM 物件）。
- wasm 呼叫 `dispatchEvent` 時，listener 照 DOM 規範同步執行，包括同一個 module 裡的。
- 社群工具鏈直接編出的 component 就能執行；驗收的語言是 Rust、C、Lua（`wasm` 分支的 Go 拿掉了，因為 Go 還沒有 WASI 0.3 的 target，見「已決定的事項」第 12 條）。
- 架構留下接其他 engine 的空間：API 宣告和 DOM 的語意只有一份。
- **wasm 的路徑不依賴 V8**：只有 wasm 的頁面不需要 isolate；之後可以有不跑 V8 的設定。

不在範圍內：
- 讓現有的 web-sys 程式不經過 JS 執行。它們用自己的 JS 膠水跑在 JS 模式上（見「理由和其他做法」）。
- 同一份文件裡同時用 JS 和 wasm，包括 wasm 文件裡的 JS 的 `WebAssembly` API 和 wasm-bindgen 的膠水：要用它們就寫成 JS 文件，`WebAssembly` 照舊由 V8 執行。
- host 提供的檔案系統、socket、thread。
- WASI 0.2 和 preview1：wasmtime-wasi 做得到，但 kun 只承諾 0.3。
- Go：還沒有 WASI 0.3 的 target。
- 編成 wasm GC 的語言直接用 GC 參照碰 DOM（要等 Component Model 的「GC 版 canonical ABI」，見「之後的可能」）。
- 不連結 V8 的 build（standalone 的 cppgc、`v8` crate 的 cppgc 部分拆出來、CI 多一種 build）：另一份 RFC。這份只保證 wasm 的路徑不依賴 V8。
- cppgc 當 wasmtime 的 GC（原本的第二層）：RFC 0005（「已決定的事項」第 54 條）。

## 設計概要

網頁宣告自己是 wasm 文件（前 1024 byte 裡的 `<meta>`，「已決定的事項」第 27、36 條），用一個新的 script type 載入 wasm：

```html
<meta name="script" content="wasm">
<script type="application/wasm" src="app.wasm"></script>
```

作者用 wit-bindgen 從 kun 的 `kun:dom` WIT 產生 binding。用 Rust 寫起來像這樣（`sdk/rust` 的 `kun_guest`，`sdk/rust/README.md`）：

```rust
use kun_guest::exports::kun::dom::guest::Guest;
use kun_guest::kun::dom::dom::{Event, MutationObserver, MutationRecord, Node, Window};

struct Script;

impl Guest for Script {
    async fn start(window: Window) {
        let document = window.document().unwrap();
        let p = document.create_element("p").unwrap();
        p.set_text_content(Some("Hello from Rust")).unwrap();
        let body = document.body().unwrap().unwrap();
        body.append_child(&p.as_node()).unwrap();
    }

    // One per kind of callback (RFC 0010's decision 10).
    fn handle_event(id: u32, event: &Event) -> bool { true }
    fn run(id: u32) {}
    fn frame(id: u32, time: f64) {}
    fn mutations(id: u32, records: Vec<MutationRecord>, observer: &MutationObserver) {}
    fn accept_node(id: u32, node: &Node) -> Option<u16> { Some(1) }
}

kun_guest::export!(Script);
```

C 用 wasi-sdk 的 `wasm32-wasip3` 和 wit-bindgen 的 C binding（`sdk/c/`）；Lua 是一個 wasm 的直譯器（用 C 寫的，`sdk/lua/`），執行頁面上的 `<script type="text/lua">`。

作者會看到的行為：
- **錯誤**：每個 DOM 函式回傳 `result<_, dom-exception>`，`dom-exception` 有 code 和訊息；binding 把它變成各語言的錯誤值。
- **DOM 物件**：WIT 的 resource，guest 拿到的是 handle，用完要 drop（Rust 的 binding 自動處理）。WIT 沒有繼承，要轉型時用 `as-node`、`from-node` 這類函式。
- **執行時機**：照 module script（`type="module"`）的規則，不擋住 parser，parse 完之後和 `defer` 的 script 照文件順序執行，都在 `DOMContentLoaded` 之前；有 `async` 的編好就執行。`start` 第一次停下來（等待）就算執行完了，後面的 script 和 `DOMContentLoaded` 不等它回傳，和 module script 的 top-level await 一樣（「已決定的事項」第 42 條）。MIME 一定要是 `application/wasm`。
- **能用什麼**：只有 import 給它的：DOM、timer，加上 WASI 0.3（stdout、stderr 到 console，時鐘、亂數；沒有檔案和 socket）。
- **不和 JS 混用**：wasm 文件裡的 JS（`<script>`、`onclick=""` 這類屬性）不執行，JS 文件裡的 wasm script 也不執行，就像 V8 不認得的 script type 一樣。wasm 呼叫 `dispatchEvent` 時，listener 照規範同步執行。
- **等待**：等待中的程式（async 的 Rust、C 的 `sleep`）不擋住頁面；它暫停期間，送給它的事件怎麼處理見「等待」。
- **記憶體**：guest 拿著的節點、guest 的 callback 和 DOM 之間的循環照樣被回收（「Store 和 GC」）。
- **出錯**：compile 失敗、import 不符合、trap，都像 JS 沒被接住的例外一樣回報，接著執行下一個 script。Component Model 規定 component trap 之後不能再被進入，所以一個 listener trap，這個 module 之後的 listener 和 timer 都不再執行。wasm 自己分派、自己的 listener trap 時，trap 照規格一路傳到最上層，那次分派排在後面的 listener（包括其他 wasm script 的）也被跳過，先記成限制（「已決定的事項」第 18 條）。
- **介面有版本**：WIT 的 package 帶版本（現在是 `kun:dom@0.6.0`；RFC 0011 的 3a 給 `MouseEventInit` 加了修飾鍵，RFC 0010 加了 `MutationObserver` 和 `AbortController`）。還不穩定：不相容的改動升 minor，照舊版本編的 guest 載入時就被擋下，要重新編（RFC 0008 的「已決定的事項」第 1、4 條；這份 RFC 做完時原本定為 1.0.0、只新增函式）。

## 詳細設計

### 架構

```
renderer：parse 之前掃前 1024 byte 的 <meta>（或照 kun --script wasm 固定），決定文件是
          JS 還是 wasm 的，用那一種的 DomIsolate 和 engine（換頁時種類不同就換）；
          <script> 依 type 分成 JS 和 wasm，
          只執行文件那一種；wasm 的 bytes 交給 engine compile，
          編好的結果回到 main thread 的 task queue，照順序執行
   └─ 只透過 dom_api 的 ScriptEngine（runs_wasm、compile_wasm、run_wasm_script）
dom_wasm：WasmtimeEngine（只有 wasm，不包住 V8Engine）、compile thread、
          kun:dom 的 host 函式（從 dom_api::wit 產生）、WASI 0.3（wasmtime-wasi，
          時鐘自己實作）、入口、「需要時才跑」的 event loop
dom_js：JS 前端、V8Engine（不變）
選 engine 的地方（shell）：裝 V8Engine 和 WasmtimeEngine，renderer 照文件選一個；
          不跑 V8 時只有 WasmtimeEngine
dom_api：API 宣告表、中性的值、規範的演算法、事件分派、ScriptHost、ScriptEngine、
         kun:dom 的 WIT（wit）
dom：樹、GC、scope、token、ScriptCallback；JS 文件的 cppgc heap 接在 isolate 上，
     wasm 文件的是單獨的 cppgc::Heap，GC 排成 renderer 的 task
   ├─ V8 模組（不變）
   └─ 外部 engine 的模組：DomAccess 和入口、holder 和 slot 表、Store 表、callback 的存法；
      和 engine 無關（第 22 條）
net：.wasm 的 MIME（application/wasm）、data: URL

guest 那一側：
sdk/：C / Rust 的範例（Rust 的 binding kun_guest）、Lua 的 runtime 和它的 binding 的
      產生器（只讀 kun:dom 的 WIT，自己的 cargo workspace，第 45 條）
```

依賴規則照 `P3.md` 第 4 節的做法，由 `sanity.py deps` 檢查：V8 專屬的程式只在 `bindings`、`dom_js` 和 `dom` 的 V8 模組；wasmtime 專屬的只在 `dom_wasm`；**`dom_wasm` 和 `dom` 的外部 engine 模組不依賴 `bindings`、`dom_js` 和 isolate**，只用 cppgc 的 API（現在是 `v8::cppgc`）。

### 不跑 V8 的設定

wasm 的路徑不依賴 V8，所以 kun 可以只當 wasm engine 用：script 只有 wasmtime，所有文件都用 wasm 的 DOM（沒有宣告的文件也是，JS 不執行）。分兩步：

1. **不建 isolate、不跑 JS**（這份 RFC 做了）：`kun --script wasm`（「已決定的事項」第 51–53 條）。`RendererFlags::script_language` 固定每份文件的種類，renderer 從啟動就只用 wasmtime；V8 照樣連結、初始化（cppgc 用它的 platform），但不建 isolate；預設首頁是 `about:wasm-demo`。
2. **不連結 V8**：換成 standalone 的 cppgc（V8 的 GN 有 `cppgc_standalone`），`v8` crate 的 cppgc 部分要拆出來，CI 多一種 build。超出 wasm 腳本的範圍，另開一份 RFC。

**沒有 isolate 時誰來排 GC**：每份 wasm 文件的 DOM 都在單獨使用的 `cppgc::Heap` 上（「已決定的事項」第 16、26 條）。JS 文件由 V8 決定什麼時候 GC；不接 isolate 的 `CppHeap` 完全不會自己收，唯一的 GC 入口（testing API）收尾時一次 sweep 完，太慢（2026-10-04 確認）。單獨的 heap 選不掃 stack 時，cppgc 自己決定什麼時候收，經過 kun 提供的 Platform 把 GC 排成 renderer 的 non-nestable task（`dom` 的 `TaskQueue`；incremental marking 的步驟 cppgc 只排第一步，之後的由 kun 自己排，RFC 0010 的「已決定的事項」第 7 條），所以 GC 只發生在 task 之間；sweep 也一段一段做。最長的停頓不到 2 ms（`wasmtime-questions.md` 的 Q18）。

### `kun:dom`

宣告表的每個 interface 是 WIT 的一個 resource，成員是它的方法（父 interface 的方法攤平到每個子 resource 上，往上轉是 `as-x`、往下是 `from-x`）；字典是 record；錯誤是 `result<_, dom-exception>`（`code` 的名稱寫在它的註解裡）；callback 是 guest 自己的 id（`type callback = u32`，只讀 WIT 的工具也分得出它，第 45 條），DOM 呼叫它時轉成那種 callback 的 guest export（`handle-event`、`run`、`frame`、`mutations`、`accept-node`，RFC 0010 的第 10 條）。WIT 由 `dom_api::wit` 從宣告表產生，有 golden file；宣告表改了已有的函式，測試失敗。guest 的入口是 export 的 `start(window)`，`async func`（第 21 條）。

會同步呼叫回 guest 的 DOM 函式（宣告表的 `async method`：`dispatchEvent`、`AbortController.abort()`）怎麼宣告，見「執行 script 的入口和重新進入」。

`wasm` 分支做好的 WIT 帶過來（只有產生 WIT 的部分），它借用的 `wasm_abi` 的幾個小函式搬進 `wit`。實作期間是 `kun:dom@0.1.0`，RFC 做完時（2026-10-05）定為 1.0.0，之後只新增函式（「已決定的事項」第 20 條）；同一天 RFC 0008 改回 0.1.0，介面還不穩定。

host 那一側，每個 WIT 函式用 wasmtime 的 `func_new` 接上同一個共用的 host 函式，參數用 `Val` 照宣告表轉換（第 24 條）；每次呼叫的成本和 profile 見 `progress.md` 的「量測」（第 50 條）。

### Store 和 GC

DOM 在 cppgc 上，wasmtime 的物件在它自己的 Store 裡，兩邊互相看不到。Store 被丟掉時，它的 instance、table、memory 才一起釋放。所以讓每個 Store 在 cppgc 的圖上有一個代表它的節點：

- **每個 wasm script 一個 Store**，cppgc 上有一個代表它的 GC 物件，叫 holder。
- **guest 拿到的 DOM 物件**（resource 的 handle）在 host 那一側只是 holder 裡一張 slot 表的位置；slot 是指向節點的 `Edge`。holder 的 `trace()` 標記所有還在用的 slot；guest drop 那個 handle 時，slot 還回去。slot 表是一串固定大小的 chunk（每個 chunk 是一個 GC 物件，放一個 `Edge` 的陣列），因為 GC 物件裡不能放 `Vec<Edge>`（`crates/dom/soundness.md` 的 I5）。
- **guest 的 callback** 在 DOM 裡是 `ScriptCallback` 的另一種：holder 加上 guest 的 id（「已決定的事項」第 30 條）。listener 存的是指向 holder 的 `Edge`，timer 存在 heap 外面，用 `Persistent` 撐住 holder。
- **Store 本身不放進 GC 物件**：放在 `DomState` 一張不在 heap 上的 Store 表裡，每一列是「holder 的 id → holder 的弱參照、Store」（第 29 條）。

這樣「節點 → listener → holder → slot → 節點」整個在 cppgc 的圖裡，沒有外部參照時一起被回收。holder 被回收時它的弱參照被清掉，event loop 的最外層 prune Store 表時，把弱參照空掉的那一列連同 Store 一起 drop，和 `dom` 現有的 `NodeIdMap` 一樣。cppgc 的 finalizer 裡不能 drop Store（會執行任意的 destructor）；2026-10-05 以前的設計把 Store 放在 holder 裡，finalizer 把它移到「待 drop」的佇列，要為 I6（finalizer 只釋放記憶體）開例外，所以改成這張表。Store 在執行中、`start` 還沒回傳、或還有等待中的 WASI 時鐘時，由一個 root 撐住（第 32 條）；之後只有 DOM 指向它（listener、timer）時才活著。guest 只拿著整數的 handle，所以不需要掃描 wasm 的 stack，也不用管 fiber。

回收的粒度是整個 Store：Store 還活著時，它拿著的 slot 都算活的。這和 `wasm` 分支裡用 linear memory 的語言一樣（Rust SDK 的 `externref` table 是 instance 的 root）。細節見 `details.md` 的「Store 當成 cppgc 物件」。

### 執行 script 的入口和重新進入

照 `P3.md` 第 3 節的規則：執行 script 的入口拿走 `&mut DomIsolate`，binding 被呼叫時才替自己開 scope。wasmtime 的機制：

- **入口**要 `&mut DomIsolate`，建立這份 DOM 唯一的 `DomAccess`（不能複製，也不能從別的地方造出來），從 Store 表取出 Store，把 `DomAccess` 放進 Store 的 data；結束時拿回 `DomAccess`、放回 Store。
- **`DomAccess` 是 `BindingContext` 的對應物**（「已決定的事項」第 28 條）：host 函式從 Store 的 data 拿到它，開 DOM scope 要借用它。所以同一時間最多只有一個 DOM scope；開 scope 時 Store 也正被借用，「開著 DOM scope 時不能執行 script」和 V8 版本一樣由借用規則保證，不需要執行期的旗標。wasmtime 要求 Store 的 data 是 `Send`，所以 `DomAccess` 是 `Send`，用的時候檢查是不是 heap 的 thread。開著 scope 時繞過去執行了 guest，巢狀的 host 函式也拿不到 `DomAccess`，只會失敗，不會有第二個 `&mut DomState`。分派給別的 Store 時，把 `DomAccess` 交給對方的 Store，呼叫完拿回來。wasm 執行中 Rust 的程式 panic 時，`DomAccess` 交還不了，所以入口在 unwind 時 abort，和 V8 的 binding 裡 panic 一樣；trap 不是 panic，照樣回報。
- **同一個 Store 的重新進入**（wasm 呼叫 `dispatchEvent`，分派到同一個 module 的 listener）：WASI 0.3 讓 Store 一定是 async 的，而 wasmtime 49 的 async Store 裡，sync 的 host 函式不能呼叫回 guest，blocking 的 host 函式重新進入也不行（同一個 Store 的 event loop 不能遞迴，`cm-prototype` 的 `async-reenter`）。能走的路是：
  - **會呼叫回 guest 的 DOM 函式宣告成 `async func`**（宣告表的 `async method`：`dispatchEvent`、`AbortController.abort()`、traversal 會呼叫 filter 的方法；之後的 `click()`、`focus()` 也是），其他 DOM 函式照舊是 sync 的。
  - **guest 的 callback 的 export 是 sync 的**（`handle-event` 等，RFC 0010 的第 10 條）；host 用 `call_concurrent` 呼叫 listener，**不等任何東西，當場完成**：JS 的 listener 本來就是同步的，送給其他暫停中 Store 的事件照「等待」的規則處理，不等它們。
  - guest 那一側：async 的程式 `await` 它；sync 的程式用 `block_on`，因為當場完成，所以不會變成「sync 的 task 不能 block」的 trap。
  - 2026-10-04 在 `cm-prototype` 確認成立（`wasmtime-questions.md` 的 Q2）：sync 和 async 的 guest 都能同步分派給自己的 listener，巢狀也可以，和 host 的 listener 混在一起時順序也對。
  - **實作**（第 34 條）：`dom_api` 只有會呼叫回 guest 的路（分派的 `dispatch`、`invoke`、`inner_invoke` 和 `dispatchEvent` 本身，`AbortController.abort()`，traversal 呼叫 filter 的方法）是 `async fn`；碰到同一個 Store 的 listener 時，把 `DomAccess` 放回 Store 的 data、`await` 那次呼叫（由 Store 的 event loop 執行）、再拿回來；分派給別的 Store 照樣同步，V8 那邊 poll 一次就完成。分派的 future 拿著 `NodeHandle`，用 `dom` 的 `ThreadBound` 包起來交給 wasmtime。
  - **限制**：分派繞回一個正在更外層執行的 Store（A → B → A）時，那個 listener 被回報、跳過（`docs/limitations.md`）。

### 等待

guest 在 WASI 0.3 的 async 函式裡等待時，不能擋住 renderer 的 main thread。2026-10-04 在 `cm-prototype` 確認的做法（`wasmtime-questions.md` 的 Q4、Q5）：

- **Store「需要時才跑」**：每個 Store 平常放在 Store 表裡。被喚醒（等待到期、事件）或被分派時才取出，跑它的 event loop（poll `run_concurrent` 到沒有事可做就丟掉；poll 途中被 guest 的 `YIELD` 叫醒時再 poll 同一個，否則 wasmtime 已經取出的工作會不見，第 41 條）再放回去。guest 的 task 和 host 的 future 都留在 Store 裡，等的東西到了，waker 在 engine 自己的佇列排一個 task（第 33 條），renderer 在 event loop 的最外層跑它，跑了 script 就請求 main frame。不需要 tokio，也沒有 busy loop。取出期間表裡那一列是空的，正好是「這個 Store 正在 stack 更外層執行」的標記。
- **時鐘由 kun 實作**：wasmtime-wasi 的 WASI 0.3 時鐘用 `tokio::time::sleep` 等待，所以 `wasi:clocks/monotonic-clock` 由 kun 自己實作：等待是 engine 佇列裡的延遲 task，不是 DOM 的 timer（不該有巢狀 timer 的下限，也不該被 `clearTimeout` 碰到，第 33 條），`now` 降到 100 µs；其他 WASI interface 用 wasmtime-wasi。
- **等待期間送來的事件**：listener 直接進去執行，不等 guest 醒來。這是 Component Model 的語意（sync 的 export 不理會 backpressure），和 JS 在 `await` 期間照樣執行事件一樣；用 C 這類 sync 程式在 `sleep` 裡被呼叫時，對它來說像 signal handler。
- **分派給別的 Store**：把那個 Store 從 Store 表取出、把 `DomAccess` 交給它、跑一次它的 event loop 呼叫 listener，listener 在 `dispatchEvent` 回來之前執行，回傳值也正確，對方正在等待也一樣。如果每個 Store 被一個常駐的 `run_concurrent` 拿著，就只能排隊，違反同步分派。

### 載入和執行

```
<script type="application/wasm" src="app.wasm">
  1. renderer 經 FetchResource 下載（和外部 JS 一樣），MIME 要是 application/wasm
  2. engine 在自己的 compile thread 上編譯 component（Cranelift），檢查 import（instantiate_pre）
  3. 編好的結果經 renderer 的 task queue 回到 main thread
  4. 輪到它時：建 Store 和 holder、接上 kun:dom 和 WASI 0.3、實例化、呼叫 start(window)；
     start 第一次停下來就算執行完了（第 42 條）
```

每次載入都重新編譯（沒有 code cache），Lua 的 runtime 約 45 ms（`progress.md` 的「量測」）；先不處理（第 49 條），做法記在「之後的可能」。安全和資源（signal handler、memory 上限、執行時間、權限、V8 的 sandbox）見 `details.md` 的「安全和資源」；接其他 engine 要實作什麼見 `details.md` 的「接其他 engine」。

### 和 V8 版本（`wasm` 分支）的關係

- **照搬**：`dom_api::wit`（只有產生 WIT 的部分，見「已決定的事項」第 20 條）、`ScriptEngine` 的 wasm 部分、renderer 和 net 的載入（`application/wasm`、`data:` URL）、和 engine 無關的 DOM 功能（`performance.now()`、`localName`、時間降到 100 µs，第 19 條）。
- **原始碼帶過來、重新編成 WASI 0.3 的 component**：C 的 W7、Lua 的 runtime（`start` 改成 async）。`wasm` 分支把它們編成 preview1 的 core module。
- **改寫的**：reftest W1–W6，原本用 `kun:dom/v1`，改用 WIT 的 guest，W2、W4 重新定義（第 39、42–44 條）；Lua binding 的產生器改成只讀 WIT（第 45 條）；Rust 的 guest 改用 `sdk/rust` 的 `kun_guest`（第 40 條）。`about:demo` 是 JS 文件，wasm 的示範是另一頁 `about:wasm-demo`（第 38、47 條）。
- **重寫的**：`wasm` 分支的 `dom_wasm`（V8 上的 wasm）重寫成這裡的 `dom_wasm`（wasmtime；2026-10-05 以前叫 `dom_wasmtime`，第 55 條），以及 `dom` 的外部 engine 模組。
- **沒帶過來**：`kun:dom/v1`（`dom_api::wasm_abi`）、Rust 的 `kun_dom` SDK、`kun-wasm link`、`bindings` 的 wasm 部分、`wasi_host`（WASI 用 wasmtime-wasi）、Go 的範例和 reftest W8。

兩個版本的啟動、記憶體和每次呼叫的比較見 `progress.md` 的「量測」（第 48 條）。

### 測試

- **GC**：只有 wasm 拿著的節點和它的 listener 經過 GC 還能用；「節點 → listener → holder → slot → 節點」的循環被回收，之後的下一個 event loop 的 step 才 drop Store；wasm 文件的 heap 只在 task 之間 GC；也在 `test.py matrix` 和 `fuzz` 底下跑。
- **入口和重新進入**：同一個 Store 的同步分派、分派給別的 Store、等待中的 guest（`start` 裡等、`start` 回傳之後等、`YIELD`）；`DomAccess` 擋住第二個 DOM scope（compile-fail 測試）。
- **文件的種類**：`<meta>` 決定文件的種類；JS 文件不執行 wasm script，wasm 文件不執行 JS；換頁時種類不同就換 DOM；`--script wasm` 從頭到尾不要 JS 的 engine。
- **整個瀏覽器**：reftest W1–W7、W9 和 `wasm/wait.html`（W8 是 Go，拿掉了）。
- **ABI**：`kun:dom` 的 WIT 有 golden file；編好的 Lua runtime 帶著它所根據的 WIT 的 hash（第 56 條）。
- **沒做的**：同一個情境用 JS 和 wasm 各跑一次、比較兩份 document 的「共用情境測試」（`docs/test-gaps.md` 的「能補、還沒補的測試空缺」）。

每一項的測試和做過的 mutation test 見 `progress.md`。

### 分階段

2026-10-04 到 2026-10-05 做完；每一步的驗收和做過的檢查見 `progress.md`。

1. **釐清**：照 `wasmtime-questions.md` 一題一題做原型（不合併），先回答 `dispatchEvent` 怎麼宣告（第 17、18 條）。
2. **搬移**：從 `wasm` 分支帶過來和 engine 無關的部分（第 19、20 條）。
3. **第一個 guest**，分五段：3a wasm 文件的單獨的 heap，GC 排成 renderer 的 task（第 16 條）；3b `dom` 的外部 engine 模組：`DomAccess` 和入口、holder 和 slot 表、Store 表、callback 的 enum（第 22、28–32 條）；3c `dom_wasm`：compile thread、`kun:dom` 的 host 函式、WASI 0.3、async 的 `start`、「需要時才跑」的 event loop（第 21、24、32、33 條）；3d 純 wasm 的重新進入，分派演算法改成 async（第 34 條）；3e renderer 照文件選 DOM 和 engine、reftest W7、`about:wasm-demo`（第 25–27、35–38 條）。
4. **其他 guest**，分三段：4a Rust 的 guest、`YIELD` 之後叫不醒的 bug、`start` 回傳之後的等待（第 40、41 條）；4b reftest W1–W6（第 39、42–44 條）；4c Lua（W9），產生器只讀 WIT（第 45–47 條）。
5. ~~**兩個 engine 交錯**~~：2026-10-05 拿掉，JS 和 wasm 不在同一份文件裡（第 26 條），不會交錯。
6. **和 V8 版本比較**：6a 量啟動、記憶體、每次呼叫的成本，6b 決定（第 48–50 條：都先不改）。
7. **不跑 V8 的設定**：`kun --script wasm`（第 51–53 條）。

原本的**第二層：cppgc 當 wasmtime 的 GC**（第 13 條）2026-10-05 移到 RFC 0005（第 54 條）。

## 缺點

- **只有 kun 能跑**：`kun:dom` 是 kun 自己的擴充。介面一旦定下來（1.0.0）就要長期維持，新版本要和舊版本並存；在那之前，不相容的改動都要 guest 重新編。
- **兩個 JIT、兩套 wasm engine**：有 JS 的設定裡，JS 和頁面 JS 的 `WebAssembly` 在 V8，`<script type="application/wasm">` 在 wasmtime。執行檔和編譯時間都變大（strip 之後的執行檔多 16 MB，乾淨的 release build 從 59 秒變成 135 秒，`wasmtime-questions.md` 的 Q11）；不連結 V8 之前，執行檔照樣帶 V8（cppgc 從它來）。
- **啟動比 V8 慢**：每次載入都用 Cranelift 重新編譯，Lua 的 runtime 在 kun 裡約 45 ms，C、Rust 的小 guest 7–13 ms；V8 的 Liftoff 3 ms（lazy 的話幾乎是 0）。code cache 之後是 0.3 ms，Winch 10 ms（Q11），先不做（第 49 條）。
- **每次呼叫比 JS 慢**：在 kun 裡量到是 JS 的 2–5 倍（`setAttribute` 666 ns、`parentNode` 488 ns、`dispatchEvent` 2.8 µs），一個 frame 一千次呼叫約 0.3–0.7 ms；比 V8 版本（`kun:dom/v1` 和 handle 形式）都快。時間大多在 wasmtime 的動態 host 函式和 handle 的兩張表（`progress.md` 的「量測」，第 50 條）。
- **trap 之後整個 module 失效**：Component Model 的規定；core wasm 的 instance trap 之後還能再呼叫。規格也要求 trap 一路傳到最上層，所以 wasm 自己分派時它的 listener trap，那次分派剩下的 listener 也被跳過（「已決定的事項」第 18 條）。
- **WASI 0.3 的工具鏈還很新**：Rust 的 `wasm32-wasip3` 在 kun 固定的 1.99 還是 tier 3（沒有預先編好的 std），要用 `RUSTC_BOOTSTRAP=1` 和 `-Zbuild-std` 自己編 std（「已決定的事項」第 40 條）；Go 還沒有，所以拿掉了。
- **wasm 文件不能用 JS**：包括頁面 JS 的 `WebAssembly` 和 wasm-bindgen 的膠水；同一個節點不能交給另一邊。要混用就只能寫成 JS 文件，用 V8 的 `WebAssembly`（「已決定的事項」第 26 條）。
- **回收的粒度是整個 Store**；guest 要自己 drop 用完的 handle，同一個節點每次拿到的 handle 都不同（「Store 和 GC」）。
- **跟著 wasmtime 和 Component Model 升級**：wasmtime 每個月一個大版本，Component Model 1.0 還沒定；`dom_wasm` 用到 Component Model 的 async 和 wasmtime 的內部行為（例如 `YIELD` 時的 `run_concurrent`，第 41 條），升級時要重新確認。
- **沒有隔離**：V8 沒開 sandbox，renderer 也還不是獨立的 process（`details.md` 的「安全和資源」）。

## 理由和其他做法

**為什麼是 wasmtime 和 WIT**：見「動機」。

**V8 版本的長處**（`wasm` 分支，2026-10-04 換掉）：一個 unified heap，編成 wasm GC 的語言經過它們自己物件的循環也收得掉；`externref` 就是 JS 的 wrapper，身分不用另外處理；Liftoff 啟動快；JS 和 wasm 在同一份文件裡共用 DOM，只有一個 engine，沒有交錯的問題。V8 的 wasm runtime 和 2026-10-01 的實測見 `details.md` 的「V8 的 wasm runtime」。

**考慮過、沒有選的做法**（完整的分析見 `details.md` 的「考慮過、沒有選的做法」）：

| 做法 | 為什麼不選 |
| --- | --- |
| wasm 跑在 V8 上（`wasm` 分支） | 做得出來，也做完了；換掉的理由見「動機」 |
| core module：照 canonical ABI 攤平、kun 自己實作同步的 ABI，WASI preview1（`wasm` 分支 W2 的做法） | 同步重新進入和 fiber 上的等待都在 kun 手上，trap 之後也還能呼叫；但要自己維護 canonical ABI 和 WASI，core module 的命名照 wit-bindgen 的慣例、規格沒寫，也接不上 WASI 0.3。留作備案：如果 `dispatchEvent` 的做法（「已決定的事項」第 17 條）在 wasmtime 上走不通 |
| 同時保留 `kun:dom/v1`（`externref`） | 兩種介面要一起維護；`externref` 讓 cppgc 和 wasmtime 之間有 GC 參照，要 fork wasmtime 的 collector 才收得乾淨；Winch 跑不了 |
| wasmtime，沒有 holder | 兩個 GC 互相看不到，跨 heap 的循環要等頁面離開才收得掉 |
| 同一份文件裡 JS 和 wasm 共用一個 DOM（2026-10-04 的設計） | wasm A → JS → `el.click()` → A 的 listener 這種交錯，要用裸指標重新進入正在更外層被借用的 Store，加上一份 aliasing 的論證；V8 的 binding 和 wasm 的 host 函式碰同一個 `DomState`，`DomAccess` 這種編譯時的保證做不到；wasm 的 DOM 也得在 V8 的 heap 上。2026-10-05 改成第 26 條 |
| 兩個 heap 分開，但互相 trace | 等於在 wasmtime 上重做 V8 的 unified heap，wasmtime 也沒有給 embedder 用的 trace 介面 |

**不做「讓 web-sys 不經過 JS」的相容層**：要在 kun 裡用 Rust 實作 wasm-bindgen 期待的那些 import，但那些 import 的名稱和參數是 wasm-bindgen 的 macro 和命令列工具之間的內部約定，沒有公開保證，名稱依每次 build 產生，版本一換就可能對不上。

## 先例

- **Blink**：DOM 放在 cppgc（Oilpan）上，和 V8 的 heap 組成 unified heap，JS 和 DOM 之間的循環一起回收。這份 RFC 的 JS 那一側照舊。
- **wasm-bindgen / web-sys、Emscripten、Kotlin/Wasm**：wasm 經過產生的 JS 膠水呼叫 DOM；kun 的中間那層是 Rust 寫的 host 函式。
- **wasmtime 的 embedder**：host 的狀態放在 `Store` 的 data，host 函式從那裡拿；WASI 用 wasmtime-wasi。
- **Component Model 和 WIT**：介面寫成 WIT，各語言的 binding 由 wit-bindgen 產生。
- **瀏覽器裡的 WASI**：browser_wasi_shim、wasmer-js 用 JS 實作 WASI；kun 的 host 是 Rust、不經過 JS，並加上 DOM。

## 已決定的事項

每條只寫決定和主要的理由；完整的理由（不選的做法和原因、原型和量測、實作時的補充）見 `details.md` 的「已決定的事項的完整理由」，編號相同。

2026-10-01 確認：

1. ~~**wasm 跑在 V8 上**，和 JS 共用一個 isolate、一個 heap、一條 event loop。~~ 2026-10-04 被第 8 條取代。
2. **wasm script 照 module script 的規則執行**：不擋住 parser，parse 完照順序執行。
3. **wasm 的 DOM 介面用回傳值回報錯誤**（WIT 的 `result<_, dom-exception>`）：各語言對 wasm exception handling 的支援不一。
4. **兩個前端從 `dom_api` 的同一張宣告表產生**（P3 做的，`P3.md` 的「已決定的事項」第 8 條）。
5. ~~**作者用從宣告表產生的 guest SDK**。~~ 2026-10-04 被第 10 條取代；不做「讓 web-sys 不經過 JS 執行」的相容層照舊（「理由和其他做法」）。
6. **fork wasmtime、用 cppgc 當它的 collector** 是之後的方向：2026-10-04 成為第 13 條，2026-10-05 移到 RFC 0005（第 54 條）。
7. ~~**`kun:dom/v1` 只用 wasmtime 也能實作的東西**。~~ 2026-10-04 起不做 `kun:dom/v1`（第 10 條）。

2026-10-04 確認：

8. **wasm 跑在 wasmtime 上，JS 照舊跑在 V8，DOM 在 cppgc 上**（理由見「動機」）。2026-10-05 起 wasm 文件的 DOM 在單獨的 `cppgc::Heap` 上，不接 isolate（第 26 條）。
9. **從 main 重新實作**：V8 版本留在 `wasm` 分支當對照組，和 engine 無關的部分從那裡帶過來。
10. **DOM 介面只有 WIT（`kun:dom`），加上 WASI**；binding 由 wit-bindgen 產生，不做 `kun:dom/v1`（理由見「動機」）。
11. **guest 是 component，WASI 用 0.3**：wasmtime 和工具鏈都往這裡走；WASI 0.2 和 preview1 不承諾。
12. **驗收的語言是 Rust、C、Lua**：Go 還沒有 WASI 0.3，拿掉（「之後的可能」）。
13. ~~**最終目標：cppgc 當 wasmtime 的 GC**（第二層）。~~ 2026-10-05 被第 54 條取代：移到 RFC 0005。
14. **wasm 的路徑不依賴 V8**：只有 wasm 的頁面不建 isolate。這份 RFC 做到「不建 isolate」（第 51 條）；不連結 V8 另開一份 RFC。
15. **cppgc 當 wasmtime 的 GC 時接受 sandbox 變弱**，安全程度和沒開 sandbox 的 V8 一樣。2026-10-05 起是 RFC 0005 的事。
16. **wasm 文件的 DOM 放在單獨使用的 `cppgc::Heap` 上，GC 由 cppgc 自己排成 renderer 的 non-nestable task**：GC 只發生在 task 之間，最長停頓不到 2 ms（Q18）；不接 isolate 的 `CppHeap` 完全不會自己收。
17. **會呼叫回 guest 的 DOM 函式（`dispatchEvent`）宣告成 `async func`、當場完成**，guest 的 `callback` export 是 sync 的：async Store 裡 sync 的 import 不能呼叫回 guest（Q2）。
18. **wasm 自己分派時它的 listener trap，那次分派剩下的 listener 跳過**，記成限制：Component Model 規定 trap 一路傳到最上層（Q3）。
19. **給 script 的時間降到 100 µs，宣告表加上 `performance.now()` 和 `Element.localName`**：kun 沒有 cross-origin isolation，HR-Time 的下限是 100 µs。
20. **`dom_api::wit` 只產生 WIT，canonical ABI 由 wasmtime 做**（2026-10-05）。實作期間是 `kun:dom@0.1.0`（`dispatchEvent` 改成 async 本身就不相容）；2026-10-05 RFC 做完時定為 1.0.0，之後只新增函式。2026-10-05 RFC 0008 改回 0.1.0（它的「已決定的事項」第 1 條）：這份 RFC 之後的 DOM API 擴充要改既有的函式和字典，還不想穩定介面。
21. **guest 的入口 `start(window)` 是 `async func`**（2026-10-05）：sync 的 export 在回傳之前不能等待，整個程式就用不了 WASI 的等待。
22. **holder 放在 `dom` 裡、和 engine 無關**（2026-10-05）：`dom` 不依賴 wasmtime，之後接其他 engine 也能用。Store 放在 Store 表（第 29 條）。
23. ~~**第 3 步只支援 JS 和 wasm 經過 DOM 的最外層互相碰到**。~~ 2026-10-05 被第 26 條取代。
24. **`kun:dom` 的 host 函式在執行時從宣告表產生**（2026-10-05）：每個 WIT 函式用 `func_new` 接上同一個共用的 host 函式，參數用 `Val` 轉換，比替每個函式產生有型別的 `func_wrap` 簡單；成本見第 50 條。
25. **shell 裝 V8 和 wasmtime 兩個 engine**（2026-10-05），renderer 照文件的種類選一個（第 26、27 條）。
26. **一份文件只用一種 script，JS 和 wasm 的 DOM 分成兩套、不互通**（2026-10-05）：同一份文件裡兩個 engine 交錯，要用裸指標重新進入正在被借用的 Store；分開之後 host 函式有編譯時的保證（第 28 條），wasm 文件也完全不經過 V8。代價是 wasm 文件不能用 JS。取代第 23 條和原本的第 5 步。
27. **wasm 文件用 `<meta>` 宣告**（2026-10-05），renderer 在 parse 之前找（第 36 條），換頁時種類不同就換 `DomIsolate` 和 engine：分頁可以各自是一般網頁或 wasm 的頁面，也不會因為夾了一個 wasm script 就突然不跑 JS。
28. **wasm 的 host 函式透過唯一的 `DomAccess` 開 DOM scope**（2026-10-05）：同時最多一個 DOM scope，由借用規則在編譯時保證；`DomAccess` 是 `Send`（wasmtime 要求 Store 的 data 是 `Send`），用的時候檢查 thread。
29. **Store 放在 `DomState` 一張不在 heap 上的 Store 表裡**（2026-10-05），event loop 的最外層 prune：cppgc 的 finalizer 不能 drop Store。
30. **`ScriptCallback`、`CallbackSlot` 是 enum：V8 的 function，或 holder 加上 guest 的 id**（2026-10-05），放錯種類時 panic：只有 engine 的 bug 會放錯，不值得用型別參數改動整個 DOM。
31. **slot 表在 `dom` 裡用 Rust 寫**（2026-10-05）：64 格一個 chunk 的 GC 物件，每格三條 `Edge`；不改 fork 的 C++，測試、canary 和對照模型都用得上。
32. **Store 在執行中、`start` 還沒回傳、或還有等待中的東西時，由 root 撐住**（2026-10-05）：和 JS 等待中的 timer 撐住 callback 一樣；整個文件期間都撐住的話，循環收不掉。
33. **wasm 自己的工作（被叫醒的 Store、等待的到期時間）放在 engine 自己的佇列**（`dom` 的 `TaskQueue`，2026-10-05）：`ScriptEngine::next_task_due` 和 `run_tasks`（跑了 script 就請求 main frame）；WASI 的等待不借用 DOM 的 timer。
34. **分派演算法改成 async，guest 的 `dispatchEvent` 碰到同一個 Store 的 listener 時讓出給 Store 的 event loop**（2026-10-05）：只有分派那條路是 async，規範的語意只有一份；分派的 future 用 `dom` 的 `ThreadBound` 包起來交給 wasmtime。
35. **reftest W7 在 3e 做**（2026-10-05）：reftest 經過 renderer，而 renderer 照文件選 DOM 和 engine 是 3e 的事。
36. **宣告是 `<meta name="script" content="wasm">`，和編碼一樣在前 1024 byte 預先掃描**（2026-10-05）：parse 之前要看的 `<meta>` 用同一套規則找，不用多 tokenize 一次。
37. **renderer 一次只有一套 DOM 和 engine**（2026-10-05），種類變了才換：一次只顯示一份文件，不必一直多佔一個 isolate 或一份 wasmtime。
38. **wasm 的示範是另一頁 `about:wasm-demo`**（2026-10-05）：wasm 不能放進 JS 文件；guest 用 `data:` URL 放進頁面。
39. **reftest W1–W6 照原本的用意用 WIT 的 guest 重寫，W2、W4 重新定義**（2026-10-05）：原本測的 JS 和 wasm 的互動不存在了；W1–W5 用 C，W6 用 Rust。
40. **Rust 的 guest 用 kun 固定的 1.99 加上 `-Zbuild-std` 編成 `wasm32-wasip3`**（2026-10-05）：`RUSTC_BOOTSTRAP=1`，連結 wasi-sdk 的 libc，放在 `sdk/rust` 自己的 workspace。有 std 的 I/O、不需要 nightly；`wasm32-wasip3` 升到 tier 2 之後拿掉這些旗標就好。
41. **Store 的 event loop 被 guest 的 `YIELD` 叫醒時，繼續 poll 同一個 `run_concurrent`**（2026-10-05）：只 poll 一次就丟掉 future 時，wasmtime 已經取出的工作會不見，guest 再也不會被叫醒。一直 yield 的 guest 會卡住 renderer，和 JS 的無限迴圈一樣。給 wasmtime 的回報是 `wasmtime-yield-report.md`（草稿，還沒送出）。
42. **guest 的 `start` 第一次停下來就算執行完了**（2026-10-05），後面的 script、`DOMContentLoaded`、`load` 不等它：和 module script 的 top-level await 一樣；不然一直在等的 `start` 會讓 `DOMContentLoaded` 永遠不觸發。
43. **reftest W4 用 `load` 代替使用者的點擊**（2026-10-05）：reftest 沒有 input。
44. **reftest W3 涵蓋八種失敗**（2026-10-05）：`wasm` 分支的六種，加上 core module、沒有 export `kun:dom/guest` 的 component。
45. **Lua binding 的產生器（`sdk/lua/generator`，自己的 workspace）只讀 `kun:dom` 的 WIT**（2026-10-05）：WIT 加上 `type callback = u32`，Lua 的 runtime 帶著 WIT 的 hash 讓 CI 檢查（原本寫在放進 repo 的 `kun_dom.c`，第 56 條）。驗證了「介面只有 WIT」，任何人只拿 WIT 就寫得出 binding。
46. **Lua 這次不能等待**（2026-10-05）：只停一個 coroutine 的等待要另外設計（「之後的可能」）。
47. **`about:wasm-demo` 加上 Lua 的 script**（2026-10-05）：視窗裡看得到 Lua；代價是執行檔多數百 KB。
48. **第 6 步只量這個分支、和已經記下的數字比，量測的程式是暫時的**（2026-10-05）：數字和量法在 `progress.md` 的「量測」。
49. **啟動的編譯成本先不處理，記成已知的限制**（2026-10-05）：Lua 的 runtime 每次載入都要編 45 ms；做法記在「之後的可能」。
50. **每次 DOM 呼叫的 host 成本先 profile、只修便宜的地方**（2026-10-05）：結果沒有單一、便宜的熱點，程式不改（`progress.md`）。要省得多，得改成有型別的 `func_wrap`，或讓 `dom_api` 收借用的參照（「之後的可能」）。
51. **不跑 V8 的設定用執行時的選項 `kun --script wasm` 打開**（2026-10-05）：每份文件都是 wasm 文件；同一個執行檔兩種設定都能跑、都能測。
52. **`--script wasm` 照舊完整初始化 V8，只是不建 isolate**（2026-10-05）：成本大多在 isolate 和 context，`bindings` 不必多一條初始化的路。
53. **`--script wasm` 的預設首頁是 `about:wasm-demo`**（2026-10-05）：`about:demo` 的 JS 在這個設定裡不執行，看起來像壞了。
54. **第二層（cppgc 當 wasmtime 的 GC）移到 RFC 0005（延後），這份 RFC 做到第一層就完成**（2026-10-05）：用 WIT 的 handle 時，第二層不會讓跨 wasm 和 DOM 的回收變細；現在也沒有用 wasm GC 的 guest，卻要立刻開始維護 wasmtime 的 fork。
55. **wasm 的前端 crate 叫 `dom_wasm`**（2026-10-05，原本叫 `dom_wasmtime`）：用 script 的種類命名，和 JS 的前端（當時叫 `dom_bindings`，2026-10-05 改名 `dom_js`）對得起來；`wasm` 分支（V8 版本，有同名的 crate）只留作參考。wasmtime 照樣只在這個 crate（`sanity.py deps`）。
56. **Lua 的 binding 不放進 repo，WIT 的 hash 放進編好的 runtime**（2026-10-06，RFC 做完之後）：`kun_dom.c` 執行和測試都用不到，`ci.py guests` 每次都重新產生它，產生在 `target/guests/lua`；檢查改成讀 `resources/kun-lua.wasm` 的 custom section `kun_dom_wit`，看的是實際載入的檔案。改了第 45 條。

## 待決定

無。實作期間的三題：GC 的做法由第 29 條決定；兩個 engine 交錯因為第 26 條不需要了；guest 的 `dispatchEvent` 碰到同一個 Store 的 listener 時怎麼讓出由第 34 條決定。第二層移到 RFC 0005（第 54 條）。

## 之後的可能

- **cppgc 當 wasmtime 的 GC**：RFC 0005（延後）。
- **DOM 參照也變成 GC 參照**：等 Component Model 的「GC 版 canonical ABI」（字串和 list 是 GC 陣列、resource 是參照）定案，或 kun 自己定一種 GC 形式的 `kun:dom`。RFC 0005 做完之後，resource 可以直接是指向節點的 cppgc 指標，回收的粒度和 V8 的 unified heap 一樣細，不再以 Store 為單位。
- **不連結 V8 的 build**：另一份 RFC（「不跑 V8 的設定」第 2 步）。
- **module 自己帶的唯讀檔案**、**Python**：`wasm` 分支 W2 的設計，需要 stdlib 的程式都卡在這裡。
- **Go**：有 WASI 0.3 的 target 之後加回驗收的語言（`wasm` 分支的 W8 可以參考）。
- **Lua 的等待**：只停一個 coroutine 的 async 等待（WASI 的時鐘），像 JS 的 `await`（第 46 條）。
- **效能**：啟動（第 49 條）：記憶體裡的 code cache 加上整個 process 共用一個 wasmtime `Engine`（不用 `unsafe`，重新整理、上一頁、共用同一個 Lua runtime 的頁面都快；JS 和 wasm 文件切換時也不用重建 engine 和 linker），磁碟上的 code cache（`Component::serialize`，重開之後也快，但 `deserialize` 是 `unsafe`，要開放 `dom`、`bindings` 以外的 `unsafe` 和處理 cache 的失效），Winch（編得快 3–4 倍，執行比較慢，wasmtime 沒有 tier-up）。每次呼叫（第 50 條的 profile）：每個 WIT 函式產生有型別的 `func_wrap`，省掉 `Val` 的轉換（第 24 條）；`dom_api` 收借用的參照（`NodeRef` 這類），不必替每個參數建立、再釋放 root。用一個真實的小應用比較 JS、V8 版本和這個版本。
- **handle 的改善**：`callback-dropped`（DOM 不再參照某個 callback 時通知 guest）、`is-same-node`、slot 去重、有型別的 host 函式，都在標準的 Component Model 裡做得到，是 linear memory 的語言長期的做法；問題和取捨見 `details.md` 的「handle 的問題和改善的方向」，要做時另開 RFC。
- **接其他 engine**：照 `details.md` 的「接其他 engine」。
- **只 export `wasi:cli/run` 的 command component**：原本打算照 command 執行（`main` 跑完就結束），沒有做；現在 guest 一定要 export `kun:dom/guest`，否則回報、跳過（reftest W3）。
- **資源和隔離**：Store 的 memory 上限、用 epoch 中斷失控的 wasm（`docs/limitations.md`）。
