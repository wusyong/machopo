# RFC 0085 的進度：GC、binding、JS、DOM 往 Chrome 靠

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-06，Linux 7.2、Fedora 44（AMD Ryzen AI 9 HX 370）。在 `0085` 上做。

完成日期：2026-11-07，同一台。GC 不再搬動的部分另寫 RFC（`README.md` 的「之後的可能」）。

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 量測工具：Chrome 的取樣、兩邊的分類、ms 的比較表進 `bench.py`（決定 5） | 完成 | 2026-11-06 |
| 2 DOM：找子 frame（2a）、`innerHTML` 的 fast path（2b）、屬性的成員和字串參數（2c）；查找和 collection 移到第 4 段（決定 8） | 完成 | 2026-11-07 |
| 3 JS：給 V8 的設定（決定 11） | 完成 | 2026-11-07 |
| 4 binding 的固定成本、建節點、`dispatchEvent`、`element.style`，追 blink-perf 的頁（決定 12 到 15；4a 到 4f） | 完成 | 2026-11-07 |
| 5 GC：配置（決定 6，在 4d 做，決定 15）、資料跟著擁有者配在同一代（5a，決定 17）、renderer 的 heap 不用 nursery（5b，決定 19）；原本是第 2 段，2026-11-07 移到最後（決定 7）；其餘另寫 RFC | 完成 | 2026-11-07 |

## 驗收

