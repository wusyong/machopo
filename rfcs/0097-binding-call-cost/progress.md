# RFC 0097 的進度：binding 每次呼叫的成本往 Chrome 靠

開始日期：2026-11-11

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型和量測（不合併） | 完成 | 2026-11-11 |
| 2 字串參數已經是字串就不轉換 | 完成 | 2026-11-11 |
| 3 front 只拿 callback 的 info：參數和結果單純的成員 | 完成 | 2026-11-11 |
| 4a 結果怎麼帶出操作：`RetType::Pending`、`finish`，front 定 `PendingString`、`PendingNode`、`PendingNumber` | 完成 | 2026-11-11 |
| 4b 新的 front 補上數字、布林、有預設值的參數，回傳不是節點的物件 | 完成 | 2026-11-11 |
| 4c 預先建 wrapper 的結果（`createElement`、`cloneNode`、`createTextNode`） | 完成 | 2026-11-12 |
| 4d handle 的機制（`Param` 多 front 的型別參數）、callback 和 `any`；函式拿 front 的成員照型別算 `PLAIN` | 完成 | 2026-11-12 |
| 4e rest、`any`、dictionary 的 handle、節點或字串；event handler 的 getter 和回傳 `any` 的（從 4f 提前）；其餘的參數型別留到 4j（決定 32） | 完成 | 2026-11-12 |
| 4f 其餘的回傳型別（promise、陣列、dictionary、第一次讀才做的值）：由回傳型別帶出操作（決定 33） | 完成 | 2026-11-12 |
| 4g static 成員和 overload 的外殼；目前的 realm 的 window 從 isolate 讀（決定 34） | 完成 | 2026-11-12 |
| 4h 之前：新的 front 補齊操作這一側的方法（`OpFront` 裡它沒有、成員的函式卻呼叫得到的 9 個：刪掉或實作，不再有會 abort 的預設實作；決定 35） | 完成 | 2026-11-12 |
| 4h constructor 的外殼（做到一半時停下來做上一列） | 完成 | 2026-11-12 |
| 4i-1 indexed getter 的 interceptor（`list[i]`、`children[i]`、`classList[i]`、`attributes[i]`；決定 36） | 完成 | 2026-11-12 |
| 4i-2 named 的 getter 和 setter 的 interceptor（`dataset.a`、`children.px`、`attributes.id`；決定 37）；空字串不做字串 | 完成 | 2026-11-12 |
| 4i-3 只被這次呼叫持有的字串不進字串的快取（決定 38） | 完成 | 2026-11-12 | |
| 4j-1 `KeptObject` 的編號和 `Uint8ClampedArg` 換成 handle（`view`、`relatedTarget`、`signal`、body 的 `Blob` 和 `URLSearchParams`、`ImageData` 的陣列）；`kept_object`、`kept_target`、`store_array` 拿掉；UI 事件的 constructor 和 `ImageData`（決定 39） | 完成 | 2026-11-12 |
| 4j-keys dictionary 的成員名稱是名稱表的固定名稱，key 從名稱的 V8 字串的快取拿（決定 40） | 完成 | 2026-11-12 |
| 4j-2 操作前讀成 Rust 值的參數型別（`fetch` 的 init、`postMessage`、keyframes…），連同卡在這些型別的 constructor、method、setter 和 static（決定 39） | 完成 | 2026-11-12 |
| 4k 研究：一次呼叫多出來的成本一類一類拆開、和 Chrome 對照、列每條冷的路該呼叫的 V8 API（決定 41、42） | 完成 | 2026-11-12 |
| 4k-A 回傳非整數的數字：直接設回傳值（決定 42） | 完成 | 2026-11-12 |
| 4k-B 回傳現算的字串：不經過 `DomString`、`Box`、`V8Front`；fork 加從 callback info 借的 token | 還沒開始 | |
| 4k-C 會呼叫 script 的成員：`this` 和參數不登記 root、少開操作 | 還沒開始 | |
| 4k-D 新的 wrapper：不登記 root、不經過 `V8Front` | 還沒開始 | |
| 4k-E 非 ASCII 和要轉換的字串參數 | 還沒開始 | |
| 4k-F dictionary、例外、陣列、promise | 還沒開始 | |
| 4k-G 最熱的路本身（`nodeType` 是 Chrome 的 1.36 倍）：另外研究 | 還沒開始 | |
| 4l cross-origin 的 16 個成員，和要找別的 realm 的 window 的 5 個（`Blob` 的 `arrayBuffer`、`bytes`、`text`，`Location.assign`，`Document.location` 的 setter）；4i 留下的 interceptor（indexed setter、iterator 的 pairs、named 的刪除、查詢和列舉）；legacy factory | 還沒開始 | |
| 4m-1 會呼叫 script 的成員走新的外殼（做法照 4k 的研究；決定 41） | 還沒開始 | |
| 4m-2 刪舊的外殼、`Call`、`Kept`、`PLAIN`（連同它的檢查：`PLAIN_FRONT_METHODS`、`PLAIN_OP_METHODS`、`dom_api` 的 `plain` 測試、`dom_js` 的 `the_plain_methods_are_the_info_front_ends_own`、`docs/test-gaps.md` 的那一列） | 還沒開始 | |
| 4m-3 刪掉 `V8Front`（4k-A 到 4k-F 之後沒有人用） | 還沒開始 | |
| 收尾：`nodeType`、`getAttribute` 的 callback 和 Chrome 的逐條指令對照，列出剩下的差（決定 20） | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| V8 在操作裡配置的三條規則在模型上成立，各拿掉一條有反例 | `python3 rfcs/0097-binding-call-cost/tla/check.py` |

| 成員的字串參數本來就是字串時不呼叫 `ToString`，其他的轉換一次、結果和例外照舊 | `cargo test -p dom_js --test it strings::`（`a_string_argument_is_converted_only_if_it_isnt_a_string`） |
| `PLAIN` 的成員當場回傳的呼叫不建 scope；要配置、丟例外的結果和原本的 front 一樣 | `cargo test -p dom_js --test it info_front`、`cargo test -p dom_api --test it table::members_of_strings` |

## 做過的檢查

- 2026-11-11（Linux，AMD Ryzen AI 9 HX 370）：`tla/check.py` 在 2×2×1 上七個版本都照預期（`details.md` 的「模型」）；`inop` 在 2×2×2 上成立。

**第 1 段，原型 1：getter 不建 scope、不建 `V8Front` 和 `Call`**（2026-11-11，同一台機器；`prototypes/raw-getter.patch`）：

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

**第 1 段，字串的原型 1：參數已經是字串就不轉換**（2026-11-11；`prototypes/string-already-a-string.patch`）：

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


**第 1 段，字串的原型 4：external string，進來和出去**（2026-11-11；`prototypes/external-strings.patch`，量完還原）：

- 不用重編 V8：`v8::String::MakeExternal`、`CanMakeExternal` 本來就在 V8 的靜態函式庫裡，fork 只是沒有包；原型在 `bindings` 直接連它們的 C++ 名稱。resource 借用 rusty_v8 建的 external 字串的那一個（兩個字串共用，isolate 不能 dispose，量測的測試跑完直接結束 process）：只能量，不能留。
- **進來**（`MACHOPO_PROTOTYPE_EXTERNALIZE`）：字串參數第一次看到、V8 肯改（`CanMakeExternal`：8 個字元以上、在 old space）時，把文字複製到一塊不釋放的 arena，前面放一個 header（名稱的編號、intern 過的值），再 `MakeExternal`；之後同一個字串進來，從 V8 的物件讀 resource、拿到文字的位址，`names::find` 和 `intern_value` 看位址在 arena 裡就讀 header，不讀字元、不雜湊、不查表（Blink 的 `StringResourceBase` 存 `AtomicString` 的做法）。指令數（cycle）：

  | | 原本 | 進來改成 external | 差 |
  | --- | ---: | ---: | ---: |
  | `getAttribute('data-long-name')` | 949（196） | 663（129） | −286 條、−67 cycle |
  | `setAttribute('data-long-name', 'a-long-value-here')` | 1624（297） | 1089（185） | −535 條、−112 cycle |
  | `createElement('blockquote')` | 2295（490） | 2069（414） | −226 條、−76 cycle |
  | `getAttribute('id')`（改不了） | 726（156） | 758（156） | +32 條 |
  | `setAttribute('a', 'b')`（改不了） | 1335（265） | 1399（270） | +64 條 |
  | `createElement('div')`（改不了） | 2130（443） | 2194（450） | +64 條 |

- 長的名稱省得比原型 2 的上限（每個名稱約 80 條）多：`data-long-name` 不是固定的名稱，原本要查兩張表（完美雜湊沒中、再查動態的表），字元也多；external 之後連字元都不讀（不掃 ASCII、不借出）。短的固定名稱開了 sandbox 之後能省的，照原型 2 的上限估（每個名稱約 80 條、每個值約 120 條），不是這張表的數字。
- 改不了的短字串每個多約 32 條：每次都問一次 `CanMakeExternal`（一次 C++ 呼叫）。要做的話先照長度擋掉（8 個字元以下不問）。
- **出去**（`MACHOPO_PROTOTYPE_EXTERNAL_OUT`）：ASCII 的 `DomString` 給 V8 時不複製，external 字串直接指著它的文字（`new_external_onebyte_raw`），多拿一份計數。**V8 在 GC 的 worker 上放掉 external 字串的緩衝區**（原型第一版在 destructor 裡檢查 thread，立刻 abort）：`DomString` 的計數不是 atomic 的，原型改成 destructor 只把位址登記到一個有鎖的清單，下次借出時在 DOM 的 thread 上放。

  | | 複製（現在） | external | 差 |
  | --- | ---: | ---: | ---: |
  | 文字節點的 `data` 每次是新的，讀出來，10 個字元 | 3888（2497） | 4297（3512） | +409 條、+1015 cycle |
  | 同上，100 個字元 | 4417（1324） | 5572（3831） | +1155 條、+2507 cycle |
  | 同上，1000 個字元 | 5234（1657） | 6881（3556） | +1647 條、+1899 cycle |
  | 同上，1000 個字元，讀出來再和另一個字串比（`!==`） | 6529（1817） | 13967（4660） | +7438 條、+2843 cycle |
  | 讀出來的值再傳回去（`setAttribute('a', v)`） | 1346（258） | 1238（230） | −108 條、−28 cycle |
  | `getAttribute('data-long-name')`（快取命中） | 949（196） | 949（196） | 0 |

- 出去不複製到 1000 個字元都比較慢：建 external 字串要配置 resource、登記、回報外部記憶體，死掉時還要回呼；複製 1000 bytes 比這些便宜。在 JS 裡讀它的字元（比較）也變慢約一倍。快取命中的路不變（本來就不建字串）。只有「讀出來的值原樣傳回去」省到（進來時直接讀 resource 的文字，不掃、不複製）。和先前的結論一樣（`dom::string` 的模組註解：借出比複製慢 22%）。cycle 含 GC 的 worker（`perf stat` 量整個 process）。
- **Chrome 實際拿到多少**（同一台機器的 Chromium 156，headless，`getAttribute` 兩千萬次的時間，三輪取後兩輪）：隨便取的字面值 1 到 8 個字元都是約 20 ns（`'qz'` 20.0 到 20.7、`'foo'` 20.1 到 20.4、`'qzwxecrv'` 20.7、`'data-long-name'` 19.7）：沒有長度的門檻，短的也改成 external 了。`'id'` 23.3 ns、`'title'` 25.0 ns 比較慢，執行時才組出來的長字串 30.7 ns。推測 `'id'`、`'title'` 是 V8 自己內建的名稱（在 read-only space，永遠改不了），每次都查 `AtomicStringTable`：和 2026-11-07 取樣 `getAttribute('id')` 看到的一致。所以「Chrome 開著 sandbox，短字串也改得了」是對的，但 benchmark 最常用的 `'id'` 剛好是例外；Chrome 在改得了的名稱上省約 3 到 5 ns（20 ns 裡）。`'class'` 17.6 ns 比其他都快，原因沒查。

**第 2 段：字串參數已經是字串就不轉換**（2026-11-11）：

- fork 的 `v8::machopo::as_string`（只改 Rust）；`dom_js` 的 `arg_string` 先問它，不是字串才呼叫 `ToString`，並在 isolate 的狀態記一次（`string_conversions`，測試用）。計數原本寫成 thread 的 `static`，`sanity.py boundary` 擋下來，改放進狀態。
- `bench.py calls`，指令數（cycle），改之前 → 之後：`getAttribute` 726（156）→ 688（151），`setAttribute` 1335（265）→ 1228（252），`createElement` 2130（443）→ 2087（444），`addEventListener` 898（183）→ 880（182）。每個字串參數約 40 到 55 條（原型量的是 57；兩次量之間指令數有約 10 條的差）。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-2.toml`，3 個都抓到。第一次「轉換丟了例外還繼續」沒抓到（例外還掛著，JS 照樣接到）：補了「成員沒有被呼叫」的斷言。fork 的 `as_string` 不在這個 repo、沒有 mutant：debug build 每次都和 V8 自己的 `is_string` 比（`debug_assert`），測試跑的是 debug build，測試裡有數字、`null`、`undefined`、物件、`String` 物件、symbol 和各種表示的字串。
- 跑過：`cargo test -p dom_js`（448 個通過）；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`test.py wpt dom/nodes/attributes.html dom/nodes/Document-createElement.html`（216 個結果都照預期）。
- 還沒做：fork 的 commit 和 push、`third_party.toml` 的 `rev`（等審核）；階段的最後跑過 `cargo test --workspace`（55 個 binary 都通過）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`。

**第 3 段開始前：一個 scope 值多少**（2026-11-11；量完還原）：`bindings` 的 `method_trampoline` 每次呼叫多建一個 `CallbackScope`（`v8::callback_scope!`，`black_box` 讓它不被消掉）。`bench.py calls`，指令數（cycle）：`nodeType` 155（30）→ 172（33），`firstChild` 176（33）→ 196（37）。一個 scope 約 17 到 20 條指令、3 到 4 個 cycle（`black_box` 逼它留在記憶體裡，可能略高估）：原型 1 省的 37 條、7 個 cycle 裡約一半。它做的事（rusty_v8 的 `src/scope.rs`）：從 callback 的 info 讀 isolate、從 isolate 的 data slot 讀 rusty_v8 自己的每個 isolate 的資料（annex）的位址，寫進一個 pin 在 stack 上的結構（isolate、annex、還沒查的 context、兩個旗標），結束時檢查兩個旗標。V8 不需要它：callback 的 handle scope 是 V8 自己在呼叫前後開關的。

**第 3a 段，第一版：scope 延後建**（2026-11-11；程式在工作目錄，還沒 commit、還沒決定留不留）：

