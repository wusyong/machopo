//! A window's `History` (https://html.spec.whatwg.org/#the-history-interface;
//! Blink's `History`), the navigations within a document, and their events,
//! the session
//! history is the browser's (the host gives where it is,
//! `dom::SessionHistory`); `pushState` and `replaceState` change the
//! document's URL and its history's state at once and tell the host; a
//! navigation to a fragment of the document does as HTML's "navigate to a
//! fragment" (its URL, its target element, a `null` state, `popstate` as
//! the call ends, `hashchange` in a task), any other the host does
//! (`FrameNavigation`, `Cx::request_navigation`).

use dom::{
    DomString, Event, EventFlags, FrameNavigation, History, NodeRef, NodeRoot, QualifiedName,
    TargetRoot, Window, names,
};
use elise::{Gc, Persistent};
use url::Url;

use crate::declare::TypeDesc;
use crate::interfaces::{HashChangeEventInit, PopStateEventInit};
use crate::typed::{ArgType, DomStr, Front, FrontTypes, OpFront, RetType, ScriptHost};
use crate::{ApiCx, ApiError, DomException, ExceptionName, dispatch};

/// Whether `document` is fully active: its window's, in its browsing
/// context still.
pub(crate) fn is_fully_active<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> bool {
    cx.window_of(document).is_some() && !cx.is_discarded(document)
}

/// `document`'s URL, parsed: none without one (or with one that doesn't
/// parse).
pub(crate) fn document_url<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> Option<Url> {
    Url::parse(&cx.document_url(document)?.to_str()).ok()
}

fn without_fragment(url: &Url) -> Url {
    let mut url = url.clone();
    url.set_fragment(None);
    url
}

/// HTML's navigate of `document`'s frame to `url` (absolute already), its
/// entry replacing the current one if `replace` (or if `url` is the
/// document's URL, as HTML's navigate makes it). To a fragment of the
/// document: navigated now (its URL, its target element, its history's
/// state `null`; `hashchange` queued if the fragment changed; `popstate`
/// as the call ends); else the host navigates.
pub(crate) fn navigate<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>, url: Url, replace: bool) {
    let current = document_url(cx, document);
    let replace = replace || current.as_ref() == Some(&url);
    if let Some(current) = current
        && url.fragment().is_some()
        && without_fragment(&url) == without_fragment(&current)
    {
        cx.set_document_url(document, url.as_str());
        update_target_element(cx, document, &url);
        if let Some(window) = cx.window_of(document) {
            cx.set_window_history_state(window, None);
            if current.fragment() != url.fragment() {
                cx.queue_hash_change(window, current.as_str().into(), url.as_str().into());
            }
        }
        if !replace {
            push_entry(cx, document);
        }
        cx.request_navigation(
            document,
            FrameNavigation::SameDocument {
                url: url.into(),
                replace,
                state: None,
                scroll: true,
                pop_state: true,
            },
        );
        return;
    }
    cx.request_navigation(
        document,
        FrameNavigation::To {
            url: url.into(),
            replace,
        },
    );
}

/// A new entry of `document` after the current one: the top-level
/// document's is in the session history (a child frame's isn't, as the
/// host keeps it).
fn push_entry<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) {
    if cx.frame_owner(document).is_none() {
        cx.push_session_history_entry();
    }
}

/// `document`'s target element for `url`'s fragment (HTML's; `:target`),
/// as scrolling to the fragment sets it: the element it indicates, or none
/// (no fragment, the top, nothing of that name).
pub fn update_target_element<'gc>(cx: &dom::Cx<'_, 'gc>, document: NodeRef<'gc>, url: &Url) {
    let target = url
        .fragment()
        .and_then(|fragment| indicated_element(cx, document, fragment));
    cx.set_target_element(document, target);
}

/// The element `fragment` indicates in `document` (HTML's "find a
/// potential indicated element"): the first with that `id` in tree order,
/// else the first `<a>` with that `name`; the fragment as it is, then
/// percent-decoded.
pub fn indicated_element<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    fragment: &str,
) -> Option<NodeRef<'gc>> {
    if fragment.is_empty() {
        return None;
    }
    let decoded = percent_decode(fragment);
    [fragment, decoded.as_str()].into_iter().find_map(|name| {
        cx.element_by_id(document, name).or_else(|| {
            let mut node = cx.first_child(document);
            while let Some(at) = node {
                if cx.qualified_name(at) == Some(QualifiedName::html(names::local::A))
                    && cx
                        .get_attribute(at, "name")
                        .is_some_and(|value| value.to_str() == name)
                {
                    return Some(at);
                }
                node = cx.following_within(at, document);
            }
            None
        })
    })
}

