//! DOM's ranges (https://dom.spec.whatwg.org/#ranges; Blink's
//! `AbstractRange`, `Range` and `StaticRange`): two boundary points, each a
//! node and an offset into it (UTF-16 code units in character data,
//! children otherwise). One type for both, as Blink's `AbstractRange` is
//! their base: a static range is set once and never adjusted; a live one
//! (`Range`) is on the weak list of the document of its start
//! (`DocumentData::live_ranges`, Blink's `Document::ranges_`), which the
//! tree's changes adjust as DOM says: inserting and removing a node,
//! replacing data, splitting a text, normalizing. Setting a boundary point
//! in another document, or adopting the nodes it is in, moves it to that
//! document's list. Until the first live range or node iterator
//! (`form::LIVE_SEEN`), a change looks at nothing more. The algorithms on
//! boundary points (comparing them, setting one with DOM's checks) are
//! `dom_api`'s.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, Member, WeakMember};

use crate::{Cx, Node, NodeKind, NodeRef, sites};

/// A `Range` or a `StaticRange`.
#[derive(GcObject)]
pub struct Range {
    /// Its start's and its end's nodes, which it keeps.
    start: Member<Node>,
    end: Member<Node>,
    start_offset: Cell<u32>,
    end_offset: Cell<u32>,
    /// A `StaticRange`: on no list, never set nor adjusted.
    is_static: Cell<bool>,
    /// The selection whose range it is, if it is one's (`selection`).
    pub(crate) selection: Member<crate::Selection>,
    wrapper: JsEdge,
}

/// A live range held weakly, on its document's list.
#[derive(GcObject)]
pub struct LiveRangeRef {
    range: WeakMember<Range>,
}

/// A boundary point: a node and an offset into it.
pub type BoundaryPoint<'gc> = (NodeRef<'gc>, u32);

