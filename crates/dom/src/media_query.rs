//! `MediaQueryList` (CSSOM View's `matchMedia()`): an event target of its
//! own kind (Blink's `MediaQueryList`, an `EventTarget`), whose data is on
//! the target (as a message port's): the document whose environment it
//! evaluates in, its queries and the result it last had. `dom_api`
//! evaluates them; its rendering update step fires `change` at those whose
//! result changed since (Blink's `MediaQueryMatcher::MediaFeaturesChanged`).
//!
//! A document keeps its lists weakly, to evaluate them in the order they
//! were made (Blink's `MediaQueryMatcher::media_lists_`, a set of weak
//! members). One with a listener is kept strongly too, so that script may
//! let go of it and still hear `change` (Blink's `MediaQueryList` has
//! pending activity while it has listeners): from its first listener, and
//! until an evaluation finds it has none.

use std::cell::Cell;
use std::sync::Arc;

use css::media::{MediaEnvironment, MediaQueryList as Queries};
use elise::{External, Gc, GcObject, HeapVector, Member, WeakMember};

use crate::{
    Cx, DomString, Event, EventTargetImpl, LocalExternal, Node, NodeRef, TargetRef, sites,
};

/// A `MediaQueryList`'s data: its document, its queries, the result it
/// last had (Blink's `matches_`), and whether its document keeps it
/// strongly.
#[derive(GcObject)]
pub struct MediaQueryListData {
    document: Member<Node>,
    queries: External<Arc<Queries>>,
    matches: Cell<bool>,
    kept: Cell<bool>,
}

/// A `MediaQueryListEvent`'s data: the queries serialized and whether
/// they matched.
#[derive(GcObject)]
pub struct MediaQueryListEventData {
    media: LocalExternal<DomString>,
    matches: Cell<bool>,
}

