# RFC 0007 的進度：用 vello_cpu 取代 Skia

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-10，Windows 11。在 `0007` 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0 圖片的解碼和讀尺寸、JPEG 的編碼換成純 Rust 的 crate（Skia 照舊畫） | 完成，還沒 merge | 2026-11-10 |
| 1a 原型：常用的 op；量文字多的 tile、存截圖（第一個檢查點） | 完成：檢查點通過（不 merge） | 2026-11-10 |
| 1b 原型：全部的 op 和組出來的功能；整套 reftest（第二個檢查點） | 完成：檢查點通過（不 merge） | 2026-11-10 |
| 2a Vello 成為唯一的 playback；`raster` 的測試搬過來、補上「上游還缺」、不會 panic、tile 接縫的測試 | 完成 | 2026-11-11 |
| 2b ink bounds 改用 skrifa、mipmap、拿掉 skia-safe | 完成 | 2026-11-11 |
| 2c playback 裡的 panic：release 接住、留白、記 log 和 DevTools Console 的警告 | 還沒開始 | |
| 2d WPT 的預期結果、列出缺的功能的文件、其他文件、整個階段的檢查 | 還沒開始 | |
| 3 改進（文字的 gamma 和對比、漸層的 dithering、fork 裡的 `ColorMatrix`；可能不需要） | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 第 0 步：每種格式（PNG、JPEG、GIF、BMP、ICO）讀得到尺寸、解得出像素 | `cargo test -p raster image_decode::`（`every_format_is_measured_and_decoded`、`an_ico_is_its_best_entry`）、`skia_backend::tests::draws_each_format` |
| 第 0 步：EXIF 的方向，尺寸和像素都轉 | `image_decode::tests::a_jpegs_orientation_turns_its_size_and_pixels`（8 種方向）、`skia_backend::tests::draws_a_jpeg_turned_as_its_orientation_says` |
| 第 0 步：壞掉的、截斷的、宣稱超大尺寸的檔案不是圖片，不 panic | `image_decode::tests::broken_files_are_no_image`、`a_header_claiming_too_many_pixels_is_no_image` |
| 第 0 步：一張圖只解一次，在第一次畫到時 | `skia_backend::tests::an_image_is_decoded_once`、`threads_share_a_cache`（8 條 thread，計數是 1） |
| 第 0 步：轉成不是 premultiplied 的結果和 Skia 逐位相同 | `encode::tests::unpremultiplying_is_skias_for_every_color_and_alpha`（每種顏色和 alpha 的組合，和 Skia 的 `readPixels` 比；Skia 拿掉時這個測試跟著拿掉，`unpremultiplying_rounds_as_skia_does` 的固定數值留著） |
| 第 0 步：在視窗裡，橫著存的照片照 EXIF 的方向顯示，頁面讀到轉過的尺寸 | `python3 scripts/test.py scenarios image-orientation`（`about:demo` 的「A photo held sideways」，等到頁面寫出「8 by 16」，截圖在 `target/scenario-results/image-orientation/`） |
| 第 0 步：JPEG 的編碼照 Blink 的設定 | `encode::tests::a_jpeg_is_blended_on_black`、`a_jpegs_quality`（取樣）、`a_jpegs_tables_are_libjpegs`（量化表）、`nothing_for_no_pixels`（邊長的上限） |
| 第 2a 步：display list 能放的東西每一種都畫得出來，不 panic | `playback_tests::one_of_everything_is_drawn`（每種 op、每種 filter 和巢狀的 chain、27 種 blend mode 各用在 draw、layer 和 `DrawColor`、每種 shader；三個 tile 和 canvas） |
| 第 2a 步：tile 和整塊畫出來的一樣，filter、陰影和文字在 tile 的邊緣不斷開 | `playback_tests::tiles_have_the_pixels_of_the_whole`（96×96 對九個 32×32，scale 1 和 1.5） |
| 第 2a 步：上游還缺的和上游的 bug，上游補上時測試會失敗 | `vello_backend::tests::upstream_lacks_color_matrix_filters`、`upstream_lacks_filters_of_several_primitives`、`upstream_pads_a_gradient_past_a_hard_stop_with_the_color_before_it`、`upstream_panics_on_a_clipped_stroke_at_the_right_edge` |
| 第 2b 步：workspace 裡沒有 Skia | `Cargo.toml` 和 `Cargo.lock` 沒有 `skia-safe`；`cargo check --workspace --all-targets` 通過 |
| 第 2b 步：ink bounds 是外框線控制點的範圍，不取整；假粗體、斜體、variation、collection 的 face 都照畫的算 | `cargo test -p raster ink::`（7 個）、`playback_tests::ink_bounds_are_where_glyphs_are_drawn`、`a_bitmap_glyphs_ink_is_where_it_is_drawn`（對著畫出來的像素）；WPT 的 `html/canvas/element/text` |
| 第 2b 步：縮到一半以下的圖從預先縮好的那一層畫，層照 cc 的規則取 | `vello_backend::tests::an_image_is_drawn_from_the_smallest_level_that_is_large_enough`、`a_smaller_level_averages_what_each_pixel_covers`、`a_level_is_made_once_and_only_when_drawn_from`，`playback_tests::an_image_made_much_smaller_is_its_pixels_averaged`、`a_part_and_a_pattern_of_an_image_made_smaller_are_from_its_level` |
| 第 2b 步：`gfx::Path` 的形狀和 Skia 逐位相同（拿掉 Skia 之後用存下來的結果比） | `cargo test -p gfx shapes_are_skias_bit_for_bit`（108 個情況） |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 第 0 步：圖片的解碼和讀尺寸、JPEG 的編碼（2026-11-10，Windows 11，AMD Radeon RX 7900 XTX）

**改之前量的現況**（對著 Skia 寫的暫時的測試，量完拿掉）：

- BMP 和 ICO 解得開（2×2 的 BMP、包著一張 PNG 的 ICO，`image_size` 和 Skia 的圖片都有尺寸）。
- 40×30、EXIF 方向是 6 的 JPEG：`image_size` 是 40×30，Skia 的圖片是 30×40。尺寸沒轉、圖轉了，是 bug。
- Skia 的 `readPixels` 轉成不是 premultiplied 的不是取最近的整數：1/6 × 255 = 42.5 得到 42，5/6 × 255 = 212.5 得到 212。整數的 `(c × 255 + a / 2) / a` 在 65536 × 3 個值裡有 1082 個不同；照 Skia 的 pipeline 用 `f32` 算（byte 除以 255、乘上 alpha 的倒數、乘 255、一半取偶數）是 0 個不同。opaque 的像素 Skia 照原樣複製。

**依賴**：`image` 0.25.10（關掉預設的 feature，開 `png`、`jpeg`、`gif`、`bmp`、`ico`）和 `jpeg-encoder` 0.7.1。`Cargo.lock` 多 10 個 package：`image`、`jpeg-encoder`、`zune-jpeg`、`zune-core`、`gif`、`weezl`、`color_quant`、`byteorder-lite`，和沒預期到的 `moxcms`、`pxfm`（`image` 不管開哪些格式都帶進的色彩管理和它的數學函式；純 Rust，machopo 沒有呼叫到）。`png` 是 machopo 原本的 0.18.1，沒有編兩份；`cargo tree -d` 多出來的重複是 0 個（`miniz_oxide`、`syn` 的兩個版本原本就有）。

**實作時發現的**：

