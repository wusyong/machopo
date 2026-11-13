//! A member's call through V8's callback info alone (Blink's generated
//! callbacks take `v8::FunctionCallbackInfo` and keep the rest in locals):
//! the front end of the members whose arguments are strings and nodes and
//! whose results are numbers, booleans, strings, nodes, null or nothing
//! (`dom_api::TypedMember::PLAIN`), which most calls are.
//!
//! It holds the info alone: no scope of rusty_v8's, no `front::Call`. A
//! result the operation can't give (V8 would allocate) is the member's
//! own, carried out of the operation by its return type
//! (`dom_api::RetType::Pending`: a `PendingString`, a node's root, a
//! number) and handed back to `finish_string`, `finish_node` or
//! `finish_number`; a member that gives none carries nothing. The isolate, the receiver, the arguments and the result
//! are read from the info where they are used. What allocates in V8,
//! throws or runs script (an argument to convert or copy, a string V8 has
//! no copy of, a node with no wrapper, an error, an element's reactions)
//! makes a scope and the whole front end (`front::V8Front`) then, out of
//! line, and is done by it.

use bindings::InfoState;
use dom::{
    DomString, EventTargetImpl, NameId, Node, NodeRef, NodeRoot, ObjectRef, ObjectRoot, TargetRef,
    Window, names,
};
use dom_api::{AnyArg, ApiCx, ApiError, Callback, Front, FrontTypes, OpFront};
use elise::JsEdge;
use elise::Persistent;

use crate::front::{self, Checked, V8Front};
use crate::state::{self, Caches, State};
use crate::strings::ArgString;
use crate::templates;

/// A string the operation couldn't give V8 (it would allocate one): what
/// `InfoFront::finish_string` makes after.
pub(crate) enum PendingString {
    /// A string V8 has no copy of.
    String(DomString),
    /// A name's string, not made yet.
    Name(NameId, DomString),
    /// Text computed for the call, or a constant: no string of the DOM's.
    Fresh(std::borrow::Cow<'static, str>),
}

/// A node the operation couldn't give V8: what `InfoFront::finish_node`
/// makes after.
pub(crate) enum PendingNode {
    /// A node with no wrapper, rooted, and its interface: a wrapper made,
    /// in an operation of its own.
    Node(NodeRoot, usize),
    /// A custom element `createElement` is to construct with its
    /// definition's constructor, in the document.
    Construct(Persistent<dom::CustomElementDefinition>, NodeRoot),
}

/// What an info callback is, known as a member's callback is made (the
/// front end is of one): a method, whose receiver V8 checked against its
/// signature; a static operation, which has none; a constructor, whose
/// `this` is the new object V8 made for the call; an indexed getter's
/// interceptor, whose one argument is the index.
pub(crate) trait InfoKind<'s>: Sized + 'static {
    /// V8's callback info (`bindings::StateInfo`, `StateIndexInfo`).
    type Info: InfoState<'s, State>;
    /// The receiver, the new object, or nothing.
    type This: Copy;
    /// What the whole front end reads the call from, and what of the
    /// info it is made of (itself, or with a scope: an index's number).
    type Args: front::CallArgs<'s>;
    type Seed;
    /// What the kind's call notes as its result is given (an indexed
    /// getter: whether there was no item).
    type Noted: Default;

    fn this(info: &Self::Info) -> Self::This;
    fn seed(info: &Self::Info) -> Self::Seed;
    fn args(seed: Self::Seed, scope: &mut v8::PinScope<'s, '_>) -> Self::Args;
    /// How many arguments the call has, and one of them.
    fn length(info: &Self::Info) -> usize;
    fn arg(info: &Self::Info, index: usize) -> v8::Local<'s, v8::Value>;
    /// Argument `index` as a number the info holds as one (an indexed
    /// getter's index), which no handle is made of.
    #[inline(always)]
    fn number_arg(_: &Self::Info, _: usize) -> Option<u32> {
        None
    }
    /// The receiver made the DOM's, if it is a `T`.
    fn this_as<'gc, T: elise::HeapType>(
        no_gc: &mut bindings::InfoNoGc<'_, 's>,
        cx: &ApiCx<'_, 'gc>,
        this: Self::This,
    ) -> Option<elise::Gc<'gc, T>>;
    /// The call's `this` object, whatever V8 checked of it.
    fn this_object(info: &Self::Info) -> v8::Local<'s, v8::Object>;
    /// A constructor's new object, which the object it makes is wrapped
    /// by.
    fn new_object(this: Self::This) -> v8::Local<'s, v8::Object>;
    /// A constructor's `new.target`.
    fn new_target(this: Self::This) -> v8::Local<'s, v8::Value>;
    /// The result is null: an indexed getter's no item, a named getter's
    /// no property.
    #[inline(always)]
    fn note_null(_: &mut Self::Noted) {}
    /// The result is an empty string: an indexed getter's no item (a
    /// named getter's is a value).
    #[inline(always)]
    fn note_empty(_: &mut Self::Noted) {}
    /// The result is a boolean; whether it is the callback's to return
    /// (a named setter's says whether it took the name: noted, not
    /// returned).
    #[inline(always)]
    fn note_bool(_: &mut Self::Noted, _: bool) -> bool {
        true
    }
}

/// A method's (an attribute's accessor's).
pub(crate) struct Method;
/// A static operation's.
pub(crate) struct Static;
/// A constructor's.
pub(crate) struct Construct;
/// An indexed getter's interceptor's.
pub(crate) struct IndexedGet;

/// A function's argument `index`, undefined past the last.
#[inline(always)]
fn function_arg<'s, K>(
    info: &bindings::StateInfo<'s, State, K>,
    index: usize,
) -> v8::Local<'s, v8::Value> {
    front::arg(&info.args(), index)
}

#[inline(always)]
fn function_length<K>(info: &bindings::StateInfo<'_, State, K>) -> usize {
    usize::try_from(info.args().length()).unwrap_or(0)
}

impl<'s> InfoKind<'s> for Method {
    type Info = bindings::StateInfo<'s, State>;
    type This = bindings::Receiver<'s>;
    type Args = Checked<'s>;
    type Seed = Checked<'s>;
    type Noted = ();

    #[inline(always)]
    fn this(info: &Self::Info) -> Self::This {
        info.receiver()
    }

    #[inline(always)]
    fn seed(info: &Self::Info) -> Checked<'s> {
        Checked {
            args: info.args(),
            this: info.receiver(),
        }
    }

    #[inline(always)]
    fn args(seed: Checked<'s>, _: &mut v8::PinScope<'s, '_>) -> Checked<'s> {
        seed
    }

    #[inline(always)]
    fn length(info: &Self::Info) -> usize {
        function_length(info)
    }

    #[inline(always)]
    fn arg(info: &Self::Info, index: usize) -> v8::Local<'s, v8::Value> {
        function_arg(info, index)
    }

    /// The receiver V8 checked against the function's signature: its map
    /// isn't read again, only its tag (`bindings::unwrap_receiver`).
    #[inline(always)]
    fn this_as<'gc, T: elise::HeapType>(
        no_gc: &mut bindings::InfoNoGc<'_, 's>,
        cx: &ApiCx<'_, 'gc>,
        this: Self::This,
    ) -> Option<elise::Gc<'gc, T>> {
        no_gc.unwrap_receiver::<T>(cx.gc, this)
    }

    #[inline(always)]
    fn this_object(info: &Self::Info) -> v8::Local<'s, v8::Object> {
        info.args().this()
    }

    fn new_object(_: Self::This) -> v8::Local<'s, v8::Object> {
        unreachable!("a method constructs nothing")
    }

    fn new_target(_: Self::This) -> v8::Local<'s, v8::Value> {
        unreachable!("a method has no `new.target`")
    }
}

impl<'s> InfoKind<'s> for Static {
    type Info = bindings::StateInfo<'s, State, bindings::Unsigned>;
    type This = ();
    type Args = v8::FunctionCallbackArguments<'s>;
    type Seed = v8::FunctionCallbackArguments<'s>;
    type Noted = ();

    #[inline(always)]
    fn this(_: &Self::Info) {}

