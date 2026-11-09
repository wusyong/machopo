//! The declaration table's interfaces as V8 templates (Blink's generated
//! `V8Node::InstallInterfaceTemplate`): each made at its first use, once
//! an isolate (`State::templates`, Blink's `V8PerIsolateData`'s template
//! map), inheriting its parent's; its constants on the interface object
//! and the prototype, its attributes accessor properties and its methods
//! functions on the prototype, each a callback of its own made from its
//! typed path (`member`), whose receiver V8 checks (a `Signature` of the
//! interface's template). An interface object is a constructor if the
//! table gives it one (`new Event(type)`; else `new Node()` throws), and is
//! a global's lazy property (`install`). A method declared more than once
//! (and the constructors, when there are several: WebIDL's overloads) is
//! one function that picks the form (`Overloaded`).

use std::marker::PhantomData;
use std::sync::OnceLock;

use dom_api::declare::{Observable, TypeDesc};
use dom_api::{CALLBACK_INTERFACES, INTERFACES, MemberKind, TypedMember, TypedVisitor};

use crate::cross_origin::{self, CrossOriginMember};
use crate::exception;
use crate::front::{Checked, IndexArgs, V8Front};
use crate::named::{IndexedSetter, NamedCalls, NamedProperties};
use crate::state::{self, Scope, State};
use crate::window::CachedAccessor;

/// `INTERFACES[interface]`'s template (`DOMException`'s at
/// `INTERFACES.len()`, `QuotaExceededError`'s after it), made at the
/// first ask.
pub(crate) fn template<'s>(
    scope: &mut Scope<'_, 's, '_>,
    interface: usize,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let cached = {
        let (state, mut no_gc) = scope.state();
        state.templates[interface]
            .as_ref()
            .map(|template| no_gc.local(template))
    };
    if let Some(template) = cached {
        return template;
    }
    let template = if interface == INTERFACES.len() {
        exception::make_template(scope.scope())
    } else if interface == exception::quota_slot() {
        exception::make_quota_template(scope)
    } else {
        let (template, named) = make_template(scope, interface);
        scope.state_mut().0.named[interface] = named;
        template
    };
    let global = v8::Global::new(scope.scope(), template);
    scope.state_mut().0.templates[interface] = Some(global);
    template
}

/// `INTERFACES[interface]`'s template, and its named properties' special
/// operations, which its interceptors call.
fn make_template<'s>(
    scope: &mut Scope<'_, 's, '_>,
    interface: usize,
) -> (v8::Local<'s, v8::FunctionTemplate>, NamedCalls) {
    let desc = INTERFACES[interface];
    let mut constructor = Constructor {
        scope: scope.scope(),
        interface,
        template: None,
    };
    dom_api::visit_typed(&mut constructor);
    let template = constructor.template.unwrap_or_else(|| {
        v8::FunctionTemplate::builder(illegal_constructor)
            .length(0)
            .build(scope.scope())
    });
    template.set_class_name(string(scope.scope(), desc.name));
    // WebIDL's interface object's `prototype`: not writable (Blink's
    // `InstallInterfaceTemplate`).
    template.read_only_prototype();
    if let Some(parent) = desc.parent {
        let parent = template_of(scope, parent);
        template.inherit(parent);
    }
    // The ancestors with unforgeable members, and their templates: V8
    // doesn't apply a parent's instance template to a child's instances,
    // so each object gets them from its own (as Blink's generated bindings
    // install them).
    let mut unforgeable_ancestors = Vec::new();
    let mut ancestor = desc.parent;
    while let Some(name) = ancestor {
        let index = INTERFACES
            .iter()
            .position(|desc| desc.name == name)
            .expect("a parent of the table");
        if INTERFACES[index]
            .members
            .iter()
            .any(|member| member.unforgeable)
        {
            unforgeable_ancestors.push((index, self::template(scope, index)));
        }
        ancestor = INTERFACES[index].parent;
    }
    let scope = scope.scope();
    let prototype = template.prototype_template(scope);
    to_string_tag(scope, prototype, desc.name);
    // WebIDL's constants: on the interface object, read-only, not
    // configurable; on the prototype too, in the members' order (below).
    for member in desc.members {
        if let MemberKind::Constant(value) = member.kind {
            let name = string(scope, member.name);
            let value = v8::Integer::new_from_unsigned(scope, value);
            template.set_with_attr(name.into(), value.into(), constant());
        }
    }
    let signature = v8::Signature::new(scope, template);
    let instance = template.instance_template(scope);
    let mut members = Members {
        scope,
        interface,
        template,
        signature,
        prototype,
        instance,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        pairs: None,
        named: NamedCalls::default(),
        only_unforgeable: false,
    };
    dom_api::visit_typed(&mut members);
    members.install_ordered();
    let Members {
        scope,
        prototype,
        instance,
        pairs,
        named,
        ..
    } = members;
    for (ancestor, ancestor_template) in unforgeable_ancestors {
        let signature = v8::Signature::new(scope, ancestor_template);
        let mut members = Members {
            scope: &mut *scope,
            interface: ancestor,
            template: ancestor_template,
            signature,
            prototype,
            instance,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            pairs: None,
            named: NamedCalls::default(),
            only_unforgeable: true,
        };
        dom_api::visit_typed(&mut members);
        members.install_ordered();
    }
    // An instance template doesn't inherit its parent's interceptors: an
    // ancestor's indexed getter is installed again (Blink's generated
    // bindings install each interface's, its own or inherited).
    if let Some(ancestor) = indexed_ancestor(interface) {
        dom_api::visit_typed(&mut InheritedIndexed {
            scope,
            interface,
            ancestor,
            instance,
        });
    }
    // HTML's [[IsHTMLDDA]] (`document.all`): V8's undetectable objects, as
    // Blink makes `HTMLAllCollection`'s; they must be callable, its `item`
    // (the visitor set the handler).
    if desc.is_html_dda {
        instance.mark_as_undetectable();
    }
    // HTML's cross-origin objects: a window and a location reached from
    // another origin go through their own interceptors.
    let cross_origin_data = match desc.name {
        "Window" => Some(cross_origin::WINDOW_DATA),
        "Location" => Some(cross_origin::LOCATION_DATA),
        _ => None,
    };
    if let Some(data) = cross_origin_data {
        let data = v8::Integer::new_from_unsigned(scope, data);
        bindings::set_state_access_check::<State, cross_origin::CrossOrigin>(
            scope,
            instance,
            data.into(),
        );
    }
    // HTML's `Location` objects: an own `valueOf` (`Object.prototype`'s)
    // and `@@toPrimitive` (`undefined`), unforgeable, so script can't
    // change how one converts; and their prototype can't change (their
    // `[[SetPrototypeOf]]`, as Blink's `SetImmutableProto`).
    if desc.name == "Location" {
        instance.set_immutable_proto();
        let value_of = string(scope, "valueOf");
        instance.set_intrinsic_data_property(
            value_of.into(),
            v8::Intrinsic::ObjProtoValueOf,
            v8::PropertyAttribute::READ_ONLY
                | v8::PropertyAttribute::DONT_DELETE
                | v8::PropertyAttribute::DONT_ENUM,
        );
        let to_primitive = v8::Symbol::get_to_primitive(scope);
        let undefined = v8::undefined(scope);
        instance.set_with_attr(
            to_primitive.into(),
            undefined.into(),
            v8::PropertyAttribute::READ_ONLY
                | v8::PropertyAttribute::DONT_ENUM
                | v8::PropertyAttribute::DONT_DELETE,
        );
    }
    if named.get.is_some() {
        let data =
            v8::Integer::new_from_unsigned(scope, u32::try_from(interface).expect("a small table"));
        bindings::set_state_named_properties::<State, NamedProperties>(
            scope,
            instance,
            named.options(desc.override_builtins, has_indexed(interface)),
            data.into(),
        );
    }
    // The iterable's methods after the members (WebIDL's order): a value
    // iterator's `Array.prototype`'s functions, on an interface with an
    // indexed getter and `length`; a pair iterator's own.
    if desc.iterable {
        for (name, intrinsic) in [
            ("entries", v8::Intrinsic::ArrayProtoEntries),
            ("keys", v8::Intrinsic::ArrayProtoKeys),
            ("values", v8::Intrinsic::ArrayProtoValues),
            ("forEach", v8::Intrinsic::ArrayProtoForEach),
        ] {
            let name = string(scope, name);
            prototype.set_intrinsic_data_property(
                name.into(),
                intrinsic,
                v8::PropertyAttribute::NONE,
            );
        }
    }
    if let Some(install) = pairs {
        install(scope, prototype, signature);
    }
    (template, named)
}

