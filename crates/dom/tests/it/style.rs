//! What the DOM keeps for style and layout: the flags its changes set and
//! mark up the ancestors (only in a connected tree), what changed of an
//! element's attributes, the stylesheets' owners, the trees' version,
//! node ids given as asked for, and an element's style, read and written
//! with the thread's token.

use std::sync::Arc;

use computed_style::ComputedStyle;
use dom::{
    AttributeChanges, Cx, Dom, LayoutTreeFlags as L, LocalToken, NodeRef, QualName,
    StyleFlags as S, StyleNames, names,
};

/// A document with `<html><body>` connected, every flag cleared: styled,
/// by rules that read every change (`StyleNames::everything`).
fn page<'gc>(cx: &Cx<'_, 'gc>) -> (NodeRef<'gc>, NodeRef<'gc>, NodeRef<'gc>) {
    let document = cx.create_document(true);
    let html = cx.create_element(document, QualName::html("html"));
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(document, html).unwrap();
    cx.append_child(html, body).unwrap();
    cx.set_style_names(
        document,
        StyleNames {
            everything: true,
            ..StyleNames::default()
        },
    );
    clear(cx, document);
    (document, html, body)
}

/// Every flag of `root`'s subtree cleared: styled and laid out.
fn clear<'gc>(cx: &Cx<'_, 'gc>, root: NodeRef<'gc>) {
    cx.clear_style_flags(root);
    cx.clear_layout_tree_flags(root);
    let mut child = cx.first_child(root);
    while let Some(node) = child {
        clear(cx, node);
        child = cx.next_sibling(node);
    }
}

#[test]
fn inserting_marks_the_node_and_its_ancestors() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, html, body) = page(cx);
        let div = cx.create_element(document, QualName::html("div"));
        cx.append_child(body, div).unwrap();
        assert!(cx.is_connected(div));
        assert_eq!(cx.style_flags(div), S::NEEDS_STYLE_RECALC);
        assert_eq!(
            cx.style_flags(body),
            S::CHILDREN_CHANGED | S::CHILD_NEEDS_STYLE_RECALC
        );
        assert_eq!(cx.style_flags(html), S::CHILD_NEEDS_STYLE_RECALC);
        assert_eq!(cx.style_flags(document), S::CHILD_NEEDS_STYLE_RECALC);
        assert_eq!(cx.layout_tree_flags(div), L::NEEDS_REATTACH);
        for ancestor in [body, html, document] {
            assert_eq!(cx.layout_tree_flags(ancestor), L::CHILD_NEEDS_UPDATE);
        }
        // Marking stops at the first ancestor marked already: the
        // document's cleared, `<html>`'s left, nothing marks it again.
        cx.clear_style_flags(document);
        cx.clear_style_flags(body);
        let span = cx.create_element(document, QualName::html("span"));
        cx.append_child(body, span).unwrap();
        assert_eq!(cx.style_flags(document), S::CLEAN);
        // The sibling before it may match otherwise (`:last-child`).
        assert!(cx.style_flags(div).contains(S::PRECEDES_CHANGE));
        // A styled node moved is styled again where it is now.
        clear(cx, document);
        cx.append_child(body, div).unwrap();
        assert_eq!(cx.style_flags(div), S::NEEDS_STYLE_RECALC);
        assert_eq!(cx.layout_tree_flags(div), L::NEEDS_REATTACH);
    });
}

#[test]
fn a_tree_built_apart_is_marked_as_it_is_inserted() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, _, body) = page(cx);
        let div = cx.create_element(document, QualName::html("div"));
        let span = cx.create_element(document, QualName::html("span"));
        cx.append_child(div, span).unwrap();
        // Never styled, and nothing marked above them.
        assert_eq!(cx.style_flags(span), S::NEEDS_STYLE_RECALC);
        assert_eq!(cx.style_flags(div), S::NEEDS_STYLE_RECALC);
        assert_eq!(cx.layout_tree_flags(div), L::CLEAN);
        assert!(!cx.is_connected(div) && !cx.is_connected(span));
        cx.append_child(body, div).unwrap();
        assert!(cx.is_connected(div) && cx.is_connected(span));
        assert!(cx.style_flags(body).contains(S::CHILD_NEEDS_STYLE_RECALC));
        // Removed, its subtree isn't connected; changing it marks nothing.
        cx.remove(div);
        assert!(!cx.is_connected(span));
        clear(cx, div);
        cx.set_attribute(span, "class", "x");
        assert_eq!(cx.style_flags(span), S::CLEAN);
        assert_eq!(cx.style_flags(div), S::CLEAN);
        assert!(cx.take_attribute_changes(span).is_none());
    });
}

