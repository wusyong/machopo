# RFC 0099：索引的 query 只比較索引和長度

- 開始日期：2026-11-14
- 狀態：🚧 已排程：2026-11-14
- 細節：無

## 摘要

有索引 getter 的介面（`NodeList`、`HTMLCollection`、`DOMTokenList` 等）被問「有沒有第 i 個屬性」和「有哪些索引」時，現在去呼叫整個 getter 看有沒有東西：每次開一個會進安全點的操作、登記一個 root，而且經過 rusty_v8 先開 scope 的 trampoline。改成只讀介面的 `length`、比較索引：一個讀的操作，不登記 root，callback 是 V8 原始的。這是 RFC 0098 路線圖的 M2。

## 動機

- `i in list`、`Array.prototype.forEach.call(list, …)`、`Array.prototype.slice.call(list)`、`Object.keys(list)` 這類寫法，V8 對每個索引問一次 query。Speedometer 一輪裡有 50,127 次（兩個 suite 各 25,051 次），是唯一大量經過 trampoline 的 callback。
- 它做了多餘的事：只要回答「有」，卻拿出了節點。`bench.py calls` 量到 `0 in list` 每次 935 條指令，`list[0]` 只要 447 條。
- 對網頁沒有看得到的變化，除了 getter 在長度以內回 null 的情況（「詳細設計」的「行為的差別」）。

## 設計概要

對網頁沒有 API 的變化。對改 binding 的人：有索引 getter 的介面一定要有 `length`（現在的 16 個都有），query 和 enumerator 不再呼叫 getter。

## 詳細設計

### 現在

`Indexed<M>` 的 query（`dom_js` 的 `templates.rs`）呼叫 `named::has_index::<M>`，它經過 `WholeFront` 跑 getter `M`，看結果是不是 null 或空字串。enumerator 從 0 開始一個一個呼叫 getter，直到拿不到東西。兩者都由 `bindings` 的 `indexed_query`、`indexed_enumerate` 接，是 rusty_v8 會先開 scope 的那一種。

### 改成

- **query**：讀 `length`，`index < length` 時回屬性的 attribute（有索引 setter 的可寫，其餘唯讀），否則不攔截（V8 接著看 prototype）。一個讀的操作（`Dom::read`），不開 scope、不登記 root。
- **enumerator**：讀一次 `length`，回 `0..length`。建陣列要 V8 配置，所以當場開一個 scope。
- **callback**：兩者都用 rusty_v8 已經有的 `query_raw`、`enumerator_raw` 接成 V8 原始的 callback，fork 不用改。
- **`length` 怎麼讀**：見「待決定」的第 2 條。

### 行為的差別

現在的規則是「getter 給 null 或空字串就是沒有」，改完是「索引小於 `length` 就是有」。兩者在 getter 於長度以內都給東西時相同。WebIDL 的定義是後者（supported property indices，DOM 的集合都定成 `0..length`）。Blink 是前者：它沒有索引的 query，`in` 經過 descriptor 的 callback，那裡呼叫 getter、getter 不攔截就不攔截（「先例」）；現在的 machopo 和它一樣。要逐一確認 16 個介面在長度以內不會給 null；有的話那是 getter 或 `length` 的 bug，照 WebIDL 修。

### 另一件：布林參數不開 scope

M0 的計數看到 `CanvasRenderingContext2D.arc` 每次呼叫都開一個 scope（Speedometer 上 30,298 次）。原因：它最後一個選用的布林參數沒給時是 `undefined`，而布林參數只有剛好是 `true`、`false` 時直接讀，其他的值都走到開 scope 的 `ToBoolean`。改成 `undefined`、`null`（都是 false）和小整數（不是 0 就是 true）也直接讀；字串、物件、不是小整數的數字照舊。沒有動 fork：rusty_v8 的 `boolean_value` 要一個 scope（其實只用它的 isolate），替它加一個只拿 isolate 的入口可以涵蓋全部的值，但這三種已經是實際會碰到的。影響所有吃布林參數的成員。

### 量法

- `bench.py calls --baseline rfcs/0098-direct-v8-bindings/calls-baseline.json`：`0 in list` 不比 `list[0]` 貴（現在 935 對 447），其他成員的指令、cycles、store forwarding 的衝突不增加。
- `bench.py census speedometer`：`Indexed` 那一列的 trampoline 的 scope 和 root 變成 0。

### 測試

- `dom_js` 的測試：每個有索引 getter 的介面，`i in object`、`Object.keys`、`hasOwnProperty`、屬性的 descriptor，在長度以內、等於長度、超過時的結果；有索引 setter 的可寫。
- 計數的測試：query 不開 scope（`info_scopes_for_testing`）、不登記 root。
- mutation test：比較反過來、差一、enumerator 少一個或多一個。
- 相關的 WPT 目錄（`dom/collections`、`dom/nodes`、`html/semantics/forms` 裡用到集合的）。

### 分階段

一段做完。

## 缺點

- 行為從「問 getter」變成「信任 `length`」：兩者不一致的介面會露出來。這是好事，但可能要順手修幾個 getter。
- 速度上的收穫在實際的頁面上量不出來（Speedometer 上佔 1.9% 的呼叫）。

## 理由和其他做法

見「待決定」。

## 先例

- Blink 的 bindings generator（`bind_gen/interface.py`，2026-11-14 讀的 `main`）對有索引 getter 的介面**不產生 query**，產生的是 descriptor 的 callback（`make_indexed_property_descriptor_callback`）：它呼叫 `IndexedPropertyGetterCallback`（會建 wrapper），不攔截就回 `kNo`，否則用回傳值組一個 `v8::PropertyDescriptor`。所以 Blink 的 `i in list` 比 `list[i]` 貴，這份做完之後 machopo 在這條路上做的事比 Blink 少。
- 同一個檔案的 enumerator（`make_indexed_property_enumerator_callback`）是 `length()` 加 `bindings::EnumerateIndexedProperties(isolate, length)`；deleter 用 `index < length()` 判斷。「讀 `length`、比較索引」在 Blink 用在這兩處。
- WebIDL 的 legacy platform object：supported property indices 由介面的說明決定，DOM 的集合都定成 `0` 到 `length - 1`。

## 已決定的事項

2026-11-14（使用者：都照建議）：

1. **query 用 `index < length` 判斷**。不選照樣呼叫 getter（仍是會進安全點的操作，front 要多一條路）；不選每個介面手寫一個 `has_index`：14 個介面（另外兩個繼承 `HTMLCollection`）在規格裡都是 `0..length`，手寫的內容就是這一句，卻多一個會和 `length` 不一致的地方。逐一對過：13 個的 getter 和 `length` 讀同一個序列，`HTMLSelectElement` 是兩種算法（getter 從選項的集合取、`length` 走樹數），規則相同。加一個測試守住每個介面「`length` 以內 getter 都給得出、`length` 那一格給不出」。
2. **`length` 由宣告表產生的入口讀**（回 `u32`，在 `Dom::read` 裡直接呼叫），不經過 front。
3. **enumerator 一起改**：讀一次 `length`、回 `0..length`（Blink 的 enumerator 也是）。
4. **布林參數不開 scope 放在這份**：`arc` 每次開 scope 的原因是布林參數的值不是 `true`、`false` 時（數字、`undefined`）走到開 scope 的路；轉成布林值不跑 script、不配置、不丟例外，只要 isolate。影響所有吃布林參數的成員。

## 待決定

無。

## 之後的可能

- named 的 interceptor 的 query 也是呼叫 getter：它沒有 `length` 可以比，要看名字在不在，另外評估。
