# RFC 0067：有型別的 bindings：每個成員一條專用的呼叫路徑

- 開始日期：2026-10-20
- 狀態：✅ 已完成：2026-10-21
- 進度和驗收：`progress.md`
- 細節：`details.md`（原型和每一段的量測、編譯的成本、4a 的 cached accessor 原型和跨源的發現）

## 摘要

從 JS（和 wasm）呼叫 DOM 時，kun 原本每個 API 都走同一條通用的路：照編號查表、照 tag 一種一種試著 unwrap `this`、沿著 parent 鏈比字串確認介面、參數轉成 `Vec<Value>`、經過 `dyn` 和函式指標、`this` 和回傳值各建一個 root、結果包成 `Value` 再轉回去。Blink 的 bindings 產生器則替每個成員生成一個專用的 V8 callback。這份 RFC 讓宣告表（`declare!`）替每個成員生成一個實作 `TypedMember` 的型別：和 engine 無關的一次呼叫，參數和結果照各自的型別轉換（`Front` 由前端實作）。`dom_js` 替每個成員建一個 V8 callback（`v8_callback::<M>`，帶介面的 `Signature`），`dom_wasm` 用 wasmtime 有型別的 `func_wrap`（照簽名去重）。讀的成員不 root `this`，回傳已有 wrapper 的節點也不 root。通用的路留著，給還不接的成員（overload、回傳 promise、可變參數、`glue.js` 建的、event handler 屬性……）。

結果（`progress.md`、`details.md`）：讀的成員每次呼叫快 2–4 倍；Speedometer 的 script 比 Chrome 慢的部分 220 → 181 ms，減少的幾乎都在 bindings；wasm 每次呼叫的指令數少兩到五成（`createElement` 23%、`nodeType` 47%）。做的時候找到並修好一個跨源的漏洞：自己 realm 的成員用在別的 origin 的 window 上不會被擋下（第 26 條）。第 4 段只做了 `Window` 的成員（4a），其餘的移到「之後的可能」（第 27 條）。

## 動機

- **script 比 Chrome 慢的部分，幾乎全在呼叫 DOM**（`docs/performance-gaps.md`，2026-10-20，Speedometer 的 20 個 suite）：kun 的 script 1234.7 ms 裡，JS 本身 662.8 ms、呼叫 DOM 571.9 ms；Chrome 的呼叫 DOM 估計約 351 ms，**kun 多花約 221 ms**。kun 的呼叫 DOM 裡：bindings 的通用路徑 131.5 ms、V8 API 63.4 ms、配置 52.5 ms、root 46.0 ms、字串 36.4 ms。
- **幾乎不做事的呼叫慢 7 到 20 倍**（blink_perf，兩個瀏覽器綁在同一組快核上）：`first-child` 13.3 倍、`node-type` 13.2 倍、`get-element-by-id` 15.7 倍、`create-element` 11.7 倍、`get-attribute` 9.4 倍、`set-attribute` 7.4 倍；本身做比較多事的才接近（`append-child` 1.5 倍）。對 `first-child` 取樣，一次呼叫約 95% 在呼叫路徑上（`dom_js::templates::call_member` 44%、V8 API 24%、`dom_api` 的分派 18%、進 V8 的 callback 9%、root 4.5%），讀 `firstChild` 本身幾乎是 0。
- **為什麼之前評估過、決定不做，現在要重做**：RFC 0061 的第 3 段量過「每個成員一個專用的 thunk」，只省得到參數和回傳值經過 `Value` 的那一段（約 20–25 ms），入口、分派、receiver、root 都不變，所以不做。這份不只是 thunk：它一起拿掉 receiver 的查法、root、入口的查表和 isolate 狀態的查找，而且這次的量法把 V8 API、配置和字串也算進呼叫路徑（約 300 ms 對 RFC 0061 量的 235 ms）。RFC 0061 的「之後的可能」也寫了「等 bindings 成為比值的主要部分時再考慮」：現在 JS 本身和 Chrome 差不多，差距主要就在這裡。
- **wasm 一起受惠**：`dom_wasm` 現在也是通用的路（`func_new` 收動態的 `&[Val]`、`lift_all` 成 `Vec<Value>`、函式指標、`lower` 回 `Val`）。有型別的核心入口和 engine 無關，wasm 之後可以改用 wasmtime 的有型別函式接上。

目標：

