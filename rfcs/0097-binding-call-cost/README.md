# RFC 0097：binding 每次呼叫的成本往 Chrome 靠

- 開始日期：2026-11-11
- 狀態：🚧 已排程：2026-11-11
- 細節：`details.md`

## 摘要

JS 呼叫一個 DOM 成員，machopo 在 V8 的地板之上自己的部分約是 Chrome 的三倍（最簡單的 getter 約 19 對 6 個 cycle），有字串參數的差得更多。這份先用原型一項一項量，再決定做哪幾項。量完做兩項：字串參數已經是字串就不轉換；front 重新設計成只拿 V8 給 callback 的 info，階段之間傳的東西是成員自己的區域變數，不再有每次呼叫都建、所有成員共用的結構（Blink 產生的 callback 的樣子）。量過不做的：V8 在 DOM 的操作裡配置（三條規則用 TLA+ 模型驗證過，留著）、最近查過的字串的快取、external string、事件派發。

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

### front 只拿 callback 的 info

V8 給 callback 的只有一個指標（`FunctionCallbackInfo`）：isolate、`this`、參數、回傳值的位置都從它算。Blink 產生的 callback 只收它，其餘是那個函式的區域變數，用到的才產生（`details.md` 的「Blink 產生的 callback 和 machopo 產生的差在哪」）。machopo 每次呼叫建 rusty_v8 的 scope、參數的包裝、`V8Front` 和一個所有成員共用的 `Call`（7 個欄位），冷的路拿著它們的位址，編譯器每次都得寫進記憶體。

- **第 3 段**：新的 front 只有 info 和一格「操作後才做完的結果」。isolate、receiver、參數、回傳值當場從 info 讀；unwrap、回傳數字、已有 wrapper 的節點、快取裡的字串、原地讀字串參數只用 isolate；要配置、丟例外、執行 script 的路當場建一個 scope、用完就丟，交給原本的 `V8Front`，在 inline 之外。不改 `Front`、`OpFront` 和產生器：跑的是同一份成員的程式。編譯時照成員的宣告決定走哪一個 front：參數是字串、節點、數字、布林，結果是數字、布林、字串、節點、`undefined` 的走新的。
- **第 4 段**：階段之間傳的東西改成有型別的回傳值（`RetType::prepare`、`give` 回傳它準備的和還沒做完的，`finish` 拿到它；callback、rest 這類參數直接回傳 handle，像字串和節點），產生器把它們接成成員的區域變數。其餘的成員搬到新的 front，原本的 `V8Front` 和 `Call` 拿掉。沒有這一步，新的 front 要涵蓋全部成員就得把共用的欄位加回來。

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
| 2 | 字串參數已經是字串就不轉換（決定 7、8） |
| 3 | front 只拿 callback 的 info：參數和結果單純的成員（決定 17、18） |
| 4 | 階段之間傳有型別的值，其餘的成員搬到新的 front，拿掉 `V8Front` 和 `Call`（決定 17、22）；4a 到 4g |

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

第 1 段量完（2026-11-11，使用者照建議）：

7. **做兩項：外殼不建 scope 加上 `Call` 只帶用得到的欄位（`nodeType` 省 37 條指令、7 個 cycle，每次呼叫都省），和字串參數已經是字串就不轉換（每個字串參數省約 57 條指令，Blink 也這樣做）**。其他都不做：
   - 最近查過的名稱和值的小快取（每個名稱約 40 條、每個值約 95 條）：Blink 沒有這一層，是「短字串改不成 external」的替代品，多一個要維護的東西，之後能做 external 時還要拆；只在一兩個名稱的 benchmark 上量過。
   - 進來的字串改成 external（Blink 的做法；長的名稱省約三成）：現在的 V8（有指標壓縮、沒有 sandbox）只改得了 8 個字元以上的字串，真的頁面上佔多少沒量過；要在 fork 接 `MakeExternal`（改 C++、重編 V8）、處理 resource 的生命。最常用的 `'id'` 連 Chrome 也改不了、每次查表；Chrome 在改得了的名稱上省約 3 到 5 ns（20 ns 裡）。
   - 出去的字串不複製（external 指著 DOM 的文字）：到 1000 個字元都比複製慢，在 JS 裡讀也變慢；V8 在 GC 的 worker 上放緩衝區，DOM 的字串的計數不是 atomic 的。
   - 為了字串開 V8 的 sandbox：效果就是上面 Chrome 的那 3 到 5 ns；要不要開看安全上的理由，另一份 RFC。
   - 只留 cage 的檢查以外的 unwrap（約 1 個 cycle；比 tag 那一步 Blink 也付）、事件派發（最大的一塊是 DOM 的演算法，不是 binding）。
8. **順序：先字串參數不轉換（第 2 段），再外殼和 `Call`（第 3 段）**：前者小，先走完一次實作、測試和 mutation test；後者動到每個成員的外殼，分段做，怎麼分在開始前決定。
9. **第 1 段的原型從 `0097` 上拿掉**（決定 3）：兩個原型的 commit 的程式存成 `prototypes/raw-getter.patch`、`prototypes/string-already-a-string.patch`，分支重新整理成只有文件的 commit（還沒 push 過）。

第 2 段開始前（2026-11-11，使用者照建議）：

10. **「是不是字串」的檢查放在 rusty_v8 的 fork**（`v8::machopo::as_string`，和 `unwrap_inline`、`one_byte_chars` 一起）：照 V8 的佈局讀 map 的 instance type，不呼叫 V8（V8 的 `Value::IsString` 在標頭檔裡就是這樣 inline 的，Blink 直接用）。照佈局讀的程式和常數集中在 fork，V8 升級時一起檢查；只改 Rust，不重編 V8。不選放在 `bindings`（佈局的常數散在兩個 repo）；不選 rusty_v8 現成的 `is_string()`（一次 C++ 呼叫）。
11. **只改 `arg_string`**（成員的字串參數，6 個呼叫點都受惠）；其他直接呼叫 `to_string` 的地方是冷的路，不動。
12. **用計數守**：isolate 的狀態記成員的字串參數經過 `ToString` 轉換的次數，測試確認本來就是字串的是 0、其他的各 1 次。

第 3 段開始前（2026-11-11，使用者決定）：

13. **scope 只在用到的時候建，熱的路改成只收 isolate**：scope 是 rusty_v8 加的（執行時是 stack 上一個放 isolate、它自己的 annex 和 context 的結構，約 17 到 20 條指令、3 到 4 個 cycle；V8 不需要它，callback 的 handle scope 是 V8 自己開的）。外殼直接拿 `FunctionCallbackInfo`；unwrap、回傳數字、已有 wrapper 的節點、快取裡的字串和名稱、原地讀字串參數這些路改成只收 isolate，這些呼叫沒有 scope；要配置、丟例外、執行 script 的路第一次要 scope 時才建。不選每個 isolate 共用一個 scope（callback 會巢狀，兩層同時拿著同一個可變參照）。所有路都改成只收 isolate（fork 要一整套不收 scope 的 API、`dom_js` 約 150 處）做完這一段、量過再決定。
14. **第 3 段分成 3a（外殼和 scope）和 3b（`Call` 的欄位照成員的宣告）**，3a 量完再看 3b 還剩多少。
15. **fork 加自己的入口**（從 `FunctionCallbackInfo` 拿 isolate、只收 isolate 的回傳和 handle），不修上游的 `FunctionCallbackArguments::get_isolate`（一呼叫就 segfault；本地寫草稿，不對上游回報）。只改 Rust。

3a 的第一版量完（2026-11-11，使用者同意重新設計 front，細節由實作決定）：

