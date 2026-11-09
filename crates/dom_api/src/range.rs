//! `AbstractRange`, `Range` and `StaticRange`
//! (https://dom.spec.whatwg.org/#ranges; Blink's `Range`, `StaticRange`):
//! boundary points set with DOM's checks and compared in tree order, the
//! contents (DOM's extract, clone, delete, insert, surround), `toString`,
//! HTML's `createContextualFragment` and CSSOM View's rects. `dom`
//! keeps them, and adjusts a live range as its tree changes.

use std::cmp::Ordering;

use dom::{BoundaryPoint, DomString, NodeKind, NodeRef, Range};
use elise::Gc;
use gfx::RectF;

use crate::exception::ExceptionName;
use crate::typed::{DomStr, OpFront, StaticRangeInit};
use crate::{ApiCx, ApiError};

fn invalid_node_type(message: &str) -> ApiError {
    ApiError::Dom(crate::exception::DomException {
        name: ExceptionName::InvalidNodeType,
        message: message.into(),
    })
}

fn wrong_document(message: &str) -> ApiError {
    ApiError::Dom(crate::exception::DomException {
        name: ExceptionName::WrongDocument,
        message: message.into(),
    })
}

/// `node`'s root (DOM's, not through shadow roots' hosts).
fn root<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> NodeRef<'gc> {
    cx.root_node(node, false)
}

/// `a` and `b` in tree order, in one tree (`Equal` for one node): an
/// ancestor before its descendants, siblings in their order.
fn tree_order<'gc>(cx: &ApiCx<'_, 'gc>, a: NodeRef<'gc>, b: NodeRef<'gc>) -> Ordering {
    if Gc::ptr_eq(a, b) {
        return Ordering::Equal;
    }
    // A descendant's `CONTAINED_BY` comes with `FOLLOWING`.
    if cx.compare_document_position(a, b) & dom::DOCUMENT_POSITION_FOLLOWING != 0 {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// DOM's "position" of boundary point `a` against `b`, both in one tree.
pub(crate) fn position<'gc>(
    cx: &ApiCx<'_, 'gc>,
    a: BoundaryPoint<'gc>,
    b: BoundaryPoint<'gc>,
) -> Ordering {
    let ((a_node, a_offset), (b_node, b_offset)) = (a, b);
    if Gc::ptr_eq(a_node, b_node) {
        return a_offset.cmp(&b_offset);
    }
    if tree_order(cx, a_node, b_node) == Ordering::Greater {
        return position(cx, b, a).reverse();
    }
    // `a`'s node is before `b`'s: if it is `b`'s ancestor, `b` is in the
    // child of it that holds `b`'s node.
    let mut child = b_node;
    while let Some(parent) = cx.parent(child) {
        if Gc::ptr_eq(parent, a_node) {
            if cx.index(child) < a_offset {
                return Ordering::Greater;
            }
            break;
        }
        child = parent;
    }
    Ordering::Less
}

/// Its boundary points set (checked); the selection whose range it is
/// follows it (`selection::range_set`).
fn set_range<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    start: BoundaryPoint<'gc>,
    end: BoundaryPoint<'gc>,
) {
    let Some(selection) = cx.range_selection(this) else {
        cx.set_range(this, start, end);
        return;
    };
    let before = crate::selection::seen(cx, selection);
    cx.set_range(this, start, end);
    crate::selection::range_set(cx, selection, this, before);
}

/// `document.createRange()`: collapsed at the start of the document.
pub(crate) fn create_range<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, Range>, ApiError> {
    Ok(cx.create_range((this, 0), (this, 0)))
}

/// `new Range()`: collapsed at the start of the realm's document.
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<Gc<'gc, Range>, ApiError> {
    let document = crate::character_data::realm_document(op, cx)?;
    Ok(cx.create_range((document, 0), (document, 0)))
}

/// `new StaticRange(init)`: a doctype or an attribute can't be a boundary
/// point's node; the offsets aren't checked.
pub(crate) fn construct_static<'gc>(
    cx: &ApiCx<'_, 'gc>,
    init: StaticRangeInit<'gc>,
) -> Result<Gc<'gc, Range>, ApiError> {
    for (node, _) in [init.start, init.end] {
        if matches!(cx.kind(node), NodeKind::DocumentType | NodeKind::Attr) {
            return Err(invalid_node_type(
                "Failed to construct 'StaticRange': a boundary point can't be in a doctype or an attribute",
            ));
        }
    }
    Ok(cx.create_static_range(init.start, init.end))
}

