//! Editability from the DOM: the nearest HTML element whose
//! `contenteditable` isn't inherit decides, through the nodes a parent's
//! editability reaches (an HTML element, an `<svg>`, a `<math>`, a node
//! that isn't an element under an HTML element); an editing host is the
//! outermost editable element of its run; what a caret can't be in.

use dom::{ContentEditable, QualName};

const SVG: &str = "http://www.w3.org/2000/svg";

#[test]
fn editability_reaches_through_the_tree() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let element = |name: QualName<'_>, parent| {
            let element = cx.create_element(doc, name);
            cx.append_child(parent, element).unwrap();
            element
        };
        let host = cx.create_element(doc, QualName::html("div"));
        cx.append_child(doc, host).unwrap();
        cx.set_attribute(host, "contenteditable", "");
        let text = cx.create_text(doc, "x");
        cx.append_child(host, text).unwrap();
        let comment = cx.create_comment(doc, "c");
        cx.append_child(host, comment).unwrap();
        let svg = element(
            QualName {
                namespace: Some(SVG),
                prefix: None,
                local: "svg",
            },
            host,
        );
        let g = element(
            QualName {
                namespace: Some(SVG),
                prefix: None,
                local: "g",
            },
            svg,
        );
        let svg_text = cx.create_text(doc, "in svg");
        cx.append_child(svg, svg_text).unwrap();
        let off = element(QualName::html("b"), host);
        cx.set_attribute(off, "contenteditable", "false");
        let off_text = cx.create_text(doc, "y");
        cx.append_child(off, off_text).unwrap();
        let inner = element(QualName::html("i"), off);
        cx.set_attribute(inner, "contenteditable", "TRUE");
        let span = element(QualName::html("span"), inner);
        assert_eq!(
            [
                host, text, comment, svg, g, svg_text, off, off_text, inner, span
            ]
            .map(|node| cx.is_editable(node)),
            [
                true, true, true, true, false, false, false, false, true, true
            ]
        );
        assert!(!cx.is_editable(doc));
        assert_eq!(cx.content_editable(inner), Some(ContentEditable::True));
        assert_eq!(cx.content_editable(svg), None);
        assert_eq!(cx.content_editable(span), Some(ContentEditable::Inherit));
        assert_eq!(
            [host, svg, off, inner, span].map(|node| cx.is_editing_host(node)),
            [true, false, false, true, false]
        );
        // The outermost of each run: an inner one's, past one not
        // editable; a text's by its parent.
        let host_of = |node| cx.editing_host_of(node);
        assert!(host_of(text).is_some_and(|found| dom::NodeRef::ptr_eq(found, host)));
        assert!(host_of(svg).is_some_and(|found| dom::NodeRef::ptr_eq(found, host)));
        assert!(host_of(span).is_some_and(|found| dom::NodeRef::ptr_eq(found, inner)));
        assert!(host_of(off_text).is_none());
        assert!(host_of(g).is_none());
        // A host inside an editable element is the outer one's.
        let outer = cx.create_element(doc, QualName::html("section"));
        cx.set_attribute(outer, "contenteditable", "plaintext-only");
        cx.append_child(outer, host).unwrap();
        assert!(!cx.is_editing_host(host));
        assert!(host_of(text).is_some_and(|found| dom::NodeRef::ptr_eq(found, outer)));
    });
}

#[test]
fn a_caret_isnt_in_what_ignores_content() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        for (name, ignores) in [
            ("img", true),
            ("input", true),
            ("hr", true),
            ("textarea", true),
            ("div", false),
            ("table", false),
        ] {
            let element = cx.create_element(doc, QualName::html(name));
            assert_eq!(cx.editing_ignores_content(element), ignores, "{name}");
        }
        let svg_img = cx.create_element(
            doc,
            QualName {
                namespace: Some(SVG),
                prefix: None,
                local: "img",
            },
        );
        assert!(!cx.editing_ignores_content(svg_img));
    });
}
