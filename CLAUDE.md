# machopo 的工作約定

給在這個 repo 工作的 AI 助手（和新的合作者）。machopo 是照 Chromium 架構切分的 Rust 瀏覽器；做什麼、做到哪裡、怎麼設計的，都在下面這些文件裡，這份只寫怎麼工作。

**回覆一律用繁體中文**，技術性的內容也一樣。

## 先讀

- `README.md`：怎麼 build、怎麼跑，第三方的專案（fork 和 WPT）放在哪裡。
- `docs/architecture.md`：目前的架構（process 和 thread、pipeline、crate、和 Chromium 刻意不同的地方）。
- `docs/limitations.md`：**目前仍然成立的已知限制**；`docs/testing.md`：測試的種類和跑法、mutation test 的做法；`docs/test-gaps.md`：測試守不住的地方（只靠審查的、只做過手動確認的、沒有計數守著的優化、等價的 mutation）。開始任何工作前先看。效能的量測在 `docs/benchmarks.md`。
- `rfcs/README.md` 和各份 RFC：MVP 之後的功能和設計；正在做的 RFC 的進度在它的 `progress.md`。
- `rfcs/pre-rfcs/`：MVP 的規劃、各階段最後的設計和理由（包括試過、改掉的做法），2026-10-04 起凍結。找某個設計為什麼這樣做時讀它，不要再改它。
- `docs/README.md`：每份文件放什麼、怎麼更新。**改文件前一定要讀**。
- 動到 `bindings` 的 GC、`unsafe`、執行 script 的入口：先讀 `crates/bindings/soundness.md`。動到 DOM 的 GC（elise，`../third_party/elise`）：先讀 `../third_party/elise/docs/soundness.md`、`../third_party/elise/docs/api.md` 和 `../third_party/elise/docs/contributing.md`。
- 加新的 CSS property、pseudo-class、會影響 style 的 DOM 狀態、box 畫的新輸入，或改 hit test、parse 的 fast path：先看 `docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」，漏改的多半是畫面不更新，不是 crash。
- 各 crate 的 `README.md`：API、使用規則、怎麼新增東西（例如 `crates/dom_api/README.md` 的「怎麼新增一個 API」）。

## 怎麼做決定

- **大的實作選擇，先列選項和建議，確認之後再做**，不要自己選了就做。附上優缺比較，效能與成本考量，並對照 Chromium 怎麼做（和 Chromium 無關、對照沒有意義時才省略）。
- **大的工作分段做**（RFC 的「分階段」），每段開始前只列那一段要決定的事，不要一次丟出全部。
- 定下來的事寫進對應 RFC 的「已決定的事項」（日期、選了什麼、不選什麼、為什麼）。
- **RFC 的編號和路徑只寫在 `rfcs/` 裡**：其他文件、程式碼、測試的清單和頁面只寫現在的行為和理由，不寫是哪一份 RFC 做的、哪一份會做，也不寫 MVP 時期的文件和階段代號（`docs/README.md` 的「每份文件放什麼」最後的其他規則；`sanity.py docs` 檢查）。
- 新的功能先寫 RFC 的草稿，設計決定經過確認才開始做（`rfcs/README.md` 的「流程」）。

## 不要做的事

- **不要 commit 或 push**，除非被要求；由人審核之後自己 commit。
- **commit message 不要加 `Co-Authored-By`**（或其他署名 AI 的 trailer，例如 `Claude-Session`）：只寫改了什麼、為什麼。
- **RFC 的實作不要做在 `main` 上**：在以編號命名的 branch（例如 `0013`）上做，RFC 完成、或「分階段」的一個階段做完並經過審核時才 merge 回 `main`，一律 fast-forward（先 `git fetch origin`、確認本地的 `main` 和 `origin/main` 一致，再 rebase 到最新的 `main`，再 `git merge --ff-only`，見 `rfcs/README.md` 的「流程」）；merge 也要等人要求。
- **不要對上游開 issue、PR 或留言**（rusty_v8、parley、taffy、web-platform-tests、html5ever……）：要回報的事先在本地寫好草稿，給人審核。
- 跑 mutation test 的期間，實作檔案會暫時被改壞：先告訴對方不要 stage 或 commit，跑完確認檔案都還原了。
- 這個目錄可能有別人的未提交變更：開始前看 `git status`，不是你改的不要動、不要還原。

## 測試

- **在視窗裡看得到的行為寫成情境**（`tests/scenarios/`），不要請人開視窗確認；情境做不到的（輸入法、剪貼簿、游標、外觀）才請人看。做法見 `docs/README.md` 的「完成一步時」第 5 步。
- **新的整合測試是那個 crate 的 tests/it/ 的模組**（加一個檔案、在 `main.rs` 加一行 `mod`），不直接放在 `tests/` 底下：那會多一個要重新 link 的 binary（`docs/testing.md` 的「測試的種類」；`sanity.py tests` 檢查）。
- 新的行為都要有測試，並且**跑 mutation test**：弄壞實作，確認有測試失敗；沒抓到的要補測試，或說明為什麼是等價的。用 `python3 scripts/test.py mutate <spec>` 跑（它先確認原本的程式通過、跑完還原檔案），做法見 `docs/testing.md` 的「Mutation test」。結果寫進那份 RFC 的 `progress.md` 的「做過的檢查」（`docs/README.md` 的「完成一步時」）。補不了測試、只能靠審查或手動確認的，記進 `docs/test-gaps.md` 的「測試守不住的地方」，寫明之後怎麼處理。
- **Speedometer（`bench.py speedometer`）平常不量，只在使用者要求時跑**：RFC 的測試計畫也不排量測（`docs/benchmarks.md` 的「效能的量測」）。
- **整套的測試由 CI 跑**（使用者設定）：每天跑 `test.py` 的 `wpt`、`wdspec`、`reftest`、`scenarios` 和 `cargo test --workspace`，每週跑 `matrix`、`fuzz`。做 RFC 時本機只跑和改動相關的部分（下面幾條），**不跑 `matrix`、`fuzz`、`wdspec` 和整套的 `wpt`**；要重現 CI 的失敗時才跑那一項。merge 不等 CI，CI 上壞掉的看到就修。
- 完成一段工作前，跑改到的 crate 的測試和 sanity 檢查：

  ```sh
  cargo test -p <crate>                       # 改到的 crate（一個或幾個）；整個 workspace 在階段做完時跑
  python3 scripts/sanity.py deps              # V8 只在 bindings、dom_js
  python3 scripts/sanity.py spans             # span 用 trace_event 的 macro（沒開 trace 時不建）
  python3 scripts/sanity.py boundary          # renderer 守住 process 的邊界
  python3 scripts/sanity.py unsafe            # unsafe 只在 bindings 和 ipc
  python3 scripts/sanity.py tests             # 每個 crate 的整合測試是一個 binary（tests/it/）
  python3 scripts/sanity.py numerics          # 整數運算寫明溢位時怎麼辦（crates/base/README.md）
  python3 scripts/sanity.py docs              # 改了文件時
  ```

  **`cargo test --workspace`、`cargo fmt --check` 和 clippy 只在整個階段做完時跑一次**（RFC 的「分階段」的一個階段：例如第 4 階段分成 4a、4b、4c 時，三段都做完才跑）；實作、mutation test 和中間的段落期間不跑，段落的 commit 可以還沒經過它們。跑到的問題在那個階段的最後一個 commit 裡修好：

  ```sh
  cargo test --workspace                      # 不含 reftest、情境和 WPT；連結 release 的 V8
  cargo fmt --check
  cargo clippy --workspace --all-targets -- -D warnings
  ```

  `python3 scripts/sanity.py -h`、`ci.py -h` 和 `test.py -h` 有每個指令的說明。**`test.py` 的 `matrix` 和 `fuzz` 由 CI 跑**，動到 GC 的 RFC 也不在本機跑：merge 之後看 CI 的結果（它們每週跑一次，動到 GC 時在 Actions 的網頁上手動開始 `gc.yml`；要在本機重現時：它們用 debug 的 V8（有 DCHECK）跑 GC 的測試，`fuzz` 跑 20 組設定（`--iterations 20`），用另一個 target 目錄（`target/v8-debug`），第一次要完整 build 一次）。
- **`python3 scripts/test.py automation` 只在動到視窗那一層、或 RFC 和階段驗收時跑**（其他時候等使用者要求）：它在真的視窗（winit、wgpu）裡跑 `tests/scenarios/` 的情境，要顯示器和 GPU，跑的時候視窗會在最上層。視窗那一層是 `crates/shell/src/window.rs`、`browser_window.rs`、`automation.rs`、`windowless.rs`、`scenario.rs`，和 `viz` 的 `WgpuRenderer`、Viz thread。同樣的情境沒有平台視窗的版本是 `python3 scripts/test.py scenarios`（下一條）。跑完看 `target/automation-results/` 的截圖。
- **改了 machopodriver（`crates/driver`）、`machopo --automation-pipe`（`shell` 的 `automation_pipe`、`automation_session`）、`protocol::automation` 或 `tests/wpt/wptrunner_machopo/` 時，用 `python3 scripts/test.py wpt` 跑一個小目錄**：WPT 的 testharness.js 測試、reftest 和 crashtest 經過上游的 wptrunner 和 machopodriver 跑，這條路壞了它會失敗。**`python3 scripts/test.py wdspec`（WPT 的 WebDriver 測試，`tests/wpt/README.md`）本機只在改了 wdspec 的清單或預期結果時跑那幾個**；machopodriver 平常由 `cargo test` 裡的 `webdriver`、`automation_pipe` 測試守著，整套由 CI 跑。
- **`python3 scripts/test.py reftest`、`scenarios` 和 `wpt` 在本機只跑改到的部分**（`cargo test --workspace` 跳過情境，reftest 和 WPT 不在 `cargo test` 裡；整套由 CI 跑）：改了 style、layout、文字、paint、raster 時，跑相關的 `test.py reftest <路徑>`（machopo 自己的 reftest）和 `test.py wpt <目錄>`（WPT 的 reftest 在它裡面；兩者的篩選見 `-h`）；改了 machopo 自己的 reftest 或它的 expectations 時跑那幾份 `test.py reftest`；寫了或改了情境時，用 `test.py scenarios <名稱>` 跑那幾份；改了 DOM API、WPT 的清單或預期結果時，跑相關的目錄的 `test.py wpt`（預期結果要更新時照樣用 `MACHOPO_UPDATE_EXPECTATIONS` 跑那幾個）。
- **一份 RFC 做完、merge 回 `main` 之後跑 `cargo clean`**（`docs/README.md` 的「一份 RFC 做完時」）：cargo 不刪舊的輸出，macOS 的 debug build 每次重新 link 都留下舊的 `.o`，`target/debug/deps` 堆大了之後，每個會畫文字的測試 binary 都要多等好幾秒才開始；Windows 上留下的是舊的整套執行檔和 `.pdb`，會吃掉幾百 GB 的磁碟（原因見 `docs/testing.md` 的「工具和跑法」）。RFC 做到一半測試明顯變慢、或磁碟不夠時也可以先跑。它刪掉整個 `target/`（包括 `test.py mutate` 存放的原檔），不要和 cargo 或 `test.py` 同時跑。
- **會跑很久的指令前面加 `caffeinate -i`**（macOS）：電腦休眠時 monotonic clock 不前進，看起來像卡住。Windows 上先在電源設定裡暫時關掉睡眠。
- 不穩定的測試不要用重跑蓋過去：找出原因，寫進 `progress.md` 的「做過的檢查」。WPT 的查過、一時修不好的，在 `.ini` 標成 known intermittent，附上註解（`docs/testing.md` 的「工具和跑法」）。結果要看 GC 什麼時候標的新測試，交審核前照 CI 的跑法重複跑上千次（同一節）。

## 程式碼

- 和周圍的程式碼保持一致：命名、註解的密度和寫法。程式碼和註解用英文；註解解釋「為什麼」，**不引用 `docs/` 的文件**。
- **文字檔一律 LF**（`.gitattributes` 的 `* text=auto eol=lf`，不管 `core.autocrlf`；只有 `tests/sites/` 照原樣保存）。寫檔的工具和腳本要寫 LF（Python 用 `newline='\n'`）。工作目錄裡的 CRLF 檔案（Windows 的編輯器存的）直接轉成 LF，不算動到別人的變更：`git add` 本來就會轉。
- 型別和模組盡量對應 Chromium 的名字和分層（`FrameView`、`LayerTreeHost`、`ScriptState`……），在註解裡寫出對應的 Chromium 類別。
- **擁有權和借用先設計好，不預設用 `Rc`、`RefCell`、`Arc`**：先想清楚誰擁有、借用能不能靜態地保證（從擁有者借、分開借欄位、綁在像 `&mut Isolate` 這樣的通行證上）。執行時的計數和借用檢查只在說得出理由時用（真的共享擁有、跨 thread、外部 API 的限制，例如 html5ever 的 `&self` sink、V8 的 finalizer），在註解寫明，而且不放在每次呼叫都會走到的路上。對照 Chromium：Blink 的 `StringImpl`、`RefCounted` 是 thread 自己的、計數不是 atomic 的。
- **整數運算和轉型寫明溢位時怎麼辦**：照 `crates/base/README.md` 的規則（大小和配置 checked、CSS 和 layout 的值飽和或 clamp、ID 用 `SequenceNumber`、float 轉整數用 `base::numerics`），證明過不會溢位的寫帶理由的 `#[expect]`，不用 `#[allow]`；clippy 和 `sanity.py numerics` 檢查。
- **`unsafe` 只在 `bindings` 和 `ipc`**，每一處都有 `// SAFETY:` 說明，改了論證要更新那個 crate 的 soundness 文件（`bindings` 是 `crates/bindings/soundness.md`；`ipc` 是它的 README 的「`unsafe` 的論證」）。elise 的 `unsafe` 照它自己的規則，論證在 `../third_party/elise/docs/soundness.md`。
- **renderer 照 process 的邊界寫**：renderer 現在是 Browser process 裡的 thread，但之後要能拆成 process。改到 renderer 和 Browser、Viz 之間的東西（`protocol`、啟動 renderer 的參數、renderer 這一側的全域狀態）前，先看 `docs/architecture.md` 的「Process 的邊界」。
- **V8 專屬的程式只在 `bindings` 和 `dom_js`**（量測用的 `gc_bench` 例外：它要量 V8 自己的 heap）；`dom_api` 和 renderer 與 engine 無關（`docs/architecture.md` 的「V8 和 `unsafe` 的位置」）。
- Toolchain 釘在 `rust-toolchain.toml`，和 workspace 的 `rust-version` 一起改；elise 釘同樣的版本，一起改（它的 compile-fail 預期輸出要重新產生，並確認錯誤的種類沒變；`docs/README.md` 的「其他會牽動文件的變更」）。

