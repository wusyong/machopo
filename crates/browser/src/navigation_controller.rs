//! `content::NavigationController`: a tab's session history, the list of
//! entries back and forward go through.

use std::collections::BTreeMap;

use protocol::renderer::HistoryEntryParams;

/// `content::NavigationEntry`: one page in the history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavigationEntry {
    /// The URL committed, after redirects.
    pub url: String,
    pub title: String,
    /// The document it is an entry of: the id of the navigation that
    /// committed that document (Chromium's document sequence number).
    /// Entries a document made of itself (a fragment, `pushState`) share
    /// it; back and forward between them don't load anything.
    pub document: u64,
    /// The classic history API's state, as the renderer serialized it;
    /// `None` is `null` (part of Chromium's `PageState`).
    pub state: Option<Vec<u8>>,
    /// Its scroll restoration mode is `"manual"`.
    pub manual_scroll_restoration: bool,
    /// The tab's first document's, which the next new entry takes the
    /// place of (Chromium's `NavigationEntryImpl::IsInitialEntry`: a new
    /// tab's `about:blank` isn't something to go back to).
    pub initial: bool,
}

/// What a navigation does to the history once it commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavigationType {
    /// A new entry after the current one, dropping those forward of it: a
    /// URL typed or given, or a link followed.
    NewEntry,
    /// Back or forward to the entry at this index (Chromium's history
    /// navigation): loading it again, or, for an entry of the document
    /// shown, within the document.
    HistoryIndex(usize),
    /// The current entry, loaded again.
    Reload,
    /// A new document in the current entry's place (`location.replace`).
    Replace,
    /// The tab's first document, as an initial entry.
    Initial,
}

#[derive(Debug, Default)]
pub struct NavigationController {
    entries: Vec<NavigationEntry>,
    /// The entry shown; `None` before the first commit.
    current: Option<usize>,
    /// Navigations started and not committed yet, by id (ids grow with
    /// each one started).
    pending: BTreeMap<u64, NavigationType>,
}

impl NavigationController {
    pub fn entries(&self) -> &[NavigationEntry] {
        &self.entries
    }

    /// The index of the entry shown (`GetLastCommittedEntryIndex`).
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// Where back and forward count from (`GetCurrentEntryIndex`): the
    /// entry the latest navigation goes to if that is a history one,
    /// else the one shown. Going back twice quickly goes back two.
    fn base_index(&self) -> Option<usize> {
        match self.pending.last_key_value() {
            Some((_, NavigationType::HistoryIndex(index))) => Some(*index),
            _ => self.current,
        }
    }

    /// The entry `offset` steps from the current one, if there is one
    /// (`GetIndexForOffset`); -1 is back, 1 forward.
    pub(crate) fn index_for_offset(&self, offset: isize) -> Option<usize> {
        let index = self.base_index()?.checked_add_signed(offset)?;
        (index < self.entries.len()).then_some(index)
    }

    pub fn can_go_back(&self) -> bool {
        self.index_for_offset(-1).is_some()
    }

    pub fn can_go_forward(&self) -> bool {
        self.index_for_offset(1).is_some()
    }

    pub(crate) fn did_start(&mut self, navigation_id: u64, kind: NavigationType) {
        self.pending.insert(navigation_id, kind);
    }

