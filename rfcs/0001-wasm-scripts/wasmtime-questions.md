# RFC 0001：wasmtime 待釐清的問題

改走 wasmtime 之後，實作前要用原型回答的問題。每題寫為什麼重要、目前知道什麼、怎麼釐清、狀態；原型不合併。**原型的位置**：`cm-prototype`、`q11-prototype`、`q18-prototype` 在 RFC 做完時（2026-10-05）刪掉了，原始碼在 commit `38fbbea` 的這個資料夾裡（例如 `git show 38fbbea:rfcs/0001-wasm-scripts/cm-prototype/README.md`）；Q14–Q16 的 `q14-prototype`、`q15-prototype`、`q16-prototype` 搬到 RFC 0005 的資料夾（`rfcs/0005-cppgc-wasm-gc/`）。一題有了結論，就併進 `README.md`（決定、設計）或 `details.md`（實測紀錄），這裡留一行結論和指過去的位置。

guest 是 component、WASI 用 0.3（`README.md` 的「已決定的事項」第 11 條）。WASI 0.3 讓 Store 一定是 async 的，所以 Q2 影響每個 guest。core module（kun 自己實作 canonical ABI）是 Q2 走不通時的備案。wasm 的路徑不依賴 V8（第 14 條），所以有兩種設定：有 JS 的（V8 和 wasmtime 都在）和不跑 V8 的（只有 wasmtime 和 cppgc）；只和其中一種有關的題目會註明。Q13–Q17 是「cppgc 當 wasmtime 的 GC」（原本這份 RFC 的第二層，第 13 條；2026-10-05 移到 RFC 0005，第 54 條）要先回答的（Q17 已經決定）。

| 題目 | 狀態 |
| --- | --- |
| Q1 只有 sync 函式時的同步重新進入 | 確認做得到 |
| Q2 async Store 裡的同步重新進入 | 確認：`async func` 加上當場完成的做法成立；分派給暫停中的 Store 併入 Q4 |
| Q3 trap 之後 | 確認：規格要求 trap 鎖住整個 Store、一路傳到 host；wasm 自己分派時剩下的 listener 被跳過，已決定先接受 |
| Q4 暫停中的 guest 收到外部事件 | 確認：listener 直接進去執行；跨 Store 的同步分派要「需要時才跑」 |
| Q5 wasmtime 的 async 接上 renderer 的 task queue | 確認：不用 tokio、沒有 busy loop；Store「需要時才跑」 |
| Q6 兩個 engine 交錯（只在有 JS 的設定） | 不需要了：2026-10-05 起 JS 和 wasm 的文件分開（`README.md` 的「已決定的事項」第 26 條） |
| Q7 `StoreHolder`（現在叫 holder） | 已接進 kun（3b）；Store 改放在 Store 表（第 29 條） |
| Q8 signal handler 共存 | 不做原型，改成實作時的測試 |
| Q9 WASI 0.3 的 host | 確認：wasmtime-wasi 的 `cli`、`random`、`filesystem` 不用 tokio 的 runtime；`monotonic-clock` 自己實作 |
| Q10 各語言的工具鏈 | 確認：C、Lua 用 wasi-sdk 34 編得出 WASI 0.3 的 component；Rust 的 std 等 toolchain |
| Q11 效能 | 量完：啟動 Cranelift 30 ms（V8 Liftoff 3 ms，code cache 0.3 ms），每次呼叫 60 ns 起（V8 10 ns），執行檔多 16 MB、乾淨 build 多 76 秒 |
| Q12 跟著 wasmtime 升級 | 持續 |
| Q13 wasmtime 的 collector 介面 | 讀完：要 fork，改動大多是新增；有三個和 wasmtime 假設衝突的地方 |
| Q14 GC 的 lowering | 讀完、反組譯比過：讀寫欄位和現在的 null 一樣便宜（寫入多兩個指令的 barrier 判斷）；要 fork 共用的定址程式碼 |
| Q15 stack 上的 root | 不跑 V8 時用安全點避開；有 V8 時的退路（弱參照表）原型確認成立；2026-10-05 起 wasm 文件都在不接 isolate 的 heap 上（第 26 條），退路用不到 |
| Q16 cppgc 上的 wasm GC 物件 | 確認：fork 的 C++ `WasmGcObject`，參照欄位是 cppgc 的 `Member`，要 16 byte 對齊；循環回收、marking 中寫入都通過，一個物件比欄位多 16 byte |
| Q17 sandbox | 已決定：先接受變弱，之後補 handle 表 |
| Q18 不接 isolate 的 cppgc heap | 確認：單獨的 `cppgc::Heap` 自己排 GC、只在 task 之間收，最長停頓不到 2 ms；已接進 kun（3a，`DomIsolate::new_standalone`） |

## Q1：只有 sync 函式時的同步重新進入

**為什麼重要**：DOM 的 `dispatchEvent` 要在回傳之前同步執行所有 listener，包括同一個 module 註冊的。guest 呼叫 `dispatchEvent` 時，它的 instance 正在執行，host 要再呼叫它的 `callback` export。

**結論**（2026-10-04，wasmtime 49.0.2）：做得到。Component Model 原本禁止重新進入（`may_enter` 的 trap），2026-08-28 的 PR #705 拿掉了，wasmtime 的 PR #14146 跟進、包含在 49 裡；`cm-prototype` 的 `sync-reenter` 確認：sync 的 export → sync 的 import → host 用 `call` 呼叫同一個 instance 的 sync export，listener 在 import 回來之前執行，回傳值也傳回去。但 WASI 0.3 讓 Store 變成 async，這條路用不到，見 Q2。

## Q2：async Store 裡的同步重新進入

**為什麼重要**：WASI 0.3 建在 Component Model 的 async 上，Store 一定是 async 的。guest 自己呼叫 `dispatchEvent` 時，照樣要同步執行它的 listener。

