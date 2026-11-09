//! A `<select>` shown as a list box, as the user works it (Blink's
//! `ListBoxSelectType::DefaultEventHandler` on Windows and Linux). The
//! primary button's press focuses it and selects the option under it
//! (with Shift, the range from the anchor; with Ctrl, Cmd on a Mac, and
//! `multiple`, one more or one less); moving with the button held (after a
//! press in it) moves the range's end, or with no `multiple` the option
//! selected; the release fires `input` and `change` if the selection
//! changed since the press. Its keys move the selection, and with
//! `multiple` and Ctrl only the end, which Space then selects or
//! unselects. What it keeps between events is `dom`'s
//! `ListBoxState`, the select's own.

use dom::{ListBoxState, NodeRef, NodeRoot, modifiers};
use elise::Gc;

use crate::ApiCx;
use crate::menu_list::{length, list_items, next_valid_option};
use crate::typed::ScriptHost;

/// A list box's rows without `size` (Blink's `kDefaultListBoxSize`).
const DEFAULT_LIST_BOX_SIZE: i32 = 4;

/// How a press or a key changes the selection (Blink's `SelectionMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    DeselectOthers,
    Range,
    NotChangeOthers,
}

/// Ctrl, or Cmd on a Mac: what adds an option to the selection (Blink's
/// `meta_or_ctrl`).
fn adds(modifiers: u16) -> bool {
    modifiers
        & if cfg!(target_os = "macos") {
            modifiers::META
        } else {
            modifiers::CTRL
        }
        != 0
}

/// The select `target` is, or whose option or UA shadow tree it is in, if
/// it is a list box that isn't disabled.
fn list_box<'gc>(cx: &ApiCx<'_, 'gc>, target: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    let select = std::iter::successors(Some(target), |node| cx.parent_or_shadow_host(*node))
        .find(|node| cx.is_select(*node))?;
    (!cx.select_uses_menu_list(select) && !cx.is_disabled_form_control(select)).then_some(select)
}

/// The option of `select`'s the event was dispatched at, or is in: up
/// through shadow hosts too (an option's label is in its UA shadow tree).
fn target_option<'gc>(
    cx: &ApiCx<'_, 'gc>,
    select: NodeRef<'gc>,
    target: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    std::iter::successors(Some(target), |node| cx.parent_or_shadow_host(*node))
        .take_while(|node| !Gc::ptr_eq(*node, select))
        .find(|node| cx.is_option(*node))
        .filter(|option| {
            cx.option_select(*option)
                .is_some_and(|of| Gc::ptr_eq(of, select))
        })
}

/// Each option's selectedness, in the list of options' order.
fn selection<'gc>(cx: &ApiCx<'_, 'gc>, select: NodeRef<'gc>) -> Vec<bool> {
    cx.list_of_options(select)
        .into_iter()
        .map(|option| cx.option_selectedness(option))
        .collect()
}

/// A list box's rows: its `size`, or 4 (Blink's `ListBoxSize`).
fn list_box_size<'gc>(cx: &ApiCx<'_, 'gc>, select: NodeRef<'gc>) -> i32 {
    cx.attribute_value_by_id(select, dom::names::local::SIZE)
        .and_then(|size| crate::reflect::parse_integer(&size.borrow().to_str()))
        .filter(|size| *size > 0)
        .unwrap_or(DEFAULT_LIST_BOX_SIZE)
}

fn index_of<'gc>(items: &[NodeRef<'gc>], option: Option<NodeRef<'gc>>) -> Option<i32> {
    let option = option?;
    let index = items.iter().position(|item| Gc::ptr_eq(*item, option))?;
    i32::try_from(index).ok()
}

/// The option a page away from `start` (Blink's
/// `NextSelectableOptionPageAway`): `size − 1` items, counted from the edge
/// the way goes from, so that it lands on one if it can.
fn page_away<'gc>(
    cx: &ApiCx<'_, 'gc>,
    items: &[NodeRef<'gc>],
    start: Option<NodeRef<'gc>>,
    forwards: bool,
    size: i32,
) -> Option<NodeRef<'gc>> {
    let page = size.saturating_sub(1);
    let start = index_of(items, start).unwrap_or(-1);
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "the length is at least 0 and `start` is -1 or an index in the list: \
                  `edge` is at least -1, and `edge - start` at most the length"
    )]
    let (edge, from_edge) = if forwards {
        (0, start)
    } else {
        let edge = length(items) - 1;
        (edge, edge - start)
    };
    next_valid_option(cx, items, edge, forwards, page.saturating_add(from_edge))
}

