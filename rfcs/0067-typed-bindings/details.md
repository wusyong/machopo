# RFC 0067 的細節：原型、生成的路、wasm 和量測

這份是 `README.md` 的補充，放支撐它的量測。結論和決定以 `README.md` 為準。

## 量法

2026-10-20、21，Fedora 44（AMD Ryzen AI 9 HX 370），兩個瀏覽器綁在最快的 CPU（0–3、12–15）上；kun 是 `perf` profile，Chrome for Testing 152.0.7977.82。`bench.py blink-perf --pages` 跑 blink_perf 的 `bindings/` 頁，每頁 20 次的中位數。

**比較兩種做法只在同一個 binary 裡做**：量的時候原型有一個環境變數 `MACHOPO_GENERIC_BINDINGS`，讓同一個 build 的所有成員改走通用的路，兩種輪流各跑兩輪（量完拿掉：它是整個 process 共用的全域狀態，`sanity.py boundary` 不讓 renderer 那一側有；第 2 段要再這樣比時，做成每個 isolate 的設定）。換一個 build，程式碼的位置和對齊不同，單頁就可能差好幾成（`docs/benchmarks.md`）：一次「多查一次 isolate 的 slot」的實驗，比的是兩個 build，看起來慢了一成（636 → 572 runs/s）；改成同一個 binary 用環境變數切換之後，兩種完全一樣（609 對 609、689 對 689）。

## 六個成員：通用的路和專用的路

runs/s，同一個 binary，兩輪的中位數（兩輪之間差不到 2%）：

| 頁 | 通用的路 | 專用的路 | 變快 | Chrome | 和 Chrome 的倍數：之前 → 之後 |
| --- | ---: | ---: | ---: | ---: | --- |
| `bindings/first-child`（`firstChild`） | 154 | 636 | 4.1 倍 | 約 2100 | 13.4 → 3.3 |
| `bindings/node-type`（`nodeType`） | 192 | 706 | 3.7 倍 | 約 2450 | 13.2 → 3.4 |
| `bindings/get-attribute`（`getAttribute`） | 82 | 165 | 2.0 倍 | 約 465 | 5.7 → 2.8 |
| `bindings/set-attribute`（`setAttribute`） | 304 | 441 | 1.45 倍 | 約 2240 | 7.4 → 5.1 |
| `bindings/append-child`（`appendChild`） | 98 | 125 | 1.28 倍 | 約 125 | 1.28 → 1.0 |
| `bindings/create-element`（`createElement`） | 244 | 266 | 1.09 倍 | 約 2920 | 11.8 → 11.0 |

讀的成員（`this` 和回傳值不 root、不經過 `Value`、`getAttribute` 也不再複製整份屬性）快最多；mutation 的成員只拿掉了分派和 `Value`，`this` 照樣 root。

## 專用的路上剩下什麼（取樣）

對專用的路的頁取樣（整頁的樣本，`perf` 每一百萬個 cycle 一個）：

- **`first-child`**：callback 本身（unwrap、read scope、讀 wrapper）32%；V8 的 C++ 小函式：`Value::IsObject` 18.7%、`Object::IsApiWrapper` 10%、設定回傳值 9.3% 加上 `GetReturnValue` 7.3%、進 callback（`CallApiCallbackOptimizedNoProfiling`）8.2%。`IsObject` 偏高，可能是第一次讀 receiver 那塊記憶體的成本算在它身上。
- **`set-attribute`**：`QualName::clone` 14.7%（`set_attribute` 找已有的屬性時用 `attribute_by_name`，複製整份屬性）；字串的轉換（`ValueView`、`to_rust_string_lossy`）約 14%；`V8Host::mutate` 4.8%、`after_call` 3.3%。
- **`create-element`**：釋放記憶體（glibc 的 `_int_free_chunk`，V8 的 platform task）13%；第一次替新元素建 wrapper 時照標籤名稱找它的種類：`memcmp` 10.1%、`node_kind` 5.9%（`classify_node` 的 `element_kind`）。

## isolate 的狀態放哪（第 4 條）

`BindingContext::new` 每次從 isolate 的 slot 拿 DOM 的狀態（一次 `GetData` 的 FFI 加上以 `TypeId` 為 key 的查找；rusty_v8 已經用專門給 `TypeId` 的雜湊）。原型試了一個 `v8_engine.rs` 私有的 thread-local（`ScriptExecution` 進入時記下 isolate 和狀態，`new` 先讀它、比對 isolate），同一個 binary 用環境變數切換比：`first-child` 609.6、609.6 對 609.0、611.4，`node-type` 689.1、690.0 對 689.2、691.9。沒有差別，不改。

