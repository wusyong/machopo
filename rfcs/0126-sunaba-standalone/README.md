# RFC 0126：`sunaba` 成為獨立的專案

- 開始日期：2026-11-27
- 狀態：✅ 已完成：2026-11-28（第 1 段在 Windows 11，10.0.26300，x86-64；第 2、3 段在 macOS 26，Apple Silicon；sunaba 在三個平台上由它自己的 CI 驗）
- 進度和驗收：`progress.md`
- 細節：`details.md`

## 摘要

process 的 sandbox `sunaba`（原本的 `crates/sunaba`）搬出 machopo，像 elise 一樣成為自己的 repo（`github.com/wusyong/sunaba`）：從一個新的 commit 開始（每一行的理由留在 machopo 的歷史裡），有自己的 lint、toolchain、英文的文件（soundness、contributing、testing）、`sanity.py` 和 CI（Linux、macOS、Windows 都跑）；machopo 用 path 依賴旁邊的 ../third_party/sunaba，在 `third_party.toml` 釘住它 `main` 上 CI 測過的 commit。`ipc` 照舊依賴它，`ipc` 要不要、怎麼分出去不在這份裡。第一次在三個平台上跑的 CI 找到兩件事（Linux 的 chroot 的測試看 kernel 的 errno、Windows 的 default DACL 帶著 logon session），在搬的過程中處理了。選了哪些做法見「已決定的事項」。

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
- 改 `sunaba` 的行為或 API（Linux 的 chroot 的測試接受兩種 errno 只改測試，已決定的事項第 11 條）。

## 設計概要

- `sunaba` 的程式、測試和文件在 `https://github.com/wusyong/sunaba`，machopo 的 repo 裡沒有 `crates/sunaba`。clone 下來在任何一個平台上 `cargo test` 就是它全部的測試（`sunaba_probe` 照舊是它自己的 bin）。
- machopo 這一側：`python3 scripts/ci.py third-party` 把它 clone 到 ../third_party/sunaba，workspace 的 `sunaba = { path = "../third_party/sunaba" }`；`shell` 和 `ipc` 照舊 `sunaba.workspace = true`，程式不變。
- 改 `sunaba` 的人照 elise 的做法：在 ../third_party/sunaba 改、跑它的測試和檢查、commit 並 push 到它的 `main`，等它的 CI 通過，再在同一次 machopo 的改動裡更新 `third_party.toml` 的 `rev`（先 push `sunaba`、再 push machopo）。renderer 的 policy 照舊在 machopo 改。弄壞它的 mutation test 照樣寫在 machopo 的 RFC 裡，用 `test.py mutate`。
- machopo 的 `CLAUDE.md` 要人動到 sandbox 前先讀 ../third_party/sunaba 的 `docs/soundness.md` 和 `docs/contributing.md`；machopo 自己的 `unsafe` 只在 `bindings` 和 `ipc`，elise 和 sunaba 的照它們自己的規則。

## 詳細設計

### 搬出去的方式（P1、H2，已決定的事項第 1、9 條）

**接回 machopo 的方式**：

| | 做法 | 優點 | 缺點 |
| --- | --- | --- | --- |
| **P1** | 照 elise：自己的 repo，../third_party/sunaba，path 依賴，`third_party.toml` 釘 commit | 和 elise、fork 同一套流程和工具（`ci.py third-party` 不用改）；本機改了馬上能在 machopo 裡試 | 改一次要兩個 repo、兩次 push |
| P2 | Cargo 的 git 依賴（`git = …, rev = …`） | 不用 `../third_party` 的 checkout | 本機改了要先 push 或另寫 `[patch]` 才試得到；和其他第三方的專案不一樣 |
| P3 | 發布到 crates.io | 別人最好用 | 要版本號和 API 的承諾；改一次要發一版，現在 API 還跟著 machopo 變 |
| P4 | 留在 machopo，只補自己的 CI | 最便宜 | 不是這份要的「獨立」；CI 要在 machopo 裡開 macOS、Windows 的 job |

**歷史**：H1 用 `git filter-repo --subdirectory-filter crates/sunaba` 留下它在 machopo 裡的 commit（`git blame` 看得到每一行為什麼這樣寫；commit message 裡的 RFC 編號指回 machopo）；H2 從一個新的 commit 開始（乾淨，但理由只留在 machopo 的歷史裡）。先選了 H1，第 2 段開始前改成 H2：sunaba 的第一個 commit 是 machopo 的 f0d1bf86 的 `crates/sunaba`，commit message 寫明從哪裡搬來，`docs/contributing.md` 指回 machopo 的 `git log -- crates/sunaba`。

