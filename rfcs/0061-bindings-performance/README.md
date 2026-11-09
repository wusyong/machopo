# RFC 0061：bindings 的效能

- 開始日期：2026-10-19
- 狀態：✅ 已完成：2026-10-20（呼叫的路上照種類和索引查表、不比字串、少配置；節點記住自己的種類、overload 照成員的索引拿、呼叫的收尾先看一眼；TodoMVC-jQuery 的取樣 cycle 約 −13%、ES5 約 −7%、Charts-chartjs 約 −4%，bindings 佔全部 cycle 的比例 13.4% → 11.5%。專用的 thunk 和 canvas 2D 的 Fast API call 量過、不做；在 Linux 上做）
- 進度和驗收：`progress.md`
- 細節：`details.md`（取樣的數字、一次呼叫走過的路、各段之後的量測、和 Blink 的對照）

## 摘要

JS 呼叫 DOM API 時夾在 V8 和 DOM 實作之間的那一層（`dom_js` 的轉換和分派、`dom_api` 宣告表產生的 thunk），RFC 開始時在 Speedometer 的 20 個 suite 合計約 300 ms，約是 Chrome 跑完整套的三成。每次呼叫都走同一條通用的路：參數先轉成通用的 `Value`，經過 `dyn ScriptHost` 和宣告表的 thunk 再轉成實作要的型別，結果再包成 `Value` 轉回 JS；成員和 template 用名稱字串找。

做了兩段，行為都不變：

- **第 1 段，路上多做的事**：wrapper 的 template 照物件的種類查、overload 只在有的成員上找、indexed 和 named 的特殊操作照介面的索引存著、參數和 receiver 的 tag 不再每次配置。
- **第 2 段，針對量出來的大頭**：節點的種類第一次算完就記在節點上、overload 照介面和成員的索引拿、宣告表的 thunk 結尾的收尾先問一次有沒有事要做。

原本的第 3 段（讓宣告表替每個成員產生直接從 V8 取參數的 thunk，Blink 從 IDL 產生的做法；或讓 canvas 2D 只收數字的方法走 V8 的 Fast API call）在第 2 段之後量過，不做：bindings 剩下約 235 ms，專用的 thunk 只省得到其中參數和回傳值的那一段（合計約 20–25 ms），Fast API call 只對 chartjs、Perf-Dashboard 有用，數字不確定，又要多一塊 `unsafe`（第 2 條）。

## 動機

- **bindings 是 script 裡唯一大到能動比值的一塊**：2026-10-19 對 20 個預設 suite 各取樣一次（`main` 的 `0bd98042`），照各 suite 的 ms 換算：style、layout、paint 約 790 ms，JS 本身約 490 ms，DOM 操作約 480 ms，**bindings 約 300 ms**，canvas 的 raster 約 200 ms，GC 約 30 ms（`details.md` 的「時間花在哪」）。JS 本身是 V8 的事（PGO 估過只有 20–40 ms，RFC 0058）。bindings 最多的是 TodoMVC-jQuery 71 ms、Charts-chartjs 55 ms、Perf-Dashboard 26 ms、ES6-Webpack 21 ms、ES5 21 ms。
- **路上有明顯多做的事**（`details.md` 的「一次呼叫走過的路」）：
  - 建 wrapper 時用介面名稱字串在整張介面表裡逐一比對，找它的 template（`templates::index_of`），是 `memcmp` 最大的來源之一；
  - 每呼叫一個方法，`dom_api::overloads` 都把整張 overload 群組表掃一遍，比介面和成員的名稱，沒有 overload 的方法也一樣；
  - indexed、named 的存取（集合的 `[i]`、`form.name`）每次先用介面名稱在整張介面表裡找介面，再逐一找 special 的成員（`inherited_member`）；
  - 每個參數各配置一個 `Vec`（為了 variadic），static 方法每次配置一個 `Rc`。
- **通用的路本身的成本**：`Vec<Value>`、`Value` 的兩次轉換、`dyn ScriptHost`、`Result<Value>`、每次呼叫才解析的 overload。Blink 從 IDL 替每個方法產生專用的 binding，這些都在產生時決定。

目標：

