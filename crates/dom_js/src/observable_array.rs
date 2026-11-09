//! WebIDL's observable array exotic object: the value of an
//! `ObservableArray<T>` attribute (`adoptedStyleSheets`), an array script
//! changes in place (`push`, `splice`, `[i] =`, `length = 0`), each change
//! going through the attribute's backing list (Blink's
//! `ObservableArrayExoticObject`).
//!
//! It is a `Proxy` whose target is an empty array, so it is an array
//! (`Array.isArray`, `Array.prototype`), and whose handler's traps read
//! and change the list: `length` and the indices below it are the list's,
//! anything else is the target's. The list's three operations are the
//! table's typed paths (the attribute's getter gives the list, its
//! `#[observable(..)]` methods set a value and take the last off), each a
//! function of its own made from its template, which the traps call as
//! any function (what it throws goes on up). The handler is one per realm,
//! kept on the global under a private key, with native functions as its
//! traps; the target carries the platform object and the attribute's
//! number (`State::observable`), under private keys, which the traps find
//! through their first argument. The platform object's wrapper keeps its
//! array under a private key too, so the attribute gives the same one each
//! time.

use crate::state;
use crate::templates;

/// An observable array attribute's templates: its interface and name, and
/// its backing list's operations' (each made from its typed path).
pub(crate) struct ObservableTemplates {
    pub(crate) interface: usize,
    pub(crate) attribute: &'static str,
    /// The interface of its values, by its index in `INTERFACES`.
    pub(crate) item: usize,
    pub(crate) list: v8::Global<v8::FunctionTemplate>,
    pub(crate) set: Option<v8::Global<v8::FunctionTemplate>>,
    pub(crate) delete: Option<v8::Global<v8::FunctionTemplate>>,
}

/// Which of an attribute's operations.
#[derive(Clone, Copy)]
enum Op {
    List,
    Set,
    Delete,
}

/// The private key of `what`: the target's platform object (`owner`) and
/// attribute (`member`), the realm's handler (`handler`), a wrapper's
/// array (`array:` and the attribute's name).
fn private<'s>(scope: &mut v8::PinScope<'s, '_>, what: &str) -> v8::Local<'s, v8::Private> {
    let name = templates::string(scope, &format!("machopo:observable-array#{what}"));
    v8::Private::for_api(scope, Some(name))
}

/// Attribute `code`'s operation `op`, a function of the current realm.
fn function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    code: usize,
    op: Op,
) -> Option<v8::Local<'s, v8::Function>> {
    let template = {
        let mut state = state::scope(scope)?;
        let (state, mut no_gc) = state.state();
        let templates = state.observable.get(code)?;
        let global = match op {
            Op::List => Some(&templates.list),
            Op::Set => templates.set.as_ref(),
            Op::Delete => templates.delete.as_ref(),
        }?;
        no_gc.local(global)
    };
    template.get_function(scope)
}

/// The attribute's getter (its accessor's): `holder`'s array, made the
/// first time; `code` its number (`State::observable`), the accessor's
/// data.
pub(crate) fn getter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let holder = args.this();
    let Some(code) = args
        .data()
        .int32_value(scope)
        .and_then(|code| usize::try_from(code).ok())
    else {
        return;
    };
    // `holder` has the attribute: its list checks so.
    if list(scope, holder, code).is_none() {
        return;
    }
    let attribute = match state::scope(scope) {
        Some(mut state) => state.state().0.observable[code].attribute,
        None => return,
    };
    let key = private(scope, &format!("array:{attribute}"));
    if let Some(array) = holder.get_private(scope, key)
        && array.is_proxy()
    {
        rv.set(array);
        return;
    }
    let target = v8::Array::new(scope, 0);
    let owner_key = private(scope, "owner");
    target.set_private(scope, owner_key, holder.into());
    let member_key = private(scope, "member");
    let number =
        v8::Integer::new_from_unsigned(scope, u32::try_from(code).expect("a few attributes"));
    target.set_private(scope, member_key, number.into());
    let handler = handler(scope);
    let Some(array) = v8::Proxy::new(scope, target.into(), handler) else {
        return;
    };
    holder.set_private(scope, key, array.into());
    rv.set(array.into());
}

