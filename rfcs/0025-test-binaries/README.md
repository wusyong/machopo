# RFC 0025：每個 crate 一個整合測試的 binary

- 開始日期：2026-10-09
- 狀態：✅ 已完成：2026-10-10（「已決定的事項」第 1 到 21 條在各段開始前和實作時定下；只在 Windows 11 上做、量；`test.py matrix`、`fuzz` 之後在 macOS 上跑過、都通過（第 13 條）；驗收在 `progress.md`）
- 細節：`details.md`
- 進度：`progress.md`（每次跑測試的時間的摘要在它的最後）

## 摘要

`crates/*/tests/*.rs`（每個檔案原本是一個獨立的測試 binary）合併成每個 crate 一個整合測試的 binary：tests/it/main.rs，原本的檔案變成它的模組。不能和其他測試共用 process 的留成獨立的 binary：自己有 `main` 的（`harness = false`：`shell` 的 `scenarios`、`speedometer`）、trybuild 的 `compile_fail`，和 V8 的 flag 不同的 stress 測試（V8 的 flag 整個 process 只設一次）。`dom`、`dom_js`、`bindings` 的合併的 binary 在 `main` 之前用 `bindings::initialize_v8_before_main!` 設 `--expose-gc`，flag 不取決於哪個測試先跑；`sanity.py tests` 守住「新的整合測試是 tests/it/ 的模組」。workspace 的測試 binary 從 159 個（140 個連結 V8）變成 44 個（30 個），改 `dom` 之後重編整個 workspace 的測試從 38.7 秒變成 13.8 秒，跑整個 workspace 的測試從 34.3 秒變成 21.3 秒（Windows 11、rust-lld，3 次的中位數）。

## 動機

**開始時（2026-10-09，Windows 11，32 個 thread）**：`cargo test --workspace` 有 145 個測試 binary（實作開始時 159 個），其中 127 個（大於 40 MB）靜態連結 V8 和 Skia：除了 `bindings`、`dom`、`dom_js`，`layout`、`paint`、`style` 也經由 `dom` 連結了 V8。cargo 把 `tests/` 底下每個檔案編成一個獨立的 binary，所以改了 `dom`（或更底層的 crate），每個整合測試的檔案都要重新 link 一次：

- 改 `dom` 之後 `cargo test --workspace --no-run` 約 29 秒（已經只留行號表、用 rust-lld link；之前是 58 秒），大部分是 link。實作開始時（159 個 binary，改 `dom` 的一個 `expect` 的字串，第 3 條）是 35.9 秒；最後在 `main` 的 worktree 和合併後在同一個時段量時是 38.7 秒（`progress.md`）。
- 一份 RFC 的 mutation test 有 71 到 228 條（`docs/testing.md` 的「Mutation test」），每一條都要重編、重新 link 它的測試。
- 每一套 binary 都佔磁碟：連結 V8 的 exe 約 90 MB、`.pdb` 約 180 MB，換 toolchain 或 `Cargo.lock` 就多一整套（`ci.py prune` 清）。
- 同時 link 的數量多：Windows 上碰到幾次間歇性的 `LNK1104: cannot open file 'msvcrt.lib'`（`link.exe`；換成 rust-lld 之後沒再碰到）。

`docs/testing.md` 的「工具和跑法」記了 link 是重編的大頭；這份 RFC 處理 binary 的數量，那裡處理每次 link 的成本，兩者相乘。

**目標**：

- 重編和跑測試的時間降下來，不設門檻（第 7 條）：改了 `dom` 之後重編整個 workspace 的測試、改了實作之後重編一個 crate 的測試（實作和 mutation test 每一條的情況），和跑測試的時間，三個一起看。
- 每個 crate 的整合測試照舊各自是一個 binary（`cargo test -p dom` 照舊），只是少了很多個。
- 測試的行為不變：同樣的測試、同樣的 V8 flag，`test.py matrix`、`fuzz` 照舊跑同樣的 crate。
- 之後新增的整合測試自然加進那個 binary，不會又長出新的 binary。

