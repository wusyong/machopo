//! A `<select>` shown as a menu list, as the user works it (Blink's
//! `MenuListSelectType::DefaultEventHandler` on Windows and Linux, and its
//! `ExternalPopupMenu`): the primary button's press opens its popup, or
//! closes it if it is open; Alt+↓, Alt+↑ and F4 (`keydown`), Space and
//! Enter (`keypress`) open it; the arrow keys, PageUp, PageDown, Home and
//! End select another option, firing `input` and `change`. The popup is
//! the host's: the select asks for it (`dom`'s `request_popup`), the
//! host shows it once no script runs, with its items (`popup_items`), and
//! hands the user's pick back (`popup_menu_closed`). One with `multiple`
//! opens none (Chrome's is another list). `showPicker()` opens it from
//! script, with transient activation.

use dom::names::local;
use dom::{NodeRef, NodeRoot, QualifiedName};
use elise::Gc;

use crate::typed::ScriptHost;
use crate::{ApiCx, ApiError, ExceptionName};

/// How far a menu list's PageUp and PageDown go (Blink's
/// `MenuListSelectType::DefaultEventHandler`).
const PAGE: i32 = 3;

/// The select `target` is, or whose UA shadow tree it is in (its inner
/// element is what the pointer is over), if it is a menu list that isn't
/// disabled.
fn menu_list<'gc>(cx: &ApiCx<'_, 'gc>, target: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    let select = if cx.is_select(target) {
        target
    } else {
        let root = cx.containing_shadow_root(target)?;
        let host = cx.shadow_host(root)?;
        (cx.user_agent_shadow_root(host)
            .is_some_and(|ua| Gc::ptr_eq(ua, root))
            && cx.is_select(host))
        .then_some(host)?
    };
    (cx.select_uses_menu_list(select) && !cx.is_disabled_form_control(select)).then_some(select)
}

/// A select's list items (Blink's `GetListItems`): its options, and the
/// optgroups and `<hr>`s among them, in tree order. The keys and the popup
/// count them.
pub(crate) fn list_items<'gc>(cx: &ApiCx<'_, 'gc>, select: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
    let mut items = Vec::new();
    let mut at = cx.first_child(select);
    while let Some(node) = at {
        let item = cx.is_option(node)
            || cx.is_optgroup(node)
            || cx.qualified_name(node) == Some(QualifiedName::html(local::HR));
        if item
            && cx
                .parent(node)
                .is_some_and(|parent| item_of(cx, parent, select))
        {
            items.push(node);
        }
        at = cx.following_within(node, select);
    }
    items
}

/// Whether an item whose parent is `parent` is one of `select`'s (HTML's
/// option element nearest ancestor select, for an optgroup and an `<hr>`
/// too).
fn item_of<'gc>(cx: &ApiCx<'_, 'gc>, parent: NodeRef<'gc>, select: NodeRef<'gc>) -> bool {
    cx.nearest_select(parent)
        .is_some_and(|of| Gc::ptr_eq(of, select))
}

/// The option `skip` list items from the one at `index` (-1 and the length
/// are before the first and after the last), forwards or backwards, that
/// the keys may go to (one that isn't disabled), or the last such option
/// before it (Blink's `SelectType::NextValidOption`).
pub(crate) fn next_valid_option<'gc>(
    cx: &ApiCx<'_, 'gc>,
    items: &[NodeRef<'gc>],
    mut index: i32,
    forwards: bool,
    mut skip: i32,
) -> Option<NodeRef<'gc>> {
    let step = if forwards { 1 } else { -1 };
    let mut last_good = None;
    // Saturating: only compared, with the list's indices and with 0.
    index = index.saturating_add(step);
    while let Some(item) = usize::try_from(index).ok().and_then(|i| items.get(i)) {
        skip = skip.saturating_sub(1);
        if cx.is_option(*item) && !cx.option_disabled(*item) {
            last_good = Some(*item);
            if skip <= 0 {
                break;
            }
        }
        index = index.saturating_add(step);
    }
    last_good
}

