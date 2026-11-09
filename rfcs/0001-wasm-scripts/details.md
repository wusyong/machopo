# RFC 0001 的細節：wasm 腳本

這份是 `README.md` 的補充，放支撐它的研究和比較。結論和決定以 `README.md` 為準（它的「已決定的事項」）；實作前用原型釐清的問題和結論在 `wasmtime-questions.md`。

## V8 的 wasm runtime

2026-10-01 原本的選擇，`wasm` 分支照它做完了。V8 本身就有完整的 wasm runtime：直譯器 DrumBrake（jitless 和除錯用）、baseline 編譯器 Liftoff、最佳化編譯器 Turboshaft，函式第一次被呼叫時才編譯，常用的再在背景 thread 重新最佳化（見 V8 原始碼裡的 [wasm 架構文件](https://chromium.googlesource.com/v8/v8/+/HEAD/docs/wasm/architecture.md)）。

2026-10-01 在 kun 的 V8（`v8` crate 152）裡實測：`WebAssembly` 全域物件存在，同步編譯和執行一個小 module 都正常；`compileStreaming` / `instantiateStreaming` 不存在，因為它們接受 fetch 的 `Response`，由 embedder 提供（Blink 用 V8 的 `SetWasmStreamingCallback` 接上）。

`wasm` 分支做的時候碰到、換到 wasmtime 之後不再需要處理的：`WebAssembly.compile` 要有人 pump platform 才會 resolve，所以自己開 compile thread 同步編譯；`v8` crate 的 `WasmModuleObject::compile` 不能帶編譯選項，JS String Builtins 要經過 JS API 打開；等待用 JSPI，暫停中的 module 的 callback 結果回不來；V8 不檢查「import 是 JS function」時的簽章，要自己在 link 之前檢查。

## wasmtime

2026-10-04 查證：
- **版本**：v47（2026-07-20）起 wasm GC 預設打開，v49 在 2026-09-21 發布；大約每個月一個大版本。
- **支援的層級**：`gc`、`exception-handling`、Component Model 是 Tier 1；stack-switching 在 x86-64 上還在做、aarch64 上不支援。
- **collector**：`Collector` 只有 `Auto`、`DeferredReferenceCounting`、`Null`、`Copying` 四種，沒有讓 embedder 提供 collector 的選項；DRC 收不掉循環。只留 WIT 之後用不到 wasmtime 的 GC（guest 只拿著整數的 handle）。
- **Winch**（baseline 編譯器）不支援 `reference-types` 和 `gc`；只用 WIT 的 guest 不需要它們。
- **exception handling**：wasmtime 只支援新版（`exnref`）的；clang 的 `-mllvm -wasm-enable-sjlj`（C 的 `setjmp`/`longjmp`，Lua 用到）預設產生舊版的 `try`/`catch`，要加 `-mllvm -wasm-use-legacy-eh=false`（`wasmtime-questions.md` 的 Q10）。
- **WASI**：wasmtime-wasi 49 的預設 feature 是 `p1`、`p2`、`p3` 三個全開（`p3` 依賴 `component-model-async`）。`WasiCtxBuilder` 可以自訂 stdin / stdout / stderr、wall clock 和 monotonic clock、亂數；預設沒有 preopen 的目錄（沒有檔案系統），TCP、UDP、名稱查詢預設關掉。
- **WASI 0.3 的工具鏈**：Rust 的 `wasm32-wasip3` 在 2026-08 升到 tier 2（rust-lang/rust 的 PR #161940），但 kun 固定的 1.99 裡還是 tier 3：`rustc --print target-list` 有它，rustup 沒有預先編好的 std。std 的原始碼已經有 WASI 0.3 的實作，kun 用 `-Zbuild-std` 自己編（`README.md` 的「已決定的事項」第 40 條）；不用 std 的 I/O 時，`wasm32-unknown-unknown` 加上 wit-bindgen 的 async 也編得出 component（`cm-prototype` 的做法）。wasi-sdk 有 `wasm32-wasip3` target，wasi-libc 支援 WASI 0.3。Go 主線只有 `wasip1`，TinyGo 到 `wasip2`，WASI 0.3 只有 2026-06 開的 issue 在問。
- **Component Model 的重新進入**：見下一節。

## Component Model 的重新進入

Component Model 原本規定 component instance 不能被重新進入：每個 instance 有 `may_enter` 旗標，export 執行中時是 false，這時再呼叫它的 export 就 trap（wasmtime 的 `CannotEnterComponent`）。這和 DOM 的同步分派衝突：guest 呼叫 `dispatchEvent` 時它正在執行，host 不能同步地呼叫它的 listener，只能 trap 或延後執行（延後的話 `dispatchEvent` 的回傳值、`defaultPrevented` 和 listener 的順序都錯）。

2026-08-28，Component Model 的 PR #705（「CABI: remove the may_enter flag/trap」）拿掉了這條限制，理由是它「實際上不是必要的」；之後除了「instance 已經 trap 過」，重新進入都允許。wasmtime 的 PR #14146（2026-08-31）跟進，包含在 49 裡；PR #14302（49）又規定同一個 Store 的 concurrent event loop（`run_concurrent`）不能遞迴。

還留下的限制：
- **async 的 export 有一把鎖**：用 sync 或 callback ABI lift 的 async 函式，執行 core wasm 時拿著 instance 層級的鎖（backpressure），這時再呼叫 async 的 export 要等鎖；外層同步地卡在 import 裡時就是死結。非 async 的函式不理會 backpressure。
- **trap 之後**：參考實作（`design/mvp/canonical-abi/definitions.py`）的 `trap()` 先鎖住整個 Store（`store.lock_down()`）再 `raise Trap()`，`Store.invoke`、`Store.tick` 都檢查沒有鎖住，整份沒有任何 `except Trap`：trap 一路傳到最上層的 host，經過的 host 函式也接不住，Store 裡什麼都不能再執行，包括已經在 stack 上的外層。wasmtime 的 async Store 照做（event loop 帶著 trap 結束）；sync Store 讓 host 拿到錯誤、外層繼續執行，比規格寬鬆。
- **guest 自己**要能承受重新進入（規格提醒產生 binding 的工具要小心）。

wasmtime 49.0.2 上的實測（`cm-prototype`，2026-10-04）和還沒解決的部分在 `wasmtime-questions.md` 的 Q1–Q3。

## 考慮過、沒有選的做法

| 做法 | 為什麼不選 |
| --- | --- |
| wasm 跑在 V8 上 | 2026-10-01 選的，`wasm` 分支做完了。長處和換掉的理由見 `README.md` 的「理由和其他做法」和「動機」 |
| 同時保留 `kun:dom/v1`（`externref`） | 兩種介面要一起維護；`externref` 讓 cppgc 和 wasmtime 之間有 GC 參照，要 fork wasmtime 的 collector 才收得乾淨（「未來考慮：wasmtime 加上 cppgc collector」）；Winch 跑不了 |
| wasmtime，沒有 holder | 兩個 GC 互相看不到：DOM 拿著 guest 的 callback 是 root，guest 拿著的節點也是 root，跨兩邊的循環要等 Store 被丟掉（頁面離開）才收得掉。緩解方式見「Store 還活著時的緩解方式」 |
| 兩個 heap 分開，但互相 trace | 等於在 wasmtime 上重做 V8 的 unified heap，wasmtime 一樣沒有給 embedder 用的 trace 介面 |

**不連結 V8**（2026-10-04 起是方向，`README.md` 的「已決定的事項」第 14 條，另開一份 RFC）：kun 的 cppgc 是從 rusty_v8 連結進來的，和完整的 V8 綁在一起，所以「不建 isolate、不跑 JS」（這份 RFC 的「不跑 V8 的設定」第 1 步）照樣連結 V8。要真的不連結，得換成單獨的 cppgc：官方支援單獨使用（V8 的 GN 參數 `cppgc_is_standalone` 和 `cppgc_standalone` target），官方的獨立 mirror `oilpan-gc/cppgc` 在 2024 年 5 月封存。換成單獨的 cppgc 要從 V8 原始碼編（P1 刻意避開的事），或自己 vendor 和維護；`v8` crate 的 cppgc 模組綁的是完整的 V8，要拆出來或自己寫 FFI；兩份 cppgc 的 C++ symbol 會衝突，所以會是另一種 build，不能在執行時切換。`dom` 除了 V8 模組只用 `v8::cppgc` 的 API，拆的時候主要改的是它從哪個 crate 來。

## Store 當成 cppgc 物件

`README.md` 的「Store 和 GC」的細節。不需要 fork wasmtime。

**每個 wasm script 一個 Store，cppgc 上有一個代表它的 holder**。holder 是 `dom` 的 GC 物件，放：
- **slot 表**：guest 拿到的每個 DOM 物件在這裡佔一格，格子是指向節點（或 event、window）的 `Edge`；guest 手上的 handle（component 的 resource，在 host 那一側是一個 rep）只對應到格子的位置。表是一串固定大小的 chunk，每個 chunk 是一個 GC 物件（GC 物件裡不能放 `Vec<Edge>`，`crates/dom/soundness.md` 的 I5）。表放在 Store 外面，因為 Store 不在 GC 的圖裡，`trace()` 要讀得到它。
- **id**：Store 表裡那一列的鍵。

**Store 本身**（instance、table、memory 和編好的程式碼）放在 `DomState` 的 Store 表，不在 heap 上：每一列是「holder 的 id → holder 的弱參照、Store」（`README.md` 的「已決定的事項」第 29 條）。入口要執行一個 Store 時從表裡取出，用完放回去。

**edge 怎麼連**：
- DOM 指向 wasm：listener 的 `CallbackSlot` 是指向 holder 的 `Edge` 加上 guest 的 id；timer 的 `ScriptCallback` 在 heap 外面，用 `Persistent` 撐住 holder（第 30 條）。相等就是同一個 holder、同一個 id，`removeEventListener` 找得到。
- wasm 指向 DOM：holder 的 `trace()` 標記所有還在用的格子。guest drop 那個 handle 時，格子還回去。

**什麼時候被回收**：沒有任何東西指向 holder 時，holder 和它的格子一起變成垃圾，Store 表裡它的弱參照被清掉；event loop 的最外層 prune 這張表時 drop 那個 Store，和 `NodeIdMap` 一樣。finalizer 裡不能 drop Store（會執行任意的 destructor，`crates/dom/README.md` 的「`Drop` 的時機」）。2026-10-05 以前的設計把 Store 放在 holder 裡、finalizer 把它移到「待 drop」的佇列，finalizer 要碰共用的狀態，所以改成這張表。正在執行或暫停中的 Store 由入口拿著的 root 撐住。

**不需要掃描 wasm 的 stack**：guest 只拿著整數的 handle，Store 活著時它所有的格子都算活的，所以 stack 上、fiber 上有什麼都不用知道。

**和 V8 版本比較回收的粒度**：
- 一樣的：用 linear memory 的語言（Rust、C、Lua）。它們拿著的 DOM 物件在 V8 版本也是 instance 的 root（`externref` table 或 handle 表），instance 活著就收不掉。
- 沒有的：V8 的 unified heap 能收掉編成 wasm GC 的語言經過它們自己物件的循環；這個版本不支援那些語言直接用 GC 參照碰 DOM（`README.md` 的「動機」裡「不在範圍內」那一段）。
- 比較好的：沒有 trampoline，callback 的比較和存法都比較簡單；Store 被丟掉時 memory 和編好的程式碼一起釋放。

**incremental marking**：slot 表的寫入經過 `Edge` 的 barrier，和其他 GC 物件一樣（`crates/dom/README.md`）。

## 未來考慮：wasmtime 加上 cppgc collector

2026-10-04 到 2026-10-05 是這份 RFC 的最終目標（第二層）；2026-10-05 移到 RFC 0005（延後，`README.md` 的「已決定的事項」第 54 條），這一節和 `wasmtime-questions.md` 的 Q13–Q17 是它的研究和原型。這份 RFC 做完的第一層（holder 和 Store 表）不依賴它。在不跑 V8 的設定裡，cppgc 是唯一的 GC，wasm 的 GC 物件放在它上面就更自然。DOM 參照也變成 GC 參照（resource 直接是指向節點的指標）是再往後的事，要等 GC 版的 canonical ABI 或 kun 自己的 GC 形式的 `kun:dom`（`README.md` 的「之後的可能」）。

**是什麼**：fork wasmtime，讓 wasm GC 物件直接配置在 DOM 所在的 cppgc heap 上。wasm 和 DOM 之間的參照就是同一個 heap 裡的指標，循環照樣能回收，回收的粒度和 V8 版本一樣細。

**形狀上可行**（2026-10-04 查 kun 用的 V8 原始碼，`wasmtime-questions.md` 的 Q16 用原型確認）：cppgc 在 64 位元上預設打開 caged heap，caged heap 會自動打開 cppgc 自己的 pointer compression；GN 的 `cppgc_enable_larger_cage` 預設也打開，所以 cage 是 16 GiB、壓縮值是 `地址 >> 3`。cppgc 的參照本來就是 32 位元，和 wasmtime「32 位元的 GC 參照」一樣寬；物件 16 byte 對齊時，最低位元可以留給 i31ref 的 tag（8 byte 對齊不夠）。

**要解決的事**：
- **wasmtime 沒有給 embedder 的 collector 介面**：`GcRuntime` / `GcHeap` 是內部 API，所以要 fork，並長期維護。
- **GC 的 lowering 要改**：wasm 的 `struct.new`、`struct.set`、`array.set` 這些指令被翻成「去哪裡配置、物件長什麼樣、寫入時的 barrier」，每種 collector 各有一份，在 wasmtime 把 wasm 翻成 Cranelift IR 的那一層（`wasmtime-cranelift`）。`wasmtime-questions.md` 的 Q14 讀過、反組譯比過：物件用 null collector 的 layout（對齊到 16），參照換成位址要 fork 共用的定址程式碼（解壓縮比現在多一個指令、不需要邊界檢查），寫入多一個讀全域旗標的 barrier 判斷，讀取沒有 barrier。
- **配置**：rusty_v8 fork 的 C++ 物件 `WasmGcObject`（`wasmtime-questions.md` 的 Q16）：型別索引和長度，欄位接在後面，`Trace` 照型別表找出參照欄位；一個物件比欄位多 16 byte。wasmtime 的型別資訊要轉成這張型別表。
- **stack 上的 root**：不跑 V8 時只在安全點 GC，stack 上沒有 wasm 的 frame；有 V8 時，正在執行的 Store 的物件這次 GC 全部算活的（一張弱參照表，`wasmtime-questions.md` 的 Q15，原型確認成立）。不需要 cppgc 認得 stack 上的壓縮指標。
- **Sandbox 變弱**：原版 wasmtime 把 GC heap 當成不可信任的記憶體：一塊獨立的區域，GC 參照是 32 位元的索引，每次存取都檢查邊界。換成 cppgc 之後，邊界變成整個 cage，裡面也有 DOM 節點，Cranelift、型別檢查或 collector 的一個 bug 就能改到旁邊的節點，例如蓋掉 `v8` crate 呼叫 `trace()` 用的 `dynamic` 指標。和 V8 版本差不多（V8 沒開 sandbox，見「安全和資源」），但比原版 wasmtime 弱。可以保住 sandbox 的做法：wasm 的 GC 物件放在自己的區域，指向 DOM 一律經過一張 handle 表，cppgc trace 時順著表走；這和開了 sandbox 的 V8 用 `CppHeapPointerTable` 指向 cppgc 物件是同一個想法。 2026-10-04 決定先接受（`README.md` 的「已決定的事項」第 15 條），之後再補上面 handle 表的做法。
- **沒有前例**：V8 自己也不是這樣做，它的 wasm GC 物件在 V8 的 JS heap，靠 unified heap 和 cppgc 連起來。

**開始之前先確認**：那時的 wasmtime 上游有沒有開放可替換的 collector（有的話就不必 fork）。原本也要先試 cppgc 的保守掃描認不認得壓縮指標，Q15 的退路成立之後不需要了。

## Store 還活著時的緩解方式

回收的粒度是整個 Store，所以 Store 還活著時（例如頁面上其他 listener 還指向它），它拿著的節點都收不掉。SDK 和 API 的設計可以減少這種情況：
- **讓循環留在 cppgc 裡**：註冊 listener 時，closure 要用到的 DOM 參照交給 host 存在 listener（cppgc 上的 GC 物件）裡，closure 只拿位置，呼叫時以 `borrow` 傳入。最常見的「節點 → listener → closure → 節點」整個在 cppgc 裡，能被回收。
- **guest 往 DOM 的參照預設是弱的**：callback 收到的節點一律是 `borrow`；長期記住用弱 handle；強 handle 要明確選擇。
- **DOM 主動通知 guest 釋放**：listener 被回收時，下一個 task 呼叫 guest 的 `release-callback(id)`（finalizer 裡不能直接呼叫 guest，見 `crates/dom/README.md` 的「`Drop` 的時機」）。
- **debug build 的洩漏偵測**：列出不在樹上、只被 Store 的格子抓住的節點。

手動管理記憶體的語言即使跑在 V8 上也有同樣的性質：Rust 經 wasm-bindgen 在瀏覽器上，listener 的 closure 抓住 DOM 物件時，一樣要靠程式自己釋放。

WASI 0.3 在 2026-06-11 通過，把 async 放進 Component Model；Wasmtime 46 是第一個實作最終版規格的版本；WASI 1.0 的目標是 2026 年底或 2027 年初。

## guest 的 binding 和 wasm-bindgen 的對照

| | wasm-bindgen / web-sys | kun（wit-bindgen + `dom_wasm`） |
| --- | --- | --- |
| 作者寫的程式 | `document.create_element("div")?`、`parent.append_child(&child)?` | 差不多；WIT 沒有繼承，參數要轉型時用 `as-node` 這類函式 |
| 型別化的 API 從哪裡來 | WebIDL（web-sys） | `dom_api` 的宣告表產生的 WIT，wit-bindgen 再產生各語言的 binding |
| 中間那層 | wasm-bindgen 產生的 JS 膠水 | `dom_wasm`，沒有 JS |
| DOM 物件 | `JsValue`（表的索引，或 `externref`） | WIT 的 resource：guest 手上是 handle，host 那一側是 holder 裡的格子 |
| 字串 | 複製進出 linear memory | UTF-8 的 `(指標, 長度)`，直接變成 `DomString`，複製一次；結果由 guest 的 `cabi_realloc` 配置 |
| 錯誤 | JS 膠水接住例外，變成 `Result<_, JsValue>` | `result<_, dom-exception>` |
| Closure 和 listener | `Closure`，JS 那邊產生一個轉呼叫的函式 | guest 保管 closure、給 host 一個 id；DOM 存 holder 加上 id，呼叫時轉給那種 callback 的 guest export（`handle-event` 等，RFC 0010 的第 10 條） |
| 能跑在哪裡 | 任何瀏覽器 | 只有 kun |

## 安全和資源

- **signal handler**：wasmtime 預設用 guard 區和 signal（macOS 上預設是 Mach port）把越界存取轉成 trap，不是 wasm 程式碼造成的 fault 交給之前的 handler。同一個 process 裡另外只有 Rust 的 std 為了印出 stack overflow 訊息裝的 handler；V8 只有打開它的 wasm trap handler 時才裝 SIGSEGV 的 handler，main 上 kun 沒有打開（頁面 JS 的 `WebAssembly` 用明確的邊界檢查），不跑 V8 的設定裡也沒有 V8。所以要確認的只有 wasmtime 和 Rust 的 handler 共存，做成實作時的測試（`wasmtime-questions.md` 的 Q8）。
- **V8 沒開 sandbox**（2026-10-01 確認）：當時 kun 用 `v8` crate 的預設 feature，沒開 `v8_enable_sandbox`，連結的 prebuilt 是 `librusty_v8_release_<target>`，檔名沒有 `_sandbox`。v152.2.0 的 release 也沒有任何開 sandbox 的 prebuilt（`_ptrcomp` 的 release 版只有 macOS 兩個架構和 Linux x86-64），要開就得從原始碼編 V8。2026-10-02 起 kun 自己編 V8（`P3.md` 的「已決定的事項」第 10 條），只開了 pointer compression，sandbox 還是沒開；要開的話是加上 `v8_enable_sandbox` 再編一次，embedder 這邊也要遵守 sandbox 的規則。所以 V8 heap 和 cppgc 之間是直接指標，JIT 或 GC 的記憶體錯誤可以碰到 DOM 和整個 process；renderer 也還不是獨立的 process（RFC 0004），目前沒有任何隔離。wasmtime 的 linear memory 有自己的 sandbox，但 DOM 在同一個 process 裡，所以整體一樣沒有隔離。
- **Memory 的上限**：每個 Store 用 `StoreLimits`，可以限制單一 memory 和整個 Store 的合計。
- **執行時間**：wasm 用 wasmtime 的 epoch 中斷；JS 照舊要由 watchdog thread 呼叫 `TerminateExecution`，kun 還沒有。
- **權限**：只 import 給它的：DOM、timer、WASI（沒有檔案系統和 socket）。

## 接其他 engine

`dom_api` 的宣告表、中性的值、`ScriptHost` 和 `ScriptCallback` 是 engine 的契約。新的 engine 要實作：engine 的值和中性的值之間的轉換、把宣告表安裝進 engine（或像 wasm 那樣從宣告表產生介面）、`ScriptCallback` 裡它的那一種 callback 怎麼存（以及它的 GC 語意；engine 的物件不在 cppgc 上時，照「Store 當成 cppgc 物件」的做法把 engine 那一側整塊當成一個 GC 物件）、`dom` 裡它自己的模組（入口、callback 裡開 scope 的 `unsafe` 和安全論證），以及執行入口遵守 token 的規則。依賴規則（`P3.md` 第 4 節）讓 engine 專屬的程式留在 engine 層。wasmtime 是 V8 之後的第二個 engine，`ScriptCallback` 從這時起改成列舉。

## 參考資料

- V8：[WebAssembly compilation pipeline](https://v8.dev/docs/wasm-compilation-pipeline)、[docs/wasm/architecture.md](https://chromium.googlesource.com/v8/v8/+/HEAD/docs/wasm/architecture.md)、[Oilpan library](https://v8.dev/blog/oilpan-library)
- cppgc：[Oilpan 的 README](https://chromium.googlesource.com/v8/v8.git/+show/main/include/cppgc/README.md)、[oilpan-gc/cppgc](https://github.com/oilpan-gc/cppgc)（已封存）、[V8 的 gni/v8.gni](https://github.com/v8/v8/blob/master/gni/v8.gni)
- Rust：[wasm32-wasip3](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip3.html)、[PR #161940](https://github.com/rust-lang/rust/pull/161940)；wasi-sdk：[issue #589](https://github.com/WebAssembly/wasi-sdk/issues/589)；TinyGo：[issue #5478](https://github.com/tinygo-org/tinygo/issues/5478)
- wasmtime：[Collector](https://docs.wasmtime.dev/api/wasmtime/enum.Collector.html)、[支援的層級](https://docs.wasmtime.dev/stability-tiers.html)、[WasiCtxBuilder](https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/struct.WasiCtxBuilder.html)、[v47.0.0](https://newreleases.io/project/github/bytecodealliance/wasmtime/release/v47.0.0)、[v49.0.0](https://newreleases.io/project/github/bytecodealliance/wasmtime/release/v49.0.0)、[PR #14146](https://github.com/bytecodealliance/wasmtime/pull/14146)、[PR #14302](https://github.com/bytecodealliance/wasmtime/pull/14302)、[CVE-2026-27195](https://nvd.nist.gov/vuln/detail/CVE-2026-27195)、[架構文件](https://docs.wasmtime.dev/contributing-architecture.html)、[GC and Exceptions in Wasmtime](https://bytecodealliance.org/articles/wasmtime-gc)
- Component Model：[PR #705](https://github.com/WebAssembly/component-model/pull/705)、[Concurrency.md](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Concurrency.md)、[CanonicalABI.md](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md)、[The Road to Component Model 1.0](https://bytecodealliance.org/articles/the-road-to-component-model-1-0)
- WASI：[WASI 0.3](https://wasi.dev/releases/wasi-p3)、[WASI 0.3 Launched](https://bytecodealliance.org/articles/WASI-0.3)

## handle 的問題和改善的方向

2026-10-05 RFC 做完之後的討論，還沒排程。之後要做時另開一份 RFC（暫定 `0006-dom-handles`，「wasm 的 DOM handle：生命週期、身分和成本」），把這一節搬過去、補上選項的比較。

**handle 是什麼**：guest 拿到的 DOM 物件（WIT 的 resource，例如 `element`）是一個 i32。它指到 wasmtime 的 handle 表（每個 component instance 一張，Component Model 規定的），那一格的 rep 是 holder 的 slot 表的位置，slot 才是指向節點的 `Edge`（`README.md` 的「Store 和 GC」）。`own` 的 handle 要 guest 自己 drop，`borrow` 的只在一次呼叫裡有效；同一個節點每拿一次就佔一格、拿到一個新的號碼。

**問題**：
1. **不 drop 就漏**：slot 撐住節點，直到 guest drop 那個 handle，或整個 Store 被回收，就算節點已經從文件拿掉。linear memory 的語言靠 binding 自動 drop（Rust 的 `Drop`、Lua 的 `__gc`）；編成 wasm GC 的語言沒有 finalizer，物件被回收時沒有機會 drop，一定會漏。
2. **callback 放不掉**：DOM 只認得 callback 的 id，guest 要自己存一張「id → 函式」的表，又不知道 DOM 什麼時候不再需要某個 id，所以那張表只增不減。Lua 的 runtime 最明顯：表在 Lua 的 registry 裡，是 Lua 的 GC 的 root，closure 和它抓住的物件（handle）都放不掉（`sdk/lua/README.md` 的「限制」）。
3. **沒有身分**：同一個節點每次拿到的 handle 都不同，guest 判斷不了兩個 handle 是不是同一個節點。
4. **以 Store 為單位回收**：「節點 → listener → holder → slot → 節點」這種跨兩邊的循環，要等整個 Store 沒人參照才收得掉。
5. **每次呼叫的成本**：handle 換成 DOM 物件、替結果建 handle，佔每次呼叫的 7–28%（`progress.md` 的 profile），一半是 wasmtime 的 handle 表。

**在標準 Component Model 裡的補法**（不需要 GC ABI）：

| 補法 | 解決 | 要注意的 |
| --- | --- | --- |
| `callback-dropped(id)`：DOM 不再參照某個 callback 時（listener 被移除、節點被回收、timer 跑完）通知 guest，guest 從自己的表拿掉那一項 | 2；4 的一部分（已經不在的 listener） | guest 的 world 要新增一個 export：舊的 guest 沒有它，`kun:dom` 不升版的話，host 要能把它當成可有可無（找不到就不通知）；`kun:dom` 是 0.x 時也可以當成不相容的改動、升 minor（RFC 0008）。「不再被參照」要由 GC 判斷（例如 callback 的 slot 被回收時、Store 表 prune 時） |
| `is-same-node`（WebIDL 本來就有 `Node.isSameNode`） | 3 | 只是新增函式 |
| slot 去重：同一個節點在同一個 Store 裡共用一格，計數 | 記憶體 | guest 拿到的 handle 號碼照樣每次不同（Component Model 每次 lower 都發新的 handle），身分還是靠 `is-same-node` |
| 有型別的 host 函式（每個 WIT 函式一個 `func_wrap`） | 5 | 第 24、50 條；Component Model 1.0 的 lazy ABI、去掉同步呼叫的額外成本也會幫忙 |

**linear memory 的語言長期都用 handle**：clang 的 `__externref_t` 和 Rust 都不能把 `externref` 存進一般的記憶體，只能放進 wasm 的 table，手上拿的還是號碼。就算 GC ABI 出現，C、Rust 和用 C 寫的 Lua 也是這樣，所以上面的補法是它們長期的做法，不是過渡。

**GC 參照那條路**（RFC 0005 加上 Component Model 的 GC ABI）：GC ABI（預提案，`rfcs/0005-cppgc-wasm-gc/README.md` 的「待決定」）讓 resource 在邊界上以 `externref` 傳；有 RFC 0005 時，它才能直接是 cppgc 上的節點，否則是 wasmtime 自己 GC heap 上的包裝物件。這條路解決 wasm GC 語言的 1 和 4，callback 也能直接是 closure 的參照、不需要 id 表。它和 handle 的路共用同一份 WIT，每個 component 自己選用哪一種 canonical option，兩條路並存。
- 不選強制所有 guest 都用 `externref`：linear memory 的語言得不到好處（只是把 handle 表搬到 guest 的 table，身分也還是要 `is-same-node`，`externref` 不能 `ref.eq`）；社群工具鏈預設編出的 component 不能用；現在的 guest 全部不相容，要升成 `kun:dom` 2.0；Winch 不支援 reference types。
- 不選現在自己 fork、只給 DOM 開 `externref` 的特例：wasm-tools（二進位格式和驗證）、wasmtime（lift/lower）、wit-bindgen（guest 的 binding）都要改，沒有 RFC 0005 也沒有效果；GC ABI 定案時細節多半不同，要重做；也違反「介面只有 WIT」（第 10 條）。

**Lua**：用 C 寫的直譯器，Lua 的物件在 linear memory 裡、由 Lua 自己的 GC 管，所以 RFC 0005 和 GC ABI 都幫不了它；`callback-dropped` 才是它的解法。要讓 Lua 也走 GC 參照那條路，得換成物件本身就是 wasm GC 物件的 Lua VM，再加上 GC ABI 和 RFC 0005；只有 wasm GC 的 Lua 和 RFC 0005、沒有 GC ABI 時反而更糟：handle 存在沒有 finalizer 的 GC 物件裡，沒有人 drop。

## 已決定的事項的完整理由

`README.md` 的「已決定的事項」每條只寫決定和主要的理由；這裡是確認當時寫下的完整版本（不選的做法和原因、原型和量測、實作時的補充），編號相同。

2026-10-01 確認：

1. ~~**wasm 跑在 V8 上**，和 JS 共用一個 isolate、一個 heap、一條 event loop。~~ 2026-10-04 改成第 8 條。
2. **wasm script 照 module script 的規則執行**：不擋住 parser，parse 完照順序執行。
3. **wasm 的 DOM 介面用回傳值回報錯誤**，例外只留在 JS 那邊，因為各語言對 wasm exception handling 的支援程度不一。（2026-10-04 起是 WIT 的 `result<_, dom-exception>`。）
4. **兩個前端從 `dom_api` 的同一張宣告表產生**，這張表和 `dom_api` 在 P3 就做（`P3.md` 的「已決定的事項」第 8 條）。
5. ~~**作者用從宣告表產生的 guest SDK**（Rust 先做），用起來像 web-sys。~~ 2026-10-04 改成第 10 條：binding 由 wit-bindgen 從宣告表產生的 WIT 產生。不做「讓 web-sys 不經過 JS 執行」的相容層這一半照舊（理由見「理由和其他做法」）。
6. **fork wasmtime、用 cppgc 當它的 collector**：2026-10-01 列為未來考慮的方向；2026-10-04 改成這份 RFC 的最終目標，見第 13 條。 2026-10-05 移到 RFC 0005（第 54 條）。
7. ~~**`kun:dom/v1` 只用 wasmtime 也能實作的東西**。~~ 2026-10-04 起不做 `kun:dom/v1`（第 10 條）。

2026-10-04 確認：

8. **wasm 跑在 wasmtime 上，DOM 留在 V8 的 cppgc 上，JS 照舊跑在 V8**；必要時 fork wasmtime（理由見「動機」）。2026-10-05 起 wasm 文件的 DOM 在單獨的 `cppgc::Heap` 上，不接 V8 的 isolate（第 26 條）。
9. **從 main 重新實作**：V8 版本留在 `wasm` 分支當對照組，和 engine 無關的部分從那裡帶過來（「和 V8 版本的關係」）。
10. **DOM 介面只有 WIT（`kun:dom`），加上 WASI**；不做 `kun:dom/v1`，binding 由 wit-bindgen 產生（理由見「動機」）。
11. **guest 是 component，WASI 用 0.3（P3）**：wasmtime 和工具鏈都往這裡走，host 用 wasmtime-wasi（P3 是預設的 feature）；WASI 0.2 和 preview1 不承諾。
12. **驗收的語言是 Rust、C、Lua，拿掉 Go**：Go 主線只有 `wasip1`，TinyGo 到 `wasip2`，都沒有 WASI 0.3。等 Go 有了再加回來（「之後的可能」）。
13. **最終目標：cppgc 當 wasmtime 的 GC**：必要時 fork wasmtime，wasm 自己的 GC 物件也放在 cppgc 上（「分階段」的第二層）。第一層（holder 和 Store 表）先做，不依賴它。 2026-10-05 改成第 54 條：第二層移到 RFC 0005。
14. **wasm 的路徑不依賴 V8**：只有 wasm 的頁面不需要 isolate，之後 kun 可以有不跑 V8、只當 wasm engine 用的設定（「不跑 V8 的設定」）。這份 RFC 做到「不建 isolate」；不連結 V8（standalone 的 cppgc）另開一份 RFC。
15. **第二層接受 sandbox 變弱**：cppgc 當 wasmtime 的 GC 之後，wasm 的 GC 物件和 DOM 在同一個 cage 裡，邊界檢查只擋得住 cage 之外，wasmtime「每個 Store 的 GC heap 是一個小 sandbox」的約定不再成立；安全程度和現在沒開 sandbox 的 V8 一樣（`details.md` 的「未來考慮：wasmtime 加上 cppgc collector」）。之後可以補 sandbox：wasm 的物件放在 cage 裡自己的區域，指向 DOM 一律經過 handle 表（像開了 sandbox 的 V8 的 `CppHeapPointerTable`）。 2026-10-05 起是 RFC 0005 的事（第 54 條）。
16. **不跑 V8 時，DOM 放在單獨使用的 `cppgc::Heap` 上，GC 由 cppgc 自己排**：heap 選 `StackSupport::kNoConservativeStackScan`，cppgc 依配置量決定什麼時候收，從配置裡開始 incremental marking（不看 stack），marking 的步驟、收尾和 sweep 經過 kun 提供的 Platform 排成 renderer 的 non-nestable task，所以 GC 只發生在 task 之間，stack 上沒有東西。原型確認：10 萬個活的節點，最長的 GC task 不到 2 ms，heap 穩定（`wasmtime-questions.md` 的 Q18）。要改 rusty_v8 fork（`StandaloneHeap`、在 `AllocationHandle` 上配置），已經寫好，還沒發布。不選不接 isolate 的 `CppHeap`：它完全不會自己收，唯一的 GC 入口是 testing API，收尾時一次 sweep 完，10 萬個節點停頓約 23 ms。將來不連結 V8 時，單獨的 cppgc 也是同一個 API。2026-10-05 起每份 wasm 文件的 DOM 都放在這種 heap 上，第 3 步就做（第 26 條）。
17. **會呼叫回 guest 的 DOM 函式宣告成 `async func`，當場完成**（宣告表的 `async method`：`dispatchEvent`、`AbortController.abort()`）：guest 的 `callback` export 是 sync 的，host 用 `call_concurrent` 照順序呼叫 listener，不等任何其他東西（「執行 script 的入口和重新進入」）。sync 的程式用 `block_on`、async 的程式用 `await`，都能同步分派給自己的 listener，巢狀的也成立；一次分派約 1.8 µs（`wasmtime-questions.md` 的 Q2、Q11）。不選 fork wasmtime、讓 async Store 裡的 sync import 直接重新進入：改動和每次升級的成本都比較大。不選退回 core module：要放棄 Component Model 和 WASI 0.3。
18. **wasm 自己分派時它的 listener trap，那次分派剩下的 listener 跳過，記成限制**：Component Model 規定 trap 鎖住整個 Store、一路傳到最上層的 host，所以那次分派排在後面的 listener（2026-10-04 時包括 JS 的；2026-10-05 起只有 wasm 的，第 26 條）不會執行，違反 HTML 的「回報例外、繼續分派」；當成「這個程式當掉了，那次分派也跟著中止」。之後再設計「可以接續的分派」：分派的進度放在 DOM 的狀態裡，最上層的入口拿到 trap 之後把剩下的 listener 分派完（`wasmtime-questions.md` 的 Q3）。不選現在就做：要改 `dom_api` 的分派演算法；2026-10-04 時中間隔著 JS 的情況要等 Q6，現在只能做一半（2026-10-05 起沒有這種情況）。
19. **給 script 的時間照 HR-Time 的「coarsen time」降到 100 µs，宣告表加上 `performance.now()` 和 `Element.localName`**（`wasm` 分支 W2 的第 14、16 條，2026-10-05 隨第 2 步帶過來）：kun 沒有 cross-origin isolation，規格的下限就是 100 µs，不加 jitter。JS 的 `performance.now()`、之後 WASI 的 monotonic 時鐘、`Event.timeStamp`、animation frame 收到的時間都經過 `dom_api::window::coarsen`；timer、animation frame 和 frame 自己照舊用完整的時鐘。`performance.now()` 是 `Window` 降階的 `performanceNow`（還沒有 `Performance` interface），script 執行時照樣在走（`FrameClient::now`）。`localName` 讓 Lua 的 runtime 認得 `<script type="text/lua">`。和 engine 無關，所以跟著 `dom_api` 一起搬；WIT 是從宣告表產生的，現在不搬的話 golden file 之後會再變一次。
20. **第 2 步的範圍**（2026-10-05）：`dom_api::wit` 只產生 WIT（加上 golden file 和用 `wit-parser` 的檢查），`wasm` 分支替 V8 版本自己實作 canonical ABI 的部分（攤平、大小、對齊、core 的名稱）不帶，wasmtime 自己做 canonical ABI；它借用的 `kun:dom/v1` 的小函式（snake_case、字典的成員、例外的代碼）搬進 `wit`。WIT 的版本從 `kun:dom@1.0.0` 改成 `0.1.0`：從沒發布過，`dispatchEvent` 改成 `async func`（第 17 條）本身就不相容，之後的 `click()`、`focus()` 也會；RFC 做完時定為 1.0.0，從那時起只能新增函式。Lua binding 的產生器留到第 4 步和 Lua guest 一起搬：它產生的 C 呼叫 wit-bindgen 的 C binding，`dispatchEvent` 改成 async 之後那部分也要改，沒有 Lua 的 runtime 也編不了。 2026-10-05 RFC 做完時改成 1.0.0。
21. **guest 的入口 `start(window)` 是 `async func`**（2026-10-05）：Component Model 規定不是 async 的 export 在回傳之前不能等待（它等的 async import 沒有當場完成就是 trap，`wasmtime-questions.md` 的 Q2），所以 sync 的 `start` 只能像 JS 一樣從頭跑到尾，`callback` 又是 sync 的，整個程式都用不了 WASI 的等待（`sleep`、I/O）。`async func` 的 `start` 可以中途暫停，和 WASI 0.3 的 `wasi:cli/run` 一樣，也和 `wasm` 分支用 JSPI 讓 guest 在 `poll_oneoff` 裡暫停的能力一樣；暫停時 listener 照樣進來執行（Q4）。`callback` 照第 17 條維持 sync。
22. **holder 放在 `dom` 裡、和 engine 無關**（2026-10-05）：`dom` 不依賴 wasmtime，只提供一個「外部 engine 的 holder」GC 物件：不透明的 payload（Store 放在 `Box<dyn Any>` 裡）、指向節點、事件、window 的 slot 表、執行中由入口拿著的 root；`ScriptCallback` 多一種「holder 加上 guest 的 id」。不選 `dom` 裡依賴 wasmtime 的模組：`unsafe` 照樣只在 `dom`，`dom` 的依賴不變，之後接其他 engine 也能用。2026-10-05 起 Store 不放在 holder 的 payload，改放在 Store 表（第 29 條）。
23. ~~**第 3 步只支援 JS 和 wasm 經過 DOM 的最外層互相碰到**（2026-10-05）：renderer 分派的事件（使用者點擊、timer）依序呼叫兩邊的 listener，組起來的 engine 照 callback 的種類交給 V8 或 wasmtime。巢狀的（wasm 的 `dispatchEvent` 碰到 JS 的 listener、反過來、再繞回同一個 Store）是第 5 步「兩個 engine 交錯」的範圍，在那之前回報錯誤、跳過那個 listener，記成限制。~~ 2026-10-05 被第 26 條取代：JS 和 wasm 不在同一份文件裡。
24. **`kun:dom` 的 host 函式在執行時從宣告表產生**（2026-10-05）：每個 WIT 函式用 wasmtime 的 `func_new` 接上同一個共用的 host 函式，參數用 `Val` 轉換，和 `dom_js` 的共用成員 callback 一樣。不選用巨集或 build script 為每個函式產生有型別的 `func_wrap`：比較快（Q11 量過一次約 60 ns），但產生程式碼比較複雜；先量動態的成本，需要時再換。
25. **第 3 步做完時 shell 預設把 V8 和 wasmtime 組起來**（2026-10-05）：頁面的 `<script type="application/wasm">` 從此會執行。只有 wasmtime 的設定是第 7 步。2026-10-05 起 shell 裝兩個 engine，renderer 照文件的種類選一個（第 26、27 條），wasm script 只在 wasm 文件裡執行。
26. **一份文件只用一種 script，JS 和 wasm 的 DOM 分成兩套、不互通**（2026-10-05）：wasm 文件只執行 wasm，DOM 放在單獨的 `cppgc::Heap` 上（第 16 條的 heap），不建 isolate；JS 文件照舊在 V8 的 isolate 上，wasm script 不執行。兩套用同一份 `dom` 和 `dom_api` 的程式碼，只有 heap、engine 和 callback 的種類不同。理由：同一份文件裡兩個 engine 交錯（wasm A → JS → `el.click()` → A 的 listener）要用裸指標重新進入正在更外層被借用的 Store，加上 aliasing 的論證；分開之後 wasm 的 host 函式可以用 `DomAccess`（第 28 條）得到編譯時的保證，wasm 文件也完全不經過 V8。取代第 23 條、「分階段」的第 5 步和原本「待決定」的第 2 題（兩個 engine 交錯）；第 7 步縮小成 shell 的設定。代價見「缺點」：wasm 文件不能用 JS。
27. **wasm 文件用 `<head>` 裡的 `<meta>` 宣告**（2026-10-05）：renderer 在 parse 之前找它（`CommitNavigation` 時手上已經有整份 HTML；原本打算掃整個 `<head>`，2026-10-05 改成第 36 條的前 1024 byte），沒有宣告的是 JS 文件。換頁時種類不同，就換一個 `DomIsolate` 和 engine。名字和掃描的範圍見第 36 條。不選整個 kun 一個設定：不能一個分頁開一般網頁、另一個開 wasm 的頁面。不選看有沒有 `<script type="application/wasm">` 自動判斷：JS 頁面夾了一個 wasm script，它所有的 JS 就突然不跑，難以預期；也要多 tokenize 一次。
28. **wasm 的 host 函式透過唯一的 `DomAccess` 開 DOM scope**（2026-10-05）：只有入口建得出來，不能複製；開 scope 要借用它，所以同時最多一個 DOM scope，由借用規則保證（「執行 script 的入口和重新進入」）。代價：分派給別的 Store 時要交接；wasm 執行中 Rust 的程式 panic 時 abort，和 V8 的 binding 一樣。`DomAccess` 是 `Send`：wasmtime 的 `run_concurrent` 要求 Store 的 data 是 `Send`；DOM 只能在 heap 的 thread 上用，所以 `mutate`、`read` 先檢查 thread，不對就 panic（碰到 DOM 之前，約 1 ns；kun 只在 renderer 的 main thread 上 poll Store，實際上不會失敗）。不選執行中把 `DomAccess` 放在 thread-local：借出 `&mut` 要執行期的借用檢查，失去編譯時的保證（2026-10-05 實作 3b 時查到 `Send` 的要求後確認）。不選執行期的旗標（每次開 scope 約 1 ns，寫錯時 panic），使用者要編譯時的保證；不選讓 `dom` 擁有 Store（`dom` 要依賴 wasmtime，engine 的程式大部分要搬進 `unsafe` 所在的 crate）。
29. **Store 放在 `DomState` 一張不在 heap 上的 Store 表裡**（2026-10-05）：每一列是「holder 的 id → holder 的弱參照、Store」，event loop 的最外層 prune，弱參照空了就 drop Store，和 `NodeIdMap` 一樣；holder 只剩 slot 表和 id。不選把 Store 放在 holder 的 payload、finalizer 把它移到「待 drop」的佇列：finalizer 碰到共用的佇列，要為 I6 開例外。成本：每個 event loop 的 step 掃一次表（列數是 wasm script 的數目），每次進入 Store 查一次表，都是 ns 級。
30. **`ScriptCallback`、`CallbackSlot` 改成 enum：V8 的 function，或 holder 加上 guest 的 id**（2026-10-05）：一個 DOM 只接受自己那一種，放錯時 panic，和現在別的 isolate 的 callback 一樣（`assert_isolate`）。只有 engine 的 bug 會放錯，panic 不是 UB。不選用型別參數把兩套 DOM 分開：型別參數會一路傳到 `dom_api` 的 `Value`、`ScriptHost`、規範的演算法和 renderer，為了只有 bug 才碰得到的錯誤改動整個 DOM。
31. **slot 表在 `dom` 裡用 Rust 寫**（2026-10-05）：一串固定大小的 chunk，每個 chunk 是一個 GC 物件，放 64 格；每格和 `TargetEdge` 一樣是節點、事件、window 三條 `Edge`，一次只有一條不是空的（不用 enum：改 variant 是非 atomic 的寫入，concurrent marker 可能讀到一半）。`gc.rs` 加上固定長度陣列的 `Trace`（不會搬家，和 I5 不准 `Vec<Edge>` 的理由不衝突）。guest 每拿一次佔一格，不去重。不選 fork 的 C++（Q16 的 `HostMember`）：每格只要一條參照、和第二層（現在是 RFC 0005）的形式接近，但要改 fork、再發 release，trace 和 barrier 只能靠審查，canary 和對照模型也用不上；省下的記憶體每格 8 byte。
32. **Store 在執行中、或還有會叫醒它的東西（它等的 timer、I/O，以及 `start` 回傳之前）時，由 root 撐住**（2026-10-05）：`dom` 提供撐住和放開，什麼時候撐由 engine 決定（3c）。和 JS 一樣：等待中的 timer 撐住它的 callback，`await` 一個永遠不會完成的 promise、又沒有人參照時會被回收。不選整個文件期間都撐住：「節點 → listener → holder → slot → 節點」的循環就永遠收不掉。不選只在執行中撐住：等 timer 的 Store 會被回收，`sleep` 醒不過來。
33. **wasm 自己的工作照 3a 的 heap task 排進 event loop**（2026-10-05）：engine 自己保管一個佇列（被叫醒的 Store、guest 等待的到期時間），放進東西時用 `WakeEventLoop` 叫醒 renderer；`ScriptEngine` 多 `next_task_due`（renderer 等待時算進去）和 `run_tasks`（在 event loop 的最外層呼叫，每跑一個 Store 之後做一次 microtask checkpoint；回傳有沒有跑 script，有的話 renderer 和跑完 timer 一樣請求 main frame。這一點原本漏了，2026-10-05 第 4 步 4a 的視窗確認才發現：guest 等完之後改的 DOM 要等到下一次 input 才畫出來）。佇列用 `dom` 的 heap task 那一個（公開出來）。代價：這些工作排在那一輪 channel 裡的 task 後面，和 DOM 的 timer 一樣。不選每次叫醒都送一個 renderer 的 task：renderer 要另外做延遲的 task 和叫醒的合併，多出來的程式落在 renderer 的 main thread 這個沒有單元測試工具的地方；它的好處（和其他 task 照放進的順序交錯）在只有一條、沒有優先順序的佇列上用不上。不選借用 DOM 的 timer：WASI 的等待不該有巢狀 timer 的下限，也不該被 `clearTimeout` 碰到。

34. **分派演算法改成可以讓出（async），guest 的 `dispatchEvent` 碰到同一個 Store 的 listener 時讓出給 Store 的 event loop**（2026-10-05）：`dom_api` 只有分派那條路（`dispatch`、`invoke`、`inner_invoke` 和 `dispatchEvent` 本身）是 `async fn`，其他 DOM 函式照舊同步；`ScriptHost` 多 `call_async`，預設包著同步的 `call`，V8 那邊 poll 一次就完成（和 wasmtime-wasi 用一份 async 的實作、同步的 binding 把它跑完同一個結構）。wasm 那邊呼叫同一個 Store 的 `callback` 時，把 `DomAccess` 放回 Store 的 data、`await` 那次呼叫、再拿回來（第 28 條的交接）；分派給別的 Store 照樣同步。wasmtime 要求 host 的 future 是 `Send`，分派的 future 拿著 `NodeHandle`（`Rc`）不是，所以用 `dom` 提供的 `ThreadBound<T>` 包起來：可以搬，在別的 thread 上 poll 會 panic、drop 會 abort，論證寫進 `crates/dom/soundness.md`。不選 stackful coroutine（corosensei 這類）：每次分派多一個 stack，切 stack 依平台而定，第二層（現在是 RFC 0005）掃描 stack 時又多一種；不選在 `dom_wasm` 另寫一份分派：規範的語意會有兩份；不選把同一個 Store 的 listener 跳過記成限制：違反「目標」裡同步執行同一個 module 的 listener。
35. **reftest W7 移到 3e**（2026-10-05）：reftest 經過 renderer，而 renderer 照 `<meta>` 選 wasm 的 DOM 和 engine 是 3e 的事。3d 做重新進入，W7 的 C 原始碼帶過來、先在 `dom_wasm` 的測試裡跑；reftest W7 和 `about:demo` 在 3e。

36. **wasm 文件的宣告是 `<meta name="script" content="wasm">`，和編碼一樣在前 1024 byte 預先掃描**（2026-10-05）：parse 之前要看的 `<meta>` 都用同一套規則找（HTML 的「prescan a byte stream」：跳過註解和其他標籤的屬性），程式共用；寫在 1024 byte 之後的不算，那份文件是 JS 文件。名稱沒有 `kun-` 這類前綴，比較短；代價是將來若有標準或常見的工具也用 `name="script"`，可能撞名。不選用 html5ever 掃整個 `<head>`：每份文件多 tokenize 一次，規則也和編碼的不同。
37. **renderer 一次只有一套 DOM 和 engine**（2026-10-05）：換頁時新文件的種類和現在的不同，才丟掉舊的 `DomIsolate` 和 engine、建另一種；`RendererFlags::script_engine` 依文件的種類產生 engine。不選兩套都留著：renderer 一直多佔一個 isolate 或一份 wasmtime；renderer 一次只顯示一份文件，切換時本來就要重建文件。
38. **`about:demo` 的 wasm 部分是另一頁 `about:wasm-demo`**（2026-10-05）：wasm 不能放進 JS 文件（第 26 條）；從 `about:demo` 連過去，guest 是 `sdk/c/demo`（click 計數、`start` 裡等待的時鐘、guest 自己分派的事件），`.wasm` 用 `data:` URL 放進頁面，不必為 `about:` 頁面另做子資源的載入。不選拿 W7 當示範：一排方塊，看不出互動。

39. **reftest W1–W6 照原本的用意用 WIT 的 guest 重寫，W2、W4 重新定義**（2026-10-05）：`wasm` 分支的 W2（wasm script 和 JS 的 `defer` script 照順序執行）、W4（JS 分派事件給 wasm 的 listener，wasm 再分派回 JS）測的是 JS 和 wasm 的互動，第 26 條之後不存在了。W2 改成只有 wasm script 之間的順序：好幾個 wasm script 在 parse 完之後照文件順序執行，都在 `DOMContentLoaded` 之前；W4 改成兩個 wasm script（兩個 Store）之間的事件：renderer 分派的事件給它們的 listener，一個 Store 再分派給另一個（內容見第 43 條）。其他照原本的用意：W1 建 DOM；W3 無效的 module、import 不合、trap 各自回報，後面的 script 照常執行（涵蓋的情況見第 44 條）；W5 animation frame；W6 用 Rust 寫的 guest，原本用的 `kun_dom` SDK 已經拿掉（第 10 條），改用 wit-bindgen，做和 W7 一樣的檢查。W1–W5 用 C（wasi-sdk 和 wit-bindgen 已經固定版本，每個都很小），W6 用 Rust；Lua 是 W9。
40. **Rust 的 guest 用 kun 固定的 1.99 加上 `-Zbuild-std` 編成 `wasm32-wasip3`**（2026-10-05）：`wasm32-wasip3` 在 1.99 是 tier 3，沒有預先編好的 std，但 std 的原始碼已經有 WASI 0.3 的實作（透過 `wasip3` crate），所以 `ci.py guests` 設 `RUSTC_BOOTSTRAP=1` 在 stable 上用 `-Zbuild-std=std,panic_abort` 自己編 std；libc、`crt1-reactor.o` 和 linker（`wasm-component-ld`）用 wasi-sdk 34 的 `wasm32-wasip3`（`-C link-self-contained=no`），cdylib 要加 `-C relocation-model=static`，否則 linker 不產生 cooperative threading 要的 `__wasm_get_stack_pointer` 這類函式。Rust 的 guest 放在 `sdk/rust` 自己的 cargo workspace（自己的 `Cargo.lock`，`rust-toolchain.toml` 一樣固定 1.99），binding 用 wit-bindgen 0.62 的 Rust 版本。原型（2026-10-05）確認 std 的 WASI import 和 C 的一樣是 `@0.3.0`，`println!`、`Instant`、`std::thread::sleep` 都能用，async 的 `start`、`spawn_local`、`await` `dispatchEvent` 也都可以。注意：WASI 0.3 下每個 Component Model 的 task 有自己的 TLS，`start` 設的 `thread_local!` 在 `callback` 裡讀不到，跨呼叫的狀態要放在一般的 `static`。CI 不編 guest，只有重新編的人需要 `rust-src`。等 `wasm32-wasip3` 升到 tier 2 的穩定版之後，拿掉 `RUSTC_BOOTSTRAP` 和 `-Zbuild-std` 就好。不選 `wasm32-unknown-unknown` 加上 wit-bindgen：沒有 std 的 I/O，等待要自己接 WASI 時鐘的 WIT，也不像一般的 Rust 程式。不選等 tier 2：第 4 步的 Rust 部分要延後。不選 nightly：多一套 toolchain，`RUSTC_BOOTSTRAP` 就夠了。
41. **Store 的 event loop 被 guest 的 `YIELD` 叫醒時繼續 poll 同一個 `run_concurrent`**（2026-10-05）：原型找到的 bug：guest 回傳 callback code `YIELD`（wit-bindgen 的 Rust async 在 waker 被叫醒時就會這樣），wasmtime 從佇列取出「再呼叫 guest」的工作之後先 `yield_now()` 一次（叫醒 waker、回傳 `Pending`）；「需要時才跑」只 poll 一次就丟掉 `run_concurrent` 的 future，取出的工作跟著不見（wasmtime 49 丟掉 future 時只處理 fiber 和 host 的 future），guest 再也不會被叫醒。改成：poll 時 waker 被叫醒，就再 poll 同一個 future，直到它不再自己叫醒自己。代價：一直 yield 的 guest 會卡住 renderer，和 JS 的無限迴圈一樣（之後的 epoch 中斷一起處理）。給 wasmtime 的回報先寫成草稿（`wasmtime-yield-report.md`，還沒送出）。不選改 wasmtime 的 fork，讓丟掉 future 時把工作放回佇列：第一層不改 wasmtime。

42. **guest 的 `start` 第一次停下來就算執行完了，後面的 script、`DOMContentLoaded`、`load` 不等它回傳**（2026-10-05）：和 module script 的 top-level await 一樣，HTML 執行 module script 時不等它的 promise，下一個 script 和 `DOMContentLoaded` 照常。第 3 步做的就是這樣（`run_wasm_script` 跑一次 Store 就回來），這裡寫成決定，reftest W2 測它。不選等 `start` 回傳：一直在等的 `start`（例如 `about:wasm-demo` 的 C script）會讓整份文件的 `DOMContentLoaded` 永遠不觸發。
43. **reftest W4 用 `load` 代替使用者的點擊**（2026-10-05）：reftest 沒有 input。兩個 C 的 script（A、B）都監聽 `window` 的 `load`，renderer 照註冊的順序呼叫兩個 Store 的 listener；A 的 listener 再對一個元素 `dispatchEvent`，B 的 listener 同步執行、`preventDefault`，A 等 `dispatchEvent` 回來之後確認結果是 false。A → B → A（繞回正在更外層執行的 Store）是已知的限制，不放進 reftest。
44. **reftest W3 涵蓋八種失敗**（2026-10-05）：`wasm` 分支的六種（不是 wasm、MIME 不是 `application/wasm`、import 不合、`start` 裡 trap、載入失敗、不認識的 `type` 不會去抓），加上 component 才有的兩種：core module（不是 component，compile 失敗）、沒有 export `kun:dom/guest` 的 component（instantiate 失敗）。每個失敗的 script 如果真的跑了會畫紅色的方塊，最後一個 wasm script 畫綠色的（wasm 文件裡沒有 JS，原本最後那個 JS 的 `defer` script 改成 wasm）。不是 C 編出來的 `.wasm` 由 `ci.py guests` 寫出來，不另外放 `.wat`：本機的 `wasm-tools`（1.235）讀不了 async 的 component。

45. **Lua binding 的產生器放在 `sdk/lua/generator`，自己的 cargo workspace，只讀 `kun:dom` 的 WIT**（2026-10-05）：用 `wit-parser` 讀 `crates/dom_api/tests/machopo-dom.wit`，產生 `kun_dom.c`（原本放進 repo，第 56 條改成產生在 `target/guests/lua`），由 `ci.py guests` 執行。WIT 只有 `u32` 時分不出 callback 和其他的數字（`request-animation-frame(callback: u32)` 和 `cancel-animation-frame(handle: u32)`），所以 `dom_api::wit` 產生 `type callback = u32;`，callback 的參數都用它；C 和 Rust 的 binding 多一個 typedef，型別和 `u32` 一樣，現有的 guest 不用改，host 照舊。編好的 runtime 帶著它所根據的 WIT 的 hash（原本寫在 `kun_dom.c` 的開頭，第 56 條），`dom_api` 的 WIT 測試比對現在的 WIT，WIT 改了卻沒重新產生時 CI 抓得到，不用編產生器。不選放在 kun 的 workspace、讀 `dom_api` 的宣告表（`wasm` 分支的做法，原本的產生器可以照搬）：產生器只靠 WIT，正好驗證「介面只有 WIT」（第 10 條），任何人只拿 WIT 就寫得出同樣的 binding；也不用為它連結帶 V8 的測試執行檔。代價：產生器要用 `wit-parser` 重寫，往上轉的父 interface 從 `as-*` 函式推回來。
46. **Lua 這次不能等待**（2026-10-05）：W9 和 `wasm` 分支一樣。會擋住的 `sleep` 只能在 script 的最上層用（listener 裡等待，sync 的 `callback` 會 trap），容易用錯；只停一個 coroutine 的 async 等待要另外設計。記在「之後的可能」。
47. **`about:wasm-demo` 加上 Lua 的 script**（2026-10-05）：一段 `<script type="text/lua">` 加上 Lua 的 runtime，在視窗裡看得到 Lua。代價：執行檔多 Lua runtime 的大小（數百 KB）。不選只用 reftest W9 的頁面確認：`about:demo` 那一系列看不到 Lua。

48. **第 6 步只量這個分支，和已經記下的數字比；量測的程式是暫時的**（2026-10-05）：在 kun 裡量 compile、instantiate、`start` 的時間、記憶體和每次 DOM 呼叫的成本（release build），對照 Q11 的原型、`wasm` 分支記下的每次呼叫成本（`w2-progress.md`，2026-10-03），以及同一個 build 裡 JS 做同樣的呼叫。量完就拿掉量測的程式，數字和量法記在 `progress.md`。不選把 `wasm` 分支另外編起來、用同樣的量法兩邊各跑一次：要另外下載它的 V8、多一次完整的 build；條件不完全相同（原型不在 kun 裡、`wasm` 分支沒量過啟動和記憶體）是接受的代價。不選留下可以重跑的量測（`ci.py bench`）：之後要重量再寫。分兩段：6a 量，6b 照數字決定 Winch 和 code cache。
49. **啟動的編譯成本先不處理，記成已知的限制**（2026-10-05）：6a 量到 Lua 的 runtime 要編 45 ms（C 的 7–11 ms、Rust 的 13 ms），instantiate 和 `start` 在 0.3 ms 以內，啟動的成本幾乎都在 Cranelift 的編譯；每次載入都重編（每個 `WasmtimeEngine` 有自己的 wasmtime `Engine`，沒有 code cache）。考慮過的做法記在「之後的可能」：記憶體裡的 code cache 加上共用的 `Engine`、磁碟上的 code cache、Winch。限制寫在 `docs/limitations.md`。
50. **每次 DOM 呼叫的 host 成本先 profile，只修便宜的地方**（2026-10-05）：6a 量到比 Q11 只量 component 邊界的原型每次多 240–340 ns（`performanceNow` 299 ns、`setAttribute` 666 ns、`parentNode` 488 ns），一個 frame 一千次約 0.3–0.7 ms，比 V8 版本快，是 JS 的 2–5 倍。先找出時間花在哪裡（`Val` 的轉換、宣告表、DOM scope、slot 表），不改架構的地方先修；第 24 條的備案（每個 WIT 函式產生有型別的 `func_wrap`）等修完還不夠時再考慮。不選直接改成有型別的：產生程式碼比較複雜、`host.rs` 要大改，還不知道省下的是不是在邊界之外。結果（2026-10-05，`progress.md` 的 6b）：沒有單一、便宜的熱點，程式不改。時間大多在 wasmtime 的動態 host 函式（`Val` 的轉換、每次呼叫建立和刪除 host task，`performanceNow` 的 64%、`parentNode` 的 53%）和真正的 DOM 操作（`setAttribute` 的 41%）；kun 這一側分散在 handle 換成 DOM 物件（slot 表、建立 root）、替結果建 handle、參數轉成 `Value`（字串複製），各 5–15%，其中約一半又是 wasmtime 的 resource 表。小修（換 hasher、省掉 `Vec`）估計只省 5–10%。要省得多，得改成有型別的 `func_wrap`，或讓 `dom_api` 收借用的參照、不必替每個參數建 root，記在「之後的可能」。
51. **不跑 V8 的設定用執行時的選項 `kun --script wasm` 打開**（2026-10-05）：`RendererFlags::script_language` 固定每份文件的種類，renderer 從啟動就用 wasmtime；名稱對應 `<meta name="script" content="wasm">`：所有文件都是 wasm 文件（沒有宣告的也是），JS 不執行；不加時照舊看每份文件的 `<meta>`。同一個執行檔兩種設定都能跑、都能測。不選編譯時的 Cargo feature：現在還連結 V8，feature 只是讓 CI 多一種 build；不連結 V8 的那份 RFC 再用 build 時的開關。
52. **這個設定照舊完整初始化 V8，只是不建 isolate**（2026-10-05）：單獨的 heap 用 V8 的 platform 和 cppgc，`bindings::initialize` 不用改。Q11 量到的成本大多在 isolate 和 context，`V8::initialize` 本身很小。不選只初始化 platform 和 cppgc：`bindings`（`unsafe` 所在的 crate）多一條初始化的路，之後不小心建 isolate 時還要能明確失敗，省下的不多。
53. **這個設定的預設首頁是 `about:wasm-demo`**（2026-10-05）：`about:demo` 是 JS 文件，在這個設定裡它的 JS 不執行，互動都不會動，看起來像壞了；`about:wasm-demo` 一打開就看得到 C、Rust、Lua 三個 script。
54. **第二層（cppgc 當 wasmtime 的 GC）移到另一份 RFC（RFC 0005，延後），這份 RFC 做到第一層就完成**（2026-10-05）：guest 用 WIT 的 handle 拿 DOM 物件，第二層做完也不會讓跨 wasm 和 DOM 的回收變細（那要 DOM 參照變成 GC 參照，要等 GC 版的 canonical ABI）；C、Rust、Lua 都不用 wasm GC，編成 wasm GC 的語言還沒有 WASI 0.3，現在做完只有測試會用到，卻要立刻開始維護 wasmtime 的 fork。等有使用者（或 wasmtime 開放可以替換的 collector）再開始。改了第 13 條。不選現在就做第二層：先打基礎，但沒有使用者、維護成本立刻開始。不選連「DOM 參照變成 GC 參照」一起做：要離開 Component Model（失去 WASI 0.3 和 async、兩套介面），或等還不存在的 GC 版 canonical ABI。
55. **wasm 的前端 crate 叫 `dom_wasm`**（2026-10-05，RFC 做完之後）：原本叫 `dom_wasmtime`。用前端的 script 種類命名，和 JS 的前端（當時叫 `dom_bindings`，2026-10-05 改名 `dom_js`）對得起來，也比較短。不選 `dom_wasi`：WASI 只是這個 crate 的一小部分（`wasi.rs`），主體是 `kun:dom` 的 host 函式、Store 的執行和重新進入，叫 WASI 會讓人以為它在實作 WASI。和 `wasm` 分支同名的 `dom_wasm`（V8 上的 wasm）不會混淆：那個分支只留作參考（archive），提到它時寫成「`wasm` 分支的 `dom_wasm`」。名字不再標明 engine 的代價：wasmtime 專屬的程式照樣由 `sanity.py deps` 守在這個 crate 裡；真的要接第二個 wasm engine 時，再拆成 engine 無關的 `dom_wasm` 和各 engine 自己的 crate。型別名稱 `WasmtimeEngine` 不改，和 `V8Engine` 一樣用 engine 命名。
56. **Lua 的 binding 不放進 repo，WIT 的 hash 放進編好的 runtime**（2026-10-06，RFC 做完之後）：`kun_dom.c` 有 860 KB，WIT 每改一次就是一大段沒人審的 diff；執行和測試都只用 `resources/kun-lua.wasm`，只有 `ci.py guests` 編 runtime 時用到它，而它每次都先重新產生。所以改成產生在 `target/guests/lua`（和 wit-bindgen 的 C binding 一樣在 `target/guests/`），不進 repo。原本放在它開頭註解裡的 WIT 的 hash，改放進 runtime 的 custom section `kun_dom_wit`：產生器寫一段 top-level 的 `__asm__`，因為 C 的 `section` attribute 在 wasm 上產生的是 data segment，不是 custom section；組合語言的 section 名稱不能有 `-`，所以用底線。`wasm-component-ld` 包成 component 之後，core module 的 custom section 還在。檢查的是實際載入的檔案，比原本準：重新產生了 binding 卻沒重編 runtime 也抓得到。不選留在 `sdk/lua/kun_dom.c` 再加進 `.gitignore`：產物和手寫的原始碼放在一起，容易被當成要手改的檔案。不選拿掉檢查：WIT 加了函式卻忘了重編時，Lua 叫不到新函式，要等 W9 用到才發現。代價：要看 binding 長什麼樣，得自己跑 `ci.py guests`。
