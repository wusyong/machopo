# RFC 0117 的進度：renderer process 關進作業系統的 sandbox

開始日期：2026-11-26

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、Linux 的第一次探測（`details.md` 的「Linux 的探測」）、第 1 段之前的九條決定 | 做完 | 2026-11-26 |
| 1a. 補探測：wasm 的文件（wasmtime 的 JIT）、`MACHOPO_TRACE`、`about:crash` 和 panic 的路；`strace -k` 找出零星的讀取是誰讀的；fontconfig 的 cache 過期、寫不了時怎麼辦；發行版和 CI 的 kernel 有沒有 Landlock。之後定第 1 段的細節（README 的「待決定」） | 探測做完；第 1 段的細節待定 | |
| 1b. 新的 crate `sunaba`：Linux 的 Landlock 規則和 seccomp 的清單、`SIGSYS` 的 handler、最低要求的檢查；輔助的執行檔和「做不到」的測試；`soundness.md`；`unsafe` 的規則和 `sanity.py unsafe` 加上它 | 做完（2026-11-26；policy 的清單是 machopo 的，在 1c） | |
| 1c. 接上 machopo：child 的 `main` 一開始進 sandbox；Browser 問 fontconfig 的路徑、啟動時交給 renderer；`--no-sandbox` 和警告；套不上時分頁說明原因；`shell` 的測試從 `/proc` 看真的 renderer | 做完（2026-11-26） | |
| 1d. trace 和 census 的輸出檔由 Browser 開好、啟動時交給 renderer；工具拿掉暫時的 `--no-sandbox` | 做完（2026-11-26；做法照已決定的事項第 16 條：renderer 自己在鎖之前開，fork 的 trace 多一個入口；**fork 的 commit、新的 release tag 和 machopo 的 pin 還沒做，等使用者**） | |
| 1e. 第 1 段的檢查和文件：整個 workspace 的測試、WPT、情境、Speedometer（使用者要求時）、`docs/limitations.md`、`docs/architecture.md`、CI | 做完（2026-11-26；Speedometer 沒量：等使用者要求；CI 等 fork 的新 release） | |
| 2a. macOS 的探測（`details.md` 的「macOS 的探測」）、對 Chromium 的 macOS sandbox 的原始碼、定這一段的事（已決定的事項第 17 到 20 條） | 做完 | 2026-11-26 |
| 2b. `sunaba::macos`：`Profile`、`enter`、`entered`、`sandboxed`、`probe`；`sunaba_probe` 的 macOS 版和「做不到」的測試；README 和 `soundness.md` | 做完 | 2026-11-26 |
| 2c. 接上 machopo：`shell::renderer_sandbox::macos` 的 profile、renderer 的 `main` 第一件事進 sandbox、`--type=renderer-sandbox-probe`；`shell` 的測試從外面看真的 renderer、從裡面試 profile | 做完 | 2026-11-26 |
| 2d. 第 2 段的檢查和文件 | 做完（Speedometer 沒量：等使用者要求） | 2026-11-26 |
| 3a. Windows 的探測（`details.md` 的「Windows 的探測」）、對 Chromium 的 Windows sandbox 的原始碼、定這一段的事（已決定的事項第 21 到 27 條） | 做完 | 2026-11-26 |
| 3b. `sunaba::windows`：啟動 process 的函式、policy（AppContainer 的 token、job、mitigation）、降 integrity；`sunaba_probe` 的 Windows 版和「做不到」的測試；README 和 `soundness.md` | 做完 | 2026-11-26 |
| 3c. `ipc` 的 `spawn` 改用它：自己的 `Child`、`spawn_in`、`spawn_reading_errors`（`spawn_with_pipe` 沒有改，鎖還在：已決定的事項第 23 條） | 做完 | 2026-11-26 |
| 3d. 接上 machopo：`shell::renderer_sandbox` 的 Windows policy、延後載入、renderer 的 `main` 降 integrity、`--allow-third-party-modules`、從外面看真的 renderer 的測試 | 做完 | 2026-11-26 |
| 3e. trace 和 census 的檔案由 Browser 開好交給 renderer（fork 的 Rust 加 Windows 的入口） | 做完（**fork 的 commit、push 和 machopo 的 `third_party.toml` 的 `rev` 還沒做，等使用者**） | 2026-11-26 |
| 3f. 第 3 段的檢查和文件 | 做完（只在 Windows 上；Linux 和 macOS 上還沒跑過這一段的改動） | 2026-11-26 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

### 0：Linux 的第一次探測（2026-11-26，Linux：Fedora 45，kernel 7.3.3，debug build，`52dc5adc`）

