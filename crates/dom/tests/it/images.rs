//! An `<img>`'s requests: "update the image data" (the image the list of
//! available images has at once, else in its microtask), the fetches the
//! host makes and hands back, the pending request that keeps the current
//! image showing, the list shared by the DOM's documents (an image still
//! loading in it too), `load` and `error` in tasks, the load event they
//! delay, and `decode()`'s promises.

use dom::{
    Cx, Dom, ElementTaskKind, ImageEvent, ImageRequestState, ImageState, ImageTaskAction, NodeRef,
    QualName,
};

/// A document with a window, at `https://example.test/`.
fn window_document<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    cx.set_document_url(document, "https://example.test/");
    document
}

fn img<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, src: &str) -> NodeRef<'gc> {
    let element = cx.create_element(document, QualName::html("img"));
    cx.set_attribute(element, "src", src);
    element
}

/// An image of `width` by 1.
fn image(width: u32) -> dom::Image {
    dom::Image::Raster(
        gfx::ImageData::new(vec![0u8; 4]),
        gfx::Size { width, height: 1 },
    )
}

/// The microtasks of the updates queued, run.
fn run_updates(cx: &Cx<'_, '_>) {
    for element in cx.take_image_updates() {
        cx.run_image_update(element, cx.image_src(element));
    }
}

/// `document`'s window's element tasks, taken: the images'.
fn tasks<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> Vec<(NodeRef<'gc>, ImageTaskAction)> {
    let window = cx.window_of(document).unwrap();
    std::iter::from_fn(|| cx.take_element_task(window))
        .map(|(element, kind, _)| match kind {
            ElementTaskKind::Image(action) => (element, action),
            _ => panic!("an image's task"),
        })
        .collect()
}

fn state<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Option<(ImageRequestState, String)> {
    cx.image_request(element)
}

fn shows<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Option<u32> {
    match cx.image_state(element)? {
        ImageState::Loaded(image) => Some(image.size().width),
        ImageState::Loading | ImageState::Failed => None,
    }
}

/// The URLs the DOM's images wait for, taken (`Dom::take_image_fetches`
/// without the documents).
fn fetched_urls(dom: &Dom) -> Vec<String> {
    dom.take_image_fetches()
        .into_iter()
        .map(|fetch| fetch.url)
        .collect()
}

/// The event of a task, its generation's request's: fired or not.
fn event<'gc>(
    cx: &Cx<'_, 'gc>,
    element: NodeRef<'gc>,
    action: ImageTaskAction,
) -> Option<ImageEvent> {
    match action {
        ImageTaskAction::Event { load, generation } => {
            cx.take_image_event(element, load, generation)
        }
        _ => panic!("an event's task"),
    }
}

#[test]
fn a_request_waits_for_its_fetch_and_delays_the_load_event() {
    let mut dom = Dom::new();
    let (document, element) = dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        // Nothing the list has: the rest in a microtask.
        assert!(cx.image_update_queued(element));
        assert_eq!(
            state(cx, element).map(|(state, _)| state),
            Some(ImageRequestState::Unavailable)
        );
        (cx.root(document), cx.root(element))
    });
    assert!(dom.take_microtasks().images());
    assert!(dom.take_image_fetches().is_empty(), "the microtask fetches");
    dom.mutate(|cx| {
        let (document, element) = (document.get(cx.gc), element.get(cx.gc));
        run_updates(cx);
        assert_eq!(
            state(cx, element),
            Some((
                ImageRequestState::Unavailable,
                "https://example.test/a.png".into()
            ))
        );
        assert!(matches!(cx.image_state(element), Some(ImageState::Loading)));
        assert!(cx.load_event_delayed(document));
        assert!(tasks(cx, document).is_empty());
    });
    // Its document's, which asked, the element asking.
    let (id, asker) = dom.read(|cx| {
        (
            cx.node_id(document.get(cx.gc)),
            cx.node_id(element.get(cx.gc)),
        )
    });
    assert_eq!(
        dom.take_image_fetches(),
        [dom::ImageFetch {
            url: "https://example.test/a.png".to_owned(),
            document: id,
            initiator: dom::LoadInitiator::Element(asker),
        }]
    );
}

