# RFC 0097 的細節：binding 每次呼叫的成本往 Chrome 靠

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 2026-11-11 的量測

Linux（Fedora 44），AMD Ryzen AI 9 HX 370，接電源；machopo 的 `perf` profile，Chrome for Testing 152.0.7977.82。

### 每次呼叫的時間（blink-perf 換算）

2026-11-10 的 `bench.py blink-perf --pages …`（RFC 0094 的那 26 頁），runs/s 乘上頁面迴圈的次數換成每次呼叫的 ns；兩邊都含 JS 的迴圈本身：

| 成員（頁、迴圈次數） | machopo | Chrome | 差 |
| --- | ---: | ---: | ---: |
| `nodeType`（`bindings/node-type`，10 萬） | 6.4 | 3.5 | 2.9 |
| `firstChild`（`first-child`，10 萬） | 7.4 | 4.2 | 3.2 |
| `id`（`id-getter`，10 萬） | 14.0 | 8.3 | 5.7 |
| `getAttribute("id")`（`get-attribute`，10 萬） | 31.4 | 21.1 | 10.3 |
| `id = "foo"`（`id-setter`，5 萬） | 44.2 | 27.9 | 16.4 |
| `setAttribute("id", "foo")`（`set-attribute`，1 萬） | 60.1 | 42.8 | 17.2 |
| `getElementById("foo")`（`get-element-by-id`，10 萬） | 38.2 | 17.4 | 20.8 |
| `getElementsByTagName("div")`（5 萬） | 33.8 | 17.6 | 16.3 |
| `createElement("div")`（`create-element`，5000） | 79.4 | 55.4 | 24.0 |

`append-child`、`insert-before` machopo 快 2 倍以上，工具標成可疑，不比。

### 每次呼叫的指令數（`bench.py calls`）

machopo `3e3e410`，每個成員呼叫一百萬次、扣掉空迴圈，3 次的中位數；指令數（cycle）[store forwarding 的衝突]：

| 成員 | |
| --- | ---: |
| `nodeType` | 153（30）[0.04] |
| `firstChild` | 178（33）[0.10] |
| `getAttribute` | 706（146）[2.10] |
| `setAttribute` | 1270（244）[8.40] |
| `appendChild` | 1348（230）[1.57] |
| `createElement` | 2099（416）[4.66] |
| `addEventListener` | 898（183）[2.25] |
| `dispatchEvent` | 3497（727）[14.41] |

V8 呼叫 API callback 的地板加 trampoline 約 50 條指令、11 cycle（RFC 0085 決定 13 量的）：`nodeType` machopo 自己的部分約 19 cycle；Chrome 整個 `nodeType` 約 17 cycle（上面的 3.5 ns），自己的部分約 6。

### Speedometer 取樣裡 binding 的函式

2026-11-10 的取樣（`docs/performance-gaps.md`），每個 iteration 的 ms：`V8Front::finish_kept` 1.8（bindings）加 2.9（V8 API）、`StringCache::insert` 1.7、rusty_v8 的 callback 外殼（`c_fn`）1.6、1.2、1.1、`V8Op::keep` 1.1、`ArgString::text` 1.1、`call_listener` 1.0、`call_callback` 1.6、`templates::new_wrapper` 3.5；DOM 那一類裡 `names::find_fixed` 4.7、`intern_value` 4.4。

## 一次呼叫現在走的路

（2026-11-11 讀 `crates/dom_js/src/front.rs`、`templates.rs`、`strings.rs`，`crates/bindings/src/isolate_state.rs`、`unified_heap.rs`，elise 的 `heap.rs`，rusty_v8 的 `function.rs`。）

