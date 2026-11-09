//! HTML's `<select>`, `<option>`, `HTMLOptionsCollection` and `<datalist>`
//! (https://html.spec.whatwg.org/#the-select-element; Blink's
//! `HTMLSelectElement`, `HTMLOptionElement`, `HTMLOptionsCollection`), and
//! the values of `<progress>`, `<meter>` and `<output>`: the members, on
//! `dom`'s `select` (an option's selectedness, a select's list of
//! options and the steps that keep them).

use dom::{DomString, HtmlCollection, NodeRef, QualName, names, number_to_string, parse_float};
use elise::Gc;

use crate::exception::DomException;
use crate::typed::{ElementOrLong, OpFront};
use crate::{ApiCx, ApiError, DomStr, ExceptionName};

/// Blink's limit on the options a `length` or an index past the end adds
/// (`kMaxListItems`): beyond it, nothing is added.
const MAX_LIST_ITEMS: u32 = 100_000;

// --- <select> -------------------------------------------------------------

/// `type`: `select-multiple` with `multiple`, else `select-one`.
pub(crate) fn select_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(
        if cx.select_is_multiple(this) {
            "select-multiple"
        } else {
            "select-one"
        },
    )))
}

/// `options`: the same each time.
pub(crate) fn options<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.select_options(this))
}

/// `selectedOptions`: the same each time.
pub(crate) fn selected_options<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.selected_options(this))
}

/// `length`: how many options.
pub(crate) fn length<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<u32, ApiError> {
    let mut count = 0u32;
    cx.for_each_option(this, |_| {
        count = count.saturating_add(1);
        true
    });
    Ok(count)
}

/// `item(index)` and `select[index]`: its option at `index`, null past the
/// last.
pub(crate) fn item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: u32,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.collection_item(cx.select_options(this), index))
}

/// `namedItem(name)`: its `options`'.
pub(crate) fn named_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    name: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.collection_named_item(cx.select_options(this), name))
}

/// `selectedIndex`: the first selected option's index, -1 for none.
pub(crate) fn selected_index<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<i32, ApiError> {
    let mut index = 0i32;
    let mut found = None;
    cx.for_each_option(this, |option| {
        if cx.option_selectedness(option) {
            found = Some(index);
            return false;
        }
        index = index.saturating_add(1);
        true
    });
    Ok(found.unwrap_or(-1))
}

/// Setting `selectedIndex`: every option unselected, and the one at it (if
/// any) selected and dirty; -1 leaves none selected.
pub(crate) fn set_selected_index<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
) -> Result<(), ApiError> {
    let chosen = u32::try_from(index)
        .ok()
        .and_then(|index| cx.collection_item(cx.select_options(this), index));
    cx.select_only(this, chosen);
    Ok(())
}

/// `value`: the first selected option's value, empty for none.
pub(crate) fn value<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(
        &cx.first_selected_option(this)
            .map(|option| cx.option_value(option))
            .unwrap_or_default(),
    )))
}

/// Setting `value`: every option unselected, and the first whose value it
/// is selected and dirty.
pub(crate) fn set_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    let mut chosen = None;
    cx.for_each_option(this, |option| {
        if cx.option_value(option) == value {
            chosen = Some(option);
            return false;
        }
        true
    });
    cx.select_only(this, chosen);
    Ok(())
}

/// A new `<option>` of `select`'s document, with no attributes or children.
fn new_option<'gc>(cx: &ApiCx<'_, 'gc>, select: NodeRef<'gc>) -> NodeRef<'gc> {
    cx.create_element(cx.document_of(select), QualName::html("option"))
}

/// Setting `length` (the select's and its `options`'): blank options
/// appended up to it, or the last ones removed down to it.
pub(crate) fn set_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    length: u32,
) -> Result<(), ApiError> {
    let options = cx.list_of_options(this);
    let current = u32::try_from(options.len()).unwrap_or(u32::MAX);
    if length > current {
        if length > MAX_LIST_ITEMS {
            return Ok(());
        }
        for _ in current..length {
            let option = new_option(cx, this);
            cx.append_child(this, option)?;
        }
    } else {
        let keep = usize::try_from(length).unwrap_or(usize::MAX);
        for option in options.iter().skip(keep) {
            cx.remove(*option);
        }
    }
    Ok(())
}

