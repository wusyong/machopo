# RFC 0017 的進度：HTML 元素和 Window 的雜項（H1）

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-08。完成：2026-10-08，macOS Apple Silicon（Rust 1.99.0）。五個階段都做完；第 1、2 階段做完時 rebase 到 RFC 0020（wptserve）之後一起 merge 回 `main`，第 3、4、5 階段在 `0017` 上，審核之後 merge。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `navigator`：UA 的來源、`Navigator`、mixin 的成員、`languages` | 完成 | 2026-10-08 |
| 2a `[Replaceable]`、`Performance`、時間原點、拿掉 `performanceNow`（`kun:dom` 0.8.0） | 完成 | 2026-10-08 |
| 2b `PerformanceEntry`、`mark`、`measure`、`getEntries*`、`clear*` | 完成 | 2026-10-08 |
| 3 `dataset` | 完成 | 2026-10-08 |
| 4 `document.title` 和分頁的標題 | 完成 | 2026-10-08 |
| 5 收尾：文件的核對、`matrix` 和 `fuzz`、RFC 改寫成完成時的樣子 | 完成 | 2026-10-08 |

## 驗收

每個階段做完時都在 macOS 上跑過 fmt（連同 `sdk/lua/generator`）、clippy（沒有警告）、`cargo test --workspace`、`sanity.py deps`、`boundary`、`unsafe`、`docs`、`test.py reftest`、`wpt`、`scenarios`；第 4 階段動到視窗那一層，另跑 `test.py automation`（真的視窗）；RFC 完成時用 debug 的 V8 跑 `test.py matrix` 和 `fuzz`。

**宣告表和前端**

| 項目 | 怎麼驗證 |
| --- | --- |
| JS：`Navigator` 的實例、不能建構、`languages` 凍結且每次同一個、沒有偵測用的成員 | `cargo test -p dom_js --test navigator` |
| `window.performance` 每次同一個、是 `EventTarget`、在它的 window 的 realm；`[Replaceable]` 的 attribute 可以被蓋掉 | `cargo test -p dom --test performance`、`cargo test -p dom_js --test bindings performance_now` |
| JS：union 的字典成員（第 13 條）、`detail` 是 structured clone 且每次同一個物件（第 14 條）、例外的種類 | `cargo test -p dom_js --test bindings user_timing` |
| JS：masking（`[LegacyOverrideBuiltIns]`）、setter、definer、deleter、enumerator、陣列的 index 也是名稱；`style` 的 definer | `cargo test -p dom_js --test bindings dataset` |
| WIT：`kun:dom` 升到 0.8.0（拿掉 `window.performance-now`）；只透過 union 的成員或 record 的欄位用到的 `record`、`variant` 也定義；匿名的特殊操作是 `get`、`set`、`delete`、`names`（第 19 條）；`InvalidAccessError` 是代碼 17 | `cargo test -p dom_api --test wit`（golden file、代碼、Lua 的 runtime 照這份 WIT 編） |
| wasm：`navigator` 的 resource、`list<string>`、`u64` 的回傳 | `cargo test -p dom_wasm --test engine navigator` |
| wasm：options 的 record、`string-or-performance-measure-options` 的兩個 case、錯誤代碼 17、往下轉型；Lua：同樣的形式 | `cargo test -p dom_wasm --test engine user_timing`、`python3 scripts/test.py reftest wasm/w9` |
| wasm：`dom-string-map` 的 `get`、`set`、`delete`、`names`，`css-style-properties` 的 `get`、`set`；Lua 的方法 | `cargo test -p dom_wasm --test engine dataset`、`python3 scripts/test.py reftest wasm/w9` |

**`dom` 和 GC**

| 項目 | 怎麼驗證 |
| --- | --- |
| `window.navigator` 每次同一個，window 和 navigator 互相撐住 | `cargo test -p dom --test navigator`、`cargo test -p dom --test script_objects navigator` |
| buffer 撐住 entry、清掉的不再讀得到；`PerformanceEntry` 的 slot 和 wrapper | `cargo test -p dom --test performance`、`cargo test -p dom --test script_objects performance_entry` |
| `dataset` 每次同一個、元素撐住它；slot 和 wrapper | `cargo test -p dom --test tree datasets`、`cargo test -p dom --test script_objects dataset` |
| `dom` 記下標題可能變了的 document：插入、移除含 `title` 的子樹，`title` 的子節點和文字；其他改動、沒有 browsing context 的不記 | `cargo test -p dom --test tree title_changes` |
| 新的 GC 型別（`Navigator`、`PerformanceEntry`、`DatasetDomStringMap`）和 `EventTargetImpl` 的 `Performance`：在泛型的 GC 測試、工作負載、`gc_stress` 裡；各種 marking 模式和 GC 的 flag | `cargo test -p dom`；`python3 scripts/test.py matrix`、`fuzz`（debug 的 V8） |

**`dom_api` 的語意**

