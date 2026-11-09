//! `<canvas>`'s 2D context in a whole browser: what its members record is
//! drawn (the `canvas` crate's steps, played by Skia), before each
//! frame; a draw alone asks for the frame that shows it.

use std::collections::HashMap;
use std::path::PathBuf;

use net::test_server::{Route, TestServer};
use shell::headless;

/// `page` in this test's scratch directory, as a `file:` URL.
fn site(test: &str, page: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("shell-canvas")
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("index.html");
    std::fs::write(&path, page).unwrap();
    net::file_url(&path).expect("an absolute path")
}

const SIZE: gfx::Size = gfx::Size::new(400, 200);

/// Rects with colors, `globalAlpha`, compositing and `clearRect`, under
/// transforms; paths of every kind filled by both rules and stroked with
/// widths, caps, joins and dashes; a clip saved and restored; an opaque
/// canvas sized by CSS; `currentcolor`; a canvas whose new width cleared
/// it, drawn again; an inline one, atomic; gradients of the three kinds,
/// a shadow, `putImageData` whole and in part, `setTransform`'s dictionary;
/// `drawImage` of canvases (one whose bitmap is on the canvas raster
/// thread) and of an `<img>`, scaled and in part, smoothed and not;
/// patterns of a canvas, repeated both ways and one, through a transform;
/// `Path2D`s of path data and of `roundRect`, added through a transform,
/// filled and clipped to; the context's `roundRect`; text filled and
/// stroked, its font relative to the canvas's style, aligned by the
/// canvas's `dir`, spaced, squeezed to a `maxWidth`.
const PAGE: &str = r#"<!DOCTYPE html>
<style>
body { margin: 0 } body > canvas { position: absolute; top: 0 }
</style>
<canvas id=a width=100 height=80 style="left: 0"></canvas>
<canvas id=b width=100 height=80 style="left: 100px"></canvas>
<canvas id=c width=40 height=20 style="left: 200px; width: 80px; height: 40px"></canvas>
<canvas id=d width=50 height=50 style="left: 300px; color: green"></canvas>
<p style="position: absolute; top: 100px; left: 0; margin: 0">a <canvas id=e width=20 height=20></canvas> b</p>
<canvas id=f width=200 height=80 style="left: 100px; top: 100px"></canvas>
<canvas id=g width=100 height=100 style="left: 300px; top: 100px"></canvas>
<canvas id=h width=50 height=100 style="left: 350px"></canvas>
<canvas id=i width=100 height=50 style="left: 200px; top: 45px; font-size: 8px" dir=rtl></canvas>
<script>
const ctx = id => document.getElementById(id).getContext('2d', id == 'c' ? {alpha: false} : {});

const a = ctx('a');
a.fillStyle = 'red';
a.fillRect(5, 5, 50, 40);
a.globalAlpha = 0.5;
a.fillStyle = 'blue';
a.fillRect(30, 20, 50, 40);
a.globalAlpha = 1;
a.globalCompositeOperation = 'multiply';
a.fillStyle = 'rgb(255 255 0)';
a.fillRect(20, 30, 40, 40);
// Not composited: it clears under any mode.
a.globalCompositeOperation = 'copy';
a.clearRect(10, 10, 10, 10);
a.globalCompositeOperation = 'source-over';
a.save();
a.translate(70, 60);
a.rotate(Math.PI / 6);
a.scale(1.5, 0.5);
a.lineWidth = 4;
a.lineJoin = 'round';
a.strokeStyle = 'purple';
a.strokeRect(-10, -10, 20, 20);
a.restore();
a.transform(1, 0.2, 0, 1, 0, 0);
a.setTransform(1, 0, 0, 1, 0, 0);
a.fillStyle = 'teal';
a.fillRect(85, 0, 10, 10);