`probe.py`（`strace -f` 底下的 `machopo --automation-pipe` 載入一頁），系統字型和 `--test-fonts` 各一次；結果在 `details.md` 的「Linux 的探測」。第一版的統計只算到 renderer 的 1 個 thread：`strace -f` 把被打斷的呼叫拆成 `<unfinished ...>` 和 `<... resumed>` 兩行，沒接起來就認不出 `clone3` 建的 thread；接起來之後是 22 個。

### 1a：補探測（2026-11-26，Linux：Fedora 45，kernel 7.3.3，debug build，`428b2f38`）

- `strace -f -k` 看零星的讀取的堆疊（`details.md` 的「零星的讀取是誰讀的」）：mimalloc 和 Rust runtime 的三個在 `main` 之前，不用放行；鎖之後還會讀的是 `/sys/devices/system/cpu/online`（glibc 的 `get_nprocs`）、`/dev/urandom`（V8 的亂數）、時區，和 glibc 的 malloc 再讀一次 `overcommit_memory`。
- fontconfig 的 cache 不在時（`XDG_CACHE_HOME` 指到空的目錄）：renderer 裡的 `libfontconfig` 會建 cache 檔（`details.md` 的「fontconfig 的 cache 不在時」）。第一次探測說「renderer 沒有寫過任何檔案」只在 cache 是新的時成立。對策：Browser 啟動 renderer 之前先初始化 fontconfig（它問路徑時本來就會），seccomp 讓檔案的寫入類呼叫回錯誤。
- wasm 的文件、`MACHOPO_TRACE`、`about:crash` 各一次（`details.md` 的「wasm 的文件、trace、crash」）：wasmtime 要 `memfd_create` 和可執行的 `mprotect`；trace 時 renderer 自己建檔案（第 6 條要改的）；crash 是對自己 `tgkill`。
- Landlock 的 ABI 和 kernel 的對照、CI 的 kernel（7.0，ABI 8；有沒有開著要在 CI 上問一次）：`details.md` 的「kernel 和 CI」。各發行版預設開不開沒查到。
- 還沒探測的：很多 worker、記憶體的壓力、`census` 的 build、列印和下載（沒有）。

### 1b：`sunaba`（2026-11-26，Linux：Fedora 45，kernel 7.3.3，Landlock ABI 11）

- `cargo test -p sunaba`：單元測試 4 個（handler 的訊息、system call 的名字），整合測試 16 個（`tests/it/linux.rs`：每個測試跑 `sunaba_probe`，在真的 sandbox 裡試一件事）。連跑 30 次都過。
- `cargo clippy -p sunaba --all-targets -- -D warnings`、`cargo fmt`、`sanity.py` 的 `unsafe`、`tests`、`numerics`、`deps`、`boundary`、`spans`、`docs`：過。
- mutation test（`mutants-1b.toml`）：18 個都抓到。第一次跑時「handler 說完就返回」那個 mutant 寫壞了（換成的 `getpid` 自己就是被擋的呼叫，是別的測試碰巧抓到）：改成什麼都不做，重跑那一個，抓到。跑完檔案都還原了。
- 等價、沒有列進去的：handler 最後不叫那個沒有的 system call、只叫 `abort`：`abort` 要的 `tgkill` 在測試的 policy 裡也是被擋的，結果一樣是 `SIGSYS`（policy 許 `tgkill` 對自己時會變成 `SIGABRT`；1c 用 renderer 的 policy 測）。
- 測試守不住、靠審查的（寫在 `crates/sunaba/soundness.md` 的「What the tests check, and don't」）：handler 的 async-signal-safety、沒有 Landlock 的 kernel 走的路、盡力而為的那幾層 Landlock（TCP、abstract socket、signal）、x86-64 以外的架構。
- 做的時候發現的：
  - seccomp 的一個 filter 只有一種「符合時」的結果，所以「回 errno」的每一種 errno 是一個自己的 filter、疊在擋下其餘的那一個之前（kernel 取最嚴的結果）。
  - glibc 建 thread 時先擋住所有 signal 再叫 `clone3`、`clone`：這時被擋的 system call 送不出 `SIGSYS`，kernel 直接結束 process，沒有那一行訊息。訊息是輔助，不是保證（寫在 README 和 soundness 文件）。
  - handler 結束 process 的方式：在 handler 裡（`SIGSYS` 被擋著）叫一個沒有的 system call 號碼，seccomp 照樣擋它、送不出 signal，kernel 以 `SIGSYS` 結束 process。不需要 policy 許任何呼叫，父 process 看到的是「被 `SIGSYS` 結束」。
  - 測試的 process 被擋下時會留 core dump（開發機是 systemd-coredump）：`probe::run` 先 `prctl(PR_SET_DUMPABLE, 0)`。renderer 自己要不要留，1c 再看。

### 1c：接上 machopo（2026-11-26，同一台機器；還沒做完）

