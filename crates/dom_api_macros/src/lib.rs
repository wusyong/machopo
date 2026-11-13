//! `declare!`: the DOM's declaration table (`dom_api::interfaces`),
//! as Blink's bindings generator reads its IDL. The syntax is in
//! `dom_api::declare`'s documentation:
//!
//! ```text
//! interface Name : Parent [NodeKind, ...] {
//!     const NAME: u32 = value;
//!     attribute webIdlName: Type, get = module::getter;
//!     attribute webIdlName: Type, get = module::getter, set = module::setter;
//!     method webIdlName(arg: Type, ...) -> Type = module::function;
//! }
//! ```
//!
//! It generates, where it is invoked:
//!
//! - Each interface's unit type and its `Interface` impl (its description:
//!   name, parent, kinds, members), and the interfaces' and their nullable
//!   forms' `ArgType` and `RetType` impls.
//! - `INTERFACES`, in declaration order.
//! - Each member's typed path (`typed_paths::<interface>::M<J>`, `J` its
//!   index in the interface's members, constants counted) and its
//!   `TypedMember` impl: its call, its arguments' and result's types.
//! - `visit_typed`, which hands each typed path to a visitor.
//! - `for_each_member!` (exported): `$cb!(Interface, member, Kind, path)`
//!   for each typed path, so that an engine's crate expands code of each
//!   member's own types (wasmtime's typed host functions), which a generic
//!   visitor can't prove it has.
//!
//! The paths it generates are `::dom_api`'s (the crate names itself
//! so); `for_each_member!`'s paths name the table's module,
//! `dom_api::interfaces`.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{ToTokens, format_ident, quote};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Ident, LitStr, Path, Token, Type, braced, bracketed, parenthesized};

mod kw {
    syn::custom_keyword!(interface);
    syn::custom_keyword!(dictionary);
    syn::custom_keyword!(callback);
    syn::custom_keyword!(constructor);
    syn::custom_keyword!(attribute);
    syn::custom_keyword!(method);
    syn::custom_keyword!(get);
    syn::custom_keyword!(element);
    syn::custom_keyword!(set);
}

#[proc_macro]
pub fn declare(input: TokenStream) -> TokenStream {
    match syn::parse::<Table>(input).and_then(|table| expand(&table)) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// The table: its interfaces, parents first, its dictionaries and its
/// callback interfaces.
struct Table {
    interfaces: Vec<InterfaceDef>,
    dictionaries: Vec<DictionaryDef>,
    callbacks: Vec<CallbackDef>,
}

/// A callback interface (`callback Name;`): an object of the engine's a
/// member keeps (a listener's), which the front end holds. One with
/// constants (`callback Name { const A: u32 = 1; }`) has WebIDL's legacy
/// callback interface object (`NodeFilter`), with them.
struct CallbackDef {
    attrs: Vec<Attribute>,
    name: Ident,
    constants: Vec<(Ident, Expr)>,
}

struct InterfaceDef {
    attrs: Vec<Attribute>,
    /// `#[iterable]`: WebIDL's value iterator (`iterable<T>` on an
    /// interface with an indexed getter): `entries`, `forEach`, `keys` and
    /// `values` are `Array.prototype`'s.
    iterable: bool,
    /// `#[override_builtins]`: WebIDL's `[LegacyOverrideBuiltIns]`, its
    /// named properties shadowing the object's own and its prototypes'.
    override_builtins: bool,
    /// `#[namespace]`: WebIDL's namespace (`CSS`): a plain object of the
    /// global, its members static operations, no objects of its own.
    namespace: bool,
    /// `#[global]`: WebIDL's `[Global]` (`Window`), its members the global
    /// object's own properties.
    global: bool,
    /// `#[is_html_dda]`: HTML's [[IsHTMLDDA]] (`HTMLAllCollection`): its
    /// objects undetectable in JS, callable as its `item`.
    is_html_dda: bool,
    name: Ident,
    parent: Option<Ident>,
    objects: Objects,
    members: Vec<MemberDef>,
}

/// What an interface's objects are: nodes, its own kinds listed; or the
/// GC type named.
enum Objects {
    Nodes(Vec<Ident>),
    Type(Path),
    /// `[element html]`, `[element html meta]`, `[element html h1 h2]`,
    /// `[element svg]`, `[element svg rect]`: HTML or SVG elements
    /// (`true`), of its local names or of no other interface's.
    Element(bool, Vec<Ident>),
}

struct DictionaryDef {
    attrs: Vec<Attribute>,
    name: Ident,
    members: Vec<DictMemberDef>,
}

struct DictMemberDef {
    name: Ident,
    ty: Type,
    default: Expr,
}

enum MemberDef {
    Const {
        name: Ident,
        value: Expr,
    },
    Attribute {
        name: Ident,
        ty: Type,
        get: Path,
        /// Its setter, and the type it takes if it isn't the attribute's
        /// (`set: DomString = f`: WebIDL's `[PutForwards]`, assigning the
        /// object's attribute). Boxed: an attribute is otherwise the largest
        /// member by two syn types, by however much syn's features make them.
        set: Option<Box<(Option<Type>, Path)>>,
        /// `#[unforgeable]`: WebIDL's `[LegacyUnforgeable]`, on each object,
        /// not its prototype, and not configurable.
        unforgeable: bool,
        /// `#[replaceable]`: WebIDL's `[Replaceable]`: read-only, but
        /// assigning it makes a data property of the object in its place.
        replaceable: bool,
        /// `#[calls_script]`: its getter runs out of the operation, as a
        /// method that calls script (an event handler's, which its engine
        /// compiles first).
        calls_script: bool,
        /// `#[front]`: its setter gets the operation's front end too (to
        /// store a callback, as a method's).
        front: bool,
        /// `#[ce_reactions]`: WebIDL's `[CEReactions]` (its setter's).
        ce_reactions: bool,
        /// `#[static_method]`: WebIDL's `static` attribute: no `this`, on
        /// the interface object, read-only.
        is_static: bool,
        /// `#[cross_origin]`, `#[cross_origin(setter)]`,
        /// `#[cross_origin(getter, setter)]`: WebIDL's `[CrossOrigin]` (its
        /// getter by default, as Blink's IDL): whether its getter and its
        /// setter are a cross-origin property's.
        cross_origin: (bool, bool),
    },
    Method {
        name: Ident,
        args: Vec<Arg>,
        ret: Option<Type>,
        function: Path,
        /// `#[front]`: its function gets the front end too (for what
        /// only the engine does: comparing and storing a callback).
        front: bool,
        /// `#[realm]`: `#[front]`, the window of `this`'s relevant realm found
        /// before its operation (`Front::prepare_relevant_window`, then
        /// `OpFront::current_window`: what it queues a task in, a blob
        /// read's).
        realm: bool,
        /// `#[calls_script]`: its function runs out of the operation and
        /// calls script through the front end (`ScriptHost`).
        calls_script: bool,
        /// `#[first(path)]` on a `#[calls_script]` method: a function
        /// tried first, in one operation (as any other member's), which
        /// gives the result, or none if this call needs script; only then
        /// is the method's own function run.
        first: Option<Path>,
        /// `#[then_script]`: its function runs in the operation and may
        /// leave something to do that calls script (`typed::ThenScript`),
        /// done out of it before the call returns; most calls leave
        /// nothing, and take the one operation.
        then_script: bool,
        /// `#[ce_reactions]`: HTML's `[CEReactions]`: a scope of the
        /// custom element reactions around its steps, its element queue
        /// invoked as it returns.
        ce_reactions: bool,
        /// `#[static_method]`: WebIDL's `static`: no `this`, on the
        /// interface object.
        is_static: bool,
        /// `#[pairs]`: WebIDL's pair iterator (`iterable<K, V>`): the pair
        /// at an index (`[key, value]`, none past the last), which the
        /// interface's iterators and `forEach` call; anonymous.
        pairs: bool,
        unforgeable: bool,
        /// `#[indexed]`: the interface's indexed getter.
        indexed: bool,
        /// `#[indexed(getter)]`: the indexed getter, anonymous (WebIDL's
        /// `getter T (unsigned long index)`: not on the prototype; its
        /// name in the description is empty).
        anonymous: bool,
        /// `#[named(getter)]`, `#[named(setter)]`, `#[named(deleter)]`,
        /// `#[named(names)]`: one of the interface's named properties'
        /// special operations, anonymous (not on the prototype).
        named: Option<Ident>,
        /// `#[indexed(setter)]`: the interface's indexed setter (WebIDL's
        /// `setter undefined (unsigned long index, T value)`), anonymous.
        indexed_setter: bool,
        /// `#[legacy_factory]`: WebIDL's `[LegacyFactoryFunction]` (`new
        /// Option(...)`): a function of the global named as the method,
        /// called with `new` only, giving the object it makes; no `this`.
        legacy_factory: bool,
        /// `#[observable(set, attribute)]`, `#[observable(delete,
        /// attribute)]`: one of an observable array attribute's backing
        /// list's operations, anonymous.
        observable: Option<(Ident, Ident)>,
        /// `#[cross_origin]`: WebIDL's `[CrossOrigin]`, a cross-origin
        /// property's.
        cross_origin: bool,
    },
    Constructor {
        token: kw::constructor,
        args: Vec<Arg>,
        function: Path,
        /// `#[front]`: its function gets the front end too (the current
        /// realm, `Front::current_window`).
        front: bool,
    },
}

impl Parse for Table {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut interfaces = Vec::new();
        let mut dictionaries = Vec::new();
        let mut callbacks = Vec::new();
        while !input.is_empty() {
            let fork = input.fork();
            fork.call(Attribute::parse_outer)?;
            if fork.peek(kw::dictionary) {
                dictionaries.push(input.parse()?);
            } else if fork.peek(kw::callback) {
                let attrs = input.call(Attribute::parse_outer)?;
                input.parse::<kw::callback>()?;
                let name = input.parse()?;
                let mut constants = Vec::new();
                if input.peek(syn::token::Brace) {
                    let body;
                    braced!(body in input);
                    while !body.is_empty() {
                        body.parse::<Token![const]>()?;
                        let name = body.parse()?;
                        body.parse::<Token![:]>()?;
                        let ty: Ident = body.parse()?;
                        if ty != "u32" {
                            return Err(syn::Error::new(ty.span(), "a constant is a `u32`"));
                        }
                        body.parse::<Token![=]>()?;
                        constants.push((name, body.parse()?));
                        body.parse::<Token![;]>()?;
                    }
                } else {
                    input.parse::<Token![;]>()?;
                }
                callbacks.push(CallbackDef {
                    attrs,
                    name,
                    constants,
                });
            } else {
                interfaces.push(input.parse()?);
            }
        }
        Ok(Self {
            interfaces,
            dictionaries,
            callbacks,
        })
    }
}

impl Parse for DictionaryDef {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        input.parse::<kw::dictionary>()?;
        let name = input.parse()?;
        let body;
        braced!(body in input);
        let members = Punctuated::<DictMemberDef, Token![,]>::parse_terminated(&body)?
            .into_iter()
            .collect();
        Ok(Self {
            attrs,
            name,
            members,
        })
    }
}

impl Parse for DictMemberDef {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.call(Ident::parse_any)?;
        input.parse::<Token![:]>()?;
        let ty = input.parse()?;
        input.parse::<Token![=]>()?;
        Ok(Self {
            name,
            ty,
            default: input.parse()?,
        })
    }
}

impl Parse for InterfaceDef {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut attrs = input.call(Attribute::parse_outer)?;
        let iterable = attrs.iter().any(|attr| attr.path().is_ident("iterable"));
        let override_builtins = attrs
            .iter()
            .any(|attr| attr.path().is_ident("override_builtins"));
        let namespace = attrs.iter().any(|attr| attr.path().is_ident("namespace"));
        let global = attrs.iter().any(|attr| attr.path().is_ident("global"));
        let is_html_dda = attrs.iter().any(|attr| attr.path().is_ident("is_html_dda"));
        attrs.retain(|attr| {
            !attr.path().is_ident("iterable")
                && !attr.path().is_ident("override_builtins")
                && !attr.path().is_ident("namespace")
                && !attr.path().is_ident("global")
                && !attr.path().is_ident("is_html_dda")
        });
        input.parse::<kw::interface>()?;
        let name: Ident = input.parse()?;
        let parent: Option<Ident> = if input.parse::<Option<Token![:]>>()?.is_some() {
            Some(input.parse()?)
        } else {
            None
        };
        let objects;
        bracketed!(objects in input);
        let objects = if objects.peek(Token![type]) {
            objects.parse::<Token![type]>()?;
            objects.parse::<Token![=]>()?;
            Objects::Type(objects.parse()?)
        } else if objects.peek(kw::element) {
            objects.parse::<kw::element>()?;
            let namespace: Ident = objects.parse()?;
            if namespace != "html" && namespace != "svg" {
                return Err(syn::Error::new(
                    namespace.span(),
                    "an element interface's are HTML or SVG elements (`html`, `svg`)",
                ));
            }
            let mut locals = Vec::new();
            while !objects.is_empty() {
                locals.push(objects.parse()?);
            }
            Objects::Element(namespace == "svg", locals)
        } else {
            Objects::Nodes(
                Punctuated::<Ident, Token![,]>::parse_terminated(&objects)?
                    .into_iter()
                    .collect(),
            )
        };
        let body;
        braced!(body in input);
        let mut members = Vec::new();
        while !body.is_empty() {
            members.push(body.parse()?);
        }
        if namespace {
            if let Some(parent) = &parent {
                return Err(syn::Error::new(parent.span(), "a namespace has no parent"));
            }
            if members.iter().any(|member| {
                !matches!(
                    member,
                    MemberDef::Method {
                        is_static: true,
                        ..
                    }
                )
            }) {
                return Err(syn::Error::new(
                    name.span(),
                    "a namespace's members are `#[static_method]` methods",
                ));
            }
        }
        Ok(Self {
            attrs,
            iterable,
            override_builtins,
            namespace,
            global,
            is_html_dda,
            name,
            parent,
            objects,
            members,
        })
    }
}