#[test]
fn a_fetched_image_is_the_current_requests_its_load_in_a_task() {
    let mut dom = Dom::new();
    let fetches = dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        run_updates(cx);
        (cx.root(document), cx.root(element))
    });
    assert_eq!(fetched_urls(&dom), ["https://example.test/a.png"]);
    dom.mutate(|cx| {
        let (document, element) = (fetches.0.get(cx.gc), fetches.1.get(cx.gc));
        cx.image_fetched("https://example.test/a.png", Some(image(7)), true);
        assert_eq!(shows(cx, element), Some(7));
        assert_eq!(cx.image_natural_size(element).width, 7);
        assert_eq!(
            state(cx, element).unwrap().0,
            ImageRequestState::CompletelyAvailable
        );
        // The load event waits for `load` to have fired.
        assert!(cx.load_event_delayed(document));
        let queued = tasks(cx, document);
        assert_eq!(queued.len(), 1);
        let fired = event(cx, element, queued[0].1).expect("its request's");
        assert!(fired.load);
        assert!(!cx.load_event_delayed(document));
    });
}

#[test]
fn the_list_of_available_images_is_the_doms_documents() {
    let mut dom = Dom::new();
    let first = dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        run_updates(cx);
        cx.root(element)
    });
    dom.take_image_fetches();
    dom.mutate(|cx| {
        // Another document's, while it loads: it waits for the same fetch.
        let other = window_document(cx);
        let waiting = img(cx, other, "https://example.test/a.png");
        run_updates(cx);
        assert_eq!(
            state(cx, waiting).unwrap().0,
            ImageRequestState::Unavailable
        );
        cx.image_fetched("https://example.test/a.png", Some(image(3)), true);
        assert_eq!(shows(cx, waiting), Some(3));
        assert_eq!(shows(cx, first.get(cx.gc)), Some(3));
        // Once loaded, a third's is available at once, `load` queued: no
        // microtask, no fetch.
        let third = window_document(cx);
        let element = img(cx, third, "a.png");
        assert!(!cx.image_update_queued(element));
        assert_eq!(shows(cx, element), Some(3));
        assert_eq!(tasks(cx, third).len(), 1);
    });
    assert!(dom.take_image_fetches().is_empty());
}

/// An image stays in the list while a document that used it hasn't shut
/// down (Blink's weak memory cache, each document's fetcher keeping what
/// it used): one document's going leaves it for another that took it; the
/// last one's takes it out, an element's request still showing it. One
/// that comes when no document uses it any more isn't kept.
#[test]
fn an_image_leaves_the_list_with_the_documents_that_used_it() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let first = window_document(cx);
        let element = img(cx, first, "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(3)), true);
        let second = window_document(cx);
        let taken = img(cx, second, "a.png");
        assert_eq!(shows(cx, taken), Some(3), "from the list");
        let url = "https://example.test/a.png";
        cx.release_document_images(first);
        assert!(cx.cached_image(url).is_some(), "the second uses it");
        cx.release_document_images(second);
        assert!(cx.cached_image(url).is_none());
        assert_eq!(shows(cx, element), Some(3), "its request holds it");
        // A background's document uses it too.
        let third = window_document(cx);
        assert!(cx.background_image(url, cx.node_id(third), None).is_none());
        cx.image_fetched(url, Some(image(3)), true);
        assert!(cx.cached_image(url).is_some());
        cx.release_document_images(third);
        assert!(cx.cached_image(url).is_none());
        // Released while it loads.
        let fourth = window_document(cx);
        let waiting = img(cx, fourth, "b.png");
        run_updates(cx);
        cx.release_document_images(fourth);
        cx.image_fetched("https://example.test/b.png", Some(image(5)), true);
        assert_eq!(shows(cx, waiting), Some(5));
        assert!(cx.cached_image("https://example.test/b.png").is_none());
    });
}

#[test]
fn a_new_src_is_the_pending_request_the_old_image_showing() {
    let mut dom = Dom::new();
    let (document, element) = dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(1)), true);
        tasks(cx, document);
        cx.set_attribute(element, "src", "b.png");
        run_updates(cx);
        // The pending request loads; the current shows its image still.
        assert!(cx.image_has_pending_request(element));
        assert_eq!(shows(cx, element), Some(1));
        assert_eq!(cx.image_natural_size(element).width, 1);
        assert!(cx.load_event_delayed(document));
        (cx.root(document), cx.root(element))
    });
    assert_eq!(dom.take_image_fetches().len(), 2);
    dom.mutate(|cx| {
        let (document, element) = (document.get(cx.gc), element.get(cx.gc));
        cx.image_fetched("https://example.test/b.png", Some(image(2)), true);
        assert!(!cx.image_has_pending_request(element));
        assert_eq!(shows(cx, element), Some(2));
        assert_eq!(state(cx, element).unwrap().1, "https://example.test/b.png");
        let queued = tasks(cx, document);
        assert!(event(cx, element, queued[0].1).unwrap().load);
    });
}