pub(crate) fn length(items: &[NodeRef<'_>]) -> i32 {
    i32::try_from(items.len()).unwrap_or(i32::MAX)
}

/// Whether a `keydown` opens a menu list's popup (Blink's
/// `ShouldOpenPopupForKeyDownEvent`): Alt+↓ or Alt+↑, or F4 without Alt.
fn opens_popup(key: &str, modifiers: u16) -> bool {
    let alt = modifiers & dom::modifiers::ALT != 0;
    let ctrl = modifiers & dom::modifiers::CTRL != 0;
    match key {
        "ArrowDown" | "ArrowUp" => alt && !ctrl,
        "F4" => !alt && !ctrl,
        _ => false,
    }
}

/// A `keydown` not canceled at `target`: a menu list opens its popup, or
/// its arrow keys select another option. Whether it took the key.
pub(crate) fn keydown<H: ScriptHost>(
    host: &mut H,
    target: &NodeRoot,
    key: &str,
    modifiers: u16,
) -> bool {
    let Some(select) =
        host.read(|cx| menu_list(cx, target.get(cx.gc)).map(|select| cx.root(select)))
    else {
        return false;
    };
    if opens_popup(key, modifiers) {
        show_popup(host, &select);
        return true;
    }
    let any =
        dom::modifiers::CTRL | dom::modifiers::SHIFT | dom::modifiers::ALT | dom::modifiers::META;
    if modifiers & any != 0 {
        return false;
    }
    let option = host.read(|cx| {
        let node = select.get(cx.gc);
        let items = list_items(cx, node);
        let index = cx
            .first_selected_option(node)
            .and_then(|selected| items.iter().position(|item| Gc::ptr_eq(*item, selected)))
            .map_or(-1, |index| i32::try_from(index).unwrap_or(i32::MAX));
        let option = match key {
            "ArrowDown" | "ArrowRight" => next_valid_option(cx, &items, index, true, 1),
            "ArrowUp" | "ArrowLeft" => next_valid_option(cx, &items, index, false, 1),
            "PageDown" => next_valid_option(cx, &items, index, true, PAGE),
            "PageUp" => next_valid_option(cx, &items, index, false, PAGE),
            "Home" => next_valid_option(cx, &items, -1, true, 1),
            "End" => next_valid_option(cx, &items, length(&items), false, 1),
            _ => return None,
        };
        Some(option.map(|option| cx.root(option)))
    });
    let Some(option) = option else {
        return false;
    };
    if let Some(option) = option {
        select_by_user(host, &select, &option);
    }
    true
}

/// A `keypress` not canceled at `target` (`text` its text): a menu list's
/// Space and Enter open its popup. Whether it took the key.
pub(crate) fn keypress<H: ScriptHost>(host: &mut H, target: &NodeRoot, text: &str) -> bool {
    if text != " " && text != "\r" {
        return false;
    }
    let Some(select) =
        host.read(|cx| menu_list(cx, target.get(cx.gc)).map(|select| cx.root(select)))
    else {
        return false;
    };
    show_popup(host, &select);
    true
}

/// A press of the primary button on `target` (its `mousedown` not
/// canceled, the focus moved): a menu list's popup opens, or closes if it
/// is open. Whether it took the press.
pub(crate) fn mouse_down<H: ScriptHost>(host: &mut H, target: &NodeRoot) -> bool {
    let Some(select) = host.read(|cx| {
        let select = menu_list(cx, target.get(cx.gc))?;
        // Its `focus` listeners may have taken it away.
        cx.is_connected(select).then(|| cx.root(select))
    }) else {
        return false;
    };
    if host.read(|cx| cx.select_popup_open(select.get(cx.gc))) {
        hide_popup(host, &select);
    } else {
        show_popup(host, &select);
    }
    true
}

/// `option` selected alone, dirty; if another was selected before, `input`
/// and `change` (Blink's `SelectOption` with
/// `kDispatchInputAndChangeEventFlag`).
fn select_by_user<H: ScriptHost>(host: &mut H, select: &NodeRoot, option: &NodeRoot) {
    let changed = host.mutate(|cx| {
        let node = select.get(cx.gc);
        let option = option.get(cx.gc);
        let changed = !cx
            .first_selected_option(node)
            .is_some_and(|before| Gc::ptr_eq(before, option));
        cx.select_only(node, Some(option));
        changed
    });
    if changed {
        crate::editing::fire_plain_input(host, select);
        crate::editing::fire_change(host, select);
    }
}

/// Asks for `select`'s popup, unless it is open or has `multiple` (Blink's
/// `MenuListSelectType::ShowPopup`): open from now on, which the host's
/// answer (`popup_menu_closed`) ends.
fn show_popup<H: ScriptHost>(host: &mut H, select: &NodeRoot) {
    host.mutate(|cx| {
        let node = select.get(cx.gc);
        if cx.select_popup_open(node) || cx.select_is_multiple(node) {
            return;
        }
        cx.set_select_popup_open(node, true);
        cx.request_popup(node, true);
    });
}

/// Closes `select`'s popup, if it is open, with no result (Blink's
/// `HidePopup`).
pub(crate) fn hide_popup<H: ScriptHost>(host: &mut H, select: &NodeRoot) {
    host.mutate(|cx| {
        let node = select.get(cx.gc);
        if cx.select_popup_open(node) {
            cx.set_select_popup_open(node, false);
            cx.request_popup(node, false);
        }
    });
}

/// What a popup's item is (the host's menu's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupItemKind {
    Option,
    Group,
    Separator,
}

/// An item of a select's popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupItem {
    pub label: String,
    pub kind: PopupItemKind,
    pub enabled: bool,
}

