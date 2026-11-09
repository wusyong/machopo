//! The sheets of `<style>`s and `<link>`s, which their elements keep: made
//! at the first ask, the same while the element's (an edit stays, its
//! `media` attribute is its media), a new one once the text changed or the
//! element went; a `<link>`'s as the host gave it, a new one of the same
//! contents after it came back. The `<style>`s of a document with the
//! same text share their contents. An edit of a sheet's rules marks its
//! tree scope's sheets as changed.

use std::sync::Arc;

use css::StyleSheetContents;
use dom::{
    Cx, Dom, NodeRef, QualName, RuleParent, Rules, ShadowRootInit, ShadowRootMode,
    SlotAssignmentMode, StyleFlags,
};
use elise::Gc;

/// A document at `https://example.test/` with a window and a `<body>`:
/// the `<body>`.
fn page<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    cx.set_document_url(document, "https://example.test/");
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    body
}

/// A `<style>` of `text` in `parent`.
fn style<'gc>(cx: &Cx<'_, 'gc>, parent: NodeRef<'gc>, text: &str) -> NodeRef<'gc> {
    let style = cx.create_element(cx.document_of(parent), QualName::html("style"));
    let text = cx.create_text(cx.document_of(parent), text);
    cx.append_child(style, text).unwrap();
    cx.append_child(parent, style).unwrap();
    style
}

fn contents(css: &str) -> Arc<StyleSheetContents> {
    Arc::new(StyleSheetContents::parse(css, None))
}

#[test]
fn a_styles_sheet_is_the_same_while_its_text_is() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let detached = cx.create_element(cx.document_of(body), QualName::html("style"));
        assert!(cx.owner_sheet(detached).is_none(), "not connected");
        let s = style(cx, body, "p { color: red }");
        let sheet = cx.owner_sheet(s).expect("a sheet");
        assert!(Gc::ptr_eq(cx.owner_sheet(s).unwrap(), sheet));
        assert!(Gc::ptr_eq(cx.style_sheet_owner(sheet).unwrap(), s));
        assert!(!cx.style_sheet_is_constructed(sheet));
        assert_eq!(cx.style_sheet_href(sheet), None);
        assert_eq!(
            cx.style_sheet_contents(sheet)
                .base_url()
                .map(url::Url::as_str),
            Some("https://example.test/"),
            "against its document's URL"
        );

        // `media`: the same sheet's.
        cx.set_attribute(s, "media", "print");
        assert!(Gc::ptr_eq(cx.owner_sheet(s).unwrap(), sheet));
        assert_eq!(cx.style_sheet_media(sheet).0, "print");

        // An edit stays; the text changing makes a new one.
        let edited = cx
            .style_sheet_contents(sheet)
            .insert_rule("q {}", 0)
            .unwrap();
        cx.css_rule_inserted(RuleParent::Sheet(sheet), Rules::Sheet(edited), 0);
        assert!(Gc::ptr_eq(cx.owner_sheet(s).unwrap(), sheet));
        assert_eq!(cx.style_sheet_contents(sheet).rules().len(), 2);
        let text = cx.first_child(s).unwrap();
        cx.set_data(text, "p { color: blue }");
        let new = cx.owner_sheet(s).unwrap();
        assert!(!Gc::ptr_eq(new, sheet));
        assert!(cx.style_sheet_owner(sheet).is_none(), "no longer its");
        assert_eq!(cx.style_sheet_media(new).0, "print");
        assert_eq!(cx.style_sheet_contents(new).rules().len(), 1);

        // A child taken out: another.
        let child = style(cx, s, "");
        let before = cx.owner_sheet(s).unwrap();
        cx.remove(child);
        assert!(!Gc::ptr_eq(cx.owner_sheet(s).unwrap(), before));
        let new = cx.owner_sheet(s).unwrap();

        // Taken out and put back: another.
        cx.remove(s);
        assert!(cx.style_sheet_owner(new).is_none());
        cx.append_child(body, s).unwrap();
        let back = cx.owner_sheet(s).unwrap();
        assert!(!Gc::ptr_eq(back, new));

        // Not CSS's `type`: none, and none of its old one.
        cx.set_attribute(s, "type", "text/plain");
        assert!(cx.owner_sheet(s).is_none());
        assert!(cx.style_sheet_owner(back).is_none());
        let svg = cx.create_element(
            cx.document_of(body),
            QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "style",
            },
        );
        cx.append_child(body, svg).unwrap();
        assert!(cx.owner_sheet(svg).is_some(), "an SVG <style>'s");
    });
}

