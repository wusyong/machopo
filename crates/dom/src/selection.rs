//! The Selection API's `Selection` (Blink's `DOMSelection`, over its
//! frame's `FrameSelection`): a document's selection is at most one live
//! `Range` and whether it was made backward. The range is an ordinary
//! live range (`range`), on its document's list, so the tree's changes
//! move the selection with it; `getRangeAt` hands script that very range,
//! which knows it is the selection's (`Range::selection`) so that script
//! setting it sets the selection. A document makes its selection the first
//! time it is asked for (`DocumentData::selection`). The algorithms (the
//! checks, which range replaces which, the direction) are
//! `dom_api`'s.
//!
//! A change to a selection schedules `selectionchange`, once until its
//! task runs (the Selection API's "has scheduled selectionchange event"):
//! at the document for its selection (`dom_api` when its methods
//! change it; the character data changes when they move its range, as
//! Chrome's, not insertions and removals), at a text control for its own
//! (`text_control`). The event is an element task of the target's window.

use std::cell::Cell;

use elise::{Gc, GcObject, JsEdge, Member};

use crate::{Cx, Node, NodeKind, NodeRef, Range, names, sites};

#[derive(GcObject)]
pub struct Selection {
    document: Member<Node>,
    /// Its range; none for an empty selection.
    range: Member<Range>,
    /// Whether its anchor is its range's end (made backward); meaningless
    /// while it has no range.
    backward: Cell<bool>,
    /// Whether `selectionchange` is scheduled at its document, its task
    /// not run yet.
    change_scheduled: Cell<bool>,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// `document`'s selection, made the first time.
    pub fn document_selection(&self, document: NodeRef<'gc>) -> Gc<'gc, Selection> {
        let data = self.ensure_document_data(document);
        if let Some(selection) = data.selection.get(self.gc) {
            return selection;
        }
        let selection = self.gc.alloc_in(
            &sites::OTHER,
            SelectionInit {
                document: Some(document),
                range: None,
                backward: Cell::new(false),
                change_scheduled: Cell::new(false),
                wrapper: (),
            },
        );
        data.selection.set(self.gc, Some(selection));
        selection
    }

    /// `document`'s selection's range, if it was made and has one (the
    /// renderer's: what it draws as selected).
    pub fn document_selection_range(&self, document: NodeRef<'gc>) -> Option<Gc<'gc, Range>> {
        self.document_data(document)?
            .selection
            .get(self.gc)?
            .range
            .get(self.gc)
    }

    pub fn selection_document(&self, selection: Gc<'gc, Selection>) -> NodeRef<'gc> {
        selection
            .document
            .get(self.gc)
            .expect("a selection's document")
    }

    pub fn selection_range(&self, selection: Gc<'gc, Selection>) -> Option<Gc<'gc, Range>> {
        selection.range.get(self.gc)
    }

    pub fn selection_is_backward(&self, selection: Gc<'gc, Selection>) -> bool {
        selection.backward.get()
    }

    /// `selection`'s range (none: empty) and direction set; the range it
    /// had before is no longer its. What the frame draws follows.
    pub fn set_selection(
        &self,
        selection: Gc<'gc, Selection>,
        range: Option<Gc<'gc, Range>>,
        backward: bool,
    ) {
        if let Some(old) = selection.range.get(self.gc) {
            old.selection.set(self.gc, None);
        }
        if let Some(range) = range {
            range.selection.set(self.gc, Some(selection));
        }
        selection.range.set(self.gc, range);
        selection.backward.set(backward);
        self.tree_changed();
    }

    /// The selection whose range `range` is, if it is one's.
    pub fn range_selection(&self, range: Gc<'gc, Range>) -> Option<Gc<'gc, Selection>> {
        range.selection.get(self.gc)
    }

    pub(crate) fn selection_wrapper_edge(&self, selection: Gc<'gc, Selection>) -> &'gc JsEdge {
        &selection.into_ref().wrapper
    }

    /// `selectionchange` scheduled at `target` (a document, for its
    /// selection, or a text control, for its own), unless it is already: a
    /// task of its window's (none without one).
    pub fn schedule_selectionchange(&self, target: NodeRef<'gc>) {
        let Some(scheduled) = self.selectionchange_cell(target) else {
            return;
        };
        if scheduled.replace(true) {
            return;
        }
        self.queue_element_task(target, names::event::SELECTIONCHANGE);
    }

    /// `selectionchange`'s task ran at `target`: another change schedules
    /// it again.
    pub fn selectionchange_fired(&self, target: NodeRef<'gc>) {
        if let Some(scheduled) = self.selectionchange_cell(target) {
            scheduled.set(false);
        }
    }

    fn selectionchange_cell(&self, target: NodeRef<'gc>) -> Option<&'gc Cell<bool>> {
        if self.kind(target) == NodeKind::Document {
            let selection = self.document_data(target)?.selection.get(self.gc)?;
            return Some(&selection.into_ref().change_scheduled);
        }
        self.text_control_selectionchange_cell(target)
    }

    /// A character data change moved `range`: if it is a selection's,
    /// `selectionchange` at its document (Chrome's: not for insertions and
    /// removals).
    pub(crate) fn selection_range_moved(&self, range: Gc<'gc, Range>) {
        if let Some(selection) = range.selection.get(self.gc) {
            self.schedule_selectionchange(self.selection_document(selection));
        }
    }
}
