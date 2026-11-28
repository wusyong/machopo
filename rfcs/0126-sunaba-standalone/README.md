# RFC 0126：`sunaba` 成為獨立的專案

- 開始日期：2026-11-27
- 狀態：🚧 已排程：2026-11-27
- 進度和驗收：`progress.md`
- 細節：`details.md`

## 摘要

把 process 的 sandbox `sunaba`（`crates/sunaba`）搬出 machopo，像 elise 一樣成為自己的 repo：保留它在 machopo 裡的歷史，有自己的 CI（Linux、macOS、Windows 都跑）、測試的缺口和修改的規則；machopo 用 path 依賴旁邊的 ../third_party/sunaba，在 `third_party.toml` 釘住它 `main` 上 CI 測過的 commit。`ipc` 照舊依賴它，`ipc` 要不要、怎麼分出去不在這份裡。選了哪些做法見「已決定的事項」。

## 動機

- **`sunaba` 的 macOS 和 Windows 的程式在 CI 上沒有人編、沒有人測**：machopo 的 CI 每個 job 都在 `ubuntu-24.04` 上跑（`.github/workflows/ci.yml`），`sunaba` 的 `macos.rs`、`windows.rs`（加起來約 3,300 行，`unsafe` 最多的地方）只在開發機上測過。自己的 repo 的 CI 可以只為它開三個平台（elise 的 CI 就是這樣），不用讓 machopo 的整套在 macOS 和 Windows 上跑。
- **它已經照獨立的專案寫了，只差搬出去**：不依賴 machopo 的任何 crate，依賴的版本都自己寫（`Cargo.toml` 的註解），README 和 `soundness.md` 是英文的，只講機制、不講任何程式的 policy（Chromium 的 `sandbox/` 和 `sandbox/policy/` 的分法；machopo 的 policy 在 `shell` 的 `renderer_sandbox`）。要搬的主要是從 workspace 繼承的設定（版本、edition、lint、toolchain、授權）和少數提到 machopo 的地方（`details.md` 的「現況」）。
- **`docs/test-gaps.md` 有等著它獨立的項目**：`SIGSYS` 的 handler 只做 async-signal-safe 的事，「`sunaba` 獨立之後用 sanitizer 或靜態檢查」；另外幾條 `sunaba` 自己的缺口（KsecDD 的 handle 的填補、module tampering protection、`one_loader_thread` 的防呆、macOS 和 Windows 的 `environment`），寫在 machopo 的文件裡，其實是 `sunaba` 的事。
- **別的程式可以用**：要把 child 關進 sandbox 的 Rust 程式（嵌 wasmtime 的、處理別人的檔案的）現在要整個 machopo 的 repo 才拿得到它。

**目標**

- `sunaba` 有自己的 repo、README、授權的檔案、CI、測試的缺口和參與開發的規則；在三個平台上單獨 `cargo test` 都通過。
- machopo 改用 ../third_party/sunaba，行為不變：`shell` 的 `renderer_sandbox`、`ipc` 的 `platform` 的程式不改，`renderer_sandbox_*` 的測試照樣通過。
- machopo 的文件（`CLAUDE.md`、`docs/`、根目錄的 `README.md`）和檢查（`sanity.py`、`ci.py third-party`）照新的位置改。

**不在範圍內**

- `ipc` 分出去，或拿掉 `ipc` 對 `sunaba` 的依賴（`ipc::platform` 現在直接露出 `sunaba::windows::Policy`、`Child`、`sunaba::linux::Namespaces`）：使用者之後再想（2026-11-27）。
- 發布到 crates.io、穩定 API、版本號的規則：之後的可能。
- 改 `sunaba` 的行為或 API。

## 設計概要

做完之後：

