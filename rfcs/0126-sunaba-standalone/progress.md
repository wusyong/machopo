# RFC 0126 的進度：`sunaba` 成為獨立的專案

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 在 machopo 裡先切乾淨：`Cargo.toml` 不再從 workspace 繼承（lint 除外）、改掉提到 machopo 的地方、延後載入失敗的 hook；複製到外面單獨編和測；已決定的事項第 5、6 條 | 做完 | 2026-11-27 |
| 2. 建 repo、搬出去：帶出歷史，設定、文件、`sanity.py`、三個平台的 CI | 還沒開始 | |
| 3. machopo 改接：`Cargo.toml`、`third_party.toml`、`sanity.py`、文件，拿掉 `crates/sunaba` | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `sunaba` 的版本、edition、`rust-version`、授權、`publish` 寫在自己的 `Cargo.toml`，不從 machopo 的 workspace 繼承（lint 除外，第 3 段換） | `crates/sunaba/Cargo.toml`；machopo 外面的複本單獨編得起來（下一列） |
| 在 machopo 外面單獨編和測：自己的 lint、`rust-toolchain.toml`（1.99.0）、`clippy.toml`，沒有 `.cargo/config.toml` | 複本（本機，不 commit）裡的 `cargo test`、`cargo test --no-default-features`、兩種 feature 的 `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` |
| 延後載入失敗的 hook 只在 `delay-load-hook` 開著時定義，預設開著 | `cargo test -p shell --test it renderer_sandbox_windows::` 看得到 hook 的那一行；`mutants-1.toml` |
| 關掉 `delay-load-hook` 照樣編得起來、沒有警告 | `cargo clippy -p sunaba --no-default-features --all-targets -- -D warnings` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1（2026-11-27，Windows 11，10.0.26300，x86-64）

- **做法**：`crates/sunaba/Cargo.toml` 寫明 `version = "0.1.0"`、`edition`、`rust-version`、`publish = false`、`license`（lint 照舊是 workspace 的：machopo 的 `sanity.py unsafe` 要每個 crate 有 `[lints] workspace = true`，第 3 段換成自己的）；加 `[features]`，`default = ["delay-load-hook"]`。`windows.rs` 的 `__pfnDliFailureHook2`、`delay_load_failed`、`DelayLoadInfo` 改成 `cfg(all(target_env = "msvc", feature = "delay-load-hook"))`。提到 machopo 的地方：`build.rs` 的註解改成說連結它、又在 policy 底下啟動的程式都要這樣做；`soundness.md` 的兩處改成用它的程式的測試，machopo 當例子；README 的「Lowering」和 `soundness.md` 寫明 feature。`ipc`、`shell` 照舊 `sunaba.workspace = true`（預設的 feature）。
- **在 machopo 外面單獨編**：把 `crates/sunaba` 的檔案（`git ls-files`）複製到本機的空目錄（不在任何 workspace 裡），`Cargo.toml` 加 `[workspace]`、把 `[lints] workspace = true` 換成照 machopo 抄的 `[lints.rust]`、`[lints.clippy]`（不抄 machopo 的 `unexpected_cfgs` 的 `ipc_loom` 那些），加 `rust-toolchain.toml`（1.99.0，clippy、rustfmt）和 `clippy.toml`（只有 `fetch_add` 那一條，理由改成不提 machopo 的 `base`），不放 `.cargo/config.toml`。依賴只有 `windows-sys 0.61.2`（Windows）。`cargo test`（unit 8 個、整合 29 個通過）、`cargo test --no-default-features`（一樣）、兩種 feature 的 clippy `-D warnings`、`cargo fmt --check` 都通過：MSVC 預設的連結器（不是 `rust-lld`）連 `sunaba_probe` 的延後載入也行，`.cargo/config.toml` 不用帶過去（待決定的那一條解決了）。這份複本是第 2 段的起點的試驗，不 commit。
- **Mutation test**（`mutants-1.toml`，2 個，`--keep-going`）：hook 不是預設的（`default = []`）、hook 不跟著 feature（條件反過來）：都由 `shell` 的 `renderer_sandbox_windows::` 抓到（看不到延後載入失敗的那一行）。跑完確認檔案都還原了。
- **測不到的**：關掉 feature 時沒有定義那個變數，只看得到編得起來（記在 `crates/sunaba/soundness.md` 的 Windows 沒有測試、靠審查的那一串）；macOS 和 Linux 上這一段改到的只有 `Cargo.toml`，沒有在那裡編（第 2 段的 CI 會）。
- **跑過的檢查**：`cargo test -p sunaba`（unit 8 個、整合 29 個通過）；`cargo clippy -p sunaba --all-targets -- -D warnings`，加 `--no-default-features` 再一次（都沒有警告）；`cargo test -p shell --test it renderer_sandbox_windows`（13 個通過）；`cargo test -p ipc`（都通過）；`sanity.py` 的七項（都通過）。`Cargo.lock` 跟著改了 `sunaba` 的版本。