| 項目 | 怎麼驗證 |
| --- | --- |
| `Navigator` 的成員照 Chrome 的值；UA 來自 navigator 自己的 window 的 frame | `cargo test -p dom_api --test navigator` |
| 每個 window 的時間軸：`now()`、`timeOrigin`；事件的 `timeStamp` 照它的 relevant global | `cargo test -p dom_api --test performance` |
| `mark`、`measure`（User Timing Level 3 的每一種給法和錯誤，第 16 條）、buffer 的順序和篩選、`clear*`、`new PerformanceMark`、`detail` | `cargo test -p dom_api --test performance` |
| 名稱的換算、只看 null namespace、跳過大寫字母、setter 的 `SyntaxError` 和 `InvalidCharacterError`、deleter | `cargo test -p dom_api --test dataset`、`cargo test -p dom_api --lib dataset` |
| `document.title` 的 getter 和 setter（HTML、SVG 的 document element、沒有 `<head>`、別的 namespace） | `cargo test -p dom_api --test title` |

**renderer、browser 和 net**

| 項目 | 怎麼驗證 |
| --- | --- |
| UA 只有一個來源：HTTP 的 `User-Agent` 和 `navigator.userAgent` 一樣、是 Chrome 的；子 frame 有自己 realm 的 `navigator`（初始的 `about:blank` 也有 UA） | `cargo test -p browser --lib user_agent`、`cargo test -p net --test load`、`cargo test -p shell --test navigator` |
| 時間原點是導覽開始的時刻（主 frame 由 browser 帶來、子 frame 自己記、初始的 `about:blank` 取建立的時刻）；rAF 的時間在同一條時間軸上 | `cargo test -p renderer --lib clock`、`cargo test -p shell --test performance`、`python3 scripts/test.py reftest j5` |
| 整個瀏覽器：mark 在 window 的時間軸上、只在自己的 window 的 buffer | `cargo test -p shell --test performance` |
| 分頁的標題跟著 `document.title`：每個 task 之後送、沒變不送、子 frame 的不送、太長的截斷 | `cargo test -p browser --test navigation tab_title` |

**WPT**

| 項目 | 怎麼驗證 |
| --- | --- |
| WPT：`the-navigator-object/` 不用 HTTPS、不用人操作的 9 個檔案（4 個全過，其他照 baseline） | `python3 scripts/test.py wpt the-navigator-object` |
| WPT：`hr-time/` 的 9 個檔案（2 個全過，其他照 baseline） | `python3 scripts/test.py wpt hr-time` |
| WPT：`user-timing/` 的 36 個檔案（25 個全過，其他照 baseline） | `python3 scripts/test.py wpt user-timing` |
| WPT：`dataset*` 的 7 個檔案（6 個全過）和 `domstringmap-supported-property-names` | `python3 scripts/test.py wpt global-attributes/dataset domstringmap` |
| WPT：`document.title-*` 的 8 個檔案全過 | `python3 scripts/test.py wpt dom-tree-accessors/document.title` |

**視窗**

| 項目 | 怎麼驗證 |
| --- | --- |
| 視窗：`about:demo` 的 `performance.now()` 按鈕記下每次點擊、從第一次量起 | `python3 scripts/test.py scenarios demo-script` |
| 視窗：`about:demo` 的按鈕用 `dataset` 記點擊次數 | `python3 scripts/test.py scenarios demo-script` |
| 情境的 `wait-title`；`about:demo` 的按鈕把點擊次數放進分頁的標題 | `cargo test -p shell --lib wait_title`、`python3 scripts/test.py scenarios demo-script`、`python3 scripts/test.py automation` |

## 做過的檢查

2026-10-08，macOS Apple Silicon。每個階段的設計和決定寫進了 `README.md` 的「已決定的事項」（第 6 到 21 條）；這裡是濃縮的紀錄。

