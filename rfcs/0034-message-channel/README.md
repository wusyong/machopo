# RFC 0034：`MessageChannel` 和 `structuredClone()`

- 開始日期：2026-10-11
- 狀態：✅ 已完成：2026-10-12（第 1 到 6 條開始前接受，第 7 到 11 條在第 1 段、第 12 到 15 條在第 2 段開始前定下，第 13 條實作時改過（2026-10-12）；兩段都在 macOS Apple Silicon 上做，第 1 段 2026-10-11 先 merge；動到 GC 而要跑的 `test.py matrix`、`fuzz` 在 merge 之後看 CI；驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 J，DOM 線的下一項。window 的 `postMessage` 和 `MessageEvent` 已經在 RFC 0030 做了（它的第 3 條），J 是其餘的部分。

## 摘要

加上 `MessageChannel`、`MessagePort`（`postMessage`、`start()`、`close()`、`onmessage`、`onmessageerror`）、transfer（`MessagePort` 和 `ArrayBuffer`，經過 port 的、window 的 `postMessage` 和 `structuredClone()`）、`MessageEvent.ports`，和 `structuredClone()`。port 是 `dom` 的 GC 物件，配對的兩個 port 在同一個 renderer 裡直接把訊息排進對方的 posted message task source（接在 RFC 0030 的 DOM task 上）；序列化從 `dom_js` 現有的「for storage」擴充成能 transfer 的版本（V8 的 `ValueSerializer` 的 host object 和 `transfer_array_buffer`，rusty_v8 已經有）。分兩段：第 1 段是 Speedometer 用到的（沒有 transfer），第 2 段是 transfer。

## 動機

**現況（2026-10-11，`main` 的 `299744ef`）**：
- 沒有 `MessageChannel`、`MessagePort`、transfer 和 `structuredClone()`（`docs/limitations.md` 的「事件」的 `postMessage`，和「Bindings 和 DOM API」的 `DOMException` 那一條）：`postMessage(message, targetOrigin, transfer)` 的第三個參數被忽略，`MessageEvent` 沒有 `ports`。
- **Speedometer**（RFC 0012 的盤點，2026-10-11）：default 的 20 個 suite 裡 TodoMVC-React-Complex-DOM、TodoMVC-React-Redux、TodoMVC-Lit-Complex-DOM、NewsSite-Next、Charts-chartjs 用到 `MessageChannel`（`details.md` 的「Speedometer 怎麼用」）。React 的 scheduler 有 `MessageChannel` 時用 port 的訊息排下一段工作，沒有時退回 `setTimeout(0)`（巢狀五層之後被夾到 4 ms）：kun 現在跑的是退回的那條路，和 Chrome 不是同一種跑法，RFC 0012 第一次量測的 React 系 suite 的比值（20.8 到 81.6 倍）因此不能直接比。Lit 的 suite 也用它（現在卡在更前面的 `CSSStyleSheet`）。
- WPT 的 `webmessaging/with-ports/` 等已經在清單裡，用到 port 的子測試是預期失敗。

目標：
- `new MessageChannel()`、`port.postMessage(message, transfer)`、`port.onmessage`（隱含 `start()`）、`addEventListener("message")` 加 `start()`、`close()`；訊息照順序、以 task 送到；沒被 script 拿著、但還配對著、有 listener 的 port 不會被 GC 收掉（Chrome 一樣）。
- transfer：`MessagePort` 和 `ArrayBuffer`（原本的那個變成 detached），在 port 的、window 的 `postMessage` 和 `structuredClone(value, { transfer })`；`MessageEvent.ports`、`MessageEventInit.ports`、`initMessageEvent` 的 `ports`。
- `structuredClone(value, options)`。
- Speedometer 的 React 系 suite 改走 port 那條路，重量一次。

