# RFC 0093 的進度：wasm 文件接回 renderer：用 `dom_wasm` 做 TodoMVC

開始日期：2026-11-10

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a guest 和 `dom_wasm` 的測試換成 WASI 0.3 和 async 的 Store（決定 5、6） | 完成 | 2026-11-10 |
| 1b WASI 0.3 的 linker：自己的時鐘，stdout 和 stderr 交給 sink（決定 7）；一輪的 poll（決定 9） | 完成 | 2026-11-10 |
| 1c-1 engine 擁有的 DOM：`WasmDom::take`、`lend`（決定 10） | 完成 | 2026-11-10 |
| 1c-2 `dom_wasm` 的 engine：`ScriptEngine`、`ScriptHost`、需要時才跑的 event loop、`start(document)`（決定 3、4、8、9） | 完成 | 2026-11-10 |
| 1d renderer：prescan `<meta>`、選 engine、換頁時換 engine、載入 `application/wasm` 的 script（決定 2） | 完成 | 2026-11-10 |
| 1e 靜態的 TodoMVC 的 guest、情境、`shell` 的測試；第 1 段的檢查 | 完成 | 2026-11-10 |
| 2a handle 的表指任何物件、記 interface（決定 12） | 完成 | 2026-11-10 |
| 2b WIT 的清單加 TodoMVC 用到的成員，`list<string>`、可省略的 dictionary（決定 13）；第 2 段的檢查 | 完成 | 2026-11-10 |
| 3a wasmtime 的 fork：set 的 Store 在 fiber 上接受同步的呼叫（決定 14） | fork 已 commit（`ef4e892`，本地的 `machopo` 分支，還沒 push），`third_party.toml` 釘到它 | 2026-11-10 |
| 3b listener：`add`/`remove`、`handle-event`、事件的成員（決定 15、16）；第 3 段的檢查 | 完成 | 2026-11-10 |
| 4 TodoMVC 的 guest、情境（決定 17）；第 4 段的檢查 | 完成 | 2026-11-10 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| wasm 的 TodoMVC 能新增、勾選、全部勾選、編輯（Enter、Escape、離開欄位、改成空的）、刪除、清除已完成、三種篩選 | `python3 scripts/test.py scenarios todomvc-app-wasm` |
| 同樣的步驟對 JS 版結果一樣 | `python3 scripts/test.py scenarios todomvc-app-js`；兩份情境逐行相同：`cargo test -p shell --test it the_todomvc_scenarios` |
| 在真的視窗裡也一樣 | `python3 scripts/test.py automation todomvc-app wasm-listeners wasm-document`（要顯示器和 GPU） |
| guest 的 listener：呼叫順序、capture、`preventDefault`、`stopPropagation`、`remove`、trap、巢狀的分派、事件 handle 的期限 | `cargo test -p dom_wasm --test it engine`；真的輸入：`python3 scripts/test.py scenarios wasm-listeners` |
| wasm 文件：只跑 wasm、不跑 JS，guest 的等待，換 engine | `python3 scripts/test.py scenarios wasm-document`；`cargo test -p shell --test it wasm_documents` |
| guest 畫的 TodoMVC 和寫死的 markup 畫出來逐像素相同 | `cargo test -p shell --test it the_wasm_todomvc`；`python3 scripts/test.py scenarios todomvc-wasm` |

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

### 1c-2（2026-11-10，macOS，Apple Silicon）