pub(crate) fn start_container<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.range_start(this).0)
}

pub(crate) fn start_offset<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<u32, ApiError> {
    Ok(cx.range_start(this).1)
}

pub(crate) fn end_container<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.range_end(this).0)
}

pub(crate) fn end_offset<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Range>) -> Result<u32, ApiError> {
    Ok(cx.range_end(this).1)
}

/// `collapsed`: its start is its end.
pub(crate) fn collapsed<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Range>) -> Result<bool, ApiError> {
    let ((start, start_offset), (end, end_offset)) = (cx.range_start(this), cx.range_end(this));
    Ok(Gc::ptr_eq(start, end) && start_offset == end_offset)
}

/// The checks of a boundary point: not in a doctype, `offset` within the
/// node's length.
fn check_point<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, offset: u32) -> Result<(), ApiError> {
    if cx.kind(node) == NodeKind::DocumentType {
        return Err(invalid_node_type(
            "a range's boundary point can't be in a doctype",
        ));
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

/// DOM's "set the start or end": the point checked; the other end moves
/// to it too if it would be on the wrong side, or in another tree.
fn set_start_or_end<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    point: BoundaryPoint<'gc>,
    start: bool,
) -> Result<(), ApiError> {
    check_point(cx, point.0, point.1)?;
    let (old_start, old_end) = (cx.range_start(this), cx.range_end(this));
    let other_tree = !Gc::ptr_eq(root(cx, point.0), root(cx, old_start.0));
    let (new_start, new_end) = if start {
        let past_end = other_tree || position(cx, point, old_end) == Ordering::Greater;
        (point, if past_end { point } else { old_end })
    } else {
        let before_start = other_tree || position(cx, point, old_start) == Ordering::Less;
        (if before_start { point } else { old_start }, point)
    };
    set_range(cx, this, new_start, new_end);
    Ok(())
}

pub(crate) fn set_start<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
    offset: u32,
) -> Result<(), ApiError> {
    set_start_or_end(cx, this, (node, offset), true)
}

pub(crate) fn set_end<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
    offset: u32,
) -> Result<(), ApiError> {
    set_start_or_end(cx, this, (node, offset), false)
}

/// The boundary point before `node` in its parent (`after`: after it);
/// a node without a parent is an `InvalidNodeTypeError`.
fn around<'gc>(
    cx: &ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
    after: bool,
) -> Result<BoundaryPoint<'gc>, ApiError> {
    let parent = cx
        .parent(node)
        .ok_or_else(|| invalid_node_type("the node has no parent"))?;
    let index = cx.index(node);
    // An index is under `u32::MAX`: each sibling before it is a node in
    // memory.
    Ok((
        parent,
        if after {
            index.saturating_add(1)
        } else {
            index
        },
    ))
}

pub(crate) fn set_start_before<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    set_start_or_end(cx, this, around(cx, node, false)?, true)
}

pub(crate) fn set_start_after<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    set_start_or_end(cx, this, around(cx, node, true)?, true)
}

pub(crate) fn set_end_before<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    set_start_or_end(cx, this, around(cx, node, false)?, false)
}

pub(crate) fn set_end_after<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    set_start_or_end(cx, this, around(cx, node, true)?, false)
}

/// `collapse(toStart)`: to its start, or its end.
pub(crate) fn collapse<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    to_start: Option<bool>,
) -> Result<(), ApiError> {
    let point = if to_start.unwrap_or(false) {
        cx.range_start(this)
    } else {
        cx.range_end(this)
    };
    set_range(cx, this, point, point);
    Ok(())
}

/// `selectNode(node)`: from before `node` to after it, in its parent.
pub(crate) fn select_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    let (parent, index) = around(cx, node, false)?;
    set_range(cx, this, (parent, index), (parent, index.saturating_add(1)));
    Ok(())
}

/// `selectNodeContents(node)`: from its start to its end; a doctype is an
/// `InvalidNodeTypeError`.
pub(crate) fn select_node_contents<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    if cx.kind(node) == NodeKind::DocumentType {
        return Err(invalid_node_type(
            "a range's boundary point can't be in a doctype",
        ));
    }
    set_range(cx, this, (node, 0), (node, cx.node_length(node)));
    Ok(())
}

/// `cloneRange()`: a new live range with the same boundary points.
pub(crate) fn clone_range<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<Gc<'gc, Range>, ApiError> {
    Ok(cx.create_range(cx.range_start(this), cx.range_end(this)))
}

