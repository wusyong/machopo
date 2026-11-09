# RFC 0088 的細節

`README.md` 的補充：

- **實驗的結論**：給下一份 RFC。
- **第 1 段量到的**（1e，2026-10-27）和過渡策略的比較：過渡策略在已決定的事項 11 之後不再決定，比較留著當參考。

數字都是 Apple M4、`perf` profile、接電源量的，出處和量法在 `progress.md` 的「做過的檢查」。寫到的程式（`dom_next`、`kunNextDom`、`gc` 的 JS 接法）只在 `0088` 分支上。

## 實驗的結論

### DOM 本身不是問題

- **改樹和寫入已經和 Chrome 一樣或更快**（每次呼叫，ns，新的 DOM / Chrome）：`appendChild` 30 / 59、`insertBefore` 47 / 69、`createElementNS` 89 / 182、寫 `textContent` 65 / 72、寫 `className` 32 / 32。
- **有效的資料的形狀**：一種 `Node`（樹的連結在每個節點上，走樹不看型別，Blink 的 `Node` 也是）；名稱 intern 成編號，表以字串為鍵、用快的雜湊；`Member` 的邊；屬性是另外一個物件。
- **要打折**：切片少做了事（「倍數裡有多少是少做了事」），改樹的頁上比 Chrome 快的有一部分是這個原因。

### GC：照 cppgc 在 Chrome 裡的方式和 V8 接

- **結果**（1f 之後，`progress.md` 的 1f）：
  - 建 wrapper 的三個 gc 頁，每次 iteration 的 GC 和 Chrome 差不多（gc-forest 10.9 對 12.8 ms），最長的停頓 21 ms（Chrome 14–33 ms）。
  - innerHTML-setter 的 GC 約 1%（Chrome 17%）：沒被 wrap 的節點在 nursery 裡收掉。
- **要留的**：
  - JS 文件的 heap 裡會被 wrap 的物件（節點）一出生就在 old、不搬，wrapper 直接存位址，不需要任何轉接的表。
  - 節點的欄位就是一格 droppable 的 V8 `TracedReference`，V8 的 scavenge 可以丟掉沒改過的 wrapper（Chrome 的預設 `reclaim_unmodified_wrappers`）。
  - 兩邊互交 worklist：V8 標到 wrapper 時直接把位址推給新的 heap，新的 heap 標到節點時在任何 thread 上直接交給 V8。
  - 新的 heap 的用量算進 V8 的 `EmbedderSizeOfObjects`，由 V8 開始、推進、收尾 major GC。新的 heap 照 V8 的 `IncrementalMarkingSchedule` 排程，平行的工作在 V8 的 platform 上。
  - 協定先寫成模型：兩種 barrier 混用（新的 heap 是 SATB、V8 是 Dijkstra），經過 wrapper 讀出的物件要標上，模型和原型都抓到過漏標。
  - 拆掉時先讓 V8 進行中的 GC 收尾，再刪掉 glue 的物件（Chrome 拆 `CppHeap` 的方式）。detach 而不刪的做法在負載下偶爾 segfault。
- **試過、拿掉的**：
  - cppgc 的 stub 加上以 id 為 key 的 wrapper 表和 weak handle（1b）：取樣裡一半的時間在表和 weak handle。
  - 讓節點會搬時要的 wrapper root 的表和 `TracedReference` 的表（1c）：1e 量到它們是 JS 文件的 GC 的大頭，scavenge 一再複製有 wrapper 的節點，V8 的停頓裡也要走這兩張表。
- **還沒解決的**：
  - V8 的 scavenge 比 Chrome 的慢約 2.7 倍；
  - create-element 最長的停頓 58 ms（Chrome 6 ms）。

### 每次呼叫：差距在 V8 和 Rust 的交界

每個成員一頁、迴圈裡呼叫 N 次，換算成每次呼叫的 ns（`progress.md` 的 1g）。「起點」是 1g 第 2 步第一次量到的新的 DOM；「最後」是第 4 步之後的，前四個成員是第 5 步之後的（第 5 步只量了這四個）。