都在 Linux 7.2、Fedora 44（AMD Ryzen AI 9 HX 370）上驗。收尾時（2026-11-07）`cargo test --workspace` 59 個 binary、3935 個過、1 個 ignored（78 秒），`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 沒有問題。整套的 WPT、reftest、情境、`matrix`、`fuzz` 由每天的 CI 跑；這份動到 GC，merge 之後看 CI 的 `matrix`、`fuzz`。

**結果**（Chrome 是 Chrome for Testing 152；量法在 README 的「量法」）

| 項目 | 開始（2026-11-06） | 完成（2026-11-07） |
| --- | --- | --- |
| Speedometer 3.1 的比值（每個 suite 的比值的幾何平均） | 1.50（suite 合計 1067.0 ms） | 1.43（1005.0 ms；交錯量的三次 998.1 ms、1.43） |
| 取樣的四類（20 個 suite 合計，每個 iteration 的 ms） | JS 320、DOM 118、GC 58、bindings 35 | JS 308、DOM 109、GC 37、bindings 35 |
| blink-perf 這四類的 26 頁（22 頁有效頁的幾何平均，kun 慢幾倍） | 2.27 | 1.55（第 4 段之後；第 5 段沒有改變 GC 的四頁） |
| renderer 的 VmHWM（Speedometer 的每個 suite） | | 第 5 段的 B 對甲 −8.6% 到 +3.0% |

**量測工具**

| 項目 | 怎麼驗證 |
| --- | --- |
| `--sample --browser chrome` 在 `perf` 底下跑 Chromium：Blink 的 frame 有名字（debuginfo），V8 命名 JS（`--no-sandbox` 才寫得出 perf map），trace 只錄 `blink.user_timing`，renderer 的 main thread 照 trace 的 tid 挑、時間是 `CLOCK_MONOTONIC`，run 留下的 perf map 和 V8 的 log 都清掉；先不取樣地量一次時間（`chromium-runs.json`） | `speedometer_test.py` 的 `ChromeSampleTests`（假的 `frames` suite，沒有 Chromium 或 `perf` 時跳過）；`CategoryTests.test_a_threads_samples_by_its_tid`、`test_the_marked_threads_ids`；`mutants-1.toml` |
| 分類（`categories.py`）：kun 照 crate、Blink 照類別名稱，配置和容器算給呼叫它的，V8 的 frame 往外看，Skia 看是 canvas 還是 paint，style 在文字之前，取樣本身的成本不算，C++ 的名稱去掉回傳型別、template、參數和修飾 | `CategoryTests` 的分類和名稱的測試；`mutants-1.toml` |
| `categories.md`：比例乘上沒有取樣時的時間、只有一類追平時的幾何平均、每個 suite 差最多的類別、Chromium 比 Chrome for Testing 的倍數；兩邊的樣本和時間都在才寫 | `CategoryTests.test_the_comparison`、`test_the_report_waits_for_both_browsers`；`mutants-1.toml` |

**DOM、JS**

| 項目 | 怎麼驗證 |
| --- | --- |
| 找子 frame：一次 rendering update 只走一次 frame tree、照樹的順序；loop 問動畫時不走 | `dom_js` 的 `frames` 的 3 個（`frame_tree_walks_for_testing`）；`mutants-2a.toml` |
| parser 的 fast path 帶著屬性建元素，結果和一個一個設的一樣；input 不改型別；值和文字解碼一次 | `dom` 的 `attributes`、`html` 的 `fast_path`（`input_type_changes_for_testing`、`decodes_for_testing`）、`style` 的 `form_states`；`mutants-2b.toml` |
| id 設回同一個值不動 id 表；`getAttribute` 照旗標判斷轉小寫；ASCII 的字串參數只掃一次 | `dom` 的 `ids`（`id_table_changes_for_testing`）、`dom_js` 的 `dom_core`、`bindings` 的 `v8_text`（`ascii_scans_for_testing`）；`mutants-2c.toml` |
| Chrome 的 renderer 打開的 JIT 和 IC 的四項 flag | 取樣：Sparkplug 的 builtin 49.1 → 36.3 ms（Chrome 152 29.1）；沒有測試守著，記在 `docs/test-gaps.md` |

**binding 和 blink-perf 的頁**

| 項目 | 怎麼驗證 |
| --- | --- |
| `window`、`document` 是 cached accessor：迴圈裡讀不呼叫 getter，換了 document 之後是新的 | `dom_js` 的 `frames`（`window_document_gets_for_testing`）；`mutants-4a.toml` |
| `getElementsByTagName` 對同一個根和名字給同一個 collection，根不留住它 | `dom` 的 `collection`；`mutants-4b.toml` |
| `Call` 的 drop 不做事時跳過 | `calls`：每個成員少 12 到 14 條；沒有測試守著（只會洩漏），記在 `docs/test-gaps.md` |
| 型別的 size class 和 JS 邊在編譯時算好、`publish` 的一般寫入 | `gc` 的測試（debug build 每次配置比對）；`mutants-4d.toml` |
| shadow tree 的 retarget 一個 tree scope 算一次 | `dom_js` 的 `shadow_dom`（`retargets_for_testing`）；`mutants-4e.toml` |
| `element.style` 只在有 `url()` 時建 URL 的環境；數值照 `%g` | `css` 的 `values`、`dom_api` 的 `css_style`；`mutants-4f.toml` |

**GC**

| 項目 | 怎麼驗證 |
| --- | --- |
| `alloc_beside` 配在擁有者的那一代；vector 和它的 store 跟著它所在的那一代 | `gc` 的 `young` 的 2 個；`mutants-5a.toml` |
| engine 驅動的 heap 裡節點的資料不進 nursery | `dom` 的 `gc::a_nodes_data_is_allocated_beside_it`；`mutants-5a.toml` |
| renderer 的 DOM heap 沒有 nursery | `dom_js` 的 `wrappers::the_doms_heap_has_no_nursery`；`mutants-5b.toml` |

## 做過的檢查

### 1：量測工具（2026-11-06）

- **先用暫時的腳本試**（重寫這份 RFC 之前的取樣，結果在 `details.md` 的「換 DOM 之後的分析（2026-11-06）」）：
  - Chrome for Testing 的 Linux 版 strip 了（沒有 `.symtab`、debug info），使用者裝了 Fedora 的 Chromium 156 和它的 debuginfo（決定 3）。
  - 第一次 0 個樣本：Linux 上 Chromium 不給 process 的 main thread 命名，`perf` 看到的 renderer main thread 叫 `chromium-browse`；照 trace 的 thread descriptor 的 tid 挑之後還是 0：Chrome 的 trace 是 `base::TimeTicks`（`CLOCK_MONOTONIC`），kun 的讀法把 `perf` 的時間換到 `CLOCK_BOOTTIME`，差了休眠過的 145977 秒。
  - 第一次 20 個 suite 的 Chrome 比 kun 還多 cycle（jQuery 每個 iteration 197 對 92.7 ms 的計時）：`run_in_chrome` 預設的 trace category（`blink`、`v8`、`cc`、`devtools.timeline`……）讓 Chromium 慢一倍；只錄 `blink.user_timing` 之後計時區間比平常長一到兩成（kun 約 6%），所以換成 ms 用比例乘上沒有取樣時的時間。
  - Fedora 的 Chromium 156 比 Chrome for Testing 152 慢 23%（每個 suite 12% 到 47%），取樣的 flag 只多 1.4%；`.text` 285 對 225 MB，應該是沒有 Google 的 PGO。
- **分類照真的 stack 調的**：C++ 的名稱要先去掉回傳型別、template、參數、`const` 和 `[clone .cfi]`（不然「其他」有一半是 `const`）；V8 的 GC 要比對去掉參數的名稱（`LookupIterator` 的參數裡有 `HeapObject`）；rusty_v8 的 callback 外殼（`<... as v8::support::CFnFrom<_>>::mapping::c_fn`，`dom_js` 的成員呼叫 inline 在裡面）名字裡有 `v8::`，暫時的腳本把它當成 V8 自己的、算進了 JS，做進 `categories.py` 時改成 `<` 開頭的 Rust frame 不是 V8 的。
- **做進 `bench.py` 之後核對**：同樣的樣本和時間，`categories.report` 和暫時的腳本的各類只差小數（除了上面的 callback 外殼）。
- **測試**：`speedometer_test.py` 加 `CategoryTests`（10 個）和 `ChromeSampleTests`（1 個）；整份 119 個，74 秒，失敗 1 個是 `KunTests.test_the_windows_are_the_runners_times`（換 DOM 的收尾時就每次失敗，原因還沒找到，記在 RFC 0089 的 `measurements.md`），跳過 2 個。
- **mutation test**（`mutants-1.toml`，22 個，每個約 4 秒，`Chromium sampled in its sandbox` 77 秒：V8 寫不出 perf map，JS 沒有名字）：21 個第一次就抓到；「V8 的 GC 照整個 frame 比對」沒抓到，測試用的 `LookupIterator` 的參數 `Tagged<v8::internal::HeapObject>` 本來就不會被當成 GC（規則要類別名稱後面接 `::`），換成參數裡有 `v8::internal::Heap::AllocationOrigin` 的 frame 之後抓到；trace 只錄 user timing 原本沒有測試守著（改回來只是 Chromium 慢一倍），加了檢查取樣的那一次的 category 和對應的 mutant，抓到。`test.py mutate --restore` 沒有要還原的。
- **驗收**（使用者要求的）：`bench.py speedometer`、`--sample --iterations 5`、`--sample --browser chrome --iterations 5`，kun `8aa9f64d`，4 分 7 秒，`categories.md` 寫出來了。比值的幾何平均 1.499（計時的也是 1.50），Chromium 156 比 Chrome for Testing 152 慢 1.207 倍（之前量的 23%）。每個 iteration 的 ms、20 個 suite 合計：kun 1067.0、Chrome 668.9；文字 +128.7、JS +76.2、style +74.5、DOM +46.6、box layout +41.4、GC +40.2、bindings +28.9。和 `details.md` 的分析比，bindings 從 +12 變成 +28.9、JS 從 +101 變成 +76：暫時的腳本把 callback 外殼（`c_fn`，`dom_js` 的成員呼叫 inline 在裡面）算進了 JS。
- **留下的**：scratchpad 的暫時的腳本試跑時，Chromium 的其他 process 也寫了 perf map，`/tmp` 留著 2026-11-06 21:00 之後的幾百個 `perf-*.map`（現在的程式每個 suite 跑完清掉這次新出現的）；刪除被權限擋下，使用者同意之後刪了 21:00 到驗收開始前的 282 個。

### 2a：找子 frame 不再一直走 DOM（2026-11-07）

- **找到的**：取樣裡 DOM 那一類的 `child_content_documents` 4.1 ms（20 個 suite 合計）都來自 `dom_api::documents_in_tree_order`：renderer 每轉一圈 loop 問動畫要不要 frame（`animations_want_frames`）、rendering update 裡的每一步（media query、動畫、animation frame、resize observer、focus fixup、intersection）各自列一次 frame，每次都走遍 frame tree 的每份文件找 iframe；Speedometer 的 suite 在 iframe 裡，`has_seen_iframes` 的捷徑永遠用不上。
- **改了什麼**（決定 8）：`animations_want_frames` 看 `frames_as_made`；`ScriptEngine` 加 `begin_rendering_update`、`end_rendering_update`、`documents_in_tree_order`，renderer 的 `begin_main_frame` 分派完滑鼠事件之後開始、最後結束，`compute_intersections` 收它的文件；`dom_api::frame_tree_walks_for_testing` 是 debug build 的計數。
- **測試**：`dom_js` 的 `frames` 加 3 個（一次 update 只走一次、照樹的順序、之後再走；loop 的問題不走、看得到子 frame 的動畫；update 期間拿掉的 frame 不給、新加的等下一次）；`dom_api`、`dom_js`、`renderer`、`shell` 的測試都過（84 秒）；WPT 的 `intersection-observer/`、`resize-observer/`、`html/webappapis/update-rendering/`、`css/css-animations/`、`web-animations/` 5869 個檢查都照預期（32 秒）。
- **取樣**（使用者要求的，`bench.py speedometer --browser kun` 和 `--sample --iterations 5`，4 分 28 秒；Chrome 用上次的）：走 frame tree 找 iframe（`documents_in_tree_order`、`child_content_documents`）每個 iteration 2300 萬 → 120 萬個 cycle（−95%，約 4.5 → 0.2 ms），剩下的是每次 rendering update 的那一次。各類看不出：DOM 118.0 → 118.8 ms、合計 1067.0 → 1068.6 ms、比值的幾何平均 1.499 → 1.497；省下的約 4 ms（合計的 0.4%）比兩次取樣之間各類的起伏（文字 −6、JS +4 ms，和這次無關）小。
- **mutation test**（`mutants-2a.toml`，8 個，168 秒）：7 個抓到；「renderer 不結束 update」等價：renderer 列 frame 的地方都在 update 裡，每次開始重新找，記進 `docs/test-gaps.md`。renderer 不呼叫 `begin_rendering_update` 只會變慢，沒有計數守著（renderer 的 thread 上的計數 `shell` 的測試讀不到），也記進去了。`test.py mutate --restore` 沒有要還原的。

### 2b：`innerHTML` 的路（2026-11-07）

- **找到的**（換 DOM 之後的取樣，TodoMVC-jQuery 每個 iteration）：`innerHTML` kun 74.7 ms、Chrome 37.5；解析和建節點 61.4 對 25.2（Chrome 是 `HTMLFastPathParser::ParseChildren`），換掉子節點 13.2 對 12.2。kun 的 61.4 裡：建文字節點 11.3（兩個 GC 物件的配置和 `publish`）、設屬性 10.9（每個屬性一個 `Attribute` 4.8、先找同名的 1.8、`intern_value` 1.8、`names::intern` 1.8）、`scan` 10.0、建元素 9.1（`form_element_made` 3.8，幾乎都是每個 input 配的 element data、表單關聯、型別三個物件；`names::intern` 2.7，HTML 的命名空間字串每次也查）、掛上屬性 6.3（`attribute_changed` 3.2，大半是 input 從 text 改成 checkbox 的 `input_type_changed`）、插入 6.1（`link` 的 write barrier）、`decode` 3.6（每個值和文字在 scan 檢查時解一次、build 又解一次）。
- **改了什麼**（決定 9）：`dom` 的 `create_html_element_with_attributes`，fast path 的 build 改用它：名稱用編號建，屬性不先找同名的（scan 已經擋掉重複的）、清單一次配到大小，input 在屬性之前照 `type` 建成那個型別（`input_made_of_type`，Blink 的 `InitializeTypeInParsing`；radio 照樣記下看過），每個屬性的 change steps 照樣跑。`fast_path::scan` 把解碼的結果留在事件和屬性裡，build 直接用。debug build 比對 fast path 和 tree builder 的結果時也比 input 的型別和 checkedness。計數：`dom::input_type_changes_for_testing`、`html::decodes_for_testing`。
- **測試**：`dom` 的 `attributes` 加 1 個（帶著屬性建的元素和一個一個設的一樣：屬性、id、class、bloom、事件處理器、input 的型別、checkedness、值；input 沒有改型別）；`html` 的 `fast_path` 加 2 個（input 和 tree builder 建的一樣、沒有改型別；每個值和文字解碼一次）；`style` 的 `form_states` 加 1 個（這樣建的 radio 記下看過，checked 的進來時同組的重算 `:indeterminate`；用 DOM 的入口直接建，因為 debug build 經過 parser 時 tree builder 也建一次，會記下看過）。`dom`、`html`、`dom_api`、`dom_js` 的測試都過（約 1 分鐘）；WPT 的 `domparsing/`、`dom/nodes/`、`html/semantics/forms/the-input-element/` 都照預期（1 分 13 秒）。
- **mutation test**（`mutants-2b.toml`，11 個，72 秒）：10 個抓到；「debug build 不比 input 的型別」等價：屬性一樣時只有 fast path 建錯才會不同，記進 `docs/test-gaps.md`。名稱用編號、不找同名的、清單配好大小沒有計數守著（拿掉時結果一樣），也記進去了。`test.py mutate --restore` 沒有要還原的。
- **量測**（AC、綁在快的核心，HEAD `898a27e1` 對工作目錄，各一輪）：blink-perf 的 `parser/innerHTML-setter` 6354 → 6700 runs/s（+5.5%）、`tiny-innerHTML` 41.8 → 42.3（+1.2%）、`innerHTML-setter-siblings` 46506 → 48802（+4.9%），幾何平均 −3.7%。照 TodoMVC 的 markup 寫的一頁（100 個 item，每個有 class、`data-id`、checkbox、`value`、`&amp;`；scratchpad 的，不在 blink-perf 裡），`perf stat` 兩輪：4435 → 5150 runs/s（+16%），每一次的指令少約 13%、cycle 少約 11%。比預估的大：Speedometer 的取樣還沒跑。

### 2c：屬性的成員和字串參數（2026-11-07）

- **先量**（`bench.py calls`，HEAD `f2c75545`）：`getAttribute` 750 條指令、`setAttribute` 1299、`addEventListener` 885、`createElement` 2304。`calls` 的 IBS 取樣裡字串換成名稱編號（`find_fixed`）佔 `getAttribute` 的 21%、`addEventListener` 的 5%；參數本來就是字串時跳過 `ToString` 的實驗每次只少約 10 條、store forwarding 的衝突反而變多，還原了。
- **和 Chrome 比**（使用者要求的；Fedora 的 Chromium 156 和 kun 跑 blink-perf 的 `bindings/get-attribute`、`set-attribute`，IBS 取樣，只算頁面迴圈裡的樣本，每項的 ns 是它佔的 micro-op 比例乘每次呼叫的時間）：`getAttribute` Chromium 25.1 ns、kun 34.6；`setAttribute` 61.5、86.8。Chromium 每次也查 `AtomicStringTable`（`GetExternalStringResourceBaseSlow`、`SupportsExternalization`、`WriteToFlat`、`AtomicStringTable::Add` 每次都在：短的 internalized 字面值 V8 不讓 externalize），字串那部分 Chrome 不比 kun 快（`setAttribute` 兩個參數 20.5 對 29 ns，`getAttribute` 3.3 對 7.8）。差在：`setAttribute` 的 id 沒變也更新 id 表（kun 17.8 ns，Blink 0）、binding 的固定成本（14.3 對 17.5、9.1 對 16.3）、`getAttribute` 判斷要不要轉小寫（0.1 對 6.5）。cycle 的取樣兩邊在函式之間都有 skid（kun 的 `lends` 29%、Chromium 的 `AtomicStringTable::Add` 86%），IBS 的樣本位置要照 `perf script -G` 讀，call chain 的第一格是中斷的位置。
- **改了什麼**（決定 10）：`attribute_changed` 在 id 的新舊值一樣時不動 id 表和表單的 id；`dom_api` 的 `qualified` 先看名稱有沒有大寫字母，有才看元素和文件；`bindings::LentString` 找到 ASCII 之後記下，之後只重讀字元的位置（`crates/bindings/soundness.md` 補了字元不會變的論證）。計數：`dom::id_table_changes_for_testing`、`bindings::ascii_scans_for_testing`。
- **測試**：`dom` 的 `ids` 加 1 個（id 設回同一個值不動表、兩個同 id 的第一個不用重找、換值動兩次）；`dom_js` 的 `dom_core` 加 1 個（HTML 文件裡的 HTML 元素轉小寫，SVG 元素和 XML 文件裡的不轉）；`bindings` 的 `v8_text` 加 1 個（ASCII 的字串只掃一次，不是 ASCII 的每次掃、不借）。`dom`、`dom_api`、`dom_js`、`bindings` 的測試都過。
- **mutation test**（`mutants-2c.toml`，5 個）：第一次 4 個抓到，「找到 ASCII 之後還是每次掃」只會變慢，補了 `ascii_scans_for_testing` 和測試之後 5 個都抓到。`test.py mutate --restore` 沒有要還原的。
- **量測**（AC、綁在快的核心，HEAD `f2c75545` 對工作目錄，各一輪）：`calls` 的 `getAttribute` 750 → 714、`setAttribute` 1299 → 1261（設的是 `a`，看得到的只有轉小寫和 ASCII 兩項）、`createElement` 2304 → 2277，其他不變。blink-perf：`get-attribute` 283 → 303 runs/s（+7.3%，每次 35.4 → 33.0 ns）、`set-attribute` 1174 → 1588（+35.2%，85.2 → 63.0 ns）、`id-setter` 307 → 436（+42.1%）、`id-getter` 679 → 701（+3.2%），幾何平均 −17.2%。Chromium 156 是 25.1 和 61.5 ns：`setAttribute` 追平了它，`getAttribute` 還差約 8 ns，剩下的主要在 binding 的固定成本（第 4 段）。
- **`get-attribute` 為什麼只快 2.4 ns**：預估的 5 到 10 ns 是 IBS 的 micro-op 比例乘每次的時間，高 IPC 的小函式被算多了。改完再取樣，`element::qualified` 還是佔 20% 的 op：剩下的是它自己的 prologue、epilogue（存回 6 個暫存器）和掃兩個字元，拿掉的元素和文件的判斷只是一小部分；`ArgString::text` 也一樣（15%）。`setAttribute` 的 id 表那項照預估省到了。之後的估計用實驗（拿掉一段、量 `calls` 的指令和 cycle），不用 op 的比例換 ns。
- **階段做完的檢查**：`cargo fmt`（改了 `dom`、`dom_api` 和 2a 的 `dom_js` 測試的排版，`mutants-2c.toml` 跟著改）、`cargo clippy --workspace --all-targets -- -D warnings` 沒有警告、`cargo test --workspace` 都過（1 分 58 秒）；WPT 的 `dom/nodes/`、`html/dom/`、`domparsing/` 都照預期（54 秒）。

### 3：JS（2026-11-07）

- **給 V8 的 flag**：kun（`MACHOPO_V8_FLAGS=--print-flag-values`，`kun --headless` 開 `about:blank`）和 Chrome for Testing 152（`--js-flags=--print-flag-values`；同一版 V8 15.2，每個 process 各印一次，8 個 process）逐項比。renderer 的 process 和 kun 差 14 項：JIT 和 IC 的 `max-valid-polymorphic-map-count`（Chrome 10、kun 4）、`homomorphic-ic`、`sparkplug-plus`（Chrome 開）、`additive-safe-int-feedback`（Chrome 關）、`verify-bytecode-light`（Chrome 開，安全檢查）；V8 的 heap 的 `compaction-on-regular-gcs`（Chrome 關）、`sqrt-allocation-limits`、`preconfigured-old-space-size`（16 對 32）、`memory-reducer-delay-ms`（15000 對 8000）、`managed-zone-memory`、`memory-pool-release-on-malloc-failures`、`single-threaded-gc-in-background`（kun 刻意關，`crates/bindings/src/platform.rs`）；功能的 `enable-sharedarraybuffer-per-context`、`js-import-text`。都是 Chrome 照 feature 給的，不是 V8 的預設。另一個 process（沒有優化編譯器，應該是 utility）不算。
- **flag 的實驗**（使用者要求的；`MACHOPO_V8_FLAGS` 帶 flag，kun 的 log 的 `V8 initialized` 確認收到；React-Stockcharts-SVG、Charts-chartjs、TodoMVC-React-Redux 各 10 個 iteration，三種設定輪流兩輪，2 分 53 秒）：現在的設定 92.2、93.8／58.0、59.1／31.1、30.0 ms；加上 JIT 的四項（`max-valid-polymorphic-map-count=10`、`homomorphic-ic`、`sparkplug-plus`、`no-additive-safe-int-feedback`）+1.7%、+0.9%、+1.5%；再加上 heap 的六項 −0.4%、−0.5%、+0.9%。都在兩輪之間的起伏（最多 5%）之內：Chrome 的 flag 不是 JS 慢的原因。
- **JS 那一類拆開**（第 1 段和 2a 的樣本，不重跑；20 個 suite 合計，每個 iteration 的 ms，Chrome 是 Chromium 156 的比例乘上 Chrome for Testing 152 的時間）：kun 324.3、Chrome 243.7。照 tier 合計（各 tier 的程式碼加上它呼叫的 builtin 和 IC）：Sparkplug 116.7 對 78.7（1.48 倍）、Maglev 67.6 對 53.1（1.27）、TurboFan 14.4 對 13.3（1.08）；V8 的其他 C++ 52.9 對 42.1、編譯 20.3 對 13.6。差最多的是 Sparkplug 的 builtin（`_Baseline`）40.9 對 20.6：`LoadICGenericBaseline` 23.4 對 10.5，`StrictEqual`、`Add`、`LessThan`、`Equal`、`GreaterThan`、`Increment` 這些 `*_Generic_Baseline` 合計約 10 對不到 0.5。取樣的 Chromium 156 是 V8 15.6、kun 和 Chrome for Testing 是 15.2：Sparkplug 在比較和算術上 inline 了快的路徑可能是新版才有的，要用同一版的 Chrome 比。Chrome for Testing 152 沒有符號，但 V8 的 perf map 有 builtin（`Builtin:Name`）和 JS 函式的名字，JS 那一類分得出來。
- **同一版的 Chrome**（使用者要求的；`--sample --browser chrome --chrome-binary` 指向 Chrome for Testing 152，20 個 suite 各 5 個 iteration，1 分 34 秒，跑兩次：第一次工具照例刪掉了 perf map，JS 的名字查不回來）：embedded builtin 在 chrome 的 `.text` 裡，`perf` 照沒有符號的執行檔找，都是 `[unknown]`；從一次 run 的 perf map 和執行檔的 mapping（`PERF_RECORD_MMAP2` 的起點和 pgoff）算出每個 builtin 在檔案裡的 offset，照樣本的 `dsoff` 換回名字（scratchpad 的 `refold.py`；Blink 的 C++ 還是 `[unknown]`，所以只比 leaf 是 JIT 的程式碼或 builtin 的樣本）。kun 222.3、Chrome 152 172.7 ms：Sparkplug 的 builtin 49.1 對 29.1（1.69 倍），其他 1.07 到 1.36 倍。Chrome 的 Sparkplug 呼叫特化的 `LoadICConstantFromPrototypeBaseline`、`LoadICInObjectNonDoubleFieldBaseline`，比較和算術幾乎不呼叫 builtin（`StrictEqual_Generic_Baseline` 0.4 對 5.3 ms），kun 一個特化的也沒有：Chromium 156 那邊差 40 倍的也是這個，不是 V8 的版本。kun 的測試的 binary 跑一段只在 Sparkplug 的迴圈（`--no-maglev --no-turbofan`）確認：不加 `--sparkplug-plus` 時前幾名是 `LoadICGenericBaseline` 32% 和 `LessThan`、`Add`、`StrictEqual` 的 `_Generic_Baseline`，加了之後是 `LoadICInObjectNonDoubleField1Baseline`、比較和加法 inline 了。之前的計時量不出來：20 個 suite 合計 20 ms，分到三個 suite 各約 1%，在起伏裡。
- **改了什麼**（決定 11）：`bindings` 的 `CHROME_FLAGS`（四項，放在呼叫者的 flag 和 `MACHOPO_V8_FLAGS` 之前）；`--no-single-threaded-gc-in-background` 和 `--no-cppgc-young-generation` 的註解和 `crates/bindings/soundness.md` 改成現在的理由。`bindings`、`dom_js` 的測試都過。
- **取樣驗收**（使用者要求的；`bench.py speedometer --browser kun`、`--sample --iterations 5`，3 分 56 秒；Chrome 那一側用 Chromium 156 的樣本，`categories.md` 照它重算）：JIT 的程式碼和 builtin 222.3 → 208.8 ms（Chrome 152 172.7），Sparkplug 的 builtin 49.1 → 36.3（29.1），其他項在起伏裡。各類和 2a 之後的取樣比：JS 324.3 → 313.4、DOM 118.8 → 100.5、HTML parse 21.1 → 14.9（DOM 和 HTML parse 是 2b、2c 的），其他類別 ±5 ms 之內；合計 1068.6 → 1029.2 ms，比值的幾何平均 1.497 → 1.465。
- **mutation test**：拿掉 `CHROME_FLAGS` 只會變慢，沒有測試失敗（V8 的 flag 從 rusty_v8 讀不到，JS 也看不出 Sparkplug 呼叫哪個 builtin），記進 `docs/test-gaps.md`，沒有寫成 spec。
- **code cache、編譯**（同一批樣本，stack 上有那一類的 frame 就算，20 個 suite 合計）：計時的區間裡兩邊都沒有 code cache 的讀寫（`CodeSerializer`）；parse 5.6 對 4.8 ms（Chromium 156）、bytecode 的產生 4.9 對 3.6、lazy compile 2.9 對 2.7、Maglev 和 TurboFan 在 main thread 上的部分 12.5 對 10.3。Chrome 就算用了 code cache 和 compile hints，也是在載入的時候，不在 Speedometer 計時的區間裡；這些和其他 JIT 的程式碼、builtin 一樣約 1.2 倍，是 V8 的 build（Chrome 的 PGO、official build），不重編 V8 時到這裡為止。
- **階段做完的檢查**：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 沒有問題，`cargo test --workspace` 都過（36 秒，大多已經編好）。

### 4：binding 和 blink-perf 的頁（2026-11-07）

- **現況**（`bench.py blink-perf --pages` 那 26 頁，kun `7c7d2b68` 對 Chrome for Testing 152，各一輪；kun 慢幾倍）：`get-elements-by-tag-name` 13.2、`get-element-by-id` 5.97、`CSSPropertySetterGetter` 4.14、`gc-forest` 3.66、`create-element` 3.35、`PseudoClassSelectors` 3.18、`EventsDispatchingInV1ShadowTrees` 3.04、`gc-mini-tree` 2.53、`gc-tree` 2.32、`node-type` 2.08、`first-child` 1.94、`flexbox-row-nowrap` 1.91、`id-getter` 1.65、`id-setter` 1.64、`get-attribute` 1.54、`set-attribute` 1.41；比 Chrome 快的照舊（`append-child` 0.28、`insert-before` 0.33、`child-list-mutation-with-unrelated-observer` 0.47、`modify-element-id` 0.74）。
- **全域的 `document` 每次都走 V8 的慢路徑**：`get-element-by-id` 的 IBS 樣本裡 `getElementById` 本身約 30 ns，`Runtime_LoadGlobalIC_Slow` → `LoadIC::Load` → `GetPropertyWithAccessor` → `InvokeApiFunction`（呼叫 `document` 的 getter）約 60 ns；Chromium 156 同一頁只有 `ToBlinkString`、callback、`TreeOrderedMap::Get`。兩邊的迴圈都是 TurboFan 的程式碼。global object 是 dictionary mode，V8 的 IC 對它上面的 API accessor 只給 slow stub；Blink 的 `window.document` 是 `[CachedAccessor=kWindowDocument]`（V8 的 `FunctionTemplate::NewWithCache`：getter 的結果放在 global 的 private property，IC 和 TurboFan 當成 data property 讀、常數化），kun 沒有（rusty_v8 也沒有 bind `NewWithCache`）。拿掉 Window 的 access check 的實驗沒有差別（−8.3%，還原了）。同一個迴圈每次讀 `document` 對先存成區域變數（scratchpad 的頁，kun 和 Chrome 各一次）：kun 104 對 39 ns、Chrome 17.4 對 18.2 ns，每次多 65 ns；`window.length` kun 178、Chrome 59 ns（Blink 的 `window` 也是 cached accessor）。Speedometer 的樣本裡經過 `LoadGlobalIC_Slow` 的只有 2.4 ms（20 個 suite 合計），blink-perf 在迴圈裡讀 `document` 的頁（`get-element-by-id`、`get-elements-by-tag-name`、`create-element`、`gc-forest`、`gc-mini-tree`、`gc-tree`）都有：`create-element` 每次 186 對 55.6 ns，`gc-forest` 一百萬次約 65 ms（Chrome 整頁 57.9 ms）。
- **4a 改了什麼**（決定 12）：rusty_v8 的 fork 的 `v8::kun::function_template_with_cache`（`FunctionTemplate::NewWithCache`；`src/machopo.rs`、`src/binding.cc` 的 `v8__kun__FunctionTemplate__NewWithCache`，`README.machopo.md` 記了；還沒 commit：改了 C++ 的 glue，要發布新的 release 才能釘），`bindings::state_cached_getter_template`（`remove_prototype`：getter 不是建構子），`dom_js` 的 `window::CachedAccessor`（`Window` 的 `window`、`document`，照 Blink 的 `kWindowProxy`、`kWindowDocument`）和 `update_cached_accessors`：建立 realm 時和 `reuse_initial_window` 換 document 時，把 window proxy 和 document 的 wrapper 寫進 global proxy 的 private property（Blink 的 `LocalWindowProxy::UpdateDocumentProperty`；V8 的 `LookupCachedProperty` 對 API 的 getter 認定 holder 有那個 property，所以建立時就寫）。本機用 `ci.py v8 --profile release` 編的 V8（只重編 glue，8 秒；`../third_party/rusty_v8_artifacts` 的 release 換了），`RUSTY_V8_ARCHIVE` 連結。計數：`dom_api::window_document_gets_for_testing`。
- **4a 的測試**：`dom_js` 的 `frames` 加 1 個（迴圈裡讀 `document` 不呼叫 getter、直接呼叫 getter 照樣呼叫；getter 沒有 `prototype`、`new` 丟 `TypeError`；同 origin 沿用的 window 換了 document 之後，`document` 是新的，V8 最佳化過的函式也是）。`dom_js`、`dom_api`、`bindings`、`shell` 的測試都過（1 分 28 秒）；WPT 的 `html/browsers/the-window-object/`、`html/browsers/windows/`、`html/dom/`、`webidl/`、`html/browsers/browsing-the-web/`（145 個測試、60295 個檢查）和 `html/semantics/embedded-content/the-iframe-element/`、`html/browsers/history/`、`html/browsers/origin/`、`dom/nodes/`（563 個測試、16135 個檢查）都照預期。
- **4a 的 mutation test**（`mutants-4a.toml`，5 個）：都抓到。`test.py mutate --restore` 沒有要還原的。
- **4a 的量測**（AC、綁在快的核心，HEAD `7c7d2b68` 對工作目錄，各一輪）：`get-element-by-id` 95.4 → 249.2 runs/s（+161%；Chrome 152 是 571，5.97 → 2.29 倍）、`create-element` 1043 → 2172（+108%；3599，3.35 → 1.66）、`gc-forest` 206.6 → 105.8 ms（57.9，3.66 → 1.83）、`get-elements-by-tag-name` 86.7 → 151.9（+75%；1142，13.2 → 7.5），幾何平均 −51.9%。
- **4b：`getElementsByTagName` 的 collection**：IBS 的樣本裡 kun 每次新建 collection（GC 物件、wrapper：`wrapper_of`、`CopyJSObjectWithAllocationSite`、`GlobalizeTracedReference`、`ExternalEntityTable::TryAllocateEntry`）加上名稱的查找；Blink 的 `NodeListsNodeData` 照種類和名稱弱弱地留著，同一個根再問同一個名字給同一個物件（DOM 允許）。改了什麼：根的 rare data 上的 `collection::NamedCollections`（8 格 `WeakMember<HtmlCollection>`，新的放進死掉的那格、否則最舊的那格），`elements_by_tag_name`、`elements_by_tag_name_ns` 先照（種類、prefix 或 namespace、local 的編號）找。`getElementsByName` 不做：它每次在 collection 外面新建 `NodeList`，只快取裡面那層沒有用，也不在追的頁裡。`dom_js` 的 `collections::a_collection_from_script` 原本斷言每次是新的，改成同一個（Chrome 的行為）。
- **4b 的測試**：`dom` 的 `collection` 加 1 個（握著時是同一個；別的名字、種類（同樣的編號、另一種）、根是另一個；根不留住它們：沒人握的 GC 之後收掉，握著的換了很多名字之後還找得到）。`dom`、`dom_api`、`dom_js` 的測試都過；WPT 的 `dom/nodes/`、`dom/collections/`、`html/dom/`、`html/semantics/forms/`（710 個測試、79019 個檢查）都照預期。
- **4b 的 mutation test**（`mutants-4b.toml`，6 個）：第一次 5 個抓到，「找到另一種的」沒抓到（測試裡兩種的 key 沒有相同的），補了 `getElementsByTagName('span')` 對 `getElementsByTagNameNS(null, 'span')` 之後 6 個都抓到。`test.py mutate --restore` 沒有要還原的。
- **4b 的量測**（HEAD `d7646ba4` 對工作目錄，各一輪）：`get-elements-by-tag-name` 151.7 → 600.7 runs/s（+296%；Chrome 152 是 1142，7.5 → 1.90 倍）。
- **4c：getter 的固定成本**：`nodeType` 一層一層拿掉量（`bench.py calls`，每個實驗重編 perf 的測試 binary；暫時的分支只在 `nodeType` 的 `Member<M>::call` 裡，量完還原）：什麼都不做 50 條指令、11 cycle（V8 的地板加 trampoline），直接設回傳值 54、11，建 `V8Front` 和空的操作 50、11（被優化掉），強迫開操作 123、27（`black_box(cx)` 把 `ApiCx` 寫進 stack，高估），加上算 tag 126、28，在操作裡 unwrap `this` 150、30，完整的 167、33。IBS 的樣本（約 1300 個）：`c_fn` 約 90 條（進出、建 `V8Front` 和 `Call` 的約 25 個 store、inline 的 unwrap、cage 和 marking 的檢查、`node_type` 的跳躍表），加上每次呼叫的 `drop_glue::<Call>`（op 的 8%：一次 call、6 個 push、兩個 `Box` 的檢查、6 個 pop，約 18 條）。Chrome 的 getter 只有 receiver、`ToWrappableUnsafe` 和 `ReturnValue`，整個約 18 cycle（決定 13）。
- **4c 的甲**：getter 專用的 front 要把 `V8Op`（借 `Call` 寫回傳值）一起拆開，改動太大；先做確定的那部分：`Call` 的 `kept`、`rare` 改成 `ManuallyDrop`，`Call` 自己的 `Drop` inline 看兩個都是空的就不做事，不空才走 cold 的 `drop_held`（`take()` 出來丟掉，沒有 `unsafe`）。`calls`：每個成員少 12 到 14 條（`nodeType` 167 → 154、33 → 28 cycle，`firstChild` 188 → 176、37 → 32，`getAttribute` 714 → 700，`createElement` 2277 → 2264，`dispatchEvent` 3480 → 3468）。blink-perf（HEAD `4fb46662` 對工作目錄，各一輪）：`node-type` 1347 → 1518 runs/s（+12.7%；Chrome 152 是 2822，2.08 → 1.86 倍）、`first-child` 1228 → 1369（+11.5%；2373，1.94 → 1.73）、`id-getter` +3.8%、`get-attribute` −2.5%（起伏），幾何平均 −5.8%。`dom_js` 的測試都過。拿掉 inline 的檢查只會漏掉兩個 `Box` 的釋放（洩漏），測試看不出來：記進 `docs/test-gaps.md`，沒有寫 spec。
- **4d：`createElement`**：`calls` 的 `createElement` 的 IBS 樣本（`perf report --sort sym`；perf profile 的 debug info 沒有 inline 的資訊，`llvm-addr2line -i` 拆不開 `c_fn`）照函式分類，比例乘上 2264 條：接上 wrapper 約 400、配置 DOM 物件約 390、glue 約 340、V8 的 scavenge 丟掉 wrapper 約 215、clone boilerplate 約 210、名稱約 180、creation context 約 90；Chromium 156 的 `CreateElementOperationCallbackForMainWorld` 裡 V8 那一側的比例差不多，配置（`HTMLDivConstructor`、`Node::Node`）和 glue 各約 6%（決定 15）。
- **4d 的乙：第 5 段 GC 暫存的配置改動**（`git stash apply`，沒有衝突）：`gc`、`dom` 的測試都過。`calls`：`createElement` 2264 → 2237 條、501 → 457 cycle，其他不變。blink-perf（HEAD `8559702b` 對工作目錄，各一輪）：`create-element` 2150 → 2285 runs/s（+6.3%）、`gc-forest` 106.2 → 102.5 ms（−3.5%）、`gc-mini-tree` 124.5 → 119.4（−4.1%）、`gc-tree` 141.7 → 128.9（−9.0%），幾何平均 −5.7%：這次沒有決定 7 那時的程式碼排列的退步。`gc-mini-tree`、`gc-tree` 在 4a 之後已經從 243、285 ms 降到 125、142（Chrome 152 是 96.1、123）。mutation test（`mutants-4d.toml`，5 個，原本是第 2 段寫的）：4 個抓到，「型別的 JS 邊永遠找不到」卡住（900 秒，算抓到）；基準 9 秒，timeout 改成 120。`Cx::node`、`Node::__init`（多半是寫十幾個欄位，`None` 的 `Member` 的 barrier 一進去就跳過）、`make_element`（幾個特殊元素的判斷）能省的不多，沒有動。
- **4d 的甲：`createElement` 逐層拆**（`calls`，暫時的分支只在 `Document.createElement` 的 `Member<M>::call` 裡，前 3 次照常建立：`calls` 的準備自己也建元素；量完還原）：什麼都不做 89 條指令，取字串參數 242（+153），加上預先建 wrapper（偷看名稱、查 interface、比 creation context、clone boilerplate）840（+598），加上開 `mutate`、unwrap `this` 901（+61），加上建元素 1318（+417），完整的 2236（+918：把 wrapper 接上元素，和 V8 的 scavenge 處理接上了的 wrapper），完整、不 `report` 2266（`report` 幾乎是 0）。cycle 兩輪之間差很多（完整的 448、535），以指令數為準。和 IBS 的函式分布對起來，最大的兩段多半是 V8 建 wrapper、接上 wrapper 的機制（clone boilerplate、`CppHeapPointerTable` 的一格、`GlobalizeTracedReference`、scavenge 的 traced handle），Chrome 也一樣；kun 自己的只剩 `kun::Wrap` 的 C++ 包裝和 `IsApiWrapper`（約 40 條：配那一格要留在 V8 裡）和名稱的檢查。「名稱只查一次」細看之後不做：`names::find` 找得到任何 intern 過的名稱（class 的 token、`#document`），找得到不等於合法；改成查特定 HTML 元素的 interface 本身也要約 6 條，只省約 15 條（0.7%）。兩項都記進 README 的「之後的可能」。
- **4e：`dispatchEvent` 經過 shadow tree**（`EventsDispatchingInV1ShadowTrees`：10 層、每層高 50 的 shadow tree，最左的葉子對最右的葉子 dispatch 帶 `relatedTarget` 的 `mousemove`，沒有 listener）：IBS 的樣本裡 `is_in_shadow_tree_of` 佔 67%、`retarget` 11%：path 上每個節點的 target、related target 各 retarget 一次，`shadow_path` 每一步問 parent 在不在 target 的樹裡，每次都走 shadow tree 的鏈，約 500 個節點是 O(n²)。Chromium 156 的是 `EventPath::Initialize`、`BuildRelatedNodeMap`、`FindRelatedNode`：一個 tree scope 找一次。改了什麼：`dom_api` 的 `event_target.rs` 的 `ScopeKey`（節點所在或所是的 shadow root，或沒有）和 `Retargeter`（retarget 只透過 `b` 的 tree scope 讀 `b`：同一個 scope 沿用上一個的結果），`shadow_entries` 和 `shadow_path` 用它，`shadow_path` 的 `last_in` 在 target 換了時清掉。計數：`dom::retargets_for_testing`。
- **4e 的測試**：`dom_js` 的 `shadow_dom` 加 1 個（5 層、每層 20 深的 shadow tree，composed、bubbles 的 `mousemove`：HEAD retarget 276 次，改了之後 20 次，上限 30；listener 看到的 related target 照 DOM）。`dom_api`、`dom_js` 的測試都過；WPT 的 `shadow-dom/`、`dom/events/`、`uievents/`（606 個測試、2623 個檢查）都照預期。
- **4e 的 mutation test**（`mutants-4e.toml`，6 個，測試是 `dom_js` 的 `shadow_dom` 和 WPT 的 `shadow-dom/`）：都抓到。`test.py mutate --restore` 沒有要還原的。
- **4e 的量測**（HEAD `f98cf5d1` 對工作目錄，各一輪）：`EventsDispatchingInV1ShadowTrees` 58274 → 223229 runs/s（+283%；Chrome 152 是 176306：3.04 → 0.79 倍，比 Chrome 快）；`EventsDispatching`（沒有 shadow tree）2448 → 2434（−0.6%，起伏；Chrome 2920，約 1.20 倍）。
- **4f：`element.style`**（`CSSPropertySetterGetter`：61 個屬性各讀一次、設成空的、再設回去）：IBS 的樣本（renderer 的 main thread 全部：迴圈的 marker 只抓到 2.8%）裡解析 URL 約 19%（`url::Parser`、`idna`、`UrlContext::new`、`document_base_url`）、把數值寫成字串約 10%（`flt2dec::dragon`、`grisu`、`fmt::write`）、宣告集合的增刪約 13%、照名稱找屬性約 6%。原因：`element.style` 每次設定先建 `UrlContext`（document 的 URL 和 base URL），不管值有沒有 `url()`；`with_document_base_url` 每次先 `Url::parse` document 的 URL（看是不是 `about:`），才看 base URL 的快取；`write_number` 每個數值兩次精確的 `format!`。改了什麼：`CssPropertyValueSet::set_property_resolved` 的 base 是 closure，只在 parse 出來的宣告有沒解析的 `url()` 時才呼叫（沒有 `url()` 的 `resolve_urls` 不改任何東西）；`cssText` 先看 `has_unresolved_urls`；`with_document_base_url` 先用字串看 `about:`，不是的話先看快取；`write_number` 小於 10⁶ 的整數直接寫（最多 6 位數，`%g` 寫出的就是它）。
- **4f 的測試**：`css` 的 `values` 加 1 個（數值照 Chrome 的 `%g`：整數、`1e+06`、`1.23457e+06`、小數、指數）；`dom_api` 的 `css_style` 加 1 個（`cssText`、`setProperty` 設的 `url()` 對文件的 URL 解析了，沒有 `url()` 的照設的）。`css`、`dom`、`dom_api`、`dom_js`、`style` 的測試都過；WPT 的 `css/cssom/`、`css/css-backgrounds/`、`html/infrastructure/urls/`（757 個測試、10841 個檢查）都照預期。
- **4f 的 mutation test**（`mutants-4f.toml`，6 個）：第一次 3 個抓到；補了上面兩個測試之後 5 個抓到，「`about:` 的文件也從快取拿」等價（快取只在非 `about:` 的路徑寫，`about:` 文件的那一格永遠是空的），記進 `docs/test-gaps.md`。重跑時基準那一輪 `dom_js` 的 `idle::the_time_remaining_is_never_past_the_deadline` 失敗：它斷言取 deadline 到 script 讀 `timeRemaining()` 不超過 1.67 ms（`r > 15`），mutation test 剛編完、整個 crate 的測試佔滿每個核心時超過了；不是 idle callback 的錯，deadline 改成 50.067 ms（一樣不在 100 µs 的格點上）、斷言 `r <= 50.067`、`r > 40`，容許 10 ms 的延遲，上限和格點照樣檢查。`test.py mutate --restore` 沒有要還原的。
- **4f 的量測**（HEAD `5ddb30d9` 對工作目錄，各一輪）：`CSSPropertySetterGetter` 5187 → 8229 runs/s（+58.7%；Chrome 152 是 21376，4.14 → 2.60 倍）。之後的樣本：宣告集合的增刪約 20%（`memmove`、`remove_property`、`set`、`shorthand_value`）、照名稱找屬性約 10%（`css_property_for_attribute`、`Shorthand::from_name`、`LonghandId::from_name`）、回傳字串的快取約 4%（rusty_v8 的 `FinalizerMap` 用 std 的 SipHash、`StringCache::insert`）。
- **4f 的甲：名稱的查找記住**（使用者同意試）：照 Blink 的 `CssPropertyInfo`，`css_property_for_attribute` 和 `PropertyName::parse`（自訂屬性之外）的結果各記在一張 thread-local 的 `FxHashMap`（上限 1024 項）。`CSSPropertySetterGetter` 8198 → 8411 runs/s（+2.6%，在一輪的起伏裡）：IBS 的比例又高估了時間（`match_ignore_ascii_case` 的指令多、IPC 高），換成 hash 表加 `RefCell` 也沒有便宜多少。為了這麼一點多兩張表和兩條 sanity 的例外，不值得：還原了。徹底的改法是照 Blink 把屬性的 id 從 `css_property_for_attribute` 一路傳到 `property_value`、`set_property_resolved`、`remove_property`（現在一次存取照名稱查三、四次），要改 `css` 好幾個公開的 API，記進 README 的「之後的可能」。
- **第 4 段做完的檢查**（V8 還是本機編的、`RUSTY_V8_ARCHIVE`：fork 的 `kun-v152.2.0-28` 那時 CI 還在編）：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 沒有問題，`cargo test --workspace` 都過（1 分 58 秒）。blink-perf 的 26 頁（kun `b316d886` 和 Chrome for Testing 152 同一次）：22 頁有效頁的幾何平均 kun 慢 2.27 → 1.55 倍；`get-elements-by-tag-name` 13.2 → 1.96、`get-element-by-id` 5.97 → 2.28、`CSSPropertySetterGetter` 4.14 → 2.57、`gc-forest` 3.66 → 1.71、`create-element` 3.35 → 1.58、`PseudoClassSelectors` 3.18 → 2.20（迴圈裡讀全域的 `document`，4a 的）、`EventsDispatchingInV1ShadowTrees` 3.04 → 0.81、`gc-mini-tree` 2.53 → 1.25、`gc-tree` 2.32 → 1.13、`node-type` 2.08 → 1.86、`first-child` 1.94 → 1.80、`parser/query-selector-all-class` 2.89 → 2.41。`docs/performance-gaps.md` 的「blink_perf：哪一種操作慢」那一節照這次改寫。
- **V8 的 release**：使用者發布了 fork 的 `kun-v152.2.0-28`（`8c5670e`，加了 `function_template_with_cache`）；kun 的 `RUSTY_V8_MIRROR_TAG` 移過去，`third_party.toml` 暫時的 `rev` 拿掉。不帶 `RUSTY_V8_ARCHIVE` 編：下載的 `librusty_v8.a` 有 `v8__kun__FunctionTemplate__NewWithCache`，`cargo test --workspace` 都過，`ci.py third-party` 對得上。
- **第 4 段之後的取樣**（使用者同意；`bench.py speedometer --browser kun`、`--sample --iterations 5`，4 分 20 秒，kun `97413117`；Chrome 那一側照舊用 Chromium 156 的樣本）：和第 3 段之後的取樣比，各類（20 個 suite 合計，每個 iteration 的 ms）：GC 62.0 → 52.7（4d 的配置）、JS 313.4 → 308.8、box layout 93.7 → 89.2、style 130.5 → 127.1、DOM 100.5 → 106.9、HTML parse 14.9 → 18.6，其他 ±2 ms；合計 1029.2 → 1015.1 ms，比值的幾何平均 1.465 → 1.448。DOM 和 HTML parse 多的在兩次取樣之間的起伏（±5 ms）上下，第 4 段沒有改它們的路。第 4 段的頁多是單一操作的迴圈（全域的 `document`、collection、shadow tree 的 retarget），Speedometer 的框架很少這樣用，blink-perf 的 2.27 → 1.55 倍在這裡只看得到 GC 那一類。第 5 段從這一次比。

