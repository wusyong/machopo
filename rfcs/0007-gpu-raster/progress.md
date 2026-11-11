# RFC 0007 的進度：用 vello_cpu 取代 Skia

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-10，Windows 11。在 `0007` 上做。

完成日期：2026-11-11，Windows 11（AMD Radeon RX 7900 XTX）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0 圖片的解碼和讀尺寸、JPEG 的編碼換成純 Rust 的 crate（Skia 照舊畫） | 完成 | 2026-11-10 |
| 1a 原型：常用的 op；量文字多的 tile、存截圖（第一個檢查點） | 完成：檢查點通過（不 merge） | 2026-11-10 |
| 1b 原型：全部的 op 和組出來的功能；整套 reftest（第二個檢查點） | 完成：檢查點通過（不 merge） | 2026-11-10 |
| 2a Vello 成為唯一的 playback；`raster` 的測試搬過來、補上「上游還缺」、不會 panic、tile 接縫的測試 | 完成 | 2026-11-11 |
| 2b ink bounds 改用 skrifa、mipmap、拿掉 skia-safe | 完成 | 2026-11-11 |
| 2c playback 裡的 panic：release 接住、留白、記 log 和 DevTools Console 的警告 | 完成 | 2026-11-11 |
| 2d WPT 的預期結果、列出缺的功能的文件、其他文件、整個階段的檢查 | 完成；第 0 到 2 步 2026-11-11 merge 回 `main` | 2026-11-11 |
| 3a 文字的速度：Vello 的 glyph atlas（fork 上的 patch） | 做了、量了、不採用：開 atlas 之後 WPT 的 reftest 不穩定（下面「量測」；改動留在 `atlas/`） | 2026-11-11 |
| 3b 量 release 只對 Vello 開整數溢位檢查的代價 | 量了（下面「量測」），不開 | 2026-11-11 |

做完時 fork 的狀態：`wusyong/vello` 的 `machopo` 是上游 `main` 的 `112736df`（2026-11-10）加上 `README.machopo.md` 的 commit（`b52311f2`），沒有改 Vello 的 patch；push 了，和 `third_party.toml` 一致。回報上游的六份草稿（`upstream-reports.md`）都沒有送出。

## 驗收

測試的名字是做完時的樣子；沒寫 crate 的都在 `raster`（`cargo test -p raster <名字>`）。

**圖片的解碼和編碼**

| 項目 | 怎麼驗證 |
| --- | --- |
| 每種格式（PNG、JPEG、GIF、BMP、ICO）讀得到尺寸、解得出像素、畫得出來 | `image_decode::tests::every_format_is_measured_and_decoded`、`an_ico_is_its_best_entry`，`playback_tests::draws_each_format` |
| EXIF 的方向，尺寸和像素都轉 | `image_decode::tests::a_jpegs_orientation_turns_its_size_and_pixels`（8 種方向）、`playback_tests::draws_a_jpeg_turned_as_its_orientation_says`；情境 `python3 scripts/test.py scenarios image-orientation` |
| 壞掉的、截斷的、宣稱超大尺寸的檔案不是圖片，不 panic | `image_decode::tests::broken_files_are_no_image`、`a_header_claiming_too_many_pixels_is_no_image` |
| 一張圖只解一次，在第一次畫到時；資料沒了項目跟著清 | `playback_tests::an_image_is_decoded_once`、`threads_share_a_cache`（8 條 thread）、`a_decoded_image_is_dropped_with_its_data` |
| 轉成不是 premultiplied 的結果和 Skia 逐位相同（Skia 拿掉之後比存下來的 hash） | `encode::tests::unpremultiplying_is_skias_for_every_color_and_alpha`、`unpremultiplying_rounds_as_skia_does` |
| JPEG 的編碼照 Blink 的設定 | `encode::tests::a_jpeg_is_blended_on_black`、`a_jpegs_quality`、`a_jpegs_tables_are_libjpegs`、`nothing_for_no_pixels` |
| 縮到一半以下的圖從預先縮好的那一層畫，層照 cc 的規則取 | `vello_backend::tests::an_image_is_drawn_from_the_smallest_level_that_is_large_enough`、`a_smaller_level_averages_what_each_pixel_covers`、`a_level_is_made_once_and_only_when_drawn_from`，`playback_tests::an_image_made_much_smaller_is_its_pixels_averaged`、`a_part_and_a_pattern_of_an_image_made_smaller_are_from_its_level` |

