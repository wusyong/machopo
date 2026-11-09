//! WebIDL's `DOMException` (Blink's `DOMException` and
//! `V8ThrowDOMException`), a V8 object alone: no object of the DOM's heap
//! behind it, as it holds nothing but its name and message (in its
//! internal fields; its `code` is its name's). Its interface object is a
//! constructor (`new DOMException(message, name)`), its prototype's
//! prototype is `Error.prototype` (WebIDL's `[ExceptionClass]`), and one
//! thrown has a `stack`. Made in the current realm. `QuotaExceededError`
//! is a `DOMException` with its quota and the amount requested too
//! (WebIDL's), what the DOM's `QuotaExceededError` is
//! thrown as. Each is branded with a private symbol, which structured
//! clone tells them by (`brand`).

use dom_api::{INTERFACES, LEGACY_CODES};

use crate::state::Scope;
use crate::templates::{self, string};

/// Its internal fields: its name, then its message.
const NAME: usize = 0;
const MESSAGE: usize = 1;
const FIELDS: usize = 2;
/// A `QuotaExceededError`'s after them: its quota, then the amount
/// requested (each a number, or null).
const QUOTA: usize = 2;
const REQUESTED: usize = 3;
const QUOTA_FIELDS: usize = 4;

/// The template slot of `QuotaExceededError` (`DOMException`'s is
/// `INTERFACES.len()`).
pub(crate) fn quota_slot() -> usize {
    INTERFACES.len().saturating_add(1)
}

/// What an object is, by its brand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Brand {
    DomException,
    QuotaExceededError,
}

/// The private symbol a `DOMException` (1) or a `QuotaExceededError` (2)
/// has, which script can't see or set.
fn brand_key<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Private> {
    let name = string(scope, "machopo:DOMException");
    v8::Private::for_api(scope, Some(name))
}

fn set_brand(scope: &mut v8::PinScope<'_, '_>, object: v8::Local<'_, v8::Object>, brand: Brand) {
    let key = brand_key(scope);
    let value = v8::Integer::new(
        scope,
        match brand {
            Brand::DomException => 1,
            Brand::QuotaExceededError => 2,
        },
    );
    object.set_private(scope, key, value.into());
}

/// Whether `object` is a `DOMException` (made by its constructor here), and
/// which kind.
pub(crate) fn brand(
    scope: &mut v8::PinScope<'_, '_>,
    object: v8::Local<'_, v8::Object>,
) -> Option<Brand> {
    let key = brand_key(scope);
    let value = object.get_private(scope, key)?;
    match value.int32_value(scope)? {
        1 => Some(Brand::DomException),
        2 => Some(Brand::QuotaExceededError),
        _ => None,
    }
}

/// Whether `object`, an API wrapper, is a `DOMException` of either kind:
/// `Error.isError`'s answer for it (`set_native_error_check`).
pub(crate) fn is_dom_exception(
    scope: &mut v8::PinScope<'_, '_>,
    object: v8::Local<'_, v8::Object>,
) -> bool {
    brand(scope, object).is_some()
}

/// `exception`'s name and message (a `DOMException` of either kind).
pub(crate) fn name_and_message(
    scope: &mut v8::PinScope<'_, '_>,
    exception: v8::Local<'_, v8::Object>,
) -> (String, String) {
    let field = |scope: &mut v8::PinScope<'_, '_>, index| {
        exception
            .get_internal_field(scope, index)
            .and_then(|data| v8::Local::<v8::Value>::try_from(data).ok())
            .map(|value| value.to_rust_string_lossy(scope))
            .unwrap_or_default()
    };
    (field(scope, NAME), field(scope, MESSAGE))
}

/// A `QuotaExceededError`'s quota and amount requested.
pub(crate) fn quota_and_requested(
    scope: &mut v8::PinScope<'_, '_>,
    error: v8::Local<'_, v8::Object>,
) -> (Option<f64>, Option<f64>) {
    let field = |scope: &mut v8::PinScope<'_, '_>, index| {
        error
            .get_internal_field(scope, index)
            .and_then(|data| v8::Local::<v8::Value>::try_from(data).ok())
            .filter(|value| value.is_number())
            .and_then(|value| value.number_value(scope))
    };
    (field(scope, QUOTA), field(scope, REQUESTED))
}

