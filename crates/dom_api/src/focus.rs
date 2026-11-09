//! HTML's focus (https://html.spec.whatwg.org/#focus) on the DOM: which
//! element of a document has the focus (`dom`'s, Blink's
//! `Document::focused_element_`), `focus()`, `blur()`, `tabIndex`,
//! `document.activeElement`, `hasFocus()`, the focus events, the page
//! taking or losing the system's focus, the focus fixup and `autofocus`.
//!
//! An element can take the focus if it is focusable and being rendered:
//! one with a valid `tabindex`, an `<a>` or `<area>` with an `href` (or
//! SVG's `<a>` with one), a form control, an editing host, with a box
//! once its frame is laid out that isn't `visibility: hidden` (Blink's
//! `Element::IsFocusable`). The
//! focus events go only while the page has the system's focus, and
//! `:focus` and the like match only then (Blink's
//! `Document::SetFocusedElement`).

use dom::names::{self, NameId, event, local};
use dom::{EventFlags, NodeKind, NodeRef, NodeRoot, TargetRef, TargetRoot, Window};
use elise::{Gc, Persistent};

use crate::interfaces::{FocusEventInit, FocusOptions};
use crate::typed::{OpFront, ScriptHost};
use crate::{ApiCx, ApiError, dispatch};

/// HTML's "rules for parsing integers": white space, a sign, then the
/// digits up to the first that isn't one (`10xyz` is 10), held to what an
/// `i32` holds. (`style_resolver`'s `hints.rs` has the same.)
#[expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "n is at most 2^31 before each digit, so n * 10 + 9 and -n fit an i64; \
              clamped to an i32's range before the cast"
)]
pub(crate) fn parse_integer(value: &str) -> Option<i32> {
    let value = value.trim_start_matches(['\t', '\n', '\x0c', '\r', ' ']);
    let (negative, rest) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let mut any = false;
    let mut n: i64 = 0;
    for digit in rest.bytes().take_while(u8::is_ascii_digit) {
        any = true;
        n = (n * 10 + i64::from(digit - b'0')).min(i64::from(i32::MAX) + 1);
    }
    if !any {
        return None;
    }
    let n = if negative { -n } else { n };
    Some(n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

fn is_html<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>, locals: &[NameId]) -> bool {
    cx.element_name(element)
        .is_some_and(|name| name.namespace == names::HTML_NAMESPACE && locals.contains(&name.local))
}

/// `element`'s `tabindex`, if it is a valid integer.
fn tabindex<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> Option<i32> {
    parse_integer(
        &cx.attribute_value_by_id(element, local::TABINDEX)?
            .borrow()
            .to_str(),
    )
}

/// Whether `element` is focusable, rendering aside: a valid `tabindex`, an
/// `<a>` or `<area>` with an `href`, a form control (an input whose type
/// isn't hidden), an iframe showing a document (HTML's navigable
/// container, Blink's `HTMLFrameOwnerElement::SupportsFocus`), an SVG
/// graphics element listening for focus events; none that
/// is a disabled form control (Blink's `SupportsFocus`,
/// `HTMLFormControlElement`'s too).
fn supports_focus<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    cx.kind(element) == NodeKind::Element
        && !cx.is_disabled_form_control(element)
        && (tabindex(cx, element).is_some()
            || focusable_by_default(cx, element)
            || cx.content_document(element).is_some()
            || has_focus_listeners(cx, element))
}

/// The elements that are `SVGGraphicsElement`s in Chrome.
const SVG_GRAPHICS: [NameId; 18] = [
    local::A,
    local::CIRCLE,
    local::DEFS,
    local::ELLIPSE,
    local::FOREIGN_OBJECT,
    local::G,
    local::IMAGE,
    local::LINE,
    local::PATH,
    local::POLYGON,
    local::POLYLINE,
    local::RECT,
    local::SVG,
    local::SWITCH,
    local::TEXT,
    local::TEXT_PATH,
    local::TSPAN,
    local::USE,
];