/// The attribute's setter (WebIDL's for an observable array type): the
/// value as a sequence of the item interface's objects (a `TypeError`
/// first, changing nothing, for one that isn't), every value deleted,
/// from the last, then each added in turn; one the set algorithm refuses
/// stops it there, the ones before it added.
pub(crate) fn setter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _: v8::ReturnValue<'s, v8::Value>,
) {
    let holder = args.this();
    let Some(code) = args
        .data()
        .int32_value(scope)
        .and_then(|code| usize::try_from(code).ok())
    else {
        return;
    };
    let Some(values) = sequence(scope, args.get(0)) else {
        return;
    };
    let item = match state::scope(scope) {
        Some(mut state) => state.state().0.observable[code].item,
        None => return,
    };
    if values.iter().any(|value| !is_item(scope, item, *value)) {
        crate::throw_type_error(
            scope,
            "Failed to convert value: a value is not of the observable array's type.",
        );
        return;
    }
    let Some(len) = list(scope, holder, code).map(|list| list.len()) else {
        return;
    };
    for _ in 0..len {
        if call(scope, code, Op::Delete, holder, &[]).is_none() {
            return;
        }
    }
    for (index, value) in values.into_iter().enumerate() {
        let index = v8::Integer::new_from_unsigned(scope, u32::try_from(index).unwrap_or(u32::MAX));
        if call(scope, code, Op::Set, holder, &[index.into(), value]).is_none() {
            return;
        }
    }
}

/// Whether `value` is an object of `INTERFACES[item]` (a wrapper of its GC
/// type): the one item interface an observable array has yet.
fn is_item(scope: &mut v8::PinScope<'_, '_>, item: usize, value: v8::Local<'_, v8::Value>) -> bool {
    match dom_api::INTERFACES[item].name {
        "CSSStyleSheet" => bindings::is_wrapper::<dom::CssStyleSheet>(scope, value),
        name => unreachable!("no observable array of {name} yet"),
    }
}

/// `value` iterated (WebIDL's conversion to a sequence); `None` with an
/// exception thrown.
pub(crate) fn sequence<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<Vec<v8::Local<'s, v8::Value>>> {
    let mut values = Vec::new();
    for_each_item(scope, value, |_, item| {
        values.push(item);
        Some(())
    })?;
    Some(values)
}

/// `f` on each item of `value` as it is iterated (WebIDL's conversion to a
/// sequence, which converts each item before it takes the next: what
/// converting one runs may change what comes after); `None` with an
/// exception thrown, by the iteration or by `f`, which stops it.
pub(crate) fn for_each_item<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    mut f: impl FnMut(&mut v8::PinScope<'s, '_>, v8::Local<'s, v8::Value>) -> Option<()>,
) -> Option<()> {
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        crate::throw_type_error(
            scope,
            "The provided value cannot be converted to a sequence.",
        );
        return None;
    };
    let symbol = v8::Symbol::get_iterator(scope);
    let method = object.get(scope, symbol.into())?;
    let Ok(method) = v8::Local::<v8::Function>::try_from(method) else {
        crate::throw_type_error(
            scope,
            "The provided value cannot be converted to a sequence.",
        );
        return None;
    };
    let iterator = method.call(scope, object.into(), &[])?;
    let Ok(iterator) = v8::Local::<v8::Object>::try_from(iterator) else {
        crate::throw_type_error(scope, "The iterator is not an object.");
        return None;
    };
    let next_key = templates::string(scope, "next");
    let next = iterator.get(scope, next_key.into())?;
    let Ok(next) = v8::Local::<v8::Function>::try_from(next) else {
        crate::throw_type_error(scope, "The iterator's next is not a function.");
        return None;
    };
    let (done_key, value_key) = (
        templates::string(scope, "done"),
        templates::string(scope, "value"),
    );
    loop {
        let result = next.call(scope, iterator.into(), &[])?;
        let Ok(result) = v8::Local::<v8::Object>::try_from(result) else {
            crate::throw_type_error(scope, "The iterator result is not an object.");
            return None;
        };
        if result.get(scope, done_key.into())?.boolean_value(scope) {
            return Some(());
        }
        let item = result.get(scope, value_key.into())?;
        f(scope, item)?;
    }
}

