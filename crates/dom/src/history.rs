//! A window's session history as script sees it (HTML's `History`,
//! Blink's `History`): the classic history API's state of its document's
//! entry (serialized by the engine; its deserialization the engine's,
//! made once and kept), its scroll restoration mode; where the page's
//! session history is, as its host says (the browser keeps it: Chromium's
//! `NavigationController`); the `popstate` and `hashchange` events' data;
//! the document's target element (`:target`), which a fragment sets.

use std::cell::Cell;

use elise::{Gc, GcObject, JsEdge, LocalExternal, Member};

use crate::{AttributeChanges, Cx, DomString, Event, NodeRef, Window, sites};

/// A window's `History`: its document's entry's state.
#[derive(GcObject)]
pub struct History {
    window: Member<Window>,
    /// The state serialized (HTML's classic history API state); none is
    /// `null`. Taken and put back.
    serialized: LocalExternal<Cell<Option<Vec<u8>>>>,
    /// `serialized` deserialized (`history.state`), the engine's value,
    /// which only it reads and writes; `fresh` once it is the current
    /// state's (a new state makes it stale: the engine deserializes it at
    /// the next ask, the same object each time after, as Blink's).
    state: JsEdge,
    fresh: Cell<bool>,
    /// Its scroll restoration mode is `manual`.
    manual_scroll_restoration: Cell<bool>,
    wrapper: JsEdge,
}

/// A `PopStateEvent`'s data: its `state`, the engine's value (the
/// history's state when the UA fires one).
#[derive(GcObject)]
pub struct PopStateEventData {
    state: JsEdge,
    has_ua_visual_transition: Cell<bool>,
}

/// A `HashChangeEvent`'s data: the URLs before and after.
#[derive(GcObject)]
pub struct HashChangeEventData {
    old_url: LocalExternal<DomString>,
    new_url: LocalExternal<DomString>,
}

/// The page's session history as its host gives it: the current entry's
/// index, and how many entries there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionHistory {
    pub index: u32,
    pub length: u32,
}