impl Parse for MemberDef {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let (
            mut front,
            mut calls_script,
            mut then_script,
            mut unforgeable,
            mut indexed,
            mut ce_reactions,
            mut is_static,
            mut pairs,
            mut replaceable,
            mut realm,
        ) = (
            false, false, false, false, false, false, false, false, false, false,
        );
        let mut first: Option<Path> = None;
        let mut named = None;
        let mut observable = None;
        let mut anonymous = false;
        let (mut indexed_setter, mut legacy_factory) = (false, false);
        let mut cross_origin = None;
        // Each attribute, and the members it may mark.
        for attr in &attrs {
            if attr.path().is_ident("cross_origin") {
                if !input.peek(kw::method) && !input.peek(kw::attribute) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`#[cross_origin]` marks a method or an attribute",
                    ));
                }
                cross_origin = Some(match &attr.meta {
                    syn::Meta::Path(_) => (true, false),
                    _ => {
                        let accessors = attr.parse_args_with(
                            Punctuated::<Ident, Token![,]>::parse_separated_nonempty,
                        )?;
                        let mut get_set = (false, false);
                        for accessor in &accessors {
                            if accessor == "getter" {
                                get_set.0 = true;
                            } else if accessor == "setter" {
                                get_set.1 = true;
                            } else {
                                return Err(syn::Error::new(
                                    accessor.span(),
                                    "a cross-origin attribute's accessors are `getter` and `setter`",
                                ));
                            }
                        }
                        if !input.peek(kw::attribute) {
                            return Err(syn::Error::new_spanned(
                                attr,
                                "a cross-origin method is `#[cross_origin]`",
                            ));
                        }
                        get_set
                    }
                });
                continue;
            }
            if attr.path().is_ident("indexed") && matches!(attr.meta, syn::Meta::List(_)) {
                let special: Ident = attr.parse_args()?;
                if special != "getter" && special != "setter" {
                    return Err(syn::Error::new(
                        special.span(),
                        "an anonymous indexed special operation is `#[indexed(getter)]` or `#[indexed(setter)]`",
                    ));
                }
                if !input.peek(kw::method) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`#[indexed(..)]` marks a method",
                    ));
                }
                if special == "getter" {
                    indexed = true;
                } else {
                    indexed_setter = true;
                }
                anonymous = true;
                continue;
            }
            if attr.path().is_ident("legacy_factory") {
                if !input.peek(kw::method) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`#[legacy_factory]` marks a method",
                    ));
                }
                (legacy_factory, is_static) = (true, true);
                continue;
            }
            if attr.path().is_ident("observable") {
                let (operation, attribute) = attr.parse_args_with(|input: ParseStream<'_>| {
                    let operation: Ident = input.parse()?;
                    input.parse::<Token![,]>()?;
                    Ok((operation, input.parse::<Ident>()?))
                })?;
                if operation != "set" && operation != "delete" {
                    return Err(syn::Error::new(
                        operation.span(),
                        "an observable array's operation is `set` or `delete`",
                    ));
                }
                if !input.peek(kw::method) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`#[observable(..)]` marks a method",
                    ));
                }
                observable = Some((operation, attribute));
                continue;
            }
            if attr.path().is_ident("first") {
                if !input.peek(kw::method) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`#[first(..)]` marks a method",
                    ));
                }
                first = Some(attr.parse_args()?);
                continue;
            }
            if attr.path().is_ident("named") {
                let special: Ident = attr.parse_args()?;
                if !["getter", "setter", "deleter", "names"]
                    .iter()
                    .any(|name| special == name)
                {
                    return Err(syn::Error::new(
                        special.span(),
                        "a named property's operation is `getter`, `setter`, `deleter` or `names`",
                    ));
                }
                if !input.peek(kw::method) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`#[named(..)]` marks a method",
                    ));
                }
                named = Some(special);
                continue;
            }
            let (flag, allowed) = if attr.path().is_ident("front") {
                (
                    &mut front,
                    input.peek(kw::method)
                        || input.peek(kw::constructor)
                        || input.peek(kw::attribute),
                )
            } else if attr.path().is_ident("calls_script") {
                (
                    &mut calls_script,
                    input.peek(kw::method) || input.peek(kw::attribute),
                )
            } else if attr.path().is_ident("then_script") {
                (&mut then_script, input.peek(kw::method))
            } else if attr.path().is_ident("unforgeable") {
                (
                    &mut unforgeable,
                    input.peek(kw::method) || input.peek(kw::attribute),
                )
            } else if attr.path().is_ident("indexed") {
                (&mut indexed, input.peek(kw::method))
            } else if attr.path().is_ident("ce_reactions") {
                (
                    &mut ce_reactions,
                    input.peek(kw::method) || input.peek(kw::attribute),
                )
            } else if attr.path().is_ident("static_method") {
                (
                    &mut is_static,
                    input.peek(kw::method) || input.peek(kw::attribute),
                )
            } else if attr.path().is_ident("pairs") {
                (&mut pairs, input.peek(kw::method))
            } else if attr.path().is_ident("replaceable") {
                (&mut replaceable, input.peek(kw::attribute))
            } else if attr.path().is_ident("realm") {
                (&mut realm, input.peek(kw::method))
            } else {
                return Err(syn::Error::new_spanned(
                    attr,
                    "a member's attribute is `#[front]`, `#[calls_script]`, `#[then_script]`, \
                     `#[unforgeable]`, `#[indexed]`, `#[ce_reactions]`, `#[static_method]`, `#[pairs]`, \
                     `#[replaceable]` or `#[realm]`",
                ));
            };
            if !allowed {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`#[front]` marks a method, a constructor or an attribute, `#[calls_script]` a \
                     method or an attribute, `#[then_script]` a method, `#[unforgeable]` a method or \
                     an attribute, `#[indexed]` a method, `#[ce_reactions]` a method or an attribute, \
                     `#[static_method]` a method or an attribute, `#[pairs]` a method, \
                     `#[replaceable]` an attribute, `#[realm]` a method",
                ));
            }
            *flag = true;
        }
        // An attribute's are its setter's and its getter's.
        if front && calls_script && !input.peek(kw::attribute) {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "a method calling script gets the front end as its script host already",
            ));
        }
        // Tried before a function that calls script: of no other.
        if first.is_some() && (!calls_script || ce_reactions || input.peek(kw::attribute)) {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`#[first(..)]` marks a `#[calls_script]` method that isn't `#[ce_reactions]`",
            ));
        }
        // Its function's in an operation (with the front end in it if
        // `#[front]`), with nothing to give (what it leaves is done before
        // the result).
        if then_script && (calls_script || ce_reactions || is_static) {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "a method that leaves script to do is none of `#[calls_script]`, \
                 `#[ce_reactions]` and `#[static_method]`",
            ));
        }
        let lookahead = input.lookahead1();
        let member = if lookahead.peek(Token![const]) {
            input.parse::<Token![const]>()?;
            let name = input.parse()?;
            input.parse::<Token![:]>()?;
            let ty: Ident = input.parse()?;
            if ty != "u32" {
                return Err(syn::Error::new(ty.span(), "a constant is a `u32`"));
            }
            input.parse::<Token![=]>()?;
            let value = input.parse()?;
            Self::Const { name, value }
        } else if lookahead.peek(kw::constructor) {
            let token = input.parse::<kw::constructor>()?;
            let args;
            parenthesized!(args in input);
            let args = Punctuated::<Arg, Token![,]>::parse_terminated(&args)?
                .into_iter()
                .collect();
            input.parse::<Token![=]>()?;
            Self::Constructor {
                token,
                args,
                function: input.parse()?,
                front,
            }
        } else if lookahead.peek(kw::attribute) {
            input.parse::<kw::attribute>()?;
            let name = input.call(Ident::parse_any)?;
            input.parse::<Token![:]>()?;
            let ty = input.parse()?;
            input.parse::<Token![,]>()?;
            input.parse::<kw::get>()?;
            input.parse::<Token![=]>()?;
            let get = input.parse()?;
            let set = if input.parse::<Option<Token![,]>>()?.is_some() {
                input.parse::<kw::set>()?;
                let ty = if input.parse::<Option<Token![:]>>()?.is_some() {
                    Some(input.parse()?)
                } else {
                    None
                };
                input.parse::<Token![=]>()?;
                Some(Box::new((ty, input.parse()?)))
            } else {
                None
            };
            if is_static && (set.is_some() || calls_script) {
                return Err(syn::Error::new_spanned(
                    &attrs[0],
                    "a static attribute is read-only and calls no script",
                ));
            }
            if replaceable && (set.is_some() || is_static || unforgeable) {
                return Err(syn::Error::new_spanned(
                    &attrs[0],
                    "a replaceable attribute is a read-only regular one",
                ));
            }
            let cross_origin = cross_origin.unwrap_or((false, false));
            if (cross_origin.0 || cross_origin.1) && is_static || cross_origin.1 && set.is_none() {
                return Err(syn::Error::new_spanned(
                    &attrs[0],
                    "a cross-origin attribute is a regular one, its setter one it has",
                ));
            }
            Self::Attribute {
                name,
                ty,
                get,
                set,
                unforgeable,
                replaceable,
                calls_script,
                front,
                ce_reactions,
                is_static,
                cross_origin,
            }
        } else if lookahead.peek(kw::method) {
            input.parse::<kw::method>()?;
            let name = input.call(Ident::parse_any)?;
            let args;
            parenthesized!(args in input);
            let args = Punctuated::<Arg, Token![,]>::parse_terminated(&args)?
                .into_iter()
                .collect();
            let ret = if input.parse::<Option<Token![->]>>()?.is_some() {
                Some(input.parse()?)
            } else {
                None
            };
            if then_script && ret.is_some() {
                return Err(syn::Error::new_spanned(
                    &attrs[0],
                    "a method that leaves script to do gives nothing",
                ));
            }
            let cross_origin = cross_origin.is_some();
            if cross_origin
                && (is_static
                    || pairs
                    || indexed
                    || named.is_some()
                    || indexed_setter
                    || legacy_factory
                    || observable.is_some())
            {
                return Err(syn::Error::new_spanned(
                    &attrs[0],
                    "a cross-origin method is a regular one",
                ));
            }
            input.parse::<Token![=]>()?;
            let function = input.parse()?;
            Self::Method {
                name,
                args,
                ret,
                function,
                front: front || realm,
                realm,
                calls_script,
                first,
                then_script,
                ce_reactions,
                is_static,
                pairs,
                unforgeable,
                indexed,
                anonymous,
                named,
                observable,
                indexed_setter,
                legacy_factory,
                cross_origin,
            }
        } else {
            return Err(lookahead.error());
        };
        input.parse::<Token![;]>()?;
        Ok(member)
    }
}

/// A method's or a constructor's argument: `name: Type`, or `name?: Type`
/// if it is optional.
struct Arg {
    name: Ident,
    optional: bool,
    ty: Type,
}

impl Parse for Arg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.call(Ident::parse_any)?;
        let optional = input.parse::<Option<Token![?]>>()?.is_some();
        input.parse::<Token![:]>()?;
        Ok(Self {
            name,
            optional,
            ty: input.parse()?,
        })
    }
}

/// `name` as the table spells it, without `r#`.
fn js_name(name: &Ident) -> LitStr {
    LitStr::new(&name.unraw().to_string(), name.span())
}

/// What a typed path is of: an attribute's getter or setter, or a method.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Getter,
    Setter,
    Method,
    Constructor,
    IndexedSetter,
    LegacyFactory,
}

impl Kind {
    fn ident(self) -> Ident {
        Ident::new(
            match self {
                Self::Getter => "Getter",
                Self::Setter => "Setter",
                Self::Method => "Method",
                Self::Constructor => "Constructor",
                Self::IndexedSetter => "IndexedSetter",
                Self::LegacyFactory => "LegacyFactory",
            },
            Span::call_site(),
        )
    }
}

