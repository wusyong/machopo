# dom_wasm

DOM（`dom`、`dom_api`）給 wasm guest 用，在 wasmtime 上：`machopo:dom` 的 WIT 和 host 函式，每個 Store 對節點的 handle，wasm 的 GC 物件跟節點共用的 heap，guest 的 WASI，和頁面的 engine（`WasmEngine`）：renderer 替宣告成 wasm 的文件（`<meta name="script" content="wasm">`）用它。最接近的 Blink 是 `bindings/core/v8`（engine 那一側的 DOM interface），wasm 本身 Blink 沒有對應（Chrome 的 wasm 經過 JS 碰 DOM）。

不寫 `unsafe`（`#![forbid(unsafe_code)]`）。

## 用法

```rust
dom_wasm::configure(&mut config);               // engine：wasm 的 GC，物件放在 DOM 的 heap
let dom = WasmDom::new();                       // 這個 thread 的 DOM（沒有 JS engine 驅動，elise 自己的 GC）
let mut set = dom.store_set::<Data>(&engine)?;  // 用它的 heap 的 Store 都在這裡
dom_wasm::add_to_linker(&mut linker, |data: &Data| &data.dom)?;  // machopo:dom 的函式
let data = Data { dom: WasmStore::new(), .. };  // 每個 Store 一個，放在它的 data
let index = set.insert(Store::new(&engine, data))?;
set.with(index, |mut cx| run.call(&mut cx, args))?;  // 呼叫都經過 set 借出
```

- **一個 thread 一份 DOM**（`WasmDom`）：它的狀態（DOM 和每個 Store 的 handle 表）是 thread-local，`WasmDom` 建的時候裝上、drop 時拿掉。不放在 Store 的 data：async 的 Store 的 data 要是 `Send`，DOM 的 heap 是那個 thread 的，host 函式是 linker 的。
- **engine 擁有的 DOM**（頁面的）：頁面自己的工作（style、layout、parser）要 `&mut Dom`，所以 engine 用 `WasmDom::take` 把 DOM 拿走、自己留著，跑 guest 的一輪時用 `WasmDom::lend(&mut dom, || ...)` 借回 thread-local，結束時（unwind 也是）交還；`lend_page(&mut dom, &layout, || ...)` 連頁面的 layout 一起借（engine 的 `LayoutCell` 的內容搬過來、結束時搬回去）：guest 的操作和 JS 文件的一樣帶著 layout，要看排版的成員（`focus()` 要元素有被畫出來）才有東西可以問。DOM 不在時 guest 的呼叫回錯誤（Store 照樣能用）、建不了 Store；Store 和它們的 `StoreSet` 要在借出期間建和丟：不在時丟掉的 Store 留下它的表（跟著 `WasmDom` 走），有 GC heap 的還留下 DOM 的 heap 的一個 root。`WasmDom` 要比 DOM 先 drop（表的 holder 是 heap 的 root）。DOM 在 engine 手上時 heap 的安全點照常 GC，`StoreSet` 交出 Store 的 root。不 `take` 的話 DOM 一直在 thread-local（這個 crate 其他的測試）。
- **每個 Store 一個 `WasmStore`**：它在 DOM 裡的編號（不重用）和那份 DOM 的序號，drop 時拿掉它的表。`add_to_linker` 收一個從 Store 的 data 找到它的函式。Store 比它的 DOM 活得久時（engine 先丟掉），它的呼叫 trap，drop 時也不碰同一個 thread 之後的 DOM：每份 DOM 有自己的序號，對不上就當它不在了。
- **wasm 的 GC 物件在 DOM 的 heap 裡**：`configure` 讓 engine 用 elise、Store 借這個 thread 的 DOM 的 heap（wasmtime 的 fork 的 `EliseHeapProvider`：從 thread-local 借 `Dom`，鑰匙是 DOM 的序號；DOM 正被借著時、不是這份 DOM 時回錯誤）。這樣的 Store 都要在 `WasmDom::store_set` 建的 `StoreSet` 裡（不在的拿不到 GC heap）：set 擁有它們、交出它們的 root、借出時算 heap 的 frame，記憶體安全靠 fork 和 elise，不靠這個 crate（fork 的 `README.machopo.md` 的「A heap borrowed from the embedder」）。一份 DOM 一個 set；`WasmDom` 比 set 先走時，set 之後的呼叫都回錯誤。
- **GC 在 host 呼叫裡經過 Store 做**：Store 借出期間 heap 在 frame 裡，host 函式的 DOM 操作的安全點不 GC（看不到 wasm 的 stack）。會配置的 host 函式（成員、`create-document`）做完之後 heap 該 GC 時呼叫 `StoreContextMut::gc`：wasmtime 交這個 Store 的 stack 上的 root，set 交其他 Store 的，DOM 的物件一起收；別的 Store 也借出中時不做。沒有 GC heap 的 Store（C 的 guest）的 GC 由 fork 直接請 heap 做。呼叫之間（沒有 Store 借出時）DOM 的安全點照常 GC。
- host 函式是同步的 `func_wrap`，同步和 async 的 Store 都能用。