- `image` 的 ICO 選 bit 數最多、其次最大的那一張，Blink 的 `ICOImageDecoder` 先比大小。照 `image` 的，差異記在 `docs/limitations.md` 的「圖片」。
- `image` 的 ICO 只收 RGBA 的 PNG（`PngNotRgba`），Skia 什麼都讀：repo 裡的 RGB 的 PNG 包成 ICO，改之前解得開，現在不行。記在同一條限制。
- `image` 的 JPEG decoder 建立時只讀檔頭，方向從檔頭裡的 EXIF 拿，所以讀尺寸不用自己解析 APP1；它會把整個檔案複製一份。
- 只有檔頭、沒有 IDAT 的 PNG，`image` 讀不到尺寸（Skia 的 `Codec` 讀得到）。測試改用改了 SOF 的寬高的 JPEG。
- 空的圖片（寬或高是 0）各個 decoder 自己就拒絕（JPEG 和 BMP 試過），原本寫的 `pixels > 0` 的檢查沒有測試分得出來，拿掉了。
- 轉成不是 premultiplied 的算式原本照 Skia 先 clamp 到 0 到 1；之後的 `saturated_cast` 本來就停在 255，clamp 拿掉了。
- `jpeg-encoder` 的量化表照 quality 縮放的算式和 libjpeg 一樣（quality 50 是 Annex K 的表本身，25 是兩倍），有測試。

**mutation test**（`python3 scripts/test.py mutate rfcs/0007-gpu-raster/mutants-0.toml`，測試指令是 `cargo test -q -p raster`）：19 條都抓到。

| 弄壞了什麼 | 哪個測試失敗 |
| --- | --- |
| 尺寸不照方向轉；像素不照方向轉；鏡像的四分之一轉不對調寬高 | `a_jpegs_orientation_turns_its_size_and_pixels` |
| 解出來的像素不 premultiply；premultiply 往下取整 | `decoded_pixels_are_premultiplied` |
| 不限制 pixel 的數量；上限少一個 pixel；解碼前不先檢查尺寸 | `a_header_claiming_too_many_pixels_is_no_image`（最後一條卡了 23 秒才失敗：真的去配置了） |
| 每次要圖片都重新解；每次都建新的項目 | `an_image_is_decoded_once`、`threads_share_a_cache` |
| 資料沒了之後項目還留著 | `a_decoded_image_is_dropped_with_its_data` |
| 轉回不是 premultiplied 時一半往上取；直接用除的；opaque 的也轉 | `unpremultiplying_rounds_as_skia_does`、`unpremultiplying_is_skias_for_every_color_and_alpha`、`a_png_is_the_pixels_not_premultiplied` |
| JPEG 不疊在黑色上；一律 4:2:0；一律 4:4:4；quality 寫死；邊長的上限是 65,535 | `a_jpeg_is_blended_on_black`、`a_jpegs_quality`、`a_jpegs_tables_are_libjpegs`、`nothing_for_no_pixels` |

第一次跑有 2 條沒抓到，都是多餘的程式，拿掉了（上面「實作時發現的」的空的圖片和 clamp），不是補測試。

沒有 mutation 的（記在 `docs/test-gaps.md` 的「只靠審查」）：`catch_unwind` 和 `Limits` 的 `max_alloc`。手上沒有會讓 decoder panic 或配置過量的檔案。

**`test.py mutate` 在這台機器上寫檔失敗了三次**（`OSError: [Errno 22] Invalid argument`，寫 `encode.rs` 兩次、`skia_backend.rs` 一次；每次都在跑了幾條之後）：工具當場停下，原檔留在 `target/mutate/originals/`，用 `test.py mutate --restore` 放回去，再用 `--only` 跑剩下的。原因沒查（猜是別的程式剛好開著那個檔案）；每次還原之後都確認過 `cargo test -p raster` 通過、`target/mutate/originals/` 是空的。

**reftest、WPT、情境**：

- `python3 scripts/test.py reftest images/ backgrounds/ canvas/`：36 個都和預期一樣，沒有改預期結果、沒有加 fuzzy（包括拿 JPEG 和 GIF 比的 `images/m6-jpeg-gif.html`）。
- `python3 scripts/test.py wpt html/semantics/embedded-content/the-img-element/ html/canvas/element/drawing-images-to-the-canvas/ html/canvas/element/pixel-manipulation/ css/css-images/image-set/`：2418 個檢查（290 個測試、2128 個 subtest），2416 個和預期一樣；2 個不一樣的都是原本預期失敗、現在通過：`the-img-element/natural-size-orientation.html` 的兩個 subtest（`naturalWidth`、`naturalHeight` 回傳照方向轉過的值）。那個檔案現在全部通過，它的 `.ini` 刪掉了（`MACHOPO_UPDATE_EXPECTATIONS=1` 跑那一個）。
- WPT 的 `css/css-images/image-orientation/` 不在 `tests/wpt/include.txt` 裡，用 `--all` 單獨跑看結果，沒有加進清單：35 個測試，reftest 21 個通過、10 個失敗、2 個逾時，testharness.js 的 2 個都 OK。失敗的是要 `image-orientation: none` 的（`image-orientation-none*.html`）、PNG 的 eXIf（`image-orientation-exif-png.html`）、`<embed>` 和 `<object>`、圖片的文件、動態改 property 的其中兩個、背景圖的一個；沒有逐一查原因。逾時的是 `image-orientation-none-cross-origin-canvas-css-image-value.sub.html` 和 `image-orientation-none-cross-origin-svg.html`。
- `python3 scripts/test.py scenarios canvas image-set`：`canvas`、`canvas-animation`、`image-set` 都通過。
- `cargo test -p shell --test it -- canvas:: images:: lazy_images:: documents:: file_reader::`：69 個通過。

**`about:demo` 和情境**：`resources/demo.html` 加了一張 16×8、EXIF 方向是 6 的 JPEG（data URL），旁邊由 `onload` 寫出 `naturalWidth` 和 `naturalHeight`；`tests/scenarios/image-orientation.txt` 等到那段文字是「8 by 16」再截圖。看過截圖：圖是直的，紅色在上、藍色在下。沒有動到視窗那一層，沒跑 `test.py automation`。

**階段結束的檢查**（第 0 步是一個階段）：

