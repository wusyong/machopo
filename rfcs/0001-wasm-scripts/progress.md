# RFC 0001 的進度：wasm 腳本（wasmtime + cppgc）

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-04（`wasmtime` 分支，從 main 重新實作；V8 版本留在 `wasm` 分支，它的進度在那邊的 `w1-progress.md`、`w2-progress.md`）

**完成：2026-10-05**，macOS（Apple M4），Rust 1.99.0，wasmtime 49.0.2，V8 是 rusty_v8 fork 的 release `kun-v152.2.0-4`，guest 用 wasi-sdk 34.0、wit-bindgen 0.62.0、Lua 5.4.9 編。做完的是第一層（holder 和 Store 表）；第二層（cppgc 當 wasmtime 的 GC）2026-10-05 移到 [RFC 0005](../0005-cppgc-wasm-gc/README.md)（README 的「已決定的事項」第 54 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 釐清（`wasmtime-questions.md` 的原型） | 完成；Q6 後來不需要了（第 26 條） | 2026-10-04 |
| 2. 搬移 | 完成 | 2026-10-05 |
| 3. 第一個 guest（3a 單獨的 heap、3b `dom` 的外部 engine 模組、3c `dom_wasm`、3d 純 wasm 的重新進入、3e renderer 照文件選 DOM） | 完成 | 2026-10-05 |
| 4. 其他 guest（4a Rust 和 `start` 回傳之後的等待、4b reftest W1–W6、4c Lua） | 完成 | 2026-10-05 |
| 5. ~~兩個 engine 交錯~~ | 拿掉（第 26 條） | |
| 6. 和 V8 版本比較（6a 量測、6b 決定） | 完成：啟動和每次呼叫都先不改（第 49、50 條） | 2026-10-05 |
| 7. 不跑 V8 的設定 | 完成 | 2026-10-05 |
| 第二層：cppgc 當 wasmtime 的 GC | 移到 RFC 0005（第 54 條） | |

## 驗收

**`kun:dom` 的 WIT 和 guest 的 binding**

| 項目 | 怎麼驗證 |
| --- | --- |
| WIT 和 golden file（`crates/dom_api/tests/machopo-dom.wit`）一樣；`wit-parser` 讀得了，讀出的 `dom` 的函式（名稱、是不是 `async`）和 `dom_api::wit` 的一樣；guest export 的是 async 的 `start` 和 sync 的 `callback`；`dispatchEvent` 在每個 resource 上都是 `async func`，沒有別的；`type callback = u32`，callback 的參數都用它；例外的代碼，`dom-exception` 的註解是「編號 名稱」的列表 | `cargo test -p dom_api --test wit` |
| Lua 的 runtime（`resources/kun-lua.wasm`）是照現在的 WIT 編的：讀它的 custom section `kun_dom_wit`（2026-10-06 起，原本讀放進 repo 的 `kun_dom.c`，第 56 條） | 同上（`the_lua_runtime_is_built_from_this_wit`） |

**載入、執行和文件的種類**

| 項目 | 怎麼驗證 |
| --- | --- |
| 哪些 `<script>` 算 wasm script（`type` 不分大小寫、前後的空白、要有 `src`、帶參數的不算）；永遠不擋 parser、照 `defer` 的時機，`async` 的編好就執行；engine 不跑 wasm 時不執行；engine 不跑 JS 時，classic script 不下載也不執行 | `cargo test -p renderer --lib script` |
| `<meta name="script" content="wasm">` 的預先掃描：名稱、`content` 不分大小寫，前後的空白不算；註解、其他標籤的屬性裡的不算；1024 byte 之後的不算；只看 HTML 文件 | `cargo test -p renderer --lib document`（`script_languages`） |
| `data:` URL（percent-decoding、base64、沒寫 MIME 時是 `text/plain`）；`.wasm` 是 `application/wasm` | `cargo test -p net --lib` |
| 整個 browser：wasm script 在 parse 完之後和 `defer` 的 script 照文件順序執行；MIME 不是 `application/wasm` 的不執行；`data:` URL 的 wasm script（測試用的 engine） | `cargo test -p shell --test wasm_scripts` |
| JS 文件換到 wasm 文件（wasm script 跑了，文件裡的 JS 沒跑）、wasm 文件換回 JS 文件（JS 照常）；`--script wasm`：沒有宣告的文件也是 wasm 文件，renderer 從啟動到結束都沒有要過 JS 的 engine（沒有建 isolate）；`kun --headless --script wasm` 把選項交給 renderer | `cargo test -p shell --test script_languages` |
| `--script wasm` 的參數解析（只接受 `wasm`、錯誤的訊息、預設首頁 `about:wasm-demo`、headless 也帶上、`--help`） | `cargo test -p shell --bin kun` |
| `about:wasm-demo`：三個 script（C、Rust、Lua 的 runtime）以 base64 的 `data:` URL 放進頁面 | `cargo test -p browser --lib loader` |