## 第 2 段：每次呼叫的指令數

### 量法

runs/s 不夠穩：同一個 binary、同一種做法，`first-child` 兩輪就差六成（6.95 對 4.2 倍於 Chrome），CPU 的頻率、程式碼的對齊和別的程式都算在裡面。第 2 段改量**每次呼叫的指令數**（和 cycles 參考）：`dom_js` 的測試 binary 用 `perf` profile 編（`cargo test --profile perf -p dom_js --test it --no-run`），一個暫時的 `#[ignore]` 測試在 `Page` 裡跑一段 script：先建好節點，再把一個成員呼叫一百萬次；`taskset -c 2 perf stat -e instructions:u,cycles:u` 量整個 process，減掉同樣的空迴圈，除以一百萬。同一個設定量三次，指令數差 0.2%、cycles 差 1–5%。指令數跨 build 也能直接比（程式碼的位置不影響指令數），所以原型那個 commit 另外編一份（`../kun-against`，`CARGO_TARGET_DIR=target/against`）一起比。量完拿掉那個測試。

### 結果

每次呼叫的指令數（括號裡是 cycles），2026-10-21，Fedora 44（AMD Ryzen AI 9 HX 370）：

| 成員 | 通用的路 | 原型（手寫） | 生成的路，經過 `Value` | 生成的路，照型別轉換 | 加上 `setAttribute` 不複製屬性 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `firstChild` | 1113 (259) | 260 (65) | 260 (63) | 260 (62) | 260 (64) |
| `nodeType` | 859 (202) | 156 (46) | 232 (63) | 182 (48) | 181 (48) |
| `getAttribute` | 1796 (410) | 912 (194) | 1091 (245) | 957 (201) | 957 (204) |
| `setAttribute` | 5245 (1434) | 4193 (998) | 4613 (1257) | 4277 (1141) | 3539 (799) |
| `appendChild` | 4174 (1069) | 3204 (762) | 3575 (862) | 3504 (818) | 3505 (724) |
| `createElement` | 9461 (4269) | 8669 (4003) | 8880 (4033) | 8778 (3958) | 8752 (3973) |

- 「通用的路」是同一個 binary 用暫時的環境變數讓所有成員走共用的 callback 量的（量完拿掉，第 14 條）；`setAttribute` 不複製屬性之後，通用的路也跟著少（5245 → 4512）。
- 生成的路經過 `Value` 時比原型多 0–420 個指令：參數經過 `convert_arg`、`from_js`（`IsNull`、`IsUndefined`、`GetCurrentContext` 的 FFI），結果經過 `into_value` 和 `to_js`。照型別轉換之後（第 12 條）剩 0–110 個，`appendChild` 還多 300 個：節點參數經過 `object_of`、`Object` 和 `from_value` 的種類檢查，原型直接 `unwrap_node`、不檢查（第 4 段處理節點參數時再看）。
- `setAttribute` 找已有的屬性時改成借用、只複製找到的那一個（`attribute_by_name`，第 10 條），少 740 個指令，比原型還少。

## 第 2 段：編譯的成本

`dom_js` 重編一次的秒數（touch 它的 `lib.rs`），和 `kun` 的大小，2026-10-21：

| | 第 1 段（原型） | 第 2 段，一開始 |
| --- | ---: | ---: |
| release 的 `dom_js` | 6 | 127 |
| `perf` 的 `dom_js`（一個 codegen unit） | 13 | 156 |
| dev，`dom_api` 加一個成員之後的 `dom_js`（含 `dom_api`） | 13.6 | 150 |
| `perf` 的 `kun`，`dom_api` 改了之後 | 165 | 305 |
| `kun` 的大小 | 363.6 MB | 377.0 MB |