#[test]
fn removing_marks_the_parent_and_the_siblings() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, _, body) = page(cx);
        let [a, b, c] = ["a", "b", "c"].map(|name| {
            let element = cx.create_element(document, QualName::html(name));
            cx.append_child(body, element).unwrap();
            element
        });
        // Texts between: the siblings marked are the elements.
        let text = cx.create_text(document, " ");
        cx.insert(body, text, Some(c));
        clear(cx, document);
        cx.remove(b);
        assert!(!cx.is_connected(b));
        assert_eq!(
            cx.style_flags(body),
            S::CHILDREN_CHANGED | S::CHILD_NEEDS_STYLE_RECALC
        );
        assert_eq!(cx.layout_tree_flags(body), L::CHILDREN_CHANGED);
        assert_eq!(cx.style_flags(a), S::PRECEDES_CHANGE);
        assert_eq!(cx.style_flags(c), S::FOLLOWS_CHANGE);
        assert_eq!(cx.style_flags(text), S::CLEAN);
    });
}

#[test]
fn an_attribute_change_is_noted_for_style() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, _, body) = page(cx);
        let div = cx.create_element(document, QualName::html("div"));
        let next = cx.create_element(document, QualName::html("p"));
        cx.append_child(body, div).unwrap();
        cx.append_child(body, next).unwrap();
        // Still to be styled whole: nothing to note.
        cx.set_attribute(div, "class", "a b");
        assert!(cx.take_attribute_changes(div).is_none());
        clear(cx, document);
        cx.set_attribute(div, "class", "b c");
        cx.set_attribute(div, "id", "x");
        cx.set_attribute(div, "title", "t");
        assert_eq!(cx.style_flags(div), S::ATTRIBUTES_CHANGED);
        // Its later siblings are the style's sibling sets', not marked.
        assert_eq!(cx.style_flags(next), S::CLEAN);
        assert_eq!(
            cx.style_flags(body),
            S::CHILDREN_ATTRIBUTES_CHANGED | S::CHILD_NEEDS_STYLE_RECALC
        );
        let changes = cx.take_attribute_changes(div).unwrap();
        assert_eq!(
            changes.attributes,
            [names::CLASS, names::ID, names::local::TITLE]
        );
        assert_eq!(changes.classes, ["a".into(), "c".into()]);
        assert_eq!(changes.ids, ["x".into()]);
        assert!(cx.take_attribute_changes(div).is_none(), "taken");
        // Removed, an id is noted as it was.
        cx.remove_attribute(div, "id");
        assert_eq!(cx.take_attribute_changes(div).unwrap().ids, ["x".into()]);
        // The style phase looked: what changed is dropped with the flags.
        cx.set_attribute(div, "title", "u");
        cx.clear_style_flags(div);
        assert!(cx.take_attribute_changes(div).is_none());
    });
}

