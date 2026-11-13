//! The Selection API's `Selection` (https://w3c.github.io/selection-api/;
//! Blink's `DOMSelection`): a document's selection, `getSelection()`'s, at
//! most one live range and its direction. `dom` keeps them; a method
//! that changes the selection makes a new range (the old one, script's, is
//! no longer the selection's), while script setting the selection's range
//! changes the selection (`range_set`, Blink's `Range::
//! UpdateSelectionIfAddedToSelection`). The checks and the directions are
//! Chrome's where it differs from the specification: a collapsed
//! selection has no direction, and `addRange` or script setting its range
//! makes it forward. A change script can see (its anchor, its focus, its
//! direction) schedules `selectionchange` at the document; setting it as
//! it was doesn't.
//!
//! A method that changes the selection, putting it in an editing host
//! that doesn't have the focus, gives it the focus (Blink's
//! `FrameSelection::SetFocusedNodeIfNeeded`): the method's operation hands
//! the host on (`FocusHost`), and its focusing steps, which dispatch the
//! focus events, run out of it (`#[then_script]`); a call that hands
//! nothing on is the one operation. An editing host taking the focus
//! otherwise takes the selection to its start (`focused`).

use std::cmp::Ordering;

use dom::{BoundaryPoint, NodeKind, NodeRef, NodeRoot, Range, Selection, Window};
use elise::Gc;

use crate::exception::ExceptionName;
use crate::typed::{DomStr, Root, ScriptHost, ThenScript};
use crate::{ApiCx, ApiError};

/// `document.getSelection()`: its selection, made the first time; null for
/// a document without a browsing context (no window, as
/// `createHTMLDocument`'s, or discarded).
pub(crate) fn get_selection<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<Gc<'gc, Selection>>, ApiError> {
    if cx.window_of(this).is_none() || cx.is_discarded(this) {
        return Ok(None);
    }
    Ok(Some(cx.document_selection(this)))
}

/// `window.getSelection()`: its document's.
pub(crate) fn window_get_selection<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Option<Gc<'gc, Selection>>, ApiError> {
    get_selection(cx, cx.window_document(this))
}

/// Whether `node` is in the selection's document (in a shadow tree of it
/// too: the specification's "document is a shadow-including inclusive
/// ancestor of node").
fn in_document<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Selection>, node: NodeRef<'gc>) -> bool {
    Gc::ptr_eq(cx.root_node(node, true), cx.selection_document(this))
}

/// Its range as script sees it: none while a boundary point isn't in its
/// document, as the specification's `rangeCount` and `anchorNode` have
/// it (removing a shadow host leaves a range in its shadow tree there:
/// the tree's changes move points only out of a removed node's own tree).
fn range_of<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Selection>) -> Option<Gc<'gc, Range>> {
    let range = cx.selection_range(this)?;
    (in_document(cx, this, cx.range_start(range).0) && in_document(cx, this, cx.range_end(range).0))
        .then_some(range)
}

/// Its anchor and focus: its range's start and end, the other way round if
/// it is backward; none while it is empty.
fn anchor_and_focus<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Option<(BoundaryPoint<'gc>, BoundaryPoint<'gc>)> {
    let range = range_of(cx, this)?;
    let (start, end) = (cx.range_start(range), cx.range_end(range));
    Some(if cx.selection_is_backward(this) {
        (end, start)
    } else {
        (start, end)
    })
}

/// What script sees of a selection: its anchor and its focus (which
/// change places when its direction does); none while it is empty.
pub(crate) type Seen<'gc> = Option<(BoundaryPoint<'gc>, BoundaryPoint<'gc>)>;

fn same<'gc>(a: Seen<'gc>, b: Seen<'gc>) -> bool {
    let point = |a: BoundaryPoint<'gc>, b: BoundaryPoint<'gc>| Gc::ptr_eq(a.0, b.0) && a.1 == b.1;
    match (a, b) {
        (None, None) => true,
        (Some((a_anchor, a_focus)), Some((b_anchor, b_focus))) => {
            point(a_anchor, b_anchor) && point(a_focus, b_focus)
        }
        _ => false,
    }
}