/// The range starts at `anchor`, the options' selectedness kept to go back
/// to (Blink's `SetActiveSelectionAnchor` and `SaveListboxActiveSelection`).
fn set_anchor<'gc>(
    cx: &ApiCx<'_, 'gc>,
    select: NodeRef<'gc>,
    state: &mut ListBoxState,
    anchor: Option<NodeRef<'gc>>,
) {
    state.anchor = anchor.map(|anchor| cx.node_id(anchor));
    state.anchor_selection = selection(cx, select);
}

/// Every option but `except` unselected, their dirtiness as it was
/// (Blink's `DeselectItemsWithoutValidation`).
fn deselect_others<'gc>(cx: &ApiCx<'_, 'gc>, select: NodeRef<'gc>, except: Option<NodeRef<'gc>>) {
    for option in cx.list_of_options(select) {
        if except.is_none_or(|except| !Gc::ptr_eq(except, option)) {
            cx.set_option_selectedness(option, false);
        }
    }
}

/// The options from the anchor to the end selected or unselected as the
/// range says, the others unselected or as they were when the anchor was
/// set; disabled ones untouched (Blink's `UpdateListBoxSelection`).
fn update_selection<'gc>(
    cx: &ApiCx<'_, 'gc>,
    select: NodeRef<'gc>,
    state: &ListBoxState,
    deselect: bool,
) {
    let options = cx.list_of_options(select);
    let index = |id: Option<gfx::NodeId>| {
        id.and_then(|id| options.iter().position(|option| cx.node_id(*option) == id))
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1)
    };
    let (anchor, end) = (index(state.anchor), index(state.end));
    let range = anchor.min(end)..=anchor.max(end);
    for (i, option) in options.iter().enumerate() {
        if cx.option_disabled(*option) {
            continue;
        }
        let at = i32::try_from(i).unwrap_or(i32::MAX);
        if range.contains(&at) {
            cx.set_option_selected_dirty(*option, state.range_selects);
        } else if deselect || i >= state.anchor_selection.len() {
            cx.set_option_selected_dirty(*option, false);
        } else {
            cx.set_option_selectedness(*option, state.anchor_selection[i]);
        }
    }
}

/// A press (or Space) on `clicked` (Blink's `ListBoxSelectType::
/// UpdateSelectedState`): the selection before kept to compare with, then
/// it, or the range to it, selected as `mode` says; one already selected
/// that Ctrl pressed is unselected, and so is the range dragged from it.
fn update_selected_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    select: NodeRef<'gc>,
    state: &mut ListBoxState,
    clicked: NodeRef<'gc>,
    mut mode: Mode,
) {
    state.change_selection = selection(cx, select);
    state.range_selects = true;
    if !cx.select_is_multiple(select) {
        mode = Mode::DeselectOthers;
    }
    if cx.option_selectedness(clicked) && mode == Mode::NotChangeOthers {
        state.range_selects = false;
        cx.set_option_selected_dirty(clicked, false);
    }
    if mode == Mode::DeselectOthers {
        deselect_others(cx, select, Some(clicked));
    }
    if state.anchor.is_none() && mode != Mode::NotChangeOthers {
        let first = cx.first_selected_option(select);
        set_anchor(cx, select, state, first);
    }
    if !cx.option_disabled(clicked) {
        cx.set_option_selected_dirty(clicked, true);
    }
    if state.anchor.is_none() || mode != Mode::Range {
        set_anchor(cx, select, state, Some(clicked));
    }
    state.end = Some(cx.node_id(clicked));
    update_selection(cx, select, state, mode != Mode::NotChangeOthers);
}

/// The selection compared with the one kept (Blink's `ListBoxOnChange`): if
/// they differ, the one now is kept and `input` and `change` fire; with
/// none kept, or one of another length (the options changed meanwhile),
/// only `change` fires.
fn on_change<H: ScriptHost>(host: &mut H, select: &NodeRoot) {
    let (fire, change_only) = host.mutate(|cx| {
        let node = select.get(cx.gc);
        let mut state = cx.list_box_state(node);
        let now = selection(cx, node);
        if state.change_selection.is_empty() || state.change_selection.len() != now.len() {
            (false, true)
        } else {
            let fire = state.change_selection != now;
            state.change_selection = now;
            cx.set_list_box_state(node, state);
            (fire, false)
        }
    });
    if change_only {
        crate::editing::fire_change(host, select);
    } else if fire {
        crate::editing::fire_plain_input(host, select);
        crate::editing::fire_change(host, select);
    }
}

