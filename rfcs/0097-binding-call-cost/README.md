# RFC 0097：binding 每次呼叫的成本往 Chrome 靠

- 開始日期：2026-11-11
- 狀態：🚧 已排程：2026-11-11
- 細節：`details.md`

## 摘要

JS 呼叫一個 DOM 成員，machopo 在 V8 的地板之上自己的部分約是 Chrome 的三倍（最簡單的 getter 約 19 對 6 個 cycle），有字串參數的差得更多。這份先用原型一項一項量，再決定做哪幾項：讓 V8 能在 DOM 的操作裡配置（一次呼叫從三段變成兩段，照 TLA+ 模型驗證過的三條規則）、callback 的外殼不建 scope、`Call` 只帶成員用得到的欄位、字串參數和結果不重複轉換、事件派發回 JS 的路。

## 動機

**現況**（2026-11-11，Linux、AMD Ryzen AI 9 HX 370；量法和完整的數字在 `details.md` 的「2026-11-11 的量測」）：

- Speedometer 取樣的 bindings 那一類 machopo 33 ms、Chrome 6 ms（每個 iteration，20 個 suite 合計；`docs/performance-gaps.md`）；字串參數的成本有一部分算在 DOM（名稱和值的查找約 9 ms）。
- 每次呼叫（blink-perf 換算，ns，machopo / Chrome）：`nodeType` 6.4 / 3.5，`firstChild` 7.4 / 4.2，`id` 14.0 / 8.3，`getAttribute` 31.4 / 21.1，`id = …` 44.2 / 27.9，`setAttribute` 60.1 / 42.8，`getElementById` 38.2 / 17.4，`createElement` 79.4 / 55.4。`bench.py calls`：`nodeType` 153 條指令、30 cycle，其中 V8 呼叫 API callback 的地板約 50 條、11 cycle；`dispatchEvent` 3497 條、727 cycle。
- 一次呼叫現在分三段（`dom_js` 的 `front.rs`）：操作之前轉參數、預先建新物件的 wrapper；操作裡 unwrap、呼叫 DOM，V8 不能配置；操作之後把「留著的」結果（新的字串、沒有 wrapper 的節點、放不進 Smi 的數字）變成 V8 的。跨段的東西放在 `Call`（7 個欄位，每次呼叫都清一遍、結束時檢查要不要釋放）。
- V8 不能在操作裡配置，是因為它一配置就可能 scavenge、開始或收尾 major GC，而 elise 不掃 stack：操作裡只在 stack 上的物件會被漏標。Blink 沒有這個限制（Oilpan 保守地掃 stack），生成的 callback 只有 receiver、參數的轉換和 `ReturnValue`。
- 以前做過的一輪（RFC 0067 每個成員一條有型別的路、RFC 0085 第 4 段的 getter 的輕的路）之後剩下的：建 `V8Front` 和 `Call`、rusty_v8 每次 callback 建的 `CallbackScope`、unwrap 的型別比對（RFC 0085 決定 14 延後的那一項）、字串參數每次重新查表。

**目標**：

- 知道每一項各佔每次呼叫的多少（原型，`bench.py calls` 的指令數和 cycle），照數字決定做哪幾項。
- 做的那幾項之後，常見的成員（getter、帶字串參數的、建節點的、派發事件的）每次呼叫的成本接近 Chrome 的。目標的數字在第 1 段之後定。

**要解決的 `docs/limitations.md` 的限制**：無（`docs/performance-gaps.md` 的「可以開 RFC 的方向」的 DOM 和 bindings 那一條裡，binding 每次呼叫的固定成本）。

**不在範圍內**：

- 操作可以巢狀（DOM 做到一半呼叫 script 而不結束操作）：要把 DOM 的狀態改成共享借用下可變，三條規則也要重新設計。事件派發那條路先量，值得的話另寫。
- 保守地掃 stack：見「理由和其他做法」。
- DOM 本身的資料怎麼放（屬性的 `HeapVector`、名稱表）：`docs/performance-gaps.md` 的 DOM 那一條。這份只處理字串怎麼在 V8 和 DOM 之間過去。
- wasm 的 front（`dom_wasm`）：成員的實作共用，但它的呼叫路徑不在這份量。