#[test]
fn a_failed_fetch_is_a_broken_image_and_leaves_the_list() {
    let mut dom = Dom::new();
    let (document, element) = dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", None, true);
        assert_eq!(state(cx, element).unwrap().0, ImageRequestState::Broken);
        assert!(matches!(cx.image_state(element), Some(ImageState::Failed)));
        let queued = tasks(cx, document);
        let failed = event(cx, element, queued[0].1).unwrap();
        assert!(!failed.load && failed.fire);
        (cx.root(document), cx.root(element))
    });
    dom.take_image_fetches();
    // Asked again, it is fetched again (Blink drops an errored resource).
    dom.mutate(|cx| {
        let element = element.get(cx.gc);
        cx.set_attribute(element, "src", "a.png");
        run_updates(cx);
        let _ = document;
    });
    assert_eq!(fetched_urls(&dom), ["https://example.test/a.png"]);
}

/// An image selected again as the environment changed (HTML's "react to
/// environment changes") that fails fires no `error`: its task still
/// ends its load delay; the next update of the image data fires one again.
#[test]
fn a_reselected_image_that_fails_fires_no_error() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(4)), true);
        drop(tasks(cx, document));
        cx.set_attribute(element, "srcset", "b.png");
        drop(tasks(cx, document));
        cx.reselect_image(element);
        let selected = Some(("b.png".to_owned(), 1.0));
        for element in cx.take_image_updates() {
            cx.run_image_update(element, selected.clone());
        }
        cx.image_fetched("https://example.test/b.png", None, true);
        let queued = tasks(cx, document);
        let failed = event(cx, element, queued[0].1).unwrap();
        assert!(!failed.load && !failed.fire, "no error");
        cx.set_attribute(element, "srcset", "c.png");
        let selected = Some(("c.png".to_owned(), 1.0));
        for element in cx.take_image_updates() {
            cx.run_image_update(element, selected.clone());
        }
        cx.image_fetched("https://example.test/c.png", None, true);
        let queued = tasks(cx, document);
        let failed = event(cx, element, queued.last().unwrap().1).unwrap();
        assert!(failed.fire, "an update of its own");
    });
}

#[test]
fn no_src_clears_the_image_at_once_and_fires_nothing() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(5)), true);
        tasks(cx, document);
        cx.remove_attribute(element, "src");
        // Gone before the microtask (Blink's loader clears it at once).
        assert_eq!(cx.image_state(element).map(|_| ()), None);
        assert_eq!(cx.image_natural_size(element).width, 0);
        run_updates(cx);
        assert!(tasks(cx, document).is_empty(), "no event without a `src`");
        assert!(!cx.load_event_delayed(document));
        // An empty one is an error.
        cx.set_attribute(element, "src", "");
        run_updates(cx);
        let queued = tasks(cx, document);
        assert!(!event(cx, element, queued[0].1).unwrap().load);
    });
}

#[test]
fn an_image_of_a_document_with_no_window_doesnt_load() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let element = img(cx, document, "https://example.test/a.png");
        run_updates(cx);
        assert!(!cx.load_event_delayed(document));
        assert!(!cx.image_has_pending_request(element));
    });
    assert!(dom.take_image_fetches().is_empty());
}

/// An update whose image the list has is done at once: the microtask an
/// earlier one queued does nothing, no fetch, one `load`.
#[test]
fn an_image_available_at_once_ends_an_earlier_update() {
    let mut dom = Dom::new();
    let (document, element) = dom.mutate(|cx| {
        let document = window_document(cx);
        let other = img(cx, document, "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(4)), true);
        tasks(cx, document);
        let element = img(cx, document, "x.png");
        assert!(cx.image_update_queued(element));
        cx.set_attribute(element, "src", "a.png");
        assert!(!cx.image_update_queued(element));
        run_updates(cx);
        assert_eq!(tasks(cx, document).len(), 1, "one `load`");
        let _ = other;
        (cx.root(document), cx.root(element))
    });
    // `a.png`'s alone: `x.png`'s microtask did nothing.
    assert_eq!(fetched_urls(&dom), ["https://example.test/a.png"]);
    dom.mutate(|cx| {
        assert_eq!(shows(cx, element.get(cx.gc)), Some(4));
        assert!(!cx.image_has_pending_request(element.get(cx.gc)));
        let _ = document;
    });
}

#[test]
fn an_update_since_makes_an_older_event_do_nothing() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(1)), true);
        // Another update before the task runs.
        cx.set_attribute(element, "src", "a.png#again");
        let queued = tasks(cx, document);
        assert!(event(cx, element, queued[0].1).is_none());
    });
}

