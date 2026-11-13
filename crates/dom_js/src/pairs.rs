//! WebIDL's pair iterator (`iterable<K, V>`: `URLSearchParams`), Blink's
//! `PairSyncIterable` and `SyncIterationSource`: `entries`, `keys`,
//! `values`, `forEach` and `@@iterator` on the interface's prototype, and
//! its default iterator objects. Each step calls the interface's `#[pairs]`
//! member's typed path (`[key, value]` at an index, none past the last), so
//! `next()` and each step of `forEach` see the list as it is then, as
//! WebIDL's "value pairs to iterate over" are.
//!
//! A default iterator object is an ordinary object with private
//! properties: its target, kind and index. Its prototype, the interface's
//! iterator prototype object (`%URLSearchParamsIteratorPrototype%`), whose
//! prototype is `%IteratorPrototype%`, is made once per realm and kept on
//! the global, under a private key, so it lives as long as the realm does.

use std::marker::PhantomData;

use dom_api::{INTERFACES, TypedMember};

use crate::front::{IndexArgs, WholeFront};
use crate::state::{self, Scope, State};
use crate::templates::{self, string};

/// What an iterator yields (WebIDL's "kind").
const KEY: u8 = 0;
const VALUE: u8 = 1;
const KEY_VALUE: u8 = 2;

/// Puts `M`'s interface's iterable methods on its prototype: operations
/// (enumerable), and `@@iterator` the same function as `entries` (not
/// enumerable).
pub(crate) fn install<'s, M: TypedMember>(
    scope: &mut v8::PinScope<'s, '_>,
    prototype: v8::Local<'s, v8::ObjectTemplate>,
    signature: v8::Local<'s, v8::Signature>,
) {
    let entries = iterator_method::<M, KEY_VALUE>(scope, signature);
    for (name, template) in [
        ("entries", entries),
        ("keys", iterator_method::<M, KEY>(scope, signature)),
        ("values", iterator_method::<M, VALUE>(scope, signature)),
    ] {
        let key = string(scope, name);
        prototype.set_with_attr(key.into(), template.into(), v8::PropertyAttribute::NONE);
    }
    let for_each = bindings::state_function_template::<State, ForEach<M>>(scope, |builder| {
        builder
            .signature(signature)
            .length(1)
            .constructor_behavior(v8::ConstructorBehavior::Throw)
    });
    templates::checks_receiver(for_each);
    let key = string(scope, "forEach");
    prototype.set_with_attr(key.into(), for_each.into(), v8::PropertyAttribute::NONE);
    let iterator = v8::Symbol::get_iterator(scope);
    prototype.set_with_attr(
        iterator.into(),
        entries.into(),
        v8::PropertyAttribute::DONT_ENUM,
    );
}

fn iterator_method<'s, M: TypedMember, const KIND: u8>(
    scope: &mut v8::PinScope<'s, '_>,
    signature: v8::Local<'s, v8::Signature>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let template =
        bindings::state_function_template::<State, Iterator<M, KIND>>(scope, |builder| {
            builder
                .signature(signature)
                .length(0)
                .constructor_behavior(v8::ConstructorBehavior::Throw)
        });
    templates::checks_receiver(template);
    template
}

/// A private key of `M`'s interface's iterators, by name: the same in
/// every context.
fn private<'s, M: TypedMember>(
    scope: &mut v8::PinScope<'s, '_>,
    what: &str,
) -> v8::Local<'s, v8::Private> {
    let name = string(
        scope,
        &format!("machopo:{} Iterator#{what}", INTERFACES[M::INTERFACE].name),
    );
    v8::Private::for_api(scope, Some(name))
}

/// The pair at `index` of `target`, `M`'s call: `[key, value]`, an empty
/// array past the last; `None` with an exception pending.
fn pair<'s, M: TypedMember>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
    index: u32,
    rv: v8::ReturnValue<'_, v8::Value>,
) -> Option<v8::Local<'s, v8::Array>> {
    let undefined = v8::undefined(scope).into();
    // The iterator's target, unwrapped whole: what `entries` kept of its
    // receiver is no `Receiver`.
    let call = IndexArgs {
        index: v8::Integer::new_from_unsigned(scope, index).into(),
        this: target,
        receiver: None,
        undefined,
    };
    {
        let scope = state::scope(scope)?;
        let mut front = WholeFront::new(scope, &call, rv);
        M::call(&mut front);
    }
    v8::Local::<v8::Array>::try_from(rv.get(scope)).ok()
}

/// `entries`, `keys`, `values` (and `@@iterator`): a new default iterator
/// object over `this`, at index 0.
struct Iterator<M, const KIND: u8>(PhantomData<M>);

impl<M: TypedMember, const KIND: u8> bindings::StateCallback<State> for Iterator<M, KIND> {
    fn call<'s>(
        mut scope: Scope<'_, 's, '_>,
        args: v8::FunctionCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'s>,
    ) {
        let scope = scope.scope();
        let prototype = iterator_prototype::<M>(scope);
        let iterator = v8::Object::with_prototype_and_properties(scope, prototype.into(), &[], &[]);
        let target = private::<M>(scope, "target");
        iterator.set_private(scope, target, args.this().into());
        let kind = private::<M>(scope, "kind");
        let value = v8::Integer::new_from_unsigned(scope, u32::from(KIND));
        iterator.set_private(scope, kind, value.into());
        let index = private::<M>(scope, "index");
        let zero = v8::Integer::new_from_unsigned(scope, 0);
        iterator.set_private(scope, index, zero.into());
        rv.set(iterator.into());
    }
}

