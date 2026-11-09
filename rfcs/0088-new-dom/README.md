# RFC 0088：新的 DOM：照 kun 自己的 GC 的規則重新設計

- 開始日期：2026-10-26
- 狀態：✅ 已完成：2026-10-27（實驗：第 1 段的 1a 到 1g；程式留在 `0088` 分支上，不 merge 回 `main`，已決定的事項 17；驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`（實驗的結論：什麼要留、什麼要改、各自的成本；第 1 段量完時（1e）的數字和過渡策略的比較）
- 程式：只在 `0088` 分支上（已 push 到 `origin`），沒有 merge 回 `main`。這份寫到的 `dom_next`、`kunNextDom`、`kun:next-dom`、`bench.py blink-perf --next-dom` 和 `bench.py speedometer --dom-calls`、`gc` 的 JS 接法、`bindings` 的 `unified` 和它們的測試，都在那個分支上；這個資料夾的 `v8/`、`tla/`、`mutants-*.toml` 也是照那個分支的程式寫的。

## 摘要

照 RFC 0087 做好的 GC（`crates/gc`）的規則重新設計 DOM 的實驗。新的 DOM 的最小切片（`dom_next`：一種 `Node`、十來個 API 和 Speedometer 最常呼叫的十幾個成員）經過並排的入口接上 JS 和 wasm 兩個 engine，在 blink-perf 的六頁和每個成員一頁的微基準上，和今天的 cppgc DOM、Chrome 比。

成果是結論和量到的數字（`details.md` 的「實驗的結論」）：

- **DOM 本身不是問題**：改樹、寫入的操作和 Chrome 一樣或更快。
- **GC 和 Chrome 一樣**：JS 文件的 heap 照 cppgc 在 Chrome 裡的方式和 V8 接（節點不搬、wrapper 存節點的位址、節點帶 V8 的 `TracedReference`），之後建 wrapper 的頁每次 iteration 的 GC 和 Chrome 差不多。
- **差距在每次呼叫的固定成本**：照 Speedometer 的呼叫分布加權約 Chrome 的 1.2 倍。剩下的大半是 kun 的 V8 呼叫 API callback 的地板：一個空的 getter 要 4.2 ns，Chrome 整個 `nodeType` 只要 3.5 ns。

真正的 `dom`、`dom_api`、`dom_js`、`dom_wasm` 由之後的 RFC 0089 重做，引用這裡的結論。原本第 1 段之後要決定的過渡策略和第 2 段以後都不做（已決定的事項 11、17）。

## 動機

**當時的現況**：RFC 0087 做好了新的 GC，在 `gc_bench` 的錄下的 DOM 工作上，main thread 的 GC 時間是 cppgc 的 1/13 到 1/140、收尾的停頓 p99 最多約 0.4 ms；但它還沒有 crate 用。今天的 DOM（`dom` 約 3.3 萬行、`dom_api` 約 6 萬行）跑在 cppgc 上：每次配置、寫邊、建 root 都經過 FFI，JS 文件和 wasm 文件各一種 heap。

**不知道的**：接上 engine 之後的成本。`gc_bench` 只量了 GC 這一層；JS 文件的 wrapper 和 V8 怎麼接、binding 每次呼叫進一次 `Heap::mutate`、wasm 的 guest 的 handle 換成 root 表的一格，這些加起來是快是慢、停頓多長，要端到端才看得到。

**原本要決定的**：過渡期兩個 engine 怎麼辦（「理由和其他做法」）。第 1 段量完之後（1e）改成：這份只做實驗，回答新的 DOM 該怎麼做、成本在哪裡，目標是 DOM 這一層的成本和 Chrome 一樣（已決定的事項 11）。

**不在範圍內**：

- style、layout、paint：量的是 DOM 的操作，新的 DOM 的節點不畫出來。
- 事件、其他所有 API、頁面原本的 `document`：頁面照舊用今天的 DOM，新的 DOM 只經過並排的入口用（已決定的事項 1 的第 2 條）。
- 把新的 DOM 做成完整的 DOM：只補量測要的成員。

要解決的 `docs/limitations.md` 裡的限制：沒有（程式不進 `main`）。

## 設計概要

做完的樣子（都在 `0088` 分支上，`main` 沒有）：

- **`crates/dom_next`**（在 `0088` 分支上）：新的 DOM 的切片，只依賴 `gc`、`gc_macros`，沒有 V8 也沒有 `unsafe`。一個 renderer thread 同一時間一個 `Dom`（一個 `gc::Heap`），跟著 engine 的 DOM 建、一起丟。
- **入口，只在 `MACHOPO_NEXT_DOM=1` 時裝**：JS 的 `kunNextDom`（`dom_js` 的 `next.rs`）、wasm 的 WIT 套件 `kun:next-dom`（`dom_wasm` 的 `next.rs`）。

  ```js
  const doc = kunNextDom.createDocument();      // 新的 DOM 的 document，不在頁面上、不畫出來
  const root = doc.createElement("div");
  for (let i = 0; i < 5000; i++) {
      const child = doc.createElement("div");
      child.setAttribute("id", "n" + i);
      root.appendChild(child);
  }
  root.innerHTML = "<p>a<b>b</b></p>";          // html5ever 照新的 DOM 建節點
  ```

- **JS 文件的 heap 照 cppgc 在 Chrome 的 unified heap 裡的方式和 V8 接**：V8 開始、推進、收尾它的 major GC；kun 給 V8 的 patch（`v8/v8.patch`）和 rusty_v8 的 glue（`v8/glue.patch`）由 `v8/build.py` 編進 `target/v8-0088`，分支的 `.cargo/config.toml` 用 `RUSTY_V8_ARCHIVE` 連結它。wasm 文件的 heap 照舊自己做 GC。
- **量測**：`bench.py blink-perf --next-dom`。blink-perf 的六頁（JS 版在 `0088` 分支上的 `scripts/bench/blink_perf/next/`，C 版在 `guests/*-next.c`），和每個成員一頁的 `next/calls/`（18 頁，只有 JS 版）。三種跑法：kun 的新 DOM、kun 今天的 DOM（detached 的 document）、Chrome，`--versions` 可以只跑其中幾種。`bench.py speedometer --dom-calls` 數 Speedometer 呼叫每個 DOM 成員幾次。

## 詳細設計

### 新的 DOM 的切片（`dom_next`）

- **型別**：一種 `Node`（`#[derive(GcObject)]`），帶它的種類（document、element、text、comment）、五條樹的連結（`Member<Node>`）、它的 document（`owner`）、名稱的編號、namespace，和 JS 的邊（`JsEdge`）。屬性是另外的物件（`HeapVector<Attribute>`），文字是 `Text`。為什麼一種、名稱為什麼放在 `Node` 上，見已決定的事項 3。
- **名稱**：intern 成編號，表以字串為鍵、用 FxHash，只有新的名稱才做 html5ever 的 atom；`class` 固定一個編號。節點給 binding 一個名稱的鍵（`NameKey`），binding 照它快取 V8 字串。
- **值**：屬性值和文字是 `External` 裡的 `Arc<str>`，由 sweep 的 worker 放掉；binding 照 `Arc` 的位址快取讀出的值的 V8 字串。
- **操作**：
  - 建立：`createDocument`、`createElement`、`createElementNS`（只有 HTML、SVG、MathML 和沒有 namespace）、`createTextNode`。
  - 改樹：`appendChild`、`insertBefore`、`removeChild`，名稱和 pre-insert 照 DOM 的規則檢查。
  - 走訪：`parentNode`、`firstChild`、`lastChild`、`previousSibling`、`nextSibling`、`firstElementChild`。
  - 讀寫：`nodeType`、`nodeName`、`tagName`、`ownerDocument`、`className`、`textContent`、`setAttribute`、`getAttribute`。
  - `innerHTML` 的 setter：最小的 html5ever `TreeSink`，用 fragment parsing 建節點。
- **操作和 GC**：每次呼叫是一次 `Dom::mutate`，開始時是安全點；只讀的 getter 用 `Dom::read`，不進安全點（`gc` 的 `mutate_without_safepoint`：不配置的操作做不出 GC，Oilpan 也只在配置時 GC）。每個配置的地方一個 `Site`（V8 的 `AllocationSite`），留著的樹被 pretenure。

### JS 文件（`dom_js`、`bindings`）

**和 V8 的 GC 接**（已決定的事項 9、10；Chrome 的 cppgc 和 V8 的 unified heap 的形式）：

- **heap 由 V8 推進**（driven，已決定的事項 6 的第 4、5 條）：新的 heap 的用量回報給 V8，算進 `EmbedderSizeOfObjects`，V8 照和 cppgc 同一套規則開始、推進、收尾它的 major GC。新的 heap 自己不開始 marking。
- **排程**（已決定的事項 7）：新的 heap 照 V8 的 `IncrementalMarkingSchedule` 的算法，在 V8 每一步裡用 cppgc 用剩的時間標。平行的工作經過 `bindings::gc_platform` 交給 V8 的 platform 的 worker。
- **節點和 wrapper**：
  - JS 文件的 heap 裡帶 `JsEdge` 的物件（節點）一出生就在 old、不搬。
  - wrapper 存節點的位址，tag 是 0x4000 加上型別的編號。V8 的 marking visitor 碰到這段 tag，把位址直接交給新的 heap 標上。
  - 節點的 `JsEdge` 就是一格 droppable 的 V8 `TracedReference`。新的 heap 的 marker 標到節點時，在任何 thread 上直接交給 V8 的 worklist（`TracedMarker`，cppgc 的 `ConcurrentUnifiedHeapMarkingVisitor`）。
  - V8 的 scavenge 丟掉沒改過的 wrapper 時，經過 `EmbedderRootsHandler::ResetRoot` 清掉節點的那一格。script 之後再拿這個節點時建新的 wrapper。
  - unwrap 時經過讀取的 barrier：新的 heap 是 SATB、V8 是 Dijkstra，少了它會漏標。
- **協定先寫成模型**：`tla/Unified.tla`，`tla/check.py` 窮舉；拿掉每一部分都要找到反例。
- **註冊的拆法**（已決定的事項 15）：照 Chrome 拆掉 `CppHeap`，先讓 V8 進行中的 GC 收尾（`kun::FinishMarking`），再取消註冊、刪掉 glue 的物件。

**binding**（已決定的事項 13、14、16；照 Blink 生成的 binding 的形狀）：

- **receiver**：函式帶 V8 的 signature，receiver 由 V8 檢查，binding 只在操作裡 unwrap 一次。
- **`State`**：新的 DOM 的 per-isolate 狀態，放在 V8 的 isolate data slot（Blink 的 `V8PerIsolateData`）。只設一次、活到 isolate 拆掉，callback 借用它，不做計數。
- **字串**：
  - 名稱的 V8 字串照 `NameKey` 快取；讀出的屬性值的 V8 字串在 256 格的快取裡（Blink 的 `StringCache`）。
  - `textContent` 經過 `State` 裡留著的 scratch 字串。
  - 寫進的值直接交進 DOM，不再複製。
- **新的 wrapper**：每個 context 一個 boilerplate，新的 wrapper 是它的 `Clone`（`bindings::clone_object`，Blink 的 `CreateWrapperFromCache`）。wrapper 在操作外先建好（V8 配置時可能 GC），再在一次操作裡建節點、wrap、寫它的 JS 的邊。
- **rusty_v8 的 fork**：兩個只改 Rust 的 commit（`510aec2`、`b4b619e`，在 fork 的 `kun` 分支上）。照 V8 的標頭檔 inline 地讀 `FunctionCallbackInfo` 的欄位、寫 `ReturnValue`、讀 isolate 的 data slot；debug build 每次也呼叫 V8 自己的、比對。

### wasm 文件（`dom_wasm`）

- **WIT 套件**：`kun:next-dom`，resource 只有一種 `node`（Component Model 的 resource 沒有繼承），成員用 typed 的 `func_wrap`。
- **handle**：guest 的 handle 是新 heap 的 root 表的一格，drop 時放掉。
- **heap**：不是 driven 的，自己做 scavenge、concurrent marking 和 sweep，節點照舊在 nursery。

### 量測

- **頁和跑法**：見「設計概要」。都在 Apple M4、`perf` profile、接電源、每種 3 次取中位數。
- **GC**：GC 從 trace 讀，Chrome 也錄 trace（只錄 `v8`、`cppgc` 和 User Timing）。
- **每次呼叫**：每次呼叫的 ns 是 runs/s 換算的。照 Speedometer 加權，是把每個 suite 每個成員被呼叫的次數乘上它每次呼叫的時間。
- **數字放在哪裡**：在 `progress.md`，第 1 段量完時（1e）的摘要存成 `bench-1-apple-m4.json`。

### 測試

- **各 crate 的測試**：
  - `dom_next` 的單元和整合測試；
  - `dom_wasm` 的 `new_dom`（C 的 guest）；
  - `dom_js` 的 `next_dom` 和 `next_dom_gc`（V8 的 incremental、單 thread 的 GC）；
  - `gc` 的 `js::`；
  - `bindings` 的 `gc_platform`、`isolate_state`。
- **模型和 mutation**：協定的 TLA+ 模型；每一段的 mutation test（`mutants-1a.toml` 到 `mutants-1g.toml`）。
- **沒有逐條測過的**：V8 的 patch 沒有逐條做 mutation（每條要重編 V8），由 `next_dom`、`next_dom_gc` 間接守著（`docs/test-gaps.md`）。

### 分階段

| 段 | 內容 |
| --- | --- |
| 1a | 新的 DOM 的最小切片和它的測試；wasm 文件的並排入口，六頁的 C 版本在新的 DOM 上跑、量 |
| 1b | JS 文件的並排入口（cppgc 的 stub、以 id 為 key 的 wrapper 表、weak handle）；六頁的 JS 版本、量。1c 整個換掉 |
| 1c | JS 文件真正的接法：V8 的 major GC 每推進一步也推進新的 heap，兩邊都做完才收尾（在本機改 V8），節點指向 wrapper 是交回 V8 的邊 |
| 1d | 排程：新的 heap 自己的排程決定 V8 每一步裡標多少，平行的工作在 V8 的 worker 上 |
| 1e | 量六頁、取樣；過渡策略的比較（`details.md`） |
| 1f | JS 文件的 heap 照 cppgc 的模型和 V8 接：節點不搬、wrapper 存位址、節點帶 `TracedReference`、wrapper 可以被 V8 的 scavenge 丟掉；先量 Chrome 的 GC 當目標 |
| 1g | 每次呼叫 DOM 的成本：Speedometer 的呼叫分布、18 個成員三邊量，五步實驗把它壓低；註冊的拆法 |

1a 先做 wasm：沒有 V8 和 cppgc 的牽連，最快端到端。各段量到的和做過的檢查在 `progress.md`。

## 缺點

- **程式不進 `main`**：新的 DOM、`gc` 為它加的 API（`0088` 分支上 `crates/gc` 的 `src/js.rs` 等）、V8 的 patch 和 rusty_v8 的 fork 的 commit 都只在 `0088` 分支上。下一份 RFC 要用的，得重新寫成經過審查的步驟。
- **切片少做了事**：今天的 DOM 和 Chrome 做的事，新的 DOM 很多都沒有（`details.md` 的「倍數裡有多少是少做了事」）。改樹的頁上，比 Chrome 快的部分有一部分是這個原因。
- **量的是並排的入口和不在頁面上的 document**，不是原本的頁面：沒有 render、事件和 style 的失效，比 Speedometer 的取樣裡「呼叫 DOM」的時間小得多。照 Speedometer 加權的數字是估計，不是 Speedometer 的成績。
- **V8 的 patch 只在本機**：V8 怎麼 fork 沒有決定（已決定的事項 5）。kun 的 V8 是一般的 release build，和 Chrome 的官方 build（PGO、ThinLTO）不同，呼叫 API callback 的地板也因此不能直接比。
- **只在一台機器上量過**：Apple M4。

## 理由和其他做法

**為什麼先做 MVP**（2026-10-26，使用者提議）：新的 DOM 怎麼接、成本在哪裡，只有端到端才量得到；先做最小的切片，用數字決定，不用猜。

**入口**（已決定的事項 1 的第 2 條）：

- 甲（選了）：新的 DOM 經過並排的入口（JS 的一個全域物件、wasm 的另一個 WIT 套件），頁面原本的 `document`、window、計時器、`fetch` 照舊。
- 乙（不選）：頁面的 `document` 整個換成新的 DOM。量的是原本的頁面，但要接 parser 建頁面、window 的 `document`、script 的執行、事件，範圍大好幾倍。

**JS 文件怎麼和 V8 接**：

- 1b 先用 cppgc 的 stub、以 id 為 key 的 wrapper 表和 weak handle（RFC 0087 為了不 fork V8 選的橋）。取樣裡一半的時間在表和 weak handle，wrapper 也不被節點撐住，所以改成真正的接法（已決定的事項 5）。
- 1c 是交換的形式，加上 wrapper root 的表和 `TracedReference` 的表，因為那時節點會搬、有 nursery（已決定的事項 6）。1e 量到這些轉接是 GC 的大頭，所以 1f 照 cppgc 的模型重做：節點不搬，兩張表都不要（已決定的事項 9、10）。
- 不把整個 GC 搬進 V8，理由在已決定的事項 9。

**過渡策略**：原本第 1 段之後要決定（四種：先只給 wasm、並排開發一起換、只換底層、一個型別一個型別搬）。1e 的比較和建議（乙：並排開發，追上之後一起換）留在 `details.md`。已決定的事項 11 之後不再決定，下一份 RFC 自己定。

## 先例

- **Blink 搬到 Oilpan**（約 2013 到 2017 年，憑記憶，細節沒有查證）：從 refcount 一個型別一個型別搬，過渡期用巨集讓同一份程式在兩種模式下都能編，在效能的 bot 上比，最後才搬 Node。這份是同樣「先並存、量了再換」的想法，規模小得多。
- **Chrome 的 unified heap**：Blink 的 DOM 是 cppgc 的物件，wrapper 直接指向 DOM 物件，V8 和 cppgc 互相交 worklist、一起標。這份最後的接法就是這個形式（1f），只是另一邊是 `gc` 而不是 cppgc，所以要改 V8。
- **Servo**：DOM 物件由 SpiderMonkey 的 GC 管（JS 的物件就是 DOM 的 reflector），沒有自己的 DOM heap；這份和它相反，DOM 有自己的 heap。

## 已決定的事項

1. **先做 MVP**（2026-10-26，使用者確認）。
   1. 第 1 段是一個最小、端到端的切片：新的 DOM 的幾個型別和十來個 API，接上 `dom_js` 和 `dom_wasm`，在 blink-perf 的六頁 DOM 工作上量；過渡策略等量完再決定。
   2. 新的 DOM 經過並排的入口給頁面用（甲），不換掉頁面的 `document`（乙）。
   3. 草稿寫在 `0087` 的 branch 上，和 RFC 0087 一起 merge 回 `main`（使用者決定）。
2. **第 1 段的做法**（2026-10-26，使用者逐條確認，都照建議）。
   1. **新的 crate `dom_next`**（甲）：只依賴 `gc`、`gc_macros`，沒有 V8 也沒有 `unsafe`；`sanity.py boundary` 把它加進 renderer 那一側的名單。兩個 engine 的 binding 在 `dom_js`、`dom_wasm` 各一個 `next` 模組（V8 的程式只能在 `dom_js`）。過渡策略選了乙就沿用，舊的 `dom` 刪掉時改名。不選放在這個資料夾的原型（乙：接不上 `kun` 裡的 engine，量不到端到端），不選 `dom` 裡的模組（丙：`dom` 有 V8 和 `unsafe`、為 cppgc 寫的規則）。Blink 的 DOM（`core/dom`）也不碰 V8，binding 在 `bindings/core/v8`。
   2. **一個 renderer thread 一個 `gc::Heap`**（甲），和現在的 DOM（`ScriptEngine::create_dom` 一個 thread 一次）、cppgc 的 `CppHeap`、Blink 的 Oilpan（一個 thread 一個 heap）一樣：同一個 thread 上的文件之間可以直接有 `Member`（之後的 `adoptNode`、iframe），stub 只要知道一個 heap。不選每份文件一個（乙：跨文件的參照要另外設計，一個 isolate 對好幾個 heap）。
   3. **每次 binding 呼叫是一次 `Heap::mutate`**（甲）：任何一次呼叫都可能是 GC 的時機，一個 task 裡一直配置也收得掉；量安全點的成本，佔得多再替只讀的呼叫加一種不進安全點的操作（乙）。不選一個 task 一次（丙：task 裡的垃圾收不掉，就是現在 wasm 文件的限制）。
   4. **stub 先當 strong root，同一個 wrapper 用 id 為 key 的表**（甲）：wrapper 的 internal field 指向 cppgc 的 stub，stub 記著新 heap 的一格 root，stub 被 cppgc 收掉時（pre-finalizer）放掉；每份文件一張表，key 是節點的 id（`gc` 的 id 表），value 是 wrapper 的 weak handle。節點和 wrapper 之間的循環收不掉（六頁沒有）；weak handle 的成本在報告裡分開算（Blink 後來不用每個 wrapper 一個 weak handle）。不選第 1 段就做 RFC 0087 的 1d 的同步對齊（乙：1d 是 Dijkstra 上做的，SATB 要另外設計，量不出它的好處，留到之後專門的一段），不選不用 stub、靠 V8 的 weak callback（丙：量的不是之後會用的橋）。
   5. **入口：JS 的 `kunNextDom`、WIT 的 `kun:next-dom`（interface `dom`），只在 `MACHOPO_NEXT_DOM=1` 時裝**（甲）：沒設時網頁看不到它、不建新的 heap，wasm 的 linker 不提供那個套件。照 Blink 的 runtime enabled features 和只在測試模式裝的 `window.internals`。不選命令列參數（乙），不選一直裝著（丙）。
   6. **`innerHTML`：替 `dom_next` 寫一個最小的 html5ever `TreeSink`**（甲）：元素、文字、屬性，註解和 doctype 丟掉，插入的操作照常（foster parenting 照常），沒有 template 的內容；setter 拿掉子節點，再用 fragment parsing 建。不選先不做那一頁（乙：沒有一頁量到 parser），不選把 `html` crate 的 sink 改成泛型（丙：動到舊的 parser 和它的掛點）。Blink 的 `innerHTML` 也用 tree builder 的 fragment 模式建節點。
   7. **基準：同樣的邏輯在現在的 DOM 的 detached document 上**（甲，`document.implementation.createHTMLDocument()`，`kun:dom` 有 `create-html-document`），同一個 build、同一次執行；原本的 blink-perf 頁面和 `gc_bench` 的數字只當參考。`bench.py blink-perf --gc`，每頁 3 次取中位數，Apple M4、接電源、`perf` profile；weak handle 的成本用取樣分開。沒有通過的門檻：1c 整理數字和過渡策略的建議給使用者決定。（2026-10-27 補，使用者同意）JS 的六頁寫成一份，只有拿文件的那一行不同（有 `kunNextDom` 就用它，沒有就 `createHTMLDocument`），同一份頁面量 kun 的新 DOM、kun 的舊 DOM 和 Chrome for Testing 三種，都在 detached document 上；wasm 的 guest 用 kun 的 WIT，只能新舊 DOM 互比。新的 DOM 變成頁面的 `document` 那一段，驗收加上原本的 blink-perf 頁面不改一行對 Chrome。

3. **1a 的資料的形狀**（2026-10-27，使用者確認，都選甲）。
   1. **一個 `Node` 型別**：帶種類（document、element、text）和五條樹的連結（`Member<Node>`）；元素才有的資料（名稱、屬性）是另一個 GC 物件，由 `Node` 的 `Member` 指著，`Node` 本身保持小、走訪時不用判斷型別。和現在的 `dom` 的 `Node` 一樣，也最接近 Blink 的 `Node`（樹的連結都在基底類別上）。不選每種節點一個型別、連結用 `Member<Erased>`（乙：每次走訪多一次型別的檢查）。
      - （2026-10-27 改，使用者同意）實作時元素的名稱放在 `Node` 上（intern 過的編號，`Cell<u32>`），只有屬性（`HeapVector<Attribute>`）和文字（`Text`）是另外的物件。名稱只佔 4 bytes（document 和 text 也帶著，用不到）；分出去的話每個元素多一次配置，`nodeName` 和 parser 問名稱時也多一次跳轉。Blink 的 `Element` 也把 tag name 放在自己身上，`ElementData` 只放屬性。不選照原本的分法改回去（要重量六頁，換到的只有 document 和 text 少 4 bytes）。
   2. **名稱 intern、值和文字放在 `External<Box<str>>`**：元素和屬性的名稱 intern 成編號（像現在的 `dom::name` 的 atom），不配置、沒有 finalizer；屬性值和文字的 `Box<str>` 在 `External` 裡，每個都有一個 finalizer、在 sweep 的 worker 上放掉，第 1 段量它的成本。不選先在 `gc` 加長度可變、沒有 finalizer 的資料物件（乙：GC heap 上的字串，RFC 0085 的「來源 2」的方向，要先改 `gc` 的配置，超出第 1 段；第 2 段和「來源 2」一起定）。

4. **1b 的做法**（2026-10-27，使用者確認，都照建議）。
   1. **stub 和 wrap、unwrap 的 `unsafe` 在 `bindings`，做成通用的**（甲）：一個帶著任意 Rust 值、有自己的 tag 的 cppgc 物件，`bindings` 給 safe 的 API（建 stub 並 wrap 一個 template 做的物件、從 wrapper 讀回那個值）；`dom_js` 的 `next` 模組用它寫 template 和 callback，`bindings` 不知道 `dom_next`。Chromium 也是這樣分：通用的 wrap、unwrap 在 `platform/bindings`（`ScriptWrappable`、`V8DOMWrapper`），各介面的 binding 在 `bindings/core/v8`。不選在 `dom` 的 `v8_engine.rs` 加 stub 的型別（乙：`dom` 要依賴 `dom_next`），不選讓 `dom_js` 也能寫 `unsafe`（丙：改 repo 的規則）。
   2. **stub 的 `Drop`（cppgc 的 finalizer）直接放掉 root**（甲），改掉 2.4 寫的 pre-finalizer：rusty_v8 沒有 pre-finalizer。cppgc 的 finalizer 在 heap 的 thread 上跑（`crates/dom/soundness.md` 的 A1），`Persistent` 的 drop 只動 root 表；binding 呼叫在新的 DOM 的一次操作裡配置 stub、觸發 cppgc 的 sweep 時，finalizer 也在那次操作裡跑，要有測試。不選排進佇列、下一次操作開始時才放（乙：多一層，等到確定直接放不行再做），不選在 fork 加 pre-finalizer（丙）。
   3. **一個 thread 一個新 heap 由 `dom_next` 持有**（甲）：JS 和 wasm 文件共用，`dom_wasm` 原本自己的那份搬過去，各自的 handle 表和 stub 照舊分開。這是 2.2 的意思。不選兩個 engine 各一個（乙：同一個 thread 上兩個 heap）。
      - （2026-10-27 改，使用者同意，已決定的事項 6 的第 4 條）heap 不再是 thread-local，跟著 engine 的 DOM 一起建、一起丟：renderer 換文件的種類時（`use_script_language`）丟掉整套 engine 和 DOM，JS 和 wasm 文件不會同時在一個 thread 上，而 thread-local 的 heap 活得比 engine 久，會接在已經丟掉的 isolate 上。同一時間仍然是一個 thread 一個 heap（2.2）。
   4. **同一個 wrapper 的表一個 thread 一張**（甲），改掉 2.4 寫的每份文件一張：新的 DOM 還沒有 node document，節點在 document 之間移動時對不上。和 Blink 的 main world 一樣：同一個節點在 main world 只有一個 wrapper。不選每份文件一張（乙）。
   5. **innerHTML-setter 共用的頁面拿掉 `document.body.appendChild(div)`**（甲）：三種跑法的 div 都不在 document 裡；三種都不 render，那一行原本只是讓它被畫出來。不選讓新的 DOM 的 document 也建出 html、head、body 和 `body` 的 getter（乙）。
   6. **JS 的頁放在 `scripts/bench/blink_perf/next/`，同樣的路徑；`--next-dom` 每頁多三列**（甲）：kun 的新 DOM（`MACHOPO_NEXT_DOM=1`）、kun 的舊 DOM（沒設，`createHTMLDocument`）、Chrome，各 `--runs` 次，GC 照樣從 trace 讀（新的 GC 和 V8、cppgc 的事件都在）；1a 的 wasm 兩列留在同一份報告。不選另一個模式、另一份報告（乙）。

5. **真正的接法提前到第 1 段，做交換**（2026-10-27，使用者決定）。1b 量到的 JS 數字用的是一個之後不會留下的接法：取樣裡一半的時間在 wrapper 的表（27.6%）和 weak handle（23.2%），而且 wrapper 不被節點撐住，節點還活著時 wrapper 就可能被收掉、expando 跟著不見，跨兩邊的循環也收不掉；拿這個數字決定過渡策略，數字和設計都偏。所以在第 1 段多做 JS 文件真正的接法，照 RFC 0087 的「接 V8」：stub 不再是 strong root，V8 的 major GC 經過 stub 標新的 heap，節點指向 wrapper 是交回 V8 的邊（Blink 的 `ScriptWrappable` 的 `TraceWrapperV8Reference`、今天的 DOM 的 `WrapperSlot`），不用表也不用 weak handle。原本的 1c（量和過渡策略）變成 1e；原本在 1b 之後列的便宜的修改（照 id 索引的表、更快的 hash）不做。不選另寫一份專門接 V8 的 RFC（0088 要停在半路等它），不選照原來的順序留到最後（過渡策略建立在暫時的接法上，expando 的錯也留很久）。
   1. **只做交換，不做同步**（RFC 0087 第 1 段的原型（1d）做過的兩種對齊）：新的 heap 的 marker 和 V8 的各自推進，V8 每推進一步 `CppHeap` 的 marking 也推進新的 heap，兩邊都做完才收尾，要改 V8。Chrome 的 unified heap 是同一種形式的完整版（對照 V8 的原始碼）：V8 的 marking visitor 碰到 wrapper 指向的 C++ 物件就丟進 cppgc 的 worklist（`VisitCppHeapPointer`，concurrent marker 也是），cppgc 碰到 `TracedReference` 就丟進 V8 的，V8 每一步給 `CppHeap::AdvanceMarking` 一段預算，兩邊的 worklist 都空了才收尾（`IncrementalMarking::ShouldFinalize`），開始 GC 的大小算進 cppgc 的（`Heap::GlobalSizeOfObjects`）。原型的交換缺排程（V8 自己的排程下新的 heap 的工作擠到收尾），所以多一段 1d 做排程。不選先同步再交換（一開始選的，同一天改）：同步只要改 rusty_v8 的 glue、不改 V8，但它專用的部分（延後 `Trace` 的 glue、一次 `Trace` 走完子樹、把 barrier 的工作包成 cppgc 物件交回去）做完交換就丟，它的停頓原型已經量過（一百萬個節點 4.5 ms），兩種的吞吐量應該差不多（都沒有表和 weak handle），過渡策略不缺它的數字；使用者接受改 V8，同步「不用 fork」的好處用不上。
   2. **V8 的改動先只在本機**（使用者決定）：怎麼 fork V8（V8 本身的 fork、rusty_v8 的 submodule、release）第 1 段之後再說。在本機怎麼改、怎麼連結，1c 開始前定。

6. **1c 的做法**（2026-10-27，使用者逐條確認）。
   1. **在本機改 V8 照 RFC 0087 的原型的做法**（甲）：V8 本身和 rusty_v8 的 `src/binding.cc` 的改動是這份 RFC 的資料夾裡 `v8` 資料夾的 patch（1c 加），腳本套用、編、編完還原（失敗也還原），library 和 binding 放在 kun 的 `target/v8-0088/`，不放進每個 kun 的 build 共用的 `../third_party/rusty_v8_artifacts`；0088 的 `.cargo/config.toml` 用 `[env]` 的 `RUSTY_V8_ARCHIVE` 連結它，給 Rust 的 `extern "C"` 宣告在 `bindings`。從原型的 patch 開始（同一個 V8 的版本）。每個 checkout 第一次要完整編一次 V8，之後只重編改過的檔案；V8 的 fork 定下來之前，0088 不能 merge 回 `main`。不選直接改 checkout 不還原（乙：`ci.py third-party` 看到 rusty_v8 的工作目錄有改動就停下，`test.py` 每個指令前都跑它），不選在 fork 的本機 branch 上 commit（丙：和 `third_party.toml` 釘的 commit 對不上，一樣停下）。
   2. **協定先寫進 TLA+ 的模型、證明不會漏標再實作**（甲）：從 RFC 0087 的 `tla/SatbYoung.tla`（`crates/gc` 實作的協定：SATB、black allocation、young generation）出發，在這份 RFC 的資料夾另寫一份，加上 V8 那一側的 marker（Dijkstra 加 black allocation）、兩邊互相交的邊（V8 標到 stub、我們標到指向 JS 的欄位）、兩邊都做完才收尾、wrapper 的 root（scavenge 時是 strong，old marking 開始時不掃）和經過它讀出時的 barrier；照 0087 的 `check.py` 的做法，每個部分拿掉一次要找到反例。RFC 0087 的協定都先寫模型；這次兩種 barrier 混用，原型就漏過一次（經過 wrapper 讀出的物件要標上）。不選只靠測試和論證（乙）。V8 和 cppgc 的 unified heap 沒有形式化的模型，兩邊都是 Dijkstra。
   3. **V8 和新的 heap 接在 worklist 這一層，不要 stub**（乙，使用者選：先試，改不動再回來看甲、丙）：wrapper 的 internal field 不再指向 cppgc 的物件；V8 的 marking visitor（`VisitCppHeapPointer`，主要的、young generation 的和 concurrent 的）碰到 kun 的 tag 時把它交給新的 heap 的 worklist，新的 heap 的 marker 碰到指向 JS 的欄位時交給 V8 的。這是 Chrome 的 unified heap 的形式（cppgc 和 V8 互丟 worklist），DOM 可以完全不用 cppgc，也省掉每個 wrapper 一個 cppgc 物件和一次 FFI（1b 的取樣裡 stub 佔 2.2%）。代價是改的是 V8 最熱的程式（marking visitor、concurrent marking、wrap 的 tag），每次升級 V8 都可能衝突，thread 之間的 worklist 要重新設計和證明，原型沒試過。兩邊都做完才收尾、收尾時推進新的 heap，仍然要甲的掛點（`CppHeap` 的 `AdvanceMarking` 和收尾的 `ReachTransitiveClosureWithEmbedder` 也是這樣推進 cppgc），所以乙包含甲：改不動時退回甲（stub 加掛點）。不選甲（在 `CppHeap` 加掛點、wrapper 照舊指向 cppgc 的 stub：改動小、原型做過，但 DOM 留著 cppgc 當 stub 的 heap），不選丙（把 V8 拿掉的 `EmbedderHeapTracer` 放回去：介面現成，但要把散在 marking visitor、concurrent marking 和收尾的舊程式接回來，一個 isolate 也只接一個 tracer，今天的 DOM 還在 `CppHeap` 上）。
   4. **一個 thread 一個 heap，跟著 engine 的 DOM 建和丟；JS 的 engine 的 heap 建好時就接上 isolate**（甲，照 Chrome）：接上之後 old marking 只在 V8 的 major GC 開始、跟著 V8 推進（兩步之間新的 heap 的 concurrent worker 照常標）、在 V8 的收尾結束，heap 長到門檻時不自己開始；wasm 的 engine 沒有 isolate，它的 heap 照現在自己推進；scavenge 一直是新的 heap 自己在安全點做。renderer 一個 thread 同一時間只有一種文件（換種類時丟掉整套 engine 和 DOM，heap 跟著丟），所以 wasm 文件不受 V8 影響；heap 的模式在建好時定、之後不換，不用處理標記中途接上、接下來，也不會接在已經丟掉的 isolate 上（改了 4.3 的 thread-local）。Blink 一個 thread 一個 Oilpan heap（`ThreadState`），有 isolate 時就是接在它上面的 `CppHeap`，major GC 一律由 V8 開始和推進；沒接上的 `CppHeap` 自己推進；Chrome 沒有換 engine 這件事，heap 用到 thread 結束。因為 V8 推進時會呼叫新的 heap，**DOM 的操作裡不呼叫 V8**（RFC 0087 的規則）這一條要守住：1b 在操作裡建 wrapper（在 V8 上配置），要改成操作裡把結果 root 起來、離開操作才建 wrapper；V8 推進時新的 heap 在一次操作中途就 panic，有測試守著。不選第一次用新的 DOM 時才接上（要處理標記中途的轉換），不選照 1b 留著 thread-local 的 heap（換成 wasm 文件之後還接著丟掉的 isolate）。
   5. **新的 heap 照 cppgc 的做法讓 V8 開始和推進 GC**（乙）：V8 的 patch 加一個回報配置的函式，新的 heap 的用量算進 `EmbedderSizeOfObjects`（V8 判斷要不要 GC 的 `GlobalSizeOfObjects` 的一部分，和 cppgc 同一套上限和成長的規則），回報時做 `CppHeap::ReportBufferedAllocationSizeIfPossible` 做的事：到上限就開始 incremental marking、標記中就照配置推進（`AdvanceOnAllocation`）、超過太多就當場收尾。它可能當場開始、推進或收尾一次 GC，所以只在安全點（`Heap::mutate` 之外）累積起來報，和第 4 條同一條規則。1d 原本的「讓 V8 知道新的 heap 的大小」在這裡做掉，1d 只剩每一步推進多少、什麼時候收尾。不選回報成 external memory（甲：不用改 V8，但上限是另一套（上次 GC 之後的最低量加上固定的量）、它開始的 GC 帶著收乾淨 external memory 的旗標、到 heap 的上限就是一次完整不分段的 GC、不隨配置推進標記；它是給 ArrayBuffer 的內容這類 heap 外的記憶體用的），不選到門檻時叫 V8 做完整的 GC（一次長停頓）。
   6. **節點指向 wrapper 是 `gc` 的一種欄位 `JsEdge`：`dom_js` 的 `TracedReference` 表的一格的編號**（甲）：表是一個陣列加上空格的清單；新的 heap 的 marker 標到帶 `JsEdge` 的物件時把編號交給 V8 標那一格；收尾時 `gc` 告訴 `dom_js` 哪些 `JsEdge` 的物件死了，那幾格忘掉、重用。V8 收尾時先放掉沒標到的 `TracedReference`（`MarkCompactCollector::CollectGarbage` 的 `ClearNonLiveReferences` 在 `CppHeap::FinishMarkingAndProcessWeakness` 之前），所以死掉的格子不能再 reset，只能忘掉（Blink 擁有它的 C++ 物件也一起死、不再碰它）。節點上 1b 加的 `id` 換成它，找 wrapper 是讀欄位再讀表的一格；之後 listener 這類 DOM 指向 JS 的欄位用同一種。Blink 的 `ScriptWrappable` 直接帶著 `TraceWrapperV8Reference`；新的 heap 不能放 V8 的 C++ 物件（`gc` 不碰 V8，scavenge 搬動物件只複製 bytes），所以多一層表。不選保留 1b 以 id 為 key 的表（乙：找 wrapper 和標記時都要 hash，每個節點都要查有沒有 wrapper；Chrome 只在非 main world 用表（`DOMDataStore`）），不選節點直接存 JS 物件的位址（丙：V8 搬動 JS 物件時要更新新的 heap 裡的欄位，要在 V8 加一套 remembered set）。
      - （2026-10-27 改，已決定的事項 10 的第 3 條）節點的欄位直接就是那一格 `TracedReference`，不再有表。
   7. **wrapper 指到的 root 是另一張表，照 V8 的 `TracedHandles`**（乙）：每格是物件的位址和這一輪有沒有被 V8 交過來的 bit，一個陣列加上空格的清單；新的 heap 的 scavenge 當 strong root 掃（不知道 wrapper 還活不活，V8 的 scavenge 也不說）；old marking 開始時不掃，只從 V8 交過來的編號標；標記中新建的一建好就算標過；經過它讀出物件時有讀取的 barrier（新的 heap 是 SATB，V8 是 Dijkstra，JS 丟掉 wrapper 的最後一個參照不經過新的 heap 的 barrier）；major GC 收尾時沒被交過來的（wrapper 死了）由 heap 自己放掉。沒有 Rust 的值持有它，編號存在 V8 的 wrapper 裡。V8 分成 `GlobalHandles`（strong、持有的人放掉）和 `TracedHandles`（scavenge 時是 root，major GC 時只有被追到才活、沒標到的 GC 自己放掉）兩張，wrapper 的 root 就是新的 heap 的 `TracedHandles`。不選和 `Persistent` 同一張表、每格記種類（甲：兩種規則（誰放掉、開始時掃不掃、收尾時找死的）混在一起，`Persistent` 的熱路徑多一次判斷）。
      - （2026-10-27 改，已決定的事項 10 的第 1、2 條）拿掉：JS 文件的節點一律在 old、不搬，wrapper 直接存位址，不需要這張表。
   8. **V8 的 scavenge 不丟掉沒改過的 wrapper**（甲，使用者要之後評估乙）：和今天的 DOM 一樣（`WrapperSlot` 不是 droppable），1e 和今天的 DOM 比時，差別只在 DOM 和 GC 本身。結果是有 wrapper 的節點和它的 wrapper 互相撐住、兩邊的 minor GC 都收不掉，要等 major GC（新的 heap 的 scavenge 把 wrapper 的 root 當 strong：wrapper 活不活要 V8 標過一輪才知道）；沒有 wrapper 的年輕節點照樣在 scavenge 收掉。Chrome 預設：cppgc 沒有 young generation（`cppgc_young_generation` 是實驗中的功能），DOM 物件都只在 major GC 收；V8 那一側 `reclaim_unmodified_wrappers` 開著，scavenge 丟掉 droppable、沒改過的 wrapper（`IsUnmodifiedApiObject`），之後再建。實驗中的 unified young generation 是兩邊的 minor GC 一起收（V8 的 minor GC 經過 wrapper 標到 C++ 物件），它關掉丟 wrapper。不選乙（照 Chrome 預設丟掉沒改過的 wrapper：只解決 V8 那一半，節點照樣活到 major GC，1e 的比較多一個不是 DOM 的差別；**1e 量完之後評估**，到時今天的 DOM 和新的一起開才公平），不選丙（兩邊的 minor GC 一起收：要改 V8 的 scavenge，Chrome 也還沒預設開）。
      - （2026-10-27 改，已決定的事項 10 的第 4 條）改成乙：節點的 `TracedReference` 是 droppable。目標從和今天的 DOM 比換成和 Chrome 比，今天的 DOM 不動。
   9. **1b 的接法全部拿掉**（甲）：`dom_js` 的 wrapper 表、weak handle 和它的 finalizer、generation，`bindings` 的 `carry` / `carried`（通用的 cppgc stub）和它在 `crates/dom/soundness.md` 的一節、它們的測試和 `sanity.py` 的例外。`bindings` 換成用 kun 的 tag 把 wrapper 的 root 的編號 wrap 進 wrapper、讀回來的 API；`next_dom` 的測試照新的規則重寫（節點撐住 wrapper、expando 經過 GC 還在，wrapper 撐住節點，兩個都沒人拿著時一起收掉）。1b 的數字和取樣留在 `progress.md` 當對照；第 3 條退回 stub 時 `carry` / `carried` 從 1b 的 commit 拿回來。不選保留 1b 的接法、用環境變數切換（乙：兩套接法和測試，1b 的那套語意不對也要修），不選只留 `carry` / `carried`（丙：沒人用的 `unsafe` API 和論證要繼續維護）。
   10. **wrapper 裡存 wrapper 的 root 表的編號**（甲）：加 1，0 是 V8 的「沒有 wrap」。V8 標到 wrapper 時交出編號，新的 heap 用它在表裡找到節點、標它、記下這一輪被交過來了；binding 拿節點是讀 V8 的那一格、讀表、讀出節點（加讀取的 barrier）。新的 heap 的 scavenge 搬動節點只更新自己的表，V8 那一格不變，V8 的背景 marker 隨時讀它都安全。公開的 `Object::Wrap` 只收 cppgc 的 `Object::Wrappable`，V8 也會把那一格當 `Wrappable` 讀（型別資訊、名稱，heap snapshot 會用），所以 V8 的 patch 加 kun 自己寫、讀那一格的函式，所有讀它的地方碰到 kun 的 tag 就跳過（第 3 條的成本，存什麼都一樣）。Chrome 存 cppgc 物件的位址，因為 cppgc 不搬動 wrapper 指著的物件。不選存節點的位址（乙：scavenge 搬動有 wrapper 的節點時要改 V8 的表、要記每個 wrapper 在 V8 的哪一格，而且和 V8 的 concurrent marker 讀那一格搶，要另外設計兩邊 GC 之間的同步）。
      - （2026-10-27 改，已決定的事項 10 的第 2 條）改成乙：存節點的位址。當初不選它，是因為 scavenge 會搬動有 wrapper 的節點；節點一律在 old 之後就不搬了。
   11. **新的 heap 的 worker 找到的 JS 參照直接交進 V8 的 worklist**（甲，照 Chrome）：V8 的 patch 照 cppgc 的背景 marker（`ConcurrentUnifiedHeapMarkingVisitor`：從 V8 的全域 worklist 開一份自己的、設好 isolate 的 scope、用 `UnifiedHeapMarkingState::MarkAndPush` 標 `TracedReference`、做完一段 publish）包一個小物件，新的 heap 的每個 worker 一輪標記用一個，碰到 `JsEdge` 就讀表的那一格直接交。所以第 6 條的 `TracedReference` 表要能讓 worker 從別的 thread 讀、main thread 同時加格子：照 V8 的 `ExternalEntityTable`（`CppHeapPointerTable` 的底層：預留整塊位址、分段 commit、格子不動、空格的清單），它要 `unsafe`，從 `dom_js` 搬進 `bindings`，論證寫進 `crates/dom/soundness.md`（改了第 6 條的表的位置）。Chrome 的 `TracedReference` 在 C++ 物件裡，背景 marker 讀它本來就在標的物件，不需要這張表。兩個做法 `gc` 的介面一樣（告訴外面這個 `JsEdge` 要交給 V8），表的並行讀寫做不出來時退回乙，`gc` 不用改。不選排進佇列、V8 叫新的 heap 推進時在 main thread 上交（乙：表留在 `dom_js`、V8 的 patch 不碰 worklist，但交給 V8 要等下一次推進，main thread 每個 `JsEdge` 一次交接）。

7. **1d 的做法**（2026-10-27，使用者確認，都照建議）。1c 做完時，V8 每推進一步，新的 heap 只叫醒自己的 worker，main thread 不標；worker 搶不到 CPU 時（V8 的 worker 同時在標，兩組 worker 加起來約是核心數的兩倍），V8 的 task 一直推進卻沒有進展，最後超過上限被迫收尾，新的 heap 剩下的全部擠進收尾的停頓。
   1. **新的 heap 有自己的排程，照 cppgc 的預設**（甲）：照 V8 的 `heap::base::IncrementalMarkingSchedule` 的算法，預計 500 ms 標完，照經過的時間算出現在應該標到多少，扣掉已經標的（main thread 加 worker），差多少就在這一步的 main thread 上補多少，最少一段（64 KiB 對應的量）；時間的上限是 V8 給這一步的時間（task 1 ms、配置時 5 ms）扣掉 cppgc 已經用掉的，V8 量這一步時把我們算進去，三邊加起來不超過一步的時間。單位用物件數：marker 只數物件（RFC 0087 的已決定的事項 36 的第 4 條，為了速度不數 bytes），估計量從上一輪標到的物件數推。`gc` 加一個有上限的 main thread 標記（現在只有標完全部）。V8 不知道我們還剩多少，只靠 `IsDone` 決定什麼時候收尾，cppgc 也是這樣（這版的 V8 預設 cppgc 自己排程，`CppHeap::AdvanceMarking` 不帶 bytes 的上限）。不選 V8 的統一排程（乙：`--incremental_marking_unified_schedule` 這版預設關、還在實驗，全域的旗標會連今天的 DOM 的 cppgc 一起改，它用 bytes 算，patch 也大），不選維持只靠 worker（丙：就是上面的問題，沒有 worker 的 platform 也全部在停頓裡做）。
   2. **接上 isolate 的 heap 的工作交給 V8 的 platform**（甲，照 Chrome）：只有一組 worker，由 V8 的 job 排程統一分配。`bindings` 實作 `gc::Platform`，glue 加一個呼叫 Rust 的 C++ `JobTask`，包 `PostJob`、`NotifyConcurrencyIncrease`、`Join`（rusty_v8 沒有從 Rust 丟工作給 V8 的 platform 的 API）；job 借用 heap、給 V8 的 worker 跑，靠 `Join` 保證，是一段新的 `unsafe`，論證寫進 `crates/dom/soundness.md`。scavenge 和 sweep 的 job 跟著換過去，`Gc.Worker` 的 trace 保留；wasm 的 heap 沒有 isolate，照舊用 `gc` 自己的 pool。Chrome 的 V8 和 cppgc 都在 process 共用的 `base::ThreadPool`；RFC 0087 的已決定的事項 20 的第 1 條定 `Platform` 時就留了這條路。不選兩組都不動、1e 量了再說（乙：1d 的排程假設 worker 跑得動，1e 的數字混進搶 CPU 的雜訊），不選 V8 標記期間限制我們的 worker 數（丙：數字是猜的）。
   3. **驗收：`GcRecord` 記收尾的停頓裡標了多少物件**（甲）：`gc` 的測試用沒有 worker 的 heap，靠推進的步驟標完大部分，排程的算法單獨測；`dom_js` 的測試讓一棵大樹經過 V8 的 incremental GC，收尾的停頓裡標的只佔一小部分；照樣跑 mutation test。停頓的長度留到 1e 和六頁一起量。不加新的 TLA 模型：排程只改每一步標多少，不改收尾的條件（兩邊都空才收尾），1c 的模型涵蓋。不選只在 1e 看 trace（乙：沒有測試守著排程）。

8. **1e 的做法**（2026-10-27，使用者確認，都照建議）。
   1. **量法沿用 1a、1b 的，另外分出新的 heap 在 V8 的 GC 裡的時間**（甲）：`bench.py blink-perf --next-dom` 的六頁、五種跑法（wasm 和 JS 各在今天的 DOM 和新的 DOM 上，加上 Chrome），Apple M4、`perf` profile、每種 3 次，和 1a、1b 的數字直接比。`gc` 在 V8 推進的步驟和收尾的停頓裡各發一個 trace 的 span，報告多一欄「V8 的 GC 裡新的 heap 的部分」：現在新的 heap 的時間混在 V8 的步驟（算成 marking）和停頓（算成 atomic）裡。V8 的 `GCTracer` 用 `MC_INCREMENTAL_EMBEDDER_TRACING`、`MC_MARK_EMBEDDER_TRACING` 把 cppgc 在 V8 的步驟和停頓裡的時間分開記，是同一件事。不選只用現在的報告（乙：JS 新 DOM 的 GC 時間說不清是誰的）。
   2. **取樣 gc-forest 和 create-element 的 JS 新 DOM**（甲）：samply，照 1b 的分類，對照 wrapper 的表和 weak handle 拿掉之後時間去了哪裡。不選只看成績（乙）。
   3. **過渡策略的比較寫進 `details.md`**（甲）：甲、乙、丙、丁四種重新估（JS 的接法做好了，乙的前提已經成立），比量到的數字、要改寫的量、並存期間的成本、風險和 Chromium 的先例；新的 DOM 少做的事每頁列出來，標明倍數裡有多少是少做了事。寫完給使用者建議，決定之後才列第 2 段以後。不選只在 `progress.md` 寫結論（乙）。
   4. **「V8 的 scavenge 丟掉沒改過的 wrapper」不在 1e 評估**（甲）：列進第 2 段以後的規劃（「之後的可能」的第一項）。1e 只量決定過渡策略要的；它要改兩套 DOM 再一起量，結果不影響選哪種過渡策略。不選放進 1e（乙）。

9. **第 1 段多一段 1f：JS 文件的 heap 照 cppgc 的模型和 V8 接**（2026-10-27，使用者決定）。1e 量到 GC 引擎本身比 V8 和 cppgc 快（RFC 0087 的 `gc_bench`），JS 文件的頁面上 GC 卻佔 26–32%：多的是 1c 為了「物件會搬、有 nursery」加上的轉接（wrapper root 的表、`TracedTable`、scavenge 把有 wrapper 的節點當 strong 而一再複製、V8 停頓裡走兩張表），cppgc 不需要這些，因為它的物件不搬、在 Chrome 裡也沒有新生代（`details.md` 的「第 1 段量到的」）。使用者要接法和 cppgc 完全一樣、把成本降到最小，可以大改目前的設計。
   1. **方向**：JS 文件的 heap 裡會被 wrap 的物件一出生就在 old、不搬；wrapper 存它的位址（加 kun 的 tag），節點的欄位直接放 V8 的 `TracedReference`；V8 標到 wrapper 時把位址推進新的 heap 的 worklist，新的 heap 標到節點時交那一格給 V8（cppgc 的 `VisitCppHeapPointer`、`MarkAndPush`）。拿掉 wrapper root 的表、`TracedTable`、`JsEdgeTable` 和收尾時走它們，以及 scavenge 的 strong root。會改掉已決定的事項 6 的第 6、7、8、10 條，細節在 1f 開始前列。wasm 文件的 heap 照舊有 nursery；JS 文件不會被 wrap 的物件要不要留在 nursery（只有節點在 old，或整個 heap 都沒有 nursery），量了再定。
   2. **先量 Chrome 的 GC**：`bench.py blink-perf --next-dom` 的 Chrome 那幾次也錄 trace，用同一個量測區間算 GC，當作 1f 的目標。
   3. **binding、字串、template 的成本排在 1f 之後**：1e 的取樣裡，binding 的進出、`createElement` 的名稱每次轉成 Rust 的字串、每建一個 wrapper 複製一次 template 的 `v8::Global`，加起來和 GC 的轉接差不多大；Chrome 也付這些，但它的 binding 是產生出來的 C++、V8 的字串對應到 atom 的快取、template 存成 `Local`，便宜很多。
   4. **過渡策略等 1f 量完再定**：結果不會改變選哪一種（`details.md` 建議乙），但第 2 段改寫 `dom_api` 建在節點怎麼存 wrapper 上。
   不選把整個 GC 搬進 V8：取代 V8 自己的 JS heap 要重寫和 JIT 綁在一起的 barrier、物件的排法、指標壓縮和 sandbox；把 `gc` 用 C++ 放進 V8 取代 cppgc，功能上和這個方向一樣，只是換語言；DOM 的節點直接當 V8 的 heap 物件，跨 heap 的成本是零，但 Rust 的 DOM、style、layout 每次都要經過會搬的 handle，wasm 文件也用不了，Chrome 也沒有這樣做。

10. **1f 的做法**（2026-10-27，使用者逐條確認，都照建議）。量 Chrome 的 GC 之後（`progress.md` 的 1f）：建 wrapper 的四頁每次 iteration 的 GC 是 Chrome 的約 5 倍，差在 young（新的 heap 的 scavenge 複製有 wrapper 的節點、V8 的 scavenge 一個 wrapper 都不能丟）和 V8 的停頓（走兩張表）；innerHTML-setter 反過來是 Chrome 的四分之一（沒有 wrapper 的節點在 nursery 裡收掉）。
   1. **JS 文件的 heap 裡所有節點一出生就在 old、不搬，其他物件照樣在 nursery**（甲，照 cppgc：Chrome 裡 DOM 物件不搬、沒有新生代）：最簡單，wrapper 一律直接存位址。代價是 JS 文件裡沒有 wrapper、很快就死的節點（innerHTML 建的）也要等 major GC。先做甲、量，innerHTML 這類頁掉得明顯時再做丙。不選整個 JS 的 heap 沒有 nursery（乙：屬性、文字這些永遠不會被 wrap 的也只能在 major GC 收；照第 5 條量一次對照），丙先不做（節點照樣在 nursery，年輕的節點被 wrap 時釘住它那一頁、scavenge 時整頁原地升到 old：兩邊的好處都拿到，但 `gc` 要加頁的原地升級和釘住；V8 的 scavenger 有整頁原地升級，保守的 stack 掃描也釘住頁），不選丁（不搬動的新生代：sticky mark bits，V8 的 MinorMS、cppgc 實驗中的新生代；要重做 `gc` 的新生代）。
   2. **wrapper 存節點的位址，靠 tag 的不變式保證讀回來的是對的，`unsafe` 在 `bindings`**（甲，照 `dom` 的 I14 和 Blink 的 `ToScriptWrappable`）：V8 那一格存節點在 cage 裡的位址加 kun 的 tag，V8 標到 wrapper 時直接把位址推進新的 heap 的 worklist（cppgc 的 `VisitCppHeapPointer`）。kun 的 tag 只用在新的 DOM 的節點上、只有 `bindings` 用它 wrap、wrap 前確認那個物件沒 wrap 過，wrapper 活著時一定撐著節點，所以讀出的位址指著活的 `Node`；`bindings` 給 `dom_js` 一個安全的泛型 unwrap，一種 tag 一種型別。unwrap 時的讀取 barrier 留著：我們的 barrier 是 SATB（cppgc 是 Dijkstra），1c 的模型證明少了它會漏標，成本是一次判斷。不選交給 `gc` 檢查（乙：死掉的節點的頁可能已經還給作業系統，讀 header 就會出錯，做不到真的檢查），不選保留 wrapper root 的表（丙：1e 量到的成本來源之一）。
   3. **節點的欄位直接就是那一格 `TracedReference`**（甲，照 Blink 的 `ScriptWrappable::main_world_wrapper_`）：`gc` 多一種 8 bytes、對齊 8、自己從不讀寫內容的欄位，marker 掃到物件時把那一格的位址交給 engine（`JsEdgeSink`）；帶這種欄位的型別在 driven 的 heap 裡 `gc` 一律配置在 old，第 1 條變成 `gc` 的保證（實作時補上「driven 的」：`Node` 帶著這種欄位，wasm 文件的 heap 不是 driven 的，節點照舊在 nursery）。`bindings` 把那一格當成 V8 的 `TracedReference`（fork 的 `TracedSlot`，大小一個指標），用 V8 的 API 原地寫（V8 的 barrier 照常）、用 1c 的 V8 marker 標，讀寫都是 V8 的 atomic 的。節點死了什麼都不用做（V8 收尾前已經放掉沒標到的 handle）。拿掉 `TracedTable`、`gc` 的 `JsEdgeTable`、收尾時忘掉格子那一段。不選保留 `TracedTable`（乙），不選節點存 handle 的位址、表只負責回收（丙：節點不搬時沒有表的必要）。
   4. **節點的 `TracedReference` 是 droppable**（甲，照 Chrome 的預設 `reclaim_unmodified_wrappers`）：V8 的 scavenge 不把沒改過（沒有 expando）的 wrapper 當 root，收掉之後呼叫 `EmbedderRootsHandler::ResetRoot`，`bindings` 把節點的那一格 reset 成空的；JS 之後再拿這個節點時建新的 wrapper（舊的沒人拿得到、沒改過，script 看不出換過）。rusty_v8 沒有 `SetEmbedderRootsHandler` 和建 droppable `TracedReference` 的 API，加在 glue 的 patch 裡（V8 的公開 API，不改 V8）。改了已決定的事項 6 的第 8 條：當初為了和今天的 DOM 比得公平，現在比的是 Chrome。不選維持不丟（乙：V8 的 scavenge 那一塊省不下來），不選只讓某些類型 droppable（丙：V8 本來就一個一個看改過沒有）。
   5. **驗證照 1c，模型先行**（甲）：改寫 `tla/Unified.tla`（wrapper 直接存位址、節點帶 `TracedReference`、V8 的 scavenge 丟掉沒改過的 wrapper），拿掉每一部分都要找到反例；`gc`、`bindings`、`dom_js` 的測試（帶 JS 欄位的型別在 old、直接存位址的 wrap 和 unwrap、droppable 和 `ResetRoot`、丟掉之後拿到新的 wrapper、有 expando 的不丟），`next_dom_gc` 的旗標和負載；mutation test（V8 的 patch 照舊排除）。驗收是量六頁：第 1 條的甲和乙各一次，和 Chrome 的 GC 比，看建 wrapper 的四頁每次 iteration 的 GC 比 1f 開始時（約 54 ms）少多少、innerHTML-setter 掉多少。1c 的 wrapper root 的表、`TracedTable`、`JsEdgeTable`、`Handoff`、`gc` 的 `WrapperRoot` 那一組 API 一起刪掉，不留切換的開關。不選只靠測試（乙）。
   6. **新的 DOM 的配置經過 `Site`，每個配置的地方一個**（甲，照 V8 的 `AllocationSite`）：`gc` 的 pretenuring（RFC 0087 第 3 段）一直沒用上，wasm 版的 gc-tree 的 GC 949 ms 裡 919 ms 是 scavenge 把留著的樹的每個節點複製兩次。JS 和 wasm 的 `createElement`、`createTextNode`、屬性的陣列、文字和屬性值、parser 的 sink 建的節點各一個 `Site`，很快就死的和留下來的不混在一起。驗收：gc-tree 的 wasm 版的 young 大幅下降，create-element、gc-forest 不被拖去 old；`dom_next` 加測試。不選每個型別一個（乙：parser 建的和 JS 建的混在一起），不選先不做（丙）。

11. **這份是過渡的實驗**（2026-10-27，使用者）：做完之後另開一份 RFC，真正重做 `dom`、`dom_api`、`dom_js`、`dom_wasm` 等；這份的成果是結論和量到的數字（什麼要留、什麼要改、各自的成本），寫在下一份 RFC 找得到的地方（`details.md`、`progress.md`）。所以不再決定過渡策略（`details.md` 的甲到丁：怎麼把今天的 DOM 換成新的），`dom_next` 也不補成完整的 DOM；1f 之後只做回答設計問題的實驗。目標照使用者的說法：DOM 這一層的成本越少越好，和 Chrome 一樣。

12. **1g：先量每次呼叫 DOM 的成本，照 Speedometer 的比例**（2026-10-27，使用者同意）。1f 之後 GC 不再是差距（`progress.md` 的 1f），剩下的在每次呼叫的路徑和 DOM 本身；新的 DOM 只是切片，跑不了 Speedometer，所以：
   1. 在今天的 kun 的 `dom_js` 加上每個 DOM 成員被呼叫幾次的計數，跑一次 Speedometer，得到每個 suite 的呼叫分布。
   2. 照分布挑最常被呼叫的成員，新的 DOM 補上它們（只為了量），用單一操作的微基準量今天的 DOM、新的 DOM 和 Chrome 每次呼叫的時間（`bench.py calls` 的做法）並取樣拆開：進出 callback、unwrap、進入 `gc` 的操作、DOM 本身、V8 的 API、字串、建 wrapper。兩者相乘，得到每個 suite 的 DOM 這一層的估計。
   3. 量完再決定做哪些實驗：進入 `gc` 的操作的成本、V8 的小函式（跨語言的 LTO 或在 Rust 重寫）、字串的快取和 external string、binding 的形狀（從 WebIDL 為各 engine 生成、直接呼叫 DOM，或保留共用的分派；V8 的 fast API call）。
   不先做丙（已決定的事項 10 的第 1 條）：甲和乙量起來一樣，剩下的 young 是 V8 那一側的；用 innerHTML 多的 suite 出現時，順便量 old 的成長和 major GC 的頻率再看。

13. **1g 第 3 步的實驗，照這個順序，每做完一個就用同樣的 18 頁重量**（2026-10-27，使用者照建議）。第 2 步量到改樹的和 Chrome 一樣或更快，讀的卡在約 17–19 ns（Chrome 約 4 ns），建立和寫字串是 2–3.5 倍（`progress.md` 的 1g）；都只在 `kunNextDom` 上做，結論給下一份 RFC：
   1. **甲：讀取的地板**：檢查和讀回 receiver 合成一次 unwrap（Blink：V8 的 signature 檢查加一次 `ToScriptWrappable`）；`State` 不每次查 isolate 的 slot；只讀的操作不進 `gc` 的安全點（Oilpan 只在配置時才可能 GC）；型別的編號 inline。
   2. **乙：V8 的呼叫本身**：在 Rust 直接讀 `FunctionCallbackInfo` 的欄位、不經過 FFI（rusty_v8 的 fork），或試跨語言的 LTO（Chrome 的是 inline 的 C++）。（2026-10-27 補，使用者選乙2）做法是改 rusty_v8 的 fork 的 Rust：`FunctionCallbackInfo` 的欄位和 `ReturnValue` 的 setter 照 V8 的標頭檔 inline 地讀寫，debug build 同時呼叫 V8 自己的、比對兩者；fork 在本機 commit、`third_party.toml` 釘它，push 等使用者。不選乙1（`bindings` 自己的 callback 入口：只有新的 DOM 受惠），不選乙3（跨語言的 LTO：預編的 V8 的 clang 和 rustc 的 LLVM 要同一個版本）。另外 receiver 是物件時讀回節點不再檢查它是不是物件（`bindings::unwrap_object`）。
   3. **丙：字串**：intern 過的名稱快取它的 V8 字串，回傳時不經過 Rust 的 `String`，查名稱時不先配置（Blink 的 `StringCache`）。
   4. **丁：建立**：`template()` 不每次複製 `v8::Global`，名稱的雜湊換成更快的，看建 wrapper 能不能更短（Blink 從 `V8PerContextData` 快取的建構子複製）。

14. **1g 第 4 步：看極限做得到哪**（2026-10-27，使用者：繼續做剩下的三項，V8 可以任意改）。第 3 步之後照 Speedometer 加權是 Chrome 的 1.33 倍，剩下的三塊疊在一起、可以相加（`progress.md` 的 1g）：
   1. **交界的固定成本**：rusty_v8 的 fork 直接讀 isolate 的 embedder data（V8 的 `Isolate::GetData` 在標頭檔裡 inline：`kIsolateEmbedderDataOffset`），拿 annex 不經過 FFI；`bindings` 拿掉 `kun::Unwrap` 之前多餘的 `IsApiWrapper`（V8 裡已經檢查了）；`State` 看 annex 變快之後還要不要 thread 的快取。其他再改 V8。
   2. **讀出的值的字串**：值存成 `Rc<str>`，`kunNextDom` 用一個大小固定的快取（以值的指標為鍵、拿著那個 `Rc`，位址不會被別的值重用）存它做過的 V8 字串（Blink 的 `StringCache` 以 `StringImpl` 為鍵）；名稱的快取改成照編號的陣列。
   3. **建 wrapper**：照 Blink 從每個 context 一個的 boilerplate 複製（`v8::Object::Clone`，rusty_v8 沒有就加在 glue），不再每次 `NewInstance`。
   每做完一項只量受影響的頁、只跑新的 DOM。

15. **新的 heap 的註冊照 Chrome 的方式拆掉**（2026-10-27，使用者選丙）。`next_dom` 在負載下偶爾 segfault，原因是 1c 起的拆法：glue 的物件 detach、不刪，V8 的 marking worker 還可能碰到已經放掉的 `Shared`（`progress.md` 的 1g）。三個做法：甲，拆掉時等進行中的呼叫做完（還剩約 0.2%，detach 本身的競爭還在）；乙，glue 的指標改成 atomic、`Shared` 用 refcount，glue 的物件照樣 leak；**丙**，照 Chrome 拆掉 `CppHeap`：先讓 V8 進行中的 GC 收尾（patch 加 `kun::FinishMarking`），再刪掉 glue 的物件、放掉 `Shared`，不 leak。選丙：這份是下一份 RFC 的原型，拆法應該就是最後的；只改拆掉的路徑，量測不受影響。

16. **1g 第 5 步：callback 的形狀**（2026-10-27，使用者照建議的順序：先做甲，量完再看乙）。第 4 步之後讀節點還是約 9 ns 對 Chrome 的 4 ns，省下的固定成本移到呼叫的其他地方（`progress.md` 的 1g）。反組譯 `nodeType` 的 callback：rusty_v8 的入口本身只有十來個指令（不建 `HandleScope`）；成本在 `state()` 沒有 inline、建 `State` 的冷路徑讓每次呼叫都設一個 3 KB 的 stack frame，`Rc` 的計數，讀回節點經過兩層沒有 inline 的呼叫和一次 FFI（V8 自己的標頭檔在 release 時 inline 讀那一格：`ReadCppHeapPointerField`）。Blink 生成的 callback 直接拿 `FunctionCallbackInfo`、inline 讀回 wrappable、沒有中間層。所以甲照這個順序做，每一項量讀取的頁（`nodeType`、`parentNode`、`nodeName`、`className`，只跑新的 DOM）：
   1. **`State` 借用**：`bindings` 的 isolate 的狀態只設一次（之後換掉是 panic），所以活到 isolate 拆掉，callback 裡借用它、不做 `Rc` 的計數；`state()` 的快路徑 inline，建立的路分開、`#[cold]`。
   2. **讀回節點 inline**：照 V8 標頭檔的 `ReadCppHeapPointerField` 在 `bindings` 裡讀 wrapper 的那一格和 isolate 的表（照乙2 的做法：常數照這版 V8 的標頭檔，debug build 每次也呼叫 patch 的 `kun::Unwrap`、比對）；receiver 由 V8 的 signature 檢查過，不再判斷是不是 API wrapper。
   3. **入口**：前兩項之後再取樣，入口還看得到時，`bindings` 給直接接 V8 的 `extern "C"` callback，不經過 rusty_v8 的 scope。
   不選 V8 的 fast API call（乙）：限制多（callback 裡不能讓 V8 配置或 GC），甲量完再看。
   （2026-10-27 補，量完）第 1 項留下（讀取的成員少 0.6–1.9 ns）；第 2 項量不出差別，拿掉；第 3 項不做：空的 getter 經過和不經過 rusty_v8 的 scope 都是 4.2 ns，是 kun 的 V8 呼叫 API callback 的地板，比 Chrome 整個 `nodeType`（3.5 ns）還慢，差距要從 V8 的 build 設定找（`progress.md` 的 1g 第 5 步）。

17. **實驗到 1g 的第 5 步結束，程式不 merge 回 `main`**（2026-10-27，使用者）。第 5 步之後剩下的差距，大半是 kun 的 V8 呼叫 API callback 的地板（`progress.md`）。還能試的兩個實驗都不做，留給下一份 RFC：
   - V8 的 fast API call：只對回傳基本型別、不配置的成員有用。
   - 用官方的設定（`is_official_build`、PGO）重編 V8，量地板。

   收尾的方式：
   1. **程式留在 `0088` 分支**（已 push 到 `origin`），不 merge 回 `main`。已決定的事項 6 的第 1 條寫的「V8 的 fork 定下來之前不能 merge」因此不再適用。下一份 RFC（0089）從 `main` 開，要用的部分（`gc` 的 JS 接法、V8 的 patch、binding 的形狀）重新寫成經過審查的步驟。
   2. **這個資料夾最後的樣子（全部的檔案）取代 `main` 上的草稿**，0089 引用它。只在分支上的路徑，到 `main` 上的版本改寫成「在 `0088` 分支上」。
   3. **rusty_v8 的 fork 的兩個 commit**（`510aec2`、`b4b619e`）留在 fork 的 `kun` 分支，`main` 的 `third_party.toml` 不釘它們。下一份 RFC 要不要用再決定（第 4 步量不出 `b4b619e` 單獨的效果）。
   4. **收尾只改文件**，不再編譯和跑測試（使用者）。

## 待決定

沒有。

## 之後的可能

給下一份 RFC（數字和出處在 `details.md` 的「實驗的結論」和 `progress.md`）：

- **呼叫 API callback 的地板**：kun 的 V8 裡一個空的 getter 要 4.2 ns，比 Chrome 整個 `nodeType`（3.5 ns）慢；JS 本身（迴圈、JS 的 getter）兩邊一樣快。可以試兩條路：
  - 用官方的設定重編 V8 量地板；
  - 回傳基本型別的成員用 V8 的 fast API call。
- **值的表示**：屬性值存成 `Arc<str>` 讓讀快（V8 字串的快取），但寫慢了（`setAttribute` 55 到 59 ns、寫 `className` 25 到 32 ns），因為從 V8 轉出的 `String` 要再配置、複製一次。值得照 Blink 的 `AtomicString` 重新設計，和 RFC 0085 的「來源 2」（屬性、class、字串放在 GC heap 上、共用不可變的資料）一起做。
- **建立**：`createTextNode` 還是 Chrome 的 2.2 倍，`createElement` 1.45 倍；成本在建 wrapper。
- **拿 wrapper**：`parentNode` 這類走訪比 `nodeType` 多約 2.4 ns，在讀節點的 JS 的邊、從 `TracedReference` 做 `Local`。
- **JS 文件的年輕節點也能被 wrap**（已決定的事項 10 的第 1 條的丙、丁）：1f 量完，甲在 innerHTML 這類頁沒有明顯變慢，所以不做；真實的頁面上 innerHTML 建的、沒被 wrap 的節點多到 major GC 變重時再看。
  - 丙：被 wrap 的年輕節點釘住它那一頁，scavenge 時整頁原地升到 old。
  - 丁：更徹底的做法，不搬動的新生代（sticky mark bits），再和 V8 的 MinorMS 一起收（Chrome 實驗中的 unified young generation）。
- **V8 的 scavenge 比 Chrome 的慢**（1f）：建 wrapper 的頁上 young 是 Chrome 的約 2.7 倍，JS 文件的 heap 沒有 nursery 時也一樣，所以是 V8 那一側；先拆開看是不是被丟掉的 wrapper 每個經過 FFI 的 `ResetRoot`。
- **create-element 最長的停頓 58 ms**（1f；Chrome 6 ms）：多半是 V8 的 major GC 收尾時新的 heap 還有一大段沒標完，先從 trace 確認。
- **V8 怎麼 fork**（已決定的事項 5）：這份的接法要改 V8，下一份 RFC 用它之前要定。
- **不穩定的測試**：`next_dom::a_wrapper_keeps_its_node_and_its_node_goes_with_it` 在壓力和負載下，V8 的背景編譯撐著 wrapper（`progress.md`）；寫同樣的測試時，在測試裡等背景編譯做完。
- **RFC 0087 留給這一段的**：哪些物件帶 `External`、`LocalExternal`（finalizer 的頁誰來掃）、熱門的型別要不要有自己的頁、ephemeron 和 weak 的容器（RFC 0087 的「分階段」第 7 列）。
