# RFC 0101 的量測：`createElement` 和建節點的頁為什麼比 Chrome 慢

2026-11-15 量的，RFC 開始前。之後每一段的數字在 `progress.md`。

## 量到的

machopo 是 `main` 的 `3cbab18`（`perf` profile）。機器：開發機（Linux、AMD Ryzen AI 9 HX 370）。

### 整頁對 Chrome for Testing 152（`bench.py blink-perf`，各一次；同一頁兩次之間約有一成的起伏）

| 頁 | machopo | Chrome | 慢幾倍 | 2026-11-13 |
| --- | ---: | ---: | ---: | ---: |
| `bindings/create-element` | 2524 runs/s | 3579 runs/s | 1.42 | 1.50 |
| `bindings/gc-forest` | 90.1 ms | 66 ms | 1.37 | 1.58 |
| `bindings/gc-mini-tree` | 109 ms | 93 ms | 1.18 | 1.10 |
| `bindings/gc-tree` | 125 ms | 119 ms | 1.05 | 1.07 |

四頁都是 `createElement` 的迴圈：`create-element` 每輪 5000 次 `document.createElement("div")`；`gc-forest` 一百萬次再強制 GC；`gc-mini-tree` 建 `div`、建 `span`、接上，五十萬次再 GC；`gc-tree` 建 `div` 接到同一個根底下，一百萬次再 GC。

### 每一圈的成本，整個 process（對本機的 Chromium；`perf stat` 量兩種迴圈長度相減，各三次取最小，減掉空迴圈）

| 迴圈 | Chrome 指令 | machopo 指令 | 倍數 | Chrome cycles | machopo cycles | 倍數 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 建一個 `div` | 1583 | 1990 | 1.26 | 381 | 406 | 1.07 |
| 建 `div` 和 `span` 再接上 | 4675 | 4781 | 1.02 | 1144 | 968 | 0.85 |
| 建 `div` 接到根底下 | 3130 | 2975 | 0.95 | 713 | 588 | 0.82 |
| `createTextNode('a')` | 2145 | 2075 | 0.97 | 593 | 459 | 0.77 |

量的是所有 thread（Chromium 連同其他 process），所以含攤進去的 GC。第一次量時迴圈太短，Chrome 的空迴圈量出負的；之後空迴圈改用長的迴圈量。

### GC 和其他 thread（`bench.py blink-perf --gc`，各三次；machopo / Chrome，ms）

| 頁 | 計時的區間 | main thread 上的 GC | 其他 thread 的工作 | 其中 DOM 的 heap |
| --- | ---: | ---: | ---: | ---: |
| `create-element` | 15593 / 15762 | 1204 / 2984 | 1391 / 5198 | 56.6 / 3072 |
| `gc-forest` | 1965 / 1336 | 241 / 306 | 375 / 526 | 32.7 / 231 |
| `gc-mini-tree` | 2365 / 2041 | 242 / 315 | 361 / 645 | 32.7 / 391 |
| `gc-tree` | 2720 / 2548 | 270 / 332 | 707 / 867 | 172 / 488 |

### machopo 的 `createElement` 照函式（`perf record -e cycles:u`，兩千萬次的迴圈減空迴圈；每次呼叫約 354 cycles，都在 main thread）

| cycles | 函式 |
| ---: | --- |
| 77 | V8 的 `ExternalEntityTable<CppHeapPointerTableEntry>::TryAllocateEntry` |
| 43 | `dom::Node` 的 `__init` |
| 33 | `info_method_trampoline`（callback 的進出） |
| 19 | V8 的 `Factory::CopyJSObjectWithAllocationSite` |
| 17 | `dom::names::find_fixed` |
| 16 | `dom::Cx::node` |
| 13 | `dom_js::templates::new_wrapper` |
| 12 | `LentString::chars` |
| 12 | `StateInfo::with_scope` |

## 讀出來的

- **GC 不是 machopo 慢的原因，是它贏的地方**：兩邊都有背景的 GC（machopo 是 V8 的 concurrent marking 加 elise 的 concurrent marking 和 sweep），但 Chrome 的 GC 總量多很多，main thread 和其他 thread 都是（DOM 的 heap：Oilpan 合計約 4300 ms，elise 約 110 ms）。Chrome 的 GC 算進「整個 process」把它的總數墊高，所以照整個 process 算兩邊差不多；扣掉 GC，Chrome 在 main thread 上建一個元素比較快。（這一條原本寫成「Chrome 把 GC 放到別的 thread」，不準。）
- **照 main thread 換算**（從 `create-element` 的分數和 GC 的比例算的，不是直接量的）：每次 `createElement` machopo 約 79 ns、Chrome 約 56 ns；扣掉 main thread 上的 GC（8% 和 19%）是約 73 對 46 ns，1.6 倍。慢的是建節點和建 wrapper，不是 GC。
- **最大的一項是 V8 的指標表替 wrapper 配一格**（77 cycles，22%）：機器碼裡時間幾乎都在一個 `lock cmpxchg` 上。為什麼這麼貴、Chrome 在同一處花多少，還沒查。
- 第二項是節點先在 stack 上組好再搬進 heap（`Node::__init`，43 cycles）。