- 常用、形狀簡單的成員（getter、setter、參數是基本型別或介面的方法）有專用的路：不查表、`this` 一次 unwrap 成那個介面的型別、不經過 `Value` 和 `Vec`、不經過 `dyn`、`this` 不 root、回傳已有 wrapper 的節點時不 root。
- blink_perf 的 `bindings/` 頁明顯變快（驗收的頁和門檻在第 1 段的原型之後定），Speedometer 的 script 裡「呼叫 DOM」那一部分變少。
- 宣告表仍然是唯一的來源：同一份宣告產生 JS 的入口、wasm 的 WIT 和兩邊的轉接層。
- 語意不變：WebIDL 的轉換順序和錯誤、`[CEReactions]`、`after_call`、overload、例外和 Promise。

不在範圍內：

- DOM 操作本身的成本（插入、移除、屬性的資料結構），style、文字、canvas。
- V8 的 Fast API call（RFC 0061 第 3 段的理由仍然成立；這份做完後如果 canvas 的呼叫還是熱點再看）。
- 字串的快取（Blink 的 `StringCache`，`docs/performance-gaps.md` 的方向 2）：另一份 RFC；這份只讓字串不多轉一次。
- 另寫一個 WebIDL 的產生器（Blink 的 `bind_gen`）：用現在的 `declare!` macro 生成（第 2 條）。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

對寫 DOM API 的人，宣告的寫法幾乎不變；只讀的成員在宣告裡標出來，實作拿 read scope 和節點的參照：

```rust
read attribute firstChild: Option<Node> = node::first_child_in;

pub fn first_child_in<'r>(_: &DomReadScope<'_>, node: NodeRef<'r>) -> Option<NodeRef<'r>> {
    node.first_child()
}
```

`declare!` 替每個成員生成：

- **成員的型別和它的呼叫**（`dom_api` 的 `typed`，和 engine 無關）：每個介面一個 `M<J>`（`typed_paths::Node::M<12>`，`J` 是成員在介面的 `members` 裡的 index，由 const fn 算出），實作 `TypedMember`：`call::<F: Front>` 數參數、拿 `this`、照型別轉參數、在 `[CEReactions]` 裡呼叫函式和 `after_call`、交回結果；`call_with` 收已轉好的參數（`Args`），直接回傳結果（`Ret`）。走訪器 `visit_typed` 對每個成員呼叫前端一次。
- **V8 的轉接**（`dom_js` 的 `typed`）：`V8Front` 實作 `Front`；建 templates 時替接的成員各建一個 `FunctionTemplate`，callback 是 `v8_callback::<M>`，帶介面的 template 的 `Signature`（錯的 `this` 由 V8 擋下），`set_accept_any_receiver(false)`（跨源的 `this` 由 V8 擋下）。建 templates 的程式不泛型，每個成員只具現化它的呼叫本身（第 15 條）。
- **wasm 的轉接**（`dom_wasm` 的 `typed`）：每種型別在呼叫邊界上的形狀（`ApiType::Wire`：字串、數字、布林、物件、可為 null 的、`()`，其他是 `Complex`）對應到 wasmtime 的型別，參數和回傳都不是 `Complex` 的成員用 typed 的 `func_wrap`，同簽名的共用一份（約 120 種）。每個成員的型別列在 `src/typed_list.rs`，由測試從宣告表生成（第 21 條）。

沒接的成員照舊走通用的路，呼叫同一個 `dom_api` 的函式。

## 詳細設計

### 1. 每次呼叫的固定成本，和誰拿掉它

| 成本 | 原本 | 專用的路 |
| --- | --- | --- |
| 找成員 | callback 的 data 是編號，`decode` 加查表，經過 `member.call` 的函式指標和 `dyn ScriptHost` | 每個成員一個 callback（`v8_callback::<M>`），直接呼叫那個成員的 `call` |
| isolate 的狀態 | `BindingContext::new` 在 isolate 的 slot 裡找 | 不變：改用私有的 thread-local 先讀，同一個 binary 量不出差別（第 4 條、`progress.md`） |
| receiver | 沒有 `Signature`；照 tag 一種一種試 unwrap；`check_this` 沿 parent 鏈比字串 | V8 的 `Signature` 擋錯的 receiver；只用那個介面的物件可能有的 tag unwrap（預先算好的表） |
| `this` 的 root | `unwrap` 時建、呼叫完放掉 | 讀的成員（`read attribute`、`read method`）不 root：在 read scope 裡用 `NodeRef`（`BindingContext::read_node_into`）；其他成員照樣 root |
| 參數 | `Vec<Value>`、`Value` 的 enum、再 `from_value` | `ApiType::take_arg`：字串、數字、布林、可為 null 的和介面直接轉成 Rust 型別（`Front::string_arg`、`object_arg`……），其他預設照舊經過 `Value`（第 12 條）；照樣在開 scope 之前轉（I13） |
| 回傳的節點 | `to_handle()` 建 root、包成 `Value`、`to_js` 再找 wrapper | 在 read scope 裡看它有沒有 wrapper：有就直接拿，不 root；沒有才 root，關 scope 後建 wrapper |
| 其他 | `after_call`、`[CEReactions]` | 照舊，生成時照成員的種類決定要不要呼叫；wasm 的 reaction 不在呼叫裡跑（`Front::CE_REACTIONS`） |

