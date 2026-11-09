//! HTML's `HTMLHyperlinkElementUtils`
//! (https://html.spec.whatwg.org/#htmlhyperlinkelementutils), the URL
//! members `<a>` and `<area>` share (Blink's `HTMLAnchorElement` and its
//! `DOMURLUtils`), their own members, and `<base>`'s. The element's URL is its `href` parsed against its node
//! document's base URL at each read (HTML's "set the url"): none without
//! an `href`, or when it doesn't parse. The getters read it with URL
//! Standard's (`url::quirks`); each setter changes a copy and writes it
//! back as the `href` ("update href"), or does nothing without a URL.

use dom::{DomString, Event, FrameNavigation, NodeRef, NodeRoot, QualifiedName, modifiers, names};
use elise::Persistent;
use url::{Url, quirks};

use crate::typed::{DomStr, ScriptHost};
use crate::{ApiCx, ApiError};

/// HTML's "set the url".
fn url<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Option<Url> {
    let href = cx.attribute_value_by_id(this, names::local::HREF)?;
    let href = href.borrow().to_str().into_owned();
    cx.resolve_url(cx.document_of(this), &href)
}

/// A part of the URL; the empty string without one.
fn get<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, part: fn(&Url) -> &str) -> DomStr<'gc> {
    DomStr::Owned(
        url(cx, this).map_or_else(|| DomString::new(""), |url| DomString::new(part(&url))),
    )
}

/// Changes the URL with `change` and writes it back as the `href` ("update
/// href"), unless there is no URL or `change` says not to (`false`).
fn set<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    change: impl FnOnce(&mut Url) -> bool,
) -> Result<(), ApiError> {
    let Some(mut url) = url(cx, this) else {
        return Ok(());
    };
    if change(&mut url) {
        crate::reflect::set_string::<{ names::local::HREF }>(cx, this, url.as_str())?;
    }
    Ok(())
}

/// `href`: the URL, or the attribute as written if it doesn't parse; the
/// empty string without one. The elements' stringifier too.
pub(crate) fn href<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(match url(cx, this) {
        Some(url) => DomStr::Owned(DomString::new(url.as_str())),
        None => crate::element::reflected(cx, this, names::local::HREF),
    })
}

/// `origin`: the URL's origin, serialized (`null` for an opaque one); the
/// empty string without a URL.
pub(crate) fn origin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(url(cx, this).map_or_else(
        || DomString::new(""),
        |url| DomString::new(&quirks::origin(&url)),
    )))
}

/// `protocol`: the scheme and `:`; `:` without a URL.
pub(crate) fn protocol<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(url(cx, this).map_or_else(
        || DomString::new(":"),
        |url| DomString::new(quirks::protocol(&url)),
    )))
}

/// Setting `protocol`: URL Standard's scheme state with an override, which
/// doesn't switch between special and other schemes (WPT's URL setter
/// tests are the spec's; Chrome switches).
pub(crate) fn set_protocol<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        let _ = quirks::set_protocol(url, value);
        true
    })
}

pub(crate) fn username<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::username))
}

/// Setting `username` (and `password`): nothing for a URL that can't have
/// one (no host, or `file:`).
pub(crate) fn set_username<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| quirks::set_username(url, value).is_ok())
}

pub(crate) fn password<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::password))
}

pub(crate) fn set_password<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| quirks::set_password(url, value).is_ok())
}

pub(crate) fn host<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::host))
}

/// Setting `host`, `hostname`, `port` and `pathname`: nothing for a URL
/// with an opaque path (`mailto:`).
pub(crate) fn set_host<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        !url.cannot_be_a_base() && {
            let _ = quirks::set_host(url, value);
            true
        }
    })
}

pub(crate) fn hostname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::hostname))
}

pub(crate) fn set_hostname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        !url.cannot_be_a_base() && {
            let _ = quirks::set_hostname(url, value);
            true
        }
    })
}

pub(crate) fn port<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::port))
}

/// Setting `port`: the empty string removes it.
pub(crate) fn set_port<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        !url.cannot_be_a_base() && quirks::set_port(url, value).is_ok()
    })
}

pub(crate) fn pathname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::pathname))
}

pub(crate) fn set_pathname<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        !url.cannot_be_a_base() && {
            quirks::set_pathname(url, value);
            true
        }
    })
}

pub(crate) fn search<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::search))
}

/// Setting `search` (and `hash`): its leading `?` (`#`) dropped; the empty
/// string removes it.
pub(crate) fn set_search<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        quirks::set_search(url, value);
        true
    })
}

pub(crate) fn hash<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(get(cx, this, quirks::hash))
}

pub(crate) fn set_hash<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set(cx, this, |url| {
        quirks::set_hash(url, value);
        true
    })
}

/// `<a>`'s `text`: its descendant text content, as `textContent`.
pub(crate) fn text<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        cx.text_content(this).unwrap_or_else(|| DomString::new("")),
    ))
}

/// Setting `text` is "string replace all": as `textContent`.
pub(crate) fn set_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_text_content(this, value);
    Ok(())
}

/// `<base>`'s `href`: its `href` (the empty string without one) parsed
/// against the document's fallback base URL (not its base URL, which it
/// may make), or as written if that fails.
pub(crate) fn base_href<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let value = cx
        .attribute_value_by_id(this, names::local::HREF)
        .map(|value| value.borrow().to_str().into_owned())
        .unwrap_or_default();
    let resolved = cx
        .fallback_base_url(cx.document_of(this))
        .and_then(|fallback| fallback.join(&value).ok());
    Ok(DomStr::Owned(DomString::new(
        resolved.as_ref().map_or(value.as_str(), Url::as_str),
    )))
}

// --- Following hyperlinks