pub(crate) fn make_template<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let template = v8::FunctionTemplate::builder(construct)
        .length(0)
        .build(scope);
    template.set_class_name(string(scope, "DOMException"));
    template.read_only_prototype();
    template
        .instance_template(scope)
        .set_internal_field_count(FIELDS);
    let prototype = template.prototype_template(scope);
    templates::to_string_tag(scope, prototype, "DOMException");
    // The attributes, then the legacy codes (WebIDL's `DOMException`'s
    // order, the prototype's keys').
    let signature = v8::Signature::new(scope, template);
    let getters = [
        ("name", getter(scope, signature, get_name)),
        ("message", getter(scope, signature, get_message)),
        ("code", getter(scope, signature, get_code)),
    ];
    for (name, getter) in getters {
        let name = string(scope, name);
        prototype.set_accessor_property(
            name.into(),
            Some(getter),
            None,
            v8::PropertyAttribute::NONE,
        );
    }
    for &(name, code) in LEGACY_CODES {
        let name = string(scope, name);
        let code = v8::Integer::new_from_unsigned(scope, u32::from(code));
        template.set_with_attr(name.into(), code.into(), templates::constant());
        prototype.set_with_attr(name.into(), code.into(), templates::constant());
    }
    template
}

/// An attribute's getter, its receiver checked against `signature`.
fn getter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    signature: v8::Local<'s, v8::Signature>,
    callback: impl v8::MapFnTo<v8::FunctionCallback>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    v8::FunctionTemplate::builder(callback)
        .signature(signature)
        .length(0)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
}

/// `DOMException`'s interface object in the current realm, its prototype's
/// prototype `Error.prototype`.
pub(crate) fn interface_object<'s>(
    scope: &mut Scope<'_, 's, '_>,
) -> Option<v8::Local<'s, v8::Function>> {
    let function = templates::template(scope, INTERFACES.len()).get_function(scope.scope())?;
    let scope = scope.scope();
    let key = string(scope, "prototype");
    let prototype = function.get(scope, key.into())?.to_object(scope)?;
    // The realm's own `Error.prototype`, whatever script did to `Error`.
    let empty = v8::String::empty(scope);
    let error = v8::Exception::error(scope, empty).to_object(scope)?;
    let error_prototype = error.get_prototype(scope)?;
    prototype.set_prototype(scope, error_prototype)?;
    Some(function)
}

/// `new DOMException(message = "", name = "Error")`.
fn construct<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _: v8::ReturnValue<'s>,
) {
    if args.new_target().is_undefined() {
        crate::throw_type_error(
            scope,
            "Failed to construct 'DOMException': Please use the 'new' operator, this DOM object \
             constructor cannot be called as a function.",
        );
        return;
    }
    let Some(message) = optional_string(scope, args.get(0), "") else {
        return;
    };
    let Some(name) = optional_string(scope, args.get(1), "Error") else {
        return;
    };
    let this = args.this();
    this.set_internal_field(NAME, name.into());
    this.set_internal_field(MESSAGE, message.into());
    set_brand(scope, this, Brand::DomException);
}

/// `QuotaExceededError`'s template: `DOMException`'s child, with its
/// `quota` and `requested` (WebIDL's `double?`s).
pub(crate) fn make_quota_template<'s>(
    scope: &mut Scope<'_, 's, '_>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let parent = templates::template(scope, INTERFACES.len());
    let scope = scope.scope();
    let template = v8::FunctionTemplate::builder(construct_quota)
        .length(0)
        .build(scope);
    template.set_class_name(string(scope, "QuotaExceededError"));
    template.read_only_prototype();
    template.inherit(parent);
    template
        .instance_template(scope)
        .set_internal_field_count(QUOTA_FIELDS);
    let prototype = template.prototype_template(scope);
    templates::to_string_tag(scope, prototype, "QuotaExceededError");
    let signature = v8::Signature::new(scope, template);
    let getters = [
        ("quota", getter(scope, signature, get_quota)),
        ("requested", getter(scope, signature, get_requested)),
    ];
    for (name, getter) in getters {
        let name = string(scope, name);
        prototype.set_accessor_property(
            name.into(),
            Some(getter),
            None,
            v8::PropertyAttribute::NONE,
        );
    }
    template
}