讀的成員每次呼叫的指令數：`nodeType` 209（`bench.py calls`）；mutation 的成員只快 1.1–1.45 倍，剩下的是 root 和 DOM 那一側的成本（`details.md`）。

### 2. soundness

- 參數的轉換可能執行 JS、在 V8 配置，照舊在開 DOM scope 之前（I13）。
- `this` 和回傳值不 root：在 read scope 裡只讀，到 scope 關掉之前不呼叫任何會在 V8 或 cppgc 配置的東西（I1、I2）。「看回傳值有沒有 wrapper」只讀 `TracedReference`，不配置；沒有 wrapper 時先 root 再關 scope。
- wasm 的讀的成員從 Store 的 slot 拿節點（`DomReadScope::held_node`）：slot 表由 holder trace，slot 放掉之前節點一定活著。
- 論證寫在 `crates/dom/soundness.md`。

### 3. 和通用路徑的關係

- 兩條路呼叫同一個 `dom_api` 的函式；不同的是前端的轉換，由測試、mutation test 和 WPT 確認一樣（第 14 條：不做執行時的交叉檢查）。`testing::shared_calls`（`dom_js`、`dom_wasm`）數經過共用路徑的呼叫，測試用它確認接的成員不走共用的路。
- JS 不接的：overload（照參數選形式）、回傳 promise 的、`FrozenArray` 和 observable array 的、可變參數、只有 WIT 有的、`glue.js` 建 JS 形式的。wasm 不接的：`async`、回傳 promise 的、`static` 的、只給 JS 的、參數或回傳有 `Complex` 形狀的（照舊 `func_new`）。

### 4. `Window` 的成員和跨源的 `this`

- `[Global]` 的成員也走專用的路：`this` 是 global proxy（`undefined` 由 V8 換成它），`convert::object_of` 照樣拿到 window（第 24 條）。V8 的 IC 不 cache global object（dictionary 模式）上的 API getter，`document` 每次仍約 2500 個指令；Blink 的 `[CachedAccessor]` 量過、不做（第 25 條）。
- 成員的 function 都設 `set_accept_any_receiver(false)`（`templates::checks_receiver`），照 Blink 的 `IDLMemberInstaller`：V8 呼叫前檢查 `this` 能不能存取，跨源的 window 是 `TypeError`；static 的方法（包括 namespace 的）不設（第 26 條）。fork 接出這個 API，發成 `kun-v152.2.0-13`。

### 測試

測了什麼、怎麼驗證在 `progress.md` 的「驗收」。

### 分階段

1. **原型和決定形狀**：手寫六個成員的專用路徑（`Node.firstChild`、`nodeType`、`appendChild`，`Element.getAttribute`、`setAttribute`，`Document.createElement`），量每一項各省多少，決定核心入口的形狀（第 5–10 條）。
2. **生成**：`declare!` 替每種形狀的成員生成，V8 的轉接（第 11–15 條）。
3. **wasm 的轉接**：typed 的 `func_wrap`、`bench.py calls`（第 16–21 條）。
4. **擴大範圍**：分成四小段（第 22、23 條），只做了 4a（`Window` 的成員，第 24–26 條）；4b–4d 移到「之後的可能」（第 27 條）。

## 缺點

- 編譯時間：release 的 `dom_js` 6 → 14–17 秒、`dom_wasm` 10.5 → 26.5 秒；dev 下加一個成員 13.6 → 28 秒（`dom_js`）、19.8 → 41 秒（`dom_wasm`）。新增每個成員一份的泛型程式前先量（`crates/dom_js/README.md`）。
- 兩條路並存：呼叫同一個函式，前端的轉換靠測試保持一致。
- 讀的成員的實作簽名不同（`&DomReadScope` 和 `NodeRef`），不 root 的 `this` 和回傳值多了安全論證要維護。
- wasm 的成員清單（`src/typed_list.rs`）是生成的檔案，宣告表改了要重寫（測試會提醒；過時的清單照樣編得過，那幾個成員暫時走共用的路）。

## 理由和其他做法

- **只做專用的 thunk**（RFC 0061 第 3 段評估過的）：只省參數和回傳的 `Value`，約 20–25 ms，不夠。
- **另寫產生器**（WebIDL 轉 Rust 的 `bind_gen`）：要兩份宣告保持一致；`declare!` 已經替每個成員展開一份程式，在它裡面生成比較省。
- **V8 的 Fast API call**：只有 TurboFan 編過的呼叫點會用，`extern "C"` 只能放在 `bindings`，RFC 0061 的理由仍然成立；可以在這份之後、在有型別的核心入口上再接。
- **保守掃描 stack 省掉 root**：RFC 0060 不選的理由仍然成立（Rust 的型別限制不了值放在哪裡、wasmtime 的 fiber stack）。