- 新的：`crates/dom_wasm/src/engine.rs`（`WasmEngine`，`dom_api::ScriptEngine` 的實作）、`crates/dom_wasm/wit/script.wit`（頁面的 script 的 world：import `dom`、export `async func` 的 `start(document)`）、`WasmStore::give`（把頁面的 document 交給 Store）。`dom_api`：`ScriptEngine::run_wasm_script`（預設什麼都不做，`V8Engine` 不用改）和 `WasmScript`；`until_next_due` 從 `dom_js` 的 `window.rs` 搬過來，兩個 engine 共用（決定 4）。
- `scripts/ci.py`：每個 guest 自己的 world（`GUEST_WORLDS`），`machopo:dom` 的 WIT 整個資料夾當 dependency。新的 guest `crates/dom_wasm/tests/guests/page.c`（world 是 `script`）。
- 設計上照預設做的：script 在 main thread 上同步編譯（沒有 compile thread）；Store 留到換頁才丟（`start` 回來、沒在等待的也是；第 3 段有 listener 之後再定什麼時候能丟）；只有最上層的 frame。
- 新的測試 `tests/it/engine.rs` 七個。`cargo test -p dom_wasm`：12 個單元測試、33 個整合測試通過。`cargo test -p dom_api -p dom_js` 通過（`until_next_due` 搬家）。
- 確認了：
  - **C 的 guest 的 `async func` 入口照 callback ABI 寫得出來、跑得起來**，`start` 裡呼叫 libc 的 `nanosleep` 就是等待。
  - **guest 讓出（`YIELD`）時同一輪接著跑完**：`poll_until_idle` 在 waker 被叫醒時繼續 poll。決定 9 提到的 wasmtime 49 的 bug（丟掉 future 會丟掉已取出的工作）照這個做法碰不到；mutation「一輪只 poll 一次」被這個測試抓到。
  - **engine 手上的 DOM 在 script 跑過之後照常用**（讀樹、full GC）。
- 找到的規則：拿著 DOM 的 root（`NodeRoot`）的人要比 engine 先 drop。測試第一版的 `Page` 把 `document` 放在 `engine` 後面，engine 的 DOM drop 時 elise panic（`1 roots outlive their heap`）；`V8Engine` 也是這個規則，renderer 本來就照做。
- mutation test（`mutants-1c-2.toml`，18 條）：17 條抓到，1 條沒抓到：trap 的 Store 沒有從 `StoreSet` 拿掉。它只是留到換頁才丟，從外面看不出來，記進 `docs/test-gaps.md`。跑完確認檔案都還原了。
- 沒有測試守著的：engine 轉給 `dom_api` 的那些方法（輸入、focus、生命週期、元素的 task、動畫、WebDriver 的查詢），要 renderer 才走得到，記進 `docs/test-gaps.md`，1d、1e 的情境會走到。
- `sanity.py boundary` 抓到的：第一版把 wasmtime 的 `Engine` 和 linker 放在 process 共用的 `static`（每個 renderer 共用）。改成每個 `WasmEngine` 自己的，第一個 script 時才建；改完重跑測試，mutation 的 spec 裡碰到那一段的三條重跑（其中一條的原文跟著改）、照樣抓到。
- `sanity.py` 的 `deps`、`unsafe`、`tests`、`numerics`、`boundary`、`spans`、`docs` 通過。

### 1d（2026-11-10，macOS，Apple Silicon）

- renderer（`crates/renderer/src`）：`encoding.rs` 的 `declares_wasm_scripts`（用原本就有的 `prescan_metas`）；`render_thread/documents.rs` 在 commit 時照它選 engine，和頁面現在的不同種時 `replace_engine`（前一份文件 unload、agent 和 renderer 拿著的前一份 DOM 的東西放掉、`Frame` 拆開把文字的 context 和 widget 交給下一個 engine 的 frame）；wasm 文件的 parser 的 script 用 `dom_api::prepare_wasm_script` 準備，當成 deferred 的 script 下載（MIME 要是 `application/wasm`）、交給 `run_wasm_script`。`ScriptEngineFactory` 多 `wasm_engine`（`shell` 裝 `WasmEngine::boxed`）。
- `dom_api`：`Prepared::Wasm`、`prepare_wasm_script`、`ScriptEngine::take_layout`（兩個 engine 都實作）。
- `dom_wasm` 的 engine 補了兩件接上 renderer 才發現的事：
  - **連結點了沒反應**：DOM 的工作要 host 導覽時經過 `ScriptHost::navigate`，預設什麼都不做；`V8Engine` 把它變成 `FrameRequest::Navigation`，wasm 的 host 沒有。補上，`take_frame_requests` 交出去。
  - **等待的 guest 時間到了不會醒**：renderer 的 loop 照 `until_next_timer` 醒來，但跑的是 `run_due_tasks`，不是 `run_timers`。到期的 guest 的那一輪改在 `run_due_tasks` 裡跑（`run_timers` 也留著）。
