//! Where a caret can first be in an editing host, as laid out: the first
//! of Blink's visually equivalent candidates (`IsVisuallyEquivalentCandidate`)
//! going forward from its start (`FirstPositionInOrBeforeNode`, made
//! canonical by `CanonicalPositionOf`), what `Element::UpdateSelectionOnFocus`
//! collapses the selection at. A candidate is, in tree order: before the
//! first character of a visible text that didn't collapse away (or a line
//! break its style keeps), before a `<br>`, before an element whose content
//! a caret can't be in (`EditingIgnoresContent`) or a table, or at the
//! start of a box with height and no candidate in it. Something not
//! editable ends the search at the start of its parent (Chrome passes the
//! empty inline elements before it back to there), as the candidate must
//! stay in the host's editable content.

use computed_style::{Display, TableDisplay};
use dom::{Cx, NodeKind, NodeRef, names};

use crate::{BoxFragment, BoxTree, NodeIdHashMap, NodeIdHashSet};

/// A text of the host as laid out: its first character on a line, and
/// whether it is visible and keeps its line breaks.
struct LaidOutText {
    first: Option<u32>,
    visible: bool,
    keeps_newlines: bool,
}

/// An element of the host with a box: the height of its block box (0 for
/// an inline box), whether it is a table (Blink's `IsDisplayInsideTable`:
/// a caret goes before it, not into its cells), and whether its visible
/// style keeps line breaks (what a text of only line breaks in it, which
/// lays out no run, shows).
#[derive(Default)]
struct LaidOutElement {
    height: f32,
    table: bool,
    keeps_newlines: bool,
}

/// What the fragments say of the host's nodes.
#[derive(Default)]
struct LaidOut {
    texts: NodeIdHashMap<LaidOutText>,
    elements: NodeIdHashMap<LaidOutElement>,
}

impl LaidOut {
    fn find(layout_view: &BoxFragment, wanted: &NodeIdHashSet) -> Self {
        let mut out = Self::default();
        out.add(layout_view, wanted);
        out
    }

    fn add(&mut self, fragment: &BoxFragment, wanted: &NodeIdHashSet) {
        if let Some(node) = fragment.node
            && wanted.contains(&node)
            && fragment.pseudo.is_none()
        {
            let element = self.elements.entry(node).or_default();
            element.height = element.height.max(fragment.size.height);
            element.table |= matches!(
                fragment.style.display,
                Display::Table(TableDisplay::Table) | Display::InlineTable
            );
            element.keeps_newlines |= keeps_newlines(&fragment.style);
        }
        for line in &fragment.lines {
            for inline in &line.boxes {
                if wanted.contains(&inline.node) && inline.pseudo.is_none() {
                    self.elements.entry(inline.node).or_default().keeps_newlines |=
                        keeps_newlines(&inline.style);
                }
            }
            for run in &line.runs {
                let Some((node, _)) = &run.source else {
                    continue;
                };
                if !wanted.contains(node) {
                    continue;
                }
                let first = run.first_cluster_start();
                let text = self.texts.entry(*node).or_insert(LaidOutText {
                    first: None,
                    visible: false,
                    keeps_newlines: false,
                });
                text.first = match (text.first, first) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                text.visible |= run.style.is_visible();
                text.keeps_newlines |= run.style.white_space_collapse.preserves_newlines();
            }
        }
        for child in &fragment.children {
            self.add(&child.fragment, wanted);
        }
    }
}