1. **rusty_v8 的外殼**（`MapFnFrom` 的 `mapping`）：建 `CallbackScope`（pin、`init`），從 `FunctionCallbackInfo` 做出 `FunctionCallbackArguments` 和 `ReturnValue`。
2. **`call_member`**：`V8Front::new`（`Call` 的 7 個欄位：`rv`、`kept`、`prepared`、`window`、`callback`、`rest`、`rare`），`M::call(&mut front)`，不是 getter 的之後 `report`（排 microtask、DOM 長大超過 `REPORT_BYTES` 時告訴 V8）。
3. **操作之前**：參數的轉換（`string_arg` 是 WebIDL 的 `ToString`，可能執行 script；純 ASCII 的一位元組字串不複製、之後借用字元，其他複製成 Rust 的字串）；`[NewObject]` 的成員先建 wrapper（`prepared`）。
4. **操作**（`V8Front::read`、`mutate`）：從 scope 借出 isolate 的狀態（不檢查的一次讀取）和 `NoGc`；`read` 在 elise 只是建一個 `Mutation`（沒有安全點），`mutate` 先 `safepoint()`。操作裡 unwrap receiver 和節點參數（比型別的 tag、cage）、呼叫 DOM、回傳：
   - Smi、布林、null：直接寫 `rv`。
   - 有 wrapper 的節點：從節點的 JS 邊寫 `rv`，不建 local（`return_wrapper`）。
   - 名稱：每個名稱的 V8 字串建一次、放在 global，直接回傳。
   - 字串：用位址查 `StringCache`，有就回傳 weak handle 指的；沒有就留著（`Kept::String`）。
   - 沒有 wrapper 的節點、放不進 Smi 的數字、序列：留著（`Kept`，節點是 root）。
5. **操作之後**（`finish_kept`）：留著的變成 V8 的：建字串（複製）加 weak global（帶 finalizer）放進快取；建 wrapper、再開一個操作 wrap。

`Call` 的欄位在「V8 能在操作裡配置」之後：`rv` 留著；`kept`、`prepared` 不需要；`window`（建構子要的 realm 的 global）可以用到時才找；`callback`、`rest` 只是存 V8 的 handle，可以用到時從參數讀；`rare` 平常是空的。

## 模型

`tla/Unified.tla` 是 elise 的 `tla/unified/Unified.tla`（elise `3bda47c`）加上：

- `snap`：elise 拿了快照沒有。elise 的 barrier、black allocation、black promotion 和 marker 都要 `snap`；現在的協定裡 `snap` 和「marking 在進行」同時成立，行為不變（`none` 的狀態數和檢查照舊成立）。
- `InOp` 的版本（`Mutation = "inop"` 和拿掉一條規則的四個）：`Wrap` 可以在操作裡、對操作拿著的節點做；`JsScavenge` 可以在操作裡；`StartMarkingInOp`（V8 那一側開始，elise 的快照等 `TakeSnapshot`：`locals = {}`、nursery 空的）；`BeginFinishInOp`（進入停頓，所有 old 標成活的）。

`python3 rfcs/0097-binding-call-cost/tla/check.py`（TLC 1.7.4，輸出在 `target/tla-0097/`）。2026-11-11 的結果，檢查 `TypeOK`、`MarkingComplete`、`NoDangling`、`WrappedNotYoung`、`ExpandosKept`：

| 大小（DOM 物件 × JS 物件 × 欄位） | 版本 | 結果 | 狀態數 |
| --- | --- | --- | ---: |
| 2×2×1 | `none`（現在的協定） | 成立 | 7,497,067 |
| 2×2×1 | `inop`（放寬加三條規則） | 成立 | 12,560,530 |
| 2×2×2 | `inop` | 成立（713 秒） | 147,635,654 |
| 2×2×1 | `inop_snapshot_at_once`（拿掉規則 1） | `MarkingComplete` 不成立 | |
| 2×2×1 | `inop_plain_finish`（拿掉規則 2） | `MarkingComplete` 不成立 | |
| 2×2×1 | `inop_no_wrap_barrier`（拿掉規則 3） | `MarkingComplete` 不成立 | |
| 2×2×1 | `inop_scan_before_snapshot`（快照前就掃描交來的物件） | `MarkingComplete` 不成立 | |