pub(crate) fn seen<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Selection>) -> Seen<'gc> {
    anchor_and_focus(cx, this)
}

/// `selectionchange` scheduled at its document if what script sees of it
/// isn't what it saw `before`.
fn changed_since<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Selection>, before: Seen<'gc>) {
    if !same(seen(cx, this), before) {
        cx.schedule_selectionchange(cx.selection_document(this));
    }
}

/// Its range and direction set, `selectionchange` scheduled if script
/// sees it change; whether it did.
fn set<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    range: Option<Gc<'gc, Range>>,
    backward: bool,
) -> bool {
    let before = seen(cx, this);
    cx.set_selection(this, range, backward);
    let changed = !same(seen(cx, this), before);
    if changed {
        cx.schedule_selectionchange(cx.selection_document(this));
    }
    changed
}

/// A new live range from `start` to `end` becomes its; the editing host
/// to give the focus to if that changed it (`focus_host`).
fn select<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    start: BoundaryPoint<'gc>,
    end: BoundaryPoint<'gc>,
    backward: bool,
) -> Option<FocusHost<'gc>> {
    let range = cx.create_range(start, end);
    set(cx, this, Some(range), backward)
        .then(|| focus_host(cx, this))
        .flatten()
}

/// An editing host the Selection API put its document's selection in, to
/// take the focus out of the method's operation.
pub(crate) struct FocusHost<'gc> {
    document: NodeRef<'gc>,
    host: NodeRef<'gc>,
}

pub(crate) struct FocusHostRoot {
    document: NodeRoot,
    host: NodeRoot,
}

impl<'gc> Root<'gc> for FocusHost<'gc> {
    type Rooted = FocusHostRoot;

    fn root(self, cx: &ApiCx<'_, 'gc>) -> FocusHostRoot {
        FocusHostRoot {
            document: cx.root(self.document),
            host: cx.root(self.host),
        }
    }
}

impl ThenScript for FocusHostRoot {
    fn run<H: ScriptHost>(self, host: &mut H) {
        crate::focus::focus_selected_editing_host(host, &self.document, &self.host);
    }
}

/// The editing host the changed selection is in, to give the focus to
/// (Blink's `FrameSelection::SetFocusedNodeIfNeeded`, after the selection
/// is kept within its anchor's editing host): its anchor's host, or else,
/// if it is backward, its focus's (Chrome 152); none if it has the focus
/// already, or its document's frame isn't the focused frame of its tree.
/// Whether it can take the focus is the focusing's (it lays out).
fn focus_host<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Selection>) -> Option<FocusHost<'gc>> {
    let (anchor, focus) = anchor_and_focus(cx, this)?;
    let host = cx.editing_host_of(anchor.0).or_else(|| {
        cx.selection_is_backward(this)
            .then(|| cx.editing_host_of(focus.0))
            .flatten()
    })?;
    let document = cx.selection_document(this);
    let focused_frame = Gc::ptr_eq(cx.focused_frame(cx.top_document(document)), document);
    let has_focus = cx
        .focused_element(document)
        .is_some_and(|focused| Gc::ptr_eq(focused, host));
    (focused_frame && !has_focus).then_some(FocusHost { document, host })
}

/// `element` took the focus: if it is an editing host and its document's
/// selection isn't in it, the selection collapses where a caret first
/// can be in it, as laid out (Blink's `Element::UpdateSelectionOnFocus`,
/// `layout::first_caret_position`), before its `focus` listeners run. A
/// document without a window has no selection.
pub(crate) fn focused<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) {
    if !cx.is_editing_host(element) {
        return;
    }
    let document = cx.document_of(element);
    if cx.window_of(document).is_none() {
        return;
    }
    let this = cx.document_selection(document);
    let inside = range_of(cx, this).is_some_and(|range| {
        [cx.range_start(range).0, cx.range_end(range).0]
            .into_iter()
            .all(|point| cx.is_inclusive_ancestor(element, point))
    });
    if inside {
        return;
    }
    // The focusing steps laid the document out.
    let point = cx
        .with_layout(element, |layout| layout.first_caret_position(cx, element))
        .unwrap_or((element, 0));
    let range = cx.create_range(point, point);
    set(cx, this, Some(range), false);
}

