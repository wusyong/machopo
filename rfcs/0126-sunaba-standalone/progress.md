# RFC 0126 的進度：`sunaba` 成為獨立的專案

開始日期：2026-11-27。完成日期：2026-11-28（第 1 段在 Windows 11，10.0.26300，x86-64；第 2、3 段在 macOS 26，Apple Silicon。sunaba 在 Linux、macOS 和 Windows 上由它自己的 CI 驗；改接之後的 machopo 由使用者在 Windows 上驗過，2026-11-28）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 在 machopo 裡先切乾淨：`Cargo.toml` 不再從 workspace 繼承（lint 除外）、改掉提到 machopo 的地方、延後載入失敗的 hook；複製到外面單獨編和測；已決定的事項第 5、6 條 | 做完 | 2026-11-27 |
| 2. 建 repo、搬出去：從一個新的 commit 開始，設定、文件、`sanity.py`、三個平台的 CI；第一次 CI 找到的兩件事；已決定的事項第 7–11 條 | 做完 | 2026-11-28 |
| 3. machopo 改接：`Cargo.toml`、`third_party.toml`、`sanity.py`、`test.py mutate`、文件，拿掉 `crates/sunaba`；已決定的事項第 12 條 | 做完 | 2026-11-28 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| sunaba 的 repo | `github.com/wusyong/sunaba`：版本（0.1.0）、edition、`rust-version`、授權、`publish = false`、`repository` 寫在自己的 `Cargo.toml`，自己的 `[workspace]`、lint、`rust-toolchain.toml`（1.99.0）、`clippy.toml`，沒有 `.cargo/config.toml` | 在 machopo 外面單獨 `cargo test`、clippy、fmt（第 1 段的複本，和 sunaba 的 CI） |
| sunaba 的 repo | 延後載入失敗的 hook 只在 `delay-load-hook` 開著時定義，預設開著；關掉照樣編得起來、沒有警告 | `shell` 的 `renderer_sandbox_windows::` 看得到 hook 的那一行（`mutants-1.toml`）；sunaba 的 CI 的 Windows 不帶預設的 feature 跑 test 和 clippy |
| sunaba 的 repo | 英文的 README（Building and testing、Contributing、License）、`docs/soundness.md`、`docs/contributing.md`、`docs/testing.md`（測試守不住的地方，從 machopo 的 `docs/test-gaps.md` 搬過去的那幾條） | 審查 |
| sunaba 的檢查 | `scripts/sanity.py`：`unsafe`（只有 `src/lib.rs` 打開；有 `unsafe` 的檔案都在 soundness 的表裡）、`tests`、`numerics` | 在一份複本裡弄壞七種，每一種都報了 |
| sunaba 的 CI | `ubuntu-latest`、`macos-latest`、`windows-latest` 上 `cargo test`、clippy（`-D warnings`），Windows 上不帶預設的 feature 再一次；Linux 打開沒有特權的 user namespace，chroot 的測試真的跑；fmt 和 `sanity.py` | run 46093751158（7973001）：都通過 |
| CI 找到的 | Linux：chroot 之後查路徑的 errno 是 `ESRCH` 或 `ENOENT`（看 kernel 的版本），測試兩種都接受 | sunaba 的 `a_chroot_leaves_no_path_not_even_to_say_what_is_there`，在 runner 上 |
| CI 找到的 | Windows：sandbox 裡的 process 建的東西不給 logon session（lockdown 的 default DACL，sunaba 照搬 machopo 的 fc17f43d） | sunaba 的 `the_process_has_a_lockdown_app_containers_token`、`what_its_threads_make_isnt_for_the_logon_session`，在 runner（logon session 在預設的 DACL 上）和使用者的 Windows 機器上 |
| machopo 改接 | workspace 用 path 依賴 ../third_party/sunaba，`third_party.toml` 釘 7973001；`crates/sunaba` 拿掉；`ipc`、`shell` 的程式不改 | `cargo test --workspace`、fmt、clippy；`shell` 的 `renderer_sandbox_macos::` |
| machopo 改接 | `ci.py third-party` 從空的 ../third_party clone 得出 sunaba，之後認得本機的 checkout | 把 ../third_party/sunaba 移開再跑 |
| machopo 的檢查 | `sanity.py unsafe` 只允許 `bindings` 和 `ipc`；`docs` 把 sunaba 當成搬走的 crate，RFC 裡當時的路徑不檢查 | `sanity.py` 的七項 |
| `test.py mutate` | machopo 以外的檔案的原檔放在 `target/mutate/originals/_outside/`，`--restore` 放得回去；更外面的檔案在動手之前拒絕 | `scripts/mutate_test.py`（CI 的 lint job）；`mutants-3.toml` |

## 做過的檢查

### 1（2026-11-27，Windows 11，10.0.26300，x86-64）

- **在 machopo 外面單獨編**：`crates/sunaba` 的檔案複製到不在任何 workspace 裡的空目錄，加上 `[workspace]`、照 machopo 抄的 lint（不抄 `unexpected_cfgs` 的 `ipc_loom` 那些）、`rust-toolchain.toml` 和只有 `fetch_add` 的 `clippy.toml`，不放 `.cargo/config.toml`：`cargo test`（unit 8 個、整合 29 個通過）、`--no-default-features`、兩種 feature 的 clippy `-D warnings`、fmt 都通過。MSVC 預設的連結器（不是 `rust-lld`）連 `sunaba_probe` 的延後載入也行，所以 `.cargo/config.toml` 不用帶過去。
- **Mutation test**（`mutants-1.toml`，2 個）：hook 不是預設的、hook 不跟著 feature：都由 `shell` 的 `renderer_sandbox_windows::` 抓到。關掉 feature 時沒有定義那個變數，只看得到編得起來（記在 sunaba 的 `docs/testing.md`）。
- **跑過的檢查**：`cargo test -p sunaba`、兩種 feature 的 clippy、`cargo test -p shell --test it renderer_sandbox_windows`（13 個通過）、`cargo test -p ipc`、`sanity.py` 的七項。

