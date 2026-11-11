# 要回報上游的草稿

六份，**都還沒送出**：照 `CLAUDE.md`，對上游開 issue 要先給人審。每份下面的英文是要貼出去的內容；重現都對照 `linebender/vello` 的 main `112736df`（2026-11-10）。送出之後把 issue 的編號寫進 `docs/render-gaps.md` 的那一列和程式裡的 `vello:` 註解。

送之前要確認的：

- 每份先在上游找一次有沒有人回報過（寫草稿時只對過 `docs/render-gaps.md` 列的那幾個 issue）。
- 第 3 份的重現只在 machopo 的 playback 裡跑過（WPT 的 `html/canvas/element/line-styles/2d.line.cross.html`），還沒有縮成只用 kurbo 的程式；送之前先縮，確認是 kurbo 的 stroker 而不是 Vello 這一側。
- 第 5、6 份是 atlas 的：machopo 沒有開 atlas（RFC 的已決定的事項第 46 條），重現用的程式在 `atlas/` 的 patch 裡。
- 第 6 份只用 Vello 重現過，但那時的程式沒有留下來，下面的是照做法重寫的：送之前先跑一次，把次數換成跑到的。
- 第 5 份只在 machopo 的 playback 裡重現過（`atlas/machopo-raster.patch` 裡 `raster` 的 `a_glyph_the_atlas_has_upright_is_drawn_as_its_run_has_it`），下面的程式是照 playback 做的事寫的，沒有單獨對 Vello 跑過：送之前先跑一次。atlas 在上游標成實驗性，先看上游是不是已經知道。
- 第 4 份可能是設計如此，不是 bug：寫成問題。它的程式是照 playback 做的事寫的（測試是經過 `raster::draw_into` 跑的），沒有單獨對 Vello 跑過：送之前先跑一次。

## 1. vello_common：被 clip 的 stroke 從 context 的右邊緣開始時，clip 的交集減出負數

對應 `raster` 的 `upstream_panics_on_a_clipped_stroke_at_the_right_edge`、`Player::shows`。

> **Title:** vello_cpu: `attempt to subtract with overflow` in `clip.rs` for a clipped stroke that begins at the context's right edge
>
> With overflow checks on, this panics at `vello_common/src/clip.rs:723` (`cur_strip_fill_area`, `let width = next.x - x;`); without them the width wraps.
>
> ```rust
> let width = 32u16; // 256 does it too
> let mut ctx = RenderContext::new(width, 256);
> let edge = f64::from(width);
> let mut clip = BezPath::new();
> clip.move_to((edge + 3.0, -3.0));
> clip.line_to((edge + 3.0, 11.0));
> clip.line_to((edge, 8.0));
> clip.line_to((edge, 0.0));
> clip.close_path();
> let mut line = BezPath::new();
> line.move_to((edge + 1.5, -3.0));
> line.line_to((edge + 1.5, 11.0));
> ctx.push_clip_path(&clip);
> ctx.set_paint(Color::BLACK);
> ctx.set_stroke(Stroke::new(3.0).with_caps(Cap::Butt).with_join(Join::Miter));
> ctx.stroke_path(&line);
> ctx.pop_clip();
> ```
>
> Both the clip and the stroke are wholly past the right edge and begin exactly at it. Moving the line one pixel left (so that it touches the last column) or giving it round caps doesn't panic.
>
> Where it comes from: a browser draws each side of a box's border as a line clipped to a trapezoid. When a border lies along a tile's edge, the next tile gets exactly this.

## 2. 漸層：最後兩個色標都在 offset 1 時，超過終點的地方用錯顏色

對應 `raster` 的 `upstream_pads_a_gradient_past_a_hard_stop_with_the_color_before_it`、`gradient`。

> **Title:** vello_cpu: a gradient with a hard stop at offset 1 pads with the color before the stop
>
> ```rust
> let (red, blue) = (Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255));
> let stops = [
>     ColorStop::from((0.0, red)),
>     ColorStop::from((1.0, red)),
>     ColorStop::from((1.0, blue)),
> ];
> let mut gradient = Gradient::new_linear((0.0, 0.0), (10.0, 0.0)).with_stops(&stops[..]);
> gradient.extend = Extend::Pad;
> let mut ctx = RenderContext::new(32, 4);
> ctx.set_paint(gradient);
> ctx.fill_rect(&Rect::new(0.0, 0.0, 32.0, 4.0));
> ```
>
> The pixel at x = 25, past the gradient's end, is red; I'd expect blue, the last stop's color (what Skia and the browsers draw for CSS's `linear-gradient(to right, red 10px, blue 10px)` in a box wider than 10px). A hard stop anywhere before offset 1 is drawn right; so it looks like the padding takes the color looked up at offset 1, which finds the first of the two stops there.
>
> The same happens with a radial gradient.

## 3. kurbo：線寬遠大於線段長度時，bevel 接合的內側多畫一塊

對應 WPT 的 `2d.line.cross`（預期結果標成失敗）。**還沒縮成只用 kurbo 的重現**。

> **Title:** stroke: a bevel join of segments much shorter than the stroke is wide fills inside the join
>
> ```rust
> let mut path = BezPath::new();
> path.move_to((110.0, 50.0));
> path.line_to((110.0, 60.0));
> path.line_to((100.0, 60.0));
> let style = Stroke::new(200.0).with_join(Join::Bevel).with_caps(Cap::Butt);
> let outline = kurbo::stroke(path, &style, &StrokeOpts::default(), 0.1);
> ```
>
> Filled (non-zero), `outline` covers (48, 48), which is outside both segments' rectangles ((10, 50)–(210, 60) and (100, -40)–(110, 160)) and outside the bevel's triangle. The HTML canvas test `2d.line.cross` expects nothing there, and Skia draws nothing there.