- 新的測試：情境 `tests/scenarios/wasm-document.txt`（頁面 `pages/wasm-hello.html`、`wasm-sleep.html`、`wasm-js.html`：wasm 文件跑 wasm、不跑 JS，guest 加的段落排版出來；點連結到 JS 文件、再點連結到 wasm 文件、再輸入網址回 JS 文件，各跑各的 script；等待的 guest 接著跑）；`shell` 的 `tests/it/wasm_documents.rs` 五個（跑了而且畫出來、MIME 不對和不存在的是 failed subresource 而且沒跑、沒宣告或不是 `text/html` 的文件不抓也不跑 wasm、wasm 文件不抓也不跑 JS）；`renderer` 的 `encoding` 一個單元測試。
- 跑過：`cargo test -p renderer -p shell -p browser -p dom_api`（改完 renderer 之後、補測試之前）全部通過；之後 `cargo test -p dom_wasm`、`-p shell --test it wasm_documents`、`-p renderer --lib a_meta_declares` 和 `test.py scenarios wasm-document` 通過。
- mutation test（`mutants-1d.toml`，13 條，各用最相關的測試）：13 條都抓到。跑完確認檔案都還原了。
- 沒有測試守著的（記進 `docs/test-gaps.md`）：wasm 文件裡鍵盤、focus、表單這些輸入；DevTools 連著時的行為和換 engine；wasm script 的 `load`、`error` 事件。`docs/limitations.md` 的 wasm 文件那一條改寫成現在的樣子，舊的 wasm 實作留下的幾句拿掉。

### 1e 和第 1 段的檢查（2026-11-10，macOS，Apple Silicon）

- 決定 11：Speedometer 3.1 的 vanilla ES5 TodoMVC 的 `dist/` 原樣複製到 `tests/sites/todomvc/`（連同授權），`tests/sites/README.md` 記來源。
- 新的：guest `crates/dom_wasm/tests/guests/todomvc.c`（`start` 建三個 todo 的 `<li>`、照 JS 版的樣板，填計數；用到的都是現有的成員：`query-selector`、`create-element`、`append-child`、`set-attribute`、`set-text-content`、`set-inner-html`、`release`）；頁面 `tests/scenarios/pages/todomvc-wasm.html`（JS 版的 markup 和樣式表，script 換成 guest）和 `todomvc-static.html`（同樣的 todo 直接寫在 markup 裡，沒有 script）。`ci.py guests` 重編三個 guest，原本的兩個 `.wasm` 逐 byte 沒變。
- 新的測試：`shell` 的 `wasm_documents::the_wasm_todomvc_is_drawn_as_its_markup_written_out`（兩頁的像素一樣，而且和 JS 版沒有 todo 的畫面不一樣）、`sites::todomvc_loads_its_style_sheets_and_scripts`（JS 版載得起來）；情境 `todomvc-wasm`（每個 todo 的文字、完成的那個、計數；再點一個勾選框、在輸入框打字，確認頁面自己的行為沒有 script 也照常，結果只留截圖）。
- 不穩定的測試，查到原因：像素比對在 `cargo test` 裡偶爾失敗（三次裡一次），單獨用 `machopo --headless` 跑八次都一樣。印出不同的像素，都在三個 `label` 的文字上；再比「靜態頁對靜態頁」六次有三次不同、「wasm 頁對 wasm 頁」六次都一樣，所以不穩的是靜態頁自己，和 wasm 無關。TodoMVC 的 CSS 給 `label` 一個 `transition: color 0.4s`；兩頁都關掉它之後，靜態頁對靜態頁 18 次都一樣，整組 `wasm_documents` 和 `sites` 連跑 40 次沒有失敗。推測是靜態頁的 `label` 在樣式表到之前被算過一次 style，樣式表到了顏色才過渡（wasm 頁的 `label` 是樣式表到了之後才建的）；什麼時候會先算那一次沒有查，記進 `docs/limitations.md`。沒有用重跑蓋過去。
- mutation test：這一步沒有新的實作（Rust 的程式沒有改，除了測試），沒有跑。guest 的 C 由像素比對和情境守著。
- 第 1 段的檢查：
  - `cargo fmt --check`：第 1 段的程式有 30 處沒排好，`cargo fmt` 排過；之後四份 mutation 的 spec 的原文都還對得上。
  - `cargo clippy --workspace --all-targets -- -D warnings`：抓到兩種，都修了：`crates/dom_wasm/src/wasi.rs` 時鐘降精度的減法沒寫明溢位（改成 `saturating_sub`，`mutants-1b.toml` 那一條的原文跟著改），`crates/dom_wasm/tests/it/engine.rs` 三處 `fetch_add`（workspace 禁用，改成讀了再存）。之後通過。
  - `cargo test --workspace`：通過（55 個測試 binary，3789 個測試通過、1 個 ignored）。
  - `test.py scenarios todomvc-wasm wasm-document` 通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。
  - 沒有跑的：`test.py automation`（視窗那一層沒有動）、整套的情境、reftest 和 WPT（由 CI 跑）。

