# RFC 0123 的進度：renderer 送什麼都不能讓 Browser 結束

開始日期：2026-11-27。

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
| 4. panic 的 lint；Viz 的 fuzzer；CI | 還沒開始 | |

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
| tile 的記憶體 | 可見的先、再照距離（貼著的不算可見）；放不下的放掉，可見的畫成背景色，之後放得下時 raster；一張放不下之後的都放不下；單色的不佔，沒 raster 過的也一樣；commit 時舊的 tile 在新的建立之前放掉 | `cargo test -p compositor --lib`（`the_tiles_nearest_the_viewport_take_the_tile_memory`、`once_a_tile_is_over_the_ones_after_it_are`、`a_tiles_distance_is_across_and_down`、`tiles_of_one_color_take_no_tile_memory`、`a_commit_lets_the_tiles_it_doesnt_keep_go_before_it_rasters`） |
| 預算到 renderer | renderer process 收到 Browser 給的預算（1 byte：一張 tile 都不建，照樣送 frame；給預設的就建） | `cargo test -p browser --test it renderer_process::a_renderer_process_keeps_the_tile_memory_it_is_given` |
| Viz 的上限 | 每個 client 的張數、位元組，所有 client 合計的位元組，各自擋得住；刪掉的、client 不在了的讓出空間；Viz thread 用啟動時給的上限 | `cargo test -p viz --lib`（`shared_images_past_the_gpu_memorys_limits_arent_made`、`thread::the_thread_keeps_the_shared_image_limits_it_is_given`） |
| compositor | quad 超過上限時丟掉最底下的、沒有 quad 的 state 拿掉，剩下的 Viz 收、每個 quad 的 state 不變；只有 state 超過時也一樣；在上限以內不動；每個測試的 frame 都過檢查（debug build 的 assert） | `cargo test -p compositor --lib` |

## 做過的檢查

### 0（2026-11-27，Fedora 45，Linux 7.3.3，x86-64，AMD Radeon 890M 和 llvmpipe）

探測的結果在 `details.md` 的「第 0 步的探測」；探測的程式是 `probe.rs`、`probe_shell.rs`，跑完從 `viz` 和 `shell` 拿掉了。

### 1a 和 1b（2026-11-27，同一台）

- `cargo test -p protocol --lib viz::`、`cargo test -p viz`、`cargo test -p compositor`、`cargo test -p browser --test it renderer_process::` 通過；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。
- 和決定的表不同的一處：shared quad state 的上限是 `MAX_QUADS` 加 1，不是「quad 數加 1」：renderer 的 compositor 每個有 clip 的 layer 給一個 state，那個 layer 的 tile 一個都沒畫時 state 照樣在（`details.md` 的「renderer 的 compositor 送多少」），照 quad 數會讓誠實的 frame 變成壞訊息。
- 加了決定的表沒有的一項：renderer 的 frame 嵌別的 surface 是壞訊息。讀程式找到的：Viz 以前照收任何 sink 的 `Material::Surface`，renderer 能把別的分頁（或 Browser 的 UI）的畫面嵌進自己的 frame；renderer 的 compositor 從來不嵌（`set_surface_layers` 只有 Browser 的 UI 用）。之後有 OOPIF 時照 Chromium 的 frame sink 的階層（`RegisterFrameSinkHierarchy`）放行。
- RFC 的「測試」寫的 `shell` 的整合測試（假的 renderer 送不合格的 frame，看 Browser 還在、另一個分頁照畫）沒有寫：`machopo` 的執行檔裡的 renderer 送不出說不通的 frame，要假的 renderer 就得在執行檔裡留一條測試用的路。改成 `browser` 的整合測試：假的 renderer process（`renderer_process::child` 的角色）送，看分頁收到壞訊息、process 被結束、Viz 沒收到；Viz 沒收到，其他分頁畫什麼就和它無關。
- **mutation test**（`python3 scripts/test.py mutate rfcs/0123-browser-survives-renderer/mutants-1.toml`）：32 個全部抓到。`protocol` 的檢查每一條拿掉、每個上限多一個和少一個、quad 只數 root 的、嵌 surface 一律放行、shared image 的大小和像素不檢查、送來的 frame 不檢查（`protocol` 的 `viz::` 測試）；IO thread 不檢查、把 renderer 當成能嵌 surface、壞訊息不告訴分頁、壞訊息照樣送給 Viz（`renderer_process::what_viz_is_sent_that_cant_be_right_ends_its_renderer`）；Viz 不檢查、任何 sink 都能嵌、root 也不能嵌（`viz` 的 `display::`）；compositor 不守上限、只看 quad、不丟 quad、quad 的 state 沒有跟著換號、留著所有的 state（`a_frame_keeps_within_what_viz_takes`）。跑完 32 個 `from` 都在原處。
- 沒有放進 mutation test 的兩處，只靠審查（`docs/test-gaps.md`）：compositor 的 `generate_compositor_frame` 呼叫 `fit_to_limits`（測試沒有超過 262,144 個 quad 的頁面：一個可見的 tile 一個 quad，要 26 萬個 tile）；送出前的 debug build 的 assert（誠實的 compositor 不會觸發，拿掉它沒有測試會失敗）。
- 第 1 段做完時：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`（新的測試的 `single_match` 改掉之後）、`cargo test --workspace --no-fail-fast`（5006 個通過、0 個失敗、1 個 ignored）通過。

### 2a 和 2b（2026-11-27，同一台）

- `cargo test -p viz`（56 個；`MACHOPO_GPU_ADAPTER=cpu` 也跑）、`cargo test -p shell`（883 個：debug build 的驗證錯誤會 panic，沒有一個觸發）、`python3 scripts/test.py reftest scroll/ effects/ canvas/ frames/ hidpi/ paint/ images/`（0 個非預期的結果）通過。
- 讀 wgpu 30 的 native 後端確認：`ErrorScopeGuard::pop` 的 future 當場就好（第 0 步的探測也是），所以 Viz 在 thread 上 `block_on` 不等；scope 是每條 thread 一份，shared image 在 renderer 的 thread 上傳時，scope 也在那條 thread 上。
- **mutation test**（`python3 scripts/test.py mutate rfcs/0123-browser-survives-renderer/mutants-2.toml`）：13 個全部抓到（`drawing_tests::`）：scope 接到的不告訴呼叫的、驗證錯誤在 debug build 不 panic、容許時也 panic、記憶體不夠不放掉池子、池子放不掉、內部錯誤不當成遺失、內部錯誤當成記憶體不夠、沒有 uncaptured error 的 handler、有錯誤的 frame 照樣給 copy、frame 不在 scope 裡、第二個 buffer 不 bind、每個 quad 都從第一個 buffer 畫、buffer 沒寫進頂點。「沒有 handler」的 mutant 第一次寫錯、編不過（括號），改成換掉整個敘述後重跑，抓到。跑完 13 個 `from` 都在原處。
- 沒有列成 mutant 的：scope 告訴呼叫的是最輕的而不是最重的（`max` 換成 `min`）：一個 scope 裡同時有兩種錯誤才分得出來，測試造不出記憶體不夠和內部錯誤；每個 quad 一個 buffer（分 buffer 的條件永遠成立）：畫出來一樣，只是慢，不算錯；`VERTICES_PER_BUFFER` 大一點：一個 buffer 要超過 256 MiB 才看得出來，Browser 的 UI 和一個分頁的 frame 合起來最多 240 MiB。上傳和頂點的 buffer 出錯的那兩條只靠審查（`docs/test-gaps.md`）。
- 第 2 段做完時：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --no-fail-fast`（5012 個通過、0 個失敗、1 個 ignored）、`sanity.py` 的七項通過。

