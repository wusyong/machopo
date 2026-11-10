# RFC 0093 的進度：wasm 文件接回 renderer：用 `dom_wasm` 做 TodoMVC

開始日期：2026-11-10

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a guest 和 `dom_wasm` 的測試換成 WASI 0.3 和 async 的 Store（決定 5、6） | 完成 | 2026-11-10 |
| 1b WASI 0.3 的 linker：自己的時鐘，stdout 和 stderr 交給 sink（決定 7）；一輪的 poll（決定 9） | 完成 | 2026-11-10 |
| 1c-1 engine 擁有的 DOM：`WasmDom::take`、`lend`（決定 10） | 完成 | 2026-11-10 |
| 1c-2 `dom_wasm` 的 engine：`ScriptEngine`、`ScriptHost`、需要時才跑的 event loop、`start(document)`（決定 3、4、8、9） | 還沒開始 | |
| 1d renderer：prescan `<meta>`、選 engine、換頁時換 engine、載入 `application/wasm` 的 script（決定 2） | 還沒開始 | |
| 1e 靜態的 TodoMVC 的 guest、情境、renderer 的測試 | 還沒開始 | |
| 2 節點以外的物件的 handle、放開凍結、TodoMVC 用到的成員 | 還沒開始 | |
| 3 事件和 callback | 還沒開始 | |
| 4 TodoMVC 的 guest、情境 | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 開始前（2026-11-10，macOS，Apple Silicon）

- 重入和等待的原型（`reenter-prototype/`，結果在 `details.md` 的「重入的原型」）：手寫的 WAT component，wasmtime 的 fork 50.0.1，沒有經過 `StoreSet` 和 elise 的 collector。
- `wit-bindgen` 0.62.0 的 C binding 對 `async func` 產生的 header 看過（`details.md` 的「C 的 guest 和 WASI 0.3 的工具鏈」）。

### 1a（2026-11-10，macOS，Apple Silicon）

- wasi-sdk 34.0（macOS arm64，release 的 sha256 核對過）放在 `../third_party/wasi-sdk-34.0`，`wit-bindgen` 0.62.0。
- `scripts/ci.py` 的 guest 的 target 從 `wasm32-wasip2` 改成 `wasm32-wasip3`，`python3 scripts/ci.py guests` 重編 `crates/dom_wasm/tests/guests/dom.wasm`（原始碼 `dom.c` 沒有改；之前那份是 Linux 上編的）。
- 測試的 harness（`crates/dom_wasm/tests/it/guest.rs`）：linker 換成 wasmtime-wasi 的 p3（只要 p3，不用再加 p2），`instantiate_async`、`call_async`，在 `StoreSet` 的一次借出裡 poll 到完成（`block_on`：不靠 waker、不用 executor，一百萬次 poll 還沒完成就失敗）。
- 分兩步確認，各跑一次 `cargo test -p dom_wasm`，都全部通過：先只改 harness（guest 還是 WASI 0.2 的）、再換 guest 和 linker。`shared` 的 GC 的 case 也通過：guest 的呼叫現在在 fiber 上跑，GC 經過 Store、`StoreSet` 交出別的 Store 的 root（開始前的原型沒涵蓋的那一塊）。
- mutation test：沒有跑。這一步沒有改實作（`crates/dom_wasm/src` 沒動），改的是測試的 harness、編 guest 的 target 和編好的 guest。
- 還沒確認的：guest 沒有印任何東西，wasmtime-wasi 的 stdout、stderr 在沒有 tokio 的 runtime 時能不能用要到 1b 才知道。
- `sanity.py` 的 `tests`、`unsafe`、`numerics`、`docs` 通過。

### 1b（2026-11-10，macOS，Apple Silicon）