### 2a（2026-11-10，macOS，Apple Silicon）

- 開始前算過成員的分類（暫時的測試，沒有留）：表上 2799 個成員、plain 163 個、WIT 裡 68 個；放寬成「任何 interface 的物件、字串、數字、布林」之後 1757 個符合。更正了 `details.md`：`value`、`checked`、`blur()` 不是被凍結擋住，是元素的 interface 整個被排除。決定 12、13。
- `crates/dom_wasm/src/handles.rs` 改寫：格子是 `Member<Erased>`，heap 外記 interface 的編號（取代節點的種類）和它在共用的表的哪一項；`give` 收 `ObjectRef`。`front.rs`：`this` 的檢查改成 `dom_api::typed::is_interface`（新的，查 `DESCENDANTS` 的一個 bit）、`this_as` 和 `ret_object` 能處理任何物件、節點的參數檢查是不是節點的 handle。`dom`：`ObjectRef::erase`、`Cx::object_engine_word`、`set_object_engine_word`。WIT 沒有變（golden 的測試照舊通過）。
- 做的時候改的設計：原本打算 release 時清掉物件的 word，但 release 手上只有 `Gc<Erased>`、不知道型別，拿不到物件放 word 的那一格。改成不清、用的時候對過才算數（main 的對 Store 的表，共用的那一項對它自己的一個 handle），每一格記著自己的那一項。原本的兩個單元測試跟著改（word 留著）。
- 新的單元測試兩個（任何物件的 handle 和 interface、過時的 word）。`cargo test -p dom_wasm`：14 個單元測試、33 個整合測試通過；`shell` 的 `wasm_documents`、情境 `todomvc-wasm`、`wasm-document` 通過。
- mutation test（`mutants-2a.toml`，11 條）：11 條都抓到。跑完確認檔案都還原了。
- 還沒有測試的：把不是節點的 handle 當節點的參數傳（`check_node` 的 trap）：現在的 WIT 交不出不是節點的 handle，2b 有了之後補。

### 2b 和第 2 段的檢查（2026-11-10，macOS，Apple Silicon）

