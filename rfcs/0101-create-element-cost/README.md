# RFC 0101：`createElement` 的成本往 Chrome 靠

- 開始日期：2026-11-15
- 狀態：🚧 已排程：2026-11-15
- 細節：`measurements.md`（開始前的量測和原型）

## 摘要

建一個元素，machopo 在 main thread 上比 Chrome for Testing 多約 750 條指令（約 1990 對 1234），blink_perf 的 `bindings/create-element` 慢 1.42 倍，三個 `gc-*` 頁（也是 `createElement` 的迴圈）慢 1.05 到 1.37 倍。GC 不是原因：machopo 的 GC 比 Chrome 少。這份分三段把 callback 裡的 1658 條指令往下拿：拿掉多餘的檢查、名稱用 V8 字串的身分查、建節點少做事。估計合計省 585 到 705 條；第二段的做法有原型，其餘是讀程式估的。

## 動機

- **差距在建節點和建 wrapper**（`measurements.md`）：四頁每一圈都在 `createElement`；頁面自己計時，`createElement('div')` 一次 machopo 80.2 ns、Chrome for Testing 53.2 ns（1.51 倍），`createTextNode` 1.26 倍。
- **之前把 Chrome 量慢了**：會配置的呼叫之前是對這台機器上 Fedora 的 Chromium 量的，它建節點比 Chrome for Testing 慢三到四成（getter 一樣）。對它量，machopo 看起來打平。
- **一次呼叫做了什麼是數得出來的**：gdb 單步，callback 本身 1658 條，照順序是讀名稱和猜 interface 約 260、看 realm 約 80、開 scope 和預先建 wrapper 約 340、建節點約 490、把 wrapper 接上節點約 255、節點指回 wrapper 約 92、頭尾約 140。
- 名稱那一段的做法對每個拿名稱的成員都有用（`getAttribute`、`setAttribute`、`hasAttribute`……），不只這四頁。

不在這份的範圍：GC；`createElement` 以外的建物件（event、range……）；序列化和解析（`innerHTML`、`cssText`）；最熱的 getter 剩下的差。

## 設計概要

對網頁沒有看得到的變化。三段各自獨立，每段做完都可以 merge：

| 段 | 做什麼 | 估計省下（條） | 把握 |
| --- | --- | ---: | --- |
| 1 | 拿掉多餘的檢查，預先建 wrapper 不開 scope | 165 到 245 | 接 wrapper 和拿樣板的檢查、scope 確定；realm 那一項要先看 |
| 2 | 名稱用 V8 字串的身分查 | 220 到 260 | 原型量到 124 到 161，其餘約 100 是估的 |
| 3 | 建節點少做事 | 約 200 | 讀程式估的，沒試 |

## 詳細設計

每一段的設計在那一段開始時定；這裡是範圍和已經知道的事。

### 第 1 段：多餘的檢查

| 現在的條數 | 是什麼 | 為什麼多餘 |
| ---: | --- | --- |
| 約 60 | 把預先建好的 wrapper 接上節點（`bindings::unified_heap::wrap`）時，`IsApiWrapper` 兩次、`IsWrapping` 一次 | wrapper 是同一次呼叫裡從樣板複製出來的：一定是 API wrapper，一定還沒接上東西 |
| 約 55 | `new_wrapper` 拿樣板物件時的 `assert_access_allowed`、`Local::New`、比 context | 每次呼叫重做同樣的事 |
| 約 80 | `this` 的 creation context、目前的 context、比較 | Blink 也問一次 creation context；有沒有更便宜的問法（例如只在 `this` 不是目前 realm 的 document 時才問）開始時查 |
| 約 50 | 為了預先建 wrapper 開的 scope（`StateInfo::with_scope`） | 樣板物件已經在的時候（除了每個 realm 每個 interface 的第一次），要做的只有問目前的 context、把樣板的 handle 拿出來、複製它：`bindings::InfoHandles`（不開 scope 就能配置的那一組）已經有 `current_context` 和 `clone_object`，缺的只有問 `this` 的 creation context。第一次建樣板時才開 scope |

第一項要一個不檢查的 `wrap`：`unsafe` 在 `bindings`，論證是「物件剛從樣板複製、還沒交給任何人」，寫進 `crates/bindings/soundness.md`。