/// Attribute `code`'s operation `op` called on `owner` with `args`: what
/// it gave, or `None` with what it threw pending.
fn call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    code: usize,
    op: Op,
    owner: v8::Local<'s, v8::Object>,
    args: &[v8::Local<'s, v8::Value>],
) -> Option<v8::Local<'s, v8::Value>> {
    let Some(function) = function(scope, code, op) else {
        crate::throw_type_error(scope, "the DOM isn't available to this script");
        return None;
    };
    function.call(scope, owner.into(), args)
}

/// The backing list (the getter's array); `None` with an exception thrown.
fn list<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    code: usize,
) -> Option<Vec<v8::Local<'s, v8::Value>>> {
    let array = call(scope, code, Op::List, owner, &[])?;
    let array = v8::Local::<v8::Array>::try_from(array).ok()?;
    (0..array.length())
        .map(|index| array.get_index(scope, index))
        .collect()
}

/// The current realm's handler: made the first time, kept on its global.
fn handler<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Object> {
    let global = scope.get_current_context().global(scope);
    let key = private(scope, "handler");
    if let Some(handler) = global.get_private(scope, key)
        && let Ok(handler) = v8::Local::<v8::Object>::try_from(handler)
    {
        return handler;
    }
    // WebIDL's: an ordinary object with a null prototype.
    let handler = v8::Object::new(scope);
    let null = v8::null(scope).into();
    handler.set_prototype(scope, null);
    add_trap(scope, handler, "defineProperty", 3, define_property);
    add_trap(scope, handler, "deleteProperty", 2, delete_property);
    add_trap(scope, handler, "get", 3, get);
    add_trap(
        scope,
        handler,
        "getOwnPropertyDescriptor",
        2,
        get_own_property_descriptor,
    );
    add_trap(scope, handler, "has", 2, has);
    add_trap(scope, handler, "ownKeys", 1, own_keys);
    add_trap(scope, handler, "preventExtensions", 1, prevent_extensions);
    add_trap(scope, handler, "set", 4, set);
    global.set_private(scope, key, handler.into());
    handler
}

fn add_trap<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handler: v8::Local<'s, v8::Object>,
    name: &str,
    length: i32,
    callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
    let function = v8::Function::builder(callback)
        .length(length)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .expect("a function");
    let name = templates::string(scope, name);
    function.set_name(name);
    handler.create_data_property(scope, name.into(), function.into());
}

/// What a trap's property key is to the array.
enum Key {
    /// `length`: the list's.
    Length,
    /// An array index: the list's value there, if it is below the length.
    Index(u32),
    /// Any other: the target's (a symbol, `constructor`, an expando).
    Other,
}

fn key_of(scope: &mut v8::PinScope<'_, '_>, key: v8::Local<'_, v8::Value>) -> Key {
    if key.is_symbol() {
        return Key::Other;
    }
    let Ok(key) = v8::Local::<v8::String>::try_from(key) else {
        return Key::Other;
    };
    let key = key.to_rust_string_lossy(scope);
    if key == "length" {
        return Key::Length;
    }
    // ECMAScript's array index: the canonical numeric string of an integer
    // below 2^32 - 1.
    match key.parse::<u32>() {
        Ok(index) if index != u32::MAX && index.to_string() == key => Key::Index(index),
        _ => Key::Other,
    }
}

/// The platform object and attribute a trap's target stands for, and the
/// list; `None` with an exception thrown.
fn backing<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
) -> Option<(
    v8::Local<'s, v8::Object>,
    usize,
    Vec<v8::Local<'s, v8::Value>>,
)> {
    let owner_key = private(scope, "owner");
    let member_key = private(scope, "member");
    let owner = target
        .get_private(scope, owner_key)
        .and_then(|owner| v8::Local::<v8::Object>::try_from(owner).ok());
    let code = target
        .get_private(scope, member_key)
        .and_then(|code| code.uint32_value(scope))
        .and_then(|code| usize::try_from(code).ok());
    let (Some(owner), Some(code)) = (owner, code) else {
        crate::throw_type_error(scope, "not an observable array");
        return None;
    };
    let list = list(scope, owner, code)?;
    Some((owner, code, list))
}