/// `detach()`: nothing (DOM keeps it for compatibility).
pub(crate) fn detach<'gc>(_: &ApiCx<'_, 'gc>, _: Gc<'gc, Range>) -> Result<(), ApiError> {
    Ok(())
}

/// `commonAncestorContainer`: the closest inclusive ancestor of its start's
/// node that is one of its end's.
pub(crate) fn common_ancestor_container<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<NodeRef<'gc>, ApiError> {
    let ((start, _), (end, _)) = (cx.range_start(this), cx.range_end(this));
    let mut ancestor = start;
    while !cx.is_inclusive_ancestor(ancestor, end) {
        ancestor = cx
            .parent(ancestor)
            .expect("a range's boundary points are in one tree");
    }
    Ok(ancestor)
}

fn ordering(order: Ordering) -> i16 {
    match order {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// `compareBoundaryPoints(how, sourceRange)`: one of its boundary points
/// against one of `source`'s, as `how` says (`START_TO_START`,
/// `START_TO_END`: its end against the source's start, `END_TO_END`,
/// `END_TO_START`: its start against the source's end): -1 before, 0, 1
/// after. Past 3 a `NotSupportedError`; ranges in different trees a
/// `WrongDocumentError`.
pub(crate) fn compare_boundary_points<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    how: u16,
    source: Gc<'gc, Range>,
) -> Result<i16, ApiError> {
    // The front end took a range's wrapper: a static one isn't a `Range`.
    if cx.is_static_range(source) {
        return Err(ApiError::type_error(
            "Failed to execute 'compareBoundaryPoints' on 'Range': parameter 2 is not of type 'Range'.",
        ));
    }
    if how > 3 {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::NotSupported,
            message: format!("The comparison method provided ({how}) is not supported."),
        }));
    }
    let (this_start, this_end) = (cx.range_start(this), cx.range_end(this));
    let (source_start, source_end) = (cx.range_start(source), cx.range_end(source));
    if !Gc::ptr_eq(root(cx, this_start.0), root(cx, source_start.0)) {
        return Err(wrong_document("The two ranges are in different trees."));
    }
    let (point, other) = match how {
        0 => (this_start, source_start),
        1 => (this_end, source_start),
        2 => (this_end, source_end),
        _ => (this_start, source_end),
    };
    Ok(ordering(position(cx, point, other)))
}

/// Where a point in the range's tree is against it, once checked: before
/// its start, in it, after its end.
fn point_against<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    point: BoundaryPoint<'gc>,
) -> Result<Ordering, ApiError> {
    check_point(cx, point.0, point.1)?;
    if position(cx, point, cx.range_start(this)) == Ordering::Less {
        return Ok(Ordering::Less);
    }
    if position(cx, point, cx.range_end(this)) == Ordering::Greater {
        return Ok(Ordering::Greater);
    }
    Ok(Ordering::Equal)
}

/// Whether `node` is in the range's tree.
fn in_tree<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Range>, node: NodeRef<'gc>) -> bool {
    Gc::ptr_eq(root(cx, node), root(cx, cx.range_start(this).0))
}

/// `isPointInRange(node, offset)`: false in another tree; then the checks;
/// whether the point is between its start and end, both included.
pub(crate) fn is_point_in_range<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
    offset: u32,
) -> Result<bool, ApiError> {
    if !in_tree(cx, this, node) {
        return Ok(false);
    }
    Ok(point_against(cx, this, (node, offset))? == Ordering::Equal)
}

/// `comparePoint(node, offset)`: another tree is a `WrongDocumentError`;
/// then the checks; -1 before its start, 1 after its end, else 0.
pub(crate) fn compare_point<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
    offset: u32,
) -> Result<i16, ApiError> {
    if !in_tree(cx, this, node) {
        return Err(wrong_document(
            "The node provided and the range are in different trees.",
        ));
    }
    Ok(ordering(point_against(cx, this, (node, offset))?))
}

/// `intersectsNode(node)`: false in another tree, true without a parent;
/// else whether the node, from before it to after it in its parent,
/// overlaps the range.
pub(crate) fn intersects_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    if !in_tree(cx, this, node) {
        return Ok(false);
    }
    let Some(parent) = cx.parent(node) else {
        return Ok(true);
    };
    let index = cx.index(node);
    Ok(
        position(cx, (parent, index), cx.range_end(this)) == Ordering::Less
            && position(cx, (parent, index.saturating_add(1)), cx.range_start(this))
                == Ordering::Greater,
    )
}

