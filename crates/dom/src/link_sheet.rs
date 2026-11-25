//! The sheets of `<link>`s as their host fetches them (Blink's
//! `LinkStyle`'s resource, and `StyleEngine`'s pending sheets): which
//! `<link>`s asked already, the requests the host sent by number, and
//! what each blocks while it loads: its document's rendering (one not
//! alternate and not in a shadow tree), its document's parser's scripts
//! (one the parser made, of media that matched as it started: HTML's "a
//! style sheet that is blocking scripts"), and its document's `load`
//! (`Cx::add_load_request`). The `<link>`s whose sheets load are kept, in
//! their document's list, till their responses come: each gets its
//! `load` or `error` then, though script let go of it.

use elise::{Gc, HeapVector};

use crate::{ALREADY_STARTED, Cx, Dom, NodeId, NodeRef};

/// A `<link>`'s: its sheet was asked for in its document (the bit a
/// script's "already started" takes: a `<link>` is no script). Its
/// adoption into another document asks again there.
const SHEET_REQUESTED: u32 = ALREADY_STARTED;

/// A `<link>`'s sheet whose request the host sent (`Cx::sheet_sent`):
/// its element, its document, the URL asked for, and whether it blocks its
/// document's rendering and its parser's scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentSheet {
    pub link: NodeId,
    pub document: NodeId,
    pub url: String,
    pub blocks_render: bool,
    pub blocks_scripts: bool,
}

impl Dom {
    /// The `<link>`'s sheet whose request is `request_id`, if one is.
    pub fn sent_sheet(&self, request_id: u64) -> Option<SentSheet> {
        self.changes.sent_sheets.borrow().get(&request_id).cloned()
    }

    /// How many of the `<link>`s' requests sent are `document`'s.
    pub fn sent_sheets_of(&self, document: NodeId) -> usize {
        self.changes
            .sent_sheets
            .borrow()
            .values()
            .filter(|sent| sent.document == document)
            .count()
    }

    /// Whether a `<link>`'s sheet that blocks `document`'s rendering is
    /// loading (Blink's `StyleEngine::HaveRenderBlockingStylesheetsLoaded`).
    pub fn sheets_block_render(&self, document: NodeId) -> bool {
        self.changes
            .sent_sheets
            .borrow()
            .values()
            .any(|sent| sent.document == document && sent.blocks_render)
    }

    /// Whether a `<link>`'s sheet that blocks `document`'s parser's scripts
    /// was asked for and is loading: though script may have taken its
    /// `<link>` away meanwhile (`Cx::sheets_block_scripts`).
    pub fn sheets_pending_for_scripts(&self, document: NodeId) -> bool {
        self.changes
            .sent_sheets
            .borrow()
            .values()
            .any(|sent| sent.document == document && sent.blocks_scripts)
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether `link`'s sheet was asked for in its document.
    pub fn sheet_requested(&self, link: NodeRef<'gc>) -> bool {
        self.is_link_element(link) && self.flag(link, SHEET_REQUESTED)
    }

    /// `link`'s sheet asked for in its document: asked for once, whether or
    /// not a request went (an `href` that doesn't parse sends none).
    pub fn set_sheet_requested(&self, link: NodeRef<'gc>) {
        if self.is_link_element(link) {
            self.set_flag(link, SHEET_REQUESTED, true);
        }
    }

    /// `node`, adopted into another document: a `<link>`'s sheet is asked
    /// for there anew.
    pub(crate) fn link_adopted(&self, node: NodeRef<'gc>) {
        if self.flag(node, SHEET_REQUESTED) && self.is_link_element(node) {
            self.set_flag(node, SHEET_REQUESTED, false);
        }
    }

    /// The host sent `sent`'s request as `request_id`, for `link` of
    /// `document`: kept till its response comes, which `document`'s `load`
    /// waits for.
    pub fn sheet_sent(&self, link: NodeRef<'gc>, request_id: u64, sent: SentSheet) {
        let Some(document) = self.node_by_id(sent.document) else {
            return;
        };
        let data = self.ensure_document_data(document);
        let list = match data.pending_sheet_links.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                data.pending_sheet_links.set(self.gc, Some(list));
                list
            }
        };
        list.push(self.gc, link);
        self.add_load_request(document, request_id);
        self.changes
            .sent_sheets
            .borrow_mut()
            .insert(request_id, sent);
    }

    /// The `<link>`'s sheet whose request is `request_id`, its response
    /// come: no longer kept, and its document's `load` no longer waits
    /// for it. Its `<link>` too, unless it is gone.
    pub fn take_sent_sheet(&self, request_id: u64) -> Option<(SentSheet, Option<NodeRef<'gc>>)> {
        let sent = self.changes.sent_sheets.borrow_mut().remove(&request_id)?;
        let link = self.node_by_id(sent.link);
        if let Some(document) = self.node_by_id(sent.document) {
            self.remove_load_request(document, request_id);
            if let Some(list) = self
                .document_data(document)
                .and_then(|data| data.pending_sheet_links.get(self.gc))
                && let Some(link) = link
                && let Some(at) = (0..list.len()).find(|&at| {
                    list.get(self.gc, at)
                        .is_some_and(|kept| Gc::ptr_eq(kept, link))
                })
            {
                list.remove(self.gc, at);
            }
        }
        Some((sent, link))
    }

    /// `href`, of a `<link>` of `document`, asked for no sheet: it doesn't
    /// parse (or went nowhere). Its host hears of it with the document's
    /// load (`take_failed_sheets`).
    pub fn note_sheet_failed(&self, document: NodeRef<'gc>, href: String) {
        let data = self.ensure_document_data(document);
        let mut failed = data.failed_sheets.take();
        failed.push(href);
        data.failed_sheets.set(failed);
    }

    /// The `href`s of `document`'s `<link>`s that asked for no sheet
    /// (`note_sheet_failed`), taken.
    pub fn take_failed_sheets(&self, document: NodeRef<'gc>) -> Vec<String> {
        self.document_data(document)
            .map(|data| data.failed_sheets.take())
            .unwrap_or_default()
    }

    /// Whether a `<link>`'s sheet that blocks `document`'s parser's scripts
    /// is loading (HTML's "has a style sheet that is blocking scripts"): one
    /// stops blocking them once script took its `<link>` away from the
    /// document or made it no stylesheet, though it still loads (Blink's
    /// `StyleEngine::RemovePendingBlockingSheet`, as the link goes).
    pub fn sheets_block_scripts(&self, document: NodeRef<'gc>) -> bool {
        let id = self.node_id(document);
        let links: Vec<NodeId> = self
            .changes
            .sent_sheets
            .borrow()
            .values()
            .filter(|sent| sent.document == id && sent.blocks_scripts)
            .map(|sent| sent.link)
            .collect();
        links.into_iter().any(|link| {
            self.node_by_id(link).is_some_and(|link| {
                self.is_connected(link)
                    && self.containing_shadow_root(link).is_none()
                    && self.node_id(self.document_of(link)) == id
                    && self.stylesheet_link_href(link).is_some()
            })
        })
    }
}
