//! HTML's cross-origin objects
//! (https://html.spec.whatwg.org/#cross-origin-objects): a `Window` and a
//! `Location` reached from a realm of another origin (another security
//! token: `frames::set_security_token`). V8's access check refuses the
//! access (`may_access`: for a window V8 asks only when the tokens differ;
//! for a location, at each access) and the object's properties go through
//! the interceptors here instead (Blink's `V8Window` and `V8Location`
//! cross-origin interceptors):
//!
//! - HTML's CrossOriginProperties (`WINDOW`, `LOCATION`): a method's value
//!   and an attribute's accessors are functions of the accessing realm,
//!   the same one each time in it (a template's, which V8 keeps a function
//!   of per context, as Blink's), never the object's own (whose
//!   `Function` would run script in its realm). A getter runs in the
//!   object's realm: the wrapper it makes (a window's `Location`'s) is that
//!   realm's, which other origins don't reach;
//! - a window's child windows, by index, and by their iframes' names (not
//!   among its own keys; one of the attributes' names is the
//!   attribute's);
//! - `then` and three well-known symbols are undefined (so a promise can
//!   be resolved with a window);
//! - anything else, and a delete, a define or a prototype set
//!   (`StateAccessCheck::refused`), a `SecurityError` (HTML's
//!   [[SetPrototypeOf]] and [[PreventExtensions]] give false, a
//!   `TypeError`: V8 tells the embedder no more than that it refused, as
//!   in Chrome).
//!
//! The members of these properties are the same callbacks
//! (`CrossOriginMember`), their own receivers not checked (WebIDL's
//! `[CrossOrigin]`, Blink's): a realm's own `postMessage` is called on a
//! window of another origin as the window's is.

use std::marker::PhantomData;

use dom::{ObjectRef, Window};
use dom_api::{INTERFACES, MemberKind, TypedMember, TypedVisitor};

use crate::exception;
use crate::state::{self, Scope, State};
use crate::templates::{self, string};

/// A function other origins are given: a member's, by its interface's
/// name, its name and its kind.
struct Function {
    interface: &'static str,
    member: &'static str,
    kind: MemberKind,
}

const fn function(interface: &'static str, member: &'static str, kind: MemberKind) -> Function {
    Function {
        interface,
        member,
        kind,
    }
}

/// The functions, by number: what `State::cross_origin` keeps the
/// templates of. Each is a member the table marks `#[cross_origin]` (whose
/// own functions are `CrossOriginMember`s too; `Make` visits those alone).
const FUNCTIONS: [Function; 16] = [
    function("Window", "window", MemberKind::Getter),
    function("Window", "self", MemberKind::Getter),
    function("Window", "location", MemberKind::Getter),
    function("Window", "location", MemberKind::Setter),
    function("Window", "close", MemberKind::Method),
    function("Window", "closed", MemberKind::Getter),
    function("Window", "focus", MemberKind::Method),
    function("Window", "blur", MemberKind::Method),
    function("Window", "frames", MemberKind::Getter),
    function("Window", "length", MemberKind::Getter),
    function("Window", "top", MemberKind::Getter),
    function("Window", "opener", MemberKind::Getter),
    function("Window", "parent", MemberKind::Getter),
    function("Window", "postMessage", MemberKind::Method),
    function("Location", "href", MemberKind::Setter),
    function("Location", "replace", MemberKind::Method),
];

/// How many functions `State::cross_origin` keeps.
pub(crate) const FUNCTION_COUNT: usize = FUNCTIONS.len();

/// A property other origins reach (an entry of HTML's
/// CrossOriginProperties): a method's function, or an attribute's getter
/// and setter, by their numbers in `FUNCTIONS`.
struct Property {
    name: &'static str,
    method: Option<usize>,
    get: Option<usize>,
    set: Option<usize>,
}

const fn method(name: &'static str, function: usize) -> Property {
    Property {
        name,
        method: Some(function),
        get: None,
        set: None,
    }
}

const fn attribute(name: &'static str, get: Option<usize>, set: Option<usize>) -> Property {
    Property {
        name,
        method: None,
        get,
        set,
    }
}

