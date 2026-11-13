//! Typed paths: what `declare!` generates for each member, which a front
//! end makes a call of its own from (Blink's bindings generator makes one
//! V8 callback for each attribute and operation). Nothing here is of an
//! engine: a member's path is generic over its front end (`Front`), which
//! gets each one as a type (`TypedMember`) from a visit (`visit_typed`), so
//! that it makes one call of its own for each, with no lookup of the
//! member and no `dyn` between the engine and the member's function.
//!
//! A call converts its arguments first, which may run script (WebIDL's
//! `ToString` of an object), so outside the DOM's operation; then runs one
//! operation of the DOM, which the front end opens (`Front::mutate`;
//! `Front::read` for a getter, which needs no safepoint and borrows the
//! DOM shared): `this` and the arguments' nodes there, the member's
//! function, and its result handed to the front end's side in the
//! operation (`OpFront`) while its nodes are still the operation's; then,
//! the operation over, the front end makes what the result needs of its
//! engine (`Front::finish`). The engine's allocation may collect its heap,
//! and in a heap it drives, the DOM's too (V8's major GC marks it): never
//! in an operation, whose nodes are no roots. The front end's side in an
//! operation borrows the front end, so the engine's own calls (script,
//! allocation) are out of its reach there.

use std::marker::PhantomData;

use base::numerics::saturated_cast;
use dom::{
    DomString, EventTargetImpl, NameId, Node, NodeKind, NodeRef, NodeRoot, ObjectRef, ObjectRoot,
    StrRef, TargetRef, TargetRoot, TraversalRoot, names,
};
use elise::{Gc, HeapType, JsEdge, Persistent};

use crate::ApiCx;

use crate::declare::{Interface, MemberKind, TypeDesc};
use crate::exception::ApiError;
/// One of `roundRect`'s radii as a front end reads it
/// (`Front::round_rect_radii_arg`).
pub use canvas::path::Radius;

/// What a front end's arguments are as its engine holds them, out of an
/// operation and in one alike.
pub trait FrontTypes {
    /// A string argument: converted before the operation (`ToString`),
    /// its text read in it (`OpFront::str_param`), where the engine may
    /// lend its own characters (no copy) until the call drops the argument.
    type Str;
    /// A node argument (or another interface's object) as the engine holds
    /// it (a wrapper), converted before the operation and made the DOM's in
    /// it (`OpFront::node`, `OpFront::object`).
    type Node;
    /// A dictionary argument as the engine holds it (an object), whose
    /// members are read before the operation (`dict_bool`).
    type Dict;
    /// What a result leaves for after the operation when the front end
    /// can't give it in one (it would have its engine allocate, whose GC
    /// may reach the DOM): a string its engine has no copy of
    /// (`OpFront::ret_string`, `ret_name`), a node or another object with
    /// no wrapper (`ret_node`, `ret_object`), a number its engine must
    /// allocate (`ret_u32`, `ret_i32`, `ret_f64`). The member's call carries it out of the
    /// operation (`RetType::Pending`) and hands it back
    /// (`Front::finish_string`, `finish_node`, `finish_number`). A front
    /// end that always gives its result in the operation names a type with
    /// no value (`std::convert::Infallible`).
    type PendingString;
    type PendingNode;
    type PendingNumber;
    /// An object of any type with no wrapper (`OpFront::ret_object`,
    /// `Front::finish_object`).
    type PendingObject;
    /// What the front end makes ahead of the operation for a node its
    /// member will make (`Front::prepare_new_node`: a wrapper, which its
    /// engine can't allocate in the operation), carried into it by the
    /// member's call (`RetType::Prepared`) and taken as the node is given
    /// (`OpFront::ret_new_node`). `()` for a front end that makes none.
    type PreparedNode: Default;
    /// A callback argument as the engine holds it (an object: a function,
    /// or one with `handleEvent`; a timer's string), taken before the
    /// operation (`Front::callback_arg`...) and handed by the member's
    /// function back to the front end to store or compare
    /// (`OpFront::store_callback`): the function can't make one.
    type Callback: Copy;
    /// A value of any type as the engine holds it (WebIDL's `any`: an
    /// argument, a dictionary's member), taken before the operation
    /// (`Front::any_arg`, `dict_any`) and handed by the member's function
    /// back to the front end to store (`OpFront::store_any`).
    type Any: Copy;
    /// A call's last arguments (WebIDL's variadic `any...`) as the engine
    /// holds them to be stored (`Front::rest_arg`, `OpFront::store_rest`).
    type Rest: Copy;
}

/// A result type's `Pending` and `finish` when it leaves nothing for
/// after the operation.
macro_rules! no_pending {
    () => {
        $crate::typed::no_prepared!();
        type Pending<F: $crate::typed::FrontTypes> = ();

        #[inline(always)]
        fn finish<F: $crate::typed::Front>(_: &mut F, (): ()) {}
    };
}

/// A result type's `Pending`, `give` and `finish` when its value is plain
/// data (no reference into the DOM's heap) the engine must allocate for:
/// the value itself leaves the operation, which gives nothing, and the
/// front end's `$finish` makes it the call's result after.
macro_rules! finished_after {
    ($value:ty, $finish:ident) => {
        const PLAIN: bool = true;
        type Ret<'gc> = $value;
        $crate::typed::no_prepared!();
        type Pending<F: $crate::typed::FrontTypes> = $value;

        #[inline(always)]
        fn give<'gc, O: $crate::typed::OpFront>(
            _: &mut O,
            _: &$crate::ApiCx<'_, 'gc>,
            value: $value,
        ) -> $value {
            value
        }

        #[inline(always)]
        fn finish<F: $crate::typed::Front>(front: &mut F, value: $value) {
            front.$finish(value);
        }
    };
}

/// The objects of a result rooted with their interfaces, in the
/// operation: what a front end makes wrappers of after it.
fn rooted_objects<'gc, I: Interface>(
    cx: &ApiCx<'_, 'gc>,
    objects: Vec<<I::Object as ObjectType>::Ref<'gc>>,
) -> Vec<(ObjectRoot, usize)> {
    objects
        .into_iter()
        .map(|object| {
            let object = <I::Object as ObjectType>::object(object);
            (cx.root_object(object), interface_of(cx, object))
        })
        .collect()
}

/// The `OpFront` methods with no default that a member's function may
/// call whatever its types are (`with_array_bytes`, `HTMLElement`'s
/// constructor's), for a front end that binds no member whose function
/// does: each says so. A front end that binds them implements them.
#[macro_export]
macro_rules! unbound_op_methods {
    () => {
        fn with_array_bytes(
            &mut self,
            _: &$crate::ApiCx<'_, '_>,
            _: &::elise::JsEdge,
            _: &mut dyn FnMut(::std::option::Option<&[u8]>),
        ) {
            unreachable!("this front end binds no member reading an image's pixels")
        }

        fn construct_html<'gc>(
            &mut self,
            _: &$crate::ApiCx<'_, 'gc>,
            _: $crate::HtmlConstructed<'gc>,
        ) -> ::std::option::Option<::dom::NodeRoot> {
            unreachable!("this front end doesn't bind `HTMLElement`'s constructor")
        }

        fn new_target_number(&mut self) -> ::std::option::Option<u32> {
            unreachable!("this front end doesn't bind `HTMLElement`'s constructor")
        }

        fn is_new_target(&mut self, _: &$crate::ApiCx<'_, '_>, _: &::elise::JsEdge) -> bool {
            unreachable!("this front end doesn't bind `HTMLElement`'s constructor")
        }
    };
}

/// A result type's `Prepared` and `prepare` when nothing is made ahead of
/// the operation for it.
macro_rules! no_prepared {
    () => {
        type Prepared<F: $crate::typed::FrontTypes> = ();

        #[inline(always)]
        fn prepare<F: $crate::typed::Front>(_: &mut F, _: Option<usize>) {}
    };
}
pub(crate) use no_prepared;

/// A result type's `Pending` and `finish` when it may leave a string, a
/// node or a number (`$pending`, `$finish`: `PendingString` and
/// `finish_string`...) for after the operation. Always inlined: where the
/// front end names a type with no value, nothing is left of it.
macro_rules! pending {
    ($pending:ident, $finish:ident) => {
        no_prepared!();
        type Pending<F: FrontTypes> = Option<F::$pending>;

        #[inline(always)]
        fn finish<F: Front>(front: &mut F, pending: Option<F::$pending>) {
            if let Some(pending) = pending {
                front.$finish(pending);
            }
        }
    };
}

/// A front end's side of one call of a member, out of the DOM's operation:
/// its arguments, the operation it opens, what it makes of the result
/// after. An argument's method returning `None` has thrown already: the
/// call returns then.
pub trait Front: FrontTypes {
    /// The front end's side in the operation, borrowing it.
    type Op<'o>: OpFront<
            Str = Self::Str,
            Node = Self::Node,
            Dict = Self::Dict,
            PendingString = Self::PendingString,
            PendingNode = Self::PendingNode,
            PendingNumber = Self::PendingNumber,
            PendingObject = Self::PendingObject,
            PreparedNode = Self::PreparedNode,
            Callback = Self::Callback,
            Any = Self::Any,
            Rest = Self::Rest,
        >
    where
        Self: 'o;

    /// A dictionary's members read (`Dictionary::read`), each of which
    /// may run script (a getter): `reads` run on the front end that reads
    /// them, itself or (a front end that makes what its engine asks for a
    /// read only then) the one it makes for all of them at once, which
    /// holds its arguments as it does.
    fn reading<V: ReadDict<Self>>(&mut self, reads: V) -> V::Out
    where
        Self: Sized;

    /// A member's call that leaves script to run between its operations
    /// (`#[calls_script]`, `#[then_script]`), on the front end that runs
    /// script: itself, or (a front end that makes what its engine asks
    /// for only then) the one it makes for the call, which takes the
    /// arguments again.
    fn whole<C: WholeCall>(&mut self, call: C) -> C::Out
    where
        Self: Sized,
    {
        call.run(self)
    }

    /// Runs one operation of the DOM that allocates nothing (a getter's):
    /// no safepoint, the DOM borrowed shared.
    fn read<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R;

    /// Runs one operation of the DOM: a safepoint first (the heap may
    /// collect), the DOM borrowed whole.
    fn mutate<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R;

    /// Whether the call has at least `required` arguments; else a
    /// `TypeError` thrown (`name` the member's).
    fn has_args(&mut self, name: &str, required: usize) -> bool;

    /// Argument `index` as a `DOMString` (WebIDL's: `ToString`, which may
    /// run script).
    fn string_arg(&mut self, index: usize) -> Option<Self::Str>;

    /// How many arguments the call has: where a variadic one ends.
    fn arg_count(&mut self) -> usize {
        unreachable!("a front end binding a member with variadic strings counts them")
    }

    /// Argument `index` as a `DOMString?`: `None` for null or undefined.
    fn nullable_string_arg(&mut self, index: usize) -> Option<Option<Self::Str>>;

    /// Argument `index` as a `[LegacyNullToEmptyString] DOMString`: `None`
    /// for null only (undefined is `"undefined"`).
    fn legacy_null_string_arg(&mut self, index: usize) -> Option<Option<Self::Str>>;

    /// Argument `index` as a node, if it is one; `Some(None)` for null or
    /// undefined; anything else is a `TypeError`, thrown.
    fn node_arg(&mut self, index: usize) -> Option<Option<Self::Node>>;

    /// Argument `index` as a `(Node or DOMString)`: a node if it is one,
    /// else `ToString` (which may run script).
    fn node_or_string_arg(&mut self, index: usize) -> Option<NodeOrStr<Self::Node, Self::Str>> {
        let _ = index;
        unreachable!("a front end binding a member with nodes or strings reads them")
    }

    /// Argument `index` as a `(HTMLElement or long)?`: `Some(None)` for
    /// null or undefined, a node if it is one, else `ToNumber` and
    /// `ToInt32` (which may run script).
    fn node_or_long_arg(&mut self, index: usize) -> Option<Option<NodeOrLong<Self::Node>>> {
        let _ = index;
        unreachable!("a front end binding a member with a node or a long reads them")
    }

    /// Argument `index` as a dictionary (WebIDL's: `None` for undefined or
    /// null, whose members are their defaults); anything else but an
    /// object is a `TypeError`, thrown (`name` the dictionary's).
    fn dict_arg(&mut self, index: usize, name: &str) -> Option<Option<Self::Dict>> {
        let _ = (index, name);
        unreachable!("a front end binding a member with a dictionary reads them")
    }

    /// `dict`'s member `member` as a `boolean`: `Some(None)` if it is
    /// undefined (missing); `None` if reading it threw (a getter may run).
    fn dict_bool(&mut self, dict: &Self::Dict, member: MemberName) -> Option<Option<bool>> {
        let _ = (dict, member);
        unreachable!("a front end binding a member with a dictionary reads them")
    }

    /// `dict`'s member `member` as a number (`ToNumber`, which may run
    /// script): `Some(None)` if it is undefined (missing); `None` if
    /// reading or converting it threw. Its WebIDL type's conversion is the
    /// member's (`DictMember`).
    fn dict_number(&mut self, dict: &Self::Dict, member: MemberName) -> Option<Option<f64>> {
        let _ = (dict, member);
        unreachable!("a front end binding a member with a dictionary of numbers reads them")
    }

    /// `dict`'s member `member` as a `DOMString` (`ToString`, which may run
    /// script): `Some(None)` if it is undefined (missing); `None` if
    /// reading or converting it threw.
    fn dict_string(&mut self, dict: &Self::Dict, member: MemberName) -> Option<Option<String>> {
        let _ = (dict, member);
        unreachable!("a front end binding a member with a dictionary of strings reads them")
    }