- 做了：fork 加 `FunctionCallbackInfo::isolate`（frame 上放 isolate 指標的那一格）、`ReturnValue::set_traced` 改收 isolate、`function_template_with_cache_raw`；`bindings` 的 `function_trampoline`、`method_trampoline` 改成 V8 自己的 callback（`builder_raw`），`StateScope` 和 `NoGc` 裡放 `CallScope`（isolate 從 frame 讀；scope 第一次要的時候在 trampoline 的 frame 上建，`LazyScope`）；`NoGc` 的 unwrap、`return_wrapper`、`return_global`、`return_weak`、`ascii` 只用 isolate；`dom_js` 的 `arg_string`、`is_wrapper`、`report_embedder_used_size` 只用 isolate。`cargo test -p bindings -p dom_js` 通過。
- 建不建 scope（debug build 的計數）：`nodeType`、`firstChild`、`parentNode`、`tagName`、`id`、`getAttribute`、`hasAttribute`、`setAttribute`、`appendChild`、`id =` 都不建；`createElement`、`addEventListener`、`dispatchEvent`、`childNodes`、`textContent`（新的字串）還建。
- 同一個 script、兩個 binary 各跑三次取最小（`perf stat`，指令數（cycle））：

  | | 改之前 | 延後建 |
  | --- | ---: | ---: |
  | `nodeType` | 154（31） | 150（29） |
  | `firstChild` | 172（33） | 170（32） |
  | `getAttribute` | 661（148） | 708（153） |
  | `setAttribute` | 1188（257） | 1255（256） |
  | `appendChild` | 1317（221） | 1360（233） |
  | `createElement` | 2038（438） | 2127（446） |
  | `addEventListener` | 845（181） | 932（191） |
  | `dispatchEvent` | 3414（716） | 3547（722） |

- 沒參數的 getter 只省 2 到 4 條；有參數的成員反而多 40 到 130 條。讀 `nodeType` 的 callback 的機器碼：改之前進函式到讀狀態 35 條，其中 scope 的寫入約 8 條（isolate、annex、context、兩個旗標、pin 的參照）；改之後 31 條，scope 的 8 條換成延後建要的 6 條（`made`、info、`CallScope` 的三格）。所以 **scope 在原本的程式裡只值約 8 條指令**，先前多建一個量到的 17 到 20 條是 `black_box` 高估的；延後建自己也要記帳，只淨省 2 到 4 條。
- 有參數的成員變慢不是 scope：`state::report`（每個不是 getter 的呼叫之後）從 inline 變成一次函式呼叫，`arg_string` 從 82 行變成 108 行（多了經過 `CallScope` 讀 isolate、檢查有沒有 scope）。調得回來，但上限就是那 2 到 4 條。
- 原型 1 的 37 條不是 scope 省的，是整個不經過 `V8Front` 和 `Call`：那些結構（`Call` 的 7 個欄位、參數的包裝、`V8Front`）因為冷的路拿著它們的位址，編譯器只能每次都寫進記憶體。

**第 3 段的原型：front 只拿 callback 的 info**（2026-11-11；`prototypes/info-only-front.patch`，套在 `prototypes/lazy-scope-rusty_v8.patch` 的 fork 上，量完還原；3a 的第一版存成 `prototypes/lazy-scope-machopo.patch`、`lazy-scope-rusty_v8.patch`）：

- `bindings` 的 `StateInfo`（V8 的 `FunctionCallbackInfo` 和狀態的型別：isolate、receiver、參數、回傳值都從 info 讀，`with_scope` 當場建一個 scope、用完就丟）、`RawNoGc`（只拿 isolate 的 unwrap、`return_wrapper`、`return_weak`、`ascii`）、`raw_method_template`（`builder_raw`）。`dom_js` 的 `RawFront`：info 加一格「操作後才能做完的結果」（`Option<Kept>`，不是 box），實作 `Front` 和 `OpFront` 必要的 18 個方法；配置、丟例外、執行 script 的路（字串參數要轉換或複製、結果沒有快取或 wrapper、錯誤、custom element 的 reaction、回報用量）當場建 scope 和原本的 `V8Front`，交給它做，都在 inline 之外。**跑的是 `dom_api_macros` 產生的同一份成員的程式**（`M::call(&mut RawFront)`），DOM 的函式不變。在 `Node` 和 `Element` 上多裝 `nodeTypeRaw`、`firstChildRaw`、`appendChildRaw`、`getAttributeRaw`、`hasAttributeRaw`、`setAttributeRaw`，template 的設定和原本的一樣（signature、length、不是 constructor）。
- 行為對照過（臨時的測試）：一般的路、沒快取的字串、要轉換的參數、參數太少、`InvalidCharacterError`、`HierarchyRequestError`、不是節點的參數、沒有 wrapper 的節點（`innerHTML` 建的）、之後 full GC。
- 同一段 script，改之前的 binary 和有原型的 binary 各跑三次取最小（`perf stat`，指令數（cycle））：

  | | 現在（改之前的 binary） | 只拿 info 的 front | 差 |
  | --- | ---: | ---: | ---: |
  | `nodeType` | 153（31） | 118（23） | −35 條、−8 cycle |
  | `firstChild` | 172（33） | 134（27） | −38 條、−6 cycle |
  | `hasAttribute` | 571（137） | 505（125） | −66 條、−12 cycle |
  | `getAttribute` | 661（146） | 560（128） | −101 條、−18 cycle |
  | `setAttribute` | 1188（255） | 1095（222） | −93 條、−33 cycle |
  | `appendChild` | 1317（219） | 1234（203） | −83 條、−16 cycle |

- 有原型的 binary 裡原本的成員和改之前差在 10 條以內（`getAttribute` 680、其餘 ±10）：多裝的程式沒有讓原本的路變慢到影響比較。
- 有參數的成員省 66 到 101 條、12 到 33 個 cycle（8% 到 15% 的 cycle），比沒參數的 getter 還多：`Call` 的 7 個欄位、參數的包裝、scope、`V8Front`、操作後的檢查都不在了，而且沒有東西逼編譯器把它們寫進記憶體。
- 原型的「留著的結果」是一個 enum（所有種類共用），正式的設計照回傳型別各自一個型別，還少一個 tag。

**第 3 段的原型，和 Chrome 同一輪量**（2026-11-11；patch 多一個開關 `MACHOPO_PROTOTYPE_RAW`：設了，那 6 個成員的本名走 `RawFront`；同一個 binary 跑兩輪 `bench.py blink-perf --pages bindings/node-type.html,first-child.html,get-attribute.html,set-attribute.html,append-child.html`，machopo `perf` profile、Chrome for Testing 152.0.7977.82）。runs/s，括號是照頁面迴圈次數（10 萬，`set-attribute` 1 萬）換成的每次呼叫 ns，含 JS 的迴圈：

| 頁 | machopo 現在 | machopo 原型 | Chrome（兩輪） | 慢幾倍：現在 → 原型 |
| --- | ---: | ---: | ---: | ---: |
| `node-type` | 1538（6.5） | 1989（5.0） | 2852、2643（3.5、3.8） | 1.85 → 1.33 |
| `first-child` | 1424（7.0） | 1770（5.6） | 2374、2403（4.2） | 1.67 → 1.36 |
| `get-attribute` | 303（33.0） | 346（28.9） | 477、473（21.0） | 1.58 → 1.36 |
| `set-attribute` | 1626（61.5） | 1774（56.4） | 2296、2347（43.6、42.6） | 1.41 → 1.32 |
| `append-child` | 535 | 631 | 155、157 | 工具標成可疑（machopo 快兩倍以上），不比；machopo 自己快 18% |

- 每頁只跑一輪（runner 自己的幾次 iteration 的中位數），Chrome 兩輪之間 `node-type` 差 7%：看方向和量級，不看個位數的百分比。

**第 3 段：front 只拿 callback 的 info，參數和結果單純的成員**（2026-11-11）：

- fork（只改 Rust）：`FunctionCallbackInfo::isolate`、`ReturnValue::set_traced` 改收 isolate（`TracedSlot::get_in_current_scope`）；`README.machopo.md` 兩條。3a 第一版的 `function_template_with_cache_raw` 拿掉。
- `bindings`：`StateInfo`（info、狀態的型別；`isolate`、`args`、`receiver`、`state`、`state_mut`、`with_scope`）、`InfoNoGc`（只要 isolate 的 unwrap、`return_wrapper`、`return_global`、`return_weak`、`ascii`，和 `ret` 給 `OpReturn`）、`StateInfoMethod`、`state_info_method_template`（`builder_raw`，signature 放最後）；`return_wrapper` 收 isolate。`soundness.md` 的「只拿 callback 的 info 的 callback」。
- `dom_api`、`dom_api_macros`：`ArgType::PLAIN`、`RetType::PLAIN`（字串、節點的參數；數字、布林、字串、節點、null、沒有的結果）、`TypedMember::PLAIN`（產生器照參數、結果和 `front`、static、`realm`、legacy factory 算）。2799 個成員的路裡 1307 個是 `PLAIN`。`DomStr::give` 和兩個字串結果的 `give` 一律 inline（留在外面時 `getAttribute` 多約 40 條，`tagName`、`id`、`className` 多約 30 條）。
- `dom_js`：`info_front.rs` 的 `InfoFront`、`InfoOp`（info 加一格 `Pending`：不是 Smi 的數字、V8 沒有的字串、還沒建的名稱、沒有 wrapper 的節點；用 `ManuallyDrop`，結束時 inline 看一眼、有東西才到外面 drop）；冷的路 `with_front` 當場建 scope 和 `V8Front`（`finish_kept_as`、`string_arg`、`node_arg`、`has_args`、`throw`、`invoke_ce_reactions`）；`templates.rs` 的 `InfoMember<M>`，`Members::function` 在 `checked && const { M::PLAIN }` 時用它（跨 origin 的物件的成員照舊）。`state::report` 拆成 `moved` 和 `report_these`。
- 過程中量到、修掉的：`InfoFront` 的 drop glue 每次呼叫都是一次函式呼叫（改成 `ManuallyDrop`；先試過 `mem::forget(self)`，搬動整個結構，getter 從 23 變成 40 個 cycle）；`state::moved` 沒 inline；custom element 的 reaction 預設的 `invoke_ce_reactions` 把 front 當 script host 用，`InfoFront` 沒有那些方法（`custom_elements` 的測試抓到），改成整個交給 `V8Front`。
- 同一段 script，改之前的 binary 和這一段的各跑三次取最小（`perf stat`，指令數（cycle））：

  | | 改之前 | 第 3 段 |
  | --- | ---: | ---: |
  | `nodeType` | 156（31） | 116（24） |
  | `firstChild` | 172（34） | 139（28） |
  | `parentNode` | 172（33） | 139（31） |
  | `tagName` | 308（59） | 244（51） |
  | `id` | 318（64） | 252（59） |
  | `className` | 300（57） | 230（49） |
  | `hasAttribute` | 571（143） | 504（126） |
  | `getAttribute` | 661（152） | 566（135） |
  | `id =` | 967（195） | 898（186） |
  | `setAttribute` | 1188（253） | 1092（224） |
  | `appendChild` | 1317（219） | 1267（210） |
  | `textContent =` | 2078（452） | 2009（431） |
  | `createElement`（不是 `PLAIN`） | 2039（436） | 2055（470） |
  | `addEventListener`（不是 `PLAIN`） | 846（173） | 857（176） |
  | `dispatchEvent`（不是 `PLAIN`） | 3413（716） | 3424（712） |

  `PLAIN` 的成員省 33 到 95 條指令；不是 `PLAIN` 的在 20 條以內（兩次量之間的差；`createElement` 的 cycle 那一次偏高，指令數沒變）。
- 和 Chrome 同一輪（`bench.py blink-perf --pages`，machopo `perf` profile、Chrome for Testing 152.0.7977.82；runs/s；「之前」是同一天原型那一輪量的原本的路，`id-getter`、`id-setter` 是 2026-11-10 的）：

  | 頁 | machopo | Chrome | 慢幾倍 | 之前 |
  | --- | ---: | ---: | ---: | ---: |
  | `node-type` | 2010 | 2832 | 1.41 | 1.85 |
  | `first-child` | 1674 | 2386 | 1.43 | 1.67 |
  | `get-attribute` | 364 | 460 | 1.26 | 1.58 |
  | `set-attribute` | 1794 | 2150 | 1.20 | 1.41 |
  | `id-getter` | 896 | 1122 | 1.25 | 1.69 |
  | `id-setter` | 479 | 717 | 1.50 | 1.58 |

  `append-child` machopo 598、Chrome 156，照舊標成可疑、不比。每頁一輪，Chrome 自己各輪之間差到 7%。