/// `%XX` sequences decoded, then read as UTF-8 (lossily).
fn percent_decode(text: &str) -> String {
    let mut rest = text.as_bytes();
    let mut out = Vec::with_capacity(rest.len());
    while let Some((&first, after)) = rest.split_first() {
        let decoded = (first == b'%')
            .then(|| after.split_first_chunk::<2>())
            .flatten()
            .and_then(|(hex, after)| {
                let hex = std::str::from_utf8(hex).ok()?;
                Some((u8::from_str_radix(hex, 16).ok()?, after))
            });
        match decoded {
            Some((byte, after)) => {
                out.push(byte);
                rest = after;
            }
            None => {
                out.push(first);
                rest = after;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// --- `History`

/// The history's document if it is fully active; a `SecurityError`
/// otherwise, as most of its members throw.
fn fully_active_document<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, History>,
) -> Result<NodeRef<'gc>, ApiError> {
    let document = cx.window_document(cx.history_window(this));
    if is_fully_active(cx, document) {
        Ok(document)
    } else {
        Err(ApiError::Dom(DomException {
            name: ExceptionName::Security,
            message: "the history's document isn't fully active".into(),
        }))
    }
}

/// `window.history`: the same object each time.
pub(crate) fn window_history<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Gc<'gc, History>, ApiError> {
    Ok(cx.window_history(this))
}

/// `length`: the session history's, as the host gave it.
pub(crate) fn length<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, History>) -> Result<u32, ApiError> {
    fully_active_document(cx, this)?;
    Ok(cx.session_history().length)
}

/// `scrollRestoration`.
pub(crate) fn scroll_restoration<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, History>,
) -> Result<DomStr<'gc>, ApiError> {
    fully_active_document(cx, this)?;
    let manual = cx.history_manual_scroll_restoration(this);
    Ok(DomStr::fresh(if manual { "manual" } else { "auto" }))
}

/// Setting `scrollRestoration` (an enumeration: another value ignored):
/// the current entry's mode, which the host keeps.
pub(crate) fn set_scroll_restoration<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, History>,
    value: &str,
) -> Result<(), ApiError> {
    let document = fully_active_document(cx, this)?;
    let manual = match value {
        "manual" => true,
        "auto" => false,
        _ => return Ok(()),
    };
    cx.set_history_manual_scroll_restoration(this, manual);
    cx.request_navigation(document, FrameNavigation::ScrollRestoration { manual });
    Ok(())
}

/// `history.state` as a result (`Front::finish_history_state`): the
/// current state's deserialization, made at the first ask and the same
/// object after.
pub struct HistoryState;

impl RetType for HistoryState {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = Gc<'gc, History>;
    crate::typed::no_prepared!();
    type Pending<F: crate::typed::FrontTypes> = elise::Persistent<History>;

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        history: Gc<'gc, History>,
    ) -> elise::Persistent<History> {
        elise::Persistent::new(cx.gc, history)
    }

    #[inline(always)]
    fn finish<F: crate::typed::Front>(front: &mut F, history: elise::Persistent<History>) {
        front.finish_history_state(history);
    }
}

/// `state`.
pub(crate) fn state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, History>,
) -> Result<Gc<'gc, History>, ApiError> {
    fully_active_document(cx, this)?;
    Ok(this)
}

/// `go(delta)`: 0 reloads; else the session history traverses, if the
/// entry `delta` away is in it as the host gave it.
pub(crate) fn go<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, History>,
    delta: i32,
) -> Result<(), ApiError> {
    let document = fully_active_document(cx, this)?;
    if delta == 0 {
        cx.request_navigation(document, FrameNavigation::Reload);
        return Ok(());
    }
    let session = cx.session_history();
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "a u32 and an i32 add up within an i64"
    )]
    let target = i64::from(session.index) + i64::from(delta);
    if (0..i64::from(session.length)).contains(&target) {
        cx.request_navigation(document, FrameNavigation::GoToOffset(delta));
    }
    Ok(())
}

pub(crate) fn back<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, History>) -> Result<(), ApiError> {
    go(cx, this, -1)
}

pub(crate) fn forward<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, History>) -> Result<(), ApiError> {
    go(cx, this, 1)
}

/// A state argument, serialized for storage before the operation (HTML's
/// `StructuredSerializeForStorage`, `Front::serialized_state_arg`): a
/// `DataCloneError` for what doesn't serialize.
pub struct SerializedState;