## 頁面的 engine（`WasmEngine`）

`dom_api::ScriptEngine` 的實作（`src/engine.rs`），wasm 文件用的；對應 `dom_js::V8Engine`。renderer 照文件選它（`shell` 把 `WasmEngine::boxed` 裝成 `ScriptEngineFactory` 的 `wasm_engine`）。

- **DOM 是它的**（上面的「engine 擁有的 DOM」）：`dom_mut()`、`dom_and_layout()` 直接給；跑 guest 的每一輪、建和丟 Store 時借進 thread-local。
- **沒有 JS**：不建 isolate。`run_classic_script`、module、`execute_webdriver_script`、DevTools 的 inspector、observer、idle callback、`requestAnimationFrame` 的方法什麼都不做（WebDriver 的 script 回錯誤）。event handler 的屬性（`onclick=""`）是 JS，不執行。
- **DOM 自己的工作照樣經過 `dom_api`**：輸入的預設動作（`handle_mouse`、`handle_key`、`handle_wheel`）、focus、文件生命週期的事件、元素的 task、CSS 的動畫、WebDriver 的查詢和輸入，都和 V8 那邊呼叫同樣的函式，host 是 engine 的 `ScriptHost`：分派走到 guest 的 listener 時呼叫它（下一條），timer、observer 這些 script 的 callback 什麼都不做。microtask checkpoint 只有 DOM 自己的（圖片的更新、元素改變之後的步驟），結束時把 guest 的 listener 印的東西送到 console。
- **guest 的 listener 經過它的 export `handle-event(callback, event)` 呼叫**：`event-target-add-event-listener(this, type, callback, options)` 的 `callback` 是 guest 自己編的 `u32`、`options` 是 `capture`；host 把「哪個 Store（它在 DOM 裡的編號加一）、哪個編號」存在 listener 的 callback edge 的 embedder word 裡（`listener_word`），`remove` 和重複的 `add` 比對這個 word。呼叫有兩條路，都是同步的、在分派裡面：
  - **頁面的分派**（輸入、`fire_event`、文件的生命週期）：`Host::call_listener` 照 word 找到那個 script 的 Store，把 DOM 和 layout 借出去（`lend_page`），在 `StoreSet::with` 裡 poll `handle-event` 的 `call_async` 到完成。它等待時（export 是同步的函式，不該等）當成 trap。
  - **guest 自己的分派**（它呼叫 `click()`、`focus()`、`blur()`，host 函式是 `front::add_scripted` 加的）：`WasmFront::call_listener` 在 guest 的那次呼叫的 stack 上直接 `call` 它的 `handle-event`（fork 讓 set 的 Store 在 fiber 上接受同步的呼叫）。**這條路只呼叫同一個 Store 的 listener**，其他 script 的 listener 和 event handler 屬性的（JS）跳過。
  - 事件的 handle 只在那次呼叫裡是 guest 的：回來時 host 收回（guest 自己 `release` 過也可以），之後拿它呼叫成員會 trap。listener trap 時報到 console（`uncaught`、script 的 URL），那個 script 的 Store 結束、它的 listener 都不再被呼叫；在 guest 自己的分派裡 trap 時，同一次分派剩下的 listener 不跑，trap 往上經過 guest 的那次呼叫。
  - host 不告訴 guest 哪個 callback 不用了：guest 自己的「編號對到什麼」的表只會長。