    /// `dict`'s member `member` as a `DOMString?` (`ToString`, which may
    /// run script): `Some(None)` if it is undefined (missing) or null;
    /// `None` if reading or converting it threw.
    fn dict_nullable_string(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<String>> {
        let _ = (dict, member);
        unreachable!("a front end binding a member with a dictionary of strings reads them")
    }

    /// `dict`'s member `member` as a `sequence<DOMString>`: `Some(None)`
    /// if it is undefined (missing); `None` if reading or converting it
    /// threw (not a sequence is a `TypeError`).
    fn dict_strings(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<Vec<String>>> {
        let _ = (dict, member);
        unreachable!("a front end binding a member with a dictionary of strings reads them")
    }

    /// Argument `index` as a callback function (`name`'s, `MutationCallback`):
    /// a function, kept by the front end, which the result names; anything
    /// else a `TypeError`, thrown.
    fn function_arg(&mut self, index: usize, name: &str) -> Option<Callback<Self::Callback>> {
        let _ = (index, name);
        unreachable!("a front end binding a member with a callback function reads it")
    }

    /// Argument `index` as `(sequence<sequence<ByteString>> or
    /// record<ByteString, ByteString>)` (`HeadersInit`): as
    /// `string_pairs_arg`, but anything not an object, or a string with a
    /// character past U+00FF, a `TypeError`, thrown.
    fn byte_string_pairs_arg(&mut self, index: usize) -> Option<StringPairsArg> {
        let _ = index;
        unreachable!("a front end binding `new Headers()` reads its init")
    }

    /// Argument `index` as an abort reason (`abort()`'s `any`): the value,
    /// kept as `any_arg` keeps one; undefined is a new `AbortError`
    /// `DOMException` of the current realm, made now.
    fn abort_reason_arg(&mut self, index: usize) -> Option<AnyArg<Self::Any>> {
        let _ = index;
        unreachable!("a front end binding `abort()` reads its reason")
    }

    /// Argument `index` as a `sequence<I>` of objects (`T`'s wrappers, of
    /// interface `interface`): each one kept; another value a `TypeError`,
    /// thrown.
    fn objects_arg<T: HeapType>(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Vec<Self::Node>> {
        let _ = (index, interface);
        unreachable!("a front end binding a member with a sequence of objects reads it")
    }

    /// `dict`'s member `member` as an object (`T`'s wrapper, of interface
    /// `interface`): `Some(None)` if it is undefined (missing); another
    /// value a `TypeError`, thrown.
    fn dict_object<T: HeapType>(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
        interface: &str,
    ) -> Option<Option<Self::Node>> {
        let _ = (dict, member, interface);
        unreachable!("a front end binding a dictionary with an object reads it")
    }

    /// Argument `index` as `BodyInit?` (`new Response(body)`'s): `Some(None)`
    /// for null or undefined; another value as a `BodyInit` converts it
    /// (a string its `ToString`).
    fn body_arg(&mut self, index: usize) -> Option<Option<BodyArg<Self::Node>>> {
        let _ = index;
        unreachable!("a front end binding a member with a body reads it")
    }

    /// `dict`'s member `member` as `BodyInit?`: `Some(None)` if missing,
    /// else null or the body, as `body_arg`.
    fn dict_body(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<BodyMember<Self::Node>>> {
        let _ = (dict, member);
        unreachable!("a front end binding a dictionary with a body reads it")
    }

    /// `dict`'s member `member` as `HeadersInit`: `Some(None)` if missing,
    /// else as `byte_string_pairs_arg`.
    fn dict_headers(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<StringPairsArg>> {
        let _ = (dict, member);
        unreachable!("a front end binding a dictionary with headers reads them")
    }

    /// `dict`'s member `member` as a nullable object (`T`'s wrapper, of
    /// interface `interface`: an init dictionary's `view`, whose default
    /// is null): `Some(None)` if null or missing; another value a
    /// `TypeError`, thrown.
    fn dict_nullable_object<T: HeapType>(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
        interface: &str,
    ) -> Option<Option<Self::Node>> {
        let _ = (dict, member, interface);
        unreachable!("a front end binding a dictionary with an object reads it")
    }

    /// `dict`'s member `member` as a nullable event target of any type
    /// (a node, a window, what `new EventTarget()` makes: an init
    /// dictionary's `relatedTarget`), as `target_arg`: `Some(None)` if
    /// null or missing; another value a `TypeError`, thrown.
    fn dict_nullable_target(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<Self::Node>> {
        let _ = (dict, member);
        unreachable!("a front end binding a dictionary with an event target reads it")
    }

    /// `dict`'s member `member` as `AbortSignal?`: `Some(None)` if missing,
    /// else null or the signal's wrapper (another value a `TypeError`,
    /// thrown).
    fn dict_signal(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<SignalMember<Self::Node>>> {
        let _ = (dict, member);
        unreachable!("a front end binding a dictionary with a signal reads it")
    }

    /// `dict`'s member `member` as `any`, whether it is null: `Some(None)`
    /// if missing.
    fn dict_is_null(&mut self, dict: &Self::Dict, member: MemberName) -> Option<Option<bool>> {
        let _ = (dict, member);
        unreachable!("a front end binding a dictionary with an `any` reads it")
    }

    /// Argument `index` serialized as JSON (`Response.json()`'s data): a
    /// value JSON can't serialize is a `TypeError`, and what serializing
    /// throws is thrown.
    fn json_arg(&mut self, index: usize) -> Option<String> {
        let _ = index;
        unreachable!("a front end binding `Response.json` serializes its data")
    }

    /// Argument `index` as a nullable callback function (`name`'s,
    /// `PositionErrorCallback`), a member's second callback: `Some(None)`
    /// for null or undefined; a function kept by the front end, which the
    /// result names; anything else a `TypeError`, thrown.
    fn nullable_function_arg(
        &mut self,
        index: usize,
        name: &str,
    ) -> Option<Option<Callback<Self::Callback>>> {
        let _ = (index, name);
        unreachable!("a front end binding a member with a nullable callback function reads it")
    }

    /// Argument `index` as a `sequence<unrestricted double>`: an iterable
    /// of values each converted to a number; another value a `TypeError`.
    fn doubles_arg(&mut self, index: usize) -> Option<Vec<f64>> {
        let _ = index;
        unreachable!("a front end binding a member taking numbers reads them")
    }

    /// Argument `index` as a `Uint8ClampedArray`: the array as the engine
    /// holds it and its length; another value a `TypeError`.
    fn uint8_clamped_arg(&mut self, index: usize) -> Option<Uint8ClampedArg<Self::Any>> {
        let _ = index;
        unreachable!("a front end binding a member taking a typed array reads it")
    }

    /// Argument `index` as an `ArrayBufferView`, kept by the front end (one
    /// a call): what it is and how many bytes it views; another value a
    /// `TypeError`.
    fn array_view_arg(&mut self, index: usize) -> Option<ArrayViewArg> {
        let _ = index;
        unreachable!("a front end binding `getRandomValues` reads its view")
    }

    /// Argument `index`, any value, serialized for `structuredClone` (HTML's
    /// `StructuredSerializeWithTransfer`), its transfer list option
    /// `transfer` of the dictionary at `index + 1`: the serialization and
    /// the ports transferred, in its order. `None` if it threw (a
    /// `DataCloneError`, or what script threw).
    fn clone_arg(&mut self, index: usize) -> Option<(dom::SerializedMessage, Vec<Self::Node>)> {
        let _ = index;
        unreachable!("a front end binding `structuredClone` serializes its value")
    }

    /// Argument `index` as an optional `sequence<BlobPart>` (File API's
    /// `BlobPart`: `(BufferSource or Blob or USVString)`): `Some(None)` if
    /// it is undefined (missing); each item a blob if it is one's wrapper,
    /// a copy of the bytes a buffer or a view of one has now, else
    /// `ToString` (a `USVString`: lone surrogates replaced). Another value
    /// a `TypeError`; `None` if it threw.
    fn blob_parts_arg(&mut self, index: usize) -> Option<Option<Vec<BlobPartArg<Self::Node>>>> {
        let _ = index;
        unreachable!("a front end binding `Blob`'s constructor reads its parts")
    }

    /// Argument `index`, not undefined, as WebIDL's `(sequence<sequence<
    /// USVString>> or record<USVString, USVString> or USVString)`: an
    /// object with an `@@iterator` method a sequence of sequences, another
    /// object a record (its own enumerable keys in order), anything else
    /// `ToString`; each string a `USVString` (lone surrogates replaced).
    /// `None` if it threw (a sequence's item that isn't an object a
    /// `TypeError`).
    fn string_pairs_arg(&mut self, index: usize) -> Option<StringPairsArg> {
        let _ = index;
        unreachable!("a front end binding `URLSearchParams`' constructor reads its init")
    }

    /// Argument `index`, a message, serialized for `postMessage` (HTML's
    /// `StructuredSerializeWithTransfer`), its transfer list where
    /// `transfer` says; `from_port`: `this` is a port, which can't be in
    /// it. The serialization and the ports transferred, in the order it
    /// numbers them; `None` if it threw (a `DataCloneError`, a transfer
    /// list that isn't a sequence, or what a getter threw).
    fn message_arg(
        &mut self,
        index: usize,
        transfer: TransferArg,
        from_port: bool,
    ) -> Option<(dom::SerializedMessage, Vec<Self::Node>)> {
        let _ = (index, transfer, from_port);
        unreachable!("a front end binding `postMessage` serializes its message")
    }

    /// Argument `index` as `roundRect`'s optional `radii` (WebIDL's
    /// `(unrestricted double or DOMPointInit or sequence<(unrestricted
    /// double or DOMPointInit)>)`, default 0): undefined one radius of 0;
    /// an object with an `@@iterator` a sequence, each item an object (or
    /// null, undefined) a `DOMPointInit`'s `x` and `y` (its members read
    /// in order: `w`, `x`, `y`, `z`), anything else a number; another
    /// object, or null, one `DOMPointInit`; anything else one number
    /// (`ToNumber`). `None` if it threw.
    fn round_rect_radii_arg(&mut self, index: usize) -> Option<Vec<Radius>> {
        let _ = index;
        unreachable!("a front end binding `roundRect` reads its radii")
    }

    /// Argument `index`, `animate()`'s options (`(unrestricted double or
    /// KeyframeAnimationOptions)`, its members read in WebIDL's order and
    /// its enumerations checked: a `TypeError` otherwise); its defaults for
    /// `undefined`. `None` if it threw.
    fn animation_options_arg(&mut self, index: usize) -> Option<AnimationOptions> {
        let _ = index;
        unreachable!("a front end binding `animate` reads its options")
    }

    /// Argument `index`, `new KeyframeEffect()`'s options (`(unrestricted
    /// double or KeyframeEffectOptions)`), read as `animation_options_arg`
    /// reads its own but `KeyframeAnimationOptions`' members.
    /// Argument `index`, a history state serialized for storage (HTML's
    /// `StructuredSerializeForStorage`): `None` if it threw (a
    /// `DataCloneError`).
    fn serialized_state_arg(&mut self, index: usize) -> Option<Vec<u8>> {
        let _ = index;
        unreachable!("a front end binding `pushState` serializes its state")
    }

    fn effect_options_arg(&mut self, index: usize) -> Option<AnimationOptions> {
        let _ = index;
        unreachable!("a front end binding `KeyframeEffect`'s constructor reads its options")
    }

    /// Argument `index`, `updateTiming()`'s `OptionalEffectTiming`: the
    /// members given, read in WebIDL's order. `None` if it threw.
    fn optional_timing_arg(&mut self, index: usize) -> Option<animation::OptionalTiming> {
        let _ = index;
        unreachable!("a front end binding `updateTiming` reads its timing")
    }

    /// Argument `index`, an `AnimationTimeline?` with no default given:
    /// whether the animation has one (missing, or a `DocumentTimeline`:
    /// its document's; `null`: none); anything else a `TypeError`. `None`
    /// if it threw.
    fn timeline_arg(&mut self, index: usize) -> Option<bool> {
        let _ = index;
        unreachable!("a front end binding `new Animation()` reads its timeline")
    }

    /// Argument `index`, a keyframes argument (web-animations 1's `object?`
    /// read as "process a keyframes argument" reads it, in its order): an
    /// iterable's keyframes, or one property-indexed keyframe; `null` or
    /// `undefined` none. `None` if it threw.
    fn keyframes_arg(&mut self, index: usize) -> Option<Option<animation::ScriptKeyframes>> {
        let _ = index;
        unreachable!("a front end binding `animate` reads its keyframes")
    }

    /// Argument `index`'s value if it is a number (WebIDL's `any`, read
    /// with no conversion: `toBlob`'s `quality`); none otherwise. It never
    /// throws.
    fn number_if(&mut self, index: usize) -> Option<f64> {
        let _ = index;
        unreachable!("a front end binding a member taking a number or anything reads it")
    }

    /// Argument `index`, if it is an object of GC type `T` (a wrapper of
    /// one): nothing thrown when it isn't (a union's other types).
    fn object_if<T: HeapType>(&mut self, index: usize) -> Option<Self::Node> {
        let _ = index;
        unreachable!("a front end binding a member taking a union of objects reads it")
    }

    /// Argument `index`'s value if it is a boolean (a union's member, read
    /// with no conversion); none otherwise. It never throws.
    fn bool_if(&mut self, index: usize) -> Option<bool> {
        let _ = index;
        unreachable!("a front end binding a member taking a union with a boolean reads it")
    }

    /// Argument `index` as a `boolean` (WebIDL's: `ToBoolean`, which runs
    /// nothing).
    fn bool_arg(&mut self, index: usize) -> bool {
        let _ = index;
        unreachable!("a front end binding a member with a boolean reads it")
    }

    /// Whether argument `index` is undefined (or missing).
    fn is_undefined(&mut self, index: usize) -> bool {
        let _ = index;
        unreachable!("a front end binding a member with an optional boolean reads it")
    }

    /// Whether argument `index` is null or undefined (or missing).
    fn is_null_or_undefined(&mut self, index: usize) -> bool {
        let _ = index;
        unreachable!("a front end binding a member with a nullable number reads it")
    }

    /// Whether argument `index` is an object, null or undefined: what a
    /// union of a dictionary and a `boolean` converts as the dictionary.
    fn is_dictionary_like(&mut self, index: usize) -> bool {
        let _ = index;
        unreachable!("a front end binding a member with a union reads it")
    }

    /// Argument `index` as a callback interface (`name`'s, `EventListener`):
    /// `Some(None)` for null or undefined; any other object kept by the
    /// front end, which the result names (`Callback`); anything else a
    /// `TypeError`, thrown.
    fn callback_arg(
        &mut self,
        index: usize,
        name: &str,
    ) -> Option<Option<Callback<Self::Callback>>> {
        let _ = (index, name);
        unreachable!("a front end binding a member with a callback reads it")
    }

    /// Argument `index` as HTML's `EventHandler` (`[LegacyTreatNonObjectAsNull]
    /// callback function`): any object kept by the front end, which the
    /// result names; anything else null (`None`). It never throws.
    fn event_handler_arg(&mut self, index: usize) -> Option<Callback<Self::Callback>> {
        let _ = index;
        unreachable!("a front end binding an event handler reads it")
    }

    /// Argument `index` as WebIDL's `unsigned long` (`ToUint32`: undefined
    /// is 0); `None` if the conversion threw.
    fn u32_arg(&mut self, index: usize) -> Option<u32> {
        let _ = index;
        unreachable!("a front end binding a member with an `unsigned long` reads it")
    }

    /// Argument `index` as WebIDL's `long` (`ToInt32`: undefined is 0);
    /// `None` if the conversion threw.
    fn i32_arg(&mut self, index: usize) -> Option<i32> {
        let _ = index;
        unreachable!("a front end binding a member with a `long` reads it")
    }

    /// Argument `index` as WebIDL's `unrestricted double` (`ToNumber`:
    /// undefined is NaN), or with `finite` its `double` (a NaN or an
    /// infinity a `TypeError`, thrown); `None` if the conversion threw.
    fn f64_arg(&mut self, index: usize, finite: bool) -> Option<f64> {
        let _ = (index, finite);
        unreachable!("a front end binding a member with a `double` reads it")
    }

    /// Argument `index` as HTML's `TimerHandler`: a function kept as it is,
    /// anything else converted to a string (`ToString`: a handler the timer
    /// runs as a classic script), kept by the front end, which the result
    /// names; `None` if the conversion threw.
    fn timer_handler_arg(&mut self, index: usize) -> Option<Callback<Self::Callback>> {
        let _ = index;
        unreachable!("a front end binding a member with a timer handler reads it")
    }

    /// The call's arguments from `index` on (WebIDL's variadic `any...`),
    /// as the front end holds them to be stored (a new array); none if
    /// there are none.
    fn rest_arg(&mut self, index: usize) -> Option<Self::Rest> {
        let _ = index;
        unreachable!("a front end binding a member with rest arguments reads them")
    }

    /// Argument `index` as WebIDL's `any` whose default is null
    /// (`initCustomEvent`'s `detail`): the value, null for undefined (or
    /// missing). It never throws.
    fn any_arg(&mut self, index: usize) -> AnyArg<Self::Any> {
        let _ = index;
        unreachable!("a front end binding a member with an `any` reads it")
    }

    /// Dictionary member `member` structured-serialized (HTML's
    /// `StructuredSerializeForStorage`: User Timing's `detail`), none if it
    /// is undefined; `None` if it threw (a `DataCloneError`).
    fn dict_serialized(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<Vec<u8>>> {
        let _ = (dict, member);
        unreachable!("a front end binding User Timing serializes its detail")
    }

    /// Dictionary member `member` as WebIDL's `(DOMString or
    /// DOMHighResTimeStamp)`: a number as it is, anything else `ToString`;
    /// none if it is undefined.
    fn dict_string_or_number(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<StringOrNumber>> {
        let _ = (dict, member);
        unreachable!("a front end binding User Timing reads its marks")
    }

    /// `dict`'s member `member` as WebIDL's `any`: `Some(None)` if it is
    /// undefined (missing); `None` if reading it threw.
    fn dict_any(
        &mut self,
        dict: &Self::Dict,
        member: MemberName,
    ) -> Option<Option<AnyArg<Self::Any>>> {
        let _ = (dict, member);
        unreachable!("a front end binding a member with a dictionary of `any`s reads them")
    }

    /// Argument `index` as an event target of any type (a node, a window,
    /// what `new EventTarget()` makes); `Some(None)` for null or undefined;
    /// anything else a `TypeError` naming `interface`, thrown.
    fn target_arg(&mut self, index: usize, interface: &str) -> Option<Option<Self::Node>> {
        let _ = (index, interface);
        unreachable!("a front end binding a member with an event target reads it")
    }

    /// Argument `index` as an object of GC type `T` (a wrapper of one);
    /// `Some(None)` for null or undefined; anything else a `TypeError`
    /// naming `interface`, thrown.
    fn object_arg<T: HeapType>(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Option<Self::Node>> {
        let _ = (index, interface);
        unreachable!("a front end binding a member with an object reads it")
    }

    /// A constructor's call, before its arguments: whether it was called as
    /// one (JS's `new`) with at least `required` arguments; else a
    /// `TypeError` thrown (`INTERFACES[interface]` the constructor's).
    fn construct(&mut self, interface: usize, required: usize) -> bool {
        let _ = (interface, required);
        unreachable!("a front end binding a constructor checks its calls")
    }

    /// A constructor's call that may ask for its realm's window in its
    /// operation (`OpFront::current_window`), before it: what the engine
    /// finds it by, found while it may.
    fn prepare_window(&mut self) {}

    /// `prepare_window` with the incumbent realm's window (HTML's: the
    /// realm of the most recent script on the stack, not the member's
    /// own): `postMessage`'s `source` and origin.
    fn prepare_incumbent_window(&mut self) {
        self.prepare_window();
    }

    /// `prepare_window` with the entry realm's window (HTML's entry
    /// settings object's: the realm the script that started the call stack
    /// is in): the document a `Location`'s URL is resolved against.
    fn prepare_entry_window(&mut self) {
        self.prepare_window();
    }

    /// `prepare_window` with the window of `this`'s relevant realm (the
    /// realm its wrapper was made in), not the member's function's: a
    /// blob read's task (File API's promise in this's relevant realm), so
    /// that another frame's `Blob.prototype.text` called on this frame's
    /// blob reads in this frame.
    fn prepare_relevant_window(&mut self) {
        self.prepare_window();
    }

    /// A new node's wrapper, of `INTERFACES[interface]`, made ahead of the
    /// operation that makes the node (WebIDL's `[NewObject]`): none in it.
    fn prepare_new_node(&mut self, interface: usize) -> Self::PreparedNode {
        let _ = interface;
        Self::PreparedNode::default()
    }

    /// `prepare_new_node` of the interface `this` is of: for a member
    /// whose new node is of it (`cloneNode`). None by default.
    fn prepare_node_like_this(&mut self) -> Self::PreparedNode {
        Self::PreparedNode::default()
    }

    /// `createElement`'s: what the local name `name` says of the element
    /// in an HTML document, read without making anything: the name's
    /// number if it found it as it is (lowercase), for the operation to
    /// use, and the interface the element will be of (what its wrapper is
    /// made ahead of, `prepare_new_node`); `HTMLElement` by default.
    fn foresee_element(&mut self, name: &Self::Str) -> (Option<dom::NameId>, usize) {
        let _ = name;
        (None, crate::declare::HTML_ELEMENT)
    }

    /// The result made the engine's value, after the operation, with none
    /// running: what the engine allocates for it (a string, a wrapper), and
    /// an operation of its own if it needs one (to wrap a node).
    fn finish(&mut self) {}

    /// What a result left for after the operation (`OpFront::ret_string`,
    /// `ret_node`, `ret_u32`...), made the engine's value now, with none
    /// running: a front end whose `Pending` types have values has these.
    fn finish_string(&mut self, pending: Self::PendingString) {
        let _ = pending;
        unreachable!("a front end that leaves a string for after the operation finishes it")
    }
    fn finish_node(&mut self, pending: Self::PendingNode) {
        let _ = pending;
        unreachable!("a front end that leaves a node for after the operation finishes it")
    }
    fn finish_number(&mut self, pending: Self::PendingNumber) {
        let _ = pending;
        unreachable!("a front end that leaves a number for after the operation finishes it")
    }
    fn finish_object(&mut self, pending: Self::PendingObject) {
        let _ = pending;
        unreachable!("a front end that leaves an object for after the operation finishes it")
    }

    /// The error the call throws, after the operation.
    fn throw(&mut self, error: ApiError);

    /// The error a call giving a promise gives it rejected with, after the
    /// operation: a new promise.
    fn reject(&mut self, error: ApiError) {
        let _ = error;
        unreachable!("a front end binding a member giving a promise rejects it")
    }

    /// A `[CEReactions]` member's element queue, left as its operation
    /// ended (`Cx::leave_ce_reactions` gave true): its reactions invoked
    /// (`custom_elements::invoke_reactions`), the front end as their script
    /// host. Out of line: most calls enqueue none.
    #[cold]
    #[inline(never)]
    fn invoke_ce_reactions(&mut self)
    where
        Self: Sized,
    {
        if self.read(|cx, _| cx.has_element_changes()) {
            self.update_elements();
        }
        let Some(queue) = self.mutate(|cx, _| cx.take_left_element_queue()) else {
            return;
        };
        crate::custom_elements::invoke_reactions(&mut FrontHost(self), &queue);
    }

    /// `getHTML`'s options (`GetHtmlOptionsArg`) at `index`: undefined or
    /// null the defaults, an object its `serializableShadowRoots` and its
    /// `shadowRoots` converted to a sequence of nodes; another value, or a
    /// `shadowRoots` item that isn't a node, a `TypeError`.
    /// `serializableShadowRoots`, and the nodes (none if `shadowRoots`
    /// wasn't given).
    fn get_html_options(&mut self, index: usize) -> Option<(bool, Vec<Self::Node>)> {
        let _ = index;
        unreachable!("a front end binding `getHTML` reads its options")
    }

    /// Argument `index` as an `IntersectionObserverInit` (undefined or null
    /// its defaults): `root`, a node's wrapper, given with it; `threshold`
    /// a number or a sequence
    /// of them; the rest as its members' types. Another value, a `root`
    /// that isn't a node, or a number that isn't finite: a `TypeError`.
    fn intersection_observer_init(
        &mut self,
        index: usize,
    ) -> Option<(IntersectionInit, Option<Self::Node>)> {
        let _ = index;
        unreachable!("a front end binding `IntersectionObserver` reads its options")
    }

    /// Argument `index` as an `IntersectionObserverEntryInit`, every
    /// member required: `target`, a node's wrapper, given with them; the
    /// rects `DOMRectInit`s (undefined
    /// or null their zeros, `rootBounds` null for none). Another value, a
    /// member missing, a `target` that isn't a node, or a `time` or
    /// `intersectionRatio` that isn't finite: a `TypeError`.
    fn intersection_entry_init(
        &mut self,
        index: usize,
    ) -> Option<(IntersectionEntryInitValues, Self::Node)> {
        let _ = index;
        unreachable!("a front end binding `IntersectionObserverEntry` reads its init")
    }

    /// Argument `index` as a `StaticRangeInit`, every member required:
    /// `startContainer` and `endContainer`, nodes' wrappers; the offsets
    /// `ToUint32`'s. The start, then the end.
    /// Another value, a member missing, or a container that isn't a node:
    /// a `TypeError`.
    fn static_range_init(&mut self, index: usize) -> Option<[(Self::Node, u32); 2]> {
        let _ = index;
        unreachable!("a front end binding `StaticRange` reads its init")
    }

    /// `ScriptHost::read_custom_element_constructor`, the call's callback
    /// argument's.
    fn read_custom_element_constructor(&mut self) -> Result<ConstructorInfo, ApiError> {
        unreachable!("a front end binding `define` reads its constructor")
    }

    /// `ScriptHost::store_custom_element_definition`.
    fn store_custom_element_definition(
        &mut self,
        registry: &Persistent<dom::CustomElementRegistry>,
        definition: &Persistent<dom::CustomElementDefinition>,
    ) {
        let _ = (registry, definition);
        unreachable!("a front end binding `define` stores its callbacks")
    }

    /// `ScriptHost::call_lifecycle_callback`.
    fn call_lifecycle_callback(
        &mut self,
        element: &Persistent<dom::Node>,
        definition: &Persistent<dom::CustomElementDefinition>,
        callback: crate::custom_elements::LifecycleCallback,
    ) {
        let _ = (element, definition, callback);
        unreachable!("a front end invoking custom element reactions calls their callbacks")
    }

    /// `ScriptHost::upgrade_element`.
    fn upgrade_element(
        &mut self,
        definition: &Persistent<dom::CustomElementDefinition>,
        element: &Persistent<dom::Node>,
    ) -> bool {
        let _ = (definition, element);
        unreachable!("a front end binding a member upgrading elements constructs them")
    }

    /// `ScriptHost::constructor_is_constructor`.
    fn constructor_is_constructor(&mut self) -> bool {
        unreachable!("a front end binding `define` checks its constructor")
    }

    /// `ScriptHost::constructor_is_defined`.
    fn constructor_is_defined(
        &mut self,
        registry: &Persistent<dom::CustomElementRegistry>,
    ) -> bool {
        let _ = registry;
        unreachable!("a front end binding `define` compares its constructor")
    }

    /// `ScriptHost::resolve_when_defined`.
    fn resolve_when_defined(
        &mut self,
        pending: &[u64],
        definition: &Persistent<dom::CustomElementDefinition>,
    ) {
        let _ = (pending, definition);
        unreachable!("a front end binding `define` resolves `whenDefined`'s promises")
    }

    /// `ScriptHost::call_mutation_callback`, for a member that calls
    /// script.
    fn call_mutation_callback(
        &mut self,
        observer: &Persistent<dom::MutationObserver>,
        records: &[Persistent<dom::MutationRecord>],
    ) {
        let _ = (observer, records);
        unreachable!("a front end binding a member that calls script calls observers")
    }

    /// `ScriptHost::call_listener`, for a member that calls script.
    fn call_listener(&mut self, event: &Persistent<dom::Event>, index: usize) {
        let _ = (event, index);
        unreachable!("a front end binding a member that calls script calls listeners")
    }

    /// `ScriptHost::reject_promise_with_reason`, for `abort()`'s steps.
    fn reject_promise_with_reason(&mut self, promise: u64, signal: &Persistent<EventTargetImpl>) {
        let _ = (promise, signal);
        unreachable!("a front end binding `abort()` rejects an aborted fetch's promise")
    }

    /// `ScriptHost::pending_input`, for `isInputPending()`.
    fn pending_input(&mut self, include_continuous: bool) -> Vec<crate::PendingInput> {
        let _ = include_continuous;
        unreachable!("a front end binding `isInputPending` asks the engine's monitor")
    }

    /// `ScriptHost::call_node_filter`.
    fn call_node_filter(
        &mut self,
        traversal: &TraversalRoot,
        node: &dom::NodeRoot,
    ) -> Result<u16, ApiError> {
        let _ = (traversal, node);
        unreachable!("a front end binding a traversal calls its filter")
    }

    /// `ScriptHost::compile_event_handler`, for a member that calls script
    /// (an event handler's getter). None by default: an engine with no
    /// script of HTML's to compile leaves it uncompiled (it is never
    /// called).
    fn compile_event_handler(&mut self, target: &TargetRoot, event_type: NameId) {
        let _ = (target, event_type);
    }

    // What a result's type carries out of the operation for the engine
    // to allocate for (`RetType::Pending`): each made the engine's value
    // and the call's result, after it. None has a default: a front end
    // binding a member of such a result makes it.

    /// A new array of the objects (each with its interface): a
    /// `sequence<I>`.
    fn finish_objects(&mut self, objects: Vec<(ObjectRoot, usize)>) {
        let _ = objects;
        unreachable!("a front end binding a member giving a sequence makes it")
    }
    /// As `finish_objects`, a `FrozenArray<I>`: frozen, and the same
    /// array each time while its values are the same.
    fn finish_frozen_objects(&mut self, objects: Vec<(ObjectRoot, usize)>) {
        let _ = objects;
        unreachable!("a front end binding a member giving a frozen array makes it")
    }
    /// A new array of the strings.
    fn finish_strings(&mut self, strings: Vec<DomString>) {
        let _ = strings;
        unreachable!("a front end binding a member giving strings makes them")
    }
    /// A new array of names: one of the names table by its number (the
    /// front end may keep its engine's string of it), or text. As
    /// `finish_strings` of them, unless the front end has its own.
    fn finish_names(&mut self, names: Vec<NameOrText>) {
        self.finish_strings(
            names
                .into_iter()
                .map(|name| match name {
                    NameOrText::Name(id) => names::string(id),
                    NameOrText::Text(text) => DomString::from(text),
                })
                .collect(),
        );
    }
    /// As `finish_strings`, a `FrozenArray<DOMString>`.
    fn finish_frozen_strings(&mut self, strings: Vec<DomString>) {
        let _ = strings;
        unreachable!("a front end binding a member giving a frozen array makes it")
    }
    /// A new array of the numbers (`sequence<double>`).
    fn finish_doubles(&mut self, numbers: Vec<f64>) {
        let _ = numbers;
        unreachable!("a front end binding a member giving numbers makes them")
    }
    /// As `finish_doubles`, a `FrozenArray<double>`.
    fn finish_numbers(&mut self, numbers: Vec<f64>) {
        let _ = numbers;
        unreachable!("a front end binding a member giving a frozen array makes it")
    }
    /// A new object of the members, in order (a dictionary).
    fn finish_dictionary(&mut self, members: Vec<(&'static str, DictValue)>) {
        let _ = members;
        unreachable!("a front end binding a member giving a dictionary makes it")
    }
    /// A new array of new objects (`getKeyframes()`'s records).
    fn finish_records(&mut self, records: Vec<Vec<(String, DictValue)>>) {
        let _ = records;
        unreachable!("a front end binding a member giving records makes them")
    }
    /// A mark's or a measure's `detail`: its serialization deserialized
    /// at the first ask, the same value after.
    fn finish_entry_detail(&mut self, entry: Persistent<dom::PerformanceEntry>) {
        let _ = entry;
        unreachable!("a front end binding User Timing gives its detail")
    }
    /// A `FileReader`'s `ArrayBuffer` result: made of its bytes at the
    /// first ask, the same buffer after.
    fn finish_reader_buffer(&mut self, reader: Persistent<EventTargetImpl>) {
        let _ = reader;
        unreachable!("a front end binding `FileReader` gives its buffer")
    }
    /// `history.state`: the current state's deserialization, made at the
    /// first ask and the same value after.
    fn finish_history_state(&mut self, history: Persistent<dom::History>) {
        let _ = history;
        unreachable!("a front end binding `history.state` gives it")
    }
    /// The first argument, an `ArrayBufferView` as `array_view_arg` took
    /// it, these bytes written to it: the result.
    fn finish_filled_view(&mut self, bytes: Vec<u8>) {
        let _ = bytes;
        unreachable!("a front end binding `getRandomValues` fills its view")
    }
    /// `payload` deserialized in the receiver's realm, its ports these
    /// objects (`structuredClone()`'s); a `DataCloneError` if it can't be.
    fn finish_cloned(&mut self, payload: dom::SerializedMessage, ports: Vec<(ObjectRoot, usize)>) {
        let _ = (payload, ports);
        unreachable!("a front end binding `structuredClone` deserializes")
    }
    /// A new promise resolved with the object (`Promise<I>`).
    fn finish_resolved(&mut self, object: ObjectRoot, interface: usize) {
        let _ = (object, interface);
        unreachable!("a front end binding a member giving a promise makes it")
    }
    /// A new promise resolved with the string.
    fn finish_resolved_string(&mut self, string: DomString) {
        let _ = string;
        unreachable!("a front end binding a member giving a promise makes it")
    }
    /// A new promise resolved with the text parsed as JSON, or rejected
    /// with what parsing threw (the body mixin's `json()`).
    fn finish_resolved_json(&mut self, text: DomString) {
        let _ = text;
        unreachable!("a front end binding a member giving a promise makes it")
    }
    /// A new promise resolved with undefined.
    fn finish_resolved_undefined(&mut self) {
        unreachable!("a front end binding a member giving a promise makes it")
    }
    /// A new promise resolved with a new `ArrayBuffer` of the bytes (its
    /// own: no copy), or (`view`) a `Uint8Array` of one.
    fn finish_resolved_bytes(&mut self, bytes: Vec<u8>, view: bool) {
        let _ = (bytes, view);
        unreachable!("a front end binding a member giving a promise makes it")
    }
    /// A new `ImageData`'s array made of `bytes` (no copy: they become
    /// its buffer's), before the result or the constructed object is
    /// given.
    fn finish_image_data_pixels(&mut self, image_data: Persistent<dom::ImageData>, bytes: Vec<u8>) {
        let _ = (image_data, bytes);
        unreachable!("a front end binding a member making an `ImageData` makes its array")
    }
    /// An element being upgraded whose wrapper `HTMLElement`'s
    /// constructor gave (`OpFront::construct_html`): the wrapper given
    /// `new.target`'s prototype.
    fn finish_upgrading(&mut self, element: NodeRoot) {
        let _ = element;
        unreachable!("a front end binding `HTMLElement`'s constructor finishes an upgrade")
    }
    /// A pending promise: a new one, its resolver kept by the number
    /// `id`; or (`existing`) the promise of the resolver kept by it
    /// already.
    fn finish_pending_promise(&mut self, id: u64, existing: bool) {
        let _ = (id, existing);
        unreachable!("a front end binding a member giving a pending promise makes it")
    }
    /// A new promise resolved with the definition's constructor
    /// (`whenDefined()`'s of a name that is defined).
    fn finish_resolved_constructor(
        &mut self,
        definition: Persistent<dom::CustomElementDefinition>,
    ) {
        let _ = definition;
        unreachable!("a front end binding `whenDefined` gives its promise")
    }
    /// An animation's object's `ready` (`ready`) or `finished` promise:
    /// the one kept on its object (of `interface`), made at the first
    /// ask, resolved at once if new and its animation's is (`resolved`).
    fn finish_animation_promise(
        &mut self,
        object: Persistent<EventTargetImpl>,
        interface: usize,
        ready: bool,
        resolved: bool,
    ) {
        let _ = (object, interface, ready, resolved);
        unreachable!("a front end binding animations gives their promises")
    }

    /// An event handler's getter's result left for after the operation
    /// (`EventHandler`'s `Pending`): `target`'s handler of `event_type`,
    /// uncompiled, is compiled, and its value then given (null if its
    /// source didn't compile).
    #[cold]
    #[inline(never)]
    fn finish_handler(&mut self, target: &TargetRoot, event_type: NameId) {
        self.compile_event_handler(target, event_type);
        self.read(
            |cx, op| match cx.event_handler(target.get(cx.gc), event_type) {
                Some(listener) => op.ret_any(cx, cx.callback_edge(listener)),
                None => op.ret_null(),
            },
        );
    }

    /// The iframes' and script elements' steps for what the operation did
    /// to them (`frame::update_elements`), before its custom element
    /// reactions are invoked: HTML's post-connection steps come first, as
    /// Blink makes an iframe's frame, and runs a script, as it is
    /// inserted. Nothing by default: a front end with neither.
    fn update_elements(&mut self) {}
}

/// A front end's side in a call's operation: the arguments made the DOM's,
/// and the result. It calls no script and allocates nothing of its
/// engine's (whose GC may reach the DOM): what the result needs of the
/// engine it keeps for `Front::finish`.
pub trait OpFront: FrontTypes {
    /// A string argument's text, borrowed from it.
    fn str_param<'a>(&mut self, string: &'a Self::Str) -> &'a str;

    /// `this` as a `T`, if it is one: the front end's checks (V8's
    /// `Signature`) made it one of the member's interface's objects, which
    /// may be of more than one type (`ObjectType::this`).
    fn this_as<'gc, T: HeapType>(&mut self, cx: &ApiCx<'_, 'gc>) -> Option<Gc<'gc, T>>;

    /// A node argument.
    fn node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: &Self::Node) -> NodeRef<'gc>;

    /// An object argument.
    fn object<'gc, T: HeapType>(&mut self, cx: &ApiCx<'_, 'gc>, object: &Self::Node) -> Gc<'gc, T> {
        let _ = (cx, object);
        unreachable!("a front end binding a member with an object reads it")
    }

    /// An event target argument (`Front::target_arg`) as the DOM's.
    fn target<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, object: &Self::Node) -> TargetRef<'gc> {
        let _ = (cx, object);
        unreachable!("a front end binding a member with an event target reads it")
    }

    /// `edge` made to refer to the value `value` (`Front::any_arg`,
    /// `Front::dict_any`), strongly.
    fn store_any(&mut self, cx: &ApiCx<'_, '_>, edge: &JsEdge, value: AnyArg<Self::Any>) {
        let _ = (cx, edge, value);
        unreachable!("a front end binding a member with an `any` stores it")
    }

    /// Whether `edge` (a listener's) refers to the callback `callback`
    /// names (DOM's listener comparison, Blink's
    /// `JSEventListener::operator==`).
    fn same_callback(
        &mut self,
        cx: &ApiCx<'_, '_>,
        edge: &JsEdge,
        callback: Callback<Self::Callback>,
    ) -> bool {
        let _ = (cx, edge, callback);
        unreachable!("a front end binding a member with a callback compares them")
    }

    /// `edge` (a new listener's) made to refer to the callback `callback`
    /// names, strongly (the engine's minor GCs keep it).
    fn store_callback(
        &mut self,
        cx: &ApiCx<'_, '_>,
        edge: &JsEdge,
        callback: Callback<Self::Callback>,
    ) {
        let _ = (cx, edge, callback);
        unreachable!("a front end binding a member with a callback stores them")
    }

    /// `f` with the bytes of the array `edge` refers to, read in place
    /// where the engine lets them be (none if it refers to none). No
    /// default: a member's function may call it whatever its types are,
    /// so every front end says what it does.
    fn with_array_bytes(
        &mut self,
        cx: &ApiCx<'_, '_>,
        edge: &JsEdge,
        f: &mut dyn FnMut(Option<&[u8]>),
    );

    /// `edge` (a new timer's) made to refer to the rest arguments `rest`
    /// (`Front::rest_arg`), strongly.
    fn store_rest(&mut self, cx: &ApiCx<'_, '_>, edge: &JsEdge, rest: Self::Rest) {
        let _ = (cx, edge, rest);
        unreachable!("a front end binding a member with rest arguments stores them")
    }

    /// A constructor's new object: the call's own new wrapper (JS's `this`
    /// in a constructor) made its.
    fn construct_object<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, object: ObjectRef<'gc>) {
        let _ = (cx, object);
        unreachable!("a front end binding a constructor wraps its objects")
    }

    /// `HTMLElement`'s constructor's element (HTML's `[HTMLConstructor]`):
    /// a new one, the call's new wrapper made its; or one being upgraded,
    /// the call's new wrapper made its if it has none. One being upgraded
    /// that has a wrapper is given as that, the call's result, and rooted:
    /// its wrapper is given `new.target`'s prototype after the operation
    /// (`Front::finish_upgrading`). No default, as `with_array_bytes`.
    fn construct_html<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        constructed: HtmlConstructed<'gc>,
    ) -> Option<NodeRoot>;

    /// The engine's number for the constructor call's `new.target` (V8's
    /// identity hash), if it is an object: what a registry finds its
    /// definitions by (`Cx::custom_element_definitions_numbered`).
    /// No default, as `with_array_bytes`.
    fn new_target_number(&mut self) -> Option<u32>;

    /// Whether `edge` (a definition's constructor's) refers to the call's
    /// `new.target`.
    /// No default, as `with_array_bytes`.
    fn is_new_target(&mut self, cx: &ApiCx<'_, '_>, edge: &JsEdge) -> bool;

    /// The engine's number for the callback `callback` names (as
    /// `new_target_number`'s).
    fn callback_number(&mut self, callback: Callback<Self::Callback>) -> u32 {
        let _ = callback;
        unreachable!("a front end binding a member taking a constructor numbers it")
    }

    /// A custom element `createElement` is to construct, after the
    /// operation, with `definition`'s constructor, in `document`: the
    /// front end constructs it (`ScriptHost`-like, out of the operation),
    /// checks what it gave (`custom_elements::created`), and gives it.
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_construct<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        definition: Gc<'gc, dom::CustomElementDefinition>,
        document: NodeRef<'gc>,
    ) -> Option<Self::PendingNode> {
        let _ = (cx, definition, document);
        unreachable!("a front end binding `createElement` constructs custom elements")
    }

    /// The window of the realm the call runs in (its current realm's
    /// global, `Front::prepare_window`), if it is one of the DOM's: what
    /// a constructor's new object belongs to (an event's time origin).
    fn current_window<'gc>(&mut self, cx: &ApiCx<'_, 'gc>) -> Option<Gc<'gc, dom::Window>> {
        let _ = cx;
        None
    }

    /// The call's result: kept, and made the engine's value after the
    /// operation (`Front::finish`), unless it needs nothing of the engine.
    fn ret_undefined(&mut self);
    fn ret_null(&mut self);
    fn ret_bool(&mut self, value: bool);
    /// A number: given, or what is left for after the operation
    /// (`FrontTypes::PendingNumber`).
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_u32(&mut self, value: u32) -> Option<Self::PendingNumber>;
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_f64(&mut self, value: f64) -> Option<Self::PendingNumber>;
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_i32(&mut self, value: i32) -> Option<Self::PendingNumber> {
        self.ret_f64(value.into())
    }
    /// The value `edge` refers to (the engine's), undefined if none:
    /// WebIDL's `any`.
    fn ret_any<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, edge: &'gc JsEdge) {
        let _ = (cx, edge);
        unreachable!("a front end binding a member giving any value gives it")
    }
    /// As `ret_any`, null if none (`AnyOrNull`).
    fn ret_any_or_null<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, edge: &'gc JsEdge) {
        let _ = (cx, edge);
        unreachable!("a front end binding a member giving any value gives it")
    }
    /// A string: given, or what is left for after the operation
    /// (`FrontTypes::PendingString`).
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_string(&mut self, value: &DomString) -> Option<Self::PendingString>;
    /// Text computed for the call (`DomStr::Fresh`): as a string of the
    /// DOM's made of it, unless the front end takes the text itself (no
    /// string of the DOM's made, none looked up in a cache).
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_fresh_string(
        &mut self,
        value: std::borrow::Cow<'static, str>,
    ) -> Option<Self::PendingString> {
        self.ret_string(&DomString::from(value.into_owned()))
    }
    /// One of the thread's names (`DomStr::Name`), which live as long as
    /// the thread: as any string, unless the front keeps its own by number.
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_name(&mut self, id: NameId) -> Option<Self::PendingString> {
        self.ret_string(&names::string(id))
    }
    /// A node: its wrapper, if it has one; else what is left for after
    /// the operation, to make one (`FrontTypes::PendingNode`).
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_node<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        node: NodeRef<'gc>,
    ) -> Option<Self::PendingNode>;
    /// An object of any type: its wrapper, if it has one; else kept (a
    /// root) for a new one.
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_object<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: ObjectRef<'gc>,
    ) -> Option<Self::PendingObject> {
        let _ = (cx, object);
        unreachable!("a front end binding a member giving an object returns it")
    }

    /// What `edge` holds, or undefined if nothing (`AnyOrUndefined`).
    fn ret_any_or_undefined<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, edge: Option<&'gc JsEdge>) {
        let _ = (cx, edge);
        unreachable!("a front end binding a member giving any value gives it")
    }

    /// A node its member made (`NewObject`), and what was made ahead for
    /// it (`Front::prepare_new_node`): as any node by default.
    #[must_use = "what the operation couldn't give is finished after it"]
    fn ret_new_node<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        node: NodeRef<'gc>,
        prepared: Self::PreparedNode,
    ) -> Option<Self::PendingNode> {
        let _ = prepared;
        self.ret_node(cx, node)
    }
}

/// A callback argument the front end keeps, by its index (`callback_arg`):
/// what a member's function hands back to the front end to compare or
/// store it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Callback<H>(pub H);

/// An item of `Blob`'s constructor's parts as the front end took it
/// (`Front::blob_parts_arg`): a string, bytes copied, or a blob's wrapper.
#[derive(Debug, Clone)]
pub enum BlobPartArg<N> {
    String(String),
    Bytes(Vec<u8>),
    Blob(N),
}

/// A dictionary member's name as a front end reads it: its number in the
/// names table (Blink's `AtomicString`), fixed as the table is compiled
/// (`dom::names::fixed`), by which a front end finds its engine's string
/// of the name, made once (Blink's bindings keep a dictionary's keys'
/// V8 strings an isolate, `V8PerIsolateData::FindOrCreateEternalNameCache`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberName(dom::names::NameId);

impl MemberName {
    /// `name`, one of the fixed names: a dictionary's member that isn't
    /// (not in `dom`'s `dictionary_members.txt`) doesn't compile.
    pub const fn of(name: &'static str) -> Self {
        match dom::names::fixed(name) {
            Some(id) => Self(id),
            None => panic!("a dictionary member's name that isn't in dom's dictionary_members.txt"),
        }
    }

    /// The name's number.
    pub const fn id(self) -> dom::names::NameId {
        self.0
    }

    /// WebIDL's name (the table's text of the number: an error's message,
    /// an engine's string made the first time).
    pub fn name(self) -> &'static str {
        match dom::names::get(self.0) {
            dom::names::Name::Fixed(name) => name,
            dom::names::Name::Dynamic(_) => unreachable!("a member's name is a fixed one"),
        }
    }
}

impl std::fmt::Display for MemberName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A `Uint8ClampedArray` argument (`uint8_clamped_arg`): the array as the
/// front end holds it (`FrontTypes::Any`), which the member's function
/// hands back to it to store (`OpFront::store_any`), and its length in
/// bytes, as it was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uint8ClampedArg<A> {
    pub array: A,
    pub len: usize,
}

/// A dictionary member's value as a result has it (`ret_dictionary`).
#[derive(Debug, Clone, PartialEq)]
pub enum DictValue {
    Bool(bool),
    String(String),
    Number(f64),
    Null,
}

/// An `any` argument (or dictionary member) as the front end took it
/// (`any_arg`, `dict_any`: `FrontTypes::Any`): what a member's function
/// hands back to the front end to store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnyArg<A>(pub A);

/// A dictionary's `signal` (`AddEventListenerOptions`') as the front end
/// took it: an object's wrapper (`FrontTypes::Node`), which the member's
/// function has the front end make the DOM's (`OpFront::object`) and
/// checks to be an `AbortSignal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalObject<N>(pub N);

/// An init dictionary's `view` (`Window?`, null by default) as the front
/// end took it: a window's wrapper (`FrontTypes::Node`), which the
/// member's function has the front end make the DOM's
/// (`OpFront::object`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewObject<N>(pub N);

/// An init dictionary's `relatedTarget` (`EventTarget?`, null by default)
/// as the front end took it: a node's, a window's or a plain event
/// target's wrapper, made the DOM's by `OpFront::target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelatedTarget<N>(pub N);

impl<F: Front> DictHandle<F> for Option<ViewObject<F::Node>> {
    fn read(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<Self>> {
        Some(Some(
            front
                .dict_nullable_object::<dom::Window>(dict, member, "Window")?
                .map(ViewObject),
        ))
    }
}

impl<F: Front> DictHandle<F> for Option<RelatedTarget<F::Node>> {
    fn read(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<Self>> {
        Some(Some(
            front.dict_nullable_target(dict, member)?.map(RelatedTarget),
        ))
    }
}

/// `BodyInit` (`(Blob or BufferSource or URLSearchParams or USVString)`)
/// as the front end took it: a string, a buffer's bytes (a copy, as the
/// buffer was), or a blob's or a `URLSearchParams`' wrapper
/// (`FrontTypes::Node`, made the DOM's by `OpFront::object`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyArg<N> {
    String(String),
    Bytes(Vec<u8>),
    Blob(N),
    SearchParams(N),
}

/// `RequestInit`'s `body` (`BodyInit?`) given: null, or a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyMember<N> {
    Null,
    Body(BodyArg<N>),
}

/// `RequestInit`'s `signal` (`AbortSignal?`) given: null, or a signal's
/// wrapper (`FrontTypes::Node`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalMember<N> {
    Null,
    Signal(N),
}

/// A dictionary member given (`RequestInit`'s `window`): whether it is
/// null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsNull(pub bool);

/// `HeadersInit` as a dictionary's member (`RequestInit`'s, `ResponseInit`'s
/// `headers`): as `HeaderPairs` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadersMember(pub StringPairsArg);

/// What calls script for the DOM (an engine, out of any operation), and
/// runs the DOM's operations between its calls: a dispatch's listeners
/// (Blink's `JSEventListener::Invoke`), a timer's callback.
pub trait ScriptHost {
    /// A value of the engine's the host stores in the DOM's JS edges (an
    /// exception thrown, a promise rejected), as it holds it.
    type Value;

    /// Runs one operation of the DOM that allocates nothing: no safepoint,
    /// the DOM borrowed shared.
    fn read<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R;

    /// Runs one operation of the DOM: a safepoint first, the DOM borrowed
    /// whole.
    fn mutate<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R;