impl ArgType for SerializedState {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    type Taken<T: FrontTypes> = Vec<u8>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a [u8];

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Vec<u8>> {
        front.serialized_state_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Vec<u8>,
        _: usize,
    ) -> Result<&'a [u8], ApiError> {
        Ok(taken)
    }
}

/// `pushState(data, unused, url)`.
pub(crate) fn push_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, History>,
    data: &[u8],
    _unused: &str,
    url: Option<&str>,
) -> Result<(), ApiError> {
    push_or_replace(cx, this, data, url, false)
}

/// `replaceState(data, unused, url)`.
pub(crate) fn replace_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, History>,
    data: &[u8],
    _unused: &str,
    url: Option<&str>,
) -> Result<(), ApiError> {
    push_or_replace(cx, this, data, url, true)
}

/// HTML's shared history push/replace state steps (`data` serialized
/// already, as Blink's `History::pushState` does first), then its URL and
/// history update steps: the document's URL is `url` (resolved against
/// the document's), its history's state `data`, and the host told. No
/// scrolling, no events.
fn push_or_replace<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, History>,
    data: &[u8],
    url: Option<&str>,
    replace: bool,
) -> Result<(), ApiError> {
    let document = fully_active_document(cx, this)?;
    let current = document_url(cx, document)
        .unwrap_or_else(|| Url::parse("about:blank").expect("about:blank parses"));
    let security = |message: String| {
        ApiError::Dom(DomException {
            name: ExceptionName::Security,
            message,
        })
    };
    // `null` and the empty string both keep the document's URL; another
    // is parsed against its base URL.
    let base = cx
        .document_base_url(document)
        .unwrap_or_else(|| current.clone());
    let new_url = match url.filter(|url| !url.is_empty()) {
        None => current.clone(),
        Some(url) => match base.join(url) {
            Ok(parsed) if can_have_url_rewritten(&current, &parsed) => parsed,
            Ok(_) => {
                return Err(security(format!(
                    "a history state object with URL '{url}' cannot be created in a document with URL '{current}'"
                )));
            }
            Err(_) => return Err(security(format!("'{url}' is not a valid URL"))),
        },
    };
    cx.set_document_url(document, new_url.as_str());
    cx.set_history_state(this, Some(data.to_vec()));
    if !replace {
        push_entry(cx, document);
    }
    cx.request_navigation(
        document,
        FrameNavigation::SameDocument {
            url: new_url.into(),
            replace,
            state: Some(data.to_vec()),
            scroll: false,
            pop_state: false,
        },
    );
    Ok(())
}

/// HTML's "can have its URL rewritten": a document at `document` may take
/// `target` as its URL without loading it (Blink's
/// `CanChangeToUrlForHistoryApi`).
pub fn can_have_url_rewritten(document: &Url, target: &Url) -> bool {
    if document.scheme() != target.scheme()
        || document.username() != target.username()
        || document.password() != target.password()
        || document.host() != target.host()
        || document.port() != target.port()
    {
        return false;
    }
    if matches!(target.scheme(), "http" | "https") {
        return true;
    }
    if target.scheme() == "file" {
        return document.path() == target.path();
    }
    document.path() == target.path() && document.query() == target.query()
}

// --- Events

/// `new PopStateEvent(type, init)`.
pub(crate) fn construct_pop_state_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &PopStateEventInit<O::Any>,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = cx.create_event(
        names::intern(event_type),
        EventFlags {
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            trusted: false,
        },
        op.current_window(cx),
    );
    cx.set_pop_state_event_data(event, init.hasUAVisualTransition);
    if let Some(state) = init.state {
        let (edge, _) = cx.pop_state_event_data(event).expect("just made one");
        op.store_any(cx, edge, state);
    }
    Ok(event)
}

pub(crate) fn pop_state_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    Ok(cx
        .pop_state_event_data(this)
        .expect("the front end's checks: a PopStateEvent")
        .0)
}

pub(crate) fn has_ua_visual_transition<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<bool, ApiError> {
    Ok(cx
        .pop_state_event_data(this)
        .expect("the front end's checks: a PopStateEvent")
        .1)
}

/// `new HashChangeEvent(type, init)`.
pub(crate) fn construct_hash_change_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &HashChangeEventInit,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = cx.create_event(
        names::intern(event_type),
        EventFlags {
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            trusted: false,
        },
        op.current_window(cx),
    );
    cx.set_hash_change_event_data(
        event,
        DomString::new(&init.oldURL),
        DomString::new(&init.newURL),
    );
    Ok(event)
}

pub(crate) fn hash_change_old_url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    let (old, _) = cx
        .hash_change_event_data(this)
        .expect("the front end's checks: a HashChangeEvent");
    Ok(DomStr::Owned(old.clone()))
}

