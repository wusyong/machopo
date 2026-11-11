# RFC 0095 的進度：tile 的 raster 花在哪裡、上傳，和 GPU raster（vello_gpu）的評估

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-11，Windows 11。在 `0095` 上做。

完成日期：2026-11-11，Windows 11（AMD Radeon RX 7900 XTX）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0 量測：上傳和 raster 各部分的 span、「GPU 畫得了嗎」的計數、`bench.py raster` 的報告 | 完成 | 2026-11-11 |
| 1a 改上傳：分開量 write；一輪一次 submit、tile 只建一個 bind group | 完成 | 2026-11-11 |
| 1b 重用 texture（Viz 這一側的池子） | 完成 | 2026-11-11 |
| 2 GPU raster 的原型和之後 | 延後（`README.md` 的已決定的事項第 6 條） | |

## 驗收

**量測**

| 項目 | 怎麼驗證 |
| --- | --- |
| playback 回報哪些東西只有 CPU 能畫，每一類分開，照實際畫的算 | `cargo test -p raster playback_tests::a_playback_tells_what_only_a_cpu_draws` |
| compositor 照它數 tile（GPU 畫得了的、只差圖片的、每一類的；單色的 tile 也算） | `cargo test -p compositor tiles_are_counted_by_what_only_a_cpu_draws`、`a_new_display_list_rasters_only_the_tiles_its_invalidation_touches` |
| `bench.py raster` 報出 raster 各部分的時間和 tile 的計數 | `python3 scripts/bench/raster/raster_test.py`；`python3 scripts/bench.py raster` 的第二張表 |

**上傳**

| 項目 | 怎麼驗證 |
| --- | --- |
| 一輪的 shared image 全部建完才 flush 一次，沒有新的不 flush | `cargo test -p compositor shared_images_are_flushed_once_all_of_a_frames_are_made` |
| bilinear 用的 bind group 第一次那樣畫才建、之後共用；畫出來照舊 | `cargo test -p viz a_texture_has_what_draws_it_bilinearly_once_it_is_drawn_so`、`a_texture_is_sampled_bilinearly_unless_it_asks_for_the_nearest_texel` |
| 放掉的 shared image 把 texture 留給下一張同樣大小的，重用之後畫出來是新的像素；還有人拿著時不還回去 | `cargo test -p viz a_shared_image_let_go_gives_its_texture_to_the_next_of_its_size` |
| 池子留最新的，有張數和位元組的上限，太大的不留，同大小拿最後放的 | `cargo test -p viz the_texture_pool_keeps_the_newest_within_its_limits` |
| 畫出來的東西沒有變 | `python3 scripts/test.py reftest`（195 個）、`scenarios`（86 次） |

## 做過的檢查

每一步當時的完整紀錄在這個檔案的 git 歷史裡（`6dda0d5`）。

### 最後的驗證