/// A member's entry in its interface's `members` (an attribute with a
/// setter is two), and its typed path, if it has one (a constant hasn't):
/// its kind, the `TypedVisitor` method that visits it, its impl's items.
struct Entry<'a> {
    name: Ident,
    desc: Tokens,
    path: Option<(Kind, &'static str, Tokens)>,
    /// The types its typed path takes and gives, to tell whether they are
    /// all plain (`plain`).
    types: Vec<&'a Type>,
    /// A static operation: no `this`, so not plain (a guest's member takes
    /// a handle).
    is_static: bool,
    /// An anonymous special operation's (`#[indexed(getter)]`): its name
    /// is empty.
    anonymous: bool,
}

/// The paths every expansion names: the API's crate, and the DOM's.
fn api() -> Tokens {
    quote!(::dom_api)
}

/// An identifier of the generated code's own (hygienic, as a
/// `macro_rules!`'s locals are): no name of the table's can clash with it.
fn local(name: &str) -> Ident {
    Ident::new(name, Span::mixed_site())
}

fn expand(table: &Table) -> syn::Result<Tokens> {
    check(table)?;
    let api = api();
    let mut items = Tokens::new();
    let mut rows = Tokens::new();
    let mut modules = Tokens::new();
    let visitor = local("visitor");
    let not_plain = not_plain(table);
    for (index, interface) in table.interfaces.iter().enumerate() {
        let entries = entries(interface);
        let InterfaceDef {
            attrs,
            iterable,
            override_builtins,
            namespace,
            global,
            is_html_dda,
            name,
            parent,
            objects,
            ..
        } = interface;
        let name_str = LitStr::new(&name.to_string(), name.span());
        let parent = match parent {
            Some(parent) => {
                let parent = LitStr::new(&parent.to_string(), parent.span());
                quote!(::std::option::Option::Some(#parent))
            }
            None => quote!(::std::option::Option::None),
        };
        let (node, kinds, object) = match objects {
            Objects::Nodes(kinds) => (true, kinds.as_slice(), quote!(#api::typed::NodeObject)),
            Objects::Element(..) => (true, &[][..], quote!(#api::typed::NodeObject)),
            Objects::Type(path) => (false, &[][..], quote!(#path)),
        };
        let element = match objects {
            Objects::Element(svg, locals) => {
                let locals = locals
                    .iter()
                    .map(|local| LitStr::new(&local.unraw().to_string(), local.span()));
                quote!(::std::option::Option::Some(#api::declare::ElementDesc {
                    svg: #svg,
                    locals: &[#(#locals),*],
                }))
            }
            Objects::Nodes(_) | Objects::Type(_) => quote!(::std::option::Option::None),
        };
        let descs = entries.iter().map(|entry| &entry.desc);
        items.extend(quote! {
            #(#attrs)*
            pub struct #name;

            impl #api::declare::Interface for #name {
                const DESC: &'static #api::declare::InterfaceDesc = &#api::declare::InterfaceDesc {
                    name: #name_str,
                    parent: #parent,
                    node: #node,
                    kinds: &[#(::dom::NodeKind::#kinds),*],
                    element: #element,
                    iterable: #iterable,
                    override_builtins: #override_builtins,
                    namespace: #namespace,
                    global: #global,
                    is_html_dda: #is_html_dda,
                    members: &[#(#descs),*],
                };
                const NAME: &'static str = #name_str;
                const INDEX: usize = #index;
                type Object = #object;
            }
        });
        if node {
            items.extend(node_types(name));
        } else {
            items.extend(object_types(name));
        }

        let mut impls = Tokens::new();
        let mut visits = Tokens::new();
        for (member, entry) in entries.iter().enumerate() {
            let Some((kind, visit, call)) = &entry.path else {
                continue;
            };
            let visit = Ident::new(visit, Span::call_site());
            let kind_ident = kind.ident();
            let js = if entry.anonymous {
                LitStr::new("", entry.name.span())
            } else {
                js_name(&entry.name)
            };
            impls.extend(quote! {
                impl #api::typed::TypedMember for M<#member> {
                    const INTERFACE: usize = #index;
                    const MEMBER: usize = #member;
                    const NAME: &'static str = #js;
                    const KIND: #api::declare::MemberKind = #api::declare::MemberKind::#kind_ident;
                    #call
                }
            });
            visits.extend(quote!(#visitor.#visit::<M<#member>>();));
            let member_name = &entry.name;
            // An element interface's: a guest's `this` is checked by its
            // kind alone.
            let plain = node
                && !matches!(objects, Objects::Element(..))
                && !KIND_UNTOLD.contains(&name.to_string().as_str())
                && *kind != Kind::Constructor
                && !entry.is_static
                && !entry.types.iter().any(|ty| names_any(ty, &not_plain));
            let class = Ident::new(if plain { "plain" } else { "object" }, Span::call_site());
            rows.extend(quote! {
                $cb!(#name, #member_name, #kind_ident, #api::interfaces::typed_paths::#name::M<#member>, #class);
            });
        }
        modules.extend(quote! {
            #[allow(non_snake_case)]
            pub mod #name {
                #[allow(unused_imports)]
                use super::super::*;

                /// A typed path of this interface: its `J`th member's.
                pub struct M<const J: usize>;

                #impls

                /// This interface's typed paths, in its members' order.
                #[allow(unused_variables)]
                pub(crate) fn visit<V: #api::typed::TypedVisitor>(#visitor: &mut V) {
                    #visits
                }
            }
        });
    }
    for dictionary in &table.dictionaries {
        items.extend(dictionary_items(dictionary));
    }
    for callback in &table.callbacks {
        items.extend(callback_items(callback));
    }
    let names = table.interfaces.iter().map(|interface| &interface.name);
    let visit_names = names.clone();
    let callback_objects = table
        .callbacks
        .iter()
        .filter(|callback| !callback.constants.is_empty())
        .map(|callback| {
            let name = LitStr::new(&callback.name.to_string(), callback.name.span());
            let constants = callback.constants.iter().map(|(constant, value)| {
                let constant = LitStr::new(&constant.to_string(), constant.span());
                quote!((#constant, #value))
            });
            quote!(#api::declare::CallbackInterfaceDesc { name: #name, constants: &[#(#constants),*] })
        });
    Ok(quote! {
        #items

        /// Every interface, in declaration order (parents first).
        pub const INTERFACES: &[&#api::declare::InterfaceDesc] =
            &[#(<#names as #api::declare::Interface>::DESC),*];

        /// The callback interfaces with constants, which have legacy
        /// callback interface objects.
        pub const CALLBACK_INTERFACES: &[#api::declare::CallbackInterfaceDesc] =
            &[#(#callback_objects),*];

        /// Each interface's typed paths, in a module of its own.
        #[doc(hidden)]
        pub mod typed_paths {
            #modules
        }

        /// Every interface's typed paths, in declaration order.
        pub(crate) fn visit_typed<V: #api::typed::TypedVisitor>(#visitor: &mut V) {
            #(typed_paths::#visit_names::visit(#visitor);)*
        }

        /// Calls `$cb!(Interface, member, Kind, path, class)` for each
        /// member's typed path, in `INTERFACES` order (`Kind` a
        /// `MemberKind`'s variant, `path` the type implementing
        /// `TypedMember`): an engine's crate expands code of each member's
        /// own types from it. `class` is `plain` for a member of a node
        /// interface that takes and gives nodes, strings, numbers and
        /// booleans only, `object` for another (a constructor, a member of
        /// another interface, or one with a dictionary): an engine binding
        /// only the plain ones skips the others.
        #[macro_export]
        macro_rules! for_each_member {
            ($cb:ident) => {
                #rows
            };
        }
    })
}

/// The names of the table's types that aren't plain (`for_each_member!`'s
/// `class`): its dictionaries and the interfaces whose objects aren't
/// nodes.
/// The node interfaces whose objects their node kind doesn't tell (a
/// shadow root is a document fragment): a guest's `this` and arguments are
/// checked by their kind alone, so they aren't a guest's.
const KIND_UNTOLD: [&str; 1] = ["ShadowRoot"];

fn not_plain(table: &Table) -> Vec<String> {
    let dictionaries = table
        .dictionaries
        .iter()
        .map(|dictionary| dictionary.name.to_string());
    let objects = table
        .interfaces
        .iter()
        .filter(|interface| matches!(interface.objects, Objects::Type(_)))
        .map(|interface| interface.name.to_string());
    let callbacks = table
        .callbacks
        .iter()
        .map(|callback| callback.name.to_string());
    // A sequence (a new array of objects), a function (an event handler's
    // too) and any value a guest can't hold yet either.
    dictionaries
        .chain(objects)
        .chain(callbacks)
        .chain(
            [
                "Sequence",
                "ObservableArray",
                "Promise",
                "PromiseOfUndefined",
                "DictOrBool",
                "TimerHandler",
                "EventHandler",
                "Any",
                "Rest",
                "Strings",
                "Names",
                "Slottables",
                "NodesOrStrings",
                "GetHtmlOptionsArg",
            ]
            .map(str::to_owned),
        )
        .chain(KIND_UNTOLD.map(str::to_owned))
        .collect()
}

/// Whether `ty` names one of `names`, as itself or a generic's argument
/// (`Option<Event>`: angle brackets aren't a token group). The table's
/// types are paths, never tuples or arrays.
fn names_any(ty: &Type, names: &[String]) -> bool {
    quote!(#ty).into_iter().any(|token| match token {
        proc_macro2::TokenTree::Ident(ident) => names.contains(&ident.to_string()),
        _ => false,
    })
}

/// What a table must be that its syntax doesn't say: each parent declared
/// before its children, each name of an interface or a dictionary once,
/// each attribute's and constant's name unique of its kind within its
/// interface, an optional argument last. A method's name and the
/// constructor may repeat: WebIDL's overloads, one function in JS that
/// picks a form (`declare::resolve_overload`).
fn check(table: &Table) -> syn::Result<()> {
    let mut errors: Option<syn::Error> = None;
    let mut fail = |error: syn::Error| match &mut errors {
        Some(errors) => errors.combine(error),
        None => errors = Some(error),
    };
    let mut types: Vec<&Ident> = Vec::new();
    for name in table
        .interfaces
        .iter()
        .map(|interface| &interface.name)
        .chain(table.dictionaries.iter().map(|dictionary| &dictionary.name))
        .chain(table.callbacks.iter().map(|callback| &callback.name))
    {
        if types.contains(&name) {
            fail(syn::Error::new(
                name.span(),
                format!("`{name}` is declared twice"),
            ));
        }
        types.push(name);
    }
    for (index, interface) in table.interfaces.iter().enumerate() {
        let earlier = &table.interfaces[..index];
        if let Some(parent) = &interface.parent
            && !earlier.iter().any(|other| other.name == *parent)
        {
            fail(syn::Error::new(
                parent.span(),
                format!("no interface `{parent}` is declared before this one"),
            ));
        }
        let mut seen: Vec<(String, u8)> = Vec::new();
        for member in &interface.members {
            let (name, kinds): (Ident, &[u8]) = match member {
                MemberDef::Const { name, .. } => (name.clone(), &[0]),
                MemberDef::Attribute {
                    name, set: None, ..
                } => (name.clone(), &[1]),
                MemberDef::Attribute {
                    name, set: Some(_), ..
                } => (name.clone(), &[1, 2]),
                MemberDef::Method { name, .. } => (name.clone(), &[3]),
                MemberDef::Constructor { token, .. } => {
                    (Ident::new("constructor", token.span), &[4])
                }
            };
            for &kind in kinds {
                let key = (name.unraw().to_string(), kind);
                let overload = matches!(kind, 3 | 4);
                if seen.contains(&key) && !overload {
                    fail(syn::Error::new(
                        name.span(),
                        format!(
                            "`{}` is declared twice in `{}`",
                            name.unraw(),
                            interface.name
                        ),
                    ));
                }
                seen.push(key);
            }
            if let MemberDef::Method { args, .. } | MemberDef::Constructor { args, .. } = member
                && let Some(first) = args.iter().position(|arg| arg.optional)
                && let Some(required) = args[first..].iter().find(|arg| !arg.optional)
            {
                fail(syn::Error::new(
                    required.name.span(),
                    "a required argument after an optional one",
                ));
            }
        }
    }
    errors.map_or(Ok(()), Err)
}

/// An argument's description.
fn param(arg: &Arg) -> Tokens {
    let api = api();
    let name = js_name(&arg.name);
    let (ty, optional) = (&arg.ty, arg.optional);
    quote! {
        #api::declare::Param {
            name: #name,
            ty: <#ty as #api::typed::ArgType>::DESC,
            optional: #optional,
        }
    }
}

/// An interface's members' entries, in order: an attribute's getter, then
/// its setter.
fn entries(interface: &InterfaceDef) -> Vec<Entry<'_>> {
    let api = api();
    let iface = &interface.name;
    let mut entries = Vec::new();
    for member in &interface.members {
        match member {
            MemberDef::Const { name, value } => {
                let js = js_name(name);
                entries.push(Entry {
                    name: name.clone(),
                    desc: quote! {
                        #api::declare::Member {
                            name: #js,
                            kind: #api::declare::MemberKind::Constant(#value),
                            params: &[],
                            returns: #api::declare::TypeDesc::U32,
                            unforgeable: false,
                            indexed: false,
                            named: ::std::option::Option::None,
                            observable: ::std::option::Option::None,
                            is_static: false,
                            pairs: false,
                            replaceable: false,
                        }
                    },
                    path: None,
                    types: Vec::new(),
                    is_static: false,
                    anonymous: false,
                });
            }
            MemberDef::Attribute {
                name,
                ty,
                get,
                set,
                unforgeable,
                replaceable,
                calls_script,
                front,
                ce_reactions,
                is_static,
                cross_origin,
            } => {
                let js = js_name(name);
                entries.push(Entry {
                    name: name.clone(),
                    desc: quote! {
                        #api::declare::Member {
                            name: #js,
                            kind: #api::declare::MemberKind::Getter,
                            params: &[],
                            returns: <#ty as #api::typed::RetType>::DESC,
                            unforgeable: #unforgeable,
                            indexed: false,
                            named: ::std::option::Option::None,
                            observable: ::std::option::Option::None,
                            is_static: #is_static,
                            pairs: false,
                            replaceable: #replaceable,
                        }
                    },
                    path: Some((
                        Kind::Getter,
                        if *is_static {
                            "static_member"
                        } else if cross_origin.0 {
                            "cross_origin"
                        } else {
                            "getter"
                        },
                        if *calls_script {
                            script_method(iface, name, &[], ty, get, false, None)
                        } else if *is_static {
                            // No `this`: a static method's path, of no
                            // arguments.
                            method(iface, name, &[], ty, get, false, false, true, false, false)
                        } else {
                            getter(iface, ty, get)
                        },
                    )),
                    types: vec![ty],
                    is_static: *is_static,
                    anonymous: false,
                });
                if let Some(set) = set {
                    let (set_ty, set) = &**set;
                    let attribute_ty = ty;
                    let ty = set_ty.as_ref().unwrap_or(ty);
                    entries.push(Entry {
                        name: name.clone(),
                        desc: quote! {
                            #api::declare::Member {
                                name: #js,
                                kind: #api::declare::MemberKind::Setter,
                                params: &[#api::declare::Param {
                                    name: "value",
                                    ty: <#ty as #api::typed::ArgType>::DESC,
                                    optional: false,
                                }],
                                returns: #api::declare::TypeDesc::Undefined,
                                unforgeable: #unforgeable,
                                indexed: false,
                                named: ::std::option::Option::None,
                                observable: ::std::option::Option::None,
                                is_static: false,
                                pairs: false,
                                replaceable: false,
                            }
                        },
                        path: Some((
                            Kind::Setter,
                            if cross_origin.1 {
                                "cross_origin"
                            } else {
                                "setter"
                            },
                            setter(iface, ty, set, *front, *ce_reactions),
                        )),
                        // The attribute's type too: a guest has an
                        // attribute whole or not at all.
                        types: vec![ty, attribute_ty],
                        is_static: false,
                        anonymous: false,
                    });
                }
            }
            MemberDef::Method {
                name,
                args,
                ret,
                function,
                front,
                realm,
                calls_script,
                first,
                then_script,
                ce_reactions,
                is_static,
                pairs,
                unforgeable,
                indexed,
                anonymous,
                named,
                observable,
                indexed_setter,
                legacy_factory,
                cross_origin,
            } => {
                let js = js_name(name);
                let desc_name = if *anonymous {
                    LitStr::new("", name.span())
                } else {
                    js.clone()
                };
                let visit = if *legacy_factory {
                    "legacy_factory"
                } else if *indexed_setter {
                    "indexed_setter"
                } else if *pairs {
                    "pairs"
                } else if named.is_some() {
                    "named"
                } else if observable.is_some() {
                    "observable"
                } else if *indexed {
                    "indexed"
                } else if *cross_origin {
                    "cross_origin"
                } else if *is_static {
                    "static_member"
                } else {
                    "method"
                };
                let observable = match observable {
                    Some((operation, attribute)) => {
                        let variant = Ident::new(
                            if operation == "set" {
                                "Set"
                            } else {
                                "DeleteLast"
                            },
                            operation.span(),
                        );
                        let attribute = js_name(attribute);
                        quote!(::std::option::Option::Some(#api::declare::Observable::#variant(#attribute)))
                    }
                    None => quote!(::std::option::Option::None),
                };
                let named = match named {
                    Some(special) => {
                        let variant = Ident::new(
                            match special.to_string().as_str() {
                                "getter" => "Getter",
                                "setter" => "Setter",
                                "deleter" => "Deleter",
                                _ => "Names",
                            },
                            special.span(),
                        );
                        quote!(::std::option::Option::Some(#api::declare::Named::#variant))
                    }
                    None => quote!(::std::option::Option::None),
                };
                let params = args.iter().map(param);
                let mut types: Vec<&Type> = args.iter().map(|arg| &arg.ty).collect();
                if let Some(ret) = ret {
                    types.push(ret);
                }
                let ret: Type = ret.clone().unwrap_or_else(|| syn::parse_quote!(()));
                let kind = if *indexed_setter {
                    Kind::IndexedSetter
                } else if *legacy_factory {
                    Kind::LegacyFactory
                } else {
                    Kind::Method
                };
                let kind_ident = kind.ident();
                entries.push(Entry {
                    name: name.clone(),
                    desc: quote! {
                        #api::declare::Member {
                            name: #desc_name,
                            kind: #api::declare::MemberKind::#kind_ident,
                            params: &[#(#params),*],
                            returns: <#ret as #api::typed::RetType>::DESC,
                            unforgeable: #unforgeable,
                            indexed: #indexed,
                            named: #named,
                            observable: #observable,
                            is_static: #is_static,
                            pairs: #pairs,
                            replaceable: false,
                        }
                    },
                    path: Some((
                        kind,
                        visit,
                        if *calls_script {
                            script_method(
                                iface,
                                name,
                                args,
                                &ret,
                                function,
                                *ce_reactions,
                                first.as_ref(),
                            )
                        } else if *then_script {
                            then_script_method(iface, name, args, function, *front)
                        } else {
                            method(
                                iface,
                                name,
                                args,
                                &ret,
                                function,
                                *front,
                                *ce_reactions,
                                *is_static,
                                *realm,
                                *legacy_factory,
                            )
                        },
                    )),
                    types,
                    is_static: *is_static,
                    anonymous: *anonymous,
                });
            }
            MemberDef::Constructor {
                token,
                args,
                function,
                front,
            } => {
                let params = args.iter().map(param);
                entries.push(Entry {
                    name: Ident::new("constructor", token.span),
                    desc: quote! {
                        #api::declare::Member {
                            name: "constructor",
                            kind: #api::declare::MemberKind::Constructor,
                            params: &[#(#params),*],
                            returns: #api::declare::TypeDesc::Undefined,
                            unforgeable: false,
                            indexed: false,
                            named: ::std::option::Option::None,
                            observable: ::std::option::Option::None,
                            is_static: false,
                            pairs: false,
                            replaceable: false,
                        }
                    },
                    path: Some((
                        Kind::Constructor,
                        "constructor",
                        constructor(iface, args, function, *front),
                    )),
                    types: args.iter().map(|arg| &arg.ty).collect(),
                    is_static: false,
                    anonymous: false,
                });
            }
        }
    }
    entries
}

/// `this`, in an operation: the interface's object type's.
fn this(iface: &Ident) -> Tokens {
    let api = api();
    let (op, cx) = (local("op"), local("cx"));
    quote! {
        <<#iface as #api::declare::Interface>::Object as #api::typed::ObjectType>::this(
            #op,
            #cx,
            <#iface as #api::declare::Interface>::INDEX,
        )
    }
}

/// A getter's call: in an operation with no safepoint, `this`, the
/// function, its result.
fn getter(iface: &Ident, ty: &Type, get: &Path) -> Tokens {
    let api = api();
    let (front, op, cx, this_, result, value, error) = locals();
    let this = this(iface);
    quote! {
        type Args = ();
        type Ret = #ty;

        fn call<F: #api::typed::Front>(#front: &mut F) {
            let prepared = <#ty as #api::typed::RetType>::prepare(#front, ::std::option::Option::None);
            let #result = #front.read(|#cx, #op| {
                let #this_ = #this;
                #get(#cx, #this_).map(|#value| {
                    <#ty as #api::typed::RetType>::give_prepared(
                        #op,
                        #cx,
                        ::std::convert::Into::into(#value),
                        prepared,
                    )
                })
            });
            match #result {
                ::std::result::Result::Ok(#value) => {
                    <#ty as #api::typed::RetType>::finish(#front, #value);
                    #front.finish();
                }
                ::std::result::Result::Err(#error) => #front.throw(#error),
            }
        }
    }
}

/// A setter's call: the value taken first, then in an operation, `this`,
/// the value, the function.
fn setter(iface: &Ident, ty: &Type, set: &Path, with_front: bool, ce_reactions: bool) -> Tokens {
    let api = api();
    let (front, op, cx, this_, result, value, error) = locals();
    let this = this(iface);
    let call = if with_front {
        quote!(#set(#op, #cx, #this_, #value))
    } else {
        quote!(#set(#cx, #this_, #value))
    };
    let op_body = ce_scope(
        ce_reactions,
        &cx,
        &result,
        quote! {
            let #this_ = #this;
            let #value = <#ty as #api::typed::ArgType>::param(#op, #cx, &#value, 0)?;
            #call
        },
    );
    let ce_invoke = ce_invoke(ce_reactions, &front, &result);
    quote! {
        type Args = (#ty,);
        type Ret = ();

        fn call<F: #api::typed::Front>(#front: &mut F) {
            let ::std::option::Option::Some(#value) = <#ty as #api::typed::ArgType>::take(#front, 0) else {
                return;
            };
            let #result = #front.mutate(|#cx, #op| {
                #op_body
            });
            // As a method's.
            #api::typed::release(#value);
            #ce_invoke
            match #result {
                ::std::result::Result::Ok(()) => #front.finish(),
                ::std::result::Result::Err(#error) => #front.throw(#error),
            }
        }
    }
}

/// An operation's body (`steps`, giving a `Result<(), ApiError>`), in a
/// `[CEReactions]` member's scope: entered first, left last, by a guard
/// if a step fails (`dom::CeReactionsScope`); the operation giving
/// whether the scope left an element queue as its `Ok`. No closure or
/// tuple: the result moves as it did (a tuple's cost a `setAttribute` 27
/// instructions). Without `[CEReactions]`, the steps as they are.
fn ce_scope(ce_reactions: bool, cx: &Ident, result: &Ident, steps: Tokens) -> Tokens {
    if !ce_reactions {
        return steps;
    }
    let api = api();
    quote! {
        let scope = ::dom::CeReactionsScope::enter(#cx);
        let #result: ::std::result::Result<_, #api::ApiError> = { #steps };
        let #result = #result?;
        ::std::result::Result::<_, #api::ApiError>::Ok((#result, scope.leave()))
    }
}

/// After a `[CEReactions]` member's operation: its scope's element queue
/// invoked, before its result is made the engine's or its error thrown (a
/// failed step's queue, if it left one, out of line).
fn ce_invoke(ce_reactions: bool, front: &Ident, result: &Ident) -> Tokens {
    if !ce_reactions {
        return quote!();
    }
    quote! {
        let #result = match #result {
            ::std::result::Result::Ok((given, left)) => {
                if left {
                    #front.invoke_ce_reactions();
                }
                ::std::result::Result::Ok(given)
            }
            ::std::result::Result::Err(error) => {
                #front.invoke_ce_reactions();
                ::std::result::Result::Err(error)
            }
        };
    }
}

/// The arguments' locals (`arg0`, `arg1`, ...), their types, and a check
/// that each optional one's type converts undefined to its default.
fn args_of(args: &[Arg]) -> (Vec<Ident>, Vec<&Type>, Tokens) {
    let api = api();
    let names = (0..args.len())
        .map(|i| format_ident!("arg{}", i, span = Span::mixed_site()))
        .collect();
    let types = args.iter().map(|arg| &arg.ty).collect();
    let optional = args.iter().filter(|arg| arg.optional).map(|arg| {
        let ty = &arg.ty;
        quote! {
            const _: () = ::std::assert!(
                <#ty as #api::typed::ArgType>::UNDEFINED_IS_DEFAULT,
                "an optional argument's type converts undefined to its default",
            );
        }
    });
    (names, types, quote!(#(#optional)*))
}

/// A method's call: its arguments counted and taken first, then in an
/// operation, `this`, the arguments, the function, its result.
#[expect(
    clippy::too_many_arguments,
    reason = "a method's parts and its table attributes, each read once where it is expanded"
)]
fn method(
    iface: &Ident,
    name: &Ident,
    args: &[Arg],
    ret: &Type,
    function: &Path,
    with_front: bool,
    ce_reactions: bool,
    is_static: bool,
    realm: bool,
    legacy_factory: bool,
) -> Tokens {
    let api = api();
    let (front, op, cx, this_, result, value, error) = locals();
    // A legacy factory function is called as a constructor (of its
    // interface, whose object it makes), and may ask for its realm's
    // window in the operation.
    let checked = if legacy_factory {
        let prepare_window = with_front.then(|| quote!(#front.prepare_window();));
        let required = args.iter().take_while(|arg| !arg.optional).count();
        quote! {
            if !#front.construct(<#iface as #api::declare::Interface>::INDEX, #required) {
                return;
            }
            #prepare_window
        }
    } else {
        let js = js_name(name);
        let required = args.iter().take_while(|arg| !arg.optional).count();
        // A static operation with the front end has no `this` to find its
        // realm's window by: the current realm's found before it
        // (`URL.createObjectURL`'s origin).
        let prepare_window = if realm {
            Some(quote!(#front.prepare_relevant_window();))
        } else {
            (is_static && with_front).then(|| quote!(#front.prepare_window();))
        };
        quote! {
            if !#front.has_args(#js, #required) {
                return;
            }
            #prepare_window
        }
    };
    // A static one has no `this`.
    let (this, this_arg) = if is_static {
        (quote!(), quote!())
    } else {
        let this = this(iface);
        (quote!(let #this_ = #this;), quote!(, #this_))
    };
    let front_arg = with_front.then(|| quote!(#op,));
    let (names, types, optional) = args_of(args);
    let indices = 0..args.len();
    let param_indices = 0..args.len();
    let op_body = ce_scope(
        ce_reactions,
        &cx,
        &result,
        quote! {
            #this
            #(
                let #names = <#types as #api::typed::ArgType>::param(#op, #cx, &#names, #param_indices)?;
            )*
            let #value = #function(#front_arg #cx #this_arg #(, #names)*)?;
            ::std::result::Result::Ok(
                <#ret as #api::typed::RetType>::give_prepared(
                    #op,
                    #cx,
                    ::std::convert::Into::into(#value),
                    prepared,
                )
            )
        },
    );
    let ce_invoke = ce_invoke(ce_reactions, &front, &result);
    quote! {
        type Args = (#(#types,)*);
        type Ret = #ret;

        fn call<F: #api::typed::Front>(#front: &mut F) {
            #optional
            #checked
            #(
                let ::std::option::Option::Some(#names) = <#types as #api::typed::ArgType>::take(#front, #indices) else {
                    return;
                };
            )*
            // What an argument foresaw of the result (`createElement`'s
            // name: its element's interface), for what is made ahead.
            let foreseen = ::std::option::Option::<usize>::None
                #(.or(<#types as #api::typed::ArgType>::foreseen_interface::<F>(&#names)))*;
            let prepared = <#ret as #api::typed::RetType>::prepare(#front, foreseen);
            let #result = #front.mutate(|#cx, #op| {
                #op_body
            });
            // Before the engine may allocate again (`finish`, `throw`): a
            // string argument may lend its own characters.
            #(#api::typed::release(#names);)*
            #ce_invoke
            match #result {
                ::std::result::Result::Ok(#value) => {
                    <#ret as #api::typed::RetType>::finish(#front, #value);
                    #front.finish();
                }
                ::std::result::Result::Err(#error) => <#ret as #api::typed::RetType>::fail(#front, #error),
            }
        }
    }
}

/// A call of a method that may leave script to do (`#[then_script]`): its
/// arguments taken, then in an operation, the arguments and the function,
/// what it leaves rooted (`typed::Root`); that done out of any operation
/// (`typed::ThenScript`), with the front end as its script host, before
/// the call returns undefined.
fn then_script_method(
    iface: &Ident,
    name: &Ident,
    args: &[Arg],
    function: &Path,
    front_arg: bool,
) -> Tokens {
    let api = api();
    let (front, op, cx, this_, result, value, error) = locals();
    let this = this(iface);
    let js = js_name(name);
    let required = args.iter().take_while(|arg| !arg.optional).count();
    let (names, types, optional) = args_of(args);
    let indices = 0..args.len();
    let param_indices = 0..args.len();
    let call = if front_arg {
        quote!(#function(#op, #cx, #this_ #(, #names)*))
    } else {
        quote!(#function(#cx, #this_ #(, #names)*))
    };
    quote! {
        type Args = (#(#types,)*);
        type Ret = ();
        // On the front end that runs script (`Front::whole`), whichever
        // the member's callback is given.

        fn call<F: #api::typed::Front>(#front: &mut F) {
            struct Whole;

            impl #api::typed::WholeCall for Whole {
                type Out = ();

                fn run<F: #api::typed::Front>(self, #front: &mut F) {
                    whole(#front);
                }
            }

            #front.whole(Whole);

            fn whole<F: #api::typed::Front>(#front: &mut F) {
            #optional
            if !#front.has_args(#js, #required) {
                return;
            }
            #(
                let ::std::option::Option::Some(#names) = <#types as #api::typed::ArgType>::take(#front, #indices) else {
                    return;
                };
            )*
            let #result = #front.mutate(|#cx, #op| {
                let #this_ = #this;
                #(
                    let #names = <#types as #api::typed::ArgType>::param(#op, #cx, &#names, #param_indices)?;
                )*
                let #value = #call?;
                ::std::result::Result::Ok(#value.map(|#value| #api::typed::Root::root(#value, #cx)))
            });
            #(#api::typed::release(#names);)*
            match #result {
                ::std::result::Result::Ok(::std::option::Option::None) => #front.finish(),
                ::std::result::Result::Ok(::std::option::Option::Some(#value)) => {
                    #api::typed::ThenScript::run(#value, &mut #api::typed::FrontHost(&mut *#front));
                    #front.finish();
                }
                ::std::result::Result::Err(#error) => <() as #api::typed::RetType>::fail(#front, #error),
            }
            }
        }
    }
}

/// A call of a method that calls script (`#[calls_script]`): its
/// arguments taken, then in an operation, `this` and the arguments rooted
/// (`typed::Root`); its function out of any operation, with the front end
/// as its script host, opening its own; its result given in an operation
/// of its own.
fn script_method(
    iface: &Ident,
    name: &Ident,
    args: &[Arg],
    ret: &Type,
    function: &Path,
    ce_reactions: bool,
    first: Option<&Path>,
) -> Tokens {
    let api = api();
    let (front, op, cx, this_, result, value, error) = locals();
    let this = this(iface);
    let js = js_name(name);
    let required = args.iter().take_while(|arg| !arg.optional).count();
    let (names, types, optional) = args_of(args);
    let indices = 0..args.len();
    let param_indices = 0..args.len();
    // The function tried first (`#[first(..)]`): one operation on the
    // front end the callback is given, as a member that calls no script
    // has; the call is over if it gave the result, and goes on to the
    // whole front end (which takes the arguments again) if it gave none.
    let tried = first.map(|first| {
        let (indices, param_indices) = (0..args.len(), 0..args.len());
        quote! {
            {
                #optional
                if !#front.has_args(#js, #required) {
                    return;
                }
                #(
                    let ::std::option::Option::Some(#names) = <#types as #api::typed::ArgType>::take(#front, #indices) else {
                        return;
                    };
                )*
                let #result = #front.mutate(|#cx, #op| {
                    let #this_ = #this;
                    #(
                        let #names = <#types as #api::typed::ArgType>::param(#op, #cx, &#names, #param_indices)?;
                    )*
                    ::std::result::Result::Ok(
                        #first(#cx, #this_ #(, #names)*)?
                            .map(|#value| <#ret as #api::typed::RetType>::give(#op, #cx, #value)),
                    )
                });
                #(#api::typed::release(#names);)*
                match #result {
                    ::std::result::Result::Ok(::std::option::Option::Some(#value)) => {
                        <#ret as #api::typed::RetType>::finish(#front, #value);
                        #front.finish();
                        return;
                    }
                    ::std::result::Result::Ok(::std::option::Option::None) => {}
                    ::std::result::Result::Err(#error) => {
                        #front.throw(#error);
                        return;
                    }
                }
            }
        }
    });
    // `this` and the arguments are rooted in an operation of their own:
    // one that only reads when there is `this` alone to root (a root
    // changes nothing of the DOM's; an argument's `param` may store).
    let rooting = if args.is_empty() {
        quote!(read)
    } else {
        quote!(mutate)
    };
    // Its scope in operations of their own, around its function's.
    let (ce_enter, ce_leave_invoke) = if ce_reactions {
        (
            quote!(#front.mutate(|#cx, _| #cx.enter_ce_reactions());),
            quote! {
                if #front.mutate(|#cx, _| #cx.leave_ce_reactions()) {
                    #front.invoke_ce_reactions();
                }
            },
        )
    } else {
        (quote!(), quote!())
    };
    quote! {
        type Args = (#(#types,)*);
        type Ret = #ret;
        // On the front end that runs script (`Front::whole`), whichever
        // the member's callback is given.

        fn call<F: #api::typed::Front>(#front: &mut F) {
            struct Whole;

            impl #api::typed::WholeCall for Whole {
                type Out = ();

                fn run<F: #api::typed::Front>(self, #front: &mut F) {
                    whole(#front);
                }
            }

            #tried
            #front.whole(Whole);

            fn whole<F: #api::typed::Front>(#front: &mut F) {
            #optional
            if !#front.has_args(#js, #required) {
                return;
            }
            #(
                let ::std::option::Option::Some(#names) = <#types as #api::typed::ArgType>::take(#front, #indices) else {
                    return;
                };
            )*
            #ce_enter
            let #result = #front.#rooting(|#cx, #op| {
                let #this_ = #this;
                #(
                    let #names = <#types as #api::typed::ArgType>::param(#op, #cx, &#names, #param_indices)?;
                )*
                ::std::result::Result::Ok((
                    #api::typed::Root::root(#this_, #cx),
                    #(#api::typed::Root::root(#names, #cx),)*
                ))
            });
            #(#api::typed::release(#names);)*
            let (#this_, #(#names,)*) = match #result {
                ::std::result::Result::Ok(#value) => #value,
                ::std::result::Result::Err(#error) => {
                    #ce_leave_invoke
                    #front.throw(#error);
                    return;
                }
            };
            match #function(&mut #api::typed::FrontHost(&mut *#front), #this_ #(, #names)*) {
                ::std::result::Result::Ok(#value) => {
                    let #value = #front.read(|#cx, #op| {
                        <#ret as #api::typed::RetType>::give(#op, #cx, #api::typed::Unroot::unroot(#value, #cx))
                    });
                    #ce_leave_invoke
                    <#ret as #api::typed::RetType>::finish(#front, #value);
                    #front.finish();
                }
                ::std::result::Result::Err(#error) => {
                    #ce_leave_invoke
                    #front.throw(#error);
                }
            }
            }
        }
    }
}