**目前知道的**（`cm-prototype`，wasmtime 49.0.2）：
- Store 裡只要有 async 的 host 函式，就只能用 `*_async` 的呼叫：sync 的 host 函式裡用 `call` 呼叫回 guest，是 `store configuration requires that *_async functions are used instead` 的錯誤。
- **blocking 的 host 函式也不能重新進入**（`async-reenter`）：sync 的 WIT 函式用 `func_wrap_async` 實作、裡面用 `call_async` 呼叫同一個 instance。`call_async` 內部會跑一次這個 Store 的 concurrent event loop，而外層的呼叫已經在跑它，遞迴是 wasmtime 的規則不允許的（PR #14302）。錯誤路徑上還有 bug：清理時 `unwrap()` 失敗，panic 蓋掉了原本的錯誤（和 CVE-2026-27195 是同一段程式碼的另一種失敗方式）。
- **能用的兩半**：import 宣告成 `async func`，host 用 `func_wrap_concurrent` 和 `call_concurrent`：
  - async 的 export `await` 它時沒有死結，sync 和 async 的 listener 都在它回來之前執行（`async-lock`）；
  - sync 的程式用 `wit_bindgen::block_on` 等它，listener 是 sync 的、當場完成時成立；listener 是 async 的就是 `cannot block a synchronous task before returning` 的 trap（`async-blocking`）。

**結論**（`cm-prototype` 的 `q2-*`，2026-10-04）：RFC 提的做法成立。`dispatch-event` 是 `async func`，guest 的 `callback` export 是 sync 的，host 用 `func_wrap_concurrent` 實作、照 listener 表用 `call_concurrent` 依序呼叫、不等任何其他東西：
- sync 的程式用 `block_on`、async 的程式用 `await`，都能同步分派給自己的 listener；`block_on` 沒有 trap，代表當場完成。
- 巢狀（listener 裡又分派給自己的另一個 listener）兩種程式都成立，內層先完成。
- 和 host 的 listener（代表 JS 的）混在一起時照順序執行。
- 成本見 Q11；trap 的情況見 Q3。
- 還沒測：分派給另一個暫停中的 Store 時，照「當場完成」的規則要排隊、不等它，併入 Q4。

**當初準備的備案**（現在不需要，留著參考）：
- **fork wasmtime**：呼叫非 async lift 的 export 時，guest 的程式不能 block（規格），所以不需要 event loop，可以在目前的 fiber 上直接執行，只是 task 的記帳（目前的 task 和 thread、`may_leave`、borrow 的 resource、post-return）要照做。規格允許這種重新進入，可以先問上游有沒有計畫支援。
- **退回 core module**：wasmtime 的 core async（fiber）沒有 Component Model 的 event loop，從 `func_wrap_async` 裡 `call_async` 同一個 instance 應該可以；要用原型確認，WASI 也要退回 preview1。

另外，錯誤路徑的 panic 寫成 issue 草稿，給人審過再決定要不要回報上游。

## Q3：trap 之後

**為什麼重要**：HTML 規定 listener 丟出例外時，回報之後繼續分派給下一個 listener，那段 script 之後也照常能用。

**目前知道的**：
- **sync 的 Store**（`cm-prototype` 的 `sync-nested-trap`、`sync-trap`）：巢狀的 listener trap 時，host 拿到錯誤，可以回報、繼續分派；已經在 stack 上的外層照常執行、正常回來。之後這個 Store 的任何 export 都是 `cannot enter component instance`。
- **async 的 Store**（`q2-sync-trap`，WASI 0.3 用的是這種）：host 的 `call_concurrent` **拿不到錯誤**，整個 Store 的 concurrent event loop 帶著 trap 結束：`dispatch-event` 沒有繼續，**那次分派排在後面的 listener 全被跳過，包括屬於其他 script 的（JS 的）listener**，最外層的呼叫直接失敗。wasmtime 把「trap 過」記在整個 Store 上（`ComponentStoreData::trapped`）。
- 每個 wasm script 一個 Store，所以「Store 不能再進入」等於「這個 module 之後的 listener、timer 全部失效」，已寫進 `README.md` 的「設計概要」和「缺點」。

**這是規格要求的**（2026-10-04 查證）：Component Model 的參考實作（`design/mvp/canonical-abi/definitions.py`）裡，`trap()` 先 `store.lock_down()`（鎖住整個 Store），再 `raise Trap()`；`Store.invoke` 和 `Store.tick` 一開頭都 `assert(not self.lockdown)`；整份沒有任何 `except Trap`，host 實作的 import 被呼叫時中間也沒有東西接住它。所以 trap 一路傳到最上層的 host，Store 裡什麼都不能再執行，包括已經在 stack 上的外層。wasmtime 49 的 async Store 照做：event loop 處理每個工作項目時是 `handle_work_item(item).await?`，guest trap 時整個 `run_concurrent` 帶著 trap 結束。sync Store 裡「host 拿到錯誤、外層照常執行」反而比規格寬鬆，是舊的 sync 路徑留下的行為，不能依賴。

**問題**：wasm 自己分派、自己的 listener trap 時，那次分派排在後面的 listener（包括屬於其他 script 的 JS listener）不會執行，違反「回報例外、繼續分派」。照設計，renderer 自己分派的事件（使用者的點擊），每個 wasm listener 是一次獨立的入口，trap 應該只結束那一次（還沒測）。

**選項**（2026-10-04 決定先接受，`README.md` 的「已決定的事項」第 18 條）：
- **接受**：當成「這個程式當掉了，那次分派也跟著中止」，寫進限制。2026-10-04 決定先這樣。
- **可以接續的分派**：分派的進度（事件、路徑、目前到第幾個 listener）放在 DOM 的狀態裡，不放在 host 的 future 裡；最上層的入口拿到 trap 之後，跳過那個鎖住的 Store，把剩下的 listener 分派完。剩下的 listener 會在 trap 回報之後才執行，彼此的順序不變；原本的呼叫者已經鎖住，看不到差別。要改 `dom_api` 的分派演算法；中間隔著 JS 的 frame（wasm → JS → wasm 的 listener trap）時要和 Q6 一起設計（2026-10-05 起沒有這種情況，第 26 條）。

## Q4：暫停中的 guest 收到外部事件

**為什麼重要**：guest 在等待時，使用者點擊、timer 到時，要送給它的 listener；別的 script 分派給它時，也要照 DOM 規範同步執行它的 listener。

**結論**（`cm-prototype` 的 `q45-*`，2026-10-04）：
- **listener 直接進去執行**，不等 guest 醒來（`q45-event`）：Component Model 的 sync export 不理會 backpressure，guest 的 async task 暫停時照樣能呼叫它的 `callback`。這和 JS 在 `await` 期間照樣執行事件一樣；`wasm` 分支（JSPI）是排到 guest 醒來之後。wit-bindgen 的 Rust async guest 在等待中被呼叫沒有出問題。用 C 這類 sync 的程式在 `sleep` 裡被呼叫時，對 guest 來說像 signal handler，要在 RFC 裡寫明。
- **跨 Store 的同步分派**：如果每個 Store 被一個常駐的 `run_concurrent` future 拿著，別的 Store 碰不到它，只能排隊，`dispatchEvent` 回來時別的 script 的 listener 還沒跑（`q45-two-stores`，違反同步分派）。改成「需要時才跑」（Q5）之後，分派的那個 Store 把對方從表裡取出、跑一次它的 event loop 呼叫 listener，listener 在 `dispatchEvent` 回來之前執行，回傳值也正確，對方正在等待也一樣（`q45-demand-two-stores`）。

