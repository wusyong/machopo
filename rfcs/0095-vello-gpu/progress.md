# RFC 0095 的進度：tile 的 GPU raster（vello_gpu）

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-11，Windows 11。在 `0095` 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0 量測：上傳和 raster 各部分的 span、「GPU 畫得了嗎」的計數、`bench.py raster` 的報告 | 完成 | 2026-11-11 |
| 1a 改上傳：分開量 write；一批一次 submit、tile 只建一個 bind group | 完成 | 2026-11-11 |
| 1b 重用 texture（Viz 這一側的池子） | 完成 | 2026-11-11 |
| 2 之後（GPU raster 的原型） | 等第 1 步做完再量、再決定 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 第 0 步：playback 回報哪些東西只有 CPU 能畫，每一類分開，照實際畫的算 | `cargo test -p raster playback_tests::a_playback_tells_what_only_a_cpu_draws` |
| 第 0 步：compositor 照它數 tile（GPU 畫得了的、只差圖片的、每一類的） | `cargo test -p compositor tiles_are_counted_by_what_only_a_cpu_draws`、`a_new_display_list_rasters_only_the_tiles_its_invalidation_touches` |
| 第 0 步：`bench.py raster` 報出 raster 各部分的時間和 tile 的計數 | `python3 scripts/bench/raster/raster_test.py`；`python3 scripts/bench.py raster` 的第二張表 |
| 第 1a 步：一輪的 shared image 全部建完才 flush 一次，沒有新的不 flush | `cargo test -p compositor shared_images_are_flushed_once_all_of_a_frames_are_made` |
| 第 1a 步：bilinear 用的 bind group 第一次那樣畫才建、之後共用；畫出來照舊 | `cargo test -p viz a_texture_has_what_draws_it_bilinearly_once_it_is_drawn_so`、`a_texture_is_sampled_bilinearly_unless_it_asks_for_the_nearest_texel` |
| 第 1b 步：放掉的 shared image 把 texture 留給下一張同樣大小的，重用之後畫出來是新的像素；還有人拿著時不還回去 | `cargo test -p viz a_shared_image_let_go_gives_its_texture_to_the_next_of_its_size` |
| 第 1b 步：池子留最新的，有張數和位元組的上限，太大的不留，同大小拿最後放的 | `cargo test -p viz the_texture_pool_keeps_the_newest_within_its_limits` |
| 第 1 步整個階段 | `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`、`sanity.py` 七項、`python3 scripts/test.py scenarios`、`reftest` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 第 0 步：量測（2026-11-11，Windows 11，AMD Radeon RX 7900 XTX）

**做了什麼**（不改 tile 怎麼畫）：

- **span**：`raster::scene`（把 op 畫進 Vello 的 context：攤平、strip、覆蓋率）和 `raster::render`（填色、合成成像素），都在 `raster::playback` 裡；`TileManager::ScanTile`（畫完讀像素看是不是單色）、`TileManager::UploadTile`（呼叫 Viz 的介面），裡面是 Viz 的 `SharedImage::Write`（建 texture、`write_texture`、兩個 bind group）和 `SharedImage::Submit`。
- **`raster::CpuOnly`**：playback 走到只有 CPU 能畫的路時記下來（已決定的事項第 3 條），`playback_into` 連同大小一起回傳（`Played`）。六類：圖片、mask（`ClipPaths`）、Vello 沒有的 filter（`ColorMatrix`、串接的）、blur（layer 的 blur 和 drop shadow、模糊的形狀、畫到旁邊的陰影）、圖案（record 或有間隔的）、有點陣 glyph 的字型。畫到旁邊的 layer 裡面的東西也算進來。字型有沒有點陣 glyph 跟著字型的快取記一次。
- **compositor**：`prepared tiles` 的 log 多了 `gpu`（一類都沒有的 tile）、`gpu_but_images`（只有圖片那一類的）和每一類的個數；一個 tile 可以算進好幾類。
- **`bench.py raster`**：報告多一張表（各部分的時間和佔 `PrepareTiles` 的比例、tile 的計數），`raster.json` 多 `parts`。

