# RFC 0126 的進度：`sunaba` 成為獨立的專案

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 在 machopo 裡先切乾淨：`Cargo.toml` 不再從 workspace 繼承（lint 除外）、改掉提到 machopo 的地方、延後載入失敗的 hook；複製到外面單獨編和測；已決定的事項第 5、6 條 | 做完 | 2026-11-27 |
| 2. 建 repo、搬出去：從一個新的 commit 開始，設定、文件、`sanity.py`、三個平台的 CI；已決定的事項第 7–9 條 | 做完 | 2026-11-28 |
| 3. machopo 改接：`Cargo.toml`、`third_party.toml`、`sanity.py`、文件，拿掉 `crates/sunaba` | 做完 | 2026-11-28 |

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

### 2（2026-11-28，macOS 26，Apple Silicon；Linux 和 Windows 由 sunaba 的 CI）

- **做法**：在 ../third_party/sunaba 開一個新的 repo（`main`，`origin` 是 `https://github.com/wusyong/sunaba`），檔案是 f0d1bf86 的 `crates/sunaba`（`git ls-files`），`soundness.md` 移到 `docs/soundness.md`（`lib.rs` 和 README 指過去）。`Cargo.toml` 加 `[workspace]`（在哪裡 checkout 都單獨編得起來；machopo 用 path 依賴時有自己的 workspace，elise 也是這樣）、`repository`、`readme`，`[lints] workspace = true` 換成自己的 `[lints.rust]`、`[lints.clippy]`（照 machopo 抄，不抄 `unexpected_cfgs`）。第 1 段試過的 `rust-toolchain.toml`（1.99.0）、`clippy.toml`（只有 `fetch_add`），加上 `.gitattributes`、`.gitignore`、machopo 的兩份授權。README 的「It depends on nothing of the workspace it sits in」改成說 policy 由用它的程式寫、machopo 是例子，加 Building and testing、Contributing、License；`cargo test -p sunaba` 改成 `cargo test`。
- **新的文件**（英文，已決定的事項第 3 條）：`docs/contributing.md`（和 machopo 的關係、改一次的三步、規則、整數運算的規則照 elise 的那份翻成英文、做完之前跑什麼）；`docs/testing.md`（指令、CI、測試守不住的地方：從 machopo 的 `docs/test-gaps.md` 搬過去的 `sunaba` 自己的那幾條，加上 `docs/soundness.md` 寫著只靠審查、還沒列在 machopo 的幾條：fork 和 exec 之間不配置、沒有 Landlock 的 kernel、Windows 10、console；由用它的程式的測試守著的 namespace；兩條等價的 mutation）。machopo 那一側的刪除是第 3 段的事。
- **`scripts/sanity.py`**：`unsafe`（`Cargo.toml` deny `unsafe_code` 和 `unsafe_op_in_unsafe_fn`、打開 `undocumented_unsafe_blocks`；只有 `src/lib.rs` allow `unsafe_code`；`src/` 底下 code 裡有 `unsafe` 的檔案都在 `docs/soundness.md` 的表裡有一列，表裡的檔案都在）、`tests`、`numerics`（照 elise 的那份，改成一個 crate 在根目錄）。`// SAFETY:` 本身由 clippy 的 `undocumented_unsafe_blocks` 檢查（block 和 impl），extern block 和 `#[unsafe(no_mangle)]` 的論證在 soundness 的表裡，所以 `unsafe` 檢查的是表。在一份複本裡弄壞過：`unsafe_code` 改成 warn、`names.rs` 加一個 `unsafe` block、`macos.rs` allow `unsafe_code`、表裡寫一個不存在的檔案、`linux.rs` 用 `#[allow]` 關掉轉型的 lint、`tests/` 底下多一個檔案、`tests/it/` 多一個沒宣告的模組：每一項都報了。
- **CI**（`.github/workflows/ci.yml`）：每次 push、PR 和每天一次；`ubuntu-latest`、`macos-latest`、`windows-latest` 上 `cargo test`、clippy（`-D warnings`），Windows 上再不帶預設的 feature 跑一次兩者（feature 只有 Windows 用）；Linux 先 `sysctl` 打開 user namespace（已決定的事項第 8 條）；`ubuntu-latest` 上 fmt 和 `sanity.py` 的三項。
- **本機（macOS）**：`cargo test`（unit 2 個、整合 19 個通過）、`--no-default-features`（一樣）、兩種 feature 的 clippy `-D warnings`、`cargo fmt --check`、`sanity.py` 的三項都通過。
- **要注意的**：使用者的全域 gitignore 排除 `.gitattributes`，commit 時要 `git add -f`（machopo 和 elise 的也是這樣進去的）。
- **第一次 push 和 CI**（sunaba 的 0b77de23，run 46091850428）：`lint` 和 macOS 通過；Linux 和 Windows 各有一個測試失敗，都是第一次在那種機器上跑才看得到的：
  - Linux（打開 user namespace 之後 chroot 的測試第一次在 CI 上跑）：`a_chroot_leaves_no_path_not_even_to_say_what_is_there` 的 `stat /etc` 是 `ENOENT`，不是測試要的 `ESRCH`。chroot 到已經結束的 thread 的 `/proc/thread-self/fdinfo` 之後，查路徑失敗的 errno 看 kernel 的版本（開發機的是 `ESRCH`）；「沒有路徑」照樣成立。
  - Windows（runner 是 elevated 的系統管理員）：`the_process_has_a_lockdown_app_containers_token` 讀到的 default DACL 是 Administrators、SYSTEM、logon session（`S-1-5-5-…`）和它自己的 AppContainer，沒有使用者。RFC 0121 的已決定的事項第 3 條（K1）不另外設 default DACL，靠的是系統給的預設沒有 logon session，探測的是一般使用者的開發機；這個測試就是為了系統的預設不同時看得到。
  - 處理（使用者 2026-11-28 照看法確認，已決定的事項第 10、11 條）：Windows 照 Chromium 自己設 default DACL（`windows.rs` 的 `lockdown_default_dacl`，用在 primary 和第一個 thread 的 token；soundness 的表多一列，README 和承諾第 1 條寫明）；Linux 的測試接受 `ESRCH` 或 `ENOENT`（`is_no_path`），README、`linux.rs` 的說明和 soundness 跟著改。`docs/testing.md` 加上 CI 的 Windows 是 elevated 的系統管理員、第一個 thread 的 token 的 default DACL 只靠審查。
  - 在 macOS 上只編得到 Linux 和 Windows 以外的部分：本機的 `cargo test`、clippy、fmt、`sanity.py unsafe` 通過；Windows 的程式和 Linux 的測試要等 CI。
  - CI（sunaba 的 `fix-ci` branch，56f8237，run 46092690361）：三個平台的 test 和 lint 都通過；Linux 的整合測試 22 個（chroot 的那幾個真的跑了：上一次是在它的斷言失敗的）、Windows 的 29 個（`the_process_has_a_lockdown_app_containers_token` 通過），Windows 不帶預設的 feature 也通過。Windows 編 `sunaba_probe` 的測試版本時連結器警告 `LNK4199`（`/DELAYLOAD:user32.dll` 沒有用到：測試的 harness 版本不呼叫 `user32`），只是警告。
  - sunaba 的 `main` fast-forward 到 56f8237、push（使用者確認）。
  - **改成 machopo 的修正**（使用者確認，已決定的事項第 10 條改寫）：machopo 的 `origin/main` 同時有修同一件事的 fc17f43d（在 Windows 上用一般使用者跑過，mutation 9 個抓到 8 個）。sunaba 的 `dacl-from-machopo` branch（7973001）把 56f8237 的 Windows 那一半換成 fc17f43d 對 `crates/sunaba` 的改動（`git diff --relative=crates/sunaba`，`soundness.md` 的路徑改成 `docs/`，和第 1 段的改動重疊的兩段手動套），Linux 那一半留著。確認過：`src/windows.rs`、`windows/probe.rs`、`windows/handles.rs`、`tests/it/windows.rs` 和 0126 rebase 到 `origin/main` 之後的 `crates/sunaba` 的完全一樣；其他檔案只差 sunaba 的 repo 自己的改動（`Cargo.toml` 的 workspace 和 lint、`lib.rs` 指的 `docs/soundness.md`、Linux 的 errno、README 和 soundness 的那幾句）。default DACL 的 mutation test 不在這裡重跑：程式和 machopo 跑過的那份一樣。
  - CI（`dacl-from-machopo`，run 46093751158）：三個平台和 lint 都通過；Windows 的整合測試 30 個（多了 `what_its_threads_make_isnt_for_the_logon_session`），兩種 feature 都是；Linux 22 個。sunaba 的 `main` fast-forward 到 7973001、push。
  - **和 CI 對不上的事實**：fc17f43d 和它的 `docs/test-gaps.md` 寫「CI 的 Windows runner 是 UAC 關掉的管理員，和 elevated 一樣，看不出來」，以及「不給使用者 `GENERIC_ALL` 是等價的 mutation」。sunaba 的第一次 CI（run 46091850428，`windows-latest`）讀到的 default DACL 是 Administrators、SYSTEM、logon session（`S-1-5-5-0-…`）和 AppContainer、沒有使用者：logon session 在上面（那個測試就是因此失敗的），使用者不在上面（`GENERIC_ALL` 那個 mutation 在 runner 上抓得到）。sunaba 的 `docs/testing.md` 照這個寫；machopo 的 `docs/test-gaps.md` 的那兩條第 3 段搬走時拿掉。