/// A window's, in HTML's order.
const WINDOW: [Property; 13] = [
    attribute("window", Some(0), None),
    attribute("self", Some(1), None),
    attribute("location", Some(2), Some(3)),
    method("close", 4),
    attribute("closed", Some(5), None),
    method("focus", 6),
    method("blur", 7),
    attribute("frames", Some(8), None),
    attribute("length", Some(9), None),
    attribute("top", Some(10), None),
    attribute("opener", Some(11), None),
    attribute("parent", Some(12), None),
    method("postMessage", 13),
];

/// A location's: `href` only set.
const LOCATION: [Property; 2] = [attribute("href", None, Some(14)), method("replace", 15)];

/// The interceptors' data: which object they are a window's or a
/// location's.
pub(crate) const WINDOW_DATA: u32 = 0;
pub(crate) const LOCATION_DATA: u32 = 1;

/// The callbacks of a window's and a location's access check.
pub(crate) struct CrossOrigin;

/// Whether the interceptor is a window's (else a location's).
fn is_window(args: &v8::PropertyCallbackArguments<'_>) -> bool {
    v8::Local::<v8::Integer>::try_from(args.data())
        .ok()
        .is_some_and(|data| data.value() == i64::from(WINDOW_DATA))
}

fn properties(args: &v8::PropertyCallbackArguments<'_>) -> &'static [Property] {
    if is_window(args) { &WINDOW } else { &LOCATION }
}

/// What a name is of the object's cross-origin ones.
enum Found<'s> {
    Property(&'static Property),
    /// A child window by its iframe's name.
    Named(v8::Local<'s, v8::Object>),
    /// `then` or a well-known symbol: undefined (HTML's
    /// CrossOriginPropertyFallback).
    Fallback,
    None,
}

/// The well-known symbols that are undefined across origins.
fn fallback_symbols<'s>(scope: &mut v8::PinScope<'s, '_>) -> [v8::Local<'s, v8::Symbol>; 3] {
    [
        v8::Symbol::get_to_string_tag(scope),
        v8::Symbol::get_has_instance(scope),
        v8::Symbol::get_is_concat_spreadable(scope),
    ]
}

fn find<'s>(
    scope: &mut Scope<'_, 's, '_>,
    args: &v8::PropertyCallbackArguments<'s>,
    name: v8::Local<'_, v8::Name>,
) -> Found<'s> {
    if let Ok(symbol) = v8::Local::<v8::Symbol>::try_from(name) {
        return if fallback_symbols(scope.scope()).contains(&symbol) {
            Found::Fallback
        } else {
            Found::None
        };
    }
    let name = name.to_rust_string_lossy(scope.scope());
    if let Some(property) = properties(args)
        .iter()
        .find(|property| property.name == name)
    {
        return Found::Property(property);
    }
    if is_window(args)
        && let Some(child) = named_child_window(scope, args.holder(), &name)
    {
        return Found::Named(child);
    }
    if name == "then" {
        Found::Fallback
    } else {
        Found::None
    }
}

/// The receiver's child window whose iframe's name is `name`, if the
/// receiver is a window with one.
fn named_child_window<'s>(
    scope: &mut Scope<'_, 's, '_>,
    this: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<v8::Local<'s, v8::Object>> {
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        let window = no_gc.unwrap::<Window>(cx.gc, this)?;
        let (_, child) = dom_api::named_child_windows(cx, window)
            .into_iter()
            .find(|(child_name, _)| child_name == name)?;
        no_gc.wrapper(cx.object_wrapper_edge(ObjectRef::Window(child)))
    })
}

/// `FUNCTIONS[function]`'s template, made at the first ask.
fn template<'s>(
    scope: &mut Scope<'_, 's, '_>,
    function: usize,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let cached = {
        let (state, mut no_gc) = scope.state();
        state.cross_origin[function]
            .as_ref()
            .map(|template| no_gc.local(template))
    };
    if let Some(template) = cached {
        return template;
    }
    let mut make = Make {
        scope: scope.scope(),
        function,
        made: None,
    };
    dom_api::visit_typed(&mut make);
    let template = make
        .made
        .expect("a cross-origin function's member is the table's");
    let global = v8::Global::new(scope.scope(), template);
    scope.state_mut().0.cross_origin[function] = Some(global);
    template
}

