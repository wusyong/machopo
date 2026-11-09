//! WebDriver's queries of a document through a frame's layout of the
//! tests' (`Fake`): which elements are the document's still, what Find
//! Element(s) finds, an element's rect, name and attributes, and the
//! points and rects that clicks, actions and screenshots take of the
//! viewport.

use std::rc::Rc;

use dom::{NodeId, NodeRoot, QualName};
use dom_api::{
    ApiCx, ElementLocator, WebDriverAnswer, WebDriverQuery, WebDriverQueryError, webdriver_answer,
};
use gfx::{PointF, RectF};

use crate::TestDom;
use crate::geometry::{fake, id, page};

fn ask(
    dom: &mut TestDom,
    document: &Rc<NodeRoot>,
    query: &WebDriverQuery<'_>,
) -> Result<WebDriverAnswer, WebDriverQueryError> {
    let layout = dom.1.layout.clone();
    dom.mutate(|cx| {
        webdriver_answer(
            &ApiCx::with_layout_cell(cx, &layout),
            document.get(cx.gc),
            query,
        )
    })
}

fn elements(answer: Result<WebDriverAnswer, WebDriverQueryError>) -> Vec<NodeId> {
    match answer {
        Ok(WebDriverAnswer::Elements(found)) => found,
        other => panic!("not elements: {other:?}"),
    }
}

#[test]
fn an_element_is_the_documents_while_it_is_in_it() {
    let (mut dom, nodes, _) = page(false, &["div", "p"]);
    let (document, div) = (&nodes[0], id(&dom, &nodes[3]));
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::Element(div)),
        Ok(WebDriverAnswer::Element)
    );
    // Not an element: no such element; another document's, or taken out:
    // stale.
    let root = id(&dom, document);
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::Element(root)),
        Err(WebDriverQueryError::NoSuchElement)
    );
    let other = dom.mutate(|cx| {
        let other = cx.create_document(true);
        let element = cx.create_element(other, QualName::html("div"));
        cx.append_child(other, element).unwrap();
        cx.node_id(element)
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::Element(other)),
        Err(WebDriverQueryError::Stale)
    );
    let p = id(&dom, &nodes[4]);
    dom.mutate(|cx| cx.remove(nodes[4].get(cx.gc)));
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::Element(p)),
        Err(WebDriverQueryError::Stale)
    );
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::TagName(p)),
        Err(WebDriverQueryError::Stale)
    );
}

#[test]
fn find_looks_among_the_descendants() {
    let (mut dom, nodes, _) = page(false, &["div", "p", "div"]);
    let document = &nodes[0];
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    let inner = dom.mutate(|cx| {
        let inner = cx.create_element(cx.document_of(nodes[3].get(cx.gc)), QualName::html("div"));
        cx.append_child(nodes[3].get(cx.gc), inner).unwrap();
        cx.node_id(inner)
    });
    let find = |using, value, from| WebDriverQuery::Find { using, value, from };
    assert_eq!(
        elements(ask(
            &mut dom,
            document,
            &find(ElementLocator::Css, "div", None)
        )),
        [ids[3], inner, ids[5]],
        "in tree order"
    );
    assert_eq!(
        elements(ask(
            &mut dom,
            document,
            &find(ElementLocator::TagName, "div", None)
        )),
        [ids[3], inner, ids[5]]
    );
    // From an element: its descendants, not itself.
    assert_eq!(
        elements(ask(
            &mut dom,
            document,
            &find(ElementLocator::Css, "div", Some(ids[3]))
        )),
        [inner]
    );
    assert_eq!(
        elements(ask(
            &mut dom,
            document,
            &find(ElementLocator::TagName, "p", Some(ids[3]))
        )),
        []
    );
    assert_eq!(
        elements(ask(
            &mut dom,
            document,
            &find(ElementLocator::TagName, "body", None)
        )),
        [ids[2]]
    );
    // A qualified name, not a selector.
    let prefixed = dom.mutate(|cx| {
        let element = cx.create_element(
            cx.document_of(nodes[3].get(cx.gc)),
            QualName {
                namespace: Some("urn:x"),
                prefix: Some("x"),
                local: "y",
            },
        );
        cx.append_child(nodes[2].get(cx.gc), element).unwrap();
        cx.node_id(element)
    });
    assert_eq!(
        elements(ask(
            &mut dom,
            document,
            &find(ElementLocator::TagName, "x:y", None)
        )),
        [prefixed]
    );
    match ask(&mut dom, document, &find(ElementLocator::Css, "[", None)) {
        Err(WebDriverQueryError::InvalidSelector(message)) => assert!(!message.is_empty()),
        other => panic!("not an invalid selector: {other:?}"),
    }
    assert_eq!(
        ask(
            &mut dom,
            document,
            &find(ElementLocator::Css, "div", Some(ids[0]))
        ),
        Err(WebDriverQueryError::NoSuchElement),
        "from the document's node"
    );
}