16. **決定 13、14 作廢**：scope 延後建做了、量了（`progress.md`），沒參數的 getter 只省 2 到 4 條指令，有參數的成員反而慢。scope 在原本的程式裡只值約 8 條（先前量的 17 到 20 條高估了），延後建自己的記帳抵掉大半；原型 1 省的 37 條是整個不經過 `V8Front` 和 `Call`。「`Call` 的欄位照形狀分」不做：在旁路上補洞，front 的方法要照形狀多編好幾份。
17. **front 重新設計成只拿 callback 的 info，分兩段**（「front 只拿 callback 的 info」）：第 3 段換 front 的形狀、不改 trait，先做參數和結果單純的成員；第 4 段改階段之間的簽名，搬完其餘的成員，拿掉 `V8Front` 和 `Call`。原型（`prototypes/info-only-front.patch`，跑產生器產生的同一份成員的程式）：`nodeType`、`firstChild` 省 35 到 38 條指令、6 到 8 個 cycle，`hasAttribute`、`getAttribute`、`setAttribute`、`appendChild` 省 66 到 101 條、12 到 33 個 cycle；blink-perf 的四頁對 Chrome 從慢 1.41 到 1.85 倍變成 1.32 到 1.36 倍。先換形狀再改簽名：每一步都有數字可以對。不選先改簽名（一次到位，中間沒有可以量的點）。
18. **第 3 段的做法**：冷的路交給原本的 `V8Front`（不重寫；第 4 段拿掉它時才搬）；哪些成員走新的 front 在編譯時照宣告的型別決定；用計數守住「走新 front 的成員一般不建 scope」；fork 的入口照決定 15。
19. **第 3 段做完再問要不要量 Speedometer、要不要 merge**；階段做完給使用者審，審過才 commit。

第 3 段做完（2026-11-11，使用者決定）：

20. **目標是 binding 的成本往 Chrome 靠，不是 Speedometer 的分數**：第 2、3 段在 Speedometer 上是 −0.9%（`progress.md`），使用者知道、不在意；要的是每次呼叫付的和 Chrome 差不多。第 4 段照做。第 4 段拿不掉的（和 Chrome 的設計不同的地方）：進出操作和借 isolate 的狀態；結果要 V8 配置時分兩步、例外出了操作才丟（Chrome 的 GC 掃 stack，當場做；要拿掉是決定 6 的「V8 在操作裡配置」）；unwrap 多的位址範圍的檢查（約 1 個 cycle）；讀 wrapper 時的 marking 檢查；短字串每次查名稱表（要 V8 的 sandbox）。最簡單的 getter 現在 machopo 自己的部分約 13 個 cycle、Chrome 約 6（開始時 19 對 6）。第 4 段做完之後把 `nodeType`、`getAttribute` 的 callback 和 Chrome 的逐條指令對照，列出剩下每一項差多少、要什麼才拿得掉。
21. **不單獨做「在新的 front 多覆寫幾個方法」那一批**（數字、布林的參數，不是節點的物件）：併進第 4 段。它擴充的 `PLAIN` 標記第 4 段做完就拿掉，標錯只能靠審查，現在標的只有字串和節點。
22. **第 4 段分小段，常被呼叫的先做**（每小段各自測、量、commit，中途兩個 front 並存）：4a 結果怎麼帶出操作（決定 23）；4b 數字、布林、有預設值的參數、不是節點的物件；4c 預先建 wrapper 的結果（`createElement`、`cloneNode`）；4d callback、rest、`any` 的參數直接回傳 handle；4e dictionary、promise、陣列和其餘的；4f static、constructor、named 和 indexed property 的外殼；4g 拿掉 `V8Front`、`Call` 和 `PLAIN`，wasm 的 front 跟上。每小段開始前細分。
23. **4a：操作裡給不出去的結果，由回傳型別帶出操作，型別由每個 front 定**（使用者選的）：`FrontTypes` 多 `PendingString`、`PendingNode`、`PendingNumber`…（之後每搬一類加一個）；`OpFront` 的 `ret_string`、`ret_node`、`ret_u32`…回傳 `Option<Self::Pending…>`（沒給出去的部分）；`Front` 多 `finish_string`、`finish_node`…；`RetType` 多 `type Pending<F: FrontTypes>`，`give` 回傳它、`finish` 拿到它；產生器把它接成成員的區域變數。和參數那一側（`ArgType::Taken<T: FrontTypes>`）同一個寫法，`Front::Op` 的關聯型別多幾個相等的限制。不給東西的 front 填不可能有值的型別（`Infallible`），那一段整個編掉。不選型別由 `dom_api` 定死（例如字串一定是 `DomString`：別的引擎也得帶著）；不選維持現狀（結果留在 front 的格子）。

第 4b 段（2026-11-11，照先前的決定，使用者說細節由實作決定）：

24. **4b 的做法**：小整數的參數從 handle 直接讀，放在 fork（`v8::machopo::as_smi`，照決定 10：照 V8 的佈局讀的程式集中在 fork）；不是小整數的數字用 rusty_v8 現成、不要 scope 的 `Number::value`（heap number 的 instance type 不在 V8 公開的常數裡，不自己讀）；不是數字的才交給原本的 front（`ToNumber` 可能執行 script）。布林本身用 `is_true`、`is_false`，其他的值交給原本的 front。回傳不是節點的物件多一個 `PendingObject`（決定 23 的寫法）；回傳「元素或集合」的兩個型別改成都當物件給。`PLAIN` 照決定 21 逐個型別加（這次加的照「`take`、`param`、`give` 只呼叫新的 front 有的方法」掃過一遍），4g 拿掉。

第 4c 段（2026-11-11，照決定 23 的方向）：

25. **4c：操作前準備的東西也是成員的區域變數，由回傳型別帶進操作**：`RetType` 多 `type Prepared<F: FrontTypes>`，`prepare` 回傳它，產生器把它交給 `give_prepared`（預設丟掉它、呼叫 `give`）；`FrontTypes::PreparedNode` 是 front 預先建的 wrapper（V8 的：wrapper 和它的 interface，別的 realm 的物件上呼叫時沒有；不預先建的 front 填 `()`），`Front::prepare_new_node` 回傳它、`OpFront::ret_new_node` 收下它。`createElement` 的 interface 是照參數猜的：參數型別說它猜到什麼（`ArgType::foreseen_interface`，預設沒有；`ElementLocalName` 在 `take` 時只看名稱、不建東西，`Front::foresee_element`），產生器把各參數猜到的交給 `RetType::prepare`。不選在 front 留一格放 wrapper（又是每個呼叫都帶的欄位，或要照成員分 front 的形狀）。custom element 的建構（操作後才呼叫 constructor）當成「之後才做的節點」的一種（`ret_construct` 回傳 `PendingNode`，V8 的 `PendingNode` 是 enum）。
26. **`cloneNode` 改成預先建**：回傳型別 `NewNodeLikeThis`（新的節點和 `this` 同一個 interface）：操作前開一個讀的操作看 `this` 是哪個 interface、建好 wrapper，操作裡直接接上；猜錯的（interface 不同）照舊操作後再建。

第 4d 段開始前（2026-11-12，使用者確認）：