/// The list box's end, or its last selected option, scrolled into view in
/// it once laid out (Blink's `ScrollToSelection` and `ScrollToOption`).
fn scroll_to_selection<'gc>(cx: &ApiCx<'_, 'gc>, select: NodeRef<'gc>) {
    let state = cx.list_box_state(select);
    let option = state.end.and_then(|end| cx.node_by_id(end)).or_else(|| {
        cx.list_of_options(select)
            .into_iter()
            .filter(|option| cx.option_selectedness(*option))
            .last()
    });
    if let Some(option) = option {
        let (container, element) = (cx.node_id(select), cx.node_id(option));
        cx.lend_layout(select, |layout| {
            layout.scroll_into_view_in(container, element)
        });
    }
}

/// A press of the primary button on `target` (its `mousedown` not
/// canceled, the focus moved), with `modifiers`: a list box selects the
/// option under it. Whether it took the press.
pub(crate) fn mouse_down<H: ScriptHost>(host: &mut H, target: &NodeRoot, modifiers: u16) -> bool {
    host.mutate(|cx| {
        let target = target.get(cx.gc);
        let Some(select) = list_box(cx, target) else {
            return false;
        };
        // Its `focus` listeners may have taken it away.
        if !cx.is_connected(select) {
            return false;
        }
        let Some(option) = target_option(cx, select, target) else {
            return false;
        };
        if !cx.option_disabled(option) {
            let mode = if modifiers & modifiers::SHIFT != 0 {
                Mode::Range
            } else if adds(modifiers) {
                Mode::NotChangeOthers
            } else {
                Mode::DeselectOthers
            };
            let mut state = cx.list_box_state(select);
            update_selected_state(cx, select, &mut state, option, mode);
            cx.set_list_box_state(select, state);
            scroll_to_selection(cx, select);
        }
        true
    })
}

/// The pointer moved over `target` with the primary button held: after a
/// press in it, a list box's range's end follows it (with no `multiple`,
/// the option selected).
pub(crate) fn mouse_moved<H: ScriptHost>(host: &mut H, target: &NodeRoot) {
    host.mutate(|cx| {
        let target = target.get(cx.gc);
        let Some(select) = list_box(cx, target) else {
            return;
        };
        let Some(option) = target_option(cx, select, target) else {
            return;
        };
        let mut state = cx.list_box_state(select);
        // The press wasn't in it.
        if state.change_selection.is_empty() {
            return;
        }
        if cx.select_is_multiple(select) {
            // Only a range from an anchor is extended.
            if state.anchor.is_none() {
                return;
            }
            state.end = Some(cx.node_id(option));
            update_selection(cx, select, &state, false);
        } else {
            set_anchor(cx, select, &mut state, Some(option));
            state.end = Some(cx.node_id(option));
            update_selection(cx, select, &state, true);
        }
        cx.set_list_box_state(select, state);
        scroll_to_selection(cx, select);
    });
}

/// The primary button went up over `target`: a list box pressed in fires
/// `input` and `change` if its selection changed since.
pub(crate) fn mouse_up<H: ScriptHost>(host: &mut H, target: &NodeRoot) {
    let select = host.read(|cx| {
        let select = list_box(cx, target.get(cx.gc))?;
        (!cx.list_box_state(select).change_selection.is_empty()).then(|| cx.root(select))
    });
    if let Some(select) = select {
        on_change(host, &select);
    }
}