- **第 1 階段（`navigator`）**：UA 由 browser 算（Chrome 的 reduced UA，版本是連結的 V8 的 milestone），`net` 送它、`RendererFlags` 帶給 renderer、`FrameClient` 交給 `dom_api`；`Navigator` 是自己的 GC 型別；宣告表的 `FrozenArray<T>`（JS 凍結、快取在 wrapper 上）。WPT 的 `the-navigator-object/` 9 個檔案，4 個全過。mutation test 18 條：三條第一輪沒抓到，兩條補了測試（從這個 window 的 getter 讀子 frame 的 `navigator`、剛插入的 iframe 的初始 `about:blank`），`FrozenArray` 的值變了換新陣列記進 `docs/test-gaps.md` 的「只靠審查」（唯一的值是固定的）。第一次改 shell 的測試時，`Window` 的 attribute 在 global 自己身上（`[Global]`），那一輪的「抓到」不算，修好之後重跑。
- **2a（`Performance` 和時間原點）**：browser 記下導覽開始的時刻，`CommitNavigation` 帶「經過了多久」，renderer 的 `Clock` 往回推；子 frame 自己記、初始的 `about:blank` 取建立的時刻；rAF 的時間是 renderer 開始 main frame 的時刻；宣告表的 `[Replaceable]`；`Event.timeStamp` 照事件的 relevant global（一開始照讀的 realm，WPT 的 `Event-timestamp-cross-realm-getter` 退步才改）；reftest `j5-animation-frames` 原本依賴 headless 的合成時鐘，改成檢查時間往前走。WPT 的 `hr-time/` 9 個檔案，2 個全過。mutation test 18 條，4 條補了 shell 的測試之後抓到（剛插入的 iframe 的 `timeOrigin` 和 realm、子 frame 的 rAF）。
- **2b（User Timing）**：一個 `PerformanceEntry` GC 型別，buffer 是新的在前的鏈；`measure` 照 User Timing Level 3；宣告表的 union 可以有字典的成員；`detail` 用 `SerializedScriptValue`；新的例外名稱 `InvalidAccessError`；WIT 和 Lua 的產生器補上新的形式。WPT 的 `user-timing/` 36 個檔案，25 個全過。mutation test 51 條：「同名的 measure 也算」補了測試；「end mark 和 options 的 end 誰先」等價。
- **第 1、2 階段 rebase 到 `main`**：RFC 0020 把 `NetworkContext::start` 改成收 `NetworkContextParams`，UA 改成它的 `user_agent`（Chromium 的也有，空的不送）；`compile_event_handler`、`run_testharness` 的簽名跟著改；在 wptserve 下 `preload/modulepreload-multiple.html` 的網址固定，原本因為 port 跳過的不用跳過。
- **第 3 階段（`dataset`）**：`DatasetDomStringMap`；宣告表的 deleter 和 `[LegacyOverrideBuiltIns]`；JS 的 masking interceptor、definer（`style` 也受影響，照 WebIDL 和 Blink）、deleter；WPT 的 `dataset-binding` 找到陣列的 index 讀不到，加了轉給 named 的 indexed interceptor。WPT 的 `dataset*` 7 個檔案，6 個全過（SVG、MathML 沒有 `dataset`），`domstringmap-supported-property-names` 也全過。mutation test 20 條：「deleter 不看 `-` 加小寫字母的規則」補了測試。
- **第 4 階段（`document.title`）**：`dom` 照 Blink 記下標題可能變了的 document，renderer 在每個 task、timer 和 parser 的每一段之後重算主 frame 的、不同才送；browser 截斷到 4096 個字元；情境的 `wait-title`。`browser` 的新測試找到連續的 timer 只送最後一個標題（原本只在一批 task 之後檢查）；寫 mutation test 的清單時找到空的 `svg` 被當成 `title`。WPT 的 `document.title-*` 8 個檔案全過，另外五個檔案的 baseline 變好。mutation test 22 條：「沒變也送」補了測試，「任何 document 變了都重算」等價，「不在每個 task 之後檢查」記進 `docs/test-gaps.md` 的「只靠審查」。`test.py automation` 22 個情境全過，截圖的分頁列是 `kun demo (clicks: 2)`。
- **第 5 階段（收尾）**：用 debug 的 V8 跑 `test.py matrix` 和 `fuzz`。第一次在 rusty_v8 fork 的 `kun-v152.2.0-6` 上，8 種設定和 fuzz 的 baseline 全部停在同一個地方：`dom_js` 的 `history_and_its_state`（RFC 0019 的 `history.state`）序列化 `SharedArrayBuffer` 時，kun 的 delegate 照 Blink 丟了 `DataCloneError` 再回傳 `None`，fork 的 C++ 接著又呼叫 V8 預設的 `GetSharedArrayBufferId`，再丟一次例外，`Isolate::Throw` 的 debug check（`IsTerminationException`）失敗；release 的 V8 不檢查，所以平常的測試都過。fork 的 `kun-v152.2.0-7` 修了它（已經有例外時不再轉給預設的版本，記在 fork 的 `README.machopo.md`），kun 跟著換到這個 tag。換了之後 `matrix` 8 種設定全過（各 655 個測試、77 個測試 binary），`fuzz` 的 baseline 和 20 種隨機的設定全過（seed 0xd1f7a3a7）。文件照 RFC 0017 改過的東西核對了一次（`soundness.md` 的 `ObjectTag` 清單和新的 GC 型別、crate README 的測試表、`double` 的轉換和 union 的測試空缺、RFC 的摘要），RFC 改寫成完成時的樣子。
- **跑過的檢查**（第 4 階段做完時，第 5 階段只改文件）：`cargo test --workspace` 1437 個全部通過；`test.py reftest` 541 個通過、68 個照預期失敗；`test.py wpt` 1598 個相符、206 個跳過（中間 rebase 進了 RFC 0020 的 wptserve，和第 2 階段之前的數字不能直接比）；`test.py scenarios` 44 次；`test.py automation` 22 個情境。
- **每個階段的 mutation test**：跑的期間使用者沒有 commit，跑完確認檔案都還原了，再跑 `ci.py prune`。等價和只靠審查的在 `docs/testing.md`。