const b = ctx('b');
b.beginPath();
b.moveTo(10, 10);
b.lineTo(60, 10);
b.quadraticCurveTo(80, 30, 60, 50);
b.bezierCurveTo(40, 70, 20, 60, 10, 40);
b.closePath();
b.rect(20, 20, 20, 20);
b.fillStyle = 'orange';
b.fill('evenodd');
b.beginPath();
b.arc(75, 20, 15, 0, Math.PI * 1.5, false);
b.lineWidth = 5;
b.lineCap = 'round';
b.setLineDash([6, 4]);
b.lineDashOffset = 3;
b.strokeStyle = 'navy';
b.stroke();
b.setLineDash([]);
b.beginPath();
b.moveTo(10, 75);
b.arcTo(50, 50, 90, 75, 20);
b.lineJoin = 'bevel';
b.lineCap = 'square';
b.lineWidth = 3;
b.miterLimit = 2;
b.stroke();
b.save();
b.beginPath();
b.ellipse(70, 60, 20, 10, Math.PI / 8, 0, Math.PI * 2);
b.clip();
b.fillStyle = 'rgba(0, 128, 0, 0.75)';
b.fillRect(40, 40, 60, 40);
b.restore();
b.fillStyle = 'black';
b.fillRect(95, 75, 5, 5);

const c = ctx('c');
c.fillStyle = '#ff00ff';
c.fillRect(5, 5, 20, 10);
c.globalCompositeOperation = 'destination-out';
c.fillRect(10, 8, 5, 5);

const e = ctx('e');
e.fillStyle = 'lime';
e.fillRect(0, 0, 20, 20);

const f = ctx('f');
const linear = f.createLinearGradient(0, 0, 60, 0);
linear.addColorStop(0, 'red');
linear.addColorStop(0.5, 'rgba(0, 0, 255, 0.5)');
linear.addColorStop(1, 'lime');
f.fillStyle = linear;
f.fillRect(0, 0, 60, 30);
const radial = f.createRadialGradient(90, 20, 2, 95, 25, 18);
radial.addColorStop(0, 'yellow');
radial.addColorStop(1, 'purple');
f.save();
f.fillStyle = radial;
f.shadowColor = 'rgba(0, 0, 0, 0.6)';
f.shadowOffsetX = 4;
f.shadowOffsetY = 6;
f.shadowBlur = 3;
f.beginPath();
f.arc(95, 25, 18, 0, Math.PI * 2);
f.fill();
f.restore();
const conic = f.createConicGradient(Math.PI / 4, 150, 30);
conic.addColorStop(0, 'orange');
conic.addColorStop(1, 'teal');
f.strokeStyle = conic;
f.lineWidth = 6;
f.strokeRect(130, 10, 40, 40);
const block = f.createImageData(20, 10);
for (let i = 0; i < block.data.length; i += 4) block.data.set([0, 128, 255, i % 8 ? 255 : 100], i);
f.putImageData(block, 10, 50);
f.putImageData(block, 40, 50, 5, 2, 10, 6);
f.setTransform({a: 1, d: 1, e: 70, f: 60});
f.fillStyle = 'black';
f.fillRect(0, 0, 10, 10);

const canvas = document.getElementById('d');
const d = ctx('d');
d.fillStyle = 'red';
d.fillRect(0, 0, 50, 50);
canvas.width = 40;
d.fillStyle = 'currentcolor';
d.fillRect(5, 5, 20, 20);
d.strokeStyle = 'currentcolor';
d.beginPath();
d.rect(10, 30, 20, 10);
d.stroke();