/// `FUNCTIONS[function]` in the current realm.
fn function_in<'s>(
    scope: &mut Scope<'_, 's, '_>,
    function: usize,
) -> Option<v8::Local<'s, v8::Function>> {
    let template = template(scope, function);
    template.get_function(scope.scope())
}

/// A cross-origin function's template, from its member's typed path.
struct Make<'a, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    function: usize,
    made: Option<v8::Local<'s, v8::FunctionTemplate>>,
}

impl TypedVisitor for Make<'_, '_, '_> {
    fn cross_origin<M: TypedMember>(&mut self) {
        let function = &FUNCTIONS[self.function];
        if self.made.is_some()
            || M::KIND != function.kind
            || M::NAME != function.member
            || INTERFACES[M::INTERFACE].name != function.interface
        {
            return;
        }
        let length = match M::KIND {
            MemberKind::Setter => 1,
            MemberKind::Method => INTERFACES[M::INTERFACE].members[M::MEMBER].required(),
            _ => 0,
        };
        // Its receiver one of the interface's (a `TypeError` else, as the
        // typed path takes it to be), its access not checked (V8's
        // default).
        let interface = {
            let mut scope = state::scope(self.scope).expect("the isolate's state");
            templates::template(&mut scope, M::INTERFACE)
        };
        let signature = v8::Signature::new(self.scope, interface);
        let template = bindings::state_info_method_template::<State, CrossOriginMember<M>>(
            self.scope,
            signature,
            |builder| {
                builder
                    .length(i32::try_from(length).expect("a few parameters"))
                    .constructor_behavior(v8::ConstructorBehavior::Throw)
            },
        );
        let name = match M::KIND {
            MemberKind::Getter => format!("get {}", M::NAME),
            MemberKind::Setter => format!("set {}", M::NAME),
            _ => M::NAME.to_owned(),
        };
        template.set_class_name(string(self.scope, &name));
        self.made = Some(template);
    }
}

/// A cross-origin property's member's callback (its own function's and
/// the cross-origin one's): its member's, a getter's in its receiver's
/// realm if it isn't the current one.
pub(crate) struct CrossOriginMember<M>(PhantomData<M>);

/// Its functions have the interface's signature: the receiver V8 checked
/// goes on to the member's call, as any member's (`templates::InfoMember`).
/// A getter of another realm's object isn't run in that realm: what these
/// getters give (a window, a location, a boolean, a count) has its wrapper
/// in its own realm whichever is current, and none of them throws, so no
/// script can tell.
impl<M: TypedMember> bindings::StateInfoMethod<State> for CrossOriginMember<M> {
    fn call(info: bindings::StateInfo<'_, State>) {
        let mut front = crate::info_front::InfoFront::<crate::info_front::Method>::new(info);
        crate::templates::census_label::<M>();
        M::call(&mut front);
        if M::KIND != MemberKind::Getter {
            front.report();
        }
    }
}

/// A `SecurityError` for an access of `name` refused.
fn refuse(scope: &mut Scope<'_, '_, '_>, key: bindings::PropertyKey<'_>) -> v8::Intercepted {
    let name = match key {
        bindings::PropertyKey::Name(name) => name.to_rust_string_lossy(scope.scope()),
        bindings::PropertyKey::Index(index) => index.to_string(),
    };
    exception::throw_dom_exception(
        scope,
        "SecurityError",
        &format!("Blocked a frame from accessing '{name}' of a cross-origin object."),
    );
    v8::Intercepted::kYes
}