/// The changes style reads are noted, the others aren't (nor marked): an
/// attribute by its name, the classes that came or went, the old and new
/// ids in any case, a state; a document never styled notes none. The debug
/// build keeps those passed over for style to check.
#[test]
fn only_what_style_reads_is_noted() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, _, body) = page(cx);
        let div = cx.create_element(document, QualName::html("div"));
        cx.append_child(body, div).unwrap();
        cx.set_attribute(div, "class", "x");
        clear(cx, document);
        let names = StyleNames {
            attributes: [names::intern("title")].into_iter().collect(),
            classes: [names::intern("a")].into_iter().collect(),
            ids: ["up".into()].into_iter().collect(),
            states: AttributeChanges::HOVER,
            everything: false,
        };
        cx.set_style_names(document, names);
        #[cfg(debug_assertions)]
        drop(cx.take_passed_over_changes(document));
        let noted = |set: &dyn Fn()| {
            set();
            let noted = cx.take_attribute_changes(div).is_some();
            assert_eq!(
                noted,
                cx.style_flags(div).contains(S::ATTRIBUTES_CHANGED),
                "marked as noted"
            );
            assert_eq!(
                noted,
                cx.style_flags(body).contains(S::CHILD_NEEDS_STYLE_RECALC)
            );
            clear(cx, document);
            noted
        };
        assert!(
            noted(&|| cx.set_attribute(div, "title", "t")),
            "an attribute by its name"
        );
        assert!(!noted(&|| cx.set_attribute(div, "lang", "en")));
        assert!(
            !noted(&|| cx.set_attribute(div, "class", "x y")),
            "no class style reads"
        );
        assert!(
            noted(&|| cx.set_attribute(div, "class", "x y A")),
            "one that came, in any case"
        );
        assert!(
            !noted(&|| cx.set_attribute(div, "class", "A x")),
            "only moved"
        );
        assert!(
            noted(&|| cx.set_attribute(div, "class", "x")),
            "one that went"
        );
        assert!(noted(&|| cx.set_attribute(div, "id", "UP")), "a new id");
        assert!(noted(&|| cx.set_attribute(div, "id", "down")), "an old id");
        assert!(!noted(&|| cx.set_attribute(div, "id", "side")));
        assert!(noted(&|| cx.set_hover_element(document, Some(div))));
        assert!(!noted(&|| cx.set_active_element(document, Some(div))));
        #[cfg(debug_assertions)]
        {
            let passed = cx.take_passed_over_changes(document);
            let id = cx.node_id(div);
            assert!(
                passed.iter().any(|(element, changes)| *element == id
                    && changes.attributes == [names::local::LANG])
            );
            assert!(
                passed
                    .iter()
                    .any(|(_, changes)| changes.states == AttributeChanges::ACTIVE)
            );
            assert!(cx.take_passed_over_changes(document).is_empty(), "taken");
        }
        // A document never styled: nothing noted, nothing kept to check.
        let other = cx.create_document(true);
        let root = cx.create_element(other, QualName::html("html"));
        cx.append_child(other, root).unwrap();
        clear(cx, other);
        cx.set_attribute(root, "class", "a");
        assert!(cx.take_attribute_changes(root).is_none());
        assert_eq!(cx.style_flags(root), S::CLEAN);
        #[cfg(debug_assertions)]
        assert!(cx.take_passed_over_changes(other).is_empty());
    });
}

#[test]
fn text_changes_mark_the_parent_and_the_texts_boxes() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, _, body) = page(cx);
        let text = cx.create_text(document, "one");
        cx.append_child(body, text).unwrap();
        clear(cx, document);
        cx.set_data(text, "two");
        assert!(cx.style_flags(body).contains(S::CHILDREN_CHANGED));
        assert_eq!(cx.layout_tree_flags(text), L::NEEDS_REATTACH);
        clear(cx, document);
        cx.append_data(text, " three");
        assert!(cx.style_flags(body).contains(S::CHILDREN_CHANGED));
        // A comment's data is no one's style.
        let comment = cx.create_comment(document, "c");
        cx.append_child(body, comment).unwrap();
        clear(cx, document);
        cx.set_data(comment, "d");
        assert_eq!(cx.style_flags(body), S::CLEAN);
    });
}

