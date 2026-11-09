//! A listed element's form owner and a form's elements
//! (https://html.spec.whatwg.org/#form-owner): `form` of the listed
//! elements, a form's `elements`, `length`, indexed and named getters, and
//! `HTMLFormControlsCollection`'s `namedItem` (`dom`'s `form`: the
//! owner is reset in the DOM's operations, a form keeps its elements); a
//! fieldset's `elements` and `type`, a legend's `form`, a label's `control` and `form`
//! and a labelable element's `labels`; an input's `type`, `value`,
//! `checked` and `indeterminate`, a button's `type`, a form's `reset()`,
//! `RadioNodeList`'s `value`.

use dom::{EventFlags, FormItem, HtmlCollection, NodeList, NodeRef, NodeRoot, TargetRoot};
use elise::{Gc, Persistent};

use crate::typed::ScriptHost;
use crate::{ApiCx, ApiError, DomStr, ExceptionName, dispatch};

/// A listed element's `form`: its form owner.
pub(crate) fn form<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.form_owner(this))
}

/// `document.forms`: the same each time.
pub(crate) fn document_forms<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.document_forms(this))
}

/// A form's `elements`: the same each time.
pub(crate) fn elements<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.form_elements(this))
}

/// A form's `length`: its `elements`' (WebIDL `unsigned long`).
pub(crate) fn length<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<u32, ApiError> {
    Ok(cx.collection_length(cx.form_elements(this)))
}

/// `form[index]`: its `elements`' at `index`, null past the last.
pub(crate) fn item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: u32,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.collection_item(cx.form_elements(this), index))
}

/// `form[name]`: its elements named so, or the one its past names map
/// keeps.
pub(crate) fn named_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    name: &str,
) -> Result<Option<FormItem<'gc>>, ApiError> {
    Ok(cx.form_named_item(this, name))
}

/// `elements.namedItem(name)` (and `elements[name]`): the one element
/// whose `id` or `name` it is, a `RadioNodeList` of several, or null.
pub(crate) fn controls_named_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, HtmlCollection>,
    name: &str,
) -> Result<Option<FormItem<'gc>>, ApiError> {
    Ok(cx.form_controls_named_item(this, name))
}

/// A fieldset's `elements`: the same each time.
pub(crate) fn fieldset_elements<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.fieldset_elements(this))
}

/// A fieldset's `type`: `"fieldset"`.
pub(crate) fn fieldset_type<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(dom::names::local::FIELDSET))
}

/// A legend's `form`: its fieldset's form owner.
pub(crate) fn legend_form<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.legend_form(this))
}

/// A label's `control`: its labeled control.
pub(crate) fn control<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.label_control(this))
}

/// A label's `form`: its labeled control's form owner.
pub(crate) fn label_form<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.label_form(this))
}

/// `labels` of an element always labelable (not an `<input>`): the same
/// each time.
pub(crate) fn labels<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, NodeList>, ApiError> {
    Ok(cx
        .labels(this)
        .expect("an element labelable whatever its attributes"))
}

/// An `<input>`'s `labels`: null for a hidden one, which isn't labelable.
pub(crate) fn input_labels<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<Gc<'gc, NodeList>>, ApiError> {
    Ok(cx.labels(this))
}

/// An input's `type`: its type's keyword (HTML's limited to only known
/// values, `text` for none).
pub(crate) fn input_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let kind = cx.input_kind(this).expect("an input has its type");
    Ok(DomStr::Name(dom::names::intern(kind.keyword())))
}

/// An input's `checked`: its checkedness.
pub(crate) fn checked<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(cx.checkedness(this))
}

/// Setting `checked`: the checkedness, dirty now (a radio button checked
/// unchecks its group).
pub(crate) fn set_checked<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    checked: bool,
) -> Result<(), ApiError> {
    cx.set_checked(this, checked);
    Ok(())
}

pub(crate) fn indeterminate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(cx.indeterminate(this))
}

pub(crate) fn set_indeterminate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    indeterminate: bool,
) -> Result<(), ApiError> {
    cx.set_indeterminate(this, indeterminate);
    Ok(())
}

/// An input's `value`, by its value mode.
pub(crate) fn input_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.input_value(this)))
}

/// Setting an input's `value`: a file input's to anything but the empty
/// string is an `InvalidStateError`.
pub(crate) fn set_input_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_input_value(this, value)
        .map_err(|()| ApiError::dom(ExceptionName::InvalidState))
}

/// A button's `type` (HTML's limited to only known values, a submit
/// button's for none or another).
pub(crate) fn button_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(dom::names::intern(cx.button_type(this))))
}

/// `RadioNodeList`'s `value`.
pub(crate) fn radio_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeList>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.radio_list_value(this)))
}

pub(crate) fn set_radio_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeList>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_radio_list_value(this, value);
    Ok(())
}