**wasm 文件的 heap 和外部 engine**

| 項目 | 怎麼驗證 |
| --- | --- |
| heap 的 task 佇列：沒有延遲的照放進的順序，延遲的到期才拿、最早的先；只剩延遲的 task 時也回報到期時間；一串放進只叫醒一次 | `cargo test -p dom --lib heap_tasks` |
| 單獨的 heap 上的 DOM：測試扮演 event loop、不要求任何 GC，heap 自己收、收完（包括延遲的 sweep），修改和 marking 交錯時對照模型；task 執行途中新排的留到下一次；不能開 GC stress、沒有 isolate | `cargo test -p dom --test standalone_heap`（也在 `MACHOPO_CPPGC_MARKING=concurrent`、`test.py matrix` 和 `fuzz` 底下） |
| 整個 browser：頁面在單獨的 heap 上載入、畫出來，建立文件時做的垃圾在 renderer 的 loop 裡被 finalize；合併 timer、heap 和 engine 的等待時間 | `cargo test -p shell --test standalone_heap`、`cargo test -p renderer --lib render_thread`（`the_sooner_wait`） |
| 「節點 → listener → holder → slot → 節點」的循環被回收，Store 在 event loop 的最外層才 drop；撐住的 Store 不被回收（計數的）；listener、timer 撐住 Store；callback 被清掉的 listener 放開 Store | `cargo test -p dom --test foreign`（也在 `matrix` 和 `fuzz` 底下） |
| slot 表：跨 chunk、guest 每拿一次佔一格、release 之後清空並重用；marking 途中寫的 slot 和新配置的 chunk 撐過收尾 | 同上（`slots`、`slots_written_while_marking_survive`） |
| 外部 engine 的 callback：同一個 Store、同一個 id 才相等；event handler 存得進去、讀得回來；Store 的 take / put | 同上 |
| `DomAccess`：同時開兩個 DOM scope、複製它，都編譯失敗；在別的 thread 上開 scope 會 panic；`ThreadBound` 在別的 thread 上存取、poll 會 panic | `cargo test -p dom --test compile_fail`、`foreign`、`cargo test -p dom --lib thread` |
| 放錯的值都 panic，而且在改動任何東西之前（外部 engine 的入口開在 JS 文件的 DOM、兩種 callback 放錯、別的 DOM 的 callback 和節點、release 空的 slot） | `foreign` 的 `should_panic` 測試 |

**wasmtime 上的 guest**（`cargo test -p dom_wasm --test engine`，guest 在 `crates/dom_wasm/tests/guests/`）