## 先例

- **Blink 的 `bind_gen`**：每個 attribute 和 operation 一個 V8 callback（`V8Node::FirstChildAttributeGetterCallback`），參數用 `NativeValueTraits`、回傳用 `ToV8Traits`，receiver 用 `FunctionTemplate` 的 `Signature` 和 `WrapperTypeInfo`，`[CEReactions]` 是 stack 上的 `CEReactionsScope`；回傳的 `Node*` 從 `ScriptWrappable` 身上直接拿 wrapper（`DOMDataStore`），不 root（Oilpan 保守掃描 stack）。
- **Servo 的 codegen**：WebIDL 生成每個成員的 JSNative，`this` 用 `DomRoot`。

## 已決定的事項

2026-10-20，草稿之後（使用者）：

1. **核心入口的實作拿 `&DomReadScope`（或 mutation scope）和 `NodeRef`，不經過 `&mut dyn ScriptHost`**。要改到的實作多，但 `this` 和回傳值的 root、`dyn` 的分派只有這樣才拿得掉。不選保留 `&mut dyn ScriptHost`、只改參數和回傳的型別：root 和 `dyn` 都還在，省的就只剩 RFC 0061 第 3 段估的那一段。前端的 trait 的細節在第 1 段的原型裡定。
2. **由 `declare!` 生成**，不另寫產生器。`declare!` 已經替每個成員展開一份程式，型別都在那裡；另寫一個 WebIDL 的產生器要兩份宣告保持一致（RFC 0061 的第 1 條）。
3. **第 1 段的原型手寫六個成員**：`Node.firstChild`、`Node.nodeType`、`Element.getAttribute`、`Element.setAttribute`、`Document.createElement`、`Node.appendChild`。涵蓋：沒有參數的 getter（回傳節點、回傳數字）、字串參數和回傳、有 `after_call` 的方法、建新節點（新 wrapper）、介面參數和 mutation。
4. **isolate 的狀態先放在 callback 的 data**（不再每次在 isolate 的 slot 裡照 `TypeId` 找），原型量過再定；不行再改 isolate 的固定 slot。

2026-10-21，第 1 段的原型之後（使用者）：

5. **核心入口放在 `dom_api`，和 engine 無關**：只拿 DOM 的 scope、節點和 Rust 的型別；V8 專屬的（`Signature`、`v8::ReturnValue`、建 wrapper）只在 `dom_js` 的轉接層，第 3 段的 wasm 呼叫同一批核心。
6. **第 2 段生成的成員**：參數和回傳都是簡單型別（數字、布林、字串、介面，可以是 null）的 getter、setter 和方法，沒有 overload、不是 async、不回傳 Promise。dictionary、union、callback、sequence 照舊走通用的路（第 4 段再看）。不選一次全部：生成的機制先在形狀簡單的成員上做對。
7. **只讀的成員在宣告裡標出來**（`read = …`）：macro 看不到實作的簽名，所以由宣告說；實作拿 `&DomReadScope` 和 `NodeRef`，走不 root 的路（原型的 `read_node`、`read_node_for_node`）。其他成員照樣 root `this`，但也不經過 `Value` 和分派。不是節點的 receiver 第 2 段一律用它的 tag unwrap、root；不 root 的讀取先只做節點。
8. **debug build 的交叉檢查**：只讀的成員走完專用的路，再用通用的路算一次比對（和快取的交叉檢查同一套做法）；mutation 的成員不比（有副作用）。
9. **第 2 段還 root 的，在第 4 段研究**（mutation 成員的 `this`、不是節點的 receiver、節點參數、回傳的不是節點的物件，見「分階段」）：每一種要各自的論證和 `v8_engine.rs` 的原語，第 2 段只做生成的機制，範圍好審；第 4 段先量值不值得。原型量到 mutation 的成員只快 1.1–1.45 倍（`details.md`），剩下的是 root 和 DOM 那一側的成本。
10. **原型找到的 DOM 那一側的成本**：`setAttribute` 找屬性時不複製整份屬性，第 2 段順手做；`createElement` 在建立時就決定新元素的種類（第一次建 wrapper 時照名稱比字串，佔它的 16%），另外處理。

2026-10-21，第 2 段開始前（使用者）：