找到反例的那幾列不列狀態數：TLC 平行地找，停在第一個反例，每次跑到的數目不同。

反例（TLC 走的動作）：

- 拿掉規則 1：`Alloc`（N 只在操作裡）→ `StartMarkingInOp`（快照只有 root）→ `AddRoot`（N 變成 root，SATB 不標新加的）→ `EndOperation` → `BeginFinish`：N 到得了、沒標到。
- 拿掉規則 2：`Alloc` → `StartMarkingInOp` → `BeginFinishInOp`：操作拿著的 N 沒標到。
- 拿掉規則 3：`Alloc` → `StartMarkingInOp` → `Wrap`（N 的 wrapper 是黑的）→ `EndOperation` → `TakeSnapshot` → `BeginFinish`：script 拿著 wrapper，N 沒標到。
- 快照前掃描：`Alloc`、`Alloc` → `StartMarkingInOp` → `Wrap` → 掃完那個節點 → `Write`（往它寫一條邊，沒有 barrier）→ `EndOperation` → `TakeSnapshot` → `BeginFinish`。

2×2×2 第一次跑時 TLC 的佇列寫滿 `/tmp`（tmpfs）而失敗，不是模型的結果；輸出改放 `target/` 之後跑完。

**模型沒有的**：weak、ephemeron 和 id 表（elise 的 `tla/marking/SatbYoung.tla` 有 weak 和 ephemeron，沒有這份的放寬；規則 2 下所有 old 都活著，沒有東西要清，只是推理）；Rust 的借用（scavenge 在操作裡重設節點的 JS 邊，同時操作拿著那個節點的參照）；規則 2 的頻率和成本；會執行 script 的呼叫（不放寬）。

## Blink 的做法

2026-11-11 查 Chromium `main` 的原始碼，`third_party/blink/renderer/platform/bindings/` 的 `script_wrappable.h`、`to_blink_string.cc`、`string_resource.h`、`v8_value_cache.cc`：

- **unwrap 照樣比 tag**：`ToScriptWrappable<T>` 呼叫 `v8::Object::Unwrap<ScriptWrappable>(isolate, wrapper, CppHeapPointerTagRange(this_tag, max_subclass_tag))`，V8 讀指標表時比 tag 落不落在那個型別和它的子類別的範圍裡，再 `static_cast`。「unsafe」指的是不再檢查 receiver 是不是那個 interface（signature 檢查過），不是不比 tag。所以原型 2 拿掉的三樣裡，比 tag 是 Blink 也付的（它比的是範圍），machopo 多的是 null 的檢查和 cage 的檢查。
- **wrapper 在物件的固定位置**：`ScriptWrappable` 的 `TraceWrapperV8Reference<v8::Object> wrapper_`（「Inline storage for the a single wrapper reference」）。
- **字串進來**（`ToBlinkString`）：先看 V8 的字串是不是 external（`GetExternalizedString`）：是的話直接拿 resource 裡的 WTF 字串，要 `AtomicString` 時 resource 把它存起來（`GetAtomicString` 第一次算、之後回同一個），不複製、不再雜湊。不是的話複製一份（短的 `AtomicString` 用 stack 上 32 bytes 的緩衝區直接查表，不配置），而且 `kExternalize` 模式下 V8 說可以（`CanMakeExternal`）就把那個 V8 字串**當場改成 external**（`MakeExternal`），之後同一個字串再傳進來走上面那條路。
- **字串出去**（`StringCache::V8ExternalString`、`SetReturnValueFromString`）：`StringImpl*` 對 weak 的 `v8::Global<v8::String>` 的表，命中就直接寫回傳值；沒命中建 **external 字串**（`NewExternalOneByte`、`NewExternalTwoByte`，resource 拿著那個 `String`，`data()` 指向 `StringImpl` 自己的字元：不複製），放進表；weak callback 放掉 `StringImpl`。
- 生成的 callback 的樣子（receiver、參數、`ReturnValue`，不建 scope）沒有在原始碼裡查到：它是 build 時生成的，repo 裡沒有。