### 第 2 段：名稱用 V8 字串的身分查

現在：確認字串的文字借得到（`text_lends` 40 條）、讀出文字（`chars` 28）、看有沒有大寫、用 perfect hash 查固定名稱表再比文字（`foresee_element_words` 75、`find_fixed` 84）；建節點時再照文字檢查合法（38）、找 `-`。

改成：JS 的字面值是 internalized 的字串，同樣內容的在一個 isolate 裡只有一個物件，header 裡有 V8 算好的 hash。isolate 的狀態留一張表：V8 的 hash 對到名稱的編號和那個名稱的 internalized 字串。查的時候讀參數的 hash、找到那一格、比是不是同一個物件；對得上就是那個名稱，不讀文字。對不上（不是字面值、第一次見）走現在的路，查到再登記。

- **原型**（`prototypes/name-identity-*.patch`，`measurements.md`）：只接在 `createElement` 猜 interface 的那一步，那一步 187 條變 30 條；`createElement` 每次 2048 → 1887 條、448（另一次量 412）→ 382 cycles；測試都過。store forwarding 的衝突 2.87 → 4.83，沒查。
- **正式的和原型不同的地方**：表記 handle 不記位址（GC 會搬字串）；放在 isolate 的狀態，不是 thread-local；大小和撞到時怎麼辦要定；名稱由身分認出來時，參數的 `text_lends` 和建節點時照文字的檢查都不做（名稱表帶「合法、沒有 `-`」的旗標），`dom_api::document::create_element` 在名稱已知時不拿文字。
- **fork**：`v8::machopo` 多一個照 V8 的佈局讀「是不是 internalized」和 hash 欄位的函式（和 `as_string`、`one_byte_chars` 同一類：只改 Rust、不重編 V8，debug build 對 V8 自己的呼叫檢查）。
- **推到其他成員**：屬性名稱（`getAttribute`、`setAttribute`、`hasAttribute`、`removeAttribute`）和 `getElementsByTagName` 的名稱是同一種查法；這一段先做 `createElement`，再照量到的決定要不要一起做。

### 第 3 段：建節點

| 現在的條數 | 現在 | 改成 |
| ---: | --- | --- |
| 164 | `Cx::node` 84 加 `Node::__init` 80：節點 13 個欄位（約 48 bytes），先組一個 init 的 struct 再逐欄寫進 heap，配置走泛用的路 | 節點的初始值除了 owner 和 flags 都是 0：配置一塊清成 0 的，再寫兩格 |
| 57 | `make_element` 照名稱一個一個比（template、表單的幾種、SVG 的 `use`、`iframe`、`base`） | 名稱表給每個名稱一個「建立時要多做事」的旗標，查一次，要做事的才進去 |
| 32 | `QualifiedName::new` | HTML 的命名空間、沒有 prefix 的名稱直接組 |
| 131 | callback 裡 inline 的（參數、進操作、結果） | 開始時細看 |

第一項要 elise 給「全部是 0 的物件」的配置；它也是 `classList (first)` 的 store forwarding 衝突裡最大的那一塊的同一件事（`ElementData`），做法定了之後其他欄位多的型別可以跟著用。

### 量法

- **對 Chrome 一律用 Chrome for Testing**（blink_perf 和 Speedometer 用的那個），不用發行版的 Chromium。
- 每段做完：
  - `bench.py calls --baseline`：`createElement` 的指令、cycles、store forwarding 的衝突；其他成員不增加。
  - gdb 單步 callback（RFC 0097 的 `prototypes/step-callback.py`）：改到的那幾段的條數。
  - `bench.py blink-perf --pages` 四頁：`bindings/create-element`、`gc-forest`、`gc-mini-tree`、`gc-tree`，各跑三次（單次之間有一成的起伏）。
  - 頁面自己計時的三方比較（`prototypes/time-three-browsers.py`）。
- 指令數少了不代表時間少了：cycles 和衝突數一起看；衝突變多的在同一段查清楚。
- Speedometer：全部做完時量一次。

### 測試

