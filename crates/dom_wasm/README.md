# dom_wasm

DOM（`dom`、`dom_api`）給 wasm guest 用，在 wasmtime 上：`machopo:dom` 的 WIT 和 host 函式，每個 Store 對節點的 handle，wasm 的 GC 物件跟節點共用的 heap，guest 的 WASI，和頁面的 engine（`WasmEngine`）。renderer 還沒有接上它（machopo 沒有 wasm 的文件），只有它自己的測試用。最接近的 Blink 是 `bindings/core/v8`（engine 那一側的 DOM interface），wasm 本身 Blink 沒有對應（Chrome 的 wasm 經過 JS 碰 DOM）。

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
- **engine 擁有的 DOM**（頁面的）：頁面自己的工作（style、layout、parser）要 `&mut Dom`，所以 engine 用 `WasmDom::take` 把 DOM 拿走、自己留著，跑 guest 的一輪時用 `WasmDom::lend(&mut dom, || ...)` 借回 thread-local，結束時（unwind 也是）交還。DOM 不在時 guest 的呼叫回錯誤（Store 照樣能用）、建不了 Store；Store 和它們的 `StoreSet` 要在借出期間建和丟：不在時丟掉的 Store 留下它的表（跟著 `WasmDom` 走），有 GC heap 的還留下 DOM 的 heap 的一個 root。`WasmDom` 要比 DOM 先 drop（表的 holder 是 heap 的 root）。DOM 在 engine 手上時 heap 的安全點照常 GC，`StoreSet` 交出 Store 的 root。不 `take` 的話 DOM 一直在 thread-local（這個 crate 其他的測試）。
- **每個 Store 一個 `WasmStore`**：它在 DOM 裡的編號（不重用）和那份 DOM 的序號，drop 時拿掉它的表。`add_to_linker` 收一個從 Store 的 data 找到它的函式。Store 比它的 DOM 活得久時（engine 先丟掉），它的呼叫 trap，drop 時也不碰同一個 thread 之後的 DOM：每份 DOM 有自己的序號，對不上就當它不在了。
- **wasm 的 GC 物件在 DOM 的 heap 裡**：`configure` 讓 engine 用 elise、Store 借這個 thread 的 DOM 的 heap（wasmtime 的 fork 的 `EliseHeapProvider`：從 thread-local 借 `Dom`，鑰匙是 DOM 的序號；DOM 正被借著時、不是這份 DOM 時回錯誤）。這樣的 Store 都要在 `WasmDom::store_set` 建的 `StoreSet` 裡（不在的拿不到 GC heap）：set 擁有它們、交出它們的 root、借出時算 heap 的 frame，記憶體安全靠 fork 和 elise，不靠這個 crate（fork 的 `README.machopo.md` 的「A heap borrowed from the embedder」）。一份 DOM 一個 set；`WasmDom` 比 set 先走時，set 之後的呼叫都回錯誤。
- **GC 在 host 呼叫裡經過 Store 做**：Store 借出期間 heap 在 frame 裡，host 函式的 DOM 操作的安全點不 GC（看不到 wasm 的 stack）。會配置的 host 函式（成員、`create-document`）做完之後 heap 該 GC 時呼叫 `StoreContextMut::gc`：wasmtime 交這個 Store 的 stack 上的 root，set 交其他 Store 的，DOM 的物件一起收；別的 Store 也借出中時不做。沒有 GC heap 的 Store（C 的 guest）的 GC 由 fork 直接請 heap 做。呼叫之間（沒有 Store 借出時）DOM 的安全點照常 GC。
- host 函式是同步的 `func_wrap`，同步和 async 的 Store 都能用。

## 頁面的 engine（`WasmEngine`）

`dom_api::ScriptEngine` 的實作（`src/engine.rs`），wasm 文件用的；對應 `dom_js::V8Engine`。renderer 還沒有照文件選它，現在只有測試建它。

