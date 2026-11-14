# RFC 0098：binding 直接呼叫 V8：不經過 scope、不登記 root、只有一個 front（路線圖）

- 開始日期：2026-11-14
- 狀態：📝 草稿
- 細節：無

## 摘要

RFC 0097 之後，最熱的路（getter、字串參數）已經只拿 V8 的 callback info、直接讀寫。其餘的路還有三種「中間層」：rusty_v8 的 scope（94 個方法當場開一個，另有幾類每次 callback 都開）、elise 的 root（一次呼叫裡為了跨過操作而登記、釋放）、和第二個 front（會跑 script 的呼叫在 `WholeFront` 上跑）。這份是路線圖：目標是 **一次 binding 的呼叫在 V8 那一側做的事，就是 Blink 產生的 callback 會做的那幾個 V8 API 呼叫**，沒有別的。分三條線（scope、root、front）、六個里程碑，每個里程碑輪到時各開一份 RFC；這份只定目標、終點的定義、量法、順序和各里程碑的範圍。

追求的是效能和一致性兩者，但**預期換到的主要是一致性**：scope 本身只有 27 條指令；root 一個約 80 到 110 條，是最貴的一種，最常呼叫的成員（`nodeType`、`getAttribute`、`createElement`）不碰它，但 M0 數到 Speedometer 上每五次呼叫有一次登記 root（`classList`、集合的索引、`TreeWalker`；數字在 `progress.md`）。binding 現在比 Chrome 慢的地方（RFC 0097 的收尾量測）大多和這三種中間層無關：最熱的 getter 的 1.3 倍是進出 callback 和 unwrap 的檢查，其餘是 DOM 那一側的序列化和解析；和 root 有關的只有「走訪沒有 wrapper 的節點」（1.51 倍，root 估計只佔差距的一部分）。每一種中間層在實際的頁面上到底佔多少，由 M0 量；量出來不值得的可以留著（決定 9）。

## 動機

**現況**（2026-11-14，machopo `3857a0e`；照原始碼盤點）：

一次成員的呼叫，V8 那一側現在可能經過這些：

| 中間層 | 是誰的 | 管什麼 | 成本（RFC 0097 的 4k 量的） |
| --- | --- | --- | --- |
| scope（`CallbackScope`） | rusty_v8 | 通行證：它的 safe API（建物件、丟例外、呼叫函式）要一個 scope 才能呼叫。V8 自己在呼叫 callback 前已經開好 handle scope，這是 Rust 這一側另外的一份紀錄 | 27 條指令 |
| root（`Persistent`、`NodeRoot`…） | elise | DOM 物件要活過安全點（兩個操作之間）時登記 | 登記加釋放約 80 到 110 條 |
| 操作（`read`、`mutate`） | elise | 拿 DOM 的指標的範圍，裡面不 GC | 讀 20 條、寫 44 條 |
| 第二個 front（`WholeFront`） | `dom_js` | 整個呼叫在一個 scope 裡，給會跑 script 的呼叫 | 殼本身量不出差別（4m-3） |

**還開 scope 的地方**：

- `InfoFront` 當場開一個、用完就丟的 94 個方法：結果要 V8 配置的（陣列、物件、dictionary、promise，約 27 個）、參數整個讀成 DOM 的值的（sequence、structured clone、keyframes…，約 26 個）、讀 dictionary（17 個）、參數要轉換或檢查失敗（約 13 個）、丟例外和 reject（3 個）、預先建新節點的 wrapper（`createElement`、`cloneNode` 這類，每次都開）、找別的 realm 的 window（3 個）、回報 DOM heap 的大小或排 microtask、`setTimeout` 的字串、編譯 event handler。
- 整個呼叫在一個 scope 裡（`WholeFront`）：會跑 script 的 34 個成員、custom element reactions、跨 origin 的 15 個屬性、`document.all(..)`。
- rusty_v8 的 trampoline（V8 一呼叫就先建好 scope）：iterator（`entries`、`keys`、`values`、`next`、`forEach`）、named 和 indexed property 的 interceptor 裡還沒換的（查詢、刪除、列舉、定義、indexed setter）、window 的 named properties、跨 origin 的 access check 的 handler、回傳 promise 的成員外面那一層（`rejecting`，約 32 個成員）、`DOMException` 的三個 getter、observable array 的 handler，和內部的函式（microtask 的 job、module 的接續、`state.rs` 的三個）。