/// A constructor's call: checked as one and its arguments counted, its
/// arguments taken, then in an operation, the arguments, the function, and
/// its new object given to the call's own new wrapper.
fn constructor(iface: &Ident, args: &[Arg], function: &Path, with_front: bool) -> Tokens {
    let api = api();
    let (front, op, cx, _, result, value, error) = locals();
    let front_arg = with_front.then(|| quote!(#op,));
    // Its function may ask for the realm's window in the operation.
    let prepare_window = with_front.then(|| quote!(#front.prepare_window();));
    let required = args.iter().take_while(|arg| !arg.optional).count();
    let (names, types, optional) = args_of(args);
    let indices = 0..args.len();
    let param_indices = 0..args.len();
    quote! {
        type Args = (#(#types,)*);
        type Ret = ();

        fn call<F: #api::typed::Front>(#front: &mut F) {
            #optional
            if !#front.construct(<#iface as #api::declare::Interface>::INDEX, #required) {
                return;
            }
            #(
                let ::std::option::Option::Some(#names) = <#types as #api::typed::ArgType>::take(#front, #indices) else {
                    return;
                };
            )*
            #prepare_window
            let #result = #front.mutate(|#cx, #op| {
                #(
                    let #names = <#types as #api::typed::ArgType>::param(#op, #cx, &#names, #param_indices)?;
                )*
                let #value = #function(#front_arg #cx #(, #names)*)?;
                ::std::result::Result::Ok(#api::typed::Constructed::construct(#value, #op, #cx))
            });
            #(#api::typed::release(#names);)*
            match #result {
                ::std::result::Result::Ok(#value) => {
                    #api::typed::finish_constructed(#front, #value);
                    #front.finish()
                }
                ::std::result::Result::Err(#error) => #front.throw(#error),
            }
        }
    }
}