不在範圍內：
- 跨源的 window 能用的成員（RFC 0009 的第 53 條，原本放在 J）：要先有真的跨源 iframe 的行為（現在跨源的 window 什麼都碰不了），和 port 無關。另外排進路線圖（第 1 條）。
- `BroadcastChannel`、worker（`Worker`、`SharedWorker`、`ServiceWorker`）和它們的 port。
- `messageerror`：序列化照舊是先檢查、反序列化不會失敗，在 kun 還是走不到（RFC 0030 的第 8 條）。
- 可序列化的平台物件（`DOMException`、`Blob`、`File`、`ImageData`……）：照舊 `DataCloneError`；`Blob` 是之後最可能要的（`Channel_postMessage_Blob.any.js`）。
- wasm 的 guest：`message` 是 `any`，等路線圖的 U（第 5 條）。

## 設計概要

對寫網頁的人：

```js
const { port1, port2 } = new MessageChannel();
port1.onmessage = (event) => console.log(event.data);   // 隱含 start()
port2.postMessage({ step: 1 });                          // 下一個 task 才送到

const buffer = new ArrayBuffer(8);
port2.postMessage(buffer, [buffer]);                     // buffer.byteLength 變成 0
iframe.contentWindow.postMessage("hi", "*", [port2]);    // 對方的 event.ports[0]
const copy = structuredClone(new Map([[1, { a: 2 }]]));
```

和 Chrome 一樣的地方：沒有 `onmessage`、只有 `addEventListener` 的 port 要 `start()` 才收得到；`close()` 之後兩邊都不再送；transfer 過的 port 和 `ArrayBuffer` 在原本的地方不能再用；把自己這個 port 放進 transfer 是 `DataCloneError`，放進配對的那個時訊息被丟掉（HTML 的 doomed）。

對改 kun 的人：
- `dom` 的 `EventTargetImpl` 多一種 message port，記著配對的那一個、它的 port message queue（序列化的訊息和帶著的 port）、有沒有 start、是不是關了、是不是 transfer 走了；`MessageChannel` 是新的 GC 型別。`DomTasks` 記著「配對著、已經 start」的 port，讓它們沒人拿著也不被收掉（Chrome 的 `MessagePort::HasPendingActivity`）。
- `dom_api` 的 `message_port` 模組：`MessageChannel`、`MessagePort` 的成員和送到時的 task（`TaskSource::PostedMessage`，和 window 的 `postMessage` 同一個）；`structuredClone` 在 `Window` 上。
- `dom_js` 的序列化多一種「能 transfer」的：port 是 host object，`ArrayBuffer` 用 `transfer_array_buffer` 交給對方、原本的 detach、backing store 跟著訊息走。`history.state` 照舊用「for storage」。

## 詳細設計

（比較在 `details.md`。）