**量到的**（`python3 scripts/bench.py raster`，headless、scale 2、256 的 tile、一條 raster thread、`perf` profile；headless 用的也是這張顯示卡，log 裡的 adapter 是 Radeon RX 7900 XTX、DX12；跑三輪，下面是中間的那一輪，ms 和佔 `PrepareTiles` 的比例）：

| 頁面 | 階段 | `PrepareTiles` | 建場景 | render | 讀像素 | 上傳（write、submit） | 畫的 tile | GPU 畫得了 | 只差圖片 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Gutenberg | 捲動 | 469.6 | 228.4（49%） | 73.9（16%） | 6.0（1%） | 128.2（27%；84.8、40.7） | 649 | 546（84%） | 103（16%） |
| Gutenberg | 改視窗大小 | 1001.7 | 426.8（43%） | 270.7（27%） | 13.4（1%） | 271.0（27%；174.4、69.1） | 6433 | 4832（75%） | 1601（25%） |
| SQLite | 改視窗大小 | 812.5 | 487.2（60%） | 90.7（11%） | 10.1（1%） | 208.4（26%；137.4、56.0） | 5381 | 5381（100%） | 0 |
| NewsSite-Next | 捲動 | 290.4 | 65.8（23%） | 68.5（24%） | 11.5（4%） | 97.9（34%；66.9、28.7） | 1077 | 816（76%） | 261（24%） |
| NewsSite-Next | 改視窗大小 | 1299.1 | 267.7（21%） | 489.9（38%） | 43.9（3%） | 458.6（35%；306.1、119.8） | 11813 | 7937（67%） | 3876（33%） |

- 三輪的比例幾乎一樣（每一格差不到 2 個百分點）；Gutenberg 的捲動有一輪整體只有 273 ms（另外兩輪 470、485），比例不變。這一格 RFC 0007 量的時候就不穩。
- SQLite 的捲動沒有畫任何 tile（RFC 0007 也是）。
- **上傳佔 `PrepareTiles` 的 26% 到 35%**：每個 tile 約 45 到 49 µs（Gutenberg 改大小：267.7 ms、5484 次），其中 Viz 的 write 那一段約 32 µs、submit 約 13 µs，其餘是鎖和介面。write 那一段裡建 texture、`write_texture`、建 bind group 各佔多少沒有再分。
- **render 佔 11% 到 38%**：文字多的頁面低（SQLite 11%），圖和色塊多的高（NewsSite-Next 改大小 38%）。
- **建場景佔 21% 到 60%**：這一段 GPU raster 也留在 CPU 上。
- **讀像素看單色佔 1% 到 4%**。
- **這三頁畫不了的 tile 全部是因為圖片**：mask、filter、blur、圖案、點陣 glyph 都是 0。有了圖片的 GPU 快取，這三頁每個 tile 都走得了 GPU。這三頁都是簡單的頁面（文字、圖片、色塊）；有 `filter`、`box-shadow`、`clip-path` 的頁面沒有量。
- **兩個候選各自的上限**（都是上限，不是預估）：
  - 只改上傳：一次 `PrepareTiles` 只 submit 一次，省得到 submit 的那 7% 到 10% 的大部分；重用 texture 省得到 write 那一段的一部分（17% 到 24% 裡建 texture 和 bind group 的部分，多少沒有量）。上限是整段上傳：26% 到 35%。
  - GPU raster：render、讀像素和上傳都不用做，合計 Gutenberg 捲動 44%、改大小 55%，SQLite 38%，NewsSite-Next 捲動 62%、改大小 76%；要扣掉 vello_gpu 自己在 CPU 上的工作（編 paint、把 strip 和覆蓋率經過 staging 上傳、錄 render pass），這一段沒有量過，原型才知道。只對走得了 GPU 的 tile 成立：沒有圖片的 GPU 快取時是 67% 到 100% 的 tile。
- 沒有量的：真的視窗（`--window`）；scale 1；多條 raster thread；lavapipe 和 WARP（上傳在軟體的 adapter 上是另一回事）；GPU 那一側的時間（這裡量的都是 compositor thread 上的時間）。