/// An accessor's function named `prefix name` (WebIDL's for an attribute's
/// getter and setter).
fn name_accessor(
    scope: &mut v8::PinScope<'_, '_>,
    function: v8::Local<'_, v8::FunctionTemplate>,
    prefix: &str,
    name: &str,
) {
    let name = string(scope, &format!("{prefix} {name}"));
    function.set_class_name(name);
}

/// The template of the interface named `name`.
fn template_of<'s>(
    scope: &mut Scope<'_, 's, '_>,
    name: &str,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let index = INTERFACES
        .iter()
        .position(|interface| interface.name == name)
        .expect("an interface of the table");
    template(scope, index)
}

/// An interface's template, its constructor's callback if it has one
/// (WebIDL's constructor; else `new` throws), from its typed path.
struct Constructor<'a, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    interface: usize,
    template: Option<v8::Local<'s, v8::FunctionTemplate>>,
}

impl TypedVisitor for Constructor<'_, '_, '_> {
    fn constructor<M: TypedMember>(&mut self) {
        if M::INTERFACE != self.interface {
            return;
        }
        // Several: the one function that picks among them.
        if let Some(set) = overload_set(M::INTERFACE, M::MEMBER) {
            if self.template.is_none() {
                self.template = Some(overloaded_template(self.scope, set, None));
            }
            return;
        }
        let length = INTERFACES[M::INTERFACE].members[M::MEMBER].required();
        self.template = Some(bindings::state_function_template::<State, Member<M>>(
            self.scope,
            |builder| builder.length(i32::try_from(length).expect("a few parameters")),
        ));
    }
}

/// One interface's members' functions, from their typed paths.
struct Members<'a, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    interface: usize,
    /// Where a static operation goes: the interface object.
    template: v8::Local<'s, v8::FunctionTemplate>,
    signature: v8::Local<'s, v8::Signature>,
    prototype: v8::Local<'s, v8::ObjectTemplate>,
    /// Where an unforgeable member goes (WebIDL's `[LegacyUnforgeable]`):
    /// each object, not the prototype.
    instance: v8::Local<'s, v8::ObjectTemplate>,
    /// Each regular method's function (an overload set's, at its first
    /// form), whether unforgeable; by its index in the interface's members.
    methods: Vec<(
        usize,
        &'static str,
        v8::Local<'s, v8::FunctionTemplate>,
        bool,
    )>,
    /// Each attribute's getter, whether unforgeable; by its index.
    getters: Vec<(
        usize,
        &'static str,
        v8::Local<'s, v8::FunctionTemplate>,
        bool,
    )>,
    /// A pair iterator's methods, installed after the members.
    pairs: Option<InstallPairs>,
    /// Whether only the unforgeable members are wanted: an ancestor's,
    /// for each object of a descendant interface.
    only_unforgeable: bool,
    setters: Vec<(&'static str, v8::Local<'s, v8::FunctionTemplate>)>,
    named: NamedCalls,
}

impl<'s> Members<'_, 's, '_> {
    /// Puts what was visited where it goes: the constants (on the
    /// prototype; not an ancestor's, which are its prototype's), methods
    /// and attributes.
    fn install_ordered(&mut self) {
        // The constants, the methods and the attributes in the table's order
        // (the order of the object's keys; WebIDL's and Blink's), each on the prototype, or on each object if unforgeable or of
        // a `[Global]` interface (the global's own).
        let desc = INTERFACES[self.interface];
        let mut ordered: Vec<(usize, Ordered<'s>)> = desc
            .members
            .iter()
            .enumerate()
            .filter(|_| !self.only_unforgeable)
            .filter_map(|(index, member)| match member.kind {
                MemberKind::Constant(value) => Some((index, Ordered::Constant(member.name, value))),
                _ => None,
            })
            .collect();
        ordered.extend(std::mem::take(&mut self.methods).into_iter().map(
            |(index, name, function, is_unforgeable)| {
                (index, Ordered::Method(name, function, is_unforgeable))
            },
        ));
        ordered.extend(std::mem::take(&mut self.getters).into_iter().map(
            |(index, name, getter, is_unforgeable)| {
                (index, Ordered::Attribute(name, getter, is_unforgeable))
            },
        ));
        ordered.sort_unstable_by_key(|(index, _)| *index);
        let global = desc.global;
        let signature = self.signature;
        let (scope, prototype, instance, setters) = (
            &mut *self.scope,
            self.prototype,
            self.instance,
            &mut self.setters,
        );
        for (index, member) in ordered {
            match member {
                Ordered::Constant(name, value) => {
                    let name = string(scope, name);
                    let value = v8::Integer::new_from_unsigned(scope, value);
                    prototype.set_with_attr(name.into(), value.into(), constant());
                }
                Ordered::Method(name, function, is_unforgeable) => {
                    let name = string(scope, name);
                    if is_unforgeable {
                        instance.set_with_attr(
                            name.into(),
                            function.into(),
                            unforgeable() | v8::PropertyAttribute::READ_ONLY,
                        );
                    } else if global {
                        instance.set(name.into(), function.into());
                    } else {
                        prototype.set(name.into(), function.into());
                    }
                }
                // An attribute: its getter, and its setter if it isn't
                // read-only. Their functions' names WebIDL's (`get x`, `set x`).
                Ordered::Attribute(name, getter, is_unforgeable) => {
                    let setter = setters
                        .iter()
                        .position(|(setter, _)| *setter == name)
                        .map(|index| setters.swap_remove(index).1)
                        .or_else(|| {
                            desc.members[index]
                                .replaceable
                                .then(|| replaceable_setter(scope, signature, name))
                        });
                    name_accessor(scope, getter, "get", name);
                    if let Some(setter) = setter {
                        name_accessor(scope, setter, "set", name);
                    }
                    let name = string(scope, name);
                    let (on, attributes) = if is_unforgeable {
                        (instance, unforgeable())
                    } else if global {
                        (instance, v8::PropertyAttribute::NONE)
                    } else {
                        (prototype, v8::PropertyAttribute::NONE)
                    };
                    on.set_accessor_property(name.into(), Some(getter), setter, attributes);
                }
            }
        }
        debug_assert!(setters.is_empty(), "each setter has its getter");
    }
}

/// WebIDL's `[Replaceable]` attribute's setter: the value a data property
/// of `this` (a window, by `signature`) of the attribute's name, which
/// shadows the attribute from then on.
fn replaceable_setter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    signature: v8::Local<'s, v8::Signature>,
    name: &'static str,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let data = string(scope, name);
    let setter = v8::FunctionTemplate::builder(replace)
        .data(data.into())
        .signature(signature)
        .length(1)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope);
    checks_receiver(setter);
    setter
}

