# RFC 0061 的細節：取樣的數字、一次呼叫走過的路、和 Blink 的對照

## 時間花在哪

2026-10-19，Fedora 44（AMD Ryzen AI 9 HX 370），`main` 的 `0bd98042`。`ci.py speedometer --sample`：20 個預設的 suite 各一次，10 個 iteration，`perf record --call-graph fp` 的 `cycles:u`，只取 renderer main thread 在 runner 計時的區間裡的樣本。每個樣本歸到它最內層、屬於某一類的 frame（略過 libc、mimalloc、`core`、`alloc`、`hashbrown` 這類共用的 frame）：

- **JS**：JIT 的程式、`Builtins_*`（API callback 的入口除外）、V8 的 runtime；
- **bindings**：`dom_js`、V8 的 API、API callback 的入口、`dom_api` 的宣告表的 thunk（`DESC::call`、`declare`、`value`、`object`、`custom_elements::call_member`）、cppgc 的 FFI；
- **DOM**：`dom`、`dom_api` 的其他部分、`html`；
- **style、layout、paint**：`style`、`selectors`、`layout`、`parley`、`paint`、compositor；
- **GC**：V8 和 cppgc 的 marking、sweeping；
- **canvas**：main thread 上的 Skia raster（Charts-chartjs、Perf-Dashboard 裡沒分類的那些）。

各類佔全部樣本的比例，再乘上 2026-10-19 量的各 suite 的 ms（`4990ae44`，和這次的程式只差 RFC 0060 的 `NodePtr`）換算：

| 類別 | 比例 | 約 ms（20 個 suite 合計 2293） |
| --- | ---: | ---: |
| style、layout、paint | 35.3% | 790 |
| JS | 20.6% | 490 |
| DOM | 22.6% | 480 |
| bindings | 13.4% | 300 |
| canvas | 6.8% | 200 |
| GC | 1.4% | 30 |

bindings 最多的 suite（ms）：

| suite | 合計 | JS | bindings | DOM |
| --- | ---: | ---: | ---: | ---: |
| TodoMVC-jQuery | 303 | 38 | 71 | 170 |
| Charts-chartjs | 315 | 64 | 55 | 29 |
| Perf-Dashboard | 195 | 27 | 26 | 21 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 165 | 18 | 21 | 87 |
| TodoMVC-JavaScript-ES5 | 93 | 16 | 21 | 39 |
| Charts-observable-plot | 81 | 35 | 13 | 9 |
| React-Stockcharts-SVG | 159 | 74 | 12 | 16 |
| NewsSite-Next | 132 | 23 | 11 | 17 |

Chrome 的 trace 只分得出「在執行 script」（JS、bindings 和 DOM 合在一起），Chrome for Testing 沒有 Blink 的符號，取樣也分不開，所以沒有 Chrome 那一邊的 bindings 數字；Chrome 跑完 18 個 suite 合計約 870–890 ms，加上 Perf-Dashboard 約 950 ms。

bindings 裡自己（self）的樣本最多的函式，佔全部樣本的比例：

| 函式 | % |
| --- | ---: |
| `dom_js::templates::call_member` | 1.73 |
| `cppgc__WeakPersistent__CONSTRUCT`（`NodeIdMap`，其實是 DOM） | 0.80 |
| API callback 的 trampoline（`<unsafe extern "C" fn …>`） | 0.78 |
| `dom_js::templates::convert_args` | 0.78 |
| `V8Host::mutate` | 0.73 |
| `dom_js::properties::call` | 0.59 |
| `Node` 的 thunk（`DESC::call`） | 0.47 |
| `V8Host::call` | 0.44 |
| `cppgc__make_garbage_collectable_on` | 0.38 |
| `cppgc__Persistent__CONSTRUCT` | 0.34 |
| `convert::wrapper` 的 closure | 0.32 |
| `Object::kind` | 0.31 |
| `custom_elements::call_member` | 0.28 |
| `cppgc__Persistent__Assign` | 0.26 |
| `dom_js::properties::indexed_item` | 0.26 |
| `convert::from_js` | 0.25 |
| `templates::new_wrapper` | 0.23 |

`__memcmp` 佔 1.70%，呼叫它最多的是 `convert::wrapper` 的 closure（建 wrapper 時用名稱找介面）、selector 的比對、`Node` 的 thunk、`properties::call`（用名稱找 member）。

## 一次呼叫走過的路

RFC 開始時（`0bd98042`）的樣子；第 1、2 段改了哪些見 README 的「詳細設計」。

`element.appendChild(child)`（`dom_js::templates::call_member`）：

