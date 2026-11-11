# RFC 0097 的進度：binding 每次呼叫的成本往 Chrome 靠

開始日期：2026-11-11

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型和量測（不合併） | 進行中 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| V8 在操作裡配置的三條規則在模型上成立，各拿掉一條有反例 | `python3 rfcs/0097-binding-call-cost/tla/check.py` |

## 做過的檢查

- 2026-11-11（Linux，AMD Ryzen AI 9 HX 370）：`tla/check.py` 在 2×2×1 上七個版本都照預期（`details.md` 的「模型」）；`inop` 在 2×2×2 上成立。

**第 1 段，原型 1：getter 不建 scope、不建 `V8Front` 和 `Call`**（2026-11-11，同一台機器）：

- `nodeType` 現在的 callback 反組譯（`perf annotate`，熱的路約 100 條指令；`bench.py calls` 的 153 條裡其餘是 V8 的 builtin 和 JS 的迴圈）：進出函式和存 6 個 callee-saved 暫存器約 17；建 `CallbackScope`、`V8Front`、`Call` 的寫入約 26；借狀態和一個全域的檢查約 5；unwrap（指標表、比 tag、cage 的檢查）約 22；marking 的檢查 2；`nodeType` 本身（讀種類、jump table）約 9；寫回傳值約 8；`Call` 和 scope 結束時的檢查約 12。
- 原型：`bindings` 的 `RawGetter`、`raw_getter_template`（`FunctionTemplate::builder_raw`，callback 直接拿 `FunctionCallbackInfo`，isolate 從 frame 的那一格讀），`dom_js` 在 `Node` 上多裝一個 `nodeTypeRaw`（借狀態、`read`、`unwrap_receiver`、`set_int32`；unwrap 的檢查照舊）。`bench.py calls --cases nodeType,nodeTypeRaw`：

  | | 指令數 | cycle |
  | --- | ---: | ---: |
  | `nodeType` | 151 | 30 |
  | `nodeTypeRaw` | 114 | 23 |

  省 37 條指令、7 個 cycle：machopo 自己的部分（扣掉 V8 的地板約 11 cycle）從約 19 到約 12 cycle，Chrome 約 6。剩下的在 unwrap 的檢查和進出函式。
- 找到的 bug：rusty_v8 的 `FunctionCallbackArguments::get_isolate` 把 V8 的 isolate 指標轉成 `*mut Isolate` 來用，而這個版本的 `Isolate` 是包著指標的結構（`repr(transparent)` 的 `NonNull<RealIsolate>`）：一用就 segfault。原型自己讀 frame 的格子。fork 裡沒有人呼叫它；不對上游回報，做外殼那一項時在 fork 給一個正確的入口。
- `firstChildRaw` 沒做：回傳 wrapper 的 `ReturnValue::set_traced` 拿 scope 當憑證（只當型別用），原型不動 fork。