/// A label as the popup shows it: ASCII whitespace stripped and collapsed
/// (Blink's `HTMLOptionElement::DisplayLabel`).
fn popup_label(label: &str) -> String {
    label
        .split([' ', '\t', '\n', '\x0c', '\r'])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A menu list's popup's items (Blink's `ExternalPopupMenu::
/// GetPopupMenuInfo`): its list items, an option's display label, an
/// optgroup's `label`, an `<hr>` a separator; and the item of its first
/// selected option.
pub fn popup_items<'gc>(
    cx: &ApiCx<'_, 'gc>,
    select: NodeRef<'gc>,
) -> (Vec<PopupItem>, Option<u32>) {
    let selected = cx.first_selected_option(select);
    let mut chosen = None;
    let items = list_items(cx, select)
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            if cx.is_option(item) {
                if selected.is_some_and(|selected| Gc::ptr_eq(selected, item)) {
                    chosen = u32::try_from(index).ok();
                }
                PopupItem {
                    label: popup_label(&cx.option_display_label(item)),
                    kind: PopupItemKind::Option,
                    enabled: !cx.option_disabled(item),
                }
            } else if cx.is_optgroup(item) {
                let label = cx
                    .attribute_value_by_id(item, local::LABEL)
                    .map(|label| label.borrow().to_str().into_owned())
                    .unwrap_or_default();
                PopupItem {
                    label: popup_label(&label),
                    kind: PopupItemKind::Group,
                    enabled: cx.attribute_value_by_id(item, local::DISABLED).is_none(),
                }
            } else {
                PopupItem {
                    label: String::new(),
                    kind: PopupItemKind::Separator,
                    enabled: false,
                }
            }
        })
        .collect();
    (items, chosen)
}

/// How a select's popup closed: the user picked the item at `index` (with
/// a key or the pointer), or closed it with no pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupResult {
    Accepted { index: u32, by_keyboard: bool },
    Canceled,
}

/// `select`'s popup closed with `result` (the host passes on its menu's;
/// Blink's `ExternalPopupMenu::DidAcceptIndices` and `DidCancel`). A pick
/// of an option that can still be picked selects it (`input` and `change`
/// if another was selected), then a `mouseup` (not for a key's pick) and a
/// `click` at the select (Blink's legacy events, after the `change`).
pub fn popup_menu_closed<H: ScriptHost>(host: &mut H, select: &NodeRoot, result: PopupResult) {
    let open = host.mutate(|cx| {
        let node = select.get(cx.gc);
        let open = cx.select_popup_open(node);
        cx.set_select_popup_open(node, false);
        open
    });
    if !open {
        return;
    }
    let PopupResult::Accepted { index, by_keyboard } = result else {
        return;
    };
    let option = host.read(|cx| {
        let node = select.get(cx.gc);
        usize::try_from(index)
            .ok()
            .and_then(|index| list_items(cx, node).get(index).copied())
            .filter(|item| cx.is_option(*item) && !cx.option_disabled(*item))
            .map(|option| cx.root(option))
    });
    if let Some(option) = option {
        select_by_user(host, select, &option);
    }
    if !by_keyboard {
        crate::input::fire_plain_mouse(host, select, dom::names::event::MOUSEUP);
    }
    crate::input::fire_plain_mouse(host, select, dom::names::event::CLICK);
}

/// Whether `select` can keep its popup open: still in its document, not
/// disabled, still a menu list without `multiple`. The host closes one
/// that can't.
pub fn can_keep_popup<'gc>(cx: &ApiCx<'_, 'gc>, select: NodeRef<'gc>) -> bool {
    cx.is_connected(select)
        && !cx.is_disabled_form_control(select)
        && cx.select_uses_menu_list(select)
        && !cx.select_is_multiple(select)
}

/// `showPicker()` (HTML's, as Blink's `HTMLSelectElement::showPicker`
/// checks; machopo's DOM has no frames, so never cross-origin): an
/// `InvalidStateError` for a disabled select, a `NotAllowedError` without
/// transient activation, a `NotSupportedError` for one with no box; then
/// the activation is consumed (as Chrome does) and a menu list opens its
/// popup (a list box nothing).
pub(crate) fn show_picker<H: ScriptHost>(host: &mut H, this: NodeRoot) -> Result<(), ApiError> {
    let error = |name, message: &str| {
        ApiError::Dom(crate::exception::DomException {
            name,
            message: message.to_owned(),
        })
    };
    host.mutate(|cx| {
        let node = this.get(cx.gc);
        if cx.is_disabled_form_control(node) {
            return Err(error(
                ExceptionName::InvalidState,
                "showPicker() cannot be used on immutable controls.",
            ));
        }
        let window = cx.window_of(cx.document_of(node));
        if !window.is_some_and(|window| cx.has_transient_activation(window)) {
            return Err(error(
                ExceptionName::NotAllowed,
                "showPicker() requires a user gesture.",
            ));
        }
        let rendered = cx
            .with_layout(node, |layout| {
                layout.element_box(cx.node_id(node)).is_some()
            })
            .unwrap_or(false);
        if !rendered {
            return Err(error(
                ExceptionName::NotSupported,
                "showPicker() requires the select is rendered.",
            ));
        }
        crate::frame::consume_activation(cx, cx.document_of(node));
        Ok(())
    })?;
    if host.read(|cx| cx.select_uses_menu_list(this.get(cx.gc))) {
        show_popup(host, &this);
    }
    Ok(())
}