/// A list held weakly, on its document's list.
#[derive(GcObject)]
pub struct MediaQueryListRef {
    list: WeakMember<EventTargetImpl>,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new `MediaQueryList` of `document` (`matchMedia`'s), with
    /// `queries` and their result now, last on the document's lists.
    pub fn create_media_query_list(
        &self,
        document: NodeRef<'gc>,
        queries: Arc<Queries>,
        matches: bool,
    ) -> Gc<'gc, EventTargetImpl> {
        let target = self.create_event_target();
        let data = self.gc.alloc_in(
            &sites::OTHER,
            MediaQueryListDataInit {
                document: Some(document),
                queries: External::new(queries),
                matches: Cell::new(matches),
                kept: Cell::new(false),
            },
        );
        target.media_query.set(self.gc, Some(data));
        let entry = self
            .gc
            .alloc_in(&sites::OTHER, MediaQueryListRefInit { list: Some(target) });
        let lists = &self
            .ensure_document_data(document)
            .into_ref()
            .media_query_lists;
        let list = match lists.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                lists.set(self.gc, Some(list));
                list
            }
        };
        crate::intersection::prune(self, list, |entry: Gc<'gc, MediaQueryListRef>| {
            entry.list.get(self.gc).is_some()
        });
        list.push(self.gc, entry);
        target
    }

    fn media_query_data(
        &self,
        target: Gc<'gc, EventTargetImpl>,
    ) -> Option<Gc<'gc, MediaQueryListData>> {
        target.media_query.get(self.gc)
    }

    /// Whether `target` is a `MediaQueryList`.
    pub fn is_media_query_list(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        self.media_query_data(target).is_some()
    }

    fn list_data(&self, list: Gc<'gc, EventTargetImpl>) -> Gc<'gc, MediaQueryListData> {
        self.media_query_data(list).expect("a MediaQueryList")
    }

    /// `list`'s document.
    pub fn media_query_list_document(&self, list: Gc<'gc, EventTargetImpl>) -> NodeRef<'gc> {
        self.list_data(list)
            .document
            .get(self.gc)
            .expect("a list's document")
    }

    /// `list`'s queries.
    pub fn media_query_list_queries(&self, list: Gc<'gc, EventTargetImpl>) -> &'gc Arc<Queries> {
        self.list_data(list).into_ref().queries.get()
    }

    /// The result `list` last had, which is `matches` now: whether it
    /// differs.
    pub fn set_media_query_list_matches(
        &self,
        list: Gc<'gc, EventTargetImpl>,
        matches: bool,
    ) -> bool {
        self.list_data(list).matches.replace(matches) != matches
    }

    /// A listener came to `target` (an `EventTargetImpl`: no node's, no
    /// window's): a `MediaQueryList` is kept by its document from now on,
    /// till an evaluation finds it has none.
    #[cold]
    #[inline(never)]
    pub(crate) fn media_query_list_listened(&self, target: Gc<'gc, EventTargetImpl>) {
        let Some(data) = self.media_query_data(target) else {
            return;
        };
        if data.kept.replace(true) {
            return;
        }
        let document = data.document.get(self.gc).expect("a list's document");
        let kept = &self
            .ensure_document_data(document)
            .into_ref()
            .kept_media_query_lists;
        let list = match kept.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                kept.set(self.gc, Some(list));
                list
            }
        };
        list.push(self.gc, target);
    }

    /// Whether `document` has had a `MediaQueryList` (none: its rendering
    /// update's step has nothing to evaluate).
    pub fn has_media_query_lists(&self, document: NodeRef<'gc>) -> bool {
        self.document_data(document)
            .is_some_and(|data| data.into_ref().media_query_lists.get(self.gc).is_some())
    }

    /// `document`'s lists alive, in the order they were made, if its
    /// environment is another than when they were last evaluated
    /// (`environment`): their results are of it alone. Those kept that have
    /// no listener now are no longer kept.
    pub fn media_query_lists_to_evaluate(
        &self,
        document: NodeRef<'gc>,
        environment: MediaEnvironment,
    ) -> Vec<Gc<'gc, EventTargetImpl>> {
        let Some(data) = self.document_data(document) else {
            return Vec::new();
        };
        let data = data.into_ref();
        if let Some(kept) = data.kept_media_query_lists.get(self.gc) {
            let mut index = 0;
            while let Some(list) = kept.get(self.gc, index) {
                if self
                    .listeners(TargetRef::EventTarget(list))
                    .is_some_and(|listeners| !listeners.is_empty())
                {
                    index = index.saturating_add(1);
                } else {
                    kept.remove(self.gc, index);
                    self.list_data(list).kept.set(false);
                }
            }
        }
        if data.media_environment.get().replace(Some(environment)) == Some(environment) {
            return Vec::new();
        }
        let Some(lists) = data.media_query_lists.get(self.gc) else {
            return Vec::new();
        };
        let mut alive = Vec::new();
        let mut index = 0;
        while let Some(entry) = lists.get(self.gc, index) {
            match entry.list.get(self.gc) {
                Some(list) => {
                    alive.push(list);
                    index = index.saturating_add(1);
                }
                None => {
                    lists.remove(self.gc, index);
                }
            }
        }
        alive
    }

    /// Makes `event` a `MediaQueryListEvent` of `media` and `matches`.
    pub fn set_media_query_list_event_data(
        &self,
        event: Gc<'gc, Event>,
        media: &str,
        matches: bool,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            MediaQueryListEventDataInit {
                media: LocalExternal::new(DomString::from(media)),
                matches: Cell::new(matches),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s `media` and `matches`, if it is a `MediaQueryListEvent`.
    pub fn media_query_list_event_data(
        &self,
        event: Gc<'gc, Event>,
    ) -> Option<(&'gc DomString, bool)> {
        let data = event
            .data
            .get(self.gc)?
            .downcast::<MediaQueryListEventData>()?;
        let data = data.into_ref();
        Some((data.media.get(), data.matches.get()))
    }
}