- **port 的狀態**：port 是 `dom` 的 `EventTargetImpl` 的一種（`EventTargetKind::MessagePort`）：traced 的是配對的 port 的 edge（兩邊互指）和還沒送出的訊息帶著的 port（`queued_ports`，`EdgeChunks`，第 12 條）；會變的 `enabled`（start 過）、`closed`、`detached`（transfer 走了）、`queue`（還沒送進 task 的訊息，各帶幾個 port）是最外層的 `#[no_trace]` 欄位 `port_state`，和 abort signal 的 `abort_state` 一樣；`MessageChannel` 是自己的 GC 型別，照 `AbortController` 拿著兩個 port。port 的 wrapper 建在它的 realm（接收端的 port 是接收端的），不管哪個 realm 先讀到。`postMessage` 照 HTML 的「message port post message steps」：先序列化（含 transfer），自己被 transfer 走、配對的 port 不在或已關時丟掉訊息；配對的 port 的 queue 收下，對方 enabled 時排一個 posted message 的 task，task 跑時分派 `message`（`data` 照 RFC 0030 第 8 條第一次讀才反序列化，`ports` 是 frozen array）。`start()` 把 queue 裡的排成 task；`onmessage` 的 setter 隱含 `start()`。
- **GC**（第 8 條）：Chrome 的 `MessagePort` 是 `ActiveScriptWrappable`，配對著、start 過時不被收掉。kun 照做：start 過、還配對著的 port 的 handle 放在 `DomTasks` 的 `active_ports`（heap 外的 root，和 DOM 的 task 一樣），`close()`（兩邊都解除配對）、transfer、它的 realm 的 document 被丟棄、`DomIsolate` 拆掉時拿掉。論證在 `crates/dom/soundness.md`。
- **process 的邊界**：Chromium 即使在同一個 process 裡，port 之間也經過 mojo 的 pipe（`MessagePortDescriptor`）。kun 的 port 只能 transfer 給同一個 renderer 裡碰得到的 window（同源的 frame），所以訊息直接排進對方的 task；之後拆 process、跨 renderer 的 port（跨源 iframe 在另一個 process）時，queue 那一層換成經過 Browser 的通道（`docs/architecture.md` 的「Process 的邊界」記一條）。
- **序列化和 transfer**：HTML 的 `StructuredSerializeWithTransfer`，照規範的順序：transfer list 先檢查型別、重複和自己這個 port（`DataCloneError`；不是物件的值在轉換參數時就是 `TypeError`，第 15 條），再序列化，序列化之後才檢查 detached（序列化時的 getter 可能把它 transfer 走；`close()` 過的 port 也算，HTML 的 `close()` 會設 [[Detached]]），都過了才 detach。port 寫成 host object（一個索引，指向這次 transfer 的 port 清單），`ArrayBuffer` 交給 V8 的 `transfer_array_buffer`，序列化之後 detach、backing store 跟著訊息走（第 14 條）；不在 transfer list 裡的 port 不能複製（`DataCloneError`）。transfer 時，那個 port 已經排成 task 的訊息從 DOM 的 task 取回、放回它的 queue 的最前面，之後送給它的也進 queue；送到時 port 換成接收端 realm 的新 `MessagePort`，接手原本的配對和整個 queue（第 13 條）。訊息的目標 port 在 transfer list 裡時訊息丟掉（HTML 的 doomed），不丟錯誤。`structuredClone` 在呼叫的 realm 序列化、在 `this` 的 realm 反序列化，transfer 的 port 也在那裡建。rusty_v8 的 `ValueSerializerImpl` 有 `write_host_object`、`ValueDeserializerImpl` 有 `read_host_object`，`ValueSerializer`、`ValueDeserializer` 都有 `transfer_array_buffer`，不用改 fork。
- **event loop**（第 7 條）：timer 和 DOM 的 task 共用一個排入的序號（`Timers::sequence`，interval 跑過之後重新拿一個），renderer 每次從到期的兩者裡挑 `(due, 序號)` 最小的一個跑，跑完 microtask checkpoint。
- **`structuredClone()`**：`window` 的方法：`ScriptHost::serialize_with_transfer` 之後，經過新的 `ScriptHost::deserialize_in` 在 `this` 的 realm 反序列化（Chrome 152 一樣：別的 window 的 `structuredClone` 用 `call` 呼叫時，複製到 `this` 的 realm）。
- **window 的 `postMessage`**：`WindowPostMessageOptions.transfer` 和第三個參數接到同一個序列化；送到時帶著的 port 在目標的 realm 換成新的，是 `MessageEvent.ports`。
- **`MessageEvent.ports`**：`EventKind::Message` 裡 traced 的 `EdgeChunks`，建立時（送到的、`MessageEventInit.ports`）寫入，`initMessageEvent` 換掉；`FrozenArray<MessagePort>`（第 9 條）。
- **宣告表**：`MessageChannel`、`MessagePort` 是 `[JsOnly]`（`message` 是 `any`，第 5 條），`MessageEvent.ports` 進 union；`structuredClone` 在 `Window`；`transfer` 是 WebIDL 的 `object`（`ObjectValue`，第 15 條）。

### 測試

