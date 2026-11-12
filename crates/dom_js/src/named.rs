//! WebIDL's named properties (`dataset.fooBar`; Blink's generated
//! `NamedPropertyGetterCallback` and the rest): interceptors on an
//! interface's instances that call its anonymous special operations'
//! typed paths (`#[named(..)]`), through a front end of their own
//! (`PropertyFront`): an interceptor's result is a `Boolean` or an `Array`,
//! not a function's `Value`, so the operation's result is kept as the
//! DOM's and made V8's after.
//!
//! - Get: the getter (a string, or an object: `form[name]`); its none is
//!   no property (V8 looks on).
//! - Query: the getter too; a named property is configurable, writable
//!   with a setter, enumerable with its names listed (else as with
//!   `[LegacyUnenumerableNamedProperties]`).
//! - Set: the setter; one that gives `false` didn't take the name, an
//!   ordinary property's (`style.notAProperty = 1`).
//! - Define: a value goes through the setter; a getter or setter is
//!   refused (WebIDL's [[DefineOwnProperty]]).
//! - Delete: a named property through the deleter; another name is an
//!   ordinary property's.
//! - Enumerate: the supported names, own keys.
//!
//! An array index is a name like any other (`dataset[9]`), as in Blink's
//! bindings.
//!
//! An interface's indexed setter (`select[0] = option`) goes through the
//! same front end, from its indexed interceptor (`IndexedSetter`): the
//! index and the value its arguments, the setter always taking the index.

use dom::{DomString, NodeRef, ObjectRef, ObjectRoot};
use dom_api::{ApiCx, ApiError, Front, FrontTypes, Named, OpFront, TypedMember};

use crate::exception;
use crate::front::cached_string;
use crate::state::{self, Scope, State};
use crate::strings::{self, ArgString};

/// A special operation's typed path, through `PropertyFront`.
pub(crate) type Call = for<'a, 's, 'i> fn(&mut PropertyFront<'a, 's, 'i>);

/// An interface's named properties' special operations, found as its
/// template is made.
#[derive(Clone, Copy, Default)]
pub(crate) struct NamedCalls {
    pub(crate) get: Option<Call>,
    set: Option<Call>,
    delete: Option<Call>,
    names: Option<Call>,
    /// The indexed setter (`#[indexed(setter)]`), which the indexed
    /// interceptor calls.
    pub(crate) indexed_set: Option<Call>,
}

impl NamedCalls {
    pub(crate) fn add<M: TypedMember>(&mut self, named: Named) {
        let call: Call = call::<M>;
        let slot = match named {
            Named::Getter => &mut self.get,
            Named::Setter => &mut self.set,
            Named::Deleter => &mut self.delete,
            Named::Names => &mut self.names,
        };
        *slot = Some(call);
    }

    pub(crate) fn add_indexed_setter<M: TypedMember>(&mut self) {
        self.indexed_set = Some(call::<M>);
    }

    /// The interceptors' options, for an interface whose named properties
    /// shadow its own (`masking`) or not, and with indexed properties of
    /// its own or not.
    pub(crate) fn options(&self, masking: bool, indexed: bool) -> bindings::NamedOptions {
        bindings::NamedOptions {
            masking,
            setter: self.set.is_some(),
            deleter: self.delete.is_some(),
            enumerator: self.names.is_some(),
            indices: !indexed,
        }
    }
}

fn call<M: TypedMember>(front: &mut PropertyFront<'_, '_, '_>) {
    M::call(front);
}

/// What a special operation gave.
enum Got {
    Undefined,
    Null,
    String(DomString),
    Strings(Vec<DomString>),
    /// An object, its wrapper's interface: its wrapper made after the
    /// operation.
    Object(ObjectRoot, usize),
    /// A setter's: whether it took the name (else it is an ordinary
    /// property's).
    Bool(bool),
    /// It threw: V8 has the exception pending.
    Threw,
}

/// A special operation's call: its receiver (the interceptor's holder), the
/// name and a setter's value, and what it gave.
pub(crate) struct PropertyFront<'a, 's, 'i> {
    scope: Scope<'a, 's, 'i>,
    this: v8::Local<'s, v8::Object>,
    args: [Option<v8::Local<'s, v8::Value>>; 2],
    got: Got,
}