## 兩個 Chrome 差很多（同一天後來量的）

之前「對本機 Chromium」的比較用的是 Fedora 的 Chromium 156；blink_perf 用的是 Chrome for Testing 152。頁面自己計時（`performance.now`，兩百萬次的迴圈七輪取最小），每次呼叫的 ns：

| 迴圈 | Chrome for Testing 152 | Fedora 的 Chromium 156 | machopo | machopo ÷ CfT | machopo ÷ Chromium |
| --- | ---: | ---: | ---: | ---: | ---: |
| `createElement('div')` | 53.2 | 87.5 | 80.2 | 1.51 | 0.92 |
| 建 `div` 和 `span` 再接上 | 152.8 | 187.7 | 184.6 | 1.21 | 0.98 |
| 建 `div` 接到根底下 | 100.5 | 128.4 | 112.2 | 1.12 | 0.87 |
| `createTextNode('a')` | 74.9 | 102.6 | 94.3 | 1.26 | 0.92 |
| `nodeType` | 3.3 | 3.3 | 4.2 | 1.26 | 1.26 |
| `getAttribute('id')` | 19.2 | 20.7 | 18.0 | 0.93 | 0.87 |

- 會建節點的呼叫，Chrome for Testing 比 Fedora 的 Chromium 快三到四成；getter 兩邊一樣。所以**對本機 Chromium 量的、會配置的項目都把 Chrome 量慢了**（上面「每一圈的成本」那張表、RFC 0097 的 63 項裡建物件的那些）：`createTextNode` 對 Chrome for Testing 其實慢 1.26 倍，不是打平。getter 和字串參數的項目不受影響。
- 對 Chrome for Testing 的倍數（1.51、1.21、1.12）和 blink_perf 整頁的（1.42、1.18、1.05）對得上。

`createElement` 每次呼叫照 thread（`perf record` 取樣兩千萬次的迴圈、減空迴圈；Chrome 用和上表同樣的啟動方式）：

| | main thread 的指令 | main thread 的 cycles | 其他 thread 的 cycles |
| --- | ---: | ---: | ---: |
| Chrome for Testing 152 | 1234 | 284 | 108 |
| Fedora 的 Chromium 156 | 1386 | 314 | 120 |
| machopo | 約 1990 | 354 到 410 | 很少 |

machopo 在 main thread 上每建一個元素多約 750 條指令（1.6 倍）。Chrome 的數字隨跑法變很多（`--dump-dom` 開 `file://` 的頁、同樣的迴圈，Fedora 的 main thread 量到 2048 條、511 cycles；釘在一顆核心上整個 process 只有 1583 條），所以只拿同一種跑法的互相比。Chrome for Testing 用 `--dump-dom` 不會結束，要用 HTTP 的頁自己回報。

查過、排除的：

- `TryAllocateEntry` 每個 wrapper 只呼叫一次（gdb 數的）；整個 `createElement` 每次只有 0.44 次最後一級的 cache miss、4.3 次 L1 miss，不是 cache miss。它的 77 cycles 有一部分是歸因的問題：`lock cmpxchg` 要等前面的都做完，前面累積的延遲算到它頭上。Fedora 的 Chromium 同一個函式是 36 cycles。

## machopo 的一次 `createElement('div')`，逐條指令（gdb 單步，RFC 0097 的 `prototypes/step-callback.py`）

callback 本身（從 trampoline 進來到回去，沒有 GC）是 **1658 條**，數字是精確的。整個 process 每次 1990 條，其餘約 330 條是 V8 呼叫 callback 的 builtin、JS 的迴圈和攤進來的 GC。Chrome for Testing 在 main thread 上連 GC 總共 1234 條。

照做的順序分段：