### 補上的設定（`details.md` 的「現況」有搬之前的清單）

- `Cargo.toml`：`version`、`edition`、`rust-version`、`license`、`publish`、`repository`、`readme` 寫明（原本從 workspace 繼承：0.0.1、2024、1.99、`publish = false`、`MIT OR Apache-2.0`）；版本從 0.1.0 開始（已決定的事項第 6 條）；自己的 `[workspace]`，在哪裡 checkout 都單獨編得起來（machopo 用 path 依賴它時，它不是 machopo 的 workspace 的成員，elise 也是這樣）。
- lint：照 machopo 的 workspace lint 抄一份自己的（`unsafe_code` 和 `unsafe_op_in_unsafe_fn` 是 deny、`undocumented_unsafe_blocks`、整數運算和轉型的 lint），elise 也是這樣抄；`unexpected_cfgs` 拿掉 machopo 的 `ipc_loom` 那些。
- `rust-toolchain.toml` 釘和 machopo 一樣的版本（1.99.0），之後和 machopo、elise 一起調（`docs/README.md` 的「其他會牽動文件的變更」）。
- `clippy.toml`：只留和它有關的（`fetch_add` 的那一條；V8 的那幾條不要）。
- `.cargo/config.toml`：不要。machopo 的 Windows 的 `/NODEFAULTLIB:libcmt.lib` 和 `rust-lld` 是為了 V8；第 1 段在 machopo 外面用 MSVC 預設的連結器編過，`sunaba_probe` 的延後載入照樣連得起來，測試都通過（`progress.md`）。
- `.gitattributes`（LF）、`.gitignore`、`LICENSE-MIT`、`LICENSE-APACHE`。使用者的全域 gitignore 排除 `.gitattributes`，所以它是 `git add -f` 進去的（machopo 和 elise 的也是）。

### 提到 machopo 的地方

- `build.rs` 的「as machopo's executable does」、`soundness.md` 的兩處「machopo's tests …」：改成說用的程式要怎麼做（延後載入的程式要連 `delayimp.lib`；namespace 和延後載入的那兩項由用它的程式的測試看），machopo 只當例子。
- `__pfnDliFailureHook2`：`sunaba` 是 library，任何連結它、又用 `/DELAYLOAD` 的執行檔都拿到這個 hook；一個程式只能有一個，自己也定義了的程式連結時撞名。改成 cargo feature `delay-load-hook`、預設開著（已決定的事項第 5 條），README 的 Windows 的「Lowering」寫明。
- desktop 的名字 `sunaba-<PID>` 本來就是通用的，不改（`shell` 的測試照它斷言）。

### 新的 repo 的內容

照 elise 的樣子（路徑是新的 repo 裡的）：README.md（原本的那份，加上 Building and testing、Contributing、License；policy 由用它的程式寫，machopo 是例子）、docs/soundness.md（原本的 `soundness.md`）、docs/contributing.md（和 machopo 的關係、改一次的三步、規則：`unsafe` 和 `// SAFETY:`、soundness 的表、整合測試是 tests/it/ 的模組、LF、新的行為要有測試和 mutation test、整數運算的規則）、docs/testing.md（指令、CI、測試守不住的地方：從 machopo 的 `docs/test-gaps.md` 搬過去的那幾條，加上 soundness 寫著只靠審查、machopo 沒列的幾條）、scripts/sanity.py、.github/workflows/ci.yml。

`scripts/sanity.py` 有三項：`tests` 和 `numerics` 照 elise 的那份改成一個 crate 在根目錄；`unsafe` 檢查 `Cargo.toml` 的 lint（deny `unsafe_code` 和 `unsafe_op_in_unsafe_fn`、打開 `undocumented_unsafe_blocks`）、只有 `src/lib.rs` 打開 `unsafe_code`、`src/` 底下 code 裡有 `unsafe` 的檔案都在 `docs/soundness.md` 的表裡有一列（表裡的檔案也都在）。`// SAFETY:` 本身交給 clippy 的 `undocumented_unsafe_blocks`（block 和 impl）；extern block 和 `#[unsafe(no_mangle)]` 沒有 `// SAFETY:`，論證在 soundness 的表裡，所以 `unsafe` 檢查的是表。