/// The first place a caret can be in `host`, laid out as `layout_view`
/// (`box_tree` its boxes): a text and an offset in it, or a parent and the
/// index of a child it is before (its children's count: after them); the
/// host's start if there is none.
pub fn first_caret_position<'gc>(
    cx: &Cx<'_, 'gc>,
    box_tree: &BoxTree,
    layout_view: &BoxFragment,
    host: NodeRef<'gc>,
) -> (NodeRef<'gc>, u32) {
    let mut wanted = NodeIdHashSet::default();
    let mut stack = vec![host];
    while let Some(node) = stack.pop() {
        wanted.insert(cx.node_id(node));
        stack.extend(std::iter::successors(cx.first_child(node), |child| {
            cx.next_sibling(*child)
        }));
    }
    let laid_out = LaidOut::find(layout_view, &wanted);
    let search = Search {
        cx,
        box_tree,
        laid_out: &laid_out,
    };
    search.within(host).unwrap_or((host, 0))
}

struct Search<'a, 'b, 'gc> {
    cx: &'a Cx<'b, 'gc>,
    box_tree: &'a BoxTree,
    laid_out: &'a LaidOut,
}

impl<'gc> Search<'_, '_, 'gc> {
    /// The first candidate in `parent`'s content, in tree order.
    fn within(&self, parent: NodeRef<'gc>) -> Option<(NodeRef<'gc>, u32)> {
        let cx = self.cx;
        let mut index = 0u32;
        let mut child = cx.first_child(parent);
        while let Some(node) = child {
            if let Some(found) = self.at(parent, index, node) {
                return Some(found);
            }
            child = cx.next_sibling(node);
            index = index.saturating_add(1);
        }
        None
    }

    /// The first candidate at `node`, `parent`'s child at `index`, or in it.
    fn at(
        &self,
        parent: NodeRef<'gc>,
        index: u32,
        node: NodeRef<'gc>,
    ) -> Option<(NodeRef<'gc>, u32)> {
        let cx = self.cx;
        let id = cx.node_id(node);
        match cx.kind(node) {
            NodeKind::Text => {
                let Some(text) = self.laid_out.texts.get(&id) else {
                    // Only line breaks its parent's style keeps: a line of
                    // its own, though no run.
                    let parent = self.laid_out.elements.get(&cx.node_id(parent))?;
                    return parent
                        .keeps_newlines
                        .then(|| first_newline(cx, node))
                        .flatten()
                        .map(|first| (node, first));
                };
                let text = Some(text).filter(|text| text.visible)?;
                // A line break the style keeps is a character laid out
                // (before the first run's clusters, on a line of its own).
                let newline = text
                    .keeps_newlines
                    .then(|| first_newline(cx, node))
                    .flatten();
                let first = match (text.first, newline) {
                    (Some(a), Some(b)) => a.min(b),
                    (a, b) => a.or(b)?,
                };
                Some((node, first))
            }
            NodeKind::Element => {
                let is_br = cx.qualified_name(node).is_some_and(|name| {
                    name.namespace() == names::HTML_NAMESPACE && name.local() == names::local::BR
                });
                if is_br {
                    return self.box_tree.is_rendered(id).then_some((parent, index));
                }
                let element = self.laid_out.elements.get(&id)?;
                if !cx.is_editable(node) {
                    return Some((parent, 0));
                }
                if cx.editing_ignores_content(node) || element.table {
                    return Some((parent, index));
                }
                if let Some(found) = self.within(node) {
                    return Some(found);
                }
                // A box with height and nothing laid out in it (Blink's
                // `HasRenderedNonAnonymousDescendantsWithHeight`).
                (element.height > 0.0).then_some((node, 0))
            }
            _ => None,
        }
    }
}

/// Whether `style` is visible and keeps line breaks.
fn keeps_newlines(style: &computed_style::ComputedStyle) -> bool {
    style.is_visible() && style.white_space_collapse.preserves_newlines()
}

/// The offset of `text`'s first line feed, if it has one.
fn first_newline<'gc>(cx: &Cx<'_, 'gc>, text: NodeRef<'gc>) -> Option<u32> {
    let data = cx.text_data(text)?;
    let data = data.borrow();
    let data = data.to_str();
    let byte = data.find('\n')?;
    Some(u32::try_from(data[..byte].encode_utf16().count()).unwrap_or(u32::MAX))
}
