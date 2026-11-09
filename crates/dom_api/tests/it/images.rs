//! An `<img>`'s source selected in its update's microtask against its
//! frame's viewport and device pixel ratio (`run_image_updates`): its
//! `srcset` (densities, widths and `sizes`), its `<picture>`'s `<source>`s
//! (`media`, `type`, the first that has a candidate); and selected again as
//! the environment changes (`react_to_environment_changes`): another URL
//! updates its image data, the same at another density changes its natural
//! size alone.

use std::rc::Rc;

use dom::{NodeRoot, QualName};

use crate::geometry::{fake, page};
use crate::{TestDom, with_host};

/// A page of `<body>` with a window at `https://example.test/`, its frame's
/// viewport `width` wide at `scale`.
fn window(width: f32, scale: f32) -> (TestDom, Rc<NodeRoot>) {
    let (mut dom, nodes, _) = page(false, &[]);
    let document = nodes[0].clone();
    dom.mutate(|cx| {
        let document = document.get(cx.gc);
        cx.create_window(document);
        cx.set_document_url(document, "https://example.test/");
    });
    viewport(&dom, width, scale);
    (dom, nodes[2].clone())
}

fn viewport(dom: &TestDom, width: f32, scale: f32) {
    fake(dom, |layout| {
        layout.viewport.size.width = width;
        layout.viewport.device_scale_factor = scale;
    });
}

/// An element of `local` with `attributes`, appended to `parent`.
fn add(
    dom: &mut TestDom,
    parent: &NodeRoot,
    local: &str,
    attributes: &[(&str, &str)],
) -> Rc<NodeRoot> {
    dom.mutate(|cx| {
        let document = cx.document_of(parent.get(cx.gc));
        let element = cx.create_element(document, QualName::html(local));
        for (name, value) in attributes {
            cx.set_attribute(element, name, value);
        }
        cx.append_child(parent.get(cx.gc), element).unwrap();
        Rc::new(cx.root(element))
    })
}

/// The microtasks run: the URLs fetched.
fn update(dom: &mut TestDom) -> Vec<String> {
    with_host(dom, |host| dom_api::run_image_updates(host));
    dom.dom()
        .take_image_fetches()
        .into_iter()
        .map(|fetch| fetch.url)
        .collect()
}

/// The URL `element`'s request is for (its pending one's while it has one).
fn requested(dom: &TestDom, element: &NodeRoot) -> Option<String> {
    dom.read(|cx| {
        let element = element.get(cx.gc);
        cx.image_request(element).map(|(_, url)| url)
    })
}

fn natural_width(dom: &TestDom, element: &NodeRoot) -> u32 {
    dom.read(|cx| cx.image_natural_size(element.get(cx.gc)).width)
}

fn fetched(dom: &mut TestDom, url: &str, width: u32) {
    dom.mutate(|cx| {
        let image = dom::Image::Raster(
            gfx::ImageData::new(vec![0u8; 4]),
            gfx::Size { width, height: 1 },
        );
        cx.image_fetched(url, Some(image), true);
    });
}

fn changed(dom: &TestDom, document: &NodeRoot) {
    with_host(dom, |host| {
        dom_api::react_to_environment_changes(host, document)
    });
}

fn document_of(dom: &TestDom, node: &NodeRoot) -> NodeRoot {
    dom.read(|cx| cx.root(cx.document_of(node.get(cx.gc))))
}

#[test]
fn srcset_densities_by_the_device_pixel_ratio() {
    let (mut dom, body) = window(800.0, 1.0);
    let img = add(
        &mut dom,
        &body,
        "img",
        &[("srcset", "a.png 1x, b.png 2x"), ("src", "s.png")],
    );
    assert_eq!(update(&mut dom), ["https://example.test/a.png"]);
    viewport(&dom, 800.0, 2.0);
    let document = document_of(&dom, &body);
    changed(&dom, &document);
    assert_eq!(update(&mut dom), ["https://example.test/b.png"]);
    assert_eq!(
        requested(&dom, &img).as_deref(),
        Some("https://example.test/b.png")
    );
    // A density of 2: half the image's size.
    fetched(&mut dom, "https://example.test/b.png", 40);
    assert_eq!(natural_width(&dom, &img), 20);
}

#[test]
fn srcset_widths_by_sizes() {
    let (mut dom, body) = window(800.0, 1.0);
    let img = add(
        &mut dom,
        &body,
        "img",
        &[
            ("srcset", "s.png 400w, l.png 1600w"),
            ("sizes", "(max-width: 500px) 100vw, 400px"),
        ],
    );
    // 400px wide: `s.png` is 1x.
    assert_eq!(update(&mut dom), ["https://example.test/s.png"]);
    // Without `sizes`, 100vw: 800px, `l.png` at 2x is denser than 1.
    let wide = add(
        &mut dom,
        &body,
        "img",
        &[("srcset", "s.png 400w, l.png 1600w")],
    );
    assert_eq!(update(&mut dom), ["https://example.test/l.png"]);
    let _ = (img, wide);
}