fn replace(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments<'_>,
    _: v8::ReturnValue<'_>,
) {
    let name = v8::Local::<v8::String>::try_from(args.data()).expect("the attribute's name");
    // `None`: an exception (a proxy's trap threw), which propagates.
    if args
        .this()
        .create_data_property(scope, name.into(), args.get(0))
        == Some(false)
    {
        let message = format!("Cannot replace '{}'", name.to_rust_string_lossy(scope));
        crate::throw_type_error(scope, &message);
    }
}

/// `pairs::install` of a pair iterator's member.
type InstallPairs = for<'s, 'i> fn(
    &mut v8::PinScope<'s, 'i>,
    v8::Local<'s, v8::ObjectTemplate>,
    v8::Local<'s, v8::Signature>,
);

/// A constant, a method or an attribute of the prototype (or each
/// object), put there in the table's order.
enum Ordered<'s> {
    Constant(&'static str, u32),
    Method(&'static str, v8::Local<'s, v8::FunctionTemplate>, bool),
    Attribute(&'static str, v8::Local<'s, v8::FunctionTemplate>, bool),
}

/// An unforgeable member's attributes: not configurable (nor writable, for
/// a method's data property).
fn unforgeable() -> v8::PropertyAttribute {
    v8::PropertyAttribute::DONT_DELETE
}

impl TypedVisitor for Members<'_, '_, '_> {
    fn getter<M: TypedMember>(&mut self) {
        self.function::<M, Member<M>>(true);
    }

    fn setter<M: TypedMember>(&mut self) {
        self.function::<M, Member<M>>(true);
    }

    fn method<M: TypedMember>(&mut self) {
        self.function::<M, Member<M>>(true);
    }

    fn static_member<M: TypedMember>(&mut self) {
        self.function::<M, Member<M>>(true);
    }

    fn observable<M: TypedMember>(&mut self) {
        self.function::<M, Member<M>>(true);
    }

    // A cross-origin property's (WebIDL's `[CrossOrigin]`): called on a
    // window or a location of any origin.
    fn cross_origin<M: TypedMember>(&mut self) {
        self.function::<M, CrossOriginMember<M>>(false);
    }

    fn indexed<M: TypedMember>(&mut self) {
        if !self.takes::<M>() {
            return;
        }
        install_indexed::<M>(self.scope, self.interface, self.instance, self.prototype);
        // Anonymous (`#[indexed(getter)]`): the interceptor's alone.
        if !INTERFACES[M::INTERFACE].members[M::MEMBER].name.is_empty() {
            self.function::<M, Member<M>>(true);
        }
    }

    fn indexed_setter<M: TypedMember>(&mut self) {
        if self.takes::<M>() {
            self.named.add_indexed_setter::<M>();
        }
    }

    // Anonymous: the pair iterator's methods call it.
    fn pairs<M: TypedMember>(&mut self) {
        if self.takes::<M>() {
            self.pairs = Some(crate::pairs::install::<M>);
        }
    }

    // Anonymous: the interceptors' alone.
    fn named<M: TypedMember>(&mut self) {
        if self.takes::<M>()
            && let Some(named) = INTERFACES[M::INTERFACE].members[M::MEMBER].named
        {
            self.named.add::<M>(named);
        }
    }
}