27. **操作前拿到、操作裡交給 DOM 函式的值是 front 的 handle，不是編號**：`FrontTypes` 多 `Callback`、`Any`（之後每搬一類加一個：rest 的陣列、dictionary 讀出來的物件…），`ArgType::Param` 多一個 front 的型別參數（`Param<'a, 'gc, T: FrontTypes>`），收 callback 和 `any` 的 DOM 函式拿 `&O::Callback`、`&O::Any`，原樣交回 `store_callback`、`same_callback`、`store_any`。`CallbackArg`、`AnyArg` 這兩個編號拿掉。和 `Taken`、`Pending`、`Prepared` 同一個寫法；DOM 那一側造不出 handle。不選「編號等於參數的位置」（callback 就是某一個參數，操作裡從 callback 的 info 再讀一次；不改簽名、每次用到多 4 到 5 條指令）：只適用於本身是參數的值，rest 的陣列和 dictionary 讀出來的物件要另一套。使用者：型別變複雜可以接受。
28. **決定 27 對後面幾段夠不夠**（照原本的 front 存的每一樣東西對過：`Call` 的 7 個欄位和 `RareCall` 的 11 個）：rest、dictionary 的物件、`Uint8ClampedArray` 和 view、custom element 的四個 callback、observer 的 root、range 的容器、`getHTML` 的 shadow root 都是 handle；預先建的 promise 是 `Prepared`；陣列、dictionary、字串清單的結果是 `Pending`。三樣不是 handle、也不用 front 存：目前的 window（每個成員要哪一種在編譯時定，操作裡從 callback 的 info 當場算）；要重丟的值（錯誤帶著存它的物件的 root，出了操作再讀）；`ImageData` 的像素（當成結果的一部分，`Pending`）。「script 已經丟過例外」問 V8。
29. **函式拿操作那一側的 front 的成員照型別算 `PLAIN`**（表上 485 個）：不收編號或 handle、卻要 front 留東西的方法（`rethrow`、`with_array_bytes`、`set_image_data_pixels`、`new_target_number`、`is_new_target`、`current_window`）各自改成照型別擋得住或不用留（決定 28），用到它們的成員在那之前換成不是 `PLAIN` 的型別，加測試釘住；然後拿掉「函式拿 front 的不是 `PLAIN`」。4d 做的：`throwIfAborted` 的回傳型別換成 `Rethrown`，建 `ImageData` 的三個成員換成 `NewImageData`（`getImageData` 的 options 這一段變成 `PLAIN` 的 dictionary，沒換時被 `dom_js` 的 canvas 測試抓到）；`with_array_bytes` 的 `putImageData` 是 overload，`new_target_number`、`is_new_target` 只在 constructor，都不走新的 front；`current_window` 沒有 `prepare_window` 的成員本來就拿不到。4d 只轉 callback：`any` 的值也會從 dictionary 的成員來（`CustomEventInit.detail`），和 dictionary 一起在 4e。不選表上逐個成員標（四百多處、每處靠審查）；不選這些成員等最後一段。
30. **剩下的分段**（取代決定 22 的 4d 以後）：4d handle 的機制、callback 和 `any`、決定 29；4e rest、dictionary 的物件和其餘的參數型別；4f 其餘的回傳型別（promise、陣列、dictionary）；4g static 成員的外殼；4h constructor 的外殼；4i named、indexed property 的外殼；4j 冷的路搬成新的 front 自己的，拿掉 `V8Front`、`Call`、`PLAIN`。wasm 的 front 每一段跟著簽名改（不改編不過），不另外排。
31. **`PLAIN` 的承諾由測試從原始碼檢查，不只靠審查**（2026-11-12）：`dom_api` 把「`PLAIN` 的成員的呼叫可以碰到的方法」寫成兩份清單（`PLAIN_FRONT_METHODS`、`PLAIN_OP_METHODS`），`crates/dom_api/tests/it/plain.rs` 讀 `src/` 的原始碼：標成 `PLAIN` 的型別（和它們共用的函式、表的 macro 產生的型別）在 `front`、`op` 上只呼叫清單裡的方法；`PLAIN` 的成員的函式（連同它在 crate 裡呼叫到的函式，算到不動點）在 `op` 上也只呼叫清單裡的。`dom_js` 的測試檢查清單裡每個方法都是 `InfoFront`、`InfoOp` 自己實作的。加這個檢查時抓到 `putImageData` 是 `PLAIN` 而它的函式呼叫 `with_array_bytes`（決定 29 靠「它是 overload、不走新的 front」放過，外殼換掉時會 panic）：參數型別換成不是 `PLAIN` 的 `ReadImageData`。不選替每個 `PLAIN` 的成員產生一次呼叫（要替每種參數造合法的值，兩千多個成員；而且只走到那次呼叫走的分支）；不選把 `OpFront` 拆成兩個 trait 讓編譯器擋（函式拿的是同一個 `O: OpFront`，拆了之後四百多個函式的簽名要跟著分，4j 兩個 front 併起來時又要合回去）。檢查是照文字掃的（`front.名字(`、`op.名字(`），守不住的寫在 `docs/test-gaps.md`。
32. **4e 的範圍和做法**（2026-11-12）：
    - **dictionary 裡的 handle**（`addEventListener` 的 `signal`、event init 的 `detail`、`state`、`data`、`error`）：dictionary 帶它用到的 handle 型別的參數，成員是 front 的 handle，函式在操作裡交回 front（決定 27 的形狀）。原本說「帶 front 的型別參數」（`AddEventListenerOptions<T>`），做的時候發現不行：front 和它在操作裡的那一側是兩個型別（`F` 和 `F::Op<'o>`），只是 handle 的關聯型別相同，`Name<F>` 和 `Name<F::Op>` 對不上；所以參數是 handle 型別本身（`AddEventListenerOptions<TNode>`，函式寫 `<O::Node>`）。哪個 dictionary 要帶參數由成員的型別決定：表上型別寫了 `T::Node`、`T::Any` 的，產生器照寫法產生參數（`T::Node` 變成 `TNode`），沒寫的照舊；寫錯兩邊都編不過（少寫是找不到 `T`，多寫沒有這種情況：參數只從成員來）。這種成員走另一個 trait（`DictHandle<F>`：要知道 front 才說得出型別），都算 `PLAIN`。不選乙（「取到的」和「操作裡的」兩份 struct：產生器多一倍，`any` 仍然換不了）。Blink 的 dictionary 是產生的 GC 物件（`Member<AbortSignal>`、`ScriptValue`），每次呼叫配置一個。
    - **`ArgType::Param` 的 front 多一個 `+ 'a`**：`&'a Name<T::Node>` 要 `T::Node: 'a`，所以 trait 的 `Param<'a, 'gc, T: FrontTypes + 'a>`、`param<'a, 'gc, O: OpFront + 'a>`，每個實作跟著寫。
    - **rest**：`FrontTypes::Rest`（V8 的是新的陣列），沒有多的參數時是 none、不建 scope。**`any`**：`FrontTypes::Any`，`AnyArg<A>`。`V8Front` 也改成直接給 `Local`，`Call` 少了 rest 的欄位，`any` 不再佔 callback 的那一格。
    - **event handler 的 getter 提前做**（393 個成員）：原本是會呼叫 script 的成員（四個操作、target 和 listener 各 root 一次），改成一個讀的操作，還沒編譯的原始碼由回傳型別帶出操作、操作後編譯（4a 的形狀：`RetType::Pending`、`Front::finish_handler`）。不選讓會呼叫 script 的成員整個走新的 front（要先決定 `ScriptHost` 在新的 front 上怎麼做，是 4j 的事）。
    - **回傳 `any` 的三個型別**（`Any`、`AnyOrNull`、`AnyOrUndefined`，12 個成員）順手做：handler 的 getter 要的 `InfoNoGc::return_value` 有了之後只是標 `PLAIN`。
    - **其餘零散的參數型別留到 4j**（操作前整個讀成 Rust 值的：`RequestInit`、keyframes、structured clone、`roundRect` 的半徑…）：成本在讀 JS 物件本身。`KeptObject`、`KeptWindow`、`KeptTarget`、`SignalMember`（init dictionary 的 `view`、`relatedTarget`、`RequestInit` 的 `signal`）都只在 constructor 或這一類成員，一起留著。
    - 4e 之後能走新的 front 的 2745 個成員（getter、setter、method）裡 2581 個是 `PLAIN`（4d 之後 2152）；剩下的是 overload、static、會呼叫 script 的、4f 的回傳型別和上面留著的。