- bindings 的時間下降，不改任何行為。
- 宣告表照舊是 V8 和 wasm（`kun:dom`）共用的一份；`dom_api` 照舊和 engine 無關（`docs/architecture.md` 的「V8 和 `unsafe` 的位置」）。
- WebIDL 的語意不變：參數的轉換順序和例外、`[CEReactions]`、overload、`[LegacyUnforgeable]` 這些。

不在範圍內：

- JS 本身（V8 的 build 設定、PGO）。
- DOM 操作本身（`link_child`、`walk_moved_subtree`、`update_elements`、`querySelectorAll`，約 480 ms）：另一份 RFC。
- wasm 那一邊的效能：它照舊走 `Value` 的路，只要不變慢。
- 宣告表以外的 binding（iterable、promise、事件的 callback）。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

對網頁和使用者：行為不變，只是變快。

對之後改 bindings 的人：

- 路上用到的表（wrapper 的 template、成員的 overload、indexed 和 named 的特殊操作、receiver 能是的型別）都照物件的種類、介面的索引或成員的索引查，不比字串。新增介面時照舊在宣告表加一份，表由它產生。
- 節點的種類算一次就記在節點上（`dom` 的 `Node::binding_kind`）：決定種類的只能是建立時就固定的東西，debug build 每次照舊算一次比對（`docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」）。
- 宣告表的 thunk 結尾的收尾（`after_call`）先問 `dom` 的 `has_call_effects`：新的一種「呼叫完要做的事」要加進它。
- 宣告表的 thunk 照舊只有一種（收 `Vec<Value>`），V8 和 wasm 共用。

## 詳細設計

### 第 1 段：路上多做的事

1. **wrapper 的 template 照種類查**：`dom_api` 照 `ObjectKind` 排的介面表（`BY_KIND`）直接給介面在 `INTERFACES` 的索引（`interface_index_of`），`new_wrapper` 收索引，不再用名稱找。
2. **overload 只在有的成員上找**：成員的編號（function 的 data）多一個 bit（`OVERLOADED`），建 template 時查過它是不是 overload 的一個 form；沒有的方法呼叫時不找。
3. **indexed、named 的特殊操作照介面的索引存著**：每個介面一份先找好的（indexed getter、setter、`length`、named getter、setter、deleter、supported names，`properties.rs` 的 `Specials`，每個 process 建一次），interceptor 的 data 本來就帶著介面的索引。
4. **少配置**：非 variadic 的參數不再各配一個 `Vec`（`convert_arg`）；receiver 能是的 tag 每個 process 算一次（`receiver_tags`，`&'static [ObjectTag]`），static 方法不再每次配置 `Rc`。

查過、不改的：
- **回傳不會是 null 的 indexed getter 先取 `length`**：只有 `CSSStyleDeclaration`，WebIDL 要求（index 小於 `length` 才是支援的 index），照舊。
- **receiver 逐一試 tag**：節點都是同一個 tag（`Node`），排第一個，`Node`、`Element` 的成員第一次就成功；只有 `EventTarget` 這類收好幾種型別的介面遇到不是節點的物件才多試。V8 的 `Unwrap` 收 tag 的範圍，但 fork 的 binding 只開放單一 tag，要用範圍得改 fork 的 C++、發新的 release，效益不值得。

### 第 2 段：針對量出來的大頭（第 1 條）

第 1 段之後再取樣（jQuery、chartjs、ES5 各三次），bindings 剩下的成本和原本假設的不同：參數經過 `Value` 的轉換在 jQuery 只有約 0.5%（chartjs 3.6%），jQuery 最大的一塊是每次呼叫判斷 receiver 是哪種物件（`Object::kind`，元素照標籤名稱查表，加上 `check_this`，約 4.4%）。各部分的數字在 `details.md` 的「第 1 段之後」。

