# raster 的函式庫缺什麼

`raster` 用 vello_cpu（`../third_party/vello`，我們的 fork，緊跟上游的 main）把 display list 畫成像素。這份列出 Vello（和它底下的 kurbo、glifo）沒有、做得不一樣或做錯，而 machopo 的 display list 用得到的每一項：補在 playback 裡、使用者看不出來的也列。只放**目前仍然成立**的：上游補上、machopo 拿掉補的程式時，刪掉那一列。使用者看得到的結果在 `docs/limitations.md` 的「Paint 和 raster」各有一句，指回這裡。

規則：

- **補的地方，註解以 `vello:` 開頭**，寫缺什麼或哪裡錯，有上游的 issue 就寫上（`grep -n "vello:" crates/raster/src` 列得出全部）。註解不引用這份文件；方向是這份指向程式。
- **Vello 碰到會 panic 或畫錯、而且有 API 可以直接呼叫的缺口，各配一個「上游還缺」的測試**（`vello_backend.rs` 的 `upstream_*`）：直接把那個東西交給 Vello，斷言它現在的行為。升級 Vello 時測試失敗，就表示上游改了，下表寫著可以拿掉哪一段。
- **API 根本不存在的缺口測不了**：升級 Vello 時逐列對一次下面第一張表。
- **加新的繪圖功能前先看這份**：Vello 有沒有；沒有的話看現在同類的東西怎麼補。
- 要回報上游的，先在本地寫好草稿給人審，不直接開 issue 或 PR。

上游的狀態是 2026-11-10 對照 `linebender/vello` 的 main 查的。

## 補在 playback 裡的

使用者看不出來；代價是速度，只在用到的頁面上。「補在哪」是 `crates/raster/src/vello_backend.rs` 裡的函式。

| 缺的 | 上游的狀態 | machopo 哪裡用到 | 補在哪、怎麼補 | 「上游還缺」的測試 | 什麼時候可以拿掉 |
| --- | --- | --- | --- | --- | --- |
| color matrix 的 filter | 碰到就 panic（`unimplemented!`）。`linebender/vello#1296` 追蹤，PR `#1765` 在做 | CSS `filter` 的 `brightness`、`contrast`、`grayscale`、`sepia`、`saturate`、`hue-rotate`、`invert` | `filtered`：layer 畫到旁邊的 pixmap，自己對像素套矩陣 | `upstream_lacks_color_matrix_filters` | 上游合了之後，單一的 color matrix 直接交給 Vello |
| 多個 primitive 的 filter | 碰到就 panic。`linebender/vello#1296` 只有清單 | 一個 `filter` 裡好幾個 function（`ImageFilter::Chain`） | `filtered`：一個一個套，每個拿前一個的像素 | `upstream_lacks_filters_of_several_primitives` | 上游能執行 filter graph |
| filter 只看得到 context 裡面的東西 | 設計如此 | tile 邊緣的 blur 和陰影：隔壁 tile 的內容也要算進來 | `filter_layer`：layer 畫在比 tile 大的 context 上（多出 filter 伸得到的範圍），再裁回來 | 沒有（不是 API 的缺） | 不會拿掉 |
| paint 上的 blur（Skia 的 mask filter） | 沒有；只有模糊的圓角矩形 | canvas 的陰影、`DrawShadow` 以外模糊的形狀 | `draw_blurred`：不模糊地畫進 blur 的 filter layer | 沒有 | 不會拿掉 |
| 模糊的圓角矩形：四個角各自的橢圓半徑、中間挖洞 | 只有一個圓形的半徑。`linebender/vello#1245` 開著 | `box-shadow`（`DrawShadow`：四個角各自的半徑；inset 和不畫在盒子底下的陰影要挖洞） | `draw_shadow`：四個角相同、是圓的、不挖洞時用 Vello 的；其他的畫形狀、包 blur 的 filter layer | 沒有 | 上游支援每個角的半徑（挖洞的還是要自己來） |
| path 的聯集 | kurbo 和 Vello 都沒有布林運算 | `ClipPaths`（好幾條 path 合起來當 clip） | `draw`：每條 path 填進和 context 一樣大的 alpha mask，後面的東西經過它畫 | 沒有 | 不會拿掉（除非 kurbo 有布林運算） |
| 圖案：圖塊是 display list、圖塊之間有間隔、某一軸只畫一塊 | 圖片的 paint 只有 `Extend`（pad、repeat、reflect） | `PaintShader::Tiled`：背景圖的 `background-repeat: space`、`no-repeat`，SVG 圖片當背景，canvas 的 pattern | `set_tiled`：圖塊（連同間隔）先畫成 pixmap；只畫一塊的那一軸用 clip 切 | 沒有 | 不會拿掉 |
| mipmap | 沒有；縮小超過一半時只取四個像素。`linebender/vello#1289` 開著 | `<img>` 和背景圖縮小（`FilterQuality::Medium`、`High`） | `VelloCache::image_at`、`shrunk`：圖片的快取裡照 cc 的規則取一層預先縮好的（面積平均），第一次用到時才做 | 沒有 | 上游的圖片取樣支援 mipmap |
| 結尾硬切的漸層，超過終點的顏色 | **bug**：最後兩個色標都在 offset 1、顏色不同時，超過終點的地方用兩個裡的第一個。還沒回報 | `linear-gradient(blue 0% 50px, green 50px)` 這類結尾硬切、漸層線比盒子短的 | `gradient`：這種漸層拉長一倍、色標的 offset 減半（只做不重複的 linear 和 radial） | `upstream_pads_a_gradient_past_a_hard_stop_with_the_color_before_it` | 上游修好 |
| 從 context 右邊緣開始、被 clip 的 stroke | **bug**：clip 的交集算出負的寬度（`vello_common` 的 `clip.rs`）：有溢位檢查時 panic，沒有時寬度是錯的。還沒回報 | 邊框的一邊剛好貼著 tile 的邊界時，隔壁的 tile | `Player::shows`：整個在 context 外面的 clip 不交給 Vello，它底下的東西也不畫 | `upstream_panics_on_a_clipped_stroke_at_the_right_edge`（`playback_tests.rs` 的 `a_panic_of_vellos_is_caught` 也靠它） | 上游修好（`shows` 省掉的工作可以留著） |
| 場景的 blend mode 只看得到場景裡的東西 | 設計如此：render 到已經有內容的像素上時，blend mode 不和那些像素混 | canvas：每次 flush 畫在已有的 bitmap 上 | `seed_canvas`、`render_canvas`：先把 bitmap 當成場景的第一樣東西畫進去；`putImageData` 之前先 render 一次 | 沒有 | 不會拿掉 |
| context 外面的 glyph 和裡面的一樣貴 | Vello 先把 glyph 的外框線攤平，才知道它在外面 | 一段文字碰到的每個 tile 都收到整段 | `draw_glyphs`：離 tile 太遠、碰不到它的 glyph 不交給 Vello | 沒有 | 上游先用範圍濾掉 |
| glyph 畫在哪就是哪 | 設計如此（Skia 的 glyph 快取把位置對齊） | 測試頁和參考頁同一段文字的位置差了浮點數的誤差時，邊緣的像素差 1 | `draw_glyphs`：matrix 只有縮放和位移時，每個 glyph 的原點沿著行對齊到四分之一個 device pixel、垂直方向對齊到整數 | 沒有 | 不會拿掉 |

