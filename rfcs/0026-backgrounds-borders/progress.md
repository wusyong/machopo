# RFC 0026 的進度：背景和邊框

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-09。完成：2026-10-10，Fedora 44（Linux x86-64，Rust 1.99.0）。四段都在 `0026` 上做，每一段做完時 commit、審核；第 1、2 段先 rebase 到 `main`、merge 回去，第 3、4 段接著在同一個 branch 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 背景圖和漸層（第 4 到 8 條） | 完成，merge 回 `main` | 2026-10-09 |
| 2. `border-radius`（第 9 條） | 完成，merge 回 `main` | 2026-10-10 |
| 3. 陰影和 outline（第 10 到 12 條） | 完成 | 2026-10-10 |
| 4. border style（第 13、14 條） | 完成 | 2026-10-10 |

## 驗收

每一段做完時跑 `CLAUDE.md` 列的檢查：`cargo fmt --check`、clippy、`cargo test --workspace`、`sanity.py` 的四項、`test.py reftest`、`wpt`、`scenarios`；第 2 段起（動到 Viz 和視窗裡看得到的行為）也跑 `automation`。

| 項目 | 怎麼驗證 |
| --- | --- |
| **style** | |
| 背景的 8 個 longhand 和 `background`、`background-position` shorthand 的解析：多層、一個 box 同時是 origin 和 clip、顏色只在最後一層、`<bg-position>` 的一到四個值、`url()`、`linear-gradient()` / `radial-gradient()` 和 `repeating-`（角度、邊角、形狀和大小、色停點的兩個位置、hint） | `cargo test -p style --lib background`、`--test cssom background_serialization` |
| cascade：shorthand 重設沒寫的 longhand、清單照圖的層數循環補齊、`url()` 對宣告所在的 base URL 解析（`<style>` 和 `style` attribute 對文件的，外部 stylesheet 對它自己的，base URL 變了整份重算）、漸層的長度和預設值 | `cargo test -p style --test cascade`（`background_shorthand_resets_every_longhand`、`background_lists_repeat_to_the_number_of_images`、`urls_resolve_against_where_they_are_declared`、`gradients_compute_their_lengths_and_defaults`） |
| 序列化：`element.style` 照 Blink 的寫法（初始值不寫、size 帶著 position），`getComputedStyle` 的 longhand（關鍵字變百分比、`calc(100% - …)`、絕對網址、漸層省略預設值）和 `background` shorthand | `cargo test -p style --test cssom background_serialization`、`--test computed background_values`、`shorthands_come_from_their_computed_longhands` |
| 背景的改變只重畫、不重排 | `cargo test -p layout --test update what_only_paints_changes_in_place` |
| `border-radius` 的四個 longhand 和 shorthand（一到四個值、`/` 之後的垂直半徑、負值無效）、`element.style` 和 `getComputedStyle` 的序列化；只改圓角只重畫 | `cargo test -p style --test cssom border_radius_serialization`、`--test computed border_radius_values`、`-p layout --test update what_only_paints_changes_in_place` |
| `box-shadow`、`outline-*` 和 `outline` shorthand 的解析和序列化（Blink 的順序：陰影是顏色、長度、`inset`，outline 是顏色、樣式、寬度）、`-webkit-focus-ring-color`；沒寫寬度的邊框和 outline 是 `medium`（`initial` 也是），outline 的寬度在樣式是 `none` 時不歸零；只改陰影和 outline 只重畫；UA 的 `:focus-visible` 讓鍵盤移動 focus 時重算 style | `cargo test -p style --test cssom box_shadow_and_outline_serialization`、`--test computed box_shadow_and_outline_values`、`--test invalidation a_focus_change_styles_again_only_what_rules_use`、`-p layout --test update what_only_paints_changes_in_place` |
| 邊框和 outline 的樣式：`hidden`、`dotted`、`dashed`、`double`、`groove`、`ridge`、`inset`、`outset` 的解析和序列化；`hidden` 的寬度歸零 | `cargo test -p style --test cssom box_shadow_and_outline_serialization`、`--test computed box_shadow_and_outline_values` |
| **paint 和 raster** | |
| 顏色照最底層的 clip、每一層由下往上、各自的 clip / origin / position / size / repeat；根元素或 `<body>` 的背景畫滿 canvas、照根元素的盒子定位；inline 元素每一行各畫一次；背景的盒子對齊取整過的邊框 | `cargo test -p paint --test artifact`（`background_layers_paint_bottom_up_in_their_boxes`、`the_root_background_covers_the_canvas_placed_in_the_root_box`、`an_inline_element_paints_its_background_on_each_line`、`a_background_meets_its_snapped_border`） |
| tile 的大小（`cover`、`contain`、一軸 `auto`）、`space` 和 `round`、tile 取整到 device pixel；漸層的線、色停點的 fixup、hint、repeating、放射的大小 | `cargo test -p paint --lib background` |
| 背景圖載到時，用到它的元素（和 inline 元素所在的 block）不沿用上一次的 display item | `cargo test -p paint --test artifact a_loaded_background_image_paints_its_element_anew` |
| Skia 的 shader：圖片的 tile 兩個方向重複、有間隔的 tile、漸層沿著線和當成 tile 重複、premultiplied 內插、橢圓 | `cargo test -p raster` |
| 圓角的矩形：角重疊時一起縮放、往內縮時半徑減掉邊框、點在不在曲線裡 | `cargo test -p gfx rounded_rects` |
| 圓角的背景（顏色和每一層，畫滿圓角的範圍、tile 外面不畫）和邊框（同色的環、其他的每邊切成梯形）；不透明的圓角邊框底下的背景退進邊框一半；`overflow` 的圓角 clip 進 display list；inline 元素的圓角只在頭尾那一行、content box 照 layout 給的 padding | `cargo test -p paint --test artifact`（`rounded_backgrounds_and_borders`、`a_background_under_a_rounded_opaque_border_stops_halfway_into_it`、`rounded_clips_go_into_the_display_list`、`an_inline_element_rounds_only_where_it_starts_and_ends`、`an_inline_elements_content_box_is_inside_its_padding_where_it_starts_and_ends`）、`cargo test -p raster decal` |
| 陰影：外陰影在背景之前、不畫在盒子底下，內陰影在背景之後、padding box 裡，第一個在最上面；spread、offset、模糊（標準差是 blur 半徑的一半）、visual rect 含模糊；inline 元素也有 | `cargo test -p paint --test artifact box_shadows_paint_around_and_inside_the_box`、`-p gfx rounded_rects`、`-p raster a_shadow_is_blurred_and_clipped_to_its_side_of_the_box`、`-p compositor_paint a_shadows_bounds_take_in_its_blur_or_stay_in_its_clip` |
| outline：行之後畫、在盒子外面 offset 的地方、跟著圓角，整個畫的 inline-block 和 flex item 在它們畫的地方畫一次；focus ring 是深色和白色兩圈；outline 每次重畫，沿用上一次的盒子時也在 | `cargo test -p paint --test artifact`（`outlines_paint_after_the_lines_around_their_boxes`、`a_focus_ring_is_a_dark_ring_and_a_white_one`）、`--test paint_controller outlines_are_painted_around_boxes_taken_from_the_last_paint` |
| 邊框的樣式：`double` 兩條、各三分之一（整數的 device pixel，太細時一條），`inset` / `outset` / `groove` / `ridge` 照 Blink 調深淺（已經夠暗或夠亮的不變，黑色變亮是 `rgb(84, 84, 84)`），虛線和點線沿著每一邊的中線、切成梯形、間隔調到落在角上（closed path 和 open path 不同、太短時是實線），圓角的沿著整圈；四邊一樣時是整個環，少一邊的直角盒子也是，透明的一邊不是；outline 的實線用同一套 | `cargo test -p paint --test artifact`（`border_styles_draw_their_bands_and_shades`、`shades_and_double_lines_follow_blink`、`dashes_and_dots_fit_their_sides`、`dash_lengths_follow_the_width_and_the_path`、`an_outline_line_is_drawn_as_a_border_of_its_style`、`border_widths_snap_down_to_whole_device_pixels_but_not_to_none`、`a_rounded_border_with_a_side_missing_is_its_sides`）、`-p raster`（`dashes_leave_gaps_and_dots_are_round`、`a_clipped_op_draws_only_inside_its_polygon`）、`-p compositor_paint a_clipped_ops_bounds_are_its_ops` |
| hit test：圓角外面和圓角 clip 外面打不到；兩層圓角 clip 底下的捲動容器不成為 layer | `cargo test -p paint --test hit_test rounded_corners_are_not_hit_outside_their_curves`、`--test layerize a_scroller_is_a_layer_under_one_rounded_clip_not_two` |
| **compositor 和 Viz** | |
| 成為 layer 的圓角捲動容器：quad 帶著圓角、捲動時圓角留在原地；software renderer 照圓角切掉角落、邊緣抗鋸齒；wgpu 的 shader 是合法的 WGSL | `cargo test -p compositor a_rounded_container_layers_quads_carry_its_corners`、`-p viz`（`a_rounded_clip_leaves_its_corners_out`、`inside_the_curves_is_covered_outside_isnt`、`the_shader_is_valid_wgsl`） |
| **renderer** | |
| 背景圖和 page 一起載入、延後 `load`（只在 stylesheet 裡的也一樣）、stylesheet 的 `url()` 對它自己的網址、同一個網址只載一次、`display: none` 的不載；`<base>` 變了之後重新設定的 `url()` 用新的 base URL | `cargo test -p shell --test network background_images_load_with_the_page`、`--test geometry background_urls_resolve_against_the_base_url` |
| **WPT 和 reftest** | |
| `css/css-backgrounds`、`css/CSS2/backgrounds`、`css/css-images` 的 286 個 reftest 候選，和之前的目錄裡因為背景才被擋掉的 50 個，322 個通過、14 個照預期失敗（原因都不是背景本身）；自己的三個 reftest（頭像的 `cover` / `contain`、漸層的銳利邊緣和 tile、多層和 box、`space` / `round`） | `python3 scripts/test.py reftest`（`backgrounds/`、`wpt/css/css-backgrounds` 等） |
| 背景 longhand 的 `-valid` / `-invalid` 共 18 個，14 個整份通過；`background-shorthand-serialization.html`、`background-valid.html`、`serialize-values.html` 的失敗大多消失 | `python3 scripts/test.py wpt css/` |
| `border-radius` 放行的 36 個 reftest 候選（`css/css-backgrounds` 34 個、`css/css-overflow` 2 個），35 個通過、`clip-008` 照預期失敗（`overflow: clip` 算成 `hidden`）；自己的三個 reftest（圓的頭像和邊框裡的橢圓 clip、圓角的邊框、成為 layer 的圓角捲動容器） | `python3 scripts/test.py reftest backgrounds/r wpt/css/css-backgrounds wpt/css/css-overflow` |
| 陰影和 outline 放行的 82 個 reftest 候選，72 個通過、10 個照預期失敗（原因記在 `reftest-expectations.txt`）；自己的兩個 reftest（陰影的 offset、spread、多層和內陰影，outline 的 offset、圓角和負的 offset） | `python3 scripts/test.py reftest backgrounds/s1 backgrounds/o1 wpt/css/css-ui wpt/css/CSS2/ui wpt/css/css-backgrounds` |
| 邊框樣式放行的 47 個 reftest 候選，43 個通過、3 個照預期失敗、1 個 skip（原因記在 `reftest-expectations.txt`）；自己的 reftest（`double`、`inset` 的深淺、`hidden`）；iframe 的 UA 邊框照 `inset` 畫 | `python3 scripts/test.py reftest backgrounds/t1 frames/f1 wpt/css/css-backgrounds wpt/css/css-ui wpt/css/css-flexbox` |
| 陰影和 outline 的 `-valid` / `-invalid` / `-shorthand` 共 13 個，6 個整份通過；有了邊框的樣式之後 `border-style-valid.html`、`border-valid.html`、`outline-style-valid.html` 整份通過 | `python3 scripts/test.py wpt css/css-backgrounds/parsing/box-shadow css/css-ui/parsing` |
| `border-radius-valid.html` 只有 `revert` 失敗、`-invalid.html` 整份通過 | `python3 scripts/test.py wpt css/css-backgrounds/parsing/border-radius` |
| **視窗裡** | |
| `about:demo` 的「Restyle the card」：漸層的卡片換成兩層（角落的小圖蓋在斜條紋上），`getComputedStyle` 讀回兩層的 `background-size`；頭像的圖 `cover` | `python3 scripts/test.py scenarios backgrounds` |
| focus ring：點了的元素沒有（`:focus-visible` 不成立），Tab 移到連結時有 UA 的 focus ring；卡片的陰影；`about:demo` 的卡片有陰影 | `python3 scripts/test.py scenarios focus-ring`、`automation focus-ring`（截圖） |
| 圓的元素：點在它的盒子裡、曲線外面的點打到底下的元素，裡面的打到它；圓角的捲動容器（layer）用滾輪捲，compositor 照圓角切；`about:demo` 的卡片有圓角、頭像是圓的 | `python3 scripts/test.py scenarios rounded`、`automation rounded`（截圖在 `target/automation-results/`） |