#[test]
fn decode_waits_for_the_request_it_is_asked_of() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        // Asked before the update's microtask: the request it makes.
        let dom::ImageDecode::Pending(first) = cx.image_decode(element) else {
            panic!("a window's");
        };
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(1)), true);
        let queued = tasks(cx, document);
        let fired = event(cx, element, queued[0].1).unwrap();
        assert_eq!(fired.decodes, [first]);
        // Asked of an available image: resolved in a task, if it is still
        // the current one.
        let dom::ImageDecode::Pending(second) = cx.image_decode(element) else {
            panic!("a window's");
        };
        let queued = tasks(cx, document);
        assert_eq!(queued[0].1, ImageTaskAction::Decode { promise: second });
        assert!(cx.take_ready_decode(element, second));
        // One whose image was replaced (by another available one) before
        // its task: rejected.
        let other = img(cx, document, "o.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/o.png", Some(image(2)), true);
        tasks(cx, document);
        let dom::ImageDecode::Pending(replaced) = cx.image_decode(element) else {
            panic!("a window's");
        };
        cx.set_attribute(element, "src", "o.png");
        assert!(!cx.take_ready_decode(element, replaced));
        let _ = other;
        tasks(cx, document);
        // Asked, then `src` changed before the microtask: rejected.
        cx.set_attribute(element, "src", "b.png");
        let dom::ImageDecode::Pending(third) = cx.image_decode(element) else {
            panic!("a window's");
        };
        cx.set_attribute(element, "src", "c.png");
        let queued = tasks(cx, document);
        assert!(
            queued
                .iter()
                .any(|(at, action)| elise::Gc::ptr_eq(*at, element)
                    && *action == ImageTaskAction::Reject { promise: third })
        );
        // Of a document with no window: rejected now.
        let inert = cx.create_document(true);
        let other = img(cx, inert, "https://example.test/a.png");
        assert_eq!(cx.image_decode(other), dom::ImageDecode::Reject);
    });
}

#[test]
fn adoption_updates_the_image_data() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let inert = cx.create_document(true);
        let element = img(cx, inert, "https://example.test/a.png");
        run_updates(cx);
        let document = window_document(cx);
        let body = cx.create_element(document, QualName::html("body"));
        cx.append_child(document, body).unwrap();
        cx.append_child(body, element).unwrap();
        assert!(cx.image_update_queued(element));
        run_updates(cx);
        assert!(cx.load_event_delayed(document));
    });
    assert_eq!(fetched_urls(&dom), ["https://example.test/a.png"]);
}

/// In a `<picture>`: an `<img>` coming or going, a `<source>` before it
/// coming or going or changing its `srcset`, `sizes`, `media` or `type`
/// update its image data (HTML's relevant mutations); a `<source>` after
/// it, another attribute, or another parent don't.
#[test]
fn a_pictures_relevant_mutations() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let picture = cx.create_element(document, QualName::html("picture"));
        let source = cx.create_element(document, QualName::html("source"));
        cx.append_child(picture, source).unwrap();
        let element = img(cx, document, "a.png");
        run_updates(cx);
        let queued = || {
            let queued = cx.image_update_queued(element);
            run_updates(cx);
            queued
        };
        cx.append_child(picture, element).unwrap();
        assert!(queued(), "into a <picture>");
        for (name, value) in [
            ("srcset", "b.png"),
            ("sizes", "1px"),
            ("media", "all"),
            ("type", "image/png"),
        ] {
            cx.set_attribute(source, name, value);
            assert!(queued(), "{name}");
        }
        cx.set_attribute(source, "title", "x");
        assert!(!queued(), "another attribute");
        let after = cx.create_element(document, QualName::html("source"));
        cx.append_child(picture, after).unwrap();
        assert!(!queued(), "a <source> after it");
        cx.set_attribute(after, "srcset", "c.png");
        assert!(!queued(), "a <source> after it changed");
        let before = cx.create_element(document, QualName::html("source"));
        cx.insert(picture, before, Some(source));
        assert!(queued(), "a <source> before it came");
        cx.remove(before);
        assert!(queued(), "a <source> before it went");
        // Out of the <picture>, its image had: available at once, `load`.
        cx.image_fetched("https://example.test/a.png", Some(image(1)), true);
        tasks(cx, document);
        cx.remove(element);
        assert_eq!(tasks(cx, document).len(), 1, "out of the <picture>");
        // Out of one: a <source> before it in another parent is nothing.
        let div = cx.create_element(document, QualName::html("div"));
        let other = cx.create_element(document, QualName::html("source"));
        cx.append_child(div, other).unwrap();
        cx.append_child(div, element).unwrap();
        cx.set_attribute(other, "srcset", "d.png");
        assert!(!queued(), "a <source> of a <div>");
    });
}