/// Whether `element` is an SVG graphics element with a `focus`, `blur`,
/// `focusin` or `focusout` listener: Blink lets one take the focus
/// (`SVGGraphicsElement::SupportsFocus`), though its `tabIndex` stays -1,
/// which keeps it out of sequential navigation.
fn has_focus_listeners<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    let graphics = cx.element_name(element).is_some_and(|name| {
        name.namespace == names::SVG_NAMESPACE && SVG_GRAPHICS.contains(&name.local)
    });
    graphics
        && cx
            .listeners(TargetRef::Node(element))
            .is_some_and(|listeners| {
                (0..listeners.len())
                    .filter_map(|index| listeners.get(cx.gc, index))
                    .any(|listener| {
                        [event::FOCUS, event::BLUR, event::FOCUSIN, event::FOCUSOUT]
                            .contains(&cx.listener_type(listener))
                    })
            })
}

/// HTML's "focusable area": focusable, in the document a frame shows, and
/// being rendered: it has a box (its frame laid out first) that isn't
/// `visibility: hidden` (Blink's `IsFocusableStyle`). An SVG element has
/// no CSS box; one that is rendered and not in a hidden container
/// (`<defs>`) has a client rect.
pub(crate) fn is_focusable_area<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    let svg = cx
        .qualified_name(element)
        .is_some_and(|name| name.namespace() == names::SVG_NAMESPACE);
    supports_focus(cx, element)
        && cx
            .with_layout(element, |layout| {
                (layout.element_box(cx.node_id(element)).is_some()
                    || (svg && layout.svg_geometry(cx, element).client_rect.is_some()))
                    && layout
                        .computed_style(cx, element)
                        .is_none_or(|style| style.is_visible())
            })
            .unwrap_or(false)
}

/// A trusted focus event of `event_type` at `target` (HTML's "fire a focus
/// event"): `focus` and `blur` don't bubble, `focusin` and `focusout` (and
/// the legacy `DOMFocusIn` and `DOMFocusOut`, which Chrome still fires) do;
/// composed, its `view` the window, its `relatedTarget` the element taking
/// or losing the focus. Then a microtask checkpoint.
fn fire_focus_event<H: ScriptHost>(
    host: &mut H,
    event_type: NameId,
    target: &TargetRoot,
    related: Option<&NodeRoot>,
) {
    let bubbles = matches!(
        event_type,
        event::FOCUSIN | event::FOCUSOUT | event::DOM_FOCUS_IN | event::DOM_FOCUS_OUT
    );
    let event = host.mutate(|cx| {
        let window = cx.target_window(target.get(cx.gc));
        let event = cx.create_event(
            event_type,
            EventFlags {
                bubbles,
                cancelable: false,
                composed: true,
                trusted: true,
            },
            window,
        );
        let related = related.map(|related| TargetRef::Node(related.get(cx.gc)));
        cx.set_focus_event_data(event, window, 0, related);
        Persistent::new(cx.gc, event)
    });
    dispatch(host, target, &event);
    host.perform_microtask_checkpoint();
}

fn focused_element<H: ScriptHost>(host: &mut H, document: &NodeRoot) -> Option<NodeRoot> {
    host.mutate(|cx| {
        cx.focused_element(document.get(cx.gc))
            .map(|element| cx.root(element))
    })
}

/// Whether `element` is `document`'s focused element.
fn has_focus_now<H: ScriptHost>(host: &mut H, document: &NodeRoot, element: &NodeRoot) -> bool {
    host.read(|cx| {
        cx.focused_element(document.get(cx.gc))
            .is_some_and(|focused| Gc::ptr_eq(focused, element.get(cx.gc)))
    })
}

fn same<H: ScriptHost>(host: &mut H, a: Option<&NodeRoot>, b: Option<&NodeRoot>) -> bool {
    host.read(|cx| match (a, b) {
        (Some(a), Some(b)) => Gc::ptr_eq(a.get(cx.gc), b.get(cx.gc)),
        (None, None) => true,
        _ => false,
    })
}

/// What an element taking the focus does to the selection (Blink's
/// `SelectionBehaviorOnFocus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionOnFocus {
    /// An editing host takes it to its start unless it is already in it
    /// (`kRestore`).
    Restore,
    /// It stays (`kNone`): the selection gave it the focus.
    Keep,
}

