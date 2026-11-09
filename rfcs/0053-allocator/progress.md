# RFC 0053 的進度：Rust 的 allocator 換成 mimalloc

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-18。完成：2026-10-18，macOS（Apple M4，Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0053` 上做，第 1、2 段一起審核、merge 回 `main`；第 3 段沒做（「已決定的事項」第 10 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 量測工具：`--against` 不帶 `--sample` 時比計時，加上記憶體的峰值；A/A，定門檻 | 完成 | 2026-10-18 |
| 2 換成 mimalloc 和量測（macOS）：採用預設設定（第 9 條） | 完成 | 2026-10-18 |
| 3 其他平台和其他 allocator | 沒做，移到「之後的可能」（第 10 條） | |

## 驗收

### allocator

| 項目 | 怎麼驗證 |
| --- | --- |
| `kun` 的 Rust 那一側的配置經過 mimalloc（配置 64 MB，mimalloc 自己的統計漲了這麼多） | `cargo test -p shell --bin kun -- rusts_allocations_are_mimallocs` |
| `kun --help` 列出 `MIMALLOC_PURGE_DELAY` | `cargo test -p shell --bin kun` |
| 行為不變：經過 `kun` 執行檔的測試 | `cargo test -p shell` |

### 量測工具（`scripts/speedometer.py`）

| 項目 | 怎麼驗證 |
| --- | --- |
| `--against REV` 不帶 `--sample` 時，兩個版本輪流（另一個版本先）各跑 `--runs` 輪，每輪每個 suite 一次；`compare.md`：每個 suite 兩邊所有 iteration 的總時間的中位數和 95% 區間、變化，記憶體峰值（各輪的中位數）和變化，有效的 suite 的變化的幾何平均，每一輪的中位數；有一輪無效的 suite 不比 | `python3 scripts/speedometer_test.py CompareTests` |
| `kun` 的記憶體峰值：macOS 是 `proc_pid_rusage` 的 lifetime max phys footprint（另一個 process 用過、還回去的記憶體也算在峰值裡），Linux 是 `VmHWM`；process 不在時沒有；kun 跑完一個 suite 時讀（被取樣時不讀） | `python3 scripts/speedometer_test.py MemoryTests KunTests.test_kuns_peak_memory_with_its_run` |
| 樣本的比較裡百萬以上的數字（macOS 的 µs）照原樣寫，不用科學記號 | `python3 scripts/speedometer_test.py CompareTests.test_macoss_cpu_time_compared` |

### 量測的結果（2026-10-18，macOS，Apple M4，接電源；20 個 default suite，每個版本 3 輪、每輪 10 個 iteration）

| 比較 | 時間（幾何平均） | 單一 suite 的時間 | 記憶體峰值（幾何平均） | 單一 suite 的記憶體峰值 |
| --- | --- | --- | --- | --- |
| A/A（`--against HEAD`，同一份程式） | +0.3% | −2.9% 到 +2.4% | −1.2% | −17.8% 到 +7.4% |
| mimalloc 的預設設定（`--against main`，第二次） | **−7.9%** | −12.9% 到 −3.2% | **+12.7%** | +0.4% 到 +43.0%（TodoMVC-jQuery） |
| `MIMALLOC_PURGE_DELAY=0` | −5.2% | −9.9% 到 −0.1% | +6.4% | −0.5% 到 +13.8%（NewsSite-Next） |
| `--against main --sample`（計時區間裡的 CPU 時間） | −8.2% | −15.2% 到 −1.6% | | |

## 做過的檢查

2026-10-18，macOS（Apple M4）。

- **量測工具**：`peak_memory`（macOS 用 `ctypes` 呼叫 libproc 的 `proc_pid_rusage`，`RUSAGE_INFO_V4` 的第 29 個 64 位元欄位；先用自己的 process 確認：`mmap` 300 MB 再歸還，第 8 個欄位（phys footprint）掉回 8 MB、第 29 個留在 309 MB）、`time_suites`、`cmd_time_compare`、`write_time_report`；`--against` 不帶 `--sample` 時走這條路（原本會被拒絕）。Windows 的 `PeakPagefileUsage` 這台測不到，記進 `docs/test-gaps.md` 的「測試守不住的地方」。
- **allocator**：`#[global_allocator]` 在 `crates/shell/src/main.rs`（`mimalloc` 0.1.52，mimalloc 3.3.2）；`shell` 的 dev-dependencies 開 `extended`（resolver 3：只有測試的 build 有）。測試用 mimalloc 安全的統計 API，不需要 `unsafe`。跨 allocator 的釋放：`crates/` 裡只有 `dom_js` 的 `ArrayBuffer` 把 Rust 的 `Vec` 交給 V8（`new_backing_store_from_vec`），V8 不用時呼叫 Rust 的 deleter，由 Rust 釋放；和 `details.md` 查的一樣。
- **mutation test**，每條各自弄壞一處，全部抓到，跑完檔案都還原：
  - `mutants-1.toml`（12 條）：不記記憶體、macOS 讀現在的 footprint 而不是峰值、macOS 不讀、Linux 的 kB 當 bytes、無效的一輪也比、只用第一輪的時間、只用第一輪的記憶體、算術平均、工作目錄的先跑、少跑一輪、不帶 `--sample` 時改成一般的量測、大的數字用科學記號。
  - `mutants-2.toml`（1 條）：拿掉 `#[global_allocator]`。
- **補的測試和不穩定的測試**：時間比較的測試起先三輪的數字一樣、只有一個有效的 suite，「只用第一輪」和算術平均都會過，改成三輪不同、兩個 suite。`MemoryTests` 起先讀自己這個 process 的峰值，單獨跑通過、整套跑失敗（前面的測試已經把峰值推得比「現在加 64 MB」還高），改成另起一個 process。
- **量測的過程**：第一次 `--against main` 時機器有別的負載（`main` 那一側比 A/A 時慢一兩成，例如 Editor-TipTap 三輪 128.0、147.8、141.6，A/A 是 115.2），不用來判斷，機器閒著時重跑（第二次，上面的表；`main` 那一側和 A/A 一致，三輪之間很穩）。兩次的記憶體一致（+12.3%、+12.7%），不是雜訊。`MIMALLOC_PURGE_DELAY=0` 的那次用來看記憶體能不能調回來（mimalloc v3 預設 `purge_delay` 10 ms、arena 再乘 10）；這個預設值在 mimalloc 裡是寫死的，要改預設只能在 `main` 開頭呼叫 `mi_option_set`（`unsafe`），使用者選了只寫文件（第 9 條）。
- **完成時的驗證**（下載的 `kun-v152.2.0-11` prebuilt）：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings` 通過；`cargo test --workspace --no-fail-fast` 3215 個通過、0 個失敗；`speedometer_test.py` 全部通過（1 個 skip：只在 Linux 跑的）；`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs` 通過。rebase 到 `main` 的 `24d240d4`（只多了 RFC 的文件）之後 `sanity.py docs` 通過。
