# RFC 0097 的進度：binding 每次呼叫的成本往 Chrome 靠

開始日期：2026-11-11

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型和量測（不合併） | 進行中 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| V8 在操作裡配置的三條規則在模型上成立，各拿掉一條有反例 | `python3 rfcs/0097-binding-call-cost/tla/check.py` |

## 做過的檢查

- 2026-11-11（Linux，AMD Ryzen AI 9 HX 370）：`tla/check.py` 在 2×2×1 上七個版本都照預期（`details.md` 的「模型」）；`inop` 在 2×2×2 上成立。

**第 1 段，原型 1：getter 不建 scope、不建 `V8Front` 和 `Call`**（2026-11-11，同一台機器）：

- `nodeType` 現在的 callback 反組譯（`perf annotate`，熱的路約 100 條指令；`bench.py calls` 的 153 條裡其餘是 V8 的 builtin 和 JS 的迴圈）：進出函式和存 6 個 callee-saved 暫存器約 17；建 `CallbackScope`、`V8Front`、`Call` 的寫入約 26；借狀態和一個全域的檢查約 5；unwrap（指標表、比 tag、cage 的檢查）約 22；marking 的檢查 2；`nodeType` 本身（讀種類、jump table）約 9；寫回傳值約 8；`Call` 和 scope 結束時的檢查約 12。
- 原型：`bindings` 的 `RawGetter`、`raw_getter_template`（`FunctionTemplate::builder_raw`，callback 直接拿 `FunctionCallbackInfo`，isolate 從 frame 的那一格讀），`dom_js` 在 `Node` 上多裝一個 `nodeTypeRaw`（借狀態、`read`、`unwrap_receiver`、`set_int32`；unwrap 的檢查照舊）。`bench.py calls --cases nodeType,nodeTypeRaw`：

  | | 指令數 | cycle |
  | --- | ---: | ---: |
  | `nodeType` | 151 | 30 |
  | `nodeTypeRaw` | 114 | 23 |

  省 37 條指令、7 個 cycle：machopo 自己的部分（扣掉 V8 的地板約 11 cycle）從約 19 到約 12 cycle，Chrome 約 6。剩下的在 unwrap 的檢查和進出函式。
- 找到的 bug：rusty_v8 的 `FunctionCallbackArguments::get_isolate` 把 V8 的 isolate 指標轉成 `*mut Isolate` 來用，而這個版本的 `Isolate` 是包著指標的結構（`repr(transparent)` 的 `NonNull<RealIsolate>`）：一用就 segfault。原型自己讀 frame 的格子。fork 裡沒有人呼叫它；不對上游回報，做外殼那一項時在 fork 給一個正確的入口。
- `firstChildRaw` 沒做：回傳 wrapper 的 `ReturnValue::set_traced` 拿 scope 當憑證（只當型別用），原型不動 fork。

**第 1 段，原型 2：unwrap 不檢查**（2026-11-11）：

- 現在 unwrap 一個 receiver：讀 wrapper 的 handle、查 isolate 的 `CppHeapPointerTable`（有指標壓縮的 V8 都要）、比型別的 tag（兩個值）和 null、elise 的 `from_engine` 檢查位址在 cage 裡、marking 進行中的話標記（`read_weak`，留著：模型的 unwrap barrier）。
- 原型：`nodeTypeRawUnchecked`，不比 tag、不檢查 null 和 cage（fork 的 `unwrap_receiver_unchecked`、elise 的 `from_engine_unchecked`，都只在本機、量完還原；三個 patch 在 `prototypes/unwrap-unchecked-*.patch`，machopo 的那份套在原型 1 的 commit 上）。`bench.py calls`：

  | | 指令數 | cycle |
  | --- | ---: | ---: |
  | `nodeType` | 155 | 30 |
  | `nodeTypeRaw`（原型 1） | 114 | 22 |
  | `nodeTypeRawUnchecked`（加上原型 2） | 89 | 19 |

  再省 25 條指令、3 個 cycle（RFC 0085 決定 14 估 1 到 2）。兩個原型合起來 machopo 自己的部分從約 19 到約 8 cycle，Chrome 約 6（整個 `nodeType` 約 17 cycle）。
- 2026-11-11 查證：Blink 照樣比 tag（`ToScriptWrappable` 給 V8 的 `Unwrap` 一個 tag 的範圍，`details.md` 的「Blink 的做法」）。所以這 3 個 cycle 裡，machopo 多的只有 null 和 cage 的檢查；比 tag 那一步兩邊都有（machopo 比兩個值，Blink 比範圍）。

**第 1 段，原型 3 之前：「V8 在操作裡配置」會省到哪裡**（2026-11-11，沒有改程式，只量現在的路）：