- `crates/dom_wasm/src/wit.rs`：`ADDED`（40 個名稱）和 `can_hold`（放寬的規則）；`describe` 多 `type object = u32`、`type node = object`、成員提到的非節點 interface 的別名，`Strings` 是 `list<string>`，可省略的 dictionary 不放。版本 `0.2.0`（`wit/script.wit`、測試的 world、`ci.py` 跟著改）。WIT 從 70 個函式變成 110 個。
- `src/lib.rs` 的 `member!`：清單上每個成員一行（只有有自己一行的、或 plain 的才編成 host 函式；`for_each_member!` 的分類不用改）。`src/front.rs`：`u32`、`bool`、可省略的 `bool`、不定個數的字串、省略的 `FocusOptions`、非節點 interface 的回傳值（`objects!`）、`call_listener`（什麼都不做）。
- 找到的 bug，修了：**guest 的操作沒有帶頁面的 layout**。`focus()` 在 renderer 裡沒有效果（`shell` 的畫面測試抓到：focus 和 blur 兩頁的像素一樣）：`WasmFront` 開操作時用的是沒有 layout 的 `ApiCx`，V8 的 front 每次都帶著。layout 在 engine 手上、thread-local 拿不到，所以加了 `WasmDom::lend_page`：借 DOM 時把 engine 的 `LayoutCell` 的內容一起搬進 thread-local、結束時搬回去。1c、1d 的測試沒抓到是因為那時 WIT 裡沒有要看排版的成員。
- 照決定 13 的清單做、有出入的：事件的成員留到第 3 段（guest 拿不到 event，加了測不到；原本加了 9 個又拿掉，`event-target` 這個函式名還和 `EventTarget` 的型別別名撞名，第 3 段要處理）；`Location.href` 只有 getter（setter 的型別是另一種）；`Window` 的 `document`、`location` 只有 getter。
- 新的測試：guest `dom.c` 五個 case（`input`、`class-list`、`style`、`lists`、`object-as-node`）、`page.c` 四個（`window`、`focus`、`blur`、`click`）；`tests/it/guest.rs` 兩個、`tests/it/engine.rs` 兩個、`shell` 的 `wasm_documents` 一個（focus 和 blur 的畫面）。`cargo test -p dom_wasm`：14 個單元測試、37 個整合測試通過；`shell` 的 `wasm_documents` 連跑 25 次沒有失敗；情境 `todomvc-wasm`、`wasm-document` 通過。
- mutation test（`mutants-2b.toml`，12 條）：12 條都抓到。2a 留下的「不是節點的 handle 當節點的參數」在這裡有測試了。跑完確認檔案都還原了。
- 沒有測試的：`document-set-body`、`element-set-class-list`、`css-style-declaration-set-css-text` 以外的 setter 都測了；`document-set-body` 沒有 case（記在這裡，第 4 段寫 TodoMVC 時用不到）。讀的那一側開操作時帶 layout（`Front::read`）沒有成員走到（清單上沒有要看排版的 getter）。
- 第 2 段的檢查：`cargo fmt` 排過（9 個檔）；之後六份 mutation 的 spec 裡有四條的原文因為程式改動對不上（`lend` 改寫、`give` 的參數、排版），改了原文、重跑那四條都抓到。
  - `cargo clippy --workspace --all-targets -- -D warnings`：抓到一處多餘的 lifetime（`front.rs` 的 `FocusOptions` 那個 impl），修了之後通過。
  - `cargo test --workspace`：通過（55 個測試 binary，3796 個測試通過、1 個 ignored；在 clippy 那一處修正之前跑的，修正只拿掉一個 lifetime 的名字，之後 `cargo test -p dom_wasm` 重跑通過）。
  - `cargo fmt --check` 乾淨；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過；`test.py scenarios todomvc-wasm wasm-document` 通過。
  - 沒有跑的：`test.py automation`、整套的情境、reftest 和 WPT（由 CI 跑）。

### 3a（2026-11-10，macOS，Apple Silicon；fork 還沒 push）

- 決定 14 之前的實驗（`details.md` 的「重入的原型」）：暫時在 fork 的工作目錄關掉檢查、跑原型，試完還原。原型多一個案例 `mixed`（Store 同時有 `async func` 的 import，像頁面的 Store 有 WASI 的時鐘）：沒改的 fork 先被另一個檢查擋住（`store configuration requires that *_async functions are used`），兩個都放行之後巢狀三層可以。
- fork 的修改（`../third_party/wasmtime` 的工作目錄，5 個檔）：
  - `crates/wasmtime/src/runtime/store.rs` 的 `validate_sync_call`：Store 在一個 `StoreSet` 裡、現在在 set 的 thread 上、而且在 fiber 上時，同步的 `call` 放行（兩個檢查都跳過）。
  - `crates/wasmtime/src/runtime/fiber.rs` 的 `resume_fiber`：set 的 Store 的 fiber 在別的 thread 上恢復時 abort，不管它有沒有 GC heap（原本只檢查有 GC heap 的）。這是放行的安全條件：fiber 上可能有 host 函式的 frame。fork 原有的測試 `a_store_is_used_on_its_heaps_thread_only` 顯示 Store 真的能被送到別的 thread（scoped thread），所以不能只靠型別。
  - `crates/wasmtime/src/runtime/vm/gc/enabled/elise.rs`：`SetShared` 記建立它的 thread，`check_thread`。
  - `tests/all/elise/shared.rs`：兩個測試（set 的 Store 用 `call_async` 進去、host 函式同步呼叫回 guest、巢狀三層；不在 set 裡的 Store 照舊被拒絕）。
  - `README.machopo.md`：「A sync call on a set's store's fiber」一節（為什麼、上游擋的兩件事各自怎麼處理）和「Local modifications」的一條。