- 測試：`dom_api` 的 `table::members_of_strings_nodes_numbers_and_booleans_are_plain`（哪些成員是 `PLAIN`、哪些不是）；`dom_js` 的 `info_front`（5 個：當場回傳的呼叫不建 scope；要配置或丟例外的建、結果一樣、V8 收掉之後重做；錯誤和 setter 的值；receiver 由 signature 檢查；不是 `PLAIN` 的不經過 info 的 callback）。計數是 `bindings::info_scopes_for_testing`（debug build）。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-3.toml`，26 個裡 23 個抓到。沒抓到的 3 個：跨 origin 的 property 的 `PLAIN` 成員不在 receiver 的 realm 呼叫（現在等價：`PLAIN` 的只有 `closed`、`length`，給布林和小的數字，不建 wrapper）；拿 front 的 setter 也算 `PLAIN`（現在等價：這種 setter 都收 event handler，不是 `PLAIN`）；負零當成零（現在沒有 `PLAIN` 的 getter 給得出負零：form control 的數字把 `"-0"` 讀成 0；這個檢查和 `V8Op::ret_f64` 的一樣）。第一輪另外沒抓到「失敗的呼叫留下的結果被做完」：成員的函式失敗時還沒給結果，那個 drop 走不到，改成 `debug_assert`、拿掉 mutant。fork 的兩個入口不在這個 repo：debug build 各自和 V8 的呼叫比對。
- 只靠審查的記進 `docs/test-gaps.md`：`PLAIN` 標錯的型別要到呼叫才 panic。
- 跑過：`cargo test --workspace`（55 個 binary 通過）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 七項（`boundary` 的允許清單加 `INFO_SCOPES`，測試的計數）、`test.py wpt dom/nodes`（15617 個結果都照預期）。
- fork 的 commit `097b54c`，`third_party.toml` 釘到它。fork 的兩個 commit（`2be356e`、`097b54c`）都還沒 push：merge 回 `main` 之前要先 push。

**第 2、3 段在 Speedometer 上**（2026-11-11，使用者要求；數字在 RFC 0012 的 `progress.md`）：`main`（加上這裡的 fork 的 pin，本地的 branch `0097-baseline`）對第 3 段做完的 `6db8168`，各 3 次：20 個 suite 的幾何平均 −0.9%，中位數的合計 984.2 → 976.1 ms（−8.1 ms）。17 個 suite 變快、3 個變慢，多數的 95% 區間重疊：方向一致，大小在雜訊的邊緣。微基準上每次呼叫省 8% 到 25% 的 cycle，在 Speedometer 上是不到 1%：它的時間大半在 JS、style、layout，binding 每次呼叫的固定成本只是其中一小塊，而且真的頁面打到的成員有一半還沒走新的 front。

**第 4a 段：結果怎麼帶出操作**（2026-11-11）：

- `dom_api`：`FrontTypes` 的 `PendingString`、`PendingNode`、`PendingNumber`；`OpFront` 的 `ret_string`、`ret_name`、`ret_node`、`ret_u32`、`ret_i32`、`ret_f64` 回傳 `Option<Self::Pending…>`（`#[must_use]`）；`Front` 的 `finish_string`、`finish_node`、`finish_number`（預設 `unreachable!`）；`Front::Op` 多三個相等的限制；`RetType` 的 `type Pending<F: FrontTypes>`、`give` 回傳它、`finish`。數字、字串、節點的回傳型別（和 `Option<HTMLCollectionOrElement>`、`Option<RadioNodeListOrElement>`、`ReaderResult`、`CanvasStyleValue`、`Hidden`）用 `pending!`，其餘 36 個用 `no_pending!`（`()`）。`ret_new_node` 的預設實作斷言沒有東西留下（留節點的 front 自己給新節點：4c）。
- `dom_api_macros`：getter、method、做完 script 才給結果的那種，把 `give` 的回傳值帶出操作、交給 `RetType::finish`，再呼叫 `Front::finish`（舊的 front 還用它，4g 拿掉）；`[CEReactions]` 的操作回傳 `(結果留下的, 有沒有留下 queue)`。node interface 的兩個回傳型別帶 `PendingNode`。
- `dom_js`：`InfoFront` 只剩 info（原本的一格 `Pending`、`ManuallyDrop` 和 `end` 拿掉）；`PendingString` 是它自己的 enum（任何字串，或一個名稱的：名稱的 V8 字串照編號存，不走字串的快取）、`PendingNode` 是 root 和 interface、`PendingNumber` 是 `f64`；`finish_string`、`finish_node`、`finish_number` 在 inline 之外交給 `V8Front`。`V8Front`、`V8Op`、named property 的 front，`dom_wasm` 的 front 和測試的 front 填 `Infallible`、回傳 `None`：照舊自己留著結果。
- 第 3 段和這一段的 binary 各跑三次取最小（`perf stat`，指令數（cycle））：`nodeType` 115（24）→ 107（20），`firstChild` 140（28）→ 127（22），`parentNode` 139（28）→ 124（23），`tagName` 244（49）→ 239（48），`id` 252（58）→ 246（53），`className` 231（47）→ 224（41），`hasAttribute` 504（124）→ 496（118），`getAttribute` 565（133）→ 559（126），`id =` 898（183）→ 890（183），`setAttribute` 1092（222）→ 1084（217），`appendChild` 1266（208）→ 1253（204），`textContent =` 2004（427）→ 1998（430）；不是 `PLAIN` 的 `createElement` 2055 → 2053、`addEventListener` 856 → 854、`dispatchEvent` 3424 → 3422。最簡單的 getter 整個約 20 個 cycle（Chrome 約 17），扣掉 V8 的地板 machopo 自己約 9、Chrome 約 6。
- 測試：`dom_js` 的 `info_front` 多一條（名稱的字串做過一次之後不再建 scope）；行為由原有的測試守著。mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4a.toml`，11 個都抓到。
- 跑過：`cargo test -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`、`cargo fmt --check`、`test.py wpt dom/nodes`。`cargo test --workspace` 和 clippy 等第 4 段全部做完。

**第 4b 段：數字、布林、有預設值的參數，不是節點的物件**（2026-11-11）：

- fork（只改 Rust，`5c327d3`，`third_party.toml` 釘到它；fork 的三個 commit 都還沒 push）：`v8::machopo::as_smi`（從 handle 讀小整數；debug build 和 `Value::is_int32`、`Int32::value` 比對），`README.machopo.md` 一條。
- `dom_api`：`FrontTypes::PendingObject`、`OpFront::ret_object` 回傳它、`Front::finish_object`；`give_object` 和產生器的 object interface 的兩個回傳型別帶它；`EventOrUndefined` 用 `pending!`；`Option<HTMLCollectionOrElement>`、`Option<RadioNodeListOrElement>` 的元素改成當物件給（`ObjectRef::Node`，一種 pending）；`CanvasStyleValue` 的 pending 是字串和物件的一對。`PLAIN` 加 26 個參數型別（`bool`、`Option<bool>`、各種整數和 `double`、`ByteString`、`Strings`、有預設值的字串、`OptionOrOptGroup`、`Slottables`、`CanvasImageSource`）、4 個回傳型別，和 object interface 的參數、回傳型別。照「`take`、`param`、`give` 裡 `front.`、`op.` 呼叫的方法都是 `InfoFront`、`InfoOp` 有的」掃過（第 3 段那次掃漏了帶 turbofish 的呼叫，例如 `front.object_if::<T>(…)`，這次補上；那次標的型別重看過，沒有受影響的）。`PLAIN` 的成員 1307 → 1726（2799 個裡，62%）；有 2 個是 indexed setter（表上寫成 method），不走 `Members::function`，`templates.rs` 的條件另外限定 getter、setter、method。
- `dom_js`：`InfoFront` 的 `arg_count`、`is_undefined`、`is_null_or_undefined`、`bool_arg`（`is_true`、`is_false`）、`i32_arg`、`u32_arg`（`as_smi`；負的照 `ToUint32` wrap）、`f64_arg`（`as_smi`，或 `Number::value`；不是 finite 而必須是時交給原本的 front 丟 `TypeError`）、`object_arg`、`target_arg`、`finish_object`；`InfoOp` 的 `object`、`target`、`ret_object`。不是數字、布林的值交給原本的 front 轉換。其他 front 的 `PendingObject` 填 `Infallible`。
- 4a 和這一段的 binary 各跑三次取最小（`perf stat`，指令數（cycle））：`childNodes` 299（64）→ 219（42），`classList` 269（60）→ 190（38），`list.item(0)` 416（89）→ 260（50），`event.target` 192（38）→ 150（28），`toggleAttribute('hidden', true)` 819（162）→ 737（141），`substringData(1, 3)` 3836 → 3688；沒變的：`list.length` 148、`contains` 338、`compareDocumentPosition` 1913（4a 就是 `PLAIN`），`nodeType` 109 → 111、`getAttribute` 562、`createElement` 2056 → 2055。`cloneNode(false)` 2273（476）→ 2280（516）：每次回傳沒有 wrapper 的新節點，走 `finish_node` 交給原本的 front，沒有省到（cycle 那一次偏高，沒有重跑）；4c 處理。
- **不穩定的測試**（找到、修掉）：`info_front` 的「當場回傳的呼叫不建 scope」有幾條靠 V8 還留著先前做的字串或 wrapper（`n.getAttribute('id')` 的值、`n.firstChild` 的 `<b>`、`n.classList`），但測試沒有拿著它們：V8 的 scavenge 會丟掉沒人拿著的字串和沒改過的 wrapper，下一次讀就重做、建 scope。測試多跑幾個呼叫之後 scavenge 剛好落在中間，`n.firstChild.tagName` 那一條失敗一次；之後三次又都通過。改成測試把每個結果留在 JS 的陣列或變數裡。修完 `info_front` 的 7 個測試重複跑 1400 次（400 加 1000），都通過。
- 測試：`dom_js` 的 `info_front` 多兩個（數字、布林、物件當場讀的不建 scope；要轉換或要建 wrapper 的建、結果一樣：字串和有 `valueOf` 的物件當數字、小數截掉、超過 32 位元的 wrap、各種值當布林、型別不對的物件和 null 是 `TypeError`、`valueOf` 丟的例外、`double` 不是 finite 的 `TypeError`、多個字串參數）；`dom_api` 的 `table` 的清單更正（第 3 段把 `cloneNode` 列成「預先建 wrapper」，它不是：那是 `createElement`、`createTextNode`）。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4b.toml`，14 個都抓到。沒有 mutant 的：`target_arg`、`is_null_or_undefined`（現在 `PLAIN` 的成員裡沒有測試呼叫到的；和原本的 front 的寫法一樣）、`templates.rs` 限定 kind 的條件（現在等價）。
- 跑過：`cargo test -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt dom html/dom`（123056 個結果都照預期）。

**第 4c 段：操作前準備的 wrapper 是成員的區域變數**（2026-11-12）：