/// `node`'s data from `from` to `to` (UTF-16 code units, at most its end).
fn substring<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, from: u32, to: u32) -> String {
    let (from, to) = (units(from), units(to));
    cx.text_data(node)
        .expect("character data's data")
        .borrow()
        .utf16_substring(from, to.saturating_sub(from))
}

fn units(offset: u32) -> usize {
    usize::try_from(offset).expect("a `u32` fits a `usize`")
}

/// Whether `node` is a text (a CDATA section too: DOM's `Text`).
fn is_text<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    cx.kind(node) == NodeKind::Text
}

fn is_character_data<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    cx.kind(node).is_character_data()
}

/// DOM's "contained": `node`, in the range's tree, from after its start to
/// before its end.
fn is_contained<'gc>(
    cx: &ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
    start: BoundaryPoint<'gc>,
    end: BoundaryPoint<'gc>,
) -> bool {
    Gc::ptr_eq(root(cx, node), root(cx, start.0))
        && position(cx, (node, 0), start) == Ordering::Greater
        && position(cx, (node, cx.node_length(node)), end) == Ordering::Less
}

/// The closest inclusive ancestor of `start`'s node that is one of
/// `end`'s.
fn common_ancestor<'gc>(
    cx: &ApiCx<'_, 'gc>,
    start: NodeRef<'gc>,
    end: NodeRef<'gc>,
) -> NodeRef<'gc> {
    let mut ancestor = start;
    while !cx.is_inclusive_ancestor(ancestor, end) {
        ancestor = cx
            .parent(ancestor)
            .expect("a range's boundary points are in one tree");
    }
    ancestor
}

/// `toString()` (its stringifier): the start's text's data from its
/// offset, the data of each text contained, in tree order, the end's
/// text's up to its offset; one text's in between if both are in it.
pub(crate) fn to_string<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<DomStr<'gc>, ApiError> {
    let (start, end) = (cx.range_start(this), cx.range_end(this));
    if Gc::ptr_eq(start.0, end.0) && is_text(cx, start.0) {
        return Ok(DomStr::Owned(DomString::from(substring(
            cx, start.0, start.1, end.1,
        ))));
    }
    let mut text = String::new();
    if is_text(cx, start.0) {
        text.push_str(&substring(cx, start.0, start.1, u32::MAX));
    }
    for_each_descendant(cx, common_ancestor(cx, start.0, end.0), |node| {
        if is_text(cx, node) && is_contained(cx, node, start, end) {
            text.push_str(&cx.text_data(node).expect("a text's data").borrow().to_str());
        }
    });
    if is_text(cx, end.0) {
        text.push_str(&substring(cx, end.0, 0, end.1));
    }
    Ok(DomStr::Owned(DomString::from(text)))
}

/// `f` of `root`'s descendants, in tree order.
fn for_each_descendant<'gc>(
    cx: &ApiCx<'_, 'gc>,
    root: NodeRef<'gc>,
    mut f: impl FnMut(NodeRef<'gc>),
) {
    let mut at = cx.first_child(root);
    while let Some(node) = at {
        f(node);
        at = cx
            .first_child(node)
            .or_else(|| next_past(cx, node, Some(root)));
    }
}

/// The node after `node`'s subtree in tree order, not past `root`'s.
fn next_past<'gc>(
    cx: &ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
    root: Option<NodeRef<'gc>>,
) -> Option<NodeRef<'gc>> {
    let mut at = node;
    loop {
        if root.is_some_and(|root| Gc::ptr_eq(at, root)) {
            return None;
        }
        if let Some(next) = cx.next_sibling(at) {
            return Some(next);
        }
        at = cx.parent(at)?;
    }
}

// --- The contents ------------------------------------------------------------

/// What extracting, cloning and deleting find before they change anything
/// (DOM's): the common ancestor's first and last children the range
/// partially contains (none where an end is the other's inclusive
/// ancestor), those it contains, and where it collapses to after an
/// extraction or deletion (its start if that holds its end, else after
/// the start's ancestor that is the common ancestor's child).
struct Parts<'gc> {
    first_partial: Option<NodeRef<'gc>>,
    last_partial: Option<NodeRef<'gc>>,
    contained: Vec<NodeRef<'gc>>,
    collapse_to: BoundaryPoint<'gc>,
}