- `cargo test --workspace`：55 個測試 binary，3749 個通過，0 個失敗。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`：通過。
- `python3 scripts/sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`：通過。
- 改了 `resources/demo.html` 之後：`python3 scripts/test.py scenarios` 78 次執行都通過，`cargo test -p shell` 通過。
- 沒有跑的：`test.py automation`（沒動到視窗那一層）、整套的 `wpt`、`wdspec`、`matrix`、`fuzz`（CI 跑）、Speedometer。

**Speedometer**（使用者要求；`python3 scripts/bench.py speedometer --against HEAD`，`against` 是 `28f0fde`（只有 RFC 的文件，程式碼是改之前的），`current` 是第 0 步的工作目錄；只跑 machopo）：

- 20 個 suite、兩邊輪流各 3 次：`current` / `against` 的幾何平均是 1.005（+0.5%），記憶體峰值各 suite 在 −1.6% 到 +3.1% 之間。17 個 suite 在 ±4% 以內；看起來變慢的三個是 NewsSite-Next +19.0%（98.3 → 117.0 ms）、TodoMVC-WebComponents +8.1%（22.3 → 24.1）、TodoMVC-Vue +5.1%（19.6 → 20.6）。
- 那三個再跑一次、各 8 次（`--suites NewsSite-Next,TodoMVC-WebComponents,TodoMVC-Vue --runs 8`）：NewsSite-Next −1.2%（94.1 → 92.9）、TodoMVC-WebComponents −3.2%（23.8 → 23.0）、TodoMVC-Vue −4.0%（22.2 → 21.3），區間都重疊。
- 第一次的差別是雜訊：兩邊開頭的幾次都比較慢，之後才穩定（NewsSite-Next 各次的中位數，`against` 是 123.5、125.4、115.2、92.8、93.9、91.8、91.3、93.9，`current` 是 123.0、114.0、92.8、92.1、90.8、92.1、93.2、91.9）。只跑 3 次時，慢的那幾次落在哪一邊就決定了結果（第一次 `against` 的三次是 122.8、93.3、119.0，`current` 是 115.0、112.9、123.6）。開頭為什麼慢沒有查。
- 結論：看不出第 0 步讓 Speedometer 變慢或變快。計時區間裡幾乎沒有圖片的解碼和 JPEG 的編碼。

### 第 1a 步：原型，常用的 op（2026-11-10，Windows 11，AMD Radeon RX 7900 XTX；不 merge）

**做了什麼**：`crates/raster/src/vello_backend.rs`，`MACHOPO_RASTER=vello` 時 tile 由 vello_cpu 畫（上游 `main` 的 `112736d`），預設還是 Skia；canvas 的 bitmap（`draw_into`）兩種模式都是 Skia。接了：transform、`ClipRect`、`ClipRRect`、`ClipPath`、`SaveLayerAlpha`、`SaveLayer` 的 alpha 和 blend mode、`DrawColor`、`DrawRect`、`ClearRect`、`DrawRRect`、`DrawDRRect`、`DrawLine`、`DrawPath`、`DrawClipped`、dash、28 種 blend mode、三種漸層、`DrawImage`、圖片和漸層的 `Tiled` shader、`DrawGlyphs`（可變字型、合成粗體和斜體）、單一圓形半徑、沒有洞的 `DrawShadow`、`DrawRecord`。沒接的跳過、在 log 裡計數（「vello prototype: not drawn」）：`ClipPaths`、image filter、mask blur、有洞或四個角不同的陰影、每個 glyph 的旋轉、`Tiled` 的 spacing、decal、record 和重複的漸層圖塊、`WritePixels`。

**跳過的 op**：`about:demo` 的前 1200px、文字比較頁、`bench.py raster` 的三頁（Gutenberg、SQLite、NewsSite-Next）都是 0 個。

**找到的 bug**（原型自己的）：一開始沒有把 tile 的起始 transform 設給 context，每個 tile 畫的都是頁面左上角；合成斜體的 skew 正負號反了（上游的測試用負的 tan）。

**文字的外觀**（`rfcs/0007-gpu-raster/text-compare.html`，`machopo --headless --test-fonts`，800×420，Skia 和 Vello 各一張，放大 4 倍對照看過）：

- 深色背景上的淺色字：Skia 比較粗，Vello 比較細。
- 淺色背景上的深色字：反過來，Skia 比較細、淡，Vello 比較粗、黑。Skia 的 gamma 和對比的調整讓深底的字變粗、淺底的字變細；Vello 兩邊一樣。
- 不同的 pixel：1x 是 336,000 個裡的 43,658 個（13.0%），平均差 31、最大 116；2x 是 1,344,000 個裡的 91,432 個（6.8%），平均差 31、最大 131。都在 glyph 的邊緣。
- 其他頁面（1x）：`about:demo` 的前 1200px 10.1% 的 pixel 不同（平均 20、最大 123），Gutenberg 的開頭 5.2%（平均 25、最大 120）。

**速度**（`python3 scripts/bench.py raster`，headless、scale 2、預設的 raster thread，`perf` profile；各跑一次，背景有別的程式在播影片，不同批之間的比較要保守看）。每個 tile 的 `PrepareTiles` 時間（ms），括號裡是總時間和 tile 數：

| 頁面 | 階段 | Skia | Vello，整段文字都交給它 | 同左，開 glyph atlas | Vello，先濾掉離 tile 遠的 glyph | 同左，開 glyph atlas |
| --- | --- | --- | --- | --- | --- | --- |
| Gutenberg | 捲動 | 0.381（247 / 649） | 0.988（641 / 649） | 1.894（1230 / 649） | 0.571（371 / 649） | 2.691（1746 / 649） |
| Gutenberg | 改視窗大小 | 0.151（944 / 6270） | 0.411（1384 / 3365） | 0.874（1662 / 1903） | 0.152（970 / 6400） | 1.468（2060 / 1403） |
| SQLite | 改視窗大小 | 0.085（469 / 5514） | 0.425（1555 / 3656） | 0.543（1491 / 2744） | 0.164（896 / 5475） | 0.694（1503 / 2165） |
| NewsSite-Next | 捲動 | 0.266（286 / 1077） | 0.308（331 / 1077） | 0.875（943 / 1077） | 0.195（210 / 1077） | 1.182（1273 / 1077） |
| NewsSite-Next | 改視窗大小 | 0.536（2037 / 3800） | 0.285（1664 / 5840） | 1.208（2424 / 2007） | 0.112（1271 / 11346） | 1.051（2086 / 1985） |

（SQLite 的捲動沒有 raster 任何 tile。改視窗大小的階段在固定的時間裡能做幾次更新就做幾次，所以各欄的 tile 數不同。）

- **整段文字都交給 Vello 時，文字多的頁面慢 2.6 到 5 倍**。原因：一段文字是一個 op，它碰到的每個 tile 都收到整個 op；Vello 先把 glyph 的輪廓攤平才知道它在 tile 外面，Skia 靠 glyph 的範圍先跳過。
- **先濾掉離 tile 遠的 glyph**（距離超過字級的兩倍加上 stroke 的寬度，在 playback 裡）之後：Gutenberg 捲動 1.5 倍、改大小 1.0 倍，SQLite 1.9 倍，NewsSite-Next 捲動 0.73 倍、改大小 0.21 倍。濾和不濾畫出來的圖逐像素相同（文字比較頁 2x，1,344,000 個 pixel 0 個不同）。`MACHOPO_VELLO_NO_CULL=1` 關掉它。
- **Vello 實驗性的 glyph atlas**（`atlas_cache(true)`，`MACHOPO_VELLO_GLYPH_CACHE=1`）在每一頁都更慢，不管有沒有濾 glyph：不能用。為什麼更慢沒有查。

**跑過的檢查**：`cargo test -p raster` 通過（預設是 Skia，75 個）；`cargo clippy -p raster --all-targets -- -D warnings`、`cargo fmt` 通過；`sanity.py` 的 `deps`、`boundary`、`unsafe`、`numerics` 通過（開關原本是 process 共用的 `static`，`boundary` 抓到，改成每條 thread 讀一次；原型的三個 thread-local 列進 `scripts/sanity.py` 的 `STATICS_ALLOWED`，其中兩個標成只給原型用，第 2 步拿掉時 sanity 會提醒把它們從清單拿掉）。`vello_backend.rs` 整個檔案帶一個 `expect(clippy::arithmetic_side_effects)`（kurbo 的 f64 幾何運算），進 `main` 之前要逐處看。沒有為 Vello 的 playback 寫測試、沒有跑 mutation test：這一段是不 merge 的原型，第 2 步才補。

**速度，重量一次**（使用者要求；同一個執行檔，Skia 和有濾 glyph 的 Vello 輪流各跑 3 次：Skia、Vello、Skia、Vello、Skia、Vello，每次約 30 秒）。每個 tile 的 `PrepareTiles` 時間（ms），三次由小到大：

| 頁面 | 階段 | Skia | Vello | 中位數的比（Vello / Skia） |
| --- | --- | --- | --- | --- |
| Gutenberg | 捲動 | 0.564、0.597、0.616 | 0.310、0.669、0.690 | 1.12 |
| Gutenberg | 改視窗大小 | 0.143、0.154、0.203 | 0.145、0.148、0.149 | 0.97 |
| SQLite | 改視窗大小 | 0.090、0.093、0.094 | 0.145、0.150、0.152 | 1.62 |
| NewsSite-Next | 捲動 | 0.292、0.295、0.296 | 0.236、0.250、0.270 | 0.85 |
| NewsSite-Next | 改視窗大小 | 0.174、0.187、0.192 | 0.107、0.108、0.112 | 0.58 |

- 捲動時最長的一次更新（ms，三次）：Gutenberg 是 Skia 8.9、11.7、7.7，Vello 9.5、11.6、8.4；NewsSite-Next 是 Skia 17.9、18.1、18.4，Vello 10.1、9.5、9.8。
- 和上面只跑一次的表比，有兩格差很多，那兩格當時是離群值：Gutenberg 捲動的 Skia（0.381，這次三次都在 0.56 到 0.62），NewsSite-Next 改視窗大小的 Skia（0.536，這次 0.17 到 0.19）。只跑一次的數字不能拿來下結論。
- Gutenberg 捲動的 Vello 三次裡有一次是 0.310，另外兩次是 0.67 左右，原因沒查；這一格的比（1.12）最不確定。
- 結論：濾掉遠的 glyph 之後，Vello 在文字多的頁面和 Skia 差不多到慢 1.6 倍（SQLite 最慢，三次很一致），圖和色塊多的頁面快 15% 到 40%。glyph atlas 沒有重量（上面量過兩次都明顯更慢）。

**SQLite 的那一頁慢在哪**（使用者要求；原型加了 `MACHOPO_VELLO_PROFILE=1`，把每個 tile 的時間照 op 拆開記在 log；`bench.py raster --pages sqlite,gutenberg`，改視窗大小的階段，4096 個 tile 的平均）：

| | SQLite | Gutenberg |
| --- | --- | --- |
| 每個 tile | 127 µs | 176 µs |
| `DrawGlyphs` | 104 µs | 112 µs |
| ops 之後的 render | 20 µs | 60 µs |
| 其他的 op 合計 | 約 3 µs | 約 4 µs |
| 每個 tile 幾個 `DrawGlyphs` | 7.4 | 3.8 |
| 每個 tile 交給 Vello 的 glyph（濾之前） | 106（356） | 72（280） |

- 時間幾乎都在文字：每個 glyph 約 1 µs（SQLite）到 1.5 µs（Gutenberg）。SQLite 每個 tile 的 glyph 比較多、切成比較多段，所以比 Gutenberg 吃虧。
- **machopo 自己快取輪廓沒有用**（`MACHOPO_VELLO_OUTLINE_CACHE=1`：一般的 glyph 的輪廓第一次用到時用 skrifa 讀出來、以字型的單位存著，一段文字合成一條 path 一次填）：SQLite 的 106 個 glyph 全部走這條路，`DrawGlyphs` 還是 106 µs（原本 104）。Skia、Vello、Vello 加輪廓快取輪流各 3 次，每個 tile 的時間對 Skia 的比：Gutenberg 捲動 1.08 和 1.08、改大小 1.09 和 1.07，SQLite 1.69 和 1.67，NewsSite-Next 捲動 0.77 和 0.75、改大小 0.57 和 0.53。畫出來幾乎一樣（文字比較頁 1x 有 18 個 pixel 不同、最大差 4，2x 有 6 個、最大差 1）。所以時間不在讀字型的輪廓，在 Vello 把 path 攤平、產生 strip。
- **Vello 的 glyph atlas 其實有省到畫文字的時間，慢在每次 render 之前的同步**：開了之後（`MACHOPO_VELLO_GLYPH_CACHE=1`）SQLite 的 `DrawGlyphs` 從 104 µs 降到 25 µs，但 ops 之後的 render 從 20 µs 變成 1124 µs。讀 `vello_cpu/src/text.rs` 的 `sync_glyph_cache`：每次 render 前把還沒進 atlas 的 glyph 畫進一個 context、再合成到 atlas 的那一頁，atlas 的一頁是 4096×4096（`DEFAULT_GLYPH_ATLAS_SIZE`，不是公開的設定）。machopo 一個 tile render 一次，每個 tile 幾乎都有新的 glyph（位置是 subpixel 的），所以每個 tile 都付一次。這是讀原始碼的推論，沒有再往下量是哪一步佔了那 1 ms。
- 這一輪 Gutenberg 捲動的 Skia 三次是 0.369、0.672、0.675，上一輪是 0.564、0.597、0.616：這一格兩邊都不穩。

**Vello 的 glyph atlas 的同步**（使用者要求；Vello 複製一份到暫存目錄、`Cargo.toml` 暫時用 `[patch]` 指過去，改 `vello_cpu/src/text.rs` 讓 atlas 的頁面大小和項目的保留時間可以從環境變數設：`MACHOPO_VELLO_ATLAS_SIZE`、`MACHOPO_VELLO_ATLAS_AGE`；改動存成 `rfcs/0007-gpu-raster/vello-atlas-experiment.patch`，`[patch]` 那一段不 commit）：

- **原因確認了**：`sync_glyph_cache` 每次 render 前把還沒進 atlas 的 glyph 畫進一個和 atlas 的頁面一樣大的 `RenderContext`，再 render 到那一頁；頁面是 4096×4096。SQLite 的那一頁，原型內部的計時（µs，每個 tile）：

  | atlas 的頁面 | `DrawGlyphs` | ops 之後的 render | 合計 |
  | --- | --- | --- | --- |
  | 不開 | 100 | 20 | 123 |
  | 4096（上游的預設） | 25 | 1124 | 約 1150 |
  | 1024 | 20 | 90 | 114 |
  | 512 | 21 | 73 | 98 |
  | 256 | 21 | 68 | 93 |

- **保留時間**（`GlyphCacheConfig::max_entry_age`，預設 64 次 render；machopo 一個 tile 一次 render）：頁面 512 時，64 是 95 µs，4096 是 82 µs，一百萬是 83 µs。
- **斜的 glyph 開 atlas 會畫錯**：文字比較頁的斜體那幾行，同一行裡有的 glyph 是斜的、有的是正的（和不開 atlas 的圖比，1x 最大差 238）。看起來 atlas 的 key 不分 `glyph_transform`。原型裡斜的文字不走 atlas 之後，和不開 atlas 的只差位置的量化（1x 是 10.0% 的 pixel、最大差 60、平均 11；放大看粗細一樣）。合成粗體沒看到問題。
- **三方的比較**（Skia、Vello、Vello 加 atlas（頁面 512、保留 4096）輪流各 3 次）。每個 tile 的 `PrepareTiles`（ms，三次由小到大）和對 Skia 的中位數的比：

  | 頁面 | 階段 | Skia | Vello | Vello 加 atlas | Vello / Skia | 加 atlas / Skia |
  | --- | --- | --- | --- | --- | --- | --- |
  | Gutenberg | 捲動 | 0.622、0.651、0.661 | 0.676、0.707、0.755 | 0.441、0.622、0.692 | 1.09 | 0.96 |
  | Gutenberg | 改視窗大小 | 0.146、0.154、0.167 | 0.149、0.150、0.166 | 0.114、0.123、0.131 | 0.97 | 0.80 |
  | SQLite | 改視窗大小 | 0.091、0.092、0.096 | 0.145、0.158、0.161 | 0.111、0.118、0.124 | 1.71 | 1.28 |
  | NewsSite-Next | 捲動 | 0.295、0.296、0.315 | 0.213、0.250、0.255 | 0.235、0.250、0.269 | 0.84 | 0.84 |
  | NewsSite-Next | 改視窗大小 | 0.179、0.183、0.187 | 0.108、0.122、0.123 | 0.101、0.104、0.114 | 0.66 | 0.57 |

- 結論：atlas 的頁面改小、保留時間拉長之後，五格裡四格比 Skia 快或一樣，最慢的 SQLite 從 1.71 倍降到 1.28 倍。剩下的差在 render（開 atlas 後 59 到 73 µs，不開 20 µs：從 atlas 貼 glyph 比填純色貴，每次 render 還是同步一次），沒有再往下查。這兩個設定在上游不是公開的，要用就得改 Vello：這是要不要 fork 的第一個具體的候選。
- 之前說「可能比 Skia 還快」是拿原型內部的計時（不含上傳和其他）和 Skia 的 `PrepareTiles` 比，基準不同，不成立。

**1a 收尾**：檢查點通過之後，輪廓快取和 atlas 的開關從原型拿掉（atlas 回到上游的預設：不開），`Cargo.toml` 指向本機 Vello 的 `[patch]` 拿掉；剩下的開關是 `MACHOPO_RASTER`、`MACHOPO_VELLO_NO_CULL` 和 `MACHOPO_VELLO_PROFILE`。拿掉之後文字比較頁和之前的圖逐像素相同。

### 第 1b 步：原型，全部的 op 和組出來的功能（2026-11-10，Windows 11，AMD Radeon RX 7900 XTX；不 merge）

**做了什麼**：`vello_backend.rs` 重寫，1a 跳過的都接上；canvas 的 bitmap（`draw_into`）也由 Vello 畫。

- image filter：layer 的 op（到它的 `Restore`）畫到旁邊一個四周加大的 context（加大的量是 filter 的範圍乘上 scale，最多 1024 px），render 成 pixmap，套 filter，再當成圖片帶著 layer 的 alpha 和 blend mode 畫回去。Blur 和 DropShadow 是把 pixmap 畫進帶 Vello filter 的 layer；ColorMatrix 是 machopo 逐個 pixel 算（先 unpremultiply）；Chain 照順序。因為旁邊的 context 比 tile 大，tile 外面的內容也在裡面，模糊跨過 tile 邊界時有東西可以模糊。
- `ClipPaths`：每條 path 照它的 fill rule 填進一張和 context 一樣大的 pixmap，做成 alpha mask，`push_mask_layer`。
- `DrawShadow`：單一圓形半徑、沒有洞的用 `fill_blurred_rounded_rect`；其他的把形狀（減掉洞）畫到旁邊、模糊、畫回去。兩種都先 clip 到盒子的裡面或外面。
- mask blur（`PaintFlags.blur`）：op 不帶 blur 畫到旁邊、模糊、帶著原本的 blend mode 畫回去。
- 圖案 shader：record、有 spacing 的、會重複的漸層圖塊，先把一個圖塊（連同間隔）照看到的大小畫成 pixmap（每邊最多 4096）再當圖片重複；`decal` 用 clip。
- 每個 glyph 的旋轉：一個 glyph 一個 run。
- canvas：bitmap 原本的像素先畫進場景當底，再畫錄製的 op，最後整個 render 回去；`WritePixels` 先把前面的 render 進 bitmap、寫入像素、reset、把還在的 clip 推回去。opaque 的畫完把 alpha 設回 255。

**原型自己的 bug，修了的**：

- 圓角矩形原本用三次曲線近似，圓和橢圓走 `gfx::Path` 的 conic：同一個形狀兩種邊，machopo 的 reftest 有 4 個不過。改成都用 `gfx::Path::add_round_rect` 之後 3 個通過（`canvas/c4-paths.html`、`effects/clip-path.html`、`svg/ellipses.html`），WPT 的 `clip-path-*Box-1e.html` 三個和 `clip-path-polygon-014.html` 也通過。
- canvas 上破壞性的 blend mode（`ClearRect`、`copy`）沒有作用：原本是畫完才用 `TargetInit::SrcOver` 疊到 bitmap 上，場景裡沒有 bitmap 原本的像素可以拿掉。改成先把 bitmap 畫進場景（每次 flush 複製一份；Skia 是直接畫在 bitmap 上）。
- 圖片的來源矩形：取樣取到矩形外面的像素。改成先把那一塊裁成自己的 pixmap。
- 長度是 0 的 dash 配圓頭：kurbo 不畫，Skia 畫成一個點。給它 0.001 的長度。
- shader 的填色沒有帶 flags 的 alpha（canvas 的 `globalAlpha` 配 pattern 和漸層）。
- conic 切成 4 段二次曲線，放大很多倍的弧邊緣偏掉（WPT 的 `2d.path.arc.scale.1.html`）：改切 16 段。

**用 Vello 跑整套**（`MACHOPO_RASTER=vello`；最後一輪）：

- `python3 scripts/test.py reftest`：195 個都和預期一樣（用的是原本為 Skia 寫的容許誤差）。
- `python3 scripts/test.py scenarios`：78 次執行，0 個失敗。
- `python3 scripts/test.py wpt`（整套）：13,917 個測試、293,653 個檢查，107 個檢查和預期不一樣，分屬 50 個測試：
  - **Vello 自己 panic，1 個**：`css/css-tables/crashtests/caption-repaint-crash.html`，`vello_common/src/clip.rs:723` 的 `attempt to subtract with overflow`（`cur_strip_fill_area` 的 `next.x - x`），compositor thread 跟著結束。debug build 才 panic，release 沒有溢位檢查。沒有查是哪個 op 觸發的。
  - **canvas，5 個**：`2d.composite.clip.destination-atop`、`destination-in`、`source-in`、`source-out`（破壞性的 blend mode 不受不隔離的 clip 限制，clip 外面的像素也被改了）；`2d.line.cross`（沒查）。
  - **reftest，35 個**，每個的截圖和參考圖比過：
    - 最大差 1 的 31 個：1 到 30 個 pixel 的 23 個，142 到 588 個 pixel 的 8 個（`css-tables/tentative/paint/background-image-*` 三個、`text-transform-full-size-kana-005` 到 `007`、`box-shadow-041`、`box-shadow-042`）。
    - 最大差 3 的 1 個：`clip-path-animation-custom-timing-function.html`（628 個 pixel）。
    - 最大差 31 的 2 個：`text-decoration-thickness-percent-001.html`（60 個 pixel）、`vars-font-shorthand-001.html`（600 個）。
    - **畫錯的 1 個**：`css-images/multiple-position-color-stop-radial.html`（5000 個 pixel 差 255：100×100 的方塊一半的顏色錯；`radial-gradient(ellipse 50px 10000px at 0px 50px, blue 0% 50px, green 50px)`）。沒查是極端的橢圓還是同一個位置的兩個色標。
  - **原本預期失敗、現在通過，3 個**：`css-transforms/scale/svg-scale-016.html`（它的 `.ini` 記著只差邊緣的反鋸齒）、`html/rendering/widgets/button-layout/display-other.html`、`baseline-alignment-and-overflow.tentative.html`（後兩個為什麼通過沒查）。
  - **和繪圖無關、每一輪不一樣的，6 個**（計時和檔案時間的）：`navigation-timing/` 的四個、`cssom-view/scroll-behavior-smooth-navigation.html`、`FileAPI/file/File-lastModified.html`。第二輪多出來的 13 個（圖片載入、srcset）單獨重跑，Vello 下 12 個通過、Skia 下都通過：整套一起跑時機器太忙。
  - 這一輪 `test.py wpt` 的 Python process 在最後回報 `Segmentation fault`，結果的 log 已經寫完；原因沒查，前兩輪沒有。
- `cargo test --workspace --no-fail-fast`：3741 個通過，8 個失敗：
  - 不適用的 4 個（直接看 Skia 的快取）：`raster` 的 `a_decoded_image_is_dropped_with_its_data`、`an_image_is_decoded_once`、`threads_share_a_cache`，`renderer` 的 `the_typefaces_drawn_with_stay_cached_for_the_next_flush`。
  - mipmap 還沒做的 1 個：`raster` 的 `a_decoded_image_is_drawn_as_its_pixels_sampled_as_its_filter_says`。
  - 差 1 的 2 個：`renderer` 的 `a_flush_sends_the_recording_to_the_thread_and_forgets_it`（191 對 192）、`browser` 的 `navigation::links::canvas_2d`（漸層中間的顏色 127,0,128 對 126,0,129）。
  - 不穩定的 1 個：`shell` 的 `devtools_initiators::script_asks_with_its_stack`，單獨跑通過。
  - `raster` 的 75 個單元測試（對著 Skia 的像素寫的）有 71 個照原樣通過。

**還沒做、還沒查的**：mipmap（第 2 步）；Vello 的 playback 自己的測試和 mutation test（第 2 步）；上面寫「沒查」的每一項；filter 在 tile 邊緣的接縫沒有另外用測試確認（加大的 context 是照推論做的，reftest 都通過）；離屏的做法每個 tile 都配置新的 context 和 pixmap，速度沒有量；開了 `MACHOPO_RASTER=vello` 的 `bench.py raster` 在 1b 之後沒有重量。

**跑過的檢查**：`cargo test -p raster`（預設 Skia）通過；`cargo clippy -p raster --all-targets -- -D warnings`、`cargo fmt` 通過；`sanity.py` 的 `docs`、`boundary` 通過。

**第二個檢查點前補查的**（使用者要求，2026-11-10）：

- **Vello 的 panic**：最小的重現是一條平頭（butt cap）的 stroke，整條在 context 的右邊緣之外、而且剛好從邊緣開始（寬 3 的直線在 x = 寬度 + 1.5），clip 的多邊形也從邊緣開始；`vello_common::clip` 的 `cur_strip_fill_area` 算 `next.x - x` 時變成負的。32 寬和 256 寬的 context 都會；線往左移一個 pixel（碰到最後一欄）就不會，圓頭的 stroke 也不會。盒子的邊框一邊一條線、各自 clip 成梯形，邊框貼著 tile 的邊界時，隔壁的 tile 收到的就是這種。原型的處理：clip 整個在 context 外面（`Player::shows`）時不推進 Vello，它底下的東西也不畫；那個 crashtest 不再 panic。`vello_backend` 的測試 `upstream_panics_on_a_clipped_stroke_at_the_right_edge` 記著上游的行為（上游修了它就失敗）。這只避開這一種；第 2 步還是要包 `catch_unwind`。
- **radial gradient 畫錯**（`multiple-position-color-stop-radial.html`）：和橢圓的形狀無關。最後兩個色標都在 offset 1、顏色不同（結尾的硬切）時，超過終點的地方 Vello 用它在 offset 1 查到的顏色（兩個裡的第一個），Skia 用最後一個。原型把這種漸層拉長一倍、色標的 offset 減半（只做不重複的 linear 和 radial），和 Skia 的圖逐像素相同。一開始對「最後兩個色標同一個 offset」都這樣做，`css-images/gradient-move-stops.html`（硬切在 70%）因為 LUT 的解析度減半差了 1（3900 個 pixel）：改成只在 offset 是 1 時做，`test.py wpt css/css-images/` 的 744 個檢查都和預期一樣。
- **`2d.line.cross.html`**：線寬 200、兩段各 10 px 的直角、bevel 的接合，照規格畫布裡沒有東西；kurbo 的 stroker 在線寬遠大於線段長度時，內側的接合多畫了一塊。沒有修，machopo 這一側沒有合理的繞法。
- **Vello 的 f32 管線**（`RenderMode::OptimizeQuality`，`vello_cpu` 的 `f32_pipeline` feature；原型的 `MACHOPO_VELLO_QUALITY=1`）：結果有變的 50 個 WPT 測試用兩種管線各跑一次，f32 的讓 5 個通過（`css-tables/tentative/paint/background-image-column.html`、`-column-collapsed.html`、`-row-collapsed.html`，`box-shadow-041.html`、`-042.html`：差 1 的裡面 pixel 數最多的，漸層和模糊的混色），沒有讓哪一個變壞；其餘差 1 的沒有變（都和文字有關，1 到 30 個 pixel 的那些）。速度（Skia、u8、f32 輪流各 3 次，每個 tile 的 `PrepareTiles` 對 Skia 的中位數的比）：

  | 頁面 | 階段 | Skia（ms，三次） | Vello u8 | Vello f32 | u8 / Skia | f32 / Skia |
  | --- | --- | --- | --- | --- | --- | --- |
  | Gutenberg | 捲動 | 0.589、0.684、0.721 | 0.653、0.714、0.744 | 0.810、0.947、0.956 | 1.04 | 1.38 |
  | Gutenberg | 改視窗大小 | 0.135、0.150、0.152 | 0.145、0.159、0.160 | 0.264、0.266、0.270 | 1.06 | 1.77 |
  | SQLite | 改視窗大小 | 0.086、0.092、0.095 | 0.147、0.157、0.161 | 0.245、0.249、0.253 | 1.71 | 2.72 |
  | NewsSite-Next | 捲動 | 0.299、0.302、0.335 | 0.234、0.265、0.289 | 0.349、0.419、0.427 | 0.88 | 1.39 |
  | NewsSite-Next | 改視窗大小 | 0.170、0.184、0.189 | 0.107、0.111、0.124 | 0.223、0.223、0.230 | 0.60 | 1.21 |

  f32 的只消掉 5 個、每一格都比 Skia 慢：不用。u8 的數字和 1a 的一樣，1b 加的東西沒有讓這三頁變慢（這三頁沒有 filter、複雜的陰影和 mask，離屏的成本這裡看不到）。
- **兩個原本預期失敗、現在通過的測試和 Vello 無關**：`html/rendering/widgets/baseline-alignment-and-overflow.tentative.html`（60 個 subtest）和 `button-layout/display-other.html` 用 Skia 單獨跑也是通過。前者的 `.ini` 寫的原因是 machopo 沒有 `document.images`：`main` 上的預期結果過時了，不是這份 RFC 的，沒有動。

**補查之後的整套**（`MACHOPO_RASTER=vello`，u8 的管線）：

- `test.py reftest` 195 個都和預期一樣；`test.py scenarios` 78 次執行 0 個失敗。
- `test.py wpt`：Vello 造成的不通過是 39 個測試：
  - canvas 5 個：`2d.composite.clip.destination-atop`、`destination-in`、`source-in`、`source-out`，`2d.line.cross`。
  - reftest 34 個：最大差 1 的 31 個、最大差 3 的 1 個、最大差 31 的 2 個（清單在上面第 1b 步的那一段，少了已經通過的 radial gradient）。
  - 另外 `svg-scale-016.html` 從預期失敗變成通過；其餘結果有變的是上面那兩個過時的預期結果和每一輪不一樣的計時類測試。
- `cargo test --workspace --no-fail-fast`：3743 個通過，7 個失敗：4 個直接看 Skia 的快取（不適用）、1 個 mipmap（還沒做）、2 個差 1（`renderer` 的 `a_flush_sends_the_recording_to_the_thread_and_forgets_it`、`browser` 的 `navigation::links::canvas_2d`）。

**差 1 的那一群的原因，和 glyph 位置的對齊**（2026-11-10）：

- 把幾個測試不同的 pixel 列出來（`quotes-032.html` 1 個、`table-anonymous-objects-079.xht` 22 個、`float-nowrap-2.html` 3 個、`counter-001.html` 4 個、`text-transform-full-size-kana-005.html` 306 個）：都在 glyph 的邊緣，每個 channel 差 1（194 對 195 這種）。測試頁和參考頁的同一段文字位置差了浮點數的誤差；Skia 的 glyph 快取把位置對齊（沿著行每四分之一個 pixel、垂直方向整數 pixel），誤差被吸收；Vello 位置在哪就畫在哪。
- 原型加了 `MACHOPO_VELLO_SNAP=<n>`：matrix 只有縮放和位移時，交給 Vello 之前把每個 glyph 的原點對齊到 device pixel 的 n 分之一（沿著行）和整數（垂直）。結果有變的那 50 個測試重跑，不通過的 reftest：不對齊 34 個，n = 4 是 8 個，n = 64 是 11 個。
- n = 4 跑整套（`MACHOPO_RASTER=vello MACHOPO_VELLO_SNAP=4`）：`test.py reftest` 195 個都和預期一樣，`test.py scenarios` 78 次 0 個失敗，`test.py wpt` Vello 造成的不通過剩 13 個測試：
  - canvas 5 個（和之前一樣）。
  - reftest 8 個：`css-tables/tentative/paint/background-image-column.html`、`-column-collapsed.html`、`-row-collapsed.html`、`box-shadow-041.html`、`-042.html`（混色的差 1，f32 的管線會過）、`clip-path-animation-custom-timing-function.html`（最大差 3）、`contain-paint-clip-006.html`、`css-scroll-anchoring/nested-overflow-subtree-layout.html`（後兩個沒看）。
  - 沒有因為對齊而變壞的；最大差 31 的 `text-decoration-thickness-percent-001.html` 和 `vars-font-shorthand-001.html` 也通過了。
- 對齊之後文字比較頁和 Skia 的差距幾乎沒變（1x：13.0% 的 pixel 變 12.9%，平均差 31.3 變 30.5）：那個差距是粗細，不是位置。

### 第 2a 步：Vello 成為唯一的 playback（2026-11-11，Windows 11，AMD Radeon RX 7900 XTX）

**做了什麼**：

- `raster::playback_into` 和 `draw_into` 直接呼叫 `vello_backend`；原型的環境變數（`MACHOPO_RASTER`、`MACHOPO_VELLO_*`）、計時和 f32 管線的開關都拿掉，`vello_cpu` 的 `f32_pipeline` feature 也關掉。glyph 的對齊（四分之一 pixel）和 culling 變成固定的行為。
- `skia_backend.rs` 剪到只剩 2b 要換掉的部分：Skia 的 typeface 快取、`ink_bounds`、collection 的拆解，和拿 Skia 對照 `gfx::Path` 的測試。少了 2,200 多行，剩 748 行。
- 原本對著 Skia 寫的 52 個 playback 測試搬到 `playback_tests.rs`，改成只經過 `raster` 的公開 API；看快取的那幾個改看 Vello 的快取（幾張圖、解了幾次、資料沒了之後項目有沒有清掉）。**搬過來之後 52 個都通過**，沒有為了讓它們通過而改預期的值。
- 補的地方每一處有一行 `vello:` 開頭的註解（14 處），寫上游缺什麼或哪裡錯。上游的 issue 編號沒有寫：還沒有逐條對過上游的 issue，2d 寫缺口的文件時一起對。
- 邊長超過 65,535 的 pixels（Vello 的 context 的邊長是 `u16`）不畫、記一筆 log，不再悄悄地什麼都不做。tile 不會這麼大；canvas 可以，2d 記成缺口。

**新的測試**（`cargo test -p raster`：85 個通過，1 個失敗）：

- 上面「驗收」的三列，加上 `glyphs_are_drawn_at_quarters_of_a_pixel`、`a_glyph_that_begins_outside_is_drawn`、`a_clip_outside_hides_what_is_in_it`、`past_a_hard_stop_at_its_end_a_gradient_is_the_last_color`、`on_a_canvas_a_blend_mode_takes_from_what_the_bitmap_has`、`pixels_too_wide_to_draw_on_are_left`。
- **失敗的一個是預期中的**：`lib.rs` 的 `tests::a_decoded_image_is_drawn_as_its_pixels_sampled_as_its_filter_says` 的後半（縮小時 `Medium` 要取平均）。Vello 沒有 mipmap，2b 在圖片的快取裡做；在那之前這個測試留著失敗，不標成忽略。

**tile 的接縫不是每一種都逐位相同**：

| 內容 | scale 1 | scale 1.5 |
| --- | --- | --- |
| 形狀（圓角矩形、粗線、半透明的 multiply） | 相同 | 最大差 1 |
| 文字（Lato 15px 和 40px 的描邊、Ahem 21px，位置不在整數上） | 相同 | 最大差 1 |
| 盒子的陰影（外側、內側帶洞） | 相同 | 相同 |
| layer 的 blur | 最大差 1 | 最大差 1 |
| layer 的 drop shadow | 相同 | 相同 |
| blur 接 color matrix | 相同 | 最大差 1 |

差 1 的都在反鋸齒的邊緣或模糊的範圍裡：Vello 的覆蓋率用浮點數算，同一條邊從不同的 tile 原點算（座標差 32）捨入不一樣；blur 則是從大小不同的離屏 context 算出來的。Skia 的掃描線用定點數，整數的位移不影響結果，所以原本沒有這個差異。測試在 scale 1 的形狀、文字和陰影要求逐位相同，其餘允許差 1；真的接縫（把四周加大的量拿掉）差 70 到 182，測試抓得到。差 1 肉眼看不到，但它是和 Skia 不一樣的地方，2d 記進缺口的文件。

**Mutation test**（`mutants-2a.toml`，13 條都抓到）：

| 弄壞了什麼 | 哪個測試失敗 |
| --- | --- |
| tile 四周不留 filter 和 blur 會用到的範圍 | `tiles_have_the_pixels_of_the_whole` |
| 原點在 tile 外面的 glyph 不畫 | `a_glyph_that_begins_outside_is_drawn`、`tiles_have_the_pixels_of_the_whole` |
| glyph 不對齊；垂直方向不對齊到整數；對齊到八分之一 | `glyphs_are_drawn_at_quarters_of_a_pixel` |
| 在 tile 外面的 clip 裡照樣畫 | `a_clip_outside_hides_what_is_in_it`、`the_ops_a_tile_finds_draw_it_as_all_of_them` |
| 每個 clip 都交給 Vello；剛好貼著 tile 邊緣的 clip 也交給 Vello | `upstream_panics_on_a_clipped_stroke_at_the_right_edge`、`a_clip_outside_hides_what_is_in_it`（Vello 在裡面 panic） |
| canvas 原本的像素不先畫進場景 | `on_a_canvas_a_blend_mode_takes_from_what_the_bitmap_has` |
| 結尾硬切的漸層不拉長；拉長了但色標的位置沒減半 | `past_a_hard_stop_at_its_end_a_gradient_is_the_last_color` |
| chain 只跑第一個 filter；color matrix 沒作用 | `layer_filters_invert_blur_and_chain` |

`test.py mutate` 開始前要原本的測試全過，所以這份的測試指令跳過 mipmap 那一個（2b 做完時拿掉 `--skip`）。跑完檔案都還原了。

**沒有測試守著的**：glyph 的 culling 省了多少沒有計數（離 tile 遠的 glyph 有沒有真的被濾掉，測試只看得到「該畫的有畫」）。2d 記進測試缺口的文件；要守的話是在 playback 加一個測試讀得到的計數。

**其他檢查**：`cargo clippy -p raster --all-targets -- -D warnings`、`cargo fmt -p raster --check`、`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs 都過。`boundary` 的清單拿掉了原型的兩個 thread-local。reftest、情境和 WPT 這一段沒有重跑：畫圖的程式和 1b 最後跑整套時（對齊開在 4）是同一份，只拿掉開關；2d 整個階段做完時再跑。