1. `BindingContext::new`：從 isolate 的 slot 拿 DOM 的狀態。
2. receiver 能是哪些型別：`receiver_tags` 照介面的 index 從 isolate 的 slot 拿一個 `Rc<[ObjectTag]>` 並 clone；static 方法每次 `Rc::from([ObjectTag::Window])` 配置一個。
3. `convert::object_of`：確認是 API wrapper，再照 tag 逐一 `Object::fetch`，每試一個是一次 V8 的 `Unwrap`；拿到的物件 root 成 `Handle`。節點都是 `Node` 這一個 tag，排第一個，`Node`、`Element` 的成員一次就成功。
4. `dom_api::overloads`：方法每次呼叫都把整張 overload 群組表（`OnceLock` 裡的 `Vec`）掃一遍，比介面的名稱和成員的名稱，沒有 overload 的也一樣；有的話再解析。
5. `convert_args`：外層一個 `Vec<Value>`，每個參數各一個 `Vec`（為了 variadic），逐一 `from_js` 轉成 `Value`（節點參數又 unwrap、root 一次）。
6. `custom_elements::call_member`：`[CEReactions]` 的成員前後推、拿 element queue。
7. 宣告表的 thunk（`(member.call)(&mut dyn ScriptHost, &Object, Vec<Value>)`）：`check_this` 把 `Object` 對到實作要的型別，`@arg` 把每個 `Value` 轉成實作的參數型別，呼叫實作，結果包成 `Value`。
8. `convert::to_js`：`Value` 轉回 V8；物件先找已經有的 wrapper，沒有就建：找 realm、用介面名稱在 `INTERFACES` 裡逐一比對找 template 的 index（`templates::index_of`）、`new_instance`、wrap。

indexed 和 named 的存取（`properties::indexed_item`、`named_item`）：`inherited_member` 先用介面名稱在 `INTERFACES` 裡逐一比對找到介面，再沿著 parent 逐一找 special 的成員，然後走 `call`（1、3、6、7、8）。回傳不會是 null 的 indexed getter（只有 `CSSStyleDeclaration`）先走一趟取 `length`：WebIDL 的支援的 index 要小於 `length`。

## 第 1 段之後

2026-10-19，第 1 段的版本（`b45a01d4`），TodoMVC-jQuery、Charts-chartjs、ES5 各取樣三次（和 `main` 輪流的那一輪的 current）。bindings 依「在付什麼」分組，佔全部 cycle 的比例；`NodeIdMap` 的 `WeakPersistent` 和配置 DOM 物件（`cppgc__make_garbage_collectable_on`）是 DOM 的成本，不算在內：

| 在付什麼 | jQuery | chartjs | ES5 |
| --- | ---: | ---: | ---: |
| V8 API callback 的入口 | 0.8 | 0.7 | 0.6 |
| 共用的分派（`call_member`、`properties::call`、`custom_elements::call_member`、overload） | 2.2 | 4.1 | 3.0 |
| 開 DOM 的 scope（`V8Host::mutate`、`read`） | 2.0 | 1.7 | 1.9 |
| receiver 的種類和檢查（`Object::kind`、`element_kind`、`check_this`、`Unwrap`） | 4.4 | 0.8 | 2.7 |
| 參數轉換（`convert_args`、`from_js`、`from_value`） | 0.5 | 3.6 | 1.5 |
| 宣告表的 thunk（`DESC::call`） | 0.9 | 1.2 | 1.3 |
| 回傳值和 wrapper | 0.8 | 0.2 | 0.9 |
| root（receiver、參數、結果的 `Handle`） | 2.2 | 1.0 | 2.5 |

- jQuery 的 `element_kind` 約 1.6%（建 wrapper 0.92%、`check_this` 0.67%），`check_this::<Node>` 自己 1.19%。
- `V8Host::mutate` 自己的指令都是在寫 `DomMutationScope` 的欄位（`perf annotate`）：成本來自開的次數，每次約 20 個參考。
- `call_member` 自己的時間（chartjs 3.9%）分散：從 isolate 的 slot 拿 DOM 狀態（`TypeId` 的雜湊表）、`Exposure` 的比對、overload 的群組表（canvas 2D 的方法很多有 overload）。

## 第 2 段之後

2026-10-20，`main` 的 `ef83f611`（第 2 段），Fedora 44（AMD Ryzen AI 9 HX 370）。`ci.py speedometer --sample`：20 個預設的 suite 各一次，分類和「時間花在哪」一樣（canvas 的 raster 算在「其他」）。ms 是乘上 RFC 開始前（`cedd95d5`）量的各 suite 的 ms 換算的，偏高一些，只當近似：

| 類別 | RFC 開始前 | 第 2 段之後 |
| --- | ---: | ---: |
| style、layout、paint | 35.3% | 35.9% |
| DOM | 22.6% | 22.3% |
| JS | 20.6% | 21.5% |
| bindings | 13.4% | 11.5% |