- 上表的數字是第 1 段和第 2 段一開始的版本各量一次（`perf` 的 `kun` 各兩次）；release 的 `dom_js` 之後在加了下面第 1 件、第 1 和第 2 件的版本上各量到 131.7、126.1 秒，一致。
- 量到的（在加了第 1、2 件、126 秒的版本上）：`-Ztime-passes` 的 `codegen_to_LLVM_IR`（rustc 自己產生 LLVM IR，單線程）91 秒、monomorphization 的收集 21 秒、crate metadata 21 秒、LLVM 的優化 36 秒、ThinLTO 11 秒（有重疊）；`-Zprint-mono-items` 22950 個，每個成員約 15 個，其中一半是建 templates 時跑一次的（找成員的 `position` 和它的 closure、`member_of`、`build`、走訪器的 `member`，各 1503 個），`[CEReactions]` 的兩個分支各有一份成員的 closure（470、285 個）。在最原始的版本上（`perf` profile）量了 LLVM IR 的行數：每個成員的 callback（rusty_v8 的 `c_fn` 和 inline 進去的呼叫）1430 份共 48.9 萬行，占六成；13 個 `visit_typed` 共 9.5 萬行；`build::<M>` 1430 份共 4.4 萬行。
- 推論的：時間花在具現化的量。沒有量每份具現化各花多少（`-Zself-profile`），所以不知道是很多小的加起來，還是 inline 了幾百個成員的 `visit_typed` 這種大函式特別慢（LLVM 對很大的函式是超線性的）。
- 試了三件事，依序做時只有第 3 件讓時間降下來（release 127 → 131 → 126 → 15；第 3 件裡的兩件事一起改、一起量）：1. callback 改成成員型別的 `extern "C"` trait 方法（rusty_v8 的 `MapFnTo` 包裝是同一個泛型函式，它的具現化全部在一個 codegen unit；成員型別的方法分到成員型別的模組的 unit）；2. 每個介面的成員型別放在自己的模組，unit 照介面切；3. 建 templates 的程式不泛型、`[CEReactions]` 的 closure 只寫一次。

對照實驗：每種組合套在第 2 段的 commit 上，三輪輪流量（秒，中位數；每種組合都跑了 `typed_paths_skip_the_shared_callback`）：

| 組合 | release 的 `dom_js` | `perf` 的 `dom_js` | dev，加一個成員 |
| --- | ---: | ---: | ---: |
| 1+2+3 | 14.0 | 14.2 | 24.8 |
| 1+3 | 13.6 | 15.8 | 25.0 |
| 2+3 | 15.8 | 18.8 | 29.5 |
| 只有 3（採用） | 17.4 | 19.5 | 27.9 |

- 第 3 件是幾乎全部的改善；第 1 件再省兩成（三個數字都一致），但要在 `bindings` 把 `extern "C" fn(&FunctionCallbackInfo)` 用 `transmute` 轉成 V8 的函式指標型別、自己開 `CallbackScope`，不採用（第 15 條）；第 2 件沒有貢獻，沒有第 1 件時 dev 加成員反而慢。
- 同一個組合單量一次時差很多（1+2+3 先量到 `perf` 43.6 秒、dev 加成員 23.5 秒，對照實驗的第一輪是 12.1 和 38.0）：比較編譯時間要輪流量幾次。
- dev 下改了 `dom_api` 又改回來之後，連結 `dom_js` 的測試時 `dom_api` 的 rlib 偶爾少了符號（`undefined hidden symbol`），清掉兩個 crate 的 `target/debug/incremental/` 就好：rustc 的 incremental 快取的問題，對照實驗每輪之後清一次。
- 採用的做法下每次呼叫的指令數和三件都做時差 ±40（`firstChild` 290、`nodeType` 198、`getAttribute` 916、`setAttribute` 3500、`appendChild` 3470、`createElement` 8774，三件都做時量的）。

## 第 3 段：wasm 呼叫 DOM 的成本

### 基準

`bench.py calls`（`perf` profile，一百萬次、減掉空迴圈、三次的中位數，CPU 0），2026-10-21，每次呼叫的指令數（cycles）：

| 成員 | JS | wasm |
| --- | ---: | ---: |
| `firstChild` | 298 (75) | 6747 (2124) |
| `nodeType` | 209 (53) | 4152 (1318) |
| `getAttribute` | 939 (202) | 6566 (2126) |
| `setAttribute` | 3602 (832) | 8154 (2498) |
| `appendChild` | 3569 (777) | 10743 (3434) |
| `createElement` | 8890 (4122) | 10569 (4265) |

wasm 的 guest 拿到的 handle 每次都 drop（`firstChild`、`appendChild`、`createElement`），那是另一次 host 呼叫，算在裡面。

### 取樣（現在的共用路徑）

`perf record -e instructions:u`，照符號名稱粗分（%，占整個 process；長的泛型名稱可能歸錯類）：