**不在範圍內**：

- crate 裡的單元測試（`src/` 的 `#[cfg(test)]`，每個 crate 的 lib 一個 binary）：照舊。全部合併之後，它們（`shell`、`renderer`、`dom_api` 的各 11 到 12 秒）和 `kun` 的執行檔是改 `dom` 之後重編的大項。
- 測試本身的內容和斷言：只搬家，不改寫（V8 flag 的處理除外，見「詳細設計」）。
- 換測試的執行器（cargo-nextest）：見「理由和其他做法」。
- 每次 link 的成本（debuginfo、linker）：已經處理過。

## 設計概要

**寫測試的人**：一個 crate 的整合測試放在 tests/it/ 底下，每個檔案一個模組，在 tests/it/main.rs 裡 `mod` 進來：

```
crates/dom/tests/
  it/
    main.rs          // mod abort; mod collections; …；共用的 helper；main 之前設 V8 的 flag
    abort.rs
    collections.rs
    …
  common/mod.rs      // 共用的 helper（stress 的 binary 也用）
  incremental_stress.rs   // 自己的 V8 flag：獨立的 binary
  compile_fail.rs         // trybuild
  ui/
```

新增一組測試：在 tests/it/ 加一個檔案、在 `main.rs` 加一行 `mod`。直接放在 `tests/` 底下的檔案會變成獨立的 binary，`sanity.py tests` 擋下沒有理由的；tests/it/ 裡沒在 `main.rs` 宣告的檔案不會編、不會跑，它也擋下。

**跑測試的人**：

| 以前 | 之後 |
| --- | --- |
| `-p dom --test tree` | `cargo test -p dom --test it tree::` |
| `-p dom --test events dispatch` | `cargo test -p dom --test it events::dispatch` |
| `cargo test -p dom` | 照舊 |
| `cargo test -p dom --test incremental_stress` | 照舊（獨立的 binary） |

篩選照 libtest 的規則比對測試的完整路徑，所以 `tree::` 選到原本 `tree.rs` 的所有測試。同時給幾個篩選時放在 `--` 之後（`cargo test -p dom --test it -- tree:: events::`）：cargo 自己只收一個。

**一個測試讓 process 結束時**（V8 的 abort、`DomIsolate` 的 abort 檢查）：以前只影響那個檔案的測試，現在整個 crate 的整合測試都停在那裡，沒跑到的不會回報。這種情況本來就是要修的 bug，但找原因時要用篩選單獨跑它（`docs/testing.md` 的「工具和跑法」）。

## 詳細設計

### 分組

每個 crate 照這個規則決定哪些測試是獨立的 binary（合併前後的清單見 `details.md` 的「現有的測試」和「合併之後」）：

1. **自己的 `main`（`harness = false`）**：`shell` 的 `scenarios`、`speedometer`。它們本來就是一個 binary 跑整個 suite。
2. **trybuild**：`dom` 的 `compile_fail`。它不連結 V8（約 2 MB），也不是 libtest 的測試在跑。
3. **V8 的 flag 和其他測試不同的**：`dom` 的 `incremental_stress`（`--stress-incremental-marking`），`bindings` 的 `cppgc_traced_marking`（同上）、`isolate_teardown`（`--stress-compaction`），`dom_js` 的 `teardown_stress`（`--expose-gc --stress-compaction`）。它們的 flag 改的是整個 process 的 GC，合進來會讓其他測試也在 stress 底下跑，或讓自己拿不到 flag（第 12 條）。
4. **其他的全部是 tests/it/ 的模組**，連只有一個整合測試的 `browser`、`dom_wasm`、`net`（第 15 條）。

結果：13 個 crate 各一個 `it`，加上 7 個獨立的；workspace 的測試 binary（含每個 crate 的 lib 的單元測試）從 159 個變成 44 個，連結 V8 的從 140 個變成 30 個。

### V8 的 flag