/// HTML's focusing steps for `element` in `document` (none: its viewport;
/// an iframe with a child frame: the child frame's viewport), as Blink's
/// `FocusController::SetFocusedElement`: if the focused frame is another
/// document's, it changes first (`change_focused_frame`); then
/// `document`'s focused element changes (`set_focused_element`). Whether
/// `element` (or its child frame's viewport) has the focus at the end.
/// `visible`: whether it shows a ring (none: as the window says).
pub(crate) fn focusing_steps<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    element: Option<&NodeRoot>,
    visible: Option<bool>,
) -> bool {
    focusing_steps_with(host, document, element, visible, SelectionOnFocus::Restore)
}

/// `focusing_steps`, the selection as `selection` says.
pub(crate) fn focusing_steps_with<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    element: Option<&NodeRoot>,
    visible: Option<bool>,
    selection: SelectionOnFocus,
) -> bool {
    // HTML's: a navigable container's focus target is its content
    // navigable's document.
    let child = element.and_then(|element| {
        host.read(|cx| {
            cx.content_document(element.get(cx.gc))
                .map(|child| cx.root(child))
        })
    });
    if let Some(child) = child {
        return focusing_steps_with(host, &child, None, visible, selection);
    }
    let (top, old) = host.read(|cx| {
        let top = cx.top_document(document.get(cx.gc));
        (cx.root(top), cx.root(cx.focused_frame(top)))
    });
    if !host.read(|cx| Gc::ptr_eq(old.get(cx.gc), document.get(cx.gc))) {
        change_focused_frame(host, &top, &old, document);
    }
    set_focused_element(host, document, element, visible, selection)
}

/// `window.focus()` (HTML's focusing steps for its navigable): its
/// document's frame the focused one of its tree, its focused element kept
/// (Chrome's `FocusController::FocusDocumentView`); nothing without a
/// browsing context. A top-level one's browser window isn't raised.
pub(crate) fn window_focus<H: ScriptHost>(
    host: &mut H,
    this: Persistent<Window>,
) -> Result<(), ApiError> {
    let frames = host.read(|cx| {
        let document = cx.window_document(this.get(cx.gc));
        if cx.is_discarded(document) {
            return None;
        }
        let top = cx.top_document(document);
        Some((
            cx.root(top),
            cx.root(cx.focused_frame(top)),
            cx.root(document),
        ))
    });
    let Some((top, old, new)) = frames else {
        return Ok(());
    };
    if !host.read(|cx| Gc::ptr_eq(old.get(cx.gc), new.get(cx.gc))) {
        change_focused_frame(host, &top, &old, &new);
    }
    Ok(())
}

/// The focused frame of `top`'s tree goes from `old`'s document to
/// `new`'s (Blink's `FocusController::SetFocusedFrame`): `old`'s focused
/// element loses the focus (`blur`, `focusout`), the iframes leading to it
/// stop being their documents' focused elements, and those leading to
/// `new` become them (no focus events of theirs: the page's
/// `activeElement` is the iframe, `:focus-within` goes through it); while
/// the page has the system's focus, `old`'s window gets `blur` and
/// `new`'s `focus`.
fn change_focused_frame<H: ScriptHost>(
    host: &mut H,
    top: &NodeRoot,
    old: &NodeRoot,
    new: &NodeRoot,
) {
    if focused_element(host, old).is_some() {
        set_focused_element(host, old, None, None, SelectionOnFocus::Restore);
    }
    host.mutate(|cx| {
        let (old, new) = (old.get(cx.gc), new.get(cx.gc));
        for owner in frame_owners(cx, old) {
            let document = cx.document_of(owner);
            if cx
                .focused_element(document)
                .is_some_and(|focused| Gc::ptr_eq(focused, owner))
            {
                cx.set_focused_element(document, None, false);
            }
        }
        for owner in frame_owners(cx, new) {
            cx.set_focused_element(cx.document_of(owner), Some(owner), false);
        }
        cx.set_focused_frame(top.get(cx.gc), new);
    });
    let windows = host.read(|cx| {
        let window = |document: &NodeRoot| {
            cx.window_of(document.get(cx.gc))
                .filter(|&window| cx.page_focused(window))
                .map(|window| Persistent::new(cx.gc, window))
        };
        (window(old), window(new))
    });
    if let Some(window) = windows.0 {
        fire_focus_event(host, event::BLUR, &TargetRoot::Window(window), None);
    }
    if let Some(window) = windows.1 {
        fire_focus_event(host, event::FOCUS, &TargetRoot::Window(window), None);
    }
}