/// The `<img>`s that use `srcset` or a `<picture>` are their document's to
/// select again as the environment changes: listed at their update, once;
/// those that no longer use them, or left for another document, aren't.
#[test]
fn the_images_to_select_again() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let plain = img(cx, document, "a.png");
        let set = img(cx, document, "a.png");
        cx.set_attribute(set, "srcset", "b.png 2x");
        let picture = cx.create_element(document, QualName::html("picture"));
        let pictured = img(cx, document, "a.png");
        cx.append_child(picture, pictured).unwrap();
        // Selected by the engine; here, as the `src` alone.
        run_updates(cx);
        let listed = cx.responsive_images(document);
        assert_eq!(listed.len(), 2);
        assert!(!listed.iter().any(|&at| elise::Gc::ptr_eq(at, plain)));
        cx.set_attribute(set, "srcset", "c.png 2x");
        run_updates(cx);
        assert_eq!(cx.responsive_images(document).len(), 2, "listed once");
        cx.remove_attribute(set, "srcset");
        assert_eq!(cx.responsive_images(document).len(), 1, "no longer");
        cx.set_attribute(set, "srcset", "d.png 2x");
        run_updates(cx);
        let other = window_document(cx);
        cx.adopt_node(set, other);
        run_updates(cx);
        let listed = cx.responsive_images(document);
        assert!(
            !listed.iter().any(|&at| elise::Gc::ptr_eq(at, set)),
            "another document's"
        );
        assert!(listed.iter().any(|&at| elise::Gc::ptr_eq(at, pictured)));
        assert_eq!(cx.responsive_images(other).len(), 1, "its new document's");
    });
}

/// A source's density: the natural size is the image's over it (truncated
/// for `naturalWidth`, as Chrome's; rounded for its box); the same image at
/// another density changes it alone.
#[test]
fn a_density_is_the_natural_sizes() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.png");
        let selected = Some(("a.png".to_owned(), 2.0));
        for element in cx.take_image_updates() {
            cx.run_image_update(element, selected.clone());
        }
        cx.image_fetched("https://example.test/a.png", Some(image(9)), true);
        assert_eq!(cx.image_natural_size(element).width, 4, "4.5 truncated");
        assert_eq!(shows(cx, element), Some(5), "4.5 rounded");
        assert!(cx.set_image_density(element, "https://example.test/a.png", 0.5));
        assert_eq!(cx.image_natural_size(element).width, 18);
        assert!(
            !cx.set_image_density(element, "https://example.test/b.png", 1.0),
            "another image"
        );
    });
}

/// An SVG image the host made: its natural size as `naturalWidth` says it
/// (one missing through the other and its ratio, else 300×150, with a
/// ratio alone the height through it), over its source's density where
/// the image gave the dimension (not the default's), as layout's
/// dimensions are; no canvas draws it.
#[test]
fn an_svg_images_natural_size() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "a.svg");
        let plain = img(cx, document, "b.svg");
        let wide = img(cx, document, "c.svg");
        for element in cx.take_image_updates() {
            let selected = cx.image_src(element).map(|(src, _)| (src, 2.0));
            cx.run_image_update(element, selected);
        }
        let natural = gfx::svg::SvgNaturalSize {
            width: Some(20.0),
            height: None,
            ratio: Some(2.0),
        };
        cx.image_fetched(
            "https://example.test/a.svg",
            Some(dom::Image::Svg(7, natural)),
            true,
        );
        let none = gfx::svg::SvgNaturalSize {
            width: None,
            height: None,
            ratio: Some(3.0),
        };
        cx.image_fetched(
            "https://example.test/b.svg",
            Some(dom::Image::Svg(8, none)),
            true,
        );
        let width_alone = gfx::svg::SvgNaturalSize {
            width: Some(20.0),
            height: None,
            ratio: None,
        };
        cx.image_fetched(
            "https://example.test/c.svg",
            Some(dom::Image::Svg(9, width_alone)),
            true,
        );
        assert_eq!(cx.image_natural_size(element), gfx::Size::new(10, 5));
        assert_eq!(
            cx.image_natural_size(plain),
            gfx::Size::new(300, 100),
            "the default's"
        );
        assert_eq!(cx.image_natural_size(wide), gfx::Size::new(10, 150));
        match cx.image_state(element) {
            Some(ImageState::Loaded(dom::Image::Svg(7, shown))) => assert_eq!(
                shown,
                gfx::svg::SvgNaturalSize {
                    width: Some(10.0),
                    height: None,
                    ratio: Some(2.0)
                }
            ),
            _ => panic!("the SVG image"),
        }
        assert!(matches!(
            cx.image_for_canvas(element),
            dom::ImageForCanvas::Nothing
        ));
    });
}