## 設計概要

對網頁沒有看得到的變化。對改 binding 的人：

- 成員照舊寫成對一個 front 做事（`dom_api` 的 `Front`、`OpFront`），JS 和 wasm 共用。
- 一次呼叫從三段變成兩段：**操作之外**只做可能執行 script 的參數轉換（物件當字串或數字用、字典、可迭代的），參數本來就是字串、數字、節點、函式時這一段是空的；**操作裡**做其他全部，包括需要 V8 配置的結果（新的 wrapper、字串、陣列）當場建。「操作之後」那一段沒有了。
- 會執行 script 的 V8 呼叫照舊不能在操作裡（型別照舊擋住：操作裡拿到的 V8 的能力不含執行 script）。

## 詳細設計

各項是候選，第 1 段量過才決定（待決定 2）。

### V8 在操作裡配置

現在 marking 只在安全點（兩個操作之間）開始和收尾，那時 stack 上沒有裸指標。放寬成 V8 可以在操作裡配置之後，V8 的 scavenge、major GC 的開始和收尾都可能發生在操作裡。照 elise 的 `tla/unified` 模型加上這些（`tla/Unified.tla`、`tla/check.py`；結果在 `details.md` 的「模型」），要三條規則：

1. **在操作裡被要求開始 marking 時，elise 的快照延到操作結束（下一個安全點）**。延後期間 barrier 和 black allocation 還沒開，V8 交過來的物件只排隊、不掃描（掃了之後被寫入的邊沒有 barrier）。worker 的 concurrent marking 照舊，只是晚一個操作開始。
2. **V8 的收尾停頓發生在操作裡時，這一輪把所有 old 的物件標成活的**（標記、掃描，JS 的邊照樣交給 V8），不釋放任何 DOM 物件。只是不 sweep 不夠：沒標到的物件指向的 wrapper 會被 V8 收掉。
3. **在操作裡建 wrapper 時，V8 正在 marking 的話把那個節點標記起來**。marking 期間新建的 wrapper 是黑的，V8 不再掃它、不會把節點交過來。rusty_v8 的 fork 現在刻意跳過這個 barrier（`heap-write-barrier.cc` 的 `MarkingSlowFromCppHeapWrappable`：「wrap 的時候一定已經標過」，在現在的規則下成立）。

V8 的 scavenge 在操作裡是安全的（協定上）：它只丟沒人用的 wrapper、重設節點指向它的參照，不動 DOM 物件。

規則做在 elise（`Heap` 知道有沒有「允許 engine 配置的操作」在進行）和 `bindings`（`unified_heap` 的 wrap）；先改 elise 的模型，再改程式。`crates/bindings/soundness.md` 要重寫的：scavenge 在操作裡重設節點的 JS 邊（現在的論證是「scavenge 只在兩個操作之間」）、`NoGc` 這個名字和它給的能力（之後是「不執行 script」）。

### 外殼不建 scope

rusty_v8 把每個 callback 包成建一個 `CallbackScope`、再給參數和回傳值的物件（`function.rs` 的 `MapFnFrom`）。成員的 callback 改成直接拿 V8 的 `FunctionCallbackInfo`，只有要配置或丟例外時才建 scope。fork 加一個不建 scope 的入口（只改 Rust），`bindings` 多一段 `unsafe` 和論證。

### `Call` 只帶用得到的欄位

三段併成兩段之後，「留著的結果」和「預先建的 wrapper」不需要了；剩下的（callback 參數、rest、少數成員用的那一包）照成員的宣告在編譯時決定有沒有。getter 已經有的輕的路推廣到參數單純的 setter 和方法。

### 字串