/// A dictionary: its struct, its defaults, and its conversion as an
/// argument, its members read in their names' order (WebIDL's), before the
/// operation.
fn dictionary_items(dictionary: &DictionaryDef) -> Tokens {
    let api = api();
    let DictionaryDef {
        attrs,
        name,
        members,
    } = dictionary;
    let (front, index, taken, value, dict) = (
        local("front"),
        local("index"),
        local("taken"),
        local("value"),
        local("dict"),
    );
    // A member whose type names the front end's (`T::Node`, `T::Any`) is
    // one of its handles: the dictionary is then of those types (its
    // parameters, `TNode`, `TAny`), which a front end and its side in
    // the operation share.
    let mut kinds: Vec<Ident> = Vec::new();
    for member in members {
        fronts_types(member.ty.to_token_stream(), &mut kinds);
    }
    let handle = |member: &DictMemberDef| {
        let mut kinds = Vec::new();
        fronts_types(member.ty.to_token_stream(), &mut kinds);
        !kinds.is_empty()
    };
    let of_front = !kinds.is_empty();
    let fields = members.iter().map(|member| {
        let (name, ty) = (&member.name, of_parameters(member.ty.to_token_stream()));
        quote!(pub #name: #ty)
    });
    let defaults = members.iter().map(|member| {
        let (name, default) = (&member.name, &member.default);
        quote!(#name: #default)
    });
    let mut ordered: Vec<&DictMemberDef> = members.iter().collect();
    ordered.sort_by_key(|member| member.name.unraw().to_string());
    let reads = ordered.iter().map(|member| {
        let field = &member.name;
        let js = js_name(field);
        let read = if handle(member) {
            quote!(#api::typed::read_handle)
        } else {
            quote!(#api::typed::read_member)
        };
        quote! {
            #read(
                #front,
                #dict,
                const { #api::typed::MemberName::of(#js) },
                &mut #value.#field,
            )?;
        }
    });
    // As a result: its members with a value, in their names' order too.
    let outs = ordered.iter().map(|member| {
        let (field, ty) = (&member.name, &member.ty);
        let js = js_name(field);
        quote! {
            if let ::std::option::Option::Some(#taken) =
                <#ty as #api::typed::DictMember>::out(&#value.#field)
            {
                #dict.push((#js, #taken));
            }
        }
    });
    let name_str = LitStr::new(&name.to_string(), name.span());
    // The dictionary as a type: of the front end `T` in scope, of any,
    // and its declaration's parameters.
    let (of_t, of_any, params, impl_params) = if of_front {
        let parameters: Vec<Ident> = kinds
            .iter()
            .map(|kind| format_ident!("T{}", kind))
            .collect();
        (
            quote!(#name<#(T::#kinds),*>),
            quote!(#name<#(#parameters),*>),
            // Named with none where no front end is meant (the table's
            // types: an argument's type names the dictionary).
            quote!(<#(#parameters = ::std::convert::Infallible),*>),
            quote!(<#(#parameters),*>),
        )
    } else {
        (quote!(#name), quote!(#name), quote!(), quote!())
    };
    let common = quote! {
        #(#attrs)*
        #[derive(Debug, Clone, PartialEq)]
        pub struct #name #params {
            #(#fields,)*
        }

        impl #impl_params ::std::default::Default for #of_any {
            fn default() -> Self {
                Self {
                    #(#defaults,)*
                }
            }
        }

        impl #api::typed::Dictionary for #name {
            const NAME: &'static str = #name_str;
            type Of<T: #api::typed::FrontTypes> = #of_t;

            fn read<T: #api::typed::Front>(#front: &mut T, #index: usize) -> ::std::option::Option<#of_t> {
                let mut #value = <#of_t as ::std::default::Default>::default();
                if let ::std::option::Option::Some(#dict) = #front.dict_arg(#index, #name_str)? {
                    // Its members' reads, which the front end runs on
                    // the one that reads them (it may make one for them
                    // all).
                    struct Reads<'d, T: #api::typed::FrontTypes>(&'d T::Dict, &'d mut #of_t);

                    impl<'d, T: #api::typed::FrontTypes> #api::typed::ReadDict<T> for Reads<'d, T> {
                        type Out = ::std::option::Option<()>;

                        fn read<F>(self, #front: &mut F) -> ::std::option::Option<()>
                        where
                            F: #api::typed::DictFront<
                                    Str = T::Str,
                                    Node = T::Node,
                                    Dict = T::Dict,
                                    Any = T::Any,
                                >,
                        {
                            let Reads(#dict, #value) = self;
                            #(#reads)*
                            ::std::option::Option::Some(())
                        }
                    }

                    #front.reading(Reads::<T>(&#dict, &mut #value))?;
                }
                ::std::option::Option::Some(#value)
            }
        }

        impl #api::typed::ArgType for #name {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::Dictionary(#name_str);
            const UNDEFINED_IS_DEFAULT: bool = true;
            type Taken<T: #api::typed::FrontTypes> = #of_t;
            type Param<'a, 'gc, T: #api::typed::FrontTypes + 'a> = &'a #of_t;

            fn take<T: #api::typed::Front>(#front: &mut T, #index: usize) -> ::std::option::Option<#of_t> {
                <#name as #api::typed::Dictionary>::read(#front, #index)
            }

            fn param<'a, 'gc, T: #api::typed::OpFront + 'a>(
                _: &mut T,
                _: &#api::ApiCx<'_, 'gc>,
                #taken: &'a #of_t,
                _: usize,
            ) -> ::std::result::Result<&'a #of_t, #api::ApiError> {
                ::std::result::Result::Ok(#taken)
            }
        }
    };
    // A result gives values: not a dictionary of a front end's handles.
    if of_front {
        return common;
    }
    quote! {
        #common

        impl #api::typed::RetType for #name {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::Dictionary(#name_str);
            type Ret<'gc> = #name;

            type Prepared<F: #api::typed::FrontTypes> = ();

            #[inline(always)]
            fn prepare<F: #api::typed::Front>(_: &mut F, _: ::std::option::Option<usize>) {}

            // Its members' values, which leave the operation as they
            // are: the front end makes the object after it.
            type Pending<F: #api::typed::FrontTypes> =
                ::std::vec::Vec<(&'static str, #api::typed::DictValue)>;

            #[inline(always)]
            fn finish<F: #api::typed::Front>(#front: &mut F, #dict: Self::Pending<F>) {
                #front.finish_dictionary(#dict);
            }

            fn give<'gc, O: #api::typed::OpFront>(
                _: &mut O,
                _: &#api::ApiCx<'_, 'gc>,
                #value: #name,
            ) -> Self::Pending<O> {
                let mut #dict = ::std::vec::Vec::new();
                #(#outs)*
                #dict
            }
        }
    }
}

/// The front end's types a type's tokens name (`T::Node`: `Node`), each
/// once, added to `kinds`.
fn fronts_types(tokens: Tokens, kinds: &mut Vec<Ident>) {
    let _ = rewrite_fronts_types(tokens, &mut |kind| {
        if !kinds.contains(kind) {
            kinds.push(kind.clone());
        }
    });
}

/// A type's tokens with the front end's types they name made a
/// dictionary's parameters (`T::Node`: `TNode`).
fn of_parameters(tokens: Tokens) -> Tokens {
    rewrite_fronts_types(tokens, &mut |_| {})
}

fn rewrite_fronts_types(tokens: Tokens, found: &mut dyn FnMut(&Ident)) -> Tokens {
    use proc_macro2::TokenTree;
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out = Vec::with_capacity(tokens.len());
    let mut at = 0;
    while at < tokens.len() {
        // `T`, `:`, `:`, a name.
        if let (
            TokenTree::Ident(t),
            Some(TokenTree::Punct(first)),
            Some(TokenTree::Punct(second)),
            Some(TokenTree::Ident(kind)),
        ) = (
            &tokens[at],
            tokens.get(at + 1),
            tokens.get(at + 2),
            tokens.get(at + 3),
        ) && t == "T"
            && first.as_char() == ':'
            && second.as_char() == ':'
        {
            found(kind);
            out.push(TokenTree::Ident(format_ident!("T{}", kind)));
            at += 4;
            continue;
        }
        out.push(match &tokens[at] {
            TokenTree::Group(group) => {
                let mut rewritten = proc_macro2::Group::new(
                    group.delimiter(),
                    rewrite_fronts_types(group.stream(), found),
                );
                rewritten.set_span(group.span());
                TokenTree::Group(rewritten)
            }
            token => token.clone(),
        });
        at += 1;
    }
    out.into_iter().collect()
}

/// The calls' locals, hygienic.
fn locals() -> (Ident, Ident, Ident, Ident, Ident, Ident, Ident) {
    (
        local("front"),
        local("op"),
        local("cx"),
        local("this"),
        local("result"),
        local("value"),
        local("error"),
    )
}

/// An interface whose objects aren't nodes as an argument and a result
/// (nullable or not): its object type's.
fn object_types(name: &Ident) -> Tokens {
    let api = api();
    let (front, cx, value, index, taken) = (
        local("front"),
        local("cx"),
        local("value"),
        local("index"),
        local("taken"),
    );
    let object = quote!(<<#name as #api::declare::Interface>::Object as #api::typed::ObjectType>);
    // What an object result leaves for after the operation, finished.
    let pending_object = quote! {
        type Prepared<F: #api::typed::FrontTypes> = ();

        #[inline(always)]
        fn prepare<F: #api::typed::Front>(_: &mut F, _: ::std::option::Option<usize>) {}

        type Pending<F: #api::typed::FrontTypes> = ::std::option::Option<F::PendingObject>;

        #[inline(always)]
        fn finish<F: #api::typed::Front>(
            front: &mut F,
            pending: ::std::option::Option<F::PendingObject>,
        ) {
            if let ::std::option::Option::Some(pending) = pending {
                front.finish_object(pending);
            }
        }
    };
    quote! {
        impl #api::typed::ArgType for #name {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::Node(stringify!(#name));
            type Taken<T: #api::typed::FrontTypes> = T::Node;
            type Param<'a, 'gc, T: #api::typed::FrontTypes + 'a> = #object::Ref<'gc>;

            fn take<F: #api::typed::Front>(#front: &mut F, #index: usize) -> ::std::option::Option<F::Node> {
                #api::typed::take_object::<#name, F>(#front, #index)
            }

            fn param<'a, 'gc, O: #api::typed::OpFront + 'a>(
                #front: &mut O,
                #cx: &#api::ApiCx<'_, 'gc>,
                #taken: &'a O::Node,
                _: usize,
            ) -> ::std::result::Result<#object::Ref<'gc>, #api::ApiError> {
                ::std::result::Result::Ok(#object::arg(#front, #cx, #taken))
            }
        }

        impl #api::typed::ArgType for ::std::option::Option<#name> {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::NullableNode(stringify!(#name));
            // WebIDL's nullable types convert undefined to null.
            const UNDEFINED_IS_DEFAULT: bool = true;
            type Taken<T: #api::typed::FrontTypes> = ::std::option::Option<T::Node>;
            type Param<'a, 'gc, T: #api::typed::FrontTypes + 'a> = ::std::option::Option<#object::Ref<'gc>>;

            fn take<F: #api::typed::Front>(
                #front: &mut F,
                #index: usize,
            ) -> ::std::option::Option<::std::option::Option<F::Node>> {
                #object::take_arg(#front, #index, stringify!(#name))
            }

            fn param<'a, 'gc, O: #api::typed::OpFront + 'a>(
                #front: &mut O,
                #cx: &#api::ApiCx<'_, 'gc>,
                #taken: &'a ::std::option::Option<O::Node>,
                _: usize,
            ) -> ::std::result::Result<::std::option::Option<#object::Ref<'gc>>, #api::ApiError> {
                ::std::result::Result::Ok(#taken.as_ref().map(|#taken| #object::arg(#front, #cx, #taken)))
            }
        }

        impl #api::typed::RetType for #name {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::Node(stringify!(#name));
            type Ret<'gc> = #object::Ref<'gc>;

            #pending_object

            fn give<'gc, O: #api::typed::OpFront>(
                #front: &mut O,
                #cx: &#api::ApiCx<'_, 'gc>,
                #value: Self::Ret<'gc>,
            ) -> ::std::option::Option<O::PendingObject> {
                #api::typed::give_object::<#name, O>(#front, #cx, #value)
            }
        }

        impl #api::typed::RetType for ::std::option::Option<#name> {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::NullableNode(stringify!(#name));
            type Ret<'gc> = ::std::option::Option<#object::Ref<'gc>>;

            #pending_object

            fn give<'gc, O: #api::typed::OpFront>(
                #front: &mut O,
                #cx: &#api::ApiCx<'_, 'gc>,
                #value: Self::Ret<'gc>,
            ) -> ::std::option::Option<O::PendingObject> {
                match #value {
                    ::std::option::Option::Some(#value) => #api::typed::give_object::<#name, O>(#front, #cx, #value),
                    ::std::option::Option::None => {
                        #front.ret_null();
                        ::std::option::Option::None
                    }
                }
            }
        }
    }
}

