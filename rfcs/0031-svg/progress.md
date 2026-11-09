# RFC 0031 的進度：SVG

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-10。完成：2026-10-11，macOS（Apple Silicon，Rust 1.99.0）。四段都在 `0031` 上做；2a-0、2d-0、2e-0（給 canvas 那條線用的型別）和第 1 到 3 段做完時 commit、審核、rebase 到 `main` 再 merge 回去，第 4 段接著在同一個 branch 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. SVG 的 DOM（第 4 到 9 條） | 完成，merge 回 `main` | 2026-10-11 |
| 1a. 元素的 interface、`SVGElement` 不需要新 GC 型別的成員 | 完成 | 2026-10-10 |
| 1b. `className`（`SVGAnimatedString`） | 完成 | 2026-10-11 |
| 2. inline `<svg>` 的繪製（第 10 到 13 條） | 完成，merge 回 `main` | 2026-10-11 |
| 2a-0. 路徑的型別（第 14 條，單獨先 merge） | 完成，merge 回 `main` | 2026-10-11 |
| 2a. `<svg>`、基本圖形和 `path`、純色的 `fill` 和 `stroke`、`transform`、`viewBox`、presentation attribute | 完成 | 2026-10-11 |
| 2a-1. style：fill、stroke 和 geometry 的 property、presentation attribute、`CSS` namespace、`visibility`（第 15、19、20 條） | 完成 | 2026-10-11 |
| 2a-2. layout 和 paint：`<svg>` 的 replaced box、SVG 的 layout 樹、`viewBox`、SVG 的 property node（第 16 到 18、22、23 條） | 完成 | 2026-10-11 |
| 2b. hit test 和事件（第 24 到 26 條） | 完成 | 2026-10-11 |
| 2c. `defs`、`use`、`symbol`、`switch`（第 27 條） | 完成 | 2026-10-11 |
| 2d-0. 漸層的型別（第 28、29 條，單獨先 merge） | 完成，merge 回 `main` | 2026-10-11 |
| 2d. 漸層（第 30 條） | 完成 | 2026-10-11 |
| 2e-0. `DrawGlyphs` 帶 `PaintFlags`（第 32 條，單獨先 merge） | 完成，merge 回 `main` | 2026-10-11 |
| 2e. `text`、`tspan`（第 31、33、34 條） | 完成 | 2026-10-11 |
| 3. 要幾何的 API（第 36 到 45 條） | 完成，merge 回 `main` | 2026-10-11 |
| 3a. `SVGMatrix`、`SVGPoint`、`SVGRect` 和 `createSVG*()` | 完成 | 2026-10-11 |
| 3b. `getBBox()`、`getCTM()`、`getScreenCTM()`、`getBoundingClientRect()`、focus | 完成 | 2026-10-11 |
| 3c. `SVGGeometryElement` 和文字的方法 | 完成 | 2026-10-11 |
| 4. SVG 當圖片（第 46 到 57 條） | 完成 | 2026-10-11 |
| 4a. XML parser（第 51 到 53 條） | 完成 | 2026-10-11 |
| 4b. `<img>` 的 SVG（第 54 到 56 條） | 完成 | 2026-10-11 |
| 4c. `background-image` 的 SVG（第 57 條） | 完成 | 2026-10-11 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **SVG 的 DOM** | |
| SVG 的元素照名稱（分大小寫）各自是規範的 interface，沒有 interface 的名稱是 `SVGElement`，別的 namespace 是 `Element`；`svg_tags` 和祖先鏈的 `accepts` | `cargo test -p dom_api --test it api::svg_elements_get_their_interface` |
| `ownerSVGElement`、`viewportElement`：巢狀的 `<svg>`、`<symbol>`、`<image>`、`foreignObject` 裡的 `<svg>`（直接的和隔著 HTML 的）、不在樹裡的 | `cargo test -p dom_api --test it api::svg_owner_and_viewport_elements` |
| `tabIndex`：SVG 的 `<a>` 是 0、其他 -1、`tabindex` 優先；有 `href` 或 `xlink:href` 的 `<a>`、有 `tabindex` 的元素可以 focus，沒有連結的 `<a>` 不行 | `cargo test -p dom_api --test it api::svg_tab_index`、`--test it focus::svg_elements_that_can_take_the_focus` |
| JS：原型鏈照規範到 `Element`，window 上有每個 interface object（`kun:dom` 沒有的也有）；`style`、`dataset`、`ownerSVGElement`、`viewportElement`、`tabIndex` 在 SVG 元素上；`SVGElement` 的成員對 HTML 元素是 Illegal invocation | `cargo test -p dom_js --test it bindings::elements_are_of_their_svg_interface` |
| `kun:dom`：只有 `svg-element`（只給 JS 的 interface 沒有 resource，型別換成最近有 resource 的祖先）；Lua 的 guest 把 SVG 元素轉成 `svg-element`、`owner_svg_element` 回傳它 | `cargo test -p dom_api --test it wit::`、`python3 scripts/test.py reftest wasm/w9`（`cargo test -p shell --test it wasm_reftests::`） |
| 宣告表和規範的 WebIDL 比對讀 `SVG.idl`、`svg-paths.idl`、`css-masking.idl`、`filter-effects.idl`、`svg-animations.idl`（還沒有的成員在 `spec-missing.txt`） | `cargo test -p dom_api --test it spec::` |
| WPT：`svg/historical.html`、`svg/idlharness.window.js`、`svg/struct/scripted/element*.html` 7 份，`dataset.html` 的 SVG 那一項、`namespaced-attribute-mutations.html` 的 SVG inline style 四項、`tabindex-focus-flag.html` 的 SVG 元素四項變成通過 | `python3 scripts/test.py wpt`（0 個不符） |
| `className`：`SVGAnimatedString`，每次同一個，`baseVal` 讀寫 `class`、`animVal` 等於它；HTML 元素的照舊是字串 | `cargo test -p dom_api --test it api::svg_class_name` |
| `dom` 的 `SvgAnimatedString`：每個元素一個、反映 `class`、slot 撐住它和它的元素；GC 型別的共通檢查和工作負載 | `cargo test -p dom --test it tree::svg_class_names`、`script_objects::svg_animated_string`、`script::every_type_unwraps_only_as_itself`、`gc_stress::` |
| JS：`className` 是 `SVGAnimatedString`；另一個 realm 的元素的是那個 realm 的（`dataset` 也是） | `cargo test -p dom_js --test it bindings::elements_are_of_their_svg_interface`、`realms::wrappers_are_made_in_their_objects_realm` |
| Speedometer：TodoMVC-Vue 跑完（3 個 test），NewsSite-Nuxt 過了 `SVGElement`、停在 `crypto.getRandomValues`（RFC 0028 的第 5 段） | `python3 scripts/ci.py speedometer --inventory --suites TodoMVC-Vue,NewsSite-Nuxt --reuse-chrome` |
| **路徑的型別**（第 14 條） | |
| `gfx::Path`：bounds 包含控制點和沒有起點時的原點；多邊形是閉合的 subpath；fill rule | `cargo test -p gfx --lib` |
| `DrawPath` 描邊的 bounds 照寬度、miter 和方形的端點往外；虛線奇數個時重複、負的、全 0、不是數字時是實線 | `cargo test -p compositor_paint --lib` |
| `raster`：路徑逐個轉成 `SkPath`（conic 的 weight、close 之後補的起點）、even-odd、`ClipPath`、端點、接合、miter、虛線的 phase | `cargo test -p raster --lib`（`paths_become_skias_element_for_element`、`a_path_fills_by_its_fill_rule`、`a_path_clip_keeps_what_its_fill_covers`、`strokes_end_and_join_as_their_flags_say`、`dashes_go_through_their_intervals_from_the_phase`） |
| border 的虛線和點線照舊（點線是 round cap） | `cargo test -p paint --test it artifact::`、`python3 scripts/test.py reftest paint/ hidpi/ backgrounds/`、`test.py wpt css/css-backgrounds/` |
| **SVG 的 property 和 presentation attribute**（第 15、19、20 條） | |
| 二十個 property 的初始值、CSS 的值（純數字是 user unit、負值、`url()` 的 fallback、虛線的寫法）、computed value（`d` 變成絕對座標、`currentcolor` 解析）、繼承和 CSS-wide 關鍵字、宣告值的序列化，都照 Chrome 152 的實測 | `cargo test -p style --test it svg::` |
| presentation attribute：純數字、不合法的值不算、幾何的只在規定的元素上、`d` 到第一個錯誤為止、`transform`（`gradientTransform`、`patternTransform`）照 SVG 的語法變成一個 matrix、動畫元素的 `fill` 不算、HTML 元素沒有、輸給 CSS | 同上 |
| `inset` shorthand 能寫回 | `cargo test -p style --test it cssom::shorthands_read_back_from_their_longhands` |
| `getComputedStyle`：SVG 元素沒有 box 時 `transform` 是它的 matrix，`display: none` 的是 `none` | `cargo test -p dom_api --test it computed_style::an_svg_elements_transform_is_its_matrix` |
| `CSS.supports()`（宣告、條件、`selector()`）、`CSS.escape()` | `cargo test -p dom_js --test it bindings::the_css_namespace`；WPT `css/cssom/CSS-supports.html`、`escape.html`、`CSS-escape-numeric-id.html`、`css/css-conditional/js/CSS-supports-*.html` |
| `visibility: hidden`：不畫自己的背景、邊框、outline、文字、inline box、圖片、iframe 的子 frame，hit test 打不到，不能 focus；`visible` 的子孫照畫、打得到 | `cargo test -p paint --test it artifact::hidden_boxes_paint_only_their_visible_descendants`、`artifact::a_hidden_image_draws_nothing`、`child_frames::a_hidden_iframe_shows_no_child_frame`、`geometry::hidden_boxes_arent_hit`；`-p dom_api --test it focus::a_hidden_box_cant_take_the_focus`；WPT `css/CSS2/visufx/visibility-*`（4 份）、`css/cssom-view/elementFromPoint-002.html`、`-003.html`、`elementFromPoint-visibility-hidden-resizer.html` |
| **inline SVG 的 layout 和 paint**（第 16 到 18、22、23 條） | |
| 最外層的 `<svg>` 的大小照 Chrome 152 的實測（預設 300×150、`viewBox` 的比例撐滿寬、一邊給定時照比例、CSS 勝過 attribute、padding 和 border），inline 的也是 | `cargo test -p layout --test it svg::an_outermost_svg_is_sized_as_chrome_sizes_it`；`test.py reftest svg/sizing.html` |
| 子孫沒有 CSS 的盒子；只抓畫得出的元素（`display: none`、不認得的、`<text>`、`foreignObject` 和它們裡面的都不抓） | `svg::svg_content_makes_no_boxes_and_only_what_is_drawn_is_laid_out` |
| 圖形的路徑：rect（`auto` 的半徑、上限是邊的一半、沒有面積不畫）、circle（百分比是正規化對角線的）、ellipse、line（attribute 的長度）、polyline、polygon、path（`S`、`T` 的反射、弧線是 conic、半徑 0 是直線、半徑太小放大），弧線的 conic 權重 | `svg::rects_and_their_corners`、`circles_ellipses_lines_and_polys`、`path_data_reflects_and_draws_arcs_as_conics`；`test.py reftest svg/`（`rect`、`path`、`line-polygon`、`ellipses`） |
| `viewBox` 和 `preserveAspectRatio`（meet、slice、none），百分比是 `viewBox` 的；寬高是 0 的 `viewBox` 不畫、負的當成沒有；巢狀 `<svg>` 的位置、`viewBox` 和裁切 | `svg::view_boxes_and_aspect_ratios`、`nested_svgs_place_and_clip_their_content`；`test.py reftest svg/view-box.html`、`view-box-aspect.html`、`nested-svg.html`、`outer-clip.html` |
| `transform` 的百分比是 viewport 的，`stroke-width`、虛線是正規化對角線的 | `svg::transforms_and_strokes_resolve_against_the_viewport`；`test.py reftest svg/transform.html`、`stroke.html` |
| 改屬性（`x`、`viewBox`）、加子元素、改子孫的 style 時整個重抓，和重新建的樹一樣 | `cargo test -p layout --test it update::an_svg_takes_its_content_again`、`-p dom --test it layout_tree_flags::an_svg_elements_attribute_flags_it`；`test.py reftest svg/dynamic.html` |
| property node：`viewBox`（連同內容框的位置）、`transform`、`opacity` 的 effect、巢狀 `<svg>` 的裁切；最外層的 `<svg>` 不是捲動容器、裁切到內容框、圓角時切到曲線；再 paint 一次時圖形照樣在自己的 state 下 | `cargo test -p paint --test it svg::`；`test.py reftest svg/group-opacity.html` |
| fill 和 stroke：先 fill 再 stroke，顏色、`currentColor`、opacity、端點、接合、miter、虛線；`url()` 的 fallback；`visibility: hidden`、寬 0 的 stroke、完全透明的不畫 | `cargo test -p paint --test it svg::fills_and_strokes`；`test.py reftest svg/paint-properties.html` |
| WPT 的 `svg/` reftest（HTML 的，不靠還沒畫的東西）12 份；SVG 有了盒子之後 `elementFromPoint`、`outer-svg.html`、SVG 的 `<a>` 和 `element.html` 的 `focus()` 變成通過 | `python3 scripts/test.py wpt svg/ css/cssom-view/ html/interaction/focus/` |
| **hit test 和事件**（第 24 到 26 條） | |
| `pointer-events` 的值（照 Chrome 寫成小寫）、presentation attribute、繼承 | `cargo test -p style --test it svg::pointer_events` |
| 圖形照幾何打到：fill（even-odd 的洞）、stroke（stroke-only 的中間打到 `<svg>`、虛線的空隙）、圓的曲線外、邊上算在裡面；變換、巢狀 `<svg>` 的裁切、後畫的在上面 | `cargo test -p paint --test it svg::shapes_are_hit_by_their_fill_and_stroke`、`svg::hit_testing_goes_through_transforms_clips_and_paint_order`；WPT `svg/interact/scripted/` 的五份 |
| `pointer-events` 的每個值對 SVG 圖形的作用（`none`、`visibleFill`、`visibleStroke`、`visiblePainted` 和 `visibility`、`painted`、`fill`、`bounding-box`、`all`、繼承） | `svg::pointer_events_say_what_of_a_shape_counts` |
| `elementsFromPoint` 列出圖形和 `<svg>`，不列 `<g>` 這類容器（第 35 條，Chrome 152 的實測），`pointer-events: bounding-box` 的容器除外 | `svg::every_element_at_a_point_lists_a_shape_and_its_svg` |
| HTML 的 `pointer-events: none`：盒子、文字、inline box、iframe 的子 frame 打不到，`auto` 的子孫打得到 | `cargo test -p paint --test it geometry::pointer_events_none_isnt_hit`、`child_frames::a_child_frame_under_pointer_events_none_isnt_hit`；WPT `css/cssom-view/elementFromPoint.html`、`elementsFromPoint.html` |
| SVG 的 `<a>` 換頁（`href`，沒有時 `xlink:href`），點到裡面的圖形也算 | `cargo test -p dom_api --test it events::an_svg_link_is_followed`；情境 `svg`（`python3 scripts/test.py scenarios svg`） |
| **漸層的型別**（第 28、29 條） | |
| `raster`：不用 tile 的漸層蓋滿整個平面、經過它的 transform；tile mode 的 clamp、repeat、mirror；兩個圓心不同的 two-point conical（焦點在圓裡、偏離圓心）；unpremultiplied 和 premultiplied 的內插；漸層的 transform 在 tile 的位置之後 | `cargo test -p raster --lib`（`a_gradient_covers_the_plane_through_its_transform`、`past_its_ends_a_gradient_extends_repeats_or_mirrors`、`a_two_point_conical_gradient_goes_from_one_circle_to_the_other`、`unpremultiplied_gradients_mix_the_colors_alone`、`gradients_interpolate_premultiplied`、`a_gradients_transform_is_after_its_tiles_place`） |
| CSS 的漸層照舊：橢圓是圓經過 transform 縮放、`repeating-*` 是 repeat、premultiplied | `cargo test -p paint --lib`（`background::tests::`）；`test.py reftest backgrounds/ paint/ hidpi/ images/`；`test.py wpt css/css-backgrounds/ css/css-images/` |
| **漸層**（第 30 條） | |
| `stop-color`、`stop-opacity`：初始值、CSS 的值（`currentcolor` 是 stop 的 `color`、百分比、計算時鉗制）、presentation attribute、不繼承 | `cargo test -p style --test it svg::stop_colors`、`svg::initial_values` |
| 線性漸層預設在圖形的 object bounding box 裡（圓的是貼著曲線的 tight bbox）從 0% 到 100%；`userSpaceOnUse` 在 user space，百分比是 viewport 的 | `cargo test -p layout --test it svg::linear_gradients_in_the_box_or_in_user_space` |
| `href`（和 `xlink:href`）鏈：每個 attribute 取第一個有它的漸層，幾何只從同一種的拿，stop 取第一個有 stop 的，循環到那裡為止；放射漸層的 `fx`、`fy` 沒有時是 `cx`、`cy`，`fr`；`spreadMethod`；`gradientTransform` 接在 bbox 的換算之後 | `svg::gradients_take_what_they_lack_through_href` |
| stop 的偏移：數字或百分比，鉗制到 0 到 1、不小於前一個；顏色乘上 `stop-opacity` | `svg::stops_are_clamped_in_order` |
| 用不了的：參考不到、不是漸層、別的文件的 `url()` 用 fallback；沒有 stop 的不畫（不用 fallback）；長度是 0 的、半徑是 0 的是最後一個 stop 的顏色；一個 stop 是它的顏色；bbox 沒有高度的用 fallback；片段 percent-decode | `svg::gradients_that_cant_be_used`；`cargo test -p dom --test it tree::fragment_ids` |
| 失效：另一個 `<svg>` 裡的漸層的 stop、attribute、`href` 改了，後來才有的 id，參考它的 `<svg>` 重抓；`<use>` 的 copy 參考的漸層（連同重新複製之後才參考的）改了，`<use>` 所在的 `<svg>` 重抓 | `cargo test -p layout --test it update::an_svg_takes_its_content_again_when_its_gradients_change`、`update::an_svg_takes_its_content_again_when_a_copys_gradient_changes` |
| paint：漸層是不用 tile 的 shader，放射的從 focal 圓到結束的圓，`reflect` 是 mirror、`repeat` 是 repeat，unpremultiplied；opacity 是 flags 的 alpha；沒有 stop 的不畫，一個顏色的照 opacity | `cargo test -p paint --test it svg::gradients_paint_as_shaders` |
| `gfx::Path` 的 tight bounds（曲線本身、Skia 插入的 move） | `cargo test -p gfx --lib` |
| 畫出來：線性、放射、fallback、改了之後 | `python3 scripts/test.py reftest svg/`（`gradient-linear`、`gradient-radial`、`gradient-fallback`、`gradient-dynamic`） |
| WPT：`css/css-transforms/` 用漸層的 63 份（SVG 的 transform、CSS、`gradientTransform`、list 的分隔、預設和不合法的 `transform-origin`），`href-gradient-element.html`、`percent-encoded-fragment-fill-stroke.html` | `python3 scripts/test.py wpt css/ svg/`（0 個不符） |
| **字帶 `PaintFlags`**（第 32 條） | |
| `raster`：`DrawGlyphs` 照 flags 填或描邊、用 shader、帶 alpha；每個 glyph 照 `rotations` 繞著自己的原點順時針轉 | `cargo test -p raster --lib`（`glyphs_are_filled_or_stroked_with_their_flags`、`glyphs_turn_about_their_origins`、`ahem_glyph_is_a_crisp_square`） |
| 頁面和瀏覽器 UI 的字照舊：填色、不轉 | `cargo test -p paint --test it artifact::`、`-p shell`；`test.py reftest text/ inline/ paint/ hidpi/` |
| **文字**（第 31、33、34 條） | |
| `text-anchor`：關鍵字、繼承、presentation attribute | `cargo test -p style --test it svg::text_anchor`、`svg::initial_values` |
| 空白（Chrome 152 實測）：跨元素合併、去掉頭尾；`xml:space="preserve"`、不合併的 `white-space` 保留，tab 變成空格；`display: none` 的 `<tspan>`、不是 `<tspan>` / `<a>` 的元素沒有字元；沒有字元的 `<text>` 不畫 | `cargo test -p layout --test it svg::svg_text_processes_white_space`；`-p paint --test it svg::svg_text_paints_its_glyphs`（tab 的 glyph） |
| 每個字元的位置：`x`、`y`（百分比是 viewport 的）、`dx`、`dy`、`rotate` 從最內層有值的元素，`dy` 累積，`rotate` 的最後一個值延續 | `svg::svg_text_places_each_character` |
| chunk（`x` 或 `y` 開始，`dy` 不會）照第一個字元的 `text-anchor`，從還沒移動前的位置算（Chrome 152 的值） | `svg::svg_text_anchors_each_chunk` |
| `<use>` 複製的 `<text>` 畫成 `<use>`；文字和 `<tspan>` 的位置改了時重抓 | `svg::a_copy_of_text_is_drawn_as_its_use`；`update::an_svg_takes_its_content_again_when_its_text_changes` |
| paint：每個 run 先填色再描邊，漸層在文字的 bbox 上（連同旋轉的格子），`<tspan>` 自己的 paint，旋轉的 glyph 帶角度 | `cargo test -p paint --test it svg::svg_text_paints_its_glyphs` |
| hit test：字元的格子（ascent 以上、chunk 之間、描邊超出的部分都打不到），回報最內層的元素，旋轉的格子，`pointer-events: none`、沒有填色也沒有描邊的打不到；`elementsFromPoint` 只列最內層的元素（`<text>` 只為它自己的字列出），照 Chrome 152 | `svg::svg_text_is_hit_by_its_cells`、`svg::every_element_at_a_character_lists_the_elements_around_it` |
| 畫出來（Ahem 的方塊比對 `<div>`）：位置、chunk、空白；漸層、描邊、旋轉、`<tspan>` 的 paint、`visibility` | `python3 scripts/test.py reftest svg/`（`text-position`、`text-paint`） |
| 情境：點 SVG 的字，事件的 target 是它所在的 `<tspan>` | `python3 scripts/test.py scenarios svg` |
| WPT：`svg/text/` 的四份 reftest、四份 crashtest，`viewBox-scaling-text-001.html`、`pct-stroke-dasharray-02.html` | `python3 scripts/test.py wpt svg/`（0 個不符） |
| **`defs`、`use`、`symbol`、`switch`**（第 27 條） | |
| `<use>` 的 copy 的 style：繼承 `<use>`、自己的 presentation attribute 和規則照樣算；規則在 copy 裡比對（copy 的根沒有 parent 和兄弟，`defs rect` 比對不到）；`href` 優先、沒有時 `xlink:href`（parser 給的 prefix），只認 `#id`，同一個 id 取樹的順序的第一個；巢狀的 `<use>` 的 copy | `cargo test -p style --test it svg_use::` |
| 循環照 Chrome（參考自己、參考 copy 裡的祖先或外面的 `<use>` 的 id）：循環接上的那一層不畫；copy 超過一萬個元素時整個不畫 | `svg_use::cycles_draw_nothing`、`svg_use::too_many_copies_draw_nothing` |
| 什麼時候重做 copy：複製的元素改了、加了或移走子元素，`<use>` 本身重算 style、`href` 改了，參考找到另一個元素（後來才有的 id、樹的順序更前面的同 id），元素移走；不相干的改動沿用；拿出去再放回的 `<use>`；每次都和重新算全部的結果一樣 | `svg_use::a_change_in_what_is_copied_styles_the_copy_again`、`a_change_to_the_use_styles_the_copy_again`、`a_change_to_what_the_reference_finds_styles_the_copy_again`、`a_use_put_back_is_copied_again` |
| layout：`<use>` 照 `x`、`y` 放、畫成 `<use>`；`<defs>` 不畫；`<symbol>` 只在 `<use>` 參考時畫，照 `<use>` 的 `width`、`height`（`auto`、負的、不合法的是 100%）當 viewport，`<use>` 參考的 `<svg>` 也是；copy 裡的 `<symbol>` 不畫；巢狀 `<use>` 畫成外面的 `<use>` | `cargo test -p layout --test it svg::a_use_places_its_copy_and_draws_it_as_itself`、`svg::a_use_sizes_the_symbol_or_svg_it_draws`、`svg::nested_uses_draw_as_the_outer_use` |
| `<switch>` 只畫第一個條件成立的子元素；`systemLanguage`（`en-US`、`en`，不分大小寫，`en-GB` 這種加 `-` 的也算）、`requiredExtensions`（空的不成立，只認得 HTML 和 MathML）、`requiredFeatures` 不管；條件在 `<switch>` 外面也算 | `svg::a_switch_draws_its_first_child_whose_conditions_hold` |
| 另一個 `<svg>` 裡 `<use>` 複製的元素改了，`<use>` 所在的 `<svg>` 重抓 | `cargo test -p layout --test it update::an_svg_takes_its_content_again_when_what_its_use_copies_changes` |
| hit test：copy 打到的是 `<use>`，每一份 copy 在自己畫的地方（同一個元素的 copy 各有自己的 property node）、`<symbol>` 的 viewport 外打不到、`pointer-events: none` 的 `<use>` 打不到 | `cargo test -p paint --test it svg::a_use_is_hit_where_its_copy_is` |
| `dom`：`attribute_ns` 不管 prefix；`<use>` 的 copy 存在元素上、可以換掉，adopt 到別的 document 時拿掉；focus 和換頁認得 parser 給的 `xlink:href` | `cargo test -p dom --test it tree::attributes_by_namespace`、`computed_style::svg_use_instances`；`-p dom_api --test it focus::svg_elements_that_can_take_the_focus`、`events::an_svg_link_is_followed` |
| 畫出來：`<use>`、`<symbol>`、`<switch>`、copy 的規則、改了之後 | `python3 scripts/test.py reftest svg/`（`use`、`use-symbol`、`use-rules`、`switch`、`use-dynamic`） |
| WPT：`svg/linking/reftests/` 的 `href-use-element`、`use-descendant-combinator-001`、`-002`、`use-hidden-attr-change`、`use-nested-symbol-001`、`use-symbol-rendered-001`、`use-template`，`svg/struct/reftests/` 的 `use-data-url-setAttribute.tentative`、`use-referencing-non-svg-fragment-element`，`svg/styling/use-element-non-rendered-has-selector`、`-sibling-selector`；`<use>` 的六份 crashtest、`symbol-g-graphics-element.html` | `python3 scripts/test.py wpt svg/`（0 個不符） |
| **SVG 的幾何值**（第 37 條） | |
| `SVGMatrix` 的 `a` 到 `f`（double）、`multiply`、`inverse`（不能反轉時 `InvalidStateError`）、`translate`、`scale`、`scaleNonUniform`、`rotate`、`rotateFromVector`（有 0 時 `InvalidAccessError`）、`flipX`、`flipY`、`skewX`、`skewY`，乘在右邊、回傳新的；`SVGPoint`（float）的 `matrixTransform`；`SVGRect`（float）；`createSVGPoint()`、`createSVGMatrix()`、`createSVGRect()`，值是 Chrome 152 的 | `cargo test -p dom_api --test it api::svg_matrices_points_and_rects` |
| JS：三個是各自的 interface（`toString`、`instanceof`），沒有建構子，數字是有限的 `float` / `double`（NaN、Infinity、物件是 `TypeError`），`multiply` 只收 `SVGMatrix` | `cargo test -p dom_js --test it bindings::svg_matrices_points_and_rects_are_chromes` |
| `dom` 的 `SvgMatrix`、`SvgPoint`、`SvgRect`：GC 型別的共通檢查 | `cargo test -p dom --test it script_objects::svg_matrix`、`svg_point`、`svg_rect`、`script::every_type_unwraps_only_as_itself` |
| WPT：`svg/types/scripted/` 的 `SVGPoint.html`、`SVGRect.html`、`SVGRect-negative-dimensions.html` 整份通過，`SVGMatrix.html` 的 `multiply` 那幾項要 `DOMMatrix` 的（Chrome 也不過）；`idlharness` 的 `createSVGPoint()` 等六項 | `python3 scripts/test.py wpt svg/`（0 個不符） |
| **SVG 的幾何和 focus**（第 39 到 42 條） | |
| `getBBox()`、`getCTM()`、`getScreenCTM()`、client rect 的值照 Chrome 152 的實測：`<use>` 的 `x`、`y` 算進 bbox 不算進矩陣，`getCTM()` 到最近的 `<svg>`、`<symbol>`（巢狀的 `<svg>` 自己的包含它的 `viewBox`），`getScreenCTM()` 經過 border、padding、外面的 CSS transform、捲動容器和 viewport 的捲動、只到自己的 frame；隱藏的容器（`<defs>`、文件裡的 `<symbol>`、`display: none` 的 `<g>`）裡的有 bbox 和矩陣、沒有 client rect；`display: none` 的、`<switch>` 沒畫的、`<svg>` 外面的沒有 bbox 和 client rect，祖先的變換照算；沒有面積的圖形的 bbox 是它的位置、不算進容器的；`<tspan>` 的 bbox 包含裡面的元素；沒有字的 `<text>` 有原點的 client rect | `cargo test -p shell --test it geometry::svg` |
| focus：有連結的 SVG `<a>`、有 `tabindex` 的、有 focus listener 的圖形元素（`tabIndex` -1）可以 `focus()`；`visibility: hidden`、隱藏的容器裡的、沒有連結的 `<a>`、HTML 元素的 listener 不行；Tab 經過 SVG 的 `<a>` 和有 `tabindex` 的，不經過只有 listener 的和 `<defs>` 裡的；Enter 跟著 SVG 的 `<a>` 換頁 | `cargo test -p shell --test it geometry::svg_elements_geometry keys::tab_and_enter_reach_svg_links` |
| 看得到：Tab 到 SVG 的 `<a>` 時 focus ring 圍住它的圖形、Enter 換頁 | `python3 scripts/test.py scenarios svg` |
| `outline` 畫在元素自己的座標裡，圍住 fill 和 stroke 的一半、容器圍住子元素；focus 之後畫出來；隱藏的容器不畫 | `python3 scripts/test.py reftest svg/outline.html svg/focus-outline.html` |
| layout：`<defs>`、`<symbol>`、`display: none` 的 `<g>` 抓成隱藏的容器（`<use>` 複製的 `<symbol>` 裡的 `<symbol>` 也是）；路徑只有 move 時 bbox 在那一點（照 Skia 的 `computeTightBounds`） | `cargo test -p layout --test it svg::a_use_places_its_copy_and_draws_it_as_itself svg::a_use_sizes_the_symbol_or_svg_it_draws`、`cargo test -p gfx --lib a_paths_tight_bounds_hold_its_curves` |
| WPT：`svg/types/scripted/SVGGraphicsElement.getBBox-01`、`-04`、`getCTM-empty-viewBox`、`svg/geometry/svg-geometry-apis-in-non-rendered-elements`、`svg-get-bounding-client-rect-in-non-rendered-elements`、`svg/text/scripted/transform-dynamic-update` 整份通過；`getBBox-03`、`-05` 到 `-10`、`getScreenCTM` 的其他項照 Chrome 152（較新的 SVG 2 決議、`getBBox()` 的 options）或要 `<image>`、`foreignObject`、`DOMPoint`、CSS 的 `rotate()`；`tabindex-focus-flag.html` 多過 SVG 的四項，`idlharness` 多過 `SVGGraphicsElement` 的成員 | `python3 scripts/test.py wpt svg/ html/interaction/focus/`（0 個不符） |
| **長度和文字的方法**（第 43 到 45 條） | |
| 長度和長度上的點照 Chrome 152 的實測：圖形照 Skia（半徑 10 的圓 62.427、圓角矩形、橢圓、多邊形），`<path>` 照 Blink 的 traversal（弧、二次、三次曲線，close 之後的一段從起點走，只有 move 的長度 0），差在 Chrome 的最後一位以內 | `cargo test -p gfx --lib lengths_along_paths_are_chromes` |
| `getTotalLength()`、`getPointAtLength()`（夾住、空路徑和沒有 style 的 `InvalidStateError`、`pathLength` 不算、CSS 的 `d` 優先、沒畫出來的照 style 而百分比是 0、零寬的 `<rect>` 有路徑、不在文件裡的 `<path>` 照 `d`）、`isPointInFill()`、`isPointInStroke()`（字典或 `SVGPoint`、`<defs>` 裡的也算、`display: none` 的不算） | `cargo test -p shell --test it geometry::svg_shapes_lengths_and_points` |
| 文字的方法：UTF-16 的編號（emoji、組合字元）、`<tspan>` 自己的編號、長度和位置、旋轉的格子、`getCharNumAtPosition()`、超出範圍的 `IndexSizeError`、沒畫出來的 `<text>` | `cargo test -p shell --test it geometry::svg_text_methods` |
| WPT：`svg/path/` 的 13 份（量 path data）、`svg/text/scripted/` 的 6 份；沒過的是 `svgtypes` 的兩個解析差異、`textPath`、直書、`dominant-baseline`；`historical.html`、`idlharness` 多過的項目 | `python3 scripts/test.py wpt svg/path/ svg/text/scripted/`（0 個不符） |
| **XML parser**（第 47、51 到 53 條） | |
| SVG 檔解析成 DOM：namespace、prefix 的 attribute、文字、註解、PI、CDATA 當文字；BOM 和 `<?xml ?>` 不是節點；DOCTYPE 宣告的 entity 展開（Illustrator 的 `xmlns="&ns_svg;"`、互相引用、attribute 值裡；註解、CDATA、PI 裡不展開；外部、parameter、未宣告的照原樣；同名的第一個有效）；一直展開下去和自己引用自己的停下；格式錯誤照 xml5ever 修復 | `cargo test -p html --test it xml::` |
| **`<img>` 的 SVG**（第 48、54 到 56 條） | |
| 固有大小和框照 Chrome 152 的實測（`width`、`height`、只有 `viewBox`、只有一邊、百分比、`em`、CSS 的 `width` 不算、`width` 和 `height` 的比例先於 `viewBox`）；根元素不是 SVG 的 `<svg>` 是壞圖；`<picture>` 選 `image/svg+xml` 的 source；格式不對的照樣載入；不是 `image/svg+xml` 的不當成 SVG | `cargo test -p shell --test it images::` |
| `Replaced` 的每一邊的固有大小（一邊、比例、沒有） | `cargo test -p layout --lib replaced::tests::svg_images_size_by_what_they_have` |
| 畫出來：固有大小、拉伸到框（有固有大小時照它排版再縮放）、只有 `viewBox` 時照框排版、`<style>`、DOCTYPE 的 entity；2x 時是向量 | `python3 scripts/test.py reftest images/m7-svg.html hidpi/svg-image.html` |
| SVG 的 `<style>`（inline 的 SVG）和改它的文字 | `python3 scripts/test.py reftest svg/style-element.html` |
| XML 的編碼（BOM、header、`<?xml encoding?>`、UTF-8） | `cargo test -p renderer --lib encoding::tests::xml_encodings` |
| WPT：`fetch/corb/` 的五份 `img-svg-*`、`svg-img-with-external-stylesheet`、`svg/as-image/sprite-sheet-fractional-size`、`svg/styling/padding-on-svg-via-img.tentative`；`decode()`、`<picture>` 和 `content-type-with-parameters` 多過的項目 | `python3 scripts/test.py wpt fetch/corb/ html/semantics/embedded-content/the-img-element/ svg/`（0 個不符） |
| **背景的 SVG**（第 57 條） | |
| 照圖塊的大小排版：有固有大小時拉伸、沒有時照 px 放、只有 `viewBox` 時照 `preserveAspectRatio` 留邊、`contain`、重複的圖塊、`width` 和 `height` 的比例先於 `viewBox` | `python3 scripts/test.py reftest backgrounds/b4-svg.html` |
| `background-size` 對一邊、只有比例、都沒有的圖片（css-images 的 default sizing algorithm） | `cargo test -p paint --lib background` |
| WPT：`css/css-backgrounds/background-size/vector/` 整個目錄（`diagonal-percentage-vector-background` 的參考頁要 `<table>`）、`background-size-043`、`-044`、`background-size-contain-svg-view`、`background-size-cover-svg`、`background-size-near-zero-svg` | `python3 scripts/test.py wpt css/css-backgrounds/`（0 個不符） |