### 第 1 段之後的改動（2026-11-27，第 3 段之中）

- **Viz 不再檢查 frame**（已決定的事項第 11 條）：一個 frame 的檢查量過（開發的 build，`opt-level = 1`；暫時的測試，量完拿掉）：150 個 quad 4.3 µs、2,000 個 29.9 µs、20,000 個 315 µs、262,144 個 6.6 ms（一個 15 到 28 ns），renderer process 的每個 frame 原本在 IO thread 和 Viz 各跑一次。拿掉了 Viz 的那一次，和守它的兩個測試（`a_frame_that_cant_be_right_is_dropped`、`a_frame_of_bad_resources_is_dropped`）、`mutants-1.toml` 的三個 mutant（「Viz takes any sink's surfaces」「Viz takes the root's no surfaces」「Viz doesn't check」：它們改的程式不在了）；加了 `what_cant_be_right_is_drawn_as_far_as_it_can_be`（Viz 收到說不通的 frame 畫得下去，debug build 裡也沒有 wgpu 的錯誤），`a_shared_image_nobody_made_isnt_drawn` 改回帶著像素也不畫。

### 3a 到 3c（2026-11-27，同一台）

- **量測**：`details.md` 的「第 3 段的量測」。第一次之後定了第 12、13 條；第二次每一頁都在預算以內，Viz 的上限照暫定的。
- **補 mutation 的測試時找到的 bug**：只是貼著可見範圍的 tile（距離 0）也當成可見的，照位置排在真的可見的前面，用掉了預算（捲到中間、layer 比視窗寬時，8 張的預算只 raster 了 3 張可見的；`the_tiles_nearest_the_viewport_take_the_tile_memory` 的第四段）。改成先看有沒有重疊、再照距離。第一次的量測是修正前的 binary。
- **mutation test**（`python3 scripts/test.py mutate rfcs/0123-browser-survives-renderer/mutants-3.toml`）：第一次 29 個抓到 24 個，漏的五個：放不下之後更小的照放（測試的 tile 一樣大：補了 `once_a_tile_is_over_the_ones_after_it_are`，邊上半寬的 tile）；距離只算縱向、只算橫向（預算剛好是可見的那幾張，距離只排可見以外的：補了 `a_tiles_distance_is_across_and_down`）；沒有像素的 tile 在滿了之後也算超過（那一段用同一個 host，單色的已經判定過、不在這一次裡：改成新的 host）；放得下的 OOM tile 重設成沒 raster（S1 之後它照計畫 raster，重設沒有作用：拿掉了那段和它的 mutant）。補完再跑那四個，都抓到：28 個全部抓到。跑完 28 個 `from` 都在原處。spec 第一次因為一個 `from` 對不上（S1 改了那一行）沒有開始跑，改了再跑。
- 只靠審查的（`docs/test-gaps.md`）：`BrowserWindow` 把預算和上限接到 renderer、Viz 的那幾行，和視窗模式用的螢幕。
- 第 3 段做完時：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --no-fail-fast`（5023 個通過、0 個失敗、1 個 ignored）、`sanity.py` 的七項、`python3 scripts/test.py reftest scroll/ effects/ canvas/ frames/ hidpi/ paint/ images/ invalidation/`（0 個非預期的結果）、`cargo check -p ipc --target x86_64-pc-windows-msvc` 和 `aarch64-apple-darwin` 通過。macOS 和 Windows 上沒有跑 `amount_of_physical_memory` 的測試。