#[test]
fn an_elements_rect_name_and_attributes() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let (document, div) = (&nodes[0], id(&dom, &nodes[3]));
    // The bounding rect, moved by the scroll offset.
    fake(&dom, |fake| {
        fake.rects.insert(
            div,
            vec![
                RectF::new(10.0, 20.0, 100.0, 40.0),
                RectF::new(0.0, 70.0, 50.0, 10.0),
            ],
        );
        fake.viewport.scroll = PointF::new(5.0, 7.0);
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::Rect(div)),
        Ok(WebDriverAnswer::Rect(RectF::new(5.0, 27.0, 110.0, 60.0)))
    );
    assert!(
        log.borrow().contains(&"layout".to_owned()),
        "laid out first"
    );
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::TagName(div)),
        Ok(WebDriverAnswer::Text(Some("div".to_owned()))),
        "not uppercased"
    );
    let prefixed = dom.mutate(|cx| {
        let element = cx.create_element(
            cx.document_of(nodes[3].get(cx.gc)),
            QualName {
                namespace: Some("urn:x"),
                prefix: Some("x"),
                local: "Thing",
            },
        );
        cx.append_child(nodes[3].get(cx.gc), element).unwrap();
        cx.set_attribute(nodes[3].get(cx.gc), "data-x", "1");
        cx.node_id(element)
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::TagName(prefixed)),
        Ok(WebDriverAnswer::Text(Some("x:Thing".to_owned())))
    );
    assert_eq!(
        ask(
            &mut dom,
            document,
            &WebDriverQuery::Attribute(div, "DATA-X")
        ),
        Ok(WebDriverAnswer::Text(Some("1".to_owned()))),
        "an HTML element's lowercased"
    );
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::Attribute(div, "nope")),
        Ok(WebDriverAnswer::Text(None))
    );
}

#[test]
fn a_click_is_at_the_center_of_what_is_in_view() {
    let (mut dom, nodes, log) = page(false, &["div", "p"]);
    let (document, div, p) = (&nodes[0], id(&dom, &nodes[3]), id(&dom, &nodes[4]));
    let inner = dom.mutate(|cx| {
        let inner = cx.create_element(cx.document_of(nodes[3].get(cx.gc)), QualName::html("span"));
        cx.append_child(nodes[3].get(cx.gc), inner).unwrap();
        cx.node_id(inner)
    });
    // Its first box, its part in the viewport: the center floored.
    fake(&dom, |fake| {
        fake.rects.insert(
            div,
            vec![
                RectF::new(790.0, -4.0, 21.0, 15.0),
                RectF::new(0.0, 0.0, 10.0, 10.0),
            ],
        );
        fake.at = Some(inner);
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::ClickPoint(div)),
        Ok(WebDriverAnswer::Point(PointF::new(795.0, 5.0)))
    );
    assert!(
        log.borrow().contains(&"hits 795 5".to_owned()),
        "{:?}",
        log.borrow()
    );
    assert!(
        log.borrow().contains(&"hit 795 5".to_owned()),
        "{:?}",
        log.borrow()
    );
    // Something else on top, the element under it: intercepted.
    fake(&dom, |fake| {
        fake.at = Some(p);
        fake.under = vec![div];
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::ClickPoint(div)),
        Err(WebDriverQueryError::ClickIntercepted)
    );
    // Not under the point at all, or out of view, or no box: not
    // interactable.
    let body = id(&dom, &nodes[2]);
    fake(&dom, |fake| fake.under = vec![body]);
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::ClickPoint(div)),
        Err(WebDriverQueryError::NotInteractable)
    );
    fake(&dom, |fake| {
        fake.under = vec![div];
        fake.rects
            .insert(div, vec![RectF::new(0.0, 600.0, 10.0, 10.0)]);
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::ClickPoint(div)),
        Err(WebDriverQueryError::NotInteractable)
    );
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::ClickPoint(p)),
        Err(WebDriverQueryError::NotInteractable)
    );
}

