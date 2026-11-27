# RFC 0123 的進度：renderer 送什麼都不能讓 Browser 結束

開始日期：2026-11-27。完成日期：2026-11-27（Linux：Fedora 45，kernel 7.3.3，x86-64；GPU 是 AMD Radeon 890M（RADV），也用 llvmpipe（`MACHOPO_GPU_ADAPTER=cpu`，CI 的）跑；`ipc` 的實體記憶體在 macOS 和 Windows 只編過，沒跑測試）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點、探測（`details.md` 的「第 0 步的探測」「Chromium 怎麼做」）、已決定的事項第 1 到 6 條 | 做完 | 2026-11-27 |
| 1a. `protocol::viz` 的 `CompositorFrameSinkMsg::check`、`MAX_QUADS`、`MAX_RENDER_PASSES`；Browser 的 IO thread 解開就檢查、不過的是 `BadMessageReason::FrameSink`；Viz 收到的每個 frame 照同樣的檢查、只有 root 嵌別的 surface（第 3 段時拿掉了：已決定的事項第 11 條）；renderer 的 compositor 守上限、debug build 送出前檢查 | 做完 | 2026-11-27 |
| 1b. 測試、第 1 段的檢查和文件 | 做完 | 2026-11-27 |
| 2a. 已決定的事項第 7 條；`GpuContext::scoped`（三種 error scope）和 uncaptured error 的 handler，照種類處理（`GpuError`）；畫一個 frame、上傳和送出 shared image 都在 scope 裡，有錯誤的 frame 不 present、不給 copy；頂點照 `MAX_QUADS` 分成幾個自己建的 buffer，不用 `create_buffer_init` | 做完 | 2026-11-27 |
| 2b. 測試、第 2 段的檢查和文件 | 做完 | 2026-11-27 |
| 3a. 已決定的事項第 8 到 10 條；`ipc::platform::amount_of_physical_memory`；`browser::tile_memory_limit`、`shared_image_limits`（Blink 的 `GetGpuMemoryPolicy`）；`RendererFlags::tile_memory_limit`；compositor 照距離給 tile 的記憶體（`assign_tile_memory`）、放不下的可見 tile 畫成背景色；Viz 的 `SharedImageLimits` | 做完 | 2026-11-27 |
| 3b. 量測（`details.md` 的「第 3 段的量測」）；已決定的事項第 11 到 13 條（Viz 不再檢查 frame；commit 時放掉 active tree 的 tile；單色的 tile 不佔預算） | 做完 | 2026-11-27 |
| 3c. 測試、第 3 段的檢查和文件 | 做完 | 2026-11-27 |
| 4a. 已決定的事項第 14、15 條（不加 fuzzer；lint 在 `browser`、`viz`、`net` 和 `protocol` 的 `viz.rs`）；五個 lint 打開，80 處改寫或寫明理由；規則寫進 `crates/base/README.md` 和 `CLAUDE.md` | 做完 | 2026-11-27 |
| 4b. 第 4 段的檢查和文件 | 做完 | 2026-11-27 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 說不通的訊息 | 每一種（沒有 pass、root 的輸出是空的或 NaN、pass、quad、shared quad state 超過上限（剛好在上限的收）、quad 的 state 和 resource 不是 frame 的、resource 的 id 重複、resource 的像素和大小不合或帶著 shared image 又帶像素、超過 `MAX_TEXTURE_SIZE`、renderer 嵌 surface、`CreateSharedImage` 的大小和像素）被擋；誠實的 frame、NaN 和無限大的座標收 | `cargo test -p protocol --lib viz::` |
| 壞訊息 | renderer process 送說不通的 frame、shared image、嵌別人 surface 的 frame 時，分頁收到 `BadMessageReason::FrameSink`、process 被結束、Viz 沒收到；說得通的到 Viz、從綁給它的 sink | `cargo test -p browser --test it renderer_process::what_viz_is_sent_that_cant_be_right_ends_its_renderer` |
| Viz | 不檢查它收到的 frame（已決定的事項第 11 條），說不通的也畫得下去、沒有 wgpu 的錯誤（不是 frame 的 state 用預設的、不是 frame 的 resource 不畫、嵌回 root 的停在那裡）；沒建過的 mailbox 不畫，帶著像素也一樣 | `cargo test -p viz --lib`（`what_cant_be_right_is_drawn_as_far_as_it_can_be`、`a_shared_image_nobody_made_isnt_drawn`） |
| wgpu 的錯誤 | scope 接到它包著的錯誤、只接那些，device 照常；沒有 scope 包著的由 handler 收、不 panic；驗證錯誤在 debug build panic（測試容許時不）、記憶體不夠放掉池子、內部錯誤讓 device 當成遺失；wgpu 回報錯誤的 frame 不給 copy、下一個照畫；GPU 和 CPU 的 adapter 都過 | `cargo test -p viz --lib drawing_tests::`（`an_error_scope_catches_what_it_is_around`、`an_error_no_scope_is_around_is_handled`、`a_validation_error_panics_in_a_debug_build`、`wgpus_errors_are_handled_by_their_kind`、`a_frame_wgpu_reports_an_error_of_isnt_copied_and_the_next_is_drawn`）；`MACHOPO_GPU_ADAPTER=cpu` 再跑一次 |
| 頂點的 buffer | 比一個 buffer 多的頂點從幾個 buffer 照順序畫（第二個 buffer 的 quad 在最上面） | `drawing_tests::a_frame_of_more_vertices_than_a_buffer_takes_is_drawn_from_several` |
| 實體記憶體 | 有、在合理的範圍、和 `/proc/meminfo` 的 `MemTotal` 一致（Linux）；三個平台都編得過 | `cargo test -p ipc --test it disk_space::`；`cargo check -p ipc --target x86_64-pc-windows-msvc`、`aarch64-apple-darwin` |
| 預算和上限 | 照 Chromium 的公式（1152 MB 對 2056 x 1329、scale 2，照像素放大；最少 512 MB；最多實體記憶體的四分之一，比最少的小時也是）；Viz 的是兩倍、65,536 張、實體記憶體的一半 | `cargo test -p browser --lib gpu_memory::` |
| quad 的上限 | quad 超過上限時丟掉最底下的、沒有 quad 的 state 拿掉，剩下的 Viz 收、每個 quad 的 state 不變；只有 state 超過時也一樣；在上限以內不動；每個測試的 frame 都過檢查（debug build 的 assert） | `cargo test -p compositor --lib` |
| tile 的記憶體 | 可見的先、再照距離（貼著的不算可見）；放不下的放掉，可見的畫成背景色，之後放得下時 raster；一張放不下之後的都放不下；單色的不佔，沒 raster 過的也一樣；commit 時舊的 tile 在新的建立之前放掉 | `cargo test -p compositor --lib`（`the_tiles_nearest_the_viewport_take_the_tile_memory`、`once_a_tile_is_over_the_ones_after_it_are`、`a_tiles_distance_is_across_and_down`、`tiles_of_one_color_take_no_tile_memory`、`a_commit_lets_the_tiles_it_doesnt_keep_go_before_it_rasters`） |
| 預算到 renderer | renderer process 收到 Browser 給的預算（1 byte：一張 tile 都不建，照樣送 frame；給預設的就建） | `cargo test -p browser --test it renderer_process::a_renderer_process_keeps_the_tile_memory_it_is_given` |
| Viz 的上限 | 每個 client 的張數、位元組，所有 client 合計的位元組，各自擋得住；刪掉的、client 不在了的讓出空間；Viz thread 用啟動時給的上限 | `cargo test -p viz --lib`（`shared_images_past_the_gpu_memorys_limits_arent_made`、`thread::the_thread_keeps_the_shared_image_limits_it_is_given`） |
| panic 的 lint | `browser`、`viz`、`net` 和 `protocol` 的 `viz.rs` 在 clippy 下沒有 `unwrap`、`expect`、索引、`panic!`、`unreachable!`，除了帶理由的 `#[expect]` | `cargo clippy -p browser -p viz -p net -p protocol --lib --tests -- -D warnings` |

