//! Who asked for the loads the DOM's documents start, as DevTools names
//! them (Blink's `FetchInitiatorInfo`, which `InspectorNetworkAgent`'s
//! `BuildInitiatorObject` makes into a `Network.Initiator`): what each fetch
//! the host takes says (`LoadInitiator`), and the stacks of the script that
//! asked, while the host wants them (`Dom::set_stacks_wanted`).
//!
//! Script asks for most loads in an operation, where the engine can't
//! capture its stack (that allocates): the DOM notes who asked
//! (`Cx::asked_for_load`: an element whose load it changed, a `fetch()`),
//! and the engine captures the stack once the operation is over
//! (`Microtasks::stacks`, `Dom::note_request_stacks`). The host takes each
//! as the load starts (`Dom::take_request_stack`) and lets go of those no
//! load took (`Dom::clear_request_stacks`). What the parser asks for is
//! the parser's, not the stack of the script that runs next.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::{Cx, Dom, Microtasks, NodeId};

/// Who asked for a load the host is to start, as far as the DOM knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadInitiator {
    /// No one DevTools names (`other`).
    Other,
    /// An element's load: the parser's that made it, or the script's that
    /// asked (`StackAsker::Element`).
    Element(NodeId),
    /// A stylesheet's `url()`: its URL (its document's for a `<style>`'s or
    /// a `style` attribute's).
    Stylesheet(Arc<str>),
    /// A module's static `import`: the importing module's URL.
    Import(String),
    /// Script, running: its stack, as DevTools takes it
    /// (`Runtime.StackTrace`, JSON).
    Script(Rc<str>),
}

/// What asked for a load in an operation, whose stack the engine captures
/// after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackAsker {
    /// An element whose load script changed: an `<img>`'s `src`, a
    /// `<link>`'s sheet, a script element inserted, an iframe's `src`.
    Element(NodeId),
    /// A `fetch()`, by its number (`FetchRequest::id`).
    Fetch(u64),
}

/// What the host wants the stacks of, the askers noted in the operation,
/// and their stacks once captured (the first to ask's), until the host
/// takes them. `RefCell`s: reached through `&Cx` within one step, and only
/// while the host wants stacks.
#[derive(Default)]
pub(crate) struct RequestStacks {
    /// The requests' (DevTools' `Network.setAttachDebugStack`).
    requests: Cell<bool>,
    /// The frames' that script makes (`Page.frameAttached`'s), its
    /// iframes'.
    frames: Cell<bool>,
    asked: RefCell<Vec<StackAsker>>,
    stacks: RefCell<HashMap<StackAsker, Rc<str>>>,
}

impl Dom {
    /// Whether the host wants the stacks of the script that asks for loads
    /// (`requests`) and of the script whose iframe makes a frame
    /// (`frames`): the DOM notes who asks only while it does.
    pub fn set_stacks_wanted(&self, requests: bool, frames: bool) {
        let stacks = &self.changes.stacks;
        stacks.requests.set(requests);
        stacks.frames.set(frames);
        if !requests && !frames {
            stacks.asked.borrow_mut().clear();
            stacks.stacks.borrow_mut().clear();
        }
    }

    /// Whether the host wants the requests' stacks: an engine that starts
    /// a load itself (an `import()`'s) captures it then.
    pub fn request_stacks_wanted(&self) -> bool {
        self.changes.stacks.requests.get()
    }

    /// The stack of the script running since the askers were noted
    /// (`Microtasks::stacks`), as the engine captured it: each asker's,
    /// unless one asked before (none: no script runs, or none is known).
    pub fn note_request_stacks(&self, stack: Option<Rc<str>>) {
        let stacks = &self.changes.stacks;
        let asked = std::mem::take(&mut *stacks.asked.borrow_mut());
        let Some(stack) = stack else {
            return;
        };
        let mut kept = stacks.stacks.borrow_mut();
        for asker in asked {
            kept.entry(asker).or_insert_with(|| stack.clone());
        }
    }

    /// The stack of the script that asked for `asker`'s load, once.
    pub fn take_request_stack(&self, asker: StackAsker) -> Option<Rc<str>> {
        self.changes.stacks.stacks.borrow_mut().remove(&asker)
    }

    /// The stack of the script that asked for `asker`'s load, kept for the
    /// load.
    pub fn request_stack(&self, asker: StackAsker) -> Option<Rc<str>> {
        self.changes.stacks.stacks.borrow().get(&asker).cloned()
    }

    /// The loads asked for have started: the stacks no load took go (an
    /// image the list had, a `<link>` with nothing to load), so they are
    /// not a later load's.
    pub fn clear_request_stacks(&self) {
        let stacks = &self.changes.stacks;
        if !stacks.stacks.borrow().is_empty() {
            stacks.stacks.borrow_mut().clear();
        }
    }

    /// The element of the pending script numbered `key`
    /// (`Cx::queue_module_script`'s), whose module graph's root it asks
    /// for.
    pub fn pending_script_element(&self, key: u64) -> Option<NodeId> {
        self.changes.scripts.element_of(key)
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether an asker of a load is to be noted: the host wants the stacks
    /// (`frame`: an iframe's, whose frame's stack is wanted too), and it
    /// isn't the parser's.
    #[inline]
    fn notes_askers(&self, frame: bool) -> bool {
        let stacks = &self.changes.stacks;
        (stacks.requests.get() || frame && stacks.frames.get())
            && self.changes.parser_line.get() == 0
    }

    /// `asker` asked for a load in this operation: noted for the engine to
    /// capture the stack of after it, if the host wants it.
    #[inline]
    pub(crate) fn asked_for_load(&self, asker: StackAsker) {
        if self.notes_askers(false) {
            self.note_asker(asker);
        }
    }

    /// `element`'s load changed in this operation (`frame`: an iframe's),
    /// as `asked_for_load`.
    #[inline]
    pub(crate) fn element_asked_for_load(&self, element: crate::NodeRef<'gc>, frame: bool) {
        if self.notes_askers(frame) {
            self.note_asker(StackAsker::Element(self.node_id(element)));
        }
    }

    #[cold]
    #[inline(never)]
    fn note_asker(&self, asker: StackAsker) {
        self.changes.stacks.asked.borrow_mut().push(asker);
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | Microtasks::STACKS);
    }
}