## 做過的檢查

- **決定**：第 4 到 14 條，每一段開始前確認（G1、H1、I1、J1、K1；D1b；F1、L1、M1；E1、N1）。各段和規劃不同的地方寫進了 `README.md` 的「詳細設計」。
- **WPT 找到的 bug**（修了才匯入）：
  - 背景：padding box 用 layout 沒取整的邊框寬度，和取整過的邊框之間差一個 pixel（`background-clip-padding-box-001`、`background-position-subpixel-at-border.tentative`）；tile 沒有取整（`subpixel-repeat-no-repeat-mix`）；週期是 0 的 repeating 漸層取平均（`normalization-*-degenerate`、`-radial-4`）；停點都在同一處時漸層的線縮到 0.001px，銳利邊緣上的 pixel 由捨入決定（`linear-gradient-non-square`）；`background` shorthand 的序列化原本照「shorthand 有沒有省略」，Blink 照「是不是初始值」（`background-shorthand-serialization.html`）。
  - 圓角：背景和不透明的邊框用同一條外側曲線，邊框抗鋸齒的 pixel 讓底下的背景透出來，照 Blink 的 bleed avoidance 修（`background-clip-*-with-border-radius-003`、`background-image-centered-with-border-radius`）。
  - outline：寬度在樣式是 `none` 時被歸零，`inherit` 拿到 0（`outline-009`、`CSS2/ui/outline-width-096`）；負的 offset 讓它翻過來時沒有保持兩倍的寬度（`outline-013` 到 `-016`）。