#[test]
fn a_pictures_first_matching_source() {
    let (mut dom, body) = window(800.0, 1.0);
    let picture = add(&mut dom, &body, "picture", &[]);
    add(
        &mut dom,
        &picture,
        "source",
        &[("srcset", "webp.png"), ("type", "image/webp")],
    );
    add(
        &mut dom,
        &picture,
        "source",
        &[("srcset", "narrow.png"), ("media", "(max-width: 500px)")],
    );
    add(&mut dom, &picture, "source", &[("srcset", "")]);
    let img = add(&mut dom, &picture, "img", &[("src", "wide.png")]);
    assert_eq!(update(&mut dom), ["https://example.test/wide.png"]);
    // Narrower: the `media` matches, the image changes.
    viewport(&dom, 400.0, 1.0);
    let document = document_of(&dom, &body);
    changed(&dom, &document);
    assert_eq!(update(&mut dom), ["https://example.test/narrow.png"]);
    assert_eq!(
        requested(&dom, &img).as_deref(),
        Some("https://example.test/narrow.png")
    );
}

/// The same image at another density: its natural size alone changes, no
/// update (no fetch, no event: Blink's).
#[test]
fn the_same_image_at_another_density_isnt_fetched_again() {
    let (mut dom, body) = window(800.0, 1.0);
    let img = add(&mut dom, &body, "img", &[("srcset", "a.png 800w")]);
    assert_eq!(update(&mut dom), ["https://example.test/a.png"]);
    fetched(&mut dom, "https://example.test/a.png", 100);
    assert_eq!(natural_width(&dom, &img), 100);
    viewport(&dom, 400.0, 1.0);
    let document = document_of(&dom, &body);
    changed(&dom, &document);
    assert!(
        !dom.read(|cx| cx.image_update_queued(img.get(cx.gc))),
        "no update"
    );
    assert!(update(&mut dom).is_empty());
    assert_eq!(
        natural_width(&dom, &img),
        50,
        "800w at 400px: a density of 2"
    );
}

/// The lazy load observer's task resumes its targets that intersect
/// (Blink's `LazyLoadImageObserver`), no script called: the image is
/// fetched.
#[test]
fn the_lazy_load_observers_task_resumes_the_load() {
    let (mut dom, body) = window(800.0, 1.0);
    let img = add(
        &mut dom,
        &body,
        "img",
        &[("loading", "lazy"), ("src", "a.png")],
    );
    assert!(update(&mut dom).is_empty(), "it waits");
    let window = dom.mutate(|cx| {
        let window = cx.window_of(cx.document_of(img.get(cx.gc))).unwrap();
        let observer = cx.window_intersection_observers(window)[0];
        let target = img.get(cx.gc);
        let rect = Some(gfx::RectF::new(0.0, 0.0, 10.0, 10.0));
        let computed = [dom_api::ComputedIntersection {
            observer,
            target,
            target_rect: rect,
            intersection_rect: rect,
            root_bounds: rect,
            is_visible: false,
        }];
        assert!(dom_api::update_intersections(
            &dom_api::ApiCx::new(cx),
            window,
            &computed,
            1.0
        ));
        elise::Persistent::new(cx.gc, window)
    });
    with_host(&dom, |host| {
        dom_api::notify_intersection_observers(host, &window)
    });
    assert!(
        dom.1.intersection_log.borrow().is_empty(),
        "no script called"
    );
    assert_eq!(update(&mut dom), ["https://example.test/a.png"]);
}