- `sunaba` 的程式、測試和文件在 `https://github.com/wusyong/sunaba`（名稱待決定），machopo 的 repo 裡沒有 `crates/sunaba`。clone 下來在任何一個平台上 `cargo test` 就是它全部的測試（`sunaba_probe` 照舊是它自己的 bin）。
- machopo 這一側：`python3 scripts/ci.py third-party` 把它 clone 到 ../third_party/sunaba，workspace 的 `sunaba = { path = "../third_party/sunaba" }`；`shell` 和 `ipc` 照舊 `sunaba.workspace = true`。
- 改 `sunaba` 的人照 elise 的做法：在 ../third_party/sunaba 改、跑它的測試和檢查、commit 並 push 到它的 `main`，等它的 CI 通過，再在同一次 machopo 的改動裡更新 `third_party.toml` 的 `rev`（先 push `sunaba`、再 push machopo）。renderer 的 policy 照舊在 machopo 改。
- `CLAUDE.md` 要人動到 `sunaba` 前先讀 `crates/sunaba/soundness.md` 的那一句，改成讀 ../third_party/sunaba 裡的那一份；「`unsafe` 只在 `bindings`、`ipc` 和 `sunaba`」裡的 `sunaba` 改成照它自己的規則（elise 的寫法）。

## 詳細設計

### 搬出去的方式（P1、H1，已決定的事項第 1、2 條）

**接回 machopo 的方式**：

| | 做法 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **P1** | 照 elise：自己的 repo，../third_party/sunaba，path 依賴，`third_party.toml` 釘 commit | 和 elise、fork 同一套流程和工具（`ci.py third-party` 不用改）；本機改了馬上能在 machopo 裡試 | 改一次要兩個 repo、兩次 push |
| P2 | Cargo 的 git 依賴（`git = …, rev = …`） | 不用 `../third_party` 的 checkout | 本機改了要先 push 或另寫 `[patch]` 才試得到；和其他第三方的專案不一樣 |
| P3 | 發布到 crates.io | 別人最好用 | 要版本號和 API 的承諾；改一次要發一版，現在 API 還跟著 machopo 變 |
| P4 | 留在 machopo，只補自己的 CI | 最便宜 | 不是這份要的「獨立」；CI 要在 machopo 裡開 macOS、Windows 的 job |

**歷史**：H1 用 `git filter-repo --subdirectory-filter crates/sunaba` 留下它在 machopo 裡的 commit（`git blame` 看得到每一行為什麼這樣寫；commit message 裡的 RFC 編號指回 machopo）；H2 從一個新的 commit 開始（乾淨，但理由只留在 machopo 的歷史裡）。選了 H1。

### 要補的設定（`details.md` 的「現況」有完整的清單）

- `Cargo.toml`：`version`、`edition`、`rust-version`、`license`、`publish` 寫明（現在從 workspace 繼承：0.0.1、2024、1.99、`publish = false`、`MIT OR Apache-2.0`）；版本從 0.1.0 開始（已決定的事項第 6 條）；`repository` 等第 2 段 repo 的名字定了再加。
- lint：照 machopo 的 workspace lint 抄一份自己的（`unsafe_code` 和 `unsafe_op_in_unsafe_fn` 是 deny、`undocumented_unsafe_blocks`、整數運算和轉型的 lint），elise 也是這樣抄；`unexpected_cfgs` 拿掉 machopo 的 `ipc_loom` 那些。
- `rust-toolchain.toml` 釘和 machopo 一樣的版本（1.99.0），之後和 machopo、elise 一起調（`docs/README.md` 的「其他會牽動文件的變更」加上它）。
- `clippy.toml`：只留和它有關的（`fetch_add` 的那一條；V8 的那幾條不要）。
- `.cargo/config.toml`：不要。machopo 的 Windows 的 `/NODEFAULTLIB:libcmt.lib` 和 `rust-lld` 是為了 V8；第 1 段在 machopo 外面用 MSVC 預設的連結器編過，`sunaba_probe` 的延後載入照樣連得起來，測試都通過（`progress.md`）。
- `.gitattributes`（LF）、`.gitignore`、`LICENSE-MIT`、`LICENSE-APACHE`。

### 提到 machopo 的地方