fn parts<'gc>(
    cx: &ApiCx<'_, 'gc>,
    start: BoundaryPoint<'gc>,
    end: BoundaryPoint<'gc>,
) -> Parts<'gc> {
    let common = common_ancestor(cx, start.0, end.0);
    let partial = |child: NodeRef<'gc>| {
        cx.is_inclusive_ancestor(child, start.0) != cx.is_inclusive_ancestor(child, end.0)
    };
    let children =
        || std::iter::successors(cx.first_child(common), |child| cx.next_sibling(*child));
    let start_holds_end = cx.is_inclusive_ancestor(start.0, end.0);
    let first_partial = if start_holds_end {
        None
    } else {
        children().find(|child| partial(*child))
    };
    let last_partial = if cx.is_inclusive_ancestor(end.0, start.0) {
        None
    } else {
        children().filter(|child| partial(*child)).last()
    };
    let contained = children()
        .filter(|child| is_contained(cx, *child, start, end))
        .collect();
    let collapse_to = if start_holds_end {
        start
    } else {
        let mut reference = start.0;
        while let Some(parent) = cx
            .parent(reference)
            .filter(|parent| !cx.is_inclusive_ancestor(*parent, end.0))
        {
            reference = parent;
        }
        let parent = cx.parent(reference).expect("under the common ancestor");
        (parent, cx.index(reference).saturating_add(1))
    };
    Parts {
        first_partial,
        last_partial,
        contained,
        collapse_to,
    }
}

/// A copy of character data `node` with its data from `from` to `to`.
fn clone_with_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
    from: u32,
    to: u32,
) -> NodeRef<'gc> {
    let data = substring(cx, node, from, to);
    let clone = cx.clone_node(node, None, false);
    cx.set_data(clone, &data);
    clone
}

/// DOM's "extract" (`extract`) or "clone the contents" of the range from
/// `start` to `end`: a new fragment of its start's document with what is
/// in it. Character data partly in it gives a copy of that part
/// (extracting takes the part out of it); a node partly in it, a shallow
/// copy holding what of it is in the range, recursively; a node wholly in
/// it moves into the fragment, or is copied deep. A doctype in it is a
/// `HierarchyRequestError`. The fragment, and where an extracted range
/// collapses to (none if it is collapsed or in one node's data, where
/// removing that data collapses it).
fn clone_or_extract<'gc>(
    cx: &ApiCx<'_, 'gc>,
    start: BoundaryPoint<'gc>,
    end: BoundaryPoint<'gc>,
    extract: bool,
) -> Result<(NodeRef<'gc>, Option<BoundaryPoint<'gc>>), ApiError> {
    let fragment = cx.create_document_fragment(cx.document_of(start.0));
    if Gc::ptr_eq(start.0, end.0) && start.1 == end.1 {
        return Ok((fragment, None));
    }
    if Gc::ptr_eq(start.0, end.0) && is_character_data(cx, start.0) {
        cx.append_child(fragment, clone_with_data(cx, start.0, start.1, end.1))?;
        if extract {
            // Start before end in one node: no underflow.
            cx.replace_data(
                start.0,
                units(start.1),
                units(end.1.saturating_sub(start.1)),
                "",
            );
        }
        return Ok((fragment, None));
    }
    let parts = parts(cx, start, end);
    if parts
        .contained
        .iter()
        .any(|node| cx.kind(*node) == NodeKind::DocumentType)
    {
        return Err(ApiError::dom(ExceptionName::HierarchyRequest));
    }
    if let Some(first) = parts.first_partial {
        if is_character_data(cx, first) {
            cx.append_child(fragment, clone_with_data(cx, start.0, start.1, u32::MAX))?;
            if extract {
                cx.replace_data(start.0, units(start.1), usize::MAX, "");
            }
        } else {
            let clone = cx.clone_node(first, None, false);
            cx.append_child(fragment, clone)?;
            let (inner, _) = clone_or_extract(cx, start, (first, cx.node_length(first)), extract)?;
            cx.append_child(clone, inner)?;
        }
    }
    for child in parts.contained {
        let child = if extract {
            child
        } else {
            cx.clone_node(child, None, true)
        };
        cx.append_child(fragment, child)?;
    }
    if let Some(last) = parts.last_partial {
        if is_character_data(cx, last) {
            cx.append_child(fragment, clone_with_data(cx, end.0, 0, end.1))?;
            if extract {
                cx.replace_data(end.0, 0, units(end.1), "");
            }
        } else {
            let clone = cx.clone_node(last, None, false);
            cx.append_child(fragment, clone)?;
            let (inner, _) = clone_or_extract(cx, (last, 0), end, extract)?;
            cx.append_child(clone, inner)?;
        }
    }
    Ok((fragment, Some(parts.collapse_to)))
}