    /// `mutate`, with what stores the host's values in the DOM's JS edges
    /// in it (strongly), which the engine allows in an operation.
    fn mutate_storing<R>(
        &mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut dyn FnMut(&JsEdge, Self::Value)) -> R,
    ) -> R {
        let _ = f;
        unreachable!("a host reporting what script threw stores it")
    }

    /// Calls the listener at `index` of `event`'s snapshot, `this` the
    /// event's current target and the event its argument (DOM's "call a
    /// user object's operation": a function, or an object's `handleEvent`),
    /// reporting what it throws (HTML's "report the exception"), which
    /// doesn't stop the dispatch.
    fn call_listener(&mut self, event: &Persistent<dom::Event>, index: usize);

    /// Calls `traversal`'s filter with `node` (DOM's "filter", its call:
    /// a function, or an object's `acceptNode`, `this` the filter),
    /// giving its result as an `unsigned short`; what it throws, or a
    /// `TypeError` if its realm can't run script, is the error, thrown on
    /// by the member (Blink's `NodeIteratorBase::AcceptNode` rethrows).
    fn call_node_filter(
        &mut self,
        traversal: &TraversalRoot,
        node: &dom::NodeRoot,
    ) -> Result<u16, ApiError> {
        let _ = (traversal, node);
        unreachable!("a host running traversals calls their filters")
    }

    /// HTML's "getting the current value of the event handler" of
    /// `target`'s handler of `event_type`, if its listener holds an
    /// uncompiled one: compiled (its function in the listener's callback,
    /// `Cx::set_compiled_handler`); a source that doesn't compile is
    /// reported at the window and the handler's value is null (its
    /// listener stays, with no callback). None by default (a host with no
    /// script of HTML's).
    fn compile_event_handler(&mut self, target: &TargetRoot, event_type: NameId) {
        let _ = (target, event_type);
    }

    /// Runs `script`, a classic script, in the realm of the browsing
    /// context `document` is shown in (HTML's "run a classic script"),
    /// reporting what it throws there; nothing if none shows it. None by
    /// default (a host with no script of HTML's).
    fn run_classic_script(&mut self, document: &dom::NodeRoot, script: &crate::ClassicScript<'_>) {
        let _ = (document, script);
    }

    /// HTML's "fetch a module script graph" (or "fetch an inline module
    /// script graph") for a module script of `document`, in the realm of
    /// the browsing context showing it: once fetched, its pending script
    /// is told (`Cx::module_graph_fetched`). Nothing by default (a host
    /// with no modules).
    fn start_module_graph(&mut self, document: &dom::NodeRoot, start: crate::ModuleStart) {
        let _ = (document, start);
    }

    /// HTML's processing of `link`, a connected `<link rel=modulepreload>`:
    /// its module fetched into its realm's module map, its `load` or
    /// `error` after. Nothing by default.
    fn preload_module(&mut self, link: &dom::NodeRoot) {
        let _ = link;
    }

    /// HTML's "run a module script" for the graph `root` numbers, in the
    /// realm of the browsing context showing `document`: evaluated, what
    /// it throws (or its error to rethrow) reported there. Nothing by
    /// default.
    fn run_module_script(&mut self, document: &dom::NodeRoot, root: u32) {
        let _ = (document, root);
    }

    /// HTML's "create a new child navigable" for iframe `owner`: a new
    /// browsing context, the child of the one `owner`'s document is shown
    /// in, whose realm's global is a window of a new, empty HTML document
    /// (its initial `about:blank`, which the caller fills): the document.
    /// None for a host with no frames (by default).
    fn create_child_window(&mut self, owner: &dom::NodeRoot) -> Option<dom::NodeRoot> {
        let _ = owner;
        None
    }

    /// `url` stands for `data` (a blob URL `document`'s realm made, File
    /// API's blob URL store's entry): the host keeps it, for as long as the
    /// realm lives. Nothing by default.
    fn register_blob_url(&mut self, document: &dom::NodeRoot, url: &str, data: &dom::BlobData) {
        let _ = (document, url, data);
    }

    /// `url` stands for nothing now. Nothing by default.
    fn revoke_blob_url(&mut self, url: &str) {
        let _ = url;
    }

    /// The frame showing `document` is to navigate (or did, within the
    /// document: `FrameNavigation`): its host told. Nothing by default.
    fn navigate(&mut self, document: &dom::NodeRoot, navigation: dom::FrameNavigation) {
        let _ = (document, navigation);
    }

    /// `event`'s state (a `PopStateEvent`'s the UA fires) is `history`'s
    /// state as `history.state` gives it (the same object). Nothing by
    /// default.
    fn copy_history_state(
        &mut self,
        event: &Persistent<dom::Event>,
        history: &Persistent<dom::History>,
    ) {
        let _ = (event, history);
    }

    /// `owner`'s browsing context is to navigate to `navigation`: its host
    /// loads the document. Nothing by default.
    fn navigate_child(&mut self, owner: &dom::NodeRoot, navigation: crate::ChildNavigation) {
        let _ = (owner, navigation);
    }

    /// `document`, a child browsing context's, went with its iframe: the
    /// browsing context discarded, with those in it. Nothing by default.
    fn discard_child(&mut self, document: &dom::NodeRoot) {
        let _ = document;
    }

    /// A message's serialization deserialized in `window`'s realm (HTML's
    /// `StructuredDeserializeWithTransfer`), its ports `ports`' objects:
    /// the value, or none if it can't be (then `messageerror`). None by
    /// default: a host with no messages.
    fn deserialize_message(
        &mut self,
        window: &Persistent<dom::Window>,
        payload: dom::SerializedMessage,
        ports: &[Persistent<dom::EventTargetImpl>],
    ) -> Option<Self::Value> {
        let _ = (window, payload, ports);
        None
    }

    /// HTML's "perform a microtask checkpoint" between the events the user
    /// agent dispatches one after another (its "clean up after running
    /// script", the stack empty after each), which a host that runs none
    /// leaves out.
    fn perform_microtask_checkpoint(&mut self) {}

    /// Calls animation frame callback `frame` of `window` with `time` (its
    /// argument, a `DOMHighResTimeStamp`), `this` undefined (WebIDL's
    /// "invoke a callback function"), reporting what it throws.
    fn call_animation_frame(
        &mut self,
        window: &Persistent<dom::Window>,
        frame: &Persistent<dom::AnimationFrame>,
        time: f64,
    ) {
        let _ = (window, frame, time);
        unreachable!("a host running animation frames calls their callbacks")
    }

    /// Calls `timer`'s callback (HTML's timer task: its arguments the
    /// timer's, `this` its window, `window`), reporting what it throws; no
    /// microtask checkpoint after (the caller's, once the timer's nesting
    /// level is back to 0).
    fn call_timer(&mut self, window: &Persistent<dom::Window>, timer: &Persistent<dom::Timer>);

    /// Calls `observer`'s callback with `records` (a new array) and the
    /// observer, `this` the observer (DOM's "notify mutation observers"),
    /// reporting what it throws.
    fn call_mutation_callback(
        &mut self,
        observer: &Persistent<dom::MutationObserver>,
        records: &[Persistent<dom::MutationRecord>],
    );

    /// Calls `observer`'s callback with `entries` (a new array) and the
    /// observer, `this` the observer (Intersection Observer's "notify
    /// intersection observers"), reporting what it throws.
    fn call_intersection_callback(
        &mut self,
        observer: &Persistent<dom::IntersectionObserver>,
        entries: &[Persistent<dom::IntersectionObserverEntry>],
    ) {
        let _ = (observer, entries);
        unreachable!("a host running the intersection observers' task calls them")
    }

    /// Calls `observer`'s callback with `list`, the observer and the
    /// options (`droppedEntriesCount` if given), `this` the observer
    /// (Performance Timeline's "queue the PerformanceObserver task"'s
    /// steps), reporting what it throws.
    fn call_performance_callback(
        &mut self,
        observer: &Persistent<dom::PerformanceObserver>,
        list: &Persistent<dom::PerformanceObserverEntryList>,
        dropped: Option<u32>,
    ) {
        let _ = (observer, list, dropped);
        unreachable!("a host running the performance timeline's task calls them")
    }

    /// Calls an idle callback with `deadline`, `this` undefined (HTML's
    /// "invoke idle callbacks"), reporting what it throws.
    fn call_idle_callback(
        &mut self,
        callback: &Persistent<dom::IdleCallback>,
        deadline: &Persistent<dom::IdleDeadline>,
    ) {
        let _ = (callback, deadline);
        unreachable!("a host running idle callbacks calls them")
    }

    /// Calls a geolocation request's error callback, if it has one, with
    /// `error`, `this` undefined, reporting what it throws.
    fn call_geolocation_error(
        &mut self,
        request: &Persistent<dom::GeoRequest>,
        error: &Persistent<dom::GeolocationPositionError>,
    ) {
        let _ = (request, error);
        unreachable!("a host running geolocation's tasks calls its callbacks")
    }

    /// The DOM's pending promise numbered `promise` (its resolver the
    /// engine's) resolved with `object`'s wrapper.
    fn resolve_promise_with(&mut self, promise: u64, object: &ObjectRoot) {
        let _ = (promise, object);
        unreachable!("a host settling a promise with an object resolves it")
    }

    /// The DOM's pending promise numbered `promise` (a blob read's)
    /// resolved with `read`'s string or bytes.
    fn resolve_promise_with_read(&mut self, promise: u64, read: dom::BlobRead) {
        let _ = (promise, read);
        unreachable!("a host reading blobs resolves their promises")
    }

    /// What the input waiting for the main thread is for (the engine's
    /// `InputMonitor`'s answer); none without one.
    fn pending_input(&mut self, include_continuous: bool) -> Vec<crate::PendingInput> {
        let _ = include_continuous;
        Vec::new()
    }

    /// The DOM's pending promise numbered `promise` rejected with
    /// `signal`'s abort reason (an aborted `fetch()`'s).
    fn reject_promise_with_reason(&mut self, promise: u64, signal: &Persistent<EventTargetImpl>) {
        let _ = (promise, signal);
        unreachable!("a host aborting a fetch rejects its promise")
    }

    /// `signal`'s abort reason a new `DOMException` of `name` and
    /// `message`, of its realm (an `AbortSignal.timeout()`'s, as it fires).
    fn set_abort_reason(
        &mut self,
        signal: &Persistent<EventTargetImpl>,
        name: crate::ExceptionName,
        message: &str,
    ) {
        let _ = (signal, name, message);
        unreachable!("a host running native timers makes abort reasons")
    }

    /// Calls `observer`'s callback with `entries` (a new array) and the
    /// observer, `this` the observer (Resize Observer's "broadcast active
    /// resize observations"), reporting what it throws.
    fn call_resize_callback(
        &mut self,
        observer: &Persistent<dom::ResizeObserver>,
        entries: &[Persistent<dom::ResizeObserverEntry>],
    ) {
        let _ = (observer, entries);
        unreachable!("a host running the rendering update's resize observers calls them")
    }

    /// Calls a `toBlob` call's callback with its blob, or null
    /// (`Cx::canvas_blob_parts`), `this` undefined (WebIDL's "invoke a
    /// callback function"), reporting what it throws.
    fn call_blob_callback(&mut self, callback: &Persistent<dom::CanvasBlobCallback>) {
        let _ = callback;
        unreachable!("a host running a canvas's element tasks calls `toBlob`'s callbacks")
    }

    /// HTML's `define` steps on its constructor (the call's callback
    /// argument): `IsConstructor`, its `prototype` an object, the
    /// prototype's lifecycle callbacks (each a function or undefined),
    /// `observedAttributes` (with an `attributeChangedCallback`),
    /// `disabledFeatures`, `formAssociated`; the callbacks kept for
    /// `store_custom_element_definition`. What it throws, an error.
    fn read_custom_element_constructor(&mut self) -> Result<ConstructorInfo, ApiError> {
        unreachable!("a host defining custom elements reads their constructors")
    }

    /// `definition`'s edges given the constructor and the callbacks
    /// `read_custom_element_constructor` kept, and `registry` the engine's
    /// number for the constructor.
    fn store_custom_element_definition(
        &mut self,
        registry: &Persistent<dom::CustomElementRegistry>,
        definition: &Persistent<dom::CustomElementDefinition>,
    ) {
        let _ = (registry, definition);
        unreachable!("a host defining custom elements stores their callbacks")
    }

    /// `definition`'s lifecycle callback `callback` invoked on `element`
    /// (`this`), with its arguments; what it throws reported at its realm.
    fn call_lifecycle_callback(
        &mut self,
        element: &Persistent<dom::Node>,
        definition: &Persistent<dom::CustomElementDefinition>,
        callback: crate::custom_elements::LifecycleCallback,
    ) {
        let _ = (element, definition, callback);
        unreachable!("a host invoking custom element reactions calls their callbacks")
    }

    /// The notices queued for `object`'s promises
    /// (`dom::Cx::take_animation_notices`), applied in order to the
    /// promises its animation keeps (`animation::notice_steps`): resolved
    /// with `object`, or rejected; let go of, the next one asked for new.
    fn settle_animation_notices(&mut self, object: &Persistent<dom::EventTargetImpl>) {
        let _ = object;
        unreachable!("a host running animations settles their promises")
    }

    /// The promises numbered `promises` (kept by the engine: `decode()`'s)
    /// resolved with undefined, or rejected with `error`.
    fn settle_promises(&mut self, promises: &[u64], error: Option<ApiError>) {
        let _ = (promises, error);
        unreachable!("a host running image tasks settles their promises")
    }

    /// HTML's upgrade's construction: `definition`'s constructor
    /// constructed (`element` on its construction stack, which
    /// `HTMLElement`'s constructor gives), and checked to have given
    /// `element`; whether it did. What it threw (a `TypeError` if it gave
    /// another object) is reported, at the constructor's realm.
    fn upgrade_element(
        &mut self,
        definition: &Persistent<dom::CustomElementDefinition>,
        element: &Persistent<dom::Node>,
    ) -> bool {
        let _ = (definition, element);
        unreachable!("a host upgrading custom elements constructs them")
    }

    /// Whether the call's callback argument is a constructor (ECMAScript's
    /// `IsConstructor`).
    fn constructor_is_constructor(&mut self) -> bool {
        unreachable!("a host defining custom elements checks their constructors")
    }

    /// Whether the call's callback argument is the constructor of one of
    /// `registry`'s definitions.
    fn constructor_is_defined(
        &mut self,
        registry: &Persistent<dom::CustomElementRegistry>,
    ) -> bool {
        let _ = registry;
        unreachable!("a host defining custom elements compares their constructors")
    }

    /// The `whenDefined` promises numbered `pending` resolved with
    /// `definition`'s constructor.
    fn resolve_when_defined(
        &mut self,
        pending: &[u64],
        definition: &Persistent<dom::CustomElementDefinition>,
    ) {
        let _ = (pending, definition);
        unreachable!("a host defining custom elements resolves their promises")
    }
}

/// What HTML's `define` reads off a constructor
/// (`ScriptHost::read_custom_element_constructor`), besides the callbacks
/// the host keeps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConstructorInfo {
    /// `observedAttributes`, read only with an `attributeChangedCallback`.
    pub observed_attributes: Vec<String>,
    /// `disabledFeatures`.
    pub disabled_features: Vec<String>,
    /// `formAssociated`.
    pub form_associated: bool,
}

/// A member's front end as the script host of the member's function (a
/// member that calls script).
pub struct FrontHost<'a, F>(pub &'a mut F);

impl<F: Front> ScriptHost for FrontHost<'_, F> {
    /// None: a member's call reports nothing in its operations.
    type Value = std::convert::Infallible;

    fn read<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R {
        self.0.read(|cx, _| f(cx))
    }

    fn mutate<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R {
        self.0.mutate(|cx, _| f(cx))
    }

    fn call_listener(&mut self, event: &Persistent<dom::Event>, index: usize) {
        self.0.call_listener(event, index);
    }

    fn call_node_filter(
        &mut self,
        traversal: &TraversalRoot,
        node: &dom::NodeRoot,
    ) -> Result<u16, ApiError> {
        self.0.call_node_filter(traversal, node)
    }

    fn compile_event_handler(&mut self, target: &TargetRoot, event_type: NameId) {
        self.0.compile_event_handler(target, event_type);
    }

    fn call_timer(&mut self, _: &Persistent<dom::Window>, _: &Persistent<dom::Timer>) {
        unreachable!("timers run out of any member's call");
    }

    fn call_mutation_callback(
        &mut self,
        observer: &Persistent<dom::MutationObserver>,
        records: &[Persistent<dom::MutationRecord>],
    ) {
        self.0.call_mutation_callback(observer, records);
    }

    fn read_custom_element_constructor(&mut self) -> Result<ConstructorInfo, ApiError> {
        self.0.read_custom_element_constructor()
    }

    fn store_custom_element_definition(
        &mut self,
        registry: &Persistent<dom::CustomElementRegistry>,
        definition: &Persistent<dom::CustomElementDefinition>,
    ) {
        self.0.store_custom_element_definition(registry, definition);
    }

    fn upgrade_element(
        &mut self,
        definition: &Persistent<dom::CustomElementDefinition>,
        element: &Persistent<dom::Node>,
    ) -> bool {
        self.0.upgrade_element(definition, element)
    }

    fn call_lifecycle_callback(
        &mut self,
        element: &Persistent<dom::Node>,
        definition: &Persistent<dom::CustomElementDefinition>,
        callback: crate::custom_elements::LifecycleCallback,
    ) {
        self.0
            .call_lifecycle_callback(element, definition, callback);
    }

    fn constructor_is_constructor(&mut self) -> bool {
        self.0.constructor_is_constructor()
    }

    fn constructor_is_defined(
        &mut self,
        registry: &Persistent<dom::CustomElementRegistry>,
    ) -> bool {
        self.0.constructor_is_defined(registry)
    }

    fn resolve_when_defined(
        &mut self,
        pending: &[u64],
        definition: &Persistent<dom::CustomElementDefinition>,
    ) {
        self.0.resolve_when_defined(pending, definition);
    }

    fn pending_input(&mut self, include_continuous: bool) -> Vec<crate::PendingInput> {
        self.0.pending_input(include_continuous)
    }

    fn reject_promise_with_reason(&mut self, promise: u64, signal: &Persistent<EventTargetImpl>) {
        self.0.reject_promise_with_reason(promise, signal);
    }
}

/// A value of an operation rooted, for a member that calls script (whose
/// function runs out of the operation).
pub trait Root<'gc> {
    type Rooted;
    fn root(self, cx: &ApiCx<'_, 'gc>) -> Self::Rooted;
}

impl<'gc, T: HeapType> Root<'gc> for Gc<'gc, T> {
    type Rooted = Persistent<T>;

    fn root(self, cx: &ApiCx<'_, 'gc>) -> Persistent<T> {
        Persistent::new(cx.gc, self)
    }
}

impl<'gc> Root<'gc> for TargetRef<'gc> {
    type Rooted = TargetRoot;

    fn root(self, cx: &ApiCx<'_, 'gc>) -> TargetRoot {
        cx.root_target(self)
    }
}

/// What a member that may leave script to do (`#[then_script]`) left,
/// rooted: done out of its operation, with the front end as the script
/// host, before the call returns (Blink's steps after a DOM change that
/// dispatch events, as `FrameSelection::SetFocusedNodeIfNeeded`).
pub trait ThenScript {
    fn run<H: ScriptHost>(self, host: &mut H);
}

/// A member that calls script's result, which its function gives out of
/// any operation, as its result type takes it in the operation that gives
/// it: a node as its root (`NodeRoot`), anything else as it is.
pub trait Unroot<'gc, T> {
    fn unroot(self, cx: &ApiCx<'_, 'gc>) -> T;
}

impl<'gc, T> Unroot<'gc, T> for T {
    fn unroot(self, _: &ApiCx<'_, 'gc>) -> T {
        self
    }
}

impl<'gc> Unroot<'gc, Option<NodeRef<'gc>>> for Option<NodeRoot> {
    fn unroot(self, cx: &ApiCx<'_, 'gc>) -> Option<NodeRef<'gc>> {
        self.map(|node| node.get(cx.gc))
    }
}

/// A string argument of a member that calls script: a copy.
impl<'gc> Root<'gc> for &str {
    type Rooted = String;

    fn root(self, _: &ApiCx<'_, 'gc>) -> String {
        self.to_owned()
    }
}

/// A callback argument of a member that calls script: the front end keeps
/// the callback for the call.
impl<'gc, H> Root<'gc> for Callback<H> {
    type Rooted = Callback<H>;

    fn root(self, _: &ApiCx<'_, 'gc>) -> Callback<H> {
        self
    }
}

/// A dictionary argument of a member that calls script: a copy, plain
/// values the heap doesn't hold.
impl<'gc, D: Dictionary + Clone> Root<'gc> for &D {
    type Rooted = D;

    fn root(self, _: &ApiCx<'_, 'gc>) -> D {
        self.clone()
    }
}

/// Drops a taken argument (`Copy` or not), as a call does once its
/// operation is over: a string argument may lend the engine's own
/// characters, which the engine's GC may not move while they are lent.
#[inline]
pub fn release<T>(_: T) {}

/// How a type of the table crosses a call as an argument: what is taken
/// from the front end before the operation (`Taken`), and what the
/// member's function gets of it in the operation (`Param`).
pub trait ArgType {
    const DESC: TypeDesc;
    /// Whether undefined converts to the type's default, so that an
    /// argument of it may be optional (`name?: Type` in the table): a
    /// dictionary's, whose members' defaults WebIDL's `= {}` gives.
    const UNDEFINED_IS_DEFAULT: bool = false;
    /// Whether it is taken and read through the methods every front end
    /// has alone (`Front`'s and `OpFront`'s with no default, and the ones
    /// whose default calls only those): a string or a node. A member
    /// whose arguments and result all are is `TypedMember::PLAIN`.
    const PLAIN: bool = false;
    type Taken<T: FrontTypes>;
    /// What the member's function gets of it, in the operation: the DOM's
    /// own (a `&str`, a `NodeRef`, a number), or the front end `T`'s
    /// handle, given back to it as it is (`Callback`).
    type Param<'a, 'gc, T: FrontTypes + 'a>;

    /// Argument `index`, before the operation; `None` if it threw.
    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>>;

    /// The interface this argument foresees the member's new node to be
    /// of, if it does (`createElement`'s local name): what the result's
    /// `RetType::prepare` is given.
    #[inline(always)]
    fn foreseen_interface<F: FrontTypes>(taken: &Self::Taken<F>) -> Option<usize> {
        let _ = taken;
        None
    }

    /// What the function gets of `taken`, in the operation (argument
    /// `index`), through the front end's side there.
    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Self::Taken<O>,
        index: usize,
    ) -> Result<Self::Param<'a, 'gc, O>, ApiError>;
}

/// How a type of the table crosses a call as a result: what the member's
/// function returns (`Ret`), handed back in the operation.
pub trait RetType {
    const DESC: TypeDesc;
    /// As `ArgType::PLAIN`: nothing made before the operation, and what
    /// it gives a number, a boolean, a string, a node, null or nothing.
    const PLAIN: bool = false;
    type Ret<'gc>;

    /// What the front end makes ahead of the operation for it, as the
    /// front end `F` names it (`FrontTypes::PreparedNode`): nothing (`()`)
    /// for most.
    type Prepared<F: FrontTypes>;

    /// Before the operation, the arguments taken: what the front end
    /// makes ahead of it. `foreseen` is the interface an argument foresaw
    /// the result to be of, if one did (`ArgType::foreseen_interface`).
    fn prepare<F: Front>(front: &mut F, foreseen: Option<usize>) -> Self::Prepared<F>;

    /// What it leaves for after the operation, as the front end `F` names
    /// it (`FrontTypes::PendingString`...): nothing (`()`) for a result
    /// that is always given in the operation or kept by the front end.
    type Pending<F: FrontTypes>;

    /// The function's result handed to the front end, in the operation:
    /// what is left of it to do after.
    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Self::Ret<'gc>,
    ) -> Self::Pending<O>;

    /// `give`, with what `prepare` made: dropped, unless the type takes it.
    #[inline(always)]
    fn give_prepared<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Self::Ret<'gc>,
        prepared: Self::Prepared<O>,
    ) -> Self::Pending<O> {
        let _ = prepared;
        Self::give(op, cx, value)
    }

    /// After the operation: what `give` left, finished.
    fn finish<F: Front>(front: &mut F, pending: Self::Pending<F>);

    /// The member's function failed: thrown (a promise's, rejected).
    fn fail<F: Front>(front: &mut F, error: ApiError) {
        front.throw(error);
    }
}

/// WebIDL's `[NewObject]` of interface `I`: a node the member always makes,
/// whose wrapper the front end makes ahead of the operation (Blink makes
/// the node, then the wrapper: Oilpan collects anywhere, machopo's heap between
/// operations only), of interface `L`, the one its nodes most likely are
/// (`NewObject<Element, HTMLElement>` for `createElement`): a node of
/// another is wrapped after.
pub struct NewObject<I, L = I>(PhantomData<(I, L)>);

impl<I: Interface, L: Interface> RetType for NewObject<I, L> {
    const DESC: TypeDesc = TypeDesc::Node(I::NAME);
    const PLAIN: bool = true;
    type Ret<'gc> = NodeRef<'gc>;
    type Prepared<F: FrontTypes> = F::PreparedNode;
    type Pending<F: FrontTypes> = Option<F::PendingNode>;

    #[inline(always)]
    fn prepare<F: Front>(front: &mut F, _: Option<usize>) -> F::PreparedNode {
        front.prepare_new_node(L::INDEX)
    }

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: NodeRef<'gc>,
    ) -> Option<O::PendingNode> {
        op.ret_node(cx, value)
    }

    #[inline(always)]
    fn give_prepared<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: NodeRef<'gc>,
        prepared: O::PreparedNode,
    ) -> Option<O::PendingNode> {
        op.ret_new_node(cx, value, prepared)
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, pending: Option<F::PendingNode>) {
        if let Some(pending) = pending {
            front.finish_node(pending);
        }
    }
}

/// An `ImageData` its member makes (`ImageDataWithPixels`): the object,
/// as `ImageData` gives one, and its pixels, carried out of the operation
/// for its array to be made of them (`Front::finish_image_data_pixels`)
/// before the object is given.
pub struct NewImageData;

impl RetType for NewImageData {
    const DESC: TypeDesc = <crate::interfaces::ImageData as RetType>::DESC;
    const PLAIN: bool = true;
    type Ret<'gc> = ImageDataWithPixels<'gc>;
    type Prepared<F: FrontTypes> = <crate::interfaces::ImageData as RetType>::Prepared<F>;
    type Pending<F: FrontTypes> = (
        Persistent<dom::ImageData>,
        Vec<u8>,
        <crate::interfaces::ImageData as RetType>::Pending<F>,
    );

    fn prepare<F: Front>(front: &mut F, foreseen: Option<usize>) -> Self::Prepared<F> {
        <crate::interfaces::ImageData as RetType>::prepare(front, foreseen)
    }

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: ImageDataWithPixels<'gc>,
    ) -> Self::Pending<O> {
        (
            Persistent::new(cx.gc, value.image_data),
            value.pixels,
            <crate::interfaces::ImageData as RetType>::give(op, cx, value.image_data),
        )
    }

    fn finish<F: Front>(front: &mut F, (image_data, pixels, object): Self::Pending<F>) {
        front.finish_image_data_pixels(image_data, pixels);
        <crate::interfaces::ImageData as RetType>::finish(front, object);
    }
}

/// Nothing: a method that returns nothing (WebIDL's `undefined`).
impl RetType for () {
    const DESC: TypeDesc = TypeDesc::Undefined;
    const PLAIN: bool = true;
    type Ret<'gc> = ();
    no_pending!();

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, (): ()) {
        op.ret_undefined();
    }
}

impl RetType for bool {
    const DESC: TypeDesc = TypeDesc::Bool;
    const PLAIN: bool = true;
    type Ret<'gc> = bool;
    no_pending!();

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, value: bool) {
        op.ret_bool(value);
    }
}

/// WebIDL's `unsigned long` (and `unsigned short`, which no value of the
/// DOM's overflows).
impl RetType for u32 {
    const DESC: TypeDesc = TypeDesc::U32;
    const PLAIN: bool = true;
    type Ret<'gc> = u32;
    pending!(PendingNumber, finish_number);

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, value: u32) -> Self::Pending<O> {
        op.ret_u32(value)
    }
}

/// `boolean` as an argument: `ToBoolean`, which makes undefined `false`,
/// so an optional one is one whose default is `false` (WebIDL's
/// `optional boolean x = false`).
impl ArgType for bool {
    const DESC: TypeDesc = TypeDesc::Bool;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = bool;
    type Param<'a, 'gc, T: FrontTypes + 'a> = bool;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<bool> {
        Some(front.bool_arg(index))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a bool,
        _: usize,
    ) -> Result<bool, ApiError> {
        Ok(*taken)
    }
}

/// An optional `boolean` with no default (`toggle`'s `force`): none if it
/// is missing or undefined (WebIDL's "not present").
impl ArgType for Option<bool> {
    const DESC: TypeDesc = TypeDesc::OptionalBool;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<bool>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<bool>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<bool>> {
        Some((!front.is_undefined(index)).then(|| front.bool_arg(index)))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<bool>,
        _: usize,
    ) -> Result<Option<bool>, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's union of dictionary `D` and `boolean` (`(AddEventListenerOptions
/// or boolean)`): an object, null or undefined is the dictionary (WebIDL's
/// union conversion), anything else the `boolean`.
#[derive(Debug, Clone, PartialEq)]
pub enum DictOrBool<D> {
    Dict(D),
    Bool(bool),
}

/// A dictionary of the table (`declare!` implements it): read from
/// argument `index` before the operation.
pub trait Dictionary {
    const NAME: &'static str;
    /// Whether its members are all read as values (`DictMember::PLAIN`)
    /// or as the front end's handles (`DictHandle`): then it is `PLAIN`
    /// as an argument.
    const PLAIN: bool;
    /// The dictionary as read by a front end `T`: itself, or (one with
    /// members that are the front end's handles) itself of `T`'s types
    /// of them, which `T`'s side in the operation shares.
    type Of<T: FrontTypes>;

    /// Argument `index` as the dictionary (its defaults for null or
    /// undefined); `None` if it threw.
    fn read<F: Front>(front: &mut F, index: usize) -> Option<Self::Of<F>>;
}

impl<D: Dictionary + 'static> ArgType for DictOrBool<D> {
    const DESC: TypeDesc = TypeDesc::DictOrBool(D::NAME);
    const PLAIN: bool = D::PLAIN;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = DictOrBool<D::Of<T>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a DictOrBool<D::Of<T>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<DictOrBool<D::Of<F>>> {
        if front.is_dictionary_like(index) {
            D::read(front, index).map(DictOrBool::Dict)
        } else {
            Some(DictOrBool::Bool(front.bool_arg(index)))
        }
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a DictOrBool<D::Of<O>>,
        _: usize,
    ) -> Result<&'a DictOrBool<D::Of<O>>, ApiError> {
        Ok(taken)
    }
}

/// WebIDL's `unsigned short`.
impl RetType for u16 {
    const DESC: TypeDesc = TypeDesc::U16;
    const PLAIN: bool = true;
    type Ret<'gc> = u16;
    pending!(PendingNumber, finish_number);

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, value: u16) -> Self::Pending<O> {
        op.ret_u32(value.into())
    }
}

/// WebIDL's `unsigned short` as an argument: `ToUint16`, `ToUint32`'s low
/// bits.
impl ArgType for u16 {
    const DESC: TypeDesc = TypeDesc::U16;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = u16;
    type Param<'a, 'gc, T: FrontTypes + 'a> = u16;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<u16> {
        front
            .u32_arg(index)
            .map(|value| u16::try_from(value & 0xFFFF).expect("the low 16 bits"))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a u16,
        _: usize,
    ) -> Result<u16, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's `short`.
impl RetType for i16 {
    const DESC: TypeDesc = TypeDesc::I16;
    const PLAIN: bool = true;
    type Ret<'gc> = i16;
    pending!(PendingNumber, finish_number);

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, value: i16) -> Self::Pending<O> {
        op.ret_i32(value.into())
    }
}

/// `createNodeIterator`'s and `createTreeWalker`'s `whatToShow`: an
/// `unsigned long` whose default is `0xFFFFFFFF` (`NodeFilter.SHOW_ALL`).
pub struct WhatToShow;

impl ArgType for WhatToShow {
    const DESC: TypeDesc = TypeDesc::U32;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = u32;
    type Param<'a, 'gc, T: FrontTypes + 'a> = u32;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<u32> {
        if front.is_undefined(index) {
            return Some(u32::MAX);
        }
        front.u32_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a u32,
        _: usize,
    ) -> Result<u32, ApiError> {
        Ok(*taken)
    }
}

/// What a `StaticRangeInitArg` is in the operation: its containers found.
pub struct StaticRangeInit<'gc> {
    pub start: (NodeRef<'gc>, u32),
    pub end: (NodeRef<'gc>, u32),
}

/// `StaticRange`'s init, a dictionary with node members the front end
/// converts before the operation: its handles, made the DOM's in it.
pub struct StaticRangeInitArg;

impl ArgType for StaticRangeInitArg {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Dictionary("StaticRangeInit");
    type Taken<T: FrontTypes> = [(T::Node, u32); 2];
    type Param<'a, 'gc, T: FrontTypes + 'a> = StaticRangeInit<'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<[(F::Node, u32); 2]> {
        front.static_range_init(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        [start, end]: &'a [(O::Node, u32); 2],
        _: usize,
    ) -> Result<StaticRangeInit<'gc>, ApiError> {
        Ok(StaticRangeInit {
            start: (op.node(cx, &start.0), start.1),
            end: (op.node(cx, &end.0), end.1),
        })
    }
}

/// WebIDL's `long`: `ToInt32`, which makes undefined 0, so an optional
/// one is one whose default is 0 (`optional long timeout = 0`).
impl ArgType for i32 {
    const DESC: TypeDesc = TypeDesc::I32;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = i32;
    type Param<'a, 'gc, T: FrontTypes + 'a> = i32;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<i32> {
        front.i32_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a i32,
        _: usize,
    ) -> Result<i32, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's `unsigned long`: `ToUint32`, which makes undefined 0.
impl ArgType for u32 {
    const DESC: TypeDesc = TypeDesc::U32;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = u32;
    type Param<'a, 'gc, T: FrontTypes + 'a> = u32;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<u32> {
        front.u32_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a u32,
        _: usize,
    ) -> Result<u32, ApiError> {
        Ok(*taken)
    }
}

/// An optional `unsigned long` with no default (`setRangeText`'s `start`
/// and `end`): none if it is missing or undefined (WebIDL's "not
/// present", which the overload of fewer arguments is).
impl ArgType for Option<u32> {
    const DESC: TypeDesc = TypeDesc::U32;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<u32>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<u32>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<u32>> {
        if front.is_undefined(index) {
            return Some(None);
        }
        front.u32_arg(index).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<u32>,
        _: usize,
    ) -> Result<Option<u32>, ApiError> {
        Ok(*taken)
    }
}

/// The `long` of an overload of one argument more (`select.remove()` and
/// `remove(index)`): none if it isn't given, else `ToInt32` (of undefined
/// too, as WebIDL's overload resolution counts the arguments).
impl ArgType for Option<i32> {
    const DESC: TypeDesc = TypeDesc::I32;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<i32>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<i32>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<i32>> {
        if front.arg_count() <= index {
            return Some(None);
        }
        front.i32_arg(index).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<i32>,
        _: usize,
    ) -> Result<Option<i32>, ApiError> {
        Ok(*taken)
    }
}

/// An optional `long` defaulting to -1 (`insertRow(index)`'s): missing or
/// undefined is -1, anything else `ToInt32`.
pub struct LongDefaultMinusOne;