## 字串的呼叫現在花在哪

2026-11-11，`perf record`（指令數）各跑五百萬次，大部分 inline 在那個成員的 callback 裡，分得出來的：

- `getAttribute('id')`：`v8::Value::ToString`（參數已經是字串也經過一次 C++ 呼叫）6.4% 加 0.9%，`LentString::lends`（看能不能借字元）5.7%，`GetCurrentContext` 1.2%，`names::find_fixed` 1.0%，`ret_string` 1.1%。
- `setAttribute('a', 'b')`：`Cx::change_attribute` 26.6%、`attribute_changed` 12.2%（DOM 本身），`intern_value` 2.4% 加雜湊表的查找 1.7%、`memcmp` 2.6%，`lends` 1.3%，`GetCurrentContext` 1.2%。
- `id`：`ret_string` 10.1%，`attribute_value_by_id` 8.6%。
- `getElementById('x')`：`lends` 8.2%，`ArgString::text` 6.4%，`memcmp` 1.4%，值的表的查找 0.6%。

和 Blink 比，machopo 每次都做、Blink 第二次起不做的：參數的 `ToString` 的呼叫和借字元的判斷、把字元拿去查名稱表或值的表（雜湊、比對）。回傳時兩邊都是一次用位址查表；沒命中時 Blink 不複製字元。

## external string 在 machopo 的 V8 上做得到多少

2026-11-11 讀 rusty_v8 的 fork（`src/string.rs`、`src/binding.cc`）和 V8 的 `src/objects/string.cc`：

- **出去（DOM 的字串給 V8，不複製）做得到，不用改 fork**：`String::new_external_onebyte_raw`（字元的指標、長度、一個 destructor）建 external 的一位元組字串；`DomString` 的文字是 `Rc<str>`，指標和長度就是 `Rc::into_raw` 給的，destructor 用 `Rc::from_raw` 放掉那一份計數。只有 Latin-1（ASCII）的字串可以；兩位元組的要另外轉。
- **進來時認出自己的 external 字串做得到**：`get_external_onebyte_string_resource` 拿到 resource，它的 `data()` 就是 `DomString` 的文字的位址，也就是 `StringCache` 的 key：查到那一筆就有 `DomString`，不讀字元、不雜湊內容。要分辨是不是 machopo 建的 resource（位址在不在快取裡就夠）。
- **把 JS 給的字串當場改成 external（`MakeExternal`）fork 沒有接**，要加 C++（重編 V8）。而且在 machopo 的 V8 的設定下短字串改不了：`String::SupportsExternalization` 在「有指標壓縮、沒有 sandbox」時，字串的大小小於 `UncachedExternalString`（header 12 bytes 加一個 8 bytes 的指標，20 bytes）就回 false，一位元組的字串要 8 個字元以上；read-only space 和 young generation 的字串也不行。Chrome 開著 sandbox（external 的指標是 4 bytes 的 handle），沒有這個限制：`'id'`、`'div'` 這種短的名稱 Blink 第一次用就改成 external，之後不再雜湊；machopo 要等 V8 的 sandbox 打開才做得到。
- 所以 benchmark 裡那些短的名稱（`'id'`、`'a'`、`'foo'`、`'div'`）在 machopo 上每次還是要拿字元去查表；能做的是讓查表本身便宜（V8 的 internalized 字串存著算好的 hash，可以不再算一次），和上面「出去不複製」。

## 參考資料

- V8 的 `src/heap/heap-write-barrier.cc`（`MarkingSlowFromCppHeapWrappable`，fork 的 `machopo:` 註解）、`src/heap/heap.cc`（`IncrementalMarkingLimitReached`）。
- RFC 0085 決定 13、14（`nodeType` 一層一層拿掉量；unwrap 不比型別延後）、RFC 0067（有型別的 bindings）、RFC 0087（elise 的設計：只在安全點收尾）、RFC 0094（`docs/performance-gaps.md` 的那次量測）。