**還登記 root 的地方**（`dom_js` 35 處、`dom_api` 161 處；其中一部分是存起來的東西，本來就該是 root）：

- 回傳沒有 wrapper 的節點或物件：建 wrapper 要 V8 配置，V8 不能在操作裡配置，所以節點先登記、離開操作、建 wrapper、再開一個操作接上。RFC 0097 的 4k-D 只把「建 wrapper」改成不開 scope，root 還在。
- 會跑 script 的成員的 `this` 和參數：呼叫 JS 的前後都要用。寫成對 `ScriptHost` 泛型的函式有 183 個。
- 帶出操作的結果（物件的陣列、要 resolve 的物件）。

**為什麼要做**：

- **效能**：root 是這些路上最貴的單項。「V8 在操作裡配置」（RFC 0097 用 TLA+ 驗證過、沒有做的三條規則）或便宜的 root 都還沒做。`createElement` 每次開的那個 scope 是熱的路上剩下的最後一個。
- **一致性**：現在「哪條路直接呼叫 V8、哪條經過 scope」沒有規則可循，是照 RFC 0097 做到哪裡決定的。改 binding 的人要知道三套寫法（只拿 info、`Scoped`、`WholeFront`）。兩個 front 各有一份 `Front` 和操作那一側的實作；`WholeFront` 有三條路現在沒有成員走得到（`docs/test-gaps.md`）。

**目標**：見「設計概要」的「終點的定義」。

**要解決的 `docs/limitations.md` 的限制**：無。`docs/test-gaps.md` 的「測試守不住的地方」裡 `WholeFront` 在操作後做字串、沒有 wrapper 的節點、不是 Smi 的數字的那一列，在 front 併起來時消失。

**不在範圍內**：

- DOM 那一側的工作（序列化、解析、selector、屬性和名稱的查找）：`docs/performance-gaps.md` 的 DOM 那一條。
- 最熱的 getter 剩下的差（進出 callback 存的暫存器、型別的 tag 不是常數、unwrap 比 tag、cage 的檢查）：RFC 0097 的「之後的可能」。它們和這份的三種中間層無關，但可以排在這份的空檔做。
- wasm 的 front（`dom_wasm`）：它不經過 V8。
- 保守地掃 stack：見「理由和其他做法」。

## 設計概要

對網頁沒有看得到的變化。對改 binding 的人，做完之後只有一種寫法。

**終點的定義**（每一條都可以用測試或 `sanity.py` 檢查）：

1. **成員的呼叫路徑上不建 rusty_v8 的 scope**。V8 那一側要做的事（建字串、物件、陣列，讀寫屬性，丟例外，呼叫函式，接住例外）都經過 fork 裡直接對應 V8 的 C++ API 的入口，拿的是 isolate、context 和 callback 的 handle scope 裡的 handle。例外要明列、附理由（預期只剩每個 interface 跑一次的初始化和測試用的函式）。
2. **一次呼叫裡不登記 elise 的 root**。root 只給真的要活過這次呼叫的東西（存起來的 listener、timer、pending 的 promise）。呼叫當中要跨過安全點的物件，用比 root 便宜、或根本不需要登記的機制（見 R 線）。
3. **V8 那一側只有一個 front**。會跑 script 的呼叫也在它上面跑；`WholeFront`、`WholeOp`、`Call` 刪掉。
4. **callback 一律是 V8 原始的 callback**：function、interceptor、iterator、回傳 promise 的成員外面那一層，都不經過 rusty_v8 會先建 scope 的 trampoline。
5. **不變慢**：每個里程碑對 `main` 量，熱的呼叫的 cycle 和 store forwarding 的衝突數不增加（見「量法」）。

