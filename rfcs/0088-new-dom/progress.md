# RFC 0088 的進度：新的 DOM：照 kun 自己的 GC 的規則重新設計

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-26，macOS 26.6.2（Apple M4）。在 `0088` 上做。

完成日期：2026-10-27，macOS 26.6.2（Apple M4：4 個 P-core、6 個 E-core，接電源）。程式只在 `0088` 分支上（已 push 到 `origin`），不 merge 回 `main`（決定 17）；最後一次編譯和測試是 1g 第 5 步，收尾只改文件。

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。實驗的結論整理在 `details.md` 的「實驗的結論」，這裡是驗收、量到的數字和做過的檢查。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 新的 DOM 的最小切片（`dom_next`）和它的測試；wasm 文件的並排入口（`kun:next-dom`），六頁的 C 版本和基準 | 完成 | 2026-10-27 |
| 1b JS 文件的並排入口（`kunNextDom`）：wrapper、stub、同一個節點同一個 wrapper；六頁的 JS 版本和基準 | 完成（1c 換掉） | 2026-10-27 |
| 1c JS 文件真正的接法，交換：V8 每一步也推進新的 heap 的 marking，兩邊都做完才收尾（在本機改 V8）；節點指向 wrapper 是交回 V8 的邊 | 完成（1f 改寫） | 2026-10-27 |
| 1d 排程：新的 heap 自己的排程決定 V8 每一步裡標多少，平行的工作在 V8 的 worker 上 | 完成 | 2026-10-27 |
| 1e 兩邊和基準一起量；過渡策略的建議（`details.md`） | 完成 | 2026-10-27 |
| 1f JS 文件的 heap 照 cppgc 的模型和 V8 接；先量 Chrome 的 GC | 完成 | 2026-10-27 |
| 1g 每次呼叫 DOM 的成本和 Chrome 比，照 Speedometer 的呼叫分布：呼叫的計數、18 個成員三邊量、第 3 到 5 步的實驗；註冊照 Chrome 的方式拆掉 | 完成 | 2026-10-27 |

## 驗收

指令都在 `0088` 分支上跑：測試的 crate 和檔案（`dom_next`、`next_dom`、`gc` 的 `js::` 等）和量測的頁，`main` 都沒有。

**新的 DOM 的切片**

| 項目 | 怎麼驗證 |
| --- | --- |
| 節點、名稱的檢查（DOM 的規則）、樹的操作和 pre-insert 的錯誤、attribute、`innerHTML`；`nodeType`、`ownerDocument`、namespace、`className`、`textContent`；名稱的鍵各不相同、API 和 parser 共用名稱；經過 GC 留下 root 撐住的、收掉其他的；留著的樹的 site 被 pretenure；GC 的階段和 worker 上的 job 進 trace | `cargo test -p dom_next` |
| wasm 文件的 `kun:next-dom`：guest 在新的 DOM 上建樹、走訪、拿到錯誤，handle 放掉之後 slot 表是空的；沒開時 import 它的 guest 不執行 | `cargo test -p dom_wasm --test it new_dom` |

**JS 文件和 V8 的 GC**