    #[inline(always)]
    fn seed(info: &Self::Info) -> Self::Seed {
        info.args()
    }

    #[inline(always)]
    fn args(seed: Self::Seed, _: &mut v8::PinScope<'s, '_>) -> Self::Args {
        seed
    }

    #[inline(always)]
    fn length(info: &Self::Info) -> usize {
        function_length(info)
    }

    #[inline(always)]
    fn arg(info: &Self::Info, index: usize) -> v8::Local<'s, v8::Value> {
        function_arg(info, index)
    }

    fn this_as<'gc, T: elise::HeapType>(
        _: &mut bindings::InfoNoGc<'_, 's>,
        _: &ApiCx<'_, 'gc>,
        (): (),
    ) -> Option<elise::Gc<'gc, T>> {
        unreachable!("a static operation has no receiver")
    }

    #[inline(always)]
    fn this_object(info: &Self::Info) -> v8::Local<'s, v8::Object> {
        info.args().this()
    }

    fn new_object((): ()) -> v8::Local<'s, v8::Object> {
        unreachable!("a static operation constructs nothing")
    }

    fn new_target((): ()) -> v8::Local<'s, v8::Value> {
        unreachable!("a static operation has no `new.target`")
    }
}

impl<'s> InfoKind<'s> for Construct {
    type Info = bindings::StateInfo<'s, State, bindings::Unsigned>;
    /// The call's `this`: called with `new` (`InfoFront::construct`
    /// checked), the object V8 made of the interface's instance template
    /// (with `new.target`'s prototype, a subclass's), wrapping nothing;
    /// and `new.target`.
    type This = (v8::Local<'s, v8::Object>, v8::Local<'s, v8::Value>);
    type Args = v8::FunctionCallbackArguments<'s>;
    type Seed = v8::FunctionCallbackArguments<'s>;
    type Noted = ();

    #[inline(always)]
    fn this(info: &Self::Info) -> Self::This {
        let args = info.args();
        (args.this(), args.new_target())
    }

    #[inline(always)]
    fn seed(info: &Self::Info) -> Self::Seed {
        info.args()
    }

    #[inline(always)]
    fn args(seed: Self::Seed, _: &mut v8::PinScope<'s, '_>) -> Self::Args {
        seed
    }

    #[inline(always)]
    fn length(info: &Self::Info) -> usize {
        function_length(info)
    }

    #[inline(always)]
    fn arg(info: &Self::Info, index: usize) -> v8::Local<'s, v8::Value> {
        function_arg(info, index)
    }

    fn this_as<'gc, T: elise::HeapType>(
        _: &mut bindings::InfoNoGc<'_, 's>,
        _: &ApiCx<'_, 'gc>,
        _: Self::This,
    ) -> Option<elise::Gc<'gc, T>> {
        unreachable!("a constructor has no receiver")
    }

    #[inline(always)]
    fn this_object(info: &Self::Info) -> v8::Local<'s, v8::Object> {
        info.args().this()
    }

    #[inline(always)]
    fn new_object((this, _): Self::This) -> v8::Local<'s, v8::Object> {
        this
    }

    #[inline(always)]
    fn new_target((_, new_target): Self::This) -> v8::Local<'s, v8::Value> {
        new_target
    }
}

/// Whether an indexed getter's result was no item (null, or an empty
/// string: `InfoKind::note_no_item`).
#[derive(Default)]
pub(crate) struct NoItem(pub(crate) bool);

impl<'s> InfoKind<'s> for IndexedGet {
    type Info = bindings::StateIndexInfo<'s, State>;
    /// The interceptor's holder: an object of the template's.
    type This = bindings::Receiver<'s>;
    type Args = front::IndexArgs<'s>;
    /// The holder and the index.
    type Seed = (bindings::Receiver<'s>, u32);
    type Noted = NoItem;

    #[inline(always)]
    fn this(info: &Self::Info) -> Self::This {
        info.holder()
    }

    #[inline(always)]
    fn seed(info: &Self::Info) -> Self::Seed {
        (info.holder(), info.index())
    }

    /// The index a number of V8's, made now.
    fn args((this, index): Self::Seed, scope: &mut v8::PinScope<'s, '_>) -> Self::Args {
        front::IndexArgs {
            index: v8::Integer::new_from_unsigned(scope, index).into(),
            this: this.object(),
            receiver: Some(this),
            undefined: v8::undefined(scope).into(),
        }
    }

    #[inline(always)]
    fn length(_: &Self::Info) -> usize {
        1
    }

    fn arg(_: &Self::Info, _: usize) -> v8::Local<'s, v8::Value> {
        unreachable!("an indexed getter's argument is its index, read as a number")
    }

    #[inline(always)]
    fn number_arg(info: &Self::Info, index: usize) -> Option<u32> {
        (index == 0).then(|| info.index())
    }

    #[inline(always)]
    fn this_as<'gc, T: elise::HeapType>(
        no_gc: &mut bindings::InfoNoGc<'_, 's>,
        cx: &ApiCx<'_, 'gc>,
        this: Self::This,
    ) -> Option<elise::Gc<'gc, T>> {
        no_gc.unwrap_receiver::<T>(cx.gc, this)
    }

    #[inline(always)]
    fn this_object(info: &Self::Info) -> v8::Local<'s, v8::Object> {
        info.holder().object()
    }

    fn new_object(_: Self::This) -> v8::Local<'s, v8::Object> {
        unreachable!("an indexed getter constructs nothing")
    }

    fn new_target(_: Self::This) -> v8::Local<'s, v8::Value> {
        unreachable!("an indexed getter has no `new.target`")
    }

    #[inline(always)]
    fn note_null(noted: &mut NoItem) {
        noted.0 = true;
    }

    #[inline(always)]
    fn note_empty(noted: &mut NoItem) {
        noted.0 = true;
    }
}

/// A named property's getter's interceptor's (`object.name`).
pub(crate) struct NamedGet;
/// A named property's setter's (`object.name = value`).
pub(crate) struct NamedSet;

/// Whether a named setter gave `false`: it didn't take the name, an
/// ordinary property's.
#[derive(Default)]
pub(crate) struct Declined(pub(crate) bool);

/// What the named kinds share: the info, the holder, the arguments.
macro_rules! named_kind {
    () => {
        type Info = bindings::StateNameInfo<'s, State>;
        /// The interceptor's holder: an object of the template's.
        type This = bindings::Receiver<'s>;
        type Args = front::NameArgs<'s>;
        /// The holder, the name and a setter's value.
        type Seed = (
            bindings::Receiver<'s>,
            v8::Local<'s, v8::Value>,
            Option<v8::Local<'s, v8::Value>>,
        );

        #[inline(always)]
        fn this(info: &Self::Info) -> Self::This {
            info.holder()
        }

        #[inline(always)]
        fn seed(info: &Self::Info) -> Self::Seed {
            (info.holder(), info.name().into(), info.value())
        }

        fn args((this, name, value): Self::Seed, scope: &mut v8::PinScope<'s, '_>) -> Self::Args {
            front::NameArgs {
                name,
                value,
                this,
                undefined: v8::undefined(scope).into(),
            }
        }

        #[inline(always)]
        fn length(info: &Self::Info) -> usize {
            if info.value().is_some() { 2 } else { 1 }
        }

        /// The name, then a setter's value (each call reads no other).
        #[inline(always)]
        fn arg(info: &Self::Info, index: usize) -> v8::Local<'s, v8::Value> {
            match (index, info.value()) {
                (0, _) => info.name().into(),
                (_, Some(value)) => value,
                (_, None) => unreachable!("a named getter's argument is the name"),
            }
        }

        #[inline(always)]
        fn this_as<'gc, T: elise::HeapType>(
            no_gc: &mut bindings::InfoNoGc<'_, 's>,
            cx: &ApiCx<'_, 'gc>,
            this: Self::This,
        ) -> Option<elise::Gc<'gc, T>> {
            no_gc.unwrap_receiver::<T>(cx.gc, this)
        }

        #[inline(always)]
        fn this_object(info: &Self::Info) -> v8::Local<'s, v8::Object> {
            info.holder().object()
        }

        fn new_object(_: Self::This) -> v8::Local<'s, v8::Object> {
            unreachable!("a named property's operation constructs nothing")
        }

        fn new_target(_: Self::This) -> v8::Local<'s, v8::Value> {
            unreachable!("a named property's operation has no `new.target`")
        }
    };
}