/// An image the list is loading is waited for at once (Blink's memory cache
/// has it): no microtask, one fetch; put into a `<picture>` then, its
/// update selects the same image, the request goes on, its `decode()`
/// standing (WPT's `image-decode-with-quick-attach.html`).
#[test]
fn an_image_loading_is_taken_at_once() {
    let mut dom = Dom::new();
    let (document, second, promise) = dom.mutate(|cx| {
        let document = window_document(cx);
        img(cx, document, "a.png");
        run_updates(cx);
        let second = img(cx, document, "a.png");
        assert!(!cx.image_update_queued(second), "at once");
        assert_eq!(
            state(cx, second),
            Some((
                ImageRequestState::Unavailable,
                "https://example.test/a.png".into()
            ))
        );
        let dom::ImageDecode::Pending(promise) = cx.image_decode(second) else {
            panic!("a window's");
        };
        let picture = cx.create_element(document, QualName::html("picture"));
        cx.append_child(picture, second).unwrap();
        run_updates(cx);
        assert!(
            tasks(cx, document).is_empty(),
            "nothing rejected, no event yet"
        );
        (cx.root(document), cx.root(second), promise)
    });
    assert_eq!(fetched_urls(&dom), ["https://example.test/a.png"]);
    dom.mutate(|cx| {
        let (document, second) = (document.get(cx.gc), second.get(cx.gc));
        cx.image_fetched("https://example.test/a.png", Some(image(3)), true);
        let fired: Vec<_> = tasks(cx, document)
            .into_iter()
            .filter(|(element, _)| elise::Gc::ptr_eq(*element, second))
            .collect();
        assert_eq!(fired.len(), 1);
        assert_eq!(event(cx, second, fired[0].1).unwrap().decodes, [promise]);
    });
}

/// With `srcset` or in a `<picture>`, an update keeps the image till its
/// microtask selects (no source at once is a `src`'s alone); the same image
/// the current has is kept, its `decode()` standing, `load` again.
#[test]
fn a_responsive_images_update_keeps_its_image() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let picture = cx.create_element(document, QualName::html("picture"));
        let source = cx.create_element(document, QualName::html("source"));
        cx.append_child(picture, source).unwrap();
        let element = cx.create_element(document, QualName::html("img"));
        cx.set_attribute(element, "srcset", "a.png");
        cx.append_child(picture, element).unwrap();
        let select = |cx: &Cx<'_, '_>| {
            for at in cx.take_image_updates() {
                cx.run_image_update(at, Some(("a.png".to_owned(), 1.0)));
            }
        };
        select(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(6)), true);
        tasks(cx, document);
        let dom::ImageDecode::Pending(promise) = cx.image_decode(element) else {
            panic!("a window's");
        };
        cx.set_attribute(element, "sizes", "10px");
        assert_eq!(shows(cx, element), Some(6), "kept till selected");
        cx.set_attribute(source, "media", "print");
        select(cx);
        assert_eq!(shows(cx, element), Some(6));
        assert!(!cx.image_has_pending_request(element));
        let queued = tasks(cx, document);
        assert!(queued.contains_decode(element, promise));
        assert!(cx.take_ready_decode(element, promise), "the same request");
        assert!(
            queued
                .iter()
                .any(|(at, action)| elise::Gc::ptr_eq(*at, element)
                    && matches!(action, ImageTaskAction::Event { load: true, .. })),
            "load again"
        );
    });
}

trait ContainsDecode<'gc> {
    fn contains_decode(&self, element: NodeRef<'gc>, promise: u64) -> bool;
}