const g = ctx('g');
g.drawImage(document.getElementById('a'), 0, 0, 50, 40);
g.imageSmoothingEnabled = false;
g.drawImage(document.getElementById('b'), 10, 10, 30, 30, 50, 0, 50, 50);
const tile = document.createElement('canvas');
tile.width = tile.height = 6;
const t = tile.getContext('2d');
t.fillStyle = 'red';
t.fillRect(0, 0, 3, 3);
t.fillStyle = 'blue';
t.fillRect(3, 3, 3, 3);
const pattern = g.createPattern(tile, 'repeat');
pattern.setTransform({a: 2, d: 2, e: 1});
g.fillStyle = pattern;
g.fillRect(0, 50, 50, 50);
g.strokeStyle = g.createPattern(tile, 'repeat-x');
g.lineWidth = 8;
g.strokeRect(60, 60, 20, 20);
// Red, lime / blue, yellow.
const img = new Image();
img.onload = () => {
  g.drawImage(img, 84, 52);
  g.imageSmoothingEnabled = true;
  g.drawImage(img, 0, 0, 2, 1, 86, 86, 12, 12);
};
const h = ctx('h');
h.fillStyle = 'blue';
h.fill(new Path2D('M0 0 H40 L0 40 Z'));
const round = new Path2D();
round.roundRect(5, 50, 40, 40, 10);
const moved = new Path2D();
moved.addPath(round, {f: 2});
h.save();
h.clip(moved);
h.fillStyle = 'lime';
h.fillRect(0, 50, 50, 50);
h.restore();
h.beginPath();
h.roundRect(30, 5, 15, 15, [3, {x: 2, y: 4}]);
h.fillStyle = 'red';
h.fill('evenodd');
const i = ctx('i');
i.font = 'bold 2.5em sans-serif';
i.fillStyle = 'navy';
i.fillText('Machopo', 2, 20);
i.font = 'italic 12px serif';
i.letterSpacing = '2px';
i.textAlign = 'start';
i.textBaseline = 'top';
i.strokeText('rtl', 98, 25);
i.direction = 'ltr';
i.textAlign = 'center';
i.fillText('squeezed', 50, 48, 30);
img.src = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAF0lEQVR4nGP4z8DwHwwZGP7///+f4T8AR8oI+P1do8oAAAAASUVORK5CYII=';
</script>"#;

/// The page drawn headless is, pixel for pixel, as drawn by the run that
/// sends input first (none here).
#[test]
fn drawn_the_same_by_both_headless_runs() {
    let url = site("drawn", PAGE);
    let rendered = headless::render(&url, SIZE, 1.0, true).expect("drawn");
    let after_input = headless::render_after_input(&url, SIZE, 1.0, true, &[]).expect("drawn");
    assert_eq!(rendered.size, after_input.size);
    let differ: Vec<(usize, usize)> = rendered
        .pixels
        .chunks(4)
        .zip(after_input.pixels.chunks(4))
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| (i % SIZE.width as usize, i / SIZE.width as usize))
        .collect();
    assert!(
        differ.is_empty(),
        "{} pixels differ, the first at {:?}",
        differ.len(),
        differ.first()
    );
    // Drawn at all: a's red, b's orange, c's magenta, d's green.
    let pixel = |x: usize, y: usize| {
        let i = (y * SIZE.width as usize + x) * 4;
        <[u8; 4]>::try_from(&after_input.pixels[i..i + 4]).unwrap()
    };
    assert_eq!(pixel(7, 7), [255, 0, 0, 255]);
    assert_eq!(pixel(12, 12), [255, 255, 255, 255], "cleared");
    assert_eq!(pixel(112, 12), [255, 165, 0, 255]);
    assert_eq!(pixel(212, 12), [255, 0, 255, 255]);
    assert_eq!(pixel(310, 10), [0, 128, 0, 255]);
    let lime = (0..SIZE.width as usize)
        .filter(|&x| pixel(x, 110) == [0, 255, 0, 255])
        .count();
    assert_eq!(lime, 20, "the inline one, 20px wide");
    // f's gradient starts red; its `putImageData` block is opaque blue.
    let [r, g, b, _] = pixel(101, 101);
    assert!(
        r > 200 && g < 30 && b < 30,
        "the gradient's start: {r},{g},{b}"
    );
    assert_eq!(pixel(111, 151), [0, 128, 255, 255], "the block put");
    // g: a's red, scaled down; the pattern's red tile at 1, 2× (red at
    // 301..307, blue at 307..313); the image's top left red, its bottom
    // right yellow.
    assert_eq!(pixel(304, 104), [255, 0, 0, 255], "a, drawn in g");
    assert_eq!(pixel(302, 152), [255, 0, 0, 255], "the pattern's red");
    assert_eq!(pixel(309, 158), [0, 0, 255, 255], "the pattern's blue");
    // The stroke's pattern repeats along x only, its tile above the
    // stroke: nothing there (Skia's decal), where clamped down its blue
    // column would be.
    assert_eq!(pixel(363, 175), [255, 255, 255, 255], "below the tile");
    assert_eq!(pixel(384, 152), [255, 0, 0, 255], "the image's red");
    assert_eq!(pixel(385, 153), [255, 255, 0, 255], "the image's yellow");
    // h: a `Path2D` of path data filled; a rounded one, moved down 2,
    // clipped to (its corner out); the context's `roundRect`.
    assert_eq!(pixel(355, 5), [0, 0, 255, 255], "the path data's");
    assert_eq!(pixel(375, 72), [0, 255, 0, 255], "in the clip");
    assert_eq!(
        pixel(356, 53),
        [255, 255, 255, 255],
        "the clip's round corner"
    );
    assert_eq!(pixel(387, 12), [255, 0, 0, 255], "the context's roundRect");
    // i: text drawn at all.
    let inked = (200..300)
        .flat_map(|x| (45..95).map(move |y| (x, y)))
        .filter(|&(x, y)| pixel(x, y) != [255, 255, 255, 255])
        .count();
    assert!(inked > 100, "i's text: {inked} pixels");
}