/// The window's `index`th child window's proxy, if the receiver is a
/// window with that many.
fn child_window<'s>(
    scope: &mut Scope<'_, 's, '_>,
    this: v8::Local<'s, v8::Object>,
    index: u32,
) -> Option<v8::Local<'s, v8::Object>> {
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        let window = no_gc.unwrap::<Window>(cx.gc, this)?;
        let child = *dom_api::child_windows(cx, window).get(usize::try_from(index).ok()?)?;
        no_gc.wrapper(cx.object_wrapper_edge(ObjectRef::Window(child)))
    })
}

/// How many child windows the receiver has, if it is a window.
fn child_window_count(scope: &mut Scope<'_, '_, '_>, this: v8::Local<'_, v8::Object>) -> u32 {
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        no_gc.unwrap::<Window>(cx.gc, this).map_or(0, |window| {
            u32::try_from(dom_api::child_windows(cx, window).len()).unwrap_or(u32::MAX)
        })
    })
}

/// A property descriptor object's field.
fn put<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    descriptor: v8::Local<'s, v8::Object>,
    key: &str,
    value: v8::Local<'s, v8::Value>,
) {
    let key = string(scope, key);
    descriptor.create_data_property(scope, key.into(), value);
}

/// A data property's descriptor object: not writable, configurable.
fn data_descriptor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    enumerable: bool,
) -> v8::Local<'s, v8::Object> {
    let descriptor = v8::Object::new(scope);
    put(scope, descriptor, "value", value);
    let no = v8::Boolean::new(scope, false).into();
    let yes = v8::Boolean::new(scope, true).into();
    let enumerable = v8::Boolean::new(scope, enumerable).into();
    put(scope, descriptor, "writable", no);
    put(scope, descriptor, "enumerable", enumerable);
    put(scope, descriptor, "configurable", yes);
    descriptor
}