/// The iframes leading to `document`, its own first, up to the top-level
/// document's.
fn frame_owners<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
    std::iter::successors(cx.frame_owner(document), |&owner| {
        cx.frame_owner(cx.document_of(owner))
    })
    .collect()
}

/// `document`'s focused element becomes `element` (none: its viewport),
/// as Blink's `Document::SetFocusedElement`: if the page has the system's
/// focus, the old one gets `blur`, `focusout` and `DOMFocusOut`, the new
/// one `focus`, `focusin` and `DOMFocusIn`, each with the other as
/// `relatedTarget`. A listener that moves the focus meanwhile wins: the
/// old one's events go on without a `relatedTarget`, the new one's stop,
/// and it doesn't take the focus. Whether `element` has the focus at the
/// end. `visible`: whether it shows a ring (none: as the window says). An
/// editing host taking it moves the selection as `selection` says, before
/// its `focus`, which sees the selection in it.
fn set_focused_element<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    element: Option<&NodeRoot>,
    visible: Option<bool>,
    selection: SelectionOnFocus,
) -> bool {
    let old = focused_element(host, document);
    if same(host, old.as_ref(), element) {
        return true;
    }
    let (focused, visible) = host.read(|cx| {
        let window = cx.window_of(document.get(cx.gc));
        (
            window.is_some_and(|window| cx.page_focused(window)),
            visible.unwrap_or_else(|| window.is_none_or(|window| cx.focus_visible(window))),
        )
    });
    let mut element = element;
    if let Some(old) = &old {
        // HTML's focus update steps: a text control commits the user's
        // change (`change`) before its `blur`.
        crate::editing::commit(host, old);
        crate::controls::did_blur(host, old);
        crate::menu_list::hide_popup(host, old);
        crate::list_box::did_blur(host, old);
        host.mutate(|cx| cx.set_focused_element(document.get(cx.gc), None, false));
        if focused {
            let target = TargetRoot::Node(old.clone());
            for event_type in [event::BLUR, event::FOCUSOUT, event::DOM_FOCUS_OUT] {
                fire_focus_event(host, event_type, &target, element);
                if focused_element(host, document).is_some() {
                    element = None;
                }
            }
            if element.is_none() {
                return false;
            }
        }
    }
    let Some(element) = element else {
        return true;
    };
    // A listener may have taken it out of the document.
    let still = host.read(|cx| {
        let node = element.get(cx.gc);
        cx.is_connected(node) && Gc::ptr_eq(cx.document_of(node), document.get(cx.gc))
    });
    if !still {
        return false;
    }
    host.mutate(|cx| {
        cx.set_focused_element(document.get(cx.gc), Some(element.get(cx.gc)), visible);
        if selection == SelectionOnFocus::Restore {
            crate::selection::focused(cx, element.get(cx.gc));
        }
    });
    if focused {
        let target = TargetRoot::Node(element.clone());
        for event_type in [event::FOCUS, event::FOCUSIN, event::DOM_FOCUS_IN] {
            fire_focus_event(host, event_type, &target, old.as_ref());
            if !has_focus_now(host, document, element) {
                return false;
            }
        }
    }
    true
}

/// `host`, an editing host the Selection API just put `document`'s
/// selection in (`selection::FocusHost`), takes the focus as Blink's
/// `FrameSelection::SetFocusedNodeIfNeeded` gives it: if it can (one not
/// rendered leaves the focus where it is: Blink's selection there is
/// none); the selection stays as it is, and nothing scrolls.
pub(crate) fn focus_selected_editing_host<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    element: &NodeRoot,
) {
    if !host.mutate(|cx| is_focusable_area(cx, element.get(cx.gc))) {
        return;
    }
    focusing_steps_with(host, document, Some(element), None, SelectionOnFocus::Keep);
}