33. **4f：engine 一定要配置的結果由回傳型別帶出操作，操作後呼叫 front 的專屬方法**（2026-11-12）：陣列（物件、字串、數字，frozen 或不是）、dictionary、record、已經 resolve 的 promise（物件、字串、JSON、bytes、undefined）、pending 的 promise、animation 的 promise、第一次讀才做的值（`history.state`、mark 的 `detail`、`FileReader` 的 buffer）、`structuredClone` 和 `getRandomValues` 的結果。這些結果沒有「操作裡直接回傳」的快路，所以操作裡不經過 front：`RetType::give` 只把資料變成能帶出操作的樣子（純資料照原樣；DOM 的物件 root 住、連 interface），`RetType::finish` 呼叫 `Front::finish_objects`、`finish_strings`…。`OpFront` 少 21 個方法，`Front` 多 21 個；`dom_js` 的 `Kept` 從 30 種剩 9 種（`V8Front` 自己在操作裡留的：數字、字串、節點、物件、custom element 的建構和 upgrade、`ImageData` 的像素，4j 跟著 `V8Front` 走），那個大 `match` 的 19 個分支變成各自的方法。Blink：實作回傳 C++ 的值（`Vector<String>`、`HeapVector<Member<Element>>`、`ScriptPromise`），binding 在實作返回後用 `ToV8Traits` 轉。不選甲（front 多一個「待完成的結果」型別、就是現有的 enum）：改動小，但 enum、裝它的 `Box` 和 `match` 留到最後。成本兩者一樣（操作後建 scope、配置）。
    - **promise 不在操作前準備**：原本 `whenDefined`、`fetch`、`wakeLock.request`、`Blob` 的讀取、`decode` 在操作前建 resolver（`Front::prepare_promise`）、留在 `Call` 裡。但 resolve 和照編號存起來本來就都在操作後做，沒有理由先建：改成操作後才建（`finish_pending_promise`、`finish_resolved_constructor`），`RareCall` 少一個欄位。`whenDefined` 已定義的那一支原本把定義的 constructor 的邊交給 front 在操作裡讀成 `Local`；現在帶出操作的是定義的 root，操作後再開一個讀的操作讀那個邊。
    - **`Rethrowing<R>`**：`Request`、`Response` 讀 body 的十個成員（`text()`、`json()`…）的結果型別變成 `PLAIN` 之後，`plain` 的測試指出它們的函式會 `rethrow`（body 的 signal abort 時留著 reason 到操作後 reject）：包一層不是 `PLAIN` 的 `Rethrowing<R>`（其餘照 `R`）。`FilledView`（`getRandomValues`）不標 `PLAIN`：它要的 view 是舊的 front 留在 `Call` 裡的參數。
    - `ImageData.data`（`Uint8ClampedArray` 的結果，`ret_any`）和 canvas 的 `fillStyle`、`strokeStyle` 的 getter（字串或物件）只是標成 `PLAIN`：它們用的方法新的 front 已經有。
    - 4f 之後能走新的 front 的 2745 個成員裡 2646 個是 `PLAIN`（4e 之後 2581）。剩下的 99 個裡卡在結果型別的只有刻意標的（`Rethrowing`、`Rethrown`、`NewImageData`、`FilledView`，共 16 個）；其餘是 overload、static、`#[realm]`、會呼叫 script 的、留到 4j 的參數型別。
34. **4g：static 和 overload 的外殼；目前的 realm 的 window 從 isolate 讀；範圍的更正**（2026-11-12）：
    - **實際走新的 front 的成員要照分派算，不是照 `PLAIN`**：4f 為止報的「`PLAIN` 的成員數」（2646）包括型別合格、卻由別的外殼處理的（overload 的形式、cross-origin）。照 `templates.rs` 的分派重算，4f 之後 2799 個成員裡走新的 front 的約 2575 個；其餘：會呼叫 script 的 33、留到 4j 的參數型別和刻意標的 38、constructor 50、overload 的形式 35（17 組）、static 18、cross-origin 16、indexed、named 和 iterator 的外殼約 35（其中有名字又 `PLAIN` 的 indexed getter，像 `item`，函式那一份已經走新的 front）。overload 和 cross-origin 原本的分段沒排到：**overload 併進 4g**（和 static 一樣是另一種外殼，`scrollTo`、canvas 的繪圖方法常用），**cross-origin 併進 4j**（callback 先檢查呼叫者能不能存取、在 receiver 的 realm 跑）。4j 因此是 87 個成員；它們搬到新的 front 省外殼的成本，會呼叫 script 的那 33 個到時逐個量、列出哪些值得像 event handler 的 getter 那樣改結構，再決定（`dispatchEvent` 的大頭在派發和回呼 listener，是「事件派發回 JS」那一節的事）。
    - **目前的 realm 的 window**（決定 28 的第一樣）：選甲，fork 加 `Context::current_global`（只有 Rust：`GetCurrentContext()->Global()`，handle 留在 V8 替 callback 開的 handle scope 裡），`InfoOp::current_window` 在操作裡當場讀。Blink 的 callback 就是 `info.GetIsolate()->GetCurrentContext()`。不選乙（操作前建 scope 取出 global、存在 front 裡）：static 的成員多半冷，但 4h 的每個 `new Event()` 都要它（time origin），每次為此建一個 scope。成員要不要 window 仍然由產生的程式在操作前說（`Front::prepare_window`，static 又 `#[front]` 的）：新的 front 只記一個布林，沒說的成員拿到 none，和舊的 front 一樣。`#[realm]`（`this` 的 realm 的 window，`Blob` 的三個讀取）不在這一段。
    - **static 的外殼**：`bindings::StateInfo` 多一個型別參數（`Signed`、`Unsigned`），`receiver()` 只在 `Signed` 上，static 的 trampoline 建 `Unsigned` 的；`dom_js` 的 `InfoFront<'s, K>`、`InfoOp<'o, 's, K>` 照它決定有沒有 receiver。不選讓 static 的也拿 `Receiver`（`this` 沒經過 signature，`Receiver` 的不讀 map 的 unwrap 就不成立）。`TypedMember::PLAIN` 不再排除 static。
    - **overload 的外殼**：一組一個只拿 info 的函式，選形式不建 scope、不配置（組的編號從 data 原地讀；`resolve_overload` 的候選原本是每次呼叫一個 `Vec`，改放 stack 上），形式是 `PLAIN` 的直接走新的 front，不是的當場建 scope 照舊呼叫。Blink 的 overload 是產生的 `switch`（照參數個數、再照型別），形式的函式直接呼叫；machopo 的仍然是照表在執行時選（一個函式指標的呼叫），沒有改成產生的程式。constructor 的 overload 留到 4h。
    - 4g 之後走新的 front 的是 2632 個（2799 個裡）。
