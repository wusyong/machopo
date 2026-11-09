//! A `SharedQuadState::rounded_clip`'s coverage of a pixel (Chromium's
//! fast rounded corners, `RoundedCornerInfo` in `SkiaRenderer`'s shaders):
//! the signed distance from the pixel's center to the clip's edge,
//! approximated for an elliptical corner by the ellipse's implicit
//! function over its gradient, made a coverage by a one-pixel ramp, in the
//! quads' fragment shader (`ROUNDED_CLIP_WGSL`).

/// The coverage in WGSL, for the quads' fragment shader: `r` is the
/// clip's rect as (x0, y0, x1, y1), none if `x1 < x0`; `rx` and `ry` the
/// corners' radii (top left, top right, bottom right, bottom left).
pub(crate) const ROUNDED_CLIP_WGSL: &str = r#"
fn rounded_clip_coverage(p: vec2<f32>, r: vec4<f32>, rx: vec4<f32>, ry: vec4<f32>) -> f32 {
    if (r.z < r.x) {
        return 1.0;
    }
    var d = max(max(r.x - p.x, p.x - r.z), max(r.y - p.y, p.y - r.w));
    let centers = array<vec2<f32>, 4>(
        vec2<f32>(r.x + rx.x, r.y + ry.x),
        vec2<f32>(r.z - rx.y, r.y + ry.y),
        vec2<f32>(r.z - rx.z, r.w - ry.z),
        vec2<f32>(r.x + rx.w, r.w - ry.w),
    );
    // Parenthesized: `a < b ... > c` would read as a template list.
    let inside = array<bool, 4>(
        (p.x < centers[0].x) && (p.y < centers[0].y),
        (p.x > centers[1].x) && (p.y < centers[1].y),
        (p.x > centers[2].x) && (p.y > centers[2].y),
        (p.x < centers[3].x) && (p.y > centers[3].y),
    );
    for (var i = 0; i < 4; i++) {
        let a = rx[i];
        let b = ry[i];
        if (!inside[i] || (a <= 0.0) || (b <= 0.0)) {
            continue;
        }
        let q = (p - centers[i]) / vec2<f32>(a, b);
        let f = dot(q, q) - 1.0;
        let g = max(length(2.0 * q / vec2<f32>(a, b)), 1e-4);
        d = max(d, f / g);
    }
    return clamp(0.5 - d, 0.0, 1.0);
}
"#;