文件的語言（L1，已決定的事項第 3 條）：L1 全部英文（README 和 `soundness.md` 本來就是英文的，給 machopo 以外的人讀）；L2 照 elise，`docs/` 用中文。

### CI（C1，已決定的事項第 4 條）

- **C1**：`sunaba` 的 CI 在 `ubuntu-latest`、`macos-latest`、`windows-latest` 上跑 `cargo test`、`cargo clippy --all-targets -- -D warnings`（Windows 上不帶預設的 feature 再跑一次兩者：feature 只有 Windows 用），`ubuntu-latest` 上 `cargo fmt --check` 和 `sanity.py`，每次 push、PR 和每天一次；machopo 照舊只在 `ubuntu-24.04` 上跑（`renderer_sandbox_linux` 和 `ipc` 的測試照樣經過 `sunaba`）。
- C2：machopo 的 CI 也在 macOS 和 Windows 上跑 `sunaba` 的測試（在 ../third_party/sunaba 跑）：重複，而且要為它開 machopo 的 macOS、Windows 的 job。
- Linux 的 runner 不允許沒有特權的 user namespace（Ubuntu 的 AppArmor 預設），`Namespaces` 的測試在那裡走「不能用」的那一條，和 machopo 的 CI 一樣（`docs/test-gaps.md` 已經記著）；`sunaba` 的 CI 在 runner 上打開（`sysctl kernel.apparmor_restrict_unprivileged_userns=0`），chroot 的測試也跑（已決定的事項第 8 條）。

### 第一次 CI 找到的

- **Linux：chroot 之後查路徑的 errno 看 kernel 的版本**。打開 user namespace 之後 chroot 的測試第一次在 CI 上跑：chroot 到已經結束的 thread 的 `/proc/thread-self/fdinfo` 之後，開發機的 kernel 回 `ESRCH`、runner 的回 `ENOENT`。要守的是每個路徑都一樣失敗，所以測試兩種都接受（已決定的事項第 11 條）。
- **Windows：系統給的 default DACL 看是誰啟動的**。RFC 0121 的 K1 靠系統的預設沒有 logon session（在 elevated 的 shell 裡探測的）；一般使用者的有 logon session、有使用者，CI 的 runner（UAC 關掉的管理員）有 logon session、沒有使用者。machopo 的 `main` 同一天在 `crates/sunaba` 改成自己設 lockdown 的 default DACL（RFC 0121 的第 12 條），sunaba 照搬（已決定的事項第 10 條）。runner 看得到 logon session、看不到使用者，所以 sunaba 的 CI 守得住「拿掉 logon session」和「給使用者」兩件事。

### machopo 這一側改的

- 拿掉 `crates/sunaba`；workspace 的 `Cargo.toml` 的 `sunaba` 改成 `path = "../third_party/sunaba"`，搬到「Internal crates」那一區的 elise 旁邊；`[workspace.lints.rust]` 提到 `sunaba` 的註解改掉。
- `third_party.toml` 加 `[sunaba]`（`repository`、`rev`），寫法照 `[elise]`。
- `scripts/sanity.py`：`UNSAFE_ALLOWED` 拿掉 `crates/sunaba/src/lib.rs`，訊息裡的 `sunaba` 拿掉（machopo 自己的 `unsafe` 只在 `bindings` 和 `ipc`）。`docs` 的 `GONE_CRATES` 加上 `sunaba`（路徑的比對也認沒有結尾 `/` 的 `crates/sunaba`）：RFC 引用它搬走之前的檔案、章節和測試時照當時的樣子，不檢查。`deps`、`tests`、`numerics` 是照 `crates/*` 走的，不用改。
- `scripts/test.py mutate`：machopo 以外的檔案的原檔放在 `target/mutate/originals/_outside/`（已決定的事項第 12 條），測試在 `scripts/mutate_test.py`（CI 的 lint job）。
- 文件：`CLAUDE.md`（先讀、`unsafe`、toolchain、第三方的專案那一節：sunaba 怎麼改、怎麼釘、mutation test 怎麼寫）、根目錄的 `README.md`（`../third_party/` 的樹）、`docs/README.md`（拿掉 `crates/sunaba/soundness.md` 那一列；調整 Rust 版本時 sunaba 也一起）、`docs/architecture.md`（`sunaba` 從 crate 的表搬到第三方的表，elise 旁邊；`unsafe` 的位置）、`docs/test-gaps.md`（`sunaba` 自己的缺口拿掉，它們在 sunaba 的 `docs/testing.md` 裡；renderer 的 policy 和 machopo 的 CI 的留下）、`docs/testing.md`（mutation test 的原檔）。`docs/limitations.md` 不用改。
- 已完成的 RFC 的 `mutants-*.toml`（0117、0119、0120、0121）寫的是 `crates/sunaba` 底下的路徑，照 `rfcs/README.md` 的規則不改：它們記的是當時的路徑。