/// `sizes="auto"` where the element allows auto-sizes (lazy): its content
/// box's width as laid out (300px for one not loaded with no width, no
/// box: 100vw); a `<source>` without `sizes` takes it; and the document's
/// observer of those watches it. Without `loading=lazy`, 100vw.
#[test]
fn sizes_auto_is_the_width() {
    let (mut dom, body) = window(800.0, 1.0);
    let srcset = "s.png 100w, m.png 300w, l.png 800w";
    let narrow = add(
        &mut dom,
        &body,
        "img",
        &[
            ("loading", "lazy"),
            ("sizes", "auto"),
            ("width", "10"),
            ("srcset", srcset),
        ],
    );
    let no_width = add(
        &mut dom,
        &body,
        "img",
        &[("loading", "lazy"), ("sizes", "auto"), ("srcset", srcset)],
    );
    let eager = add(
        &mut dom,
        &body,
        "img",
        &[("sizes", "auto"), ("srcset", srcset)],
    );
    let ids = dom.read(|cx| [&narrow, &no_width, &eager].map(|e| cx.node_id(e.get(cx.gc))));
    fake(&dom, |layout| {
        layout.boxes.insert(
            ids[0],
            crate::geometry::block(0.0, 0.0, 10.0, 10.0, [0.0; 4]),
        );
        layout.boxes.insert(
            ids[1],
            crate::geometry::block(0.0, 0.0, 0.0, 10.0, [0.0; 4]),
        );
        layout.boxes.insert(
            ids[2],
            crate::geometry::block(0.0, 0.0, 10.0, 10.0, [0.0; 4]),
        );
    });
    update(&mut dom);
    assert_eq!(
        requested(&dom, &narrow).as_deref(),
        Some("https://example.test/s.png"),
        "10px"
    );
    assert_eq!(
        requested(&dom, &no_width).as_deref(),
        Some("https://example.test/m.png"),
        "300px"
    );
    assert_eq!(
        requested(&dom, &eager).as_deref(),
        Some("https://example.test/l.png"),
        "100vw"
    );
    let observed = dom.read(|cx| {
        let window = cx.window_of(cx.document_of(narrow.get(cx.gc))).unwrap();
        let observer = cx.window_resize_observers(window)[0];
        assert!(cx.is_auto_sizes_observer(observer));
        cx.resize_observations(observer).len()
    });
    assert_eq!(observed, 2, "the lazy ones");
    // In a `<picture>`, a `<source>` without `sizes` takes the `<img>`'s.
    let picture = add(&mut dom, &body, "picture", &[]);
    add(&mut dom, &picture, "source", &[("srcset", srcset)]);
    let pictured = add(
        &mut dom,
        &picture,
        "img",
        &[("loading", "lazy"), ("sizes", "auto")],
    );
    let id = dom.read(|cx| cx.node_id(pictured.get(cx.gc)));
    fake(&dom, |layout| {
        layout
            .boxes
            .insert(id, crate::geometry::block(0.0, 0.0, 90.0, 10.0, [0.0; 4]));
    });
    update(&mut dom);
    assert_eq!(
        requested(&dom, &pictured).as_deref(),
        Some("https://example.test/s.png"),
        "90px"
    );
}

/// Laid out at another width, a lazy `<img>` with `sizes="auto"` selects
/// again in the resize observer steps (Blink's `HTMLImageElement::
/// OnResize`), no script called: another image is updated to; the same
/// one only changes its density.
#[test]
fn sizes_auto_selects_again_as_the_width_changes() {
    let (mut dom, body) = window(800.0, 1.0);
    let img = add(
        &mut dom,
        &body,
        "img",
        &[
            ("loading", "lazy"),
            ("sizes", "auto"),
            ("width", "10"),
            ("srcset", "s.png 100w, l.png 800w"),
        ],
    );
    let id = dom.read(|cx| cx.node_id(img.get(cx.gc)));
    let width = |dom: &TestDom, width: f32| {
        fake(dom, |layout| {
            layout
                .boxes
                .insert(id, crate::geometry::block(0.0, 0.0, width, 10.0, [0.0; 4]));
        });
    };
    width(&dom, 10.0);
    update(&mut dom);
    let window = dom.read(|cx| {
        elise::Persistent::new(cx.gc, cx.window_of(cx.document_of(img.get(cx.gc))).unwrap())
    });
    let round = |dom: &TestDom| {
        with_host(dom, |host| dom_api::gather_and_broadcast(host, &window, 0));
    };
    // The first observation: the same image, nothing to fetch.
    round(&dom);
    assert!(update(&mut dom).is_empty());
    width(&dom, 500.0);
    round(&dom);
    assert!(dom.1.resize_log.borrow().is_empty(), "no script called");
    update(&mut dom);
    assert_eq!(
        requested(&dom, &img).as_deref(),
        Some("https://example.test/l.png")
    );
}

/// A frame never laid out (a child frame's, before its first layout) is
/// laid out before its images select, its viewport its iframe's then: not
/// the empty one it has before.
#[test]
fn a_frame_never_laid_out_is_laid_out_first() {
    let (mut dom, body) = window(0.0, 1.0);
    fake(&dom, |layout| layout.unknown_width = Some(400.0));
    let picture = add(&mut dom, &body, "picture", &[]);
    add(
        &mut dom,
        &picture,
        "source",
        &[("srcset", "wide.png"), ("media", "(min-width: 300px)")],
    );
    add(&mut dom, &picture, "img", &[("src", "empty.png")]);
    assert_eq!(update(&mut dom), ["https://example.test/wide.png"]);
}