impl<'gc> ContainsDecode<'gc> for Vec<(NodeRef<'gc>, ImageTaskAction)> {
    fn contains_decode(&self, element: NodeRef<'gc>, promise: u64) -> bool {
        self.iter().any(|(at, action)| {
            elise::Gc::ptr_eq(*at, element) && *action == ImageTaskAction::Decode { promise }
        })
    }
}

/// A lazy `<img>` (its `loading` lazy, a window) waits for its lazy load:
/// not fetched, its document's lazy load observer observing it, its load
/// event not delayed; resumed, fetched (still not delaying), no longer
/// observed, and its `load` fires once it comes.
#[test]
fn a_lazy_image_waits_for_its_lazy_load() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let element = cx.create_element(document, QualName::html("img"));
        cx.set_attribute(element, "loading", "LAZY");
        cx.set_attribute(element, "src", "a.png");
        run_updates(cx);
        assert!(cx.image_waits_for_lazy_load(element));
        assert_eq!(
            state(cx, element),
            Some((
                ImageRequestState::Unavailable,
                "https://example.test/a.png".to_owned()
            ))
        );
        assert!(!cx.load_event_delayed(document));
        let window = cx.window_of(document).unwrap();
        let observers = cx.window_intersection_observers(window);
        assert_eq!(observers.len(), 1);
        assert!(cx.is_lazy_load_observer(observers[0]));
        assert_eq!(cx.intersection_observations(observers[0]).len(), 1);
        // The same `src` again: it still waits.
        cx.set_attribute(element, "src", "a.png");
        run_updates(cx);
        assert!(cx.image_waits_for_lazy_load(element));
        cx.resume_lazy_image(element);
        assert!(cx.image_update_queued(element));
        assert!(
            cx.intersection_observations(observers[0]).is_empty(),
            "unobserved"
        );
        run_updates(cx);
        assert!(!cx.image_waits_for_lazy_load(element));
        assert!(
            !cx.load_event_delayed(document),
            "a lazy one doesn't delay it"
        );
        cx.image_fetched("https://example.test/a.png", Some(image(3)), true);
        assert_eq!(shows(cx, element), Some(3));
        let events: Vec<bool> = tasks(cx, document)
            .into_iter()
            .filter_map(|(element, action)| event(cx, element, action).map(|e| e.load))
            .collect();
        assert_eq!(events, [true]);
        // Resumed again: nothing waits, nothing done.
        cx.resume_lazy_image(element);
        assert!(!cx.image_update_queued(element));
    });
    assert_eq!(fetched_urls(&dom), ["https://example.test/a.png"]);
}

/// A lazy `<img>` takes an image the list of available images has at once
/// (Chrome's `ShouldLoadImmediately`); one in a document with no window
/// isn't lazy (scripting is disabled).
#[test]
fn a_lazy_image_takes_an_available_one() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let first = img(cx, document, "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(3)), true);
        let element = cx.create_element(document, QualName::html("img"));
        cx.set_attribute(element, "loading", "lazy");
        cx.set_attribute(element, "src", "a.png");
        run_updates(cx);
        assert!(!cx.image_waits_for_lazy_load(element));
        assert_eq!(shows(cx, element), Some(3));
        // One with `srcset` selects in its microtask: there too.
        let responsive = cx.create_element(document, QualName::html("img"));
        cx.set_attribute(responsive, "loading", "lazy");
        cx.set_attribute(responsive, "srcset", "a.png 1x");
        cx.set_attribute(responsive, "src", "a.png");
        run_updates(cx);
        assert!(!cx.image_waits_for_lazy_load(responsive));
        assert_eq!(shows(cx, responsive), Some(3));
        let _ = first;
    });
}

/// A lazy `<img>` showing its image, given another `src`: the new request
/// (the pending one, the old image showing) waits for its lazy load, and
/// resumed is fetched.
#[test]
fn a_lazy_pending_request_waits_and_resumes() {
    let mut dom = Dom::new();
    let element = dom.mutate(|cx| {
        let document = window_document(cx);
        let element = cx.create_element(document, QualName::html("img"));
        cx.set_attribute(element, "src", "a.png");
        run_updates(cx);
        cx.image_fetched("https://example.test/a.png", Some(image(3)), true);
        cx.set_attribute(element, "loading", "lazy");
        cx.set_attribute(element, "src", "b.png");
        run_updates(cx);
        assert!(cx.image_has_pending_request(element));
        assert!(cx.image_waits_for_lazy_load(element));
        assert_eq!(shows(cx, element), Some(3), "the old image showing");
        cx.root(element)
    });
    assert_eq!(fetched_urls(&dom), ["https://example.test/a.png"]);
    dom.mutate(|cx| {
        let element = element.get(cx.gc);
        cx.resume_lazy_image(element);
        run_updates(cx);
        assert!(!cx.image_waits_for_lazy_load(element));
    });
    assert_eq!(fetched_urls(&dom), ["https://example.test/b.png"]);
}