- **只有最上層的 frame**：`create_window` 對其他 frame 回 `None`，iframe 沒有子文件。DOM 的工作要 host 做的事只轉導覽（連結、表單、fragment：`take_frame_requests` 的 `Navigation`）。
- **一個 script 一個 Store**（`run_wasm_script`，bytes 是 component，world 是 `wit/script.wit` 的 `script`）：在呼叫的 thread 上同步編譯、建 Store、實例化、呼叫 `start(document)`（頁面的 document 在這個 Store 的 handle）。`start` 第一次等待時這個 script 對頁面就算執行完了，Store 留著。編不過、import 對不上、trap 都報到 console（`uncaught`，URL 是 script 的），trap 的 Store 丟掉；之後的 script 照常跑。新的 window（換頁）時前一份文件的 Store 全部丟掉。
- **guest 的等待是 engine 的 timer**：`until_next_timer()` 是 DOM 自己的下一個 task 和各個 Store 最早的期限裡比較早的（renderer 的 loop 照它醒來），`run_due_tasks()`（和 `run_timers()`）讓期限到了的 Store 各跑一輪。期限在每一輪結束時從 Store 讀出來記著（時鐘在 Store 裡，只有一輪裡拿得到）。
- **guest 印的東西**在每一輪結束時一行一行交給 console 的 sink（不是 `uncaught`）。
- `WasmEngine::with_clock` 給測試用：guest 的時鐘讀測試給的時間。

## WASI 和 Store 的一輪

guest 是 WASI 0.3 的 component（C 的 libc 啟動時就 import 它），Store 是 async 的。

```rust
dom_wasm::add_wasi_to_linker(&mut linker)?;      // Data 要實作 WasiView 和 GuestClockView
let wasi = WasmWasi::new(now, sink);             // 每個 Store 一個：machopo 的時間（奈秒）、輸出要去的地方
// 一輪：到期的等待叫醒，poll 到不動為止，future 丟掉
set.with(index, |mut cx| {
    cx.data_mut().guest_clock().wake_due();
    let turn = pin!(cx.run_concurrent(async |accessor| start.call_concurrent(accessor, args).await));
    poll_until_idle(turn)
})?;
```

- **用 wasmtime-wasi 的 p3，除了 monotonic clock**（`src/wasi.rs`）：wasmtime-wasi 的等待用 tokio 的 timer，而跑 guest 的 thread 沒有 tokio 的 runtime。這裡的 `wait-for`、`wait-until` 只在 Store 的 `GuestClock` 記一個期限；embedder 讀最早的期限（`next_deadline`）、到時候再跑這個 Store 一輪，那一輪開始前呼叫 `wake_due`。期限用 embedder 給的時間，不是 DOM 的 timer。
- **guest 讀到的時鐘降到 100 µs**（`CLOCK_RESOLUTION`，和 `performance.now()` 一樣不給更細的）；等待的期限照原本的時間算。
- **stdout、stderr 一行一行交給 sink**（不含換行），寫的當下就給，不等待；最後沒有換行的一行在輸出關閉時（Store 丟掉）才給：WASI 的 stream 沒有 flush。沒有檔案、socket、引數和環境變數（interface 接上了，guest 拿不到東西）。
- **一輪**（`poll_until_idle`，`src/turn.rs`）：Store 只在有事要做時借出來，`run_concurrent` 的 future poll 到做完、或 pending 而且上一次 poll 沒有叫醒 waker 為止，然後丟掉。guest 的 task 和 host 函式的 future 留在 Store 裡，下一輪接著跑。這一輪的 waker 跟著丟掉，所以 guest 等的東西到了要由 embedder 自己排下一輪；`GuestClock` 留著的是 wasmtime 給 host 函式的 future 的 waker（叫醒它，下一輪才會 poll 那個 future）。
- **guest 只能在 async 的 export 裡等待**：不是 async 的 export 回傳之前等待會 trap（Component Model 的規定）。
- 等待被取消（task 丟掉）時，它的期限留到時間到才拿掉：多一輪沒事做的。

