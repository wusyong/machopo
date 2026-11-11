# RFC 0097 的進度：binding 每次呼叫的成本往 Chrome 靠

開始日期：2026-11-11

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型和量測（不合併） | 完成 | 2026-11-11 |
| 2 字串參數已經是字串就不轉換 | 完成 | 2026-11-11 |
| 3 front 只拿 callback 的 info：參數和結果單純的成員 | 完成 | 2026-11-11 |
| 4 階段之間傳有型別的值，拿掉 `V8Front` 和 `Call` | 還沒開始 | |

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