| 項目 | 測試 |
| --- | --- |
| C 的 guest：`start` 改 DOM；DOM 的錯誤回傳例外的代碼；upcast、downcast；`performance.now()`；建構子和字典；event handler 讀回來是 guest 自己的；listener 改 DOM、event handler 回傳 false 取消事件；GC 之後 listener 和 guest 拿著的節點還在；drop 的 handle 放開節點 | `a_guest_builds_dom_and_handles_clicks` |
| 在 `start` 裡等待（WASI 的單調時鐘，降到 100 µs）：時間到了叫醒 Store；等待中不被回收，做完之後被回收 | `a_guest_waits_in_start` |
| `start` 回傳之後還在等待（Rust 的 async task）：只有等待撐住 Store；guest 回傳 `YIELD`（連續三次）、`WAIT` 之後照樣被叫醒；等待到期時 DOM 正好呼叫它也一樣 | `a_guest_waits_after_start_returns`、`a_guest_resumes_while_the_dom_calls_it` |
| 沒有人參照的 Store（它的 div 和 div 的 listener 只互相參照）被回收、drop | `a_store_in_a_cycle_is_collected` |
| trap 照 JS 沒被接住的例外回報，trap 之前的 DOM 改動留著 | `a_trap_is_reported` |
| guest 的 `dispatchEvent` 分派給自己的 listener：在 `dispatchEvent` 回來之前執行，巢狀的也可以，`preventDefault` 是分派的結果；分派給別的 Store 的同步執行；繞回正在更外層執行的 Store（A → B → A）的 listener 被回報、跳過；自己的 listener 在它的分派中 trap 時回報，拆掉 DOM 時不出事 | `a_guest_dispatches_to_its_own_listeners`、`…_to_another_stores_listeners`、`a_dispatch_back_into_a_running_store_skips_its_listener`、`a_listener_trapping_during_its_guests_dispatch` |
| W7 的九項檢查，最後兩項由測試照 renderer 的方式跑 timer 和 animation frame | `w7` |

