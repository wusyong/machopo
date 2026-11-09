# RFC 0081 的進度：canvas 的 raster 移出 main thread

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-23，Windows 11。在 `0081` 上做。

完成日期：2026-10-23，Windows 11（Intel Core i9-13900K）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 raster thread 和「交出去、要用時等」（第 4、5 條；原本的第 2 段併進來，「已決定的事項」第 6 條） | 完成 | 2026-10-23 |
| 2 不用等的來源：canvas 當 `drawImage`、`createPattern` 的來源，`putImageData` | 完成（併進第 1 段） | 2026-10-23 |
| 3 讀回多次的 canvas 搬回 main thread（「已決定的事項」第 9、10 條） | 完成 | 2026-10-23 |

## 驗收

**快照和等待點（`gfx`、`raster`、`compositor`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 還沒填的快照：等到填好才讀；沒等就讀會 panic；填的一方丟掉時「不會來了」 | `gfx` 的 `a_pending_snapshot_is_read_once_filled`、`a_pending_snapshot_is_not_read_before_waiting`、`a_dropped_filler_says_the_pixels_never_come` |
| raster 等快照才畫，沒來的不畫；compositor 等快照才上傳，沒來的不畫那個 layer | `raster` 的 `a_pending_snapshot_is_drawn_once_filled`；`compositor` 的 `a_texture_layer_waits_for_its_snapshots_pixels` |

**canvas raster thread（`renderer`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 錄滿和 frame 之前的 raster 不在 main thread 上 | `canvas::tests::a_flush_sends_the_recording_to_the_thread_and_forgets_it`（main thread 上 `raster::draws_into_for_testing` 不變）；`dom_api` 的 `a_long_recording_is_drawn_without_waiting`（錄滿時是 `CanvasFlush::Draw`） |
| 照送來的順序畫，`WritePixels` 也是；快照是做的時候的內容（copy-on-write），canvas 畫自己、畫另一個都對 | `canvas_raster::tests::a_bitmap_is_drawn_in_order_and_its_snapshots_copied_on_write`、`a_bitmap_draws_snapshots_as_they_were_made` |
| 兩種上限：script 中途的 op、落後的 frame；一個 frame 的 canvas 是一批 | `canvas_raster::tests::the_main_thread_waits_past_either_limit`、`what_is_sent_counts_until_drawn`、`a_frame_too_far_ahead_waits`；`canvas::tests::a_frame_sends_its_canvases_at_once` |
| 沒有 context 拿著的 bitmap 在下一個 frame 釋放 | `canvas_raster::tests::a_bitmap_no_context_holds_is_let_go_of`、`canvas::tests::a_frame_sends_its_canvases_at_once` |
| thread panic 或結束之後沒人等它 | `canvas_raster::tests::a_thread_that_panicked_is_waited_for_by_nobody`、`a_closed_thread_drops_what_it_is_sent` |
| frame 顯示 frame 之前錄的全部 | 情境 `canvas-animation` |
| Charts-chartjs、Perf-Dashboard 的 raster 不在 main thread 上：120.8 → 57.7 ms（−52.2%）、128.5 → 68.4 ms（−46.7%），記憶體的高峰差不到 1% | `python3 scripts/bench.py speedometer --suites Charts-chartjs,Perf-Dashboard --against 29781da1`（「做過的檢查」的量測） |

**script 看到的（`dom_api`、`browser`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 讀像素同步等，結果不變 | `dom_api` 的 `a_bitmap_another_thread_has_reads_back_its_snapshot`；`browser` 的 `canvas_2d`、`canvas_images`、`canvas_export`；WPT 的 `html/canvas/element/` |
| canvas 當來源、`putImageData` 照順序錄進錄製，不等 | `dom_api` 的 `a_canvas_another_thread_has_is_drawn_as_its_snapshot`、`pixels_put_on_a_bitmap_another_thread_has_are_sent_in_order`、`pixels_put_without_end_are_sent_without_waiting` |
| `willReadFrequently` 在 main thread 上畫 | `renderer` 的 `a_canvas_read_frequently_is_drawn_here` |
| `getImageData` 讀回 2 次搬回 main thread，之後在那裡畫、內容不變；其他的讀不算；快照的像素沒來時不搬；`reset()` 留著 | `dom_api` 的 `a_bitmap_read_back_twice_is_taken_back`、`a_bitmap_not_read_back_from_another_thread_stays`；`renderer` 的 `a_canvas_read_frequently_is_drawn_here`；`dom` 的 `a_reset_keeps_where_the_bitmap_is_drawn`；`browser` 的 `canvas_2d`（`taken`） |