impl<'s> InfoKind<'s> for NamedGet {
    named_kind!();
    type Noted = NoItem;

    /// No property of the name: V8 looks on.
    #[inline(always)]
    fn note_null(noted: &mut NoItem) {
        noted.0 = true;
    }
}

impl<'s> InfoKind<'s> for NamedSet {
    named_kind!();
    type Noted = Declined;

    #[inline(always)]
    fn note_bool(noted: &mut Declined, took: bool) -> bool {
        noted.0 = !took;
        false
    }
}

/// A plain member's call out of its operation.
pub(crate) struct InfoFront<'s, K: InfoKind<'s> = Method> {
    info: K::Info,
    /// Whether the member asks for the current realm's window
    /// (`Front::prepare_window`, a static operation's): told before the
    /// operation, found in it (`InfoOp::current_window`). Known as the
    /// member's call is expanded.
    window: bool,
    /// What the call notes as its result is given.
    noted: K::Noted,
}

/// A plain member's call in its operation: what of V8 it may use and the
/// result (`InfoNoGc`), the receiver, and the state's caches it reads.
pub(crate) struct InfoOp<'o, 's, K: InfoKind<'s> = Method> {
    no_gc: bindings::InfoNoGc<'o, 's>,
    this: K::This,
    window: bool,
    noted: &'o mut K::Noted,
    caches: &'o Caches,
}

impl<'s, K: InfoKind<'s>> FrontTypes for InfoFront<'s, K> {
    type Str = ArgString<'s>;
    type Node = v8::Local<'s, v8::Object>;
    type Dict = v8::Local<'s, v8::Object>;
    type PendingString = PendingString;
    type PendingNode = PendingNode;
    /// A number that is no Smi.
    type PendingNumber = f64;
    /// An object with no wrapper, rooted, and its interface.
    type PendingObject = (ObjectRoot, usize);
    /// A new node's wrapper made ahead of the operation, and its
    /// interface; none for a call on another realm's object.
    type PreparedNode = Option<(v8::Local<'s, v8::Object>, usize)>;
    /// The argument itself: an object, or a timer's string.
    type Callback = v8::Local<'s, v8::Value>;
    /// The argument itself (or a dictionary's member's value).
    type Any = v8::Local<'s, v8::Value>;
    /// The arguments in a new array, made by the whole front end.
    type Rest = v8::Local<'s, v8::Array>;
}

impl<'s, K: InfoKind<'s>> FrontTypes for InfoOp<'_, 's, K> {
    type Str = ArgString<'s>;
    type Node = v8::Local<'s, v8::Object>;
    type Dict = v8::Local<'s, v8::Object>;
    type PendingString = PendingString;
    type PendingNode = PendingNode;
    /// A number that is no Smi.
    type PendingNumber = f64;
    /// An object with no wrapper, rooted, and its interface.
    type PendingObject = (ObjectRoot, usize);
    /// A new node's wrapper made ahead of the operation, and its
    /// interface; none for a call on another realm's object.
    type PreparedNode = Option<(v8::Local<'s, v8::Object>, usize)>;
    /// The argument itself: an object, or a timer's string.
    type Callback = v8::Local<'s, v8::Value>;
    /// The argument itself (or a dictionary's member's value).
    type Any = v8::Local<'s, v8::Value>;
    /// The arguments in a new array, made by the whole front end.
    type Rest = v8::Local<'s, v8::Array>;
}

impl<'s, K: InfoKind<'s>> InfoFront<'s, K> {
    #[inline(always)]
    pub(crate) fn new(info: K::Info) -> Self {
        // As `V8Front::new`'s: every way into script counts itself.
        debug_assert!(
            bindings::trace::in_script(),
            "a DOM member is called from script that the engine didn't count as entered"
        );
        Self {
            info,
            window: false,
            noted: K::Noted::default(),
        }
    }

    /// What the call noted as its result was given.
    #[inline(always)]
    pub(crate) fn noted(&self) -> &K::Noted {
        &self.noted
    }

    /// `f` of the whole front end, a scope made for it: what allocates,
    /// throws or runs script.
    #[cold]
    #[inline(never)]
    fn with_front<R>(&mut self, f: impl FnOnce(&mut V8Front<'_, 's, 's, 's, K::Args>) -> R) -> R {
        let seed = K::seed(&self.info);
        self.info.with_scope(|mut scope, rv| {
            let args = K::args(seed, scope.scope());
            f(&mut V8Front::new(scope, &args, rv))
        })
    }

    /// `text` as a new V8 string, the call's result.
    #[inline]
    fn finish_new_string(&mut self, text: &str) {
        let mut handles = self.info.handles();
        match handles.new_string(text) {
            Some(string) => handles.return_value(string.into()),
            None => self.throw_too_long(),
        }
    }

    /// A string longer than V8's may be: a `RangeError`, the whole front
    /// end's to throw.
    #[cold]
    #[inline(never)]
    fn throw_too_long(&mut self) {
        self.info.with_scope(|mut scope, _| {
            crate::exception::throw_range_error(scope.scope(), "Invalid string length");
        });
    }

    /// A string argument that doesn't lend its text in place (not a
    /// string yet, not ASCII, not flat): converted or copied, as the whole
    /// front end does.
    #[cold]
    #[inline(never)]
    fn converted_string_arg(&mut self, index: usize) -> Option<ArgString<'s>> {
        self.with_front(|front| front.string_arg(index))
    }

    /// Tells V8 of the DOM's growth after a call that may allocate, and
    /// queues the microtasks its operation asked for (`state::report`): a
    /// scope only if there is either.
    #[inline(always)]
    pub(crate) fn report(&mut self) {
        let (microtasks, used) = {
            let (state, _) = self.info.state_mut();
            (state.dom.take_microtasks(), state::moved(state))
        };
        if !microtasks.is_empty() || used.is_some() {
            self.report_these(microtasks, used);
        }
    }

    #[cold]
    #[inline(never)]
    fn report_these(&mut self, microtasks: dom::Microtasks, used: Option<usize>) {
        self.info
            .with_scope(|mut scope, _| state::report_these(&mut scope, microtasks, used));
    }
}