- 做了什麼：`shell::renderer_sandbox`（renderer 的 policy、child 的命令列開關、Browser 列字型目錄、`enter`）；renderer process 的 `main` 先 `start_renderer`（找字型、進 sandbox）才建 log、trace 和通道；`layout::SystemFonts` 從 child 的開頭一路交到 `RenderThread` 的 `TextContext`；`--no-sandbox` 和兩種警告；sad tab 依 renderer 的結束碼（5：進不了 sandbox）換說明；`machopo --type=renderer-sandbox-probe`（同一個執行檔照 renderer 的方式進 sandbox、試一件事：測 policy 用，不另外做一個要 link V8 的執行檔）。
- `sunaba` 多的：`Policy::fail_when`（某些參數時回 errno）、探測的 `chmod`、`truncate`、`abort`，和 `attempt_and_exit`（呼叫的一方自己進 sandbox）。
- 做的時候發現的（都改了 policy，第 13 條的原則 (3) 跟著修正）：
  - Landlock 不管 `chmod`、`chown`、`utime`、xattr：seccomp 放行、等 Landlock 擋是擋不到的（`sunaba` 的測試實際改到了 policy 以外的檔案的 mode）。renderer 的 policy 改成這些呼叫一律回 `EACCES`。
  - Landlock 的第 3 版（kernel 6.2）之前不管 truncate：讀得到的檔案用 `O_TRUNC` 開就清空了（使用者自己的字型目錄）。policy 讓 `open`、`openat` 帶寫入、`O_CREAT`、`O_TRUNC` 的回 `EACCES`，`openat2` 回 `ENOSYS`，`truncate` 回 `EACCES`：不靠 Landlock 的版本。
  - glibc 的 `isatty` 現在用 `TCGETS2`；`prctl(PR_GET_AUXV)` 和 `MADV_GUARD_INSTALL` 比 `libc` crate 的常數新，自己定義。
  - V8 開 `/dev/urandom` 之後對它 `ioctl(TCGETS2)`（`fopen` 看是不是 terminal）：Landlock 的第 5 版擋 device 的 `ioctl`，回 `EACCES`，`fopen` 照樣用。
  - trace 一開始就建一個 thread（Perfetto 的 `TracingMuxer`），檔案是它的 C++ 自己照路徑開的：不能在鎖之前開始（`sunaba` 要 process 只有一個 thread），鎖之後又開不了檔案。1d 要定怎麼辦。
- `cargo test -p shell --test it`：757 個裡 3 個沒過，都是 trace 的（`trace::a_js_documents_cppgc_phases_are_traced`、`a_renderer_process_writes_a_trace_of_its_own`、`marks_and_measures_are_chromes_user_timing_events`：renderer 在 sandbox 裡建不了自己的 trace 檔），等 1d。整套跑的時候沒有任何 renderer 被 `SIGSYS` 結束。
- 新的測試：`shell` 的 `tests/it/renderer_sandbox.rs` 11 個（真的 renderer 的每個 thread 的 `Seccomp`、`NoNewPrivs`；`--no-sandbox` 的沒有；sandbox 裡用系統字型排的寬度和外面一樣；policy 從裡面試：讀不到使用者的檔案、改不了檔案系統、出 process 的呼叫被結束、只能建 thread、只能 signal 自己、限制參數的幾個呼叫），單元測試（開關的來回、字型目錄、sad tab 的原因、警告）；`sunaba` 的整合測試 19 個。
- mutation test（`mutants-1c.toml`）：18 個都抓到，跑完檔案都還原了。
- 等價、沒有列進去的：`open` 的旗標少擋 `O_TRUNC`、`O_WRONLY` 等：這台的 Landlock（ABI 11）本來就擋，結果一樣；守的是 6.2 之前的 kernel，要那樣的 kernel 才測得到（記進 `docs/test-gaps.md`，1e）。
- 測試守不住的（1e 記進 `docs/test-gaps.md`）：`start_renderer` 進不了 sandbox 的那條路（要沒有 Landlock 的 kernel；sad tab 的說明只有單元測試）。

### 1d：trace 和 census（2026-11-26）