| 條數 | 做什麼 | 裡面有什麼 |
| ---: | --- | --- |
| 約 260 | 讀名稱、猜是哪個 interface（操作前） | `LentString::text_lends` 40、`chars` 28、`foresee_element_words` 75、`names::find_fixed` 84（只查一次） |
| 約 80 | 看 `this` 的 realm 是不是目前的 realm | `GetCurrentContext` 19、`GetCreationContext` 46、`v8__Data__EQ` 14 |
| 約 340 | 開 scope、預先建 wrapper | `with_scope` 50、`new_wrapper` 63、`assert_access_allowed` 23、`v8__Data__EQ` 14、`v8__Local__New` 18、複製樣板物件（`Object::Clone` 到 `CopyJSObjectWithAllocationSite`）約 170 |
| 約 490 | 操作：建節點 | callback 裡 inline 的 131、`create_element` 69、`is_valid_element_local_name` 38、`make_element` 57、`Cx::node` 84、`Node::__init` 80、`QualifiedName::new` 32 |
| 約 255 | 把 wrapper 接上節點 | `unified_heap::wrap` 49、`IsApiWrapper` 兩次共 44、`IsWrapping` 17、`v8::machopo::Wrap` 51、`WriteLazilyInitializedCppHeapPointerField` 45、`TryAllocateEntry` 49 |
| 約 92 | 節點指回 wrapper 的參照 | `GlobalizeTracedReference` 65、`SetDroppable` 27 |
| 約 140 | callback 的頭尾和其他 inline 的 | |

看起來可以省、不動架構的（合計約 150 到 250 條；還沒試）：

- 剛建好的 wrapper 接上節點時檢查兩次 `IsApiWrapper`、一次 `IsWrapping`（約 60）：這個 wrapper 是同一次呼叫裡從樣板複製出來的，一定是、也一定還沒接上東西。
- 名稱已經在固定名稱表裡找到了，還照文字檢查一次是不是合法的 local name（38）。
- `new_wrapper` 拿樣板物件時的 `assert_access_allowed`、`Local::New`、比 context（約 55）。
- 看 realm 的那 80 條：Blink 也問 creation context；有沒有更便宜的問法還沒看。

省不掉、要另外設計的：名稱從 V8 的字串變成編號（約 230；Blink 是 `AtomicString` 的字串快取加 hash）、建節點本身（約 300）、開 scope（50）。

## 兩大塊能怎麼做（2026-11-15 讀程式之後的想法，都還沒試，條數是估的）

### 名稱從 V8 的字串變成編號（現在約 230 條）

現在：確認字串借得到文字（`text_lends` 40）、讀出文字（`chars` 28）、看有沒有大寫再用 perfect hash 查固定名稱表、比對文字（`foresee_element_words` 75、`find_fixed` 84）。之前量過不做的（RFC 0097 的決定 7）：最近查過的名稱的小快取（每個名稱省約 40 條；Blink 沒有這一層）、external string（短的名稱要 V8 的 sandbox；`'id'` 連 Chrome 也每次查表）。

沒試過的做法：**用 V8 的 internalized 字串的身分查**。JS 原始碼裡的字面值（`'div'`、`'id'`）是 internalized 的，同樣內容的在一個 isolate 裡只有一個物件，header 裡有 V8 算好的 hash。isolate 這一側留一張表：V8 的 hash → 名稱的編號和那個名稱的 internalized 字串的 handle；查的時候讀參數的 hash、找到那一格、比兩個字串是不是同一個物件。對得上就是那個名稱，不讀文字、不算 hash、不比內容。估約 20 到 30 條，每個字面值的名稱省約 150 到 200 條，所有拿名稱的成員都受惠（`getAttribute`、`setAttribute`、`hasAttribute`、`createElement`……）。

- 表是懶惰地填的（第一次照文字查到之後登記），大小以用過的名稱為限，不是會淘汰的快取；不是字面值的字串（串接出來的、`ThinString`）對不上，走現在的路。
- 要的東西：fork 裡照 V8 的佈局讀字串的 hash 欄位和「是不是 internalized」（和現有的 `as_string`、`one_byte_chars` 同一類，只改 Rust、不重編 V8）；handle 會跟著 GC 更新位址，所以比位址是安全的。
- 還沒確認的：hash 欄位對 internalized 字串是不是一定算好了；V8 升級時佈局變動的風險；這比 Blink 做得多（Blink 對短字串照文字查 `AtomicString` 的表）。

不動設計也能省的：名稱已經在表裡找到時不再照文字檢查合法（38 條）、不再掃一次有沒有 `-`（名稱表可以帶這個旗標）。

**原型（2026-11-15，只接在 `createElement` 猜 interface 的那一步）**：fork 加 `v8::machopo::internalized_identity`（照佈局讀 instance type 的 `kIsNotInternalizedMask` 和 map 後面的 `raw_hash_field`，回 hash 和位址），`dom_js` 的 `foresee_element_parts` 先用 hash 找一張 256 格的表、比位址，對不上才照文字查、查到再登記。