35. **新的 front 補齊操作這一側的方法，不再靠標記型別擋**（2026-11-12，插在 4h 之前）：兩個 front 並存期間，成員走哪一個看參數和回傳的型別（`PLAIN`），但成員的函式在操作裡呼叫得到 `OpFront` 的任何方法，型別看不到；新的 front 沒有的方法預設實作是 `unreachable!`，呼叫時才 abort。這發生了四次（4d `getImageData`、`putImageData`，4f `Request`、`Response` 讀 body 的十個，4h `new ImageData()`），每次加一個標記型別把成員擋回舊的 front（`Rethrown`、`NewImageData`、`ReadImageData`、`Rethrowing<R>`），靠決定 31 的原始碼掃描發現，而掃描漏了 constructor。改成讓這種錯不可能發生：新的 front 缺的 12 個方法裡，
    - **不用憑證就能呼叫的 6 個**：`rethrow` 刪掉（函式回傳一種帶著 signal 的 root 的錯誤，front 丟錯時才讀 reason）；`set_image_data_pixels` 刪掉（像素和物件一起由結果帶出操作，constructor 的結果也能帶東西出操作）；`with_array_bytes` 在新的 front 實作（fork 加不用 scope 從邊讀出 handle 的讀法）；`new_target_number`、`is_new_target`、`construct_html`（`HTMLElement` 的 constructor）在新的 front 實作。
    - **讀 front 事先留著的東西的 3 個**（`intersection_node`、`static_range_nodes`、`listed_shadow_roots`，只有參數型別自己呼叫）：刪掉，參數型別帶著節點的 handle 進操作。
    - **要編號才能呼叫的 3 個**（`kept_object`、`kept_target`、`store_array`）這次不動：編號只有舊的 front 的參數型別給得出來，留到 4j 和那些型別一起改成 handle。
    做完之後前 9 個在 trait 裡沒有會 abort 的預設實作（新增這類方法卻沒在新的 front 實作是編不過），標記型別和掃描裡「函式呼叫了什麼」那一半拿掉。不選繼續加標記（`Unplain<A>`）、把掃描擴到所有種類：仍然是事後發現，文字比對有看不到的地方。不選把 trait 拆成兩個（決定 31 已經不選）。Blink 只有一種產生的 callback，沒有「這個成員能不能走某條路」的判斷。這些成員也因此從舊的 front 搬到新的。
    - **4h（constructor 的外殼）沒有新的選擇**，照決定 17、34：只拿 info 的 callback（沒有 signature，`Unsigned`），`this` 是 V8 做的新物件。`HTMLElement` 的 constructor 也走新的 front，但每次建一個 scope 檢查 `new.target` 是不是 `HTMLElement` 本身（保留的成本）。legacy factory（`new Option()`、`new Image()`，2 個）沒動。
    - **一個 dictionary 的成員在一個 scope 裡讀完**（量測 4h 時找到帶 init 的 constructor 變慢）：`Front::reading` 和產生的 `ReadDict`。Blink 讀 dictionary 是在 callback 自己的 handle scope 裡逐個 `Get`，沒有額外的 scope。
36. **4i 分兩步，4j 拆成四段**（2026-11-12）：
    - interceptor（`list[0]`、`dataset.a`）現在全走舊的做法，named 的是另寫的一個小 front（`PropertyFront`）。量到的（指令數（cycle））：`list.item(0)` 257（52）對 `list[0]` 686（145）、超出範圍的 `list[9]` 1202（234）、`children[0]` 761、`classList[0]` 791、`attributes[0]` 785、`children.px` 1847、`attributes.id` 1848、`dataset.a` 1310、`dataset.a = '1'` 1814。Blink 的 interceptor 是產生的 callback、直接拿 `PropertyCallbackInfo`，和一般成員同一個形狀。
    - **4i-1 只做 indexed getter**，**4i-2 做 named 的 getter 和 setter**（做之前再列要決定的事：named property 的規則多，`PropertyFront` 要重做）；很少用的（indexed setter、iterator 的 pairs、named 的刪除、查詢和列舉）留到 4l。不選一次做完（21 個成員、named 的所有規則一起動，出錯時難分辨）。
    - fork 再加一個只有 Rust 的讀法：從 `PropertyCallbackInfo` 不建 scope 取得 isolate。
    - `style.color` 4052 條指令卻 3293 個 cycle，比例和其他成員差很多，不像是外殼的成本：不在這份 RFC 查，記在「之後的可能」。
    - **4j 原本包了五件性質不同的事**（決定 30、34 累積下來的）：拆成 4j 剩下的參數型別、4k 會呼叫 script 的成員、4l cross-origin 和留下的 interceptor、4m 冷的路和刪掉舊的 front。4j 到 4l 彼此獨立，4m 在最後。
37. **4i-2：named 的 getter 和 setter 各自一個 callback，只換照名字的那條路**（2026-11-12）：現在所有 interface 的 named property 共用一個 callback（`NamedProperties`），執行時從 callback 的 data 讀出 interface、查狀態裡的表（`NamedCalls`）、經過函式指標呼叫成員，成員跑在另寫的 `PropertyFront` 上、結果先存成 Rust 的值（`Got`）再做成 V8 的值。
    - **每個成員一個 callback**（`NamedGetter<M>`、`NamedSetter<M>`，template 建立時就知道是哪個成員），和 4i-1 的 `Indexed<M>`、Blink 產生的 `NamedPropertyGetterCallback` 一樣：沒有查表、沒有函式指標、沒有中間的值，成員的呼叫 inline 進 callback。不選保留共用的 callback 加查表（改動小，但每次呼叫多一次查表和一次間接呼叫，named 會是唯一還在執行時找成員的地方）。代價是每個成員一份機器碼。
    - **只換 getter 和 setter、只換照名字的**（8 個 getter、2 個 setter 裡是 `PLAIN` 的）：查詢、定義、刪除、列舉，和用數字當名字的（`dataset[9]`，走 indexed 的 interceptor）照舊、留到 4l，`PropertyFront` 也留到那時。named property 的規則都在 interceptor 的設定和那幾個 callback 裡，這次不動。
    - setter「不收這個名字」（`style.notAProperty = 1` 落回一般屬性）：給結果的當下記下來（`InfoKind` 的 `Noted`，和 4i-1 的「沒有這一項」一樣），不寫 V8 的回傳值那一格。named getter 的空字串是值（`data-a=""`），只有 null 是沒有這個屬性：`note_no_item` 分成 null 和空字串兩個。
    - fork 再加一個只有 Rust 的讀法：interceptor 的 raw callback 拿到的名稱和值是 `SealedLocal`，要不建 scope 換成 `Local`。