- rusty_v8 的 fork：`v8::machopo_trace::start_into`（`machopo__Trace__StartInto`：接受已經開好的檔案；Unix），`README.machopo.md` 加一條。**還沒 commit**；改了 C++，prebuilt 的 V8（`machopo-v152.2.0-29`）沒有這個函式：本機用 `python3 scripts/ci.py v8 --profile release` 編的（要先在 fork 裡 `git submodule update --init --recursive` 和 `build/linux/sysroot_scripts/install-sysroot.py --arch=amd64`，`LIBCLANG_PATH=/usr/lib64`；release 13 分半），測試都帶 `RUSTY_V8_ARCHIVE`。
- machopo：renderer process 先開 census 檔（`base::census::own_file`：之後在同一個檔案上覆寫，不再寫旁邊再改名）和 trace 檔（`<檔名>.<pid>`），再進 sandbox，再開始 trace（`bindings::trace::start_into`、`shell::trace::layer_into`）。
- policy 多許 `prctl(PR_GET_NAME)`：Perfetto 讀 thread 的名字，沒有就被結束。
- Perfetto 讀 `/proc/self/stat`、`/proc/self/cmdline`、`/proc/stat`、`/sys/devices/system/cpu/possible` 都被擋（`EACCES`）：trace 照樣寫出來，`shell` 的 trace 測試和量測工具的測試都過，所以照已決定的事項第 10 條不放行。
- `bench.py speedometer --sample`：V8 的 `--perf-basic-prof` 讓每個 renderer 寫 `/tmp/perf-<pid>.map`，sandbox 裡寫不出來；取樣時工具自己加 `--no-sandbox`（`scripts/bench/common.py` 的 `sampled_switches`；照記憶 Chrome 的 `--perf-prof` 也要 `--no-sandbox`）。其他量測照樣在 sandbox 裡。
- mutation test（`mutants-1d.toml`）：3 個都抓到，檔案都還原了。`cargo test -p shell --features census --test it census`：過。

### 1e：第 1 段的檢查（2026-11-26，都帶 `RUSTY_V8_ARCHIVE`）