### 5：GC（2026-11-06 到 07）

第一步（決定 6）原本是第 2 段，決定 7 時沒有 commit，2026-11-07 在 4d 做了（決定 15，量測在第 4 段那一節）：

- **改了什麼**（決定 6，存在 `git stash` 的「RFC 0085 GC allocation」）：型別的 size class 在編譯時算好（`TypeInfo::class`、`page::const_class_for`），「有沒有 JS 邊」也是，配置不再二分搜尋、不再查型別表；`refill`、`allocate_large`、`young_page` 標成 cold、不 inline；`publish` 的 finalizable bit 用一般的讀和寫（`Marks::set_finalizable_alone`，`crates/gc/soundness.md` 的 I6 補了同一頁只有一個 thread 寫的論證）。`gc` 的測試都過了（單元 69、整合 154），加了編譯時的 class 和搜尋的一樣的測試，debug build 每次配置比對；mutation test 那時沒有跑（4d 跑了，`mutants-4d.toml`）。
- **先找到的熱點**（`perf annotate`，jQuery 的取樣）：`Inner::allocate` 最熱的是 `page::class_for` 的二分搜尋（51 級），整個函式 inline 成 3.8 KiB；`publish` 六到七成在對頁的 bitmap 的 `lock or`。
- **量測的錯**：`bench.py` 的 `--against HEAD` 在 `../kun-against` 解析 `HEAD`，比到它停著的 `37520412`（報告卻寫成 repo 的 HEAD）；修好另外 commit（`56fdcf79`）。之前用 `--against HEAD` 的比較要存疑。
- **量到的**（HEAD `26ff0735` 對工作目錄，blink-perf 兩輪交錯）：`gc-forest` +3.8%、`gc-mini-tree` +9.2%、`gc-tree` +13.2%、`create-element` −0.5%；`calls` 只有 `createElement` 2317 → 2291，其他一樣。一次拿掉 cold、改回 atomic、改回搜尋，`gc-tree` 還是 +8% 到 +10%。
- **對照**：工作目錄編 HEAD 一樣的程式碼，三頁只差 −1% 到 +1.9%；HEAD 加一行沒有意義的程式（`std::hint::black_box(())`），三頁 +9.4%、+10.1%、+9.8%。`perf stat` 看 `gc-tree`：指令數 1118 億 → 1083 億（−3.1%），cycles +10%，L1 指令快取的 miss 11.2 億 → 19.7 億（+76%），L1 data、dTLB、iTLB 差不多，store forwarding 的衝突少 12%。
- **GC 那部分**（三頁在兩個 build 和 Chromium 156 上取樣，比例乘上沒取樣時的時間，每個 run 的 ms）：GC 35.4 → 22.5、40.8 → 19.4、35.9 → 25.7（其中配置 14 到 17 → 4 到 5）；bindings、DOM、JS、V8 API 都多了 3 到 11。Chrome 的 GC 13.0、18.4、16.1：marking 兩邊差不多（kun 4.4 到 8.8，Chrome 6.3 到 12.5），kun 多的是配置（Chrome 的大多 inline 在 Blink 裡、算成 DOM）和建 wrapper 時接上兩個 heap（`bindings::unified_heap::wrap`）、通知 GC job（`notify_concurrency_increase`、`max_concurrency`）。這次的 Chromium 取樣沒有讀到 V8 的 perf map（沒有一個 `JS:` 的 frame），Chrome 的 JS 算到了 V8 API，那兩欄不能比；原因沒查（`bench.py` 的 Speedometer 取樣讀得到）。
- **結論**：改動讓 GC 那部分變少，整頁在這台機器上被程式碼的排列蓋過；回來時從 README 的「第 5 段：GC」接著做。