/// `extractContents()`: what is in it moves into a new fragment (copies of
/// the parts of nodes partly in it), and it collapses where its start was.
pub(crate) fn extract_contents<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<NodeRef<'gc>, ApiError> {
    let (fragment, collapse_to) =
        clone_or_extract(cx, cx.range_start(this), cx.range_end(this), true)?;
    if let Some(point) = collapse_to {
        set_range(cx, this, point, point);
    }
    Ok(fragment)
}

/// `cloneContents()`: a new fragment with copies of what is in it.
pub(crate) fn clone_contents<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(clone_or_extract(cx, cx.range_start(this), cx.range_end(this), false)?.0)
}

/// `deleteContents()`: what is in it goes (the parts of character data
/// partly in it, the nodes it contains whose parent it doesn't), and it
/// collapses where its start was.
pub(crate) fn delete_contents<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<(), ApiError> {
    let (start, end) = (cx.range_start(this), cx.range_end(this));
    if Gc::ptr_eq(start.0, end.0) && start.1 == end.1 {
        return Ok(());
    }
    if Gc::ptr_eq(start.0, end.0) && is_character_data(cx, start.0) {
        // Start before end in one node: no underflow.
        cx.replace_data(
            start.0,
            units(start.1),
            units(end.1.saturating_sub(start.1)),
            "",
        );
        return Ok(());
    }
    let common = common_ancestor(cx, start.0, end.0);
    let mut to_remove = Vec::new();
    let mut at = cx.first_child(common);
    while let Some(node) = at {
        if is_contained(cx, node, start, end) {
            // Its descendants go with it.
            to_remove.push(node);
            at = next_past(cx, node, Some(common));
        } else {
            at = cx
                .first_child(node)
                .or_else(|| next_past(cx, node, Some(common)));
        }
    }
    let collapse_to = parts(cx, start, end).collapse_to;
    if is_character_data(cx, start.0) {
        cx.replace_data(start.0, units(start.1), usize::MAX, "");
    }
    for node in to_remove {
        cx.remove(node);
    }
    if is_character_data(cx, end.0) {
        cx.replace_data(end.0, 0, units(end.1), "");
    }
    set_range(cx, this, collapse_to, collapse_to);
    Ok(())
}

/// DOM's "insert" of `node` into the range at its start: a comment's, a
/// processing instruction's, a text without a parent's or `node`'s own
/// start is a `HierarchyRequestError`; in a text it splits there (once
/// `node` is known to fit) and `node` goes between the halves, else
/// before the start's child at its offset. A collapsed range then ends
/// after `node` (a fragment's children).
fn insert<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    let (start, offset) = cx.range_start(this);
    let start_is_text = is_text(cx, start);
    if matches!(
        cx.kind(start),
        NodeKind::Comment | NodeKind::ProcessingInstruction
    ) || (start_is_text && cx.parent(start).is_none())
        || Gc::ptr_eq(start, node)
    {
        return Err(ApiError::dom(ExceptionName::HierarchyRequest));
    }
    let reference = if start_is_text {
        Some(start)
    } else {
        std::iter::successors(cx.first_child(start), |child| cx.next_sibling(*child))
            .nth(units(offset))
    };
    let parent = match reference {
        Some(reference) => cx.parent(reference).expect("the reference has a parent"),
        None => start,
    };
    cx.ensure_pre_insertion_validity(parent, node, reference)?;
    let mut reference = if start_is_text {
        Some(cx.split_text(start, offset))
    } else {
        reference
    };
    if reference.is_some_and(|reference| Gc::ptr_eq(reference, node)) {
        reference = cx.next_sibling(node);
    }
    if cx.parent(node).is_some() {
        cx.remove(node);
    }
    let at = match reference {
        Some(reference) => cx.index(reference),
        None => cx.node_length(parent),
    };
    let added = if cx.kind(node) == NodeKind::DocumentFragment {
        cx.node_length(node)
    } else {
        1
    };
    // Under the children `parent` will have: fewer than a `u32` counts.
    let new_offset = at.saturating_add(added);
    cx.pre_insert(parent, node, reference)?;
    let (start, end) = (cx.range_start(this), cx.range_end(this));
    if Gc::ptr_eq(start.0, end.0) && start.1 == end.1 {
        set_range(cx, this, start, (parent, new_offset));
    }
    Ok(())
}