V8 的 flag 由第一個初始化 V8 的呼叫決定，整個 process 只設一次（`bindings` 的 `platform.rs`）。以前每個測試 binary 靠「自己是獨立的 process」帶自己的 flag：大多數是 `--expose-gc`，有幾個不帶。合併之後不能靠測試自己設：`dom` 和 `bindings` 自己就會不帶 flag 地初始化 V8（`DomIsolate::new_detached_for_testing`、`bindings::platform()`，`new_standalone` 經過它，和 `V8PerIsolateData::new`），哪個測試先跑（libtest 的 thread 同時跑，順序不固定），整個 process 的 flag 就是它的。

- **`bindings::initialize_v8_before_main!(flags)`**（第 6、10 條）：展開成 `ctor` 的 constructor，在 `main` 之前呼叫 `initialize_with_flags`，所以 flag 不取決於哪個測試先跑，新加的測試也不用記得。`ctor` 要求標 `unsafe`，宣稱和 `SAFETY` 只在 `bindings`（審查要點在 `crates/dom/soundness.md`）；測試的 crate 只展開巨集，經由 `bindings` 的 re-export 用 `ctor` 的宣告式寫法。V8 的 `unittests` 一樣在所有測試之前建好 platform（gtest 的 global test environment）。
- **哪些 binary 用它**（第 9、16 條）：flag 混在一起的 `dom`、`dom_js`、`bindings` 設 `--expose-gc`（它只讓 JS 看得到 `gc()`，不改 GC 的行為；以前不帶 flag 的測試在它底下都通過）。其他 crate 的測試本來就全部不帶 flag，照舊不帶，不用它。
- **測試裡不再生效的 flag 拿掉**（第 11 條）：各檔的 `initialize_with_flags("--expose-gc")`、`bindings::initialize()`、`const FLAGS`；`dom` 的 helper 改成 `new_dom()`，stress 的 binary 用 `new_dom_with_flags`。`dom` 的 `readme_example` 照 README 的範例保留它的 `initialize_with_flags`。
- `main.rs` 寫成 `::bindings::initialize_v8_before_main!`：`dom_js` 有一個測試模組叫 `bindings`，在 `main.rs` 裡蓋過同名的 crate。

### 共用的 helper

`dom` 的 `crates/dom/tests/common/` 留在原處：`it/main.rs` 用 `#[path = "../common/mod.rs"]` 引用一次，模組用 `crate::common`；`incremental_stress` 照舊 `mod common;`。`dom_api` 的 `webidl` 只給 `spec` 用，變成 `spec` 的子模組（`it/spec/webidl.rs`）。

### 同時跑的測試

libtest 在一個 binary 裡用多個 thread 同時跑測試（預設是 CPU 數），binary 之間一個接一個跑。合併之後同一個 crate 的測試在同一個 process 裡同時跑：同時跑的測試數一樣，但不再一個 binary 等另一個，所以跑測試的時間也降了（`shell` 從 20.6 秒變成 9.4 秒）。`shell` 的端到端測試（各自開 renderer 和 thread）在合併的 binary 裡照樣穩定（每一段連跑幾次、`cargo test --workspace`）。

### 守住

- **`sanity.py tests`**（第 18 條，`CLAUDE.md` 的檢查清單）：`crates/*/tests/` 直接底下的 `.rs` 只能是 `Cargo.toml` 的 `[[test]]` 設了 `harness = false` 的，或列在 `sanity.py` 的 `OWN_TEST_BINARIES` 裡、附上理由的；tests/it/ 裡的檔案都要是 `main.rs` 宣告的模組（沒宣告的不會編，測試靜悄悄地不跑）；清單裡的檔案不在了也報錯。不用 `autotests = false`：它會讓 `tests/` 底下忘了宣告的檔案靜悄悄地不編譯、不跑。
- **`sanity.py docs`**：認得 tests/it/main.rs 這種目錄形式的 target；`--test it <模組>::` 的模組要在 tests/it/ 底下（第 19 條：打錯時測試只會跑零個）；已完成的 RFC 和 `rfcs/pre-rfcs/` 的路徑和測試指令不檢查（第 14 條：它們記的是當時的樣子）。

### 文件和指令

