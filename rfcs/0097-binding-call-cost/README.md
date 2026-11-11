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

## 待決定

3. 「V8 在操作裡配置」（決定 6：先不做）要做時：規則 2 用「整個 old 當成活的」還是保守地掃 stack。

## 之後的可能

- 進來的字串改成 external（決定 7）：V8 的 sandbox 打開之後短字串也改得了，那時再做，照 Blink 把名稱的編號和 intern 過的值記在 resource 上（原型和數字在 `progress.md`、`prototypes/external-strings.patch`）。
- 操作可以巢狀：DOM 做到一半呼叫 script 不用結束操作。
- wasm 的 front 照同樣的方式量。