impl<'gc> Cx<'_, 'gc> {
    /// A new `Range` from `start` to `end` (`createRange()`, `new
    /// Range()`, `cloneRange()`), on its document's list; the caller checked
    /// the points.
    pub fn create_range(
        &self,
        start: BoundaryPoint<'gc>,
        end: BoundaryPoint<'gc>,
    ) -> Gc<'gc, Range> {
        let range = self.make_range(start, end, false);
        self.add_live_range(self.document_of(start.0), range);
        range
    }

    /// A new `StaticRange` from `start` to `end`, taken as they are (DOM
    /// checks only their kinds).
    pub fn create_static_range(
        &self,
        start: BoundaryPoint<'gc>,
        end: BoundaryPoint<'gc>,
    ) -> Gc<'gc, Range> {
        self.make_range(start, end, true)
    }

    fn make_range(
        &self,
        start: BoundaryPoint<'gc>,
        end: BoundaryPoint<'gc>,
        is_static: bool,
    ) -> Gc<'gc, Range> {
        self.gc.alloc_in(
            &sites::OTHER,
            RangeInit {
                start: Some(start.0),
                end: Some(end.0),
                start_offset: Cell::new(start.1),
                end_offset: Cell::new(end.1),
                is_static: Cell::new(is_static),
                selection: None,
                wrapper: (),
            },
        )
    }

    /// Whether `range` is a `StaticRange`.
    pub fn is_static_range(&self, range: Gc<'gc, Range>) -> bool {
        range.is_static.get()
    }

    /// `range`'s start.
    pub fn range_start(&self, range: Gc<'gc, Range>) -> BoundaryPoint<'gc> {
        (
            range.start.get(self.gc).expect("a range's start"),
            range.start_offset.get(),
        )
    }

    /// `range`'s end.
    pub fn range_end(&self, range: Gc<'gc, Range>) -> BoundaryPoint<'gc> {
        (
            range.end.get(self.gc).expect("a range's end"),
            range.end_offset.get(),
        )
    }

    /// A live range's boundary points set (the caller checked them, and
    /// that they are in one tree), and the range moved to the list of its
    /// new start's document if that is another.
    pub fn set_range(
        &self,
        range: Gc<'gc, Range>,
        start: BoundaryPoint<'gc>,
        end: BoundaryPoint<'gc>,
    ) {
        let old = self.document_of(self.range_start(range).0);
        self.set_start(range, start);
        self.set_end(range, end);
        let new = self.document_of(start.0);
        if !Gc::ptr_eq(old, new) {
            self.remove_live_range(old, range);
            self.add_live_range(new, range);
        }
    }

    fn set_start(&self, range: Gc<'gc, Range>, (node, offset): BoundaryPoint<'gc>) {
        range.start.set(self.gc, Some(node));
        range.start_offset.set(offset);
    }

    fn set_end(&self, range: Gc<'gc, Range>, (node, offset): BoundaryPoint<'gc>) {
        range.end.set(self.gc, Some(node));
        range.end_offset.set(offset);
    }

    pub(crate) fn range_wrapper_edge(&self, range: Gc<'gc, Range>) -> &'gc JsEdge {
        &range.into_ref().wrapper
    }

    fn add_live_range(&self, document: NodeRef<'gc>, range: Gc<'gc, Range>) {
        self.see(crate::form::LIVE_SEEN);
        let data = self.ensure_document_data(document);
        let list = match data.live_ranges.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                data.live_ranges.set(self.gc, Some(list));
                list
            }
        };
        crate::intersection::prune(self, list, |entry: Gc<'gc, LiveRangeRef>| {
            entry.range.get(self.gc).is_some()
        });
        let entry = self
            .gc
            .alloc_in(&sites::OTHER, LiveRangeRefInit { range: Some(range) });
        list.push(self.gc, entry);
    }

    fn remove_live_range(&self, document: NodeRef<'gc>, range: Gc<'gc, Range>) {
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.live_ranges.get(self.gc))
        else {
            return;
        };
        let at = (0..list.len()).find(|&index| {
            list.get(self.gc, index)
                .and_then(|entry| entry.range.get(self.gc))
                .is_some_and(|live| Gc::ptr_eq(live, range))
        });
        if let Some(index) = at {
            list.remove(self.gc, index);
        }
    }

    /// `f` of each live range on `document`'s list.
    fn for_each_live_range(&self, document: NodeRef<'gc>, mut f: impl FnMut(Gc<'gc, Range>)) {
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.live_ranges.get(self.gc))
        else {
            return;
        };
        for index in 0..list.len() {
            if let Some(range) = list
                .get(self.gc, index)
                .and_then(|entry| entry.range.get(self.gc))
            {
                f(range);
            }
        }
    }

    /// Whether a live range or a node iterator was ever made: what the
    /// tree's changes look at before they adjust any.
    #[inline]
    pub(crate) fn has_seen_live(&self) -> bool {
        self.has_seen(crate::form::LIVE_SEEN)
    }

    /// DOM's "insert", its steps for live ranges: a node is to go into
    /// `parent`'s children before `child`, so the offsets in `parent` past
    /// `child`'s index grow by one (a fragment's children go one at a
    /// time: the same as growing by their count at once).
    #[cold]
    #[inline(never)]
    pub(crate) fn live_ranges_inserting(&self, parent: NodeRef<'gc>, child: NodeRef<'gc>) {
        let mut index = None;
        self.for_each_live_range(self.document_of(parent), |range| {
            for (node, offset) in [
                (&range.start, &range.start_offset),
                (&range.end, &range.end_offset),
            ] {
                if node
                    .get(self.gc)
                    .is_some_and(|node| Gc::ptr_eq(node, parent))
                    && offset.get() > *index.get_or_insert_with(|| self.index(child))
                {
                    offset.set(offset.get().saturating_add(1));
                }
            }
        });
    }

    /// DOM's "remove", its steps for live ranges and node iterators before
    /// `node` leaves `parent`: a point in `node` moves to where it was in
    /// `parent`, an offset in `parent` past it shrinks by one; each node
    /// iterator's pre-removing steps (`traversal`).
    #[cold]
    #[inline(never)]
    pub(crate) fn live_removing(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        let document = self.document_of(node);
        let index = self.index(node);
        self.for_each_live_range(document, |range| {
            for (point, offset) in [
                (&range.start, &range.start_offset),
                (&range.end, &range.end_offset),
            ] {
                let at = point.get(self.gc).expect("a range's boundary point");
                if self.is_inclusive_ancestor(node, at) {
                    point.set(self.gc, Some(parent));
                    offset.set(index);
                } else if Gc::ptr_eq(at, parent) && offset.get() > index {
                    offset.set(offset.get().saturating_sub(1));
                }
            }
        });
        self.node_iterators_removing(document, node);
    }

    /// DOM's "replace data", its steps for live ranges: `count` code units
    /// of `node`'s data from `offset` replaced by `added` of them (a
    /// `count` past the end is the rest: no point is past the end). A point
    /// in the part replaced moves to its start; one past it moves with the
    /// data after.
    #[cold]
    #[inline(never)]
    pub(crate) fn live_ranges_data_replaced(
        &self,
        node: NodeRef<'gc>,
        offset: u32,
        count: u32,
        added: u32,
    ) {
        // Under the length, which is a `u32`: no overflow.
        let replaced_end = offset.saturating_add(count);
        self.for_each_live_range(self.document_of(node), |range| {
            let mut moved = false;
            for (point, at) in [
                (&range.start, &range.start_offset),
                (&range.end, &range.end_offset),
            ] {
                if !point
                    .get(self.gc)
                    .is_some_and(|point| Gc::ptr_eq(point, node))
                {
                    continue;
                }
                let before = at.get();
                if before > offset && before <= replaced_end {
                    at.set(offset);
                } else if before > replaced_end {
                    // Past the part replaced: more than `count`, which
                    // doesn't underflow. A length past `u32::MAX`
                    // saturates (the DOM's offsets are `unsigned long`s).
                    at.set(before.saturating_sub(count).saturating_add(added));
                }
                moved |= at.get() != before;
            }
            if moved {
                self.selection_range_moved(range);
            }
        });
    }

    /// `splitText`, its steps for live ranges once `new` (the data from
    /// `offset` on) is in `parent` after `node`: a point in `node` past
    /// `offset` moves into `new`; one in `parent` right after `node` moves
    /// past `new`.
    #[cold]
    #[inline(never)]
    fn live_ranges_split(
        &self,
        node: NodeRef<'gc>,
        offset: u32,
        parent: NodeRef<'gc>,
        new: NodeRef<'gc>,
    ) {
        let after = self.index(node).saturating_add(1);
        self.for_each_live_range(self.document_of(node), |range| {
            for (point, at) in [
                (&range.start, &range.start_offset),
                (&range.end, &range.end_offset),
            ] {
                let Some(container) = point.get(self.gc) else {
                    continue;
                };
                if Gc::ptr_eq(container, node) && at.get() > offset {
                    point.set(self.gc, Some(new));
                    // Past `offset`: no underflow.
                    at.set(at.get().saturating_sub(offset));
                    self.selection_range_moved(range);
                }
            }
            for (point, at) in [
                (&range.start, &range.start_offset),
                (&range.end, &range.end_offset),
            ] {
                if point
                    .get(self.gc)
                    .is_some_and(|point| Gc::ptr_eq(point, parent))
                    && at.get() == after
                {
                    at.set(after.saturating_add(1));
                }
            }
        });
    }

    /// `normalize()`, its steps for live ranges as `current`, a text after
    /// `node` (`length` code units of data before it), is joined into
    /// `node`: a point in `current` moves into `node`, past `length`; one
    /// in their parent right before `current` moves to `length` in `node`.
    #[cold]
    #[inline(never)]
    pub(crate) fn live_ranges_merged(
        &self,
        node: NodeRef<'gc>,
        current: NodeRef<'gc>,
        length: u32,
    ) {
        let parent = self.parent(current);
        let mut index = None;
        self.for_each_live_range(self.document_of(node), |range| {
            for (point, at) in [
                (&range.start, &range.start_offset),
                (&range.end, &range.end_offset),
            ] {
                let container = point.get(self.gc).expect("a range's boundary point");
                if Gc::ptr_eq(container, current) {
                    point.set(self.gc, Some(node));
                    at.set(at.get().saturating_add(length));
                    self.selection_range_moved(range);
                } else if parent.is_some_and(|parent| Gc::ptr_eq(container, parent))
                    && at.get() == *index.get_or_insert_with(|| self.index(current))
                {
                    point.set(self.gc, Some(node));
                    at.set(length);
                    self.selection_range_moved(range);
                }
            }
        });
    }

    /// DOM's "adopt", for the live ranges and node iterators of `old`: those
    /// whose start (an iterator's root) is now in `document` move to its
    /// list.
    #[cold]
    #[inline(never)]
    pub(crate) fn live_adopted(&self, old: NodeRef<'gc>, document: NodeRef<'gc>) {
        let mut moved = Vec::new();
        self.for_each_live_range(old, |range| {
            if Gc::ptr_eq(self.document_of(self.range_start(range).0), document) {
                moved.push(range);
            }
        });
        for range in moved {
            self.remove_live_range(old, range);
            self.add_live_range(document, range);
        }
        self.move_node_iterators(old, document);
    }

    /// `splitText(offset)`: the data from `offset` (at most the length:
    /// the caller checked) moved into a new text of the same type and
    /// document, put after `node` in its parent if it has one, the live
    /// ranges moved with it.
    ///
    /// # Panics
    ///
    /// If `node` isn't a text.
    pub fn split_text(&self, node: NodeRef<'gc>, offset: u32) -> NodeRef<'gc> {
        let at = usize::try_from(offset).expect("a `u32` fits a `usize`");
        let rest = self
            .text_data(node)
            .expect("a text's data")
            .borrow()
            .utf16_substring(at, usize::MAX);
        // Of its own type (a CDATA section's is one: Blink's `CloneWithData`).
        let new = if self.is_cdata_section(node) {
            self.create_cdata_section(self.document_of(node), &rest)
        } else {
            self.create_text(self.document_of(node), &rest)
        };
        if let Some(parent) = self.parent(node) {
            self.insert(parent, new, self.next_sibling(node));
            if self.has_seen_live() {
                self.live_ranges_split(node, offset, parent, new);
            }
        }
        self.replace_data(node, at, usize::MAX, "");
        new
    }

    /// DOM's "index": how many siblings precede `node` (a `u32`: no node
    /// has more children than memory holds `u32`s of).
    pub fn index(&self, node: NodeRef<'gc>) -> u32 {
        let mut index = 0u32;
        let mut at = self.previous_sibling(node);
        while let Some(sibling) = at {
            index = index.saturating_add(1);
            at = self.previous_sibling(sibling);
        }
        index
    }

    /// DOM's "length" of a node: a doctype's and an attribute's 0, character
    /// data's in UTF-16 code units, another's its children.
    pub fn node_length(&self, node: NodeRef<'gc>) -> u32 {
        match self.kind(node) {
            NodeKind::DocumentType | NodeKind::Attr => 0,
            NodeKind::Text | NodeKind::Comment | NodeKind::ProcessingInstruction => {
                let length = self
                    .text_data(node)
                    .expect("character data's data")
                    .borrow()
                    .utf16_len();
                u32::try_from(length).unwrap_or(u32::MAX)
            }
            NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element => {
                let mut count = 0u32;
                let mut at = self.first_child(node);
                while let Some(child) = at {
                    count = count.saturating_add(1);
                    at = self.next_sibling(child);
                }
                count
            }
        }
    }

    /// Whether `ancestor` is `node` or one of its ancestors (in its tree,
    /// not through a shadow root's host).
    pub fn is_inclusive_ancestor(&self, ancestor: NodeRef<'gc>, node: NodeRef<'gc>) -> bool {
        let mut at = Some(node);
        while let Some(current) = at {
            if Gc::ptr_eq(current, ancestor) {
                return true;
            }
            at = self.parent(current);
        }
        false
    }
}