對照 Blink（「先例」）：它產生的 callback 就是這個樣子。handle 直接放在 V8 替 callback 開的 handle scope 裡，沒有另一層 scope；Oilpan 保守地掃 stack，所以 callback 裡的裸指標不用登記；每個成員的程式是產生出來的，沒有第二條路。

## 詳細設計

這份不定每個里程碑的設計，只定它們的範圍、順序和彼此的關係。每個里程碑開始時另開一份 RFC，照流程先列那一段要決定的事。

### 三條線

**S 線：scope**

每條路換成直接呼叫它該呼叫的 V8 API。難度看的是那條路會不會執行 script、要不要接住例外：

| 路 | 要直接呼叫的 V8 API | 備註 |
| --- | --- | --- |
| 預先建 wrapper、回報 heap 的大小 | 複製 boilerplate（入口已經有）、`AdjustAmountOfExternalAllocatedMemory` 這一類 | 最熱、最便宜 |
| 丟例外、reject | `ThrowException`、建 `TypeError` 和 `DOMException` 的物件、`Promise::Resolver` | 不執行 script |
| 結果：陣列、物件、dictionary、record、promise | `Array::New`、`Object::New`、`CreateDataProperty`、`SetIntegrityLevel` | 不執行 script |
| iterator、interceptor、`rejecting`、`DOMException` 的 getter | 換成 V8 原始的 callback 型別；`rejecting` 要 `TryCatch` | trampoline 重寫 |
| 讀 dictionary、參數的轉換（`ToNumber`、`ToString`、迭代 sequence） | `Object::Get`、`Value::To*`、iterator protocol | 會執行 script，但不接例外：失敗時 V8 留著 pending 的例外，callback 直接返回就好 |
| 呼叫 listener、filter、callback | `Function::Call`、`TryCatch`、`MicrotasksScope` | 會執行 script、要接例外、要回報 |

每個新入口是 fork 裡的 `unsafe`，論證寫進 `crates/bindings/soundness.md`（多半是同一句：handle 活在 V8 替 callback 開的 handle scope 裡）。

**R 線：root**

三種機制，各自拿掉不同地方的 root，可以並用：

| 機制 | 拿掉哪裡的 root | 成本和風險 |
| --- | --- | --- |
| 甲：只活在一次呼叫裡的便宜的 root（像 V8 的 handle scope：一個 stack 式的表，登記是寫一格、離開時整批放掉） | 全部「呼叫當中跨過安全點」的：回傳沒有 wrapper 的節點、物件的陣列、會跑 script 的成員的 `this` 和參數。函式的簽名不用改 | elise 多一種 root，要補 GC 的 soundness 論證、模型和測試。RFC 0097 估登記約 10 條（估計，沒有原型） |
| 乙：V8 可以在操作裡配置 | 回傳沒有 wrapper 的節點和物件：wrapper 在操作裡當場建，不用離開操作，連第二個操作都省了 | 三條規則在 TLA+ 的模型上驗證過（RFC 0097）。動到 elise 的 marking 的開始和收尾，是 GC 最容易出錯的地方；規則 2 發生時那一輪什麼都不收 |
| 丙：`this` 和參數拿 wrapper、不拿 root | 會跑 script 的成員的 `this` 和參數（wrapper 活著就撐著 DOM 物件，callback 的參數整個呼叫期間被 V8 留著） | 不碰 GC。要改 183 個函式和它們之間傳的型別（RFC 0097 估兩三千行） |

甲涵蓋最廣、單項省得最少；乙省得最多但只涵蓋一類、風險最高；丙不碰 GC 但改動最大。選哪幾個、先後，是 R 線第一個里程碑要用原型和數字決定的（「待決定」1）。

**F 線：front**

把 `WholeFront` 的使用者一個一個搬到 `InfoFront`：

