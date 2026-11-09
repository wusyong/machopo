# RFC 0033 的進度：Canvas 2D

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-11。完成：2026-10-12，Windows 11（Rust 1.99.0）。四段都在 `0033` 上做；第 1 到 3 段做完時 commit、審核、rebase 到 `main` 再 merge，第 4 段和整份 RFC 的收尾一起。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 元素和 layout：`HTMLCanvasElement`（`width`、`height`、`getContext`）、replaced element、fallback content 不排版；JS 和 wasm（畫布和狀態的重設在 1b） | 完成 | 2026-10-11 |
| 1b context：`width` 或 `height` 改了時重設畫布和狀態、狀態、transform、`globalAlpha`、純色的 `fillStyle` / `strokeStyle`、`lineWidth`、`fillRect` / `strokeRect` / `clearRect`，錄成 op；像素和大小的上限、`raster` 畫進既有的像素、rendering update 時畫、`getImageData` 和 `ImageData`（經過 `LayoutClient`）；JS 和 wasm | 完成 | 2026-10-11 |
| 1c 畫進頁面：canvas 的 display item、快照當成解碼好的圖 | 完成 | 2026-10-11 |
| 1d `TextureLayer`：layerize 決定 canvas 成為 layer、compositor 的快照和版本、Viz 的 texture quad（雙線性）、`alpha: false` 的不透明；是 layer 的 canvas 畫過時直接換快照 | 完成 | 2026-10-11 |
| 1e 路徑和線的樣式（等 SVG 那邊的路徑型別進 `main`，rebase 之後做）：路徑的 API（`moveTo` 到 `roundRect`，`arc` 和 `ellipse` 的 conic）、`fill` / `stroke` / `clip`、`lineCap`、`lineJoin`、`miterLimit`、`setLineDash`；缺的 op 和 flags 補上 | 完成 | 2026-10-11 |
| 1f WPT、reftest、情境和 `about:demo`、文件 | 完成 | 2026-10-11 |
| 2a `font`（解析、照元素計算、序列化）和文字的狀態（`textAlign`、`textBaseline`、`direction`、`letterSpacing`、`wordSpacing`、`fontKerning`、`fontStretch`、`fontVariantCaps`、`textRendering`），`save` / `restore` 和重設；JS 和 wasm | 完成 | 2026-10-11 |
| 2b `fillText`、`strokeText`：經過 client 排版、錄成 `DrawGlyphs`，對齊、基準線、`maxWidth`、bidi | 完成 | 2026-10-11 |
| 2c `measureText` 和 `TextMetrics` | 完成 | 2026-10-11 |
| 2d WPT 的 `text/`、reftest、情境和 `about:demo`、Speedometer 的盤點、文件 | 完成 | 2026-10-11 |
| 3a `CanvasGradient`（線性、放射、圓錐），`fillStyle` / `strokeStyle` 可以是物件 | 完成 | 2026-10-11 |
| 3b `drawImage`（`<img>`、`<canvas>`）、`CanvasPattern`、`imageSmoothingEnabled` / `Quality`、origin-clean；宣告表的多載 | 完成 | 2026-10-11 |
| 3c `globalCompositeOperation`、陰影 | 完成 | 2026-10-11 |
| 3d `Path2D`、`isPointInPath` / `isPointInStroke` | 完成 | 2026-10-11 |
| 3e `putImageData`、`createImageData` | 完成 | 2026-10-11 |
| 3f WPT、reftest、情境和 `about:demo`、文件 | 完成 | 2026-10-12 |
| 4a `toDataURL`：`raster` 的 PNG 和 JPEG 編碼器（照 Chrome 的設定），經過 `LayoutClient`；格式和品質、origin-clean、大小是 0；JS 和 wasm | 完成 | 2026-10-12 |
| 4b `toBlob`：呼叫時編碼、task 裡呼叫 callback、Blob；只給 JS | 完成 | 2026-10-12 |
| 4c WPT 的 `the-canvas-element/` 的 `toDataURL` / `toBlob`、情境和 `about:demo`、文件，整份 RFC 的收尾 | 完成 | 2026-10-12 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **元素和 layout** | |
| `width`、`height` 照 attribute（HTML 的非負整數，預設 300、150，超過 `long` 是預設值）；改了 `width` 或 `height`（IDL 或 `setAttribute`）canvas 重新排版，別的 attribute、別的元素不會 | `cargo test -p dom_api --test it canvas::`、`cargo test -p dom --lib canvas` |
| `getContext("2d")` 第一次建 context、之後給同一個（之後的 options 不改它），其他 id 是 `null`；context 的 `canvas`；`getContextAttributes()` 是建立時的設定 | `cargo test -p dom_api --test it canvas::` |
| context 是 GC 型別：只當自己的型別讀回、slot 撐住它、它撐住它的元素、元素的 rare data 撐住它；工作負載裡經過配置和 GC 還是同一個 | `cargo test -p dom --test it script_objects::canvas_rendering_context_2d`、`cargo test -p dom --test it script::every_type_unwraps_only_as_itself`、`cargo test -p dom --test it` |
| JS：interface 和 prototype、不能建構、`getContext.length`、reflection、同一個 context、其他 id 是 `null`、`getContextAttributes()` 每次一個新的一般物件、成員照名稱排序、不是 enum 的值是 `TypeError` | `cargo test -p dom_js --test it canvas::` |
| wasm：`html-canvas-element` 的 `width`、`get-context`（settings record，別的 id 是 `none`）、context 的 `canvas`，`get-context-attributes` 回傳 record（宣告表第一個回傳字典的成員）；Lua 的 table 沒寫 `alpha` 時是 `true`、回傳的 record 的欄位是 snake_case | `cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p shell --test it wasm_reftests::w9_lua` |
| WIT 的名稱裡數字後面的大寫字母留在同一個字（`canvas-rendering-context2d`），`uint8-array` 不變 | `cargo test -p dom_api --test it wit::` |
| layout：`<canvas>` 是 replaced element，自然大小是畫布的大小（預設 300×150、attribute 不是 presentational hint、CSS 一邊的大小照比例、0 寬的沒有比例），fallback content 不建 box；新的大小重建它的 box | `cargo test -p layout --test it canvas` |
| **context、狀態、矩形和像素** | |
| 每個畫的呼叫錄成 op，前面是它的 `SetMatrix`（同一個 matrix 不重複），顏色乘上 `globalAlpha`；負的大小往回、空的矩形和不是有限的數什麼都不錄；`strokeRect` 沒有寬或高時是一條線；`clearRect` 是 `ClearRect`，不透明的 canvas 是黑色的 `DrawRect` | `cargo test -p dom_api --test it canvas_2d::` |
| 狀態的堆疊（`save`、`restore` 停在最底、`reset`）、transform 的乘法和不是有限的數、屬性不收的值（`globalAlpha`、`lineWidth`、不是顏色的字串）、顏色的序列化、`currentcolor` 是設定時 inline style 的 `color`（不 connected 是黑色） | `cargo test -p dom_api --test it canvas_2d::`、`cargo test -p style --lib canvas` |
| 新的大小重設 context（狀態、錄下的 op、像素）；不能有像素的大小（Chromium 的上限）什麼都不錄、讀回透明；錄到 10,000 個 op 就經過 client 先畫 | `cargo test -p dom_api --test it canvas_2d::` |
| `getImageData` 先經過 client 畫，讀 bitmap、轉回不是 premultiplied（四捨五入），bitmap 外是透明、負的大小往回，不透明的 canvas 什麼都沒畫時是黑色，寬或高是 0 是 `IndexSizeError`；`new ImageData` 的預設色彩空間、`IndexSizeError`、`RangeError` | `cargo test -p dom_api --test it canvas_2d::`、`cargo test -p dom_api --lib canvas` |
| renderer 把錄下的 op 畫進 bitmap（第一次配置、接著畫在已有的像素上、不透明的從黑開始、不能有像素的丟掉錄下的）；rendering update 只畫顯示的文件裡 connected 的 canvas，其他的留著；`raster` 的 `draw_into` 和 `ClearRect` | `cargo test -p renderer --lib canvas::`、`cargo test -p raster --lib` |
| 真的畫：填色疊上半透明、`clearRect` 不管 `globalAlpha`、`save` / `restore` 和 transform、`rotate`、`strokeRect` 的外框、不透明的 canvas、新的寬度清掉 bitmap 和狀態、bitmap 外和負的大小、`data` 是同一個陣列且寫進去的留著 | `cargo test -p browser --test it canvas_2d` |
| JS：狀態的預設值和不收的值、成員的 `length`、`ImageData` 的 `data` 經過 GC 還是同一個、留著寫進去的值、`getImageData` 的色彩空間；wasm：`fill-style`、`fill-rect`、`get-image-data` 回傳 `image-data`、`data` 是 guest 的 `typed-array`；Lua 的 wasm 文件裡填色真的畫出來、讀回 `typed-array` 的 table | `cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p shell --test it wasm_reftests::w9_lua` |
| **畫進頁面和 `TextureLayer`** | |
| canvas 的 bitmap 畫進頁面：填色、清掉的地方露出頁面、transform、外框；在 border 和 padding 裡的內容框；不透明的沒畫的地方是黑色；什麼都沒畫的透明；新的寬度清掉畫過的；沒有讀像素也畫 | `python3 scripts/test.py reftest canvas/` |
| paint 把快照畫進內容框、取樣是沒有 mipmap 的雙線性、自己一個 chunk（前後的 item 在別的 chunk）、沒有快照就沒有 item、`border-radius` 切掉；layout 標出 canvas 的 box | `cargo test -p paint --test it canvas` |
| 快照在像素沒變時是同一個、畫過之後是新的、重設之後沒有；不透明的沒畫時是黑色；`gfx::ImageData` 的解碼好的像素；raster 直接畫它，`Low` 和 `Medium` 的取樣不同 | `cargo test -p dom --lib canvas`、`cargo test -p gfx`、`cargo test -p raster --lib` |
| 在視窗裡：`about:demo` 的 canvas，點它加一個方塊、「Add one later」從 idle callback 加一個、「Read a pixel」讀回第一個方塊 | `python3 scripts/test.py scenarios canvas` |
| canvas 到最上層只有平移、沒有 opacity、至多一個圓角 clip 時自己是一個 layer（前後的 chunk 在別的 layer），在它自己的 transform 和 clip 底下，大小是它的 item；`scale`、skew、opacity、兩層圓角 clip 時畫進頁面；在成為 layer 的捲動容器裡跟著捲 | `cargo test -p paint --test it layerize::` |
| `TextureLayer` 是 canvas 現在的快照（不是 paint 時的）、不透明的設定、layer 的 id 在它一直是 layer 時不變；沒有東西可顯示時沒有 layer | `cargo test -p renderer --lib compositing` |
| 是 layer 的 canvas 畫過時只換 layer 的快照、不重新 paint；它的 item 在下一次 paint 時重畫（transform 從平移變成 `scale` 時，box 沒變也一樣）；reset 之後沒有 layer | `cargo test -p renderer --lib frame_view::tests::a_canvas_layer` |
| compositor：texture layer 是一個 quad，大小照 device scale factor，雙線性取樣，tile 取最近的 texel；同一份快照只放上 GPU 一次、新的快照是新的 resource，damage 只有那個 layer；GPU 遺失之後放上新的 GPU；Viz：`SoftwareRenderer` 的雙線性取樣（邊緣延伸）、`SurfaceAggregator` 照樣傳下去 | `cargo test -p compositor --lib texture`、`cargo test -p viz --lib` |
| 在頁面上：CSS 放大的 bitmap、之後的 frame 再畫、opacity 底下、捲動容器裡捲過且切掉；2x 時在小數位置的 canvas 照 device pixel 對齊 | `python3 scripts/test.py reftest canvas/ hidpi/` |
| **路徑和線的樣式** | |
| `gfx::Path` 的橢圓弧、切線的弧、矩形、橢圓和圓角矩形（radii 照 Skia 縮）、transform，和 Skia 的 `SkPathBuilder` 做出來的動詞、點和 conic 的權重一樣（逐位元比對） | `cargo test -p raster --lib path_shape` |
| 路徑的方法照 Blink：空路徑上的 `lineTo` 是長度 0 的線、`closePath` 清掉它，沒有點時曲線從控制點開始，`rect`、`arc`（半徑 0、一圈以上、逆時針）、`ellipse`（旋轉、扁的是直線）、`arcTo`（沒有點、`closePath` 之後）、`roundRect`（radii 的分配、點、負的大小、最後的 `moveTo`、`RangeError`），不是有限的數不加，負的半徑是 `IndexSizeError` | `cargo test -p dom_api --test it canvas_paths::` |
| 路徑在目前的 transform 的座標裡，換 transform、`restore` 時換算；transform 不可逆時點換算成 bitmap 的座標、形狀和 `translate` 不做、什麼都不畫，`resetTransform` 回到原本的 | `cargo test -p dom_api --test it canvas_paths::` |
| `fill`（fill rule）、`stroke`（線的樣式、虛線、anti-alias）錄成 `DrawPath`，一個點的路徑不畫；線的樣式的預設值和不收的值；`strokeRect` 用線的樣式，只有寬或高時是閉合的來回 | `cargo test -p dom_api --test it canvas_paths::`、`cargo test -p dom_api --test it canvas_2d::` |
| `clip` 記在它那一層（bitmap 的座標，`save` 不複製），錄製中時照呼叫錄，每次開始錄製時重播每一層的 `Save` 和 clip，`restore` 之後重設 matrix | `cargo test -p dom_api --test it canvas_paths::` |
| JS 和 wasm：`roundRect` 的 radii 是數字、點或陣列，`lineCap` / `lineJoin` 的 enum，`setLineDash` / `getLineDash` 的陣列；C 的 guest 和 Lua 的 wasm 文件畫路徑 | `cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p shell --test it wasm_reftests::w9_lua` |
| 真的畫：三角形、even-odd、圓、跨過讀像素還在的 clip 和 `restore`、虛線、圓角、方的端點；在頁面上和 div、`border-radius` 逐像素一樣 | `cargo test -p browser --test it canvas_2d`、`python3 scripts/test.py reftest canvas/` |
| **文字** | |
| canvas 的 `font` 照 Chromium 的形式讀回（pt 換成 px、style 在 weight 前、`oblique` 不寫、沒有 line height、只有一個 identifier 的名稱不加引號），相對的單位照 canvas（`em`、`%`、`larger`、`bolder`）和根元素（`rem`）；只收 `font` shorthand（CSS-wide keyword、`var()`、system font 不收）；canvas 的收 `small-caps` 和 stretch 的 keyword，頁面的不收；spacing 只收一個帶單位的長度 | `cargo test -p style --lib canvas` |
| `font` 在設定時照 canvas 的 computed style 算（沒有 style 時照 10px sans-serif），文件沒有 frame 時、同一個字串時不收；其他文字屬性的預設值和不收的值，`fontStretch` / `fontVariantCaps` 和 `font` 共用；`save` / `restore`、`reset()`、新的寬度 | `cargo test -p dom_api --test it canvas_text::` |
| JS（enum 不認得的字串不改值）、wasm（`font` 的字串、`text-align` 的 enum） | `cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas` |
| 一行的排版：從左端、在 alphabetic baseline 上，寬度；spacing、RTL 的順序、kerning 可以關掉；字型的度量（hhea、OS/2 正規化成 1em、BASE 的 baseline，沒有字時也有） | `cargo test -p layout --lib canvas_text` |
| `fillText` / `strokeText`：ASCII 的空白換成空格（U+000B 不換）、不合併；照狀態的字型、方向、spacing（`em` 照畫的時候的字型）和 kerning 排；`textAlign`（`start` / `end` 照方向）、`textBaseline`（BASE 的優先，沒有時 80% ascent、−descent、0）；比 `maxWidth` 寬時是 transform 底下的水平縮放，之後的 op 回到原本的 transform；`strokeText` 用線的樣式，填色乘上 `globalAlpha`；x、y 不是有限的數、`maxWidth` 不是有限的數或 ≤ 0、transform 不可逆、文件沒有 frame、client 不排字時不畫 | `cargo test -p dom_api --test it canvas_text::` |
| `direction` 的 `inherit` 照 HTML 的 directionality：`dir`（不分大小寫，只有 HTML 元素的）、不認得的值繼承、`auto` 和沒有 `dir` 的 `bdi` 取第一個有方向的字（L、R、AL），跳過 `bdi`、`script`、`style`、`textarea` 和有自己 `dir` 的元素，巢狀的元素結束後接著找，沒有時是 ltr | `cargo test -p dom_api --test it canvas_text::` |
| renderer 排字、所有 canvas 共用的 `RasterCache` 留下用過的 typeface；真的畫（系統字型）：從 x 開始、靠右、擠進 `maxWidth`、NaN 不畫、描邊 | `cargo test -p renderer --lib canvas::`、`cargo test -p browser --test it canvas_2d` |
| JS（`fillText.length` 是 3、`maxWidth` 可以省略、少了參數是 `TypeError`）、wasm（`option<f64>` 的 `max-width`，Lua 的 runtime 也收） | `cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p dom_api --test it wit::` |
| `measureText` 要 ink、`fillText` 不要；`width`、`actualBoundingBox*`（照 `textAlign` 和方向的對齊點、照 `textBaseline` 的線）、`fontBoundingBox*`（hhea）、三條 baseline（BASE 的優先）；每次一個新的 `TextMetrics`、文字照畫的時候前處理；文件沒有 frame、client 不排字時全部是 0 | `cargo test -p dom_api --test it canvas_text::` |
| ink 是 glyph 輪廓的 bounds 的聯集（13px 的 Ahem 不取整），每個 glyph 在自己的位置，沒有墨跡的不算、都沒有時是原點的空矩形，合成的粗體和斜體算進去 | `cargo test -p raster --lib ink_bounds` |
| `TextMetrics` 是 GC 型別（只當自己的型別讀回、slot 撐住它、數字留著）；JS（`instanceof`、不能建構、沒有 renderer 時是 0）、wasm（`measure-text` 回傳 `text-metrics`）；真的量（系統字型）：ink 的左右邊和畫出來的像素差不到 1px | `cargo test -p dom --test it script_objects::text_metrics`、`cargo test -p dom --test it script::`、`cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p browser --test it canvas_2d` |
| **漸層、圖片、圖案和 origin-clean** | |
| 漸層的建立和檢查（負的半徑、範圍外的 offset 是 `IndexSizeError`，不是顏色是 `SyntaxError`）；`fillStyle` / `strokeStyle` 讀回同一個漸層，經過 `save` / `restore` / `reset`，不是顏色的字串不改；畫的時候的 shader：stop 照 offset 排、同一個 offset 照加入的順序、頭尾補到 0 和 1、沒有 stop 是透明黑，之後加的 stop 也算，沒有長度的是空的 shader，圓錐的是轉過起始角度的 sweep，放射的是 two-point conical，`globalAlpha` 是 paint 的 alpha，描邊也用 | `cargo test -p dom_api --test it canvas_styles::` |
| `CanvasGradient` 是 GC 型別；context 每一層的 style 撐住它的漸層，`save` 之後下一層也撐住，pop、改成顏色、重設之後放掉 | `cargo test -p dom --test it script_objects::canvas_gradient`、`cargo test -p dom --test it script_objects::a_contexts_styles_keep_their_gradients`、`cargo test -p dom --test it script::` |
| raster 的 sweep（從 x 軸順時針、經過 transform）和空的 shader；真的畫：線性和圓錐的漸層；JS（`instanceof`、不能建構、`fillStyle === g`、`{}` 不改、錯誤的種類）、wasm（`fill-style` 和 `stroke-style` 的兩個 variant） | `cargo test -p raster --lib sweep`、`cargo test -p browser --test it canvas_2d`、`cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas` |
| `drawImage` 的三種形式：來源和目的的矩形（負的大小正規化、切到圖片裡、目的跟著等比例切）、`<img>` 的密度、`imageSmoothing*` 的取樣、`globalAlpha`；不是有限的數、還沒載入、大小是 0 時不畫，壞掉的 `<img>`、寬或高是 0 的 canvas 是 `InvalidStateError`；canvas 的來源先 flush，沒畫過的是透明（不透明的是黑）；`createPattern` 的 repetition（`""` 是 `repeat`，不認得的是 `SyntaxError`，比圖片先檢查）、還沒載入是 `null`、圖案的 shader 經過 `setTransform`；origin-clean：別的 origin 的圖片、有這種圖片的 canvas 或圖案（設成樣式時）讓 `getImageData` 是 `SecurityError`，`reset()` 之後也是，改了 `width` 才變回 origin-clean | `cargo test -p dom_api --test it canvas_images::` |
| `setTransform` 收 `DOMMatrix2DInit`（短的和長的名稱、兩個都給時要相同（NaN 和 NaN 相同），不是有限的數不改）；宣告表的多載：照個數選、超過最長的照最長的、沒有的個數是列出可用個數的 `TypeError`，個數相同時照 interface 選；每組多載分得開、`kun:dom` 的名稱不重複；和規範的 IDL 比對時合起來 | `cargo test -p dom_api --test it canvas_2d::set_transform`、`cargo test -p dom_api --test it api::`、`cargo test -p dom_api --test it spec::` |
| `CanvasPattern` 是 GC 型別；context 的一個 slot 撐住漸層或圖案，換成另一種時放掉原本的 | `cargo test -p dom --test it script_objects::canvas_pattern`、`cargo test -p dom --test it script_objects::a_contexts_style_keeps_its_pattern`、`cargo test -p dom --test it script::` |
| raster：來源的矩形 strict（不取到矩形外）、`PaintFlags` 的 alpha、最近的點、雙線性和放大的 cubic；重複的圖片的 shader 經過它的 transform | `cargo test -p raster --lib an_images_source_rect_alpha_and_filter`、`cargo test -p raster --lib a_tiled_shader` |
| JS（`setTransform` 的兩種形式、`length` 是 0，`drawImage` 的三種、圖案、`imageSmoothing*`）、wasm（`draw-image` 的 canvas 來源、`create-pattern` 的 `option`、圖案的 `set-transform`）；真的畫：`<img>` 整張、縮放、一部分、`repeat-x` 的圖案、經過另一個 canvas，別的 origin（另一個 port）的圖片之後 `getImageData` 是 `SecurityError` | `cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p browser --test it canvas_images` |
| **合成和陰影** | |
| `globalCompositeOperation` 收 Porter-Duff 的 12 種和 blend mode 的 16 種，`normal` 讀回 `source-over`，其他的字串（包括大小寫不同的）不改；陰影的屬性：不是有限的數、負的模糊、不是顏色的字串不改，經過 `save` / `restore` | `cargo test -p dom_api --test it canvas_compositing::` |
| 錄下的 op：一般的模式是 op 的 blend mode（路徑、矩形、文字、圖片都是）；`copy` 先清掉 clip（不透明的畫布清成黑色）、不畫陰影；`source-in`、`source-out`、`destination-in`、`destination-atop` 在 bitmap 的座標裡的 layer；陰影是 layer 的 drop shadow（位移、模糊是 bitmap 的座標，sigma 是模糊的一半），`source-over`、`source-atop`、`destination-out` 一個 layer，其他的模式陰影和本體各一個；透明的、沒有模糊也沒有位移的陰影不畫；layer 之後的畫重新設 matrix；不透明的畫布在會降低 alpha 的模式之後補黑色；`clearRect` 不受影響 | `cargo test -p dom_api --test it canvas_compositing::` |
| raster：draw 的 blend mode（`destination-out`、`multiply`）、layer 的模式（`source-in` 清掉 layer 外面）、drop shadow（和本體、只有陰影、在 layer 的座標裡、模糊）、layer 的 alpha 和 bounds、`DrawColor` 照模式填 clip；每種模式對到 Skia 同名的 | `cargo test -p raster --lib blend_mode`、`cargo test -p raster --lib a_layer`、`cargo test -p raster --lib draw_color` |
| JS（不認得的模式、`normal`、陰影屬性的轉換）、wasm（`set-global-composite-operation` 的字串、`shadow-blur` 的數字）；真的畫：`source-in`、`xor`、`copy`、不透明的畫布上的 `destination-out`、縮放之下的陰影位移、模糊的邊緣 | `cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p browser --test it canvas_2d` |
| **`Path2D` 和 hit test** | |
| `Path2D` 的建構子（空的、複製、path data：相對的指令換成絕對、出錯時留下之前的部分、弧線是 conic）；路徑的方法沒有 transform（context 的 transform 不可逆時也照加），負的半徑是 `IndexSizeError`；`addPath` 經過 transform、不是有限的數不加、名稱不一致是 `TypeError`、加自己；context 的 `fill` / `stroke` / `clip` 收 `Path2D`：當下的路徑、fill rule、clip 換算成 bitmap 的座標，目前的路徑不變 | `cargo test -p dom_api --test it canvas_path2d::` |
| `isPointInPath`：照 fill rule、邊上也算，點經過 transform 的反矩陣換算，目前的路徑和 `Path2D` 兩種形式，不是有限的數（和夾到 float 最大值的不同）、transform 不可逆時是 `false`；`isPointInStroke`：線寬、端點、虛線、接合（尖角的 miter、bevel）、transform 的縮放 | `cargo test -p dom_api --test it canvas_path2d::` |
| `gfx` 的 hit test：fill rule、四個邊、不是有限的點；線的寬度、端點、虛線和 phase、接合和 miter 的上限；`add_path` 經過 transform；SVG 的 `isPointInStroke` 的 `square` 端點和虛線經過它 | `cargo test -p gfx --lib path_hit`、`cargo test -p shell --test it geometry` |
| `Path2D` 是 GC 型別（只當自己的型別讀回、slot 撐住它、路徑留著）；JS（`instanceof`、不能直接呼叫、`fill(path)` 和 `fill("evenodd")` 照 interface 分開、`fill({})` 是 `TypeError`、`length`、多給的參數不算、`addPath`）、wasm（`option<variant>` 的字串建 `path2d`、`fill-path2d`、`is-point-in-path-path2d`）；真的畫：even-odd 的 `Path2D` 在 transform 底下、hit test、`clip(path)` | `cargo test -p dom --test it script_objects::path_2d`、`cargo test -p dom --test it script::`、`cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p browser --test it canvas_2d` |
| **`putImageData` 和 `createImageData`** | |
| `createImageData(sw, sh, settings)`：大小取絕對值、0 是 `IndexSizeError`、色彩空間是 settings 的、不然是 context 的；`createImageData(imageData)`：同樣的大小和色彩空間、透明黑；`putImageData`：premultiplied（Skia 的四捨五入）、不透明的畫布照搬顏色和 255 的 alpha，不經過 transform、clip、`globalAlpha`、合成和陰影；髒矩形的負的大小、切到圖和畫布裡（不寫進下一列）、移動、太大的面積不寫；先畫錄下的 op，蓋住整個畫布時丟掉；detach 是 `InvalidStateError`；`[EnforceRange] long` 的 `TypeError` | `cargo test -p dom_api --test it canvas_image_data::` |
| JS（`length`、兩種 `createImageData`、`putImageData` 的小數取整、`getImageData` / `putImageData` 的 `Infinity` 和超過範圍的 `TypeError`、`transfer()` 之後是 `InvalidStateError`、讀回）、wasm（`create-image-data` 的 `s32`、`put-image-data`）；真的畫：在畫過的東西上面、不經過 transform、半透明的讀回 | `cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p browser --test it canvas_2d` |
| **輸出** | |
| `raster` 的 PNG：不是 premultiplied 的像素，有 alpha（不透明的是 RGB），deflate level 1（zlib 的 FLEVEL 是 0）、每一列 Up filter；JPEG：合成到黑色、品質影響大小、4:2:0，品質 100 是 4:4:4；沒有像素是 `None` | `cargo test -p raster --lib encode` |
| `toDataURL` 是 client 編出來的 base64 的 `data:` URL，先畫完錄下的 op；格式照 ASCII 小寫比對，`image/jpeg` 是 JPEG（沒有數字的品質是 92），其他（`image/webp`、後面有空白的）是 PNG；寬或高是 0、超過上限、編不了、沒有 client 是 `data:,`；沒有 context 是透明的、`alpha: false` 是不透明的黑色；不是 origin-clean 是 `SecurityError`，在看大小之前；品質是 0 到 1 乘 100 四捨五入，其他是 92 | `cargo test -p dom_api --test it canvas_export::` |
| 真的編碼：PNG 經過 `<img>` 畫回另一個 canvas 是一樣的像素（半透明的也是）；JPEG 的品質只認數字（字串是預設的 92），低的品質比較小；`image/webp` 給 PNG；寬是 0 的是 `data:,` | `cargo test -p browser --test it canvas_export` |
| JS（`length` 是 0、沒有 renderer 時是 `data:,`，品質不是數字也是）、wasm（`to-data-url` 只有格式，C 的 guest 沒有 renderer 時拿到 `data:,`） | `cargo test -p dom_js --test it canvas::`、`cargo test -p dom_wasm --test it a_guest_uses_a_canvas`、`cargo test -p dom_api --test it wit::` |
| `toBlob` 和 `toDataURL` 是同一個檔（逐位元組比對），是呼叫時的 bitmap（之後畫的不算），callback 在之後的 task 才呼叫；Blob 的 type 是編成的格式（`image/jpeg`，`image/webp` 是 `image/png`）；沒有像素是 `null`；被 taint 的 canvas 的 `toDataURL` 和 `toBlob` 是 `SecurityError` | `cargo test -p browser --test it canvas_export`、`cargo test -p browser --test it canvas_images` |
| JS：`length` 是 1，callback 不是函式是 `TypeError`；callback 在 canvas blob serialization 的 task 裡呼叫，沒有 renderer 時拿到 `null`，它丟的例外被回報、下一個照樣呼叫；文件沒有 window 的 canvas 什麼都不做；`kun:dom` 沒有 `toBlob`（它不改 WIT 的 golden） | `cargo test -p dom_js --test it canvas::to_blob`、`cargo test -p dom_api --test it wit::` |
| 改了 `width` 或 `height`（同一個值也算）時 canvas 變回 origin-clean，context 的 `reset()` 不會（照 Chrome） | `cargo test -p dom_api --test it canvas_images::`、`python3 scripts/test.py wpt html/semantics/embedded-content/the-canvas-element/` |
| **WPT、reftest、`about:demo` 和情境** | |
| WPT 的 `html/canvas/element/` 六個目錄（`canvas-host/`、`drawing-rectangles-to-the-canvas/`、`line-styles/`、`path-objects/`、`the-canvas-state/`、`transformations/`）：366 個測試，313 個全部通過，其他 52 個的失敗是之後幾段的功能，寫在預期結果；`2d.canvas.host.size.large.html` 停用（compositor 替整個 layer 建每一塊 tile） | `python3 scripts/test.py wpt html/canvas/element/` |
| `about:demo` 的路徑的 canvas（圓、圓角矩形、虛線、圓角接合、被 clip 切的 even-odd 星形），「New size」改 `width` 清掉畫布和狀態、照新的大小重畫 | `python3 scripts/test.py scenarios canvas demo-script` |
| WPT 的 `html/canvas/element/text/`：127 個測試，57 個全部通過，其他 70 個的失敗寫在預期結果（web font、experimental 的 `TextMetrics`、`lang`、small caps 的合成、`ex` / `ch` / `ic` 和 `lh`、彩色 emoji 的字型）；其他六個目錄裡 `font`、`textAlign`、`textBaseline` 的 `save` / `restore` 和重設變成通過 | `python3 scripts/test.py wpt html/canvas/element/` |
| Ahem 的文字和 div 的方塊逐像素一樣：對齊、每條 baseline、擠進 `maxWidth`、描邊、`letterSpacing`、右到左的 `start`；照 `measureText` 的寬度和 `fontBoundingBox` 畫的框和頁面上 `<span>` 的同一段文字一樣 | `python3 scripts/test.py reftest canvas/` |
| `about:demo` 的文字的 canvas（圖表的標籤、描邊的標題、照 `measureText` 的 ink 畫的框），「Bigger text」用大一點的字重畫 | `python3 scripts/test.py scenarios canvas`、`python3 scripts/test.py automation` |
| WPT 的 `html/canvas/element/` 加上八個目錄和直接在它底下的六個測試：595 個，459 個全部通過，其他 136 個的失敗寫在預期結果（`createImageBitmap`、kun 沒有的 CSS 顏色、參考頁的 `object-fit`、SVG 圖片、`new ImageData(data)` 和 `pixelFormat`、Chrome experimental 的顏色物件……），之前的 33 個預期的失敗變成通過；`new ImageData` 太大是 `IndexSizeError` | `python3 scripts/test.py wpt html/canvas/element/`、`cargo test -p dom_api --test it canvas_image_data::` |
| canvas 和頁面上同樣的東西逐像素一樣：不透明的硬 stop 的漸層和 `linear-gradient`、1:1 的 `drawImage` 和 `<img>`、`repeat` 的圖案和背景圖、沒有模糊的陰影、`destination-over`、even-odd 的 `Path2D`、經過 transform 也照位置寫的 `putImageData` | `python3 scripts/test.py reftest canvas/` |
| `about:demo` 的樣式和圖片的 canvas（漸層、圖案、陰影、畫上去的 canvas、`multiply` 的 `Path2D`），點中間的圓用 `isPointInPath` 找到、`putImageData` 把它的框的顏色反過來 | `python3 scripts/test.py scenarios canvas`、`python3 scripts/test.py automation` |
| WPT 的 `html/semantics/embedded-content/the-canvas-element/`：70 個測試，61 個全部通過（`toDataURL` 和 `toBlob` 的全部通過，除了 `toBlob.null` 用 Window 的 named access），其他的失敗寫在預期結果（fallback content 的 focus、`new ImageData(data)`、shadow DOM、`reset()` 不讓 canvas 變回 origin-clean，Chrome 也一樣）；`security.pattern.fillStyle.sub.html` 停用（等 body 裡的 SVG `<image>` 的 load，跑到 timeout） | `python3 scripts/test.py wpt html/semantics/embedded-content/the-canvas-element/` |
| `about:demo` 的「Copy as image」：樣式的 canvas 經過 `toBlob` 和 blob URL 顯示在下面的 `<img>`，說明寫出格式和大小；`demo-script` 的按鈕清單多一個 | `python3 scripts/test.py scenarios canvas demo-script`、`python3 scripts/test.py automation` |

