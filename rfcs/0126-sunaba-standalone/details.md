# RFC 0126 的細節：`sunaba` 成為獨立的專案

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 現況

2026-11-27，machopo 的 `main`（180c224a）。

### `crates/sunaba` 本身

- 程式約 5,200 行：`lib.rs`；`linux.rs` 和 `linux/` 的 `names.rs`、`namespaces.rs`、`probe.rs`、`sigsys.rs`；`macos.rs`、`macos/probe.rs`；`windows.rs`、`windows/handles.rs`、`windows/probe.rs`；`bin/sunaba_probe.rs`（測試啟動、在 policy 底下試一件事的程式，從 `src/bin/` 自動找到，`Cargo.toml` 沒有 `[[bin]]`）。
- 測試：`crates/sunaba/tests/it/` 的 `linux.rs`、`macos.rs`、`windows.rs`（一個 binary），用 `env!("CARGO_BIN_EXE_sunaba_probe")` 和 `CARGO_TARGET_TMPDIR`；Linux 的 chroot 的測試要 util-linux 的 `unshare`、`setpriv`。
- `Cargo.toml`：`description` 有；`version`、`edition`、`rust-version`、`publish`、`license` 都是 `.workspace = true`（0.0.1、2024、1.99、`publish = false`、`MIT OR Apache-2.0`）；`[lib] doctest = false`；`[lints] workspace = true`。依賴的版本都自己寫，不經過 workspace：Linux 的 `landlock 0.4.8`、`seccompiler 0.5.0`、`libc 0.2`，macOS 的 `libc 0.2`，Windows 的 `windows-sys 0.61`（自己的 feature 清單）。沒有 `repository`、`readme`、`keywords`，crate 裡沒有授權的檔案。
- `build.rs`：只在 windows-msvc，讓 `sunaba_probe` 延後載入 `user32.dll`（加 `delayimp.lib`），只對它自己的 bin。
- `lib.rs`：`#![allow(unsafe_code, reason = …)]`；測試裡打開整數運算的 lint（`cfg_attr(test)`）。
- 提到 machopo 的地方：`build.rs` 的註解、`crates/sunaba/soundness.md` 的「What the tests check, and don't」的兩處（namespace 的 chroot 由 machopo 的測試看、延後載入失敗的 hook 由 machopo 的測試看）。

### 從 workspace 繼承、搬出去要自己寫的

- `[workspace.package]` 的五項（上一節）。
- `[workspace.lints]`：`unexpected_cfgs`（裡面有 machopo 的 `ipc_loom`、`ipc_shuttle`、`verus_only`）、`unsafe_op_in_unsafe_fn = "deny"`、`unsafe_code = "deny"`；clippy 的 `undocumented_unsafe_blocks`、`arithmetic_side_effects` 和三個 `cast_*`。
- 根目錄的 `clippy.toml`（`fetch_add` 的 `disallowed-methods`，和 V8 的幾條）、`rust-toolchain.toml`（1.99.0，clippy、rustfmt）、profile（`dev` 的 opt-level 1 和 line-tables-only，`dev.package."*"` 的 opt-level 2）、`.cargo/config.toml`（windows-msvc 的 `/NODEFAULTLIB:libcmt.lib` 和 `linker = "rust-lld"`）、`.gitattributes`（`eol=lf`）、`LICENSE-MIT`、`LICENSE-APACHE`。

### 誰用它

- `ipc`（Linux 和 Windows）：`platform/mod.rs` 露出 `sunaba::windows::Child`、`sunaba::linux::Namespaces as Sandbox`、`sunaba::windows::Policy as Sandbox`，在 `pre_exec` 裡呼叫 `Namespaces::enter`；`platform/windows/channel.rs` 用 `sunaba::windows::Command` 啟動 renderer。`browser` 經過 `ipc::platform::Sandbox` 間接用。
- `shell`（三個平台）：`renderer_sandbox.rs` 照平台建 policy、`enter`、`lower`、`probe::attempt_and_exit`、`physical_memory`；測試（`renderer_sandbox_windows.rs`、`renderer_sandbox_macos.rs`、`automation_pipe.rs`）從外面看 renderer：token、integrity、mitigation、desktop 的名字（`sunaba-` 開頭）、handle、`pShimData`、環境、`open` 和 `running`。
- 其他 crate 沒有。
- `__pfnDliFailureHook2`（`windows.rs`，`#[unsafe(no_mangle)]` 的 static）：連結 `sunaba`、又用 `/DELAYLOAD` 的執行檔，延後載入的 helper 都拿到它；machopo 的執行檔是唯一的一個（`shell` 的 `build.rs` 把 17 個 DLL 連成延後載入）。