11. **生成的程式經過「每個成員一個型別，加上走訪器」接到前端**：`declare!` 替每個生成的成員產生一個零大小的型別，實作 `dom_api` 裡和 engine 無關的 trait（`TypedMember::call::<F: Front>`，`F` 是前端：拿參數、交回結果），和一個走訪器（`visit_typed::<V: TypedVisitor>`，對每個成員呼叫 `v.member::<M>(介面, 成員)`）。`dom_js` 的走訪器替每個成員建 `FunctionTemplate::builder(v8_callback::<M>)`：V8 直接呼叫每個成員專用的 callback，沒有查表和間接呼叫，和 Blink 生成的一樣；wasm 之後實作同一個走訪器。不選函式指標表加上共用的 trampoline：每次呼叫多讀一次 callback 的 data（FFI）和一次間接呼叫，正是原型省掉的那種成本。不選讓 `declare!` 產生一個列出成員的 macro 給 `dom_js` 展開：效果一樣，兩層 macro 難讀、難除錯。代價是每個成員、每個 engine 各具現化一份（編譯時間、binary 的大小，第 2 段量）。

2026-10-21，第 2 段接上生成的機制之後（使用者）：

12. **參數和回傳照型別各自轉換**：`ApiType` 多兩個有預設實作的方法，`take_arg` 拿參數、`give` 交回結果，預設照舊經過 `Value`；`DOMString`、數字、布林、null 和介面覆寫成前端的專用方法（`Front::string_arg`、`object_arg`、`ret_u32`、`ret_string`……），和 Blink 生成的程式對每種 IDL 型別用各自的 `NativeValueTraits<IDLType>`、`ToV8Traits<IDLType>` 一樣。不選維持經過 `Value`：每次呼叫多 80–420 個指令，讀的成員差最多（`nodeType` 多約五成，`details.md`）。代價是 `Front` 多幾個方法，wasm 的前端也要實作。
13. **第 2 段涵蓋所有的成員形狀**（改第 6 條）：生成的機制對每種宣告的形狀都成立，沒有照參數和回傳的型別篩；dictionary、union、callback、sequence 也走生成的路，轉換照舊經過 `Value`，行為和通用的路一樣。V8 的前端不接的只有：overload、回傳 Promise、`FrozenArray` 和 observable array 的、variadic、只有 wasm 看得到的、`glue.js` 建 JS 形式的、`[Global]` 介面的成員。不選照第 6 條篩：機制已經對所有形狀成立，篩掉反而多一層判斷；行為照樣由測試、mutation test 和 WPT 確認。
14. **不做執行時的交叉檢查**（改第 8 條）：生成的路和共用的路呼叫同一個 `dom_api` 的函式，「再用共用的路算一次」比的是同一個函式；兩條路真正不同的是前端的轉換，而參數的轉換會跑 script，不能跑兩次來比。行為由測試、mutation test 和 WPT 守著；最後所有的成員都走生成的路，共用的路不再存在。不選每個 isolate 的「只用共用的路」開關加上差異測試：只在兩條路並存的過渡期有用。量測也不用開關：比較兩種做法時量每次呼叫的指令數，指令數跨 build 可以直接比（`details.md`）。

2026-10-21，量了生成的路的編譯成本之後（使用者）：

15. **每個成員只具現化它的呼叫本身**：第 2 段一開始 `dom_js` 的 release build 從 6 秒變成 127 秒，dev 下加一個成員要重編 150 秒（量到的，`details.md`；多數只量一次，幾次的結果一致）。量到的還有：大半的時間在 rustc 自己產生 LLVM IR（`-Ztime-passes`，單線程），每個成員有十幾份泛型的具現化（`-Zprint-mono-items`），一半是只在建 templates 時跑一次的程式（找成員、建 function、走訪器的 `member`），`[CEReactions]` 的兩個分支又各有一份成員的 closure。「時間花在具現化的量」是從這些推的，沒有量每份具現化的時間（`-Zself-profile`）。改成建 templates 的程式不泛型（走訪器只交出成員的 callback，用 rusty_v8 的 `MapFnTo` 取出它的函式指標）、closure 只寫一次：release 17 秒、`perf` 19 秒、dev 加一個成員 28 秒（第 1 段 13.6 秒）；這兩件一起改、一起量，各占多少沒有分開量。不選另外兩個也試過的做法（對照實驗，`details.md`）：callback 改成成員型別的方法（`extern "C"` 的 trait 方法，rustc 把它分到成員型別的 codegen unit）再省兩成，但要在 `bindings` 把參照版的 callback 用 `transmute` 轉成 V8 的函式指標型別、自己開 `CallbackScope`，多兩段 `unsafe`；每個介面的成員型別放在自己的模組，沒有貢獻。不選共用的 trampoline 加上間接呼叫（第 11 條不選的）：編譯成本已經回到可以接受的程度，不必付執行時的成本。