- `dom`（`message_ports`）：配對、queue、`close()`、start 過的 port 的 root；queue 裡的 port 由等的 port 撐住；detach 和接手（standalone heap，GC 不掃 stack）；GC 型別的共通檢查（`script::every_type_unwraps_only_as_itself`、`gc_stress::`）；timer 和 DOM 的 task 的順序（`timers`）。
- `dom_js`（`messaging`）：送到的順序、`start()` 前不送、`onmessage` 隱含 start、`close()`、transfer 的 port 和 `ArrayBuffer`、在途訊息的順序、transfer list 的錯誤和順序、doomed、跨 realm、`ports`、`structuredClone`。`shell`（`messaging`）：event loop 裡 timer 和訊息的順序和 Chrome 152 一樣。
- WPT：`webmessaging/` 的 channel、transfer 和 `multi-globals/`，`html/webappapis/structured-clone/`、`html/infrastructure/safe-passing-of-structured-data/` 的能跑的（`tests/wpt/include.txt` 的註解寫明收了什麼、沒收什麼）。
- Chrome 152 的實測和 Speedometer 的重量記在 `progress.md`。
- mutation test：`mutants-1.toml`、`mutants-2.toml`。

### 分階段

1. **port 和 `structuredClone()`，沒有 transfer**（2026-10-11）：`MessageChannel`、`MessagePort` 的成員、GC、task，`MessageEvent.ports`（空的 frozen array），`structuredClone(value)`。Speedometer 用到的只有這些；做完重量 React 系的 suite。
2. **transfer**（2026-10-12）：port 和 `ArrayBuffer`，在 port 的、window 的 `postMessage` 和 `structuredClone` 的 `transfer`；`MessageEvent.ports`、`MessageEventInit.ports`、`initMessageEvent` 的 `ports`。

## 缺點

- **改到 `dom` 的 GC**：多一個 GC 型別，和一份「有 pending activity 的」強引用；漏拿掉會漏記憶體（Chrome 也一樣：配對著、start 過、沒 close 的 port 一直活著）。
- **和 Chrome 的 mojo 那一層不同**：同一個 renderer 裡直接排 task，之後拆 process、要跨 renderer 時要補上通道。
- **只給 JS**：wasm 文件用不到，要等 U。
- **沒送到的訊息帶著的 port 留著**：目標的 document 被丟棄、`targetOrigin` 不符、doomed 時，那些 port 一直是 detached，配對的另一個送過去的訊息留在它的 queue 裡（Chrome 的 pipe 會關閉；`docs/limitations.md`）。

## 理由和其他做法

各項的理由在「已決定的事項」，完整的比較在 `details.md`。重點：
- port 是 `dom` 的 GC 物件（第 2 條），照 Chrome 的 `MessagePort`：listener、`onmessage` 和其他 `EventTarget` 一樣寫，transfer 時就是搬這個物件。
- 訊息在同一個 renderer 裡直接排 task（第 3 條）：kun 的 port 碰不到別的 renderer，照 Chromium 經過通道現在只有成本。
- 序列化用 V8 的 `ValueSerializer`（第 4 條）：V8 已經會，rusty_v8 也已經公開 transfer 要的 hook。

## 先例

- **Chromium**：`third_party/blink/renderer/core/messaging/`：`MessageChannel`、`MessagePort`（`EventTarget`、`ActiveScriptWrappable`，`HasPendingActivity` 是 start 過、還連著）、`BlinkTransferableMessage`（`SerializedScriptValue` 加上 transfer 的 port）；port 之間是 mojo 的 message pipe（`MessagePortDescriptor`），同一個 process 裡也一樣。`SerializedScriptValue` 用 V8 的 `ValueSerializer`，port 和 `ArrayBuffer` 的 transfer 經過它的 delegate（`V8ScriptValueSerializer::WriteHostObject`）。`structuredClone` 是 `WindowOrWorkerGlobalScope` 的，序列化再反序列化。
- **RFC 0030**：window 的 `postMessage`、`MessageEvent`、`TaskSource::PostedMessage`、`ScriptHost::serialize`，這份接在它們上面。
- **React 的 scheduler**：`MessageChannel` 存在時用 `port2.postMessage(null)` 排下一段工作（比 `setTimeout(0)` 不受 4 ms 的夾限），沒有時退回 `setTimeout`。

## 已決定的事項

2026-10-11 確認第 1 到 6 條（比較在 `details.md` 的「選項」）。