1. legacy caller（一種新的 `InfoKind`：receiver 沒有被 signature 檢查過）、iterator 的 pairs、舊的 indexed 的路。
2. 跨 origin 的屬性：getter 要先進 receiver 的 realm，`bindings` 要一個在 info 上進入另一個 context 的入口。
3. 會跑 script 的 34 個成員：`InfoFront` 實作 `ScriptFront`，script 的狀態（`define` 讀到的 callback、filter 丟的例外）搬過來，放在平常是空的一格裡，不讓熱的路多清欄位。
4. 刪掉 `WholeFront`、`WholeOp`、`Call`、`RareCall`。

`PropertyFront`（named property 的刪除、查詢、列舉，indexed setter）是另一種 callback（interceptor 的 info）：它跟著 S 線的 interceptor 那一項一起看（「待決定」2）。

### 三條線的關係

- **F 線的第 3 步依賴 S 線的最後一項**：會跑 script 的成員搬到只拿 info 的 front 上，就需要「不經過 scope 呼叫 JS」。不等它的話，可以先用當場開的 scope 呼叫，之後再換；但那樣會動這 34 個成員兩次。
- **R 線的甲、丙和 F 線的第 3 步碰同一批函式**（會跑 script 的成員的 `this` 和參數）。放在同一個里程碑，只動一次。
- **R 線的乙讓 S 線的「預先建 wrapper」不需要**：wrapper 可以在操作裡當場建，就不用在操作前猜 interface。所以乙如果要做，排在那一項之前才不會白做；不做的話那一項照舊。

### 里程碑

| | 內容 | 線 | 預期的收穫 |
| --- | --- | --- | --- |
| M0 | 量測的基準和工具：照路徑數一次呼叫開幾個 scope、登記幾個 root、開幾個操作（Speedometer 和 blink_perf 上各走到幾次）；`bench.py calls` 加冷的路；驗收的腳本連同 cycle 和 store forwarding 的衝突 | | 知道每條路被走到幾次，之後的順序照它調整 |
| M1 | root 的機制：甲、乙的原型和量測，選定之後做進 elise | R | 回傳沒有 wrapper 的節點和物件（約 80 到 110 條一個） |
| M2 | 不執行 script 的路直接呼叫 V8：預先建 wrapper、回報大小、丟例外、reject、陣列和物件的結果 | S | `createElement` 這類少一個 scope；丟例外和回傳陣列的路 |
| M3 | callback 都是 V8 原始的：iterator、interceptor（連同 `PropertyFront`）、`rejecting`、`DOMException` 的 getter | S、F | `for…of`、`forEach`、回傳 promise 的 API |
| M4 | 會執行 script 的讀取直接呼叫 V8：dictionary、參數的轉換、sequence | S | 帶 dictionary 的建構子、非原始型別的參數 |
| M5 | 呼叫 JS 不經過 scope；會跑 script 的成員搬到唯一的 front，`this` 和參數不登記 root；刪掉 `WholeFront` | S、R、F | `dispatchEvent`、`click`、走訪；程式少一份 front |
| M6 | 收尾：`sanity.py` 檢查終點的五條、文件、對 Chrome 的量測 | | |

順序的理由：先量（M0），因為 RFC 0097 好幾次估計不準；root 排在最前面（M1），因為它單項最貴，而且它的結果決定 M2 的「預先建 wrapper」還要不要；之後由不執行 script 的（簡單、熱）到會執行的（難）；併 front 放最後，因為它依賴前面全部。M0 的數字出來之後，M2 到 M4 的先後可以對調。

每個里程碑做完都是完整的狀態（可以 merge、可以停）。

### 量法

RFC 0097 每一段用指令數驗收，漏掉了 store forwarding 造成的退步（`createElement` 的指令數變少、整頁慢 12%，收尾才發現）。這份每個里程碑的驗收：

