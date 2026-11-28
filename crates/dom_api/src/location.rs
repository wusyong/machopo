//! A window's `Location` (https://html.spec.whatwg.org/#the-location-interface;
//! Blink's `Location`): its document's URL and its
//! parts (`url`'s `quirks`, URL's API steps), each setter navigating to a
//! copy of the URL changed so (HTML's Location-object navigate). A
//! worker's (HTML's `WorkerLocation`) has the getters alone, of its
//! global's URL.

use dom::{FrameNavigation, Location, NavigationInitiator, NodeRef};
use elise::Gc;
use url::{Url, quirks};

use crate::typed::{DomStr, OpFront};
use crate::{ApiCx, ApiError, DomException, ExceptionName};

/// The location's relevant document: its window's, if fully active; none
/// for a worker's.
fn relevant_document<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Location>) -> Option<NodeRef<'gc>> {
    let document = cx.window_document(cx.location_window(this)?);
    crate::history::is_fully_active(cx, document).then_some(document)
}

/// The location's URL: its relevant document's, `about:blank` without one
/// (or with one that doesn't parse); a worker's, its global's URL.
pub(crate) fn url_of<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Location>) -> Url {
    if let Some(global) = cx.location_worker(this) {
        return cx.worker_global_url(global).clone();
    }
    relevant_document(cx, this)
        .and_then(|document| crate::history::document_url(cx, document))
        .unwrap_or_else(blank)
}

fn blank() -> Url {
    Url::parse("about:blank").expect("about:blank parses")
}

/// HTML's Location-object navigate: the relevant document's frame
/// navigates to `url`, asked for by `by` (the incumbent realm, if known:
/// the realm of the most recent script on the stack, not the entry's the
/// URL is resolved against);
/// before the document is completely loaded (as without a user's gesture:
/// machopo has none) it replaces the current entry.
fn navigate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    url: Url,
    replace: bool,
    by: Option<Gc<'gc, dom::ExecutionContext>>,
) {
    let Some(document) = relevant_document(cx, this) else {
        return;
    };
    let replace = replace || !cx.is_completely_loaded(document);
    crate::history::navigate(cx, document, url, replace, initiator(document, by));
}

/// Who asks to navigate the location's `document`: the realm `by`, else
/// the document itself. The setters but `href`'s are the same origin's
/// alone: one of the document's origin asks, which it stands for.
fn initiator<'gc>(
    document: NodeRef<'gc>,
    by: Option<Gc<'gc, dom::ExecutionContext>>,
) -> NavigationInitiator<'gc> {
    by.map_or(
        NavigationInitiator::Document(document),
        NavigationInitiator::Context,
    )
}

/// A `SecurityError` if `by` may not navigate the location's document to
/// `url` (Blink's `Location::SetLocation`; `history::may_navigate`): a
/// `javascript:` URL from another origin. HTML's navigate does nothing
/// then; Chrome throws.
fn check_may_navigate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    url: &Url,
    by: Option<Gc<'gc, dom::ExecutionContext>>,
) -> Result<(), ApiError> {
    let Some(document) = relevant_document(cx, this) else {
        return Ok(());
    };
    if crate::history::may_navigate(cx, document, url, initiator(document, by)) {
        return Ok(());
    }
    Err(ApiError::Dom(DomException {
        name: ExceptionName::Security,
        message: format!(
            "The current window does not have permission to navigate the target frame to '{url}'."
        ),
    }))
}

fn get<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Location>, part: fn(&Url) -> &str) -> DomStr<'gc> {
    DomStr::Owned(part(&url_of(cx, this)).into())
}

/// A setter's steps: nothing without a relevant document; else `set`
/// changes a copy of the URL, which the frame navigates to unless `set`
/// says not to.
fn set<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Location>, set: impl FnOnce(&mut Url) -> bool) {
    if relevant_document(cx, this).is_none() {
        return;
    }
    let mut copy = url_of(cx, this);
    if set(&mut copy) {
        navigate(cx, this, copy, false, None);
    }
}

/// `url` parsed against the entry realm's document's base URL (HTML's
/// entry settings object's API base URL; Blink's `EnteredDOMWindow`), the
/// location's document's without one: a `SyntaxError` if it doesn't
/// parse.
fn parse_relative_to_entry<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    url: &str,
) -> Result<Url, ApiError> {
    let base = match op.current_context(cx) {
        Some(entry) => cx.context_base_url(entry),
        None => relevant_document(cx, this).and_then(|document| cx.document_base_url(document)),
    }
    .unwrap_or_else(|| url_of(cx, this));
    base.join(url).map_err(|_| {
        ApiError::Dom(DomException {
            name: ExceptionName::Syntax,
            message: format!("'{url}' is not a valid URL."),
        })
    })
}

