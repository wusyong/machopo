//! What the unit tests share: a page parsed into a `dom::Dom`, styled
//! and built into a box tree, as `FrameView` does before layout, and
//! its elements by their ids.

use std::collections::HashMap;

use dom::{Cx, Dom, LocalToken, NodeId, NodeRef};
use layout::{BoxTree, TextContext};

/// `html`'s box tree, before layout, and the elements of each `id`.
pub(crate) fn box_tree(html: &str) -> (BoxTree, HashMap<String, NodeId>) {
    let mut dom = Dom::new();
    let document = html::parser::parse_document(&mut dom, html, true)
        .root()
        .clone();
    let mut token = LocalToken::new();
    let mut engine = style::StyleEngine::new();
    dom.mutate(|cx| engine.recalc_style(cx, &mut token, document.get(cx.gc)));
    dom.read(|cx| {
        let document = document.get(cx.gc);
        let mut ids = HashMap::new();
        collect_ids(cx, document, &mut ids);
        let tree = BoxTree::build(cx, &token, document, &mut TextContext::new(true));
        (tree, ids)
    })
}

fn collect_ids<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, ids: &mut HashMap<String, NodeId>) {
    let mut child = cx.first_child(node);
    while let Some(at) = child {
        if let Some(id) = cx.get_attribute(at, "id") {
            ids.insert(id.to_string(), cx.node_id(at));
        }
        collect_ids(cx, at, ids);
        child = cx.next_sibling(at);
    }
}
