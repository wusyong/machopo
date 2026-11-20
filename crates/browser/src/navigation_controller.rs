//! `content::NavigationController`: a tab's session history, the list of
//! entries back and forward go through. Each entry is a tree of its frames'
//! places (Chromium's `NavigationEntryImpl::TreeNode`), a frame's matched by
//! its unique name; what a place has (`FrameNavigationEntry`) is the
//! controller's, by its item sequence number, so entries that have one (a
//! frame that didn't navigate between them) see it change as one. Chromium
//! shares a `FrameNavigationEntry` by `scoped_refptr`; here the controller is
//! its one owner, and drops what no entry has any more (`sweep`).

use std::collections::{BTreeMap, HashMap, HashSet};

use base::SequenceNumber;
use protocol::FrameToken;
use protocol::renderer::HistoryEntryParams;

use crate::frame_tree::FrameTree;

/// `content::FrameNavigationEntry`: a frame's history item, which the
/// entries it is in share.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameNavigationEntry {
    /// Its item sequence number: the controller's name for it.
    pub item: u64,
    /// The URL committed, after redirects.
    pub url: String,
    /// The document it is an item of: the id of the navigation that
    /// committed that document (Chromium's document sequence number).
    /// Items a document made of itself (a fragment, `pushState`) share it;
    /// back and forward between them don't load anything.
    pub document: u64,
    /// The classic history API's state, as the renderer serialized it;
    /// `None` is `null` (part of Chromium's `PageState`).
    pub state: Option<Vec<u8>>,
    /// Its scroll restoration mode is `"manual"`.
    pub manual_scroll_restoration: bool,
}

/// A frame's place in an entry: its item, and its child frames' places.
#[derive(Debug, Clone)]
struct TreeNode {
    /// The frame's unique name: empty for the main frame.
    unique_name: String,
    item: u64,
    children: Vec<TreeNode>,
}

impl TreeNode {
    fn new(unique_name: String, item: u64) -> Self {
        Self {
            unique_name,
            item,
            children: Vec::new(),
        }
    }

    /// The place `path` (unique names, from this one's children down) leads
    /// to.
    fn find(&self, path: &[&str]) -> Option<&Self> {
        path.iter().try_fold(self, |node, name| {
            node.children
                .iter()
                .find(|child| child.unique_name == *name)
        })
    }

    fn find_mut(&mut self, path: &[&str]) -> Option<&mut Self> {
        path.iter().try_fold(self, |node, name| {
            node.children
                .iter_mut()
                .find(|child| child.unique_name == *name)
        })
    }

    /// The place at `path` is `item`'s, a new document's (its frames'
    /// places go): added under its parent's if there is none. False if its
    /// parent has no place (Chromium's `AddOrUpdateFrameEntry` finding no
    /// parent `TreeNode`).
    fn place(&mut self, path: &[&str], item: u64) -> bool {
        self.place_keeping(path, item, false)
    }

    /// `place`, its frames' places kept if `keep_children` (the same
    /// document's item: Chromium's `clone_children_of_target`).
    fn place_keeping(&mut self, path: &[&str], item: u64, keep_children: bool) -> bool {
        let Some((name, parent)) = path.split_last() else {
            self.item = item;
            if !keep_children {
                self.children.clear();
            }
            return true;
        };
        let Some(parent) = self.find_mut(parent) else {
            return false;
        };
        match parent
            .children
            .iter_mut()
            .find(|child| child.unique_name == *name)
        {
            Some(node) => {
                node.item = item;
                if !keep_children {
                    node.children.clear();
                }
            }
            None => parent.children.push(Self::new((*name).to_owned(), item)),
        }
        true
    }

    /// A copy with only the places of `frame`'s frames there are now, as
    /// `frame` is this one's (Chromium's `CloneAndReplace` leaving out
    /// "subframe history items that do not correspond to frames actually in
    /// the current page").
    fn clone_live(&self, tree: &FrameTree, frame: FrameToken) -> Self {
        let children =
            self.children
                .iter()
                .filter_map(|child| {
                    let live = tree.children(frame).iter().find(|live| {
                        tree.unique_name(**live) == Some(child.unique_name.as_str())
                    })?;
                    Some(child.clone_live(tree, *live))
                })
                .collect();
        Self {
            unique_name: self.unique_name.clone(),
            item: self.item,
            children,
        }
    }

    fn items(&self, into: &mut HashSet<u64>) {
        into.insert(self.item);
        for child in &self.children {
            child.items(into);
        }
    }
}

/// One page in the history: its frames' places, and what it has of the
/// main frame's document.
#[derive(Debug, Clone)]
struct Entry {
    /// Its own number, which no other entry has (Chromium's
    /// `NavigationEntry::GetUniqueID`): what a frame's document committed
    /// in (`HistoryPlace::entry`).
    id: u64,
    title: String,
    /// The tab's first document's, which the next new entry takes the
    /// place of (Chromium's `NavigationEntryImpl::IsInitialEntry`: a new
    /// tab's `about:blank` isn't something to go back to).
    initial: bool,
    root: TreeNode,
}

/// `content::NavigationEntry`: one page in the history, as its controller
/// has it.
#[derive(Debug, Clone, Copy)]
pub struct NavigationEntry<'a> {
    controller: &'a NavigationController,
    entry: &'a Entry,
}

impl<'a> NavigationEntry<'a> {
    /// The main frame's URL.
    pub fn url(&self) -> &'a str {
        &self.root().url
    }

    pub fn title(&self) -> &'a str {
        &self.entry.title
    }

    /// Whether it is the tab's first document's, which the next new entry
    /// takes the place of.
    pub fn is_initial(&self) -> bool {
        self.entry.initial
    }

    /// The main frame's item.
    pub fn root(&self) -> &'a FrameNavigationEntry {
        self.controller.item(self.entry.root.item)
    }

    /// The item of the frame whose unique name is `unique_name` (empty for
    /// the main frame), if the entry has a place for it.
    pub fn frame(&self, unique_name: &str) -> Option<&'a FrameNavigationEntry> {
        let mut nodes = vec![&self.entry.root];
        while let Some(node) = nodes.pop() {
            if node.unique_name == unique_name {
                return Some(self.controller.item(node.item));
            }
            nodes.extend(&node.children);
        }
        None
    }

    /// How many frames it has a place for, the main frame's included.
    pub fn frame_count(&self) -> usize {
        let mut items = HashSet::new();
        self.entry.root.items(&mut items);
        items.len()
    }
}

/// What a navigation does to the history once it commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavigationType {
    /// A new entry after the current one, dropping those forward of it: a
    /// URL typed or given, or a link followed. A child frame's has the
    /// current one's places of the other frames there are.
    NewEntry,
    /// Back or forward to the entry at this index (Chromium's history
    /// navigation): loading it again, or, for an entry of the document
    /// shown, within the document.
    HistoryIndex(usize),
    /// The current entry, loaded again.
    Reload,
    /// A new document in the current entry's place (`location.replace`);
    /// a child frame's, in its place in the current entry.
    Replace,
    /// The tab's first document, as an initial entry.
    Initial,
}