- 第 1 步做完時（2026-11-11）：`cargo test --workspace --no-fail-fast` 3834 個通過、0 個失敗；`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`、`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs 都過；`python3 scripts/test.py scenarios` 86 次執行 0 個失敗；`python3 scripts/test.py reftest` 195 個都和預期一樣。之後只改了文件。
- 沒有跑的：`test.py automation`（真的視窗；上傳的路在 headless 和視窗是同一段程式，但視窗被遮住時的行為沒有看）；WPT；macOS 和 Linux；軟體的 adapter（CI 的 lavapipe 上上傳的時間是另一回事，要等 CI）。

### 量測

`python3 scripts/bench.py raster`：headless、scale 2、256 的 tile、一條 raster thread、`perf` profile。headless 用的也是這張顯示卡（log 裡的 adapter 是 Radeon RX 7900 XTX、DX12）。量的都是 compositor thread 上的時間，不含 GPU 那一側。SQLite 的捲動沒有畫任何 tile，不列。

**開始時，各部分佔 `PrepareTiles`**（三輪，比例每一格差不到 2 個百分點，下面是中間的那一輪；ms）：

| 頁面 | 階段 | `PrepareTiles` | 建場景 | render | 讀像素 | 上傳（write、submit） | 畫的 tile | GPU 畫得了 | 只差圖片 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Gutenberg | 捲動 | 469.6 | 228.4（49%） | 73.9（16%） | 6.0（1%） | 128.2（27%；84.8、40.7） | 649 | 546（84%） | 103（16%） |
| Gutenberg | 改視窗大小 | 1001.7 | 426.8（43%） | 270.7（27%） | 13.4（1%） | 271.0（27%；174.4、69.1） | 6433 | 4832（75%） | 1601（25%） |
| SQLite | 改視窗大小 | 812.5 | 487.2（60%） | 90.7（11%） | 10.1（1%） | 208.4（26%；137.4、56.0） | 5381 | 5381（100%） | 0 |
| NewsSite-Next | 捲動 | 290.4 | 65.8（23%） | 68.5（24%） | 11.5（4%） | 97.9（34%；66.9、28.7） | 1077 | 816（76%） | 261（24%） |
| NewsSite-Next | 改視窗大小 | 1299.1 | 267.7（21%） | 489.9（38%） | 43.9（3%） | 458.6（35%；306.1、119.8） | 11813 | 7937（67%） | 3876（33%） |

- **這三頁畫不了的 tile 全部是因為圖片**：mask、filter、blur、圖案、點陣 glyph 都是 0。這三頁都是簡單的頁面（文字、圖片、色塊）；有 `filter`、`box-shadow`、`clip-path` 的頁面沒有量。
- Gutenberg 的捲動每一輪差很多（這一輪 470，另有一輪 273），比例不變；RFC 0007 量的時候就不穩，下面的比較不列它。

**Viz 的 write 那一段分開量**（佔 `PrepareTiles`，五個階段的範圍；每個 tile 是 Gutenberg 改視窗大小的）：建 texture 5% 到 7%（約 10 µs）；`write_texture` 的複製 8% 到 11%（約 16 µs）；建兩個 bind group 3% 到 6%（約 8 µs）；submit 7% 到 9%（約 13 µs）；其餘（鎖、介面、span）約 2%。

**改上傳的效果**（三輪的中間值；每次上傳是 `TileManager::UploadTile` 的合計除以次數，直接量到的）：

| 頁面 | 階段 | 每次上傳（µs）：開始 → 一輪一次 submit、一個 bind group → 重用 texture | 上傳佔 `PrepareTiles` | 每個 tile 的 `PrepareTiles`（µs）：開始 → 最後 |
| --- | --- | --- | --- | --- |
| Gutenberg | 改視窗大小 | 49 → 29 → 19 | 27% → 18% → 13% | 177 → 138（-22%） |
| SQLite | 改視窗大小 | 45 → 24 → 17 | 26% → 16% → 11% | 185 → 150（-19%） |
| NewsSite-Next | 捲動 | 104 → 58 → 44 | 34% → 22% → 17% | 239 → 223（-7%） |
| NewsSite-Next | 改視窗大小 | 45 → 26 → 20 | 35% → 24% → 20% | 128 → 92（-28%） |

- submit 的合計從 60 到 120 ms 降到 2 到 5 ms（改視窗大小的三格）。第一步降得比 submit 加一個 bind group 還多（每次少 22 µs，那兩項合計約 17 µs）：推論是不再每個 tile submit 之後，後面的建 texture 和寫入也變快；沒有再查。
- 剩下的上傳幾乎都是 `write_texture` 的複製。
- **每個 tile 的 `PrepareTiles` 那一欄只能看大概**：改視窗大小的階段每次畫的 tile 數不同，所以用每個 tile 的時間比；而同一份 `raster::playback` 的程式在不同的 build 之間，每個 tile 就差 -3% 到 +11%（重用 texture 前後 playback 的程式沒有變，SQLite 是 121 → 129 µs）。重用 texture 對整體的效果在這一欄看不出來，只在上傳自己的時間上看得到。
- NewsSite-Next 改視窗大小時池子沒命中的次數比較多（bind group 還有 30 ms，其他格 3 到 4 ms）：那一頁同時活著的 tile 多、大小也多。命中率沒有另外量；上限（32 MB、256 張）是保守的起點，沒有試別的值。

**最後，各部分佔 `PrepareTiles`**（最後一輪）：

| 頁面 | 階段 | 建場景 | render | 讀像素 | 上傳 |
| --- | --- | --- | --- | --- | --- |
| Gutenberg | 捲動 | 57% | 19% | 2% | 13% |
| Gutenberg | 改視窗大小 | 50% | 33% | 2% | 13% |
| SQLite | 改視窗大小 | 71% | 13% | 2% | 11% |
| NewsSite-Next | 捲動 | 27% | 31% | 5% | 17% |
| NewsSite-Next | 改視窗大小 | 25% | 47% | 4% | 20% |

- GPU raster 還能拿走的上限（render、讀像素和上傳）：SQLite 26%，Gutenberg 34% 到 48%，NewsSite-Next 53% 到 71%；要扣掉 vello_gpu 自己在 CPU 上的工作，那一段沒有量過。
- 沒有量的：真的視窗；scale 1；多條 raster thread；軟體的 adapter；GPU 那一側的時間。

### Mutation test

`python3 scripts/test.py mutate rfcs/0095-vello-gpu/mutants-<步>.toml`，每份跑完都確認檔案還原了。

| 清單 | 條數 | 結果 |
| --- | --- | --- |
| `mutants-0.toml`（每一類各自不記或記錯、畫到旁邊的不算、字型的點陣 glyph、藏起來的 filter layer、compositor 的計數） | 18 | 都抓到 |
| `mutants-1a.toml`（從不 flush、沒建也 flush、每個 tile 之後都 flush；bilinear 的 bind group 跟著 texture 一起建、用錯 sampler、拿錯 bind group） | 7 | 都抓到 |
| `mutants-1b.toml`（池子的取用、上限和淘汰；丟掉時不還、重用時不寫像素、從不取用） | 11 | 都抓到。第一輪「同大小拿最舊的」沒抓到，是測試沒有分出來，補了斷言 |

沒有 mutation 的：span 本身（拿掉只是 trace 少一段，`bench.py raster` 的那幾欄變成 0）；playback panic 時回報「什麼都沒有」；`flush` 裡有沒有真的 submit（`docs/test-gaps.md` 的「沒有計數守著的優化」）。

### 不穩定的測試和工具

都和這份 RFC 的改動無關（改動全部暫存、乾淨的 tree 上一樣）：

- **`shell` 的 `wasm_documents` 的兩個測試**（`a_wasm_documents_script_runs_and_what_it_adds_is_drawn`、`a_guest_focuses_and_blurs_an_element`）：整個 binary 平行跑 7 次失敗 7 次（有改動的 3 次、池子關掉的 2 次、乾淨的 tree 2 次），`--test-threads=1` 通過，只跑那一組也通過。原因：每個測試的 `page()` 都把 `page.wasm` 重新複製進同一個暫存目錄，另一個測試的頁面正在抓它時拿到被截斷的檔案，載入成功但 guest 沒跑。改成整個 process 只複製一次之後平行跑 3 次、workspace 1 次都過（`14a3c24`，單獨的 commit）。今天稍早的兩次 workspace 測試沒有碰到，為什麼之後每次都碰到沒有查。
- `shell` 的 `devtools_initiators::script_asks_with_its_stack`：9 次裡失敗 2 次（一次在乾淨的 tree 上）；RFC 0007 記過。沒查。
- `browser` 的 `navigation::links::idle_callbacks`：失敗 1 次，之後整個 binary 2 次、單獨 3 次、workspace 1 次都過。沒查。
- `cargo fmt` 寫 `wgpu_renderer.rs` 失敗過一次、`test.py mutate` 寫回 `gpu_context.rs` 失敗過一次（都是 Windows 回的檔案操作錯誤，重跑就好；`mutate` 用 `--restore` 放回去再整份重跑）：這台機器上有東西偶爾佔著原始檔，RFC 0007 也碰過，原因沒查。

### 順便改的文件

`docs/limitations.md` 和 `docs/benchmarks.md` 原本寫「真的視窗才有上傳到 GPU 的時間」：headless 和 windowless 也經過同一個介面上傳，改掉了。`crates/shell/src/headless.rs` 開頭的說明還寫著 software 的 renderer（調查時看到的，沒有對過、沒有改）。