**5a：資料跟著擁有者配在同一代**（2026-11-07，決定 17）

- **量到的**（暫時的探針：scavenge 搬的物件照型別數，`MACHOPO_GC_TYPES`，各 suite 10 個 iteration，沒有 commit）：搬的量 TodoMVC-jQuery 246 → 0.78 MB（scavenge 28 → 8 次）、React-Complex-DOM 13.7 → 2.2、NewsSite-Next 13.7 → 1.7、ES5 21.0 → 7.5。剩下的大多是短命的：事件的資料（`MouseEventData`、`AnimationEventData`）、事件路徑和暫時的 `HeapVector`、`MemberArray`，以及 `ElementAnimationsData`。NewsSite-Next 那一輪 GC 的暫停合計 10.5 → 2.8 ms，full GC 都是 1 次。時間（探針開著，各一次）：jQuery 的 CompletingAllItems 64.6 → 54.3 ms，其他差 1 ms 以內，只有 NewsSite-Next 的 NavigateToUS 兩次都比較慢（16.4 → 22.8、18.8 → 21.1），範圍很寬（94 到 131），之後不用 nursery 的那次是 17.7；5b 量 Speedometer 時再看。
- **測試**：`gc` 的 `young::objects_allocated_beside_their_owner_are_in_its_generation`、`a_vector_and_its_store_follow_their_generation`（`an_old_vectors_edges_keep_young_objects` 的註解跟著改：長大的 store 現在是 old）；`dom` 的 `gc::a_nodes_data_is_allocated_beside_it`（engine 驅動的 heap 裡建節點、設屬性、classes、`childNodes`、表單、parser 的 fast path、`Attr`、doctype、template，nursery 一個 byte 都沒有用）。這個測試找到了一開始漏掉的 `childNodes` 的 list、`Attr` 的資料和 document 的資料。`gc`、`dom`、`html`、`dom_api`、`dom_js` 的測試都過（90 秒）。
- **mutation test**（`mutants-5a.toml`，24 個）：24 個都抓到。`classList` 改成 `alloc_beside` 的那個沒抓到：它有 wrapper，在 engine 驅動的 heap 本來就配在 old，是等價的，所以改回 `alloc_in`，拿掉那個 mutant。沒有 mutant 的：CSSOM 寫回 `style` 屬性時新建屬性的那條路（`write_attribute_quietly`，和其他建屬性的地方同一行改法）。
- **`crates/gc/soundness.md`**：`young.rs` 的審查清單補上一開始就配在 old 的物件（`alloc_beside`、跟著 vector 的 store）照樣經過 `InitSlot::field`、`MemberArray::write` 的 barrier，marking 期間拿到的格子已經是黑的。