**事件和其他 DOM 功能**

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom_api` 的分派是 async 的：host 在每個 listener 之間讓出時照順序一個一個執行，event handler 的 false 取消事件；同步的入口碰到會讓出的 host 會 panic；JS 的分派照舊 | `cargo test -p dom_api --test events`、`cargo test -p dom_js` |
| 給 script 的時間降到 100 µs（`performance.now()`、`Event.timeStamp`、animation frame），timer 照舊用完整的時鐘；JS 的 `performance.now()` 的膠水；script 執行時照樣在走；`Element.localName` | `cargo test -p dom_api --test events`（`times_are_coarsened`）、`cargo test -p dom_js --test bindings`（`performance_now`）、`cargo test -p shell --test script`（`performance_now_moves_while_script_runs`）、`cargo test -p dom_api --test api`（`local_name`）、reftest `script/j5-animation-frames.html` |
| engine 的 task（被叫醒的 Store）跑了 script，renderer 就請求 main frame：guest 等完之後改的頁面畫得出來 | reftest `wasm/wait.html` |

**reftest**（`tests/reftests/wasm/`，經過整個 browser）

| reftest | 測什麼 |
| --- | --- |
| W1 `w1-dom.html` | wasm script 建 DOM（C） |
| W2 `w2-order.html` | wasm script 照文件順序在 parse 之後、`DOMContentLoaded` 之前執行；`start` 裡等待的不擋住後面的 script 和 `DOMContentLoaded`；`async` 的編好就執行 |
| W3 `w3-errors.html` | 不是 wasm、core module、沒有 export `kun:dom/guest`、MIME 不對、import 不合（WASI 的 sockets）、`start` 裡 trap、載入失敗、不認識的 `type`，各自回報、跳過，後面的照常 |
| W4 `w4-events.html` | `load` 照註冊的順序給兩個 Store；一個 Store 分派給另一個，對方的 listener 在 `dispatchEvent` 回來之前執行，`preventDefault` 是結果 |
| W5 `w5-animation-frames.html` | animation frame 的 callback 拿到 frame 的時間 |
| W6 `w6-rust.html` | Rust 的 guest（std、`await` `dispatchEvent`），九項檢查 |
| W7 `w7-c.html` | C 的 guest，九項檢查；同一頁的 JS 不執行 |
| W9 `w9-lua.html` | 頁面上的 Lua script 由編成 wasm 的 Lua 直譯器執行：字串、錯誤（`pcall`，wasm 的 exception handling）、轉型、`nil`、出錯的 script 之後照常、listener（加和移除）、event handler、MouseEvent、coroutine、GC、timer、animation frame |
| `wait.html` | 見上面「事件和其他 DOM 功能」 |

W8（Go）拿掉了：Go 還沒有 WASI 0.3（第 12 條）。

## 做過的檢查

**最後的驗證**（2026-10-05，macOS，Apple M4）：`cargo fmt --check`；`cargo clippy --workspace --all-targets -- -D warnings`（沒有警告）；`cargo test --workspace` 652 個通過，reftest 193 個通過（`harness/chain-broken*.html` 2 個是預期中的失敗）；`sanity.py deps`、`sanity.py unsafe`、`sanity.py docs`；`test.py matrix` 6 組設定都通過（每組 171 個）；`test.py fuzz` 20 組都通過（這次沒記下序列的 seed，每組的 `MACHOPO_TEST_SEED` 在當時的輸出裡；實作期間 3b 跑過一次，seed `0x09d77df7`）；之後 `ci.py prune`。guest 用 `ci.py guests` 重編是可重現的（C、Rust、Lua 的 runtime 都逐 byte 相同）。

**mutation test**（每一步做完時跑，每個測試指令先確認沒弄壞時會通過；沒抓到的就補測試再跑，補不了的記進 `docs/testing.md`）：

| 步驟 | 個數 | 結果 |
| --- | --- | --- |
| 2 搬移 | 18 | 都抓到；「renderer 不檢查 wasm script 的 MIME」一開始沒抓到（執行 wasm 的端到端測試在 `wasm` 分支是跟著 V8 的 engine 寫的），補了 `shell` 的 `wasm_scripts` 之後抓到；舊文件的 compile 結果被丟掉只靠審查 |
| 3a 單獨的 heap | 12 | 抓到 11 個；「只剩延遲的 task 時不回報到期時間」補了佇列的單元測試之後抓到；「loop 等待時不算 heap 的到期時間」測試逼不出來（cppgc 只在 concurrent sweep 進行中才排延遲的 task），合併等待時間抽成 `sooner` 加單元測試，呼叫的那一行只靠審查 |
| 3b 外部 engine 模組 | 19 | 都抓到；「清掉 callback 時沒清 holder 的 edge」補了 `a_cleared_callback_lets_go_of_its_store` 之後抓到；入口拿回別的 `DomAccess`、unwind 時 abort 只靠審查 |
| 3c `dom_wasm` | 17 | 抓到 16 個；「撐住 Store 時不看等待的計數」在 4a 補了 Rust 的 guest 之後抓到 |
| 3d 重新進入 | 9 | 都抓到；「同一個 Store 的 listener 跑完之後沒放掉事件的 slot」補了「分派給沒接在文件上的 div」之後抓到；`ThreadBound` 在別的 thread 上被 drop 時 abort 只靠審查 |
| 3e 照文件選 DOM | 8 | 都抓到 |
| 4a Rust、`YIELD` | 9 | 抓到 8 個；「第二次以後的 poll 沒設 `polling`」補了連續的 `yield_async` 之後抓到；`call` 只 poll 一次（用真的 waker）是等價的（`docs/test-gaps.md` 的「等價、不用處理的 mutation」） |
| 4b reftest W1–W6 | 3 | 都抓到（弄壞新 reftest 蓋到的實作：MIME、`type` 的比對、animation frame 的時間） |
| 4c Lua | 6 | 都抓到（產生器和 runtime 的每個都重新產生、重編、跑 reftest；「認不出 `callback`」是產生器自己拒絕） |
| 7 不跑 V8 的設定 | 7 | 抓到 6 個；視窗不把選項交給 renderer 只靠視窗的確認（自動測試不經過視窗） |
| 第 56 條（2026-10-06，Windows 11） | 4 | 都抓到（每個都重新產生、重編 Lua 的 runtime，再跑 `the_lua_runtime_is_built_from_this_wit`）：hash 算錯、section 換了名字、寫成 data segment、照舊的 WIT 編。重編是可重現的（連編兩次逐 byte 相同），只多了 custom section；W9 照樣通過 |

**找到的 bug 和踩到的坑**（設計上的理由在 README 對應的條目）：

- **guest 回傳 `YIELD` 之後叫不醒**（4a 的原型）：wasmtime 為了 `YIELD` 從佇列取出工作、先回傳 `Pending`，只 poll 一次就丟掉 `run_concurrent` 的 future 時，那件工作不見了。改成 `poll_store`（第 41 條）。C 的 guest 只用同步的等待，碰不到。給 wasmtime 的回報草稿是 `wasmtime-yield-report.md`，還沒送出。
- **engine 的 task 改了 DOM 卻沒請求 main frame**（4a 的視窗確認）：從 3c 起就有，`about:wasm-demo` 的時鐘要點一下才畫出來；`ScriptEngine::run_tasks` 改成回傳有沒有跑 script（第 33 條），加上 reftest `wasm/wait.html`。
- **wasmtime 的 borrow**（3c）：傳給 guest 的 borrow 不能先轉成 `ResourceAny`（只存在於一次呼叫裡，wasmtime 會 panic），改用 `ResourceDynamic` 當 export 的參數型別；C binding 收到的 borrow 要在回傳前 drop，否則那次呼叫失敗。
- **拆 DOM 時 abort**（3d）：guest 的 listener 在它自己的分派中 trap，Store 裡留著拿著 `NodeHandle` 的 future；改成先 drop Store 表、再檢查 root。
- **A → B → A 被靜靜地跳過**（3d）：改成回報。
- **WASI 0.3 的 TLS 是每個 task 一份**（4a）：`start` 設的 `thread_local!` 在 `callback` 裡是空的，跨呼叫的狀態要放在 `static`（`sdk/rust/README.md`）。
- **Rust 的 cdylib 預設 PIC**（4a）：linker 不產生 cooperative threading 的 `__wasm_{get,set}_stack_pointer`，要 `-C relocation-model=static`（第 40 條）。
- **宣告表裡父 interface 的連結是描述的另一份複本**（3c）：用指標比對找不到，resource 的編號改用名稱找。
- **檢查等於沒效的測試**（3e）：「這行 JS 不能執行」一開始用 `element.style`，kun 還沒有它，JS 就算執行了也只會丟例外；改成 `setAttribute`。
- **reftest J5 的容許誤差**（2）：時間降到 100 µs 之後放寬到 0.1 ms，並檢查每個時間都是 100 µs 的倍數。
- **單獨的 heap 的 cppgc 行為**（3a）：配置時不會當場做 atomic GC、改排 task；GC stress 需要保守掃描 stack，在這種 heap 上不能開；延遲的 task 只來自 concurrent sweep。
- **`DomAccess` 必須是 `Send`**（3b）：wasmtime 的 `run_concurrent` 要求 Store 的 data 是 `Send`，所以用的時候檢查 thread（第 28 條）。
- **單獨的 heap 拆掉時不執行任何 finalizer**（2026-10-06 審查 GC 封裝時發現）：`cppgc::Heap` 的解構子只收尾進行中的 GC、不 finalize 活著的物件，所以 wasm 文件的 DOM 拆掉時每個節點的 Rust `Drop` 都沒跑（洩漏，不是 UB）。fork 的 `cppgc__StandaloneHeap__DELETE` 改成先做一次不掃 stack 的 GC；測試是 `standalone_heap` 的 `dropping_the_dom_finalizes_every_node`：對修改前的 artifact 失敗（finalize 0 個，83 個活著），也就是「只 `delete`」這個 mutation；對重編的 V8（macOS，Apple M4，debug 1289 秒、release 1047 秒）通過，`dom`、`bindings`、`dom_js`、`dom_wasm` 連結 release 的全過，`test.py matrix` 8 組都過（每組 352 個）。修正在 fork 的 release `kun-v152.2.0-5`（2026-10-06），`RUSTY_V8_MIRROR_TAG` 已換過去，下載的 prebuilt 也通過這個測試（`crates/dom/soundness.md` 的「找到並修正過的問題」）。

**量測**（第 6 步，2026-10-05，release build；量測的程式是暫時的，量完拿掉，第 48 條）：

- **量法**：暫時的 `dom_wasm` 測試扮演 renderer。啟動：compile（engine 的 compile thread，Cranelift）和 `run_wasm_script`（instantiate 加上 `start` 第一次停下來），各 5 次取中位數。記憶體：`ps` 的 RSS。每次呼叫：暫時的 C guest 在 `start` 裡呼叫 20 萬次（`dispatchEvent` 2 萬次），用 WASI 的時鐘量；JS 的對照是同一個 release build 的 `kun --headless` 跑同樣的呼叫。profile：暫時的 C guest 呼叫同一個函式 2,000 萬次，macOS 的 `sample` 取樣 4 秒。
- **啟動**：

  | guest | 大小 | compile | instantiate + `start` |
  | --- | --- | --- | --- |
  | C，`crates/dom_wasm/tests/guests/dom.wasm` | 72 KB | 10.3–10.9 ms | 0.2 ms |
  | C，W7 | 63 KB | 7.4 ms | 0.1 ms |
  | Rust，W6 | 173 KB | 13.4 ms | 0.2 ms |
  | Lua 的 runtime（沒有 Lua script） | 493 KB | 44.5 ms | 0.3 ms |

  V8 版本（Q11 的原型，Lua 的 core module）：Liftoff lazy 0.4 ms、全部先編 2.8 ms；同一個原型裡 wasmtime 的 Winch 10 ms、讀預先編好的程式碼 0.3 ms。啟動的成本幾乎都在 compile（第 49 條：先不處理）。
- **記憶體**：每多一個頁面，C 的小 guest 約 0.5–1.3 MB、Lua 約 3.5–4.4 MB。一個 process 的第一個 wasm 頁面多 26–59 MB（wasmtime 的 engine、Cranelift 編譯時用掉、留在 allocator 裡的記憶體；量的順序不同差很多），是一次性的。Q11 量的 V8 的 isolate 加上 context 約 6 MiB。RSS 的雜訊大，只看量級。
- **每次呼叫**：

  | 呼叫 | wasm（這個 RFC） | 同一個 build 的 JS | V8 版本（`kun:dom/v1` / handle 形式，`wasm` 分支 2026-10-03） | Q11 的原型（只有 component 的邊界） |
  | --- | --- | --- | --- | --- |
  | 沒有參數、回傳數字（`performanceNow`） | 299 ns | 130 ns | | 60 ns |
  | `setAttribute`（兩個字串） | 666 ns | 305 ns | 920 / 1580 ns | 155 ns |
  | `parentNode`（回傳 handle，wasm 再 drop） | 488 ns | 88 ns | 770 / 2550 ns | 145 ns |
  | `dispatchEvent`（給自己的 listener） | 2840 ns | 770 ns | | 1760 ns（Q2） |

  比 V8 版本的兩種形式都快，是 JS 的 2–5 倍；一個 frame 一千次呼叫約 0.3–0.7 ms。
- **每次呼叫的 profile**（時間佔的比例）：

  | | `performanceNow` | `setAttribute` | `parentNode` |
  | --- | --- | --- | --- |
  | wasmtime 的動態 host 函式（`Val` 的轉換、每次呼叫建立和刪除 host task、`Vec`） | 64% | 27% | 53% |
  | 真正的 DOM 操作 | 1% | 41% | 6% |
  | handle 換成 DOM 物件（wasmtime 的 resource 表、slot 表、建立 root） | 17% | 7% | 12% |
  | 替結果建 handle（佔 slot、建 wasmtime 的 resource） | | | 16% |
  | 參數轉成 `Value`（字串複製、`Vec`） | 少 | 19% | 1% |
  | drop `Object`（釋放 root）和其他配置 | 約 8% | 約 3% | 約 5% |

  沒有單一、便宜的熱點，程式不改（第 50 條）。

**視窗**（每一步做完時手動確認，macOS，2026-10-05）：

- `about:demo` 的「performance.now()」按鈕（數字是 0.1 的倍數）和原本的東西（藍色方塊、「Click me」、綠燈、時鐘）。
- `about:wasm-demo`（從 `about:demo` 的連結進去）：renderer 換成 wasmtime；C 的 script 的 click 計數、guest 自己分派的 `counted`、`start` 裡每秒更新的時鐘；Rust 的 script 在 `start` 回傳之後每秒更新的時鐘、它自己的 listener；Lua 的 script 的 listener；三個的計數一樣，兩個時鐘不碰頁面也會自己更新；「JS ran」不出現。換回 `about:demo` 時換回 V8，再進來時三個 script 都重新開始。
- `kun --script wasm`：log 的 engine 是 wasmtime，直接開 `about:wasm-demo`；到 `about:demo` 時不換 engine，頁面只有靜態的內容；不加選項時照舊。
