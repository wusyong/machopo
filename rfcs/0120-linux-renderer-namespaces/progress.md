# RFC 0120 的進度：Linux 的 renderer sandbox 的下一級：namespace 和 chroot

開始日期：2026-11-27。完成日期：2026-11-27（Linux：Fedora 45，kernel 7.3.3，glibc 2.44，x86-64；三段都在這台上做和驗。CI 的 `ubuntu-24.04` runner 多半不允許沒有特權的 user namespace，那裡的測試走的是退路：已決定的事項第 7 條、`docs/test-gaps.md`）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、探測（`details.md`）、第 1 段的四條決定 | 做完 | 2026-11-27 |
| 1a. V8 的 entropy source（`bindings`）；policy 不放行 CPU 的數量和 `/dev/urandom`，時區的兩個路徑只在 renderer 的 `TZ` 不是 ICU 收的名字、或是三四個字的名字時放行；Browser 照 `/etc/localtime` 的連結給 renderer `TZ` | 做完 | 2026-11-27 |
| 1b. 測試；CI 的 glibc 的退路 | 做完 | 2026-11-27 |
| 1c. 第 1 段的檢查和文件 | 做完 | 2026-11-27 |
| 2a. 第 2 段的探測（`nsinit.c`、Chromium 的原始碼）和三條決定（已決定的事項第 5 到 7 條） | 做完 | 2026-11-27 |
| 2b. `sunaba::linux::Namespaces`；`ipc` 的 Linux 上 `Sandbox` 是它、`spawn_in` 在 `pre_exec` 裡進去；Browser 試一次、給 renderer process，不行的話說一次 | 做完 | 2026-11-27 |
| 2c. 測試、第 2 段的檢查和文件 | 做完 | 2026-11-27 |
| 3a. 第 3 段的探測（進了 sandbox 之後還碰的路徑、`chroot.c`）和三條決定（已決定的事項第 8 到 10 條） | 做完 | 2026-11-27 |
| 3b. sunaba：`Namespaces::keeping_chroot`；`Policy::chroot`；`enter` 先 `tzset`、chroot、丟掉全部的權限 | 做完 | 2026-11-27 |
| 3c. shell：時區不用讀檔的 renderer chroot；短的名字不再放行時區的檔案；`/etc/localtime` 是複本時 Browser 找出名字 | 做完 | 2026-11-27 |
| 3d. 測試、第 3 段的檢查和文件 | 做完 | 2026-11-27 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| Landlock 的路徑 | policy 不放行任何路徑，只有時區要讀檔時放行時區的兩個；sandbox 裡的 renderer 開不了 CPU 數的檔案、`/dev/urandom`，`TZ` 是 ICU 收的名字時也開不了時區的檔案 | `cargo test -p shell --lib renderer_sandbox::`（`a_renderer_reads_no_file_but_the_time_zones_it_has_to_find`）；`cargo test -p shell --test it renderer_sandbox::a_renderer_reads_no_file_but_the_time_zones_it_has_to_find` |
| Landlock 的路徑 | V8 不自己開 `/dev/urandom` | 手動：`docs/test-gaps.md` 的「只做過手動確認」 |
| 時區 | 名字照 `/etc/localtime` 的連結解；複本照 zoneinfo 比對（area 的優先、不看 `posix/`、ICU 不收的不算）；`TZ` 照 ICU 的 `isValidOlsonID` 判斷收不收 | `cargo test -p shell --lib renderer_sandbox::`（`a_zone_is_named_by_where_the_systems_time_zone_links`、`a_copied_zone_is_named_by_the_file_it_is_a_copy_of`、`icu_takes_a_tz_as_its_zone_as_it_decides`） |
| 時區 | Browser 沒有 `TZ` 時 renderer process 拿到這台的時區的名字，使用者的 `TZ` 不被蓋掉；`/etc/localtime` 是複本時也拿到；短的名字（`CET`）照樣對；都和 thread 上的 renderer 相同 | `renderer_sandbox::a_renderer_process_is_told_this_machines_time_zone_by_name`、`a_renderer_process_has_only_the_listed_part_of_its_browsers_environment`、`a_renderer_process_is_told_the_zone_a_copied_localtime_is`、`a_short_zone_name_is_the_zone_in_a_renderer_process_too` |
| namespace | 機器允許時 renderer process 在自己的 user 和 network namespace 裡（PID、mount 的是 Browser 的）：只有 `lo`、同一個使用者、沒有任何權限；`--no-sandbox` 的不在；預期值由 `unshare --user --net true` 獨立判斷 | `renderer_sandbox::a_renderer_process_has_namespaces_of_its_own_where_the_machine_allows`；`cargo test -p sunaba --lib namespaces` |
| namespace | 機器不允許時照舊啟動、畫得出頁面、和 Browser 在同一個 namespace、Browser 說一次 | `renderer_sandbox::a_renderer_process_goes_without_namespaces_where_the_machine_forbids_them` |
| chroot | renderer process 的根目錄是一條已經結束的 thread 的 `fdinfo`（不允許的、`--no-sandbox` 的是 `/`）；從裡面看 `/` 以外的路徑都是 `ESRCH`；沒有權限時不 chroot；chroot 的 policy 讀路徑被拒絕；`enter` 之後沒有任何權限 | `renderer_sandbox::a_renderer_process_has_namespaces_of_its_own_where_the_machine_allows`；`cargo test -p sunaba --test it linux::` |
| 網頁看到的 | sandbox 裡、外和 thread 上的 renderer 看到的時區、`Intl`、`navigator.hardwareConcurrency`、寬度相同；chroot 的 renderer 上整套 `shell` 的測試通過 | `renderer_sandbox::a_sandboxed_renderer_shows_a_page_as_one_outside_does`；`cargo test -p shell` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 0（2026-11-27，Fedora 45，Linux 7.3.3，glibc 2.44，x86-64）