## 做過的檢查

- **決定**：第 1 到 13 條 2026-10-11 確認，第 14 到 65 條在每段和它的小段開始前確認（每段的比較和 Chromium 的對照在 `details.md` 第 5 到 22 節）。和建議不同、實作中改的：第 26 條選了 E3-b（是 layer 的 canvas 畫過時直接換快照、不重新 paint；建議的是 E3-a）；第 54 條的組路徑的程式放在 `computed_style`，不是 `style`（`layout` 不依賴 `style`，`PathSegment` 在 `computed_style`）；`details.md` 第 10 節原本寫 U+000A 到 U+000D 換成空格，照 HTML 和 WPT 改成 ASCII 的空白（不含 U+000B）；3b 以為 Blink 的 `reset()` 和新的大小都不讓 canvas 變回 origin-clean，4c 的 WPT 找到新的大小會（`OnWidthOrHeightAssigned`），照它改。
- **和 Chromium、Chrome 的對照**：每段的細節照 Chromium 的原始碼（`details.md` 各節記了對照的檔案和日期）。這台 Windows 上 Chrome for Testing 起不來（network service 和 GPU process 一開始就 crash，記在 `docs/limitations.md` 的「平台和建置」），`browser` 的測試的預期像素照 Skia 的 source-over 算，`dom_js` 的預期結果照規範和原始碼寫，還沒在 Chrome 152 上跑同樣的頁（`colorType` 是不是 `getContextAttributes()` 的成員、成員的順序、半透明的像素）；之後在有 Chrome 的機器上補對照。
- **實作上值得記的**：
  - 是 layer 的 canvas 畫過時，`FrameView` 拿上次的 `PaintOutput` 換掉它的快照，它的 item 標成 stale、下一次 paint 時重畫：不然 canvas 之後畫進頁面時（祖先的 transform 從平移變成 `scale`，box 和 property 的節點都沒變）會露出舊的快照。`TextureLayer` 的快照在建 layer 時從 DOM 拿，不從 display item 拿。
  - clip 記在它那一層、bitmap 的座標裡，每次開始錄製時重播每一層的 `Save` 和 clip，只替第一層以上的狀態錄 `Save`。
  - `gfx` 的 `path_shapes` 移植 Skia 的 `arcTo`、`addRect`、`addOval`、`addRRect`、`BuildUnitArc`、`setRectRadii`，和 Skia 自己做的路徑逐位元比對；`arcTo` 照 Blink 先比較最後一個點（`closePath` 之後和 Skia 不同）。
  - 文字：parley 排一行，度量讀 hhea、OS/2 和 BASE；ink 在 renderer 經過 `raster::ink_bounds` 加上（`layout` 不碰 Skia）；renderer 的 `RasterCache` 給所有 canvas 共用。spacing 在狀態裡存讀回的字串（`dom` 不依賴 `style`），排版時再換成 px。
  - 漸層和圖案在 context 每一層的 slot 裡（`StyleSlot`：一條漸層、一條圖案的 edge），狀態只記是哪一種。
  - renderer 的圖片 request 原本沒帶 document 的 origin 當 initiator，network service 當成 browser 的 request，別的 origin 的回應不是 opaque；改成帶著，圖片表記下 opaque 的網址。
  - 陰影一律是 drop shadow 的 layer；`copy` 先清 clip、不畫陰影；不透明的畫布在會降低 alpha 的模式之後補黑色。
  - `toBlob` 的 task 只帶編好的位元組和格式，`Blob` 在 task 裡建（物件屬於執行 task 的 realm）；文件沒有 window 時照 Blink 什麼都不做。PNG 的設定用 `fdeflate` 解開 IDAT、看每一列的 filter byte 來測。