## 第三方的 fork

- `../third_party/rusty_v8`、`../third_party/parley`、`../third_party/taffy` 和 `../third_party/wasmtime` 是我們的 fork，用 `machopo` 分支；`../third_party/rusty_v8_artifacts` 是編好的 V8，大家共用。
- **parley 和 taffy 緊跟上游的 main，上游優先**：fork 上每個 commit 只做一件事、帶 fork 自己的測試，寫成可以直接送上游的樣子，在 `README.machopo.md` 的「Local modifications」標明上游的狀態（只在 fork 的寫理由）。`README.machopo.md` 是最後一個、只改它的 commit。每個 commit 也是 machopo 的 `patches/<名稱>/` 底下的一個 patch：fork 每多一個 commit，在同一次改動裡更新 `third_party.toml` 的 `rev`（rebase 到上游的新 commit 時連同 `upstream_rev`），再用 `python3 scripts/ci.py patches <名稱>` 匯出；`ci.py third-party` 檢查 patch 套在 `upstream_rev` 上和 `rev` 一樣。rebase 之前在舊的頂端打 tag（`machopo-<日期>`），machopo 舊的 commit 釘的 rev 才拿得到。
- **改 parley 或 taffy 的 fork 時，跑 fork 的測試**（它的 `README.machopo.md` 的「Tests」）和 machopo 相關的測試。taffy 沒有釘 toolchain，用 machopo 的編；要送上游的修改守住 taffy 的 `rust-version`，送出前用那個版本確認。
- `../third_party/elise` 是 DOM 的 GC，machopo 自己的專案（MIT OR Apache-2.0，wasmtime 的 fork 也用它），不是 fork，用 `main`：改 GC 在那裡改，照 `../third_party/elise/docs/contributing.md`，跑它的 `cargo test --workspace`、`scripts/sanity.py` 和改到的部分的 `scripts/test.py`（models、fuzz、verus、miri），GC 自己的測試的缺口記在它的 `docs/testing.md`。**它每多一個 commit 也在同一次改動裡更新 `third_party.toml`**（下面的規則），只釘推到它的 `main` 上的 commit（它的 CI 測過的）。
- **改 wasmtime 的 fork，或改 elise 的 `raw`（fork 接 elise 的那一層）時，跑 fork 的測試**（它的 `README.machopo.md` 的「Tests」：`cargo test --features gc-elise --test all elise::`、`python3 machopo/elise_tests.py` 和 clippy）：machopo 的 `dom_wasm` 只用到其中一部分（借 DOM 的 heap 的 Store），上游的 GC 測試、spec test 和 fuzz 用 elise 跑的結果只有 fork 的測試看得到，machopo 的 CI 不跑它們。
- `../third_party/wpt` 是上游的 WPT，不是 fork，測試直接讀它：**不要改它**。要跑哪些測試、預期結果、怎麼換 commit，都在 `tests/wpt/`（見它的 README）。
- **改 fork 要盡量小**，程式裡用 `machopo:` 註解標出來，並記在那個 fork 的 `README.machopo.md`（「Local modifications」）。fork 要用它自己 `rust-toolchain.toml` 釘住的版本也能編（有釘的話）。
- **fork 每多一個 commit（只改 `README.machopo.md` 的也算），就在同一次改動裡更新 machopo 的 `third_party.toml`**，先 push fork、再 push machopo：每台機器的 `python3 scripts/ci.py third-party`（`test.py` 的每個指令開始前也跑）把 `../third_party` 同步到 machopo 釘的 commit，fork 和 pin 對不上時會停下。
- **改了 rusty_v8 的 C++（`src/binding.cc` 等）或 V8 的設定，要重編 V8**（`python3 scripts/ci.py v8`：第一次約 40 分鐘，之後只重編改過的檔案），並告訴其他人：共用的 artifact 會跟著換。只改 Rust 的部分不用重編。

## 平台

驗收和視窗模式的手動確認只在 macOS（Apple Silicon）上做過；自動測試在開發機和每天的 CI 上跑（`docs/testing.md` 的「CI」），從原始碼編 V8 在 macOS、Linux 和 Windows 11（只有 release）上跑過。在其他平台上碰到的問題，記成 `docs/limitations.md` 的「平台和建置」。
