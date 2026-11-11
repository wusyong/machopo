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

use dom::{
    DomString, EventTargetImpl, NameId, Node, NodeRef, NodeRoot, ObjectRef, ObjectRoot, TargetRef,
    Window, names,
};
use dom_api::{ApiCx, ApiError, Front, FrontTypes, OpFront};
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

/// A plain member's call out of its operation.
pub(crate) struct InfoFront<'s> {
    info: bindings::StateInfo<'s, State>,
}

/// A plain member's call in its operation: what of V8 it may use and the
/// result (`InfoNoGc`), the receiver, and the state's caches it reads.
pub(crate) struct InfoOp<'o, 's> {
    no_gc: bindings::InfoNoGc<'o, 's>,
    this: bindings::Receiver<'s>,
    caches: &'o Caches,
}

impl<'s> FrontTypes for InfoFront<'s> {
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
}

impl<'s> FrontTypes for InfoOp<'_, 's> {
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
}

impl<'s> InfoFront<'s> {
    #[inline(always)]
    pub(crate) fn new(info: bindings::StateInfo<'s, State>) -> Self {
        // As `V8Front::new`'s: every way into script counts itself.
        debug_assert!(
            bindings::trace::in_script(),
            "a DOM member is called from script that the engine didn't count as entered"
        );
        Self { info }
    }

    /// `f` of the whole front end, a scope made for it: what allocates,
    /// throws or runs script.
    #[cold]
    #[inline(never)]
    fn with_front<R>(
        &mut self,
        f: impl FnOnce(&mut V8Front<'_, 's, 's, 's, Checked<'s>>) -> R,
    ) -> R {
        let args = Checked {
            args: self.info.args(),
            this: self.info.receiver(),
        };
        self.info
            .with_scope(|scope, rv| f(&mut V8Front::new(scope, &args, rv)))
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

impl<'s> Front for InfoFront<'s> {
    type Op<'o>
        = InfoOp<'o, 's>
    where
        Self: 'o;

    #[inline(always)]
    fn read<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let this = self.info.receiver();
        let (state, no_gc) = self.info.state();
        let mut op = InfoOp {
            no_gc,
            this,
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
        let this = self.info.receiver();
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
            caches,
        };
        dom.mutate(|cx| f(&ApiCx::with_layout_cell(cx, layout), &mut op))
    }

    #[inline(always)]
    fn has_args(&mut self, name: &str, required: usize) -> bool {
        let given = usize::try_from(self.info.args().length()).unwrap_or(0);
        given >= required || self.with_front(|front| front.has_args(name, required))
    }

    /// A string that lends its text in place (most: an ASCII literal) is
    /// taken with the isolate alone.
    #[inline(always)]
    fn string_arg(&mut self, index: usize) -> Option<ArgString<'s>> {
        let value = front::arg(&self.info.args(), index);
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
        if front::arg(&self.info.args(), index).is_null_or_undefined() {
            return Some(None);
        }
        self.string_arg(index).map(Some)
    }

    #[inline(always)]
    fn legacy_null_string_arg(&mut self, index: usize) -> Option<Option<ArgString<'s>>> {
        if front::arg(&self.info.args(), index).is_null() {
            return Some(None);
        }
        self.string_arg(index).map(Some)
    }

    #[inline(always)]
    fn node_arg(&mut self, index: usize) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = front::arg(&self.info.args(), index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if bindings::is_wrapper::<Node>(self.info.isolate(), value) {
            return v8::Local::<v8::Object>::try_from(value).ok().map(Some);
        }
        // Not a node: the whole front end's `TypeError`.
        self.with_front(|front| front.node_arg(index))
    }

    /// A string V8 had no copy of: made (and cached) now.
    #[cold]
    #[inline(never)]
    fn finish_string(&mut self, pending: PendingString) {
        self.with_front(|front| {
            front.finish_kept_as(match pending {
                PendingString::String(string) => front::Kept::String(string),
                PendingString::Name(id, name) => front::Kept::Name(id, name),
            });
        });
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
        let this = self.info.receiver().object();
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

    /// A number that is no Smi: a heap number.
    #[cold]
    #[inline(never)]
    fn finish_number(&mut self, number: f64) {
        self.with_front(|front| front.finish_kept_as(front::Kept::F64(number)));
    }

    /// An object with no wrapper: one made now.
    #[cold]
    #[inline(never)]
    fn finish_object(&mut self, (object, interface): (ObjectRoot, usize)) {
        self.with_front(|front| front.finish_kept_as(front::Kept::Object(object, interface)));
    }

    #[inline(always)]
    fn arg_count(&mut self) -> usize {
        usize::try_from(self.info.args().length()).unwrap_or(0)
    }

    #[inline(always)]
    fn is_undefined(&mut self, index: usize) -> bool {
        front::arg(&self.info.args(), index).is_undefined()
    }

    #[inline(always)]
    fn is_null_or_undefined(&mut self, index: usize) -> bool {
        front::arg(&self.info.args(), index).is_null_or_undefined()
    }

    /// A boolean itself is read at once; anything else is `ToBoolean`'s.
    #[inline(always)]
    fn bool_arg(&mut self, index: usize) -> bool {
        let value = front::arg(&self.info.args(), index);
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
        match v8::machopo::as_smi(front::arg(&self.info.args(), index)) {
            Some(value) => Some(value),
            None => self.with_front(|front| front.i32_arg(index)),
        }
    }

    /// As `i32_arg`: a negative small integer wraps, as `ToUint32` does.
    #[inline(always)]
    fn u32_arg(&mut self, index: usize) -> Option<u32> {
        match v8::machopo::as_smi(front::arg(&self.info.args(), index)) {
            Some(value) => Some(value.cast_unsigned()),
            None => self.with_front(|front| front.u32_arg(index)),
        }
    }

    /// A number is read with no conversion (a small integer from its
    /// handle, another by V8's `Number::Value`); anything else is
    /// `ToNumber`'s, which may run script, and what isn't finite where it
    /// must be is the whole front end's `TypeError`.
    #[inline(always)]
    fn f64_arg(&mut self, index: usize, finite: bool) -> Option<f64> {
        let value = front::arg(&self.info.args(), index);
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
        let value = front::arg(&self.info.args(), index);
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
        let value = front::arg(&self.info.args(), index);
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

impl<'s> OpFront for InfoOp<'_, 's> {
    #[inline(always)]
    fn str_param<'a>(&mut self, string: &'a ArgString<'s>) -> &'a str {
        string.text_lent(&mut self.no_gc)
    }

    /// The receiver V8 checked against the function's signature: its map
    /// isn't read again, only its tag (`bindings::unwrap_receiver`).
    #[inline(always)]
    fn this_as<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
    ) -> Option<elise::Gc<'gc, T>> {
        self.no_gc.unwrap_receiver::<T>(cx.gc, self.this)
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

    // V8's `undefined` is the return value until set.
    fn ret_undefined(&mut self) {}

    #[inline(always)]
    fn ret_null(&mut self) {
        self.no_gc.ret().set_null();
    }

    #[inline(always)]
    fn ret_bool(&mut self, value: bool) {
        self.no_gc.ret().set_bool(value);
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
        let returned = self
            .caches
            .strings
            .find(value)
            .is_some_and(|weak| self.no_gc.return_weak(weak));
        (!returned).then(|| PendingString::String(value.clone()))
    }

    /// The name's V8 string, returned from its global; else made after
    /// the operation.
    #[inline(always)]
    fn ret_name(&mut self, id: NameId) -> Option<PendingString> {
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

impl InfoOp<'_, '_> {
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
