# RFC 0034 的進度：`MessageChannel` 和 `structuredClone()`

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-11。完成：2026-10-12，macOS Apple Silicon（Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0034` 上分兩段做，第 1 段做完時先 merge 回 `main`。動到 GC 而要跑的 `test.py matrix`、`fuzz` 在 merge 之後看 CI。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. port 和 `structuredClone()`，沒有 transfer（第 7 到 11 條） | 完成 | 2026-10-11 |
| 2. transfer（第 12 到 15 條） | 完成 | 2026-10-12 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| channel 的兩個 port 互相 post，每個訊息一個 task、照順序、`data` 是複本，沒有 origin、source、ports；queue 開之前留著（`start()`、設定 `onmessage` 才開，`addEventListener` 不會）；`close()` 之後兩邊都不送，關掉的 port 等著的也丟掉；start 過的 port 沒人拿著也照樣收到；`structuredClone()` 的深拷貝、循環、錯誤，複製到 `this` 的 realm；interface 的形狀。transfer：port 在接收端是新的、和原本的配對互通，原本的 detach（post、close 都沒作用）；等著的、在路上的、已經排成 task 的訊息照送出的順序跟著走，新的 port start 了才送；等著的訊息帶著的 port 經過 GC 還在；`ArrayBuffer` 搬過去（原本的和 view detach，經過 port、window 的兩種 `postMessage` 和 `structuredClone`）；transfer list 的錯誤（型別、重複、detached、closed、自己這個 port、不是物件、沒 transfer 的 port）和序列化先於 detached 的檢查；目標 port 在 transfer list 裡的訊息丟掉；transfer 給 iframe 的 port 是那個 realm 的；`MessageEvent` 的 `ports`（建構子、`initMessageEvent` 換掉原本的） | `cargo test -p dom_js --test it messaging::` |
| `dom`：channel 拿著兩個互相配對的 port；queue 的訊息第一次 `start()` 照順序交回（連同各自帶著的 port）；`close()` 兩邊都解除配對；start 過、還配對著的 port 在 heap 外 root 著，關掉、transfer、document 丟棄時拿掉；queue 裡的 port 由等的 port 撐住；detach 之後取回的訊息排在等著的前面，接手的 port 和原本的配對互相配對、照順序接手 queue；GC 型別的共通檢查和工作負載 | `cargo test -p dom --test it message_ports:: script_objects:: script::every_type_unwraps_only_as_itself gc_stress::` |
| timer 和 DOM 的 task 共用排入的先後，interval 跑過之後重新排入 | `cargo test -p dom --test it timers::timers_and_tasks_keep_the_order_queued` |
| event loop 照排入的先後一次跑一個 task：`setTimeout(0)` 和 port 的訊息的五種情況和 Chrome 152 一樣 | `cargo test -p shell --test it messaging::messages_and_timers_run_in_the_order_queued` |
| `kun:dom` 不變：`MessageEventInit.ports`、`initMessageEvent` 的 `ports` 只給 JS（`JsOnly<T>`）；`spec-missing.txt` 少了 `MessageEvent.ports`、`MessageEventInit.ports`、`structuredClone()` | `cargo test -p dom_api --test it wit:: spec::` |
| WPT：`webmessaging/message-channels/` 的七份、`multi-globals/` 的兩份、`Channel_postMessage_*` 的八份、`MessagePort_*` 的兩份、`postMessage_dup_transfer_objects.htm`、`structured-clone/` 的三份、`safe-passing-of-structured-data/` 的三份；`with-ports/`、`without-ports/`、`with-options/` 原本預期失敗的十份整份通過，第 2 段再六份（`023`、`025` 到 `027`、`message-channel-transferable`、`null-transfer`）；電池測試的 transfer 那幾項通過 | `python3 scripts/test.py wpt webmessaging/ html/webappapis/structured-clone/ html/infrastructure/safe-passing-of-structured-data/`（119 個，0 個不符） |

## 做過的檢查

兩段做完時都跑過 `cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（第 1 段 2140 個、第 2 段 2152 個，全部通過，第 2 段 83 秒）、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`，和上面的 WPT（0 個不符）；第 1 段另外跑 `speedometer_test.py`（37 個）。

- **做了什麼**：
  - 第 1 段：`dom` 的 `message_port` 模組（`MessageChannel` 是新的 GC 型別，`ObjectTag` 四十一種；port 是 `EventTargetKind::MessagePort`，會變的狀態在 `EventTargetImpl::port_state`；pending activity 的 root 在 `DomTasks::active_ports`）；timer 和 DOM 的 task 共用排入的序號（`Timers::sequence`、`next_key`），renderer 的 `run_due_tasks` 一次挑一個（第 7 條）；`dom_api` 的 `message_port` 模組、`Task::PortMessage`、`interfaces/core_messaging.rs`（`[JsOnly]`）、`MessageEvent.ports`（宣告表多了唯讀的 `[JsOnly] attribute`）、`window.structuredClone()`（`ScriptHost::deserialize_in`）。
  - 第 2 段：宣告表的 WebIDL `object`（`ObjectValue`，第 15 條）和只給 JS 的參數、字典成員（`JsOnly<T>`：`MessageEventInit.ports` 照型別會以 `list<borrow<event-target>>` 進 `kun:dom`，改用它保持 WIT 不變）；`dom_js` 的 `serialize_with_transfer`（port 是 host object，`ArrayBuffer` 用 `transfer_array_buffer`、序列化之後 detach，backing store 是 `dom` 的 `TransferredBuffer`，跟著 `SerializedScriptValue` 走，第 14 條）；`dom` 的 port 多了 `queued_ports`（第 12 條）、detach 和 `receive_transferred_port`、`DomTasks::take_where`；`MessageEvent` 的 `ports` 是 `EdgeChunks`；port 的 wrapper 照 port 的 realm 建；`dom_api` 的 port、window 的 `postMessage` 和 `structuredClone` 接上 transfer。
- **第 13 條改成 B3**：照 B1 做完之後寫順序的測試時發現，start 過的 port 已經排成 task 的訊息在 transfer 之後晚於後來送的訊息（`b / a1 / a2`），轉交到還沒 start 的新 port 時也不等它的 queue 開；改成 detach 時把那個 port 的 message task 從 DOM 的 task 取回、放在它的 queue 最前面，拿掉轉交和 `transferred_to`（2026-10-12 確認）。
- **照規範的順序檢查 transfer list**：WPT 的 `transfer-errors.window.js` 要求序列化的錯誤先於 detached 的檢查、序列化時被 transfer 走的也要抓到，`close.any.js` 要求 `close()` 過的 port 不能 transfer（HTML 的 `close()` 設 [[Detached]]）：型別、重複、自己這個 port 先檢查，detached 和 closed 在序列化之後。
- **實作中量的 Chrome 152**：`structuredClone` 用別的 window 的函式 `call` 時複製到 `this` 的 realm（照規範），kun 一樣。timer 的鏈從 timer 的 microtask 開始時，Chrome 第五個 timer 就被夾到 4 ms，kun 照 HTML（和 WPT 的 `timer-nesting-not-inherited-in-microtask.html`）不算那一層；從頁面最上層開始時兩邊一樣。這是 timer 原本就有的差別，記進 `docs/limitations.md`，順序的測試從最上層開始。
- **`dom` 的 GC 測試用 standalone heap**：第 2 段的 `take_queue` 讓 `a_started_port_lives_while_entangled` 失敗（start 過、沒配對的 port 沒被收）：isolate 的 GC 保守地掃 stack，debug build 留在 stack 上的指標撐住了它。檢查回收的四個測試改用不掃 stack 的 standalone heap（`collections.rs` 的做法）。
- **WPT**：第 1 段試跑 18 份候選、收 15 份，第 2 段再收 13 份（`message-channels/cross-document.html`、`dictionary-transferrable.any.js`，`multi-globals/messageport-*.html`，`Channel_postMessage_clone_port`、`ports_readonly_array`、`with_transfer_*` 的三份，`postMessage_dup_transfer_objects.htm`，`structured-clone-detached-window-crash.html`，`transfer-errors.window.js`；清單的註解寫明）。電池測試沒過的是 kun 還不能複製的平台物件（`Blob`、`File`、`ImageData`……）和沒有的 `ReadableStream`；`transfer-errors` 沒過的是 kun 沒有的 `ImageBitmap`、`OffscreenCanvas`。不收的：`postMessage_MessagePorts_sorigin.htm`（在 iframe 載入前讀了 `contentWindow`，沒有 `WindowProxy` 就送到舊的 window）、`.sub` 的幾份（跨源）、`close-event/`（port 的 `close` 事件）、worker、`SharedArrayBuffer` 的。`spec-missing.txt` 少了 `MessageEvent.ports`、`MessageEventInit.ports`、`structuredClone()`，多了 `MessagePort.onclose`（Chrome 有，這份不做）。
- **不穩定的 WPT**：
  - `with-ports/018.html`、`without-ports/018.html`（`<iframe src="javascript:''">`）：kun 把 `javascript:` 的 `src` 當一般的網址抓、失敗時 commit 一份空的新文件，送到第一個 window 的訊息若晚於那次 commit 就跟著舊文件丟掉。第 1 段碰巧兩份都過，刪掉了 RFC 0030 時的 TIMEOUT；第 2 段在第 1 段的程式上也連續逾時。預期結果改成 `[OK, TIMEOUT]`，`.ini` 寫明原因。
  - `moving-between-documents/` 的 move-back 幾份：子頁面在 parse 時 `postMessage("fox")`，父頁面要在伺服器 `sleep(1)` 送出第二段之前搬走元素；超過一秒時 script 在原本的文件 prepare、觸發 `load`、`error`。每次 1 到 7 個 subtest 不符，一次一個 process 跑也有（0、1 個）；第 1 段的程式和第 2 段的程式交錯各跑三次是 5、4、2 對 4、3、4，不是這份造成的。之後排查（2026-10-12）：kun 一次 parse 整份回應（沒有 incremental parsing），script 在測試搬走它之前就在原本的文件 prepare 了，接著看它 2 秒的抓取和 2 秒後的搬回誰先；12 份 external 的 move-back 的 `.ini` 改成 `[PASS, FAIL]`，寫明原因。
- **Speedometer**（第 1 段做完，20 個 default suite，3 輪 × 10 個 iteration，接電源，和這份之前的比）：TodoMVC-React-Complex-DOM、TodoMVC-React-Redux 沒變（Speedometer 用 `dispatchEvent` 送的 discrete 事件在 React 裡同步 render，不經過 scheduler）；NewsSite-Next 的 kun 從 185.2 ms 到 207.4 ms（比值 5.34 到 5.96），sync 一樣、每個 test 的 async 多 6 到 8 ms：React 排的工作現在走 port 的訊息，排在 Speedometer 量 async 的 `setTimeout(0)` 前面而算進 async，和 Chrome 算的一樣；其他 suite 都在 2% 以內。數字記在 RFC 0012 的 `progress.md`。第 2 段沒有讓新的 suite 能跑，照 `docs/benchmarks.md` 的「效能的量測」不重量。
- **mutation test**：
  - `mutants-1.toml`（15 條）：第一次漏了 2 條。interval 跑過之後沒有重新拿序號：補 `dom` 的 `timers_and_tasks_keep_the_order_queued`；`structuredClone` 在呼叫者的 realm 反序列化：V8 呼叫 binding 時的 current context 本來就是函式的 realm，`child.structuredClone(x)` 分不出來，補用 `call` 換 `this` 的兩種。重跑都抓到。
  - `mutants-2.toml`（28 條）：第一次漏了 2 條。window 的 `postMessage` 把 port 建在呼叫者的 realm：查下去是 `Object::realm_window` 對 message port 回傳 `None`，wrapper 建在當下讀 `ports` 的 realm，測試剛好在接收端讀所以看不出來；改成 port 照自己的 realm，測試改成父頁面先讀子頁面收到的 `ports`，另加一條 mutant。目標 port 在 transfer list 裡的訊息照送（doomed）：等價，理由記進 `docs/testing.md`。重跑 27 條抓到、1 條等價。

### Chrome 的 timer 和 port 的訊息（2026-10-11，第 1 段開始前，macOS Apple Silicon，Chrome for Testing 152.0.7977.82）

`--headless=new` 讀一頁測試頁（`ci.py speedometer` 的 server，結果 `POST` 回來），三次都一樣：

| 情況 | 順序 |
| --- | --- |
| 先 `setTimeout(0)`、再 `port.postMessage` | timer、message |
| 先 `port.postMessage`、再 `setTimeout(0)` | message、timer |
| rAF 的 callback 裡先 `postMessage`、再 `setTimeout(0)` | message、timer |
| 五個 message 的鏈對五個 timer 的鏈（各在 callback 裡排下一個） | t0 m0 t1 m1 t2 m2 t3 m3 m4 t4（第五層的 timer 被夾到 4 ms） |
| `postMessage` 和下一個 rAF | message、rAF |

照排進來的先後一次跑一個（第 7 條）。五個的鏈這一項是從 timer 的 microtask 開始的（測試頁在 `await` 一個 timer 之後）：Chrome 把那裡設的 timer 算進那個 timer 的巢狀層數，所以第五個被夾到 4 ms；從頁面最上層開始時是 t0 m0 t1 m1 t2 m2 t3 m3 t4 m4（第 1 段補量）。