/// A length or an index as a JS number.
fn number<'s>(scope: &mut v8::PinScope<'s, '_>, value: usize) -> v8::Local<'s, v8::Value> {
    v8::Integer::new_from_unsigned(scope, u32::try_from(value).unwrap_or(u32::MAX)).into()
}

/// WebIDL's "set the length": `value` as a length, deleting the values
/// from the end down to it; `false` for one past the end. `None` with an
/// exception thrown.
fn set_the_length<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    code: usize,
    value: v8::Local<'s, v8::Value>,
) -> Option<bool> {
    let uint32 = value.uint32_value(scope)?;
    let number = value.number_value(scope)?;
    if f64::from(uint32) != number {
        let message = templates::string(scope, "Invalid array length");
        let error = v8::Exception::range_error(scope, message);
        scope.throw_exception(error);
        return None;
    }
    // Converting may have run script that changed the list.
    let len = list(scope, owner, code)?.len();
    let new = usize::try_from(uint32).unwrap_or(usize::MAX);
    if new > len {
        return Some(false);
    }
    for _ in new..len {
        call(scope, code, Op::Delete, owner, &[])?;
    }
    Some(true)
}

/// WebIDL's "set the indexed value": `value` set at `index` (at the
/// length, added; its conversion the set operation's); `false` past the
/// length. `None` with an exception thrown.
fn set_the_indexed_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    code: usize,
    len: usize,
    index: u32,
    value: v8::Local<'s, v8::Value>,
) -> Option<bool> {
    if usize::try_from(index).unwrap_or(usize::MAX) > len {
        return Some(false);
    }
    let index = v8::Integer::new_from_unsigned(scope, index);
    call(scope, code, Op::Set, owner, &[index.into(), value])?;
    Some(true)
}

/// The trap's target (its first argument).
fn target<'s>(args: &v8::FunctionCallbackArguments<'s>) -> Option<v8::Local<'s, v8::Object>> {
    v8::Local::<v8::Object>::try_from(args.get(0)).ok()
}

fn get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let (Some(target), key) = (target(&args), args.get(1)) else {
        return;
    };
    let result = match key_of(scope, key) {
        Key::Other => target.get(scope, key),
        Key::Length => backing(scope, target).map(|(_, _, list)| number(scope, list.len())),
        Key::Index(index) => backing(scope, target).map(|(_, _, list)| {
            usize::try_from(index)
                .ok()
                .and_then(|index| list.get(index).copied())
                .unwrap_or_else(|| v8::undefined(scope).into())
        }),
    };
    if let Some(result) = result {
        rv.set(result);
    }
}

fn set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let (Some(target), key, value) = (target(&args), args.get(1), args.get(2)) else {
        return;
    };
    let done = match key_of(scope, key) {
        Key::Other => target.set(scope, key, value),
        Key::Length => backing(scope, target)
            .and_then(|(owner, code, _)| set_the_length(scope, owner, code, value)),
        Key::Index(index) => backing(scope, target).and_then(|(owner, code, list)| {
            set_the_indexed_value(scope, owner, code, list.len(), index, value)
        }),
    };
    if let Some(done) = done {
        rv.set_bool(done);
    }
}

fn delete_property<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let (Some(target), key) = (target(&args), args.get(1)) else {
        return;
    };
    let done = match key_of(scope, key) {
        Key::Other => target.delete(scope, key),
        Key::Length => Some(false),
        // Only the last value can go.
        Key::Index(index) => backing(scope, target).and_then(|(owner, code, list)| {
            if list.len().checked_sub(1) != usize::try_from(index).ok() {
                return Some(false);
            }
            call(scope, code, Op::Delete, owner, &[]).map(|_| true)
        }),
    };
    if let Some(done) = done {
        rv.set_bool(done);
    }
}