- fork（只改 Rust，`b70c0fa`，`third_party.toml` 釘到它；fork 的四個 commit 都還沒 push）：`TracedSlot::set_droppable` 改收 isolate（不建 `Local`），`README.machopo.md` 一條；用不到的 `GetIsolate` import 拿掉。
- `bindings`：`set_wrapper` 收 isolate；`InfoNoGc` 的 `wrap`、`set_wrapper`；`soundness.md` 補上。
- `dom_api`：`FrontTypes::PreparedNode`（`Default`）；`RetType` 的 `type Prepared<F>`、`prepare(front, foreseen)` 回傳它、`give_prepared`（預設丟掉它、呼叫 `give`）；`no_pending!`、`pending!` 帶 `no_prepared!`（`()`）；`ArgType::foreseen_interface`（預設 `None`）；`Front::prepare_new_node` 回傳 `PreparedNode`、`prepare_node_like_this`、`foresee_element`（取代 `prepare_new_element`：只看名稱、不建東西）；`OpFront::ret_new_node` 多收 `PreparedNode`、回傳 `Option<PendingNode>`，`ret_construct` 回傳 `Option<PendingNode>`。`NewObject`、`NewElement`、`NewHtmlElement` 的 `Prepared` 是 `F::PreparedNode`、`Pending` 是 `Option<F::PendingNode>`，都標 `PLAIN`；`ElementLocalName` 的 `Taken` 多帶猜到的 interface，標 `PLAIN`；新的回傳型別 `NewNodeLikeThis`，`cloneNode` 用它。兩個 promise 的型別照舊在 `prepare` 呼叫 `prepare_promise`（還在原本的 front 的那一格）。
- `dom_api_macros`：getter 和 method 把 `prepare` 的回傳值帶進操作給 `give_prepared`；method 先把各參數的 `foreseen_interface` 用 `or` 接起來（沒有這種參數的編出來是 `None`）。
- `dom_js`：`InfoFront` 的 `PreparedNode` 是 `Option<(wrapper, interface)>`，`prepare_new_node`（這時建 scope；別的 realm 的物件上呼叫時 `None`）、`prepare_node_like_this`（先開一個讀的操作看 `this` 的 interface）、`foresee_element`（和 `V8Front` 共用 `front::foresee_element`，只用 isolate）；`InfoOp::ret_new_node` interface 對得上就 `wrap`、`set_wrapper`、寫回傳值，對不上照沒有 wrapper 的節點；`PendingNode` 改成 enum（沒有 wrapper 的節點，或要建構的 custom element），`ret_construct` 回傳它。`V8Front` 照舊把 wrapper 放在 `Call`（它的 `PreparedNode` 是 `()`）。測試用的計數 `wrappers_ahead`（預先建的 wrapper 成了節點的 wrapper 的次數，兩個 front 都數）。`dom_wasm` 的 front：`NewNodeLikeThis` 的 `WitRet`。
- `PLAIN` 的成員又多 `createElement`、`createElementNS`、`createTextNode`、`createComment`、`createDocumentFragment`、`createAttribute`、`splitText`、`cloneNode`、`extractContents`、`cloneContents` 等。
- 4b 和這一段的 binary 各跑三次取最小（`perf stat`，指令數（cycle））：`cloneNode(false)` 2279（509）→ 1897（384），`cloneNode(true)`（有一個子節點）4017（955）→ 3615（793），`createTextNode` 2301（536）→ 2223（518），`createDocumentFragment` 1487（317）→ 1448（301），`createElement('div')` 2056（435）→ 1988（449），`createElement('x-y')` 3069（645）→ 3004（651）；沒變的：`nodeType` 109 → 113、`getAttribute` 561 → 562、`childNodes` 218 → 219、`appendChild` 1488 → 1509（兩次量之間的差）。`cloneNode` 從三步（root、操作後建 wrapper、再一個操作）變成一步；`createElement` 本來就預先建，只省了 `V8Front` 和 `Call`，cycle 沒有跟著降（沒有重跑）。
- 測試：`dom_js` 的 `info_front` 多兩個（建節點的成員各用上預先建的 wrapper、沒有浪費的：`createElement` 三種名稱、`createTextNode`、`createComment`、`createDocumentFragment`、`splitText`、`cloneNode` 對元素、文字、fragment、document；interface 猜錯的（大寫的名稱、XML 文件、`createElementNS`）浪費一個、結果照樣對，custom element 操作後建構）；`dom_api` 的 `table` 的清單（`createElement` 等現在是 `PLAIN`）。`info_front` 的 9 個測試重複跑 1000 次都通過。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4c.toml`，11 個都抓到。
- 跑過：`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt dom html/dom custom-elements`（126618 個結果都照預期）。
- 看到、沒有動的：window 的 document 的 `cloneNode()` 給的是 `Document`、不是 `HTMLDocument`（改之前就是；沒有查 Chrome）。

**第 4d 段：callback 是 front 的 handle；函式拿 front 的成員照型別算 `PLAIN`**（2026-11-12）：

- fork（只改 Rust，還沒 commit）：`TracedSlot::set` 改收 isolate、`TracedSlot::refers_to`（邊指的物件是不是某個 handle 的，讀位址比），`README.machopo.md` 那一條一起寫；用不到的 import 拿掉。
- `dom_api`：`FrontTypes::Callback`（`Copy`）；`ArgType::Param` 多 front 的型別參數（`Param<'a, 'gc, T: FrontTypes>`，74 個實作加產生器的 6 個跟著改簽名）；編號 `CallbackArg` 換成 `Callback<H>`（包著 front 的 handle，照樣能 `Root`）；`Front` 的 `callback_arg`、`function_arg`、`nullable_function_arg`、`event_handler_arg`、`timer_handler_arg` 回傳 `Callback<Self::Callback>`，`OpFront` 的 `same_callback`、`store_callback`、`callback_number` 收它；14 個模組裡收 callback 的函式的參數改成 `Callback<O::Callback>`（`define` 是 script host 的，對 handle 的型別泛型）。11 個 callback 的參數型別、產生器的 callback interface、`EventHandler` 的參數標 `PLAIN`。`DictMember::PLAIN`（布林、數字、字串、字串清單的成員）、`Dictionary::PLAIN`（成員都是），dictionary 和 `DictOrBool` 照它標。`Rethrown`（`throwIfAborted` 的回傳）、`NewImageData`（`createImageData` 兩個、`getImageData`）：和 `()`、`ImageData` 一樣，但不是 `PLAIN`。
- `dom_api_macros`：`PLAIN` 不再看函式拿不拿 front。
- `bindings`：`set_strong`、`set_strong_value` 收 isolate，`refers_to`；`InfoNoGc` 的 `set_strong`、`set_strong_value`、`refers_to`；`soundness.md` 補上。
- `dom_js`：`InfoFront` 的 `Callback` 是 `Local<Value>`（參數本身），五個拿 callback 的方法只看它是不是物件或函式（timer 的字串在這時 `ToString`，建 scope）；`InfoOp` 的 `same_callback`（`refers_to`）、`store_callback`、`callback_number`；`is_dictionary_like`、`dict_arg`（沒給是 `None`）、六個讀成員的方法交給 `V8Front`。`V8Front`、測試的 front、`dom_wasm` 的 front 的 `Callback` 是它們原本的編號（`usize`），named property 的是 `Infallible`。
- 計畫裡這一段沒做的：`any` 的值（也從 dictionary 的成員來，和 dictionary 一起在 4e）。`addEventListener` 的 options 有 `signal`（一個物件），還不是 `PLAIN`：4e。event handler 屬性的 getter 會編譯屬性的原始碼（script），本來就不是 `PLAIN`。
- 找到、修掉的：拿掉「函式拿 front 的不是 `PLAIN`」之後，`getImageData` 因為 `ImageDataSettings` 變成 `PLAIN` 的 dictionary 而被放行，它的函式呼叫 `set_image_data_pixels`，`dom_js` 的 `canvas::images_and_origin_clean` 在預設實作的 `unreachable!` abort：換成 `NewImageData`。之後 `cargo test --workspace`（55 個 binary）和 `test.py wpt dom html/dom html/webappapis custom-elements html/semantics/forms html/canvas/element/pixel-manipulation`（131588 個結果）沒有再出現。
- 寫了又拿掉的：`InfoOp` 的 `ret_any`、`ret_any_or_null`、`ret_any_or_undefined`、`with_array_bytes`，`InfoNoGc::strong_value` 和 fork 的 `TracedSlot::get_unscoped`：這一段沒有 `PLAIN` 的成員走得到（event handler 的 getter 不是 `PLAIN`、`putImageData` 是 overload），沒有測試守得住；用得到的那一段再加。
- 4c 和這一段的 binary 各跑三次取最小（`perf stat`，指令數（cycle））：`t.onclick = f` 566（112）→ 500（96），`t.onclick = null` 368（78）→ 306（58），`removeEventListener('y', f)` 767（155）→ 590（126），`removeEventListener('y', f, { capture: true })` 1679（319）→ 1610（355）（給了 dictionary：建 scope、交給 `V8Front` 讀成員；cycle 那一次偏高，沒有重跑）；沒變的：`nodeType` 109、`getAttribute` 562 → 561、`addEventListener` 857 → 862、`dispatchEvent` 3425、`createElement` 1987 → 1989、`appendChild` 1508 → 1509。
- 測試：`dom_js` 的 `handlers::a_callback_is_its_arguments_handle`（event handler 設值、取代、不是物件的是 null；移除的是 callback 相同、capture 相同的那個，不是同類型的第一個；options dictionary 的成員讀得到；不是 callback 的 `TypeError`；給物件的呼叫不建 scope）；`dom_api` 的 `table` 的清單（`removeEventListener`、`requestAnimationFrame`、`onclick` 的 setter 是 `PLAIN`；`addEventListener`、`setTimeout`、`onclick` 的 getter、`throwIfAborted`、`getImageData` 不是）。先在 `Env::window` 上寫這個測試，handler 讀回來是 null：那個環境沒有瀏覽的 context，handler 不運作（原本的 front 也一樣），不是新的 front 的問題；改用 `handlers` 的 `page()`。`info_front` 和這個測試重複跑 500 次都通過。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4d.toml`，12 個都抓到。
- `PLAIN` 的檢查（決定 31）：`dom_api` 的 `plain::a_plain_type_calls_only_the_plain_methods`（72 個標成 `PLAIN` 的型別、`ObjectType`、共用的函式、macro 產生的型別）、`plain::a_plain_members_function_calls_only_the_plain_methods`（2152 個 `PLAIN` 的成員裡 2087 個的函式對得到、查過；測試要求至少九成）、`plain::the_functions_that_keep_something_are_seen`（已知會留東西的四個函式掃得出來），`dom_js` 的 `info_front::the_plain_methods_are_the_info_front_ends_own`。第一次跑就失敗在 `putImageData`（兩個 overload 都是 `PLAIN`、函式呼叫 `with_array_bytes`）：參數換成 `ReadImageData`。mutation test：`mutants-4d-plain.toml`，7 個都抓到（`putImageData` 換回 `ImageData`、`ReadImageData` 標成 `PLAIN`、`Rest` 和 `Any` 標成 `PLAIN`、`throwIfAborted` 不回傳 `Rethrown`、`rethrow` 放進清單、`InfoOp` 少一個方法）；第七個第一次寫成改名字，編不過，改成刪掉方法。
- 跑過：`cargo test --workspace`（提前跑，為了找被誤放行的成員）、`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、上面那組 WPT。

**第 4e 段：`any`、rest 和 dictionary 的物件是 front 的 handle；節點或字串；event handler 的 getter 一個操作**（2026-11-12，決定 32）：

- `dom_api`：`FrontTypes::Any`、`Rest`（`Copy`；`Front::Op` 的和 front 的相同）；`AnyArg<A>`、`SignalObject<N>`，拿掉 `RestArg`；`Front` 的 `any_arg`、`abort_reason_arg`、`dict_any`、`rest_arg`、`dict_object` 回傳 handle，`OpFront` 的 `store_any`、`store_rest` 收 handle；`DictHandle<F>`（成員是 front 的 handle 的 dictionary 成員）；`Dictionary::Of<T>`（讀出來的型別）；`ArgType::Param` 和 `param` 的 front 多 `+ 'a`（每個實作跟著加）。`EventHandler` 的結果是 `HandlerValue<'gc>`（`None`、`Listener`、`Uncompiled`），`Pending` 是 target 的 root 和 type，`Front::finish_handler`（預設：編譯、再開一個讀的操作給結果）。四個 handler 的 getter 函式改成一般的 getter（`cx` 和 `this`），表上 393 個 attribute 拿掉 `#[calls_script]`。`Any`、`AnyOrNull`、`AnyOrUndefined` 的結果，`NodesOrStrings`、`NullableElementOrLong`、`Any`、`AbortReason`、`Rest` 的參數標成 `PLAIN`。`TargetObject::this` 改成 `#[inline(always)]`（下面）。
- `dom_api_macros`：dictionary 的成員型別寫了 `T::Node`、`T::Any` 的，struct 帶那幾個型別的參數（`T::Node` 變成 `TNode`，預設 `Infallible`），成員用 `DictHandle<T>` 讀，不產生當結果的 `RetType`；`Dictionary::PLAIN` 只看其餘的成員。
- `bindings`：`InfoNoGc::return_value`（邊當成任何值的 slot，從 traced handle 寫進回傳值），`soundness.md` 補上。
- `dom_js`：`V8Front` 的 `Any`、`Rest` 是 `Local`，`dict_object` 給 wrapper 本身，`Call` 拿掉 `rest`、`any` 不再佔 `callback` 那一格。`InfoFront`：`node_or_string_arg`、`node_or_long_arg`（從 info 讀）、`any_arg`、`abort_reason_arg`、`rest_arg`（沒有多的參數不建 scope）、`dict_any`、`dict_object`、`compile_event_handler`（交給 `V8Front`）；`i32_arg`、`u32_arg` 對 undefined 直接給 0（`setTimeout(f)`、`insertRule(rule)` 原本為了 `ToInt32` 建 scope）。`InfoOp`：`store_any`、`store_rest`、`ret_any`、`ret_any_or_null`、`ret_any_or_undefined`。
- `dom_wasm`、`dom_api` 的測試用 front：新的關聯型別（`Infallible`；測試的 rest 是個數）。
- 和事先說的不同：dictionary 的參數不是 front 的型別而是 handle 的型別（決定 32）。`AbortController.abort()`、`AbortSignal.abort()` 的參數型別是 `PLAIN` 了，但成員本身不是（前者之後要跑 abort 的步驟，後者是 static）：`InfoFront::abort_reason_arg` 現在走不到，沒有測試，4g 接上 static 時才有。
- `PLAIN` 的成員：能走新的 front 的 2745 個裡 2152 → 2581（getter 1356、setter 822、method 403）。
- 找到、修掉的（量測時）：第一次量 `removeEventListener` 比 4d 多 34 條指令（595 → 629，連 callback 是 null、馬上回傳的也多 24）。原因是 `TargetObject::this`（receiver 是節點、window 還是其他 target 的三次比較）：4d 時新的 front 上只有 `removeEventListener` 一個成員用它、被 inline，4e 多了 `addEventListener` 之後 LLVM 不再 inline。改成 `#[inline(always)]`，回到 595。取樣的 profiler 在這裡沒用（`perf record` 的樣本和迴圈對齊，都落在同一條指令上）；是比 `removeEventListener('y', f)`、`('y', f, false)`、`('y', null)` 三種寫法和不相干的成員的 `perf stat` 找到的。另一個教訓：每個 binary 的空迴圈基準各量一次，三次取最小時基準自己有 ±10 條的誤差，整組一起偏；下面的數字改成七次取最小，沒動到的成員兩邊一樣。
- 4d 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`x = t.onclick`（有 handler）808（178）→ 280（50），`x = t.oninput`（沒有）729（162）→ 267（49），`x = ce.detail` 192（41）→ 133（24），`ce.initCustomEvent('c', false, false, f)` 828（196）→ 731（173），`t.addEventListener('x', f)`（已經有同一個 listener）861（176）→ 634（128），`n.append(c)` 2007（403）→ 1955（391），`n.append(c, tx)` 4682（844）→ 4640（843）；沒變的：`nodeType` 109、`getAttribute` 561、`t.onclick = f` 500、`removeEventListener` 605、`removeEventListener` 給 options 1626、`createElement` 1989 → 1992、`appendChild` 1579、`dispatchEvent` 3473 → 3446。`append` 省得少：它的成本在 DOM 的插入（和 `appendChild` 的 1579 比），每次呼叫還配置一個 `Vec`（Blink 也配置一個 `HeapVector`）。timer 沒量（`bench.py calls` 的頁面沒有 window）。
- 測試：`dom_js` 的 `info_front::nodes_or_strings_are_taken_as_they_are`（節點不建 scope；字串、要轉換的值照參數順序；轉換丟的例外；`select.add` 的元素、小整數、其他數字）、`handlers::a_value_rest_arguments_and_a_signal_are_handles`（listener 沒給 options、給布林不建 scope；dictionary 的 `once`、`capture`、`signal`（abort 時移除、已經 abort 的不加、不是 `AbortSignal` 和 getter 丟例外）；`detail` 存進去和讀回來、沒有時是 null；`signal.reason`、`customElements.get` 的 undefined；省略的整數參數；timer 沒有多的參數不建 scope、有的照樣傳給 callback）、`handlers::a_handlers_getter_gives_its_callback_in_the_operation`（有 handler、沒有、window 和 document 的、`<body>` 反映 window 的都不建 scope；content attribute 的原始碼第一次編譯、之後是同一個函式；編不過的是 null；屬性重設後重新編譯）；`dom_api` 的 `table` 的清單。寫測試時弄錯的預期：`abort()`、`new CustomEvent()`、`selection.collapse()` 都不走新的 front（之後要跑 script、constructor、`#[then_script]`），不會有它的 scope；字串的 timer handler 本來就建 scope。第一次呼叫 `addEventListener` 的那個 scope 是回報 DOM 的 heap 長大，不是成員自己的：測試先呼叫一次再數。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4e.toml`，26 個都抓到。第一次 25 個：「省略的 unsigned 整數交給整個 front」沒抓到，因為測試用的 `selection.collapse(d)` 不走新的 front；換成 `sheet.insertRule('b {}')` 之後抓到。
- 跑過：`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt dom html/dom html/webappapis custom-elements html/semantics/forms html/canvas/element/pixel-manipulation`（131588 個結果都照預期，和 4d 同一個數）。

**第 4f 段：engine 一定要配置的結果由回傳型別帶出操作**（2026-11-12，決定 33）：

- `dom_api`：`OpFront` 拿掉 21 個方法（`ret_objects`、`ret_frozen_objects`、`ret_strings`、`ret_frozen_strings`、`ret_doubles`、`ret_numbers`、`ret_dictionary`、`ret_records`、`ret_entry_detail`、`ret_reader_buffer`、`ret_history_state`、`ret_filled_view`、`ret_cloned`、`ret_resolved`、`ret_resolved_string`、`ret_resolved_json`、`ret_resolved_undefined`、`ret_resolved_bytes`、`ret_animation_promise`、`ret_resolved_with`、`ret_pending`），`Front` 拿掉 `prepare_promise`、多 21 個 `finish_*`（同樣的名字換成 `finish_`；最後兩個變成 `finish_resolved_constructor`、`finish_pending_promise`）。對應的回傳型別的 `Pending` 是帶出操作的資料：純資料照原樣（`finished_after!`），DOM 的物件在 `give` 裡 root 住、連 interface（`rooted_objects`）。`PromiseOf::Resolved(邊)` 換成 `Defined(定義)`，`PromisePending`；`ReaderResult` 的 `Pending` 是字串或 buffer（`ReaderPending`）。這些型別標成 `PLAIN`，`FilledView` 除外。`Rethrowing<R>`，`Request`、`Response` 讀 body 的十個成員用它。`Uint8ClampedArray` 和 `CanvasStyleValue` 的結果標成 `PLAIN`。
- `dom_api_macros`：dictionary 當結果時 `Pending` 是成員的值，`finish` 呼叫 `finish_dictionary`。
- `dom_js`：`V8Front` 的那 21 個結果從 `V8Op` 的方法加 `finish_kept` 的分支變成 `Front` 的方法（程式是原來的分支）；`Kept` 30 種剩 9 種；拿掉 `PreparedPromise` 和 `RareCall::promise`。`InfoFront` 的 `finish_*`（`finish_filled_view` 除外）和 `reject` 建 scope、交給 `V8Front` 的同一個方法。named property 的 front 的名稱列表改成 `finish_strings`。
- `dom_api` 的測試用 front：三個結果改成 `finish_*`。`dom_wasm` 沒有實作過這些結果，不用改。
- 和事先說的不同：說「一次做完 `Kept` 才能整個拿掉」不對，`Kept` 剩下的 9 種是 `V8Front` 自己在操作裡留的，4j 才拿掉。事先說可能套不進去的「操作前準備的 promise」套得進去：操作前建 resolver 沒有必要（resolve 和存起來本來就在操作後），改成操作後才建。
- 找到、修掉的：`plain` 的測試在回傳型別標成 `PLAIN` 後指出 `Request`、`Response` 的 `arrayBuffer`、`blob`、`bytes`、`json`、`text` 的函式會 `rethrow`（經過 `body::consume`）：`Rethrowing<R>`。
- `PLAIN` 的成員：2581 → 2646（getter 1374、setter 822、method 450）；能走新的 front 的 getter 只剩 1 個不是。
- 4e 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`x = n.getAttributeNames()`（兩個屬性）1900（420）→ 1707（361），`x = mo.takeRecords()`（空的）907（171）→ 696（127）；沒變的：`nodeType` 109 → 112、`getAttribute` 562 → 561、`x = t.onclick` 280、`addEventListener` 633、`createElement` 1992 → 1989、`appendChild` 1580。省下的是舊的 front 的 `Call` 和裝 `Kept` 的 `Box`；陣列本身的配置還在（`getAttributeNames` 的 1707 裡大部分是兩個字串和陣列）。其他的結果（promise、dictionary）在 `bench.py calls` 的頁面量不到（沒有 window），沒量。
- 測試：`dom_js` 的 `handlers::a_result_to_allocate_for_is_made_after_the_operation`（字串、物件、數字的陣列，每次新的；`takeRecords` 取走；frozen 的同一個、是 frozen 的；dictionary 和 record 的成員；animation 的 `ready`、`finished` 各自同一個；pending 的 `whenDefined` 同一個、已定義的每次新的、resolve 成 constructor、不合法的名稱 reject；`history.state`、mark 的 `detail` 第一次做、之後同一個；都建 scope、走新的 front）；`dom_api` 的 `table` 的清單。寫測試時弄錯的預期：`navigator.languages` 不是 frozen array（每次新的，本來就是）；`Blob` 的讀取有 `#[realm]`、不走新的 front；microtask 的順序。其餘靠原有的測試（`dom_js` 463 個照舊通過：這些結果原本就各有測試）。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4f.toml`，23 個都抓到。沒有寫的：把 `FilledView` 標成 `PLAIN`（`getRandomValues` 的參數型別不是 `PLAIN`，成員不會變，等價）。
- 跑過：`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt dom html/dom html/webappapis custom-elements html/semantics/forms html/canvas/element/pixel-manipulation fetch/api/response fetch/api/request FileAPI web-animations/interfaces hr-time user-timing`（135084 個結果都照預期；唯一失敗的 reftest 是本來就預期失敗的 `moveBefore-as-flex-item`）。