- **`createElement` 不會受惠**：它現在就在操作前照名稱猜好 interface、預先建 wrapper（`prepare_new_element`），操作裡直接 wrap，沒有 root、沒有第二個操作。放寬之後只是順序換了。
- 受惠的是「留著的結果」那條路（`V8Op::keep`、`finish_kept`）：回傳還沒有 wrapper 的節點、快取裡沒有的字串。
- **回傳還沒有 wrapper 的節點**：`innerHTML` 建 20 萬個 `<p>`，用 `nextSibling` 走一遍（`perf stat`，扣掉只建不走的）：每個節點約 1700 條指令、410 cycle（`nodeType` 是 153、30）。走第二遍一樣貴：V8 的 scavenge 丟掉沒人拿著、沒改過的 wrapper，每次走訪都重建。
- 走十遍的取樣（指令數的比例，含建樹的那一次）：V8 配置指標表的格子（`ExternalEntityTable::TryAllocateEntry`）13.5%、寫 wrapper 的指標欄位（`WriteLazilyInitializedCppHeapPointerField`）11.8%、複製 boilerplate（`CopyJSObjectWithAllocationSite`）5.5%；machopo 的 `V8Op::keep` 13.4%、`finish_kept` 6.1%、`Cx::root` 1.4%、`malloc` 1.3%、`keep_node` 0.7%（合計約 23%，每個節點約 390 條指令）；scavenge 重設 root 的那條路（`IsUnmodifiedApiObject`、`ResetIfSame`、`TracedHandles::Destroy`、`reset_root`、`try_reset_root`）約 9%。
- 所以放寬之後這條路最多省約 23%（root、留著、第二個操作），其餘是 V8 建 wrapper 和接上的成本，兩邊都要付。Speedometer 的取樣裡 `finish_kept` 合計 4.7 ms、`keep` 1.1 ms（每個 iteration，字串的也在裡面）：上限約 5 到 6 ms。
- 還沒做的：字串那條路單獨量；和 Chrome 在同一個走訪上比。

**第 1 段，字串：查證和現在的成本**（2026-11-11）：Blink 的做法和 machopo 現在各花在哪，寫在 `details.md` 的「Blink 的做法」和「字串的呼叫現在花在哪」。原型（external string、位址對名稱的快取）還沒做。

**第 1 段，原型 2 的修正：留著比 tag，只拿掉 cage 的檢查**（2026-11-11；elise 的 patch 套上、量完還原）：`nodeType` 155（32）、`nodeTypeRaw` 118（23）、`nodeTypeRawNoCage` 113（22）。只省 5 條指令、約 1 個 cycle。Blink 也比 tag，所以 unwrap 這一項照 Blink 的做法能拿的就是這 1 個 cycle；原型 2 另外的 20 條指令、2 個 cycle 是比 tag 和 null，Blink 也付。

**第 1 段，字串的原型 1：參數已經是字串就不轉換**（2026-11-11；`0097` 上的原型 commit）：

- 現在每個字串參數都呼叫 V8 的 `ToString`（先呼叫 `GetCurrentContext`，兩次 C++ 呼叫），參數本來就是字串時它只是原樣回傳。原型：`bindings` 的 `string_if_string` 照 V8 的 header 從 map 的 instance type 看它是不是字串，是就直接用（`MACHOPO_PROTOTYPE_NO_STRING_CHECK` 設了走原本的路，同一個 build 比）。
- `bench.py calls`，指令數（cycle），原本 → 原型：`getAttribute` 749（162）→ 692（158），`setAttribute`（兩個字串）1350（271）→ 1238（255），`createElement` 2148（449）→ 2091（446）。每個字串參數省約 57 條指令；cycle 省 3 到 8 個一個（雜訊比指令數大）。
- 還沒做：external string（進來時改成 external、記住對應的 DOM 字串；出去時不複製）。

**第 1 段，external string 做得到多少**（2026-11-11，只讀程式）：`details.md` 的「external string 在 machopo 的 V8 上做得到多少」。出去不複製不用改 fork；進來的短字串在沒有 sandbox 的 V8 上改不成 external（要 8 個字元以上），fork 也沒有接 `MakeExternal`。原型沒有做。

**第 1 段，`dispatchEvent` 拆開**（2026-11-11；`t.dispatchEvent(ev)`，目標不在樹上、一個空的 listener，兩百萬次的取樣，指令數的比例；每次 3497 條）：

| 部分 | 比例 |
| --- | ---: |
| DOM 的派發演算法（`event_target::dispatch` 20.3、`invoke_listeners` 10.6、`invoke_pass` 3.6、`event_current_target` 2.9、`snapshot_listeners` 2.5、`node_rare` 2.0、`event_path_at` 1.7、設和清 path、`listeners` 等約 2.5） | 約 46% |
| `dispatchEvent` 這個成員的 callback（外殼、front、被 inline 進來的） | 13.7% |
| machopo 呼叫 listener 的那一側（`call_listener` 13.7、`GetCreationContext` 2.5、`TracedReference::Get` 1.6） | 約 18% |
| V8 進 JS（`Function::Call`、`Invoke`、`JSEntry`、`SaveContext`、`CallDepthScope` 等） | 約 14% |
| 其他（`PerformanceEntry` 的 `HeapVector::push` 1.9、trace span 的 drop 1.2、`RootTable::remove` 1.1） | 約 4% |