fn has<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let (Some(target), key) = (target(&args), args.get(1)) else {
        return;
    };
    let has = match key_of(scope, key) {
        Key::Other => target.has(scope, key),
        Key::Length => Some(true),
        Key::Index(index) => backing(scope, target)
            .map(|(_, _, list)| usize::try_from(index).is_ok_and(|index| index < list.len())),
    };
    if let Some(has) = has {
        rv.set_bool(has);
    }
}

/// A data property's descriptor object (ECMAScript's
/// `FromPropertyDescriptor`).
fn data_descriptor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> v8::Local<'s, v8::Value> {
    let descriptor = v8::Object::new(scope);
    let fields = [
        ("value", value),
        ("writable", v8::Boolean::new(scope, writable).into()),
        ("enumerable", v8::Boolean::new(scope, enumerable).into()),
        ("configurable", v8::Boolean::new(scope, configurable).into()),
    ];
    for (name, value) in fields {
        let name = templates::string(scope, name);
        descriptor.create_data_property(scope, name.into(), value);
    }
    descriptor.into()
}

fn get_own_property_descriptor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let (Some(target), key) = (target(&args), args.get(1)) else {
        return;
    };
    let descriptor = match key_of(scope, key) {
        Key::Other => v8::Local::<v8::Name>::try_from(key)
            .ok()
            .and_then(|key| target.get_own_property_descriptor(scope, key)),
        // Writable, so it may differ from the target's own (0).
        Key::Length => backing(scope, target).map(|(_, _, list)| {
            let len = number(scope, list.len());
            data_descriptor(scope, len, true, false, false)
        }),
        Key::Index(index) => backing(scope, target).map(|(_, _, list)| {
            match usize::try_from(index)
                .ok()
                .and_then(|index| list.get(index).copied())
            {
                Some(value) => data_descriptor(scope, value, true, true, true),
                None => v8::undefined(scope).into(),
            }
        }),
    };
    if let Some(descriptor) = descriptor {
        rv.set(descriptor);
    }
}

/// A property descriptor's fields (ECMAScript's `ToPropertyDescriptor`).
#[derive(Default)]
struct Descriptor<'s> {
    enumerable: Option<bool>,
    configurable: Option<bool>,
    value: Option<v8::Local<'s, v8::Value>>,
    writable: Option<bool>,
    get: Option<v8::Local<'s, v8::Value>>,
    set: Option<v8::Local<'s, v8::Value>>,
}

impl<'s> Descriptor<'s> {
    /// `object`'s fields; `None` with an exception thrown (not an object,
    /// a getter or setter that isn't callable, both an accessor and a
    /// value or `writable`).
    fn from_js(scope: &mut v8::PinScope<'s, '_>, object: v8::Local<'s, v8::Value>) -> Option<Self> {
        let Ok(object) = v8::Local::<v8::Object>::try_from(object) else {
            crate::throw_type_error(scope, "Property description must be an object");
            return None;
        };
        let field = |name: &str| -> Option<Option<v8::Local<'s, v8::Value>>> {
            let name = templates::string(scope, name).into();
            if !object.has(scope, name)? {
                return Some(None);
            }
            object.get(scope, name).map(Some)
        };
        let enumerable = field("enumerable")?;
        let configurable = field("configurable")?;
        let value = field("value")?;
        let writable = field("writable")?;
        let get = field("get")?;
        let set = field("set")?;
        let descriptor = Self {
            enumerable: enumerable.map(|value| value.boolean_value(scope)),
            configurable: configurable.map(|value| value.boolean_value(scope)),
            value,
            writable: writable.map(|value| value.boolean_value(scope)),
            get,
            set,
        };
        let callable = |accessor: Option<v8::Local<v8::Value>>| {
            accessor.is_none_or(|accessor| accessor.is_undefined() || accessor.is_function())
        };
        if !callable(descriptor.get) || !callable(descriptor.set) {
            crate::throw_type_error(scope, "Getter and setter must be functions");
            return None;
        }
        if descriptor.is_accessor() && (descriptor.value.is_some() || descriptor.writable.is_some())
        {
            crate::throw_type_error(
                scope,
                "Invalid property descriptor. Cannot both specify accessors and a value or writable attribute",
            );
            return None;
        }
        Some(descriptor)
    }

    fn is_accessor(&self) -> bool {
        self.get.is_some() || self.set.is_some()
    }

    /// As V8's. It has no descriptor with only one of `get` and `set`, nor
    /// `writable` without `value`: the missing one is `undefined` (which
    /// differs only when redefining a property of the target that has it).
    fn to_v8(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::PropertyDescriptor {
        let undefined = || v8::undefined(scope).into();
        let mut descriptor = if self.is_accessor() {
            v8::PropertyDescriptor::new_from_get_set(
                self.get.unwrap_or_else(undefined),
                self.set.unwrap_or_else(undefined),
            )
        } else if let Some(writable) = self.writable {
            v8::PropertyDescriptor::new_from_value_writable(
                self.value.unwrap_or_else(undefined),
                writable,
            )
        } else if let Some(value) = self.value {
            v8::PropertyDescriptor::new_from_value(value)
        } else {
            v8::PropertyDescriptor::new()
        };
        if let Some(enumerable) = self.enumerable {
            descriptor.set_enumerable(enumerable);
        }
        if let Some(configurable) = self.configurable {
            descriptor.set_configurable(configurable);
        }
        descriptor
    }
}