| 成員 | Speedometer 佔比 | 今天的 DOM | 新的 DOM：起點 → 最後 | Chrome | 最後 / Chrome |
| --- | ---: | ---: | --- | ---: | ---: |
| `nodeType` | 8.4% | 9.2 | 17.2 → 6.1 | 3.5 | 1.74 |
| `parentNode` | 4.0% | 31 | 19.4 → 8.5 | 3.9 | 2.18 |
| `nodeName` | 1.8% | 36 | 35.6 → 8.0 | 7.3 | 1.10 |
| `className`（讀） | 2.2% | 33 | 35.4 → 10.8 | 6.6 | 1.64 |
| `nextSibling` | 2.2% | 31 | 19.2 → 9.2 | 4.1 | 2.25 |
| `firstChild` | 1.1% | 10.7 | 19.3 → 9.6 | 4.1 | 2.34 |
| `ownerDocument` | 2.3% | 34 | 19.2 → 9.9 | 4.1 | 2.43 |
| `tagName` | 1.4% | 31 | 35.1 → 10.1 | 7.0 | 1.44 |
| `getAttribute` | | 39 | 54.6 → 27.9 | 17.9 | 1.56 |
| `setAttribute` | 7.8% | 153 | 75.6 → 58.8 | 38.9 | 1.51 |
| `className`（寫） | （同上） | 161 | 41.5 → 31.7 | 31.7 | 1.00 |
| `textContent`（寫） | 1.0% | 337 | 79.6 → 64.6 | 72.4 | 0.89 |
| `createElement` | 3.2% | 360 | 129 → 71 | 49 | 1.45 |
| `createTextNode` | 1.4% | 317 | 129 → 80 | 37 | 2.16 |
| `createElementNS` | 1.0% | 366 | 145 → 89 | 182 | 0.49 |
| `appendChild` | 2.6% | 134 | 45 → 30 | 59 | 0.51 |
| `insertBefore` | 3.6% | 175 | 74 → 47 | 69 | 0.68 |
| `removeChild`（減掉 `appendChild`） | | 63 | 25 → 16 | 12 | 1.27 |

**照 Speedometer 的呼叫次數加權**：17 個成員，每個 suite 一次 iteration 的估計，20 個 suite 合計：

| | 合計 | 對 Chrome 的倍數（每個 suite 比值的幾何平均） |
| --- | ---: | ---: |
| 今天的 DOM | 24.5 ms | 4.3 |
| 新的 DOM，起點 | 12.7 ms | 2.1 |
| 新的 DOM，第 4 步之後 | 7.4 ms | 1.21 |
| Chrome | 6.6 ms | — |

第 5 步只讓讀取又快了一點，沒有重新加權。這些成員佔 Speedometer 呼叫的 45%；形狀相同、切片沒有的再 18%。

**每一項做法買到什麼**（照做的順序，每一項都照 Chrome 的做法）：

| 做法 | 效果 | Chromium |
| --- | --- | --- |
| receiver 由 V8 的 signature 檢查、只 unwrap 一次；getter 不進 GC 的安全點；per-isolate 狀態不每次查表；型別的編號 inline | 讀取每次少 7–8 ns（`nodeType` 17.2 → 9.7） | Blink 的成員帶 signature，`ToScriptWrappable` 一次；Oilpan 只在配置時 GC |
| rusty_v8 的 fork 照 V8 的標頭檔 inline 讀 `FunctionCallbackInfo`、寫 `ReturnValue` | 每次少 1.5–2.5 ns（今天的 DOM 也是） | Blink 的 binding 是 C++，這些在 V8 的標頭檔裡本來就 inline |
| 名稱的 V8 字串照編號快取；讀出的值經過留著的 scratch 字串；寫進的值不複製 | `nodeName` 35.6 → 22.5、`getAttribute` 49 → 35、`setAttribute` 71 → 58 | `StringCache`、`AtomicString` |
| 新的節點先建 wrapper、一次操作建好；instance template 不每次複製 `v8::Global`；名稱的表以字串為鍵、FxHash | `createElement` 129 → 81、`createTextNode` 129 → 91 | Blink 先建節點再建 wrapper（C++ 不怕中間 GC） |
| 狀態放進 isolate 的 data slot；讀出的值存成 `Arc<str>`，V8 字串照 `Arc` 的位址快取；新的 wrapper 是 boilerplate 的 `Clone` | `nodeName` 22 → 9.9、讀 `className` 19.5 → 12、`createElement` 79 → 71；但寫值變慢（`setAttribute` 55 → 59、寫 `className` 25 → 32：`String` 要再複製成 `Arc<str>`） | `V8PerIsolateData`、`StringCache` 以 `StringImpl` 為鍵、`CreateWrapperFromCache` |
| 狀態只設一次、callback 借用不計數；建它的路分開（不在每次呼叫設它的大 stack frame） | 讀取每次少 0.6–1.9 ns | `V8PerIsolateData` 活到 isolate 結束 |

**量不出效果、拿掉的**：