## 沒有補的

使用者看得到，或 script、測試讀得到。

| 差在哪 | 上游的狀態 | 看得出來嗎 | 之後怎麼處理 |
| --- | --- | --- | --- |
| 文字沒有 gamma 和對比的調整 | `linebender/vello#1694` 開著 | **看得出來**：字比較細、淡，深色背景上的淺色字最明顯 | 在 playback 調整、做進 fork，或接受：還沒定 |
| 漸層沒有 dithering | `linebender/vello#856` 開著 | 大面積、顏色相近的漸層可能有色帶 | 同上 |
| 會清掉形狀以外的 blend mode（`source-in`、`source-out`、`destination-in`、`destination-atop`）不受 clip 限制 | layer 用這種 blend mode 疊回去時，作用在整個 context 上，不只在它所在的 clip 裡。還沒回報。`playback_tests.rs` 的 `upstream_doesnt_keep_a_clearing_layer_to_its_clip` 記著現在的行為，上游改了它會失敗 | canvas 上這四種模式配落在畫布裡的 `clip()` 時，clip 外面也被清掉。clip 整個在畫布外面時沒事（那一層根本不畫）；`destination-out`（橡皮擦）不受影響 | 回報上游；或在 playback 把目前的 clip 當成 mask 套在疊回去的那一步 |
| 線寬遠大於線段長度時，bevel 接合的內側多畫一塊 | kurbo 的 stroker。還沒回報 | 很少見（WPT 的 `2d.line.cross`：線寬 200、兩段各 10） | 回報上游 |
| 8-bit 的混色用整數算，捨入和 Skia 不同 | 設計如此（f32 的管線準，但每個 tile 慢三到八成，不用） | 看不出來；`getImageData` 讀得到差 1（半透明的顏色疊兩次、漸層中間的顏色） | 接受 |
| 同一塊內容切成 tile 畫和整塊畫，邊緣和模糊的範圍最多差 1 | 設計如此：覆蓋率用浮點數算，從不同的原點算捨入不同 | 看不出來；scale 是 1 的形狀、文字和陰影逐位相同 | 接受 |
| 邊長超過 65,535 的像素不畫 | context 的邊長是 `u16` | tile 不會這麼大；這麼大的 canvas 畫不出東西（記一筆 log） | 需要時分塊畫 |
| 沒有外框線的 COLR glyph 沒有 ink bounds | 和 Vello 無關：`raster` 的 `ink_bounds` 只量外框線和點陣圖 | `measureText` 的 `actualBoundingBox*` 對這種 glyph 是 0 | 需要時照 COLR 的 clip box 或 paint 的範圍量 |
| release 沒有整數溢位的檢查 | Vello 裡的溢位在 release 算出錯的值繼續跑，不 panic，接不到 | 已知的那一個（上面的 clip）擋掉了；其他的不知道 | 只對 Vello 的 crate 打開溢位檢查：慢多少還沒量 |

## 只在速度上看得出來的

- **沒有 glyph 的快取**：Vello 的 atlas 快取還是實驗性的，沒有開；每個 glyph 每次都從外框線畫。
- **x86 的速度**：上游說 x86 還有改進的空間。
- **離屏的每一項**（上面第一張表裡「畫到旁邊」的）每個 tile 都配置新的 context 和 pixmap。