## guest 怎麼指 DOM 的物件（handle）

guest 拿到的物件（節點、`Event`、`Window` 這些）是一個 `u32`，那個 Store 自己的編號（`src/handles.rs`）：

- **一個物件在一個 Store 裡一個編號**：交出時先找它在這個 Store 有沒有編號，有就給同一個，沒有才配一格。guest 直接比整數就知道是不是同一個物件；只有一種 handle，往上、往下轉型不用呼叫 host。guest 不要時呼叫 `release`（重複拿到不另外計數，guest 的 binding 自己數）。對應 Blink 每個物件在每個 world 最多一個 wrapper，一個 Store 是一個 world。
- **編號**：低 24 位元是 Store 的表的 index，高 8 位元是 generation（1 到 255，放掉時加一），0 永遠不是 handle。用了放掉的、沒給過的編號，呼叫 trap（只有 binding 的 bug 會這樣，和 wasm 拿 table 的 index 超出範圍一樣）。
- **表**：物件是 Store 的 holder（DOM 的 heap 上的物件，每塊 256 個 `Member<Erased>`）的 edge，不是 root：只有 guest 拿著的物件活到 Store 死掉為止，跟著它走。每一格的 generation 和物件的 **interface**（它的 wrapper 會是哪一個：`dom_api` 的 `interface_of`，元素的照它的名稱，例如 `<input>` 是 `HTMLInputElement`；交出時算一次）在 heap 外，呼叫前不碰 DOM 就能檢查 handle 和 `this`：`this` 的 interface 要是成員的 interface 或它的子孫（`dom_api::typed::is_interface`）。對應 Blink 的 wrapper 帶著 `WrapperTypeInfo`。元素的 interface 記下來之後不變（custom element 升級會改它，wasm 文件沒有）。
- **找一個物件在一個 Store 的編號**：elise 的 heap 的物件在 nursery 裡會搬，不能用位址當 key。這份 DOM 的第一個 Store 是 main（Blink 的 main world），它的編號在物件的 engine word 的低 32 位元（`Cx::object_engine_word`：JS 文件的物件放 wrapper 的那 8 bytes，每種物件都有，沒有 JS engine 驅動的 heap 裡沒人用）；其他 Store 的編號在一張共用的表，高 32 位元是它的 index，每一項是幾個（Store，編號）。main Store 死掉之後不再有 main（換給別的 Store 時，它原本記在共用的表的編號會漏看）。
- **word 可能是過時的**：放掉一個 handle 時手上只有那一格，不知道物件的型別，所以不去改物件的 word。word 講的東西要對過才算數：main 的編號對回 Store 的表（那一格拿的是這個物件），共用的表的那一項對它裡面的一個編號（拿的是這個物件；一項沒有 Store 之後會給別的物件用）。每一格記著它在共用的表的哪一項，放掉時不用物件就找得到。共用的表的每一項只在有 Store 拿著那個物件時存在：release 或 Store 死掉時拿掉。word 和表存的都是數字，不撐住任何東西。

## 一次呼叫

每個成員一個有型別的 host 函式（`src/front.rs`），從 `dom_api::for_each_member!` 展開：參數和結果是成員自己的型別（`TypedMember::Args`、`Ret`），wasmtime 直接 lift、lower，沒有 `Val`。

1. 操作之外，不碰 DOM：`this` 和每個節點參數是這個 Store 的 handle（不是就 trap）；`this` 是成員的 interface 或它的子孫的（照表裡記的 interface；不是就是 `TypeError`，像 JS 的 receiver 檢查）；節點的參數要是節點的 handle（不是就 trap）；字串參數在 guest 的記憶體裡讀（UTF-8，不複製），檢查過（不是 UTF-8 就 trap）。
2. 成員的專用路徑（`TypedMember::call`，front 是 `WasmFront`）：在操作裡從 Store 的表拿節點，結果記下來；節點的結果在操作裡給 handle（表的 edge 在 DOM 的 heap 上）。
3. wasmtime lower 結果：字串先複製成一個 `Box<str>`（wasmtime 不 lower `Rc<str>`，`DomString` 是 thread 自己的），wasmtime 再複製進 guest。