## Q5：wasmtime 的 async 接上 renderer 的 task queue

**為什麼重要**：renderer 的 main thread 睡在自己的 task queue 上，沒有 tokio。guest 暫停時，等的東西到了要有人推進它的 future。

**結論**（`cm-prototype` 的 `q45-*`，2026-10-04）：
- **不用 tokio**：原型的 executor 是一個 task queue 加上 timer 的 heap，沒事時用 condvar 睡到下一個 timer；guest 的等待是 host 的 future，註冊 timer、拿 wasmtime 給的 waker，timer 到時 waker 把「跑這個 Store」排進 queue。
- **沒有 busy loop**：sleep 50 ms 再 30 ms，executor 只睡 2 次、poll Store 4 到 5 次；每次醒來晚大約 5 ms，三次都一樣，看起來是 macOS 上 condvar 的計時，kun 的 renderer 用自己的 timer，實作時再量。
- **Store「需要時才跑」**：Store 放在表裡；被喚醒（timer、事件）或被分派時才取出，poll 一次 `run_concurrent`（跑到沒有事可做）就丟掉，再放回去。丟掉之後 guest 的 task 和 host 的 future 都留在 Store 裡，下次接著跑（`q45-demand-sleep`）。這樣別的 Store 才能同步分派給它（Q4）。不同 Store 的 event loop 巢狀執行是 wasmtime 允許的（PR #14302 只禁止同一個 Store 遞迴）。
- 表裡取出的 Store 是 `None`，正好也是「這個 Store 正在 stack 更外層執行」的標記，2026-10-05 起 kun 的 Store 表（`README.md` 的「已決定的事項」第 29 條）就是這張表。
- **還沒確認**：wasmtime 的文件提醒，`run_concurrent` 的 closure 裡做 select（同時等 wasm 和別的東西）可能卡住（wasmtime 的 issue #11869、#11870）；「需要時才跑」的 closure 不等任何東西（永遠 `Pending`、poll 一次就丟），應該碰不到，實作時要有測試。

## Q6：兩個 engine 交錯（只在有 JS 的設定）

**為什麼重要**：wasm A → JS 的 listener → JS 呼叫 `el.click()` → A 的 listener。V8 的 binding 手上沒有 A 的 `&mut Store`，A 又正在更外層被借用。

**目前知道的**：要用裸指標接回去，加上一份 aliasing 的論證（`README.md` 的「待決定」第 2 題）。純 wasm 的頁面沒有這個問題。

**怎麼釐清**：要接進 kun 的 `dom` 才能試，放在實作的第 5 步；先在 `soundness.md` 寫好論證的草稿。

**2026-10-05 不需要了**：JS 和 wasm 的文件分開、各有自己的 DOM（`README.md` 的「已決定的事項」第 26 條），不會交錯。

## Q7：`StoreHolder`

**為什麼重要**：這是 GC 的做法（`README.md` 的「Store 和 GC」），對錯決定會不會漏收或提早釋放。

**目前知道的**：設計在 `details.md` 的「Store 當成 cppgc 物件」，2026-10-05 起叫 holder。要確認的：
- `StoreHolder` 的 `trace()` 在 wasm 執行中（拿著 `&mut Store`）也要讀得到 slot 表，所以表放在 Store 外面。
- finalizer 不能直接 drop Store（sweep 可能在別的 thread），要排一個 task。2026-10-05 改成 Store 不放在 holder 裡，放在 Store 表，以 holder 的弱參照為鍵，event loop 的最外層 prune（第 29 條）。
- component 的 resource 在 host 那一側是 `Resource<T>`（一個 `u32` 的 rep），要對應到 slot；guest drop handle 時 wasmtime 呼叫的 destructor 把 slot 還回去。
- 執行中、暫停中的 Store 由 root 撐住，不會被回收。

**怎麼釐清**：接進 kun 的 `dom`，寫 GC 的測試（循環被回收、回收之後 Store 被 drop、執行中不會被回收），在 `test.py matrix` 底下跑。

## Q8：signal handler 共存

**為什麼重要**：wasmtime 預設用 guard 區和 signal（macOS 上預設是 Mach port）把越界存取轉成 trap；同一個 process 裡的其他 handler 不能被它弄壞。

**結論**（2026-10-04，不做原型）：process 裡會裝 SIGSEGV 這類 handler 的只有 wasmtime 和 Rust 的 std（印出 stack overflow 訊息的那個）。V8 只有打開它的 wasm trap handler 時才裝，main 上 kun 沒有打開（頁面 JS 的 `WebAssembly` 用明確的邊界檢查），不跑 V8 的設定裡也沒有 V8。wasmtime 把不是 wasm 程式碼造成的 fault 交給之前的 handler。所以做成實作時的兩個測試：guest 越界存取，結果是 trap 而不是 process 當掉；子 process 裡讓一條 thread 的 stack overflow，Rust 的訊息照樣印出來（`wasm` 分支 `bindings` 的測試做過一樣的事）。將來頁面 JS 的 `WebAssembly` 要打開 V8 的 trap handler 時，再加兩邊同時打開的測試。

## Q9：WASI 0.3 的 host

**為什麼重要**：kun 給的 WASI 權限很少：stdout、stderr 送到 console，時鐘（monotonic 照 HR-Time 降到 100 µs），亂數，沒有檔案系統和 socket。

