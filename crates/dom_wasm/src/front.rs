//! A member's call from a guest: a typed host function of its own
//! (wasmtime's `func_wrap`), its arguments and result of the member's own
//! types (`TypedMember::Args`, `Ret`, as `for_each_member!` names each
//! member), and wasm's side of the call (`dom_api::Front`).
//!
//! Before the DOM's operation, with no DOM: `this` and each node argument
//! is a handle of the store's (else the call traps: only a binding's bug
//! makes one that isn't), `this` of the member's interface (else a
//! `TypeError`, as JS's receiver check: a guest has one kind of handle),
//! and each string argument read from the guest's memory, checked UTF-8,
//! in place. Then the member's typed path: in its operation, the nodes from
//! the store's table, the result kept (a node's handle given then, the
//! table being the DOM's), and after it wasmtime lowers the result (a
//! string copied into the guest's memory from the DOM's own buffer).

use std::borrow::Cow;
use std::cell::RefCell;

use dom::{Dom, DomString, NodeRef};
use dom_api::interfaces::{
    Attr, CDATASection, CharacterData, Comment, Document, DocumentFragment, DocumentType, Element,
    HTMLElement, HTMLSlotElement, Node, ProcessingInstruction, Text, XMLDocument,
};
use dom_api::typed::LegacyNullToEmptyString;
use dom_api::{
    ApiCx, ApiError, ArgType, ExceptionName, Front, FrontTypes, NewElement, NewObject, OpFront,
    RetType, TypedMember,
};
use wasmtime::component::{
    ComponentNamedList, ComponentType, Lift, LinkerInstance, Lower, WasmStr,
};
use wasmtime::{AsContextMut, StoreContextMut};

use crate::handles::{Handles, Tables};
use crate::{STATE, WasmStore};

/// `record dom-exception { code, message }`.
#[derive(ComponentType, Lift, Lower)]
#[component(record)]
pub(crate) struct DomException {
    code: u32,
    message: String,
}

/// The `code` of a `TypeError`: `this` of another interface.
pub const TYPE_ERROR: u32 = 1;

/// The `code` of a `DOMException` name, fixed, so that a guest compares
/// against the same numbers.
pub fn exception_code(name: ExceptionName) -> u32 {
    match name {
        ExceptionName::HierarchyRequest => 2,
        ExceptionName::NotFound => 3,
        ExceptionName::InvalidCharacter => 4,
        ExceptionName::InvalidState => 5,
        ExceptionName::InvalidAccess => 17,
        ExceptionName::Syntax => 6,
        ExceptionName::Namespace => 9,
        ExceptionName::NoModificationAllowed => 11,
        ExceptionName::NotSupported => 7,
        ExceptionName::NotAllowed => 24,
        ExceptionName::IndexSize => 8,
        ExceptionName::Encoding => 19,
        ExceptionName::Security => 15,
        ExceptionName::DataClone => 16,
        ExceptionName::InUseAttribute => 10,
        ExceptionName::InvalidNodeType => 22,
        ExceptionName::WrongDocument => 23,
        ExceptionName::Abort => 12,
        ExceptionName::TypeMismatch => 20,
        ExceptionName::QuotaExceeded => 21,
        ExceptionName::InvalidModification => 25,
        ExceptionName::Timeout => 26,
    }
}

impl DomException {
    fn of(error: ApiError) -> Self {
        match error {
            ApiError::Type(message) => Self {
                code: TYPE_ERROR,
                message,
            },
            // Today's `machopo:dom`'s code of a `RangeError`.
            ApiError::Range(message) => Self { code: 18, message },
            ApiError::Dom(exception) => Self {
                code: exception_code(exception.name),
                message: exception.message,
            },
        }
    }
}

/// A trap: a handle that isn't one of the store's.
fn not_held(handle: u32) -> wasmtime::Error {
    wasmtime::format_err!("{handle:#x} is no handle of this store")
}