#[test]
fn an_actions_origin_and_a_screenshots_rect() {
    let (mut dom, nodes, _) = page(false, &["div"]);
    let (document, div) = (&nodes[0], id(&dom, &nodes[3]));
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::InViewCenter(div)),
        Err(WebDriverQueryError::OutOfBounds),
        "no box"
    );
    // The first box clamped to the viewport, its middle floored, whatever
    // is on top; a box of negative size is its rect the other way.
    fake(&dom, |fake| {
        fake.rects.insert(
            div,
            vec![
                RectF::new(-10.0, 590.0, 31.0, 21.0),
                RectF::new(0.0, 0.0, 800.0, 600.0),
            ],
        );
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::InViewCenter(div)),
        Ok(WebDriverAnswer::Point(PointF::new(10.0, 595.0)))
    );
    fake(&dom, |fake| {
        fake.rects
            .insert(div, vec![RectF::new(21.0, 611.0, -31.0, -21.0)]);
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::InViewCenter(div)),
        Ok(WebDriverAnswer::Point(PointF::new(10.0, 595.0)))
    );
    // A screenshot's: the bounding rect's part in view.
    fake(&dom, |fake| {
        fake.rects.insert(
            div,
            vec![
                RectF::new(-10.0, 590.0, 31.0, 21.0),
                RectF::new(0.0, 0.0, 5.0, 5.0),
            ],
        );
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::ScreenshotRect(div)),
        Ok(WebDriverAnswer::Rect(RectF::new(0.0, 0.0, 21.0, 600.0)))
    );
    fake(&dom, |fake| {
        fake.rects
            .insert(div, vec![RectF::new(800.0, 0.0, 10.0, 10.0)]);
    });
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::ScreenshotRect(div)),
        Err(WebDriverQueryError::NothingToCapture),
        "none of it in view"
    );
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::Viewport),
        Ok(WebDriverAnswer::Rect(RectF::new(0.0, 0.0, 800.0, 600.0)))
    );
    // No frame shows the document.
    dom.1.layout.set(None);
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::Viewport),
        Err(WebDriverQueryError::NotShown)
    );
    assert_eq!(
        ask(&mut dom, document, &WebDriverQuery::ScreenshotRect(div)),
        Err(WebDriverQueryError::NotShown)
    );
}

/// An automated check's element: the first match, its bounding rect (none
/// without a box), its text; none for a selector that doesn't parse or
/// matches nothing.
#[test]
fn a_probe_finds_the_first_match() {
    let (mut dom, nodes, _) = page(false, &["p", "p"]);
    let (document, first, second) = (&nodes[0], id(&dom, &nodes[3]), id(&dom, &nodes[4]));
    dom.mutate(|cx| {
        let document = cx.document_of(nodes[3].get(cx.gc));
        let text = cx.create_text(document, "one");
        cx.append_child(nodes[3].get(cx.gc), text).unwrap();
        let b = cx.create_element(document, QualName::html("b"));
        cx.append_child(nodes[3].get(cx.gc), b).unwrap();
        let inner = cx.create_text(document, " two");
        cx.append_child(b, inner).unwrap();
    });
    let probe = |dom: &mut TestDom, selector: &str| match ask(
        dom,
        document,
        &WebDriverQuery::Probe(selector),
    ) {
        Ok(WebDriverAnswer::Probed(found)) => found,
        other => panic!("not probed: {other:?}"),
    };
    let found = probe(&mut dom, "p").expect("a p");
    assert_eq!(
        (found.element, found.rect, found.text.as_str()),
        (first, None, "one two"),
        "no box"
    );
    fake(&dom, |fake| {
        fake.rects.insert(
            second,
            vec![
                RectF::new(1.0, 2.0, 3.0, 4.0),
                RectF::new(1.0, 6.0, 5.0, 4.0),
            ],
        );
    });
    let found = probe(&mut dom, "p + p").expect("the second");
    assert_eq!(
        (found.element, found.rect),
        (second, Some(RectF::new(1.0, 2.0, 5.0, 8.0)))
    );
    assert_eq!(probe(&mut dom, "div"), None);
    assert_eq!(probe(&mut dom, "["), None);
}