**5b：renderer 的 heap 不用 nursery**（2026-11-07，決定 18、19）

- **對照**（甲之後，暫時把 nursery 設成 0，各 suite 10 個 iteration、各一次）：時間都沒有變差；full GC 和甲一樣多。
- **記憶體**（暫時的探針，每次 GC 印 heap 的 used、committed 和 process 的 VmHWM）：甲、不用 nursery 的 full GC 次數一樣（jQuery 8、ES5 2、React-Complex-DOM 和 NewsSite-Next 1），used 的高峰一樣（jQuery 141.8 MB、React-Complex-DOM 36.3 和 34.9、NewsSite-Next 10.8 和 11.0、ES5 59.2 和 59.3），committed 的高峰 146.0 → 144.4、49.0 → 38.3、20.2 → 14.2、75.8 → 63.2 MB，VmHWM 差 −22 到 +3 MB。
- **取樣**（`bench.py speedometer --browser kun --sample --iterations 5`，甲 211 秒、不用 nursery 191 秒；乘的是第 4 段之後的時間）：GC 那一類 第 4 段之後 52.7 → 甲 36.4 → 不用 nursery 38.5 ms。
- **Speedometer**（`--against HEAD`：HEAD `0d5e4a0f` 是甲，工作目錄不用 nursery，交錯各 3 次，494 秒）：幾何平均 −0.3%（各 suite −3.7% 到 +3.6%，都在範圍裡），VmHWM −8.6% 到 +3.0%。甲那一側的 suite 合計約 1003 ms（第 4 段之後的計時 1015.1；不是同一次量的），TodoMVC-jQuery 130.4 → 121.7。5a 的 NewsSite-Next 的 NavigateToUS 的退步在這裡沒有出現（suite 合計 103.0、103.2，範圍 97 到 127）。
- **測試**：`dom_js` 的 `wrappers::the_doms_heap_has_no_nursery`（建元素、設屬性、加 listener、分派事件之後，nursery 是 0、young 沒有用）；`dom_js` 的測試都過（48 秒）。mutation test（`mutants-5b.toml`，1 個）：抓到。

