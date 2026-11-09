//! What HTML's `define` reads off a custom element's constructor (Blink's
//! `ScriptCustomElementDefinitionBuilder`): its `prototype`, the
//! prototype's lifecycle callbacks, `observedAttributes`,
//! `disabledFeatures` and `formAssociated`, in the order the steps read
//! them: each a `Get`, which may run script (a getter, a proxy's trap).

use dom_api::ConstructorInfo;

use crate::{observable_array, templates};

/// The lifecycle callbacks the definition keeps, in its edges' order.
const CALLBACKS: [&str; 4] = [
    "connectedCallback",
    "disconnectedCallback",
    "adoptedCallback",
    "attributeChangedCallback",
];

/// What a form-associated definition's prototype must have as functions
/// (or not at all); read and checked, not kept: form-associated custom
/// elements do nothing more yet.
const FORM_CALLBACKS: [&str; 4] = [
    "formAssociatedCallback",
    "formResetCallback",
    "formDisabledCallback",
    "formStateRestoreCallback",
];

type Callbacks<'s> = [Option<v8::Local<'s, v8::Function>>; 4];

/// `constructor`'s callbacks and the rest `define` reads; `None` with an
/// exception thrown (script's, or a `TypeError` of the steps').
pub(crate) fn read_constructor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    constructor: v8::Local<'s, v8::Object>,
) -> Option<(Callbacks<'s>, ConstructorInfo)> {
    let key = templates::string(scope, "prototype");
    let prototype = constructor.get(scope, key.into())?;
    let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) else {
        crate::throw_type_error(
            scope,
            "Failed to execute 'define' on 'CustomElementRegistry': The prototype of the constructor is not an object.",
        );
        return None;
    };
    let mut callbacks: Callbacks<'s> = [None; 4];
    for (slot, name) in callbacks.iter_mut().zip(CALLBACKS) {
        *slot = callback(scope, prototype, name)?;
    }
    let mut info = ConstructorInfo::default();
    if callbacks[3].is_some() {
        info.observed_attributes = strings(scope, constructor, "observedAttributes")?;
    }
    info.disabled_features = strings(scope, constructor, "disabledFeatures")?;
    let key = templates::string(scope, "formAssociated");
    info.form_associated = constructor.get(scope, key.into())?.boolean_value(scope);
    if info.form_associated {
        for name in FORM_CALLBACKS {
            callback(scope, prototype, name)?;
        }
    }
    Some((callbacks, info))
}

/// `prototype[name]`: undefined is none, a function is the callback,
/// anything else a `TypeError` (WebIDL's conversion to `Function`).
fn callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    prototype: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<Option<v8::Local<'s, v8::Function>>> {
    let key = templates::string(scope, name);
    let value = prototype.get(scope, key.into())?;
    if value.is_undefined() {
        return Some(None);
    }
    match v8::Local::<v8::Function>::try_from(value) {
        Ok(function) => Some(Some(function)),
        Err(_) => {
            crate::throw_type_error(
                scope,
                &format!(
                    "Failed to execute 'define' on 'CustomElementRegistry': The '{name}' property of the prototype is not a function."
                ),
            );
            None
        }
    }
}

/// `constructor[name]` as a `sequence<DOMString>`: none if undefined.
fn strings<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    constructor: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<Vec<String>> {
    let key = templates::string(scope, name);
    let value = constructor.get(scope, key.into())?;
    if value.is_undefined() {
        return Some(Vec::new());
    }
    observable_array::sequence(scope, value)?
        .into_iter()
        .map(|item| Some(item.to_string(scope)?.to_rust_string_lossy(scope)))
        .collect()
}
