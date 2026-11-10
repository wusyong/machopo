//! The site snapshots in `tests/sites/` (see its README), loaded from disk
//! and rendered headless. Not pixel comparisons: each page is checked for
//! things that must hold whatever the fonts.

use std::path::{Path, PathBuf};

use gfx::Size;
use protocol::input::{InputEvent, Modifiers, MouseButton};
use shell::headless::{self, Frame};

fn sites_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/sites")
}

/// The `file:` URL of `page`, which may end in a `#fragment`.
fn url(page: &str) -> String {
    let (page, fragment) = page.split_once('#').unwrap_or((page, ""));
    let path = sites_dir()
        .join(page)
        .canonicalize()
        .expect("the snapshot exists");
    let url = net::file_url(&path).expect("an absolute path");
    if fragment.is_empty() {
        url
    } else {
        format!("{url}#{fragment}")
    }
}

fn render(page: &str) -> Frame {
    headless::render(&url(page), Size::new(800, 600), 1.0, true).expect("the page renders")
}

/// `page` after a click at `(x, y)`, once what it led to has loaded.
fn click(page: &str, x: f32, y: f32) -> Frame {
    let button = MouseButton::Left;
    let input = [
        InputEvent::mouse_down(x, y, button, Modifiers::NONE),
        InputEvent::mouse_up(x, y, button, Modifiers::NONE),
    ];
    headless::render_after_input(&url(page), Size::new(800, 600), 1.0, true, &input)
        .expect("the page renders")
}

/// How many pixels aren't the white canvas: the page drew something.
fn inked_pixels(frame: &Frame) -> usize {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| **px != [255, 255, 255, 255])
        .count()
}

#[test]
fn cern_pages_load_with_their_titles() {
    for (page, title) in [
        (
            "cern/hypertext/WWW/TheProject.html",
            "The World Wide Web project",
        ),
        ("cern/hypertext/WWW/WhatIs.html", "What is Hypertext?"),
        ("cern/hypertext/WWW/Summary.html", "Summary -- /WWW"),
        ("cern/hypertext/WWW/Status.html", "WWW Software products"),
        ("cern/hypertext/WWW/History.html", "WWW Project History"),
    ] {
        let frame = render(page);
        assert_eq!(frame.title.as_deref(), Some(title), "{page}");
        assert!(inked_pixels(&frame) > 1000, "{page} drew almost nothing");
        assert!(
            frame.failed_subresources.is_empty(),
            "{page}: {:?}",
            frame.failed_subresources
        );
    }
}

#[test]
fn cern_links_lead_to_their_pages() {
    // "hypermedia" in the first paragraph (at these coordinates with the
    // test fonts).
    let frame = click("cern/hypertext/WWW/TheProject.html", 335.0, 93.0);
    assert_eq!(frame.url, Some(url("cern/hypertext/WWW/WhatIs.html")));
    assert_eq!(frame.title.as_deref(), Some("What is Hypertext?"));
}

#[test]
fn sqlite_about_loads_its_stylesheet() {
    let frame = render("sqlite/about.html");
    assert_eq!(frame.title.as_deref(), Some("About SQLite"));
    // Everything loads, the logo too, an SVG image.
    assert!(
        frame.failed_subresources.is_empty(),
        "{:?}",
        frame.failed_subresources
    );
    // `sqlite.css` colors the menu bar #044a64.
    let menu = [0x04, 0x4a, 0x64, 255];
    assert!(
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| **px == menu)
            .count()
            > 10_000
    );
}

#[test]
fn gutenberg_alice_decodes_every_image() {
    let frame = render("gutenberg-28885/pg28885-images.html");
    assert!(
        frame
            .title
            .as_deref()
            .is_some_and(|t| t.contains("Alice's Adventures in Wonderland"))
    );
    // 37 images (42 `<img>`s, some sharing one), PNG and JPEG: none fails.
    // Only the background image its style sheet names, which the saved
    // copy doesn't have, does.
    assert_eq!(
        frame.failed_subresources.len(),
        1,
        "{:?}",
        frame.failed_subresources
    );
    assert!(frame.failed_subresources[0].ends_with("images/p0003-rabbithole.png"));
    assert!(inked_pixels(&frame) > 1000);
}

#[test]
fn gutenberg_contents_link_to_their_chapters() {
    // From the contents (a table in a cell beside the spine image's),
    // chapter II's "13", in the body that `@media screen` narrows.
    let page = "gutenberg-28885/pg28885-images.html";
    let frame = click(&format!("{page}#img_images_spine.jpg"), 566.0, 168.0);
    assert_eq!(frame.url, Some(url(&format!("{page}#Page_13"))));
    // It shows what loading that URL shows: the chapter at the top.
    let loaded = render(&format!("{page}#Page_13"));
    assert!(frame.pixels == loaded.pixels, "not scrolled to the chapter");
}

/// TodoMVC's JS app (Speedometer's vanilla ES5 one) loads with its style
/// sheets and scripts, and draws its header.
#[test]
fn todomvc_loads_its_style_sheets_and_scripts() {
    let frame = render("todomvc/index.html");
    assert_eq!(frame.title.as_deref(), Some("TodoMVC: JavaScript Es5"));
    assert!(
        frame.failed_subresources.is_empty(),
        "{:?}",
        frame.failed_subresources
    );
    assert!(inked_pixels(&frame) > 10_000, "{}", inked_pixels(&frame));
}