錯誤是 `result<_, dom-exception>`，`code` 是 `exception_code` 的編號（`TYPE_ERROR` 是 1）。

## WIT

`wit/machopo-dom.wit`（`machopo:dom@0.2.0`），`src/wit.rs` 從宣告表產生（`wit::describe`）、和檔案比對：

- **成員是明列的清單**，照 guest 的需要加：每個成員是一個照它的型別編出來的 host 函式，全部都放的話編譯時間和執行檔會大很多。清單是兩份名稱：
  - `wit::FROZEN`：節點的 interface 上、只拿和給節點、字串、數字、布林的成員（`wit::is_plain`，`for_each_member!` 的 `plain`），2026-10-30 那些。
  - `wit::ADDED`：之後加的，規則比較寬（`wit::can_hold`）：`this`、參數、回傳值可以是任何 interface 的物件（元素自己的 interface、`Window`、`Location`、`DOMTokenList`、`CSSStyleDeclaration`、`HTMLCollection`、`NodeList`），不定個數的字串是 `list<string>`（`classList.add`），可省略的 dictionary 不放進函式（`focus(options?)` 只有 `this`，等於用預設值）。現在有：`HTMLInputElement` 的 `value`、`checked`；`HTMLElement` 的 `focus()`、`blur()`、`click()`、`style`；`Element` 的 `remove()`、`nextElementSibling`、`previousElementSibling`、`classList`、`children`、`querySelectorAll`；`Document` 的 `body`、`defaultView`、`querySelectorAll`；`DOMTokenList`、`CSSStyleDeclaration`、`HTMLCollection`、`NodeList` 的成員；`Window` 的 `document`、`location`；`Location` 的 `href`（只有讀）、`hash`。
  - listener 和事件的：`EventTarget` 的 `addEventListener`、`removeEventListener`（callback 是 `u32`，options 只有布林的 `capture`，`this` 的型別寫 `object`：`event-target` 這個函式名和 `EventTarget` 的型別別名撞名）；`Event` 的 `type`、`target`、`currentTarget`、`defaultPrevented`、`preventDefault()`、`stopPropagation()`；`UIEvent.detail`、`MouseEvent.button`、`KeyboardEvent.key`。
  - 建構子、static 的操作、拿 union、sequence、promise、`any` 的成員照舊不放；callback 只有 listener 的。
- 一種 handle `type object = u32`，`type node = object`，每個節點的 interface 一個 `node` 的別名（`type element = node;`），成員提到的其他 interface 一個 `object` 的別名（`type window = object;`）；只給 binding 的型別名稱。
- 每個成員只在宣告它的 interface 上一次，名稱是 `<interface>-<member>`（kebab-case），attribute 的 setter 是 `<interface>-set-<attribute>`，第一個參數是 `this`；每個都回傳 `result<T, dom-exception>`。常數不放（WIT 沒有常數）。
- `create-document` 建一份 HTML 文件，`release` 放掉一個物件。
- `wit/script.wit`：頁面的 script 的 world（上面的「頁面的 engine」）。

**加一個成員**：

1. 名稱加進 `wit::ADDED`（`<interface>-<member>`；getter 和 setter 各一個）。
2. `src/lib.rs` 的 `member!` 加它的一行（`(Interface, member, $kind:ident, ...)`；只要某一種時把 `$kind` 寫成 `Getter` 這類）：只有有自己一行的成員、或 plain 的，才會編成 host 函式。漏了這一步，WIT 有它、linker 沒有，import 它的 guest 建不起來。
3. 它用到 `src/front.rs` 還沒有的型別時補上（參數是 `WitArg`，回傳值是 `WitRet`；不是節點的 interface 加進 `objects!`）。
4. `MACHOPO_UPDATE_GOLDEN=1 cargo test -p dom_wasm --test it wit` 重新寫 WIT，`python3 scripts/ci.py guests` 重編測試的 guest，替它在 guest 加一個 case。