- 結束和重開操作、登記 root 這些「操作不能巢狀」的成本 inline 在 `dispatch` 和 `invoke_listeners` 裡，這個取樣分不出來；看得到的 root 只有 `RootTable::remove` 1.1%。
- 最大的一塊是 DOM 的演算法本身（約 1600 條指令，目標不在樹上、只有一個 listener），不是 binding；`call_listener` 約 480 條。blink-perf 的 `events/EventsDispatching` machopo 慢 1.24 倍。
- 每次派發往 performance 的 entry 推一筆（`PerformanceEntry`）和一個 trace span，值得看是不是每次都需要。

**第 1 段，字串的原型 2：查名稱和值的表的上限**（2026-11-11；`prototypes/last-string-upper-bound.patch`，量完還原）：

- 先前只看取樣裡分得出來的函式（查表 1% 到 7%）就判斷「不值得做」，是錯的：雜湊和比對大多 inline 在成員的 callback 裡，取樣分不出來。補做原型。
- 名稱表是自己的完美雜湊（`names::find_fixed`），V8 的雜湊值不能直接套；值的表是 `FxHash` 的 `HashSet`（thread-local 的 `RefCell`）。原型量的是上限：`names::find` 和 `intern_value` 記住上一次查的文字的位址和長度，一樣就直接回上次的結果，不算雜湊、不比對（位址會被重用，一般不正確，只用來量）。`MACHOPO_PROTOTYPE_LAST_STRING` 開關，同一個 build：

  | | 原本 | 原型 | 差 |
  | --- | ---: | ---: | ---: |
  | `getAttribute`（1 個名稱） | 712（154） | 634（131） | −78 條、−23 cycle |
  | `setAttribute`（1 個名稱、1 個值） | 1281（265） | 1082（222） | −199 條、−43 cycle |
  | `createElement`（1 個名稱） | 2098（441） | 2011（409） | −87 條、−32 cycle |

- 每查一次名稱約 80 條指令、20 多個 cycle，查一次值約 120 條：比「不轉換」（57 條）還大。正確的做法能拿到其中多少要另外量：例如照位址和長度查一個小的快取、命中時比對一次字元（不算雜湊、不借 `RefCell`、不走雜湊表），或沿用 V8 的 internalized 字串存好的雜湊。

**第 1 段，`dispatchEvent` 裡「操作不能巢狀」的成本**（2026-11-11，讀 `dom_api` 的 `event_target.rs` 和 elise 的 `heap.rs`，沒有量）：

- 一次派發（目標不在樹上、一個 listener）開的操作：`dispatch_event` 1 個讀的、`dispatch` 1 個寫的（建 path，一個 `Vec`）、capture 和 bubble 兩趟各 1 個寫的（設 current target、拍 listener 的快照）、有 listener 的那一趟呼叫前後各 1 個讀的、收尾的 1 到 2 個，加上 `call_listener` 裡面拿 callback、事件和 target 的 wrapper 的：約 8 到 10 個。root：成員的 front 替 `this` 和事件各登記一個（`TargetRoot`、`Persistent<Event>`），每個操作裡再從 root 讀回來。
- 每開一個操作本身便宜：`read` 只建一個 `Mutation`；`mutate` 先 `safepoint()`，是 inline 的四個旗標的檢查（有工作才呼叫）；V8 這一側借狀態是一次讀取。成本在每個操作重新從 root 讀回物件、建 `V8Op` 和 `Cx`。
- 估計（沒有量）：每個操作 20 到 35 條指令，一次派發約 200 到 350 條，3497 條的 6% 到 10%；取樣裡看得到的只有 `RootTable::remove` 1.1%。要量準要做一個派發全程在一個操作裡的對照，那要先能在操作裡呼叫 script（不在這份的範圍）。

**第 1 段，字串的原型 3：最近查過的名稱和值的小快取（正確的版本）**（2026-11-11；`prototypes/recent-strings-cache.patch`，量完還原）：

- `names::find`：64 格、照文字的位址和長度選格子，每格存名稱的文字（23 bytes 以內）和編號，命中時比對字元（`name_eq`），不算雜湊、不查表；沒找到的名稱不存（之後可能有編號）。`intern_value`：64 格的 `DomString`，命中時比對文字、clone。`MACHOPO_PROTOTYPE_STRING_CACHE` 開關，同一個 build。開著時 `dom` 和 `dom_js` 的測試照樣通過。

  | | 原本 | 快取 | 差 | 上限（原型 2） |
  | --- | ---: | ---: | ---: | ---: |
  | `getAttribute` | 717（158） | 676（142） | −41 條、−16 cycle | −78 條 |
  | `setAttribute` | 1287（261） | 1148（236） | −139 條、−25 cycle | −199 條 |
  | `createElement` | 2105（483） | 2058（434） | −47 條 | −87 條 |

- 拿到上限的五到七成：每個名稱約 40 到 47 條指令，每個值約 95 條。cycle 的雜訊比指令數大（`createElement` 兩次的基準差 40 個 cycle），看指令數。
- 沒有量的：名稱很多、格子互相擠掉時（benchmark 只有一兩個名稱）；格子存 23 bytes 的文字讓每個 thread 多約 2 KiB。