/// Where a frame's document committed in the session history: the entry,
/// and its item there (with its document), as the frame tree keeps it for
/// the frame (Chromium's `RenderFrameHostImpl::nav_entry_id` and
/// `last_committed_frame_entry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoryPlace {
    pub(crate) entry: u64,
    pub(crate) item: u64,
    pub(crate) document: u64,
}

/// What back or forward does to a frame (Chromium's
/// `HistoryNavigationAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryLoad {
    /// A traversal within the document it shows.
    SameDocument,
    /// Its entry's document loaded again.
    DifferentDocument,
}

/// A navigation started and not committed yet: whose, and what it does.
#[derive(Debug, Clone, Copy)]
struct Pending {
    frame: FrameToken,
    kind: NavigationType,
}

#[derive(Debug, Default)]
pub struct NavigationController {
    entries: Vec<Entry>,
    entry_ids: SequenceNumber<u64>,
    /// Every item an entry has a place for, by item sequence number.
    items: HashMap<u64, FrameNavigationEntry>,
    item_numbers: SequenceNumber<u64>,
    /// The entry shown; `None` before the first commit.
    current: Option<usize>,
    /// Navigations started and not committed yet, by id (ids grow with
    /// each one started).
    pending: BTreeMap<u64, Pending>,
}