### 2（2026-11-28，macOS 26，Apple Silicon；Linux 和 Windows 由 sunaba 的 CI）

- **第一個 commit**（sunaba 的 0b77de23）：f0d1bf86 的 `crates/sunaba`，`soundness.md` 移到 `docs/`，加上第 1 段試過的設定、`.gitattributes`、`.gitignore`、授權、文件、`sanity.py` 和 CI。`sanity.py` 在一份複本裡弄壞過七種（`unsafe_code` 改成 warn、沒有列在表裡的 `unsafe`、別的檔案打開 `unsafe_code`、表裡不存在的檔案、`#[allow]` 關掉轉型的 lint、`tests/` 底下多一個 binary、沒宣告的模組），每一種都報了。本機的 `cargo test`（unit 2 個、整合 19 個）、兩種 feature 的 clippy、fmt、`sanity.py` 都通過。
- **第一次 CI**（run 46091850428）：lint 和 macOS 通過，Linux 和 Windows 各一個測試失敗，都是第一次在那種機器上跑才看得到的：
  - Linux：打開 user namespace 之後 chroot 的測試第一次在 CI 上跑，`stat /etc` 是 `ENOENT`，測試要的是 `ESRCH`（開發機的）。chroot 到已經結束的 thread 的 `/proc/thread-self/fdinfo` 之後查路徑失敗的 errno 看 kernel 的版本；改成兩種都接受（已決定的事項第 11 條）。
  - Windows：runner 上 sandbox 裡的 process 的 default DACL 是 Administrators、SYSTEM、logon session（`S-1-5-5-0-…`）和它的 AppContainer，沒有使用者。RFC 0121 的已決定的事項第 3 條（K1）靠系統的預設沒有 logon session。同一天 machopo 的 `main` 已經在 `crates/sunaba` 修好（fc17f43d，RFC 0121 的第 12 條）；sunaba 先自己寫了一版（56f8237：建好 AppContainer 的 token 之後才改，第一個 thread 的 token 沒有測試），核對之後換成 fc17f43d 的（7973001，已決定的事項第 10 條）。`src/windows.rs`、`windows/probe.rs`、`windows/handles.rs`、`tests/it/windows.rs` 和 0126 rebase 到 `origin/main` 之後的 `crates/sunaba` 的完全一樣；default DACL 的 mutation test 是 fc17f43d 在 Windows 上跑過的那一份。
- **和 CI 對不上的事實**：fc17f43d 和它的 `docs/test-gaps.md` 寫「CI 的 Windows runner 是 UAC 關掉的管理員，和 elevated 一樣，看不出來」和「不給使用者 `GENERIC_ALL` 是等價的 mutation」。runner 上預設的 DACL 有 logon session（那個測試就是因此失敗的）、沒有使用者（`GENERIC_ALL` 那個 mutation 在那裡抓得到）。sunaba 的 `docs/testing.md` 照這個寫，machopo 的那兩條在第 3 段拿掉。
- **之後的 CI**：run 46092690361（56f8237）、run 46093751158（7973001）都通過；Linux 的整合測試 22 個，Windows 的 30 個（兩種 feature 都是）。Windows 編 `sunaba_probe` 的測試版本時連結器警告 `LNK4199`（測試的 harness 版本不呼叫 `user32`，`/DELAYLOAD:user32.dll` 沒有用到），只是警告。
- **要注意的**：使用者的全域 gitignore 排除 `.gitattributes`，sunaba 的是 `git add -f` 進去的（machopo 和 elise 的也是）；HTTPS 沒有認證，../third_party/sunaba 的 `origin` 的 push URL 設成 SSH，fetch 照舊是 HTTPS。

### 3（2026-11-28，macOS 26，Apple Silicon）

- **`test.py mutate`**：新的測試 `scripts/mutate_test.py`（5 個）。**Mutation test**（`mutants-3.toml`，6 個）：原檔照舊放在 `originals/` 外面、不放在 `_outside/`、spec 的路徑不拿掉 `..`、`_outside/` 的放回 machopo 裡、`--restore` 照存的路徑放回、更外面的不拒絕：都由 `mutate_test.py` 抓到；跑完確認 `scripts/test.py` 還原了。
- **改接**：`Cargo.lock` 不變（path 依賴不記來源）。`ci.py third-party` 不用改：把 ../third_party/sunaba 移開再跑，它 shallow clone 到 7973001；放回去之後照樣認得本機的 checkout。
- **跑過的檢查**：`cargo test --workspace`（5071 個通過、1 個 ignored，沒有失敗）、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`；`sanity.py` 的七項；`scripts/sanity_numerics_test.py`、`ci_patches_test.py`、`mutate_test.py`。Linux 的 `renderer_sandbox_*` 由 machopo 的 CI 跑；Windows 的由使用者在改接之後的 machopo 上驗過（2026-11-28）。
