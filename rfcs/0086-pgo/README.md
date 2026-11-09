# RFC 0086：PGO：kun 的 Rust 和 V8 照 Chrome 的 official build 編

- 開始日期：2026-10-24
- 狀態：❌ 否決：2026-11-06（使用者決定不做；kun 的 Rust 和 V8 照現在的設定編，「已決定的事項」）
- 細節：無（量到的數字寫進 Speedometer 量測工具那份 RFC 的 `progress.md`，Chromium 的做法在「先例」）

## 摘要

Chrome 的 official build 是 ThinLTO 加 PGO（profile-guided optimization），V8 的 builtin 另外有 PGO；kun 的 `perf` profile 有 ThinLTO、沒有 PGO，kun 用的 V8 兩者都沒有。RFC 0085 量到 kun 在 V8 以外的每一類都比 Chrome 慢 1.2 到 6.8 倍、平均分散在每一行程式，PGO 正是作用在這種形狀的成本上。這份 RFC 分三部分：

1. **kun 的 Rust 開 PGO**：rustc 的 `-Cprofile-generate` 編插樁的 `perf` profile，跑 Speedometer 當訓練，`llvm-profdata merge`，再用 `-Cprofile-use` 編；先做一次實驗量比值的變化，有效再變成固定的流程。
2. **V8 照 official build 編**：`use_thin_lto`，和 Chromium 一樣的兩階段 PGO（`chrome_pgo_phase` 1 和 2），用 kun 跑 Speedometer 收 profile；要重編兩次 V8、換共用的 artifact。看第 1 部分的結果再決定。
3. **mimalloc 的 purge delay**：量 `MIMALLOC_PURGE_DELAY` 調大對 page fault 和時間的影響，決定預設值。

不做 V8 的 builtins PGO（要開 sandbox 和 external code space）。

## 動機

**現況**：

- `Cargo.toml` 的 `profile.perf`：繼承 release，`lto = "thin"`、`codegen-units = 1`、`debug = "line-tables-only"`。沒有 PGO；rustc 支援 PGO（`-Cprofile-generate`、`-Cprofile-use`，`llvm-tools` 元件的 `llvm-profdata`），kun 從來沒試過。
- V8 由 rusty_v8 的 fork 從原始碼編成 prebuilt 的靜態函式庫（`../third_party/rusty_v8_artifacts`），`build.rs` 的 gn 參數只有 `is_debug=false`、sandbox 和 pointer compression：沒有 `is_official_build`、`use_thin_lto`，`chrome_pgo_phase` 是 0。Chromium 的 PGO 設定（`build/config/compiler/pgo/`）在 fork 裡，只是沒開。
- mimalloc 是 kun 可執行檔的 Rust allocator（`crates/shell/src/main.rs`），`MIMALLOC_PURGE_DELAY` 預設 10 ms：釋放的記憶體 10 ms 後還給系統。
- RFC 0085 的量測（2026-10-24）：kun 的配置、複製和 page fault 比 Chrome 多很多，kernel 的 page fault 25 對 18 ms（差最多的 8 個 suite），`create_node`、`WordRuns::flush`、`fast_path::build`、`InlineContent` 底下都看得到 `KiPageFault`、`MiInitializePfn`；JS 那一類 kun 1.3 倍，V8 的 C++ runtime 每一項慢 1.2 到 1.5 倍（RFC 0058 查過是沒有 PGO 和 ThinLTO 對得上的範圍）。

**問題**：PGO 讓編譯器照實際執行的 profile 決定 inline、basic block 的順序和 register 的分配，對「每一行都慢一點、沒有熱點」的程式最有效；Chrome 有、kun 沒有，所以比較表裡的差有一部分是編譯設定而不是 kun 的程式。這部分不量出來，RFC 0085 其他方向的估計就偏高。

**目標**：

- 量出 Rust 的 PGO 對 20 個 suite 的比值的影響，有效（幾何平均至少降 3%）就讓 `bench.py speedometer` 量的 `perf` profile 固定帶 PGO，profile 的產生有固定的流程、不會悄悄過期。
- V8 的 ThinLTO 和 PGO：量出影響，決定要不要發新的 artifact。
- purge delay：決定預設值。

**要解決的 `docs/limitations.md` 的限制**：無。

**不在範圍內**：

- V8 的 builtins PGO（`v8_enable_builtins_optimization`）：要求 sandbox 和 external code space，kun 沒開；收益比 C++ 的小。
- Chromium 現成的 profdata：對應 Chromium 的 clang 版本和整個 chrome.dll 的編法，和 kun 的 V8 build 對不上，用不了。
- 用 PGO 以外的方法改善程式本身（RFC 0085 的其他方向）。

## 設計概要