1. **R1：範圍是 port、transfer 和 `structuredClone()`，不含跨源 window 的成員**（2026-10-11）：`MessageChannel`、`MessagePort`、transfer（port 和 `ArrayBuffer`，在 port 的、window 的 `postMessage` 和 `structuredClone`）、`MessageEvent.ports`。原本放在 J 的跨源 window 的成員（RFC 0009 的第 53 條）要先有跨源 iframe 的行為，和 port 無關，另外排進路線圖。不選 R2（只做 Speedometer 用到的、沒有 transfer：WPT 的 port 測試大多用 transfer，`ports` 會一直是空的）、R3（連同跨源 window 的成員）。
2. **G1：`MessagePort` 是 `dom` 的 GC 物件**（2026-10-11）：`EventTarget`，配對的 port 是 edge；`DomIsolate` 一份「配對著、start 過」的強引用，有 listener 的 port 沒有 script 的引用也不被收掉，`close()`、document 結束時拿掉（Chrome 的 `ActiveScriptWrappable`、`HasPendingActivity`）。改到 GC，照 `crates/dom/soundness.md` 寫論證。不選 G2（renderer 的表、port 只是 id：`EventTarget` 的部分和 transfer 的對應都要另外接）。
3. **P1：同一個 renderer 裡，訊息直接排進對方的 posted message task**（2026-10-11）：接在 RFC 0030 的 `TaskSource::PostedMessage` 上。Chromium 在同一個 process 裡也經過 mojo 的 pipe；kun 的 port 只能 transfer 給同一個 renderer 碰得到的 window，之後拆 process、跨 renderer 時再補通道（記進 `docs/architecture.md` 的「Process 的邊界」）。不選 P2（現在就經過 `protocol` 的通道：只有成本）。
4. **S1：transfer 用 V8 的 `ValueSerializer`**（2026-10-11）：`dom_js` 的序列化多一種能 transfer 的，port 是 host object，`ArrayBuffer` 用 `transfer_array_buffer`、序列化之後 detach；rusty_v8 已經有 `write_host_object`、`read_host_object`、`transfer_array_buffer`，不改 fork。`history.state` 照舊用「for storage」。Chromium 的 `SerializedScriptValue` 一樣經過 `ValueSerializer` 的 delegate。不選 S2（自己寫結構化複製）。
5. **W1：`MessageChannel`、`MessagePort` 只給 JS（`[JsOnly]`）**（2026-10-11）：`message` 是 `any`，和 window 的 `postMessage` 一樣等路線圖的 U；這是 RFC 0012 第 7 條允許的例外，記進 `docs/limitations.md`。不選 W2（進 WIT、只有 `start`、`close`：guest 收發不了訊息）。
6. **T1：分兩段**（2026-10-11）：第 1 段沒有 transfer（Speedometer 用到的），做完重量 React 系的 suite；第 2 段 transfer。GC 的改動和序列化的改動分開審查。不選 T2（一次做完）。

2026-10-11 確認第 7 到 11 條（第 1 段開始前）。

7. **E1：event loop 照 Chrome，從到期的 timer 和 DOM 的 task 裡挑最早排進來的一個跑**（2026-10-11）：比 `due`，同時間的比全域的排入序號，一次一個，之後 microtask checkpoint。Chrome 152 的實測（`progress.md`）：`setTimeout(0)` 和 port 的訊息照排進來的先後、一次一個交錯（五層的鏈是 t0 m0 t1 m1 t2 m2 t3 m3 m4 t4，第五層的 timer 被夾到 4 ms），`postMessage` 在下一個 rAF 之前。kun 原本先跑完到期的 timer 再跑 DOM 的 task，React 的工作會一直排在 timer 後面。不選 E2（維持原本的順序：React 系 suite 的量測和 Chrome 不是同一種跑法）。
8. **K1：port 配對著、start 過、沒 close 就不被收掉**（2026-10-11）：照 Chrome 的 `MessagePort::HasPendingActivity`，不看有沒有 listener；document 結束時拿掉它的 port。兩個沒人引用、互相配對、start 過的 port 活到 `close()` 或 document 結束（Chrome 一樣）。不選 K2（再要求有 `message` listener：和 Chrome 收的時機不同）。
9. **F1：`MessageEvent.ports` 第 1 段就宣告成 `FrozenArray<MessagePort>`**（2026-10-11）：第 1 段一律是空的，第 2 段有 transfer 的 port。和 Chrome 的形狀一樣；RFC 0030 的第 9 條等的就是 `MessagePort` 這個型別。不選 F2（等第 2 段）。
10. **X1：第 1 段遇到非空的 transfer 丟 `DataCloneError`**（2026-10-11；第 2 段做完之後由 transfer 取代）：`port.postMessage(m, [x])`、`structuredClone(v, { transfer: [x] })`，訊息寫明還沒支援，記進 `docs/limitations.md`；第 1 段可能先 merge 回 `main`，靜靜忽略會讓網頁以為成功了。window 的 `postMessage` 的第三個參數照舊忽略，第 2 段一起處理。不選 X2（靜靜忽略）。
11. **W1：WPT 照 RFC 0030 的做法，開始時試跑候選、大部分能跑的目錄才整個加**（2026-10-11）：`webmessaging/message-channels/`、`MessagePort_*`、`Channel_postMessage_*`，`html/webappapis/structured-clone/`、`html/infrastructure/safe-passing-of-structured-data/` 裡 `structuredClone` 的部分；用到 transfer 的第 2 段再收。