/// `href` (and `toString()`): the URL serialized.
pub(crate) fn href<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::href))
}

/// Setting `href`, and `assign(url)`: navigates to `url` resolved against
/// the entry realm's document's URL.
pub(crate) fn set_href<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    url: &str,
) -> Result<(), ApiError> {
    if relevant_document(cx, this).is_none() {
        return Ok(());
    }
    let url = parse_relative_to_entry(op, cx, this, url)?;
    let by = op.incumbent_context(cx);
    check_may_navigate(cx, this, &url, by)?;
    navigate(cx, this, url, false, by);
    Ok(())
}

/// `replace(url)`: as `assign`, its entry replacing the current one.
pub(crate) fn replace<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    url: &str,
) -> Result<(), ApiError> {
    if relevant_document(cx, this).is_none() {
        return Ok(());
    }
    let url = parse_relative_to_entry(op, cx, this, url)?;
    let by = op.incumbent_context(cx);
    check_may_navigate(cx, this, &url, by)?;
    navigate(cx, this, url, true, by);
    Ok(())
}

/// `reload()`.
pub(crate) fn reload<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Location>) -> Result<(), ApiError> {
    if let Some(document) = relevant_document(cx, this) {
        cx.request_navigation(document, FrameNavigation::Reload);
    }
    Ok(())
}

/// `origin`: the origin's serialization.
pub(crate) fn origin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        quirks::origin(&url_of(cx, this)).as_str().into(),
    ))
}

pub(crate) fn protocol<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::protocol))
}

/// Setting `protocol`: a `SyntaxError` for what can't be a scheme; the
/// frame navigates only to an HTTP(S) URL.
pub(crate) fn set_protocol<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    value: &str,
) -> Result<(), ApiError> {
    let scheme = value.split(':').next().unwrap_or_default();
    let valid = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::Syntax,
            message: format!("'{value}' is not a valid scheme"),
        }));
    }
    set(cx, this, |url| {
        let _ = quirks::set_protocol(url, value);
        matches!(url.scheme(), "http" | "https")
    });
    Ok(())
}

pub(crate) fn host<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::host))
}

/// The setters of `host`, `hostname`, `port` and `pathname`: nothing for a
/// URL with an opaque path.
fn set_unless_opaque<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    apply: impl FnOnce(&mut Url),
) {
    set(cx, this, |url| {
        if url.cannot_be_a_base() {
            return false;
        }
        apply(url);
        true
    });
}

pub(crate) fn set_host<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    value: &str,
) -> Result<(), ApiError> {
    set_unless_opaque(cx, this, |url| {
        let _ = quirks::set_host(url, value);
    });
    Ok(())
}

pub(crate) fn hostname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::hostname))
}

pub(crate) fn set_hostname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    value: &str,
) -> Result<(), ApiError> {
    set_unless_opaque(cx, this, |url| {
        let _ = quirks::set_hostname(url, value);
    });
    Ok(())
}

pub(crate) fn port<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::port))
}

pub(crate) fn set_port<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    value: &str,
) -> Result<(), ApiError> {
    set_unless_opaque(cx, this, |url| {
        let _ = quirks::set_port(url, value);
    });
    Ok(())
}

/// `pathname`: the URL's path serialized (an opaque path as it is:
/// `about:blank`'s is `blank`).
pub(crate) fn pathname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::pathname))
}

pub(crate) fn set_pathname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    value: &str,
) -> Result<(), ApiError> {
    set_unless_opaque(cx, this, |url| quirks::set_pathname(url, value));
    Ok(())
}

pub(crate) fn search<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::search))
}

pub(crate) fn set_search<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        quirks::set_search(url, value);
        true
    });
    Ok(())
}

pub(crate) fn hash<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::hash))
}

/// Setting `hash`: the fragment (the empty one for "", unlike `URL`'s), a
/// leading `#` removed; nothing if it is the fragment already, none and
/// the empty one counting as the same (Blink's `Location::setHash`
/// compares them ignoring nullity, and WPT expects it).
pub(crate) fn set_hash<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Location>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        let before = url.fragment().unwrap_or_default().to_owned();
        url.set_fragment(Some(value.strip_prefix('#').unwrap_or(value)));
        url.fragment().unwrap_or_default() != before
    });
    Ok(())
}