`rfcs/` 以外的文件、程式碼註解、腳本和黃金檔的開頭（`dom_api` 的 `spec-missing.txt`、`url-expected-failures.txt`，用 `MACHOPO_UPDATE_GOLDEN` 重新產生），和還沒完成的 RFC，用一個腳本把 `--test <名稱>` 和 `tests/<名稱>.rs` 改成 tests/it/ 的（`details.md` 的「要改的指令」）；parley fork 的 `README.machopo.md` 也改了一行。`docs/testing.md` 的「測試的種類」寫整合測試的組織，「工具和跑法」寫 link 的數字、一個測試讓 process 結束時的影響和 `initialize_v8_before_main!`；`dom`、`dom_api`、`dom_js` 的 README 的測試表說明每一列是一個模組；`CLAUDE.md` 加上 `sanity.py tests` 和「新的整合測試是 tests/it/ 的模組」。`test.py` 的 `matrix`、`fuzz` 用 `-p`，不用改。

### 測試

- 每個 crate 合併前後的 `cargo test -p` 的測試數相同，名稱只多了模組的前綴；`cargo test --workspace` 合併前後的測試數相同、全過。
- 合併的 binary 的 V8 flag：拿掉 `initialize_v8_before_main!`、單一 thread 讓不帶 flag 的測試先跑，用 `gc()` 的測試失敗（`dom`、`dom_js`、`bindings`，`test.py mutate`）。
- 重編和跑測試的時間：合併前（`main` 的 worktree）和合併後各量 3 次取中位數（第 3、17 條），記在 `progress.md`。
- `sanity.py tests` 和模組名稱的檢查：手動放違規的檔案、打錯的模組名稱，確認失敗。
- `test.py matrix`：在 macOS 上跑（第 13 條；這份動到的是測試的組織，不是 GC，但 `matrix` 跑的就是這些 binary）。
- 不需要 mutation test：這份 RFC 不改行為，測試數相同就是驗收。

### 分階段

0. **原型和實測**（不 merge）：合併 `dom`，量重編和跑測試的時間；只合併 `dom` 時整個 workspace 的那一項幾乎沒降（瓶頸在 `shell` 的 23 個同時 link 的 binary），所以也合併了 `shell`。結果決定做下去，目標改成三個數字一起看（第 7 條）。
1. **`dom`、`dom_api`、`dom_js`、`bindings`**：V8 flag 的問題集中在這幾個；`initialize_v8_before_main!`。
2. **`shell`、`browser`、`layout`、`paint`、`style`、`html`、`dom_wasm`、`protocol`、`net`**：沒有 flag 的問題。
3. **守住和文件**：`sanity.py tests`、模組名稱的檢查、`docs/testing.md`、各 crate 的 README、`CLAUDE.md`。

四段都在 `0025` 上做完才一起 merge：文件的整體說明在第 3 段，前面的段落單獨 merge 時 `main` 上的文件會和程式不一致。

## 缺點

- **隔離變差**：一個測試讓 process 結束（abort、stack overflow），同一個 crate 的其他整合測試都不會回報；以前只影響一個檔案。
- **只改一個測試檔時重編變大**：要重編整個 crate 的整合測試，不是只有那個檔案。實測最多多 1.7 秒（`shell`，從 2.6 秒到 4.3 秒），測試檔少的 crate 幾乎不變；改實作時沒有差別（本來就全部重編）。
- **指令變長**：`--test it tree::` 比 `--test tree` 長，幾個篩選要放在 `--` 之後。
- **V8 的 flag 一律是 `--expose-gc`**（`dom`、`dom_js`、`bindings`）：不帶 flag 的測試從此在 `--expose-gc` 底下跑，「沒有 `gc()`」的情況不再被測到（沒有測試依賴它）。
- **多一個第三方的依賴**：`ctor`（`bindings` 的依賴，只有測試展開它）；在 `main` 之前出錯時沒有測試的 panic 那麼清楚。

## 理由和其他做法

完整的比較在 `details.md` 的「其他做法」。