impl bindings::StateAccessCheck<State> for CrossOrigin {
    fn may_access<'s>(
        mut scope: Scope<'_, 's, '_>,
        accessing: v8::Local<'s, v8::Context>,
        accessed: v8::Local<'s, v8::Object>,
    ) -> bool {
        let scope = scope.scope();
        let Some(context) = accessed.get_creation_context(scope) else {
            return false;
        };
        let token = context.get_security_token(scope);
        token.strict_equals(accessing.get_security_token(scope))
    }

    fn get<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted {
        let name = match key {
            bindings::PropertyKey::Index(index) => {
                if is_window(&args)
                    && let Some(child) = child_window(&mut scope, args.holder(), index)
                {
                    rv.set(child.into());
                    return v8::Intercepted::kYes;
                }
                return refuse(&mut scope, key);
            }
            bindings::PropertyKey::Name(name) => name,
        };
        match find(&mut scope, &args, name) {
            Found::Property(Property {
                method: Some(method),
                ..
            }) => {
                if let Some(function) = function_in(&mut scope, *method) {
                    rv.set(function.into());
                }
            }
            Found::Named(child) => rv.set(child.into()),
            Found::Property(Property { get: Some(get), .. }) => {
                let Some(getter) = function_in(&mut scope, *get) else {
                    return v8::Intercepted::kYes;
                };
                if let Some(value) = getter.call(scope.scope(), args.holder().into(), &[]) {
                    rv.set(value);
                }
            }
            Found::Fallback => rv.set_undefined(),
            Found::Property(_) | Found::None => return refuse(&mut scope, key),
        }
        v8::Intercepted::kYes
    }

    fn query<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Integer>,
    ) -> v8::Intercepted {
        let attributes = match key {
            bindings::PropertyKey::Index(index)
                if is_window(&args) && index < child_window_count(&mut scope, args.holder()) =>
            {
                v8::PropertyAttribute::READ_ONLY
            }
            bindings::PropertyKey::Index(_) => return refuse(&mut scope, key),
            bindings::PropertyKey::Name(name) => match find(&mut scope, &args, name) {
                Found::Property(Property { method: None, .. }) => v8::PropertyAttribute::DONT_ENUM,
                Found::Property(_) | Found::Named(_) | Found::Fallback => {
                    v8::PropertyAttribute::DONT_ENUM | v8::PropertyAttribute::READ_ONLY
                }
                Found::None => return refuse(&mut scope, key),
            },
        };
        rv.set_int32(attributes.as_u32().cast_signed());
        v8::Intercepted::kYes
    }

    fn set<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        value: v8::Local<'s, v8::Value>,
        args: v8::PropertyCallbackArguments<'s>,
    ) -> v8::Intercepted {
        let bindings::PropertyKey::Name(name) = key else {
            return refuse(&mut scope, key);
        };
        let Found::Property(Property { set: Some(set), .. }) = find(&mut scope, &args, name) else {
            return refuse(&mut scope, key);
        };
        if let Some(setter) = function_in(&mut scope, *set) {
            setter.call(scope.scope(), args.holder().into(), &[value]);
        }
        v8::Intercepted::kYes
    }

    fn descriptor<'s>(
        mut scope: Scope<'_, 's, '_>,
        key: bindings::PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted {
        let name = match key {
            bindings::PropertyKey::Index(index) => {
                if is_window(&args)
                    && let Some(child) = child_window(&mut scope, args.holder(), index)
                {
                    let descriptor = data_descriptor(scope.scope(), child.into(), true);
                    rv.set(descriptor.into());
                    return v8::Intercepted::kYes;
                }
                return refuse(&mut scope, key);
            }
            bindings::PropertyKey::Name(name) => name,
        };
        let descriptor = match find(&mut scope, &args, name) {
            Found::Property(Property {
                method: Some(method),
                ..
            }) => {
                let Some(function) = function_in(&mut scope, *method) else {
                    return v8::Intercepted::kYes;
                };
                data_descriptor(scope.scope(), function.into(), false)
            }
            Found::Property(Property { get, set, .. }) => {
                let accessor = |scope: &mut Scope<'_, 's, '_>, function: Option<usize>| {
                    function
                        .and_then(|function| function_in(scope, function))
                        .map_or_else(|| v8::undefined(scope.scope()).into(), Into::into)
                };
                let (get, set) = (accessor(&mut scope, *get), accessor(&mut scope, *set));
                let scope = scope.scope();
                let descriptor = v8::Object::new(scope);
                put(scope, descriptor, "get", get);
                put(scope, descriptor, "set", set);
                let no = v8::Boolean::new(scope, false).into();
                let yes = v8::Boolean::new(scope, true).into();
                put(scope, descriptor, "enumerable", no);
                put(scope, descriptor, "configurable", yes);
                descriptor
            }
            Found::Named(child) => data_descriptor(scope.scope(), child.into(), false),
            Found::Fallback => {
                let undefined = v8::undefined(scope.scope()).into();
                data_descriptor(scope.scope(), undefined, false)
            }
            Found::None => return refuse(&mut scope, key),
        };
        rv.set(descriptor.into());
        v8::Intercepted::kYes
    }

    fn enumerate<'s>(
        mut scope: Scope<'_, 's, '_>,
        indices: bool,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Array>,
    ) {
        let keys: Vec<v8::Local<'s, v8::Value>> = if indices {
            let count = if is_window(&args) {
                child_window_count(&mut scope, args.holder())
            } else {
                0
            };
            let scope = scope.scope();
            (0..count)
                .map(|index| v8::Integer::new_from_unsigned(scope, index).into())
                .collect()
        } else {
            // A child's name isn't among them (HTML's [[HideFromKeys]]).
            let scope = scope.scope();
            let mut keys: Vec<v8::Local<'s, v8::Value>> = properties(&args)
                .iter()
                .map(|property| string(scope, property.name).into())
                .collect();
            keys.push(string(scope, "then").into());
            keys.extend(fallback_symbols(scope).map(v8::Local::<v8::Value>::from));
            keys
        };
        let keys = v8::Array::new_with_elements(scope.scope(), &keys);
        rv.set(keys);
    }

    fn refused<'s>(mut scope: Scope<'_, 's, '_>, _: v8::Local<'s, v8::Object>) {
        exception::throw_dom_exception(
            &mut scope,
            "SecurityError",
            "Blocked a frame from accessing a cross-origin object.",
        );
    }
}