### 測試

- `sunaba`：在新的 repo 的 CI 上，三個平台的 `cargo test`、clippy、fmt、`sanity.py` 都通過；第 1 段在 Windows 上照新的 repo 的樣子單獨跑過一次（不在 machopo 的 workspace 裡）；`sanity.py` 在一份複本裡弄壞過。
- machopo：`python3 scripts/ci.py third-party` 從空的 `../third_party` clone 得出 `sunaba`；`cargo test --workspace`、fmt、clippy、`sanity.py` 的七項通過。
- Mutation test：延後載入的 hook 的 feature（`mutants-1.toml`，由 `shell` 的 `renderer_sandbox_windows` 抓）和 `test.py mutate` 的原檔（`mutants-3.toml`）。default DACL 的是 RFC 0121 在 Windows 上跑過的那一份（sunaba 照搬同一段程式）。結果在 `progress.md`。

### 分階段

1. **在 machopo 裡先切乾淨**：`crates/sunaba` 的 `Cargo.toml` 不再從 workspace 繼承（lint 除外：留到搬出去時才換成自己的那份，不然 machopo 的 `sanity.py unsafe` 要它 `[lints] workspace = true`）、改掉提到 machopo 的地方、補 `__pfnDliFailureHook2` 的說明；把它複製到 machopo 外面的空目錄，加上 toolchain、lint、clippy 的設定，單獨 `cargo test` 通過。
2. **建 repo、搬出去**：使用者在 GitHub 建 repo（對外的動作，由使用者做或確認）；從第 1 段做完時的 `crates/sunaba` 開始一個新的 commit，加上第 1 段試過的設定、文件、`sanity.py`、CI，push，等 CI 在三個平台上通過。
3. **machopo 改接**：照上一節改 `Cargo.toml`、`third_party.toml`、`sanity.py`、`test.py mutate` 和文件，拿掉 `crates/sunaba`；跑測試和檢查。

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
2. ~~**保留歷史（H1）**~~（2026-11-28 改成 H2，第 9 條）：用 `git filter-repo --subdirectory-filter crates/sunaba` 帶出它在 machopo 裡的 commit。理由：`git blame` 看得到每一行的理由。不選從新的 commit 開始（H2：理由只留在 machopo 的歷史裡）。
3. **文件全部英文（L1）**：README、soundness 和新的 contributing、testing 都是英文。理由：README 和 `soundness.md` 本來就是英文的，要給 machopo 以外的人讀。不選照 elise 的中文 docs/（L2）。
4. **CI 在三個平台上跑（C1）**：`sunaba` 的 CI 在 `ubuntu-latest`、`macos-latest`、`windows-latest` 上跑 `cargo test`、clippy（`-D warnings`）、`cargo fmt --check` 和它的 `sanity.py`，每次 push 都跑；machopo 的 CI 照舊只在 `ubuntu-24.04` 上。理由：macOS 和 Windows 的程式第一次有 CI，machopo 不用為它開新的平台。不選讓 machopo 的 CI 也跑它的測試（C2：重複）。

5. **延後載入失敗的 hook 是預設開著的 feature（D2）**（2026-11-27，第 1 段開始前，使用者確認）：`__pfnDliFailureHook2` 和它呼叫的 `delay_load_failed` 只在 `delay-load-hook` 這個 feature 開著時定義，`default = ["delay-load-hook"]`。理由：一個程式只能有一個這個變數，library 自己定義時，已經有自己的 hook 的程式連結不起來；預設開著，machopo、`ipc` 和 `sunaba` 自己的測試都不用改。不選照舊、只在 README 寫明（D1：那種程式用不了 `sunaba`）；不選照 Chromium 由執行檔自己定義（D3：`chrome.exe` 的 hook 在 `chrome/app`，但 machopo 的 `shell` 就要寫 `#[unsafe(no_mangle)]`，違反 `unsafe` 只在 `bindings`、`ipc` 和 `sunaba`）。
6. **版本從 0.1.0 開始**（2026-11-27，同上）：從 workspace 繼承的 0.0.1 是 machopo 的佔位；`publish` 照舊是 `false`（不發布到 crates.io，第 1 條）。