- 跑過的 fork 的測試（fork 沒有 toolchain 檔，這台預設的 stable 是 1.90、編不了，用 `cargo +1.99.0`）：`cargo test --features gc-elise --test all elise::` 39 個通過（含新的兩個）；`--test all -- async_functions::` 24 個通過；`python3 machopo/elise_tests.py` 的 `elise` 和 `elise-stress` 各 228 個通過、0 個失敗，spec test 沒有跑（這台沒有 `tests/spec_testsuite` 的 submodule）。沒有跑：fork 的 clippy、miri、fuzz。
- machopo 這邊：`cargo test -p dom_wasm` 用改過的 fork 通過（還沒有用到新的行為）；`ci.py third-party` 不介意 fork 的工作目錄有未 commit 的修改。
- fork 在本地的 `machopo` 分支 commit 了（`ef4e892`，上游的 `machopo` 在 `5c38af0`，push 是 fast-forward）；`third_party.toml` 釘到它。還要做的：先 push fork、再 push machopo。

### 3b（2026-11-10，macOS，Apple Silicon；fork 還沒 push）

- WIT（`machopo:dom@0.2.0`，版本沒有改：還沒有外面的 guest）：`ADDED` 多 11 個名稱（`event-target-add-event-listener`、`-remove-event-listener`，事件的 9 個成員），`can_hold` 收 callback（`u32`）和 listener 的 options（`bool`）。world `script` 多 `export handle-event: func(callback: u32, event: object)`。和決定 15 寫的有兩處出入：
  - 參數的名字是 `options`（IDL 的名字，產生器照抄），不是 `capture`；值還是 `capture` 的布林。
  - `handle-event` 的 `event` 的型別寫 `object`，不是 `event`：world 自己 `use` 的 `event` 在 wit-bindgen 的 C 裡叫 `script_event_t`，和它 async runtime 的同名型別撞名，編不過。
- `src/front.rs`：callback 和 options 的參數；`same_callback`、`store_callback` 讀寫 callback edge 的 embedder word（`listener_word`：Store 的編號加一在高 32 bit、guest 的編號在低 32 bit；0 是 handler 屬性的）；`add_scripted`（`focus`、`blur`、`click` 的 host 函式：把「同步呼叫這個 Store 的 `handle-event`」交給 front）和 `WasmFront::call_listener`（guest 自己的分派）。
- `src/engine.rs`：`Host` 拿著 engine 的 `Option<Dom>`、`StoreSet`、script 的清單和 console，`Host::call_listener`（頁面的分派）把 DOM 借給那個 script 的 Store、poll `handle-event`；`start_script` 記 Store 的編號、存 `handle-event`（world 的 export，guest 沒有它時 instantiate 之後報錯）；microtask checkpoint 結束時 flush guest 印的東西。
- guest：`page.c` 多 `handle-event` 和十個 case（`listen`、`listen-click`、`prevent-click`、`stop-click`、`remove-click`、`listener-trap`、`trap-click`、`keep`、`keep-click`、`nested`）；`todomvc.c` 的 `handle-event` 是空的。三個 `.wasm` 重編。
- 測試：`tests/it/engine.rs` 多 12 個；情境 `wasm-listeners`（真的滑鼠點擊走 capture、target、bubble 三個 listener、帶 `button` 和 `detail`，按鍵的 `keydown` 帶 `key`）。`cargo test -p dom_wasm`：14 個單元測試、49 個整合測試通過；`cargo test -p shell --test it wasm`：7 個通過；`test.py scenarios wasm-listeners wasm-document todomvc-wasm` 通過。
- 寫測試時發現、照現在的行為寫的：`click()` 的事件的 `detail` 是 1（Chrome 是 0；是 DOM 那一層的行為，JS 文件也一樣，不是這一步改的）；留著的事件 handle 之後用是 trap（「不是這個 Store 的 handle」），不是 `TypeError`。
- mutation test（`mutants-3b.toml`，15 條）：14 條抓到。第一次跑有一條沒抓到（trap 之後繼續呼叫 listener：下一個呼叫本來就進不去 trap 過的 instance，畫面一樣），補了斷言（回報的是原本的 trap、不是後面那個錯誤）之後抓到。有一條一開始打錯地方（打到 `is_plain`），改成 `can_hold` 之後抓到。沒抓到的一條：
  - 「trap 的 listener 的 Store 留在 set 裡」：script 已經從 engine 的清單拿掉、不會再進去，Store 只是留到換頁才丟，從外面看不出來（和 `turn` 的那一條一樣，記在 `docs/test-gaps.md`）。
  - 跑完確認檔案都還原了（跑之前和之後的 `git diff` 一樣）。