**目前知道的**：
- wasmtime-wasi 49 的預設 feature 是 `p1`、`p2`、`p3` 三個全開，可以按 interface 分別接上 linker（`p3::clocks::add_to_linker`、`p3::cli::add_to_linker`…），所以可以只用一部分。
- **時鐘的等待由 kun 自己實作**：wasmtime-wasi 的 WASI 0.3 `wait-for`、`wait-until` 直接用 `tokio::time::sleep`（`src/p3/clocks/host.rs`）。它要在 tokio runtime 的 context 裡建立和 poll（否則 panic「there is no reactor running」），而且 timer 要靠那個 runtime 的 time driver 推進：current-thread 的 runtime 只在 `block_on` 期間推進。kun 的 renderer main thread 不是 tokio 的 runtime，它睡在自己的 task queue 上（`crates/renderer/src/render_thread.rs` 的 `recv_timeout`，醒來的時間由 kun 自己的 timer 決定），所以照原樣用的話會 panic；只 `enter()` 一個 runtime 的 context 也沒用，timer 永遠不會響。
  - **技術上繞得過去**（沒有實測）：借用一條一直在 `block_on` 裡的 runtime，例如 `net` 的 NetworkService thread 那個（`crates/net/src/lib.rs`，`enable_all`）。renderer 跑 Store 的 event loop 時 `enter()` 它的 `Handle`，`sleep` 就註冊到那邊的 timer；時間到時 network thread 呼叫 wasmtime 給的 waker，waker 把「跑這個 Store」排進 renderer 的 queue（Q5 的 waker 本來就是 thread-safe 的）。
  - **不選它的理由**：網頁的 `setTimeout` 排在 renderer 自己的 timer 裡，wasm 的等待排在另一條 thread 的 tokio timer 裡，同樣期限的兩個 timer 誰先執行沒有保證，每次醒來也要多跨一次 thread；wasmtime-wasi 的 `wait-for` 不看自訂的 monotonic clock、直接睡真實的時間，`wait-until` 把自訂時鐘的 `now` 和真實的睡眠時間混在一起，而 kun 要 timer、`performance.now()` 和 WASI 的時鐘都是同一個（給 script 的降到 100 µs）；時間由 renderer 自己的 timer 控制，測試比較容易穩定；renderer 拆成獨立的 process 之後（RFC 0004），網路在另一個 process，renderer 手上沒有那個 runtime，還是得自己開一條 tokio 的 thread 只為了 timer。
  - **自己實作的成本很小**：`wasi:clocks/monotonic-clock` 只有 `now`、`get-resolution`、`wait-until`、`wait-for`；wasmtime-wasi 可以按 interface 分別接上，只換時鐘，其他照用。`now` 降到 100 µs，等待接 renderer 的 timer（Q5 的原型用自訂的 `sleep` 代替，做法一樣）。
- `WasiCtxBuilder` 可以自訂 stdin / stdout / stderr、wall clock 和 monotonic clock、亂數，預設沒有 preopen 的目錄（沒有檔案系統），TCP、UDP、名稱查詢預設關掉。WASI 0.3 的 stdout 用到 tokio 的 `AsyncWrite` trait 和 `oneshot`，自訂的輸出應該不需要 tokio 的 runtime，還沒確認。

**結論**（`cm-prototype` 的 `q9-*`，2026-10-04）：
- **不需要 tokio 的 runtime**：wasmtime-wasi 49 的 `p3::cli`、`p3::random`、`p3::filesystem`（沒有 preopen 的目錄）和 `clocks` 的 `types`、`system-clock` 直接接上；stdout 換成自己的 `AsyncWrite`（寫進去就印出完整的行，不用 tokio 的 I/O）。整個 process 沒有 tokio 的 runtime，C 的 `printf` 和 Lua 的 `print` 都正常。
- **`monotonic-clock` 自己實作**：`monotonic_clock::add_to_linker::<_, D>` 指定自己的型別，`now` 降到 100 µs，`get-resolution` 說 100 µs，`wait-for`、`wait-until` 掛在 executor 的 timer 上。C 的 `clock_getres` 拿到 100000 ns，`usleep(100000)` 醒來時量到 105400 µs。
- **wasmtime-wasi 的 `p3::cli::add_to_linker` 沒有接 `wasi:cli/types`**（只有一個 enum 的 interface），guest 有 import 它；自己補一個空的 `wasi:cli/types@0.3.0` instance。用 `define_unknown_imports_as_traps` 補的話，`terminal-stdin` 的 resource 型別會對不上。
- C 的 guest 只 import `cli` 和 `clocks`；Lua 多了 `wasi:cli/exit` 和 `wasi:filesystem`（`lauxlib` 的 `luaL_loadfilex`）。兩個都沒有 import `random`、`sockets`。

## Q10：各語言的工具鏈

**為什麼重要**：驗收的語言是 Rust、C、Lua（`README.md` 的「已決定的事項」第 12 條），要能編成 WASI 0.3 的 component。

**目前知道的**：
- Rust：`wasm32-wasip3` 在 kun 固定的 1.99 裡是 tier 3（沒有預先編好的 std），2026-08 升到 tier 2，要等含它的穩定版、升級 toolchain；在那之前，不用 std I/O 的 guest 可以用 `wasm32-unknown-unknown` 加上 wit-bindgen（`cm-prototype` 的做法），或用 nightly 的 `-Zbuild-std`。
- C：wasi-sdk 有 `wasm32-wasip3` target，wasi-libc 支援 WASI 0.3；binding 用 wit-bindgen 的 C。
- Lua：runtime 是 C，跟著 C 走。

**結論**（`cm-prototype` 的 `build-c.sh` 和 `q9-*`，2026-10-04）：
- **C**：wit-bindgen 0.62 的 C binding 加上 wasi-sdk 34 的 `wasm32-wasip3`，`wasm-component-ld` 直接連結出 WASI 0.3 的 component，不需要另外跑 `wasm-tools component new`。
- **Lua**：Lua 5.4.9 的核心和不碰檔案的函式庫，照 `wasm` 分支的旗標（`-mllvm -wasm-enable-sjlj`、`-lsetjmp`），**再加上 `-mllvm -wasm-use-legacy-eh=false`**：clang 預設產生舊版（legacy）的 exception handling，wasmtime 只支援 `exnref` 的新版（`wasm` 分支在 V8 上兩種都可以）。`pcall` 正常。
- **Rust**：std 在 kun 的 1.99 上沒有預先編好的 `wasm32-wasip3`；`cm-prototype` 的 Rust guest 都是 `wasm32-unknown-unknown` 加上 wit-bindgen，`kun:dom` 用得了，但沒有 WASI 的 I/O。2026-10-05 的原型：1.99 的 std 原始碼已經有 WASI 0.3 的實作，`RUSTC_BOOTSTRAP=1` 加上 `-Zbuild-std` 在 stable 上就編得出來（不需要 nightly），連結 wasi-sdk 34 的 libc；import 和 C 的一樣是 `@0.3.0`，`println!`、`Instant`、`std::thread::sleep`、wit-bindgen 的 async `start` 和 `spawn_local` 都能用。做法和要注意的事見 `README.md` 的「已決定的事項」第 40 條；原型也找到 `dom_wasm` 的一個 bug（第 41 條）。
- 還沒試：真正的 `kun:dom` WIT（`wasm` 分支 W7 的 C、Lua 的 runtime）。它們 import 整個 `kun:dom`，要等 `dom_wasm` 實作出所有函式才跑得起來，放在實作的第 3、4 步。