/// Its side in the operation.
pub(crate) struct PropertyOp<'o, 's, 'i> {
    no_gc: bindings::NoGc<'o, 's, 'i>,
    this: v8::Local<'s, v8::Object>,
    got: &'o mut Got,
}

impl<'s> FrontTypes for PropertyFront<'_, 's, '_> {
    type Str = ArgString<'s>;
    type Node = v8::Local<'s, v8::Object>;
    type Dict = ();
    type PendingString = std::convert::Infallible;
    type PendingNode = std::convert::Infallible;
    type PendingNumber = std::convert::Infallible;
    type PendingObject = std::convert::Infallible;
    type PreparedNode = ();
    type Callback = std::convert::Infallible;
    type Any = std::convert::Infallible;
    type Rest = std::convert::Infallible;
}

impl<'s> FrontTypes for PropertyOp<'_, 's, '_> {
    type Str = ArgString<'s>;
    type Node = v8::Local<'s, v8::Object>;
    type Dict = ();
    type PendingString = std::convert::Infallible;
    type PendingNode = std::convert::Infallible;
    type PendingNumber = std::convert::Infallible;
    type PendingObject = std::convert::Infallible;
    type PreparedNode = ();
    type Callback = std::convert::Infallible;
    type Any = std::convert::Infallible;
    type Rest = std::convert::Infallible;
}

impl<'a, 's, 'i> Front for PropertyFront<'a, 's, 'i> {
    type Op<'o>
        = PropertyOp<'o, 's, 'i>
    where
        Self: 'o;

    fn read<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let (state, no_gc) = self.scope.state();
        let mut op = PropertyOp {
            no_gc,
            this: self.this,
            got: &mut self.got,
        };
        state
            .dom
            .read(|cx| f(&ApiCx::with_layout_cell(cx, &state.layout), &mut op))
    }

    fn mutate<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let (state, no_gc) = self.scope.state_mut();
        let mut op = PropertyOp {
            no_gc,
            this: self.this,
            got: &mut self.got,
        };
        let crate::state::State { dom, layout, .. } = state;
        dom.mutate(|cx| f(&ApiCx::with_layout_cell(cx, layout), &mut op))
    }

    // An interceptor gives each operation its arguments.
    fn has_args(&mut self, _: &str, _: usize) -> bool {
        true
    }

    fn string_arg(&mut self, index: usize) -> Option<ArgString<'s>> {
        let value = match self.args.get(index).copied().flatten() {
            Some(value) => value,
            None => v8::undefined(self.scope.scope()).into(),
        };
        strings::arg_string(self.scope.scope(), value)
    }

    fn nullable_string_arg(&mut self, _: usize) -> Option<Option<ArgString<'s>>> {
        unreachable!("a named property's operation takes strings")
    }

    fn legacy_null_string_arg(&mut self, index: usize) -> Option<Option<ArgString<'s>>> {
        match self.args.get(index).copied().flatten() {
            Some(value) if value.is_null() => Some(None),
            _ => self.string_arg(index).map(Some),
        }
    }

    // An indexed setter's value.
    fn node_arg(&mut self, index: usize) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = match self.args.get(index).copied().flatten() {
            Some(value) if !value.is_null_or_undefined() => value,
            _ => return Some(None),
        };
        if bindings::is_wrapper::<dom::Node>(self.scope.scope(), value) {
            return v8::Local::<v8::Object>::try_from(value).ok().map(Some);
        }
        crate::throw_type_error(
            self.scope.scope(),
            &format!(
                "parameter {} is not of type 'Node'.",
                index.saturating_add(1)
            ),
        );
        self.got = Got::Threw;
        None
    }

    // An indexed setter's index.
    fn u32_arg(&mut self, index: usize) -> Option<u32> {
        let value = self.args.get(index).copied().flatten()?;
        crate::front::to_uint32(self.scope.scope(), value)
    }

    /// A setter's reactions (`dataset.x = ...`, an options collection's
    /// indexed setter): the lifecycle callbacks they call.
    fn call_lifecycle_callback(
        &mut self,
        element: &elise::Persistent<dom::Node>,
        definition: &elise::Persistent<dom::CustomElementDefinition>,
        callback: dom_api::LifecycleCallback,
    ) {
        crate::front::call_lifecycle_callback(&mut self.scope, element, definition, callback);
    }

    /// A named property's names (the enumerator's): kept, as its other
    /// results are.
    fn finish_strings(&mut self, strings: Vec<DomString>) {
        self.got = Got::Strings(strings);
    }

    fn throw(&mut self, error: ApiError) {
        self.got = Got::Threw;
        match error {
            ApiError::Type(message) => crate::throw_type_error(self.scope.scope(), &message),
            ApiError::Range(message) => {
                crate::strings::throw_range_error(self.scope.scope(), &message)
            }
            ApiError::Dom(exception) => exception::throw_dom_exception(
                &mut self.scope,
                exception.name.name(),
                &exception.message,
            ),
        }
    }
}

