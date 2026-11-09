//! Blink's `HTMLDocumentParserFastPath` on the DOM: `fast_path`'s
//! scan (the markup `innerHTML` and the like are mostly given, which the
//! tree builder would nest as written), then the nodes made from it in the
//! operation, with no tokenizer state machine and no tree builder. Anything
//! the scan doesn't know makes it give up before any node is made, and the
//! fragment parsing algorithm parses the input instead.

use dom::{Cx, NodeRef, QualName, QuirksMode, names};

use crate::fast_path::{Event, Scan, count_lines, is_context, is_void, scan};

/// Parses `input` into `fragment` (an empty fragment of `document`), as
/// children of `context` would be, if the fast path can; `false`, having
/// made nothing, if it can't.
pub(crate) fn parse_fragment<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    fragment: NodeRef<'gc>,
    context: NodeRef<'gc>,
    input: &str,
) -> bool {
    let Some(name) = cx.element_name(context) else {
        return false;
    };
    if name.namespace != names::HTML_NAMESPACE || !is_context(&names::get(name.local)) {
        return false;
    }
    let Some(scanned) = scan(input) else {
        return false;
    };
    #[cfg(debug_assertions)]
    crate::fast_path::note_fast_parse();
    build(cx, document, fragment, scanned);
    true
}

/// Parses `input` as a whole document into `document` (a new one, no
/// browsing context: `DOMParser`'s, `parseHTMLUnsafe`'s), if the fast path
/// can; `false`, having made nothing, if it can't. With no DOCTYPE the
/// tree builder puts the document in quirks mode, skips the white space
/// before anything else, and makes `<html>`, an empty `<head>` and
/// `<body>`, which what the fast path takes all goes in.
pub(crate) fn parse_document<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, input: &str) -> bool {
    let start = input.trim_start_matches([' ', '\t', '\n', '\x0C']);
    let Some(mut scanned) = scan(start) else {
        return false;
    };
    // The lines skipped still count (saturating, as `count_lines` does).
    let skipped = count_lines(input.strip_suffix(start).unwrap_or_default());
    for event in &mut scanned.events {
        if let Event::Start { line, .. } = event {
            *line = line.saturating_add(skipped);
        }
    }
    cx.set_quirks_mode(document, QuirksMode::Quirks);
    let html = cx.create_element(document, QualName::html("html"));
    cx.insert(document, html, None);
    let head = cx.create_element(document, QualName::html("head"));
    cx.insert(html, head, None);
    let body = cx.create_element(document, QualName::html("body"));
    cx.insert(html, body, None);
    #[cfg(debug_assertions)]
    crate::fast_path::note_fast_parse();
    build(cx, document, body, scanned);
    true
}

/// Makes the nodes of `scanned` under `root`, each element with its
/// attributes (its event handlers' line its tag's), then into the tree.
/// The names are the known ones and lowercase, each attribute's once in
/// its tag (the scan's): the element is made with its attributes by
/// number.
fn build<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, root: NodeRef<'gc>, scanned: Scan<'_>) {
    let mut stack = vec![root];
    let mut attributes = Vec::new();
    for event in scanned.events {
        let parent = *stack.last().expect("the root stays");
        match event {
            Event::Start { name, attrs, line } => {
                attributes.clear();
                attributes.extend(
                    scanned.attrs[attrs]
                        .iter()
                        .map(|(attribute, value)| (names::intern(attribute), &**value)),
                );
                // Where its event handlers' sources are: its tag's `>`.
                cx.set_parser_line(line);
                let element = cx.create_html_element_with_attributes(
                    document,
                    names::intern(name),
                    &attributes,
                );
                cx.set_parser_line(0);
                cx.insert(parent, element, None);
                if !is_void(name) {
                    stack.push(element);
                }
            }
            Event::End => {
                stack.pop();
            }
            Event::Text(text) => {
                let node = cx.create_text(document, &text);
                cx.insert(parent, node, None);
            }
        }
    }
}

/// Whether two subtrees are the same: the same kinds of nodes, the same
/// element names and attributes in order, inputs of the same type and
/// checkedness (the fast path's are made of theirs), the same text. Debug
/// builds check the fast path's against the fragment parsing algorithm's
/// with it.
#[cfg(debug_assertions)]
pub(crate) fn same_tree<'gc>(cx: &Cx<'_, 'gc>, a: NodeRef<'gc>, b: NodeRef<'gc>) -> bool {
    use dom::NodeKind;

    if cx.kind(a) != cx.kind(b) {
        return false;
    }
    let same_self = match cx.kind(a) {
        NodeKind::Element => {
            cx.element_name(a) == cx.element_name(b)
                && cx.attribute_count(a) == cx.attribute_count(b)
                && (0..cx.attribute_count(a))
                    .all(|index| cx.attribute_at(a, index) == cx.attribute_at(b, index))
                && cx.input_kind(a) == cx.input_kind(b)
                && cx.checkedness(a) == cx.checkedness(b)
        }
        NodeKind::Text | NodeKind::Comment => cx.data(a) == cx.data(b),
        _ => true,
    };
    if !same_self {
        return false;
    }
    let (mut x, mut y) = (cx.first_child(a), cx.first_child(b));
    loop {
        match (x, y) {
            (None, None) => return true,
            (Some(c), Some(d)) => {
                if !same_tree(cx, c, d) {
                    return false;
                }
                x = cx.next_sibling(c);
                y = cx.next_sibling(d);
            }
            _ => return false,
        }
    }
}