| 項目 | 怎麼驗證 |
| --- | --- |
| 新的 heap 和 V8 一起標記的協定：兩邊到得了的都標過、沒有到得了的被釋放；拿掉每一部分各有反例 | `python3 rfcs/0088-new-dom/tla/check.py`（要 Java） |
| `gc` 的 JS 的接法：帶 JS 的邊的物件只在 driven 的 heap 在 old；交過來的和它到得了的留下；marking 期間建的留下；讀回來的標上；JS 的邊交給 engine；driven 的 heap 不自己開始 marking；別的 heap 的物件交過來不理；marking 結束和 heap 丟掉之後交過來的什麼都不碰 | `cargo test -p gc --test it js::` |
| `gc` 的排程：落後多少補多少、沒進展時 64 KiB 的量；沒有 worker 時 heap 的 thread 在 engine 的步驟裡標完、收尾的停頓不標；一步到時間就停；worker 標的量在 run 中途就算進去；不配置的操作把 GC 留到下一次 | `cargo test -p gc --lib schedule`、`cargo test -p gc --test it js:: young::` |
| 打了 patch 的 V8（`target/v8-0088`） | `python3 rfcs/0088-new-dom/v8/build.py`（套用、編、還原） |
| 新的 heap 的工作在 V8 的 worker 上：呼叫的 thread 跑 joining run、要幾個 worker 就用幾個、`run` 等 worker 返回、posted 的 job 到 cancel 或 drop 為止、panic 傳回 `run` 或 `cancel` | `cargo test -p bindings --test it gc_platform` |
| per-isolate 狀態：只讀得到自己的型別、只設一次、和 isolate 一起放掉 | `cargo test -p bindings --test it isolate_state` |
| JS 文件的 `kunNextDom`：沒開時沒有；建樹、走訪、同一個節點同一個 wrapper、`DOMException`、receiver 的檢查；Speedometer 最常呼叫的成員；每次拿到自己的字串（比值的快取多的值也是）；每個 realm 的 wrapper 是自己的；同一個 thread 的下一個 isolate 有自己的新 DOM | `cargo test -p dom_js --test it next_dom` |
| V8 的 GC 標新的 heap：wrapper 撐住節點、節點撐住 wrapper 和 expando、都到不了時一起收；沒改過的 wrapper 被 V8 的 scavenge 丟掉、節點下次拿到新的；V8 只用一個 thread 和 incremental 的壓力下也一樣；沒有 worker 時新的 heap 在 V8 的步驟裡標完 | `cargo test -p dom_js --test it next_dom`、`cargo test -p dom_js --test next_dom_gc`；`MACHOPO_V8_FLAGS` 加 `--stress-incremental-marking`、`--stress-compaction`、`--stress-marking=100`、`--single-threaded-gc`、`--stress-scavenge=10` |
| 註冊拆掉時 V8 的 GC 先收尾：負載下不 segfault | `mutants-1g.toml` 的「the registration goes while V8 marks」的 shell 迴圈（4 個 process × 120 次，`--stress-compaction`） |

**量測的工具**

| 項目 | 怎麼驗證 |
| --- | --- |
| `bench.py blink-perf --next-dom`：挑頁、三種跑法輪流、`--versions`、只有 JS 版的頁、`MACHOPO_NEXT_DOM=1`、Chrome 也錄 trace、報告；`--gc` 不把兩種版本當成頁 | `python3 scripts/bench/blink_perf/blink_perf_test.py` |
| `gc_bench` 的視窗算出 engine 的 GC 裡 `gc` 的部分 | `python3 scripts/bench/gc/gc_bench_test.py` |
| DOM 成員的呼叫計數：typed 和共用的 callback 都數到、照次數排序；kun 結束時 renderer 寫出檔案；`bench.py speedometer --dom-calls` | `cargo test -p dom_js --test it bindings::dom_calls_are_counted_by_member`、`cargo test -p shell --test it dom_calls::`、`python3 scripts/bench/speedometer/speedometer_test.py` |

**量到的**：下面「做過的檢查」的「量測」，結論在 `details.md`。

## 做過的檢查

都在 macOS 26.6.2（Apple M4）上，2026-10-26 到 27。量測都是 `perf` profile、接電源、每種 3 次取中位數，Chrome for Testing 152.0.7977.82；兩種 DOM 都在不在頁面上的 document 裡、不 render。

### 測試

