//! `Window` (https://html.spec.whatwg.org/#the-window-object): a realm's
//! global, its document and its `Location` (`location.rs`).

use dom::{Cx, Location, NodeRef, ObjectRef, Window, WindowNamed};
use elise::Gc;

use crate::declare::TypeDesc;
use crate::typed::{ArgType, Front, FrontTypes, OpFront};
use crate::{ApiCx, ApiError};

/// `window`, `self`, `frames`, `parent` and `top`: the window itself (no
/// frames yet), as its global proxy.
pub(crate) fn window<'gc>(
    _: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Gc<'gc, Window>, ApiError> {
    Ok(this)
}

/// `event`: the event whose listener of this realm runs (HTML's current
/// event), or `undefined`.
pub(crate) fn event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Option<Gc<'gc, dom::Event>>, ApiError> {
    Ok(cx.window_current_event(this))
}

pub(crate) fn document<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<NodeRef<'gc>, ApiError> {
    if cfg!(debug_assertions) {
        DOCUMENT_GETS.set(DOCUMENT_GETS.get().wrapping_add(1));
    }
    Ok(cx.window_document(this))
}

thread_local! {
    /// How many times, on this thread, a window's `document` getter ran.
    /// Counted in debug builds only.
    static DOCUMENT_GETS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many times, on this thread, a window's `document` getter ran: what
/// tells an engine that reads the global's `document` from a cache (V8's
/// cached accessor) calls it no more. `None` in release builds. For tests.
pub fn window_document_gets_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| DOCUMENT_GETS.get())
}

/// What `window`'s named properties object has under `name`, without
/// making anything (HTML's named access on the `Window` object, as Chrome
/// finds it; Blink's `WindowProperties::AnonymousNamedGetter`): the window
/// of the first child browsing context whose iframe's `name` it is; else
/// the element whose id it is (any element's) or whose `name` it is (an
/// HTML `embed`, `form`, `img` or `object`'s), if only one; else several,
/// which `named_property_collection` gives. Not in shadow trees.
pub fn named_property<'gc>(
    cx: &Cx<'_, 'gc>,
    window: Gc<'gc, Window>,
    name: &str,
) -> NamedProperty<'gc> {
    let document = cx.window_document(window);
    match cx.window_named(document, name) {
        WindowNamed::None => NamedProperty::None,
        WindowNamed::Frame(iframe) => cx
            .content_document(iframe)
            .and_then(|child| cx.window_of(child))
            .map_or(NamedProperty::None, |child| {
                NamedProperty::Object(ObjectRef::Window(child))
            }),
        WindowNamed::One(element) => NamedProperty::Object(ObjectRef::Node(element)),
        WindowNamed::Several => NamedProperty::Several,
    }
}

/// `named_property`'s answer.
pub enum NamedProperty<'gc> {
    None,
    Object(ObjectRef<'gc>),
    /// Several elements: a live `HTMLCollection` of them.
    Several,
}

/// `window`'s named objects under `name` when there are several: a live
/// `HTMLCollection`, the same each time.
pub fn named_property_collection<'gc>(
    cx: &Cx<'_, 'gc>,
    window: Gc<'gc, Window>,
    name: &str,
) -> ObjectRef<'gc> {
    let document = cx.window_document(window);
    ObjectRef::HtmlCollection(cx.window_named_collection(document, name))
}

/// `location`: the same object each time.
pub(crate) fn location<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Gc<'gc, Location>, ApiError> {
    Ok(cx.window_location(this))
}

/// A URL a `Location` navigates to: a string, resolved against the entry
/// realm's document's URL (HTML's entry settings object's; Blink's
/// `EnteredDOMWindow`): the caller's, not the location's own.
pub struct NavigationUrl;

impl ArgType for NavigationUrl {
    const DESC: TypeDesc = TypeDesc::String;
    type Taken<T: FrontTypes> = T::Str;
    type Param<'a, 'gc> = &'a str;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<F::Str> {
        let url = front.string_arg(index)?;
        front.prepare_entry_window();
        Some(url)
    }

    fn param<'a, 'gc, O: OpFront>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a O::Str,
        _: usize,
    ) -> Result<&'a str, ApiError> {
        Ok(op.str_param(taken))
    }
}

/// `document.location`: its window's location; none without a window.
pub(crate) fn document_location<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<Gc<'gc, Location>>, ApiError> {
    Ok(cx.window_of(this).map(|window| cx.window_location(window)))
}

/// `document.location`'s setter (`[PutForwards=href]`): a `TypeError`
/// without a window (WebIDL's: the forwarded-to value isn't an object).
pub(crate) fn set_document_location<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    url: &str,
) -> Result<(), ApiError> {
    match cx.window_of(this) {
        Some(window) => crate::location::set_href(op, cx, cx.window_location(window), url),
        None => Err(ApiError::Type("The document has no location.".into())),
    }
}

/// `window.location`'s setter (`[PutForwards=href]`).
pub(crate) fn set_location<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    url: &str,
) -> Result<(), ApiError> {
    crate::location::set_href(op, cx, cx.window_location(this), url)
}