/// A timer's draw, which changes nothing else, is drawn in a frame of its
/// own: the load's, the draw's, and the one that shows `reftest-wait`
/// gone.
#[test]
fn a_draw_asks_for_a_frame() {
    let url = site(
        "frame",
        r#"<!DOCTYPE html>
<html class="reftest-wait">
<body style="margin: 0">
<canvas id=c width=10 height=10 style="display: block"></canvas>
<script>
const ctx = document.getElementById('c').getContext('2d');
onload = () => {
  setTimeout(() => ctx.fillRect(0, 0, 10, 10), 100);
  setTimeout(() => document.documentElement.className = '', 600);
};
</script>"#,
    );
    let frame = headless::render_after_input(&url, SIZE, 1.0, true, &[]).expect("drawn");
    assert_eq!(frame.frames_drawn, 3);
    assert_eq!(&frame.pixels[..4], [0, 0, 0, 255]);
}

/// What a canvas in no document drew is kept, and drawn once it is in
/// the frame's; a new `width` is laid out at once, the bitmap cleared
/// and drawn again at its new size.
#[test]
fn drawn_once_shown_at_its_new_size() {
    let url = site(
        "shown",
        r#"<!DOCTYPE html>
<html class="reftest-wait">
<body style="margin: 0">
<div id=d style="height: 10px"></div>
<canvas id=c width=10 height=10 style="display: block"></canvas>
<script>
const detached = document.createElement('canvas');
detached.width = detached.height = 10;
detached.style.display = 'block';
detached.getContext('2d').fillRect(0, 0, 10, 10);
const c = document.getElementById('c');
const ctx = c.getContext('2d');
ctx.fillRect(0, 0, 10, 10);
onload = () => {
  setTimeout(() => {
    document.getElementById('d').appendChild(detached);
    c.width = 20;
    ctx.fillStyle = 'lime';
    ctx.fillRect(10, 0, 10, 10);
  }, 100);
  setTimeout(() => document.documentElement.className = '', 400);
};
</script>"#,
    );
    let frame = headless::render_after_input(&url, SIZE, 1.0, true, &[]).expect("drawn");
    let pixel = |x: usize, y: usize| {
        let i = (y * SIZE.width as usize + x) * 4;
        <[u8; 4]>::try_from(&frame.pixels[i..i + 4]).unwrap()
    };
    assert_eq!(pixel(5, 5), [0, 0, 0, 255], "the detached one");
    assert_eq!(pixel(5, 15), [255, 255, 255, 255], "cleared");
    assert_eq!(pixel(15, 15), [0, 255, 0, 255], "at its new width");
}