/// `loading` eager resumes one that waits: fetched, and (the update having
/// begun lazy) its load still not delayed. The parser sets `src` before
/// `loading`: the update, its microtask not run yet, begins as lazy as
/// `loading` is then (Blink's parser sets every attribute first).
#[test]
fn loading_eager_resumes_and_an_update_begins_as_lazy_as_loading_is() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let element = cx.create_element(document, QualName::html("img"));
        cx.set_attribute(element, "src", "a.png");
        cx.set_attribute(element, "loading", "lazy");
        run_updates(cx);
        assert!(cx.image_waits_for_lazy_load(element));
        cx.set_attribute(element, "loading", "eager");
        assert!(cx.image_update_queued(element));
        run_updates(cx);
        assert!(!cx.image_waits_for_lazy_load(element));
        assert!(!cx.load_event_delayed(document));
        // An update that begins eager delays it.
        let other = img(cx, document, "b.png");
        run_updates(cx);
        assert!(cx.load_event_delayed(document));
        let _ = other;
    });
    assert_eq!(
        fetched_urls(&dom),
        ["https://example.test/a.png", "https://example.test/b.png"]
    );
}

/// The document's observer of its lazy `<img>`s with `sizes="auto"`:
/// made at the first, first among the window's resize observers, and
/// what allows auto-sizes (lazy, `sizes` `auto` or starting with
/// `auto,`).
#[test]
fn the_auto_sizes_observer() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = window_document(cx);
        let window = cx.window_of(document).unwrap();
        let script = cx.create_resize_observer(window);
        let element = cx.create_element(document, QualName::html("img"));
        for (loading, sizes, allows) in [
            ("lazy", "auto", true),
            ("lazy", "AUTO, 100px", true),
            ("lazy", "auto 100px", false),
            ("eager", "auto", false),
        ] {
            cx.set_attribute(element, "loading", loading);
            cx.set_attribute(element, "sizes", sizes);
            assert_eq!(cx.allows_auto_sizes(element), allows, "{loading} {sizes}");
        }
        cx.observe_auto_sizes(element, true);
        let observers = cx.window_resize_observers(window);
        assert_eq!(observers.len(), 2);
        assert!(cx.is_auto_sizes_observer(observers[0]));
        assert!(elise::Gc::ptr_eq(observers[1], script));
        assert_eq!(cx.resize_observations(observers[0]).len(), 1);
        cx.observe_auto_sizes(element, false);
        assert!(cx.resize_observations(observers[0]).is_empty());
    });
}

/// An `<img>` nothing else holds (a `new Image()`) stays alive while its
/// request waits for its image: its window holds it, and lets it go once
/// the image came (the task queued holds it then), so that its `load`
/// still fires.
#[test]
fn a_waiting_image_is_held_by_its_window() {
    let mut dom = Dom::new();
    let (document, id) = dom.mutate(|cx| {
        let document = window_document(cx);
        let element = img(cx, document, "held.png");
        run_updates(cx);
        (cx.root(document), cx.node_id(element))
    });
    let _ = dom.take_microtasks();
    assert_eq!(fetched_urls(&dom), ["https://example.test/held.png"]);
    dom.collect();
    dom.read(|cx| {
        let window = cx.window_of(document.get(cx.gc)).unwrap();
        assert!(cx.node_by_id(id).is_some(), "held while it waits");
        assert_eq!(cx.pending_images_for_testing(window), 1);
    });
    dom.mutate(|cx| {
        cx.image_fetched("https://example.test/held.png", Some(image(3)), true);
        let window = cx.window_of(document.get(cx.gc)).unwrap();
        assert_eq!(
            cx.pending_images_for_testing(window),
            0,
            "its task holds it now"
        );
    });
    dom.collect();
    dom.mutate(|cx| {
        let document = document.get(cx.gc);
        let tasks = tasks(cx, document);
        assert_eq!(tasks.len(), 1);
        assert_eq!(cx.node_id(tasks[0].0), id);
    });
    dom.collect();
    dom.read(|cx| {
        assert!(
            cx.node_by_id(id).is_none(),
            "nothing holds it after its task"
        )
    });
}