/// A child frame's document (its viewport at (100, 50) in the top-level
/// one's): a click's point and a screenshot's rect in the top-level
/// viewport, what is on top hit there through the frames (a hit in it is
/// in its iframe); an action's origin in its own viewport; its viewport
/// where it is; a probe's box in the top-level viewport.
#[test]
fn a_child_frames_points_are_in_the_top_level_viewport() {
    let (mut dom, nodes, log) = page(false, &["iframe"]);
    let iframe = &nodes[3];
    let (child, p) = dom.mutate(|cx| {
        let child = cx.create_document(true);
        let html = cx.create_element(child, QualName::html("html"));
        cx.append_child(child, html).unwrap();
        let p = cx.create_element(child, QualName::html("p"));
        cx.append_child(html, p).unwrap();
        cx.set_content_document(iframe.get(cx.gc), Some(child));
        (Rc::new(cx.root(child)), cx.node_id(p))
    });
    let child_id = id(&dom, &child);
    fake(&dom, |fake| {
        fake.frames.insert(child_id, PointF::new(100.0, 50.0));
        fake.rects
            .insert(p, vec![RectF::new(10.0, 10.0, 20.0, 20.0)]);
        fake.at = Some(p);
    });
    assert_eq!(
        ask(&mut dom, &child, &WebDriverQuery::ClickPoint(p)),
        Ok(WebDriverAnswer::Point(PointF::new(120.0, 70.0)))
    );
    // In view in its frame at its own point, on top at the top-level one.
    let log_now: Vec<String> = log.borrow_mut().drain(..).collect();
    assert!(log_now.contains(&"hits 20 20".to_owned()), "{log_now:?}");
    assert!(log_now.contains(&"hit 120 70".to_owned()), "{log_now:?}");
    assert_eq!(
        ask(&mut dom, &child, &WebDriverQuery::InViewCenter(p)),
        Ok(WebDriverAnswer::Point(PointF::new(20.0, 20.0)))
    );
    assert_eq!(
        ask(&mut dom, &child, &WebDriverQuery::ScreenshotRect(p)),
        Ok(WebDriverAnswer::Rect(RectF::new(110.0, 60.0, 20.0, 20.0)))
    );
    assert_eq!(
        ask(&mut dom, &child, &WebDriverQuery::Viewport),
        Ok(WebDriverAnswer::Rect(RectF::new(100.0, 50.0, 800.0, 600.0)))
    );
    match ask(&mut dom, &child, &WebDriverQuery::Probe("p")) {
        Ok(WebDriverAnswer::Probed(Some(probed))) => {
            assert_eq!(probed.rect, Some(RectF::new(110.0, 60.0, 20.0, 20.0)));
        }
        other => panic!("{other:?}"),
    }
    // The iframe clicked: the hit in its frame is in it.
    let iframe_id = id(&dom, iframe);
    fake(&dom, |fake| {
        fake.rects
            .insert(iframe_id, vec![RectF::new(100.0, 50.0, 300.0, 150.0)]);
    });
    assert_eq!(
        ask(&mut dom, &nodes[0], &WebDriverQuery::ClickPoint(iframe_id)),
        Ok(WebDriverAnswer::Point(PointF::new(250.0, 125.0)))
    );
}