#[test]
fn a_links_sheet_is_as_the_host_gave_it() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let link = cx.create_element(cx.document_of(body), QualName::html("link"));
        cx.set_attribute(link, "rel", "stylesheet");
        cx.set_attribute(link, "href", "a.css");
        cx.set_attribute(link, "media", "screen");
        cx.append_child(body, link).unwrap();
        assert!(cx.owner_sheet(link).is_none(), "not loaded");
        cx.set_link_sheet(link, contents("p {}"), "https://other.test/a.css", false);
        let sheet = cx.owner_sheet(link).unwrap();
        assert_eq!(cx.style_sheet_href(sheet), Some("https://other.test/a.css"));
        assert!(!cx.style_sheet_origin_clean(sheet));
        assert_eq!(cx.style_sheet_media(sheet).0, "screen");
        let loaded = cx.style_sheet_contents(sheet).clone();

        // Back after it went: a new sheet of the contents loaded, as they
        // were (not as edited).
        let edited = loaded.delete_rule(0).unwrap();
        cx.css_rule_deleted(RuleParent::Sheet(sheet), Rules::Sheet(edited), 0);
        cx.remove(link);
        cx.append_child(body, link).unwrap();
        let back = cx.owner_sheet(link).unwrap();
        assert!(!Gc::ptr_eq(back, sheet));
        assert!(Arc::ptr_eq(cx.style_sheet_contents(back), &loaded));
        assert_eq!(cx.style_sheet_href(back), Some("https://other.test/a.css"));
        assert!(!cx.style_sheet_origin_clean(back));

        // No longer a stylesheet `<link>`: none; again one, a new one.
        cx.set_attribute(link, "rel", "icon");
        assert!(cx.owner_sheet(link).is_none());
        cx.set_attribute(link, "rel", "stylesheet");
        assert!(!Gc::ptr_eq(cx.owner_sheet(link).unwrap(), back));

        // Another load: another sheet.
        let before = cx.owner_sheet(link).unwrap();
        cx.set_link_sheet(link, contents("q {}"), "https://example.test/b.css", true);
        let reloaded = cx.owner_sheet(link).unwrap();
        assert!(!Gc::ptr_eq(reloaded, before));
        assert!(cx.style_sheet_owner(before).is_none());
        assert!(cx.style_sheet_origin_clean(reloaded));
    });
}

/// The `<style>`s of a document with the same text, in its tree or in
/// its shadow trees, share their contents; other text, other contents.
#[test]
fn styles_of_the_same_text_share_their_contents() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let mut roots = Vec::new();
        for _ in 0..3 {
            let host = cx.create_element(cx.document_of(body), QualName::html("div"));
            cx.append_child(body, host).unwrap();
            let root = cx.attach_shadow(
                host,
                ShadowRootInit {
                    mode: ShadowRootMode::Open,
                    delegates_focus: false,
                    slot_assignment: SlotAssignmentMode::Named,
                    clonable: false,
                    serializable: false,
                },
            );
            roots.push(style(cx, root, ":host { color: red }"));
        }
        let other = style(cx, body, ":host { color: blue }");
        let sheets: Vec<_> = roots.iter().map(|s| cx.owner_sheet(*s).unwrap()).collect();
        let first = cx.style_sheet_contents(sheets[0]);
        for sheet in &sheets[1..] {
            assert!(Arc::ptr_eq(cx.style_sheet_contents(*sheet), first));
        }
        assert!(!Arc::ptr_eq(
            cx.style_sheet_contents(cx.owner_sheet(other).unwrap()),
            first
        ));
        assert_eq!(
            cx.owner_sheets_in(cx.document_of(body)).len(),
            1,
            "its own tree's"
        );

        // Its URL changed: parsed again, against it.
        cx.set_document_url(cx.document_of(body), "https://example.test/b/");
        let later = style(cx, body, ":host { color: red }");
        let later = cx.style_sheet_contents(cx.owner_sheet(later).unwrap());
        assert!(!Arc::ptr_eq(later, first));
        assert_eq!(
            later.base_url().map(url::Url::as_str),
            Some("https://example.test/b/")
        );
    });
}

/// An edit of an element's sheet, or of its media or `disabled`, marks its
/// tree scope's sheets as changed: the document's, a shadow root's.
#[test]
fn an_edit_marks_the_sheets_as_changed() {
    let mut dom = Dom::new();
    dom.mutate(edits_mark_the_sheets);
}