- **每一段**：跑改到的 crate 的測試（`gc`、`dom_next`、`dom_wasm`、`dom_js`、`bindings`、`shell` 的相關部分）和 `sanity.py deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
- **新的 DOM 的測試在壓力下**：`next_dom` 在 V8 的 `--stress-incremental-marking`、`--stress-marking=100`、`--stress-compaction`、`--single-threaded-gc`、`--stress-scavenge=10` 下都跑過。依賴 GC 的測試在 10 個 process 同時跑的負載下各跑 100 到 400 次。
- **第 1 段的階段檢查**（1e、1f、1g 各一次）：`cargo test --workspace --no-fail-fast`、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`。
  - 1e：4261 個通過、3 個 ignored。兩個失敗和這份無關：`shell` 的 `navigation_timing::the_entry_of_a_redirected_load`（下面的「不穩定的測試」），和 `browser` 的 `navigation::links::idle_callbacks`（只在整套平行跑的負載下失敗一次，`requestIdleCallback` 的 timeout 和 `performance.now()` 對不上，沒有再追）。
  - 1g：58 個 test binary 全部通過。
- **1g 第 5 步之後**：`cargo test -p bindings -p dom_js`、clippy 和 fmt 都過。

### 模型

`tla/check.py`，1f 的協定（wrapper 直接存物件、節點帶 V8 的參照、V8 的 scavenge 丟掉沒改過的 wrapper 並 reset 那一格）：

- **窮舉**：2 個物件 × 2 個 JS 物件 × 每個物件 1 個欄位，成立（749 萬個狀態，42 秒）；每個物件 2 個欄位也成立（8857 萬個狀態，17 分鐘）。
- **14 個 mutation**（15 次檢查）都找到預期的反例：SATB 的 barrier、黑的配置、黑的升級、讓年輕的物件被 wrap（兩種不變式各一次）、wrapper 的交接、JS 的邊的交接、讀取的 barrier、V8 的 Dijkstra、V8 收尾時重掃 root、只看一邊做完就收尾（兩種）、V8 丟掉時不 reset、丟掉改過的、丟掉到得了的。
- **1c 的協定**（wrapper root 的表）：也窮舉過，那時讀取的 barrier 的反例就是 RFC 0087 原型發現的情境：script 從另一個 JS 物件拿出 wrapper、unwrap、存成 root、丟掉 wrapper，V8 永遠掃不到它。
- **跑不完的大小**：3 個物件時一個多小時沒跑完，停掉。

### Mutation test

`mutants-1a.toml` 到 `mutants-1g.toml`，189 條，都跑完確認檔案還原了。

| 段 | 條數 | 沒抓到、補了測試的 | 等價的（`docs/test-gaps.md`） |
| --- | ---: | --- | --- |
| 1a | 49 | 同名的 attribute 設第二次時多一個（補 `attributes.length`）；parser 的文字接到後面的節點（補 foster parenting 的兩段文字）；platform 不包 job 的兩條（補單元測試） | parser 的 `add_attrs_if_missing` 不看在不在 |
| 1b | 24 | `MACHOPO_NEXT_DOM` 沒設也開（判斷拆成 `enabled_by` 測） | 拿掉死掉的 wrapper 時不比對（1c 拿掉了這張表） |
| 1c | 18 | — | — |
| 1d | 19 | 一步標完不處理 ephemeron（補測試）；V8 給的時間沒傳下去（換算抽到 `bindings` 測） | — |
| 1e | 6 | — | — |
| 1f | 23 | marking 收尾時不 retire（改成交一個這次收掉的物件） | `retire` 不等進行中的交接（只是一個交錯，只靠審查） |
| 1g | 50 | 狀態的型別檢查、值的快取比對 `Arc`、狀態在 isolate 之前放掉（各補測試）；`names::CLASS` 換成 0 是等價的，改寫成 `className` 讀別的 attribute | 狀態的 thread 快取不比對 isolate（後來拿掉了這個快取） |

- 卡住到逾時被砍掉、也算抓到的：1a 一條（detach 不改下一個兄弟的 back link，走樹繞圈），1f 兩條（driven 的 heap 裡帶 JS 的邊的物件放進 nursery、別的 heap 的物件交過來照樣標）。
- 註冊拆掉的那條（1g）只在負載下抓得到，用 shell 迴圈跑。
- V8 的 patch 沒有逐條做 mutation（每條要重編 V8，`docs/test-gaps.md`）；rusty_v8 的 fork 的 8 條（欄位的編號、static roots、Smi 的編碼、只寫低 32 bits）都抓到。

