# RFC 0117 的進度：renderer process 關進作業系統的 sandbox

開始日期：2026-11-26

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、Linux 的第一次探測（`details.md` 的「Linux 的探測」）、第 1 段之前的九條決定 | 做完 | 2026-11-26 |
| 1a. 補探測：wasm 的文件（wasmtime 的 JIT）、`MACHOPO_TRACE`、`about:crash` 和 panic 的路；`strace -k` 找出零星的讀取是誰讀的；fontconfig 的 cache 過期、寫不了時怎麼辦；發行版和 CI 的 kernel 有沒有 Landlock。之後定第 1 段的細節（README 的「待決定」） | 探測做完；第 1 段的細節待定 | |
| 1b. 新的 crate `sunaba`：Linux 的 Landlock 規則和 seccomp 的清單、`SIGSYS` 的 handler、最低要求的檢查；輔助的執行檔和「做不到」的測試；`soundness.md`；`unsafe` 的規則和 `sanity.py unsafe` 加上它 | 做完（2026-11-26；policy 的清單是 machopo 的，在 1c） | |
| 1c. 接上 machopo：child 的 `main` 一開始進 sandbox；Browser 問 fontconfig 的路徑、啟動時交給 renderer；`--no-sandbox` 和警告；套不上時分頁說明原因；`shell` 的測試從 `/proc` 看真的 renderer | 做到一半（2026-11-26）：進 sandbox、字型、`--no-sandbox`、sad tab 的原因、測試和 mutation test 做完；trace 的 3 個測試要等 1d；文件和 WPT、情境還沒 | |
| 1d. trace 和 census 的輸出檔由 Browser 開好、啟動時交給 renderer；工具拿掉暫時的 `--no-sandbox` | 沒開始 | |
| 1e. 第 1 段的檢查和文件：整個 workspace 的測試、WPT、情境、Speedometer（使用者要求時）、`docs/limitations.md`、`docs/architecture.md`、CI | 沒開始 | |
| 2. macOS | 沒開始 | |
| 3. Windows | 沒開始 | |

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