- 行為：`dom_js` 和 `dom` 的測試、相關的 WPT 目錄（`dom/nodes`、`custom-elements`、`html/dom`）。
- 第 2 段的新測試：同樣內容但不是同一個物件的字串（串接出來的）、大寫的名稱、有 `-` 的名稱、不合法的名稱、GC 搬動字串之後、兩個 realm、表滿了之後，結果都和照文字查的一樣；名稱由身分認出來時不讀文字（計數）。
- 第 1 段：不檢查的 `wrap` 只在預先建的 wrapper 上用，debug build 仍然檢查。
- 第 3 段動到 elise：它自己的測試、模型、fuzz；merge 之後手動開始 `gc.yml`。
- 每段的 mutation test。

### 分階段

三段，照「設計概要」的表。順序見「待決定」的第 2 條。

## 缺點

- 第 2 段依賴 V8 字串的佈局（instance type 的一個 bit、hash 欄位的位置和格式）：V8 升級時要跟著檢查。debug build 對 V8 自己的 API 比對，升級時壞了看得到。
- 第 2 段比 Blink 做得多：Blink 對短字串照文字查 `AtomicString` 的表。多一張要維護的表。
- 第 1 段多一處不檢查的 `unsafe`。
- 第 3 段要動 elise 配置物件的介面。
- 估計可能不準：三段裡只有第 2 段的一部分量過。全部做到估計還差 Chrome 約 100 到 250 條。

## 理由和其他做法

- **名稱**：之前量過不做的是最近查過的名稱的小快取（每個名稱省約 40 條；會淘汰、Blink 沒有）和 external string（短的名稱要 V8 的 sandbox）。身分的表不淘汰、行為確定，原型省 157 條。
- **不做、留著**：V8 複製樣板物件（約 170，Blink 也付）、登記 wrapper 的指標表（Blink 也付）。

## 先例

- Blink 的 `V8DOMWrapper::CreateWrapper`、`DOMDataStore::SetWrapper`：wrapper 從每個 context 的樣板複製、接上物件，沒有再檢查一次它是不是 wrapper。
- Blink 的 `HTMLElementFactory::Create`：用 `AtomicString`（指標）查一張 hash 表拿到建構的函式；名稱本身是進來時轉成 `AtomicString` 的（短字串照文字查 `AtomicString` 的表）。
- Blink 的 `MakeGarbageCollected<HTMLDivElement>`：物件在 Oilpan 配置的位置上直接建構。

## 已決定的事項

2026-11-15（使用者：都照建議；選項在「當時的選項」）：

1. **三段都排進這份**，一段一段做，每段做完看數字再決定下一段。
2. **順序是第 1、2、3 段**：第 1 段最小、最確定，先走完一次量法；第 2 段有原型；第 3 段動 elise，放最後。
3. **第 2 段先做 `createElement`**，量完再決定屬性名稱要不要跟著做（在這份的第 2 段裡）。
4. **原型留在 `prototypes/`**。
5. **預先建 wrapper 的 scope 放進第 1 段**（使用者問能不能優化）：樣板物件已經在時不開 scope，用 `InfoHandles`。

## 當時的選項

1. **範圍：三段都做，還是先做一部分**
   - A（建議）：三段都排進這份，一段一段做，每段做完看數字再決定下一段。
   - B：只做第 1、2 段，第 3 段（動 elise）另外開。
2. **順序**
   - A（建議）：第 1 段、第 2 段、第 3 段。第 1 段最小、最確定，先走完一次量法；第 2 段有原型；第 3 段動 elise，放最後。
   - B：第 2 段先（收穫最大，而且對其他成員也有用）。
3. **第 2 段推到哪些成員**
   - A（建議）：先做 `createElement`，量完再決定屬性名稱要不要跟著做（寫進這份的第 2 段，不另開 RFC）。
   - B：一開始就連屬性名稱一起做。
4. **原型要不要留在 RFC 的資料夾**
   - A（建議）：留（`prototypes/`，和之前的 RFC 一樣）。
   - B：不留，只留數字。

## 待決定

每一段開始前列那一段的。

## 之後的可能

- `createTextNode`、`createComment`、`cloneNode` 的同一件事（第 1、3 段的改動它們大多直接受惠，沒有另外量）。
- 其他欄位多的型別用第 3 段的配置方式（`ElementData`）。
- 發行版的 Chromium 量出來的舊數字裡，會配置的項目要對 Chrome for Testing 重量。