### 收尾（2026-11-07）

- `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（9 秒）沒有問題；`cargo test --workspace` 59 個 binary、3935 個過、1 個 ignored（78 秒；輸出裡兩行 panic 是 `bindings` 的 `gc_platform` 刻意讓 worker panic 的測試）。
- **blink-perf 的 GC 四頁**（`--against 97413117`：第 4 段結束時的程式加上 V8 的 release；各一輪，345 秒）：`gc-forest` 100.4 → 101.2 ms（+0.8%）、`gc-mini-tree` 116.9 → 118.8（+1.6%）、`gc-tree` 134.7 → 142.8（+6.0%，兩邊的區間重疊）、`create-element` 2289 → 2292 runs/s，幾何平均 +2.0%：這幾頁的節點和 wrapper 本來就在 old，第 5 段省的節點資料在這裡不多。`gc-tree` 的 +6% 沒有查，在這頁的起伏裡；交審核時列出。`72259064` 不能比（`third_party.toml`、`.cargo/config.toml` 釘的 V8 不一樣，`--against` 不讓兩個版本共用 `../third_party`）。
- **最後的取樣和計時**：5b 的取樣（`--sample --iterations 5`）配上同一份程式碼的計時（`bench.py speedometer --browser kun`，51 秒）重算 `categories.md`：比值的幾何平均 1.432、合計 1005.0 ms，GC 37.3、DOM 108.7、JS 307.7、bindings 35.0、HTML parse 17.9 ms。`docs/performance-gaps.md` 的整體、各類、每個 suite、每一類最多的函式和可以開 RFC 的方向照這次改寫。