- **這份 RFC 找到的既有 bug**：cascade 的邊框寬度從 0 開始，`border-style: solid` 沒寫寬度時是 0px（Chrome 是 `medium`，3px），改成從 `medium` 開始（`initial` 也是），`finish` 照舊把 `none`、`hidden` 的歸零；inline 元素的 `content-box` 不算百分比的 padding、不知道哪一側是頭尾（layout 多交出那一行的 padding 和頭尾）。
- **候選腳本**：一段一段放行背景的 longhand、`url()` 和兩種漸層，`border-radius`，`box-shadow` 和 outline，最後是所有邊框樣式；多擋 `min()`、`max()`、`clamp()`、`border-area`、`outline-offset: inset`（kun 不解析）。整個 `css/` 一共收進 336、36、82、47 個候選；過不了的都記在 `reftest-expectations.txt`，大多不是這份 RFC 的東西（`position` 的三個 bug、grid item 裡照格線放的絕對定位盒子、`overflow: clip` 只切一軸、taffy、一個 tentative 的小數 pixel 背景、用到 window 的 named property 的 script），屬於這份的是 inline 元素的陰影沒有 `slice`、block 的 focus ring 不圍住超出它的子孫、`double` 的圓角抗鋸齒超過容差。
- **testharness**：背景 longhand、`border-radius`、陰影和 outline 的 `-valid`、`-invalid`、`-shorthand`；`-computed` 不收（每一項都先呼叫 `CSS.supports`），`canonical-order-outline-sub-properties-001.html` 不收（它期待舊的順序和 `invert`）。既有的 baseline 跟著變好的，每一段更新（`serialize-values.html`、`cssom-setProperty-shorthand.html`、`cssstyledeclaration-csstext-all-shorthand.html`、`elementFromPoint.html` 的 fieldset、`getComputedStyle-resolved-colors.html` 的 `outline-color`、`border-*`、`outline-*` 的 parsing）。
- **一處不照 Chrome**：`<base>` 變了之後，下一次重算 style 時整份文件的 `url()` 都用新的 base URL；Chrome 只有重新設定的宣告才用新的。記進 `docs/limitations.md`。
- **reftest 的容差**：圓角的 reftest 只差在曲線的抗鋸齒（Skia 的抗鋸齒 clip 是比較粗的遮罩；Viz 照 pixel 中心到曲線的距離算，和解析解一樣），fuzzy 放寬到量到的差異，最大差值都在 100 以下，曲線錯一個 pixel 就會超過。
- **mutation test**（`mutants-1.toml` 到 `-4.toml`，42、44、36、30 條）：全部抓到。補了測試之後才抓到的有 2、9、3、10 條，各段補的測試寫在 git 的紀錄裡；值得記的幾類：reftest 的參考頁也是 kun 畫的，顏色、寬度和 dash 的參數錯了兩邊一起錯，要靠單元測試（第 4 段的 10 條）；情境只比對文字和畫面變了沒有，`expect-differs` 要選在只有那個變化的地方（focus ring 的 `focus-ring`、沒有圓角 clip 的卡片的 `backgrounds`）；`DrawShadow`、`DrawClipped` 的 op 範圍只有 `push_op` 用到，補了 `compositor_paint` 的測試。等價的一條：`Color::Dark` 把白色寫死成 `rgb(171, 171, 171)`，公式算出來一樣，拿掉了。wgpu 的 shader 拿掉曲線（只剩矩形）沒有自動檢查抓得到，記進 `docs/test-gaps.md` 的「只靠審查」。
- **不穩定的測試**：`scenarios` 和 `automation` 裡的 `wasm-demo`（第 14 行等時鐘顯示 1 秒，看到時已經是 12 秒）：在沒有這份 RFC 的 `main`（`c279e64`）上單獨跑三次也失敗兩次，是 RFC 0023 之後 `main` 上既有的，原因還沒查。第 1 段的一次 `cargo test --workspace` 裡 `browser` 的 `a_click_on_the_old_page_doesnt_reach_the_new_one` 等載入等到上限，單獨跑都通過，和 RFC 0013 記下的是同一個。
- **既有的 site 測試**：`shell` 的 `gutenberg_alice_decodes_every_image`：存下來的 Gutenberg 頁面的 CSS 用到一張存檔裡沒有的背景圖，現在 kun 會去載而失敗；測試改成只容許這一個。
- **完成時的驗證**（2026-10-10，Fedora 44，Rust 1.99.0）：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`（沒有警告）、`cargo test --workspace` 1738 個全部通過、`sanity.py deps`、`boundary`、`unsafe`、`docs`（只有既有的 RFC 0021 那一項：它的 `progress.md` 寫到一個這台機器沒有的第三方目錄）。`test.py reftest`（1226 個通過、103 個照預期失敗、2 個 skip、0 個 unexpected）、`wpt`（1945 個跑、223 個 skip；八份 baseline 更新之後 0 個不符）、`scenarios`（41 runs，只有 `wasm-demo`）、`automation`（33 個情境 32 個通過，只有 `wasm-demo`；`pipe --window` 通過；`rounded` 和 `focus-ring` 的截圖看過）。沒有動到 GC（`matrix`、`fuzz` 不用跑）和 machopodriver（`wdspec`、`smoke` 不用跑）。