/// `insertNode(node)`.
pub(crate) fn insert_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    insert(cx, this, node)
}

/// `surroundContents(newParent)`: its contents move into `new_parent`
/// (emptied first), which takes their place and is then what it selects.
/// A node other than a text partly in it is an `InvalidStateError`; a
/// document, a doctype or a fragment as `new_parent` an
/// `InvalidNodeTypeError`.
pub(crate) fn surround_contents<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    new_parent: NodeRef<'gc>,
) -> Result<(), ApiError> {
    let (start, end) = (cx.range_start(this), cx.range_end(this));
    // The nodes partly in it: the inclusive ancestors of one end only.
    let partly = |from: NodeRef<'gc>, other: NodeRef<'gc>| {
        std::iter::successors(Some(from), |node| cx.parent(*node))
            .take_while(|node| !cx.is_inclusive_ancestor(*node, other))
            .any(|node| !is_text(cx, node))
    };
    if partly(start.0, end.0) || partly(end.0, start.0) {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::InvalidState,
            message: "The Range has partially selected a non-Text node.".into(),
        }));
    }
    if matches!(
        cx.kind(new_parent),
        NodeKind::Document | NodeKind::DocumentType | NodeKind::DocumentFragment
    ) {
        return Err(invalid_node_type(
            "The node provided is of type document, doctype or document fragment.",
        ));
    }
    let (fragment, collapse_to) = clone_or_extract(cx, start, end, true)?;
    if let Some(point) = collapse_to {
        set_range(cx, this, point, point);
    }
    if cx.first_child(new_parent).is_some() {
        cx.replace_all(new_parent, None);
    }
    insert(cx, this, new_parent)?;
    cx.append_child(new_parent, fragment)?;
    select_node(cx, this, new_parent)
}

/// `createContextualFragment(string)` (HTML's): `string` parsed with the
/// start's element as the context (a text's or a comment's parent element;
/// a new `<body>` for none, or an HTML document's `<html>`), in a fragment
/// of the start's document. Its scripts aren't already started nor the
/// parser's, so they run once inserted (unlike `innerHTML`'s); its
/// custom elements are upgraded in this call's `[CEReactions]`. An XML
/// document's markup is a `NotSupportedError`.
pub(crate) fn create_contextual_fragment<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
    string: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    let (start, _) = cx.range_start(this);
    crate::markup::html_only(cx, start, "createContextualFragment")?;
    let element = match cx.kind(start) {
        NodeKind::Element => Some(start),
        NodeKind::Text | NodeKind::Comment => cx
            .parent(start)
            .filter(|parent| cx.kind(*parent) == NodeKind::Element),
        _ => None,
    };
    let is_html_root = |element: NodeRef<'gc>| {
        cx.is_html_document(cx.document_of(element))
            && cx.element_name(element).is_some_and(|name| {
                name.namespace == dom::names::HTML_NAMESPACE
                    && name.local == dom::names::local::HTML
            })
    };
    let context = match element {
        Some(element) if !is_html_root(element) => element,
        _ => crate::markup::body_of(cx, start),
    };
    let fragment = crate::markup::parse_fragment(cx, context, string);
    for_each_descendant(cx, fragment, |node| {
        let is_script = cx.element_name(node).is_some_and(|name| {
            name.namespace == dom::names::HTML_NAMESPACE && name.local == dom::names::local::SCRIPT
        });
        if is_script {
            cx.set_script_flags(node, dom::ScriptFlags::default());
        }
    });
    cx.upgrade_defined_in(fragment);
    Ok(fragment)
}

// --- CSSOM View (https://drafts.csswg.org/cssom-view/#extensions-to-the-range-interface)