/// `getImageData` reads what was drawn (the bitmap on the canvas raster
/// thread: the main thread waits for its snapshot), what `putImageData`
/// wrote over it after, and after the second read, the bitmap taken back
/// to the main thread, the same; `willReadFrequently`'s, drawn here all
/// along (what was recorded drawn before the pixels are written), the same.
#[test]
fn pixels_are_read_back() {
    let url = site(
        "read",
        r#"<!DOCTYPE html>
<canvas id=c width=4 height=4></canvas>
<canvas id=w width=4 height=4></canvas>
<script>
const read = (ctx, x, y) => Array.from(ctx.getImageData(x, y, 1, 1).data).join(',');
const log = [];
for (const [id, options] of [['c', {}], ['w', {willReadFrequently: true}]]) {
  const ctx = document.getElementById(id).getContext('2d', options);
  ctx.fillStyle = 'rgba(255, 0, 0, 0.5)';
  ctx.fillRect(0, 0, 2, 2);
  const green = ctx.createImageData(1, 1);
  green.data.set([0, 255, 0, 255]);
  ctx.putImageData(green, 1, 1);
  log.push(read(ctx, 0, 0), read(ctx, 1, 1), read(ctx, 2, 2));
  ctx.fillStyle = 'blue';
  ctx.fillRect(1, 1, 1, 1);
  log.push(read(ctx, 1, 1), read(ctx, 0, 0));
}
testRunner.reportHarnessResults(log.join(' '));
</script>"#,
    );
    let report =
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .expect("the page reports");
    let one = "255,0,0,128 0,255,0,255 0,0,0,0 0,0,255,255 255,0,0,128";
    assert_eq!(report, format!("{one} {one}"));
}

/// A 2×2 PNG: red, lime / blue, yellow.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72, 0xb6, 0x0d,
    0x24, 0x00, 0x00, 0x00, 0x17, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x0c, 0x19, 0x18, 0xfe, 0xff, 0xff, 0xff, 0x9f, 0xe1, 0x3f, 0x00, 0x47, 0xca, 0x08, 0xf8,
    0xfd, 0x5d, 0xa3, 0xca, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// An image from another origin, loaded without CORS, leaves a canvas