- 沒有測試的（記在 `docs/test-gaps.md`）：listener 等待（走不到）；guest 的分派裡再分派、兩個 Store 互相分派、很深的巢狀；script 元素的 `load`、`error`。`focus`、`blur` 分派的事件走 guest 的 listener 沒有 case（和 `click` 同一條路，`add_scripted`）。
- 第 3 段的檢查（`cargo fmt -p dom_wasm` 排過之後）：`cargo fmt --check` 乾淨；`cargo clippy --workspace --all-targets -- -D warnings` 通過；`cargo test --workspace` 通過（55 個測試 binary，3808 個測試通過、1 個 ignored）；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。
  - 沒有跑的：`test.py automation`、整套的情境、reftest 和 WPT（由 CI 跑）；fork 的測試在 3a 跑過，3b 沒有改 fork。
- fork 的 commit（`ef4e892`）還沒 push：先 push fork、再 push machopo。

### 4 和第 4 段的檢查（2026-11-10，macOS，Apple Silicon；fork 還沒 push）

- 開始前查過：JS 版（`tests/sites/todomvc`）用到的 DOM API 現在的 WIT 都有，沒有加成員。`event.keyCode` 改讀 `key`，`dataset` 改用 `getAttribute`/`setAttribute`。
- guest（`crates/dom_wasm/tests/guests/todomvc.c`，約 570 行）：JS 版的 model、view、controller 一項一項搬（每個函式的註解寫它對應 JS 的哪一個），listener 照 JS 版加的順序、同樣的 capture（`blur` 是 capture）。JS 版會丟例外的地方直接回來。
- 找到的問題，改了 guest 的寫法：**一個物件在一個 Store 裡只有一個 handle**，「用完就 `release`」的寫法會把呼叫它的那一層還拿著的 handle 放掉（勾選時：listener 的 target 是那個 checkbox，裡面的 query 又找到它、放掉，外層再放一次就 trap：「不是這個 Store 的 handle」，第一次跑靜態的情境時抓到）。改成記下來、最外層的 export 回來時一起放（巢狀的 listener 不放）。這是 binding 這種設計對 guest 的要求，記在 `crates/dom_wasm/README.md`。
- 測試頁面的 seed：原本靜態的 guest 一開始就有三個 todo，互動的 guest 和 JS 版一樣從空的開始；`todomvc-wasm.html` 的清單加 `data-seed`，guest 看到它就先放三個 todo、在 `start` 裡畫（headless 的畫面在 `load` 的 listener 之前取，照 JS 版在 `load` 才畫的話和寫死的 markup 比不過）。
- 情境（決定 17）：`todomvc-app-js` 和 `todomvc-app-wasm`，步驟逐行相同（`shell` 的 `wasm_documents::the_todomvc_scenarios_of_both_apps_are_the_same_steps` 檢查；手動改掉一行確認它會失敗、指出是第幾步）。先對 JS 版寫到通過，原樣給 wasm 版跑，第一次就通過。靜態的情境 `todomvc-wasm` 的後半改成「勾一個、計數變 1」。
- 視窗裡看得到的行為都寫成情境，沒有請人看的項目。
- mutation test（`mutants-4.toml`，1 條，測試是情境 `todomvc-app-wasm`）：「guest 的 `blur()` 不呼叫 listener」抓到（Enter 存檔靠它）；跑完確認檔案還原。這一段沒有改 Rust 的實作，其他的行為由 3b 的 15 條守著。
- 過程中的失誤：我誤下 `git checkout` 還原了還沒 commit 的 `todomvc.c`，照對話裡的內容重寫回來（只影響那一個檔案，之後的測試都是對重寫後的版本跑的）。
- 第 4 段的檢查：`cargo fmt --check` 乾淨；`cargo clippy --workspace --all-targets -- -D warnings` 通過；`cargo test --workspace` 通過（55 個測試 binary，3809 個測試通過、1 個 ignored）；`sanity.py` 七項通過；`test.py scenarios todomvc wasm`：8 次都通過；`test.py automation todomvc-app wasm-listeners wasm-document todomvc-wasm`（真的視窗）：5 份都通過。
  - 沒有跑的：整套的情境、reftest、WPT（由 CI 跑）；fork 的測試（這一段沒有改 fork）。
- 還要做的：RFC 完成時的整理（狀態、`progress.md` 併成一份、RFC 改寫成完成時的樣子）、merge 回 `main`、`cargo clean`，等審核。fork 的 commit（`ef4e892`）還沒 push：先 push fork、再 push machopo。