/// A part of a range's boxes: an element it holds whole, or a text's data
/// from one offset to another.
enum BoxPart<'gc> {
    Element(NodeRef<'gc>),
    Text(NodeRef<'gc>, u32, u32),
}

/// The parts of the range's boxes, in tree order: from its first node to
/// the one past its last (a character data container is its first or
/// last node), each element whose parent isn't among them, unless it holds
/// the start's or the end's container (when they differ), and each text's
/// part in it. A range not in a document's are none: no layout is lent
/// for them (`ApiCx::with_layout`).
fn box_parts<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Range>) -> Vec<BoxPart<'gc>> {
    let ((start, start_offset), (end, end_offset)) = (cx.range_start(this), cx.range_end(this));
    let child_at = |node: NodeRef<'gc>, offset: u32| {
        std::iter::successors(cx.first_child(node), |child| cx.next_sibling(*child))
            .nth(units(offset))
    };
    let first = if is_character_data(cx, start) {
        Some(start)
    } else if let Some(child) = child_at(start, start_offset) {
        Some(child)
    } else if start_offset == 0 {
        Some(start)
    } else {
        next_past(cx, start, None)
    };
    let past_last = if is_character_data(cx, end) {
        next_past(cx, end, None)
    } else {
        child_at(end, end_offset).or_else(|| next_past(cx, end, None))
    };
    let mut nodes = Vec::new();
    let mut at = first;
    while let Some(node) = at {
        if past_last.is_some_and(|past| Gc::ptr_eq(past, node)) {
            break;
        }
        if matches!(cx.kind(node), NodeKind::Element | NodeKind::Text) {
            nodes.push(node);
        }
        at = cx.first_child(node).or_else(|| next_past(cx, node, None));
    }
    let around = |node: NodeRef<'gc>| {
        !Gc::ptr_eq(start, end)
            && (cx.is_inclusive_ancestor(node, start) || cx.is_inclusive_ancestor(node, end))
    };
    nodes
        .iter()
        .filter_map(|&node| {
            if cx.kind(node) == NodeKind::Text {
                let from = if Gc::ptr_eq(node, start) {
                    start_offset
                } else {
                    0
                };
                let to = if Gc::ptr_eq(node, end) {
                    end_offset
                } else {
                    u32::MAX
                };
                return Some(BoxPart::Text(node, from, to));
            }
            let parent_in = cx
                .parent(node)
                .is_some_and(|parent| nodes.iter().any(|n| Gc::ptr_eq(*n, parent)));
            (!parent_in && !around(node)).then_some(BoxPart::Element(node))
        })
        .collect()
}

/// The range's boxes, CSS px of the viewport, its layout up to date first.
fn client_rects<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Range>) -> Vec<RectF> {
    let parts = box_parts(cx, this);
    let Some(first) = parts.first() else {
        return Vec::new();
    };
    let anchor = match first {
        BoxPart::Element(node) | BoxPart::Text(node, ..) => *node,
    };
    cx.with_layout(anchor, |layout| {
        let mut rects = Vec::new();
        for part in &parts {
            match *part {
                BoxPart::Element(element) => element_rects(cx, layout, element, &mut rects),
                BoxPart::Text(text, from, to) => {
                    rects.extend(text_rects(cx, layout, text, from, to))
                }
            }
        }
        rects
    })
    .unwrap_or_default()
}

fn text_rects<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: &mut dyn crate::FrameLayout,
    text: NodeRef<'gc>,
    from: u32,
    to: u32,
) -> Vec<RectF> {
    let element = cx.parent(text).map(|parent| cx.node_id(parent));
    layout.text_rects(cx.node_id(text), element, from, to)
}

/// The boxes of `element`, held whole by a range: its own, or for a
/// `display: contents` element, which has none, its children's (Blink's
/// `Range::GetBorderAndTextQuads` goes into it).
fn element_rects<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: &mut dyn crate::FrameLayout,
    element: NodeRef<'gc>,
    out: &mut Vec<RectF>,
) {
    let contents = layout
        .computed_style(cx, element)
        .is_some_and(|style| style.display == computed_style::Display::Contents);
    if !contents {
        out.extend(crate::geometry::client_rects_in(cx, layout, element));
        return;
    }
    let mut at = cx.first_child(element);
    while let Some(child) = at {
        match cx.kind(child) {
            NodeKind::Element => element_rects(cx, layout, child, out),
            NodeKind::Text => out.extend(text_rects(cx, layout, child, 0, u32::MAX)),
            _ => {}
        }
        at = cx.next_sibling(child);
    }
}

/// `getClientRects()`: a new list each call: the boxes of the elements it
/// holds whole, and of the parts of its texts in it (a collapsed range in
/// a text has one of no width); empty for a range not in a document.
pub(crate) fn get_client_rects<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<Gc<'gc, dom::DomRectList>, ApiError> {
    Ok(cx.new_rect_list(&client_rects(cx, this)))
}

/// `getBoundingClientRect()`: a new rect each call around
/// `getClientRects()`'s boxes (the union of those not empty, else the
/// first, else all zero).
pub(crate) fn get_bounding_client_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Range>,
) -> Result<Gc<'gc, dom::DomRect>, ApiError> {
    Ok(cx.new_rect(crate::geometry::bounding_rect(&client_rects(cx, this))))
}