**playback**

| 項目 | 怎麼驗證 |
| --- | --- |
| workspace 裡沒有 Skia | `Cargo.toml` 和 `Cargo.lock` 沒有 `skia-safe`；`cargo check --workspace --all-targets` |
| display list 能放的東西每一種都畫得出來，不 panic | `playback_tests::one_of_everything_is_drawn`（每種 op、每種 filter 和巢狀的 chain、27 種 blend mode 各用在 draw、layer 和 `DrawColor`、每種 shader；三個 tile 和 canvas） |
| 原本對著 Skia 寫的 52 個 playback 測試照原樣的預期值通過 | `cargo test -p raster playback_tests::` |
| tile 和整塊畫出來的一樣（scale 1 的形狀、文字和陰影逐位相同，其餘最多差 1），filter、陰影和文字在 tile 的邊緣不斷開 | `playback_tests::tiles_have_the_pixels_of_the_whole`（96×96 對九個 32×32，scale 1 和 1.5） |
| 上游還缺的和上游的 bug，上游補上時測試會失敗 | `vello_backend::tests::upstream_lacks_color_matrix_filters`、`upstream_lacks_filters_of_several_primitives`、`upstream_pads_a_gradient_past_a_hard_stop_with_the_color_before_it`、`upstream_panics_on_a_clipped_stroke_at_the_right_edge`，`playback_tests::upstream_doesnt_keep_a_clearing_layer_to_its_clip` |
| 為上游的 bug 加的：在 tile 外面的 clip 不交給 Vello、藏起來時不推 layer；結尾硬切的漸層；canvas 的 blend mode 看得到 bitmap 原本的像素 | `playback_tests::a_clip_outside_hides_what_is_in_it`、`a_clearing_layer_in_a_clip_outside_changes_nothing`、`past_a_hard_stop_at_its_end_a_gradient_is_the_last_color`、`on_a_canvas_a_blend_mode_takes_from_what_the_bitmap_has`；`python3 scripts/test.py wpt html/canvas/element/compositing` |
| `gfx::Path` 的形狀和 Skia 逐位相同（拿掉 Skia 之後比存下來的結果） | `cargo test -p gfx shapes_are_skias_bit_for_bit`（108 個情況） |

**文字**

| 項目 | 怎麼驗證 |
| --- | --- |
| glyph 對齊到四分之一個 pixel；原點在 tile 外面的 glyph 照樣畫 | `playback_tests::glyphs_are_drawn_at_quarters_of_a_pixel`、`a_glyph_that_begins_outside_is_drawn` |
| ink bounds 是外框線控制點的範圍，不取整；假粗體、斜體、variation、collection 的 face、點陣的 glyph 都照畫的算 | `cargo test -p raster ink::`（7 個）、`playback_tests::ink_bounds_are_where_glyphs_are_drawn`、`a_bitmap_glyphs_ink_is_where_it_is_drawn`；`python3 scripts/test.py wpt html/canvas/element/text` |

**playback 的 panic**

| 項目 | 怎麼驗證 |
| --- | --- |
| 接住的 playback：tile 是底色、那條 thread 的 context 丟掉、下一次照常畫；開著 debug assertions 時不接 | `playback_tests::a_playback_that_panics_leaves_the_tile_its_clear_color`、`a_panic_of_vellos_is_caught`（真的 Vello panic）、`with_debug_assertions_a_playback_that_panics_panics` |
| 失敗只在第一次和次數翻倍時說；panic 的訊息不管是哪一種都拿得到 | `playback_tests::failures_are_told_when_their_count_doubles`、`tests::a_panics_message_is_told_whatever_it_is` |
| canvas 留著 panic 之前的像素、不透明的仍然不透明；canvas raster thread 接著畫後面的 | `playback_tests::a_canvas_keeps_what_it_had_before_a_panic`、`cargo test -p renderer a_drawing_that_fails_is_left_out_and_the_next_is_drawn` |
| tile 失敗時 compositor 告訴 main thread；Console 的那一筆是 `rendering` 的 `warning`、帶網址 | `cargo test -p compositor the_main_thread_is_told_of_tiles_whose_raster_failed`、`cargo test -p renderer a_raster_failure_is_a_rendering_warning` |