/// that draws it not origin-clean (its pixels can't be read: the
/// renderer's request is from the document's origin, so the network
/// service taints the response); one from the document's own origin
/// doesn't.
#[test]
fn another_origins_image_taints() {
    let other = TestServer::start(HashMap::from([(
        "/other.png".to_owned(),
        Route::ok("image/png", PNG),
    )]))
    .expect("a server");
    let page = format!(
        r#"<!DOCTYPE html>
<img id=same src="same.png"><img id=other src="{}">
<script>
onload = () => {{
  const log = [];
  for (const id of ['same', 'other']) {{
    const ctx = document.createElement('canvas').getContext('2d');
    ctx.drawImage(document.getElementById(id), 0, 0);
    try {{
      log.push(Array.from(ctx.getImageData(1, 1, 1, 1).data).join(','));
    }} catch (e) {{
      log.push(e.name);
    }}
  }}
  testRunner.reportHarnessResults(log.join(' '));
}};
</script>"#,
        other.url("/other.png")
    );
    let server = TestServer::start(HashMap::from([
        ("/".to_owned(), Route::ok("text/html", page)),
        ("/same.png".to_owned(), Route::ok("image/png", PNG)),
    ]))
    .expect("a server");
    let report = headless::run_testharness(
        &server.url("/"),
        std::time::Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(report, "255,255,0,255 SecurityError");
}

/// `toDataURL` and `toBlob` of a canvas whose bitmap is on the canvas
/// raster thread (the main thread waits for its snapshot for the URL; the
/// encoder thread waits for the blob's) and of one drawn here: the URL a
/// PNG that decodes to the pixels; a blob of the bitmap as it was at the
/// call (the same PNG as the URL's, not what was drawn after), its
/// callbacks called in the order of the calls; a JPEG's of its type.
#[test]
fn exported_as_files() {
    let url = site(
        "export",
        r#"<!DOCTYPE html>
<canvas id=c width=2 height=1></canvas>
<canvas id=w width=2 height=1></canvas>
<script>
const bytes = blob => blob.arrayBuffer().then(buffer => Array.from(new Uint8Array(buffer)).join(','));
const decoded = url => new Promise(resolve => {
  const img = document.createElement('img');
  img.onload = () => {
    const ctx = document.createElement('canvas').getContext('2d');
    ctx.drawImage(img, 0, 0);
    resolve(Array.from(ctx.getImageData(0, 0, 2, 1).data).join(','));
  };
  img.src = url;
});
const blob = (canvas, ...args) => new Promise(resolve => canvas.toBlob(resolve, ...args));
const order = [];
const files = [];
for (const [id, options] of [['c', {}], ['w', {willReadFrequently: true}]]) {
  const canvas = document.getElementById(id);
  const ctx = canvas.getContext('2d', options);
  ctx.fillStyle = 'lime';
  ctx.fillRect(0, 0, 2, 1);
  ctx.fillStyle = 'red';
  ctx.fillRect(0, 0, 1, 1);
  const url = canvas.toDataURL();
  const png = blob(canvas).then(b => { order.push(`${id} png`); return b; });
  const jpeg = blob(canvas, 'image/jpeg', 0.9).then(b => { order.push(`${id} jpeg`); return b; });
  ctx.fillStyle = 'blue';
  ctx.fillRect(0, 0, 2, 1);
  const after = blob(canvas).then(b => { order.push(`${id} after`); return b; });
  files.push(Promise.all([png, jpeg, after]).then(async ([png, jpeg, after]) => [
    url, await decoded(url), png.type, await bytes(png), jpeg.type,
    (await bytes(jpeg)).split(',').slice(0, 2).join(','), await bytes(after) !== await bytes(png),
  ].join(' ')));
}
Promise.all(files).then(files => testRunner.reportHarnessResults([order.join(','), ...files].join('\n')));
</script>"#,
    );
    let report =
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .expect("the page reports");
    let mut lines = report.lines();
    assert_eq!(
        lines.next(),
        Some("c png,c jpeg,c after,w png,w jpeg,w after")
    );
    for line in lines {
        let [url, pixels, png_type, png, jpeg_type, jpeg_start, changed] =
            line.split(' ').collect::<Vec<_>>()[..]
        else {
            panic!("a canvas's files: {line}");
        };
        assert_eq!(pixels, "255,0,0,255,0,255,0,255", "{line}");
        let base64 = url
            .strip_prefix("data:image/png;base64,")
            .expect("a PNG's URL");
        let file = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(base64)
                .expect("base64")
        };
        let blob: Vec<u8> = png.split(',').map(|byte| byte.parse().unwrap()).collect();
        assert_eq!(
            blob, file,
            "the blob is the bitmap at the call, as the URL is"
        );
        assert_eq!(
            (png_type, jpeg_type, jpeg_start, changed),
            ("image/png", "image/jpeg", "255,216", "true")
        );
    }
}

/// `measureText` lays the text out with the frame's fonts, its ink from
/// the glyphs' outlines: wider text is wider, ink on both sides of a
/// centered alignment point, above the baseline, the font's ascent above
/// its ink.
#[test]
fn text_is_measured() {
    let url = site(
        "measure",
        r#"<!DOCTYPE html>
<canvas id=c></canvas>
<script>
const ctx = document.getElementById('c').getContext('2d');
ctx.font = '20px sans-serif';
ctx.textAlign = 'center';
const m = ctx.measureText('Machopo');
const wider = ctx.measureText('MachopoMachopo');
testRunner.reportHarnessResults([
  m.width > 20, wider.width > m.width, m.actualBoundingBoxLeft > 5, m.actualBoundingBoxRight > 5,
  m.actualBoundingBoxAscent > 5, m.fontBoundingBoxAscent >= m.actualBoundingBoxAscent,
].join(' '));
</script>"#,
    );
    let report =
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .expect("the page reports");
    assert_eq!(report, "true true true true true true");
}