impl ArgType for LongDefaultMinusOne {
    const DESC: TypeDesc = TypeDesc::I32;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = i32;
    type Param<'a, 'gc, T: FrontTypes + 'a> = i32;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<i32> {
        if front.is_undefined(index) {
            return Some(-1);
        }
        front.i32_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a i32,
        _: usize,
    ) -> Result<i32, ApiError> {
        Ok(*taken)
    }
}

/// An optional `long` defaulting to 1 (`stepUp(n)`'s): missing or
/// undefined is 1, anything else `ToInt32`.
pub struct LongDefaultOne;

impl ArgType for LongDefaultOne {
    const DESC: TypeDesc = TypeDesc::I32;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = i32;
    type Param<'a, 'gc, T: FrontTypes + 'a> = i32;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<i32> {
        if front.is_undefined(index) {
            return Some(1);
        }
        front.i32_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a i32,
        _: usize,
    ) -> Result<i32, ApiError> {
        Ok(*taken)
    }
}

/// An optional `unsigned short` defaulting to 302 (`Response.redirect()`'s
/// status): missing or undefined is 302, anything else `ToUint16`.
pub struct U16Default302;

impl ArgType for U16Default302 {
    const DESC: TypeDesc = TypeDesc::U16;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = u16;
    type Param<'a, 'gc, T: FrontTypes + 'a> = u16;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<u16> {
        if front.is_undefined(index) {
            return Some(302);
        }
        <u16 as ArgType>::take(front, index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a u16,
        _: usize,
    ) -> Result<u16, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's `unsigned long?` (`selectionStart`'s): null or undefined is
/// none, anything else `ToUint32`.
pub struct NullableU32;

impl ArgType for NullableU32 {
    const DESC: TypeDesc = TypeDesc::NullableU32;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Option<u32>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<u32>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<u32>> {
        if front.is_null_or_undefined(index) {
            return Some(None);
        }
        front.u32_arg(index).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<u32>,
        _: usize,
    ) -> Result<Option<u32>, ApiError> {
        Ok(*taken)
    }
}

impl RetType for NullableU32 {
    const DESC: TypeDesc = TypeDesc::NullableU32;
    const PLAIN: bool = true;
    type Ret<'gc> = Option<u32>;
    pending!(PendingNumber, finish_number);

    fn give<'gc, O: OpFront>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        value: Option<u32>,
    ) -> Self::Pending<O> {
        match value {
            Some(value) => op.ret_u32(value),
            None => {
                op.ret_null();
                None
            }
        }
    }
}

impl RetType for i32 {
    const DESC: TypeDesc = TypeDesc::I32;
    const PLAIN: bool = true;
    type Ret<'gc> = i32;
    pending!(PendingNumber, finish_number);

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, value: i32) -> Self::Pending<O> {
        op.ret_i32(value)
    }
}

/// HTML's `TimerHandler` (`(Function or TrustedScript or DOMString)`): a
/// function, or a string the timer runs as a classic script (no Trusted
/// Types yet: any other value converts to the string).
pub struct TimerHandler;

impl ArgType for TimerHandler {
    const DESC: TypeDesc = TypeDesc::Function("TimerHandler");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Callback<T::Callback>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
        front.timer_handler_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Callback<O::Callback>,
        _: usize,
    ) -> Result<Callback<O::Callback>, ApiError> {
        Ok(*taken)
    }
}

/// HTML's `EventHandler`: as an argument, an object (a function, or any
/// object: `[LegacyTreatNonObjectAsNull]`) or null; as a result, the
/// handler's listener, rooted by a getter that calls script (its source
/// compiled first), whose callback is given, or null.
pub struct EventHandler;

impl ArgType for EventHandler {
    const DESC: TypeDesc = TypeDesc::Function("EventHandler");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Option<Callback<T::Callback>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<Callback<T::Callback>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<Callback<F::Callback>>> {
        Some(front.event_handler_arg(index))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<Callback<O::Callback>>,
        _: usize,
    ) -> Result<Option<Callback<O::Callback>>, ApiError> {
        Ok(*taken)
    }
}

/// An event handler as its getter finds it (`EventHandler`'s result):
/// none (null), its listener, whose callback is the value, or one whose
/// source its engine hasn't compiled yet (a content attribute's), on its
/// target.
pub enum HandlerValue<'gc> {
    None,
    Listener(Gc<'gc, dom::Listener>),
    Uncompiled(TargetRef<'gc>, NameId),
}

/// HTML's "getting the current value of the event handler": the callback
/// its listener holds, given in the operation; an uncompiled one is left
/// for after it, where the front end compiles it (script) and gives what
/// it is then (`Front::finish_handler`).
impl RetType for EventHandler {
    const DESC: TypeDesc = TypeDesc::Function("EventHandler");
    const PLAIN: bool = true;
    type Ret<'gc> = HandlerValue<'gc>;
    no_prepared!();
    type Pending<F: FrontTypes> = Option<(TargetRoot, NameId)>;

    #[inline(always)]
    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: HandlerValue<'gc>,
    ) -> Option<(TargetRoot, NameId)> {
        match value {
            HandlerValue::None => op.ret_null(),
            HandlerValue::Listener(listener) => op.ret_any(cx, cx.callback_edge(listener)),
            HandlerValue::Uncompiled(target, event_type) => {
                return Some((cx.root_target(target), event_type));
            }
        }
        None
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, pending: Option<(TargetRoot, NameId)>) {
        if let Some((target, event_type)) = pending {
            front.finish_handler(&target, event_type);
        }
    }
}

/// Intersection Observer's `IntersectionObserverCallback`, as
/// `MutationCallback`.
pub struct IntersectionObserverCallback;

impl ArgType for IntersectionObserverCallback {
    const DESC: TypeDesc = TypeDesc::Function("IntersectionObserverCallback");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Callback<T::Callback>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
        front.function_arg(index, "IntersectionObserverCallback")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Callback<O::Callback>,
        _: usize,
    ) -> Result<Callback<O::Callback>, ApiError> {
        Ok(*taken)
    }
}

/// An `IntersectionObserverInit` as the front end read it (its `root`
/// kept: whether there is one).
#[derive(Debug, Clone, PartialEq)]
pub struct IntersectionInit {
    pub root_margin: String,
    pub scroll_margin: String,
    pub thresholds: Vec<f64>,
    pub delay: f64,
    pub track_visibility: bool,
}

impl Default for IntersectionInit {
    fn default() -> Self {
        Self {
            root_margin: "0px".to_owned(),
            scroll_margin: "0px".to_owned(),
            thresholds: vec![0.0],
            delay: 0.0,
            track_visibility: false,
        }
    }
}

/// Resize Observer's `ResizeObserverCallback`, as `MutationCallback`.
pub struct ResizeObserverCallback;

impl ArgType for ResizeObserverCallback {
    const DESC: TypeDesc = TypeDesc::Function("ResizeObserverCallback");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Callback<T::Callback>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
        front.function_arg(index, "ResizeObserverCallback")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Callback<O::Callback>,
        _: usize,
    ) -> Result<Callback<O::Callback>, ApiError> {
        Ok(*taken)
    }
}

/// What an `IntersectionObserverInitArg` is in the operation: its `root`
/// found.
pub struct IntersectionOptions<'gc> {
    pub root: Option<NodeRef<'gc>>,
    pub init: IntersectionInit,
}

/// `IntersectionObserver`'s options, a dictionary with a node member
/// (`root`) the front end converts before the operation: its handle,
/// made the DOM's in it.
pub struct IntersectionObserverInitArg;

impl ArgType for IntersectionObserverInitArg {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Dictionary("IntersectionObserverInit");
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = (IntersectionInit, Option<T::Node>);
    type Param<'a, 'gc, T: FrontTypes + 'a> = IntersectionOptions<'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>> {
        front.intersection_observer_init(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        (init, root): &'a (IntersectionInit, Option<O::Node>),
        _: usize,
    ) -> Result<IntersectionOptions<'gc>, ApiError> {
        Ok(IntersectionOptions {
            root: root.as_ref().map(|root| op.node(cx, root)),
            init: init.clone(),
        })
    }
}

/// An `IntersectionObserverEntryInit` as the front end read it (but its
/// `target`): the rects `[x, y, width, height]`, doubles as a
/// `DOMRect`'s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntersectionEntryInitValues {
    pub time: f64,
    pub root_bounds: Option<[f64; 4]>,
    pub bounding_client_rect: [f64; 4],
    pub intersection_rect: [f64; 4],
    pub is_intersecting: bool,
    pub is_visible: bool,
    pub intersection_ratio: f64,
}

/// What an `IntersectionObserverEntryInitArg` is in the operation: its
/// `target` found (`None`: the node is gone, which a kept wrapper's isn't).
pub struct IntersectionEntryInit<'gc> {
    pub target: Option<NodeRef<'gc>>,
    pub values: IntersectionEntryInitValues,
}

/// `IntersectionObserverEntry`'s init, a dictionary with a node member
/// (`target`), as `IntersectionObserverInitArg`.
pub struct IntersectionObserverEntryInitArg;

impl ArgType for IntersectionObserverEntryInitArg {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Dictionary("IntersectionObserverEntryInit");
    type Taken<T: FrontTypes> = (IntersectionEntryInitValues, T::Node);
    type Param<'a, 'gc, T: FrontTypes + 'a> = IntersectionEntryInit<'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>> {
        front.intersection_entry_init(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        (values, target): &'a (IntersectionEntryInitValues, O::Node),
        _: usize,
    ) -> Result<IntersectionEntryInit<'gc>, ApiError> {
        Ok(IntersectionEntryInit {
            target: Some(op.node(cx, target)),
            values: *values,
        })
    }
}

/// DOM's `MutationCallback`: a function, kept by the front end
/// (`Front::function_arg`); anything else is a `TypeError`.
pub struct MutationCallback;

impl ArgType for MutationCallback {
    const DESC: TypeDesc = TypeDesc::Function("MutationCallback");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Callback<T::Callback>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
        front.function_arg(index, "MutationCallback")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Callback<O::Callback>,
        _: usize,
    ) -> Result<Callback<O::Callback>, ApiError> {
        Ok(*taken)
    }
}

/// HTML's `FrameRequestCallback` (`requestAnimationFrame`'s): a function,
/// kept by the front end; anything else a `TypeError`.
pub struct FrameRequestCallback;

impl ArgType for FrameRequestCallback {
    const DESC: TypeDesc = TypeDesc::Function("FrameRequestCallback");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Callback<T::Callback>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
        front.function_arg(index, "FrameRequestCallback")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Callback<O::Callback>,
        _: usize,
    ) -> Result<Callback<O::Callback>, ApiError> {
        Ok(*taken)
    }
}

macro_rules! callback_functions {
    ($($(#[$meta:meta])* $name:ident;)*) => {$(
        $(#[$meta])*
        pub struct $name;

        impl ArgType for $name {
            const DESC: TypeDesc = TypeDesc::Function(stringify!($name));
            const PLAIN: bool = true;
            type Taken<T: FrontTypes> = Callback<T::Callback>;
            type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

            fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
                front.function_arg(index, stringify!($name))
            }

            fn param<'a, 'gc, O: OpFront + 'a>(
                _: &mut O,
                _: &ApiCx<'_, 'gc>,
                taken: &'a Callback<O::Callback>,
                _: usize,
            ) -> Result<Callback<O::Callback>, ApiError> {
                Ok(*taken)
            }
        }
    )*};
}

callback_functions! {
    /// `requestIdleCallback`'s: a function, kept by the front end;
    /// anything else a `TypeError`.
    IdleRequestCallback;
    /// Geolocation's success callback: a function, as above.
    PositionCallback;
}

/// Geolocation's `PositionErrorCallback?`, a member's second callback: a
/// function, kept by the front end, or none for null and undefined;
/// anything else a `TypeError`.
pub struct PositionErrorCallback;

impl ArgType for PositionErrorCallback {
    const DESC: TypeDesc = TypeDesc::Function("PositionErrorCallback");
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<Callback<T::Callback>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<Callback<T::Callback>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<Callback<F::Callback>>> {
        front.nullable_function_arg(index, "PositionErrorCallback")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<Callback<O::Callback>>,
        _: usize,
    ) -> Result<Option<Callback<O::Callback>>, ApiError> {
        Ok(*taken)
    }
}

/// An abort reason (`abort(reason)`'s `any`): the value, or a new
/// `AbortError` `DOMException` for undefined (`Front::abort_reason_arg`).
pub struct AbortReason;

impl ArgType for AbortReason {
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = AnyArg<T::Any>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = AnyArg<T::Any>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<AnyArg<F::Any>> {
        front.abort_reason_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a AnyArg<O::Any>,
        _: usize,
    ) -> Result<AnyArg<O::Any>, ApiError> {
        Ok(*taken)
    }
}

/// `sequence<AbortSignal>` (`AbortSignal.any()`'s): event targets' wrappers
/// (`Front::objects_arg`), each an abort signal (else a `TypeError`).
pub struct AbortSignals;

impl ArgType for AbortSignals {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Sequence("AbortSignal");
    type Taken<T: FrontTypes> = Vec<T::Node>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Vec<Gc<'gc, EventTargetImpl>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Vec<F::Node>> {
        front.objects_arg::<EventTargetImpl>(index, "AbortSignal")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Vec<O::Node>,
        index: usize,
    ) -> Result<Vec<Gc<'gc, EventTargetImpl>>, ApiError> {
        taken
            .iter()
            .map(|object| {
                let signal = op.object::<EventTargetImpl>(cx, object);
                if cx.is_abort_signal(signal) {
                    Ok(signal)
                } else {
                    Err(ApiError::type_error(format!(
                        "parameter {}: Failed to convert value to 'AbortSignal'.",
                        index.saturating_add(1)
                    )))
                }
            })
            .collect()
    }
}

/// `sequence<MessagePort>`, optional (`initMessageEvent`'s `ports`): none if
/// missing or undefined; else event targets' wrappers
/// (`Front::objects_arg`), each a message port (else a `TypeError`).
pub struct MessagePorts;

impl ArgType for MessagePorts {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Sequence("MessagePort");
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Vec<T::Node>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Vec<Gc<'gc, EventTargetImpl>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Vec<F::Node>> {
        if front.is_undefined(index) {
            return Some(Vec::new());
        }
        front.objects_arg::<EventTargetImpl>(index, "MessagePort")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Vec<O::Node>,
        index: usize,
    ) -> Result<Vec<Gc<'gc, EventTargetImpl>>, ApiError> {
        taken
            .iter()
            .map(|object| {
                let port = op.object::<EventTargetImpl>(cx, object);
                if cx.is_message_port(port) {
                    Ok(port)
                } else {
                    Err(ApiError::type_error(format!(
                        "parameter {}: Failed to convert value to 'MessagePort'.",
                        index.saturating_add(1)
                    )))
                }
            })
            .collect()
    }
}

/// WebIDL's `[EnforceRange] unsigned long long` (`AbortSignal.timeout()`'s
/// milliseconds): `ToNumber`, its integer part, from 0 to 2^53 - 1; NaN,
/// the infinities and what is outside a `TypeError`. As a double, which
/// holds each.
pub struct EnforcedU64;

impl ArgType for EnforcedU64 {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = f64;
    type Param<'a, 'gc, T: FrontTypes + 'a> = f64;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<f64> {
        const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
        let value = front.f64_arg(index, false)?;
        if !value.is_finite() {
            front.throw(ApiError::type_error(
                "Value is infinite and cannot be converted to an unsigned long long.",
            ));
            return None;
        }
        let value = value.trunc() + 0.0;
        if !(0.0..=MAX_SAFE_INTEGER).contains(&value) {
            front.throw(ApiError::type_error(
                "Value is outside the 'unsigned long long' value range.",
            ));
            return None;
        }
        Some(value)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a f64,
        _: usize,
    ) -> Result<f64, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's `ByteString`: a string each of whose characters is a byte
/// (U+0000 to U+00FF); one past it a `TypeError`.
pub struct ByteString;

impl ArgType for ByteString {
    const DESC: TypeDesc = TypeDesc::String;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = T::Str;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a str;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<F::Str> {
        front.string_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a O::Str,
        index: usize,
    ) -> Result<&'a str, ApiError> {
        let string = op.str_param(taken);
        if string.chars().any(|c| u32::from(c) > 0xFF) {
            return Err(ApiError::type_error(format!(
                "parameter {}: Cannot convert to a ByteString because it contains a character outside the range 0-255.",
                index.saturating_add(1)
            )));
        }
        Ok(string)
    }
}

/// `HeadersInit` (`(sequence<sequence<ByteString>> or record<ByteString,
/// ByteString>)`), optional (`new Headers()`'s): none if missing or
/// undefined; else as `Front::byte_string_pairs_arg` takes it.
pub struct HeaderPairs;

impl ArgType for HeaderPairs {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::StringPairs;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<StringPairsArg>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = StringPairsInit<'a>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<StringPairsArg>> {
        if front.is_undefined(index) {
            return Some(None);
        }
        front.byte_string_pairs_arg(index).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<StringPairsArg>,
        _: usize,
    ) -> Result<StringPairsInit<'a>, ApiError> {
        Ok(match taken {
            None => StringPairsInit::Sequences(&[]),
            Some(StringPairsArg::Sequences(items)) => StringPairsInit::Sequences(items),
            Some(StringPairsArg::Record(entries)) => StringPairsInit::Record(entries),
            Some(StringPairsArg::String(string)) => StringPairsInit::String(string),
        })
    }
}

/// `(Request or USVString)` (`RequestInfo`, `fetch()`'s and `new
/// Request()`'s input): a `Request`'s wrapper, or anything else's string.
pub struct RequestInfo;

/// What `RequestInfo` took.
pub enum RequestInfoArg<N, S> {
    Request(N),
    Url(S),
}

/// `RequestInfo` in the operation.
pub enum RequestInput<'a, 'gc> {
    Request(Gc<'gc, dom::Request>),
    Url(&'a str),
}

impl ArgType for RequestInfo {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::StringOr(&["Request"]);
    type Taken<T: FrontTypes> = RequestInfoArg<T::Node, T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = RequestInput<'a, 'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<RequestInfoArg<F::Node, F::Str>> {
        match front.object_if::<dom::Request>(index) {
            Some(request) => Some(RequestInfoArg::Request(request)),
            None => front.string_arg(index).map(RequestInfoArg::Url),
        }
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a RequestInfoArg<O::Node, O::Str>,
        _: usize,
    ) -> Result<RequestInput<'a, 'gc>, ApiError> {
        Ok(match taken {
            RequestInfoArg::Request(request) => {
                RequestInput::Request(op.object::<dom::Request>(cx, request))
            }
            RequestInfoArg::Url(url) => RequestInput::Url(op.str_param(url)),
        })
    }
}

/// `BodyInit?` whose default is null (`new Response(body)`'s): none for
/// null or undefined (`Front::body_arg`).
pub struct NullableBody;

impl ArgType for NullableBody {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<BodyArg<T::Node>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<&'a BodyArg<T::Node>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<BodyArg<F::Node>>> {
        front.body_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<BodyArg<O::Node>>,
        _: usize,
    ) -> Result<Option<&'a BodyArg<O::Node>>, ApiError> {
        Ok(taken.as_ref())
    }
}

/// `Response.json()`'s data: its JSON (`Front::json_arg`).
pub struct JsonData;

impl ArgType for JsonData {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = String;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a str;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<String> {
        front.json_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a String,
        _: usize,
    ) -> Result<&'a str, ApiError> {
        Ok(taken)
    }
}

/// WebIDL's `Promise<any>` of the body mixin's `json()`: a new promise,
/// resolved with the text parsed as JSON, rejected with what parsing threw
/// (`OpFront::ret_resolved_json`); a failure rejects it.
pub struct PromiseOfJson;

impl RetType for PromiseOfJson {
    const DESC: TypeDesc = TypeDesc::Promise("any");
    finished_after!(DomString, finish_resolved_json);

    fn fail<F: Front>(front: &mut F, error: ApiError) {
        front.reject(error);
    }
}

impl<F: Front> DictHandle<F> for Option<BodyMember<F::Node>> {
    fn read(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<Self>> {
        Some(front.dict_body(dict, member)?.map(Some))
    }
}

impl DictMember for Option<HeadersMember> {
    const PLAIN: bool = true;
    fn read<F: Front>(
        front: &mut F,
        dict: &F::Dict,
        member: MemberName,
    ) -> Option<Option<Option<HeadersMember>>> {
        Some(
            front
                .dict_headers(dict, member)?
                .map(|pairs| Some(HeadersMember(pairs))),
        )
    }
}

impl<F: Front> DictHandle<F> for Option<SignalMember<F::Node>> {
    fn read(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<Self>> {
        Some(front.dict_signal(dict, member)?.map(Some))
    }
}

impl DictMember for Option<IsNull> {
    const PLAIN: bool = true;
    fn read<F: Front>(
        front: &mut F,
        dict: &F::Dict,
        member: MemberName,
    ) -> Option<Option<Option<IsNull>>> {
        Some(
            front
                .dict_is_null(dict, member)?
                .map(|null| Some(IsNull(null))),
        )
    }
}

/// An `AbortSignal` member of a dictionary (`AddEventListenerOptions`'
/// `signal`): kept by the front end (`Front::dict_object`).
/// A member's call run on a front end that runs script (`Front::whole`):
/// the table's, of a member that leaves script to run.
pub trait WholeCall {
    type Out;

    fn run<F: Front>(self, front: &mut F) -> Self::Out;
}

/// A dictionary's members' reads (the table's: `Dictionary::read` makes
/// one), of a dictionary a front end `T` took: run on a front end that
/// holds its arguments as `T` does (`Front::reading`).
pub trait ReadDict<T: FrontTypes> {
    type Out;

    fn read<F>(self, front: &mut F) -> Self::Out
    where
        F: Front<Str = T::Str, Node = T::Node, Dict = T::Dict, Any = T::Any>;
}

/// `dict`'s member `member` read into `out`, if it has one
/// (`Dictionary::read`'s, of each member): `None` if reading it threw.
#[inline(always)]
pub fn read_member<F: Front, M: DictMember>(
    front: &mut F,
    dict: &F::Dict,
    member: MemberName,
    out: &mut M,
) -> Option<()> {
    if let Some(value) = M::read(front, dict, member)? {
        *out = value;
    }
    Some(())
}

/// As `read_member`, of a member that is the front end's handle.
#[inline(always)]
pub fn read_handle<F: Front, H: DictHandle<F>>(
    front: &mut F,
    dict: &F::Dict,
    member: MemberName,
    out: &mut H,
) -> Option<()> {
    if let Some(value) = H::read(front, dict, member)? {
        *out = value;
    }
    Some(())
}

/// A dictionary member's type that is a front end's handle (an object's
/// wrapper, a value of any type): read from the dictionary before the
/// operation as the front end `F` holds it, which the dictionary is then
/// of (`Dictionary::Of`). The member's function hands it back to the
/// front end in the operation.
pub trait DictHandle<F: Front>: Sized {
    /// As `DictMember::read`.
    fn read(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<Self>>;
}

impl<F: Front> DictHandle<F> for Option<SignalObject<F::Node>> {
    fn read(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<Self>> {
        Some(
            front
                .dict_object::<EventTargetImpl>(dict, member, "AbortSignal")?
                .map(|object| Some(SignalObject(object))),
        )
    }
}

/// An `any` member: none if it is undefined (missing).
impl<F: Front> DictHandle<F> for Option<AnyArg<F::Any>> {
    fn read(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<Self>> {
        Some(front.dict_any(dict, member)?.map(Some))
    }
}

/// WebIDL's variadic `any... arguments`, the last argument: the arguments
/// from its index on, as one array, none if there are none (the table's
/// `name?: Rest`, as it may be missing).
pub struct Rest;

impl ArgType for Rest {
    const DESC: TypeDesc = TypeDesc::Rest;
    const UNDEFINED_IS_DEFAULT: bool = true;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Option<T::Rest>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<T::Rest>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<F::Rest>> {
        Some(front.rest_arg(index))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<O::Rest>,
        _: usize,
    ) -> Result<Option<O::Rest>, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's variadic `DOMString...`, the last argument: each from its
/// index on converted (`ToString`, in order, which may run script), none
/// if there are none (the table's `name?: Strings`).
pub struct Strings;

impl ArgType for Strings {
    const DESC: TypeDesc = TypeDesc::Strings;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Vec<T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Vec<&'a str>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Vec<F::Str>> {
        (index..front.arg_count())
            .map(|index| front.string_arg(index))
            .collect()
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Vec<O::Str>,
        _: usize,
    ) -> Result<Vec<&'a str>, ApiError> {
        Ok(taken.iter().map(|taken| op.str_param(taken)).collect())
    }
}

/// A `(Node or DOMString)` argument as the front end took it.
pub enum NodeOrStr<N, S> {
    Node(N),
    Str(S),
}

/// A `(Node or DOMString)` argument in the operation.
#[derive(Clone, Copy)]
pub enum NodeOrString<'a, 'gc> {
    Node(NodeRef<'gc>),
    String(&'a str),
}

/// WebIDL's variadic `(Node or DOMString)...`, the last argument: each from
/// its index on, in order (the strings converted as they come), none if
/// there are none (the table's `name?: NodesOrStrings`).
pub struct NodesOrStrings;

impl ArgType for NodesOrStrings {
    const DESC: TypeDesc = TypeDesc::NodesOrStrings;
    const UNDEFINED_IS_DEFAULT: bool = true;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Vec<NodeOrStr<T::Node, T::Str>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Vec<NodeOrString<'a, 'gc>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>> {
        (index..front.arg_count())
            .map(|index| front.node_or_string_arg(index))
            .collect()
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Self::Taken<O>,
        _: usize,
    ) -> Result<Vec<NodeOrString<'a, 'gc>>, ApiError> {
        Ok(taken
            .iter()
            .map(|taken| match taken {
                NodeOrStr::Node(node) => NodeOrString::Node(op.node(cx, node)),
                NodeOrStr::Str(string) => NodeOrString::String(op.str_param(string)),
            })
            .collect())
    }
}

/// A `(HTMLElement or long)?` argument as the front end took it.
pub enum NodeOrLong<N> {
    Node(N),
    Long(i32),
}

/// A `(HTMLElement or long)?` argument in the operation (`add`'s
/// `before`).
#[derive(Clone, Copy)]
pub enum ElementOrLong<'gc> {
    Element(NodeRef<'gc>),
    Long(i32),
}

/// WebIDL's `(HTMLElement or long)?`: null or undefined is none; a node
/// that isn't an HTML element is `ToNumber` of an object, `NaN`, so 0.
pub struct NullableElementOrLong;

impl ArgType for NullableElementOrLong {
    const DESC: TypeDesc = TypeDesc::NullableUnion(&["HTMLElement", "long"]);
    const UNDEFINED_IS_DEFAULT: bool = true;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Option<NodeOrLong<T::Node>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<ElementOrLong<'gc>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>> {
        front.node_or_long_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Self::Taken<O>,
        _: usize,
    ) -> Result<Option<ElementOrLong<'gc>>, ApiError> {
        Ok(taken.as_ref().map(|taken| match taken {
            NodeOrLong::Node(node) => {
                let node = op.node(cx, node);
                if cx
                    .qualified_name(node)
                    .is_some_and(|name| name.namespace() == dom::names::HTML_NAMESPACE)
                {
                    ElementOrLong::Element(node)
                } else {
                    ElementOrLong::Long(0)
                }
            }
            NodeOrLong::Long(long) => ElementOrLong::Long(*long),
        }))
    }
}

/// WebIDL's `(HTMLOptionElement or HTMLOptGroupElement)` (`add`'s
/// `element`): an option or an optgroup; anything else a `TypeError`.
pub struct OptionOrOptGroup;

const OPTION_OR_OPTGROUP: &str = "(HTMLOptionElement or HTMLOptGroupElement)";

impl ArgType for OptionOrOptGroup {
    const DESC: TypeDesc = TypeDesc::Union(&["HTMLOptionElement", "HTMLOptGroupElement"]);
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = T::Node;
    type Param<'a, 'gc, T: FrontTypes + 'a> = NodeRef<'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<F::Node> {
        match front.node_arg(index)? {
            Some(node) => Some(node),
            None => {
                front.throw(ApiError::type_error(format!(
                    "parameter {} is not of type '{OPTION_OR_OPTGROUP}'",
                    index.saturating_add(1)
                )));
                None
            }
        }
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a O::Node,
        index: usize,
    ) -> Result<NodeRef<'gc>, ApiError> {
        let node = op.node(cx, taken);
        if cx.is_option(node) || cx.is_optgroup(node) {
            return Ok(node);
        }
        Err(ApiError::type_error(format!(
            "parameter {} is not of type '{OPTION_OR_OPTGROUP}'",
            index.saturating_add(1)
        )))
    }
}

/// WebIDL's variadic `(Element or Text)...`, the last argument: each from
/// its index on a node (null or undefined a `TypeError`, thrown), each an
/// element or a text (else a `TypeError`), none if there are none (the
/// table's `name?: Slottables`).
pub struct Slottables;

impl ArgType for Slottables {
    const DESC: TypeDesc = TypeDesc::Slottables;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Vec<T::Node>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Vec<NodeRef<'gc>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Vec<F::Node>> {
        (index..front.arg_count())
            .map(|index| match front.node_arg(index)? {
                Some(node) => Some(node),
                None => {
                    front.throw(slottable_error(index));
                    None
                }
            })
            .collect()
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Vec<O::Node>,
        index: usize,
    ) -> Result<Vec<NodeRef<'gc>>, ApiError> {
        taken
            .iter()
            .zip(index..)
            .map(|(taken, index)| {
                let node = op.node(cx, taken);
                match cx.kind(node) {
                    NodeKind::Element | NodeKind::Text => Ok(node),
                    _ => Err(slottable_error(index)),
                }
            })
            .collect()
    }
}

fn slottable_error(index: usize) -> ApiError {
    ApiError::type_error(format!(
        "parameter {} is not of type '(Element or Text)'",
        index.saturating_add(1)
    ))
}

/// WebIDL's `sequence<unrestricted double>`: as an argument the numbers
/// (`Front::doubles_arg`), as a result a new array of them.
pub struct Doubles;

impl ArgType for Doubles {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Doubles;
    type Taken<T: FrontTypes> = Vec<f64>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Vec<f64>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Vec<f64>> {
        front.doubles_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Vec<f64>,
        _: usize,
    ) -> Result<Vec<f64>, ApiError> {
        Ok(taken.clone())
    }
}

impl RetType for Doubles {
    const DESC: TypeDesc = TypeDesc::Doubles;
    finished_after!(Vec<f64>, finish_doubles);
}

/// WebIDL's `FrozenArray<double>` as a result: a new array each time.
pub struct Numbers;

impl RetType for Numbers {
    const DESC: TypeDesc = TypeDesc::Numbers;
    finished_after!(Vec<f64>, finish_numbers);
}

/// WebIDL's `FrozenArray<DOMString>` as a result: a frozen array of the
/// strings, the same while they are (`OpFront::ret_frozen_strings`).
pub struct FrozenStrings;

impl RetType for FrozenStrings {
    const DESC: TypeDesc = TypeDesc::FrozenStrings;
    finished_after!(Vec<DomString>, finish_frozen_strings);
}

/// A name as a result's array has it: one of the names table (a local
/// name), by its number, or text made for it (a qualified name with a
/// prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameOrText {
    Name(NameId),
    Text(String),
}

/// `sequence<DOMString>` of names (`getAttributeNames()`'s): a new array
/// each time, a name of the table given by its number
/// (`Front::finish_names`).
pub struct Names;

impl RetType for Names {
    const DESC: TypeDesc = TypeDesc::Strings;
    finished_after!(Vec<NameOrText>, finish_names);
}

/// As a result: a named properties' supported names (`#[named(names)]`).
impl RetType for Strings {
    const DESC: TypeDesc = TypeDesc::Strings;
    finished_after!(Vec<DomString>, finish_strings);
}

/// WebIDL's `any` as a result: a value the engine keeps for the DOM (a JS
/// edge's), which the member's function gives.
pub struct Any;

impl RetType for Any {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = &'gc JsEdge;
    no_pending!();

    fn give<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, edge: &'gc JsEdge) {
        op.ret_any(cx, edge);
    }
}

/// WebIDL's `any` as an argument, its default null (the table's,
/// `initCustomEvent`'s and `initMessageEvent`'s, have that one): kept by
/// the front end (`AnyArg`).
impl ArgType for Any {
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = AnyArg<T::Any>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = AnyArg<T::Any>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<AnyArg<F::Any>> {
        Some(front.any_arg(index))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a AnyArg<O::Any>,
        _: usize,
    ) -> Result<AnyArg<O::Any>, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's `any` whose default is null (`CustomEvent`'s `detail`) as a
/// result: the value of the JS edge the member's function gives, null if
/// it refers to none (the DOM can't read an edge: the engine tells).
pub struct AnyOrNull;

impl RetType for AnyOrNull {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = &'gc JsEdge;
    no_pending!();

    fn give<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, edge: &'gc JsEdge) {
        op.ret_any_or_null(cx, edge);
    }
}

/// WebIDL's `double` (and `DOMHighResTimeStamp`, which is one).
impl RetType for f64 {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    type Ret<'gc> = f64;
    pending!(PendingNumber, finish_number);

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, value: f64) -> Self::Pending<O> {
        op.ret_f64(value)
    }
}