- **DOM 是它的**（上面的「engine 擁有的 DOM」）：`dom_mut()`、`dom_and_layout()` 直接給；跑 guest 的每一輪、建和丟 Store 時借進 thread-local。
- **沒有 JS**：不建 isolate。`run_classic_script`、module、`execute_webdriver_script`、DevTools 的 inspector、observer、idle callback、`requestAnimationFrame` 的方法什麼都不做（WebDriver 的 script 回錯誤）。event handler 的屬性（`onclick=""`）是 JS，不執行。
- **DOM 自己的工作照樣經過 `dom_api`**：輸入的預設動作（`handle_mouse`、`handle_key`、`handle_wheel`）、focus、文件生命週期的事件、元素的 task、CSS 的動畫、WebDriver 的查詢和輸入，都和 V8 那邊呼叫同樣的函式，host 是一個呼叫 script 時什麼都不做的 `ScriptHost`（guest 還不能註冊 listener）。microtask checkpoint 只有 DOM 自己的（圖片的更新、元素改變之後的步驟）。
- **只有最上層的 frame**：`create_window` 對其他 frame 回 `None`，iframe 沒有子文件。
- **一個 script 一個 Store**（`run_wasm_script`，bytes 是 component，world 是 `wit/script.wit` 的 `script`）：在呼叫的 thread 上同步編譯、建 Store、實例化、呼叫 `start(document)`（頁面的 document 在這個 Store 的 handle）。`start` 第一次等待時這個 script 對頁面就算執行完了，Store 留著。編不過、import 對不上、trap 都報到 console（`uncaught`，URL 是 script 的），trap 的 Store 丟掉；之後的 script 照常跑。新的 window（換頁）時前一份文件的 Store 全部丟掉。
- **guest 的等待是 engine 的 timer**：`until_next_timer()` 是 DOM 自己的下一個 task 和各個 Store 最早的期限裡比較早的，`run_timers()` 讓期限到了的 Store 各跑一輪。期限在每一輪結束時從 Store 讀出來記著（時鐘在 Store 裡，只有一輪裡拿得到）。
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

## guest 怎麼指節點（handle）

guest 拿到的節點是一個 `u32`，那個 Store 自己的編號（`src/handles.rs`）：

- **一個節點在一個 Store 裡一個編號**：交出時先找它在這個 Store 有沒有編號，有就給同一個，沒有才配一格。guest 直接比整數就知道是不是同一個節點；只有一種 handle，往上、往下轉型不用呼叫 host。guest 不要時呼叫 `release`（重複拿到不另外計數，guest 的 binding 自己數）。對應 Blink 每個節點在每個 world 最多一個 wrapper，一個 Store 是一個 world。
- **編號**：低 24 位元是 Store 的表的 index，高 8 位元是 generation（1 到 255，放掉時加一），0 永遠不是 handle。用了放掉的、沒給過的編號，呼叫 trap（只有 binding 的 bug 會這樣，和 wasm 拿 table 的 index 超出範圍一樣）。
- **表**：節點是 Store 的 holder（DOM 的 heap 上的物件，每塊 256 個 `Member<Node>`）的 edge，不是 root：只有 guest 拿著的節點活到 Store 死掉為止，跟著它走。每一格的 generation 和節點的種類在 heap 外，呼叫前不碰 DOM 就能檢查 handle 和 `this` 的 interface。
- **找一個節點在一個 Store 的編號**：elise 的 heap 的節點在 nursery 裡會搬，不能用位址當 key。這份 DOM 的第一個 Store 是 main（Blink 的 main world），它的編號在節點的 engine word 的低 32 位元（`Cx::engine_word`：JS 文件的節點放 wrapper 的那 8 bytes，沒有 JS engine 驅動的 heap 裡沒人用）；其他 Store 的編號在一張共用的表，高 32 位元是它的 index，每一項是幾個（Store，編號）。讀到的 main 的編號對回 Store 的表才算數。main Store 死掉之後不再有 main（換給別的 Store 時，它原本記在共用的表的編號會漏看）。共用的表的每一項只在有 Store 拿著那個節點時存在：release 或 Store 死掉時拿掉。engine word 和表存的都是數字，不撐住任何東西。