impl Members<'_, '_, '_> {
    /// Whether `M` is one of the members wanted: this interface's (only
    /// its unforgeable ones for a descendant's objects).
    fn takes<M: TypedMember>(&self) -> bool {
        M::INTERFACE == self.interface
            && (!self.only_unforgeable || INTERFACES[M::INTERFACE].members[M::MEMBER].unforgeable)
    }

    /// An attribute's accessor or an operation: its function, its
    /// callback `C`, its receiver checked (as the typed path takes it to
    /// be) if `checked` and it has one.
    fn function<
        M: TypedMember,
        C: bindings::StateCallback<State> + bindings::StateMethod<State>,
    >(
        &mut self,
        checked: bool,
    ) {
        if !self.takes::<M>() {
            return;
        }
        let desc = &INTERFACES[M::INTERFACE].members[M::MEMBER];
        // An overload's forms: one function for them all, as its first is
        // reached.
        if M::KIND == MemberKind::Method
            && let Some(set) = overload_set(M::INTERFACE, M::MEMBER)
        {
            if OVERLOADS.get().expect("made by `overload_set`")[set].members[0] == M::MEMBER {
                let name = string(self.scope, M::NAME);
                // A static operation's (a namespace's): on the interface
                // object, its `this` anything.
                if desc.is_static {
                    let function = overloaded_template(self.scope, set, None);
                    self.template.set(name.into(), function.into());
                } else {
                    let function = overloaded_template(self.scope, set, Some(self.signature));
                    let function = if matches!(desc.returns, TypeDesc::Promise(_)) {
                        rejecting(self.scope, function, desc.required())
                    } else {
                        function
                    };
                    self.methods
                        .push((M::MEMBER, M::NAME, function, desc.unforgeable));
                }
            }
            return;
        }
        let length = match M::KIND {
            MemberKind::Setter => 1,
            MemberKind::Method => desc.required(),
            _ => 0,
        };
        // A static operation's `this` is anything (the interface object,
        // mostly): no signature.
        let signature = self.signature;
        let member_signature = (!desc.is_static).then_some(signature);
        let cached = (M::KIND == MemberKind::Getter)
            .then(|| CachedAccessor::of(INTERFACES[M::INTERFACE].name, M::NAME))
            .flatten();
        // With a signature, its callback is given the receiver V8 checked
        // against it (`bindings::StateMethod`).
        let function = match (cached, member_signature) {
            (Some(cached), Some(signature)) => {
                let key = cached.key(self.scope);
                bindings::state_cached_getter_template::<State, C>(self.scope, key, signature)
            }
            (Some(_), None) => unreachable!("a cached accessor is an instance's"),
            (None, Some(signature)) => {
                bindings::state_method_template::<State, C>(self.scope, signature, |builder| {
                    member_builder(builder, None, length)
                })
            }
            (None, None) => bindings::state_function_template::<State, C>(self.scope, |builder| {
                member_builder(builder, None, length)
            }),
        };
        if checked && !desc.is_static {
            checks_receiver(function);
        }
        let function = if !desc.is_static && matches!(desc.returns, TypeDesc::Promise(_)) {
            rejecting(self.scope, function, length)
        } else {
            function
        };
        // An `[IsHTMLDDA]` interface's `item` is its `legacycaller`:
        // calling an object calls it, `this` the object. Its interface
        // known at compile time: the handler made for that member alone.
        if const { INTERFACES[M::INTERFACE].is_html_dda }
            && M::KIND == MemberKind::Method
            && M::NAME == "item"
        {
            bindings::set_state_call_as_function_handler::<State, Member<M>>(
                self.scope,
                self.instance,
            );
        }
        // An observable array attribute: its getter's typed path gives the
        // list, its `#[observable(..)]` methods change it, all anonymous;
        // its accessors are the array's (`observable_array`).
        if let Some(observable) = desc.observable {
            let (attribute, set) = match observable {
                Observable::Set(attribute) => (attribute, true),
                Observable::DeleteLast(attribute) => (attribute, false),
            };
            let global = v8::Global::new(self.scope, function);
            let mut scope = state::scope(self.scope).expect("the isolate's state");
            let (state, _) = scope.state_mut();
            let templates = state
                .observable
                .iter_mut()
                .find(|templates| {
                    templates.interface == M::INTERFACE && templates.attribute == attribute
                })
                .expect("an observable array's operations after its attribute");
            *(if set {
                &mut templates.set
            } else {
                &mut templates.delete
            }) = Some(global);
            return;
        }
        if M::KIND == MemberKind::Getter
            && let TypeDesc::ObservableArray(item) = desc.returns
        {
            let item = INTERFACES
                .iter()
                .position(|interface| interface.name == item)
                .expect("an interface of the table");
            let list = v8::Global::new(self.scope, function);
            let code = {
                let mut scope = state::scope(self.scope).expect("the isolate's state");
                let (state, _) = scope.state_mut();
                state
                    .observable
                    .push(crate::observable_array::ObservableTemplates {
                        interface: M::INTERFACE,
                        attribute: M::NAME,
                        item,
                        list,
                        set: None,
                        delete: None,
                    });
                state.observable.len().saturating_sub(1)
            };
            let data = v8::Integer::new_from_unsigned(
                self.scope,
                u32::try_from(code).expect("a few attributes"),
            );
            let getter = v8::FunctionTemplate::builder(crate::observable_array::getter)
                .data(data.into())
                .signature(signature)
                .length(0)
                .constructor_behavior(v8::ConstructorBehavior::Throw)
                .build(self.scope);
            let setter = v8::FunctionTemplate::builder(crate::observable_array::setter)
                .data(data.into())
                .signature(signature)
                .length(1)
                .constructor_behavior(v8::ConstructorBehavior::Throw)
                .build(self.scope);
            checks_receiver(getter);
            checks_receiver(setter);
            self.getters
                .push((M::MEMBER, M::NAME, getter, desc.unforgeable));
            self.setters.push((M::NAME, setter));
            return;
        }
        match M::KIND {
            // A static attribute's (WebIDL's): an accessor of the interface
            // object, read-only.
            MemberKind::Getter if desc.is_static => {
                name_accessor(self.scope, function, "get", M::NAME);
                let name = string(self.scope, M::NAME);
                self.template.set_accessor_property(
                    name.into(),
                    Some(function),
                    None,
                    v8::PropertyAttribute::NONE,
                );
            }
            MemberKind::Getter => {
                self.getters
                    .push((M::MEMBER, M::NAME, function, desc.unforgeable));
            }
            MemberKind::Setter => self.setters.push((M::NAME, function)),
            MemberKind::Method => {
                if desc.is_static {
                    let name = string(self.scope, M::NAME);
                    self.template.set(name.into(), function.into());
                } else {
                    self.methods
                        .push((M::MEMBER, M::NAME, function, desc.unforgeable));
                }
            }
            MemberKind::Constant(_)
            | MemberKind::Constructor
            | MemberKind::IndexedSetter
            | MemberKind::LegacyFactory => {
                unreachable!(
                    "a constant has no typed path, the rest are visited by kinds of their own"
                )
            }
        }
    }
}

/// What a member returning a promise is installed as, in place of
/// `inner`, its function (whose `Signature` checks `this`): a function of
/// no `Signature` that calls it, what it throws (a wrong `this`, an
/// argument that doesn't convert, the DOM's error) the promise it gives
/// rejected with (WebIDL's operations and attributes of promise types;
/// Blink's `ExceptionToRejectPromiseScope`). Only these members pay for
/// the call.
fn rejecting<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    inner: v8::Local<'s, v8::FunctionTemplate>,
    length: usize,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let global = v8::Global::new(scope, inner);
    let index = {
        let mut scope = state::scope(scope).expect("the isolate's state");
        let (state, _) = scope.state_mut();
        state.rejecting.push(global);
        state.rejecting.len().saturating_sub(1)
    };
    let data = v8::Integer::new_from_unsigned(
        scope,
        u32::try_from(index).expect("a few members return promises"),
    );
    v8::FunctionTemplate::builder(call_rejecting)
        .data(data.into())
        .length(i32::try_from(length).expect("a few parameters"))
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
}

/// `rejecting`'s function: its member's called with `this` and the
/// arguments in a `TryCatch`; its result, or a promise rejected with what
/// it threw (script terminating: nothing).
fn call_rejecting(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments<'_>,
    mut rv: v8::ReturnValue<'_>,
) {
    let Some(index) = args
        .data()
        .uint32_value(scope)
        .and_then(|index| usize::try_from(index).ok())
    else {
        return;
    };
    let inner = {
        let mut scope = state::scope(scope).expect("the isolate's state");
        let (state, mut no_gc) = scope.state();
        no_gc.local(&state.rejecting[index])
    };
    let Some(function) = inner.get_function(scope) else {
        return;
    };
    let values: Vec<v8::Local<'_, v8::Value>> = (0..args.length()).map(|at| args.get(at)).collect();
    v8::tc_scope!(let tc, scope);
    if let Some(result) = function.call(tc, args.this().into(), &values) {
        rv.set(result);
        return;
    }
    if !tc.can_continue() {
        tc.rethrow();
        return;
    }
    let Some(exception) = tc.exception() else {
        return;
    };
    tc.reset();
    let Some(resolver) = v8::PromiseResolver::new(tc) else {
        return;
    };
    resolver.reject(tc, exception);
    rv.set(resolver.get_promise(tc).into());
}

/// A member's function: its receiver checked by `signature` (none for a
/// static operation), `length` its `length`, not a constructor (WebIDL's
/// operations and accessors aren't).
fn member_builder<'s>(
    builder: v8::FunctionBuilder<'s, v8::FunctionTemplate>,
    signature: Option<v8::Local<'s, v8::Signature>>,
    length: usize,
) -> v8::FunctionBuilder<'s, v8::FunctionTemplate> {
    let builder = match signature {
        Some(signature) => builder.signature(signature),
        None => builder,
    };
    builder
        .length(i32::try_from(length).expect("a few parameters"))
        .constructor_behavior(v8::ConstructorBehavior::Throw)
}