2026-10-21，第 3 段開始前（使用者）：

16. **量每次呼叫的指令數，做成常駐的 `bench.py calls`**：JS 和 wasm 各一組固定次數的迴圈（JS 是 `dom_js` 的 `#[ignore]` 測試跑一段 script，wasm 是一個 C 的 guest），`perf stat` 量、減掉空迴圈。第 2 段的量法（第 14 條）每次都暫時加、量完拿掉，已經重做三次。不選 wasm 版的 blink_perf 頁：runs/s 太吵（`details.md`），頁面和 guest 的基礎設施也多。
17. **wasm 的前端用 typed 的 `func_wrap`**（乙，2026-10-21 量了之後）：wasm 每次呼叫比 JS 貴 10–20 倍（`bench.py calls`：`nodeType` 4152 個指令，JS 209），取樣看到 wasmtime 動態的 `Val`（`DynamicHostFn`、`Val::store` / `lift`）占兩到三成、我們自己的 `Val` ↔ `Value`、查表和 `check_this` 占一成半到兩成，其餘一半是 wasmtime 每次 host 呼叫的 task 記帳和 resource 表（兩種做法都省不掉）。乙用 typed 的 `func_wrap`：wasmtime 直接把參數轉成 Rust 型別，連 `Val` 都不經過；`declare!` 替每個成員多生成參數型別的 tuple 和一個收已轉好參數的入口，`dom_wasm` 替每種型別寫 wasmtime 的對應。編譯成本照簽名去重：WIT 的 7461 個函式裡，參數和回傳都是簡單型別（resource、字串、數字、布林、`option`、`result<_, dom-exception>`）、不是 async 的 6342 個，換成 Rust 的型別只有 120 種簽名，所有同簽名的函式共用一種只捕捉函式指標的 closure，wasmtime 的程式只具現化 120 份。只量 wasmtime 那一半的原型：release 的 `dom_wasm` 從 10.9 秒到 19.2 秒、`perf` 從 18.6 到 32.8（三輪輪流，`details.md`）；加上每個成員一份的呼叫（和甲一樣，量級同 JS 的 `M::call`），估計 release 約 29 秒（甲約 21 秒）。其他函式（dictionary、union、enum、`list`、callback）先照舊走共用的路，乙做完再量要不要替它們做甲。不選甲：保留 wasmtime 的 `Val`，每次呼叫多兩到三成；改的行數少（使用者：RFC 本來就是為了效益大改設計）。
18. **wasm 的讀的成員不 root `this`**：`dom` 的 `DomAccess` 加 `read_held`，在 read scope 裡把 Store 的 slot 的節點以 `NodeRef` 交出（slot 表由 holder trace，slot 放掉之前節點一定活著）。回傳的節點照舊佔一格 slot（guest 拿到 handle 就要一格），mutation 成員的 `this` 照舊 root（第 4 段）。
19. **wasm 接的成員**：`declare!` 生成的都接，除了 `async`、回傳 promise 的（要經過 Store 的 event loop）、`static` 的和只給 JS 的。比 JS 多接 overload 的第一個形式（WIT 裡每個形式是各自的函式，不用照參數選；`as` 改名的照舊）和 `[Global]` 的成員（wasm 的 `this` 就是 slot 裡的 window，沒有 global proxy）。
20. **乙的設計**（2026-10-21，使用者）：`dom_api` 的 `ApiType` 多一個關聯型別 `Wire`，和 engine 無關地標出每種型別在呼叫邊界上的形狀（字串、各種數字、布林、物件、可為 null 的、`()`，其他是 `Complex`）；`TypedMember` 多參數的 `Args`、回傳的 `Ret` 和收已轉好參數、直接回傳結果的 `call_with`（JS 照舊用 `call`）。`dom_wasm` 把每種形狀對應到 wasmtime 的型別（字串參數 `WasmStr`、回傳 `String`，物件 `ResourceDynamic`，`option`，`result<_, dom-exception>`），參數和回傳都不是 `Complex` 的成員用 typed 的 `func_wrap`（照簽名的 `wrap::<P, R>`、每個成員一份 `typed_call::<M>`；`Complex` 的成員由常數排除，rustc 不具現化那個分支），其他照舊 `func_new`。不選讓 `dom_wasm` 自己替每種 `ApiType` 寫對應：介面、dictionary、union 的型別是 `dom_api` 的 macro 生成的，`dom_wasm` 列不完，blanket impl 又和具體型別的衝突（coherence）。
21. **wasm 的 typed 函式怎麼列出每個成員**（2026-10-21，實作第 20 條時，使用者）：泛型的走訪器（`TypedVisitor::member::<M>`）證明不了任意成員的參數有 wasmtime 的型別（Rust 不做「`Wire` 只有這幾種」的封閉推理，`dom_api` 又不能提到 wasmtime 的 `Lift`），所以每個成員要在一個具體的呼叫點出現：`dom_wasm` 的 `src/typed_list.rs` 每行一個成員的型別，由測試從宣告表算出、和檔案比對（`MACHOPO_UPDATE_GOLDEN=1` 重寫，和 `machopo-dom.wit` 一樣）。成員的型別是它的介面自己的 `M<J>`（`typed_paths::Node::M<12>`，`J` 由 const fn 算出），不用名字：`macro_rules!` 接不出 identifier，成員的名字又可能是關鍵字（`type`）。先用過所有成員共用的 `TypedPath<I, J>`：`dom_api` 的 dev build 多了 14 秒，`-Ztime-passes` 量到在 coherence checking（13.2 秒，第 2 段 0.9 秒；rustc 在同一個型別的 impl 之間兩兩檢查重疊），改成每個介面一個型別之後 3.1 秒，`dom_api` 回到第 2 段的水準。過時的清單（介面裡加了成員，後面的 `J` 移動了）照樣編得過：`Complex` 也有 wasmtime 型別的空實作，`Registry::add` 用常數條件排除，那幾個成員暫時走共用的路，測試提醒重寫。`[CEReactions]` 的 reaction 在 wasm 不在呼叫裡跑（`Front::CE_REACTIONS`）：會呼叫回 guest，同步的 host 函式不能，和共用的路一樣留給 backup element queue。不選 `declare!` 再生成一個列出成員的 macro：巢狀的 `macro_rules!` 要傳 `$` 的技巧，13 個檔案的 `declare!` 也都要多一個名字（第 11 條擔心的難讀、難除錯）。不選 `paste` 接出名字：多一個上游已封存的 dependency。