/// WebIDL's `double` as an argument: a finite number.
impl ArgType for f64 {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = f64;
    type Param<'a, 'gc, T: FrontTypes + 'a> = f64;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<f64> {
        front.f64_arg(index, true)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a f64,
        _: usize,
    ) -> Result<f64, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's `double?` as a result: a number, or null.
impl RetType for Option<f64> {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    type Ret<'gc> = Option<f64>;
    pending!(PendingNumber, finish_number);

    fn give<'gc, O: OpFront>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        value: Option<f64>,
    ) -> Self::Pending<O> {
        match value {
            Some(value) => op.ret_f64(value),
            None => {
                op.ret_null();
                None
            }
        }
    }
}

/// WebIDL's `double?` as an argument: null (or undefined) none, else a
/// finite number.
impl ArgType for Option<f64> {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Option<f64>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<f64>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<f64>> {
        if front.is_null_or_undefined(index) {
            return Some(None);
        }
        front.f64_arg(index, true).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<f64>,
        _: usize,
    ) -> Result<Option<f64>, ApiError> {
        Ok(*taken)
    }
}

/// An optional `unrestricted double` with no default (`fillText`'s
/// `maxWidth`): none if missing or undefined, else `ToNumber`.
pub struct OptionalUnrestrictedDouble;

impl ArgType for OptionalUnrestrictedDouble {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<f64>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<f64>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<f64>> {
        if front.is_undefined(index) {
            return Some(None);
        }
        front.f64_arg(index, false).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<f64>,
        _: usize,
    ) -> Result<Option<f64>, ApiError> {
        Ok(*taken)
    }
}

/// WebIDL's `unrestricted double`: any number, NaN and the infinities too
/// (a `DOMRect`'s values).
pub struct UnrestrictedDouble;

impl ArgType for UnrestrictedDouble {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = f64;
    type Param<'a, 'gc, T: FrontTypes + 'a> = f64;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<f64> {
        front.f64_arg(index, false)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a f64,
        _: usize,
    ) -> Result<f64, ApiError> {
        Ok(*taken)
    }
}

impl RetType for UnrestrictedDouble {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    type Ret<'gc> = f64;
    pending!(PendingNumber, finish_number);

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, value: f64) -> Self::Pending<O> {
        op.ret_f64(value)
    }
}

/// A dictionary member's type: read from the dictionary before the
/// operation (WebIDL's conversion of the member's value).
pub trait DictMember: Sized {
    /// Whether the front end reads it as a value (a boolean, a number, a
    /// string, strings), keeping nothing of its engine's for the
    /// operation: a dictionary whose members all are is `PLAIN`.
    const PLAIN: bool = false;

    /// `dict`'s member `member`: `Some(None)` if it is undefined, `None`
    /// if reading it threw.
    fn read<F: Front>(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<Self>>;

    /// The member's value in a dictionary given as a result; none for a
    /// missing one (or one a result doesn't give).
    fn out(&self) -> Option<DictValue> {
        None
    }
}

impl DictMember for bool {
    const PLAIN: bool = true;

    fn read<F: Front>(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<bool>> {
        front.dict_bool(dict, member)
    }

    fn out(&self) -> Option<DictValue> {
        Some(DictValue::Bool(*self))
    }
}

/// A `boolean` member with no default: none if it is missing.
impl DictMember for Option<bool> {
    const PLAIN: bool = true;

    fn read<F: Front>(
        front: &mut F,
        dict: &F::Dict,
        member: MemberName,
    ) -> Option<Option<Option<bool>>> {
        Some(front.dict_bool(dict, member)?.map(Some))
    }
}

/// A `DOMString` member.
impl DictMember for String {
    const PLAIN: bool = true;

    fn read<F: Front>(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<String>> {
        front.dict_string(dict, member)
    }

    fn out(&self) -> Option<DictValue> {
        Some(DictValue::String(self.clone()))
    }
}

/// An `unrestricted double` member with no default: none if it is
/// missing (`DOMMatrix2DInit`'s).
impl DictMember for Option<f64> {
    const PLAIN: bool = true;

    fn read<F: Front>(
        front: &mut F,
        dict: &F::Dict,
        member: MemberName,
    ) -> Option<Option<Option<f64>>> {
        Some(front.dict_number(dict, member)?.map(Some))
    }

    fn out(&self) -> Option<DictValue> {
        self.map(DictValue::Number)
    }
}

/// A `double` member: a finite number.
impl DictMember for f64 {
    const PLAIN: bool = true;

    fn read<F: Front>(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<f64>> {
        let Some(value) = front.dict_number(dict, member)? else {
            return Some(None);
        };
        if !value.is_finite() {
            front.throw(ApiError::type_error(format!(
                "The provided double value for '{member}' is non-finite."
            )));
            return None;
        }
        Some(Some(value))
    }
}

/// WebIDL's conversion of a number to an integer type of `bits` bits
/// (`ToInt32` and its kin): NaN and the infinities 0, else truncated and
/// taken modulo 2^`bits`, `signed` from -2^(`bits` - 1).
fn webidl_integer(value: f64, bits: i32, signed: bool) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let modulus = 2f64.powi(bits);
    let unsigned = value.trunc().rem_euclid(modulus);
    if signed && unsigned >= modulus / 2.0 {
        unsigned - modulus
    } else {
        unsigned
    }
}

macro_rules! integer_members {
    ($($ty:ty: $bits:literal, $signed:literal;)*) => {$(
        /// A WebIDL integer member, converted modulo its size.
        impl DictMember for $ty {
            const PLAIN: bool = true;

            fn read<F: Front>(front: &mut F, dict: &F::Dict, member: MemberName) -> Option<Option<$ty>> {
                Some(
                    front
                        .dict_number(dict, member)?
                        .map(|value| saturated_cast(webidl_integer(value, $bits, $signed))),
                )
            }
        }
    )*};
}

// `long`, `short`, `unsigned long`, `unsigned short`: each value in range
// after the modulo, so the cast is exact.
integer_members! {
    i32: 32, true;
    i16: 16, true;
    u32: 32, false;
    u16: 16, false;
}

/// A `sequence<DOMString>` member with no default: none if it is missing.
/// A `DOMString?` member whose default is null: none if it is undefined
/// (missing) or null.
impl DictMember for Option<String> {
    const PLAIN: bool = true;

    fn read<F: Front>(
        front: &mut F,
        dict: &F::Dict,
        member: MemberName,
    ) -> Option<Option<Option<String>>> {
        Some(front.dict_nullable_string(dict, member)?.map(Some))
    }
}

impl DictMember for Option<Vec<String>> {
    const PLAIN: bool = true;

    fn read<F: Front>(
        front: &mut F,
        dict: &F::Dict,
        member: MemberName,
    ) -> Option<Option<Option<Vec<String>>>> {
        Some(front.dict_strings(dict, member)?.map(Some))
    }
}

/// What an interface's objects are, as its members get `this` (Blink's
/// `WrapperTypeInfo`'s C++ class): nodes (`NodeObject`), events
/// (`dom::Event`), or event targets of any type (`TargetObject`).
pub trait ObjectType {
    type Ref<'gc>: Copy + Root<'gc>;

    /// `this`, in the operation, one of `INTERFACES[interface]`'s objects.
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, interface: usize) -> Self::Ref<'gc>;

    /// One as an object of any type.
    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc>;

    /// Argument `index`, an object of interface `interface` (none for null
    /// or undefined), before the operation; `None` if it threw.
    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        let _ = (front, index, interface);
        unreachable!("no argument is of this interface")
    }

    /// An object argument, in the operation.
    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        let _ = (op, cx, taken);
        unreachable!("no argument is of this interface")
    }
}

/// A node interface's objects.
pub struct NodeObject;

impl ObjectType for NodeObject {
    type Ref<'gc> = NodeRef<'gc>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, interface: usize) -> NodeRef<'gc> {
        let node = op
            .this_as::<Node>(cx)
            .expect("the front end's checks: `this` is one of the interface's");
        debug_assert!(accepts_node(cx, interface, node));
        node
    }

    fn object<'gc>(node: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Node(node)
    }
}

impl ObjectType for dom::Event {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event")
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::Event>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::Event>(cx, taken)
    }
}

/// `EventTarget`'s objects: nodes, and what `new EventTarget()` makes.
pub struct TargetObject;

impl ObjectType for TargetObject {
    type Ref<'gc> = TargetRef<'gc>;

    // In each member's call: the receiver's type is told by three
    // compares, which a call of its own costs more than.
    #[inline(always)]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> TargetRef<'gc> {
        if let Some(node) = op.this_as::<Node>(cx) {
            return TargetRef::Node(node);
        }
        if let Some(window) = op.this_as::<dom::Window>(cx) {
            return TargetRef::Window(window);
        }
        TargetRef::EventTarget(
            op.this_as::<EventTargetImpl>(cx)
                .expect("the front end's checks: `this` is an event target"),
        )
    }

    fn object<'gc>(target: Self::Ref<'gc>) -> ObjectRef<'gc> {
        target.into()
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.target_arg(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.target(cx, taken)
    }
}

impl ObjectType for dom::Window {
    type Ref<'gc> = Gc<'gc, dom::Window>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Window>(cx)
            .expect("the front end's checks: `this` is a window")
    }

    fn object<'gc>(window: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Window(window)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::Window>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::Window>(cx, taken)
    }
}

/// `ErrorEvent`'s objects: events with an error's data (an event's
/// interface is its data's, `interface_of`).
pub struct ErrorEventObject;

impl ObjectType for ErrorEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.error_event_data(event).is_some(),
            "the front end's checks: `this` is an error event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `UIEvent`'s objects: events with a UI event's data (a mouse event's
/// too).
pub struct UiEventObject;

impl ObjectType for UiEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.is_ui_event(event),
            "the front end's checks: `this` is a UI event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `MouseEvent`'s objects: events with a mouse event's data.
pub struct MouseEventObject;

impl ObjectType for MouseEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.mouse_event_data(event).is_some(),
            "the front end's checks: `this` is a mouse event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `WheelEvent`'s objects: events with a wheel event's data.
pub struct WheelEventObject;

impl ObjectType for WheelEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.wheel_event_data(event).is_some(),
            "the front end's checks: `this` is a wheel event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `TextEvent`'s objects: events with a text event's data.
pub struct TextEventObject;

impl ObjectType for TextEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.text_event_data(event).is_some(),
            "the front end's checks: `this` is a text event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `InputEvent`'s objects: events with an input event's data.
pub struct InputEventObject;

impl ObjectType for InputEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.input_event_data(event).is_some(),
            "the front end's checks: `this` is an input event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `CustomEvent`'s objects: events with a custom event's data.
pub struct CustomEventObject;

impl ObjectType for CustomEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.custom_event_data(event).is_some(),
            "the front end's checks: `this` is a custom event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// A namespace's (`#[namespace]`): it has no objects of its own, its
/// members are static (no `this`).
pub struct NamespaceObject;

impl ObjectType for NamespaceObject {
    type Ref<'gc> = Gc<'gc, dom::Window>;

    fn this<'gc, O: OpFront>(_: &mut O, _: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        unreachable!("a namespace's members are static")
    }

    fn object<'gc>(window: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Window(window)
    }
}

/// `MediaQueryList`'s objects: event targets with a list's data.
pub struct MediaQueryListObject;

impl ObjectType for MediaQueryListObject {
    type Ref<'gc> = Gc<'gc, dom::EventTargetImpl>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let list = op
            .this_as::<dom::EventTargetImpl>(cx)
            .expect("the front end's checks: `this` is an event target");
        debug_assert!(
            cx.is_media_query_list(list),
            "the front end's checks: `this` is a MediaQueryList"
        );
        list
    }

    fn object<'gc>(list: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::EventTarget(list)
    }
}

/// `MediaQueryListEvent`'s objects: events with its data.
pub struct MediaQueryListEventObject;

impl ObjectType for MediaQueryListEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.media_query_list_event_data(event).is_some(),
            "the front end's checks: `this` is a MediaQueryListEvent"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `TransitionEvent`'s and `AnimationEvent`'s objects: events with their
/// data (`transition` tells which).
pub struct AnimationEventObjectOf<const TRANSITION: bool>;

pub type TransitionEventObject = AnimationEventObjectOf<true>;
pub type AnimationEventObject = AnimationEventObjectOf<false>;

impl<const TRANSITION: bool> ObjectType for AnimationEventObjectOf<TRANSITION> {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.animation_event_data(event)
                .is_some_and(|data| data.transition == TRANSITION),
            "the front end's checks: `this` is a TransitionEvent or an AnimationEvent"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `AnimationPlaybackEvent`'s objects: events with its data.
pub struct AnimationPlaybackEventObject;

impl ObjectType for AnimationPlaybackEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.animation_playback_event_data(event).is_some(),
            "the front end's checks: `this` is an AnimationPlaybackEvent"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `PopStateEvent`'s objects: events with its data.
pub struct PopStateEventObject;

impl ObjectType for PopStateEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.pop_state_event_data(event).is_some(),
            "the front end's checks: `this` is a PopStateEvent"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `HashChangeEvent`'s objects: events with its data.
pub struct HashChangeEventObject;

impl ObjectType for HashChangeEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.hash_change_event_data(event).is_some(),
            "the front end's checks: `this` is a HashChangeEvent"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `MessageEvent`'s objects: events with a message event's data.
pub struct MessageEventObject;

impl ObjectType for MessageEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.message_event_data(event).is_some(),
            "the front end's checks: `this` is a message event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `MessagePort`'s objects: event targets with a port's data.
pub struct MessagePortObject;

impl ObjectType for MessagePortObject {
    type Ref<'gc> = Gc<'gc, dom::EventTargetImpl>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let port = op
            .this_as::<dom::EventTargetImpl>(cx)
            .expect("the front end's checks: `this` is an event target");
        debug_assert!(
            cx.is_message_port(port),
            "the front end's checks: `this` is a message port"
        );
        port
    }

    fn object<'gc>(port: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::EventTarget(port)
    }
}

impl ObjectType for dom::MessageChannel {
    type Ref<'gc> = Gc<'gc, dom::MessageChannel>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::MessageChannel>(cx)
            .expect("the front end's checks: `this` is a `MessageChannel`")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::MessageChannel(object)
    }
}

/// `FocusEvent`'s objects: events with a focus event's data.
pub struct FocusEventObject;

impl ObjectType for FocusEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.focus_event_data(event).is_some(),
            "the front end's checks: `this` is a focus event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `KeyboardEvent`'s objects: events with a keyboard event's data.
pub struct KeyboardEventObject;

impl ObjectType for KeyboardEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.keyboard_event_data(event).is_some(),
            "the front end's checks: `this` is a keyboard event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// `PromiseRejectionEvent`'s objects: events with a rejection's data.
pub struct PromiseRejectionEventObject;

impl ObjectType for PromiseRejectionEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.promise_rejection_data(event).is_some(),
            "the front end's checks: `this` is a rejection's event"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

impl ObjectType for dom::HtmlCollection {
    type Ref<'gc> = Gc<'gc, dom::HtmlCollection>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::HtmlCollection>(cx)
            .expect("the front end's checks: `this` is a collection")
    }

    fn object<'gc>(collection: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::HtmlCollection(collection)
    }
}

/// `HTMLFormControlsCollection`'s objects: forms' `elements`.
pub struct FormControlsCollectionObject;

impl ObjectType for FormControlsCollectionObject {
    type Ref<'gc> = Gc<'gc, dom::HtmlCollection>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let collection = op
            .this_as::<dom::HtmlCollection>(cx)
            .expect("the front end's checks: `this` is a collection");
        debug_assert!(
            cx.is_form_controls_collection(collection),
            "the front end's checks: `this` is a form's elements"
        );
        collection
    }

    fn object<'gc>(collection: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::HtmlCollection(collection)
    }
}

/// `HTMLAllCollection`'s objects: documents' `all`.
pub struct AllCollectionObject;

impl ObjectType for AllCollectionObject {
    type Ref<'gc> = Gc<'gc, dom::HtmlCollection>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let collection = op
            .this_as::<dom::HtmlCollection>(cx)
            .expect("the front end's checks: `this` is a collection");
        debug_assert!(
            cx.is_all_collection(collection),
            "the front end's checks: `this` is a document's all"
        );
        collection
    }

    fn object<'gc>(collection: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::HtmlCollection(collection)
    }
}

/// `(HTMLCollection or Element)?` (`document.all`'s `namedItem`): an
/// element, or a collection of those a name names when there are several.
pub struct HTMLCollectionOrElement;

/// What a member giving an `HTMLCollectionOrElement` gives.
pub enum CollectionOrElement<'gc> {
    Element(NodeRef<'gc>),
    Collection(Gc<'gc, dom::HtmlCollection>),
}

impl RetType for Option<HTMLCollectionOrElement> {
    const DESC: TypeDesc = TypeDesc::NullableUnion(&["HTMLCollection", "Element"]);
    const PLAIN: bool = true;
    type Ret<'gc> = Option<CollectionOrElement<'gc>>;
    pending!(PendingObject, finish_object);

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Self::Ret<'gc>,
    ) -> Self::Pending<O> {
        match value {
            Some(CollectionOrElement::Element(element)) => {
                op.ret_object(cx, ObjectRef::Node(element))
            }
            Some(CollectionOrElement::Collection(collection)) => {
                op.ret_object(cx, ObjectRef::HtmlCollection(collection))
            }
            None => {
                op.ret_null();
                None
            }
        }
    }
}

/// `HTMLOptionsCollection`'s objects: selects' `options`.
pub struct OptionsCollectionObject;

impl ObjectType for OptionsCollectionObject {
    type Ref<'gc> = Gc<'gc, dom::HtmlCollection>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let collection = op
            .this_as::<dom::HtmlCollection>(cx)
            .expect("the front end's checks: `this` is a collection");
        debug_assert!(
            cx.is_options_collection(collection),
            "the front end's checks: `this` is a select's options"
        );
        collection
    }

    fn object<'gc>(collection: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::HtmlCollection(collection)
    }
}

/// `RadioNodeList`'s objects: a form's elements of a name.
pub struct RadioNodeListObject;

impl ObjectType for RadioNodeListObject {
    type Ref<'gc> = Gc<'gc, dom::NodeList>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let list = op
            .this_as::<dom::NodeList>(cx)
            .expect("the front end's checks: `this` is a list of nodes");
        debug_assert!(
            cx.is_radio_node_list(list),
            "the front end's checks: `this` is a radio node list"
        );
        list
    }

    fn object<'gc>(list: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::NodeList(list)
    }
}

/// WebIDL's `(RadioNodeList or Element)?` as a result (`Option` of it): a
/// form's named getter's, its `elements`' `namedItem`'s.
pub struct RadioNodeListOrElement;

impl RetType for Option<RadioNodeListOrElement> {
    const DESC: TypeDesc = TypeDesc::NullableUnion(&["RadioNodeList", "Element"]);
    const PLAIN: bool = true;
    type Ret<'gc> = Option<dom::FormItem<'gc>>;
    pending!(PendingObject, finish_object);

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Self::Ret<'gc>,
    ) -> Self::Pending<O> {
        match value {
            Some(dom::FormItem::Element(element)) => op.ret_object(cx, ObjectRef::Node(element)),
            Some(dom::FormItem::List(list)) => op.ret_object(cx, ObjectRef::NodeList(list)),
            None => {
                op.ret_null();
                None
            }
        }
    }
}

impl ObjectType for dom::NodeList {
    type Ref<'gc> = Gc<'gc, dom::NodeList>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::NodeList>(cx)
            .expect("the front end's checks: `this` is a list of nodes")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::NodeList(object)
    }
}

impl ObjectType for dom::DomTokenList {
    type Ref<'gc> = Gc<'gc, dom::DomTokenList>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::DomTokenList>(cx)
            .expect("the front end's checks: `this` is a token list")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::DomTokenList(object)
    }
}

impl ObjectType for dom::NamedNodeMap {
    type Ref<'gc> = Gc<'gc, dom::NamedNodeMap>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::NamedNodeMap>(cx)
            .expect("the front end's checks: `this` is an attributes map")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::NamedNodeMap(object)
    }
}

impl ObjectType for dom::Range {
    type Ref<'gc> = Gc<'gc, dom::Range>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Range>(cx)
            .expect("the front end's checks: `this` is a range")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Range(object)
    }

    /// A range's wrapper: a `StaticRange`'s too, which a member taking a
    /// `Range` turns away (`compareBoundaryPoints`).
    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::Range>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::Range>(cx, taken)
    }
}

impl ObjectType for dom::Selection {
    type Ref<'gc> = Gc<'gc, dom::Selection>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Selection>(cx)
            .expect("the front end's checks: `this` is a selection")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Selection(object)
    }
}

impl ObjectType for dom::NodeIterator {
    type Ref<'gc> = Gc<'gc, dom::NodeIterator>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::NodeIterator>(cx)
            .expect("the front end's checks: `this` is a node iterator")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::NodeIterator(object)
    }
}

impl ObjectType for dom::TreeWalker {
    type Ref<'gc> = Gc<'gc, dom::TreeWalker>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::TreeWalker>(cx)
            .expect("the front end's checks: `this` is a tree walker")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::TreeWalker(object)
    }
}

impl ObjectType for dom::DomParser {
    type Ref<'gc> = Gc<'gc, dom::DomParser>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::DomParser>(cx)
            .expect("the front end's checks: `this` is a DOMParser")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::DomParser(object)
    }
}

impl ObjectType for dom::DomStringMap {
    type Ref<'gc> = Gc<'gc, dom::DomStringMap>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::DomStringMap>(cx)
            .expect("the front end's checks: `this` is a dataset")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::DomStringMap(object)
    }
}

impl ObjectType for dom::CssStyleDeclaration {
    type Ref<'gc> = Gc<'gc, dom::CssStyleDeclaration>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::CssStyleDeclaration>(cx)
            .expect("the front end's checks: `this` is a style declaration")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::CssStyleDeclaration(object)
    }
}

impl ObjectType for dom::MutationObserver {
    type Ref<'gc> = Gc<'gc, dom::MutationObserver>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::MutationObserver>(cx)
            .expect("the front end's checks: `this` is a mutation observer")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::MutationObserver(object)
    }
}

impl ObjectType for dom::IntersectionObserver {
    type Ref<'gc> = Gc<'gc, dom::IntersectionObserver>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::IntersectionObserver>(cx)
            .expect("the front end's checks: `this` is an intersection observer")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::IntersectionObserver(object)
    }
}

impl ObjectType for dom::IntersectionObserverEntry {
    type Ref<'gc> = Gc<'gc, dom::IntersectionObserverEntry>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::IntersectionObserverEntry>(cx)
            .expect("the front end's checks: `this` is an intersection observer entry")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::IntersectionObserverEntry(object)
    }
}

impl ObjectType for dom::ResizeObserver {
    type Ref<'gc> = Gc<'gc, dom::ResizeObserver>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::ResizeObserver>(cx)
            .expect("the front end's checks: `this` is a resize observer")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::ResizeObserver(object)
    }
}

impl ObjectType for dom::ResizeObserverEntry {
    type Ref<'gc> = Gc<'gc, dom::ResizeObserverEntry>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::ResizeObserverEntry>(cx)
            .expect("the front end's checks: `this` is a resize observer entry")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::ResizeObserverEntry(object)
    }
}

impl ObjectType for dom::ResizeObserverSize {
    type Ref<'gc> = Gc<'gc, dom::ResizeObserverSize>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::ResizeObserverSize>(cx)
            .expect("the front end's checks: `this` is a resize observer size")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::ResizeObserverSize(object)
    }
}

impl ObjectType for dom::CanvasRenderingContext2d {
    type Ref<'gc> = Gc<'gc, dom::CanvasRenderingContext2d>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::CanvasRenderingContext2d>(cx)
            .expect("the front end's checks: `this` is a 2D context")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::CanvasRenderingContext2d(object)
    }
}

impl ObjectType for dom::CanvasGradient {
    type Ref<'gc> = Gc<'gc, dom::CanvasGradient>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::CanvasGradient>(cx)
            .expect("the front end's checks: `this` is a gradient")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::CanvasGradient(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::CanvasGradient>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::CanvasGradient>(cx, taken)
    }
}

impl ObjectType for dom::CanvasPattern {
    type Ref<'gc> = Gc<'gc, dom::CanvasPattern>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::CanvasPattern>(cx)
            .expect("the front end's checks: `this` is a pattern")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::CanvasPattern(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::CanvasPattern>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::CanvasPattern>(cx, taken)
    }
}

impl ObjectType for dom::ImageData {
    type Ref<'gc> = Gc<'gc, dom::ImageData>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::ImageData>(cx)
            .expect("the front end's checks: `this` is an `ImageData`")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::ImageData(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::ImageData>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::ImageData>(cx, taken)
    }
}

impl ObjectType for dom::SvgAnimatedString {
    type Ref<'gc> = Gc<'gc, dom::SvgAnimatedString>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::SvgAnimatedString>(cx)
            .expect("the front end's checks: `this` is an `SVGAnimatedString`")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::SvgAnimatedString(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::SvgAnimatedString>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::SvgAnimatedString>(cx, taken)
    }
}

impl ObjectType for dom::SvgMatrix {
    type Ref<'gc> = Gc<'gc, dom::SvgMatrix>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::SvgMatrix>(cx)
            .expect("the front end's checks: `this` is an `SVGMatrix`")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::SvgMatrix(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::SvgMatrix>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::SvgMatrix>(cx, taken)
    }
}

impl ObjectType for dom::SvgPoint {
    type Ref<'gc> = Gc<'gc, dom::SvgPoint>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::SvgPoint>(cx)
            .expect("the front end's checks: `this` is an `SVGPoint`")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::SvgPoint(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::SvgPoint>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::SvgPoint>(cx, taken)
    }
}

impl ObjectType for dom::SvgRect {
    type Ref<'gc> = Gc<'gc, dom::SvgRect>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::SvgRect>(cx)
            .expect("the front end's checks: `this` is an `SVGRect`")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::SvgRect(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::SvgRect>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::SvgRect>(cx, taken)
    }
}

impl ObjectType for dom::TextMetrics {
    type Ref<'gc> = Gc<'gc, dom::TextMetrics>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::TextMetrics>(cx)
            .expect("the front end's checks: `this` is a `TextMetrics`")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::TextMetrics(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::TextMetrics>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::TextMetrics>(cx, taken)
    }
}

impl ObjectType for dom::Path2d {
    type Ref<'gc> = Gc<'gc, dom::Path2d>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Path2d>(cx)
            .expect("the front end's checks: `this` is a `Path2D`")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Path2d(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::Path2d>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::Path2d>(cx, taken)
    }
}

impl ObjectType for dom::Blob {
    type Ref<'gc> = Gc<'gc, dom::Blob>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Blob>(cx)
            .expect("the front end's checks: `this` is a blob")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Blob(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::Blob>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::Blob>(cx, taken)
    }
}

impl ObjectType for dom::MutationRecord {
    type Ref<'gc> = Gc<'gc, dom::MutationRecord>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::MutationRecord>(cx)
            .expect("the front end's checks: `this` is a mutation record")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::MutationRecord(object)
    }
}

impl ObjectType for dom::DomRect {
    type Ref<'gc> = Gc<'gc, dom::DomRect>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::DomRect>(cx)
            .expect("the front end's checks: `this` is a rect")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::DomRect(object)
    }
}

impl ObjectType for dom::CustomElementRegistry {
    type Ref<'gc> = Gc<'gc, dom::CustomElementRegistry>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::CustomElementRegistry>(cx)
            .expect("the front end's checks: `this` is a registry")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::CustomElementRegistry(object)
    }
}

impl ObjectType for dom::CssStyleSheet {
    type Ref<'gc> = Gc<'gc, dom::CssStyleSheet>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::CssStyleSheet>(cx)
            .expect("the front end's checks: `this` is a sheet")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::CssStyleSheet(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::CssStyleSheet>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::CssStyleSheet>(cx, taken)
    }
}

impl ObjectType for dom::CssRule {
    type Ref<'gc> = Gc<'gc, dom::CssRule>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::CssRule>(cx)
            .expect("the front end's checks: `this` is a rule")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::CssRule(object)
    }
}

impl ObjectType for dom::CssRuleList {
    type Ref<'gc> = Gc<'gc, dom::CssRuleList>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::CssRuleList>(cx)
            .expect("the front end's checks: `this` is a rule list")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::CssRuleList(object)
    }
}

impl ObjectType for dom::MediaList {
    type Ref<'gc> = Gc<'gc, dom::MediaList>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::MediaList>(cx)
            .expect("the front end's checks: `this` is a media list")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::MediaList(object)
    }
}

impl ObjectType for dom::StyleSheetList {
    type Ref<'gc> = Gc<'gc, dom::StyleSheetList>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::StyleSheetList>(cx)
            .expect("the front end's checks: `this` is a sheet list")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::StyleSheetList(object)
    }
}

impl ObjectType for dom::DomImplementation {
    type Ref<'gc> = Gc<'gc, dom::DomImplementation>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::DomImplementation>(cx)
            .expect("the front end's checks: `this` is an implementation")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::DomImplementation(object)
    }
}

impl ObjectType for dom::DomRectList {
    type Ref<'gc> = Gc<'gc, dom::DomRectList>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::DomRectList>(cx)
            .expect("the front end's checks: `this` is a list of rects")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::DomRectList(object)
    }
}

impl ObjectType for dom::History {
    type Ref<'gc> = Gc<'gc, dom::History>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::History>(cx)
            .expect("the front end's checks: `this` is a history")
    }

    fn object<'gc>(history: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::History(history)
    }
}

impl ObjectType for dom::Url {
    type Ref<'gc> = Gc<'gc, dom::Url>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Url>(cx)
            .expect("the front end's checks: `this` is a URL")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Url(object)
    }
}

impl ObjectType for dom::UrlSearchParams {
    type Ref<'gc> = Gc<'gc, dom::UrlSearchParams>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::UrlSearchParams>(cx)
            .expect("the front end's checks: `this` is a URLSearchParams")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::UrlSearchParams(object)
    }
}

impl ObjectType for dom::Crypto {
    type Ref<'gc> = Gc<'gc, dom::Crypto>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Crypto>(cx)
            .expect("the front end's checks: `this` is a crypto")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Crypto(object)
    }
}

impl ObjectType for dom::Navigator {
    type Ref<'gc> = Gc<'gc, dom::Navigator>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Navigator>(cx)
            .expect("the front end's checks: `this` is a navigator")
    }

    fn object<'gc>(navigator: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Navigator(navigator)
    }
}

impl ObjectType for dom::UserActivation {
    type Ref<'gc> = Gc<'gc, dom::UserActivation>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::UserActivation>(cx)
            .expect("the front end's checks: `this` is a user activation")
    }

    fn object<'gc>(activation: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::UserActivation(activation)
    }
}

impl ObjectType for dom::Location {
    type Ref<'gc> = Gc<'gc, dom::Location>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Location>(cx)
            .expect("the front end's checks: `this` is a location")
    }

    fn object<'gc>(location: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Location(location)
    }
}

/// An object interface `I`'s object as a result (`I` or `Option<I>` in the
/// table, `declare!` implements them): given as an object of any type.
pub fn give_object<'gc, I: Interface, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    object: <I::Object as ObjectType>::Ref<'gc>,
) -> Option<O::PendingObject> {
    op.ret_object(cx, <I::Object as ObjectType>::object(object))
}

/// A non-nullable object interface's argument taken: null or undefined is a
/// `TypeError`.
pub fn take_object<I: Interface, F: Front>(front: &mut F, index: usize) -> Option<F::Node> {
    match <I::Object as ObjectType>::take_arg(front, index, I::DESC.name)? {
        Some(object) => Some(object),
        None => {
            front.throw(ApiError::type_error(format!(
                "parameter {} is not of type '{}'",
                index.saturating_add(1),
                I::DESC.name
            )));
            None
        }
    }
}

/// `createElement`'s local name: a string, and the wrapper of the element's
/// interface its name has made ahead (`Front::prepare_new_element`; with
/// `NewHtmlElement` as the result).
pub struct ElementLocalName;

impl ArgType for ElementLocalName {
    const DESC: TypeDesc = TypeDesc::String;
    const PLAIN: bool = true;
    /// The name, its number if it is one already, and the interface its
    /// element will be of in an HTML document (`Front::foresee_element`).
    type Taken<T: FrontTypes> = (T::Str, Option<dom::NameId>, usize);
    type Param<'a, 'gc, T: FrontTypes + 'a> = (&'a str, Option<dom::NameId>);

    fn take<F: Front>(front: &mut F, index: usize) -> Option<(F::Str, Option<dom::NameId>, usize)> {
        let name = front.string_arg(index)?;
        let (id, interface) = front.foresee_element(&name);
        Some((name, id, interface))
    }

    #[inline(always)]
    fn foreseen_interface<F: FrontTypes>(
        taken: &(F::Str, Option<dom::NameId>, usize),
    ) -> Option<usize> {
        Some(taken.2)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a (O::Str, Option<dom::NameId>, usize),
        _: usize,
    ) -> Result<(&'a str, Option<dom::NameId>), ApiError> {
        Ok((op.str_param(&taken.0), taken.1))
    }
}

/// `createElement`'s element, its wrapper made ahead by its local name's
/// argument (`ElementLocalName`), or the custom element to construct.
pub struct NewHtmlElement;

impl RetType for NewHtmlElement {
    const DESC: TypeDesc = TypeDesc::Node("Element");
    const PLAIN: bool = true;
    type Ret<'gc> = Created<'gc>;
    type Prepared<F: FrontTypes> = F::PreparedNode;
    type Pending<F: FrontTypes> = Option<F::PendingNode>;