- 讀回節點不經過 FFI（照 V8 標頭檔的 `ReadCppHeapPointerField` 在 Rust 讀）：成本在一串互相依賴的記憶體讀取，Chrome 也要讀，不在那次呼叫。
- binding 直接接 V8 的 `extern "C"` callback，不經過 rusty_v8 的 scope：一個空的 getter 經過和不經過都是 4.2 ns。rusty_v8 的 callback 入口只有十來個指令。
- 狀態的 slot 存 `Rc<dyn Any>`：比 thread-local 的快取慢約 1 ns（多一層間接、`type_id` 是虛擬呼叫），改成存 `TypeId` 和瘦指標。

**地板**：kun 裡一個空的 getter（只回傳一個數字）要 4.2 ns，Chrome 整個 `nodeType` 只要 3.5 ns。

- 只跑迴圈、讀 JS 自己定義的 getter，kun 和 Chrome 都約 0.25 ns，所以 JIT 一樣快；開不開 trace 也一樣。
- 差在 V8 呼叫 API callback 的那一步。kun 的 V8 是一般的 release build，Chrome 的是官方 build（PGO、ThinLTO，builtin 照 profile 最佳化），還沒驗證是不是這個原因。
- `nodeType` 的 6.1 ns 裡，binding 和 DOM 只佔約 1.9 ns；`parentNode` 多出的 2.4 ns 在讀節點的 JS 的邊、做 `Local`。

### 給下一份 RFC 的設計

- **binding 的形狀照 Blink 生成的**：
  - 每個成員一個 callback，帶 signature；
  - receiver 只 unwrap 一次；
  - getter 不進 GC 的安全點；
  - per-isolate 狀態在 isolate 的 data slot、借用；
  - 名稱和值的 V8 字串快取；
  - 新的 wrapper 從 boilerplate 複製，先建 wrapper 再在一次操作裡建好節點。
- **跨語言的成本**：V8 在標頭檔裡 inline 的東西（`FunctionCallbackInfo`、`ReturnValue`、isolate 的 data），在 Rust 也要 inline 地做，省得到；V8 的 C++ 函式本身（`Unwrap`）不是成本。
- **值的表示**：要讀寫都快，值得照 `AtomicString` 重新設計：V8 字串和 Rust 之間不複製、共用不可變的資料。
- **地板**：先用官方的設定重編 V8 量地板，再看 fast API call 值不值得。

## 第 1 段量到的

Apple M4，`perf` profile，`bench.py blink-perf --next-dom`，每種 3 次。倍數是新的 DOM 比今天的 DOM 好幾倍（兩邊都在不在頁面上的 document 裡、都不 render）。

| 頁 | wasm | JS（1b） | JS（1e） | JS 今天 / Chrome | JS 新 / Chrome |
| --- | ---: | ---: | ---: | ---: | ---: |
| `bindings/create-element` | 2.20 | 0.72 | 2.16 | 0.14 | 0.30 |
| `bindings/gc-forest` | 2.10 | 0.58 | 1.75 | 0.18 | 0.31 |
| `bindings/gc-mini-tree` | 2.73 | 0.71 | 1.90 | 0.22 | 0.41 |
| `bindings/gc-tree` | 2.54 | 0.77 | 1.98 | 0.24 | 0.48 |
| `bindings/insert-before` | 1.95 | 3.51 | 3.35 | 0.43 | 1.45 |
| `parser/innerHTML-setter` | 1.20 | 1.18 | 1.19 | 0.20 | 0.24 |

- **新的 DOM 在兩種文件、六頁都比較快**。JS 版在 1b 變慢的四頁，換成真正的接法（1c）和排程（1d）之後從 0.58–0.77 倍變成 1.75–2.16 倍：1b 的取樣裡一半的時間在 wrapper 的表和 weak handle，1c 把兩者都拿掉了。
- **停頓變短**：JS 新 DOM 在四個建 wrapper 的頁最長的停頓 47–49 ms，今天的 DOM 是 71–460 ms（gc-tree 三次各自最長的 major GC 是 142、360、460 ms，cppgc 的 heap 在 V8 的停頓裡標完）。兩邊最長的都是 V8 的 major GC 的停頓。
- **GC 佔的比例反而高**（JS 新 DOM 26–32%，今天的 13–22%）：時間短了一半，GC 的時間差不多。分開看：
  - **scavenge 是大頭**：gc-forest 一次 3 秒左右的量測裡，新的 heap 的 scavenge 104 次共 553 ms（每次約 5 ms），V8 的 scavenge 19 次 284 ms。wasm 的新 DOM 同一頁 scavenge 916 次只要 0.46 ms：JS 的節點有 wrapper，wrapper root 在 scavenge 是 strong（已決定的事項 6 的第 8 條），每個節點都被複製、升到 old，V8 那一側的 wrapper 也留到 major GC。這是「V8 的 scavenge 丟掉沒改過的 wrapper」（「之後的可能」）要處理的。
  - **V8 的 major GC 停頓裡約一半是新的 heap 的**：gc-forest 9 次 major GC 共 199 ms，其中新的 heap 的收尾 100 ms、開始時的 scavenge 34 ms。收尾的 100 ms 幾乎都在 `Gc.Weak`：每次 11 ms 走過整張 wrapper root 的表（放掉沒交過來的）和 JS 的邊的表（清掉死的）。create-element 每次 17 ms。V8 的 `TracedHandles::ResetDeadNodes` 也在停頓裡走整張表，但只看一個 bit；我們的兩張表都可以改成只走這次變了的部分，或移到停頓之外。
  - **V8 每一步裡新的 heap 標的很少**（`Gc.Step` 一次量測合計 0.2–0.8 ms）：有 worker 時 worker 標完了，排程只在沒進展時補。
  - **其他 thread 的 GC 多了一倍以上**（gc-forest 0.6 秒到 1.2 秒，create-element 1.4 秒到 5.9 秒）：新的 heap 的 concurrent marking 和 sweep 在 V8 的 worker 上。