| 類別 | `nodeType` | `firstChild` |
| --- | ---: | ---: |
| wasmtime：每次呼叫建一個 host task 的記帳（`host_task_create`、刪 `HostTask`、`current_scope_id`、`call_context`） | 約 22 | 約 22 |
| wasmtime：動態的 `Val`（`DynamicHostFn::run`、`load_params`、`Val::store`、`Val::lift`） | 約 30 | 約 20 |
| wasmtime：resource 和 handle 的表 | 16.6 | 23.5 |
| `dom_wasm`：`Val` ↔ `Value`（`lift_all`、`lower`、`own`、`object`） | 7.9 | 10.6 |
| `dom_api`：分派和檢查（`check_this`、`Object::kind`、`resource_of`） | 5.8 | 5.4 |
| `dom`：slot、holder、root | 3.4 | 10.8 |
| 成員本身、guest 的程式 | 約 1 | 約 1 |

### typed 的 `func_wrap` 的編譯成本（原型）

從 `machopo-dom.wit` 數：7461 個函式（子 resource 各列一次祖先的方法），參數和回傳都是簡單型別、不是 async 的 6342 個，換成 Rust 的型別（resource 都是 `ResourceDynamic`，字串參數 `WasmStr`、回傳 `String`，`option`、`result<_, dom-exception>`，方法加上 `self`）有 120 種簽名；最多的是 `(self) -> result<string>` 809 個、`(self) -> result<option<resource>>` 690 個、`(self, string) -> result<_>` 591 個。原型替每種簽名經過同一個 `wrap::<P, R>` 具現化一份 `func_wrap`（執行時不註冊），`dom_wasm` 多了 7014 個 mono item（全部 27750 個）。三輪輪流量（秒，中位數）：

| `dom_wasm` 重編 | 沒有原型 | 加上 120 種 typed `func_wrap` |
| --- | ---: | ---: |
| release | 10.9（10.7、10.9、11.3） | 19.2（19.2、18.9、19.2） |
| `perf` | 18.6（20.3、18.5、18.6） | 32.8（34.0、32.7、32.8） |

### typed 的 `func_wrap` 做出來之後

每次呼叫的指令數（`bench.py calls`，wasm）：`firstChild` 6747 → 4287、`nodeType` 4152 → 2184、`getAttribute` 6566 → 3962、`setAttribute` 8154 → 5946、`appendChild` 10743 → 7900、`createElement` 10569 → 8150。

編譯的成本（秒，單次；從頭到尾只編 `-p dom_wasm`，dev 加一個成員那次用 `--timings` 拆出每個 crate 的時間，有重疊）：

| 版本 | release `dom_wasm` | dev 加成員：總共 | `dom_api` | `dom_wasm` |
| --- | ---: | ---: | ---: | ---: |
| 第 2 段（wasm 走共用的路） | 10.5 | 19.8 | 19.6 | 10.6 |
| 成員的型別是 `TypedPath<I, J>` | 30.6 | 57.5 | 33.8 | 35.2 |
| 同上，不照簽名去重（每個成員一份 `func_wrap`） | 100.1 | 131.5 | 34.2 | 109.1 |
| 成員的型別是每個介面的 `M<J>`（採用） | 26.5 | 41.0 | 20.7 | 31.9 |

- `dom_api` 多的 14 秒：dev 下整個重編 `dom_api` 的 `-Ztime-passes`，`coherence_checking` 第 2 段 0.9 秒、`TypedPath<I, J>` 13.2 秒、每個介面的 `M<J>` 3.1 秒；總共 31.8、50.7、35.9 秒。
- release 的 `dom_js`：第 2 段 17.1、16.9 秒，現在 14.4、15.1 秒（mono item 兩邊都是 26031 個）。
- 量法的教訓：先用 `cargo build -p dom_wasm -p dom_js` 一起熱身、再單獨量 `-p dom_js`，cargo 合成的 feature 組合不同，共用的相依又重編一次，量到 64–105 秒；比較時從頭到尾用同一組 package。改了 `dom_api` 又改回來之後清掉 `target/debug/incremental/` 的那兩個 crate（rustc 的 incremental 快取偶爾少了符號）。
- 也試了兩件沒有明確效果的（量法有上面的問題，只量一輪半，退回）：每個介面的 index 是一個有名字的常數、清單照介面分模組並把 `typed_call` 這條鏈標 `#[inline]`。

## 第 3 段之後的 Speedometer

2026-10-21，kun `4c008a87` 和 Chrome 的 trace（`bench.py speedometer trace`、`sample`）。script 的差距 220.5 → 181.2 ms（每次迭代，20 個 suite 加起來），減少的幾乎都在 bindings（131.5 → 96.8 ms）；呼叫 DOM 的時間 571.9 → 528.5 ms，剩下 bindings 97、DOM 本身 113、V8 API 和 wrapper 62、配置 50、root 43 ms。總分 1.52 和第 2 段的 1.47 差在雜訊裡（`--against dc5000da`）。