- `cargo test --workspace`：4941 個通過，0 失敗（1 分 56 秒）；整個過程沒有 renderer 被 `SIGSYS` 結束。`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 七項：過。
- `test.py scenarios`：98 個都過。`test.py reftest`：過。
- `test.py wpt dom/nodes html/canvas/element/text workers wasm/jsapi/memory css/css-fonts/parsing`：18460 個檢查，和預期不同的 4 個都是「預期失敗、結果通過」，在 `workers` 的同一份 data URL 的測試（和 sandbox 無關的預期結果過期，沒有動）。
- 量測工具的測試：`speedometer_test.py`（123 個）、`blink_perf_test.py`、`raster_test.py`、`census_test.py`：過。
- 文件：`docs/architecture.md`（「Process 的邊界」第 5 條的命令列、crate 表的 `sunaba`、`unsafe` 的位置）、`docs/limitations.md`（「隔離」改成現況）、`docs/test-gaps.md`（三條只靠審查的）、`docs/README.md`、`CLAUDE.md`。
- 還沒做、等使用者：fork 的 commit 和 push、新的 release tag（`machopo-v152.2.0-30`）、machopo 的 `RUSTY_V8_MIRROR_TAG` 和 `third_party.toml`；之後 CI 才編得過。CI 的 runner 有沒有開 Landlock 到時候看（已決定的事項第 12 條）。
- 編 V8 時 fork 的 `gen/src_binding_ptrcomp_release_x86_64-unknown-linux-gnu.sum` 變成刪除的狀態（開始前它和 debug 的那一份就有沒 commit 的修改，不是這份改的）。
- Speedometer（2026-11-26，使用者要求；`bench.py speedometer --against e27b751d`，各 3 次，20 個 suite）：sandbox 裡的 renderer（`4817809f`）對接上 sandbox 之前的程式，時間的幾何平均 0.997（-0.3%，在雜訊裡），各 suite 從 -3.3% 到 +1.7%；記憶體的峰值各 suite 從 -3.5% 到 +3.8%。seccomp 的成本量不出來。基準 `e27b751d` 是只為量測做的 commit（`e6b65653` 加上現在的 `third_party.toml`：工具要兩邊的 pin 一樣），不在任何 branch 上。
- fork 的 release（2026-11-26）：`cb5186f` 推到 `machopo`、tag `machopo-v152.2.0-30`。Linux x86-64 的 release 版先編好，machopo 的 `RUSTY_V8_MIRROR_TAG` 換到它、`third_party.toml` 的 `rev` 拿掉；不帶 `RUSTY_V8_ARCHIVE`（連結下載的 prebuilt）`cargo test --workspace`：4941 個通過，0 失敗。**換的時候 Linux 的 debug 版、macOS 和 Windows 的還在編**（使用者要求先當成功繼續）：它們編完之前，那幾個平台和 `V8_FORCE_DEBUG=1` 的 build 下載不到檔案。
- rebase 到 `origin/main`（2026-11-26，`98509a4f`：automation pipe 改成 Chrome 的 `--remote-debugging-pipe`、fd 3 和 4 之後）：`shell` 的 `lib.rs`、`main.rs`、兩份測試和 `scripts/bench/common.py` 有衝突，都是開關改名和啟動 machopo 的方式；`probe.py` 跟著改成新的 pipe。量測工具認不認得 `--no-sandbox` 改成像其他版本差異一樣記在 `--against` 的執行檔上（`WITHOUT_NO_SANDBOX`）。rebase 之後：`cargo test --workspace` 4954 個通過，0 失敗；fmt、clippy、`sanity.py` 七項、情境 98 個、量測工具的四份測試、`test.py wpt` 的小目錄：過。

### 2a：macOS 的探測（2026-11-26，macOS 26.6.2，Apple M4，debug build，`311f74f7`）

- `probe_macos.py`：兩種跑法（全部放行並回報；renderer 自己的 profile 被擋了什麼），從系統的 log 讀 kernel 的回報。結果在 `details.md` 的「macOS 的探測」。
- 從只放行 rendezvous 的 profile 一樣一樣加，每次拿截圖和沒有 sandbox 的比：缺 `com.apple.fonts` 時 CJK 的 fallback 字型不同，是唯一一個「畫得出來但不一樣」的；時區要 `/etc`（symlink）的 metadata。放行 `cfprefsd`、偏好設定檔、`/etc/passwd`、`opendirectoryd`、整個檔案系統的 metadata 各試過一次，都不能取代它。
- 純 `(deny default)` 時 renderer 查不到 rendezvous、結束；探測的腳本原本會一直等 Browser 回答，加了 20 秒的期限。
- 這台機器上有 2026-11-20 留下的一批 `machopodriver`（parent 是 launchd），和這份無關，沒有動。
- 開始前 `../third_party/rusty_v8` 有沒 commit 的修改（`src/machopo.rs`、`src/scope.rs`，只有排版），`ci.py third-party` 因此不動它：手動換到釘的 commit（`cb5186f`），那兩個檔案的修改留著。

### 2b、2c：`sunaba::macos` 和接上 machopo（2026-11-26，同一台）

- `cargo test -p sunaba`：整合測試 18 個（`tests/it/macos.rs`：每個測試跑 `sunaba_probe`，在真的 sandbox 裡試一件事），連跑 10 次都過。`cargo test -p shell --lib renderer_sandbox`：4 個。`cargo test -p shell --test it renderer_sandbox`：11 個（`tests/it/renderer_sandbox_macos.rs`）。
- 做的時候發現的：
  - **被擋的路徑回 `EPERM`、不存在的回 `ENOENT`**：只放行用到的 metadata（已決定的事項第 19 條）擋不住「路徑在不在」。第 19 條和 `docs/limitations.md` 照實改了，兩邊各有一個寫明這個限制的測試。
  - **`(deny default)` 之後還問得到同一個使用者的其他 process 的資料**（`proc_pidinfo`）：profile 明寫 `(deny process-info*)`，有測試。
  - **`sandbox_check` 對不存在的 pid 也回「在 sandbox 裡」**：`sunaba::macos::sandboxed` 的說明寫明只能問在跑的 process；測試問的是還活著的 renderer。
  - 進去之後再套一份：一模一樣的成功（沒有效果），不同的被拒絕。
  - 測試的 process 自己有 rendezvous server（同一個 binary 的別的測試啟動過 renderer）：查自己 parent 的 rendezvous 的測試兩種答案都收（查到、或 launchd 說沒有），不收「沒有權限」。
  - `kill(-1, 0)` 在 sandbox 裡成功（對象包含自己）：不拿它當「送不出去」的測試。
- mutation test（`mutants-2.toml`）：第一次 34 個裡 4 個沒抓到，都是 profile 裡的放行：
  - 對自己送訊號（`(allow signal (target self))`）：拿掉之後 `abort` 照樣以 `SIGABRT` 結束（它不經過 sandbox 管的那一種），沒有別的東西要它：**從 profile 拿掉**。
  - 讀 `/usr/share/icu`、sysctl `kern.osproductversion`：拿掉之後頁面、wasm 的文件的截圖和沒有 sandbox 時一樣：**從 profile 拿掉**（探測時是因為「看到 renderer 讀」才放的，不是因為少了會壞）。
  - sysctl `hw.optional.`（CPU 的功能）：要留（JIT 和標準庫照它選指令，結果看不出來、效能看得出來）：補了測試（讀得到）。
  - 改完重跑：31 個都抓到，檔案都還原了。
- 手動確認（`docs/test-gaps.md`）：`sample <renderer 的 pid> 1` 列得出 `CrRendererMain`、`Compositor`、`Chrome_ChildIOThread`、V8 的 worker；拿掉 `process-info-setcontrol` 那一條時名字都是空的（確認完還原）。
- 測試守不住、靠審查的（`docs/test-gaps.md`）：profile 明寫擋掉的 IOKit 屬性、NVRAM、分散式通知（沒有嘗試它們的輔助程式）；進不了 sandbox 的那條路。

### 2d：第 2 段的檢查（2026-11-26，macOS 26.6.2，Apple M4；renderer process 都在 sandbox 裡）

- `cargo test --workspace`：4969 個通過，0 失敗。`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 七項：過。
- `test.py scenarios`：98 個都過。`test.py reftest`：152 個，都和預期一樣。`test.py wpt dom/nodes css/css-fonts/parsing`：16873 個檢查，沒有和預期不同的。
- 量測工具的測試：`speedometer_test.py`（126 個，跳過 4 個）、`blink_perf_test.py`（37 個，跳過 3 個）、`raster_test.py`（13 個）、`census_test.py`（5 個）：過。
- 探測的頁面、wasm 的文件在最後的 profile 底下的截圖和沒有 sandbox 時逐 byte 相同；`about:crash` 照 renderer 當掉處理；`MACHOPO_TRACE` 寫得出 renderer 的 trace。
- 沒有做的：Speedometer（等使用者要求）；`scenarios --window`（沒有動到視窗那一層）；`census` feature 的 build 在 sandbox 裡的測試（`cargo test -p shell --features census` 沒有在 macOS 上跑）；Linux 上重跑（這一段沒有改 Linux 的 policy，改到的共用程式是 `renderer_sandbox.rs` 的 `enter`、`probe`、`ChildSwitches::for_renderer` 和 `lib.rs` 的 `renderer_launcher`：Linux 那一側由 CI 和使用者在 Linux 的機器上確認）。