## 做過的檢查

- **決定**：第 1 到 7 條 2026-10-10 確認，第 8 到 57 條在每段和它的小段開始前（或實作中發現時）確認。和規劃不同、實作中改的：SVG 的 `transform`、`viewBox` 從留在 layout 的結果改成 property node（第 17 條，P1 改 P2）；`<use>` 的複本的 selector 照 Chrome 比對到複本的根為止（第 27 條）；`elementsFromPoint` 照 Chrome 不列 SVG 的容器（第 35 條，2b 原本照 WPT）；幾何的 API 照 Chrome 從 layout 算、不經過 property tree（第 39 條）；路徑長度從 `kurbo` 的精確弧長改成 Chrome 的算法（第 43 條）；SVG 圖片的大小從 paint 之前由 renderer 準備，改成 paint 記下沒錄的大小、錄好後再 paint（第 57 條）。理由寫在 `README.md`。
- **Chrome 152 的實測**（Chrome for Testing 152.0.7977.82，`--headless=new` 讀探測頁，畫面用截圖；每段開始前和實作中）：測試的預期值照它寫。記下的：每個 SVG 名稱的 interface（`clipPath` 分大小寫，沒有 `SVGUnknownElement`）、`tabIndex` 和能不能 focus（連同 focus listener 的特例）、`ownerSVGElement` 和 `viewportElement`；二十多個 property 的初始值、CSS 和 attribute 的值、computed value，哪些元素有哪些 presentation attribute（`transform`、幾何的只在規定的元素上）；`<svg>` 的大小（`viewBox` 撐滿寬、一邊給定時照比例）；文字的空白、位置、chunk、hit test 的格子；`elementsFromPoint` 對 SVG 列出什麼；`getBBox()`、`getCTM()`、`getScreenCTM()` 對 `<use>`、隱藏的容器、`display: none`、沒有面積的圖形的值；圓的長度 62.427（Skia）和同樣的弧寫成 path data 的 62.8408（Blink）、零寬的 `<rect>` 有路徑；文字的方法的 UTF-16 編號；SVG 圖片的固有大小、`naturalWidth`（300×150 的預設、`em` 是 16px、CSS 的 `width` 不算）、拉伸到框和圖塊、只有 `viewBox` 時照框排版；格式錯誤的 XML 是壞圖、DOCTYPE 的 entity 會展開。和 Chrome 不同、留著的都在 `docs/limitations.md`。
- **WPT 找到、修掉的**：`viewBox` 寬或高是 0 時 `svgtypes` 當成不合法、照畫（SVG 2 是不畫，自己讀四個數字）；`rect-hittest-*` 等測邊上的點，`kurbo` 的 winding number 在右邊和下面的邊是 0（點在外框上也算在裡面，照 Skia 的 `contains`）；`url(#%66%6f%6f)` 要先 percent-decode 再找 id（`dom::fragment_id`）；`use-nested-symbol-001`：複本裡的 `<symbol>` 不畫；`SVGPoint`、`SVGRect` 的數字第一版用了 `unrestricted`；`CSS-supports-CSSStyleDeclaration.html` 碰到 `inset` shorthand 序列化時 panic（`Shorthand::name()` 沒有它，原本就有的 bug）。
- **這份 RFC 找到的其他既有 bug**：`<svg>` 外面的 SVG 元素（`document.body.append(circle)`）有一個 CSS 的盒子，照 Blink 的 `SVGElement::LayoutObjectIsNeeded` 改成沒有；比例極端的圖片 `contain` 之後的圖塊（1.2e-7 px 高）對齊到最少一個 device pixel、畫出一條線，照 Blink 以 `LayoutUnit`（1/64 px）計算，小於它是空的；`shell` 的 `sites::sqlite_about_loads_its_stylesheet` 原本預期 SQLite 的 SVG logo 載入失敗；`docs/limitations.md` 說 SVG 元素沒有 `style`（1a 漏改）。
- **WPT**：`svg/` 的 testharness（`historical.html`、`idlharness.window.js`、`struct/scripted/`、`types/scripted/`、`geometry/`、`path/`、`text/scripted/`、`interact/scripted/`）、HTML 的 reftest 和 crashtest（試跑 `svg/` 底下 139 份 HTML reftest 和每段的候選，兩邊都沒畫出東西才通過的不收）、`css/css-transforms/` 用漸層的 63 份、`css/css-backgrounds/background-size/vector/`、`fetch/corb/` 的 `img-svg-*`、`the-img-element/` 的 SVG，還有 `CSS.supports`、`CSS.escape`、`visibility` 的；清單裡受影響的目錄每段重跑，預期結果照實更新。`svg/` 大多數是 `.svg` 檔（頂層的 SVG 文件，第 50 條）或用 Window 的具名屬性（第 21 條），跑不了；收不了的原因寫在各段的 `.ini` 和 `docs/limitations.md`。
- **P2 的量測**（第 17 條；暫時的測試頁，release build）：每個圖形一個 `<g transform>`，1000 個時 pre-paint 0.12 ms、paint 0.22 ms、轉成 display list 0.20 ms；5000 個時 0.36、1.0、0.89 ms。style 和建 box 最多（1000 個 7.2 ms、5000 個 28.6 ms），多出來的 node 和 chunk 不是瓶頸。`<use>` 的複本上限（一萬個元素）的測試疊 40 層，要 1.8 秒；一般的複本不慢（9000 個元素多不到 1 ms）。
- **Speedometer**（`--reuse-chrome`，第 1 段之後）：TodoMVC-Vue 跑完 3 個 test；NewsSite-Nuxt 過了 `SVGElement`，停在 `crypto.getRandomValues`。
- **mutation test**（`mutants-1a.toml` 到 `mutants-4c.toml` 和 `mutants-shapes.toml`，共 19 份：1a 16、1b 10、2a-0 23、2a-1 47、2a-2 44、2b 21、2c 39、2d-0 16、2d 43、2e-0 5、2e 30、3a 19、圖形改用 `gfx::Path` 的方法 4、3b 35、3c 29、4a 14、4b 14、4c 8 條）：全部抓到。補了測試之後才抓到的約 30 條，各段補的寫在 git 的紀錄裡；值得記的幾類：
  - reftest 的參考頁也是 kun 畫的，幾何對稱的測試看不出方向和先後（橢圓逆時針、漸層的 transform 和 tile 的先後、容器的 outline 不算子元素的 transform），要不對稱的例子；
  - 測試的輸入剛好落在邊界上（圓的 conic 控制點在外接正方形上、`<title>` 本來就是 `display: none`、Ahem 的 tab 和空格一樣寬），要換輸入；
  - 拿掉的多餘程式（等價的 mutant）：漸層的 reached 裡的 `<stop>`、`getPointAtLength()` 的上限和 Skia 那邊的下限、非有限的點、文字和路徑多餘的夾住、XML 的 parameter entity 判斷、SVG 固有大小裡由兩邊算比例的分支；
  - `dom_wasm` 的 linker 替只給 JS 的 interface 定義 resource 是等價的，記在 `docs/testing.md`。