### 量測

**六頁**（1e，kun `88b4e807`；倍數是新的 DOM 比今天的好幾倍，`/Chrome` 是 kun 比 Chrome 好幾倍）：

| 頁 | 單位 | wasm 今天 | wasm 新 | 倍數 | JS 今天 | JS 新 | 倍數 | Chrome | JS 今天 /Chrome | JS 新 /Chrome |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `bindings/create-element` | runs/s | 513 | 1129 | 2.20 | 579 | 1249 | 2.16 | 4131 | 0.14 | 0.30 |
| `bindings/gc-forest` | ms | 371 | 177 | 2.10 | 269 | 154 | 1.75 | 47.4 | 0.18 | 0.31 |
| `bindings/gc-mini-tree` | ms | 694 | 254 | 2.73 | 335 | 177 | 1.90 | 72.9 | 0.22 | 0.41 |
| `bindings/gc-tree` | ms | 992 | 391 | 2.54 | 391 | 197 | 1.98 | 94.6 | 0.24 | 0.48 |
| `bindings/insert-before` | runs/s | 57.3 | 112 | 1.95 | 133 | 445 | 3.35 | 308 | 0.43 | 1.45 |
| `parser/innerHTML-setter` | runs/s | 2554 | 3053 | 1.20 | 2611 | 3096 | 1.19 | 12883 | 0.20 | 0.24 |

- **wasm**：wasm 版從 1a 起六頁都比較快。很快就死的節點在新的 GC 的 nursery 裡收掉，cppgc 那邊是 incremental marking 的上萬個小步和 sweep。
- **JS，1b**：建 wrapper 的四頁變慢（0.58–0.77 倍）。取樣裡以 id 為 key 的 wrapper 表 27.6%、weak handle 23.2%。
- **JS，1e**：1c、1d 之後六頁都比較快。最長的停頓都是 V8 的 major GC，新的 DOM 47–49 ms、今天的 71–460 ms。
- **1e 的 GC 和取樣**：GC 佔的比例反而高（26–32%），大頭是新的 heap 的 scavenge 一再複製有 wrapper 的節點（gc-forest 104 次 553 ms），和 V8 停頓裡走 wrapper root 和 JS 的邊兩張表（`Gc.Weak`，每次約 11 ms）。完整的數字在 `details.md` 和 `bench-1-apple-m4.json`。

**GC 和 Chrome 比**（1f；1f 開始時 kun `2aedc826`，甲是做好的樣子，乙是量的時候暫時讓 JS 文件的 heap 沒有 nursery；main thread 上 runner 量的 19 次 iteration 的合計，最長的停頓是 3 次裡最長的）：

| 頁 | 成績：1f 開始 → 甲 / 乙 / Chrome | GC ms：1f 開始 → 甲 / 乙 / Chrome | young：甲 / 乙 / Chrome | atomic：甲 / 乙 / Chrome | 最長的停頓 ms：甲 / 乙 / Chrome |
| --- | --- | --- | --- | --- | --- |
| `gc-forest` | 154 → 123 / 125 / 46.9 ms | 1024 → 207 / 204 / 243 | 184 / 182 / 67 | 24 / 21 / 43 | 21 / 14 / 33 |
| `gc-mini-tree` | 175 → 144 / 144 / 70.8 ms | 1028 → 205 / 204 / 225 | 179 / 184 / 62 | 25 / 19 / 46 | 21 / 13 / 14 |
| `gc-tree` | 197 → 160 / 160 / 93.0 ms | 1041 → 215 / 208 / 218 | 177 / 176 / 79 | 28 / 23 / 29 | 21 / 16 / 29 |
| `create-element` | 1246 → 1646 / 1625 / 4103 runs/s | 1273 / 1304 / 2983 | 1131 / 1172 / 60 | 138 / 139 / 1157 | 58 / 67 / 6.2 |
| `insert-before` | 450 → 404 / 401 / 308 runs/s | 3 / 3 / 4 | | | |
| `innerHTML-setter` | 3135 → 3014 / 3054 / 12920 runs/s | 160 / 10 / 2739 | 77 / 7 / 2 | 1 / 1 / 136 | 20 / 0.4 / 80 |