- `build.rs` 的「as machopo's executable does」、`soundness.md` 的兩處「machopo's tests …」：改成說用的程式要怎麼做（延後載入的程式要連 `delayimp.lib`；namespace 和延後載入的那兩項由用它的程式的測試看），machopo 只當例子。
- `__pfnDliFailureHook2`：`sunaba` 是 library，任何連結它、又用 `/DELAYLOAD` 的執行檔都拿到這個 hook；一個程式只能有一個，自己也定義了的程式連結時撞名。改成 cargo feature `delay-load-hook`、預設開著（已決定的事項第 5 條），README 的 Windows 的「Lowering」寫明。
- desktop 的名字 `sunaba-<PID>` 本來就是通用的，不改（`shell` 的測試照它斷言）。

### 新的 repo 的內容

照 elise 的樣子（路徑是新的 repo 裡的）：README.md（現在的那份，加上 Building and testing、License）、docs/soundness.md（現在的 `soundness.md`）、docs/contributing.md（規則：`unsafe` 和 `// SAFETY:`、soundness 的表、整合測試是 tests/it/ 的模組、LF、新的行為要有測試和 mutation test、和 machopo 的關係）、docs/testing.md（測試守不住的地方：從 machopo 的 `docs/test-gaps.md` 搬過去的那幾條）、scripts/sanity.py（`tests`、`numerics`、`unsafe` 的 `// SAFETY:`）、.github/workflows/ci.yml。

文件的語言（L1，已決定的事項第 3 條）：L1 全部英文（README 和 `soundness.md` 本來就是英文的，給 machopo 以外的人讀）；L2 照 elise，`docs/` 用中文。

### CI（C1，已決定的事項第 4 條）