/// `M`'s interface's iterator prototype object in the current realm: made
/// the first time, kept on the realm's global.
fn iterator_prototype<'s, M: TypedMember>(
    scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Object> {
    let global = scope.get_current_context().global(scope);
    let key = private::<M>(scope, "prototype");
    if let Some(prototype) = global.get_private(scope, key)
        && let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype)
    {
        return prototype;
    }
    let prototype = v8::Object::new(scope);
    let intrinsic = iterator_prototype_intrinsic(scope);
    prototype.set_prototype(scope, intrinsic);
    let next = bindings::state_function_template::<State, Next<M>>(scope, |builder| {
        builder
            .length(0)
            .constructor_behavior(v8::ConstructorBehavior::Throw)
    })
    .get_function(scope)
    .expect("a function");
    let name = string(scope, "next");
    next.set_name(name);
    prototype.define_own_property(scope, name.into(), next.into(), v8::PropertyAttribute::NONE);
    // WebIDL's class string: not writable, not enumerable, configurable.
    let tag = v8::Symbol::get_to_string_tag(scope);
    let class = string(
        scope,
        &format!("{} Iterator", INTERFACES[M::INTERFACE].name),
    );
    prototype.define_own_property(
        scope,
        tag.into(),
        class.into(),
        v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_ENUM,
    );
    global.set_private(scope, key, prototype.into());
    prototype
}

/// The current realm's `%IteratorPrototype%`: read from V8's intrinsic, not
/// from objects script can change.
fn iterator_prototype_intrinsic<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
    let template = v8::ObjectTemplate::new(scope);
    let name = string(scope, "prototype");
    template.set_intrinsic_data_property(
        name.into(),
        v8::Intrinsic::IteratorPrototype,
        v8::PropertyAttribute::NONE,
    );
    let holder = template.new_instance(scope).expect("an object");
    holder.get(scope, name.into()).expect("the intrinsic")
}

/// A default iterator object's `next()` (WebIDL's): the pair at its index,
/// if the list has one now, and the index moves on; else done.
struct Next<M>(PhantomData<M>);

impl<M: TypedMember> bindings::StateCallback<State> for Next<M> {
    fn call<'s>(
        mut scope: Scope<'_, 's, '_>,
        args: v8::FunctionCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'s>,
    ) {
        let scope = scope.scope();
        let iterator = args.this();
        let target_key = private::<M>(scope, "target");
        let target = iterator
            .get_private(scope, target_key)
            .and_then(|target| v8::Local::<v8::Object>::try_from(target).ok());
        let Some(target) = target else {
            let message = format!(
                "next called on an object that isn't a {} Iterator",
                INTERFACES[M::INTERFACE].name
            );
            crate::throw_type_error(scope, &message);
            return;
        };
        let kind_key = private::<M>(scope, "kind");
        let kind = iterator
            .get_private(scope, kind_key)
            .and_then(|kind| kind.uint32_value(scope))
            .unwrap_or(u32::from(KEY_VALUE));
        let index_key = private::<M>(scope, "index");
        let index = iterator
            .get_private(scope, index_key)
            .and_then(|index| index.uint32_value(scope))
            .unwrap_or(0);
        let Some(pair) = pair::<M>(scope, target, index, rv) else {
            return;
        };
        if pair.length() < 2 {
            let undefined = v8::undefined(scope).into();
            rv.set(iter_result(scope, undefined, true).into());
            return;
        }
        let value = match u8::try_from(kind) {
            Ok(KEY) => pair.get_index(scope, 0),
            Ok(VALUE) => pair.get_index(scope, 1),
            _ => Some(pair.into()),
        };
        let Some(value) = value else {
            return;
        };
        let next = v8::Integer::new_from_unsigned(scope, index.saturating_add(1));
        iterator.set_private(scope, index_key, next.into());
        rv.set(iter_result(scope, value, false).into());
    }
}

/// ECMAScript's `CreateIterResultObject`.
fn iter_result<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    done: bool,
) -> v8::Local<'s, v8::Object> {
    let result = v8::Object::new(scope);
    let value_key = string(scope, "value");
    result.create_data_property(scope, value_key.into(), value);
    let done_key = string(scope, "done");
    let done = v8::Boolean::new(scope, done);
    result.create_data_property(scope, done_key.into(), done.into());
    result
}

/// `forEach(callback, thisArg)`: calls `callback` with each pair's value,
/// key and the object, reading the list again after each call (WebIDL's
/// `forEach` of a pair iterable).
struct ForEach<M>(PhantomData<M>);

impl<M: TypedMember> bindings::StateCallback<State> for ForEach<M> {
    fn call<'s>(
        mut scope: Scope<'_, 's, '_>,
        args: v8::FunctionCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'s>,
    ) {
        let scope = scope.scope();
        let this = args.this();
        let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
            let message = format!(
                "Failed to execute 'forEach' on '{}': The callback provided as parameter 1 is not a function.",
                INTERFACES[M::INTERFACE].name
            );
            crate::throw_type_error(scope, &message);
            return;
        };
        let this_arg = args.get(1);
        let mut index = 0u32;
        loop {
            let Some(pair) = pair::<M>(scope, this, index, rv) else {
                return;
            };
            if pair.length() < 2 {
                break;
            }
            let (Some(key), Some(value)) = (pair.get_index(scope, 0), pair.get_index(scope, 1))
            else {
                return;
            };
            // `None`: the callback threw; it propagates.
            if callback
                .call(scope, this_arg, &[value, key, this.into()])
                .is_none()
            {
                return;
            }
            index = index.saturating_add(1);
        }
        rv.set_undefined();
    }
}