/// A member's callback: its typed path, through V8's front end, on the
/// isolate's DOM (its template made where it is, the state borrowed with no
/// check); V8 told of the DOM's growth after a call that may allocate.
pub(crate) struct Member<M>(PhantomData<M>);

/// A call with no word of its receiver (a constructor's, a static
/// member's, a legacy caller's, a form of such an overload set's): checked
/// whole as it is unwrapped.
impl<M: TypedMember> bindings::StateCallback<State> for Member<M> {
    fn call<'s>(
        scope: Scope<'_, 's, '_>,
        args: v8::FunctionCallbackArguments<'s>,
        rv: v8::ReturnValue<'s>,
    ) {
        call_member::<M, _>(scope, &args, rv);
    }
}

/// A call whose receiver V8 checked against the function's signature (a
/// getter's, a setter's, a method's that isn't static).
impl<M: TypedMember> bindings::StateMethod<State> for Member<M> {
    fn call<'s>(
        scope: Scope<'_, 's, '_>,
        args: v8::FunctionCallbackArguments<'s>,
        this: bindings::Receiver<'s>,
        rv: v8::ReturnValue<'s>,
    ) {
        call_member::<M, _>(scope, &Checked { args, this }, rv);
    }
}

/// One call of `M` through V8's front end, V8 told of the DOM's growth
/// after one that may allocate.
#[inline(always)]
fn call_member<'s, M: TypedMember, A: crate::front::CallArgs<'s>>(
    scope: Scope<'_, 's, '_>,
    args: &A,
    rv: v8::ReturnValue<'s>,
) {
    let mut front = V8Front::new(scope, args, rv);
    M::call(&mut front);
    if M::KIND != MemberKind::Getter {
        front.report();
    }
}

/// A form's call (its `Member<M>`'s), for its overload set's function:
/// with no word of its receiver, and with the receiver V8 checked.
type Call = for<'s> fn(Scope<'_, 's, '_>, v8::FunctionCallbackArguments<'s>, v8::ReturnValue<'s>);
type MethodCall = for<'s> fn(
    Scope<'_, 's, '_>,
    v8::FunctionCallbackArguments<'s>,
    bindings::Receiver<'s>,
    v8::ReturnValue<'s>,
);

/// An overloaded method's forms, or an interface's constructors when it has
/// several: their members (indexes in the interface's, the table's order)
/// and calls.
struct OverloadSet {
    interface: usize,
    members: Vec<usize>,
    calls: Vec<(Call, MethodCall)>,
}

/// Every overload set of the table, found at the first ask: derived from
/// the static table alone (each form's call is a function of the
/// process), so the same in every isolate and renderer, as Blink's
/// generated bindings' overload dispatch is code.
static OVERLOADS: OnceLock<Vec<OverloadSet>> = OnceLock::new();

/// Collects the overload sets: a method whose name another of its
/// interface's methods has, constructors when there are several.
#[derive(Default)]
struct CollectOverloads(Vec<OverloadSet>);

impl TypedVisitor for CollectOverloads {
    fn any<M: TypedMember>(&mut self) {
        // Known at compile time: only an overload's forms have their
        // calls' addresses taken (a function whose address is taken isn't
        // inlined as readily where V8's trampoline calls it: every
        // member's call would pay).
        if !const { dom_api::declare::is_overload(M::INTERFACE, M::MEMBER) } {
            return;
        }
        let members = INTERFACES[M::INTERFACE].members;
        let this = &members[M::MEMBER];
        let same = |member: &dom_api::Member| {
            member.kind == M::KIND && member.name == M::NAME && member.is_static == this.is_static
        };
        let call: (Call, MethodCall) = (
            <Member<M> as bindings::StateCallback<State>>::call,
            <Member<M> as bindings::StateMethod<State>>::call,
        );
        let first = members.iter().position(same).expect("this member is one");
        match self
            .0
            .iter_mut()
            .find(|set| set.interface == M::INTERFACE && set.members.first() == Some(&first))
        {
            Some(set) => {
                set.members.push(M::MEMBER);
                set.calls.push(call);
            }
            None => self.0.push(OverloadSet {
                interface: M::INTERFACE,
                members: vec![M::MEMBER],
                calls: vec![call],
            }),
        }
    }
}

/// The overload set `INTERFACES[interface].members[member]` is a form of,
/// if any (by its index in `OVERLOADS`).
fn overload_set(interface: usize, member: usize) -> Option<usize> {
    OVERLOADS
        .get_or_init(|| {
            let mut collect = CollectOverloads::default();
            dom_api::visit_typed(&mut collect);
            // In the table's order, whatever order the visit took.
            for set in &mut collect.0 {
                let mut forms: Vec<(usize, (Call, MethodCall))> = set
                    .members
                    .iter()
                    .copied()
                    .zip(set.calls.iter().copied())
                    .collect();
                forms.sort_by_key(|(member, _)| *member);
                (set.members, set.calls) = forms.into_iter().unzip();
            }
            collect.0
        })
        .iter()
        .position(|set| set.interface == interface && set.members.contains(&member))
}

/// The function of the overload set `set` (WebIDL's: its `length` the
/// fewest arguments a form needs), its receiver checked by `signature` (a
/// method's; none for constructors).
fn overloaded_template<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    set: usize,
    signature: Option<v8::Local<'s, v8::Signature>>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let overloads = OVERLOADS.get().expect("made by `overload_set`");
    let members = INTERFACES[overloads[set].interface].members;
    let length = overloads[set]
        .members
        .iter()
        .map(|&member| members[member].required())
        .min()
        .unwrap_or(0);
    let data = v8::Integer::new_from_unsigned(scope, u32::try_from(set).expect("a few overloads"));
    let configure = |builder: v8::FunctionBuilder<'s, v8::FunctionTemplate>| {
        builder
            .data(data.into())
            .length(i32::try_from(length).expect("a few parameters"))
    };
    // With a signature, the form is given the receiver V8 checked against
    // it (`bindings::StateMethod`).
    let template = match signature {
        Some(signature) => {
            bindings::state_method_template::<State, Overloaded>(scope, signature, |builder| {
                configure(builder).constructor_behavior(v8::ConstructorBehavior::Throw)
            })
        }
        None => bindings::state_function_template::<State, Overloaded>(scope, configure),
    };
    if signature.is_some() {
        checks_receiver(template);
    }
    template
}

/// `template`, a member's function, made to have V8 check that its `this`
/// may be accessed: an object of an origin the calling realm's may not
/// reach (another security token) is refused before the call, as reading
/// its properties is (Blink's `IDLMemberInstaller` does it for every member
/// that isn't `[CrossOrigin]`; V8's default skips the check, so a realm's
/// own `document` getter would run on another origin's window). The
/// cross-origin properties' functions are others (`cross_origin`).
pub(crate) fn checks_receiver(template: v8::Local<'_, v8::FunctionTemplate>) {
    template.set_accept_any_receiver(false);
}

/// An overload set's function: the form for the arguments
/// (`resolve_overload`, an argument's interface told by its template),
/// called as its own function is; a `TypeError` for a number of them no
/// form takes.
struct Overloaded;