/// `focus(options)`: an element that can have the focus takes it; one
/// that has it already does nothing, with no layout (Blink returns first:
/// a page calling `focus()` after each change would lay out each time).
/// `focusVisible` decides its ring when given. Unless `preventScroll`, it
/// is scrolled into view as Blink's `Element::UpdateSelectionOnFocus`
/// does, centered if none of it is in view, to its nearer edge if part of
/// it is (`CenterIfNeeded`): a text control or an editing host where its
/// caret now is (Blink's `FrameSelection::RevealSelection`: a tall editor
/// isn't scrolled to its edge), anything else its box.
pub(crate) fn focus<H: ScriptHost>(
    host: &mut H,
    this: NodeRoot,
    options: FocusOptions,
) -> Result<(), ApiError> {
    let document = host.mutate(|cx| cx.root(cx.document_of(this.get(cx.gc))));
    if has_focus_now(host, &document, &this) {
        return Ok(());
    }
    if !host.mutate(|cx| is_focusable_area(cx, this.get(cx.gc))) {
        return Ok(());
    }
    if !focusing_steps(host, &document, Some(&this), options.focusVisible) || options.preventScroll
    {
        return Ok(());
    }
    let params = gfx::ScrollIntoViewParams {
        block: gfx::ScrollAlignment::CenterIfNeeded,
        inline: gfx::ScrollAlignment::CenterIfNeeded,
        nearest_only: false,
    };
    host.mutate(|cx| {
        let (element, document) = (this.get(cx.gc), document.get(cx.gc));
        let reveals_caret =
            crate::editing::is_text_field(cx, element) || cx.is_editing_host(element);
        cx.with_layout(element, |layout| {
            // Without a caret there (a selection not collapsed), its box.
            if !(reveals_caret && layout.scroll_caret_into_view(cx, document, params)) {
                layout.scroll_into_view(cx, element, params);
            }
        });
    });
    Ok(())
}

/// `blur()`: HTML's unfocusing steps: if it is its document's focused
/// element, the viewport takes the focus (Blink's `Element::blur`).
pub(crate) fn blur<H: ScriptHost>(host: &mut H, this: NodeRoot) -> Result<(), ApiError> {
    let document = host.mutate(|cx| cx.root(cx.document_of(this.get(cx.gc))));
    if !has_focus_now(host, &document, &this) {
        return Ok(());
    }
    focusing_steps(host, &document, None, None);
    Ok(())
}

/// `tabIndex`: the `tabindex` attribute's value if it is a valid integer,
/// or else 0 for the elements HTML makes focusable by default (`<a>`,
/// `<area>`, the form controls, `<iframe>`...), -1 for the others.
pub(crate) fn tab_index<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    if let Some(value) = tabindex(cx, this) {
        return Ok(value);
    }
    let default = is_html(
        cx,
        this,
        &[
            local::A,
            local::AREA,
            local::BUTTON,
            local::FRAME,
            local::IFRAME,
            local::INPUT,
            local::OBJECT,
            local::SELECT,
            local::TEXTAREA,
        ],
    ) || cx.qualified_name(this) == Some(dom::QualifiedName::svg(local::A));
    Ok(if default { 0 } else { -1 })
}

pub(crate) fn set_tab_index<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: i32,
) -> Result<(), ApiError> {
    cx.set_attribute(this, "tabindex", &value.to_string());
    Ok(())
}

/// `document.activeElement`: its focused element; without one its body,
/// or its root element, or null.
pub(crate) fn active_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    // Retargeted against the document (DocumentOrShadowRoot's): one in a
    // shadow tree is its host here.
    Ok(cx
        .focused_element(this)
        .map(|focused| cx.retarget(focused, Some(this)))
        .or_else(|| crate::geometry::body(cx, this))
        .or_else(|| crate::geometry::document_element(cx, this)))
}