/// `Node`, which every node's interface is or descends from.
const NODE: usize = dom_api::declare::interface_index("Node");

/// A node argument's handle checked: one of the store's, and a node's
/// (which of the node interfaces it must be is the member's to check, in
/// its operation). A trap otherwise: a binding's mistake.
fn check_node(handle: u32, handles: &Handles) -> wasmtime::Result<()> {
    let of = handles.interface(handle).ok_or_else(|| not_held(handle))?;
    if dom_api::typed::is_interface(NODE, of) {
        Ok(())
    } else {
        Err(wasmtime::format_err!(
            "{handle:#x} is a {}'s handle, not a node's",
            dom_api::INTERFACES[of].name
        ))
    }
}

/// An argument's type of the table as wasm's: what wasmtime lifts, and what
/// the call reads of it before the operation.
pub(crate) trait WitArg: ArgType {
    type Wit: ComponentType + Lift + 'static;
    type Read<'s>: ReadArg;

    /// Whether it is a handle of the store's, if it is one.
    fn check(wit: &Self::Wit, handles: &Handles) -> wasmtime::Result<()> {
        let _ = (wit, handles);
        Ok(())
    }

    fn read<'s, T: 'static>(
        wit: &Self::Wit,
        store: &'s StoreContextMut<'_, T>,
    ) -> wasmtime::Result<Self::Read<'s>>;
}

/// What a read argument gives the front end: `None` if it is of another
/// type (none of the table's arguments asks for it).
pub(crate) trait ReadArg {
    /// How many of the call's arguments it is: one, but any number of
    /// strings.
    fn count(&self) -> usize {
        1
    }
    /// The string `offset` arguments after its first.
    fn string_at(&self, offset: usize) -> Option<&str> {
        let _ = offset;
        None
    }
    fn nullable_string(&self) -> Option<Option<&str>> {
        None
    }
    fn node(&self) -> Option<Option<u32>> {
        None
    }
    fn unsigned_long(&self) -> Option<u32> {
        None
    }
    /// A `boolean`, or an optional one's: none if it wasn't given.
    fn boolean(&self) -> Option<Option<bool>> {
        None
    }
}

impl ReadArg for Cow<'_, str> {
    fn string_at(&self, offset: usize) -> Option<&str> {
        (offset == 0).then_some(self)
    }
}

/// Any number of strings (`classList.add`'s tokens): the call's last
/// arguments.
impl ReadArg for Vec<Cow<'_, str>> {
    fn count(&self) -> usize {
        self.len()
    }

    fn string_at(&self, offset: usize) -> Option<&str> {
        self.get(offset).map(|string| &**string)
    }
}

impl WitArg for dom_api::typed::Strings {
    type Wit = Vec<WasmStr>;
    type Read<'s> = Vec<Cow<'s, str>>;

    fn read<'s, T: 'static>(
        wit: &Vec<WasmStr>,
        store: &'s StoreContextMut<'_, T>,
    ) -> wasmtime::Result<Vec<Cow<'s, str>>> {
        wit.iter().map(|wit| read_string(wit, store)).collect()
    }
}

impl ReadArg for Option<Cow<'_, str>> {
    fn nullable_string(&self) -> Option<Option<&str>> {
        Some(self.as_deref())
    }
}

impl ReadArg for u32 {
    fn node(&self) -> Option<Option<u32>> {
        Some(Some(*self))
    }
}

impl ReadArg for Option<u32> {
    fn node(&self) -> Option<Option<u32>> {
        Some(*self)
    }
}

/// A `double`: none of the frozen members takes one (`wit::FROZEN`), so
/// none registers; its type is the table's all the same.
impl ReadArg for f64 {}

impl WitArg for f64 {
    type Wit = f64;
    type Read<'s> = f64;

    fn read<T: 'static>(wit: &f64, _: &StoreContextMut<'_, T>) -> wasmtime::Result<f64> {
        Ok(*wit)
    }
}