### 第 2b 步：ink bounds 改用 skrifa、mipmap、拿掉 skia-safe（2026-11-11，Windows 11，AMD Radeon RX 7900 XTX）

**做了什麼**：

- **ink bounds**（`crates/raster/src/ink.rs`，已決定的事項第 35、36 條）：skrifa 取外框線，算控制點的範圍；`raster::ink_bounds` 不再拿快取。假粗體和畫的時候一樣用 `kurbo::expand_path`（加多少抽成 `embolden_side`，畫和量都呼叫），在字型的座標（y 朝上）裡加粗、再翻轉和歪斜，和 Vello 的順序相同。點陣的 glyph 照 Vello 擺點陣圖的算法算矩形。
- **外框線用字型的單位取、自己縮放**：skrifa 把縮放過的外框線對齊到 1/64 px，13 px 的 Ahem 會差 0.006 px，1.5 px 的字差得更多（WPT 有 `actualBoundingBox.small-font`）。改成取沒縮放的再乘上字級之後，和 Skia 的數字相同。這一點是實作時發現的，沒有另外問。
- **mipmap**（第 34 條）：圖片快取的每一項多了 16 個層的位置，第一次用到時從原圖做面積平均。`DrawImage` 和圖案 shader 的圖片照「看到的大小」（目前的 matrix 乘上 dst 對 src 的比）取層。
- **Skia 拿掉了**：`skia_backend.rs` 刪除，`raster` 不再依賴 `skia-safe` 和 `read-fonts`，workspace 的 `Cargo.toml` 拿掉 `skia-safe`。`RasterCache` 搬到 `lib.rs`，只包著 Vello 的快取；它的 `Debug` 從 `typefaces` 改成 `fonts`。