1. **節點記住自己的種類**：元素的種類只看它的名稱（建立後不變），原本每次 `Object::kind` 都照標籤名稱查表（`html_element::element_kind`），建 wrapper 和 `check_this` 各一次。`dom` 的 `Node` 多一個 `binding_kind`（`Cell<u16>`），`dom_api` 第一次算完存在節點上，之後直接讀（Blink 的節點是哪個 C++ 類別在建立時就固定了）。
2. **overload 照成員的索引拿 form**：第 1 段只讓沒有 overload 的方法不找；有的（canvas 2D 的 `fill`、`stroke`、`drawImage` 這類）原本每次呼叫照名稱掃整張群組表。`dom_api` 的群組表多存一張以介面和成員的索引為 key 的表（`overloads_at`）。
3. **呼叫的收尾先看一眼**：`V8Host::mutate` 每次組一個約 20 個參考的 `DomMutationScope`（chartjs 1.7%、jQuery 2.0%），查下來是開得太多次：宣告表替方法和 setter 產生的 thunk，每次呼叫完都跑四步收尾（`update_elements`、mutation observer 和 custom element 的 microtask、`selectionchange`），每一步開一次 scope 取一個旗標或清單，大部分時候全是空的。四步合成 `after_call`，先用 `dom` 的 `has_call_effects` 開一次 scope 問有沒有事要做，沒有就全部跳過。
4. **每次呼叫從 isolate 拿 DOM 的狀態**：查過不做。`BindingContext::new` 每次呼叫只查一次 isolate 的 slot（以 `TypeId` 為 key 的雜湊表，雜湊本身幾乎不花），省不了多少。

### 原本的第 3 段：量過，不做（第 2 條）

第 2 段之後對 20 個 suite 再取樣（`details.md` 的「第 2 段之後」），bindings 佔全部 cycle 的 11.5%，19 個 suite 合計約 235 ms：共用的分派約 41 ms、參數轉換 27 ms、receiver 26 ms、root 25 ms、V8 的入口 18 ms、回傳值 13 ms、宣告表的 thunk 12.5 ms、DOM 的 scope 9 ms。

- **每個成員一個專用的 thunk**：宣告表的 macro 多產生一個對「參數來源、結果去處」泛型的 thunk，`dom_js` 用直接讀寫 V8 的版本具現化，`dom_wasm` 照舊用 `Value` 的。它只省得到參數經過 `Value` 的那一段、thunk 拆 `Value` 和回傳值的包裝，入口、分派、receiver、root 都不變：全部的介面合計約 20–25 ms（jQuery 約 1%、chartjs 約 3%），只做 canvas 2D 約 10 ms。代價是宣告表的每個成員多一份具現化、編譯變慢，兩種 thunk 要交叉檢查。
- **canvas 2D 的 Fast API call**：V8 讓 TurboFan 編過的程式直接呼叫一個 C 函式，不經過 API callback 的入口；只收數字這類簡單的型別、不能配置 JS 物件（例外可以丟：Turboshaft 在 fast call 之後檢查）。Maglev 預設不用它（`maglev_inline_api_calls`）。chartjs 的 canvas 呼叫很多不合用（收字串的 `fillStyle`、`strokeStyle`，有 overload 的 `fill()`、`stroke()`，會丟例外的 `arc`），粗估 chartjs 15–20 ms、Perf-Dashboard 5–8 ms，不確定。fast callback 是 `extern "C"`，只能放在 `bindings`（`unsafe` 只在 `dom`、`bindings`、`ipc`），要另外的安全論證，宣告表也要替這些方法產生帶型別的入口。

### 測試

- 行為不變：現有的 `dom_js`、`dom_api`、`dom_wasm` 的測試，和 `test.py wpt` 的相關目錄（`dom/`、`custom-elements/`、`selection/`、`html/dom/`、`html/canvas/element/`、`webidl/`）。
- 節點記住的種類和照名稱算的一樣：debug build 每次比對（`docs/testing.md` 的「快取和增量的交叉檢查」）。
- 收尾的每一種事（元素的變化、表單狀態、mutation observer 和 custom element 的 microtask、`selectionchange`）各自都會讓 `has_call_effects` 成立：`dom` 的測試。
- mutation test：每一段的捷徑弄壞，確認有測試失敗（`progress.md`）。
- 效能的量測不排進測試計畫（`docs/benchmarks.md` 的「效能的量測」）。

### 分階段

1. **路上多做的事**（第 1 段的 1–4）。
2. **針對量出來的大頭**（第 1 條）：節點記住種類、overload 照索引、收尾先看一眼；DOM 狀態的查找查過不做。
3. **專用的 thunk 或 Fast API call**：量過，不做（第 2 條）。

## 缺點