#[test]
fn stylesheets_coming_going_and_changing_mark_the_document() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, html, body) = page(cx);
        let sheets = || cx.style_flags(document).contains(S::STYLE_SHEETS_CHANGED);
        // Inside an element inserted: the walk finds it.
        let div = cx.create_element(document, QualName::html("div"));
        let style = cx.create_element(document, QualName::html("style"));
        cx.append_child(div, style).unwrap();
        cx.append_child(body, div).unwrap();
        assert!(sheets());
        clear(cx, document);
        let text = cx.create_text(document, "p {}");
        cx.append_child(style, text).unwrap();
        assert!(sheets(), "text into a <style>");
        clear(cx, document);
        cx.set_data(text, "div {}");
        assert!(sheets(), "a <style>'s text changed");
        clear(cx, document);
        let link = cx.create_element(document, QualName::html("link"));
        cx.append_child(html, link).unwrap();
        clear(cx, document);
        cx.set_attribute(link, "href", "a.css");
        assert!(sheets(), "a <link>'s attribute");
        clear(cx, document);
        cx.remove(div);
        assert!(sheets(), "a <style> went");
        clear(cx, document);
        // Another element: no sheets.
        let p = cx.create_element(document, QualName::html("p"));
        cx.append_child(body, p).unwrap();
        cx.set_attribute(p, "class", "x");
        assert!(!sheets());
    });
}

#[test]
fn every_change_counts_in_the_trees_version() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let div = cx.create_element(document, QualName::html("div"));
        let text = cx.create_text(document, "t");
        let version = cx.tree_version();
        // Apart from any document too.
        cx.append_child(div, text).unwrap();
        cx.set_attribute(div, "id", "a");
        cx.set_data(text, "u");
        cx.remove(text);
        assert_eq!(cx.tree_version(), version + 4);
    });
}

#[test]
fn a_node_gets_its_id_as_it_is_asked_for() {
    let mut dom = Dom::new();
    let (id, other, kept) = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let text = cx.create_text(document, "t");
        let id = cx.node_id(document);
        assert_eq!(cx.node_id(document), id, "the same each time");
        let other = cx.node_id(text);
        assert_ne!(id, other);
        assert!(
            cx.node_by_id(id)
                .is_some_and(|node| ::elise::Gc::ptr_eq(node, document))
        );
        (id, other, cx.root(document))
    });
    dom.collect();
    dom.read(|cx| {
        assert!(cx.node_by_id(id).is_some());
        assert!(cx.node_by_id(other).is_none(), "its node died");
    });
    drop(kept);
}

#[test]
fn an_elements_style_is_read_with_the_token_and_written_with_it() {
    let mut token = LocalToken::new();
    let mut dom = Dom::new();
    let computed = Arc::new(ComputedStyle::initial());
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let div = cx.create_element(document, QualName::html("div"));
        assert!(cx.element_style(div, &token).is_none(), "never styled");
        assert!(cx.computed_style(document, &token).is_none());
        cx.element_style_mut(div, &mut token).computed = Some(computed.clone());
        let read = cx.computed_style(div, &token).unwrap();
        assert!(Arc::ptr_eq(read, &computed), "read where it is, no clone");
        assert_eq!(Arc::strong_count(&computed), 2);
        // Selector flags: an element's, only added to; another node's are
        // every bit.
        assert_eq!(cx.selector_flags(div), 0);
        cx.add_selector_flags(div, 0b10);
        cx.add_selector_flags(div, 0b1);
        assert_eq!(cx.selector_flags(div), 0b11);
        assert_eq!(cx.selector_flags(document), u16::MAX);
    });
}

/// An element's classes: numbered as written and ASCII lowercase, each
/// once, in order, with their bloom hashes; made again as `class` changes,
/// on an element apart from any document too.
#[test]
fn an_elements_classes_are_numbers() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let div = cx.create_element(document, QualName::html("div"));
        assert!(cx.class_names(div).is_empty());
        cx.set_attribute(div, "class", " Foo bar  Foo\tbaz ");
        let classes = cx.class_names(div);
        let text: Vec<String> = classes
            .iter()
            .map(|c| names::get(c.name).to_string())
            .collect();
        assert_eq!(text, ["Foo", "bar", "baz"]);
        assert_eq!(classes[0].lower, names::find("foo").unwrap());
        assert_eq!(classes[1].lower, classes[1].name);
        assert_eq!(classes[0].hash, css::selector::name_hash("Foo"));
        cx.set_attribute(div, "class", "x");
        assert_eq!(cx.class_names(div).len(), 1);
        cx.remove_attribute(div, "class");
        assert!(cx.class_names(div).is_empty());
        // Another attribute leaves them.
        cx.set_attribute(div, "class", "y");
        cx.set_attribute(div, "title", "t");
        assert_eq!(cx.class_names(div).len(), 1);
    });
}

