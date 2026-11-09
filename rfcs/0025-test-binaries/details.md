# RFC 0025 的細節

## 現有的測試

草稿時，2026-10-09 的 `main`（`2cc70fa`）。每個 `crates/*/tests/*.rs` 是一個測試 binary；「V8」是那個 crate 的測試 binary 有沒有靜態連結 V8（大於 40 MB）。實作開始時（2026-10-10 的 `main`，`5bd761e1`）多了一些：`bindings` 的 `promise_resolvers`，`dom_api` 的 `forms`、`headers`，`dom_js` 的 `console`、`editing`、`fetch`、`html_element`、`messaging`、`user_activation`，`layout` 的 `position`，`protocol` 的 `network`，`shell` 的 `fetch`、`forms`、`messaging`、`speedometer`（`harness = false`）、`wasm_reftests`；`shell` 的 `reftest`、`wpt_testharness` 已經拿掉（改由 wptrunner 跑），一共 135 個整合測試檔、159 個測試 binary（140 個連結 V8）。

| crate | 整合測試的檔案 | V8 |
| --- | --- | --- |
| `bindings` | `cppgc_member`、`cppgc_traced`、`cppgc_traced_marking`、`isolate_teardown`、`modules`、`v8_smoke`（6） | 有 |
| `browser` | `navigation`（1） | 有 |
| `dom` | `abort`、`collections`、`compile_fail`、`computed_style`、`cross_isolate`、`detached_marking`、`events`、`focus`、`foreign`、`gc_stress`、`history`、`incremental_stress`、`layout_client`、`layout_tree_flags`、`listeners`、`location`、`mutation_observers`、`navigator`、`performance`、`readme_example`、`script`、`script_objects`、`standalone_heap`、`style_flags`、`timers`、`traversal`、`tree`、`url`（28；`crates/dom/tests/common/` 是 24 個檔案共用的 helper，`crates/dom/tests/ui/` 是 `compile_fail` 的） | 有（`compile_fail` 沒有） |
| `dom_api` | `api`、`computed_style`、`dataset`、`dictionary`、`events`、`focus`、`frames`、`geometry`、`history`、`location`、`markup`、`navigator`、`performance`、`records`、`spec`、`title`、`ui_events`、`url`、`url_data`、`wit`（20） | 有 |
| `dom_js` | `abort`、`bindings`、`computed_style`、`dispatch`、`dynamic_import`、`errors`、`gc_stress`、`geometry`、`iframes`、`layout_client`、`modules`、`mutation_observers`、`navigator`、`realms`、`teardown_stress`、`test_runner`、`traversal`（17） | 有 |
| `dom_wasm` | `engine`（1） | 有 |
| `html` | `parse`、`parser`、`serialize`、`tree_construction`（4） | 有 |
| `layout` | `block`、`flex`、`grid`、`inline`、`inline_block`、`parley_inline_box`、`update`（7） | 有 |
| `net` | `load`（1） | 沒有 |
| `paint` | `artifact`、`child_frames`、`geometry`、`hit_test`、`layerize`、`paint_controller`、`raster_invalidation`（7） | 有 |
| `protocol` | `automation`、`boundary`（2） | 沒有 |
| `shell` | `automation_pipe`、`browser_view`、`focus`、`geometry`、`history`、`inserted_scripts`、`keys`、`modules`、`mouse`、`navigator`、`network`、`performance`、`reftest`、`scenarios`、`script`、`script_languages`、`sites`、`standalone_heap`、`wasm_scripts`、`webdriver`、`wheel`、`windowless`、`wpt_testharness`（23；`reftest`、`scenarios`、`wpt_testharness` 是 `harness = false`） | 有 |
| `style` | `cascade`、`computed`、`cssom`、`invalidation`（4） | 有 |