/// An `unrestricted double` (`scrollTop`'s): as a `double`.
impl WitArg for dom_api::typed::UnrestrictedDouble {
    type Wit = f64;
    type Read<'s> = f64;

    fn read<T: 'static>(wit: &f64, _: &StoreContextMut<'_, T>) -> wasmtime::Result<f64> {
        Ok(*wit)
    }
}

/// An `unsigned long` (an index). Read as its own type: a `u32` read is
/// an object's handle.
pub(crate) struct UnsignedLong(u32);

impl ReadArg for UnsignedLong {
    fn unsigned_long(&self) -> Option<u32> {
        Some(self.0)
    }
}

impl WitArg for u32 {
    type Wit = u32;
    type Read<'s> = UnsignedLong;

    fn read<T: 'static>(wit: &u32, _: &StoreContextMut<'_, T>) -> wasmtime::Result<UnsignedLong> {
        Ok(UnsignedLong(*wit))
    }
}

impl ReadArg for bool {
    fn boolean(&self) -> Option<Option<bool>> {
        Some(Some(*self))
    }
}

impl WitArg for bool {
    type Wit = bool;
    type Read<'s> = bool;

    fn read<T: 'static>(wit: &bool, _: &StoreContextMut<'_, T>) -> wasmtime::Result<bool> {
        Ok(*wit)
    }
}

/// An optional `boolean` with no default: the guest's none.
impl ReadArg for Option<bool> {
    fn boolean(&self) -> Option<Option<bool>> {
        Some(*self)
    }
}

impl WitArg for Option<bool> {
    type Wit = Option<bool>;
    type Read<'s> = Option<bool>;

    fn read<T: 'static>(
        wit: &Option<bool>,
        _: &StoreContextMut<'_, T>,
    ) -> wasmtime::Result<Option<bool>> {
        Ok(*wit)
    }
}

/// A string read in place from the guest's memory (it is UTF-8, its
/// encoding, so no copy), checked.
fn read_string<'s, T: 'static>(
    wit: &WasmStr,
    store: &'s StoreContextMut<'_, T>,
) -> wasmtime::Result<Cow<'s, str>> {
    wit.to_str(store)
}

impl WitArg for DomString {
    type Wit = WasmStr;
    type Read<'s> = Cow<'s, str>;

    fn read<'s, T: 'static>(
        wit: &WasmStr,
        store: &'s StoreContextMut<'_, T>,
    ) -> wasmtime::Result<Cow<'s, str>> {
        read_string(wit, store)
    }
}

/// `createElement`'s local name: a string.
impl WitArg for dom_api::typed::ElementLocalName {
    type Wit = WasmStr;
    type Read<'s> = Cow<'s, str>;

    fn read<'s, T: 'static>(
        wit: &WasmStr,
        store: &'s StoreContextMut<'_, T>,
    ) -> wasmtime::Result<Cow<'s, str>> {
        read_string(wit, store)
    }
}

/// `DOMString?`, and `[LegacyNullToEmptyString] DOMString`: `option<string>`.
macro_rules! nullable_strings {
    ($($ty:ty),*) => {$(
        impl WitArg for $ty {
            type Wit = Option<WasmStr>;
            type Read<'s> = Option<Cow<'s, str>>;

            fn read<'s, T: 'static>(
                wit: &Option<WasmStr>,
                store: &'s StoreContextMut<'_, T>,
            ) -> wasmtime::Result<Option<Cow<'s, str>>> {
                wit.as_ref().map(|wit| read_string(wit, store)).transpose()
            }
        }
    )*};
}

nullable_strings!(Option<DomString>, LegacyNullToEmptyString);