- **取樣**（gc-forest 和 create-element 的 JS 新 DOM，main thread 在 runner 裡的樣本，照 1b 的分類）：新的 GC 30.6% / 31.4%（其中 scavenge 17.6%）、binding 的 callback 16%、新的 DOM 的操作 10–11%、wrapper 物件（`NewInstance`）9.6%、新的 heap 在 V8 的 GC 裡 7–8.5%、字串 6–7%、V8 和 cppgc 自己的 GC 7–9%、`TracedTable` 和 wrapper root 4%、V8 的 `GlobalHandles` 3.4–3.7%。最後一項是每建一個 wrapper 從 isolate 的 slot 複製一次 template 的 `v8::Global`（`dom_js` 的 `template()`），和接法無關，改成存 `Local` 就沒有。

## 倍數裡有多少是少做了事

新的 DOM 只有最小的切片，每頁都少做今天的 DOM（和 Chrome）做的事。量不出各自多少，這裡列出來，判斷時打折：

| 頁 | 今天的 DOM 多做、新的 DOM 還沒有的 |
| --- | --- |
| `create-element` | 元素的種類（`HTMLDivElement` 等，`binding_kind`）和它的 prototype、custom element 的查表、node document、id 的表（`NodeId`）；wrapper 是那個介面的（新的只有一個 `NextNode`） |
| `gc-forest`、`gc-mini-tree`、`gc-tree` | 上面那些，加上插入時的 mutation record、live range 的更新、node list 和 collection 的失效、style 的失效旗標、`connectedCallback` 的排隊（detached 的 document 裡多半很快跳過，但每次都檢查） |
| `insert-before` | 同上的插入步驟。新的 DOM 比 Chrome 還快 1.45 倍，可見少做的部分在這頁佔得多 |
| `innerHTML-setter` | 兩邊大部分時間在 html5ever；今天的 DOM 的 sink 多建 template 的內容、註解和 doctype |

1a 用 wasm 量時的看法不變：wasm 的倍數裡 GC 的差最大（cppgc 的 incremental marking 的上萬個小步和 sweep 對 nursery 的 scavenge），少做的事主要影響插入的那幾頁。JS 的倍數同一個方向，大小和 wasm 接近，所以接上 V8 之後新的 GC 的好處沒有被 wrapper 和 V8 那一側吃掉。

## 過渡策略重新估（1e 時）

已決定的事項 11 之後不再決定過渡策略；這裡是 1e 時的比較，留著當參考。

`README.md` 的「理由和其他做法」列的四種，照第 1 段的結果重新估。共同的前提：

- 要改寫的量：`dom` 約 3.3 萬行、`dom_api` 約 6 萬行（`dom_api` 用 `dom` 的 handle 和 scope：`NodeHandle`、`DomReadScope`、`DomMutationScope` 等），engine 的 binding `dom_js` 約 8400 行、`dom_wasm` 約 5100 行；`style`、`layout`、`renderer` 有約 40 個檔案用 `dom`（`LayoutClient`、`NodeId`）。
- 現在 kun 的 `main` 只做這條 GC 和 DOM 的工作，其他功能本來就停著：「並排期間舊的 DOM 凍結新功能」的代價比寫 RFC 時小。
- JS 文件的接法已經做好（1c、1d），乙的前提（一開始就給兩個 engine 用）已經成立；只是 V8 是本機的 patch，怎麼 fork 還沒定（已決定的事項 5）。