**和 Skia 的對照**（拿掉之前，同一個 glyph 兩邊各算一次，位置 (3.25, -1.5)；Ahem 278 個、Lato 3,023 個、RobotoExtremo 子集 4 個、Noto Color Emoji 子集 7 個 glyph）：

| 情況 | 四邊最大的差（px） |
| --- | --- |
| 不加粗（13、7.3、40.5 px；歪斜 20°；`wght` 100 和 900）：Ahem、RobotoExtremo | 0.0000 |
| 同上：Lato | 0.034（glyph 1259，組合的 glyph；其餘更小） |
| 假粗體，13 到 40.5 px | 0.06（Ahem）到 0.21（Lato） |
| 假粗體，100 px | 0.53（Lato 的 glyph 238） |
| 點陣的 emoji | 0.34 到 0.97 |

- 有墨水的 glyph 兩邊一致：沒有一邊有、另一邊沒有的。
- **假粗體的差是預期中的**：Skia 用它自己的加粗，Vello 畫的時候用 `kurbo::expand_path`，量的要跟著畫的。`ink_bounds_are_where_glyphs_are_drawn` 拿畫出來的像素對過（8 組字、7 種字級和粗斜的組合，ink 包住每個畫到的 pixel、最多多出 1.5 px）。
- **emoji 的差**：Skia 回的是取整到整數 pixel 的範圍，這裡是點陣圖實際畫的矩形。
- Lato 那 0.034 px 沒有追原因（猜是組合 glyph 的位移在兩邊捨入不同）。