/// The table's interfaces: a node is a handle (`u32`), nullable or not;
/// as a result, a handle the call gives. Each interface is listed: an
/// interface added to the table is added here (a member of it doesn't
/// register until it is).
macro_rules! interfaces {
    ($($interface:ty),*) => {$(
        impl WitArg for $interface {
            type Wit = u32;
            type Read<'s> = u32;

            fn check(wit: &u32, handles: &Handles) -> wasmtime::Result<()> {
                check_node(*wit, handles)
            }

            fn read<T: 'static>(wit: &u32, _: &StoreContextMut<'_, T>) -> wasmtime::Result<u32> {
                Ok(*wit)
            }
        }

        impl WitArg for Option<$interface> {
            type Wit = Option<u32>;
            type Read<'s> = Option<u32>;

            fn check(wit: &Option<u32>, handles: &Handles) -> wasmtime::Result<()> {
                match *wit {
                    Some(handle) => check_node(handle, handles),
                    None => Ok(()),
                }
            }

            fn read<T: 'static>(wit: &Option<u32>, _: &StoreContextMut<'_, T>) -> wasmtime::Result<Option<u32>> {
                Ok(*wit)
            }
        }

        impl WitRet for $interface {
            type Wit = u32;
            fn wit(ret: Ret) -> u32 {
                match ret {
                    Ret::Node(handle) => handle,
                    other => other.unexpected(),
                }
            }
        }

        impl WitRet for Option<$interface> {
            type Wit = Option<u32>;
            fn wit(ret: Ret) -> Option<u32> {
                match ret {
                    Ret::Node(handle) => Some(handle),
                    Ret::Null => None,
                    other => other.unexpected(),
                }
            }
        }

        // Whichever interface its wrapper would be made ahead as: a guest's
        // handle is the node's.
        impl<L: dom_api::Interface> WitRet for NewObject<$interface, L> {
            type Wit = u32;
            fn wit(ret: Ret) -> u32 {
                match ret {
                    Ret::Node(handle) => handle,
                    other => other.unexpected(),
                }
            }
        }
    )*};
}

// `createElement`'s: a guest's document defines no custom elements (it
// has no registry of its own), so its element is made in the operation.
impl WitRet for dom_api::typed::NewHtmlElement {
    type Wit = u32;
    fn wit(ret: Ret) -> u32 {
        match ret {
            Ret::Node(handle) => handle,
            other => other.unexpected(),
        }
    }
}

impl<L: dom_api::Interface> WitRet for NewElement<L> {
    type Wit = u32;
    fn wit(ret: Ret) -> u32 {
        match ret {
            Ret::Node(handle) => handle,
            other => other.unexpected(),
        }
    }
}

/// The interfaces whose objects aren't nodes, as results: a handle the
/// call gives, as a node's. (None is an argument yet.)
macro_rules! objects {
    ($($interface:ty),*) => {$(
        impl WitRet for $interface {
            type Wit = u32;
            fn wit(ret: Ret) -> u32 {
                match ret {
                    Ret::Node(handle) => handle,
                    other => other.unexpected(),
                }
            }
        }

        impl WitRet for Option<$interface> {
            type Wit = Option<u32>;
            fn wit(ret: Ret) -> Option<u32> {
                match ret {
                    Ret::Node(handle) => Some(handle),
                    Ret::Null => None,
                    other => other.unexpected(),
                }
            }
        }
    )*};
}

objects!(
    dom_api::interfaces::EventTarget,
    dom_api::interfaces::Window,
    dom_api::interfaces::Location,
    dom_api::interfaces::DOMTokenList,
    dom_api::interfaces::CSSStyleProperties,
    dom_api::interfaces::HTMLCollection,
    dom_api::interfaces::NodeList
);

interfaces!(
    Node,
    Document,
    DocumentType,
    DocumentFragment,
    Element,
    CharacterData,
    Text,
    Comment,
    ProcessingInstruction,
    Attr,
    CDATASection,
    XMLDocument,
    HTMLElement,
    HTMLSlotElement
);

/// A member's arguments as wasm's: `this`'s handle first, then each one's
/// (`ComponentNamedList`); read, the front end takes them by index.
pub(crate) trait WitArgs {
    type Wit: ComponentNamedList + Lift + 'static;
    type Rest;
    type Read<'s>: ReadArgs;