38. **只被這次呼叫持有的字串不進字串的快取**（2026-11-12，4i-2 之後獨立的一小段）：`StringCache` 照 `DomString` 的位址記 V8 的字串（`Weak` 加 finalizer，V8 回收時標成 dead）。DOM 留著的字串（屬性值、文字、名稱）下次回傳時位址相同、找得到；每次現算的（`style.color` 的序列化、其他 getter 做出來的新字串）位址每次不同，永遠找不到，卻每次登記一筆：`style.color` 是 `'red'` 時 4709 條指令、2664 個 cycle（4i-2）。做法：做 V8 的字串時，`DomString` 的引用計數是 1（只有這次呼叫要回傳的那一份）就直接做、不登記；DOM 還留著的照舊。不選留著不動。Blink 的 `StringCache` 也照 `StringImpl` 的位址記、也替暫時的字串登記，但它的字串是 external 的（V8 的字串指著 `StringImpl`，不複製），登記的是一個 persistent handle，沒有 machopo 這邊每筆一個 `Box` 的 finalizer。
39. **4j：剩下的參數型別怎麼搬，切成 4j-1 和 4j-2**（2026-11-12）：
    - **操作前整個讀成 Rust 值的型別（約 30 種：keyframes、`RequestInit` 的成員、structured clone、sequence…）照樣在操作前讀**：新的 front 的這些方法是冷的，建一個 scope、呼叫現有的讀取的程式（這一段經過 `with_front`，4m 搬成自己的），型別標成 `PLAIN`。讀 JS 物件會配置、會跑 getter，那個 scope 省不掉；Chromium 也在呼叫實作之前轉成 native 的值（`NativeValueTraits<IDLSequence<…>>`、dictionary 的 `Create`）。不選改成 handle 在操作裡讀（讀物件會跑 script，V8 在操作裡不能配置）；不選留到 4m（4m 會同時搬型別又刪 `V8Front`）。
    - **`KeptObject` 的編號換成現有的 handle**（`FrontTypes::Node`，V8 上是檢查過 interface 的 wrapper）：init dictionary 的 `view`、`relatedTarget`，`RequestInit` 的 `signal`，body 的 `Blob` 和 `URLSearchParams`。操作裡用已有的 `OpFront::object`、`target` 解出 DOM 的物件；`kept_object`、`kept_target`、`KeptKind`、`dict_nullable_object` 和 `V8Front` 那個陣列拿掉。執行時少一次放進陣列和照編號查回來；代價是這些 dictionary 多型別參數（決定 32 的機制）。不選另加一個 `FrontTypes::Object`（V8 上和節點都是 `Local<Object>`、wasm 上都是 `u32`，多一個沒有差別的型別）；不選讓新的 front 也實作編號（每次呼叫要帶一個陣列，三個預設會 panic 的方法拿不掉）。
    - **`new ImageData(array, w)` 的 `Uint8ClampedArray` 讀成值的 handle（`FrontTypes::Any`）加長度**，操作裡用已有的 `store_any` 存進 edge，`store_array` 拿掉。不選替 typed array 加專屬的 handle（只有這一個成員用）。
    - **切法**：4j-1 是編號換成 handle 的這兩項，連同 UI 事件的 constructor 和 `ImageData`；4j-2 是讀成 Rust 值的型別，連同其餘的 constructor、method、setter 和 2 個 static。量測每類量代表的幾個：這些成員的成本主要在讀 JS 物件本身，換外殼省的比例不大，這一段主要是讓 4m 能刪掉 `V8Front`。
40. **dictionary 的成員名稱是名稱表的固定名稱，key 從名稱的 V8 字串的快取拿**（2026-11-12，4j-2 之前獨立的一小段）：4j-1 量到帶 init 的 `new MouseEvent` 三萬多條指令；剖析：V8 的 `Object::Get` 本身約 69%（29 個成員各查一次，沒給的也要走完 prototype 鏈；WebIDL 規定每個成員照順序各 `Get` 一次，V8 的 API 沒有 inline cache 也沒有批次讀取，Chromium 產生的轉換也是每個成員一次 `Get`），每次把成員名稱做成 internalized 字串約 17%。能省的是後者。
    - **選：成員名稱列成 DOM 的名稱表的固定名稱**（`crates/dom/dictionary_members.txt`，排在最後），`dom_api` 傳給 front 的是 `MemberName`（名稱的編號，macro 產生 `const { MemberName::of("…") }`，編譯時由 `dom::names::fixed` 算出、不在清單裡的編不過），`dom_js` 照編號從已有的 `caches.names`（`localName`、`tagName` 用的那一份）拿 V8 的字串，第一次用到時做。執行時是一次陣列索引，不多一套快取。名稱的 V8 字串改成建的時候就 internalized（當 key 時 V8 不用再 internalize）。
    - 不選 binding 自己一張照 `&'static str` 位址查的表（Chromium 的分法：`AtomicString` 表歸 DOM，dictionary 的 key 在 bindings 的 `V8PerIsolateData::FindOrCreateEternalNameCache`）：分層和 Chromium 一致、改動最小，但是第三個字串快取，每次讀多一次 hash。不選 macro 替成員編連續的號碼、binding 自己一個陣列：執行時一樣，但也是多一個快取。
    - 和 Chromium 不同的地方：binding 的名稱（WebIDL 的成員名稱）進了 DOM 的名稱表。代價是一份要維護的清單（漏列編不過；多列由 `dom_api` 的 `members` 測試擋）。
    - 順帶修掉的：local name 清單以外的固定名稱（event type，現在加上成員名稱）拿來當 HTML 元素的名字時 `tagName` 沒有轉大寫（`<click>` 的 `tagName` 是 `click`）：`UPPERCASE` 表對這些名稱給的是它自己。表只留 local name 的那一段，其餘走一般名稱的路。
41. **4k 重置成研究段：一次呼叫多出來的成本一類一類拆開，再設計**（2026-11-12；數字在 `progress.md` 的 4k）：
    - **原本的 4k（會呼叫 script 的 33 個成員逐個加快路）不做了，先研究**：量下來，這些成員貴的不是 scope（一個 scope 27 條指令；事先估 100 到 200 條是錯的），是 root（登記再放掉一個約 110 條）和操作的個數（讀的 20、寫的 44）。同一次量測也發現走新的 front 的成員「要建 scope 的那些路」實際多花幾百到一千多條（長的串接字串當參數 +773 到 +936、非 ASCII 的字串 +516、回傳非整數的數字約 +370、新的 wrapper 約 +1550、現算的字串約 +660），而且其中幾條很常走到。計畫原本假設這些路少走到、照搬就好，這個假設不成立。
    - **研究的範圍**：字串參數（cons string、非 ASCII、要轉換的）；回傳非整數的數字；新的 wrapper，連同沒被 JS 留著的 wrapper 被 V8 收掉又重建；現算的字串結果；會呼叫 script 的成員的 root 和操作個數（`this` 和參數的 wrapper 在 callback 期間本來就讓 DOM 的物件活著，能不能不登記 root 要對照 `crates/bindings/soundness.md` 和 elise 的 soundness）。每一類先分出 V8 一定要做的和我們這條路自己加的，再列選項。沒量過的數字標明是估的。
    - **已經 commit 的不重置**：每一段都是新舊 binary 對照過才進的，這些路的成本和改之前一樣（從 4d 起就是當場交給 `V8Front`），修正能建在現有的 handle 和新的外殼上。
    - **使用者的方向（2026-11-12 再確認）：不建 scope，一次呼叫只呼叫它該呼叫的 V8 API**。scope 是 rusty_v8 的 API 要的憑證（不開 handle scope，本身 27 條），跟著它一起來的整個 `V8Front`（62 條）、`Box<Kept>` 和字串的 heap 配置、root、多開的操作才是大宗；目標是這些路直接憑 callback info 呼叫 V8（配置數字、建字串、建 wrapper、攤平字串、呼叫函式、丟例外），研究要列出每條路需要哪些 V8 API、rusty_v8 哪些要 scope、fork 要補什麼。
    - **4m 拆成三段**：4m-1 會呼叫 script 的成員走新的外殼（做法照研究的結果）；4m-2 刪舊的外殼、`Call`、`Kept`、`PLAIN` 和它的檢查；4m-3 冷的路改成新的 front 自己的，照研究的結果重做、不是把 `V8Front` 的程式原樣搬過來，然後刪掉 `V8Front`。