/// After script set `range` (its members, on a selection's range): the
/// selection follows it, forward (Chrome's); if a boundary point left the
/// selection's document, the selection is empty, and stays so if the
/// range comes back. `before` is what script saw of the selection then.
pub(crate) fn range_set<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    range: Gc<'gc, Range>,
    before: Seen<'gc>,
) {
    if in_document(cx, this, cx.range_start(range).0)
        && in_document(cx, this, cx.range_end(range).0)
    {
        cx.set_selection(this, Some(range), false);
    } else {
        cx.set_selection(this, None, false);
    }
    changed_since(cx, this, before);
}

pub(crate) fn anchor_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(anchor_and_focus(cx, this).map(|(anchor, _)| anchor.0))
}

/// `anchorOffset`: 0 while it is empty.
pub(crate) fn anchor_offset<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<u32, ApiError> {
    Ok(anchor_and_focus(cx, this).map_or(0, |(anchor, _)| anchor.1))
}

pub(crate) fn focus_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(anchor_and_focus(cx, this).map(|(_, focus)| focus.0))
}

/// `focusOffset`: 0 while it is empty.
pub(crate) fn focus_offset<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<u32, ApiError> {
    Ok(anchor_and_focus(cx, this).map_or(0, |(_, focus)| focus.1))
}

/// Whether it has a range and the range is collapsed.
fn is_caret<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Selection>) -> Option<bool> {
    let range = range_of(cx, this)?;
    let (start, end) = (cx.range_start(range), cx.range_end(range));
    Some(Gc::ptr_eq(start.0, end.0) && start.1 == end.1)
}

/// `isCollapsed`: an empty selection is too.
pub(crate) fn is_collapsed<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<bool, ApiError> {
    Ok(is_caret(cx, this).unwrap_or(true))
}

/// `rangeCount`: 0 or 1.
pub(crate) fn range_count<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<u32, ApiError> {
    Ok(u32::from(range_of(cx, this).is_some()))
}

/// `type`: `None` while empty, `Caret` collapsed, else `Range`.
pub(crate) fn selection_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<DomStr<'gc>, ApiError> {
    let name = match is_caret(cx, this) {
        None => "None",
        Some(true) => "Caret",
        Some(false) => "Range",
    };
    Ok(DomStr::fresh(name))
}

/// `direction`: `none` while empty or collapsed (Chrome's; the
/// specification keeps a collapsed selection's), else `forward` or
/// `backward`.
pub(crate) fn direction<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<DomStr<'gc>, ApiError> {
    let name = if is_caret(cx, this).unwrap_or(true) {
        "none"
    } else if cx.selection_is_backward(this) {
        "backward"
    } else {
        "forward"
    };
    Ok(DomStr::fresh(name))
}

/// `getRangeAt(index)`: its range itself, not a copy; past it an
/// `IndexSizeError`.
pub(crate) fn get_range_at<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    index: u32,
) -> Result<Gc<'gc, Range>, ApiError> {
    match range_of(cx, this) {
        Some(range) if index == 0 => Ok(range),
        _ => Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::IndexSize,
            message: format!(
                "The index provided ({index}) is greater than or equal to the maximum bound."
            ),
        })),
    }
}

/// `addRange(range)`: takes `range` itself, unless the selection has one
/// already or `range` isn't in its document; forward (Chrome's).
pub(crate) fn add_range<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    range: Gc<'gc, Range>,
) -> Result<Option<FocusHost<'gc>>, ApiError> {
    if range_of(cx, this).is_some()
        || !in_document(cx, this, cx.range_start(range).0)
        || !in_document(cx, this, cx.range_end(range).0)
    {
        return Ok(None);
    }
    Ok(set(cx, this, Some(range), false)
        .then(|| focus_host(cx, this))
        .flatten())
}