    #[inline(always)]
    fn prepare<F: Front>(front: &mut F, foreseen: Option<usize>) -> F::PreparedNode {
        NewElement::<crate::interfaces::HTMLElement>::prepare(front, foreseen)
    }

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Created<'gc>,
    ) -> Option<O::PendingNode> {
        NewElement::<crate::interfaces::HTMLElement>::give(op, cx, value)
    }

    #[inline(always)]
    fn give_prepared<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Created<'gc>,
        prepared: O::PreparedNode,
    ) -> Option<O::PendingNode> {
        NewElement::<crate::interfaces::HTMLElement>::give_prepared(op, cx, value, prepared)
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, pending: Option<F::PendingNode>) {
        if let Some(pending) = pending {
            front.finish_node(pending);
        }
    }
}

/// A new node of the interface `this` is of (`cloneNode`'s copy): its
/// wrapper made ahead of the operation, as `NewObject`'s, of the
/// interface the front end finds `this` to be of
/// (`Front::prepare_node_like_this`); one of another interface after all is
/// given as any node.
pub struct NewNodeLikeThis;

impl RetType for NewNodeLikeThis {
    const DESC: TypeDesc = TypeDesc::Node("Node");
    const PLAIN: bool = true;
    type Ret<'gc> = NodeRef<'gc>;
    type Prepared<F: FrontTypes> = F::PreparedNode;
    type Pending<F: FrontTypes> = Option<F::PendingNode>;

    #[inline(always)]
    fn prepare<F: Front>(front: &mut F, _: Option<usize>) -> F::PreparedNode {
        front.prepare_node_like_this()
    }

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: NodeRef<'gc>,
    ) -> Option<O::PendingNode> {
        op.ret_node(cx, value)
    }

    #[inline(always)]
    fn give_prepared<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: NodeRef<'gc>,
        prepared: O::PreparedNode,
    ) -> Option<O::PendingNode> {
        op.ret_new_node(cx, value, prepared)
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, pending: Option<F::PendingNode>) {
        if let Some(pending) = pending {
            front.finish_node(pending);
        }
    }
}

/// `createElement`'s and `createElementNS`'s element (`NewObject<Element,
/// L>`'s: its wrapper of interface `L` made ahead), or the custom element
/// to construct after the operation (HTML's "create an element" with a
/// definition and the synchronous custom elements flag).
pub struct NewElement<L>(PhantomData<L>);

/// What a member giving a `NewElement` gives.
pub enum Created<'gc> {
    Element(NodeRef<'gc>),
    Construct {
        definition: Gc<'gc, dom::CustomElementDefinition>,
        document: NodeRef<'gc>,
    },
}

impl<L: Interface> RetType for NewElement<L> {
    const DESC: TypeDesc = TypeDesc::Node("Element");
    const PLAIN: bool = true;
    type Ret<'gc> = Created<'gc>;
    type Prepared<F: FrontTypes> = F::PreparedNode;
    type Pending<F: FrontTypes> = Option<F::PendingNode>;

    /// A wrapper of the interface an argument foresaw (`createElement`'s
    /// local name), else `L`'s.
    #[inline(always)]
    fn prepare<F: Front>(front: &mut F, foreseen: Option<usize>) -> F::PreparedNode {
        front.prepare_new_node(foreseen.unwrap_or(L::INDEX))
    }

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Created<'gc>,
    ) -> Option<O::PendingNode> {
        Self::give_prepared(op, cx, value, O::PreparedNode::default())
    }

    #[inline(always)]
    fn give_prepared<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Created<'gc>,
        prepared: O::PreparedNode,
    ) -> Option<O::PendingNode> {
        match value {
            Created::Element(element) => op.ret_new_node(cx, element, prepared),
            Created::Construct {
                definition,
                document,
            } => op.ret_construct(cx, definition, document),
        }
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, pending: Option<F::PendingNode>) {
        if let Some(pending) = pending {
            front.finish_node(pending);
        }
    }
}

impl<'gc> From<NodeRef<'gc>> for Created<'gc> {
    fn from(element: NodeRef<'gc>) -> Self {
        Self::Element(element)
    }
}

/// What `HTMLElement`'s constructor makes (`OpFront::construct_html`).
pub enum HtmlConstructed<'gc> {
    New(NodeRef<'gc>),
    Upgrading(NodeRef<'gc>),
}

/// A constructor's function's result, given to the call: an object, the
/// call's new wrapper made its; `HTMLElement`'s element; an `ImageData`
/// with its pixels. What it leaves for after the operation it gives back.
pub trait Constructed<'gc> {
    fn construct<O: OpFront>(self, op: &mut O, cx: &ApiCx<'_, 'gc>) -> ConstructPending;
}

/// What a constructed object leaves for after the operation
/// (`finish_constructed`): for the engine to allocate, or to set.
pub enum ConstructPending {
    None,
    /// A new `ImageData`'s pixels, its array to make of them.
    Pixels(Persistent<dom::ImageData>, Vec<u8>),
    /// An element being upgraded, whose wrapper is the result.
    Upgrading(NodeRoot),
}

/// What a constructor's call does after its operation with what its
/// object left.
#[inline(always)]
pub fn finish_constructed<F: Front>(front: &mut F, pending: ConstructPending) {
    match pending {
        ConstructPending::None => {}
        ConstructPending::Pixels(image_data, bytes) => {
            front.finish_image_data_pixels(image_data, bytes);
        }
        ConstructPending::Upgrading(element) => front.finish_upgrading(element),
    }
}

impl<'gc, T: elise::HeapType> Constructed<'gc> for Gc<'gc, T>
where
    Gc<'gc, T>: Into<ObjectRef<'gc>>,
{
    #[inline(always)]
    fn construct<O: OpFront>(self, op: &mut O, cx: &ApiCx<'_, 'gc>) -> ConstructPending {
        op.construct_object(cx, self.into());
        ConstructPending::None
    }
}

impl<'gc> Constructed<'gc> for HtmlConstructed<'gc> {
    fn construct<O: OpFront>(self, op: &mut O, cx: &ApiCx<'_, 'gc>) -> ConstructPending {
        match op.construct_html(cx, self) {
            Some(element) => ConstructPending::Upgrading(element),
            None => ConstructPending::None,
        }
    }
}

/// A new `ImageData` and the pixels its array is made of after the
/// operation (the engine allocates it): what a member making one gives.
pub struct ImageDataWithPixels<'gc> {
    pub image_data: Gc<'gc, dom::ImageData>,
    pub pixels: Vec<u8>,
}

impl<'gc> Constructed<'gc> for ImageDataWithPixels<'gc> {
    fn construct<O: OpFront>(self, op: &mut O, cx: &ApiCx<'_, 'gc>) -> ConstructPending {
        op.construct_object(cx, ObjectRef::ImageData(self.image_data));
        ConstructPending::Pixels(Persistent::new(cx.gc, self.image_data), self.pixels)
    }
}

/// HTML's `GetHTMLOptions` (`getHTML`'s): `serializableShadowRoots`, and
/// `shadowRoots`, a sequence of `ShadowRoot`s the front end converts and
/// keeps before the operation (a dictionary of the table holds no
/// objects), its nodes found in it (`OpFront::listed_shadow_roots`).
pub struct GetHtmlOptionsArg;

/// What a `GetHtmlOptionsArg` is in the operation.
pub struct GetHtmlOptions<'gc> {
    pub serializable_shadow_roots: bool,
    pub shadow_roots: Vec<NodeRef<'gc>>,
}

impl ArgType for GetHtmlOptionsArg {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Dictionary("GetHTMLOptions");
    const UNDEFINED_IS_DEFAULT: bool = true;
    /// `serializableShadowRoots`, and `shadowRoots`' nodes.
    type Taken<T: FrontTypes> = (bool, Vec<T::Node>);
    type Param<'a, 'gc, T: FrontTypes + 'a> = GetHtmlOptions<'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<(bool, Vec<F::Node>)> {
        front.get_html_options(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        (serializable_shadow_roots, listed): &'a (bool, Vec<O::Node>),
        _: usize,
    ) -> Result<GetHtmlOptions<'gc>, ApiError> {
        // Each a shadow root (WebIDL's conversion of the sequence's
        // items, told in the operation).
        let shadow_roots = listed
            .iter()
            .map(|node| {
                let node = op.node(cx, node);
                if cx.is_shadow_root(node) {
                    Ok(node)
                } else {
                    Err(ApiError::type_error(
                        "Failed to execute 'getHTML': Failed to read the 'shadowRoots' property from 'GetHTMLOptions': Failed to convert value to 'ShadowRoot'.",
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(GetHtmlOptions {
            serializable_shadow_roots: *serializable_shadow_roots,
            shadow_roots,
        })
    }
}

/// HTML's `CustomElementConstructor` (`define`'s and `getName`'s): a
/// function (callable), kept by the front end; anything else a
/// `TypeError`. Whether it is a constructor `define` asks of the host.
pub struct CustomElementConstructor;

impl ArgType for CustomElementConstructor {
    const DESC: TypeDesc = TypeDesc::Function("CustomElementConstructor");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Callback<T::Callback>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
        front.function_arg(index, "CustomElementConstructor")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Callback<O::Callback>,
        _: usize,
    ) -> Result<Callback<O::Callback>, ApiError> {
        Ok(*taken)
    }
}

/// A value the DOM holds for the engine (a JS edge's), or `undefined` if
/// none (`customElements.get`'s `(CustomElementConstructor or undefined)`).
pub struct AnyOrUndefined;

impl RetType for AnyOrUndefined {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = Option<&'gc JsEdge>;
    no_pending!();

    fn give<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, edge: Option<&'gc JsEdge>) {
        op.ret_any_or_undefined(cx, edge);
    }
}

/// An event, or `undefined` if none (`window.event`'s `(Event or
/// undefined)`).
pub struct EventOrUndefined;

impl RetType for EventOrUndefined {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = Option<Gc<'gc, dom::Event>>;
    pending!(PendingObject, finish_object);

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        event: Option<Gc<'gc, dom::Event>>,
    ) -> Self::Pending<O> {
        match event {
            Some(event) => op.ret_object(cx, ObjectRef::Event(event)),
            None => {
                op.ret_undefined();
                None
            }
        }
    }
}

/// A promise of a value the DOM holds for the engine (`whenDefined`'s
/// `Promise<CustomElementConstructor>`): resolved after the operation with
/// what an edge holds, or pending, its resolver kept in an edge; a failure
/// rejects it (WebIDL's for an operation returning a promise).
pub struct PromiseOfAny;

/// What a member giving a `PromiseOfAny` gives.
pub enum PromiseOf<'gc> {
    /// Resolved with the definition's constructor.
    Defined(Gc<'gc, dom::CustomElementDefinition>),
    /// A new one, its resolver to keep by the number.
    Pending(u64),
    /// The one whose resolver is kept by the number.
    Existing(u64),
}

/// What a `PromiseOfAny` leaves for after the operation: the definition
/// whose constructor a new promise is resolved with, or the number a
/// pending one's resolver is kept by (and whether one is already).
pub enum PromisePending {
    Defined(Persistent<dom::CustomElementDefinition>),
    Pending(u64, bool),
}

impl RetType for PromiseOfAny {
    const DESC: TypeDesc = TypeDesc::Promise("any");
    const PLAIN: bool = true;
    type Ret<'gc> = PromiseOf<'gc>;
    no_prepared!();
    type Pending<F: FrontTypes> = PromisePending;

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: PromiseOf<'gc>,
    ) -> PromisePending {
        match value {
            PromiseOf::Defined(definition) => {
                PromisePending::Defined(Persistent::new(cx.gc, definition))
            }
            PromiseOf::Pending(id) => PromisePending::Pending(id, false),
            PromiseOf::Existing(id) => PromisePending::Pending(id, true),
        }
    }

    fn finish<F: Front>(front: &mut F, pending: PromisePending) {
        match pending {
            PromisePending::Defined(definition) => front.finish_resolved_constructor(definition),
            PromisePending::Pending(id, existing) => front.finish_pending_promise(id, existing),
        }
    }

    fn fail<F: Front>(front: &mut F, error: ApiError) {
        front.reject(error);
    }
}

/// WebIDL's `ObservableArray<I>`, an attribute's type: the getter gives the
/// backing list, which the front end's array reads (and changes through
/// the attribute's `#[observable(..)]` operations).
pub struct ObservableArray<I>(PhantomData<I>);

impl<I: Interface> RetType for ObservableArray<I> {
    const DESC: TypeDesc = TypeDesc::ObservableArray(I::NAME);
    const PLAIN: bool = true;
    type Ret<'gc> = Vec<<I::Object as ObjectType>::Ref<'gc>>;
    no_prepared!();
    type Pending<F: FrontTypes> = Vec<(ObjectRoot, usize)>;

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Self::Ret<'gc>,
    ) -> Vec<(ObjectRoot, usize)> {
        rooted_objects::<I>(cx, value)
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, objects: Vec<(ObjectRoot, usize)>) {
        front.finish_objects(objects);
    }
}

/// WebIDL's `Promise<I>` as a result: a new promise, resolved with the
/// member's object, or (its function failed) rejected with the error
/// (WebIDL's for an operation returning a promise).
pub struct Promise<I>(PhantomData<I>);

impl<I: Interface> RetType for Promise<I> {
    const DESC: TypeDesc = TypeDesc::Promise(I::NAME);
    const PLAIN: bool = true;
    type Ret<'gc> = <I::Object as ObjectType>::Ref<'gc>;
    no_prepared!();
    type Pending<F: FrontTypes> = (ObjectRoot, usize);

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Self::Ret<'gc>,
    ) -> (ObjectRoot, usize) {
        let object = <I::Object as ObjectType>::object(value);
        (cx.root_object(object), interface_of(cx, object))
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, (object, interface): (ObjectRoot, usize)) {
        front.finish_resolved(object, interface);
    }

    fn fail<F: Front>(front: &mut F, error: ApiError) {
        front.reject(error);
    }
}

/// WebIDL's `Promise<I>` settled later (`wakeLock.request()`'s): a new
/// promise, pending, its resolver kept by the engine by the number the
/// member gives (`ScriptHost::resolve_promise_with`, `settle_promises`); a
/// failure rejects it.
pub struct PendingPromise<I>(PhantomData<I>);

impl<I: Interface> RetType for PendingPromise<I> {
    const DESC: TypeDesc = TypeDesc::Promise(I::NAME);
    const PLAIN: bool = true;
    type Ret<'gc> = u64;
    no_prepared!();
    type Pending<F: FrontTypes> = u64;

    #[inline(always)]
    fn give<'gc, O: OpFront>(_: &mut O, _: &ApiCx<'_, 'gc>, promise: u64) -> u64 {
        promise
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, promise: u64) {
        front.finish_pending_promise(promise, false);
    }

    fn fail<F: Front>(front: &mut F, error: ApiError) {
        front.reject(error);
    }
}

/// WebIDL's `sequence<I>` as a result: a new array of `I`'s objects.
pub struct Sequence<I>(PhantomData<I>);

impl<I: Interface> RetType for Sequence<I> {
    const DESC: TypeDesc = TypeDesc::Sequence(I::DESC.name);
    const PLAIN: bool = true;
    type Ret<'gc> = Vec<<I::Object as ObjectType>::Ref<'gc>>;
    no_prepared!();
    type Pending<F: FrontTypes> = Vec<(ObjectRoot, usize)>;

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Self::Ret<'gc>,
    ) -> Vec<(ObjectRoot, usize)> {
        rooted_objects::<I>(cx, value)
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, objects: Vec<(ObjectRoot, usize)>) {
        front.finish_objects(objects);
    }
}

/// WebIDL's `FrozenArray<I>` as a result: a frozen array of `I`'s
/// objects, the same while they are (`OpFront::ret_frozen_objects`).
pub struct FrozenArray<I>(PhantomData<I>);

impl<I: Interface> RetType for FrozenArray<I> {
    const DESC: TypeDesc = TypeDesc::FrozenArray(I::DESC.name);
    const PLAIN: bool = true;
    type Ret<'gc> = Vec<<I::Object as ObjectType>::Ref<'gc>>;
    no_prepared!();
    type Pending<F: FrontTypes> = Vec<(ObjectRoot, usize)>;

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: Self::Ret<'gc>,
    ) -> Vec<(ObjectRoot, usize)> {
        rooted_objects::<I>(cx, value)
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, objects: Vec<(ObjectRoot, usize)>) {
        front.finish_frozen_objects(objects);
    }
}

/// The document whose realm a new wrapper of `object` is made in (HTML's
/// relevant realm; Blink makes a wrapper in its holder's creation
/// context): a node's, and that of the node a collection, a node's
/// attributes, its class list, its dataset or an element's style (its own
/// or computed) belongs to; a window's own objects' (its `location`,
/// `history`, `navigator` and theirs, `performance` and its entries, an
/// event target of a window's: a port, a reader, a signal), its document;
/// a node of a document with no window, its relevant global's document;
/// a style sheet's, its document's, and its rules' and their lists'.
/// None for the others: made in the current realm (an `IdleDeadline`, in
/// the callback's, which its caller enters).
pub fn realm_document<'gc>(cx: &dom::Cx<'_, 'gc>, object: ObjectRef<'gc>) -> Option<NodeRef<'gc>> {
    let window = match object {
        ObjectRef::Location(location) => Some(cx.location_window(location)),
        ObjectRef::History(history) => Some(cx.history_window(history)),
        ObjectRef::Navigator(navigator) => Some(cx.navigator_window(navigator)),
        ObjectRef::UserActivation(activation) => Some(cx.user_activation_window(activation)),
        ObjectRef::Scheduling(scheduling) => Some(cx.scheduling_window(scheduling)),
        ObjectRef::Geolocation(geolocation) => Some(cx.geolocation_window(geolocation)),
        ObjectRef::WakeLock(wake_lock) => Some(cx.wake_lock_window(wake_lock)),
        ObjectRef::PerformanceTiming(timing) => Some(cx.performance_timing_window(timing)),
        ObjectRef::PerformanceEntry(entry) => Some(cx.performance_entry_window(entry)),
        ObjectRef::EventTarget(target) if cx.is_performance(target) => {
            Some(cx.performance_window(target))
        }
        ObjectRef::EventTarget(target) => cx.target_window(dom::TargetRef::EventTarget(target)),
        _ => None,
    };
    if let Some(window) = window {
        return Some(cx.window_document(window));
    }
    // A rule's sheet's (none out of one).
    let rule_document = |rule| {
        cx.css_rule_sheet(rule)
            .map(|sheet| cx.style_sheet_document(sheet))
    };
    let node = match object {
        ObjectRef::Node(node) => node,
        ObjectRef::HtmlCollection(collection) => cx.collection_root(collection),
        ObjectRef::NamedNodeMap(map) => cx.attribute_map_element(map),
        ObjectRef::DomTokenList(list) => cx.token_list_element(list),
        ObjectRef::DomStringMap(map) => cx.dataset_element(map),
        ObjectRef::CssStyleDeclaration(declaration) => {
            match cx.style_declaration_owner(declaration) {
                dom::DeclarationOwner::Element(element)
                | dom::DeclarationOwner::Computed(element, _) => element,
                dom::DeclarationOwner::Rule(rule) => rule_document(rule)?,
            }
        }
        // CSSOM's: a sheet's document (a constructed one's constructor
        // document), its rules' and their lists'.
        ObjectRef::CssStyleSheet(sheet) => cx.style_sheet_document(sheet),
        ObjectRef::CssRule(rule) => rule_document(rule)?,
        ObjectRef::CssRuleList(list) => match cx.rule_list_parent(list) {
            dom::RuleParent::Sheet(sheet) => cx.style_sheet_document(sheet),
            dom::RuleParent::Rule(rule) => rule_document(rule)?,
        },
        // Web Animations': their document's (an `animate()`'s: its
        // target's).
        ObjectRef::EventTarget(target) => cx.animation_of_object(target)?.2,
        ObjectRef::KeyframeEffect(effect) => cx.effect_of_object(effect).1,
        ObjectRef::DocumentTimeline(timeline) => cx.timeline_document(timeline),
        _ => return None,
    };
    // A document of no window: its relevant global's (the window whose
    // script made it).
    let document = cx.document_of(node);
    if cx.window_of(document).is_none()
        && let Some(window) = cx.document_relevant_window(document)
    {
        return Some(cx.window_document(window));
    }
    Some(document)
}

/// The interface an object's wrapper is of: a node's kind's most derived
/// one, an event's `Event`, a plain target's `EventTarget`.
pub fn interface_of<'gc>(cx: &ApiCx<'_, 'gc>, object: ObjectRef<'gc>) -> usize {
    match object {
        ObjectRef::Node(node) => node_interface_of(cx, node),
        ObjectRef::Event(event) => {
            if cx.error_event_data(event).is_some() {
                const { crate::declare::interface_index("ErrorEvent") }
            } else if cx.promise_rejection_data(event).is_some() {
                const { crate::declare::interface_index("PromiseRejectionEvent") }
            } else if cx.message_event_data(event).is_some() {
                const { crate::declare::interface_index("MessageEvent") }
            } else if cx.custom_event_data(event).is_some() {
                const { crate::declare::interface_index("CustomEvent") }
            } else if cx.wheel_event_data(event).is_some() {
                // Before `MouseEvent`: a wheel event's mouse data is one's.
                const { crate::declare::interface_index("WheelEvent") }
            } else if cx.mouse_event_data(event).is_some() {
                const { crate::declare::interface_index("MouseEvent") }
            } else if cx.keyboard_event_data(event).is_some() {
                const { crate::declare::interface_index("KeyboardEvent") }
            } else if cx.focus_event_data(event).is_some() {
                const { crate::declare::interface_index("FocusEvent") }
            } else if cx.input_event_data(event).is_some() {
                const { crate::declare::interface_index("InputEvent") }
            } else if cx.media_query_list_event_data(event).is_some() {
                const { crate::declare::interface_index("MediaQueryListEvent") }
            } else if cx.animation_playback_event_data(event).is_some() {
                const { crate::declare::interface_index("AnimationPlaybackEvent") }
            } else if cx.pop_state_event_data(event).is_some() {
                const { crate::declare::interface_index("PopStateEvent") }
            } else if cx.progress_event_data(event).is_some() {
                const { crate::declare::interface_index("ProgressEvent") }
            } else if cx.hash_change_event_data(event).is_some() {
                const { crate::declare::interface_index("HashChangeEvent") }
            } else if let Some(data) = cx.animation_event_data(event) {
                if data.transition {
                    const { crate::declare::interface_index("TransitionEvent") }
                } else {
                    const { crate::declare::interface_index("AnimationEvent") }
                }
            } else if cx.text_event_data(event).is_some() {
                const { crate::declare::interface_index("TextEvent") }
            } else if cx.is_ui_event(event) {
                const { crate::declare::interface_index("UIEvent") }
            } else {
                const { crate::declare::interface_index("Event") }
            }
        }
        ObjectRef::EventTarget(target) if cx.is_performance(target) => {
            const { crate::declare::interface_index("Performance") }
        }
        ObjectRef::EventTarget(target) if cx.is_file_reader(target) => {
            const { crate::declare::interface_index("FileReader") }
        }
        ObjectRef::EventTarget(target) if cx.is_abort_signal(target) => {
            const { crate::declare::interface_index("AbortSignal") }
        }
        ObjectRef::EventTarget(target) if cx.is_wake_lock_sentinel(target) => {
            const { crate::declare::interface_index("WakeLockSentinel") }
        }
        ObjectRef::EventTarget(target) if cx.is_network_information(target) => {
            const { crate::declare::interface_index("NetworkInformation") }
        }
        ObjectRef::EventTarget(target) if cx.is_message_port(target) => {
            const { crate::declare::interface_index("MessagePort") }
        }
        ObjectRef::EventTarget(target) if cx.is_media_query_list(target) => {
            const { crate::declare::interface_index("MediaQueryList") }
        }
        ObjectRef::EventTarget(target) if cx.is_animation_object(target) => {
            match cx.animation_of_object(target).map(|(_, class, _)| class) {
                Some(dom::AnimationClass::CssTransition) => {
                    const { crate::declare::interface_index("CSSTransition") }
                }
                Some(dom::AnimationClass::CssAnimation) => {
                    const { crate::declare::interface_index("CSSAnimation") }
                }
                _ => const { crate::declare::interface_index("Animation") },
            }
        }
        ObjectRef::EventTarget(_) => const { crate::declare::interface_index("EventTarget") },
        ObjectRef::MessageChannel(_) => const { crate::declare::interface_index("MessageChannel") },
        ObjectRef::Window(_) => const { crate::declare::interface_index("Window") },
        ObjectRef::Location(_) => const { crate::declare::interface_index("Location") },
        ObjectRef::History(_) => const { crate::declare::interface_index("History") },
        ObjectRef::Navigator(_) => const { crate::declare::interface_index("Navigator") },
        ObjectRef::Url(_) => const { crate::declare::interface_index("URL") },
        ObjectRef::UrlSearchParams(_) => {
            const { crate::declare::interface_index("URLSearchParams") }
        }
        ObjectRef::Crypto(_) => const { crate::declare::interface_index("Crypto") },
        ObjectRef::IdleDeadline(_) => const { crate::declare::interface_index("IdleDeadline") },
        ObjectRef::AbortController(_) => {
            const { crate::declare::interface_index("AbortController") }
        }
        ObjectRef::Headers(_) => const { crate::declare::interface_index("Headers") },
        ObjectRef::Request(_) => const { crate::declare::interface_index("Request") },
        ObjectRef::Response(_) => const { crate::declare::interface_index("Response") },
        ObjectRef::Geolocation(_) => const { crate::declare::interface_index("Geolocation") },
        ObjectRef::GeolocationPositionError(_) => {
            const { crate::declare::interface_index("GeolocationPositionError") }
        }
        ObjectRef::WakeLock(_) => const { crate::declare::interface_index("WakeLock") },
        ObjectRef::Scheduling(_) => const { crate::declare::interface_index("Scheduling") },
        ObjectRef::PerformanceEntry(entry) => match cx.performance_entry_type(entry) {
            dom::PerformanceEntryType::Mark => {
                const { crate::declare::interface_index("PerformanceMark") }
            }
            dom::PerformanceEntryType::Measure => {
                const { crate::declare::interface_index("PerformanceMeasure") }
            }
            dom::PerformanceEntryType::Navigation => {
                const { crate::declare::interface_index("PerformanceNavigationTiming") }
            }
        },
        ObjectRef::PerformanceTiming(timing) => match cx.performance_timing_kind(timing) {
            dom::PerformanceTimingKind::Timing => {
                const { crate::declare::interface_index("PerformanceTiming") }
            }
            dom::PerformanceTimingKind::Navigation => {
                const { crate::declare::interface_index("PerformanceNavigation") }
            }
            dom::PerformanceTimingKind::Confidence(_) => {
                const { crate::declare::interface_index("PerformanceTimingConfidence") }
            }
        },
        ObjectRef::PerformanceObserver(_) => {
            const { crate::declare::interface_index("PerformanceObserver") }
        }
        ObjectRef::PerformanceObserverEntryList(_) => {
            const { crate::declare::interface_index("PerformanceObserverEntryList") }
        }
        ObjectRef::UserActivation(_) => const { crate::declare::interface_index("UserActivation") },
        ObjectRef::HtmlCollection(collection) => {
            if cx.is_all_collection(collection) {
                const { crate::declare::interface_index("HTMLAllCollection") }
            } else if cx.is_form_controls_collection(collection) {
                const { crate::declare::interface_index("HTMLFormControlsCollection") }
            } else if cx.is_options_collection(collection) {
                const { crate::declare::interface_index("HTMLOptionsCollection") }
            } else {
                const { crate::declare::interface_index("HTMLCollection") }
            }
        }
        ObjectRef::NodeList(list) => {
            if cx.is_radio_node_list(list) {
                const { crate::declare::interface_index("RadioNodeList") }
            } else {
                const { crate::declare::interface_index("NodeList") }
            }
        }
        ObjectRef::DomTokenList(_) => const { crate::declare::interface_index("DOMTokenList") },
        ObjectRef::DomStringMap(_) => const { crate::declare::interface_index("DOMStringMap") },
        ObjectRef::NamedNodeMap(_) => const { crate::declare::interface_index("NamedNodeMap") },
        ObjectRef::DomParser(_) => const { crate::declare::interface_index("DOMParser") },
        ObjectRef::Range(range) => {
            if cx.is_static_range(range) {
                const { crate::declare::interface_index("StaticRange") }
            } else {
                const { crate::declare::interface_index("Range") }
            }
        }
        ObjectRef::NodeIterator(_) => const { crate::declare::interface_index("NodeIterator") },
        ObjectRef::TreeWalker(_) => const { crate::declare::interface_index("TreeWalker") },
        ObjectRef::Selection(_) => const { crate::declare::interface_index("Selection") },
        ObjectRef::CssStyleDeclaration(_) => {
            const { crate::declare::interface_index("CSSStyleProperties") }
        }
        ObjectRef::MutationObserver(_) => {
            const { crate::declare::interface_index("MutationObserver") }
        }
        ObjectRef::MutationRecord(_) => const { crate::declare::interface_index("MutationRecord") },
        ObjectRef::IntersectionObserver(_) => {
            const { crate::declare::interface_index("IntersectionObserver") }
        }
        ObjectRef::IntersectionObserverEntry(_) => {
            const { crate::declare::interface_index("IntersectionObserverEntry") }
        }
        ObjectRef::ResizeObserver(_) => const { crate::declare::interface_index("ResizeObserver") },
        ObjectRef::ResizeObserverEntry(_) => {
            const { crate::declare::interface_index("ResizeObserverEntry") }
        }
        ObjectRef::ResizeObserverSize(_) => {
            const { crate::declare::interface_index("ResizeObserverSize") }
        }
        ObjectRef::CanvasRenderingContext2d(_) => {
            const { crate::declare::interface_index("CanvasRenderingContext2D") }
        }
        ObjectRef::CanvasGradient(_) => const { crate::declare::interface_index("CanvasGradient") },
        ObjectRef::CanvasPattern(_) => const { crate::declare::interface_index("CanvasPattern") },
        ObjectRef::ImageData(_) => const { crate::declare::interface_index("ImageData") },
        ObjectRef::Blob(blob) if cx.blob_data(blob).file().is_some() => {
            const { crate::declare::interface_index("File") }
        }
        ObjectRef::Blob(_) => const { crate::declare::interface_index("Blob") },
        ObjectRef::Path2d(_) => const { crate::declare::interface_index("Path2D") },
        ObjectRef::TextMetrics(_) => const { crate::declare::interface_index("TextMetrics") },
        ObjectRef::SvgAnimatedString(_) => {
            const { crate::declare::interface_index("SVGAnimatedString") }
        }
        ObjectRef::SvgMatrix(_) => const { crate::declare::interface_index("SVGMatrix") },
        ObjectRef::SvgPoint(_) => const { crate::declare::interface_index("SVGPoint") },
        ObjectRef::SvgRect(_) => const { crate::declare::interface_index("SVGRect") },
        ObjectRef::DomRect(rect) if cx.is_read_only_rect(rect) => {
            const { crate::declare::interface_index("DOMRectReadOnly") }
        }
        ObjectRef::DomRect(_) => const { crate::declare::interface_index("DOMRect") },
        ObjectRef::DomRectList(_) => const { crate::declare::interface_index("DOMRectList") },
        ObjectRef::DomImplementation(_) => {
            const { crate::declare::interface_index("DOMImplementation") }
        }
        ObjectRef::CssStyleSheet(_) => const { crate::declare::interface_index("CSSStyleSheet") },
        ObjectRef::CssRule(rule) => match cx.css_rule_kind(rule) {
            dom::RuleKind::Style => const { crate::declare::interface_index("CSSStyleRule") },
            dom::RuleKind::Media => const { crate::declare::interface_index("CSSMediaRule") },
            dom::RuleKind::Supports => const { crate::declare::interface_index("CSSSupportsRule") },
            dom::RuleKind::Keyframes => {
                const { crate::declare::interface_index("CSSKeyframesRule") }
            }
            dom::RuleKind::Keyframe => const { crate::declare::interface_index("CSSKeyframeRule") },
        },
        ObjectRef::CssRuleList(_) => const { crate::declare::interface_index("CSSRuleList") },
        ObjectRef::MediaList(_) => const { crate::declare::interface_index("MediaList") },
        ObjectRef::KeyframeEffect(_) => const { crate::declare::interface_index("KeyframeEffect") },
        ObjectRef::DocumentTimeline(_) => {
            const { crate::declare::interface_index("DocumentTimeline") }
        }
        ObjectRef::StyleSheetList(_) => const { crate::declare::interface_index("StyleSheetList") },
        ObjectRef::CustomElementRegistry(_) => {
            const { crate::declare::interface_index("CustomElementRegistry") }
        }
    }
}

/// A string result: one the function made, or one of the DOM's where it
/// is (`StrRef`: an attribute's value, a text's data; `Name`: one of its
/// names), which the front end borrows as it is given: no clone, so no
/// reference counted (an atomic add and subtract) when the front end has
/// its own string cached, nor the string's three words moved about.
pub enum DomStr<'gc> {
    Owned(DomString),
    InPlace(StrRef<'gc>),
    Name(NameId),
    /// Text the function computed for this call (a serialization), or a
    /// constant of the program's (a keyword): no string of the DOM's is
    /// made of it, the front end takes the text (`DomStr::fresh`).
    Fresh(std::borrow::Cow<'static, str>),
}

impl From<DomString> for DomStr<'_> {
    fn from(string: DomString) -> Self {
        Self::Owned(string)
    }
}

impl<'gc> From<StrRef<'gc>> for DomStr<'gc> {
    fn from(string: StrRef<'gc>) -> Self {
        Self::InPlace(string)
    }
}