impl<'s> OpFront for PropertyOp<'_, 's, '_> {
    fn str_param<'a>(&mut self, string: &'a ArgString<'s>) -> &'a str {
        string.text(&mut self.no_gc)
    }

    /// The interceptor is on the interface's instances: its holder is one
    /// of its objects.
    fn this_as<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
    ) -> Option<elise::Gc<'gc, T>> {
        self.no_gc.unwrap::<T>(cx.gc, self.this)
    }

    fn node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: &v8::Local<'s, v8::Object>) -> NodeRef<'gc> {
        self.no_gc
            .unwrap::<dom::Node>(cx.gc, *node)
            .expect("a wrapper, checked as it was taken")
    }

    fn ret_undefined(&mut self) {
        *self.got = Got::Undefined;
    }

    fn ret_null(&mut self) {
        *self.got = Got::Null;
    }

    fn ret_bool(&mut self, value: bool) {
        *self.got = Got::Bool(value);
    }

    fn ret_u32(&mut self, _: u32) -> Option<std::convert::Infallible> {
        unreachable!("a named property's operation gives strings")
    }

    fn ret_f64(&mut self, _: f64) -> Option<std::convert::Infallible> {
        unreachable!("a named property's operation gives strings")
    }

    fn ret_string(&mut self, value: &DomString) -> Option<std::convert::Infallible> {
        *self.got = Got::String(value.clone());
        None
    }

    fn ret_node<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        node: NodeRef<'gc>,
    ) -> Option<std::convert::Infallible> {
        self.ret_object(cx, ObjectRef::Node(node))
    }

    fn ret_object<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: ObjectRef<'gc>,
    ) -> Option<std::convert::Infallible> {
        *self.got = Got::Object(cx.root_object(object), dom_api::interface_of(cx, object));
        None
    }
}

/// The interceptors of `NamedCalls` (the interface's, by its index in
/// the callbacks' data).
pub(crate) struct NamedProperties;

/// The special operations of the interceptor's interface.
fn calls(scope: &mut Scope<'_, '_, '_>, args: &v8::PropertyCallbackArguments<'_>) -> NamedCalls {
    let interface = v8::Local::<v8::Integer>::try_from(args.data())
        .ok()
        .and_then(|index| usize::try_from(index.value()).ok())
        .expect("named interceptors carry their interface's index");
    scope.state().0.named[interface]
}

/// The key as the operations take it: a name, or an index as one.
fn key_value<'s>(
    scope: &mut Scope<'_, 's, '_>,
    key: bindings::PropertyKey<'s>,
) -> v8::Local<'s, v8::Value> {
    match key {
        bindings::PropertyKey::Name(name) => name.into(),
        bindings::PropertyKey::Index(index) => {
            v8::Integer::new_from_unsigned(scope.scope(), index).into()
        }
    }
}

/// `call` on `this`, with `args`: what it gave, and the scope back.
fn run<'a, 's, 'i>(
    scope: Scope<'a, 's, 'i>,
    call: Call,
    this: v8::Local<'s, v8::Object>,
    args: [Option<v8::Local<'s, v8::Value>>; 2],
) -> (Scope<'a, 's, 'i>, Got) {
    let mut front = PropertyFront {
        scope,
        this,
        args,
        got: Got::Undefined,
    };
    call(&mut front);
    (front.scope, front.got)
}