2026-10-21，第 4 段開始前（使用者）：

22. **第 4 段照量到的效益分成四小段**，每一小段開始前再列它的決定。Speedometer 在第 3 段之後重量（`details.md`）：script 的差距 220 → 181 ms，減少的幾乎都在 bindings（131.5 → 96.8 ms）；呼叫 DOM 剩下 bindings 97、DOM 本身 113、V8 API 和 wrapper 62、配置 50、root 43 ms。
    - 4a：`Window`（`[Global]`）的成員走專用的路（224 個；`document` 每次呼叫 2727 個指令，同樣回傳 document 的 `ownerDocument` 走專用的路 837 個）；
    - 4b：會改 DOM 的成員的 `this` 和節點參數不 root（root 占這類呼叫的 5–12%），加上第 23 條；
    - 4c：共用路上可能常用的（`addEventListener`、`setTimeout`、`dispatchEvent`、`classList.add`、overload），先用 `bench.py calls` 量過再定；
    - 4d：其他（非節點的 receiver 和回傳值不 root、event handler 屬性、wasm 的複雜型別成員、wasmtime 每次 host 呼叫的成本）。
23. **取樣看到的 DOM 那一側的熱點放進 4b**：`setAttribute` 的 `QualName::clone`（那次呼叫的 15%）、`appendChild` 的 `own_filter`（17.5%），像第 10 條順手修 `setAttribute` 不複製屬性那樣。不選另開 RFC：熱點已經找到、在同樣的呼叫裡，量法也是同一套。

2026-10-21，4a 的原型量過之後（使用者）：

24. **`[Global]` 的成員也走專用的路**：拿掉 `takes()` 的 `!desc.global`。`this` 是 global proxy（`undefined` 由 V8 換成它），`convert::object_of` 照樣拿到 window；直接讀、`.call(window)`、`.call(undefined)`、不帶 receiver 和另一個同源 window 都有測試。效益小（`document` 2727 → 2561 個指令）：global object 一定是 dictionary 模式，V8 的 IC 不 cache 它上面的 API getter，每次都走 `LoadIC` 的慢路，binding 只是一小部分。留下是因為沒有代價、少一個例外。
25. **不做 cached accessor**（Blink 的 `[CachedAccessor]`，V8 的 `FunctionTemplate::NewWithCache`）：原型在 fork 接出 `NewWithCache`，套在 `Window.document`，建 realm 時把 document 的 wrapper 存在 global 的 private 屬性上；直接讀 `document` 2519 → 約 0 個指令，經過存起來的 proxy 2375 → 107。但第 3 段的 Speedometer 取樣裡，所有 global 的 getter 的 V8 慢路和 binding 加起來每次迭代約 0.5 ms（script 的差距 181 ms），fork 要多一個 C++ 的改動。原型和量法記在 `details.md`。
26. **成員的 function 讓 V8 檢查 `this` 能不能存取**（`set_accept_any_receiver(false)`，fork 接出 `FunctionTemplate::SetAcceptAnyReceiver`）：補 4a 的測試時發現，跨源的 window 讀屬性會被 security token 擋下，但拿自己 realm 的成員用在它身上（`Object.getOwnPropertyDescriptor(window, 'document').get.call(other)`、`getSelection.call(other)`、`addEventListener.call(other, …)`）不會：V8 的 function template 預設 accept any receiver，呼叫時不做 access check。共用的路也一樣，不是 4a 造成的。照 Blink：`IDLMemberInstaller` 對不是 `[CrossOrigin]` 的成員都設 false（`bind_gen/interface.py` 的 `_make_property_entry_cross_origin_check`），static 的不設。kun 的屬性的 getter、setter、方法、iterator、`[PutForwards]` 和 `[Replaceable]` 的 setter 都設；static 的方法、namespace 的成員和建構子不設。不選甲（在 binding 裡比 `Window`、`EventTarget` 的成員的 `this` 的 security token）：要列出哪些介面，新的介面會漏；V8 在呼叫前就擋，兩條路都不用改。效能：一般的 DOM 物件的 map 不需要 access check，TurboFan 知道 map 時省掉檢查。