**跑過的**（階段的檢查）：`cargo test --workspace --no-fail-fast` 3830 個通過、0 個失敗；`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`、`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs 都過。`python3 scripts/bench/raster/raster_test.py` 不用瀏覽器的 4 個過（要瀏覽器的那幾個沒有另外跑，`bench.py raster` 本身跑了三輪）。沒有跑的：reftest、情境、WPT（畫的東西沒有變）。

**Mutation test**（`mutants-0.toml`，18 條都抓到）：

| 弄壞了什麼 | 哪個測試失敗 |
| --- | --- |
| 圖片、圖片的圖案、畫成 pixmap 的圖案、`ClipPaths`、模糊的形狀、畫到旁邊的陰影各自不記；圖案記成圖片 | `a_playback_tells_what_only_a_cpu_draws` |
| filter 都算成 blur；drop shadow 算成 Vello 沒有的 filter | 同上 |
| 畫到旁邊的 layer 裡面的不算；合併時漏掉圖片 | 同上 |
| 沒有字型有點陣 glyph；每個字型都有 | 同上 |
| 藏在 tile 外面的 clip 裡的 filter layer 也記 | 同上 |
| 只有 CPU 能畫的 tile 算成 GPU 的；不只差圖片的算成只差圖片；blur 算成 filter | `tiles_are_counted_by_what_only_a_cpu_draws` |
| 單色的 tile 不算 | `a_new_display_list_rasters_only_the_tiles_its_invalidation_touches` |

跑完檔案都還原了（`target/mutate/originals/` 是空的）。沒有 mutation 的：span 本身（拿掉只是 trace 少一段，`bench.py raster` 的那幾欄變成 0）；playback panic 時回報「什麼都沒有」（沒有測試）。

### 第 1a 步：一批一次 submit、tile 只建一個 bind group（2026-11-11，Windows 11，AMD Radeon RX 7900 XTX）

**先把 Viz 的 write 那一段分開量**（多三個 span：`SharedImage::CreateTexture`、`WriteTexture`、`BindGroups`；兩輪，佔 `PrepareTiles` 的比例，五個階段的範圍）：

| 上傳裡的哪一段 | 佔比 | 每個 tile（Gutenberg 改大小） |
| --- | --- | --- |
| 建 texture | 5% 到 7% | 約 10 µs |
| `write_texture`（複製進 wgpu 的 staging） | 8% 到 11% | 約 16 µs |
| 建兩個 bind group（連同 texture 的 view） | 3% 到 6% | 約 8 µs |
| submit | 7% 到 9% | 約 13 µs |
| 其餘（鎖、介面、span） | 約 2% | 約 3 µs |

**做了什麼**：

- `SharedImageInterface` 多 `flush`（Chromium 的介面的 `Flush`）：`create_shared_image` 不再 submit，`prepare_tiles` 把這一輪的 tile 和 canvas 的 texture 都建完之後呼叫一次；一張都沒建時不呼叫。`GpuSharedImages::flush` 拿 `submit_lock`、送一次空的 submit。
- `UploadedResource` 的 bilinear 的 bind group 改成第一次那樣畫才建（`GpuContext::linear`，在 Viz thread 上；`Arc<OnceLock>`，renderer 複製的那一份共用）。tile 永遠用 nearest，不再為它建第二個。

**量到的**（改完之後三輪；`PrepareTiles` 除以畫的 tile 數，因為改視窗大小的階段每次畫的 tile 數不同；「之前」是分開量 write 的那兩輪，同一天、同一台機器、不同的 build）：

| 頁面 | 階段 | 之前，每個 tile（µs） | 之後 | 上傳佔 `PrepareTiles`：之前 → 之後 | 每次上傳（µs）：之前 → 之後 |
| --- | --- | --- | --- | --- | --- |
| Gutenberg | 改視窗大小 | 177 | 152（-14%） | 27% → 18% | 51 → 29 |
| SQLite | 改視窗大小 | 185 | 148（-20%） | 27% → 16% | 51 → 24 |
| NewsSite-Next | 捲動 | 239 | 218（-9%） | 33% → 22% | 90 → 58 |
| NewsSite-Next | 改視窗大小 | 128 | 99（-22%） | 37% → 24% | 51 → 26 |

- submit 的合計從 60 到 120 ms 降到 2 到 5 ms（改視窗大小的三格）；bind group 從 35 到 80 ms 降到 18 到 50 ms。
- 上傳降得比 submit 加一個 bind group 還多（Gutenberg 改大小：每次少 22 µs，那兩項合計約 17 µs）：推論是每個 tile 之後不再 submit，後面的建 texture 和寫入也變快；沒有再查。
- Gutenberg 的捲動沒有列：這一格每一輪差很多（第 0 步就是），之前 470 左右、之後 360 到 400，方向一樣但數字不可靠。
- 不同 build 之間每個 tile 的時間本來就可以差幾個百分點（RFC 0007 第 3b 步量過）；上傳自己的 span 和 submit 的合計是直接量到的，比整體的百分比可靠。
- 之後上傳還佔 16% 到 24%：建 texture、複製和剩下的一個 bind group。

**測試**：上面「驗收」的兩列。`cargo test -p viz`（37 個）、`-p compositor`（128 個）過；`cargo check --workspace --all-targets` 過。階段的檢查、情境和 reftest 和第 1b 步一起跑（下面）。

**Mutation test**（`mutants-1a.toml`，7 條都抓到）：從不 flush、沒建也 flush、tile 的 shared image 不算、每個 tile 之後都 flush → `shared_images_are_flushed_once_all_of_a_frames_are_made`；bilinear 的 bind group 跟著 texture 一起建 → `a_texture_has_what_draws_it_bilinearly_once_it_is_drawn_so`；它用 nearest 的 sampler、bilinear 的 quad 拿 nearest 的 bind group → `a_texture_is_sampled_bilinearly_unless_it_asks_for_the_nearest_texel`。第一次跑到一半 `test.py mutate` 寫回 `gpu_context.rs` 失敗（`OSError: [Errno 22]`，RFC 0007 記過的同一個問題），`--restore` 放回去之後整份重跑；跑完 `target/mutate/originals/` 是空的。

**測試守不住的**：`flush` 裡有沒有真的 submit（`docs/test-gaps.md` 的「沒有計數守著的優化」）。

**跟著改的文件**：`docs/limitations.md`（上傳怎麼做的那一句，原本寫「真的視窗裡」才有，headless 其實一樣）、`docs/benchmarks.md`（報告的第二張表；headless 也有上傳；比較時用每個 tile 的時間）。

### 第 1b 步：重用 texture（2026-11-11，Windows 11，AMD Radeon RX 7900 XTX）

**做了什麼**（已決定的事項第 5 條）：`viz` 的 `TexturePool`（`GpuContext` 上，一個 device 一個）。`SharedImageBacking` 被丟掉時把它的 `UploadedResource` 還回池子；`create_shared_image` 先 `take` 同樣大小的，有就只 `write_texture`，沒有才建新的。上限 32 MB、256 張，超過時放掉最舊的；單獨超過 32 MB 的不留。

**量到的**（三輪的中間值；每次上傳的時間是 `TileManager::UploadTile` 的合計除以次數，直接量到的）：

| 頁面 | 階段 | 每次上傳（µs）：第 0 步 → 1a → 1b | 上傳佔 `PrepareTiles`：第 0 步 → 1a → 1b | 每個 tile 的 `PrepareTiles`（µs）：第 0 步 → 1b |
| --- | --- | --- | --- | --- |
| Gutenberg | 改視窗大小 | 49 → 29 → 19 | 27% → 18% → 13% | 177 → 138（-22%） |
| SQLite | 改視窗大小 | 45 → 24 → 17 | 26% → 16% → 11% | 185 → 150（-19%） |
| NewsSite-Next | 捲動 | 104 → 58 → 44 | 34% → 22% → 17% | 239 → 223（-7%） |
| NewsSite-Next | 改視窗大小 | 45 → 26 → 20 | 35% → 24% → 20% | 128 → 92（-28%） |

- 上傳從每次 45 到 49 µs 降到 17 到 20 µs（改視窗大小的三格）；剩下的幾乎都是 `write_texture` 的複製。
- **每個 tile 的 `PrepareTiles` 那一欄不可靠到個位數的百分比**：1a 到 1b 之間 `raster::playback` 的程式沒有變，但每個 tile 的 playback 在不同的 build 之間差了 -3% 到 +11%（SQLite 121 → 129 µs，NewsSite-Next 捲動 119 → 132 µs）。所以 1b 對整體的效果在這一欄看不出來（SQLite 的 1a 是 148、1b 是 150）；上傳自己的時間是直接量到的，確實少了三成左右。
- NewsSite-Next 改視窗大小時 bind group 還有 30 ms（其他格 3 到 4 ms）：池子沒命中、建新的 texture 的次數比較多（那一頁同時活著的 tile 多、大小也多）。命中率沒有另外量；上限是保守的起點，沒有試別的值。
- Gutenberg 的捲動照舊不列（每一輪差很多）；這三輪是 351、362、387 ms，第 0 步是 470 左右。

**Mutation test**（`mutants-1b.toml`，11 條都抓到）：拿任何大小的、同大小拿最舊的、取用之後位元組沒減、太大的也留、不限張數、不限位元組、放掉最新的、放掉之後位元組沒減 → `the_texture_pool_keeps_the_newest_within_its_limits`；丟掉時不還、重用時不寫像素、從不取用 → `a_shared_image_let_go_gives_its_texture_to_the_next_of_its_size`。第一輪「同大小拿最舊的」沒抓到，是測試沒有分出來，補了斷言。跑完檔案都還原了。

**第 1 步整個階段的檢查**（1a 和 1b）：

- `cargo test --workspace --no-fail-fast`：3834 個通過、0 個失敗（修了下面的測試之後）。`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`、`sanity.py` 七項：過。
- `python3 scripts/test.py scenarios`：86 次執行、0 個失敗。`python3 scripts/test.py reftest`：195 個都和預期一樣。
- 沒有跑的：`test.py automation`（真的視窗；上傳的路在 headless 和視窗是同一段程式，但視窗被遮住時的行為沒有看）、WPT、多條 raster thread 的量測。

**不穩定的測試**（都和這份 RFC 的改動無關：改動全部暫存、乾淨的 tree 上一樣）：

- **`shell` 的 `wasm_documents` 的兩個測試**（`a_wasm_documents_script_runs_and_what_it_adds_is_drawn`、`a_guest_focuses_and_blurs_an_element`）：整個 binary 平行跑 7 次失敗 7 次（有改動的 3 次、池子關掉的 2 次、乾淨的 tree 2 次），`--test-threads=1` 通過，只跑那一組也通過。原因：每個測試的 `page()` 都把 `page.wasm` 重新複製進同一個暫存目錄，另一個測試的頁面正在抓它時拿到被截斷的檔案，載入成功但 guest 沒跑。改成整個 process 只複製一次（`std::sync::Once`）之後平行跑 3 次都過。這個修改單獨一個 commit。今天稍早的兩次 workspace 測試沒有碰到，為什麼之後每次都碰到沒有查。
- `shell` 的 `devtools_initiators::script_asks_with_its_stack`：9 次裡失敗 2 次（一次在乾淨的 tree 上）；RFC 0007 記過。沒查。
- `browser` 的 `navigation::links::idle_callbacks`：失敗 1 次，之後整個 binary 2 次、單獨 3 次、workspace 1 次都過。沒查。

**工具**：`cargo fmt` 寫 `wgpu_renderer.rs` 失敗過一次、`test.py mutate` 寫回 `gpu_context.rs` 失敗過一次（都是 Windows 回的檔案操作錯誤，重跑就好）：這台機器上有東西偶爾佔著原始檔，RFC 0007 也碰過，原因沒查。