/// A callback interface: its name, and its nullable form as an argument
/// (the front end's handle, `Callback`).
fn callback_items(callback: &CallbackDef) -> Tokens {
    let api = api();
    let CallbackDef { attrs, name, .. } = callback;
    let (front, index, taken) = (local("front"), local("index"), local("taken"));
    let name_str = LitStr::new(&name.to_string(), name.span());
    quote! {
        #(#attrs)*
        pub struct #name;

        impl #api::typed::ArgType for ::std::option::Option<#name> {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::Callback(#name_str);
            // WebIDL's nullable types convert undefined to null (`callback_arg`
            // takes it as none).
            const UNDEFINED_IS_DEFAULT: bool = true;
            type Taken<T: #api::typed::FrontTypes> = ::std::option::Option<#api::typed::Callback<T::Callback>>;
            type Param<'a, 'gc, T: #api::typed::FrontTypes + 'a> = ::std::option::Option<#api::typed::Callback<T::Callback>>;

            fn take<F: #api::typed::Front>(
                #front: &mut F,
                #index: usize,
            ) -> ::std::option::Option<::std::option::Option<#api::typed::Callback<F::Callback>>> {
                #front.callback_arg(#index, #name_str)
            }

            fn param<'a, 'gc, O: #api::typed::OpFront + 'a>(
                _: &mut O,
                _: &#api::ApiCx<'_, 'gc>,
                #taken: &'a ::std::option::Option<#api::typed::Callback<O::Callback>>,
                _: usize,
            ) -> ::std::result::Result<::std::option::Option<#api::typed::Callback<O::Callback>>, #api::ApiError> {
                ::std::result::Result::Ok(*#taken)
            }
        }
    }
}