/// Whether the getter gives `key` (`None` if it threw).
fn has<'a, 's, 'i>(
    scope: Scope<'a, 's, 'i>,
    calls: NamedCalls,
    key: v8::Local<'s, v8::Value>,
    this: v8::Local<'s, v8::Object>,
) -> (Scope<'a, 's, 'i>, Option<bool>) {
    let get = calls.get.expect("a named getter");
    let (scope, got) = run(scope, get, this, [Some(key), None]);
    let has = match got {
        Got::Threw => None,
        Got::Undefined | Got::Null => Some(false),
        Got::String(_) | Got::Object(..) => Some(true),
        Got::Strings(_) | Got::Bool(_) => {
            unreachable!("a named getter gives a string or an object")
        }
    };
    (scope, has)
}

/// Whether the indexed getter `M` gives an item at `index` (`None` if it
/// threw): neither null nor an empty string (a `DOMString` getter's past
/// its length), as its interceptor sees it.
pub(crate) fn has_index<'a, 's, 'i, M: TypedMember>(
    mut scope: Scope<'a, 's, 'i>,
    index: u32,
    this: v8::Local<'s, v8::Object>,
) -> (Scope<'a, 's, 'i>, Option<bool>) {
    let index = v8::Integer::new_from_unsigned(scope.scope(), index).into();
    let (scope, got) = run(scope, call::<M>, this, [Some(index), None]);
    let has = match got {
        Got::Threw => None,
        Got::Undefined | Got::Null => Some(false),
        Got::String(string) => Some(!string.is_empty()),
        Got::Object(..) => Some(true),
        Got::Strings(_) | Got::Bool(_) => {
            unreachable!("an indexed getter gives a string or an object")
        }
    };
    (scope, has)
}

/// Whether the indexed interceptor's interface (by its index in the
/// callbacks' data) has an indexed setter.
pub(crate) fn has_indexed_setter(
    scope: &mut Scope<'_, '_, '_>,
    args: &v8::PropertyCallbackArguments<'_>,
) -> bool {
    calls(scope, args).indexed_set.is_some()
}

/// What the setter given `key` and `value` did.
enum Assigned {
    /// It took the name.
    Took,
    /// It didn't: the name is an ordinary property's.
    Declined,
    Threw,
}

/// The setter given `key` and `value`.
fn assign<'s>(
    scope: Scope<'_, 's, '_>,
    calls: NamedCalls,
    key: v8::Local<'s, v8::Value>,
    value: v8::Local<'s, v8::Value>,
    this: v8::Local<'s, v8::Object>,
) -> Assigned {
    let set = calls.set.expect("a named setter");
    let (mut scope, got) = run(scope, set, this, [Some(key), Some(value)]);
    state::report(&mut scope);
    match got {
        Got::Threw => Assigned::Threw,
        Got::Bool(false) => Assigned::Declined,
        _ => Assigned::Took,
    }
}

impl bindings::StateNamedProperties<State> for NamedProperties {
    fn get<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted {
        let calls = calls(&mut scope, &args);
        let key = key_value(&mut scope, key);
        let get = calls.get.expect("a named getter");
        let (mut scope, got) = run(scope, get, args.holder(), [Some(key), None]);
        match got {
            Got::Undefined | Got::Null => v8::Intercepted::kNo,
            Got::Threw => v8::Intercepted::kYes,
            Got::String(value) => {
                match cached_string(&mut scope, &value) {
                    Some(string) => rv.set(string.into()),
                    None => exception::throw_range_error(scope.scope(), "Invalid string length"),
                }
                v8::Intercepted::kYes
            }
            Got::Object(object, interface) => {
                let wrapper = crate::front::wrapper_of(&mut scope, &object, interface);
                rv.set(wrapper.into());
                v8::Intercepted::kYes
            }
            Got::Strings(_) | Got::Bool(_) => {
                unreachable!("a named getter gives a string or an object")
            }
        }
    }