**拿 Skia 當對照的測試怎麼處理**：

- `gfx::Path` 的弧、圓角矩形、transform 原本逐位和 `SkPathBuilder` 比（4 個測試）。gfx 自己沒有這些形狀的測試，直接刪掉就沒人守了：拿掉 Skia 之前把 108 個情況的輸出存成 `crates/gfx/src/path_shapes.golden`，新的 `shapes_are_skias_bit_for_bit` 逐位比它。之後新加的情況沒有 Skia 可以對。
- 「每種顏色配每種 alpha 轉成不 premultiplied 和 Skia 相同」改成比兩個 FNV-1a 的 hash（拿掉之前算的）。
- `gfx::Path` 轉成 Skia 的 path、collection 拆成單獨的字型、Skia 的 typeface 快取的測試跟著程式一起刪了。「collection 的第二個 face 畫得出來」留著，改成只經過 playback；另外加了量的。

**改了預期值的兩個測試**（都是 1b 就知道的差 1，不是這一步造成的）：

- `renderer` 的 `a_flush_sends_the_recording_to_the_thread_and_forgets_it`：半透明的紅疊兩次，192 改成 191。算出來是 191.75，Vello 的 8-bit 混色往下取。
- `browser` 的 `navigation::links::canvas_2d`：線性漸層中間和尾端的兩個 pixel，`126,0,129` 和 `1,0,254` 改成 `127,0,128` 和 `2,0,253`。

