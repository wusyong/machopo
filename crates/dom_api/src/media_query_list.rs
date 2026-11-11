//! `matchMedia()` and `MediaQueryList` (https://drafts.csswg.org/cssom-view/#the-mediaquerylist-interface),
//! and `MediaQueryListEvent`: Blink's `MediaQueryList`, `MediaQueryMatcher`
//! and `MediaQueryListEvent`. `dom` keeps each list's queries and the
//! result it last had; `matches` evaluates them again (and keeps the
//! result, as Blink's `UpdateMatches`), and HTML's "evaluate media queries
//! and report changes", which the host runs in its rendering update, fires
//! `change` at each list of a document whose result is otherwise than it
//! last had, oldest first.

use std::sync::Arc;

use css::media::{MediaEnvironment, MediaQueryList};
use dom::{
    DomString, Event, EventFlags, EventTargetImpl, ListenerFlags, NodeRef, NodeRoot, TargetRef,
    TargetRoot, Window, names,
};
use elise::{Gc, Persistent};

use crate::interfaces::MediaQueryListEventInit;
use crate::typed::{Callback, DomStr, ScriptHost};
use crate::{ApiCx, ApiError, OpFront};

/// What media queries see for `document`: its frame's viewport and device
/// pixel ratio, as laid out last (none: a 0 by 0 one at 1x, for a
/// document no frame shows).
pub(crate) fn environment<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> MediaEnvironment {
    let viewport = cx.peek_layout(document, |layout| layout.viewport());
    MediaEnvironment {
        width: viewport.map_or(0.0, |viewport| viewport.size.width),
        height: viewport.map_or(0.0, |viewport| viewport.size.height),
        device_pixel_ratio: viewport.map_or(1.0, |viewport| viewport.device_scale_factor),
    }
}

/// `window.matchMedia(query)`: a new list of the window's document, with
/// the queries `query` parses into and their result now.
pub(crate) fn match_media<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    query: &str,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let document = cx.window_document(this);
    let queries = MediaQueryList::parse(query);
    let matches = queries.matches(&environment(cx, document));
    Ok(cx.create_media_query_list(document, Arc::new(queries), matches))
}

/// `media`: the queries, serialized.
pub(crate) fn media<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(
        cx.media_query_list_queries(this).to_css(),
    )))
}

/// The list's queries evaluated now, kept as its result: whether that
/// differs from the one it had.
fn update_matches<'gc>(cx: &ApiCx<'_, 'gc>, list: Gc<'gc, EventTargetImpl>) -> bool {
    let document = cx.media_query_list_document(list);
    let matches = cx
        .media_query_list_queries(list)
        .matches(&environment(cx, document));
    cx.set_media_query_list_matches(list, matches)
}

/// `matches`: the queries evaluated now (Blink's `UpdateMatches`).
pub(crate) fn matches<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<bool, ApiError> {
    let document = cx.media_query_list_document(this);
    let matches = cx
        .media_query_list_queries(this)
        .matches(&environment(cx, document));
    cx.set_media_query_list_matches(this, matches);
    Ok(matches)
}

/// `addListener(callback)`, legacy: `addEventListener('change',
/// callback)`. Its steps written out, not `event_target`'s called: another
/// caller of those keeps `addEventListener`'s own from being inlined into
/// its member (`bench.py calls`: 20 to 40 instructions more a call).
pub(crate) fn add_listener<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    callback: Option<Callback<O::Callback>>,
) -> Result<(), ApiError> {
    let Some(callback) = callback else {
        return Ok(());
    };
    let target = TargetRef::EventTarget(this);
    let event_type = names::event::CHANGE;
    if cx
        .find_listener(target, event_type, false, |edge| {
            op.same_callback(cx, edge, callback)
        })
        .is_none()
    {
        let flags = ListenerFlags {
            capture: false,
            once: false,
            passive: cx.default_passive(target, event_type),
        };
        let listener = cx.add_listener(target, event_type, flags);
        op.store_callback(cx, cx.callback_edge(listener), callback);
    }
    Ok(())
}

/// `removeListener(callback)`, legacy: `removeEventListener('change',
/// callback)` (written out, as `add_listener`).
pub(crate) fn remove_listener<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    callback: Option<Callback<O::Callback>>,
) -> Result<(), ApiError> {
    let Some(callback) = callback else {
        return Ok(());
    };
    let target = TargetRef::EventTarget(this);
    if let Some(listener) = cx.find_listener(target, names::event::CHANGE, false, |edge| {
        op.same_callback(cx, edge, callback)
    }) {
        cx.remove_listener(target, listener);
    }
    Ok(())
}

/// `new MediaQueryListEvent(type, eventInitDict)`: an event, as `new
/// Event`'s, with the init's `media` and `matches`.
pub(crate) fn construct_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &MediaQueryListEventInit,
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
    cx.set_media_query_list_event_data(event, &init.media, init.matches);
    Ok(event)
}

fn event_data<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> (&'gc DomString, bool) {
    cx.media_query_list_event_data(event)
        .expect("the front end's checks: a MediaQueryListEvent")
}

pub(crate) fn event_media<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(event_data(cx, this).0.clone()))
}

pub(crate) fn event_matches<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<bool, ApiError> {
    Ok(event_data(cx, this).1)
}

/// HTML's "evaluate media queries and report changes" for `document`: each
/// of its lists (oldest first) whose queries evaluate otherwise than they
/// last did gets a `change`, a `MediaQueryListEvent` of its queries and
/// result now. Nothing while its environment is the one they were last
/// evaluated in.
pub fn report_media_query_changes<H: ScriptHost>(host: &mut H, document: &NodeRoot) {
    let changed: Vec<(Persistent<EventTargetImpl>, Persistent<Event>)> = host.mutate(|cx| {
        let document = document.get(cx.gc);
        let environment = environment(cx, document);
        cx.media_query_lists_to_evaluate(document, environment)
            .into_iter()
            .filter(|&list| update_matches(cx, list))
            .map(|list| {
                let event = cx.create_event(
                    names::event::CHANGE,
                    EventFlags {
                        trusted: true,
                        ..EventFlags::default()
                    },
                    cx.window_of(document),
                );
                let media = cx.media_query_list_queries(list).to_css();
                let matches = cx.media_query_list_queries(list).matches(&environment);
                cx.set_media_query_list_event_data(event, &media, matches);
                (Persistent::new(cx.gc, list), Persistent::new(cx.gc, event))
            })
            .collect()
    });
    for (list, event) in changed {
        crate::event_target::dispatch(host, &TargetRoot::EventTarget(list), &event);
    }
}