**第 4g 段：static 成員和 overload 的外殼；目前的 realm 的 window 從 isolate 讀**（2026-11-12，決定 34）：

- fork（只改 Rust，`36b85bd`，還沒 push）：`Context::current_global(isolate)`（`GetCurrentContext()->Global()`，handle 留在 V8 目前的 handle scope），`README.machopo.md`；`third_party.toml` 的 `rev` 跟著改。
- `bindings`：`StateInfo<'s, T, K = Signed>`，`Signed`、`Unsigned`；`receiver()` 只在 `Signed` 上；`StateInfoFunction`、`state_info_function_template`（沒有 signature）；`InfoNoGc::current_global`；`soundness.md` 補兩條。
- `dom_api_macros`：method 的 `PLAIN` 不再排除 static。`dom_api`：`resolve_overload` 的候選放 stack 上（`MOST_FORMS` 8 個），不再每次呼叫配置 `Vec`。
- `dom_js`：`InfoFront<'s, K>`、`InfoOp<'o, 's, K>`（`InfoKind`：receiver 的型別、交給 `V8Front` 時的參數來源、`this_as`）；`InfoFront::prepare_window` 記一個布林，`InfoOp::current_window` 在操作裡讀目前的 context 的 global、unwrap 成 window。`templates.rs`：`InfoStatic<M>`，plain 的 static 用 `state_info_function_template`；overload 的 method 和 static 的函式是只拿 info 的 `Overloaded`（`info_form` 選形式，`info_call::<M>`、`info_static_call::<M>` 呼叫），constructor 的那一組照舊；拿掉 `MethodCall`。
- 實際走新的 front 的成員（照分派算，決定 34）：2575 → 2632（2799 個裡）。剩下：不是 `PLAIN` 的 getter、setter、method 76（包括 5 個 overload 的形式）、constructor 50、cross-origin 16、named 13、沒有名字的 indexed getter 4、static 2（`AbortSignal.any`、`Response.json`）、iterator 的 pairs 2、legacy factory 2、indexed setter 2。
- 4f 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`ctx.setTransform(1, 0, 0, 1, 0, 0)` 1309（271）→ 755（142），`ctx.fill()` 2929（829）→ 2632（760），`ctx.isPointInPath(1, 1)` 2979（814）→ 2628（717），`ctx.isPointInPath(path, 1, 1)` 3511（909）→ 3101（809），`URL.canParse('https://a.test/')` 2305（476）→ 2252（469），`CSS.escape('ab')` 3719（2400）→ 3701（2407）；沒變的：`nodeType` 109 → 112、`getAttribute` 562 → 563、`removeEventListener` 606 → 607、`appendChild` 1580 → 1581。overload 省的是舊外殼的 scope、`Call`、候選的 `Vec` 和讀 data 的 `uint32_value`；static 的兩個省得少，成本在解析 URL、配置字串（`escape` 的 cycle 高是字串的配置，兩邊一樣）。
- 測試：`dom_js` 的 `handlers::static_operations_and_overloads_are_given_the_info_alone`（static 的布林結果不建 scope、`this` 是任何值也行；新字串、新物件操作後才建；`AbortSignal.abort()` 找到目前的 window、reason 是預設的 `AbortError` 或給的值（`InfoFront::abort_reason_arg` 4e 時走不到，現在有測試）；少給參數的 `TypeError`；overload 照參數個數和第一個參數是不是 `Path2D` 選形式，不建 scope；不是 `PLAIN` 的形式（`putImageData`、`scrollTo`）在 scope 裡照舊；個數不對的 `TypeError` 的訊息；method 的 receiver 照樣檢查）；`dom_api` 的 `table` 的清單。寫測試時弄錯的：以為 `Response.json(data)` 是 static 的 overload，它和實例的 `json()` 不同組、自己一個，又不是 `PLAIN`，不走新的 front。這個錯的案例讓 mutation test 的第一次在基準就失敗（那次測試的輸出在背景執行裡，沒看到）：之後啟動 mutation test 前先在前景跑一次測試。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4g.toml`，10 個都抓到。
- 跑過：`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`ci.py third-party`、`test.py wpt dom html/dom html/webappapis custom-elements html/semantics/forms html/canvas/element url css/cssom css/css-conditional/js fetch/api/response FileAPI`（148537 個結果都照預期）。

**第 4h 段之前：新的 front 補齊操作這一側的方法；第 4h 段：constructor 的外殼**（2026-11-12，決定 35）：

- 起因：constructor 標成 `PLAIN` 之後，`dom_js` 的 `canvas::image_data` 在 `new ImageData(1, 1)` abort（函式呼叫 `set_image_data_pixels`，新的 front 沒有）；決定 31 的原始碼掃描沒涵蓋 constructor。第四次了（決定 35 列了前三次）。先加的 `Unplain<A>` 沒有留下來。
- fork（只改 Rust，`5b1b5ae`，還沒 push）：`TracedSlot::get_in_callback`（從邊讀出 handle、留在 V8 目前的 handle scope），順便把 `set`、`set_droppable` 照 rustfmt 排好；`third_party.toml` 的 `rev` 跟著改。
- `dom_api`，`OpFront` 39 → 34 個方法：
  - 刪掉 `rethrow`：`ApiError::Aborted(AbortedSignal)`（signal 的 root；`ApiError` 不再是 `Eq`，`AbortedSignal` 的 `PartialEq` 永遠 false），`ApiError::aborted(cx, signal)`。`throwIfAborted`、body 的讀取、`fetch` 的函式回傳它；`body::consume` 和 `Request`、`Response` 讀 body 的十個函式不再拿 front（表上拿掉 `#[front]`）。刪掉 `Rethrown`、`Rethrowing<R>`。
  - 刪掉 `set_image_data_pixels`：`ImageDataWithPixels<'gc>`，`NewImageData` 的 `Pending` 帶著像素，`Front::finish_image_data_pixels`。`Constructed::construct` 回傳 `ConstructPending`（`None`、`Pixels`、`Upgrading`），產生的 constructor 在操作後 `finish_constructed`。`createImageData`、`getImageData`、`new ImageData(sw, sh)` 的函式不再拿 front。
  - 刪掉 `intersection_node`、`static_range_nodes`、`listed_shadow_roots`：`Front::intersection_observer_init`、`intersection_entry_init`、`static_range_init`、`get_html_options` 連同節點的 handle 一起回傳，參數型別的 `param` 用 `op.node` 換成節點（`getHTML` 的「是不是 shadow root」在那裡檢查）。`IntersectionInit` 拿掉 `root` 欄位。
  - `with_array_bytes`、`construct_html`（回傳 upgrade 中、已有 wrapper 的元素的 root）、`new_target_number`、`is_new_target` 沒有預設實作：`unbound_op_methods!` 給不綁這類成員的 front 明寫。`Front::finish_upgrading`。刪掉 `ReadImageData`。
  - 還有預設實作、新的 front 沒有的只剩 `kept_object`、`kept_target`、`store_array`（要編號）。
  - `plain` 的測試拿掉「函式呼叫了什麼」那一半（三個測試剩一個），說明改寫。
- `dom_api_macros`：constructor 的 `PLAIN`（參數都是 `PLAIN` 的）；constructor 的結果留到操作後的東西。
- `bindings`：`InfoNoGc::strong_value`；`soundness.md` 補一條。
- `dom_js`：`InfoKind` 改成三種（`Method`、`Static`、`Construct`，各自說 info 的種類、`this`、交給 `V8Front` 的參數來源；`Construct` 的 `this` 是新物件和 `new.target`）。`InfoFront::construct`（原地檢查 `new` 和參數個數；`HTMLElement` 的交給 `V8Front`）、`finish_image_data_pixels`、`finish_upgrading`；`InfoOp` 的 `construct_object`、`construct_html`、`new_target_number`、`is_new_target`、`with_array_bytes`。`WrapsNew`（兩種 no-GC 的 handle 都能把新的 wrapper 接上物件）。`templates.rs`：`InfoConstructor<M>`，plain 的 constructor 用 `state_info_function_template`，constructor 的 overload 那一組也是只拿 info 的 `Overloaded`（`Call` 那一份留給不是 `PLAIN` 的形式）。`V8Front`：`throw_abort_reason`（五個丟錯的地方都認得 `ApiError::Aborted`），`Kept` 9 → 7 種（少 `Pixels`、`Upgrading`），`RareCall` 少三個欄位。
- 量測時找到、修掉的：帶 init dictionary 的 constructor 比舊的慢（`new Event('x', init)` 5475 → 5616）。新的 front 讀 dictionary 的成員時每個成員各建一個 scope（4d 起就是這樣，成員少時不明顯；`EventInit` 有三個）。改成整個 dictionary 一個 scope：產生器替每個 dictionary 產生一個讀取器（`ReadDict<T>`：一個泛型的 `read<F>`，`F` 拿參數的方式和 `T` 一樣），`Front::reading` 決定在哪個 front 上跑它（`InfoFront`：一次 `with_front`；其他的：自己）。先試了關聯型別 `Reader<'r>`，`V8Front` 借著臨時的 scope、lifetime 寫不出來，沒留下。
- 保留的成本：`HTMLElement` 的 constructor（custom element）每次建一個 scope 檢查 `new.target` 是不是 `HTMLElement` 本身（要拿 realm 的 interface 物件）。
- 實際走新的 front 的成員：2632 → 2678（2799 個裡）。constructor 50 個裡 30 個走新的（其餘 20 個的參數型別不是 `PLAIN`）；剩下：不是 `PLAIN` 的 getter、setter、method 60、constructor 20、cross-origin 16、named 13、沒有名字的 indexed getter 4、static 2、iterator 的 pairs 2、legacy factory 2、indexed setter 2。
- 4g 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`new Event('x')` 2761（691）→ 2612（671），`new Event('x', init)` 5489（1186）→ 5350（1139），`new CustomEvent('c')` 2739（680）→ 2599（656），`new EventTarget()` 1877（376）→ 1786（361），`ctx.putImageData(img, 0, 0)` 1531（321）→ 1324（263），`ctx.getImageData(0, 0, 1, 1)` 9298（2981）→ 9025（3071），`removeEventListener('y', f, opts)`（一個成員）1628（326）→ 1558（297），`addEventListener('x', f, opts3)`（三個成員）4718（922）→ 4338（798）；沒變的：`nodeType` 109、`getAttribute` 563、`removeEventListener` 607 → 605、`appendChild` 1581。`new Event('x')` 的 2612 裡大部分是建事件物件和它的 wrapper（V8 在呼叫前就做好新物件）；`throwIfAborted`、`new Text()`、custom element 的 constructor 要 window，`bench.py calls` 的頁面量不到。
- 測試：`dom_js` 的 `handlers::what_a_function_asks_of_the_operations_front_end_every_front_end_has`（丟出來的就是 reason 那個物件、沒 abort 的不丟、預設的 `AbortError`；像素寫入再讀回、新的 `ImageData` 是零、`putImageData` 不建 scope；`new Mine()`、`new HTMLElement()` 和沒有定義的 `new.target` 是 `TypeError`、upgrade 的元素是原來那個、prototype 是 class 的；`fetch` 的 signal abort 時 reject 成 reason）、`handlers::a_constructor_is_given_the_info_alone`（`new Event` 等不建 scope；`new.target` 的 prototype；沒用 `new`、參數不夠的 `TypeError` 訊息；好幾個 constructor 的照參數選；一個 dictionary 只建一個 scope）、`info_front::a_constructed_objects_wrapper_is_counted`；`dom_api` 的 `table` 的清單。寫測試時弄錯的：這個測試環境沒有 raster，`fillRect` 之後 `getImageData` 是零（原有的 canvas 測試也是用 `putImageData` 寫入）；把分號換成逗號的寫法弄壞了 class 的內容；回傳字串和非整數的 getter 本來就建 scope；`addEventListener` 之後回報 heap 長大的那個 scope 不是 dictionary 的。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4h.toml`，22 個都抓到。第一次 20 個裡 19 個：「constructor 建的物件不計入 wrapper 的計數」沒抓到，補了 `a_constructed_objects_wrapper_is_counted`。
- 跑過：`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`ci.py third-party`、`test.py wpt dom html/dom html/webappapis custom-elements html/semantics/forms html/canvas/element url fetch/api/response fetch/api/request fetch/api/abort FileAPI intersection-observer shadow-dom`（144990 個結果都照預期；失敗的 22 個 reftest 都是本來就預期失敗的）。

**第 4i-1 段：indexed getter 的 interceptor**（2026-11-12，決定 36）：