/// The `:hover` and `:active` chains: the hover element and its ancestors
/// have the state, the change is noted where the chains differ (the trees'
/// version too: a frame is asked for); taking the bottom out moves it to
/// the parent and clears the subtree's.
#[test]
fn hover_and_active_chains() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, html, body) = page(cx);
        let div = cx.create_element(document, QualName::html("div"));
        let span = cx.create_element(document, QualName::html("span"));
        let p = cx.create_element(document, QualName::html("p"));
        cx.append_child(body, div).unwrap();
        cx.append_child(div, span).unwrap();
        cx.append_child(body, p).unwrap();
        clear(cx, document);
        let version = cx.tree_version();
        cx.set_hover_element(document, Some(span));
        assert!([html, body, div, span].iter().all(|&e| cx.is_hovered(e)));
        assert!(!cx.is_hovered(p) && !cx.is_active(span));
        assert!(cx.tree_version() > version);
        let changes = cx.take_attribute_changes(span).expect("noted");
        assert_eq!(changes.states, dom::AttributeChanges::HOVER);
        assert!(cx.style_flags(span).contains(S::ATTRIBUTES_CHANGED));
        clear(cx, document);
        // From the span to the paragraph: the body and the root keep it.
        cx.set_hover_element(document, Some(p));
        assert!(
            !cx.is_hovered(span) && !cx.is_hovered(div) && cx.is_hovered(p) && cx.is_hovered(body)
        );
        assert!(cx.take_attribute_changes(body).is_none(), "in both chains");
        assert!(
            cx.take_attribute_changes(div)
                .is_some_and(|c| c.states == dom::AttributeChanges::HOVER)
        );
        cx.set_active_element(document, Some(span));
        assert!(cx.is_active(span) && cx.is_active(div) && cx.is_active(html) && !cx.is_active(p));
        // The active element's parent taken out: the chain ends at the body.
        cx.remove(div);
        assert!(!cx.is_active(div) && !cx.is_active(span) && cx.is_active(body));
        assert!(
            cx.active_element(document)
                .is_some_and(|a| elise::Gc::ptr_eq(a, body))
        );
        assert!(
            cx.hover_element(document)
                .is_some_and(|h| elise::Gc::ptr_eq(h, p)),
            "another chain's"
        );
        // Nothing: no state left.
        cx.set_active_element(document, None);
        cx.set_hover_element(document, None);
        assert!(
            [html, body, p]
                .iter()
                .all(|&e| !cx.is_hovered(e) && !cx.is_active(e))
        );
    });
}

/// The focus chain: the bits while the page has the system's focus (the
/// chain's whatever), the change noted, the page's focus moving them, the
/// focused element taken out clearing the chain above.
#[test]
fn the_focus_chain() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, html, body) = page(cx);
        let window = cx.create_window(document);
        let div = cx.create_element(document, QualName::html("div"));
        let span = cx.create_element(document, QualName::html("span"));
        cx.append_child(body, div).unwrap();
        cx.append_child(div, span).unwrap();
        clear(cx, document);
        cx.set_focused_element(document, Some(span), true);
        assert!(
            !cx.is_focused(span) && !cx.has_focus_within(div),
            "no system focus"
        );
        cx.set_page_focused(window, true);
        assert!(cx.is_focused(span) && cx.is_focus_visible(span) && !cx.is_focused(div));
        assert!(
            [html, body, div, span]
                .iter()
                .all(|&e| cx.has_focus_within(e))
        );
        let changes = cx.take_attribute_changes(span).expect("noted");
        assert_eq!(
            changes.states,
            dom::AttributeChanges::FOCUS
                | dom::AttributeChanges::FOCUS_WITHIN
                | dom::AttributeChanges::FOCUS_VISIBLE
        );
        assert_eq!(
            cx.take_attribute_changes(div).expect("noted").states,
            dom::AttributeChanges::FOCUS_WITHIN
        );
        clear(cx, document);
        // To the div without a ring: the body and the root keep theirs.
        cx.set_focused_element(document, Some(div), false);
        assert!(cx.is_focused(div) && !cx.is_focus_visible(div) && !cx.has_focus_within(span));
        assert!(cx.take_attribute_changes(body).is_none(), "in both chains");
        // Taken out: no focused element, the chain above clear.
        cx.remove(div);
        assert!(cx.focused_element(document).is_none());
        assert!(!cx.has_focus_within(body) && !cx.has_focus_within(html) && !cx.is_focused(div));
        assert!(
            cx.take_attribute_changes(body)
                .is_some_and(|c| c.states == dom::AttributeChanges::FOCUS_WITHIN)
        );
        // A document's own bits there aren't an element's.
        assert!(cx.is_html_document(document));
    });
}