- **A. 每個 crate 一個 tests/it/（選這個）**：binary 少了四分之三，`-p` 的範圍不變，`test.py matrix` 不用改；V8 flag 不同的留成獨立的 binary。
- **B. 整個 workspace 一個測試的 crate**：link 最少（整個 workspace 只剩幾個），但測試和它測的 crate 分開、依賴全部集中在一個 crate，`cargo test -p dom` 不再跑 `dom` 的整合測試，`test.py matrix` 要改；改 `dom` 時也要重編所有 crate 的整合測試。效益和 A 的差距不大（A 已經去掉大部分），代價大。
- **C. 整合測試搬進 `src/` 當單元測試**：合進每個 crate 已有的 lib 測試 binary，binary 最少，但測試能碰到私有的東西、不再只經過公開的 API，`dom` 的安全論證有些測試要的就是「只用公開的 API 做不到」；也會讓 lib 在測試設定下重編得更多。
- **D. 換成 cargo-nextest，每個測試一個 process**：解決 V8 flag 和隔離的問題，但不減少 binary 的數量（link 的成本照舊），而且多一個第三方工具、`harness = false` 的 binary 要另外處理。和 A 不衝突，之後可以再考慮。
- **不做**：改 `dom` 之後的重編照舊約 30 到 40 秒，mutation test 每條多幾秒。

## 先例

- **Chromium**：每個元件一個大的測試 binary（`blink_unittests`、`content_unittests`、`browser_tests`），不是每個測試檔一個；隔離交給 test launcher：它把測試分批交給子 process 跑，browser test 預設每個測試一個 process。kun 照「一個大 binary」，隔離沿用 libtest 的 thread（不做 launcher，見 D）。
- **V8**：`cctest`、`unittests` 也是少數幾個大 binary；`unittests` 在所有測試之前建好 platform（gtest 的 global test environment），`tools/run-tests.py` 每個測試起一個 process 跑，所以每個測試可以有自己的 flag。kun 沒有這種 runner，所以 flag 不同的測試留成獨立的 binary。
- **Rust 的專案**：matklad 的〈Delete Cargo Integration Tests〉（2021）建議每個 crate 只有一個 tests/it/main.rs，理由就是每個檔案一個 binary 的 link 成本；rust-analyzer 等照這樣做。

## 已決定的事項

2026-10-10 確認（第 0 段開始前）：

1. **排程，先做第 0 段**：從 `main` 開 `0025`，原型只合併 `dom`，不 merge；結果回答當時「待決定」的前兩題（值不值得做、不帶 flag 的測試能不能一律用 `--expose-gc`），再決定做不做第 1 段。
2. **原型的 binary 名稱用 `it`**：`-p dom` 已經說了是哪個 crate，指令短（matklad 的寫法）。不選 `dom_tests`（Chromium 的 `<component>_unittests` 的寫法）：在 cargo 的輸出裡看得出是哪個 crate，但每個指令都重複 crate 的名字。只是原型的名稱，當時「待決定」的第 3 題在第 1 段開始前定案。
3. **「改了 `dom`」的量法**：在 `dom` 的一個函式本體裡改一個常數，在兩個值之間來回切換（最接近實作和 mutation test；只 `touch` 不改內容時 cargo 照樣重編，但和真的改程式還是有差別）。量四項，合併前後各量，每項 3 次取中位數：(a) 改 `dom` 之後 `cargo test --workspace --no-run`；(b) 改 `dom` 之後 `cargo test -p dom --no-run`；(c) 只改一個測試檔之後 `cargo test -p dom --no-run`（「缺點」的「一個測試的重編變大」）；(d) 編好之後 `cargo test -p dom` 的執行時間。另外記下 `target/debug/deps` 裡測試 binary 的個數和大小。
4. **門檻**（第 7 條取代）：(a) 降不到一半就停下來討論（當時「待決定」的第 1 題）；(c) 變慢多少可以接受先不定，看了數字再一起判斷。
5. **每次跑測試的時間都記下來，RFC 做完時用來評估測試流程**：這份 RFC 實作期間跑的每一個測試和檢查的指令（`cargo test`、`cargo check`、`test.py` 的各項、`sanity.py`、mutation test、fmt、clippy，和為了跑測試做的 build；查東西的指令不算），經過一個包裝腳本跑，每次一列附加到 `test-times.tsv`：開始的時間、階段、為什麼跑（實作中、量測、驗收、重跑）、指令、cargo 的編譯時間（`Finished … in` 的合計）、libtest 的執行時間（`finished in` 的合計）、總時間、exit code、passed 和 failed 的合計、備註（快取冷熱、同時在跑的東西）。用 TSV 不用 Markdown：它是原始資料，之後要加總、分組；放在 `.md` 裡時 `sanity.py docs` 會檢查表裡舊的 `--test` 名稱和測試數（合併之後舊的名稱不存在）。包裝腳本不放進 repo，評估時再決定要不要變成 `test.py` 的功能。RFC 做完時把它整理成摘要（每種指令的次數、時間和佔比），原始的檔案留不留到時再決定。