    fn split(wit: Self::Wit) -> (u32, Self::Rest);
    fn check(rest: &Self::Rest, handles: &Handles) -> wasmtime::Result<()>;
    fn read<'s, T: 'static>(
        rest: &Self::Rest,
        store: &'s StoreContextMut<'_, T>,
    ) -> wasmtime::Result<Self::Read<'s>>;
}

/// The read arguments, by index: each a constant where the call asks, so
/// its `match` folds.
pub(crate) trait ReadArgs {
    /// How many arguments the call has (any number of strings count each).
    fn count(&self) -> usize;
    fn string(&self, index: usize) -> Option<&str>;
    fn nullable_string(&self, index: usize) -> Option<Option<&str>>;
    fn node(&self, index: usize) -> Option<Option<u32>>;
    fn unsigned_long(&self, index: usize) -> Option<u32>;
    fn boolean(&self, index: usize) -> Option<Option<bool>>;
}

macro_rules! wit_args {
    ($(($($t:ident $v:ident $i:tt),*))*) => {$(
        impl<$($t: WitArg),*> WitArgs for ($($t,)*) {
            type Wit = (u32, $($t::Wit,)*);
            type Rest = ($($t::Wit,)*);
            type Read<'s> = ($($t::Read<'s>,)*);

            fn split((this, $($v,)*): Self::Wit) -> (u32, Self::Rest) {
                (this, ($($v,)*))
            }

            #[allow(unused_variables)]
            fn check(rest: &Self::Rest, handles: &Handles) -> wasmtime::Result<()> {
                $($t::check(&rest.$i, handles)?;)*
                Ok(())
            }

            #[allow(unused_variables, clippy::unused_unit)]
            fn read<'s, T: 'static>(rest: &Self::Rest, store: &'s StoreContextMut<'_, T>) -> wasmtime::Result<Self::Read<'s>> {
                Ok(($($t::read(&rest.$i, store)?,)*))
            }
        }

        impl<$($t: ReadArg),*> ReadArgs for ($($t,)*) {
            fn count(&self) -> usize {
                0usize $(.saturating_add(self.$i.count()))*
            }

            // The argument at `index`, or one of the strings that start
            // before it (the last arguments, any number of them).
            #[allow(unused_variables)]
            fn string(&self, index: usize) -> Option<&str> {
                $(
                    if let Some(string) = index
                        .checked_sub($i)
                        .and_then(|offset| self.$i.string_at(offset))
                    {
                        return Some(string);
                    }
                )*
                None
            }

            #[allow(unused_variables)]
            fn unsigned_long(&self, index: usize) -> Option<u32> {
                match index {
                    $($i => self.$i.unsigned_long(),)*
                    _ => None,
                }
            }

            #[allow(unused_variables)]
            fn boolean(&self, index: usize) -> Option<Option<bool>> {
                match index {
                    $($i => self.$i.boolean(),)*
                    _ => None,
                }
            }

            #[allow(unused_variables)]
            fn nullable_string(&self, index: usize) -> Option<Option<&str>> {
                match index {
                    $($i => self.$i.nullable_string(),)*
                    _ => None,
                }
            }

            #[allow(unused_variables)]
            fn node(&self, index: usize) -> Option<Option<u32>> {
                match index {
                    $($i => self.$i.node(),)*
                    _ => None,
                }
            }
        }
    )*};
}

wit_args! {
    ()
    (A a 0)
    (A a 0, B b 1)
    (A a 0, B b 1, C c 2)
}

/// `focus(options?)`: its dictionary may be left out, and the guest's
/// function has no such parameter (`wit::can_hold`): `this` alone, the
/// options their defaults.
impl WitArgs for (dom_api::interfaces::FocusOptions,) {
    type Wit = (u32,);
    type Rest = ();
    type Read<'s> = ();