## 一次呼叫

每個成員一個有型別的 host 函式（`src/front.rs`），從 `dom_api::for_each_member!` 展開：參數和結果是成員自己的型別（`TypedMember::Args`、`Ret`），wasmtime 直接 lift、lower，沒有 `Val`。

1. 操作之外，不碰 DOM：`this` 和每個節點參數是這個 Store 的 handle（不是就 trap）；`this` 是成員的 interface 的（不是就是 `TypeError`，像 JS 的 receiver 檢查）；字串參數在 guest 的記憶體裡讀（UTF-8，不複製），檢查過（不是 UTF-8 就 trap）。
2. 成員的專用路徑（`TypedMember::call`，front 是 `WasmFront`）：在操作裡從 Store 的表拿節點，結果記下來；節點的結果在操作裡給 handle（表的 edge 在 DOM 的 heap 上）。
3. wasmtime lower 結果：字串先複製成一個 `Box<str>`（wasmtime 不 lower `Rc<str>`，`DomString` 是 thread 自己的），wasmtime 再複製進 guest。

錯誤是 `result<_, dom-exception>`，`code` 是 `exception_code` 的編號（`TYPE_ERROR` 是 1）。

## WIT

`wit/machopo-dom.wit`，`src/wit.rs` 從宣告表產生（`wit::describe`）、和檔案比對：

- 只有 plain 的成員（`wit::is_plain`，`for_each_member!` 的 `plain`）：節點的 interface 上、只拿和給節點、字串、數字、布林的成員（可選的布林是 `option<bool>`，`toggleAttribute` 的 `force`）。建構子、物件不是節點的 interface（`Event`、`EventTarget`、`Window`、`Location`）、拿 dictionary、union、callback 或給 sequence 的成員都不放：guest 還拿不了它們（只做 JS，wasm 需要時再補）。plain 的成員也只到 2026-10-30 那些（`wit::FROZEN`）：表之後加的 plain 成員只給 JS，等研究 wasm 時再看。
- 一種 handle `type node = u32`，每個節點的 interface 一個別名（`type element = node;`，只給 binding 的型別名稱）。
- 每個成員只在宣告它的 interface 上一次，名稱是 `<interface>-<member>`（kebab-case），attribute 的 setter 是 `<interface>-set-<attribute>`，第一個參數是 `this`；每個都回傳 `result<T, dom-exception>`。常數不放（WIT 沒有常數）。
- `create-document` 建一份 HTML 文件，`release` 放掉一個節點。

**加了或改了表上的成員之後**，用 `MACHOPO_UPDATE_GOLDEN=1 cargo test -p dom_wasm --test it wit` 重新寫，再用 `python3 scripts/ci.py guests` 重編測試的 guest。表上加了新的 interface 時，`src/front.rs` 的 `interfaces!` 也要加一行（沒加的話用到它的成員編不過）。

## 測試

```sh
cargo test -p dom_wasm
python3 scripts/ci.py guests          # 重編 tests/guests/ 的 guest（wasi-sdk 34.0、wit-bindgen 0.62.0）；每個 guest 的 world 在 ci.py 的 GUEST_WORLDS
```

測試不編 guest，只用 repo 裡的 `.wasm`。測試的 guest 是 C，編成 WASI 0.3 的 component（`wasm32-wasip3`）；測試的 Store 是 async 的（linker 是 wasmtime-wasi 的 p3），每次呼叫在 `StoreSet` 的一次借出裡 poll 到完成，world 在 `tests/guests/wit/test.wit`：import `machopo:dom` 的 interface，export `run`（照名稱跑一個 case，回傳哪裡錯了，沒錯是空字串）和 `sleep`（`async func`：等一段時間，醒來留一個記號給 `run` 的 case 讀）。