    fn query<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Integer>,
    ) -> v8::Intercepted {
        let calls = calls(&mut scope, &args);
        let key = key_value(&mut scope, key);
        match has(scope, calls, key, args.holder()).1 {
            Some(false) => v8::Intercepted::kNo,
            None => v8::Intercepted::kYes,
            Some(true) => {
                let mut attributes = v8::PropertyAttribute::NONE;
                if calls.set.is_none() {
                    attributes = attributes | v8::PropertyAttribute::READ_ONLY;
                }
                if calls.names.is_none() {
                    attributes = attributes | v8::PropertyAttribute::DONT_ENUM;
                }
                rv.set_int32(attributes.as_u32().cast_signed());
                v8::Intercepted::kYes
            }
        }
    }

    fn set<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        value: v8::Local<'s, v8::Value>,
        args: v8::PropertyCallbackArguments<'s>,
    ) -> v8::Intercepted {
        let calls = calls(&mut scope, &args);
        let key = key_value(&mut scope, key);
        match assign(scope, calls, key, value, args.holder()) {
            Assigned::Declined => v8::Intercepted::kNo,
            Assigned::Took | Assigned::Threw => v8::Intercepted::kYes,
        }
    }

    fn define<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        descriptor: &v8::PropertyDescriptor,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Boolean>,
    ) -> v8::Intercepted {
        if descriptor.has_get() || descriptor.has_set() {
            rv.set_bool(false);
            return v8::Intercepted::kYes;
        }
        let calls = calls(&mut scope, &args);
        let key = key_value(&mut scope, key);
        let value = if descriptor.has_value() {
            // In this scope: the descriptor's handle lives only as long as it.
            v8::Local::new(scope.scope(), descriptor.value())
        } else {
            v8::undefined(scope.scope()).into()
        };
        match assign(scope, calls, key, value, args.holder()) {
            Assigned::Declined => return v8::Intercepted::kNo,
            Assigned::Took => rv.set_bool(true),
            Assigned::Threw => {}
        }
        v8::Intercepted::kYes
    }

    fn delete<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Boolean>,
    ) -> v8::Intercepted {
        let calls = calls(&mut scope, &args);
        let key = key_value(&mut scope, key);
        let this = args.holder();
        let (scope, has) = has(scope, calls, key, this);
        match has {
            Some(false) => return v8::Intercepted::kNo,
            None => return v8::Intercepted::kYes,
            Some(true) => {}
        }
        let delete = calls.delete.expect("a named deleter");
        let (mut scope, got) = run(scope, delete, this, [Some(key), None]);
        state::report(&mut scope);
        if !matches!(got, Got::Threw) {
            rv.set_bool(true);
        }
        v8::Intercepted::kYes
    }

    fn enumerate<'s>(
        mut scope: Scope<'_, 's, '_>,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Array>,
    ) {
        let calls = calls(&mut scope, &args);
        let names = calls.names.expect("named properties' names");
        let (mut scope, got) = run(scope, names, args.holder(), [None, None]);
        let Got::Strings(names) = got else {
            return;
        };
        let mut keys: Vec<v8::Local<'s, v8::Value>> = Vec::with_capacity(names.len());
        for name in &names {
            match cached_string(&mut scope, name) {
                Some(string) => keys.push(string.into()),
                None => {
                    exception::throw_range_error(scope.scope(), "Invalid string length");
                    return;
                }
            }
        }
        rv.set(v8::Array::new_with_elements(scope.scope(), &keys));
    }
}

/// The indexed setter of the interceptor's interface (by its index in the
/// callbacks' data), given the index and the value: it takes every index.
pub(crate) struct IndexedSetter;

impl bindings::StateIndexedSetter<State> for IndexedSetter {
    fn set<'s>(
        mut scope: Scope<'_, 's, '_>,
        index: u32,
        value: v8::Local<'s, v8::Value>,
        args: v8::PropertyCallbackArguments<'s>,
    ) -> v8::Intercepted {
        let calls = calls(&mut scope, &args);
        let set = calls.indexed_set.expect("an indexed setter");
        let index = v8::Integer::new_from_unsigned(scope.scope(), index).into();
        let (mut scope, _) = run(scope, set, args.holder(), [Some(index), Some(value)]);
        state::report(&mut scope);
        v8::Intercepted::kYes
    }
}