impl bindings::StateCallback<State> for Overloaded {
    fn call<'s>(
        scope: Scope<'_, 's, '_>,
        args: v8::FunctionCallbackArguments<'s>,
        rv: v8::ReturnValue<'s>,
    ) {
        if let Some((scope, set, form)) = Overloaded::form(scope, &args) {
            (set.calls[form].0)(scope, args, rv);
        }
    }
}

/// An overloaded method's (one with a signature): its form given the
/// receiver V8 checked.
impl bindings::StateMethod<State> for Overloaded {
    fn call<'s>(
        scope: Scope<'_, 's, '_>,
        args: v8::FunctionCallbackArguments<'s>,
        this: bindings::Receiver<'s>,
        rv: v8::ReturnValue<'s>,
    ) {
        if let Some((scope, set, form)) = Overloaded::form(scope, &args) {
            (set.calls[form].1)(scope, args, this, rv);
        }
    }
}

impl Overloaded {
    /// The call's overload set and the form its arguments pick; `None`
    /// with a `TypeError` thrown for a number of them no form takes.
    fn form<'a, 's, 'i>(
        mut scope: Scope<'a, 's, 'i>,
        args: &v8::FunctionCallbackArguments<'s>,
    ) -> Option<(Scope<'a, 's, 'i>, &'static OverloadSet, usize)> {
        let set = args
            .data()
            .uint32_value(scope.scope())
            .and_then(|set| usize::try_from(set).ok())?;
        let set = &OVERLOADS.get().expect("made with its template")[set];
        let members = INTERFACES[set.interface].members;
        let forms: Vec<&dom_api::Member> =
            set.members.iter().map(|&member| &members[member]).collect();
        let given = usize::try_from(args.length()).unwrap_or(0);
        let form = dom_api::resolve_overload(&forms, given, |index, name| {
            let value = args.get(i32::try_from(index).unwrap_or(i32::MAX));
            implements(scope.scope(), value, name)
                .expect("an overload's interface `implements` knows")
        });
        match form {
            Ok(form) => Some((scope, set, form)),
            Err(message) => {
                crate::throw_type_error(
                    scope.scope(),
                    &format!("Failed to execute '{}': {message}", forms[0].name),
                );
                None
            }
        }
    }
}

/// The interfaces `implements` tells an object of.
const TOLD_APART: [&str; 2] = ["Uint8ClampedArray", "Path2D"];

/// Whether `value` is an object of the interface named `name`, for the
/// interfaces an overload's forms are told apart by (`resolve_overload`,
/// `TOLD_APART`); `None` for another.
fn implements(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<'_, v8::Value>,
    name: &str,
) -> Option<bool> {
    Some(match name {
        "Uint8ClampedArray" => value.is_uint8_clamped_array(),
        "Path2D" => bindings::is_wrapper::<dom::Path2d>(scope, value),
        _ => return None,
    })
}

/// The interfaces the table's overloads are told apart by that
/// `implements` doesn't tell (one would fail its resolution): none, the
/// tests check.
#[doc(hidden)]
pub fn overload_interfaces_untold() -> Vec<&'static str> {
    let _ = overload_set(0, 0);
    let mut names = Vec::new();
    for set in OVERLOADS.get().expect("just made") {
        let members = INTERFACES[set.interface].members;
        let forms: Vec<&dom_api::Member> =
            set.members.iter().map(|&member| &members[member]).collect();
        names.extend(
            dom_api::overload_interfaces(&forms)
                .into_iter()
                .filter(|name| !TOLD_APART.contains(name)),
        );
    }
    names
}

/// The nearest of `INTERFACES[interface]`'s ancestors with an indexed
/// getter, if it has none of its own.
fn indexed_ancestor(interface: usize) -> Option<usize> {
    let has = |desc: &dom_api::InterfaceDesc| desc.members.iter().any(|member| member.indexed);
    let mut at = INTERFACES[interface];
    if has(at) {
        return None;
    }
    while let Some(parent) = at.parent {
        let index = INTERFACES
            .iter()
            .position(|desc| desc.name == parent)
            .expect("a parent of the table");
        at = INTERFACES[index];
        if has(at) {
            return Some(index);
        }
    }
    None
}

/// The indexed getter of `ancestor` on another interface's instances
/// (`interface`'s, with its own indexed setter if it has one).
struct InheritedIndexed<'a, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    interface: usize,
    ancestor: usize,
    instance: v8::Local<'s, v8::ObjectTemplate>,
}

impl TypedVisitor for InheritedIndexed<'_, '_, '_> {
    fn indexed<M: TypedMember>(&mut self) {
        if M::INTERFACE == self.ancestor {
            set_indexed::<M>(self.scope, self.interface, self.instance);
        }
    }
}

/// `interface`'s instances' indexed getter, `M`, and its indexed setter if
/// it has one (`named::IndexedSetter`, which finds it by the interface's
/// index in the handler's data).
fn set_indexed<'s, M: TypedMember>(
    scope: &mut v8::PinScope<'s, '_>,
    interface: usize,
    instance: v8::Local<'s, v8::ObjectTemplate>,
) {
    let setter = INTERFACES[interface]
        .members
        .iter()
        .any(|member| member.kind == MemberKind::IndexedSetter);
    if setter {
        let data =
            v8::Integer::new_from_unsigned(scope, u32::try_from(interface).expect("a small table"));
        bindings::set_state_indexed_properties::<State, Indexed<M>, IndexedSetter>(
            scope,
            instance,
            data.into(),
        );
    } else {
        let data =
            v8::Integer::new_from_unsigned(scope, u32::try_from(interface).expect("a small table"));
        bindings::set_state_indexed_getter::<State, Indexed<M>>(scope, instance, data.into());
    }
}

/// Whether `INTERFACES[interface]` (or an ancestor) has an indexed getter.
fn has_indexed(interface: usize) -> bool {
    let mut at = Some(INTERFACES[interface]);
    while let Some(desc) = at {
        if desc.members.iter().any(|member| member.indexed) {
            return true;
        }
        at = desc
            .parent
            .and_then(|parent| INTERFACES.iter().find(|desc| desc.name == parent).copied());
    }
    false
}

/// An interface's indexed getter (`#[indexed]`, its typed path `M`):
/// `object[i]` calls it with `i`, its null no property (V8 then looks on
/// the prototype); `@@iterator` is `Array.prototype.values` (WebIDL's for an
/// interface with an indexed getter and `length`), on the prototype of the
/// interface that declares it. Its query and enumerator ask the getter
/// (`in`, `Object.keys`, a property's descriptor).
fn install_indexed<'s, M: TypedMember>(
    scope: &mut v8::PinScope<'s, '_>,
    interface: usize,
    instance: v8::Local<'s, v8::ObjectTemplate>,
    prototype: v8::Local<'s, v8::ObjectTemplate>,
) {
    set_indexed::<M>(scope, interface, instance);
    // A window's child windows by index are its `WindowProxy`'s (HTML's
    // `[[GetOwnProperty]]`), no indexed getter of WebIDL's: not iterable.
    if INTERFACES[interface].name == "Window" {
        return;
    }
    let iterator = v8::Symbol::get_iterator(scope);
    prototype.set_intrinsic_data_property(
        iterator.into(),
        v8::Intrinsic::ArrayProtoValues,
        v8::PropertyAttribute::DONT_ENUM,
    );
}