- fork（只改 Rust，`cc74cef`，還沒 push）：`PropertyCallbackInfo::isolate`、`arguments`（不建 `CallbackScope` 讀 isolate、holder、data）；`third_party.toml` 的 `rev` 跟著改。
- `bindings`：`InfoState<'s, T>`（`isolate`、`state`、`state_mut`、`with_scope`：`StateInfo` 和新的 `StateIndexInfo` 共同的介面）；`StateIndexInfo<'s, T>`（interceptor 的 info、讀出來的 isolate、索引；`index()`、`holder()`）；`StateInfoIndexedGetter`、`set_state_info_indexed_getter`（getter 用 `getter_raw`，查詢和列舉照舊，有沒有 setter 是參數）；`soundness.md` 補一條。
- `dom_js`：`InfoKind::Info` 是 info 的型別本身（原本是 `Signed`、`Unsigned` 的標記），參數、個數、`this` 都經過 kind 讀（`arg`、`length`、`number_arg`、`this_object`；交給 `V8Front` 的參數來源分成 `seed` 和 `args`，index 的 V8 數字在有 scope 時才建）；`IndexedGet`；`InfoFront` 多 `noted`（`K::Noted`：indexed getter 的「沒有這一項」，其他 kind 是 `()`）。`templates.rs`：`Indexed<M>` 的 `StateInfoIndexedGetter`，plain 的 getter 用新的安裝函式。
- 找到、修掉的（新的測試）：`n.style[0]`（沒有宣告）是空字串而不是 `undefined`。`CSSStyleDeclaration.item` 過了最後一個給的是名稱表的空字串（`DomStr::Name(NONE)`），經過 `ret_name` 不是 `ret_string`，沒記成「沒有這一項」；原有的 467 個測試沒有一個看這個。`ret_name` 也記。
- 實際走新的 front 的成員：2678 → 2682（沒有名字的 indexed getter 4 個；有名字的 10 個的函式那一份本來就是，這次換的是它們的 interceptor）。
- 4h 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`list[0]` 686（143）→ 440（84），`list[9]`（超出範圍）1202（231）→ 969（178），`children[0]` 761（152）→ 515（96），`classList[0]` 791（168）→ 521（103），`attributes[0]` 785（159）→ 538（102），`n.childNodes[0]` 902（188）→ 656（128）；沒變的：`nodeType` 109、`list.item(0)` 257、`list.length` 150、`children.px` 1847、`dataset.a` 1310、`dataset.a = '1'` 1814、`attributes.id` 1848（named 的是 4i-2）。`list[0]` 還比 `list.item(0)` 多 183 條：V8 的 indexed interceptor 那條路本身（不是 inline cache 直接呼叫的 API 函式），加上從 info 讀 isolate、holder、回傳值的三個 C++ 呼叫（函式的 info 是從 frame 的固定位置讀的）。
- 測試：`dom_js` 的 `info_front::an_indexed_getter_is_given_the_info_alone`（`childNodes`、`children`、`attributes`、`classList` 的項不建 scope；過了最後一個、很大的索引是 `undefined`；空字串的沒有這一項（`style[0]`）；`in`、`Object.keys`、展開、唯讀、字串的索引、負的和不是整數的照舊；沒有 wrapper 的項建一個、之後同一個）。寫測試時弄錯的：`list.item(0)` 是 method、呼叫完會回報 heap 的成長，那個 scope 不是 getter 的；回傳字串的 `.name` 本來就建 scope。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4i.toml`，8 個裡 7 個抓到。沒抓到的是「`ret_string` 的空字串不記成沒有這一項」：表上沒有 indexed getter 經過 `ret_string` 給空字串（`item` 給 `Option` 的過了最後一個是 null，`CSSStyleDeclaration` 的給名稱表的空字串、走 `ret_name`），等價；檢查留著，和 `V8Front::returned_no_item` 一致。
- 跑過：`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`ci.py third-party`、`test.py wpt dom html/dom html/webappapis custom-elements html/semantics/forms css/cssom`（136064 個結果都照預期）。

**第 4i-2 段：named 的 getter 和 setter 的 interceptor；空字串不做字串**（2026-11-12，決定 37）：

- fork（只改 Rust，`6dbddfd`，還沒 push）：`SealedLocal::unseal_in_callback`；`third_party.toml` 的 `rev` 跟著改。
- `bindings`：`StateNameInfo<'s, T>`（info、isolate、名稱、setter 的值；`InfoState`）；`StateInfoNamedGetter`、`StateInfoNamedSetter`、`info_named_getter`、`info_named_setter`（回傳 V8 的 raw callback）；`NamedOptions` 多 `getter_info`、`setter_info`，`set_state_named_properties` 有的話用它們（`getter_raw`、`setter_raw`），其餘的 callback 照舊；`OpReturn::set_empty_string`；`soundness.md` 補兩處。
- `dom_js`：`InfoKind` 的 `NamedGet`、`NamedSet`（`named_kind!` 是兩者共同的部分）；`note_no_item` 分成 `note_null`、`note_empty`，多 `note_bool`（`NamedSet` 記下「不收」、不寫回傳值）；`front.rs` 的 `NameArgs`（交給 `V8Front` 時的參數來源）；`named.rs` 的 `NamedGetter<M>`、`NamedSetter<M>`，`NamedCalls` 多 `get_info`、`set_info`（成員是 `PLAIN` 時填）。`InfoOp::ret_string` 和 `ret_name` 的空字串直接設 V8 的空字串。
- 換到的成員：8 個 named getter 和 2 個 setter 都是 `PLAIN`（`dataset`、`CSSStyleDeclaration`、`HTMLCollection`、`HTMLAllCollection`、`HTMLFormElement`、`HTMLFormControlsCollection`、`HTMLOptionsCollection`、`NamedNodeMap`）。實際走新的 front 的成員 2682 → 2692。
- 量測時找到的（不在這一段修的那一半記在 README 的「之後的可能」）：`style.color` 的 cycle 異常（決定 36）的原因。字串的快取照 `DomString` 的位址記（`StringCache`），每次現算的字串位址都不同、永遠找不到，於是每次新建 V8 的字串並在快取登記一筆（`Weak` 加 finalizer）。空字串這一段繞過去了（下面）；非空的（`style.color` 是 `'red'`）仍然 4709 條、2664 個 cycle。
- 4i-1 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`children.px` 1873（404）→ 834（155），`attributes.id` 1848（396）→ 814（154），`dataset.a` 1310（324）→ 1042（211），`dataset.a = '1'` 1814（373）→ 1576（306），`dataset.zz`（沒有）1822（380）→ 1621（307），`style.color`（空字串）4052（3293）→ 1544（338），`style.color = 'red'` 4555（1188）→ 4339（1121），`style.zz`（沒有）893（194）→ 704（144），`style.color`（`'red'`）4631（2619）→ 4709（2664）；沒變的：`nodeType` 109、`list.item(0)` 259 → 258、`list[0]` 440、`children[0]` 515、`n.title`（空字串）278 → 279。`style.color`（`'red'`）多的 78 條是結果的字串 V8 沒有時當場建 scope、交給 `V8Front` 的那一趟（舊的 interceptor 本來就有 scope），4m 的事。
- 測試：`dom_js` 的 `info_front::a_named_getter_and_setter_are_given_the_info_alone`（`children`、`attributes`、`dataset` 照名字讀不建 scope；沒有的名字是 `undefined`；空字串是值、不建 scope（`dataset`、`style`、`title`）；prototype 的方法和 `length` 不被蓋掉；setter 寫進屬性和宣告、非字串的值轉換、`null` 是空字串；不收的名字變成一般屬性；setter 丟的例外；`in`、`Object.keys`、`delete`、數字當名字、symbol、descriptor、`defineProperty` 照舊；沒有 wrapper 的項建一個）。`strings::a_names_v8_string_is_made_once` 的預期從 3 個名稱改成 2 個（空字串不再做）。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4i2.toml`，9 個都抓到。第一次 5 個：四個沒抓到的都是測試分辨不出來（判斷「走了新的 front」的那幾行後面接了別的會建 scope 的呼叫：`.localName`、再讀一次；named setter 的參數個數少算時 `has_args` 交給 `V8Front` 補回來、只多一個 scope），改成只量 getter 或 setter 自己的那一次，加一個「字串的值、已有的名字不建 scope」。
- 跑過：`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`ci.py third-party`、`test.py wpt dom html/dom html/webappapis custom-elements html/semantics/forms css/cssom css/css-variables`（136871 個結果都照預期）。

**第 4i-3 段：只被這次呼叫持有的字串不進字串的快取**（2026-11-12，決定 38）：

- `dom`：`DomString::is_unique` 公開（只有 `self` 持有這個字串）。
- `dom_js`：`cached_string`（操作之後做字串的那一步，`V8Front` 的 `Kept::String` 和 named getter 都經過它）在快取找不到、而且字串只有呼叫自己持有時，直接做 V8 的字串（`strings::new_string`），不登記（沒有 `Weak`、沒有 finalizer、快取不多留一份 `DomString`）。DOM 持有的字串（屬性的值、文字節點的資料）照舊登記。`cached_strings`（測試用：isolate 的快取有幾筆）。
- 判斷放在冷的那一步：操作裡的 `ret_string` 沒有動（照位址查快取，找不到就把字串帶出操作），所以已經在快取裡的字串一條指令都不多。
- 4i-2 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`style.color`（`'red'`）4712（2695）→ 2192（544），`style.cssText` 13438（4594）→ 10901（2434）；沒變的（差在空迴圈的雜訊 ±10 之內）：`nodeType` 110 → 99、`list[0]` 440 → 430、`dataset.a` 1042 → 1031、`style.color`（空字串）1544 → 1533、`getAttribute('id')` 566 → 556、`n.id` 254 → 244、`text.data` 187 → 176、`tagName` 241 → 234。
- 測試：`dom_js` 的 `strings::a_string_only_the_call_holds_isnt_cached`（`style.color`、`style['color']`、`style.cssText` 各讀 50 次，快取的筆數不變、值正確；`getAttribute('title')` 讀 50 次只多一筆）。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4i3.toml`，4 個都抓到（一律登記、反過來只登記呼叫自己持有的、一律不登記、兩個持有也算唯一）。
- 跑過：`cargo test -p dom -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt dom html/dom css/cssom css/css-variables`（128481 個結果都照預期；失敗的 13 個 reftest 都是本來就預期失敗的）。

**第 4j-1 段：`KeptObject` 的編號和 `Uint8ClampedArg` 換成 handle**（2026-11-12，決定 39）：

- `dom_api`：`ViewObject<N>`、`RelatedTarget<N>`（init dictionary 的 `view`、`relatedTarget`）、`SignalMember<N>`、`BodyMember<N>`、`BodyArg<N>`（`RequestInit` 的 `signal`、`body`，`new Response(body)`）都帶 front 的 handle（`FrontTypes::Node`），dictionary 的成員經過 `DictHandle` 讀；`Uint8ClampedArg<A>` 是 handle（`FrontTypes::Any`）加長度。`Front::dict_nullable_object::<T>`（原本照 `KeptKind` 分）、新的 `dict_nullable_target`，`body_arg`、`dict_body`、`dict_signal`、`uint8_clamped_arg` 回傳 handle。操作裡用已有的 `OpFront::object`、`target`、`store_any`。拿掉 `KeptObject`、`KeptWindow`、`KeptTarget`、`KeptKind`、`OpFront::kept_object`、`kept_target`、`store_array`：`OpFront` 沒有「要編號才能呼叫」的方法了。UI 事件的六個 init dictionary 和 `RequestInit` 多型別參數（函式拿 `&MouseEventInit<O::Node>`）。
- `dom_js`：`V8Front` 的 `kept_objects`、`array`（`RareCall` 的欄位）和 `keep_object` 拿掉；`InfoFront` 實作這六個方法（dictionary 的四個和 `body_arg` 經過 `with_front`；`uint8_clamped_arg` 自己讀，是 typed array 時不建 scope）。`dom_wasm` 不用改（這些方法它用預設的）。
- `PLAIN`：`Uint8ClampedArray` 的參數標成 `PLAIN`；UI 事件的 init dictionary 的成員都是 handle 或 `PLAIN`，跟著是。`PLAIN_FRONT_METHODS` 加 `dict_nullable_object`、`dict_nullable_target`、`dict_body`、`dict_signal`、`uint8_clamped_arg`（原始碼的檢查先擋下來：`BodyMember`、`SignalMember` 改成 `DictHandle` 之後呼叫了清單外的方法）。
- 換到的成員：`UIEvent`、`MouseEvent`、`WheelEvent`、`FocusEvent`、`KeyboardEvent`、`InputEvent`、`ImageData` 的 constructor。實際走新的 front 的成員 2692 → 2699；不是 `PLAIN` 的 constructor 20 → 13。
- 4i-3 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`new MouseEvent('x')` 2689（667）→ 2568（637），`new MouseEvent('x', { clientX: 1 })` 32216（5911）→ 32045（5889），`new MouseEvent('x', { relatedTarget: n })` 33208（6282）→ 32726（6100），`new UIEvent('x', { detail: 1 })` 8343（1714）→ 8227（1676），`new KeyboardEvent('x', { key: 'a' })` 31846（6224）→ 31500（6115），`new ImageData(u8, 2)` 2947（620）→ 2603（533）；沒變的：`nodeType` 109 → 112、`new ImageData(2, 1)` 8648 → 8665、`new Event('x')` 2612 → 2617、`new Event('x', init)` 5358 → 5352。
- 量測時看到的（不在這一段修，記在 README 的「之後的可能」）：帶 init 的 `new MouseEvent` 三萬多條指令，幾乎都在讀 dictionary：每個成員各把名稱做成 V8 的 internalized 字串（`templates::string`）再 `Get`，一個成員約一千條，`MouseEventInit` 有三十個左右。
- 測試：`dom_js` 的 `handlers::an_objects_handle_is_taken_as_it_is`（`view` 是 window、null、undefined、沒給；`relatedTarget` 是節點、window、`new EventTarget()` 的物件、null；六個 constructor 都建 scope 讀 dictionary（走新的 front）；`view` 是節點或數字、`relatedTarget` 是一般物件或事件時是 `TypeError`；`new ImageData(bytes, 2).data === bytes` 不建 scope、長度不對的 `InvalidStateError`、別種 typed array 走另一個 constructor；`Request` 的 signal（跟著 abort、null 不跟 input 的、沒給的跟、別種物件是 `TypeError`）、body 是 `Blob` 和 `URLSearchParams` 時的 `Content-Type`）。寫測試時弄錯的：這幾個 `TypeError` 和 `InvalidStateError` 的訊息沒有 `Failed to construct` 的前綴；`new ImageData(new Uint8Array(8), 2)` 不是 `TypeError`，是另一個 constructor 的 `IndexSizeError`。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4j1.toml`，15 個都抓到。第一次停在「null 的 signal 當成沒給」沒抓到（原有的測試也沒有分辨），補了「input 有 signal、init 的 signal 是 null 時不跟」。
- 跑過：`cargo test -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt dom/events uievents html/canvas/element fetch/api/request fetch/api/response fetch/api/abort`（4505 個結果都照預期；失敗的 18 個 reftest 都是本來就預期失敗的）。

**第 4j-keys 段：dictionary 的成員名稱是名稱表的固定名稱**（2026-11-12，決定 40）：