## 4a：`Window` 的成員

### 原型：`[Global]` 的成員走專用的路

每次存取的指令數（`perf stat`，`dom_js` 的 `bindings::bench_calls`，十萬次的迴圈減掉空的迴圈，三次取中位數）：

| 存取 | 指令 |
| --- | ---: |
| `document`，共用的路 | 2727 |
| `document`，專用的路 | 2561 |
| `n.ownerDocument`（同樣回傳 document，專用的路） | 837 |
| global 的資料屬性 | 約 0 |
| global 的 JS getter | 約 97 |

差的不在 binding：V8 的 `LoadIC` 只 cache holder 是 fast mode 的 API getter（`ic.cc` 的 `ComputeHandler`），global object 一定是 dictionary 模式，每次都走 `LoadIC` 的慢路（`Runtime_LoadNoFeedbackIC_Miss` → `GetPropertyWithAccessor`）。

### 原型：cached accessor（不採用，第 25 條）

fork 接出 `FunctionTemplate::NewWithCache`（`build_with_cache`），`Window.document` 的 getter 帶一個 private 名字（`Private::for_api`），建 realm 時把 document 的 wrapper 用它存在 global 上（`set_private` 經過 global proxy 存到 global object，`LookupIterator::GetStoreTarget`）；kun 的 window 只在建立時有 document，存一次就好（Blink 的 `LocalWindowProxy::UpdateDocumentProperty` 在換 document 時更新）。V8 的 IC 在 getter 有 cache 時改讀那個屬性（`LookupIterator::LookupCachedProperty`）：`LoadGlobalIC` 直接讀 property cell，經過 proxy 的 `LoadIC` 是 `LoadGlobal` 的 handler。同一個 build 只切換 cache（指令，三次中位數）：

| 存取 | 沒有 cache | 有 cache |
| --- | ---: | ---: |
| `document` | 2519 | 約 0 |
| `w.document`（存起來的 proxy） | 2375 | 107 |
| `window` | 2255 | 2255 |
| `n.ownerDocument` | 807 | 807 |

`window.document` 還是約 2300：慢的是 `window` 的 getter 本身。Speedometer：第 3 段的取樣（每 1M cycles 一次，10 次迭代）裡，讀 global 的 getter 走慢路的共 2000 個取樣（約 40 ms 一次迭代），getter 本身的工作占 1973 個，V8 的慢路和 binding 只有 27 個（約 0.5 ms），cache 省得到的就是這一點。原型沒有 commit。

### `innerHeight` 的 layout（不在這份 RFC 的範圍）

那 2000 個取樣裡 1908 個在 NewsSite-Next 的同一個 `resize` handler：`let e = .01 * window.innerHeight; document.documentElement.style.setProperty("--vh", e + "px")`。kun 的 `innerHeight`（`dom_api::geometry` 的 `viewport`）先把 frame 自己的 document 的 style 和 layout 更新一次（取樣落在 flexbox、parley 的斷行和 `style::cascade`），約每次迭代 38 ms、NewsSite-Next 在 kun 的三成。Chrome 的 `LocalDOMWindow::GetViewportSize` 只在 main frame（viewport 開著，或還沒有第一次 layout）時更新自己的 layout，子 frame 只更新 parent 的（Speedometer 的 suite 在 iframe 裡）。

### 跨源的 window 和 `this` 的檢查（第 26 條）

V8 的 function template 預設 accept any receiver（`v8-template.h`：「Currently defaults to true」），`Builtins::HandleApiCallHelper` 和 `CallFunctionTemplate` 只在旗標是 false、receiver 的 map 要 access check 時（global proxy 一定要）呼叫 `MayAccess`，比對 security token。設成 false 之前，跨源的 `other` 讀屬性被擋下，但 `Object.getOwnPropertyDescriptor(window, 'document').get.call(other)` 拿到對方的 document，`getSelection.call(other)`、`addEventListener.call(other, …)`、`fetch.call(other, …)` 都在對方的 window 上執行；共用的路也一樣。Blink：`IDLMemberInstaller` 照 `bind_gen/interface.py` 的 `_make_property_entry_cross_origin_check`，不是 `[CrossOrigin]`、不是 static 的成員都設 false。fork 加 `set_accept_any_receiver`（只改 `binding.cc` 和 Rust，release 的 V8 增量重編 9 秒）。`setTimeout.call(other, …)` 沒有被擋：`glue.js` 的 `setTimeout` 不看 `this`，用自己的 global 呼叫原生的函式，計時器設在自己的 window 上（和規範不同，不是跨源的問題）。
