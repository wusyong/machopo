# RFC 0120 的進度：Linux 的 renderer sandbox 的下一級：namespace 和 chroot

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、探測（`details.md`）、第 1 段的四條決定 | 做完 | 2026-11-27 |
| 1a. V8 的 entropy source（`bindings`）；policy 不放行 CPU 的數量和 `/dev/urandom`，時區的兩個路徑只在 renderer 的 `TZ` 不是 ICU 收的名字、或是三四個字的名字時放行；Browser 照 `/etc/localtime` 的連結給 renderer `TZ` | 做完 | 2026-11-27 |
| 1b. 測試；CI 的 glibc 的退路 | 做完 | 2026-11-27 |
| 1c. 第 1 段的檢查和文件 | 做完 | 2026-11-27 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 時區的名字照 `/etc/localtime` 的連結解（`posix/`、`right/` 拿掉；不是名字的、ICU 不收的不給）；`TZ` 照 ICU 的 `isValidOlsonID` 判斷收不收；policy 只在要讀時放行時區的兩個路徑，其他一個都不放 | `cargo test -p shell --lib renderer_sandbox::`（`a_zone_is_named_by_where_the_systems_time_zone_links`、`icu_takes_a_tz_as_its_zone_as_it_decides`、`a_renderer_reads_no_file_but_the_time_zones_it_has_to_find`） |
| sandbox 裡的 renderer 開不了 `/sys/devices/system/cpu/online`、`/dev/urandom`；`TZ` 是 ICU 收的名字時也開不了時區的檔案，沒有 `TZ` 或 ICU 不收時開得了 | `cargo test -p shell --test it renderer_sandbox::a_renderer_reads_no_file_but_the_time_zones_it_has_to_find` |
| Browser 沒有 `TZ` 時，renderer process（sandbox 裡外）的 `TZ` 是這台的時區的名字，看到的時區和 thread 上的 renderer 相同；使用者的 `TZ` 不被蓋掉 | `renderer_sandbox::a_renderer_process_is_told_this_machines_time_zone_by_name`、`renderer_sandbox::a_renderer_process_has_only_the_listed_part_of_its_browsers_environment` |
| sandbox 裡、外和 thread 上的 renderer 看到的時區、`Intl`、`navigator.hardwareConcurrency`、寬度相同 | `renderer_sandbox::a_sandboxed_renderer_shows_a_page_as_one_outside_does` |
| 短的名字（`TZ=CET`）在 renderer process 裡也是那個時區：冬天 -60、夏天 -120，和 thread 上的相同 | `renderer_sandbox::a_short_zone_name_is_the_zone_in_a_renderer_process_too` |
| V8 不自己開 `/dev/urandom` | 手動：`docs/test-gaps.md` 的「只做過手動確認」 |

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
