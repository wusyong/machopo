//! A `url()` (Blink's `CSSUrlData`): as written, which it serializes as,
//! and resolved against the base URL of where it was parsed, which it
//! computes to: a linked sheet's own URL, or the document's base URL when
//! a `<style>`, a `style` attribute, a presentation attribute or CSSOM's
//! text was parsed. The base URL changing later doesn't change it, as in
//! Chrome.

use std::sync::Arc;

/// What the `url()`s of where they were parsed resolve against, and who
/// they are fetched for (Blink's `CSSParserContext`'s base URL and
/// referrer): a linked sheet's own URL for both; the document's base URL
/// and the document's URL for a `<style>`, a `style` attribute, a
/// presentation attribute or CSSOM's text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UrlContext {
    pub base: url::Url,
    /// Without its fragment: what DevTools names as having asked for an
    /// image (its request's initiator).
    pub referrer: Arc<str>,
}

impl UrlContext {
    /// `base`, for `referrer`'s `url()`s (its fragment dropped).
    pub fn new(base: url::Url, referrer: &str) -> Self {
        let referrer = referrer.split('#').next().unwrap_or_default();
        Self {
            base,
            referrer: referrer.into(),
        }
    }

    /// A linked sheet's, at `url`.
    pub fn of_sheet(url: url::Url) -> Self {
        let referrer = url.as_str().to_owned();
        Self::new(url, &referrer)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CssUrlData {
    specified: Arc<str>,
    /// `None` until resolved, and if it doesn't resolve.
    absolute: Option<Arc<str>>,
    /// Who it is fetched for, once resolved (`UrlContext::referrer`).
    referrer: Option<Arc<str>>,
}

impl CssUrlData {
    /// As parsed: not resolved yet.
    pub(crate) fn new(specified: &str) -> Self {
        Self {
            specified: specified.into(),
            absolute: None,
            referrer: None,
        }
    }

    pub fn specified(&self) -> &Arc<str> {
        &self.specified
    }

    /// Resolves it in `context`, unless it was already.
    pub(crate) fn resolve(&mut self, context: &UrlContext) {
        if self.absolute.is_none() {
            self.absolute = context
                .base
                .join(&self.specified)
                .ok()
                .map(|url| url.as_str().into());
            self.referrer = Some(context.referrer.clone());
        }
    }

    /// Who it is fetched for, if it was resolved.
    pub(crate) fn referrer(&self) -> Option<&Arc<str>> {
        self.referrer.as_ref()
    }

    pub(crate) fn is_resolved(&self) -> bool {
        self.absolute.is_some()
    }

    /// What it computes to: the URL it resolved to; as written if it was
    /// parsed with no base URL or doesn't resolve, which the renderer then
    /// fails to load.
    pub(crate) fn computed(&self) -> Arc<str> {
        self.absolute.as_ref().unwrap_or(&self.specified).clone()
    }

    /// A fragment alone: an element of the same document, which stays as
    /// written (Blink's `CSSUrlData::IsLocal`).
    pub(crate) fn is_local(&self) -> bool {
        self.specified.starts_with('#')
    }

    /// Itself as if not resolved yet, for comparing what was written.
    pub(crate) fn as_written(&self) -> Self {
        Self::new(&self.specified)
    }
}