- **1f 開始時**：建 wrapper 的四頁每次 iteration 的 GC 是 Chrome 的 5 到 5.6 倍，差在 young 和 V8 的停頓。innerHTML-setter 反過來是 Chrome 的四分之一。
- **1f 之後**：三個 gc 頁的 GC 和 Chrome 差不多，最長的停頓從 47 降到 21 ms。
- **甲和乙幾乎一樣**：所以剩下的 young（Chrome 的約 2.7 倍）是 V8 的 scavenge，不做會讓年輕節點也能被 wrap 的丙。
- **成績**：還差 Chrome 1.7–2.6 倍，差距在 GC 以外。
- **錄 trace 的成本**：Chrome 只錄 `v8`、`cppgc` 和 User Timing。錄預設的那一組 category 時，innerHTML-setter 從 12848 慢到 4378 runs/s。

**每次呼叫**（1g）：

- **Speedometer 的呼叫分布**（kun `24e344f1`，`--dom-calls`，407 個成員被呼叫過）：
  - 最多的是 `Node.nodeType` 8.4%（jQuery 一個 suite 就 130 萬次）、`Element.setAttribute` 7.8%、`Node.parentNode` 4.0%。
  - canvas 的成員 8.8%。
  - 量的 17 個成員佔 45%，形狀相同、切片沒有的再 18%，剩下 28% 是長尾。
  - 第一次跑時最多的是工具自己的摘要走整個 document 的呼叫，`--dom-calls` 改成給頁面一個什麼都不讀的摘要。
- **18 頁三邊量**：每一步之後只量受影響的頁、只跑新的 DOM。最後的數字和每一步買到的效果在 `details.md` 的「每次呼叫：差距在 V8 和 Rust 的交界」。照 Speedometer 加權：今天的 DOM 24.5 ms、新的 DOM 從 12.7 到 7.4 ms、Chrome 6.6 ms；對 Chrome 的幾何平均從 2.1 到 1.21 倍。
- **加權的極端**：比 Chrome 快的是 Charts-observable-plot（0.82：`createElementNS`、`setAttribute` 多），最慢的是 jQuery（1.86）和 TodoMVC-ES6-Webpack（1.72）。
- **取樣**（samply）：
  - 第 2 步的 `nodeType`（17 ns）：檢查和讀回 receiver 約 45%（各兩次），V8 的呼叫和 rusty_v8 的 FFI 約 26%，每次查 isolate 的 slot 約 13%，進 GC 的安全點約 11%，DOM 本身幾乎 0。
  - 第 4 步之後：省下的固定成本移到呼叫的其他地方。反組譯第 5 步前的 callback，`state()` 沒有 inline、每次設約 3 KB 的 stack frame。
- **第 5 步的地板**：暫時加的頁量完就拿掉。
  - 空的 getter 經過 rusty_v8 是 2361 runs/s，直接接 V8 的 `extern "C"` callback 是 2350 runs/s，都是 4.2 ns。
  - 只跑迴圈、讀 JS 自己的 getter，kun 和 Chrome 都約 40000 runs/s（0.25 ns）。
  - 不錄 trace 也一樣。

### 找到、修好的錯

- **1a**：
  - 名稱的檢查原本只擋空白和幾個字元，`1x` 被當成合法的元素名稱，改成 DOM 的規則。
  - 新的 GC 在 worker 上的工作不進 trace，platform 把 job 包成 `Gc.Worker`。