2026-11-28，第 2 段開始前，使用者確認：

7. **repo 是 `github.com/wusyong/sunaba`**（公開），授權照 machopo 的 `MIT OR Apache-2.0`：使用者已經在 GitHub 建好（空的）。`Cargo.toml` 的 `repository` 和 `third_party.toml` 的 `repository` 都寫這個網址，大小寫一致（搬 elise 時留下三種寫法，`details.md`）。
8. **Linux 的 CI 打開沒有特權的 user namespace**：Linux 的 job 跑測試前先 `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`。理由：不打開時 chroot 的兩個測試在 runner 上直接 return，看不出來沒跑；打開之後沒有權限的那一條照樣由 `a_chroot_is_made_only_where_the_process_may` 守著（它不經過 `unshare`），所以一個 Linux 的 job 就夠。不選照舊（chroot 只在開發機上測得到），也不選開兩個 Linux 的 job（多出來的那個測不到新的東西）。
9. **歷史從一個新的 commit 開始（H2）**，取代第 2 條：新的 repo 的第一個 commit 是第 1 段做完時的 `crates/sunaba`（machopo 的 f0d1bf86）加上新的設定和文件，commit message 寫明它是從 machopo 的哪個 commit 搬來的；每一行的理由照舊在 machopo 的 `git log -- crates/sunaba` 裡。不選 H1（`git filter-repo` 帶出歷史）。

2026-11-28，第一次 CI 之後（`progress.md` 的第 2 步），使用者照看法確認：

10. **sunaba 的 repo 用 machopo 的 `main` 上的 default DACL 的修正**（2026-11-28 改寫）：sunaba 第一次的 CI 在 Windows 上看到 sandbox 裡的 process 的 default DACL 有 logon session；同一天 machopo 的 `main` 已經在 `crates/sunaba` 修好（fc17f43d，RFC 0121 的已決定的事項第 12 條：K2，照 Chromium 的 `SetLockdownDefaultDacl`），在 Windows 上用一般使用者跑過測試和 mutation test。sunaba 的 repo 照搬它（sunaba 的 7973001），不用第 2 段自己寫的那一版（sunaba 的 56f8237：建好 AppContainer 的 token 之後才改，第一個 thread 的 token 沒有測試）。程式和 machopo 的 `crates/sunaba` 一樣，所以不是這份 RFC 改了 sunaba 的行為。
11. **chroot 之後查路徑的 errno 接受 `ESRCH` 或 `ENOENT`**：只改測試和文件。chroot 到已經結束的 thread 的 `/proc/thread-self/fdinfo`，查路徑失敗回哪一個看 kernel 的版本（開發機 `ESRCH`、runner `ENOENT`）；要守的是「每個路徑都一樣失敗」，Chromium 的同一個做法也不看 errno。

2026-11-28，第 3 段開始前，使用者確認：

12. **`test.py mutate` 把 machopo 以外的檔案的原檔存在 `target/mutate/originals/_outside/` 底下（M1）**：spec 的 `file` 寫 ../third_party/sunaba 底下的路徑、`test` 用 `--manifest-path` 本來就能跑（RFC 0094 弄壞 elise 時這樣做過），但原檔存在 `originals/` 加上 `file` 的位置，`../` 把它帶到 `originals/` 外面：跑到一半中斷時，「上一次沒跑完」的檢查和 `--restore` 都找不到它，`../third_party` 的檔案留著壞的。改成 machopo 以外的檔案存在 `originals/_outside/<相對於 machopo 的上一層的路徑>`，`--restore` 照那個路徑放回去；machopo 裡的檔案照舊。理由：改得少，elise 的也一起修好，改 `sunaba` 的 mutation test 照樣寫在 machopo 的 RFC 裡、可以用 machopo 的測試抓（例如 `renderer_sandbox_windows`）。不選只在文件寫明不要中斷（M2：漏洞還在）、`sunaba` 自己寫一份工具（M3：兩份要一起維護，跨 repo 的測試做不到）。

## 待決定

沒有。

## 之後的可能

- 發布到 crates.io（API 穩定之後）。
- sunaba 的 `docs/testing.md` 等著它獨立的：`SIGSYS` 的 handler 用 sanitizer 或靜態檢查。
- `ipc` 拿掉對 `sunaba` 的依賴（Chromium 的分法：`ipc` 只準備要給 child 的 handle 和環境，由用的一方用 `sunaba` 啟動），之後再決定要不要分出去。