/// `removeRange(range)`: empties it if `range` is its range; another is a
/// `NotFoundError`.
pub(crate) fn remove_range<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    range: Gc<'gc, Range>,
) -> Result<(), ApiError> {
    if !cx
        .selection_range(this)
        .is_some_and(|selected| Gc::ptr_eq(selected, range))
    {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::NotFound,
            message: "The given range isn't in document.".into(),
        }));
    }
    set(cx, this, None, false);
    Ok(())
}

/// `removeAllRanges()`, `empty()`.
pub(crate) fn remove_all_ranges<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<(), ApiError> {
    set(cx, this, None, false);
    Ok(())
}

/// A doctype is an `InvalidNodeTypeError`, an offset past the node's
/// length an `IndexSizeError`.
fn check_point<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, offset: u32) -> Result<(), ApiError> {
    if cx.kind(node) == NodeKind::DocumentType {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::InvalidNodeType,
            message: "The node provided is a doctype.".into(),
        }));
    }
    let length = cx.node_length(node);
    if offset > length {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::IndexSize,
            message: format!("The offset {offset} is larger than the node's length ({length})."),
        }));
    }
    Ok(())
}

/// `collapse(node, offset)`, `setPosition()`: a new collapsed range at the
/// point; null empties it; a node outside its document changes nothing.
pub(crate) fn collapse<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    node: Option<NodeRef<'gc>>,
    offset: u32,
) -> Result<Option<FocusHost<'gc>>, ApiError> {
    let Some(node) = node else {
        remove_all_ranges(cx, this)?;
        return Ok(None);
    };
    check_point(cx, node, offset)?;
    if !in_document(cx, this, node) {
        return Ok(None);
    }
    Ok(select(cx, this, (node, offset), (node, offset), false))
}

/// `collapseToStart()`, `collapseToEnd()`: a new collapsed range at its
/// range's start or end; an empty one is an `InvalidStateError`.
fn collapse_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    to_start: bool,
) -> Result<Option<FocusHost<'gc>>, ApiError> {
    let Some(range) = range_of(cx, this) else {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::InvalidState,
            message: "There is no selection.".into(),
        }));
    };
    let point = if to_start {
        cx.range_start(range)
    } else {
        cx.range_end(range)
    };
    Ok(select(cx, this, point, point, false))
}

pub(crate) fn collapse_to_start<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<Option<FocusHost<'gc>>, ApiError> {
    collapse_to(cx, this, true)
}

pub(crate) fn collapse_to_end<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<Option<FocusHost<'gc>>, ApiError> {
    collapse_to(cx, this, false)
}

/// The range from `anchor` to `focus` (in either order) and whether it is
/// backward; in different trees, collapsed at `focus` (setting a range's
/// end in another tree collapses it there).
fn ordered<'gc>(
    cx: &ApiCx<'_, 'gc>,
    anchor: BoundaryPoint<'gc>,
    focus: BoundaryPoint<'gc>,
) -> (BoundaryPoint<'gc>, BoundaryPoint<'gc>, bool) {
    if !Gc::ptr_eq(cx.root_node(anchor.0, false), cx.root_node(focus.0, false)) {
        return (focus, focus, false);
    }
    if crate::range::position(cx, focus, anchor) == Ordering::Less {
        (focus, anchor, true)
    } else {
        (anchor, focus, false)
    }
}

/// `extend(node, offset)`: the anchor stays, the focus moves to the point
/// (into another tree, both do); a new range. A node outside the document
/// changes nothing; an empty selection is an `InvalidStateError`, then the
/// point's checks.
pub(crate) fn extend<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    node: NodeRef<'gc>,
    offset: u32,
) -> Result<Option<FocusHost<'gc>>, ApiError> {
    if !in_document(cx, this, node) {
        return Ok(None);
    }
    let Some((anchor, _)) = anchor_and_focus(cx, this) else {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::InvalidState,
            message: "This Selection object doesn't have any Ranges.".into(),
        }));
    };
    check_point(cx, node, offset)?;
    let (start, end, backward) = ordered(cx, anchor, (node, offset));
    Ok(select(cx, this, start, end, backward))
}