- **找到、修掉的既有 bug**：Lua 的產生器把回傳的 record 的欄位寫成 kebab-case（之前回傳的 record 只有一個字的欄位）；W9 的結果方塊有幾個在截圖外面，錯了也不會失敗（改成 10px 高）；`new ImageData(sw, sh)` 太大時 Blink 是 `IndexSizeError`（kun 原本是 `RangeError`）；rebase 之後 `寬 × 高 × 4` 超過 `u64`（debug build panic，改用 `u128`）；canvas 改了大小不變回 origin-clean；圖片 request 的 initiator（上一條）。
- **WPT**：每段先用 `--all` 跑，失敗的照原因分類，範圍外的功能和已知的不同寫成預期的失敗、每個 `.ini` 附上原因。1f：六個目錄 366 個，313 個全部通過；2d：`text/` 127 個，57 個全部通過（失敗的大多是 web font 和 experimental 的 `TextMetrics`）；3f：再加八個目錄和六個檔，`html/canvas/element/` 595 個，459 個全部通過，之前 33 個預期的失敗變成通過；4c：`the-canvas-element/` 70 個，61 個全部通過。停用兩個：`2d.canvas.host.size.large.html`（compositor 替 2147483647 px 見方的 layer 建每一塊 tile，kun 記憶體不夠；同樣大小的 div 也一樣，記在 `docs/limitations.md`）、`security.pattern.fillStyle.sub.html`（跑到 timeout）。
- **reftest**：`canvas/c1` 到 `c6`、`hidpi/c3-canvas-layer.html`，都不需要 fuzzy。
- **`about:demo` 和情境**：四個 canvas 和五個按鈕（`demo-script` 的按鈕清單跟著多）；每段的截圖照看過，真的視窗（1.5x、系統字型）也對。`about:demo` 變長兩次讓情境的 `scroll-to` 失效：`backgrounds`（1f）和 `canvas`（4c）都是捲到按鈕時、按鈕被捲到最上面，要比對的元素在它上面、出了畫面；改成捲到要比對的元素，點了按鈕之後再捲回去。
- **Speedometer 的盤點**（只跑 kun 那一側，`--reuse-chrome` 和一份空的 `chrome.json`）：2d 之後三個 suite 都不再停在 canvas：React-Stockcharts-SVG 跑完、沒有例外；Charts-chartjs 跑完，第一個例外是 `ResizeObserver is not defined`；Perf-Dashboard 照舊停在 `alert is not defined`；3f 時一樣。完整的盤點（Chrome 的紀錄和 probe）在 macOS 上重跑。
- **mutation test**（`mutations-1a.toml` 到 `mutations-4c.toml`、`mutations-1a-lua.toml`，共 19 份 456 條：1a 22 和 Lua 的 2、1b 39、1c 15、1d 30、1e 48、2a 19、2b 55、2c 19、2d 7、3a 23、3b 53、3c 39、3d 32、3e 23、3f 2、4a 20、4b 7、4c 1）：補了測試之後全部抓到，除了下面的等價和守不住的。第一次沒抓到的常見原因：
  - 測試的輸入剛好看不出差別：90 度的角 miter 和上限一樣（改用約 19 度）、Ahem 的 OS/2 剛好合起來 1em（加 Lato）、模式已經是 `xor` 時設 `XOR`、差一點點才一圈的弧、Skia 的 `flush_to_zero` 只走了一個方向；
  - 只測了一個方向：漸層換成圖案、改成顏色時放掉漸層；
  - 清單的測試名稱寫錯，篩不到測試，原本的程式也「通過」（3b 的 `navigation::canvas_images`）；
  - 搬過來的程式本來就沒有測試（SVG 的 `square` 端點和虛線的 `isPointInStroke`）。

  等價的：「不透明的 bitmap 照 premultiplied 畫」（`docs/test-gaps.md` 的「等價、不用處理的 mutation」）、`fillText` 也要 ink（只多花時間）、起點在左邊且寬是負的 `putImageData`（算出來一樣是空的）；拿掉的多餘程式：`ImageData` 的「單邊超過 `long`」。守不住的：「畫過不要求 frame」（跑 script 的 task 結束時本來就要求 frame）、`WgpuRenderer` 的 sampler 只在真的視窗裡跑（都記在 `docs/test-gaps.md` 的「測試守不住的地方」）；`TextureLayer::contents_opaque` 只帶著，Viz 還沒照它省掉 blending。之後的步驟改了程式碼，前面幾份的原文有些對不上（`mutations-1c.toml` 的 `frame_view.rs` 兩條、`mutations-4a.toml` 的 ASCII 小寫那條在 clippy 改成 `eq_ignore_ascii_case` 之後）：它們是當時的紀錄，要重跑時先照現在的程式碼改原文。