/// Whether `node` is a hyperlink with an activation behavior: an `<a>` or
/// an `<area>` with an `href`, or SVG's `<a>` with one (`link_href`).
pub(crate) fn is_hyperlink<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    link_href(cx, node).is_some()
}

/// A hyperlink's `href`: an HTML `<a>`'s or `<area>`'s, or an SVG `<a>`'s,
/// or SVG 1.1's `xlink:href` without one (Blink's `SVGAElement`, which
/// follows its link as `HTMLAnchorElement` does).
fn link_href<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> Option<dom::StrRef<'gc>> {
    let name = cx.qualified_name(node)?;
    if name == QualifiedName::html(names::local::A)
        || name == QualifiedName::html(names::local::AREA)
    {
        return cx.attribute_value_by_id(node, names::local::HREF);
    }
    if name != QualifiedName::svg(names::local::A) {
        return None;
    }
    cx.attribute_value_by_id(node, names::local::HREF)
        .or_else(|| cx.attribute_value_ns(node, Some(XLINK_NAMESPACE), "href"))
}

/// The XLink namespace, of SVG 1.1's `xlink:href`.
const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";

/// The frame a hyperlink's `target` names, from `document`'s (HTML's
/// rules for choosing a navigable): none for one that opens a new tab
/// (`_blank`, or a name no frame has).
fn chosen_document<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    target: &str,
) -> Option<NodeRef<'gc>> {
    let parent =
        |document: NodeRef<'gc>| cx.frame_owner(document).map(|owner| cx.document_of(owner));
    let top = |mut document: NodeRef<'gc>| {
        while let Some(up) = parent(document) {
            document = up;
        }
        document
    };
    if target.is_empty() || target.eq_ignore_ascii_case("_self") {
        return Some(document);
    }
    if target.eq_ignore_ascii_case("_parent") {
        return Some(parent(document).unwrap_or(document));
    }
    if target.eq_ignore_ascii_case("_top") {
        return Some(top(document));
    }
    if target.eq_ignore_ascii_case("_blank") {
        return None;
    }
    // A name: this frame, its descendants, then the page's others in tree
    // order (Blink's `FrameTree::FindFrameForNavigation`).
    let named = |at: NodeRef<'gc>| {
        cx.frame_owner(at)
            .and_then(|owner| cx.attribute_value_by_id(owner, names::local::NAME))
            .is_some_and(|name| name.borrow().to_str() == target)
    };
    let in_tree = |root: NodeRef<'gc>| {
        let mut found = None;
        let mut stack = vec![root];
        while let Some(at) = stack.pop() {
            if named(at) {
                found = Some(at);
                break;
            }
            let mut children = cx.child_content_documents(at);
            children.reverse();
            stack.extend(children);
        }
        found
    };
    in_tree(document).or_else(|| in_tree(top(document)))
}

/// HTML's "follow the hyperlink" of `element` (an `<a>` or `<area>`),
/// clicked by `event` (Blink's `HTMLAnchorElement::HandleClick`): its URL
/// against its document's base URL, in the frame its `target` (or its
/// document's `<base>`'s) chooses; a new tab for one that opens one, if
/// the user clicked (script's clicks open none: with no user activation
/// to allow it, Chrome blocks such a popup), in the background for a
/// middle button or the platform's new-tab key, in the foreground with
/// Shift too or else. Not for one disconnected or of a document not fully
/// active; navigated before the document is completely loaded, without
/// the user, it replaces the entry. Recorded, as `location`'s are: the
/// engine does it once the call (or the input's dispatch) is over.
pub(crate) fn follow_hyperlink<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    event: &Persistent<Event>,
) {
    host.mutate(|cx| {
        let element = element.get(cx.gc);
        let document = cx.document_of(element);
        if !cx.is_connected(element) || !crate::history::is_fully_active(cx, document) {
            return;
        }
        let Some(url) = link_href(cx, element).and_then(|href| {
            let href = href.borrow().to_str().into_owned();
            cx.resolve_url(document, &href)
        }) else {
            return;
        };
        let target = cx
            .get_attribute(element, "target")
            .or_else(|| base_target(cx, document))
            .map(|target| target.to_str().trim().to_owned())
            .unwrap_or_default();
        let event = event.get(cx.gc);
        let trusted = cx.event_flags(event).trusted;
        let mouse = cx
            .mouse_event_data(event)
            .map(|data| cx.mouse_event_info(data));
        let new_tab_key = if cfg!(target_os = "macos") {
            modifiers::META
        } else {
            modifiers::CTRL
        };
        let by_keys = mouse.filter(|_| trusted).and_then(|mouse| {
            let shift = mouse.modifiers & modifiers::SHIFT != 0;
            if mouse.button == 1 || mouse.modifiers & new_tab_key != 0 {
                Some(!shift)
            } else {
                shift.then_some(false)
            }
        });
        let chosen = chosen_document(cx, document, &target);
        match (by_keys, chosen) {
            (Some(background), _) => cx.request_navigation(
                document,
                FrameNavigation::NewTab {
                    url: url.into(),
                    background,
                },
            ),
            (None, Some(chosen)) => {
                let replace = !trusted && !cx.is_completely_loaded(chosen);
                crate::history::navigate(cx, chosen, url, replace);
            }
            (None, None) if trusted => cx.request_navigation(
                document,
                FrameNavigation::NewTab {
                    url: url.into(),
                    background: false,
                },
            ),
            (None, None) => {}
        }
    });
}

/// The `target` of `document`'s first `<base>` with one (HTML's
/// "document's base target").
fn base_target<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> Option<DomString> {
    let mut node = cx.first_child(document);
    while let Some(at) = node {
        if cx.is_base(at)
            && let Some(target) = cx.get_attribute(at, "target")
        {
            return Some(target);
        }
        node = cx.following_within(at, document);
    }
    None
}