    fn split((this,): Self::Wit) -> (u32, ()) {
        (this, ())
    }

    fn check((): &(), _: &Handles) -> wasmtime::Result<()> {
        Ok(())
    }

    fn read<T: 'static>((): &(), _: &StoreContextMut<'_, T>) -> wasmtime::Result<()> {
        Ok(())
    }
}

/// A call's result, kept in its operation.
pub(crate) enum Ret {
    None,
    Undefined,
    Null,
    Bool(bool),
    U32(u32),
    I32(i32),
    F64(f64),
    Str(Box<str>),
    Node(u32),
}

impl Ret {
    fn unexpected(&self) -> ! {
        let kind = match self {
            Self::None => "nothing",
            Self::Undefined => "undefined",
            Self::Null => "null",
            Self::Bool(_) => "a bool",
            Self::U32(_) => "a u32",
            Self::I32(_) => "an i32",
            Self::F64(_) => "an f64",
            Self::Str(_) => "a string",
            Self::Node(_) => "a node",
        };
        unreachable!("a member's typed path gave {kind}, not its type's")
    }
}

/// A result's type of the table as wasm's: what wasmtime lowers, from the
/// kept result.
pub(crate) trait WitRet: RetType {
    type Wit: ComponentType + Lower + 'static;
    fn wit(ret: Ret) -> Self::Wit;
}

/// Nothing: a method that returns `undefined`, or a setter, which gives no
/// result (V8's is `undefined` until set).
impl WitRet for () {
    type Wit = ();
    fn wit(ret: Ret) {
        match ret {
            Ret::None | Ret::Undefined => {}
            other => other.unexpected(),
        }
    }
}

impl WitRet for bool {
    type Wit = bool;
    fn wit(ret: Ret) -> bool {
        match ret {
            Ret::Bool(value) => value,
            other => other.unexpected(),
        }
    }
}

impl WitRet for u32 {
    type Wit = u32;
    fn wit(ret: Ret) -> u32 {
        match ret {
            Ret::U32(value) => value,
            other => other.unexpected(),
        }
    }
}

/// WebIDL's `unsigned short`: given as a `u32` (`RetType for u16`).
impl WitRet for u16 {
    type Wit = u16;
    fn wit(ret: Ret) -> u16 {
        match ret {
            Ret::U32(value) => u16::try_from(value).expect("an unsigned short's value"),
            other => other.unexpected(),
        }
    }
}

/// WebIDL's `long`.
impl WitRet for i32 {
    type Wit = i32;
    fn wit(ret: Ret) -> i32 {
        match ret {
            Ret::I32(value) => value,
            other => other.unexpected(),
        }
    }
}

impl WitRet for f64 {
    type Wit = f64;
    fn wit(ret: Ret) -> f64 {
        match ret {
            Ret::F64(value) => value,
            other => other.unexpected(),
        }
    }
}

impl WitRet for dom_api::typed::UnrestrictedDouble {
    type Wit = f64;
    fn wit(ret: Ret) -> f64 {
        match ret {
            Ret::F64(value) => value,
            other => other.unexpected(),
        }
    }
}

/// `DOMString`, and a `[LegacyNullToEmptyString]` attribute's getter: a copy
/// of the DOM's string, which wasmtime copies into the guest.
macro_rules! strings {
    ($($ty:ty),*) => {$(
        impl WitRet for $ty {
            type Wit = Box<str>;
            fn wit(ret: Ret) -> Box<str> {
                match ret {
                    Ret::Str(value) => value,
                    other => other.unexpected(),
                }
            }
        }
    )*};
}

strings!(DomString, LegacyNullToEmptyString);

impl WitRet for Option<DomString> {
    type Wit = Option<Box<str>>;
    fn wit(ret: Ret) -> Option<Box<str>> {
        match ret {
            Ret::Str(value) => Some(value),
            Ret::Null => None,
            other => other.unexpected(),
        }
    }
}