- 探測和 Chromium 的做法：`details.md`。

### 1a 到 1c（2026-11-27，同一台）

- **CI 的 glibc**：讀了 glibc 2.39（Ubuntu 24.04）的 `sysdeps/unix/sysv/linux/getsysstats.c`：`get_nprocs` 先讀 `/sys/devices/system/cpu/online`，讀不到讀 `/proc/stat`，再讀不到用 `sched_getaffinity`，連它也失敗才回 2；`get_nprocs_conf` 一樣，只是第一個讀 `possible`。和這台的 2.44 量到的一樣，`sched_getaffinity` 在 seccomp 的清單上，所以照第 2 條，不加 P2。
- **ICU 收不收 `TZ`**：照 rusty_v8 帶的 ICU 的 `putil.cpp` 的 `isValidOlsonID`：不含數字和逗號，或最後最多兩個數字（`Etc/GMT+11`、`JST-9`），或 `PST8PDT`、`MST7MDT`、`CST6CDT`、`EST5EDT`；開頭的 `:` 拿掉，`posix/`、`right/` 拿掉。收的話 ICU 只用 `TZ`，不收的話讀 `/etc/localtime` 的連結或內容。原本以為 `JST-9` 不收（單元測試這樣寫、失敗了），照 ICU 改正。
- **找到的 bug**（實作時，在 commit 之前）：`strace -f -e trace=openat` 看到帶 `TZ=America/New_York` 的 sandbox 裡的 renderer 開 `/usr/share/zoneinfo/America/New_York` 被拒絕；`strace -k` 的堆疊是 ICU 的 `detectHostTimeZone` 呼叫 glibc 的 `tzset`。ICU 對三四個字的名字拿 glibc 的時差比對（`details.md` 的「時區不讀檔」），所以 `TZ=CET` 會安靜地變成 UTC+0。改成短的名字照舊放行時區的檔案，加了 `a_short_zone_name_is_the_zone_in_a_renderer_process_too` 和 probe 的 `CET` 一列。
- **V8 的 entropy source**：`strace -f -e trace=openat` 跟著一個帶 Browser 和兩個 renderer process 的測試：裝了之後沒有 `O_RDONLY` 的 `/dev/urandom`（只剩 Landlock 建規則時的 `O_PATH`，policy 拿掉它之後也沒了）；暫時拿掉 entropy source 之後開了好幾次。測試看不出來（V8 開不了時安靜地退回用時間），記在 `docs/test-gaps.md`。
- **新的測試**：`shell` 的單元測試 3 個（時區的名字、ICU 收不收 `TZ` 和要不要讀時區的檔案、policy 放行的路徑）；`renderer_sandbox.rs` 的 `a_renderer_reads_no_file_but_the_time_zones_it_has_to_find`（四種 `TZ`：沒有、`Asia/Tokyo`、`AST4ADT`、`CET`；`open` CPU 的檔案、`/dev/urandom`、時區的檔案）、`a_renderer_process_is_told_this_machines_time_zone_by_name`、`a_short_zone_name_is_the_zone_in_a_renderer_process_too`。`a_renderer_process_has_only_the_listed_part_of_its_browsers_environment` 的時區改成和這台不同的（這台是 `Asia/Tokyo` 時用 `America/New_York`）：原本用的剛好是這台的，蓋掉使用者的 `TZ` 也看不出來（下面的 mutation test 抓到的）。`a_sandboxed_renderer_shows_a_page_as_one_outside_does` 多比 `navigator.hardwareConcurrency`。原本的 `a_renderer_reads_no_font_of_the_systems_but_the_time_zone` 改名 `a_renderer_reads_no_font_of_the_systems`，時區的部分搬到新的那個。`sunaba` 的 Linux probe 多一種 `open PATH`（開了就關，不讀：`/dev/urandom` 這種讀不完的也能試）。
- **Mutation test**（`mutants-1.toml`，12 個，`--keep-going`）：第一輪（11 個）抓到 9 個。沒抓到的兩個：「Browser 的時區蓋掉使用者的 `TZ`」（測試的 `TZ` 剛好是這台的，改了之後重跑抓到）；「V8 沒有 entropy source」（測不到，手動確認，上面）。改了短的名字之後整份重跑（多一個「短的名字不放行時區的檔案」，一個 mutant 跟著改寫過的函式改）：12 個抓到 11 個，只剩 entropy source。抓到的：時區的檔案不管 `TZ` 都放行、都不放行、沒有 `TZ` 時不放行、ICU 的規則收任意多個數字、少一個 POSIX 的例外、Browser 不給 `TZ`、Browser 的蓋掉使用者的、名字取整個連結、留著 `posix/`、名字的字元不檢查、短的名字不放行。檔案都還原了（`git status`，`target/mutate/originals/` 不在）。
- **跑過的檢查**：`cargo test -p shell -p bindings -p sunaba`（都通過）；量測工具的測試（`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py`、`census_test.py`，都通過）；`strace` 再看一次最後的 build：renderer 沒有開 `/dev/urandom`；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；第 1 段做完的 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（抓到 ICU 的規則裡一個沒寫明溢位的加法，改寫成不用加法，它的兩個 mutant 重跑都抓到）、`cargo test --workspace`（63 個 binary，4985 個通過，1 個 ignored，沒有失敗）。