/// An interface as an argument and a result, nullable or not.
fn node_types(name: &Ident) -> Tokens {
    let api = api();
    let (front, cx, value, index) = (local("front"), local("cx"), local("value"), local("index"));
    let taken = local("taken");
    let node = local("node");
    // What a node result leaves for after the operation, finished.
    let pending_node = quote! {
        type Prepared<F: #api::typed::FrontTypes> = ();

        #[inline(always)]
        fn prepare<F: #api::typed::Front>(_: &mut F, _: ::std::option::Option<usize>) {}

        type Pending<F: #api::typed::FrontTypes> = ::std::option::Option<F::PendingNode>;

        #[inline(always)]
        fn finish<F: #api::typed::Front>(
            front: &mut F,
            pending: ::std::option::Option<F::PendingNode>,
        ) {
            if let ::std::option::Option::Some(pending) = pending {
                front.finish_node(pending);
            }
        }
    };
    quote! {
        impl #api::typed::ArgType for #name {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::Node(stringify!(#name));
            type Taken<T: #api::typed::FrontTypes> = T::Node;
            type Param<'a, 'gc, T: #api::typed::FrontTypes + 'a> = ::dom::NodeRef<'gc>;

            fn take<F: #api::typed::Front>(#front: &mut F, #index: usize) -> ::std::option::Option<F::Node> {
                #api::typed::take_node::<#name, F>(#front, #index)
            }

            fn param<'a, 'gc, O: #api::typed::OpFront + 'a>(
                #front: &mut O,
                #cx: &#api::ApiCx<'_, 'gc>,
                #taken: &'a O::Node,
                #index: usize,
            ) -> ::std::result::Result<::dom::NodeRef<'gc>, #api::ApiError> {
                #api::typed::node_param::<#name, O>(#front, #cx, #taken, #index)
            }
        }

        impl #api::typed::RetType for #name {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::Node(stringify!(#name));
            type Ret<'gc> = ::dom::NodeRef<'gc>;

            #pending_node

            fn give<'gc, O: #api::typed::OpFront>(
                #front: &mut O,
                #cx: &#api::ApiCx<'_, 'gc>,
                #value: ::dom::NodeRef<'gc>,
            ) -> ::std::option::Option<O::PendingNode> {
                #front.ret_node(#cx, #value)
            }
        }

        impl #api::typed::ArgType for ::std::option::Option<#name> {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::NullableNode(stringify!(#name));
            // WebIDL's nullable types convert undefined to null (`node_arg`
            // takes it as none).
            const UNDEFINED_IS_DEFAULT: bool = true;
            type Taken<T: #api::typed::FrontTypes> = ::std::option::Option<T::Node>;
            type Param<'a, 'gc, T: #api::typed::FrontTypes + 'a> = ::std::option::Option<::dom::NodeRef<'gc>>;

            fn take<F: #api::typed::Front>(
                #front: &mut F,
                #index: usize,
            ) -> ::std::option::Option<::std::option::Option<F::Node>> {
                #front.node_arg(#index)
            }

            fn param<'a, 'gc, O: #api::typed::OpFront + 'a>(
                #front: &mut O,
                #cx: &#api::ApiCx<'_, 'gc>,
                #taken: &'a ::std::option::Option<O::Node>,
                #index: usize,
            ) -> ::std::result::Result<::std::option::Option<::dom::NodeRef<'gc>>, #api::ApiError> {
                #taken
                    .as_ref()
                    .map(|#node| #api::typed::node_param::<#name, O>(#front, #cx, #node, #index))
                    .transpose()
            }
        }

        impl #api::typed::RetType for ::std::option::Option<#name> {
            const DESC: #api::declare::TypeDesc = #api::declare::TypeDesc::NullableNode(stringify!(#name));
            type Ret<'gc> = ::std::option::Option<::dom::NodeRef<'gc>>;

            #pending_node

            fn give<'gc, O: #api::typed::OpFront>(
                #front: &mut O,
                #cx: &#api::ApiCx<'_, 'gc>,
                #value: ::std::option::Option<::dom::NodeRef<'gc>>,
            ) -> ::std::option::Option<O::PendingNode> {
                match #value {
                    ::std::option::Option::Some(#node) => #front.ret_node(#cx, #node),
                    ::std::option::Option::None => {
                        #front.ret_null();
                        ::std::option::Option::None
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Table, check, expand};

    /// The error `declare!` reports for `table`.
    fn error(table: &str) -> String {
        match syn::parse_str::<Table>(table).and_then(|table| expand(&table)) {
            Ok(_) => panic!("expanded: {table}"),
            Err(error) => error
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        }
    }

    #[test]
    fn a_table_expands() {
        let table = "interface Node [] {
            const ELEMENT_NODE: u32 = 1;
            attribute nodeType: u32, get = node::node_type;
            attribute textContent: Option<DomString>, get = node::text_content, set = node::set_text_content;
            method appendChild(node: Node) -> Node = node::append_child;
            method normalize() = node::normalize;
        }
        interface Element : Node [Element] {}";
        let tokens = expand(&syn::parse_str::<Table>(table).unwrap())
            .unwrap()
            .to_string();
        // Each typed path at its index among the members, constants counted;
        // the setter after its getter.
        for row in [
            "$ cb ! (Node , nodeType , Getter , :: dom_api :: interfaces :: typed_paths :: Node :: M < 1usize > , plain)",
            "$ cb ! (Node , textContent , Getter , :: dom_api :: interfaces :: typed_paths :: Node :: M < 2usize > , plain)",
            "$ cb ! (Node , textContent , Setter , :: dom_api :: interfaces :: typed_paths :: Node :: M < 3usize > , plain)",
            "$ cb ! (Node , appendChild , Method , :: dom_api :: interfaces :: typed_paths :: Node :: M < 4usize > , plain)",
            "$ cb ! (Node , normalize , Method , :: dom_api :: interfaces :: typed_paths :: Node :: M < 5usize > , plain)",
        ] {
            assert!(tokens.contains(row), "no {row}");
        }
        assert!(!tokens.contains("ELEMENT_NODE , Getter"));
        assert!(
            tokens.contains("const INDEX : usize = 1usize"),
            "Element's index"
        );
        // A method without a result returns `()`.
        assert!(tokens.contains("type Ret = () ;"));
    }

    #[test]
    fn a_parent_is_declared_before_its_children() {
        assert_eq!(
            error("interface Element : Node [Element] {} interface Node [] {}"),
            "no interface `Node` is declared before this one"
        );
    }

    #[test]
    fn an_interface_is_declared_once() {
        assert_eq!(
            error("interface Node [] {} interface Node [] {}"),
            "`Node` is declared twice"
        );
        // A dictionary's name is one too.
        assert_eq!(
            error("interface Node [] {} dictionary Node {}"),
            "`Node` is declared twice"
        );
    }

    /// A name is unique among an interface's members of its kind: a getter
    /// and a method may share one, two getters (or a getter and an
    /// attribute with a setter) can't; every error is reported.
    #[test]
    fn a_member_is_declared_once_of_its_kind() {
        let table = "interface Node [] {
            attribute a: u32, get = f;
            method a() = f;
            attribute b: u32, get = f;
            attribute b: u32, get = f, set = g;
            const C: u32 = 1;
            const C: u32 = 2;
        }";
        assert_eq!(
            error(table),
            "`b` is declared twice in `Node`; `C` is declared twice in `Node`"
        );
    }

    #[test]
    fn a_constant_is_a_u32() {
        assert_eq!(
            error("interface Node [] { const A: u16 = 1; }"),
            "a constant is a `u32`"
        );
    }

    #[test]
    fn a_member_is_a_constant_an_attribute_or_a_method() {
        assert_eq!(
            error("interface Node [] { operation f() = f; }"),
            "expected one of: `const`, `constructor`, `attribute`, `method`"
        );
    }

    /// An interface of another type's objects, a constructor (a node
    /// interface's too), a member taking a dictionary or giving another
    /// interface's object (nested in a type too): not plain. A raw name is the table's without
    /// `r#`; a dictionary's members are read in their names' order.
    #[test]
    fn objects_constructors_and_dictionaries_expand() {
        let table = "interface Event [type = dom::Event] {
            constructor(r#type: DomString, init?: EventInit) = event::construct;
            attribute r#type: DomString, get = event::event_type;
        }
        dictionary EventInit { composed: bool = false, bubbles: bool = true }
        interface Node [] {
            constructor() = node::construct;
            method m(init?: EventInit) = node::m;
            method n(event: Event) = node::n;
            method o() -> Option<Event> = node::o;
            method plain(name: DomString) -> u32 = node::plain;
        }";
        let tokens = expand(&syn::parse_str::<Table>(table).unwrap())
            .unwrap()
            .to_string();
        for row in [
            "$ cb ! (Event , constructor , Constructor , :: dom_api :: interfaces :: typed_paths :: Event :: M < 0usize > , object)",
            "$ cb ! (Event , r#type , Getter , :: dom_api :: interfaces :: typed_paths :: Event :: M < 1usize > , object)",
            "$ cb ! (Node , constructor , Constructor , :: dom_api :: interfaces :: typed_paths :: Node :: M < 0usize > , object)",
            "$ cb ! (Node , m , Method , :: dom_api :: interfaces :: typed_paths :: Node :: M < 1usize > , object)",
            "$ cb ! (Node , n , Method , :: dom_api :: interfaces :: typed_paths :: Node :: M < 2usize > , object)",
            "$ cb ! (Node , o , Method , :: dom_api :: interfaces :: typed_paths :: Node :: M < 3usize > , object)",
            "$ cb ! (Node , plain , Method , :: dom_api :: interfaces :: typed_paths :: Node :: M < 4usize > , plain)",
        ] {
            assert!(tokens.contains(row), "no {row}");
        }
        assert!(tokens.contains("const NAME : & 'static str = \"type\""));
        assert!(
            tokens.contains("node : false"),
            "Event's objects aren't nodes"
        );
        assert!(tokens.contains("type Object = dom :: Event"));
        assert!(tokens.contains("type Object = :: dom_api :: typed :: NodeObject"));
        // An optional argument's type is checked, and isn't required.
        assert!(tokens.contains("UNDEFINED_IS_DEFAULT"));
        assert!(tokens.contains(
            "construct (< Event as :: dom_api :: declare :: Interface > :: INDEX , 1usize)"
        ));
        // The dictionary's struct, its defaults, its members read by name.
        assert!(
            tokens.contains("pub struct EventInit { pub composed : bool , pub bubbles : bool , }")
        );
        assert!(tokens.contains("composed : false , bubbles : true"));
        let bubbles = tokens.find("\"bubbles\")").unwrap();
        let composed = tokens.find("\"composed\")").unwrap();
        assert!(bubbles < composed, "read in their names' order");
    }

    /// `#[unforgeable]` on an attribute (both its getter and setter) and a
    /// method; `#[front]` on a constructor hands its function the front end's
    /// side in the operation, and asks for the realm's window before it.
    #[test]
    fn member_attributes_expand() {
        let table = "interface W [type = W] {
            #[front]
            constructor() = w::construct;
            #[unforgeable]
            attribute a: u32, get = w::a, set = w::set_a;
            attribute b: u32, get = w::b;
            #[unforgeable]
            method m() = w::m;
        }";
        let tokens = expand(&syn::parse_str::<Table>(table).unwrap())
            .unwrap()
            .to_string();
        assert_eq!(
            tokens.matches("unforgeable : true").count(),
            3,
            "a's getter and setter, m"
        );
        assert_eq!(
            tokens.matches("unforgeable : false").count(),
            2,
            "the constructor, b"
        );
        assert!(tokens.contains("w :: construct (op , cx)"), "{tokens}");
        assert!(tokens.contains("front . prepare_window ()"), "{tokens}");
    }

    /// `#[calls_script]` on an attribute: its getter's function runs out
    /// of the operation, with the front end as its script host; its
    /// setter's in one, as any, with the front end too if `#[front]`.
    #[test]
    fn an_attribute_may_call_script() {
        let table = "interface W [type = W] {
            #[calls_script]
            #[front]
            attribute a: u32, get = w::a, set = w::set_a;
            #[calls_script]
            attribute b: u32, get = w::b, set = w::set_b;
        }";
        let tokens = expand(&syn::parse_str::<Table>(table).unwrap())
            .unwrap()
            .to_string();
        assert!(
            tokens.contains("w :: a (& mut :: dom_api :: typed :: FrontHost"),
            "{tokens}"
        );
        assert!(tokens.contains("w :: set_a (op , cx , this"), "{tokens}");
        assert!(tokens.contains("w :: set_b (cx , this"), "{tokens}");
    }

    /// `#[ce_reactions]`: the scope entered and left in the operation (a
    /// method's, a setter's) or in operations of their own (a method
    /// calling script), its queue invoked; a member not marked has none.
    #[test]
    fn ce_reactions_scope_marked_members() {
        let table = "interface W [type = W] {
            #[ce_reactions]
            method m() = w::m;
            #[ce_reactions]
            attribute a: u32, get = w::a, set = w::set_a;
            #[ce_reactions]
            #[calls_script]
            method s() = w::s;
            method plain() = w::plain;
            attribute b: u32, get = w::b, set = w::set_b;
        }";
        let tokens = expand(&syn::parse_str::<Table>(table).unwrap())
            .unwrap()
            .to_string();
        // The method's and the setter's by a guard in their operation; the
        // one calling script's in operations of its own (entered once,
        // left at each of its three ends).
        assert_eq!(
            tokens.matches("CeReactionsScope :: enter").count(),
            2,
            "{tokens}"
        );
        assert_eq!(tokens.matches("scope . leave ()").count(), 2, "{tokens}");
        assert_eq!(tokens.matches("enter_ce_reactions").count(), 1, "{tokens}");
        assert_eq!(tokens.matches("leave_ce_reactions").count(), 3, "{tokens}");
        // After the guarded operations, both ends; and the three.
        assert_eq!(
            tokens.matches("invoke_ce_reactions").count(),
            2 * 2 + 3,
            "{tokens}"
        );
    }

    #[test]
    fn member_attributes_go_where_they_may() {
        assert_eq!(
            error("interface W [type = W] { #[unforgeable] constructor() = f; }"),
            "`#[front]` marks a method, a constructor or an attribute, `#[calls_script]` a method or an attribute, `#[then_script]` a method, `#[unforgeable]` a method or an attribute, `#[indexed]` a method, `#[ce_reactions]` a method or an attribute, `#[static_method]` a method or an attribute, `#[pairs]` a method, `#[replaceable]` an attribute, `#[realm]` a method"
        );
        assert_eq!(
            error("interface W [type = W] { #[inline] method m() = f; }"),
            "a member's attribute is `#[front]`, `#[calls_script]`, `#[then_script]`, `#[unforgeable]`, `#[indexed]`, `#[ce_reactions]`, `#[static_method]`, `#[pairs]`, `#[replaceable]` or `#[realm]`"
        );
        assert_eq!(
            error("interface W [type = W] { #[calls_script] constructor() = f; }"),
            "`#[front]` marks a method, a constructor or an attribute, `#[calls_script]` a method or an attribute, `#[then_script]` a method, `#[unforgeable]` a method or an attribute, `#[indexed]` a method, `#[ce_reactions]` a method or an attribute, `#[static_method]` a method or an attribute, `#[pairs]` a method, `#[replaceable]` an attribute, `#[realm]` a method"
        );
        assert_eq!(
            error("interface W [type = W] { #[indexed] attribute a: u32, get = f; }"),
            "`#[front]` marks a method, a constructor or an attribute, `#[calls_script]` a method or an attribute, `#[then_script]` a method, `#[unforgeable]` a method or an attribute, `#[indexed]` a method, `#[ce_reactions]` a method or an attribute, `#[static_method]` a method or an attribute, `#[pairs]` a method, `#[replaceable]` an attribute, `#[realm]` a method"
        );
        assert_eq!(
            error("interface W [type = W] { #[realm] attribute a: u32, get = f; }"),
            "`#[front]` marks a method, a constructor or an attribute, `#[calls_script]` a method or an attribute, `#[then_script]` a method, `#[unforgeable]` a method or an attribute, `#[indexed]` a method, `#[ce_reactions]` a method or an attribute, `#[static_method]` a method or an attribute, `#[pairs]` a method, `#[replaceable]` an attribute, `#[realm]` a method"
        );
        assert_eq!(
            error("interface W [type = W] { #[then_script] attribute a: u32, get = f; }"),
            "`#[front]` marks a method, a constructor or an attribute, `#[calls_script]` a method or an attribute, `#[then_script]` a method, `#[unforgeable]` a method or an attribute, `#[indexed]` a method, `#[ce_reactions]` a method or an attribute, `#[static_method]` a method or an attribute, `#[pairs]` a method, `#[replaceable]` an attribute, `#[realm]` a method"
        );
        for marks in ["#[calls_script]", "#[ce_reactions]", "#[static_method]"] {
            assert_eq!(
                error(&format!(
                    "interface W [type = W] {{ #[then_script] {marks} method m() = f; }}"
                )),
                "a method that leaves script to do is none of `#[calls_script]`, \
                 `#[ce_reactions]` and `#[static_method]`",
                "{marks}"
            );
        }
        assert_eq!(
            error("interface W [type = W] { #[then_script] method m() -> u32 = f; }"),
            "a method that leaves script to do gives nothing"
        );
        assert_eq!(
            error("interface W [type = W] { #[cross_origin] constructor() = f; }"),
            "`#[cross_origin]` marks a method or an attribute"
        );
        assert_eq!(
            error("interface W [type = W] { #[cross_origin(setter)] method m() = f; }"),
            "a cross-origin method is `#[cross_origin]`"
        );
        assert_eq!(
            error("interface W [type = W] { #[cross_origin(deleter)] attribute a: u32, get = f; }"),
            "a cross-origin attribute's accessors are `getter` and `setter`"
        );
        for attribute in [
            "#[cross_origin(setter)] attribute a: u32, get = f;",
            "#[static_method] #[cross_origin] attribute a: u32, get = f;",
        ] {
            assert_eq!(
                error(&format!("interface W [type = W] {{ {attribute} }}")),
                "a cross-origin attribute is a regular one, its setter one it has",
                "{attribute}"
            );
        }
        for marks in [
            "#[static_method]",
            "#[pairs]",
            "#[indexed]",
            "#[named(getter)]",
            "#[legacy_factory]",
        ] {
            assert_eq!(
                error(&format!(
                    "interface W [type = W] {{ {marks} #[cross_origin] method m() = f; }}"
                )),
                "a cross-origin method is a regular one",
                "{marks}"
            );
        }
    }

    /// Each typed path is visited by its kind's `TypedVisitor` method alone.
    #[test]
    fn typed_paths_are_visited_by_kind() {
        let table = "interface W [type = W] {
            attribute a: u32, get = w::a, set = w::set_a;
            #[cross_origin(setter)]
            attribute b: u32, get = w::b, set = w::set_b;
            #[cross_origin]
            method c() = w::c;
            #[static_method]
            method d() = w::d;
            #[indexed(getter)]
            method e(index: u32) -> u32 = w::e;
        }";
        let tokens = expand(&syn::parse_str::<Table>(table).unwrap())
            .unwrap()
            .to_string();
        for visit in [
            "visitor . getter :: < M < 0usize >> ()",
            "visitor . setter :: < M < 1usize >> ()",
            "visitor . getter :: < M < 2usize >> ()",
            "visitor . cross_origin :: < M < 3usize >> ()",
            "visitor . cross_origin :: < M < 4usize >> ()",
            "visitor . static_member :: < M < 5usize >> ()",
            "visitor . indexed :: < M < 6usize >> ()",
        ] {
            assert!(tokens.contains(visit), "{visit}: {tokens}");
        }
    }

    /// `#[then_script]`: its function in the one operation, what it leaves
    /// rooted there and done after it, through the front end as a script
    /// host, before the call finishes.
    #[test]
    fn then_script_leaves_script_to_do() {
        let table = "interface W [type = W] {
            #[then_script]
            method m(n: u32) = w::m;
        }";
        let tokens = expand(&syn::parse_str::<Table>(table).unwrap())
            .unwrap()
            .to_string();
        assert_eq!(tokens.matches(". mutate (").count(), 1, "{tokens}");
        assert!(tokens.contains("w :: m (cx , this , arg0)"), "{tokens}");
        assert!(
            tokens.contains("typed :: Root :: root (value , cx)"),
            "{tokens}"
        );
        assert!(
            tokens.contains("typed :: ThenScript :: run (value , & mut"),
            "{tokens}"
        );
    }

    /// `#[indexed]` marks the method its description says is the indexed
    /// getter; an element interface (`[element html]`, `[element html
    /// meta]`) is a node interface of no kinds, its elements described,
    /// its members not plain; an SVG one (`[element svg r#use]`) too, its
    /// name unraw; another namespace is an error.
    #[test]
    fn indexed_getters_and_element_interfaces_expand() {
        let table = "interface Node [] {}
        interface Element : Node [Element] {}
        interface HTMLElement : Element [element html] {}
        interface HTMLMetaElement : HTMLElement [element html meta] {
            attribute content: DomString, get = h::content;
        }
        interface SVGElement : Element [element svg] {}
        interface SVGUseElement : SVGElement [element svg r#use] {}
        interface C [type = C] {
            #[indexed]
            method item(index: u32) -> Option<Element> = c::item;
            method other(index: u32) -> Option<Element> = c::other;
        }";
        let tokens = expand(&syn::parse_str::<Table>(table).unwrap())
            .unwrap()
            .to_string();
        assert_eq!(tokens.matches("indexed : true").count(), 1, "{tokens}");
        // `#[indexed(getter)]`: the indexed getter, its name empty in its
        // description and its typed path; another is an error.
        let anonymous = expand(
            &syn::parse_str::<Table>(
                "interface C [type = C] {
                    #[indexed(getter)]
                    method getter(index: u32) -> u32 = c::item;
                }",
            )
            .unwrap(),
        )
        .unwrap()
        .to_string();
        assert!(
            anonymous
                .contains("name : \"\" , kind : :: dom_api :: declare :: MemberKind :: Method"),
            "{anonymous}"
        );
        assert!(anonymous.contains("indexed : true"), "{anonymous}");
        assert!(
            anonymous.contains("const NAME : & 'static str = \"\" ;"),
            "{anonymous}"
        );
        assert_eq!(
            error("interface C [type = C] { #[indexed(deleter)] method s(index: u32) = c::s; }"),
            "an anonymous indexed special operation is `#[indexed(getter)]` or `#[indexed(setter)]`"
        );
        assert_eq!(
            error("interface C [type = C] { #[indexed(getter)] attribute a: u32, get = c::a; }"),
            "`#[indexed(..)]` marks a method"
        );
        // `#[indexed(setter)]`: the indexed setter, anonymous;
        // `#[legacy_factory]`: a static method of its own kind.
        let special = expand(
            &syn::parse_str::<Table>(
                "interface C [type = C] {
                    #[indexed(setter)]
                    method s(index: u32, value: u32) = c::s;
                    #[legacy_factory]
                    method Make() -> u32 = c::make;
                }",
            )
            .unwrap(),
        )
        .unwrap()
        .to_string();
        assert!(special.contains("MemberKind :: IndexedSetter"), "{special}");
        assert!(special.contains("MemberKind :: LegacyFactory"), "{special}");
        assert!(
            special.contains("construct"),
            "a legacy factory checks its call: {special}"
        );
        assert!(tokens.contains("element : :: std :: option :: Option :: Some (:: dom_api :: declare :: ElementDesc { svg : false , locals : & [] , })"));
        assert!(tokens.contains("locals : & [\"meta\"]"));
        assert!(
            tokens.contains("ElementDesc { svg : true , locals : & [\"use\"] , }"),
            "{tokens}"
        );
        assert!(tokens.contains("$ cb ! (HTMLMetaElement , content , Getter , :: dom_api :: interfaces :: typed_paths :: HTMLMetaElement :: M < 0usize > , object)"), "{tokens}");
        assert_eq!(
            error("interface Element [Element] {} interface M : Element [element mathml] {}"),
            "an element interface's are HTML or SVG elements (`html`, `svg`)"
        );
    }

    #[test]
    fn an_optional_argument_comes_last() {
        assert_eq!(
            error("interface E [type = E] { method m(a?: D, b: D) = f; }"),
            "a required argument after an optional one"
        );
    }

    /// A method and the constructor may be declared again (overloads),
    /// each form its own member.
    #[test]
    fn methods_and_constructors_overload() {
        let table: Table = syn::parse_str(
            "interface E [type = E] {
                constructor() = f; constructor(a: D) = g;
                method m() = f; method m(a: D, b: D) = g;
            }",
        )
        .unwrap();
        check(&table).unwrap();
        expand(&table).unwrap();
        assert_eq!(table.interfaces[0].members.len(), 4);
    }
}
