# RFC 0007 的進度：用 vello_cpu 取代 Skia

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-10，Windows 11。在 `0007` 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0 圖片的解碼和讀尺寸、JPEG 的編碼換成純 Rust 的 crate（Skia 照舊畫） | 完成，還沒 merge | 2026-11-10 |
| 1a 原型：常用的 op；量文字多的 tile、存截圖（第一個檢查點） | 進行中 | |
| 1b 原型：全部的 op 和組出來的功能；整套 reftest（第二個檢查點） | 還沒開始 | |
| 2 playback 換成 vello_cpu、ink bounds 改用 skrifa、拿掉 skia-safe、加上列出缺的功能的文件 | 還沒開始 | |
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