- `tests/it/engine.rs`（guest 是 `tests/guests/page.c`，world 是 `script`，照 document element 的 `data-case` 做事）：`start` 拿到頁面的 document、改它，每個 script 自己的 Store，之後 engine 的 full GC 留著改過的樹；等待的 script 在第一次等待時算執行完，期限是 `until_next_timer`，還沒到時 `run_timers` 不跑，到了才接著跑；trap 的 script 報到 console（`uncaught`、它的 URL）、之前做的留著、下一個 script 照常跑；不是 component 的 bytes 報到 console；guest 印的一行到 console；新的 window 丟掉前一份文件的 script（等待中的也是）；讓出（callback code 的 `YIELD`）的 guest 在同一輪裡接著跑完。
- `tests/it/lend.rs`：DOM 被 engine 拿走之後，engine 直接用它；guest 只有借出期間碰得到，不在時呼叫回錯誤、之後照樣能用；engine 手上的 full GC 留著 guest 拿著的節點和另一個 Store 的 GC 物件；unwind 的借出也交還；已經在的 DOM 不能再借進來；DOM 不在時丟掉的 Store 留下它的表。
- `tests/it/wasi.rs`：guest 在 async 的 export `sleep` 裡等待時留在 Store 裡、期限是 `next_deadline`，期限之前的一輪不叫醒它，到了之後的一輪才醒（時間到了但還沒跑那一輪時也還沒醒），等待期間照樣能呼叫它；不用等的等待在開始的那一輪就結束；兩個 Store 各自的等待；guest 讀到的時鐘和解析度是 100 µs；印的東西一行一行來、分 stdout 和 stderr，沒有換行的最後一行在 Store 丟掉時來。
- `tests/it/wit.rs`：`wit/machopo-dom.wit` 是表產生的那份；`for_each_member!` 的 `plain` 剛好是 `wit::is_plain` 的，跳過的是物件不是節點的那幾個 interface，WIT 裡沒有 event；WIT 的成員剛好是凍結的那些，每個都還是 plain。
- `tests/it/guest.rs`（guest 是 `tests/guests/dom.c`，每個 case 一個新的 Store）：建樹、插入、走訪、移除，同一個節點每次同一個編號，element 的 handle 給 `Node` 的成員也行；字串兩個方向（屬性值的 ASCII 和非 ASCII、名稱、命名空間、text 的 data、`[LegacyNullToEmptyString]` 的 none 是空字串、`textContent`）；DOM 的錯照代碼（2、3、4、9），`this` 是別的 interface 的是代碼 1 的錯，不是 trap，Store 照樣能用；放掉的格子給下一個節點別的編號；guest 留著的節點經過 GC 還在、放掉之後 Store 沒拿著東西；放掉的、沒給過的、0、放兩次的 handle 和不是 UTF-8 的字串 trap；兩個 Store 各自的編號，第二個（不是 main）的在共用的表、它死掉時拿掉，第一個死掉之後新的 Store 也走共用的表；比它的 DOM 活得久的 Store 的呼叫 trap，它 drop 時不碰同一個 thread 之後的 DOM（那份 DOM 的第一個 Store 和它同號）。
- 單元測試（`src/handles.rs`）：main Store 的編號每次一樣、在 engine word 的低 32 位元，放掉之後不是任何節點的、同一格的下一個編號不一樣；其他 Store 的在共用的一項，最後一個 Store release 或死掉時拿掉；main 死掉之後新的 Store 不讀它留下的 word；handle 撐住節點經過 GC，放掉或 Store 死掉之後收掉；generation 從 255 繞回 1、超過一塊的節點。`src/wit.rs`：kebab-case。`src/wasi.rs`：最早的期限是最早的那個等待的，同一個等待再 poll 還是一個；時間到了、沒人叫醒就被 poll 的等待結束、不再留著；被叫醒但時間還沒到（時間倒退）的再留一次。`src/turn.rs`：叫醒自己的 future 接著 poll，沒叫醒的那次 poll 之後這一輪結束，下一輪從那裡接著。
