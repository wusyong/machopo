# 給 taffy 的 issue 草稿：BFC 的根包住 float 時沒加下方的 padding 和 border

還沒送出（`CLAUDE.md`：不對上游開 issue，草稿給人審核）。2026-10-13，taffy 0.15.0，RFC 0043 第 4b 段跑 WPT 時發現。

---

**Title**: A block formatting context root's height from its floats leaves out its bottom padding and border

**taffy version**: 0.15.0 (`block_layout`, `float_layout`)

**What happens**: a block that establishes a block formatting context (`overflow: hidden`, or a float) and holds floats taller than its in-flow content is as tall as the floats' bottom, measured from its border box's top, without its own bottom padding and border.

```html
<div style="overflow: hidden; width: 300px; padding: 10px; border: 2px solid">
  <div style="float: left; width: 50px; height: 40px"></div>
</div>
```

Expected (Chrome, Firefox): the outer box is 2 + 10 + 40 + 10 + 2 = 64px tall. taffy: 52px (the float's bottom, 12 + 40).

**Where**: `compute/block.rs`, `compute_inner`:

```rust
// Root BFCs contain floats
if block_ctx.is_bfc_root() || establishes_new_bfc {
    intrinsic_outer_height = intrinsic_outer_height.max(block_ctx.floated_content_height_contribution());
}
```

`intrinsic_outer_height` includes `resolved_content_box_inset.bottom` (added at the end of `perform_final_layout_on_in_flow_children`), but `floated_content_height_contribution()` is the floats' bottom in the border box's coordinates, without it. Adding `resolved_content_box_inset.bottom` to the contribution before the `max` gives the expected height.

**WPT**: `css/CSS2/floats-clear/float-replaced-height-001.xht` (a floated div with a bottom border around a floated image).
