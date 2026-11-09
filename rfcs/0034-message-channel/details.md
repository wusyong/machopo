# RFC 0034 的細節：`MessageChannel` 和 `structuredClone()`

## Speedometer 怎麼用

RFC 0012 的盤點（Chrome for Testing 152 記錄，2026-10-11 的 `chrome.json`）裡用到 `MessageChannel` 或 `MessagePort` 的 suite：

| Suite | default | 用到的 |
| --- | --- | --- |
| TodoMVC-React-Complex-DOM | 是 | `new MessageChannel`、`port1`、`port2`、`MessagePort.onmessage=`、`MessagePort.postMessage` |
| TodoMVC-React-Redux | 是 | 同上 |
| TodoMVC-Lit-Complex-DOM | 是 | 同上 |
| NewsSite-Next | 是 | `MessagePort.postMessage` |
| Charts-chartjs | 是 | `new MessageChannel`、`port1`、`port2`、`MessagePort.onmessage=`（沒有送） |
| TodoMVC-React | 否 | 同 React-Complex-DOM |
| TodoMVC-React-Redux-Complex-DOM | 否 | `MessagePort.postMessage` |
| TodoMVC-WebComponents-Complex-DOM | 否 | `new MessageChannel`、`port1`、`port2`、`MessagePort.onmessage=` |

沒有一個用到 transfer、`structuredClone()` 或 `MessageEvent.ports`：第 1 段（沒有 transfer）就夠 Speedometer 用。

React 的 scheduler（`scheduler` 套件）挑排下一段工作的方式：有 `setImmediate`（Node、舊的 IE）用它，否則有 `MessageChannel` 用 `port2.postMessage(null)`、在 `port1.onmessage` 裡做，都沒有才用 `setTimeout(0)`。`setTimeout` 巢狀五層之後最少 4 ms，所以 kun 現在的 React 系 suite 在 test 之間可能多等；Speedometer 量的 async 是到下一個 frame 畫完，React 排的工作在不在那之前，取決於 port 的訊息和 timer、rAF 的先後（待決定裡的實測）。

## Chromium 的做法

- `MessageChannel`（`core/messaging/message_channel.cc`）建一對 `MessagePortDescriptor`（mojo 的 message pipe 的兩端），各自包成 `MessagePort`。
- `MessagePort`（`message_port.cc`）是 `EventTarget`、`ActiveScriptWrappable`：`HasPendingActivity()` 是「start 過、還連著」，所以配對著、有 listener 的 port 沒有 script 的引用也不會被收掉。`start()` 開始從 pipe 收訊息；`onmessage` 的 setter 隱含 `start()`（`DEFINE_ATTRIBUTE_EVENT_LISTENER` 之外特別處理）。`close()` 關掉 pipe，對方收到 disconnect。
- 送：`PostMessage` 把 `SerializedScriptValue`、transfer 的 port（`MessagePortDescriptor`）、`ArrayBuffer` 的內容包成 `BlinkTransferableMessage`，寫進 pipe。收：在 posted message 的 task 裡把 transfer 過來的 descriptor 包成新的 `MessagePort`，建 `MessageEvent`（`data` 反序列化）。
- 序列化：`V8ScriptValueSerializer`（V8 的 `ValueSerializer` 的 delegate）把 port 寫成 host object（一個 tag 加上在 transfer 清單裡的索引），`ArrayBuffer` 照 transfer list 交給 V8 的 `TransferArrayBuffer`，序列化之後把內容拿走、原本的 detach。
- `structuredClone`（`WindowOrWorkerGlobalScope`）：同一個 realm 序列化、反序列化，transfer 照樣有效。

## 選項

### 範圍（R）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **R1**（選了，第 1 條） | port、transfer、`structuredClone`；不含跨源 window 的成員 | WPT 的 port 測試大多用 transfer；`MessageEvent.ports` 有意義 | 比 Speedometer 要的大（第 2 段） |
| R2 | 只做 Speedometer 用到的，沒有 transfer | 最小 | `ports` 一直是空的、transfer 照舊忽略；之後還要回來 |
| R3 | 連同跨源 window 的成員 | 路線圖原本寫的 J | 要先有跨源 iframe 的行為（現在全部 `TypeError`），和 port 無關 |

### port 放在哪（G）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **G1**（選了，第 2 條） | `dom` 的 GC 物件（`EventTarget`），配對的 port 是 edge，`DomIsolate` 一份「配對著、start 過」的強引用 | 和 Chrome 的 `MessagePort` 一樣；listener、`onmessage` 照其他 `EventTarget` 的寫法 | 改到 GC，要寫 soundness 的論證 |
| G2 | renderer 的表，port 只是 id | GC 簡單 | `EventTarget` 的部分要另外接；transfer 時 id 的對應也要自己管 |

### 訊息怎麼走（P）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **P1**（選了，第 3 條） | 同一個 renderer 裡，直接排進對方的 posted message task | 簡單；kun 的 port 只能 transfer 給同一個 renderer 碰得到的 window | 拆 process、跨 renderer 時要補通道 |
| P2 | 照 Chromium 經過 `protocol` 的通道（mojo 的角色） | 拆 process 時不用改 | 現在只有成本：每則訊息多一次跨 thread |

### 序列化（S）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **S1**（選了，第 4 條） | `dom_js` 的序列化加上 transfer：port 是 host object，`ArrayBuffer` 用 `transfer_array_buffer` | V8 已經會；rusty_v8 有 `write_host_object`、`read_host_object`、`transfer_array_buffer`，不改 fork | `dom_js` 多一個 delegate |
| S2 | 自己寫結構化複製 | 和 engine 無關 | 重做 V8 已經會的；格式和 `history.state` 的不同 |

### wasm（W）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **W1**（選了，第 5 條） | `MessageChannel`、`MessagePort` 是 `[JsOnly]` | 和 window 的 `postMessage` 一樣，等 U 的 `any` | wasm 文件沒有 |
| W2 | 進 WIT，只有 `start`、`close` | 有 resource | 沒有 `postMessage` 和 `data`，guest 用不了 |

### 分段（T）

| 選項 | 做法 | 好處 | 壞處 |
| --- | --- | --- | --- |
| **T1**（選了，第 6 條） | 第 1 段沒有 transfer（Speedometer 用到的），第 2 段 transfer | GC 的改動和序列化的改動分開審查；第 1 段做完就能重量 React 系的 suite | 兩段 |
| T2 | 一次做完 | 一次 | 審查的範圍大 |

## WPT

目錄的大小（`../third_party/wpt`，2026-10-11；`resources/` 以外的 `.htm*` 和 `.js`）：`webmessaging/` 151（其中 `message-channels/` 14、`with-ports/` 24、`without-ports/` 30）、`html/webappapis/structured-clone/` 6、`html/infrastructure/safe-passing-of-structured-data/` 44。`webmessaging/with-ports/`、`without-ports/`、`with-options/` 和幾份 `webmessaging/*.htm` 已經在 `tests/wpt/include.txt`（RFC 0030），用到 port 的子測試現在是預期失敗。哪些加進清單在各段開始前試跑決定。