## Q11：效能

**為什麼重要**：DOM 的呼叫很頻繁，一個 frame 可能上千次；頁面的 script 也要快點開始執行。

**目前知道的**：
- 每次呼叫：`wasm` 分支在 V8 上量到 handle 形式比 `externref` 慢 1.7–3.3 倍；Bytecode Alliance 2026-06 的文章說跨 component 的同步呼叫因為 async 的 task 管理，有大約 3.5 倍的額外成本，1.0 之前要拿掉。
- 啟動：Cranelift 編得比 Liftoff 慢；Winch 不支援 `reference-types` 和 `gc`（只用 WIT 時用不到它們）。
- 執行檔和編譯時間：加上 wasmtime 和 Cranelift。

**量過的**（`cm-prototype` 的 `q2-bench`，2026-10-04，Apple M4，每種 100,000 次）：
- 一般的 sync DOM 呼叫（async Store 裡的 sync import）：54–57 ns。
- `dispatchEvent`（`async func`，呼叫回一個 listener）：`await` 1.76–1.79 µs，`block_on` 1.90–1.93 µs。大約是 sync Store 裡同步分派（452–473 ns）的 4 倍；不呼叫回 guest 的 `async func` 是 189–194 ns，所以大部分花在呼叫回 guest（`call_concurrent` 的 task）。
- 一般的 DOM 呼叫遠比分派頻繁，`dispatchEvent` 的成本可以接受；Component Model 1.0 前說要拿掉的同步呼叫成本，之後可以重量。

**其餘的**（`q11-prototype`，2026-10-04，Apple M4）：
- **啟動**（Lua guest，849 KB 的 component，到 instantiate 完）：wasmtime 的 Cranelift 28–33 ms，不最佳化 24–28 ms，Winch 10 ms（編得了 Lua，包括 exception handling），讀預先編好的程式碼（`Component::serialize`）0.3 ms；同一份 core module 在 V8 上，Liftoff lazy 0.4 ms（函式第一次執行時才編），Liftoff 全部先編 2.8 ms，TurboFan 53 ms。instantiate 兩邊都在 0.2 ms 以內。**Cranelift 比 Liftoff 慢十倍，一個頁面看得出來**；code cache 和 Winch 是兩個解法（`README.md` 的「分階段」第 6 步決定）。
- **每次呼叫**（host 什麼都不做，只量邊界）：wasmtime 的 component 沒有參數約 60 ns、兩個字串約 155 ns、回傳 handle 再 drop 約 145 ns；V8 的 core wasm 呼叫 Rust 函式是 10、61、25 ns。sync Store 或關掉 Component Model 的 async 都一樣，成本在 component 的邊界本身。一個 frame 一千次呼叫是 60–150 µs，可以接受，也比 `wasm` 分支現在的 binding（真正的 DOM 呼叫 770–2,550 ns）快得多。
- **執行檔和 build**：kun 加上 wasmtime 和 wasmtime-wasi（預設的 feature），乾淨的 release build 從 59 秒變成 135 秒，strip 之後的執行檔從 63.2 MB 變成 79.1 MB（多 16 MB）。
- **不跑 V8 省下的**：`V8::initialize` 加上一個帶 cppgc heap 的 isolate 和一個 context，約 0.85 ms、6 MiB（RSS 從 3.8 到 9.7 MiB）；kun 的 renderer 在 context 裝上 binding 之後更多。不連結 V8 的執行檔大小留給另一份 RFC。

## Q12：跟著 wasmtime 升級

**為什麼重要**：wasmtime 大約每個月一個大版本，2026-07 到 09 就出了三個；`cm-prototype` 寫到一半就碰到 49 的錯誤型別從 `anyhow::Error` 換成 `wasmtime::Error`。Component Model 的 async 和重新進入是最近改得最多的地方。

**目前知道的**：固定一個版本，定期升級；如果 Q2 走 fork，每次升級都要 rebase。Rust 的 toolchain 也要為了 `wasm32-wasip3` 升級（Q10）。

**怎麼釐清**：不需要原型。決定升級的節奏，寫進 `README.md`。

## Q13：wasmtime 的 collector 介面

**為什麼重要**：cppgc 當 wasmtime 的 GC（RFC 0005）要 fork 多少，取決於 wasmtime 裡 collector 是怎麼接進去的。

**目前知道的**：`Config::collector()` 只能選內建的 `DeferredReferenceCounting`、`Null`、`Copying`（`Auto` 現在是 `Copying`），沒有讓 embedder 提供的選項；內部的介面是 `GcRuntime` / `GcHeap`。GC 參照是 32 位元、相對於 GC heap 的索引，產生的程式碼每次存取都檢查邊界。

**結論**（2026-10-04，讀 wasmtime 49.0.2 的原始碼）：

一個 collector 在 wasmtime 裡分成四塊，現有的三個（DRC、copying、null）各寫一份：

| 哪裡 | 是什麼 | 現有的大小 |
| --- | --- | --- |
| `wasmtime` 的 `runtime/vm/gc/gc_runtime.rs` | `GcRuntime`（`layouts`、`new_gc_heap`）和 `GcHeap`：生命週期（`attach`、`detach`，帶一塊 `vm::Memory`）、no-GC scope、runtime 端的 barrier（`clone_gc_ref`、`drop_gc_ref`、`write_gc_ref`、`expose_gc_ref_to_wasm`）、`externref` 的 host 資料、header 和大小、配置（`alloc_raw`、struct、exception、array 和對應的 dealloc）、`gc(roots, trace_state)` 回傳一個可以一段一段做的 `GarbageCollection`、給 JIT 的 `vmctx_gc_heap_data` 和 `vmmemory`、`take_memory` / `replace_memory` | `runtime/vm/gc/enabled/` 的 `null.rs` 404 行、`copying.rs` 1,303 行、`drc.rs` 1,385 行 |
| `wasmtime-environ` 的 `gc.rs` | `GcTypeLayouts`：header 的大小和對齊、array 長度和 exception 欄位的位置、struct / array / exception 的 layout | `gc/` 的 `null.rs` 48 行、`drc.rs` 52 行、`copying.rs` 204 行 |
| `wasmtime-cranelift` 的 `func_environ/gc.rs` | `GcCompiler`：`layouts`、`is_moving_collector`、`alloc_uninit_array` / `alloc_struct` / `alloc_exn`、讀和寫 GC 參照時的 barrier（`translate_read_gc_reference`、`translate_write_gc_reference`）；共用的 lowering（欄位存取、轉型）在 `gc.rs` 本身（1,798 行） | `gc/` 的 `null.rs` 403 行、`copying.rs` 466 行、`drc.rs` 928 行 |
| `wasmtime` 的 `config.rs` | `Collector` 是封閉的 enum，對應到 environ 的 `EnvCollector` 和 runtime 的 `DrcCollector` / `NullCollector` / `CopyingCollector`，各自在一個 cargo feature 後面 | 幾十行 |