/// `select[index] = option` (and `options[index] = option`): blank options
/// up to `index` then `option` appended, or the option at `index` replaced
/// by it; null removes the one at `index`.
pub(crate) fn set_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: u32,
    option: Option<NodeRef<'gc>>,
) -> Result<(), ApiError> {
    let Some(option) = option else {
        return remove_index(cx, this, i32::try_from(index).unwrap_or(i32::MAX));
    };
    let options = cx.list_of_options(this);
    let length = u32::try_from(options.len()).unwrap_or(u32::MAX);
    if index >= length {
        if index > MAX_LIST_ITEMS {
            return Ok(());
        }
        for _ in length..index {
            let blank = new_option(cx, this);
            cx.append_child(this, blank)?;
        }
        cx.append_child(this, option)?;
    } else {
        let old = options[usize::try_from(index).unwrap_or(usize::MAX)];
        if let Some(parent) = cx.parent(old) {
            cx.replace_child(parent, option, old)?;
        }
    }
    Ok(())
}

/// `add(element, before)`: `element` inserted before `before` (an element
/// in the select, or the option at an index), or at the end of the
/// select.
pub(crate) fn add<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    element: NodeRef<'gc>,
    before: Option<ElementOrLong<'gc>>,
) -> Result<(), ApiError> {
    // An ancestor of the select is the DOM's `HierarchyRequestError` as it
    // is inserted (HTML's first step says so too).
    let reference = match before {
        Some(ElementOrLong::Element(before)) => {
            let inside = std::iter::successors(cx.parent(before), |node| cx.parent(*node))
                .any(|node| Gc::ptr_eq(node, this));
            if !inside {
                return Err(ApiError::Dom(DomException {
                    name: ExceptionName::NotFound,
                    message: "the element to add before isn't in the select".to_owned(),
                }));
            }
            if Gc::ptr_eq(before, element) {
                return Ok(());
            }
            Some(before)
        }
        Some(ElementOrLong::Long(index)) => u32::try_from(index)
            .ok()
            .and_then(|index| cx.collection_item(cx.select_options(this), index)),
        None => None,
    };
    let parent = reference
        .and_then(|reference| cx.parent(reference))
        .unwrap_or(this);
    cx.pre_insert(parent, element, reference)?;
    Ok(())
}

/// `options.remove(index)`: the option at `index` removed, if there is one.
pub(crate) fn remove_index<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
) -> Result<(), ApiError> {
    if let Some(option) = u32::try_from(index)
        .ok()
        .and_then(|index| cx.collection_item(cx.select_options(this), index))
    {
        cx.remove(option);
    }
    Ok(())
}

/// The select's `remove()`: without an index, `ChildNode`'s (the select
/// itself); with one, its option's.
pub(crate) fn remove<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: Option<i32>,
) -> Result<(), ApiError> {
    match index {
        Some(index) => remove_index(cx, this, index),
        None => crate::child_node::remove(cx, this),
    }
}

// --- HTMLOptionsCollection -------------------------------------------------

fn root<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, HtmlCollection>) -> NodeRef<'gc> {
    cx.collection_root(this)
}

pub(crate) fn options_set_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    length: u32,
) -> Result<(), ApiError> {
    set_length(cx, root(cx, this), length)
}

pub(crate) fn options_set_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    index: u32,
    option: Option<NodeRef<'gc>>,
) -> Result<(), ApiError> {
    set_item(cx, root(cx, this), index, option)
}

pub(crate) fn options_add<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    element: NodeRef<'gc>,
    before: Option<ElementOrLong<'gc>>,
) -> Result<(), ApiError> {
    add(cx, root(cx, this), element, before)
}

pub(crate) fn options_remove<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    index: i32,
) -> Result<(), ApiError> {
    remove_index(cx, root(cx, this), index)
}