- `bench.py calls`：指令數、cycle、store forwarding 的衝突數三個一起看，受影響的呼叫和最熱的幾個（`nodeType`、`getAttribute`、`createElement`）都不變差。
- `bench.py blink-perf --against main`：`bindings/` 的頁（一定包含 `create-element`、`gc-forest`）和那個里程碑動到的頁，各三次。
- 對 Chrome 的腳本（RFC 0097 的 `chrome-compare.py`）：那個里程碑動到的呼叫。
- 計數的測試：換過的路斷言「這次呼叫沒有建 scope」「沒有登記 root」（debug build 的計數，照 `docs/test-gaps.md` 的做法）。
- Speedometer 照規定只在使用者要求時量。建議 M1 和 M5 做完時各量一次。

### 測試

- 行為：`dom_js`、`dom_api`、`bindings` 的測試和相關的 WPT 目錄；動到 GC 的里程碑（M1，可能還有 M5）加 elise 的測試、模型、fuzz，merge 之後手動開始 `gc.yml`。
- 每個里程碑的 mutation test 照 `docs/testing.md`。
- 終點的五條在 M6 做成 `sanity.py` 的檢查（例如 `dom_js` 的成員的路上不出現 `with_scope`、V8 那一側只有一個 `impl Front`），之後不會退回去。

### 分階段

見「里程碑」。這份 RFC 自己只做 M0；M1 到 M6 各開一份 RFC，在這份的 `progress.md` 記每個里程碑的狀態和它的 RFC 的編號。

## 缺點

- **fork 的表面變大**：S 線每一項都要在 rusty_v8 的 fork 加直接對應 V8 API 的入口，估計數十個。每個都是 `unsafe`、要在升級 rusty_v8 和 V8 時跟著對。
- **整份換到的速度可能很小**：binding 現在落後 Chrome 的主因不在這三種中間層（「摘要」）。root 在 Speedometer 上佔多少沒有現在的數字（最後一次是 2026-10-24、DOM 還在 cppgc 上時的 2.2%）。
- **scope 那條線換到的速度很小**：一個 scope 27 條，多數冷的路本體是幾百到上萬條。RFC 0097 的 4m-3 量到換殼對這些路沒有可見的影響。這條線主要是為了一致。
- **R 線動到 GC**：甲要新的 root 種類，乙動 marking 的開始和收尾。
- **M5 動行為最複雜的 34 個成員**：重入、例外在哪一步丟、microtask 什麼時候跑、reaction 的順序。
- **時間長**：六個里程碑，其中 M1 和 M5 各自都不小。
- rusty_v8 的 safe API 用型別保證「在對的 isolate 和 context 裡」；直接呼叫之後，這個保證改由 `bindings` 的論證負責。

## 理由和其他做法

- **只併 front，不處理 scope 和 root**：得到的只有少一份程式。會跑 script 的呼叫還是要一個 scope，root 照舊。
- **只做 root**：效能的收穫大半在這裡，但寫法還是三套。
- **保守地掃 stack**（讓 callback 裡的裸指標不用登記，像 Oilpan）：一次解決 root 的問題。代價是要在 Rust 裡掃 stack（暫存器、每個平台），elise 對外的 API 的安全性不再能用型別證明，有 nursery 的 heap 的物件也不能搬。RFC 0087 和 0097 都評估過、不選。
- **不做**：現在的結構是清楚的（`Scoped` 共用、`ScriptFront` 有編譯期的界線），放著不會壞。代價是 root 的成本和三套寫法留著。
- **做成一份大的 RFC**：範圍太大，中間任何一段的數字都可能改變後面的設計（例如 M1 選了乙，M2 的一項就不用做）。分成里程碑、各自一份 RFC，每份開始時才定那一段的設計。

## 先例