對量效能的人：`bench.py speedometer` 照舊，它編的 `perf` profile 多一步「有 profile 就用」。對改程式的人：程式改了之後 profile 會過期（改過的函式沒有 profile，編譯器當它是冷的），所以 profile 要定期重新產生；哪裡產生、怎麼知道過期，是「待決定」的主題。對 V8 的部分：fork 的 release 多一種設定，artifact 跟著換，其他機器 `ci.py third-party` 同步時會拿到新的。

## 詳細設計

### 第 1 部分：kun 的 Rust

1. **插樁的 build**：`RUSTFLAGS="-Cprofile-generate=<dir>"` 編 `perf` profile 到另一個 target 目錄（`target/pgo-gen/`），不污染 `target/perf/`。插樁的 binary 會慢，不拿它的時間。
2. **訓練**：用插樁的 kun 跑 `bench.py speedometer --browser kun`（20 個預設的 suite，和 Chrome 的訓練 workload 一致：Chromium 的 `generate_profile.py` 一定跑 Speedometer 3）。每個 process 寫一份 `.profraw`（`LLVM_PROFILE_FILE` 的 `%m`、`%p`）。要不要加 blink-perf 的頁或其他 workload，看第一次的結果。
3. **合併**：rustc 自帶的 `llvm-profdata merge -o kun.profdata <dir>/*.profraw`（`rustup component add llvm-tools`；版本要和 rustc 的 LLVM 一致，1.99.0 是 LLVM 23.1）。
4. **使用**：`RUSTFLAGS="-Cprofile-use=<kun.profdata> -Cllvm-args=-pgo-warn-missing-function"` 編 `perf` profile。和 ThinLTO 相容。mimalloc 和其他 C 的部分（cc crate 編的）不在 rustc 的 PGO 裡，不管。
5. **量**：`bench.py speedometer` 20 個 suite 各 10 個 iteration，和沒有 PGO 的 `perf` 比（`--against` 現在比的是兩個 commit，要能比兩個 build 設定，或分兩次量再比）；另外 `--sample` 一次看每一類各少多少。
6. **變成流程**（有效才做）：
   - 甲、每次 `bench.py speedometer` 量之前都重新產生：最準，但每次多一次插樁的 build 加一輪訓練（約 15 到 20 分鐘）。
   - 乙、`ci.py pgo` 產生 `target/pgo/kun.profdata`，`bench.py` 有它就用、沒有就不用並印出來；CI 每天產生一次，開發機要的時候跑。profile 和它對應的 commit 一起記，差太遠時 `bench.py` 警告。
   - 丙、profdata commit 進 repo：Chromium 不這麼做（用 GCS 加 `.pgo.txt` 指向），檔案幾十 MB 又常換，不選。
   - 丁、放在 `../third_party/rusty_v8_artifacts` 那種共用的地方：profile 跟 kun 的 commit 走，不像 V8 的 artifact 那麼穩定，不選。
   - 2026-10-24 的看法：乙。

### 第 2 部分：V8

1. `build.rs` 的 gn 參數加 `use_thin_lto=true`（Chromium 的 official build 有；`is_official_build=true` 會一起拉進其他東西，例如 `symbol_level`、`dcheck_always_on` 的預設和 `chrome_pgo_phase=2`，要逐一看）。先只加 ThinLTO 量一次。
2. PGO：`chrome_pgo_phase=1` 編一次（clang 的 `-fprofile-generate`），用連結它的 kun 跑 Speedometer 收 `.profraw`，Chromium 的 clang 自帶的 `llvm-profdata` 合併，`chrome_pgo_phase=2 pgo_data_path=<profdata>` 編一次。Chromium 的 `pgo.gni` 寫明 profile 要從 `dcheck_always_on=false` 的 build 產生。
3. 每次從原始碼編 V8 約 40 分鐘（`ci.py v8`），兩階段兩次；fork 要發新的 release（`kun-v152.2.0-<n+1>`），`third_party.toml` 跟著更新，其他機器會換 artifact。profile 的過期問題比 Rust 的小：V8 的版本不常動。
4. 要不要做，看第 1 部分的結果和 RFC 0058 估的 20 到 40 ms 值不值得這個 build 時間。

### 第 3 部分：mimalloc

`MIMALLOC_PURGE_DELAY` 設 10（現在）、100、1000 和 −1（不還）各量一次 20 個 suite，看時間和 kernel 的 page fault（`--sample`）以及記憶體的峰值（`bench.py` 有量）；選一個預設值。

### 測試

- 正確性不受影響：PGO 和 purge delay 不改程式的語意。`cargo test` 照舊用 dev，不帶 PGO。
- `bench.py speedometer --sample` 照舊能命名函式（`debug = "line-tables-only"` 不變）。
- 流程的測試（第 6 步的乙）：`bench.py` 的測試裡，有 profdata 時 `RUSTFLAGS` 帶 `-Cprofile-use`，沒有時不帶並印出；profile 的 commit 和現在差太遠時警告。
- 數字寫進 Speedometer 量測工具那份 RFC 的 `progress.md`。