## 做過的檢查

2026-10-23，Windows 11（Intel Core i9-13900K），全部在這台上做。

- **做了什麼**（設計見 `README.md` 的「詳細設計」）：
  - `gfx` 的 `ImageData::pending`、`wait_ready` 和 `PixelsFiller`；等的地方：compositor 上傳、`RasterCache::image`、`dom_api` 的讀回。
  - `renderer` 的 `canvas_raster.rs`（`CanvasRaster`：每個 renderer 一條 thread、`Backlog` 的兩個上限、`Weak<CanvasResource>` 的釋放），`canvas.rs` 的 `flush` 和 `flush_shown` 交給它。
  - `putImageData` 在 thread 上的 bitmap 是 `PaintOp::WritePixels`，不 flush。
  - `willReadFrequently` 的、`getImageData` 讀回 2 次搬回來的（`CanvasData` 的 `readbacks`、`drawn_here`）在 main thread 上畫。
- **測試時找到的**：
  - 第一版的 `putImageData` 每次部分的寫入都先 flush，連續的寫入一次一批送出；改成 bitmap 在 thread 上時不 flush，直接錄成 `WritePixels`。
  - 情境 `canvas-animation` 第一次失敗（畫面沒變），改動之前也一樣：頁面用 `hsl()` 的 `fillStyle`，點都是白的；頁面改用十六進位的顏色。`hsl()` 沒作用不是 canvas 的問題：kun 的 CSS 還沒有 `hsl()`（`docs/limitations.md`），解析失敗、`fillStyle` 不變。
  - `sanity.py boundary`：`raster` 給測試用的 thread-local 計數器 `DRAWS_INTO` 加進 `STATICS_ALLOWED`（和其他測試的計數器一樣）。
- **mutation test**：
  - `mutants-1.toml`（第 1 段，45 個）：全部抓到。5 個是卡住（拿掉喚醒、或 backlog 不關，等的一方一直等）。第一次的 timeout 是 600 秒，跑了 40 個之後停掉，改成 120 秒（基準最久 13 秒）再用 `--only` 跑剩下的 5 個。
  - `mutants-3.toml`（第 3 段，11 個）：「a taken back canvas goes to the thread again」第一次沒抓到：`renderer` 的測試沒有改到（改測試的腳本在它那裡停了，cargo fmt 換過那段的排版），補上之後抓到。其他 10 個第一次就抓到。
- **WPT**：`html/canvas/element/` 和 `html/semantics/embedded-content/the-canvas-element/` 2472 項（`putImageData` 改寫之前的 build），之後 `pixel-manipulation/` 142 項、第 3 段的 `pixel-manipulation/` 和 `the-canvas-element/` 344 項：都沒有預期之外的結果。
- **reftest 和情境**：`test.py reftest canvas/`；情境 `canvas`、`canvas-animation`。
- **量測**（做完之後，使用者要求）：`bench.py speedometer --suites Charts-chartjs,Perf-Dashboard --against 29781da1`，開始之前的 `29781da1` 和做完的 `ef62ef3e` 輪流各 3 次、10 個 iteration：Charts-chartjs 120.8 ms（119.8–123.6）→ 57.7 ms（56.6–59.2），Perf-Dashboard 128.5 ms（127.1–130.2）→ 68.4 ms（67.8–69.1）；記憶體的高峰 535 → 531 MB、534 → 538 MB。和 Chrome（24.1、33.2 ms）比，從 5.06、3.85 倍變成 2.39、2.06 倍。剩下的差沒有取樣（`docs/performance-gaps.md` 的「可以開 RFC 的方向」第 1 項）。
- **最後的驗證**（第 3 段之後）：`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`；`cargo test --workspace`（3671 個，3 個 ignored）。