### 3a：Windows 的探測（2026-11-26，Windows 11 10.0.26300，x86-64，debug build，`3ce52b1c` 加沒有 commit 的原型）

- `probe_windows.py`：每一種限制（job、mitigation、三級 restricted token、integrity、AppContainer、LPAC）各跑一次，拿截圖和沒有 sandbox 的比；另外用一個小的執行檔在每一種裡面各試 17 件事。結果在 `details.md` 的「Windows 的探測」。
- 這台不是管理員、沒有 debugger：renderer 被結束時用 `crashwatch_windows.py` 接上去看堆疊。lockdown 的 token 底下 renderer 死在 V8 第一次要亂數（`rand_s` 要載入系統的 DLL）：先在第一個 thread 上要一次亂數就好了。
- 探測用的原型在工作目錄裡、沒有 commit：`sunaba` 的 `windows.rs`（啟動函式）和 `sunaba_probe` 的 Windows 版、`ipc` 的 `windows/child.rs` 和 `spawn` 裡的分支、`browser` 的 `render_process_host.rs`（`Child` 的型別）、`shell` 的 `renderer_sandbox.rs`（降權的步驟）。都標著 `PROBE`；定了機制之後改寫成正式的，或丟掉。
- 探測時登記過一個 AppContainer（`machopo.probe.renderer`），做完用 `DeleteAppContainerProfile` 刪掉了；在家目錄和 Temp 建過的測試檔也刪了。
- 沒有做的：Process Monitor 那種「開了哪些檔案和登錄機碼」的清單（要管理員）；另一個 desktop；使用者自己裝的字型；Windows 10。
- 補的探測（同一天）：AppContainer 的 token 照 Chromium 自己建（`CreateAppContainerToken`，不登記 profile），和 renderer 自己再降到 Untrusted：都成功，截圖相同（`details.md` 的「AppContainer 的 token 自己建（Chromium 的做法）」）。原本照記憶說 Chromium 用的是 `NtCreateLowBoxToken`、要自己建物件的目錄：讀了原始碼，不是。
- 補的探測（同一天）：Chromium 的清單裡第一輪沒試的七個 mitigation，各自和全部一起都沒有改變截圖（`details.md` 的「其餘的 mitigation」）。
- 補的探測（同一天）：把視窗和繪圖的 DLL 改成延後載入之後，建立時關掉 win32k 可以了（`details.md` 的「win32k：把視窗和繪圖的 DLL 改成延後載入」）。**`target/debug/machopo.exe` 現在是那一次試驗連結出來的**，要用一般的之前先重新 build。
- 補的探測（同一天）：延後載入之後有視窗的 Browser 照常；另一個 window station 和 desktop 加上去沒有壞（`details.md` 的「另一個 window station 和 desktop」）。

### 3b、3c、3d：`sunaba::windows`、`ipc`、接上 machopo（2026-11-26，同一台，debug build）