### 分階段

1. Rust 的 PGO 實驗：手動跑一次第 1 部分的第 1 到 5 步，量比值和每一類的變化。半天到一天。
2. 有效的話：第 6 步的流程（`ci.py pgo`、`bench.py` 用它、CI 每天產生）。
3. purge delay 的量測和預設值。
4. V8：先 ThinLTO，再 PGO；每一步量一次。

## 缺點

- Profile 會過期：改程式之後 `perf` profile 的數字會慢慢變差，直到重新產生；要靠流程（第 6 步）和警告維持。
- 兩個 build 設定（有、沒有 PGO）的數字不能直接比：比較表和 `progress.md` 的紀錄要寫明用的是哪一種。
- V8 的部分每次 40 分鐘乘二、換 artifact，所有機器都受影響。
- PGO 的收益隨訓練的 workload 走：用 Speedometer 訓練會偏向它，其他頁面不一定一樣快（Chrome 也是這樣訓練的，所以比較是公平的）。

## 理由和其他做法

- **為什麼先做 Rust 不先做 V8**：Rust 的部分只動 flag 和一支腳本、半天知道結果，而且它改善的是 RFC 0085 量到差 1.6 到 2.8 倍的那幾類（DOM、style、layout），正好是 kun 自己的程式；V8 的部分貴，先看值不值得。
- **為什麼不用 Chromium 的 profdata**：見「不在範圍內」。
- **為什麼不做 builtins PGO**：見「不在範圍內」。
- **不做會怎樣**：比較表裡有一塊是編譯設定的差，RFC 0085 的其他方向的估計會偏高，也少一個便宜的、每個 suite 都受惠的改善。

## 先例

- **Chromium**（PGO 的文件 pgo.md、`tools/pgo/generate_profile.py`、`build/config/compiler/pgo/`）：`chrome_pgo_phase` 0、1、2；phase 1 是 `is_official_build=true`、`symbol_level=0`、`-fprofile-generate`；訓練用 crossbench 跑 Speedometer 3（Apple Silicon 上重複 3 次、權重 5）和 JetStream 3，桌面非 Linux 加 `system_health.common_<platform>` 的精簡版，macOS 和 Android 加 MotionMark；插樁的瀏覽器用 `LLVM_PROFILE_FILE=<dir>/default-%2m.profraw`，關掉 spare renderer 和 hang watcher；`llvm-profdata merge`（`--weighted-input`）；phase 2 是 `-fprofile-use=<profdata>`、`-Wno-profile-instr-unprofiled`、`-Wno-profile-instr-out-of-date`，加 linker 的 `-mllvm:-enable-ext-tsp-block-placement=1`，和 ThinLTO 一起；專門的 bot 產生、上傳到 GCS，`chrome/build/<platform>.pgo.txt` 指向現在用的那份，`update_pgo_profiles.py` 在 gn 時下載。PGO 的設定只給 clang 的 `cflags`，Chromium 裡的 Rust 沒有 PGO。
- **V8 的 builtins PGO**（`tools/builtins-pgo/`）：`v8_enable_builtins_profiling` 的 build 跑 benchmark 收 branch hint，存成 `profiles/x64.profile`，編 builtin 時當提示；`v8_enable_builtins_optimization` 預設只在 `build_with_chromium && chrome_pgo_phase == 2` 時開。
- **Firefox**：PGO 加 LTO 的 official build，訓練用它自己的頁面集（`build/pgo/`）。
- **Rust**：rustc 的 PGO（`-Cprofile-generate` / `-Cprofile-use`）是 rustc 自己 official build 用的做法。

## 已決定的事項

1. 2026-11-06，使用者決定不做這份 RFC：kun 的 `perf` profile 照舊是 ThinLTO、沒有 PGO，V8 照現在的設定編。換 DOM 之後兩邊照同樣的方法取樣（`rfcs/0085-optimization-roadmap/details.md` 的「換 DOM 之後的分析（2026-11-06）」），JS 那一類 kun 是 Chrome 的 1.4 倍；RFC 0085 的 JS 那一段只比 V8 的設定（flag、tiering），不重編 V8。

## 待決定

1. Profile 放哪裡、怎麼知道過期（第 1 部分的第 6 步，2026-10-24 的看法：乙）。
2. 訓練的 workload 只用 Speedometer，還是加 blink-perf 的頁。
3. V8 的部分做不做，等第 1 階段的數字。
4. `--against` 要不要能比兩個 build 設定。

## 之後的可能

- 其他的編譯設定：`-C target-cpu`（比較表的機器固定時）、`panic = "abort"`、Rust 的 `-Zbranch-protection` 以外的 LLVM 選項（例如 `-enable-ext-tsp-block-placement`）。
- V8 跟上 Chrome 的版本，讓 JS 那一類能直接比。