/// `document.hasFocus()`: the page has the system's focus and the document
/// is the focused frame's or one around it.
pub(crate) fn has_focus<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    if !cx
        .window_of(this)
        .is_some_and(|window| cx.page_focused(window))
    {
        return Ok(false);
    }
    // HTML's "has focus steps": in the focus chain, the focused frame's
    // document or one of its iframes' up.
    let mut at = Some(cx.focused_frame(cx.top_document(this)));
    while let Some(document) = at {
        if Gc::ptr_eq(document, this) {
            return Ok(true);
        }
        at = cx.frame_owner(document).map(|owner| cx.document_of(owner));
    }
    Ok(false)
}

/// `new FocusEvent(type, eventInitDict)`, its init's `view` a window or
/// null and its `relatedTarget` an event target or null.
pub(crate) fn construct_focus_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &FocusEventInit,
) -> Result<Gc<'gc, dom::Event>, ApiError> {
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
    let view = crate::ui_event::init_view(op, cx, init.view);
    let related = crate::ui_event::related_target_of(op, cx, init.relatedTarget);
    cx.set_focus_event_data(event, view, init.detail, related);
    Ok(event)
}

pub(crate) fn focus_related_target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, dom::Event>,
) -> Result<Option<TargetRef<'gc>>, ApiError> {
    Ok(cx
        .focus_event_data(this)
        .and_then(|data| cx.focus_related_target(data))
        .map(|related| crate::ui_event::retargeted_related(cx, this, related)))
}

/// The page took or lost the system's focus (the browser's `SetFocus`,
/// Blink's `FocusController::SetFocused`), `window` its top-level frame's:
/// every window of its frame tree knows, and the focused frame's focused
/// element and window get `blur` and `focusout`, or `focus` and `focusin`
/// (the window's first when taking it, last when losing it), and the
/// focus states follow.
pub fn set_page_focused<H: ScriptHost>(host: &mut H, window: &Persistent<Window>, focused: bool) {
    let frame = host.mutate(|cx| {
        let top = cx.window_document(window.get(cx.gc));
        if cx
            .window_of(top)
            .is_none_or(|window| cx.page_focused(window) == focused)
        {
            return None;
        }
        // Every window of the frame tree's: what their focus events and
        // states go by.
        for document in crate::documents_in_tree_order(cx, top) {
            if let Some(window) = cx.window_of(document) {
                cx.set_page_focused(window, focused);
            }
        }
        let frame = cx.focused_frame(top);
        let window = cx.window_of(frame)?;
        Some((
            Persistent::new(cx.gc, window),
            cx.focused_element(frame).map(|element| cx.root(element)),
        ))
    });
    let Some((window, element)) = frame else {
        return;
    };
    let window_target = TargetRoot::Window(window);
    let target = element.map(TargetRoot::Node);
    if focused {
        fire_focus_event(host, event::FOCUS, &window_target, None);
        if let Some(target) = &target {
            for event_type in [event::FOCUS, event::FOCUSIN, event::DOM_FOCUS_IN] {
                fire_focus_event(host, event_type, target, None);
            }
        }
    } else {
        if let Some(target) = &target {
            for event_type in [event::BLUR, event::FOCUSOUT, event::DOM_FOCUS_OUT] {
                fire_focus_event(host, event_type, target, None);
            }
        }
        fire_focus_event(host, event::BLUR, &window_target, None);
    }
}

/// HTML's focus fixup at the rendering update: if `document`'s focused
/// element can't have the focus any more (no `tabindex`, no box), its
/// viewport takes it, with `blur` and `focusout` (as Chrome). (One taken
/// out of the document lost it at once, with no events: `dom`'s
/// removing steps.)
pub fn focus_fixup<H: ScriptHost>(host: &mut H, document: &NodeRoot) {
    let Some(element) = focused_element(host, document) else {
        return;
    };
    if host.mutate(|cx| is_focusable_area(cx, element.get(cx.gc))) {
        return;
    }
    focusing_steps(host, document, None, None);
}

/// HTML's "autofocus" steps for `element`, which has the attribute and
/// became connected (`dom` records it): one in a fully active
/// top-level document, or a child frame's of the same origin, becomes a
/// candidate of the top-level document's, until its autofocus has run.
pub(crate) fn autofocus_inserted<H: ScriptHost>(host: &mut H, element: dom::NodeId) {
    host.mutate(|cx| {
        let Some(element) = cx.node_by_id(element) else {
            return;
        };
        let document = cx.document_of(element);
        if !cx.is_connected(element) || !crate::history::is_fully_active(cx, document) {
            return;
        }
        let top = cx.top_document(document);
        if cx.autofocus_processed(top)
            || (!Gc::ptr_eq(top, document) && !crate::frame::same_origin(cx, document, top))
        {
            return;
        }
        cx.add_autofocus_candidate(top, element);
    });
}