/// A form's `reset()`: unless it is resetting already, it is reset.
pub(crate) fn reset<H: ScriptHost>(host: &mut H, this: NodeRoot) -> Result<(), ApiError> {
    if host.read(|cx| cx.form_locked_for_reset(this.get(cx.gc))) {
        return Ok(());
    }
    host.mutate(|cx| cx.set_form_locked_for_reset(this.get(cx.gc), true));
    reset_form(host, &this);
    host.mutate(|cx| cx.set_form_locked_for_reset(this.get(cx.gc), false));
    Ok(())
}

/// HTML's reset of `form`: its `reset` event (trusted, bubbling,
/// cancelable), and if no listener canceled it, its controls reset.
pub(crate) fn reset_form<H: ScriptHost>(host: &mut H, form: &NodeRoot) {
    let target = TargetRoot::Node(form.clone());
    let event = host.mutate(|cx| {
        let window = cx.target_window(target.get(cx.gc));
        let flags = EventFlags {
            bubbles: true,
            cancelable: true,
            composed: false,
            trusted: true,
        };
        Persistent::new(
            cx.gc,
            cx.create_event(dom::names::event::RESET, flags, window),
        )
    });
    if dispatch(host, &target, &event) {
        host.mutate(|cx| cx.reset_form_controls(form.get(cx.gc)));
    }
}

/// A form's `method` and `formMethod`'s keywords.
const METHODS: &[&str] = &["get", "post", "dialog"];
/// `enctype`'s and `formEnctype`'s.
const ENCTYPES: &[&str] = &[
    "application/x-www-form-urlencoded",
    "multipart/form-data",
    "text/plain",
];
const ON_OFF: &[&str] = &["on", "off"];

macro_rules! enumerated {
    ($get:ident, $local:ident, $keywords:expr, $missing:expr, $invalid:expr) => {
        pub(crate) fn $get<'gc>(
            cx: &ApiCx<'_, 'gc>,
            this: NodeRef<'gc>,
        ) -> Result<DomStr<'gc>, ApiError> {
            Ok(crate::reflect::enumerated(
                cx,
                this,
                dom::names::local::$local,
                $keywords,
                $missing,
                $invalid,
            ))
        }
    };
}

enumerated!(method, METHOD, METHODS, Some("get"), "get");
// `enctype` and `encoding` both.
enumerated!(enctype, ENCTYPE, ENCTYPES, Some(ENCTYPES[0]), ENCTYPES[0]);
enumerated!(form_autocomplete, AUTOCOMPLETE, ON_OFF, Some("on"), "on");
// A submitter's overrides have no missing value default: the form's
// applies.
enumerated!(form_method, FORMMETHOD, METHODS, None, "get");
enumerated!(form_enctype, FORMENCTYPE, ENCTYPES, None, ENCTYPES[0]);

/// `valueAsNumber`: a number's or a range's value as a number, `NaN` for
/// none or for a type it doesn't apply to.
pub(crate) fn value_as_number<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<f64, ApiError> {
    Ok(cx.input_value_as_number(this))
}

/// Setting `valueAsNumber`: an infinite number is a `TypeError`; a type it
/// doesn't apply to an `InvalidStateError`; `NaN` empties the value (a
/// range's is then its default); another number is the value.
pub(crate) fn set_value_as_number<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: f64,
) -> Result<(), ApiError> {
    if value.is_infinite() {
        return Err(ApiError::type_error("The value provided is infinite."));
    }
    if !cx.steps(this) {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::InvalidState,
            message: "This input element does not support Number values.".to_owned(),
        }));
    }
    let value = if value.is_nan() {
        String::new()
    } else {
        dom::number_to_string(value)
    };
    set_input_value(cx, this, &value)
}

/// `stepUp(n)` and `stepDown(n)` (HTML's, as Blink's `InputType::StepUp`):
/// from the value, 0 for none, `count` steps; an `InvalidStateError` for a
/// type the steps don't apply to, or with no allowed value step.
fn step<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, count: f64) -> Result<(), ApiError> {
    let error = |message: &str| {
        ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::InvalidState,
            message: message.to_owned(),
        })
    };
    match cx.step_for_script(this, count) {
        Ok(Some(value)) => set_input_value(cx, this, &value),
        Ok(None) => Ok(()),
        Err(dom::StepError::NotSteppable) => Err(error("This form element is not steppable.")),
        Err(dom::StepError::NoAllowedStep) => Err(error(
            "This form element does not have an allowed value step.",
        )),
    }
}

pub(crate) fn step_up<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    n: i32,
) -> Result<(), ApiError> {
    step(cx, this, f64::from(n))
}

pub(crate) fn step_down<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    n: i32,
) -> Result<(), ApiError> {
    step(cx, this, -f64::from(n))
}