所以 **要 fork**（沒有讓 embedder 提供 collector 的介面，上游也沒有看到計畫），但**改動大多是新增**：照 null 和 DRC 的樣子寫一個 cppgc 版的 `GcHeap`、`GcTypeLayouts`、`GcCompiler`，再在 `Collector` 加一個值。原有的檔案改得少，rebase 的成本比改動 Cranelift 的核心小得多。

**和 wasmtime 的假設衝突的三個地方**（RFC 0005 真正的難題）：
1. **GC heap 是 wasmtime 自己配置、可以長大的一塊 `vm::Memory`**。JIT 經過 `vm_store_context.gc_heap`（基底和長度）存取，`VMGcRef` 是相對於基底的 32 位元索引，低位元 1 是 i31ref；配置失敗時回傳需要的大小，wasmtime 用 `take_memory` / `replace_memory` 把 heap 變大。cppgc 的 heap 是它自己的 cage（連續的 16 GiB，Q16），也自己長大。做法：`vmmemory` 回傳 cage 的基底和大小，`VMGcRef` 用 cppgc 的壓縮指標（Q16：物件 16 byte 對齊，低位元留給 i31），配置永遠不要求 wasmtime 長大；wasmtime 給的那塊 memory 不用。
2. **`GcHeap` 的安全約定是「每個 heap 是一個小 sandbox，heap 裡永遠不放 native 指標」**（`gc_runtime.rs` 的說明），而且實作「不可以加上新的安全前提」。cppgc 的 cage 裡有所有 Store 和 DOM，邊界檢查只擋得住 cage 之外，這個約定一定會被打破（Q17）。
3. **GC 由 wasmtime 發起，root 由 wasmtime 算**：配置失敗或明確呼叫時，`StoreOpaque::trace_roots`（`runtime/store/gc.rs`）收集 wasm 的 stack frame（stack map）、continuation、instance 的 table 和 global、host 的 `Rooted`、pending 的 exception，交給 `gc()`。換成 cppgc 之後，GC 由 cppgc 發起（DOM 配置也會觸發），要反過來：cppgc 標記時向每個 Store 要 root，fork 要把 `trace_roots` 開放出來，而它需要 Store 的 `&mut`。簡化的方向：**只在安全點 GC**（task 之間，所有 Store 都在 `StoreHolder` 裡、native stack 上沒有 wasm 的 frame），root 只剩每個 Store 的 table、global、host root 和暫停中的 fiber，由 holder 的 `trace()` 向 wasmtime 要。不跑 V8 的設定裡 GC 的時機由 kun 決定（Q18），做得到；有 JS 的設定裡 V8 隨時可能 GC（wasm 分派給 JS 的 listener、JS 配置），要另外處理（Q15）。

另外：cppgc 不移動物件，`is_moving_collector` 是 false；`externref` 在 wasmtime 裡是一個 GC 物件加上 side table 裡的 host 資料，只用 WIT 的 handle 時 kun 不會給 guest `externref`。

## Q14：GC 的 lowering

**為什麼重要**：wasm 的 `struct.new`、`struct.get` / `struct.set`、`array.*`、`ref.cast` 由 wasmtime 翻成 Cranelift IR，每種 collector 各有一份（配置、物件格式、barrier）；cppgc 版要寫一份新的。

**目前知道的**：在 `wasmtime-cranelift` 這一層，Cranelift 的核心應該大多不用動；`GcCompiler` 的方法和現有三份的大小見 Q13。cppgc 的寫入 barrier 有公開的 `HeapConsistency::DijkstraWriteBarrier`；kun 預設用 incremental marking（concurrent 是環境變數的選項）；kun 的 cppgc 參照是 32 位元的壓縮指標（`cppgc_enable_larger_cage`：cage 16 GiB、`地址 >> 3`），和 wasmtime 的 GC 參照一樣寬。