2026-10-21，4a 做完之後（使用者）：

27. **這份 RFC 在 4a 結束**：4b–4d 移到「之後的可能」，第 22、23 條的安排跟著改。生成的路、wasm 的轉接和共用 callback 的成本已經做完；剩下的是各自獨立的優化，每一項都要先量、各自的論證（不 root 的 mutation 成員要新的原語），適合各開一份。

## 待決定

（無）

## 之後的可能

- **會改 DOM 的成員的 `this` 和節點參數不 root**（原本的 4b）：呼叫期間它們的 wrapper 在 V8 的 handle scope 裡，V8 trace 它，cppgc 不搬動物件；要新的原語表示「靠 wrapper 保持存活」，加上 mutation scope 的新形狀。root 占這類呼叫的 5–12%，Speedometer 裡 root 共 43 ms（`docs/performance-gaps.md`）。
- **DOM 那一側的熱點**（原本第 23 條放進 4b 的）：`setAttribute` 的 `QualName::clone`（那次呼叫的 15%）、`appendChild` 的 `own_filter`（17.5%）。
- **共用路上常用的成員**（原本的 4c）：`addEventListener`、`setTimeout`、`dispatchEvent`、`classList.add`、overload；先用 `bench.py calls` 量。Blink 對 overload 照參數個數分派。
- **其他**（原本的 4d）：不是節點的 receiver 和回傳值不 root（`read_node` 推廣成 `read_object::<T>`、`NodeOut` 推廣到任何 `ScriptObject`）、event handler 屬性、wasm 的複雜型別成員（每個成員一個 `func_new` 的 handler，第 17 條的甲）、wasmtime 每次 host 呼叫的固定成本（開著 component model 的 async 時，同步的 host 函式每次也建一個 host task，加上 resource 表，`nodeType` 約 2000 個指令，是 wasm 比 JS 慢的大頭；在本地調查 wasmtime 有沒有快路徑，要回報的寫成草稿給人看）。
- **`Window.document`、`window` 的 cached accessor**（第 25 條，原型在 `details.md`）：Speedometer 裡看不到效益；有頁面在熱迴圈裡讀 `document` 時再做。
- **和 Chrome 逐個 JS 函式比**：兩邊打開 V8 的 CPU profiler 試過（`details.md`、`docs/performance-gaps.md`），trace 的 profiler 每 100 µs 取樣（寫死在 V8 裡）讓 Chrome 慢五成以上，只能看各自的排序。兩種做法：每邊的時間比例乘上不開 profiler 時量到的時間（先確認 kun 被拖慢的比例和 Chrome 一樣）；或 Chrome 改用 CDP 的 `Profiler` 以 1 ms 取樣，kun 用 fork 接出 V8 的 `CpuProfiler` 以同樣的間隔取樣。要開一份量測工具的 RFC。要看 Blink 內部時，編一個照正式版設定、帶符號的 Chromium（`is_official_build`、`symbol_level=1`）。
- **這份做的時候看到、不在範圍內的**（`docs/performance-gaps.md` 的「可以開 RFC 的方向」）：canvas 的 `fill`、`stroke` 讓 main thread 等（Charts-chartjs 的 script 差距的大半）；`innerWidth`、`innerHeight` 每次更新 frame 自己的 layout（NewsSite-Next 每個 iteration 約 38 ms）。
- 字串的快取（Blink 的 `StringCache`）。
- V8 的 Fast API call（canvas 2D）。
- wasm 的 guest 拿到的 handle 不必每次佔一個新的 slot。