| | 之前 | 原型 |
| --- | ---: | ---: |
| `createElement` 每次（`bench.py calls`） | 2048 條、448 cycles（另一次量 412） | 1887 條、382 cycles |
| callback 本身（gdb 單步） | 1658 條 | 1534 條 |
| 其中猜 interface 那一步 | 187 條（`foresee_element_words` 75、`chars` 28、`find_fixed` 84） | 30 條 |
| store forwarding 的衝突 | 2.87 | 4.83 |

- 做得到：internalized 的字面值讀得到 hash，比位址對得上；`dom_js` 的測試都通過。`getAttribute`、`nodeType` 沒有接、數字不變。
- 衝突多了約 2 次，還沒查（原型的表一格是 16 bytes 的 tuple，可能是它的讀寫）；正式做的時候要一起看。
- 原型偷懶的地方：表記的是字串的位址、不是 handle，GC 搬動字串之後那一格就失效（只是查不到、重新登記；理論上另一個同 hash 的字串搬到同一個位址會認錯，正式的要用 handle）；表是 thread-local 的、只有 256 格、撞到就蓋掉。
- 還沒省的：參數進來時的 `text_lends`（43 條）和操作裡照文字的檢查（`is_valid_element_local_name` 38、找 `-`）仍然做，因為 `create_element` 還拿文字。名稱由身分認出來時這些都可以不做，估再省約 100 條。
- patch 在 `prototypes/`（`name-identity-machopo.patch`、`name-identity-rusty_v8.patch`），沒有進 `main`。

### 建節點（現在約 490 條）

| 條數 | 現在 | 可以怎麼做 |
| ---: | --- | --- |
| 164 | `Cx::node` 84 加 `Node::__init` 80：13 個欄位的節點（約 48 bytes）先組一個 init 的 struct 再逐欄寫進 heap，配置走泛用的路（site 的計數、size class、有沒有 JS 的邊） | 節點的初始值除了 owner 和 flags 都是 0：配置一塊清成 0 的再寫兩格。要 elise 給「全部是空的」的配置（之前決定暫時不做的那一項的小版本） |
| 57 | `make_element` 照名稱一個一個比：template、表單的幾種、SVG 的 `use`、`iframe`、`base` | 名稱表給每個名稱一個「建立時要多做事」的旗標，一次查表，要做事的才進去 |
| 107 | `create_element` 69 加 `is_valid_element_local_name` 38 | 名稱已經有編號時跳過合法性的檢查和找 `-` |
| 32 | `QualifiedName::new` | HTML 的命名空間、沒有 prefix 的名稱可以直接組 |
| 131 | callback 裡 inline 的：參數、進操作（一個會進安全點的操作 44 條）、結果 | 還沒細看 |

估計建節點這一塊可以從約 490 降到約 250 到 300。

### 加起來

多餘的檢查約 150 到 250、名稱約 150 到 200、建節點約 200：合計約 500 到 650 條，是差距（約 750 條）的大半。全部做到的話 `createElement` 從 1990 降到約 1350 到 1500，Chrome for Testing 是 1234。都是估的；之前的經驗是指令數省了不一定換到時間，要連 cycles 和 store forwarding 一起量。

## 量法上要記得的

- 用 `instructions` 取樣拆函式不可靠：取樣點會被慢的指令（`lock cmpxchg`）吸過去，`TryAllocateEntry` 看起來有 411 條指令，是假象。拆函式用 `cycles`。
- `perf record` 取樣太密會被節流，總數只剩一部分：先對一下取樣的總數和 `perf stat` 的。
- 和 Chrome 比會配置的呼叫時要看 main thread，整個 process 的指令數會低估差距。
- 和 Chrome 比要用 Chrome for Testing（blink_perf 和 Speedometer 用的那個），不要用發行版的 Chromium：它建節點慢三到四成。

## 開始前還不知道的

- machopo 多的約 750 條指令在哪：Chrome for Testing 沒有符號，Fedora 的 Chromium 有符號但比較慢（main thread 1386 條），只能拿它看 Blink 走哪些函式。machopo 這邊照指令取樣的前幾名（有偏差，只當線索）：`Node::__init` 249、callback 的進出 159、`names::find_fixed` 85、`CopyJSObjectWithAllocationSite` 84、`Cx::node` 76、`with_scope` 58、`new_wrapper` 56、`LentString::chars` 50，和建 wrapper 前後幾個 rusty_v8 的檢查（`v8__Data__EQ`、`IsWrapping`、`GetCreationContext`、`assert_access_allowed`、`GetCurrentContext`，合計約 120）。