fn edits_mark_the_sheets<'gc>(cx: &Cx<'_, 'gc>) {
    {
        let body = page(cx);
        let document = cx.document_of(body);
        let s = style(cx, body, "p {}");
        let sheet = cx.owner_sheet(s).unwrap();
        let changed = |cx: &Cx<'_, 'gc>| {
            cx.style_flags(document)
                .contains(StyleFlags::STYLE_SHEETS_CHANGED)
        };
        let clear = |cx: &Cx<'_, 'gc>| cx.clear_style_flags(document);
        clear(cx);
        let edited = cx
            .style_sheet_contents(sheet)
            .insert_rule("q {}", 1)
            .unwrap();
        cx.css_rule_inserted(RuleParent::Sheet(sheet), Rules::Sheet(edited), 1);
        assert!(changed(cx), "a rule inserted");
        clear(cx);
        cx.set_style_sheet_disabled(sheet, true);
        assert!(changed(cx), "disabled");
        clear(cx);
        let rule = cx.rule_at(RuleParent::Sheet(sheet), 0).unwrap();
        let renamed = cx
            .css_rule(rule)
            .as_style()
            .unwrap()
            .with_selector_text("b")
            .unwrap();
        cx.set_css_rule(rule, css::CssRule::Style(Arc::new(renamed)));
        assert!(changed(cx), "a rule changed");
        assert_eq!(
            cx.style_sheet_contents(sheet).rules()[0].css_text(),
            "b { }"
        );

        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(body, host).unwrap();
        let root = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let inner = style(cx, root, "p {}");
        let inner = cx.owner_sheet(inner).unwrap();
        cx.take_sheet_changed_roots(document);
        cx.set_style_sheet_media(
            inner,
            "print",
            Arc::new(css::media::MediaQueryList::parse("print")),
        );
        assert_eq!(
            cx.take_sheet_changed_roots(document).len(),
            1,
            "the shadow root's"
        );

        // One no longer the element's marks nothing.
        cx.remove(s);
        clear(cx);
        cx.set_style_sheet_disabled(sheet, false);
        assert!(!changed(cx));
    }
}

/// A shadow root connected again is in the list of those whose sheets may
/// have changed: a `<style>` taken out of it while it wasn't marked
/// nothing.
#[test]
fn a_shadow_root_connected_again_is_collected_again() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let host = cx.create_element(cx.document_of(body), QualName::html("div"));
        cx.append_child(body, host).unwrap();
        let root = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let s = style(cx, root, "p {}");
        cx.remove(host);
        cx.remove(s);
        let document = cx.document_of(body);
        cx.take_sheet_changed_roots(document);
        cx.append_child(body, host).unwrap();
        let roots = cx.take_sheet_changed_roots(document);
        assert_eq!(roots.len(), 1);
        assert!(Gc::ptr_eq(roots[0], root));
    });
}

/// A document's `MediaQueryList`s: evaluated in the order made, only when
/// the environment changed; one with a listener kept with nothing else
/// holding it, till an evaluation finds it has none; one without, let go.
#[test]
fn media_query_lists_are_kept_while_they_have_listeners() {
    use css::media::{MediaEnvironment, MediaQueryList};
    let mut dom = Dom::new();
    let document = dom.mutate(|cx| cx.root(cx.document_of(page(cx))));
    let environment = |width| MediaEnvironment {
        width,
        height: 100.0,
        device_pixel_ratio: 1.0,
    };
    let listener = dom.mutate(|cx| {
        let document = document.get(cx.gc);
        let queries = Arc::new(MediaQueryList::parse("(min-width: 1px)"));
        let listened = cx.create_media_query_list(document, queries.clone(), true);
        let listener = cx.add_listener(
            dom::TargetRef::EventTarget(listened),
            dom::names::event::CHANGE,
            Default::default(),
        );
        cx.create_media_query_list(document, queries, true);
        assert!(cx.has_media_query_lists(document));
        (
            elise::Persistent::new(cx.gc, listened),
            elise::Persistent::new(cx.gc, listener),
        )
    });
    let (listened, listener) = listener;
    let count = |dom: &mut Dom, width| {
        dom.mutate(|cx| {
            cx.media_query_lists_to_evaluate(document.get(cx.gc), environment(width))
                .len()
        })
    };
    assert_eq!(count(&mut dom, 10.0), 2, "both, in the first environment");
    assert_eq!(count(&mut dom, 10.0), 0, "the same environment");
    drop(listened);
    dom.collect();
    assert_eq!(count(&mut dom, 20.0), 1, "the one with a listener, kept");
    // Its listener taken off: the next evaluation lets go of it.
    dom.mutate(|cx| {
        let list = cx.media_query_lists_to_evaluate(document.get(cx.gc), environment(30.0))[0];
        cx.remove_listener(dom::TargetRef::EventTarget(list), listener.get(cx.gc));
    });
    drop(listener);
    assert_eq!(
        count(&mut dom, 40.0),
        1,
        "still kept: the evaluation then is what looks"
    );
    dom.collect();
    assert_eq!(count(&mut dom, 50.0), 0, "let go of");
}