- 新的：`crates/dom_wasm/src/wasi.rs`（`add_wasi_to_linker`、`WasmWasi`、`GuestClock`、輸出的 sink）和 `src/turn.rs`（`poll_until_idle`）。`dom_wasm` 多依賴 wasmtime-wasi（原本只有測試用）和 tokio（只用 `AsyncWrite` 這個 trait，沒有 runtime）。接到 console 是 1c 的事：這一步的 sink 是 embedder 給的函式。
- 測試的 guest：world 多一個 `async func` 的 export `sleep`（C 的 callback ABI，第一個實際編過的 async export：在裡面呼叫 libc 的 `nanosleep`，它是時鐘的 `wait-for`），`run` 多四個 case（`is-awake`、`is-asleep`、`print`、`coarse-clock`）。
- 新的測試：`tests/it/wasi.rs` 五個（等待是期限、期限之前的一輪不叫醒、之後的一輪才醒、等待期間照樣能呼叫 guest；不用等的等待；兩個 Store 各自的等待；時鐘降到 100 µs；輸出一行一行、分 stdout 和 stderr、沒有換行的最後一行在 Store 丟掉時來），`src/wasi.rs` 三個、`src/turn.rs` 兩個單元測試。`cargo test -p dom_wasm`：12 個單元測試、21 個整合測試通過。
- 確認了開始前沒把握的三件事：
  - **stdout、stderr 沒有 tokio 的 runtime 也能用**：guest 的 `printf` 經過 wasmtime-wasi 的 p3 到我們的 `AsyncWrite`，測試的 thread 沒有 runtime。
  - **等待中的 guest 留在 Store 裡**，經過 `StoreSet` 的借出和 elise 的 collector 也一樣（開始前的原型是手寫的 WAT、沒有 `StoreSet`）：`run_concurrent` 的 future 丟掉之後，下一輪新的 `run_concurrent` 接著跑它。
  - **wasmtime 只 poll 被叫醒的 host 函式的 future**：新的一輪不會自己重新 poll 上一輪 pending 的等待，所以 `GuestClock` 要留著那個 future 的 waker、到期時叫醒（`wake_due`），不是只看時間。
- 行為上要知道的：沒有換行的最後一行不是寫的當下給 sink，而是輸出關閉時（WASI 的 stream 沒有 flush，C 的 `fflush` 到不了 host）。記在 `crates/dom_wasm/README.md`。
- mutation test（`mutants-1b.toml`，20 條）：第一次 16 條抓到、1 條卡住（一輪永遠不結束，等到逾時）、3 條沒抓到：`next_deadline` 拿最晚的而不是最早的（整合測試每個 Store 只有一個等待）、結束的等待沒有從清單拿掉（只有沒人叫醒就被 poll 時才走到，guest 的測試走不到）、一輪只 poll 一次（guest 的測試裡剛好一次就夠）。補了五個單元測試之後 20 條都抓到，「永遠不結束」那條也改由單元測試數 poll 的次數當場抓到。跑完確認檔案都還原了。
- 還沒做的：guest 讓出（callback code 的 `YIELD`）時在同一輪繼續 poll 的測試（決定 9 提到的 wasmtime 49 的 bug）：C 的 guest 要自己寫出會讓出的 async export，放到 1c 和入口一起。
- `sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`docs` 通過。

### 1c-1（2026-11-10，macOS，Apple Silicon）

- 開始 1c 前讀 `V8Engine` 發現的：renderer 要 `&mut Dom`（`ScriptEngine::dom_mut`），`dom_wasm` 的 DOM 在 thread-local 的 `RefCell` 裡，給不出來。決定 10：engine 擁有 DOM、跑 guest 時借進 thread-local。
- `crates/dom_wasm/src/lib.rs`：thread-local 的 DOM 變成可以不在（`Option`），`WasmDom::take`、`WasmDom::lend`；`Slot::of`（每個 host 函式找 Store 的狀態的地方）在 DOM 不在時回沒有，provider 借 heap 時也是。
- 新的測試 `tests/it/lend.rs` 六個。`cargo test -p dom_wasm`：12 個單元測試、26 個整合測試通過。
- 確認了：`Dom` 搬進搬出之後 heap 照常運作；DOM 在 engine 手上時的 full GC 經過 `StoreSet` 拿到 Store 的 root（guest 拿著的節點、另一個 Store 的 GC 物件都留著）。
- 找到的規則：`StoreSet` 也要在借出期間丟掉。第一版的測試在借出之外 `drop(set)`，有 GC heap 的 Store 這時放不掉 DOM 的 heap 的 root，DOM drop 時 elise panic（`1 roots outlive their heap`）。寫進 `crates/dom_wasm/README.md`，1c-2 的 engine 要照做。
- mutation test（`mutants-1c-1.toml`，5 條）：第一次 1 條沒抓到：`Slot::of` 不看 DOM 在不在。guest 的呼叫在進 wasm 之前 fork 就先檢查 heap 借不借得到，走不到 host 函式；走得到的是 Store 在 DOM 不在時 drop。補了一個測試之後 5 條都抓到。跑完確認檔案都還原了。