/// What a member's host function returns: its `result`.
type Returns<M> = (Result<<<M as TypedMember>::Ret as WitRet>::Wit, DomException>,);

/// Registers member `M` as the function `name`, for stores whose
/// `WasmStore` `store` finds.
pub(crate) fn add<T, M>(
    instance: &mut LinkerInstance<'_, T>,
    name: &str,
    store: fn(&T) -> &WasmStore,
) -> wasmtime::Result<()>
where
    T: 'static,
    M: TypedMember,
    M::Args: WitArgs,
    M::Ret: WitRet,
{
    instance.func_wrap(
        name,
        move |mut cx: StoreContextMut<'_, T>, params: <M::Args as WitArgs>::Wit| {
            let returns = call::<T, M>(cx.as_context_mut(), params, store)?;
            crate::collect_if_due(&mut cx)?;
            Ok(returns)
        },
    )
}

/// One call of `M` from the guest.
fn call<T: 'static, M>(
    cx: StoreContextMut<'_, T>,
    params: <M::Args as WitArgs>::Wit,
    store: fn(&T) -> &WasmStore,
) -> wasmtime::Result<Returns<M>>
where
    M: TypedMember,
    M::Args: WitArgs,
    M::Ret: WitRet,
{
    let key = store(cx.data()).key;
    let number = key.number;
    STATE.with_borrow(|slot| {
        let state = slot.of(key).ok_or_else(crate::no_store)?;
        let (this, rest) = <M::Args as WitArgs>::split(params);
        // `this`'s interface, as its handle was given: the member's, or a
        // descendant of it (an element's by its name), with no DOM.
        let of = {
            let tables = state.tables.borrow();
            let handles = tables.store(number).ok_or_else(crate::no_store)?;
            let of = handles.interface(this).ok_or_else(|| not_held(this))?;
            <M::Args as WitArgs>::check(&rest, handles)?;
            of
        };
        if !dom_api::typed::is_interface(M::INTERFACE, of) {
            return Ok((Err(DomException::of(ApiError::type_error(format!(
                "Illegal invocation: a {} is not a {}",
                dom_api::INTERFACES[of].name,
                dom_api::INTERFACES[M::INTERFACE].name
            )))),));
        }
        let args = <M::Args as WitArgs>::read(&rest, &cx)?;
        let mut front = WasmFront {
            dom: state.dom(),
            layout: &state.layout,
            tables: &state.tables,
            store: number,
            this,
            args: &args,
            ret: Ret::None,
            error: None,
        };
        M::call(&mut front);
        Ok((match front.error {
            Some(error) => Err(DomException::of(error)),
            None => Ok(<M::Ret as WitRet>::wit(front.ret)),
        },))
    })
}

/// wasm's side of one call: the DOM, the store's table, `this`'s handle,
/// the read arguments, and what the call gave. Its own side in an
/// operation (`OpFront`).
struct WasmFront<'r, A> {
    dom: &'r RefCell<Dom>,
    /// The page's layout, if the DOM was lent with one.
    layout: &'r dom_api::LayoutCell,
    tables: &'r RefCell<Tables>,
    store: u32,
    this: u32,
    args: &'r A,
    ret: Ret,
    error: Option<ApiError>,
}

impl<'r, A> FrontTypes for WasmFront<'r, A> {
    type Str = &'r str;
    type Node = u32;
    // No dictionary is plain: no member taking one is bound.
    type Dict = ();
}

impl<'r, A: ReadArgs> Front for WasmFront<'r, A> {
    type Op<'o>
        = WasmFront<'r, A>
    where
        Self: 'o;