    /// What the renderer is told of the entry a navigation of `kind` will
    /// commit to, as things are now: its state, and its place.
    pub(crate) fn entry_params(&self, kind: NavigationType) -> HistoryEntryParams {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let len = self.entries.len();
        let (entry, index, length) = match (kind, self.current) {
            (NavigationType::HistoryIndex(index), _) if index < len => {
                (Some(&self.entries[index]), index, len)
            }
            (NavigationType::Reload, Some(current)) => (Some(&self.entries[current]), current, len),
            (NavigationType::Replace, Some(current)) => (None, current, len),
            (NavigationType::NewEntry, Some(current)) if self.entries[current].initial => {
                (None, current, len)
            }
            (_, current) => {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "an index of the entries, a Vec's (at most isize::MAX), and one past it"
                )]
                let (index, len) = {
                    let index = current.map_or(0, |current| current + 1);
                    (index, index + 1)
                };
                (None, index, len)
            }
        };
        HistoryEntryParams {
            state: entry.and_then(|entry| entry.state.clone()),
            manual_scroll_restoration: entry.is_some_and(|entry| entry.manual_scroll_restoration),
            index: count(index),
            length: count(length),
        }
    }

    /// The current entry's index and the number of entries, for the
    /// renderer (`SetHistoryOffsetAndLength`).
    pub(crate) fn offset_and_length(&self) -> (u32, u32) {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        (count(self.current.unwrap_or(0)), count(self.entries.len()))
    }

    /// Whether the entry at `index` is one of the document shown, which a
    /// traversal reaches without loading anything.
    pub(crate) fn is_same_document(&self, index: usize) -> bool {
        match (self.current, self.entries.get(index)) {
            (Some(current), Some(entry)) => entry.document == self.entries[current].document,
            _ => false,
        }
    }

    /// The document shown: its entry's `document`.
    pub(crate) fn current_document(&self) -> Option<u64> {
        self.current.map(|current| self.entries[current].document)
    }

    /// `RendererDidNavigate`: `navigation_id` committed at `url`. The
    /// renderer drops commits older than the document it shows, so every
    /// navigation started before this one is over too.
    pub(crate) fn did_commit(&mut self, navigation_id: u64, url: String) {
        let kind = self.pending.get(&navigation_id).copied();
        // Those after this one stay: none if it is the last there can be.
        self.pending = match navigation_id.checked_add(1) {
            Some(after) => self.pending.split_off(&after),
            None => BTreeMap::new(),
        };
        // A new document: the entry it commits to is its, keeping its
        // state (a reload, a traversal).
        let new = || NavigationEntry {
            url: url.clone(),
            document: navigation_id,
            ..NavigationEntry::default()
        };
        match (kind, self.current) {
            (Some(NavigationType::HistoryIndex(index)), _) if index < self.entries.len() => {
                let entry = &mut self.entries[index];
                entry.url = url;
                entry.document = navigation_id;
                self.current = Some(index);
            }
            (Some(NavigationType::Reload), Some(current)) => {
                let entry = &mut self.entries[current];
                entry.url = url;
                entry.document = navigation_id;
            }
            (Some(NavigationType::Replace), Some(current)) => self.entries[current] = new(),
            (Some(NavigationType::NewEntry), Some(current)) if self.entries[current].initial => {
                self.entries[current] = new();
            }
            (Some(NavigationType::Initial), _) => self.push(NavigationEntry {
                initial: true,
                ..new()
            }),
            (None, _) => {
                tracing::warn!(navigation_id, %url, "a commit for no navigation the browser started");
                self.push(new());
            }
            _ => self.push(new()),
        }
    }

    /// `RendererDidNavigateToSameDocument`: the document shown navigated
    /// within itself (a fragment, `pushState`), nothing loaded: a new entry
    /// of the same document, with the current one's title and scroll
    /// restoration mode, or, if `replace`, the current one's new URL and
    /// state.
    pub(crate) fn did_commit_same_document(
        &mut self,
        url: String,
        replace: bool,
        state: Option<Vec<u8>>,
    ) {
        match self.current {
            Some(current) if replace => {
                let entry = &mut self.entries[current];
                entry.url = url;
                entry.state = state;
            }
            current => {
                let base = current.map(|current| self.entries[current].clone());
                let base = base.unwrap_or_default();
                self.push(NavigationEntry {
                    url,
                    title: base.title,
                    document: base.document,
                    state,
                    manual_scroll_restoration: base.manual_scroll_restoration,
                    initial: false,
                });
            }
        }
    }

    /// The traversal `navigation_id` within the document committed at
    /// `url` (`DidCommitSameDocumentNavigation` of a history navigation):
    /// its entry is the current one.
    pub(crate) fn did_commit_history_same_document(&mut self, navigation_id: u64, url: String) {
        match self.pending.remove(&navigation_id) {
            Some(NavigationType::HistoryIndex(index)) if index < self.entries.len() => {
                self.entries[index].url = url;
                self.current = Some(index);
            }
            other => {
                tracing::warn!(
                    navigation_id,
                    ?other,
                    "a traversal within the document for no entry"
                );
            }
        }
    }

    /// The renderer showed another document than the traversal
    /// `navigation_id` was for: the entry to load instead, if it is still
    /// there.
    pub(crate) fn did_fail_same_document(&mut self, navigation_id: u64) -> Option<usize> {
        match self.pending.remove(&navigation_id) {
            Some(NavigationType::HistoryIndex(index)) if index < self.entries.len() => Some(index),
            _ => None,
        }
    }

    /// The current entry's scroll restoration mode (`UpdateState`).
    pub(crate) fn set_manual_scroll_restoration(&mut self, manual: bool) {
        if let Some(current) = self.current {
            self.entries[current].manual_scroll_restoration = manual;
        }
    }

    /// The title of the entry shown.
    pub(crate) fn set_title(&mut self, title: String) {
        if let Some(current) = self.current {
            self.entries[current].title = title;
        }
    }

    /// A new entry after the current one; the ones forward of it go.
    fn push(&mut self, entry: NavigationEntry) {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the current entry's index, one of a Vec's: one past it fits"
        )]
        let keep = self.current.map_or(0, |current| current + 1);
        self.entries.truncate(keep);
        // A history navigation pending to an entry that just went can't
        // commit as one any more.
        for kind in self.pending.values_mut() {
            if matches!(kind, NavigationType::HistoryIndex(index) if *index >= keep) {
                *kind = NavigationType::NewEntry;
            }
        }
        let index = self.entries.len();
        self.entries.push(entry);
        self.current = Some(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(controller: &NavigationController) -> Vec<&str> {
        controller
            .entries()
            .iter()
            .map(|entry| entry.url.as_str())
            .collect()
    }

    /// `a`, `b`, `c` committed in turn, as navigations 1 to 3.
    fn three() -> NavigationController {
        let mut controller = NavigationController::default();
        for (id, url) in [(1, "a"), (2, "b"), (3, "c")] {
            controller.did_start(id, NavigationType::NewEntry);
            controller.did_commit(id, url.into());
        }
        controller
    }

    #[test]
    fn new_entries_go_after_the_current_one() {
        let mut controller = NavigationController::default();
        assert!(!controller.can_go_back() && !controller.can_go_forward());
        controller.did_start(1, NavigationType::NewEntry);
        assert_eq!(controller.current_index(), None);
        controller.did_commit(1, "a".into());
        controller.set_title("A".into());
        assert_eq!(controller.entries()[0].title, "A");
        let mut controller = three();
        assert_eq!(urls(&controller), ["a", "b", "c"]);
        assert_eq!(controller.current_index(), Some(2));
        assert!(controller.can_go_back() && !controller.can_go_forward());
        // Back to `b`, then a new page: `c` goes.
        controller.did_start(4, NavigationType::HistoryIndex(1));
        controller.did_commit(4, "b".into());
        assert!(controller.can_go_forward());
        controller.did_start(5, NavigationType::NewEntry);
        controller.did_commit(5, "d".into());
        assert_eq!(urls(&controller), ["a", "b", "d"]);
        assert!(!controller.can_go_forward());
    }

    #[test]
    fn back_and_forward_move_through_the_entries() {
        let mut controller = three();
        assert_eq!(controller.index_for_offset(-1), Some(1));
        controller.did_start(4, NavigationType::HistoryIndex(1));
        // Pending: a second back counts from where the first goes.
        assert_eq!(controller.index_for_offset(-1), Some(0));
        controller.did_start(5, NavigationType::HistoryIndex(0));
        assert_eq!(controller.index_for_offset(-1), None);
        // The later one commits; the earlier one never will.
        controller.did_commit(5, "a".into());
        assert_eq!(controller.current_index(), Some(0));
        assert_eq!(urls(&controller), ["a", "b", "c"]);
        assert_eq!(controller.index_for_offset(2), Some(2));
        // A redirect on the way changes the entry's URL.
        controller.did_start(6, NavigationType::HistoryIndex(2));
        controller.did_commit(6, "c2".into());
        assert_eq!(urls(&controller), ["a", "b", "c2"]);
        assert_eq!(controller.current_index(), Some(2));
    }

    #[test]
    fn reload_keeps_the_entry_and_same_document_adds_one() {
        let mut controller = three();
        controller.did_start(4, NavigationType::Reload);
        controller.did_commit(4, "c".into());
        assert_eq!(urls(&controller), ["a", "b", "c"]);
        controller.did_commit_same_document("c#x".into(), false, None);
        assert_eq!(urls(&controller), ["a", "b", "c", "c#x"]);
        assert_eq!(controller.current_index(), Some(3));
    }

    /// `location.replace`: the new document, or the fragment, takes the
    /// current entry's place; the ones forward of it stay. With no entry
    /// yet, it is the first.
    #[test]
    fn replace_takes_the_current_entry_s_place() {
        let mut controller = three();
        controller.did_start(4, NavigationType::HistoryIndex(1));
        controller.did_commit(4, "b".into());
        controller.set_title("B".into());
        controller.did_start(5, NavigationType::Replace);
        controller.did_commit(5, "r".into());
        assert_eq!(urls(&controller), ["a", "r", "c"]);
        assert_eq!(controller.current_index(), Some(1));
        assert_eq!(controller.entries()[1].title, "");
        controller.did_commit_same_document("r#x".into(), true, None);
        assert_eq!(urls(&controller), ["a", "r#x", "c"]);
        assert_eq!(controller.current_index(), Some(1));
        let mut empty = NavigationController::default();
        empty.did_start(1, NavigationType::Replace);
        empty.did_commit(1, "a".into());
        assert_eq!(urls(&empty), ["a"]);
    }

    /// The initial entry is there until the next new entry takes its
    /// place; another kind of navigation keeps it.
    #[test]
    fn a_new_entry_takes_the_initial_entry_s_place() {
        let mut controller = NavigationController::default();
        controller.did_start(1, NavigationType::Initial);
        assert_eq!(controller.entry_params(NavigationType::Initial).index, 0);
        controller.did_commit(1, "about:blank".into());
        assert_eq!(urls(&controller), ["about:blank"]);
        assert!(controller.entries()[0].initial);
        controller.did_start(2, NavigationType::Reload);
        controller.did_commit(2, "about:blank".into());
        assert!(controller.entries()[0].initial);
        let params = controller.entry_params(NavigationType::NewEntry);
        assert_eq!((params.index, params.length), (0, 1));
        controller.did_start(3, NavigationType::NewEntry);
        controller.did_commit(3, "a".into());
        assert_eq!(urls(&controller), ["a"]);
        assert!(!controller.entries()[0].initial);
        controller.did_start(4, NavigationType::NewEntry);
        controller.did_commit(4, "b".into());
        assert_eq!(urls(&controller), ["a", "b"]);
    }

    /// Each commit is a new document; entries a document made of itself
    /// share it, with their own state and the current one's scroll
    /// restoration mode. A reload is a new document: its siblings keep the
    /// old one, so they are loaded again.
    #[test]
    fn entries_know_their_document_and_state() {
        let mut controller = NavigationController::default();
        controller.did_start(1, NavigationType::NewEntry);
        controller.did_commit(1, "a".into());
        controller.set_manual_scroll_restoration(true);
        controller.did_commit_same_document("a#x".into(), false, Some(vec![1]));
        let entries = controller.entries();
        assert_eq!((entries[0].document, entries[1].document), (1, 1));
        assert_eq!(entries[1].state, Some(vec![1]));
        assert!(entries[1].manual_scroll_restoration);
        assert!(controller.is_same_document(0));
        controller.did_commit_same_document("a#y".into(), true, Some(vec![2]));
        assert_eq!(urls(&controller), ["a", "a#y"]);
        assert_eq!(controller.entries()[1].state, Some(vec![2]));
        controller.did_start(2, NavigationType::Reload);
        controller.did_commit(2, "a#y".into());
        assert_eq!(controller.entries()[1].document, 2);
        assert_eq!(controller.entries()[1].state, Some(vec![2]));
        assert!(!controller.is_same_document(0));
        assert_eq!(controller.current_document(), Some(2));
        assert_eq!(controller.offset_and_length(), (1, 2));
        let params = |kind| {
            let p = controller.entry_params(kind);
            (p.state, p.manual_scroll_restoration, p.index, p.length)
        };
        assert_eq!(params(NavigationType::Reload), (Some(vec![2]), true, 1, 2));
        assert_eq!(params(NavigationType::HistoryIndex(0)), (None, true, 0, 2));
        assert_eq!(params(NavigationType::NewEntry), (None, false, 2, 3));
        assert_eq!(params(NavigationType::Replace), (None, false, 1, 2));
        // A new document's entry has no state.
        controller.did_start(3, NavigationType::NewEntry);
        controller.did_commit(3, "b".into());
        let b = &controller.entries()[2];
        assert_eq!(
            (b.document, b.state.clone(), b.manual_scroll_restoration),
            (3, None, false)
        );
    }

    /// A traversal within the document: its entry is the current one once
    /// the renderer says it committed. One the renderer couldn't do gives
    /// back its entry, to load instead.
    #[test]
    fn traversals_within_the_document() {
        let mut controller = NavigationController::default();
        controller.did_start(1, NavigationType::NewEntry);
        controller.did_commit(1, "a".into());
        controller.did_commit_same_document("a#x".into(), false, None);
        controller.did_start(2, NavigationType::HistoryIndex(0));
        assert_eq!(controller.index_for_offset(1), Some(1));
        controller.did_commit_history_same_document(2, "a".into());
        assert_eq!(controller.current_index(), Some(0));
        controller.did_start(3, NavigationType::HistoryIndex(1));
        assert_eq!(controller.did_fail_same_document(3), Some(1));
        assert_eq!(controller.current_index(), Some(0));
        assert_eq!(controller.did_fail_same_document(3), None);
        // Loaded instead: a new document, of that entry only.
        controller.did_start(4, NavigationType::HistoryIndex(1));
        controller.did_commit(4, "a#x".into());
        assert_eq!(controller.current_document(), Some(4));
        assert!(!controller.is_same_document(0));
    }

    #[test]
    fn a_history_navigation_to_a_dropped_entry_becomes_a_new_one() {
        let mut controller = three();
        // Forward is pending when a same-document navigation drops that
        // entry: it commits as a new one instead of an index that's gone.
        controller.did_start(4, NavigationType::HistoryIndex(1));
        controller.did_commit(4, "b".into());
        controller.did_start(5, NavigationType::HistoryIndex(2));
        controller.did_commit_same_document("b#x".into(), false, None);
        controller.did_commit(5, "c".into());
        assert_eq!(urls(&controller), ["a", "b", "b#x", "c"]);
    }
}
