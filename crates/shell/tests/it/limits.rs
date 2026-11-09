//! What a page asks for past what the GPU or the layout takes, in a whole
//! browser: drawn, or left out, as Chrome does, never a crash.

use std::path::PathBuf;

use gfx::Size;
use shell::headless;

/// Renders `html` (a 300 × 200 viewport, test fonts): the frame's RGBA
/// pixels and its width.
fn render(name: &str, html: &str) -> (Vec<u8>, usize) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-limits");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.html"));
    std::fs::write(&path, html).unwrap();
    let url = net::file_url(&path).expect("an absolute path");
    let frame = headless::render(&url, Size::new(300, 200), 1.0, true).expect("the page renders");
    let width = frame.size.width as usize;
    (frame.pixels, width)
}

/// The color at (x, y).
fn rgb(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 3] {
    let at = (y * width + x) * 4;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

/// A canvas wider than a texture Viz takes is drawn into its layer's
/// tiles, as Blink draws one it can't composite, rather than handed to the
/// GPU, which can't take it; one that fits is a layer of its own. Both
/// look the same: red on the left, green on the right, scaled into their
/// boxes.
#[test]
fn a_canvas_too_big_for_a_texture() {
    for width in [16_385, 8_192] {
        let html = format!(
            "<style>html,body{{margin:0}}canvas{{display:block;width:300px;height:100px}}</style>\
             <canvas width={width} height=4></canvas>\
             <script>\
             const c = document.querySelector('canvas').getContext('2d');\
             c.fillStyle = 'rgb(255,0,0)'; c.fillRect(0, 0, {width} / 2, 4);\
             c.fillStyle = 'rgb(0,128,0)'; c.fillRect({width} / 2, 0, {width} / 2, 4);\
             </script>"
        );
        let (pixels, frame_width) = render(&format!("canvas-{width}"), &html);
        assert_eq!(
            rgb(&pixels, frame_width, 50, 50),
            [255, 0, 0],
            "{width} wide, left"
        );
        assert_eq!(
            rgb(&pixels, frame_width, 250, 50),
            [0, 128, 0],
            "{width} wide, right"
        );
        assert_eq!(
            rgb(&pixels, frame_width, 150, 150),
            [255, 255, 255],
            "{width} wide, below"
        );
    }
}

/// Lengths and font sizes past any float, infinite or NaN, through
/// layout, text and paint: computed in range (as Chrome clamps them), so
/// none of them is infinite or NaN, and the page draws: the box placed
/// over them is there.
#[test]
fn lengths_past_any_float() {
    let (pixels, width) = render(
        "lengths",
        "<style>html,body{margin:0}</style>\
         <div style='width:1e39px;height:1e39px;margin-left:-1e39px;border:1e39px double red'></div>\
         <p style='font-size:1e39px;letter-spacing:1e39px;text-indent:1e39em;line-height:calc(infinity)'>X</p>\
         <p style='width:calc(infinity * 1px);padding:calc(NaN * 1px);word-spacing:calc(-infinity * 1px)'>X Y</p>\
         <div style='font-size:1000%'><div style='font-size:1000%'><div style='font-size:1000%'>X</div></div></div>\
         <div style='position:absolute;left:0;top:0;width:50px;height:50px;background:rgb(0,128,0)'></div>",
    );
    assert_eq!(rgb(&pixels, width, 25, 25), [0, 128, 0]);
}