### 甲：新的 DOM 先只給 wasm，JS 之後才換

- 好處：wasm 文件先拿到全部的好處（六頁 1.2–2.7 倍、GC 幾乎沒有停頓）。
- 代價：過渡期 JS 文件留在舊的 DOM，兩套 DOM 都要能跑整頁，`dom_api` 要複製一份或抽象成兩個 backend；新功能做兩次或兩邊都停。
- 第 1 段之後的評估：甲當初的理由是 JS 那一側的接法還沒有、要等第 9 段；現在已經有了，而且 JS 的倍數和 wasm 接近。分兩次換只剩代價。**不建議。**

### 乙：新的 DOM 一開始就給兩個 engine 用，並排開發，追上之後一起換

- 做法：新的 `dom`、`dom_api`（照新 GC 的規則）在新的 crate 裡長大，`dom_js`、`dom_wasm` 各有一份接新的 DOM 的 binding；同一組 WPT、情境和 reftest 在兩邊各跑一次（新的那邊開關打開），預期結果各自記；追上之後兩個 engine 一起換、刪掉舊的 DOM 和 cppgc 的 heap。
- 好處：只換一次；並排期間舊的 DOM 照常讓頁面跑，量測和測試隨時能比兩邊；第 1 段的入口（`kunNextDom`、`kun:next-dom`）和 binding 的寫法直接沿用。
- 代價：並排期間兩套 DOM 的程式都在 repo 裡；改寫 `dom_api` 是最大的工作（雖然大多是照搬邏輯、換掉 handle 和 scope）；style、layout 的介面要先在新的 DOM 上做出來，頁面才能真的跑在新的 DOM 上；一起換的那一刻風險集中（靠兩邊都跑同一組測試降低）。
- Chromium：Blink 把 DOM 搬到 Oilpan 是一個型別一個型別（丁），但那是 refcount 換 GC；V8 的 Ignition 和 TurboFan 換掉 Full-codegen 和 Crankshaft、Blink 的 LayoutNG 換掉舊的 layout，都是並排長大、用開關切、追上之後刪掉舊的，和乙一樣。
- **建議。**

### 丙：不重新設計，把今天的 `dom` 底層的 cppgc 換成 `gc`

- 好處：`dom_api` 幾乎不動，最快。
- 代價：新 GC 的規則（就地初始化、`Member`、操作裡不用 root 的 `Gc`）大多用不上，handle 的設計照舊（每次建 root、經過 scope）；違反 RFC 0087 的已決定的事項 2（目前的型別和限制不是前提）。第 1 段沒有量到這種做法：倍數裡有多少來自 GC、多少來自新的物件排法（一種 `Node`、名稱 intern、`Member`），分不開，但 1b 的取樣顯示 binding 和表的成本會吃掉一大半。
- **不建議。**

### 丁：一個型別一個型別搬，兩個 heap 同時存在

- 好處：每步都小，頁面一直跑在完整的 DOM 上。
- 代價：DOM 的型別幾乎都連到 `Node`（樹的連結、node document、collection、range、observer），搬一個就有大量跨 heap 的參照；跨 heap 的參照靠 root 收不掉循環，要讓 cppgc 和新的 heap 互相交 worklist（V8 的 patch 再多一個方向），並存的時間又最長。Blink 的 Oilpan 搬家能一個一個來，是因為 refcount 的物件可以被 GC 的物件持有、反過來也行；兩個 tracing GC 之間沒有這種便宜的邊界。
- **不建議。**

## 1e 時的建議：乙

照 2026-10-26 的看法，第 1 段的數字支持它：JS 和 wasm 兩邊的倍數接近，沒有理由分兩次換；JS 那一側的接法已經做好。第 2 段以後怎麼分段，等決定過渡策略之後再列，到時要一起決定的：

- V8 怎麼 fork（已決定的事項 5）：乙要一直用 kun 的 V8 patch，`0088` 要 merge 回 `main` 前先定。
- 新的 crate 的名字和位置（`dom_next` 長大，還是另開），`dom_api` 怎麼搬（照現在的模組一個一個搬、每個搬完在兩邊跑同一組測試）。
- 測試在兩邊跑的方式（WPT 的預期結果分開記、情境和 reftest 的開關）。
- style、layout、paint 的介面（`LayoutClient`、`NodeId`）和 parser 建整份文件，排在哪一段。
- 第 1 段量到、可以先做的優化：wrapper root 和 JS 的邊的表在停頓裡的清理（`Gc.Weak`）、`template()` 的 `v8::Global`、V8 的 scavenge 丟掉沒改過的 wrapper（「之後的可能」，今天的 DOM 和新的一起開再量）。