/// `object[index]`: the indexed getter's typed path, its one argument the
/// index.
struct Indexed<M>(PhantomData<M>);

impl<M: TypedMember> bindings::StateIndexedGetter<State> for Indexed<M> {
    fn get<'s>(
        mut scope: Scope<'_, 's, '_>,
        index: u32,
        _: v8::PropertyCallbackArguments<'s>,
        this: bindings::Receiver<'s>,
        rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted {
        let call = IndexArgs {
            index: v8::Integer::new_from_unsigned(scope.scope(), index).into(),
            this: this.object(),
            receiver: Some(this),
            undefined: v8::undefined(scope.scope()).into(),
        };
        let mut front = V8Front::new(scope, &call, rv);
        M::call(&mut front);
        if front.returned_no_item() {
            v8::Intercepted::kNo
        } else {
            v8::Intercepted::kYes
        }
    }

    /// WebIDL's indexed property: enumerable, configurable, read-only
    /// unless the interface has an indexed setter.
    fn query<'s>(
        mut scope: Scope<'_, 's, '_>,
        index: u32,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Integer>,
    ) -> v8::Intercepted {
        let setter = crate::named::has_indexed_setter(&mut scope, &args);
        match crate::named::has_index::<M>(scope, index, args.holder()).1 {
            Some(false) => v8::Intercepted::kNo,
            None => v8::Intercepted::kYes,
            Some(true) => {
                let attributes = if setter {
                    v8::PropertyAttribute::NONE
                } else {
                    v8::PropertyAttribute::READ_ONLY
                };
                rv.set_int32(attributes.as_u32().cast_signed());
                v8::Intercepted::kYes
            }
        }
    }

    /// The supported property indices: from 0 while there is an item (an
    /// indexed getter's are each below its `length`).
    fn enumerate<'s>(
        mut scope: Scope<'_, 's, '_>,
        args: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Array>,
    ) {
        let this = args.holder();
        let mut count = 0_u32;
        loop {
            let (back, has) = crate::named::has_index::<M>(scope, count, this);
            scope = back;
            match has {
                Some(true) => {}
                Some(false) => break,
                None => return,
            }
            let Some(next) = count.checked_add(1) else {
                break;
            };
            count = next;
        }
        let indices: Vec<v8::Local<'s, v8::Value>> = (0..count)
            .map(|index| v8::Integer::new_from_unsigned(scope.scope(), index).into())
            .collect();
        rv.set(v8::Array::new_with_elements(scope.scope(), &indices));
    }
}

fn illegal_constructor(
    scope: &mut v8::PinScope<'_, '_>,
    _: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    crate::throw_type_error(scope, "Illegal constructor");
}

/// WebIDL's legacy callback interface object of `interface`: a function
/// named for it, without a `prototype`, that throws when called, with its
/// constants (read-only, enumerable, not configurable).
fn callback_interface_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    interface: &dom_api::declare::CallbackInterfaceDesc,
) -> Option<v8::Local<'s, v8::Function>> {
    let template = v8::FunctionTemplate::new(scope, illegal_constructor);
    template.set_class_name(string(scope, interface.name));
    template.remove_prototype();
    let function = template.get_function(scope)?;
    for &(name, value) in interface.constants {
        let name = string(scope, name);
        let value = v8::Integer::new_from_unsigned(scope, value);
        function.define_own_property(
            scope,
            name.into(),
            value.into(),
            v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
        )?;
    }
    Some(function)
}

/// A new wrapper of `INTERFACES[interface]`'s, in the current context,
/// wrapping nothing yet: a copy of the context's boilerplate (Blink's
/// `CreateWrapperFromCache`). V8 allocates: never in an operation of the
/// DOM.
pub(crate) fn new_wrapper<'s>(
    scope: &mut Scope<'_, 's, '_>,
    interface: usize,
) -> v8::Local<'s, v8::Object> {
    let context = scope.scope().get_current_context();
    let cached = {
        let (state, mut no_gc) = scope.state();
        match &state.boilerplates[interface] {
            Some((of, boilerplate)) if *of == context => Some(no_gc.local(boilerplate)),
            _ => None,
        }
    };
    if let Some(boilerplate) = cached {
        return v8::machopo::clone_object(scope.scope(), boilerplate);
    }
    let template = template(scope, interface);
    let scope_ = scope.scope();
    let boilerplate = template
        .instance_template(scope_)
        .new_instance(scope_)
        .expect("a wrapper boilerplate");
    let made = (
        v8::Global::new(scope_, context),
        v8::Global::new(scope_, boilerplate),
    );
    scope.state_mut().0.boilerplates[interface] = Some(made);
    v8::machopo::clone_object(scope.scope(), boilerplate)
}

/// Installs the table's interface objects, `DOMException` and
/// `QuotaExceededError`, and the legacy
/// callback interface objects (`NodeFilter`) on `global` (a realm's): lazy
/// data properties (made the first time they are read), not enumerable,
/// as WebIDL's interface objects. The isolate's DOM is made with the
/// first.
pub fn install(scope: &mut v8::PinScope<'_, '_>, global: v8::Local<'_, v8::Object>) {
    state::install(scope);
    let names = INTERFACES
        .iter()
        .map(|interface| interface.name)
        .chain(["DOMException", "QuotaExceededError"])
        .chain(CALLBACK_INTERFACES.iter().map(|interface| interface.name));
    for (index, name) in names.enumerate() {
        let name = string(scope, name);
        let data = v8::Integer::new(scope, i32::try_from(index).expect("a small table"));
        global.set_lazy_data_property_with_data(
            scope,
            name.into(),
            interface_object,
            data.into(),
            v8::PropertyAttribute::DONT_ENUM,
            v8::SideEffectType::HasNoSideEffect,
            v8::SideEffectType::HasSideEffect,
        );
    }
    // WebIDL's `[LegacyWindowAlias]`es: the same interface object by
    // another name.
    for (alias, name) in LEGACY_WINDOW_ALIASES {
        let index = INTERFACES
            .iter()
            .position(|interface| interface.name == name)
            .expect("an interface of the table");
        let alias = string(scope, alias);
        let data = v8::Integer::new(scope, i32::try_from(index).expect("a small table"));
        global.set_lazy_data_property_with_data(
            scope,
            alias.into(),
            interface_object,
            data.into(),
            v8::PropertyAttribute::DONT_ENUM,
            v8::SideEffectType::HasNoSideEffect,
            v8::SideEffectType::HasSideEffect,
        );
    }
    // WebIDL's legacy factory functions (`Option`), each by its interface's
    // and member's index.
    for (interface, desc) in INTERFACES.iter().enumerate() {
        for (member, entry) in desc.members.iter().enumerate() {
            if entry.kind != MemberKind::LegacyFactory {
                continue;
            }
            let name = string(scope, entry.name);
            let data = v8::Integer::new_from_unsigned(scope, factory_data(interface, member));
            global.set_lazy_data_property_with_data(
                scope,
                name.into(),
                legacy_factory_object,
                data.into(),
                v8::PropertyAttribute::DONT_ENUM,
                v8::SideEffectType::HasNoSideEffect,
                v8::SideEffectType::HasSideEffect,
            );
        }
    }
}