**結論**（`q14-prototype`，2026-10-04，讀 wasmtime 49.0.2 的原始碼，並把一個小的 wasm GC module 用三種 collector 編譯、反組譯）：
- **現在的成本**（arm64）：null 的 `struct.get` 是 null 檢查、`基底 + zext(ref)`（`add … uxtw`，基底的載入可以移出迴圈）、`ldr`，沒有邊界檢查（靠 guard 區）；`struct.set` 一樣，沒有 barrier。DRC 的寫入多十幾個指令（新舊兩邊的 ref count，減到零時呼叫 `drop_gc_ref`），讀取也有 barrier。配置：null 在 JIT 裡 bump，DRC 和 copying 呼叫 `gc_alloc_raw` libcall。
- **物件格式不用動共用的程式碼**：header 就是 wasmtime 的 `VMGcHeader`（kind、`VMSharedTypeIndex`，8 byte），array 的長度在第 8 byte，就是 null collector 的 layout，只是對齊到 16；`ref.test` / `ref.cast` 照 `VM_GC_HEADER_*` 讀 header，i31 的編碼（`value << 1 | 1`）和 null（0）都和 wasmtime 一樣。
- **參照換成位址要 fork 共用的程式碼**：wasmtime 到處假設 GC 參照是 heap 裡的 byte 索引：`FuncEnvironment::prepare_gc_ref_access`、陣列批次操作的基底、`emit_defensive_array_bounds_check`，runtime 那邊 `GcHeap` 用 `heap_index()` 的預設方法。cppgc 的解壓縮是 `(sext(ref) << 3) & 基底`（cage 對齊到 16 GiB、基底的低 34 位元存成 1），arm64 兩個指令；它對合法的指標等於 `(基底 − 16 GiB) + (zext(ref) << 3)`，一個指令（和現在一樣），但壞掉的參照會跑出 cage。照「已決定的事項」第 15 條至少留在 cage 裡，選 AND 的版本：**比現在多一個指令，不需要邊界檢查**。
- **寫入 barrier**：寫入之後讀 cppgc 的全域旗標（`WriteBarrier::write_barrier_enabled_`，位址放在 `VMStoreContext`），沒在 marking 時就結束，比 null 多兩個指令；marking 時呼叫 libcall，跳過 null 和 i31，再呼叫 `CombinedWriteBarrierSlow(slot)`。初始化的寫入不需要 barrier。
- **讀取**沒有 barrier，也**不需要 stack map**（GC 不是 wasmtime 發起的；stack 上的參照由安全點或 Q15 的表處理），共用程式碼宣告的 stack map 可以關掉。
- **配置**：`gc_alloc_raw` libcall（和 DRC 一樣）；cppgc 的快速路徑在 C++ 裡，JIT 不能像 null 那樣自己 bump。
- **型別表**：engine 登記型別時，把 layout 也登記進 `WasmGcObject` 用的 C++ 表（以 `VMSharedTypeIndex` 為索引）。
- **還沒決定**：`externref` 指向 DOM 節點時，要像 wasmtime 那樣包一個 GC 物件，還是直接指向節點（那樣節點經過 `any.convert_extern` 會進到 `anyref` 欄位，DOM 物件也要 16 byte 對齊）；配置經過 libcall 的實際成本要等真的接上 wasmtime 才能量。

## Q15：stack 上的 root

**為什麼重要**：GC 時，wasm 的 stack 上拿著的 GC 參照要算是活的。wasmtime 的 collector 用精確的 stack map；cppgc 掃描 native stack，而且是保守的。

**目前知道的**：wasm 的 frame 在 native stack 上，但 Cranelift 把參照存成 32 位元的值；cppgc 的保守掃描認不認得 32 位元的壓縮指標還沒確認。WASI 0.3 的等待讓 guest 停在 fiber 上，fiber 的 stack 不在 native stack 上。wasmtime 自己有精確的 root（`trace_roots`，Q13）；如果只在安全點 GC（Q13 的第 3 點），native stack 上沒有 wasm 的 frame，這一題只剩「有 JS 的設定裡，V8 在 wasm 的 frame 還在 stack 上時 GC」這種情況。

**做法**：
- **不跑 V8 的設定**：只在安全點 GC（Q13 的第 3 點、Q18）。native stack 上沒有 wasm 的 frame，暫停中的 fiber 和 table、global 由 holder 的 `trace()` 向 wasmtime 要（`trace_roots`）。不需要保守掃描認得 wasm 的參照。
- **有 V8 的設定（退路，2026-10-04 的看法）**：V8 隨時可能 GC，wasm 的 frame 可能還在 stack 上。**正在執行的 Store，這次 GC 把它的物件全部當成活的**，只回收沒在執行的 Store 的垃圾：
  - 「正在執行」就是 Store 表裡那一列是空的（Store 被取出、在 stack 更外層，Q5）；
  - cppgc 不能列舉某個 Store 的所有物件，所以 holder 要記得自己配置過的物件：一張弱參照的表，Store 沒在執行時是弱的，正在執行時 `trace()` 把它們全部標成活的；
  - 回收得比較粗（正在執行的 Store 的垃圾等下一次 GC），但不需要知道 wasm 的 stack 上有什麼；
  - 原型確認成立（`q15-prototype`，2026-10-04，見下面的結論）。
- **還有用的實驗**：如果退路的成本太高（目前看來不高），再回到原本的問題：cppgc 的保守掃描認不認得 32 位元的壓縮指標（只在 stack 上留一個物件的 32 位元壓縮值，用「stack 上可能有 heap 指標」的方式 GC，看物件活不活），或 fork cppgc、在 GC 時用 wasmtime 的 stack map 回報 root。

**退路的結論**（`q15-prototype`，2026-10-04）：成立。
- **做法**：rusty_v8 fork 的 C++ 類別 `WeakObjectTable`，每個 Store 一張，由 holder 指著，每筆 4 byte（壓縮指標）。Store 沒在執行時，trace 只登記一個 weak callback，GC 收尾時刪掉死的；執行中（`enter`、`exit`，可以巢狀）trace 走過整張表，在 concurrent marker 上時延到主 thread 做；marking 進行到一半才進入時，用 Steele barrier 讓表再被 trace 一次，執行中新配置的物件用 Dijkstra barrier 馬上標記。
- **表也要記 host 交給 Store 的物件**（DOM 節點，之後的 `externref`）：不記的話，wasm stack 上拿著的節點在從文件上被拿掉之後就被回收了（原型的 `fuzz-unrecorded` 失敗）。`kun:dom` 每個回傳節點的函式都要做；重複交進來的，表在 GC 收尾時去掉。
- **驗證**：3 個 Store、巢狀的呼叫、GC 一步一步推進並在任意時刻收尾（300 次裡約 280 次有 Store 正在執行）、不掃描 stack：所有 frame 的 stack 上和從那裡到得了的物件都完好；Store 都不執行之後，每張表剛好剩下活的物件。主 thread marking 和全部 concurrent marking 都通過；拿掉 Steele barrier、Dijkstra barrier 或執行中的強 trace，測試會失敗。
- **成本**：每次配置多約 1.5 ns（6.5 → 8 ns）、4 byte；`enter` 加 `exit` 3.5 ns；marking 中途進入時多 trace 一次表，10 萬筆約 0.1 ms。執行中的 Store 的垃圾等到下一次 GC。

## Q16：cppgc 上的 wasm GC 物件

**為什麼重要**：wasm 的 struct 和 array 要變成 cppgc 上的物件：不定長度，`trace()` 照 wasmtime 的型別資訊找出參照欄位。