    fn read<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let (dom, layout) = (self.dom, self.layout);
        dom.borrow()
            .read(|cx| f(&ApiCx::with_layout_cell(cx, layout), self))
    }

    fn mutate<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let (dom, layout) = (self.dom, self.layout);
        dom.borrow_mut()
            .mutate(|cx| f(&ApiCx::with_layout_cell(cx, layout), self))
    }

    // A typed function's arguments are counted by its type.
    fn has_args(&mut self, _: &str, _: usize) -> bool {
        true
    }

    fn string_arg(&mut self, index: usize) -> Option<&'r str> {
        let args: &'r A = self.args;
        Some(args.string(index).expect("a string argument"))
    }

    fn arg_count(&mut self) -> usize {
        self.args.count()
    }

    fn u32_arg(&mut self, index: usize) -> Option<u32> {
        Some(self.args.unsigned_long(index).expect("an unsigned long"))
    }

    fn bool_arg(&mut self, index: usize) -> bool {
        self.args
            .boolean(index)
            .expect("a boolean argument")
            .expect("a boolean that was given")
    }

    /// An optional boolean the guest gave none for.
    fn is_undefined(&mut self, index: usize) -> bool {
        self.args
            .boolean(index)
            .expect("a boolean argument")
            .is_none()
    }

    /// A dictionary the guest's function has no parameter for: its
    /// defaults.
    fn dict_arg(&mut self, _: usize, _: &str) -> Option<Option<()>> {
        Some(None)
    }

    /// A listener of a wasm document is an event handler attribute's
    /// (`onfocus=""`), which is JS: it doesn't run.
    fn call_listener(&mut self, _: &elise::Persistent<dom::Event>, _: usize) {}

    fn nullable_string_arg(&mut self, index: usize) -> Option<Option<&'r str>> {
        let args: &'r A = self.args;
        Some(
            args.nullable_string(index)
                .expect("a nullable string argument"),
        )
    }

    fn legacy_null_string_arg(&mut self, index: usize) -> Option<Option<&'r str>> {
        self.nullable_string_arg(index)
    }

    fn node_arg(&mut self, index: usize) -> Option<Option<u32>> {
        Some(self.args.node(index).expect("a node argument"))
    }

    fn throw(&mut self, error: ApiError) {
        self.error = Some(error);
    }
}

impl<'r, A: ReadArgs> OpFront for WasmFront<'r, A> {
    fn str_param<'a>(&mut self, string: &'a &'r str) -> &'a str {
        string
    }

    /// `this`, whose interface the call checked: an object of its type.
    fn this_as<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
    ) -> Option<elise::Gc<'gc, T>> {
        self.tables
            .borrow()
            .store(self.store)
            .expect("a store's table")
            .object(cx, self.this)
            .downcast()
    }

    fn node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: &u32) -> NodeRef<'gc> {
        self.tables
            .borrow()
            .store(self.store)
            .expect("a store's table")
            .node(cx, *node)
    }

    fn ret_undefined(&mut self) {
        self.ret = Ret::Undefined;
    }

    fn ret_null(&mut self) {
        self.ret = Ret::Null;
    }

    fn ret_bool(&mut self, value: bool) {
        self.ret = Ret::Bool(value);
    }

    fn ret_u32(&mut self, value: u32) {
        self.ret = Ret::U32(value);
    }

    fn ret_f64(&mut self, value: f64) {
        self.ret = Ret::F64(value);
    }

    fn ret_i32(&mut self, value: i32) {
        self.ret = Ret::I32(value);
    }

    fn ret_string(&mut self, value: &DomString) {
        // A copy: wasmtime lowers no `Rc<str>`, and a `DomString` is its
        // thread's (wasmtime copies it into the guest again).
        self.ret = Ret::Str(Box::from(&*value.to_str()));
    }

    /// The store's handle to the node: the one it has, or a new one.
    fn ret_node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) {
        let handle = self.tables.borrow_mut().give(cx, self.store, node.into());
        self.ret = Ret::Node(handle);
    }

    /// The store's handle to the object, as a node's.
    fn ret_object<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, object: dom::ObjectRef<'gc>) {
        let handle = self.tables.borrow_mut().give(cx, self.store, object);
        self.ret = Ret::Node(handle);
    }
}