/// `QuotaExceededError`'s interface object in the current realm, after
/// `DOMException`'s (which links the prototypes to `Error.prototype`), its
/// `[[Prototype]]` `DOMException` (WebIDL's; V8's `Inherit` links only the
/// prototypes).
pub(crate) fn quota_interface_object<'s>(
    scope: &mut Scope<'_, 's, '_>,
) -> Option<v8::Local<'s, v8::Function>> {
    let parent = interface_object(scope)?;
    let function = templates::template(scope, quota_slot()).get_function(scope.scope())?;
    function.set_prototype(scope.scope(), parent.into())?;
    Some(function)
}

/// `new QuotaExceededError(message = "", options = {})`: its quota and
/// amount requested the options' (finite numbers, WebIDL's `double`); as
/// Chrome 152, a negative one, or one requested under
/// the quota, isn't a `RangeError` (WebIDL's is).
fn construct_quota<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _: v8::ReturnValue<'s>,
) {
    if args.new_target().is_undefined() {
        crate::throw_type_error(
            scope,
            "Failed to construct 'QuotaExceededError': Please use the 'new' operator, this DOM \
             object constructor cannot be called as a function.",
        );
        return;
    }
    let Some(message) = optional_string(scope, args.get(0), "") else {
        return;
    };
    let options = args.get(1);
    let (mut quota, mut requested) = (None, None);
    if !options.is_null_or_undefined() {
        let Ok(options) = v8::Local::<v8::Object>::try_from(options) else {
            crate::throw_type_error(
                scope,
                "Failed to construct 'QuotaExceededError': The provided value is not of type \
                 'QuotaExceededErrorOptions'.",
            );
            return;
        };
        for (member, slot) in [("quota", &mut quota), ("requested", &mut requested)] {
            let key = string(scope, member);
            let Some(value) = options.get(scope, key.into()) else {
                return;
            };
            if value.is_undefined() {
                continue;
            }
            let Some(number) = value.number_value(scope) else {
                return;
            };
            if !number.is_finite() {
                crate::throw_type_error(
                    scope,
                    &format!(
                        "Failed to construct 'QuotaExceededError': Failed to read the '{member}' \
                         property from 'QuotaExceededErrorOptions': The provided double value is \
                         non-finite."
                    ),
                );
                return;
            }
            *slot = Some(number);
        }
    }
    let this = args.this();
    set_quota_error(scope, this, message, quota, requested);
}

fn set_quota_error<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    this: v8::Local<'s, v8::Object>,
    message: v8::Local<'s, v8::String>,
    quota: Option<f64>,
    requested: Option<f64>,
) {
    let name = string(scope, "QuotaExceededError");
    this.set_internal_field(NAME, name.into());
    this.set_internal_field(MESSAGE, message.into());
    let number =
        |scope: &mut v8::PinScope<'s, '_>, value: Option<f64>| -> v8::Local<'s, v8::Value> {
            match value {
                Some(value) => v8::Number::new(scope, value).into(),
                None => v8::null(scope).into(),
            }
        };
    let quota = number(scope, quota);
    let requested = number(scope, requested);
    this.set_internal_field(QUOTA, quota.into());
    this.set_internal_field(REQUESTED, requested.into());
    set_brand(scope, this, Brand::QuotaExceededError);
}