/// A `keydown` not canceled at `target`, with `modifiers` (Blink's): ↓ and
/// ↑ go to the next option and the one before from the end (or, with
/// none, from the last and the first selected), PageDown and PageUp a page
/// away, Home and End to the first and the last; the option gone to is
/// selected alone, with Shift the range from the anchor to it, and with
/// `multiple` and Ctrl the end only moves; then `input` and `change` if
/// the selection changed. Ctrl+Space toggles the end (machopo gives it no
/// `keypress`; Chrome does). Whether it took the key.
pub(crate) fn keydown<H: ScriptHost>(
    host: &mut H,
    target: &NodeRoot,
    key: &str,
    modifiers: u16,
) -> bool {
    let Some(select) =
        host.read(|cx| list_box(cx, target.get(cx.gc)).map(|select| cx.root(select)))
    else {
        return false;
    };
    if key == " " && adds(modifiers) {
        return toggle_end(host, &select);
    }
    let changed = host.mutate(|cx| {
        let node = select.get(cx.gc);
        let mut state = cx.list_box_state(node);
        let items = list_items(cx, node);
        let size = list_box_size(cx, node);
        let selected: Vec<NodeRef<'_>> = cx
            .list_of_options(node)
            .into_iter()
            .filter(|o| cx.option_selectedness(*o))
            .collect();
        let end = state.end.and_then(|end| cx.node_by_id(end));
        // With no end, from the last selected going down and the first
        // going up.
        let from_down = end.or_else(|| selected.last().copied());
        let from_up = end.or_else(|| selected.first().copied());
        let option = match key {
            "ArrowDown" => next_valid_option(
                cx,
                &items,
                index_of(&items, from_down).unwrap_or(-1),
                true,
                1,
            ),
            "ArrowUp" => next_valid_option(
                cx,
                &items,
                index_of(&items, from_up).unwrap_or(length(&items)),
                false,
                1,
            ),
            "PageDown" => page_away(cx, &items, from_down, true, size),
            "PageUp" => page_away(cx, &items, from_up, false, size),
            "Home" => next_valid_option(cx, &items, -1, true, 1),
            "End" => next_valid_option(cx, &items, length(&items), false, 1),
            _ => return None,
        };
        let option = option?;
        let multiple = cx.select_is_multiple(node);
        let shift = modifiers & modifiers::SHIFT != 0;
        state.change_selection = selection(cx, node);
        state.end = Some(cx.node_id(option));
        state.non_contiguous = multiple && adds(modifiers);
        let select_new_item = !multiple || shift || !state.non_contiguous;
        if select_new_item {
            state.range_selects = true;
        }
        // Without Shift, the range starts again at the end.
        let deselect = !multiple || (!shift && select_new_item);
        if state.anchor.is_none() || deselect {
            if deselect {
                deselect_others(cx, node, None);
            }
            set_anchor(cx, node, &mut state, Some(option));
        }
        if select_new_item {
            update_selection(cx, node, &state, deselect);
        }
        cx.set_list_box_state(node, state);
        scroll_to_selection(cx, node);
        Some(select_new_item)
    });
    let Some(changed) = changed else {
        return false;
    };
    if changed {
        on_change(host, &select);
    }
    true
}

/// A `keypress` not canceled at `target` (`text` its text): a list box
/// takes Enter (it would submit the form implicitly: machopo's DOM doesn't
/// yet), and Space toggles the end Ctrl moved. Whether it took the key.
pub(crate) fn keypress<H: ScriptHost>(host: &mut H, target: &NodeRoot, text: &str) -> bool {
    let Some(select) =
        host.read(|cx| list_box(cx, target.get(cx.gc)).map(|select| cx.root(select)))
    else {
        return false;
    };
    match text {
        "\r" => true,
        " " => toggle_end(host, &select),
        _ => false,
    }
}

/// Space after Ctrl moved the end of a `multiple` list box: the end (or,
/// with none, the option after the last selected) selected or unselected,
/// the others as they were (Blink's `ToggleSelection`).
fn toggle_end<H: ScriptHost>(host: &mut H, select: &NodeRoot) -> bool {
    let toggled = host.mutate(|cx| {
        let node = select.get(cx.gc);
        let mut state = cx.list_box_state(node);
        if !(cx.select_is_multiple(node) && state.non_contiguous) {
            return false;
        }
        let option = state.end.and_then(|end| cx.node_by_id(end)).or_else(|| {
            let items = list_items(cx, node);
            let last = cx
                .list_of_options(node)
                .into_iter()
                .filter(|o| cx.option_selectedness(*o))
                .last();
            next_valid_option(cx, &items, index_of(&items, last).unwrap_or(-1), true, 1)
        });
        let Some(option) = option else {
            return false;
        };
        update_selected_state(cx, node, &mut state, option, Mode::NotChangeOthers);
        cx.set_list_box_state(node, state);
        scroll_to_selection(cx, node);
        true
    });
    if toggled {
        on_change(host, select);
    }
    toggled
}

/// `element` lost the focus (Blink's `ListBoxSelectType::DidBlur`): a
/// select's selection to compare with is forgotten, so that a drag that
/// didn't start with a press in it changes nothing.
pub(crate) fn did_blur<H: ScriptHost>(host: &mut H, element: &NodeRoot) {
    host.mutate(|cx| {
        let node = element.get(cx.gc);
        if !cx.is_select(node) {
            return;
        }
        let mut state = cx.list_box_state(node);
        if !state.change_selection.is_empty() {
            state.change_selection.clear();
            cx.set_list_box_state(node, state);
        }
    });
}