impl NavigationController {
    /// The entries, the first first.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = NavigationEntry<'_>> {
        self.entries.iter().map(|entry| NavigationEntry {
            controller: self,
            entry,
        })
    }

    /// The entry at `index`, if there is one.
    pub fn entry(&self, index: usize) -> Option<NavigationEntry<'_>> {
        self.entries.get(index).map(|entry| NavigationEntry {
            controller: self,
            entry,
        })
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// The index of the entry shown (`GetLastCommittedEntryIndex`).
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// Whether the entry shown is the tab's first document's, which the
    /// next new entry takes the place of.
    pub(crate) fn is_initial(&self) -> bool {
        self.current
            .is_some_and(|current| self.entries[current].initial)
    }

    /// An item an entry has a place for: there is one, as `sweep` keeps
    /// every item a place has.
    fn item(&self, item: u64) -> &FrameNavigationEntry {
        &self.items[&item]
    }

    /// Where back and forward count from (`GetCurrentEntryIndex`): the
    /// entry the latest navigation goes to if that is a history one,
    /// else the one shown. Going back twice quickly goes back two.
    fn base_index(&self) -> Option<usize> {
        match self.pending.last_key_value() {
            Some((
                _,
                Pending {
                    kind: NavigationType::HistoryIndex(index),
                    ..
                },
            )) => Some(*index),
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

    /// Navigation `navigation_id` stopped: it won't commit.
    pub(crate) fn did_stop(&mut self, navigation_id: u64) {
        self.pending.remove(&navigation_id);
    }

    /// `frame`'s navigation `navigation_id` started, to do `kind` as it
    /// commits.
    pub(crate) fn did_start(
        &mut self,
        navigation_id: u64,
        frame: FrameToken,
        kind: NavigationType,
    ) {
        self.pending.insert(navigation_id, Pending { frame, kind });
    }

    /// What the renderer is told of the item `frame`'s navigation of
    /// `kind` will commit to, as things are now: its state, and its entry's
    /// place.
    pub(crate) fn entry_params(
        &self,
        tree: &FrameTree,
        frame: FrameToken,
        kind: NavigationType,
    ) -> HistoryEntryParams {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let len = self.entries.len();
        let item_in = |index: usize| {
            let path = path(tree, frame)?;
            let node = self.entries[index].root.find(&path)?;
            Some(self.item(node.item))
        };
        let item = match (kind, self.current) {
            (NavigationType::HistoryIndex(index), _) if index < len => item_in(index),
            (NavigationType::Reload, Some(current)) => item_in(current),
            _ => None,
        };
        let (index, length) = self.place(kind);
        HistoryEntryParams {
            state: item.and_then(|item| item.state.clone()),
            manual_scroll_restoration: item.is_some_and(|item| item.manual_scroll_restoration),
            index: count(index),
            length: count(length),
            // `WebContents`'s, which goes in as it commits.
            entries_heard: 0,
        }
    }

    /// Where the entry navigation `navigation_id` commits to will be, and
    /// how many entries there will be, as things are now: for its commit,
    /// what changed since it started counted. None for a navigation not
    /// pending.
    pub(crate) fn commit_place(&self, navigation_id: u64) -> Option<(u32, u32)> {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let (index, length) = self.place(self.pending.get(&navigation_id)?.kind);
        Some((count(index), count(length)))
    }

    /// Where the entry a navigation of `kind` commits to will be, and how
    /// many entries there will be then, as things are now.
    fn place(&self, kind: NavigationType) -> (usize, usize) {
        let len = self.entries.len();
        match (kind, self.current) {
            (NavigationType::HistoryIndex(index), _) if index < len => (index, len),
            (NavigationType::Reload | NavigationType::Replace, Some(current)) => (current, len),
            (NavigationType::NewEntry, Some(current)) if self.entries[current].initial => {
                (current, len)
            }
            (_, current) => {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "an index of the entries, a Vec's (at most isize::MAX), and one past it"
                )]
                let place = {
                    let index = current.map_or(0, |current| current + 1);
                    (index, index + 1)
                };
                place
            }
        }
    }

    /// The current entry's index and the number of entries, for the
    /// renderer (`SetHistoryOffsetAndLength`).
    pub(crate) fn offset_and_length(&self) -> (u32, u32) {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        (count(self.current.unwrap_or(0)), count(self.entries.len()))
    }

    /// `RendererDidNavigate`: `frame` of `tree` committed navigation
    /// `navigation_id` at `url`. The renderer drops commits older than the
    /// document a frame shows, so every navigation of the frame started
    /// before this one is over too; the main frame's, every frame's (they
    /// went with its document).
    pub(crate) fn did_commit(
        &mut self,
        tree: &FrameTree,
        frame: FrameToken,
        navigation_id: u64,
        url: String,
    ) {
        let main_frame = frame == tree.root();
        let kind = self
            .pending
            .remove(&navigation_id)
            .map(|pending| pending.kind);
        self.pending
            .retain(|id, pending| *id > navigation_id || !(main_frame || pending.frame == frame));
        if main_frame {
            self.did_commit_main_frame(kind, navigation_id, url);
        } else {
            self.did_commit_child_frame(tree, frame, kind, navigation_id, url);
        }
        self.sweep();
    }

    fn did_commit_main_frame(
        &mut self,
        kind: Option<NavigationType>,
        navigation_id: u64,
        url: String,
    ) {
        // A new document: the entry it commits to is its, keeping its
        // state (a reload, a traversal).
        match (kind, self.current) {
            (Some(NavigationType::HistoryIndex(index)), _) if index < self.entries.len() => {
                // Its frames' places stay: what its new document's frames
                // are restored from.
                let item = self.entries[index].root.item;
                self.loaded_back(item, url, navigation_id);
                self.current = Some(index);
            }
            (Some(NavigationType::Reload), Some(current)) => {
                // Its new document's frames load by their `src`, each
                // taking its place again.
                let root = &mut self.entries[current].root;
                root.children.clear();
                let item = root.item;
                self.loaded_again(item, url, navigation_id);
            }
            (Some(NavigationType::Replace), Some(current)) => {
                self.entries[current] = self.new_entry(url, navigation_id, false);
            }
            (Some(NavigationType::NewEntry), Some(current)) if self.entries[current].initial => {
                self.entries[current] = self.new_entry(url, navigation_id, false);
            }
            (Some(NavigationType::Initial), _) => {
                let entry = self.new_entry(url, navigation_id, true);
                self.push(entry);
            }
            (None, _) => {
                tracing::warn!(navigation_id, %url, "a commit for no navigation the browser started");
                let entry = self.new_entry(url, navigation_id, false);
                self.push(entry);
            }
            _ => {
                let entry = self.new_entry(url, navigation_id, false);
                self.push(entry);
            }
        }
    }

    /// A child frame's new document (Chromium's `NEW_SUBFRAME` and
    /// `AUTO_SUBFRAME`): a new entry, the current one's but for the frame's
    /// place, or the frame's place in the current one. Nothing for a frame
    /// whose parent has no place in it (Chromium's neither), or with no
    /// entry yet.
    fn did_commit_child_frame(
        &mut self,
        tree: &FrameTree,
        frame: FrameToken,
        kind: Option<NavigationType>,
        navigation_id: u64,
        url: String,
    ) {
        let (Some(current), Some(path)) = (self.current, path(tree, frame)) else {
            return;
        };
        match kind {
            Some(NavigationType::NewEntry) => {
                let item = self.new_item(url, navigation_id);
                let base = &self.entries[current];
                let mut root = base.root.clone_live(tree, tree.root());
                if !root.place(&path, item) {
                    return;
                }
                let entry = Entry {
                    id: self.entry_ids.get_next(),
                    title: base.title.clone(),
                    initial: false,
                    root,
                };
                self.push(entry);
            }
            // Back or forward to its item in the entry at `index`: the
            // current entry now (Chromium's `RendererDidNavigateAutoSubframe`
            // of a history navigation); its frames' places stay, what its
            // new document's frames are restored from.
            Some(NavigationType::HistoryIndex(index))
                if self
                    .entries
                    .get(index)
                    .is_some_and(|entry| entry.root.find(&path).is_some()) =>
            {
                let item = self.entries[index]
                    .root
                    .find(&path)
                    .expect("found above")
                    .item;
                self.loaded_back(item, url, navigation_id);
                self.current = Some(index);
            }
            // Its document loaded again: its item has it.
            Some(NavigationType::Reload) if self.entries[current].root.find(&path).is_some() => {
                let node = self.entries[current]
                    .root
                    .find_mut(&path)
                    .expect("found above");
                node.children.clear();
                let item = node.item;
                self.loaded_again(item, url, navigation_id);
            }
            // A replacement, the frame's first document, its renderer's own
            // (a `javascript:` URL's).
            _ => {
                let item = self.new_item(url, navigation_id);
                self.entries[current].root.place(&path, item);
            }
        }
    }

    /// `RendererDidNavigateToSameDocument`: `frame`'s document navigated
    /// within itself (a fragment, `pushState`), nothing loaded: a new entry,
    /// the current one's but for the frame's item, an item of the same
    /// document with the current one's scroll restoration mode (its frames'
    /// places kept, the entry's title too); or, if `replace`, the current
    /// item's new URL and state. A frame with no place in the current entry
    /// (on its initial `about:blank`, its document the browser's 0: none it
    /// committed) gets one, if its parent has one.
    pub(crate) fn did_commit_same_document(
        &mut self,
        tree: &FrameTree,
        frame: FrameToken,
        url: String,
        replace: bool,
        state: Option<Vec<u8>>,
    ) {
        let Some(path) = path(tree, frame) else {
            return;
        };
        let Some(current) = self.current else {
            if path.is_empty() {
                let entry = self.new_entry(url, 0, false);
                self.items
                    .get_mut(&entry.root.item)
                    .expect("just made")
                    .state = state;
                self.push(entry);
            }
            return;
        };
        let base = &self.entries[current];
        let before = base
            .root
            .find(&path)
            .map(|node| self.item(node.item).clone());
        match before {
            Some(before) if replace => {
                let item = self.items.get_mut(&before.item).expect("an entry's");
                item.url = url;
                item.state = state;
                return;
            }
            None if replace => {
                let item = self.new_item(url, 0);
                self.items.get_mut(&item).expect("just made").state = state;
                self.entries[current].root.place(&path, item);
                return self.sweep();
            }
            _ => {}
        }
        let before = before.unwrap_or_default();
        let mut root = base.root.clone_live(tree, tree.root());
        let title = base.title.clone();
        let item = self.item_numbers.get_next();
        self.items.insert(
            item,
            FrameNavigationEntry {
                item,
                url,
                document: before.document,
                state,
                manual_scroll_restoration: before.manual_scroll_restoration,
            },
        );
        if !root.place_keeping(&path, item, true) {
            return self.sweep();
        }
        let id = self.entry_ids.get_next();
        self.push(Entry {
            id,
            title,
            initial: false,
            root,
        });
        self.sweep();
    }

    /// The traversal `navigation_id` within `frame`'s document committed
    /// at `url` (`DidCommitSameDocumentNavigation` of a history
    /// navigation): its entry is the current one, the frame's item there
    /// at `url`.
    pub(crate) fn did_commit_history_same_document(
        &mut self,
        tree: &FrameTree,
        frame: FrameToken,
        navigation_id: u64,
        url: String,
    ) {
        match self
            .pending
            .remove(&navigation_id)
            .map(|pending| pending.kind)
        {
            Some(NavigationType::HistoryIndex(index)) if index < self.entries.len() => {
                let node = path(tree, frame)
                    .and_then(|path| self.entries[index].root.find(&path).map(|node| node.item));
                if let Some(item) = node {
                    self.items.get_mut(&item).expect("an entry's").url = url;
                }
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
    /// `navigation_id` was for: the frame, and the entry whose item it
    /// loads instead, if it is still there.
    pub(crate) fn did_fail_same_document(
        &mut self,
        navigation_id: u64,
    ) -> Option<(FrameToken, usize)> {
        match self.pending.remove(&navigation_id) {
            Some(Pending {
                frame,
                kind: NavigationType::HistoryIndex(index),
            }) if index < self.entries.len() => Some((frame, index)),
            _ => None,
        }
    }

    /// `frame`'s item in the entry at `index`, if it has a place there.
    pub(crate) fn item_at(
        &self,
        tree: &FrameTree,
        frame: FrameToken,
        index: usize,
    ) -> Option<&FrameNavigationEntry> {
        let path = path(tree, frame)?;
        let node = self.entries.get(index)?.root.find(&path)?;
        Some(self.item(node.item))
    }

    /// Where `frame`'s document is in the current entry, for its node to
    /// keep as what it committed: none for a frame with no place there.
    pub(crate) fn current_place(
        &self,
        tree: &FrameTree,
        frame: FrameToken,
    ) -> Option<HistoryPlace> {
        let entry = &self.entries[self.current?];
        let path = path(tree, frame)?;
        let item = self.item(entry.root.find(&path)?.item);
        Some(HistoryPlace {
            entry: entry.id,
            item: item.item,
            document: item.document,
        })
    }

    /// A new child frame `frame`'s document as the session history has it
    /// (Chromium's `StartHistoryNavigationInNewSubframe`): its item in the
    /// entry `entry` its parent's document committed in, and that entry's
    /// index, if that is still there and has a place for it.
    pub(crate) fn item_for_new_child_frame(
        &self,
        tree: &FrameTree,
        frame: FrameToken,
        entry: u64,
    ) -> Option<(usize, &FrameNavigationEntry)> {
        let index = self.entries.iter().position(|it| it.id == entry)?;
        Some((index, self.item_at(tree, frame, index)?))
    }

    /// Back or forward to the entry at `index`: the frames of `tree` it
    /// moves and how (Chromium's `FindFramesToNavigate` and
    /// `DetermineActionForHistoryNavigation`), each frame's own item as
    /// `committed` has it (else the current entry's). From the main frame
    /// down: a frame the entry has no place for stays as it is, and the
    /// frames in it; one at the same item, its frames looked at; one at an
    /// item of the same document traverses within it, its frames looked at
    /// too; one at another document loads it again, the frames in it going
    /// with the one it shows. An item of no document the browser committed
    /// (an initial `about:blank`'s, a `javascript:` URL's: 0) is loaded
    /// again. None at all (the entry's frames that differ gone:
    /// crbug.com/705550, or the current entry itself) is
    /// for the entry to be the current one, nothing loaded
    /// (`go_to_entry_in_place`).
    pub(crate) fn frames_to_navigate(
        &self,
        tree: &FrameTree,
        index: usize,
        committed: impl Fn(FrameToken) -> Option<HistoryPlace>,
    ) -> Vec<(FrameToken, HistoryLoad)> {
        let mut loads = Vec::new();
        let mut frames = vec![tree.root()];
        while let Some(frame) = frames.pop() {
            let Some(new) = self.item_at(tree, frame, index) else {
                continue;
            };
            let old = committed(frame)
                .map(|place| (place.item, place.document))
                .or_else(|| {
                    let item = self.item_at(tree, frame, self.current?)?;
                    Some((item.item, item.document))
                });
            match old {
                Some((item, _)) if item == new.item => {
                    frames.extend(tree.children(frame).iter().rev());
                }
                Some((_, document)) if document == new.document && document != 0 => {
                    loads.push((frame, HistoryLoad::SameDocument));
                    frames.extend(tree.children(frame).iter().rev());
                }
                _ => loads.push((frame, HistoryLoad::DifferentDocument)),
            }
        }
        loads
    }

    /// Back or forward to the entry at `index` with no frame to move
    /// (`frames_to_navigate` found none): it is the current one, the
    /// documents shown kept (Chromium's "redundant" same-document
    /// navigation of the main frame, which fires nothing).
    pub(crate) fn go_to_entry_in_place(&mut self, index: usize) {
        if index < self.entries.len() {
            self.current = Some(index);
        }
    }

    /// `frame`'s scroll restoration mode in the current entry
    /// (`UpdateState`): nothing for a frame with no place there (one
    /// showing its initial `about:blank`).
    pub(crate) fn set_manual_scroll_restoration(
        &mut self,
        tree: &FrameTree,
        frame: FrameToken,
        manual: bool,
    ) {
        let Some(current) = self.current else {
            return;
        };
        let Some(node) = path(tree, frame)
            .and_then(|path| self.entries[current].root.find(&path).map(|node| node.item))
        else {
            return;
        };
        self.items
            .get_mut(&node)
            .expect("an entry's")
            .manual_scroll_restoration = manual;
    }

    /// The title of the entry shown.
    pub(crate) fn set_title(&mut self, title: String) {
        if let Some(current) = self.current {
            self.entries[current].title = title;
        }
    }

    /// A new item of a new document.
    fn new_item(&mut self, url: String, document: u64) -> u64 {
        let item = self.item_numbers.get_next();
        self.items.insert(
            item,
            FrameNavigationEntry {
                item,
                url,
                document,
                ..FrameNavigationEntry::default()
            },
        );
        item
    }

    /// An entry of the main frame's new document alone.
    fn new_entry(&mut self, url: String, document: u64, initial: bool) -> Entry {
        let item = self.new_item(url, document);
        Entry {
            id: self.entry_ids.get_next(),
            title: String::new(),
            initial,
            root: TreeNode::new(String::new(), item),
        }
    }

    /// `item`'s document loaded back or forward, as `document`: what Blink
    /// has of its item (its document sequence number) is the new one's, so
    /// the items of the document it was are of this one, traversals within
    /// it (Chromium's history load restoring the `HistoryItem`). Not those
    /// of no document the browser committed (0), which are no one document.
    fn loaded_back(&mut self, item: u64, url: String, document: u64) {
        let before = self.item(item).document;
        if before != 0 {
            for other in self.items.values_mut() {
                if other.document == before {
                    other.document = document;
                }
            }
        }
        self.loaded_again(item, url, document);
    }

    /// `item`'s frame has its document again, a new one: the entries that
    /// have it have it.
    fn loaded_again(&mut self, item: u64, url: String, document: u64) {
        let item = self.items.get_mut(&item).expect("an entry's");
        item.url = url;
        item.document = document;
    }

    /// A new entry after the current one; the ones forward of it go.
    fn push(&mut self, entry: Entry) {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the current entry's index, one of a Vec's: one past it fits"
        )]
        let keep = self.current.map_or(0, |current| current + 1);
        self.entries.truncate(keep);
        // A history navigation pending to an entry that just went can't
        // commit as one any more.
        for pending in self.pending.values_mut() {
            if matches!(pending.kind, NavigationType::HistoryIndex(index) if index >= keep) {
                pending.kind = NavigationType::NewEntry;
            }
        }
        let index = self.entries.len();
        self.entries.push(entry);
        self.current = Some(index);
    }

    /// Drops the items no entry has a place for any more (an entry gone,
    /// a place another item took): what Chromium's `scoped_refptr`s do as
    /// the last goes. Once a commit, through every entry's places.
    fn sweep(&mut self) {
        let mut kept = HashSet::new();
        for entry in &self.entries {
            entry.root.items(&mut kept);
        }
        self.items.retain(|item, _| kept.contains(item));
    }
}

