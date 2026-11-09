# RFC 0066 的進度：Windows 上的取樣和綁核心

開始日期：2026-10-20。完成：2026-10-21，Windows 11（i9-13900K：8 顆 P-core、16 顆 E-core；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0066` 上做；第 2 段 commit 之後接著做第 3 段，三段做完之後一起 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型：samply 在 Windows 上錄到什麼（`prototype/sample.py`） | 完成 | 2026-10-20 |
| 2 綁核心：`fastest_windows_cpus`、`browser_affinity` | 完成 | 2026-10-20 |
| 3 接進工具：取樣、提權、Windows 的 DLL 的名稱、測試、文件 | 完成 | 2026-10-21 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **取樣** | |
| samply 在 Windows 上錄到什麼：thread 的名稱、`threadCPUDelta`、JS 的名稱、kun 的符號、時鐘 | 原型提權跑 TodoMVC-jQuery 三次（`details.md` 的「原型」） |
| Windows 上 samply 的指令、V8 的旗標、ETL 的名稱和刪除 | `SampleReadingTests` |
| 樣本的時間從 kernel ETL 的開頭起算 | `SampleReadingTests`（假的 ETL header）；原型裡用 trace 的 slice 對齊：最佳偏移在 1 ms 以內 |
| `perf` profile 的 exe 記 PDB 的絕對路徑（`--against` 的 target 也是） | `SampleReadingTests`；端到端的報告裡 kun 的函式都有名稱 |
| Windows 自己的 DLL 錄完之後命名 | `WindowsNamesTests`（真的 `samply load` 一次）；TodoMVC-jQuery 上手動確認（下面） |
| 報告寫明 Windows 怎麼取樣 | `ElevatedTests` |
| 真的經過 ETW 取樣、只取計時的區間 | `SampleTests`，以系統管理員跑 |
| **提權** | |
| 取樣的工作整份交給以系統管理員跑的一份自己、環境和全域變數、結果、失敗；系統管理員和其他平台時直接跑 | `ElevatedTests`（同一個 process 代替）；`CompareTests`（`--against` 的輪流） |
| 沒有 xperf 時停下 | `ElevatedTests` |
| UAC 只跳一次、輸出印在原本的終端機 | 端到端，手動（`run_elevated` 不在測試裡，`docs/test-gaps.md`） |
| **綁核心** | |
| Windows 上兩個瀏覽器在 efficiency class 最高的 CPU 上啟動，子 process 繼承，工具自己的 affinity 之後還原 | `CpuTests`；真的啟動 Chrome 和 kun，列出它們的子孫 process 的 affinity；blink-perf 的預設清單綁之前和之後各連跑兩次（下面） |

## 做過的檢查

2026-10-20 到 21，Windows 11。samply 0.13.1（`cargo install --locked samply`），Windows Performance Toolkit 的 xperf。

- **原型**：提權跑了三次（每次 UAC 一次）：第一次發現時鐘的換算錯了（samply 的 `startTime` 和樣本的基準差 +533 ms，第二次 −184 ms），第二次發現 kun 沒有符號（exe 裡只有 PDB 的檔名），第三次用帶 `/PDBALTPATH` 的 build 確認兩者都解決了；數字在 `details.md` 的「原型」。
- **綁和不綁**：`bench.py blink-perf` 的預設清單（10 頁），不綁（`browser_cpus` 換成 `None`）連跑兩次、綁在 0–15 連跑兩次。兩次之間每頁倍數的差：不綁時中位數 6%、最多 119%，10 頁裡 6 頁在 16% 以內；綁了之後中位數 3%、最多 23%，8 頁在 16% 以內。不綁時兩個瀏覽器常常整頁在 E-core 上：`bindings/create-element` 的 kun 95、99 runs/s，Chrome 1310、1392，綁了之後 kun 246、202，Chrome 3482、2779；倍數會跳是因為某一頁有一個剛好落在 P-core 上：`parser/query-selector-all-class` 的 kun 103 和 224（Chrome 都約 1710），倍數 16.7 和 7.6；`layout/flexbox-row-nowrap` 第一次 kun 4710、Chrome 2318，倍數 0.49，被標成「kun 快 2 倍以上」，第二次 1.02，綁了之後兩次都 1.03；`events/EventsDispatchingInV1ShadowTrees` 74.5 和 42.5，綁了之後 79.2 和 78.9。綁了之後還差最多的是 `css/PseudoClassSelectors`（8.78 和 7.12，kun 1989 和 2480 runs/s）和 `bindings/insert-before`（1.20 和 1.44）。綁了之後同一頁兩個瀏覽器的數值有時一起變兩成（`create-element`），倍數不變：0–15 是 8 顆 P-core 的 16 條 hyper-thread，頻率和同一顆核心上的另一條 thread 都會影響，沒有再細分。
- **affinity 的繼承**（手動）：真的啟動 headless Chrome 和 kun，列出它們的子孫 process：Chrome 的 11 個 process（GPU process、sandbox 裡的 renderer 和 utility process，都由 broker 建立）和 kun 的 affinity 都是 `0xffff`（P-core），工具自己之後是 `0xffffffff`。
- **Windows 的 DLL 的名稱**：給 `samply record` 加 `--windows-symbol-server`（第 9 條原本的做法）沒有用，DLL 在磁碟上時 samply 錄的時候不去 symbol server 找。用 `samply load` 的 symbolication API 查同一個 profile：DLL 的路徑在時一樣沒有名稱，拿掉路徑之後下載了 ntdll、vcruntime140 的 PDB，名稱都有了（`fun_32750` 是 `RtlpLowFragHeapAllocFromContext`，`fun_1cb40` 是 `memcpy_avx_ermsb_Intel`，`fun_1da00` 是 `memcmp`）。TodoMVC-jQuery 的 main thread 上沒有名稱的 486 個 frame（13 個 library）查到 453 個，剩下的是 AMD 的顯示卡驅動（`amdkmdag`，沒有公開的符號）：沒有名稱的 self time 從約 13% 變成 0.1%。第一次 48 秒（下載 ntkrnlmp 等的 PDB），之後 0.3 秒。kun 自己的符號也試過同一條路（拿掉 exe 的路徑、`--symbol-dir`）可以，但錄的時候就拿得到，沒有改。
- **端到端**：`bench.py speedometer --sample --suites TodoMVC-jQuery` 從一般權限的終端機跑兩次（替 DLL 補名稱之前和之後），build 完 UAC 各跳一次，提權的那份的輸出照樣印出來；`sample.md`（30 個計時區間，1937.7 和 1904.5 ms 的 CPU 時間）、folded、trace、profile 和符號留下，ETL 刪掉；第二次的報告裡沒有一個 `fun_`。
- **新的測試**：`blink_perf_test.py` 的 `CpuTests`；`speedometer_test.py` 的 `SampleReadingTests`（Windows 的部分）、`WindowsNamesTests`、`ElevatedTests`，`SampleTests` 在 Windows 上以系統管理員跑（`perf` profile 的 kun；PDB 把 trait 的 impl 寫成 `dom_js::host::impl$1::call()`，沒有 `V8Host`，斷言照平台比對）。Linux 的 `taskset` 測試原本在 Windows 上照真的平台走，改成固定在 `linux`、Chrome 的路徑換成假的（不然會去下載 linux64 的 Chrome for Testing）；macOS 的 samply 測試固定在 `darwin`。
- **mutation test**：`mutants-2.toml`（綁核心）19 條：第一次用了 `--keep-going`，漏了 3 條：不知道速度的 CPU 被略過（測試的 `[0, 40]` 拿掉 40 之後只剩一顆，照樣是 `None`）、其他種類的 entry 也當成 CPU set（假的 entry 全是 0，誤讀的結果又被真的蓋掉）、其他平台也改 affinity（沒有測試看）；補了 `[0, 16, 40]`、內容不是 0 的假 entry、Linux 上 affinity 一動就失敗的 mock 之後 19 條都抓到。`mutants-3.toml`（取樣、提權、DLL 的名稱）39 條：第一次漏了「JIT frame 也拿去查」（假資料的 JIT frame 都有名稱），補了一個只有位址的 JIT frame 之後 39 條都抓到（「`samply load` 沒關」是卡住、超過時限，也算抓到）。`sample_suites` 讀完刪 ETL 那一處只由以系統管理員跑的 `SampleTests` 守著，不在 spec 裡（`docs/test-gaps.md`）。
- **找到的問題**：
  - `speedometer_test.py`、`blink_perf_test.py` 和 `raster_test.py` 用 `open(os.devnull, "w")` 吞掉輸出，用的是系統的 locale 編碼：這台的 cp950 寫不進報告裡的 `−`，`MeasureTests.test_traces_of_both` 在中文 locale 的 Windows 上一直失敗（和這份的改動無關）。改成 `encoding="utf-8"`。
  - `CompareTests` 第一次在 Windows 上跑時走到了提權，跳了一次預期外的 UAC（沒有東西以系統管理員跑）。測試模組改成把 `run_elevated` 換成一要求就失敗的版本，要提權的測試用同一個 process 代替。
  - `symbolicate` 沒關 `samply load` 的 stdout（`SampleTests` 印出 ResourceWarning），補上，加一條 mutation。
- **跑過的檢查**：`speedometer_test.py` 98 個（2 個 skip：Windows 上的 Chrome 那幾組）、`blink_perf_test.py` 35 個、`raster_test.py` 9 個（1 個 skip）通過；`SampleTests` 以系統管理員通過；`sanity.py docs`、`tests` 通過；`cargo fmt --check`（連同 `sdk/lua/generator`）、clippy 通過；`cargo test --workspace` 45 個 binary、3398 個通過。這份沒有改 Rust 的程式。