- 剖析（`perf record`，`new MouseEvent('x', { clientX: 1 })`）：V8 的 `Object::Get` 和它底下的查找約 69%，把成員名稱做成 internalized 字串約 17%，其餘（建事件、外殼、轉換）約 14%。
- `dom`：`dictionary_members.txt`（145 個名稱，其中 116 個原本不是固定名稱），`build.rs` 排在所有其他固定名稱之後。`UPPERCASE` 表只留 local name 的那一段：其餘的固定名稱（event type、成員名稱）當 HTML 元素的名字時走一般名稱的路轉大寫。修掉原有的問題：`<click>` 的 `tagName` 是 `click`（加了成員名稱之後 `dom` 原有的兩個測試先抓到 `<c>`）。
- `dom_api`：`MemberName`（只包名稱的編號；`of` 是 `const fn`，不在清單裡的編不過；`name()` 從名稱表讀文字，給錯誤訊息和第一次建字串用），`Front` 的 `dict_*`、`DictMember::read`、`DictHandle::read`、`read_member`、`read_handle` 的 `member: &str` 都換成它；macro 產生 `const { MemberName::of("…") }`。
- `dom_js`：`member_key`（照編號從 `caches.names` 拿，沒有時建）、`make_name`（建 internalized 的字串、存進 `caches.names`、計數；`Kept::Name` 也用它，所以 `localName` 這類名稱的 V8 字串也是 internalized 的）。`V8Front` 讀成員的 10 處都經過 `member_key`。
- 和事先說的不同：一開始寫的 `MemberName` 同時帶字串和編號，使用者指出放進名稱表之後它就是 atom，字串是多餘的，改成只包編號。估計每個成員省約 175 條，實際約 300 條：key 是 internalized 時 V8 查屬性前的處理（`PropertyKey`）也變便宜。
- 4j-1 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`new MouseEvent('x', { clientX: 1 })` 32045（5890）→ 23451（4874），`new MouseEvent('x', { relatedTarget: n })` 32727（6083）→ 24128（5125），`new KeyboardEvent('x', { key: 'a' })` 31500（6105）→ 23380（5116），`new UIEvent('x', { detail: 1 })` 8227（1671）→ 6585（1454），`new Event('x', init)` 5348（1156）→ 4612（1082），`addEventListener('z', f, opts)` 4370（788）→ 3338（660）；沒變的（差在雜訊 ±10 之內）：`new MouseEvent('x')` 2568 → 2558、`new Event('x')` 2605 → 2595、`nodeType` 109 → 101、`tagName` 243 → 235、`n.id` 254 → 245、`getAttribute('id')` 567 → 558、`style.color` 2202 → 2196。
- 測試：`dom_js` 的 `strings::a_dictionary_members_key_is_made_once`（第一次讀 `EventInit` 做三個名稱、之後 20 次不再做；別的 dictionary 同名的成員不再做、新的名稱多一個；getter 照名稱的順序各叫一次；同時是成員名稱和元素名稱的 `once` 兩邊都對）；`dom_api` 的 `members::the_listed_member_names_are_the_dictionaries`（清單和原始碼裡 dictionary 的成員一致、照順序、都是固定名稱）；`dom` 的 `names::a_fixed_name_that_is_no_local_name_is_uppercased_too`。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4j-keys.toml`，5 個都抓到。另外試了「清單少一個名稱」：編不過（`MemberName::of` 的 const 求值 panic），這個由編譯器擋，不列在 spec 裡。「名稱的字串不是 internalized」結果一樣、只是慢，沒有測試分辨得出來（rusty_v8 沒有查詢的 API），記在 `docs/test-gaps.md` 的「沒有計數守著的優化」。
- 跑過：`cargo test -p dom -p dom_api -p dom_js -p dom_wasm -p html -p style`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt dom/events dom/nodes uievents fetch/api/request fetch/api/response intersection-observer web-animations/interfaces`（19302 個結果都照預期；失敗的 1 個 reftest 是本來就預期失敗的）。

**第 4j-2 段：操作前讀成 Rust 值的參數型別**（2026-11-12，決定 39）：