/// HTML's "flush autofocus candidates" of `top`, a top-level document, at
/// its rendering update: unless something has the focus already, the
/// first candidate still in a fully active document that can have the
/// focus takes it; then its autofocus has run.
pub fn flush_autofocus_candidates<H: ScriptHost>(host: &mut H, top: &NodeRoot) {
    let candidates = host.mutate(|cx| {
        let top = top.get(cx.gc);
        if !cx.has_autofocus_candidates(top) {
            return Vec::new();
        }
        // Something has the focus: autofocus is over.
        if cx.focused_element(cx.focused_frame(top)).is_some() {
            cx.set_autofocus_processed(top);
            return Vec::new();
        }
        cx.take_autofocus_candidates(top)
    });
    for candidate in candidates {
        let target = host.mutate(|cx| {
            let candidate = cx.node_by_id(candidate)?;
            let document = cx.document_of(candidate);
            (cx.is_connected(candidate)
                && crate::history::is_fully_active(cx, document)
                && is_focusable_area(cx, candidate))
            .then(|| (cx.root(document), cx.root(candidate)))
        });
        let Some((document, candidate)) = target else {
            continue;
        };
        host.mutate(|cx| cx.set_autofocus_processed(top.get(cx.gc)));
        focusing_steps(host, &document, Some(&candidate), None);
        return;
    }
}

/// Whether `element` is focusable and in sequential navigation without a
/// `tabindex`: an `<a>` or `<area>` with a link, or SVG's `<a>` with one
/// (Blink's `SVGAElement::SupportsFocus`), a form control (an input whose
/// type isn't hidden), an editing host (whose `tabIndex` is -1 all the
/// same, as HTML and Chrome have it).
fn focusable_by_default<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    crate::hyperlink::is_hyperlink(cx, element)
        || is_html(
            cx,
            element,
            &[local::BUTTON, local::SELECT, local::TEXTAREA],
        )
        || cx
            .input_kind(element)
            .is_some_and(|kind| kind != dom::InputKind::Hidden)
        || cx.is_editing_host(element)
}

/// `document`'s sequential focus navigation order (HTML's, Blink's
/// `FocusNavigation`): its elements with a positive `tabindex` by it
/// (tree order within each), then those with 0 or focusable by default,
/// in tree order; none with a negative one. An iframe showing a document
/// (0 without a `tabindex`) stands for that document's order (Blink's
/// `FocusNavigation` scopes).
fn navigation_order<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
    let mut entries: Vec<(i32, NodeRef<'gc>)> = Vec::new();
    let mut stack: Vec<NodeRef<'gc>> =
        std::iter::successors(cx.first_child(document), |&node| cx.next_sibling(node)).collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        if cx.kind(node) == NodeKind::Element {
            let frame = cx.content_document(node).is_some();
            let index = tabindex(cx, node)
                .or_else(|| (frame || focusable_by_default(cx, node)).then_some(0));
            if let Some(index) = index.filter(|&index| index >= 0) {
                entries.push((index, node));
            }
        }
        let first = stack.len();
        stack.extend(std::iter::successors(cx.first_child(node), |&child| {
            cx.next_sibling(child)
        }));
        stack[first..].reverse();
    }
    // Stable: tree order within each `tabindex`; 0 goes last.
    entries.sort_by_key(|&(index, _)| if index == 0 { i32::MAX } else { index });
    let mut order = Vec::new();
    for (_, element) in entries {
        match cx.content_document(element) {
            Some(child) => order.extend(navigation_order(cx, child)),
            None => order.push(element),
        }
    }
    order
}