42. **冷的路只呼叫它該呼叫的 V8 API：憑證換成 token、照和 Chrome 的差距分段、每段對著 Chrome 驗收**（2026-11-12；研究的數字在 `progress.md` 的 4k）：
    - **研究的結論**：同一段 script 在這台機器的 Chromium 和 machopo 量（`prototypes/chrome-compare.js`）。machopo 落後的是冷的路：現算的字串結果 2.46 倍、會呼叫 script 的成員的外殼 2.38 倍、回傳非整數的數字 2.32 倍、新的 wrapper 1.42 倍、非 ASCII 的字串參數 1.32 倍、帶 dictionary 的 constructor 1.26 倍，和最熱的路本身（`nodeType` 1.36 倍）。不是差距的：window 的 getter 當全域變數讀（Chrome 一樣慢）、`dispatchEvent`（machopo 快）、長的串接字串（machopo 快一倍）。這些路上我們多做的是 heap 配置（`Box<Kept>`、`String`、`DomString`）、整個 `V8Front`、root（一個約 80 到 90 條）、多開的操作；scope 本身 27 條。
    - **憑證：fork 加一個從 callback info 借出來的 token，不建 rusty_v8 的 scope**。rusty_v8 的 API 要 scope 的地方，C++ 要的只是 isolate 和 context；callback 裡 V8 已經開好 handle scope。token 不帶執行時的成本（就是 isolate 的指標），借著它時借不到狀態：「拿著狀態時不能叫 V8 配置」照舊由借用檢查保證。fork 的函式拿 token（建字串、`ToString`、複製物件、呼叫函式、丟例外…），每條路用到才加，只改 Rust。不選留著 scope 只拿掉 `V8Front`、`Box`、root（不改 fork、拿得到大部分，但使用者不要 scope）；不選把 rusty_v8 的 scope 本身改成零成本（影響它所有的 API，和上游差太遠）。
    - **`this` 和參數不登記 root**：wrapper 活著就撐著它的 DOM 物件、物件不搬（`crates/bindings/soundness.md` 已有的論證，普通成員已經靠它）；callback 的 `this` 和參數整個呼叫期間都被 V8 留著，跨操作重新解出來是同一條論證。要 root 的只剩沒有 wrapper 的中間物件。
    - **分段，照差距排**：4k-A 回傳非整數的數字；4k-B 回傳現算的字串（token 在這一段加）；4k-C 會呼叫 script 的成員（不登記 root、少開操作）；4k-D 新的 wrapper；4k-E 非 ASCII 和要轉換的字串參數；4k-F dictionary、例外、陣列、promise；4k-G 最熱的路本身，另外研究。A 到 F 做完 `V8Front` 沒有人用，4m-3 剩下刪掉它。不選先做 C（影響最多、最複雜）。
    - **4k-F 只做 `getAttributeNames`（2026-11-13，使用者）**：對 Chrome 量下來，dictionary、例外、promise 和多數回傳陣列的已經比較快（0.37 到 0.83 倍），落後的只有 `getAttributeNames`（1.64 倍：每個名稱各配置一個字串）和不是 binding 的（`matches` 1.73 倍，style 那一側搬一塊大的結構）。所以「A 到 F 做完 `V8Front` 沒有人用」不成立：新的 front 還有約 85 個方法當場建 scope、交給 `V8Front`。它們怎麼搬（程式留著、只拿掉 `V8Front` 的殼，留一個 scope；或全部改成不建 scope，要在 fork 補一大批 API 或改 rusty_v8 的 scope 本身）到 4m-3 再評估。
    - **驗收**：每段的那個呼叫在 `chrome-compare.js` 裡不比 Chrome 慢超過 1.2 倍（兩邊的 V8 版本和 build 不同，再小量不準），不只是比上一段快。
43. **4k-C：會呼叫 script 的成員，沒跑到 script 的一個操作做完，跑到的 `this` 和參數改拿 wrapper**（2026-11-13；數字在 `progress.md` 的 4k-C）：
    - **對 Chrome 量的結果**：真的跑到 script 的呼叫 machopo 已經比較快（`dispatchEvent` 0.57 到 0.87 倍、`click()` 0.46、`form.reset()` 0.71、有 filter 的走訪 0.67、`selection.collapse` 0.45）；落後的是沒跑到 script 卻付了完整代價的：沒有 filter 的走訪 1.38 到 1.62 倍，另外 `inp.select()`、`new AbortController().abort()` 1.29 倍（原因還沒拆）。
    - **「先試的普通函式」**：表上會呼叫 script 的成員可以多宣告一個普通的函式（`#[calls_script(first = …)]`），在一個操作裡跑，回「做完了、結果是這個」或「這次需要 script」；需要時才走原本的路。它要在動任何狀態之前就決定需要 script（原本那條路會從頭再做）。不選把成員拆成兩個、外殼在執行時選（只對走訪有用）。
    - **33 個成員一次搬到新的外殼**（原本排在 4m-1）：新的 front 有一個入口，在裡面用完整的 front 跑原本那段。
    - **跑到 script 的路：`this` 和參數改拿 wrapper，不登記 root（使用者：希望全改）**。函式和它們往下呼叫的內部函式的簽名改成由呼叫的一方決定怎麼拿著物件（binding 用 wrapper，引擎自己派發時用 root），一個模組一個模組做。安全性靠 `crates/bindings/soundness.md` 已有的論證（wrapper 活著就撐著它的 DOM 物件，callback 的 `this` 和參數整個呼叫期間被 V8 留著），不碰 GC。省不到的：呼叫中途新建的物件、要帶出操作的結果、走訪到一半的節點（沒有 wrapper，還是 root）；這些呼叫已經比 Chrome 快。
    - **不選在 elise 加只活在一次呼叫的便宜的 root**（像 handle scope：登記估約 10 條、涵蓋所有的 root、函式的簽名不用改）：涵蓋廣，但是新的 root 種類，要補 GC 的 soundness 論證和測試；使用者選只做上一條。記在「之後的可能」。不選兩個都做。
    - **加快審核（使用者同意，2026-11-13）**：4k-C 的八小段併成三組，各停一次給使用者審：C1 到 C3；C4（型別的設計，走訪當試點）；C5 到 C8。組裡每一步只跑改到的 crate 的測試和 `cargo fmt`；mutation test、`sanity.py`、相關的 WPT、對 Chrome 的量測和 `progress.md` 在那一組的最後做一次。
    - **`this` 和參數改拿 wrapper 移到 4k-F 之後再評估（2026-11-13，使用者）**：查型別的設計時發現規模比當初說的大：寫成會呼叫 script 的函式有 185 個、分布在 `dom_api` 的 39 個檔案，彼此之間和向 host 要求（`ScriptHost` 的二十多個方法）傳的都是 root，所以不能只改入口。可行的設計是 `ScriptHost::Held<T>`（`Hold<T>`：在一個操作裡交出物件；`Persistent<T>` 實作它，binding 的是「wrapper 或 root」），函式把型別換掉、`this.get(cx.gc)` 換成 `this.get(cx)`；估兩三千行。省的是每個呼叫一到兩個 root（各約 80 到 90 條、合計約 3 到 8 ns），這些呼叫是 70 到 700 ns、已經是 Chrome 的 0.44 到 1.0 倍。先做真的落後的（4k-D 到 4k-F），到時再決定。先做掉不用改簽名的一點：沒有參數的成員登記 `this` 的 root 用讀的操作（20 條）而不是寫的（44 條）。
    - **分段**：4k-C1 新的外殼；4k-C2 「先試」的機制和走訪的 9 個成員的快路；4k-C3 剖析 `inp.select()`、`abort()`；4k-C4（`this` 和參數改拿 wrapper）移到 4k-F 之後評估。
44. **`PLAIN` 刪掉，每個成員都從只拿 info 的 front 開始；`Front` 的預設實作留到 4m-3 拿掉**（2026-11-13；經過在 `progress.md` 的 4m-2）：
    - 4m-2 開始前選的是「把 `Front` 的預設實作拿掉，由編譯器保證新的 front 什麼都有；wasm 和測試的 front 用 macro 補」，不選留著 `PLAIN` 的清單和照原始碼檢查的測試。
    - 做的時候發現新的 front 沒有的 15 個方法幾乎都是 script 那一側才用的（listener、filter、observer、custom element 的 callback…），只在 `Front::whole` 交給完整的 front 之後被呼叫。要讓編譯器保證，得先把它們分到完整的 front 才實作的 trait；那個 trait 由誰實作是 4m-3 的題目（`V8Front` 怎麼收掉）。所以拿掉預設實作排進 4m-3，和搬法一起決定（待使用者確認）；在那之前由 `dom_js` 的一個照原始碼檢查的測試守著（`docs/test-gaps.md`）。
    - cached getter 和 legacy caller 要 fork 多兩個收原始 callback 的入口（只改 Rust）：不選留著 `Member<M>` 拿 scope 的實作只給這兩處用。