bindings 在 19 個 suite（Editor-TipTap 在 Chrome 那一邊無效，沒有 ms）合計約 235 ms（開始前約 300 ms，換算的基準不同）：jQuery 約 59 ms、chartjs 約 40 ms、Perf-Dashboard 約 21 ms、ES5 約 14 ms、ES6-Webpack 約 13 ms。依「在付什麼」分組（ms，19 個 suite 合計；`find_map` 裡內聯的 `Object::fetch` 算 receiver，`Handle` 的 drop 算 root）：

| 在付什麼 | ms |
| --- | ---: |
| 共用的分派（`call_member`、`properties::call`、`custom_elements::call_member`、`after_call`） | 41 |
| 參數轉換（`convert_args`、`convert_arg`、`from_js`、`from_value`） | 27 |
| receiver 的種類和 unwrap | 26 |
| root（receiver、參數、結果的 `Handle`） | 25 |
| V8 API callback 的入口 | 18 |
| 回傳值和 wrapper | 13 |
| 宣告表的 thunk（`DESC::call`） | 12.5 |
| 開 DOM 的 scope | 9 |
| 其他（大多是 cppgc 的 `Member` 讀寫，其實是 DOM 的成本） | 63 |

第 3 段的兩個做法省得到的：

- **專用的 thunk**：省的是參數經過 `Value` 的那一段、thunk 拆 `Value` 和回傳值的包裝；入口、分派、receiver、root 不變。全部的介面合計約 20–25 ms（jQuery 約 4 ms、1%，chartjs 約 8 ms、3%），只做 canvas 2D 的話 chartjs 約 6–7 ms、Perf-Dashboard 約 3 ms。
- **canvas 2D 的 Fast API call**：跳過入口、分派、root 和 `Value`，但只有 TurboFan 編過的呼叫點會用（V8 的 `maglev_inline_api_calls` 預設關閉，Maglev 的照舊走 API callback）。V8 的 Turboshaft 在 fast call 之後檢查 isolate 上的例外（`fast-api-call-lowering-reducer.h`），所以 fast callback 可以丟例外；`v8-fast-api-calls.h` 開頭註解裡的 `fallback` 已經不在 `FastApiCallbackOptions` 裡。chartjs 的 canvas 呼叫很多不合用：`fillStyle`、`strokeStyle` 這類 setter 收字串，`fill()`、`stroke()` 有 overload，`arc` 會丟 `IndexSizeError`。粗估 chartjs 15–20 ms、Perf-Dashboard 5–8 ms，實際要數過有多少呼叫合用。

順帶看到、不在 bindings 裡的：

- **canvas 的顏色解析**：chartjs 的 canvas 2D 的 thunk 底下（連實作約 20% 的 cycle），`fillStyle`、`strokeStyle` 的 setter（`set_style_value`）佔 6.4%，其中 `css::canvas::parse_canvas_color` 4.9%、`dom_api::frame::current_system_colors` 1.1%：每次設定都重新解析顏色字串，約 16 ms，比專用的 thunk 在整套省的還多。其他是 `fill` 的 `flush` 3.8%、`measure_text` 1.7%。
- **新節點第一次判斷種類**：jQuery 的 `node_kind` 自己 1.1%，幾乎都在建 wrapper 時（新的元素第一次照名稱查表）。

## 和 Blink 的對照

| | Blink | kun |
| --- | --- | --- |
| binding 函式 | `bind_gen` 從 IDL 替每個 operation、attribute 產生一個 V8 callback | 每個成員共用 `call_member`，經過宣告表的 thunk |
| 參數 | `NativeValueTraits<T>` 直接轉成 C++ 型別 | 先轉成 `Value`、收進 `Vec`，thunk 再轉 |
| 回傳值 | `ToV8Traits<T>` 直接轉 | 包成 `Value` 再轉 |
| overload | 產生時展開成依參數個數和型別的分支 | 建 template 時標記哪些成員有 overload，呼叫時照介面和成員的索引拿 form，再依參數解析 |
| receiver | `FunctionTemplate` 的 signature，V8 檢查是不是那個介面的 instance；之後 `ToScriptWrappable` 一次 | 照介面能收的 tag 逐一試 `Unwrap`（節點一次就成功）；節點的種類第一次算完記在節點上 |
| wrapper 的 template | 照 `WrapperTypeInfo` 的指標，每個 context 一張表 | 照物件的種類查介面的 index（`interface_index_of`） |
| 很熱的簡單方法 | `[NoAllocDirectCall]` 走 V8 的 Fast API call | 無（量過，不做，README 第 2 條） |
| `[CEReactions]` | 產生的程式碼裡一個 stack 上的 scope | `custom_elements::call_member` 依 `ce_reactions` 判斷 |

## 參考資料

- Blink 的 binding 產生器：Chromium 的 `third_party/blink/renderer/bindings/scripts/bind_gen/`
- V8 的 Fast API call：`v8/include/v8-fast-api-calls.h`；rusty_v8 的 `src/fast_api.rs`
- WebIDL 的 overload resolution：<https://webidl.spec.whatwg.org/#es-overloads>