- **C1**：`sunaba` 的 CI 在 `ubuntu-latest`、`macos-latest`、`windows-latest` 上跑 `cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`sanity.py`，每次 push 都跑；machopo 照舊只在 `ubuntu-24.04` 上跑（`renderer_sandbox_linux` 和 `ipc` 的測試照樣經過 `sunaba`）。
- C2：machopo 的 CI 也在 macOS 和 Windows 上跑 `sunaba` 的測試（在 ../third_party/sunaba 跑）：重複，而且要為它開 machopo 的 macOS、Windows 的 job。
- Linux 的 runner 不允許沒有特權的 user namespace（Ubuntu 的 AppArmor 預設），`Namespaces` 的測試在那裡走「不能用」的那一條，和 machopo 的 CI 一樣（`docs/test-gaps.md` 已經記著）；之後可以在 runner 上打開（`sysctl kernel.apparmor_restrict_unprivileged_userns=0`）讓兩條都跑：待決定。

### machopo 這一側要改的

- 拿掉 `crates/sunaba`；workspace 的 `Cargo.toml` 的 `sunaba` 改成 `path = "../third_party/sunaba"`，搬到「Internal crates」那一區的 elise 旁邊；`[workspace.lints.rust]` 提到 `sunaba` 的註解改掉。
- `third_party.toml` 加 `[sunaba]`（`repository`、`rev`），寫法照 `[elise]`。
- `scripts/sanity.py`：`UNSAFE_ALLOWED` 拿掉 `crates/sunaba/src/lib.rs`，訊息裡的 `sunaba` 拿掉（machopo 自己的 `unsafe` 只在 `bindings` 和 `ipc`）。`deps`、`tests`、`numerics` 是照 `crates/*` 走的，不用改。
- 文件：`CLAUDE.md`（先讀、`unsafe`、第三方的專案那一節）、根目錄的 `README.md`（第三方的專案）、`docs/README.md`（`crates/sunaba/soundness.md` 那一列改成指 ../third_party/sunaba）、`docs/architecture.md`（crate 的表、`unsafe` 的位置、第三方的選型照 elise 那一列寫）、`docs/test-gaps.md`（`sunaba` 自己的缺口搬走，renderer 的 policy 的留下）、`docs/limitations.md`（只確認，看法是不用改）。
- 已完成的 RFC 的 `mutants-*.toml`（0117、0119、0120、0121）寫的是 `crates/sunaba/...`，照 `rfcs/README.md` 的規則不改：它們記的是當時的路徑。

### 測試

- `sunaba`：在新的 repo 的 CI 上，三個平台的 `cargo test`、clippy、fmt、`sanity.py` 都通過；本機在 Windows 上照新的 repo 的樣子單獨跑一次（不在 machopo 的 workspace 裡）。
- machopo：`python3 scripts/ci.py third-party` 從空的 `../third_party` clone 得出 `sunaba`；`cargo test -p shell -p ipc -p browser` 和 `renderer_sandbox_*` 的測試通過；`sanity.py` 的七項通過；`cargo test --workspace`、fmt、clippy（搬完的階段）。
- 程式不改，所以不用 mutation test；只確認 `test.py mutate` 的 spec 能不能指 ../third_party/sunaba 的檔案（之後改 `sunaba` 時要用；待決定）。

### 分階段

1. **在 machopo 裡先切乾淨**：`crates/sunaba` 的 `Cargo.toml` 不再從 workspace 繼承（lint 除外：留到搬出去時才換成自己的那份，不然 machopo 的 `sanity.py unsafe` 要它 `[lints] workspace = true`）、改掉提到 machopo 的地方、補 `__pfnDliFailureHook2` 的說明；把它複製到 machopo 外面的空目錄，加上 toolchain、lint、clippy 的設定，單獨 `cargo test` 通過。
2. **建 repo、搬出去**：使用者在 GitHub 建 repo（對外的動作，由使用者做或確認）；`git filter-repo` 帶出歷史，加上第 1 段試過的設定、文件、`sanity.py`、CI，push，等 CI 在三個平台上通過。
3. **machopo 改接**：照上一節改 `Cargo.toml`、`third_party.toml`、`sanity.py` 和文件，拿掉 `crates/sunaba`；跑測試和檢查。

## 缺點

- 改 `sunaba` 的 RFC（0117、0119、0120、0121 都大改過它）之後每一步要動兩個 repo：先改、測、push `sunaba`，等它的 CI，再更新 machopo 的 `rev`；mutation test 和審查也分在兩邊。
- machopo 的 `sanity.py` 管不到它了：`numerics`、`tests` 和 `unsafe` 的規則要在 `sunaba` 自己的 `sanity.py` 裡維護一份，和 machopo 的可能慢慢不一樣。
- 多一個要釘 toolchain、跟著調版本的專案。

## 理由和其他做法

- 只補 CI、不搬（P4）也能解決最大的問題（macOS 和 Windows 的程式沒有 CI），但 machopo 的 CI 要多開兩個平台的 job，而它們只為了一個 crate；別人也還是拿不到它。
- 和 `ipc` 合成一個專案、或一起搬：見「先例」的 Chromium；`ipc` 的 API 接下來還會跟著 site isolation 變（使用者 2026-11-27 決定之後再想）。

## 先例

- **elise**（machopo 的 DOM 的 GC，2026-10-31 從 machopo 的 `gc` crate 分出去）：同樣是 machopo 自己的專案、path 依賴加 `third_party.toml` 的釘法、自己的 CI 和 `sanity.py`。照它做；不照它的是文件的語言（L1）。
- **Chromium**：`//sandbox` 在 Chromium 的 tree 裡，不依賴 `//mojo`，各種 process 的 policy 在 `//sandbox/policy`；用的一方在 `//content` 啟動 child 的地方（`ChildProcessLauncherHelper`）把 sandbox 和 IPC 接起來。Chromium 外面也有人用這個機制：Firefox 的 Windows 的 sandbox 是 Chromium 的 `sandbox/win` 的一份複本（`security/sandbox/chromium`）。
- **Servo**：它的 sandbox 是獨立的 crate `gaol`（自己的 repo），Servo 依賴它。

## 已決定的事項

2026-11-27，使用者照草稿的看法確認：

1. **照 elise 接回 machopo（P1）**：`sunaba` 是自己的 repo，machopo 用 path 依賴 ../third_party/sunaba，`third_party.toml` 釘它 `main` 上 CI 測過的 commit，`ci.py third-party` 照 elise 的那一條路 clone 和切換。理由：和 elise、fork 同一套流程和工具，本機改了馬上能在 machopo 裡試。不選 Cargo 的 git 依賴（P2：本機的改動要先 push 或另寫 `[patch]` 才試得到，和其他第三方的專案不一樣）、crates.io（P3：要版本號和 API 的承諾，API 還跟著 machopo 變）、不搬只補 CI（P4：不是要的獨立，machopo 的 CI 要為一個 crate 開兩個平台的 job）。
2. **保留歷史（H1）**：用 `git filter-repo --subdirectory-filter crates/sunaba` 帶出它在 machopo 裡的 commit。理由：`git blame` 看得到每一行的理由。不選從新的 commit 開始（H2：理由只留在 machopo 的歷史裡）。
3. **文件全部英文（L1）**：README、soundness 和新的 contributing、testing 都是英文。理由：README 和 `soundness.md` 本來就是英文的，要給 machopo 以外的人讀。不選照 elise 的中文 docs/（L2）。
4. **CI 在三個平台上跑（C1）**：`sunaba` 的 CI 在 `ubuntu-latest`、`macos-latest`、`windows-latest` 上跑 `cargo test`、clippy（`-D warnings`）、`cargo fmt --check` 和它的 `sanity.py`，每次 push 都跑；machopo 的 CI 照舊只在 `ubuntu-24.04` 上。理由：macOS 和 Windows 的程式第一次有 CI，machopo 不用為它開新的平台。不選讓 machopo 的 CI 也跑它的測試（C2：重複）。

5. **延後載入失敗的 hook 是預設開著的 feature（D2）**（2026-11-27，第 1 段開始前，使用者確認）：`__pfnDliFailureHook2` 和它呼叫的 `delay_load_failed` 只在 `delay-load-hook` 這個 feature 開著時定義，`default = ["delay-load-hook"]`。理由：一個程式只能有一個這個變數，library 自己定義時，已經有自己的 hook 的程式連結不起來；預設開著，machopo、`ipc` 和 `sunaba` 自己的測試都不用改。不選照舊、只在 README 寫明（D1：那種程式用不了 `sunaba`）；不選照 Chromium 由執行檔自己定義（D3：`chrome.exe` 的 hook 在 `chrome/app`，但 machopo 的 `shell` 就要寫 `#[unsafe(no_mangle)]`，違反 `unsafe` 只在 `bindings`、`ipc` 和 `sunaba`）。
6. **版本從 0.1.0 開始**（2026-11-27，同上）：從 workspace 繼承的 0.0.1 是 machopo 的佔位；`publish` 照舊是 `false`（不發布到 crates.io，第 1 條）。

## 待決定

1. repo 的名字（`sunaba`）和位置（`github.com/wusyong/sunaba`）；授權照 machopo 的 `MIT OR Apache-2.0`。第 2 段開始前，由使用者在 GitHub 建 repo 時定。
2. Linux 的 runner 要不要打開沒有特權的 user namespace（`Namespaces` 的兩條路都跑）：第 2 段寫 CI 時決定。
3. `test.py mutate` 要不要支援 ../third_party 底下的檔案（elise 的 mutation test 現在怎麼跑，先查）：第 3 段之前。

## 之後的可能

- 發布到 crates.io（API 穩定之後）。
- `docs/test-gaps.md` 等著它獨立的：`SIGSYS` 的 handler 用 sanitizer 或靜態檢查。
- `ipc` 拿掉對 `sunaba` 的依賴（Chromium 的分法：`ipc` 只準備要給 child 的 handle 和環境，由用的一方用 `sunaba` 啟動），之後再決定要不要分出去。