/// The global's other names of interface objects, and theirs: Blink's
/// IDL gives `MutationObserver` `[LegacyWindowAlias=WebKitMutationObserver]`
/// (the DOM's doesn't), which pages written for old WebKit still look for.
const LEGACY_WINDOW_ALIASES: [(&str, &str); 1] = [("WebKitMutationObserver", "MutationObserver")];

/// A legacy factory function's lazy property's data: its interface's and
/// member's indices.
fn factory_data(interface: usize, member: usize) -> u32 {
    let interface = u32::try_from(interface).expect("a small table");
    let member = u32::try_from(member).expect("a small interface");
    assert!(member < 1 << 10, "fewer than 1024 members");
    interface
        .checked_mul(1 << 10)
        .and_then(|index| index.checked_add(member))
        .expect("a small table")
}

/// A legacy factory function of the global, read the first time: a
/// function of its typed path (which checks it is called with `new`) whose
/// `prototype` is its interface's prototype object, read-only (WebIDL's).
fn legacy_factory_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let data = v8::Local::<v8::Integer>::try_from(args.data())
        .ok()
        .and_then(|data| u32::try_from(data.value()).ok())
        .expect("a legacy factory's lazy property carries its indices");
    let index = |bits: u32| usize::try_from(bits).expect("a small table");
    let (interface, member) = (index(data >> 10), index(data & ((1 << 10) - 1)));
    let Some(context) = args.holder().get_creation_context(scope) else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    let Some(mut scope) = state::scope(scope) else {
        return;
    };
    let mut factory = Factory {
        scope: scope.scope(),
        interface,
        member,
        template: None,
    };
    dom_api::visit_typed(&mut factory);
    let template = factory.template.expect("a legacy factory's typed path");
    let Some(function) = template.get_function(scope.scope()) else {
        return;
    };
    let name = string(scope.scope(), INTERFACES[interface].members[member].name);
    function.set_name(name);
    let Some(object) = interface_function(&mut scope, interface) else {
        return;
    };
    let key = string(scope.scope(), "prototype");
    let Some(prototype) = object.get(scope.scope(), key.into()) else {
        return;
    };
    function.define_own_property(
        scope.scope(),
        key.into(),
        prototype,
        v8::PropertyAttribute::READ_ONLY
            | v8::PropertyAttribute::DONT_ENUM
            | v8::PropertyAttribute::DONT_DELETE,
    );
    rv.set(function.into());
}

/// A legacy factory function's template, from its typed path.
struct Factory<'a, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    interface: usize,
    member: usize,
    template: Option<v8::Local<'s, v8::FunctionTemplate>>,
}

impl TypedVisitor for Factory<'_, '_, '_> {
    fn legacy_factory<M: TypedMember>(&mut self) {
        if M::INTERFACE != self.interface || M::MEMBER != self.member {
            return;
        }
        let length = INTERFACES[M::INTERFACE].members[M::MEMBER].required();
        self.template = Some(bindings::state_function_template::<State, Member<M>>(
            self.scope,
            |builder| builder.length(i32::try_from(length).expect("a few parameters")),
        ));
    }
}

/// An interface object of the global, read the first time: V8 replaces the
/// property with a data property of it.
fn interface_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let index = v8::Local::<v8::Integer>::try_from(args.data())
        .expect("an interface object's lazy property carries its index")
        .value();
    let index = usize::try_from(index).expect("an interface index");
    let Some(context) = args.holder().get_creation_context(scope) else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    let Some(mut scope) = state::scope(scope) else {
        return;
    };
    if INTERFACES
        .get(index)
        .is_some_and(|interface| interface.namespace)
    {
        if let Some(object) = namespace_object(&mut scope, index) {
            rv.set(object.into());
        }
        return;
    }
    let function = match index.checked_sub(INTERFACES.len()) {
        None => interface_function(&mut scope, index),
        Some(0) => exception::interface_object(&mut scope),
        Some(1) => exception::quota_interface_object(&mut scope),
        Some(callback) => callback_interface_object(
            scope.scope(),
            &CALLBACK_INTERFACES[callback.saturating_sub(2)],
        ),
    };
    if let Some(function) = function {
        rv.set(function.into());
    }
}

/// The namespace `INTERFACES[interface]` (WebIDL's namespace object): a
/// plain object of the current realm (`Object.prototype`'s, not callable)
/// with its operations and its name as its class string, as Blink's
/// generated `InstallInterfaceTemplate` for a namespace makes it. Its
/// operations are the template's static ones, taken off its function.
fn namespace_object<'s>(
    scope: &mut Scope<'_, 's, '_>,
    interface: usize,
) -> Option<v8::Local<'s, v8::Object>> {
    let function = template(scope, interface).get_function(scope.scope())?;
    let scope_ = scope.scope();
    let object = v8::Object::new(scope_);
    let desc = &INTERFACES[interface];
    let mut names: Vec<&str> = desc.members.iter().map(|member| member.name).collect();
    names.dedup();
    for name in names {
        let key = string(scope_, name);
        let operation = function.get(scope_, key.into())?;
        object.define_own_property(scope_, key.into(), operation, v8::PropertyAttribute::NONE);
    }
    let tag = v8::Symbol::get_to_string_tag(scope_);
    let name = string(scope_, desc.name);
    object.define_own_property(
        scope_,
        tag.into(),
        name.into(),
        v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_ENUM,
    );
    Some(object)
}

/// `INTERFACES[interface]`'s interface object in the current realm, whose
/// prototype is its parent's (WebIDL's; V8's `Inherit` links only their
/// prototype objects), as Blink sets it.
fn interface_function<'s>(
    scope: &mut Scope<'_, 's, '_>,
    interface: usize,
) -> Option<v8::Local<'s, v8::Function>> {
    let function = template(scope, interface).get_function(scope.scope())?;
    if let Some(parent) = INTERFACES[interface].parent {
        let parent = INTERFACES
            .iter()
            .position(|interface| interface.name == parent)
            .expect("an interface of the table");
        let parent = interface_function(scope, parent)?;
        function.set_prototype(scope.scope(), parent.into())?;
    }
    Some(function)
}

/// The prototype's `Symbol.toStringTag`: the interface's name (WebIDL's
/// class string), read-only, not enumerable.
pub(crate) fn to_string_tag(
    scope: &mut v8::PinScope<'_, '_>,
    prototype: v8::Local<'_, v8::ObjectTemplate>,
    name: &str,
) {
    let tag = v8::Symbol::get_to_string_tag(scope);
    let name = string(scope, name);
    prototype.set_with_attr(
        tag.into(),
        name.into(),
        v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_ENUM,
    );
}

/// A WebIDL constant's attributes: read-only, not configurable.
pub(crate) fn constant() -> v8::PropertyAttribute {
    v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE
}

/// An internalized string of `text`, a name.
pub(crate) fn string<'s>(
    scope: &v8::PinScope<'s, '_, ()>,
    text: &str,
) -> v8::Local<'s, v8::String> {
    v8::String::new_from_utf8(scope, text.as_bytes(), v8::NewStringType::Internalized)
        .expect("a short name")
}