- **Blink 產生的 callback**：只收 `v8::FunctionCallbackInfo`。參數用 `NativeValueTraits` 轉換，直接呼叫 V8 的 `ToNumber`、`Object::Get`，失敗時（`ExceptionState` 記著）直接返回。結果用 `ToV8Traits` 和 `V8SetReturnValue`。handle 都在 V8 替 callback 開的 handle scope 裡，沒有另一層。
- **Blink 的 wrapper**：`ScriptWrappable::ToV8` 在 callback 裡當場建 wrapper（`V8DOMWrapper::CreateWrapper`）。Oilpan 保守地掃 stack，所以建 wrapper 時配置、GC 都不影響 stack 上的 `Node*`：沒有 root，也沒有「操作」這個概念。
- **Blink 呼叫 JS**：`V8EventListener`、`CallbackInterfaceBase` 用 `v8::TryCatch` 和 `v8::MicrotasksScope` 包著 `Function::Call`；`this` 和參數是 stack 上的裸指標。
- **Oilpan 的 `Persistent`**：只給 heap 之外、要長期拿著物件的地方用，不在每次呼叫裡建。
- **V8 的 `HandleScope`**：一個 stack 式的表，建 handle 是寫一格、離開時整批放掉。R 線的甲是它在 elise 這一側的對應。

（以上除了 `ToWrappableUnsafe` 和字串那兩處在 RFC 0097 查證過，其餘照記憶；每個里程碑的 RFC 開始時對著 Chromium 的原始碼查。）

## 已決定的事項

2026-11-14（使用者）：

1. **目標是讓 binding 的成本更接近直接呼叫 V8**，範圍包含 root、scope 和只有一個 front。
2. **除了效能也追求一致性**。
3. **做成路線圖型的 RFC**。

4. **整體的方向照草稿**：終點的五條、六個里程碑的切法，和「先量、root 排第一、併 front 放最後」的順序。M0 的數字出來之後 M2 到 M4 的先後可以對調。
5. **`PropertyFront` 要不要併進唯一的 front，M3 換 interceptor 的 trampoline 時一起評估**，現在不定。
6. **每個里程碑各開一份 RFC**，不當成這份的階段：M1（動 GC）和 M5（動 34 個會跑 script 的成員）各自要決定的事不少，而且前面的數字會改變後面的設計。不選一份到底：這份會很長，而且要到全部做完才算完成。這份自己只做 M0，在 `progress.md` 記每個里程碑的狀態和它的 RFC 的編號。
7. **M0 先數三樣**：每條路開幾個 scope、登記幾個 root、開幾個操作，和它們在 Speedometer、blink_perf 上各被走到幾次。handle 和 V8 API 的呼叫次數先不數，不夠再加。
8. **Speedometer 在 M1 和 M5 做完時各量一次**（照規定平常不量）。

9. **待決定的三條等 M0 的數字出來再定**（使用者：現在難以決定；overhead 希望越少越好，但有些看起來留著可能比較合適）。所以終點的五條是方向，不是每一條都一定做到底：M0 量出來某一種中間層在實際的頁面上佔得很少、拿掉它的代價又高時，可以決定留著，寫明留在哪裡、為什麼。
10. **里程碑的順序維持草稿的**（M1 的 root 在 M2 之前）。

## 待決定

M0 的數字出來之後和使用者逐條確認：

1. **R 線選哪幾種機制、先後**（甲：便宜的 root；乙：V8 在操作裡配置；丙：拿 wrapper）。
2. **fork 的入口要不要寫成可以送上游的樣子**。rusty_v8 的 fork 現在不照 parley、taffy、vello 那套「每個 commit 可以直接送上游」的規則；入口多了之後，rebase 的成本會變高。
3. **終點的第 1 條的例外清單**：哪些可以留著 scope（目前的看法：每個 interface 只跑一次的初始化、測試和 WebDriver 用的函式、module 的載入）。

## 之後的可能

- 最熱的 getter 剩下的差（RFC 0097 的 `measurements.md` 的逐條對照）：冷的路搬出 callback、型別的 tag 變成常數、開 `Weak` 時不比 isolate。
- 字串參數每次用文字查固定名稱表（要把編號記在 V8 的字串上，等 V8 的 sandbox）。
- wasm 的 front 的同一件事：它的 root 和呼叫的成本沒有量過。