45. **4m-3：冷的方法留 scope、拿掉殼；script 那一側分成自己的 trait**（2026-11-13；使用者：都照建議）：
    - **新的 front 還交給完整的 front 的 88 個方法**：本體改成拿 scope 和參數的普通函式，新的 front 在當場建的 scope 裡直接呼叫，不再建完整的 front（殼約 35 條指令，先前量的 62 減 27）。不選全部不建 scope：要在 fork 加很多只拿 isolate 的入口（陣列、物件的讀寫、promise、`TryCatch`、數字的轉換），每個都要 soundness 論證，會執行 script 的轉換沒有 scope 很難寫對；這些路量過不落後 Chrome，多省的 27 條換不到看得見的差距。Blink 這些路也在 callback 的 `HandleScope` 裡用一般的 V8 API。之後量到落後的個別方法仍可以改成不建 scope。
    - **script 那一側**：分出 `ScriptFront: Front`，放只有跑 script 時才用的方法，`WholeCall::run` 要它；實作它的是一個瘦的型別（scope、參數、回傳值），給 `whole`、cross-origin 的成員、legacy caller、pairs、interceptor 用，取代完整的 front。之後拿掉 `Front` 的預設實作（wasm 和測試的 front 用 macro 補）。不選讓新的 front 為那些方法寫 `unreachable!`（沒有編譯期的保證），不選留著完整的 front 只給 script 用（兩套幾乎重複的程式）。
    - **不是成員的**：pairs 和 4i 留下的 interceptor 改到瘦的型別上，trampoline 不換（不熱，換了要 fork 再加幾種原始 callback）；`state.rs` 的內部函式不動。
    - **分段**：4m-3a 88 個方法照類別搬；4m-3b `ScriptFront` 和瘦的型別，刪掉完整的 front、`Call`、`Kept`；4m-3c 拿掉預設實作、清舊的實作、整個階段的檢查和對 Chrome 的收尾量測。各停一次給使用者審。

## 待決定

3. 「V8 在操作裡配置」（決定 6：先不做）要做時：規則 2 用「整個 old 當成活的」還是保守地掃 stack。

## 之後的可能

- 進來的字串改成 external（決定 7）：V8 的 sandbox 打開之後短字串也改得了，那時再做，照 Blink 把名稱的編號和 intern 過的值記在 resource 上（原型和數字在 `progress.md`、`prototypes/external-strings.patch`）。
- 操作可以巢狀：DOM 做到一半呼叫 script 不用結束操作。
- wasm 的 front 照同樣的方式量。
- 每次現算的字串（`style.color`、其他 getter 給的新的 `DomString`）在字串的快取裡永遠找不到（快取照位址記），每次新建 V8 的字串並登記一筆（`Weak` 加 finalizer）：`style.color` 是 `'red'` 時 4709 條指令、2664 個 cycle（4i-2 量的；決定 36 當時猜是 CSS 那一側，不是）。空字串已經不經過快取。決定 38 做了。
- dictionary 的成員名稱每次讀都重新做成 V8 的 internalized 字串（`templates::string`）再 `Get`：一個成員約一千條指令，`new MouseEvent('x', init)`（三十個左右的成員）三萬兩千條（4j-1 量的）。Chromium 產生的 dictionary 轉換把成員名稱的 V8 字串每個 isolate 快取一份（`V8PerIsolateData::FindOrCreateEternalNameCache`），讀成員時不建字串。 決定 40 做了。
- window 的 getter 當全域變數讀（`navigator`、`location`、`performance`、`innerWidth`、`onload`…）每次約 1700 條指令（4k 量的）：V8 不替 holder 是 dictionary mode 的 API getter 建 inline cache（全域物件一定是），每次走 `Runtime_LoadGlobalIC_Slow`。V8 留的出路是 cached accessor（`FunctionTemplate` 帶 cache 的 private symbol，`TryLookupCachedProperty`）。還沒查 Chromium 對哪些屬性用、也沒量 Chrome 同樣的讀取。 2026-11-12 量了 Chrome：一樣慢（`navigator` Chrome 94.7 ns、machopo 88.3 ns），不是差距，不排。
- 只活在一次呼叫的便宜的 root（elise 加，像 V8 的 handle scope：呼叫開始記下位置，登記是往陣列尾端寫一格，結束時整批丟掉）：現在登記再放掉一個 `Persistent` 約 80 到 90 條，估可以到約 10 條（沒量），而且涵蓋呼叫中途新建的物件和要帶出操作的結果。要補 elise 的 soundness 論證和 model、fuzz（concurrent marking 進行中丟掉 root 照現有的規則記下來）。決定 43 不選。
- 會跑 script 的函式的 `this` 和參數改拿 wrapper、不登記 root（決定 43 的設計：`ScriptHost::Held<T>`）：185 個函式和 `ScriptHost` 的介面，估兩三千行；省每個呼叫一到兩個 root（各約 80 到 90 條，合計約 3 到 8 ns），而這些呼叫已經是 Chrome 的 0.44 到 1.0 倍。2026-11-13 4k 做完後決定不排（步驟表的 4k-C4 拿掉）。
- 給節點新的 wrapper 時判斷它的 realm（`InfoOp::root_node`：節點的 realm 的文件是不是目前的 global 那個 window 的文件）約佔那條路的一成多、估 3 到 4 ns（4k-D 的取樣換算，沒有單獨量），只在節點第一次被 JS 拿到時付。isolate 只有一個 window 的 realm 時可以不判斷，但要先定清楚幾種邊界（iframe 建立和移除的當下、導覽時新舊 realm 交替、目前的 realm 不是任何 frame 的）並各有測試：判斷錯是 wrapper 建在錯的 realm（拿到另一個視窗的 prototype）。2026-11-13 使用者問值不值得，決定不做。
- 最簡單的 getter 的 callback 還比 Blink 的多約 28 條指令（4k-G 數的：`nodeType` 102.6 對 Chrome 72.9，1.40 倍；`getAttribute` 以上的呼叫已經持平或更少）。剩下的：解出 `this` 之後檢查位址在這個 heap 的 cage 裡 5 條（`elise::Mutation::from_engine`：可以只留 debug build，代價是「拿錯 heap 的 wrapper 只會 panic」改成靠「一個 isolate 一個 DOM heap」的論證；2026-11-13 使用者決定不做）；型別的 tag 是執行時向 elise 登記的編號，每次讀一個全域的值、檢查初始化、算出要比的值共 6 條（變成編譯時的常數要改 elise 分配型別編號的方式）；對只對應一種型別的 interface 不比 tag、直接信任 V8 的 signature 檢查（Blink 的做法）約 5 條，出錯時是記憶體錯亂而不是 `TypeError`、只能靠審查，而且 `this_as` 要變成 `unsafe` 的契約，不做；標記中的旗標 2 條是 elise 的 concurrent marking 要的。
- `HTMLElement` 的 constructor 每次建一個 scope 檢查 `new.target` 是不是 `HTMLElement` 本身（決定 35）：要不建 scope 拿到 realm 的 interface 物件，或換一種比法。