合計 121 個整合測試的 binary，加上 24 個 crate 的 lib 的單元測試（其中 12 個連結 V8），`cargo test --workspace` 一共 145 個 binary，127 個連結 V8。`layout`、`paint`、`style`、`html` 不直接用 V8，但依賴 `dom`，`dom` 依賴 `v8` 和 `bindings`，所以照樣連結。

## V8 的 flag（草稿時）

`bindings::initialize_with_flags(flags)` 只有一個 process 裡第一次的呼叫生效（`OnceLock`），之後的呼叫什麼都不做；`MACHOPO_V8_FLAGS` 接在後面，`PINNED_FLAGS` 最後。`crates/dom/tests/common/mod.rs` 的 `new_dom(flags)` 也走這裡，它的註解寫明「V8 flags are process-wide and set once, so each test binary passes its own」。現在各測試檔帶的 flag：

| flag | 測試檔 |
| --- | --- |
| `--expose-gc` | `bindings` 的 `cppgc_traced`、`modules`、`v8_smoke`；`dom` 的大多數（`const FLAGS: &str = "--expose-gc"`）；`dom_js` 的 `bindings`、`dispatch`、`dynamic_import`、`gc_stress`、`iframes`、`modules`、`realms` |
| 不帶（`bindings::initialize()`、`new_dom("")`，或經由 `DomIsolate::new` 初始化） | `bindings` 的 `cppgc_member`；`dom` 的 `foreign`、`cross_isolate`、`detached_marking`、`layout_client`；`dom_js` 的 `abort`、`computed_style`、`errors`、`geometry`、`layout_client`、`mutation_observers`、`navigator`、`test_runner`、`traversal`；`shell` 等其他 crate 的全部 |
| `--stress-incremental-marking` | `bindings` 的 `cppgc_traced_marking`、`dom` 的 `incremental_stress` |
| `--stress-compaction` | `bindings` 的 `isolate_teardown` |
| `--expose-gc --stress-compaction` | `dom_js` 的 `teardown_stress` |

`--expose-gc` 只讓 JS 看得到 `gc()`，不改 GC 的時機和做法；stress 的兩個改的是整個 heap 的 GC（每次配置都推進 incremental marking、每次 GC 都 compact）。

測試沒有用 `std::env::set_var`，也沒有裝全域的 tracing subscriber：除了 V8 的 flag，沒找到其他整個 process 只設一次的狀態（第 0 步合併 `dom` 時再確認一次）。

## 合併之後

2026-10-10，`0025` 做完時：

| crate | 合併前（實作開始時） | 之後 |
| --- | --- | --- |
| `bindings` | 7 | 3：`it`、`cppgc_traced_marking`、`isolate_teardown` |
| `browser` | 1 | 1：`it` |
| `dom` | 28 | 3：`it`、`incremental_stress`、`compile_fail` |
| `dom_api` | 22 | 1：`it` |
| `dom_js` | 23 | 2：`it`、`teardown_stress` |
| `dom_wasm` | 1 | 1：`it` |
| `html` | 4 | 1：`it` |
| `layout` | 8 | 1：`it` |
| `net` | 1 | 1：`it` |
| `paint` | 7 | 1：`it` |
| `protocol` | 3 | 1：`it` |
| `shell` | 26 | 3：`it`、`scenarios`、`speedometer` |
| `style` | 4 | 1：`it` |
| 合計 | 135 | 20 |

加上 24 個 crate 的 lib 的單元測試，`cargo test --workspace --no-run` 列出的測試 binary 從 159 個變成 44 個，大於 40 MB（連結 V8）的從 140 個變成 30 個，合計的大小從 8.0 GB 變成 1.7 GB。

## 要改的指令