- `dom_js`：`InfoFront` 實作 26 個讀取的方法。22 個是冷的，當場建 scope、呼叫 `V8Front` 的那段程式（`serialized_state_arg`、`message_arg`、`static_range_init`、`intersection_observer_init`、`intersection_entry_init`、`byte_string_pairs_arg`、`json_arg`、`doubles_arg`、`get_html_options`、`round_rect_radii_arg`、`animation_options_arg`、`effect_options_arg`、`optional_timing_arg`、`timeline_arg`、`keyframes_arg`、`clone_arg`、`string_pairs_arg`、`blob_parts_arg`、`array_view_arg`、`objects_arg`、`dict_headers`、`dict_is_null`、`dict_serialized`）；三個自己判斷、不建 scope：`object_if`（wrapper 的 tag）、`bool_if`（本身是布林）、`number_if`（小整數；其他數字才建 scope）；`finish_filled_view`。
- `getRandomValues` 的 view 不再由 front 留在呼叫裡：`finish_filled_view` 讀第一個參數（`array_view_arg` 檢查過的那個）。`V8Front` 的 `RareCall::view` 拿掉。
- `dom_api`：33 個型別標成 `PLAIN`（`SerializedState`、`PortMessage`、`StaticRangeInitArg`、`IntersectionObserverInitArg`、`IntersectionObserverEntryInitArg`、`AbortSignals`、`MessagePorts`、`HeaderPairs`、`RequestInfo`、`NullableBody`、`JsonData`、`Doubles`、`GetHtmlOptionsArg`、`CanvasStyleValue`、`OptionalPath2dOrString`、`RoundRectRadii`、`AnimationOptions`、`EffectOptions`、`OptionalTimingArg`、`TimelineArg`、`Keyframes`、`Hidden`、`NumberIfNumber`、`ArrayView`、`FilledView`、`CloneValue`、`MeasureStart`、`StringPairs`、`BlobParts`，dictionary 的成員 `HeadersMember`、`IsNull`、`Serialized`）；`PLAIN_FRONT_METHODS` 加 28 個方法。沒標的：`WindowMessage`、`NavigationUrl`（要 incumbent、entry realm 的 window：`prepare_incumbent_window`、`prepare_entry_window`，新的 front 沒有，4l 和 cross-origin 的成員一起做）。
- 換到的成員：剩下的 13 個 constructor、19 個 method、3 個 setter、2 個 static（`AbortSignal.any`、`Response.json`）。不是 `PLAIN` 的只剩 45 個：method 40（會讓 script 跑的 33、`Blob` 的 `arrayBuffer`、`bytes`、`text`（`this` 的 relevant realm 的 window）、`Location.assign`、cross-origin 的 `Location.replace`、`Window.focus`、`Window.postMessage`）、setter 3（`Document.location`、cross-origin 的 `Location.href`、`Window.location`）、legacy factory 2。實際走新的 front 的成員 2699 → 2736。
- 4j-keys 和這一段的 binary 各跑七次取最小（`perf stat`，指令數（cycle））：`new Blob(['ab'])` 9666（2338）→ 9624（2405），`new Headers([['a', 'b']])` 19504（4569）→ 19462（4423），`new URLSearchParams([['a', '1']])` 19267（4429）→ 19226（4353），`c.getHTML(options)` 3525（806）→ 3381（767），`c.hidden = true` 969（477）→ 895（446），`new StaticRange(init)` 6046（1207）→ 6041（1137），`new Path2D('M0 0')` 4196（1098）→ 4079（1057）；沒變的：`nodeType` 111、`new MouseEvent('x', init)` 23533 → 23532、`new Event('x')` 2616 → 2617。和決定 39 說的一樣：成本在讀 JS 物件本身，換外殼省 0 到 150 條。
- 測試：`dom_js` 的 `handlers::an_argument_read_whole_is_read_in_a_scope`（`hidden` 給布林、小整數不建 scope、給字串和要轉換的物件；`fillStyle` 給 gradient 不建 scope、給字串和要轉換的物件；`setLineDash`、`roundRect`、`toDataURL` 的品質、`getRandomValues`（同一個 view、填了、buffer 的一部分只寫那一段、浮點的 `TypeMismatchError`、不是 view 的 `TypeError`）、`structuredClone`、`Blob`、`File`、`Headers`（sequence 和 record）、`URLSearchParams`、`AbortSignal.any`、`Response.json`、`Response` 的 init、`Request` 的 headers、`StaticRange`、`getHTML`、`pushState`、`replaceState`、`performance.mark`、`PerformanceMark`、`measure`、`animate`、`KeyframeEffect`、`setKeyframes`、`updateTiming`、`Animation`、`IntersectionObserver`、`postMessage`、`Path2D`：值正確、走新的 front 的建 scope）；`dom_api` 的 `table::members_of_strings_nodes_numbers_and_booleans_are_plain` 加「不是 `PLAIN` 的成員就是這 45 個」的名單（任何型別的 `PLAIN` 被改掉都會改變名單）。寫測試時弄錯的：測試沒有 raster，`toDataURL` 是 `data:,`；量測用的環境沒有 window，`new AbortController()` 丟 `InvalidStateError`（那一項不量）。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4j2.toml`，10 個都抓到。
- 跑過：`cargo test -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt fetch/api FileAPI url web-animations html/canvas/element webmessaging html/webappapis/structured-clone intersection-observer user-timing dom/abort dom/ranges WebCryptoAPI/getRandomValues.any.js html/browsers/history`（63646 個結果都照預期；失敗的 25 個 reftest 都是本來就預期失敗的）。

**第 4k 段，量測：會呼叫 script 的 33 個成員**（2026-11-12；`bench_calls` 加 `MACHOPO_BENCH_WINDOW`，在 window 的 realm 裡跑；各跑五次取最小，指令數（cycle））：

| 呼叫 | 成本 |
| --- | ---: |
| `t.dispatchEvent(ev)`，沒有 listener | 1623（354） |
| `t.dispatchEvent(ev)`，一個 listener | 3596（744） |
| `c.dispatchEvent(ev)`，在樹上、bubble、一個 listener | 6798（1450） |
| `c.click()` | 6412（1466） |
| `inp.focus()` / `inp.blur()` | 907（189）/ 426（90） |
| `tw.parentNode()`（在 root，回 null） | 398（78） |
| `tw.currentNode = c; tw.parentNode()` 扣掉 setter | 525 |
| `tw.currentNode = n; tw.nextNode()` 扣掉 setter | 538 |
| 同上，有 filter | 2130 |
| `ni.nextNode(); ni.previousNode()` / 有 filter | 1083（224）/ 4483（1045） |
| `form.reset()` | 7156（1801） |
| `inp.select()` / `ta.select()` | 2204（1011）/ 1810（472） |
| `sel.collapse(n, 0)` / `sel.selectAllChildren(n)` | 1688（313）/ 1627（297） |
| `customElements.upgrade(c)` | 2401（429） |
| `navigator.scheduling.isInputPending()` | 2288（426），其中讀 `navigator.scheduling` 1868 |
| `ac.abort()`，已經 abort 過 | 20526（6533）：每次建預設的 `AbortError`、抓 stack trace |

對照的普通成員：`c.parentNode`、`tw.currentNode`、`tw.root` 128 到 130；`tw.currentNode = c` 376；`n.contains(c)` 339。

**第 4k 段，一次呼叫的各個零件各多少**（2026-11-12；`prototypes/call-pieces-probe.patch`，量完還原：在普通成員的 callback 裡多做 5 次某個零件、看 `nodeType` 多幾條，除以 5）：

| 零件 | 指令數（cycle） |
| --- | ---: |
| 一個 scope（`StateInfo::with_scope`：rusty_v8 的 `CallbackScope`，不開 handle scope） | 27（5） |
| scope 加整個 `V8Front`（`with_front`） | 62（22） |
| 一個讀的操作（`Front::read`，空的） | 20（4） |
| 一個寫的操作（`Front::mutate`，空的） | 44（8） |
| 登記再放掉一個 root（`Persistent::new`、drop；扣掉包它的讀的操作） | 約 110（20） |
| 回報 heap（`report`，沒有要報的） | 22（3） |

- 事先估「一個 scope 100 到 200 條」是錯的：27 條。會呼叫 script 的成員貴的不是 scope，是 root 和操作的個數。`tw.parentNode()` 比普通的 getter 多的約 270 條：舊的外殼和 `V8Front` 約 60、`this` 的 root 約 110、四個操作（root、看有沒有 filter、走、給結果）約 130、回報 22，其中有重疊。`dispatchEvent`（沒有 listener）：`this` 和事件兩個 root 約 220、八到十個操作約 300，1623 條裡約三成。
- 另外量到的（不是這 33 個成員的事，記在 README 的「之後的可能」）：window 的 getter 當全域變數讀，每次約 1700 條（`navigator` 1707、`innerWidth` 1773、`performance` 1848、`location` 1888；`performance.now()` 2547 裡 1850 是讀 `performance`）；`document`、`window`、`setTimeout` 是 0。V8 的 `ic.cc`（`LoadIC::ComputeHandler` 的 `ACCESSOR`）：API getter 的 holder 不是 fast properties 時給 `LoadSlow`，全域物件一定是 dictionary mode；它前面有 `TryLookupCachedProperty`（cached accessor）這條出路。

**第 4k 段，盤點：走新的 front 的成員什麼時候還建 scope**（2026-11-12；`info_front.rs` 裡會建 scope 的方法 91 個，照 `INTERFACES` 的型別數成員；一個 scope 27 條指令）：

| 什麼時候 | 哪些成員（個數是表上的） | 多常發生 |
| --- | --- | --- |
| 結果一定要 V8 配置 | 回傳 promise 的 34 個；回傳陣列、dictionary、record 的約 35 個（`getAttributeNames`、`querySelectorAll` 以外的 sequence、`getLineDash`…）；建新節點的 method（`createElement`、`cloneNode`、`createTextNode`…：先建 wrapper） | 每次 |
| 結果是現算的字串 | 回傳字串的 getter 383 個、method 21 個、nullable 的 29 個裡，值不是 DOM 留著的那些：`style.color` 這類序列化、`innerHTML`、`outerHTML`、`textContent`（元素的）、`href` 這類 URL 的部分… | 每次（DOM 留著的字串只有第一次） |
| 結果是還沒有 wrapper 的節點或物件 | 回傳節點的 getter 177 個、method 150 個；回傳其他物件的 | 每個物件第一次 |
| 結果是放不進小整數的數字 | 回傳 double 的 getter 154 個、method 6 個（值不是整數時）；`i32`、`u32` 超過 31 bits 時 | 看值 |
| 字串參數要轉換 | 字串參數 477 個、nullable 的 57 個：不是字串、不是 ASCII、或不是扁平的（剛串接出來的 cons string） | 看呼叫的寫法；`'a' + i` 這種每次 |
| 數字、布林參數要轉換 | double 229 個、`i32`、`u32`：給的不是數字（或不是小整數的 `i32`） | 少 |
| 讀 dictionary、sequence、callback 物件 | dictionary 參數 65 個、`DictOrBool` 3 個、4j-2 的那些型別 | 有給就建（dictionary 的成員共用一個） |
| 丟例外、reject | 任何成員 | 出錯時 |
| 呼叫之後回報 | setter 和 method：DOM 的 heap 長到要告訴 V8、有 microtask 要排 | 偶爾 |
| constructor | 50 個：`new.target` 不是 interface 自己的、`HTMLElement` 的每次 | 看情況 |

- 不建的：回傳布林、小整數、null、`undefined`、已有 wrapper 的節點、DOM 留著而且做過 V8 字串的字串、名稱（`tagName`、`localName`）、event handler 的函式；參數是扁平的 ASCII 字串、數字、布林、節點、函式。
- 更正（使用者問「確定都只有 27 條嗎」之後量的）：27 條只是空的 scope。走到這些情況時實際多花的是幾百到一千多條（下表）；其中多少是 V8 一定要做的配置和轉換、多少是我們這條路自己的（整個 `V8Front` 62、root 約 110、多開的操作 20 到 44、把字串複製成 Rust 的 `String`），還沒拆開。

  | 情況 | 不建 scope 的對照 | 走到這條路 | 多花 |
  | --- | ---: | ---: | ---: |
  | `setAttribute('a', s)`，`s` 是剛串接的長字串（cons string；扣掉串接本身） | 1075（扁平的 ASCII） | 1848 | +773 |
  | `getAttribute(s)`，`s` 是 cons string | 456 | 1392 | +936 |
  | `setAttribute('a', 'valué…')`（非 ASCII） | 1075 | 1591 | +516 |
  | `setAttribute('a', 5)`（數字轉字串） | 1075 | 1361 | +286 |
  | `setAttribute('a', 'v' + (i & 7))`（短的串接，V8 直接給扁平的） | 1075 | 1146 | +71 |
  | `ev.timeStamp`（不是整數的 double） | 約 130（回傳小整數的 getter） | 497 | 約 +370 |
  | `cur.nextSibling`，節點還沒有 wrapper | 約 130 | 1679 | 約 +1550 |
  | 現算的字串（`style.color` 是 `'red'` 對空字串） | 1533 | 2192 | 約 +660 |
  | `c.textContent`（元素的，現算） | 189（`text.data`，DOM 留著的） | 1094 | 含算字串本身 |

  量 wrapper 時想量「第二次走同一批節點」當對照，結果和第一次一樣（1673）：兩趟之間 wrapper 被 V8 收掉、第二趟又重建，那個對照不成立，用 `parentNode` 的 128 當對照。這本身要另外看（沒有被 JS 留著的 wrapper 多久被收、重建的成本）。

**第 4k 段，研究：每一類多出來的成本拆開**（2026-11-12；取樣的比例換算成條數，是近似）：

- **字串參數**（`InfoFront::string_arg` 借不到字元時 → `converted_string_arg` → `with_front` → `strings::arg_string`：`to_rust_string_lossy` 複製成 Rust 的 `String`）：
  - cons string（`getAttribute('data-attribute-name-' + (i & 7))`，+936）：V8 攤平（`ValueView` 的建構約 30%、配置新的扁平字串和 `WriteToFlat` 約 30%）佔大半，這部分 Blink 也要付（它也要攤平）；我們加的是攤平之後再複製成 `String`（`to_rust_string_lossy`、`arg_string` 約 17%，約 260 條）和 `with_front` 62 條。攤平之後那個字串已經是扁平的 one-byte，可以像一般的 ASCII 字串原地借，不用複製。
  - 非 ASCII（Latin-1 的 `é`，+516）：`ValueView` 約 200、Latin-1 轉 UTF-8 約 90、`arg_string` 和複製約 120、malloc。DOM 的字串是 UTF-8，轉碼是這個選擇的成本（Blink 的字串和 V8 同樣是 Latin-1 或 UTF-16，不轉碼）；能省的是 malloc（短的轉進 stack 上的 buffer）和 `with_front`。
  - 數字轉字串（+286）：V8 的 `ToString`，轉出來是扁平的、原地借；少見，不追。
- **回傳非整數的數字**（`ret_f64` → `finish_number` → `with_front` → `finish_kept_as`：`Box::new(Kept::F64)`、建整個 `V8Front`、再 `set_double`）：固定回傳 1.5 的 getter（`ctx.lineWidth`）481，回傳 1 的 160。原型（`prototypes/number-result-direct.patch`：`finish_number` 直接對回傳值 `set_double`，量完還原）：481 → 303。多出來的 321 條裡約 180 條是我們的（一次 malloc 和 free、`V8Front`），剩下約 140 是 scope 27 和 V8 配置 heap number。`ev.timeStamp` 每次跑的值不同（有時是整數），不能當固定的對照。
- **新的 wrapper**（`cur.nextSibling`，節點沒有 wrapper，約 +1550；`finish_node` → `with_front` → `finish_kept_as(Kept::Node(root, interface))` → `new_wrapper_in_realm`）：取樣裡 V8 把 C++ 指標登記進 sandbox 的表（`CppHeapPointerTable` 的 `TryAllocateEntry`、`WriteLazilyInitializedCppHeapPointerField`）最大，其次是 `with_front` 自己（inline 了建物件）、`finish_kept_as`（`Box`）、`finish_kept`、memmove。V8 的部分 Chrome（有 sandbox）一樣要付；我們加的有節點的 root（約 110）、`Box`、`V8Front`、多的操作。這一類的取樣混了建 20 萬個節點的 `innerHTML`，比例只能看大概，要再做乾淨的量測。
- **現算的字串**（`style.color` 是 `'red'`，2192）：malloc 和 free 合計約 15%（約 330 條）：序列化成 `String`、轉成 `DomString`（再配置一次、複製）、`Box<Kept>`。V8 建字串本身只佔 1%。另外 `Shorthand::from_name` 佔 17%（約 370 條）：照名稱找 CSS property 的表，是 CSS 那一側的事，不是 binding。
- 共同的型態：這些路上我們自己加的成本是 **heap 配置**（`Box<Kept>`、`String`、`DomString`）、**整個 `V8Front`**（62 條）、**root**（約 110 條）和**多開的操作**，不是 scope。

**第 4k 段，研究：同一段 script 在 Chrome 和 machopo 各要多久**（2026-11-12；`prototypes/chrome-compare.js`：每個呼叫一百萬次、`performance.now()` 計時、五輪取最小，ns 一次。Chrome 是這台機器的 Fedora 的 Chromium，`--headless=new` 開 `file:` 的頁面；machopo 是 `bench_calls` 的 perf binary 在 window 的 realm 裡跑同一份。兩邊的 V8 版本和 build 設定不同、Chrome 是完整的瀏覽器，只看比例）：

| 呼叫 | Chrome | machopo | machopo / Chrome |
| --- | ---: | ---: | ---: |
| `n.nodeType` | 7.4 | 10.1 | 1.36 |
| `c.parentNode` | 9.6 | 11.5 | 1.20 |
| `n.getAttribute('id')` | 19.7 | 24.6 | 1.25 |
| `n.setAttribute('a', '…')`（扁平的 ASCII） | 55.4 | 43.2 | 0.78 |
| `setAttribute`，長的串接字串（扣掉串接） | 157 | 71 | 0.45 |
| `getAttribute`，長的串接字串（扣掉串接） | 107 | 54 | 0.51 |
| `setAttribute`，非 ASCII | 55.2 | 72.9 | 1.32 |
| `ctx.lineWidth`（值 1） | 5.6 | 7.5 | 1.34 |
| `ctx.lineWidth`（值 1.5） | 11.4 | 26.5 | 2.32 |
| `st.color`（`'red'`，現算的字串） | 45.7 | 112.2 | 2.46 |
| `tw.parentNode()` | 10.1 | 24.0 | 2.38 |
| `tw.currentNode = n; tw.nextNode()` | 26.4 | 37.2 | 1.41 |
| `tw.currentNode = n` | 11.6 | 14.5 | 1.25 |
| `t2.dispatchEvent(ev)`，沒有 listener | 91.8 | 69.0 | 0.75 |
| `t.dispatchEvent(ev)`，一個 listener | 288.9 | 185.6 | 0.64 |
| `new Event('x')` | 104.9 | 110.5 | 1.05 |
| `new MouseEvent('x', { clientX: 1 })` | 751 | 943.5 | 1.26 |
| 走過 20 萬個沒有 wrapper 的節點（`nextSibling`，一個節點） | 44 | 62.5 | 1.42 |
| 再走一次（wrapper 還在） | 9 | 9 | 1.00 |
| 全域的 `navigator` | 94.7 | 88.3 | 0.93 |
| 全域的 `location` | 128.6 | 100.4 | 0.78 |
| 全域的 `performance` | 93.8 | 66.6 | 0.71 |
| 全域的 `innerWidth` | 103.5 | 60.1 | 0.58 |
| `performance.now()` | 159.7 | 132 | 0.83 |
| 全域的 `document` | 0.5 | 0.5 | |

- **不是差距的**（先前以為要查的）：window 的 getter 當全域變數讀，Chrome 一樣慢（V8 的 `LoadSlow`，兩邊都付）；`dispatchEvent` machopo 比 Chrome 快；長的串接字串 machopo 快一倍（Chrome 也要攤平，之後還要做成它自己的字串）。
- **是差距的，照比例排**：現算的字串結果（2.46 倍）、會呼叫 script 的成員的外殼（`tw.parentNode()` 2.38 倍）、回傳非整數的數字（2.32 倍）、新的 wrapper（1.42 倍）、非 ASCII 的字串參數（1.32 倍）、帶 dictionary 的 constructor（1.26 倍），以及最熱的那條路本身（`nodeType` 1.36 倍、`getAttribute` 1.25 倍、`parentNode` 1.20 倍）。
- 先前說「兩趟之間 wrapper 被收掉、第二趟又重建」沒有重現：在 window 的 realm 裡第二趟是 9 ns，和 Chrome 一樣。那次是用三個不同長度的 script 的指令數相減量的，方法有問題，那個疑慮收回。

**第 4k 段，研究：每條冷的路要呼叫哪些 V8 API、rusty_v8 要不要 scope**（2026-11-12；讀 `dom_js` 的 `front.rs`、`templates.rs`、`strings.rs` 和 fork 的 `src/*.rs`）：

| 路 | 該呼叫的 V8 API | 現在多做的 | rusty_v8 的簽名 |
| --- | --- | --- | --- |
| 回傳非整數的數字 | `ReturnValue::Set(double)` | `Box<Kept>`、整個 `V8Front` | 不要 scope（`ReturnValue::set_double`） |
| 回傳新的字串 | `String::NewFromUtf8`（或 one-byte）、`ReturnValue::Set` | 字串先變成 `String` 再變成 `DomString`、`Box<Kept>`、`V8Front`；進快取的再加 weak handle 和 finalizer | 要 scope，只拿它的 isolate |
| 回傳沒有 wrapper 的節點或物件 | 目前的 context、複製樣板物件（fork 的 `clone_object`）、wrap、設 `TracedReference`、`ReturnValue::Set` | 節點的 root、`Box<Kept>`、`V8Front`、每次一個讀的操作比對 realm（`other_realm`）、建和接上各一個操作 | `clone_object` 要 scope；wrap、設邊已經不要 |
| 字串參數（不是字串） | `Value::ToString` | `V8Front` | 要 scope（isolate 和 context） |
| 字串參數（cons、非 ASCII） | 攤平或讀字元（`String::ValueView`、`WriteUtf8`） | 複製成 `String`（malloc）、`V8Front` | `to_rust_string_lossy` 要 scope；`ValueView` 只要 isolate |
| 數字、布林參數（不是數字） | `Value::NumberValue`、`Int32Value`、`BooleanValue` | `V8Front` | 要 scope |
| dictionary | 每個成員一次 `Object::Get`，再照型別轉換 | `V8Front`（一個 dictionary 一次） | 要 scope |
| 丟例外 | `Exception::TypeError`（訊息的字串）或建 `DOMException` 的 wrapper、`Isolate::ThrowException` | `V8Front` | 要 scope |
| 回報 heap | fork 的 `report_embedder_used_size` | scope | 只要 isolate |
| 呼叫 JS（listener、filter） | `Function::Call`、`TryCatch` | `V8Front`、`this` 和參數的 root、多開的操作 | 要 scope |
| 陣列、promise | `Array::New`、`Promise::Resolver::New`、`Resolve` | `V8Front` | 要 scope |

- rusty_v8 要 scope 的地方，C++ 那一側要的只是 isolate 和（有些）context；scope 在簽名裡是「現在可以建 handle」的憑證。callback 裡 V8 已經開好 handle scope，`StateInfo::with_scope` 的 `SAFETY` 靠的就是這一點。
- 不建 scope 而守住現有的保證（拿著狀態時不能配置）：憑證改成從 `StateInfo` 借出來的零大小的 token（借著它時借不到狀態，和現在 scope 佔住 `StateInfo` 是同一條借用規則），fork 的函式拿 token 而不是 scope。

**第 4k-A 段：回傳非整數的數字**（2026-11-12，決定 42）：

- `bindings`：`InfoState::return_number`（function 的 info 和兩種 property 的 info 各一份）：`ReturnValue::set_double`，V8 的回傳值自己建 heap number，不建 scope；借整個 info，所以拿著狀態時呼叫不到；`soundness.md` 補一句。
- `dom_js`：`InfoFront::finish_number` 只呼叫它（原本是 `with_front`、`Box<Kept>`、`V8Front`），不再是 `#[cold]`。走這條路的：回傳 double 的 getter 154 個、method 6 個（值不是整數、-0、NaN、無限大時），`u32`、`i32` 超過 31 bits 時。property 的 info 的那兩份現在沒有成員走到（indexed、named 的 getter 沒有回傳數字的）。
- `chrome-compare`（`prototypes/chrome-compare.py`：Chrome 和兩個 machopo 的 binary 各跑三次取最小、釘在一個核心上；ns 一次，括號是 Chrome 的倍數）：

  | | Chrome | 之前 | 這一段 |
  | --- | ---: | ---: | ---: |
  | `ctx.lineWidth`（1.5） | 11.7 | 26.8（2.29） | 14.9（1.27） |
  | `ctx.lineWidth`（1） | 5.7 | 7.7（1.35） | 7.6（1.33） |
  | `performance.now()` | 106.8 | 132.1（1.24） | 120.1（1.12） |
  | `n.nodeType` | 3.5 | 4.8（1.37） | 4.5（1.29） |

  驗收的 1.2 倍沒有到：`lineWidth`（1.5）剩下的 1.27 倍就是整數那一條的 1.33 倍，也就是最熱的路本身的差距（4k-G），這一段自己多出來的已經沒有了（非整數比整數多 7.3 ns，Chrome 多 6.0 ns）。
- 計時的雜訊：不釘核心、只跑一次時同一個呼叫差到兩倍（`nodeType` 10.1 和 4.5）；之後的對照都用 `chrome-compare.py`。
- 測試：`dom_js` 的 `handlers::a_number_that_is_no_small_integer_is_returned_with_no_scope`（分數、浮點的誤差照舊、整數、-0、負的分數、很大的數、超過 31 bits 的 `unsigned long`、NaN：值正確、都不建 scope）；`a_constructor_is_given_the_info_alone` 的 `timeStamp` 那一項改成不建 scope。
- mutation test：`python3 scripts/test.py mutate rfcs/0097-binding-call-cost/mutants-4kA.toml`，3 個都抓到。
- 跑過：`cargo test -p bindings -p dom_api -p dom_js -p dom_wasm`、`sanity.py` 七項、`cargo fmt --check`、`test.py wpt html/canvas/element dom/events hr-time`（3359 個結果都照預期）。