## 做過的檢查

2026-11-27，上面那台。各段的細節在 git 的歷史裡（每段一個 commit），這裡是濃縮的。

- **最後的驗證**（第 4 段做完時）：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --no-fail-fast`（5023 個通過、0 個失敗、1 個 ignored）、`sanity.py` 的七項通過。各段做完時也跑過一次（第 1 段 5006 個、第 2 段 5012 個、第 3 段 5023 個）。和改動相關的：`cargo test -p viz` 在 llvmpipe 上也跑；`cargo test -p shell`（883 個，debug build 的驗證錯誤 panic，沒有一個觸發）；`python3 scripts/test.py reftest scroll/ effects/ canvas/ frames/ hidpi/ paint/ images/ invalidation/`（0 個非預期的結果）；`cargo check -p ipc --target x86_64-pc-windows-msvc`、`aarch64-apple-darwin`。
- **mutation test**：`mutants-1.toml` 32 個（拿掉 Viz 的檢查之後剩 29 個：已決定的事項第 11 條，它們改的程式不在了）、`mutants-2.toml` 13 個、`mutants-3.toml` 28 個，全部抓到，跑完每個 `from` 都在原處。漏過、補了的：第 3 段第一次漏了五個（放不下之後更小的照放、距離只算一邊（兩個）、沒有像素的 tile 滿了之後也算超過、放得下的 OOM tile 重設：前四個補了測試，最後一個是 S1 之後沒有作用的程式，連同 mutant 拿掉）；第 2 段的「沒有 handler」第一次寫錯、編不過，改了再跑。沒有列成 mutant 的：error scope 告訴呼叫的是最輕的（一個 scope 裡要同時有兩種錯誤，造不出來）、每個 quad 一個 buffer（只是慢）、`VERTICES_PER_BUFFER` 大一點（要超過 256 MiB 才看得出來）；第 4 段（lint 是靜態的，改寫照原本的行為）。只靠審查的記在 `docs/test-gaps.md`：compositor 的 `generate_compositor_frame` 呼叫 `fit_to_limits` 和送出前的 assert、上傳和頂點的 buffer 出錯的兩條路、`BrowserWindow` 把預算和上限接上的幾行、四個範圍的 lint 的那一行。
- **量測**：一個 frame 的檢查（開發的 build，暫時的測試）150 個 quad 4.3 µs、2,000 個 29.9 µs、20,000 個 315 µs、262,144 個 6.6 ms，之後拿掉了 Viz 的那一次（第 11 條）；三層 error scope 約 0.2 µs（第 0 步）；GPU 的記憶體兩次（`details.md` 的「第 3 段的量測」），第一次之後定了第 12、13 條，第二次每一頁都在預算以內。
- **實作時和定的不同的**：shared quad state 的上限是 quad 的上限加 1，不是 quad 數加 1（有 clip 的 layer 的 tile 一個都沒畫時 state 照樣在）；多了一項壞訊息：renderer 的 frame 嵌別的 surface（Viz 以前照收，renderer 能把別的分頁或 Browser 的 UI 嵌進自己的 frame）；`shell` 的整合測試改成 `browser` 的（`machopo` 的 renderer 送不出說不通的 frame）。
- **找到的 bug**：只是貼著可見範圍的 tile 當成可見的，排在真的可見的前面用掉預算（補 mutation 的測試時找到：捲到中間、layer 比視窗寬時，8 張的預算只 raster 了 3 張可見的）；改成先看有沒有重疊。
- **lint 的 80 處**（`browser` 41、`viz` 9、`net` 30，`net` 的 `test_server` 另外 8 處）：多半改寫成不 panic 的（`net` 的解壓縮、`file:` 的磁碟代號、host mapping、sniff、HTTP cache 的淘汰，被 poison 的鎖照拿，Viz 找不到 resource 時不畫、沒有輸出時不畫，commit 找不到送出的導覽時是壞訊息，`about:blank` 的 origin 直接是 opaque 的，session history 的四處用 `get`）；其他寫帶理由的 `#[expect]`：session history 的 `current` 和 `items` 的不變式（逐一讀過所有改它們的地方，renderer 的訊息只經過檢查過的幾條）、記憶體裡的 localStorage 資料庫、作業系統的亂數、unique name 的編號、Viz 的輸出、網路服務的 tokio runtime、`test_server` 整個模組。
- **原本就有的不穩定的測試**（`viz` 的 `a_shared_image_made_by_a_message_is_drawn`）：沒有用重跑蓋過去；這份做完之後回頭查清（使用者決定），2026-11-27 查清、修了（在 `main` 上，這份之後的 commit）：Viz 寫 texture 有兩條路，共用記憶體的像素錄進暫存的 command buffer，訊息裡的像素用 `Queue::write_texture`，而 wgpu 在 submit 時把 queue 自己的寫入排在所有 command buffer 之前；shared image 放掉時 texture 立刻回到池子，暫存的複製可能還沒送出，下一張同大小、像素在訊息裡的 image 拿到同一張 texture，被舊的複製蓋掉。`pixels_in_shared_memory_are_what_is_drawn` 最後正好留下一個這樣的複製；CPU 忙只是改變了兩個測試的先後。修法是所有的寫入都走暫存的那一條，照來的順序；回歸測試是 `pixels_land_in_the_order_they_come`。以下是查的時候的紀錄。這一版 RADV 40 次 2 次、80 次 5 次，llvmpipe 200 次 6 次；RFC 0123 之前的 `1c2c8825` 120 次 3 次，同一個測試、同樣的症狀。查到的：只用一條 thread 時 80 次沒有失敗；跳過這份加的 GPU 測試照樣失敗；GPU 的測試都拿著同一把鎖，不拿的不碰 GPU，沒有測試另外開 thread；照失敗時的順序接著跑前一個測試和它，20 輪沒有失敗。失敗時 image 建出來了、generation 一樣、沒有遺失、`sync_resources` 放進了它的 resource；背景改成黑色時失敗的像素是黑的：quad 畫了，取到的 texture 是全透明的，那次 `queue.write_texture` 的像素沒有在畫之前到；texture 新建的、從池子拿的都會。
- **探測和暫時的程式**：第 0 步的 `probe.rs`、`probe_shell.rs`（留在這裡，不是 build 的一部分）；量測的腳本、檢查的計時、不穩定的測試的紀錄都是暫時的，拿掉了。