草稿時（2026-10-09）`rfcs/` 以外寫了 `--test <名稱>` 的有 33 處，其中 11 處指向留成獨立 binary 的（`compile_fail`、`scenarios`、`cppgc_traced_marking` 等）。第 1 段用一個腳本改：`rfcs/` 以外的文件、程式碼註解、腳本和黃金檔的開頭，cargo test -p <crate> --test <名稱> 改成 --test it <名稱>::，crates/<crate>/tests/<名稱>.rs（和 crate 自己的文件、註解裡的 tests/<名稱>.rs）改成 tests/it/ 底下的路徑，只換已經搬進 tests/it/ 的名稱。`dom_api` 的 `spec-missing.txt`、`url-expected-failures.txt` 的開頭是測試寫出來的，改了測試裡的字串，再用 `MACHOPO_UPDATE_GOLDEN` 重新產生（內容沒變）。

`sanity.py docs` 檢查 `cargo test` 指令裡 `--test` 後面的名稱是那個 crate 的測試 target（第 1 段起認得 tests/it/main.rs 這種目錄形式的 target），所以改漏的會失敗；它不檢查 `it` 後面的模組名稱（libtest 的篩選，寫錯只會跑零個測試）。這一項之後可以補：檢查 `--test it <模組>::` 的模組在 tests/it/ 底下。

`rfcs/` 裡：已完成的 RFC 和 `rfcs/pre-rfcs/` 的 487 處不改，`sanity.py docs` 不再檢查它們的路徑和測試指令（README 的第 14 條）；還沒完成的 RFC 用同一個腳本改。

## 其他做法

### B. 整個 workspace 一個測試的 crate

`tests/`（workspace 的根目錄）一個 package，依賴所有 crate，所有整合測試是它的模組。

- 好處：整個 workspace 的整合測試只 link 一次（加上 stress 和 `harness = false` 的那幾個）。
- 壞處：
  - 改 `dom` 時要重編所有 crate 的整合測試（現在已經是這樣，因為大多數 crate 依賴 `dom`），但改 `shell` 時也要重編 `dom` 的整合測試（現在不用）。
  - 測試離開它測的 crate：`cargo test -p dom` 不再跑 `dom` 的整合測試，crate 的 README 的測試清單要指向別的地方。
  - `test.py matrix` 的 `GC_TEST_ARGS` 用 `-p dom -p bindings …` 挑 GC 相關的 crate，要改成篩選模組。
  - 依賴集中：測試用的 dev-dependency 全部堆在一個 crate。
- 和 A 的差距：A 之後約 30 個 V8 binary，B 約 15 個；重編的時間主要看改了哪一層，A 已經去掉大部分的 link。

### C. 搬進 `src/` 當單元測試

每個 crate 已經有一個 lib 的單元測試 binary，整合測試搬進去就不多任何 binary。

- 壞處：
  - 測試看得到 crate 私有的東西，不再保證只經過公開的 API；`dom` 的測試有些要證明的正是「用公開的 API 做不到」（GC 的規則、`DomAccess` 的唯一性；`compile_fail` 是另一種形式的同一件事）。
  - 改一個測試要重編整個 crate 的 test 設定（實作連同所有測試），整合測試現在只重編測試自己。

### D. cargo-nextest

每個測試一個 process，binary 之間也平行。

- 好處：隔離回來（一個 abort 只影響一個測試），V8 的 flag 不再衝突（每個 process 自己初始化），`cargo test --workspace` 的執行時間也會下降（binary 平行）。
- 壞處：不減少 binary 的數量，link 的成本照舊；多一個第三方工具，開發的機器都要裝；`harness = false` 的 binary 要實作 nextest 的 `--list` 格式，或排除在外。
- 和 A 不衝突：A 之後再用 nextest，隔離和 flag 的問題都消失（stress 的測試甚至可以合進 `it`，用 nextest 的設定給它們自己的環境變數）。

## 參考資料

- Chromium 的測試 launcher：`base/test/launcher/`（`--test-launcher-batch-limit`、每個 browser test 一個子 process）。
- V8 的 `tools/run-tests.py`：每個 cctest 一個 process，`--extra-flags`、每個測試的 flag。
- matklad，〈Delete Cargo Integration Tests〉，2021：<https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html>。
