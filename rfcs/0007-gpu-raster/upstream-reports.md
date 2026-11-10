# 要回報上游的草稿

四份，**都還沒送出**：照 `CLAUDE.md`，對上游開 issue 要先給人審。每份下面的英文是要貼出去的內容；重現都對照 `linebender/vello` 的 main `112736df`（2026-11-10）。送出之後把 issue 的編號寫進 `docs/render-gaps.md` 的那一列和程式裡的 `vello:` 註解。

送之前要確認的：

- 每份先在上游找一次有沒有人回報過（寫草稿時只對過 `docs/render-gaps.md` 列的那幾個 issue）。
- 第 3 份的重現只在 machopo 的 playback 裡跑過（WPT 的 `html/canvas/element/line-styles/2d.line.cross.html`），還沒有縮成只用 kurbo 的程式；送之前先縮，確認是 kurbo 的 stroker 而不是 Vello 這一側。
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