2026-10-10 確認（第 0 段，量完合併前的基準之後）：

6. **合併的 binary 在 `main` 之前初始化 V8**：`main.rs` 用 `ctor`（只是 dev-dependency）在 `main` 之前呼叫 `bindings::initialize_with_flags("--expose-gc")`。原本的計畫（共用的初始化函式，各測試改呼叫它）不夠：`dom` 和 `bindings` 自己就有不帶 flag 的初始化（`DomIsolate::new_detached_for_testing`、`bindings::platform()`，`new_standalone` 經過它，和 `V8PerIsolateData::new`），合併之後 `foreign`、`cross_isolate`、`detached_marking` 這類測試先跑時整個 process 就沒有 `--expose-gc`，用 `gc()` 的測試照執行的順序時好時壞（libtest 的 thread 同時跑，順序不固定）。在 `main` 之前初始化，不管哪個測試先跑、新加的測試有沒有記得，flag 都一樣；對應 V8 的 `unittests` 用 gtest 的 global test environment 在所有測試之前建好 platform、`blink_unittests` 在 `main` 裡先初始化 `TestSuite`。不選每個測試開頭呼叫 `common::init()`：忘了的測試變成和順序有關的不穩定，檢查守不住。不選讓 `bindings` 不帶 flag 的初始化讀一個預設值：預設值一樣要在第一個測試之前設好，還是回到上面兩種。代價是多一個第三方的 dev-dependency，在 `main` 之前出錯時沒有測試的 panic 那麼清楚。

2026-10-10 確認（第 0 段做完，結果在 `progress.md`）：

7. **做下去，目標改成三個數字一起看，不設門檻**（「有多少算多少」），取代第 4 條的門檻。第 0 段只合併 `dom` 和 `shell`：改 `dom` 之後重編整個 workspace 的測試從 35.9 秒到 26.8 秒（−25%，全部合併後推估 −40% 到 −47%，大概到不了一半），改 `dom` 之後重編 `dom`、`shell` 的測試 −51%、−38%，跑 `shell` 的測試 −54%。整個 workspace 的那一項降得少，是因為關鍵路徑上 `dom` → `dom_api` → `renderer` → `shell` 的 lib 和 `kun` 的執行檔本來就要編、要 link，剩下的大項是各 crate 的 lib 的單元測試，不在這份的範圍；只看它會低估效益，實作和 mutation test 每次碰到的是重編、跑一個 crate 的測試。不選照原本的門檻先把所有 crate 粗略搬一次實測：多花時間，而且結果大概在門檻附近，不改變要不要做。不選停下來：其他兩項的效益已經夠大。原本的「待決定」第 1 題（值不值得做）由這條回答。

2026-10-10 確認（第 1 段開始前）：