- 節點多存一個種類（一個 `u16`）；它只能依建立時就固定的東西決定，debug build 比對。
- 新的一種「呼叫完要做的事」要記得加進 `has_call_effects`，否則只有它待處理的呼叫什麼都不做（`docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」）。
- 每次呼叫照舊經過 `Value` 和共用的分派：kun 的 bindings 每次呼叫的成本仍然比 Blink 產生的專用 binding 高。

## 理由和其他做法

- **先修路上多做的事、再改架構**：第 1、2 段幾天就做完、行為不變，量完才知道專用的 thunk 剩多少空間，避免先做大改動（第 1 條）。量下來剩的空間不值得改架構（第 2 條）。
- **不另外寫一個產生器**（Blink 的 `bind_gen` 從 IDL 產生 C++）：效益最大，但要兩份宣告保持一致，以 kun 現在的規模不划算（第 1 條）。
- **不讓 `dom_api` 依賴 V8**：V8 的部分留在 `dom_js`。

## 先例

- **Blink**：`bindings/scripts/bind_gen` 從 IDL 替每個 operation、attribute 產生專用的 V8 callback，參數直接轉成 C++ 型別（`NativeValueTraits`），回傳值直接轉（`ToV8Traits`），overload 在產生時展開；receiver 的型別由 `FunctionTemplate` 的 signature 檢查；wrapper 的 template 照 `WrapperTypeInfo` 的指標找；少數熱的方法標 `[NoAllocDirectCall]` 走 V8 的 Fast API call。
- **Servo**：`codegen` 從 WebIDL 產生 SpiderMonkey 的 binding，一樣是每個成員專用的函式。
- **V8**：Fast API call（`v8-fast-api-calls.h`），Chrome 用在 canvas、WebGL 的一些方法。

## 已決定的事項

1. **2026-10-19，第 1 段做完、再取樣之後：第 2 段改成針對現在的路修**（節點記住種類、overload 照成員編號、DOM 的 scope、DOM 狀態的查找），專用的 thunk 和 Fast API call 移到第 3 段、第 2 段之後量過再決定。第 1 段之後 jQuery 的 bindings 裡參數轉換只有約 0.5%，原本的第 2 段（專用的 thunk）在 jQuery 只省得到約 2–3%，而 receiver 的種類判斷、scope、root 這些它都不碰；這幾項成本小、行為不變。不選另寫一個產生器（Blink 的 `bind_gen`）：效益最大，但要兩份宣告保持一致，以 kun 現在的規模不划算，除非專用的 thunk 做完還不夠。
2. **2026-10-20，第 2 段做完、對 20 個 suite 再取樣之後：第 3 段不做，這份 RFC 在第 2 段結束**。不選專用的 thunk（全部的介面）：合計約省 20–25 ms，不到 Speedometer 總時間的 1.5%，卻要動整張宣告表、多一種要交叉檢查的 thunk。不選只做 canvas 2D 的專用 thunk：約 10 ms，同一個介面有兩條路。不選 canvas 2D 的 Fast API call：效益只在 chartjs 和 Perf-Dashboard，合用的呼叫有多少不確定，又要多一塊 `unsafe` 和它的安全論證。量測時看到比它們都划算、不在 bindings 裡的兩件事記在「之後的可能」。

## 待決定

無。

## 之後的可能

- **canvas 的顏色解析**：chartjs 的 `fillStyle`、`strokeStyle` 每次設定都重新解析顏色字串（`css::canvas::parse_canvas_color` 加上 `current_system_colors`），佔 chartjs 約 6%（約 16 ms），比專用的 thunk 在整套省的還多。先查 Blink 的 `BaseRenderingContext2D` 怎麼避免重新解析。
- **新節點的種類在建立時就決定**：jQuery 建 wrapper 時第一次照名稱查表，約 1%。
- **canvas 2D 的 Fast API call**：先數 chartjs 和 Perf-Dashboard 有多少 canvas 呼叫合用，再估一次（第 2 條）。
- **專用的 thunk**：bindings 成為比值的主要部分時再考慮；wasm 那一邊（`kun:dom`）可以一起省掉 `Value`。
- 宣告表以外的 binding（iterable 的 `next`、事件的 callback 參數、promise）。
- DOM 操作本身的效能（約 480 ms，jQuery 和 ES6-Webpack 最多）。