impl DomStr<'_> {
    /// Text that is no string of the DOM's: a `String` computed for the
    /// call (moved, not copied), or a constant (`&'static str`: nothing
    /// allocated).
    pub fn fresh(text: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        Self::Fresh(text.into())
    }

    /// Always inlined: one call a string getter, into the front end's own
    /// inlined returns (left out of line, its call and the string moved
    /// to it cost a `getAttribute` some 40 instructions).
    #[inline(always)]
    fn give<O: OpFront>(self, op: &mut O) -> Option<O::PendingString> {
        match self {
            Self::Owned(string) => op.ret_string(&string),
            Self::InPlace(string) => op.ret_string(&string.borrow()),
            Self::Name(id) => op.ret_name(id),
            Self::Fresh(text) => op.ret_fresh_string(text),
        }
    }
}

/// A `DOMString?` result: none, or a string as `DomStr` has it.
pub struct NullableStr<'gc>(pub Option<DomStr<'gc>>);

impl From<Option<DomString>> for NullableStr<'_> {
    fn from(string: Option<DomString>) -> Self {
        Self(string.map(DomStr::Owned))
    }
}

impl<'gc> From<Option<StrRef<'gc>>> for NullableStr<'gc> {
    fn from(string: Option<StrRef<'gc>>) -> Self {
        Self(string.map(DomStr::InPlace))
    }
}

impl<'gc> From<Option<DomStr<'gc>>> for NullableStr<'gc> {
    fn from(string: Option<DomStr<'gc>>) -> Self {
        Self(string)
    }
}

/// `DOMString`: the function gets the argument's text, and returns a
/// string of the DOM (`DomStr`: kept, or where it is).
impl ArgType for DomString {
    const DESC: TypeDesc = TypeDesc::String;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = T::Str;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a str;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<F::Str> {
        front.string_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a O::Str,
        _: usize,
    ) -> Result<&'a str, ApiError> {
        Ok(op.str_param(taken))
    }
}

impl RetType for DomString {
    const DESC: TypeDesc = TypeDesc::String;
    const PLAIN: bool = true;
    type Ret<'gc> = DomStr<'gc>;
    pending!(PendingString, finish_string);

    // As `DomStr::give`: inlined into the member's call.
    #[inline(always)]
    fn give<'gc, O: OpFront>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        value: DomStr<'gc>,
    ) -> Self::Pending<O> {
        value.give(op)
    }
}

/// WebIDL's `[EnforceRange] long` (`getImageData`'s, `putImageData`'s,
/// `createImageData`'s): `ToNumber`, its integer part, in `long`'s range;
/// `NaN`, the infinities and what is outside a `TypeError`.
pub struct EnforcedI32;

impl ArgType for EnforcedI32 {
    const DESC: TypeDesc = TypeDesc::I32;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = i32;
    type Param<'a, 'gc, T: FrontTypes + 'a> = i32;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<i32> {
        let value = front.f64_arg(index, false)?;
        if !value.is_finite() {
            front.throw(ApiError::type_error(
                "Value is infinite and cannot be converted to a long.",
            ));
            return None;
        }
        let value = value.trunc() + 0.0;
        if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value) {
            front.throw(ApiError::type_error(
                "Value is outside the 'long' value range.",
            ));
            return None;
        }
        Some(saturated_cast(value))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a i32,
        _: usize,
    ) -> Result<i32, ApiError> {
        Ok(*taken)
    }
}

/// A `Uint8ClampedArray`: as an argument, the script's own, as the front
/// end holds it (`Uint8ClampedArg`); as a result, the one an `ImageData`
/// holds (its `JsEdge`), the same each time.
pub struct Uint8ClampedArray;

impl ArgType for Uint8ClampedArray {
    const DESC: TypeDesc = TypeDesc::Uint8ClampedArray;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Uint8ClampedArg<T::Any>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Uint8ClampedArg<T::Any>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Uint8ClampedArg<F::Any>> {
        front.uint8_clamped_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Uint8ClampedArg<O::Any>,
        _: usize,
    ) -> Result<Uint8ClampedArg<O::Any>, ApiError> {
        Ok(*taken)
    }
}

impl RetType for Uint8ClampedArray {
    const DESC: TypeDesc = TypeDesc::Uint8ClampedArray;
    const PLAIN: bool = true;
    type Ret<'gc> = &'gc JsEdge;
    no_pending!();

    fn give<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, value: &'gc JsEdge) {
        op.ret_any(cx, value);
    }
}

/// A 2D context's `fillStyle` and `strokeStyle` (WebIDL's `(DOMString or
/// CanvasGradient or CanvasPattern)`): as an argument, a gradient's or a
/// pattern's wrapper is the object, anything else `ToString`ed; as a
/// result, the color serialized or the object.
pub struct CanvasStyleValue;

const CANVAS_STYLE_OBJECTS: &[&str] = &["CanvasGradient", "CanvasPattern"];

/// What a `CanvasStyleValue` argument is as the front end took it.
pub enum CanvasStyleTaken<N, S> {
    Gradient(N),
    Pattern(N),
    String(S),
}

/// What a `CanvasStyleValue` argument is in the operation.
pub enum CanvasStyleParam<'a, 'gc> {
    String(&'a str),
    Object(dom::CanvasStyleObject<'gc>),
}

/// What a `CanvasStyleValue` result is.
pub enum CanvasStyleRet<'gc> {
    String(DomStr<'gc>),
    Object(dom::CanvasStyleObject<'gc>),
}

impl ArgType for CanvasStyleValue {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::StringOr(CANVAS_STYLE_OBJECTS);
    type Taken<T: FrontTypes> = CanvasStyleTaken<T::Node, T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = CanvasStyleParam<'a, 'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<CanvasStyleTaken<F::Node, F::Str>> {
        if let Some(object) = front.object_if::<dom::CanvasGradient>(index) {
            return Some(CanvasStyleTaken::Gradient(object));
        }
        if let Some(object) = front.object_if::<dom::CanvasPattern>(index) {
            return Some(CanvasStyleTaken::Pattern(object));
        }
        front.string_arg(index).map(CanvasStyleTaken::String)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a CanvasStyleTaken<O::Node, O::Str>,
        _: usize,
    ) -> Result<CanvasStyleParam<'a, 'gc>, ApiError> {
        use dom::CanvasStyleObject;
        Ok(match taken {
            CanvasStyleTaken::Gradient(object) => {
                CanvasStyleParam::Object(CanvasStyleObject::Gradient(op.object(cx, object)))
            }
            CanvasStyleTaken::Pattern(object) => {
                CanvasStyleParam::Object(CanvasStyleObject::Pattern(op.object(cx, object)))
            }
            CanvasStyleTaken::String(string) => CanvasStyleParam::String(op.str_param(string)),
        })
    }
}

impl RetType for CanvasStyleValue {
    const DESC: TypeDesc = TypeDesc::StringOr(CANVAS_STYLE_OBJECTS);
    const PLAIN: bool = true;
    type Ret<'gc> = CanvasStyleRet<'gc>;
    no_prepared!();
    type Pending<F: FrontTypes> = (Option<F::PendingString>, Option<F::PendingObject>);

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, (string, object): Self::Pending<F>) {
        if let Some(string) = string {
            front.finish_string(string);
        }
        if let Some(object) = object {
            front.finish_object(object);
        }
    }

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        value: CanvasStyleRet<'gc>,
    ) -> Self::Pending<O> {
        use dom::CanvasStyleObject;
        match value {
            CanvasStyleRet::String(string) => (string.give(op), None),
            CanvasStyleRet::Object(CanvasStyleObject::Gradient(gradient)) => {
                (None, op.ret_object(cx, ObjectRef::CanvasGradient(gradient)))
            }
            CanvasStyleRet::Object(CanvasStyleObject::Pattern(pattern)) => {
                (None, op.ret_object(cx, ObjectRef::CanvasPattern(pattern)))
            }
        }
    }
}

/// WebIDL's `CanvasImageSource` (`drawImage`'s, `createPattern`'s
/// `image`), of the sources machopo has: an `<img>` or a `<canvas>`; anything
/// else a `TypeError`.
pub struct CanvasImageSource;

const CANVAS_IMAGE_SOURCE: &str = "(HTMLImageElement or HTMLCanvasElement)";

fn canvas_image_source_error(index: usize) -> ApiError {
    ApiError::type_error(format!(
        "parameter {} is not of type '{CANVAS_IMAGE_SOURCE}'.",
        index.saturating_add(1)
    ))
}

impl ArgType for CanvasImageSource {
    const DESC: TypeDesc = TypeDesc::Union(&["HTMLImageElement", "HTMLCanvasElement"]);
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = T::Node;
    type Param<'a, 'gc, T: FrontTypes + 'a> = NodeRef<'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<F::Node> {
        match front.node_arg(index)? {
            Some(node) => Some(node),
            None => {
                front.throw(canvas_image_source_error(index));
                None
            }
        }
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a O::Node,
        index: usize,
    ) -> Result<NodeRef<'gc>, ApiError> {
        let node = op.node(cx, taken);
        if cx.is_img(node) || cx.is_canvas(node) {
            return Ok(node);
        }
        Err(canvas_image_source_error(index))
    }
}

/// `new Path2D()`'s optional `(Path2D or DOMString)`: none if missing or
/// undefined; a `Path2D`'s wrapper is the path, anything else `ToString`.
pub struct OptionalPath2dOrString;

/// What an `OptionalPath2dOrString` is as the front end took it.
pub enum Path2dOrStringTaken<N, S> {
    Path(N),
    String(S),
}

/// What an `OptionalPath2dOrString` is in the operation.
pub enum Path2dOrString<'a, 'gc> {
    Path(Gc<'gc, dom::Path2d>),
    String(&'a str),
}

impl ArgType for OptionalPath2dOrString {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::StringOr(&["Path2D"]);
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<Path2dOrStringTaken<T::Node, T::Str>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<Path2dOrString<'a, 'gc>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>> {
        if front.is_undefined(index) {
            return Some(None);
        }
        if let Some(path) = front.object_if::<dom::Path2d>(index) {
            return Some(Some(Path2dOrStringTaken::Path(path)));
        }
        front
            .string_arg(index)
            .map(|string| Some(Path2dOrStringTaken::String(string)))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Self::Taken<O>,
        _: usize,
    ) -> Result<Option<Path2dOrString<'a, 'gc>>, ApiError> {
        Ok(taken.as_ref().map(|taken| match taken {
            Path2dOrStringTaken::Path(path) => Path2dOrString::Path(op.object(cx, path)),
            Path2dOrStringTaken::String(string) => Path2dOrString::String(op.str_param(string)),
        }))
    }
}

/// `roundRect`'s optional `radii` (`Front::round_rect_radii_arg`): the
/// radii as script gave them, one to four checked in the operation. Any
/// value to the table (its union converts any).
pub struct RoundRectRadii;

impl ArgType for RoundRectRadii {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Vec<Radius>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a [Radius];

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Vec<Radius>> {
        front.round_rect_radii_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Vec<Radius>,
        _: usize,
    ) -> Result<&'a [Radius], ApiError> {
        Ok(taken)
    }
}

/// `getKeyframes()`'s value: a new object for each keyframe, its members
/// in order (JS's only: keyframes are script objects).
pub struct Records;

impl RetType for Records {
    const DESC: TypeDesc = TypeDesc::Any;
    finished_after!(Vec<Vec<(String, DictValue)>>, finish_records);
}

/// A dictionary result whose members are given at run time (`getTiming()`,
/// `getComputedTiming()`: a `duration` a number or `"auto"`, a member
/// `null`).
pub struct DictionaryOut;

impl RetType for DictionaryOut {
    const DESC: TypeDesc = TypeDesc::Any;
    finished_after!(Vec<(&'static str, DictValue)>, finish_dictionary);
}

/// `animate()`'s options as the front end read them
/// (`Front::animation_options_arg`): a number is its duration.
#[derive(Debug, Clone)]
pub struct AnimationOptions {
    pub timing: animation::OptionalTiming,
    /// `composite`: machopo composites every effect as `replace`.
    pub composite: String,
    pub pseudo_element: Option<String>,
    pub id: String,
    /// `timeline`: missing (its document's) or a `DocumentTimeline`, not
    /// `null`.
    pub has_timeline: bool,
}

impl Default for AnimationOptions {
    fn default() -> Self {
        Self {
            timing: animation::OptionalTiming {
                duration: Some(animation::DurationInput::Text("auto".into())),
                fill: Some(animation::FillInput::Auto),
                ..Default::default()
            },
            composite: "replace".into(),
            pseudo_element: None,
            id: String::new(),
            has_timeline: true,
        }
    }
}

impl ArgType for AnimationOptions {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = AnimationOptions;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a AnimationOptions;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<AnimationOptions> {
        front.animation_options_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a AnimationOptions,
        _: usize,
    ) -> Result<&'a AnimationOptions, ApiError> {
        Ok(taken)
    }
}

/// `new KeyframeEffect()`'s options (`Front::effect_options_arg`).
pub struct EffectOptions;

impl ArgType for EffectOptions {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = AnimationOptions;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a AnimationOptions;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<AnimationOptions> {
        front.effect_options_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a AnimationOptions,
        _: usize,
    ) -> Result<&'a AnimationOptions, ApiError> {
        Ok(taken)
    }
}

/// `updateTiming()`'s `OptionalEffectTiming` (`Front::optional_timing_arg`).
pub struct OptionalTimingArg;

impl ArgType for OptionalTimingArg {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = animation::OptionalTiming;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a animation::OptionalTiming;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<animation::OptionalTiming> {
        front.optional_timing_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a animation::OptionalTiming,
        _: usize,
    ) -> Result<&'a animation::OptionalTiming, ApiError> {
        Ok(taken)
    }
}

/// `new Animation()`'s `timeline` (`AnimationTimeline?`, its absence its
/// document's): whether it has one (`Front::timeline_arg`).
pub struct TimelineArg;

impl ArgType for TimelineArg {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = bool;
    type Param<'a, 'gc, T: FrontTypes + 'a> = bool;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<bool> {
        front.timeline_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a bool,
        _: usize,
    ) -> Result<bool, ApiError> {
        Ok(*taken)
    }
}

/// `ready` and `finished`: `Promise<Animation>`, kept on the animation's
/// object (Blink's `ScriptPromiseProperty`).
pub struct AnimationPromise<'gc> {
    pub object: Gc<'gc, dom::EventTargetImpl>,
    /// `ready`'s, else `finished`'s.
    pub ready: bool,
    /// Its animation's is settled already: resolved at once.
    pub resolved: bool,
}

pub struct PromiseOfAnimation;

impl RetType for PromiseOfAnimation {
    const DESC: TypeDesc = TypeDesc::Promise("Animation");
    const PLAIN: bool = true;
    type Ret<'gc> = AnimationPromise<'gc>;
    no_prepared!();
    /// The animation's object rooted, its interface, which promise and
    /// whether it is resolved.
    type Pending<F: FrontTypes> = (Persistent<EventTargetImpl>, usize, bool, bool);

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        promise: AnimationPromise<'gc>,
    ) -> (Persistent<EventTargetImpl>, usize, bool, bool) {
        (
            Persistent::new(cx.gc, promise.object),
            interface_of(cx, ObjectRef::EventTarget(promise.object)),
            promise.ready,
            promise.resolved,
        )
    }

    #[inline(always)]
    fn finish<F: Front>(
        front: &mut F,
        (object, interface, ready, resolved): (Persistent<EventTargetImpl>, usize, bool, bool),
    ) {
        front.finish_animation_promise(object, interface, ready, resolved);
    }
}

/// `Animation`'s, `CSSTransition`'s and `CSSAnimation`'s objects: event
/// targets with an animation's data.
pub struct AnimationObject;

impl ObjectType for AnimationObject {
    type Ref<'gc> = Gc<'gc, dom::EventTargetImpl>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let target = op
            .this_as::<dom::EventTargetImpl>(cx)
            .expect("the front end's checks: `this` is an event target");
        debug_assert!(
            cx.is_animation_object(target),
            "the front end's checks: `this` is an Animation"
        );
        target
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::EventTarget(object)
    }
}

impl ObjectType for dom::KeyframeEffectObject {
    type Ref<'gc> = Gc<'gc, dom::KeyframeEffectObject>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::KeyframeEffectObject>(cx)
            .expect("the front end's checks: `this` is an effect")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::KeyframeEffect(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::KeyframeEffectObject>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::KeyframeEffectObject>(cx, taken)
    }
}

impl ObjectType for dom::DocumentTimeline {
    type Ref<'gc> = Gc<'gc, dom::DocumentTimeline>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::DocumentTimeline>(cx)
            .expect("the front end's checks: `this` is a timeline")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::DocumentTimeline(object)
    }

    fn take_arg<F: Front>(front: &mut F, index: usize, interface: &str) -> Option<Option<F::Node>> {
        front.object_arg::<dom::DocumentTimeline>(index, interface)
    }

    fn arg<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, taken: &O::Node) -> Self::Ref<'gc> {
        op.object::<dom::DocumentTimeline>(cx, taken)
    }
}

/// A keyframes argument (`animate()`'s `object? keyframes`), as the front
/// end read it (`Front::keyframes_arg`): processed by the member.
pub struct Keyframes;

impl ArgType for Keyframes {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<animation::ScriptKeyframes>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<&'a animation::ScriptKeyframes>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<animation::ScriptKeyframes>> {
        front.keyframes_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<animation::ScriptKeyframes>,
        _: usize,
    ) -> Result<Option<&'a animation::ScriptKeyframes>, ApiError> {
        Ok(taken.as_ref())
    }
}

/// `toBlob`'s `BlobCallback`: a function, kept by the front end; anything
/// else a `TypeError`.
pub struct BlobCallback;

impl ArgType for BlobCallback {
    const DESC: TypeDesc = TypeDesc::Function("BlobCallback");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Callback<T::Callback>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
        front.function_arg(index, "BlobCallback")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Callback<O::Callback>,
        _: usize,
    ) -> Result<Callback<O::Callback>, ApiError> {
        Ok(*taken)
    }
}

/// `hidden`'s `(boolean or unrestricted double or DOMString)?`, as WebIDL
/// converts it: null or undefined none, a boolean or a number as it is,
/// anything else `ToString`.
pub enum HiddenValue<S> {
    Bool(bool),
    Number(f64),
    String(S),
}

/// `hidden` as an argument (`HiddenValue`) and as a result: the string
/// `until-found` or a boolean.
pub struct Hidden;

impl ArgType for Hidden {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    type Taken<T: FrontTypes> = Option<HiddenValue<T::Str>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<HiddenValue<&'a str>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<HiddenValue<F::Str>>> {
        if front.is_null_or_undefined(index) {
            return Some(None);
        }
        if let Some(value) = front.bool_if(index) {
            return Some(Some(HiddenValue::Bool(value)));
        }
        if let Some(value) = front.number_if(index) {
            return Some(Some(HiddenValue::Number(value)));
        }
        front
            .string_arg(index)
            .map(|value| Some(HiddenValue::String(value)))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<HiddenValue<O::Str>>,
        _: usize,
    ) -> Result<Option<HiddenValue<&'a str>>, ApiError> {
        Ok(taken.as_ref().map(|value| match value {
            HiddenValue::Bool(value) => HiddenValue::Bool(*value),
            HiddenValue::Number(value) => HiddenValue::Number(*value),
            HiddenValue::String(value) => HiddenValue::String(op.str_param(value)),
        }))
    }
}

/// `hidden`'s state as its getter gives it.
pub enum HiddenState {
    /// Whether the attribute is there.
    Hidden(bool),
    /// In the hidden until found state: the string `until-found`.
    UntilFound,
}

impl RetType for Hidden {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = HiddenState;
    pending!(PendingString, finish_string);

    fn give<'gc, O: OpFront>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        value: HiddenState,
    ) -> Self::Pending<O> {
        match value {
            HiddenState::Hidden(hidden) => {
                op.ret_bool(hidden);
                None
            }
            HiddenState::UntilFound => DomStr::Owned(dom::DomString::new("until-found")).give(op),
        }
    }
}

/// `toDataURL`'s and `toBlob`'s `optional any quality`: its value if it
/// is a number (HTML reads nothing else), else none; no conversion, so it
/// runs no script.
pub struct NumberIfNumber;

impl ArgType for NumberIfNumber {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<f64>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<f64>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<f64>> {
        Some(front.number_if(index))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<f64>,
        _: usize,
    ) -> Result<Option<f64>, ApiError> {
        Ok(*taken)
    }
}

/// An optional `[Clamp] long long` (`slice`'s): none if missing or
/// undefined; else `ToNumber`, NaN 0, clamped to a safe integer's range
/// (±(2^53 − 1)) and rounded to the nearest, ties to even (WebIDL's
/// `ConvertToInt`). A number to the table (as `double`: no other member
/// takes a 64-bit integer).
pub struct OptionalClampedI64;

/// WebIDL's largest safe integer, `long long`'s bound with `[Clamp]`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

impl ArgType for OptionalClampedI64 {
    const DESC: TypeDesc = TypeDesc::Double;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<i64>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<i64>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<i64>> {
        if front.is_undefined(index) {
            return Some(None);
        }
        let value = front.f64_arg(index, false)?;
        let value = if value.is_nan() {
            0.0
        } else {
            value
                .clamp(-MAX_SAFE_INTEGER, MAX_SAFE_INTEGER)
                .round_ties_even()
        };
        Some(Some(base::numerics::clamp_floor::<i64>(value)))
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<i64>,
        _: usize,
    ) -> Result<Option<i64>, ApiError> {
        Ok(*taken)
    }
}

/// Where a message's transfer list is (`Front::message_arg`): an
/// argument (WebIDL's `optional sequence<object> transfer = []`: undefined
/// is none, anything that isn't a sequence a `TypeError`), an options
/// dictionary argument's `transfer` (`StructuredSerializeOptions`), or
/// either, the overloads of a port's `postMessage` told apart as WebIDL
/// does (an iterable object is the sequence).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferArg {
    Sequence(usize),
    Options(usize),
    SequenceOrOptions(usize),
}

/// What an `ArrayBufferView` argument is (`Front::array_view_arg`): its
/// interface's name (`Uint8Array`, `DataView`), whether it is an integer
/// typed array, and how many bytes it views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayViewArg {
    pub name: &'static str,
    pub integer: bool,
    pub byte_length: usize,
}

/// An `ArrayBufferView` the front end keeps for the call
/// (`getRandomValues`' argument), as what it is.
pub struct ArrayView;

impl ArgType for ArrayView {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = false;
    type Taken<T: FrontTypes> = ArrayViewArg;
    type Param<'a, 'gc, T: FrontTypes + 'a> = ArrayViewArg;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<ArrayViewArg> {
        front.array_view_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a ArrayViewArg,
        _: usize,
    ) -> Result<ArrayViewArg, ApiError> {
        Ok(*taken)
    }
}

/// The `ArrayView` argument as a result, its bytes these now
/// (`OpFront::ret_filled_view`).
pub struct FilledView;

impl RetType for FilledView {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    type Ret<'gc> = Vec<u8>;
    no_prepared!();
    type Pending<F: FrontTypes> = Vec<u8>;

    #[inline(always)]
    fn give<'gc, O: OpFront>(_: &mut O, _: &ApiCx<'_, 'gc>, bytes: Vec<u8>) -> Vec<u8> {
        bytes
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, bytes: Vec<u8>) {
        front.finish_filled_view(bytes);
    }
}

/// `structuredClone`'s value and options, serialized by the front end
/// (`Front::clone_arg`).
pub struct CloneValue;

/// `CloneValue` as taken: the serialization, taken by the operation, and
/// the ports transferred.
pub struct CloneTaken<N> {
    payload: std::cell::Cell<Option<dom::SerializedMessage>>,
    ports: Vec<N>,
}

/// `CloneValue` in the operation.
pub struct CloneParam<'a, 'gc> {
    pub payload: &'a std::cell::Cell<Option<dom::SerializedMessage>>,
    pub ports: Vec<Gc<'gc, dom::EventTargetImpl>>,
}

impl ArgType for CloneValue {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = false;
    type Taken<T: FrontTypes> = CloneTaken<T::Node>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = CloneParam<'a, 'gc>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<CloneTaken<F::Node>> {
        let (payload, ports) = front.clone_arg(index)?;
        Some(CloneTaken {
            payload: std::cell::Cell::new(Some(payload)),
            ports,
        })
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a CloneTaken<O::Node>,
        _: usize,
    ) -> Result<CloneParam<'a, 'gc>, ApiError> {
        Ok(CloneParam {
            payload: &taken.payload,
            ports: taken
                .ports
                .iter()
                .map(|port| op.object::<dom::EventTargetImpl>(cx, port))
                .collect(),
        })
    }
}

/// `structuredClone`'s result: a serialization and its ports, deserialized
/// by the front end in the current realm (`OpFront::ret_cloned`).
pub struct Cloned;

impl RetType for Cloned {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = (dom::SerializedMessage, Vec<Gc<'gc, dom::EventTargetImpl>>);
    no_prepared!();
    type Pending<F: FrontTypes> = (dom::SerializedMessage, Vec<(ObjectRoot, usize)>);

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        (payload, ports): Self::Ret<'gc>,
    ) -> Self::Pending<O> {
        let ports = ports
            .into_iter()
            .map(|port| {
                let object = ObjectRef::EventTarget(port);
                (cx.root_object(object), interface_of(cx, object))
            })
            .collect();
        (payload, ports)
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, (payload, ports): Self::Pending<F>) {
        front.finish_cloned(payload, ports);
    }
}

/// `FileReader.result`'s `(DOMString or ArrayBuffer)?` (the reader's
/// result as it stands): an `ArrayBuffer` the same one each time for a
/// read (`OpFront::ret_reader_buffer`).
pub struct ReaderResult;

/// What a `FileReader`'s `result` leaves for after the operation: a
/// string its engine has no copy of, or its buffer to make.
pub enum ReaderPending<S> {
    String(S),
    Buffer(Persistent<EventTargetImpl>),
}

impl RetType for ReaderResult {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = Gc<'gc, dom::EventTargetImpl>;
    no_prepared!();
    type Pending<F: FrontTypes> = Option<ReaderPending<F::PendingString>>;

    fn give<'gc, O: OpFront>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        reader: Gc<'gc, dom::EventTargetImpl>,
    ) -> Self::Pending<O> {
        match cx.file_reader_result(reader) {
            dom::FileReaderResult::None => {
                op.ret_null();
                None
            }
            dom::FileReaderResult::Text(text) => op
                .ret_string(&DomString::new(&text))
                .map(ReaderPending::String),
            dom::FileReaderResult::Bytes(_) => {
                Some(ReaderPending::Buffer(Persistent::new(cx.gc, reader)))
            }
        }
    }

    fn finish<F: Front>(front: &mut F, pending: Self::Pending<F>) {
        match pending {
            Some(ReaderPending::String(string)) => front.finish_string(string),
            Some(ReaderPending::Buffer(reader)) => front.finish_reader_buffer(reader),
            None => {}
        }
    }
}

/// WebIDL's `(DOMString or DOMHighResTimeStamp)` (User Timing's marks), as
/// the front end took it.
#[derive(Debug, Clone, PartialEq)]
pub enum StringOrNumber {
    String(String),
    Number(f64),
}

/// A dictionary member `(DOMString or DOMHighResTimeStamp)` with no
/// default: none if it is missing.
impl DictMember for Option<StringOrNumber> {
    const PLAIN: bool = true;

    fn read<F: Front>(
        front: &mut F,
        dict: &F::Dict,
        member: MemberName,
    ) -> Option<Option<Option<StringOrNumber>>> {
        Some(front.dict_string_or_number(dict, member)?.map(Some))
    }
}

/// A dictionary member `any` structured-serialized as it is read (User
/// Timing's `detail`): none if it is missing (its default, `null`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serialized(pub Vec<u8>);

impl DictMember for Option<Serialized> {
    const PLAIN: bool = true;
    fn read<F: Front>(
        front: &mut F,
        dict: &F::Dict,
        member: MemberName,
    ) -> Option<Option<Option<Serialized>>> {
        Some(
            front
                .dict_serialized(dict, member)?
                .map(|bytes| Some(Serialized(bytes))),
        )
    }
}

/// `measure()`'s `(DOMString or PerformanceMeasureOptions)`, optional: the
/// options for an object, null or undefined (WebIDL's union with a
/// dictionary), the start mark's name for anything else; none if missing.
pub struct MeasureStart;

/// `MeasureStart` as taken and in the operation.
pub enum MeasureStartArg<S> {
    None,
    Mark(S),
    Options(crate::interfaces::PerformanceMeasureOptions),
}

impl ArgType for MeasureStart {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Any;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = MeasureStartArg<T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = MeasureStartParam<'a>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<MeasureStartArg<F::Str>> {
        // Missing or undefined: not given (WebIDL's optional argument).
        if front.is_undefined(index) {
            return Some(MeasureStartArg::None);
        }
        if front.is_dictionary_like(index) {
            return crate::interfaces::PerformanceMeasureOptions::read(front, index)
                .map(MeasureStartArg::Options);
        }
        front.string_arg(index).map(MeasureStartArg::Mark)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a MeasureStartArg<O::Str>,
        _: usize,
    ) -> Result<MeasureStartParam<'a>, ApiError> {
        Ok(match taken {
            MeasureStartArg::None => MeasureStartParam::None,
            MeasureStartArg::Mark(mark) => MeasureStartParam::Mark(op.str_param(mark)),
            MeasureStartArg::Options(options) => MeasureStartParam::Options(options),
        })
    }
}

pub enum MeasureStartParam<'a> {
    None,
    Mark(&'a str),
    Options(&'a crate::interfaces::PerformanceMeasureOptions),
}

/// Performance Timeline's `PerformanceObserverCallback`, as
/// `MutationCallback`.
pub struct PerformanceObserverCallback;

impl ArgType for PerformanceObserverCallback {
    const DESC: TypeDesc = TypeDesc::Function("PerformanceObserverCallback");
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Callback<T::Callback>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Callback<T::Callback>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Callback<F::Callback>> {
        front.function_arg(index, "PerformanceObserverCallback")
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Callback<O::Callback>,
        _: usize,
    ) -> Result<Callback<O::Callback>, ApiError> {
        Ok(*taken)
    }
}

/// A mark's or measure's `detail` (`any`): `OpFront::ret_entry_detail`.
pub struct EntryDetail;

impl RetType for EntryDetail {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = Gc<'gc, dom::PerformanceEntry>;
    no_prepared!();
    type Pending<F: FrontTypes> = Persistent<dom::PerformanceEntry>;

    fn give<'gc, O: OpFront>(
        _: &mut O,
        cx: &ApiCx<'_, 'gc>,
        entry: Self::Ret<'gc>,
    ) -> Persistent<dom::PerformanceEntry> {
        Persistent::new(cx.gc, entry)
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, entry: Persistent<dom::PerformanceEntry>) {
        front.finish_entry_detail(entry);
    }
}

macro_rules! marked_objects {
    ($($(#[$meta:meta])* $name:ident: $ty:ty, $check:ident, $what:literal, $variant:ident;)*) => {$(
        $(#[$meta])*
        pub struct $name;

        impl ObjectType for $name {
            type Ref<'gc> = Gc<'gc, $ty>;

            #[inline]
            fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
                let object = op
                    .this_as::<$ty>(cx)
                    .expect(concat!("the front end's checks: `this` is ", $what));
                debug_assert!($check(cx, object), concat!("the front end's checks: `this` is ", $what));
                object
            }

            fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
                ObjectRef::$variant(object)
            }
        }
    )*};
}

fn is_performance<'gc>(cx: &ApiCx<'_, 'gc>, target: Gc<'gc, dom::EventTargetImpl>) -> bool {
    cx.is_performance(target)
}

fn is_entry_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    entry: Gc<'gc, dom::PerformanceEntry>,
    of: &[dom::PerformanceEntryType],
) -> bool {
    of.contains(&cx.performance_entry_type(entry))
}

fn is_mark<'gc>(cx: &ApiCx<'_, 'gc>, entry: Gc<'gc, dom::PerformanceEntry>) -> bool {
    is_entry_of(cx, entry, &[dom::PerformanceEntryType::Mark])
}

fn is_measure<'gc>(cx: &ApiCx<'_, 'gc>, entry: Gc<'gc, dom::PerformanceEntry>) -> bool {
    is_entry_of(cx, entry, &[dom::PerformanceEntryType::Measure])
}

fn is_navigation<'gc>(cx: &ApiCx<'_, 'gc>, entry: Gc<'gc, dom::PerformanceEntry>) -> bool {
    is_entry_of(cx, entry, &[dom::PerformanceEntryType::Navigation])
}

fn is_any_entry<'gc>(_: &ApiCx<'_, 'gc>, _: Gc<'gc, dom::PerformanceEntry>) -> bool {
    true
}

fn is_timing<'gc>(cx: &ApiCx<'_, 'gc>, timing: Gc<'gc, dom::PerformanceTiming>) -> bool {
    cx.performance_timing_kind(timing) == dom::PerformanceTimingKind::Timing
}

fn is_legacy_navigation<'gc>(cx: &ApiCx<'_, 'gc>, timing: Gc<'gc, dom::PerformanceTiming>) -> bool {
    cx.performance_timing_kind(timing) == dom::PerformanceTimingKind::Navigation
}

fn is_confidence<'gc>(cx: &ApiCx<'_, 'gc>, timing: Gc<'gc, dom::PerformanceTiming>) -> bool {
    matches!(
        cx.performance_timing_kind(timing),
        dom::PerformanceTimingKind::Confidence(_)
    )
}