### machopo 的檢查和文件裡的 `sunaba`

- `scripts/sanity.py`：`UNSAFE_ALLOWED` 有 `crates/sunaba/src/lib.rs`，`unsafe` 的訊息提到它；`unsafe` 也要每個 `crates/*/Cargo.toml` 有 `[lints] workspace = true`。`deps`、`tests`、`numerics`、`boundary` 照 `crates/*` 走，沒有它的專屬設定；`docs` 讀 `crates/*/*.md`。
- `scripts/test.py`、`scripts/ci.py`、量測的腳本、`.github/workflows/`：沒有提到它；CI 經過 `cargo clippy --workspace`、`cargo test --workspace` 和 `sanity.py` 間接跑它，都在 `ubuntu-24.04` 上。
- 文件：`CLAUDE.md`（先讀它的 `soundness.md`、`sanity.py unsafe`、`unsafe` 的位置）、`docs/README.md`（`crates/sunaba/soundness.md` 那一列）、`docs/architecture.md`（crate 的表、`unsafe` 的位置、Linux 的 namespace）、`docs/limitations.md`（兩處提到）、`docs/test-gaps.md`（約二十條，一部分是 `sunaba` 自己的缺口，一部分是 renderer 的 policy 的；`SIGSYS` 的那一條寫「`sunaba` 獨立之後」）。
- 已完成的 RFC 0117、0119、0120、0121 的 `mutants-*.toml` 寫 `crates/sunaba` 底下的路徑和 `cargo test -p sunaba`。

### elise 怎麼接在 machopo 上（照著做的樣子）

- `third_party.toml` 的 `[elise]`：`repository`、`rev`，沒有 `upstream`、`upstream_rev`、`archive`；註解說它是 machopo 自己的專案、釘的是它 CI 測過的 `main` 上的 commit。
- workspace 的 `Cargo.toml`：`elise = { path = "../third_party/elise/crates/elise" }`，在「Internal crates」那一區；沒有 `[patch]`。
- `ci.py third-party`：沒有的就 shallow clone 到 `rev`，有的就 fetch、切到 `rev`（有自己的改動、或 HEAD 是 origin 沒有的 commit 時不動）；elise 沒有 patch，所以只做這些。`test.py` 每個要編的指令前跑它，CI 的 setup 也跑。
- elise 的 repo：自己的 workspace（`[workspace.package]`、從 machopo 抄來再調過的 `[workspace.lints]`、profile）、`rust-toolchain.toml`（和 machopo 同一版）、`clippy.toml`、`.gitattributes`、`.gitignore`、`LICENSE-*`、英文的 `README.md`、中文的 `docs/`（`soundness.md`、`api.md`、`contributing.md`、`testing.md`、`design.md`、`benchmark.md`）、`scripts/sanity.py`（`tests`、`numerics`）和 `scripts/test.py`（models、fuzz、verus、miri）、`.github/workflows/`（`ci.yml` 在 ubuntu、macos、windows 上 `cargo test --workspace`，lint 的 job；`scheduled.yml` 每天和每週的）。
- 搬 elise 時留下的不一致（之後搬 `sunaba` 時避開）：repository 的網址三處寫法不同（`third_party.toml` 的 `wusyong/elise`、elise 的 `Cargo.toml` 的 `wusyong/Elise`、`../third_party/elise/docs/contributing.md` 指向 machopo 的舊位置）。

## 參考資料

- elise 的 `../third_party/elise/docs/contributing.md`（和 machopo 的關係、規則）和 `../third_party/elise/.github/workflows/ci.yml`。
- Chromium：`sandbox/`、`sandbox/policy/`、`content/browser/child_process_launcher_helper*.cc`。
- Firefox：`security/sandbox/chromium/`（Chromium 的 Windows 的 sandbox 的複本）。
- Servo：`gaol`（https://github.com/servo/gaol）。