8. **binary 的名稱是 `it`**（原本「待決定」第 3 題的前半）：原型用起來沒有問題，理由同第 2 條。只有一個整合測試的 crate 要不要搬，第 2 段開始前決定。
9. **只有 flag 混在一起的 crate 在 `main` 之前設 `--expose-gc`**：`dom`、`dom_js`、`bindings`。`dom_api`（和第 2 段的 `shell` 等）的測試本來就全部不帶 flag（經由 `DomIsolate`、`V8PerIsolateData::new` 初始化），合併之後沒有衝突，照舊不帶，行為不變。不選每個合併的 binary 都設：沒有理由地把 `dom_api` 改成 `--expose-gc`，和目標的「測試的行為不變」不合，也多了用不到的 `ctor`。
10. **`main` 之前的初始化是 `bindings` 的巨集**（`bindings::initialize_v8_before_main!`）：`ctor` 要求標 `unsafe`（在 `main` 之前，std 不保證可用；初始化讀環境變數、開 V8 的 thread），這是一個「這樣做安全」的宣稱，照「`unsafe` 只在 `dom`、`bindings` 和 `ipc`」的規則，宣稱和 `SAFETY` 只寫在 `bindings` 一處（`crates/dom/soundness.md` 記一段），測試的 crate 只展開巨集，不直接依賴 `ctor`（`ctor` 的宣告式寫法經由 `bindings` 的 re-export 解析）。`ctor` 是 `bindings` 的一般依賴，產品的程式碼不展開它。對照 V8：`unittests` 的 platform 由 V8 自己的測試基礎設施（`DefaultPlatformEnvironment`）建，不是每個測試檔各自做。不選在 `dom_js` 等的測試裡直接用 `ctor`：lint 和 `sanity.py unsafe` 都抓不到（展開在巨集裡），`unsafe` 的宣稱散到規則以外的 crate。第 6 條原型裡直接用 `ctor` 的寫法照這條改。
11. **合併的 binary 裡拿掉不再生效的 flag**：各檔的 `initialize_with_flags("--expose-gc")`、`bindings::initialize()`、`const FLAGS` 和 `new_dom(FLAGS)` 的參數（`foreign` 的 `new_dom("")` 實際上也是 `--expose-gc`，留著會誤導）；helper 改成 `new_dom()`，只有 stress 的 binary 自己設 flag。這是「詳細設計」允許的「V8 flag 的處理除外」。`dom` 的 `readme_example` 照 README 的範例保留它的 `initialize_with_flags`（在合併的 binary 裡沒有作用，但範例要示範給沒有巨集的使用者）。
12. **stress 的測試維持各自獨立**（原本「待決定」第 4 題）：`bindings` 的 `cppgc_traced_marking`、`isolate_teardown`，`dom_js` 的 `teardown_stress`，`dom` 的 `incremental_stress`；數量少，合併也省不了幾個 link。
13. **`test.py matrix` 留到最後，由使用者在 macOS 上跑**：實作期間不跑（要先編 debug 的 V8）；驗收表上標成「等 macOS」。

2026-10-10 確認（第 1 段，改文件時）：

14. **已完成的 RFC 和 `rfcs/pre-rfcs/` 裡的路徑和測試指令，`sanity.py docs` 不再檢查**：它們記的是當時的樣子（當時跑過的 `--test tree`、當時的檔案），合併之後 487 處指向不存在的名稱。照原本的規則（完成的 RFC 只修 `sanity.py docs` 抓到的壞掉的路徑），要一次改掉這些歷史紀錄，之後每次搬家、改名又要再來一次；歷史的用途是讓人知道當時怎麼做，Chromium 的舊設計文件也不跟著程式碼改名。日期、測試數、章節的引用照舊檢查；還沒完成的 RFC（🚧、👍、📝）照舊檢查，這次的合併照 `rfcs/` 以外的做法改了它們（0016、0028、0031）。不選照原本的規則改掉全部：一次大量改動歷史，以後還會重來。不選只改這次的、另外寫例外清單：比兩者都繞。`rfcs/README.md` 的「流程」第 6 步和 `docs/README.md` 跟著改。

2026-10-10 確認（第 2 段開始前）：