- 推送：HTTPS 沒有認證，`origin` 的 push URL 設成 SSH（`git@github.com:wusyong/sunaba.git`，只在 ../third_party/sunaba 的設定），fetch 照舊是 HTTPS。

### 3（2026-11-28，macOS 26，Apple Silicon）

- **`test.py mutate`（已決定的事項第 12 條）**：`mutant_path`（spec 的 `file` 照文字拿掉 `..`）、`saved_original`（machopo 的檔案照舊在 `originals/<路徑>`，machopo 以外的在 `originals/_outside/<相對於上一層的路徑>`）、`original_home`（反過來，`--restore` 用）；`load_mutants` 在動手之前拒絕 machopo 的上一層以外的檔案。新的測試 `scripts/mutate_test.py`（5 個：兩種檔案放在哪裡、`--restore` 放回被強制結束的 run 留下的兩種、一次 run 的期間 machopo 以外的原檔在 `_outside/`、跑完放回去、更外面的拒絕），加進 CI 的 lint job 和 `docs/testing.md` 的表。
- **Mutation test**（`mutants-3.toml`，6 個，`--keep-going`）：原檔照舊放在 `originals/` 外面、不放在 `_outside/`、spec 的路徑不拿掉 `..`、`_outside/` 的放回 machopo 裡、`--restore` 照舊照存的路徑放回、更外面的不拒絕：都由 `mutate_test.py` 抓到。跑完確認 `scripts/test.py` 還原了（只剩這一段的改動）。
- **改接**：workspace 的 `sunaba` 改成 `path = "../third_party/sunaba"`，放在 elise 旁邊；`third_party.toml` 加 `[sunaba]`（釘 7973001，sunaba 的 CI 在三個平台上通過的 `main`）；拿掉 `crates/sunaba`；`Cargo.lock` 不變（path 依賴不記來源）。`ipc`、`shell` 照舊 `sunaba.workspace = true`，程式不改。`ci.py third-party` 不用改：把 ../third_party/sunaba 移開再跑，它照 `third_party.toml` shallow clone 到 7973001；放回去之後照樣認得本機的 checkout。
- **`sanity.py`**：`unsafe` 的 `UNSAFE_ALLOWED` 和訊息拿掉 `sunaba`；`docs` 的 `GONE_CRATES` 加上 `sunaba`，路徑的比對也認沒有結尾 `/` 的 `crates/sunaba`：已完成的 RFC 照舊不檢查，進行中的 RFC 引用搬走之前的 `crates/sunaba` 底下的檔案和章節時也不檢查。
- **文件**：`CLAUDE.md`（先讀 ../third_party/sunaba 的 soundness 和 contributing；`unsafe` 只在 `bindings` 和 `ipc`，elise 和 sunaba 照它們自己的規則；toolchain 一起調；第三方的那一節加上 sunaba 怎麼改、怎麼釘、mutation test 怎麼寫）、根目錄的 `README.md`（`../third_party/` 的樹）、`docs/README.md`（拿掉 `crates/sunaba/soundness.md` 那一列；調整 Rust 版本時 sunaba 也一起）、`docs/architecture.md`（`sunaba` 從 crate 的表搬到第三方的表，elise 的旁邊；`unsafe` 的位置）、`docs/test-gaps.md`（sunaba 自己的缺口拿掉：它們在 sunaba 的 `docs/testing.md` 裡；fc17f43d 加的「CI 守不住」和「不給 `GENERIC_ALL` 是等價的」也拿掉，見第 2 步；renderer 的 policy 和 machopo 的 CI 的留著）、`docs/testing.md`（mutation test 的原檔）。`docs/limitations.md` 確認過，不用改。
- **跑過的檢查**：`cargo test --workspace`（5071 個通過、1 個 ignored，沒有失敗；`shell` 的 `renderer_sandbox_macos::` 和 `automation_pipe` 經過 ../third_party/sunaba）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（都通過）；`sanity.py` 的七項；`scripts/sanity_numerics_test.py`、`ci_patches_test.py`、`mutate_test.py`。Linux 和 Windows 的 `renderer_sandbox_*` 沒有在這裡跑：程式和 machopo 原本的 `crates/sunaba` 一樣（第 2 步確認過），Linux 的由 machopo 的 CI 跑，Windows 的等使用者的 Windows 機器。