- 探測的原型都換掉了：`sunaba` 的 `windows.rs` 重寫成 `Command`、`Policy`、`Child`、`lower` 和從外面問一個 process 的幾個函式；`ipc`、`browser`、`shell` 裡標著 `PROBE` 的分支和環境變數拿掉了。
- `cargo test -p sunaba`：單元測試 5 個，整合測試 21 個（`tests/it/windows.rs`：每個測試跑 `sunaba_probe`，它在整個 policy 底下啟動自己、試一件事），整合測試連跑 3 次都過。`cargo test -p ipc`：都過。`cargo test -p browser -p driver`：都過。`cargo test -p shell`：3 個沒過，都是 trace 的（`trace::a_renderer_process_writes_a_trace_of_its_own`、`marks_and_measures_are_chromes_user_timing_events`、`a_js_documents_cppgc_phases_are_traced`：renderer 在 sandbox 裡開不了自己的 trace 檔），等 3e；其餘都過，其中 `renderer_sandbox_windows` 9 個。
- 真的 machopo（一般的 build）：renderer 在 AppContainer 裡、Untrusted、19 個 DLL，探測的頁面的截圖和 `--no-sandbox` 時、和 3a 的基準都相同（系統字型、測試字型）。
- `sanity.py` 七項：過。
- 做的時候發現的：
  - **`spawn_with_pipe` 換不過去**：它的呼叫端用標準庫的 `Command` 設定 stdout、stderr 再從標準庫的 `Child` 讀，那些設定讀不出來。第 23 條照實改了。同一個原因，`shell` 的測試讀 renderer 的 stderr 的地方改用新的 `ipc::platform::spawn_reading_errors`（三個平台都有）。
  - **AppContainer 裡的 process 本來就有嚴格的 handle 檢查**（還沒 `lower` 時讀 mitigation 就是開的）：`lower` 裡開它的那一行拿掉也看不出差別（`docs/test-gaps.md`）。
  - **在 sandbox 裡啟動程式是被 process 自己的屬性擋的**（`ERROR_CHILD_PROCESS_BLOCKED`），不是 job（探測時沒有那個屬性，是 job 擋的）：job 的「一個 process」這個上限因此沒有測試單獨守著。
  - 測試裡「一個號碼在 child 裡是不是 handle」一開始只看 `GetHandleInformation`：child 自己剛好有一個同號碼的別的 handle 時會誤判（跑第一次就遇到）。改成還要是同一種物件（開的是 `NUL`）。
  - 從真的 console 啟動（stdout、stderr 都是 console）時，sandbox 裡的 process 起得來（手動確認，`docs/test-gaps.md`）。
- mutation test（`mutants-3.toml`）：33 個都抓到，跑完檔案都還原了。第一次跑時測試的篩選字把還沒修的 trace 測試也選進去，基準沒過、一個都沒跑；改了篩選重跑。兩個 mutant 寫壞了重寫：一個編不過；一個把「降 integrity 失敗時回錯誤」換成不看結果，呼叫還是做了，測不出來（要一個會失敗的 `SetTokenInformation`，沒有），換成「成功當成失敗」。
- 等價、沒有列進去的：`lower` 裡不開嚴格的 handle 檢查（上面）；不設 job 的「一個 process」上限（上面）。
- 測試守不住的（`docs/test-gaps.md`）：關掉 KTM、job 的 UI 限制、DEP 那幾個（沒有讀得回來的狀態）；DLL 的搜尋順序；建不出 sandbox 時不啟動；`--allow-third-party-modules` 在有注入 DLL 的軟體的機器上；console。
- 沒有做的：`cargo test --workspace`、fmt、clippy（這一段全部做完時跑）；`test.py scenarios --window`（延後載入動到整個執行檔，3f 跑）；release build；Linux 和 macOS 上重跑（改到的共用程式：`ipc::platform` 的 `Child`、`Sandbox`、`spawn_in`、`spawn_reading_errors`，`browser` 的 `RendererLauncher::Process` 的回傳值，`shell` 的 `renderer_launcher`、`renderer_sandbox::sandbox`、`sad_tab` 的新原因，和 `shell`、`ipc` 的測試讀 stderr 的方式：那兩個平台由 CI 和使用者在那裡的機器上確認）。

### 3e：trace 和 census（2026-11-26，同一台）

- rusty_v8 的 fork：只改了 Rust。`machopo__Trace__StartInto` 收的是 C runtime 的 file descriptor，C++ 在每個平台都編進去了，prebuilt 裡本來就有；只有 Rust 的 `start_into` 標了只限 Unix。加了 Windows 版（`_open_osfhandle` 把 handle 變成 descriptor，帶 `_O_BINARY`），`README.machopo.md` 跟著改。**沒有重編 V8**；原本以為要改 C++（第 24 條照實改了）。**fork 的兩個檔案改了、沒有 commit**。
- machopo：`browser::RendererProcess`（命令、sandbox、交出去的檔案、process 建好之後叫的函式）；`ipc::platform` 的 `spawn_handing`、`inherited_file`（Windows）；`shell` 的 `renderer_sandbox::windows`（`Handed`：先用別的名字建檔、建好再改名；`HandedFiles`：renderer 讀命令列的 `--trace-file`、`--census-file`）；renderer 的 `main` 接手檔案、降權、才開始 trace；`base::census::own_file_from`；`bindings::trace::start_into` 和 `shell::trace::layer_into` 不再只限 Unix。
- 原本沒過的 3 個 trace 測試過了。`cargo test -p shell --features census --test it census`：過。新的單元測試 3 個（命令列的值、開著時改名、每個 renderer 自己的 AppContainer），`ipc` 1 個（`a_handed_file_is_taken_by_its_value_and_nothing_else_is`）。
- 量測工具的測試：`speedometer_test.py`（142 個，跳過 4 個）、`blink_perf_test.py`（46 個）、`raster_test.py`（13 個，跳過 1 個）、`census_test.py`（5 個）：過。
- mutation test（`mutants-3.toml` 裡 3e 的 9 個，和因為程式改了而重寫的 3 個）：都抓到，檔案都還原了。第一次有一個沒抓到：`inherited_file` 把不是檔案的 handle 當成檔案；補了 `ipc` 的那個測試之後抓到。整份 `mutants-3.toml` 是 42 個。
- 沒有列進去、靠審查的：fork 的 `_open_osfhandle` 的 `_O_BINARY`（少了它，寫出去的 trace 裡的換行會被改掉；trace 的測試讀得回事件，但沒有專門守這一點）。