**結論**（`q16-prototype`，2026-10-04）：做得到。
- **格式**：wasm 物件是 rusty_v8 fork 的 C++ 類別 `WasmGcObject`：header 是型別索引和長度（8 byte），欄位接在後面，配置時在建構子裡清成 0；`Trace` 在 C++ 裡照 Rust 登記的型別表走。參照欄位就是 cppgc 的 `Member`（壓縮指標），barrier 用 cppgc 的，分三種：指向 wasm 物件、指向 host 物件（DOM 節點，經過 `RustObj` 的 virtual `Trace`）、i31 或 wasm 物件。一個物件比欄位多 16 byte（cppgc 的 header、自己的 header，對齊到 16）：4 個欄位的 struct 約 33 byte。第一版經過 rusty_v8 的 `RustObj` 是 82 byte，從 `&self` 讀欄位也是 Stacked Borrows 不允許的寫法，所以改成 C++。
- **kun 的壓縮指標的 shift 是 3**（`cppgc_enable_larger_cage` 預設打開，cage 16 GiB），8 byte 對齊的物件壓縮之後最低位元可能是 1，和 i31 的 tag 撞在一起。**wasm 物件一律 16 byte 對齊**就解決（最多多 8 byte）。之前這裡和 `details.md` 寫的 shift 1、4 GiB 是錯的。
- **`anyref` 的 trace 只讀一次**：heap 的 thread 隨時可能把 i31 寫到指標上，cppgc 的 trace 會再讀一次 slot，所以讀到的值放進 stack 上的 `Member` 再交給 cppgc；i31 的 0 編碼成 1，正好是 cppgc 的 sentinel，絕不能交給 cppgc。
- **`Member` 要寫明目標的類別**：cppgc 從物件 header 找 trace 的路（mixin）只對 mixin 類別有效，`WasmGcObject` 和 `RustObj` 都不是。
- **young generation 一定要關著**（kun 的 V8 編進了 `CPPGC_YOUNG_GENERATION`，執行時預設不開）：開了之後，寫入 barrier 記下的 slot 會被當成指標讀回來，放著 i31 的欄位會被讀錯。
- **驗證**：wasm 物件和節點的循環被回收；隨機的圖（約 2.4 萬個可達的物件）在 incremental GC 的每個 marking 步驟之前改寫 50 次（i31 和物件互換、長 array 在 marking 途中被改寫），主 thread marking、全部 concurrent marking、`StandaloneHeap` 自己排 GC 三種都沒有漏標，最後剩下的剛好是可達的；拿掉 barrier，測試會失敗。
- **還沒做**：和真正的 `dom` 節點互相指向（等 `dom` 能放在 `StandaloneHeap` 上）；debug 版的 V8（`anyref` 的 trace 在 marker thread 上建立暫時的 `Member`，debug 的檢查可能不接受）。

## Q17：sandbox

**為什麼重要**：原版 wasmtime 把 GC heap 當成不可信任的記憶體（獨立的區域、索引、邊界檢查）；換成 cppgc 之後，邊界變成整個 cage，裡面也有 DOM 節點（`details.md` 的「未來考慮：wasmtime 加上 cppgc collector」）。

**結論**（2026-10-04，`README.md` 的「已決定的事項」第 15 條）：先接受，安全程度和現在沒開 sandbox 的 V8 一樣。之後補 sandbox 的做法：wasm 的物件放在 cage 裡自己的區域，產生的程式碼只能存取這個區域、照樣檢查邊界；指向 DOM 一律經過 handle 表（像開了 sandbox 的 V8 的 `CppHeapPointerTable`），每次存取多一次查表。到時候再量查表的成本。

## Q18：不接 isolate 的 cppgc heap

**為什麼重要**：不跑 V8 的設定裡（`README.md` 的「不跑 V8 的設定」），DOM 的 cppgc heap 不接 isolate；現在由 V8 決定什麼時候 GC、什麼時候做 incremental marking，到時候要由 renderer 自己排。

**目前知道的**：`dom` 已經有不接 isolate 的 heap（`crates/dom/src/isolate.rs` 的 `Backing::Detached`），現在只給測試用，GC 由測試一步一步推進（cppgc 的 testing API）。

**結論**（`q18-prototype`，2026-10-04）：
- **不接 isolate 的 `CppHeap` 不會自己 GC**：`CppHeap::IsGCAllowed()` 要有 isolate（V8 的 `src/heap/cppgc-js/cpp-heap.cc`），配置量的回報和配置失敗時的 GC 都直接返回。實測配置 40 萬個沒有參照的節點、從不呼叫 GC，沒有一個被 finalize，記憶體從 18 MiB 長到 128 MiB；一次 GC 全部回收。所以「只在安全點 GC」成立，代價是 kun 一定要自己收。
- **用 testing API 自己排程，收尾太慢**：marking 一段一段做，每段 50 µs 以內；但收尾一次 sweep 完（`FinalizeIncrementalGarbageCollectionForTesting` 是 `CollectGarbageForTesting` 加上 `sweeper_.FinishIfRunning()`），10 萬個活的加 10 萬個垃圾 `<div>` 的頁面停頓約 23 ms，和一次做完的 GC 差不多，超過一個 frame。不接 isolate 的 `CppHeap` 沒有非 testing 的 GC 入口。
- **方向：單獨使用的 `cppgc::Heap`**：它有自己的 GC 排程；選 `StackSupport::kNoConservativeStackScan` 時，會經過 embedder 提供的 Platform 用 non-nestable task 發起 GC，cppgc 保證這種 task 的 stack 上沒有東西（`include/cppgc/heap.h`），incremental 和 concurrent 的 sweep 也經過同一個 Platform。kun 的 Platform 把 task 排進 renderer 的 queue，GC 就發生在 task 之間。現在連結的 V8 程式庫裡就有它（`cppgc::Heap::Create` 是 exported 的 symbol），將來不連結 V8 時，單獨的 cppgc 也是同一個 API。

- **單獨的 `cppgc::Heap` 成立**（`q18-prototype` 的 `standalone`，2026-10-04）：rusty_v8 fork 加了 `StandaloneHeap`（還沒發布，見原型的 README）：前景 task 交給 Rust 的 `PlatformImpl`，worker 的 job 用 V8 的 platform。原型不呼叫 `V8::initialize`、從不呼叫 GC，每個「page task」配置 1,000 個 `<div>`，task 之間執行 cppgc 排進來的 task：cppgc 自己收，heap 穩定（10 萬個活的節點加 200 萬個垃圾，記憶體 26 MiB）；最長的 GC task 1.3–1.9 ms（1 萬個活的節點 0.35 ms），testing API 的收尾是約 23 ms；留下的節點都還在。
- **還沒做**：DOM 本身放上去（`dom` 加一種 backing、配置改用 `allocation_handle`、renderer 的 task queue 實作 `PlatformImpl`），放在實作的第 7 步；fork 的改動要發布才能進 kun。