pub(crate) fn hash_change_new_url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    let (_, new) = cx
        .hash_change_event_data(this)
        .expect("the front end's checks: a HashChangeEvent");
    Ok(DomStr::Owned(new.clone()))
}

/// `popstate` at `window` now (trusted, not bubbling, not cancelable), its
/// state the history's (the same object as `history.state`).
pub fn fire_pop_state<H: ScriptHost>(host: &mut H, window: &Persistent<Window>) {
    let (event, history) = host.mutate(|cx| {
        let window = window.get(cx.gc);
        let event = cx.create_event(
            names::event::POPSTATE,
            EventFlags {
                trusted: true,
                ..EventFlags::default()
            },
            Some(window),
        );
        cx.set_pop_state_event_data(event, false);
        let history = cx.window_history(window);
        (
            Persistent::new(cx.gc, event),
            Persistent::new(cx.gc, history),
        )
    });
    host.copy_history_state(&event, &history);
    dispatch(host, &TargetRoot::Window(window.clone()), &event);
}

/// A queued `hashchange`'s task: fired at `document`'s window (trusted, not
/// bubbling, not cancelable).
pub(crate) fn run_hash_change<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    old_url: DomString,
    new_url: DomString,
) {
    let Some((window, event)) = host.mutate(|cx| {
        let window = cx.window_of(document.get(cx.gc))?;
        let event = cx.create_event(
            names::event::HASHCHANGE,
            EventFlags {
                trusted: true,
                ..EventFlags::default()
            },
            Some(window),
        );
        cx.set_hash_change_event_data(event, old_url, new_url);
        Some((
            Persistent::new(cx.gc, window),
            Persistent::new(cx.gc, event),
        ))
    }) else {
        return;
    };
    dispatch(host, &TargetRoot::Window(window), &event);
}

// --- The host's

/// What a navigation script asked for comes to, once the call is over: a
/// fragment's `popstate` fires, then the host is told (`ScriptHost::
/// navigate`).
pub(crate) fn update_navigations<H: ScriptHost>(host: &mut H) {
    for request in host.mutate(|cx| cx.take_navigation_requests()) {
        let Some((document, window)) = host.read(|cx| {
            let document = cx.node_by_id(request.document)?;
            let window = cx
                .window_of(document)
                .map(|window| Persistent::new(cx.gc, window));
            Some((cx.root(document), window))
        }) else {
            continue;
        };
        if let (
            FrameNavigation::SameDocument {
                pop_state: true, ..
            },
            Some(window),
        ) = (&request.navigation, &window)
        {
            fire_pop_state(host, window);
        }
        host.navigate(&document, request.navigation);
    }
}

/// The host's traversal to another entry of `document` (HTML's "update
/// document for history step application"; Blink's
/// `CommitSameDocumentNavigation` of a history navigation): its URL is
/// `url`, its history's state `state` and mode `manual`, its target
/// element the fragment's; `popstate` fires now; `hashchange` is queued if
/// the fragment changed. Whether the document has a window.
pub fn traverse_within_document<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    url: &str,
    state: Option<Vec<u8>>,
    manual: bool,
) -> bool {
    let Ok(new_url) = Url::parse(url) else {
        return false;
    };
    let window = host.mutate(|cx| {
        let document = document.get(cx.gc);
        let window = cx.window_of(document)?;
        let old_url = document_url(cx, document);
        cx.set_document_url(document, new_url.as_str());
        let history = cx.window_history(window);
        cx.set_history_state(history, state);
        cx.set_history_manual_scroll_restoration(history, manual);
        update_target_element(cx, document, &new_url);
        if let Some(old_url) = old_url
            && old_url.fragment() != new_url.fragment()
        {
            cx.queue_hash_change(window, old_url.as_str().into(), new_url.as_str().into());
        }
        Some(Persistent::new(cx.gc, window))
    });
    let Some(window) = window else {
        return false;
    };
    fire_pop_state(host, &window);
    true
}

/// A new document's entry's history state (`state`) and scroll
/// restoration mode (`manual`), as the host gives them at its commit
/// (HTML's "restore the history object state").
pub fn restore_history<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    state: Option<Vec<u8>>,
    manual: bool,
) {
    host.mutate(|cx| {
        let Some(window) = cx.window_of(document.get(cx.gc)) else {
            return;
        };
        if state.is_none() && !manual {
            cx.set_window_history_state(window, None);
            return;
        }
        let history = cx.window_history(window);
        cx.set_history_state(history, state);
        cx.set_history_manual_scroll_restoration(history, manual);
    });
}