/// `setBaseAndExtent(anchorNode, anchorOffset, focusNode, focusOffset)`: a
/// new range from the anchor to the focus, backward if the focus comes
/// first. The points' checks first; a node outside the document changes
/// nothing.
pub(crate) fn set_base_and_extent<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    anchor_node: NodeRef<'gc>,
    anchor_offset: u32,
    focus_node: NodeRef<'gc>,
    focus_offset: u32,
) -> Result<Option<FocusHost<'gc>>, ApiError> {
    check_point(cx, anchor_node, anchor_offset)?;
    check_point(cx, focus_node, focus_offset)?;
    if !in_document(cx, this, anchor_node) || !in_document(cx, this, focus_node) {
        return Ok(None);
    }
    let (start, end, backward) =
        ordered(cx, (anchor_node, anchor_offset), (focus_node, focus_offset));
    Ok(select(cx, this, start, end, backward))
}

/// `selectAllChildren(node)`: a new range over its children (a text's
/// none), forward; a
/// doctype is an `InvalidNodeTypeError`, a node outside the document
/// changes nothing.
pub(crate) fn select_all_children<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    node: NodeRef<'gc>,
) -> Result<Option<FocusHost<'gc>>, ApiError> {
    check_point(cx, node, 0)?;
    if !in_document(cx, this, node) {
        return Ok(None);
    }
    // Its children: a character data node's are none.
    let children =
        std::iter::successors(cx.first_child(node), |child| cx.next_sibling(*child)).count();
    let children = u32::try_from(children).unwrap_or(u32::MAX);
    Ok(select(cx, this, (node, 0), (node, children), false))
}

/// `deleteFromDocument()`: its range's `deleteContents()`, if it has one.
pub(crate) fn delete_from_document<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<(), ApiError> {
    match range_of(cx, this) {
        Some(range) => crate::range::delete_contents(cx, range),
        None => Ok(()),
    }
}

/// `containsNode(node, allowPartialContainment)`, as Blink's
/// `DOMSelection::containsNode`: whether `node` (from before it to after
/// it, in its parent) is in its range; one partly in it counts with
/// `allowPartialContainment`, and a text always does. False while empty,
/// for a node without a parent or in another tree.
pub(crate) fn contains_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
    node: NodeRef<'gc>,
    allow_partial: Option<bool>,
) -> Result<bool, ApiError> {
    let Some(range) = range_of(cx, this) else {
        return Ok(false);
    };
    let (start, end) = (cx.range_start(range), cx.range_end(range));
    let Some(parent) = cx.parent(node) else {
        return Ok(false);
    };
    if !Gc::ptr_eq(cx.root_node(node, false), cx.root_node(start.0, false)) {
        return Ok(false);
    }
    let index = cx.index(node);
    let (before, after) = ((parent, index), (parent, index.saturating_add(1)));
    let position = |a, b| crate::range::position(cx, a, b);
    if position(start, before) != Ordering::Greater && position(end, after) != Ordering::Less {
        return Ok(true);
    }
    if position(before, end) == Ordering::Greater || position(after, start) == Ordering::Less {
        return Ok(false);
    }
    Ok(allow_partial.unwrap_or(false) || cx.kind(node) == NodeKind::Text)
}

/// `toString()`: its range's text, empty while it has none. Chrome's is
/// the text as laid out (`innerText`'s); machopo's is the range's.
pub(crate) fn to_string<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Selection>,
) -> Result<DomStr<'gc>, ApiError> {
    match range_of(cx, this) {
        Some(range) => crate::range::to_string(cx, range),
        None => Ok(DomStr::fresh("")),
    }
}