- 進來：純 ASCII 的 V8 字串現在不複製（借用字元），但每次都要拿字元去查名稱表或值的表。候選：讓 V8 的字串記住它對應的 DOM 字串或名稱（V8 的 external string，或 internalized 字串的位址對名稱的快取）。要 fork 支援。
- 出去：快取裡沒有的字串現在複製一份給 V8、加 weak handle、放進快取。候選：external string 直接指向 DOM 的字串。

### 事件派發回 JS

`dispatchEvent` 3497 條指令。先拆開量：結束和重開操作、登記 root、`call_listener`、建事件的 wrapper 各佔多少，再看有沒有不動「操作不能巢狀」就能省的。

### unwrap 不比型別

signature 檢查過的 receiver 不再比型別的 tag（RFC 0085 決定 14 量過約 1 到 2 個 cycle）。排最後，和外殼那一項一起看。

### 測試

- 第 1 段的原型不合併，只有量測。
- 實作的每一項：行為由現有的 `dom_js`、`bindings` 的測試和 WPT 守著；優化本身用計數守（例如「這個成員的呼叫沒有建 scope」「沒有留著的結果」），照 `docs/test-gaps.md` 的「沒有計數守著的優化」的做法。
- V8 在操作裡配置：elise 的模型先改（三條規則各拿掉一條的反例）；elise 的整合測試（操作裡開始、操作裡收尾、操作裡 wrap）；`bindings` 的 `unified_heap` 測試；結果看 GC 什麼時候標的測試照 `docs/testing.md` 重複跑。merge 之後手動開始 CI 的 `gc.yml`。
- mutation test 照 `docs/testing.md`。

### 分階段

| 段 | 內容 |
| --- | --- |
| 1 | 原型和量測（不合併）：每一項單獨做最小的原型，`bench.py calls` 量指令數和 cycle；V8 在操作裡配置的原型另外量規則 2 多久發生一次、停頓多長 |
| 2 起 | 照第 1 段的數字決定做哪幾項、順序和怎麼分段（待決定 2） |

## 缺點

- V8 在操作裡配置動到 elise 的 marking 的開始和收尾，和 `bindings` 的 `unsafe` 論證：這是 GC 最容易出錯的地方。
- 規則 2 發生時那一輪什麼都不收，還要走過整個 old（毫秒級的停頓）；多久發生一次還沒量。
- 外殼和字串要改 rusty_v8 的 fork；字串那一項可能要動 C++（要重編 V8）。
- 之後每個新的成員都走兩段式：改 `dom_api` 的 `Front` 這一層的人要知道哪些轉換可能執行 script。

## 理由和其他做法

- **只在 V8 的 GC 落在操作裡時保守地掃 stack**（規則 1、2 的另一種做法；全面改成保守式不考慮：有 nursery 的 heap 的物件會搬）：不會有「這一輪什麼都不收」，掃的次數很少；要把 32 位元的值也當成可能的壓縮指標。精準這個選擇本身在 Speedometer 上估淨虧 3 到 6 ms（回收那一側 DOM heap 省約 2 ms，多半來自 worker 而不是不掃 stack；binding 這一側「留著的結果」和 `Call` 的欄位多付約 6 到 8 ms），換來的是物件能搬（nursery、之後 wasm GC 的物件）和用型別證明的 API。
- **保守地掃 stack**（原本的說明）：JS 文件的 heap 不搬物件，在 V8 的收尾停頓裡保守地掃 stack 就能取代規則 2，Oilpan 就是這樣。代價是要在 Rust 裡掃 stack（暫存器、每個平台），elise 對外的 API 的安全性不再能用型別證明。
- **維持三段**（不做「V8 在操作裡配置」）：其他幾項照樣可以做；`Call` 的欄位少不了那麼多，回傳沒有 wrapper 的節點照舊要 root 和第二個操作。第 1 段的數字會說它值不值得。
- **只做 unwrap 不比型別**：1 到 2 個 cycle，差距的一小塊。
- 完整的比較在第 1 段之後補進 `details.md`。

