# RFC 0099 的進度：索引的 query 只比較索引和長度

開始日期：2026-11-14

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| query 和 enumerator 讀 `length`，V8 原始的 callback | 做完 | 2026-11-14 |
| 布林參數的 `undefined`、`null`、小整數不開 scope | 做完 | 2026-11-14 |

## 驗收

| 項目 | 怎麼驗證 | 結果 |
| --- | --- | --- |
| `0 in list` 不比 `list[0]` 貴 | `bench.py calls --baseline rfcs/0098-direct-v8-bindings/calls-baseline.json` | 935 → 442 條指令（`list[0]` 446），cycles 231 → 85，store forwarding 的衝突 4.90 → 0.57 |
| 其他成員不變慢 | 同上 | 指令差都在 2 條以內；`list[i] (first)` 2289 → 2300（+11，這一項含 GC，沒有查是不是雜訊） |
| query 不開 scope、不登記 root | `bench.py census speedometer` | `Indexed` 沒有成員的那一列（query）：50,127 次，scope 0、trampoline 的 scope 0、root 0、讀的操作 50,127、會進安全點的 0（之前每次一個 trampoline 的 scope、一個 root、一個會進安全點的操作） |
| `arc` 不開 scope | 同上 | `CanvasRenderingContext2D.arc` 30,298 次，scope 0（之前 30,298） |

Speedometer 一輪的總數：自己開 scope 的呼叫 8.8% → 7.7%，經過 trampoline 的 1.9% → 0.0%（207 次），登記 root 的 19.5% → 17.5%。

## 做過的檢查

2026-11-14，Linux（x86-64）。

- **新的測試**：`dom_js` 的 `indexed_query`（3 個）：20 種有索引的物件（`NodeList`、`HTMLCollection`、`HTMLAllCollection`、`DOMTokenList`、`NamedNodeMap`、`CSSStyleDeclaration`（inline 和 computed）、`StyleSheetList`、`CSSRuleList`、`MediaList`、`CSSKeyframesRule`、`HTMLFormElement` 和它的 `elements`、`HTMLSelectElement` 和它的 `options`、`selectedOptions`、`DOMRectList`、`Window`）`length` 以內每個索引是屬性、拿得到東西、descriptor 對，`length` 和之後的不是，`Object.keys` 剛好是那些索引，有 setter 的可寫：全部一致；屬性跟著 `length` 變；query 不開 scope、`Object.keys` 剛好一個。`info_front::a_boolean_argument_of_nothing_or_a_small_integer_needs_no_scope`；原本「`toggleAttribute` 的 `1` 和 `null` 會開 scope」的兩條預期改成不開。`dom_api_macros`：indexed getter 產生 `indexed_length`、沒有 `length` 是錯誤、`length` 失敗時是 0。
- **`arc` 的原因**：選用的布林參數沒給時是 `undefined`，走到 `ToBoolean` 的 scope（修之前五個參數的 `arc` 一個 scope，修之後 0；暫時拿掉修正確認過）。沒有動 fork：`undefined`、`null`、小整數在 `dom_js` 直接讀。
- **mutation test**（`mutants.toml`，15 個）：都抓到。第一次漏一個（`length` 失敗時回 `u32::MAX`）：補了 macro 的測試。
- **WPT**：`dom/collections/`、`dom/lists/`、`dom/nodes/`、`html/semantics/forms/the-select-element/`、`the-form-element/`、`css/cssom/`、`html/infrastructure/common-dom-interfaces/collections/`：773 個測試、20,809 個檢查，非預期的結果 0。
- **測試**：`cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、sanity 的七項通過。
- **測試守不住的**：query 不登記 root、操作是讀的（`docs/test-gaps.md` 的「沒有計數守著的優化」）。
- **留著沒動的**：`bindings` 的 `set_state_indexed_getter`、`set_state_indexed_properties` 和 `StateIndexedGetter`（經過 rusty_v8 的 trampoline 的那一組）`dom_js` 不再用，只剩 `bindings` 自己的測試在用。