impl Default for SessionHistory {
    fn default() -> Self {
        Self {
            index: 0,
            length: 1,
        }
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// `window`'s `History`, made at the first ask (the same after).
    pub fn window_history(&self, window: Gc<'gc, Window>) -> Gc<'gc, History> {
        if let Some(history) = window.history.get(self.gc) {
            return history;
        }
        let history = self.gc.alloc_in(
            &sites::OTHER,
            HistoryInit {
                window: Some(window),
                serialized: LocalExternal::new(Cell::new(None)),
                state: (),
                fresh: Cell::new(false),
                manual_scroll_restoration: Cell::new(false),
                wrapper: (),
            },
        );
        window.history.set(self.gc, Some(history));
        history
    }

    pub fn history_window(&self, history: Gc<'gc, History>) -> Gc<'gc, Window> {
        history.window.get(self.gc).expect("a history's window")
    }

    /// `history`'s state serialized; none is `null`.
    pub fn history_serialized_state(&self, history: Gc<'gc, History>) -> Option<Vec<u8>> {
        let cell = history.into_ref().serialized.get();
        let state = cell.take();
        cell.set(state.clone());
        state
    }

    /// `history`'s state is `serialized` now (`null` for none): its
    /// deserialization is stale.
    pub fn set_history_state(&self, history: Gc<'gc, History>, serialized: Option<Vec<u8>>) {
        history.into_ref().serialized.get().set(serialized);
        history.fresh.set(false);
    }

    /// `window`'s history's state is `serialized` (none: `null`), if it has
    /// a `History` yet (one made later starts with `null`, as the new
    /// entry's state of a fragment's navigation is).
    pub fn set_window_history_state(&self, window: Gc<'gc, Window>, serialized: Option<Vec<u8>>) {
        if serialized.is_some() || window.history.get(self.gc).is_some() {
            let history = self.window_history(window);
            self.set_history_state(history, serialized);
        }
    }

    /// The engine's reference to `history`'s deserialized state, and
    /// whether it holds the current state's.
    pub fn history_state_edge(&self, history: Gc<'gc, History>) -> (&'gc JsEdge, bool) {
        (&history.into_ref().state, history.fresh.get())
    }

    /// The engine deserialized `history`'s state into its edge.
    pub fn set_history_state_fresh(&self, history: Gc<'gc, History>) {
        history.fresh.set(true);
    }

    pub fn history_manual_scroll_restoration(&self, history: Gc<'gc, History>) -> bool {
        history.manual_scroll_restoration.get()
    }

    pub fn set_history_manual_scroll_restoration(&self, history: Gc<'gc, History>, manual: bool) {
        history.manual_scroll_restoration.set(manual);
    }

    pub(crate) fn history_wrapper_edge(&self, history: Gc<'gc, History>) -> &'gc JsEdge {
        &history.into_ref().wrapper
    }

    /// The page's session history, as its host last gave it.
    pub fn session_history(&self) -> SessionHistory {
        self.changes.session_history.get()
    }

    /// The browser's: `index` of `length` (at least the entries to it).
    pub fn set_session_history(&self, history: SessionHistory) {
        self.changes.session_history.set(SessionHistory {
            index: history.index,
            length: history.length.max(history.index.saturating_add(1)),
        });
    }

    /// The top-level document added an entry after the current one
    /// (`pushState`, a fragment's navigation), which the browser hears of
    /// after: the current one now, those forward of it gone (Blink's
    /// renderer counts it as well).
    pub fn push_session_history_entry(&self) {
        let index = self.session_history().index.saturating_add(1);
        self.changes.session_history.set(SessionHistory {
            index,
            length: index.saturating_add(1),
        });
    }

    /// Makes `event` a `PopStateEvent` (its `state` the engine writes).
    pub fn set_pop_state_event_data(&self, event: Gc<'gc, Event>, has_ua_visual_transition: bool) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            PopStateEventDataInit {
                state: (),
                has_ua_visual_transition: Cell::new(has_ua_visual_transition),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s state edge and `hasUAVisualTransition`, if it is a
    /// `PopStateEvent`.
    pub fn pop_state_event_data(&self, event: Gc<'gc, Event>) -> Option<(&'gc JsEdge, bool)> {
        let data = event.data.get(self.gc)?.downcast::<PopStateEventData>()?;
        Some((&data.into_ref().state, data.has_ua_visual_transition.get()))
    }

    /// Makes `event` a `HashChangeEvent` from `old_url` to `new_url`.
    pub fn set_hash_change_event_data(
        &self,
        event: Gc<'gc, Event>,
        old_url: DomString,
        new_url: DomString,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            HashChangeEventDataInit {
                old_url: LocalExternal::new(old_url),
                new_url: LocalExternal::new(new_url),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s `oldURL` and `newURL`, if it is a `HashChangeEvent`.
    pub fn hash_change_event_data(
        &self,
        event: Gc<'gc, Event>,
    ) -> Option<(&'gc DomString, &'gc DomString)> {
        let data = event.data.get(self.gc)?.downcast::<HashChangeEventData>()?;
        let data = data.into_ref();
        Some((data.old_url.get(), data.new_url.get()))
    }

    /// HTML's queue of a `hashchange` from `old_url` to `new_url` at
    /// `window`, a task of its own (`ElementTaskKind::HashChange`, its
    /// element the window's document).
    pub fn queue_hash_change(
        &self,
        window: Gc<'gc, Window>,
        old_url: DomString,
        new_url: DomString,
    ) {
        let cell = window.into_ref().hash_changes.get();
        let mut queued = cell.take();
        queued.push((old_url, new_url));
        cell.set(queued);
        let document = self.window_document(window);
        self.push_element_task(
            window,
            document,
            crate::names::event::HASHCHANGE,
            crate::window::HASH_CHANGE,
            0,
        );
    }

    /// The first queued `hashchange`'s URLs (its task runs).
    pub(crate) fn take_hash_change(
        &self,
        window: Gc<'gc, Window>,
    ) -> Option<(DomString, DomString)> {
        let cell = window.into_ref().hash_changes.get();
        let mut queued = cell.take();
        let first = (!queued.is_empty()).then(|| queued.remove(0));
        cell.set(queued);
        first
    }

    /// Whether `document` is completely loaded (HTML's: its `load` event
    /// fired; one never loaded isn't).
    pub fn is_completely_loaded(&self, document: NodeRef<'gc>) -> bool {
        self.document_data(document)
            .is_some_and(|data| data.completely_loaded.get())
    }

    pub fn set_completely_loaded(&self, document: NodeRef<'gc>) {
        self.ensure_document_data(document)
            .completely_loaded
            .set(true);
    }

    /// `document`'s target element (HTML's; `:target`), if it has one.
    pub fn target_element(&self, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.document_data(document)?.target_element.get(self.gc)
    }

    /// Whether `element` is its document's target element.
    pub fn is_target(&self, element: NodeRef<'gc>) -> bool {
        self.target_element(self.document_of(element))
            .is_some_and(|target| Gc::ptr_eq(target, element))
    }

    /// `document`'s target element is `element` now (none: no fragment, or
    /// nothing it names): the one before and it match `:target` again.
    pub fn set_target_element(&self, document: NodeRef<'gc>, element: Option<NodeRef<'gc>>) {
        let data = self.ensure_document_data(document);
        let old = data.target_element.get(self.gc);
        let same = match (old, element) {
            (Some(old), Some(new)) => Gc::ptr_eq(old, new),
            (old, new) => old.is_none() && new.is_none(),
        };
        if same {
            return;
        }
        data.target_element.set(self.gc, element);
        for changed in [old, element].into_iter().flatten() {
            self.state_changed(changed, AttributeChanges::TARGET);
        }
    }
}