impl<'s, K: InfoKind<'s>> Front for InfoFront<'s, K> {
    /// A dictionary's members are read by the whole front end (a getter
    /// may run script), all in the one scope made for them.
    fn reading<V: dom_api::typed::ReadDict<Self>>(&mut self, reads: V) -> V::Out {
        self.with_front(|front| reads.read(front))
    }

    type Op<'o>
        = InfoOp<'o, 's, K>
    where
        Self: 'o;

    #[inline(always)]
    fn read<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let this = K::this(&self.info);
        let window = self.window;
        let (state, no_gc) = self.info.state();
        let mut op = InfoOp {
            no_gc,
            this,
            window,
            noted: &mut self.noted,
            caches: &state.caches,
        };
        state
            .dom
            .read(|cx| f(&ApiCx::with_layout_cell(cx, &state.layout), &mut op))
    }

    #[inline(always)]
    fn mutate<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let this = K::this(&self.info);
        let window = self.window;
        let (state, no_gc) = self.info.state_mut();
        let State {
            dom,
            caches,
            layout,
            ..
        } = state;
        let mut op = InfoOp {
            no_gc,
            this,
            window,
            noted: &mut self.noted,
            caches,
        };
        dom.mutate(|cx| f(&ApiCx::with_layout_cell(cx, layout), &mut op))
    }

    #[inline(always)]
    fn has_args(&mut self, name: &str, required: usize) -> bool {
        K::length(&self.info) >= required || self.with_front(|front| front.has_args(name, required))
    }

    /// A string that lends its text in place (most: an ASCII literal) is
    /// taken with the isolate alone.
    #[inline(always)]
    fn string_arg(&mut self, index: usize) -> Option<ArgString<'s>> {
        let value = K::arg(&self.info, index);
        if let Some(string) = v8::machopo::as_string(value) {
            let string = bindings::LentString::new(string);
            if string.lends(self.info.isolate()) {
                return Some(ArgString::lent(string));
            }
        }
        self.converted_string_arg(index)
    }

    #[inline(always)]
    fn nullable_string_arg(&mut self, index: usize) -> Option<Option<ArgString<'s>>> {
        if K::arg(&self.info, index).is_null_or_undefined() {
            return Some(None);
        }
        self.string_arg(index).map(Some)
    }

    #[inline(always)]
    fn legacy_null_string_arg(&mut self, index: usize) -> Option<Option<ArgString<'s>>> {
        if K::arg(&self.info, index).is_null() {
            return Some(None);
        }
        self.string_arg(index).map(Some)
    }

    #[inline(always)]
    fn node_arg(&mut self, index: usize) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = K::arg(&self.info, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if bindings::is_wrapper::<Node>(self.info.isolate(), value) {
            return v8::Local::<v8::Object>::try_from(value).ok().map(Some);
        }
        // Not a node: the whole front end's `TypeError`.
        self.with_front(|front| front.node_arg(index))
    }

    /// A node as it is; anything else a string, as `string_arg` takes
    /// one.
    #[inline(always)]
    fn node_or_string_arg(
        &mut self,
        index: usize,
    ) -> Option<dom_api::typed::NodeOrStr<v8::Local<'s, v8::Object>, ArgString<'s>>> {
        let value = K::arg(&self.info, index);
        if bindings::is_wrapper::<Node>(self.info.isolate(), value) {
            return v8::Local::<v8::Object>::try_from(value)
                .ok()
                .map(dom_api::typed::NodeOrStr::Node);
        }
        self.string_arg(index).map(dom_api::typed::NodeOrStr::Str)
    }

    /// A node as it is, a small integer as it is; anything else
    /// converted by the whole front end.
    #[inline(always)]
    fn node_or_long_arg(
        &mut self,
        index: usize,
    ) -> Option<Option<dom_api::typed::NodeOrLong<v8::Local<'s, v8::Object>>>> {
        let value = K::arg(&self.info, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if bindings::is_wrapper::<Node>(self.info.isolate(), value) {
            return v8::Local::<v8::Object>::try_from(value)
                .ok()
                .map(|node| Some(dom_api::typed::NodeOrLong::Node(node)));
        }
        match v8::machopo::as_smi(value) {
            Some(long) => Some(Some(dom_api::typed::NodeOrLong::Long(long))),
            None => self.with_front(|front| front.node_or_long_arg(index)),
        }
    }

    /// A string V8 had no copy of: made now, by V8's string constructor
    /// alone, with no scope (Blink's `V8String`). Text computed for the
    /// call, and a string of the DOM's only the call holds, are given and
    /// not cached (their address goes with the call); a string the DOM
    /// holds is cached by its address, a name by its number.
    #[inline(never)]
    fn finish_string(&mut self, pending: PendingString) {
        match pending {
            PendingString::Fresh(text) => self.finish_new_string(&text),
            PendingString::String(string) if string.is_unique() => {
                self.finish_new_string(&string.to_str());
            }
            PendingString::String(string) => {
                let dead = self.info.state().0.caches.strings.dead();
                let mut handles = self.info.handles();
                let Some(local) = handles.new_string(&string.to_str()) else {
                    return self.throw_too_long();
                };
                let weak =
                    crate::strings::StringCache::weak_of(handles.isolate(), local, dead, &string);
                handles.return_value(local.into());
                self.info.state_mut().0.caches.strings.insert(&string, weak);
            }
            PendingString::Name(id, name) => {
                let mut handles = self.info.handles();
                // Internalized: a name is a property's key too.
                let Some(local) = handles.new_internalized_string(&name.to_str()) else {
                    return self.throw_too_long();
                };
                let global = v8::Global::new(handles.isolate(), local);
                handles.return_value(local.into());
                front::keep_name(self.info.state_mut().0, id, global);
            }
        }
    }

    /// A node with no wrapper: one made now, in an operation of its own;
    /// a custom element constructed.
    #[cold]
    #[inline(never)]
    fn finish_node(&mut self, pending: PendingNode) {
        self.with_front(|front| {
            front.finish_kept_as(match pending {
                PendingNode::Node(node, interface) => front::Kept::Node(node, interface),
                PendingNode::Construct(definition, document) => {
                    front::Kept::Construct(definition, document)
                }
            });
        });
    }

    /// A wrapper of `INTERFACES[interface]`, made now for the node the
    /// operation will make (V8 allocates: a scope). None for a method of
    /// another realm called on this one's object (as `V8Front`'s: the
    /// wrapper is made after, in the node's relevant realm).
    #[inline(never)]
    fn prepare_new_node(&mut self, interface: usize) -> Option<(v8::Local<'s, v8::Object>, usize)> {
        let this = K::this_object(&self.info);
        self.info.with_scope(|mut scope, _| {
            let current = scope.scope().get_current_context();
            if this
                .get_creation_context(scope.scope())
                .is_some_and(|context| context != current)
            {
                return None;
            }
            Some((templates::new_wrapper(&mut scope, interface), interface))
        })
    }

    /// The interface `this` is of, read in an operation of its own (no
    /// safepoint), then its wrapper made.
    fn prepare_node_like_this(&mut self) -> Option<(v8::Local<'s, v8::Object>, usize)> {
        let interface = self.read(|cx, op| {
            op.this_as::<Node>(cx)
                .map(|node| front::interface_of(cx, node))
        })?;
        self.prepare_new_node(interface)
    }

    /// The name only peeked at, with the isolate alone.
    #[inline(always)]
    fn foresee_element(&mut self, name: &ArgString<'s>) -> (Option<NameId>, usize) {
        front::foresee_element(self.info.isolate(), name)
    }

    /// A number that is no Smi: a heap number, which V8's return value
    /// makes (Blink's `bindings::V8SetReturnValue` of a double): nothing
    /// else of V8's is called.
    #[inline]
    fn finish_number(&mut self, number: f64) {
        self.info.return_number(number);
    }

    /// An object with no wrapper: one made now.
    #[cold]
    #[inline(never)]
    fn finish_object(&mut self, (object, interface): (ObjectRoot, usize)) {
        self.with_front(|front| front.finish_kept_as(front::Kept::Object(object, interface)));
    }

    /// A callback is its argument's handle, as the call holds it: nothing
    /// is kept for it. Any object (a function, or one with `handleEvent`,
    /// read when it is called); none for null or undefined; anything else
    /// the whole front end's `TypeError`.
    #[inline(always)]
    fn callback_arg(
        &mut self,
        index: usize,
        name: &str,
    ) -> Option<Option<Callback<v8::Local<'s, v8::Value>>>> {
        let value = K::arg(&self.info, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if value.is_object() {
            return Some(Some(Callback(value)));
        }
        self.with_front(|front| front.callback_arg(index, name).map(|_| ()))?;
        None
    }

    /// A function; anything else the whole front end's `TypeError`.
    #[inline(always)]
    fn function_arg(
        &mut self,
        index: usize,
        name: &str,
    ) -> Option<Callback<v8::Local<'s, v8::Value>>> {
        let value = K::arg(&self.info, index);
        if value.is_function() {
            return Some(Callback(value));
        }
        self.with_front(|front| front.function_arg(index, name).map(|_| ()))?;
        None
    }

    /// A function, none for null or undefined.
    #[inline(always)]
    fn nullable_function_arg(
        &mut self,
        index: usize,
        name: &str,
    ) -> Option<Option<Callback<v8::Local<'s, v8::Value>>>> {
        let value = K::arg(&self.info, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if value.is_function() {
            return Some(Some(Callback(value)));
        }
        self.with_front(|front| front.nullable_function_arg(index, name).map(|_| ()))?;
        None
    }

    /// Any object (callable or not: `[LegacyTreatNonObjectAsNull]`),
    /// anything else null.
    #[inline(always)]
    fn event_handler_arg(&mut self, index: usize) -> Option<Callback<v8::Local<'s, v8::Value>>> {
        let value = K::arg(&self.info, index);
        value.is_object().then_some(Callback(value))
    }

    /// A function as it is, anything else its `ToString` (which may run
    /// script, before the operation: a scope).
    #[inline(always)]
    fn timer_handler_arg(&mut self, index: usize) -> Option<Callback<v8::Local<'s, v8::Value>>> {
        let value = K::arg(&self.info, index);
        if value.is_function() {
            return Some(Callback(value));
        }
        self.info
            .with_scope(|mut scope, _| value.to_string(scope.scope()))
            .map(|string| Callback(string.into()))
    }

    #[inline(always)]
    fn is_dictionary_like(&mut self, index: usize) -> bool {
        let value = K::arg(&self.info, index);
        value.is_null_or_undefined() || value.is_object()
    }

    /// A dictionary argument: none for null or undefined (most: its
    /// defaults), an object as it is; anything else the whole front end's
    /// `TypeError`.
    #[inline(always)]
    fn dict_arg(&mut self, index: usize, name: &str) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = K::arg(&self.info, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
            return Some(Some(object));
        }
        self.with_front(|front| front.dict_arg(index, name))
    }

    // The results V8 allocates for: made by the whole front end, in a
    // scope.

    fn finish_objects(&mut self, objects: Vec<(ObjectRoot, usize)>) {
        self.with_front(|front| front.finish_objects(objects));
    }

    fn finish_frozen_objects(&mut self, objects: Vec<(ObjectRoot, usize)>) {
        self.with_front(|front| front.finish_frozen_objects(objects));
    }

    fn finish_strings(&mut self, strings: Vec<DomString>) {
        self.with_front(|front| front.finish_strings(strings));
    }

    fn finish_frozen_strings(&mut self, strings: Vec<DomString>) {
        self.with_front(|front| front.finish_frozen_strings(strings));
    }

    fn finish_doubles(&mut self, numbers: Vec<f64>) {
        self.with_front(|front| front.finish_doubles(numbers));
    }

    fn finish_numbers(&mut self, numbers: Vec<f64>) {
        self.with_front(|front| front.finish_numbers(numbers));
    }

    fn finish_dictionary(&mut self, members: Vec<(&'static str, dom_api::typed::DictValue)>) {
        self.with_front(|front| front.finish_dictionary(members));
    }

    fn finish_records(&mut self, records: Vec<Vec<(String, dom_api::typed::DictValue)>>) {
        self.with_front(|front| front.finish_records(records));
    }

    fn finish_entry_detail(&mut self, entry: Persistent<dom::PerformanceEntry>) {
        self.with_front(|front| front.finish_entry_detail(entry));
    }

    fn finish_reader_buffer(&mut self, reader: Persistent<dom::EventTargetImpl>) {
        self.with_front(|front| front.finish_reader_buffer(reader));
    }

    fn finish_history_state(&mut self, history: Persistent<dom::History>) {
        self.with_front(|front| front.finish_history_state(history));
    }

    fn finish_cloned(&mut self, payload: dom::SerializedMessage, ports: Vec<(ObjectRoot, usize)>) {
        self.with_front(|front| front.finish_cloned(payload, ports));
    }

    fn finish_resolved(&mut self, object: ObjectRoot, interface: usize) {
        self.with_front(|front| front.finish_resolved(object, interface));
    }

    fn finish_resolved_string(&mut self, string: DomString) {
        self.with_front(|front| front.finish_resolved_string(string));
    }

    fn finish_resolved_json(&mut self, text: DomString) {
        self.with_front(|front| front.finish_resolved_json(text));
    }

    fn finish_resolved_undefined(&mut self) {
        self.with_front(|front| front.finish_resolved_undefined());
    }

    fn finish_resolved_bytes(&mut self, bytes: Vec<u8>, view: bool) {
        self.with_front(|front| front.finish_resolved_bytes(bytes, view));
    }

    fn finish_animation_promise(
        &mut self,
        object: Persistent<dom::EventTargetImpl>,
        interface: usize,
        ready: bool,
        resolved: bool,
    ) {
        self.with_front(|front| front.finish_animation_promise(object, interface, ready, resolved));
    }

    fn finish_image_data_pixels(&mut self, image_data: Persistent<dom::ImageData>, bytes: Vec<u8>) {
        self.with_front(|front| front.finish_image_data_pixels(image_data, bytes));
    }

    fn finish_upgrading(&mut self, element: NodeRoot) {
        self.with_front(|front| front.finish_upgrading(element));
    }

    fn finish_pending_promise(&mut self, id: u64, existing: bool) {
        self.with_front(|front| front.finish_pending_promise(id, existing));
    }

    fn finish_resolved_constructor(
        &mut self,
        definition: Persistent<dom::CustomElementDefinition>,
    ) {
        self.with_front(|front| front.finish_resolved_constructor(definition));
    }

    /// A promise rejected with the error, by the whole front end.
    fn reject(&mut self, error: ApiError) {
        self.with_front(|front| front.reject(error));
    }

    /// An event handler's source compiled, by the whole front end.
    fn compile_event_handler(&mut self, target: &dom::TargetRoot, event_type: dom::NameId) {
        self.with_front(|front| front.compile_event_handler(target, event_type));
    }

    /// A constructor's call: with `new` and enough arguments (the checks
    /// in place); else the whole front end's `TypeError`.
    #[inline(always)]
    fn construct(&mut self, interface: usize, required: usize) -> bool {
        // `[HTMLConstructor]`: `new.target` mustn't be `HTMLElement`
        // itself, which the whole front end tells (its realm's interface
        // object).
        if interface == const { dom_api::declare::interface_index("HTMLElement") } {
            return self.with_front(|front| front.construct(interface, required));
        }
        (!K::new_target(K::this(&self.info)).is_undefined() && K::length(&self.info) >= required)
            || self.with_front(|front| front.construct(interface, required))
    }

    /// The member asks for the current realm's window: found in the
    /// operation.
    #[inline(always)]
    fn prepare_window(&mut self) {
        self.window = true;
    }

    /// The argument itself; null for undefined, which V8 gives a scope.
    #[inline(always)]
    fn any_arg(&mut self, index: usize) -> AnyArg<v8::Local<'s, v8::Value>> {
        let value = K::arg(&self.info, index);
        if value.is_undefined() {
            return self.with_front(|front| front.any_arg(index));
        }
        AnyArg(value)
    }

    /// The argument itself; a new `AbortError` for undefined, made by
    /// the whole front end.
    #[inline(always)]
    fn abort_reason_arg(&mut self, index: usize) -> Option<AnyArg<v8::Local<'s, v8::Value>>> {
        let value = K::arg(&self.info, index);
        if value.is_undefined() {
            return self.with_front(|front| front.abort_reason_arg(index));
        }
        Some(AnyArg(value))
    }

    /// None with no more arguments (most calls: `setTimeout(f, 0)`);
    /// else a new array of them, made by the whole front end.
    #[inline(always)]
    fn rest_arg(&mut self, index: usize) -> Option<v8::Local<'s, v8::Array>> {
        if K::length(&self.info) <= index {
            return None;
        }
        self.with_front(|front| front.rest_arg(index))
    }

    // A dictionary's members are read by the whole front end (a getter
    // may run script): what it gives is a value, or the member's value
    // itself (a handle of the callback's own handle scope, which the
    // front end's scope leaves it in).
    fn dict_any(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<AnyArg<v8::Local<'s, v8::Value>>>> {
        self.with_front(|front| front.dict_any(dict, member))
    }

    fn dict_object<T: elise::HeapType>(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
        interface: &str,
    ) -> Option<Option<v8::Local<'s, v8::Object>>> {
        self.with_front(|front| front.dict_object::<T>(dict, member, interface))
    }

    // The arguments read whole into the DOM's values before the
    // operation (a sequence, a dictionary of its own shape, a structured
    // serialization, keyframes): reading one allocates and may run script,
    // so each is the whole front end's, in a scope.
    fn serialized_state_arg(&mut self, index: usize) -> Option<Vec<u8>> {
        self.with_front(|front| front.serialized_state_arg(index))
    }

    fn message_arg(
        &mut self,
        index: usize,
        transfer: dom_api::typed::TransferArg,
        from_port: bool,
    ) -> Option<(dom::SerializedMessage, Vec<v8::Local<'s, v8::Object>>)> {
        self.with_front(|front| front.message_arg(index, transfer, from_port))
    }

    fn static_range_init(&mut self, index: usize) -> Option<[(v8::Local<'s, v8::Object>, u32); 2]> {
        self.with_front(|front| front.static_range_init(index))
    }

    fn intersection_observer_init(
        &mut self,
        index: usize,
    ) -> Option<(
        dom_api::typed::IntersectionInit,
        Option<v8::Local<'s, v8::Object>>,
    )> {
        self.with_front(|front| front.intersection_observer_init(index))
    }

    fn intersection_entry_init(
        &mut self,
        index: usize,
    ) -> Option<(
        dom_api::typed::IntersectionEntryInitValues,
        v8::Local<'s, v8::Object>,
    )> {
        self.with_front(|front| front.intersection_entry_init(index))
    }

    fn byte_string_pairs_arg(&mut self, index: usize) -> Option<dom_api::typed::StringPairsArg> {
        self.with_front(|front| front.byte_string_pairs_arg(index))
    }

    fn json_arg(&mut self, index: usize) -> Option<String> {
        self.with_front(|front| front.json_arg(index))
    }

    fn doubles_arg(&mut self, index: usize) -> Option<Vec<f64>> {
        self.with_front(|front| front.doubles_arg(index))
    }

    fn get_html_options(&mut self, index: usize) -> Option<(bool, Vec<v8::Local<'s, v8::Object>>)> {
        self.with_front(|front| front.get_html_options(index))
    }

    fn round_rect_radii_arg(&mut self, index: usize) -> Option<Vec<dom_api::typed::Radius>> {
        self.with_front(|front| front.round_rect_radii_arg(index))
    }

    fn animation_options_arg(&mut self, index: usize) -> Option<dom_api::typed::AnimationOptions> {
        self.with_front(|front| front.animation_options_arg(index))
    }

    fn effect_options_arg(&mut self, index: usize) -> Option<dom_api::typed::AnimationOptions> {
        self.with_front(|front| front.effect_options_arg(index))
    }

    fn optional_timing_arg(&mut self, index: usize) -> Option<animation::OptionalTiming> {
        self.with_front(|front| front.optional_timing_arg(index))
    }

    fn timeline_arg(&mut self, index: usize) -> Option<bool> {
        self.with_front(|front| front.timeline_arg(index))
    }

    fn keyframes_arg(&mut self, index: usize) -> Option<Option<animation::ScriptKeyframes>> {
        self.with_front(|front| front.keyframes_arg(index))
    }

    fn clone_arg(
        &mut self,
        index: usize,
    ) -> Option<(dom::SerializedMessage, Vec<v8::Local<'s, v8::Object>>)> {
        self.with_front(|front| front.clone_arg(index))
    }

    fn string_pairs_arg(&mut self, index: usize) -> Option<dom_api::typed::StringPairsArg> {
        self.with_front(|front| front.string_pairs_arg(index))
    }

    fn blob_parts_arg(
        &mut self,
        index: usize,
    ) -> Option<Option<Vec<dom_api::typed::BlobPartArg<v8::Local<'s, v8::Object>>>>> {
        self.with_front(|front| front.blob_parts_arg(index))
    }

    fn array_view_arg(&mut self, index: usize) -> Option<dom_api::typed::ArrayViewArg> {
        self.with_front(|front| front.array_view_arg(index))
    }

    fn dict_headers(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<dom_api::typed::StringPairsArg>> {
        self.with_front(|front| front.dict_headers(dict, member))
    }

    fn dict_is_null(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<bool>> {
        self.with_front(|front| front.dict_is_null(dict, member))
    }

    fn dict_serialized(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<Vec<u8>>> {
        self.with_front(|front| front.dict_serialized(dict, member))
    }

    fn objects_arg<T: elise::HeapType>(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Vec<v8::Local<'s, v8::Object>>> {
        self.with_front(|front| front.objects_arg::<T>(index, interface))
    }

    /// A wrapper is told by its type's tag: no scope.
    #[inline]
    fn object_if<T: elise::HeapType>(&mut self, index: usize) -> Option<v8::Local<'s, v8::Object>> {
        let value = K::arg(&self.info, index);
        if bindings::is_wrapper::<T>(self.info.isolate(), value) {
            return v8::Local::<v8::Object>::try_from(value).ok();
        }
        None
    }

    /// A boolean itself: no scope.
    #[inline]
    fn bool_if(&mut self, index: usize) -> Option<bool> {
        let value = K::arg(&self.info, index);
        value.is_boolean().then(|| value.is_true())
    }

    /// A small integer is read from its handle; another number is the
    /// whole front end's.
    #[inline]
    fn number_if(&mut self, index: usize) -> Option<f64> {
        let value = K::arg(&self.info, index);
        if !value.is_number() {
            return None;
        }
        match v8::machopo::as_smi(value) {
            Some(number) => Some(f64::from(number)),
            None => self.with_front(|front| front.number_if(index)),
        }
    }

    /// The view is the first argument, as `array_view_arg` took it.
    fn finish_filled_view(&mut self, bytes: Vec<u8>) {
        self.with_front(|front| front.finish_filled_view(bytes));
    }

    fn dict_nullable_object<T: elise::HeapType>(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
        interface: &str,
    ) -> Option<Option<v8::Local<'s, v8::Object>>> {
        self.with_front(|front| front.dict_nullable_object::<T>(dict, member, interface))
    }

    fn dict_nullable_target(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<v8::Local<'s, v8::Object>>> {
        self.with_front(|front| front.dict_nullable_target(dict, member))
    }

    fn dict_body(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<dom_api::typed::BodyMember<v8::Local<'s, v8::Object>>>> {
        self.with_front(|front| front.dict_body(dict, member))
    }

    fn dict_signal(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<dom_api::typed::SignalMember<v8::Local<'s, v8::Object>>>> {
        self.with_front(|front| front.dict_signal(dict, member))
    }

    fn body_arg(
        &mut self,
        index: usize,
    ) -> Option<Option<dom_api::typed::BodyArg<v8::Local<'s, v8::Object>>>> {
        self.with_front(|front| front.body_arg(index))
    }

    /// A typed array is no allocation to tell: no scope unless it isn't
    /// one (the `TypeError`).
    #[inline]
    fn uint8_clamped_arg(
        &mut self,
        index: usize,
    ) -> Option<dom_api::typed::Uint8ClampedArg<v8::Local<'s, v8::Value>>> {
        let value = K::arg(&self.info, index);
        match v8::Local::<v8::Uint8ClampedArray>::try_from(value) {
            Ok(array) => Some(dom_api::typed::Uint8ClampedArg {
                array: value,
                len: array.byte_length(),
            }),
            Err(_) => self.with_front(|front| front.uint8_clamped_arg(index)),
        }
    }

    fn dict_bool(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<bool>> {
        self.with_front(|front| front.dict_bool(dict, member))
    }

    fn dict_number(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<f64>> {
        self.with_front(|front| front.dict_number(dict, member))
    }

    fn dict_string(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<String>> {
        self.with_front(|front| front.dict_string(dict, member))
    }

    fn dict_nullable_string(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<String>> {
        self.with_front(|front| front.dict_nullable_string(dict, member))
    }

    fn dict_strings(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<Vec<String>>> {
        self.with_front(|front| front.dict_strings(dict, member))
    }

    fn dict_string_or_number(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: dom_api::typed::MemberName,
    ) -> Option<Option<dom_api::typed::StringOrNumber>> {
        self.with_front(|front| front.dict_string_or_number(dict, member))
    }

    #[inline(always)]
    fn arg_count(&mut self) -> usize {
        K::length(&self.info)
    }

    #[inline(always)]
    fn is_undefined(&mut self, index: usize) -> bool {
        K::arg(&self.info, index).is_undefined()
    }

    #[inline(always)]
    fn is_null_or_undefined(&mut self, index: usize) -> bool {
        K::arg(&self.info, index).is_null_or_undefined()
    }

    /// A boolean itself is read at once; anything else is `ToBoolean`'s.
    #[inline(always)]
    fn bool_arg(&mut self, index: usize) -> bool {
        let value = K::arg(&self.info, index);
        if value.is_true() {
            return true;
        }
        if value.is_false() {
            return false;
        }
        self.with_front(|front| front.bool_arg(index))
    }

    /// A small integer (most integer arguments) is read from its handle;
    /// anything else is `ToInt32`'s, which may run script.
    #[inline(always)]
    fn i32_arg(&mut self, index: usize) -> Option<i32> {
        let value = K::arg(&self.info, index);
        match v8::machopo::as_smi(value) {
            Some(value) => Some(value),
            // A missing optional one (`setTimeout(f)`): `ToInt32` of
            // undefined.
            None if value.is_undefined() => Some(0),
            None => self.with_front(|front| front.i32_arg(index)),
        }
    }

    /// As `i32_arg`: a negative small integer wraps, as `ToUint32` does.
    #[inline(always)]
    fn u32_arg(&mut self, index: usize) -> Option<u32> {
        // An indexed getter's index is a number already.
        if let Some(number) = K::number_arg(&self.info, index) {
            return Some(number);
        }
        let value = K::arg(&self.info, index);
        match v8::machopo::as_smi(value) {
            Some(value) => Some(value.cast_unsigned()),
            None if value.is_undefined() => Some(0),
            None => self.with_front(|front| front.u32_arg(index)),
        }
    }

    /// A number is read with no conversion (a small integer from its
    /// handle, another by V8's `Number::Value`); anything else is
    /// `ToNumber`'s, which may run script, and what isn't finite where it
    /// must be is the whole front end's `TypeError`.
    #[inline(always)]
    fn f64_arg(&mut self, index: usize, finite: bool) -> Option<f64> {
        let value = K::arg(&self.info, index);
        let number = match v8::machopo::as_smi(value) {
            Some(value) => Some(f64::from(value)),
            None => v8::Local::<v8::Number>::try_from(value)
                .ok()
                .map(|number| number.value()),
        };
        match number {
            Some(number) if !finite || number.is_finite() => Some(number),
            _ => self.with_front(|front| front.f64_arg(index, finite)),
        }
    }

    /// An object of the DOM's of type `T`, checked with the isolate
    /// alone; anything else is the whole front end's `TypeError`.
    #[inline(always)]
    fn object_arg<T: elise::HeapType>(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = K::arg(&self.info, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if bindings::is_wrapper::<T>(self.info.isolate(), value) {
            return v8::Local::<v8::Object>::try_from(value).ok().map(Some);
        }
        self.with_front(|front| front.object_arg::<T>(index, interface))
    }

    #[inline(always)]
    fn target_arg(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = K::arg(&self.info, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        let isolate = self.info.isolate();
        if bindings::is_wrapper::<Node>(isolate, value)
            || bindings::is_wrapper::<Window>(isolate, value)
            || bindings::is_wrapper::<EventTargetImpl>(isolate, value)
        {
            return v8::Local::<v8::Object>::try_from(value).ok().map(Some);
        }
        self.with_front(|front| front.target_arg(index, interface))
    }

    #[cold]
    #[inline(never)]
    fn throw(&mut self, error: ApiError) {
        // What the operation left before it failed isn't the result.
        self.with_front(|front| front.throw(error));
    }

    /// The reactions' callbacks are script: the whole front end's, which
    /// is their script host.
    #[cold]
    #[inline(never)]
    fn invoke_ce_reactions(&mut self) {
        self.with_front(|front| front.invoke_ce_reactions());
    }
}

impl<'s, K: InfoKind<'s>> OpFront for InfoOp<'_, 's, K> {
    #[inline(always)]
    fn str_param<'a>(&mut self, string: &'a ArgString<'s>) -> &'a str {
        string.text_lent(&mut self.no_gc)
    }

    #[inline(always)]
    fn this_as<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
    ) -> Option<elise::Gc<'gc, T>> {
        K::this_as(&mut self.no_gc, cx, self.this)
    }

    /// The call's new object made the object's wrapper (V8 returns it).
    #[inline(always)]
    fn construct_object<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, object: dom::ObjectRef<'gc>) {
        front::wrap_new_object(&mut self.no_gc, cx, K::new_object(self.this), object);
        front::count(&self.caches.wrappers_made);
    }

    /// A new element, or one being upgraded that has no wrapper: the
    /// call's new object made its. One that has a wrapper: that given
    /// as the call's result now (from its edge's handle), its prototype
    /// set after the operation.
    fn construct_html<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        constructed: dom_api::HtmlConstructed<'gc>,
    ) -> Option<NodeRoot> {
        let element = match constructed {
            dom_api::HtmlConstructed::New(element) => element,
            dom_api::HtmlConstructed::Upgrading(element) => {
                if self.no_gc.return_wrapper(cx.wrapper_edge(element)) {
                    return Some(cx.root(element));
                }
                element
            }
        };
        self.construct_object(cx, dom::ObjectRef::Node(element));
        None
    }

    /// `new.target`'s number (V8's identity hash), if it is an object.
    #[inline(always)]
    fn new_target_number(&mut self) -> Option<u32> {
        v8::Local::<v8::Object>::try_from(K::new_target(self.this))
            .ok()
            .map(front::number)
    }

    /// Compared in place, no local made of the edge's.
    #[inline(always)]
    fn is_new_target(&mut self, _: &ApiCx<'_, '_>, edge: &JsEdge) -> bool {
        self.no_gc.refers_to(edge, K::new_target(self.this))
    }

    /// In place, unless V8 keeps the bytes on its heap (a small array):
    /// copied to the stack then. The array's handle is the callback's own
    /// handle scope's.
    fn with_array_bytes(
        &mut self,
        _: &ApiCx<'_, '_>,
        edge: &JsEdge,
        f: &mut dyn FnMut(Option<&[u8]>),
    ) {
        let array = self
            .no_gc
            .strong_value(edge)
            .and_then(|value| v8::Local::<v8::Uint8ClampedArray>::try_from(value).ok());
        match array {
            Some(array) => {
                let mut storage = [0; v8::TYPED_ARRAY_MAX_SIZE_IN_HEAP];
                f(Some(array.get_contents(&mut storage)));
            }
            None => f(None),
        }
    }

    /// The current realm's window, of a member that asked for it
    /// (`InfoFront::prepare_window`): its context's global, read from the
    /// isolate now; none if that isn't a window of the DOM's.
    #[inline(always)]
    fn current_window<'gc>(&mut self, cx: &ApiCx<'_, 'gc>) -> Option<elise::Gc<'gc, Window>> {
        if !self.window {
            return None;
        }
        let global = self.no_gc.current_global();
        self.no_gc.unwrap::<Window>(cx.gc, global)
    }

    #[inline(always)]
    fn node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: &v8::Local<'s, v8::Object>) -> NodeRef<'gc> {
        self.no_gc
            .unwrap::<Node>(cx.gc, *node)
            .expect("a wrapper, checked as it was taken")
    }

    #[inline(always)]
    fn object<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: &v8::Local<'s, v8::Object>,
    ) -> elise::Gc<'gc, T> {
        self.no_gc
            .unwrap::<T>(cx.gc, *object)
            .expect("a wrapper, checked as it was taken")
    }

    #[inline(always)]
    fn target<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: &v8::Local<'s, v8::Object>,
    ) -> TargetRef<'gc> {
        if let Some(node) = self.no_gc.unwrap::<Node>(cx.gc, *object) {
            return TargetRef::Node(node);
        }
        if let Some(window) = self.no_gc.unwrap::<Window>(cx.gc, *object) {
            return TargetRef::Window(window);
        }
        TargetRef::EventTarget(
            self.no_gc
                .unwrap::<EventTargetImpl>(cx.gc, *object)
                .expect("an event target, checked as it was taken"),
        )
    }

    /// Whether the edge's object is the callback: compared in place, no
    /// local made of the edge's.
    #[inline(always)]
    fn same_callback(
        &mut self,
        _: &ApiCx<'_, '_>,
        edge: &JsEdge,
        callback: Callback<v8::Local<'s, v8::Value>>,
    ) -> bool {
        self.no_gc.refers_to(edge, callback.0)
    }

    /// An object as an object's strong edge, a timer's string as any
    /// value's.
    #[inline(always)]
    fn store_callback(
        &mut self,
        _: &ApiCx<'_, '_>,
        edge: &JsEdge,
        callback: Callback<v8::Local<'s, v8::Value>>,
    ) {
        match v8::Local::<v8::Object>::try_from(callback.0) {
            Ok(object) => self.no_gc.set_strong(edge, object),
            Err(_) => self.no_gc.set_strong_value(edge, callback.0),
        }
    }

    #[inline(always)]
    fn store_any(
        &mut self,
        _: &ApiCx<'_, '_>,
        edge: &JsEdge,
        value: AnyArg<v8::Local<'s, v8::Value>>,
    ) {
        self.no_gc.set_strong_value(edge, value.0);
    }

    fn store_rest(&mut self, _: &ApiCx<'_, '_>, edge: &JsEdge, rest: v8::Local<'s, v8::Array>) {
        self.no_gc.set_strong_value(edge, rest.into());
    }

    /// The engine's number for the callback (V8's identity hash).
    fn callback_number(&mut self, callback: Callback<v8::Local<'s, v8::Value>>) -> u32 {
        v8::Local::<v8::Object>::try_from(callback.0).map_or(0, front::number)
    }

    // V8's `undefined` is the return value until set.
    fn ret_undefined(&mut self) {}

    #[inline(always)]
    fn ret_null(&mut self) {
        K::note_null(self.noted);
        self.no_gc.ret().set_null();
    }

    /// The value as it is, from the edge's own handle (no local made);
    /// undefined if none.
    #[inline(always)]
    fn ret_any<'gc>(&mut self, _: &ApiCx<'_, 'gc>, edge: &'gc JsEdge) {
        if !self.no_gc.return_value(edge) {
            self.no_gc.ret().set_undefined();
        }
    }

    #[inline(always)]
    fn ret_any_or_null<'gc>(&mut self, _: &ApiCx<'_, 'gc>, edge: &'gc JsEdge) {
        if !self.no_gc.return_value(edge) {
            self.no_gc.ret().set_null();
        }
    }

    #[inline(always)]
    fn ret_any_or_undefined<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, edge: Option<&'gc JsEdge>) {
        match edge {
            Some(edge) => self.ret_any(cx, edge),
            None => self.no_gc.ret().set_undefined(),
        }
    }

    #[inline(always)]
    fn ret_bool(&mut self, value: bool) {
        if K::note_bool(self.noted, value) {
            self.no_gc.ret().set_bool(value);
        }
    }

    /// A Smi's at once; past one a heap number, made after the operation.
    #[inline(always)]
    fn ret_u32(&mut self, value: u32) -> Option<f64> {
        (!self.no_gc.ret().set_smi_u32(value)).then(|| value.into())
    }

    #[inline(always)]
    fn ret_i32(&mut self, value: i32) -> Option<f64> {
        (!self.no_gc.ret().set_smi_i32(value)).then(|| value.into())
    }

    /// A Smi's at once (an integer in its range, not -0); another a heap
    /// number, made after the operation.
    #[inline(always)]
    fn ret_f64(&mut self, value: f64) -> Option<f64> {
        let integer = value.fract() == 0.0
            && value.abs() <= f64::from(i32::MAX)
            && !(value == 0.0 && value.is_sign_negative());
        let given = integer
            && self
                .no_gc
                .ret()
                .set_smi_i32(base::numerics::saturated_cast::<i32, f64>(value));
        (!given).then_some(value)
    }

    /// The string's cached V8 string, returned from the cache's weak
    /// handle; else made after the operation.
    #[inline(always)]
    fn ret_string(&mut self, value: &DomString) -> Option<PendingString> {
        // V8's own empty string, whatever string of the DOM's it is (one
        // made for the call is no entry of the cache).
        if value.is_empty() {
            K::note_empty(self.noted);
            self.no_gc.ret().set_empty_string();
            return None;
        }
        let returned = self
            .caches
            .strings
            .find(value)
            .is_some_and(|weak| self.no_gc.return_weak(weak));
        (!returned).then(|| PendingString::String(value.clone()))
    }

    /// The text itself is carried out of the operation: no string of the
    /// DOM's is made of it, none looked up in the cache.
    #[inline(always)]
    fn ret_fresh_string(&mut self, value: std::borrow::Cow<'static, str>) -> Option<PendingString> {
        if value.is_empty() {
            K::note_empty(self.noted);
            self.no_gc.ret().set_empty_string();
            return None;
        }
        front::count(&self.caches.fresh_strings);
        Some(PendingString::Fresh(value))
    }

    /// The name's V8 string, returned from its global; else made after
    /// the operation.
    #[inline(always)]
    fn ret_name(&mut self, id: NameId) -> Option<PendingString> {
        // The names table's empty string: V8's own.
        if id == names::NONE {
            K::note_empty(self.noted);
            self.no_gc.ret().set_empty_string();
            return None;
        }
        match usize::try_from(id)
            .ok()
            .and_then(|index| self.caches.names.get(index))
            .and_then(Option::as_ref)
        {
            Some(string) => {
                self.no_gc.return_global(string);
                None
            }
            None => Some(PendingString::Name(id, names::string(id))),
        }
    }

    /// The node's wrapper, written from the node's reference to it; else
    /// made after the operation.
    #[inline(always)]
    fn ret_node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> Option<PendingNode> {
        if self.no_gc.return_wrapper(cx.wrapper_edge(node)) {
            return None;
        }
        Some(self.root_node(cx, node))
    }

    /// The object's wrapper, written from the object's reference to it;
    /// else made after the operation.
    #[inline(always)]
    fn ret_object<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: ObjectRef<'gc>,
    ) -> Option<(ObjectRoot, usize)> {
        if self.no_gc.return_wrapper(cx.object_wrapper_edge(object)) {
            return None;
        }
        Some(self.root_object(cx, object))
    }

    /// A node its member made: the wrapper made ahead is its, if it is of
    /// the node's interface (most), with no root and no operation after;
    /// else as any node (one of another interface than foreseen, or none
    /// made ahead: its wrapper is made after).
    #[inline(always)]
    fn ret_new_node<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        node: NodeRef<'gc>,
        prepared: Option<(v8::Local<'s, v8::Object>, usize)>,
    ) -> Option<PendingNode> {
        match prepared {
            Some((wrapper, interface)) if interface == front::interface_of(cx, node) => {
                self.no_gc.wrap(cx.gc, wrapper, node);
                self.no_gc.set_wrapper(cx.wrapper_edge(node), wrapper);
                front::count(&self.caches.wrappers_made);
                front::count(&self.caches.wrappers_ahead);
                self.no_gc.ret().set(wrapper.into());
                None
            }
            prepared => {
                if prepared.is_some() {
                    front::count(&self.caches.wrappers_wasted);
                }
                self.ret_node(cx, node)
            }
        }
    }

    /// A custom element to construct after the operation (its
    /// constructor is script).
    #[cold]
    #[inline(never)]
    fn ret_construct<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        definition: elise::Gc<'gc, dom::CustomElementDefinition>,
        document: NodeRef<'gc>,
    ) -> Option<PendingNode> {
        Some(PendingNode::Construct(
            Persistent::new(cx.gc, definition),
            cx.root(document),
        ))
    }
}

impl<'s, K: InfoKind<'s>> InfoOp<'_, 's, K> {
    /// As `root_node`, an object of any type.
    #[cold]
    #[inline(never)]
    fn root_object<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: ObjectRef<'gc>,
    ) -> (ObjectRoot, usize) {
        (cx.root_object(object), dom_api::interface_of(cx, object))
    }

    /// A node with no wrapper, rooted for after the operation, and its
    /// interface: out of line, as `front::V8Op`'s (the interface's lookup).
    #[cold]
    #[inline(never)]
    fn root_node<'gc>(&mut self, cx: &dom::Cx<'_, 'gc>, node: NodeRef<'gc>) -> PendingNode {
        PendingNode::Node(cx.root(node), front::interface_of(cx, node))
    }
}
