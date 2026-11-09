//! A document's base URL (HTML's "document base URL"; Blink's
//! `Document::base_url_`, which `ProcessBaseElement` keeps): the `href` of
//! its first `<base>` with one, resolved against its fallback base URL
//! (its own URL; an `about:blank` or `about:srcdoc` child frame's
//! container's document's base URL), the fallback alone without one or
//! if it doesn't resolve or is a `data:` or `javascript:` URL. Worked out
//! at the first ask after something it depends on changed and kept: a
//! `<base>` connected, disconnected or its `href` changed (from the first
//! `<base>` made on: `form::BASES_SEEN`), the document's URL set.

use std::cell::RefCell;

use url::Url;

use crate::{Cx, NodeRef, QualifiedName, names};

/// A document's base URL as last worked out (`DocumentData::base_url`):
/// none until the first ask, or when it is out of date.
pub(crate) type BaseUrlCell = RefCell<Option<Option<Url>>>;

/// `fallback` with `href` resolved against it, unless that doesn't resolve
/// or is a `data:` or `javascript:` URL (Blink's `ProcessBaseElement`
/// drops those too).
fn frozen_base_url(fallback: &Url, href: &str) -> Url {
    fallback
        .join(href)
        .ok()
        .filter(|url| !matches!(url.scheme(), "data" | "javascript"))
        .unwrap_or_else(|| fallback.clone())
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether `node` is an HTML `<base>`.
    pub fn is_base(&self, node: NodeRef<'gc>) -> bool {
        self.qualified_name(node) == Some(QualifiedName::html(names::local::BASE))
    }

    /// A `<base>` was made: from now on what is connected, removed and
    /// changed is looked at for them.
    pub(crate) fn base_made(&self) {
        self.see(crate::form::BASES_SEEN);
    }

    /// `node` (connected, or disconnected) is a `<base>`, or its `href`
    /// changed: its document's base URL is worked out again at the next
    /// ask.
    pub(crate) fn base_changed(&self, node: NodeRef<'gc>) {
        if self.is_base(node) {
            self.base_url_stale(self.document_of(node));
        }
    }

    /// `document`'s base URL is out of date (its URL changed, a `<base>`).
    pub(crate) fn base_url_stale(&self, document: NodeRef<'gc>) {
        if let Some(data) = self.document_data(document) {
            *data.into_ref().base_url.get().borrow_mut() = None;
        }
    }

    /// `document`'s fallback base URL (HTML's): an iframe's srcdoc
    /// document's container's document's base URL, as it is now; an
    /// `about:blank` one's about base URL, its creator's as it was made;
    /// else its own URL. None if that doesn't parse.
    pub fn fallback_base_url(&self, document: NodeRef<'gc>) -> Option<Url> {
        let own = self
            .document_url(document)
            .and_then(|url| Url::parse(&url.to_str()).ok());
        match own.as_ref().and_then(about_path) {
            Some("srcdoc") => {
                if let Some(owner) = self.frame_owner(document) {
                    return self.document_base_url(self.document_of(owner));
                }
                if let Some(kept) = self.kept_about_base_url(document) {
                    return Some(kept);
                }
            }
            Some("blank") => {
                if let Some(kept) = self.kept_about_base_url(document) {
                    return Some(kept);
                }
            }
            _ => {}
        }
        own
    }

    /// The about base URL `document` kept as its iframe let it go, if any.
    fn kept_about_base_url(&self, document: NodeRef<'gc>) -> Option<Url> {
        self.document_data(document)?
            .into_ref()
            .about_base_url
            .borrow()
            .clone()
    }

    /// `document`'s iframe lets it go (removed, or navigated): a srcdoc
    /// one keeps its container's base URL as it is now, which a `Request`
    /// made in its realm still resolves against (an `about:blank` one kept
    /// its creator's as it was made: `copy_about_base_url`).
    pub(crate) fn keep_about_base_url(&self, document: NodeRef<'gc>) {
        let srcdoc = self
            .document_url(document)
            .and_then(|url| Url::parse(&url.to_str()).ok())
            .is_some_and(|url| about_path(&url) == Some("srcdoc"));
        let Some(owner) = self.frame_owner(document).filter(|_| srcdoc) else {
            return;
        };
        // An iframe moved into its own content document (and so removed)
        // has none to keep: its container is that document.
        let container = self.document_of(owner);
        if elise::Gc::ptr_eq(container, document) {
            return;
        }
        let base = self.document_base_url(container);
        if let Some(data) = self.document_data(document) {
            *data.into_ref().about_base_url.borrow_mut() = base;
        }
    }

    /// An `about:blank` content document of `owner`'s takes its about base
    /// URL (HTML's, its creator's base URL as it is made: kept, not read
    /// again as the creator's changes): its iframe's document's now.
    pub(crate) fn copy_about_base_url(&self, owner: NodeRef<'gc>, document: NodeRef<'gc>) {
        let blank = self
            .document_url(document)
            .and_then(|url| Url::parse(&url.to_str()).ok())
            .is_some_and(|url| about_path(&url) == Some("blank"));
        let container = self.document_of(owner);
        if !blank || elise::Gc::ptr_eq(container, document) {
            return;
        }
        let base = self.document_base_url(container);
        if let Some(data) = self.document_data(document) {
            *data.into_ref().about_base_url.borrow_mut() = base;
        }
    }

    /// `document`'s base URL; none if neither it nor its fallback parses
    /// (a document of no URL).
    pub fn document_base_url(&self, document: NodeRef<'gc>) -> Option<Url> {
        self.with_document_base_url(document, |base| base.cloned())
    }

    /// `url` parsed against `document`'s base URL (HTML's "encoding-parsing
    /// a URL" relative to it; UTF-8); as an absolute URL without one. None
    /// if it doesn't parse.
    pub fn resolve_url(&self, document: NodeRef<'gc>, url: &str) -> Option<Url> {
        self.with_document_base_url(document, |base| {
            Url::options().base_url(base).parse(url).ok()
        })
    }

    /// `f` with `document`'s base URL, worked out first if it isn't kept.
    pub fn with_document_base_url<R>(
        &self,
        document: NodeRef<'gc>,
        f: impl FnOnce(Option<&Url>) -> R,
    ) -> R {
        let href = self.document_url(document);
        // Kept for a document not at an `about:` URL: found with no parse
        // of its URL (an element's every `style` change with a `url()`
        // asks; the `about:` documents' below are worked out each time).
        let about = href.as_ref().is_some_and(|href| {
            href.to_str()
                .get(..6)
                .is_some_and(|scheme| scheme.eq_ignore_ascii_case("about:"))
        });
        if !about
            && let Some(data) = self.document_data(document)
            && let Some(kept) = &*data.into_ref().base_url.get().borrow()
        {
            return f(kept.as_ref());
        }
        let own = href.and_then(|url| Url::parse(&url.to_str()).ok());
        // A srcdoc or `about:blank` document's: its own `<base>` against
        // its fallback, else the fallback (HTML's document base URL). Not
        // kept: a srcdoc document's fallback follows its container's.
        if own
            .as_ref()
            .and_then(about_path)
            .is_some_and(|path| matches!(path, "blank" | "srcdoc"))
        {
            let fallback = self.fallback_base_url(document);
            return match (self.first_base_href(document), fallback) {
                (Some(href), Some(fallback)) => f(Some(&frozen_base_url(&fallback, &href))),
                (_, fallback) => f(fallback.as_ref()),
            };
        }
        let Some(data) = self.document_data(document) else {
            return f(own.as_ref());
        };
        let cell = data.into_ref().base_url.get();
        if let Some(kept) = &*cell.borrow() {
            return f(kept.as_ref());
        }
        let base = own.map(|fallback| match self.first_base_href(document) {
            Some(href) => frozen_base_url(&fallback, &href),
            None => fallback,
        });
        // Borrowed only here, never across a call out.
        let result = f(base.as_ref());
        *cell.borrow_mut() = Some(base);
        result
    }

    /// The `href` of `document`'s first `<base>` with one, in tree order
    /// (not in shadow trees).
    fn first_base_href(&self, document: NodeRef<'gc>) -> Option<String> {
        if !self.has_seen(crate::form::BASES_SEEN) {
            return None;
        }
        let mut node = self.first_child(document);
        while let Some(at) = node {
            if self.is_base(at)
                && let Some(href) = self.get_attribute(at, "href")
            {
                return Some(href.to_str().into_owned());
            }
            node = self.following_within(at, document);
        }
        None
    }
}

/// The path of an `about:` URL (HTML's "matches about:blank" and the
/// srcdoc document's URL look at it, not at the query or fragment): none
/// for another scheme, or one with a host or credentials.
fn about_path(url: &Url) -> Option<&str> {
    (url.scheme() == "about"
        && !url.has_host()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.path())
}