pub(crate) fn options_selected_index<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
) -> Result<i32, ApiError> {
    selected_index(cx, root(cx, this))
}

pub(crate) fn options_set_selected_index<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    index: i32,
) -> Result<(), ApiError> {
    set_selected_index(cx, root(cx, this), index)
}

// --- <option> -------------------------------------------------------------

/// `new Option(text, value, defaultSelected, selected)`: an `<option>` of
/// the current realm's document with `text` as its text, `value` and
/// `selected` (from `defaultSelected`) attributes, and `selected` its
/// selectedness.
pub(crate) fn construct_option<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    text: &str,
    value: Option<&str>,
    default_selected: bool,
    selected: bool,
) -> Result<NodeRef<'gc>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("Illegal constructor"))?;
    let document = cx.window_document(window);
    let option = cx.create_element(document, QualName::html("option"));
    if let Some(value) = value {
        cx.set_attribute(option, "value", value);
    }
    if default_selected {
        cx.set_attribute(option, "selected", "");
    }
    if !text.is_empty() {
        let text = cx.create_text(document, text);
        cx.append_child(option, text)?;
    }
    cx.set_option_constructed(option, selected);
    Ok(option)
}

/// An option's `form`: its select's form owner.
pub(crate) fn option_form<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx
        .option_select(this)
        .and_then(|select| cx.form_owner(select)))
}

/// `label`: the attribute where there is one, else `text`.
pub(crate) fn label<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(&cx.option_label(this))))
}

/// `value`: the attribute where there is one, else `text`.
pub(crate) fn option_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(&cx.option_value(this))))
}

/// `text`: its descendant text but a script's, stripped and collapsed.
pub(crate) fn text<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(&cx.option_text(this))))
}

/// Setting `text`: DOM's string replace all, as `textContent`'s.
pub(crate) fn set_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_text_content(this, value);
    Ok(())
}

/// `index`: where it is in its select's list of options, 0 out of one.
pub(crate) fn index<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(i32::try_from(cx.option_index(this)).unwrap_or(i32::MAX))
}

/// `selected`: its selectedness.
pub(crate) fn selected<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(cx.option_selectedness(this))
}

/// Setting `selected`: its selectedness, now dirty; a select without
/// `multiple` keeps it alone.
pub(crate) fn set_selected<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    cx.set_option_selected(this, value);
    Ok(())
}

// --- <datalist> -----------------------------------------------------------

/// A datalist's `options`: its option descendants, the same each time.
pub(crate) fn datalist_options<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.datalist_options(this))
}

// --- <progress> and <meter> -----------------------------------------------

/// An attribute's number by HTML's rules for parsing floating-point
/// number values, if it has one.
fn attribute_float<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    local: names::NameId,
) -> Option<f64> {
    cx.attribute_value_by_id(this, local)
        .and_then(|value| parse_float(&value.borrow().to_str()))
        .filter(|value| value.is_finite())
}

/// Setting a reflected `double`: its best representation.
fn set_double<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, local: &str, value: f64) {
    cx.set_attribute(this, local, &number_to_string(value));
}

/// A progress's `max`: the attribute where it is a positive number, else 1.
pub(crate) fn progress_max<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<f64, ApiError> {
    Ok(attribute_float(cx, this, names::local::MAX)
        .filter(|max| *max > 0.0)
        .unwrap_or(1.0))
}

/// Setting a `max` that isn't positive does nothing ("limited to only
/// positive numbers").
pub(crate) fn set_progress_max<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: f64,
) -> Result<(), ApiError> {
    if value > 0.0 {
        set_double(cx, this, "max", value);
    }
    Ok(())
}

/// The current value of a determinate progress bar (one with a `value`):
/// the attribute's number, 0 if it isn't one or is negative, at most
/// `max`. None for an indeterminate one.
fn progress_current<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Option<f64> {
    cx.attribute_value_by_id(this, names::local::VALUE)?;
    let max = attribute_float(cx, this, names::local::MAX)
        .filter(|max| *max > 0.0)
        .unwrap_or(1.0);
    let value = attribute_float(cx, this, names::local::VALUE).filter(|value| *value >= 0.0);
    Some(value.unwrap_or(0.0).min(max))
}