**整體**

| 項目 | 怎麼驗證 |
| --- | --- |
| Vello 造成的 WPT 差異都有預期結果（8 個 `fuzzy`、1 個失敗，清單在下面） | `python3 scripts/test.py wpt` 加上那 9 個測試的路徑 |
| reftest、情境、視窗裡的情境和整個 workspace | `python3 scripts/test.py reftest`（195 個）、`scenarios`、`automation`；`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`、`sanity.py` 七項 |

## 做過的檢查

都在 Windows 11（AMD Radeon RX 7900 XTX）上，2026-11-10 到 11-11。每一步當時的完整紀錄在這個檔案的 git 歷史裡（`a90c6c0` 是第 0 到 2 步，`4dfcb35` 加上第 3 步）。

### 最後的驗證

- 第 0 到 2 步 merge 之前（2026-11-11，rebase 到 `origin/main` 之後）：`cargo test --workspace --no-fail-fast` 3828 個通過、0 個失敗；`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`、`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs 都過。rebase 之前：`test.py reftest` 195 個都和預期一樣，`test.py scenarios` 78 次執行 0 個失敗，`test.py automation` 61 份情境和 `pipe --window` 都過（看過 `canvas/paths.png` 和 `backgrounds/gradient.png`）。rebase 之後 reftest 和原有的情境沒有重跑（`main` 新來的改動沒有碰 paint 和 raster），新來的 5 份情境跑了、都過。
- 第 3 步沒有留下程式的改動（atlas 還原了，溢位檢查只量）：還原之後 `cargo test -p raster` 100 個通過，`ci.py third-party` 確認 fork 和 `third_party.toml` 對得上，`sanity.py docs` 過。整個 workspace 沒有重跑。
- 沒有跑的：整套 `wpt`（第二個檢查點用原型跑過一次，之後只跑相關的目錄：2a 到 2d 之間別的測試變了的話要等 CI 才看得到）、`wdspec`、`matrix`、`fuzz`；macOS 和 Linux。Speedometer 只在第 0 步跑過（使用者要求）：20 個 suite 的幾何平均 +0.5%，在雜訊裡。

### 用 Vello 跑整套測試的結果

第二個檢查點時用原型跑的，加上之後各步改掉的。

- machopo 的 reftest 195 個、情境 78 次都通過，沒有改預期結果。
- `raster` 對著 Skia 寫的 75 個單元測試有 71 個照原樣通過；`cargo test --workspace` 裡因為 Vello 改預期值的只有兩個（都是 8-bit 混色差 1，script 用 `getImageData` 讀得到）：`renderer` 的 `a_flush_sends_the_recording_to_the_thread_and_forgets_it`（192 改成 191）、`browser` 的 `navigation::links::canvas_2d`（漸層的兩個 pixel）。
- 整套 WPT（13,917 個測試）：不對齊 glyph 時 Vello 造成 39 個不通過（34 個 reftest，31 個最大差 1），glyph 對齊到四分之一個 pixel 之後 13 個（8 個 reftest、5 個 canvas）；修掉 machopo 自己的錯（下面）之後剩 9 個，都有預期結果：

| 測試 | 預期 | 量到的差異 |
| --- | --- | --- |
| `html/canvas/element/line-styles/2d.line.cross.html` | 失敗 | kurbo 的 stroker 在接合內側多畫一塊 |
| `css/css-tables/tentative/paint/background-image-column.html` | `fuzzy` 1、140 | 8-bit 混色的捨入 |
| `css/css-tables/tentative/paint/background-image-column-collapsed.html` | `fuzzy` 1、588 | 同上 |
| `css/css-tables/tentative/paint/background-image-row-collapsed.html` | `fuzzy` 1、236 | 同上 |
| `css/css-backgrounds/box-shadow-041.html` | `fuzzy` 1、304 | 同上 |
| `css/css-backgrounds/box-shadow-042.html` | `fuzzy` 1、358 | 同上 |
| `css/css-masking/clip-path/animations/clip-path-animation-custom-timing-function.html` | `fuzzy` 3、628 | clip 的邊落在 pixel 之間 |
| `css/css-contain/contain-paint-clip-006.html` | `fuzzy` 1、4 | 文字的邊緣 |
| `css/css-scroll-anchoring/nested-overflow-subtree-layout.html` | `fuzzy` 1、1 | 文字的邊緣 |

- 第 0 步（解碼換掉、Skia 照舊畫）沒有改任何預期結果、沒有加 fuzzy；WPT 的 `natural-size-orientation.html` 從預期失敗變成全部通過。

### 找到的 bug

**上游的**（都在 machopo 這一側避開或沒有用到；`docs/render-gaps.md` 各有一列，草稿在 `upstream-reports.md`）：

- `vello_common` 的 clip：平頭的 stroke 整條在 context 右邊緣之外、剛好從邊緣開始時，交集減出負數（有溢位檢查時 panic）。邊框貼著 tile 邊界時隔壁的 tile 會收到。避開：`Player::shows`。
- 漸層最後兩個色標都在 offset 1 時，超過終點的地方用錯顏色。避開：拉長一倍。
- kurbo 的 stroker 在線寬遠大於線段長度時 bevel 接合的內側多畫一塊（`2d.line.cross`）。沒有繞法。
- 會清掉形狀以外的 blend mode 不受落在 context 裡的 clip 限制。沒有補。
- glyph atlas（沒有採用）：斜的或 matrix 不等比例的 run 拿到同一個 scene 裡剛放進去的直的 glyph；glyph 的像素跟著它在 atlas 的位置變；預設 4096 的頁面在一個 tile render 一次時每次要多 1 ms。

**machopo 自己的**：

- canvas 的四個 `2d.composite.clip.*` 一開始以為是 Vello 的缺口：`shows` 藏起 clip 時 `SaveLayer` 還是推了一層，疊回去時清掉整張 canvas。改成不推（已決定的事項第 41 條）。
- 換之前的 bug，第 0 步改掉：JPEG 有 EXIF 方向時尺寸沒轉、圖轉了。
- 原型期間修掉的（圓角矩形和橢圓用了兩種曲線、canvas 上 `copy` 和 `ClearRect` 沒有作用、圖片的來源矩形取樣到外面、長度 0 的 dash 配圓頭、shader 沒帶 alpha、conic 切得太粗、合成斜體的方向反了）都有測試或 reftest 守著。
- skrifa 把縮放過的外框線對齊到 1/64 px：ink bounds 改成取字型單位的外框線、自己縮放，和 Skia 的數字相同。

### 量測

每個 tile 的 `PrepareTiles`（`bench.py raster`，headless、scale 2、一個 raster thread、`perf` profile；各組輪流跑）。

**Vello 對 Skia**（原型，同一個執行檔輪流各 3 次，中位數的比；三輪量測的範圍）：

| 頁面 | 階段 | Vello / Skia |
| --- | --- | --- |
| Gutenberg | 捲動 | 1.04 到 1.12（這一格兩邊都不穩） |
| Gutenberg | 改視窗大小 | 0.97 到 1.06 |
| SQLite | 改視窗大小 | 1.62 到 1.71 |
| NewsSite-Next | 捲動 | 0.84 到 0.88 |
| NewsSite-Next | 改視窗大小 | 0.58 到 0.66 |

- 這是濾掉離 tile 遠的 glyph 之後的數字；不濾時文字多的頁面慢 2.6 到 5 倍（一段文字是一個 op，碰到的每個 tile 都收到整段）。
- 時間幾乎都在文字（SQLite 每個 tile 127 µs 裡 `DrawGlyphs` 佔 104 µs），在 Vello 把 path 攤平、產生 strip；machopo 自己快取輪廓沒有用。
- Vello 的 f32 管線只讓 5 個差 1 的 WPT 通過，每一頁都是 Skia 的 1.2 到 2.7 倍：不用。
- 文字的外觀（`text-compare.html`，放大對照看過）：深底的淺色字 Vello 比較細，淺底的深色字 Vello 比較粗；1x 有 13% 的 pixel 不同、平均差 31，都在 glyph 的邊緣。
- 沒有量的：離屏的 filter 和四個角半徑不同的陰影的成本（那三頁沒有用到）；多個 raster thread；macOS。

**glyph atlas**（第 3a 步，做完量過、不採用；設定是「頁面邊長、頁數上限、保留幾次 render」，每組都是 16 MB；ms，三次由小到大）：

| 頁面 | 階段 | 不開 | 256、64、4096 | 512、16、4096 | 1024、4、4096 | 512、16、64 |
| --- | --- | --- | --- | --- | --- | --- |
| Gutenberg | 捲動 | 0.713、0.722、0.728 | 0.593、0.701、0.718 | 0.697、0.699、0.708 | 0.785、0.801、0.803 | 0.707、0.713、0.783 |
| Gutenberg | 改視窗大小 | 0.144、0.154、0.158 | 0.111、0.123、0.126 | 0.120、0.123、0.129 | 0.118、0.130、0.135 | 0.141、0.151、0.156 |
| SQLite | 改視窗大小 | 0.147、0.155、0.160 | 0.111、0.118、0.121 | 0.118、0.119、0.123 | 0.114、0.125、0.126 | 0.117、0.126、0.132 |
| NewsSite-Next | 捲動 | 0.282、0.301、0.302 | 0.272、0.278、0.291 | 0.282、0.296、0.297 | 0.298、0.300、0.312 | 0.291、0.293、0.314 |
| NewsSite-Next | 改視窗大小 | 0.106、0.118、0.119 | 0.101、0.102、0.110 | 0.094、0.100、0.110 | 0.097、0.104、0.110 | 0.114、0.122、0.130 |

- 256、64 頁、4096 次最好：對不開的比是 0.97、0.80、0.76、0.92、0.86。頁面 1024 捲動時比不開還慢；保留 64 次（Vello 的預設）和不開差不多。
- **不採用的原因**：開了之後 `css/css-transforms`（548 個測試）每次有 7 到 29 個意外的失敗，每次不同的幾個，不開時 0 個。測試頁和參考頁同一行文字，某個 glyph 邊緣的一欄 pixel 差 1：同一個 glyph 畫進 atlas 的不同位置結果可以差 1（只用 Vello 重現過：頁面 4096、每次用新的字型 blob，200 次裡 57 次不同），而兩頁各自的 raster thread 有各自的 atlas。試過沒用的：讓 atlas 用字型原本的 id、測試字型的 blob 整個 process 共用、加大 padding。
- 另外查到：斜的、轉過或拉伸的 run 不會放進 atlas，但會拿到同一個 scene 裡剛放進去的直的 glyph（canvas 的 `fillText` 配 `maxWidth`，machopo 的 reftest `canvas/c5-text` 抓到）；實作裡這種 run 不開 atlas。
- 改動、測試和 mutation 的清單在 `atlas/`（fork 的 patch 套在 Vello 的 `112736df` 上，machopo 的套在 `a90c6c0` 上；9 條 mutation 抓到 7 條，沒抓到的兩條只影響速度）。

**release 只對 Vello 開整數溢位的檢查**（第 3b 步，只量、不開；用 cargo 的 `--config profile.perf.package.<crate>.overflow-checks=true`，三個執行檔輪流各 5 次；ms，中位數）：

| 頁面 | 階段 | 不開 | `vello_cpu`、`vello_common`、`glifo` | 再加 kurbo、peniko、color、fearless_simd、skrifa、read-fonts |
| --- | --- | --- | --- | --- |
| Gutenberg | 捲動 | 0.667 | 0.729 | 0.733 |
| Gutenberg | 改視窗大小 | 0.150 | 0.156 | 0.150 |
| SQLite | 改視窗大小 | 0.158 | 0.157 | 0.161 |
| NewsSite-Next | 捲動 | 0.265 | 0.251 | 0.254 |
| NewsSite-Next | 改視窗大小 | 0.110 | 0.117 | 0.111 |

- Gutenberg 的捲動慢約 9%，其他四格在雜訊裡（-5% 到 +6%，範圍重疊）。這 9% 和換一個 build 的差別一樣大（第 3a 步「不開 atlas」的那個 build 這一格是 0.713 到 0.728），分不出是不是檢查造成的。

### Mutation test

`python3 scripts/test.py mutate rfcs/0007-gpu-raster/mutants-<步>.toml`，每份跑完都確認檔案還原了。

| 清單 | 條數 | 結果 |
| --- | --- | --- |
| `mutants-0.toml`（解碼、方向、快取、premultiply、JPEG 的編碼） | 19 | 都抓到。第一輪沒抓到的 2 條是多餘的程式（空圖片的檢查、clamp），拿掉了 |
| `mutants-2a.toml`（tile 四周的範圍、glyph 的對齊和 culling、藏起來的 clip、canvas 的底、漸層、filter 的 chain） | 13 | 都抓到 |
| `mutants-2b.toml`（ink bounds、mipmap 的取層和平均、`gfx::Path` 的形狀、premultiply） | 22 | 21 條抓到。第一輪有 2 條是測試寫得不好（剛好縮成四分之一時 bilinear 本來就是平均），測試改掉。沒抓到的 1 條：點陣 glyph 的外側 bearing 的正負號（測試字型那個值是 0），記在 `docs/test-gaps.md` |
| `mutants-2c.toml`（接住之後的 tile 和 context、次數和節流、訊息、接不接、canvas 的 alpha、告訴 main thread、Console 的那一筆） | 17 | 都抓到。第一輪沒抓到的 1 條是多餘的程式（commit 之後另外送一次），拿掉了 |
| `mutants-2d.toml`（clip 藏起來時照樣推 layer） | 1 | 抓到 |
| `atlas/mutants.toml`（對套了 `atlas/machopo-raster.patch` 的程式） | 9 | 7 條抓到；保留時間改回 64、離屏的 layer 也開 atlas 只影響速度 |

沒有 mutation、只靠審查的（都記在 `docs/test-gaps.md`）：解碼的 `catch_unwind` 和 `Limits` 的 `max_alloc`；`RenderThread` 裡把 raster 的失敗接到 Console 的兩小段；render 途中 panic 時的 bitmap；沒有外框線的 COLR glyph 的 ink bounds；離 tile 遠的 glyph 有沒有真的被濾掉（沒有計數）。

### 工具和不穩定的東西

- `test.py mutate` 在這台機器上寫檔失敗過三次（`OSError: [Errno 22]`，第 0 步）：工具當場停下，用 `--restore` 放回去、`--only` 跑剩下的。原因沒查。
- `test.py wpt` 一次給很多目錄時，Windows 上命令列太長（`WinError 206`）：一個目錄跑一次。
- 整套 WPT 一起跑時，計時類的測試（`navigation-timing/`、`scroll-behavior-smooth-navigation`、`File-lastModified`）和幾個圖片載入的測試每一輪結果不同，單獨跑通過：和這份 RFC 無關。有一輪 `test.py wpt` 的 Python process 最後回報 `Segmentation fault`（結果已經寫完），原因沒查。
- `shell` 的 `devtools_initiators::script_asks_with_its_stack` 在整個 workspace 一起跑時失敗過一次，單獨跑通過，沒查。
- 只跑一次的 `bench.py raster` 不能拿來下結論：同一格前後兩次可以差到兩三倍（Gutenberg 捲動的 Skia 0.381 對 0.56 到 0.62）；上面的數字都是輪流跑 3 次以上。

### 留著沒處理的

- **Rust 預設的 panic hook**：接住的 playback panic，hook 每次都印一行到 stderr，不受「翻倍才記」的節流。要擋得換掉整個 process 的 hook。
- **兩處文件要在 macOS 上看過才知道**：`docs/limitations.md` 的「Compositor、layer 和 input」寫著幾個 reftest 只在 macOS 上預期失敗（原因是 Skia 在 macOS 上的捨入），換成 Vello 之後可能變了；`docs/testing.md` 說 macOS 上 `target/debug/deps` 堆大之後畫文字的測試變慢，那是用 Skia 時量的。
- **不屬於這份 RFC、順便看到的**：WPT 的 `css/css-images/image-orientation/` 不在清單裡（單獨跑：reftest 21 個通過、10 個失敗、2 個逾時）；`html/rendering/widgets/baseline-alignment-and-overflow.tentative.html` 和 `button-layout/display-other.html` 的預期結果過時了（換之前就通過）；`css-transforms/scale/svg-scale-016.html` 換成 Vello 之後從預期失敗變成通過，它的 `.ini` 還沒拿掉。
- `image` 的 ICO 選哪一張、只收 RGBA 的 PNG，和 Blink、Skia 不同：記在 `docs/limitations.md` 的「圖片」。