- **不穩定的**：3e 時 rustc 編 `dom_api` 的一個 mutant 時自己 crash（`STATUS_ACCESS_VIOLATION`，不是 mutant 的錯），用 `--only` 重跑那一條正常；只出現這一次。
- **rebase**：1e 前 rebase 到 SVG 那邊的路徑型別（`37315bdb`，19 個檔案衝突：兩邊都加的 GC 型別、WIT、W9、文件，都兩邊保留）；3f 之後 rebase 到 `main` 的 SVG 圖片和 `MessageChannel`（`ObjectTag` 合起來 44 種，圖片表兩邊都留）。
- **工作上的教訓**：Windows 的 Git Bash 的 heredoc 會換掉反斜線（Lua 字串裡的 `\255` 變成真的 byte，Python 的 `\u{212A}` 失敗），有反斜線的內容一律用寫檔的工具。
- **各段做完時的驗證**（Windows 11，Rust 1.99.0）：每段 `cargo fmt --check`（連同 `sdk/lua/generator`）和 clippy 抓到的都在那一段修掉（過長的型別改成 type alias、`drop` 一個 read scope、多餘的 `&`、`unsafe` 和它的 `SAFETY` 註解被 fmt 拆開）；`cargo test --workspace` 的 42 個測試 binary 全部通過；`test.py scenarios` 全部、`test.py automation` 和 `pipe --window` 通過。
- **完成時的驗證**（2026-10-12，Windows 11，Rust 1.99.0）：`cargo fmt --check`（連同 `sdk/lua/generator`）修了第 4 段的程式碼；`cargo clippy --workspace --all-targets -- -D warnings` 修了三處（`chunks_exact(4)` 改成 `as_chunks`、比對格式字串改用 `eq_ignore_ascii_case`、測試的 client 的型別改成 type alias），改了之後 `raster`、`dom_api` 和 `browser` 的 canvas 測試重跑通過；`cargo test --workspace --no-fail-fast` 的 42 個測試 binary、2219 個測試全部通過；`test.py wpt html/canvas/element/ html/semantics/embedded-content/the-canvas-element/` 的 1158 個測試（2466 個 check）沒有意外的結果；`test.py scenarios` 全部 48 個、`test.py automation` 的 37 個情境和 `pipe --window` 通過（1.5x 的視窗，`canvas` 的截圖照看過）；`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs` 通過。
- **rebase 到 `main`（2026-10-12）**：`main` 多了 `MessageChannel` 的 transfer（`PortMessage`、`JsOnly<T>`、`serialize_with_transfer`）和 RFC 0035 的草稿。衝突只在文件：`crates/dom_api/README.md` 的 `ScriptHost` 那一列和 `[JsOnly]` 的說明（兩邊的內容都留下）、`rfcs/README.md` 的索引（0033 取這一邊的，0034、0035 取 `main` 的）；`Task`、`TaskSource`、`CallbackType` 和 `messaging` 測試的 `Page` 自動合併。之後 `cargo fmt --check`（連同 `sdk/lua/generator`）、clippy、`cargo test --workspace --no-fail-fast`（42 個測試 binary、2231 個測試）、`test.py wpt` 的兩個 canvas 目錄（1158 個測試沒有意外的結果）、`test.py scenarios` 全部 48 個和 `sanity.py` 的五項都通過。