- **1b**：`kunNextDom` 的錯誤原本用 global 的 `DOMException` 建，頁面換掉它時就變了。
- **1c**：
  - 新的 heap 的 marking 原本在 `CppHeap::StartMarking` 開始，V8 的主 thread 的 marking state 那時已經建好、沒有新的 heap 的 local，改成在 `InitializeMarking`。
  - `--single-threaded-gc` 時 V8 的收尾一直不結束：V8 主 thread 交過來的留在沒發布的 local，改成每次推進前先發布。
- **1d**：
  - V8 的 `Join` 只在 job 還要 worker 時才讓呼叫的 thread 跑，`gc` 的 seed 沒人做，改成呼叫的 thread 自己跑 joining run 再 `Cancel`。
  - 這版 V8 的 `TimeDelta` 沒有 `IsNegative`。
- **1f**：
  - `Node` 帶 `JsEdge` 之後 wasm 文件的節點也被配置到 old，改成只在 driven 的 heap。
  - `--stress-incremental-marking` 下 abort：heap 丟掉之後 V8 又交物件過來、設了已釋放的頁的 mark bit，加上 `Marker::retire`。
- **1g**：
  - patch 的 `kun::Unwrap` 對 Proxy 在 debug 的 V8 碰到 `Cast` 的 `DCHECK`，改成 `Cast<HeapObject>`。試過 `Cast<HeapObject>(obj)->map()` 時，V8 的 `IsJSApiWrapperObject` 對 `Tagged<Map>` 的多載判斷的是那個 map 物件，`Unwrap` 全部回 null，測試在 GC 之後抓到。
  - 拆掉時的 segfault：下面的「不穩定的測試」的第一條，修法是決定 15。
- **雜項**：`ci.py guests` 重新編了和這份無關的 `.wasm`，還原成 repo 裡的（1a）。

### 不穩定的測試

- **`dom_js` 的 `next_dom` 拆掉時 segfault**（1g，修好）：在 `--stress-compaction` 加上 4 個 process 同時跑的負載下約 1%。
  - 原因：V8 的 marking worker 在 `new_local` 裡鎖 `Shared`，而 `Shared` 已經放掉。1c 起的拆法讓 glue 的物件 detach、不刪，但 `new_local` 碰 `Shared`，patch 在 worker 上讀 glue 的指標也不是 atomic 的。
  - 試過：等進行中的呼叫做完（還剩 480 次 1 次）、標成要走（V8 收尾時 `CHECK` 失敗）。
  - 修法照 Chrome 拆 `CppHeap`（決定 15）。之後同樣的負載 1440 次 0 次。
- **`next_dom::a_wrapper_keeps_its_node_and_its_node_goes_with_it`**（1c 起，留著）：`kept = null` 之後 V8 那一側一直撐著樹上的一個 wrapper。
  - 原因：V8 的背景最佳化編譯的工作 handle 撐著迴圈裡的一個 wrapper，直到主 thread 安裝編好的程式（關掉背景編譯時 300 次 0 失敗）。
  - 1c 讓測試每一輪 GC 前跑一行 script、最多 10 輪。之後在 `--stress-compaction` 加上負載下還有約 1.8%（1440 次 26 次）。和新的 DOM 的正確性無關，記在 `docs/test-gaps.md`。
- **`gc` 的 `concurrent::markings_run_alongside_on_a_pool`**（1d，修好）：CPU 忙時約 2/10 失敗，200 輪只跑約 10 ms、worker 排不上時一次 marking 都沒做完。測試改成等一次 marking 做完。
- **`shell` 的 `navigation_timing::the_entry_of_a_redirected_load`**（和這份無關，修好）：`redirectStart` 從 time origin 算、粗化到 100 µs 的倍數，第一個請求在導覽開始後 100 µs 內發出時就是 0。測試改讀 `performance.timing.redirectStart`。