/// A document's and a shadow root's adopted sheets: set at an index or
/// added at the end, the last taken off, each change marking the scope's
/// sheets as changed (the document's flag, the shadow root's list); a
/// sheet's contents replaced or its `disabled` changed put it in the list
/// style takes, once, and nothing else.
#[test]
fn adopted_sheets_and_their_changes() {
    use css::StyleSheetContents;
    use std::sync::Arc;
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (document, _, body) = page(cx);
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(body, host).unwrap();
        let root = cx.attach_shadow(
            host,
            dom::ShadowRootInit {
                mode: dom::ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: dom::SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        clear(cx, document);
        drop(cx.take_sheet_changed_roots(document));
        let contents = || Arc::new(StyleSheetContents::parse("p {}", None));
        let a = cx.create_style_sheet(document, contents(), "print", false);
        let b = cx.create_style_sheet(document, contents(), "", true);
        assert_ne!(cx.style_sheet_id(a), cx.style_sheet_id(b));
        assert_eq!(cx.style_sheet_media(a).0, "print");
        assert!(!cx.style_sheet_disabled(a) && cx.style_sheet_disabled(b));
        assert!(cx.adopted_style_sheets(document).is_empty());
        cx.set_adopted_style_sheet(document, 0, a);
        cx.set_adopted_style_sheet(document, 1, b);
        cx.set_adopted_style_sheet(document, 0, b);
        let ids = |list: Vec<_>| {
            list.into_iter()
                .map(|sheet| cx.style_sheet_id(sheet))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(cx.adopted_style_sheets(document)),
            [cx.style_sheet_id(b); 2]
        );
        assert!(cx.style_flags(document).contains(S::STYLE_SHEETS_CHANGED));
        cx.remove_last_adopted_style_sheet(document);
        assert_eq!(cx.adopted_style_sheets(document).len(), 1);
        // A shadow root's: its list, its host marked.
        cx.set_adopted_style_sheet(root, 0, a);
        assert_eq!(cx.take_sheet_changed_roots(document).len(), 1);
        assert!(cx.style_flags(host).contains(S::STYLE_SHEETS_CHANGED));
        assert_eq!(
            cx.adopted_style_sheets(document).len(),
            1,
            "each scope its own"
        );
        // A sheet's changes: listed once, the scopes not marked.
        clear(cx, document);
        assert!(cx.take_changed_style_sheets(document).is_empty());
        cx.replace_style_sheet_contents(b, contents());
        assert_eq!(
            ids(cx.take_changed_style_sheets(document)),
            [cx.style_sheet_id(b)],
            "replaced"
        );
        cx.replace_style_sheet_contents(b, contents());
        assert_eq!(
            cx.take_changed_style_sheets(document).len(),
            1,
            "again, once taken"
        );
        cx.replace_style_sheet_contents(a, contents());
        cx.set_style_sheet_disabled(a, true);
        cx.set_style_sheet_disabled(b, true);
        let changed = cx.take_changed_style_sheets(document);
        assert_eq!(
            ids(changed),
            [cx.style_sheet_id(a)],
            "once; `b` was disabled"
        );
        assert!(cx.take_changed_style_sheets(document).is_empty(), "taken");
        assert_eq!(cx.style_flags(document), S::CLEAN);
    });
}
