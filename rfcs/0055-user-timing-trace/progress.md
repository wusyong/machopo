# RFC 0055 的進度：User Timing 寫進 trace，Speedometer 的計時區間照 runner 的 mark

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-18。完成：2026-10-18，macOS（Apple M4，Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0055` 上做，三段一起審核。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 fork：`blink.user_timing`、`Mark`、`Measure`，重編 V8，發 release，換 pin 和 tag | 完成（`531a3ae`、`kun-v152.2.0-10`；第 2 段找到的 track 撞號在 `aa29a24`、`kun-v152.2.0-11` 修好） | 2026-10-18 |
| 2 kun 寫事件：`bindings::trace`、`dom_api`、`shell::trace` | 完成 | 2026-10-18 |
| 3 工具：計時區間照 mark、沒有 mark 時照 frame，Chrome 錄 `blink.user_timing`，假的 runner 打 mark；20 個 suite 的驗收 | 完成 | 2026-10-18 |

## 驗收

### trace 裡的 User Timing（fork、`bindings`、`dom_api`、`shell`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `performance.mark()` 和 `measure()` 把 entry 放進 buffer 之後送一個 `blink.user_timing` 的 `tracing` 事件：名稱，和 entry 的時間離現在多久（ns，用 frame 沒有 coarsen 的時鐘，未來是負的）；`new PerformanceMark()` 和出錯的不送 | `cargo test -p dom_api --test it -- performance::marks_and_measures_are_sent_for_the_trace` |
| `shell` 的 trace layer 收 span 和 `blink.user_timing` 的事件，事件變成 `bindings::trace` 的 `mark`、`measure`；其他 target 的事件和 log 不進 trace | `cargo test -p shell --lib -- trace::` |
| `MACHOPO_TRACE` 裡網頁的 mark 是 `CrRendererMain` 的 track 上的 instant、measure 各自一條 track（parent 是那個 thread 的 track，不撞別的 thread 的 track），時間是 entry 的時間、measure 的長度精確；建構子的 mark 不在 | `cargo test -p shell --test it -- trace::` |
| fork 的 `mark`、`measure` 在過去、未來的時間，同名重疊的 measure，結束在開始之前的 measure，另一個 thread 上的事件，沒錄 `blink.user_timing` 時不寫 | 第 1 段手動（「做過的檢查」）；之後由上一條守 |

### 計時區間（`scripts/speedometer.py`）

| 項目 | 怎麼驗證 |
| --- | --- |
| 照 runner 的 mark：每個 `-async-end` 配它之前最近、還沒配過的同名 `-start`，亂序的照時間排，runner 的其他 mark 不算；沒有能配成區間的 mark 時照 runner 的 frame（原型的測試），回傳用的是哪一種 | `python3 scripts/speedometer_test.py SampleReadingTests` |
| `perfetto_marks` 只給 `blink.user_timing` 的 instant（分類是 interned 的 iid 或字串），不給別的分類的 instant 和 user timing 的 slice | `python3 scripts/speedometer_test.py SampleReadingTests.test_marks_are_user_timings_instants` |
| `sample.md` 每個 suite 寫明區間是照什麼找的；`compare.md` 寫兩邊各用哪一種 | `python3 scripts/speedometer_test.py SampleReadingTests.test_the_report_has_both_events CompareTests.test_the_versions_take_turns` |
| 假的 runner 的 trace：照 mark 找的區間一個 test 一個、等於 runner 回報的 sync + async（差在 0.15 ms 內）；measure 在自己的 track 上，區間等於兩個 measure 的長度；Chrome 的 trace 有 runner 的 mark 的 instant | `python3 scripts/speedometer_test.py KunTests.test_the_windows_are_the_runners_times MeasureTests.test_traces_of_both` |
| `--sample` 的報告寫照 mark 找（Linux、有 `perf` 時才跑） | `python3 scripts/speedometer_test.py SampleTests` |
| 20 個 default suite 的 kun trace（各 3 個 iteration）：照 mark 找的每個區間和 runner 回報的 sync + async 一致 | 2026-10-18 跑過一次（「做過的檢查」），平常不跑 |

## 做過的檢查

2026-10-18，macOS（Apple M4）。

- **對照 Chrome for Testing 152**（`details.md` 的「Chrome 的 user timing 事件」）：mark 和 measure 用 entry 的時間、mark 在 renderer main thread 的 track 上、每個 measure 一條 track（parent 是 thread 的 track）、負的和未來的時間照寫、`new PerformanceMark()` 不寫。
- **fork**：`ci.py v8` 第一次 debug 1470 秒、release 1286 秒，之後的修正增量約 10 秒。fork 自己的 toolchain（1.91.0）`cargo check` 和 `rustfmt --check` 通過。第 1 段沒有加測試（「已決定的事項」第 10 條），用 scratchpad 的小程式連本機的 V8 錄 trace、用 `protobuf_fields` 讀回：mark 的時間差 −50、+1000 ms，measure 長度 10、15、−10、1 ms，另一個 thread 的掛在它底下，只錄 `kun` 時沒有 user timing 的事件；debug 和 release 的 V8 都一樣。
- **找到的 bug**：
  - 第一版 `Measure` 的開始和結束各取一次 Perfetto 的現在，結束晚了寫 begin 事件的時間（debug 的 V8 上 10 ms 的 measure 變成 10.055 ms）；改成一次呼叫只取一次。第 1 段的小程式找到。
  - measure 的 track 的 id 從 1 數，track uuid 是 id 和 parent 的 xor，thread 的 track 只差 thread id 那幾個 bit：第一個 measure 的 track 就是 compositor thread 的 track。第 1 段的小程式剛好沒撞到（只有兩個 thread），第 2 段的端到端測試找到；id 改成 splitmix64 打散，fork 發 `kun-v152.2.0-11`。
- **不穩定的測試**（寫的時候就修掉）：`dom_api` 的事件測試起先用 `tracing::subscriber::with_default`，單獨跑通過，和同一個 binary 的其他測試平行跑時收不到事件：別的 thread 同時註冊 callsite 時 `tracing` 重算 interest 和最大等級，沒算到這個 thread 自己的 subscriber。改成整個 binary 一個全域的 subscriber（只開這個 target），照 thread 分開收。
- **mutation test**，每條各自弄壞一處（`--keep-going`），跑完檔案都還原（`target/mutate/originals/` 不在，fork 沒有改動）：
  - 第 2 段（`RUSTY_V8_ARCHIVE=… python3 scripts/test.py mutate rfcs/0055-user-timing-trace/mutants-2.toml`，21 條）全部抓到。`dom_api` 的 9 條（用 coarsen 過的時鐘、時間的正負、measure 的兩端、`mark()` 或 `measure()` 不送、建構子的也送、只在沒有東西收時送、mark 的 target）由 `dom_api` 的事件測試抓到；`shell` 的 layer 的 5 條、`bindings` 的 3 條和 fork 的 C++ 的 4 條（id 從 1 數、measure 在 thread 的 track 上、時間不管多久以前、measure 的結束另取一次現在）由 layer 的單元測試或端到端測試抓到。
  - **工具的坑**：fork 的 mutation 的測試指令要重編 V8（再碰一下 `BUILD.gn`，`v8` 的 build script 才重新複製 archive），第一次跑時有兩條其實是 V8 編不過（`-Werror` 的沒用到的變數、函式），卻被算成抓到（只花 4 秒露了餡）。測試指令改成 V8 編不過時 `exit 0`（編不過的會變成沒抓到），三條改寫成編得過的，4 條重跑都抓到（各約 16 秒）。跑完用還原的原始碼重編一次 V8，共用的 artifact 不留 mutation 的版本。
  - 第 3 段（`python3 scripts/test.py mutate rfcs/0055-user-timing-trace/mutants-3.toml`，16 條）：第一次沒抓到「Chrome 不錄 `blink.user_timing`」：測試在 Chrome 的 trace 的 bytes 裡找 mark 的名稱，但不錄這個分類時名稱也在（DevTools 的事件的參數）。改成找 `blink.user_timing` 的 instant 之後抓到。其他 15 條（區間從第一個 start 算、一個 start 配兩次、不照時間排、在 sync 的結束停、有 mark 就不照 frame、任何分類的 instant、slice 的開始當成 mark、interned 的或字串的分類沒讀、instant 當成 slice、報告不寫用哪一種、`compare.md` 只記一邊、假的 runner 沒有結束的 mark、開始的 mark 在 origin、沒有 measure）都抓到。
  - 等價的一條記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」：送事件前不先看 `tracing::enabled!`。
- **20 個 suite 的驗收**（`scratchpad` 的腳本：每個 default suite 用 `run_in_kun` 跑一次 `perf` profile 的 kun，3 個 iteration，同時拿 trace 和 runner 回報的每個 test 的時間；用電池跑，比的是同一次執行裡的兩個量，不是快慢；184 秒）：20 個都有效、都照 mark 找，區間數等於 test 數（各 9 個，兩個 Editor 6 個）；每個區間減 runner 的 sync + async，中位數 0.000 ms、範圍 −0.013 到 +0.200 ms（`performance.now()` coarsen 的一兩格）；合計 4587.6 ms 對 runner 的 4584.8 ms。照 frame 找的同一批 trace 在 React-Stockcharts-SVG 比 runner 少 23.2 ms（215.2 對 238.4），其他 suite 差在 1.1 ms 內。
- **完成時的驗證**（不設 `RUSTY_V8_ARCHIVE`，連 `kun-v152.2.0-11` 的 prebuilt；Windows 的 prebuilt 那時還在 fork 的 CI 裡，由使用者處理）：`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs` 通過；`cargo fmt --check`（連同 `sdk/lua/generator`）通過；`cargo clippy --workspace --all-targets -- -D warnings` 通過；`cargo test --workspace --no-fail-fast` 3167 個通過、0 個失敗（包括修好的 `select_type`）；`python3 scripts/speedometer_test.py` 66 個通過、2 個 skip（只在 Linux 跑的 `SampleTests` 等）。
- **和這份 RFC 無關的失敗，一起修掉**：`dom_api` 的 `select_type::a_menu_lists_keys` 在 macOS 上每次都失敗（`main` 的 `6d47b962` 也是）。原因在測試：迴圈要測「另一個」修飾鍵，`(false, false, !cfg!(target_os = "macos"))` 在 macOS 上變成沒有修飾鍵；改成不分平台列出 Shift、Ctrl、Meta（和這份 RFC 同一個 commit）。mutation（menu list 的方向鍵不看 Ctrl、不看 Meta）兩條都抓到，修之前 macOS 上抓不到不看 Ctrl 的那條。