## 4. Vello：會清掉 layer 沒蓋到的地方的 blend mode，不受 layer 所在的 clip 限制

對應 `raster` 的 `upstream_doesnt_keep_a_clearing_layer_to_its_clip`。

> **Title:** vello_cpu: is a layer's compose mode meant to apply outside the clip the layer is pushed in?
>
> ```rust
> let mut ctx = RenderContext::new(10, 10);
> ctx.set_paint(Color::from_rgb8(0, 255, 0));
> ctx.fill_rect(&Rect::new(0.0, 0.0, 10.0, 10.0));
> ctx.push_clip_path(&Rect::new(2.0, 2.0, 6.0, 6.0).to_path(0.1));
> ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::SrcIn)), None, None, None);
> ctx.set_paint(Color::from_rgb8(255, 0, 0));
> ctx.fill_rect(&Rect::new(0.0, 0.0, 3.0, 3.0));
> ctx.pop_layer();
> ctx.pop_clip();
> ```
>
> The pixel at (8, 8), outside the clip, comes out transparent; I expected it to stay green. The same with `SrcOut`, `DestIn` and `DestAtop`: the modes that change the backdrop where the layer has nothing.
>
> An HTML canvas needs the clip to bound them (`ctx.clip(); ctx.globalCompositeOperation = 'source-in'; ctx.fill()` must leave what is outside the clip alone). Is there a way to get that, short of rendering the clipped part separately?

## 5. glifo：斜的或 matrix 不等比例的 run，從 atlas 拿到同一個 scene 裡剛放進去的直的 glyph

對應 `atlas/machopo-raster.patch` 裡 `raster` 的 `a_glyph_the_atlas_has_upright_is_drawn_as_its_run_has_it`、`draw_glyphs` 裡不開 atlas 的判斷。

> **Title:** glifo: with the atlas cache, a skewed or non-uniformly scaled run takes the upright glyph an earlier run of the same scene cached
>
> ```rust
> let mut resources = Resources::new();
> let mut ctx = RenderContext::new(80, 40);
> let glyph = [Glyph { id, x: 0.0, y: 0.0 }];
> ctx.set_paint(Color::BLACK);
> ctx.set_transform(Affine::translate((10.0, 30.0)));
> ctx.glyph_run(&mut resources, &font)
>     .font_size(24.0)
>     .hint(false)
>     .atlas_cache(true)
>     .fill_glyphs(glyph.into_iter());
> // Half as wide; the same happens with a rotation, a vertical scale, or
> // `.glyph_transform(Affine::skew(-0.36, 0.0))` under the first transform.
> ctx.set_transform(Affine::translate((50.0, 30.0)) * Affine::scale_non_uniform(0.5, 1.0));
> ctx.glyph_run(&mut resources, &font)
>     .font_size(24.0)
>     .hint(false)
>     .atlas_cache(true)
>     .fill_glyphs(glyph.into_iter());
> ctx.flush();
> ctx.render(&mut target, &mut resources);
> ```
>
> The second glyph comes out as wide as the first. Drawn alone, or after the first has been rendered (a second scene with the same `Resources`), it is drawn half as wide, and such a run adds nothing to the atlas; so it looks like the lookup isn't kept from runs whose glyphs wouldn't be cached, and finds the entry the first run has just added (same font, size and subpixel position).
>
> Where it comes from: an HTML canvas's `fillText(text, x, y, maxWidth)` squeezes the text with such a transform, next to text drawn upright.

## 6. glifo：同一個 glyph 放在 atlas 的不同位置，邊緣的像素差 1

machopo 不採用 atlas 的原因。每個 raster thread 有自己的 atlas，測試頁和參考頁的同一段文字因此對不上。

> **Title:** glifo: a glyph's pixels depend on where in the atlas it was put
>
> With `atlas_cache(true)`, the same glyph at the same size and subpixel position can come out one level different in a column of edge pixels, depending on the slot the atlas gave it.
>
> ```rust
> // The same 50 glyphs each round, with a new blob each time, so that the
> // atlas takes them for new glyphs and gives them the next free slots.
> let mut resources = Resources::new();
> let mut first = None;
> let mut different = 0;
> for _ in 0..200 {
>     let font = FontData::new(Blob::new(Arc::new(ROBOTO.to_vec())), 0);
>     let mut ctx = RenderContext::new(600, 40);
>     let mut target = Pixmap::new(600, 40);
>     ctx.set_paint(Color::BLACK);
>     ctx.set_transform(Affine::translate((4.0, 28.0)));
>     ctx.glyph_run(&mut resources, &font)
>         .font_size(16.0)
>         .hint(false)
>         .atlas_cache(true)
>         .fill_glyphs((0..50).map(|i| Glyph { id: 40 + i, x: i as f32 * 11.0, y: 0.0 }));
>     ctx.flush();
>     ctx.render(&mut target, &mut resources);
>     match &first {
>         None => first = Some(target),
>         Some(first) => different += usize::from(first.data() != target.data()),
>     }
> }
> ```
>
> Here 57 of the 200 rounds differed from the first, each by one level in a few pixels. As far as I can tell the glyph is rasterized in place at its slot's coordinates, so the float rounding of its edges follows the slot's position; rasterizing it at a fixed origin and copying it into the slot would make it the same everywhere.
>
> Where it matters: a browser's reftests compare a test page with a reference page pixel for pixel, each rastered on its own thread with its own atlas, so the same line of text lands in different slots in the two.