marked_objects! {
    /// A window's `performance`: an event target with its data.
    PerformanceObject: dom::EventTargetImpl, is_performance, "a performance", EventTarget;
    PerformanceEntryObject: dom::PerformanceEntry, is_any_entry, "a performance entry", PerformanceEntry;
    PerformanceMarkObject: dom::PerformanceEntry, is_mark, "a mark", PerformanceEntry;
    PerformanceMeasureObject: dom::PerformanceEntry, is_measure, "a measure", PerformanceEntry;
    /// A navigation entry (`PerformanceResourceTiming`'s too: machopo has no
    /// resource entries).
    NavigationTimingObject: dom::PerformanceEntry, is_navigation, "a navigation entry", PerformanceEntry;
    PerformanceTimingObject: dom::PerformanceTiming, is_timing, "performance.timing", PerformanceTiming;
    PerformanceNavigationObject: dom::PerformanceTiming, is_legacy_navigation, "performance.navigation", PerformanceTiming;
    ConfidenceObject: dom::PerformanceTiming, is_confidence, "a confidence", PerformanceTiming;
    /// An abort signal: an event target with its state.
    AbortSignalObject: dom::EventTargetImpl, is_abort_signal, "an abort signal", EventTarget;
    /// A wake lock's sentinel: an event target with its data.
    WakeLockSentinelObject: dom::EventTargetImpl, is_wake_lock_sentinel, "a wake lock sentinel", EventTarget;
    /// A navigator's `connection`: an event target of its window's.
    NetworkInformationObject: dom::EventTargetImpl, is_network_information, "a network information", EventTarget;
    /// `GeolocationPosition`'s and `GeolocationCoordinates`': none is made
    /// (machopo has no position), so the front end's check stops every call.
    NoPositionObject: dom::GeolocationPositionError, no_position, "a geolocation position", GeolocationPositionError;
    /// An interface none of whose objects is made (`PerformanceServerTiming`,
    /// `NotRestoredReasons`: machopo has no server timing or back-forward
    /// cache): the front end's check stops every call.
    NoObject: dom::GeolocationPositionError, no_position, "an object", GeolocationPositionError;
}

fn is_abort_signal<'gc>(cx: &ApiCx<'_, 'gc>, target: Gc<'gc, dom::EventTargetImpl>) -> bool {
    cx.is_abort_signal(target)
}

fn is_wake_lock_sentinel<'gc>(cx: &ApiCx<'_, 'gc>, target: Gc<'gc, dom::EventTargetImpl>) -> bool {
    cx.is_wake_lock_sentinel(target)
}

fn is_network_information<'gc>(cx: &ApiCx<'_, 'gc>, target: Gc<'gc, dom::EventTargetImpl>) -> bool {
    cx.is_network_information(target)
}

fn no_position<'gc>(_: &ApiCx<'_, 'gc>, _: Gc<'gc, dom::GeolocationPositionError>) -> bool {
    false
}

impl ObjectType for dom::IdleDeadline {
    type Ref<'gc> = Gc<'gc, dom::IdleDeadline>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::IdleDeadline>(cx)
            .expect("the front end's checks: `this` is an idle deadline")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::IdleDeadline(object)
    }
}

impl ObjectType for dom::Geolocation {
    type Ref<'gc> = Gc<'gc, dom::Geolocation>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Geolocation>(cx)
            .expect("the front end's checks: `this` is a geolocation")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Geolocation(object)
    }
}

impl ObjectType for dom::GeolocationPositionError {
    type Ref<'gc> = Gc<'gc, dom::GeolocationPositionError>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::GeolocationPositionError>(cx)
            .expect("the front end's checks: `this` is a geolocation position error")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::GeolocationPositionError(object)
    }
}

impl ObjectType for dom::WakeLock {
    type Ref<'gc> = Gc<'gc, dom::WakeLock>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::WakeLock>(cx)
            .expect("the front end's checks: `this` is a wake lock")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::WakeLock(object)
    }
}

impl ObjectType for dom::Scheduling {
    type Ref<'gc> = Gc<'gc, dom::Scheduling>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Scheduling>(cx)
            .expect("the front end's checks: `this` is a scheduling")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Scheduling(object)
    }
}

impl ObjectType for dom::AbortController {
    type Ref<'gc> = Gc<'gc, dom::AbortController>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::AbortController>(cx)
            .expect("the front end's checks: `this` is an abort controller")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::AbortController(object)
    }
}

impl ObjectType for dom::Headers {
    type Ref<'gc> = Gc<'gc, dom::Headers>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Headers>(cx)
            .expect("the front end's checks: `this` is a headers")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Headers(object)
    }
}

impl ObjectType for dom::Request {
    type Ref<'gc> = Gc<'gc, dom::Request>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Request>(cx)
            .expect("the front end's checks: `this` is a request")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Request(object)
    }
}

impl ObjectType for dom::Response {
    type Ref<'gc> = Gc<'gc, dom::Response>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Response>(cx)
            .expect("the front end's checks: `this` is a response")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Response(object)
    }
}

impl ObjectType for dom::PerformanceObserver {
    type Ref<'gc> = Gc<'gc, dom::PerformanceObserver>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::PerformanceObserver>(cx)
            .expect("the front end's checks: `this` is a performance observer")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::PerformanceObserver(object)
    }
}

impl ObjectType for dom::PerformanceObserverEntryList {
    type Ref<'gc> = Gc<'gc, dom::PerformanceObserverEntryList>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::PerformanceObserverEntryList>(cx)
            .expect("the front end's checks: `this` is an entry list")
    }

    fn object<'gc>(object: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::PerformanceObserverEntryList(object)
    }
}

/// A result that is always `null` (`FileReader.error`'s `DOMException?`:
/// reading bytes in memory doesn't fail).
pub struct AlwaysNull;

impl RetType for AlwaysNull {
    const DESC: TypeDesc = TypeDesc::Any;
    const PLAIN: bool = true;
    type Ret<'gc> = ();
    no_pending!();

    fn give<'gc, O: OpFront>(op: &mut O, _: &ApiCx<'_, 'gc>, (): ()) {
        op.ret_null();
    }
}

/// `File`'s objects: blobs with a file's name and time.
pub struct FileObject;

impl ObjectType for FileObject {
    type Ref<'gc> = Gc<'gc, dom::Blob>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        op.this_as::<dom::Blob>(cx)
            .expect("the front end's checks: `this` is a blob")
    }

    fn object<'gc>(blob: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Blob(blob)
    }
}

/// `FileReader`'s objects: event targets with a reader's data.
pub struct FileReaderObject;

impl ObjectType for FileReaderObject {
    type Ref<'gc> = Gc<'gc, dom::EventTargetImpl>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let reader = op
            .this_as::<dom::EventTargetImpl>(cx)
            .expect("the front end's checks: `this` is an event target");
        debug_assert!(
            cx.is_file_reader(reader),
            "the front end's checks: `this` is a FileReader"
        );
        reader
    }

    fn object<'gc>(reader: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::EventTarget(reader)
    }
}

/// `ProgressEvent`'s objects: events with its data.
pub struct ProgressEventObject;

impl ObjectType for ProgressEventObject {
    type Ref<'gc> = Gc<'gc, dom::Event>;

    #[inline]
    fn this<'gc, O: OpFront>(op: &mut O, cx: &ApiCx<'_, 'gc>, _: usize) -> Self::Ref<'gc> {
        let event = op
            .this_as::<dom::Event>(cx)
            .expect("the front end's checks: `this` is an event");
        debug_assert!(
            cx.progress_event_data(event).is_some(),
            "the front end's checks: `this` is a ProgressEvent"
        );
        event
    }

    fn object<'gc>(event: Self::Ref<'gc>) -> ObjectRef<'gc> {
        ObjectRef::Event(event)
    }
}

/// What `Front::string_pairs_arg` took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringPairsArg {
    /// Each item's strings (a pair's, if it has two).
    Sequences(Vec<Vec<String>>),
    Record(Vec<(String, String)>),
    String(String),
}

/// An optional `(sequence<sequence<USVString>> or record<USVString,
/// USVString> or USVString)` whose default is the empty string
/// (`URLSearchParams`' init), as `Front::string_pairs_arg` takes it.
pub struct StringPairs;

/// `StringPairs` in the operation.
pub enum StringPairsInit<'a> {
    Sequences(&'a [Vec<String>]),
    Record(&'a [(String, String)]),
    String(&'a str),
}

impl ArgType for StringPairs {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::StringPairs;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<StringPairsArg>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = StringPairsInit<'a>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<StringPairsArg>> {
        if front.is_undefined(index) {
            return Some(None);
        }
        front.string_pairs_arg(index).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        _: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<StringPairsArg>,
        _: usize,
    ) -> Result<StringPairsInit<'a>, ApiError> {
        Ok(match taken {
            None => StringPairsInit::String(""),
            Some(StringPairsArg::Sequences(items)) => StringPairsInit::Sequences(items),
            Some(StringPairsArg::Record(entries)) => StringPairsInit::Record(entries),
            Some(StringPairsArg::String(string)) => StringPairsInit::String(string),
        })
    }
}

/// An optional `sequence<BlobPart>` (`new Blob()`'s): none if missing or
/// undefined; each item as the front end took it (`Front::blob_parts_arg`),
/// a blob made the DOM's in the operation.
pub struct BlobParts;

/// A part of a blob, in the operation.
pub enum BlobPart<'a, 'gc> {
    String(&'a str),
    Bytes(&'a [u8]),
    Blob(Gc<'gc, dom::Blob>),
}

impl ArgType for BlobParts {
    const PLAIN: bool = true;
    const DESC: TypeDesc = TypeDesc::Sequence("BlobPart");
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<Vec<BlobPartArg<T::Node>>>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Vec<BlobPart<'a, 'gc>>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Self::Taken<F>> {
        front.blob_parts_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        cx: &ApiCx<'_, 'gc>,
        taken: &'a Self::Taken<O>,
        _: usize,
    ) -> Result<Vec<BlobPart<'a, 'gc>>, ApiError> {
        Ok(taken
            .iter()
            .flatten()
            .map(|part| match part {
                BlobPartArg::String(string) => BlobPart::String(string),
                BlobPartArg::Bytes(bytes) => BlobPart::Bytes(bytes),
                BlobPartArg::Blob(blob) => BlobPart::Blob(op.object::<dom::Blob>(cx, blob)),
            })
            .collect())
    }
}

/// WebIDL's `Promise<undefined>` (`scrollTo()`): a new promise, resolved
/// once the member's steps are done; a failure rejects it.
pub struct PromiseOfUndefined;

impl RetType for PromiseOfUndefined {
    const DESC: TypeDesc = TypeDesc::Promise("undefined");
    const PLAIN: bool = true;
    type Ret<'gc> = ();
    no_prepared!();
    type Pending<F: FrontTypes> = ();

    #[inline(always)]
    fn give<'gc, O: OpFront>(_: &mut O, _: &ApiCx<'_, 'gc>, (): ()) {}

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, (): ()) {
        front.finish_resolved_undefined();
    }

    fn fail<F: Front>(front: &mut F, error: ApiError) {
        front.reject(error);
    }
}

/// WebIDL's `Promise<USVString>` (`text()`): a new promise, resolved with
/// the string; a failure rejects it.
pub struct PromiseOfString;

impl RetType for PromiseOfString {
    const DESC: TypeDesc = TypeDesc::Promise("USVString");
    finished_after!(DomString, finish_resolved_string);

    fn fail<F: Front>(front: &mut F, error: ApiError) {
        front.reject(error);
    }
}

/// WebIDL's `Promise<ArrayBuffer>` (`arrayBuffer()`), or with `VIEW`
/// `Promise<Uint8Array>` (`bytes()`): a new promise, resolved with a new
/// buffer of the bytes; a failure rejects it.
pub struct PromiseOfBytes<const VIEW: bool>;

impl<const VIEW: bool> RetType for PromiseOfBytes<VIEW> {
    const DESC: TypeDesc = TypeDesc::Promise(if VIEW { "Uint8Array" } else { "ArrayBuffer" });
    const PLAIN: bool = true;
    type Ret<'gc> = Vec<u8>;
    no_prepared!();
    type Pending<F: FrontTypes> = Vec<u8>;

    #[inline(always)]
    fn give<'gc, O: OpFront>(_: &mut O, _: &ApiCx<'_, 'gc>, value: Vec<u8>) -> Vec<u8> {
        value
    }

    #[inline(always)]
    fn finish<F: Front>(front: &mut F, value: Vec<u8>) {
        front.finish_resolved_bytes(value, VIEW);
    }

    fn fail<F: Front>(front: &mut F, error: ApiError) {
        front.reject(error);
    }
}

pub type PromiseOfArrayBuffer = PromiseOfBytes<false>;
pub type PromiseOfUint8Array = PromiseOfBytes<true>;

/// WebIDL's `optional DOMString x = ""` (`initKeyboardEvent`'s `keyArg`):
/// undefined is the empty string, null is `"null"`.
pub struct EmptyStringDefault;

impl ArgType for EmptyStringDefault {
    const DESC: TypeDesc = TypeDesc::String;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a str;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<F::Str>> {
        if front.is_undefined(index) {
            return Some(None);
        }
        front.string_arg(index).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<O::Str>,
        _: usize,
    ) -> Result<&'a str, ApiError> {
        Ok(taken.as_ref().map_or("", |taken| op.str_param(taken)))
    }
}

/// An optional `DOMString` with no default (`new Option`'s `value`):
/// none if it is missing or undefined (WebIDL's "not present"), else
/// `ToString` (null is `"null"`).
pub struct OptionalString;

impl ArgType for OptionalString {
    const DESC: TypeDesc = TypeDesc::String;
    const PLAIN: bool = true;
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<&'a str>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<F::Str>> {
        if front.is_undefined(index) {
            return Some(None);
        }
        front.string_arg(index).map(Some)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<O::Str>,
        _: usize,
    ) -> Result<Option<&'a str>, ApiError> {
        Ok(taken.as_ref().map(|taken| op.str_param(taken)))
    }
}

/// `DOMString?`.
impl ArgType for Option<DomString> {
    const DESC: TypeDesc = TypeDesc::NullableString;
    const PLAIN: bool = true;
    /// An optional one's missing is undefined, which is none, as null.
    const UNDEFINED_IS_DEFAULT: bool = true;
    type Taken<T: FrontTypes> = Option<T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = Option<&'a str>;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<F::Str>> {
        front.nullable_string_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<O::Str>,
        _: usize,
    ) -> Result<Option<&'a str>, ApiError> {
        Ok(taken.as_ref().map(|taken| op.str_param(taken)))
    }
}

impl RetType for Option<DomString> {
    const DESC: TypeDesc = TypeDesc::NullableString;
    const PLAIN: bool = true;
    type Ret<'gc> = NullableStr<'gc>;
    pending!(PendingString, finish_string);

    // As `DomStr::give`: inlined into the member's call.
    #[inline(always)]
    fn give<'gc, O: OpFront>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        value: NullableStr<'gc>,
    ) -> Self::Pending<O> {
        match value.0 {
            Some(value) => value.give(op),
            None => {
                op.ret_null();
                None
            }
        }
    }
}

/// `[LegacyNullToEmptyString] DOMString`: null is the empty string. A
/// getter of such an attribute returns a `DOMString`.
pub struct LegacyNullToEmptyString;

impl ArgType for LegacyNullToEmptyString {
    const DESC: TypeDesc = TypeDesc::NullableString;
    const PLAIN: bool = true;
    type Taken<T: FrontTypes> = Option<T::Str>;
    type Param<'a, 'gc, T: FrontTypes + 'a> = &'a str;

    fn take<F: Front>(front: &mut F, index: usize) -> Option<Option<F::Str>> {
        front.legacy_null_string_arg(index)
    }

    fn param<'a, 'gc, O: OpFront + 'a>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        taken: &'a Option<O::Str>,
        _: usize,
    ) -> Result<&'a str, ApiError> {
        Ok(taken.as_ref().map_or("", |taken| op.str_param(taken)))
    }
}

impl RetType for LegacyNullToEmptyString {
    const DESC: TypeDesc = TypeDesc::String;
    const PLAIN: bool = true;
    type Ret<'gc> = DomStr<'gc>;
    pending!(PendingString, finish_string);

    // As `DomStr::give`: inlined into the member's call.
    #[inline(always)]
    fn give<'gc, O: OpFront>(
        op: &mut O,
        _: &ApiCx<'_, 'gc>,
        value: DomStr<'gc>,
    ) -> Self::Pending<O> {
        value.give(op)
    }
}

/// An interface's node argument, in the operation: a `TypeError` unless
/// the node is one of `I`'s.
pub(crate) fn node_param<'gc, I: Interface, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    taken: &O::Node,
    index: usize,
) -> Result<NodeRef<'gc>, ApiError> {
    let node = op.node(cx, taken);
    if !accepts_node(cx, I::INDEX, node) {
        return Err(ApiError::type_error(format!(
            "parameter {} is not of type '{}'",
            index.saturating_add(1),
            I::DESC.name
        )));
    }
    Ok(node)
}

/// A non-nullable interface's argument taken: null or undefined is a
/// `TypeError`.
pub(crate) fn take_node<I: Interface, F: Front>(front: &mut F, index: usize) -> Option<F::Node> {
    match front.node_arg(index)? {
        Some(node) => Some(node),
        None => {
            front.throw(ApiError::type_error(format!(
                "parameter {} is not of type '{}'",
                index.saturating_add(1),
                I::DESC.name
            )));
            None
        }
    }
}

/// The `Front` methods a `PLAIN` member's call may reach (its own code's,
/// and its types' `take` and `prepare`): what a front end that is given
/// only `PLAIN` members has. With `PLAIN_OP_METHODS`, what `PLAIN` means;
/// the tests check the types and the members' functions against them.
pub const PLAIN_FRONT_METHODS: &[&str] = &[
    "read",
    "mutate",
    "has_args",
    "reading",
    "whole",
    "construct",
    "prepare_window",
    "throw",
    "finish",
    "finish_string",
    "finish_node",
    "finish_number",
    "finish_object",
    "finish_handler",
    "finish_image_data_pixels",
    "finish_upgrading",
    "finish_pending_promise",
    "finish_resolved_constructor",
    "finish_objects",
    "finish_frozen_objects",
    "finish_strings",
    "finish_names",
    "finish_frozen_strings",
    "finish_doubles",
    "finish_numbers",
    "finish_dictionary",
    "finish_records",
    "finish_entry_detail",
    "finish_reader_buffer",
    "finish_history_state",
    "finish_cloned",
    "finish_resolved",
    "finish_resolved_string",
    "finish_resolved_json",
    "finish_resolved_undefined",
    "finish_resolved_bytes",
    "finish_animation_promise",
    "reject",
    "compile_event_handler",
    "invoke_ce_reactions",
    "arg_count",
    "is_undefined",
    "is_null_or_undefined",
    "is_dictionary_like",
    "string_arg",
    "nullable_string_arg",
    "legacy_null_string_arg",
    "bool_arg",
    "i32_arg",
    "u32_arg",
    "f64_arg",
    "node_arg",
    "node_or_string_arg",
    "node_or_long_arg",
    "object_arg",
    "target_arg",
    "callback_arg",
    "any_arg",
    "abort_reason_arg",
    "rest_arg",
    "dict_any",
    "dict_object",
    "dict_nullable_object",
    "dict_nullable_target",
    "dict_body",
    "dict_signal",
    "uint8_clamped_arg",
    "serialized_state_arg",
    "message_arg",
    "static_range_init",
    "intersection_observer_init",
    "intersection_entry_init",
    "objects_arg",
    "byte_string_pairs_arg",
    "object_if",
    "body_arg",
    "json_arg",
    "doubles_arg",
    "get_html_options",
    "round_rect_radii_arg",
    "animation_options_arg",
    "effect_options_arg",
    "optional_timing_arg",
    "timeline_arg",
    "keyframes_arg",
    "number_if",
    "bool_if",
    "clone_arg",
    "string_pairs_arg",
    "blob_parts_arg",
    "array_view_arg",
    "dict_headers",
    "dict_is_null",
    "dict_serialized",
    "finish_filled_view",
    "function_arg",
    "nullable_function_arg",
    "event_handler_arg",
    "timer_handler_arg",
    "dict_arg",
    "dict_bool",
    "dict_number",
    "dict_string",
    "dict_nullable_string",
    "dict_strings",
    "dict_string_or_number",
    "prepare_new_node",
    "prepare_node_like_this",
    "foresee_element",
];

/// The `OpFront` methods a `PLAIN` member's call may reach: its types'
/// `param` and `give`, and its function's, if it is given the front end's
/// side in the operation. (`current_window` is none by default: a `PLAIN`
/// member has none found for it.)
pub const PLAIN_OP_METHODS: &[&str] = &[
    "str_param",
    "this_as",
    "node",
    "object",
    "target",
    "same_callback",
    "store_callback",
    "construct_object",
    "construct_html",
    "new_target_number",
    "is_new_target",
    "with_array_bytes",
    "store_any",
    "store_rest",
    "callback_number",
    "current_window",
    "ret_undefined",
    "ret_any",
    "ret_any_or_null",
    "ret_any_or_undefined",
    "ret_null",
    "ret_bool",
    "ret_u32",
    "ret_i32",
    "ret_f64",
    "ret_string",
    "ret_fresh_string",
    "ret_name",
    "ret_node",
    "ret_object",
    "ret_new_node",
    "ret_construct",
];

/// A member's typed path, as a type of its own: `declare!` makes one for
/// each member.
pub trait TypedMember: 'static {
    /// Its interface's index in `INTERFACES`, and its index in the
    /// interface's `members`.
    const INTERFACE: usize;
    const MEMBER: usize;
    const NAME: &'static str;
    const KIND: MemberKind;
    /// Its arguments' types of the table (`ArgType`s), as a tuple, and its
    /// result's: what an engine expanding code of each member's own types
    /// (`for_each_member!`) converts them as.
    type Args;
    type Ret: RetType;
    /// Whether its call reaches the front end through the methods every
    /// front end has (`Front`'s and `OpFront`'s with no default, and the
    /// ones whose default calls only those), `Front::finish` and
    /// `Front::invoke_ce_reactions` alone: a getter, a setter or a method
    /// of an object (not static), its arguments and result `PLAIN`, its
    /// function given no front end. An engine may call such a member
    /// through a front end that has nothing else (as it expands each
    /// member's own code).
    const PLAIN: bool = false;

    /// One call, through `front` (which opens its operations).
    fn call<F: Front>(front: &mut F);
}

/// What visits every member's typed path (a front end making its calls),
/// by the member's kind: `visit_typed` calls the one method of each
/// member's, picked as the table expands, so a visitor's generic code is
/// made for the members it takes alone, not for every member to be
/// dropped as dead after (Blink's generated bindings have each member's own
/// callbacks only). Each method's default is `any`'s, which does nothing:
/// a visitor of every member overrides `any`. `M` is a typed path of
/// `INTERFACES[M::INTERFACE]`'s.
pub trait TypedVisitor {
    /// Any member's, unless its kind's method is overridden.
    fn any<M: TypedMember>(&mut self) {}
    /// A regular attribute's getter.
    fn getter<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// A regular attribute's setter.
    fn setter<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// A regular operation.
    fn method<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// A static attribute's getter or a static operation (WebIDL's
    /// `static`, a namespace's operation).
    fn static_member<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// An attribute's getter or setter or an operation that is a
    /// cross-origin property's (`#[cross_origin]`, WebIDL's
    /// `[CrossOrigin]`).
    fn cross_origin<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// A constructor.
    fn constructor<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// A legacy factory function (`#[legacy_factory]`).
    fn legacy_factory<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// The indexed getter (`#[indexed]`, `#[indexed(getter)]`).
    fn indexed<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// The indexed setter (`#[indexed(setter)]`).
    fn indexed_setter<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// A named properties' special operation (`#[named(..)]`).
    fn named<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// A pair iterator's pair at an index (`#[pairs]`).
    fn pairs<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
    /// An observable array's operation (`#[observable(..)]`).
    fn observable<M: TypedMember>(&mut self) {
        self.any::<M>();
    }
}

/// Every interface's typed paths, in `INTERFACES` order.
pub fn visit_typed<V: TypedVisitor>(visitor: &mut V) {
    crate::interfaces::visit_typed(visitor);
}

/// Whether a node of `kind` is one of `INTERFACES[interface]`'s: its own
/// kinds, or those of an interface it is an ancestor of (Blink's
/// `WrapperTypeInfo` along `parent_class`), from the table worked out at
/// compile time.
#[inline]
pub fn accepts(interface: usize, kind: NodeKind) -> bool {
    crate::declare::ACCEPTED[interface] & crate::declare::kind_bit(kind) != 0
}

/// Whether an object whose wrapper is of the interface `of` (`interface_of`)
/// is one of `INTERFACES[interface]`'s: `of` is it, or a descendant of it.
/// What a front end that keeps each object's interface checks a receiver
/// by, with no DOM (a node's too: its name is in `of`).
#[inline]
pub fn is_interface(interface: usize, of: usize) -> bool {
    crate::declare::DESCENDANTS[interface][of / 128] >> (of % 128) & 1 != 0
}

/// The interface a node of `kind`'s wrapper is of: its kind's most derived
/// one (an element's of the table's `Element`: `node_interface_of` reads
/// its name).
#[inline]
pub fn node_interface(kind: NodeKind) -> usize {
    crate::declare::NODE_INTERFACES[kind as usize]
}

/// The interface `node`'s wrapper is of: an HTML or SVG element's by its
/// local name (`html_element_interface`, `svg_element_interface`), another
/// node's by its kind.
#[inline]
pub fn node_interface_of<'gc>(cx: &dom::Cx<'_, 'gc>, node: NodeRef<'gc>) -> usize {
    match cx.element_name(node) {
        Some(name) if name.namespace == dom::names::HTML_NAMESPACE => {
            let interface = html_element_interface(name.local);
            // A failed custom element (its name is a valid one, never a
            // fixed one's with an interface of its own).
            if interface == crate::declare::HTML_ELEMENT
                && cx.custom_element_state(node) == dom::CustomElementState::Failed
            {
                return crate::declare::HTML_UNKNOWN_ELEMENT;
            }
            interface
        }
        Some(name) if name.namespace == dom::names::SVG_NAMESPACE => {
            svg_element_interface(name.local)
        }
        // A fragment that is a shadow root, a text that is a CDATA section,
        // an `XMLDocument` (no node kinds of their own).
        None if cx.is_shadow_root(node) => SHADOW_ROOT,
        None if cx.is_cdata_section(node) => {
            const { crate::declare::interface_index("CDATASection") }
        }
        None if cx.is_xml_document_interface(node) => {
            const { crate::declare::interface_index("XMLDocument") }
        }
        _ => node_interface(cx.kind(node)),
    }
}

/// `ShadowRoot`'s index: the interface of the fragments that are shadow
/// roots, which no node kind's is (`node_interface_of`, `accepts_node`).
const SHADOW_ROOT: usize = crate::declare::interface_index("ShadowRoot");

/// An HTML element's interface by its local name: a table read by the
/// name's number (Blink's element factory by `html_names`); a name past
/// the fixed ones `HTMLElement` if it is a valid custom element name, else
/// `HTMLUnknownElement` (out of line: few elements have one).
#[inline]
pub fn html_element_interface(local: dom::NameId) -> usize {
    match usize::try_from(local)
        .ok()
        .and_then(|local| crate::declare::HTML_TAGS.get(local))
    {
        Some(&interface) => usize::from(interface),
        None => unfixed_html_element_interface(local),
    }
}

/// An SVG element's interface by its local name (`declare::SVG_TAGS`): a
/// name past the fixed ones `SVGElement`.
#[inline]
pub fn svg_element_interface(local: dom::NameId) -> usize {
    usize::try_from(local)
        .ok()
        .and_then(|local| crate::declare::SVG_TAGS.get(local))
        .map_or(crate::declare::SVG_ELEMENT, |&interface| {
            usize::from(interface)
        })
}

#[cold]
#[inline(never)]
fn unfixed_html_element_interface(local: dom::NameId) -> usize {
    if dom::is_valid_custom_element_name(&dom::names::get(local)) {
        crate::declare::HTML_ELEMENT
    } else {
        crate::declare::HTML_UNKNOWN_ELEMENT
    }
}

/// Whether `node` is one of `INTERFACES[interface]`'s: its kind (`accepts`),
/// and its name for an element interface's (whose interface it is must
/// be `interface` or a descendant).
#[inline]
pub fn accepts_node<'gc>(cx: &dom::Cx<'_, 'gc>, interface: usize, node: NodeRef<'gc>) -> bool {
    if interface == SHADOW_ROOT {
        return cx.is_shadow_root(node);
    }
    if !accepts(interface, cx.kind(node)) {
        return false;
    }
    if !crate::declare::BY_NAME[interface] {
        return true;
    }
    let of = node_interface_of(cx, node);
    crate::declare::DESCENDANTS[interface][of / 128] >> (of % 128) & 1 != 0
}

#[cfg(test)]
mod tests {
    use dom::{Dom, DomString, NodeRef, NodeRoot, QualName};

    use super::{Front, FrontTypes, OpFront, node_param, take_node};
    use crate::interfaces::{CharacterData, Element};
    use crate::{ApiCx, ApiError};

    /// A front end whose only argument is one node, or null; its own side
    /// in an operation, which the test opens itself.
    struct OneNode {
        node: Option<NodeRoot>,
        thrown: Option<ApiError>,
    }

    impl FrontTypes for OneNode {
        type Str = String;
        type Node = ();
        type Dict = ();
        type PendingString = std::convert::Infallible;
        type PendingNode = std::convert::Infallible;
        type PendingNumber = std::convert::Infallible;
        type PendingObject = std::convert::Infallible;
        type PreparedNode = ();
        type Callback = usize;
        type Any = std::convert::Infallible;
        type Rest = std::convert::Infallible;
    }

    impl Front for OneNode {
        type Op<'o> = OneNode;
        fn reading<V: crate::typed::ReadDict<Self>>(&mut self, reads: V) -> V::Out {
            reads.read(self)
        }

        fn read<'o, R>(
            &'o mut self,
            _: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
        ) -> R {
            unreachable!()
        }
        fn mutate<'o, R>(
            &'o mut self,
            _: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
        ) -> R {
            unreachable!()
        }
        fn has_args(&mut self, _: &str, _: usize) -> bool {
            true
        }
        fn string_arg(&mut self, _: usize) -> Option<String> {
            None
        }
        fn nullable_string_arg(&mut self, _: usize) -> Option<Option<String>> {
            None
        }
        fn legacy_null_string_arg(&mut self, _: usize) -> Option<Option<String>> {
            None
        }
        fn node_arg(&mut self, _: usize) -> Option<Option<()>> {
            Some(self.node.as_ref().map(|_| ()))
        }
        fn throw(&mut self, error: ApiError) {
            self.thrown = Some(error);
        }
    }

    impl OpFront for OneNode {
        crate::unbound_op_methods!();

        fn str_param<'a>(&mut self, string: &'a String) -> &'a str {
            string
        }
        fn this_as<'gc, T: elise::HeapType>(
            &mut self,
            _: &ApiCx<'_, 'gc>,
        ) -> Option<elise::Gc<'gc, T>> {
            unreachable!()
        }
        fn node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, (): &()) -> NodeRef<'gc> {
            self.node.as_ref().unwrap().get(cx.gc)
        }
        fn ret_undefined(&mut self) {}
        fn ret_null(&mut self) {}
        fn ret_bool(&mut self, _: bool) {}
        fn ret_u32(&mut self, _: u32) -> Option<std::convert::Infallible> {
            None
        }
        fn ret_f64(&mut self, _: f64) -> Option<std::convert::Infallible> {
            None
        }
        fn ret_string(&mut self, _: &DomString) -> Option<std::convert::Infallible> {
            None
        }
        fn ret_node<'gc>(
            &mut self,
            _: &ApiCx<'_, 'gc>,
            _: NodeRef<'gc>,
        ) -> Option<std::convert::Infallible> {
            None
        }
    }

    /// An interface's argument takes its own objects only: a text for an
    /// element is a `TypeError` naming the parameter and the interface.
    #[test]
    fn a_node_argument_of_another_interface_is_a_type_error() {
        let mut dom = Dom::new();
        let (element, text) = dom.mutate(|cx| {
            let doc = cx.create_document(true);
            let element = cx.create_element(doc, QualName::html("p"));
            (cx.root(element), cx.root(cx.create_text(doc, "t")))
        });
        let mut front = OneNode {
            node: Some(text),
            thrown: None,
        };
        dom.mutate(|cx| {
            let cx = &ApiCx::new(cx);
            let error = node_param::<Element, _>(&mut front, cx, &(), 1).unwrap_err();
            assert_eq!(
                error,
                ApiError::type_error("parameter 2 is not of type 'Element'")
            );
            assert!(node_param::<CharacterData, _>(&mut front, cx, &(), 0).is_ok());
        });
        front.node = Some(element);
        dom.mutate(|cx| {
            let cx = &ApiCx::new(cx);
            assert!(node_param::<Element, _>(&mut front, cx, &(), 0).is_ok());
            assert!(node_param::<CharacterData, _>(&mut front, cx, &(), 0).is_err());
        });
        // Null for a node that can't be null: thrown as it is taken.
        front.node = None;
        assert!(take_node::<Element, _>(&mut front, 0).is_none());
        assert_eq!(
            front.thrown,
            Some(ApiError::type_error("parameter 1 is not of type 'Element'"))
        );
    }
}