2026-10-11 確認第 12 到 15 條（第 2 段開始前）。

12. **Q1：還沒送出的訊息帶著的 port，由接收的 port 上 traced 的清單撐住**（2026-10-11）：照 `AbortSignal` 的 `EdgeChunks`；接收的 port 沒人拿著、也沒 start 時，queue 和裡面的 port 一起被收掉。Chrome 的訊息在 mojo 的 pipe 裡，接收端被收掉時 pipe 關閉、裡面的 port 跟著關。不選 Q2（heap 外的 handle：接收的 port 被收掉後，裡面的 port 要等 document 丟棄才放掉）。
13. **B3：transfer 時把已經排成 task 的訊息取回 port 的 queue**（2026-10-11 定 B1，2026-10-12 改成 B3）：規範的 transfer 把 port message queue 整個搬給新的 port。detach 時把那個 port 還沒跑的 message task 從 DOM 的 task 取回，照順序放在它的 queue 最前面，之後送給它的也進 queue（它的 queue 不再開著）；接手的 port 拿走整個 queue，start 時照順序送。Chrome 的訊息在 mojo 的 pipe 裡、一個 task 讀一則，沒讀的跟著 pipe 走，結果一樣。原本的 B1（原本的 port 記一條 edge 指向新的，task 跑時一路轉交）實作之後發現順序不對：transfer 之後送的訊息先進了 queue，之前排好的 task 晚一點跑、排在它們後面（或轉交時直接分派，不等新的 port start），要補每則訊息的序號才能修。不選 B2（丟掉：queue 開了的 port 被 transfer 時，在途的訊息不見）、B4（保留轉交、加上排序：兩條路徑都要維持順序）。
14. **T1：`ArrayBuffer` 的 transfer 照 Chrome 搬 backing store**（2026-10-11）：序列化時 V8 只寫編號，原本的 buffer detach，它的 `BackingStore` 跟著訊息走（`dom` 的 V8 模組的型別，和 `ScriptValue` 一樣），反序列化時在對方的 realm 用同一個 backing store 建新的 `ArrayBuffer`。不選 T2（複製再 detach：transfer 就是為了不複製）。
15. **O1：宣告表加上 WebIDL 的 `object`**（2026-10-11）：只給 JS（`kun:dom` 沒有），JS 的前端轉換參數時，不是物件的值丟 `TypeError`；`transfer` 宣告成 `sequence<object>`，和 Chrome 一樣。不選 O2（維持 `sequence<any>`：不是物件的值丟 `DataCloneError`）。

## 待決定

- 跨源 window 的成員排進路線圖的哪裡（不在這份）。

## 之後的可能

- 跨源 window 能用的成員。
- `BroadcastChannel`；worker 和它們的 port。
- 可序列化的平台物件（`Blob`、`DOMException`、`ImageData`）。
- 拆 process 之後，跨 renderer 的 port 經過 Browser。
- wasm 的 guest 收發訊息（路線圖的 U 之後）。