- **spec 的原文**：`cargo fmt` 和後面的步驟改了程式碼之後，前面各步的 `mutants-*.toml` 有些原文對不上（`test.py mutate` 開始前會整份檢查）：它們是各步做的時候的紀錄，要重跑時先照現在的程式碼改原文。
- **sanity**：`boundary` 抓到兩次 renderer 這一側的全域狀態：`html_element.rs` 的 `static SVG_TAG_KINDS`（從靜態的宣告表算出來，列進允許的清單）、SVG 圖片編號的 `static` 計數器（改成文件的 id 加上 `ImageTable` 自己的計數）。
- **不穩定的測試**：2b 時改到的 crate 一起跑，`shell` 的 `webdriver::a_crashed_tab_is_told` 失敗一次（machopodriver 對 `navigate("about:crash")` 回了成功），單獨跑三次都通過；和 hit test、activation 無關，看起來是 `driver` 的 `wait_for_load` 和送出 `TargetCrashed` 的先後，另開工作找原因。
- **補回的紀錄**：「做過的檢查」在 2a-0 的 commit（解 rebase 衝突時）整節不見過，2c 時從工作紀錄補回。
- **各段做完時的驗證**（macOS Apple Silicon，Rust 1.99.0）：第 1 段 `cargo test --workspace` 1889 個、第 2 段 1988 個、第 3 段 2080 個全部通過，`cargo fmt --check`（連同 `sdk/lua/generator`）和 clippy 抓到的格式、`drop` 等都在那一段修掉；改到的 crate 的測試、相關的 `test.py reftest`、`wpt`、`scenarios svg` 和 `sanity.py` 的五項每一小段都跑，0 個不符。`test.py automation` 沒有跑（沒有動到視窗那一層）。
- **完成時的驗證**（2026-10-11，macOS Apple Silicon，Rust 1.99.0）：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`（抓到 `SvgImage::new` 對 `DomReadScope` 呼叫 `drop`，刪掉）、`cargo test --workspace` 2090 個全部通過；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`。第 4 段的 `test.py reftest images/ svg/ backgrounds/ paint/ hidpi/`（56 個）、`test.py wpt fetch/corb/ html/semantics/embedded-content/the-img-element/ svg/ css/css-backgrounds/`，都是 0 個不符。rebase 到 canvas 第 2 段之後的 `main`（`docs/architecture.md` 的 `renderer`、`raster` 兩列有衝突，兩邊的內容都留）重跑：fmt、clippy 沒有問題，`cargo test --workspace` 2123 個全部通過，`test.py reftest images/ svg/ backgrounds/ paint/ hidpi/ text/`（66 個）、`test.py wpt fetch/corb/ html/semantics/embedded-content/the-img-element/ svg/ css/css-backgrounds/ html/canvas/`（1156 個）、`test.py scenarios svg canvas`，都是 0 個不符。整套的 `reftest`、`wpt`、`scenarios`、`matrix`、`fuzz` 由每天的 CI 跑。