### 2a 到 2c（2026-11-27，同一台）

- **探測**（`details.md` 的「第 2 段的探測」）：`nsinit.c` 量到沒有特權也能在只有一條 thread 時 `unshare` user、network、PID namespace、fork 出 init 和 renderer；`SCM_CREDENTIALS` 的 pid 被 kernel 換成收的一方的 namespace 的；init 和外面的 process 把 renderer 的 `SIGABRT` 原樣交回。讀了 Chromium 的 `namespace_sandbox.cc`、`credentials.cc`、`zygote_linux.cc`、`sandbox_linux.cc`。之後定了不要 PID namespace（第 6 條），這些留著給之後。
- **新的測試**：`sunaba` 的單元測試 2 個（對應、多執行緒被拒絕）；`shell` 的 `renderer_sandbox.rs` 的 `a_renderer_process_has_namespaces_of_its_own_where_the_machine_allows`（從外面看 `/proc/<pid>/ns`、`/proc/<pid>/status`、`/proc/<pid>/net/dev`；sandbox 裡外），`a_renderer_process_goes_without_namespaces_where_the_machine_forbids_them`（`unshare -Ur sh -c 'echo 0 > /proc/sys/user/max_user_namespaces && exec …'` 包著啟動 Browser：畫得出、和 Browser 在同一個 namespace、stderr 說了一次）。`automation_pipe.rs` 的 `Browser` 多一種包著啟動的（`start_wrapped`）。
- **找到的測試的洞**：第一版「這台允許嗎」是測試自己問 `namespaces()`，和 Browser 用的是同一段程式：`enter` 壞了的話兩邊都說不允許、renderer 安靜地走退路，測試照樣過。改成用 `unshare --user --net true` 獨立判斷，再確認兩者一致（下面的「no user namespace」「setgroups isn't given up」「the machine never allows them」就是靠它抓到的）。
- **手動確認**：在一個不准再開 user namespace 的 namespace 裡跑整組 `renderer_sandbox::`（16 個，那時還沒有退路的測試），都通過。
- **Mutation test**（`mutants-2.toml`，10 個，`--keep-going`）：全部抓到：沒有 network namespace、沒有 user namespace、不放棄 `setgroups`、在 namespace 裡是 root（exec 之後權限還在）、機器一律允許、一律不允許、child 沒有進去、Linux 的 renderer 不給、`--no-sandbox` 的也給、不說少了。檔案都還原了（`git status`，`target/mutate/originals/` 不在）。
- **量的**：開 user 和 network namespace、寫對應，每次啟動多 0.48 ms（中位數，`details.md`）。
- **跑過的檢查**：`cargo test -p shell -p ipc -p sunaba`（都通過）；量測工具的測試（`speedometer_test.py`、`blink_perf_test.py`、`raster_test.py`、`census_test.py`，都通過）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；第 2 段做完的 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個 binary，4989 個通過，1 個 ignored，沒有失敗）。