15. **只有一個整合測試的 crate 也搬進 tests/it/**（原本「待決定」第 3 題的後半）：`browser`、`dom_wasm`、`net` 搬了不少任何 binary，但每個 crate 的寫法一致，之後新加的測試自然放進去，第 3 段的 `sanity.py` 檢查也不用為它們開例外。代價是指令變長（`--test navigation` 變成 `--test it navigation::`）。不選留著：多一種寫法，檢查要開例外。
16. **第 2 段的 crate 都不在 `main` 之前設 flag**（第 9 條）：`browser`、`layout`、`paint`、`style`、`html`、`dom_wasm`、`protocol`、`net`、`shell` 的測試都不帶 V8 的 flag，合併之後照舊。
17. **第 2 段的量測**：每個 crate 照第 1 段，在 `main` 的 worktree 量合併前、在 branch 上量合併後的 (b)、(c)、(d)（依賴 `dom` 的 crate 的 (b) 改 `dom`，`protocol`、`net` 改它們自己）；全部合併之後再量整個 workspace 的 (a) 和跑測試的時間，是這份的主要數字。`shell` 的第 0 段量過，不重量。

2026-10-10 確認（第 3 段開始前）：

18. **守住的檢查是 `sanity.py tests`**（新的子指令，加進 `CLAUDE.md` 的檢查清單）：`crates/*/tests/` 直接底下的 `.rs` 只能是 `Cargo.toml` 的 `[[test]]` 設了 `harness = false` 的，或列在 `sanity.py` 的一份清單裡、附上理由的（stress 的 4 個、trybuild 的 1 個；照 `UNSAFE_ALLOWED` 的先例放在腳本裡）；其他的報錯，提示放進 tests/it/。不選放進 `sanity.py docs`：它不是文件的檢查。
19. **`sanity.py docs` 也檢查 `--test it <模組>::` 的模組**（`details.md` 的「要改的指令」寫的「之後可以補」）：模組名稱打錯時測試只會跑零個，沒有任何錯誤；只檢查 `::` 前面的第一層模組在 tests/it/ 底下。
20. **文件**：`docs/testing.md` 的「測試的種類」寫整合測試的組織（每個 crate 一個 tests/it/，哪些獨立、為什麼），「工具和跑法」更新 link 那一條，加上篩選的寫法、一個測試讓 process 結束時的影響和 `initialize_v8_before_main!`；各 crate 的 README 寫到測試清單或位置的地方照實改；`CLAUDE.md` 加 `sanity.py tests` 和「新的整合測試放進 tests/it/」。新的檢查照「測試」一節手動放一個違規的檔案確認失敗。
21. **第 3 段之後做 RFC 完成時的整理**，包括把 `test-times.tsv` 整理成摘要；量合併前的數字用的 worktree 在第 3 段開始時移除。

2026-10-10 確認（完成之後）：

22. **`test-times.tsv` 刪掉，只留 `progress.md` 裡的摘要**，回答第 5 條留下的問題（原始的檔案留不留）：之後的時間紀錄只放在每台機器的本機，摘要才進 repo（RFC 0032 的第 2 條）；這份的原始紀錄在 commit `a2ffb990` 裡找得到。

## 待決定

無。原本的「不帶 flag 的測試能不能一律用 `--expose-gc`」由第 0、1 段回答：可以，`dom`、`dom_js`、`bindings` 不帶 flag 的測試在 `--expose-gc` 底下都通過（`progress.md`）。

## 之後的可能

- 用 cargo-nextest 跑 `cargo test --workspace`（D）：binary 之間平行、每個測試一個 process，隔離回來（stress 的測試也可以合進 `it`，用 nextest 的設定給它們自己的環境變數）。
- `test.py` 的 `matrix`、`fuzz` 可以只跑 `it` 和 stress 的 binary，不用跑整個 `-p`。
- 改 `dom` 之後重編的剩下的大項：各 crate 的 lib 的單元測試（`shell`、`renderer`、`dom_api` 的各 11 到 12 秒）和 `kun` 的執行檔。
- 在 macOS 上量同樣的數字（Apple 的 ld-prime，加上 macOS 掃描每個剛 link 好的執行檔：binary 少了，那邊可能省得更多）。