/// A new `QuotaExceededError` of `message`, `quota` and `requested`, made
/// in the current realm (one read back from a structured clone).
pub(crate) fn new_quota_exceeded_error<'s>(
    scope: &mut Scope<'_, 's, '_>,
    message: &str,
    quota: Option<f64>,
    requested: Option<f64>,
) -> Option<v8::Local<'s, v8::Object>> {
    let function = quota_interface_object(scope)?;
    let scope = scope.scope();
    let message = v8::String::new(scope, message)?;
    let error = function.new_instance(scope, &[message.into()])?;
    set_quota_error(scope, error, message, quota, requested);
    Some(error)
}

fn get_quota<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s>,
) {
    rv.set(quota_field(scope, &args, QUOTA));
}

fn get_requested<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s>,
) {
    rv.set(quota_field(scope, &args, REQUESTED));
}

/// A `QuotaExceededError`'s field `index`: a number, or null.
fn quota_field<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::FunctionCallbackArguments<'s>,
    index: usize,
) -> v8::Local<'s, v8::Value> {
    args.this()
        .get_internal_field(scope, index)
        .and_then(|data| v8::Local::<v8::Value>::try_from(data).ok())
        .unwrap_or_else(|| v8::null(scope).into())
}

/// An optional `DOMString` argument: `default` if undefined, else its
/// `ToString` (none if that threw).
fn optional_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    default: &str,
) -> Option<v8::Local<'s, v8::String>> {
    if value.is_undefined() {
        return Some(string(scope, default));
    }
    value.to_string(scope)
}

/// Field `index` of the receiver (V8's signature checked it is one).
fn field<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::FunctionCallbackArguments<'s>,
    index: usize,
) -> Option<v8::Local<'s, v8::String>> {
    let data = args.this().get_internal_field(scope, index)?;
    v8::Local::<v8::Value>::try_from(data)
        .ok()
        .and_then(|value| v8::Local::<v8::String>::try_from(value).ok())
}

fn get_name<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s>,
) {
    if let Some(name) = field(scope, &args, NAME) {
        rv.set(name.into());
    }
}

fn get_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s>,
) {
    if let Some(message) = field(scope, &args, MESSAGE) {
        rv.set(message.into());
    }
}

fn get_code<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s>,
) {
    let code = field(scope, &args, NAME).map_or(0, |name| {
        dom_api::legacy_code(&name.to_rust_string_lossy(scope))
    });
    rv.set_uint32(u32::from(code));
}

/// Throws a `DOMException` of `name` and `message`, made in the current
/// realm, with its stack: after the DOM's operation (V8 allocates).
pub(crate) fn throw_dom_exception(scope: &mut Scope<'_, '_, '_>, name: &str, message: &str) {
    if let Some(exception) = new_dom_exception(scope, name, message) {
        scope.scope().throw_exception(exception.into());
    }
}

/// A new `DOMException` of `name` and `message`, made in the current
/// realm, with its stack (an abort reason's): out of the DOM's operations.
pub(crate) fn new_dom_exception<'s>(
    scope: &mut Scope<'_, 's, '_>,
    name: &str,
    message: &str,
) -> Option<v8::Local<'s, v8::Object>> {
    // The DOM's `QuotaExceededError` is one of its own interface, its
    // quota and amount requested unknown (null).
    if name == "QuotaExceededError" {
        let error = new_quota_exceeded_error(scope, message, None, None)?;
        let context = scope.scope().get_current_context();
        v8::Exception::capture_stack_trace(context, error);
        return Some(error);
    }
    let function = interface_object(scope)?;
    let scope = scope.scope();
    let message = v8::String::new(scope, message)?;
    let name = v8::String::new(scope, name)?;
    let exception = function.new_instance(scope, &[message.into(), name.into()])?;
    let context = scope.get_current_context();
    v8::Exception::capture_stack_trace(context, exception);
    Some(exception)
}

/// Throws a `RangeError` of `message`.
pub(crate) fn throw_range_error(scope: &mut v8::PinScope<'_, '_>, message: &str) {
    let Some(message) = v8::String::new(scope, message) else {
        return;
    };
    let error = v8::Exception::range_error(scope, message);
    scope.throw_exception(error);
}
