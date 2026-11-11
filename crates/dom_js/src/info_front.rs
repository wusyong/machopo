//! A member's call through V8's callback info alone (Blink's generated
//! callbacks take `v8::FunctionCallbackInfo` and keep the rest in locals):
//! the front end of the members whose arguments are strings and nodes and
//! whose results are numbers, booleans, strings, nodes, null or nothing
//! (`dom_api::TypedMember::PLAIN`), which most calls are.
//!
//! It holds the info and one result left for after the operation, nothing
//! made for a call that returns at once: no scope of rusty_v8's, no
//! `front::Call`. The isolate, the receiver, the arguments and the result
//! are read from the info where they are used. What allocates in V8,
//! throws or runs script (an argument to convert or copy, a string V8 has
//! no copy of, a node with no wrapper, an error, an element's reactions)
//! makes a scope and the whole front end (`front::V8Front`) then, out of
//! line, and is done by it.

use std::mem::ManuallyDrop;

use dom::{DomString, NameId, Node, NodeRef, NodeRoot, names};
use dom_api::{ApiCx, ApiError, Front, FrontTypes, OpFront};

use crate::front::{self, Checked, V8Front};
use crate::state::{self, Caches, State};
use crate::strings::ArgString;

/// A result the operation couldn't give V8 (it would allocate): what
/// `InfoFront::finish` gives it after.
enum Pending {
    /// A number past a Smi.
    Number(f64),
    /// A string V8 has no copy of.
    String(DomString),
    /// A name's string, not made yet.
    Name(NameId, DomString),
    /// A node with no wrapper, and its interface.
    Node(NodeRoot, usize),
}

/// A plain member's call out of its operation.
pub(crate) struct InfoFront<'s> {
    info: bindings::StateInfo<'s, State>,
    /// Dropped by `end`, out of line and only if set: most calls leave
    /// none, and the drop glue is a call each would make (as
    /// `front::Call`'s boxes).
    pending: ManuallyDrop<Option<Pending>>,
}

/// A plain member's call in its operation: what of V8 it may use and the
/// result (`InfoNoGc`), the receiver, where a result that must wait goes,
/// and the state's caches it reads.
pub(crate) struct InfoOp<'o, 's> {
    no_gc: bindings::InfoNoGc<'o, 's>,
    this: bindings::Receiver<'s>,
    pending: &'o mut Option<Pending>,
    caches: &'o Caches,
}

impl<'s> FrontTypes for InfoFront<'s> {
    type Str = ArgString<'s>;
    type Node = v8::Local<'s, v8::Object>;
    type Dict = v8::Local<'s, v8::Object>;
}

impl<'s> FrontTypes for InfoOp<'_, 's> {
    type Str = ArgString<'s>;
    type Node = v8::Local<'s, v8::Object>;
    type Dict = v8::Local<'s, v8::Object>;
}

impl<'s> InfoFront<'s> {
    #[inline(always)]
    pub(crate) fn new(info: bindings::StateInfo<'s, State>) -> Self {
        // As `V8Front::new`'s: every way into script counts itself.
        debug_assert!(
            bindings::trace::in_script(),
            "a DOM member is called from script that the engine didn't count as entered"
        );
        Self {
            info,
            pending: ManuallyDrop::new(None),
        }
    }

    /// The call's end: nothing to drop unless a result was left and never
    /// finished (a call that failed after it), looked at inline (the drop
    /// glue is a call every member's would make).
    #[inline(always)]
    pub(crate) fn end(&mut self) {
        if self.pending.is_some() {
            self.drop_pending();
        }
    }

    #[cold]
    #[inline(never)]
    fn drop_pending(&mut self) {
        drop(self.pending.take());
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

    #[cold]
    #[inline(never)]
    fn finish_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.with_front(|front| {
            front.finish_kept_as(match pending {
                Pending::Number(number) => front::Kept::F64(number),
                Pending::String(string) => front::Kept::String(string),
                Pending::Name(id, name) => front::Kept::Name(id, name),
                Pending::Node(node, interface) => front::Kept::Node(node, interface),
            });
        });
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
            pending: &mut self.pending,
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
            pending: &mut self.pending,
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

    #[inline(always)]
    fn finish(&mut self) {
        if self.pending.is_some() {
            self.finish_pending();
        }
    }

    #[cold]
    #[inline(never)]
    fn throw(&mut self, error: ApiError) {
        // What the operation left before it failed isn't the result.
        // A member's function fails before its result is given.
        debug_assert!(self.pending.is_none(), "a failed call left a result");
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
    fn ret_u32(&mut self, value: u32) {
        if !self.no_gc.ret().set_smi_u32(value) {
            *self.pending = Some(Pending::Number(value.into()));
        }
    }

    #[inline(always)]
    fn ret_i32(&mut self, value: i32) {
        if !self.no_gc.ret().set_smi_i32(value) {
            *self.pending = Some(Pending::Number(value.into()));
        }
    }

    /// A Smi's at once (an integer in its range, not -0); another a heap
    /// number, made after the operation.
    #[inline(always)]
    fn ret_f64(&mut self, value: f64) {
        let integer = value.fract() == 0.0
            && value.abs() <= f64::from(i32::MAX)
            && !(value == 0.0 && value.is_sign_negative());
        if !(integer
            && self
                .no_gc
                .ret()
                .set_smi_i32(base::numerics::saturated_cast::<i32, f64>(value)))
        {
            *self.pending = Some(Pending::Number(value));
        }
    }

    /// The string's cached V8 string, returned from the cache's weak
    /// handle; else made after the operation.
    #[inline(always)]
    fn ret_string(&mut self, value: &DomString) {
        let returned = self
            .caches
            .strings
            .find(value)
            .is_some_and(|weak| self.no_gc.return_weak(weak));
        if !returned {
            *self.pending = Some(Pending::String(value.clone()));
        }
    }

    /// The name's V8 string, returned from its global; else made after
    /// the operation.
    #[inline(always)]
    fn ret_name(&mut self, id: NameId) {
        match usize::try_from(id)
            .ok()
            .and_then(|index| self.caches.names.get(index))
            .and_then(Option::as_ref)
        {
            Some(string) => self.no_gc.return_global(string),
            None => *self.pending = Some(Pending::Name(id, names::string(id))),
        }
    }

    /// The node's wrapper, written from the node's reference to it; else
    /// made after the operation.
    #[inline(always)]
    fn ret_node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) {
        if !self.no_gc.return_wrapper(cx.wrapper_edge(node)) {
            self.keep_node(cx, node);
        }
    }
}

impl InfoOp<'_, '_> {
    /// A node with no wrapper, kept for after the operation: out of line,
    /// as `front::V8Op`'s (its interface's lookup).
    #[cold]
    #[inline(never)]
    fn keep_node<'gc>(&mut self, cx: &dom::Cx<'_, 'gc>, node: NodeRef<'gc>) {
        *self.pending = Some(Pending::Node(cx.root(node), front::interface_of(cx, node)));
    }
}