/// `frame`'s place in `tree`: the unique names from the main frame's
/// children down to it (none for the main frame). None for a frame not in
/// it.
fn path(tree: &FrameTree, frame: FrameToken) -> Option<Vec<&str>> {
    let mut path = Vec::new();
    let mut at = frame;
    while let Some(parent) = tree.parent(at) {
        path.push(tree.unique_name(at)?);
        at = parent;
    }
    (at == tree.root()).then(|| {
        path.reverse();
        path
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: FrameToken = FrameToken([0; 16]);

    fn token(n: u8) -> FrameToken {
        FrameToken([n; 16])
    }

    /// The main frame's document in the entry at `index`.
    fn document(controller: &NavigationController, index: usize) -> u64 {
        controller.entry(index).expect("an entry").root().document
    }

    fn urls(controller: &NavigationController) -> Vec<&str> {
        controller.entries().map(|entry| entry.url()).collect()
    }

    /// The main frame's navigation `id`, of `kind`, committed at `url`.
    fn commit(
        controller: &mut NavigationController,
        tree: &FrameTree,
        id: u64,
        kind: NavigationType,
        url: &str,
    ) {
        controller.did_start(id, MAIN, kind);
        controller.did_commit(tree, MAIN, id, url.into());
    }

    /// `a`, `b`, `c` committed in turn, as navigations 1 to 3.
    fn three() -> (NavigationController, FrameTree) {
        let tree = FrameTree::new(MAIN);
        let mut controller = NavigationController::default();
        for (id, url) in [(1, "a"), (2, "b"), (3, "c")] {
            commit(&mut controller, &tree, id, NavigationType::NewEntry, url);
        }
        (controller, tree)
    }

    #[test]
    fn new_entries_go_after_the_current_one() {
        let tree = FrameTree::new(MAIN);
        let mut controller = NavigationController::default();
        assert!(!controller.can_go_back() && !controller.can_go_forward());
        controller.did_start(1, MAIN, NavigationType::NewEntry);
        assert_eq!(controller.current_index(), None);
        controller.did_commit(&tree, MAIN, 1, "a".into());
        controller.set_title("A".into());
        assert_eq!(controller.entries().next().map(|e| e.title()), Some("A"));
        let (mut controller, tree) = three();
        assert_eq!(urls(&controller), ["a", "b", "c"]);
        assert_eq!(controller.current_index(), Some(2));
        assert!(controller.can_go_back() && !controller.can_go_forward());
        // Back to `b`, then a new page: `c` goes.
        commit(
            &mut controller,
            &tree,
            4,
            NavigationType::HistoryIndex(1),
            "b",
        );
        assert!(controller.can_go_forward());
        commit(&mut controller, &tree, 5, NavigationType::NewEntry, "d");
        assert_eq!(urls(&controller), ["a", "b", "d"]);
        assert!(!controller.can_go_forward());
        // `c`'s item went with it.
        assert_eq!(controller.items.len(), 3);
    }

    #[test]
    fn back_and_forward_move_through_the_entries() {
        let (mut controller, tree) = three();
        assert_eq!(controller.index_for_offset(-1), Some(1));
        controller.did_start(4, MAIN, NavigationType::HistoryIndex(1));
        // Pending: a second back counts from where the first goes.
        assert_eq!(controller.index_for_offset(-1), Some(0));
        controller.did_start(5, MAIN, NavigationType::HistoryIndex(0));
        assert_eq!(controller.index_for_offset(-1), None);
        // The later one commits; the earlier one never will.
        controller.did_commit(&tree, MAIN, 5, "a".into());
        assert_eq!(controller.current_index(), Some(0));
        assert_eq!(urls(&controller), ["a", "b", "c"]);
        assert_eq!(controller.index_for_offset(2), Some(2));
        // A redirect on the way changes the entry's URL.
        commit(
            &mut controller,
            &tree,
            6,
            NavigationType::HistoryIndex(2),
            "c2",
        );
        assert_eq!(urls(&controller), ["a", "b", "c2"]);
        assert_eq!(controller.current_index(), Some(2));
    }

    #[test]
    fn reload_keeps_the_entry_and_same_document_adds_one() {
        let (mut controller, tree) = three();
        commit(&mut controller, &tree, 4, NavigationType::Reload, "c");
        assert_eq!(urls(&controller), ["a", "b", "c"]);
        controller.did_commit_same_document(&tree, MAIN, "c#x".into(), false, None);
        assert_eq!(urls(&controller), ["a", "b", "c", "c#x"]);
        assert_eq!(controller.current_index(), Some(3));
    }

    /// `location.replace`: the new document, or the fragment, takes the
    /// current entry's place; the ones forward of it stay. With no entry
    /// yet, it is the first.
    #[test]
    fn replace_takes_the_current_entry_s_place() {
        let (mut controller, tree) = three();
        commit(
            &mut controller,
            &tree,
            4,
            NavigationType::HistoryIndex(1),
            "b",
        );
        controller.set_title("B".into());
        commit(&mut controller, &tree, 5, NavigationType::Replace, "r");
        assert_eq!(urls(&controller), ["a", "r", "c"]);
        assert_eq!(controller.current_index(), Some(1));
        assert_eq!(controller.entry(1).map(|e| e.title()), Some(""));
        controller.did_commit_same_document(&tree, MAIN, "r#x".into(), true, None);
        assert_eq!(urls(&controller), ["a", "r#x", "c"]);
        assert_eq!(controller.current_index(), Some(1));
        let mut empty = NavigationController::default();
        commit(&mut empty, &tree, 1, NavigationType::Replace, "a");
        assert_eq!(urls(&empty), ["a"]);
    }

    /// The initial entry is there until the next new entry takes its
    /// place; another kind of navigation keeps it.
    #[test]
    fn a_new_entry_takes_the_initial_entry_s_place() {
        let tree = FrameTree::new(MAIN);
        let mut controller = NavigationController::default();
        controller.did_start(1, MAIN, NavigationType::Initial);
        assert_eq!(
            controller
                .entry_params(&tree, MAIN, NavigationType::Initial)
                .index,
            0
        );
        controller.did_commit(&tree, MAIN, 1, "about:blank".into());
        assert_eq!(urls(&controller), ["about:blank"]);
        assert!(controller.is_initial());
        commit(
            &mut controller,
            &tree,
            2,
            NavigationType::Reload,
            "about:blank",
        );
        assert!(controller.is_initial());
        let params = controller.entry_params(&tree, MAIN, NavigationType::NewEntry);
        assert_eq!((params.index, params.length), (0, 1));
        commit(&mut controller, &tree, 3, NavigationType::NewEntry, "a");
        assert_eq!(urls(&controller), ["a"]);
        assert!(!controller.is_initial());
        commit(&mut controller, &tree, 4, NavigationType::NewEntry, "b");
        assert_eq!(urls(&controller), ["a", "b"]);
    }

    /// Each commit is a new document; entries a document made of itself
    /// share it, with their own state and the current one's scroll
    /// restoration mode. A reload is a new document: its siblings keep the
    /// old one, so they are loaded again.
    #[test]
    fn entries_know_their_document_and_state() {
        let tree = FrameTree::new(MAIN);
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        controller.set_manual_scroll_restoration(&tree, MAIN, true);
        controller.did_commit_same_document(&tree, MAIN, "a#x".into(), false, Some(vec![1]));
        let root = |controller: &NavigationController, index| {
            controller.entry(index).expect("an entry").root().clone()
        };
        assert_eq!(
            (root(&controller, 0).document, root(&controller, 1).document),
            (1, 1)
        );
        assert_eq!(root(&controller, 1).state, Some(vec![1]));
        assert!(root(&controller, 1).manual_scroll_restoration);
        assert_eq!(document(&controller, 0), document(&controller, 1));
        controller.did_commit_same_document(&tree, MAIN, "a#y".into(), true, Some(vec![2]));
        assert_eq!(urls(&controller), ["a", "a#y"]);
        assert_eq!(root(&controller, 1).state, Some(vec![2]));
        commit(&mut controller, &tree, 2, NavigationType::Reload, "a#y");
        assert_eq!(root(&controller, 1).document, 2);
        assert_eq!(root(&controller, 1).state, Some(vec![2]));
        assert_ne!(document(&controller, 0), document(&controller, 1));
        assert_eq!(document(&controller, 1), 2);
        assert_eq!(controller.offset_and_length(), (1, 2));
        let params = |kind| {
            let p = controller.entry_params(&tree, MAIN, kind);
            (p.state, p.manual_scroll_restoration, p.index, p.length)
        };
        assert_eq!(params(NavigationType::Reload), (Some(vec![2]), true, 1, 2));
        assert_eq!(params(NavigationType::HistoryIndex(0)), (None, true, 0, 2));
        assert_eq!(params(NavigationType::NewEntry), (None, false, 2, 3));
        assert_eq!(params(NavigationType::Replace), (None, false, 1, 2));
        // A new document's entry has no state.
        commit(&mut controller, &tree, 3, NavigationType::NewEntry, "b");
        let b = root(&controller, 2);
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
        let tree = FrameTree::new(MAIN);
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        controller.did_commit_same_document(&tree, MAIN, "a#x".into(), false, None);
        controller.did_start(2, MAIN, NavigationType::HistoryIndex(0));
        assert_eq!(controller.index_for_offset(1), Some(1));
        controller.did_commit_history_same_document(&tree, MAIN, 2, "a".into());
        assert_eq!(controller.current_index(), Some(0));
        controller.did_start(3, MAIN, NavigationType::HistoryIndex(1));
        assert_eq!(controller.did_fail_same_document(3), Some((MAIN, 1)));
        assert_eq!(controller.current_index(), Some(0));
        assert_eq!(controller.did_fail_same_document(3), None);
        // Loaded instead: the new document is the one its items were of,
        // the other entry's too (Blink restores the item's document
        // sequence number); a reload's isn't.
        commit(
            &mut controller,
            &tree,
            4,
            NavigationType::HistoryIndex(1),
            "a#x",
        );
        assert_eq!((document(&controller, 0), document(&controller, 1)), (4, 4));
        commit(&mut controller, &tree, 5, NavigationType::Reload, "a#x");
        assert_eq!((document(&controller, 0), document(&controller, 1)), (4, 5));
    }

    #[test]
    fn a_history_navigation_to_a_dropped_entry_becomes_a_new_one() {
        let (mut controller, tree) = three();
        // Forward is pending when a same-document navigation drops that
        // entry: it commits as a new one instead of an index that's gone.
        commit(
            &mut controller,
            &tree,
            4,
            NavigationType::HistoryIndex(1),
            "b",
        );
        controller.did_start(5, MAIN, NavigationType::HistoryIndex(2));
        controller.did_commit_same_document(&tree, MAIN, "b#x".into(), false, None);
        controller.did_commit(&tree, MAIN, 5, "c".into());
        assert_eq!(urls(&controller), ["a", "b", "b#x", "c"]);
    }

    /// A tree of the main frame with child frames 1 and 2, and 3 in 1.
    fn frames() -> FrameTree {
        let mut tree = FrameTree::new(MAIN);
        tree.add_child(MAIN, token(1), "one", false).expect("made");
        tree.add_child(MAIN, token(2), "two", false).expect("made");
        tree.add_child(token(1), token(3), "three", false)
            .expect("made");
        tree
    }

    /// Child frame `frame`'s navigation `id`, of `kind`, committed at `url`.
    fn commit_child(
        controller: &mut NavigationController,
        tree: &FrameTree,
        frame: FrameToken,
        id: u64,
        kind: NavigationType,
        url: &str,
    ) {
        controller.did_start(id, frame, kind);
        controller.did_commit(tree, frame, id, url.into());
    }

    fn child_url<'a>(
        controller: &'a NavigationController,
        index: usize,
        name: &str,
    ) -> Option<&'a str> {
        let entry = controller.entry(index)?;
        entry.frame(name).map(|item| item.url.as_str())
    }

    /// A child frame's first document is a place in the current entry; a
    /// later one is a new entry, the current one's but for its place (and
    /// its frames'), its other frames' items shared with it; a replacement
    /// or a reload stays in its place.
    #[test]
    fn a_child_frame_s_documents_are_in_the_history() {
        let mut tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        // Its parent's first, then its own, as Chromium's `AUTO_SUBFRAME`s.
        for (id, frame, url) in [(2, 1, "one"), (3, 2, "two"), (4, 3, "three")] {
            commit_child(
                &mut controller,
                &tree,
                token(frame),
                id,
                NavigationType::Replace,
                url,
            );
        }
        assert_eq!(controller.entry_count(), 1);
        assert_eq!(controller.entry(0).map(|e| e.frame_count()), Some(4));
        assert_eq!(child_url(&controller, 0, "three"), Some("three"));
        // A frame whose parent has no place: none.
        tree.add_child(token(2), token(4), "four", false)
            .expect("made");
        tree.add_child(token(4), token(5), "five", false)
            .expect("made");
        commit_child(
            &mut controller,
            &tree,
            token(5),
            5,
            NavigationType::Replace,
            "five",
        );
        assert_eq!(child_url(&controller, 0, "five"), None);
        commit_child(
            &mut controller,
            &tree,
            token(5),
            50,
            NavigationType::NewEntry,
            "five",
        );
        assert_eq!(controller.entry_count(), 1);
        // Frame 1's next document: a new entry; frame 3's place went with
        // its document, frame 2's is the one before's.
        commit_child(
            &mut controller,
            &tree,
            token(1),
            6,
            NavigationType::NewEntry,
            "one2",
        );
        assert_eq!(controller.entry_count(), 2);
        assert_eq!(controller.offset_and_length(), (1, 2));
        assert_eq!(urls(&controller), ["a", "a"]);
        assert_eq!(child_url(&controller, 0, "one"), Some("one"));
        assert_eq!(child_url(&controller, 1, "one"), Some("one2"));
        assert_eq!(child_url(&controller, 1, "three"), None);
        let item = |controller: &NavigationController, index: usize, name| {
            controller
                .entry(index)
                .and_then(|e| e.frame(name))
                .map(|i| i.item)
        };
        assert_eq!(item(&controller, 0, "two"), item(&controller, 1, "two"));
        assert_eq!(item(&controller, 0, ""), item(&controller, 1, ""));
        // Frame 2's state changes in both.
        controller.set_manual_scroll_restoration(&tree, token(2), true);
        let manual = |controller: &NavigationController, index: usize, name| {
            controller
                .entry(index)
                .and_then(|e| e.frame(name))
                .is_some_and(|i| i.manual_scroll_restoration)
        };
        assert!(manual(&controller, 0, "two") && manual(&controller, 1, "two"));
        assert!(!manual(&controller, 1, ""));
        // A replacement, and a reload: the place, the entry kept. A reload
        // keeps the item and its state.
        commit_child(
            &mut controller,
            &tree,
            token(2),
            7,
            NavigationType::Reload,
            "two",
        );
        assert_eq!(item(&controller, 1, "two"), item(&controller, 0, "two"));
        assert!(manual(&controller, 1, "two"));
        // Frame 1's reload: frame 3's place goes with its document.
        commit_child(
            &mut controller,
            &tree,
            token(3),
            70,
            NavigationType::Replace,
            "three2",
        );
        assert_eq!(child_url(&controller, 1, "three"), Some("three2"));
        commit_child(
            &mut controller,
            &tree,
            token(1),
            71,
            NavigationType::Reload,
            "one2",
        );
        assert_eq!(child_url(&controller, 1, "three"), None);
        assert_eq!(child_url(&controller, 1, "one"), Some("one2"));
        commit_child(
            &mut controller,
            &tree,
            token(2),
            8,
            NavigationType::Replace,
            "two2",
        );
        assert_eq!(controller.entry_count(), 2);
        assert_eq!(child_url(&controller, 1, "two"), Some("two2"));
        assert_eq!(child_url(&controller, 0, "two"), Some("two"));
        let params = controller.entry_params(&tree, token(2), NavigationType::NewEntry);
        assert_eq!((params.index, params.length), (2, 3));
        // A frame gone: a new entry has no place for it.
        tree.remove(token(2)).expect("there");
        commit_child(
            &mut controller,
            &tree,
            token(1),
            9,
            NavigationType::NewEntry,
            "one3",
        );
        assert_eq!(child_url(&controller, 2, "two"), None);
        assert_eq!(child_url(&controller, 1, "two"), Some("two2"));
        // The main frame's reload: its new document's frames take their
        // places again.
        commit(&mut controller, &tree, 10, NavigationType::Reload, "a");
        assert_eq!(controller.entry(2).map(|e| e.frame_count()), Some(1));
        assert_eq!(controller.entry(1).map(|e| e.frame_count()), Some(3));
    }

    /// A child frame's navigation within its document: a new entry, the
    /// current one's but for its item (of the same document, with its
    /// mode; its frames' places kept), or, replacing, its item changed in
    /// every entry that has it. One with no place gets one.
    #[test]
    fn a_child_frame_s_navigations_within_its_document() {
        let tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        controller.did_commit_same_document(
            &tree,
            token(2),
            "about:blank#x".into(),
            true,
            Some(vec![2]),
        );
        assert_eq!(controller.entry_count(), 1);
        let frame = |controller: &NavigationController, index: usize, name: &str| {
            controller.entry(index).and_then(|e| e.frame(name).cloned())
        };
        let two = frame(&controller, 0, "two").expect("placed");
        assert_eq!(
            (two.url.as_str(), two.document, two.state),
            ("about:blank#x", 0, Some(vec![2]))
        );
        commit_child(
            &mut controller,
            &tree,
            token(1),
            2,
            NavigationType::Replace,
            "one",
        );
        commit_child(
            &mut controller,
            &tree,
            token(3),
            3,
            NavigationType::Replace,
            "three",
        );
        controller.set_manual_scroll_restoration(&tree, token(1), true);
        controller.did_commit_same_document(&tree, token(1), "one#y".into(), false, Some(vec![1]));
        assert_eq!(controller.entry_count(), 2);
        assert_eq!(controller.current_index(), Some(1));
        let one = frame(&controller, 1, "one").expect("there");
        assert_eq!(
            (
                one.url.as_str(),
                one.document,
                one.state,
                one.manual_scroll_restoration
            ),
            ("one#y", 2, Some(vec![1]), true)
        );
        assert_eq!(child_url(&controller, 0, "one"), Some("one"));
        assert_eq!(child_url(&controller, 1, "three"), Some("three"));
        assert_eq!(
            frame(&controller, 1, "two").map(|i| i.item),
            frame(&controller, 0, "two").map(|i| i.item)
        );
        // Frame 2's `replaceState`: both entries' item.
        controller.did_commit_same_document(&tree, token(2), "about:blank#z".into(), true, None);
        assert_eq!(child_url(&controller, 0, "two"), Some("about:blank#z"));
        assert_eq!(controller.entry_count(), 2);
        // Back, then a child frame's new entry: the one forward goes, and
        // its items.
        commit(
            &mut controller,
            &tree,
            4,
            NavigationType::HistoryIndex(0),
            "a",
        );
        controller.did_commit_same_document(&tree, token(3), "three#w".into(), false, None);
        assert_eq!(controller.entry_count(), 2);
        assert_eq!(child_url(&controller, 1, "one"), Some("one"));
        assert_eq!(controller.items.len(), 5);
    }

    /// Back or forward: the frames at another item than the entry's, from
    /// the main frame down, each by the item it committed last; a frame's
    /// commit of its item there makes that entry the current one. A new
    /// child frame's item is its parent's entry's.
    #[test]
    fn back_and_forward_move_the_frames_at_another_item() {
        let tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        for (id, frame, url) in [(2, 1, "one"), (3, 2, "two"), (4, 3, "three")] {
            commit_child(
                &mut controller,
                &tree,
                token(frame),
                id,
                NavigationType::Replace,
                url,
            );
        }
        commit_child(
            &mut controller,
            &tree,
            token(1),
            5,
            NavigationType::NewEntry,
            "one2",
        );
        controller.did_commit_same_document(&tree, token(2), "two#x".into(), false, None);
        let current =
            |controller: &NavigationController, frame| controller.current_place(&tree, frame);
        let none = |_| None;
        // Entry 1 from 2: frame 2 within its document.
        assert_eq!(
            controller.frames_to_navigate(&tree, 1, none),
            [(token(2), HistoryLoad::SameDocument)]
        );
        // Entry 0 from 2: frame 1 loads again (frame 3 with it), frame 2
        // within its document.
        assert_eq!(
            controller.frames_to_navigate(&tree, 0, none),
            [
                (token(1), HistoryLoad::DifferentDocument),
                (token(2), HistoryLoad::SameDocument)
            ]
        );
        // Frame 2 at entry 0's item already (a traversal half done): frame
        // 1 alone.
        let entry_0 = controller.entry(0).map(|e| e.entry.id).expect("there");
        let two = controller
            .item_at(&tree, token(2), 0)
            .expect("there")
            .clone();
        let committed = |frame| {
            (frame == token(2)).then_some(HistoryPlace {
                entry: entry_0,
                item: two.item,
                document: two.document,
            })
        };
        assert_eq!(
            controller.frames_to_navigate(&tree, 0, committed),
            [(token(1), HistoryLoad::DifferentDocument)]
        );
        // Frame 1's load of its item there: entry 0 the current one, its
        // item kept, the place of frame 3 under it too.
        controller.did_start(6, token(1), NavigationType::HistoryIndex(0));
        controller.did_commit(&tree, token(1), 6, "one".into());
        assert_eq!(controller.current_index(), Some(0));
        assert_eq!(controller.entry_count(), 3);
        assert_eq!(child_url(&controller, 0, "three"), Some("three"));
        assert_eq!(current(&controller, token(1)).map(|p| p.document), Some(6));
        // The main frame at another item: it alone.
        commit(&mut controller, &tree, 7, NavigationType::NewEntry, "b");
        assert_eq!(
            controller.frames_to_navigate(&tree, 0, none),
            [(MAIN, HistoryLoad::DifferentDocument)]
        );
        // The current entry: nothing to move.
        assert_eq!(controller.frames_to_navigate(&tree, 1, none), []);
        // A new child frame's item: in its parent's entry, if that is
        // there and has one.
        let entry_0_place = controller.item_for_new_child_frame(&tree, token(3), entry_0);
        assert_eq!(
            entry_0_place.map(|(index, item)| (index, item.url.as_str())),
            Some((0, "three"))
        );
        let entry_1 = controller.entry(1).map(|e| e.entry.id).expect("there");
        assert_eq!(
            controller.item_for_new_child_frame(&tree, token(3), entry_1),
            None
        );
        assert_eq!(
            controller.item_for_new_child_frame(&tree, token(3), 999),
            None
        );
    }

    /// A frame traversing within its document has its frames looked at
    /// too: the main frame and a child frame both within theirs.
    #[test]
    fn back_and_forward_move_the_frames_in_one_traversing() {
        let tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        commit_child(
            &mut controller,
            &tree,
            token(1),
            2,
            NavigationType::Replace,
            "one",
        );
        controller.did_commit_same_document(&tree, MAIN, "a#x".into(), false, None);
        controller.did_commit_same_document(&tree, token(1), "one#y".into(), false, None);
        assert_eq!(
            controller.frames_to_navigate(&tree, 0, |_| None),
            [
                (MAIN, HistoryLoad::SameDocument),
                (token(1), HistoryLoad::SameDocument)
            ]
        );
    }

    /// An entry whose frames that differ are gone moves nothing: it is the
    /// current one, nothing loaded.
    #[test]
    fn back_and_forward_to_an_entry_of_frames_gone_move_nothing() {
        let mut tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        commit_child(
            &mut controller,
            &tree,
            token(2),
            2,
            NavigationType::Replace,
            "two",
        );
        commit_child(
            &mut controller,
            &tree,
            token(2),
            3,
            NavigationType::NewEntry,
            "two2",
        );
        tree.remove(token(2)).expect("there");
        assert_eq!(controller.frames_to_navigate(&tree, 0, |_| None), []);
        controller.go_to_entry_in_place(0);
        assert_eq!(controller.current_index(), Some(0));
        assert_eq!(controller.entry_count(), 2);
    }

    /// An item of no document the browser committed (an initial
    /// `about:blank`'s) is loaded again; a frame the entry has no place
    /// for stays.
    #[test]
    fn back_and_forward_load_a_document_of_no_number_again() {
        let tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        controller.did_commit_same_document(&tree, token(2), "about:blank#x".into(), true, None);
        controller.did_commit_same_document(&tree, token(2), "about:blank#y".into(), false, None);
        controller.did_commit_same_document(&tree, token(1), "about:blank#z".into(), true, None);
        assert_eq!(
            controller.frames_to_navigate(&tree, 0, |_| None),
            [(token(2), HistoryLoad::DifferentDocument)]
        );
        // Its load: the other frames' items of no number stay so.
        controller.did_start(4, token(2), NavigationType::HistoryIndex(0));
        controller.did_commit(&tree, token(2), 4, "about:blank#x".into());
        let document = |index: usize, name| {
            controller
                .entry(index)
                .and_then(|e| e.frame(name))
                .map(|i| i.document)
        };
        assert_eq!(document(0, "two"), Some(4));
        assert_eq!(document(1, "one"), Some(0));
        assert_eq!(document(1, "two"), Some(0));
    }

    /// What no entry has a place for goes: an entry gone, a place another
    /// item took.
    #[test]
    fn items_go_with_their_last_place() {
        let tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        commit_child(
            &mut controller,
            &tree,
            token(1),
            2,
            NavigationType::Replace,
            "one",
        );
        commit_child(
            &mut controller,
            &tree,
            token(1),
            3,
            NavigationType::Replace,
            "one2",
        );
        assert_eq!(controller.items.len(), 2);
        commit_child(
            &mut controller,
            &tree,
            token(1),
            4,
            NavigationType::NewEntry,
            "one3",
        );
        assert_eq!(controller.items.len(), 3);
        // Back to the first: its frames' places stay, to restore them from.
        commit(
            &mut controller,
            &tree,
            5,
            NavigationType::HistoryIndex(0),
            "a",
        );
        assert_eq!(child_url(&controller, 0, "one"), Some("one2"));
        commit(&mut controller, &tree, 6, NavigationType::NewEntry, "b");
        // `one3`'s entry went; `a` and `one2` are the first entry's.
        assert_eq!(controller.items.len(), 3);
        assert_eq!(urls(&controller), ["a", "b"]);
        // The main frame's new entry within its document keeps its frames'
        // places.
        commit_child(
            &mut controller,
            &tree,
            token(1),
            7,
            NavigationType::Replace,
            "one4",
        );
        controller.did_commit_same_document(&tree, MAIN, "b#x".into(), false, None);
        assert_eq!(urls(&controller), ["a", "b", "b#x"]);
        assert_eq!(child_url(&controller, 2, "one"), Some("one4"));
    }

    /// A commit's place is as things are as it is sent: a child frame's
    /// new entry after the one the main frame's `pushState` made since its
    /// navigation started.
    #[test]
    fn a_commit_s_place_is_as_it_commits() {
        let tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        controller.did_start(2, token(1), NavigationType::NewEntry);
        controller.did_start(3, token(2), NavigationType::Replace);
        assert_eq!(controller.commit_place(2), Some((1, 2)));
        controller.did_commit_same_document(&tree, MAIN, "a#x".into(), false, None);
        assert_eq!(controller.commit_place(2), Some((2, 3)));
        assert_eq!(controller.commit_place(3), Some((1, 2)));
        assert_eq!(controller.commit_place(4), None);
    }

    /// A commit is the end of its frame's navigations started before it,
    /// not another frame's; the main frame's, of every frame's.
    #[test]
    fn a_commit_ends_its_frame_s_older_navigations() {
        let tree = frames();
        let mut controller = NavigationController::default();
        commit(&mut controller, &tree, 1, NavigationType::NewEntry, "a");
        controller.did_start(2, token(1), NavigationType::Replace);
        controller.did_start(3, token(2), NavigationType::Replace);
        controller.did_start(4, token(1), NavigationType::Replace);
        controller.did_commit(&tree, token(1), 4, "one".into());
        assert_eq!(controller.pending.keys().copied().collect::<Vec<_>>(), [3]);
        controller.did_start(5, token(2), NavigationType::Replace);
        controller.did_start(6, MAIN, NavigationType::NewEntry);
        controller.did_start(7, token(2), NavigationType::Replace);
        controller.did_commit(&tree, MAIN, 6, "b".into());
        assert_eq!(controller.pending.keys().copied().collect::<Vec<_>>(), [7]);
    }
}
