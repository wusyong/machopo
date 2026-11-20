//! `document.cookie` (https://html.spec.whatwg.org/#dom-document-cookie)
//! and `navigator.cookieEnabled`: the browser keeps the cookies, and the
//! host asks it (Blink's `CookieJar` through the frame's
//! `RestrictedCookieManager`), each read waiting for the answer. A write
//! is queued in the DOM in its operation and handed the host before the
//! call returns, with `localStorage`'s changes (`Cx::take_cookie_writes`):
//! a read after it, out of an operation, comes after it.

use dom::{Navigator, NodeRef, NodeRoot};
use elise::Persistent;

use crate::exception::{DomException, ExceptionName};
use crate::typed::{DomStr, ScriptHost};
use crate::{ApiCx, ApiError};

/// The host's side of cookies (Blink's `CookieJar` and its
/// `RestrictedCookieManager`), the engine's to call. `document` is the
/// DOM's number for the document; `url` its cookie URL.
pub trait CookieHost {
    /// The cookie string script sees at `url` (`GetCookiesString`),
    /// waiting for the answer.
    fn cookies(&self, document: dom::NodeId, url: &str) -> String;

    /// Sets `line` as script sets it at `url` (`SetCookieFromString`),
    /// without waiting: a later `cookies` sees it. Given the DOM's queued
    /// writes (`Cx::take_cookie_writes`) by the engine.
    fn set_cookie(&self, document: dom::NodeId, url: &str, line: &str);

    /// Whether the cookie policy lets the document have every cookie
    /// (`CookiesEnabledFor`), waiting for the answer.
    fn cookies_enabled(&self, document: dom::NodeId, url: &str) -> bool;
}

/// What a front end's script host asks its cookie host, waiting for the
/// answer (`ScriptHost::cookie_call`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieCall {
    Get,
    Enabled,
}

/// The answer to a `CookieCall`: `Get`'s string, `Enabled`'s flag; none
/// without a cookie host (no cookies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieReply {
    Cookies(String),
    Enabled(bool),
    None,
}

impl CookieCall {
    /// Asks `host` of `document`'s cookies at `url`.
    pub fn on(self, host: &dyn CookieHost, document: dom::NodeId, url: &str) -> CookieReply {
        match self {
            Self::Get => CookieReply::Cookies(host.cookies(document, url)),
            Self::Enabled => CookieReply::Enabled(host.cookies_enabled(document, url)),
        }
    }
}

/// Where `document`'s cookies are, for script.
enum Access {
    /// A cookie-averse document (HTML's): none in a browsing context, or
    /// its cookie URL not HTTP(S). Reads are empty, writes do nothing.
    Averse,
    /// Its origin is opaque (a sandboxed document's): a `SecurityError`.
    Opaque,
    /// Of its cookie URL: its own URL's, or for an `about:blank` or
    /// `about:srcdoc` document, its creator's (Blink's `CookieURL`,
    /// `DocumentInit::GetCookieUrl`), asked through `through`: itself, or
    /// that creator (Blink's initial empty document takes its creator's
    /// factories; an iframe's first `about:blank` has no loader of its own
    /// until its frame is handed the host, after the script that made it).
    At { url: String, through: NodeRoot },
}

fn access<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> Access {
    // In a browsing context: a window's, not discarded.
    let in_context = crate::history::is_fully_active(cx, document);
    let through = cx.origin_document(document);
    let url = cx
        .document_url(through)
        .and_then(|url| url::Url::parse(&url.to_str()).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    let (true, Some(url)) = (in_context, url) else {
        return Access::Averse;
    };
    if !cx
        .document_origin(document)
        .is_some_and(|origin| origin.is_tuple())
    {
        return Access::Opaque;
    }
    Access::At {
        url: url.into(),
        through: cx.root(through),
    }
}

/// Blink's message for the cookies of a document of an opaque origin.
fn opaque() -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::Security,
        message: "Access is denied for this document.".into(),
    })
}

/// `document.cookie`'s getter: the cookie string of its cookie URL, as
/// script sees it; empty for a cookie-averse document.
pub(crate) fn document_cookie<H: ScriptHost>(
    host: &mut H,
    this: NodeRoot,
) -> Result<DomStr<'static>, ApiError> {
    match host.read(|cx| access(cx, this.get(cx.gc))) {
        Access::Averse => Ok(DomStr::Fresh("".into())),
        Access::Opaque => Err(opaque()),
        Access::At { url, through } => Ok(DomStr::Fresh(
            match host.cookie_call(&through, &url, CookieCall::Get) {
                CookieReply::Cookies(cookies) => cookies.into(),
                _ => "".into(),
            },
        )),
    }
}

/// `document.cookie`'s setter: `value` received as a cookie of its cookie
/// URL, as script sets one (RFC 6265bis's "receive a cookie", non-HTTP),
/// queued for the host; nothing for a cookie-averse document.
pub(crate) fn set_document_cookie<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    match access(cx, this) {
        Access::Averse => Ok(()),
        Access::Opaque => Err(opaque()),
        Access::At { url, through } => {
            cx.queue_cookie_write(through.get(cx.gc), url, value.to_owned());
            Ok(())
        }
    }
}

/// `navigator.cookieEnabled` (Blink's `Document::CookiesEnabled`): whether
/// its document may have every cookie, by the cookie policy; false for a
/// navigator of no window, true for an opaque origin's (Blink's
/// "compatible behavior") and a cookie-averse document's URL.
pub(crate) fn cookie_enabled<H: ScriptHost>(
    host: &mut H,
    this: Persistent<Navigator>,
) -> Result<bool, ApiError> {
    let Some(document) = host.read(|cx| {
        let window = cx.navigator_window(this.get(cx.gc))?;
        Some(cx.root(cx.window_document(window)))
    }) else {
        return Ok(false);
    };
    match host.read(|cx| access(cx, document.get(cx.gc))) {
        Access::Averse | Access::Opaque => Ok(true),
        Access::At { url, through } => Ok(matches!(
            host.cookie_call(&through, &url, CookieCall::Enabled),
            CookieReply::Enabled(true)
        )),
    }
}