### 3a 到 3d（2026-11-27，同一台）

- **探測**（`details.md` 的「第 3 段的探測」）：`strace` 列出進了 sandbox 之後還碰的路徑，會因為 chroot 壞掉的只有短名字的時區（glibc 的 `tzset`）；`chroot.c` 量到只把 `CAP_SYS_CHROOT` 留成 ambient capability 行得通、chroot 到一條 thread 的 `fdinfo` 之後 `/` 以外都是 `ESRCH`；sunaba 的 `enter` 讀 `/proc` 確認只有一條 thread，所以 chroot 在那之後。
- **新的測試**：sunaba 的 `tests/it/linux.rs` 3 個（chroot 之後什麼路徑都碰不到、沒有權限時不 chroot 和 policy 的檢查、權限全部丟掉），用 `unshare --keep-caps` 加 `setpriv` 照 `keeping_chroot` 的樣子啟動 probe（第一版只用 `setpriv` 拿不到權限，`unshare` exec 它時權限已經清掉了）；sunaba 的 probe 多一個 `--chroot`、`Policy` 多一個 `chroots()`。shell 的單元測試：複本的名字（area 的優先，`Eire` 和 `Europe/Dublin` 這種舊名字排在前面的也是；只有 `posix/` 有的不算）、policy 要不要 chroot、短的名字不再讀檔；整合測試：`a_renderer_process_has_namespaces_of_its_own_where_the_machine_allows` 多看根目錄，`a_renderer_process_is_told_the_zone_a_copied_localtime_is`（測試自己的 mount namespace 裡，把 `/etc` 換成連到原本的每個項目、只有 `localtime` 是 `Europe/Paris` 的複本的目錄：`/etc/localtime` 是 symlink，bind mount 會跟著連結走，第一版蓋到的是 `Asia/Tokyo` 那個檔案）。`stat("/")` 在 chroot 之後是成功的（根目錄本身），測試原本寫錯了。
- **chroot 的 renderer 上的整套測試**：`cargo test -p shell` 全部通過（767 個整合測試），沒有程式依賴 `ENOENT`。
- **Mutation test**（`mutants-3.toml`，14 個，`--keep-going`）：13 個抓到，1 個寫法編不過（「複本不比對」），改寫之後單獨重跑抓到。抓到的：child 不留 chroot 的權限、權限沒留過 exec、Browser 的 namespace 不留、不 chroot、沒有權限也 chroot、chroot 到原本的根目錄、權限不丟、不先 `tzset`、chroot 的 policy 可以讀路徑、renderer 的 policy 不 chroot、複本不比對、複本的名字不先挑 area 的、看 `posix/`、名字不檢查 ICU 收不收。檔案都還原了。
- **跑過的檢查**：`cargo test -p shell -p sunaba -p ipc`（都通過）；量測工具的測試（四個，都通過）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；第 3 段做完的 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個 binary，4994 個通過，1 個 ignored，沒有失敗）。

### Speedometer（2026-11-27，同一台：AMD Ryzen AI 9 HX 370，接電源，瀏覽器綁在 CPU 0-3、12-15）

使用者要求時量的（`python3 scripts/bench.py speedometer --against 5ba58434 --runs 3`）：`against` 是 `main` 的 `5ba58434`（這份 RFC 的草稿：字型服務和環境變數的過濾都在），`current` 是 `261ec4be`（三段都做完）；兩邊的 renderer 都是自己的 process，輪流各跑 3 次，每次 10 個 iteration。

- **時間**：20 個 suite 的 current / against 的幾何平均是 0.999（-0.1%）；每個 suite 的中位數的差在 -4.7%（Charts-chartjs）到 +2.7%（TodoMVC-React-Complex-DOM）之間，都在兩邊的 95% 區間裡重疊：量不出差別。
- **記憶體的高峰**（Browser 加上 renderer 的 VmHWM）：-10.7% 到 +1.0%；Browser 的幾乎不變（138 到 169 MB），renderer 的大多少了一點（NewsSite-Next 362 → 307 MB，其餘多半是 0 到 6%）。推測（沒有驗證）是 CPU 的數量：工具用 `taskset` 把瀏覽器綁在 8 個 CPU 上，第 1 段之後 renderer 讀不到 `/sys/devices/system/cpu/online`，glibc 照 affinity 數成 8 個（之前是整台的 24 個），V8 的 worker 和 glibc 的 malloc 的 arena 跟著少。

