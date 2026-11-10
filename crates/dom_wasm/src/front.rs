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

    /// The argument read with nothing of the guest's memory borrowed, if
    /// it can be (not a string): a member that calls script has the store
    /// free to call the guest while it runs.
    fn owned(wit: &Self::Wit) -> Option<Self::Read<'static>> {
        let _ = wit;
        None
    }
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
    /// A callback: the guest's number for it.
    fn callback(&self) -> Option<u32> {
        None
    }
}

/// A callback (a listener): the guest's own number for it, which its
/// `handle-event` is called with. Read as its own type: a `u32` read is an
/// object's handle.
pub(crate) struct Callback(u32);

impl ReadArg for Callback {
    fn callback(&self) -> Option<u32> {
        Some(self.0)
    }
}

impl WitArg for Option<dom_api::interfaces::EventListener> {
    type Wit = u32;
    type Read<'s> = Callback;

    fn read<T: 'static>(wit: &u32, _: &StoreContextMut<'_, T>) -> wasmtime::Result<Callback> {
        Ok(Callback(*wit))
    }
}

/// A listener's options: the boolean alone, `capture` (`wit::can_hold`).
impl<D: dom_api::typed::Dictionary + 'static> WitArg for dom_api::typed::DictOrBool<D> {
    type Wit = bool;
    type Read<'s> = bool;

    fn read<T: 'static>(wit: &bool, _: &StoreContextMut<'_, T>) -> wasmtime::Result<bool> {
        Ok(*wit)
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

            fn owned(wit: &u32) -> Option<u32> {
                Some(*wit)
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

            fn owned(wit: &Option<u32>) -> Option<Option<u32>> {
                Some(*wit)
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
    /// The arguments read with nothing of the guest's memory borrowed, if
    /// each can be (`WitArg::owned`).
    fn owned(rest: &Self::Rest) -> Option<Self::Read<'static>>;
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
    fn callback(&self, index: usize) -> Option<u32>;
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

            #[allow(unused_variables, clippy::unused_unit)]
            fn owned(rest: &Self::Rest) -> Option<Self::Read<'static>> {
                Some(($($t::owned(&rest.$i)?,)*))
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
            fn callback(&self, index: usize) -> Option<u32> {
                match index {
                    $($i => self.$i.callback(),)*
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

    fn owned((): &()) -> Option<()> {
        Some(())
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

/// Registers member `M`, which calls script (it dispatches events as it
/// runs: `focus()`, `click()`), as the function `name`: the guest's own
/// listeners are called within it (`WasmFront::call_listener`), so its
/// arguments borrow nothing of the guest's memory.
pub(crate) fn add_scripted<T, M>(
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
            let (this, rest) = <M::Args as WitArgs>::split(params);
            let args = <M::Args as WitArgs>::owned(&rest)
                .expect("a member that calls script takes no string");
            // The guest's listener export, called on this store while the
            // member runs: a sync call, on the stack of the guest's call.
            let (key, handler) = {
                let store = store(cx.data());
                (store.key, store.handler())
            };
            let returns = {
                let mut guest = |callback: u32, event: u32| -> wasmtime::Result<()> {
                    match handler {
                        Some(handler) => handler.call(&mut cx, (callback, event)),
                        None => Ok(()),
                    }
                };
                call_with::<M, _>(key, this, &rest, &args, Some(&mut guest))?
            };
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
    let (this, rest) = <M::Args as WitArgs>::split(params);
    let args = <M::Args as WitArgs>::read(&rest, &cx)?;
    call_with::<M, _>(key, this, &rest, &args, None)
}

/// The call, its arguments read: `this` and the handles checked, then the
/// member's typed path. `guest` calls the guest's listener export, for a
/// member that calls script.
fn call_with<'g, M, A>(
    key: crate::Key,
    this: u32,
    rest: &<M::Args as WitArgs>::Rest,
    args: &A,
    guest: Option<&'g mut (dyn FnMut(u32, u32) -> wasmtime::Result<()> + 'g)>,
) -> wasmtime::Result<Returns<M>>
where
    M: TypedMember,
    M::Args: WitArgs,
    M::Ret: WitRet,
    A: ReadArgs,
{
    let number = key.number;
    STATE.with_borrow(|slot| {
        let state = slot.of(key).ok_or_else(crate::no_store)?;
        // `this`'s interface, as its handle was given: the member's, or a
        // descendant of it (an element's by its name), with no DOM.
        let of = {
            let tables = state.tables.borrow();
            let handles = tables.store(number).ok_or_else(crate::no_store)?;
            let of = handles.interface(this).ok_or_else(|| not_held(this))?;
            <M::Args as WitArgs>::check(rest, handles)?;
            of
        };
        if !dom_api::typed::is_interface(M::INTERFACE, of) {
            return Ok((Err(DomException::of(ApiError::type_error(format!(
                "Illegal invocation: a {} is not a {}",
                dom_api::INTERFACES[of].name,
                dom_api::INTERFACES[M::INTERFACE].name
            )))),));
        }
        let mut front = WasmFront {
            dom: state.dom(),
            layout: &state.layout,
            tables: &state.tables,
            store: number,
            this,
            args,
            ret: Ret::None,
            error: None,
            guest,
            trap: None,
        };
        M::call(&mut front);
        // A listener trapped: the trap goes on up, through the guest's
        // call (its store takes no call after).
        if let Some(trap) = front.trap {
            return Err(trap);
        }
        Ok((match front.error {
            Some(error) => Err(DomException::of(error)),
            None => Ok(<M::Ret as WitRet>::wit(front.ret)),
        },))
    })
}

/// wasm's side of one call: the DOM, the store's table, `this`'s handle,
/// the read arguments, and what the call gave. Its own side in an
/// operation (`OpFront`).
struct WasmFront<'r, 'g, A> {
    dom: &'r RefCell<Dom>,
    /// The page's layout, if the DOM was lent with one.
    layout: &'r dom_api::LayoutCell,
    tables: &'r RefCell<Tables>,
    store: u32,
    this: u32,
    args: &'r A,
    ret: Ret,
    error: Option<ApiError>,
    /// The guest's listener export (its callback's number, the event's
    /// handle), for a member that calls script: none for the others,
    /// which dispatch nothing.
    guest: Option<&'g mut (dyn FnMut(u32, u32) -> wasmtime::Result<()> + 'g)>,
    /// What a listener trapped with: no listener of the store runs after.
    trap: Option<wasmtime::Error>,
}

impl<A: ReadArgs> WasmFront<'_, '_, A> {
    /// The word of the callback the argument `callback` names.
    fn callback_word(&self, callback: dom_api::typed::CallbackArg) -> u64 {
        let number = self.args.callback(callback.0).expect("a callback argument");
        crate::listener_word(self.store, number)
    }
}

impl<'r, A> FrontTypes for WasmFront<'r, '_, A> {
    type Str = &'r str;
    type Node = u32;
    // No dictionary is plain: no member taking one is bound.
    type Dict = ();
}

impl<'r, 'g, A: ReadArgs> Front for WasmFront<'r, 'g, A> {
    type Op<'o>
        = WasmFront<'r, 'g, A>
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

    /// A callback: the argument's place, which `store_callback` and
    /// `same_callback` read its number at.
    fn callback_arg(
        &mut self,
        index: usize,
        _: &str,
    ) -> Option<Option<dom_api::typed::CallbackArg>> {
        Some(Some(dom_api::typed::CallbackArg(index)))
    }

    /// A listener's options are the boolean alone.
    fn is_dictionary_like(&mut self, _: usize) -> bool {
        false
    }

    /// A dictionary the guest's function has no parameter for: its
    /// defaults.
    fn dict_arg(&mut self, _: usize, _: &str) -> Option<Option<()>> {
        Some(None)
    }

    /// A listener of the event the member dispatched, called within the
    /// guest's call: this store's own (its `handle-event`, with a handle
    /// to the event the host takes back after). An event handler
    /// attribute's (`onfocus=""`) is JS and doesn't run; another script's
    /// listener isn't called here (the member's store alone is entered).
    fn call_listener(&mut self, event: &elise::Persistent<dom::Event>, index: usize) {
        if self.trap.is_some() {
            return;
        }
        let Some(guest) = self.guest.as_mut() else {
            return;
        };
        let store = self.store;
        let (dom, tables) = (self.dom, self.tables);
        let listener = dom.borrow_mut().mutate(|cx| {
            let event = event.get(cx.gc);
            let listener = cx.snapshot_at(event, index)?;
            let word = cx.callback_edge(listener).embedder_word(cx.gc);
            let (of, callback) = crate::listener_of(word)?;
            (of == store).then(|| {
                let handle = tables.borrow_mut().give(cx, store, event.into());
                (callback, handle)
            })
        });
        let Some((callback, handle)) = listener else {
            return;
        };
        if let Err(trap) = guest(callback, handle) {
            self.trap = Some(trap);
        }
        // The event's handle back, unless the guest let it go itself.
        let mut tables = tables.borrow_mut();
        if tables
            .store(store)
            .is_some_and(|handles| handles.interface(handle).is_some())
        {
            dom.borrow_mut()
                .read(|cx| tables.release(cx, store, handle));
        }
    }

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

impl<'r, A: ReadArgs> OpFront for WasmFront<'r, '_, A> {
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

    /// A listener's callback as its edge's word: the store that gave it
    /// (its number and one: 0 is no guest's, a handler attribute's) and
    /// the guest's number for it.
    fn same_callback(
        &mut self,
        cx: &ApiCx<'_, '_>,
        edge: &elise::JsEdge,
        callback: dom_api::typed::CallbackArg,
    ) -> bool {
        edge.embedder_word(cx.gc) == self.callback_word(callback)
    }

    fn store_callback(
        &mut self,
        cx: &ApiCx<'_, '_>,
        edge: &elise::JsEdge,
        callback: dom_api::typed::CallbackArg,
    ) {
        edge.set_embedder_word(cx.gc, self.callback_word(callback));
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