/// HTML's sequential focus navigation through the frames of `document`'s
/// tree (Tab, Shift+Tab if `reverse`; Blink's `FocusController::
/// AdvanceFocus`): from the focused frame's focused element (with none, or
/// one not in the order, the start, the end if `reverse`), the next
/// element in the top-level document's order that can take the focus
/// takes it, in its document, shown with a ring. Whether there was one: if
/// not, the browser takes the focus back.
pub(crate) fn sequential_navigation<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    reverse: bool,
) -> bool {
    let (top, from) = host.read(|cx| {
        let top = cx.top_document(document.get(cx.gc));
        let from = cx
            .focused_element(cx.focused_frame(top))
            .map(|element| cx.root(element));
        (cx.root(top), from)
    });
    advance(host, &top, from.as_ref(), reverse)
}

/// `SetInitialFocus` (Blink's `WebViewImpl::SetInitialFocus`): Tab from
/// the browser's toolbar: the first element that can take the focus takes
/// it (its last, `reverse`), as if nothing had it.
pub fn set_initial_focus<H: ScriptHost>(host: &mut H, document: &NodeRoot, reverse: bool) -> bool {
    let top = host.read(|cx| cx.root(cx.top_document(document.get(cx.gc))));
    advance(host, &top, None, reverse)
}

fn advance<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    from: Option<&NodeRoot>,
    reverse: bool,
) -> bool {
    let candidate = host.mutate(|cx| {
        let mut order = navigation_order(cx, document.get(cx.gc));
        if reverse {
            order.reverse();
        }
        let from = from.map(|from| from.get(cx.gc));
        let start = from
            .and_then(|from| order.iter().position(|&element| Gc::ptr_eq(element, from)))
            // Past it: `index` < `order.len()`, so this is at most the length.
            .map_or(0, |index| index.saturating_add(1));
        order[start.min(order.len())..]
            .iter()
            .copied()
            .find(|&element| is_focusable_area(cx, element))
            .map(|element| cx.root(element))
    });
    let Some(candidate) = candidate else {
        return false;
    };
    // In its own document, a child frame's maybe.
    let document = host.mutate(|cx| {
        let document = cx.document_of(candidate.get(cx.gc));
        if let Some(window) = cx.window_of(document) {
            cx.set_focus_visible(window, true);
        }
        cx.root(document)
    });
    focusing_steps(host, &document, Some(&candidate), Some(true));
    true
}

/// What a press of the mouse does to the focus (Blink's `MouseEventManager::
/// HandleMouseFocus`), once its `mousedown` wasn't canceled: the nearest
/// element around `target` (in the flat tree) that can take the focus takes it, without a
/// ring (one that has it keeps it); with none, the document's viewport.
/// Script's next focus has no ring either (Chrome's `:focus-visible`
/// heuristic).
pub(crate) fn mouse_focus<H: ScriptHost>(host: &mut H, document: &NodeRoot, target: &NodeRoot) {
    let element = host.mutate(|cx| {
        if let Some(window) = cx.window_of(document.get(cx.gc)) {
            cx.set_focus_visible(window, false);
        }
        // Out of a shadow tree to its host (a text control's, whose UA
        // shadow tree's inner editor is what the mouse hits).
        std::iter::successors(Some(target.get(cx.gc)), |&node| cx.flat_tree_parent(node))
            .find(|&node| cx.kind(node) == NodeKind::Element && is_focusable_area(cx, node))
            .map(|element| cx.root(element))
    });
    focusing_steps(host, document, element.as_ref(), Some(false));
}

/// A key shows the focus ring from now on (Chrome's `:focus-visible`
/// heuristic: the keyboard was used).
pub(crate) fn key_shows_rings<H: ScriptHost>(host: &mut H, document: &NodeRoot) {
    host.mutate(|cx| {
        if let Some(window) = cx.window_of(document.get(cx.gc)) {
            cx.set_focus_visible(window, true);
        }
    });
}

/// A shadow root's `activeElement`: its document's focused element
/// retargeted against it, if that is in it (DocumentOrShadowRoot's).
pub(crate) fn shadow_active_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let Some(focused) = cx.focused_element(cx.document_of(this)) else {
        return Ok(None);
    };
    let candidate = cx.retarget(focused, Some(this));
    Ok(Gc::ptr_eq(cx.root_node(candidate, false), this).then_some(candidate))
}