/// A progress's `value`: the current value; 0 while indeterminate.
pub(crate) fn progress_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<f64, ApiError> {
    Ok(progress_current(cx, this).unwrap_or(0.0))
}

pub(crate) fn set_progress_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: f64,
) -> Result<(), ApiError> {
    set_double(cx, this, "value", value);
    Ok(())
}

/// `position`: the current value over the maximum, -1 while
/// indeterminate.
pub(crate) fn progress_position<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<f64, ApiError> {
    let max = progress_max(cx, this)?;
    Ok(progress_current(cx, this).map_or(-1.0, |value| value / max))
}

/// A meter's boundaries and values, each clamped as HTML has them.
struct Meter {
    min: f64,
    max: f64,
    value: f64,
    low: f64,
    high: f64,
    optimum: f64,
}

fn meter<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Meter {
    let min = attribute_float(cx, this, names::local::MIN).unwrap_or(0.0);
    let max = attribute_float(cx, this, names::local::MAX)
        .unwrap_or(1.0)
        .max(min);
    let value = attribute_float(cx, this, names::local::VALUE)
        .unwrap_or(0.0)
        .clamp(min, max);
    let low = attribute_float(cx, this, names::local::LOW)
        .unwrap_or(min)
        .clamp(min, max);
    let high = attribute_float(cx, this, names::local::HIGH)
        .unwrap_or(max)
        .clamp(low, max);
    let optimum = attribute_float(cx, this, names::local::OPTIMUM)
        .unwrap_or((min + max) / 2.0)
        .clamp(min, max);
    Meter {
        min,
        max,
        value,
        low,
        high,
        optimum,
    }
}

macro_rules! meter_attribute {
    ($get:ident, $set:ident, $field:ident, $attr:literal) => {
        pub(crate) fn $get<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<f64, ApiError> {
            Ok(meter(cx, this).$field)
        }

        pub(crate) fn $set<'gc>(
            cx: &ApiCx<'_, 'gc>,
            this: NodeRef<'gc>,
            value: f64,
        ) -> Result<(), ApiError> {
            set_double(cx, this, $attr, value);
            Ok(())
        }
    };
}

meter_attribute!(meter_min, set_meter_min, min, "min");
meter_attribute!(meter_max, set_meter_max, max, "max");
meter_attribute!(meter_value, set_meter_value, value, "value");
meter_attribute!(meter_low, set_meter_low, low, "low");
meter_attribute!(meter_high, set_meter_high, high, "high");
meter_attribute!(meter_optimum, set_meter_optimum, optimum, "optimum");

// --- <output> -------------------------------------------------------------

/// An output's `type`: `"output"`.
pub(crate) fn output_type<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(names::local::OUTPUT))
}

/// An output's `value`: its descendant text content.
pub(crate) fn output_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        cx.text_content(this).unwrap_or_else(|| DomString::new("")),
    ))
}

/// Setting an output's `value` keeps the default value as it was (its
/// override), then replaces the children with the text.
pub(crate) fn set_output_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    let default = output_default(cx, this);
    cx.set_output_default_value_override(this, Some(default));
    cx.set_text_content(this, value);
    Ok(())
}

fn output_default<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> DomString {
    cx.output_default_value_override(this)
        .unwrap_or_else(|| cx.text_content(this).unwrap_or_else(|| DomString::new("")))
}

/// An output's `defaultValue`: the override, or while there is none, the
/// descendant text content.
pub(crate) fn output_default_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(output_default(cx, this)))
}

/// Setting an output's `defaultValue`: the children's text while there is
/// no override (the value is the default value then), else the override.
pub(crate) fn set_output_default_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    if cx.output_default_value_override(this).is_none() {
        cx.set_text_content(this, value);
    } else {
        cx.set_output_default_value_override(this, Some(DomString::new(value)));
    }
    Ok(())
}