## 先例

- Blink 生成的 callback（`nodeType` 的 getter）：receiver 用 `ToWrappableUnsafe` 轉成 `Node*`、呼叫實作、`V8SetReturnValue`；不建 scope，沒有共用的呼叫結構，會丟例外或有字串參數的才在 stack 上多放東西；參數照 WebIDL 的順序先轉完才呼叫實作。（生成的 callback 在 repo 裡沒有，照記憶；unwrap 的部分 2026-11-11 查證過：`ToScriptWrappable` 照樣比 tag 的範圍，`details.md` 的「Blink 的做法」。）
- Blink 的字串：V8 的字串第一次轉成 Blink 的字串時外部化，之後不再複製；`StringCache` 記 Blink 的字串對 V8 字串。（2026-11-11 查證過，`details.md` 的「Blink 的做法」：進來時非 external 的字串複製一次、當場改成 external，resource 存著 `AtomicString`；出去時建指向 `StringImpl` 的 external 字串。）
- V8 的 `WriteBarrier::ForCppHeapPointer`：wrap 的時候 marking 在進行就標記 cppgc 的物件；規則 3 是它在 elise 這一側的對應。
- elise 的 `tla/unified`：現在的協定的模型，這份的模型從它改。

## 已決定的事項

2026-11-11（使用者看過草稿，說開始）：

1. **寫成一份 RFC，第 1 段的原型一起做**，看完數字再決定要不要拆（「V8 在操作裡配置」動 elise 和安全論證，「字串」動 fork，性質不同；但各項互相牽動，例如三段併成兩段之後 `Call` 要帶的欄位跟著變）。

第 1 段開始前（2026-11-11，使用者照建議）：

2. **原型的順序**：外殼不建 scope 加上 `Call` 只帶用得到的欄位（`nodeType`、`firstChild`）；V8 在操作裡配置（`createElement`、回傳沒有 wrapper 的節點，記規則 2 的次數和時間）；字串（先查證 Blink，再做 external string 和位址對名稱的快取兩個原型）；事件派發只拆開量。
3. **原型放在 `0097` 分支上**，每個一個 commit、標明是原型，量完把數字寫進 `progress.md`，這一段結束時拿掉；elise 的原型只在本機的分支上，不推、不動 `third_party.toml`。
4. **Blink 的做法上網查 Chromium 的原始碼**（只讀）。
5. **數字用 `bench.py calls`**（指令數和 cycle），對應的 blink-perf 頁和 Chrome 比；這一段不量 Speedometer。

第 1 段做到一半（2026-11-11，使用者同意）：

6. **「V8 在操作裡配置」的原型先不做**：量了現在的路（`progress.md`），它只省「留著的結果」那條（回傳沒有 wrapper 的節點約 23%、沒快取到的字串），Speedometer 上的上限約 5 到 6 ms；`createElement` 不受惠（已經在操作前預先建 wrapper）。它是各項裡工作量和風險最大的（elise 的 marking 的開始和收尾、`bindings` 的論證）。不做的話一次呼叫照舊三段，但每次呼叫都付的固定成本由「`Call` 只帶用得到的欄位」拿掉（回傳數字、名稱、一定有 wrapper 的物件的成員編出來只有操作那一段）。模型和三條規則留著，之後要做（為了那 5 到 6 ms、拿掉「留著的結果」那一套、或事件派發）不用重來。接著做字串，再拆開量事件派發。

## 待決定

2. 第 1 段之後：做哪幾項、順序、目標的數字。
3. 「V8 在操作裡配置」：規則 2 用「整個 old 當成活的」還是保守地掃 stack（第 1 段量規則 2 的頻率和停頓之後）。
4. 字串：用 V8 的 external string，還是位址對名稱的快取（第 1 段各做一個原型）。

## 之後的可能

- 操作可以巢狀：DOM 做到一半呼叫 script 不用結束操作。
- wasm 的 front 照同樣的方式量。
