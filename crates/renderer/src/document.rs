//! Reading back the parts of a document the browser and the web tests
//! need.

/// Whether the document element's `class` has `class` (web tests'
/// `reftest-wait`).
pub(crate) fn root_has_class<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    document: dom::NodeRef<'gc>,
    class: &str,
) -> bool {
    std::iter::successors(cx.first_child(document), |&child| cx.next_sibling(child))
        .find(|&child| cx.kind(child) == dom::NodeKind::Element)
        .and_then(|root| cx.get_attribute(root, "class"))
        .is_some_and(|classes| {
            classes
                .to_str()
                .split_ascii_whitespace()
                .any(|c| c == class)
        })
}