fn define_property<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let (Some(target), key) = (target(&args), args.get(1)) else {
        return;
    };
    let Some(descriptor) = Descriptor::from_js(scope, args.get(2)) else {
        return;
    };
    let done = match key_of(scope, key) {
        Key::Other => {
            let descriptor = descriptor.to_v8(scope);
            v8::Local::<v8::Name>::try_from(key)
                .ok()
                .and_then(|key| target.define_property(scope, key, &descriptor))
        }
        // Only a writable data property that is neither enumerable nor
        // configurable: its value is the new length.
        Key::Length => {
            if descriptor.is_accessor()
                || descriptor.configurable == Some(true)
                || descriptor.enumerable == Some(true)
                || descriptor.writable == Some(false)
            {
                Some(false)
            } else {
                match descriptor.value {
                    Some(value) => backing(scope, target)
                        .and_then(|(owner, code, _)| set_the_length(scope, owner, code, value)),
                    None => Some(true),
                }
            }
        }
        // Only a writable, enumerable, configurable data property.
        Key::Index(index) => {
            if descriptor.is_accessor()
                || descriptor.configurable == Some(false)
                || descriptor.enumerable == Some(false)
                || descriptor.writable == Some(false)
            {
                Some(false)
            } else {
                match descriptor.value {
                    Some(value) => backing(scope, target).and_then(|(owner, code, list)| {
                        set_the_indexed_value(scope, owner, code, list.len(), index, value)
                    }),
                    None => Some(true),
                }
            }
        }
    };
    if let Some(done) = done {
        rv.set_bool(done);
    }
}

/// The list's indices, then the target's own keys (`length`, any
/// expandos and symbols).
fn own_keys<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let Some(target) = target(&args) else {
        return;
    };
    let keys = backing(scope, target).and_then(|(_, _, list)| {
        let own = target.get_own_property_names(
            scope,
            v8::GetPropertyNamesArgs {
                mode: v8::KeyCollectionMode::OwnOnly,
                property_filter: v8::PropertyFilter::ALL_PROPERTIES,
                index_filter: v8::IndexFilter::IncludeIndices,
                key_conversion: v8::KeyConversionMode::ConvertToString,
            },
        )?;
        let mut keys: Vec<v8::Local<v8::Value>> = (0..list.len())
            .map(|index| templates::string(scope, &index.to_string()).into())
            .collect();
        for index in 0..own.length() {
            keys.push(own.get_index(scope, index)?);
        }
        Some(v8::Array::new_with_elements(scope, &keys))
    });
    if let Some(keys) = keys {
        rv.set(keys.into());
    }
}

/// An observable array can't be made non-extensible.
fn prevent_extensions<'s>(
    _scope: &mut v8::PinScope<'s, '_>,
    _args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    rv.set_bool(false);
}