### 3f：第 3 段的檢查（2026-11-26，Windows 11 10.0.26300，x86-64，debug build；renderer process 都在 sandbox 裡）

- `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 七項：過。clippy 抓到的兩處改了（計數器換成 `base::AtomicSequenceNumber`、一個多餘的 `.ok()`）。**Windows 上的 clippy 和編譯看不到 Linux、macOS 專屬的程式**：這一段改到共用的型別和函式（3b 到 3d 那一節列的，加上 3e 的 `RendererProcess` 和 renderer 的 `main` 開檔的那一段），那兩個平台連編不編得過都沒有確認（這台沒有那兩個 target）。
- `cargo test --workspace --no-fail-fast`：跑了 4 次，每次 4969 個通過、0 失敗、2 個略過。
- `test.py scenarios`：98 個都過。`test.py scenarios --window`（延後載入動到整個執行檔）：74 個都過；更早在背景和其他檢查排在一起跑的那一次以失敗結束，當時只留了輸出的最後幾行、不知道是哪一個，單獨重跑沒有重現。
- `test.py reftest`：152 個，都和預期一樣。`test.py wpt dom/nodes`：15617 個檢查，沒有和預期不同的。`test.py wpt css/css-fonts/parsing`：1256 個檢查，沒有和預期不同的。
- **不穩定的測試**：第一次跑整個 workspace 時 `shell` 的整合測試有 1 個失敗（當時沒留下名字）。之後 `shell` 的整合測試連跑 20 次，2 次各有 1 個失敗：`event_loop::an_idle_callbacks_timeout_comes_while_the_page_is_busy`（`didTimeout` 是 false）和 `automation_pipe::the_window_has_bounds_to_set`（等 machopo 回答逾時）。兩個都不經過這一段改的程式：
  - 前一個的 renderer 是測試的 process 裡的 thread。頁面用一個接一個的 `fetch` 讓 thread 忙著，兩個回應之間 thread 閒下來時 idle callback 就照常跑了。
  - 後一個截一張 8192×8192 的圖。8 個同時、各跑 40 次（每次連同兩個啟動 sandbox 裡的 renderer 的測試）：它失敗 1 次，machopo 寫那張圖的訊息寫到一半就結束了；另外兩個測試 640 次啟動都沒有失敗。**Browser 為什麼寫到一半結束沒有查**（猜是記憶體或 GPU 撐不住）。
  - 沒有拿這一段之前的程式重跑來對照：「和 sandbox 無關」靠的是上面兩點，不是前後的比較。
  - 這兩個測試，和 CI 最近 40 次裡失敗過的 `navigation::links::location_navigates_to_other_documents_through_the_browser`（同一個 task 裡被取代的那次導覽，請求有時來得及送出去），另外改了（`6523c436`，和這份無關的一個 commit）：改完各跑 320 次、8 個同時，都沒有失敗。
- 文件：`docs/limitations.md`（「隔離」改成三個平台、Windows 擋不住的、環境變數、`--allow-third-party-modules`、console；`ipc` 在 Windows 上啟動 child 的那一條改成只剩 `spawn_with_pipe`）、`docs/architecture.md`（「Process 的邊界」第 5 條、crate 表的 `sunaba`、`unsafe` 的位置）、`docs/test-gaps.md`（五條）、`crates/sunaba` 的 README 和 `soundness.md`、`crates/ipc` 的 README 和 `soundness.md`、`machopo --help`。
- 沒有做的：Speedometer（等使用者要求）；release build（延後載入和 sandbox 只在 debug build 上跑過）；Windows 10；Linux 和 macOS 上重跑。
- 還沒做、等使用者：fork 的 commit 和 push、`third_party.toml` 的 `rev`（之後別的機器和 CI 才編得過）；這一段的 commit。