表上加了新的節點 interface 時，`src/front.rs` 的 `interfaces!` 也要加一行（沒加的話用到它的成員編不過）。改了 WIT 的形狀（不是只加函式）時升 `wit::PACKAGE` 的版本，`wit/script.wit`、`tests/guests/wit/test.wit` 和 `scripts/ci.py` 的 `GUEST_WORLDS` 跟著改。

## 測試

```sh
cargo test -p dom_wasm
python3 scripts/ci.py guests          # 重編 tests/guests/ 的 guest（wasi-sdk 34.0、wit-bindgen 0.62.0）；每個 guest 的 world 在 ci.py 的 GUEST_WORLDS
```

測試不編 guest，只用 repo 裡的 `.wasm`。測試的 guest 是 C，編成 WASI 0.3 的 component（`wasm32-wasip3`）；測試的 Store 是 async 的（linker 是 wasmtime-wasi 的 p3），每次呼叫在 `StoreSet` 的一次借出裡 poll 到完成，world 在 `tests/guests/wit/test.wit`：import `machopo:dom` 的 interface，export `run`（照名稱跑一個 case，回傳哪裡錯了，沒錯是空字串）和 `sleep`（`async func`：等一段時間，醒來留一個記號給 `run` 的 case 讀）。

- `tests/it/engine.rs`（guest 是 `tests/guests/page.c`，world 是 `script`，照 document element 的 `data-case` 做事）：`start` 拿到頁面的 document、改它，每個 script 自己的 Store，之後 engine 的 full GC 留著改過的樹；等待的 script 在第一次等待時算執行完，期限是 `until_next_timer`，還沒到時 `run_timers` 不跑，到了才接著跑；trap 的 script 報到 console（`uncaught`、它的 URL）、之前做的留著、下一個 script 照常跑；不是 component 的 bytes 報到 console；guest 印的一行到 console；新的 window 丟掉前一份文件的 script（等待中的也是）；讓出（callback code 的 `YIELD`）的 guest 在同一輪裡接著跑完；guest 從 document 拿到頁面的 window（每次同一個、它的 document 是原本那個）和 location（URL、設 fragment），window 不是節點；`click()` 勾選一個 checkbox（`focus()`、`blur()` 要 layout，在 `shell` 的 `wasm_documents` 測）。listener（guest 的 `handle-event` 把「編號:事件的 type、target、currentTarget、滑鼠事件的 button 和 detail、鍵盤事件的 key」寫成一段，編號的 bit 決定它另外做什麼）：頁面分派的事件呼叫 capture 和 target 的 listener、重複加的只有一次、每次分派都呼叫；兩個 script 的 listener 各自在自己的 Store 裡被呼叫；guest 自己的 `click()` 在回來之前照順序呼叫它的 listener（capture、target、bubble），不呼叫別的 script 的；`preventDefault()` 擋住勾選；`stopPropagation()`；`remove` 要同樣的 callback 和 capture；listener 的 trap（兩條路各一個：Store 結束、其他 script 照常；`start` 跟著結束、回報的是原本的 trap）；listener 裡再 `click()`（頁面的分派裡巢狀的分派）和它印的一行；留著的事件 handle 下次用會 trap（兩條路各一個）。真的滑鼠和鍵盤輸入走到 listener 的是情境 `wasm-listeners`（頁面 `tests/scenarios/pages/wasm-listen.html`）。
- `tests/guests/todomvc.c`（world 是 `script`）：TodoMVC 的 guest，現在只把 todo 建出來、不聽事件（`handle-event` 是空的）（每個 todo 的 `<li>` 照 JS 版的樣板、計數的文字）。`dom_wasm` 自己的測試不跑它：頁面 `tests/scenarios/pages/todomvc-wasm.html` 用它，由情境 `todomvc-wasm` 和 `shell` 的 `wasm_documents` 測。
- `tests/it/lend.rs`：DOM 被 engine 拿走之後，engine 直接用它；guest 只有借出期間碰得到，不在時呼叫回錯誤、之後照樣能用；engine 手上的 full GC 留著 guest 拿著的節點和另一個 Store 的 GC 物件；unwind 的借出也交還；已經在的 DOM 不能再借進來；DOM 不在時丟掉的 Store 留下它的表。
- `tests/it/wasi.rs`：guest 在 async 的 export `sleep` 裡等待時留在 Store 裡、期限是 `next_deadline`，期限之前的一輪不叫醒它，到了之後的一輪才醒（時間到了但還沒跑那一輪時也還沒醒），等待期間照樣能呼叫它；不用等的等待在開始的那一輪就結束；兩個 Store 各自的等待；guest 讀到的時鐘和解析度是 100 µs；印的東西一行一行來、分 stdout 和 stderr，沒有換行的最後一行在 Store 丟掉時來。
- `tests/it/wit.rs`：`wit/machopo-dom.wit` 是表產生的那份；`for_each_member!` 的 `plain` 剛好是 `wit::is_plain` 的，跳過的是物件不是節點的那幾個 interface；WIT 的成員剛好是清單上的（`FROZEN` 和 `ADDED`），凍結的每個都還是 plain、加的每個 guest 都拿得了。
- `tests/it/guest.rs`（guest 是 `tests/guests/dom.c`，每個 case 一個新的 Store）：建樹、插入、走訪、移除，同一個節點每次同一個編號，element 的 handle 給 `Node` 的成員也行；字串兩個方向（屬性值的 ASCII 和非 ASCII、名稱、命名空間、text 的 data、`[LegacyNullToEmptyString]` 的 none 是空字串、`textContent`）；DOM 的錯照代碼（2、3、4、9），`this` 是別的 interface 的是代碼 1 的錯，不是 trap，Store 照樣能用；放掉的格子給下一個節點別的編號；guest 留著的節點經過 GC 還在、放掉之後 Store 沒拿著東西；放掉的、沒給過的、0、放兩次的 handle 和不是 UTF-8 的字串 trap；元素自己的 interface 的成員（input 的 `value`、`checked`，`style`）照 handle 的 interface 檢查 `this`，別的元素是 `TypeError`；不是節點的物件（class list、style 的宣告、`children`、`querySelectorAll` 的結果）一個物件一個編號，它們的成員（不定個數的字串、可省略的布林、index）；不是節點的 handle 當節點的參數 trap；兩個 Store 各自的編號，第二個（不是 main）的在共用的表、它死掉時拿掉，第一個死掉之後新的 Store 也走共用的表；比它的 DOM 活得久的 Store 的呼叫 trap，它 drop 時不碰同一個 thread 之後的 DOM（那份 DOM 的第一個 Store 和它同號）。
- 單元測試（`src/handles.rs`）：main Store 的編號每次一樣、在 engine word 的低 32 位元，放掉之後不是任何節點的（word 留著、不算數）、同一格的下一個編號不一樣；handle 能指任何物件（`Window` 和節點一樣每次同一個，main 和其他 Store 都是），記著它的 interface（`<input>` 是 `HTMLInputElement`、`<p>` 是 `HTMLParagraphElement`）；共用的表放掉的一項給了別的物件之後，原本那個物件過時的 word 不算數，兩個物件各自的編號都不變；其他 Store 的在共用的一項，最後一個 Store release 或死掉時拿掉；main 死掉之後新的 Store 不讀它留下的 word；handle 撐住節點經過 GC，放掉或 Store 死掉之後收掉；generation 從 255 繞回 1、超過一塊的節點。`src/wit.rs`：kebab-case。`src/wasi.rs`：最早的期限是最早的那個等待的，同一個等待再 poll 還是一個；時間到了、沒人叫醒就被 poll 的等待結束、不再留著；被叫醒但時間還沒到（時間倒退）的再留一次。`src/turn.rs`：叫醒自己的 future 接著 poll，沒叫醒的那次 poll 之後這一輪結束，下一輪從那裡接著。