這兩個都是 script 用 `getImageData` 讀得到的差異，2d 記進缺口的文件。

**跑過的**：

- `cargo test -p raster`（92 個）、`-p gfx`（23 個）、`-p renderer`（111 個）、`-p canvas`、`-p layout`、`-p browser --test it canvas`：都過。
- `test.py reftest images`：17 個都和預期一樣。
- `test.py wpt html/canvas/element/text`：315 個 check 都和預期一樣（`actualBoundingBox` 的三個結果沒變）。
- `test.py wpt html/semantics/embedded-content/the-img-element`（1,983 個 check）、`css/css-images`（744 個）：都和預期一樣。`css/css-backgrounds`（6,223 個）有 2 個不通過：`box-shadow-041.html`、`box-shadow-042.html`，是 1b 就列出的那 8 個裡的，2d 處理。
- `cargo clippy`（`raster`、`gfx`、`renderer`、`layout`、`browser`）、`cargo fmt --check`、`sanity.py` 的 deps、boundary、unsafe、tests、numerics、docs：都過。

**Mutation test**（`mutants-2b.toml`，22 條，21 條抓到）：

| 弄壞了什麼 | 哪個測試失敗 |
| --- | --- |
| ink 不加粗；粗兩倍；斜的方向反了 | `ink_bounds_are_made_bold_and_slanted`、`bold_is_wider_by_the_size`、`ink_bounds_are_where_glyphs_are_drawn` |
| 控制點不算進範圍 | `bounds_are_the_points_control_points_too` |
| 外框線不照字級縮放；variation 不套用；每個 glyph 都在原點；collection 一律用第一個 face | `ink_bounds_are_the_glyphs_outlines_together`、`a_variable_font_is_measured_at_its_axes`、`a_collections_face_is_measured_and_no_font_has_no_ink` |
| 點陣的 glyph 沒有墨水；點陣圖不照字級縮放 | `a_bitmap_glyph_has_the_rect_it_is_drawn_in`、`a_bitmap_glyphs_ink_is_where_it_is_drawn` |
| 圖一律從原圖畫；canvas 的圖（`Low`）也取層 | `an_image_made_much_smaller_is_its_pixels_averaged`、`a_level_is_made_once_and_only_when_drawn_from`、`a_decoded_image_is_drawn_as_its_pixels_sampled_as_its_filter_says` |
| 層的邊長往下取整；一邊夠大就算夠大；取小一層 | `an_image_is_drawn_from_the_smallest_level_that_is_large_enough` |
| 來源範圍沒有換算到層；圖案的圖不取層；取層只看畫的大小、不看 matrix | `a_part_and_a_pattern_of_an_image_made_smaller_are_from_its_level`、`an_image_made_much_smaller_is_its_pixels_averaged` |
| 縮小時兩端的 pixel 算整個 | `a_smaller_level_averages_what_each_pixel_covers` |
| 橢圓的四分之一弧的權重改掉 | `shapes_are_skias_bit_for_bit` |
| 轉回不 premultiplied 時四捨五入 | `unpremultiplying_is_skias_for_every_color_and_alpha` |

- 第一輪有 3 條沒抓到，其中 2 條是測試寫得不好：圖剛好縮成四分之一時，bilinear 取樣點落在四個 pixel 的交點，本來就是平均值，分不出有沒有取層。測試改成縮成三分之一（和 5 px 的 tile）之後抓到。
- **沒抓到的 1 條**：點陣 glyph 的外側 bearing 正負號反了。手上的測試字型（Noto Color Emoji，CBDT）那個值是 0，要 sbix 的字型才測得到；Apple Color Emoji 專用的那個位移同樣沒有測試（沒有可以放進 repo 的字型）。2d 記進測試缺口的文件。
- 跑完檔案都還原了。

**沒有做的**：沒有外框線的 COLR glyph 當成沒有墨水（第 35 條），沒有測試；2d 記成缺口。
